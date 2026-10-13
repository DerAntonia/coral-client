// Erzeugt eine Tabelle aller mitgelieferten Klangdateien (assets/sounds/**.wav) und bettet sie
// per include_bytes! ins Binary ein.
//
// Warum eingebettet und nicht zur Laufzeit geladen: der Client soll startbar sein, egal aus
// welchem Verzeichnis er aufgerufen wird. Ein relativer Pfad auf assets/ funktioniert nur,
// solange man aus dem Projektordner startet -- ein Klangsatz, der je nach Arbeitsverzeichnis da
// ist oder nicht, waere eine unnoetige Fehlerquelle.
//
// Warum generiert und nicht von Hand: es sind ueber 150 Dateien. Eine handgepflegte Liste waere
// nach der ersten Aenderung am Klangsatz veraltet.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let wurzel = manifest.join("../../assets/sounds");

    let mut eintraege: Vec<(String, PathBuf)> = Vec::new();
    sammle(&wurzel, &wurzel, &mut eintraege);
    // Sortiert, damit die erzeugte Datei bei gleichem Bestand identisch bleibt und nicht bei
    // jedem Build einen anderen Inhalt (und damit unnoetige Neuuebersetzungen) erzeugt.
    eintraege.sort();

    let mut out = String::from("// Erzeugt von build.rs -- nicht von Hand aendern.\npub static SOUNDS: &[(&str, &[u8])] = &[\n");
    for (name, pfad) in &eintraege {
        out.push_str(&format!("    ({:?}, include_bytes!({:?})),\n", name, pfad));
    }
    out.push_str("];\n");

    let ziel = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("sounds_table.rs");
    std::fs::write(&ziel, out).expect("Klangtabelle schreiben");

    // Neu bauen, wenn Dateien dazukommen oder verschwinden.
    println!("cargo:rerun-if-changed={}", wurzel.display());
    for (_, pfad) in &eintraege {
        println!("cargo:rerun-if-changed={}", pfad.display());
    }
}

fn sammle(wurzel: &Path, ordner: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(eintraege) = std::fs::read_dir(ordner) else { return };
    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();
        if pfad.is_dir() {
            sammle(wurzel, &pfad, out);
        } else if pfad.extension().is_some_and(|e| e == "wav") {
            // Name ohne Wurzel und ohne Endung: "kenney-interface/click_001".
            let Ok(rel) = pfad.strip_prefix(wurzel) else { continue };
            let name = rel.with_extension("").to_string_lossy().replace('\\', "/");
            out.push((name, pfad));
        }
    }
}
