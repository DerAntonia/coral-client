// Baut den Block-Atlas gegen die echte lokale 1.21.11-Installation. #[ignore], weil es
// eine vorhandene Installation braucht -- manuell: cargo test -p assets --ignored -- --nocapture
use assets::{BlockAtlas, MinecraftInstallation, FACE_NORTH, FACE_UP};

#[test]
#[ignore]
fn atlas_gegen_echte_installation() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let names = ["stone", "grass_block", "dirt", "oak_planks", "oak_log", "cobblestone", "sand", "glass"];
    let atlas = BlockAtlas::build(&mut mc, &names).expect("Atlas-Bau fehlgeschlagen");
    println!("Atlas {}x{} px, {} Bloecke aufgeloest", atlas.size, atlas.size, atlas.uvs.len());
    for n in names {
        match atlas.uvs.get(n) {
            Some(uv) => println!("  {n}: up={:?} north={:?}", uv[FACE_UP], uv[FACE_NORTH]),
            None => println!("  {n}: NICHT aufgeloest"),
        }
    }
    // grass_block muss oben (top) und Seite (north) UNTERSCHIEDLICHE Kacheln haben.
    let grass = atlas.uvs.get("grass_block").expect("grass_block nicht aufgeloest");
    assert_ne!(grass[FACE_UP], grass[FACE_NORTH], "Gras: oben und Seite muessen sich unterscheiden");
    // stone: alle sechs Flaechen gleich (cube_all).
    let stone = atlas.uvs.get("stone").expect("stone nicht aufgeloest");
    assert!(stone.iter().all(|f| *f == stone[0]), "Stein: alle Flaechen gleich");
}

// Kesselinhalt (Wasser/Lava) darf NICHT mehr in die Randtextur hineinkomponiert werden
// (frueherer Bug: benutzte denselben Compositing-Pfad wie grass_block, landete gruen
// getoent und an falscher Stelle in cauldron_top) -- er muss stattdessen als eigene,
// unveraenderte Atlas-Kachel ueber content_uvs erreichbar UND von der Randtextur
// unterscheidbar sein.
#[test]
#[ignore]
fn kesselinhalt_bekommt_eigene_kachel() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let names = ["cauldron", "water_cauldron", "lava_cauldron", "powder_snow_cauldron", "cobweb"];
    let atlas = BlockAtlas::build(&mut mc, &names).expect("Atlas-Bau fehlgeschlagen");

    let rim_top = atlas.uvs.get("water_cauldron").expect("water_cauldron nicht aufgeloest")[FACE_UP];
    let water_content = atlas.content_uvs.get("water_cauldron").and_then(|c| c[FACE_UP]).expect("water_cauldron hat keine Inhalts-UV");
    let lava_content = atlas.content_uvs.get("lava_cauldron").and_then(|c| c[FACE_UP]).expect("lava_cauldron hat keine Inhalts-UV");
    assert_ne!(water_content, rim_top, "Inhalt darf nicht dieselbe Kachel wie der Kesselrand sein");
    assert_ne!(water_content, lava_content, "Wasser- und Lava-Inhalt muessen verschiedene Kacheln sein");

    // Leerer Kessel und Kreuz-Bloecke (Cobweb) haben KEINEN Inhalt.
    assert!(atlas.content_uvs.get("cauldron").is_none_or(|c| c.iter().all(|f| f.is_none())));
    assert!(atlas.content_uvs.get("cobweb").is_none_or(|c| c.iter().all(|f| f.is_none())));
}

// Auf Nutzerwunsch: Laub soll nicht mehr durchsichtig sein (Performance -- Alpha-Test/
// discard() im Fragment-Shader entfaellt, s. block.wgsl-Kommentar). oak_leaves hat in der
// echten Vanilla-Textur garantiert transparente Randpixel (das "Fancy"-Durchsehen) -- nach
// force_opaque() muss JEDES Pixel der Atlas-Kachel voll deckend sein.
#[test]
#[ignore]
fn laub_ist_nicht_mehr_transparent() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let names = ["oak_leaves", "spruce_leaves", "azalea_leaves", "stone"];
    let atlas = BlockAtlas::build(&mut mc, &names).expect("Atlas-Bau fehlgeschlagen");

    for name in ["oak_leaves", "spruce_leaves", "azalea_leaves"] {
        let uv = atlas.uvs.get(name).unwrap_or_else(|| panic!("{name} nicht aufgeloest"))[FACE_UP];
        let px0 = (uv[0] * atlas.size as f32).round() as u32;
        let py0 = (uv[1] * atlas.size as f32).round() as u32;
        let mut min_alpha = 255u8;
        for y in 0..16u32 {
            for x in 0..16u32 {
                let idx = (((py0 + y) * atlas.size + (px0 + x)) * 4 + 3) as usize;
                min_alpha = min_alpha.min(atlas.mips[0][idx]);
            }
        }
        assert_eq!(min_alpha, 255, "{name}: mindestens ein Pixel ist noch nicht voll deckend");
    }
}

// Die zehn Riss-Stufen (destroy_stage_0..9) muessen IMMER im Atlas landen, egal welche
// Bloecke gefragt wurden -- abbauen kann man jeden Block. Jede Stufe braucht ihre EIGENE
// Kachel, sonst saehe die Animation auf allen Stufen gleich aus.
#[test]
#[ignore]
fn riss_stufen_liegen_im_atlas() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let atlas = BlockAtlas::build(&mut mc, &["stone"]).expect("Atlas-Bau fehlgeschlagen");
    for (stufe, uv) in atlas.destroy_stage_uvs.iter().enumerate() {
        let uv = uv.unwrap_or_else(|| panic!("destroy_stage_{stufe} fehlt im Atlas"));
        println!("  destroy_stage_{stufe}: {uv:?}");
    }
    let mut gesehen = std::collections::HashSet::new();
    for uv in atlas.destroy_stage_uvs.iter().flatten() {
        assert!(gesehen.insert(format!("{uv:?}")), "jede Stufe braucht eine eigene Kachel");
    }
}

// Animierte Blocktexturen: jede mit .mcmeta muss als TileAnimation herauskommen -- Wasser nicht,
// das hat seinen eigenen Weg (water_animation_frames). Stein ist nicht animiert.
#[test]
#[ignore]
fn animierte_texturen_werden_gefunden() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let names = ["stone", "lava", "magma_block", "sea_lantern", "prismarine", "nether_portal", "water"];
    let atlas = BlockAtlas::build(&mut mc, &names).expect("Atlas-Bau fehlgeschlagen");
    println!("{} animierte Kacheln", atlas.animations.len());
    // Lava einfach (ihr Muster laeuft ueber Blockgrenzen), die vier anderen je ANIM_VARIANTS mal.
    let kopien_je_textur = assets::ANIM_VARIANTS as usize;
    assert_eq!(atlas.animations.len(), 2 + 4 * kopien_je_textur, "lava_still, lava_flow + magma, sea_lantern, prismarine, portal");
    assert_eq!(atlas.animations.iter().filter(|a| a.path.starts_with("block/lava")).count(), 2);
    assert_eq!(atlas.anim_variant_tiles.len(), 4);
    for (_, kopien) in &atlas.anim_variant_tiles {
        assert_eq!(kopien.len(), kopien_je_textur - 1);
    }
    // Die Kopien einer Textur laufen versetzt: nicht alle zeigen im selben Tick dasselbe Bild.
    for path in ["block/magma", "block/sea_lantern", "block/prismarine", "block/nether_portal"] {
        let bilder: std::collections::HashSet<_> =
            atlas.animations.iter().filter(|a| a.path == path).map(|a| format!("{:?}", a.state_at(0))).collect();
        assert!(bilder.len() > 1, "{path}: alle Kopien im Gleichschritt");
        let kopien: Vec<_> = atlas.animations.iter().filter(|a| a.path == path).collect();
        nicht_nur_verschoben(path, &kopien);
    }
    for animation in &atlas.animations {
        let mips = animation.render(animation.state_at(7));
        assert_eq!(mips[0].len(), (animation.tile_size * animation.tile_size * 4) as usize);
    }
}

#[test]
#[ignore]
fn animation_zeitmessung() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let names = world::block_registry::all_block_names();
    let atlas = BlockAtlas::build(&mut mc, &names).expect("Atlas-Bau fehlgeschlagen");
    println!("{} animierte Kacheln, Atlas-Kachel {}, {} Kacheln je Zeile", atlas.animations.len(), atlas.tile_size, atlas.tiles_per_row);
    // Im vollen Atlas bekommen auch Wasserpflanzen ihre Kopien (sonst wiegt Seegras im Gleichschritt).
    for path in ["block/seagrass", "block/tall_seagrass_top", "block/kelp", "block/kelp_plant"] {
        let n = atlas.animations.iter().filter(|a| a.path == path).count();
        assert_eq!(n, assets::ANIM_VARIANTS as usize, "{path}");
    }
    for animation in &atlas.animations {
        let t = std::time::Instant::now();
        for tick in 0..20 {
            let _ = animation.render(animation.state_at(tick));
        }
        println!("tile {} -> {:.2} ms pro render", animation.tile_size, t.elapsed().as_secs_f64() * 1000.0 / 20.0);
    }
}

// Feuer: je Textur mehrere Kacheln mit Zeitversatz, beide Texturen eines Paars in einem Topf,
// und die Kopien zeigen im selben Moment verschiedene Bilder (Feuer wechselt jeden Tick --
// ohne Versatz liefen alle Feuerbloecke im Gleichschritt).
#[test]
#[ignore]
fn feuer_hat_versetzte_kopien() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let atlas = BlockAtlas::build(&mut mc, &["fire", "soul_fire", "stone"]).expect("Atlas-Bau fehlgeschlagen");
    for path in ["block/fire_0", "block/fire_1", "block/soul_fire_0", "block/soul_fire_1"] {
        assert_eq!(atlas.texture_variants.get(path).map(|p| p.len()), Some(8), "{path}");
    }
    assert_eq!(atlas.animations.len(), 16, "4 Texturen x 4 Kopien");
    // Je Textur starten die vier Kopien mit vier verschiedenen Bildern und laufen verschieden schnell.
    for path in ["block/fire_0", "block/fire_1", "block/soul_fire_0", "block/soul_fire_1"] {
        let kopien: Vec<_> = atlas.animations.iter().filter(|a| a.path == path).collect();
        assert_eq!(kopien.len(), 4, "{path}");
        let staende: std::collections::HashSet<_> = kopien.iter().map(|a| a.state_at(0)).collect();
        assert_eq!(staende.len(), 4, "{path}");
        nicht_nur_verschoben(path, &kopien);
    }
}

// Kopien mit eigenem Tempo: keine ist bloss eine zeitverschobene Fassung einer anderen (dann
// liefen beide im selben Takt mit festem Abstand).
fn nicht_nur_verschoben(path: &str, kopien: &[&assets::TileAnimation]) {
    for (i, a) in kopien.iter().enumerate() {
        for b in &kopien[i + 1..] {
            let verschoben = (0..4000u64).find(|&d| (0..4000u64).step_by(7).all(|t| a.state_at(t + d) == b.state_at(t)));
            assert_eq!(verschoben, None, "{path}: zwei Kopien laufen im selben Takt");
        }
    }
}
