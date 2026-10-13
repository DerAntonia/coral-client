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
// Nur Linux; anderswo liefert start() None.

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

#[cfg(not(target_os = "linux"))]
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

#[cfg(not(target_os = "linux"))]
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
