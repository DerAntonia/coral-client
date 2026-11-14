// Markierungen im Textstrom (Schatten, Fett, Tippen, Schrift, Clip), Tipp- und Loesch-
// Animation, Textauswahl, Chat-Umbruch und Farbumrechnung.

use crate::*;

// Grenze zwischen zwei Auswahlbereichen im Textstrom -- z.B. zwischen Log und Serverliste
// oder zwischen Chatverlauf, Eingabe und Liste der gesendeten Nachrichten. Ohne Grenze nahm
// eine Auswahl ueber zwei Listen hinweg Text aus beiden mit. Technisch ein leerer
// Text mit negativem Alpha, den shape_text als Bereichswechsel liest und nicht zeichnet.
pub(crate) fn selection_boundary(texts: &mut Vec<UiText>) {
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, -1.0], scale: 1.0 });
}

pub(crate) fn is_selection_boundary(t: &UiText) -> bool {
    t.text.is_empty() && t.color[3] == -1.0
}

// Rand eines Textlaufs: Art, Staerke (s. Settings::shadow_strength) und ob der Vanilla-Schatten
// nur einen halben Schriftpixel versetzt ist (s. Settings::shadow_half).
pub(crate) type ShadowRun = (TextShadow, f32, bool);

// Marker in der Textliste: ab hier gilt fuer die folgenden Texte dieser Rand (None = zurueck
// zur allgemeinen Einstellung). Wie selection_boundary ein leerer Text mit negativem Alpha,
// den shape_text liest und nicht zeichnet -- so muss nicht jeder UiText ein eigenes Feld
// tragen.
pub(crate) fn push_shadow_marker(texts: &mut Vec<UiText>, stil: Option<ShadowRun>) {
    // Die Staerke (s. Settings::shadow_strength) reist im x des Markers mit, der halbe Versatz im y.
    let staerke = stil.map_or(0.0, |(_, s, _)| s);
    let halb = if stil.is_some_and(|(_, _, h)| h) { 1.0 } else { 0.0 };
    let code = match stil.map(|(s, _, _)| s) {
        None => -2.0,
        Some(TextShadow::Outline) => -3.0,
        Some(TextShadow::Vanilla) => -4.0,
        Some(TextShadow::Off) => -5.0,
        Some(TextShadow::Below) => -30.0,
        Some(TextShadow::Around) => -31.0,
    };
    texts.push(UiText { text: String::new(), x: staerke, y: halb, color: [0.0, 0.0, 0.0, code], scale: 1.0 });
}

// Some(neuer Stil) fuer einen Marker aus push_shadow_marker, sonst None.
pub(crate) fn shadow_marker(t: &UiText) -> Option<Option<ShadowRun>> {
    if !t.text.is_empty() {
        return None;
    }
    let stil = match t.color[3] {
        -2.0 => return Some(None),
        -3.0 => TextShadow::Outline,
        -4.0 => TextShadow::Vanilla,
        -5.0 => TextShadow::Off,
        -30.0 => TextShadow::Below,
        -31.0 => TextShadow::Around,
        _ => return None,
    };
    Some(Some((stil, t.x, t.y > 0.5)))
}

// Schattenfarbe, die der Server einem Abschnitt gibt (TextSpan::shadow), fuer den GLEICH
// FOLGENDEN Text -- nur fuer ihn, damit nichts auf Nachbartext ueberlaeuft. Ohne eigene Farbe
// kein Marker. Code -6; rgb in color (linear), das Alpha des Servers in x.
pub(crate) fn push_span_shadow(texts: &mut Vec<UiText>, shadow: Option<u32>) {
    let Some(argb) = shadow.and_then(ego_name::shadow) else { return };
    let [r, g, b, _] = rgb_to_rgba(argb & 0xFF_FFFF);
    texts.push(UiText { text: String::new(), x: (argb >> 24) as f32 / 255.0, y: 0.0, color: [r, g, b, -6.0], scale: 1.0 });
}

// Der GLEICH FOLGENDE Text ist fett (Code -7): shape_text zeichnet ihn ein zweites Mal um
// einen Font-Pixel nach rechts (Vanillas boldOffset) und gibt jedem Zeichen diesen Pixel mehr
// Vorschub. Als EIN Text statt zwei, damit erst beide Raender und dann beide Glyphen kommen --
// sonst lag der Rand des zweiten Durchgangs mitten in den Buchstaben des ersten, und
// fett mit Kontur sah schlecht aus.
pub(crate) fn push_bold_marker(texts: &mut Vec<UiText>) {
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, -7.0], scale: 1.0 });
}

pub(crate) fn is_bold_marker(t: &UiText) -> bool {
    t.text.is_empty() && t.color[3] == -7.0
}

// Der GLEICH FOLGENDE Text ist Eingabe (Code -8): aus ihm lernt stable_numbers nichts. Wer
// tippt, aendert Zahlen staendig -- als "wechselnd" erkannt, behielte eine Zahl im Eingabefeld
// den Platz ihrer laengsten Fassung, und nach dem Loeschen klaffte eine Luecke. Gilt fuer
// jedes Feld, in das man schreibt: Chat, Buch, Schild, Amboss, Zahlen- und Namensfelder.
pub(crate) fn push_typed_marker(texts: &mut Vec<UiText>) {
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, -8.0], scale: 1.0 });
}

pub(crate) fn is_typed_marker(t: &UiText) -> bool {
    t.text.is_empty() && t.color[3] == -8.0
}

// Geloeschte Zeichen fallen heraus wie in osu! (osu-framework BasicTextBox,
// FallingDownContainer.Hide): ausblenden und dabei beschleunigt (InQuad) nach unten. Dazu
// jedes Zeichen fuer sich und etwas zufaellig (Wunsch des Nutzers): Fallweg, seitliche Drift
// und Drehung streuen je Zeichen, die Dauer auch (ERASE_FALL ist die laengste). Manche fliegen
// stattdessen nach oben, manche zerplatzen in Pixel (s. GlyphExit). Gilt fuer jedes Feld mit
// Eingabe-Markierung (s. push_typed_marker).
pub(crate) const ERASE_FALL: Duration = Duration::from_millis(320);
// Deckkraft, mit der ein geloeschtes Zeichen losfaellt: sofort sichtlich weg, nicht erst am
// Ende des Falls.
pub(crate) const ERASE_START_ALPHA: f32 = 0.5;
// Splitter eines zerplatzenden Zeichens.
pub(crate) const ERASE_SHARDS: u32 = 8;

// Wie ein geloeschtes Zeichen verschwindet.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum GlyphExit {
    Fall,
    Rise,
    // Zerplatzt; der Wert streut die Splitter.
    Burst(u32),
}

// Ein Stueck getippter Text, wie es gezeichnet wurde.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TypedRun {
    pub(crate) text: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) scale: f32,
    pub(crate) color: [f32; 4],
    // Schrift und Rand des Bereichs, in dem er stand (s. begin_text_area) -- das Herausfallende
    // wird darin gemessen und gezeichnet, sonst laege es neben seiner alten Stelle.
    pub(crate) font: Option<FontKind>,
    pub(crate) shadow: Option<ShadowRun>,
}

// Ein herausfallendes Zeichen: Lage und Aussehen beim Loeschen, Breite und Hoehe (um die
// Mitte dreht es sich), dazu sein Zufall: Fallweg, Drift zur Seite, Drehung am Ende (rad) und
// Dauer.
pub(crate) struct FallingGlyph {
    pub(crate) run: TypedRun,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) fall: f32,
    pub(crate) drift: f32,
    pub(crate) angle: f32,
    pub(crate) dur: Duration,
    pub(crate) start: Instant,
    pub(crate) exit: GlyphExit,
}

impl FallingGlyph {
    // Zufall aus sechs Werten in -1..1 (s. zufallswert): Fallweg, Drift, Drehung, Dauer, Art
    // (gut die Haelfte faellt, ein Drittel steigt, der Rest zerplatzt), Splitter-Streuung.
    pub(crate) fn new(run: TypedRun, w: f32, h: f32, start: Instant, r: [f32; 6]) -> Self {
        let exit = if r[4] > 0.35 {
            GlyphExit::Rise
        } else if r[4] < -0.75 {
            GlyphExit::Burst((r[5] * 1.0e6) as i32 as u32)
        } else {
            GlyphExit::Fall
        };
        Self {
            run,
            w,
            h,
            fall: h * (1.3 + 0.5 * r[0]),
            drift: h * 0.35 * r[1],
            angle: 0.6 * r[2],
            dur: ERASE_FALL.mul_f32(0.8 + 0.2 * r[3]),
            start,
            exit,
        }
    }

    // Stand nach `t` Sekunden als eigene Overlay-Gruppe (jedes Zeichen dreht sich um seine
    // eigene Mitte). None = vorbei.
    pub(crate) fn group(&self, t: f32) -> Option<UiOverlayGroup> {
        let k = t / self.dur.as_secs_f32();
        if k >= 1.0 {
            return None;
        }
        let [r, g, b, a] = self.run.color;
        let alpha = a * ERASE_START_ALPHA * (1.0 - k);
        let mut gruppe = UiOverlayGroup { rects: Vec::new(), texts: Vec::new(), sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None };
        let (x, y) = match self.exit {
            GlyphExit::Fall => (self.run.x + self.drift * k, self.run.y + self.fall * k * k),
            // Schnell hoch und dabei abbremsend: Wurf gegen die Schwerkraft, am Ende oben.
            GlyphExit::Rise => (self.run.x + self.drift * k, self.run.y - self.fall * (2.0 * k - k * k)),
            GlyphExit::Burst(seed) => {
                // Pixelsplitter in der Farbe des Zeichens, aus seiner Mitte nach allen Seiten,
                // auslaufend und leicht absackend.
                let px = (self.h / 8.0).max(1.0).round();
                let (mx, my) = (self.run.x + self.w / 2.0, self.run.y + self.h / 2.0);
                let weg = 1.0 - (1.0 - k) * (1.0 - k);
                for i in 0..ERASE_SHARDS {
                    let z = |n: u32| zufallswert(seed.wrapping_add(i * 31 + n));
                    let winkel = (i as f32 + 0.5 * z(1)) / ERASE_SHARDS as f32 * std::f32::consts::TAU;
                    let weite = self.h * (0.9 + 0.5 * z(2)) * weg;
                    let sx = mx + winkel.cos() * weite - px / 2.0;
                    let sy = my + winkel.sin() * weite + self.h * 0.6 * k * k - px / 2.0;
                    gruppe.rects.push(UiRect { rect: Rect { x: sx.round(), y: sy.round(), w: px, h: px }, color: [r, g, b, alpha], ..UiRect::FLAT });
                }
                return Some(gruppe);
            }
        };
        push_shadow_marker(&mut gruppe.texts, self.run.shadow);
        push_font_marker(&mut gruppe.texts, self.run.font);
        gruppe.texts.push(UiText { text: self.run.text.clone(), x, y, color: [r, g, b, alpha], scale: self.run.scale });
        push_shadow_marker(&mut gruppe.texts, None);
        push_font_marker(&mut gruppe.texts, None);
        gruppe.pivot = (x + self.w / 2.0, y + self.h / 2.0);
        gruppe.angle = self.angle * k;
        Some(gruppe)
    }
}

// Alle Eingabe-Texte eines Frames in Zeichenreihenfolge: je der Text direkt hinter einer
// Eingabe-Markierung.
pub(crate) fn typed_runs<'a>(texts: impl Iterator<Item = &'a UiText>) -> Vec<TypedRun> {
    let mut out = Vec::new();
    let mut nach_marker = false;
    let (mut font, mut shadow) = (None, None);
    for t in texts {
        if is_typed_marker(t) {
            nach_marker = true;
            continue;
        }
        if let Some(f) = font_marker(t) {
            font = f;
            continue;
        }
        if let Some(s) = shadow_marker(t) {
            shadow = s;
            continue;
        }
        if nach_marker && !t.text.is_empty() {
            out.push(TypedRun { text: t.text.clone(), x: t.x, y: t.y, scale: t.scale, color: t.color, font, shadow });
        }
        nach_marker = false;
    }
    out
}

// Was zwischen zwei Frames aus dem getippten Text verschwunden ist, an seiner ALTEN Stelle:
// gemeinsamer Anfang und gemeinsames Ende bleiben, der Rest des alten Texts ist geloescht.
// `breite` misst Text in Pixeln, so wie er in diesem Stueck steht (Schrift, Skalierung).
//
// `caret` = wo der Cursor im alten Frame stand. Er entscheidet zweierlei:
//   - WELCHE Zeichen weg sind, wenn der Text das offenlaesst: aus "asdasdasd" wird durch
//     Loeschen jedes "asd" dasselbe "asdasd". Geloescht wird immer am Cursor (Ruecktaste
//     davor, Entf dahinter, eine Markierung reicht an ihn heran), also die Stelle dort.
//   - WO sie losfallen: ein Stueck, das am Cursor sass, faellt buendig an ihm. Im Chat gleitet
//     der Text links vom Cursor nach jedem Loeschen noch kurz (s. ChatSlide); wer schnell
//     loescht, faende die Zeichen sonst dort, wo der Text gerade hinglitt, vor dem Cursor.
//
// `genau` = was das Feld selbst gemeldet hat (s. chat_input::Erased): Anfang im alten Text,
// Laenge und ob der Cursor daran stand. Passt es zum Text, gilt es statt jeder Vermutung --
// eine Mausmarkierung kann weit weg vom Cursor liegen, dort fiele sonst das Falsche.
pub(crate) fn erased_pieces(alt: &[TypedRun], neu: &[TypedRun], breite: impl Fn(&TypedRun, &str) -> f32, mut caret: Option<(f32, f32)>, genau: Option<(usize, usize, bool)>) -> Vec<TypedRun> {
    let a: String = alt.iter().map(|r| r.text.as_str()).collect();
    let b: String = neu.iter().map(|r| r.text.as_str()).collect();
    let anfang: usize = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
    let ende: usize = a[anfang..].chars().rev().zip(b[anfang..].chars().rev()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
    let (mut von, mut bis) = (anfang, a.len() - ende);
    // Ergibt alt ohne a[s..s+n] wieder neu?
    let passt = |s: usize, n: usize| a.len() == b.len() + n && a.is_char_boundary(s) && a.is_char_boundary(s + n) && a[..s] == b[..s] && a[s + n..] == b[s..];
    // Bildschirmstelle einer Byte-Position im alten Text: (x, y) des Zeichens dort bzw. des
    // Endes des letzten Stuecks.
    let stelle = |pos: usize, am_ende: bool| -> Option<(f32, f32)> {
        let mut offset = 0;
        for r in alt {
            let (s, e) = (offset, offset + r.text.len());
            offset = e;
            if (s <= pos && pos < e) || (am_ende && s < pos && pos <= e) {
                return Some((r.x + breite(r, &r.text[..pos - s]), r.y));
            }
        }
        None
    };
    if let Some((s, n, am_cursor)) = genau.filter(|&(s, n, _)| passt(s, n)) {
        (von, bis) = (s, s + n);
        // Nur was am Cursor sass, rueckt an ihn heran.
        if !am_cursor {
            caret = None;
        }
    } else if let Some((cx, cy)) = caret.filter(|_| a.len() > b.len()) {
        // Reines Loeschen: jede Lage p, bei der alt ohne a[p..p+n] wieder neu ergibt, passt.
        // Davon die, deren Anfang oder Ende dem Cursor am naechsten liegt.
        let n = a.len() - b.len();
        let hinten: usize = a.chars().rev().zip(b.chars().rev()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
        let abstand = |p: usize| {
            [stelle(p, false), stelle(p + n, true)]
                .into_iter()
                .flatten()
                .map(|(x, y)| (x - cx).abs() + (y - cy).abs())
                .fold(f32::INFINITY, f32::min)
        };
        let erste = b.len().saturating_sub(hinten);
        let beste = (erste..=anfang.min(b.len())).filter(|&p| passt(p, n)).min_by(|&p, &q| abstand(p).total_cmp(&abstand(q)));
        if let Some(p) = beste {
            (von, bis) = (p, p + n);
        }
    }
    let mut out = Vec::new();
    let mut offset = 0;
    for r in alt {
        let (s, e) = (offset, offset + r.text.len());
        offset = e;
        let (lo, hi) = (von.max(s), bis.min(e));
        if lo >= hi {
            continue;
        }
        let (i, j) = (lo - s, hi - s);
        let mut stueck = TypedRun { text: r.text[i..j].to_string(), x: r.x + breite(r, &r.text[..i]), ..r.clone() };
        // Sass das Stueck in der Zeile des Cursors an ihm, buendig an ihn (s. oben).
        if let Some((cx, cy)) = caret {
            let w = breite(r, &stueck.text);
            let h = 8.0 * r.scale;
            let (vorn, hinten) = (cx - stueck.x, cx - (stueck.x + w));
            let schub = if hinten.abs() <= vorn.abs() { hinten } else { vorn };
            if (stueck.y - cy).abs() < h && schub.abs() < 4.0 * h {
                stueck.x += schub;
            }
        }
        out.push(stueck);
    }
    out
}

// Ab hier ruhiger Text (Code -33) bzw. wieder normaler (-34): Zahlen darin stehen in ihrer
// natuerlichen Breite und lehren stable_numbers nichts. Fuer den Chat -- eine Nachricht
// aendert sich nicht, reservierter Platz zerriss dort nur "(1 - 0)" oder "14/15", weil
// dasselbe Muster anderswo wechselte.
pub(crate) fn push_plain_numbers_marker(texts: &mut Vec<UiText>, an: bool) {
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, if an { -33.0 } else { -34.0 }], scale: 1.0 });
}

// Ab hier stufenlos skalierter Text (Code -35, Weltbeschriftungen) bzw. wieder normaler (-36):
// rastet nie aufs Pixelraster ein (s. FontAtlas::smooth_raster). Sonst rastete er genau an den
// vorgerasterten Faktoren fuer ein Bild ein und sprang beim Drehen der Kamera hin und her.
pub(crate) fn push_smooth_scale_marker(texts: &mut Vec<UiText>, an: bool) {
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, if an { -35.0 } else { -36.0 }], scale: 1.0 });
}

pub(crate) fn smooth_scale_marker(t: &UiText) -> Option<bool> {
    if !t.text.is_empty() {
        return None;
    }
    match t.color[3] {
        -35.0 => Some(true),
        -36.0 => Some(false),
        _ => None,
    }
}

// Ab hier Grossbuchstaben stehen lassen, auch wenn "Grossbuchstaben" (Schrift-Tab, Stil) aus
// ist (Code -41), bzw. wieder wie eingestellt (-42). Fuer Tastennamen auf den Belegungsknoepfen.
// Wer misst, setzt dafuer GpuState::keep_capitals mit (s. App::build_menu_ui).
pub(crate) fn push_keep_capitals_marker(texts: &mut Vec<UiText>, an: bool) {
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, if an { -41.0 } else { -42.0 }], scale: 1.0 });
}

pub(crate) fn keep_capitals_marker(t: &UiText) -> Option<bool> {
    if !t.text.is_empty() {
        return None;
    }
    match t.color[3] {
        -41.0 => Some(true),
        -42.0 => Some(false),
        _ => None,
    }
}

pub(crate) fn plain_numbers_marker(t: &UiText) -> Option<bool> {
    if !t.text.is_empty() {
        return None;
    }
    match t.color[3] {
        -33.0 => Some(true),
        -34.0 => Some(false),
        _ => None,
    }
}

// Die Schattenfarbe aus einem Marker von push_span_shadow (Alpha noch ohne die Textdeckkraft).
pub(crate) fn span_shadow_marker(t: &UiText) -> Option<[f32; 4]> {
    (t.text.is_empty() && t.color[3] == -6.0).then_some([t.color[0], t.color[1], t.color[2], t.x])
}

// Wie push_shadow_marker, fuer die Schrift: ab hier gilt fuer die folgenden Texte diese
// Schrift (None = zurueck zur allgemeinen). Codes -9 (allgemein) und -10 - FontKind-Index.
pub(crate) fn push_font_marker(texts: &mut Vec<UiText>, font: Option<FontKind>) {
    let code = font.map_or(-9.0, |k| -10.0 - FontKind::ALL.iter().position(|&f| f == k).unwrap_or(0) as f32);
    texts.push(UiText { text: String::new(), x: 0.0, y: 0.0, color: [0.0, 0.0, 0.0, code], scale: 1.0 });
}

// Some(neue Schrift) fuer einen Marker aus push_font_marker, sonst None.
pub(crate) fn font_marker(t: &UiText) -> Option<Option<FontKind>> {
    if !t.text.is_empty() {
        return None;
    }
    let c = t.color[3];
    if c == -9.0 {
        return Some(None);
    }
    let i = -10.0 - c;
    (i >= 0.0 && i.fract() == 0.0).then(|| FontKind::ALL.get(i as usize).copied()).flatten().map(Some)
}

// Das Rand-Viereck eines Glyphen (s. font.wgsl): bei der Kontur rundherum einen
// Bildschirmpixel groesser, beim Vanilla-Schatten um den Versatz nach rechts unten verlaengert.
// Die UV laufen im selben Mass ueber das Glyphenrechteck hinaus; was dort liegt, blendet der
// Shader ueber `bounds` aus.
#[allow(clippy::too_many_arguments)]
// Vanillas Schatten: ein Viertel der Textfarbe -- gerechnet im ANZEIGE-Raum (sRGB), wie
// Vanilla es tut. Die UI-Farben hier sind linear; ein Viertel davon kaeme auf der
// sRGB-Oberflaeche als helles Grau heraus (fuer weissen Text ~54 % statt 25 %), der Schatten
// sah fast weiss aus.
pub(crate) fn vanilla_shadow_color(c: [f32; 4]) -> [f32; 4] {
    let viertel = |v: f32| world::sky::srgb_to_linear(world::sky::linear_to_srgb(v) * 0.25);
    [viertel(c[0]), viertel(c[1]), viertel(c[2]), c[3]]
}

// Dunkel (lineare Farbe, jeder Kanal bis etwa sRGB 56/255): ein schwarzer Rand um dunkle Schrift
// macht sie nur dicker und unscharf, deshalb bekommt sie keinen (s. shape_text_in,
// world_text_border). Auch der fast schwarze Text auf Akzent-Knoepfen (TextOnAccent) zaehlt.
pub(crate) fn is_dark(c: [f32; 4]) -> bool {
    c[0].max(c[1]).max(c[2]) < 0.04
}

// `staerke`: bei der Kontur ihre Dicke in Bildschirmpixeln, sonst die Deckkraft (0..1) des
// Rands (s. Settings::shadow_strength).
pub(crate) fn text_border_quad(
    stil: TextShadow,
    staerke: f32,
    [x0, y0, x1, y1]: [f32; 4],
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    scale: f32,
    snap: bool,
    halb: bool,
    farbe: [f32; 4],
    screen_px: (f32, f32),
) -> [TextVertex; 4] {
    // Bei den weichen Raendern darf die Staerke ueber 1 gehen (s. Settings::shadow_strength),
    // font.wgsl begrenzt die Deckung.
    let grenze = if matches!(stil, TextShadow::Below | TextShadow::Around) { 100.0 / SOFT_SHADOW_FULL_PERCENT } else { 1.0 };
    let farbe = if stil == TextShadow::Outline { farbe } else { [farbe[0], farbe[1], farbe[2], farbe[3] * staerke.clamp(0.0, grenze)] };
    let je_px = [(uv_max[0] - uv_min[0]) / (x1 - x0), (uv_max[1] - uv_min[1]) / (y1 - y0)];
    let bounds = [uv_min[0], uv_min[1], uv_max[0], uv_max[1]];
    match stil {
        TextShadow::Below | TextShadow::Around => {
            // Unschaerfe mit der Schrift wachsend; "Unten" zusaetzlich um dasselbe Mass nach
            // unten versetzt, "Aussen" ohne Versatz.
            let b = (scale * 0.9).max(1.5);
            let (art, v) = if stil == TextShadow::Below { (TEXT_MODE_BELOW, b) } else { (TEXT_MODE_AROUND, 0.0) };
            text_quad_fx(
                x0 - b,
                y0 - b,
                x1 + b,
                y1 + v + b,
                [uv_min[0] - je_px[0] * b, uv_min[1] - je_px[1] * b],
                [uv_max[0] + je_px[0] * b, uv_max[1] + je_px[1] * (v + b)],
                farbe,
                bounds,
                [art, je_px[0], je_px[1], b],
                screen_px,
            )
        }
        TextShadow::Vanilla => {
            // Ein Schriftpixel (oder ein halber, s. Settings::shadow_half); bei pixelgenau gesetzten
            // Glyphen ganzzahlig, solange das noch mindestens ein Bildschirmpixel ist.
            let px = if halb { scale * 0.5 } else { scale };
            let v = if snap && px >= 1.0 { px.round() } else { px };
            text_quad_fx(
                x0,
                y0,
                x1 + v,
                y1 + v,
                uv_min,
                [uv_max[0] + je_px[0] * v, uv_max[1] + je_px[1] * v],
                farbe,
                bounds,
                [TEXT_MODE_SHADOW, je_px[0], je_px[1], v],
                screen_px,
            )
        }
        _ => {
            // Die Kontur reicht `dicke` Pixel weit, das Viereck ringsum auf ganze Pixel.
            let dicke = staerke.clamp(0.5, 4.0);
            let d = dicke.ceil();
            text_quad_fx(
                x0 - d,
                y0 - d,
                x1 + d,
                y1 + d,
                [uv_min[0] - je_px[0] * d, uv_min[1] - je_px[1] * d],
                [uv_max[0] + je_px[0] * d, uv_max[1] + je_px[1] * d],
                farbe,
                bounds,
                [TEXT_MODE_OUTLINE, je_px[0], je_px[1], dicke],
                screen_px,
            )
        }
    }
}

// Eine Textauswahl mit der Maus: Anfangspunkt, aktueller Punkt, und in welchem Bildschirm
// sie entstand. Gespeichert werden PUNKTE, keine Zeichenindizes: der Text wird jedes Bild neu
// gesetzt, und die Auswahl ergibt sich jedes Mal neu aus dem, was an diesen Stellen steht (s.
// select_in_runs). Wechselt der Bildschirm, gilt sie nicht mehr.
#[derive(Clone)]
pub(crate) struct TextSelection {
    pub(crate) anchor: (f32, f32),
    pub(crate) focus: (f32, f32),
    pub(crate) dragging: bool,
    // Erst nach ein paar Pixeln Ziehen wird ein Klick zur Auswahl -- ein gewoehnlicher Klick
    // soll nichts markieren.
    pub(crate) moved: bool,
    // 1 = zeichenweise, 2 = wortweise (Doppelklick), 3 = zeilenweise (Dreifachklick). Ab 2
    // gilt die Auswahl sofort, ohne Ziehen, und Ziehen erweitert in ganzen Woertern/Zeilen.
    pub(crate) clicks: u8,
    pub(crate) context: SelectionContext,
    // Im Chatverlauf begonnen: (Zeilenstand, Zeilenhoehe) beim Druecken (s. App::chat_view).
    // Die Punkte sind dann relativ zu DIESEM Stand gespeichert -- rueckt der Chat nach oben
    // (neue Nachricht, Blaettern), wandert die Auswahl mit ihrem Text mit. Vorher
    // blieb sie an derselben Bildschirmstelle und markierte dann fremden Text.
    pub(crate) chat_anchor: Option<(usize, f32)>,
}

// Zwei Klicks gelten als Doppel-/Dreifachklick, wenn sie so kurz nacheinander und so nah
// beieinander kommen (uebliche Desktop-Werte).
pub(crate) const MULTI_CLICK_MS: u128 = 450;
pub(crate) const MULTI_CLICK_PX: f32 = 5.0;

// Wo eine Auswahl gilt: Modus, Menue-Bildschirm und welche Overlays offen sind.
pub(crate) type SelectionContext = (AppMode, MenuScreen, bool, bool);

// Mindestweg (Pixel), ab dem aus einem Klick eine Auswahl wird.
pub(crate) const SELECTION_DRAG_MIN: f32 = 4.0;

// Toleranz, ab der zwei Zeilen als verschiedene Zeilen gelten (Pixel).
pub(crate) const SELECTION_LINE_EPS: f32 = 2.0;

// Die "Zeile", auf der ein Punkt liegt: Oberkante des Textstreifens, der ihn enthaelt, sonst
// sein eigenes y.
pub(crate) fn selection_line_of(runs: &[TextRun], p: (f32, f32)) -> f32 {
    runs.iter().find(|r| p.1 >= r.top && p.1 < r.bottom).map_or(p.1, |r| r.top)
}

// Liegt ein Zeichen (Zeile `top`, Mitte `mid`) in Lesereihenfolge an oder hinter dem Punkt?
pub(crate) fn selection_after(top: f32, mid: f32, line: f32, x: f32) -> bool {
    top > line + SELECTION_LINE_EPS || ((top - line).abs() <= SELECTION_LINE_EPS && mid >= x)
}

// Alle Zeichen der Bildschirmzeile ab Oberkante `top`, nach x sortiert -- ueber alle
// Textstuecke hinweg (ein Farbwechsel im Chat ist ein eigenes Stueck, gehoert aber zur Zeile).
pub(crate) fn line_glyphs(runs: &[TextRun], top: f32) -> Vec<(char, f32, f32)> {
    let mut g: Vec<(char, f32, f32)> =
        runs.iter().filter(|r| (r.top - top).abs() <= SELECTION_LINE_EPS).flat_map(|r| r.glyphs.iter().copied()).collect();
    g.sort_by(|a, b| a.1.total_cmp(&b.1));
    g
}

// Waagerechter Bereich [x0, x1) des Wortes unter dem Punkt (Doppelklick). Ein Wort endet an
// Leerraum und an Luecken zwischen getrennt stehenden Texten (Beschriftung links, Wert
// rechts). Auf Leerraum selbst: nur dieses eine Zeichen. None = kein Text unter dem Punkt.
pub(crate) fn word_bounds(runs: &[TextRun], p: (f32, f32)) -> Option<(f32, f32)> {
    let run = runs.iter().find(|r| p.1 >= r.top && p.1 < r.bottom)?;
    let g = line_glyphs(runs, run.top);
    let i = g.iter().position(|&(_, x0, x1)| p.0 >= x0 && p.0 < x1)?;
    if g[i].0.is_whitespace() {
        return Some((g[i].1, g[i].2));
    }
    let zum_wort = |a: &(char, f32, f32), b: &(char, f32, f32)| !a.0.is_whitespace() && !b.0.is_whitespace() && b.1 - a.2 <= 2.0;
    let (mut von, mut bis) = (i, i);
    while von > 0 && zum_wort(&g[von - 1], &g[von]) {
        von -= 1;
    }
    while bis + 1 < g.len() && zum_wort(&g[bis], &g[bis + 1]) {
        bis += 1;
    }
    Some((g[von].1, g[bis].2))
}

// Waagerechter Bereich des zusammenhaengenden Textblocks unter dem Punkt (Dreifachklick):
// die Zeile, aber nur so weit, wie der Text ohne grosse Luecke weiterlaeuft. Eine Luecke von
// mehr als einer Zeilenhoehe trennt zwei Texte, die zufaellig auf derselben Hoehe stehen --
// sonst nahm etwa ein Dreifachklick auf die Chunk-Statistik im Pausemenue den
// "Disconnect"-Knopf daneben mit.
pub(crate) fn line_block_bounds(runs: &[TextRun], p: (f32, f32)) -> Option<(f32, f32)> {
    let run = runs.iter().find(|r| p.1 >= r.top && p.1 < r.bottom)?;
    let luecke = run.bottom - run.top;
    let g = line_glyphs(runs, run.top);
    if g.is_empty() {
        return None;
    }
    // Der Block, in dem der Punkt liegt -- oder der naechste, wenn er zwischen zwei steht.
    let mut bloecke: Vec<(f32, f32)> = Vec::new();
    for &(_, x0, x1) in &g {
        match bloecke.last_mut() {
            Some(b) if x0 - b.1 <= luecke => b.1 = b.1.max(x1),
            _ => bloecke.push((x0, x1)),
        }
    }
    let abstand = |b: &(f32, f32)| (b.0 - p.0).max(p.0 - b.1).max(0.0);
    bloecke.into_iter().min_by(|x, y| abstand(x).total_cmp(&abstand(y)))
}

// Auswahl in ganzen Woertern (clicks = 2) oder ganzen Zeilen (clicks = 3) zwischen zwei
// Punkten: beide Enden werden auf ihre Wort- bzw. Zeilengrenzen erweitert.
pub(crate) fn select_in_runs_by_unit(runs: &[TextRun], a: (f32, f32), b: (f32, f32), clicks: u8) -> Vec<(usize, std::ops::Range<usize>)> {
    // Nur Text aus dem Bereich des Ankers zaehlt (s. selection_boundary).
    let bereich = anchor_group(runs, a);
    let eigene: Vec<TextRun> = runs.iter().filter(|r| Some(r.group) == bereich).cloned().collect();
    let grenzen = |p: (f32, f32)| {
        if clicks >= 3 {
            line_block_bounds(&eigene, p).unwrap_or((p.0, p.0))
        } else {
            word_bounds(&eigene, p).unwrap_or((p.0, p.0))
        }
    };
    let (la, lb) = (selection_line_of(runs, a), selection_line_of(runs, b));
    let b_zuerst = lb + SELECTION_LINE_EPS < la || ((lb - la).abs() <= SELECTION_LINE_EPS && b.0 < a.0);
    let (s, e) = if b_zuerst { (b, a) } else { (a, b) };
    let (sg, eg) = (grenzen(s), grenzen(e));
    // Mitten liegen innerhalb des Wortes, also: Anfang = Wortanfang, Ende = Wortende. Der
    // Bereich kommt vom ANKER a, nicht vom (evtl. vertauschten) Anfang.
    let ergebnis = select_in_runs_of_group(runs, (sg.0, s.1), (eg.1, e.1), bereich);
    // Ein Punkt ohne Text darunter (Wortgrenzen (x, x)) liefert fuer sich allein nichts.
    if ergebnis.is_empty() && clicks < 3 {
        return Vec::new();
    }
    ergebnis
}

// Welche Zeichen zwischen zwei Punkten liegen -- wie im Browser in LESEREIHENFOLGE (Zeile
// fuer Zeile, links nach rechts), nicht als Rechteck. Ergebnis: je betroffener Zeile deren
// Index und der Zeichenbereich.
pub(crate) fn select_in_runs(runs: &[TextRun], a: (f32, f32), b: (f32, f32)) -> Vec<(usize, std::ops::Range<usize>)> {
    let bereich = anchor_group(runs, a);
    select_in_runs_of_group(runs, a, b, bereich)
}

// Der Auswahlbereich unter dem Anfangspunkt: die Zeile, die ihn enthaelt, sonst die naechste.
pub(crate) fn anchor_group(runs: &[TextRun], a: (f32, f32)) -> Option<u32> {
    let abstand = |r: &TextRun| {
        let (x0, x1) = match (r.glyphs.first(), r.glyphs.last()) {
            (Some(f), Some(l)) => (f.1, l.2),
            _ => return f32::INFINITY,
        };
        let dx = (x0 - a.0).max(a.0 - x1).max(0.0);
        let dy = (r.top - a.1).max(a.1 - r.bottom).max(0.0);
        dx * dx + dy * dy
    };
    runs.iter().min_by(|p, q| abstand(p).total_cmp(&abstand(q))).filter(|r| abstand(r).is_finite()).map(|r| r.group)
}

pub(crate) fn select_in_runs_of_group(runs: &[TextRun], a: (f32, f32), b: (f32, f32), bereich: Option<u32>) -> Vec<(usize, std::ops::Range<usize>)> {
    let Some(bereich) = bereich else { return Vec::new() };
    let im_bereich: Vec<TextRun> = runs.iter().map(|r| if r.group == bereich { r.clone() } else { TextRun { glyphs: Vec::new(), ..r.clone() } }).collect();
    let runs = &im_bereich[..];
    let (la, lb) = (selection_line_of(runs, a), selection_line_of(runs, b));
    // Anfang = der Punkt, der in Lesereihenfolge zuerst kommt.
    let b_zuerst = lb + SELECTION_LINE_EPS < la || ((lb - la).abs() <= SELECTION_LINE_EPS && b.0 < a.0);
    let ((ls, s), (le, e)) = if b_zuerst { ((lb, b), (la, a)) } else { ((la, a), (lb, b)) };
    let mut out = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        let mut von = None;
        let mut bis = 0;
        for (j, &(_, x0, x1)) in run.glyphs.iter().enumerate() {
            let mid = f32::midpoint(x0, x1);
            if selection_after(run.top, mid, ls, s.0) && !selection_after(run.top, mid, le, e.0) {
                von.get_or_insert(j);
                bis = j + 1;
            }
        }
        if let Some(v) = von {
            out.push((i, v..bis));
        }
    }
    out
}

// Der ausgewaehlte Text: Zeilen durch Zeilenumbrueche getrennt, Stuecke derselben Zeile nach
// x geordnet und -- wo zwischen ihnen Luft ist -- durch ein Leerzeichen.
pub(crate) fn selection_text(runs: &[TextRun], sel: &[(usize, std::ops::Range<usize>)]) -> String {
    let mut stuecke: Vec<(f32, f32, f32, String)> = sel
        .iter()
        .map(|(i, r)| {
            let run = &runs[*i];
            let g = &run.glyphs[r.clone()];
            (run.top, g[0].1, g[g.len() - 1].2, g.iter().map(|(c, _, _)| *c).collect())
        })
        .collect();
    stuecke.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut text = String::new();
    let mut vorher: Option<(f32, f32)> = None;
    for (top, x0, x1, s) in stuecke {
        match vorher {
            Some((t, _)) if (top - t).abs() > SELECTION_LINE_EPS => {
                let ende = text.trim_end().len();
                text.truncate(ende);
                text.push('\n');
            }
            Some((_, ende)) if x0 - ende > 2.0 && !text.ends_with(' ') && !s.starts_with(' ') => text.push(' '),
            _ => {}
        }
        text.push_str(&s);
        vorher = Some((top, x1));
    }
    text.trim_end().to_string()
}

// Der gesetzte Verlauf eines Chats (s. App::chat_wrap_update): Schluessel, Nummer der ersten
// Nachricht (s. App::chat_pushed), Zeilen je Nachricht und alle Zeilen hintereinander.
// Alles, wovon der Umbruch abhaengt: Chat-Generation, Breite, Groesse (Bits), Schrift,
// Glyphen-Generation, Grossbuchstaben, Ziffern ohne Platzhalter, Zaehler der Ziffernbreiten (s.
// stable_numbers), Menuemassstab, Sprache (s. i18n::tr_state).
pub(crate) type ChatWrapKey = (u64, u32, u32, u8, u64, bool, bool, u64, u32, (u8, bool, bool));

pub(crate) struct ChatWrapCache {
    pub(crate) key: ChatWrapKey,
    pub(crate) first: u64,
    pub(crate) counts: std::collections::VecDeque<usize>,
    pub(crate) lines: std::rc::Rc<Vec<ChatLine>>,
}

// Eine gesetzte Chatzeile: Einzug (Bildschirmpixel) und die farbigen Abschnitte.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChatLine {
    pub(crate) indent: f32,
    pub(crate) spans: Vec<protocol::nbt::TextSpan>,
}

// Wo die Folgezeilen einer umbrochenen Nachricht anfangen: immer fest zwei Leerzeichen vom
// linken Chatrand. (Eine Fassung, die buendig unter dem Text nach "<Name> " weiterlief, war
// nicht gewollt -- so sind Folgezeilen gleichmaessig und sofort als solche zu erkennen.)
pub(crate) fn chat_hanging_indent(measure: &impl Fn(&str) -> f32) -> f32 {
    measure("  ")
}

// Eine Chatnachricht auf `max_width` Pixel setzen.
//
//  - Zeilenumbrueche ("\n") im Text werden umgesetzt. Frueher liefen die Zeilen einfach
//    ineinander: der Umbruch hat keine Glyphe und verschwand.
//  - Umbrochene Folgezeilen werden fest eingerueckt (s. chat_hanging_indent) und bekommen
//    entsprechend weniger Breite.
//  - Leerzeichen am Anfang einer Server-Zeile bleiben stehen (Server richten damit aus), das
//    Leerzeichen, an dem umbrochen wurde, faellt weg.
//  - Ein Wort, das allein zu breit ist, wird zeichenweise zerlegt.
pub(crate) fn wrap_chat_message(msg: &[protocol::nbt::TextSpan], max_width: f32, measure: impl Fn(&str) -> f32) -> Vec<ChatLine> {
    let einzug = chat_hanging_indent(&measure);
    let zeilen = wrap_spans(msg, max_width, measure, einzug);
    // Eine leere Nachricht ist eine leere Zeile (ComponentRenderUtils.wrapComponents): Server
    // schaffen damit Platz, etwa unter einem hohen Logo-Zeichen.
    if zeilen.is_empty() {
        return vec![ChatLine { indent: 0.0, spans: Vec::new() }];
    }
    zeilen
}

// Farbige Abschnitte auf `max_width` setzen wie wrap_chat_message, Folgezeilen um `einzug`
// eingerueckt (0 = buendig, wie Vanillas Font.split fuer Buchseiten).
pub(crate) fn wrap_spans(msg: &[protocol::nbt::TextSpan], max_width: f32, measure: impl Fn(&str) -> f32, einzug: f32) -> Vec<ChatLine> {
    wrap_spans_styled(msg, max_width, |t, _| measure(t), einzug)
}

// Wie wrap_spans, die Breite eines Stuecks haengt aber auch an seinem Stil (STYLE_*) -- fuer
// Vanillas Breiten, in denen Fett jedes Zeichen breiter macht.
pub(crate) fn wrap_spans_styled(msg: &[protocol::nbt::TextSpan], max_width: f32, measure: impl Fn(&str, u8) -> f32, einzug: f32) -> Vec<ChatLine> {
    let absaetze = split_label_lines(msg);
    let mut out = Vec::new();
    for absatz in absaetze {
        let mut aktuell: Vec<protocol::nbt::TextSpan> = Vec::new();
        let mut breite = 0.0f32;
        let mut zeilen_einzug = 0.0f32;
        let mut umbrochen = false;
        let anfuegen = |aktuell: &mut Vec<protocol::nbt::TextSpan>, text: &str, span: &protocol::nbt::TextSpan| match aktuell.last_mut() {
            Some(last) if last.color == span.color && last.link == span.link && last.action == span.action && last.style == span.style && last.shadow == span.shadow => {
                last.text.push_str(text);
            }
            _ => aktuell.push(protocol::nbt::TextSpan { text: text.to_string(), color: span.color, link: span.link.clone(), action: span.action.clone(), style: span.style, shadow: span.shadow }),
        };
        for span in &absatz {
            let measure = |t: &str| measure(t, span.style);
            for (i, wort) in span.text.split(' ').enumerate() {
                // Das Leerzeichen VOR dem Wort gehoert dazu -- ausser direkt nach einem
                // Umbruch, dort faellt es weg.
                let mit_luecke = if i > 0 && !(umbrochen && aktuell.is_empty()) { format!(" {wort}") } else { wort.to_string() };
                if mit_luecke.is_empty() {
                    continue;
                }
                let grenze = max_width - zeilen_einzug;
                let w = measure(&mit_luecke);
                if breite > 0.0 && breite + w > grenze {
                    out.push(ChatLine { indent: zeilen_einzug, spans: std::mem::take(&mut aktuell) });
                    breite = 0.0;
                    zeilen_einzug = einzug;
                    umbrochen = true;
                }
                let stueck = if aktuell.is_empty() && umbrochen { wort } else { mit_luecke.as_str() };
                if stueck.is_empty() {
                    continue;
                }
                let grenze = max_width - zeilen_einzug;
                if measure(stueck) > grenze {
                    for teil in hard_break(measure, stueck, grenze.min(max_width - einzug).max(1.0)) {
                        if breite > 0.0 {
                            out.push(ChatLine { indent: zeilen_einzug, spans: std::mem::take(&mut aktuell) });
                            breite = 0.0;
                            zeilen_einzug = einzug;
                            umbrochen = true;
                        }
                        breite += measure(&teil);
                        anfuegen(&mut aktuell, &teil, span);
                    }
                    continue;
                }
                breite += measure(stueck);
                anfuegen(&mut aktuell, stueck, span);
            }
        }
        out.push(ChatLine { indent: zeilen_einzug, spans: aktuell });
    }
    out
}

// Ein Text aus Vanillas Sprachdatei (der Sprache des Spiels), sonst der englische.
pub(crate) fn vanilla_text(key: &str, standard: &str) -> String {
    protocol::nbt::translate(key).unwrap_or_else(|| standard.to_string())
}

// Nackte Adressen im Text klickbar machen ("https://..." bis zum naechsten Leerzeichen).
// Vanilla macht das fuer Spielernachrichten ebenso; Server, die selbst ein ClickEvent setzen,
// haben ihren Link schon (s. nbt::TextSpan::link) und bleiben unangetastet.
pub(crate) fn linkify_chat(spans: Vec<protocol::nbt::TextSpan>) -> Vec<protocol::nbt::TextSpan> {
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        let eigener_klick = span.action.as_ref().is_some_and(|a| a.click.is_some());
        if span.link.is_some() || eigener_klick || !(span.text.contains("http://") || span.text.contains("https://")) {
            out.push(span);
            continue;
        }
        let mut rest = span.text.as_str();
        while let Some(start) = ["https://", "http://"].iter().filter_map(|p| rest.find(p)).min() {
            let ende = rest[start..].find(char::is_whitespace).map_or(rest.len(), |e| start + e);
            // Satzzeichen direkt hinter der Adresse gehoeren nicht dazu ("siehe https://a.b.").
            let url = rest[start..ende].trim_end_matches(['.', ',', ')', '!', '?', ';', ':', '"', '\'']);
            let ende = start + url.len();
            if start > 0 {
                out.push(protocol::nbt::TextSpan { text: rest[..start].to_string(), color: span.color, link: None, action: span.action.clone(), style: span.style, shadow: span.shadow });
            }
            out.push(protocol::nbt::TextSpan { text: url.to_string(), color: span.color, link: Some(url.to_string()), action: span.action.clone(), style: span.style, shadow: span.shadow });
            rest = &rest[ende..];
        }
        if !rest.is_empty() {
            out.push(protocol::nbt::TextSpan { text: rest.to_string(), color: span.color, link: None, action: span.action.clone(), style: span.style, shadow: span.shadow });
        }
    }
    out
}

// Ein Farbkanal 0..255 aus Vanilla (sRGB-Wert) -> linearer Wert der UI. Die Oberflaeche ist
// ein sRGB-Ziel und kodiert beim Schreiben selbst; ungewandelt kamen alle Server-Farben zu
// hell an (etwa Chat-Textfarben). Alpha bleibt linear.
pub(crate) fn srgb_channel(v: u32) -> f32 {
    world::sky::srgb_to_linear((v & 0xFF) as f32 / 255.0)
}

// 0xAARRGGBB (Vanillas Hintergrundfarbe) -> die Vec4-Form der UI.
pub(crate) fn argb_to_rgba(argb: i32) -> [f32; 4] {
    let v = argb as u32;
    [srgb_channel(v >> 16), srgb_channel(v >> 8), srgb_channel(v), ((v >> 24) & 0xFF) as f32 / 255.0]
}

// Vanilla mischt halbdurchsichtige Flaechen im sRGB-Raum (OpenGL ohne sRGB-Framebuffer),
// unsere Oberflaeche ist ein sRGB-Ziel und mischt LINEAR. Dieselbe Deckkraft wirkt dadurch
// ganz anders: Vanillas weisser Zeilenschleier der Tab-Liste (0x20FFFFFF) hebt dunklen Grund
// um 12,5 % der ANZEIGE-Helligkeit, linear gemischt aber um 12,5 % der Lichtmenge -- das
// sind rund 39 % Anzeige-Helligkeit, und die Zeilen wirken milchig weiss. Umgerechnet wird so, dass das Ergebnis dort exakt stimmt, wo
// der Unterschied am groessten ist: ein weisser Schleier ueber Schwarz, ein schwarzer ueber
// Weiss. Dazwischen weicht es nur noch wenig ab.
pub(crate) fn white_veil(alpha: f32) -> [f32; 4] {
    [1.0, 1.0, 1.0, world::sky::srgb_to_linear(alpha)]
}

// Gegenstueck zu white_veil fuer schwarze Hintergruende.
// Abdunklung hinter Fenstern in der Bildschirmmitte (Quick Connect, Acc-Manager, Server-
// Eintrag, Buch, Schild): ueberall dieselbe, halbe Helligkeit.
// Was das Disconnect-Fenster zeigt: nur die Meldung des Servers, ohne die Vorsaetze, die
// der Netz-Task fuers Log davorsetzt. Ohne Servermeldung (IO-Fehler, Timeout) bleibt der
// ganze Text -- er sagt dann selbst, was passiert ist.
// Das Disconnect-Fenster ist doppelt so gross (Flaeche) wie ein gewoehnlicher Dialog: jede
// Seite, jeder Abstand und die Schrift mal Wurzel 2.
pub(crate) const DISCONNECT_POPUP_SCALE: f32 = std::f32::consts::SQRT_2;

// Ein Grund mit Paragraphen-Codes (s. net::disconnect_message) als reiner Text -- fuer Log und Addons.
pub(crate) fn strip_legacy_codes(text: &str) -> String {
    protocol::nbt::legacy_to_spans(text, None).into_iter().map(|s| s.text).collect()
}

pub(crate) fn disconnect_popup_text(reason: &str) -> &str {
    let r = reason.strip_prefix("Protokoll-Fehler: ").unwrap_or(reason);
    if let Some(grund) = r.strip_prefix("Server hat Login abgelehnt: ") {
        return grund;
    }
    if r.starts_with("Server-Disconnect im ") {
        if let Some((_, grund)) = r.split_once("-State: ") {
            return grund;
        }
    }
    r
}

pub(crate) fn dialog_dim() -> [f32; 4] {
    black_veil(0.5)
}

pub(crate) fn black_veil(alpha: f32) -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0 - world::sky::srgb_to_linear(1.0 - alpha)]
}

// 0xRRGGBB (Textfarbe eines Components) -> die Vec4-Form der UI, voll deckend.
pub(crate) fn rgb_to_rgba(rgb: u32) -> [f32; 4] {
    // Zeichen des eigenen Namens tragen eine Marke statt einer Farbe (s. ego_name).
    let rgb = ego_name::paint(rgb);
    [srgb_channel(rgb >> 16), srgb_channel(rgb >> 8), srgb_channel(rgb), 1.0]
}
