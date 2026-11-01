use super::*;

fn state_von(block: &str, passt: impl Fn(u32) -> bool) -> u32 {
    (0..crate::block_registry::state_count() as u32)
        .find(|&s| block_name(s) == Some(block) && passt(s))
        .unwrap_or_else(|| panic!("{block}-State nicht gefunden"))
}

// Zwei nebeneinanderliegende waterlogged Stufen auf Gras -- die typische Lage am
// Ufer. Nebeneinander ist wichtig: bei einer EINZELNEN Stufe sind alle vier
// Eckhoehen gleich, ein zu hoher Rand faellt dort weit weniger auf.
fn zwei_waterlogged_stufen() -> (World, BlockShapes) {
    let slab = state_von("oak_slab", |s| {
        crate::state_props::property(s, "waterlogged") == Some("true") && crate::state_props::property(s, "type") == Some("bottom")
    });
    let gras = state_von("grass_block", |_| true);
    let untere_haelfte = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
    let mut per_state = vec![None; crate::block_registry::state_count()];
    per_state[slab as usize] = Some(vec![untere_haelfte]);

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = gras;
            states[1] = gras;
            states[16 * 16] = slab;
            states[16 * 16 + 1] = slab;
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);
    (w, BlockShapes::new(per_state))
}

// DER Kern: keine Wasserwand darf hoeher enden als die hoechste Stelle der
// Wasseroberflaeche. Vorher liefen die Waende bis zur Zellkante (y+1) und standen damit
// rund einen Viertelblock ueber dem Wasser ins Leere.
#[test]
fn waende_ragen_nicht_ueber_die_wasseroberflaeche() {
    let (welt, shapes) = zwei_waterlogged_stufen();
    let (_opaque, translucent) = mesh_column(
        &welt,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert!(!translucent.is_empty(), "die Stufen muessen ueberhaupt eine Wasserfuellung liefern");

    let hoechste_flaeche = translucent.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
    // Die Zellkante liegt bei y=2 (Stufe steht auf y=1). Wasser darf spuerbar darunter
    // bleiben -- sonst ist wieder die flache Vollbox am Werk.
    assert!(hoechste_flaeche < 1.95, "kein Wasser-Vertex darf bis an die Zellkante y=2 reichen, hoechster war {hoechste_flaeche}");
}

// Gegenprobe, damit der Test oben nicht bloss "irgendwas ist niedriger" bestaetigt: die
// Waende muessen GENAU bis zur Oberflaeche reichen, nicht darunter aufhoeren (sonst
// klaffte zwischen Wand und Wasserspiegel eine Luecke).
#[test]
fn waende_reichen_bis_an_die_wasseroberflaeche() {
    let (welt, shapes) = zwei_waterlogged_stufen();
    let (_opaque, translucent) = mesh_column(
        &welt,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    // Waagerechte Quads (alle vier Ecken nahezu gleich hoch) sind die Oberflaeche, der
    // Rest sind Waende. Beide muessen oben auf derselben Hoehe enden.
    let mut oben_flaeche = f32::MIN;
    let mut oben_wand = f32::MIN;
    for q in translucent.chunks_exact(6) {
        let ymin = q.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let ymax = q.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        if ymin > 1.5 {
            oben_flaeche = oben_flaeche.max(ymax); // liegt ganz oben -> Oberflaeche
        } else {
            oben_wand = oben_wand.max(ymax);
        }
    }
    // Toleranz etwas ueber EPS (0.001): die Oberflaeche wird bewusst um genau diesen
    // Betrag abgesenkt, damit sie nicht koplanar auf der Wandoberkante liegt und
    // z-fightet. Ein groesserer Abstand waere dagegen eine echte Luecke.
    assert!(
        (oben_flaeche - oben_wand).abs() < 0.005,
        "Wand endet bei {oben_wand}, Oberflaeche bei {oben_flaeche} -- das ist mehr als der EPS-Versatz"
    );
}

// Die Wasserfuellung muss GENAUSO verdecken wie echtes Wasser: nach der tatsaechlichen
// Abdeckung, nicht nach "Nachbar ist nicht Luft". Vorher stand in push_waterlogged_fill
// nur `is_solid(neighbor)` -- damit verschluckte JEDER beliebige Nachbar die Wasserwand,
// auch eine halbhohe Stufe, ein Zaun oder eine Fackel. Genau daran fehlte die
// Fliessflaeche an waterlogged Stufen.
// Vanillas Riss-Ueberlagerung projiziert die Textur in Blockkoordinaten auf die Flaeche
// (SheetedDecalTextureGenerator), sie wird NICHT ueber die Flaeche gestreckt. Auf einem
// vollen Wuerfel deckt deshalb jede Seite genau eine Kachel ab -- und eine untere Stufe
// zeigt an ihren Seiten nur die UNTERE Haelfte des Rissbilds (in der Textur ist das die
// untere Haelfte, also v von 0,5 bis 1,0).
#[test]
fn riss_uv_folgt_vanillas_projektion() {
    let stein = state_von("stone", |_| true);
    let mut per_state = vec![None; crate::block_registry::state_count()];
    per_state[stein as usize] = Some(vec![ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
    let shapes = BlockShapes::new(per_state);
    let luft = |_: i32, _: i32, _: i32| STATE_AIR;

    let voll = block_breaking_vertices([0.0; 3], (0, 0, 0), stein, [0.0, 0.0, 1.0, 1.0], &shapes, &luft);
    assert_eq!(voll.len(), 36, "sechs Seiten mit je zwei Dreiecken");
    for v in &voll {
        assert!((0.0..=1.0).contains(&v.uv[0]) && (0.0..=1.0).contains(&v.uv[1]), "UV bleibt in der Kachel: {:?}", v.uv);
    }
    // Jede Seite spannt die Kachel voll auf.
    let (umin, umax) = voll.iter().fold((1.0f32, 0.0f32), |(a, b), v| (a.min(v.uv[0]), b.max(v.uv[0])));
    assert!((umin - 0.0).abs() < 1e-6 && (umax - 1.0).abs() < 1e-6);

    let mut per_state = vec![None; crate::block_registry::state_count()];
    per_state[stein as usize] =
        Some(vec![ShapeBox { min: [0.0; 3], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
    let halb = BlockShapes::new(per_state);
    let stufe = block_breaking_vertices([0.0; 3], (0, 0, 0), stein, [0.0, 0.0, 1.0, 1.0], &halb, &luft);
    // Nordseite: das eine Sechsergespann, dessen Ecken ALLE auf z = 0 liegen (die
    // Deckel-/Bodenflaechen beruehren z = 0 nur mit zwei Ecken). Ihr v kommt aus der Hoehe.
    let nord: Vec<[f32; 2]> = stufe
        .chunks_exact(6)
        .find(|q| q.iter().all(|v| v.position[2] == 0.0))
        .expect("Nordflaeche vorhanden")
        .iter()
        .map(|v| v.uv)
        .collect();
    let (vmin, vmax) = nord.iter().fold((1.0f32, 0.0f32), |(a, b), uv| (a.min(uv[1]), b.max(uv[1])));
    assert!((vmin - 0.5).abs() < 1e-6 && (vmax - 1.0).abs() < 1e-6, "untere Blockhaelfte -> untere Texturhaelfte, war {vmin}..{vmax}");
}

#[test]
fn fuellung_culled_nach_abdeckung_nicht_nach_luft() {
    let waterlogged = state_von("oak_slab", |s| {
        crate::state_props::property(s, "waterlogged") == Some("true") && crate::state_props::property(s, "type") == Some("bottom")
    });
    let trocken = state_von("oak_slab", |s| {
        crate::state_props::property(s, "waterlogged") == Some("false") && crate::state_props::property(s, "type") == Some("bottom")
    });
    let stein = state_von("stone", |_| true);
    let untere_haelfte = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
    let mut per_state = vec![None; crate::block_registry::state_count()];
    per_state[waterlogged as usize] = Some(vec![untere_haelfte]);
    per_state[trocken as usize] = Some(vec![untere_haelfte]);
    // Stein steht hier fuer "voller Wuerfel deckt ab" -- der Eintrag muss also da sein,
    // seit ein eintragsloser Nachbar nichts mehr verdeckt (s. has_resolved_shape).
    per_state[stein as usize] =
        Some(vec![ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
    let shapes = BlockShapes::new(per_state);

    // Zaehlt die Quads der Ost-Wand (x nahe der Zellgrenze 1.0) der Fuellung bei (0,0,0).
    let ostwand = |nachbar: u32| {
        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = waterlogged;
                states[1] = nachbar;
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        let (_o, tr) = mesh_column(
            &w,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        tr.chunks_exact(6).filter(|q| q.iter().all(|v| (v.position[0] - 1.0).abs() < 0.01)).count()
    };

    assert!(ostwand(0) > 0, "an Luft muss die Wand da sein");
    assert_eq!(ostwand(trocken), ostwand(0), "eine halbhohe Nachbarstufe deckt die Wasserhoehe nicht ab -- die Wand muss bleiben");
    assert_eq!(ostwand(stein), 0, "ein voller Wuerfel deckt die Flaeche ab -- die Wand muss weg");
}

// Keine Wasserwand darf EXAKT auf einer Zellgrenze liegen. Genau dort liegt naemlich die
// opake Seitenflaeche der Stufe selbst, und zwei deckungsgleiche Flaechen haben
// bit-identische interpolierte Tiefe -- der Translucent-Pass testet mit LESS (s.
// render::block_renderer), die Wasserwand faellt dann durch den Tiefentest und ist
// unsichtbar. Echtes Wasser hat diesen Versatz seit laengerem, die waterlogged-Fuellung
// war der letzte Pfad ohne ihn (die Geometrie war da, sie kam nur nie durch die
// Tiefenpruefung).
#[test]
fn waende_liegen_nicht_auf_der_zellgrenze() {
    let (welt, shapes) = zwei_waterlogged_stufen();
    let (_opaque, translucent) = mesh_column(
        &welt,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    // Waende erkennt man daran, dass sie in x ODER z keine Ausdehnung haben.
    for q in translucent.chunks_exact(6) {
        let spanne = |achse: usize| {
            let min = q.iter().map(|v| v.position[achse]).fold(f32::MAX, f32::min);
            let max = q.iter().map(|v| v.position[achse]).fold(f32::MIN, f32::max);
            (min, max)
        };
        for achse in [0, 2] {
            let (min, max) = spanne(achse);
            if (max - min).abs() > 1e-6 {
                continue; // in dieser Achse ausgedehnt -> keine Wand senkrecht dazu
            }
            // Die beiden Stufen liegen bei x=0..2, z=0..1 -- ganzzahlige Ebenen sind hier
            // die Zellgrenzen. Die einzige erlaubte Ausnahme waere eine Wand zwischen zwei
            // Zellen, die es hier gar nicht gibt (beide Stufen grenzen aneinander und
            // cullen die gemeinsame Flaeche weg).
            let abstand_zur_ganzzahl = (min - min.round()).abs();
            assert!(
                abstand_zur_ganzzahl > 1e-4,
                "Wand liegt bei Achse {achse} exakt auf der Zellgrenze {min} -- ohne Versatz verliert sie den Tiefentest"
            );
        }
    }
}
