// Testet die Asset-Lade-Kette end-to-end gegen tatsaechlich vorhandene lokale
// Versionen. #[ignore], da abhaengig von der lokalen .minecraft-Installation des
// jeweiligen Rechners -- manuell ausfuehren mit:
// cargo test -p assets -- --ignored --nocapture

#[test]
#[ignore]
fn load_icon_texture_from_shared_objects() {
    // 1.8.9: Icon liegt im geteilten Objects-System (assets/objects/), noch kein
    // Client-Jar-Fallback noetig -- validiert den ersten Lade-Pfad.
    let mut install = assets::MinecraftInstallation::locate("1.8.9", None).expect("1.8.9 nicht gefunden -- installiert?");

    let texture = install.load_texture("minecraft/icons/icon_16x16.png").expect("Icon-Textur konnte nicht geladen werden");

    println!("Icon geladen: {}x{}, {} Bytes RGBA", texture.width, texture.height, texture.rgba.len());
    assert_eq!(texture.width, 16);
    assert_eq!(texture.height, 16);
    assert_eq!(texture.rgba.len(), (texture.width * texture.height * 4) as usize);
}

#[test]
#[ignore]
fn load_block_texture_from_client_jar() {
    // 1.21.11: Block-Texturen sind nicht im Asset-Index, sondern im Client-Jar
    // gebuendelt -- validiert den Jar-Fallback-Pfad.
    let mut install = assets::MinecraftInstallation::locate("1.21.11", None).expect("1.21.11 nicht gefunden -- installiert?");

    let texture = install.load_texture("minecraft/textures/block/stone.png").expect("Stone-Textur konnte nicht geladen werden");

    println!("Stone-Textur geladen: {}x{}", texture.width, texture.height);
    assert_eq!(texture.width, 16);
    assert_eq!(texture.height, 16);
    assert_eq!(texture.rgba.len(), (texture.width * texture.height * 4) as usize);
}
