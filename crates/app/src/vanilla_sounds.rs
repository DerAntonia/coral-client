//! Vanillas Klaenge: vom Event-Namen zur abspielbaren Aufnahme.
//!
//! Drei Schritte liegen dazwischen, und keiner davon ist im Protokoll enthalten:
//!
//! 1. Das Paket nennt ein Event (`block.stone.step`) -- `assets::SoundIndex` sagt, welche
//!    Dateien dazu gehoeren und welche davon diese Zufallssaat waehlt.
//! 2. Die Datei liegt als .ogg im Objects-Store der Installation und wird ueber
//!    `MinecraftInstallation::load_bytes` geholt.
//! 3. Ogg dekodieren (`audio::vorbis`) -- danach ist es dasselbe wie jede andere Aufnahme.
//!
//! Alles davon passiert auf dem AUFRUFENDEN Thread und nie im Audio-Callback: dort darf nichts
//! allokieren, und eine 3-Sekunden-.ogg zu dekodieren ist ein Vielfaches eines Puffers.
//!
//! Gehalten wird jede einmal dekodierte Aufnahme dauerhaft. Der Grund ist nicht nur
//! Geschwindigkeit: der Mixer haelt seine Stimmen ueber `Arc`, und weil die Bank nie etwas
//! freigibt, kann im Audio-Callback auch nie eine Freigabe passieren.

use std::collections::HashMap;
use std::sync::Arc;

use audio::wav::Wav;

pub struct VanillaSounds {
    /// Was `sounds.json` sagt. `None`, wenn die Datei nicht lesbar war -- dann bleibt der
    /// Client eben still, statt beim Start abzubrechen.
    index: Option<assets::SoundIndex>,
    /// Schon dekodierte Aufnahmen, nach Asset-Pfad.
    geladen: HashMap<String, Option<Arc<Wav>>>,
    /// Events, deren Fehlen schon geloggt wurde -- sonst flutet ein einziger unbekannter
    /// Klang (z.B. ein Plugin-Sound) bei jedem Schritt das Log.
    gewarnt: HashMap<String, ()>,
}

/// Eine fertige Aufnahme samt der Faktoren, die aus `sounds.json` kommen.
pub struct Resolved {
    pub sample: Arc<Wav>,
    /// Lautstaerkefaktor DIESER Aufnahme -- noch ohne die Lautstaerke aus dem Paket.
    pub volume: f32,
    /// Tonhoehenfaktor dieser Aufnahme -- ebenfalls noch ohne die aus dem Paket.
    pub pitch: f32,
}

impl VanillaSounds {
    pub fn load(installation: &mut assets::MinecraftInstallation) -> VanillaSounds {
        let index = match assets::SoundIndex::load(installation) {
            Ok(index) => {
                println!("[audio] {} Sound-Events aus sounds.json geladen", index.len());
                Some(index)
            }
            Err(e) => {
                // Kein Abbruch: ohne Ton laesst sich spielen, ohne Welt nicht.
                eprintln!("[audio] sounds.json nicht lesbar ({e}) -- Weltgeräusche bleiben stumm");
                None
            }
        };
        VanillaSounds { index, geladen: HashMap::new(), gewarnt: HashMap::new() }
    }

    pub fn is_ready(&self) -> bool {
        self.index.is_some()
    }

    /// Event-Name + Zufallssaat -> abspielbare Aufnahme.
    ///
    /// `None` heisst still bleiben: unbekanntes Event, fehlende Datei oder kaputtes Ogg. Ein
    /// fehlender Klang darf nie mehr sein als ein fehlender Klang.
    pub fn resolve(&mut self, installation: &mut assets::MinecraftInstallation, event: &str, seed: i64) -> Option<Resolved> {
        let index = self.index.as_ref()?;
        let Some(variante) = index.pick(event, seed) else {
            // Zwei verschiedene Faelle, die sich sehr unterschiedlich anfuehlen: ein Event,
            // das wir gar nicht kennen (dann stimmt etwas mit den Klangdaten nicht), und
            // eines mit absichtlich leerer Liste -- `intentionally_empty` gibt es in Vanilla
            // wirklich, und ein "unbekannt" waere dort schlicht falsch.
            if index.variants(event).is_none() && self.gewarnt.insert(event.to_string(), ()).is_none() {
                eprintln!("[audio] unbekanntes Sound-Event: {event}");
            }
            return None;
        };
        let (path, volume, pitch) = (variante.path.clone(), variante.volume, variante.pitch);

        // Der Eintrag wird auch bei MISSERFOLG gesetzt (als None): eine kaputte Datei soll
        // nicht bei jedem Schritt neu vom Datentraeger gelesen und neu dekodiert werden.
        if let Some(vorhanden) = self.geladen.get(&path) {
            return vorhanden.clone().map(|sample| Resolved { sample, volume, pitch });
        }
        let dekodiert = match installation.load_bytes(&path) {
            Ok(bytes) => match audio::vorbis::decode(&bytes) {
                Ok(wav) => Some(Arc::new(wav)),
                Err(e) => {
                    eprintln!("[audio] {path} nicht dekodierbar: {e}");
                    None
                }
            },
            Err(e) => {
                eprintln!("[audio] {path} nicht ladbar: {e}");
                None
            }
        };
        self.geladen.insert(path, dekodiert.clone());
        dekodiert.map(|sample| Resolved { sample, volume, pitch })
    }

    /// Wie viele Aufnahmen schon im Speicher liegen -- fuers Debug-Overlay.
    pub fn cached(&self) -> usize {
        self.geladen.len()
    }
}
