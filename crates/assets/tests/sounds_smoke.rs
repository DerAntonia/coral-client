// Prueft die ganze Kette gegen die echte lokale 1.21.11-Installation: sounds.json laden, ein
// Event aufloesen, die genannte Datei tatsaechlich aus dem Objects-Store holen. #[ignore] wie
// die uebrigen Installations-Tests -- manuell:
// cargo test -p assets --test sounds_smoke -- --ignored --nocapture
//
// Warum das ueber die Unit-Tests hinaus noetig ist: die pruefen das FORMAT an erfundenem JSON.
// Ob Vanillas echte Datei diesem Format entspricht, ob die Pfade im Objects-Store wirklich so
// heissen und ob jedes Event auch eine Datei hat, sagt nur ein Lauf gegen die Installation.
use assets::{MinecraftInstallation, SoundIndex};

#[test]
#[ignore]
fn sounds_gegen_echte_installation() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let index = SoundIndex::load(&mut mc).expect("sounds.json muss lesbar sein");
    println!("Sound-Events: {}", index.len());
    assert!(index.len() > 1500, "unplausibel wenige Events: {}", index.len());

    // Ein paar Events, die im Spiel dauernd vorkommen -- und eines mit mehreren Aufnahmen,
    // damit die Variantenauswahl ueberhaupt etwas zu tun hat.
    for event in [
        "minecraft:block.stone.step",
        "minecraft:entity.generic.explode",
        "minecraft:entity.player.attack.strong",
        "minecraft:item.totem.use",
        "minecraft:entity.experience_orb.pickup",
    ] {
        let varianten = index.variants(event).unwrap_or_else(|| panic!("{event} fehlt"));
        assert!(!varianten.is_empty(), "{event} hat keine Aufnahmen");
        let gewaehlt = index.pick(event, 12345).unwrap();
        let bytes = mc.load_bytes(&gewaehlt.path).unwrap_or_else(|e| panic!("{}: {e}", gewaehlt.path));
        let wav = audio::vorbis::decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", gewaehlt.path));
        println!(
            "{event}: {} Aufnahme(n), gewaehlt {} -> {:.0} Hz, {:.3} s",
            varianten.len(),
            gewaehlt.path,
            wav.sample_rate,
            wav.duration()
        );
        assert!(!wav.samples.is_empty(), "{} dekodiert zu Stille", gewaehlt.path);
    }

    // Jetzt die Breite: JEDE von sounds.json genannte Datei muss auch im Objects-Store
    // liegen. Fehlt eine, bliebe im Spiel genau ein Geraeusch stumm -- und das faellt
    // erfahrungsgemaess nie jemandem auf, bis man es sucht.
    let mut fehlend = Vec::new();
    let mut geprueft = 0usize;
    for event in index.event_names() {
        for variante in index.variants(event).unwrap_or(&[]) {
            geprueft += 1;
            if mc.load_bytes(&variante.path).is_err() {
                fehlend.push(variante.path.clone());
            }
        }
    }
    println!("{geprueft} Aufnahmen genannt, {} fehlen", fehlend.len());
    assert!(fehlend.is_empty(), "fehlende Dateien (erste 10): {:?}", &fehlend[..fehlend.len().min(10)]);
}

// Der Fall, der die Weltgeraeusche tatsaechlich stumm gemacht hat: ein aktives Resourcepack
// mit EIGENER sounds.json. Vorher wurde nur dessen Datei gelesen -- aus 1837 Events wurden 19,
// und alles ausser den vom Pack ersetzten Klaengen blieb still. Dieser Test laeuft gegen das
// echte Pack im Skin-Ordner, wenn dort eines mit sounds.json liegt.
#[test]
#[ignore]
fn ein_pack_mit_eigener_sounds_json_verdeckt_vanilla_nicht() {
    let Some(pack) = pack_mit_sounds_json() else {
        eprintln!("kein Pack mit sounds.json im Skin-Ordner -- Test uebersprungen");
        return;
    };
    println!("Pack: {}", pack.display());

    let mut ohne = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let vanilla = SoundIndex::load(&mut ohne).expect("sounds.json muss lesbar sein");

    let mut mit = MinecraftInstallation::locate("1.21.11", Some(pack)).expect("Installation fehlt");
    let geschichtet = SoundIndex::load(&mut mit).expect("sounds.json muss lesbar sein");

    println!("ohne Pack: {} Events, mit Pack: {} Events", vanilla.len(), geschichtet.len());
    assert!(
        geschichtet.len() >= vanilla.len(),
        "ein Pack darf Events nur ersetzen oder hinzufuegen, nie welche verlieren ({} -> {})",
        vanilla.len(),
        geschichtet.len()
    );
    // Stichprobe: ein Klang, den ein PvP-Pack sicher nicht mitbringt, muss weiter da sein.
    assert!(
        geschichtet.variants("minecraft:entity.player.hurt").is_some_and(|v| !v.is_empty()),
        "entity.player.hurt ist mit aktivem Pack verschwunden"
    );
}

fn pack_mit_sounds_json() -> Option<std::path::PathBuf> {
    let skins = std::path::PathBuf::from(std::env::var_os("HOME")?).join("coral-client").join("skins");
    for eintrag in std::fs::read_dir(skins).ok()?.flatten() {
        let dir = eintrag.path();
        if dir.join("assets").join("minecraft").join("sounds.json").is_file() {
            return Some(dir);
        }
    }
    None
}
