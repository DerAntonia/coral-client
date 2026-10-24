// Laedt Minecrafts Standard-Font aus der lokalen Installation des Nutzers.
//
// Aufbau (verifiziert am Client-Jar 1.21.11): font/default.json verweist per "reference"
// auf include/space.json (Leerzeichen-Breite) und include/default.json (die eigentlichen
// Bitmap-Provider). Dort stehen DREI Bitmap-Seiten, nicht eine:
//
//   nonlatin_european.png  128x536, 16x67 Zellen (8x8), ascent 7   -- u.a. das ß
//   accented.png           144x900, 16x75 Zellen (9x12), ascent 10 -- ä ö ü Ä Ö Ü und Verwandte
//   ascii.png              128x128, 16x16 Zellen (8x8), ascent 7   -- der ASCII-Bereich
//
// Frueher wurde NUR ascii.png gelesen -- deutscher Text fiel damit auseinander, weil jedes
// Zeichen ausserhalb von ASCII schlicht fehlte (ä, ö, ß). Die drei Seiten ueberschneiden sich in keinem einzigen Zeichen (nachgerechnet
// an den chars-Rastern), die Reihenfolge entscheidet also nichts -- spaetere Seiten
// ueberschreiben trotzdem, weil ascii.png in der Datei zuletzt steht und im Zweifel gelten
// soll.
//
// WICHTIG: Die Font-Texturen sind Mojangs Eigentum und werden niemals mitgeliefert -- sie
// werden zur Laufzeit aus der Installation des Nutzers gelesen (s. Briefing).

use std::collections::HashMap;

use serde_json::Value;

use crate::installation::{AssetsError, DecodedTexture, MinecraftInstallation};

// Eine Bitmap-Seite des Fonts.
pub struct FontPage {
    // Die Seite als RGBA. Die Glyphen sind weiss; die Form steckt im Alphakanal.
    pub texture: DecodedTexture,
    // Zeilen x Spalten des Rasters; '\0' bedeutet "Zelle unbenutzt".
    pub chars: Vec<Vec<char>>,
    // Abstand von der Grundlinie zur Oberkante der Zelle.
    pub ascent: i32,
    // Zielhoehe einer Zelle in Font-Pixeln ("height" im Provider, Vorgabe 8). Vanilla
    // skaliert die Zelle darauf: `f = height / Zellenhoehe` (BitmapFont.Loader.load). Bei
    // allen drei Vanilla-Seiten kommt dabei genau 1.0 heraus -- der Faktor steht hier
    // trotzdem, weil ein Resourcepack ihn sehr wohl veraendern kann.
    pub height: i32,
}

impl FontPage {
    pub fn cell_size(&self) -> (u32, u32) {
        (self.texture.width / self.chars[0].len() as u32, self.texture.height / self.chars.len() as u32)
    }

    // Vanillas Skalierungsfaktor der Seite (BitmapFont.Loader.load: `f = height / l`).
    pub fn scale(&self) -> f32 {
        let (_, cell_h) = self.cell_size();
        if cell_h == 0 {
            return 1.0;
        }
        self.height as f32 / cell_h as f32
    }

    // Minecraft legt die Zeichenbreite nicht in einer Tabelle ab, sondern misst sie:
    // gesucht wird die rechteste Spalte der Zelle, die ueberhaupt etwas zeichnet
    // (BitmapFont.Loader.findCharacterStartX). Genau deshalb ist ein "i" schmal und ein "W"
    // breit. Rueckgabe 0 = leere Zelle.
    pub fn glyph_width(&self, col: u32, row: u32) -> u32 {
        let (cw, ch) = self.cell_size();
        for x in (0..cw).rev() {
            for y in 0..ch {
                let px = col * cw + x;
                let py = row * ch + y;
                let alpha = self.texture.rgba[((py * self.texture.width + px) * 4 + 3) as usize];
                if alpha != 0 {
                    return x + 1;
                }
            }
        }
        0
    }

    // Der Vorschub eines Zeichens, exakt nach Vanilla: `(int)(0.5 + q * f) + 1`.
    pub fn advance(&self, col: u32, row: u32) -> f32 {
        let q = self.glyph_width(col, row) as f32;
        ((0.5 + q * self.scale()) as i32 + 1) as f32
    }
}

pub struct BitmapFont {
    // In der Reihenfolge der Datei. Die ASCII-Seite steht bei Vanilla zuletzt.
    pub pages: Vec<FontPage>,
    // Vorschub des Leerzeichens; kommt aus einem eigenen "space"-Provider, weil die
    // zugehoerige Bitmap-Zelle leer ist und sonst die Breite 1 ergaebe.
    pub space_advance: f32,
    // Die uebrigen Zeichen des space-Providers mit ihrem Vorschub. Bei Vanilla nur U+200C
    // (Zero Width Non-Joiner) mit 0 -- ohne den Eintrag kaeme es als Unifont-Kaestchen "ZW NJ".
    pub spaces: Vec<(char, f32)>,
}

impl BitmapFont {
    // Die Seite, an der sich Zeilenhoehe und Grundlinie der Oberflaeche ausrichten: die mit
    // dem ASCII-Bereich. Alles Layout (Zeilenabstand, senkrechtes Zentrieren) haengt daran,
    // und es soll sich nicht aendern, nur weil ein Umlaut in der Zeile steht -- genau so
    // haelt es Vanilla mit seiner festen Zeilenhoehe.
    pub fn base_page(&self) -> &FontPage {
        self.pages.iter().find(|p| p.chars.iter().any(|row| row.contains(&'A'))).unwrap_or(&self.pages[0])
    }
}

// Ein Zeichen der Zauberschrift: Pixelrechteck in ascii_sga.png (x, y, Breite bis zur
// rechtesten gemalten Spalte, Zellenhoehe) und Vorschub wie beim Standard-Font.
#[derive(Clone, Copy, Debug)]
pub struct AltGlyph {
    pub cell: [u32; 4],
    pub advance: f32,
}

#[derive(Default)]
pub struct AltFontGlyphs {
    pub glyphs: HashMap<char, AltGlyph>,
    pub space_advance: f32,
    pub sheet_size: [u32; 2],
}

impl AltFontGlyphs {
    // Breite eines Textes in GUI-Pixeln; unbekannte Zeichen zaehlen nicht.
    pub fn width(&self, text: &str) -> f32 {
        text.chars().map(|c| if c == ' ' { self.space_advance } else { self.glyphs.get(&c).map_or(0.0, |g| g.advance) }).sum()
    }
}

impl MinecraftInstallation {
    pub fn load_default_font(&mut self) -> Result<BitmapFont, AssetsError> {
        let spaces = self.load_space_advances().unwrap_or_default();
        let space_advance = spaces.iter().find(|(c, _)| *c == ' ').map_or(4.0, |&(_, a)| a);
        let spaces = spaces.into_iter().filter(|(c, _)| *c != ' ').collect();

        let definition: Value = serde_json::from_slice(&self.load_bytes("minecraft/font/include/default.json")?)?;
        let providers = definition
            .get("providers")
            .and_then(Value::as_array)
            .ok_or_else(|| AssetsError::FontFormat("include/default.json ohne providers-Array".into()))?;

        let mut pages = Vec::new();
        for provider in providers {
            if provider.get("type").and_then(Value::as_str) != Some("bitmap") {
                continue;
            }
            // Eine einzelne unlesbare Seite darf nicht den ganzen Font kippen: dann fehlen
            // eben deren Zeichen, statt dass der Client gar nichts anzeigen kann.
            if let Some(page) = self.load_font_page(provider) {
                pages.push(page);
            }
        }

        if pages.is_empty() {
            return Err(AssetsError::FontFormat("kein brauchbarer bitmap-Provider in include/default.json".into()));
        }
        Ok(BitmapFont { pages, space_advance, spaces })
    }

    // Die Zauberschrift (font/alt.json, "Standard Galactic Alphabet"): EINE Bitmap-Seite
    // (ascii_sga.png) mit den Buchstaben a-z und A-Z. Der Zaubertisch setzt seine Zierzeilen
    // in diese Schrift. Der Atlas zeichnet die Seite als UI-Sheet, hier interessiert je
    // Zeichen nur seine Zelle und sein Vorschub.
    pub fn load_alt_font_glyphs(&mut self) -> Result<AltFontGlyphs, AssetsError> {
        let definition: Value = serde_json::from_slice(&self.load_bytes("minecraft/font/alt.json")?)?;
        let page = definition
            .get("providers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("bitmap"))
            .find_map(|p| self.load_font_page(p))
            .ok_or_else(|| AssetsError::FontFormat("alt.json ohne brauchbare Bitmap-Seite".into()))?;
        let (cw, ch) = page.cell_size();
        let mut glyphs = HashMap::new();
        for (row, zeile) in page.chars.iter().enumerate() {
            for (col, &c) in zeile.iter().enumerate() {
                if c == '\0' {
                    continue;
                }
                let (col, row) = (col as u32, row as u32);
                let w = page.glyph_width(col, row);
                glyphs.insert(c, AltGlyph { cell: [col * cw, row * ch, w, ch], advance: page.advance(col, row) });
            }
        }
        Ok(AltFontGlyphs { glyphs, space_advance: self.load_space_advance().unwrap_or(4.0), sheet_size: [page.texture.width, page.texture.height] })
    }

    fn load_font_page(&mut self, provider: &Value) -> Option<FontPage> {
        let ascent = provider.get("ascent").and_then(Value::as_i64)? as i32;
        // "height" ist optional, Vorgabe 8 (BitmapFont.Loader-Codec).
        let height = provider.get("height").and_then(Value::as_i64).unwrap_or(8) as i32;
        let chars: Vec<Vec<char>> =
            provider.get("chars")?.as_array()?.iter().filter_map(|row| row.as_str().map(|s| s.chars().collect())).collect();
        if chars.is_empty() || chars[0].is_empty() {
            return None;
        }

        // "minecraft:font/ascii.png" -> "minecraft/textures/font/ascii.png"
        let file = provider.get("file").and_then(Value::as_str)?;
        let (namespace, path) = file.split_once(':').unwrap_or(("minecraft", file));
        let texture = self.load_texture(&format!("{namespace}/textures/{path}")).ok()?;
        Some(FontPage { texture, chars, ascent, height })
    }

    fn load_space_advance(&mut self) -> Option<f32> {
        self.load_space_advances()?.into_iter().find(|(c, _)| *c == ' ').map(|(_, a)| a)
    }

    fn load_space_advances(&mut self) -> Option<Vec<(char, f32)>> {
        let value: Value = serde_json::from_slice(&self.load_bytes("minecraft/font/include/space.json").ok()?).ok()?;
        let advances = value
            .get("providers")?
            .as_array()?
            .iter()
            .find(|p| p.get("type").and_then(Value::as_str) == Some("space"))?
            .get("advances")?
            .as_object()?;
        Some(
            advances
                .iter()
                .filter_map(|(k, a)| {
                    let mut it = k.chars();
                    match (it.next(), it.next(), a.as_f64()) {
                        (Some(c), None, Some(a)) => Some((c, a as f32)),
                        _ => None,
                    }
                })
                .collect(),
        )
    }
}

// --- Schriften aus Resourcepacks (Server-HUDs, Logos, Symbole) -------------------------------
//
// Server legen ihr HUD aus Schriftzeichen: Logos, Leisten, Rahmen sind farbige Bilder in einer
// Schrift, negative Abstaende ("space"-Provider oder Bitmaps mit negativer Hoehe) schieben sie
// an ihren Platz. Dazu kommen eigene Schriften ("peacock:hud"), die ein Text-Component ueber
// sein "font"-Feld waehlt.
//
// Geladen wird hier alles AUSSER Vanillas Grundschrift (include/default, include/space -- die
// liest load_default_font) und den Unicode-Ersatzschriften (unihex/ttf, nicht unterstuetzt).
// Je Schrift-ID entsteht eine Zeichentabelle; welcher Provider ein Zeichen liefert, entscheidet
// die Reihenfolge wie bei Vanilla: das oberste Pack zuerst, in einer Datei von oben nach unten.

// Ein Zeichen aus einem Pack. `page` = Index in PackFonts::pages, None fuer reinen Abstand.
#[derive(Clone, Copy, Debug)]
pub struct PackGlyph {
    pub page: Option<u32>,
    // Zelle in der Seite, in Texeln.
    pub cell: [u32; 4],
    // Zielhoehe / Zellenhoehe (darf negativ sein -- dann ist das Bild unsichtbar).
    pub scale: f32,
    pub ascent: f32,
    pub advance: f32,
}

#[derive(Default)]
pub struct PackFonts {
    pub pages: Vec<DecodedTexture>,
    // Schrift-ID ("minecraft:default", "peacock:hud") -> Zeichen.
    pub fonts: HashMap<String, HashMap<char, PackGlyph>>,
    // Vanillas Unicode-Ersatzschrift (include/unifont), s. Unihex.
    pub unihex: Option<std::sync::Arc<Unihex>>,
}

// --- Unicode-Ersatzschrift (UnihexFont) ------------------------------------------------------
//
// Was die Bitmap-Seiten der Standardschrift nicht haben (Symbole wie ✦, fast alle Schriften
// der Welt), zeichnet Vanilla aus GNU Unifont: eine .hex-Datei in font/unifont.zip, je Zeile
// "Codepunkt:Bitmap" mit 16 Zeilen zu 8, 16, 24 oder 32 Pixeln. Gezeichnet wird mit halber
// Groesse (16 Zeilen = 8 Font-Pixel), auf die besetzten Spalten beschnitten, Vorschub
// Breite / 2 + 1 (UnihexFont.UnicodeTextureGlyph).

// Ein Zeichen: 16 Zeilen, Pixel x liegt in Bit 31 - x; `left`/`right` sind die erste und
// letzte gezeichnete Spalte.
#[derive(Clone, Copy, Debug)]
pub struct UniGlyph {
    pub rows: [u32; 16],
    pub left: u8,
    pub right: u8,
}

impl UniGlyph {
    pub fn width(&self) -> u32 {
        self.right as u32 - self.left as u32 + 1
    }

    pub fn advance(&self) -> f32 {
        (self.width() / 2 + 1) as f32
    }

    // Deckung (0/255) Zeile fuer Zeile, `width()` Pixel breit.
    pub fn pixels(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width() as usize * 16);
        for row in self.rows {
            for x in self.left as u32..=self.right as u32 {
                out.push(if x < 32 && row >> (31 - x) & 1 != 0 { 255 } else { 0 });
            }
        }
        out
    }
}

#[derive(Default)]
pub struct Unihex {
    pub glyphs: HashMap<char, UniGlyph>,
}

// Eine .hex-Datei einlesen (UnihexFont.readLines). Unlesbare Zeilen werden uebersprungen.
fn read_hex(text: &str, glyphs: &mut HashMap<char, (u32, [u32; 16])>) {
    for line in text.lines() {
        let Some((cp, data)) = line.split_once(':') else { continue };
        let Some(ch) = u32::from_str_radix(cp, 16).ok().and_then(char::from_u32) else { continue };
        let data = data.trim();
        let breite = match data.len() {
            32 => 8,
            64 => 16,
            96 => 24,
            128 => 32,
            _ => continue,
        };
        let ziffern = breite / 4;
        let mut rows = [0u32; 16];
        let mut ok = true;
        for (y, row) in rows.iter_mut().enumerate() {
            match u32::from_str_radix(&data[y * ziffern..(y + 1) * ziffern], 16) {
                Ok(v) => *row = if breite == 32 { v } else { v << (32 - breite) },
                Err(_) => ok = false,
            }
        }
        if ok {
            glyphs.insert(ch, (breite as u32, rows));
        }
    }
}

enum PackProvider {
    Bitmap { file: String, chars: Vec<Vec<char>>, ascent: i32, height: i32 },
    Space(Vec<(char, f32)>),
    Reference(String),
}

// "peacock:hud" bzw. "hud" -> mit Namensraum.
pub fn full_font_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

// Was die Grundschrift schon abdeckt (s. load_default_font) oder hier nicht gelesen wird.
fn is_base_font(id: &str) -> bool {
    matches!(id, "minecraft:include/default" | "minecraft:include/space" | "minecraft:include/unifont")
}

fn parse_providers(bytes: &[u8]) -> Vec<PackProvider> {
    let Ok(v) = serde_json::from_slice::<Value>(crate::installation::strip_bom(bytes)) else { return Vec::new() };
    let Some(list) = v.get("providers").and_then(Value::as_array) else { return Vec::new() };
    let mut out = Vec::new();
    for p in list {
        // Filter: Provider nur fuer erzwungene Unicode-Schrift oder Japanisch -- beides aus.
        if let Some(f) = p.get("filter") {
            if f.get("uniform").and_then(Value::as_bool) == Some(true) || f.get("jp").and_then(Value::as_bool) == Some(true) {
                continue;
            }
        }
        match p.get("type").and_then(Value::as_str) {
            Some("bitmap") => {
                let (Some(file), Some(ascent)) = (p.get("file").and_then(Value::as_str), p.get("ascent").and_then(Value::as_i64)) else { continue };
                let height = p.get("height").and_then(Value::as_i64).unwrap_or(8) as i32;
                let chars: Vec<Vec<char>> = p
                    .get("chars")
                    .and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(|r| r.as_str().map(|s| s.chars().collect())).collect())
                    .unwrap_or_default();
                // Vanilla verwirft die Seite bei ungleich langen Zeilen und bei ascent > height.
                if chars.is_empty() || chars[0].is_empty() || chars.iter().any(|r| r.len() != chars[0].len()) || ascent as i32 > height {
                    continue;
                }
                out.push(PackProvider::Bitmap { file: file.to_string(), chars, ascent: ascent as i32, height });
            }
            Some("space") => {
                let Some(adv) = p.get("advances").and_then(Value::as_object) else { continue };
                let mut v = Vec::new();
                for (k, a) in adv {
                    let mut it = k.chars();
                    if let (Some(c), None, Some(a)) = (it.next(), it.next(), a.as_f64()) {
                        v.push((c, a as f32));
                    }
                }
                out.push(PackProvider::Space(v));
            }
            Some("reference") => {
                if let Some(id) = p.get("id").and_then(Value::as_str) {
                    out.push(PackProvider::Reference(full_font_id(id)));
                }
            }
            _ => {}
        }
    }
    out
}

impl MinecraftInstallation {
    // Vanillas Ersatzschrift aus font/include/unifont.json (Provider "unihex", ohne den
    // japanischen Filter-Satz). None, wenn sie fehlt -- dann bleiben solche Zeichen leer.
    pub fn load_unifont(&mut self) -> Option<Unihex> {
        let def: Value = serde_json::from_slice(&self.load_bytes("minecraft/font/include/unifont.json").ok()?).ok()?;
        let mut roh: HashMap<char, (u32, [u32; 16])> = HashMap::new();
        let mut ueber: Vec<(u32, u32, u8, u8)> = Vec::new();
        for p in def.get("providers")?.as_array()? {
            if p.get("type").and_then(Value::as_str) != Some("unihex") {
                continue;
            }
            if p.get("filter").and_then(|f| f.get("jp")).and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let Some(datei) = p.get("hex_file").and_then(Value::as_str) else { continue };
            let (ns, pfad) = datei.split_once(':').unwrap_or(("minecraft", datei));
            let Ok(bytes) = self.load_bytes(&format!("{ns}/{pfad}")) else { continue };
            let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { continue };
            for i in 0..zip.len() {
                let Ok(mut f) = zip.by_index(i) else { continue };
                if !f.name().ends_with(".hex") {
                    continue;
                }
                let mut text = String::new();
                if std::io::Read::read_to_string(&mut f, &mut text).is_ok() {
                    read_hex(&text, &mut roh);
                }
            }
            for o in p.get("size_overrides").and_then(Value::as_array).into_iter().flatten() {
                let zeichen = |k: &str| o.get(k).and_then(Value::as_str).and_then(|s| s.chars().next()).map(|c| c as u32);
                let zahl = |k: &str| o.get(k).and_then(Value::as_i64).map(|v| v as u8);
                if let (Some(von), Some(bis), Some(l), Some(r)) = (zeichen("from"), zeichen("to"), zahl("left"), zahl("right")) {
                    ueber.push((von, bis, l, r));
                }
            }
        }
        if roh.is_empty() {
            return None;
        }
        let mut glyphs = HashMap::with_capacity(roh.len());
        for (ch, (breite, rows)) in roh {
            let feste = ueber.iter().find(|(von, bis, _, _)| (*von..=*bis).contains(&(ch as u32)));
            let (left, right) = match feste {
                Some(&(_, _, l, r)) => (l, r),
                None => {
                    // getPackedDimensions: erste und letzte besetzte Spalte; leer = ganze Breite.
                    let maske = rows.iter().fold(0u32, |a, r| a | r);
                    if maske == 0 {
                        (0, breite as u8)
                    } else {
                        (maske.leading_zeros() as u8, (31 - maske.trailing_zeros()) as u8)
                    }
                }
            };
            glyphs.insert(ch, UniGlyph { rows, left, right });
        }
        Some(Unihex { glyphs })
    }

    pub fn load_pack_fonts(&mut self) -> PackFonts {
        // Je Schrift die Provider, oberste Datei zuerst.
        let mut defs: HashMap<String, Vec<PackProvider>> = HashMap::new();
        for (id, dateien) in self.font_definitions() {
            let list = dateien.iter().rev().flat_map(|b| parse_providers(b)).collect();
            defs.insert(id, list);
        }

        let mut out = PackFonts { unihex: self.load_unifont().map(std::sync::Arc::new), ..Default::default() };
        let mut seiten: HashMap<String, Option<u32>> = HashMap::new();
        let ids: Vec<String> = defs.keys().filter(|id| !is_base_font(id)).cloned().collect();
        for id in ids {
            let mut tabelle: HashMap<char, PackGlyph> = HashMap::new();
            let mut besucht = Vec::new();
            self.fill_font(&id, &defs, &mut besucht, &mut seiten, &mut out.pages, &mut tabelle);
            if !tabelle.is_empty() {
                out.fonts.insert(id, tabelle);
            }
        }
        out
    }

    fn fill_font(
        &mut self,
        id: &str,
        defs: &HashMap<String, Vec<PackProvider>>,
        besucht: &mut Vec<String>,
        seiten: &mut HashMap<String, Option<u32>>,
        pages: &mut Vec<DecodedTexture>,
        tabelle: &mut HashMap<char, PackGlyph>,
    ) {
        if besucht.iter().any(|b| b == id) || besucht.len() > 8 || is_base_font(id) {
            return;
        }
        besucht.push(id.to_string());
        let Some(providers) = defs.get(id) else { return };
        for p in providers {
            match p {
                PackProvider::Reference(r) => self.fill_font(r, defs, besucht, seiten, pages, tabelle),
                PackProvider::Space(adv) => {
                    for &(c, a) in adv {
                        tabelle.entry(c).or_insert(PackGlyph { page: None, cell: [0; 4], scale: 1.0, ascent: 0.0, advance: a });
                    }
                }
                PackProvider::Bitmap { file, chars, ascent, height } => {
                    let page = *seiten.entry(file.clone()).or_insert_with(|| {
                        let (ns, path) = file.split_once(':').unwrap_or(("minecraft", file));
                        let tex = self.load_texture(&format!("{ns}/textures/{path}")).ok()?;
                        pages.push(tex);
                        Some(pages.len() as u32 - 1)
                    });
                    let Some(page) = page else { continue };
                    let tex = &pages[page as usize];
                    let (cw, ch) = (tex.width / chars[0].len() as u32, tex.height / chars.len() as u32);
                    if cw == 0 || ch == 0 {
                        continue;
                    }
                    let scale = *height as f32 / ch as f32;
                    for (row, zeile) in chars.iter().enumerate() {
                        for (col, &c) in zeile.iter().enumerate() {
                            if c == '\0' || tabelle.contains_key(&c) {
                                continue;
                            }
                            let (x, y) = (col as u32 * cw, row as u32 * ch);
                            let breite = glyph_width_in(tex, x, y, cw, ch);
                            // Vanillas Vorschub: (int)(0.5 + Breite * Faktor) + 1, Java rundet
                            // dabei zur Null hin -- auch bei negativem Faktor.
                            let advance = ((0.5 + breite as f32 * scale) as f64).trunc() as f32 + 1.0;
                            tabelle.insert(c, PackGlyph { page: Some(page), cell: [x, y, cw, ch], scale, ascent: *ascent as f32, advance });
                        }
                    }
                }
            }
        }
    }
}

// Rechteste Spalte mit Deckung + 1 (s. FontPage::glyph_width), 0 fuer eine leere Zelle.
fn glyph_width_in(tex: &DecodedTexture, x0: u32, y0: u32, cw: u32, ch: u32) -> u32 {
    for x in (0..cw).rev() {
        for y in 0..ch {
            let i = (((y0 + y) * tex.width + x0 + x) * 4 + 3) as usize;
            if tex.rgba.get(i).is_some_and(|&a| a != 0) {
                return x + 1;
            }
        }
    }
    0
}

#[cfg(test)]
mod unifont_tests {
    // Vanillas Ersatzschrift aus der Installation; ohne Installation wird nichts geprueft.
    #[test]
    fn stern_kommt_aus_unifont() {
        let Ok(mut mc) = crate::MinecraftInstallation::locate("1.21.11", Vec::new()) else { return };
        let t = std::time::Instant::now();
        let Some(u) = mc.load_unifont() else { panic!("unifont fehlt") };
        eprintln!("unifont: {} Zeichen in {:?}", u.glyphs.len(), t.elapsed());
        // U+2726 (✦): 16 Pixel breit gespeichert, besetzt von Spalte 0 bis 14.
        let g = u.glyphs[&'\u{2726}'];
        assert_eq!((g.left, g.right), (0, 14));
        assert_eq!(g.advance(), 8.0);
        assert_eq!(g.pixels().len(), 15 * 16);
        assert!(u.glyphs.len() > 50_000);
    }
}
