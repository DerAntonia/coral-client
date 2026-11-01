// Zaubertisch-Buch, Puppen-Kamera, Item-Arten, externe Programme, Skins und Sprache.

use crate::*;

// Skins/Resourcepacks liegen BEWUSST NICHT unter ~/.config (das ist fuer Einstellungen, ein
// Skin ist keine Konfiguration), sondern direkt unter ~/coral-client/skins --
// analog zu Vanillas .minecraft/resourcepacks: jeder Unterordner darin (mit eigenem
// "assets/"-Verzeichnis) ist ein waehlbares Pack, s. list_available_skins.
// Beide Arme auf einmal: wer etwas haelt, hebt den jeweiligen Arm (s.
// world::apply_item_arm_pose). Die Haupthand ist der rechte Arm -- welchen Arm ein fremder
// Spieler als Haupthand fuehrt, steht in seinen Entity-Metadaten, die dieser Client noch
// nicht liest; Vanillas Standard ist rechts.
// Sicht- und Projektionsmatrix fuer die Spielerfigur im Inventar (s. App::build_doll).
//
// ORTHOGRAFISCH, nicht perspektivisch: Vanillas drawEntity baut keine Kamera, sondern skaliert
// das Modell schlicht mit (size, size, -size) in den GUI-Raum. Die Figur
// hat dadurch keine Fluchtpunkte -- ein vorgestreckter Arm ist genauso gross wie einer hinten.
//
// Gerechnet wird direkt von Modellmetern nach NDC, statt Vanillas Matrix-Stapel Zeile fuer
// Zeile nachzubauen. Der Stapel dort enthaelt eine Punktspiegelung (das `-size` auf Z
// zusammen mit `rotateZ(PI)`), die zusammen mit der y-nach-unten-Projektion des GUI netto auf
// eine halbe Drehung um Y hinauslaeuft -- die steckt hier im Koerperwinkel (s. build_doll)
// statt in der Matrix. Das spart eine spiegelnde Transformation, und mit ihr die Frage, ob das
// Backface-Culling der Pipeline dann noch richtig herum greift.
//
// BEWUSST WEGGELASSEN: Vanillas zusaetzliche Kameraneigung (`rotateX(j * 20°)`, die die ganze
// Figur mitkippt, wenn der Zeiger ueber oder unter ihr steht). Der Haupteffekt -- die Figur
// schaut dem Zeiger nach -- steckt schon im Kopfwinkel; die Neigung ist eine Zugabe, deren
// Vorzeichen sich nur im Bild pruefen laesst, und ein falsches Vorzeichen waere schlechter als
// keine Neigung.
// Das Buch im Zaubertisch-Bildschirm (EnchantmentScreen.tickBook): Aufklappen, sobald
// eine Option angeboten wird, und Blaettern, wenn ein anderes Item im Slot liegt. Alle Werte
// je Tick; gezeichnet wird zwischen `prev_*` und dem aktuellen Wert interpoliert.
#[derive(Default)]
pub(crate) struct BookAnim {
    // Blaetter-Stand (pageAngle) und wohin er laeuft (pageRotationSpeed-Ziel, flipT).
    pub(crate) flip: f32,
    pub(crate) prev_flip: f32,
    pub(crate) flip_target: f32,
    pub(crate) flip_speed: f32,
    // Aufgeklappt 0..1.
    pub(crate) open: f32,
    pub(crate) prev_open: f32,
    // Das Item, bei dessen Wechsel geblaettert wird (Item-ID und Anzahl; die Components
    // spielen fuers Blaettern keine Rolle).
    pub(crate) last_item: Option<(u32, i32)>,
    pub(crate) rng: u32,
}

impl BookAnim {
    // Kleiner Zufallsgenerator (xorshift) fuer die Blaetter-Ziele -- welche Seite genau
    // aufschlaegt, ist reine Zierde.
    pub(crate) const fn zufall(&mut self, n: u32) -> i32 {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng % n) as i32
    }

    pub(crate) fn tick(&mut self, item: Option<(u32, i32)>, any_option: bool) {
        if item != self.last_item {
            self.last_item = item;
            // Ein neues Ziel, mindestens eine ganze Seite vom jetzigen Stand entfernt.
            loop {
                self.flip_target += (self.zufall(4) - self.zufall(4)) as f32;
                if !(self.flip <= self.flip_target + 1.0 && self.flip >= self.flip_target - 1.0) {
                    break;
                }
            }
        }
        self.prev_flip = self.flip;
        self.prev_open = self.open;
        self.open = (self.open + if any_option { 0.2 } else { -0.2 }).clamp(0.0, 1.0);
        let ziel = ((self.flip_target - self.flip) * 0.4).clamp(-0.2, 0.2);
        self.flip_speed += (ziel - self.flip_speed) * 0.9;
        self.flip += self.flip_speed;
    }

    // Interpolierter Stand fuer das Bild: (aufgeklappt, Blaetter-Stand).
    pub(crate) fn at(&self, t: f32) -> (f32, f32) {
        (self.prev_open + (self.open - self.prev_open) * t, self.prev_flip + (self.flip - self.prev_flip) * t)
    }
}

// Das aufgeschlagene Buch links oben im Zaubertisch (EnchantmentScreen.drawBook +
// BookModelGuiElementRenderer), als 3D-Geometrie fuer den Figuren-Durchgang. Gerechnet in
// Bildschirmpixeln mit Y nach OBEN (s. book_camera): Vanillas Kette laeuft im GUI-Raum mit Y
// nach unten; die eine Spiegelung vorneweg macht daraus wieder ein Rechtssystem, dann bleiben
// Umlaufsinn und Licht wie bei jedem anderen Modell.
pub(crate) fn enchant_book_vertices(model: &world::Model, tex: world::entity_model::TexRect, rect: inventory::DollRect, open: f32, flip: f32) -> Vec<world::BlockVertex> {
    use world::entity_model::Affine;
    let blatt = |versatz: f32| ((flip + versatz).fract() * 1.6 - 0.3).clamp(0.0, 1.0);
    let pose = world::entity_model::enchanting_book_pose(0.0, blatt(0.25), blatt(0.75), open);
    // Mitte oben des Buchfelds plus Vanillas fester Versatz von 17 GUI-Pixeln nach unten.
    let gui = rect.pixels_per_meter / 40.0;
    let cx = f32::midpoint(rect.x0, rect.x1);
    let cy = rect.y0 + 17.0 * gui;
    let f = rect.pixels_per_meter;
    let zu = 1.0 - open;
    let base = Affine::scale([1.0, -1.0, 1.0])
        .then(Affine::translation([cx, cy, 0.0]))
        .then(Affine::scale([f, f, -f]))
        .then(Affine::rotation_zyx(0.0, std::f32::consts::PI, 0.0))
        .then(Affine::rotation_zyx(25f32.to_radians(), 0.0, 0.0))
        .then(Affine::translation([zu * 0.2, zu * 0.1, zu * 0.25]))
        .then(Affine::rotation_zyx(0.0, (-zu * 90.0 - 90.0).to_radians(), 0.0))
        .then(Affine::rotation_zyx(std::f32::consts::PI, 0.0, 0.0));
    let mut out = Vec::with_capacity(256);
    world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
    out
}

// Ortho ueber den ganzen Bildschirm fuer das Buch: x/y in Pixeln (y nach oben, s.
// enchant_book_vertices), z grob in Vanillas GUI-Tiefe von +-1000; naeher (groesseres z) =
// kleinere Tiefe.
pub(crate) fn book_camera(screen: (f32, f32)) -> (glam::Mat4, glam::Mat4) {
    let (w, h) = screen;
    let proj = glam::Mat4::from_cols(
        glam::Vec4::new(2.0 / w, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 2.0 / h, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, -0.5 / 1000.0, 0.0),
        glam::Vec4::new(-1.0, 1.0, 0.5, 1.0),
    );
    (glam::Mat4::IDENTITY, proj)
}

// Die Zierzeile einer Zauber-Option: drei oder vier zufaellige "Woerter" aus Kleinbuchstaben,
// fest bestimmt durch den Samen, den der Server dem Tisch mitgibt (Datenplatz 3) -- dieselbe
// Auswahl ergibt dieselben Zeilen, wie in Vanilla. Die Woerter sind eigene Silbenketten: in
// der Zauberschrift gesetzt ist ohnehin keines lesbar.
pub(crate) fn enchant_phrase(seed: u32, option: usize) -> String {
    let mut x = seed ^ (option as u32 + 1).wrapping_mul(0x9E37_79B9);
    let mut zufall = move |n: u32| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x % n
    };
    const ANLAUT: &[u8] = b"bcdfghklmnprstvz";
    const VOKAL: &[u8] = b"aeiouy";
    let woerter = 3 + zufall(2);
    let mut out = String::new();
    for w in 0..woerter {
        if w > 0 {
            out.push(' ');
        }
        for _ in 0..=zufall(3) {
            out.push(ANLAUT[zufall(ANLAUT.len() as u32) as usize] as char);
            out.push(VOKAL[zufall(VOKAL.len() as u32) as usize] as char);
        }
        if zufall(2) == 0 {
            out.push(ANLAUT[zufall(ANLAUT.len() as u32) as usize] as char);
        }
    }
    out
}

pub(crate) fn doll_camera(rect: inventory::DollRect, screen: (f32, f32)) -> (glam::Mat4, glam::Mat4) {
    // Vanillas Vector3f(0, entity.getHeight() / 2 + 0.0625, 0): die Figur wird an ihrer
    // halben Koerperhoehe zentriert, nicht an den Fuessen.
    let center_y = physics::PLAYER_HEIGHT / 2.0 + 0.0625;
    let view = glam::Mat4::from_translation(glam::Vec3::new(0.0, -center_y, 0.0));

    let (w, h) = screen;
    let (cx, cy) = rect.center();
    let ppm = rect.pixels_per_meter;
    // Modellmeter -> NDC. x und y bleiben BEIDE positiv skaliert (NDC hat y nach oben, das
    // Modell auch) -- damit ist die Determinante des xy-Anteils positiv und die
    // Umlaufrichtung der Dreiecke bleibt dieselbe wie in der Welt.
    let sx = 2.0 * ppm / w;
    let sy = 2.0 * ppm / h;
    let tx = 2.0 * cx / w - 1.0;
    let ty = 1.0 - 2.0 * cy / h;
    // Tiefenbereich: vier Meter nach vorn und hinten reichen fuer eine Figur samt
    // ausgestrecktem Arm und Umhang reichlich. Naeher (groesseres z) = kleinere Tiefe.
    const DEPTH_RANGE: f32 = 4.0;
    let sz = -0.5 / DEPTH_RANGE;
    let proj = glam::Mat4::from_cols(
        glam::Vec4::new(sx, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, sy, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, sz, 0.0),
        glam::Vec4::new(tx, ty, 0.5, 1.0),
    );
    (view, proj)
}

// Spieler und Mannequins (die Spieler-NPCs seit 1.21.9, MannequinEntityRenderer ist ein
// PlayerEntityRenderer). Ein Mannequin ohne eigenes Profil ist schwarz-lila wie ein Spieler
// ohne Textur.
// 24.-26. Dezember (Weihnachtstruhen). Einmal beim Start bestimmt -- wer ueber Mitternacht
// in den 24. hinein spielt, sieht sie erst nach einem Neustart; Vanilla fragt ebenfalls nur,
// wenn der Truhen-Renderer entsteht.
pub(crate) fn is_christmas() -> bool {
    static WEIHNACHTEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WEIHNACHTEN.get_or_init(|| {
        let heute = chrono::Local::now();
        chrono::Datelike::month(&heute) == 12 && (24..=26).contains(&chrono::Datelike::day(&heute))
    })
}

pub(crate) fn is_player_like(entity_type: i32) -> bool {
    matches!(world::entity_type_name(entity_type), Some("player" | "mannequin"))
}

// Ein Speer (piercing_weapon) in dieser Hand? Ohne erreichbaren Stapel nach Namen.
// Welche Arme Vanilla schon belegt (rechts, links): Schild und Speer aus `taken`, dazu der Arm
// einer Benutzungshaltung und -- bei Bogen, Armbrust, Dreizack -- auch der andere.
pub(crate) const fn arms_taken(use_pose: Option<(bool, world::entity_model::UsePose)>, taken: (bool, bool)) -> (bool, bool) {
    match use_pose {
        Some((_, h)) if h.affects_offhand() => (true, true),
        Some((true, _)) => (true, taken.1),
        Some((false, _)) => (taken.0, true),
        None => taken,
    }
}

pub(crate) fn is_spear_item(item: Option<world::HeldItem>) -> bool {
    item.is_some_and(|h| world::entity::held_piercing(&h))
}

pub(crate) fn apply_item_arm_poses(pose: &mut world::Pose, main_hand: bool, off_hand: bool) {
    if main_hand {
        world::entity_model::apply_item_arm_pose(pose, true);
    }
    if off_hand {
        world::entity_model::apply_item_arm_pose(pose, false);
    }
}

// X11-Fensternummer unseres Fensters (unter Wayland und Windows None).
pub(crate) fn x11_window_id(window: &winit::window::Window) -> Option<u32> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok(),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

// Ordner oder Link mit dem Standardprogramm des Systems oeffnen: xdg-open unter Linux,
// unter Windows der Shell-Handler aus url.dll. Nicht `cmd /C start`: cmd liest das `&` in
// Links als Befehlstrenner und schneidet z.B. die Microsoft-Anmelde-URL nach dem ersten
// Parameter ab (AADSTS900144 "must contain ... 'scope'").
pub(crate) fn open_external_command(ziel: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(ziel);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(ziel);
        c
    }
}

pub(crate) fn skins_dir() -> Option<std::path::PathBuf> {
    Some(assets::user_dir()?.join("skins"))
}

// Namen aller Unterordner in skins_dir(), die wie ein Resourcepack aussehen (eigenes
// "assets/"-Verzeichnis) -- alphabetisch sortiert, damit die Liste nicht bei jedem Aufruf
// (jeder Frame, waehrend der Skin-Tab offen ist) durcheinanderspringt. Leere Liste, wenn der
// Ordner fehlt (kein Fehler -- einfach noch kein Pack hineingelegt).
pub(crate) fn list_available_skins() -> Vec<String> {
    let Some(dir) = skins_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().join("assets").is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        // Versteckt: u.a. der Zwischenordner, in den gerade ein Zip entpackt wird.
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

// Einstellungen -> Sprache anwenden: die Oberflaeche, die Locale fuer den Server und je
// Bereich Minecrafts Sprachdatei (protocol::nbt::set_area_language).
//
// Jede Tabelle beginnt mit en_us und legt die gewaehlte Sprache darueber -- wie Vanilla, das
// fehlende Schluessel (Latein ist unvollstaendig) auf Englisch zeigt statt roh. Alle Ebenen
// zusammengefuehrt: ein Resourcepack ergaenzt oder ueberschreibt einzelne Schluessel, statt
// die ganze Datei zu ersetzen.
pub(crate) fn apply_language_settings(installation: &mut assets::MinecraftInstallation, settings: &Settings) {
    // Das Theme gleich mit: dieselben Stellen (Start, Config-Wechsel, Zuruecksetzen) laden es.
    theme::apply(&settings.theme);
    i18n::set_ui_language(i18n::Language::from_index(settings.lang_ui));
    i18n::set_latin_macrons(settings.latin_macrons);
    i18n::set_english_loanwords(settings.english_loanwords);
    protocol::packets::set_locale(i18n::Language::from_index(settings.lang_chat).code());
    let mut geladen: Vec<(i18n::Language, std::sync::Arc<std::collections::HashMap<String, String>>)> = Vec::new();
    let mut tabelle_fuer = |lang: i18n::Language| {
        if let Some((_, t)) = geladen.iter().find(|(l, _)| *l == lang) {
            return t.clone();
        }
        let mut tabelle = std::collections::HashMap::new();
        let mut codes = vec!["en_us"];
        if lang.code() != "en_us" {
            codes.push(lang.code());
        }
        for code in codes {
            for ebene in installation.load_layers(&format!("minecraft/lang/{code}.json")) {
                if let Some(t) = protocol::nbt::parse_language_json(&ebene) {
                    tabelle.extend(t);
                }
            }
        }
        if lang == i18n::Language::Latin && settings.latin_macrons {
            add_latin_macrons(&mut tabelle);
        }
        let t = std::sync::Arc::new(tabelle);
        geladen.push((lang, t.clone()));
        t
    };
    use protocol::nbt::LangArea;
    for (area, index) in [(LangArea::Text, settings.lang_chat), (LangArea::Items, settings.lang_items), (LangArea::Effects, settings.lang_effects)] {
        protocol::nbt::set_area_language(area, tabelle_fuer(i18n::Language::from_index(index)));
    }
}

// Mojangs la_la.json kennt keine Laengen (keiner der gut 8000 Texte hat ein ā). Fuer Item-,
// Block-, Effekt- und Verzauberungsnamen setzt der Client sie nach: la_macrons.txt nennt die
// Woerter dieser Namen mit Laengen, ersetzt wird Wort fuer Wort (Gross-/Kleinschreibung wie
// im Original). Server-Texte bleiben, wie Mojang sie schreibt.
pub(crate) fn add_latin_macrons(tabelle: &mut std::collections::HashMap<String, String>) {
    static WOERTER: std::sync::OnceLock<std::collections::HashMap<String, &'static str>> = std::sync::OnceLock::new();
    let woerter = WOERTER.get_or_init(|| {
        include_str!("la_macrons.txt").split_whitespace().map(|w| (strip_macrons(w), w)).collect()
    });
    for (schluessel, text) in tabelle.iter_mut() {
        let name = ["item.minecraft.", "block.minecraft.", "effect.minecraft.", "enchantment.minecraft."]
            .iter()
            .any(|p| schluessel.strip_prefix(p).is_some_and(|rest| !rest.contains('.')));
        if name {
            *text = macronize_words(text, woerter);
        }
    }
}

pub(crate) fn strip_macrons(w: &str) -> String {
    w.chars()
        .map(|c| match c {
            'ā' => 'a',
            'ē' => 'e',
            'ī' => 'i',
            'ō' => 'o',
            'ū' => 'u',
            'ȳ' => 'y',
            'Ā' => 'A',
            'Ē' => 'E',
            'Ī' => 'I',
            'Ō' => 'O',
            'Ū' => 'U',
            c => c,
        })
        .collect()
}

pub(crate) fn macronize_words(text: &str, woerter: &std::collections::HashMap<String, &'static str>) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut wort = String::new();
    let flush = |wort: &mut String, out: &mut String| {
        if wort.is_empty() {
            return;
        }
        match woerter.get(&wort.to_lowercase()) {
            Some(lang) => {
                let gross = wort.chars().next().is_some_and(char::is_uppercase);
                let mut zeichen = lang.chars();
                if gross {
                    out.extend(zeichen.next().into_iter().flat_map(char::to_uppercase));
                }
                out.extend(zeichen);
            }
            None => out.push_str(wort),
        }
        wort.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphabetic() {
            wort.push(c);
        } else {
            flush(&mut wort, &mut out);
            out.push(c);
        }
    }
    flush(&mut wort, &mut out);
    out
}

// Trenner der gestapelten Packs in der Einstellungsdatei. '|' darf unter Windows in keinem
// Ordnernamen stehen und kommt unter Linux praktisch nie vor.
pub(crate) const SKIN_STACK_SEP: &str = "|";

pub(crate) fn parse_skin_stack(value: &str) -> Vec<String> {
    let mut packs: Vec<String> = Vec::new();
    for name in value.split(SKIN_STACK_SEP).map(str::trim).filter(|n| !n.is_empty()) {
        if !packs.iter().any(|p| p == name) {
            packs.push(name.to_string());
        }
    }
    packs
}

// Anzeige eines Stapels, oberstes Pack zuerst.
pub(crate) fn skin_stack_label(stack: &[String]) -> String {
    let echte: Vec<&str> = stack.iter().filter(|n| !skin_tab::is_marker(n)).map(String::as_str).collect();
    if echte.is_empty() { "Kein Pack".to_string() } else { echte.join(" + ") }
}

// Liegt im Stapel ueberhaupt ein eigenes Pack (nicht nur Vanilla und der Server-Eintrag)?
pub(crate) fn skin_stack_has_packs(stack: &[String]) -> bool {
    stack.iter().any(|n| !skin_tab::is_marker(n))
}

// Die Packs eines Stapels mit dem, was in ihnen ausgeschaltet ist (s. Settings::skin_exclusions),
// getrennt in ueber und unter Vanilla. Am Server-Eintrag stehen die Packs des Servers.
pub(crate) fn skin_stack_dirs(
    stack: &[String],
    exclusions: &std::collections::BTreeMap<String, Vec<String>>,
    server: &[(std::path::PathBuf, Vec<String>)],
) -> (Vec<assets::PackDir>, Vec<assets::PackDir>) {
    let dir = skins_dir();
    let (mut oben, mut unten) = (Vec::new(), Vec::new());
    let mut unter_vanilla = false;
    for name in stack {
        if name == skin_tab::VANILLA {
            unter_vanilla = true;
            continue;
        }
        let ziel = if unter_vanilla { &mut unten } else { &mut oben };
        if name == skin_tab::SERVER {
            ziel.extend(server.iter().map(|(p, aus)| assets::PackDir { path: p.clone(), excluded: aus.clone() }));
        } else if let Some(d) = &dir {
            ziel.push(assets::PackDir { path: d.join(name), excluded: exclusions.get(name).cloned().unwrap_or_default() });
        }
    }
    (oben, unten)
}
