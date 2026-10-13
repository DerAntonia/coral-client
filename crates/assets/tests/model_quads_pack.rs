// Das 3D-Item-Modell des End-Kristalls aus dem Crystal-PvP-Pack des Nutzers. #[ignore], weil es
// die lokale Installation UND das Pack braucht -- manuell:
// cargo test -p assets --test model_quads_pack -- --ignored --nocapture
use assets::MinecraftInstallation;

#[test]
#[ignore]
fn end_kristall_aus_dem_pack_wird_ein_3d_modell() {
    let pack = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("coral-client/skins/Crystal PvP Texture-Pack by DerAntonia betav4");
    let mut mc = MinecraftInstallation::locate("1.21.11", Some(pack)).expect("Installation fehlt");
    let quads = assets::item_model_quads(&mut mc, "item/end_crystal").expect("kein Modell");
    println!("{} Flaechen, Textur {}", quads.len(), quads[0].texture);
    assert_eq!(quads.len(), 18, "drei Wuerfel mit je sechs Flaechen");
    assert_eq!(quads[0].texture, "item/end_crystal");
    // Ohne Pack bleibt es Vanillas flaches Item.
    let mut vanilla = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    assert!(assets::item_model_quads(&mut vanilla, "item/end_crystal").is_none());
}
