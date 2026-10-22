// Modul "Eigenname": der eigene Name, wo immer er in einem Text steht, umgeschrieben (Alias)
// und/oder verziert (Farbe, Verlauf, Regenbogen, eigene Schattenfarbe).
//
// Umschreiben und Markieren passieren an den Abschnitten (TextSpan), die Farbe dagegen erst
// beim Zeichnen: ein bewegter Verlauf aendert sich jedes Bild, die Chatzeile aber nicht. Jedes
// Zeichen des Namens bekommt dazu einen eigenen Abschnitt, dessen Farbe eine MARKE traegt --
// Bit 31 (Textfarben sind 24 Bit), in den Bits 24..30 die Lage des Zeichens im Namen (0..127),
// darunter die Farbe, die vorher dort stand. rgb_to_rgba loest die Marke ueber paint() auf; so
// faerbt jeder Zeichenweg (Chat, Tab-Liste, Namensschild ...) ohne eigenen Code.

use protocol::nbt::TextSpan;
use std::sync::RwLock;
use std::time::Instant;

const MARK: u32 = 0x8000_0000;
// Schattenfarbe "vom Modul": Alpha 0, also ohne Aufloesung gar kein Schatten statt eines
// falschen. In den unteren 7 Bits die Lage des Zeichens im Namen (Verlauf, Regenbogen).
const SHADOW_MARK: u32 = 0x00E6_0A00;
const SHADOW_MARK_MASK: u32 = 0xFFFF_FF80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deco {
    Off,
    Solid,
    Gradient,
    Flowing,
    Rainbow,
}

impl Deco {
    pub const ALL: [Deco; 5] = [Deco::Off, Deco::Solid, Deco::Gradient, Deco::Flowing, Deco::Rainbow];

    pub fn label(self) -> &'static str {
        self.label_or("Aus")
    }

    // Beschriftung, bei der "Aus" anders heisst (beim Schatten: der uebliche).
    pub fn label_or(self, aus: &'static str) -> &'static str {
        match self {
            Deco::Off => aus,
            Deco::Solid => "Einfarbig",
            Deco::Gradient => "Verlauf",
            Deco::Flowing => "Verlauf bewegt",
            Deco::Rainbow => "Regenbogen",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Deco::Off => "off",
            Deco::Solid => "solid",
            Deco::Gradient => "gradient",
            Deco::Flowing => "flowing",
            Deco::Rainbow => "rainbow",
        }
    }

    pub fn parse(s: &str) -> Option<Deco> {
        Deco::ALL.into_iter().find(|d| d.key() == s)
    }

    pub fn next(self) -> Deco {
        Deco::ALL[(self as usize + 1) % Deco::ALL.len()]
    }

    // Braucht die zweite Farbe.
    pub fn two_colors(self) -> bool {
        matches!(self, Deco::Gradient | Deco::Flowing)
    }

    pub fn moves(self) -> bool {
        matches!(self, Deco::Flowing | Deco::Rainbow)
    }
}

// Waehlbare Toene bei Erwaehnung: (Ereignis, Anzeigename). "" = aus.
pub const SOUNDS: [(&str, &str); 11] = [
    ("", "Aus"),
    ("minecraft:block.note_block.pling", "Pling"),
    ("minecraft:block.note_block.bell", "Glocke"),
    ("minecraft:block.note_block.chime", "Glockenspiel"),
    ("minecraft:block.note_block.harp", "Harfe"),
    ("minecraft:block.amethyst_block.chime", "Amethyst"),
    ("minecraft:entity.experience_orb.pickup", "Erfahrung"),
    ("minecraft:entity.player.levelup", "Levelaufstieg"),
    ("minecraft:entity.arrow.hit_player", "Treffer"),
    ("minecraft:entity.item.pickup", "Aufheben"),
    ("minecraft:ui.button.click", "Klick"),
];

pub fn sound_label(event: &str) -> &str {
    SOUNDS.iter().find(|(e, _)| *e == event).map_or(event, |(_, l)| l)
}

pub fn next_sound(event: &str) -> &'static str {
    let i = SOUNDS.iter().position(|(e, _)| *e == event).map_or(0, |i| i + 1);
    SOUNDS[i % SOUNDS.len()].0
}

// Eine Faerbung: Art, die beiden Farben (sRGB 0..1) und beim Regenbogen, wie weit er zu
// Weiss hin aufgehellt ist (0..1).
#[derive(Clone, Copy)]
pub struct Paint {
    pub deco: Deco,
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub light: f32,
}

// Was paint() und shadow() zum Aufloesen brauchen.
#[derive(Clone, Copy)]
pub struct Look {
    pub name: Paint,
    // Umlaeufe je Sekunde (bewegter Verlauf, Regenbogen), fuer Name und Schatten.
    pub speed: f32,
    // Faerbung des Schattens und seine Deckkraft (0..1); Deco::Off = der uebliche Schatten.
    pub shadow: Paint,
    pub shadow_alpha: f32,
}

static LOOK: RwLock<Option<Look>> = RwLock::new(None);
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

// None = Modul aus: Marken zeigen ihre alte Farbe.
pub fn set_look(look: Option<Look>) {
    if let Ok(mut l) = LOOK.write() {
        *l = look;
    }
}

fn look() -> Option<Look> {
    LOOK.read().ok().and_then(|l| *l)
}

fn to_u32(c: [f32; 3]) -> u32 {
    let k = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (k(c[0]) << 16) | (k(c[1]) << 8) | k(c[2])
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn hue(h: f32) -> [f32; 3] {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let x = 1.0 - (h6 % 2.0 - 1.0).abs();
    match h6 as u32 {
        0 => [1.0, x, 0.0],
        1 => [x, 1.0, 0.0],
        2 => [0.0, 1.0, x],
        3 => [0.0, x, 1.0],
        4 => [x, 0.0, 1.0],
        _ => [1.0, 0.0, x],
    }
}

// Farbe an Stelle t (0..1) im Namen; None bei Deco::Off.
fn color_at(p: &Paint, t: f32, speed: f32) -> Option<[f32; 3]> {
    let phase = START.get_or_init(Instant::now).elapsed().as_secs_f32() * speed;
    Some(match p.deco {
        Deco::Off => return None,
        Deco::Solid => p.a,
        Deco::Gradient => mix(p.a, p.b, t),
        // Ueber den Namen ein halber Umlauf: im Stand genau a bis b, dann wandert es hin und her.
        Deco::Flowing => {
            let u = (t * 0.5 - phase).rem_euclid(1.0);
            mix(p.a, p.b, 1.0 - (2.0 * u - 1.0).abs())
        }
        Deco::Rainbow => mix(hue(t * 0.6 - phase), [1.0; 3], p.light),
    })
}

// Die Farbe (0xRRGGBB), die eine Textfarbe tatsaechlich zeigt -- ungezeichnete Farben bleiben.
pub fn paint(rgb: u32) -> u32 {
    if rgb & MARK == 0 {
        return rgb;
    }
    let alt = rgb & 0xFF_FFFF;
    let t = ((rgb >> 24) & 0x7F) as f32 / 127.0;
    look().and_then(|l| color_at(&l.name, t, l.speed)).map_or(alt, to_u32)
}

// Schattenfarbe eines Abschnitts: die Marke wird zur eingestellten, None = der uebliche.
pub fn shadow(argb: u32) -> Option<u32> {
    if argb & SHADOW_MARK_MASK != SHADOW_MARK {
        return Some(argb);
    }
    let t = (argb & 0x7F) as f32 / 127.0;
    let l = look()?;
    let c = color_at(&l.shadow, t, l.speed)?;
    Some(((l.shadow_alpha.clamp(0.0, 1.0) * 255.0).round() as u32) << 24 | to_u32(c))
}

// Wie der Name umgeschrieben und markiert wird.
pub struct Rewrite<'a> {
    pub name: &'a str,
    // Leer = Name bleibt.
    pub alias: &'a str,
    pub deco: bool,
    pub shadow: bool,
}

fn word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// Fundstellen des Namens (Bytebereiche), Gross-/Kleinschreibung egal, nur als ganzes Wort --
// "Max" soll nicht in "Maximal" treffen.
fn matches(text: &str, name: &str) -> Vec<(usize, usize)> {
    let (t, n) = (text.as_bytes(), name.as_bytes());
    let mut out = Vec::new();
    if n.is_empty() || !name.is_ascii() || t.len() < n.len() {
        return out;
    }
    let mut i = 0;
    while i + n.len() <= t.len() {
        let e = i + n.len();
        if t[i..e].eq_ignore_ascii_case(n) && (i == 0 || !word_char(t[i - 1])) && (e == t.len() || !word_char(t[e])) {
            out.push((i, e));
            i = e;
        } else {
            i += 1;
        }
    }
    out
}

// Die Abschnitte mit umgeschriebenem und markiertem Namen; None = kein Treffer (oder nichts zu
// tun). Der Name darf ueber mehrere Abschnitte laufen (Plugins faerben gern jeden Buchstaben
// einzeln) -- der Ersatz nimmt den Stil des Abschnitts, in dem der Name beginnt.
pub fn rewrite(spans: &[TextSpan], r: &Rewrite) -> Option<Vec<TextSpan>> {
    if r.alias.is_empty() && !r.deco && !r.shadow {
        return None;
    }
    let voll: String = spans.iter().map(|s| s.text.as_str()).collect();
    let treffer = matches(&voll, r.name);
    if treffer.is_empty() {
        return None;
    }
    let ersatz = if r.alias.is_empty() { None } else { Some(r.alias) };
    let mut out = Vec::with_capacity(spans.len() + 16);
    let mut start = 0;
    for span in spans {
        let ende = start + span.text.len();
        let mut pos = start;
        for &(a, e) in treffer.iter().filter(|(a, e)| *e > start && *a < ende) {
            if a > pos {
                out.push(TextSpan { text: voll[pos..a].to_string(), ..span.clone() });
            }
            if a >= start {
                let text = ersatz.unwrap_or(&voll[a..e]);
                push_name(&mut out, span, text, r);
            }
            pos = e.min(ende);
        }
        if pos < ende {
            out.push(TextSpan { text: voll[pos..ende].to_string(), ..span.clone() });
        }
        start = ende;
    }
    Some(out)
}

fn push_name(out: &mut Vec<TextSpan>, stil: &TextSpan, text: &str, r: &Rewrite) {
    if !r.deco && !r.shadow {
        out.push(TextSpan { text: text.to_string(), ..stil.clone() });
        return;
    }
    let alt = stil.color.unwrap_or(0xFF_FFFF);
    let n = text.chars().count();
    for (i, c) in text.chars().enumerate() {
        let lage = if n <= 1 { 0 } else { (i * 127 / (n - 1)) as u32 };
        let color = if r.deco { Some(MARK | (lage << 24) | alt) } else { stil.color };
        let shadow = if r.shadow { Some(SHADOW_MARK | lage) } else { stil.shadow };
        out.push(TextSpan { text: c.to_string(), color, shadow, ..stil.clone() });
    }
}

// Nennt die Nachricht den Namen, und zwar nicht nur als Absender ("<Name> hi", "[Rang] Name:
// hi")? Fuer den Ton bei Erwaehnung.
pub fn mentions(spans: &[TextSpan], name: &str) -> bool {
    let voll: String = spans.iter().map(|s| s.text.as_str()).collect();
    matches(&voll, name).into_iter().any(|(a, e)| {
        let davor = voll[..a].trim_end();
        let danach = voll[e..].trim_start();
        let sprecher_zeichen = danach.starts_with(['>', ':', '»', '›']);
        let kopf = davor.is_empty() || davor.ends_with(['<', ']', ')', '|', '»', '›', '*']) || !davor.contains(char::is_whitespace);
        !(sprecher_zeichen && kopf)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(t: &str, c: Option<u32>) -> TextSpan {
        TextSpan { text: t.to_string(), color: c, ..Default::default() }
    }

    fn text(s: &[TextSpan]) -> String {
        s.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn alias_ueber_abschnitte_und_ohne_gross_klein() {
        let r = Rewrite { name: "DerAntonia", alias: "Coral", deco: false, shadow: false };
        let s = [span("hi der", None), span("ANTONIA!", Some(0xFF0000)), span(" derantonias", None)];
        let neu = rewrite(&s, &r).unwrap();
        assert_eq!(text(&neu), "hi Coral! derantonias");
        assert_eq!(neu.iter().find(|s| s.text == "Coral").unwrap().color, None);
    }

    #[test]
    fn markierung_je_zeichen() {
        let r = Rewrite { name: "Abc", alias: "", deco: true, shadow: true };
        let neu = rewrite(&[span("<abc> x", Some(0x123456))], &r).unwrap();
        assert_eq!(text(&neu), "<abc> x");
        let marken: Vec<u32> = neu[1..4].iter().map(|s| s.color.unwrap()).collect();
        assert_eq!(marken, vec![MARK | 0x123456, MARK | (63 << 24) | 0x123456, MARK | (127 << 24) | 0x123456]);
        let schatten: Vec<u32> = neu[1..4].iter().map(|s| s.shadow.unwrap()).collect();
        assert_eq!(schatten, vec![SHADOW_MARK, SHADOW_MARK | 63, SHADOW_MARK | 127]);
        assert_eq!(paint(0x123456), 0x123456);
    }

    #[test]
    fn erwaehnung_nicht_als_absender() {
        let n = "DerAntonia";
        assert!(!mentions(&[span("<DerAntonia> hallo", None)], n));
        assert!(!mentions(&[span("[Owner] DerAntonia: hallo", None)], n));
        assert!(mentions(&[span("<Max> hi derantonia", None)], n));
        assert!(mentions(&[span("<Max> DerAntonia, komm", None)], n));
    }
}
