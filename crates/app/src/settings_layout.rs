// Einstellungsseiten: Zeilen, Abschnitte, Scrollen, Clip und Ausblenden, Steuerung,
// HUD-Konfiguration.

use crate::*;

// Zeilen der Lichtstaerken-Seite: oben Sonnenlicht und "alle Lichtquellen", dann alle
// leuchtenden Bloecke (world::light::light_source_blocks), einmal ermittelt -- die
// Block-Registry aendert sich zur Laufzeit nicht.
pub(crate) fn light_source_blocks() -> &'static [&'static str] {
    static LISTE: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    LISTE.get_or_init(|| [world::light::SUNLIGHT, world::light::ALL_LIGHTS].into_iter().chain(world::light::light_source_blocks()).collect())
}

// Anzeigename eines Blocks: Uebersetzung, sonst die ID. Dazu die beiden Sonderzeilen.
pub(crate) fn block_display_name(id: &str) -> String {
    match id {
        world::light::SUNLIGHT => "Sonnenlicht".to_string(),
        world::light::ALL_LIGHTS => "Alle Lichtquellen".to_string(),
        _ => protocol::nbt::translate(&format!("block.minecraft.{id}")).unwrap_or_else(|| id.to_string()),
    }
}

// Weichheit der Blocklicht-Schatten (scheinbare Groesse der Quellen) in ganzen Grad.
pub(crate) const BLOCK_SOFTNESS_MAX_DEG: u32 = 30;
// Stufen der Lichtkarten-Ansicht (Settings::shader_lightmap_debug).
pub(crate) const LIGHTMAP_DEBUG_NAMES: [&str; 4] = ["Aus", "Licht", "Abweichung", "Art"];
pub(crate) const BLOCK_SOFTNESS_STEP_DEG: u32 = 1;

// Renderpfad (Settings::render_path): Index in RENDER_PATH_NAMES.
#[allow(dead_code)]
pub(crate) const RENDER_PATH_NAMES: [&str; 2] = ["PvP", "Shader"];
// Riss-Reserve (Settings::tear_reserve_us), s. App::adapt_tear_lead.
pub(crate) const TEAR_RESERVE_MIN_US: i32 = 0;
pub(crate) const TEAR_RESERVE_MAX_US: i32 = 3000;
pub(crate) const TEAR_RESERVE_STEP_US: i32 = 50;
pub(crate) const TEAR_RESERVE_DEFAULT_US: i32 = 300;
pub(crate) const TEAR_RESERVE_ROW: usize = 11;
// Riss-Timing (Settings::tear_timing_us): Verschiebung des Bildwechsels in Mikrosekunden,
// negativ = frueher. Ohne Hardware-Flip kopiert der X-Server das Bild erst ~1,5 ms nach dem
// Present auf den Schirm -- wie lange, haengt an X-Server, Treiber und Monitor und ist von hier
// nicht messbar, also stellt man es ein. +-8 ms deckt bei 60 Hz das ganze Bild ab.
pub(crate) const TEAR_TIMING_MIN_US: i32 = -8000;
pub(crate) const TEAR_TIMING_MAX_US: i32 = 8000;
pub(crate) const TEAR_TIMING_STEP_US: i32 = 50;
pub(crate) const TEAR_TIMING_DEFAULT_US: i32 = 0;
pub(crate) const TEAR_TIMING_ROW: usize = 10;
// Scanout-Timing (Settings::tear_position, s. App::tear_target_phase): 0 = aus, 1 = jeden
// Bildwechsel knapp vor die Austastluecke legen.
//
// Frueher auch "Oben" und "Austastluecke". Gemessen: unsere Present-Aufrufe kamen in allen Modi
// auf +-5 us puenktlich, aber X-Server/Treiber setzen einen Bildwechsel, der in der
// Austastluecke oder kurz danach verlangt wird, erst irgendwo im oberen Viertel um -- und bei
// weiterem Verschieben unregelmaessig (der Riss sprang wild). Zuverlaessig ist nur der Wechsel
// knapp VOR der Luecke: er landet unsichtbar darin.
pub(crate) const TEAR_POSITION_ROW: usize = 9;
// Maus-Empfindlichkeit ist echte Steuerung, keine Performance-Einstellung -- eigene Zeile
// (Zeile 0) auf dem Steuerung-Screen, s. build_settings_controls_tab.
pub(crate) const SENSITIVITY_TOOLTIP: &str =
    "Wie stark Mausbewegung die KAMERA dreht (Radiant pro Pixel Delta, angezeigt als Tausendfaches). Gilt nur im Spiel, nicht für den Mauszeiger in den Menüs -- dafür gibt es die Zeile darunter. Zahl anklicken für einen eigenen Wert, bis auf vier Nachkommastellen genau.";
pub(crate) const CURSOR_SENSITIVITY_TOOLTIP: &str =
    "Wie schnell der MAUSZEIGER in Menüs und im Inventar läuft -- ein Faktor auf die gelieferte Mausbewegung. 1 = ein Bildschirmpixel je geliefertem Pixel, 0.5 = halb so schnell (feineres Zielen auf kleine Felder), 2 = doppelt so schnell (schneller quer über einen großen Bildschirm). Völlig unabhängig von der Kamera-Empfindlichkeit darüber.";

// Sichtfeld-Grenzen (vertikal, Grad). Untergrenze und Default wie Vanilla (dessen
// Schieberegler ist ValidatingIntSliderCallbacks(30, 110), s. GameOptions im 
// 1.21.11-Client); nach oben bewusst weiter als Vanilla, weil die Warp-Projektionen
// (Panini/Stereografisch/Zylindrisch) davon leben.
//
// 180 ist dabei eine HARTE Grenze der jetzigen Umsetzung, keine Geschmacksfrage: das
// Kamera-Uniform reicht tan(fov/2) an den Shader (s. render::block.wgsl, params.y) und
// perspective_rh baut die Matrix ueber denselben Term. Bei 180 Grad ist tan(90) unendlich,
// darueber negativ -- das Bild klappt um. Ein echtes Rundumbild (360) waere deshalb kein
// groesserer Wert hier, sondern eine andere Projektion: Geometrie HINTER der Kamera muesste
// mitgezeichnet werden, was project() derzeit ausdruecklich nicht tut (fuer d <= 0 faellt es
// auf die Standardprojektion zurueck, und w = d waere dort <= 0).
pub(crate) const FOV_MIN: f32 = 30.0;
pub(crate) const FOV_MAX: f32 = 170.0;
pub(crate) const FOV_STEP: f32 = 5.0;
// Projektionsmodi in fester Reihenfolge -- Index = Settings::projection_mode = params.x im
// Shader (block.wgsl). Reihenfolge NICHT umsortieren, ohne den Shader anzupassen.
pub(crate) const PROJECTION_NAMES: [&str; 4] = ["Normal", "Panini", "Stereografisch", "Zylindrisch"];
// Vanillas ParticlesMode, Index = Settings::particles_mode (s. particles::ParticleMode).
pub(crate) const PARTICLE_MODE_NAMES: [&str; 3] = ["Alle", "Verringert", "Minimal"];
// Partikel-Anpassung (MenuScreen::SettingsParticles): Dauer und Tempo in Prozent von Vanilla,
// in Stufen; Deckkraft in Zehnern. Unter 10 % Deckkraft verwirft der Shader ohnehin jedes
// Pixel (Schwelle 0.1), dafuer gibt es "Aus".
pub(crate) const PARTICLE_LIFETIME_STEPS: [u32; 11] = [10, 25, 50, 75, 100, 125, 150, 200, 300, 400, 500];
pub(crate) const PARTICLE_SPEED_STEPS: [u32; 10] = [10, 25, 50, 75, 100, 125, 150, 200, 300, 400];
pub(crate) const PARTICLE_OPACITY_STEP: u32 = 10;
// Menge bei Schwall-Arten (Prozent der Vanilla-Anzahl) und Intervall bei verweilenden (Prozent
// des Vanilla-Abstands -- 200 % = halb so oft).
pub(crate) const PARTICLE_AMOUNT_STEPS: [u32; 10] = [10, 25, 50, 75, 100, 125, 150, 200, 250, 300];
pub(crate) const PARTICLE_INTERVAL_STEPS: [u32; 10] = [25, 50, 75, 100, 125, 150, 200, 300, 400, 500];
// Spalten einer Partikelzeile: Schalter "Zeigen", vier Regler (Dauer, Deckkraft, Tempo, Menge
// bzw. Intervall) mit halb so langer Bahn wie sonst, dann der Schalter "Auf einmal". Der
// Schalter steht weiter links als SETTINGS_CTRL_X, damit alles neben die Namen passt.
pub(crate) const PARTICLE_TOGGLE_X: f32 = 340.0;
pub(crate) const PARTICLE_TOGGLE_W: f32 = 90.0;
pub(crate) const PARTICLE_VALUE_W: f32 = 100.0;
pub(crate) const PARTICLE_TRACK_W: f32 = 172.0;
pub(crate) const PARTICLE_SLIDER_W: f32 = 6.0 + PARTICLE_TRACK_W + SLIDER_BOX_GAP + 52.0;
pub(crate) const PARTICLE_COLUMN_W: f32 = 272.0;
// Spalte des Schalters "Auf einmal" (nach den vier Reglern).
pub(crate) const PARTICLE_ALL_AT_ONCE_COL: usize = 4;
// Naechste Stufe in einer aufsteigenden Liste; ein Wert zwischen zwei Stufen (von Hand in
// der Datei) rastet auf die naechste in der gewuenschten Richtung ein.
pub(crate) fn step_in(steps: &[u32], value: u32, up: bool) -> u32 {
    if up {
        steps.iter().copied().find(|&s| s > value).unwrap_or(value)
    } else {
        steps.iter().rev().copied().find(|&s| s < value).unwrap_or(value)
    }
}
// Linker Rand der Spalte `col` (0..3 Regler, 4 Schalter "Auf einmal").
pub(crate) fn particle_column_x(col: usize) -> f32 {
    PARTICLE_TOGGLE_X + PARTICLE_TOGGLE_W + 24.0 + col as f32 * PARTICLE_COLUMN_W
}

// Gleitende Liste unter einer festen Kopfzeile (Partikel, Lichtstaerken): die Eintraege von
// `n`, die beim Scrollstand `stand` ganz in den Zeilen 1..=sichtbar stehen, mit ihrer Hoehe.
// Was ueber den Rand gleitet, verschwindet ganz -- es gibt kein Abschneiden.
pub(crate) fn header_list_rows(n: usize, stand: f32, sichtbar: usize) -> Vec<(usize, f32)> {
    let von = stand.max(0.0).floor() as usize;
    (von..(von + sichtbar + 1).min(n))
        .filter_map(|i| {
            let platz = 1.0 + i as f32 - stand;
            (platz >= 0.99 && platz <= sichtbar as f32 + 0.01).then(|| (i, settings_row_y(0) + platz * SETTINGS_ROW_H))
        })
        .collect()
}

// Wie header_list_rows, aber gleitend: auch die am Rand nur angeschnittenen Zeilen, die das
// Zeilenband der Liste beschneidet (s. App::particle_list_band).
pub(crate) fn header_list_rows_smooth(n: usize, stand: f32, sichtbar: usize) -> Vec<(usize, f32)> {
    let von = stand.max(0.0).floor() as usize;
    (von..(von + sichtbar + 2).min(n))
        .filter_map(|i| {
            let platz = 1.0 + i as f32 - stand;
            (platz > 0.0 && platz < sichtbar as f32 + 1.0).then(|| (i, settings_row_y(0) + platz * SETTINGS_ROW_H))
        })
        .collect()
}

// Erste Zeile fuer "a–b von n": der gleitende Stand, in die Liste geklemmt.
pub(crate) fn header_list_first(n: usize, stand: f32, sichtbar: usize) -> usize {
    (stand.max(0.0).round() as usize).min(n.saturating_sub(sichtbar))
}

// Wertfeld des Reglers in Spalte `col`: dort steht der Wert, der Regler beginnt SLIDER_GROW davor.
pub(crate) fn particle_value_rect(col: usize, row: usize) -> Rect {
    Rect { x: particle_column_x(col) + SLIDER_GROW, y: settings_row_y(row), w: PARTICLE_VALUE_W, h: SETTINGS_CTRL_H }
}
pub(crate) const TEXT_SHADOW_TOOLTIP: &str = "Rand um allen Text. Kontur: eine dünne schwarze Linie rundherum. Vanilla: Minecrafts Schatten -- dieselbe Zeile einen Schriftpixel nach rechts unten versetzt, in einem Viertel der Helligkeit. Unten: ein weicher Schatten nach unten. Außen: derselbe weiche Schatten rundherum. Aus: kein Rand.";
pub(crate) const AREA_SHADOW_TOOLTIP: &str = "Eigene Schrift (linkes Kästchen) und eigener Rand (rechtes Kästchen) für diesen Bereich. Ein leeres Kästchen folgt der Zeile Standard oben. Schild-Countdown und Pfeil-Zahl haben keinen Rand -- ihre Farbe stellt man im Modul ein.";
pub(crate) const FONT_DRAG_HINT: &str = "Zum Ändern eine Schrift oder einen Schatten von hier in ein Kästchen links ziehen. Ein Kästchen lässt sich auch in ein anderes ziehen, nach draußen gezogen oder mit Rechtsklick wird es leer.";
// Zeile "Stabile Zahlen" im Schrift-Tab: eigener Abschnitt ganz unten.
pub(crate) const STABLE_NUMBERS_ROW: usize = FONT_EXCEPTIONS_ROW + TEXT_AREA_COUNT;
// Zeile "Grossbuchstaben" im Abschnitt Stil darunter.
pub(crate) const CAPITALS_ROW: usize = STABLE_NUMBERS_ROW + 1;
// Knoepfe "1 px" / "1/2 px" rechts neben dem Regler eines Vanilla-Schattens (s. shadow_half_rect).
pub(crate) const SHADOW_HALF_W: f32 = 150.0;
pub(crate) const VANILLA_SHADOW_TOOLTIP: &str = "Wie weit der Vanilla-Schatten neben dem Text liegt: wie in Minecraft einen Schriftpixel nach rechts und nach unten, oder nur einen halben -- dann ist er schmaler und der Text bleibt besser lesbar.";
pub(crate) const CAPITALS_TOOLTIP: &str = "Aus: aller Text wird klein geschrieben gezeichnet, auch Chat, Namen und Scoreboard. Was der Server schickt und was du tippst, bleibt unverändert.";
pub(crate) const STABLE_NUMBERS_TOOLTIP: &str = "Wechselnde Zahlen (Ping, FPS, Countdown, Scoreboard) halten ihren Platz, solange sie gleich viele Stellen haben: eine schmale 1 steht mittig, wo eben eine 8 war, und der Rest der Zeile bleibt stehen. Bekommt die Zahl eine Stelle mehr oder weniger, rückt die Zeile ganz normal.";
// Schrift-Tab: Zeile 0 "Standard" (Schrift und Schatten nebeneinander), dann die Ausnahmen,
// je Bereich eine Zeile.
pub(crate) const FONT_EXCEPTIONS_ROW: usize = 1;
// Kaestchen, in die man Schrift und Schatten zieht (s. FontSlot): darin der Name der Schrift
// bzw. des Schattens. Hoeher als ein Knopf, sie ragen oben und unten in den Zeilenabstand.
// Der Schrift-Tab setzt sie weiter links an als die anderen Seiten ihre Knoepfe: rechts
// daneben steht noch der Regler fuer die Schattenstaerke, und der Rand rechts ist hier
// doppelt so breit.
pub(crate) const FONT_CTRL_X: f32 = 260.0;
pub(crate) const FONT_BOX_W: f32 = 160.0;
pub(crate) const SHADOW_BOX_W: f32 = 104.0;
pub(crate) const FONT_SLOT_H: f32 = 46.0;
pub(crate) const FONT_SHADOW_X: f32 = FONT_CTRL_X + FONT_BOX_W + 10.0;
// Regler der Schattenstaerke: beginnt rechts neben den Schatten-Kaestchen.
pub(crate) const SHADOW_STRENGTH_X: f32 = FONT_SHADOW_X + SHADOW_BOX_W + 6.0;
// Karos hinter dem A der Schatten-Kaestchen, grau und gelb, damit man den Rand auf hellem
// wie auf mittlerem Grund sieht.
pub(crate) const SHADOW_CHECKER_PX: f32 = 8.0;
pub(crate) const SHADOW_CHECKER_GREY: [f32; 4] = [0.42, 0.42, 0.42, 1.0];
pub(crate) const SHADOW_CHECKER_YELLOW: [f32; 4] = [0.93, 0.78, 0.22, 1.0];
pub(crate) const SHADOW_PREVIEW_TEXT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
// So oft wird auf dem Startbildschirm nachgesehen, ob der Desktop-Hintergrund gewechselt hat
// (s. App::poll_wallpaper). Kostet einen gsettings-Aufruf im Hintergrund, dekodiert wird nur
// bei einer Aenderung.
pub(crate) const WALLPAPER_DESKTOP_RECHECK: Duration = Duration::from_secs(5);
// Abdunklung ueber dem Hintergrundbild, damit Titel und Knoepfe lesbar bleiben.
pub(crate) const WALLPAPER_DIM: [f32; 4] = [0.0, 0.0, 0.0, 0.35];
pub(crate) const FONT_TOOLTIP: &str = "Schrift für allen Text im Client: Menüs, HUD, Chat, Namensschilder. Einzelne Bereiche können unter Ausnahmen eine eigene Schrift bekommen. \"Minecraft\" ist die Pixelschrift des Spiels (aus dem installierten Resourcepack), DM Sans, Outfit, Lexend, Lexend Exa und Noto Sans sind glatte Vektorschriften. Kapitälchen (ᴀʙᴄ...) kommen notfalls aus Noto Sans, andere Zeichen, die eine Schrift nicht kennt (z.B. Server-Symbole), aus der Minecraft-Schrift. Das Layout bleibt gleich -- alle Schriften haben dieselbe Versal- und Zeilenhöhe.";
// Kameraperspektive -- Vanillas F5-Umschaltung (client/option/Perspective.java: FIRST_PERSON,
// THIRD_PERSON_BACK, THIRD_PERSON_FRONT, zyklisch per next()). Jede hat ihr EIGENES Sichtfeld
// in den Einstellungen: in der 3. Person will man meist weiter sehen als im Ego-Modus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Perspective {
    FirstPerson,
    ThirdPersonBack,
    ThirdPersonFront,
}

impl Perspective {
    pub(crate) const ALL: [Self; 3] = [Self::FirstPerson, Self::ThirdPersonBack, Self::ThirdPersonFront];
    // Die beiden Perspektiven mit Kameraabstand -- die Ego-Ansicht hat keinen (die Kamera
    // sitzt dort im Auge).
    pub(crate) const THIRD_PERSON: [Self; 2] = [Self::ThirdPersonBack, Self::ThirdPersonFront];

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::FirstPerson => Self::ThirdPersonBack,
            Self::ThirdPersonBack => Self::ThirdPersonFront,
            Self::ThirdPersonFront => Self::FirstPerson,
        }
    }

    pub(crate) fn is_first_person(self) -> bool {
        self == Self::FirstPerson
    }

    // Frontansicht: die Kamera schaut den Spieler von vorn an (Vanillas isFrontView()).
    pub(crate) fn is_front_view(self) -> bool {
        self == Self::ThirdPersonFront
    }

    // Zeile im Video-Tab, in der das Sichtfeld dieser Perspektive steht. Die 3.-Person-
    // Ansichten haben direkt darunter ihre Abstandszeile, deshalb die Luecken.
    pub(crate) const fn fov_row(self) -> usize {
        match self {
            Self::FirstPerson => VIDEO_CAMERA_ROW,
            Self::ThirdPersonBack => VIDEO_CAMERA_ROW + 1,
            Self::ThirdPersonFront => VIDEO_CAMERA_ROW + 2,
        }
    }

    // Zeile des Kameraabstands unter der Projektion -- None fuer die Ego-Ansicht, die keinen
    // hat.
    pub(crate) const fn distance_row(self) -> Option<usize> {
        match self {
            Self::FirstPerson => None,
            Self::ThirdPersonBack => Some(PROJECTION_ROW + 1),
            Self::ThirdPersonFront => Some(PROJECTION_ROW + 3),
        }
    }

    // Zeile der Kamerahoehe, direkt unter dem jeweiligen Abstand.
    pub(crate) const fn y_offset_row(self) -> Option<usize> {
        match self {
            Self::FirstPerson => None,
            Self::ThirdPersonBack => Some(PROJECTION_ROW + 2),
            Self::ThirdPersonFront => Some(PROJECTION_ROW + 4),
        }
    }

    pub(crate) const fn y_offset_label(self) -> &'static str {
        match self {
            Self::FirstPerson => "",
            Self::ThirdPersonBack => "Kamerahöhe (hinten)",
            Self::ThirdPersonFront => "Kamerahöhe (vorn)",
        }
    }

    pub(crate) const fn distance_label(self) -> &'static str {
        match self {
            Self::FirstPerson => "",
            Self::ThirdPersonBack => "Kameraabstand (hinten)",
            Self::ThirdPersonFront => "Kameraabstand (vorn)",
        }
    }

    pub(crate) const fn fov_label(self) -> &'static str {
        match self {
            Self::FirstPerson => "Sichtfeld (Ego)",
            Self::ThirdPersonBack => "Sichtfeld (3. Person)",
            Self::ThirdPersonFront => "Sichtfeld (3. Person, vorn)",
        }
    }

    pub(crate) const fn hud_label(self) -> &'static str {
        match self {
            Self::FirstPerson => "Ego",
            Self::ThirdPersonBack => "3. Person",
            Self::ThirdPersonFront => "3. Person (vorn)",
        }
    }
}

// Vanillas Camera.BASE_CAMERA_DISTANCE bzw. das Attribut EntityAttributes.CAMERA_DISTANCE --
// beide 4.0. Wie weit die Kamera in der 3. Person hinter dem Auge sitzt, bevor
// physics::clip_camera_distance sie an einer Wand naeher heranholt.
pub(crate) const THIRD_PERSON_DISTANCE: f32 = 4.0;
// Verstellbereich desselben Attributs (ClampedEntityAttribute mit min 0.0, max 32.0) --
// bewusst Vanillas Grenzen uebernommen statt eigener geraten. 0 legt die Kamera ins Auge
// (praktisch Ego-Ansicht), 32 ist weit genug, um sich als Punkt in der Landschaft zu sehen.
pub(crate) const CAMERA_DISTANCE_MIN: f32 = 0.0;
pub(crate) const CAMERA_DISTANCE_MAX: f32 = 32.0;
pub(crate) const CAMERA_DISTANCE_STEP: f32 = 0.5;
// Hoehenversatz der 3.-Person-Kamera, in Bloecken ueber (positiv) bzw. unter (negativ) der
// Augenhoehe. Kein Vanilla-Gegenstueck -- Vanillas Kamera sitzt starr auf Augenhoehe. Der
// Bereich ist bewusst eng: darueber hinaus verliert man den Bezug zur eigenen Figur, und
// der Versatz wirkt in WELT-Koordinaten (senkrecht), nicht entlang der Blickrichtung --
// beim Hoch-/Runterschauen bleibt die Kamera dadurch auf derselben Hoehe statt zu wandern.
pub(crate) const CAMERA_Y_OFFSET_MIN: f32 = -5.0;
pub(crate) const CAMERA_Y_OFFSET_MAX: f32 = 5.0;
pub(crate) const CAMERA_Y_OFFSET_STEP: f32 = 0.25;

// Video-Tab, LINKE Spalte: Zeilen 0..2 = Sichtfeld je Perspektive, Zeile 3 = Projektion,
// danach Licht, Partikel, Glint und Sichtweiten. Die 3.-Person-Kameraeinstellungen (Abstand/Hoehe) sitzen
// in der RECHTEN Spalte, s. Perspective::distance_row/y_offset_row.
pub(crate) const PROJECTION_ROW: usize = VIDEO_CAMERA_ROW + 3;
// CMAA2 (s. Settings::cmaa2_enabled, cmaa2.wgsl) -- DEAKTIVIERT: der Gather-Ansatz glaettete
// nicht nur Silhouettenkanten, sondern auch Kanten INNERHALB von Texturen (jede Kontraststelle
// im Texturmaterial selbst), und kostete spuerbar Leistung. Code (Shader, Pipelines,
// GpuState-Felder, Settings-Feld, Persistenz) bleibt vollstaendig erhalten, nur die
// UI-Einstiegspunkte (Zeile/Tooltip/Umschalter) sind unten auskommentiert -- fuer eine
// spaetere Ueberarbeitung (z.B. Kantenerkennung auf die HUD-/Silhouetten-Ebene beschraenken).
// const CMAA2_ROW: usize = 13;
// Zeile 4: der (inzwischen echte) Schalter fuer weiche Beleuchtung, Platzhalter ab Zeile 5.
// Er war vorher selbst ein Platzhalter weiter unten -- dass er nach oben rutscht, laesst die
// Gesamthoehe des Tabs unveraendert.
pub(crate) const SMOOTH_LIGHTING_ROW: usize = VIDEO_CAMERA_ROW + 8;
// Staerke der weichen Beleuchtung in Prozent der Vanilla-Abdunklung (s. Settings).
pub(crate) const SMOOTH_LIGHTING_MAX: u32 = 100;
pub(crate) const SMOOTH_LIGHTING_STEP: u32 = 10;
// Zeile 5: die Partikel-Stufe -- ebenfalls vorher ein Platzhalter weiter unten.
pub(crate) const BLOCK_ROTATION_ROW: usize = SMOOTH_LIGHTING_ROW + 1;
// Zwei Zeilen: die eigenen Partikel (an der eigenen Figur), darunter die anderen.
pub(crate) const PARTICLES_ROW: usize = BLOCK_ROTATION_ROW + 1;
pub(crate) const PARTICLES_OTHER_ROW: usize = PARTICLES_ROW + 1;
// Glint-Regler (frueher im Tab Barrierefreiheit) stehen in der linken Spalte unter den
// Partikeln, die Platzhalter danach.
pub(crate) const GLINT_SPEED_ROW: usize = PARTICLES_OTHER_ROW + 1;
pub(crate) const GLINT_STRENGTH_ROW: usize = GLINT_SPEED_ROW + 1;
// Ruestungsbesatz (Trims) an getragener Ruestung zeigen (Settings::armor_trims).
pub(crate) const ARMOR_TRIMS_ROW: usize = GLINT_STRENGTH_ROW + 1;
pub(crate) const ARMOR_TRIMS_TOOLTIP: &str = "Zeigt den Rüstungsbesatz (Trims) auf getragener Rüstung -- bei Spielern, Mobs und Rüstungsständern. Aus: nur die blanke Rüstung, wie ohne Schmiedevorlage. Die Items im Inventar behalten ihr Aussehen.";
// Entity-Renderdistanz (Settings::entity_render_distance), in Bloecken.
pub(crate) const ENTITY_DISTANCE_MIN: u32 = 16;
// Das Maximum heisst "unbegrenzt".
pub(crate) const ENTITY_DISTANCE_MAX: u32 = 512;
pub(crate) const ENTITY_DISTANCE_STEP: u32 = 16;
pub(crate) const ENTITY_DISTANCE_DEFAULT: u32 = 128;
// Block-Entity-Renderdistanz (Settings::block_entity_render_distance), in Bloecken: Truhen,
// Betten, Shulker, Koepfe, Statuen, Schilder. Standard wie Vanilla (BlockEntityRenderer
// getViewDistance 64), das Maximum heisst "unbegrenzt".
pub(crate) const BLOCK_ENTITY_DISTANCE_MIN: u32 = 16;
pub(crate) const BLOCK_ENTITY_DISTANCE_MAX: u32 = 512;
pub(crate) const BLOCK_ENTITY_DISTANCE_STEP: u32 = 16;
pub(crate) const BLOCK_ENTITY_DISTANCE_DEFAULT: u32 = 64;
// Video-Tab, Abschnitt Sichtweite: Render-Distance (Settings::max_render_distance), Entity-
// Weite, dann die Fernsicht aus dem Chunk-Saver (s. lod.rs) -- Weite und Detail. Das Modul
// selbst speichert nur. Danach die Abschnitte Kamera (Sichtfeld je Perspektive, Projektion,
// Abstand und Hoehe der 3. Person, s. Perspective), Darstellung und Treffer.
pub(crate) const RENDER_DISTANCE_ROW: usize = 0;
pub(crate) const ENTITY_DISTANCE_ROW: usize = RENDER_DISTANCE_ROW + 1;
pub(crate) const BLOCK_ENTITY_DISTANCE_ROW: usize = ENTITY_DISTANCE_ROW + 1;
pub(crate) const LOD_DETAIL_ROW: usize = BLOCK_ENTITY_DISTANCE_ROW + 1;
pub(crate) const VIDEO_CAMERA_ROW: usize = LOD_DETAIL_ROW + 1;
pub(crate) const VIDEO_ROW_COUNT: usize = VIEW_MODEL_ROW + 1;
pub(crate) const RENDER_DISTANCE_TOOLTIP: &str = "Radius in Chunks um den Spieler, in dem geladene Chunks gezeichnet werden. Weiter draußen bleiben die Blockdaten geladen (kein Loch beim Zurückkommen), sie werden nur nicht gezeichnet. Unabhängig von der Sichtweite des Servers -- wirkt sofort, ohne neu zu verbinden. Zahl anklicken für einen eigenen Wert, -1 = unbegrenzt.";
pub(crate) const LOD_DETAIL_TOOLTIP: &str = "Wie fein die Fernsicht unterteilt wird: so groß (in Pixeln²) darf ein Fernsicht-Block auf dem Bildschirm höchstens werden, bevor eine feinere Stufe geladen wird. Kleiner = schärfer, kostet mehr Speicher und GPU-Zeit. Standard 4.";
pub(crate) const CRIT_COLOR_TOOLTIP: &str = "Eigene Farbe fuers Aufblitzen bei kritischen Treffern (kein Vanilla). AUS: Crits blitzen \
wie jeder andere Treffer in der Schadens-Faerbung auf. Dauer und \"Nur Skin\" der Schadens-Färbung gelten auch hier.";
pub(crate) const PARTICLES_TOOLTIP: &str = "Vanillas \"Partikel\": Alle, Verringert (jeder dritte fällt weg) oder Minimal (nur noch, was immer erscheint -- Explosionen, Schwung, Schadensherzen). Blockstaub beim Abbauen und Zerbrechen ist wie in Vanilla von der Stufe unabhängig; die Explosions-Trümmer gibt es nur bei \"Alle\". \"Anpassen\" (kein Vanilla): je Partikelart an/aus, wie lange sie bleibt, wie deckend sie ist und wie schnell sie sich bewegt -- z.B. Explosion aus gegen die weißen Punkte beim Crystallen. Eigene Partikel sind die, die an deinem Spieler entstehen (Treffer auf dich, deine Effekte, dein Totem), andere alle übrigen.";
pub(crate) const SMOOTH_LIGHTING_TOOLTIP: &str = "Vanillas \"Weiche Beleuchtung\": Kanten, Ecken und Nischen werden dort dunkler, wo Blöcke aneinanderstoßen -- das gibt der Welt Tiefe. Nur die Eckabdunklung, keine Lichtwerte (dieser Client rendert bewusst immer fullbright). Die Prozentzahl ist die Stärke der Schatten: 100 % wie Vanilla, 0 % aus. Änderung baut alle geladenen Chunks neu. Im Shader-Pfad ohne Wirkung: dort kommen Schatten nur von echten Lichtquellen.";
#[allow(dead_code)]
pub(crate) const CMAA2_TOOLTIP: &str =
    "CMAA2 (Conservative Morphological Anti-Aliasing 2, inspiriert von Intels Referenz-Implementierung). Erkennt Kanten über einen lokal kontrastadaptiven Schwellwert (dieselbe Formel wie im Original) und glättet sie durch Mischen mit den Nachbarpixeln -- flache Flächen bleiben unangetastet. Noch keine Z-Form-/Simple-Shape-Erkennung wie im Original (kommt in einer späteren Stufe), daher etwas weicher als das Original an manchen Diagonalen.";
pub(crate) const FOV_TOOLTIP: &str =
    "Vertikales Sichtfeld in Grad (Vanilla-Standard 70). Kleiner = herangezoomt, größer = weitwinkliger. Zahl anklicken für einen eigenen Wert -- die Pfeile gehen in 5er-Schritten.";
pub(crate) const CAMERA_Y_OFFSET_TOOLTIP: &str =
    "Verschiebt die Kamera senkrecht gegenüber der Augenhöhe -- positiv nach oben (Blick über die Schulter von schräg oben), negativ nach unten. Wirkt in Weltkoordinaten, bleibt beim Hoch-/Runterschauen also auf derselben Höhe. 0 = wie Vanilla. Zahl anklicken für einen eigenen Wert, Minus erlaubt.";
pub(crate) const CAMERA_DISTANCE_TOOLTIP: &str =
    "Wie weit die Kamera hinter (bzw. bei der Frontansicht vor) dem Spieler sitzt, in Blöcken. Vanillas Wert ist 4; erlaubt sind 0 bis 32. Der Wert ist eine OBERGRENZE -- an einer Wand rückt die Kamera automatisch näher heran, damit man nicht in die Wand schaut. Zahl anklicken für einen eigenen Wert.";
pub(crate) const FOV_THIRD_BACK_TOOLTIP: &str =
    "Sichtfeld der Verfolgeransicht (F5, erste Stufe) -- eigener Wert, damit ein weiter Blick hier den Ego-Modus nicht mitzieht. Die Kamera sitzt 4 Blöcke hinter dem Auge und rückt an Wänden automatisch näher heran.";
pub(crate) const FOV_THIRD_FRONT_TOOLTIP: &str =
    "Sichtfeld der Frontansicht (F5, zweite Stufe -- Kamera vor dem Spieler, blickt ihn an). Die an den Server gesendete Blickrichtung bleibt dabei die des Spielers, nur die Kamera dreht sich.";
// Schadens-Faerbung, Abschnitt Treffer des Video-Tabs: eine Zeile mit dem Farbfeld; die
// Regler stecken im Farbfenster (s. ColorPopup).
pub(crate) const DAMAGE_TINT_FIRST_ROW: usize = ARMOR_TRIMS_ROW + 1;
// Direkt unter dem Farbfeld: nur der Skin oder alles, und wie lange der Blitz haelt.
pub(crate) const DAMAGE_TINT_SKIN_ONLY_ROW: usize = DAMAGE_TINT_FIRST_ROW + 1;
pub(crate) const DAMAGE_TINT_TICKS_ROW: usize = DAMAGE_TINT_FIRST_ROW + 2;
// Crit-Farbe (frueher ein eigenes Modul) gehoert zur Schadens-Faerbung und steht direkt
// darunter: Schalter, rechts daneben das Farbfeld.
pub(crate) const CRIT_COLOR_ROW: usize = DAMAGE_TINT_TICKS_ROW + 1;
// Abschnitt Spielerliste: ihre Groesse in Prozent der GUI-Skala (Settings::player_list_scale).
pub(crate) const PLAYER_LIST_SCALE_ROW: usize = CRIT_COLOR_ROW + 1;
// Abschnitt Hurt-Cam: an/aus und Staerke (Settings::hurt_cam/_strength).
pub(crate) const HURT_CAM_ROW: usize = PLAYER_LIST_SCALE_ROW + 1;
pub(crate) const HURT_CAM_STRENGTH_ROW: usize = HURT_CAM_ROW + 1;
// Abschnitt Schulterkamera (Settings::shoulder_*): an/aus, seitlicher Versatz, dann die drei
// Zusaetze.
pub(crate) const SHOULDER_ROW: usize = HURT_CAM_STRENGTH_ROW + 1;
pub(crate) const SHOULDER_OFFSET_ROW: usize = SHOULDER_ROW + 1;
pub(crate) const SHOULDER_CROSSHAIR_ROW: usize = SHOULDER_ROW + 2;
pub(crate) const SHOULDER_FADE_ROW: usize = SHOULDER_ROW + 3;
pub(crate) const SHOULDER_DECOUPLED_ROW: usize = SHOULDER_ROW + 4;
// Seitlicher Versatz in Hundertstel Bloecken, in 5er-Schritten.
pub(crate) const SHOULDER_OFFSET_MIN: i32 = -200;
pub(crate) const SHOULDER_OFFSET_MAX: i32 = 200;
pub(crate) const SHOULDER_OFFSET_STEP: i32 = 5;
pub(crate) const SHOULDER_OFFSET_DEFAULT: i32 = 75;
// Ab diesem Abstand zwischen Kamera und Auge (Bloecke) ist der eigene Spieler ganz zu sehen,
// darunter blendet er aus (Settings::shoulder_fade); unter SHOULDER_FADE_NEAR ist er weg.
pub(crate) const SHOULDER_FADE_FAR: f32 = 1.8;
pub(crate) const SHOULDER_FADE_NEAR: f32 = 0.6;
// Kamera frei vom Koerper: so lange nach einem Angriff oder Benutzen schaut der Spieler noch
// dorthin, wo die Kamera hinzielt (s. App::aim_until).
pub(crate) const SHOULDER_AIM_HOLD: Duration = Duration::from_millis(1500);
pub(crate) const SHOULDER_TOOLTIP: &str = "Wie die Mod Shoulder Surfing Reloaded: in der 3. Person von hinten sitzt die Kamera seitlich über der Schulter. Abstand und Höhe kommen aus dem Abschnitt Kamera. Die Taste \"Schulter wechseln\" (Steuerung, Ansicht) setzt sie auf die andere Seite.";
pub(crate) const SHOULDER_CROSSHAIR_TOOLTIP: &str = "Das Fadenkreuz steht dort, wo dein Spieler wirklich hinzielt -- über der Schulter liegt das nicht in der Bildmitte. Mit \"Kamera frei vom Körper\" bleibt es in der Mitte: dort zielt dann die Kamera, und beim Angreifen oder Benutzen dreht sich der Spieler dorthin.";
pub(crate) const SHOULDER_FADE_TOOLTIP: &str = "Kommt die Kamera deinem Spieler nahe (etwa an einer Wand), wird er durchsichtig, damit er nicht das ganze Bild verdeckt.";
pub(crate) const SHOULDER_DECOUPLED_TOOLTIP: &str = "Die Maus dreht nur die Kamera. Dein Spieler läuft in die Richtung, in die du ihn steuerst, und dreht sich beim Angreifen, Abbauen oder Benutzen zu dem, was in der Bildmitte liegt.";
// Abschnitt Feuer: Modus, darunter je nach Modus die Hoehe oder die Fadenkreuzfarbe.
pub(crate) const FIRE_MODE_ROW: usize = SHOULDER_DECOUPLED_ROW + 1;
pub(crate) const FIRE_DETAIL_ROW: usize = FIRE_MODE_ROW + 1;
// Abschnitt Tod: die Todesanimation (Settings::death_animation).
pub(crate) const DEATH_ANIM_ROW: usize = FIRE_DETAIL_ROW + 1;
// Abschnitt Animationen: eine Zeile mit Hauptschalter (Settings::anims_enabled) und dem Knopf
// zur Unterseite MenuScreen::SettingsAnimations, dort je Animation aus
// world::more_anims::FEATURES ein Schalter (Zeile i = Animation i).
pub(crate) const ANIM_ROW: usize = DEATH_ANIM_ROW + 1;
// Letzter Abschnitt, Modified View Model (Settings::modified_view_model): eigenes Ego-Modell,
// der Arm haelt das Item, je Item eingestellt. Aus gilt es trotzdem beim Item-Ansehen.
pub(crate) const VIEW_MODEL_ROW: usize = ANIM_ROW + 1;
pub(crate) const VIEW_MODEL_TOOLTIP: &str = "In der Ich-Sicht hält deine Hand das Item, mit den im Hand-Debug je Item eingestellten Werten. Aus: wie in Vanilla, das Item ohne Arm -- nur beim Ansehen eines Items (Taste \"Item ansehen\") hält die Hand es trotzdem.";
pub(crate) const ANIMS_TOOLTIP: &str = "Haltungen für Spieler in der dritten Person, die Vanilla nur in der Ich-Sicht oder gar nicht zeigt (wie die Mod Not Enough Animations) -- bei anderen Spielern und bei dir in F5. Schild, Speer und Schläge bleiben wie in Vanilla.";
// Vanilla: umkippen (LivingEntityRenderer.setupRotations, deathTime) und rot. Aus: Spieler
// verschwinden sofort. Explosion: Spieler fliegen in ihre Teile auseinander (death_burst.rs).
// Aus und Explosion gelten nur fuer Spieler, Mobs sterben immer wie in Vanilla.
pub(crate) const DEATH_ANIM_VANILLA: u8 = 0;
pub(crate) const DEATH_ANIM_OFF: u8 = 1;
pub(crate) const DEATH_ANIM_EXPLOSION: u8 = 2;
pub(crate) const DEATH_ANIM_NAMES: [&str; 3] = ["Vanilla", "Aus", "Explosion"];
pub(crate) const DEATH_ANIM_TOOLTIP: &str = "Wie sterbende Spieler aussehen: Vanilla kippt sie rot um, Aus lässt sie sofort verschwinden, Explosion lässt Kopf, Rumpf, Arme und Beine auseinanderfliegen. Mobs sterben immer wie in Vanilla.";
// Hurt-Cam: Ausschlag bei 100 % in Grad (Vanillas bobHurt).
pub(crate) const HURT_CAM_MAX_DEGREES: f32 = 14.0;
// Feuer vor den Augen (Settings::fire_mode).
pub(crate) const FIRE_MODE_VANILLA: u8 = 0;
pub(crate) const FIRE_MODE_CROSSHAIR: u8 = 1;
// Gar nichts: kein Bild vor den Augen, kein gefaerbtes Fadenkreuz.
pub(crate) const FIRE_MODE_OFF: u8 = 2;
pub(crate) const FIRE_MODE_NAMES: [&str; 3] = ["Vanilla", "Fadenkreuz", "Aus"];

// Inventar > Animation: Hervorhebung unter dem Zeiger (Uebergang, Farbe) und fliegende Items
// beim Verschieben (s. Settings::slot_hover_fade_ms, swap_anim_ms).
pub(crate) const INVENTORY_ANIM_ROW: usize = 9;
pub(crate) const SLOT_HOVER_FADE_ROW: usize = INVENTORY_ANIM_ROW;
pub(crate) const SLOT_HOVER_COLOR_ROW: usize = INVENTORY_ANIM_ROW + 1;
pub(crate) const SWAP_ANIM_ROW: usize = INVENTORY_ANIM_ROW + 2;
pub(crate) const INVENTORY_ROW_COUNT: usize = SWAP_ANIM_ROW + 1;
pub(crate) const SLOT_HOVER_FADE_MAX: u32 = 500;
pub(crate) const SLOT_HOVER_FADE_STEP: u32 = 10;
pub(crate) const SWAP_ANIM_MAX: u32 = 1000;
pub(crate) const SWAP_ANIM_STEP: u32 = 10;
pub(crate) const SLOT_HOVER_FADE_TOOLTIP: &str = "Wie lange die Hervorhebung des Slots unter dem Zeiger ein- und ausblendet, wenn du über die Slots fährst. Aus: sie springt sofort wie in Vanilla.";
pub(crate) const SLOT_HOVER_COLOR_TOOLTIP: &str = "Farbe und Deckkraft der Hervorhebung des Slots unter dem Zeiger. Vanilla: weiß.";
pub(crate) const SWAP_ANIM_TOOLTIP: &str = "Verschiebst du Items (Shift-Klick, Zifferntaste, Zweithand-Taste, Doppelklick), fliegen sie in dieser Zeit zu ihrem neuen Platz, statt dorthin zu springen. Aus: wie in Vanilla.";

// Sonstiges > Server-Bildschirme (MenuScreen::SettingsServerScreens): je Bildschirm, ob der
// Server ihn schliessen (Settings::server_close_blocked, Bit = Zeile) und oeffnen
// (Settings::server_open, ein Modus je Eintrag) darf. Container gehen nach Gruppen
// (s. inventory::menu_group); Name und Erklaerung der Zeile beim Schliessen und beim Oeffnen.
macro_rules! container_group {
    ($name:literal, $inhalt:literal) => {
        (
            $name,
            concat!($inhalt, ". An: der Server darf sie schließen, wie in Vanilla. Aus: sie bleiben offen -- Klicks darin nimmt der Server dann nicht mehr an."),
            concat!($inhalt, ". Erlaubt: wie in Vanilla. Sofort zu: nicht zeigen und dem Server gleich das Schließen melden, als hättest du direkt Esc gedrückt. Ignorieren: nicht zeigen und nichts melden -- für den Server bleibt es offen, bis du dein Inventar öffnest und schließt."),
        )
    };
}
pub(crate) const CONTAINER_GROUPS: [(&str, &str, &str); 5] = [
    container_group!("Truhen & Lager", "Truhen, Fässer, Spender, Werfer, Trichter, Shulkerkisten -- und die Menüs von Plugins, die sind fast immer Truhen"),
    container_group!("Öfen", "Ofen, Schmelzofen, Räucherofen"),
    container_group!("Werkbänke", "Werkbank, Crafter, Amboss, Schmiedetisch, Schleifstein, Steinsäge, Webstuhl, Kartentisch, Zaubertisch"),
    container_group!("Handel", "Dorfbewohner und fahrende Händler"),
    container_group!("Sonstige Container", "Leuchtfeuer, Braustand, Pult"),
];
pub(crate) const SERVER_CLOSE_INVENTORY_TOOLTIP: &str = "Dein eigenes Inventar. An: der Server darf es schließen, wie in Vanilla. Aus: es bleibt offen.";
pub(crate) const SERVER_CLOSE_SIGN_TOOLTIP: &str = "An: wie in Vanilla, der Editor geht zu und schickt seinen Text. Aus: er bleibt offen und schickt erst etwas, wenn du ihn schließt. Server prüfen damit den Client (Editor auf und sofort wieder zu) und sehen, dass die Antwort ausbleibt.";
pub(crate) const SERVER_OPEN_SIGN_TOOLTIP: &str = "Erlaubt: wie in Vanilla. Sofort zu: nicht zeigen, aber den Text gleich zurückschicken wie ein sofort geschlossener Editor -- so antwortet auch Vanilla auf die Prüfung von Servern. Ignorieren: nichts zeigen, nichts schicken.";
pub(crate) const SERVER_OPEN_BOOK_TOOLTIP: &str = "Bücher, die der Server dir aufschlägt (Regeln, Anleitungen). Aus: sie gehen nicht auf.";
pub(crate) const SERVER_OPEN_DIALOG_TOOLTIP: &str = "Dialog-Fenster des Servers (seit 1.21.6, z. B. Regeln vor dem Spielen). Aus: sie gehen nicht auf. Verlangt ein Server im Dialog eine Bestätigung, bevor du spielen darfst, kommst du dann nicht weiter.";
pub(crate) const SERVER_SCREENS_TOOLTIP: &str = "Für jeden Bildschirm einzeln, ob der Server ihn schließen und ob er ihn öffnen darf.";
// Abschnitt Schliessen: das eigene Inventar, die Gruppen, der Schild-Editor.
pub(crate) const SERVER_CLOSE_INVENTORY_ROW: usize = 0;
pub(crate) const SERVER_CLOSE_GROUP_ROW: usize = 1;
pub(crate) const SERVER_CLOSE_SIGN_ROW: usize = SERVER_CLOSE_GROUP_ROW + CONTAINER_GROUPS.len();
// Abschnitt Oeffnen ab dieser Zeile; Eintrag i in Settings::server_open steht in Zeile
// SERVER_OPEN_ROW + i. Erst die Gruppen, dann Schild-Editor, Buch, Dialog.
pub(crate) const SERVER_OPEN_ROW: usize = SERVER_CLOSE_SIGN_ROW + 1;
pub(crate) const SERVER_OPEN_SIGN: usize = CONTAINER_GROUPS.len();
pub(crate) const SERVER_OPEN_BOOK: usize = SERVER_OPEN_SIGN + 1;
pub(crate) const SERVER_OPEN_DIALOG: usize = SERVER_OPEN_BOOK + 1;
pub(crate) const SERVER_OPEN_COUNT: usize = SERVER_OPEN_DIALOG + 1;
pub(crate) const SERVER_SCREENS_ROWS: usize = SERVER_OPEN_ROW + SERVER_OPEN_COUNT;
// Was mit einem gesperrten Oeffnen passiert. Sofort zu: wie geoeffnet und gleich mit Esc
// geschlossen (Container Close bzw. Sign Update mit dem Text). Ignorieren: nichts zeigen,
// nichts schicken. Buch und Dialog schicken beim Schliessen nichts, dort nur An/Aus
// (Aus = Ignorieren).
pub(crate) const SERVER_OPEN_ALLOW: u8 = 0;
pub(crate) const SERVER_OPEN_CLOSE: u8 = 1;
pub(crate) const SERVER_OPEN_IGNORE: u8 = 2;
pub(crate) const SERVER_OPEN_NAMES: [(&str, u8); 3] = [("Erlaubt", SERVER_OPEN_ALLOW), ("Sofort zu", SERVER_OPEN_CLOSE), ("Ignorieren", SERVER_OPEN_IGNORE)];
// Sonstiges > No slot set / No rotate set (Settings::no_slot_set, no_rotate_set).
pub(crate) const SERVER_SET_FOLLOW: u8 = 0;
pub(crate) const SERVER_SET_IGNORE: u8 = 1;
pub(crate) const SERVER_SET_RESET: u8 = 2;
pub(crate) const SERVER_SET_NAMES: [(&str, u8); 3] = [("Aus", SERVER_SET_FOLLOW), ("Ignore", SERVER_SET_IGNORE), ("Reset", SERVER_SET_RESET)];
pub(crate) const PLAYER_LIST_SCALE_TOOLTIP: &str = "Größe der Spielerliste (Tab) in Prozent. 100 % = wie Vanilla, sie folgt dann nur der GUI-Skala wie die Hotbar. Zahl anklicken für einen eigenen Wert.";
// Vanillas hurtTime: 10 Ticks. Nach oben bis 2 s, darueber verschwimmen Treffer ineinander.
pub(crate) const DAMAGE_TINT_TICKS_DEFAULT: u32 = 10;
pub(crate) const DAMAGE_TINT_TICKS_MIN: u32 = 1;
pub(crate) const DAMAGE_TINT_TICKS_MAX: u32 = 40;
pub(crate) const DAMAGE_TINT_SKIN_ONLY_TOOLTIP: &str = "AN: nur der Skin bzw. Koerper leuchtet bei Schaden auf -- Ruestung, \
Items in der Hand, Schild und Umhang bleiben unveraendert, genau wie in Vanilla (Vorgabe). \
AUS: alles am Getroffenen wird eingefaerbt. Gilt auch fuer die Crit-Farbe.";
pub(crate) const DAMAGE_TINT_TICKS_TOOLTIP: &str = "Wie lange die Schadens-Faerbung nach einem Treffer anhaelt, in Sekunden \
(ein Schritt = ein Tick = 0,05 s). Vanilla: 0,50 s (10 Ticks). Gilt auch fuer die Crit-Farbe. \
Die Wackel-Animation beim Treffer bleibt davon unberuehrt.";
pub(crate) const DAMAGE_TINT_TOOLTIP: &str = "Farbe, in der ein Lebewesen aufblitzt, wenn es Schaden nimmt (Vanillas \"damage tint\"). \
Vorgabe ist genau Vanilla: reines Rot mit 30 % Deckkraft -- Minecraft mischt in entity.fsh 1 - 178/255 = 30.2 % Rot \
ueber die Textur. Die Faerbung haelt konstant an und blendet nicht aus, wie Vanilla; wie lange, steht unter \"Dauer\" (Vanilla: 0,5 s). \
Deckkraft 0 % schaltet sie ab.";

pub(crate) const PROJECTION_TOOLTIP: &str =
    "Normal ist die übliche geradlinige Perspektive. Panini/Stereografisch/Zylindrisch krümmen das Bild (Weitwinkel). Hinweis: Die Krümmung wirkt an den Eckpunkten -- große zusammengefasste Flächen krümmen sich nicht mit.";

// Lautstaerke je Kategorie (Settings::category_volume), in Vanillas Reihenfolge und mit
// Vanillas Namen in settings.conf (volume_<key>). Im Audio-Tab ab Zeile
// AUDIO_CATEGORY_FIRST_ROW, unter der Hauptlautstaerke. Die Kategorie eines Tons legt der
// Server im Ton-Paket fest; Klaenge, die der Client selbst spielt, bekommen die, die Vanilla
// dafuer nimmt (s. Settings::sound_gain).
pub(crate) struct VolumeCategory {
    pub(crate) category: protocol::packets::SoundCategory,
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
    pub(crate) tooltip: &'static str,
}

pub(crate) const VOLUME_CATEGORIES: [VolumeCategory; 10] = {
    use protocol::packets::SoundCategory as C;
    [
        VolumeCategory { category: C::Music, key: "music", label: "Musik", tooltip: "Musik, die der Server abspielt." },
        VolumeCategory { category: C::Records, key: "record", label: "Musikblöcke", tooltip: "Plattenspieler und Notenblöcke." },
        VolumeCategory { category: C::Weather, key: "weather", label: "Wetter", tooltip: "Regen und Donner." },
        VolumeCategory {
            category: C::Blocks,
            key: "block",
            label: "Blöcke",
            tooltip: "Abbauen, Setzen, Türen, Kisten, Explosionen und alles andere, was von Blöcken kommt.",
        },
        VolumeCategory { category: C::Hostile, key: "hostile", label: "Feindliche Kreaturen", tooltip: "Zombies, Skelette, Creeper und andere Monster." },
        VolumeCategory { category: C::Neutral, key: "neutral", label: "Freundliche Kreaturen", tooltip: "Tiere, Dorfbewohner und andere friedliche Kreaturen." },
        VolumeCategory {
            category: C::Players,
            key: "player",
            label: "Spieler",
            tooltip: "Schritte, Treffer, Essen, Totems und alles andere, was von Spielern kommt, auch von dir selbst.",
        },
        VolumeCategory { category: C::Ambient, key: "ambient", label: "Atmosphäre/Umgebung", tooltip: "Höhlengeräusche, Unterwasser-Rauschen und andere Umgebungsklänge." },
        VolumeCategory { category: C::Voice, key: "voice", label: "Sprachausgabe/Stimme", tooltip: "Sprachausgabe, wenn der Server welche schickt. Der Sprachchat hat seine eigene Lautstärke." },
        VolumeCategory {
            category: C::Ui,
            key: "ui",
            label: "UI-Sounds",
            tooltip: "Klänge der Oberfläche wie Klicks in Menüs. Hängt als einzige nicht an der Hauptlautstärke.",
        },
    ]
};
// Audio-Tab: Abschnitt Lautstaerke (Hauptlautstaerke, darunter je Kategorie), dann Ausgabe.
pub(crate) const AUDIO_MASTER_ROW: usize = 0;
pub(crate) const AUDIO_CATEGORY_FIRST_ROW: usize = AUDIO_MASTER_ROW + 1;
// Abschnitt UI-Klaenge: je Klang ein Regler (s. ui_sound::UiSound::ALL).
pub(crate) const AUDIO_UI_SOUND_FIRST_ROW: usize = AUDIO_CATEGORY_FIRST_ROW + VOLUME_CATEGORIES.len();
pub(crate) const AUDIO_DEVICE_ROW: usize = AUDIO_UI_SOUND_FIRST_ROW + ui_sound::UiSound::ALL.len();
pub(crate) const AUDIO_TEST_TONE_ROW: usize = AUDIO_DEVICE_ROW + 1;
pub(crate) const AUDIO_SPATIAL_ROW: usize = AUDIO_TEST_TONE_ROW + 1;
pub(crate) const AUDIO_PITCH_ROW: usize = AUDIO_SPATIAL_ROW + 1;
// Puffer des Ausgabe-Streams: Auswahl plus Anzeige, was laeuft (s. audio::BufferInfo).
pub(crate) const AUDIO_BUFFER_ROW: usize = AUDIO_PITCH_ROW + 1;
pub(crate) const AUDIO_BUFFER_INFO_ROW: usize = AUDIO_BUFFER_ROW + 1;
// PipeWire (s. pipewire-Modul): womit der Graph laeuft, darunter dessen Untergrenze senken
// oder wieder entfernen.
pub(crate) const AUDIO_PIPEWIRE_ROW: usize = AUDIO_BUFFER_INFO_ROW + 1;
pub(crate) const AUDIO_PIPEWIRE_PRIO_ROW: usize = AUDIO_PIPEWIRE_ROW + 1;
pub(crate) const AUDIO_PIPEWIRE_SET_ROW: usize = AUDIO_PIPEWIRE_PRIO_ROW + 1;
pub(crate) const AUDIO_AUTO_ROW: usize = AUDIO_PIPEWIRE_SET_ROW + 1;
// Waehlbare Puffergroessen in Frames, 0 = Vorgabe des Systems (bei ALSA rund 100 ms). Was das
// Geraet nicht kann, klemmt audio::low_latency_config auf seinen Bereich; was wirklich laeuft,
// zeigt die Zeile darunter.
pub(crate) const AUDIO_BUFFER_CHOICES: [u32; 15] = [16, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536, 2048, 0];

pub(crate) fn audio_buffer_request(frames: u32) -> Option<u32> {
    (frames != 0).then_some(frames)
}

pub(crate) fn audio_buffer_label(frames: u32) -> String {
    if frames == 0 {
        "System-Vorgabe".to_string()
    } else {
        format!("{frames} Frames")
    }
}

// Chat-Einstellungen im Fenster des Chats im HUD-Editor (Rechtsklick auf den Chat), je Zeile
// die Erklaerung beim Draufzeigen: Aussehen, dann Verlauf.
pub(crate) const CHAT_TOOLTIPS: [&str; 9] = [
    "Textgröße des Chats in Prozent. 100 % ist die bisherige Größe. Mit DM Sans in Viertelschritten gestochen scharf, dazwischen wird gerundet; die Minecraft-Schrift ist Pixelkunst und nur bei 50/100/150/... % pixelgenau.",
    "Breite des Chats, solange er geschlossen ist (in Schriftpixeln, wächst mit der Textgröße). Geöffnet (T) nimmt er die ganze Bildschirmbreite.",
    "Wie stark der Hintergrund hinter dem Chattext abgedunkelt wird. 0 % = kein Hintergrund. Gilt auch für die Eingabezeile und die Verlaufsliste.",
    "Wie stark bei offenem Chat (T) alles bis einschließlich der Eingabezeile abgedunkelt wird. 0 % = gar nicht.",
    "Wie viele Zeilen der Chat im Spiel zeigt.",
    "Wie viele Zeilen der Chat zeigt, während man tippt (T). Er rückt dann direkt über die Eingabezeile; mit dem Mausrad lässt sich durch den ganzen Verlauf blättern.",
    "Wie lange eine Nachricht im Spiel sichtbar bleibt, danach blendet sie schnell aus. Beim Tippen (T) ist der ganze Verlauf wieder da. Nie = Nachrichten bleiben stehen. Vanilla: 10 s.",
    "Zusätzlicher Platz zwischen den Chatzeilen, in Prozent der Zeilenhöhe; der Text steht mittig darin. 0 % = dicht wie bisher. Vanilla: Chat Line Spacing, 0 %.",
    "Java: der Chat hängt unten, neue Nachrichten schieben die älteren nach oben. Bedrock: der Chat hängt oben, neue Nachrichten kommen unten dazu, und blendet eine aus, rücken alle neueren nach oben. Gilt für den Chat im Spiel; der offene Chat (T) bleibt wie Java.",
];
// Zeile der Wahl Java/Bedrock im Einstellungsfenster des Chats.
pub(crate) const CHAT_STYLE_ROW: usize = 8;
// Zeilenabstand im Chat (Settings::chat_line_spacing), in Prozent der Texthoehe.
pub(crate) const CHAT_LINE_SPACING_MAX: u32 = 100;
pub(crate) const CHAT_LINE_SPACING_STEP: u32 = 5;

// Glint-Regler im Video-Tab (s. GLINT_SPEED_ROW).
pub(crate) const GLINT_SPEED_TOOLTIP: &str =
    "Wie schnell der Verzauberungs-Schimmer über Items und Rüstung wandert (Vanilla: Glint Speed, Standard 50 %). 0 % = steht still. Zahl anklicken für einen eigenen Wert.";
pub(crate) const GLINT_STRENGTH_TOOLTIP: &str =
    "Wie stark der Verzauberungs-Schimmer leuchtet (Vanilla: Glint Strength, Standard 75 %). 0 % = kein Schimmer. Zahl anklicken für einen eigenen Wert.";

// Die Steuerungs-Seite als Liste von Zeilen in Abschnitten (s. CONTROLS_SECTIONS): Regler,
// Schalter und Tastenbelegungen gemischt -- eine Einstellung steht bei der Taste, zu der sie
// gehoert (Sprung-Abklingzeit bei Springen, Sneak-Kantenschutz bei Schleichen, ...).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlsRow {
    SensX,
    SensY,
    SensSplit,
    SensCalc,
    CursorSens,
    CursorSize,
    CursorWidth,
    CursorCircle,
    SneakProtection,
    SnapTap,
    JumpCooldown,
    SprintOnStart,
    HoldUseRepeat,
    RocketRepeat,
    ExtinguishDcGhost,
    UseWhileMining,
    CapsLock,
    HoldConfirm,
    ReconnectHold,
    Bind(KeyBindAction),
}

pub(crate) const CONTROLS_SECTIONS: &[(&str, &[ControlsRow])] = {
    use ControlsRow::{SensX, SensY, SensSplit, SensCalc, CursorSens, CursorSize, CursorWidth, CursorCircle, Bind, SnapTap, JumpCooldown, SneakProtection, SprintOnStart, HoldUseRepeat, RocketRepeat, ExtinguishDcGhost, UseWhileMining, CapsLock, HoldConfirm, ReconnectHold};
    use KeyBindAction as K;
    &[
        ("Maus", &[SensX, SensY, SensSplit, SensCalc]),
        ("Zeiger", &[CursorSens, CursorSize, CursorWidth, CursorCircle]),
        (
            "Bewegung",
            &[
                Bind(K::Forward),
                Bind(K::Backward),
                Bind(K::Left),
                Bind(K::Right),
                SnapTap,
                Bind(K::Jump),
                JumpCooldown,
                Bind(K::Sneak),
                SneakProtection,
                Bind(K::Sprint),
                SprintOnStart,
            ],
        ),
        ("Benutzen", &[Bind(K::Attack), Bind(K::Use), HoldUseRepeat, RocketRepeat, ExtinguishDcGhost, UseWhileMining, Bind(K::PickBlock)]),
        (
            "Inventar",
            &[
                Bind(K::OpenInventory),
                Bind(K::DropItem),
                Bind(K::SwapOffhand),
                Bind(K::Hotbar(1)),
                Bind(K::Hotbar(2)),
                Bind(K::Hotbar(3)),
                Bind(K::Hotbar(4)),
                Bind(K::Hotbar(5)),
                Bind(K::Hotbar(6)),
                Bind(K::Hotbar(7)),
                Bind(K::Hotbar(8)),
                Bind(K::Hotbar(9)),
                Bind(K::HotbarPrev),
                Bind(K::HotbarNext),
                Bind(K::InventoryMoveStack),
                Bind(K::InventoryDropItem),
                Bind(K::InspectItem),
                Bind(K::InspectHeld),
            ],
        ),
        ("Chat", &[Bind(K::OpenChat), Bind(K::OpenCommand)]),
        (
            "Ansicht",
            &[Bind(K::TogglePerspective), Bind(K::Zoom), Bind(K::Freelook), Bind(K::SwapShoulder), Bind(K::PlayerList), Bind(K::HideHud), Bind(K::ToggleDebugOverlay), Bind(K::ToggleProfiler), Bind(K::ToggleHandDebug)],
        ),
        ("Sprachchat", &[Bind(K::PushToTalk), Bind(K::VoiceWhisper), Bind(K::VoiceMute), Bind(K::VoiceDeafen), Bind(K::VoiceConfig)]),
        (
            "Sonstiges",
            &[
                Bind(K::OpenModuleMenu),
                Bind(K::GameModeWheel),
                Bind(K::SpectatorSlower),
                Bind(K::SpectatorFaster),
                Bind(K::VolumeUp),
                Bind(K::VolumeDown),
                CapsLock,
                HoldConfirm,
                Bind(K::Reconnect),
                ReconnectHold,
            ],
        ),
    ]
};

// Alle Zeilen der Seite der Reihe nach; der Index ist die Zeile fuer settings_row_y.
pub(crate) fn controls_rows() -> &'static [ControlsRow] {
    static ZEILEN: std::sync::OnceLock<Vec<ControlsRow>> = std::sync::OnceLock::new();
    ZEILEN.get_or_init(|| CONTROLS_SECTIONS.iter().flat_map(|(_, zeilen)| zeilen.iter().copied()).collect())
}

// Abschnitte der Seite (Name, erste Zeile) fuer SettingsPage.
pub(crate) fn controls_sections() -> &'static [(&'static str, usize)] {
    static ABSCHNITTE: std::sync::OnceLock<Vec<(&'static str, usize)>> = std::sync::OnceLock::new();
    ABSCHNITTE.get_or_init(|| {
        let mut zeile = 0;
        CONTROLS_SECTIONS
            .iter()
            .map(|(name, zeilen)| {
                let anfang = zeile;
                zeile += zeilen.len();
                (*name, anfang)
            })
            .collect()
    })
}

pub(crate) fn controls_row(r: ControlsRow) -> usize {
    controls_rows().iter().position(|&z| z == r).unwrap_or(0)
}

pub(crate) const HOLD_CONFIRM_MAX: u32 = 2000;
pub(crate) const HOLD_CONFIRM_STEP: u32 = 50;
// Reconnect-Taste ohne eigene Dauer: es gilt die allgemeine (linkes Ende des Reglers).
pub(crate) const RECONNECT_HOLD_GENERAL: i32 = -(HOLD_CONFIRM_STEP as i32);
pub(crate) const HOLD_CONFIRM_TOOLTIP: &str = "Wie lange eine Taste gehalten werden muss, bis ihre Aktion ausgelöst wird (wie osus Hold-to-confirm). Gilt für jede Halte-Bestätigung ohne eigene Dauer.";
pub(crate) const RECONNECT_HOLD_TOOLTIP: &str = "Wie lange die Reconnect-Taste gehalten werden muss, bis neu verbunden wird; währenddessen dunkelt das Bild ab. Ganz links: die allgemeine Bestätigungsdauer.";

pub(crate) const CURSOR_SIZE_MIN: u32 = 50;
pub(crate) const CURSOR_SIZE_MAX: u32 = 300;
pub(crate) const CURSOR_SIZE_STEP: u32 = 10;
pub(crate) const CURSOR_WIDTH_MIN: u32 = 50;
pub(crate) const CURSOR_WIDTH_MAX: u32 = 200;
pub(crate) const CURSOR_WIDTH_STEP: u32 = 5;
pub(crate) const CURSOR_WIDTH_TOOLTIP: &str = "Breite des Mauszeiger-Pfeils in Prozent (100 % = bisherige Form). Die Spitze und die Länge bleiben gleich, nur die beiden Flügel rücken zusammen oder auseinander.";
pub(crate) const CURSOR_CIRCLE_TOOLTIP: &str = "Kleiner Kreis unter der Pfeilspitze des Mauszeigers. AUS = nur der Pfeil.";
pub(crate) const CURSOR_SIZE_TOOLTIP: &str = "Größe des Mauszeigers in Menüs, im Inventar und im offenen Chat, in Prozent (100 % = bisherige Größe). Die Spitze bleibt dabei genau dort, wo geklickt wird.";
pub(crate) const KEYBIND_TOOLTIP: &str = "Taste anklicken, dann die neue Taste oder Maustaste drücken; Escape bricht ab. Rechtsklick entfernt die Belegung. H/T daneben: Halten (gilt, solange die Taste unten ist) oder Toggle (ein Druck schaltet an, der nächste wieder aus). Mehrere Tasten für eine Aktion: beim Warten auf die Taste \"Erweitert\" anklicken.";
pub(crate) const CAPS_LOCK_RECHECK: Duration = Duration::from_millis(30);
pub(crate) const CAPS_LOCK_TOOLTIP: &str = "AN: Im Spiel schaltet die Feststelltaste die Großschreibung nicht mehr ein, die Lampe auf der Tastatur bleibt aus. Als Taste funktioniert sie weiter ganz normal (z.B. für Push-to-Talk). Im Chat und in Textfeldern verhält sie sich wie immer.";
pub(crate) const SPRINT_ON_START_TOOLTIP: &str = "AN: Sprinten ist nach dem Start des Clients schon eingeschaltet, als hätte man die Sprint-Taste einmal gedrückt. Wirkt nur, wenn Sprinten links in der Tabelle auf \"Umschalten\" steht -- mit \"Halten\" gibt es keinen Zustand, der anbleiben könnte.";
pub(crate) const JUMP_COOLDOWN_TOOLTIP: &str =
    "Vanilla erlaubt nach jedem echten Sprung (Land UND Wasser/Lava) erst nach 10 Ticks (0.5s) den nächsten -- an Land meist unsichtbar, im flachen Wasser deutlich spürbar. AUS entfernt diese Verzögerung komplett und weicht damit von Vanilla ab. Betrifft NICHT \"Water Hopping\": das Herausklettern an einer Kante (resetVerticalVelocityInFluid) hat gar keine Abklingzeit und läuft unabhängig von dieser Einstellung.";
pub(crate) const USE_WHILE_MINING_TOOLTIP: &str =
    "AUS (wie Vanilla): während man einen Block abbaut, wird jeder Rechtsklick verschluckt -- Vanillas doItemUse() steigt bei `isBreakingBlock()` sofort aus. Im Crystal-PvP kostet das Kristalle: ein verrutschter Linksklick sperrt das Setzen, bis man ihn loslässt. AN führt den Rechtsklick trotzdem aus und lässt den Abbau dabei WEITERLAUFEN -- dasselbe, was die entsprechende Fabric-Mod macht, und es geht dabei kein einziges zusätzliches Paket raus. Standard: an.";
pub(crate) const ROCKET_REPEAT_TOOLTIP: &str = "AN (wie Vanilla): mit einer Feuerwerksrakete in der Hand zündet GEDRÜCKT HALTEN alle 4 Ticks die nächste -- beim Elytra-Flug verbraucht man so schnell mehrere Raketen. AUS: jede Rakete braucht einen eigenen Klick, egal was bei \"Benutzen wiederholen\" steht. Gilt für Raketen in der Haupt- wie in der Nebenhand.";
pub(crate) const EXTINGUISH_DC_GHOST_TOOLTIP: &str = "AN: brennt man beim Ausleeren eines Wasser- oder Pulverschnee-Eimers, wartet der nächste Rechtsklick auf demselben Slot, bis der Server das Ausleeren bestätigt hat (höchstens 20 Ticks). Sonst kann ein schneller Doppelklick das Wasser beim Server im selben Tick setzen und wieder aufnehmen -- es war nie da, das Feuer brennt weiter. Der Klick geht nicht verloren, er kommt nur später. AUS (wie Vanilla): der Klick geht sofort raus.";
pub(crate) const HOLD_USE_REPEAT_TOOLTIP: &str =
    "IMMER (wie Vanilla): wer die Benutzen-Taste GEDRÜCKT HÄLT, benutzt alle 4 Ticks erneut -- so zieht man eine Reihe Blöcke, statt für jeden einzeln zu klicken. AUSSER BLÖCKE: wiederholt wird weiter, nur nicht mit einem Block in der Hand -- Halten setzt genau einen Block, Kristalle, Perlen, Armbrust und Co. wiederholen trotzdem (wie die Mod no-hold-use). NIE: Halten ist immer nur ein einzelner Klick. Betrifft NICHT das Essen/Bogenspannen (das läuft ohnehin als eine durchgehende Benutzung) und nichts am Angriff.";

// Die drei Stufen von "Benutzen wiederholen" (s. Settings::hold_use_repeat).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HoldUseRepeat {
    // Vanilla: alle 4 Ticks erneut, egal womit.
    Always,
    // Wie die Mod no-hold-use: bei der WIEDERHOLUNG faellt jede Hand mit einem Block durch
    // (Vanilla-PASS, es geht fuer sie kein Paket raus), alles andere wiederholt wie gehabt.
    ExceptBlocks,
    // Halten ist ein einzelner Klick.
    Never,
}

impl HoldUseRepeat {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Always => "Immer",
            Self::ExceptBlocks => "Außer Blöcke",
            Self::Never => "Nie",
        }
    }

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::ExceptBlocks => "except_blocks",
            Self::Never => "never",
        }
    }

    // "true"/"false" sind die Werte aus der Zeit, als das noch ein Schalter war.
    pub(crate) fn from_key(v: &str) -> Self {
        match v {
            "except_blocks" => Self::ExceptBlocks,
            "never" | "false" => Self::Never,
            _ => Self::Always,
        }
    }

    pub(crate) const ALL: [Self; 3] = [Self::Always, Self::ExceptBlocks, Self::Never];

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Always => Self::ExceptBlocks,
            Self::ExceptBlocks => Self::Never,
            Self::Never => Self::Always,
        }
    }
}
pub(crate) const SNAP_TAP_TOOLTIP: &str =
    "Bei zwei gegensätzlichen Bewegungstasten (z.B. A und D) gewinnt die ZULETZT gedrückte, statt dass sich beide aufheben; lässt man sie wieder los, wirkt sofort die noch gehaltene. Reine Eingabe-Behandlung im Client -- der Server sieht ganz normale Bewegungspakete, es wird nichts umgangen oder ausgenutzt. Vorsicht: viele kompetitive Server verbieten SnapTap/SOCD per REGELWERK (in CS2 ist es serverseitig gesperrt). Standard: aus.";
pub(crate) const SNEAK_PROTECTION_TOOLTIP: &str =
    "Wie Vanilla: beim Schleichen (Sneak) am Boden kann man nicht über eine Blockkante hinausgehen und herunterfallen -- die Bewegung wird an der Kante gestoppt. AUS erlaubt das Herunterfallen trotz Schleichen. Grim flaggt beide Zustände nicht.";

// Knopfspalte links im Pixel-Editor des Fadenkreuzes, im Zeilenraster der Einstellungen.
pub(crate) const HUD_LIST_ITEM_W: f32 = 220.0;
pub(crate) const HUD_LIST_ITEM_H: f32 = 40.0;
pub(crate) fn hud_list_item_rect(row: usize) -> Rect {
    Rect { x: 40.0, y: settings_row_y(row), w: HUD_LIST_ITEM_W, h: HUD_LIST_ITEM_H }
}
// Ein eigenes Fadenkreuz als UI-Rechtecke (Vorschau): `zelle` Bildschirmpixel je Rasterpixel,
// `mitte` = wohin die Rastermitte faellt. Fuellung in HUD-Farbe, Kontur schwarz.
pub(crate) fn push_pattern_rects(p: &crosshair_editor::Pattern, mitte: (f32, f32), zelle: f32, rects: &mut Vec<UiRect>) {
    for (art, farbe) in [(crosshair_editor::Pixel::Outline, [0.0, 0.0, 0.0, 1.0]), (crosshair_editor::Pixel::Fill, COLOR_HUD)] {
        for (x, y, w) in p.runs(art) {
            rects.push(UiRect {
                rect: Rect { x: mitte.0 + x as f32 * zelle, y: mitte.1 + y as f32 * zelle, w: w as f32 * zelle, h: zelle },
                color: farbe,
                ..UiRect::FLAT
            });
        }
    }
}

// Zeile der Formauswahl im Fadenkreuz-Editor: unter Farbe, Kontur (und Aufhellung).
pub(crate) const fn crosshair_shape_row(color_mode: usize) -> usize {
    if color_mode == CROSSHAIR_MODE_COMPLEMENTARY {
        4
    } else {
        3
    }
}

// Beschriftungen im Einstellungsfenster eines Elements.
pub(crate) const HUD_CONFIG_LABEL_X: f32 = 56.0;
// "Zuruecksetzen" sitzt IMMER unter der letzten Konfig-Zeile des ausgewaehlten Elements --
// deshalb die Zeile als Parameter. Fest auf Zeile 1 verdrahtet lag der Button genau auf der
// zweiten Konfig-Zeile, sobald ein Element mehr als eine hat (Fadenkreuz mit Aufhellung).
// Wie viele Konfig-Zeilen das ausgewaehlte Element belegt -- danach richtet sich, wo
// "Zuruecksetzen" landet. Mindestens eine, auch bei Elementen ohne eigene Werte: dort steht
// in Zeile 0 der Hinweis "Nur Position". Freie Funktion statt Methode, damit sie ohne
// GPU-Zustand testbar bleibt.
// Linke Kante eines w breiten Elements aus seinem Anker (x) und der Ausrichtung -- und zurueck.
pub(crate) fn align_left(anchor: f32, w: f32, align: u8) -> f32 {
    match align {
        1 => anchor - w / 2.0,
        2 => anchor - w,
        _ => anchor,
    }
}

pub(crate) fn align_anchor(left: f32, w: f32, align: u8) -> f32 {
    match align {
        1 => left + w / 2.0,
        2 => left + w,
        _ => left,
    }
}

// Zeilen der Effekte im Einstellungsfenster (s. hud_config_rows).
pub(crate) const EFFECTS_HIDE_AMBIENT_ROW: usize = 3;
pub(crate) const EFFECTS_STYLE_ROW: usize = 4;
pub(crate) const EFFECTS_ALIGN_ROW: usize = 5;
pub(crate) const EFFECTS_ICON_ROW: usize = 6;
pub(crate) const EFFECTS_NAME_ROW: usize = 7;
pub(crate) const EFFECTS_TIMER_ROW: usize = 8;
pub(crate) const EFFECTS_TIMER_COLOR_ROW: usize = 9;
pub(crate) const EFFECTS_BLINK_ROW: usize = 10;

// Auswahl mit mehr als zwei Knoepfen in einer Konfig-Zeile, so breit wie ein Schieberegler.
pub(crate) fn hud_config_pick_rect(row: usize) -> Rect {
    Rect { w: SLIDER_SPAN_W, ..settings_ctrl_rect(row) }
}

// Die drei Ausrichtungsknoepfe in Konfig-Zeile 0 (Titel, Actionbar).
// Groessenregler der HUD-Elemente in Prozent (Titel, Untertitel, Spielerliste).
pub(crate) const HUD_SCALE_PCT_MIN: u32 = 50;
pub(crate) const HUD_SCALE_PCT_MAX: u32 = 300;
pub(crate) const HUD_SCALE_PCT_STEP: u32 = 10;

pub(crate) fn hud_align_button(i: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X + i as f32 * 96.0, y: settings_row_y(0), w: 90.0, h: SETTINGS_CTRL_H }
}

// Ab welchem Abstand (Bildschirmpixel) ein gezogenes Element an der senkrechten Mittellinie
// einrastet.
pub(crate) const HUD_SNAP_PX: f32 = 12.0;

// Schrittweite der Haltbarkeits-Schwelle der Hotbar in Prozent.
pub(crate) const HOTBAR_DURABILITY_STEP: u32 = 5;
// Pauschale Schwelle: waehlbare verbleibende Haltbarkeitspunkte.
pub(crate) const HOTBAR_DURABILITY_POINTS: [u32; 18] = [5, 10, 15, 20, 25, 30, 40, 50, 75, 100, 150, 200, 250, 300, 400, 500, 750, 1000];

pub(crate) fn nearest_durability_points(v: u32) -> u32 {
    *HOTBAR_DURABILITY_POINTS.iter().min_by_key(|&&p| p.abs_diff(v)).unwrap()
}

// Ab wann der Haltbarkeitsbalken eines Items erscheint (s. App::push_item_limited).
#[derive(Clone, Copy)]
pub(crate) enum DurabilityBarLimit {
    Always,
    // Hoechstens dieser Anteil (0..1) uebrig.
    Rest(f32),
    // Hoechstens so viele Punkte uebrig.
    Points(i32),
}

// Kamera-Empfindlichkeit (X, Y): gleich, solange "X/Y getrennt" aus ist.
pub(crate) const fn camera_sensitivities(settings: &Settings) -> (f32, f32) {
    let x = settings.mouse_sensitivity;
    (x, if settings.mouse_sens_split { settings.mouse_sensitivity_y } else { x })
}

pub(crate) const fn hud_config_rows(element: HudElement, set: &Settings) -> usize {
    let crosshair_color_mode = set.crosshair_color_mode;
    match element {
        // Skalierung, Haltbarkeit ab, Grenze prozentual/pauschal, Sauerstoff Blasen/Balken,
        // Farbe der Stueckzahl.
        HudElement::Hotbar => 5,
        // Zeile 0 Farbe, Zeile 1 Kontur, Zeile 2 Kontur des Rings, Zeile 3 Aufhellung (nur beim
        // Komplementaeren). Dazu "Form" und darunter "Groesse" bzw. "Pixel bearbeiten".
        HudElement::Crosshair if crosshair_color_mode == CROSSHAIR_MODE_COMPLEMENTARY => 6,
        HudElement::Crosshair => 5,
        // Ausrichtung + Groesse.
        HudElement::Title | HudElement::Subtitle => 2,
        // Groesse, Im HUD, Im Inventar, Ambiente verstecken, Stil -- beim detaillierten dazu Ausrichtung, Icon,
        // Name (mit Farbe), Timer, Timer-Farbe und Blinken.
        HudElement::Effects if set.effects_detailed => EFFECTS_BLINK_ROW + 1,
        HudElement::Effects => EFFECTS_STYLE_ROW + 1,
        HudElement::Perf => 0,
        // Groesse.
        HudElement::Scoreboard => 1,
        // Farbe des Countdowns (Countdown oder Kreis waehlt das Schilde-Modul).
        HudElement::ShieldCooldown => 1,
        // Groesse, Nur Zahl, Deckkraft, Zahl-Farbe.
        HudElement::Arrows => 4,
        HudElement::ActionBar => 1,
        // Groesse.
        HudElement::BossBar => 1,
        // Groesse.
        HudElement::HeldName => 1,
        // Groesse, Ausrichtung, Ruecken, Abstand, Zahlgroesse, Zahl x und y, Gruen ab 87 %,
        // Mending-Ring.
        HudElement::Armor => 9,
        // Groesse, Deckkraft.
        HudElement::Keystrokes => 2,
        HudElement::Session => 2,
        // Die Chat-Einstellungen (s. CHAT_TOOLTIPS): Groesse, Breite, Hintergrund, Abdunkeln,
        // Zeilen im Spiel und beim Tippen, Ausblenden.
        HudElement::Chat => CHAT_TOOLTIPS.len(),
    }
}

// Das Einstellungsfenster eines Elements in Inhaltskoordinaten: Titel ueber Zeile 0, unten
// "Zuruecksetzen", rechts Platz fuer die Einheit hinter den Reglern.
pub(crate) fn hud_config_panel(element: HudElement, set: &Settings) -> Rect {
    let x = HUD_CONFIG_LABEL_X - 28.0;
    let y = SETTINGS_FIRST_Y - 72.0;
    let unten = hud_reset_button(hud_config_rows(element, set)).y + SETTINGS_CTRL_H + 28.0;
    Rect { x, y, w: SETTINGS_CTRL_X + SLIDER_SPAN_W + 100.0 - x, h: unten - y }
}

// "Schliessen" oben rechts im Einstellungsfenster.
pub(crate) fn hud_config_close_button(panel: Rect) -> Rect {
    Rect { x: panel.x + panel.w - 28.0 - 140.0, y: panel.y + 18.0, w: 140.0, h: SETTINGS_CTRL_H }
}

// Farbfeld der Kreuz-Farbe im Einstellungsfenster, rechts neben Komplementaer/Einfarbig.
pub(crate) fn crosshair_swatch_rect() -> Rect {
    let wahl = settings_wide_rect(0);
    Rect { x: wahl.x + wahl.w + STEPPER_GAP, w: SWATCH_W, ..wahl }
}

pub(crate) fn hud_reset_button(row: usize) -> Rect {
    Rect { x: HUD_CONFIG_LABEL_X, y: settings_row_y(row), w: 210.0, h: 36.0 }
}

// Seiten-Scroll der Einstellungen (s. MenuScreen::settings_page): erste sichtbare Zeile, die
// Unterkante, an der Zeilen aufhoeren, und wo Abschnitte beginnen. Gesetzt von
// App::sync_settings_scroll, bevor gezeichnet oder geklickt wird. Alle Positionen laufen ueber
// settings_row_y -- Zeilen ausserhalb des sichtbaren Bereichs landen weit ausserhalb des
// Bildes, damit stimmen Zeichnung, Klickflaechen und Tooltips ohne jeden Sonderfall. Auf
// Seiten ohne Seiten-Scroll 0, unbegrenzt und ohne Abschnitte.
// Die erste Zeile gleitend (f32-Bits, auch Bruchteile und jenseits der Seite, s. smooth_scroll).
pub(crate) static SETTINGS_SCROLL_FIRST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub(crate) static SETTINGS_SCROLL_BOTTOM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(f32::INFINITY.to_bits());
// Abschnittsmaske (s. SettingsPage::section_starts) in zwei Haelften: es gibt kein AtomicU128.
pub(crate) static SETTINGS_SECTION_STARTS: [std::sync::atomic::AtomicU64; 2] = [std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0)];

pub(crate) fn store_section_starts(starts: u128) {
    SETTINGS_SECTION_STARTS[0].store(starts as u64, std::sync::atomic::Ordering::Relaxed);
    SETTINGS_SECTION_STARTS[1].store((starts >> 64) as u64, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn load_section_starts() -> u128 {
    u128::from(SETTINGS_SECTION_STARTS[0].load(std::sync::atomic::Ordering::Relaxed)) | u128::from(SETTINGS_SECTION_STARTS[1].load(std::sync::atomic::Ordering::Relaxed)) << 64
}

// Zusaetzlicher Platz vor jedem Abschnitt ausser dem obersten: zusammen mit dem Abstand
// zwischen zwei Zeilen genug fuer seine Trennlinie, der Name steht rechts darueber.
pub(crate) const SETTINGS_SECTION_GAP: f32 = 12.0;

pub(crate) fn settings_row_y(row: usize) -> f32 {
    use std::sync::atomic::Ordering::Relaxed;
    let erste = f32::from_bits(SETTINGS_SCROLL_FIRST.load(Relaxed));
    let unten = f32::from_bits(SETTINGS_SCROLL_BOTTOM.load(Relaxed));
    settings_row_y_cut(row, erste, load_section_starts(), unten).unwrap_or(-10_000.0)
}

// Oberkante des Zeilenbands (Inhaltskoordinaten): knapp unter der Linie unter den Reitern.
// Darueber schneidet der Scissor-Test angeschnittene Zeilen ab (s. content_clip).
pub(crate) const SETTINGS_CLIP_TOP: f32 = SETTINGS_FIRST_Y - 4.0;

// Wie settings_row_y_in, aber auch eine nur angeschnittene Zeile hat ihren Platz: was ueber
// das Zeilenband ragt, schneidet die GPU ab (s. content_clip), statt dass die Zeile beim
// ersten Pixel ganz verschwindet.
pub(crate) fn settings_row_y_cut(row: usize, erste: f32, starts: u128, unten: f32) -> Option<f32> {
    let y = SETTINGS_FIRST_Y + settings_row_pos(row as f32, starts) - settings_row_pos(erste, starts);
    (y + SETTINGS_ROW_H > SETTINGS_CLIP_TOP && y < unten).then_some(y)
}

// Das Zeilenband der Einstellungsseite in Bildschirmpixeln (x0, y0, x1, y1), solange eine mit
// Seiten-Scroll offen ist (s. App::sync_settings_scroll). Inhalt zwischen push_clip_marker(an)
// und (aus) wird darauf beschnitten -- per Scissor-Rechteck, kostet die GPU nichts -- und
// Knoepfe sind nur darin anklickbar.
pub(crate) static CONTENT_CLIP: [std::sync::atomic::AtomicU32; 4] = [
    std::sync::atomic::AtomicU32::new(0),
    std::sync::atomic::AtomicU32::new(0),
    std::sync::atomic::AtomicU32::new(0),
    std::sync::atomic::AtomicU32::new(0),
];

pub(crate) fn store_content_clip(r: Option<Rect>) {
    let w = r.map_or([0.0; 4], |r| [r.x, r.y, r.x + r.w, r.y + r.h]);
    for (a, v) in CONTENT_CLIP.iter().zip(w) {
        a.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn content_clip() -> Option<Rect> {
    let [x0, y0, x1, y1] = std::array::from_fn(|i| f32::from_bits(CONTENT_CLIP[i].load(std::sync::atomic::Ordering::Relaxed)));
    (x1 > x0).then_some(Rect { x: x0, y: y0, w: x1 - x0, h: (y1 - y0).max(0.0) })
}

// Marker in Flaechen- und Textliste: ab hier wird auf content_clip beschnitten (an) bzw. nicht
// mehr (aus). Leere Flaeche und leerer Text, die render_menu liest und nicht zeichnet.
pub(crate) fn push_clip_marker(rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, an: bool) {
    let code = if an { -50.0 } else { -51.0 };
    rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, color: [0.0, 0.0, 0.0, code], ..UiRect::FLAT });
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, code], scale: 1.0 });
}

pub(crate) fn clip_marker_rect(r: &UiRect) -> Option<bool> {
    (r.rect.w == 0.0 && r.rect.h == 0.0).then_some(()).and_then(|()| match r.color[3] {
        -50.0 => Some(true),
        -51.0 => Some(false),
        _ => None,
    })
}

pub(crate) fn clip_marker_text(t: &UiText) -> Option<bool> {
    if !t.text.is_empty() {
        return None;
    }
    match t.color[3] {
        -50.0 => Some(true),
        -51.0 => Some(false),
        _ => None,
    }
}

// Raender einer gescrollten Liste wie in osu!: wo es weitergeht, ein Verlauf ueber die Pixel,
// an der Kante `top` nur noch `edge_top` Deckkraft, `zone` Pixel tiefer wieder volle; unten
// genauso von `bottom` nach oben mit `edge_bottom`. Jenseits der Kanten bleibt es bei der
// Deckkraft der Kante (dort schneidet die Liste ohnehin hart ab). Bildschirmpixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollFade {
    pub(crate) top: f32,
    pub(crate) bottom: f32,
    pub(crate) zone: f32,
    pub(crate) edge_top: f32,
    pub(crate) edge_bottom: f32,
}

// Deckkraft an der Kante (osu! geht fast bis 0, hier bewusst weniger).
pub(crate) const SCROLL_FADE_EDGE: f32 = 0.4;
// Laenge des Verlaufs als Anteil einer Zeile.
pub(crate) const SCROLL_FADE_LEN: f32 = 0.5;

impl ScrollFade {
    // Fuer eine Liste zwischen `top` und `bottom` mit Zeilen der Hoehe `row_h`, um `rows` von
    // `max` Zeilen gescrollt: an einem Ende ohne weiteren Inhalt kein Verlauf, ueber die erste
    // Zeile Scrollweg blendet er ein.
    pub(crate) fn new(top: f32, bottom: f32, row_h: f32, rows: f32, max: f32) -> Self {
        let kante = |weiter: f32| 1.0 - (1.0 - SCROLL_FADE_EDGE) * weiter.clamp(0.0, 1.0);
        let zone = (row_h * SCROLL_FADE_LEN).min((bottom - top) / 2.0).max(1.0);
        Self { top, bottom, zone, edge_top: kante(rows), edge_bottom: kante(max - rows) }
    }

    // Grenzen, an denen der Verlauf abknickt (dort werden Flaechen geteilt).
    pub(crate) fn knicke(self) -> [f32; 4] {
        [self.top, self.top + self.zone, self.bottom - self.zone, self.bottom]
    }

    // Ganz in der Mitte, also unveraendert.
    pub(crate) fn voll(self, lo: f32, hi: f32) -> bool {
        lo >= self.top + self.zone && hi <= self.bottom - self.zone
    }

    pub(crate) fn alpha(self, y: f32) -> f32 {
        let oben = self.edge_top + (1.0 - self.edge_top) * ((y - self.top) / self.zone).clamp(0.0, 1.0);
        let unten = self.edge_bottom + (1.0 - self.edge_bottom) * ((self.bottom - y) / self.zone).clamp(0.0, 1.0);
        oben.min(unten)
    }
}

// Ab hier faden Flaechen und Text an den Listenraendern (Some) bzw. nicht mehr (None). Marker
// wie push_clip_marker: leere Flaeche bzw. leerer Text, die Kanten stehen in y und x -- so
// wandern sie mit, wenn der Inhalt verschoben wird (s. content_offset).
pub(crate) fn push_fade_marker(rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, fade: Option<ScrollFade>) {
    let f = fade.unwrap_or(ScrollFade { top: 0.0, bottom: 0.0, zone: 0.0, edge_top: 1.0, edge_bottom: 1.0 });
    let code = if fade.is_some() { -52.0 } else { -53.0 };
    // Text hat kein freies Feld fuer die Unterkante: Abstand zur Oberkante in scale.
    rects.push(UiRect { rect: Rect { x: f.bottom - f.top, y: f.top, w: 0.0, h: 0.0 }, color: [f.zone, f.edge_top, f.edge_bottom, code], ..UiRect::FLAT });
    texts.push(UiText { text: String::new(), x: 0.0, y: f.top, color: [f.zone, f.edge_top, f.edge_bottom, code], scale: f.bottom - f.top });
}

pub(crate) fn fade_aus_marker(top: f32, hoehe: f32, c: [f32; 4]) -> Option<Option<ScrollFade>> {
    match c[3] {
        -52.0 => Some(Some(ScrollFade { top, bottom: top + hoehe, zone: c[0], edge_top: c[1], edge_bottom: c[2] })),
        -53.0 => Some(None),
        _ => None,
    }
}

pub(crate) fn fade_marker_rect(r: &UiRect) -> Option<Option<ScrollFade>> {
    if r.rect.w != 0.0 || r.rect.h != 0.0 {
        return None;
    }
    fade_aus_marker(r.rect.y, r.rect.x, r.color)
}

// Ab hier wird Text um (x, y) auf `s` verkleinert (Some) bzw. nicht mehr (None) -- erst die
// fertigen Glyphen-Vierecke, stufenlos: neu gerastert in kleinerer Schrift sprang er von Pixel
// zu Pixel (gedrueckte Einstellungsknoepfe, s. menu/press.rs). Nur Text, Flaechen werden
// ohnehin ungerundet gezeichnet.
pub(crate) fn push_zoom_marker(texts: &mut Vec<UiText>, zoom: Option<(f32, f32, f32)>) {
    let (x, y, s) = zoom.unwrap_or((0.0, 0.0, 1.0));
    let code = if zoom.is_some() { -54.0 } else { -55.0 };
    texts.push(UiText { text: String::new(), x, y, color: [s, 0.0, 0.0, code], scale: 1.0 });
}

pub(crate) fn zoom_marker_text(t: &UiText) -> Option<Option<(f32, f32, f32)>> {
    if !t.text.is_empty() {
        return None;
    }
    match t.color[3] {
        -54.0 => Some(Some((t.x, t.y, t.color[0]))),
        -55.0 => Some(None),
        _ => None,
    }
}

pub(crate) fn fade_marker_text(t: &UiText) -> Option<Option<ScrollFade>> {
    if !t.text.is_empty() {
        return None;
    }
    fade_aus_marker(t.y, t.scale, t.color)
}

pub(crate) fn ndc_y_px(y: f32, screen_h: f32) -> f32 {
    (1.0 - y) / 2.0 * screen_h
}

// Die Dreiecke einer Flaeche mit dem Verlauf: was die Knicke des Verlaufs schneidet, wird
// dort geteilt (alle Attribute linear dazwischen), damit jedes Stueck zwischen zwei Knicken
// liegt -- dann ist die lineare Deckkraft je Ecke genau der Verlauf, auch bei grossen Karten.
pub(crate) fn fade_quad_verts(verts: &[Vertex], fade: ScrollFade, screen_h: f32, out: &mut Vec<Vertex>) {
    let y = |v: &Vertex| ndc_y_px(v.position[1], screen_h);
    let lerp = |a: &Vertex, b: &Vertex, t: f32| {
        let m = |p: &[f32], q: &[f32], i: usize| p[i] + (q[i] - p[i]) * t;
        Vertex {
            position: std::array::from_fn(|i| m(&a.position, &b.position, i)),
            color: std::array::from_fn(|i| m(&a.color, &b.color, i)),
            local: std::array::from_fn(|i| m(&a.local, &b.local, i)),
            shape: a.shape,
            border_color: std::array::from_fn(|i| m(&a.border_color, &b.border_color, i)),
        }
    };
    // Vieleck an der Linie y = c teilen (Sutherland-Hodgman): (darueber, darunter).
    let teilen = |poly: &[Vertex], c: f32| {
        let (mut oben, mut unten) = (Vec::new(), Vec::new());
        for i in 0..poly.len() {
            let (a, b) = (&poly[i], &poly[(i + 1) % poly.len()]);
            let (ya, yb) = (y(a), y(b));
            if ya < c { oben.push(*a) } else { unten.push(*a) }
            if (ya < c) != (yb < c) {
                let m = lerp(a, b, (c - ya) / (yb - ya));
                oben.push(m);
                unten.push(m);
            }
        }
        (oben, unten)
    };
    for tri in verts.chunks_exact(3) {
        let (lo, hi) = tri.iter().map(y).fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
        if fade.voll(lo, hi) {
            out.extend_from_slice(tri);
            continue;
        }
        let mut rest = tri.to_vec();
        let mut stuecke = Vec::with_capacity(5);
        for c in fade.knicke() {
            let (oben, unten) = teilen(&rest, c);
            stuecke.push(oben);
            rest = unten;
        }
        stuecke.push(rest);
        for stueck in stuecke {
            for k in 1..stueck.len().saturating_sub(1) {
                for mut v in [stueck[0], stueck[k], stueck[k + 1]] {
                    let a = fade.alpha(y(&v));
                    v.color[3] *= a;
                    v.border_color[3] *= a;
                    out.push(v);
                }
            }
        }
    }
}

// Text: Glyphen sind klein, dort reicht die Deckkraft je Ecke (ohne Teilen). Heller Text auf
// hellem Grund wirkt bei gleicher Deckkraft viel kraeftiger als die dunkle Karte darunter, er
// blendet darum mit dem Quadrat aus (am Rand 16 % statt 40 %).
pub(crate) fn fade_text_verts(verts: &mut [TextVertex], fade: ScrollFade, screen_h: f32) {
    for v in verts {
        v.color[3] *= fade.alpha(ndc_y_px(v.position[1], screen_h)).powi(2);
    }
}

// Bilder (Server-Icons, Pack-Bilder) haben eine Farbe je Bild: in den Verlaeufen in duenne
// Streifen geteilt, jeder mit der Deckkraft seiner Mitte. Nur ungedrehte Bilder (Ecken im
// Uhrzeigersinn ab links oben, s. UiSprite::quad).
pub(crate) fn fade_sprite(s: UiSprite, fade: ScrollFade, out: &mut Vec<UiSprite>) {
    const STREIFEN: f32 = 2.0;
    let [[x0, y0], _, [x1, y1], _] = s.corners;
    if fade.voll(y0, y1) || y1 <= y0 {
        out.push(s);
        return;
    }
    let [u0, v0, u1, v1] = s.uv;
    let teil = |a: f32, b: f32, deckung: f32| {
        let (ta, tb) = ((a - y0) / (y1 - y0), (b - y0) / (y1 - y0));
        let mut farbe = s.color;
        farbe[3] *= deckung;
        UiSprite::quad(x0, a, x1 - x0, b - a, [u0, v0 + (v1 - v0) * ta, u1, v0 + (v1 - v0) * tb], farbe, s.atlas)
    };
    let [_, mitte_ab, mitte_bis, _] = fade.knicke();
    let mut y = y0;
    while y < y1 {
        // Die Mitte (volle Deckkraft) am Stueck, sonst Streifen.
        let b = if y >= mitte_ab && y < mitte_bis { mitte_bis.min(y1) } else { (y + STREIFEN).min(y1).min(if y < mitte_ab { mitte_ab } else { y1 }) };
        let b = b.max(y + 0.01).min(y1);
        out.push(teil(y, b, fade.alpha(f32::midpoint(y, b))));
        y = b;
    }
}

// Den Scissor fuer den naechsten Zeichenaufruf: an = auf content_clip, sonst ganzer Schirm.
// false, wenn das Band leer ist (dann nichts zeichnen).
pub(crate) fn set_ui_clip(pass: &mut wgpu::RenderPass<'_>, an: bool, screen_px: (f32, f32)) -> bool {
    set_scissor(pass, content_clip().filter(|_| an), screen_px)
}

// Scissor auf `r` (Bildschirmpixel), None = ganzer Schirm. false, wenn `r` leer ist.
pub(crate) fn set_scissor(pass: &mut wgpu::RenderPass<'_>, r: Option<Rect>, screen_px: (f32, f32)) -> bool {
    let (w, h) = (screen_px.0.max(1.0) as u32, screen_px.1.max(1.0) as u32);
    match r {
        Some(r) => {
            let x0 = (r.x.max(0.0) as u32).min(w);
            let y0 = (r.y.max(0.0) as u32).min(h);
            let x1 = ((r.x + r.w).ceil().max(0.0) as u32).min(w);
            let y1 = ((r.y + r.h).ceil().max(0.0) as u32).min(h);
            if x1 <= x0 || y1 <= y0 {
                return false;
            }
            pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
        }
        None => pass.set_scissor_rect(0, 0, w, h),
    }
    true
}

// `bereich` in Abschnitte teilen: an jedem Wechsel in `clips` (ab Vertex, an/aus) beginnt ein
// neuer, mit seinem Zustand. Vor dem ersten Wechsel ist aus.
pub(crate) fn clip_segments(bereich: std::ops::Range<u32>, clips: &[(u32, bool)]) -> Vec<(std::ops::Range<u32>, bool)> {
    let mut an = clips.iter().take_while(|&&(ab, _)| ab <= bereich.start).last().is_some_and(|&(_, a)| a);
    let mut von = bereich.start;
    let mut out = Vec::new();
    for &(ab, neu) in clips.iter().filter(|&&(ab, _)| ab > bereich.start).chain(std::iter::once(&(bereich.end, false))) {
        let ab = ab.min(bereich.end);
        if ab > von {
            out.push((von..ab, an));
            von = ab;
        }
        an = neu;
        if von >= bereich.end {
            break;
        }
    }
    out
}

// `bereich` abschnittsweise zeichnen (s. clip_segments), jeder mit seinem Scissor (s.
// set_ui_clip). Danach wieder der ganze Schirm.
pub(crate) fn draw_clipped(pass: &mut wgpu::RenderPass<'_>, bereich: std::ops::Range<u32>, clips: &[(u32, bool)], screen_px: (f32, f32), mut draw: impl FnMut(&mut wgpu::RenderPass<'_>, std::ops::Range<u32>)) {
    if clips.is_empty() {
        draw(pass, bereich);
        return;
    }
    for (r, an) in clip_segments(bereich, clips) {
        if set_ui_clip(pass, an, screen_px) {
            draw(pass, r);
        }
    }
    set_ui_clip(pass, false, screen_px);
}

// Wo Zeile `row` steht, wenn Zeile `erste` oben ist (gleitend, auch Bruchteile); None, wenn
// sie ueber die oberste Zeile ragt oder nicht mehr ganz vor `unten` passt. `starts`: Bit n =
// Abschnitt beginnt bei Zeile n. Vor jedem Abschnittsanfang unterhalb von `erste` kommt
// SETTINGS_SECTION_GAP dazu. Ein Abschnitt, der oben steht, braucht keine Trennlinie: dort
// zeigt der Reiter, wo man ist.
pub(crate) fn settings_row_y_in(row: usize, erste: f32, starts: u128, unten: f32) -> Option<f32> {
    let y = SETTINGS_FIRST_Y + settings_row_pos(row as f32, starts) - settings_row_pos(erste, starts);
    (y >= SETTINGS_FIRST_Y - 0.5 && y + SETTINGS_ROW_H <= unten).then_some(y)
}

// Abstand der Zeile `row` von Zeile 0, Abschnittsluecken eingerechnet. Zwischen zwei Zeilen
// (beim Gleiten) linear, vor der ersten Zeile im Zeilenraster weiter.
pub(crate) fn settings_row_pos(row: f32, starts: u128) -> f32 {
    let ganz = |r: i64| {
        let luecken = if r <= 0 { 0 } else { (starts & if r >= 127 { u128::MAX } else { (1u128 << (r + 1)) - 1 }).count_ones() };
        r as f32 * SETTINGS_ROW_H + luecken as f32 * SETTINGS_SECTION_GAP
    };
    let unten = row.floor();
    let a = ganz(unten as i64);
    a + (row - unten) * (ganz(unten as i64 + 1) - a)
}

// Unterkante der Zeilen bei dieser Fensterhoehe -- darunter bleibt etwas Rand.
pub(crate) fn settings_rows_bottom(screen_h: f32) -> f32 {
    screen_h - SETTINGS_CONTENT_DY - 24.0
}

// Kleinste erste Zeile, bei der die letzte von `rows` Zeilen ganz sichtbar ist: so weit laesst
// sich die Seite scrollen. 0, wenn alles passt.
pub(crate) fn settings_max_first(rows: usize, starts: u128, unten: f32) -> usize {
    let letzte = rows.saturating_sub(1);
    (0..letzte).find(|&erste| settings_row_y_in(letzte, erste as f32, starts, unten).is_some()).unwrap_or(letzte)
}

// Wie viele Einstellungs-Zeilen bei dieser Fensterhoehe ganz sichtbar sind -- darunter
// brauchen Fussleiste (Zuruecksetzen, Speicherstatus) und etwas Rand ihren Platz.
pub(crate) fn settings_visible_rows(screen_h: f32) -> usize {
    (((screen_h - SETTINGS_CONTENT_DY - SETTINGS_FIRST_Y - 24.0) / SETTINGS_ROW_H).floor() as usize).max(1)
}

// Knopf "Ueberspringen" rechts neben dem Sichtfeld-Regler (hinter der Einheit).
pub(crate) fn skip_perspective_rect(row: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X - SLIDER_GROW + SLIDER_SPAN_W + 40.0, y: settings_row_y(row), w: 150.0, h: SETTINGS_CTRL_H }
}

pub(crate) fn stepper_value_rect(row: usize) -> Rect {
    Rect { x: SETTINGS_CTRL_X + STEPPER_BTN_W + STEPPER_GAP, y: settings_row_y(row), w: STEPPER_VALUE_W, h: SETTINGS_CTRL_H }
}

pub(crate) const LOG_LINE_HEIGHT: f32 = 24.0;
// Nur fuer die Verbindungsanzeige (s. OVERLAY_LOG_LINES); das ganze Log steht im Launcher.
pub(crate) const LOG_MAX_LINES: usize = 50;
