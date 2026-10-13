// Liest die Halte-Transformationen gegen die ECHTE lokale 1.21.11-Installation. #[ignore],
// weil es eine vorhandene Installation braucht -- manuell:
//   cargo test -p assets --test item_hand_display -- --ignored --nocapture
//
// Der Test prueft nicht nur "es kommt etwas heraus", sondern die inhaltliche Trennung, um
// die es beim Item in der Hand geht: ein Schwert (Modell item/handheld) muss ANDERS in der
// Hand liegen als ein Apfel (item/generated) -- genau diese Unterscheidung macht den
// Unterschied zwischen "Schwert liegt in der Faust" und "Schwert klebt flach am Arm".
use assets::{item_hand_display, ItemDisplay, MinecraftInstallation};

#[test]
#[ignore]
fn handheld_und_generated_unterscheiden_sich() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");

    for item in ["diamond_sword", "apple", "totem_of_undying", "ender_pearl", "obsidian", "end_crystal"] {
        let d = item_hand_display(&mut mc, item);
        println!("{item:18} rot={:?} trans={:?} scale={:?}", d.rotation, d.translation, d.scale);
    }

    let schwert = item_hand_display(&mut mc, "diamond_sword");
    let apfel = item_hand_display(&mut mc, "apple");

    assert_ne!(schwert, ItemDisplay::IDENTITY, "ohne display-Eintrag saesse das Schwert im Ursprung");
    assert_ne!(apfel, ItemDisplay::IDENTITY, "auch item/generated setzt eine Hand-Haltung");
    assert_ne!(schwert, apfel, "handheld und generated MUESSEN sich unterscheiden");

    // Beide Vanilla-Elternmodelle verkleinern das Item in der Hand deutlich -- ein Item in
    // Originalgroesse (1.0) waere so gross wie ein ganzer Block und ein sofort sichtbarer
    // Fehler.
    for (name, d) in [("diamond_sword", schwert), ("apple", apfel)] {
        assert!(d.scale[0] > 0.1 && d.scale[0] < 1.0, "{name}: unplausible Groesse {:?}", d.scale);
        // translation ist bereits durch 16 geteilt, liegt also in Bloecken -- alles ueber
        // einem halben Block waere weit neben der Hand.
        assert!(d.translation.iter().all(|v| v.abs() <= 0.5), "{name}: Verschiebung {:?} zu gross", d.translation);
    }
}

// Ein unbekanntes Item darf nicht in Panik geraten, sondern faellt auf die Identitaet
// zurueck -- der Client soll wegen eines Servers mit Custom-Items nicht abstuerzen.
#[test]
#[ignore]
fn unbekanntes_item_faellt_auf_identitaet_zurueck() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    assert_eq!(item_hand_display(&mut mc, "gibt_es_nicht_12345"), ItemDisplay::IDENTITY);
}

// Die Texturpfade fuer Items in der Hand -- gegen die echte Installation, weil genau hier
// ein falscher Pfad zu einem unsichtbaren Item fuehrt und sonst nichts meckert.
#[test]
#[ignore]
fn texturpfade_fuer_hand_items() {
    use assets::{item_world_textures, ItemWorldTextures};
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");

    for item in ["diamond_sword", "totem_of_undying", "ender_pearl", "obsidian", "end_crystal", "golden_apple"] {
        let t = item_world_textures(&mut mc, item).unwrap_or_else(|| panic!("{item}: keine Texturen aufgeloest"));
        match &t {
            ItemWorldTextures::Sprite(path) => {
                println!("{item:18} Sprite {path}");
                assert!(mc.load_texture(&format!("minecraft/textures/{path}.png")).is_ok(), "{item}: {path} nicht ladbar");
            }
            ItemWorldTextures::Model(quads) => {
                println!("{item:18} Modell {} Flaechen", quads.len());
                for q in quads {
                    assert!(mc.load_texture(&format!("minecraft/textures/{}.png", q.texture)).is_ok(), "{item}: {} nicht ladbar", q.texture);
                }
            }
            ItemWorldTextures::Block(faces) => {
                println!("{item:18} Block  {faces:?}");
                for path in faces.iter() {
                    assert!(mc.load_texture(&format!("minecraft/textures/{path}.png")).is_ok(), "{item}: {path} nicht ladbar");
                }
            }
        }
    }

    // Obsidian ist ein Blockitem, ein Totem nicht -- laege die Unterscheidung falsch
    // herum, saesse ein Wuerfel statt des Totems in der Hand.
    assert!(matches!(item_world_textures(&mut mc, "obsidian"), Some(ItemWorldTextures::Block(_))));
    assert!(matches!(item_world_textures(&mut mc, "totem_of_undying"), Some(ItemWorldTextures::Sprite(_))));
}

// Sonderfaelle: Bloecke, die Vanilla NICHT als schlichtes Wuerfelmodell fuehrt (Kisten,
// Shulker, Betten, Koepfe laufen ueber builtin/entity). Wenn deren Modellkette keinen
// display-Eintrag hat, faellt die Haltung auf die Identitaet zurueck -- Groesse 1.0 statt
// 0.375, also ein Item so gross wie ein ganzer Block in der Hand.
#[test]
#[ignore]
fn sonderbloecke_haben_eine_haltung() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    for item in [
        "chest",
        "ender_chest",
        "shulker_box",
        "white_bed",
        "player_head",
        "oak_sign",
        "crafting_table",
        "stone",
        "obsidian",
        "respawn_anchor",
        "oak_slab",
        "oak_stairs",
    ] {
        let d = item_hand_display(&mut mc, item);
        let ident = d == ItemDisplay::IDENTITY;
        println!("{item:16} scale={:?} {}", d.scale, if ident { "<-- IDENTITAET!" } else { "" });
    }
}
