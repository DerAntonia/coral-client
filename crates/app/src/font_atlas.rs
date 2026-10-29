// Text-Rendering-Atlas: Minecrafts Bitmap-Font ODER eine freie TTF-Schrift (DM Sans, Outfit, Noto Sans,
// Lexend, Lexend Exa; Einstellungen -> Schrift).
//
// Die lokale Minecraft-Installation ist eine harte Voraussetzung des Clients (s.
// Briefing): Assets werden zur Laufzeit aus ihr gelesen und niemals mitgeliefert.
// Fehlt sie, bricht der Client beim Start ab (s. main) -- ein Ersatz-Font wuerde die
// fehlende Installation nur verschleiern.
//
// DM Sans ist davon ausgenommen: sie gehoert nicht zu Minecraft, sondern ist eine freie
// Schrift (SIL Open Font License, s. assets/fonts/DMSans-OFL.txt) und steckt deshalb direkt im
// Programm.
//
// Beide Schriften leben im SELBEN Einheitensystem, den "Font-Pixeln" der Minecraft-Schrift
// (Zelle 8 hoch, Grundlinie 7 darunter). Alles Layout im Client rechnet damit -- die
// Umschaltung aendert also nur, wie die Glyphen aussehen, nicht wo irgendetwas steht.

use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, VariableFont};

#[derive(Clone, Copy)]
pub struct GlyphInfo {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size_px: [f32; 2],
    // Versatz von der Pen-Position (oben-links) zur oberen linken Ecke des Glyphen-Quads.
    pub offset_px: [f32; 2],
    pub advance_px: f32,
}

// Ein Glyph, fertig fuer EINE Zeichengroesse: alles in Bildschirmpixeln. `snap` heisst: das
// Bild ist genau fuer diese Groesse gerastert und muss auf ganze Pixel gesetzt werden, damit
// jedes Texel auf genau einem Bildschirmpixel landet -- sonst verwischt es wieder.
#[derive(Clone, Copy)]
pub struct PlacedGlyph {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub snap: bool,
    // Farbiges Bild aus einem Resourcepack (s. PackGlyphCache): liegt im Glyphenblatt
    // (Binding 2), nicht im Deckungsatlas, und behaelt seine Farben.
    pub pack: bool,
    // Wie unscharf diese gerasterte Fassung ist: Summe c * (1 - c) ueber ihre Pixel, also
    // halb gedeckte Pixel. Ein Strich, der zwischen zwei Pixel faellt, zaehlt viel, einer
    // genau auf dem Raster wenig (s. sharpest_shift). 0 fuer nicht vorgerasterte Zeichen.
    pub unschaerfe: f32,
}

// Welche Schrift gezeichnet wird. Index = Settings::font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    Minecraft,
    DmSans,
    Outfit,
    Lexend,
    LexendExa,
    NotoSans,
}

impl FontKind {
    // Die Reihenfolge ist der gespeicherte Index (settings.conf: font=, arrow_text_font=, ...)
    // -- neue Schriften nur hinten anhaengen.
    pub const ALL: [FontKind; 6] = [FontKind::Minecraft, FontKind::DmSans, FontKind::Outfit, FontKind::Lexend, FontKind::LexendExa, FontKind::NotoSans];

    pub fn from_index(i: u8) -> Self {
        Self::ALL.get(i as usize).copied().unwrap_or(FontKind::Minecraft)
    }

    pub fn label(self) -> &'static str {
        match self {
            FontKind::Minecraft => "Minecraft",
            FontKind::DmSans => "DM Sans",
            FontKind::Outfit => "Outfit",
            FontKind::Lexend => "Lexend",
            FontKind::LexendExa => "Lexend Exa",
            FontKind::NotoSans => "Noto Sans",
        }
    }

    // Die eingebettete TTF samt Einstellungen der variablen Achsen, None fuer die
    // Minecraft-Bitmapschrift. Alle freien Schriften (OFL, Lizenzen in assets/fonts).
    fn ttf(self) -> Option<TtfSpec> {
        Some(match self {
            FontKind::Minecraft => return None,
            FontKind::DmSans => TtfSpec { bytes: DM_SANS_TTF, weight: 400.0, optical_size: Some(14.0) },
            FontKind::Outfit => TtfSpec { bytes: OUTFIT_TTF, weight: 400.0, optical_size: None },
            FontKind::Lexend => TtfSpec { bytes: LEXEND_TTF, weight: 400.0, optical_size: None },
            FontKind::LexendExa => TtfSpec { bytes: LEXEND_EXA_TTF, weight: 400.0, optical_size: None },
            FontKind::NotoSans => NOTO_SANS,
        })
    }
}

impl TtfSpec {
    // Die fette Fassung (s. FontAtlas::place_bold): bei variablen Schriften dieselbe Datei mit
    // Gewicht 700, Noto Sans ist statisch und hat eine eigene Datei.
    fn bold(self) -> TtfSpec {
        if self.bytes == NOTO_SANS.bytes {
            return NOTO_SANS_BOLD;
        }
        TtfSpec { weight: 700.0, ..self }
    }
}

#[derive(Clone, Copy)]
struct TtfSpec {
    bytes: &'static [u8],
    // Normalgewicht; DM Sans hat zusaetzlich eine Achse fuer die optische Groesse.
    weight: f32,
    optical_size: Option<f32>,
}

const DM_SANS_TTF: &[u8] = include_bytes!("../../../assets/fonts/DMSans-Variable.ttf");
const OUTFIT_TTF: &[u8] = include_bytes!("../../../assets/fonts/Outfit-Variable.ttf");
const LEXEND_TTF: &[u8] = include_bytes!("../../../assets/fonts/Lexend-Variable.ttf");
const LEXEND_EXA_TTF: &[u8] = include_bytes!("../../../assets/fonts/LexendExa-Variable.ttf");
const NOTO_SANS: TtfSpec = TtfSpec { bytes: include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf"), weight: 400.0, optical_size: None };
const NOTO_SANS_BOLD: TtfSpec = TtfSpec { bytes: include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf"), weight: 700.0, optical_size: None };
const DM_SANS_ARROWS: TtfSpec = TtfSpec { bytes: DM_SANS_TTF, weight: 400.0, optical_size: Some(14.0) };

// Zeichen, die es auch in echtem Fettdruck gibt (s. FontAtlas::place_bold): Latein bis
// Extended-A, Griechisch, Kyrillisch, Satzzeichen, Waehrungen, Pfeile, Kapitaelchen. Alles
// andere wird wie bei Vanilla doppelt gezeichnet -- fuer jedes Zeichen der Schrift sprengte
// Noto Sans die 8192 Pixel, die eine Textur hoch sein darf.
fn has_bold(ch: char) -> bool {
    matches!(ch as u32, 0x20..=0x17F | 0x370..=0x3FF | 0x400..=0x45F | 0x2010..=0x205F | 0x20A0..=0x20CF | 0x2190..=0x2195) || is_small_capital(ch)
}

// Zeichen mit Subpixel-Fassungen (s. SUBPIXEL_STEPS): Latein bis Extended-A, Satzzeichen,
// Kapitaelchen. Griechisch und Kyrillisch nicht -- mit ihnen passte Noto Sans nicht mehr in eine
// Textur.
fn has_subpixel(ch: char) -> bool {
    matches!(ch as u32, 0x20..=0x17F | 0x2010..=0x205F) || is_small_capital(ch)
}

// Kapitaelchen als eigene Unicode-Zeichen (ᴀʙᴄᴅᴇꜰɢʜɪ...), wie Server sie fuer "Small Caps"
// schreiben -- keine OpenType-Funktion, sondern Buchstaben aus dem IPA- und Lautschrift-
// Block. Kaum eine Schrift hat sie; fehlen sie, kommen sie aus Noto Sans statt aus der
// Minecraft-Pixelschrift und wirken blockig.
fn is_small_capital(ch: char) -> bool {
    matches!(
        ch as u32,
        0x01EB | 0x0262 | 0x026A | 0x0274 | 0x0276 | 0x0280 | 0x0281 | 0x028F | 0x0299 | 0x029B | 0x029C | 0x029F
            | 0x1D00..=0x1D22 | 0x1D26..=0x1D2B | 0xA730 | 0xA731 | 0xA7AF
    )
}

// Server schreiben "Small Caps" oft mit normalem s (ᴘᴠᴘ ꜱᴇʀᴠᴇʀ mit "s"): in der Minecraft-
// Schrift ist ꜱ dasselbe Bild wie s, in einer Vektorschrift faellt das kleine s heraus. In einem
// Wort aus Kapitaelchen (Grossbuchstaben duerfen dabei sein) wird es deshalb als ꜱ gezeichnet;
// nur fuer die Darstellung, der Text selbst bleibt. Manche nehmen statt s das kyrillische ѕ
// (DonutSMP: "ѕᴜʀᴠɪᴠᴀʟ"), das einer Vektorschrift ohne Kyrillisch genauso fehlt und dann blockig
// aus der Pixelschrift kaeme -- es wird ebenso zu ꜱ.
pub(crate) fn small_caps_s(text: &str) -> std::borrow::Cow<'_, str> {
    let ist_s = |c: char| c == 's' || c == '\u{0455}';
    if !text.contains(ist_s) || !text.chars().any(is_small_capital) {
        return std::borrow::Cow::Borrowed(text);
    }
    let zeichen: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 2);
    let mut i = 0;
    while i < zeichen.len() {
        if !zeichen[i].is_alphabetic() {
            out.push(zeichen[i]);
            i += 1;
            continue;
        }
        let ende = zeichen[i..].iter().position(|c| !c.is_alphabetic()).map_or(zeichen.len(), |n| i + n);
        let wort = &zeichen[i..ende];
        let kapitaelchen = wort.iter().any(|&c| is_small_capital(c)) && wort.iter().all(|&c| ist_s(c) || c.is_uppercase() || is_small_capital(c));
        out.extend(wort.iter().map(|&c| if kapitaelchen && ist_s(c) { '\u{A731}' } else { c }));
        i = ende;
    }
    std::borrow::Cow::Owned(out)
}

// Versalhoehe jeder TTF-Schrift in Font-Pixeln: die 7 Pixel der Minecraft-Grossbuchstaben.
// Die Geviertgroesse folgt daraus je Schrift (bei DM Sans mit 0.7 em genau 10) -- so fuellen
// alle Schriften dieselbe Zeile gleich aus, egal wie gross ihr Geviert gezeichnet ist.
const CAP_HEIGHT_FONT_PX: f32 = 7.0;

// Anteil des Randes links und rechts, den die Kapitaelchen aus Noto Sans behalten. Sie stammen aus
// der Lautschrift und sind fuer einzelne Zeichen gesetzt, nicht fuer Woerter: zwei gerade Striche
// (ᴜʀ) standen weit auseinander, schraege Paare (ᴠᴀ) beruehrten sich fast. Kerning hat Noto fuer
// sie keins (GPOS geprueft).
const NOTO_KAPITAELCHEN_RAND: f32 = 0.6;
// DM Sans wird fuer JEDEN Zeichenfaktor (UI_SCALE_*: 1 bis 5 Bildschirmpixel je Font-Pixel)
// einzeln gerastert und 1:1 gezeichnet. Die fruehere Fassung rasterte einmal gross und
// verkleinerte ueber eine Mip-Kette -- das war sichtbar unscharf, und die beim Verkleinern
// verlaufende Kantenglaettung ragte ueber das Glyphenrechteck hinaus und wurde dort gerade
// abgeschnitten.
// Die Fassungen liegen in VIERTEL-Schritten vor (x1.0, x1.25, ... x5.0, Schluessel = Faktor
// mal 4), damit auch prozentual skalierter Text (Chat) scharf bleibt. Die Minecraft-
// Ersatzzeichen sind Pixelkunst und gibt es nur ganzzahlig. Krumme Faktoren nehmen die
// naechstgroessere Fassung und skalieren sie.
const DM_SANS_QUARTERS: std::ops::RangeInclusive<u8> = 4..=20;
// Abstand zwischen zwei Glyphen im Atlas: lineares Sampling bei krummen Faktoren darf nicht
// in den Nachbarn greifen.
const DM_SANS_PADDING: u32 = 2;
// Subpixel-Fassungen: lateinische Buchstaben, Satzzeichen und Kapitaelchen (s. has_subpixel)
// werden zusaetzlich um
// 1/4, 2/4 und 3/4 Pixel nach rechts verschoben gerastert. Beim Zeichnen nimmt snap_x die
// Fassung, die zum Nachkomma-Anteil der Stiftposition passt -- sonst rundete jeder Buchstabe
// seinen Start auf ganze Pixel, und schmale wie "l" rueckten je nach Lage einen Pixel an den
// Nachbarn heran. Abgelegt unter (Faktor mal 4 + SUBPIXEL_KEY * Stufe, Zeichen).
const SUBPIXEL_STEPS: u8 = 4;
const SUBPIXEL_KEY: u8 = 32;
const ATLAS_MAX_WIDTH: u32 = 4096;
// Groesste Kantenlaenge einer Textur (wgpu-Standardgrenze).
const ATLAS_MAX_SIDE: u32 = 8192;

pub struct FontAtlas {
    pub bind_group: wgpu::BindGroup,
    // Geteilt: ein zweiter Atlas (andere Schrift fuer einzelne Anzeigen, s. build_in_background) muss
    // an dieselben Pipelines passen -- wgpu verlangt dafuer DASSELBE Layout-Objekt.
    pub bind_group_layout: Arc<wgpu::BindGroupLayout>,
    glyphs: HashMap<char, GlyphInfo>,
    // Alle Masse in Font-Pixeln (Vanilla: Zelle 8 hoch, Grundlinie 7 darunter).
    // Skaliert wird erst beim Zeichnen -- und nur ganzzahlig, s. UiText::scale.
    cell_height_px: f32,
    ascent_px: f32,
    // Wie weit Unterlaengen (g, j, p, q, y) unter die Grundlinie reichen, in Font-Pixeln.
    // Bei der Minecraft-Schrift endet die Zelle dort (8 - 7 = 1), DM Sans reicht tiefer --
    // wer Zeilen stapelt, braucht diesen Wert, sonst beruehrt das "g" die Zeile darunter.
    descent_px: f32,
    // Je (Faktor mal 4, Zeichen) die genau fuer diese Groesse gerasterte Fassung (nur DM Sans).
    placed: HashMap<(u8, char), PlacedGlyph>,
    // Dasselbe in Fett (s. place_bold), dazu je Zeichen der fette Vorschub in Font-Pixeln.
    placed_bold: HashMap<(u8, char), PlacedGlyph>,
    bold_advance: HashMap<char, f32>,
    // Vorschub jedes Zeichens in der Minecraft-Schrift, gleich welche Schrift gezeichnet
    // wird: wo Vanilla nach Breite umbricht (MOTD in der Serverliste), sollen die Zeilen
    // genauso enden.
    vanilla_advance: Arc<HashMap<char, f32>>,
    kind: FontKind,
    // Fuer den Neubau der Bind-Group, wenn ein anderes Pack-Glyphenblatt kommt.
    coverage_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pack: Option<Arc<PackGlyphCache>>,
}

// Der CPU-Teil eines Atlas: Deckungsbild und Glyphen.
struct AtlasPixels {
    // levels[0] = volle Aufloesung; weitere Stufen wuerden als Mip-Kette mit hochgeladen.
    levels: Vec<Vec<u8>>,
    width: u32,
    height: u32,
    glyphs: HashMap<char, GlyphInfo>,
    placed: HashMap<(u8, char), PlacedGlyph>,
    placed_bold: HashMap<(u8, char), PlacedGlyph>,
    bold_advance: HashMap<char, f32>,
    vanilla_advance: Arc<HashMap<char, f32>>,
    cell_height_px: f32,
    ascent_px: f32,
    descent_px: f32,
}

// Ein Atlas, der noch gerastert wird (s. FontAtlas::build_in_background).
pub struct PendingFont {
    kind: FontKind,
    rx: std::sync::mpsc::Receiver<AtlasPixels>,
}

impl PendingFont {
    pub fn kind(&self) -> FontKind {
        self.kind
    }

    // Fertig? Dann hochladen. Ok(None) = noch nicht, Err = der Thread ist ohne Ergebnis weg.
    pub fn try_finish(&self, device: &wgpu::Device, queue: &wgpu::Queue, bind_group_layout: Arc<wgpu::BindGroupLayout>) -> Result<Option<FontAtlas>, ()> {
        match self.rx.try_recv() {
            Ok(pixels) => Ok(Some(FontAtlas::from_pixels(device, queue, pixels, self.kind, bind_group_layout))),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(()),
        }
    }
}

impl FontAtlas {
    // Die Installation kommt von aussen: sie wird beim Start einmal geoeffnet (und dort
    // auch geprueft) und spaeter ebenso fuer Block-Texturen gebraucht.
    pub fn build(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        installation: &mut assets::MinecraftInstallation,
        kind: FontKind,
    ) -> Result<Self, assets::AssetsError> {
        let pixels = build_pixels(installation, kind)?;
        Ok(Self::from_pixels(device, queue, pixels, kind, Arc::new(create_layout(device))))
    }

    // Ein zweiter Atlas mit anderer Schrift, der mit dem Layout eines schon bestehenden in
    // dieselben Pipelines passt (s. PendingFont::try_finish). Gerastert in einem eigenen Thread:
    // eine TTF braucht dafuer 200 bis 300 ms, und der Schrift-Tab will alle auf einmal -- im
    // Hauptthread stand dabei gut eine Sekunde lang alles. Die Minecraft-Schrift (unter 1 ms,
    // braucht die Installation) kommt vorher noch hier.
    pub fn build_in_background(installation: &mut assets::MinecraftInstallation, kind: FontKind) -> Result<PendingFont, assets::AssetsError> {
        let minecraft = build_minecraft(installation)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let pixels = match kind.ttf() {
                Some(spec) => build_ttf(&minecraft, spec),
                None => minecraft,
            };
            let _ = tx.send(pixels);
        });
        Ok(PendingFont { kind, rx })
    }

    fn from_pixels(device: &wgpu::Device, queue: &wgpu::Queue, pixels: AtlasPixels, kind: FontKind, bind_group_layout: Arc<wgpu::BindGroupLayout>) -> Self {
        let (coverage_view, sampler) = upload_atlas(device, queue, &pixels, kind);
        let bind_group = make_bind_group(device, &bind_group_layout, &coverage_view, &sampler, None);
        Self {
            bind_group,
            bind_group_layout,
            glyphs: pixels.glyphs,
            cell_height_px: pixels.cell_height_px,
            ascent_px: pixels.ascent_px,
            descent_px: pixels.descent_px,
            placed: pixels.placed,
            placed_bold: pixels.placed_bold,
            bold_advance: pixels.bold_advance,
            vanilla_advance: pixels.vanilla_advance,
            kind,
            coverage_view,
            sampler,
            pack: None,
        }
    }

    // Die Schriften der geladenen Resourcepacks einsetzen (None = keine).
    pub fn set_pack(&mut self, device: &wgpu::Device, pack: Option<Arc<PackGlyphCache>>) {
        self.bind_group = make_bind_group(device, &self.bind_group_layout, &self.coverage_view, &self.sampler, pack.as_deref());
        self.pack = pack;
    }

    // Schrift zur Laufzeit wechseln. Das Layout bleibt DASSELBE Objekt -- die Text-Pipeline
    // ist fest daran gebunden, und wgpu 22 kann es nicht klonen.
    pub fn switch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        installation: &mut assets::MinecraftInstallation,
        kind: FontKind,
    ) -> Result<(), assets::AssetsError> {
        if kind == self.kind {
            return Ok(());
        }
        let pixels = build_pixels(installation, kind)?;
        let (coverage_view, sampler) = upload_atlas(device, queue, &pixels, kind);
        self.bind_group = make_bind_group(device, &self.bind_group_layout, &coverage_view, &sampler, self.pack.as_deref());
        self.coverage_view = coverage_view;
        self.sampler = sampler;
        self.glyphs = pixels.glyphs;
        self.cell_height_px = pixels.cell_height_px;
        self.ascent_px = pixels.ascent_px;
        self.descent_px = pixels.descent_px;
        self.placed = pixels.placed;
        self.placed_bold = pixels.placed_bold;
        self.bold_advance = pixels.bold_advance;
        self.vanilla_advance = pixels.vanilla_advance;
        self.kind = kind;
        Ok(())
    }

    pub fn kind(&self) -> FontKind {
        self.kind
    }

    // Zeilenhoehe in Font-Pixeln -- mit dem ganzzahligen Faktor multiplizieren.
    pub fn cell_height_px(&self) -> f32 {
        self.cell_height_px
    }

    pub fn ascent_px(&self) -> f32 {
        self.ascent_px
    }

    pub fn descent_px(&self) -> f32 {
        self.descent_px
    }

    // Wo die Oberkante des "A" unter dem Zeilenanfang steht, in Font-Pixeln. Bei der
    // Minecraft-Schrift 0, DM Sans reicht hoeher hinaus (negativ) -- daran richtet sich der
    // Schreibcursor aus, sonst steht er unter den Buchstaben.
    pub fn cap_top_px(&self) -> f32 {
        let top = *DM_SANS_QUARTERS.end();
        self.placed.get(&(top, 'A')).map_or(0.0, |p| self.ascent_px + p.offset[1] / (top as f32 / 4.0))
    }

    // Nur fuer Vorschub/Breite (Font-Pixel) -- gezeichnet wird ueber place().
    //
    // Mit Resourcepack-Schriften (s. PackGlyphCache) gilt: Zeichen aus eigenen Schriften und
    // aus dem privaten Unicode-Bereich (Server-Symbole) kommen zuerst aus dem Pack, alles
    // andere zuerst aus der eingestellten Schrift -- so bleibt DM Sans DM Sans, und ein
    // Server-Logo ist trotzdem da.
    pub fn glyph(&self, ch: char) -> Option<GlyphInfo> {
        match self.resolve(ch)? {
            Resolved::Base(c) => self.glyphs.get(&c).copied(),
            Resolved::Shifted(c, dy) => self.glyphs.get(&c).map(|g| GlyphInfo { offset_px: [g.offset_px[0], g.offset_px[1] + dy], ..*g }),
            Resolved::Pack(g) => Some(GlyphInfo {
                uv_min: [0.0; 2],
                uv_max: [0.0; 2],
                size_px: [g.cell[2] as f32 * g.scale, g.cell[3] as f32 * g.scale],
                offset_px: [0.0, -g.ascent],
                advance_px: g.advance,
            }),
        }
    }

    // Vorschub in der Minecraft-Schrift (Vanilla-Einheiten, ohne Fett). Server-Symbole aus
    // Pack-Schriften mit deren Vorschub; Zeichen, die die Minecraft-Schrift nicht kennt, so
    // breit wie Vanillas Ersatzzeichen.
    pub fn vanilla_advance(&self, ch: char) -> f32 {
        match self.resolve(ch) {
            Some(Resolved::Pack(g)) => g.advance,
            Some(Resolved::Base(c) | Resolved::Shifted(c, _)) => self.vanilla_advance.get(&c).copied().unwrap_or(6.0),
            None => 6.0,
        }
    }

    fn resolve(&self, ch: char) -> Option<Resolved> {
        let pack = self.pack.as_deref();
        if ch as u32 >= protocol::nbt::FONT_CHAR_FIRST {
            let (font, orig) = protocol::nbt::font_char_origin(ch)?;
            if let Some(g) = pack.and_then(|p| p.fonts.fonts.get(&*font)).and_then(|f| f.get(&orig)) {
                return Some(match g.text_shift {
                    Some(dy) if self.glyphs.contains_key(&orig) => Resolved::Shifted(orig, dy),
                    _ => Resolved::Pack(*g),
                });
            }
            // Schrift unbekannt oder ohne dieses Zeichen: wie die Standardschrift.
            return self.resolve(orig);
        }
        let aus_pack = || pack.and_then(|p| p.default_glyph(ch)).map(Resolved::Pack);
        let aus_basis = || self.glyphs.contains_key(&ch).then_some(Resolved::Base(ch));
        if is_private_use(ch) {
            aus_pack().or_else(aus_basis)
        } else {
            aus_basis().or_else(aus_pack)
        }
    }

    pub fn has_small_capital_s(&self) -> bool {
        self.glyph('\u{A731}').is_some()
    }

    // Text mit ꜱ statt s in Kapitaelchen-Woertern (s. small_caps_s) -- nur, wenn die Schrift ꜱ
    // auch hat, sonst fiele das Zeichen ganz weg.
    pub fn display_text<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if !self.has_small_capital_s() {
            return std::borrow::Cow::Borrowed(text);
        }
        small_caps_s(text)
    }

    // Startpunkt eines genau gerasterten Zeichens (p.snap) auf dem Pixelraster: die
    // Subpixel-Fassung zum Nachkomma-Anteil von `x` samt ganzzahligem Ursprung, ohne passende
    // Fassung wie bisher auf den naechsten ganzen Pixel gerundet.
    pub fn snap_x(&self, ch: char, scale: f32, bold: bool, p: PlacedGlyph, x: f32) -> (PlacedGlyph, f32) {
        if !p.snap {
            return (p, x);
        }
        // Die Subpixel-Fassungen liegen unter dem Zeichen selbst, nicht unter seinem Pack-Code.
        let (ch, dy) = match self.resolve(ch) {
            Some(Resolved::Shifted(c, dy)) => (c, dy),
            _ => (ch, 0.0),
        };
        let (gesetzt, x) = snap_in(if bold { &self.placed_bold } else { &self.placed }, ch, scale, shifted(p, -dy, scale), x);
        (shifted(gesetzt, dy, scale), x)
    }

    // Um wie viel ein Text verschoben gezeichnet wird, damit er am schaerfsten aussieht.
    //
    // Mit den Subpixel-Fassungen stimmen die Abstaende, aber je nach Lage auf dem Raster faellt
    // ein schmaler Strich (das l) zwischen zwei Pixel und wird links blass, rechts kraeftig --
    // derselbe Name sah so an einer Stelle sauber und an der anderen schief aus.
    // Die Lage mit der kleinsten Unschaerfe-Summe haengt nur am Text, nicht an der Stelle
    // (s. sharpest_shift_in). Hoechstens einen halben Pixel, die Abstaende im Text bleiben.
    // `positionen`: wie aus char_positions, je Zeichen von `text`.
    pub fn sharpest_shift(&self, text: &str, positionen: &[f32], x: f32, scale: f32) -> f32 {
        sharpest_shift_in(&self.placed, text, positionen, x, scale, |ch| self.place(ch, scale))
    }

    // Wo und wie gross ein Zeichen bei Faktor `scale` gezeichnet wird.
    pub fn place(&self, ch: char, scale: f32) -> Option<PlacedGlyph> {
        let (ch, dy) = match self.resolve(ch)? {
            Resolved::Base(c) => (c, 0.0),
            Resolved::Shifted(c, dy) => (c, dy),
            Resolved::Pack(g) => {
                let (uv_min, uv_max) = self.pack.as_ref()?.slot(&g)?;
                return Some(PlacedGlyph {
                    uv_min,
                    uv_max,
                    offset: [0.0, -g.ascent * scale],
                    size: [g.cell[2] as f32 * g.scale * scale, g.cell[3] as f32 * g.scale * scale],
                    snap: false,
                    pack: true,
                    unschaerfe: 0.0,
                });
            }
        };
        if let Some(p) = pick_placed(&self.placed, ch, scale) {
            return Some(shifted(p, dy, scale));
        }
        let g = self.glyphs.get(&ch)?;
        Some(PlacedGlyph {
            uv_min: g.uv_min,
            uv_max: g.uv_max,
            offset: [g.offset_px[0] * scale, (g.offset_px[1] + dy) * scale],
            size: [g.size_px[0] * scale, g.size_px[1] * scale],
            snap: false,
            pack: false,
            unschaerfe: 0.0,
        })
    }

    // Ein Zeichen in echtem Fettdruck (Gewicht 700 bzw. Noto Sans Bold) samt seinem Vorschub
    // in Font-Pixeln. None, wo es keinen gibt (Minecraft-Schrift, Pack- und Ersatzzeichen,
    // s. has_bold) -- dort zeichnet der Aufrufer wie Vanilla zweimal mit einem Pixel Versatz.
    pub fn place_bold(&self, ch: char, scale: f32) -> Option<(PlacedGlyph, f32)> {
        let (ch, dy) = match self.resolve(ch)? {
            Resolved::Base(c) => (c, 0.0),
            Resolved::Shifted(c, dy) => (c, dy),
            Resolved::Pack(_) => return None,
        };
        Some((shifted(pick_placed(&self.placed_bold, ch, scale)?, dy, scale), *self.bold_advance.get(&ch)?))
    }
}

// Kern von FontAtlas::snap_x, auf einer Karte der gerasterten Fassungen.
fn snap_in(karte: &HashMap<(u8, char), PlacedGlyph>, ch: char, scale: f32, p: PlacedGlyph, x: f32) -> (PlacedGlyph, f32) {
    let ganz = x.floor();
    let stufe = ((x - ganz) * SUBPIXEL_STEPS as f32).round() as u8;
    if stufe == SUBPIXEL_STEPS {
        return (p, ganz + 1.0);
    }
    if stufe == 0 {
        return (p, ganz);
    }
    let viertel = (scale * 4.0).round() as u8;
    match karte.get(&(viertel + SUBPIXEL_KEY * stufe, ch)) {
        Some(v) => (*v, ganz),
        None => (p, x.round()),
    }
}

// Kern von FontAtlas::sharpest_shift; `place` liefert die ungeschobene Fassung eines Zeichens.
//
// Wie der Text aussieht, haengt nur an u, dem Nachkomma-Anteil seines Anfangs: jedes Zeichen
// nimmt die Fassung zu u + seiner Lage im Text (s. snap_in). Ueber u in [0, 1) wechselt
// Zeichen i seine Fassung genau viermal; dazwischen bleibt das Bild gleich. Abgelaufen werden
// alle diese Stuecke, je Wechsel aendert sich nur der Beitrag eines Zeichens. Genommen wird
// das schaerfste Stueck, bei Gleichstand das erste -- beides haengt nicht an `x`, derselbe
// Text sieht also an jeder Stelle gleich aus. Verschoben wird um hoechstens einen halben Pixel.
fn sharpest_shift_in(placed: &HashMap<(u8, char), PlacedGlyph>, text: &str, positionen: &[f32], x: f32, scale: f32, place: impl Fn(char) -> Option<PlacedGlyph>) -> f32 {
    let viertel = (scale * 4.0).round() as u8;
    let steps = SUBPIXEL_STEPS as usize;
    // Je Zeichen: Nachkomma-Anteil seiner Lage und die Unschaerfe seiner vier Fassungen.
    let mut zeichen: Vec<(f32, [f32; SUBPIXEL_STEPS as usize])> = Vec::new();
    for (ch, &rel) in text.chars().zip(positionen) {
        let Some(p) = place(ch).filter(|p| p.snap) else { continue };
        let mut u = [p.unschaerfe; SUBPIXEL_STEPS as usize];
        for (stufe, wert) in u.iter_mut().enumerate().skip(1) {
            if let Some(v) = placed.get(&(viertel + SUBPIXEL_KEY * stufe as u8, ch)) {
                *wert = v.unschaerfe;
            }
        }
        zeichen.push((rel.rem_euclid(1.0), u));
    }
    if zeichen.is_empty() {
        return 0.0;
    }
    let fassung = |anteil: f32| ((anteil * SUBPIXEL_STEPS as f32).round() as usize) % steps;
    // Wechsel: bei u + f = (k + 1/2) / 4 springt das Zeichen eine Fassung weiter.
    let mut wechsel: Vec<(f32, usize)> = zeichen
        .iter()
        .enumerate()
        .flat_map(|(i, &(f, _))| (0..steps).map(move |k| (((k as f32 + 0.5) / SUBPIXEL_STEPS as f32 - f).rem_euclid(1.0), i)))
        .collect();
    wechsel.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Das erste Stueck [0, erster Wechsel) direkt ausrechnen, dann weiterzaehlen.
    let mitte0 = wechsel[0].0 / 2.0;
    let mut stufen: Vec<usize> = zeichen.iter().map(|&(f, _)| fassung((mitte0 + f).rem_euclid(1.0))).collect();
    let mut summe: f32 = zeichen.iter().zip(&stufen).map(|(z, &s)| z.1[s]).sum();
    let (mut beste, mut beste_summe) = (mitte0, summe);
    for (n, &(ab, i)) in wechsel.iter().enumerate() {
        summe -= zeichen[i].1[stufen[i]];
        stufen[i] = (stufen[i] + 1) % steps;
        summe += zeichen[i].1[stufen[i]];
        let bis = wechsel.get(n + 1).map_or(1.0 + wechsel[0].0, |w| w.0);
        if bis - ab > 1e-4 && summe < beste_summe - 1e-3 {
            (beste, beste_summe) = ((ab + bis) / 2.0, summe);
        }
    }
    // Von der jetzigen Lage zur besten, auf kuerzestem Weg.
    (beste.rem_euclid(1.0) - x.rem_euclid(1.0) + 0.5).rem_euclid(1.0) - 0.5
}

// Die fuer Faktor `scale` gerasterte Fassung, sonst die naechstgroessere (verkleinern ist
// weicher als vergroessern) skaliert; ueber der groessten wird die groesste vergroessert.
// Ein gesetzter Glyph `dy` Font-Pixel tiefer (s. Resolved::Shifted).
fn shifted(p: PlacedGlyph, dy: f32, scale: f32) -> PlacedGlyph {
    PlacedGlyph { offset: [p.offset[0], p.offset[1] + dy * scale], ..p }
}

fn pick_placed(placed: &HashMap<(u8, char), PlacedGlyph>, ch: char, scale: f32) -> Option<PlacedGlyph> {
    if placed.is_empty() {
        return None;
    }
    let quarters = scale * 4.0;
    let exact = quarters.round();
    if (quarters - exact).abs() < 0.05 {
        if let Some(p) = placed.get(&(exact as u8, ch)) {
            return Some(*p);
        }
    }
    let (lo, hi) = (*DM_SANS_QUARTERS.start(), *DM_SANS_QUARTERS.end());
    let first = (quarters.ceil().max(lo as f32) as u8).min(hi);
    let (q, p) = (first..=hi).chain((lo..first).rev()).find_map(|q| placed.get(&(q, ch)).map(|p| (q, p)))?;
    let k = quarters / q as f32;
    Some(PlacedGlyph { offset: [p.offset[0] * k, p.offset[1] * k], size: [p.size[0] * k, p.size[1] * k], snap: false, ..*p })
}

enum Resolved {
    Base(char),
    Pack(assets::PackGlyph),
    // Gewoehnlicher Text aus einer verschobenen Kopie der Standardschrift (s.
    // PackGlyph::text_shift): in der eingestellten Schrift, so viele Font-Pixel tiefer.
    Shifted(char, f32),
}

// Privater Unicode-Bereich der BMP -- dort legen Server ihre Symbole ab.
fn is_private_use(ch: char) -> bool {
    ('\u{E000}'..='\u{F8FF}').contains(&ch)
}

// --- Schriften aus Resourcepacks --------------------------------------------------------------
//
// Ein Server-Pack bringt leicht zehntausende Glyphen mit (Heldeninsel: 22000 Zellen, 40
// Megapixel) -- alle als Textur waeren ueber 150 MB. Wie Vanilla kommen sie deshalb erst beim
// ersten Zeichnen auf die GPU, in ein festes Glyphenblatt. Messen braucht nur die Vorschuebe,
// die schon beim Laden feststehen.
//
// Laeuft das Blatt voll, wird es geleert und neu befuellt; im schlimmsten Fall zeigt ein
// einziges Bild dann ein falsches Zeichen.

const SHEET_SIZE: u32 = 2048;

pub struct PackGlyphCache {
    fonts: assets::PackFonts,
    sheet: wgpu::Texture,
    view: wgpu::TextureView,
    queue: Arc<wgpu::Queue>,
    slots: std::sync::Mutex<Sheet>,
}

#[derive(Default)]
struct Sheet {
    // (Seite, x, y) der Zelle -> Lage im Blatt.
    slots: HashMap<(u32, u32, u32), [u32; 2]>,
    x: u32,
    y: u32,
    row_h: u32,
}

// Kennung der Ersatzschrift in PackGlyph::page (echte Seiten zaehlen von 0 aufwaerts).
const UNIHEX_PAGE: u32 = u32::MAX;

impl PackGlyphCache {
    pub fn new(device: &wgpu::Device, queue: Arc<wgpu::Queue>, fonts: assets::PackFonts) -> Self {
        let sheet = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pack glyph sheet"),
            size: wgpu::Extent3d { width: SHEET_SIZE, height: SHEET_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = sheet.create_view(&wgpu::TextureViewDescriptor::default());
        Self { fonts, sheet, view, queue, slots: std::sync::Mutex::new(Sheet::default()) }
    }

    // Ein Zeichen der Standardschrift aus den Packs, sonst aus Vanillas Ersatzschrift (unifont,
    // s. assets::Unihex) -- wie Vanilla, das unifont hinter alle Bitmap-Seiten haengt.
    fn default_glyph(&self, ch: char) -> Option<assets::PackGlyph> {
        if let Some(g) = self.fonts.fonts.get("minecraft:default").and_then(|f| f.get(&ch)) {
            return Some(*g);
        }
        let u = self.fonts.unihex.as_ref()?.glyphs.get(&ch)?;
        // Seite UNIHEX_PAGE, Zelle (Codepunkt, 0): s. slot. 16 Zeilen auf 8 Font-Pixel, die
        // Oberkante 7 ueber der Grundlinie wie bei der Bitmap-Schrift.
        Some(assets::PackGlyph { page: Some(UNIHEX_PAGE), cell: [ch as u32, 0, u.width(), 16], scale: 0.5, ascent: 7.0, advance: u.advance(), text_shift: None })
    }

    // UV-Rechteck der Zelle im Blatt; laedt sie beim ersten Mal hoch.
    fn slot(&self, g: &assets::PackGlyph) -> Option<([f32; 2], [f32; 2])> {
        let page = g.page?;
        let [cx, cy, w, h] = g.cell;
        if w == 0 || h == 0 || w + 2 > SHEET_SIZE || h + 2 > SHEET_SIZE {
            return None;
        }
        let mut sheet = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let pos = match sheet.slots.get(&(page, cx, cy)) {
            Some(&p) => p,
            None => {
                if sheet.x + w + 1 > SHEET_SIZE {
                    sheet.x = 0;
                    sheet.y += sheet.row_h + 1;
                    sheet.row_h = 0;
                }
                if sheet.y + h + 1 > SHEET_SIZE {
                    eprintln!("[font] Pack-Glyphenblatt voll -- wird neu befuellt");
                    *sheet = Sheet::default();
                }
                let p = [sheet.x, sheet.y];
                sheet.x += w + 1;
                sheet.row_h = sheet.row_h.max(h);
                sheet.slots.insert((page, cx, cy), p);
                let mut pixel = Vec::with_capacity((w * h * 4) as usize);
                if page == UNIHEX_PAGE {
                    // Weisse Glyphe, Form im Alpha -- die Textfarbe kommt im Shader dazu.
                    let g = char::from_u32(cx).and_then(|c| self.fonts.unihex.as_ref()?.glyphs.get(&c).copied())?;
                    for a in g.pixels() {
                        pixel.extend_from_slice(&[255, 255, 255, a]);
                    }
                } else {
                    let tex = &self.fonts.pages[page as usize];
                    for row in 0..h {
                        let start = (((cy + row) * tex.width + cx) * 4) as usize;
                        pixel.extend_from_slice(&tex.rgba[start..start + (w * 4) as usize]);
                    }
                }
                self.queue.write_texture(
                    wgpu::ImageCopyTexture { texture: &self.sheet, mip_level: 0, origin: wgpu::Origin3d { x: p[0], y: p[1], z: 0 }, aspect: wgpu::TextureAspect::All },
                    &pixel,
                    wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
                p
            }
        };
        let s = SHEET_SIZE as f32;
        Some(([pos[0] as f32 / s, pos[1] as f32 / s], [(pos[0] + w) as f32 / s, (pos[1] + h) as f32 / s]))
    }
}

fn build_pixels(installation: &mut assets::MinecraftInstallation, kind: FontKind) -> Result<AtlasPixels, assets::AssetsError> {
    let minecraft = build_minecraft(installation)?;
    Ok(match kind {
        FontKind::Minecraft => minecraft,
        k => build_ttf(&minecraft, k.ttf().expect("jede Schrift außer Minecraft ist eine TTF")),
    })
}

fn build_minecraft(installation: &mut assets::MinecraftInstallation) -> Result<AtlasPixels, assets::AssetsError> {
    let font = installation.load_default_font()?;

    // Alle Seiten UNTEREINANDER in eine einzige Textur: der Zeichenpfad bleibt damit ein
    // Draw-Call mit einer Bind-Group, und die Seite steckt nur noch in den UV-Werten.
    // Breite = die breiteste Seite, Hoehe = Summe (bei Vanilla 144 x 1564 Pixel, als
    // Ein-Kanal-Deckung also gut 200 KB).
    let tex_w = font.pages.iter().map(|p| p.texture.width).max().unwrap_or(1);
    let tex_h: u32 = font.pages.iter().map(|p| p.texture.height).sum();
    let mut coverage = vec![0u8; (tex_w * tex_h) as usize];

    let base = font.base_page();
    let (base_cell_h, base_ascent) = (base.cell_size().1 as f32 * base.scale(), base.ascent as f32);

    let mut glyphs = HashMap::default();
    let mut page_y = 0u32;
    for page in &font.pages {
        // Der Shader liest einen Kanal als Deckung. Die Font-Textur ist weiss, die Form
        // steckt im Alphakanal -- also diesen nach R8 uebernehmen.
        for y in 0..page.texture.height {
            for x in 0..page.texture.width {
                let alpha = page.texture.rgba[((y * page.texture.width + x) * 4 + 3) as usize];
                coverage[((page_y + y) * tex_w + x) as usize] = alpha;
            }
        }

        let (cell_w, cell_h) = page.cell_size();
        let scale = page.scale();
        for (row_index, row) in page.chars.iter().enumerate() {
            for (col_index, &ch) in row.iter().enumerate() {
                if ch == '\0' {
                    continue; // unbenutzte Rasterzelle
                }
                let (col, row_i) = (col_index as u32, row_index as u32);

                // Vorschub nach Vanillas Formel (s. FontPage::advance). Das Leerzeichen
                // hat eine leere Zelle und bekaeme so die Breite 1 -- sein echter Wert
                // steht im space-Provider.
                let advance = if ch == ' ' { font.space_advance } else { page.advance(col, row_i) };

                glyphs.insert(
                    ch,
                    GlyphInfo {
                        uv_min: [(col * cell_w) as f32 / tex_w as f32, (page_y + row_i * cell_h) as f32 / tex_h as f32],
                        uv_max: [((col + 1) * cell_w) as f32 / tex_w as f32, (page_y + (row_i + 1) * cell_h) as f32 / tex_h as f32],
                        // Ganze Zelle zeichnen, auf die Zielhoehe der Seite skaliert; der
                        // ungenutzte Teil ist transparent.
                        size_px: [cell_w as f32 * scale, cell_h as f32 * scale],
                        // Die Zelle beginnt `ascent` Pixel ueber der Grundlinie -- PRO
                        // SEITE verschieden: accented.png sitzt mit ascent 10 hoeher als
                        // ascii.png mit 7, genau dadurch stehen die Puenktchen ueber dem a.
                        offset_px: [0.0, -(page.ascent as f32)],
                        advance_px: advance,
                    },
                );
            }
        }
        page_y += page.texture.height;
    }
    // Zeichen ohne Bild aus dem space-Provider (U+200C): nur Vorschub. Sonst fiele es auf das
    // Unifont-Kaestchen zurueck.
    for &(ch, advance) in &font.spaces {
        glyphs.entry(ch).or_insert(GlyphInfo { uv_min: [0.0; 2], uv_max: [0.0; 2], size_px: [0.0; 2], offset_px: [0.0; 2], advance_px: advance });
    }

    Ok(AtlasPixels {
        levels: vec![coverage],
        width: tex_w,
        height: tex_h,
        vanilla_advance: Arc::new(glyphs.iter().map(|(&c, g)| (c, g.advance_px)).collect()),
        glyphs,
        placed: HashMap::default(),
        placed_bold: HashMap::default(),
        bold_advance: HashMap::default(),
        cell_height_px: base_cell_h,
        ascent_px: base_ascent,
        descent_px: (base_cell_h - base_ascent).max(0.0),
    })
}

// Ein fertig gerastertes Glyphenbild fuer einen Faktor, noch ohne Platz im Atlas.
struct Bitmap {
    scale: u8,
    ch: char,
    bold: bool,
    w: u32,
    h: u32,
    coverage: Vec<u8>,
    // Obere linke Ecke relativ zum Grundlinien-Ursprung, in ganzen Bildschirmpixeln.
    offset: [f32; 2],
}

// Eine TTF-Schrift, dazu jedes Zeichen der Minecraft-Schrift, das sie nicht hat (Kyrillisch,
// Symbole, die Sonderzeichen vieler Server-Scoreboards) -- sonst fehlten sie einfach. Die
// Ersatzzeichen werden je Faktor pixelgenau vergroessert und sehen aus wie in der
// Minecraft-Schrift.
// Rastert die Zeichen einer TTF, die `nehmen` zulaesst und die noch nicht in `glyphs` stehen,
// in allen Viertel-Fassungen.
// `bold`: die Bilder gehoeren in die fette Fassung (s. FontAtlas::place_bold).
fn raster_ttf(spec: TtfSpec, bold: bool, nehmen: &dyn Fn(char) -> bool, glyphs: &mut HashMap<char, GlyphInfo>, bitmaps: &mut Vec<Bitmap>) {
    let mut font = FontRef::try_from_slice(spec.bytes).expect("eingebettete TTF ist gültig");
    font.set_variation(b"wght", spec.weight);
    if let Some(opsz) = spec.optical_size {
        font.set_variation(b"opsz", opsz);
    }
    let units_per_em = font.units_per_em().unwrap_or(1000.0);
    // Versalhoehe aus dem Umriss des "H" (in Font-Einheiten) -- daraus die Geviertgroesse, die
    // sie auf CAP_HEIGHT_FONT_PX bringt. Ohne "H" die ueblichen 0.7 em.
    let versal = font.outline(font.glyph_id('H')).map(|o| o.bounds.height()).filter(|h| *h > 0.0).unwrap_or(0.7 * units_per_em);
    let em_font_px = CAP_HEIGHT_FONT_PX * units_per_em / versal;
    // PxScale meint die Hoehe Oberlaenge bis Unterlaenge, nicht das Geviert.
    let px_scale = |em_px: f32| PxScale::from(em_px * font.height_unscaled() / units_per_em);

    // Unendlich (∞) ist in den Schriften kaum hoeher als ein kleines o. Es soll so gross
    // sein wie eine liegende 8: so breit wie die 8 hoch und so hoch wie sie breit, auf ihrer
    // Mitte. Faktoren je Achse (Font-Einheiten; die Umrisse zaehlen y nach oben).
    let umriss = |c: char| font.outline(font.glyph_id(c)).map(|o| o.bounds);
    let (unendlich, mitte_acht) = match (umriss('8'), umriss('\u{221E}')) {
        (Some(a), Some(u)) if u.width() > 0.0 && u.height().abs() > 0.0 => {
            ((a.height().abs() / u.width(), a.width() / u.height().abs()), (a.min.y + a.max.y) / 2.0)
        }
        _ => ((1.0, 1.0), 0.0),
    };
    let groesse = |ch: char| if ch == '\u{221E}' { unendlich } else { (1.0, 1.0) };
    // Wie viel Rand links und rechts ein Zeichen abgibt (Font-Einheiten), s. NOTO_KAPITAELCHEN_RAND.
    let noto = spec.bytes == NOTO_SANS.bytes || spec.bytes == NOTO_SANS_BOLD.bytes;
    let kuerzung = |id: ab_glyph::GlyphId, ch: char| -> (f32, f32) {
        let Some(b) = font.outline(id).map(|o| o.bounds).filter(|_| noto && is_small_capital(ch)) else { return (0.0, 0.0) };
        let weg = 1.0 - NOTO_KAPITAELCHEN_RAND;
        (weg * b.min.x.max(0.0), weg * (font.h_advance_unscaled(id) - b.max.x).max(0.0))
    };

    // Vorschuebe in Font-Pixeln (fuer Layout und Breitenmessung), unabhaengig vom Faktor.
    let unit = font.as_scaled(px_scale(em_font_px));
    let mut ids = Vec::new();
    for (id, ch) in font.codepoint_ids() {
        if ch.is_control() || glyphs.contains_key(&ch) || !nehmen(ch) {
            continue;
        }
        let (links, rechts) = kuerzung(id, ch);
        let advance_px = unit.h_advance(id) * groesse(ch).0 - (links + rechts) * unit.h_scale_factor();
        glyphs.insert(ch, GlyphInfo { uv_min: [0.0; 2], uv_max: [0.0; 2], size_px: [0.0; 2], offset_px: [0.0; 2], advance_px });
        ids.push((id, ch, links));
    }

    // KEINE Tabellenziffern: DM Sans hat kein "tnum"-Feature (GSUB geprueft), und ein Nachbau
    // mit dem Vorschub der breitesten Ziffer riss die schmale "1" weit auseinander. Gegen zitternde Zahlen hilft
    // stattdessen das Einrasten der Breite dort, wo es zaehlt (s. App::latched_width).

    for scale in DM_SANS_QUARTERS {
        let f = scale as f32 / 4.0;
        let ps = px_scale(em_font_px * f);
        // Mitte der 8 in Pixeln (y nach unten) -- dort sitzt das ∞.
        let mitte = -mitte_acht * em_font_px * f / units_per_em;
        let links_px = font.as_scaled(ps).h_scale_factor();
        for (&(id, ch, links), stufe) in ids.iter().flat_map(|e| (0..SUBPIXEL_STEPS).filter(move |&s| s == 0 || has_subpixel(e.1)).map(move |s| (e, s))) {
            let k = groesse(ch);
            let x = stufe as f32 / SUBPIXEL_STEPS as f32 - links * links_px;
            let glyph = id.with_scale_and_position(PxScale { x: ps.x * k.0, y: ps.y * k.1 }, ab_glyph::point(x, 0.0));
            let Some(outline) = font.outline_glyph(glyph) else { continue };
            let mut b = outline.px_bounds();
            if k != (1.0, 1.0) {
                let h = b.height();
                b.min.y = (mitte - h / 2.0).round();
                b.max.y = b.min.y + h;
            }
            let (w, h) = (b.width() as u32, b.height() as u32);
            let mut coverage = vec![0u8; (w * h) as usize];
            outline.draw(|x, y, c| {
                if x < w && y < h {
                    coverage[(y * w + x) as usize] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            });
            // px_bounds ist ganzzahlig und relativ zum Grundlinien-Ursprung (y nach unten) --
            // genau die Konvention von PlacedGlyph::offset.
            bitmaps.push(Bitmap { scale: scale + SUBPIXEL_KEY * stufe, ch, bold, w, h, coverage, offset: [b.min.x, b.min.y] });
        }
    }
}

fn build_ttf(minecraft: &AtlasPixels, spec: TtfSpec) -> AtlasPixels {
    let mut glyphs: HashMap<char, GlyphInfo> = HashMap::default();
    let mut bitmaps: Vec<Bitmap> = Vec::new();
    // Erst die Schrift selbst, dann die Kapitaelchen, die ihr fehlen, aus Noto Sans (s.
    // is_small_capital) -- je auf dieselbe Versalhoehe gebracht.
    let pfeil = |c: char| ('\u{2190}'..='\u{2195}').contains(&c) || c == '\u{221E}';
    raster_ttf(spec, false, &|_| true, &mut glyphs, &mut bitmaps);
    raster_ttf(NOTO_SANS, false, &is_small_capital, &mut glyphs, &mut bitmaps);
    // Pfeile (← ↑ → ↓ ↔) und Unendlich (∞, FPS-Grenze), die der Schrift fehlen, aus DM Sans --
    // sonst kaemen sie blockig aus der Pixelschrift.
    raster_ttf(DM_SANS_ARROWS, false, &pfeil, &mut glyphs, &mut bitmaps);
    // Fettdruck aus denselben Schriften in fett, statt die normalen Glyphen zweimal versetzt zu
    // zeichnen -- das sah bei Vektorschriften zerhackt aus. Nur Zeichen, die oben schon aus
    // einer TTF kamen; die Minecraft-Ersatzzeichen bleiben Vanilla-fett.
    let mut bold_glyphs: HashMap<char, GlyphInfo> = HashMap::default();
    let aus_ttf = |c: char| has_bold(c) && glyphs.contains_key(&c);
    raster_ttf(spec.bold(), true, &aus_ttf, &mut bold_glyphs, &mut bitmaps);
    raster_ttf(NOTO_SANS_BOLD, true, &|c| aus_ttf(c) && is_small_capital(c), &mut bold_glyphs, &mut bitmaps);
    raster_ttf(DM_SANS_ARROWS.bold(), true, &|c| aus_ttf(c) && pfeil(c), &mut bold_glyphs, &mut bitmaps);
    let bold_advance = bold_glyphs.into_iter().map(|(c, g)| (c, g.advance_px)).collect();

    // Ersatz aus der Minecraft-Schrift, je Faktor ganzzahlig vergroessert.
    for (&ch, g) in &minecraft.glyphs {
        if ch.is_control() || glyphs.contains_key(&ch) {
            continue;
        }
        glyphs.insert(ch, GlyphInfo { advance_px: g.advance_px, ..*g });
        let x0 = (g.uv_min[0] * minecraft.width as f32).round() as u32;
        let y0 = (g.uv_min[1] * minecraft.height as f32).round() as u32;
        let x1 = (g.uv_max[0] * minecraft.width as f32).round() as u32;
        let y1 = (g.uv_max[1] * minecraft.height as f32).round() as u32;
        let (sw, sh) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if sw == 0 || sh == 0 {
            continue;
        }
        let src = &minecraft.levels[0];
        for scale in DM_SANS_QUARTERS.filter(|q| q % 4 == 0) {
            let f = scale as f32 / 4.0;
            let (w, h) = ((g.size_px[0] * f).round().max(1.0) as u32, (g.size_px[1] * f).round().max(1.0) as u32);
            let mut coverage = vec![0u8; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = (x * sw / w, y * sh / h);
                    coverage[(y * w + x) as usize] = src[((y0 + sy) * minecraft.width + x0 + sx) as usize];
                }
            }
            let offset = [(g.offset_px[0] * f).round(), (g.offset_px[1] * f).round()];
            bitmaps.push(Bitmap { scale, ch, bold: false, w, h, coverage, offset });
        }
    }

    // Regal-Packer: Hoehe absteigend, mit etwas Abstand fuer das lineare Sampling.
    bitmaps.sort_by_key(|b| (std::cmp::Reverse(b.h), b.scale, b.ch, b.bold));
    let pad = DM_SANS_PADDING;
    let packen = |width: u32| {
        let mut spots = Vec::with_capacity(bitmaps.len());
        let (mut x, mut y, mut row_h) = (pad, pad, 0u32);
        for b in &bitmaps {
            if x + b.w + pad > width {
                x = pad;
                y += row_h + pad;
                row_h = 0;
            }
            spots.push((x, y));
            x += b.w + pad;
            row_h = row_h.max(b.h);
        }
        (spots, y + row_h + pad)
    };
    // Erst schmal; wird es hoeher als eine Textur sein darf (mit den Subpixel-Fassungen bei
    // Schriften mit vielen Zeichen), doppelt so breit.
    let mut width = ATLAS_MAX_WIDTH;
    let (mut spots, mut height) = packen(width);
    if height > ATLAS_MAX_SIDE {
        width = ATLAS_MAX_SIDE;
        (spots, height) = packen(width);
    }

    let mut coverage = vec![0u8; (width * height) as usize];
    let mut placed = HashMap::default();
    let mut placed_bold = HashMap::default();
    for (b, &(px, py)) in bitmaps.iter().zip(&spots) {
        for row in 0..b.h {
            let d = ((py + row) * width + px) as usize;
            coverage[d..d + b.w as usize].copy_from_slice(&b.coverage[(row * b.w) as usize..((row + 1) * b.w) as usize]);
        }
        let ziel = if b.bold { &mut placed_bold } else { &mut placed };
        ziel.insert(
            (b.scale, b.ch),
            PlacedGlyph {
                uv_min: [px as f32 / width as f32, py as f32 / height as f32],
                uv_max: [(px + b.w) as f32 / width as f32, (py + b.h) as f32 / height as f32],
                offset: b.offset,
                size: [b.w as f32, b.h as f32],
                snap: true,
                pack: false,
                unschaerfe: b.coverage.iter().map(|&c| c as f32 * (255 - c) as f32).sum::<f32>() / (255.0 * 255.0),
            },
        );
    }

    // Unterlaenge aus den tatsaechlich gerasterten Buchstaben der groessten Fassung -- genauer
    // als die Kennzahl im Font-Kopf, die auch Akzente unten mitzaehlt.
    let top = *DM_SANS_QUARTERS.end();
    let descent_px = ['g', 'j', 'p', 'q', 'y']
        .iter()
        .filter_map(|c| placed.get(&(top, *c)))
        .map(|p| (p.offset[1] + p.size[1]) / (top as f32 / 4.0))
        .fold(minecraft.descent_px, f32::max);

    AtlasPixels {
        levels: vec![coverage],
        width,
        height,
        glyphs,
        placed,
        placed_bold,
        bold_advance,
        vanilla_advance: minecraft.vanilla_advance.clone(),
        cell_height_px: minecraft.cell_height_px,
        ascent_px: minecraft.ascent_px,
        descent_px,
    }
}

fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("font atlas bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // Glyphenblatt der Resourcepack-Schriften (s. PackGlyphCache), per textureLoad.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    coverage: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    pack: Option<&PackGlyphCache>,
) -> wgpu::BindGroup {
    // Ohne Pack ein 1x1-Platzhalter -- die Bindung muss belegt sein.
    let platzhalter;
    let pack_view = match pack {
        Some(p) => &p.view,
        None => {
            platzhalter = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("pack glyph placeholder"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default());
            &platzhalter
        }
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("font atlas bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(coverage) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(pack_view) },
        ],
    })
}

// Laedt den Deckungspuffer (samt Mip-Stufen) als Textur hoch; dazu der passende Sampler.
fn upload_atlas(device: &wgpu::Device, queue: &wgpu::Queue, pixels: &AtlasPixels, kind: FontKind) -> (wgpu::TextureView, wgpu::Sampler) {
    let (width, height) = (pixels.width, pixels.height);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("font atlas texture"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: pixels.levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (level, data) in pixels.levels.iter().enumerate() {
        let (w, h) = ((width >> level).max(1), (height >> level).max(1));
        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = match kind {
        // Nearest: ein Pixel-Font soll scharf bleiben, nicht verwaschen.
        FontKind::Minecraft => device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("font atlas sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        }),
        // DM Sans liegt fuer die ueblichen Faktoren 1:1 im Atlas -- Linear trifft dort genau
        // die Texelmitte und aendert nichts. Nur krumme Faktoren werden skaliert, und die
        // sollen dann weich statt treppig werden.
        _ => device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("font atlas sampler (dm sans)"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        }),
    };

    (view, sampler)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s_in_kapitaelchen_woertern() {
        assert_eq!(small_caps_s("ᴘᴠᴘ sᴇʀᴠᴇʀ"), "ᴘᴠᴘ ꜱᴇʀᴠᴇʀ");
        assert_eq!(small_caps_s("Pʟᴀʏᴇʀs: 5"), "Pʟᴀʏᴇʀꜱ: 5");
        // Normale Woerter daneben bleiben.
        assert_eq!(small_caps_s("ᴘᴠᴘ servers"), "ᴘᴠᴘ servers");
        assert_eq!(small_caps_s("sss"), "sss");
        // Kyrillisches ѕ (U+0455) ebenso, aber nur in Kapitaelchen-Woertern.
        assert_eq!(small_caps_s("\u{0455}ᴜʀᴠɪᴠᴀʟ"), "ꜱᴜʀᴠɪᴠᴀʟ");
        assert_eq!(small_caps_s("ᴘᴠᴘ \u{0455}ad"), "ᴘᴠᴘ \u{0455}ad");
    }

    #[test]
    // Ohne Installation: nur der DM-Sans-Teil, mit leerer Minecraft-Vorlage.
    fn alle_ttf_schriften_haben_minecraft_versalhoehe() {
        let leer = AtlasPixels {
            levels: vec![vec![0]],
            width: 1,
            height: 1,
            glyphs: HashMap::default(),
            placed: HashMap::default(),
            placed_bold: HashMap::default(),
            bold_advance: HashMap::default(),
            vanilla_advance: Arc::default(),
            cell_height_px: 8.0,
            ascent_px: 7.0,
            descent_px: 1.0,
        };
        for kind in FontKind::ALL.into_iter().filter(|k| k.ttf().is_some()) {
            let atlas = build_ttf(&leer, kind.ttf().unwrap());
            let h = atlas.placed[&(8, 'H')];
            assert!((h.size[1] - 14.0).abs() <= 1.5, "{}: H bei Faktor 2 ist {} hoch", kind.label(), h.size[1]);
            assert!(atlas.placed.contains_key(&(8, 'ä')), "{}: ä fehlt", kind.label());
            // Kapitaelchen gibt es in jeder Vektorschrift (notfalls aus Noto Sans), etwa so hoch
            // wie Kleinbuchstaben -- nicht die Minecraft-Pixel (die fehlen hier ganz).
            // (ǫ steht fuer q und hat einen Haken nach unten, zaehlt hier nicht mit.)
            assert!(atlas.placed.contains_key(&(8, 'ǫ')), "{}: ǫ fehlt", kind.label());
            for c in "ᴀʙᴄᴅᴇꜰɢʜɪᴊᴋʟᴍɴᴏᴘʀꜱᴛᴜᴠᴡʏᴢ".chars() {
                let g = atlas.placed.get(&(8, c)).unwrap_or_else(|| panic!("{}: {c} fehlt", kind.label()));
                assert!(g.size[1] < h.size[1] && g.size[1] > h.size[1] * 0.5, "{}: {c} ist {} hoch", kind.label(), g.size[1]);
            }
            // Fettdruck aus der fetten Schrift: dicker als normal, gleiche Versalhoehe; auch
            // Kapitaelchen und Umlaute.
            let fett = atlas.placed_bold[&(8, 'H')];
            let tinte = |p: &PlacedGlyph| -> u32 {
                let (x0, y0) = ((p.uv_min[0] * atlas.width as f32) as u32, (p.uv_min[1] * atlas.height as f32) as u32);
                (y0..y0 + p.size[1] as u32).flat_map(|y| (x0..x0 + p.size[0] as u32).map(move |x| (x, y))).map(|(x, y)| atlas.levels[0][(y * atlas.width + x) as usize] as u32).sum()
            };
            assert!(tinte(&fett) as f32 > tinte(&h) as f32 * 1.3, "{}: fettes H nicht dicker", kind.label());
            assert!((fett.size[1] - h.size[1]).abs() <= 1.5, "{}: fettes H ist {} hoch", kind.label(), fett.size[1]);
            assert!(atlas.bold_advance[&'H'] > atlas.glyphs[&'H'].advance_px, "{}: fetter Vorschub", kind.label());
            for c in ['ä', 'ß', 'ᴀ', 'ꜱ', '→'] {
                assert!(atlas.placed_bold.contains_key(&(8, c)), "{}: fettes {c} fehlt", kind.label());
            }
            // ∞ so gross wie eine liegende 8: so breit wie die 8 hoch, auf derselben Mitte.
            let (acht, unendlich) = (atlas.placed[&(8, '8')], atlas.placed[&(8, '\u{221E}')]);
            assert!((unendlich.size[0] - acht.size[1]).abs() <= 2.0, "{}: ∞ {} breit, 8 {} hoch", kind.label(), unendlich.size[0], acht.size[1]);
            assert!((unendlich.size[1] - acht.size[0]).abs() <= 2.0, "{}: ∞ {} hoch, 8 {} breit", kind.label(), unendlich.size[1], acht.size[0]);
            let mitte = |p: &PlacedGlyph| p.offset[1] + p.size[1] / 2.0;
            assert!((mitte(&unendlich) - mitte(&acht)).abs() <= 1.0, "{}: ∞ nicht auf der Mitte der 8", kind.label());
        }
        assert_eq!(FontKind::from_index(4), FontKind::LexendExa);
        assert_eq!(FontKind::from_index(5), FontKind::NotoSans);
        assert_eq!(FontKind::from_index(9), FontKind::Minecraft);
    }

    #[test]
    fn dm_sans_passt_ins_minecraft_raster() {
        let leer = AtlasPixels {
            levels: vec![vec![0]],
            width: 1,
            height: 1,
            glyphs: HashMap::default(),
            placed: HashMap::default(),
            placed_bold: HashMap::default(),
            bold_advance: HashMap::default(),
            vanilla_advance: Arc::default(),
            cell_height_px: 8.0,
            ascent_px: 7.0,
            descent_px: 1.0,
        };
        let atlas = build_ttf(&leer, FontKind::DmSans.ttf().unwrap());
        for scale in DM_SANS_QUARTERS {
            let h = atlas.placed[&(scale, 'H')];
            let f = scale as f32 / 4.0;
            // Versalhoehe ~7 Font-Pixel je Faktor, auf der Grundlinie aufsitzend (die
            // Kantenglaettung darf einen Pixel ueberstehen).
            assert!((h.size[1] - 7.0 * f).abs() <= 1.5, "H bei Faktor {scale} ist {} hoch", h.size[1]);
            assert!((h.offset[1] + h.size[1]).abs() <= 1.0, "H steht bei Faktor {scale} auf der Grundlinie");
            // Pixelgenau: ganzzahlige Lage und Groesse, sonst verwischt es beim Zeichnen.
            assert!(h.snap && h.offset.iter().chain(&h.size).all(|v| v.fract() == 0.0));
        }
        // Umlaute und ß sind drin, das Leerzeichen hat nur einen Vorschub.
        for c in ['ä', 'ö', 'ü', 'ß', 'Ä'] {
            assert!(atlas.placed.contains_key(&(8, c)), "{c} fehlt");
        }
        assert!(atlas.glyphs[&' '].advance_px > 1.0);
        // Natuerliche Ziffernbreiten: die "1" ist schmaler als die "0".
        assert!(atlas.glyphs[&'1'].advance_px < atlas.glyphs[&'0'].advance_px);
        // Die Unterlaenge reicht tiefer als bei Minecraft (1 Pixel), aber nicht absurd tief.
        assert!(atlas.descent_px > 1.5 && atlas.descent_px < 4.0, "Unterlänge {}", atlas.descent_px);
        // Keine Mip-Kette mehr.
        assert_eq!(atlas.levels.len(), 1);
    }

    // Derselbe Text soll an jeder Lage auf dem Raster gleich aussehen (s. sharpest_shift):
    // mit der gewaehlten Verschiebung gezeichnet, ist die Deckung je Bildschirmspalte bis auf
    // ganze Pixel Versatz dieselbe. Vorher sah "slopcore" je nach Lage verschieden aus, das l
    // mal mittig, mal schief.
    #[test]
    fn text_an_jeder_lage_gleich() {
        let leer = AtlasPixels {
            levels: vec![vec![0]],
            width: 1,
            height: 1,
            glyphs: HashMap::default(),
            placed: HashMap::default(),
            placed_bold: HashMap::default(),
            bold_advance: HashMap::default(),
            vanilla_advance: Arc::default(),
            cell_height_px: 8.0,
            ascent_px: 7.0,
            descent_px: 1.0,
        };
        let atlas = build_ttf(&leer, FontKind::Outfit.ttf().unwrap());
        let scale = 2.0f32;
        for text in ["slopcore", "DerAntonia", "CoralClient", "Illinois"] {
            let mut pos = Vec::new();
            let mut pen = 0.0;
            for ch in text.chars() {
                pos.push(pen);
                pen += atlas.glyphs[&ch].advance_px * scale;
            }
            let mut profile = Vec::new();
            for start in (0..16).map(|i| 100.0 + i as f32 / 16.0 + 0.01) {
                let d = sharpest_shift_in(&atlas.placed, text, &pos, start, scale, |c| pick_placed(&atlas.placed, c, scale));
                let mut spalten: Vec<(i32, u8)> = Vec::new();
                for (ch, &rel) in text.chars().zip(&pos) {
                    let p = pick_placed(&atlas.placed, ch, scale).unwrap();
                    let (p, x) = snap_in(&atlas.placed, ch, scale, p, start + d + rel);
                    let (x0, y0) = ((p.uv_min[0] * atlas.width as f32).round() as u32, (p.uv_min[1] * atlas.height as f32).round() as u32);
                    for dx in 0..p.size[0] as u32 {
                        let m = (0..p.size[1] as u32).map(|dy| atlas.levels[0][((y0 + dy) * atlas.width + x0 + dx) as usize]).max().unwrap_or(0);
                        spalten.push(((x + p.offset[0]) as i32 + dx as i32, m));
                    }
                }
                let links = spalten.iter().map(|s| s.0).min().unwrap();
                let mut prof = vec![0u8; 200];
                for (sp, m) in spalten {
                    let k = (sp - links) as usize;
                    prof[k] = prof[k].max(m);
                }
                profile.push(prof);
            }
            let verschieden: std::collections::HashSet<_> = profile.iter().collect();
            assert_eq!(verschieden.len(), 1, "{text}: {} verschiedene Bilder bei 16 Lagen", verschieden.len());
        }
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    #[ignore]
    // Gegen die echte Installation: wie gross wird der Atlas mit allen Ersatzzeichen?
    // cargo test -p app --bin coral-client font_atlas::install_tests -- --ignored --nocapture
    fn dm_sans_atlas_mit_minecraft_ersatz() {
        let mut mc = assets::MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
        let start = std::time::Instant::now();
        let minecraft = build_minecraft(&mut mc).expect("Minecraft-Schrift");
        let atlas = build_ttf(&minecraft, FontKind::DmSans.ttf().unwrap());
        let bytes: usize = atlas.levels.iter().map(Vec::len).sum();
        println!(
            "{} Glyphen ({} aus Minecraft), Atlas {}x{}, {:.1} MB, {:?}",
            atlas.glyphs.len(),
            minecraft.glyphs.len(),
            atlas.width,
            atlas.height,
            bytes as f64 / 1e6,
            start.elapsed()
        );
        // Jedes Zeichen der Minecraft-Schrift bleibt darstellbar.
        assert!(minecraft.glyphs.keys().all(|c| atlas.glyphs.contains_key(c)));
        assert!(atlas.height <= 8192);
        // Auch die Schriften mit mehr Zeichen (samt Fettdruck) passen.
        for kind in FontKind::ALL.into_iter().filter(|k| k.ttf().is_some()) {
            let atlas = build_ttf(&minecraft, kind.ttf().unwrap());
            println!("{}: Atlas {}x{}", kind.label(), atlas.width, atlas.height);
            assert!(atlas.height <= 8192);
        }
    }
}
