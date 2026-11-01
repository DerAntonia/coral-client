// Wasser in wassergefuellten Bloecken, Eckhoehen und Quads der Fluessigkeiten.

use super::*;

// Fuellt eine waterlogged-Zelle mit Wasser -- eigenstaendiger Wuerfel ueber den gesamten
// Zellraum (0..1), unabhaengig von der tatsaechlichen (oft duenneren) Form des eigentlichen
// Blocks: eine exakte Aussparung um z.B. einen Zaunpfosten waere ungleich aufwaendiger und
// der optische Unterschied bei einer duennen Geometrie kaum sichtbar. `own_faces` (s.
// Aufrufer) blendet Richtungen aus, die der Block selbst schon deckt (Z-Fighting-Vermeidung,
// z.B. eine waterlogged Treppenstufe deckt ihre eigene Unterseite bereits opak ab).
#[allow(clippy::too_many_arguments)] // wie push_fluid_quads: Zellposition, Farbe, UVs, Formen
                                     // und Nachbarzugriff gehoeren alle zwingend dazu
pub(super) fn push_waterlogged_fill(
    out: &mut Vec<BlockVertex>,
    origin: ColumnOrigin,
    x: i32,
    y: i32,
    z: i32,
    tint: [f32; 3],
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    block_at: &impl Fn(i32, i32, i32) -> u32,
    own_faces: [bool; 6],
) {
    use crate::fluid::fluid_kind;

    let same_fluid = |s: u32| fluid_kind(s) == Some(FluidKind::Water);
    let name_at = |s: u32| block_name(s).unwrap_or("");

    let (lx, ly, lz) = origin.local(x, y, z);
    let world_min = [lx, ly, lz];
    let world_max = [lx + 1.0, ly + 1.0, lz + 1.0];

    // Eckhoehen einmal vorab: sie werden sowohl fuer die Geometrie (Oberflaeche UND Waende)
    // als auch fuer die Verdeckungspruefung gebraucht -- Vanilla testet je Seite gegen die
    // tatsaechliche WASSERHOEHE, nicht gegen einen vollen Wuerfel (s. water_side_box).
    let (o, p, q, r) = fluid_corner_heights(x, y, z, FluidKind::Water, block_at);
    let hoehe_der_seite = |atlas_face: usize| match atlas_face {
        FACE_DOWN => FLUID_DOWN_HEIGHT,
        FACE_UP => p.min(r).min(q.min(o)),
        FACE_NORTH => p.max(o),
        FACE_SOUTH => r.max(q),
        FACE_WEST => p.max(r),
        _ => o.max(q),
    };

    // DIESELBE Verdeckungspruefung wie bei echtem Wasser (push_fluid_quads::occludes).
    //
    // Hier stand frueher nur `is_solid(neighbor)`, also "Nachbar ist nicht Luft" -- eine viel
    // zu grobe Regel, die JEDE Wasserflaeche an JEDEM beliebigen Nachbarn verschluckt hat.
    // Eine waterlogged Stufe neben einer normalen Stufe verlor so ihre Wasserwand komplett,
    // obwohl die Nachbarstufe nur bis zur halben Hoehe reicht und Vanilla dort sehr wohl die
    // Fliessflaeche zeichnet; dasselbe an Zaeunen, Fackeln, Pflanzen, Treppen. Echtes Wasser
    // macht es seit laengerem richtig, dieser Pfad war der letzte mit der groben Regel --
    // und er ist genau der Pfad, der waterlogged Stufen zeichnet.
    let occludes = |neighbor: u32, offset: [i32; 3], atlas_face: usize| {
        is_solid(neighbor)
            && shapes.has_resolved_shape(neighbor)
            && !never_occludes_neighbors(name_at(neighbor))
            // Glas ausgenommen wie bei echtem Wasser -- seine Culling-Form ist leer, weil es
            // nicht opak ist (ausfuehrlich s. push_fluid_quads::occludes).
            && !is_glass(name_at(neighbor))
            && neighbor_covers_face(
                shapes.shapes_of(neighbor),
                offset,
                atlas_face,
                &water_side_box(hoehe_der_seite(atlas_face)),
            )
    };

    for (offset, brightness, atlas_face) in FACES {
        if own_faces[atlas_face] {
            continue;
        }
        let neighbor = block_at(x + offset[0], y + offset[1], z + offset[2]);
        // Nachbar gehoert zum selben Wasserkoerper (echtes Wasser, andere waterlogged Bloecke,
        // Seegras/Kelp) -> gemeinsame Flaeche entfaellt, sonst saehe man eine Naht mitten im
        // Wasser.
        if same_fluid(neighbor) || occludes(neighbor, offset, atlas_face) {
            continue;
        }
        // Oben-Flaeche: NICHT flach auf voller Zellhoehe wie die anderen fuenf Seiten,
        // sondern mit demselben Eck-Hoehen-Gefaelle wie richtiges Wasser (s.
        // fluid_corner_heights/push_fluid_quads) -- sonst haengt genau an dieser Zelle
        // (z.B. Seegras/Kelp mit always_wet-Wasserfuellung, s. fluid::always_wet, oder ein
        // waterlogged Zaunpfosten) die Oberflaeche IMMER auf y+1, waehrend das echte
        // Nachbarwasser ringsum ganz normal absackt (isolierte Quelle, Rand einer Flaeche,
        // ...) -- optisch eine hochgezogene Kante/Wand genau an dieser Stelle (etwa
        // bei Seegras).
        if atlas_face == FACE_UP {
            const EPS: f32 = 0.001;
            let corners = [
                [world_min[0], world_min[1] + p - EPS, world_min[2]],
                [world_min[0], world_min[1] + r - EPS, world_max[2]],
                [world_max[0], world_min[1] + q - EPS, world_max[2]],
                [world_max[0], world_min[1] + o - EPS, world_min[2]],
            ];
            let uv = uvs.water_rect(FACE_UP);
            let oberseite_start = out.len();
            push_quad_with_uv(out, corners, [brightness * tint[0], brightness * tint[1], brightness * tint[2]], uv);
            // Nur die Oberseite doppelseitig, wie bei echtem Wasser (s. push_rueckseiten).
            push_rueckseiten(out, oberseite_start);
            continue;
        }
        let uv = uvs.water_rect(atlas_face);
        let shade = [brightness * tint[0], brightness * tint[1], brightness * tint[2]];
        if atlas_face == FACE_DOWN {
            // Unterseite liegt flach auf der Zellunterkante -- da gibt es nichts anzupassen.
            push_face_quad(out, world_min, world_max, offset, shade, uv);
            continue;
        }
        // SEITENFLAECHEN muessen oben an derselben Eck-Hoehe enden wie die Oberflaeche.
        // Vorher lief hier push_face_quad ueber die VOLLE Zellbox: die Waende reichten bis
        // zur Zellkante (y+1), waehrend die Wasseroberflaeche darueber bei z.B. y+0.74 lag --
        // ein 0.26 hoher Wasserkragen rund um die Zelle, der ins Leere stand (etwa an
        // einer waterlogged Stufe). Echtes Wasser macht es laengst richtig, s.
        // push_fluid_quads; nur dieser Pfad war noch bei der flachen Vollbox geblieben.
        let (x0, y0, z0) = (world_min[0], world_min[1], world_min[2]);
        let (x1, z1) = (world_max[0], world_max[2]);
        // SEITENFLAECHEN UM EPS NACH INNEN -- derselbe Versatz, den Vanilla und (seit laengerem)
        // auch echtes Wasser hier machen (FluidRenderer: 0.001 von der Zellkante weg).
        // Dieser Pfad hatte ihn als einziger nie, und genau hier faellt er am staerksten ins
        // Gewicht: die Wasserwand einer waterlogged Stufe liegt ueber ihre untere Haelfte EXAKT
        // auf der opaken Seitenflaeche der Stufe selbst (beide bei x=0/x=1 bzw. z=0/z=1, die
        // Stufe reicht bis y+0.5). Zwei deckungsgleiche Flaechen haben bit-identische
        // interpolierte Tiefe, und der Translucent-Pass testet mit LESS (nicht LessEqual, s.
        // render::block_renderer) -- die Wasserwand faellt dort komplett durch den Tiefentest.
        // Mit dem Versatz liegt sie knapp VOR der Stufenflaeche und besteht ihn.
        const EPS: f32 = 0.001;
        let (zn, zs) = (z0 + EPS, z1 - EPS);
        let (xw, xe) = (x0 + EPS, x1 - EPS);
        // Eckzuordnung wie in push_fluid_quads: p=(x0,z0), r=(x0,z1), q=(x1,z1), o=(x1,z0).
        let (corners, h1, h2) = match atlas_face {
            FACE_NORTH => ([[x0, y0, zn], [x0, y0 + p, zn], [x1, y0 + o, zn], [x1, y0, zn]], p, o),
            FACE_SOUTH => ([[x1, y0, zs], [x1, y0 + q, zs], [x0, y0 + r, zs], [x0, y0, zs]], q, r),
            FACE_WEST => ([[xw, y0, z1], [xw, y0 + r, z1], [xw, y0 + p, z0], [xw, y0, z0]], r, p),
            _ => ([[xe, y0, z0], [xe, y0 + o, z0], [xe, y0 + q, z1], [xe, y0, z1]], o, q),
        };
        // Seitenflaechen tragen die Fliesstextur (water_flow) bzw. an Glas/Laub das
        // water_overlay -- exakt dieselbe Auswahl und dieselbe Halbkachel-UV wie bei echtem
        // Wasser, s. push_fluid_quads::seite und water_side_corner_uvs.
        let n = block_name(neighbor).unwrap_or("");
        let overlay = is_glass(n) || is_leaves(n);
        let rect = if overlay { uvs.water_overlay_rect().unwrap_or(uv) } else { uvs.water_flow_rect().unwrap_or(uv) };
        let start = out.len();
        push_quad_with_corner_uvs(out, corners, shade, water_side_corner_uvs(rect, h1, h2));
        if !overlay {
            push_rueckseiten(out, start);
        }
    }
}

// Die vier Eck-Hoehen (Nordwest/Nordost/Suedost/Suedwest, s. push_fluid_quads-Aufrufer fuer
// die genaue o/p/q/r-Zuordnung) einer Fluessigkeitszelle bei (x,y,z) -- ausgelagert aus
// push_fluid_quads, damit push_waterlogged_fill (Seegras/Kelp/waterlogged-Bloecke) fuer ihre
// eigene Oberflaeche exakt dieselbe Vanilla-Formel (FluidRenderer.getFluidHeight, s.
// world::fluid-Modulkommentar) nutzen kann statt einer immer vollen, flachen Zelle.
pub(super) fn fluid_corner_heights(x: i32, y: i32, z: i32, kind: FluidKind, block_at: &impl Fn(i32, i32, i32) -> u32) -> (f32, f32, f32, f32) {
    use crate::fluid::{calculate_corner_height, render_height_at};

    let rh = |px: i32, py: i32, pz: i32| render_height_at(block_at(px, py, pz), block_at(px, py + 1, pz), kind, is_solid);
    let state = block_at(x, y, z);
    let above = block_at(x, y + 1, z);
    let n = render_height_at(state, above, kind, is_solid);
    if n >= 1.0 {
        return (1.0, 1.0, 1.0, 1.0);
    }
    let s_h = rh(x, y, z - 1);
    let t_h = rh(x, y, z + 1);
    let u_h = rh(x + 1, y, z);
    let v_h = rh(x - 1, y, z);
    let ne_h = rh(x + 1, y, z - 1);
    let nw_h = rh(x - 1, y, z - 1);
    let se_h = rh(x + 1, y, z + 1);
    let sw_h = rh(x - 1, y, z + 1);
    (
        calculate_corner_height(n, s_h, u_h, ne_h),
        calculate_corner_height(n, s_h, v_h, nw_h),
        calculate_corner_height(n, t_h, u_h, se_h),
        calculate_corner_height(n, t_h, v_h, sw_h),
    )
}

// "Meine eigene Form" fuer neighbor_covers_face() beim Wasser -- eine Zellbox, die nur bis
// zur tatsaechlichen Fluessigkeitshoehe `height` reicht, NICHT bis 1.0.
//
// Das ist der Unterschied, der frueher ein sichtbares Z-Fighting erzeugt hat (da wo das Wasser
// das Farmland beruehrt, flimmerte eine fremde Textur): Vanillas
// FluidRenderer prueft die Nachbar-Abdeckung gegen die WASSERHOEHE, nicht gegen einen vollen
// Wuerfel -- shouldSkipRendering(direction, Math.max(adx, yx), neighborState) fuer die Seiten,
// mit dem Festwert 8/9 fuer unten (s. Yarn-1.21.11-Fassung von FluidRenderer.render).
// Farmland ist nur 15/16 hoch (template_farmland.json: Element [0,0,0]..[16,15,16]) und deckt
// damit zwar die 8/9 hohe Wasserflaeche komplett ab, aber eben keinen vollen Wuerfel. Gegen
// eine 1.0-Box getestet galt der Nachbar deshalb als "deckt nicht", die Wasser-Seitenflaeche
// wurde gezeichnet -- exakt koplanar mit der Erdseite des Farmlands, die an derselben
// Zellgrenze liegt. Zwei koplanare Flaechen = Z-Fighting; man sah abwechselnd Wasser- und
// Dirt-Textur ueber die gemeinsame Flaeche gesprenkelt.
pub(super) const fn water_side_box(height: f32) -> ShapeBox {
    ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, height, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] }
}

// Vanillas Festwert fuer die Unterseite (FluidRenderer.render prueft die Unterseite mit
// 0.8888889) -- bewusst NICHT die tatsaechliche Eckhoehe: nach unten ist die Zelle
// immer bis zur Zellgrenze gefuellt, die 8/9 stehen dort nur fuer "eine Quelle hoch".
pub(super) const FLUID_DOWN_HEIGHT: f32 = 8.0 / 9.0;

// Vanilla-getreue Fluessigkeitsoberflaeche MIT Eck-Hoehen-Gefaelle (FluidRenderer.render()
// aus dem Yarn-1.21.11-Jar nachgesehen, s. world::fluid::calculate_corner_height/
// render_height_at) statt eines immer vollen, flachen Wuerfels -- der bisherige Stand
// (Level-Property nur fuer die Schwimmphysik, nie fuers Aussehen). Eigener Zeichenpfad statt der generischen Box-Schleife oben: eine
// Fluessigkeitszelle hat IMMER genau eine volle Zellbox (kein Content/xz-Offset), aber VIER
// unabhaengige Eckhoehen statt einer einzigen Flaeche -- ein [u0,v0,u1,v1]-UV-Rechteck kann
// das nicht ausdruecken, push_face_quad bleibt hier deshalb nur fuer die flache Unterseite
// nutzbar (s. render_down unten).
//
// GILT FUER BEIDE FLUESSIGKEITEN. Vanillas FluidRenderer.render zeichnet Wasser und Lava mit
// derselben Methode und derselben Geometrie; die Frage "ist das Lava?"
// entscheidet dort nur ueber drei Dinge, und genau die drei haengen hier an `kind`:
//   1. die Sprites (lava_still/lava_flow statt water_still/water_flow),
//   2. den Tint (Lava weiss, Wasser die Biom-Wasserfarbe),
//   3. die water_overlay-Ausnahme an Glas/Laub, die nur im Nicht-Lava-Zweig greift.
// Lava lief frueher stattdessen ueber den Vollwuerfel-Fallback der Box-Schleife und stand
// damit einen ganzen Block hoch statt 8/9. Der Aufrufer legt Lava
// ausserdem in den OPAKEN Puffer statt in den Translucent-Pass -- auch das ist Vanilla
// (RenderLayers.getFluidLayer: nur FluidTags.WATER bekommt translucent).
//
// BEWUSST NICHT nachgebildet (bewusste Vereinfachung, s. Klassenkommentar-Stil dieses
// Moduls): "isSolid()" (Vanilla-Materialeigenschaft) wird durch
// is_solid() angenaehert (alles ausser Luft zaehlt als Deckungs-Kandidat).
#[allow(clippy::too_many_arguments)]
pub(super) fn push_fluid_quads(
    out: &mut Vec<BlockVertex>,
    origin: ColumnOrigin,
    x: i32,
    y: i32,
    z: i32,
    state: u32,
    kind: FluidKind,
    tint: [f32; 3],
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    block_at: &impl Fn(i32, i32, i32) -> u32,
) {
    use crate::fluid::fluid_kind;

    let same_fluid = |s: u32| fluid_kind(s) == Some(kind);

    let above = block_at(x, y + 1, z);
    let below = block_at(x, y - 1, z);
    let north = block_at(x, y, z - 1);
    let south = block_at(x, y, z + 1);
    let west = block_at(x - 1, y, z);
    let east = block_at(x + 1, y, z);

    // FRUEHER AUSSTIEG VOR den Eckhoehen: eine Zelle, deren sechs Nachbarn alle dieselbe
    // Fluessigkeit sind, zeichnet garantiert nichts (jede der sechs render_*-Bedingungen unten
    // beginnt mit !same_fluid). In einem Ozean ist das die grosse Mehrheit aller Zellen, und
    // fluid_corner_heights kostet je Zelle 18 block_at-Aufrufe (9 Positionen mal Zelle+darueber)
    // plus vier gewichtete Mittelungen. Die standen vorher IMMER an, auch fuer jede
    // vollstaendig eingeschlossene Zelle -- ein grosser Wasserblock laggte deshalb total.
    if same_fluid(above) && same_fluid(below) && same_fluid(north) && same_fluid(south) && same_fluid(west) && same_fluid(east) {
        return;
    }

    let (o, p, q, r) = fluid_corner_heights(x, y, z, kind, block_at);

    let name_at = |s: u32| block_name(s).unwrap_or("");
    // `height` = die Fluessigkeitshoehe, die der Nachbar abdecken muss, damit die Flaeche
    // entfaellt (s. water_side_box-Kommentar). Je Seite das Maximum der beiden Eckhoehen an
    // ihrer Kante -- genau Vanillas Math.max(adx, yx).
    let occludes = |neighbor: u32, offset: [i32; 3], atlas_face: usize, height: f32| {
        is_solid(neighbor)
            && shapes.has_resolved_shape(neighbor)
            && !never_occludes_neighbors(name_at(neighbor))
            // Glas verdeckt auch Wasser nicht. Der Weg dorthin ist in Vanilla anders als bei
            // Bloecken, das Ergebnis aber dasselbe -- und beim ersten Anlauf hatte ich es
            // falsch herum:
            //
            // FluidRenderer.isSideCovered fragt die FORM ab (BlockState.getCullingFace) und
            // nicht die Deckkraft. Naheliegend, aber falsch, waere der Schluss "Glas ist ein
            // Vollwuerfel, also verdeckt es". Die Form wird naemlich erst gar nicht gefuellt,
            // wenn der Block nicht opak ist -- AbstractBlock, ShapeCache:
            //     cullingShape = opaque ? owner.getCullingShape(state) : VoxelShapes.empty()
            // Glas ist nonOpaque, seine Culling-Form ist also LEER, und isSideCovered steigt
            // bei leerer Form sofort mit false aus.
            && !is_glass(name_at(neighbor))
            && neighbor_covers_face(shapes.shapes_of(neighbor), offset, atlas_face, &water_side_box(height))
    };

    // Eckzuordnung wie beim Oberflaechen-Quad weiter unten: p=(x0,z0), r=(x0,z1), q=(x1,z1),
    // o=(x1,z0). Eine Seite liegt jeweils an zwei dieser Ecken.
    let render_up = !same_fluid(above);
    let render_down = !same_fluid(below) && !occludes(below, [0, -1, 0], 0, FLUID_DOWN_HEIGHT);
    let render_north = !same_fluid(north) && !occludes(north, [0, 0, -1], 2, p.max(o));
    let render_south = !same_fluid(south) && !occludes(south, [0, 0, 1], 3, r.max(q));
    let render_west = !same_fluid(west) && !occludes(west, [-1, 0, 0], 4, p.max(r));
    let render_east = !same_fluid(east) && !occludes(east, [1, 0, 0], 5, o.max(q));

    if !(render_up || render_down || render_north || render_south || render_west || render_east) {
        return;
    }

    const EPS: f32 = 0.001;
    let w = if render_down { EPS } else { 0.0 };
    let (o, p, q, r) = if render_up { (o - EPS, p - EPS, q - EPS, r - EPS) } else { (o, p, q, r) };

    let (x0, y0, z0) = origin.local(x, y, z);
    let (x1, z1) = (x0 + 1.0, z0 + 1.0);
    // Stand-Sprite: bei Wasser ueber das eigenstaendige water_rect (das auch die Fuellung
    // waterlogged-Bloecke braucht, die selbst gar kein "water"-State sind), bei Lava ganz
    // normal ueber die State-ID -- der Block "lava" traegt lava_still auf allen sechs Seiten
    // (s. assets::fixed_fluid_texture). Der Flaechenindex ist in beiden Faellen egal, beide
    // Sprites haben keine Ausrichtung.
    let (up_uv, side_uv, flow_rect) = match kind {
        FluidKind::Water => (uvs.water_rect(FACE_UP), uvs.water_rect(2), uvs.water_flow_rect()),
        FluidKind::Lava => (uvs.face_rect(state, FACE_UP), uvs.face_rect(state, 2), uvs.lava_flow_rect()),
    };

    // Grenzt eine Wasser-SEITE an Glas oder Laub, nimmt Vanilla eine eigene, deutlich
    // flachere Textur (water_overlay) statt der normalen Wasseroberflaeche -- FluidRenderer nimmt sie, wenn der Nachbar ein
    // TranslucentBlock oder LeavesBlock ist.
    // Das ist der Grund, warum Wasser hinter einer Glasscheibe in Vanilla als ruhige blaue
    // Flaeche erscheint und nicht als volle Wassergrafik. Sonst kommt die FLIESSTEXTUR (water_flow) zum Einsatz -- die senkrecht nach
    // unten laufende Grafik, die man an jeder Wasserkante sieht. Fehlt eine Kachel im Atlas,
    // bleibt es bei der Standtextur.
    //
    // Rueckgabe: (Eck-UVs, "ist Overlay"). Das zweite Feld entscheidet ueber die Rueckseite:
    // Vanilla gibt jede Seitenflaeche ein zweites Mal mit vertauschter Wicklung aus -- ausser
    // im Overlay-Fall.
    //
    // Fuer LAVA entfaellt der Overlay-Zweig komplett -- Vanilla haengt ihn an `if (!bl)`,
    // Lava bekommt an Glas/Laub also ganz normal ihr Fliess-Sprite (und damit auch die
    // doppelseitige Ausgabe unten).
    let flow_tile = flow_rect.unwrap_or(side_uv);
    let seite = |neighbor: u32, h1: f32, h2: f32| -> ([[f32; 2]; 4], bool) {
        let n = name_at(neighbor);
        if kind == FluidKind::Water && (is_glass(n) || is_leaves(n)) {
            (water_side_corner_uvs(uvs.water_overlay_rect().unwrap_or(side_uv), h1, h2), true)
        } else {
            (water_side_corner_uvs(flow_tile, h1, h2), false)
        }
    };

    let shade = |brightness: f32| [brightness * tint[0], brightness * tint[1], brightness * tint[2]];

    if render_up {
        let corners = [[x0, y0 + p, z0], [x0, y0 + r, z1], [x1, y0 + q, z1], [x1, y0 + o, z0]];
        // Fliessrichtung dieser Zelle -- dieselbe Funktion, die auch den Schub auf den
        // Spieler liefert (world::fluid), damit Optik und Physik nie auseinanderlaufen
        // koennen. Nur die OBERSEITE bekommt bisher die gedrehte Fliesstextur; die
        // Seitenflaechen bleiben bei der Standtextur (Vanilla texturiert auch sie mit dem
        // Fliess-Sprite, das ist hier noch offen).
        let flow = crate::fluid::flow_velocity_at(block_at, shapes, x, y, z);
        let corner_uv = water_top_corner_uvs(flow, up_uv, flow_rect);
        let oberseite_start = out.len();
        push_quad_with_corner_uvs(out, corners, shade(1.0), corner_uv);
        // NUR die Oberseite doppelseitig, damit man die Wasseroberflaeche auch von unten
        // sieht (man schwimmt darunter). Genau das -- und nur das -- macht Vanillas
        // FluidRenderer ebenfalls: dort werden dieselben vier Vertices ein zweites Mal in
        // umgekehrter Reihenfolge ausgegeben, s. den canFlowTo(up)-Zweig.
        push_rueckseiten(out, oberseite_start);
    }
    if render_down {
        push_face_quad(out, [x0, y0 + w, z0], [x1, y0 + w, z1], [0, -1, 0], shade(0.5), up_uv);
    }
    // Seitenflaechen um EPS NACH INNEN geruecken -- exakt wie Vanilla (FluidRenderer:
    // 0.001 von der Zellkante weg, je Richtung). Ohne das liegt die Wasserflaeche
    // deckungsgleich auf der Flaeche des Nachbarblocks. Bei einem opaken Nachbarn faellt das
    // nicht auf (die Flaeche wird ohnehin geculled), bei GLAS aber sind dann zwei
    // durchsichtige Flaechen exakt gleich tief -- und weil der Translucent-Pass keine Tiefe
    // schreibt, entscheidet die Zeichenreihenfolge zufaellig, welche oben liegt. Sichtbar
    // als blau flackernde Glaskante.
    let (zn, zs) = (z0 + EPS, z1 - EPS);
    let (xw, xe) = (x0 + EPS, x1 - EPS);
    let mut seitenflaeche = |corners: [[f32; 3]; 4], helligkeit: f32, neighbor: u32, h1: f32, h2: f32| {
        let (uv, overlay) = seite(neighbor, h1, h2);
        let start = out.len();
        push_quad_with_corner_uvs(out, corners, shade(helligkeit), uv);
        if !overlay {
            push_rueckseiten(out, start);
        }
    };
    if render_north {
        let corners = [[x0, y0 + w, zn], [x0, y0 + p, zn], [x1, y0 + o, zn], [x1, y0 + w, zn]];
        seitenflaeche(corners, 0.6, north, p, o);
    }
    if render_south {
        let corners = [[x1, y0 + w, zs], [x1, y0 + q, zs], [x0, y0 + r, zs], [x0, y0 + w, zs]];
        seitenflaeche(corners, 0.6, south, q, r);
    }
    if render_west {
        let corners = [[xw, y0 + w, z1], [xw, y0 + r, z1], [xw, y0 + p, z0], [xw, y0 + w, z0]];
        seitenflaeche(corners, 0.8, west, r, p);
    }
    if render_east {
        let corners = [[xe, y0 + w, z0], [xe, y0 + o, z0], [xe, y0 + q, z1], [xe, y0 + w, z1]];
        seitenflaeche(corners, 0.8, east, o, q);
    }
}
