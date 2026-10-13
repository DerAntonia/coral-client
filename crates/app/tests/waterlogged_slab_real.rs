// Meshed eine waterlogged Stufe gegen die ECHTE lokale 1.21.11-Installation -- echte
// Block-Formen (build_shapes) UND echte Atlas-UVs (BlockAtlas::build), nicht die
// synthetischen Ersatzformen der Unit-Tests im world-Crate.
//
// Grund: die Unit-Tests belegten, dass der Mesher die vier Wasser-Waende erzeugt, im Spiel
// waren sie trotzdem unsichtbar. Genau die zwei Dinge, die dort ersetzt sind (Form und UV),
// blieben damit ungeprueft.
//
// #[ignore], weil eine Installation gebraucht wird -- manuell:
//   cargo test -p app --test waterlogged_slab_real -- --ignored --nocapture
#![allow(clippy::type_complexity)] // wie im Schwestertest block_shape_audit.rs: die state_entries-Signatur kommt so aus assets::build_shapes

use assets::{build_cross, build_shapes, BlockAtlas, MinecraftInstallation};

#[test]
#[ignore]
fn waterlogged_stufe_mit_echten_assets() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");

    let state_count = world::block_registry::state_count();
    let props_storage: Vec<Vec<(&str, &str)>> = (0..state_count as u32).map(|s| world::state_props::properties_of(s).collect()).collect();
    let state_entries: Vec<(u32, &str, &[(&str, &str)])> = (0..state_count as u32)
        .filter_map(|s| {
            let name = world::block_name(s)?;
            Some((s, name, props_storage[s as usize].as_slice()))
        })
        .collect();

    let atlas = BlockAtlas::build(&mut mc, &world::block_registry::all_block_names()).expect("Atlas");
    println!("water_uv        = {:?}", atlas.uvs.get("water"));
    println!("water_flow_uv   = {:?}", atlas.water_flow_uv);
    println!("water_flow_frames = {}", atlas.water_flow_animation_frames.len());
    println!("water_overlay_uv= {:?}", atlas.water_overlay_uv);

    let shapes_raw = build_shapes(&mut mc, &state_entries);
    let cross_raw = build_cross(&mut mc, &state_entries);
    let shapes_per_state: Vec<Option<Vec<world::ShapeBox>>> = (0..state_count as u32)
        .map(|s| {
            if cross_raw.contains(&s) {
                return Some(Vec::new());
            }
            shapes_raw.get(&s).map(|boxes| {
                boxes.iter().map(|b| world::ShapeBox { min: b.min, max: b.max, faces: b.faces, content: b.content, uv: b.uv }).collect()
            })
        })
        .collect();
    let shapes = world::BlockShapes::new(shapes_per_state);
    let cross = world::CrossBlocks::new((0..state_count as u32).map(|s| cross_raw.contains(&s)).collect());

    let state_von = |block: &str, passt: &dyn Fn(u32) -> bool| {
        (0..state_count as u32)
            .find(|&s| world::block_name(s) == Some(block) && passt(s))
            .unwrap_or_else(|| panic!("{block} nicht gefunden"))
    };
    let slab = state_von("oak_slab", &|s| {
        world::state_props::property(s, "waterlogged") == Some("true") && world::state_props::property(s, "type") == Some("bottom")
    });
    let gras = state_von("grass_block", &|_| true);

    println!("oak_slab(waterlogged,bottom) Boxen: {:?}", shapes.shapes_of(slab));

    let water_uv = atlas.uvs.get("water").copied().unwrap_or(atlas.missing);
    let uvs = world::BlockUvs::with_content_and_overlay(
        // Die Block-UVs selbst spielen hier keine Rolle (geprueft wird das WASSER) -- die
        // Wasser-Rechtecke unten sind die echten aus dem Atlas.
        (0..state_count as u32).map(|_| atlas.missing).collect(),
        (0..state_count as u32).map(|_| [None; 6]).collect(),
        (0..state_count as u32).map(|_| [None; 6]).collect(),
        (0..state_count as u32).map(|_| Vec::new()).collect(),
        water_uv,
        atlas.missing,
    )
    .with_water_flow(atlas.water_flow_uv)
    .with_water_overlay(atlas.water_overlay_uv)
    .with_lava_flow(atlas.lava_flow_uv);

    let trockene_stufe = state_von("oak_slab", &|s| {
        world::state_props::property(s, "waterlogged") == Some("false") && world::state_props::property(s, "type") == Some("bottom")
    });

    // Nachbar wahlweise Luft, trockene Stufe oder Grasblock -- genau die Faelle, die die alte
    // grobe "Nachbar ist nicht Luft"-Regel unterschiedslos verschluckt hat.
    let szene = |nachbar: u32| {
        let mut w = world::World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; world::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = gras; // (0,0,0)
                states[1] = gras;
                states[16 * 16] = slab; // (0,1,0)
                states[16 * 16 + 1] = nachbar; // (1,1,0), oestlich daneben
            }
            sections.push(world::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        w
    };
    for (bezeichnung, nachbar) in [("Luft", 0), ("trockene Stufe", trockene_stufe), ("Grasblock", gras)] {
        let (_o, tr) = world::mesh_column(
            &szene(nachbar),
            0,
            0,
            &uvs,
            &shapes,
            &world::BiomePalette::fallback(),
            &cross,
            &world::BlockShapes::all_full_cubes(),
        );
        // Nur die Waende der Stufe bei x=0 zaehlen (die Ost-Wand liegt bei x~0.999).
        let ostwand = tr.chunks_exact(6).filter(|q| q.iter().all(|v| (v.position[0] - 0.999).abs() < 1e-3)).count();
        println!("Nachbar {bezeichnung}: {} Ost-Wand-Quads, {} Vertices gesamt", ostwand, tr.len());
    }

    let w = szene(0);

    let (opaque, translucent) =
        world::mesh_column(&w, 0, 0, &uvs, &shapes, &world::BiomePalette::fallback(), &cross, &world::BlockShapes::all_full_cubes());
    println!("opak {} Vertices, translucent {} Vertices", opaque.len(), translucent.len());
    for (i, q) in translucent.chunks_exact(6).enumerate() {
        let spanne = |achse: usize| {
            let min = q.iter().map(|v| v.position[achse]).fold(f32::MAX, f32::min);
            let max = q.iter().map(|v| v.position[achse]).fold(f32::MIN, f32::max);
            (min, max)
        };
        println!("Quad {i}: x={:?} y={:?} z={:?} uv={:?}..{:?}", spanne(0), spanne(1), spanne(2), q[0].uv, q[2].uv);
    }
}
