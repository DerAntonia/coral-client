use super::*;
use crate::block_registry::{all_block_names, state_count};

// Vertexzahl fuer `flaechen` sichtbare Wasserflaechen, davon `doppelseitig` viele mit
// Rueckseite. Jede Flaeche sind 2 Dreiecke a 3 Vertices; doppelseitig sind die Oberseiten
// (man schaut beim Schwimmen von unten dagegen) und alle Seitenflaechen ausser denen zu
// Glas/Laub (s. push_rueckseiten) -- also praktisch: alles ausser Unterseiten und
// Overlay-Seiten. Als Funktion statt als nackte Zahl, damit an jeder Zusicherung ablesbar
// bleibt, WOHER die Zahl kommt -- und damit eine kuenftige Aenderung an der
// Doppelseitigkeit genau einmal auffaellt statt in fuenf fremden Zahlen.
fn wasser_vertices(flaechen: usize, doppelseitig: usize) -> usize {
    (flaechen + doppelseitig) * 6
}

fn state_of(block: &str, matches: impl Fn(u32) -> bool) -> u32 {
    (0..state_count() as u32).find(|&s| block_name(s) == Some(block) && matches(s)).expect("State nicht gefunden")
}

// Baut eine Spalte mit genau einem einzelnen Block bei Welt (0,0,0), sonst nur Luft --
// isoliert genug, dass alle sechs Flaechen des Blocks an Luft grenzen (keine Kulissen).
fn single_block_world(state: u32) -> World {
    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            // Section 4 beginnt bei MIN_Y + 4*16 = 0 -- lokal (0,0,0) ist also Welt-y=0.
            states[0] = state;
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);
    w
}

// Regressionstest: is_solid() sagt nur "kein
// Luft", nicht "blockiert die Sicht" -- Wasser ist solid, aber transparent. Vor dem Fix
// hat ein Stein neben Wasser seine gemeinsame Flaeche wie gegen einen echten Vollwuerfel
// geculled (unsichtbar, solange Wasser selbst noch opak gezeichnet wurde) -- seit Wasser
// echtes Alpha-Blending hat, waere das ein sichtbares Loch. Stein bei (0,0,0), Wasser bei
// (1,0,0): Stein muss trotzdem alle sechs Seiten zeigen (auch die zum Wasser hin).
#[test]
fn stein_neben_wasser_behaelt_seine_flaeche() {
    let stone = state_of("stone", |_| true);
    let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = stone; // lokal (0,0,0)
            states[1] = water; // lokal (1,0,0)
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);

    let (opaque, translucent) = mesh_column(
        &w,
        0,
        0,
        &BlockUvs::single_tile(),
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    // 6 Seiten * 6 Vertices (2 Dreiecke) = 36 -- KEINE darf gegen das Wasser geculled sein.
    assert_eq!(opaque.len(), 36, "Stein muss trotz Wassernachbar alle sechs Seiten zeigen");
    // Wasser selbst culled weiterhin normal gegen den soliden Steinnachbarn: nur 5 der 6
    // Seiten (die zum Stein hin nicht).
    assert_eq!(translucent.len(), wasser_vertices(5, 4), "Wasser muss weiterhin gegen den soliden Nachbarn culln");
}

// Regressionstest fuer das sichtbare Kastennaht-Raster um jede Seegras-Zelle unter
// Wasser: eine echte Wasserzelle direkt
// neben Seegras deckte dessen duenne Ebenen-Form (neighbor_covers_face) nie voll ab und
// zeichnete deshalb ihre eigene Grenzflaeche -- obwohl Seegras (always_wet, s.
// world::fluid) genauso "nass" ist wie echtes Wasser und beide denselben Wasserkoerper
// bilden sollten, ohne sichtbare interne Naht.
#[test]
fn wasser_neben_seegras_culled_die_gemeinsame_flaeche() {
    let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
    let seagrass = state_of("seagrass", |_| true);
    let z_plane = ShapeBox {
        min: [0.0, 0.0, 0.25],
        max: [1.0, 1.0, 0.25],
        faces: [false, false, true, true, false, false],
        content: [false; 6],
        uv: [None; 6],
    };
    let x_plane = ShapeBox {
        min: [0.75, 0.0, 0.0],
        max: [0.75, 1.0, 1.0],
        faces: [false, false, false, false, true, true],
        content: [false; 6],
        uv: [None; 6],
    };
    let mut per_state = vec![None; state_count()];
    per_state[seagrass as usize] = Some(vec![z_plane, x_plane]);
    let shapes = BlockShapes::new(per_state);

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = water; // lokal (0,0,0)
            states[1] = seagrass; // lokal (1,0,0)
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);

    let (_opaque, translucent) = mesh_column(
        &w,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    // translucent enthaelt sowohl Wasser (5 Seiten -- die zum Seegras hin geculled, sonst
    // ueberall Luft) als auch die Seegras-Wasserfuellung (5 Seiten -- die zum Wasser hin
    // von push_waterlogged_fill schon vorher korrekt geculled, sonst ueberall Luft):
    // 10 Flaechen, davon 8 doppelseitig (je Zelle Oberseite + 3 Seitenflaechen, s.
    // wasser_vertices). OHNE den Fix waere Wassers Ost-Seite (zum Seegras) NICHT geculled
    // (die duennen Ebenen decken die Grenze ja nie voll ab) -- eine Flaeche mehr, mit
    // einer sichtbaren Naht genau an dieser Grenze.
    assert_eq!(translucent.len(), wasser_vertices(10, 8), "Wasser darf keine sichtbare Naht gegen Seegras zeichnen");
}

// Ende-zu-Ende-Gegenstueck zu water_uv_tests: ein einzelner Wasserblock in Luft muss seine
// vier SEITENFLAECHEN aus der Fliesskachel texturieren (und nur die -- Ober- und Unterseite
// bleiben bei der Standkachel), und jede davon doppelseitig. Damit ist die ganze Kette
// abgesichert, nicht bloss die UV-Formel: frueher fehlte die "Wasser fliesst nach unten"-Textur an den
// Seiten komplett, auch von innen, wenn man im Wasser stand.
#[test]
fn wasser_seitenflaechen_kommen_aus_der_fliesskachel() {
    const FLOW: [f32; 4] = [0.5, 0.5, 0.75, 0.75];
    let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
    let uvs = BlockUvs::single_tile().with_water_flow(Some(FLOW));
    let world = single_block_world(water);
    let (_opaque, translucent) = mesh_column(
        &world,
        0,
        0,
        &uvs,
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );

    let aus_fliesskachel = |v: &BlockVertex| {
        v.uv[0] >= FLOW[0] - 1e-5 && v.uv[0] <= FLOW[2] + 1e-5 && v.uv[1] >= FLOW[1] - 1e-5 && v.uv[1] <= FLOW[3] + 1e-5
    };
    let seiten: Vec<&BlockVertex> = translucent.iter().filter(|v| aus_fliesskachel(v)).collect();
    // 4 Seiten * 6 Vertices, doppelseitig = 48. Die Standkachel liegt bei [0,0,1,1] und
    // umschliesst die Fliesskachel; Ober-/Unterseite nutzen davon aber nur die Eckpunkte
    // 0 und 1, fallen also nicht in den Bereich 0.5..0.75.
    assert_eq!(seiten.len(), 48, "vier doppelseitige Seitenflaechen aus der Fliesskachel erwartet");
    // Und sie muessen waagerecht wirklich nur die halbe Kachel nutzen (Vanillas
    // getFrameU(0.0)..getFrameU(0.5)) -- sonst waere die Grafik doppelt so breit gezogen.
    let max_u = seiten.iter().fold(f32::MIN, |a, v| a.max(v.uv[0]));
    assert!((max_u - 0.625).abs() < 1e-5, "die Seitenflaeche darf nur die halbe Kachelbreite nutzen, war bis {max_u}");
}

#[test]
fn wasser_landet_im_translucent_pass() {
    let _ = all_block_names(); // Registry sicher geladen
    let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
    let world = single_block_world(water);
    let (opaque, translucent) = mesh_column(
        &world,
        0,
        0,
        &BlockUvs::single_tile(),
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert!(!translucent.is_empty(), "ein Wasserblock muss Vertices in den Translucent-Pass liefern");
    assert!(opaque.is_empty(), "ein reiner Wasserblock darf keine Opak-Vertices erzeugen");
}

// Regressionstest fuer das Loch um Seegras unter Wasser: Kelp ist ein Kreuz-Block (Modell "block/cross") OHNE
// "waterlogged"-Property -- der fruehe `continue` im Kreuz-Zweig hat die Wasserfuellung
// komplett uebersprungen, weil sie vorher nur im Box-Pfad haengte.
#[test]
fn kelp_hat_wasserfuellung_trotz_kreuz_pfad() {
    let kelp = state_of("kelp", |_| true);
    let cross = CrossBlocks::new({
        let mut v = vec![false; state_count()];
        v[kelp as usize] = true;
        v
    });
    let world = single_block_world(kelp);
    let (opaque, translucent) = mesh_column(
        &world,
        0,
        0,
        &BlockUvs::single_tile(),
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &cross,
        &BlockShapes::all_full_cubes(),
    );
    assert!(!opaque.is_empty(), "Kelp selbst muss weiterhin als Kreuz gezeichnet werden");
    assert!(!translucent.is_empty(), "Kelp muss trotz Kreuz-Pfad eine Wasserfuellung bekommen");
}

// Zeitversetzte Animationen (desync_animations): ein ebenes Seegrasfeld (alle Bloecke auf
// derselben Hoehe) muss ueber alle Kopien streuen, und Nachbarn duerfen nicht gehaeuft
// dieselbe nehmen.
#[test]
fn animierte_kacheln_streuen_ueber_ein_ebenes_feld() {
    let seagrass = state_of("seagrass", |_| true);
    let mut per_state = vec![[[0.0f32; 4]; 6]; state_count()];
    // Raster 4x4: Basis Kachel 1, Kopien 4..=10 -- jede Kopie in eigener Kachel.
    per_state[seagrass as usize] = [[0.25, 0.0, 0.5, 0.25]; 6];
    let uvs = BlockUvs::new(per_state, [[0.0; 4]; 6]).with_anim_variants(4, &[(1, (4..=10).collect())]);
    let plane = ShapeBox { min: [0.0, 0.0, 0.25], max: [1.0, 1.0, 0.25], faces: [false, false, true, true, false, false], content: [false; 6], uv: [None; 6] };
    let mut shapes = vec![None; state_count()];
    shapes[seagrass as usize] = Some(vec![plane]);
    let shapes = BlockShapes::new(shapes);

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[..256].fill(seagrass); // eine volle Lage bei y = 0
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);
    let (opaque, _) = mesh_column(&w, 0, 0, &uvs, &shapes, &BiomePalette::fallback(), &CrossBlocks::none(), &BlockShapes::all_full_cubes());

    // Kachel je Block ueber die Schwerpunkte seiner Dreiecke; alle eines Blocks muessen
    // dieselbe Kopie zeigen.
    let mut kachel = [[None::<usize>; 16]; 16];
    for tri in opaque.chunks_exact(3) {
        let mitte = |f: &dyn Fn(&BlockVertex) -> f32| tri.iter().map(f).sum::<f32>() / 3.0;
        let (u, v) = (mitte(&|v| v.uv[0]), mitte(&|v| v.uv[1]));
        let (x, z) = (mitte(&|v| v.position[0]).floor() as usize, mitte(&|v| v.position[2]).floor() as usize);
        let k = (v * 4.0) as usize * 4 + (u * 4.0) as usize;
        assert!(kachel[x][z].is_none_or(|alt| alt == k), "Block ({x}, {z}) mischt Kopien");
        kachel[x][z] = Some(k);
    }
    let kachel = kachel.map(|reihe| reihe.map(|k| k.expect("Block ohne Flaeche")));
    let alle: std::collections::BTreeSet<usize> = kachel.iter().flatten().copied().collect();
    assert_eq!(alle, std::iter::once(1).chain(4..=10).collect(), "jede Kopie muss vorkommen");
    let gleich = (0..15).flat_map(|x| (0..16).map(move |z| (x, z))).filter(|&(x, z)| kachel[x][z] == kachel[x + 1][z]).count();
    assert!(gleich < 240 / 4, "zu viele Nachbarn mit derselben Kopie: {gleich} von 240");
}

// Regressionstest fuer denselben Bug bei Seegras (Screenshot): sein Modell
// "template_seagrass" nutzt NICHT rotierte Nullstaerke-Ebenen (zwei duenne, versetzte
// Platten, KEINE an der Zellgrenze), ist also KEIN Kreuz-Block in dieser Klassifikation
// (state_is_cross verlangt rotation.is_some() bei ALLEN Elementen) -- laeuft durch den
// Box-Pfad, hat aber ebenfalls keine "waterlogged"-Property (existiert in Vanilla nie
// trocken). Braucht wie der Zaun-Test echte (nicht randstaendige) Boxen statt
// all_full_cubes(), sonst waere own_faces wieder faelschlich ueberall true.
#[test]
fn seegras_hat_wasserfuellung_ohne_waterlogged_property() {
    let seagrass = state_of("seagrass", |_| true);
    assert_eq!(crate::state_props::property(seagrass, "waterlogged"), None, "Seegras hat in Vanilla keine waterlogged-Property");

    let z_plane = ShapeBox {
        min: [0.0, 0.0, 0.25],
        max: [1.0, 1.0, 0.25],
        faces: [false, false, true, true, false, false],
        content: [false; 6],
        uv: [None; 6],
    };
    let x_plane = ShapeBox {
        min: [0.75, 0.0, 0.0],
        max: [0.75, 1.0, 1.0],
        faces: [false, false, false, false, true, true],
        content: [false; 6],
        uv: [None; 6],
    };
    let mut per_state = vec![None; state_count()];
    per_state[seagrass as usize] = Some(vec![z_plane, x_plane]);
    let shapes = BlockShapes::new(per_state);

    let world = single_block_world(seagrass);
    let (opaque, translucent) = mesh_column(
        &world,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert!(!opaque.is_empty(), "Seegras selbst muss weiterhin gezeichnet werden");
    assert!(!translucent.is_empty(), "Seegras muss trotz fehlender waterlogged-Property eine Wasserfuellung bekommen");
}

// Braucht eine ECHTE (duenne) Zaunpfosten-Box statt BlockShapes::all_full_cubes(): ein
// Vollwuerfel-Fallback wuerde jede Richtung als "voll abgedeckt" zaehlen und die
// Wasserfuellung faelschlich komplett unterdruecken (own_faces waere ueberall true) --
// genau der Fall, den own_faces' Vollabdeckungs-Check (s. Aufrufer) unterscheiden soll.
#[test]
fn waterlogged_zaun_hat_wasserfuellung_und_bleibt_opak() {
    let fence = state_of("oak_fence", |s| {
        crate::state_props::property(s, "waterlogged") == Some("true")
            && crate::state_props::property(s, "north") == Some("false")
            && crate::state_props::property(s, "south") == Some("false")
            && crate::state_props::property(s, "east") == Some("false")
            && crate::state_props::property(s, "west") == Some("false")
    });
    let post = ShapeBox { min: [0.375, 0.0, 0.375], max: [0.625, 1.0, 0.625], faces: [true; 6], content: [false; 6], uv: [None; 6] };
    let mut per_state = vec![None; state_count()];
    per_state[fence as usize] = Some(vec![post]);
    let shapes = BlockShapes::new(per_state);

    let world = single_block_world(fence);
    let (opaque, translucent) = mesh_column(
        &world,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert!(!opaque.is_empty(), "der Zaun selbst muss weiterhin opak gezeichnet werden");
    assert!(!translucent.is_empty(), "waterlogged muss eine Wasserfuellung in den Translucent-Pass liefern");
}

// Regressionstest: own_faces hat bisher nur
// geprueft, ob IRGENDEINE Box die Flaeche voll abdeckt -- nicht, ob das AN DER
// ZELLGRENZE passiert. Die untere Haelfte einer Stufe/Slab deklariert eine volle
// "up"-Flaeche bei y=0.5 (ihre eigene, sichtbare Oberseite) -- das hat faelschlich die
// Wasserfuellung an der eigentlichen Zellkante y=1 unterdrueckt, obwohl dort (0.5..1)
// gar keine Blockgeometrie liegt und man dort auf die Wasseroberflaeche schauen muss
// (z.B. eine waterlogged untere Stufe/Slab unter Wasser).
#[test]
fn waterlogged_untere_stufe_zeigt_wasser_darueber() {
    let slab = state_of("oak_slab", |s| {
        crate::state_props::property(s, "waterlogged") == Some("true") && crate::state_props::property(s, "type") == Some("bottom")
    });
    // Untere Haelfte: volle X/Z-Abdeckung, aber nur y 0..0.5 -- deklariert eine volle
    // "up"-Flaeche bei y=0.5 (NICHT an der Zellgrenze y=1).
    let bottom_half = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
    let mut per_state = vec![None; state_count()];
    per_state[slab as usize] = Some(vec![bottom_half]);
    let shapes = BlockShapes::new(per_state);

    let world = single_block_world(slab);
    let (opaque, translucent) = mesh_column(
        &world,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert!(!opaque.is_empty(), "die Slab selbst muss weiterhin opak gezeichnet werden");
    // Die Wasserfuellung muss GENAU 5 Seiten zeigen: alle bis auf "unten" (dort deckt
    // die Slab selbst an der echten Zellgrenze y=0 voll ab, s. own_faces[DOWN]) -- vor
    // dem Fix waeren es nur 4 gewesen (auch "oben" faelschlich unterdrueckt).
    assert_eq!(translucent.len(), wasser_vertices(5, 5), "Wasserfuellung muss oben sichtbar sein, nur unten fehlen");
}

// Regressionstest: da wo das Wasser das Farmland beruehrt, flimmerte eine fremde
// Textur. Farmland ist nur 15/16 hoch, deckt damit die
// 8/9 hohe Wasseroberflaeche aber vollstaendig ab. Vanilla verwirft die Wasser-Seiten-
// flaeche deshalb (FluidRenderer: Abdeckungstest gegen die WASSERHOEHE, s.
// water_side_box-Kommentar). Vorher lief der Test gegen einen vollen Wuerfel, die Flaeche
// wurde gezeichnet und lag koplanar auf der Erdseite des Farmlands -> Z-Fighting.
//
// Genau 5 Quads: oben, unten, und die drei Seiten an Luft -- die vierte (zum Farmland)
// muss fehlen. Vor dem Fix waren es 6.
#[test]
fn wasser_neben_farmland_zeigt_keine_flaeche_zum_farmland() {
    let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
    let farmland = state_of("farmland", |_| true);

    // Farmlands echte Vanilla-Box: volle Grundflaeche, 15/16 hoch.
    let column = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 15.0 / 16.0, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
    let mut per_state = vec![None; state_count()];
    per_state[farmland as usize] = Some(vec![column]);
    let shapes = BlockShapes::new(per_state);

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = water; // Welt (0,0,0)
            states[1] = farmland; // Welt (1,0,0), also oestlich daneben
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);

    let (_opaque, translucent) = mesh_column(
        &w,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert_eq!(
        translucent.len(),
        wasser_vertices(5, 4),
        "Wasser darf keine Flaeche zum (nur 15/16 hohen, aber voll abdeckenden) Farmland zeichnen"
    );
}

// Gegenprobe zum Test darueber: deckt der Nachbar die Wasserhoehe NICHT ab (halbhohe
// Box, 0.5 < 8/9), muss die Seitenflaeche sehr wohl gezeichnet werden -- sonst haette
// der Fix schlicht jede Seitenflaeche unterdrueckt statt nur die verdeckten.
#[test]
fn wasser_neben_halbhohem_block_zeigt_seine_flaeche() {
    let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
    let slab = state_of("oak_slab", |s| {
        crate::state_props::property(s, "waterlogged") == Some("false") && crate::state_props::property(s, "type") == Some("bottom")
    });

    let bottom_half = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
    let mut per_state = vec![None; state_count()];
    per_state[slab as usize] = Some(vec![bottom_half]);
    let shapes = BlockShapes::new(per_state);

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = water;
            states[1] = slab;
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);

    let (_opaque, translucent) = mesh_column(
        &w,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert_eq!(translucent.len(), wasser_vertices(6, 5), "ueber der halbhohen Stufe muss die Wasserflaeche sichtbar bleiben");
}

#[test]
fn nicht_waterlogged_zaun_hat_keine_wasserfuellung() {
    let fence = state_of("oak_fence", |s| {
        crate::state_props::property(s, "waterlogged") == Some("false")
            && crate::state_props::property(s, "north") == Some("false")
            && crate::state_props::property(s, "south") == Some("false")
            && crate::state_props::property(s, "east") == Some("false")
            && crate::state_props::property(s, "west") == Some("false")
    });
    let world = single_block_world(fence);
    let (_opaque, translucent) = mesh_column(
        &world,
        0,
        0,
        &BlockUvs::single_tile(),
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    assert!(translucent.is_empty(), "ohne waterlogged darf keine Wasserfuellung entstehen");
}

// Regressionstest: Bloecke unter Kakteen fehlten im Mesh. Kaktus' Render-Modell hat ein volles 0..16-
// Element mit cullface up/down (s. never_occludes_neighbors-Kommentar) -- das darf NUR
// Kaktus' EIGENE Ober-/Unterseite gegen einen sturen Nachbarn culln, nicht umgekehrt
// den Nachbarn gegen Kaktus. Sand bei (0,0,0), Kaktus direkt darueber bei (0,1,0), mit
// der echten (an den Seiten eingerueckten) Kaktus-Form aus drei Boxen wie im echten
// Der Fehler nach dem Bett-Renderer (man konnte durch den Boden gucken):
// Betten haben KEIN Blockmodell, shapes_of liefert fuer sie den Vollwuerfel-Notnagel --
// und mit dem hat der Boden unter dem Bett seine Oberseite verloren, waehrend das Bett
// selbst nur eine flache Matratze zeichnet. Das Loch ging quer durch die Welt.
#[test]
fn boden_unter_einem_bett_behaelt_seine_oberseite() {
    let stein = state_of("stone", |_| true);
    let bett = state_of("red_bed", |_| true);
    let vollwuerfel = ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] };
    // Genau die Lage aus dem echten Spiel: Stein hat ein Modell, das Bett nicht.
    let mut per_state = vec![None; state_count()];
    per_state[stein as usize] = Some(vec![vollwuerfel]);
    let shapes = BlockShapes::new(per_state);

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = stein; // (0,0,0)
            states[256] = bett; // (0,1,0)
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);
    let (opaque, _) = mesh_column(
        &w,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );

    // Die Oberseite des Steins liegt bei y = 1.0 (lokal), Sektion 4 beginnt bei y = 0.
    let basis_y = (crate::chunk::MIN_Y + 4 * 16) as f32;
    let oberseiten_quads = opaque.chunks_exact(6).filter(|q| q.iter().all(|v| (v.position[1] - (basis_y + 1.0)).abs() < 0.01)).count();
    assert!(oberseiten_quads > 0, "der Boden unter dem Bett muss seine Oberseite behalten");
}

// cactus.json (aus dem 1.21.11-Jar).
#[test]
fn sand_unter_kaktus_behaelt_seine_oberseite() {
    let sand = state_of("sand", |_| true);
    let cactus = state_of("cactus", |_| true);

    let full_updown = ShapeBox {
        min: [0.0, 0.0, 0.0],
        max: [1.0, 1.0, 1.0],
        faces: [true, true, false, false, false, false],
        content: [false; 6],
        uv: [None; 6],
    };
    let north_south_wall = ShapeBox {
        min: [0.0, 0.0, 1.0 / 16.0],
        max: [1.0, 1.0, 15.0 / 16.0],
        faces: [false, false, true, true, false, false],
        content: [false; 6],
        uv: [None; 6],
    };
    let west_east_wall = ShapeBox {
        min: [1.0 / 16.0, 0.0, 0.0],
        max: [15.0 / 16.0, 1.0, 1.0],
        faces: [false, false, false, false, true, true],
        content: [false; 6],
        uv: [None; 6],
    };
    let mut per_state = vec![None; state_count()];
    per_state[cactus as usize] = Some(vec![full_updown, north_south_wall, west_east_wall]);
    // Sand braucht seinen (trivialen) Vollwuerfel-Eintrag: seit BlockShapes zwischen
    // "Modell sagt Vollwuerfel" und "gar kein Modell" unterscheidet (s.
    // has_resolved_shape), verdeckt ein EINTRAGSLOSER Nachbar nichts mehr -- und Sand hat
    // in Wirklichkeit sehr wohl ein Modell.
    per_state[sand as usize] =
        Some(vec![ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
    let shapes = BlockShapes::new(per_state);

    let mut w = World::new();
    let mut sections = Vec::new();
    for i in 0..24 {
        let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
        if i == 4 {
            states[0] = sand; // lokal (0,0,0)
            states[256] = cactus; // lokal (0,1,0) -- (1*16+0)*16+0
        }
        sections.push(crate::chunk::ChunkSection::from_states(states));
    }
    w.insert_column(0, 0, sections);

    let (opaque, _translucent) = mesh_column(
        &w,
        0,
        0,
        &BlockUvs::single_tile(),
        &shapes,
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
    );
    // Sand: alle 6 Seiten (36 Vertices) -- Kaktus deckt seine Oberseite trotz vollem
    // Cullface-Element NICHT ab. Kaktus: 5 der 6 Seiten (30 Vertices) -- seine EIGENE
    // Unterseite bleibt weiterhin gegen den sturen Sand-Nachbarn geculled (unveraendert
    // richtig, betrifft die andere Blickrichtung). Macht 66 zusammen -- ohne den Fix
    // waeren es nur 60 (Sand nur 5 Seiten, seine Oberseite faelschlich unsichtbar).
    assert_eq!(opaque.len(), 66, "Sand unter Kaktus muss seine Oberseite behalten");
}
