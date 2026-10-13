// Modul "input-log" (Modul-Menue: Utility). Schreibt die ueber den Raw-Input-Pfad (evdev,
// s. raw_input) erkannten SPIEL-Eingaben mit Zeitstempel in eine lokale Textdatei neben die
// uebrigen Client-Dateien (shared_config_dir, wie servers.conf).
//
// BEWUSST nur bei Fenster-Fokus und nur im Spiel (nicht bei offenem Chat/Textfeld). Der
// evdev-Pfad liefert Tastendruecke FOKUS-UNABHAENGIG: die Lese-Threads laufen durch (s.
// raw_input::spawn_key_reader), und der Haupt-Loop leert den Tastatur-Kanal in JEDEM Frame
// bedingungslos (s. main.rs, raw_keyboard.drain()). Ohne die Fokus-Schranke landete hier also
// auch alles, was man ausserhalb des Clients tippt -- das waere ein systemweiter Keylogger,
// nicht ein Spiel-Input-Log. Die Aufrufer in main.rs stehen deshalb hinter denselben
// self.focused- und chat_input.is_none()-Schranken wie apply_raw_key.
//
// Protokolliert werden die EINZELNEN Eingaben: Taste runter/hoch, Maustaste runter/hoch,
// Radraste. NICHT die fortlaufende Mausbewegung -- die kommt jeden Frame zu Dutzenden und
// wuerde die Datei fluten, ohne etwas ueber eine "Eingabe" auszusagen.
//
// Aus (Vorgabe): nichts wird geoeffnet oder geschrieben. Erst beim Einschalten im Modul-Menue
// (ModuleId::InputLog) wird beim ersten Ereignis die Datei im Anhaenge-Modus geoeffnet und mit
// einer Sitzungs-Kopfzeile begonnen.
//
// Ein Loslassen wird nur protokolliert, wenn sein Druecken seit dem Einschalten im Log steht.
// Sonst begann jede Sitzung mit "MAUS HOCH Left": der Klick, der das Modul im Menue
// einschaltet, war schon unten, bevor das Log lief.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use winit::keyboard::KeyCode;

use crate::raw_input::MouseButton;

pub struct InputLog {
    // Wohin geschrieben wird. None = kein Ort bekannt (kein Config-Verzeichnis) -> stumm.
    path: Option<PathBuf>,
    enabled: bool,
    // Lazy: erst beim ersten Ereignis geoeffnet, damit blosses Einschalten keine leere Datei
    // anlegt und die Reihenfolge zum Settings-Laden egal ist.
    writer: Option<BufWriter<File>>,
    // Seit dem Einschalten gedrueckte und noch nicht losgelassene Eingaben ("TASTE KeyW",
    // "MAUS Left") -- nur deren Loslassen kommt ins Log.
    unten: HashSet<String>,
}

impl InputLog {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path, enabled: false, writer: None, unten: HashSet::new() }
    }

    // Schaltet das Modul um. Beim Ausschalten wird der Puffer geleert und die Datei
    // geschlossen; das naechste Einschalten beginnt eine neue Sitzung mit eigener Kopfzeile.
    pub fn set_enabled(&mut self, on: bool) {
        if on == self.enabled {
            return;
        }
        self.enabled = on;
        self.unten.clear();
        if !on {
            if let Some(mut w) = self.writer.take() {
                let _ = w.flush();
            }
        }
    }

    pub fn key(&mut self, code: KeyCode, pressed: bool) {
        if !self.enabled {
            return;
        }
        self.button("TASTE", format!("{code:?}"), pressed);
    }

    pub fn mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if !self.enabled {
            return;
        }
        self.button("MAUS", format!("{button:?}"), pressed);
    }

    // Druecken immer, Loslassen nur nach einem protokollierten Druecken (s. Kopfkommentar).
    fn button(&mut self, art: &str, detail: String, pressed: bool) {
        let id = format!("{art} {detail}");
        if pressed {
            self.unten.insert(id);
        } else if !self.unten.remove(&id) {
            return;
        }
        self.write_line(art, richtung(pressed), detail);
    }

    pub fn wheel(&mut self, rasten: f32) {
        if !self.enabled || rasten == 0.0 {
            return;
        }
        let richtung = if rasten > 0.0 { "HOCH" } else { "RUNTER" };
        self.write_line("RAD", richtung, format!("{}", rasten.abs() as i32));
    }

    // Oeffnet die Datei beim ersten Bedarf. Scheitert das (kein Ort, keine Rechte), wird das
    // Modul still abgeschaltet -- ein Logger darf nicht bei jedem Tastendruck erneut scheitern
    // und schon gar nicht das Spiel stoeren.
    fn writer(&mut self) -> Option<&mut BufWriter<File>> {
        if self.writer.is_none() {
            let path = self.path.clone()?;
            match OpenOptions::new().create(true).append(true).open(&path) {
                Ok(file) => {
                    let mut w = BufWriter::new(file);
                    let _ = writeln!(w, "# --- Sitzung {} ---", now());
                    let _ = w.flush();
                    self.writer = Some(w);
                }
                Err(_) => {
                    self.enabled = false;
                    return None;
                }
            }
        }
        self.writer.as_mut()
    }

    fn write_line(&mut self, art: &str, richtung: &str, detail: String) {
        let zeile = fmt_line(&now(), art, richtung, &detail);
        if let Some(w) = self.writer() {
            // Fehler still schlucken -- s. writer().
            let _ = writeln!(w, "{zeile}");
            // Zeilenweise leeren: ein Absturz soll die letzten Eingaben nicht verschlucken,
            // und die Rate (Tasten/Klicks, nicht pro Frame) macht das unproblematisch.
            let _ = w.flush();
        }
    }
}

fn richtung(pressed: bool) -> &'static str {
    if pressed {
        "RUNTER"
    } else {
        "HOCH"
    }
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

// Eine Zeile als Tab-getrennte Spalten: Zeitstempel, Art, Richtung, Detail. Als eigene reine
// Funktion, damit das Format geprueft werden kann, ohne eine Datei anzulegen.
fn fmt_line(stamp: &str, art: &str, richtung: &str, detail: &str) -> String {
    format!("{stamp}\t{art}\t{richtung}\t{detail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn richtung_folgt_dem_druckzustand() {
        assert_eq!(richtung(true), "RUNTER");
        assert_eq!(richtung(false), "HOCH");
    }

    #[test]
    fn zeile_ist_tab_getrennt_in_fester_reihenfolge() {
        let z = fmt_line("2026-10-02 14:33:01.234", "TASTE", "RUNTER", "KeyW");
        assert_eq!(z, "2026-10-02 14:33:01.234\tTASTE\tRUNTER\tKeyW");
        assert_eq!(z.split('\t').count(), 4);
    }

    // Aus (Vorgabe) heisst wirklich aus: ein frisch gebauter Logger schreibt nichts und legt
    // auch keine Datei an (Pfad None -> selbst bei aktivem Modul kein Oeffnen).
    #[test]
    fn aus_als_vorgabe_und_ohne_pfad_stumm() {
        let mut log = InputLog::new(None);
        assert!(!log.enabled);
        log.key(KeyCode::KeyW, true); // darf nicht paniken
        log.set_enabled(true);
        log.key(KeyCode::KeyW, true); // Pfad None -> writer() gibt None, kein Oeffnen
        assert!(log.writer.is_none());
    }

    // Der Klick, der das Modul einschaltet, war schon unten: sein Loslassen gehoert nicht ins
    // Log, ein danach gedrueckter schon.
    #[test]
    fn loslassen_ohne_druecken_wird_ausgelassen() {
        let mut log = InputLog::new(None);
        log.set_enabled(true);
        log.mouse_button(MouseButton::Left, false);
        assert!(log.unten.is_empty());
        log.mouse_button(MouseButton::Left, true);
        assert!(log.unten.contains("MAUS Left"));
        log.mouse_button(MouseButton::Left, false);
        assert!(log.unten.is_empty());
    }
}
