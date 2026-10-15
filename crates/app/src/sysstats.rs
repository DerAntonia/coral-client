// Sampling fuer die Graphen im F3-Debug-Overlay: CPU, RAM (mit Swap) und GPU, je als
// Gesamtauslastung des Rechners und als Anteil, den Coral selbst davon nimmt. Alles
// best-effort: was sich auf einem System nicht lesen laesst, bleibt None und der Graph zeigt
// "n/v", statt den Client aufzuhalten.
//
// Gemessen wird in einem eigenen Faden, nur solange das Overlay offen ist (maybe_sample setzt
// je Frame ein Flag). Die GPU-Zaehler von Windows (PDH) brauchen pro Abfrage ein paar
// Millisekunden -- im Frame waere das ein Ruckler.
//
// Quellen:
//   CPU gesamt   Linux /proc/stat, Windows NtQuerySystemInformation (Zeiten je Kern)
//   CPU Coral    Linux getrusage, Windows GetProcessTimes -- geteilt durch alle Kerne
//   RAM gesamt   system_info::memory (RAM + Swap belegt / RAM + Swap gesamt)
//   RAM Coral    Linux VmRSS + VmSwap, Windows PrivateUsage (privat zugesagt, ob im RAM
//                oder ausgelagert)
//   GPU gesamt   Linux amdgpu gpu_busy_percent, Windows "GPU Engine"-Zaehler (je Engine-Typ
//                summiert, davon der hoechste -- nur Grafik/Compute, s. windows_gpu::rendert)
//   GPU Coral    Linux drm-engine-* in /proc/self/fdinfo, Windows dieselben Zaehler, nur
//                die Instanzen mit unserer PID

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// 120 Punkte * 250 ms = 30 s sichtbarer Verlauf.
pub const HISTORY_LEN: usize = 120;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
// Glaettung je Abtastung (exponentiell): Anteil des neuen Werts. Die Kurve selbst rundet der
// Spline beim Zeichnen; das hier nimmt nur das Zittern einzelner Messungen heraus.
const GLAETTUNG: f32 = 0.6;

/// Ein Messpunkt in Prozent (0..100): Rechner gesamt und Anteil von Coral.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub total: f32,
    pub own: f32,
}

/// Ein Graph: Verlauf (aeltester Wert vorn) und eine Zeile fuer die Beschriftung.
#[derive(Clone, Debug, Default)]
pub struct Series {
    pub history: VecDeque<Sample>,
    pub label: String,
    pub available: bool,
}

impl Series {
    fn push(&mut self, s: Sample) {
        let s = match self.history.back() {
            Some(alt) => Sample { total: alt.total + (s.total - alt.total) * GLAETTUNG, own: alt.own + (s.own - alt.own) * GLAETTUNG },
            None => s,
        };
        if self.history.len() >= HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(s);
        self.available = true;
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub cpu: Series,
    pub ram: Series,
    pub gpu: Series,
}

pub struct SystemStats {
    shared: Arc<Mutex<Stats>>,
    wanted: Arc<AtomicBool>,
    started: bool,
}

impl SystemStats {
    pub fn new() -> Self {
        Self { shared: Arc::new(Mutex::new(Stats::default())), wanted: Arc::new(AtomicBool::new(false)), started: false }
    }

    // Je Frame bei offenem Overlay: haelt den Messfaden wach (und startet ihn beim ersten Mal).
    pub fn maybe_sample(&mut self) {
        self.wanted.store(true, Ordering::Relaxed);
        if !self.started {
            self.started = true;
            let (shared, wanted) = (Arc::clone(&self.shared), Arc::clone(&self.wanted));
            let _ = std::thread::Builder::new().name("sysstats".into()).spawn(move || sampler(shared, wanted));
        }
    }

    pub fn snapshot(&self) -> Stats {
        self.shared.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn sampler(shared: Arc<Mutex<Stats>>, wanted: Arc<AtomicBool>) {
    let mut quellen = Quellen::new();
    loop {
        std::thread::sleep(SAMPLE_INTERVAL);
        // Nur noch dieser Faden haelt den Stand: App weg.
        if Arc::strong_count(&shared) == 1 {
            return;
        }
        // Overlay zu: nichts messen, und die Zaehlerstaende vergessen -- sonst wuerde die
        // erste Messung nach dem Wiederoeffnen ueber die ganze Pause mitteln.
        if !wanted.swap(false, Ordering::Relaxed) {
            quellen.vergessen();
            continue;
        }
        let jetzt = Instant::now();
        let cpu = quellen.cpu(jetzt);
        let ram = ram();
        let gpu = quellen.gpu(jetzt);
        let Ok(mut s) = shared.lock() else { return };
        if let Some((sample, label)) = cpu {
            s.cpu.push(sample);
            s.cpu.label = label;
        }
        if let Some((sample, label)) = ram {
            s.ram.push(sample);
            s.ram.label = label;
        }
        if let Some((sample, label)) = gpu {
            s.gpu.push(sample);
            s.gpu.label = label;
        }
    }
}

// Zaehlerstaende der letzten Messung, fuer die Differenzen.
struct Quellen {
    cpu_vorher: Option<(Instant, CpuJiffies, u64)>,
    #[cfg(target_os = "linux")]
    gpu_vorher: Option<(Instant, u64)>,
    #[cfg(windows)]
    pdh: Option<windows_gpu::Pdh>,
    #[cfg(windows)]
    pdh_versucht: bool,
}

impl Quellen {
    fn new() -> Self {
        Self {
            cpu_vorher: None,
            #[cfg(target_os = "linux")]
            gpu_vorher: None,
            #[cfg(windows)]
            pdh: None,
            #[cfg(windows)]
            pdh_versucht: false,
        }
    }

    fn vergessen(&mut self) {
        self.cpu_vorher = None;
        #[cfg(target_os = "linux")]
        {
            self.gpu_vorher = None;
        }
    }

    fn cpu(&mut self, jetzt: Instant) -> Option<(Sample, String)> {
        let kerne = read_cpu_jiffies();
        let summe = kerne.iter().fold(CpuJiffies::default(), |a, k| CpuJiffies { idle: a.idle + k.idle, total: a.total + k.total });
        let eigen = process_cpu_ns()?;
        let vorher = self.cpu_vorher.replace((jetzt, summe, eigen))?;
        let (t0, alt, eigen_alt) = vorher;
        let d_total = summe.total.saturating_sub(alt.total);
        let d_idle = summe.idle.saturating_sub(alt.idle);
        let gesamt = if d_total == 0 { 0.0 } else { (1.0 - d_idle as f32 / d_total as f32).clamp(0.0, 1.0) * 100.0 };
        let wand_ns = jetzt.duration_since(t0).as_nanos() as f64 * kerne.len().max(1) as f64;
        let coral = ((eigen.saturating_sub(eigen_alt)) as f64 / wand_ns.max(1.0) * 100.0).clamp(0.0, 100.0) as f32;
        // Coral kann nicht mehr nehmen, als insgesamt belegt ist (Messzeitpunkte weichen leicht ab).
        let coral = coral.min(gesamt);
        Some((Sample { total: gesamt, own: coral }, format!("CPU {gesamt:.0} %  ·  Coral {coral:.0} %  ({} Kerne)", kerne.len())))
    }

    #[cfg(target_os = "linux")]
    fn gpu(&mut self, jetzt: Instant) -> Option<(Sample, String)> {
        let gesamt = linux_gpu_busy()?;
        let eigen = linux_gpu_own_ns();
        let coral = match (eigen, self.gpu_vorher) {
            (Some(ns), Some((t0, alt))) => {
                let d = ns.saturating_sub(alt) as f64 / jetzt.duration_since(t0).as_nanos().max(1) as f64 * 100.0;
                Some(d.clamp(0.0, 100.0) as f32)
            }
            _ => None,
        };
        self.gpu_vorher = eigen.map(|ns| (jetzt, ns));
        let coral = coral.unwrap_or(0.0).min(gesamt);
        Some((Sample { total: gesamt, own: coral }, format!("GPU {gesamt:.0} %  ·  Coral {coral:.0} %")))
    }

    #[cfg(windows)]
    fn gpu(&mut self, _jetzt: Instant) -> Option<(Sample, String)> {
        if !self.pdh_versucht {
            self.pdh_versucht = true;
            self.pdh = windows_gpu::Pdh::open();
        }
        let (gesamt, coral) = self.pdh.as_mut()?.sample()?;
        let coral = coral.min(gesamt);
        Some((Sample { total: gesamt, own: coral }, format!("GPU {gesamt:.0} %  ·  Coral {coral:.0} %")))
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn gpu(&mut self, _jetzt: Instant) -> Option<(Sample, String)> {
        None
    }
}

fn ram() -> Option<(Sample, String)> {
    let m = crate::system_info::memory()?;
    let gesamt_bytes = (m.ram_total + m.swap_total).max(1);
    let belegt = m.ram_used + m.swap_used;
    let eigen = process_memory_bytes().unwrap_or(0);
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let gesamt = (belegt as f64 / gesamt_bytes as f64 * 100.0) as f32;
    let coral = ((eigen as f64 / gesamt_bytes as f64 * 100.0) as f32).min(gesamt);
    Some((
        Sample { total: gesamt, own: coral },
        format!("RAM + Swap {:.1} / {:.1} GB  ·  Coral {:.1} GB", belegt as f64 / GB, gesamt_bytes as f64 / GB, eigen as f64 / GB),
    ))
}

// Ein Kern: die fuer die Auslastung relevanten Zaehler seit Boot. Nur idle(+iowait) und die
// Summe aller Felder werden gebraucht -- die Differenz zweier Snapshots ergibt die
// Auslastung im Intervall dazwischen.
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
    let Ok(text) = std::fs::read_to_string("/proc/stat") else {
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
        let total: u64 = values.iter().take(8).sum();
        cores.push(CpuJiffies { idle, total });
    }
    cores
}

// CPU-Zeit dieses Prozesses (alle Faeden, Kernel + User) in ns.
#[cfg(target_os = "linux")]
fn process_cpu_ns() -> Option<u64> {
    // SAFETY: rusage ist ein reines Datenstruct, getrusage fuellt es.
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) } != 0 {
        return None;
    }
    let us = |t: libc::timeval| t.tv_sec as u64 * 1_000_000 + t.tv_usec as u64;
    Some((us(r.ru_utime) + us(r.ru_stime)) * 1000)
}

#[cfg(windows)]
fn process_cpu_ns() -> Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut z = [FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }; 4];
    let [a, b, c, d] = &mut z;
    // SAFETY: Pseudo-Handle des eigenen Prozesses, vier gueltige Ausgabefelder.
    if unsafe { GetProcessTimes(GetCurrentProcess(), a, b, c, d) } == 0 {
        return None;
    }
    let ft = |f: &FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) * 100;
    Some(ft(&z[2]) + ft(&z[3]))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn process_cpu_ns() -> Option<u64> {
    None
}

// Speicher dieses Prozesses, im RAM und ausgelagert zusammen.
#[cfg(target_os = "linux")]
fn process_memory_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = |name: &str| -> u64 {
        text.lines().find_map(|z| z.strip_prefix(name)?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0) * 1024
    };
    Some(kb("VmRSS:") + kb("VmSwap:"))
}

#[cfg(windows)]
fn process_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: reines Datenstruct, cb wie verlangt; die _EX-Fassung beginnt mit der einfachen.
    let mut m: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    m.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), (&mut m as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(), m.cb) } == 0 {
        return None;
    }
    Some(m.PrivateUsage as u64)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn process_memory_bytes() -> Option<u64> {
    None
}

// Linux, amdgpu: Auslastung der ersten Karte, die sie meldet (0..100, vom Treiber gemittelt).
#[cfg(target_os = "linux")]
fn linux_gpu_busy() -> Option<f32> {
    let mut karten: Vec<_> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("card") && !n.contains('-')))
        .collect();
    karten.sort();
    karten.iter().find_map(|k| std::fs::read_to_string(k.join("device/gpu_busy_percent")).ok()?.trim().parse::<f32>().ok())
}

// Linux: GPU-Zeit dieses Prozesses in ns, aus den DRM-Angaben je offenem Geraet
// (drm-engine-<name>: <ns> ns). Mehrere fds koennen zu einem Client gehoeren -- je
// drm-client-id nur einmal zaehlen.
#[cfg(target_os = "linux")]
fn linux_gpu_own_ns() -> Option<u64> {
    let mut gesehen = std::collections::HashSet::new();
    let mut summe = 0u64;
    let mut gefunden = false;
    for e in std::fs::read_dir("/proc/self/fdinfo").ok()?.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Some(id) = text.lines().find_map(|z| z.strip_prefix("drm-client-id:")).map(|s| s.trim().to_string()) else { continue };
        if !gesehen.insert(id) {
            continue;
        }
        for z in text.lines() {
            // Nur die Zeiten ("... ns"), nicht drm-engine-capacity-* (Anzahl der Engines).
            if let Some(rest) = z.strip_prefix("drm-engine-").filter(|r| !r.starts_with("capacity-") && r.trim_end().ends_with(" ns")) {
                if let Some(ns) = rest.split_once(':').and_then(|(_, v)| v.split_whitespace().next()?.parse::<u64>().ok()) {
                    summe += ns;
                    gefunden = true;
                }
            }
        }
    }
    gefunden.then_some(summe)
}

// Windows: die Leistungszaehler "GPU Engine(*)\Utilization Percentage" -- eine Instanz je
// Prozess und Engine, Name "pid_1234_luid_..._engtype_3D". Wie im Task-Manager: je Engine-
// Typ ueber alle Instanzen summiert, die Auslastung der GPU ist der hoechste davon.
#[cfg(windows)]
mod windows_gpu {
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
    };

    pub struct Pdh {
        // Query und Zaehler leben so lange wie der Messfaden; Schliessen spart nichts.
        counter: isize,
        query: isize,
        pid: String,
    }

    // Engine-Typ aus dem Instanznamen ("..._engtype_3D" -> "3D").
    fn engine_typ(name: &str) -> &str {
        name.rsplit_once("_engtype_").map_or(name, |(_, t)| t)
    }

    // Nur was rendert: 3D/Grafik und Compute. Video-Dekodierung (ein YouTube-Video im
    // Browser), Kopier- und Encoder-Engines laufen daneben her und bremsen das Spiel nicht --
    // mitgezaehlt standen sie als Spitzen im Graphen, die mit Coral nichts zu tun hatten.
    fn rendert(typ: &str) -> bool {
        let t = typ.to_ascii_lowercase();
        t.contains("3d") || t.starts_with("compute") || t.starts_with("graphics")
    }

    impl Pdh {
        pub fn open() -> Option<Self> {
            let mut query = 0isize;
            let mut counter = 0isize;
            let pfad: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0".encode_utf16().collect();
            // SAFETY: Ausgabezeiger gueltig, Pfad nullterminiert.
            unsafe {
                if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                    return None;
                }
                if PdhAddEnglishCounterW(query, pfad.as_ptr(), 0, &mut counter) != 0 {
                    return None;
                }
                // Raten brauchen zwei Abfragen; die erste legt den Anfang fest.
                PdhCollectQueryData(query);
            }
            Some(Self { counter, query, pid: format!("pid_{}_", std::process::id()) })
        }

        // (gesamt, Coral) in Prozent.
        pub fn sample(&mut self) -> Option<(f32, f32)> {
            // SAFETY: Query aus open().
            if unsafe { PdhCollectQueryData(self.query) } != 0 {
                return None;
            }
            let mut bytes = 0u32;
            let mut anzahl = 0u32;
            // SAFETY: erst nach der noetigen Groesse fragen, dann in einen passenden Puffer lesen.
            let r = unsafe { PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut bytes, &mut anzahl, std::ptr::null_mut()) };
            if r != PDH_MORE_DATA || bytes == 0 {
                return None;
            }
            // Der Puffer enthaelt die Eintraege und dahinter die Namen; als u64 angelegt fuer die
            // Ausrichtung der Eintraege.
            let mut puffer = vec![0u64; (bytes as usize).div_ceil(8)];
            let eintraege = puffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            if unsafe { PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut bytes, &mut anzahl, eintraege) } != 0 {
                return None;
            }
            let mut gesamt: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
            let mut eigen: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
            for i in 0..anzahl as usize {
                // SAFETY: anzahl Eintraege liegen im Puffer, ihre Namen zeigen in denselben.
                let e = unsafe { &*eintraege.add(i) };
                if e.FmtValue.CStatus != 0 {
                    continue;
                }
                let wert = unsafe { e.FmtValue.Anonymous.doubleValue };
                let name = unsafe {
                    let mut n = 0;
                    while *e.szName.add(n) != 0 {
                        n += 1;
                    }
                    String::from_utf16_lossy(std::slice::from_raw_parts(e.szName, n))
                };
                let typ = engine_typ(&name).to_string();
                if !rendert(&typ) {
                    continue;
                }
                *gesamt.entry(typ.clone()).or_default() += wert;
                if name.starts_with(&self.pid) {
                    *eigen.entry(typ).or_default() += wert;
                }
            }
            let hoechster = |m: &std::collections::HashMap<String, f64>| m.values().copied().fold(0.0f64, f64::max).clamp(0.0, 100.0) as f32;
            Some((hoechster(&gesamt), hoechster(&eigen)))
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn engine_typ_aus_dem_namen() {
            assert_eq!(super::engine_typ("pid_1234_luid_0x00000000_0x0000D1B5_phys_0_eng_0_engtype_3D"), "3D");
            assert_eq!(super::engine_typ("pid_8_luid_0x0_0x1_phys_0_eng_3_engtype_VideoDecode"), "VideoDecode");
        }

        #[test]
        fn nur_rendernde_engines() {
            assert!(super::rendert("3D") && super::rendert("Compute_0") && super::rendert("High Priority 3D"));
            assert!(!super::rendert("video codec engine") && !super::rendert("VideoDecode") && !super::rendert("Copy"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verlauf_ist_begrenzt_und_geglaettet() {
        let mut s = Series::default();
        s.push(Sample { total: 0.0, own: 0.0 });
        s.push(Sample { total: 100.0, own: 50.0 });
        let letzter = *s.history.back().unwrap();
        assert!((letzter.total - 100.0 * GLAETTUNG).abs() < 1e-4 && (letzter.own - 50.0 * GLAETTUNG).abs() < 1e-4);
        for _ in 0..HISTORY_LEN * 2 {
            s.push(Sample::default());
        }
        assert_eq!(s.history.len(), HISTORY_LEN);
    }

    #[cfg(windows)]
    #[test]
    fn windows_liefert_jeden_kern() {
        let kerne = read_cpu_jiffies();
        assert_eq!(kerne.len(), std::thread::available_parallelism().unwrap().get());
        assert!(kerne.iter().all(|k| k.total >= k.idle && k.total > 0));
    }

    // Echte Messung: zwei Abtastungen im Abstand, alle drei Quellen muessen etwas liefern.
    #[test]
    #[ignore]
    fn misst_cpu_ram_gpu() {
        let mut q = Quellen::new();
        let t0 = Instant::now();
        let _ = q.cpu(t0);
        let _ = q.gpu(t0);
        std::thread::sleep(Duration::from_millis(500));
        let t1 = Instant::now();
        let (cpu, ram, gpu) = (q.cpu(t1), ram(), q.gpu(t1));
        println!("{cpu:?}\n{ram:?}\n{gpu:?}");
        assert!(cpu.is_some() && ram.is_some() && gpu.is_some());
    }
}
