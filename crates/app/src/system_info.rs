// Angaben fuer den Abschnitt System im Render-Tab: Name der CPU, Arbeitsspeicher, Swap und ob
// ein Compositor laeuft. Alles best-effort -- was sich nicht lesen laesst, ist None, und der
// Tab schreibt "unbekannt".

// Belegt/gesamt in Bytes, aus /proc/meminfo: belegt = gesamt - verfuegbar (MemAvailable rechnet
// den Cache, den der Kernel jederzeit freigeben kann, schon heraus).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Memory {
    pub ram_used: u64,
    pub ram_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
}

pub fn memory() -> Option<Memory> {
    #[cfg(target_os = "linux")]
    {
        parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
    }
    #[cfg(windows)]
    {
        windows_memory()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

// Windows kennt keinen Swap wie Linux, nur die Auslagerungsdatei: der Commit-Rahmen
// (TotalPageFile) ist RAM plus Auslagerungsdatei, der Swap also der Teil darueber.
#[cfg(windows)]
fn windows_memory() -> Option<Memory> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX ist ein reines Datenstruct, dwLength wird wie verlangt gesetzt.
    let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut s) } == 0 {
        return None;
    }
    let ram_used = s.ullTotalPhys.saturating_sub(s.ullAvailPhys);
    let swap_total = s.ullTotalPageFile.saturating_sub(s.ullTotalPhys);
    let commit_used = s.ullTotalPageFile.saturating_sub(s.ullAvailPageFile);
    let swap_used = commit_used.saturating_sub(ram_used).min(swap_total);
    Some(Memory { ram_used, ram_total: s.ullTotalPhys, swap_used, swap_total })
}

// "AMD Ryzen 7 9800X3D 8-Core Processor" aus der Registry -- PROCESSOR_IDENTIFIER nennt nur
// Familie/Modell/Stepping.
#[cfg(windows)]
fn windows_cpu_name() -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (key, value) = (wide(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"), wide("ProcessorNameString"));
    let mut buf = [0u16; 256];
    let mut len = std::mem::size_of_val(&buf) as u32;
    // SAFETY: Schluessel und Wertname sind nullterminiert, buf/len beschreiben einen gueltigen Puffer.
    let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len) };
    if r != 0 {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]).split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_meminfo(text: &str) -> Option<Memory> {
    let kb = |name: &str| -> Option<u64> {
        let zeile = text.lines().find(|z| z.split(':').next() == Some(name))?;
        zeile.split_whitespace().nth(1)?.parse::<u64>().ok().map(|v| v * 1024)
    };
    let ram_total = kb("MemTotal")?;
    let ram_used = ram_total.saturating_sub(kb("MemAvailable")?);
    let swap_total = kb("SwapTotal").unwrap_or(0);
    let swap_used = swap_total.saturating_sub(kb("SwapFree").unwrap_or(swap_total));
    Some(Memory { ram_used, ram_total, swap_used, swap_total })
}

// "AMD Ryzen 7 7800X3D 8-Core Processor (16 Threads)".
pub fn cpu_name() -> Option<String> {
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    #[cfg(target_os = "linux")]
    let name = {
        let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        text.lines().find(|z| z.starts_with("model name"))?.split_once(':')?.1.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    #[cfg(windows)]
    let name = windows_cpu_name().or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok())?;
    #[cfg(not(any(target_os = "linux", windows)))]
    let name = std::env::var("PROCESSOR_IDENTIFIER").ok()?;
    Some(if threads > 0 { format!("{name} ({threads} Threads)") } else { name })
}

// Ob auf dem Bildschirm ein Compositor laeuft (und wie er heisst). Ob er unser Fenster auch
// wirklich zusammensetzt, sagt erst watch_window -- Vollbildfenster lassen viele aus.
#[derive(Clone, Debug, PartialEq)]
pub enum Compositing {
    Off,
    // Name des Compositors (vom Fenstermanager, falls er beides ist).
    On { name: Option<String> },
    // Windows: der Desktop Window Manager setzt immer zusammen.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Always,
}

pub fn compositing() -> Option<Compositing> {
    #[cfg(target_os = "linux")]
    {
        x11_compositing()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Some(Compositing::Always)
    }
}

// Ein Compositor haelt unter X11 die Auswahl _NET_WM_CM_S<Bildschirm> (EWMH).
#[cfg(target_os = "linux")]
fn x11_compositing() -> Option<Compositing> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
    let (conn, screen) = x11rb::connect(None).ok()?;
    let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
    let besitzer = conn.get_selection_owner(atom(&format!("_NET_WM_CM_S{screen}"))?).ok()?.reply().ok()?.owner;
    if besitzer == x11rb::NONE {
        return Some(Compositing::Off);
    }
    // Name: erst am Besitzer der Auswahl, sonst am Fenster, mit dem sich der Fenstermanager
    // ausweist (Muffin, Mutter und KWin sind beides).
    let utf8 = atom("UTF8_STRING")?;
    let wm_name = atom("_NET_WM_NAME")?;
    let name_von = |fenster: u32| -> Option<String> {
        let p = conn.get_property(false, fenster, wm_name, utf8, 0, 64).ok()?.reply().ok()?;
        String::from_utf8(p.value).ok().filter(|n| !n.is_empty())
    };
    let root = conn.setup().roots.get(screen)?.root;
    let wm_fenster = || -> Option<u32> {
        let p = conn.get_property(false, root, atom("_NET_SUPPORTING_WM_CHECK")?, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        let fenster = p.value32()?.next();
        fenster
    };
    let name = name_von(besitzer).or_else(|| wm_fenster().and_then(name_von));
    Some(Compositing::On { name })
}

// Wie der X-Server unser Fenster gerade auf den Schirm bringt (s. watch_window).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WindowPath {
    // Leitet ein Compositor das Fenster um, setzt er es also selbst zusammen? None = noch nicht
    // geprueft oder nicht pruefbar.
    pub composited: Option<bool>,
    // Wie der X-Server das letzte Bild umgesetzt hat (Present-Erweiterung).
    pub last_present: Option<PresentPath>,
    // Wann (CLOCK_MONOTONIC, ns) die letzten Bilder umgesetzt wurden, als Ring: Daraus und aus
    // den VBlanks folgt, an welcher Bildzeile der Wechsel geschah -- dort liegt der Riss.
    pub done_ns: [i64; DONE_RING],
    pub done_count: usize,
}

pub const DONE_RING: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PresentPath {
    // Hardware-Flip: das Bild wird direkt angezeigt, ohne Kopie.
    Flip,
    // Der X-Server kopiert das Bild auf den Schirm (bzw. in den Puffer des Compositors).
    Copy,
    // Kopie, obwohl ein Flip ginge, wenn das Bild anders vorlaege (Format, Groesse).
    SuboptimalCopy,
    // Bild uebersprungen, weil schon ein neueres kam.
    Skip,
}

// Wo die Bildwechsel der letzten Bilder lagen, aus ihren Zeitpunkten und dem VBlank
// (`vblank_ns` = Beginn des Auslesens von Zeile 0, `period_ns`, `visible_ns` = Anteil der
// sichtbaren Zeilen daran). Mitte und Streuung der Lage: 0..1 = von oben nach unten, >= 1 =
// in der Austastluecke.
pub fn tear_position(p: &WindowPath, vblank_ns: i64, period_ns: f64, visible_ns: f64) -> Option<(f64, f64)> {
    let n = p.done_count.min(DONE_RING);
    if n < 8 || period_ns <= 0.0 || visible_ns <= 0.0 {
        return None;
    }
    let mut lagen: Vec<f64> = p.done_ns[..n].iter().map(|&t| ((t - vblank_ns) as f64).rem_euclid(period_ns) / visible_ns).collect();
    lagen.sort_by(f64::total_cmp);
    let mitte = lagen[n / 2];
    let streuung = lagen[n * 3 / 4] - lagen[n / 4];
    Some((mitte, streuung))
}

// Beobachtet unser Fenster mit eigenen X-Verbindungen: ein Thread wartet auf die
// Present-Meldung jedes Bildes (Flip oder Kopie, und wann), ein zweiter prueft jede Sekunde, ob
// ein Compositor das Fenster umleitet. Beide enden, sobald der Aufrufer den Stand fallen laesst
// (die App haelt ihn nur, solange die Render-Seite offen ist).
#[cfg(target_os = "linux")]
pub fn watch_window(window: u32) -> Option<std::sync::Arc<std::sync::Mutex<WindowPath>>> {
    let stand = std::sync::Arc::new(std::sync::Mutex::new(WindowPath::default()));
    let ziel = std::sync::Arc::clone(&stand);
    std::thread::Builder::new().name("present-meldungen".into()).spawn(move || watch_presents(window, &ziel)).ok()?;
    let ziel = std::sync::Arc::clone(&stand);
    std::thread::Builder::new().name("compositor-pruefung".into()).spawn(move || watch_composited(window, &ziel)).ok()?;
    Some(stand)
}

#[cfg(not(target_os = "linux"))]
pub fn watch_window(_window: u32) -> Option<std::sync::Arc<std::sync::Mutex<WindowPath>>> {
    None
}

// Auf die Meldungen warten (poll auf die Verbindung): die Zeit beim Aufwachen ist der
// Zeitpunkt, zu dem der X-Server das Bild umgesetzt hat (er meldet gleich danach), bis auf ein
// paar Mikrosekunden. Hoechstens 250 ms am Stueck, dann nachsehen, ob noch jemand zusieht.
#[cfg(target_os = "linux")]
fn watch_presents(window: u32, ziel: &std::sync::Arc<std::sync::Mutex<WindowPath>>) -> Option<()> {
    use std::os::fd::AsRawFd;
    use x11rb::connection::Connection;
    use x11rb::protocol::present::{self, ConnectionExt as _};
    use x11rb::protocol::Event;
    let (conn, _) = x11rb::connect(None).ok()?;
    conn.present_query_version(1, 2).ok()?.reply().ok()?;
    let eid = conn.generate_id().ok()?;
    conn.present_select_input(eid, window, present::EventMask::COMPLETE_NOTIFY).ok()?.check().ok()?;
    let fd = conn.stream().as_raw_fd();
    // Gehalten von der App, diesem und dem Pruef-Thread: ohne die App sind es hoechstens zwei.
    while std::sync::Arc::strong_count(ziel) > 2 {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: ein gueltiger pollfd, das fd gehoert der Verbindung und lebt so lange wie sie.
        unsafe { libc::poll(&mut pfd, 1, 250) };
        let jetzt = crate::vblank::monotonic_ns();
        while let Some(ereignis) = conn.poll_for_event().ok()? {
            if let Event::PresentCompleteNotify(e) = ereignis {
                note_present(ziel, &e, jetzt)?;
            }
        }
    }
    Some(())
}

// Eine Meldung eintragen: wie das Bild umgesetzt wurde und wann.
#[cfg(target_os = "linux")]
fn note_present(ziel: &std::sync::Mutex<WindowPath>, e: &x11rb::protocol::present::CompleteNotifyEvent, jetzt: i64) -> Option<()> {
    use x11rb::protocol::present;
    if e.kind != present::CompleteKind::PIXMAP {
        return Some(());
    }
    let weg = match e.mode {
        present::CompleteMode::FLIP => PresentPath::Flip,
        present::CompleteMode::COPY => PresentPath::Copy,
        present::CompleteMode::SUBOPTIMAL_COPY => PresentPath::SuboptimalCopy,
        _ => PresentPath::Skip,
    };
    let mut z = ziel.lock().ok()?;
    z.last_present = Some(weg);
    if weg != PresentPath::Skip {
        let i = z.done_count % DONE_RING;
        z.done_ns[i] = jetzt;
        z.done_count += 1;
    }
    Some(())
}

// Ein Compositor leitet das oberste Fenster unter der Wurzel um -- den Rahmen, in den der
// Fenstermanager unseres gehaengt hat. Nur ein umgeleitetes Fenster hat einen eigenen Pixmap
// (sonst BadMatch). Endet, wenn niemand mehr den Stand haelt.
#[cfg(target_os = "linux")]
fn watch_composited(window: u32, ziel: &std::sync::Arc<std::sync::Mutex<WindowPath>>) -> Option<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::composite::ConnectionExt as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    let (conn, _) = x11rb::connect(None).ok()?;
    conn.composite_query_version(0, 4).ok()?.reply().ok()?;
    let oberstes = || -> Option<u32> {
        let mut w = window;
        loop {
            let t = conn.query_tree(w).ok()?.reply().ok()?;
            if t.parent == t.root || t.parent == x11rb::NONE {
                return Some(w);
            }
            w = t.parent;
        }
    };
    // Gehalten von der App, dem Meldungs-Thread und diesem: ohne die App hoechstens zwei.
    while std::sync::Arc::strong_count(ziel) > 2 {
        let umgeleitet = oberstes().and_then(|oben| {
            let pixmap = conn.generate_id().ok()?;
            let ok = conn.composite_name_window_pixmap(oben, pixmap).ok()?.check().is_ok();
            if ok {
                let _ = conn.free_pixmap(pixmap);
            }
            Some(ok)
        });
        if let Ok(mut z) = ziel.lock() {
            z.composited = umgeleitet;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Some(())
}

// "12.3 / 31.2 GB" -- Punkt und Schraegstrich, damit es in jeder Sprache passt.
pub fn format_used(used: u64, total: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    format!("{:.1} / {:.1} GB", used as f64 / GB, total as f64 / GB)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_liefert_cpu_und_speicher() {
        let name = cpu_name().unwrap();
        assert!(!name.contains("Family"), "{name}");
        let m = memory().unwrap();
        assert!(m.ram_total > 0 && m.ram_used <= m.ram_total && m.swap_used <= m.swap_total, "{m:?}");
        eprintln!("{name} | RAM {} | Swap {}", format_used(m.ram_used, m.ram_total), format_used(m.swap_used, m.swap_total));
    }

    #[test]
    fn meminfo_wird_gelesen() {
        let text = "MemTotal:       32000000 kB\nMemFree:         1000000 kB\nMemAvailable:   20000000 kB\nSwapTotal:       8000000 kB\nSwapFree:        6000000 kB\n";
        let m = parse_meminfo(text).unwrap();
        assert_eq!(m.ram_total, 32_000_000 * 1024);
        assert_eq!(m.ram_used, 12_000_000 * 1024);
        assert_eq!((m.swap_used, m.swap_total), (2_000_000 * 1024, 8_000_000 * 1024));
        // Ohne Swap.
        let m = parse_meminfo("MemTotal: 100 kB\nMemAvailable: 40 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n").unwrap();
        assert_eq!((m.ram_used, m.swap_total), (60 * 1024, 0));
        assert!(parse_meminfo("Unsinn").is_none());
    }

    // Fragt das echte System ab -- nur von Hand: cargo test -- --ignored system_info.
    #[test]
    #[ignore]
    fn zeigt_dieses_system() {
        println!("CPU {:?}\nSpeicher {:?}\nCompositing {:?}", cpu_name(), memory(), compositing());
    }

    #[test]
    fn rissstelle_aus_den_zeitpunkten() {
        let mut p = WindowPath::default();
        // 120 Hz, 1080 von 1125 Zeilen sichtbar; jeder Wechsel 1,6 ms nach Zeile 0 -> ~20 %.
        let periode = 8_333_333.0;
        let sichtbar = periode * 1080.0 / 1125.0;
        for i in 0..20 {
            p.done_ns[i] = 1_000_000_000 + (i as f64 * periode) as i64 + 1_600_000;
            p.done_count += 1;
        }
        let (mitte, streuung) = tear_position(&p, 1_000_000_000, periode, sichtbar).unwrap();
        assert!((mitte - 0.2).abs() < 0.01, "{mitte}");
        assert!(streuung < 0.01);
        // Kurz vor dem naechsten Zeile-0: in der Austastluecke.
        for i in 0..20 {
            p.done_ns[i] = 1_000_000_000 + ((i + 1) as f64 * periode) as i64 - 100_000;
        }
        assert!(tear_position(&p, 1_000_000_000, periode, sichtbar).unwrap().0 >= 1.0);
    }

    #[test]
    fn mengen_mit_einer_nachkommastelle() {
        assert_eq!(format_used(3 * 1024 * 1024 * 1024 / 2, 16 * 1024 * 1024 * 1024), "1.5 / 16.0 GB");
    }
}
