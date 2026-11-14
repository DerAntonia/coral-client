// Rueckseiten, Boxen, Geister- und Display-Bloecke und Abbau-Risse.

use super::*;

// Ergaenzt alle ab `start` erzeugten Dreiecke um ihre Rueckseite (vertauschte Wicklung).
//
// Noetig, seit die Translucent-Pipeline Rueckseiten verwirft (s. render-Crate). Angewandt auf
// die Wasser-OBERSEITE und auf die SEITENFLAECHEN -- genau die zwei Stellen, an denen Vanillas
// FluidRenderer dieselben vier Vertices ein zweites Mal in umgekehrter Reihenfolge ausgibt
// (canFlowTo(up)-Zweig bzw. ohne Overlay am Ende der
// HORIZONTAL-Schleife). Ohne die Seiten-Rueckseite fehlt die Wasserkante, sobald man IM Wasser
// steht und von innen dagegen schaut: die Fliesstextur an den Raendern fehlt.
//
// AUSGENOMMEN bleibt der Overlay-Fall (Nachbar Glas/Laub) -- auch das ist Vanilla, und es war
// schon einmal ein ECHTER FEHLER: eine fruehere Fassung verdoppelte JEDE Wasserflaeche
// bedingungslos. Von aussen sieht man dann durch die Scheibe zwei Wasserschichten uebereinander
// (die nahe Vorderseite plus die Rueckseite der fernen Flaeche), und das Blending laeuft
// doppelt -- in Vanilla ist dort nur ruhiges Blau.
//
// Glas bekommt diese Behandlung gar nicht: doppelseitiges Glas blendet ebenfalls zweimal und
// wirkt dadurch doppelt so dicht.
pub(super) fn push_rueckseiten(out: &mut Vec<BlockVertex>, start: usize) {
    let mut rueck: Vec<BlockVertex> = Vec::with_capacity(out.len() - start);
    for tri in out[start..].chunks_exact(3) {
        rueck.extend_from_slice(&[tri[0], tri[2], tri[1]]);
    }
    out.extend_from_slice(&rueck);
}

// Achsenparallele Box zwischen `min` und `max` (Weltkoordinaten) als 36 Vertices
// (12 Dreiecke), mit derselben Richtungs-Helligkeit wie die Chunk-Faces -- fuer
// Entity-Boxen im Renderer. Bewusst hier statt im render-Crate: BlockVertex und die
// FACES-Konvention leben in diesem Modul.
#[must_use] 
pub fn box_vertices(min: [f32; 3], max: [f32; 3], color: [f32; 3], uv: [f32; 4]) -> Vec<BlockVertex> {
    let mut out = Vec::with_capacity(36);
    for (dir, brightness, _atlas_face) in FACES {
        let shaded = [color[0] * brightness, color[1] * brightness, color[2] * brightness];
        push_face_quad(&mut out, min, max, dir, shaded, uv);
    }
    out
}

// Ein einzelner Block als freistehende Geometrie -- alle sechs Seiten, ohne Nachbar-Culling,
// mit den echten Flaechentexturen des States. Fuer die halbdurchsichtigen Geisterbloecke
// (unbestaetigte Platzierungen, s. app::PendingBlock): die stecken bewusst NICHT in den
// Chunk-Daten und werden deshalb auch nicht vom Spalten-Mesher erfasst.
//
// Kein Culling gegen die Nachbarn: ein Geisterblock steht per Definition auf einem freien
// Platz, und die halbe Deckkraft macht die Rueckseiten ohnehin sichtbar. Die Form kommt aus
// denselben ShapeBoxes wie im Chunk-Mesh, ein Vollwuerfel ist also wirklich ein Wuerfel und
// eine Stufe wirklich eine Stufe.
// `base` ist die untere Ecke des Blocks, BEREITS relativ zum Bezugspunkt des Aufrufers
// (beim Client: die Kameraposition, in f64 abgezogen -- s. render::BlockRenderer zum Grund).
// Frueher stand hier die absolute Blockposition; bei grossen Weltkoordinaten war die in f32
// schon zerstoert, bevor sie ueberhaupt in den Puffer kam.
// Geisterblock an einer echten Weltposition -- wie block_ghost_vertices, aber auch fuer die
// beiden Blocksorten, die KEINE Quader-Formen haben und dort deshalb unsichtbar blieben:
// Kreuzmodelle (Cobweb, Pflanzen) und Fluessigkeiten (Wasser/Lava aus dem Eimer). Beide laufen
// durch DENSELBEN Zeichenpfad wie im Chunk-Mesh, damit der Geist genau so aussieht wie der
// Block, den der Server gleich bestaetigt. `block_at` liefert die Nachbarn (fuer Fluid-
// Eckhoehen und Culling), `water_tint` die Biomfarbe des Wassers.
#[allow(clippy::too_many_arguments)]
pub fn block_ghost_vertices_at(
    base: [f32; 3],
    pos: (i32, i32, i32),
    state: u32,
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    cross: &CrossBlocks,
    water_tint: [f32; 3],
    block_at: &impl Fn(i32, i32, i32) -> u32,
) -> Vec<BlockVertex> {
    let name = block_name(state).unwrap_or("");
    let origin = ColumnOrigin { x: pos.0, z: pos.2 };
    let mut out = Vec::new();
    if cross.is_cross(state) {
        push_cross_quads(&mut out, origin, pos.0, pos.1, pos.2, [1.0, 1.0, 1.0], uvs.face_rect(state, 2));
    } else if name == "water" {
        push_fluid_quads(&mut out, origin, pos.0, pos.1, pos.2, state, FluidKind::Water, water_tint, uvs, shapes, block_at);
    } else if name == "lava" {
        push_fluid_quads(&mut out, origin, pos.0, pos.1, pos.2, state, FluidKind::Lava, [1.0, 1.0, 1.0], uvs, shapes, block_at);
    } else {
        return ghost_vertices_with_model(base, pos, state, uvs, shapes);
    }
    // Spalten-lokal mit Ursprung genau auf dem Block: x/z liegen schon bei 0..1, y ist die
    // absolute Hoehe -- auf den augenrelativen Ursprung umrechnen.
    for v in &mut out {
        v.position = [v.position[0] + base[0], v.position[1] - pos.1 as f32 + base[1], v.position[2] + base[2]];
    }
    out
}

// 0xRRGGBB als Farbe 0..1.
pub(super) fn rgb_f32(rgb: u32) -> [f32; 3] {
    [((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0]
}

// Ein Block fuer ein block_display (BlockRenderManager.renderBlockAsEntity): die echte Form des
// States im Einheitswuerfel 0..1, ohne Welt -- also ohne Culling und ohne Biom. Die Faerbung
// ist deshalb Vanillas Vorgabe ohne Welt (BlockColors mit world == null): Gras 0x91BD59,
// Laub 0x48B518, trockenes Laub 0xA0A69C. Vorher stand an der Stelle das ITEM-Modell des
// Blocks -- ein Zaun als Inventarbild mit zwei Latten statt als der Pfosten mit genau den
// Armen, die der State nennt.
pub fn block_display_vertices(state: u32, uvs: &BlockUvs, shapes: &BlockShapes, cross: &CrossBlocks) -> Vec<BlockVertex> {
    let name = block_name(state).unwrap_or("");
    let farbe = |rgb: u32| [((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0];
    let (tint, nur_oben) = match tint_kind_of_state(state, name) {
        TintKind::None => ([1.0; 3], false),
        TintKind::GrassTop => (farbe(0x91BD59), true),
        TintKind::GrassAll => (farbe(0x91BD59), false),
        TintKind::FoliageAll => (farbe(0x48B518), false),
        TintKind::DryFoliageAll => (farbe(0xA0A69C), false),
        // Ohne Welt nimmt Vanilla fuer die Seerose ihre Item-Farbe.
        TintKind::Fixed(0x208030) => (farbe(0x71C35C), false),
        TintKind::Fixed(rgb) => (farbe(rgb), false),
    };
    let mut out = Vec::new();
    if cross.is_cross(state) {
        push_cross_quads(&mut out, ColumnOrigin { x: 0, z: 0 }, 0, 0, 0, tint, uvs.face_rect(state, 2));
        return out;
    }
    if let Some(quads) = shapes.model_quads_at(state, 0, 0, 0) {
        for q in quads {
            let color = [tint[0] * q.shade, tint[1] * q.shade, tint[2] * q.shade];
            let vert = |i: usize| BlockVertex { position: q.corners[i], color, uv: q.uvs[i] };
            out.extend_from_slice(&[vert(0), vert(2), vert(1), vert(0), vert(3), vert(2)]);
        }
        return out;
    }
    if is_marker_block(name) || name == "water" || name == "lava" {
        return out;
    }
    for shape_box in shapes.shapes_of(state) {
        for (dir, brightness, atlas_face) in FACES {
            if !shape_box.faces[atlas_face] {
                continue;
            }
            let t = if !nur_oben || atlas_face == FACE_UP { tint } else { [1.0; 3] };
            let uv = crop_face_uv(uvs.face_rect(state, atlas_face), shape_box.uv[atlas_face]);
            push_face_quad(&mut out, shape_box.min, shape_box.max, dir, [brightness * t[0], brightness * t[1], brightness * t[2]], uv);
        }
    }
    out
}

pub fn block_ghost_vertices(base: [f32; 3], state: u32, uvs: &BlockUvs, shapes: &BlockShapes) -> Vec<BlockVertex> {
    ghost_vertices_with_model(base, (0, 0, 0), state, uvs, shapes)
}

// Modelle mit gedrehten Elementen (Wandfackel, Laterne, Feuer, ...) als ihre echten Vierecke
// wie im Chunk-Mesh, sonst die Boxen. Frueher immer die Boxen: eine vorhergesagte Wandfackel
// stand dann einen Frame lang ungedreht quer im Block. `pos` waehlt die Fassung
// (s. BlockShapes::model_quads_at).
pub(super) fn ghost_vertices_with_model(base: [f32; 3], pos: (i32, i32, i32), state: u32, uvs: &BlockUvs, shapes: &BlockShapes) -> Vec<BlockVertex> {
    let mut out = Vec::new();
    if let Some(quads) = shapes.model_quads_at(state, pos.0, pos.1, pos.2) {
        for q in quads {
            let color = [face_shade(q.shade); 3];
            let vert = |i: usize| BlockVertex {
                position: [base[0] + q.corners[i][0], base[1] + q.corners[i][1], base[2] + q.corners[i][2]],
                color,
                uv: q.uvs[i],
            };
            // Umlauf wie im Chunk-Mesh (s. dort).
            out.extend_from_slice(&[vert(0), vert(2), vert(1), vert(0), vert(3), vert(2)]);
        }
        return out;
    }
    for shape_box in shapes.shapes_of(state) {
        let min = [base[0] + shape_box.min[0], base[1] + shape_box.min[1], base[2] + shape_box.min[2]];
        let max = [base[0] + shape_box.max[0], base[1] + shape_box.max[1], base[2] + shape_box.max[2]];
        for (dir, brightness, atlas_face) in FACES {
            let uv = crop_face_uv(uvs.face_rect(state, atlas_face), shape_box.uv[atlas_face]);
            let shade = [face_shade(brightness); 3];
            push_face_quad(&mut out, min, max, dir, shade, uv);
        }
    }
    out
}

// Die Riss-Ueberlagerung beim Abbauen -- Vanillas LevelRenderer.renderBlockDestroyAnimation.
//
// Gezeichnet wird KEIN geschrumpfter Kasten und kein Drahtgitter, sondern die echte Form des
// Blocks noch einmal, mit der Textur destroy_stage_0..9 darueber (Pipeline "crumbling",
// s. render::BlockRenderer). Genau wie in Vanilla:
//
//  - Die UV kommt NICHT aus der Flaeche, sondern aus der Position des Eckpunkts
//    (SheetedDecalTextureGenerator): der Riss wird wie eine Abziehfolie in Blockkoordinaten
//    auf die Flaeche projiziert, eine Kachel je Block. Eine Stufe zeigt deshalb nur die
//    untere Haelfte des Rissbilds statt ein gestauchtes Ganzes.
//  - `textureScale` ist 1.0, die Drehung je Richtung steckt in Direction.getRotation().
//    Ausmultipliziert (s. Test riss_uv_folgt_vanillas_projektion) ergeben sich die sechs
//    Achsenzuordnungen unten; negative Werte wiederholen sich in Vanilla ueber GL_REPEAT,
//    hier genuegt +1, weil eine Blockform nie breiter als eine Kachel ist.
//  - Flaechen, die ein Nachbar ohnehin vollstaendig verdeckt, fallen weg. Ohne das laegen
//    die Risse (durch den Tiefen-Bias nach vorn gezogen) auf der Nachbarwand.
//
// `base` ist der augenrelative Ursprung des Blocks, `stage_uv` die Atlas-Kachel der Stufe
// (s. BlockUvs::destroy_stage_rect).
pub fn block_breaking_vertices(
    base: [f32; 3],
    pos: (i32, i32, i32),
    state: u32,
    stage_uv: [f32; 4],
    shapes: &BlockShapes,
    block_at: &impl Fn(i32, i32, i32) -> u32,
) -> Vec<BlockVertex> {
    let [su0, sv0, su1, sv1] = stage_uv;
    let mut out = Vec::new();
    for shape_box in shapes.shapes_of(state) {
        for (dir, _brightness, atlas_face) in FACES {
            if !shape_box.faces[atlas_face] {
                continue;
            }
            let neighbor = block_at(pos.0 + dir[0], pos.1 + dir[1], pos.2 + dir[2]);
            if is_solid(neighbor) && shapes.has_resolved_shape(neighbor) && neighbor_covers_face(shapes.shapes_of(neighbor), dir, atlas_face, shape_box)
            {
                continue;
            }
            let local = face_quad_corners(shape_box.min, shape_box.max, dir);
            let raw = local.map(|c| breaking_uv(dir, c));
            // Die Verschiebung in den Kachelbereich gilt fuer das GANZE Viereck, nicht je
            // Ecke -- sonst risse eine Flaeche mit u/v von -0,5 bis 0 (z.B. die Seite einer
            // Stufe) in zwei verschiedene Kachelhaelften auseinander.
            let shift = |werte: [f32; 4]| -werte.iter().copied().fold(f32::INFINITY, f32::min).floor();
            let (du, dv) = (shift(raw.map(|(u, _)| u)), shift(raw.map(|(_, v)| v)));
            let verts: [BlockVertex; 4] = std::array::from_fn(|i| {
                let [x, y, z] = local[i];
                let (u, v) = (raw[i].0 + du, raw[i].1 + dv);
                BlockVertex {
                    position: [base[0] + x, base[1] + y, base[2] + z],
                    color: [1.0, 1.0, 1.0],
                    uv: [su0 + u * (su1 - su0), sv0 + v * (sv1 - sv0)],
                }
            });
            out.extend_from_slice(&[verts[0], verts[1], verts[2], verts[0], verts[2], verts[3]]);
        }
    }
    out
}

// Die Riss-UV eines Eckpunkts in Blockkoordinaten (0..1), s. block_breaking_vertices.
pub(super) fn breaking_uv(dir: [i32; 3], p: [f32; 3]) -> (f32, f32) {
    let [x, y, z] = p;
    let (u, v) = match dir {
        [0, -1, 0] => (x, -z),
        [0, 1, 0] => (x, z),
        [0, 0, -1] => (-x, -y),
        [0, 0, 1] => (x, -y),
        [-1, 0, 0] => (-z, -y),
        [1, 0, 0] => (z, -y),
        _ => unreachable!("Face-Richtung muss ein Einheitsvektor sein"),
    };
    // Roh, also noch ohne die Verschiebung in den Kachelbereich -- die macht der Aufrufer
    // fuer das ganze Viereck auf einmal (in Vanilla uebernimmt das GL_REPEAT).
    (u, v)
}

// Waehlt den passenden Ausschnitt INNERHALB einer Atlas-Flaechenkachel (s. ShapeBox::uv) --
// `crop` ist normiert (0..1) relativ zur Kachel, `crop=None` liefert die volle Kachel
// unveraendert. Ohne das wird die ganze Textur ueber jede Flaeche gestreckt, egal wie klein
// sie ist (Beispiel: Falllaub mit segment_amount zeigte immer die komplette, gestauchte
// Grafik statt eines kleinen, dichten Ausschnitts -- betrifft genauso Stufen/Treppen, deren
// Modelle ebenfalls "uv"-Ausschnitte nutzen, um das Holzmaser-/Steinmuster ueber die Kanten
// hinweg fortzusetzen statt es zu verzerren). u0>u1 bzw. v0>v1 im Ausschnitt sind gueltig
// (Vanilla-Spiegeltrick, z.B. eine gespiegelte Unterseite) -- reine lineare Interpolation
// ohne Sortierung erhaelt das.
pub(crate) fn crop_face_uv(tile: [f32; 4], crop: Option<[f32; 4]>) -> [f32; 4] {
    let Some([u0, v0, u1, v1]) = crop else { return tile };
    let [tu0, tv0, tu1, tv1] = tile;
    let (tw, th) = (tu1 - tu0, tv1 - tv0);
    [tu0 + u0 * tw, tv0 + v0 * th, tu0 + u1 * tw, tv0 + v1 * th]
}
