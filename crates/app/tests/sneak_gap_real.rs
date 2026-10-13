// Reproduziert die Stelle aus Grims consoledebug ("test sneak", 2026-08-05) mit den ECHTEN
// Blockformen der lokalen 1.21.11-Installation -- nicht mit den Vollwuerfel-Ersatzformen der
// Unit-Tests, denn genau die Form ist hier die Frage.
//
// Grims Blockdump an der geflaggten Stelle (Spieler bei x~7.47, y=-12, z~9.20):
//   y=-11, x=7, z=8 : oak_slab[type=top]          <- die "Decke" der Luecke
//   y=-12, x=6/8, z=8: dirt                        <- die Wangen daneben
//   y=-13           : Boden (grass_block/dirt)
// Die obere Stufe fuellt -10.5 .. -10.0, unter ihr bleiben also von -12 bis -10.5 GENAU 1.5
// Bloecke -- exakt die Schleich-Hitbox. Vanilla/Grim sagen darum "Schleich-Pose passt" und
// rechnen mit dem Sneak-Faktor 0.3; Grims Bounding box im Mitschnitt ist entsprechend
// minY=-12 / maxY=-10.5.
//
// Genau daran haengt bei uns die ganze Kette: passt die 1.5er-Box nicht, faellt resolve_pose
// in die Schwimm-Pose durch, in_sneaking_pose_move bleibt false und der ×0.3 entfaellt -- im
// Mitschnitt als "Client input 0.098 / Simulated input 0.0294" sichtbar, Offset 0.0686.
//
// #[ignore], weil eine Installation gebraucht wird -- manuell:
//   cargo test -p app --test sneak_gap_real -- --ignored --nocapture
#![allow(clippy::type_complexity)]

use assets::{build_cross, build_shapes, MinecraftInstallation};

fn state_of(name: &str, props: &[(&str, &str)]) -> u32 {
    let count = world::block_registry::state_count();
    (0..count as u32)
        .find(|&s| {
            world::block_name(s) == Some(name) && {
                let vorhanden: Vec<(String, String)> =
                    world::state_props::properties_of(s).map(|(k, v)| (k.to_string(), v.to_string())).collect();
                props.iter().all(|(k, v)| vorhanden.iter().any(|(pk, pv)| pk == k && pv == v))
            }
        })
        .unwrap_or_else(|| panic!("Blockstate {name}{props:?} nicht in der Registry"))
}

#[test]
#[ignore]
fn obere_stufe_laesst_die_schleich_box_frei() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");

    let state_count = world::block_registry::state_count();
    let props_storage: Vec<Vec<(&str, &str)>> = (0..state_count as u32).map(|s| world::state_props::properties_of(s).collect()).collect();
    let state_entries: Vec<(u32, &str, &[(&str, &str)])> = (0..state_count as u32)
        .filter_map(|s| {
            let name = world::block_name(s)?;
            Some((s, name, props_storage[s as usize].as_slice()))
        })
        .collect();

    let shapes_raw = build_shapes(&mut mc, &state_entries);
    let cross_raw = build_cross(&mut mc, &state_entries);
    let per_state: Vec<Option<Vec<world::ShapeBox>>> = (0..state_count as u32)
        .map(|s| {
            if cross_raw.contains(&s) {
                return Some(Vec::new());
            }
            shapes_raw.get(&s).map(|boxes| {
                boxes.iter().map(|b| world::ShapeBox { min: b.min, max: b.max, faces: b.faces, content: b.content, uv: b.uv }).collect()
            })
        })
        .collect();
    let shapes = world::BlockShapes::new(per_state);

    let slab = state_of("oak_slab", &[("type", "top")]);
    let boxes = world::collision_boxes(slab, &shapes);
    println!("oak_slab[type=top] Kollisionsboxen: {boxes:?}");

    assert_eq!(boxes.len(), 1, "eine Stufe hat GENAU eine Kollisionsbox, hier: {boxes:?}");
    let b = &boxes[0];
    // Die obere Haelfte: y von 0.5 bis 1.0, in x/z voll. Ein Vollwuerfel (0..1) waere der
    // Fallback aus BlockShapes::shapes_of -- und genau der wuerde die Schleich-Box sperren.
    assert!(
        (b.min[1] - 0.5).abs() < 1.0e-6 && (b.max[1] - 1.0).abs() < 1.0e-6,
        "obere Stufe muss y 0.5..1.0 belegen, ist aber {:?}..{:?} -- bei 0.0..1.0 ist es der Vollwuerfel-Fallback",
        b.min,
        b.max
    );
}
