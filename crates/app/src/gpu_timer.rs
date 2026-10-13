// GPU-Zeit je Render-Pass ueber Timestamp-Queries -- fuer den Frame-Profiler.
//
// Die GPU schreibt am Anfang und Ende jedes gemessenen Passes einen Zeitstempel in ein
// QuerySet; am Frame-Ende werden sie in einen Puffer aufgeloest und in einen von mehreren
// Auslese-Puffern kopiert. Gelesen wird ASYNCHRON (map_async), ein bis zwei Frames spaeter --
// die CPU wartet dafuer nie auf die GPU. Genau das tat die bisherige Messung "GPU nach
// Present" (device.poll(Wait) nach jedem Frame): sie zwang CPU und GPU in den Gleichschritt
// und verfaelschte damit, was sie messen sollte.
//
// Innerhalb eines Passes lassen sich zusaetzlich Abschnitte markieren (mark, braucht
// TIMESTAMP_QUERY_INSIDE_PASSES): der Welt-Pass zeichnet Gelaende, Wasser, Entities usw.
// nacheinander, und erst die Abschnitte zeigen, wo seine Zeit hingeht. Die GPU arbeitet
// Zeichenaufrufe ueberlappend ab -- ein Abschnitt ist deshalb eine Naeherung, keine exakte
// Grenze; die Summe stimmt.
//
// Misst immer (die Gesamtzeit braucht die Leistungsanzeige); ohne Timestamp-Unterstuetzung des
// Adapters gibt es gar keinen GpuTimer (s. GpuState::new). Der Kopierweg in die Swapchain
// (copy_texture_to_texture) ist kein Pass und damit nicht erfasst -- er kostet kaum etwas.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Hoechstens so viele Zeitstempel je Frame (je Pass zwei, je Abschnitt einer). Mehr werden
// einfach nicht gemessen.
const MAX_STAMPS: usize = 64;
// Auslese-Puffer im Umlauf: die Ergebnisse kommen ein bis zwei Frames spaeter an, in der
// Zwischenzeit braucht jeder Frame einen eigenen. Ist keiner frei, wird der Frame nicht gemessen.
const SLOTS: usize = 4;
const BYTES: u64 = (MAX_STAMPS * 8) as u64;

// Ein gemessener Pass: Zeitstempel-Index fuer Anfang und Ende, dazwischen die Abschnitte
// (Name, Index) in Zeichenreihenfolge.
#[derive(Clone)]
struct PassRecord {
    label: &'static str,
    begin: u32,
    end: u32,
    marks: Vec<(&'static str, u32)>,
}

struct Slot {
    buffer: wgpu::Buffer,
    // Passes des Frames, der in diesem Puffer steht, und wie viele Zeitstempel er belegt.
    // Leer = Puffer frei.
    passes: Vec<PassRecord>,
    stamps: u32,
    // Von map_async gesetzt, sobald der Puffer lesbar ist.
    mapped: Arc<AtomicBool>,
    // map_async ist angefordert (nach dem Submit), Ergebnis steht noch aus.
    waiting: bool,
}

pub struct GpuTimer {
    query_set: Arc<wgpu::QuerySet>,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    // Nanosekunden je Zeitstempel-Takt (Queue::get_timestamp_period).
    period_ns: f32,
    // Passes dieses Frames; None = dieser Frame wird nicht gemessen.
    frame: Option<Vec<PassRecord>>,
    // Naechster freier Zeitstempel-Index in diesem Frame.
    next: u32,
    inside_passes: bool,
    // Slot, in den dieser Frame aufgeloest wurde und der nach dem Submit gemappt werden muss.
    submitted: Option<usize>,
}

impl GpuTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu timer queries"),
            ty: wgpu::QueryType::Timestamp,
            count: MAX_STAMPS as u32,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu timer resolve"),
            size: BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("gpu timer readback"),
                    size: BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                passes: Vec::new(),
                stamps: 0,
                mapped: Arc::new(AtomicBool::new(false)),
                waiting: false,
            })
            .collect();
        let inside_passes = device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
        Some(Self { query_set: Arc::new(query_set), resolve, slots, period_ns: queue.get_timestamp_period(), frame: None, next: 0, inside_passes, submitted: None })
    }

    // Vor dem ersten Pass eines Frames. Gemessen wird, wenn ein Auslese-Puffer frei ist.
    pub fn begin_frame(&mut self) {
        let free = self.slots.iter().any(|s| s.passes.is_empty());
        self.frame = free.then(Vec::new);
        self.next = 0;
        self.submitted = None;
    }

    // Zeitstempel fuer den naechsten Pass, oder None (nicht gemessen) -- direkt als
    // `timestamp_writes` in den RenderPassDescriptor.
    pub fn pass(&mut self, label: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let (begin, end) = self.reserve(label)?;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(begin),
            end_of_pass_write_index: Some(end),
        })
    }

    // Wie pass, aber nur die Indizes -- fuer mehrere Passes hintereinander, die ihre
    // Zeitstempel selbst zusammensetzen (mit query_set, s. ShaderPath::shadows).
    pub fn reserve(&mut self, label: &'static str) -> Option<(u32, u32)> {
        let frame = self.frame.as_mut()?;
        if self.next as usize + 2 > MAX_STAMPS {
            return None;
        }
        let (begin, end) = (self.next, self.next + 1);
        self.next += 2;
        frame.push(PassRecord { label, begin, end, marks: Vec::new() });
        Some((begin, end))
    }

    pub fn query_set(&self) -> Arc<wgpu::QuerySet> {
        self.query_set.clone()
    }

    // Ab hier beginnt im zuletzt begonnenen Pass der Abschnitt `label` (bis zur naechsten
    // Marke oder zum Pass-Ende). Ohne Messung oder ohne Unterstuetzung ein No-Op.
    pub fn mark(&mut self, pass: &mut wgpu::RenderPass<'_>, label: &'static str) {
        if !self.inside_passes || self.next as usize >= MAX_STAMPS {
            return;
        }
        let Some(record) = self.frame.as_mut().and_then(|f| f.last_mut()) else { return };
        pass.write_timestamp(&self.query_set, self.next);
        record.marks.push((label, self.next));
        self.next += 1;
    }

    // Nach dem letzten Pass, vor encoder.finish(): Zeitstempel aufloesen und in einen freien
    // Auslese-Puffer kopieren.
    pub fn end_frame(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(passes) = self.frame.take() else { return };
        if passes.is_empty() {
            return;
        }
        let Some(index) = self.slots.iter().position(|s| s.passes.is_empty()) else { return };
        let count = self.next;
        encoder.resolve_query_set(&self.query_set, 0..count, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.slots[index].buffer, 0, count as u64 * 8);
        self.slots[index].passes = passes;
        self.slots[index].stamps = count;
        self.submitted = Some(index);
    }

    // Nach queue.submit(): den eben beschriebenen Puffer zum Lesen anfordern.
    pub fn after_submit(&mut self) {
        let Some(index) = self.submitted.take() else { return };
        let slot = &mut self.slots[index];
        let flag = slot.mapped.clone();
        let bytes = slot.stamps as u64 * 8;
        slot.buffer.slice(..bytes).map_async(wgpu::MapMode::Read, move |r| {
            if r.is_ok() {
                flag.store(true, Ordering::Release);
            }
        });
        slot.waiting = true;
    }

    // Fertige Messungen abholen, ohne zu warten. Gibt die NEUESTE zurueck: (Name, ms) je
    // Pass und je markiertem Abschnitt, dazu die Gesamtzeit vom Anfang des ersten bis zum
    // Ende des letzten Passes.
    pub fn collect(&mut self, device: &wgpu::Device) -> Option<(Vec<(&'static str, f32)>, f32)> {
        if !self.slots.iter().any(|s| s.waiting) {
            return None;
        }
        device.poll(wgpu::Maintain::Poll);
        let mut newest = None;
        for slot in &mut self.slots {
            if !slot.waiting || !slot.mapped.load(Ordering::Acquire) {
                continue;
            }
            let bytes = slot.stamps as u64 * 8;
            let stamps: Vec<u64> = {
                let view = slot.buffer.slice(..bytes).get_mapped_range();
                bytemuck::cast_slice::<u8, u64>(&view).to_vec()
            };
            slot.buffer.unmap();
            let ms = |from: u32, to: u32| {
                let (a, b) = (stamps.get(from as usize).copied().unwrap_or(0), stamps.get(to as usize).copied().unwrap_or(0));
                b.saturating_sub(a) as f32 * self.period_ns / 1e6
            };
            let mut times: Vec<(&'static str, f32)> = Vec::new();
            for pass in &slot.passes {
                times.push((pass.label, ms(pass.begin, pass.end)));
                // Abschnitt i reicht von seiner Marke bis zur naechsten bzw. zum Pass-Ende.
                let grenzen: Vec<u32> = pass.marks.iter().map(|&(_, i)| i).chain(std::iter::once(pass.end)).collect();
                for (k, &(label, start)) in pass.marks.iter().enumerate() {
                    times.push((label, ms(start, grenzen[k + 1])));
                }
            }
            let (first, last) = (slot.passes.first().map_or(0, |p| p.begin), slot.passes.last().map_or(0, |p| p.end));
            newest = Some((times, ms(first, last)));
            slot.passes.clear();
            slot.mapped.store(false, Ordering::Relaxed);
            slot.waiting = false;
        }
        newest
    }
}
