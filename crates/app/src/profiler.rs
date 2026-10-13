// Frame-Profiler: misst je Frame, wo die Zeit hingeht -- in der Fensterschleife
// (about_to_wait, oft mehrmals pro Frame), beim Aufbau des Frames und beim Zeichnen. Dazu die
// Abstaende zwischen den Frames und wie spaet das Betriebssystem die Schleife nach einem
// WaitUntil tatsaechlich aufweckt.
//
// Drei Ausgaben, solange er laeuft:
// - eine Tabelle oben rechts (Mittel/Maximum je Phase ueber die letzten Sekunden),
// - frame_profile.csv im Konfigurationsordner, eine Zeile je Frame (zum Auswerten),
// - eine Logzeile je Ausreisser-Frame mit den teuersten Phasen genau dieses Frames.
//
// Dazu, wenn der Adapter Zeitstempel kann, die GPU-Zeit je Render-Pass (s. gpu_timer) -- sie
// kommt ein, zwei Frames spaeter an und steht beim Frame, in dem sie eintraf.
//
// Aus = kostet fast nichts: add() liest dann nur ein Flag.

use std::collections::VecDeque;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;

use crate::vblank::VblankMonitor;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    // --- Fensterschleife (about_to_wait)
    DrainEvents,
    DrainAuth,
    LoopMisc,
    RenderLimits,
    Mesh,
    Input,
    Movement,
    // --- Frame-Aufbau (RedrawRequested)
    Acquire,
    MovementFrame,
    Animations,
    AnimUpload,
    Hud,
    Textures,
    Entities,
    Hitboxes,
    Hand,
    WorldExtras,
    Doll,
    Upload,
    Menu,
    // --- Zeichnen (in render_world/render_menu)
    Sort,
    Encode,
    Submit,
    Present,
}

impl Phase {
    pub const ALL: [Phase; 24] = [
        Phase::DrainEvents,
        Phase::DrainAuth,
        Phase::LoopMisc,
        Phase::RenderLimits,
        Phase::Mesh,
        Phase::Input,
        Phase::Movement,
        Phase::Acquire,
        Phase::MovementFrame,
        Phase::Animations,
        Phase::AnimUpload,
        Phase::Hud,
        Phase::Textures,
        Phase::Entities,
        Phase::Hitboxes,
        Phase::Hand,
        Phase::WorldExtras,
        Phase::Doll,
        Phase::Upload,
        Phase::Menu,
        Phase::Sort,
        Phase::Encode,
        Phase::Submit,
        Phase::Present,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Phase::DrainEvents => "Pakete",
            Phase::DrainAuth => "Login",
            Phase::LoopMisc => "Schleife sonst",
            Phase::RenderLimits => "Renderdistanz",
            Phase::Mesh => "Mesh-Upload",
            Phase::Input => "Eingabe",
            Phase::Movement => "Ticks (Schleife)",
            Phase::Acquire => "Acquire/Warten",
            Phase::MovementFrame => "Ticks (Frame)",
            Phase::Animations => "Animationen",
            Phase::AnimUpload => "Textur-Upload",
            Phase::Hud => "HUD",
            Phase::Textures => "Texturen/Skins",
            Phase::Entities => "Entities",
            Phase::Hitboxes => "Hitboxen",
            Phase::Hand => "Hand",
            Phase::WorldExtras => "Geister/Partikel",
            Phase::Doll => "Inventarfigur",
            Phase::Upload => "GPU-Upload",
            Phase::Menu => "Menü-UI",
            Phase::Sort => "Sortieren/Offsets",
            Phase::Encode => "Encode",
            Phase::Submit => "Submit",
            Phase::Present => "Present",
        }
    }

    fn csv_name(self) -> String {
        format!("{self:?}").to_lowercase()
    }
}

#[derive(Clone)]
struct FrameRecord {
    at: Instant,
    // Abstand zum vorigen Frame-Ende, ms.
    interval_ms: f32,
    phases: [f32; Phase::ALL.len()],
    loop_calls: u32,
    ticks: u32,
    // Groesste Aufweck-Verspaetung (tatsaechlich - angefordert) in diesem Frame, ms.
    wake_late_ms: f32,
    // Anzeige-Timing (s. set_display_timing), ms. NaN = nicht gemessen.
    display: DisplayTiming,
    gpu: GpuTiming,
}

// Wo im Bildaufbau des Monitors ein Frame ankommt, gemessen an den VBlanks (s. vblank.rs).
// Phase = Zeit seit Beginn des Auslesens der obersten Zeile; bei Immediate sitzt dort der Riss.
#[derive(Clone, Copy)]
struct DisplayTiming {
    period_ms: f32,
    present_phase_ms: f32,
    gpu_phase_ms: f32,
    // Vom Present-Aufruf bis die GPU den Frame fertig hat.
    gpu_ms: f32,
    // Zielphase der Riss-Steuerung (NaN = aus).
    target_ms: f32,
}

impl DisplayTiming {
    const NONE: Self =
        Self { period_ms: f32::NAN, present_phase_ms: f32::NAN, gpu_phase_ms: f32::NAN, gpu_ms: f32::NAN, target_ms: f32::NAN };

    // Die Phase, die ueber den Riss entscheidet: GPU fertig, falls gemessen.
    fn tear_phase(&self) -> f32 {
        if self.gpu_phase_ms.is_nan() { self.present_phase_ms } else { self.gpu_phase_ms }
    }
}

// Ab dieser Phasenaenderung von einem Frame zum naechsten zaehlt ein Frame als Sprung des
// Risses. 0.25 ms sind bei 1080p/120 Hz gut 30 Zeilen.
const TEAR_JUMP_MS: f32 = 0.25;

// Abstand zweier Phasen im Kreis (Periode `period`), mit Vorzeichen.
fn phase_delta(a: f32, b: f32, period: f32) -> f32 {
    let d = (b - a).rem_euclid(period);
    if d > period / 2.0 { d - period } else { d }
}

const WINDOW: Duration = Duration::from_secs(3);
// So oft wird die Tabelle neu berechnet. Jeden Frame kostete sie bei ~400 FPS (1200 Frames
// im Fenster, ueber 30 Zeilen Mittel und Maximum, ~45 formatierte Texte) rund 50 FPS
// -- der Profiler verfaelschte, was er misst. Lesen kann man sie ohnehin nur ein paarmal je
// Sekunde.
const OVERLAY_REFRESH: Duration = Duration::from_millis(250);

// Die gemessenen Passes (Namen wie in GpuState::render_world vergeben), feste Spalten fuer
// CSV und Tabelle. Mehrere Passes mit demselben Namen (zwei HUD-Passes) werden addiert.
// Danach die Abschnitte des Welt-Passes (s. render::BlockRenderer::render).
// "G-Buffer", "Licht", "Tonemap", "Lichtkarten" gibt es nur im Shader-Pfad (s. render::shader_path).
pub const GPU_PASSES: [&str; 15] = [
    "Welt",
    "Hand",
    "HUD",
    "Figur",
    "Ausgabe",
    "Gelaende",
    "Wasser/Glas",
    "Entities",
    "Partikel",
    "Sonstiges",
    "G-Buffer",
    "Licht",
    "Tonemap",
    "Lichtkarten",
    "Spiegelungen",
];

// GPU-Zeit eines Frames: je Pass aus GPU_PASSES, dazu die Gesamtspanne. NaN = nicht gemessen.
#[derive(Clone, Copy)]
struct GpuTiming {
    passes: [f32; GPU_PASSES.len()],
    total: f32,
}

impl GpuTiming {
    const NONE: Self = Self { passes: [f32::NAN; GPU_PASSES.len()], total: f32::NAN };
}

pub struct FrameProfiler {
    active: bool,
    current: [f32; Phase::ALL.len()],
    loop_calls: u32,
    ticks: u32,
    wake_late_ms: f32,
    last_frame_end: Option<Instant>,
    history: VecDeque<FrameRecord>,
    csv: Option<BufWriter<std::fs::File>>,
    started: Instant,
    vblank: Option<Arc<VblankMonitor>>,
    display: DisplayTiming,
    gpu: GpuTiming,
    last_tear_phase: Option<f32>,
    // Zuletzt berechnete Tabelle (s. overlay_rows, OVERLAY_REFRESH).
    overlay_cache: std::cell::RefCell<Option<(Instant, Vec<Vec<String>>)>>,
}

impl FrameProfiler {
    pub fn new() -> Self {
        Self {
            active: false,
            current: [0.0; Phase::ALL.len()],
            loop_calls: 0,
            ticks: 0,
            wake_late_ms: 0.0,
            last_frame_end: None,
            history: VecDeque::new(),
            csv: None,
            started: Instant::now(),
            vblank: None,
            display: DisplayTiming::NONE,
            gpu: GpuTiming::NONE,
            last_tear_phase: None,
            overlay_cache: std::cell::RefCell::new(None),
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    // Ein-/Ausschalten. Beim Einschalten wird frame_profile.csv neu angelegt; gibt den Pfad
    // zurueck (fuer die Meldung), falls das geklappt hat.
    pub fn toggle(&mut self, csv_path: Option<PathBuf>) -> Option<PathBuf> {
        self.active = !self.active;
        self.current = [0.0; Phase::ALL.len()];
        self.loop_calls = 0;
        self.ticks = 0;
        self.wake_late_ms = 0.0;
        self.last_frame_end = None;
        self.history.clear();
        *self.overlay_cache.borrow_mut() = None;
        if let Some(mut csv) = self.csv.take() {
            let _ = csv.flush();
        }
        if !self.active {
            return None;
        }
        self.started = Instant::now();
        self.last_tear_phase = None;
        let path = csv_path?;
        let file = std::fs::File::create(&path).ok()?;
        let mut csv = BufWriter::new(file);
        let mut header = String::from("t_ms,interval_ms,ziel_ms,loop_calls,ticks,wake_late_ms,vblank_period_ms,present_phase_ms,gpu_phase_ms,gpu_ms,riss_ziel_ms,gpu_gesamt_ms");
        for pass in GPU_PASSES {
            header.push_str(&format!(",gpu_{}_ms", pass.to_lowercase().replace('/', "_")));
        }
        for phase in Phase::ALL {
            header.push(',');
            header.push_str(&phase.csv_name());
        }
        let _ = writeln!(csv, "{header}");
        self.csv = Some(csv);
        Some(path)
    }

    // Zeit seit `since` der Phase gutschreiben. Mehrfachaufrufe je Frame summieren sich.
    pub fn add(&mut self, phase: Phase, since: Instant) {
        if self.active {
            self.current[phase as usize] += since.elapsed().as_secs_f32() * 1000.0;
        }
    }

    pub fn add_ms(&mut self, phase: Phase, ms: f32) {
        if self.active {
            self.current[phase as usize] += ms;
        }
    }

    pub fn loop_call(&mut self) {
        self.loop_calls += 1;
    }

    pub fn tick(&mut self) {
        self.ticks += 1;
    }

    // Wann dieser Frame praesentiert wurde und wann die GPU ihn fertig hatte (ns, MONOTONIC).
    pub fn set_display_timing(&mut self, present_ns: i64, gpu_done_ns: Option<i64>, target_ns: Option<f64>) {
        if !self.active {
            return;
        }
        let Some(vblank) = &self.vblank else { return };
        let Some((present_phase, period)) = vblank.phase_ns(present_ns) else { return };
        let gpu_phase = gpu_done_ns.and_then(|t| vblank.phase_ns(t)).map(|(p, _)| p);
        self.display = DisplayTiming {
            period_ms: (period / 1e6) as f32,
            present_phase_ms: (present_phase / 1e6) as f32,
            gpu_phase_ms: gpu_phase.map_or(f32::NAN, |p| (p / 1e6) as f32),
            gpu_ms: gpu_done_ns.map_or(f32::NAN, |t| (t - present_ns) as f32 / 1e6),
            target_ms: target_ns.map_or(f32::NAN, |t| (t / 1e6) as f32),
        };
    }

    // GPU-Zeiten je Pass (s. gpu_timer::GpuTimer::collect), fuer den laufenden Frame.
    pub fn set_gpu_passes(&mut self, passes: &[(&'static str, f32)], total: f32) {
        if !self.active {
            return;
        }
        let mut gpu = GpuTiming { passes: [f32::NAN; GPU_PASSES.len()], total };
        for &(label, ms) in passes {
            if let Some(i) = GPU_PASSES.iter().position(|p| *p == label) {
                gpu.passes[i] = if gpu.passes[i].is_nan() { ms } else { gpu.passes[i] + ms };
            }
        }
        self.gpu = gpu;
    }

    // Die VBlank-Messung gehoert der App (die Riss-Steuerung braucht sie auch), s. App::vblank.
    pub fn set_vblank(&mut self, vblank: Option<Arc<VblankMonitor>>) {
        self.vblank = vblank;
    }

    pub fn vblank_source(&self) -> Option<&str> {
        self.vblank.as_ref().map(|v| v.source.as_str())
    }

    pub fn wake(&mut self, requested: Instant, actual: Instant) {
        if self.active {
            let late = actual.saturating_duration_since(requested).as_secs_f32() * 1000.0;
            self.wake_late_ms = self.wake_late_ms.max(late);
        }
    }

    // Am Ende jedes gezeichneten Frames. `target` = gewolltes Frame-Intervall (None = kein
    // eigener Takt, z.B. Fifo oder unbegrenzt).
    pub fn end_frame(&mut self, target: Option<Duration>) {
        if !self.active {
            return;
        }
        let now = Instant::now();
        let interval_ms = self.last_frame_end.map(|t| (now - t).as_secs_f32() * 1000.0).unwrap_or(0.0);
        self.last_frame_end = Some(now);
        let record = FrameRecord {
            at: now,
            interval_ms,
            phases: self.current,
            loop_calls: self.loop_calls,
            ticks: self.ticks,
            wake_late_ms: self.wake_late_ms,
            display: std::mem::replace(&mut self.display, DisplayTiming::NONE),
            gpu: std::mem::replace(&mut self.gpu, GpuTiming::NONE),
        };
        self.current = [0.0; Phase::ALL.len()];
        self.loop_calls = 0;
        self.ticks = 0;
        self.wake_late_ms = 0.0;

        let target_ms = target.map(|t| t.as_secs_f32() * 1000.0).unwrap_or(0.0);
        if let Some(csv) = self.csv.as_mut() {
            let mut line = format!(
                "{:.3},{:.3},{:.3},{},{},{:.3}",
                (now - self.started).as_secs_f64() * 1000.0,
                record.interval_ms,
                target_ms,
                record.loop_calls,
                record.ticks,
                record.wake_late_ms
            );
            let d = record.display;
            line.push_str(&format!(",{:.6},{:.3},{:.3},{:.3},{:.3}", d.period_ms, d.present_phase_ms, d.gpu_phase_ms, d.gpu_ms, d.target_ms));
            line.push_str(&format!(",{:.3}", record.gpu.total));
            for ms in record.gpu.passes {
                line.push_str(&format!(",{ms:.3}"));
            }
            for ms in record.phases {
                line.push_str(&format!(",{ms:.3}"));
            }
            let _ = writeln!(csv, "{line}");
        }

        // Riss springt: die Phase dieses Frames liegt deutlich anders als die des vorigen.
        let tear = record.display.tear_phase();
        if !tear.is_nan() {
            if let Some(prev) = self.last_tear_phase {
                let delta = phase_delta(prev, tear, record.display.period_ms);
                if delta.abs() > TEAR_JUMP_MS {
                    let mut phases: Vec<(Phase, f32)> = Phase::ALL.into_iter().zip(record.phases).filter(|(_, ms)| *ms >= 0.05).collect();
                    phases.sort_by(|a, b| b.1.total_cmp(&a.1));
                    let teuerste: Vec<String> = phases.iter().take(4).map(|(p, ms)| format!("{} {ms:.2}", p.label())).collect();
                    eprintln!(
                        "[profil] Riss springt {delta:+.2} ms ({prev:.2} -> {tear:.2} von {:.3}): Intervall {interval_ms:.2}, Present-Phase {:.2}, GPU nach Present {:.2} ms | {}",
                        record.display.period_ms,
                        record.display.present_phase_ms,
                        record.display.gpu_ms,
                        teuerste.join(", ")
                    );
                }
            }
            self.last_tear_phase = Some(tear);
        }

        // Ausreisser: deutlich laenger als das Ziel (bzw. als der Schnitt, wenn es keins gibt).
        let reference = if target_ms > 0.0 { target_ms } else { self.mean_interval() };
        if reference > 0.0 && interval_ms > reference * 1.5 {
            let mut phases: Vec<(Phase, f32)> = Phase::ALL.into_iter().zip(record.phases).filter(|(_, ms)| *ms >= 0.05).collect();
            phases.sort_by(|a, b| b.1.total_cmp(&a.1));
            let teuerste: Vec<String> = phases.iter().take(5).map(|(p, ms)| format!("{} {ms:.2}", p.label())).collect();
            let gemessen: f32 = record.phases.iter().sum();
            eprintln!(
                "[profil] Frame {interval_ms:.2} ms (Ziel {reference:.2}): {} | gemessen {gemessen:.2}, Rest (Schlaf/unerfasst) {:.2} | {} Schleifendurchläufe, {} Ticks, Aufwecken bis {:.2} ms zu spät",
                teuerste.join(", "),
                interval_ms - gemessen,
                record.loop_calls,
                record.ticks,
                record.wake_late_ms
            );
        }

        self.history.push_back(record);
        while self.history.front().is_some_and(|r| now - r.at > WINDOW) {
            self.history.pop_front();
        }
    }

    fn mean_interval(&self) -> f32 {
        let n = self.history.len();
        if n == 0 {
            return 0.0;
        }
        self.history.iter().map(|r| r.interval_ms).sum::<f32>() / n as f32
    }

    // Zeilen fuer die Tabelle auf dem Bildschirm: je Zeile eine Zelle (Kopfzeilen) oder drei
    // (Phase, Mittel, Maximum) -- die Schrift ist nicht dicktengleich, die Spalten richtet
    // deshalb der Aufrufer aus.
    pub fn overlay_rows(&self, target: Option<Duration>) -> Vec<Vec<String>> {
        let mut cache = self.overlay_cache.borrow_mut();
        if let Some((at, rows)) = cache.as_ref() {
            if at.elapsed() < OVERLAY_REFRESH {
                return rows.clone();
            }
        }
        let rows = self.compute_overlay_rows(target);
        *cache = Some((Instant::now(), rows.clone()));
        rows
    }

    fn compute_overlay_rows(&self, target: Option<Duration>) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        let n = self.history.len();
        rows.push(vec![format!("FRAME-PROFILER ({} s, {n} Frames)", WINDOW.as_secs())]);
        if n == 0 {
            return rows;
        }
        let intervals: Vec<f32> = self.history.iter().map(|r| r.interval_ms).collect();
        let mean = intervals.iter().sum::<f32>() / n as f32;
        let sd = (intervals.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n as f32).sqrt();
        let max = intervals.iter().copied().fold(0.0, f32::max);
        let min = intervals.iter().copied().fold(f32::INFINITY, f32::min);
        let target_ms = target.map(|t| t.as_secs_f32() * 1000.0);
        let spikes = match target_ms {
            Some(t) if t > 0.0 => intervals.iter().filter(|&&x| x > t * 1.5).count(),
            _ => intervals.iter().filter(|&&x| x > mean * 1.5).count(),
        };
        rows.push(vec![format!(
            "Intervall {mean:.2} ms  sd {sd:.2}  min {min:.2}  max {max:.2}{}",
            target_ms.map(|t| format!("  (Ziel {t:.2})")).unwrap_or_default()
        )]);
        let wake_max = self.history.iter().map(|r| r.wake_late_ms).fold(0.0, f32::max);
        let loops = self.history.iter().map(|r| r.loop_calls).sum::<u32>() as f32 / n as f32;
        let ticks = self.history.iter().map(|r| r.ticks).sum::<u32>();
        rows.push(vec![format!("Ausreißer {spikes}  Aufwecken max +{wake_max:.2} ms  Schleife {loops:.1}x/Frame  Ticks {ticks}")]);
        rows.extend(self.display_rows());
        rows.extend(self.gpu_rows());
        rows.push(vec!["Phase (ms)".into(), "mittel".into(), "max".into()]);
        let mut summe = 0.0;
        for (i, phase) in Phase::ALL.into_iter().enumerate() {
            let avg = self.history.iter().map(|r| r.phases[i]).sum::<f32>() / n as f32;
            let max = self.history.iter().map(|r| r.phases[i]).fold(0.0, f32::max);
            summe += avg;
            if max < 0.01 {
                continue;
            }
            rows.push(vec![phase.label().into(), format!("{avg:.2}"), format!("{max:.2}")]);
        }
        rows.push(vec!["Summe".into(), format!("{summe:.2}"), String::new()]);
        rows.push(vec!["Rest (Schlaf/unerfasst)".into(), format!("{:.2}", mean - summe), String::new()]);
        rows
    }

    // GPU-Zeit je Pass (Zeitstempel), Mittel und Maximum ueber das Fenster.
    fn gpu_rows(&self) -> Vec<Vec<String>> {
        let stat = |values: Vec<f32>| -> Option<(f32, f32)> {
            (!values.is_empty()).then(|| (values.iter().sum::<f32>() / values.len() as f32, values.iter().copied().fold(0.0, f32::max)))
        };
        let Some((avg, max)) = stat(self.history.iter().map(|r| r.gpu.total).filter(|x| !x.is_nan()).collect()) else {
            return Vec::new();
        };
        let mut rows = vec![vec!["GPU (ms)".into(), "mittel".into(), "max".into()], vec!["GPU gesamt".into(), format!("{avg:.2}"), format!("{max:.2}")]];
        for (i, pass) in GPU_PASSES.into_iter().enumerate() {
            if let Some((avg, max)) = stat(self.history.iter().map(|r| r.gpu.passes[i]).filter(|x| !x.is_nan()).collect()) {
                // Abschnitte des Welt-Passes eingerueckt unter "Welt".
                let name = if i >= 5 { format!("  {pass}") } else { format!("GPU {pass}") };
                rows.push(vec![name, format!("{avg:.2}"), format!("{max:.2}")]);
            }
        }
        rows
    }

    // Kopfzeilen zum Anzeige-Timing: Monitorfrequenz, Lage und Streuung des Risses, Spruenge.
    fn display_rows(&self) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        let Some(source) = self.vblank_source() else {
            rows.push(vec!["Monitor: keine VBlank-Zeitstempel".into()]);
            return rows;
        };
        let measured: Vec<&DisplayTiming> = self.history.iter().map(|r| &r.display).filter(|d| !d.tear_phase().is_nan()).collect();
        let Some(last) = measured.last() else {
            rows.push(vec![format!("Monitor: warte auf VBlanks ({source})")]);
            return rows;
        };
        let period = last.period_ms;
        rows.push(vec![format!("Monitor {:.5} Hz  Periode {:.4} ms  ({source})", 1000.0 / period, period)]);
        // Kreisstatistik: Phasen als Winkel, Streuung um die mittlere Richtung.
        let (mut sx, mut sy) = (0.0f32, 0.0f32);
        for d in &measured {
            let a = d.tear_phase() / period * std::f32::consts::TAU;
            sx += a.cos();
            sy += a.sin();
        }
        let mean = (sy.atan2(sx) / std::f32::consts::TAU * period).rem_euclid(period);
        let sd = (measured.iter().map(|d| phase_delta(mean, d.tear_phase(), period).powi(2)).sum::<f32>() / measured.len() as f32).sqrt();
        let jumps = measured.windows(2).filter(|w| phase_delta(w[0].tear_phase(), w[1].tear_phase(), period).abs() > TEAR_JUMP_MS).count();
        let quelle = if last.gpu_phase_ms.is_nan() { "Present" } else { "GPU fertig" };
        if !last.target_ms.is_nan() {
            let abw: Vec<f32> = measured.iter().filter(|d| !d.target_ms.is_nan()).map(|d| phase_delta(d.target_ms, d.tear_phase(), period)).collect();
            let n = abw.len().max(1) as f32;
            let mittel = abw.iter().sum::<f32>() / n;
            let max = abw.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            rows.push(vec![format!("Riss-Steuerung: Ziel {:.2} ms  Abweichung mittel {mittel:+.3}  max {max:.3} ms", last.target_ms)]);
        }
        rows.push(vec![format!(
            "Riss ({quelle}) bei {:.0} %  Phase {mean:.2} ms  sd {sd:.3} ms  Sprünge {jumps}",
            mean / period * 100.0
        )]);
        let gpu: Vec<f32> = measured.iter().map(|d| d.gpu_ms).filter(|x| !x.is_nan()).collect();
        if !gpu.is_empty() {
            let avg = gpu.iter().sum::<f32>() / gpu.len() as f32;
            let max = gpu.iter().copied().fold(0.0, f32::max);
            rows.push(vec![format!("GPU nach Present: mittel {avg:.2} ms  max {max:.2} ms")]);
        }
        rows
    }
}
