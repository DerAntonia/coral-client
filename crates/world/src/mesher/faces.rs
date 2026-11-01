// Flaechen-Quads, Kreuz-Quads, Verdeckung, Glas, Laub und Wasser-UVs.

use super::*;

// Haengt die 6 Vertices (2 Dreiecke) fuer eine Seite einer achsenparallelen Box
// zwischen `min` und `max` (Weltkoordinaten) an `out` an. `dir` ist die Face-Normale,
// `uv` das Atlas-Rechteck (u0,v0,u1,v1) fuer diese Seite. Fuer volle Bloecke ist die Box
// genau eine Zelle (min=Blockposition, max=min+1); fuer Nicht-Wuerfel-Formen (Stufen,
// Zaeune, ...) eine der kleineren Boxen aus assets::shapes.
#[cfg(test)]
pub(crate) fn push_face_quad_for_test(out: &mut Vec<BlockVertex>, min: [f32; 3], max: [f32; 3], dir: [i32; 3]) {
    push_face_quad(out, min, max, dir, [1.0; 3], [0.0, 0.0, 1.0, 1.0]);
}

pub(super) fn push_face_quad(out: &mut Vec<BlockVertex>, min: [f32; 3], max: [f32; 3], dir: [i32; 3], color: [f32; 3], uv: [f32; 4]) {
    push_quad_with_uv(out, face_quad_corners(min, max, dir), color, uv);
}

// Wie push_face_quad, aber mit einem Helligkeitsfaktor JE ECKE (weiche Beleuchtung). Die
// Faktoren stehen in derselben Reihenfolge wie face_quad_corners -- beide Funktionen
// muessen dieselbe Eckreihenfolge benutzen, deshalb gibt es sie nur einmal.
pub(super) fn push_face_quad_ao(out: &mut Vec<BlockVertex>, min: [f32; 3], max: [f32; 3], dir: [i32; 3], color: [f32; 3], ao: [f32; 4], uv: [f32; 4]) {
    let corners = face_quad_corners(min, max, dir);
    let [u0, v0, u1, v1] = uv;
    let corner_uv = [[u0, v1], [u0, v0], [u1, v0], [u1, v1]];
    let vert = |i: usize| BlockVertex { position: corners[i], color: [color[0] * ao[i], color[1] * ao[i], color[2] * ao[i]], uv: corner_uv[i] };
    out.extend_from_slice(&[vert(0), vert(1), vert(2), vert(0), vert(2), vert(3)]);
}

// Die vier Eckpunkte einer Quaderseite, in der Reihenfolge, in der push_quad_with_uv sie zu
// zwei Dreiecken verbindet.
pub(super) fn face_quad_corners(min: [f32; 3], max: [f32; 3], dir: [i32; 3]) -> [[f32; 3]; 4] {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;

    // Vier Eckpunkte der jeweiligen Seite, in einer Reihenfolge, die zwei Dreiecke
    // ergibt. Cull-Mode ist bewusst deaktiviert (s. render-Crate), daher ist die
    // Wicklungsrichtung hier unkritisch fuer die Sichtbarkeit.
    match dir {
        [1, 0, 0] => [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
        [-1, 0, 0] => [[x0, y0, z1], [x0, y1, z1], [x0, y1, z0], [x0, y0, z0]],
        [0, 1, 0] => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        [0, -1, 0] => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        [0, 0, 1] => [[x1, y0, z1], [x1, y1, z1], [x0, y1, z1], [x0, y0, z1]],
        [0, 0, -1] => [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
        _ => unreachable!("Face-Richtung muss ein Einheitsvektor sein"),
    }
}

// Zwei diagonale Ebenen ("X"-Form) fuer Kreuz-Bloecke (Zuckerrohr, Gras, Blumen, ...) --
// entspricht Vanillas "cross"/"tinted_cross"-Modell: zwei um 45 Grad gegeneinander
// gedrehte, volle Ebenen von Eckpunkt zu gegenueberliegendem Eckpunkt. `tint` wird direkt
// als Farbe verwendet (kein Richtungs-Shading, s. Vanillas "shade": false in cross.json).
//
// JEDE Ebene wird ZWEIMAL ausgegeben, einmal pro Wicklungsrichtung. Frueher genuegte ein
// Quad pro Ebene, weil der Cull-Mode aus war -- seit die Opak-Pipeline rueckseitige Flaechen
// verwirft (s. render-Crate), waere eine Pflanze sonst von genau einer Seite unsichtbar.
// Vanilla macht dasselbe: das cross-Modell definiert pro Ebene beide Faces (north UND
// south). Die Verdopplung kostet praktisch nichts -- Kreuz-Bloecke sind gegenueber
// Vollwuerfeln verschwindend selten, und ohne sie waere Culling gar nicht erst moeglich.
pub(super) fn push_cross_quads(out: &mut Vec<BlockVertex>, origin: ColumnOrigin, x: i32, y: i32, z: i32, tint: [f32; 3], uv: [f32; 4]) {
    let (x0, y0, z0) = origin.local(x, y, z);
    let (x1, y1, z1) = (x0 + 1.0, y0 + 1.0, z0 + 1.0);
    let plane_a = [[x0, y0, z0], [x0, y1, z0], [x1, y1, z1], [x1, y0, z1]];
    let plane_b = [[x0, y0, z1], [x0, y1, z1], [x1, y1, z0], [x1, y0, z0]];
    for plane in [plane_a, plane_b] {
        push_quad_with_uv(out, plane, tint, uv);
        // Umgekehrte Eckenreihenfolge = umgekehrte Wicklung = die Rueckseite derselben
        // Ebene. Die UVs laufen dadurch gespiegelt, was bei den symmetrischen
        // Kreuz-Texturen nicht auffaellt -- und Vanilla spiegelt die Rueckseite ebenso.
        let rueckseite = [plane[3], plane[2], plane[1], plane[0]];
        push_quad_with_uv(out, rueckseite, tint, uv);
    }
}

// Bloecke, deren echte Vanilla-Kollisions-/Verdeckungsform NIE eine Nachbar-Flaeche voll
// abdeckt, obwohl ihr RENDER-Modell (assets::shapes, aus dem BLOCK_SHAPES kommt) das an
// einzelnen Grenzflaechen so aussehen laesst. Kaktus ist der bekannte Fall: sein Modell hat
// ein volles 0..16-Element mit "cullface":"up"/"down" (siehe cactus.json) -- das ist aber
// NUR eine Modellierungs-Abkuerzung, damit KAKTUS SELBST seine eigene Ober-/Unterseite
// gegen einen sturen Nachbarn (Sand) culln kann. Die tatsaechliche Vanilla-Form ist auf
// allen Seiten um 1px eingerueckt (nie randstaendig), verdeckt also NIE die Flaeche eines
// Nachbarn -- weder von oben/unten (der Sand UNTER einem Kaktus zeigt in echtem Minecraft
// einen sichtbaren schmalen Rand seiner Oberseite) noch seitlich. Ohne diese Ausnahme
// culln wir faelschlich anhand des vollen Render-Elements und der Nachbarblock (z.B. Sand
// unter dem Kaktus) wird komplett unsichtbar.
//
// Dazu alle Bloecke, die Vanilla als NICHT deckend fuehrt (Block.Settings.nonOpaque):
// Block.shouldDrawSide cullt nur gegen `isOpaque()`-Nachbarn. Falltueren und Tueren liegen
// als volle 16x16-Platte an der Zellkante und verdeckten hier die Wand dahinter -- durch ihre
// Loecher sah man dann ins Leere. Ebenso Spawner, Kupfergitter und Geruest.
pub(crate) fn never_occludes_neighbors(name: &str) -> bool {
    name == "cactus"
        || name.ends_with("_trapdoor")
        || name.ends_with("_door")
        || name.ends_with("_grate")
        || matches!(name, "spawner" | "trial_spawner" | "vault" | "scaffolding")
}

// Ob eine Boxflaeche an der Aussenkante der Block-Zelle (0..1 lokal) liegt -- nur dann
// ist der Nachbarblock ueberhaupt fuer das Culling relevant (s. Aufrufer). Eine Flaeche
// mitten in der Zelle (z.B. die Seite eines duennen Zaunpfostens) ist immer sichtbar.
pub(super) fn touches_cell_boundary(b: &ShapeBox, offset: [i32; 3]) -> bool {
    const EPS: f32 = 1e-4;
    match offset {
        [1, 0, 0] => b.max[0] > 1.0 - EPS,
        [-1, 0, 0] => b.min[0] < EPS,
        [0, 1, 0] => b.max[1] > 1.0 - EPS,
        [0, -1, 0] => b.min[1] < EPS,
        [0, 0, 1] => b.max[2] > 1.0 - EPS,
        [0, 0, -1] => b.min[2] < EPS,
        _ => unreachable!("Face-Richtung muss ein Einheitsvektor sein"),
    }
}

// FACE_DOWN=0,FACE_UP=1,FACE_NORTH=2,FACE_SOUTH=3,FACE_WEST=4,FACE_EAST=5 (s. assets::FACE_*)
// -- fuer jede Richtung die gegenueberliegende (fuer den Nachbar-Deckungstest: meine Ost-
// Flaeche kann nur durch die WEST-Flaeche des Nachbarn verdeckt werden).
pub(super) const OPPOSITE_FACE: [usize; 6] = [1, 0, 3, 2, 5, 4];

// Welche Koordinatenachse (Index in [f32;3]) die Flaechen-Normale bildet -- fuer den
// Nullstaerke-Ebenen-Test (s. Aufrufer): down/up=Y(1), north/south=Z(2), west/east=X(0).
// Nur noch von Tests gebraucht, seit die Nullstaerke-Kuerzung entfallen ist (s. dortigen
// Kommentar) -- bleibt stehen, weil die Zuordnung Flaeche->Achse jederzeit wieder gebraucht
// wird und sie nirgends sonst ausgeschrieben ist.
#[allow(dead_code)]
pub(super) const fn face_normal_axis(atlas_face: usize) -> usize {
    match atlas_face {
        0 | 1 => 1,
        2 | 3 => 2,
        _ => 0,
    }
}

// Die zwei Achsen-Indices (in [f32;3]), die in der Ebene der jeweiligen Flaeche liegen --
// z.B. die Ost/West-Flaeche (Normale = X-Achse) hat ihr Rechteck in Y/Z.
pub(super) const fn face_plane_axes(atlas_face: usize) -> (usize, usize) {
    match atlas_face {
        0 | 1 => (0, 2), // down/up: Normale Y -> Rechteck in X/Z
        2 | 3 => (0, 1), // north/south: Normale Z -> Rechteck in X/Y
        _ => (1, 2),     // west/east: Normale X -> Rechteck in Y/Z
    }
}

// Deckt IRGENDEINE Box des Nachbarn meine Flaeche `atlas_face` (meiner Box `my`, an der
// Grenze in Richtung `offset`) vollstaendig ab? Verdeckung braucht mehr als "Nachbar ist
// nicht Luft": der Nachbar muss an der GLEICHEN Grenze eine eigene Flaeche haben, die die
// volle Breite/Hoehe meiner Flaeche ueberdeckt -- eine Treppe, Stufe oder ein Zaun deckt
// eine Zellgrenze meist nur teilweise ab, s. Aufrufer-Kommentar.
// Liegt ein Modell-Viereck ganz in einer Grenzebene der Zelle? Dann: Richtung zum Nachbarn,
// die Flaeche (FACE_*) und seine Ausdehnung als flache Box fuer neighbor_covers_face.
pub(crate) fn boundary_quad(corners: &[[f32; 3]; 4]) -> Option<([i32; 3], usize, ShapeBox)> {
    const EPS: f32 = 1e-4;
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for c in corners {
        for i in 0..3 {
            min[i] = min[i].min(c[i]);
            max[i] = max[i].max(c[i]);
        }
    }
    // (Achse, Grenzwert) -> (Nachbar-Versatz, Flaeche)
    let (offset, face) = [
        (0, 0.0, [-1, 0, 0], 4usize),
        (0, 1.0, [1, 0, 0], 5),
        (1, 0.0, [0, -1, 0], 0),
        (1, 1.0, [0, 1, 0], 1),
        (2, 0.0, [0, 0, -1], 2),
        (2, 1.0, [0, 0, 1], 3),
    ]
    .into_iter()
    .find(|&(achse, grenze, _, _)| (min[achse] - grenze).abs() < EPS && (max[achse] - grenze).abs() < EPS)
    .map(|(_, _, o, f)| (o, f))?;
    Some((offset, face, ShapeBox { min, max, faces: [false; 6], content: [false; 6], uv: [None; 6] }))
}

pub(super) fn neighbor_covers_face(neighbor_boxes: &[ShapeBox], offset: [i32; 3], atlas_face: usize, my: &ShapeBox) -> bool {
    const EPS: f32 = 1e-4;
    let opposite = OPPOSITE_FACE[atlas_face];
    let back = [-offset[0], -offset[1], -offset[2]];
    let (a0, a1) = face_plane_axes(atlas_face);
    neighbor_boxes.iter().any(|nb| {
        nb.faces[opposite]
            && touches_cell_boundary(nb, back)
            && nb.min[a0] <= my.min[a0] + EPS
            && nb.max[a0] >= my.max[a0] - EPS
            && nb.min[a1] <= my.min[a1] + EPS
            && nb.max[a1] >= my.max[a1] - EPS
    })
}

// Gemeinsamer Kern: vier Eckpunkte -> zwei Dreiecke, mit Standard-UV-Zuordnung. Die
// Eckreihenfolge ist bei allen FACES gleich aufgebaut (0 = unten-nah, 1 = oben-nah, 2 =
// oben-fern, 3 = unten-fern), daher passt eine feste Zuordnung: v0 ist die obere Kachelkante
// (RGBA-Zeilen laufen von oben), sodass Texturen an Seitenflaechen aufrecht stehen.
// Bloecke, die Vanilla in die TRANSLUCENT-Schicht legt und die deshalb echtes
// Alpha-Blending statt eines Alpha-Tests brauchen -- aus BlockRenderLayers des
// 1.21.11-Clients abgelesen, nicht geraten. Dort stehen ALLE Glasarten unter
// demselben Layer:
//   glass, glass_pane, tinted_glass,
//   <farbe>_stained_glass, <farbe>_stained_glass_pane
//
// Wichtig: auch KLARES Glas und klare Scheiben gehoeren dazu. Die naheliegende Annahme, nur
// gefaerbtes Glas sei "richtig" transparent und klares Glas ein Alpha-Cutout, ist falsch --
// Vanilla behandelt beide gleich.
//
// Laub steht bewusst NICHT hier: dessen Alpha wird beim Atlas-Bau ohnehin hart auf voll
// gesetzt (s. assets::is_leaves, feste Nutzerentscheidung "Grafik: Schnell").
// Feuer und Seelenfeuer (Vanillas AbstractFireBlock).
#[must_use] 
pub fn is_fire(name: &str) -> bool {
    name == "fire" || name == "soul_fire"
}

// Vanillas IronBarsBlock.skipRendering (Glasscheiben und Gitter): gegen denselben Block -- bei
// Gittern gegen jedes Gitter -- faellt die Flaeche nach oben und unten immer weg, zur Seite
// nur, wenn beide Bloecke zueinander verbunden sind. Sonst saehe man in einer Scheibenwand an
// jeder Blockgrenze den Kantenstreifen.
pub(super) fn pane_skips_face(state: u32, neighbor: u32, offset: [i32; 3]) -> bool {
    let (Some(name), Some(other)) = (block_name(state), block_name(neighbor)) else { return false };
    let bars = |n: &str| n.ends_with("_bars");
    let pane = |n: &str| n.ends_with("_pane") || bars(n);
    if !pane(name) || !(name == other || (bars(name) && bars(other))) {
        return false;
    }
    let (mine, theirs) = match offset {
        [0, 0, -1] => ("north", "south"),
        [0, 0, 1] => ("south", "north"),
        [-1, 0, 0] => ("west", "east"),
        [1, 0, 0] => ("east", "west"),
        _ => return true,
    };
    property(state, mine) == Some("true") && property(neighbor, theirs) == Some("true")
}

#[must_use] 
pub fn is_glass(name: &str) -> bool {
    matches!(name, "glass" | "glass_pane" | "tinted_glass") || name.ends_with("_stained_glass") || name.ends_with("_stained_glass_pane")
}

// Laub -- gebraucht fuer dieselbe Wasser-Sonderregel wie Glas (s. push_fluid_quads:
// Vanillas FluidRenderer prueft `TranslucentBlock || LeavesBlock`). Absichtlich getrennt von
// is_glass: fuer die Pass-Zuordnung zaehlt Laub NICHT als transparent (sein Alpha wird beim
// Atlas-Bau hart auf voll gesetzt, s. assets::is_leaves), nur fuer die Wassertextur daneben.
#[must_use] 
pub fn is_leaves(name: &str) -> bool {
    name.ends_with("_leaves")
}

// Wie push_quad_with_uv, aber mit FREI gewaehlten UVs je Ecke statt eines achsenparallelen
// Rechtecks. Nur so laesst sich eine Textur GEDREHT auf eine Flaeche legen -- ein
// [u0,v0,u1,v1]-Rechteck kann eine Drehung nicht ausdruecken (derselbe Grund, aus dem die
// RNG-Blockrotation vorab gedrehte Kacheln backt, s. assets::rng_face_uvs). Gebraucht fuer
// die Wasser-Fliesstextur, die in Stroemungsrichtung zeigen muss.
pub(super) fn push_quad_with_corner_uvs(out: &mut Vec<BlockVertex>, corners: [[f32; 3]; 4], color: [f32; 3], corner_uv: [[f32; 2]; 4]) {
    let vert = |i: usize| BlockVertex { position: corners[i], color, uv: corner_uv[i] };
    out.extend_from_slice(&[vert(0), vert(1), vert(2), vert(0), vert(2), vert(3)]);
}

// UVs der vier Ecken der Wasser-OBERSEITE -- Nachbau von FluidRenderer (Yarn-1.21.11-Jar
// nachgesehen). Die Eckenreihenfolge entspricht der von push_fluid_quads erzeugten:
// (x0,z0), (x0,z1), (x1,z1), (x1,z0) -- exakt Vanillas Vertex-Reihenfolge.
//
// Steht das Wasser (Stroemung 0/0), wird die Standtextur flaechig aufgezogen. Fliesst es,
// kommt die Fliesstextur um den Stroemungswinkel gedreht zum Einsatz -- und zwar nur ein
// HALBER Kachelausschnitt (+-0.25 um die Kachelmitte). Das ist kein Sparen, sondern noetig:
// beim Drehen eines vollen Ausschnitts liefen die Ecken aus der Kachel heraus und zoegen
// Nachbarkacheln aus dem Atlas herein.
pub(super) fn water_top_corner_uvs(flow: Option<[f64; 3]>, still: [f32; 4], flow_tile: Option<[f32; 4]>) -> [[f32; 2]; 4] {
    // Punkt innerhalb einer Atlas-Kachel: (0,0) = obere linke Ecke der Kachel, (1,1) = untere
    // rechte -- Vanillas Sprite.getFrameU/getFrameV.
    let punkt = |rect: [f32; 4], fu: f32, fv: f32| [rect[0] + (rect[2] - rect[0]) * fu, rect[1] + (rect[3] - rect[1]) * fv];

    if let (Some(v), Some(rect)) = (flow, flow_tile) {
        if v[0] != 0.0 || v[2] != 0.0 {
            // atan2(z, x) - PI/2, s. FluidRenderer. Der Versatz um PI/2 bringt die Textur-
            // richtung mit Minecrafts Achsen zur Deckung.
            let winkel = (v[2] as f32).atan2(v[0] as f32) - std::f32::consts::FRAC_PI_2;
            let s = winkel.sin() * 0.25;
            let c = winkel.cos() * 0.25;
            return [
                punkt(rect, 0.5 + (-c - s), 0.5 + (-c + s)),
                punkt(rect, 0.5 + (-c + s), 0.5 + (c + s)),
                punkt(rect, 0.5 + (c + s), 0.5 + (c - s)),
                punkt(rect, 0.5 + (c - s), 0.5 + (-c - s)),
            ];
        }
    }
    // Ruhiges Wasser (oder keine Fliesstextur im Atlas): volle Kachel, Reihenfolge wie
    // Vanillas Still-Zweig.
    [punkt(still, 0.0, 0.0), punkt(still, 0.0, 1.0), punkt(still, 1.0, 1.0), punkt(still, 1.0, 0.0)]
}

// DIAGNOSE (nur aktiv mit gesetzter Umgebungsvariable MC_DEBUG_WATER_SIDES): faerbt jede
// Wasser-SEITENFLAECHE knallrot statt in der Biomfarbe. Damit laesst sich in EINEM Durchlauf
// unterscheiden, ob die Waende gar nicht auf dem Bildschirm ankommen (kein Rot) oder ob sie
// gezeichnet werden und nur nicht so aussehen wie erwartet (Rot sichtbar).
//
// Hintergrund: Mesher-Ausgabe, Atlas-UVs und Fliesstextur sind gegen die echte Installation
// nachgemessen und alle drei korrekt -- der Fehler muss also zwischen Vertexdaten und Bildschirm
// liegen, und das laesst sich nicht mehr aus dem Quelltext ablesen. Wieder rausnehmen, sobald
// die Frage beantwortet ist.
// UVs der vier Ecken einer Wasser-SEITENFLAECHE -- Nachbau von FluidRenderer (Yarn-1.21.11,
// Schleife ueber Type.HORIZONTAL):
//     u1 = sprite.getFrameU(0.0);          u2 = sprite.getFrameU(0.5);
//     v1 = sprite.getFrameV((1-h1)*0.5);   v2 = sprite.getFrameV((1-h2)*0.5);
//     v3 = sprite.getFrameV(0.5);
// Also nur die LINKE OBERE Vierteldeckung der Kachel: waagerecht die halbe Breite, senkrecht
// von (1-h)/2 bis 1/2. Eine volle Zelle (h=1) bekommt damit genau die obere Kachelhaelfte,
// eine halbhohe genau ein Viertel -- die Textur wird also NICHT auf die Wasserhoehe
// gestaucht, sondern beschnitten. Genau daran erkennt man in Vanilla die nach unten laufende
// Fliessgrafik an jeder Wasserkante.
//
// `h1`/`h2` sind die Hoehen der beiden oberen Ecken in der Reihenfolge, die push_fluid_quads
// erzeugt: Ecke 0 unten-links, 1 oben-links (h1), 2 oben-rechts (h2), 3 unten-rechts. Das ist
// Vanillas Vertex-Reihenfolge um eins gedreht (dort steht die obere Ecke zuerst) -- dieselbe
// Flaeche, nur ein anderer Startpunkt im Umlauf.
pub(super) fn water_side_corner_uvs(rect: [f32; 4], h1: f32, h2: f32) -> [[f32; 2]; 4] {
    let punkt = |fu: f32, fv: f32| [rect[0] + (rect[2] - rect[0]) * fu, rect[1] + (rect[3] - rect[1]) * fv];
    let unten = 0.5;
    [punkt(0.0, unten), punkt(0.0, (1.0 - h1) * 0.5), punkt(0.5, (1.0 - h2) * 0.5), punkt(0.5, unten)]
}

pub(super) fn push_quad_with_uv(out: &mut Vec<BlockVertex>, corners: [[f32; 3]; 4], color: [f32; 3], uv: [f32; 4]) {
    let [u0, v0, u1, v1] = uv;
    let corner_uv = [[u0, v1], [u0, v0], [u1, v0], [u1, v1]];
    let vert = |i: usize| BlockVertex { position: corners[i], color, uv: corner_uv[i] };
    out.extend_from_slice(&[vert(0), vert(1), vert(2), vert(0), vert(2), vert(3)]);
}
