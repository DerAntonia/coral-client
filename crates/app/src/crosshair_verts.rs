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
    // 5 = Kreis in Fadenkreuz-Farbe, 6 = Kreis schwarz (Kontur).
    // Als Attribut und nicht als Uniform, weil alles in EINEM Zeichenaufruf rausgeht.
    pub(crate) kind: f32,
    // Deckkraft (Pfeil-Anzeige hat eine eigene).
    pub(crate) alpha: f32,
    // Nur fuer Kreise (Arten 5 und 6): innerer und aeusserer Radius in Pixeln; glyph_uv ist
    // dann der Abstand zur Kreismitte in Pixeln.
    pub(crate) circle: [f32; 2],
}

pub(crate) const CROSSHAIR_KIND_FILL: f32 = 0.0;
pub(crate) const CROSSHAIR_KIND_OUTLINE: f32 = 1.0;
pub(crate) const CROSSHAIR_KIND_DISC: f32 = 5.0;
pub(crate) const CROSSHAIR_KIND_DISC_OUTLINE: f32 = 6.0;
pub(crate) const CROSSHAIR_KIND_GLYPH: f32 = 2.0;
pub(crate) const CROSSHAIR_KIND_GLYPH_BLACK: f32 = 3.0;
pub(crate) const CROSSHAIR_KIND_GLYPH_WHITE: f32 = 4.0;

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
    // Schrift (GpuState::hud_font).
    pub(crate) font: FontKind,
    pub(crate) alpha: f32,
}

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
    // Kontur um den Ring (Settings::crosshair_ring_outline).
    pub(crate) ring_outline: bool,
    // Feuer-Modus "Fadenkreuz" beim Brennen: diese Farbe nur fuer Kreuz und Ring -- die
    // Zahlen (Pfeile, Countdown) behalten die normale Fadenkreuz-Farbe.
    pub(crate) fire_rgb: Option<[f32; 3]>,
}

// Ring um `center_px`, ab 12 Uhr im Uhrzeigersinn bis `fraction` des Umfangs (verbleibende
// Zeit) -- als Segment-Vierecke. Mit Kontur zuerst ein breiterer schwarzer Ring.
pub(crate) fn crosshair_ring_vertices(center_px: (f32, f32), fraction: f32, screen_px: (f32, f32), outline: bool) -> Vec<CrosshairVertex> {
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
    let (r, t) = (SHIELD_RING_RADIUS_PX, SHIELD_RING_THICKNESS_PX / 2.0);
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

pub(crate) fn rotate_point(p: (f32, f32), center: (f32, f32), degrees: f32) -> (f32, f32) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (p.0 - center.0, p.1 - center.1);
    (center.0 + dx * cos - dy * sin, center.1 + dx * sin + dy * cos)
}

// Die Punkte des Zeigers in Bildschirm-Pixeln: erst die Kite-Form, dann der Griff. `tip_pos` ist die Position, an der die
// Pfeilspitze sitzt -- das ist die uebliche Hotspot-Konvention: der Klickpunkt ist die
// Spitze der Form, nicht die Mitte ihrer Bounding-Box.
// `fx_scale` staucht den Zeiger beim Druecken, `fx_rotation_deg` dreht ihn beim Ziehen und
// `fx_additive` faerbt den Kern beim Druecken in der Akzentfarbe -- alle drei kommen aus cursor_fx und sind 1.0 / 0.0 /
// 0.0, solange nichts passiert.
//
// Gedreht und skaliert wird um die SPITZE, nicht um die Mitte: die Spitze ist der Punkt, auf
// den man zeigt, und der darf sich beim Klicken nicht verschieben.
// Ersetzt die Ecke `corner` (zwischen `prev` und `next`) durch einen Kreisbogen mit Radius
// `r`, der beide Kanten tangential beruehrt -- als `n` Punkte von der prev- zur next-Seite.
pub(crate) fn round_corner(prev: (f32, f32), corner: (f32, f32), next: (f32, f32), r: f32, n: usize) -> Vec<(f32, f32)> {
    let unit = |p: (f32, f32)| {
        let (dx, dy) = (p.0 - corner.0, p.1 - corner.1);
        let len = (dx * dx + dy * dy).sqrt();
        (dx / len, dy / len)
    };
    let (u1, u2) = (unit(prev), unit(next));
    let halb = (u1.0 * u2.0 + u1.1 * u2.1).clamp(-1.0, 1.0).acos() / 2.0;
    let tangente = r / halb.tan();
    let bis = {
        let (bx, by) = (u1.0 + u2.0, u1.1 + u2.1);
        let len = (bx * bx + by * by).sqrt();
        (bx / len, by / len)
    };
    let mitte = (corner.0 + bis.0 * r / halb.sin(), corner.1 + bis.1 * r / halb.sin());
    let winkel = |p: (f32, f32)| (p.1 - mitte.1).atan2(p.0 - mitte.0);
    let a0 = winkel((corner.0 + u1.0 * tangente, corner.1 + u1.1 * tangente));
    let a1 = winkel((corner.0 + u2.0 * tangente, corner.1 + u2.1 * tangente));
    // Den kurzen Weg um die Aussenseite der Ecke nehmen.
    let mut spanne = a1 - a0;
    if spanne > std::f32::consts::PI {
        spanne -= std::f32::consts::TAU;
    } else if spanne < -std::f32::consts::PI {
        spanne += std::f32::consts::TAU;
    }
    (0..n)
        .map(|i| {
            let a = a0 + spanne * i as f32 / (n - 1) as f32;
            (mitte.0 + r * a.cos(), mitte.1 + r * a.sin())
        })
        .collect()
}

// Der Pfeil-Umriss im 100x130-Raum (noch ungedreht): CURSOR_KITE mit abgerundeten Fluegeln,
// die Fluegel um `breite` von der Mittelachse (x = 50) weg gestreckt.
pub(crate) fn cursor_kite_outline(breite: f32) -> Vec<(f32, f32)> {
    let [spitze, rechts, kerbe, links] = CURSOR_KITE;
    let strecken = |(x, y): (f32, f32)| (50.0 + (x - 50.0) * breite, y);
    let (rechts, links) = (strecken(rechts), strecken(links));
    let mut out = vec![spitze];
    out.extend(round_corner(spitze, rechts, kerbe, CURSOR_WING_RADIUS, CURSOR_WING_ARC_POINTS));
    out.push(kerbe);
    out.extend(round_corner(kerbe, links, spitze, CURSOR_WING_RADIUS, CURSOR_WING_ARC_POINTS));
    out
}

pub(crate) fn cursor_polygons(tip_pos: (f32, f32), screen_px: (f32, f32), fx: CursorFxFrame) -> CursorUniform {
    let (fx_scale, fx_rotation_deg, fx_additive) = (fx.scale, fx.rotation_deg, fx.additive);
    let local_center = (50.0, 50.0);
    let rotate = |p: (f32, f32)| rotate_point(p, local_center, CURSOR_ROTATION_DEG);

    let kite: Vec<(f32, f32)> = cursor_kite_outline(fx.width).into_iter().map(rotate).collect();
    let tip = rotate(CURSOR_KITE[0]);

    let scale = CURSOR_HEIGHT_PX / 130.0 * fx_scale;
    // lokal (rotiert, unskaliert) -> Bildschirm-Pixel: skalieren, um die Spitze drehen, dann
    // so verschieben, dass die Spitze exakt auf tip_pos landet.
    let (sin_fx, cos_fx) = fx_rotation_deg.to_radians().sin_cos();
    let to_screen = |p: (f32, f32)| {
        let (dx, dy) = ((p.0 - tip.0) * scale, (p.1 - tip.1) * scale);
        (tip_pos.0 + dx * cos_fx - dy * sin_fx, tip_pos.1 + dx * sin_fx + dy * cos_fx)
    };

    let mitte = to_screen(rotate(CURSOR_CIRCLE_CENTER));
    let tail = [mitte, mitte];
    // Ohne Kreis: ein stark negativer Radius -- der Abstand zur "Kapsel" ist dann ueberall
    // riesig, sie deckt nichts ab. Die Zeichenflaeche richtet sich nur nach dem Pfeil.
    let tail_radius = if fx.circle { CURSOR_CIRCLE_RADIUS * scale } else { -1.0e6 };
    let flat: Vec<(f32, f32)> = kite.iter().map(|&p| to_screen(p)).chain(tail).collect();
    debug_assert_eq!(flat.len(), CURSOR_POINTS);
    let colors = [COLOR_CURSOR_OUTLINE, COLOR_CURSOR_FILL];

    // Je zwei Punkte teilen sich ein vec4 (s. cursor.wgsl: Uniform-Arrays brauchen
    // 16-Byte-Schrittweite, ein Array aus vec2 waere sonst mit Fuellbytes durchsetzt).
    let mut points = [[0.0; 4]; CURSOR_POINTS / 2];
    for (i, &(x, y)) in flat.iter().enumerate() {
        points[i / 2][(i % 2) * 2] = x;
        points[i / 2][(i % 2) * 2 + 1] = y;
    }

    // Zeichenflaeche = Bounding-Box aller Punkte plus Rand, damit die Kantenglaettung
    // (greift bis ~0.5 px ausserhalb der Form) nicht am Quadrand abgeschnitten wird.
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let r = tail_radius.max(0.0);
    let kapselrand = tail.iter().flat_map(|&(x, y)| [(x - r, y - r), (x + r, y + r)]);
    let pfeil = if fx.circle { &flat[..] } else { &flat[..CURSOR_KITE_POINTS] };
    for (x, y) in pfeil.iter().copied().chain(kapselrand) {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let pad = CURSOR_AA_PADDING_PX;
    // Die Akzentfarbe des Themes (schon linear, wie die Oberflaeche rechnet): Akzent, Akzent 2
    // oder ihre Mischung, je nach Taste (s. CursorFx::tint).
    let (a1, a2) = (theme::get(theme::Slot::Accent), theme::get(theme::Slot::Accent2));
    let [ar, ag, ab] = std::array::from_fn(|i| a1[i] + (a2[i] - a1[i]) * fx.right);
    let kontur = CURSOR_OUTLINE_PX;
    CursorUniform {
        points,
        colors,
        additive: [ar, ag, ab, fx_additive * CURSOR_PRESS_TINT],
        bounds: [x0 - pad, y0 - pad, x1 + pad, y1 + pad],
        screen: [screen_px.0, screen_px.1, kontur, tail_radius],
    }
}
