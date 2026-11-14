// Wiedergabe einer Aufnahme (Phase 2 im Plan): eine Uhr, die App und Netz-Task teilen, und die
// Paketquelle fuer die Verbindung im Wiedergabe-Modus.
//
// Spulen nach vorn stellt nur die Uhr: alles bis dahin ist dann faellig und kommt ohne Warten.
// Zurueck geht nur mit einem neuen Durchlauf ab dem Anfang (die Welt laesst sich nicht
// zurueckdrehen), dessen Uhr gleich auf dem Ziel steht -- der Schnelldurchlauf aus dem Plan.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{Entry, Header, ReadError, Reader, Tag};

struct ClockState {
    // Stand der Uhr (ms seit Aufnahmestart) zum Zeitpunkt `at`.
    base_ms: f64,
    at: Instant,
    speed: f64,
    paused: bool,
}

// Die Wiedergabezeit. Nur ein Mutex um vier Zahlen; gefragt wird je Bild und je Paket.
pub struct ReplayClock {
    s: Mutex<ClockState>,
}

impl ReplayClock {
    #[must_use] 
    pub fn new(start_ms: f64) -> Self {
        Self { s: Mutex::new(ClockState { base_ms: start_ms, at: Instant::now(), speed: 1.0, paused: false }) }
    }

    fn with<T>(&self, f: impl FnOnce(&mut ClockState) -> T) -> T {
        let mut s = self.s.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut s)
    }

    fn stand(s: &ClockState) -> f64 {
        if s.paused {
            s.base_ms
        } else {
            s.base_ms + s.at.elapsed().as_secs_f64() * 1000.0 * s.speed
        }
    }

    pub fn now_ms(&self) -> f64 {
        self.with(|s| Self::stand(s))
    }

    pub fn paused(&self) -> bool {
        self.with(|s| s.paused)
    }

    pub fn set_paused(&self, paused: bool) {
        self.with(|s| {
            s.base_ms = Self::stand(s);
            s.at = Instant::now();
            s.paused = paused;
        });
    }

    pub fn speed(&self) -> f64 {
        self.with(|s| s.speed)
    }

    pub fn set_speed(&self, speed: f64) {
        self.with(|s| {
            s.base_ms = Self::stand(s);
            s.at = Instant::now();
            s.speed = speed;
        });
    }

    // Nur nach vorn sinnvoll (s. Modulkopf).
    pub fn seek(&self, ms: f64) {
        self.with(|s| {
            s.base_ms = ms;
            s.at = Instant::now();
        });
    }
}

pub enum Next {
    // Faellig: dieses Paket jetzt verarbeiten.
    Packet(Vec<u8>),
    // Das naechste Paket ist erst in dieser Zeit (Wiedergabezeit, schon durch die
    // Geschwindigkeit geteilt) dran -- oder die Uhr steht.
    Wait(Duration),
    End,
}

// Die Pakete vom Server (Spur S) einer Aufnahme, Block fuer Block gelesen. Eine Aufnahme ab
// der Mitte beginnt mit dem Snapshot (Spur Z, eigene Bloecke), der vorab gelesen wird.
pub struct PacketSource {
    reader: Reader,
    next_block: usize,
    queue: VecDeque<Entry>,
    clock: std::sync::Arc<ReplayClock>,
    snapshot: Vec<Vec<u8>>,
    // Aufnahmezeit des zuletzt herausgegebenen Pakets (ms).
    last_t: f64,
}

impl PacketSource {
    pub fn open(path: &Path, clock: std::sync::Arc<ReplayClock>) -> Result<Self, ReadError> {
        let mut s = Self { reader: Reader::open(path)?, next_block: 0, queue: VecDeque::new(), clock, snapshot: Vec::new(), last_t: 0.0 };
        if !s.reader.header.from_join {
            while s.next_block < s.reader.blocks.len() {
                let block = s.reader.block(s.next_block)?;
                s.next_block += 1;
                let nur_z = block.iter().all(|e| e.tag == Tag::Z);
                for e in block {
                    match e.tag {
                        Tag::Z => s.snapshot.push(e.data),
                        Tag::S => s.queue.push_back(e),
                        _ => {}
                    }
                }
                if !nur_z {
                    break;
                }
            }
        }
        Ok(s)
    }

    // Die Eintraege des Snapshots (leer bei einer Aufnahme ab dem Join oder ohne Snapshot).
    pub fn take_snapshot(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.snapshot)
    }

    #[must_use] 
    pub const fn header(&self) -> &Header {
        &self.reader.header
    }

    // Aufnahmezeit des zuletzt herausgegebenen Pakets (ms), 0 vor dem ersten.
    #[must_use]
    pub const fn last_t(&self) -> f64 {
        self.last_t
    }

    pub fn next_due(&mut self) -> Result<Next, ReadError> {
        while self.queue.is_empty() {
            if self.next_block >= self.reader.blocks.len() {
                return Ok(Next::End);
            }
            let block = self.reader.block(self.next_block)?;
            self.next_block += 1;
            self.queue.extend(block.into_iter().filter(|e| e.tag == Tag::S));
        }
        let t = self.queue[0].t;
        let jetzt = self.clock.now_ms();
        if t <= jetzt {
            self.last_t = t;
            return Ok(Next::Packet(self.queue.pop_front().map(|e| e.data).unwrap_or_default()));
        }
        // Waehrend der Pause wird regelmaessig nachgesehen, ob es weitergeht.
        let speed = self.clock.speed().max(0.01);
        let warte = if self.clock.paused() { 50.0 } else { (t - jetzt) / speed };
        Ok(Next::Wait(Duration::from_secs_f64(warte.clamp(0.0, 50.0) / 1000.0)))
    }
}

// Die Spuren, die die App braucht (Kamera, Zustand, Markierungen), und die Laenge der
// Aufnahme. Liest die ganze Datei, gedacht fuer einen Hintergrund-Thread.
pub struct Tracks {
    pub header: Header,
    pub camera: Vec<(f64, [u8; 37])>,
    pub state: Vec<(f64, Vec<u8>)>,
    pub marks: Vec<(f64, Vec<u8>)>,
    pub duration_ms: f64,
}

pub fn load_tracks(path: &Path) -> Result<Tracks, ReadError> {
    let mut r = Reader::open(path)?;
    let mut t = Tracks { header: r.header.clone(), camera: Vec::new(), state: Vec::new(), marks: Vec::new(), duration_ms: 0.0 };
    r.for_each(|e| {
        t.duration_ms = t.duration_ms.max(e.t);
        match e.tag {
            Tag::K => {
                if let Ok(k) = <[u8; 37]>::try_from(e.data.as_slice()) {
                    t.camera.push((e.t, k));
                }
            }
            Tag::L => t.state.push((e.t, e.data)),
            Tag::E => t.marks.push((e.t, e.data)),
            _ => {}
        }
    })?;
    Ok(t)
}

pub struct Summary {
    pub header: Header,
    pub duration_ms: f64,
    pub complete: bool,
    // Ab dem Join oder mit Snapshot: laesst sich abspielen.
    pub playable: bool,
}

// Kopf und Laenge fuer die Liste der Aufnahmen. Entpackt hoechstens den ersten Block (ob ein
// Snapshot da ist).
pub fn summary(path: &Path) -> Result<Summary, ReadError> {
    let mut r = Reader::open(path)?;
    let duration_ms = r.blocks.last().map_or(0.0, |b| b.end);
    let playable = r.header.from_join || (!r.blocks.is_empty() && r.block(0)?.first().is_some_and(|e| e.tag == Tag::Z));
    Ok(Summary { header: r.header.clone(), duration_ms, complete: r.complete, playable })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uhr_steht_in_der_pause_und_springt_beim_spulen() {
        let c = ReplayClock::new(1000.0);
        c.set_paused(true);
        let a = c.now_ms();
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(c.now_ms(), a);
        c.seek(5000.0);
        assert_eq!(c.now_ms(), 5000.0);
        c.set_paused(false);
        std::thread::sleep(Duration::from_millis(5));
        assert!(c.now_ms() > 5000.0);
    }
}
