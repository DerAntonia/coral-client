// Prueft den Resourcepack-Override-Pfad (s. MinecraftInstallation::resource_pack_dirs) UND
// die dynamische Atlas-Kachelgroesse (s. BlockAtlas::tile_size), beide gegen die echte
// lokale 1.21.11-Installation (fuer den Jar-Fallback-Vergleich/die restlichen Bloecke
// gebraucht). #[ignore], selbes Muster wie atlas_smoke.rs -- manuell:
// cargo test -p assets --test resource_pack --ignored -- --nocapture

use assets::{BlockAtlas, MinecraftInstallation};
use image::{Rgba, RgbaImage};
use std::path::PathBuf;

// Eigener, eindeutig benannter Ordner pro Test unter dem OS-Temp-Verzeichnis -- kein
// zusaetzlicher tempfile-Crate noetig, am Ende wird selbst aufgeraeumt.
fn temp_resource_pack_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("coral_client_test_resourcepack_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets/minecraft/textures/block")).expect("Testverzeichnis anlegen");
    dir
}

#[test]
#[ignore]
fn resource_pack_override_gewinnt_gegen_jar() {
    let dir = temp_resource_pack_dir("override");
    let override_path = dir.join("assets/minecraft/textures/block/stone.png");
    // 16x16, komplett rot -- die echte Vanilla-stone.png ist grau, ein eindeutiger Marker,
    // dass wirklich die Override-Datei und nicht doch die Jar-Version gelesen wurde.
    RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 255])).save(&override_path).expect("Test-PNG schreiben");

    let mut mc = MinecraftInstallation::locate("1.21.11", Some(dir.clone())).expect("Installation fehlt");
    let bytes = mc.load_bytes("minecraft/textures/block/stone.png").expect("Textur nicht ladbar");
    let expected = std::fs::read(&override_path).unwrap();
    assert_eq!(bytes, expected, "Override-Datei haette gewinnen muessen, nicht die Jar-Version");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore]
fn hd_textur_setzt_die_atlas_kachelgroesse() {
    let dir = temp_resource_pack_dir("hd");
    let override_path = dir.join("assets/minecraft/textures/block/stone.png");
    RgbaImage::from_pixel(32, 32, Rgba([10, 20, 30, 255])).save(&override_path).expect("Test-PNG schreiben");

    let mut mc = MinecraftInstallation::locate("1.21.11", Some(dir.clone())).expect("Installation fehlt");
    // dirt bleibt vanilla-16x16 (kein Override im Testordner) -- gemischter Fall: beide
    // muessen im selben, auf 32 hochskalierten Atlas landen statt eines Groessenkonflikts.
    let atlas = BlockAtlas::build(&mut mc, &["stone", "dirt"]).expect("Atlas-Bau fehlgeschlagen");
    assert_eq!(atlas.tile_size, 32, "die 32x32-Override-Textur haette die Atlas-Kachelgroesse setzen muessen");
    assert!(atlas.uvs.contains_key("dirt"), "dirt (vanilla 16x16, kein Override) muss trotzdem aufgeloest sein");
    let expected_mip0_len = (atlas.size * atlas.size * 4) as usize;
    assert_eq!(atlas.mips[0].len(), expected_mip0_len, "Level-0-Mip muss zur Atlasgroesse passen");

    let _ = std::fs::remove_dir_all(&dir);
}

// Gestapelte Packs: das obere gewinnt, wo beide eine Datei haben; was nur das untere
// mitbringt, kommt trotzdem aus dem unteren statt aus dem Jar.
#[test]
#[ignore]
fn gestapelte_packs_oberes_gewinnt_unteres_fuellt_auf() {
    let oben = temp_resource_pack_dir("stapel_oben");
    let unten = temp_resource_pack_dir("stapel_unten");
    let stein_oben = oben.join("assets/minecraft/textures/block/stone.png");
    let stein_unten = unten.join("assets/minecraft/textures/block/stone.png");
    let erde_unten = unten.join("assets/minecraft/textures/block/dirt.png");
    RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 255])).save(&stein_oben).unwrap();
    RgbaImage::from_pixel(16, 16, Rgba([0, 0, 255, 255])).save(&stein_unten).unwrap();
    RgbaImage::from_pixel(16, 16, Rgba([0, 255, 0, 255])).save(&erde_unten).unwrap();

    let mut mc = MinecraftInstallation::locate("1.21.11", vec![oben.clone(), unten.clone()]).expect("Installation fehlt");
    assert_eq!(mc.load_bytes("minecraft/textures/block/stone.png").unwrap(), std::fs::read(&stein_oben).unwrap());
    assert_eq!(mc.load_bytes("minecraft/textures/block/dirt.png").unwrap(), std::fs::read(&erde_unten).unwrap());
    // Alle Ebenen: Vanilla, dann das untere, dann das obere Pack.
    let ebenen = mc.load_layers("minecraft/textures/block/stone.png");
    assert_eq!(ebenen.len(), 3);
    assert_eq!(ebenen[1], std::fs::read(&stein_unten).unwrap());
    assert_eq!(ebenen[2], std::fs::read(&stein_oben).unwrap());

    let _ = std::fs::remove_dir_all(&oben);
    let _ = std::fs::remove_dir_all(&unten);
}

// Ein echtes Server-Pack (Zip, oft absichtlich verbogen) mit eigenen Schriften. Pfad ueber
// MC_TEST_PACK, z.B. ein Pack aus .minecraft/downloads.
#[test]
#[ignore]
fn server_pack_schriften() {
    let Ok(pfad) = std::env::var("MC_TEST_PACK") else { return };
    let mut mc = MinecraftInstallation::locate("1.21.11", Some(PathBuf::from(pfad))).expect("Installation fehlt");
    let start = std::time::Instant::now();
    let fonts = mc.load_pack_fonts();
    let mut ids: Vec<_> = fonts.fonts.iter().map(|(k, v)| (k.clone(), v.len())).collect();
    ids.sort();
    println!("{} Seiten, {:?}, {:?}", fonts.pages.len(), ids, start.elapsed());
    assert!(!fonts.fonts.is_empty());
}
