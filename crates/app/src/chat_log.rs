//! Chat-Mitschrift: jede Chatzeile landet als Klartext in ~/coral-client/chatlogs/<Datum>.log
//! (Windows: %USERPROFILE%\coral-client\chatlogs), eine Datei pro Tag.
//!
//! Neben dem Skin-Ordner statt in ~/.config, weil man die Logs von Hand oeffnen und
//! durchsuchen will -- ein versteckter Konfigurationsordner ist dafuer der falsche Ort.
//! Jede Zeile wird sofort angehaengt (Datei auf, schreiben, zu), damit bei einem Absturz
//! nichts verloren geht -- aber in einem EIGENEN Thread: Dateizugriffe im Haupt-Thread lassen
//! bei einem Schwall Nachrichten oder langsamer Platte Bilder haengen.

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
            for (datum, zeile) in rx {
                write_now(&datum, &zeile);
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

fn write_now(datum: &str, zeile: &str) {
    let Some(ordner) = dir() else { return };
    // Fehler still schlucken: ein voller oder schreibgeschuetzter Ordner darf den Chat nicht stoeren.
    let _ = std::fs::create_dir_all(&ordner);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(ordner.join(format!("{datum}.log"))) {
        let _ = f.write_all(zeile.as_bytes());
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
