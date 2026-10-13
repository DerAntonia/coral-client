// Stellt nach, was die App beim ersten empfangenen Chunk tut: eine einzelne Spalte
// einfuegen und meshen. Genau dieser Fall lieferte im Client ein leeres Mesh, obwohl
// derselbe Mesher im Loopback-Test Millionen Vertices erzeugt hat.

use world::chunk::SECTION_VOLUME;
use world::{ChunkSection, World};

// Eine Spalte aus 24 Sections; nur `solid_sections` ganz unten sind gefuellt.
fn column(solid_sections: usize) -> Vec<ChunkSection> {
    (0..24)
        .map(|i| {
            let state = if i < solid_sections { 1 } else { 0 };
            ChunkSection::from_states(vec![state; SECTION_VOLUME])
        })
        .collect()
}

#[test]
fn single_column_produces_geometry() {
    let mut world = World::new();
    world.insert_column(6, 12, column(9));

    let dirty = world.take_dirty(usize::MAX);
    assert_eq!(dirty, vec![(6, 12)], "die eingefuegte Spalte muss zum Meshen anstehen");

    let (vertices, _translucent) = world::mesh_column(
        &world,
        6,
        12,
        &world::BlockUvs::single_tile(),
        &world::BlockShapes::all_full_cubes(),
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
    );
    assert!(!vertices.is_empty(), "eine Spalte mit 9 vollen Sections darf kein leeres Mesh ergeben");
    println!("{} Vertices fuer eine einzelne Spalte", vertices.len());
}

// Der eigentliche Zweck der Nachbar-Abfrage ueber die Spaltengrenze hinaus (s. Kommentar in
// mesher.rs): sobald die Ost-Nachbarspalte geladen ist, muss die Ost-Seite jedes Blocks an
// der gemeinsamen Grenze verschwinden -- die ist von aussen nie sichtbar. Bricht das (z.B.
// durch einen Vorzeichenfehler beim Nachbar-Lookup ueber Arc<ChunkColumn>), waere die
// Geometrie an jeder Chunk-Grenze dauerhaft verdoppelt.
//
// Ueber Vertex-POSITIONEN pruefen geht hier nicht zuverlaessig: Nord/Sued-Quads von
// Bloecken direkt an der Grenze beruehren an einer Ecke ebenfalls x=SECTION_SIZE, ohne
// etwas mit Ost/West-Culling zu tun zu haben. Stattdessen die Vertex-ANZAHL vergleichen:
// mit geladenem Ost-Nachbarn muss sie um genau die Zahl der jetzt verdeckten Ost-Quads
// sinken (SECTION_SIZE Bloecke tief * SECTION_SIZE*24 Bloecke hoch, 6 Vertices je Quad).
#[test]
fn ost_nachbar_entfernt_genau_die_grenzflaeche() {
    let mut world = World::new();
    world.insert_column(0, 0, column(24));
    let vertices_alone = world::mesh_column(
        &world,
        0,
        0,
        &world::BlockUvs::single_tile(),
        &world::BlockShapes::all_full_cubes(),
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
    )
    .0
    .len();

    world.insert_column(1, 0, column(24));
    let vertices_with_neighbor = world::mesh_column(
        &world,
        0,
        0,
        &world::BlockUvs::single_tile(),
        &world::BlockShapes::all_full_cubes(),
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
    )
    .0
    .len();

    let section_size = world::chunk::SECTION_SIZE as usize;
    let expected_removed_quads = section_size * (section_size * 24);
    let expected_removed_vertices = expected_removed_quads * 6;

    assert_eq!(
        vertices_alone - vertices_with_neighbor,
        expected_removed_vertices,
        "genau die Ost-Grenzflaeche muss verschwinden, sobald der Nachbar geladen ist"
    );
}

// Entladen (Forget Level Chunk): die Spalte verschwindet, und der verbleibende Nachbar wird
// neu zum Meshen markiert -- seine zur Luecke zeigende Seite braucht jetzt wieder Flaechen.
// Ausserdem muss die volle Grenzflaeche exakt zurueckkehren (Gegenprobe zum Culling-Test
// oben): nach dem Entladen des Ost-Nachbarn hat die Spalte wieder so viele Vertices wie
// allein.
#[test]
fn entladen_markiert_nachbarn_neu_und_stellt_grenzflaeche_wieder_her() {
    let mut world = World::new();
    world.insert_column(0, 0, column(24));
    let vertices_alone = world::mesh_column(
        &world,
        0,
        0,
        &world::BlockUvs::single_tile(),
        &world::BlockShapes::all_full_cubes(),
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
    )
    .0
    .len();

    world.insert_column(1, 0, column(24));
    let _ = world.take_dirty(usize::MAX); // frisch gemeshte Spalten "abarbeiten"
    assert!(world.take_dirty(usize::MAX).is_empty(), "nach dem Abarbeiten nichts mehr offen");

    assert!(world.remove_column(1, 0), "eine geladene Spalte zu entladen meldet true");
    assert!(world.column(1, 0).is_none(), "die entladene Spalte ist weg");
    assert_eq!(world.take_dirty(usize::MAX), vec![(0, 0)], "der verbleibende Nachbar muss zum Neu-Meshen anstehen");

    let vertices_after_unload = world::mesh_column(
        &world,
        0,
        0,
        &world::BlockUvs::single_tile(),
        &world::BlockShapes::all_full_cubes(),
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
    )
    .0
    .len();
    assert_eq!(vertices_after_unload, vertices_alone, "die Grenzflaeche muss nach dem Entladen zurueck sein");

    assert!(!world.remove_column(1, 0), "eine nicht geladene Spalte zu entladen meldet false");
}

// Die unterste Blockreihe (Weltboden, y=MIN_Y) darf keine Boden-Flaeche bekommen: darunter
// gibt es nichts, und dieser Client kennt keinen Weg unter die Welt, um sie je zu sehen (s.
// Kommentar an SOLID_BELOW_WORLD in mesher.rs). Bei einer isolierten, komplett vollen Spalte
// laesst sich die erwartete Vertex-Zahl exakt herleiten: vier Seitenwaende ueber die volle
// Hoehe (kein Nachbar geladen -> "Luft" an allen vier Raendern) plus die Oben-Flaeche
// (weiterhin sichtbar, unveraendert) -- aber keine Unten-Flaeche.
#[test]
fn weltboden_bekommt_keine_unsichtbare_bodenflaeche() {
    let mut world = World::new();
    world.insert_column(0, 0, column(24));

    let vertices = world::mesh_column(
        &world,
        0,
        0,
        &world::BlockUvs::single_tile(),
        &world::BlockShapes::all_full_cubes(),
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
    )
    .0
    .len();

    let section_size = world::chunk::SECTION_SIZE as usize;
    let height = section_size * 24;
    let side_quads = 4 * height * section_size; // West/Ost/Nord/Sued ueber die volle Hoehe
    let top_quads = section_size * section_size; // Oben-Flaeche bleibt sichtbar
    let expected_vertices = (side_quads + top_quads) * 6;

    assert_eq!(
        vertices,
        expected_vertices,
        "keine Unten-Flaeche erwartet -- {} Vertices zu viel (= {} Quads)",
        vertices as i64 - expected_vertices as i64,
        (vertices as i64 - expected_vertices as i64) / 6
    );
}

// Weiche Beleuchtung im echten Mesher: ein Boden, darauf ein einzelner Block. Die Oberseite des
// Bodens direkt neben dem Block muss an der anliegenden Kante dunkler sein als weit davon
// entfernt -- genau die Eckabdunklung, die Vanilla "Smooth Lighting" nennt. Ohne den Schalter
// bleibt jede Oberseite gleich hell.
#[test]
fn weiche_beleuchtung_dunkelt_die_kante_am_nachbarblock() {
    let stein = (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).unwrap();
    let mut world = World::new();
    // Nur die unterste Section: Boden in y = MIN_Y..MIN_Y+15 komplett Stein.
    let mut sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::from_states(vec![0; SECTION_VOLUME])).collect();
    sections[0] = ChunkSection::from_states(vec![stein; SECTION_VOLUME]);
    world.insert_column(0, 0, sections);
    let boden_oben = world::MIN_Y + 15; // oberste Bodenschicht
    // Ein Block auf dem Boden bei x=8, z=8.
    world.set_block(8, boden_oben + 1, 8, stein);
    let neighbors = world.column_neighbors(0, 0).unwrap();
    let shapes = world::BlockShapes::all_full_cubes();

    let mesh = |staerke: f32| {
        world::mesh_column_with_lighting(
            0,
            0,
            &neighbors,
            &world::BlockUvs::single_tile(),
            &shapes,
            &world::BiomePalette::fallback(),
            &world::CrossBlocks::none(),
            &world::BlockShapes::all_full_cubes(),
            staerke,
            false,
        )
        .0
    };
    // Die Oberseite des Bodens liegt bei y = boden_oben + 1 (Spalten-lokal = Weltkoordinate).
    let oben_y = (boden_oben + 1) as f32;
    // Nur WAAGERECHTE Dreiecke auf Bodenhoehe -- an der Ecke (8,8) enden sonst auch die
    // Seitenflaechen des aufgesetzten Blocks, und die haben ihre eigene Richtungs-Helligkeit.
    let helligkeit = |verts: &[world::BlockVertex], x: f32, z: f32| {
        verts
            .chunks_exact(3)
            .filter(|dreieck| dreieck.iter().all(|v| (v.position[1] - oben_y).abs() < 1e-4))
            .flatten()
            .filter(|v| (v.position[0] - x).abs() < 1e-4 && (v.position[2] - z).abs() < 1e-4)
            .map(|v| v.color[0])
            .fold(f32::INFINITY, f32::min)
    };

    let weich = mesh(1.0);
    let flach = mesh(0.0);
    // Die Ecke (8,8) beruehrt den aufgesetzten Block, (2,2) liegt frei.
    let an_der_kante = helligkeit(&weich, 8.0, 8.0);
    let frei = helligkeit(&weich, 2.0, 2.0);
    assert!(an_der_kante < frei - 0.1, "Kante {an_der_kante} muss deutlich dunkler sein als frei {frei}");
    assert!((frei - 1.0).abs() < 1e-4, "eine freie Oberseite bleibt voll hell: {frei}");
    // Ohne weiche Beleuchtung gibt es keinen Unterschied.
    assert!((helligkeit(&flach, 8.0, 8.0) - helligkeit(&flach, 2.0, 2.0)).abs() < 1e-6);
    // Halbe Staerke: genau die halbe Abdunklung.
    let halb = helligkeit(&mesh(0.5), 8.0, 8.0);
    assert!((halb - (1.0 + an_der_kante) / 2.0).abs() < 1e-4, "halbe Staerke {halb}, voll {an_der_kante}");
}

// Weiche Beleuchtung an einer Gras-Stufe: die Oberseite eines Grasblocks direkt vor einer
// hoeheren Wand muss an der Wandkante dunkler sein als an der freien Kante.
#[test]
fn grasoberseite_vor_einer_wand_bekommt_eckabdunklung() {
    let state_of = |name: &str| (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap();
    let gras = state_of("grass_block");
    let mut world = World::new();
    world.insert_column(0, 0, column(0));
    for x in 0..6 {
        for z in 0..6 {
            world.set_block(x, 0, z, gras);
        }
    }
    // Wand eine Lage hoeher bei x = 3.
    for z in 0..6 {
        world.set_block(3, 1, z, gras);
    }
    let neighbors = world.column_neighbors(0, 0).unwrap();
    let (verts, _) = world::mesh_column_with_lighting(
        0,
        0,
        &neighbors,
        &world::BlockUvs::single_tile(),
        &world::BlockShapes::all_full_cubes(),
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
        1.0,
        false,
    );
    // Oberseite des Blocks (2, 0, 2): Ebene y = 1, x in [2, 3], z in [2, 3].
    let oben: Vec<_> = verts
        .iter()
        .filter(|v| (v.position[1] - 1.0).abs() < 1e-4 && (2.0..=3.0).contains(&v.position[0]) && (2.0..=3.0).contains(&v.position[2]))
        .collect();
    assert!(!oben.is_empty(), "Oberseite fehlt");
    let hell = |x: f32| oben.iter().filter(|v| (v.position[0] - x).abs() < 1e-4).map(|v| v.color[1]).fold(0.0f32, f32::max);
    println!("freie Kante {} / Wandkante {}", hell(2.0), hell(3.0));
    assert!(hell(3.0) < hell(2.0) * 0.9, "Wandkante {} nicht dunkler als freie Kante {}", hell(3.0), hell(2.0));
}

// Einblenden nach einer Platzierung: mit der Platzierung als Quelle liefert der Mesher fuer die
// Flaechen, deren Eckabdunklung sich geaendert hat, die Fassung "vorher" -- am Boden neben dem
// Block heller als jetzt, am Block selbst flach wie der Geisterblock. Ohne Quelle nichts.
#[test]
fn einblenden_liefert_eckabdunklung_wie_vorher() {
    let stein = (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).unwrap();
    let mut world = World::new();
    let mut sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::from_states(vec![0; SECTION_VOLUME])).collect();
    sections[0] = ChunkSection::from_states(vec![stein; SECTION_VOLUME]);
    world.insert_column(0, 0, sections);
    let block = (8, world::MIN_Y + 16, 8);
    world.set_block(block.0, block.1, block.2, stein);
    let neighbors = world.column_neighbors(0, 0).unwrap();
    let mesh = |fades: &[world::AoFadeSource]| {
        world::mesh_column_ao_fading(
            0,
            0,
            &neighbors,
            &world::BlockUvs::single_tile(),
            &world::BlockShapes::all_full_cubes(),
            &world::BiomePalette::fallback(),
            &world::CrossBlocks::none(),
            &world::BlockShapes::all_full_cubes(),
            1.0,
            false,
            fades,
        )
    };
    let (_, _, ohne) = mesh(&[]);
    assert!(ohne.is_empty());
    let (opak, _, quads) = mesh(&[(block, 0)]);
    assert!(!quads.is_empty(), "keine Flaeche zum Einblenden");
    for q in &quads {
        assert_eq!(q.before.len(), q.after.len());
        // `after` ist genau das, was im Mesh steht.
        let s = q.start as usize;
        assert!(q.after.iter().zip(&opak[s..s + q.after.len()]).all(|(a, b)| a.position == b.position && a.color == b.color));
        // Vorher nie dunkler: die Platzierung kann nur abdunkeln.
        assert!(q.before.iter().zip(&q.after).all(|(v, n)| v.color[0] >= n.color[0] - 1e-6));
    }
    // Der Boden direkt am Block (Ecke 8,8 auf der Bodenoberseite) war vorher voll hell.
    let oben_y = block.1 as f32;
    let boden = quads
        .iter()
        .flat_map(|q| q.before.iter().zip(&q.after))
        .find(|(v, _)| (v.position[1] - oben_y).abs() < 1e-4 && v.position[0] == 8.0 && v.position[2] == 8.0)
        .expect("Bodenecke am Block fehlt");
    assert!((boden.0.color[0] - 1.0).abs() < 1e-4 && boden.1.color[0] < 0.9, "vorher {} / jetzt {}", boden.0.color[0], boden.1.color[0]);
}

// Pilzbloecke laufen ueber den Modell-Vierecke-Pfad (sechs Einzelflaechen mit eigenen
// Texturen). Liegen alle Vierecke auf der Zellgrenze, ist der Block ein Vollwuerfel: er wirft
// Eckabdunklung UND bekommt sie selbst.
#[test]
fn pilzblock_aus_modellvierecken_hat_eckabdunklung() {
    let state_of = |name: &str| (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap();
    let stein = state_of("stone");
    let pilz = state_of("red_mushroom_block");
    // Sechs Grenzflaechen eines Wuerfels (Ecken in beliebigem Umlauf -- fuer AO zaehlt nur die Lage).
    let flaeche = |corners: [[f32; 3]; 4]| world::ModelFace { corners, uvs: [[0.0, 0.0]; 4], shade: 1.0 };
    let wuerfel = vec![
        flaeche([[0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
        flaeche([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
        flaeche([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]),
        flaeche([[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
        flaeche([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]]),
        flaeche([[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
    ];
    let mut shapes = world::BlockShapes::all_full_cubes();
    let mut quads: Vec<Option<Vec<Vec<world::ModelFace>>>> = (0..world::state_count()).map(|_| None).collect();
    quads[pilz as usize] = Some(vec![wuerfel]);
    shapes.set_model_quads(quads);
    shapes.build_ao_flags(&Default::default());
    assert_ne!(shapes.ao_flags(pilz) & world::ambient_occlusion::AO_FULL_CUBE, 0, "Pilzblock muss als Vollwuerfel gelten");

    let mut welt = World::new();
    welt.insert_column(0, 0, column(0));
    welt.set_block(8, 0, 8, pilz);
    // Ein Stein schraeg darueber: die Oberseite des Pilzblocks dunkelt zur Kante x=9 hin ab.
    welt.set_block(9, 1, 8, stein);
    let neighbors = welt.column_neighbors(0, 0).unwrap();
    let (verts, _) = world::mesh_column_with_lighting(
        0,
        0,
        &neighbors,
        &world::BlockUvs::single_tile(),
        &shapes,
        &world::BiomePalette::fallback(),
        &world::CrossBlocks::none(),
        &world::BlockShapes::all_full_cubes(),
        1.0,
        false,
    );
    // Nur ganze waagerechte Dreiecke -- die Seitenflaechen enden mit ihrer Oberkante auch auf y = 1.
    let oben: Vec<_> = verts
        .chunks_exact(3)
        .filter(|d| d.iter().all(|v| (v.position[1] - 1.0).abs() < 1e-4 && (8.0..=9.0).contains(&v.position[0]) && (8.0..=9.0).contains(&v.position[2])))
        .flatten()
        .collect();
    assert!(!oben.is_empty(), "Oberseite des Pilzblocks fehlt");
    let hell = |x: f32| oben.iter().filter(|v| (v.position[0] - x).abs() < 1e-4).map(|v| v.color[0]).fold(0.0f32, f32::max);
    assert!(hell(9.0) < hell(8.0) * 0.9, "Kante am Stein {} nicht dunkler als frei {}", hell(9.0), hell(8.0));
}
