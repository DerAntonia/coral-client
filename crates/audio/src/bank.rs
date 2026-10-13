// Die mitgelieferten Klangaufnahmen, ansprechbar ueber ihren Namen.
//
// Herkunft und Lizenz: Kenney (kenney.nl), "Interface Sounds" und "UI Audio", beide unter
// Creative Commons CC0 1.0 -- Nutzung auch kommerziell erlaubt, Namensnennung nicht
// erforderlich. Der Lizenztext liegt bei den Dateien (assets/sounds/LICENSE-kenney.txt).
//
// Kenneys Originalpakete sind OGG; die Dateien hier sind daraus nach WAV gewandelt (48 kHz,
// Mono, 16 Bit), damit der Client ohne Ogg-Dekoder auskommt -- s. assets/sounds/README.md.
//
// Eingebettet ist damit weit mehr, als aktuell in der App verdrahtet ist: eine erste
// Zuordnung nach allen Kategorien wurde nach DATEINAMEN geraten, nicht nach Gehoer -- "die
// hoeren sich ultra scheisse an" -- und ist deshalb bis auf einen einzigen, durchgehoerten
// und bestaetigten Klang zurueckgenommen (s. app::ui_sound::UiSound::sample). Die restlichen
// Aufnahmen bleiben trotzdem hier: scripts/klaenge-anhoeren.sh braucht sie zum Durchhoeren,
// wenn als naechstes ein weiterer Klang ausgesucht wird.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::wav::Wav;

include!(concat!(env!("OUT_DIR"), "/sounds_table.rs"));

/// Entpackt Aufnahmen bei Bedarf und haelt sie danach fest.
///
/// Zwei Gruende fuer das Zwischenspeichern: eine Aufnahme wird beim ersten Mal entpackt und
/// danach nur noch gereicht (ein Arc-Klon), und die Bank haelt jede Aufnahme dauerhaft --
/// deshalb kann im Audio-Callback nie eine Freigabe passieren (s. Voice::sample).
///
/// Entpackt wird IMMER auf dem aufrufenden Thread, nie im Audio-Callback: dort darf nichts
/// allokieren.
pub struct SoundBank {
    geladen: Mutex<HashMap<&'static str, Option<Arc<Wav>>>>,
}

impl SoundBank {
    pub fn new() -> Self {
        SoundBank { geladen: Mutex::new(HashMap::new()) }
    }

    /// Alle mitgelieferten Namen, alphabetisch -- z.B. "kenney-interface/click_001".
    pub fn names() -> impl Iterator<Item = &'static str> {
        SOUNDS.iter().map(|(name, _)| *name)
    }

    /// `None`, wenn es den Namen nicht gibt oder die Datei unlesbar ist. Ein fehlender Klang
    /// darf den Client nicht anhalten -- er bleibt dann still.
    pub fn get(&self, name: &str) -> Option<Arc<Wav>> {
        let Ok(mut cache) = self.geladen.lock() else { return None };
        // Der Schluessel muss 'static sein (er kommt aus der erzeugten Tabelle); der
        // uebergebene Name ist es nicht zwingend, deshalb erst in der Tabelle nachschlagen.
        let (schluessel, bytes) = SOUNDS.iter().find(|(n, _)| *n == name)?;
        if let Some(vorhanden) = cache.get(schluessel) {
            return vorhanden.clone();
        }
        let geparst = match Wav::from_bytes(bytes) {
            Ok(w) => Some(Arc::new(w)),
            Err(e) => {
                eprintln!("[audio] Klang \"{name}\" nicht lesbar: {e:?}");
                None
            }
        };
        cache.insert(schluessel, geparst.clone());
        geparst
    }
}

impl Default for SoundBank {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Die Klaenge muessen ueberhaupt eingebettet sein -- schlaegt das fehl, hat build.rs den
    // Ordner nicht gefunden und der Client waere lautlos, ohne dass es sonst auffiele.
    fn die_klaenge_sind_eingebettet() {
        let namen: Vec<&str> = SoundBank::names().collect();
        assert!(namen.len() > 100, "zu wenige Klaenge eingebettet: {}", namen.len());
        assert!(namen.iter().any(|n| n.starts_with("kenney-interface/")), "Interface-Pack fehlt");
        assert!(namen.iter().any(|n| n.starts_with("kenney-ui/")), "UI-Pack fehlt");
    }

    #[test]
    // Jede eingebettete Datei muss auch lesbar sein. Das prueft in einem Rutsch den WAV-Leser
    // gegen den ECHTEN Bestand -- die Testdateien im wav-Modul sind selbstgebaut und koennten
    // an einer Eigenheit echter Dateien vorbeigehen.
    fn jede_datei_ist_lesbar() {
        let bank = SoundBank::new();
        for name in SoundBank::names() {
            let w = bank.get(name).unwrap_or_else(|| panic!("{name} nicht lesbar"));
            assert!(!w.samples.is_empty(), "{name} ist leer");
            assert_eq!(w.sample_rate, 48_000.0, "{name} hat eine andere Abtastrate");
            assert!(w.duration() < 5.0, "{name} ist mit {}s zu lang fuer einen UI-Klang", w.duration());
        }
    }

    #[test]
    // Zweimal dasselbe holen darf nicht zweimal entpacken -- sonst haenge die Latenz eines
    // Klangs an seiner Dateigroesse.
    fn zweiter_zugriff_kommt_aus_dem_zwischenspeicher() {
        let bank = SoundBank::new();
        let name = SoundBank::names().next().expect("mindestens ein Klang");
        let a = bank.get(name).expect("erster Zugriff");
        let b = bank.get(name).expect("zweiter Zugriff");
        assert!(Arc::ptr_eq(&a, &b), "es wurde zweimal entpackt");
    }

    #[test]
    fn unbekannte_namen_bleiben_still() {
        assert!(SoundBank::new().get("gibt/es/nicht").is_none());
    }
}
