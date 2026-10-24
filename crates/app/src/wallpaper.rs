//! Hintergrundbild des Startbildschirms (Einstellungen -> Design).
//!
//! Drei Quellen: keins, ein eigenes Bild oder das Hintergrundbild des Linux-Desktops. Ein
//! eigenes Bild wird beim Auswaehlen in den Einstellungsordner KOPIERT (neben
//! settings.conf) -- verschiebt oder loescht man danach das Original, bleibt der Hintergrund.
//!
//! Alles, was dauert (Bild dekodieren, Desktop fragen, Dateidialog), laeuft in eigenen
//! Threads; die App holt die Ergebnisse jedes Bild mit `poll` ab. Ein 5K-JPEG zu dekodieren
//! kostet sonst hunderte Millisekunden Standbild.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperMode {
    None,
    File,
    Desktop,
    // Das Bild der Launcher-Szene (s. hero_scene.rs), mit den Texturen der eigenen Installation.
    Coral,
}

impl WallpaperMode {
    pub const ALL: [WallpaperMode; 4] = [WallpaperMode::None, WallpaperMode::File, WallpaperMode::Desktop, WallpaperMode::Coral];

    pub fn from_index(i: u8) -> Self {
        Self::ALL.get(i as usize).copied().unwrap_or(WallpaperMode::None)
    }

    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|&m| m == self).unwrap_or(0) as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            WallpaperMode::None => "Keins",
            WallpaperMode::File => "Eigenes Bild",
            WallpaperMode::Desktop => "Linux-Desktop",
            WallpaperMode::Coral => "Coral",
        }
    }
}

/// Wohin ein eigenes Bild kopiert wird. Ohne Endung: das Format erkennt der Dekoder am Inhalt.
pub fn stored_path(config_dir: &Path) -> PathBuf {
    config_dir.join("wallpaper")
}

/// Das fertig dekodierte, schon auf Bildschirmgroesse verkleinerte Bild (RGBA8, sRGB).
#[derive(Clone)]
pub struct Decoded {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Groesster Texturrand, den wgpu ohne Sonder-Limits garantiert.
const MAX_TEXTURE_SIDE: u32 = 8192;

/// Auf welche Groesse ein Bild verkleinert wird, damit es `ziel` gerade noch ganz bedeckt
/// (wie "Zoom" beim Desktop-Hintergrund). Nie vergroessert -- das macht die GPU beim
/// Zeichnen ohnehin, und ein groesseres Bild kostet nur Speicher.
pub fn cover_size(bild: (u32, u32), ziel: (u32, u32)) -> (u32, u32) {
    let (w, h) = (bild.0.max(1) as f64, bild.1.max(1) as f64);
    let deckend = (ziel.0.max(1) as f64 / w).max(ziel.1.max(1) as f64 / h).min(1.0);
    let grenze = (MAX_TEXTURE_SIDE as f64 / w.max(h)).min(1.0);
    let s = deckend.min(grenze);
    (((w * s).ceil() as u32).max(1), ((h * s).ceil() as u32).max(1))
}

/// Der Bildausschnitt (u0, v0, u1, v1), der einen Bildschirm der Groesse `schirm` mittig
/// ganz bedeckt, ohne das Bild zu verzerren -- was uebersteht, wird links/rechts bzw.
/// oben/unten gleichmaessig abgeschnitten.
pub fn cover_uv(bild: (u32, u32), schirm: (f32, f32)) -> [f32; 4] {
    let bild_seite = bild.0.max(1) as f32 / bild.1.max(1) as f32;
    let schirm_seite = schirm.0.max(1.0) / schirm.1.max(1.0);
    if bild_seite > schirm_seite {
        // Bild breiter als der Schirm: seitlich abschneiden.
        let anteil = schirm_seite / bild_seite;
        let rand = (1.0 - anteil) / 2.0;
        [rand, 0.0, 1.0 - rand, 1.0]
    } else {
        let anteil = bild_seite / schirm_seite;
        let rand = (1.0 - anteil) / 2.0;
        [0.0, rand, 1.0, 1.0 - rand]
    }
}

pub fn decode(path: &Path, ziel: (u32, u32)) -> Result<Decoded, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bild = image::load_from_memory(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = cover_size((bild.width(), bild.height()), ziel);
    let bild = if (w, h) == (bild.width(), bild.height()) {
        bild.into_rgba8()
    } else {
        image::imageops::resize(&bild.into_rgba8(), w, h, image::imageops::FilterType::CatmullRom)
    };
    Ok(Decoded { width: bild.width(), height: bild.height(), rgba: bild.into_raw() })
}

// --- Das Hintergrundbild des Desktops ----------------------------------------------------

/// Pfad des aktuellen Desktop-Hintergrunds, oder None, wenn die Arbeitsumgebung keinen
/// verraet. Gefragt wird der Reihe nach, was die verbreiteten Umgebungen benutzen; die
/// laufende (XDG_CURRENT_DESKTOP) zuerst, damit etwa ein uebrig gebliebener GNOME-Eintrag
/// unter Cinnamon nicht gewinnt.
pub fn desktop_wallpaper() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_wallpaper()
    }
    #[cfg(not(windows))]
    {
        linux_wallpaper()
    }
}

// Windows: der Pfad steht in der Registry (HKCU\Control Panel\Desktop, Wert WallPaper).
// Fehlt die Datei dort inzwischen (verschoben, geloescht) oder ist der Wert leer, bleibt die
// Kopie, die Windows selbst vom aktuellen Hintergrund anlegt: TranscodedWallpaper (ein Bild
// ohne Dateiendung -- decode erkennt das Format am Inhalt).
#[cfg(windows)]
fn windows_wallpaper() -> Option<PathBuf> {
    let aus_registry = befehl("reg", &["query", r"HKCU\Control Panel\Desktop", "/v", "WallPaper"]).and_then(|text| {
        // Zeile: "    WallPaper    REG_SZ    C:\pfad\bild.jpg"
        let zeile = text.lines().find(|l| l.trim_start().starts_with("WallPaper"))?;
        let (_, wert) = zeile.split_once("REG_SZ")?;
        let wert = wert.trim();
        (!wert.is_empty()).then(|| PathBuf::from(wert))
    });
    let transcoded = std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("Microsoft").join("Windows").join("Themes").join("TranscodedWallpaper"));
    aus_registry.filter(|p| p.is_file()).or(transcoded.filter(|p| p.is_file()))
}

#[cfg(not(windows))]
fn linux_wallpaper() -> Option<PathBuf> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase();
    let mut reihenfolge: Vec<fn() -> Option<PathBuf>> = Vec::new();
    if desktop.contains("cinnamon") {
        reihenfolge.push(cinnamon);
    }
    if desktop.contains("kde") {
        reihenfolge.push(kde);
    }
    if desktop.contains("xfce") {
        reihenfolge.push(xfce);
    }
    if desktop.contains("mate") {
        reihenfolge.push(mate);
    }
    reihenfolge.extend([gnome as fn() -> Option<PathBuf>, cinnamon, mate, kde, xfce]);
    reihenfolge.into_iter().find_map(|f| f().filter(|p| p.is_file()))
}

fn befehl(prog: &str, args: &[&str]) -> Option<String> {
    let out = hintergrund_befehl(prog).args(args).stderr(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// Ein Hilfsprogramm starten, ohne dass unter Windows kurz ein Konsolenfenster aufblitzt.
fn hintergrund_befehl(prog: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(prog);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

#[cfg_attr(windows, allow(dead_code))]
fn gsettings(schema: &str, key: &str) -> Option<String> {
    let wert = befehl("gsettings", &["get", schema, key])?;
    // gsettings gibt GVariant-Text aus: 'file:///pfad' (einfache Anfuehrungszeichen).
    let wert = wert.trim().trim_matches('\'').to_string();
    (!wert.is_empty()).then_some(wert)
}

#[cfg_attr(windows, allow(dead_code))]
fn cinnamon() -> Option<PathBuf> {
    gsettings("org.cinnamon.desktop.background", "picture-uri").and_then(|u| uri_to_path(&u))
}

#[cfg_attr(windows, allow(dead_code))]
fn gnome() -> Option<PathBuf> {
    // Im dunklen Stil zeigt GNOME ein eigenes Bild.
    let dunkel = gsettings("org.gnome.desktop.interface", "color-scheme").is_some_and(|s| s == "prefer-dark");
    let dunkles = dunkel.then(|| gsettings("org.gnome.desktop.background", "picture-uri-dark")).flatten();
    dunkles.or_else(|| gsettings("org.gnome.desktop.background", "picture-uri")).and_then(|u| uri_to_path(&u))
}

#[cfg_attr(windows, allow(dead_code))]
fn mate() -> Option<PathBuf> {
    gsettings("org.mate.background", "picture-filename").map(PathBuf::from)
}

#[cfg_attr(windows, allow(dead_code))]
fn xfce() -> Option<PathBuf> {
    // Der erste Monitor/Arbeitsbereich, der ein Bild hat.
    let liste = befehl("xfconf-query", &["-c", "xfce4-desktop", "-l"])?;
    liste.lines().filter(|l| l.ends_with("/last-image")).find_map(|prop| befehl("xfconf-query", &["-c", "xfce4-desktop", "-p", prop]).map(PathBuf::from))
}

#[cfg_attr(windows, allow(dead_code))]
fn kde() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let conf = PathBuf::from(home).join(".config").join("plasma-org.kde.plasma.desktop-appletsrc");
    let text = std::fs::read_to_string(conf).ok()?;
    kde_image(&text)
}

// Plasma schreibt je Bildschirm einen Abschnitt [...][Wallpaper][org.kde.image][General]
// mit `Image=file:///...`. Der erste gewinnt.
#[cfg_attr(windows, allow(dead_code))]
fn kde_image(conf: &str) -> Option<PathBuf> {
    let mut im_bildabschnitt = false;
    for zeile in conf.lines() {
        let zeile = zeile.trim();
        if zeile.starts_with('[') {
            im_bildabschnitt = zeile.contains("[Wallpaper][org.kde.image][General]");
            continue;
        }
        if im_bildabschnitt {
            if let Some(wert) = zeile.strip_prefix("Image=") {
                return uri_to_path(wert).or_else(|| Some(PathBuf::from(wert)));
            }
        }
    }
    None
}

/// `file:///pfad%20mit%20leerzeichen` -> `/pfad mit leerzeichen`. Andere Schemata -> None;
/// ein schlichter absoluter Pfad bleibt, wie er ist.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    if uri.starts_with('/') {
        return Some(PathBuf::from(uri));
    }
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
    }
    // Ausserhalb von Unix sind Pfade kein Byte-Strom -- dort muss es gueltiges UTF-8 sein.
    #[cfg(not(unix))]
    {
        String::from_utf8(out).ok().map(PathBuf::from)
    }
}

// --- Dateiauswahl ------------------------------------------------------------------------

/// Den Dateidialog der Arbeitsumgebung oeffnen (zenity, sonst kdialog). Blockiert, bis
/// gewaehlt oder abgebrochen wurde -- nur aus einem eigenen Thread aufrufen.
/// Ok(None) = abgebrochen.
fn pick_file() -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    {
        // Der Standard-Dialog von Windows, ueber PowerShell (kein zusaetzliches Crate).
        let skript = "Add-Type -AssemblyName System.Windows.Forms; \
            $d = New-Object System.Windows.Forms.OpenFileDialog; \
            $d.Title = 'Hintergrundbild waehlen'; \
            $d.Filter = 'Bilder|*.png;*.jpg;*.jpeg;*.webp;*.bmp|Alle Dateien|*.*'; \
            if ($d.ShowDialog() -eq 'OK') { [Console]::OutputEncoding = [Text.Encoding]::UTF8; $d.FileName }";
        let out = hintergrund_befehl("powershell")
            .args(["-NoProfile", "-STA", "-Command", skript])
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("Dateidialog: {e}"))?;
        let pfad = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return Ok((!pfad.is_empty()).then(|| PathBuf::from(pfad)));
    }
    #[allow(unreachable_code)]
    let titel = "Hintergrundbild wählen";
    let muster = "*.png *.jpg *.jpeg *.webp *.bmp *.PNG *.JPG *.JPEG *.WEBP *.BMP";
    let versuche: [(&str, Vec<String>); 2] = [
        (
            "zenity",
            vec!["--file-selection".into(), format!("--title={titel}"), format!("--file-filter=Bilder | {muster}"), "--file-filter=Alle Dateien | *".into()],
        ),
        ("kdialog", vec!["--title".into(), titel.into(), "--getopenfilename".into(), std::env::var("HOME").unwrap_or_default(), format!("Bilder ({muster})")]),
    ];
    for (prog, args) in versuche {
        let Ok(out) = std::process::Command::new(prog).args(&args).stderr(std::process::Stdio::null()).output() else { continue };
        // Beide melden Abbrechen mit Exit-Code 1 und leerer Ausgabe.
        let pfad = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return Ok((out.status.success() && !pfad.is_empty()).then(|| PathBuf::from(pfad)));
    }
    Err("Kein Dateidialog gefunden (zenity oder kdialog installieren)".into())
}

/// Ein gewaehltes Bild pruefen und nach `ziel` kopieren. Erst in eine Nachbardatei, dann
/// umbenennen: ein kaputtes Bild ueberschreibt nie das bisherige.
fn import(quelle: &Path, ziel: &Path) -> Result<(), String> {
    let bytes = std::fs::read(quelle).map_err(|e| format!("{}: {e}", quelle.display()))?;
    image::guess_format(&bytes).map_err(|_| format!("{} ist kein unterstütztes Bild", quelle.display()))?;
    if let Some(ordner) = ziel.parent() {
        std::fs::create_dir_all(ordner).map_err(|e| e.to_string())?;
    }
    let tmp = ziel.with_extension("neu");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, ziel).map_err(|e| e.to_string())
}

// --- Hintergrund-Arbeit ------------------------------------------------------------------

pub enum Event {
    /// Neues Bild fertig dekodiert.
    Loaded(Decoded),
    /// Die Quelle hat kein Bild (Modus "Keins", Desktop ohne Hintergrund, noch nichts gewaehlt).
    Cleared,
    /// Laden ging schief -- Meldung fuer den Nutzer.
    Failed(String),
    /// Ein Bild wurde im Dialog gewaehlt und in den Einstellungsordner kopiert.
    Imported,
    /// Import fehlgeschlagen.
    ImportFailed(String),
    /// Dialog ohne Auswahl geschlossen.
    PickCancelled,
}

/// Welche Datei zuletzt geladen wurde, samt Aenderungszeit -- ein erneutes Nachsehen
/// dekodiert nur, wenn sich eines davon geaendert hat.
type Stand = Option<(PathBuf, Option<SystemTime>)>;

pub struct Loader {
    tx: mpsc::Sender<(u64, Event, Stand)>,
    rx: mpsc::Receiver<(u64, Event, Stand)>,
    /// Jede Anfrage bekommt eine Nummer; Antworten auf aeltere werden verworfen (schnell
    /// zweimal umgeschaltet -> nur das Ergebnis der letzten Wahl zaehlt).
    generation: u64,
    stand: Stand,
    picking: bool,
}

impl Default for Loader {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Loader { tx, rx, generation: 0, stand: None, picking: false }
    }
}

impl Loader {
    /// Die Quelle (neu) laden. `force = false`: nur, wenn sich Pfad oder Datei geaendert
    /// haben -- fuer das regelmaessige Nachsehen beim Desktop-Hintergrund.
    // `coral`: wo das Bild der Launcher-Szene liegt (s. hero_scene::path).
    pub fn request(&mut self, mode: WallpaperMode, stored: Option<PathBuf>, coral: Option<PathBuf>, ziel: (u32, u32), force: bool) {
        self.generation += 1;
        let generation = self.generation;
        let bisher = if force { None } else { self.stand.clone() };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let pfad = match mode {
                WallpaperMode::None => None,
                WallpaperMode::File => stored.filter(|p| p.is_file()),
                WallpaperMode::Desktop => desktop_wallpaper(),
                WallpaperMode::Coral => coral.filter(|p| p.is_file()),
            };
            let Some(pfad) = pfad else {
                let _ = tx.send((generation, Event::Cleared, None));
                return;
            };
            let geaendert = std::fs::metadata(&pfad).and_then(|m| m.modified()).ok();
            let stand = Some((pfad.clone(), geaendert));
            if !force && stand == bisher {
                return;
            }
            let event = match decode(&pfad, ziel) {
                Ok(d) => Event::Loaded(d),
                Err(e) => Event::Failed(e),
            };
            let _ = tx.send((generation, event, stand));
        });
    }

    /// Dateidialog oeffnen und das Gewaehlte nach `stored` kopieren.
    pub fn pick(&mut self, stored: PathBuf) {
        if self.picking {
            return;
        }
        self.picking = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let event = match pick_file() {
                Ok(Some(quelle)) => match import(&quelle, &stored) {
                    Ok(()) => Event::Imported,
                    Err(e) => Event::ImportFailed(e),
                },
                Ok(None) => Event::PickCancelled,
                Err(e) => Event::ImportFailed(e),
            };
            // Ein Dialog ist keine Ladeanfrage, die Generation zaehlt hier nicht (s. poll).
            let _ = tx.send((0, event, None));
        });
    }

    /// Die Datei, aus der das aktuelle Bild stammt.
    pub fn current_path(&self) -> Option<&Path> {
        self.stand.as_ref().map(|(p, _)| p.as_path())
    }

    pub fn picking(&self) -> bool {
        self.picking
    }

    /// Fertige Ergebnisse abholen.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok((generation, event, stand)) = self.rx.try_recv() {
            match event {
                Event::Imported | Event::ImportFailed(_) | Event::PickCancelled => {
                    self.picking = false;
                    out.push(event);
                }
                _ if generation != self.generation => {}
                _ => {
                    self.stand = stand;
                    out.push(event);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verkleinert_nur_bis_der_schirm_bedeckt_ist() {
        // 5120x2880 auf 2560x1440: genau halbe Groesse.
        assert_eq!(cover_size((5120, 2880), (2560, 1440)), (2560, 1440));
        // Hochformat auf Querformat-Schirm: die Breite bestimmt.
        assert_eq!(cover_size((2000, 4000), (1000, 500)), (1000, 2000));
        // Kleiner als der Schirm: bleibt.
        assert_eq!(cover_size((800, 600), (1920, 1080)), (800, 600));
    }

    #[test]
    fn riesiges_panorama_passt_in_eine_textur() {
        let (w, h) = cover_size((20000, 500), (1920, 1080));
        assert!(w <= MAX_TEXTURE_SIDE && h >= 1);
    }

    #[test]
    fn ausschnitt_schneidet_gleichmaessig_ab() {
        // Breites Bild auf 16:9: links und rechts gleich viel weg.
        let [u0, v0, u1, v1] = cover_uv((3200, 900), (1600.0, 900.0));
        assert_eq!((v0, v1), (0.0, 1.0));
        assert!((u0 - 0.25).abs() < 1e-6 && (u1 - 0.75).abs() < 1e-6);
        // Hohes Bild: oben und unten.
        let [u0, v0, u1, v1] = cover_uv((1000, 1000), (1000.0, 500.0));
        assert_eq!((u0, u1), (0.0, 1.0));
        assert!((v0 - 0.25).abs() < 1e-6 && (v1 - 0.75).abs() < 1e-6);
    }

    #[test]
    fn datei_uri_wird_zum_pfad() {
        assert_eq!(uri_to_path("file:///home/a/Bilder/mein%20bild.jpg"), Some(PathBuf::from("/home/a/Bilder/mein bild.jpg")));
        assert_eq!(uri_to_path("/usr/share/x.png"), Some(PathBuf::from("/usr/share/x.png")));
        assert_eq!(uri_to_path("https://example.org/x.png"), None);
        // Kaputtes Escape am Ende bleibt stehen, statt abzustuerzen.
        assert_eq!(uri_to_path("file:///a%2"), Some(PathBuf::from("/a%2")));
    }

    #[test]
    fn kde_bild_aus_der_plasma_konfiguration() {
        let conf = "[Containments][1][General]\nfoo=bar\n\n[Containments][1][Wallpaper][org.kde.image][General]\nImage=file:///usr/share/wallpapers/Next/\nSlidePaths=x\n";
        assert_eq!(kde_image(conf), Some(PathBuf::from("/usr/share/wallpapers/Next/")));
        assert_eq!(kde_image("[Containments][1][General]\nImage=file:///falsch.png\n"), None);
    }

    #[test]
    fn modus_ueberlebt_speichern_und_laden() {
        for m in WallpaperMode::ALL {
            assert_eq!(WallpaperMode::from_index(m.index()), m);
        }
        assert_eq!(WallpaperMode::from_index(99), WallpaperMode::None);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    // Echte Arbeitsumgebung: `cargo test -p app -- --ignored desktop_hintergrund`.
    #[test]
    #[ignore]
    fn desktop_hintergrund_wird_gefunden_und_dekodiert() {
        let pfad = desktop_wallpaper().expect("kein Desktop-Hintergrund gefunden");
        let t = std::time::Instant::now();
        let bild = decode(&pfad, (2560, 1440)).expect("dekodieren");
        eprintln!("{} -> {}x{} in {:?}", pfad.display(), bild.width, bild.height, t.elapsed());
        assert_eq!(bild.rgba.len(), (bild.width * bild.height * 4) as usize);
    }
}
