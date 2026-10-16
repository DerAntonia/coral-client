// Minimaler NBT-Reader -- nur zum Lesen, kein Schreiben (wir empfangen NBT nur in
// Registry-Data-Paketen, s. connection.rs). Deckt alle Standard-Tag-Typen ab, damit
// beliebige Registry-Eintraege (Biome, ...) per Namen navigiert werden koennen, auch wenn
// wir nur einen Bruchteil ihrer Felder brauchen.
//
// Format ("Netzwerk-NBT", s. NbtIo.writeForPacket/PacketByteBuf.readNbt im Vanilla-Client,
// per javap/Vineflower gegen den Fabric-Loom-Cache verifiziert): 1 Byte Tag-Typ, dann --
// ANDERS als bei NBT-Dateien -- OHNE Namen fuer das Wurzel-Tag direkt der Payload. Innerhalb
// eines Compounds traegt jedes Kind-Tag ganz normal Typ+Name+Payload, terminiert durch
// Typ 0 (TAG_End).

use crate::buffer::{PacketReader, ReadError};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum NbtValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<NbtValue>),
    Compound(HashMap<String, NbtValue>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

// Seit 1.21.5 darf eine NBT-Liste gemischte Typen enthalten: Vanilla (NbtOps.ListBuilder)
// schreibt sie dann als Liste von Compounds und packt jedes Nicht-Compound-Element in einen
// Compound mit dem leeren Schluessel "" (`{"": "⚔"}`). Beim Lesen wieder auspacken -- sonst
// fehlte z.B. in einem Titel `["", {text:"⚔",color:...}, " Match Found ", "⚔"]` jedes
// reine Textstueck (der Titel stand ohne das Symbol am Ende da).
fn unwrap_list_element(v: NbtValue) -> NbtValue {
    match v {
        NbtValue::Compound(mut map) if map.len() == 1 && map.contains_key("") => map.remove("").unwrap_or(NbtValue::Compound(HashMap::new())),
        other => other,
    }
}

impl NbtValue {
    pub fn as_compound(&self) -> Option<&HashMap<String, NbtValue>> {
        match self {
            NbtValue::Compound(m) => Some(m),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&NbtValue> {
        self.as_compound()?.get(key)
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            NbtValue::Float(v) => Some(*v),
            NbtValue::Double(v) => Some(*v as f32),
            NbtValue::Byte(v) => Some(*v as f32),
            NbtValue::Short(v) => Some(*v as f32),
            NbtValue::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            NbtValue::Int(v) => Some(*v),
            NbtValue::Byte(v) => Some(*v as i32),
            NbtValue::Short(v) => Some(*v as i32),
            NbtValue::Long(v) => Some(*v as i32),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            NbtValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

const TAG_END: u8 = 0;
const TAG_BYTE: u8 = 1;
const TAG_SHORT: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_LONG: u8 = 4;
const TAG_FLOAT: u8 = 5;
const TAG_DOUBLE: u8 = 6;
const TAG_BYTE_ARRAY: u8 = 7;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;
const TAG_INT_ARRAY: u8 = 11;
const TAG_LONG_ARRAY: u8 = 12;

// NBT-eigene String-Kodierung: 2 Byte vorzeichenlose Laenge, dann Javas "modified UTF-8"
// (DataOutput.writeUTF) -- ANDERS als PacketReader::read_string (VarInt-Laenge, echtes UTF-8).
fn read_nbt_string(r: &mut PacketReader) -> Result<String, ReadError> {
    let len = r.read_u16()? as usize;
    let bytes = r.read_bytes(len)?;
    decode_modified_utf8(bytes)
}

// Modified UTF-8 unterscheidet sich von UTF-8 an genau zwei Stellen: NUL ist 0xC0 0x80, und
// Zeichen ausserhalb der Grundebene (Emojis!) stehen als ZWEI einzeln kodierte UTF-16-
// Surrogate zu je drei Bytes statt als eine Vier-Byte-Folge.
//
// ECHTER BUG gewesen: hier stand from_utf8 mit der Annahme "fuer ASCII-lastige Namen
// aequivalent". Ein Emoji im Itemnamen (mcpvp.club: "🗡 Kit Editor", ein Golden Head)
// brach damit den Namen ab -- und mit ihm die ganze restliche Slot-Liste.
fn decode_modified_utf8(bytes: &[u8]) -> Result<String, ReadError> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        // Ohne Surrogate und ohne 0xC0 0x80 sind beide Kodierungen gleich.
        return Ok(s.to_string());
    }
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let folge = |k: usize| bytes.get(i + k).copied().filter(|c| c & 0xC0 == 0x80).ok_or(ReadError);
        let (unit, laenge) = if b & 0x80 == 0 {
            (b as u16, 1)
        } else if b & 0xE0 == 0xC0 {
            ((((b & 0x1F) as u16) << 6) | (folge(1)? & 0x3F) as u16, 2)
        } else if b & 0xF0 == 0xE0 {
            ((((b & 0x0F) as u16) << 12) | (((folge(1)? & 0x3F) as u16) << 6) | (folge(2)? & 0x3F) as u16, 3)
        } else {
            return Err(ReadError);
        };
        units.push(unit);
        i += laenge;
    }
    // Ein einzelnes, unpaariges Surrogat macht nicht den ganzen Text kaputt.
    Ok(String::from_utf16_lossy(&units))
}

fn read_payload(r: &mut PacketReader, tag_type: u8) -> Result<NbtValue, ReadError> {
    Ok(match tag_type {
        TAG_BYTE => NbtValue::Byte(r.read_i8()?),
        TAG_SHORT => NbtValue::Short(r.read_i16()?),
        TAG_INT => NbtValue::Int(r.read_i32()?),
        TAG_LONG => NbtValue::Long(r.read_i64()?),
        TAG_FLOAT => NbtValue::Float(r.read_f32()?),
        TAG_DOUBLE => NbtValue::Double(r.read_f64()?),
        TAG_BYTE_ARRAY => {
            let len = r.read_i32()?.max(0) as usize;
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                out.push(r.read_i8()?);
            }
            NbtValue::ByteArray(out)
        }
        TAG_STRING => NbtValue::String(read_nbt_string(r)?),
        TAG_LIST => {
            let elem_type = r.read_u8()?;
            let len = r.read_i32()?.max(0) as usize;
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                out.push(unwrap_list_element(read_payload(r, elem_type)?));
            }
            NbtValue::List(out)
        }
        TAG_COMPOUND => {
            let mut map = HashMap::new();
            loop {
                let child_type = r.read_u8()?;
                if child_type == TAG_END {
                    break;
                }
                let name = read_nbt_string(r)?;
                let value = read_payload(r, child_type)?;
                map.insert(name, value);
            }
            NbtValue::Compound(map)
        }
        TAG_INT_ARRAY => {
            let len = r.read_i32()?.max(0) as usize;
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                out.push(r.read_i32()?);
            }
            NbtValue::IntArray(out)
        }
        TAG_LONG_ARRAY => {
            let len = r.read_i32()?.max(0) as usize;
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                out.push(r.read_i64()?);
            }
            NbtValue::LongArray(out)
        }
        _ => return Err(ReadError),
    })
}

// Liest ein Wurzel-Tag im Netzwerk-Format (Typ-Byte, KEIN Name, dann Payload). None fuer
// TAG_End (der leere/"nicht vorhandene" Fall, den Vanilla als null behandelt).
pub fn read_root(r: &mut PacketReader) -> Result<Option<NbtValue>, ReadError> {
    let tag_type = r.read_u8()?;
    if tag_type == TAG_END {
        return Ok(None);
    }
    Ok(Some(read_payload(r, tag_type)?))
}

// Gegenstueck zu read_root: ein Wurzel-Tag im Netzwerk-Format (Typ-Byte, kein Name,
// Payload). Gebraucht fuer die Werte, die ein Dialog an den Server zurueckschickt.
pub fn write_root(w: &mut crate::buffer::PacketWriter, value: &NbtValue) {
    w.write_u8(tag_type(value));
    write_payload(w, value);
}

fn tag_type(value: &NbtValue) -> u8 {
    match value {
        NbtValue::Byte(_) => TAG_BYTE,
        NbtValue::Short(_) => TAG_SHORT,
        NbtValue::Int(_) => TAG_INT,
        NbtValue::Long(_) => TAG_LONG,
        NbtValue::Float(_) => TAG_FLOAT,
        NbtValue::Double(_) => TAG_DOUBLE,
        NbtValue::ByteArray(_) => TAG_BYTE_ARRAY,
        NbtValue::String(_) => TAG_STRING,
        NbtValue::List(_) => TAG_LIST,
        NbtValue::Compound(_) => TAG_COMPOUND,
        NbtValue::IntArray(_) => TAG_INT_ARRAY,
        NbtValue::LongArray(_) => TAG_LONG_ARRAY,
    }
}

// Javas DataOutput.writeUTF: u16-Laenge, Modified UTF-8 (NUL als 0xC0 0x80, Zeichen
// ausserhalb der Grundebene als zwei Surrogate zu je drei Bytes).
fn write_nbt_string(w: &mut crate::buffer::PacketWriter, s: &str) {
    let mut bytes = Vec::with_capacity(s.len());
    for unit in s.encode_utf16() {
        match unit {
            0x0001..=0x007F => bytes.push(unit as u8),
            0x0000 | 0x0080..=0x07FF => {
                bytes.push(0xC0 | (unit >> 6) as u8);
                bytes.push(0x80 | (unit & 0x3F) as u8);
            }
            _ => {
                bytes.push(0xE0 | (unit >> 12) as u8);
                bytes.push(0x80 | ((unit >> 6) & 0x3F) as u8);
                bytes.push(0x80 | (unit & 0x3F) as u8);
            }
        }
    }
    bytes.truncate(u16::MAX as usize);
    w.write_u16(bytes.len() as u16);
    w.write_raw_bytes(&bytes);
}

fn write_payload(w: &mut crate::buffer::PacketWriter, value: &NbtValue) {
    match value {
        NbtValue::Byte(v) => {
            w.write_u8(*v as u8);
        }
        NbtValue::Short(v) => {
            w.write_u16(*v as u16);
        }
        NbtValue::Int(v) => {
            w.write_i32(*v);
        }
        NbtValue::Long(v) => {
            w.write_i64(*v);
        }
        NbtValue::Float(v) => {
            w.write_f32(*v);
        }
        NbtValue::Double(v) => {
            w.write_f64(*v);
        }
        NbtValue::ByteArray(v) => {
            w.write_i32(v.len() as i32);
            for b in v {
                w.write_u8(*b as u8);
            }
        }
        NbtValue::String(s) => write_nbt_string(w, s),
        NbtValue::List(items) => {
            // Gemischte Listen schreibt Vanilla als Compounds mit dem Schluessel "" (s.
            // unwrap_list_element); einheitliche wie gewohnt.
            let typ = items.first().map_or(TAG_END, tag_type);
            if items.iter().all(|v| tag_type(v) == typ) {
                w.write_u8(typ);
                w.write_i32(items.len() as i32);
                for v in items {
                    write_payload(w, v);
                }
            } else {
                w.write_u8(TAG_COMPOUND);
                w.write_i32(items.len() as i32);
                for v in items {
                    match v {
                        NbtValue::Compound(_) => write_payload(w, v),
                        other => {
                            w.write_u8(tag_type(other));
                            write_nbt_string(w, "");
                            write_payload(w, other);
                            w.write_u8(TAG_END);
                        }
                    }
                }
            }
        }
        NbtValue::Compound(map) => {
            // Feste Reihenfolge, damit gleiche Werte gleiche Bytes ergeben.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                let v = &map[k];
                w.write_u8(tag_type(v));
                write_nbt_string(w, k);
                write_payload(w, v);
            }
            w.write_u8(TAG_END);
        }
        NbtValue::IntArray(v) => {
            w.write_i32(v.len() as i32);
            for i in v {
                w.write_i32(*i);
            }
        }
        NbtValue::LongArray(v) => {
            w.write_i32(v.len() as i32);
            for i in v {
                w.write_i64(*i);
            }
        }
    }
}

// --- Farbige Text-Components ----------------------------------------------------------------
//
// Ein Text-Component traegt neben dem Wortlaut auch eine Farbe, und bei Hologrammen
// (Namensschilder, Text-Display-Entities) ist die Farbe kein Zierrat, sondern die halbe
// Information: Server bauen damit Ueberschriften, Preise und Zustaende. `text_to_plain_string`
// wirft sie weg -- fuer den Chat war das bislang egal, fuer ein Hologramm nicht.
//
// `text_to_spans` zerlegt denselben Baum stattdessen in ABSCHNITTE aus (Wortlaut, Farbe).
// Zwei Farbquellen werden zusammengefuehrt, weil beide in freier Wildbahn vorkommen:
//   - das moderne "color"-Feld eines Components (Name oder "#rrggbb"), vererbt an "extra";
//   - die alten Paragraphen-Codes MITTEN im Wortlaut, die viele Plugins weiterhin schreiben.
// Reine Stil-Codes (fett/kursiv/...) werden gelesen und verworfen -- der Bitmap-Font dieses
// Clients hat keine Schnitte dafuer, und ein sichtbares Steuerzeichen waere schlechter als
// nichts.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextSpan {
    pub text: String,
    // 0xRRGGBB. None = keine eigene Farbe, der Aufrufer nimmt seine Vorgabe.
    pub color: Option<u32>,
    // Ziel eines Klicks (ClickEvent "open_url"). Wird wie die Farbe an "extra" vererbt.
    pub link: Option<String>,
    // Die uebrigen Klick-Aktionen, der Hover-Tooltip und das Shift-Klick-Einfuegen
    // (s. SpanAction). Hinter einem Arc, weil Abschnitte beim Umbrechen oft geklont werden.
    pub action: Option<Arc<SpanAction>>,
    // Schnitte und Linien (STYLE_*): fett, kursiv, unterstrichen, durchgestrichen,
    // verschleiert. Wie die Farbe an "extra" vererbt.
    pub style: u8,
    // Style.shadow_color: Farbe des Textschattens als ARGB, vom Server gesetzt. None = der
    // uebliche (ein Viertel der Textfarbe); Alpha 0 = gar kein Schatten. Vererbt wie die Farbe.
    pub shadow: Option<u32>,
}

pub const STYLE_BOLD: u8 = 1;
pub const STYLE_ITALIC: u8 = 2;
pub const STYLE_UNDERLINED: u8 = 4;
pub const STYLE_STRIKETHROUGH: u8 = 8;
pub const STYLE_OBFUSCATED: u8 = 16;

// Die Component-Felder zu den Bits (Style.CODEC: "bold", "italic", ...).
const STYLE_FIELDS: [(&str, u8); 5] =
    [("bold", STYLE_BOLD), ("italic", STYLE_ITALIC), ("underlined", STYLE_UNDERLINED), ("strikethrough", STYLE_STRIKETHROUGH), ("obfuscated", STYLE_OBFUSCATED)];

// Was ein Klick auf einen Abschnitt ausser "open_url" (das ist TextSpan::link) tun kann --
// die Faelle aus Vanillas Screen.handleClickEvent, die ein Server in eine Chatzeile legen
// kann. Nicht dabei: open_file (nur fuer clientseitig erzeugten Text, "userDefinable" =
// false), change_page (nur Buecher), show_dialog und custom (Dialog-System, das dieser
// Client nicht hat).
#[derive(Clone, Debug, PartialEq)]
pub enum ClickAction {
    RunCommand(String),
    SuggestCommand(String),
    CopyToClipboard(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpanAction {
    pub click: Option<ClickAction>,
    // Tooltip beim Drueberfahren (HoverEvent show_text / show_item). "\n" trennt Zeilen.
    pub hover: Option<Vec<TextSpan>>,
    // Style.insertion: Shift+Klick fuegt diesen Text in die Chat-Eingabe ein.
    pub insertion: Option<String>,
}

impl SpanAction {
    fn is_empty(&self) -> bool {
        self.click.is_none() && self.hover.is_none() && self.insertion.is_none()
    }
}

// Style, der an Kinder vererbt wird -- Farbe, Link, die Aktionen und die Schrift.
#[derive(Clone, Default)]
struct Style {
    color: Option<u32>,
    link: Option<String>,
    action: Option<Arc<SpanAction>>,
    // Schrift-ID ("peacock:hud"), None = Standardschrift (s. font_char).
    font: Option<Arc<str>>,
    // STYLE_*-Bits.
    style: u8,
    // s. TextSpan::shadow.
    shadow: Option<u32>,
}

// Ein Wahrheitswert aus einem Component: NBT-Byte (Netz) oder "true"/"false" (JSON).
fn nbt_bool(value: &NbtValue) -> Option<bool> {
    match value {
        NbtValue::Byte(b) => Some(*b != 0),
        NbtValue::String(s) => s.parse().ok(),
        NbtValue::Int(i) => Some(*i != 0),
        _ => None,
    }
}

// Style.shadow_color: ARGB als Zahl (im JSON eine Zahl, die hier als Text ankommt) oder als
// Liste [r, g, b, a] mit Anteilen 0..1.
fn shadow_color(value: &NbtValue) -> Option<u32> {
    let anteil = |v: &NbtValue| match v {
        NbtValue::Float(f) => Some(*f),
        NbtValue::Double(d) => Some(*d as f32),
        NbtValue::String(s) => s.parse().ok(),
        _ => None,
    };
    match value.get("shadow_color")? {
        NbtValue::Int(i) => Some(*i as u32),
        NbtValue::Long(l) => Some(*l as u32),
        NbtValue::String(s) => s.parse::<i64>().ok().map(|v| v as u32),
        NbtValue::List(l) if l.len() == 4 => {
            let mut argb = 0u32;
            for (i, v) in l.iter().enumerate() {
                let byte = (anteil(v)?.clamp(0.0, 1.0) * 255.0).round() as u32;
                // Reihenfolge r, g, b, a -> Bits 16, 8, 0, 24.
                argb |= byte << [16, 8, 0, 24][i];
            }
            Some(argb)
        }
        _ => None,
    }
}

// Die Stil-Bits eines Components: ausdruecklich gesetzte Felder ueberschreiben die geerbten
// (auch ein "bold": false unter einem fetten Eltern-Component).
fn own_style_bits(value: &NbtValue, inherited: u8) -> u8 {
    let mut bits = inherited;
    for (feld, bit) in STYLE_FIELDS {
        match value.get(feld).and_then(nbt_bool) {
            Some(true) => bits |= bit,
            Some(false) => bits &= !bit,
            None => {}
        }
    }
    bits
}

// --- Zeichen aus eigenen Schriften ---------------------------------------------------------
//
// Ein Component kann eine andere Schrift waehlen ("font": "peacock:hud"); Server bauen damit
// ihr HUD (s. assets::PackFonts). Dasselbe Zeichen U+E001 ist in jeder Schrift ein anderes
// Bild. Statt die Schrift durch Abschnitte, Umbruch, Messen und Zeichnen zu reichen, bekommt
// jedes Paar (Schrift, Zeichen) beim Lesen einen eigenen Codepunkt aus der Supplementary
// Private Use Area-A/B (U+F0000 ...). Der Text traegt die Schrift damit in sich; die
// Schriftatlanten fragen ueber font_char ab, welcher Codepunkt zu welchem Bild gehoert, und
// ueber font_char_origin, welches Zeichen gemeint war (Rueckfall auf die Standardschrift).
//
// Die Vergabe ist prozessweit und haengt nicht davon ab, ob das Pack schon geladen ist -- Text,
// der vor dem Pack ankam, zeigt nach dem Laden ohne neues Paket die richtigen Bilder.

pub const FONT_CHAR_FIRST: u32 = 0xF0000;
const FONT_CHAR_LAST: u32 = 0x10FFFD;

#[derive(Default)]
struct FontChars {
    fonts: Vec<Arc<str>>,
    vorwaerts: HashMap<(u16, char), char>,
    zurueck: HashMap<char, (u16, char)>,
    naechster: u32,
}

static FONT_CHARS: std::sync::Mutex<Option<FontChars>> = std::sync::Mutex::new(None);

// Standardschrift? Dann bleibt das Zeichen, wie es ist.
fn is_default_font(font: &str) -> bool {
    matches!(font, "minecraft:default" | "default")
}

// Der Codepunkt fuer `c` in Schrift `font` ("ns:pfad"). Zeilenumbrueche bleiben, damit
// mehrzeiliger Text weiter umbricht.
pub fn font_char(font: &str, c: char) -> char {
    if is_default_font(font) || c == '\n' {
        return c;
    }
    let mut lock = FONT_CHARS.lock().unwrap_or_else(|e| e.into_inner());
    let fc = lock.get_or_insert_with(FontChars::default);
    let idx = match fc.fonts.iter().position(|f| &**f == font) {
        Some(i) => i as u16,
        None => {
            fc.fonts.push(Arc::from(font));
            (fc.fonts.len() - 1) as u16
        }
    };
    if let Some(&m) = fc.vorwaerts.get(&(idx, c)) {
        return m;
    }
    let nummer = FONT_CHAR_FIRST + fc.naechster;
    // Der Bereich (gut 130000 Paare) reicht fuer jedes Pack; voll -> Standardschrift.
    let Some(neu) = (nummer <= FONT_CHAR_LAST).then(|| char::from_u32(nummer)).flatten() else { return c };
    fc.naechster += 1;
    fc.vorwaerts.insert((idx, c), neu);
    fc.zurueck.insert(neu, (idx, c));
    neu
}

// Zu einem vergebenen Codepunkt: (Schrift, gemeintes Zeichen).
pub fn font_char_origin(c: char) -> Option<(Arc<str>, char)> {
    if (c as u32) < FONT_CHAR_FIRST {
        return None;
    }
    let lock = FONT_CHARS.lock().unwrap_or_else(|e| e.into_inner());
    let fc = lock.as_ref()?;
    let &(idx, orig) = fc.zurueck.get(&c)?;
    Some((fc.fonts[idx as usize].clone(), orig))
}

// Die Aktionen eines Components, feldweise mit denen des Eltern zusammengelegt -- wie
// Vanillas Style.withParent: was das Kind selbst setzt, gewinnt, der Rest wird geerbt.
fn own_action(value: &NbtValue, inherited: Option<&Arc<SpanAction>>) -> Option<Arc<SpanAction>> {
    let eigen = SpanAction {
        click: click_action(value),
        hover: hover_tooltip(value),
        insertion: value.get("insertion").and_then(NbtValue::as_str).map(str::to_string),
    };
    if eigen.is_empty() {
        return inherited.cloned();
    }
    let erbe = inherited.map(|a| a.as_ref());
    Some(Arc::new(SpanAction {
        click: eigen.click.or_else(|| erbe.and_then(|a| a.click.clone())),
        hover: eigen.hover.or_else(|| erbe.and_then(|a| a.hover.clone())),
        insertion: eigen.insertion.or_else(|| erbe.and_then(|a| a.insertion.clone())),
    }))
}

// ClickEvent ausser "open_url" (s. open_url). Neues Format (ab 1.21.5): "click_event" mit
// "command"/"value" je nach Aktion; altes: "clickEvent" und immer "value".
fn click_action(value: &NbtValue) -> Option<ClickAction> {
    for (feld, neu) in [("click_event", true), ("clickEvent", false)] {
        let Some(ev) = value.get(feld) else { continue };
        let lies = |schluessel_neu: &str| ev.get(if neu { schluessel_neu } else { "value" }).and_then(NbtValue::as_str).map(str::to_string);
        return match ev.get("action").and_then(NbtValue::as_str)? {
            "run_command" => lies("command").map(ClickAction::RunCommand),
            "suggest_command" => lies("command").map(ClickAction::SuggestCommand),
            "copy_to_clipboard" => lies("value").map(ClickAction::CopyToClipboard),
            _ => None,
        };
    }
    None
}

// HoverEvent als Tooltip-Text. Neues Format: "hover_event" mit "value" (show_text) bzw. den
// Item-Feldern direkt im Event (show_item); altes: "hoverEvent" mit "contents" (oder "value").
//
// show_entity zeigt Vanilla NUR mit erweiterten Tooltips (F3+H, DrawContext.drawHoverEvent
// prueft die Option advancedItemTooltips) -- standardmaessig also nichts.
fn hover_tooltip(value: &NbtValue) -> Option<Vec<TextSpan>> {
    for feld in ["hover_event", "hoverEvent"] {
        let Some(ev) = value.get(feld) else { continue };
        return match ev.get("action").and_then(NbtValue::as_str)? {
            "show_text" => ev.get("value").or_else(|| ev.get("contents")).map(text_to_spans),
            "show_item" => Some(item_tooltip(ev.get("contents").unwrap_or(ev))),
            _ => None,
        };
    }
    None
}

// Tooltip eines Items aus show_item: Name, darunter die Lore. Der Name ist custom_name,
// sonst item_name, sonst die Item-ID lesbar gemacht -- Uebersetzungen hat dieser Client
// nicht (dieselbe Notloesung wie im Inventar). Lore steht in Vanilla dunkelviolett.
fn item_tooltip(item: &NbtValue) -> Vec<TextSpan> {
    const LORE_FARBE: u32 = 0xAA00AA;
    // Altes Format erlaubt statt des Compounds die blanke ID.
    let id = item.as_str().or_else(|| item.get("id").and_then(NbtValue::as_str)).unwrap_or("?");
    let komponente = |name: &str| {
        let c = item.get("components")?;
        c.get(&format!("minecraft:{name}")).or_else(|| c.get(name))
    };
    let mut out = match komponente("custom_name").or_else(|| komponente("item_name")) {
        Some(name) => text_to_spans(name),
        None => vec![TextSpan { text: id.strip_prefix("minecraft:").unwrap_or(id).replace('_', " "), ..Default::default() }],
    };
    if let Some(NbtValue::List(zeilen)) = komponente("lore") {
        for zeile in zeilen {
            out.push(TextSpan { text: "\n".to_string(), ..Default::default() });
            collect_spans(zeile, &Style { color: Some(LORE_FARBE), ..Default::default() }, &mut out);
        }
    }
    out
}

// ClickEvent "open_url" eines Components. Seit 1.21.5 heisst das Feld "click_event" und die
// Adresse "url"; aeltere Server (und JSON aus dem Login) schreiben "clickEvent"/"value".
fn open_url(value: &NbtValue) -> Option<String> {
    for (feld, schluessel) in [("click_event", "url"), ("clickEvent", "value")] {
        let Some(ev) = value.get(feld) else { continue };
        if ev.get("action").and_then(NbtValue::as_str) == Some("open_url") {
            if let Some(url) = ev.get(schluessel).and_then(NbtValue::as_str) {
                return Some(url.to_string());
            }
        }
    }
    None
}

// Die 16 Vanilla-Farben (Formatting.class, dieselben Werte, die auch die Paragraphen-Codes
// meinen). Reihenfolge = Code 0..9,a..f.
const LEGACY_COLORS: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xFFAA00, 0xAAAAAA, 0x555555, 0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555,
    0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

const COLOR_NAMES: [(&str, u32); 16] = [
    ("black", 0x000000),
    ("dark_blue", 0x0000AA),
    ("dark_green", 0x00AA00),
    ("dark_aqua", 0x00AAAA),
    ("dark_red", 0xAA0000),
    ("dark_purple", 0xAA00AA),
    ("gold", 0xFFAA00),
    ("gray", 0xAAAAAA),
    ("dark_gray", 0x555555),
    ("blue", 0x5555FF),
    ("green", 0x55FF55),
    ("aqua", 0x55FFFF),
    ("red", 0xFF5555),
    ("light_purple", 0xFF55FF),
    ("yellow", 0xFFFF55),
    ("white", 0xFFFFFF),
];

// "hud" -> "minecraft:hud"; mit Namensraum unveraendert.
fn full_font_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

// "gold" oder "#1affc3" -> 0xRRGGBB.
fn parse_color(name: &str) -> Option<u32> {
    if let Some(hex) = name.strip_prefix('#') {
        return u32::from_str_radix(hex, 16).ok().map(|v| v & 0x00FF_FFFF);
    }
    COLOR_NAMES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

// Einen Text-Component-Baum in farbige Abschnitte zerlegen.
// Ein Text-Component im JSON-Format in dieselben Abschnitte zerlegen.
//
// Der Login-State ist der einzige Ort, an dem Vanilla noch JSON schickt (LoginDisconnect) --
// alles danach ist NBT. Statt eines zweiten Auswerters wird das JSON in dieselbe
// NbtValue-Form gebracht und durch text_to_spans geschickt: eine Regel, ein Ergebnis.
//
// Frueherer Fehler: im Log stand die rohe Zeichenkette
// `{"italic":false,"color":"red","extra":[...],"text":"You are already online!"}` statt der
// Meldung -- lesbar war sie nur mit Muehe.
pub fn json_text_to_spans(json: &str) -> Vec<TextSpan> {
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(value) => text_to_spans(&json_to_nbt(&value)),
        // Kein gueltiges JSON: dann ist die Zeichenkette selbst die Meldung (aeltere Server
        // schicken gelegentlich blanken Text).
        Err(_) => vec![TextSpan { text: json.to_string(), ..Default::default() }],
    }
}

fn json_to_nbt(value: &serde_json::Value) -> NbtValue {
    match value {
        serde_json::Value::String(s) => NbtValue::String(s.clone()),
        serde_json::Value::Array(items) => NbtValue::List(items.iter().map(json_to_nbt).collect()),
        serde_json::Value::Object(map) => NbtValue::Compound(map.iter().map(|(k, v)| (k.clone(), json_to_nbt(v))).collect()),
        // Zahlen und Wahrheitswerte kommen in Text-Components nur als Stil-Angaben vor
        // ("bold": true, "color": 16711680) -- als String abgelegt bleiben sie lesbar, und
        // text_to_spans greift ohnehin nur auf die Felder zu, die es kennt.
        serde_json::Value::Bool(b) => NbtValue::String(b.to_string()),
        serde_json::Value::Number(n) => NbtValue::String(n.to_string()),
        serde_json::Value::Null => NbtValue::String(String::new()),
    }
}

// Die Farbe eines nackten Style-Compounds (StyledNumberFormat traegt nur einen Style, keinen
// Text).
pub fn style_color(value: &NbtValue) -> Option<u32> {
    value.get("color").and_then(NbtValue::as_str).and_then(parse_color)
}

pub fn text_to_spans(value: &NbtValue) -> Vec<TextSpan> {
    let mut out = Vec::new();
    collect_spans(value, &Style::default(), &mut out);
    out
}

// `inherited` ist der Style des Eltern-Components: Vanilla vererbt ihn an die "extra"-Kinder
// und an die Argumente von "translate".
fn collect_spans(value: &NbtValue, inherited: &Style, out: &mut Vec<TextSpan>) {
    match value {
        NbtValue::String(s) => push_legacy_styled(s, inherited, out),
        NbtValue::List(items) => {
            for item in items {
                collect_spans(item, inherited, out);
            }
        }
        NbtValue::Compound(_) => {
            let own = Style {
                color: value.get("color").and_then(NbtValue::as_str).and_then(parse_color).or(inherited.color),
                link: open_url(value).or_else(|| inherited.link.clone()),
                action: own_action(value, inherited.action.as_ref()),
                font: value.get("font").and_then(NbtValue::as_str).map(|f| Arc::from(full_font_id(f))).or_else(|| inherited.font.clone()),
                style: own_style_bits(value, inherited.style),
                shadow: shadow_color(value).or(inherited.shadow),
            };
            if let Some(s) = value.get("text").and_then(NbtValue::as_str) {
                push_legacy_styled(s, &own, out);
            } else if let Some(key) = value.get("translate").and_then(NbtValue::as_str) {
                let args: &[NbtValue] = match value.get("with") {
                    Some(NbtValue::List(args)) => args,
                    _ => &[],
                };
                // TranslatableTextContent: Language.get(key, fallback) liefert ohne Eintrag den
                // SCHLUESSEL selbst, und der wird genauso mit den Argumenten gefuellt. Ein
                // Server, der als Schluessel "%s" schickt (gesehen auf eu.applepvp.com), zeigt
                // so den Text statt "%s: Name: Nachricht".
                let pattern = translation_pattern(value, key).unwrap_or_else(|| key.to_string());
                for teil in split_translation(&pattern) {
                    match teil {
                        TranslationPart::Literal(s) => push_legacy_styled(&s, &own, out),
                        TranslationPart::Arg(i) => {
                            if let Some(arg) = args.get(i) {
                                collect_arg_spans(arg, &own, out);
                            }
                        }
                    }
                }
            }
            if let Some(NbtValue::List(extra)) = value.get("extra") {
                for e in extra {
                    collect_spans(e, &own, out);
                }
            }
        }
        _ => {}
    }
}

// --- Uebersetzung ("translate"-Components) --------------------------------------------------
//
// Vanilla schickt viele Texte nur als Schluessel ("enchantment.minecraft.sharpness",
// "death.attack.explosion.player") und uebersetzt sie erst beim Anzeigen mit der Sprachdatei.
// Ohne diese Tabelle standen die rohen Schluessel im Tooltip und im Chat (etwa
// "death.attack.explosion.player: <Spieler>, <Spieler>").
//
// Die Tabelle ist prozessweit, weil Text an sehr vielen Stellen umgewandelt wird (Chat,
// Titel, Scoreboard, Hologramme, Items) und keine davon einen Kontext durchreicht. Leer, bis
// die App sie setzt -- dann gilt das alte Verhalten (Schluessel roh anzeigen).
//
// Je Bereich eine eigene Tabelle (Einstellungen -> Sprache): Itemnamen koennen in einer
// anderen Sprache stehen als Chat und Effekte. Welcher Bereich gerade uebersetzt, steht
// threadlokal (s. with_lang_area); ohne Angabe gilt LangArea::Text.
static LANGUAGES: std::sync::RwLock<[Option<Arc<HashMap<String, String>>>; LangArea::COUNT]> = std::sync::RwLock::new([None, None, None]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangArea {
    // Chat, Titel, Scoreboard, Hologramme -- alles, was der Server als Text schickt.
    Text = 0,
    // Item- und Blocknamen, Verzauberungen und Traenke im Tooltip.
    Items = 1,
    // Effektnamen im HUD und im Inventar.
    Effects = 2,
}

impl LangArea {
    pub const COUNT: usize = 3;
}

thread_local! {
    static AREA: std::cell::Cell<LangArea> = const { std::cell::Cell::new(LangArea::Text) };
}

pub fn set_language(table: HashMap<String, String>) {
    set_area_language(LangArea::Text, Arc::new(table));
}

pub fn set_area_language(area: LangArea, table: Arc<HashMap<String, String>>) {
    if let Ok(mut guard) = LANGUAGES.write() {
        guard[area as usize] = Some(table);
    }
}

// `f` uebersetzt mit der Tabelle von `area` (verschachtelbar, danach gilt wieder der alte).
pub fn with_lang_area<R>(area: LangArea, f: impl FnOnce() -> R) -> R {
    let vorher = AREA.with(|a| a.replace(area));
    let r = f();
    AREA.with(|a| a.set(vorher));
    r
}

// Eine Sprachdatei (assets/<ns>/lang/<sprache>.json: flaches Objekt Schluessel -> Text).
pub fn parse_language_json(bytes: &[u8]) -> Option<HashMap<String, String>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    Some(value.as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
}

pub fn translate(key: &str) -> Option<String> {
    let area = AREA.with(|a| a.get()) as usize;
    let guard = LANGUAGES.read().ok()?;
    guard[area].as_ref().or(guard[LangArea::Text as usize].as_ref())?.get(key).cloned()
}

// TranslatableTextContent: erst die Sprachdatei, sonst das optionale "fallback" des
// Components. None = weder noch.
fn translation_pattern(value: &NbtValue, key: &str) -> Option<String> {
    translate(key).or_else(|| value.get("fallback").and_then(NbtValue::as_str).map(str::to_string))
}

#[derive(Debug, PartialEq)]
enum TranslationPart {
    Literal(String),
    // 0-basierter Index in "with".
    Arg(usize),
}

// Vanillas Muster (TranslatableTextContent.ARG_FORMAT): `%s` nimmt das naechste Argument,
// `%<n>$s` das n-te (1-basiert), `%%` ist ein Prozentzeichen. Alles andere bleibt Text.
fn split_translation(pattern: &str) -> Vec<TranslationPart> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut next = 0usize;
    let mut chars = pattern.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '%' {
            text.push(c);
            continue;
        }
        let rest = &pattern[i + 1..];
        if rest.starts_with('%') {
            chars.next();
            text.push('%');
        } else if rest.starts_with('s') {
            chars.next();
            out.push(TranslationPart::Literal(std::mem::take(&mut text)));
            out.push(TranslationPart::Arg(next));
            next += 1;
        } else if let Some((ziffern, _)) = rest.split_once("$s").filter(|(z, _)| !z.is_empty() && z.bytes().all(|b| b.is_ascii_digit())) {
            for _ in 0..ziffern.len() + 2 {
                chars.next();
            }
            out.push(TranslationPart::Literal(std::mem::take(&mut text)));
            out.push(TranslationPart::Arg(ziffern.parse::<usize>().unwrap_or(1).saturating_sub(1)));
        } else {
            text.push('%');
        }
    }
    out.push(TranslationPart::Literal(text));
    out.retain(|p| !matches!(p, TranslationPart::Literal(s) if s.is_empty()));
    out
}

// Ein Argument von "with": meist ein Text-Component, aber Zahlen stehen auch direkt da
// (z.B. die Stufe in "commands.enchant.success.single").
fn collect_arg_spans(arg: &NbtValue, style: &Style, out: &mut Vec<TextSpan>) {
    match arg {
        NbtValue::Byte(v) => push_legacy_styled(&v.to_string(), style, out),
        NbtValue::Short(v) => push_legacy_styled(&v.to_string(), style, out),
        NbtValue::Int(v) => push_legacy_styled(&v.to_string(), style, out),
        NbtValue::Long(v) => push_legacy_styled(&v.to_string(), style, out),
        NbtValue::Float(v) => push_legacy_styled(&v.to_string(), style, out),
        NbtValue::Double(v) => push_legacy_styled(&v.to_string(), style, out),
        _ => collect_spans(arg, style, out),
    }
}

// push_legacy mit Link und Aktionen: sie gelten fuer alles, was dieser Wortlaut ergibt.
// Verschmolzen wird nur mit einem Vorgaenger, der in Farbe, Link UND Aktionen gleich ist --
// sonst liefe ein Tooltip oder Klick auf den Nachbartext ueber.
fn push_legacy_styled(text: &str, style: &Style, out: &mut Vec<TextSpan>) {
    let mut neu = Vec::new();
    push_legacy_with(text, style.color, style.style, style.shadow, &mut neu);
    for mut span in neu {
        if let Some(font) = style.font.as_deref().filter(|f| !is_default_font(f)) {
            span.text = span.text.chars().map(|c| font_char(font, c)).collect();
        }
        span.link = style.link.clone();
        span.action = style.action.clone();
        match out.last_mut() {
            Some(l) if l.color == span.color && l.style == span.style && l.shadow == span.shadow && l.link == span.link && l.action == span.action => l.text.push_str(&span.text),
            _ => out.push(span),
        }
    }
}

// Die Farbe eines Formatting-Index (dieselbe Reihenfolge wie die Paragraphen-Codes 0..9,a..f).
// Werte ausserhalb 0..15 heissen "keine eigene Farbe" -- Vanillas RESET/None.
pub fn formatting_color(index: i32) -> Option<u32> {
    usize::try_from(index).ok().and_then(|i| LEGACY_COLORS.get(i).copied())
}

// Ein Stueck Text mit Paragraphen-Codes in Abschnitte zerlegen -- fuer alles, was NICHT als
// Component ankommt, sondern als blanke Zeichenkette mit eingebetteten Codes (Team-Prefix
// und -Suffix sind der Hauptfall).
pub fn legacy_to_spans(text: &str, inherited: Option<u32>) -> Vec<TextSpan> {
    let mut out = Vec::new();
    push_legacy(text, inherited, &mut out);
    out
}

// Einen Wortlaut anhaengen und dabei die alten Paragraphen-Codes darin aufloesen. Ein "r"
// (Reset) faellt auf die ERERBTE Farbe zurueck, nicht auf Weiss -- genau wie in Vanilla, wo
// der Reset den Style des Eltern-Components wiederherstellt.
fn push_legacy(text: &str, inherited: Option<u32>, out: &mut Vec<TextSpan>) {
    push_legacy_with(text, inherited, 0, None, out);
}

// Clientinterne Codes fuer den Schatten (s. TextSpan::shadow): Scoreboard und Teams reichen
// ihre Texte als Zeichenkette mit Paragraphen-Codes weiter (s. spans_to_legacy), und dafuer
// gibt es keinen Vanilla-Code. Nichtzeichen hinter dem §, die kein Server schickt; nach
// SHADOW_SET folgen acht Hex-Ziffern ARGB, SHADOW_CLEAR heisst "der uebliche Schatten".
const SHADOW_SET: char = '\u{fdd0}';
const SHADOW_CLEAR: char = '\u{fdd1}';

// Mit geerbten Stil-Bits. Wie in Vanillas TextVisitFactory: ein Farbcode setzt die Schnitte
// zurueck (nur die Farbe bleibt), k/l/m/n/o schalten einen dazu, r stellt den geerbten Stil
// wieder her. Der Schatten bleibt ueber Farbcodes hinweg (withExclusiveFormatting laesst ihn
// stehen), nur r stellt den geerbten wieder her.
fn push_legacy_with(text: &str, inherited: Option<u32>, inherited_style: u8, inherited_shadow: Option<u32>, out: &mut Vec<TextSpan>) {
    if text.is_empty() {
        return;
    }
    let mut color = inherited;
    let mut style = inherited_style;
    let mut shadow = inherited_shadow;
    let mut buf = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{a7}' {
            buf.push(c);
            continue;
        }
        let Some(code) = chars.next() else { break };
        if code == SHADOW_SET || code == SHADOW_CLEAR {
            let neu = if code == SHADOW_SET {
                let hex: String = chars.clone().take(8).collect();
                let Some(v) = (hex.len() == 8).then(|| u32::from_str_radix(&hex, 16).ok()).flatten() else { continue };
                chars.nth(7);
                Some(v)
            } else {
                None
            };
            if neu != shadow {
                push_span(&mut buf, color, style, shadow, out);
                shadow = neu;
            }
            continue;
        }
        let neu = match code.to_ascii_lowercase() {
            // Hex-Farbe im BungeeCord-Format: §x§R§R§G§G§B§B (s. spans_to_legacy).
            'x' => {
                let mut rest = chars.clone();
                let mut hex = String::new();
                for _ in 0..6 {
                    match (rest.next(), rest.next()) {
                        (Some('\u{a7}'), Some(h)) if h.is_ascii_hexdigit() => hex.push(h),
                        _ => break,
                    }
                }
                if hex.len() != 6 {
                    continue;
                }
                chars = rest;
                u32::from_str_radix(&hex, 16).ok()
            }
            'r' => {
                if inherited != color || inherited_style != style || inherited_shadow != shadow {
                    push_span(&mut buf, color, style, shadow, out);
                    color = inherited;
                    style = inherited_style;
                    shadow = inherited_shadow;
                }
                continue;
            }
            d @ '0'..='9' => Some(LEGACY_COLORS[d as usize - '0' as usize]),
            h @ 'a'..='f' => Some(LEGACY_COLORS[10 + (h as usize - 'a' as usize)]),
            k @ ('k' | 'l' | 'm' | 'n' | 'o') => {
                let bit = match k {
                    'k' => STYLE_OBFUSCATED,
                    'l' => STYLE_BOLD,
                    'm' => STYLE_STRIKETHROUGH,
                    'n' => STYLE_UNDERLINED,
                    _ => STYLE_ITALIC,
                };
                if style & bit == 0 {
                    push_span(&mut buf, color, style, shadow, out);
                    style |= bit;
                }
                continue;
            }
            _ => continue,
        };
        // Ein Farbcode: neue Farbe, Schnitte aus.
        if neu != color || style != 0 {
            push_span(&mut buf, color, style, shadow, out);
            color = neu;
            style = 0;
        }
    }
    push_span(&mut buf, color, style, shadow, out);
}

// Die Umkehrung von legacy_to_spans: Abschnitte als Zeichenkette mit Paragraphen-Codes.
// Fuer Texte, die der Client als Zeichenkette weiterreicht und erst spaeter zusammensetzt
// (Team-Prefix + Name + Suffix, Seitenleisten-Zeilen) -- so ueberleben ihre Farben den Weg.
// Farben ausserhalb der 16 klassischen kommen als §x-Hexfolge; ohne eigene Farbe steht §r
// (= die geerbte Farbe, s. push_legacy).
pub fn spans_to_legacy(spans: &[TextSpan]) -> String {
    let mut out = String::new();
    // Der Schatten, der gerade gilt: §r setzt ihn zurueck, Farbcodes lassen ihn stehen.
    let mut schatten = None;
    for span in spans {
        match span.color {
            None => {
                out.push_str("\u{a7}r");
                schatten = None;
            }
            Some(c) => match LEGACY_COLORS.iter().position(|&l| l == c) {
                Some(i) => {
                    out.push('\u{a7}');
                    out.push(char::from_digit(i as u32, 16).unwrap_or('f'));
                }
                None => {
                    out.push_str("\u{a7}x");
                    for h in format!("{:06x}", c & 0xFF_FFFF).chars() {
                        out.push('\u{a7}');
                        out.push(h);
                    }
                }
            },
        }
        // Nach der Farbe, denn ein Farbcode setzt die Schnitte zurueck (s. push_legacy_with).
        for (code, bit) in [('k', STYLE_OBFUSCATED), ('l', STYLE_BOLD), ('m', STYLE_STRIKETHROUGH), ('n', STYLE_UNDERLINED), ('o', STYLE_ITALIC)] {
            if span.style & bit != 0 {
                out.push('\u{a7}');
                out.push(code);
            }
        }
        if span.shadow != schatten {
            out.push('\u{a7}');
            match span.shadow {
                Some(argb) => {
                    out.push(SHADOW_SET);
                    out.push_str(&format!("{argb:08x}"));
                }
                None => out.push(SHADOW_CLEAR),
            }
            schatten = span.shadow;
        }
        out.push_str(&span.text);
    }
    // Nicht in angehaengten Text weiterreichen (Team-Prefix + Name + Suffix sind in Vanilla
    // getrennte Components).
    if schatten.is_some() {
        out.push('\u{a7}');
        out.push(SHADOW_CLEAR);
    }
    out
}

fn push_span(buf: &mut String, color: Option<u32>, style: u8, shadow: Option<u32>, out: &mut Vec<TextSpan>) {
    if buf.is_empty() {
        return;
    }
    // Gleich gestaltete Nachbarn zusammenfassen: sonst entstuende pro Component-Knoten ein
    // eigener Abschnitt, und der Aufrufer zeichnete denselben Satz in vielen Stuecken.
    match out.last_mut() {
        Some(last) if last.color == color && last.style == style && last.shadow == shadow => {
            last.text.push_str(buf);
            buf.clear();
        }
        _ => out.push(TextSpan { text: std::mem::take(buf), color, style, shadow, ..Default::default() }),
    }
}

// Ein Text-Component-Baum (Chat-Nachrichten, s. GameMessageS2CPacket) auf reinen, lesbaren
// Klartext reduzieren -- kein vollstaendiger Style-/Klick-Event-Parser, nur "text"/
// "translate"+"with"/"extra" (die weit ueberwiegende Mehrheit realer Server-/Plugin-
// Nachrichten), reicht fuer eine Debug-Chatanzeige. "translate" wird ueber dieselbe
// Sprachtabelle aufgeloest wie in text_to_spans; ohne Treffer bleibt der rohe Schluessel.
pub fn text_to_plain_string(value: &NbtValue) -> String {
    match value {
        NbtValue::String(s) => s.clone(),
        NbtValue::Compound(_) => {
            let mut out = String::new();
            if let Some(s) = value.get("text").and_then(NbtValue::as_str) {
                out.push_str(s);
            } else if let Some(pattern) = value.get("translate").and_then(NbtValue::as_str).and_then(|key| translation_pattern(value, key))
            {
                let args: &[NbtValue] = match value.get("with") {
                    Some(NbtValue::List(args)) => args,
                    _ => &[],
                };
                for teil in split_translation(&pattern) {
                    match teil {
                        TranslationPart::Literal(s) => out.push_str(&s),
                        TranslationPart::Arg(i) => out.push_str(&args.get(i).map(arg_plain_string).unwrap_or_default()),
                    }
                }
            } else if let Some(key) = value.get("translate").and_then(NbtValue::as_str) {
                out.push_str(key);
                if let Some(NbtValue::List(args)) = value.get("with") {
                    if !args.is_empty() {
                        out.push_str(": ");
                        out.push_str(&args.iter().map(arg_plain_string).collect::<Vec<_>>().join(", "));
                    }
                }
            }
            if let Some(NbtValue::List(extra)) = value.get("extra") {
                for e in extra {
                    out.push_str(&text_to_plain_string(e));
                }
            }
            out
        }
        NbtValue::List(items) => items.iter().map(text_to_plain_string).collect(),
        _ => String::new(),
    }
}

// Wie collect_arg_spans, nur als Klartext.
fn arg_plain_string(arg: &NbtValue) -> String {
    match arg {
        NbtValue::Byte(v) => v.to_string(),
        NbtValue::Short(v) => v.to_string(),
        NbtValue::Int(v) => v.to_string(),
        NbtValue::Long(v) => v.to_string(),
        NbtValue::Float(v) => v.to_string(),
        NbtValue::Double(v) => v.to_string(),
        _ => text_to_plain_string(arg),
    }
}

#[cfg(test)]
mod span_tests {
    use super::*;

    #[test]
    fn uebersetzungsmuster_wie_vanilla() {
        use TranslationPart::*;
        assert_eq!(split_translation("%s was blown up by %s"), vec![Arg(0), Literal(" was blown up by ".into()), Arg(1)]);
        assert_eq!(split_translation("%2$s vor %1$s"), vec![Arg(1), Literal(" vor ".into()), Arg(0)]);
        assert_eq!(split_translation("100%% sicher"), vec![Literal("100% sicher".into())]);
        assert_eq!(split_translation("Sharpness"), vec![Literal("Sharpness".into())]);
    }

    // Ohne Sprachtabelle greift das "fallback" des Components -- und Zahlen-Argumente
    // erscheinen als Zahl statt zu verschwinden.
    #[test]
    fn fallback_und_zahlenargumente() {
        let mut c = HashMap::new();
        c.insert("translate".to_string(), NbtValue::String("mod.unbekannt.x".into()));
        c.insert("fallback".to_string(), NbtValue::String("Stufe %s von %s".into()));
        c.insert("with".to_string(), NbtValue::List(vec![NbtValue::Int(3), NbtValue::String("5".into())]));
        let v = NbtValue::Compound(c);
        let text: String = text_to_spans(&v).into_iter().map(|s| s.text).collect();
        assert_eq!(text, "Stufe 3 von 5");
        assert_eq!(text_to_plain_string(&v), "Stufe 3 von 5");
    }

    // Die echten Bytes vom Server (s. unlesbare_slots.log): 🗡 als Surrogat-Paar, dann ein
    // Leerzeichen. Und NUL als 0xC0 0x80.
    #[test]
    fn modified_utf8_mit_emoji_und_nul() {
        let bytes = [0xed, 0xa0, 0xbd, 0xed, 0xb7, 0xa1, 0x20];
        assert_eq!(decode_modified_utf8(&bytes).unwrap(), "\u{1F5E1} ");
        assert_eq!(decode_modified_utf8(&[0x61, 0xC0, 0x80, 0x62]).unwrap(), "a\u{0}b");
        assert_eq!(decode_modified_utf8("Kit Editor".as_bytes()).unwrap(), "Kit Editor");
    }
    use std::collections::HashMap;

    fn compound(pairs: Vec<(&str, NbtValue)>) -> NbtValue {
        NbtValue::Compound(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<HashMap<_, _>>())
    }

    // Eigene Schrift: jedes Zeichen bekommt einen festen Codepunkt je (Schrift, Zeichen),
    // vererbt an "extra"; die Standardschrift bleibt unveraendert.
    #[test]
    fn eigene_schrift_bekommt_eigene_codepunkte() {
        let kind = compound(vec![("text", NbtValue::String("\u{E001}".into()))]);
        let v = compound(vec![
            ("text", NbtValue::String("\u{E001}a".into())),
            ("font", NbtValue::String("peacock:hud".into())),
            ("extra", NbtValue::List(vec![kind, compound(vec![("text", NbtValue::String("\u{E001}".into())), ("font", NbtValue::String("minecraft:default".into()))])])),
        ]);
        let text: Vec<char> = text_to_spans(&v).into_iter().flat_map(|s| s.text.chars().collect::<Vec<_>>()).collect();
        assert_eq!(text.len(), 4);
        assert!(text[0] as u32 >= FONT_CHAR_FIRST && text[1] as u32 >= FONT_CHAR_FIRST);
        assert_eq!(text[0], text[2], "geerbt: dasselbe Zeichen in derselben Schrift");
        assert_eq!(text[3], '\u{E001}');
        assert_eq!(font_char_origin(text[1]).map(|(f, c)| (f.to_string(), c)), Some(("peacock:hud".to_string(), 'a')));
        // Andere Schrift, anderer Codepunkt; ohne Namensraum gilt "minecraft:".
        assert_ne!(font_char("minecraft:hud", '\u{E001}'), text[0]);
        assert_eq!(font_char("minecraft:hud", 'x'), font_char("minecraft:hud", 'x'));
    }

    // Die alten Paragraphen-Codes stehen mitten im Wortlaut und muessen zu Farben werden,
    // nicht als Zeichen im Bild landen.
    #[test]
    fn paragraphen_codes_werden_zu_farben_und_verschwinden_aus_dem_text() {
        let spans = text_to_spans(&NbtValue::String("\u{a7}aGruen\u{a7}cRot".to_string()));
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].text.as_str(), spans[0].color), ("Gruen", Some(0x55FF55)));
        assert_eq!((spans[1].text.as_str(), spans[1].color), ("Rot", Some(0xFF5555)));

        // Stil-Codes (fett) faerben nicht und werden nur weggelassen.
        let spans = text_to_spans(&NbtValue::String("\u{a7}lFett".to_string()));
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0].text.as_str(), spans[0].color), ("Fett", None));
    }

    // Das moderne "color"-Feld vererbt sich an die "extra"-Kinder, bis eines selbst eine
    // Farbe setzt.
    #[test]
    fn extra_erbt_die_farbe_des_elternteils() {
        let value = compound(vec![
            ("text", NbtValue::String("A".to_string())),
            ("color", NbtValue::String("gold".to_string())),
            (
                "extra",
                NbtValue::List(vec![
                    compound(vec![("text", NbtValue::String("B".to_string()))]),
                    compound(vec![("text", NbtValue::String("C".to_string())), ("color", NbtValue::String("#102030".to_string()))]),
                ]),
            ),
        ]);
        let spans = text_to_spans(&value);
        // A und B haben dieselbe (ererbte) Farbe und werden zusammengefasst.
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].text.as_str(), spans[0].color), ("AB", Some(0xFFAA00)));
        assert_eq!((spans[1].text.as_str(), spans[1].color), ("C", Some(0x102030)));
    }

    // Ein Reset stellt den Style des Elternteils wieder her -- nicht Weiss.
    #[test]
    fn reset_faellt_auf_die_ererbte_farbe_zurueck() {
        let value =
            compound(vec![("text", NbtValue::String("A\u{a7}cB\u{a7}rC".to_string())), ("color", NbtValue::String("aqua".to_string()))]);
        let spans = text_to_spans(&value);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].color, Some(0x55FFFF));
        assert_eq!(spans[1].color, Some(0xFF5555));
        assert_eq!((spans[2].text.as_str(), spans[2].color), ("C", Some(0x55FFFF)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Von Hand kodiertes Compound {"a": 5 (int), "b": "hi" (string)} im Netzwerk-Format
    // (kein Name fuers Wurzel-Tag).
    #[test]
    fn liest_einfaches_compound() {
        let mut bytes = vec![TAG_COMPOUND];
        bytes.push(TAG_INT);
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.push(b'a');
        bytes.extend_from_slice(&5i32.to_be_bytes());
        bytes.push(TAG_STRING);
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.push(b'b');
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(b"hi");
        bytes.push(TAG_END);

        let mut r = PacketReader::new(&bytes);
        let value = read_root(&mut r).unwrap().expect("kein TAG_End");
        assert_eq!(value.get("a").unwrap().as_i32(), Some(5));
        assert_eq!(value.get("b").unwrap().as_str(), Some("hi"));
    }

    #[test]
    // Gemischte Liste (seit 1.21.5): Nicht-Compounds stehen als {"": wert} darin und werden
    // beim Lesen ausgepackt -- so kommt ein Titel ["⚔", {text:" Match Found "}, "⚔"] ganz an.
    fn gemischte_liste_wird_ausgepackt() {
        let s = |b: &mut Vec<u8>, t: &str| {
            b.extend_from_slice(&(t.len() as u16).to_be_bytes());
            b.extend_from_slice(t.as_bytes());
        };
        let mut b = vec![TAG_LIST, TAG_COMPOUND];
        b.extend_from_slice(&3i32.to_be_bytes());
        for (key, text) in [("", "A"), ("text", " Match Found "), ("", "B")] {
            b.push(TAG_STRING);
            s(&mut b, key);
            s(&mut b, text);
            b.push(TAG_END);
        }
        let mut r = PacketReader::new(&b);
        let value = read_root(&mut r).unwrap().unwrap();
        let text: String = text_to_spans(&value).iter().map(|t| t.text.as_str()).collect();
        assert_eq!(text, "A Match Found B");
    }

    #[test]
    fn tag_end_ist_none() {
        let bytes = [TAG_END];
        let mut r = PacketReader::new(&bytes);
        assert!(read_root(&mut r).unwrap().is_none());
    }

    #[test]
    fn verschachteltes_compound_mit_liste() {
        // {"effects": {"water_color": 4159204 (int)}, "list": [1,2,3] (int list)}
        let mut inner = vec![TAG_COMPOUND];
        inner.push(TAG_INT);
        inner.extend_from_slice(&11u16.to_be_bytes());
        inner.extend_from_slice(b"water_color");
        inner.extend_from_slice(&4159204i32.to_be_bytes());
        inner.push(TAG_END);

        let mut bytes = vec![TAG_COMPOUND];
        bytes.push(TAG_COMPOUND);
        bytes.extend_from_slice(&7u16.to_be_bytes());
        bytes.extend_from_slice(b"effects");
        bytes.extend_from_slice(&inner[1..]); // ohne das aeussere TAG_COMPOUND-Byte

        bytes.push(TAG_LIST);
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(b"list");
        bytes.push(TAG_INT);
        bytes.extend_from_slice(&3i32.to_be_bytes());
        for v in [1i32, 2, 3] {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        bytes.push(TAG_END);

        let mut r = PacketReader::new(&bytes);
        let value = read_root(&mut r).unwrap().unwrap();
        let water_color = value.get("effects").unwrap().get("water_color").unwrap().as_i32();
        assert_eq!(water_color, Some(4159204));
        match value.get("list").unwrap() {
            NbtValue::List(items) => assert_eq!(items.len(), 3),
            other => panic!("erwartete Liste, war {other:?}"),
        }
    }
}

#[cfg(test)]
mod json_text_tests {
    use super::*;

    // Genau die Meldung, die als rohes JSON im Log stand.
    #[test]
    fn die_login_ablehnung_wird_zu_lesbarem_text() {
        let json = r#"{"italic":false,"color":"red","extra":[{"color":"gray","text":"(This may be caused by autoreconnect mods)"}],"text":"You are already online!\n"}"#;
        let spans = json_text_to_spans(json);
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(text.starts_with("You are already online!"), "{text:?}");
        assert!(text.contains("autoreconnect"), "der Zusatz aus extra fehlt: {text:?}");
        assert!(!text.contains("italic"), "Stil-Felder duerfen nicht im Text landen: {text:?}");
    }

    // Und die Farben kommen mit: rot fuer den Hauptteil, grau fuer den Zusatz.
    #[test]
    fn die_farben_der_abschnitte_bleiben_erhalten() {
        let json = r#"{"color":"red","extra":[{"color":"gray","text":"grau"}],"text":"rot"}"#;
        let spans = json_text_to_spans(json);
        let rot = spans.iter().find(|s| s.text.contains("rot")).expect("roter Abschnitt");
        let grau = spans.iter().find(|s| s.text.contains("grau")).expect("grauer Abschnitt");
        assert_eq!(rot.color, Some(0xFF5555), "Vanillas 'red'");
        assert_eq!(grau.color, Some(0xAAAAAA), "Vanillas 'gray'");
    }

    // Kein JSON: dann IST die Zeichenkette die Meldung -- nichts verschlucken.
    #[test]
    fn kaputtes_json_bleibt_als_text_stehen() {
        let spans = json_text_to_spans("einfach nur Text");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "einfach nur Text");
    }

    #[test]
    // Link-Klicks: neues Format (click_event/url, ab 1.21.5) und altes (clickEvent/value),
    // vererbt an "extra" wie die Farbe; ein Kind ohne eigenen Link erbt den des Eltern.
    fn open_url_wird_gelesen_und_vererbt() {
        let json = r#"{"text":"Seite: ","extra":[{"text":"hier","click_event":{"action":"open_url","url":"https://example.org"}},{"text":" ende"}]}"#;
        let spans = json_text_to_spans(json);
        let hier = spans.iter().find(|s| s.text == "hier").expect("hier");
        assert_eq!(hier.link.as_deref(), Some("https://example.org"));
        assert!(spans.iter().filter(|s| s.text.contains("Seite") || s.text.contains("ende")).all(|s| s.link.is_none()));

        let alt = json_text_to_spans(r#"{"text":"a","clickEvent":{"action":"open_url","value":"http://x.y"},"extra":[{"text":"b"}]}"#);
        assert!(alt.iter().all(|s| s.link.as_deref() == Some("http://x.y")), "{alt:?}");
        // Andere Aktionen sind keine Links.
        let cmd = json_text_to_spans(r#"{"text":"c","click_event":{"action":"run_command","command":"/spawn"}}"#);
        assert!(cmd.iter().all(|s| s.link.is_none()));
    }

    fn aktion(spans: &[TextSpan], text: &str) -> SpanAction {
        let span = spans.iter().find(|s| s.text.contains(text)).unwrap_or_else(|| panic!("{text} fehlt in {spans:?}"));
        span.action.as_deref().cloned().unwrap_or_default()
    }

    // Die drei uebrigen Klick-Aktionen, im neuen und im alten Format.
    #[test]
    fn klick_aktionen_werden_gelesen() {
        let neu = json_text_to_spans(
            r#"[{"text":"a","click_event":{"action":"run_command","command":"/spawn"}},{"text":"b","click_event":{"action":"suggest_command","command":"/msg x "}},{"text":"c","click_event":{"action":"copy_to_clipboard","value":"kopie"}}]"#,
        );
        assert_eq!(aktion(&neu, "a").click, Some(ClickAction::RunCommand("/spawn".into())));
        assert_eq!(aktion(&neu, "b").click, Some(ClickAction::SuggestCommand("/msg x ".into())));
        assert_eq!(aktion(&neu, "c").click, Some(ClickAction::CopyToClipboard("kopie".into())));
        let alt = json_text_to_spans(r#"{"text":"d","clickEvent":{"action":"run_command","value":"/home"}}"#);
        assert_eq!(aktion(&alt, "d").click, Some(ClickAction::RunCommand("/home".into())));
    }

    // Hover-Text wird vererbt, ein Kind mit eigenem Klick behaelt den geerbten Tooltip, und
    // Nachbartext ohne Aktion verschmilzt NICHT mit dem Abschnitt.
    #[test]
    fn hover_wird_feldweise_vererbt() {
        let json = r#"{"text":"vorne ","extra":[{"text":"Name","color":"gold","hover_event":{"action":"show_text","value":{"text":"Rang: Admin","color":"red"}},"insertion":"Name","extra":[{"text":" [Klick]","click_event":{"action":"suggest_command","command":"/msg Name "}}]},{"text":" hinten"}]}"#;
        let spans = json_text_to_spans(json);
        let name = aktion(&spans, "Name");
        let tip = name.hover.expect("Tooltip");
        assert_eq!(tip[0].text, "Rang: Admin");
        assert_eq!(tip[0].color, Some(0xFF5555));
        assert_eq!(name.insertion.as_deref(), Some("Name"));
        let klick = aktion(&spans, "[Klick]");
        assert!(klick.hover.is_some(), "der Tooltip wird an das Kind vererbt");
        assert_eq!(klick.click, Some(ClickAction::SuggestCommand("/msg Name ".into())));
        assert_eq!(klick.insertion.as_deref(), Some("Name"));
        assert!(spans.iter().filter(|s| s.text.contains("vorne") || s.text.contains("hinten")).all(|s| s.action.is_none()), "{spans:?}");
    }

    #[test]
    fn show_item_zeigt_name_und_lore() {
        let json = r#"{"text":"[Schwert]","hover_event":{"action":"show_item","id":"minecraft:diamond_sword","count":1,"components":{"minecraft:custom_name":{"text":"Excalibur","color":"aqua"},"minecraft:lore":["Zeile eins",{"text":"Zeile zwei","color":"gray"}]}}}"#;
        let tip = aktion(&json_text_to_spans(json), "Schwert").hover.expect("Tooltip");
        let text: String = tip.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "Excalibur\nZeile eins\nZeile zwei");
        assert_eq!(tip.iter().find(|s| s.text == "Zeile eins").and_then(|s| s.color), Some(0xAA00AA));
        // Ohne Namen: die ID lesbar gemacht; altes Format mit "contents".
        let alt = r#"{"text":"x","hoverEvent":{"action":"show_item","contents":{"id":"minecraft:golden_apple"}}}"#;
        assert_eq!(aktion(&json_text_to_spans(alt), "x").hover.unwrap()[0].text, "golden apple");
    }

    // show_entity zeigt Vanilla nur mit F3+H.
    #[test]
    fn show_entity_bleibt_ohne_tooltip() {
        let json = r#"{"text":"e","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":[1,2,3,4]}}"#;
        assert!(json_text_to_spans(json)[0].action.is_none());
    }
}
#[cfg(test)]
mod legacy_rundreise_tests {
    use super::*;

    #[test]
    fn farben_ueberleben_den_weg_ueber_paragraphen_codes() {
        let spans = vec![
            TextSpan { text: "Rang ".into(), color: Some(LEGACY_COLORS[12]), link: None, action: None, style: 0, shadow: None },
            TextSpan { text: "hex".into(), color: Some(0x12ab9f), link: None, action: None, style: 0, shadow: None },
            TextSpan { text: " ohne".into(), color: None, link: None, action: None, style: 0, shadow: None },
        ];
        let legacy = spans_to_legacy(&spans);
        assert_eq!(legacy_to_spans(&legacy, None), spans);
        // Ohne eigene Farbe gilt die geerbte (Teamfarbe).
        let geerbt = legacy_to_spans(&legacy, Some(0x5555ff));
        assert_eq!(geerbt[2].color, Some(0x5555ff));
    }

    #[test]
    fn unvollstaendige_hexfolge_wird_ignoriert() {
        let spans = legacy_to_spans("\u{a7}x\u{a7}1\u{a7}2abc", None);
        assert_eq!(spans.iter().map(|s| s.text.as_str()).collect::<String>(), "abc");
    }
}

#[cfg(test)]
mod style_tests {
    use super::*;

    fn json(text: &str) -> Vec<TextSpan> {
        json_text_to_spans(text)
    }

    #[test]
    fn schattenfarbe_als_zahl_und_als_liste_und_vererbt() {
        let spans = json(r#"{"text":"a","shadow_color":-16777216,"extra":[{"text":"b"},{"text":"c","shadow_color":[1.0,0.0,0.0,0.5]}]}"#);
        assert_eq!(spans.len(), 2, "a und b teilen sich den Schatten: {spans:?}");
        assert_eq!(spans[0].text, "ab");
        assert_eq!(spans[0].shadow, Some(0xFF00_0000));
        assert_eq!(spans[1].shadow, Some(0x80FF_0000));
        // Alpha 0 heisst: kein Schatten -- muss als eigener Wert ankommen, nicht als "keiner gesetzt".
        assert_eq!(json(r#"{"text":"x","shadow_color":0}"#)[0].shadow, Some(0));
        assert_eq!(json(r#"{"text":"x"}"#)[0].shadow, None);
    }

    #[test]
    fn schatten_ueberlebt_den_weg_ueber_paragraphen_codes() {
        let spans = vec![
            TextSpan { text: "ohne ".into(), color: Some(LEGACY_COLORS[12]), ..Default::default() },
            TextSpan { text: "aus".into(), color: Some(LEGACY_COLORS[12]), shadow: Some(0), ..Default::default() },
            TextSpan { text: "rot".into(), color: Some(LEGACY_COLORS[14]), shadow: Some(0x80FF_0000), ..Default::default() },
            TextSpan { text: " wieder".into(), color: None, ..Default::default() },
        ];
        let legacy = spans_to_legacy(&spans);
        assert_eq!(legacy_to_spans(&legacy, None), spans);
        // Ein angehaengter Text (Name nach dem Team-Prefix) erbt keinen Schatten.
        let mit_name = format!("{}Name", spans_to_legacy(&spans[1..3]));
        assert_eq!(legacy_to_spans(&mit_name, None).last().unwrap().shadow, None);
    }

    // "&8&m          " -- die Trennlinie vieler Scoreboards: Leerzeichen, durchgestrichen.
    #[test]
    fn durchgestrichene_leerzeichen_bleiben_als_abschnitt() {
        let spans = legacy_to_spans("\u{a7}8\u{a7}m          \u{a7}r Text", None);
        assert_eq!(spans[0].text, "          ");
        assert_eq!(spans[0].style, STYLE_STRIKETHROUGH);
        assert_eq!(spans[1].style, 0, "§r stellt den Stil zurueck");
    }

    // Ein Farbcode beendet fett (TextVisitFactory: Farbe setzt die Schnitte zurueck).
    #[test]
    fn farbcode_setzt_schnitte_zurueck() {
        let spans = legacy_to_spans("\u{a7}lFett\u{a7}cRot", None);
        assert_eq!((spans[0].text.as_str(), spans[0].style), ("Fett", STYLE_BOLD));
        assert_eq!((spans[1].text.as_str(), spans[1].style), ("Rot", 0));
    }

    #[test]
    fn component_stil_wird_vererbt_und_ueberschrieben() {
        let json = r#"{"text":"a","bold":true,"extra":[{"text":"b"},{"text":"c","bold":false,"strikethrough":true}]}"#;
        let spans = json_text_to_spans(json);
        let stile: Vec<(String, u8)> = spans.iter().map(|s| (s.text.clone(), s.style)).collect();
        assert_eq!(stile, [("ab".to_string(), STYLE_BOLD), ("c".to_string(), STYLE_STRIKETHROUGH)]);
        // Rundreise ueber die Paragraphen-Codes (Scoreboard) behaelt die Stile.
        let zurueck = legacy_to_spans(&spans_to_legacy(&spans), None);
        assert_eq!(zurueck.iter().map(|s| s.style).collect::<Vec<_>>(), [STYLE_BOLD, STYLE_STRIKETHROUGH]);
    }
}
