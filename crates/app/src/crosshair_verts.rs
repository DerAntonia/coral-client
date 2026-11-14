// Fadenkreuz- und Zeiger-Geometrie.

use crate::*;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CrosshairVertex {
    pub(crate) position: [f32; 2],
    pub(crate) uv: [f32; 2],
    // Glyphen-UV im Font-Atlas (Gruppe 1), nur fuer die Glyphen-Arten (s. kind).
    pub(crate) glyph_uv: [f32; 2],
    // Was dieses Dreieck ist (s. crosshair.wgsl): 0 = Flaeche in Fadenkreuz-Farbe, 1 = schwarze
    // Kontur, 2 = Glyphe in Fadenkreuz-Farbe, 3 = Glyphe schwarz (Kontur), 4 = Glyphe weiss,
    // 5 = Kreis in Fadenkreuz-Farbe, 6 = Kreis schwarz (Kontur), 7 = Glyphe gelb.
    // Als Attribut und nicht als Uniform, weil alles in EINEM Zeichenaufruf rausgeht.
    pub(crate) kind: f32,
    // Deckkraft (Pfeil-Anzeige hat eine eigene).
    pub(crate) alpha: f32,
    // Kreise (Arten 5 und 6): innerer und aeusserer Radius in Pixeln; glyph_uv ist dann der
    // Abstand zur Kreismitte in Pixeln. Glyphen: x = Farbplatz (s. CrosshairText::fade_slot).
    pub(crate) circle: [f32; 2],
}

pub(crate) const CROSSHAIR_KIND_FILL: f32 = 0.0;
pub(crate) const CROSSHAIR_KIND_OUTLINE: f32 = 1.0;
pub(crate) const CROSSHAIR_KIND_DISC: f32 = 5.0;
pub(crate) const CROSSHAIR_KIND_DISC_OUTLINE: f32 = 6.0;
pub(crate) const CROSSHAIR_KIND_GLYPH: f32 = 2.0;
pub(crate) const CROSSHAIR_KIND_GLYPH_BLACK: f32 = 3.0;
pub(crate) const CROSSHAIR_KIND_GLYPH_WHITE: f32 = 4.0;
pub(crate) const CROSSHAIR_KIND_GLYPH_YELLOW: f32 = 7.0;

// s. App::area_font_scope: stellt beim Fallen die vorige Messschrift wieder her.
pub(crate) struct LayoutFontScope<'a> {
    pub(crate) gpu: &'a GpuState,
    pub(crate) vorher: Option<FontKind>,
    pub(crate) ruhig_vorher: bool,
}

impl Drop for LayoutFontScope<'_> {
    fn drop(&mut self) {
        self.gpu.set_layout_font(self.vorher);
        self.gpu.plain_numbers.set(self.ruhig_vorher);
    }
}

// Eine HUD-Zahl, die im Fadenkreuz-Pass gezeichnet wird (Schild-Countdown, Pfeile): so kann
// sie die Gegenfarbe zum Untergrund bekommen wie das Kreuz selbst.
pub(crate) struct CrosshairText {
    pub(crate) text: String,
    // Mitte in Bildschirmpixeln.
    pub(crate) center_px: (f32, f32),
    pub(crate) scale: f32,
    // true = weiss mit schwarzer Kontur, false = Fadenkreuz-Farbe (Kontur nur, wenn das
    // Fadenkreuz eine hat).
    pub(crate) white: bool,
    // Gelb mit schwarzer Kontur, geht vor `white` (Pfeil-Zahl mit geladener Armbrust).
    pub(crate) yellow: bool,
    // Schrift (GpuState::hud_font).
    pub(crate) font: FontKind,
    pub(crate) alpha: f32,
    // Eigene Gegenfarbe zum Untergrund der Zahl (s. crosshair.wgsl::FADE_PLAETZE).
    pub(crate) fade_slot: u32,
    // Jede Ziffer in der Breite der breitesten, mittig darin (Einstellung "Stabile Zahlen"):
    // aus einer 8 wird eine 1, ohne dass die Zahl ruckt. Anders als in stable_numbers ohne
    // Lernen -- ein Zaehler wechselt ohnehin.
    pub(crate) stable_digits: bool,
}

// Platz und eigener Vorschub eines Zeichens im Fadenkreuz-Text (in Schriftpixeln), s.
// CrosshairText::stable_digits. None = die Schrift hat das Zeichen nicht.
pub(crate) fn crosshair_advance(atlas: &FontAtlas, ch: char, stable_digits: bool) -> Option<(f32, f32)> {
    let eigen = atlas.glyph(ch)?.advance_px;
    if !(stable_digits && ch.is_ascii_digit()) {
        return Some((eigen, eigen));
    }
    let breiteste = ('0'..='9').filter_map(|c| atlas.glyph(c)).map(|g| g.advance_px).fold(eigen, f32::max);
    Some((breiteste, eigen))
}

// Farbplaetze der Gegenfarbe (crosshair.wgsl::FADE_PLAETZE): Kreuz und Ring, Schild-Countdown,
// Pfeilzahl.
pub(crate) const CROSSHAIR_FADE_SLOTS: usize = 3;
pub(crate) const CROSSHAIR_FADE_SHIELD: u32 = 1;
pub(crate) const CROSSHAIR_FADE_ARROWS: u32 = 2;

// Zustand des Pixel-Editors: Malmodus und der laufende Strich (gesetzter Wert, letzter Pixel).
pub(crate) struct CrosshairPainter {
    pub(crate) symmetry: crosshair_editor::Symmetry,
    pub(crate) stroke: Option<(crosshair_editor::Pixel, (i32, i32))>,
}

impl Default for CrosshairPainter {
    fn default() -> Self {
        Self { symmetry: crosshair_editor::Symmetry::Quad, stroke: None }
    }
}

// Ein eigenes Fadenkreuz fuer den Fadenkreuz-Pass: Laeufe (x, y, Breite) relativ zur Mitte.
pub(crate) struct CustomCrosshair {
    pub(crate) fill: Vec<(i32, i32, i32)>,
    pub(crate) outline: Vec<(i32, i32, i32)>,
}

// Pixel-genau, je Lauf ein Viereck: Kontur zuerst, dann die Fuellung.
pub(crate) fn custom_crosshair_vertices(c: &CustomCrosshair, center_px: (f32, f32), screen_px: (f32, f32)) -> Vec<CrosshairVertex> {
    let mut out = Vec::with_capacity((c.fill.len() + c.outline.len()) * 6);
    for (laeufe, art) in [(&c.outline, 1.0), (&c.fill, CROSSHAIR_KIND_FILL)] {
        for &(x, y, w) in laeufe {
            let (x0, y0) = (center_px.0 + x as f32, center_px.1 + y as f32);
            out.extend(crosshair_quad_verts(x0, y0, x0 + w as f32, y0 + 1.0, screen_px, art));
        }
    }
    out
}

// Alles, was der Fadenkreuz-Pass ausser dem Kreuz zeichnet.
#[derive(Default)]
pub(crate) struct CrosshairExtras {
    // Eigenes Fadenkreuz statt des eingebauten (s. crosshair_editor).
    pub(crate) custom: Option<CustomCrosshair>,
    // Form und Groesse des eingebauten (s. CROSSHAIR_SHAPES); Groesse 0 = Standard des Kreuzes.
    pub(crate) shape: usize,
    pub(crate) size: u32,
    pub(crate) texts: Vec<CrosshairText>,
    // Schild-Cooldown als Kreis: Mitte (normiert) und verbleibender Anteil 0..1.
    pub(crate) ring: Option<((f32, f32), f32)>,
    // Kontur um den Ring (Settings::crosshair_ring_outline) und sein Radius in Pixeln.
    pub(crate) ring_outline: bool,
    pub(crate) ring_radius: f32,
    // Feuer-Modus "Fadenkreuz" beim Brennen: diese Farbe nur fuer Kreuz und Ring -- die
    // Zahlen (Pfeile, Countdown) behalten die normale Fadenkreuz-Farbe.
    pub(crate) fire_rgb: Option<[f32; 3]>,
}

// Ring um `center_px`, ab 12 Uhr im Uhrzeigersinn bis `fraction` des Umfangs (verbleibende
// Zeit) -- als Segment-Vierecke. Mit Kontur zuerst ein breiterer schwarzer Ring.
pub(crate) fn crosshair_ring_vertices(center_px: (f32, f32), radius: f32, fraction: f32, screen_px: (f32, f32), outline: bool) -> Vec<CrosshairVertex> {
    let fraction = fraction.clamp(0.0, 1.0);
    let n = ((SHIELD_RING_SEGMENTS as f32 * fraction).ceil() as usize).max(1);
    let mut out = Vec::new();
    let mut ring = |inner: f32, outer: f32, kind: f32| {
        for i in 0..n {
            let a0 = std::f32::consts::TAU * fraction * i as f32 / n as f32;
            let a1 = std::f32::consts::TAU * fraction * (i + 1) as f32 / n as f32;
            // 0 = oben, im Uhrzeigersinn (Bildschirm-y zeigt nach unten).
            let p = |a: f32, r: f32| (center_px.0 + r * a.sin(), center_px.1 - r * a.cos());
            let (i0, o0, o1, i1) = (p(a0, inner), p(a0, outer), p(a1, outer), p(a1, inner));
            let v = |(x, y): (f32, f32)| CrosshairVertex {
                position: pixel_to_ndc(x, y, screen_px),
                uv: [x / screen_px.0, y / screen_px.1],
                glyph_uv: [0.0, 0.0],
                kind,
                alpha: 1.0,
                circle: [0.0; 2],
            };
            out.extend_from_slice(&[v(i0), v(o0), v(o1), v(i0), v(o1), v(i1)]);
        }
    };
    let (r, t) = (radius, SHIELD_RING_THICKNESS_PX / 2.0);
    if outline {
        ring(r - t - CROSSHAIR_OUTLINE_PX, r + t + CROSSHAIR_OUTLINE_PX, 1.0);
    }
    ring(r - t, r + t, CROSSHAIR_KIND_FILL);
    out
}

// UV hier ist die ABSOLUTE Position im gesamten offscreen_view (0..1 ueber den ganzen
// Bildschirm), nicht mehr relativ zu einem kleinen Kopierbereich.
pub(crate) fn crosshair_quad_verts(x0: f32, y0: f32, x1: f32, y1: f32, screen_px: (f32, f32), outline: f32) -> [CrosshairVertex; 6] {
    let uv_of = |px: f32, py: f32| [px / screen_px.0, py / screen_px.1];
    let to_vert =
        |px: f32, py: f32| CrosshairVertex { position: pixel_to_ndc(px, py, screen_px), uv: uv_of(px, py), glyph_uv: [0.0, 0.0], kind: outline, alpha: 1.0, circle: [0.0; 2] };
    [to_vert(x0, y0), to_vert(x1, y0), to_vert(x1, y1), to_vert(x0, y0), to_vert(x1, y1), to_vert(x0, y1)]
}

// Hoechstzahl der Vertices im Fadenkreuz-Pass: Kreuz mit Kontur (24), der Schild-Kreis (48
// Segmente, mit Kontur doppelt) und Glyphen-Text (je Zeichen mit Kontur 9 Vierecke) --
// reichlich bemessen. (Frueher stand hier eine eigene 3x5-Pixelschrift fuer den Countdown;
// jetzt kommen die Glyphen aus dem Font-Atlas, s. GpuState::crosshair_glyph_vertices.)
// Ein eigenes Kreuz (s. crosshair_editor) kann bis zu GRID*GRID/2 Laeufe haben.
pub(crate) const CROSSHAIR_MAX_VERTICES: usize = 4096 + crosshair_editor::GRID * crosshair_editor::GRID * 3;

// Das eingebaute Fadenkreuz um `center_px` (Bildschirmpixel, die Bildmitte samt Wackeln),
// aus den Bausteinen von crosshair_parts: Balken als Vierecke,
// Kreise als Viereck um den Kreis, dessen Kante der Shader je Pixel ausrechnet.
//
// ECHTER BUG gewesen: die Kontur lief zuerst ueber die normalen UI-Rechtecke. Der HUD-Pass
// zeichnet aber in DIESELBE Offscreen-Textur, die der Fadenkreuz-Pass danach abtastet, um die
// Gegenfarbe zu bestimmen. Unter dem Kreuz stand damit ueberall die eigene schwarze Kontur;
// Schwarz hat keinen definierten Farbton (rgb_to_hue faellt auf 0 zurueck), also kam immer
// dieselbe Farbe heraus. Jetzt entsteht die Kontur im Fadenkreuz-Pass selbst -- also NACH dem
// Abtasten und damit unsichtbar fuer die eigene Farbwahl.
pub(crate) fn crosshair_vertices(shape: usize, size: u32, center_px: (f32, f32), screen_px: (f32, f32), outline: bool) -> Vec<CrosshairVertex> {
    let mut out = Vec::new();
    for (teil, kontur) in crosshair_parts(shape, size, center_px, outline) {
        match teil {
            CrosshairPart::Bar(r) => {
                let art = if kontur { CROSSHAIR_KIND_OUTLINE } else { CROSSHAIR_KIND_FILL };
                out.extend(crosshair_quad_verts(r.x, r.y, r.x + r.w, r.y + r.h, screen_px, art));
            }
            CrosshairPart::Disc { center, inner, outer } => {
                // Ein Pixel mehr als der Radius: Platz fuer die weiche Kante.
                let e = outer + 1.0;
                let kind = if kontur { CROSSHAIR_KIND_DISC_OUTLINE } else { CROSSHAIR_KIND_DISC };
                let v = |dx: f32, dy: f32| {
                    let (px, py) = (center.0 + dx, center.1 + dy);
                    CrosshairVertex {
                        position: pixel_to_ndc(px, py, screen_px),
                        uv: [px / screen_px.0, py / screen_px.1],
                        glyph_uv: [dx, dy],
                        kind,
                        alpha: 1.0,
                        circle: [inner, outer],
                    }
                };
                out.extend_from_slice(&[v(-e, -e), v(e, -e), v(e, e), v(-e, -e), v(e, e), v(-e, e)]);
            }
        }
    }
    out
}

// Die vier Punkte des Pfeils in den Einheiten der Vorlage: CURSOR_SHAPE, die beiden Fluegel
// um `breite` von der Achse Spitze-Kerbe weg gestreckt. Spitze und Kerbe bleiben stehen.
pub(crate) fn cursor_shape(breite: f32) -> [(f32, f32); 4] {
    let [spitze, rechts, kerbe, unten] = CURSOR_SHAPE;
    let (ax, ay) = (kerbe.0 - spitze.0, kerbe.1 - spitze.1);
    let len = (ax * ax + ay * ay).sqrt();
    let (ax, ay) = (ax / len, ay / len);
    let strecken = |p: (f32, f32)| {
        let (dx, dy) = (p.0 - spitze.0, p.1 - spitze.1);
        let (laengs, quer) = (dx * ax + dy * ay, -dx * ay + dy * ax);
        let quer = quer * breite;
        (spitze.0 + laengs * ax - quer * ay, spitze.1 + laengs * ay + quer * ax)
    };
    [spitze, strecken(rechts), kerbe, strecken(unten)]
}

// Der Klickpunkt in den Einheiten der Vorlage: die aeusserste Stelle der runden Spitze, also
// CURSOR_ROUNDING von der Spitze der vier Punkte weg, mitten zwischen ihren beiden Kanten.
pub(crate) fn cursor_hotspot(form: &[(f32, f32); 4]) -> (f32, f32) {
    let spitze = form[0];
    let unit = |p: (f32, f32)| {
        let (dx, dy) = (p.0 - spitze.0, p.1 - spitze.1);
        let len = (dx * dx + dy * dy).sqrt();
        (dx / len, dy / len)
    };
    let (u1, u2) = (unit(form[1]), unit(form[3]));
    let (bx, by) = (u1.0 + u2.0, u1.1 + u2.1);
    let len = (bx * bx + by * by).sqrt();
    (spitze.0 - bx / len * CURSOR_ROUNDING, spitze.1 - by / len * CURSOR_ROUNDING)
}

// Der Zeiger fuer cursor.wgsl in Bildschirm-Pixeln. `tip_pos` ist die Stelle, an der die
// Spitze sitzt -- die uebliche Hotspot-Konvention: der Klickpunkt ist die Spitze der Form,
// nicht die Mitte ihrer Bounding-Box.
// `fx.scale` ist die Groesse, `fx.rotation_deg` dreht ihn beim Ziehen und `fx.additive` faerbt
// das Innere beim Druecken in der Akzentfarbe (s. cursor_fx).
//
// Gedreht und skaliert wird um die SPITZE, nicht um die Mitte: die Spitze ist der Punkt, auf
// den man zeigt, und der darf sich beim Klicken nicht verschieben.
pub(crate) fn cursor_polygons(tip_pos: (f32, f32), screen_px: (f32, f32), fx: CursorFxFrame) -> CursorUniform {
    let form = cursor_shape(fx.width);
    let tip = cursor_hotspot(&form);
    let scale = CURSOR_HEIGHT_PX / CURSOR_HEIGHT_UNITS * fx.scale;
    let (sin_fx, cos_fx) = fx.rotation_deg.to_radians().sin_cos();
    // Richtung in Einheiten -> Bildschirm (skaliert und gedreht), und dasselbe fuer Punkte.
    let dreh = |(dx, dy): (f32, f32)| ((dx * cos_fx - dy * sin_fx) * scale, (dx * sin_fx + dy * cos_fx) * scale);
    let to_screen = |p: (f32, f32)| {
        let (dx, dy) = dreh((p.0 - tip.0, p.1 - tip.1));
        (tip_pos.0 + dx, tip_pos.1 + dy)
    };

    let pfeil = form.map(to_screen);
    let mitte = to_screen(CURSOR_CIRCLE_CENTER);
    // Der Schatten faellt immer in dieselbe Richtung: das Licht dreht sich nicht mit dem Zeiger.
    let schatten = (CURSOR_SHADOW_OFFSET.0 * scale, CURSOR_SHADOW_OFFSET.1 * scale);
    let rundung = CURSOR_ROUNDING * scale;
    // Ohne Ring: ein stark negativer Radius -- der Abstand zu ihm ist dann ueberall riesig,
    // er deckt nichts ab. Die Zeichenflaeche richtet sich nur nach dem Pfeil.
    let ring_radius = if fx.circle { CURSOR_CIRCLE_RADIUS * scale } else { -1.0e6 };

    // Zeichenflaeche = alles, was die Form und ihr Schatten erreichen, plus Rand, damit die
    // Kantenglaettung (greift bis ~0.5 px ausserhalb der Form) nicht abgeschnitten wird.
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let kreise = pfeil.iter().map(|&p| (p, rundung)).chain(fx.circle.then_some((mitte, ring_radius)));
    for ((x, y), r) in kreise {
        for (sx, sy) in [(0.0, 0.0), schatten] {
            x0 = x0.min(x + sx - r);
            y0 = y0.min(y + sy - r);
            x1 = x1.max(x + sx + r);
            y1 = y1.max(y + sy + r);
        }
    }
    let pad = CURSOR_AA_PADDING_PX;
    // Die Akzentfarbe des Themes (schon linear, wie die Oberflaeche rechnet): Akzent, Akzent 2
    // oder ihre Mischung, je nach Taste (s. CursorFx::tint).
    let (a1, a2) = (theme::get(theme::Slot::Accent), theme::get(theme::Slot::Accent2));
    let [ar, ag, ab] = std::array::from_fn(|i| a1[i] + (a2[i] - a1[i]) * fx.right);
    CursorUniform {
        points: [[pfeil[0].0, pfeil[0].1, pfeil[1].0, pfeil[1].1], [pfeil[2].0, pfeil[2].1, pfeil[3].0, pfeil[3].1]],
        ring: [mitte.0, mitte.1, schatten.0, schatten.1],
        colors: [COLOR_CURSOR_OUTLINE, [COLOR_CURSOR_FILL[0], COLOR_CURSOR_FILL[1], COLOR_CURSOR_FILL[2], fx.fill_alpha.clamp(0.0, 1.0)]],
        additive: [ar, ag, ab, fx.additive * CURSOR_PRESS_TINT],
        bounds: [x0 - pad, y0 - pad, x1 + pad, y1 + pad],
        screen: [screen_px.0, screen_px.1, CURSOR_OUTLINE * fx.outline * scale, ring_radius],
        form: [rundung, CURSOR_SHADOW_ALPHA, 0.0, 0.0],
    }
}
