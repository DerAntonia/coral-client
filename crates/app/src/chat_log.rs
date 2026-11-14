//! Chat-Mitschrift: jede Chatzeile landet als Klartext in ~/coral-client/chatlogs/<Datum>.log
//! (Windows: %USERPROFILE%\coral-client\chatlogs), eine Datei pro Tag.
//!
//! Neben dem Skin-Ordner statt in ~/.config, weil man die Logs von Hand oeffnen und
//! durchsuchen will -- ein versteckter Konfigurationsordner ist dafuer der falsche Ort.
//! Jede Zeile wird sofort angehaengt und auf die Platte gesichert (Datei auf, schreiben,
//! sichern, zu), damit bei einem Absturz oder Stromausfall nichts verloren geht -- aber in
//! einem EIGENEN Thread: Dateizugriffe im Haupt-Thread lassen bei einem Schwall Nachrichten
//! oder langsamer Platte Bilder haengen.

use std::io::Write;
use std::path::PathBuf;

pub fn dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join("coral-client").join("chatlogs"))
}

// "[HH:MM:SS] text" -- Zeilenumbrueche einer Nachricht werden eingerueckt, damit jede
// Logzeile mit einem Zeitstempel oder einer Einrueckung beginnt und sich greppen laesst.
fn format_line(zeit: &str, text: &str) -> String {
    format!("[{zeit}] {}\n", text.replace('\n', "\n           "))
}

pub fn append(text: &str) {
    let jetzt = chrono::Local::now();
    append_at(&jetzt.format("%Y-%m-%d").to_string(), &format_line(&jetzt.format("%H:%M:%S").to_string(), text));
}

// Trennzeile beim Betreten eines Servers, damit sich Sitzungen im Tageslog auseinanderhalten lassen.
pub fn session_start(server: &str) {
    let jetzt = chrono::Local::now();
    append_at(&jetzt.format("%Y-%m-%d").to_string(), &format!("\n----- {} {} -----\n", jetzt.format("%H:%M:%S"), server));
}

// Schreib-Thread, beim ersten Gebrauch gestartet. (Datum, Zeile) je Auftrag.
fn writer() -> &'static std::sync::Mutex<std::sync::mpsc::Sender<(String, String)>> {
    static TX: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<(String, String)>>> = std::sync::OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
        let _ = std::thread::Builder::new().name("chat-log".into()).spawn(move || {
            while let Ok(erster) = rx.recv() {
                // Alles mitnehmen, was schon wartet: ein Schwall kostet so nur EIN Sichern.
                let mut auftraege = vec![erster];
                auftraege.extend(rx.try_iter());
                write_now(&auftraege);
            }
        });
        std::sync::Mutex::new(tx)
    })
}

fn append_at(datum: &str, zeile: &str) {
    if let Ok(tx) = writer().lock() {
        let _ = tx.send((datum.to_string(), zeile.to_string()));
    }
}

// Haengt die Auftraege an ihre Tagesdateien an und sichert jede Datei danach auf die Platte
// (sync_data). Ohne das Sichern liegen die Zeilen nur im Seitencache: faellt der Strom aus,
// waehrend das Spiel laeuft, kennt das Dateisystem schon die neue Laenge, aber nicht den
// Inhalt -- die Datei bekommt dort einen Block aus Nullbytes und gilt als beschaedigt.
fn write_now(auftraege: &[(String, String)]) {
    let Some(ordner) = dir() else { return };
    // Fehler still schlucken: ein voller oder schreibgeschuetzter Ordner darf den Chat nicht stoeren.
    let _ = std::fs::create_dir_all(&ordner);
    let mut i = 0;
    while i < auftraege.len() {
        let datum = &auftraege[i].0;
        let ende = i + auftraege[i..].iter().take_while(|(d, _)| d == datum).count();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(ordner.join(format!("{datum}.log"))) {
            for (_, zeile) in &auftraege[i..ende] {
                let _ = f.write_all(zeile.as_bytes());
            }
            let _ = f.sync_data();
        }
        i = ende;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mehrzeilige_nachricht_wird_eingerueckt() {
        assert_eq!(format_line("12:00:00", "a\nb"), "[12:00:00] a\n           b\n");
        assert_eq!(format_line("12:00:00", "hallo"), "[12:00:00] hallo\n");
    }
}
