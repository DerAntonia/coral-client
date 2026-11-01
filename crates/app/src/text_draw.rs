// Text-Vertices, Schatten, Schriftkacheln und Schriftpalette.

use crate::*;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextVertex {
    pub(crate) position: [f32; 2],
    pub(crate) uv: [f32; 2],
    pub(crate) color: [f32; 4],
    // Nur fuer Kontur/Schatten (s. font.wgsl): Atlas-Rechteck des Glyphen und die Art samt
    // UV je Bildschirmpixel. Alles 0 = gewoehnlicher Glyph bzw. Sprite.
    pub(crate) bounds: [f32; 4],
    pub(crate) mode: [f32; 4],
}

// Wie Text einen Rand bekommt. Kontur = duenne schwarze Linie rundherum (eigene Vorgabe),
// Vanilla = Minecrafts Schatten ein Schriftpixel nach rechts unten in einem Viertel der
// Textfarbe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextShadow {
    Outline,
    Vanilla,
    // Weicher Schatten unter dem Text, wie in osu!: nach unten versetzt und verschwommen.
    Below,
    // Derselbe weiche Schatten ohne Versatz: rundherum gleich stark.
    Around,
    Off,
}

impl TextShadow {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Outline => "Kontur",
            Self::Vanilla => "Vanilla",
            Self::Below => "Unten",
            Self::Around => "Außen",
            Self::Off => "Aus",
        }
    }
}

// Die Bereiche, deren Textrand sich eigens einstellen laesst (Settings::area_shadow). Je
// Bereich 0 = wie die allgemeine Einstellung, 1 = Kontur, 2 = Vanilla, 3 = aus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShadowArea {
    Chat,
    Hotbar,
    PlayerList,
    Scoreboard,
    Inventory,
    Title,
    ActionBar,
    ShieldCountdown,
    ArrowNumber,
    // Die FPS-Anzeige (Modul unter Debug, s. push_hud_perf_texts).
    Fps,
    // Name und Timer der detaillierten Effekt-Anzeige (s. build_effect_hud).
    Effects,
    // Namensschilder ueber Entities und Text-Displays (s. build_world_labels).
    Nametags,
    // Text auf Schildern in der Welt (s. build_sign_texts).
    Signs,
    // Die Haltbarkeitszahlen des Ruestungs-HUD (s. push_armor_hud).
    ArmorHud,
}

pub(crate) const TEXT_AREA_COUNT: usize = 14;

// Schildtext je Bild hoechstens (Vertices, mit Kontur 9 Vierecke je Zeichen).
pub(crate) const MAX_SIGN_TEXT_VERTICES: usize = 65_536;

impl ShadowArea {
    // Reihenfolge = Index in Settings::area_shadow/area_font und Zeile im Schrift-Tab -- neue
    // Bereiche hinten anhaengen.
    pub(crate) const ALL: [Self; TEXT_AREA_COUNT] = [
        Self::Chat,
        Self::Hotbar,
        Self::PlayerList,
        Self::Scoreboard,
        Self::Inventory,
        Self::Title,
        Self::ActionBar,
        Self::ShieldCountdown,
        Self::ArrowNumber,
        Self::Fps,
        Self::Effects,
        Self::Nametags,
        Self::Signs,
        Self::ArmorHud,
    ];

    // Schild-Countdown und Pfeil-Zahl haben statt eines Randes ihre eigene Faerbung (s.
    // HUD_TEXT_STYLE_NAMES) -- dort gibt es nur die Schrift.
    pub(crate) const fn has_shadow(self) -> bool {
        !matches!(self, Self::ShieldCountdown | Self::ArrowNumber)
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    // Text, der sich nicht aendert: dort halten Zahlen keinen Platz frei (s.
    // push_plain_numbers_marker).
    pub(crate) fn plain_numbers(self) -> bool {
        self == Self::Chat
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Hotbar => "Hotbar",
            Self::PlayerList => "Spielerliste",
            Self::Scoreboard => "Scoreboard",
            Self::Inventory => "Inventar",
            Self::Title => "Titel",
            Self::ActionBar => "Actionbar",
            Self::ShieldCountdown => "Schild-Countdown",
            Self::ArrowNumber => "Pfeil-Zahl",
            Self::Fps => "FPS-Anzeige",
            Self::Effects => "Effekte",
            Self::Nametags => "Nametags",
            Self::Signs => "Schilder",
            Self::ArmorHud => "Rüstungs-HUD",
        }
    }

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Chat => "shadow_chat",
            Self::Hotbar => "shadow_hotbar",
            Self::PlayerList => "shadow_player_list",
            Self::Scoreboard => "shadow_scoreboard",
            Self::Inventory => "shadow_inventory",
            Self::Title => "shadow_title",
            Self::ActionBar => "shadow_actionbar",
            Self::ShieldCountdown => "shadow_shield_cd",
            Self::ArrowNumber => "shadow_arrow_number",
            Self::Fps => "shadow_fps",
            Self::Effects => "shadow_effects",
            Self::Nametags => "shadow_nametags",
            Self::Signs => "shadow_signs",
            Self::ArmorHud => "shadow_armor_hud",
        }
    }

    // Schluessel der eigenen Schrift in settings.conf (s. Settings::area_font).
    pub(crate) const fn font_key(self) -> &'static str {
        match self {
            Self::Chat => "font_chat",
            Self::Hotbar => "font_hotbar",
            Self::PlayerList => "font_player_list",
            Self::Scoreboard => "font_scoreboard",
            Self::Inventory => "font_inventory",
            Self::Title => "font_title",
            Self::ActionBar => "font_actionbar",
            Self::ShieldCountdown => "font_shield_cd",
            Self::ArrowNumber => "font_arrow_number",
            Self::Fps => "font_fps",
            Self::Effects => "font_effects",
            Self::Nametags => "font_nametags",
            Self::Signs => "font_signs",
            Self::ArmorHud => "font_armor_hud",
        }
    }
}

pub(crate) const AREA_SHADOW_CHOICES: u8 = 6;

// Dicke der Kontur in Zehntelpixeln (s. Settings::shadow_thickness): Vorgabe und Bereich.
pub(crate) const SHADOW_THICKNESS_DEFAULT: u8 = 10;
pub(crate) const SHADOW_THICKNESS_RANGE: (u8, u8) = (5, 40);

// Wert je Bereich -> Rand; None = allgemeine Einstellung.
pub(crate) const fn area_shadow_style(v: u8) -> Option<TextShadow> {
    match v {
        1 => Some(TextShadow::Outline),
        2 => Some(TextShadow::Vanilla),
        3 => Some(TextShadow::Off),
        4 => Some(TextShadow::Below),
        5 => Some(TextShadow::Around),
        _ => None,
    }
}

// Umkehrung von area_shadow_style.
pub(crate) fn area_shadow_code(stil: TextShadow) -> u8 {
    (1..AREA_SHADOW_CHOICES).find(|&v| area_shadow_style(v) == Some(stil)).unwrap_or(0)
}

// Schrift je Bereich: 0 = die allgemeine, sonst FontKind-Index + 1.
pub(crate) fn area_font_kind(v: u8) -> Option<FontKind> {
    (v > 0).then(|| FontKind::ALL.get(v as usize - 1).copied()).flatten()
}

pub(crate) fn area_font_code(font: Option<FontKind>) -> u8 {
    font.and_then(|k| FontKind::ALL.iter().position(|&f| f == k)).map_or(0, |i| i as u8 + 1)
}

// Was man im Schrift-Tab in ein Kaestchen zieht: eine Schrift oder einen Rand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FontTile {
    Font(FontKind),
    Shadow(TextShadow),
}

impl FontTile {
    // Die Auswahl rechts, in ihrer Reihenfolge: erst die Schriften, dann die Raender.
    pub(crate) fn palette() -> impl Iterator<Item = Self> {
        let raender = [TextShadow::Outline, TextShadow::Vanilla, TextShadow::Below, TextShadow::Around, TextShadow::Off];
        FontKind::ALL.into_iter().map(FontTile::Font).chain(raender.into_iter().map(FontTile::Shadow))
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Font(k) => k.label(),
            Self::Shadow(s) => s.label(),
        }
    }

    pub(crate) const fn box_w(self) -> f32 {
        match self {
            Self::Font(_) => FONT_BOX_W,
            Self::Shadow(_) => SHADOW_BOX_W,
        }
    }
}

// Ein Kaestchen im Schrift-Tab: Standard-Schrift und -Schatten (nie leer) oder die Ausnahme
// eines Bereichs (leer = wie der Standard).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FontSlot {
    DefaultFont,
    DefaultShadow,
    AreaFont(ShadowArea),
    AreaShadow(ShadowArea),
}

impl FontSlot {
    pub(crate) fn all() -> impl Iterator<Item = Self> {
        let bereiche = ShadowArea::ALL.into_iter().flat_map(|a| {
            let rand = a.has_shadow().then_some(Self::AreaShadow(a));
            std::iter::once(Self::AreaFont(a)).chain(rand)
        });
        [Self::DefaultFont, Self::DefaultShadow].into_iter().chain(bereiche)
    }

    pub(crate) const fn accepts(self, tile: FontTile) -> bool {
        match self {
            Self::DefaultFont | Self::AreaFont(_) => matches!(tile, FontTile::Font(_)),
            Self::DefaultShadow | Self::AreaShadow(_) => matches!(tile, FontTile::Shadow(_)),
        }
    }

    pub(crate) const fn is_default(self) -> bool {
        matches!(self, Self::DefaultFont | Self::DefaultShadow)
    }

    // Zeile im Schrift-Tab und linke Kante.
    pub(crate) const fn cell(self) -> (usize, f32) {
        match self {
            Self::DefaultFont => (0, FONT_CTRL_X),
            Self::DefaultShadow => (0, FONT_SHADOW_X),
            Self::AreaFont(a) => (FONT_EXCEPTIONS_ROW + a.index(), FONT_CTRL_X),
            Self::AreaShadow(a) => (FONT_EXCEPTIONS_ROW + a.index(), FONT_SHADOW_X),
        }
    }

    // In Inhaltskoordinaten; None, solange die Zeile weggescrollt ist.
    pub(crate) fn rect(self) -> Option<Rect> {
        let (row, x) = self.cell();
        let y = settings_row_y(row);
        let w = match self {
            Self::DefaultFont | Self::AreaFont(_) => FONT_BOX_W,
            Self::DefaultShadow | Self::AreaShadow(_) => SHADOW_BOX_W,
        };
        (y >= 0.0).then(|| Rect { x, y: y - (FONT_SLOT_H - SETTINGS_CTRL_H) / 2.0, w, h: FONT_SLOT_H })
    }
}

// Ein Kaestchen, das gerade gezogen wird (s. App::font_drag): was, woher (None = aus der
// Auswahl rechts), und wo der Zeiger es gepackt hat.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FontDrag {
    pub(crate) tile: FontTile,
    pub(crate) from: Option<FontSlot>,
    pub(crate) grab: (f32, f32),
}

// Die Auswahl rechts im Schrift-Tab (s. App::font_palette): linke Kante, Ueberschriften mit
// ihrer Oberkante, die Kaestchen, und wo darunter Platz fuer die Erklaerungen ist.
pub(crate) struct FontPalette {
    pub(crate) x: f32,
    pub(crate) headings: Vec<(&'static str, f32)>,
    pub(crate) tiles: Vec<(FontTile, Rect)>,
    // Strich zwischen Schriften und Schatten und das Raster der Schatten.
    pub(crate) lines: Vec<Rect>,
    pub(crate) bottom: f32,
}

// Hoehe des Namensfelds unter der Probe in den Kacheln der Auswahl.
pub(crate) fn font_palette_name_h(gpu: &GpuState) -> f32 {
    (gpu.text_height(UI_SCALE_SMALL) + 12.0).round()
}

pub(crate) const TEXT_MODE_GLYPH: f32 = 0.0;
pub(crate) const TEXT_MODE_OUTLINE: f32 = 1.0;
pub(crate) const TEXT_MODE_SHADOW: f32 = 2.0;
// Reglerstand der weichen Raender ("Unten", "Aussen"), der ihrer frueheren vollen Staerke
// entspricht -- darueber werden sie kraeftiger.
pub(crate) const SOFT_SHADOW_FULL_PERCENT: f32 = 40.0;
pub(crate) const TEXT_MODE_BELOW: f32 = 3.0;
pub(crate) const TEXT_MODE_AROUND: f32 = 4.0;
// Auf die Art addiert: farbiges Bild aus dem Pack-Glyphenblatt (s. font.wgsl).
pub(crate) const TEXT_MODE_PACK: f32 = 8.0;

#[allow(clippy::too_many_arguments)]
pub(crate) fn text_quad(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    color: [f32; 4],
    screen_px: (f32, f32),
) -> [TextVertex; 6] {
    text_quad_fx(x0, y0, x1, y1, uv_min, uv_max, color, [0.0; 4], [TEXT_MODE_GLYPH, 0.0, 0.0, 0.0], screen_px)
}

// Glyphen-Vierecke als farbiges Pack-Bild kennzeichnen (s. font.wgsl: Art + TEXT_MODE_PACK).
pub(crate) fn mark_pack_glyph(verts: &mut [TextVertex]) {
    for v in verts {
        v.mode[0] += TEXT_MODE_PACK;
    }
}

// Wie text_quad, mit Glyphen-Rechteck und Art fuer Kontur/Schatten (s. font.wgsl).
#[allow(clippy::too_many_arguments)]
pub(crate) fn text_quad_fx(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    color: [f32; 4],
    bounds: [f32; 4],
    mode: [f32; 4],
    screen_px: (f32, f32),
) -> [TextVertex; 6] {
    let (w, h) = screen_px;
    let to_ndc = |px: f32, py: f32, u: f32, v: f32| TextVertex { position: [(px / w) * 2.0 - 1.0, 1.0 - (py / h) * 2.0], uv: [u, v], color, bounds, mode };
    [
        to_ndc(x0, y0, uv_min[0], uv_min[1]),
        to_ndc(x1, y0, uv_max[0], uv_min[1]),
        to_ndc(x1, y1, uv_max[0], uv_max[1]),
        to_ndc(x0, y0, uv_min[0], uv_min[1]),
        to_ndc(x1, y1, uv_max[0], uv_max[1]),
        to_ndc(x0, y1, uv_min[0], uv_max[1]),
    ]
}
