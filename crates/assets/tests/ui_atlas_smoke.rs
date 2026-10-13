// Baut den UI-Atlas (GUI-Sheets + Item-Sprites) gegen die echte lokale 1.21.11-
// Installation und prueft, dass die Item-Modelle sauber aufgeloest werden. #[ignore] wie
// die uebrigen Installations-Tests -- manuell:
// cargo test -p assets --test ui_atlas_smoke -- --ignored --nocapture
use assets::{BlockAtlas, ItemIcon, MinecraftInstallation, GUI_SHEETS};

fn item_names() -> Vec<String> {
    [
        "air",
        "stone",
        "grass_block",
        "diamond_sword",
        "apple",
        "oak_planks",
        "chest",
        "potion",
        "bow",
        "shulker_box",
        "oak_leaves",
        "short_grass",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[test]
#[ignore]
fn ui_atlas_gegen_echte_installation() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let block_names = ["stone", "grass_block", "oak_planks", "chest", "shulker_box", "oak_leaves"];
    let block_atlas = BlockAtlas::build(&mut mc, &block_names).expect("Block-Atlas fehlgeschlagen");
    let names = item_names();
    let (atlas, icons, definitions) = assets::build_ui_atlas(&mut mc, &names, &block_atlas);
    // Jeder Zweig der Bogen-Definition hat ein Icon (gespannt in drei Stufen).
    let bogen = definitions.get("bow").expect("items/bow.json");
    assert_eq!(bogen.leaves.len(), 4);
    for key in &bogen.icon_keys {
        assert!(icons.contains_key(key), "{key}");
    }

    println!("UI-Atlas {}x{} px", atlas.size, atlas.size);
    for (key, _) in GUI_SHEETS {
        let rect = atlas.rect(key).unwrap_or_else(|| panic!("GUI-Sheet {key} fehlt im Atlas"));
        println!("  {key}: {}x{} px", rect.width, rect.height);
    }
    // Die Effektsymbole gehoeren zur Tabelle im Protokoll-Crate und sind alle 18x18.
    assert_eq!(assets::MOB_EFFECT_SPRITES, protocol::packets::MOB_EFFECTS);
    for name in assets::MOB_EFFECT_SPRITES {
        let rect = atlas.rect(&format!("mob_effect/{name}")).unwrap_or_else(|| panic!("Effektsymbol {name} fehlt im Atlas"));
        assert_eq!((rect.width, rect.height), (18, 18), "{name}");
    }
    for name in &names {
        match icons.get(name) {
            Some(ItemIcon::Sprite { layers, .. }) => println!("  {name}: Sprite mit {} Lage(n)", layers.len()),
            Some(ItemIcon::Block { .. }) => println!("  {name}: Blockwuerfel"),
            Some(ItemIcon::Model { quads, .. }) => println!("  {name}: 3D-Modell mit {} Flaechen", quads.len()),
            None => println!("  {name}: KEIN Icon"),
        }
    }

    // Vanillas Hotbar-Sheet ist 182x22 -- wenn das nicht stimmt, stimmt der Pfad nicht.
    let hotbar = atlas.rect("hud/hotbar").expect("Hotbar-Sheet fehlt");
    assert_eq!((hotbar.width, hotbar.height), (182, 22));

    // Bloecke muessen als Wuerfel kommen (sonst waere die Isometrie nie sichtbar),
    // Werkzeuge/Nahrung als flache Sprites.
    assert!(matches!(icons.get("stone"), Some(ItemIcon::Block { .. })), "stone muss ein Blockwuerfel sein");
    assert!(matches!(icons.get("grass_block"), Some(ItemIcon::Block { .. })), "grass_block muss ein Blockwuerfel sein");
    assert!(matches!(icons.get("diamond_sword"), Some(ItemIcon::Sprite { .. })), "diamond_sword muss ein Sprite sein");
    assert!(matches!(icons.get("apple"), Some(ItemIcon::Sprite { .. })), "apple muss ein Sprite sein");
    // Traenke bestehen aus zwei Lagen (Fuellung + Flasche).
    if let Some(ItemIcon::Sprite { layers, .. }) = icons.get("potion") {
        assert!(layers.len() >= 2, "potion sollte mehrere Lagen haben, hat {}", layers.len());
    }
    // Luft hat nie ein Icon.
    assert!(!icons.contains_key("air"));

    // Gras muss GRUEN sein: grass_block_top.png ist ein Graustufenbild, erst die Toenung
    // aus dem Item-Modell (minecraft:grass, temperature 0.5 / downfall 1.0) macht daraus
    // die vertraute Farbe. Ohne sie war der Grasblock im Inventar grau.
    let Some(ItemIcon::Block { faces, overlay }) = icons.get("grass_block") else {
        panic!("grass_block muss ein Blockwuerfel sein");
    };
    let top = faces[assets::FACE_UP].tint;
    assert!(top[1] > top[0] && top[1] > top[2], "Grasblock-Deckel muss gruen getoent sein, ist {top:?}");
    // Die Seiten bleiben ungetoente Erde -- gruen wird dort erst die Overlay-Lage.
    let side = faces[assets::FACE_NORTH].tint;
    assert_eq!(side, [1.0, 1.0, 1.0], "Grasblock-Seite selbst ist Erde, also ungetoent");
    let side_overlay = overlay[assets::FACE_NORTH].expect("Grasblock-Seite braucht die getoente Overlay-Lage").tint;
    assert!(side_overlay[1] > side_overlay[0] && side_overlay[1] > side_overlay[2], "Seiten-Overlay muss gruen sein");

    // Stein hat gar keine Toenung -- pruefen, dass wir nicht pauschal faerben.
    let Some(ItemIcon::Block { faces, .. }) = icons.get("stone") else { panic!("stone fehlt") };
    assert_eq!(faces[assets::FACE_UP].tint, [1.0, 1.0, 1.0], "Stein darf keine Toenung bekommen");

    // Blaetter: ebenfalls Graustufentextur, aber KONSTANTE Toenung (minecraft:constant im
    // Item-Modell, nicht die Biom-Colormap) -- und anders als beim Grasblock auf ALLEN
    // Flaechen, weil das Modell dort ueberall tintindex 0 setzt.
    let Some(ItemIcon::Block { faces, .. }) = icons.get("oak_leaves") else { panic!("oak_leaves fehlt") };
    for index in [assets::FACE_UP, assets::FACE_NORTH, assets::FACE_EAST] {
        let tint = faces[index].tint;
        assert!(tint[1] > tint[0] && tint[1] > tint[2], "Eichenlaub-Flaeche {index} muss gruen sein, ist {tint:?}");
    }

    // Und derselbe Mechanismus bei einem flachen Sprite: Lage 0 traegt tintindex 0.
    let Some(ItemIcon::Sprite { layers, .. }) = icons.get("short_grass") else { panic!("short_grass fehlt") };
    let tint = layers[0].tint;
    assert!(tint[1] > tint[0] && tint[1] > tint[2], "Gras-Sprite muss gruen sein, ist {tint:?}");
}

// Der teure Fall: ALLE 1505 Items der echten Registry aufloesen. Zeigt, wie viele ohne
// Icon bleiben -- ein sprunghafter Anstieg heisst, dass sich das Item-Modellformat
// geaendert hat.
#[test]
#[ignore]
fn alle_items_bekommen_ein_icon() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let block_names = world::block_registry::all_block_names();
    let block_atlas = BlockAtlas::build(&mut mc, &block_names).expect("Block-Atlas fehlgeschlagen");
    let names = world::item_registry::all_item_names();
    let (atlas, icons, definitions) = assets::build_ui_atlas(&mut mc, names, &block_atlas);

    let missing: Vec<&str> = names.iter().map(|s| s.as_str()).filter(|n| *n != "air" && !icons.contains_key(*n)).collect();
    println!("UI-Atlas {}x{} px, {} von {} Items mit Icon", atlas.size, atlas.size, names.len() - 1 - missing.len(), names.len() - 1);
    let zweige: Vec<&String> = definitions.values().flat_map(|d| d.icon_keys.iter()).collect();
    let ohne: Vec<&&String> = zweige.iter().filter(|k| !icons.contains_key(k.as_str())).collect();
    println!("{} Definitionen, {} Zweige, ohne Icon ({}): {:?}", definitions.len(), zweige.len(), ohne.len(), &ohne[..ohne.len().min(40)]);
    if !missing.is_empty() {
        println!("ohne Icon ({}): {:?}", missing.len(), &missing[..missing.len().min(40)]);
    }
    assert!(
        missing.len() < names.len() / 10,
        "{} von {} Items ohne Icon -- Item-Modellformat vermutlich geaendert",
        missing.len(),
        names.len()
    );
}

// Der Streitkolben aus dem Pack "CPVP Aesthetic" (~/Downloads): je Verzauberung ein eigenes Modell.
#[test]
#[ignore]
fn pack_streitkolben_hat_icons_je_verzauberung() {
    let pack = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Downloads/CPVP Aesthetic");
    let mut mc = MinecraftInstallation::locate("1.21.11", Some(pack)).expect("Installation fehlt");
    let block_atlas = BlockAtlas::build(&mut mc, &["stone"]).expect("Block-Atlas fehlgeschlagen");
    let (_atlas, icons, definitions) = assets::build_ui_atlas(&mut mc, &["mace".to_string()], &block_atlas);
    let def = definitions.get("mace").expect("items/mace.json aus dem Pack");
    let modelle: Vec<&str> = def.leaves.iter().map(|l| l.model.as_str()).collect();
    assert_eq!(modelle, ["item/mace_breach", "item/mace_density", "item/mace"]);
    for key in &def.icon_keys {
        assert!(icons.contains_key(key), "{key} ohne Icon");
        println!("{key}: {:?}", matches!(icons[key], ItemIcon::Model { .. }));
    }
}

// Der Gegenstandsrahmen ist ein Blockmodell ohne Block: seine Flaechen muessen sich laden lassen.
#[test]
#[ignore]
fn rahmenmodell_hat_flaechen() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    for modell in ["block/item_frame", "block/item_frame_map", "block/glow_item_frame", "block/glow_item_frame_map"] {
        match assets::model_world_textures(&mut mc, modell) {
            Some(assets::ItemWorldTextures::Model(q)) => assert!(q.len() >= 6, "{modell}: {} Flaechen", q.len()),
            _ => panic!("{modell} nicht als Modell geladen"),
        }
    }
}
