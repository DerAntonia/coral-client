// VBlank-Zeitstempel direkt vom Kernel (DRM), fuer den Frame-Profiler: tatsaechliche
// Monitorfrequenz und die Phase jedes Frames im Bildaufbau. Bei Immediate sitzt der Riss
// dort, wo der Monitor gerade ausliest, wenn das neue Bild umschaltet -- die Phase sagt also,
// in welcher Zeile er landet.
//
// Ein eigener Thread wartet per DRM_IOCTL_WAIT_VBLANK auf jeden VBlank und merkt sich
// Zaehler und Zeitstempel. Das braucht keine DRM-Master-Rechte, nur Lesezugriff auf
// /dev/dri/card*. DRM korrigiert die Zeitstempel auf den Beginn des Auslesens der ersten
// sichtbaren Zeile, Zeitbasis ist CLOCK_MONOTONIC (s. monotonic_ns).
//
// Linux ueber DRM, Windows ueber D3DKMT (s. dort); anderswo liefert start() None.

use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
pub struct VblankSample {
    pub seq: u32,
    pub ts_ns: i64,
}

#[derive(Default)]
struct Inner {
    // Die letzten VBlanks, fuer die Periodenschaetzung per Ausgleichsgerade.
    samples: std::collections::VecDeque<VblankSample>,
    period_ns: f64,
}

pub struct VblankMonitor {
    inner: Mutex<Inner>,
    pub source: String,
    // Sichtbare und gesamte Zeilen des Bildmodus (vdisplay, vtotal), falls der Kernel sie
    // verraten hat -- daraus folgt die echte Laenge der Austastluecke.
    pub lines: Option<(u32, u32)>,
}

// Der Bildschirm, auf dem das Spielfenster liegt, wie ihn das Fenstersystem meldet: Lage und
// Groesse in physischen Pixeln, Bildwiederholrate in mHz. Unter X11 ist die Lage genau die
// x/y-Verschiebung der CRTC im gemeinsamen Framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub refresh_mhz: Option<u32>,
}

// Ein aktiver Bildschirmausgang aus den DRM-Moderessourcen. `pipe` ist der Index, den
// WAIT_VBLANK als "crtc" erwartet (Position in der CRTC-Liste der Karte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    pub pipe: u32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub vtotal: u32,
    pub refresh_mhz: u32,
}

// Welcher Ausgang zum Fenster gehoert: gleiche Lage und Groesse vor gleicher Wiederholrate vor
// dem ersten. Frueher nahm die Messung einfach den ersten Ausgang, der antwortete -- mit zwei
// Bildschirmen (oder anderer Reihenfolge nach dem Start) folgte die Riss-Steuerung dann dem
// Takt eines ANDEREN Monitors, und der Riss stand irgendwo im Bild, etwa
// mittendrin.
pub fn pick_output(outputs: &[Output], target: Option<Target>) -> Option<Output> {
    let Some(t) = target else { return outputs.first().copied() };
    let lage = |o: &&Output| o.x as i64 == t.x as i64 && o.y as i64 == t.y as i64 && o.w == t.w && o.h == t.h;
    let rate = |o: &&Output| t.refresh_mhz.is_some_and(|r| (o.refresh_mhz as i64 - r as i64).abs() <= 1500);
    outputs.iter().find(lage).or_else(|| outputs.iter().find(|o| rate(o) && o.w == t.w && o.h == t.h)).or_else(|| outputs.iter().find(rate)).or(outputs.first()).copied()
}

const HISTORY: usize = 480;

impl VblankMonitor {
    // Periode in ns (Ausgleichsgerade ueber die letzten Sekunden) und letzter VBlank.
    pub fn latest(&self) -> Option<(VblankSample, f64)> {
        let inner = self.inner.lock().ok()?;
        let last = *inner.samples.back()?;
        (inner.period_ns > 0.0).then_some((last, inner.period_ns))
    }

    // Phase von `t_ns` im Bildaufbau: Zeit seit dem Beginn des Auslesens, 0..Periode.
    pub fn phase_ns(&self, t_ns: i64) -> Option<(f64, f64)> {
        let (last, period) = self.latest()?;
        Some((((t_ns - last.ts_ns) as f64).rem_euclid(period), period))
    }

    fn push(&self, sample: VblankSample) {
        let Ok(mut inner) = self.inner.lock() else { return };
        inner.samples.push_back(sample);
        while inner.samples.len() > HISTORY {
            inner.samples.pop_front();
        }
        let n = inner.samples.len();
        if n < 8 {
            return;
        }
        let first = inner.samples[0];
        let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
        for s in &inner.samples {
            let x = s.seq.wrapping_sub(first.seq) as f64;
            let y = (s.ts_ns - first.ts_ns) as f64;
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
        }
        let nf = n as f64;
        let denom = nf * sxx - sx * sx;
        if denom > 0.0 {
            inner.period_ns = (nf * sxy - sx * sy) / denom;
        }
    }
}

#[cfg(target_os = "linux")]
pub fn monotonic_ns() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec * 1_000_000_000 + ts.tv_nsec
}

// Windows: QueryPerformanceCounter, dieselbe Uhr, auf die die VBlank-Zeitstempel (s. d3dkmt)
// bezogen werden.
#[cfg(windows)]
pub fn monotonic_ns() -> i64 {
    use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    static FREQ: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let freq = *FREQ.get_or_init(|| {
        let mut f = 0;
        // SAFETY: schreibt nur in f.
        unsafe { QueryPerformanceFrequency(&mut f) };
        f.max(1)
    });
    let mut c = 0;
    // SAFETY: schreibt nur in c.
    unsafe { QueryPerformanceCounter(&mut c) };
    (c as i128 * 1_000_000_000 / freq as i128) as i64
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn monotonic_ns() -> i64 {
    0
}

#[cfg(target_os = "linux")]
mod drm {
    // union drm_wait_vblank aus drm.h: Anfrage (type, sequence, signal) bzw. Antwort (type,
    // sequence, tval_sec, tval_usec) -- beides 24 Byte auf 64 Bit.
    #[repr(C)]
    pub struct WaitVblank {
        pub kind: u32,
        pub sequence: u32,
        pub tval_sec: libc::c_long,
        pub tval_usec: libc::c_long,
    }
    // DRM_IOWR(0x3a, union drm_wait_vblank)
    const DRM_IOCTL_WAIT_VBLANK: libc::c_ulong = 0xC018_643A;
    const VBLANK_RELATIVE: u32 = 0x1;
    const VBLANK_SECONDARY: u32 = 0x2000_0000;
    const VBLANK_HIGH_CRTC_SHIFT: u32 = 1;
    const VBLANK_HIGH_CRTC_MASK: u32 = 0x3e;

    // struct drm_mode_card_res (drm_mode.h), 64 Byte.
    #[repr(C)]
    #[derive(Default)]
    struct CardRes {
        fb_id_ptr: u64,
        crtc_id_ptr: u64,
        connector_id_ptr: u64,
        encoder_id_ptr: u64,
        count_fbs: u32,
        count_crtcs: u32,
        count_connectors: u32,
        count_encoders: u32,
        min_width: u32,
        max_width: u32,
        min_height: u32,
        max_height: u32,
    }
    // struct drm_mode_modeinfo, 68 Byte.
    #[repr(C)]
    #[derive(Default)]
    struct ModeInfo {
        clock: u32,
        hdisplay: u16,
        hsync_start: u16,
        hsync_end: u16,
        htotal: u16,
        hskew: u16,
        vdisplay: u16,
        vsync_start: u16,
        vsync_end: u16,
        vtotal: u16,
        vscan: u16,
        vrefresh: u32,
        flags: u32,
        kind: u32,
        name: [u8; 32],
    }
    // struct drm_mode_crtc, 104 Byte.
    #[repr(C)]
    #[derive(Default)]
    struct ModeCrtc {
        set_connectors_ptr: u64,
        count_connectors: u32,
        crtc_id: u32,
        fb_id: u32,
        x: u32,
        y: u32,
        gamma_size: u32,
        mode_valid: u32,
        mode: ModeInfo,
    }
    const _: () = assert!(std::mem::size_of::<CardRes>() == 64);
    const _: () = assert!(std::mem::size_of::<ModeInfo>() == 68);
    const _: () = assert!(std::mem::size_of::<ModeCrtc>() == 104);
    // DRM_IOWR(0xA0, drm_mode_card_res) / DRM_IOWR(0xA1, drm_mode_crtc). Beide brauchen
    // keine Master-Rechte.
    const DRM_IOCTL_MODE_GETRESOURCES: libc::c_ulong = 0xC040_64A0;
    const DRM_IOCTL_MODE_GETCRTC: libc::c_ulong = 0xC068_64A1;

    // Alle aktiven Ausgaenge dieser Karte (leer, wenn der Treiber keine Moderessourcen hat).
    pub fn outputs(fd: libc::c_int) -> Vec<super::Output> {
        let mut res = CardRes::default();
        if unsafe { libc::ioctl(fd, DRM_IOCTL_MODE_GETRESOURCES as _, &mut res as *mut CardRes) } != 0 || res.count_crtcs == 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; res.count_crtcs.min(32) as usize];
        let mut res2 = CardRes { crtc_id_ptr: ids.as_mut_ptr() as u64, count_crtcs: ids.len() as u32, ..Default::default() };
        if unsafe { libc::ioctl(fd, DRM_IOCTL_MODE_GETRESOURCES as _, &mut res2 as *mut CardRes) } != 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (pipe, &crtc_id) in ids.iter().enumerate().take(res2.count_crtcs.min(32) as usize) {
            let mut c = ModeCrtc { crtc_id, ..Default::default() };
            if unsafe { libc::ioctl(fd, DRM_IOCTL_MODE_GETCRTC as _, &mut c as *mut ModeCrtc) } != 0 || c.mode_valid == 0 {
                continue;
            }
            let m = &c.mode;
            let refresh_mhz = if m.htotal > 0 && m.vtotal > 0 {
                (m.clock as u64 * 1_000_000 / (m.htotal as u64 * m.vtotal as u64)) as u32
            } else {
                m.vrefresh * 1000
            };
            out.push(super::Output {
                pipe: pipe as u32,
                x: c.x,
                y: c.y,
                w: m.hdisplay as u32,
                h: m.vdisplay as u32,
                vtotal: m.vtotal as u32,
                refresh_mhz,
            });
        }
        out
    }

    // Wartet auf den naechsten VBlank von `crtc`; Zaehler und Zeitstempel (ns, MONOTONIC).
    pub fn wait_next(fd: libc::c_int, crtc: u32) -> Option<(u32, i64)> {
        let mut kind = VBLANK_RELATIVE;
        if crtc == 1 {
            kind |= VBLANK_SECONDARY;
        } else if crtc > 1 {
            kind |= (crtc << VBLANK_HIGH_CRTC_SHIFT) & VBLANK_HIGH_CRTC_MASK;
        }
        let mut req = WaitVblank { kind, sequence: 1, tval_sec: 0, tval_usec: 0 };
        let r = unsafe { libc::ioctl(fd, DRM_IOCTL_WAIT_VBLANK as _, &mut req as *mut WaitVblank) };
        (r == 0).then(|| (req.sequence, req.tval_sec * 1_000_000_000 + req.tval_usec * 1000))
    }
}

// Startet den Mess-Thread am Bildschirmausgang des Spielfensters (s. pick_output); ohne
// Moderessourcen am ersten Ausgang, der VBlanks liefert. None, wenn keiner antwortet (kein
// Zugriff, anderes System).
#[cfg(target_os = "linux")]
pub fn start(target: Option<Target>) -> Option<Arc<VblankMonitor>> {
    use std::os::fd::AsRawFd;
    let mut cards: Vec<_> = std::fs::read_dir("/dev/dri")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("card")))
        .collect();
    cards.sort();
    // Erst ueber alle Karten den passenden Ausgang suchen (Hybridgrafik: der Bildschirm kann an
    // der zweiten Karte haengen).
    let mut offen = Vec::new();
    for path in cards {
        let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) else { continue };
        let ausgaenge = drm::outputs(file.as_raw_fd());
        offen.push((path, file, ausgaenge));
    }
    let alle: Vec<Output> = offen.iter().flat_map(|(_, _, a)| a.iter().copied()).collect();
    let gewaehlt = pick_output(&alle, target);
    if let Some(g) = gewaehlt {
        offen.sort_by_key(|(_, _, a)| !a.contains(&g));
    }
    for (path, file, ausgaenge) in offen {
        let fd = file.as_raw_fd();
        let wunsch = gewaehlt.filter(|g| ausgaenge.contains(g));
        let kandidaten: Vec<u32> = match wunsch {
            Some(g) => vec![g.pipe],
            None => (0..4).collect(),
        };
        for crtc in kandidaten {
            if drm::wait_next(fd, crtc).is_none() {
                continue;
            }
            let modus = ausgaenge.iter().find(|o| o.pipe == crtc);
            let monitor = Arc::new(VblankMonitor {
                inner: Mutex::new(Inner::default()),
                source: match modus {
                    Some(o) => format!("{} crtc {crtc} ({}x{} bei {},{}, {:.2} Hz)", path.display(), o.w, o.h, o.x, o.y, o.refresh_mhz as f64 / 1000.0),
                    None => format!("{} crtc {crtc}", path.display()),
                },
                lines: modus.filter(|o| o.vtotal > o.h && o.h > 0).map(|o| (o.h, o.vtotal)),
            });
            let weak = Arc::downgrade(&monitor);
            std::thread::Builder::new()
                .name("vblank".into())
                .spawn(move || {
                    let _file = file;
                    // Endet, sobald niemand mehr den Monitor haelt.
                    while let Some(monitor) = weak.upgrade() {
                        match drm::wait_next(fd, crtc) {
                            Some((seq, ts_ns)) => monitor.push(VblankSample { seq, ts_ns }),
                            None => break,
                        }
                    }
                })
                .ok()?;
            return Some(monitor);
        }
    }
    None
}

// Windows: ueber den Kernel-Grafiktreiber (D3DKMT, gdi32). WaitForVerticalBlankEvent weckt am
// Beginn der Austastluecke, nicht wie DRM mit dem Zeitpunkt von Zeile 0 -- deshalb wird gleich
// nach dem Aufwachen die aktuelle Bildzeile gelesen und daraus der Beginn des naechsten
// Auslesens gerechnet. vtotal (sichtbare Zeilen + Luecke) kennt Windows nicht direkt: beim Start
// einmal zwei Bilder lang die hoechste gemeldete Zeile gesucht.
#[cfg(windows)]
mod d3dkmt {
    use windows_sys::Wdk::Graphics::Direct3D::{
        D3DKMTGetScanLine, D3DKMTOpenAdapterFromGdiDisplayName, D3DKMTWaitForVerticalBlankEvent, D3DKMT_GETSCANLINE,
        D3DKMT_OPENADAPTERFROMGDIDISPLAYNAME, D3DKMT_WAITFORVERTICALBLANKEVENT,
    };
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, ENUM_CURRENT_SETTINGS};

    pub struct Display {
        pub name: [u16; 32],
        pub x: i32,
        pub y: i32,
        pub w: u32,
        pub h: u32,
        pub refresh_hz: u32,
    }

    impl Display {
        pub fn name(&self) -> String {
            let n = self.name.iter().position(|&c| c == 0).unwrap_or(self.name.len());
            String::from_utf16_lossy(&self.name[..n])
        }
    }

    // Alle Bildschirme, die zum Desktop gehoeren, mit ihrem aktuellen Modus.
    pub fn displays() -> Vec<Display> {
        let mut out = Vec::new();
        for i in 0..16 {
            // SAFETY: reine Datenstructs, cb/dmSize wie verlangt gesetzt.
            let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
            dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } == 0 {
                break;
            }
            if dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
                continue;
            }
            let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
            dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            if unsafe { EnumDisplaySettingsW(dd.DeviceName.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) } == 0 {
                continue;
            }
            // SAFETY: bei Bildschirmen ist der Positions-Zweig der Union belegt.
            let pos = unsafe { dm.Anonymous1.Anonymous2.dmPosition };
            out.push(Display { name: dd.DeviceName, x: pos.x, y: pos.y, w: dm.dmPelsWidth, h: dm.dmPelsHeight, refresh_hz: dm.dmDisplayFrequency });
        }
        out
    }

    #[derive(Clone, Copy, PartialEq)]
    pub struct Source {
        adapter: u32,
        vidpn: u32,
    }

    pub fn open(d: &Display) -> Option<Source> {
        // SAFETY: reines Datenstruct, DeviceName ist nullterminiert (aus EnumDisplayDevicesW).
        let mut o: D3DKMT_OPENADAPTERFROMGDIDISPLAYNAME = unsafe { std::mem::zeroed() };
        o.DeviceName = d.name;
        (unsafe { D3DKMTOpenAdapterFromGdiDisplayName(&mut o) } == 0).then_some(Source { adapter: o.hAdapter, vidpn: o.VidPnSourceId })
    }

    // Blockiert bis zum naechsten VBlank. false bei Fehler.
    pub fn wait(s: Source) -> bool {
        let w = D3DKMT_WAITFORVERTICALBLANKEVENT { hAdapter: s.adapter, hDevice: 0, VidPnSourceId: s.vidpn };
        // SAFETY: gueltiger Adapter-Handle aus open().
        unsafe { D3DKMTWaitForVerticalBlankEvent(&w) == 0 }
    }

    // Aktuelle Bildzeile und ob sie in der Austastluecke liegt.
    pub fn scanline(s: Source) -> Option<(u32, bool)> {
        let mut g = D3DKMT_GETSCANLINE { hAdapter: s.adapter, VidPnSourceId: s.vidpn, InVerticalBlank: 0, ScanLine: 0 };
        // SAFETY: gueltiger Adapter-Handle aus open().
        (unsafe { D3DKMTGetScanLine(&mut g) } == 0).then_some((g.ScanLine, g.InVerticalBlank != 0))
    }
}

// Windows: der Ausgang der laufenden Messung, fuer note_present -- mit sichtbaren und gesamten
// Zeilen und der Periode, um Zeilen in Zeit umzurechnen.
#[cfg(windows)]
#[derive(Clone, Copy)]
struct Aktiv {
    source: d3dkmt::Source,
    sichtbar: u32,
    gesamt: u32,
    periode_ns: f64,
}
#[cfg(windows)]
static AKTIV: Mutex<Option<Aktiv>> = Mutex::new(None);
// Zeit seit Beginn des Auslesens von Zeile 0 (ns) direkt nach den letzten Presents.
#[cfg(windows)]
static PRESENTS: Mutex<std::collections::VecDeque<f64>> = Mutex::new(std::collections::VecDeque::new());
#[cfg(windows)]
const PRESENT_RING: usize = 240;

/// Windows: direkt nach dem Present die Bildzeile lesen. Unter Windows gibt es keine
/// Meldung, wann ein Bild wirklich umgeschaltet wurde (unter X11 die Present-Erweiterung) --
/// das hier ist nur, WANN das Present abgegeben wurde. Als Zeit seit Zeile 0 statt in Prozent
/// des Bildes: so laeuft der Wert durch die Austastluecke gleichmaessig weiter, wie Tear timing.
/// Nur waehrend eine Messung laeuft.
#[cfg(windows)]
pub fn note_present() {
    let Some(a) = AKTIV.lock().ok().and_then(|a| *a) else { return };
    let Some((zeile, in_luecke)) = d3dkmt::scanline(a.source) else { return };
    // Treiber, die in der Luecke 0 melden: Beginn der Luecke annehmen.
    let zeile = if in_luecke && zeile < a.sichtbar { a.sichtbar } else { zeile };
    let ns = zeile as f64 * a.periode_ns / a.gesamt as f64;
    let Ok(mut ring) = PRESENTS.lock() else { return };
    ring.push_back(ns);
    if ring.len() > PRESENT_RING {
        ring.pop_front();
    }
    // Alle 2 s bei 120 Hz eine Zeile ins Log.
    static ZAEHLER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if ZAEHLER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % PRESENT_RING as u32 == PRESENT_RING as u32 - 1 {
        if let Some(p) = summarize(&ring, a) {
            println!(
                "[riss] Present {:.2} ms nach Zeile 0 (Mitte 50 %: {:.2}..{:.2}), Bild bis {:.2}, Luecke bis {:.2} ms",
                p.mitte_ms, p.unten_ms, p.oben_ms, p.sichtbar_ms, p.periode_ms
            );
        }
    }
}

#[cfg(windows)]
pub struct PresentPhase {
    pub mitte_ms: f64,
    pub unten_ms: f64,
    pub oben_ms: f64,
    // Ende des sichtbaren Bildes und der Luecke, ab Zeile 0.
    pub sichtbar_ms: f64,
    pub periode_ms: f64,
}

#[cfg(windows)]
fn summarize(ring: &std::collections::VecDeque<f64>, a: Aktiv) -> Option<PresentPhase> {
    if ring.len() < 8 {
        return None;
    }
    let mut werte: Vec<f64> = ring.iter().map(|ns| ns / 1e6).collect();
    werte.sort_by(f64::total_cmp);
    let q = |f: f64| werte[((werte.len() as f64 - 1.0) * f).round() as usize];
    let periode_ms = a.periode_ns / 1e6;
    Some(PresentPhase { mitte_ms: q(0.5), unten_ms: q(0.25), oben_ms: q(0.75), sichtbar_ms: periode_ms * a.sichtbar as f64 / a.gesamt as f64, periode_ms })
}

/// Windows: wo die letzten Presents lagen (s. note_present), None ohne Messung.
#[cfg(windows)]
pub fn present_phase() -> Option<PresentPhase> {
    let a = (*AKTIV.lock().ok()?)?;
    summarize(&*PRESENTS.lock().ok()?, a)
}

#[cfg(windows)]
pub fn start(target: Option<Target>) -> Option<Arc<VblankMonitor>> {
    let displays = d3dkmt::displays();
    // Wie pick_output, aber mit vorzeichenbehafteter Lage (links vom Hauptbildschirm ist x < 0).
    let gewaehlt = target
        .and_then(|t| displays.iter().find(|d| d.x == t.x && d.y == t.y && d.w == t.w && d.h == t.h))
        .or_else(|| target.and_then(|t| displays.iter().find(|d| d.w == t.w && d.h == t.h)))
        .or(displays.first())?;
    let source = d3dkmt::open(gewaehlt)?;
    if !d3dkmt::wait(source) {
        return None;
    }
    let h = gewaehlt.h;
    let period0 = 1e9 / gewaehlt.refresh_hz.max(1) as f64;
    // vtotal: zwei Bilder lang die hoechste Zeile suchen. Treiber, die in der Luecke 0 melden,
    // liefern nichts ueber h -- dann bleibt es bei der Schaetzung in tear_target_phase.
    let anfang = monotonic_ns();
    let mut hoechste = 0;
    while ((monotonic_ns() - anfang) as f64) < 2.0 * period0 {
        if let Some((zeile, _)) = d3dkmt::scanline(source) {
            hoechste = hoechste.max(zeile);
        }
    }
    let lines = (hoechste >= h).then_some((h, hoechste + 1));
    let monitor = Arc::new(VblankMonitor {
        inner: Mutex::new(Inner::default()),
        source: format!(
            "D3DKMT {} ({}x{} bei {},{}, {} Hz, {})",
            gewaehlt.name(),
            gewaehlt.w,
            h,
            gewaehlt.x,
            gewaehlt.y,
            gewaehlt.refresh_hz,
            match lines {
                Some((_, gesamt)) => format!("{gesamt} Zeilen"),
                None => "Zeilen geschaetzt".into(),
            }
        ),
        lines,
    });
    if let Ok(mut a) = AKTIV.lock() {
        let (sichtbar, gesamt) = lines.unwrap_or((h, h + 45));
        *a = Some(Aktiv { source, sichtbar, gesamt, periode_ns: period0 });
    }
    let weak = Arc::downgrade(&monitor);
    std::thread::Builder::new()
        .name("vblank".into())
        .spawn(move || {
            let mut letzter: Option<VblankSample> = None;
            while let Some(monitor) = weak.upgrade() {
                if !d3dkmt::wait(source) {
                    break;
                }
                let jetzt = monotonic_ns();
                let periode = monitor.latest().map_or(period0, |(_, p)| p);
                let (sichtbar, gesamt) = lines.unwrap_or((h, h + 45));
                let zeile_ns = periode / gesamt as f64;
                // Beginn des Auslesens von Zeile 0, das auf diese Luecke folgt.
                let ts_ns = match d3dkmt::scanline(source) {
                    Some((zeile, true)) if zeile >= sichtbar => jetzt + ((gesamt - zeile) as f64 * zeile_ns) as i64,
                    Some((zeile, _)) if zeile < sichtbar => jetzt - (zeile as f64 * zeile_ns) as i64,
                    _ => jetzt + ((gesamt - sichtbar) as f64 * zeile_ns) as i64,
                };
                // Verpasste VBlanks (Thread kam spaet dran) im Zaehler nachtragen.
                let seq = match letzter {
                    Some(l) => l.seq.wrapping_add((((ts_ns - l.ts_ns) as f64 / periode).round() as u32).max(1)),
                    None => 0,
                };
                let sample = VblankSample { seq, ts_ns };
                monitor.push(sample);
                letzter = Some(sample);
            }
            // Messung vorbei: note_present liest nicht mehr -- ausser eine neue Messung hat
            // schon uebernommen.
            if let Ok(mut a) = AKTIV.lock() {
                if a.is_some_and(|a| a.source == source) {
                    *a = None;
                    if let Ok(mut r) = PRESENTS.lock() {
                        r.clear();
                    }
                }
            }
        })
        .ok()?;
    Some(monitor)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn start(_target: Option<Target>) -> Option<Arc<VblankMonitor>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_liegt_in_der_periode() {
        let m = VblankMonitor { inner: Mutex::new(Inner::default()), source: String::new(), lines: None };
        for i in 0..20u32 {
            m.push(VblankSample { seq: 100 + i, ts_ns: 1_000_000 + i as i64 * 8_333_333 });
        }
        let (last, period) = m.latest().unwrap();
        assert!((period - 8_333_333.0).abs() < 1.0);
        let (phase, _) = m.phase_ns(last.ts_ns - 1_000_000).unwrap();
        assert!((phase - 7_333_333.0).abs() < 2.0, "vor dem letzten VBlank zählt vom vorletzten: {phase}");
    }

    #[test]
    // Zwei Bildschirme: der Ausgang unter dem Fenster gewinnt, nicht der erste in der Liste.
    fn nimmt_den_ausgang_unter_dem_fenster() {
        let links = Output { pipe: 0, x: 0, y: 0, w: 2560, h: 1440, vtotal: 1481, refresh_mhz: 59_951 };
        let rechts = Output { pipe: 1, x: 2560, y: 0, w: 1920, h: 1080, vtotal: 1125, refresh_mhz: 119_982 };
        let fenster = Target { x: 2560, y: 0, w: 1920, h: 1080, refresh_mhz: Some(120_000) };
        assert_eq!(pick_output(&[links, rechts], Some(fenster)).unwrap().pipe, 1);
        // Lage unbekannt (Wayland): dann nach Groesse und Wiederholrate.
        let ohne_lage = Target { x: 0, y: 0, w: 1920, h: 1080, refresh_mhz: Some(120_000) };
        assert_eq!(pick_output(&[links, rechts], Some(ohne_lage)).unwrap().pipe, 1);
        assert_eq!(pick_output(&[links, rechts], None).unwrap().pipe, 0);
    }

    // Echte Hardware: misst eine Sekunde lang die Monitorfrequenz.
    #[test]
    #[ignore]
    fn misst_den_monitor() {
        let m = start(None).expect("keine VBlanks");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let (_, period) = m.latest().expect("keine Periode");
        println!("{}: {:.5} Hz", m.source, 1e9 / period);
        let (phase, _) = m.phase_ns(monotonic_ns()).unwrap();
        assert!(phase >= 0.0 && phase < period);
    }
}
