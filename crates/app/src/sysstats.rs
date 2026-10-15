// Sampling fuer den CPU-Graphen im F3-Debug-Overlay (ein Verlauf pro Kern). Reine
// /proc/stat-Auswertung, kein zusaetzlicher Crate noetig -- best-effort: liefert der
// Kernel mal keine Zeile im erwarteten Format, bleibt der Verlauf fuer diesen Tick einfach
// unveraendert, statt den Client zum Abbruch zu bringen.
//
// RAM- und GPU-Graph gab es hier frueher ebenfalls (Prozess-RSS/Swap aus /proc/meminfo,
// GPU-Auslastung aus amdgpu-sysfs) -- rausgeworfen, weil sie im Overlay sichtbar
// fehlerhaft aussahen.

use std::collections::VecDeque;
#[cfg(not(windows))]
use std::fs;
use std::time::{Duration, Instant};

// 90 Punkte * 500ms Abtastintervall = 45s sichtbarer Verlauf.
pub const HISTORY_LEN: usize = 90;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

// Ein Kern aus /proc/stat: die fuer die Auslastung relevanten Jiffie-Zaehler seit Boot.
// Nur idle+iowait ("nicht arbeitend") und die Summe aller Felder ("total") werden
// gebraucht -- die Differenz zweier Snapshots ergibt die Auslastung im Intervall
// dazwischen, absolute Werte allein sagen nichts (die zaehlen seit Systemstart hoch).
#[derive(Clone, Copy, Default)]
struct CpuJiffies {
    idle: u64,
    total: u64,
}

// Windows: dieselben Zaehler je Kern ueber NtQuerySystemInformation (100-ns-Einheiten statt
// Jiffies -- fuer die Auslastung zaehlt nur das Verhaeltnis). KernelTime enthaelt die
// Leerlaufzeit schon, total ist also Kernel + User.
#[cfg(windows)]
fn read_cpu_jiffies() -> Vec<CpuJiffies> {
    use windows_sys::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessorPerformanceInformation};
    use windows_sys::Win32::System::WindowsProgramming::SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION;
    let n = std::thread::available_parallelism().map_or(64, |n| n.get()).max(1);
    // SAFETY: reines Datenstruct.
    let mut buf: Vec<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION> = vec![unsafe { std::mem::zeroed() }; n];
    let mut laenge = 0u32;
    let groesse = (buf.len() * std::mem::size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>()) as u32;
    // SAFETY: Puffer und Groesse passen zusammen.
    let status = unsafe { NtQuerySystemInformation(SystemProcessorPerformanceInformation, buf.as_mut_ptr().cast(), groesse, &mut laenge) };
    if status != 0 {
        return Vec::new();
    }
    let anzahl = laenge as usize / std::mem::size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>();
    buf[..anzahl.min(buf.len())]
        .iter()
        .map(|k| CpuJiffies { idle: k.IdleTime.max(0) as u64, total: (k.KernelTime.max(0) + k.UserTime.max(0)) as u64 })
        .collect()
}

#[cfg(not(windows))]
fn read_cpu_jiffies() -> Vec<CpuJiffies> {
    let Ok(text) = fs::read_to_string("/proc/stat") else {
        return Vec::new();
    };
    let mut cores = Vec::new();
    for line in text.lines() {
        // "cpu " (mit Leerzeichen) ist die Summe ueber alle Kerne -- die einzelnen Kerne
        // heissen "cpu0", "cpu1", ... ohne Leerzeichen davor. Nur die einzelnen wollen wir,
        // sonst gaebe es einen Kern zu viel (den Aggregat-Kern mit).
        if !line.starts_with("cpu") || line.starts_with("cpu ") {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(label) = fields.next() else { continue };
        if !label[3..].chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let values: Vec<u64> = fields.filter_map(|f| f.parse().ok()).collect();
        // user nice system idle iowait irq softirq steal [guest guest_nice] -- guest/
        // guest_nice sind in "user" schon mitgezaehlt (Kernel-Doku), separat aufsummieren
        // wuerde sie doppelt zaehlen.
        if values.len() < 5 {
            continue;
        }
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        let total: u64 = values.iter().sum();
        cores.push(CpuJiffies { idle, total });
    }
    cores
}

pub struct SystemStats {
    last_sample: Instant,
    prev_cpu: Vec<CpuJiffies>,
    pub cpu_history: Vec<VecDeque<f32>>, // ein Ring pro Kern, 0..100
}

impl SystemStats {
    pub fn new() -> Self {
        Self { last_sample: Instant::now() - SAMPLE_INTERVAL, prev_cpu: Vec::new(), cpu_history: Vec::new() }
    }

    // Nur alle SAMPLE_INTERVAL wirklich /proc lesen -- pro Frame (an 120+ FPS) waere das
    // unnoetiger Syscall-Spam fuer einen Graphen, der ohnehin nur alle paar hundert ms
    // sichtbar einen neuen Punkt braucht.
    pub fn maybe_sample(&mut self) {
        if self.last_sample.elapsed() < SAMPLE_INTERVAL {
            return;
        }
        self.last_sample = Instant::now();

        let cores = read_cpu_jiffies();
        if self.cpu_history.len() != cores.len() {
            self.cpu_history = vec![VecDeque::with_capacity(HISTORY_LEN); cores.len()];
        }
        if self.prev_cpu.len() == cores.len() {
            for (i, (prev, cur)) in self.prev_cpu.iter().zip(cores.iter()).enumerate() {
                let delta_total = cur.total.saturating_sub(prev.total);
                let delta_idle = cur.idle.saturating_sub(prev.idle);
                let usage = if delta_total == 0 { 0.0 } else { (1.0 - delta_idle as f32 / delta_total as f32).clamp(0.0, 1.0) * 100.0 };
                push_capped(&mut self.cpu_history[i], usage);
            }
        }
        self.prev_cpu = cores;
    }

    pub fn core_count(&self) -> usize {
        self.cpu_history.len()
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn windows_liefert_jeden_kern() {
        let kerne = super::read_cpu_jiffies();
        assert_eq!(kerne.len(), std::thread::available_parallelism().unwrap().get());
        assert!(kerne.iter().all(|k| k.total >= k.idle && k.total > 0));
    }
}

fn push_capped(ring: &mut VecDeque<f32>, value: f32) {
    if ring.len() >= HISTORY_LEN {
        ring.pop_front();
    }
    ring.push_back(value);
}
