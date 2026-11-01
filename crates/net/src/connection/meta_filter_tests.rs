// Ein Metadaten-Paket, das NUR die Kristall-Bodenplatte aendert, muss durchkommen --
// fehlte das Feld im Filter, stand die Platte auch bei gesetzten Kristallen da.
#[test]
fn kristall_bodenplatte_wird_weitergereicht() {
    let update = protocol::packets::EntityDataUpdate { end_crystal_show_bottom: Some(false), ..Default::default() };
    assert!(super::meta_betrifft_anzeige(&update));
    let rot = protocol::packets::EntityDataUpdate { display_left_rotation: Some([0.0, 0.0, 0.0, 1.0]), ..Default::default() };
    assert!(super::meta_betrifft_anzeige(&rot));
}
