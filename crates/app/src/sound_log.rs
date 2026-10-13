// Klang-Mitschnitt (CORAL_SOUNDLOG=<pfad>, Launcher-Schalter "Klang-Mitschnitt"): jeder Klang,
// den der Client abspielt oder verwirft, als eine Zeile in einer eigenen Datei. Gebaut fuer
// das PvP-Soundpack -- welche Events in einer echten Runde ueberhaupt vorkommen, wie oft, von
// wem und ob der Server oder der Client sie ausloest (nur Server-Klaenge kann ein anderer
// Client ueberhaupt sehen; die vom Client selbst gespielten kommen nie als Paket).
//
// Anders als CORAL_SOUND_DEBUG (Fehlersuche im Paketpfad, ins Client-Log) steht hier nur,
// was am Ende gehoert wurde, in einer Datei fuer sich -- auswertbar, ohne das Client-Log
// durchzusieben.
//
// Global statt im App-Zustand: der eigene Totem-Pop spielt im Netzwerk-Task (s.
// net::Events::with_immediate), und der soll genauso im Mitschnitt stehen.

use std::fs::File;
use std::io::{LineWriter, Write};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct SoundLog {
    start: Instant,
    // LineWriter: jede Zeile steht sofort in der Datei, auch wenn der Client abstuerzt. Ein
    // Schreibfehler schaltet den Mitschnitt ab, statt bei jedem Klang erneut zu scheitern.
    file: Mutex<Option<LineWriter<File>>>,
}

fn log() -> Option<&'static SoundLog> {
    static LOG: OnceLock<Option<SoundLog>> = OnceLock::new();
    LOG.get_or_init(|| {
        let path = std::env::var("CORAL_SOUNDLOG").ok().filter(|p| !p.is_empty())?;
        let mut file = match File::create(&path) {
            Ok(f) => LineWriter::new(f),
            Err(e) => {
                eprintln!("[soundlog] {path} nicht schreibbar ({e}) -- kein Klang-Mitschnitt");
                return None;
            }
        };
        println!("[soundlog] Klang-Mitschnitt nach {path}");
        let _ = writeln!(
            file,
            "# Klang-Mitschnitt. Spalten: Sekunden seit Start | Herkunft | Event | Kategorie | Verursacher | Abstand zum Ohr | Lautstaerke/Tonhoehe | Ergebnis\n\
             # Herkunft: server = Ton-Paket; explosion/levelevent/gameevent = Server-Paket, Klang spielt der Client;\n\
             #           client = der Client spielt ihn ganz selbst (eigene Schritte, eigener Schaden, ...); ui = Menueklang.\n\
             # Verursacher bei ortsgebundenen Klaengen (z.B. alle Angriffsklaenge): die naechste Entity hoechstens 2 Bloecke daneben."
        );
        Some(SoundLog { start: Instant::now(), file: Mutex::new(Some(file)) })
    })
    .as_ref()
}

pub fn enabled() -> bool {
    log().is_some()
}

// Eine Zeile schreiben; die Zeit davor setzt diese Funktion. Die Zeile wird nur gebaut, wenn
// der Mitschnitt laeuft -- das hier liegt im Abspielpfad jedes Klangs.
pub fn line(text: impl FnOnce() -> String) {
    let Some(log) = log() else { return };
    let t = log.start.elapsed().as_secs_f64();
    let Ok(mut file) = log.file.lock() else { return };
    if let Some(f) = file.as_mut() {
        if writeln!(f, "{t:9.3}  {}", text()).is_err() {
            *file = None;
        }
    }
}

// Die Spalten einer Klang-Zeile, damit alle Abspielstellen gleich aussehen.
pub fn sound(herkunft: &str, name: &str, kategorie: impl std::fmt::Debug, wer: &str, abstand: Option<f64>, volume: f32, pitch: f32, ergebnis: &str) {
    line(|| {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        let abstand = abstand.map(|a| format!("{a:5.1} Bl")).unwrap_or_else(|| "    -   ".to_string());
        format!("{herkunft:<10} {name:<44} {:<9} {wer:<30} {abstand}  vol={volume:.2} pitch={pitch:.2}  {ergebnis}", format!("{kategorie:?}"))
    });
}
