// Womit der Ton unter PipeWire wirklich laeuft, und die Untergrenze von pipewire-pulse senken.
//
// Der Client spielt ueber ALSA "default", und das geht unter PipeWire durch das Pulse-Plugin zu
// pipewire-pulse. Dort gilt eine eigene Untergrenze (pulse.min.quantum/min.req, Vorgabe
// 128/48000 = 2,7 ms): ein kleinerer Puffer im Client aendert dann nichts mehr, PipeWire
// mischt trotzdem in 128er-Bloecken. Gemessen wird deshalb an PipeWire selbst: pw-dump findet
// den Knoten dieses Prozesses, pw-top seine tatsaechliche Blockgroesse.
//
// Die Grenze steht in einer eigenen Datei unter ~/.config/pipewire/pipewire-pulse.conf.d/, die
// nur dieser Client anlegt und wieder entfernt. Danach muss pipewire-pulse neu starten -- das
// reisst alle Pulse-Stroeme kurz ab, auch den eigenen; der wird hinterher neu geoeffnet.
//
// Alles nur unter Linux und alles best-effort: fehlt pw-dump/pw-top oder laeuft kein
// PipeWire, bleibt die Messung leer.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DATEI: &str = "90-coral-client.conf";
// So oft wird neu gemessen, solange der Audio-Tab offen ist. pw-top braucht selbst rund eine
// Sekunde fuer zwei Durchlaeufe.
const ABSTAND: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Messung {
    // Blockgroesse, in der PipeWire den Graphen dieses Clients gerade fuehrt (s. aus_top).
    pub quantum: u32,
    pub rate: u32,
    // Die Untergrenze des ganzen Graphen (clock.min-quantum): darunter geht es auch mit
    // eigener Pulse-Grenze nicht.
    pub min_quantum: Option<u32>,
    // Was in der eigenen Datei steht (s. eigene_grenze), None = keine.
    pub eigene_grenze: Option<u32>,
}

impl Messung {
    pub fn ms(&self) -> f32 {
        self.quantum as f32 * 1000.0 / self.rate.max(1) as f32
    }
}

#[derive(Default)]
pub struct Status {
    messung: Arc<Mutex<Option<Messung>>>,
    laeuft: Arc<AtomicBool>,
    neu_oeffnen: Arc<AtomicBool>,
    fehler: Arc<Mutex<Option<String>>>,
    letzte: Option<Instant>,
}

impl Status {
    pub fn messung(&self) -> Option<Messung> {
        *self.messung.lock().ok()?
    }

    // Laeuft gerade eine Messung oder ein Neustart?
    pub fn beschaeftigt(&self) -> bool {
        self.laeuft.load(Ordering::Relaxed)
    }

    pub fn fehler(&self) -> Option<String> {
        self.fehler.lock().ok()?.clone()
    }

    // Neu messen, falls die letzte Messung alt genug ist und keine laeuft.
    pub fn messen_wenn_faellig(&mut self) {
        if !cfg!(target_os = "linux") || self.laeuft.load(Ordering::Relaxed) || self.letzte.is_some_and(|t| t.elapsed() < ABSTAND) {
            return;
        }
        self.letzte = Some(Instant::now());
        self.laeuft.store(true, Ordering::Relaxed);
        let (ziel, laeuft) = (self.messung.clone(), self.laeuft.clone());
        std::thread::spawn(move || {
            let m = messen(std::process::id());
            if let Ok(mut z) = ziel.lock() {
                *z = m;
            }
            laeuft.store(false, Ordering::Relaxed);
        });
    }

    // Eigene Grenze in Frames setzen (None = Datei entfernen), dann pipewire-pulse neu starten.
    pub fn grenze_setzen(&mut self, frames: Option<u32>, rate: u32) {
        if self.laeuft.swap(true, Ordering::Relaxed) {
            return;
        }
        let (laeuft, neu, fehler) = (self.laeuft.clone(), self.neu_oeffnen.clone(), self.fehler.clone());
        // Sofort neu messen, sobald der Strom wieder steht.
        self.letzte = None;
        std::thread::spawn(move || {
            let ergebnis = schreiben(frames, rate).and_then(|()| neu_starten());
            if let Ok(mut f) = fehler.lock() {
                *f = ergebnis.as_ref().err().cloned();
            }
            if ergebnis.is_ok() {
                neu.store(true, Ordering::Relaxed);
            }
            laeuft.store(false, Ordering::Relaxed);
        });
    }

    // Nach dem Neustart von pipewire-pulse ist der eigene Strom tot: einmal true, dann neu oeffnen.
    pub fn strom_neu_oeffnen(&self) -> bool {
        self.neu_oeffnen.swap(false, Ordering::Relaxed)
    }
}

fn conf_pfad() -> Option<PathBuf> {
    let basis = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(basis.join("pipewire/pipewire-pulse.conf.d").join(DATEI))
}

// Die eigene Grenze aus der Datei, falls es sie gibt.
pub fn eigene_grenze() -> Option<u32> {
    parse_grenze(&std::fs::read_to_string(conf_pfad()?).ok()?)
}

fn parse_grenze(text: &str) -> Option<u32> {
    let zeile = text.lines().find(|z| z.trim_start().starts_with("pulse.min.quantum"))?;
    zeile.split('=').nth(1)?.trim().split('/').next()?.trim().parse().ok()
}

fn conf_text(frames: u32, rate: u32) -> String {
    format!(
        "# Angelegt von Coral Client (Audio-Einstellungen, \"PipeWire-Grenze\"). Senkt die\n\
         # Untergrenze von pipewire-pulse, damit kleine Puffer wirklich wirken.\n\
         # Loeschen und pipewire-pulse neu starten stellt die Vorgabe (128/48000) wieder her.\n\
         pulse.properties = {{\n    \
         pulse.min.req = {frames}/{rate}\n    \
         pulse.min.frag = {frames}/{rate}\n    \
         pulse.min.quantum = {frames}/{rate}\n\
         }}\n"
    )
}

fn schreiben(frames: Option<u32>, rate: u32) -> Result<(), String> {
    let pfad = conf_pfad().ok_or("kein Konfigurationsordner")?;
    match frames {
        Some(n) => {
            if let Some(ordner) = pfad.parent() {
                std::fs::create_dir_all(ordner).map_err(|e| e.to_string())?;
            }
            std::fs::write(&pfad, conf_text(n, rate)).map_err(|e| e.to_string())
        }
        None => match std::fs::remove_file(&pfad) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        },
    }
}

fn neu_starten() -> Result<(), String> {
    let status = std::process::Command::new("systemctl").args(["--user", "restart", "pipewire-pulse.service"]).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("systemctl: {status}"));
    }
    // Bis der Socket wieder Verbindungen annimmt.
    std::thread::sleep(Duration::from_millis(500));
    Ok(())
}

fn ausgabe(programm: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(programm).args(args).stderr(std::process::Stdio::null()).output().ok()?;
    String::from_utf8(out.stdout).ok()
}

fn messen(pid: u32) -> Option<Messung> {
    let dump: serde_json::Value = serde_json::from_str(&ausgabe("pw-dump", &[])?).ok()?;
    let (knoten, min_quantum) = aus_dump(&dump, pid)?;
    let (quantum, rate) = aus_top(&ausgabe("pw-top", &["-b", "-n", "2"])?, knoten)?;
    Some(Messung { quantum, rate, min_quantum, eigene_grenze: eigene_grenze() })
}

// Der Ausgabe-Knoten dieses Prozesses und clock.min-quantum aus den Einstellungen.
fn aus_dump(dump: &serde_json::Value, pid: u32) -> Option<(u64, Option<u32>)> {
    let objekte = dump.as_array()?;
    let knoten = objekte.iter().find_map(|o| {
        let props = o.pointer("/info/props")?;
        let eigener = props.get("application.process.id")?.as_u64()? == pid as u64;
        let ausgabe = props.get("media.class")?.as_str()? == "Stream/Output/Audio";
        (eigener && ausgabe).then(|| o.get("id")?.as_u64()).flatten()
    })?;
    let min_quantum = objekte
        .iter()
        .filter(|o| o.pointer("/props/metadata.name").and_then(|n| n.as_str()) == Some("settings"))
        .filter_map(|o| o.get("metadata")?.as_array())
        .flatten()
        .find(|e| e.get("key").and_then(|k| k.as_str()) == Some("clock.min-quantum"))
        .and_then(|e| e.get("value")?.as_u64().or_else(|| e.get("value")?.as_str()?.parse().ok()))
        .map(|v| v as u32);
    Some((knoten, min_quantum))
}

// Spalten von pw-top: S ID QUANT RATE ... -- der erste Durchlauf steht noch auf 0, also die
// letzte Zeile des Knotens. Zaehlt die Blockgroesse seines TREIBERS (die Zeile ohne "+" davor,
// meist die Soundkarte): der Knoten selbst zeigt, was er verlangt, PipeWire rundet den Graphen
// aber auf eine Zweierpotenz ab -- aus 48 wurden so 32 Frames und Aussetzer.
fn aus_top(top: &str, knoten: u64) -> Option<(u32, u32)> {
    let mut treiber = None;
    let mut treffer = None;
    for z in top.lines() {
        let Some((id, quantum, rate)) = (|| {
            let mut s = z.split_whitespace().skip(1);
            Some((s.next()?.parse::<u64>().ok()?, s.next()?.parse::<u32>().ok()?, s.next()?.parse::<u32>().ok()?))
        })() else {
            continue;
        };
        let folger = z.contains(" + ");
        if !folger {
            treiber = (quantum > 0 && rate > 0).then_some((quantum, rate));
        }
        if id == knoten {
            treffer = if folger { treiber } else { treiber.or(treffer) };
        }
    }
    treffer
}

// Die Grenze, die fuer einen Puffer in die Datei kommt: auf die naechste Zweierpotenz
// aufgerundet (PipeWire rundet sonst ab, s. aus_top), nie unter clock.min-quantum.
pub fn grenze_fuer(puffer: u32, min_quantum: Option<u32>) -> u32 {
    puffer.next_power_of_two().max(min_quantum.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grenze_kommt_aus_der_eigenen_datei_zurueck() {
        assert_eq!(parse_grenze(&conf_text(48, 48000)), Some(48));
        assert_eq!(parse_grenze("pulse.properties = {}\n"), None);
    }

    #[test]
    fn pw_top_letzter_durchlauf_zaehlt() {
        let top = "S   ID  QUANT   RATE    WAIT\n\
                   C   89      0      0    ---\n\
                   R   51     32  48000  10,6us   S32LE 2 48000 alsa_output\n\
                   R   89     48  48000   6,6us   F32LE 2 48000  + ALSA plug-in\n";
        assert_eq!(aus_top(top, 89), Some((32, 48000)), "der Treiber zaehlt, nicht der Wunsch des Knotens");
        assert_eq!(aus_top(top, 7), None);
    }

    #[test]
    fn grenze_auf_zweierpotenz() {
        assert_eq!(grenze_fuer(48, Some(32)), 64);
        assert_eq!(grenze_fuer(64, Some(32)), 64);
        assert_eq!(grenze_fuer(16, Some(32)), 32);
    }

    #[test]
    fn eigener_knoten_aus_pw_dump() {
        let dump: serde_json::Value = serde_json::from_str(
            r#"[
            {"id": 30, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "settings"},
             "metadata": [{"subject": 0, "key": "clock.min-quantum", "type": "", "value": 32}]},
            {"id": 65, "info": {"props": {"application.process.id": 5, "media.class": "Stream/Output/Audio"}}},
            {"id": 88, "info": {"props": {"application.process.id": 7, "media.class": "Stream/Input/Audio"}}},
            {"id": 89, "info": {"props": {"application.process.id": 7, "media.class": "Stream/Output/Audio"}}}
        ]"#,
        )
        .unwrap();
        assert_eq!(aus_dump(&dump, 7), Some((89, Some(32))));
    }
}
