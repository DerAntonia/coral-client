// Farben der Oberflaeche (Menues, Einstellungen, Leiste oben). Jede Farbe ist ein Platz
// (Slot) mit einem sRGB-Wert; Presets fuellen alle Plaetze auf einmal, danach laesst sich
// jeder einzeln aendern (Einstellungen -> Aussehen -> Theme). Gespeichert wird jeder Platz
// als `theme_<schluessel>=RRGGBB` (s. Settings).
//
// Der aktuelle Stand liegt global (Atomics), weil die Oberflaeche an sehr vielen Stellen
// Farben braucht und sonst jede Bau-Funktion das Theme durchreichen muesste.

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    BgApp,
    BgPanel,
    SurfaceSunken,
    Surface1,
    Surface2,
    Surface3,
    SurfacePressed,
    Border1,
    Border2,
    BorderStrong,
    Text1,
    Text2,
    Text3,
    TextDisabled,
    TextOnAccent,
    Accent,
    AccentHover,
    AccentText,
    AccentSurface,
    AccentSurfaceHover,
    Success,
    Warning,
    Danger,
    DangerSurface,
    // Hinter markiertem Text (halb deckend, s. SELECTION_ALPHA).
    Selection,
    // Zweite Akzentfarbe, passend zum Akzent jedes Themes.
    Accent2,
    Accent2Hover,
    Accent2Text,
}

pub const SLOT_COUNT: usize = 28;

impl Slot {
    pub const ALL: [Slot; SLOT_COUNT] = [
        Slot::BgApp,
        Slot::BgPanel,
        Slot::SurfaceSunken,
        Slot::Surface1,
        Slot::Surface2,
        Slot::Surface3,
        Slot::SurfacePressed,
        Slot::Border1,
        Slot::Border2,
        Slot::BorderStrong,
        Slot::Text1,
        Slot::Text2,
        Slot::Text3,
        Slot::TextDisabled,
        Slot::TextOnAccent,
        Slot::Accent,
        Slot::AccentHover,
        Slot::AccentText,
        Slot::AccentSurface,
        Slot::AccentSurfaceHover,
        Slot::Success,
        Slot::Warning,
        Slot::Danger,
        Slot::DangerSurface,
        Slot::Selection,
        Slot::Accent2,
        Slot::Accent2Hover,
        Slot::Accent2Text,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    // Schluessel in der Einstellungsdatei (hinter `theme_`).
    pub fn key(self) -> &'static str {
        match self {
            Slot::BgApp => "bg_app",
            Slot::BgPanel => "bg_panel",
            Slot::SurfaceSunken => "surface_sunken",
            Slot::Surface1 => "surface_1",
            Slot::Surface2 => "surface_2",
            Slot::Surface3 => "surface_3",
            Slot::SurfacePressed => "surface_pressed",
            Slot::Border1 => "border_1",
            Slot::Border2 => "border_2",
            Slot::BorderStrong => "border_strong",
            Slot::Text1 => "text_1",
            Slot::Text2 => "text_2",
            Slot::Text3 => "text_3",
            Slot::TextDisabled => "text_disabled",
            Slot::TextOnAccent => "text_on_accent",
            Slot::Accent => "accent",
            Slot::AccentHover => "accent_hover",
            Slot::AccentText => "accent_text",
            Slot::AccentSurface => "accent_surface",
            Slot::AccentSurfaceHover => "accent_surface_hover",
            Slot::Success => "success",
            Slot::Warning => "warning",
            Slot::Danger => "danger",
            Slot::DangerSurface => "danger_surface",
            Slot::Selection => "selection",
            Slot::Accent2 => "accent_2",
            Slot::Accent2Hover => "accent_2_hover",
            Slot::Accent2Text => "accent_2_text",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Slot::BgApp => "Hintergrund",
            Slot::BgPanel => "Panel",
            Slot::SurfaceSunken => "Eingabefeld",
            Slot::Surface1 => "Fläche 1",
            Slot::Surface2 => "Fläche 2 (Knopf)",
            Slot::Surface3 => "Fläche 3 (Hover)",
            Slot::SurfacePressed => "Gedrückt",
            Slot::Border1 => "Rahmen",
            Slot::Border2 => "Rahmen Bedienelement",
            Slot::BorderStrong => "Rahmen Hover",
            Slot::Text1 => "Text",
            Slot::Text2 => "Text sekundär",
            Slot::Text3 => "Hinweistext",
            Slot::TextDisabled => "Text deaktiviert",
            Slot::TextOnAccent => "Text auf Akzent",
            Slot::Accent => "Akzent",
            Slot::AccentHover => "Akzent Hover",
            Slot::AccentText => "Akzent-Text",
            Slot::AccentSurface => "Ausgewählt",
            Slot::AccentSurfaceHover => "Ausgewählt Hover",
            Slot::Success => "Erfolg",
            Slot::Warning => "Warnung",
            Slot::Danger => "Gefahr",
            Slot::DangerSurface => "Gefahr-Fläche",
            Slot::Selection => "Markierung",
            Slot::Accent2 => "Akzent 2",
            Slot::Accent2Hover => "Akzent 2 Hover",
            Slot::Accent2Text => "Akzent-2-Text",
        }
    }
}

// Die Farben, die man in einem eigenen Theme einstellt, in der Reihenfolge der Liste: Akzent 2
// direkt unter dem Akzent. Die uebrigen folgen aus ihnen (s. derive): die Markierung ist der
// Akzent (in allen Presets gleich), die Hover-Farben eine Stufe heller als ihre Grundfarbe.
pub const EDITABLE: [Slot; 22] = [
    Slot::BgApp,
    Slot::BgPanel,
    Slot::SurfaceSunken,
    Slot::Surface1,
    Slot::Surface2,
    Slot::SurfacePressed,
    Slot::Border1,
    Slot::Border2,
    Slot::Text1,
    Slot::Text2,
    Slot::Text3,
    Slot::TextDisabled,
    Slot::TextOnAccent,
    Slot::Accent,
    Slot::Accent2,
    Slot::AccentText,
    Slot::Accent2Text,
    Slot::AccentSurface,
    Slot::Success,
    Slot::Warning,
    Slot::Danger,
    Slot::DangerSurface,
];

// Abgeleitete Farben: (Ziel, Grundfarbe).
const DERIVED: [(Slot, Slot); 6] = [
    (Slot::Selection, Slot::Accent),
    (Slot::AccentHover, Slot::Accent),
    (Slot::Accent2Hover, Slot::Accent2),
    (Slot::AccentSurfaceHover, Slot::AccentSurface),
    (Slot::Surface3, Slot::Surface2),
    (Slot::BorderStrong, Slot::Border2),
];

// Hover-Stufe: helle Farben ein Stueck Richtung Weiss, dunkle um gut ein Viertel heller --
// so liegen auch die Presets (Akzent 4C9EFF -> 6AAFFF, Flaeche 2 1C2127 -> 242A31).
// Ganzzahlig und const, damit die Presets dieselben Werte bekommen wie ein eigenes Theme.
pub const fn lighter(srgb: u32) -> u32 {
    let (r, g, b) = ((srgb >> 16) & 0xFF, (srgb >> 8) & 0xFF, srgb & 0xFF);
    let hell = r >= 128 || g >= 128 || b >= 128;
    const fn f(c: u32, hell: bool) -> u32 {
        let v = if hell { (c * 100 + (255 - c) * 17 + 50) / 100 } else { (c * 128 + 50) / 100 };
        if v > 255 { 255 } else { v }
    }
    (f(r, hell) << 16) | (f(g, hell) << 8) | f(b, hell)
}

// Text in Akzentfarbe: gut zur Haelfte Richtung Weiss (Standard 4C9EFF -> 9CC8FF).
pub const fn text_tone(srgb: u32) -> u32 {
    const fn f(c: u32) -> u32 {
        (c * 100 + (255 - c) * 45 + 50) / 100
    }
    (f((srgb >> 16) & 0xFF) << 16) | (f((srgb >> 8) & 0xFF) << 8) | f(srgb & 0xFF)
}

// Akzent 2 Hover und Akzent-2-Text aus Akzent 2 (Presets und Dateien von vor diesen Plaetzen).
pub const fn derive_accent2_tones(c: &mut Colors) {
    c[Slot::Accent2Hover as usize] = lighter(c[Slot::Accent2 as usize]);
    c[Slot::Accent2Text as usize] = text_tone(c[Slot::Accent2 as usize]);
}

// Nach einer Aenderung an `slot`: die daraus abgeleiteten Farben nachziehen.
pub fn derive(colors: &mut Colors, slot: Slot) {
    for (ziel, basis) in DERIVED {
        if basis == slot {
            colors[ziel.index()] = if ziel == Slot::Selection { colors[basis.index()] } else { lighter(colors[basis.index()]) };
        }
    }
}

pub type Colors = [u32; SLOT_COUNT];

pub struct Preset {
    pub name: &'static str,
    pub colors: Colors,
}

// Grundlage aller Presets: das Design-System (dunkle Grautoene, ein blauer Akzent). Die
// anderen tauschen nur den Akzent oder die Flaechen.
const STANDARD: Colors = [
    0x0B0D10, 0x101317, 0x0D1013, 0x161A1F, 0x1C2127, 0x242A31, 0x13171B, 0x2A3038, 0x3A414B, 0x4C5561, 0xE8EBEF, 0xA4ACB7,
    0x7A828E, 0x474E58, 0x06111F, 0x4C9EFF, 0x6AAFFF, 0x9CC8FF, 0x16212D, 0x1C2938, 0x3DD68C, 0xF2B53A, 0xF0524F, 0x261416,
    0x4C9EFF, 0x9B7BFF, lighter(0x9B7BFF), text_tone(0x9B7BFF),
];

// Deckkraft der Markierung ueber dem Text dahinter.
pub const SELECTION_ALPHA: f32 = 0.45;

// `accent`: Text auf Akzent, Akzent, Akzent Hover, Akzent-Text, Ausgewaehlt, Ausgewaehlt Hover
// und zuletzt Akzent 2.
const fn with_accent(base: Colors, accent: [u32; 7]) -> Colors {
    let mut c = base;
    c[Slot::TextOnAccent as usize] = accent[0];
    c[Slot::Accent as usize] = accent[1];
    c[Slot::AccentHover as usize] = accent[2];
    c[Slot::AccentText as usize] = accent[3];
    c[Slot::AccentSurface as usize] = accent[4];
    c[Slot::AccentSurfaceHover as usize] = accent[5];
    // Markieren in der Akzentfarbe (s. auch Settings::parse fuer Dateien ohne den Eintrag).
    c[Slot::Selection as usize] = accent[1];
    c[Slot::Accent2 as usize] = accent[6];
    derive_accent2_tones(&mut c);
    c
}

const OLED: Colors = {
    let mut c = STANDARD;
    c[Slot::BgApp as usize] = 0x000000;
    c[Slot::BgPanel as usize] = 0x050607;
    c[Slot::SurfaceSunken as usize] = 0x030405;
    c[Slot::Surface1 as usize] = 0x0B0D0F;
    c[Slot::Surface2 as usize] = 0x111417;
    c[Slot::Surface3 as usize] = 0x1A1E22;
    c[Slot::SurfacePressed as usize] = 0x08090B;
    c[Slot::Border1 as usize] = 0x1C2026;
    c[Slot::Border2 as usize] = 0x2A3038;
    c
};

// Flaechen, Raender und Texte (BgApp bis TextDisabled, in der Reihenfolge von Slot) tauschen.
const fn with_surfaces(base: Colors, s: [u32; 14]) -> Colors {
    let mut c = base;
    let mut i = 0;
    while i < 14 {
        c[i] = s[i];
        i += 1;
    }
    c
}

// Blau getoente Flaechen.
const MITTERNACHT: Colors = with_surfaces(
    STANDARD,
    [0x070B14, 0x0B1120, 0x080D18, 0x111A2C, 0x162237, 0x1D2B44, 0x0E1626, 0x22304A, 0x2E3F5E, 0x3E5378, 0xE6ECF7, 0xA3B1C9, 0x7787A3, 0x45526A],
);
// Gruen getoente Flaechen.
const WALD: Colors = with_surfaces(
    STANDARD,
    [0x080D0A, 0x0C130F, 0x09100C, 0x121C16, 0x18241C, 0x1F2D24, 0x0F1712, 0x253429, 0x324536, 0x435A48, 0xE6EFE8, 0xA5B5A9, 0x7A8B7E, 0x4A574D],
);
// Die Farben von Nord (nordtheme.com): helleres Grau-Blau, gedaempfte Signalfarben.
const NORD: Colors = {
    let mut c = with_surfaces(
        STANDARD,
        [0x242933, 0x2E3440, 0x272C36, 0x3B4252, 0x434C5E, 0x4C566A, 0x353B49, 0x434C5E, 0x4C566A, 0x616E88, 0xECEFF4, 0xD8DEE9, 0x9AA3B5, 0x6B7489],
    );
    c[Slot::Success as usize] = 0xA3BE8C;
    c[Slot::Warning as usize] = 0xEBCB8B;
    c[Slot::Danger as usize] = 0xBF616A;
    c[Slot::DangerSurface as usize] = 0x3B2E35;
    c
};
// Braun getoente Flaechen.
const MOKKA: Colors = with_surfaces(
    STANDARD,
    [0x110D0A, 0x17120E, 0x130F0C, 0x1F1813, 0x271F18, 0x30261E, 0x1A1410, 0x3A2E24, 0x4A3B2E, 0x5E4B3B, 0xF2E8DE, 0xC4B4A4, 0x948372, 0x5A4D41],
);

pub const PRESETS: [Preset; 16] = [
    Preset { name: "Standard", colors: STANDARD },
    Preset { name: "Smaragd", colors: with_accent(STANDARD, [0x04140C, 0x3DD68C, 0x5BE3A0, 0x8FEABF, 0x12241B, 0x183026, 0x4C9EFF]) },
    Preset { name: "Violett", colors: with_accent(STANDARD, [0x0E0820, 0x9B7BFF, 0xB095FF, 0xC8B8FF, 0x1D1830, 0x251F3D, 0xFF66AA]) },
    Preset { name: "Bernstein", colors: with_accent(STANDARD, [0x1A1204, 0xF2B53A, 0xF7C55E, 0xF8D998, 0x2A2112, 0x33291A, 0xFF8A3D]) },
    Preset { name: "Rosa", colors: with_accent(STANDARD, [0x22040F, 0xFF66AA, 0xFF85BC, 0xFFB3D5, 0x2D1622, 0x381C2A, 0x9B7BFF]) },
    Preset { name: "OLED", colors: OLED },
    Preset { name: "Türkis", colors: with_accent(STANDARD, [0x041616, 0x2EC4C4, 0x4FD3D3, 0x8CE6E6, 0x112323, 0x162D2D, 0x4C9EFF]) },
    Preset { name: "Kirsche", colors: with_accent(STANDARD, [0x1F0507, 0xE5484D, 0xEC6A6E, 0xF4A3A5, 0x2A1416, 0x351A1C, 0xFF8A3D]) },
    Preset { name: "Orange", colors: with_accent(STANDARD, [0x1F0F02, 0xFF8A3D, 0xFFA061, 0xFFC59C, 0x2B1D12, 0x352417, 0xF2B53A]) },
    Preset { name: "Limette", colors: with_accent(STANDARD, [0x0F1A02, 0xA3E635, 0xB5EC5C, 0xCFF396, 0x1C2410, 0x232D14, 0x3DD68C]) },
    Preset { name: "Indigo", colors: with_accent(STANDARD, [0x0A0B26, 0x6366F1, 0x7F82F4, 0xB3B5F8, 0x17182E, 0x1E1F3A, 0xC084FC]) },
    Preset { name: "Gold", colors: with_accent(STANDARD, [0x1A1402, 0xE6C35C, 0xEDD07E, 0xF3E1AE, 0x29230F, 0x322B14, 0xFF8A3D]) },
    Preset { name: "Mitternacht", colors: with_accent(MITTERNACHT, [0x061226, 0x5B9BFF, 0x7AAFFF, 0xA9CBFF, 0x14233D, 0x1A2C4C, 0x2EC4C4]) },
    Preset { name: "Wald", colors: with_accent(WALD, [0x04140A, 0x4ADE80, 0x6EE59A, 0xA3EFBF, 0x14261A, 0x1A3121, 0xA3E635]) },
    Preset { name: "Nord", colors: with_accent(NORD, [0x1E2430, 0x88C0D0, 0x9CCDDB, 0xB8DCE6, 0x3A4A5A, 0x41535F, 0xB48EAD]) },
    Preset { name: "Mokka", colors: with_accent(MOKKA, [0x1A0F05, 0xD4A373, 0xDFB78E, 0xEBD0B3, 0x2B2017, 0x35281C, 0xE07A5F]) },
];

// Akzent 2 fuer Farben ohne eigenen Eintrag (aeltere Dateien): der des Presets mit demselben
// Akzent, sonst der des Standards.
pub fn accent2_for(accent: u32) -> u32 {
    PRESETS.iter().find(|p| p.colors[Slot::Accent as usize] == accent).map_or(STANDARD[Slot::Accent2 as usize], |p| p.colors[Slot::Accent2 as usize])
}

pub fn default_colors() -> Colors {
    STANDARD
}

// Welches Preset genau diese Farben hat (None = eigene Aenderungen).
pub fn preset_of(colors: &Colors) -> Option<usize> {
    PRESETS.iter().position(|p| &p.colors == colors)
}

static CURRENT: [AtomicU32; SLOT_COUNT] = [const { AtomicU32::new(0) }; SLOT_COUNT];
static INITIALIZED: AtomicU32 = AtomicU32::new(0);

pub fn apply(colors: &Colors) {
    for (slot, &c) in CURRENT.iter().zip(colors) {
        slot.store(c, Relaxed);
    }
    INITIALIZED.store(1, Relaxed);
}

pub fn srgb(slot: Slot) -> u32 {
    if INITIALIZED.load(Relaxed) == 0 {
        return STANDARD[slot.index()];
    }
    CURRENT[slot.index()].load(Relaxed)
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

// sRGB 0xRRGGBB -> lineares RGBA fuer das sRGB-Ziel der Oberflaeche.
pub fn rgba(srgb: u32) -> [f32; 4] {
    let ch = |shift: u32| to_linear(((srgb >> shift) & 0xFF) as f32 / 255.0);
    [ch(16), ch(8), ch(0), 1.0]
}

pub fn get(slot: Slot) -> [f32; 4] {
    rgba(srgb(slot))
}

pub fn with_alpha(mut c: [f32; 4], a: f32) -> [f32; 4] {
    c[3] = a;
    c
}

// Kurznamen fuer die haeufigsten Rollen.
pub fn text() -> [f32; 4] {
    get(Slot::Text1)
}
pub fn text_2() -> [f32; 4] {
    get(Slot::Text2)
}
pub fn text_3() -> [f32; 4] {
    get(Slot::Text3)
}
pub fn text_disabled() -> [f32; 4] {
    get(Slot::TextDisabled)
}
pub fn accent() -> [f32; 4] {
    get(Slot::Accent)
}
pub fn accent_2() -> [f32; 4] {
    get(Slot::Accent2)
}

pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    (s.len() == 6).then(|| u32::from_str_radix(s, 16).ok()).flatten()
}

// sRGB <-> HSV (h 0..360, s/v 0..1) fuer den Farbwaehler.
pub fn to_hsv(srgb: u32) -> (f32, f32, f32) {
    let r = ((srgb >> 16) & 0xFF) as f32 / 255.0;
    let g = ((srgb >> 8) & 0xFF) as f32 / 255.0;
    let b = (srgb & 0xFF) as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

pub fn from_hsv(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |f: f32| ((f + m).clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_in_reihenfolge() {
        for (i, s) in Slot::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
    }

    #[test]
    fn jede_farbe_einstellbar_oder_abgeleitet() {
        for s in Slot::ALL {
            let n = EDITABLE.iter().filter(|&&e| e == s).count() + DERIVED.iter().filter(|(z, _)| *z == s).count();
            assert_eq!(n, 1, "{s:?}");
        }
    }

    #[test]
    fn hover_wie_in_den_presets() {
        // Nah an den von Hand gesetzten Werten des Standards (je Kanal hoechstens 4 daneben).
        for (ziel, basis) in DERIVED.into_iter().filter(|(z, _)| *z != Slot::Selection) {
            let (a, b) = (lighter(STANDARD[basis.index()]), STANDARD[ziel.index()]);
            for shift in [16, 8, 0] {
                let d = ((a >> shift) & 0xFF).abs_diff((b >> shift) & 0xFF);
                assert!(d <= 4, "{ziel:?}: {a:06X} statt {b:06X}");
            }
        }
    }

    #[test]
    fn hsv_hin_und_zurueck() {
        for c in [0x4C9EFF, 0xF0524F, 0x3DD68C, 0x000000, 0xFFFFFF, 0x7A828E] {
            let (h, s, v) = to_hsv(c);
            assert_eq!(from_hsv(h, s, v), c, "{c:06X}");
        }
    }

    #[test]
    fn presets_haben_eindeutige_namen_und_werden_erkannt() {
        for (i, p) in PRESETS.iter().enumerate() {
            assert_eq!(preset_of(&p.colors), Some(i), "{}", p.name);
        }
    }
}
