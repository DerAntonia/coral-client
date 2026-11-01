// Rechtecke, UI-Elemente, Overlays und Vertex-Erzeugung fuer Quads, Linien, Hitboxen und
// Graphen.

use crate::*;

#[derive(Clone, Copy)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Rect {
    // Unter den Bildschirmrand gerutschte Eintraege ganz aus dem Bild nehmen.
    pub(crate) fn clamp_bottom(self, screen_h: f32) -> Self {
        if self.y + self.h > screen_h { Self { y: -10_000.0, ..self } } else { self }
    }

    pub(crate) fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

// Immediate-Mode-UI-Primitive: eine gefuellte Flaeche bzw. ein Text-Run. Die App baut
// jeden Frame Listen davon, GpuState zeichnet sie.
pub(crate) struct UiRect {
    pub(crate) rect: Rect,
    pub(crate) color: [f32; 4],
    // Form (s. quad.wgsl): Eckenradius, Rahmenbreite nach innen samt Rahmenfarbe, und eine
    // weiche Kante in Pixeln (Schatten). Alles 0 = die alte harte Flaeche, ohne Mehrkosten.
    pub(crate) radius: f32,
    pub(crate) border: f32,
    pub(crate) border_color: [f32; 4],
    pub(crate) soft: f32,
    // Drehung um die Mitte (Bogenmass, im Uhrzeigersinn) -- fuer Pfeile und andere Formen.
    pub(crate) angle: f32,
    // Farbe am rechten Rand: von `color` links waagerecht dorthin verlaufend (nur eckige
    // Flaechen ohne Rahmen, Schatten und Drehung).
    pub(crate) color_right: Option<[f32; 4]>,
}

impl UiRect {
    // Grundwerte fuer `..UiRect::FLAT`: eckig, ohne Rahmen. rect/color setzt jeder Aufrufer.
    pub(crate) const FLAT: Self = Self {
        rect: Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
        color: [0.0; 4],
        radius: 0.0,
        border: 0.0,
        border_color: [0.0; 4],
        soft: 0.0,
        angle: 0.0,
        color_right: None,
    };

    pub(crate) const fn rounded(rect: Rect, color: [f32; 4], radius: f32) -> Self {
        Self { rect, color, radius, ..Self::FLAT }
    }

    // Flaeche mit Rahmen; der Rahmen liegt innerhalb von `rect`.
    pub(crate) const fn framed(rect: Rect, color: [f32; 4], radius: f32, border: f32, border_color: [f32; 4]) -> Self {
        Self { rect, color, radius, border, border_color, ..Self::FLAT }
    }

    // Weicher Schatten unter einer Flaeche: dieselbe Form, um `y` nach unten versetzt und mit
    // `blur` Pixeln Auslauf.
    pub(crate) fn shadow(rect: Rect, radius: f32, y: f32, blur: f32, alpha: f32) -> Self {
        Self { rect: Rect { y: rect.y + y, ..rect }, color: [0.0, 0.0, 0.0, alpha], radius, soft: blur, ..Self::FLAT }
    }
}

// Ein gerades Liniensegment fester Dicke, in Bildschirm-Pixeln (wie UiRect). Fuer echte
// Polylinien statt Punktwolken (s. App::build_debug_overlay) -- die vorhandene Pipeline
// kennt sonst nur achsenparallele Rechtecke, keine beliebig geneigten Strecken.
pub(crate) struct UiLine {
    pub(crate) p0: (f32, f32),
    pub(crate) p1: (f32, f32),
    pub(crate) color: [f32; 4],
    // Dicke in Pixeln, und wie breit der Rand nach aussen auf Deckkraft 0 auslaeuft
    // (Kantenglaettung ueber die Vertexfarbe, s. soft_line_verts). 0 = harte Kante.
    pub(crate) width: f32,
    pub(crate) feather: f32,
    // Statt der Strecke ein beliebiges Viereck (Ecken der Reihe nach), einfarbig -- fuer
    // Kreisringe wie das Spielmodus-Rad. p0/p1/width zaehlen dann nicht.
    pub(crate) quad: Option<[(f32, f32); 4]>,
    // Zum Viereck je Ecke eine eigene Farbe (sonst `color` ueberall) -- fuer Verlaeufe wie die
    // auslaufende Flaeche unter den F3-Graphen.
    pub(crate) quad_colors: Option<[[f32; 4]; 4]>,
    // Ganz oben zeichnen, nach dem Text und der Overlay-Ebene (die F3-Graphen liegen ueber
    // dem Scoreboard). Sonst vor dem Text.
    pub(crate) top: bool,
}

pub(crate) struct UiText {
    pub(crate) text: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) color: [f32; 4],
    // Vergroesserungsfaktor der Bitmap-Schrift, s. UI_SCALE_NORMAL.
    //
    // FLIESSKOMMA, nicht ganzzahlig: die Bedienoberflaeche benutzt weiterhin ganze Stufen
    // (der Pixel-Font bleibt dort gestochen scharf), Weltbeschriftungen brauchen aber einen
    // stufenlosen Faktor -- sonst springt ein Namensschild beim Gehen in Sprüngen groesser
    // und kleiner, statt gleichmaessig mit der Entfernung zu schrumpfen.
    pub(crate) scale: f32,
}

// Die oberste UI-Ebene (derzeit: Benachrichtigungen, s. notifications). Sie wird NACH allem
// anderen gezeichnet, auch nach dem Text: der normale UI-Pass zeichnet typweise gebatcht
// (erst alle Flaechen, dann aller Text, s. build_hud) -- ohne eigene Ebene schiene jeder
// Menuetext durch eine Karte hindurch.
//
// Anders als die normale UI darf sie GEDREHT sein: jede Gruppe (eine Karte) dreht sich als
// Ganzes, Flaechen wie Text, um ihren Drehpunkt (s. notifications: gezogene Karten kippen).
#[derive(Default)]
pub(crate) struct UiOverlay {
    pub(crate) groups: Vec<UiOverlayGroup>,
}

// Wo eine Overlay-Gruppe in den Vertexpuffern liegt (s. GpuState::overlay_vertices).
pub(crate) struct OverlayRange {
    pub(crate) quads: std::ops::Range<u32>,
    pub(crate) text: std::ops::Range<u32>,
    // Laeufe wie bei shape_sprites, Anfang relativ zu den Overlay-Sprites.
    pub(crate) sprites: Vec<(SpriteAtlas, u32, u32)>,
    // Schriftwechsel wie bei shape_text, Vertex relativ zum Anfang des Gruppentexts.
    pub(crate) schriften: Vec<(u32, FontKind)>,
    pub(crate) clip: Option<Rect>,
}

// Was overlay_vertices fuer alle Gruppen zusammen liefert.
pub(crate) struct OverlayVertices {
    pub(crate) quads: Vec<Vertex>,
    pub(crate) text: Vec<TextVertex>,
    pub(crate) sprites: Vec<TextVertex>,
    pub(crate) ranges: Vec<OverlayRange>,
}

pub(crate) struct UiOverlayGroup {
    pub(crate) rects: Vec<UiRect>,
    pub(crate) texts: Vec<UiText>,
    // Texturierte Quads (z.B. Skin-Koepfe), gezeichnet nach den Flaechen, vor dem Text.
    pub(crate) sprites: Vec<UiSprite>,
    // Drehpunkt in Bildschirmpixeln und Winkel (rad, im Uhrzeigersinn).
    pub(crate) pivot: (f32, f32),
    pub(crate) angle: f32,
    // Nur darin zeichnen (Scissor, Bildschirmpixel; ungedreht gemeint), z.B. die Eintraege
    // einer gleitend gescrollten Liste, die am Rand halb hereinragen.
    pub(crate) clip: Option<Rect>,
}

// Eine NDC-Position um einen Punkt in Bildschirmpixeln drehen. Die Drehung muss in PIXELN
// passieren: NDC ist bei einem Breitbildschirm waagerecht gestaucht, eine Drehung dort
// verzerrte die Karte zur Raute.
pub(crate) fn rotate_ndc(position: [f32; 2], pivot: (f32, f32), angle: f32, screen_px: (f32, f32)) -> [f32; 2] {
    let (w, h) = screen_px;
    let px = (f32::midpoint(position[0], 1.0) * w, (1.0 - position[1]) / 2.0 * h);
    let (x, y) = notifications::rotate_about(px, pivot, angle);
    pixel_to_ndc(x, y, screen_px)
}

// Haengt die Vertices der Overlay-Ebene hinten an `main` an und liefert, wo die normale UI
// endet. Reicht der Puffer nicht fuer beides, weicht die normale UI: die Overlay-Ebene liegt
// obenauf und besteht nur aus wenigen Karten. `capacity` ist ein Vielfaches von
// VERTS_PER_QUAD, das Abschneiden trennt also nie ein Quad.
// Overlay-Sprites hinter die normalen, soweit der Puffer reicht. Anders als append_overlay
// wird nichts Normales abgeschnitten: dessen Laeufe zeigen auf feste Stellen.
pub(crate) fn append_overlay_sprites(main: &mut Vec<TextVertex>, overlay: &[TextVertex]) -> u32 {
    let cap = (MAX_UI_SPRITES + 1) * VERTS_PER_QUAD;
    let split = main.len();
    main.extend_from_slice(&overlay[..overlay.len().min(cap.saturating_sub(split))]);
    split as u32
}

pub(crate) fn append_overlay<T: Copy>(main: &mut Vec<T>, overlay: &[T], capacity: usize) -> u32 {
    let overlay = &overlay[..overlay.len().min(capacity)];
    main.truncate(capacity - overlay.len());
    let split = main.len() as u32;
    main.extend_from_slice(overlay);
    split
}

// --- Vertex-Typen ---

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    pub(crate) position: [f32; 2],
    pub(crate) color: [f32; 4],
    // Nur fuer geformte Flaechen (UiRect mit Radius/Rahmen/weicher Kante, s. rect_verts):
    // xy = Pixel relativ zur Mitte der Form, z = weiche Kante; shape = halbe Breite, halbe
    // Hoehe, Radius, Rahmenbreite. shape.x == 0 heisst: einfach `color`.
    pub(crate) local: [f32; 4],
    pub(crate) shape: [f32; 4],
    pub(crate) border_color: [f32; 4],
}

impl Vertex {
    pub(crate) const PLAIN: Self = Self { position: [0.0; 2], color: [0.0; 4], local: [0.0; 4], shape: [0.0; 4], border_color: [0.0; 4] };
}

// Die komplette Zeigerform fuer cursor.wgsl: alle Punkte in Bildschirm-Pixeln statt einer
// Dreiecksliste. Die Form wird im Fragment-Shader analytisch ausgewertet und dadurch
// kantengeglaettet (s. cursor.wgsl).
// Was die Zeiger-Effekte in DIESEM Frame ergeben (s. cursor_fx). Als Struktur statt dreier
// f32-Parameter: drei gleichartige Zahlen in einer Reihe verwechselt man, und der Fehler waere
// still -- der Zeiger saehe nur ein bisschen falsch aus.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CursorFxFrame {
    pub(crate) scale: f32,
    pub(crate) rotation_deg: f32,
    pub(crate) additive: f32,
    // Farbe des Leuchtens als Anteil von Akzent 2 (s. CursorFx::tint): 0 links, 1 rechts,
    // 0.5 beide.
    pub(crate) right: f32,
    // Kreis unter dem Pfeil zeichnen (Settings::cursor_circle).
    pub(crate) circle: bool,
    // Breite des Pfeils als Faktor (Settings::cursor_width / 100).
    pub(crate) width: f32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CursorUniform {
    // Je zwei aufeinanderfolgende Punkte in einem vec4.
    pub(crate) points: [[f32; 4]; CURSOR_POINTS / 2],
    // [Kontur, Kern].
    pub(crate) colors: [[f32; 4]; 2],
    // Akzentfarbe beim Druecken (rgb linear, a = wie weit der Kern dorthin gemischt wird).
    pub(crate) additive: [f32; 4],
    // Zeichenflaeche (x0, y0, x1, y1) in Pixeln.
    pub(crate) bounds: [f32; 4],
    // Bildschirmgroesse in Pixeln (xy), z = Konturdicke, w = Kapselradius (beides Pixel).
    pub(crate) screen: [f32; 4],
}

// Bildschirm-Pixel -> NDC (-1..1, Y nach unten in Pixeln, nach oben in NDC). Gemeinsam von
// quad_verts und dem Cursor-Polygon genutzt, damit beide garantiert dieselbe Umrechnung
// verwenden statt zwei Kopien, die auseinanderlaufen koennten.
pub(crate) fn pixel_to_ndc(px: f32, py: f32, screen_px: (f32, f32)) -> [f32; 2] {
    let (w, h) = screen_px;
    [(px / w) * 2.0 - 1.0, 1.0 - (py / h) * 2.0]
}

// Ein UiRect als zwei Dreiecke. Geformte Flaechen bekommen ein um die weiche Kante (plus ein
// Pixel Kantenglaettung) groesseres Viereck, die Form selbst rechnet der Fragment-Shader.
pub(crate) fn rect_verts(r: &UiRect, screen_px: (f32, f32)) -> [Vertex; 6] {
    let Rect { x, y, w, h } = r.rect;
    if r.radius <= 0.0 && r.border <= 0.0 && r.soft <= 0.0 && r.angle == 0.0 {
        let mut q = quad_verts(x, y, x + w, y + h, r.color, screen_px);
        if let Some(rechts) = r.color_right {
            // Ecken 1, 2 und 4 liegen rechts (s. quad_verts).
            for i in [1, 2, 4] {
                q[i].color = rechts;
            }
        }
        return q;
    }
    let pad = r.soft + 1.0;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let shape = [w / 2.0, h / 2.0, r.radius.min(w / 2.0).min(h / 2.0), r.border];
    let (sin, cos) = r.angle.sin_cos();
    let v = |px: f32, py: f32| Vertex {
        // Gedreht wird nur die Lage auf dem Bildschirm; die Form rechnet der Shader weiter im
        // ungedrehten Raum der Flaeche.
        position: pixel_to_ndc(cx + (px - cx) * cos - (py - cy) * sin, cy + (px - cx) * sin + (py - cy) * cos, screen_px),
        color: r.color,
        local: [px - cx, py - cy, r.soft, 0.0],
        shape,
        border_color: r.border_color,
    };
    let (x0, y0, x1, y1) = (x - pad, y - pad, x + w + pad, y + h + pad);
    [v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y0), v(x1, y1), v(x0, y1)]
}

pub(crate) fn quad_verts(x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4], screen_px: (f32, f32)) -> [Vertex; 6] {
    let to_ndc = |px: f32, py: f32| Vertex { position: pixel_to_ndc(px, py, screen_px), color, ..Vertex::PLAIN };
    [to_ndc(x0, y0), to_ndc(x1, y0), to_ndc(x1, y1), to_ndc(x0, y0), to_ndc(x1, y1), to_ndc(x0, y1)]
}

// Ein Liniensegment als duennes, um seine Richtung gedrehtes Quad -- die Senkrechte auf
// (p1-p0) verschiebt beide Endpunkte um die halbe Dicke nach links/rechts, macht daraus
// ein Viereck mit 4 Ecken, das dann wie jedes andere Quad trianguliert wird (gleiches
// Vertex-Format wie quad_verts, laeuft ueber dieselbe Pipeline).
pub(crate) fn free_quad_verts_colored(q: [(f32, f32); 4], c: [[f32; 4]; 4], screen_px: (f32, f32)) -> [Vertex; 6] {
    let v = |i: usize| Vertex { position: pixel_to_ndc(q[i].0, q[i].1, screen_px), color: c[i], ..Vertex::PLAIN };
    [v(0), v(1), v(2), v(0), v(2), v(3)]
}

pub(crate) fn line_quad_verts(line: &UiLine, thickness: f32, screen_px: (f32, f32)) -> [Vertex; 6] {
    let (dx, dy) = (line.p1.0 - line.p0.0, line.p1.1 - line.p0.1);
    let len = (dx * dx + dy * dy).sqrt();
    let (nx, ny) = if len < 0.0001 { (0.0, 0.0) } else { (-dy / len * thickness * 0.5, dx / len * thickness * 0.5) };
    let to_ndc = |px: f32, py: f32| Vertex { position: pixel_to_ndc(px, py, screen_px), color: line.color, ..Vertex::PLAIN };
    let a = to_ndc(line.p0.0 + nx, line.p0.1 + ny);
    let b = to_ndc(line.p1.0 + nx, line.p1.1 + ny);
    let c = to_ndc(line.p1.0 - nx, line.p1.1 - ny);
    let d = to_ndc(line.p0.0 - nx, line.p0.1 - ny);
    [a, b, c, a, c, d]
}

// Eine kantengeglaettete Strecke: der Kern (volle Deckkraft) und rundherum ein Saum von
// `feather` Pixeln, dessen Aussenkante Deckkraft 0 hat -- die GPU interpoliert die Farbe
// dazwischen, das ergibt weiche Kanten ohne eigenen Shader. Der Saum laeuft auch um die
// Enden, sonst waeren die schraegen Stirnseiten treppig. Zwei Dreiecke Kern plus vier
// Trapeze Saum.
pub(crate) fn soft_line_verts(line: &UiLine, screen_px: (f32, f32)) -> [Vertex; 30] {
    let (dx, dy) = (line.p1.0 - line.p0.0, line.p1.1 - line.p0.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-4);
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy, ux);
    // Kern um einen halben Saum schmaler, Aussenkante um einen halben breiter: die Linie
    // behaelt so ihre Dicke (die 50-%-Kante liegt genau auf width).
    let halb = (line.width - line.feather).max(0.0) / 2.0;
    let rand = line.feather;
    let punkt = |p: (f32, f32), laengs: f32, quer: f32| (p.0 + ux * laengs + nx * quer, p.1 + uy * laengs + ny * quer);
    let r = rand / 2.0;
    // Innen (Kern) und aussen (Saum), je vier Ecken: p0 links, p1 links, p1 rechts, p0 rechts.
    // Auch laengs liegt die 50-%-Kante genau auf den Endpunkten.
    let innen = [punkt(line.p0, r, halb), punkt(line.p1, -r, halb), punkt(line.p1, -r, -halb), punkt(line.p0, r, -halb)];
    let aussen = [
        punkt(line.p0, -r, halb + rand),
        punkt(line.p1, r, halb + rand),
        punkt(line.p1, r, -halb - rand),
        punkt(line.p0, -r, -halb - rand),
    ];
    let voll = line.color;
    let leer = [line.color[0], line.color[1], line.color[2], 0.0];
    let v = |p: (f32, f32), c: [f32; 4]| Vertex { position: pixel_to_ndc(p.0, p.1, screen_px), color: c, ..Vertex::PLAIN };
    let mut out = [v(innen[0], voll); 30];
    let mut n = 0;
    let mut tri = |a: Vertex, b: Vertex, c: Vertex| {
        out[n] = a;
        out[n + 1] = b;
        out[n + 2] = c;
        n += 3;
    };
    tri(v(innen[0], voll), v(innen[1], voll), v(innen[2], voll));
    tri(v(innen[0], voll), v(innen[2], voll), v(innen[3], voll));
    for i in 0..4 {
        let j = (i + 1) % 4;
        tri(v(innen[i], voll), v(aussen[i], leer), v(aussen[j], leer));
        tri(v(innen[i], voll), v(aussen[j], leer), v(innen[j], voll));
    }
    out
}

// Ein Kanten-Segment der Hitbox-Anzeige (s. App::build_hitbox_vertices) als sechs Vertices
// (zwei Dreiecke) zwischen zwei bereits AUGENRELATIVEN Punkten. Hier wird NICHTS verbreitert:
// alle sechs Vertices tragen dieselben beiden Endpunkte, die Breite entsteht erst im
// Vertex-Shader nach der Projektion (s. block.wgsl::vs_hitbox).
//
// Genau darin liegt der Unterschied zu den drei fruehren CPU-Fassungen, die der Reihe nach an
// Blickwinkel-Verkuerzung, an einer Box-weiten Entfernungsnaeherung und zuletzt am
// FOV-Stretch zum Bildschirmrand hin gescheitert sind: eine Breite, die VOR der Projektion
// in Weltlaenge festgelegt wird, kann nach der Projektion gar nicht ueberall dieselbe
// Pixelzahl ergeben -- die Projektion zieht den Bildrand staerker auf als die Mitte.
pub(crate) fn push_hitbox_edge(out: &mut Vec<render::HitboxVertex>, a: Vec3, b: Vec3, color: [f32; 4], half_width: f32) {
    if (b - a).length_squared() < 1e-10 {
        return;
    }
    let pos_a = [a.x, a.y, a.z];
    let pos_b = [b.x, b.y, b.z];
    // (Seite, Endpunkt) je Vertex -- zwei Dreiecke, die das Band aufspannen.
    const CORNERS: [[f32; 2]; 6] = [[-1.0, 0.0], [-1.0, 1.0], [1.0, 1.0], [-1.0, 0.0], [1.0, 1.0], [1.0, 0.0]];
    out.extend(CORNERS.map(|side_and_end| render::HitboxVertex { pos_a, pos_b, side_and_end, color, half_width }));
}

// Anteil der Entfernung, um den der Blockumriss zum Auge rueckt (s.
// build_block_selection_vertices): bei 5 Bloecken 1,5 cm -- genug gegen das Ringen mit der
// Flaeche darunter, zu wenig, um durch einen echten Block davor zu scheinen.
pub(crate) const OUTLINE_PULL: f64 = 0.003;

// Abdunklung des Weltbilds hinter den Menues in Prozent (s. world_backdrop_visible) -- Vorgabe
// dunkel genug, dass Text darauf lesbar bleibt, hell genug, dass man die Welt erkennt.
pub(crate) const MENU_BACKDROP_OPACITY_DEFAULT: u32 = 55;
pub(crate) const SCROLL_INERTIA_DEFAULT: u32 = 50;
// Scrollweite: wie weit eine Mausrad-Raste jede Liste bewegt, in Prozent ihres eigenen
// Schritts (eine Zeile, eine Karte, ...).
pub(crate) const SCROLL_DISTANCE_DEFAULT: u32 = 100;
pub(crate) const SCROLL_DISTANCE_MIN: u32 = 25;
pub(crate) const SCROLL_DISTANCE_MAX: u32 = 400;
pub(crate) const SCROLL_DISTANCE_STEP: u32 = 25;

// Breite der Container-Sheets in GUI-Pixeln (176, wie inventory::INVENTORY_SHEET_W).
pub(crate) const INVENTORY_GUI_W: f32 = 176.0;

// Vanillas Blockumriss (WorldRenderer.drawBlockOutline): schwarz mit 40 % Deckkraft
// (ColorHelper 0x66000000), Linienbreite max(2.5, Fensterbreite/1920*2.5) Pixel
// (RenderLayers.lines). Rueckgabe: (rgba, halbe Breite in Pixeln).
pub(crate) fn vanilla_block_outline(screen_width: f32) -> ([f32; 4], f32) {
    let breite = (screen_width / 1920.0 * 2.5).max(2.5);
    ([0.0, 0.0, 0.0, 0.4], breite / 2.0)
}

// F3-Graphen (s. App::build_debug_overlay): gruen = Rechner gesamt, pink = Anteil von Coral.
pub(crate) const GRAPH_GREEN: [f32; 4] = [0.24, 0.86, 0.48, 1.0];
pub(crate) const GRAPH_PINK: [f32; 4] = [1.0, 0.40, 0.67, 1.0];
pub(crate) const GRAPH_LINE_THICKNESS: f32 = 2.0;
// Zwischenpunkte je Messabstand fuer die gerundete Kurve.
pub(crate) const GRAPH_SUBDIVISIONS: usize = 3;

// Werte (0..100, aeltester vorn) als glatte Kurve: Catmull-Rom durch die Messpunkte, rechts
// buendig (der neueste Wert steht an der rechten Kante, aeltere wandern nach links hinaus).
pub(crate) fn graph_curve(area: Rect, werte: &[f32]) -> Vec<(f32, f32)> {
    let n = werte.len();
    let schritt = area.w / (sysstats::HISTORY_LEN.max(2) - 1) as f32;
    let x0 = area.x + area.w - (n.saturating_sub(1)) as f32 * schritt;
    let y = |v: f32| area.y + area.h * (1.0 - (v / 100.0).clamp(0.0, 1.0));
    let wert = |i: isize| werte[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::with_capacity(n * GRAPH_SUBDIVISIONS);
    for i in 0..n.saturating_sub(1) {
        let (p0, p1, p2, p3) = (wert(i as isize - 1), wert(i as isize), wert(i as isize + 1), wert(i as isize + 2));
        for k in 0..GRAPH_SUBDIVISIONS {
            let t = k as f32 / GRAPH_SUBDIVISIONS as f32;
            let v = 0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t);
            out.push((x0 + (i as f32 + t) * schritt, y(v)));
        }
    }
    if n > 0 {
        out.push((x0 + (n - 1) as f32 * schritt, y(werte[n - 1])));
    }
    out
}

// Flaeche unter der Kurve: an der Kurve `alpha` Deckkraft, nach unten auf 0 auslaufend.
pub(crate) fn push_graph_fill(lines: &mut Vec<UiLine>, area: Rect, kurve: &[(f32, f32)], farbe: [f32; 4], alpha: f32) {
    let unten = area.y + area.h;
    let oben = [farbe[0], farbe[1], farbe[2], alpha];
    let leer = [farbe[0], farbe[1], farbe[2], 0.0];
    for w in kurve.windows(2) {
        let (a, b) = (w[0], w[1]);
        lines.push(UiLine {
            p0: a,
            p1: b,
            color: oben,
            width: 0.0,
            feather: 0.0,
            quad: Some([a, b, (b.0, unten), (a.0, unten)]),
            quad_colors: Some([oben, oben, leer, leer]),
            top: true,
        });
    }
}

pub(crate) fn push_graph_line(lines: &mut Vec<UiLine>, kurve: &[(f32, f32)], farbe: [f32; 4]) {
    for w in kurve.windows(2) {
        lines.push(UiLine { p0: w[0], p1: w[1], color: farbe, width: GRAPH_LINE_THICKNESS, feather: 1.0, quad: None, quad_colors: None, top: true });
    }
}

// Lage der drei F3-Graphen unten rechts, von unten: CPU, RAM, GPU. Gross und ueber allem
// anderen HUD (auch dem Scoreboard): die Flaechen und Ueberschriften stehen in der Overlay-
// Ebene (s. debug_graph_overlay), die Kurven als UiLine mit top (s. render_world).
pub(crate) const DEBUG_GRAPH_W: f32 = 640.0;
pub(crate) const DEBUG_GRAPH_H: f32 = 190.0;
pub(crate) fn debug_graph_areas(gpu: &GpuState) -> [Rect; 3] {
    const RAND: f32 = 16.0;
    let (sw, sh) = gpu.size();
    let abstand = DEBUG_GRAPH_H + gpu.text_height(UI_SCALE_SMALL) + 14.0;
    std::array::from_fn(|i| Rect { x: sw - DEBUG_GRAPH_W - RAND, y: sh - RAND - DEBUG_GRAPH_H - i as f32 * abstand, w: DEBUG_GRAPH_W, h: DEBUG_GRAPH_H })
}

// Schwarze Flaeche und Ueberschrift eines Graphen.
pub(crate) fn push_stats_graph_frame(gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, area: Rect, titel: &str, serie: &sysstats::Series) {
    rects.push(UiRect { rect: area, color: [0.0, 0.0, 0.0, 1.0], ..UiRect::FLAT });
    let zeile = gpu.text_height(UI_SCALE_SMALL) + 2.0;
    let text = if serie.available { serie.label.clone() } else { format!("{titel}: n/v") };
    // Ueberschrift auf schwarzem Streifen, damit sie auch ueber dem Scoreboard lesbar ist.
    let breite = gpu.measure_text(&text, UI_SCALE_SMALL) + 12.0;
    rects.push(UiRect { rect: Rect { x: area.x, y: area.y - zeile - 2.0, w: breite, h: zeile + 2.0 }, color: [0.0, 0.0, 0.0, 1.0], ..UiRect::FLAT });
    texts.push(UiText { text, x: area.x + 6.0, y: area.y - zeile, color: COLOR_WHITE, scale: UI_SCALE_SMALL });
}

// Die Kurven eines Graphen: gruen (gesamt) und pink (Coral), je mit Flaeche darunter.
pub(crate) fn push_stats_graph(lines: &mut Vec<UiLine>, area: Rect, serie: &sysstats::Series) {
    if serie.history.len() < 2 {
        return;
    }
    let gesamt: Vec<f32> = serie.history.iter().map(|s| s.total).collect();
    let coral: Vec<f32> = serie.history.iter().map(|s| s.own).collect();
    let (kurve_g, kurve_c) = (graph_curve(area, &gesamt), graph_curve(area, &coral));
    push_graph_fill(lines, area, &kurve_g, GRAPH_GREEN, 0.30);
    push_graph_fill(lines, area, &kurve_c, GRAPH_PINK, 0.40);
    push_graph_line(lines, &kurve_g, GRAPH_GREEN);
    push_graph_line(lines, &kurve_c, GRAPH_PINK);
}

// "+"-Fadenkreuz aus zwei sich ueberlappenden Balken (Rechtecken), mittig auf dem
// Bildschirm, das immer die am staerksten unterscheidbare Farbe zum Untergrund zeigt (s.
// crosshair.wgsl: Farbton +180 Grad, Saettigung/Helligkeit (HSV) immer maximal -- nicht
// einfach RGB-Invertierung, die auf neutralen/entsaettigten Untergruenden kaum sichtbar
// waere). Zwei vorherige Ansaetze scheiterten auf diesem (laut eigener Meldung "not a
// conformant Vulkan implementation") RADV-Treiber: (1) ein GPU-Blend-Trick
// (srcFarbe*(1-dstFarbe)) wurde nicht umgesetzt, das Kreuz blieb einfarbig; (2) eine
// Kopie des Bildbereichs in eine eigene kleine Textur (copy_texture_to_texture) lieferte
// meist Schwarz statt des echten Bildinhalts, selbst mit hartem device.poll(Wait)
// dazwischen. Diese Version verzichtet auf jede Kopie: der Crosshair-Shader sampelt
// direkt aus offscreen_view (derselben Textur, in die Welt+HUD gerade gezeichnet wurden).
// Linienstaerke von Kreuz und Kreis.
pub(crate) const CROSSHAIR_THICKNESS_PX: f32 = 2.0;
// Breite der schwarzen Kontur (s. Settings::crosshair_outline). Ein Pixel, bewusst fest.
pub(crate) const CROSSHAIR_OUTLINE_PX: f32 = 1.0;
// Luft zwischen Kreuz und dem runden Griff darum im HUD-Editor (s. hud_crosshair_rect).
pub(crate) const CROSSHAIR_HANDLE_MARGIN: f32 = 12.0;

// Formen des eingebauten Fadenkreuzes (Index = Settings::crosshair_shape), je mit dem
// Groessenbereich in Pixeln: kleinste, groesste, Standard. Die Groesse ist die ganze
// Ausdehnung -- Balkenlaenge beim Kreuz, Durchmesser bei Punkt und Kreis.
pub(crate) const CROSSHAIR_SHAPES: [(&str, u32, u32, u32); 4] = [("Kreuz", 4, 40, 10), ("Punkt", 2, 24, 4), ("Kreis", 6, 48, 12), ("Offenes Kreuz", 8, 48, 14)];
// Hinter den eingebauten Formen steht in der Auswahl noch "Eigene" (Pixel-Editor, s.
// Settings::crosshair_custom).
pub(crate) const CROSSHAIR_SHAPE_CUSTOM: usize = CROSSHAIR_SHAPES.len();
pub(crate) const CROSSHAIR_SHAPE_DOT: usize = 1;
pub(crate) const CROSSHAIR_SHAPE_CIRCLE: usize = 2;
pub(crate) const CROSSHAIR_SHAPE_OPEN: usize = 3;
