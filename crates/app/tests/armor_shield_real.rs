// Ruestungs- und Schildtexturen gegen die ECHTE lokale 1.21.11-Installation.
//
// Grund: die Modelle sind in world:: gegen synthetische Daten geprueft, aber ob die
// Texturpfade aus world::armor::all_textures() ueberhaupt Dateien treffen und im
// Entity-Atlas landen, bliebe damit ungeprueft -- und ein Tippfehler im Pfad faellt im Spiel
// nur als "Ruestung unsichtbar" auf, also genau als das Symptom, das behoben werden sollte.
//
// #[ignore], weil eine Installation gebraucht wird -- manuell:
//   cargo test -p app --test armor_shield_real -- --ignored --nocapture

use assets::MinecraftInstallation;

#[test]
#[ignore]
fn ruestung_und_schild_landen_im_entity_atlas() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");

    let mut texturen = world::armor::all_textures();
    texturen.push(world::entity_model::shield_model().texture);
    let atlas = assets::build_entity_atlas(&mut mc, &texturen);

    let mut fehlend = Vec::new();
    for name in &texturen {
        match atlas.rect(name) {
            Some(rect) => println!("{name:40} {rect:?}"),
            None => fehlend.push(*name),
        }
    }
    assert!(fehlend.is_empty(), "nicht im Atlas gelandet: {fehlend:?}");

    // Gegenprobe gegen eine stille Verwechslung: die Hosen-Ebene muss eine ANDERE Kachel
    // sein als die Rumpf-Ebene desselben Materials.
    let (_, eisen) = world::armor::armor_of("iron_chestplate").expect("Eisenpanzer");
    let humanoid = world::armor::layer_for(eisen, world::armor::ArmorSlot::Chest).expect("aeussere Ebene");
    let leggings = world::armor::layer_for(eisen, world::armor::ArmorSlot::Legs).expect("innere Ebene");
    assert_ne!(atlas.rect(humanoid.base), atlas.rect(leggings.base), "humanoid und humanoid_leggings muessen getrennte Kacheln sein");
}

// shield.json traegt keine Geometrie, aber sehr wohl die Halte-Transformationen -- die
// braucht emit_shield. Faellt sie auf die Einheitsmatrix zurueck, liegt das Schild mitten im
// Koerper.
#[test]
#[ignore]
fn shield_json_liefert_echte_haltetransformationen() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let rechts = assets::item_hand_display(&mut mc, "shield");
    let links = assets::item_hand_display_left(&mut mc, "shield");
    println!("thirdperson_righthand = {rechts:?}");
    println!("thirdperson_lefthand  = {links:?}");
    // Vanillas Werte, geteilt durch 16: [10,6,-4] bzw. [10,6,12].
    assert!((rechts.translation[0] - 10.0 / 16.0).abs() < 1e-4, "rechts x {}", rechts.translation[0]);
    assert!((rechts.translation[2] + 4.0 / 16.0).abs() < 1e-4, "rechts z {}", rechts.translation[2]);
    // DER Grund, warum die linke Hand einen eigenen Eintrag braucht statt der Spiegelung:
    // das z unterscheidet sich um eine ganze Blocklaenge.
    assert!((links.translation[2] - 12.0 / 16.0).abs() < 1e-4, "links z {}", links.translation[2]);
    assert!((rechts.rotation[1] - 90.0).abs() < 1e-4, "rechts yaw {}", rechts.rotation[1]);
}

// Das HOCHGERISSENE Schild ist ein eigenes Modell (item/shield_blocking), kein verschobenes
// Original -- items/shield.json waehlt per `minecraft:using_item`-Bedingung zwischen beiden.
// Ohne das wird das Schild nicht gehoben: geladen wurde nur
// das Ruhe-Modell, also blieb das Schild beim Blocken an derselben Stelle stehen.
#[test]
#[ignore]
fn shield_blocking_hat_eigene_haltetransformationen() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let ruhe = assets::item_hand_display(&mut mc, "shield");
    let block = assets::model_hand_display(&mut mc, "item/shield_blocking", "thirdperson_righthand");
    let block_ego = assets::model_hand_display(&mut mc, "item/shield_blocking", "firstperson_righthand");
    println!("ruhe  = {ruhe:?}");
    println!("block = {block:?}");
    println!("ego   = {block_ego:?}");
    // Vanillas shield_blocking.json, geteilt durch 16: [-3.49, 11, -2], Drehung [45, 155, 0].
    assert!((block.translation[0] + 3.49 / 16.0).abs() < 1e-4, "x {}", block.translation[0]);
    assert!((block.translation[1] - 11.0 / 16.0).abs() < 1e-4, "y {}", block.translation[1]);
    assert!((block.rotation[0] - 45.0).abs() < 1e-4, "pitch {}", block.rotation[0]);
    assert!((block.rotation[1] - 155.0).abs() < 1e-4, "yaw {}", block.rotation[1]);
    // Und die Gegenprobe: es ist wirklich ein ANDERER Eintrag als der im Ruhezustand.
    assert_ne!(ruhe.translation, block.translation, "Ruhe- und Blockhaltung duerfen nicht gleich sein");
    // Die Ego-Ansicht hat ebenfalls einen eigenen Eintrag ([-15,5,-11] statt [-10,2,-10]).
    assert!((block_ego.translation[0] + 15.0 / 16.0).abs() < 1e-4, "ego x {}", block_ego.translation[0]);
}
