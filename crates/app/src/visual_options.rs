// Darstellungs-Optionen: Schildfarben und -marken, Schadensfarbe, Himmel, Wetter,
// Farbumrechnung.

use crate::*;

// Die beiden Farben des Schild-Moduls (s. ModuleId::Shields).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShieldColor {
    // Hitbox eines Spielers, dessen Schild gerade deaktiviert ist.
    Cooldown,
    // Farbfilter ueber einem Spieler mit gehobenem Schild.
    Hold,
}

impl ShieldColor {
    // Die Regler dieser Farbe: als Hitbox-Farbe faerbt sie Linien und hat keine Deckkraft,
    // als Modell-Filter schon.
    pub(crate) const fn axes(mark: ShieldMark) -> &'static [DamageTintAxis] {
        match mark {
            ShieldMark::Hitbox => &[DamageTintAxis::Hue, DamageTintAxis::Saturation, DamageTintAxis::Value],
            ShieldMark::Model => &DamageTintAxis::ALL,
        }
    }
}

// Wie eine Schild-Markierung an einem anderen Spieler aussieht (Settings::shield_*_mark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShieldMark {
    // Farbfilter ueber dem ganzen Modell (wie die Schadens-Faerbung).
    Model,
    // Die Trefferbox in der Farbe -- auch ohne eingeschaltetes Hitbox-Modul.
    Hitbox,
}

impl ShieldMark {
    pub(crate) const fn from_index(i: u8) -> Self {
        if i == 1 { Self::Hitbox } else { Self::Model }
    }

    pub(crate) const fn index(self) -> u8 {
        match self {
            Self::Model => 0,
            Self::Hitbox => 1,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Model => "Modell",
            Self::Hitbox => "Hitbox",
        }
    }
}

// Eigener Schild-Cooldown: Zahl oder Kreis ums Fadenkreuz (Settings::shield_cd_self_mode).
pub(crate) const SHIELD_SELF_MODE_NAMES: [&str; 2] = ["Countdown", "Kreis"];
// Wie eine HUD-Zahl (Schild-Countdown, Pfeile) gefaerbt wird (Settings::*_text_style):
// 0 = wie das Fadenkreuz (Gegenfarbe zum Untergrund, s. crosshair.wgsl), 1 = weiss mit
// schwarzer Kontur.
pub(crate) const HUD_TEXT_STYLE_NAMES: [&str; 2] = ["Fadenkreuz", "Weiß+Kontur"];
// Kreis-Anzeige: Radius (Mitte des Rings) und Staerke in Bildschirmpixeln, Anzahl Segmente
// fuer den vollen Kreis.
pub(crate) const SHIELD_RING_RADIUS_PX: f32 = 11.0;
pub(crate) const SHIELD_RING_THICKNESS_PX: f32 = 2.0;
pub(crate) const SHIELD_RING_SEGMENTS: usize = 48;
// Zeichenfaktor des eigenen Countdowns (Minecraft-Schrift: 16 px hoch).
pub(crate) const SHIELD_CD_TEXT_SCALE: f32 = 2.0;

// Wie lange ein Schild nach einem Axthieb deaktiviert ist: 5 s (die Axt setzt
// disable_blocking_for_seconds = 5, das Schild skaliert mit 1). Fremde Cooldowns schickt der
// Server nicht -- sie werden am Klang item.shield.break erkannt (s. App::note_shield_break).
pub(crate) const SHIELD_DISABLE_TICKS: u32 = 100;
// Pfeil-Trail: Grenzen fuer Standzeit und Ausblenden (Zehntelsekunden, Schritt 0,5 s) und
// Breite der Spur (Bloecke).
pub(crate) const TRAIL_HOLD_MAX_TENTHS: u32 = 300;
pub(crate) const TRAIL_FADE_MAX_TENTHS: u32 = 100;
pub(crate) const TRAIL_TENTHS_STEP: u32 = 5;
pub(crate) const TRAIL_WIDTH: f32 = 0.05;
// Deckkraft der Pfeil-Anzeige: Untergrenze und Schritt (Prozent).
pub(crate) const ARROW_OPACITY_MIN: u32 = 10;
pub(crate) const ARROW_OPACITY_STEP: u32 = 10;

// Die Spur eines Geschosses: ein Punkt je Tick (Weltposition, Tick der Aufnahme). Sie lebt
// laenger als ihr Geschoss -- verschwindet der Pfeil, verblasst die Spur noch zu Ende.
#[derive(Debug, Default)]
pub(crate) struct Trail {
    pub(crate) punkte: std::collections::VecDeque<(DVec3, u64)>,
    // Gibt es das Geschoss noch? Nur dann haengt das letzte Stueck bis zu seiner Bildposition.
    pub(crate) lebt: bool,
    // Einmal gelandet, waechst die Spur nie wieder (s. App::tick_trails).
    pub(crate) gelandet: bool,
}

// So nah muss ein Spieler am Ort des Klangs stehen (die Position im Paket ist auf 1/8 Block
// gerundet und liegt bei den Fuessen des Spielers).
pub(crate) const SHIELD_BREAK_MATCH_RADIUS: f64 = 1.5;
// So lange darf ein Spieler nach dem Klang noch als blockend gelten, bevor der Eintrag
// als Fehlzuordnung gilt (Metadaten kommen erst am Tick-Ende, dazu Paket-Jitter).
pub(crate) const SHIELD_LOWER_GRACE_TICKS: u32 = 10;

// Geschaetzter Schild-Cooldown eines fremden Spielers (s. App::note_shield_break).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShieldCooldown {
    pub(crate) rest: u32,
    // Schon einmal ohne gehobenes Schild gesehen? Erst danach zaehlt erneutes Heben als
    // Beweis, dass der Cooldown vorbei (oder nie da) war.
    pub(crate) gesenkt: bool,
}

impl ShieldCooldown {
    pub(crate) const fn new() -> Self {
        Self { rest: SHIELD_DISABLE_TICKS, gesenkt: false }
    }
}

// Welche der beiden Hitbox-Varianten eine Farbeinstellung/-aktion betrifft (s.
// Settings::module_hitbox_{smoothed,raw}_{hue,saturation,value} und
// ButtonAction::HitboxColorEditor). Getrennte Farben statt einer gemeinsamen: bei
// gleichzeitig sichtbarer geglaetteter UND roher Box sollen sich beide auf den ersten Blick
// unterscheiden lassen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HitboxVariant {
    Smoothed,
    Raw,
}

// Die vier Regler der Schadens-Faerbung (s. Settings::damage_tint_*). Eigene Aufzaehlung
// fuer genau diese Farbe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DamageTintAxis {
    Hue,
    Saturation,
    Value,
    Alpha,
}

impl DamageTintAxis {
    // Reihenfolge der Zeilen im Video-Tab, ab DAMAGE_TINT_FIRST_ROW.
    pub(crate) const ALL: [Self; 4] = [Self::Hue, Self::Saturation, Self::Value, Self::Alpha];

    // Alle Achsen teilen sich die Zeile des Farbfelds.
    pub(crate) const fn row(self) -> usize {
        DAMAGE_TINT_FIRST_ROW
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Hue => "Ton",
            Self::Saturation => "Sättigung",
            Self::Value => "Helligkeit",
            Self::Alpha => "Deckkraft",
        }
    }

    pub(crate) const fn unit(self) -> &'static str {
        match self {
            Self::Hue => "\u{b0}",
            _ => "%",
        }
    }

    // Ton ist ZYKLISCH (0 und 360 sind dieselbe Farbe), die uebrigen klemmen -- genau wie
    // ueberall im Farbfenster.
    pub(crate) fn cyclic(self) -> bool {
        self == Self::Hue
    }

    pub(crate) const fn range(self) -> (u32, u32, u32) {
        match self {
            Self::Hue => (HSV_HUE_MIN, HSV_HUE_MAX, HSV_HUE_STEP),
            _ => (HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX, HSV_SAT_VAL_STEP),
        }
    }
}

// Die drei Regler der Himmelsfarbe (s. ModuleId::SkyColor). KEINE Deckkraft: der Himmel ist
// das Hinterste im Bild, es gibt nichts, wogegen er mischen koennte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SkyColorAxis {
    Hue,
    Saturation,
    Value,
}

// Uhrzeit-Regler des Himmels: ein Tag in Ticks, Schritt 15 Minuten (250 Ticks), zyklisch.
pub(crate) const SKY_TIME_DAY: u32 = 24_000;
pub(crate) const SKY_TIME_STEP: u32 = 250;

pub(crate) const fn adjust_sky_time(set: &mut Settings, dir: i32) {
    set.module_sky_time = (set.module_sky_time as i32 + dir * SKY_TIME_STEP as i32).rem_euclid(SKY_TIME_DAY as i32) as u32;
}

// Ticks des Tages als Uhrzeit "hh:mm" (Tick 0 = 6:00).
pub(crate) fn sky_time_label(ticks: u32) -> String {
    let stunde = (ticks / 1000 + 6) % 24;
    let minute = (ticks % 1000) * 60 / 1000;
    format!("{stunde:02}:{minute:02}")
}

// Modul "Wetter": was vom Himmel faellt. Server = wie vom Server geschickt (Regenstaerke, Gewitter, je
// Biom Regen/Schnee/nichts), sonst fest und ueberall gleich. Nur fuers Bild: der Dreizack
// mit Sog fragt weiter das echte Wetter (s. touching_water_or_rain).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WeatherMode {
    Server,
    Clear,
    Rain,
    Snow,
}

impl WeatherMode {
    pub(crate) const ALL: [Self; 4] = [Self::Server, Self::Clear, Self::Rain, Self::Snow];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Clear => "Klar",
            Self::Rain => "Regen",
            Self::Snow => "Schnee",
        }
    }

    // Name in der Einstellungsdatei.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Clear => "clear",
            Self::Rain => "rain",
            Self::Snow => "snow",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    pub(crate) fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

// Modul Himmel: was statt Vanillas Himmel zu sehen ist (nur im Leistungs-Pfad).
//   Einfarbig  eine feste Farbe, Tag wie Nacht; die Uhrzeit spielt keine Rolle.
//   Verlauf    der Verlaufshimmel des fruehen Shader-Pfads (render::gradient_sky) mit Sonne
//              und Mond, beide abschaltbar.
//   Skin       der Himmel aus dem Resourcepack (s. custom_sky), ohne Pack-Himmel die
//              Himmelsfarbe der Tageszeit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SkyMode {
    Solid,
    Gradient,
    Skin,
}

impl SkyMode {
    pub(crate) const ALL: [Self; 3] = [Self::Solid, Self::Gradient, Self::Skin];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Solid => "Einfarbig",
            Self::Gradient => "Verlauf",
            Self::Skin => "Skin",
        }
    }

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Gradient => "gradient",
            Self::Skin => "skin",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    pub(crate) fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

// Ton in Grad (0..360, zyklisch), Saettigung/Helligkeit in
// Prozent wie ueberall sonst im Menue (inventory_transparency, module_hitbox_opacity, ...).
// Von ALLEN Farbwaehlern des Clients benutzt (Hitbox-Modul, Schadens-Faerbung) -- deshalb
// neutral benannt und nicht nach ihrem ersten Einsatzort.
pub(crate) const HSV_HUE_MIN: u32 = 0;
pub(crate) const HSV_HUE_MAX: u32 = 360;
pub(crate) const HSV_HUE_STEP: u32 = 10;
pub(crate) const HSV_SAT_VAL_MIN: u32 = 0;
pub(crate) const HSV_SAT_VAL_MAX: u32 = 100;
pub(crate) const HSV_SAT_VAL_STEP: u32 = 5;

// Einen Regler der Schadens-Faerbung um einen Schritt verstellen. `dir` ist +1/-1. Der Ton
// laeuft zyklisch um (0 und 360 sind dieselbe Farbe), die uebrigen klemmen -- dieselbe Regel
// wie ueberall im Farbfenster.
//
// Freie Funktion statt Methode: der Aufrufer haelt bereits `&mut self.settings` und koennte
// daneben nicht noch `&self` ausleihen.
// Anzeige der Blitzdauer: Sekunden mit Komma, zwei Stellen (ein Tick = 0,05 s).
pub(crate) fn damage_tint_duration_text(ticks: u32) -> String {
    format!("{:.2} s", ticks as f32 / 20.0).replace('.', ",")
}

// Ein Schritt an einer Schild-Farbe: Ton zyklisch (0 und 360 sind dieselbe Farbe), der Rest
// klemmt.
pub(crate) fn adjust_shield_color(set: &mut Settings, farbe: ShieldColor, achse: DamageTintAxis, dir: i32) {
    let (min, max, step) = achse.range();
    let Some(wert) = set.shield_color_mut(farbe, achse) else { return };
    let neu = *wert as i32 + dir * step as i32;
    *wert = if achse.cyclic() { neu.rem_euclid(max as i32) as u32 } else { neu.clamp(min as i32, max as i32) as u32 };
}

// HSV (h in Grad 0..360, s/v in 0..1) -> RGB (0..1). Standardformel (Wikipedia "HSL and HSV"),
// kein echter Farbwaehler-Widget noetig (s. Kommentar an build_module_menu zu Drag-
// Interaktion) -- drei Stepper pro Variante reichen fuer ein Debug-Overlay.
pub(crate) fn hsv_to_rgb(hue_deg: f32, saturation: f32, value: f32) -> [f32; 3] {
    let c = value * saturation;
    let h_prime = (hue_deg / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h_prime as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    [r1 + m, g1 + m, b1 + m]
}
// "Dichte" der Hitbox-Anzeige, in Bildschirm-PIXELN -- IMMER die Kantenbreite (egal ob
// gepunktet oder durchgezogen, s. build_hitbox_vertices), im gepunkteten Modus zusaetzlich
// die Strich+Luecke-Laenge (dieselbe Zahl fuer beides: ein dichterer Wert macht die Linie
// gleichzeitig dicker UND -- falls gepunktet -- die Striche laenger, statt zwei getrennte
// Regler fuer eine einzige "wie deutlich soll die Anzeige sein"-Frage zu brauchen).
pub(crate) const HITBOX_DENSITY_MIN: u32 = 1;
pub(crate) const HITBOX_DENSITY_MAX: u32 = 10;
pub(crate) const HITBOX_DENSITY_STEP: u32 = 1;
// Linienbreite des eigenen Blockumrisses (s. ModuleId::BlockOutline), in Bildschirmpixeln.
pub(crate) const BLOCK_OUTLINE_WIDTH_MIN: u32 = 1;
pub(crate) const BLOCK_OUTLINE_WIDTH_MAX: u32 = 10;
// Wie lang ein Strich+Luecke-Paar im Vergleich zur Linienbreite ist (s.
// build_hitbox_vertices). Striche in Breite der Linie waeren optisch Punkte -- der Faktor
// macht daraus erkennbare Striche, ohne einen zweiten Regler dafuer zu brauchen.
pub(crate) const HITBOX_DASH_PER_DENSITY: f32 = 6.0;
// Wie weit um das Auge Spinnennetze eine Hitbox bekommen, in Bloecken. Der Wuerfel wird jedes
// Bild abgesucht; 8 sind gut 5000 Bloecke und reichen fuer den Nahkampf.
pub(crate) const HITBOX_COBWEB_RADIUS: i32 = 8;
