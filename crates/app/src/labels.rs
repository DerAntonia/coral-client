// Chat-Ausschnitt, Teams und Tab-Liste, Namensschilder, Schilder, Display-Entities und
// Zeilenumbruch fuer Beschriftungen.

use crate::*;

// Welcher Ausschnitt des Chatverlaufs gezeichnet wird und um wie viele Zeilen der Block
// dafuer nach unten waechst.
//
// `basis` sind die Zeilen, die ueber der Eingabezeile Platz haben, `max_wachstum` die, die
// darunter noch frei sind (bis zur Hotbar). Die Regel: eine Radstufe kostet erst dann eine
// Zeile am unteren Ende, wenn der Platz darunter aufgebraucht ist. Vorher waechst der Block
// stattdessen -- die oberste Zeile bleibt liegen, alles darunter rueckt um GENAU eine Zeile
// tiefer, und unten faellt nichts weg.
//
// Rueckgabe: (Wachstum in Zeilen, erster gezeichneter Index, erster NICHT mehr gezeichneter).
pub(crate) fn chat_scroll_layout(gesamt: usize, basis: usize, max_wachstum: usize, scroll: usize) -> (usize, usize, usize) {
    let wachstum = scroll.min(max_wachstum);
    let ende = gesamt - (scroll - wachstum);
    let anfang = ende.saturating_sub(basis + wachstum);
    (wachstum, anfang, ende)
}
// Chat-Einstellungen (Settings::chat_*), s. Chat-Tab.
pub(crate) const CHAT_SCALE_MIN: u32 = 50;
pub(crate) const CHAT_SCALE_MAX: u32 = 300;
pub(crate) const CHAT_SCALE_STEP: u32 = 5;
pub(crate) const CHAT_WIDTH_MIN: u32 = 80;
pub(crate) const CHAT_WIDTH_MAX: u32 = 800;
pub(crate) const CHAT_WIDTH_STEP: u32 = 20;
// So breit war der Chat vor der Einstellung (160 Font-Pixel).
pub(crate) const CHAT_WIDTH_DEFAULT: u32 = 160;
pub(crate) const CHAT_LINES_MAX: u32 = 40;
// Wie lange eine Nachricht im geschlossenen Chat stehen bleibt (Settings::chat_fade_secs),
// die Stufen des Steppers; 0 = nie ausblenden. Vanilla: 10 s.
pub(crate) const CHAT_FADE_STEPS: [u32; 13] = [0, 1, 2, 3, 5, 7, 10, 15, 20, 30, 45, 60, 120];
// Wie schnell eine Nachricht nach Ablauf verschwindet (Vanilla blendet eine volle Sekunde
// lang aus -- hier bewusst kurz, so gewuenscht).
pub(crate) const CHAT_FADE_OUT_SECS: f32 = 0.25;

// Deckkraft einer Nachricht im geschlossenen Chat, `alter` Sekunden nach dem Eintreffen.
pub(crate) fn chat_fade_alpha(alter: f32, anzeige_secs: u32) -> f32 {
    if anzeige_secs == 0 {
        return 1.0;
    }
    let rest = anzeige_secs as f32 - alter;
    (rest / CHAT_FADE_OUT_SECS).clamp(0.0, 1.0)
}

pub(crate) fn chat_fade_step(aktuell: u32, schritt: i32) -> u32 {
    // Ein Wert zwischen zwei Stufen (von Hand in die Datei geschrieben) rastet in
    // Schrittrichtung auf der naechsten Stufe ein.
    let i = CHAT_FADE_STEPS.iter().position(|&v| v >= aktuell).unwrap_or(CHAT_FADE_STEPS.len() - 1) as i32;
    let ziel = if CHAT_FADE_STEPS[i as usize] == aktuell {
        i + schritt
    } else if schritt < 0 {
        i - 1
    } else {
        i
    };
    CHAT_FADE_STEPS[ziel.clamp(0, CHAT_FADE_STEPS.len() as i32 - 1) as usize]
}
pub(crate) const CHAT_LINES_OPEN_MAX: u32 = 100;
// Zeilen je Mausrad-Raste im offenen Chat.
pub(crate) const CHAT_SCROLL_LINES_PER_NOTCH: f32 = 3.0;
// Text-Vergroesserung als GANZZAHLIGER Faktor: der Minecraft-Font ist ein Bitmap mit
// 8px hohen Zellen. Jeder Bruchfaktor verteilt Quellpixel ungleich auf Zielpixel (mal
// zwei Spalten breit, mal eine) -- der Font sieht dann ausgefranst aus. Deshalb ist
// UiText::scale ein u32: Bruchskalen sind hier nicht "unerwuenscht", sondern unmoeglich.
pub(crate) const UI_SCALE_TITLE: f32 = 5.0; // 40px -- Ueberschriften (Pause)
pub(crate) const UI_SCALE_NORMAL: f32 = 3.0; // 24px hohe Zeilen
pub(crate) const UI_SCALE_SMALL: f32 = 2.0; // 16px -- Log, Hinweise, Statistik

// Fenster fuer die Mittelwerte der Frame-Metrik: eine Sekunde. Die Position des HUD-
// Elements steckt jetzt in den Settings (per HUD-Editor verschiebbar, s. hud_perf_rect).
pub(crate) const HUD_WINDOW: Duration = Duration::from_secs(1);
// Fenster der Performance-Anzeige: eine halbe Sekunde -- ein Ausreisser haengt in "work" nur
// so lange (s. FrameStats).
pub(crate) const HUD_PERF_WINDOW: Duration = Duration::from_millis(500);

// Ab dieser Laenge wird ein Weltgeraeusch nachgefuehrt (s. App::play_tracked).
pub(crate) const TRACK_MIN_SECONDS: f32 = 0.4;

// Wo ein nachgefuehrtes Geraeusch herkommt.
pub(crate) enum SoundSource {
    // Fester Ort (Block, Explosion, Jukebox).
    Fixed(DVec3),
    // An einer Entity, mit ihrer zuletzt bekannten Position.
    Entity(i32, DVec3),
    // Am eigenen Kopf.
    Own,
}

pub(crate) struct TrackedSound {
    pub(crate) handle: u32,
    pub(crate) source: SoundSource,
    // Wann die Aufnahme sicher zu Ende ist -- danach faellt der Eintrag weg.
    pub(crate) ends: Instant,
}
pub(crate) const COLOR_HUD: [f32; 4] = [1.0, 1.0, 0.4, 1.0];
// Die FPS-Anzeige (s. push_hud_perf_texts).
pub(crate) const COLOR_PERF: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
// Wie lange ein Actionbar-Text ueber der Hotbar stehen bleibt (Vanilla ~3s), danach weg.
pub(crate) const ACTION_BAR_DURATION: Duration = Duration::from_secs(3);
// Vanillas Sichtbarkeitsregeln fuer den Namen eines SPIELERS, zusammengefasst an einer Stelle
// (LivingEntityRenderer.hasLabel + EntityRenderer.updateRenderState):
//
//   - der eigene Koerper traegt nie einen Namen,
//   - ein unsichtbarer Spieler auch nicht (`bl = !isInvisibleTo(player)`),
//   - ueber 64 Bloecken gar keiner (`squaredDistanceToCamera < 4096`),
//   - und wer schleicht, nur bis 32 (`if (d >= 1024.0) return false`).
//
// Als eigene Funktion, weil genau diese vier Regeln vorher fehlten: der Name eines vanishten
// Spielers hing frei in der Luft, und schleichende Gegner waren ueber die ganze Karte
// sichtbar.
pub(crate) fn player_label_visible(is_own: bool, invisible: bool, sneaking: bool, distance_sq: f64) -> bool {
    if is_own || invisible {
        return false;
    }
    let grenze = if sneaking { NAMETAG_SNEAK_DISTANCE } else { NAMETAG_MAX_DISTANCE };
    distance_sq < grenze * grenze
}

// AbstractTeam.VisibilityRule als Index (s. packets::TeamUpdate::name_tag_visibility).
pub(crate) const TEAM_VISIBILITY_ALWAYS: u8 = 0;
pub(crate) const TEAM_VISIBILITY_NEVER: u8 = 1;
pub(crate) const TEAM_VISIBILITY_HIDE_FOR_OTHER_TEAMS: u8 = 2;
pub(crate) const TEAM_VISIBILITY_HIDE_FOR_OWN_TEAM: u8 = 3;

// Was der Client von einem Scoreboard-Team wissen muss. Prefix/Suffix speisen die
// Seitenleiste (Team.decorateName), die Sichtbarkeitsregel und die Farbe das Namensschild.
#[derive(Clone, Default)]
pub(crate) struct TeamInfo {
    pub(crate) prefix: String,
    pub(crate) suffix: String,
    pub(crate) name_tag_visibility: u8,
    pub(crate) color: i32,
    // AbstractTeam.CollisionRule als Index (s. team_allows_push).
    pub(crate) collision_rule: u8,
}

// Darf ein Entity aus Team `schieber` (Regel `r1`) ein Entity aus Team `ziel` (Regel `r2`)
// schubsen? Vanillas EntityPredicates.canBePushedBy, nachgerechnet am 1.21.11-Jar:
// NEVER (1) auf einer der beiden Seiten verbietet es; Regel 2 erlaubt es nur zwischen
// VERSCHIEDENEN Teams, Regel 3 nur innerhalb DESSELBEN Teams; ohne Team gilt ALWAYS (0).
pub(crate) fn team_allows_push(schieber: Option<&str>, r1: u8, ziel: Option<&str>, r2: u8) -> bool {
    if r1 == 1 || r2 == 1 {
        return false;
    }
    let gleich = schieber.is_some() && schieber == ziel;
    if (r1 == 3 || r2 == 3) && gleich {
        return false;
    }
    if (r1 == 2 || r2 == 2) && !gleich {
        return false;
    }
    true
}

// Ein Eintrag der Tab-Liste -- Vanillas PlayerListEntry, soweit die Liste ihn braucht. Die
// Anfangswerte sind die von PlayerListEntry: Ueberleben, Ping 0, Hut sichtbar, und NICHT
// gelistet -- sichtbar wird ein Spieler erst mit UPDATE_LISTED (s. handle_player_info).
#[derive(Clone)]
pub(crate) struct TabEntry {
    pub(crate) name: String,
    pub(crate) game_mode: i32,
    pub(crate) listed: bool,
    pub(crate) latency: i32,
    pub(crate) display_name: Option<Vec<protocol::nbt::TextSpan>>,
    pub(crate) list_order: i32,
    pub(crate) show_hat: bool,
}

impl TabEntry {
    pub(crate) const fn new(name: String) -> Self {
        Self { name, game_mode: 0, listed: false, latency: 0, display_name: None, list_order: 0, show_hat: true }
    }
}

// Darf der Name eines Spielers ueberhaupt erscheinen? -- LivingEntityRenderer.hasLabel,
// Zweig `if (abstractTeam != null)`.
//
// Sonst stehen Namen oft doppelt da: Server mit eigenen Rang-Schildern
// stellen das Vanilla-Namensschild per Team auf NEVER und haengen ihr eigenes Hologramm
// darueber. Wer die Regel ignoriert, zeigt BEIDE -- den Namen und das Hologramm.
//
// `own_team` ist das Team des eigenen Spielers, `team` das des betrachteten.
pub(crate) fn team_allows_name(team: Option<&TeamInfo>, own_team: Option<&str>, team_name: Option<&str>) -> bool {
    let Some(team) = team else { return true };
    let gleiches_team = match (own_team, team_name) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    match team.name_tag_visibility {
        TEAM_VISIBILITY_NEVER => false,
        // "hideForOtherTeams": sichtbar NUR fuer das eigene Team. Ohne eigenes Team zaehlt
        // Vanilla das als "nicht im selben Team" (abstractTeam2 == null -> bl).
        TEAM_VISIBILITY_HIDE_FOR_OTHER_TEAMS => own_team.is_none() || gleiches_team,
        TEAM_VISIBILITY_HIDE_FOR_OWN_TEAM => own_team.is_none() || !gleiches_team,
        _ => true,
    }
}

// --- Namensschilder (s. App::world_label) ---------------------------------------------------
// Vanillas Ankerpunkt: halber Block ueber dem Kopf.
pub(crate) const NAMETAG_HEIGHT_ABOVE_HEAD: f64 = 0.5;
// Vanillas Sichtweite fuer Namen: 64 Bloecke (der Vergleich dort lautet quadriert < 4096).
pub(crate) const NAMETAG_MAX_DISTANCE: f64 = 64.0;
// Schleichend ist bei 32 Bloecken Schluss (derselbe Vergleich, quadriert: `d >= 1024`).
pub(crate) const NAMETAG_SNEAK_DISTANCE: f64 = 32.0;
// Alles mit kleinerem w liegt in oder hinter der Bildebene -- durch so ein w zu teilen ergibt
// einen Punkt irgendwo im Nirgendwo, typischerweise gespiegelt am Bildrand.
pub(crate) const NAMETAG_MIN_CLIP_W: f32 = 0.001;
// Schrittweite der Sichtlinien-Abtastung in Bloecken (s. sicht_frei).
pub(crate) const SIGHT_SAMPLE_STEP: f64 = 0.25;

// Wie oft ein fehlgeschlagener Skin-/Umhang-Download wiederholt wird, bevor er endgueltig
// aufgegeben wird (s. App::note_skin_failure). Drei Versuche ueberbruecken einen kurzen
// Netzaussetzer, ohne bei einem dauerhaft nicht erreichbaren Host in eine Endlosschleife zu
// laufen.
pub(crate) const SKIN_MAX_RETRIES: u8 = 3;

// --- Weltbeschriftungen: Text-Displays und eigene Namen (s. build_world_labels) -------------
// Vanillas Massstab fuer JEDE Schrift in der Welt (TextDisplayEntityRenderer.render und
// EntityRenderer.renderLabelIfPresent benutzen beide 0.025): ein Schriftpixel ist 0.025
// Bloecke hoch, eine Zeile also 0.225.
pub(crate) const WORLD_LABEL_WORLD_SCALE: f32 = 0.025;
// Vanillas Zeilenabstand ist Schrifthoehe + 1 Pixel (9 + 1).
pub(crate) const WORLD_LABEL_LINE_GAP: f32 = 1.0;
// Der ganzzahlige Vergroesserungsfaktor der Bitmap-Schrift, in den die berechnete Groesse
// gerundet wird. Unter 1 gibt es nichts (die Schrift kann nicht feiner als ihre Pixel), nach
// oben gedeckelt, damit ein Display direkt vor der Nase nicht den ganzen Bildschirm fuellt.
// Untergrenze BEWUSST unter 1: ein Namensschild auf 60 Bloecken ist in Vanilla winziger als
// ein einzelner Schriftpixel bei Faktor 1. Nach unten deckelt ohnehin die Sichtweite.
pub(crate) const WORLD_LABEL_MIN_SCALE: f32 = 0.35;
pub(crate) const WORLD_LABEL_MAX_SCALE: f32 = 8.0;
// TextDisplayEntity.DEFAULT_LINE_WIDTH: Umbruchbreite in Schriftpixeln.
pub(crate) const WORLD_LABEL_DEFAULT_LINE_WIDTH: f32 = 200.0;
// DisplayEntity.shouldRender: `distance < (viewRange * 64 * multiplier)^2`. Der Multiplikator
// haengt in Vanilla an der Sichtweite-Einstellung; hier steht er auf 1.
pub(crate) const DISPLAY_VIEW_RANGE_BLOCKS: f64 = 64.0;

// Eine fertig aufbereitete Weltbeschriftung (s. App::build_world_labels). Fasst die beiden
// Quellen -- text_display-Entity und Namensschild -- auf eine Form zusammen, damit das
// Zeichnen sie nicht mehr unterscheiden muss.
pub(crate) struct WorldLabel<'a> {
    // Der Wortlaut, noch ungetrennt. Als Verweis statt als Kopie: das Zerlegen in Zeilen
    // kostet Allokationen und passiert deshalb erst NACH der Entfernungs- und
    // Sichtbarkeitspruefung -- eine Beschriftung hinter einer Wand soll gar nichts kosten.
    // Cow, weil die beiden Quellen verschieden liegen: Display-Text und Custom-Name stehen
    // schon als Abschnitte in der Entity (geliehen), der Spielername aus der Tabellenliste
    // ist eine nackte Zeichenkette und wird hier erst zu einem Abschnitt gemacht (eigen).
    pub(crate) spans: std::borrow::Cow<'a, [protocol::nbt::TextSpan]>,
    // Weltpunkt, an dem die Beschriftung haengt.
    pub(crate) anchor: DVec3,
    // Zusaetzliche Skalierung aus der AffineTransformation des Displays (1.0 = Vanillas
    // Grundgroesse).
    pub(crate) world_scale: f32,
    // ARGB. Alpha 0 = kein Hintergrund.
    pub(crate) background: i32,
    pub(crate) opacity: u8,
    pub(crate) see_through: bool,
    // Rand um die Schrift. Text-Displays: ihr Schatten-Flag (Vanilla zeichnet sie sonst ohne);
    // Namensschilder: immer, s. text_shadow_style.
    pub(crate) shadow: bool,
    pub(crate) align: LabelAlign,
    // true = der Block sitzt UEBER dem Anker (Text-Display), false = mittig darauf
    // (Namensschild).
    pub(crate) above_anchor: bool,
    // Umbruchbreite in Schriftpixeln; INFINITY = nicht umbrechen.
    pub(crate) line_width: f32,
    pub(crate) max_distance: f64,
    // Nur Text-Displays: Entity-Position (augenrelativ) * Billboard-Drehung *
    // AffineTransformation -- also Vanillas Matrixstapel bis vor die Text-Skalierung. Solche
    // Beschriftungen liegen als echte Flaeche in der Welt (s. App::build_display_text);
    // Namensschilder (None) stehen immer parallel zum Bildschirm.
    pub(crate) display_matrix: Option<glam::Mat4>,
}

// JOMLs Matrix4f.rotate(Quaternionfc) als Drehmatrix -- bewusst die Formel selbst statt
// glam::Mat4::from_quat: Vanilla normalisiert die vom Server geschickten Quaternionen nicht,
// und JOML rechnet mit dem UNnormalisierten Wert (eine nicht normierte Quaternion skaliert
// dann mit). glam setzt eine normierte voraus.
pub(crate) fn joml_quat_matrix(q: [f32; 4]) -> glam::Mat4 {
    let [x, y, z, w] = q;
    let (w2, x2, y2, z2) = (w * w, x * x, y * y, z * z);
    let (dzw, dxy, dxz, dyw, dyz, dxw) = (2.0 * z * w, 2.0 * x * y, 2.0 * x * z, 2.0 * y * w, 2.0 * y * z, 2.0 * x * w);
    glam::Mat4::from_cols(
        glam::Vec4::new(w2 + x2 - z2 - y2, dxy + dzw, dxz - dyw, 0.0),
        glam::Vec4::new(dxy - dzw, y2 - z2 + w2 - x2, dyz + dxw, 0.0),
        glam::Vec4::new(dyw + dxz, dyz - dxw, z2 - y2 - x2 + w2, 0.0),
        glam::Vec4::W,
    )
}

// DisplayEntityRenderer.getBillboardRotation: rotationYXZ(-yaw, pitch, 0), je nach Modus mit
// den Winkeln des Entities oder der Kamera. 0 FIXED = feste Flaeche, 1 VERTICAL = dreht sich
// nur um die Hochachse mit, 2 HORIZONTAL = nur im Nick, 3 CENTER = wie ein Namensschild.
pub(crate) fn display_billboard_matrix(mode: u8, yaw: f32, pitch: f32, cam_yaw: f32, cam_pitch: f32) -> glam::Mat4 {
    let (y, p) = match mode {
        1 => (cam_yaw - 180.0, pitch),
        2 => (yaw, -cam_pitch),
        3 => (cam_yaw - 180.0, -cam_pitch),
        _ => (yaw, pitch),
    };
    glam::Mat4::from_rotation_y((-y).to_radians()) * glam::Mat4::from_rotation_x(p.to_radians())
}

// Die beiden Feuer-Texturen, im Wechsel uebereinander (ModelBaker.FIRE_0/FIRE_1).
pub(crate) const ENTITY_FIRE_TEXTURES: [&str; 2] = ["block/fire_0", "block/fire_1"];

// ItemEntity.uniqueOffset: in Vanilla zufaellig 0..2pi je Entity. Hier aus der Id gemischt
// (splitmix), damit derselbe Stapel in jedem Frame gleich wippt und dreht.
pub(crate) fn item_unique_offset(id: i32) -> f32 {
    let mut z = u64::from(id as u32).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32 * std::f32::consts::TAU
}

// ItemStackEntityRenderState.getRenderedAmount: wie viele Kopien ein Stapel zeigt.
pub(crate) const fn dropped_item_rendered_amount(count: i32) -> usize {
    match count {
        ..=1 => 1,
        2..=16 => 2,
        17..=32 => 3,
        33..=48 => 4,
        _ => 5,
    }
}

// Javas LCG (CheckedRandom), mit dem ItemEntityRenderer die Kopien verstreut -- derselbe
// Seed ergibt dieselben Positionen wie in Vanilla.
pub(crate) struct JavaRandom(u64);

impl JavaRandom {
    pub(crate) const MUL: u64 = 0x5_DEEC_E66D;
    pub(crate) const MASK: u64 = (1 << 48) - 1;

    pub(crate) const fn new(seed: i64) -> Self {
        Self((seed as u64 ^ Self::MUL) & Self::MASK)
    }

    pub(crate) fn next_float(&mut self) -> f32 {
        self.0 = (self.0.wrapping_mul(Self::MUL).wrapping_add(0xB)) & Self::MASK;
        (self.0 >> (48 - 24)) as f32 / (1u32 << 24) as f32
    }
}

// Die Versaetze der Kopien eines liegenden Stapels (ItemEntityRenderer.render), relativ zur
// gedrehten Grundlage. `tiefe` ist die z-Ausdehnung der Modellbox: flache Items (bis 1/16)
// stehen hintereinander wie ein Stapel Karten, dicke (Bloecke) liegen zufaellig im Umkreis
// von 0.15 verstreut.
pub(crate) fn dropped_item_copies(count: i32, seed: i32, tiefe: f32) -> Vec<world::entity_model::Affine> {
    use world::entity_model::Affine;
    let n = dropped_item_rendered_amount(count);
    let mut rng = JavaRandom::new(i64::from(seed));
    let mut streu = |faktor: f32| (rng.next_float() * 2.0 - 1.0) * 0.15 * faktor;
    let mut out = Vec::with_capacity(n);
    if tiefe > 0.0625 {
        out.push(Affine::IDENTITY);
        for _ in 1..n {
            let (x, y, z) = (streu(1.0), streu(1.0), streu(1.0));
            out.push(Affine::translation([x, y, z]));
        }
    } else {
        let schritt = tiefe * 1.5;
        let mut z = -(schritt * (n - 1) as f32 / 2.0);
        out.push(Affine::translation([0.0, 0.0, z]));
        z += schritt;
        for _ in 1..n {
            let (x, y) = (streu(0.5), streu(0.5));
            out.push(Affine::translation([x, y, z]));
            z += schritt;
        }
    }
    out
}

// Die Pfeiltextur fuer einen Entitytyp, oder None wenn es kein Pfeil ist. Der Dreizack hat
// ein eigenes Modell (ein Gegenstand, kein Kreuz) und steht deshalb NICHT hier.
pub(crate) fn arrow_texture(entity_type: i32) -> Option<&'static str> {
    match world::entity_type_name(entity_type)? {
        "arrow" => Some(world::entity_model::ARROW_TEXTURE),
        "spectral_arrow" => Some(world::entity_model::SPECTRAL_ARROW_TEXTURE),
        _ => None,
    }
}

// glam-Matrix (Spalten) -> world::entity_model::Affine (Zeilen: m[zeile][spalte]).
pub(crate) fn affine_from_mat4(m: glam::Mat4) -> world::entity_model::Affine {
    let c = |i: usize| m.col(i);
    world::entity_model::Affine {
        m: [[c(0).x, c(1).x, c(2).x], [c(0).y, c(1).y, c(2).y], [c(0).z, c(1).z, c(2).z]],
        t: [c(3).x, c(3).y, c(3).z],
    }
}

// Die Umkehrung von affine_from_mat4.
pub(crate) const fn mat4_from_affine(a: &world::entity_model::Affine) -> glam::Mat4 {
    let m = &a.m;
    glam::Mat4::from_cols(
        glam::Vec4::new(m[0][0], m[1][0], m[2][0], 0.0),
        glam::Vec4::new(m[0][1], m[1][1], m[2][1], 0.0),
        glam::Vec4::new(m[0][2], m[1][2], m[2][2], 0.0),
        glam::Vec4::new(a.t[0], a.t[1], a.t[2], 1.0),
    )
}

// DyeColor.getSignColor je Farbindex -- die Schriftfarben auf Schildern.
pub(crate) const SIGN_TEXT_COLORS: [u32; 16] = [
    0xFFFFFF, 0xFF681F, 0xFF00FF, 0x9AC0CD, 0xFFFF00, 0xBFFF00, 0xFF69B4, 0x808080, 0xD3D3D3, 0x00FFFF, 0xA020F0, 0x0000FF, 0x8B4513, 0x00FF00,
    0xFF0000, 0x000000,
];

// AbstractSignBlockEntityRenderer.getTextColor: leuchtende Schrift in voller Farbe (Schwarz
// wird dabei zu 0xF0EBCC), sonst auf 40 % abgedunkelt.
// Blockecke relativ zum Auge, in f64 gerechnet und erst dann verkuerzt (s. ColumnOrigin).
pub(crate) fn block_rel(pos: (i32, i32, i32), eye: DVec3) -> [f32; 3] {
    let v = (DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)) - eye).as_vec3();
    [v.x, v.y, v.z]
}

pub(crate) fn sign_text_color(color: u8, glowing: bool) -> u32 {
    let c = SIGN_TEXT_COLORS[(color as usize).min(15)];
    if glowing {
        return if c == 0 { 0xF0EBCC } else { c };
    }
    let d = |v: u32| ((v as f32 * 0.4) as u32).min(255);
    (d((c >> 16) & 0xff) << 16) | (d((c >> 8) & 0xff) << 8) | d(c & 0xff)
}

// Offener Schild-Editor.
pub(crate) struct SignEdit {
    pub(crate) pos: (i32, i32, i32),
    pub(crate) front: bool,
    pub(crate) lines: [String; 4],
    pub(crate) row: usize,
    // Haengeschild: schmaleres Brett, 60 statt 90 Schriftpixel je Zeile.
    pub(crate) hanging: bool,
    pub(crate) wood: usize,
    pub(crate) color: u8,
    pub(crate) opened: Instant,
    // Schreibcursor auf dem Schild in der Welt, in Textkoordinaten des Bretts (s. sign_caret).
    pub(crate) caret: std::cell::Cell<Option<SignCaret>>,
}

// Gleitet wie CaretAnim (CARET_GLIDE, Easing.Out), aber auch zwischen den Zeilen.
#[derive(Clone, Copy)]
pub(crate) struct SignCaret {
    pub(crate) from: (f32, f32),
    pub(crate) to: (f32, f32),
    pub(crate) start: Instant,
}

impl SignCaret {
    pub(crate) fn at(&self, jetzt: Instant) -> (f32, f32) {
        let k = ease_out(jetzt.duration_since(self.start).as_secs_f32() / CARET_GLIDE.as_secs_f32());
        (self.from.0 + (self.to.0 - self.from.0) * k, self.from.1 + (self.to.1 - self.from.1) * k)
    }
}

// Eine Schildseite, fertig gesetzt in Schriftpixeln ihrer Textebene (s. App::push_sign_side):
// Glyphen-Vierecke samt Atlas-UV, Farbe und Rand, die Zeilen schon mittig, je Farbabschnitt
// erst die Raender, dann die Glyphen. Liegt im VRAM (s. GpuState::upload_signs); je Bild kommt
// nur die Matrix der Seite dazu -- Glyphen nachschlagen, setzen, Zeilen messen und projizieren
// kostete in jedem Bild bei vielen Schildern ein Drittel des Hauptthreads.
pub(crate) struct SignLayout {
    // Wofuer gesetzt wurde (s. SignLayout::gilt).
    pub(crate) seite: protocol::sign::SignSide,
    pub(crate) generation: u64,
    pub(crate) font: FontKind,
    // FontAtlas::linear_raster, Err = genau dieser Faktor (dort rasten Zeichen ein).
    pub(crate) raster: Result<u8, u32>,
    pub(crate) lauf: ShadowRun,
    pub(crate) zeile_h: f32,
    pub(crate) instanzen: Vec<SignInstance>,
    // Platz der Matrix (SignInstance::seite) und Bereich im Instanzpuffer, None = noch nicht
    // hochgeladen.
    pub(crate) slot: u32,
    pub(crate) gpu: Option<std::ops::Range<u32>>,
    // Zuletzt gezeichnet in dieser Runde (s. SignLayouts::runde).
    pub(crate) runde: u64,
}

impl SignLayout {
    pub(crate) fn gilt(&self, seite: &protocol::sign::SignSide, generation: u64, font: FontKind, raster: Result<u8, u32>, lauf: ShadowRun, zeile_h: f32) -> bool {
        self.generation == generation && self.font == font && self.raster == raster && self.lauf == lauf && self.zeile_h == zeile_h && self.seite == *seite
    }
}

// Die gesetzten Schildseiten je (Position, vorn). `runde` zaehlt build_sign_texts mit; eine
// Seite, die SIGN_LAYOUT_KEEP Runden nicht gezeichnet wurde, faellt raus. `sichtbar`: die
// Seiten dieses Bildes mit ihrer Matrix in den Clip-Raum.
#[derive(Default)]
pub(crate) struct SignLayouts {
    pub(crate) seiten: rustc_hash::FxHashMap<((i32, i32, i32), bool), SignLayout>,
    pub(crate) runde: u64,
    pub(crate) sichtbar: Vec<(((i32, i32, i32), bool), glam::Mat4)>,
    // Matrix-Plaetze: vergeben bis `slots`, frei gewordene zum Wiederverwenden.
    pub(crate) slots: u32,
    pub(crate) freie_slots: Vec<u32>,
}

pub(crate) const SIGN_LAYOUT_KEEP: u64 = 600;

// Eine Beschriftung zerlegt, umgebrochen und vermessen (in Schriftpixeln). Haengt nur am
// Wortlaut, an der Zeilenbreite und an der Umgebung (s. LabelLinesEnv) -- und wird darum je
// Entity gemerkt, statt jeden Frame fuer jedes sichtbare Schild neu zu entstehen.
pub(crate) struct LabelLines {
    pub(crate) zeilen: Vec<Vec<protocol::nbt::TextSpan>>,
    // Breite je Abschnitt und je Zeile.
    pub(crate) abschnitt_breiten: Vec<Vec<f32>>,
    pub(crate) breiten: Vec<f32>,
}

// Wovon Zerlegen und Vermessen ausser Wortlaut und Zeilenbreite abhaengen: Glyphen und
// Schrift, Schreibweise, Ziffernmodus und gelernte Ziffernbreiten, Menue-Skalierung, Uebersetzung (s. i18n::tr) und der
// Ego-Name (s. App::ego_rewrite).
#[derive(Clone, PartialEq)]
pub(crate) struct LabelLinesEnv {
    pub(crate) generation: u64,
    pub(crate) font: FontKind,
    pub(crate) keep_capitals: bool,
    pub(crate) plain_numbers: bool,
    // s. StableNumbers::generation: Breiten von Texten mit Ziffern.
    pub(crate) stable_numbers: u64,
    pub(crate) ui_scale: u32,
    pub(crate) sprache: (u8, bool, bool),
    // (eigener Name, Alias, Deko an, Schatten an); None = Modul aus oder kein Name.
    pub(crate) ego: Option<(String, String, bool, bool)>,
}

pub(crate) struct LabelLinesEntry {
    pub(crate) spans: Vec<protocol::nbt::TextSpan>,
    pub(crate) line_width: u32,
    pub(crate) env_nr: u64,
    pub(crate) lines: std::rc::Rc<LabelLines>,
    pub(crate) runde: u64,
}

#[derive(Default)]
pub(crate) struct LabelLinesCache {
    pub(crate) eintraege: rustc_hash::FxHashMap<i32, LabelLinesEntry>,
    pub(crate) env: Option<LabelLinesEnv>,
    // Zaehlt jede Aenderung der Umgebung; Eintraege mit anderer Nummer gelten nicht mehr.
    pub(crate) env_nr: u64,
    pub(crate) runde: u64,
}

// So viele Frames bleibt eine Beschriftung gemerkt, die nicht mehr gezeichnet wurde.
pub(crate) const LABEL_LINES_KEEP: u64 = 600;

// Punkt der Schild-Textebene (Schriftpixel) auf den Bildschirm; None hinter der Kamera.
pub(crate) fn sign_projector(clip: glam::Mat4, screen: (f32, f32)) -> impl Fn(f32, f32) -> Option<(f32, f32)> {
    let (sw, sh) = screen;
    move |x, y| {
        let c = clip * glam::Vec4::new(x, y, 0.0, 1.0);
        (c.w > NAMETAG_MIN_CLIP_W).then(|| ((c.x / c.w * 0.5 + 0.5) * sw, (0.5 - c.y / c.w * 0.5) * sh))
    }
}

// AffineTransformation.getMatrix: translation * left_rotation * scale * right_rotation.
// Alles, was an einem Entity einen Spielerkopf zeigen kann: beide Haende, der Kopfplatz und
// das Item eines item_display.
pub(crate) fn entity_head_items(e: &world::Entity) -> [Option<world::HeldItem>; 4] {
    let display = match e.display.as_ref().map(|d| &d.content) {
        Some(world::DisplayContent::Item(item)) => *item,
        _ => None,
    };
    [e.main_hand, e.off_hand, e.armor[world::armor::ArmorSlot::Head.index()], display]
}

pub(crate) fn display_affine_matrix(d: &world::DisplayTransform) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::from(d.translation))
        * joml_quat_matrix(d.left_rotation)
        * glam::Mat4::from_scale(glam::Vec3::from(d.scale))
        * joml_quat_matrix(d.right_rotation)
}

// TextDisplayEntityRenderer.render: rotate(PI um Y) * scale(-0.025) -- zusammen
// diag(0.025, -0.025, 0.025) -- dann translate(1 - m/2, -n, 0). Danach sind die Einheiten
// Schriftpixel, x nach rechts, y nach unten, Ursprung oben links an der ersten Zeile.
pub(crate) fn display_text_frame(m: f32, n: f32) -> glam::Mat4 {
    glam::Mat4::from_scale(glam::Vec3::new(WORLD_LABEL_WORLD_SCALE, -WORLD_LABEL_WORLD_SCALE, WORLD_LABEL_WORLD_SCALE))
        * glam::Mat4::from_translation(glam::Vec3::new(1.0 - m / 2.0, -n, 0.0))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabelAlign {
    Center,
    Left,
    Right,
}

impl LabelAlign {
    // TextDisplayEntity.getAlignment: sind BEIDE Bits gesetzt, gilt CENTER.
    pub(crate) const fn from_flags(flags: u8) -> Self {
        let links = flags & protocol::packets::TEXT_DISPLAY_FLAG_ALIGN_LEFT != 0;
        let rechts = flags & protocol::packets::TEXT_DISPLAY_FLAG_ALIGN_RIGHT != 0;
        match (links, rechts) {
            (true, false) => Self::Left,
            (false, true) => Self::Right,
            _ => Self::Center,
        }
    }
}

// Abschnitte an ihren Zeilenumbruechen in Zeilen zerlegen. Ein Umbruch mitten in einem
// Abschnitt teilt ihn, die Farbe laeuft in der neuen Zeile weiter.
pub(crate) fn split_label_lines(spans: &[protocol::nbt::TextSpan]) -> Vec<Vec<protocol::nbt::TextSpan>> {
    let mut lines = split_lines_vanilla(spans);
    // Leere Zeilen am Ende wegwerfen -- ein abschliessendes "\n" soll keinen leeren Streifen
    // Hintergrund erzeugen. Leere Zeilen MITTENDRIN bleiben: sie sind gesetzter Abstand.
    while lines.last().is_some_and(std::vec::Vec::is_empty) {
        lines.pop();
    }
    lines
}

// Wie Vanillas StringSplitter: jedes "\n" beginnt eine Zeile, auch am Ende. Die Spielerliste
// braucht das -- Server schaffen mit leeren Zeilen nach einem hohen Logo-Zeichen im Kopf den
// Platz darunter; ohne sie lagen die Namen im Logo.
pub(crate) fn split_lines_vanilla(spans: &[protocol::nbt::TextSpan]) -> Vec<Vec<protocol::nbt::TextSpan>> {
    let mut lines = vec![Vec::new()];
    for span in spans {
        for (i, teil) in span.text.split('\n').enumerate() {
            if i > 0 {
                lines.push(Vec::new());
            }
            if !teil.is_empty() {
                lines.last_mut().expect("lines ist nie leer").push(protocol::nbt::TextSpan { text: teil.to_string(), color: span.color, link: span.link.clone(), action: span.action.clone(), style: span.style, shadow: span.shadow });
            }
        }
    }
    lines
}

// Zeilen auf `max_width` Schriftpixel umbrechen (TextDisplayEntity.LINE_WIDTH). Umgebrochen
// wird an Leerzeichen; ein einzelnes Wort, das allein schon zu breit ist, bleibt stehen und
// ragt heraus -- genau wie in Vanilla, wo TextRenderer.wrapLines es ebenfalls nicht
// zerschneidet.
pub(crate) fn wrap_label_lines(
    lines: &[Vec<protocol::nbt::TextSpan>],
    max_width: f32,
    measure: impl Fn(&str) -> f32,
) -> Vec<Vec<protocol::nbt::TextSpan>> {
    if !max_width.is_finite() {
        return lines.to_vec();
    }
    let mut out: Vec<Vec<protocol::nbt::TextSpan>> = Vec::with_capacity(lines.len());
    for line in lines {
        let mut aktuell: Vec<protocol::nbt::TextSpan> = Vec::new();
        let mut breite = 0.0f32;
        for span in line {
            // Die Trennstellen mitfuehren: nur das Leerzeichen an der Umbruchstelle verschwindet
            // (s. unten), alle anderen bleiben -- auch die am Zeilenanfang. Wie Vanillas
            // StringSplitter.splitLines, der nur genau das Zeichen an der Trennstelle schluckt.
            // Server polstern Zeilen der Tab-Liste mit Leerzeichen vorn UND hinten; fielen die
            // vorderen weg, stand der Text um die halbe Polsterung zu weit links (mcpvp.club,
            // Nutzer 2026-11-12).
            for (i, wort) in span.text.split(' ').enumerate() {
                let mit_luecke = if i > 0 { format!(" {wort}") } else { wort.to_string() };
                if mit_luecke.is_empty() {
                    continue;
                }
                let w = measure(&mit_luecke);
                if breite > 0.0 && breite + w > max_width {
                    out.push(std::mem::take(&mut aktuell));
                    breite = 0.0;
                    let nackt = wort.to_string();
                    breite += measure(&nackt);
                    aktuell.push(protocol::nbt::TextSpan { text: nackt, color: span.color, link: span.link.clone(), action: span.action.clone(), style: span.style, shadow: span.shadow });
                    continue;
                }
                breite += w;
                match aktuell.last_mut() {
                    Some(last) if last.color == span.color && last.link == span.link && last.action == span.action => last.text.push_str(&mit_luecke),
                    _ => aktuell.push(protocol::nbt::TextSpan { text: mit_luecke, color: span.color, link: span.link.clone(), action: span.action.clone(), style: span.style, shadow: span.shadow }),
                }
            }
        }
        out.push(aktuell);
    }
    out
}

// Eine gezeichnete Textzeile: ihr senkrechter Streifen und je Zeichen der waagerechte
// Bereich [von, bis) in Bildschirmpixeln (s. GpuState::shape_text).
#[derive(Clone, Debug, Default)]
pub(crate) struct TextRun {
    pub(crate) top: f32,
    pub(crate) bottom: f32,
    pub(crate) glyphs: Vec<(char, f32, f32)>,
    // Auswahlbereich (s. selection_boundary): eine Auswahl bleibt in dem Bereich, in dem sie
    // begonnen hat.
    pub(crate) group: u32,
}

// Was unter einer Stelle des offenen Chats liegt: ein Link und/oder die uebrigen Aktionen
// des Abschnitts (Klick, Tooltip, Shift-Einfuegen, s. nbt::SpanAction).
#[derive(Clone)]
pub(crate) struct ChatHit {
    pub(crate) link: Option<String>,
    pub(crate) action: Option<std::sync::Arc<protocol::nbt::SpanAction>>,
}

// Der UI-Pass zeichnet ALLE Flaechen vor ALLEM Text (s. render_world) -- ein Tooltip-Kasten
// ueber dem Chat laege also unter dessen Schrift. Deshalb wird der Chattext ab `from` unter
// `cover` ausgespart: von jedem Stueck, das hineinragt, bleibt nur, was links und rechts
// daneben steht. Flaechen ab `rects_from` (Unterstreichungen, Eingabefeld, Listen) bekommen
// ein Loch in Kastengroesse: bis zu vier Stuecke drumherum.
pub(crate) fn cut_ui_under(gpu: &GpuState, texts: &mut Vec<UiText>, from: usize, rects: &mut Vec<UiRect>, rects_from: usize, cover: Rect) {
    let rechts = cover.x + cover.w;
    let unten = cover.y + cover.h;
    let mut neu = Vec::with_capacity(texts.len());
    for (i, t) in texts.drain(..).enumerate() {
        let w = gpu.measure_text(&t.text, t.scale);
        let h = gpu.text_height(t.scale);
        let trifft = i >= from && !is_selection_boundary(&t) && t.y < unten && t.y + h > cover.y && t.x < rechts && t.x + w > cover.x;
        if !trifft {
            neu.push(t);
            continue;
        }
        // Zeichenweise: was vor dem Kasten endet, bleibt links stehen; was nach ihm anfaengt,
        // rechts -- an seiner alten Stelle.
        let mut links = String::new();
        let mut rest: Option<(String, f32)> = None;
        let mut x = t.x;
        for c in t.text.chars() {
            let cw = gpu.measure_text(c.encode_utf8(&mut [0u8; 4]), t.scale);
            if x + cw <= cover.x {
                links.push(c);
            } else if x >= rechts {
                rest.get_or_insert_with(|| (String::new(), x)).0.push(c);
            }
            x += cw;
        }
        if !links.is_empty() {
            neu.push(UiText { text: links, x: t.x, y: t.y, color: t.color, scale: t.scale });
        }
        if let Some((text, x)) = rest {
            neu.push(UiText { text, x: x.round(), ..t });
        }
    }
    *texts = neu;
    let mut flaechen = Vec::with_capacity(rects.len());
    for (i, r) in rects.drain(..).enumerate() {
        let Rect { x, y, w, h } = r.rect;
        if i < rects_from || y >= unten || y + h <= cover.y || x >= rechts || x + w <= cover.x {
            flaechen.push(r);
            continue;
        }
        // Oben und unten ueber die volle Breite, links und rechts nur im Band des Kastens.
        let band_oben = y.max(cover.y);
        let band_unten = (y + h).min(unten);
        let stuecke = [
            Rect { x, y, w, h: cover.y - y },
            Rect { x, y: unten, w, h: y + h - unten },
            Rect { x, y: band_oben, w: cover.x - x, h: band_unten - band_oben },
            Rect { x: rechts, y: band_oben, w: x + w - rechts, h: band_unten - band_oben },
        ];
        for rect in stuecke.into_iter().filter(|r| r.w > 0.0 && r.h > 0.0) {
            flaechen.push(UiRect { rect, color: r.color, ..UiRect::FLAT });
        }
    }
    *rects = flaechen;
}
