// Meshed eine Lavaquelle gegen die ECHTE lokale 1.21.11-Installation -- Gegenstueck zu
// waterlogged_slab_real.rs fuer die zweite Fluessigkeit.
//
// Grund: die Unit-Tests im world-Crate belegen die Geometrie gegen SYNTHETISCHE UVs
// (BlockUvs::single_tile().with_lava_flow(...)). Ob "block/lava_flow" beim echten Atlas-Bau
// ueberhaupt eine Kachel bekommt, bliebe damit ungeprueft -- und genau das war bei Wasser
// schon einmal die Stelle, an der es im Spiel anders aussah als im Test.
//
// #[ignore], weil eine Installation gebraucht wird -- manuell:
//   cargo test -p app --test lava_fluid_real -- --ignored --nocapture

use assets::{BlockAtlas, MinecraftInstallation};

#[test]
#[ignore]
fn lava_hat_stand_und_fliesskachel_im_echten_atlas() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let atlas = BlockAtlas::build(&mut mc, &world::block_registry::all_block_names()).expect("Atlas");

    println!("lava_uv      = {:?}", atlas.uvs.get("lava"));
    println!("lava_flow_uv = {:?}", atlas.lava_flow_uv);

    let lava_uv = atlas.uvs.get("lava").expect("lava muss eine Atlas-Kachel haben");
    let flow_uv = atlas.lava_flow_uv.expect("block/lava_flow muss im Atlas landen");
    // Zwei verschiedene Kacheln -- waere lava_flow auf die Standkachel zurueckgefallen,
    // saehe man an jeder Lavakante die ruhige statt der stroemenden Grafik.
    assert_ne!(lava_uv[0], flow_uv, "lava_still und lava_flow muessen verschiedene Kacheln sein");
}
