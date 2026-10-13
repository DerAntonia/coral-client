// Systematische Uebersicht ALLER echten Bloecke/Blockstates gegen die echte lokale
// 1.21.11-Installation, statt jeden Darstellungsfehler einzeln per Screenshot zu melden.
// #[ignore], weil eine Installation gebraucht wird -- manuell:
//   cargo test -p app --test block_shape_audit -- --ignored --nocapture
//
// Ein generischer "sieht falsch aus"-Test (z.B. Box-Seitenverhaeltnis vs. UV-Seitenverhaeltnis)
// wurde probiert und wieder verworfen: er produzierte >6000 Falschmeldungen, weil Vanilla
// selbst auf vielen duennen Flaechen (Tuerkanten, Kesselwaende, ...) die Textur ABSICHTLICH
// verzerrt zieht, und andere Elemente absichtlich leicht ausserhalb 0..1 hinausragen
// (Vergleichsschaltung, Beetroots, Blumentopf-Inhalt) -- ohne echten Vergleichsrenderer laesst
// sich das nicht von einem echten Bug unterscheiden.
//
// Was tatsaechlich OHNE Falschmeldungen aussagekraeftig ist: welche Bloecke ueberhaupt KEINE
// Form aufloesen (weder shapes_raw noch cross_raw) und deshalb auf den Vollwuerfel-Fallback
// zurueckfallen -- das ist ein eindeutiges, falschmeldungsfreies Signal (kein Vollwuerfel ist
// abhaengig von Interpretation), jeder Treffer hier ist ein Block, dessen Kollisions-/Render-
// Form mit hoher Wahrscheinlichkeit falsch ist (Vollwuerfel statt echter Form).
use assets::{build_cross, build_shapes, MinecraftInstallation};
use std::collections::HashSet;

#[test]
#[ignore]
fn vollwuerfel_fallback_liste() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");

    let state_count = world::block_registry::state_count();
    let props_storage: Vec<Vec<(&str, &str)>> = (0..state_count as u32).map(|s| world::state_props::properties_of(s).collect()).collect();
    let state_entries: Vec<(u32, &str, &[(&str, &str)])> = (0..state_count as u32)
        .filter_map(|s| {
            let name = world::block_name(s)?;
            Some((s, name, props_storage[s as usize].as_slice()))
        })
        .collect();

    let shapes = build_shapes(&mut mc, &state_entries);
    let cross = build_cross(&mut mc, &state_entries);

    // Namen, die bekanntermassen KEIN Modell mit "elements" haben (Luft, Fluessigkeiten,
    // Barriere/Struktur/Licht-Marker) -- fuer die IST der Vollwuerfel-/kein-Element-Fallback
    // korrekt bzw. wird an anderer Stelle (world::mesher, Fluid-Rendering) speziell behandelt,
    // hier also keine echte Anomalie.
    let bekannte_ausnahmen: HashSet<&str> =
        ["air", "cave_air", "void_air", "water", "lava", "barrier", "light", "structure_void", "moving_piston", "bubble_column"]
            .into_iter()
            .collect();

    let mut fallback_namen: Vec<&str> = Vec::new();
    for (state, name, _) in &state_entries {
        if cross.contains(state) {
            continue; // Kreuz-Bloecke: bewusst kein Box-Fallback, eigener Zeichenpfad
        }
        if bekannte_ausnahmen.contains(name) {
            continue;
        }
        if !shapes.contains_key(state) && !fallback_namen.contains(name) {
            fallback_namen.push(name);
        }
    }
    fallback_namen.sort_unstable();

    println!("{} Blocknamen ohne aufgeloeste Form (Vollwuerfel-Fallback):", fallback_namen.len());
    for name in &fallback_namen {
        println!("  {name}");
    }
}

// Gegenprobe: Knoepfe in der Welt muessen sich druecken lassen.
// Knopf/Hebel/Druckplatte haben in Vanilla KEINE Kollisionsbox, aber sehr wohl einen
// UMRISS -- und nur gegen den Umriss zielt der Rechtsklick (physics::raycast_block ->
// world::outline_boxes). Hier wird gegen die ECHTE Installation geprueft, dass
//   1. die Form ueberhaupt aus den Modelldaten aufloest (sonst greift der bewusste
//      Notnagel in outline_boxes und der Block bliebe umrisslos),
//   2. sie deutlich KLEINER als ein Vollwuerfel ist (ein blockgrosser Knopf wuerde den
//      Strahl auf die Wand dahinter blockieren),
//   3. die Kollision trotzdem leer bleibt (man laeuft weiter durch),
//   4. die Blockstate-Rotation wirkt: ein WAND-Knopf sitzt an einer Seitenwand, nicht am
//      Boden. Ohne angewandte Rotation laege jede Variante an derselben Stelle.
#[test]
#[ignore]
fn knoepfe_hebel_druckplatten_haben_einen_umriss_aber_keine_kollision() {
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
    let cross = build_cross(&mut mc, &state_entries);
    let per_state: Vec<Option<Vec<world::ShapeBox>>> = (0..state_count as u32)
        .map(|s| {
            if cross.contains(&s) {
                return Some(Vec::new());
            }
            shapes_raw.get(&s).map(|boxes| {
                boxes.iter().map(|b| world::ShapeBox { min: b.min, max: b.max, faces: b.faces, content: b.content, uv: b.uv }).collect()
            })
        })
        .collect();
    let shapes = world::BlockShapes::new(per_state);

    let interessant = [
        "stone_button",
        "oak_button",
        "lever",
        "stone_pressure_plate",
        "heavy_weighted_pressure_plate",
        "repeater",
        "comparator",
        "tripwire_hook",
        "rail",
        "powered_rail",
        "redstone_wire",
    ];
    let mut geprueft = 0usize;
    let mut wand_knoepfe = 0usize;
    for (state, name, props) in &state_entries {
        if !interessant.contains(name) {
            continue;
        }
        let umriss = world::outline_boxes(*state, &shapes);
        assert!(!umriss.is_empty(), "{name} {props:?}: braucht einen Umriss, sonst zielt der Rechtsklick daran vorbei");
        assert!(
            world::collision_boxes(*state, &shapes).is_empty(),
            "{name} {props:?}: darf keine Kollisionsbox haben -- man laeuft in Vanilla hindurch"
        );
        for b in &umriss {
            let volumen: f32 = (0..3).map(|a| b.max[a] - b.min[a]).product();
            // Groesster berechtigter Fall ist die STEIGENDE Schiene: 16x8x16, also genau 0.5.
            assert!(volumen <= 0.5, "{name} {props:?}: Umriss {b:?} ist fast ein Vollwuerfel -- Fallback statt echter Form?");
        }
        geprueft += 1;

        if *name == "stone_button" && props.iter().any(|&(k, v)| k == "face" && v == "wall") {
            let unten = umriss.iter().all(|b| b.min[1] <= 1e-6);
            assert!(!unten, "{name} {props:?}: Wandknopf darf nicht am Boden kleben -- Blockstate-Rotation nicht angewandt?");
            let an_einer_wand =
                umriss.iter().any(|b| b.min[0] <= 1e-6 || b.max[0] >= 1.0 - 1e-6 || b.min[2] <= 1e-6 || b.max[2] >= 1.0 - 1e-6);
            assert!(an_einer_wand, "{name} {props:?}: Wandknopf muss buendig an einer Seitenflaeche sitzen, Umriss {umriss:?}");
            wand_knoepfe += 1;
        }
    }
    assert!(geprueft > 0, "keiner der geprueften Bloecke im Register gefunden");
    assert!(wand_knoepfe > 0, "kein Wand-Knopf-State gefunden -- Property-Namen geaendert?");
    println!("{geprueft} States geprueft, davon {wand_knoepfe} Wand-Knoepfe");
}
