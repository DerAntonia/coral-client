// Baut den Partikel-Atlas gegen die echte lokale 1.21.11-Installation. #[ignore] wie die
// uebrigen Installations-Tests -- manuell:
// cargo test -p assets --test particle_atlas_smoke -- --ignored --nocapture
use assets::MinecraftInstallation;

#[test]
#[ignore]
fn vanilla_partikel_landen_in_json_reihenfolge_im_atlas() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let start = std::time::Instant::now();
    let atlas = assets::build_particle_atlas(&mut mc, &protocol::particle::PARTICLE_NAMES, world::item_registry::all_item_names());
    println!("gebaut in {:?}", start.elapsed());
    let mit_sprites = protocol::particle::PARTICLE_NAMES.iter().filter(|n| !atlas.sprites(n).is_empty()).count();
    println!("Partikel-Atlas {}x{} px, {mit_sprites} von {} Typen mit Sprites", atlas.size, atlas.size, protocol::particle::PARTICLE_NAMES.len());

    // Anzahl je Typ aus particles/<name>.json.
    assert_eq!(atlas.sprites("crit").len(), 1);
    assert_eq!(atlas.sprites("explosion").len(), 16);
    assert_eq!(atlas.sprites("totem_of_undying").len(), 8);
    assert_eq!(atlas.sprites("poof").len(), 8);
    assert_eq!(atlas.sprites("sweep_attack").len(), 8);
    // Kein JSON: die sampeln den Block- bzw. Item-Atlas.
    assert!(atlas.sprites("block").is_empty());
    assert!(atlas.sprites("item").is_empty());
    // Totem zaehlt RUECKWAERTS (glitter_7 zuerst) -- dieselben Bilder wie firework, aber
    // andere Reihenfolge. Nach Position im Atlas pruefbar: verschiedene Rechtecke.
    assert_ne!(atlas.sprites("totem_of_undying")[0], atlas.sprites("totem_of_undying")[7]);
    // Item-Splitter: jedes Item mit Partikeltextur hat ein Bild.
    let items: std::collections::HashMap<&str, [f32; 4]> = atlas.item_sprites().collect();
    println!("{} von {} Items mit Splitterbild", items.len(), world::item_registry::all_item_names().len());
    for item in ["bread", "cooked_beef", "golden_apple", "diamond_sword", "splash_potion", "snowball", "egg", "stone", "shield"] {
        assert!(items.contains_key(item), "{item} ohne Splitterbild");
    }
    assert!(atlas.size <= 2048);
}

// Blockitems nehmen das "particle"-Feld des Blockmodells, flache Items ihr layer0, besondere
// Modelle (Schild) ihr eigenes "particle".
#[test]
#[ignore]
fn item_splitter_nehmen_die_particle_textur() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    assert_eq!(assets::item_particle_texture(&mut mc, "grass_block").as_deref(), Some("block/dirt"));
    assert_eq!(assets::item_particle_texture(&mut mc, "bread").as_deref(), Some("item/bread"));
    assert_eq!(assets::item_particle_texture(&mut mc, "shield").as_deref(), Some("block/dark_oak_planks"));
}

#[test]
#[ignore]
// Die Partikeltextur eines Blocks ist das Modellfeld "particle", nicht irgendeine Flaeche:
// der Grasblock staubt Erde, das Bett (Modell ohne Elemente) Eichenbretter.
fn blockstaub_nimmt_die_particle_textur_des_modells() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let atlas = assets::BlockAtlas::build(&mut mc, &["grass_block", "dirt", "red_bed", "oak_planks", "stone"]).expect("Atlas");
    assert_eq!(atlas.particle_uvs["grass_block"], atlas.uvs["dirt"][0]);
    assert_eq!(atlas.particle_uvs["red_bed"], atlas.uvs["oak_planks"][0]);
    assert_eq!(atlas.particle_uvs["stone"], atlas.uvs["stone"][0]);
}
