// Baut den Entity-Atlas gegen die echte lokale 1.21.11-Installation. #[ignore] wie die
// uebrigen Installations-Tests -- manuell:
// cargo test -p assets --test entity_atlas_smoke -- --ignored --nocapture
//
// Der Test prueft die eine Sache, die sich beim Umschreiben der Modelldaten still kaputt
// machen laesst: dass jeder Texturpfad in der Modelltabelle in der Installation wirklich
// existiert. Ein Tippfehler dort fuehrt im Spiel nicht zu einem Fehler, sondern zu einem
// unsichtbaren Mob (die Textur fehlt im Atlas, das Modell wird uebersprungen).
use assets::MinecraftInstallation;

#[test]
#[ignore]
fn alle_modelltexturen_landen_im_atlas() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let table = world::entity_model::model_table();
    let player_wide = world::player_model(false);
    let player_slim = world::player_model(true);

    let mut textures: Vec<&str> = table.iter().map(|(_, model, _)| model.texture).collect();
    textures.push(player_wide.texture);
    textures.push(player_slim.texture);
    // Die 18 Standard-Skins: ein Tippfehler hier trifft nicht einen Mob, sondern jeden
    // Spieler ohne Textur-Property -- und zwar nur den Teil der Spieler, dessen UUID-Hash
    // auf den kaputten Eintrag faellt. Im Spiel praktisch nicht zu finden, hier sofort.
    textures.extend(world::DEFAULT_SKINS.iter().map(|(texture, _)| *texture));

    // Der Rest der Liste, die die App wirklich packt (App::new): Betten, Ruestung, Schild,
    // Kristall, Pfeile und das animierte Feuer. Ohne sie zaehlte dieser Test Item-Plaetze,
    // die es im Spiel gar nicht gibt.
    textures.extend(world::BED_TEXTURES.iter().copied());
    textures.extend(world::armor::all_textures());
    textures.push("entity/shield_base_nopattern");
    textures.push(world::entity_model::END_CRYSTAL_TEXTURE);
    textures.push(world::entity_model::ARROW_TEXTURE);
    textures.push(world::entity_model::SPECTRAL_ARROW_TEXTURE);
    textures.push("block/fire_0");
    textures.push("block/fire_1");

    let atlas = assets::build_entity_atlas(&mut mc, &textures);
    // Das Feuer ist animiert: verschiedene Ticks zeigen verschiedene Bilder derselben Groesse.
    let (a, b) = (atlas.animated_rect("block/fire_1", 0).unwrap(), atlas.animated_rect("block/fire_1", 1).unwrap());
    assert_ne!(a, b, "Feuer-Animation steht still");
    assert!(((a[2] - a[0]) - (a[3] - a[1])).abs() < 1e-6, "Feuerbild nicht quadratisch");
    println!("Entity-Atlas {}x{} px, {} Skin-Plaetze", atlas.size, atlas.size, atlas.skin_slot_count());
    // Alle reservierten Plaetze muessen hineinpassen -- auch die Karten und dahinter die Items.
    assert_eq!(atlas.map_slot_count(), assets::MAP_SLOTS, "Kartenplaetze");
    assert_eq!(atlas.item_slot_count(), assets::ITEM_SLOTS, "Item-Plaetze");
    assert_eq!(atlas.head_slot_count(), assets::HEAD_SLOTS, "Kopfplaetze");

    let mut fehlend = Vec::new();
    for texture in &textures {
        match atlas.rect(texture) {
            Some([u0, v0, u1, v1]) => println!("  {texture}: u {u0:.4}..{u1:.4}, v {v0:.4}..{v1:.4}"),
            None => fehlend.push(*texture),
        }
    }
    assert!(fehlend.is_empty(), "Texturen fehlen in der Installation (Tippfehler im Modellpfad?): {fehlend:?}");

    // Die Skin-Plaetze muessen alle da sein -- ohne sie kann kein nachgeladener Skin landen.
    assert_eq!(atlas.skin_slot_count(), assets::SKIN_SLOTS, "reservierte Skin-Plaetze fehlen (Atlas zu klein?)");
    for slot in 0..atlas.skin_slot_count() {
        let (x, y) = atlas.skin_slot_origin(slot).expect("Platz ohne Position");
        assert!(x + assets::SKIN_SIZE <= atlas.size && y + assets::SKIN_SIZE <= atlas.size, "Skin-Platz {slot} liegt ausserhalb");
    }

    // Dasselbe fuer die Umhaenge. Sie liegen NACH den Skins im Regal, also trifft ein zu
    // kleiner Atlas zuerst sie -- genau darum stehen sie hier mit eigener Zusicherung.
    assert_eq!(atlas.cape_slot_count(), assets::CAPE_SLOTS, "reservierte Umhang-Plaetze fehlen (Atlas zu klein?)");
    for slot in 0..atlas.cape_slot_count() {
        let (x, y) = atlas.cape_slot_origin(slot).expect("Umhang-Platz ohne Position");
        assert!(x + assets::CAPE_WIDTH <= atlas.size && y + assets::CAPE_HEIGHT <= atlas.size, "Umhang-Platz {slot} liegt ausserhalb");
    }

    // Und fuer die Item-Plaetze (Items in den Haenden). Sie liegen als LETZTE im Regal, ein
    // zu voller Atlas trifft also zuerst sie -- ohne diese Zusicherung wuerde ein spaeter
    // hinzugefuegter Mob still dazu fuehren, dass Items in Haenden unsichtbar bleiben.
    assert_eq!(atlas.item_slot_count(), assets::ITEM_SLOTS, "reservierte Item-Plaetze fehlen (Atlas zu klein?)");
    for slot in 0..atlas.item_slot_count() {
        let (x, y) = atlas.item_slot_origin(slot).expect("Item-Platz ohne Position");
        assert!(x + assets::ITEM_SLOT_SIZE <= atlas.size && y + assets::ITEM_SLOT_SIZE <= atlas.size, "Item-Platz {slot} liegt ausserhalb");
    }

    assert_eq!(atlas.trim_slot_count(), assets::TRIM_SLOTS, "reservierte Trim-Plaetze fehlen (Atlas zu klein?)");
    // Die Tab-Listen-Koepfe kommen ganz zuletzt.
    assert_eq!(atlas.head_slot_count(), assets::HEAD_SLOTS, "reservierte Kopf-Plaetze fehlen (Atlas zu klein?)");

    // Kein Rechteck darf sich mit einem anderen ueberlappen -- ueberlappende Kacheln zeigen
    // im Spiel Teile der Nachbartextur (halbe Zombiegesichter auf dem Creeper).
    //
    // Vorher entdoppeln: mehrere Typen teilen sich absichtlich eine Textur (der Riese nutzt
    // die Zombie-Textur), und die bekommen dasselbe Rechteck -- das ist gewollt, keine
    // Ueberlappung. Genau darueber ist der erste Anlauf dieses Tests gestolpert.
    let mut eindeutig: Vec<&str> = textures.clone();
    eindeutig.sort_unstable();
    eindeutig.dedup();
    let mut rects: Vec<(&str, [f32; 4])> = eindeutig.iter().filter_map(|t| atlas.rect(t).map(|r| (*t, r))).collect();
    let skins: Vec<(&str, [f32; 4])> = (0..atlas.skin_slot_count()).filter_map(|s| atlas.skin_slot_rect(s).map(|r| ("skin", r))).collect();
    rects.extend(skins);
    let capes: Vec<(&str, [f32; 4])> = (0..atlas.cape_slot_count()).filter_map(|s| atlas.cape_slot_rect(s).map(|r| ("cape", r))).collect();
    rects.extend(capes);
    let items: Vec<(&str, [f32; 4])> = (0..atlas.item_slot_count()).filter_map(|s| atlas.item_slot_rect(s).map(|r| ("item", r))).collect();
    rects.extend(items);
    let heads: Vec<(&str, [f32; 4])> = (0..atlas.head_slot_count()).filter_map(|s| atlas.head_slot_rect(s).map(|r| ("head", r))).collect();
    rects.extend(heads);
    let trims: Vec<(&str, [f32; 4])> = (0..atlas.trim_slot_count()).filter_map(|s| atlas.trim_slot_rect(s).map(|r| ("trim", r))).collect();
    rects.extend(trims);
    for (i, (name_a, a)) in rects.iter().enumerate() {
        for (name_b, b) in rects.iter().skip(i + 1) {
            let getrennt = a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1];
            assert!(getrennt, "'{name_a}' und '{name_b}' ueberlappen im Atlas: {a:?} / {b:?}");
        }
    }
}

// Die weisse Kachel im ECHTEN, voll gepackten Atlas: sie muss da sein UND im Bild wirklich
// weiss, sonst faerbt sie die Boxen der modellosen Entities ein (etwa gruen).
#[test]
#[ignore]
fn die_weisse_kachel_ueberlebt_den_vollen_atlas() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let table = world::entity_model::model_table();
    let mut textures: Vec<&str> = table.iter().map(|(_, model, _)| model.texture).collect();
    textures.push(world::player_model(false).texture);
    textures.extend(world::DEFAULT_SKINS.iter().map(|(texture, _)| *texture));

    let atlas = assets::build_entity_atlas(&mut mc, &textures);
    let [u0, v0, u1, v1] = atlas.white_rect();
    println!("weisse Kachel = [{u0}, {v0}, {u1}, {v1}]");
    assert!(u1 > u0 && v1 > v0, "die weisse Kachel hat keinen Platz bekommen");

    // Mitte der Kachel im Bild nachschlagen.
    let px = ((u0 + u1) / 2.0 * atlas.size as f32) as u32;
    let py = ((v0 + v1) / 2.0 * atlas.size as f32) as u32;
    let i = ((py * atlas.size + px) * 4) as usize;
    assert_eq!(&atlas.rgba[i..i + 4], &[255, 255, 255, 255], "die Kachel ist nicht weiss -- eine Textur hat sie ueberschrieben");
}
