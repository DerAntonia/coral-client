// Varianten mit eigenen Texturen (geladener Respawn-Anker). #[ignore]: braucht die lokale
// Installation -- manuell:
// cargo test --release -p assets --test variant_textures -- --ignored --nocapture
use assets::MinecraftInstallation;

#[test]
#[ignore]
fn geladener_anker_hat_seine_eigene_oberseite() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let atlas = assets::BlockAtlas::build(&mut mc, &["respawn_anchor", "furnace"]).expect("Atlas");
    let leer = atlas.uvs["respawn_anchor"];
    let (geladen, _) = atlas.variant_uvs.get("block/respawn_anchor_1").expect("Variante fehlt");
    assert_ne!(leer[assets::FACE_UP], geladen[assets::FACE_UP], "Oberseite muss sich unterscheiden");
    assert_ne!(leer[assets::FACE_NORTH], geladen[assets::FACE_NORTH], "Seite (Ladeanzeige) muss sich unterscheiden");

    let props: &[(&str, &str)] = &[("charges", "3")];
    let modelle = assets::state_variant_models(&mut mc, &[(42, "respawn_anchor", props)]);
    assert_eq!(modelle.get(&42).map(String::as_str), Some("block/respawn_anchor_3"));
}
