// Jede WGSL-Datei einmal uebersetzen und validieren.
//
// Grund: Shader-Fehler fallen sonst erst beim Start des Clients auf -- wgpu uebersetzt die
// Quellen erst zur Laufzeit, ein Tippfehler laesst also den ganzen Client mit einem Panic
// beim Erzeugen der Pipeline hochgehen statt im Test aufzufallen. Das Verzeichnis wird
// eingelesen statt einzelne Dateien aufzuzaehlen, damit neue Shader automatisch mitlaufen.
//
// BEIDE Shader-Verzeichnisse: die Oberflaechen-Shader liegen im app-Crate, die Welt-Shader
// (block.wgsl mit Block-, Entity-, Geister- und Hitbox-Pfad) im render-Crate. Letzteres fehlte
// hier lange -- ein Tippfehler in block.wgsl waere also trotz gruener Testlage erst beim Start
// aufgefallen, und genau das soll dieser Test verhindern.
#[test]
fn alle_wgsl_shader_sind_gueltig() {
    let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates-Verzeichnis");
    let mut geprueft = 0;
    for dir in [wurzel.join("app/src"), wurzel.join("render/src")] {
        geprueft += pruefe_verzeichnis(&dir);
    }
    assert!(geprueft > 0, "keine WGSL-Datei gefunden");
}

fn pruefe_verzeichnis(dir: &std::path::Path) -> usize {
    let mut geprueft = 0;
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{} lesbar: {e}", dir.display())) {
        let path = entry.expect("Verzeichniseintrag").path();
        if path.extension().and_then(|e| e.to_str()) != Some("wgsl") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("Shader lesbar");
        let module = match naga::front::wgsl::parse_str(&src) {
            Ok(module) => module,
            Err(err) => panic!("{}: {}", path.display(), err.emit_to_string(&src)),
        };
        let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
        if let Err(err) = validator.validate(&module) {
            panic!("{}: {err:?}", path.display());
        }
        geprueft += 1;
    }
    assert!(geprueft > 0, "keine WGSL-Datei in {} gefunden", dir.display());
    geprueft
}
