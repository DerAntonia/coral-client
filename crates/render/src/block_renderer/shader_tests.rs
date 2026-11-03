// block.wgsl wird sonst erst beim Anlegen der Pipelines geprueft, also beim Start des
// Clients. Hier schon im Test: parsen und validieren wie wgpu es tut.
#[test]
fn block_shader_ist_gueltiges_wgsl() {
    let quelle = crate::block_shader_source();
    let modul = naga::front::wgsl::parse_str(&quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&quelle)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&modul)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(modul.entry_points.iter().any(|e| e.name == "fs_weather"));
}
