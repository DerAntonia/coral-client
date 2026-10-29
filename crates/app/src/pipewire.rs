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
// So oft prueft "Audio automatisch optimieren" die Prioritaet von pipewire-pulse nach.
const AUTO_ABSTAND: Duration = Duration::from_secs(10);

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
    // Wie pipewire-pulse gerade eingeplant ist (s. prioritaet), None = nicht gefunden.
    pub pulse: Option<Prioritaet>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prioritaet {
    // Echtzeit-Prioritaet der Ton-Faeden (pw-data-loop), None = mindestens einer ohne Echtzeit.
    pub echtzeit: Option<u32>,
    // Nice des Haupt-Threads.
    pub nice: i32,
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
    // Fehlgeschlagene Messungen in Folge (s. messen_wenn_faellig).
    fehlversuche: Arc<std::sync::atomic::AtomicU32>,
    letzte: Option<Instant>,
    // Automatisch optimieren: fuer welche Zielgrenze zuletzt geprueft wurde, und wann.
    auto_ziel: Option<Option<u32>>,
    auto_letzte: Option<Instant>,
    // Der Ton-Faden des Clients, der zuletzt Echtzeit bekommen hat (s. client_echtzeit).
    client_faden: Option<u32>,
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
        if !cfg!(target_os = "linux") || self.letzte.is_some_and(|t| t.elapsed() < ABSTAND) || self.laeuft.swap(true, Ordering::Relaxed) {
            return;
        }
        self.letzte = Some(Instant::now());
        let (ziel, laeuft, fehlversuche) = (self.messung.clone(), self.laeuft.clone(), self.fehlversuche.clone());
        std::thread::spawn(move || {
            let m = messen(std::process::id());
            // Ab und zu liefert pw-top einen Durchlauf ganz ohne Werte (alles 0) -- eine einzelne
            // leere Messung ueberschreibt deshalb nicht die letzte gute, erst die dritte in Folge.
            let versuche = if m.is_some() { 0 } else { fehlversuche.load(Ordering::Relaxed) + 1 };
            fehlversuche.store(versuche, Ordering::Relaxed);
            if let Ok(mut z) = ziel.lock() {
                if m.is_some() || versuche >= 3 {
                    *z = m;
                }
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
            if ergebnis.is_ok() {
                neu.store(true, Ordering::Relaxed);
            }
            let ergebnis = ergebnis.and_then(|()| prioritaet_sichern());
            if let Ok(mut f) = fehler.lock() {
                *f = ergebnis.as_ref().err().cloned();
            }
            laeuft.store(false, Ordering::Relaxed);
        });
    }

    // "Audio automatisch optimieren": pipewire-pulse nur dann neu starten, wenn die eigene Grenze
    // fuer `ziel` (s. grenze_fuer) noch nicht stimmt -- der Neustart reisst allen Programmen kurz
    // den Ton ab. Fehlende Echtzeit oder Nice holt RTKit ohne Neustart nach (s.
    // prioritaet_sichern). Ohne pipewire-pulse geschieht nichts.
    pub fn auto_optimieren(&mut self, ziel: Option<u32>, rate: u32) {
        let faellig = self.auto_ziel != Some(ziel) || self.auto_letzte.is_none_or(|t| t.elapsed() >= AUTO_ABSTAND);
        if !cfg!(target_os = "linux") || !faellig || self.laeuft.swap(true, Ordering::Relaxed) {
            return;
        }
        self.auto_ziel = Some(ziel);
        self.auto_letzte = Some(Instant::now());
        let (laeuft, neu, fehler) = (self.laeuft.clone(), self.neu_oeffnen.clone(), self.fehler.clone());
        let neustart = ziel.is_some() && eigene_grenze() != ziel;
        if neustart {
            self.letzte = None;
        }
        std::thread::spawn(move || {
            let ergebnis = if pulse_pid().is_none() {
                Ok(())
            } else if neustart {
                let r = schreiben(ziel, rate).and_then(|()| neu_starten());
                if r.is_ok() {
                    neu.store(true, Ordering::Relaxed);
                }
                r.and_then(|()| prioritaet_sichern())
            } else {
                prioritaet_sichern()
            };
            if let Ok(mut f) = fehler.lock() {
                *f = ergebnis.err();
            }
            laeuft.store(false, Ordering::Relaxed);
        });
    }

    // Den eigenen Ton-Faden (cpal-Callback) ueber RTKit in Echtzeit setzen, einmal je Faden --
    // ein neu geoeffneter Strom hat einen neuen. Ohne das laeuft er mit normaler Prioritaet, und
    // beim Start, wenn das Laden alle Kerne belegt, kam er bei 1 ms Puffer zu spaet: es knackte.
    pub fn client_echtzeit(&mut self, tid: u32) {
        if !cfg!(target_os = "linux") || tid == u32::MAX || self.client_faden == Some(tid) || self.laeuft.swap(true, Ordering::Relaxed) {
            return;
        }
        self.client_faden = Some(tid);
        let (laeuft, fehler) = (self.laeuft.clone(), self.fehler.clone());
        std::thread::spawn(move || {
            if let Err(e) = faden_echtzeit(tid) {
                if let Ok(mut f) = fehler.lock() {
                    *f = Some(e);
                }
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

// Nach mehreren Neustarts kurz hintereinander lief pipewire-pulse ohne Echtzeit und mit Nice 0
// (RTKit hat module-rt vermutlich abgewiesen) -- bei 64 Frames rauschte es dann ab und zu.
// Fehlt die Prioritaet, wird sie ueber RTKit nachgeholt, mit denselben Werten wie module-rt.
const RT_PRIO: u32 = 20;
const NICE: i32 = -11;

fn prioritaet_sichern() -> Result<(), String> {
    let pid = pulse_pid().ok_or("pipewire-pulse nicht gefunden")?;
    let faeden = faeden(pid);
    let haupt_nice = faeden.iter().find(|(tid, _)| *tid == pid).map(|(_, f)| f.nice);
    let ohne_echtzeit: Vec<u32> = faeden.iter().filter(|(_, f)| f.name == "pw-data-loop" && f.policy == 0).map(|(tid, _)| *tid).collect();
    let (pid_s, prio_s, nice_s) = (pid.to_string(), RT_PRIO.to_string(), NICE.to_string());
    for tid in ohne_echtzeit {
        rtkit(&["MakeThreadRealtimeWithPID", "ttu", &pid_s, &tid.to_string(), &prio_s])?;
    }
    if haupt_nice.is_some_and(|n| n > NICE) {
        rtkit(&["MakeThreadHighPriorityWithPID", "tti", &pid_s, &pid_s, "--", &nice_s])?;
    }
    Ok(())
}

fn rtkit(args: &[&str]) -> Result<(), String> {
    let mut alle = vec!["call", "--system", "org.freedesktop.RealtimeKit1", "/org/freedesktop/RealtimeKit1", "org.freedesktop.RealtimeKit1"];
    alle.extend_from_slice(args);
    let status = std::process::Command::new("busctl").args(&alle).stdout(std::process::Stdio::null()).status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("RTKit hat die Prioritaet abgelehnt".into())
    }
}

// Unter pipewire-pulse (RT_PRIO): der Client liefert ihm zu, nicht umgekehrt.
const CLIENT_RT_PRIO: u32 = 10;
// RTKit gibt Echtzeit nur Prozessen mit begrenzter RLIMIT_RTTIME (Vorgabe hoechstens 200 ms
// Rechenzeit am Stueck): ein haengender Echtzeit-Faden soll das System nicht einfrieren.
const RTTIME_US: u64 = 200_000;

#[cfg(target_os = "linux")]
fn faden_echtzeit(tid: u32) -> Result<(), String> {
    // SAFETY: getrlimit/setrlimit mit gueltigem Zeiger auf eine lokale Struktur.
    unsafe {
        let mut grenze: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_RTTIME, &mut grenze) == 0 && grenze.rlim_max > RTTIME_US {
            let neu = libc::rlimit { rlim_cur: RTTIME_US, rlim_max: RTTIME_US };
            libc::setrlimit(libc::RLIMIT_RTTIME, &neu);
        }
    }
    rtkit(&["MakeThreadRealtimeWithPID", "ttu", &std::process::id().to_string(), &tid.to_string(), &CLIENT_RT_PRIO.to_string()])
}

#[cfg(not(target_os = "linux"))]
fn faden_echtzeit(_tid: u32) -> Result<(), String> {
    Ok(())
}

fn pulse_pid() -> Option<u32> {
    ausgabe("systemctl", &["--user", "show", "-p", "MainPID", "--value", "pipewire-pulse.service"])?.trim().parse().ok().filter(|&p| p != 0)
}

fn faeden(pid: u32) -> Vec<(u32, Faden)> {
    let Ok(ordner) = std::fs::read_dir(format!("/proc/{pid}/task")) else { return Vec::new() };
    ordner
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|tid| Some((tid, aus_stat(&std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/stat")).ok()?)?)))
        .collect()
}

fn prioritaet(faeden: &[(u32, Faden)], pid: u32) -> Option<Prioritaet> {
    let nice = faeden.iter().find(|(tid, _)| *tid == pid)?.1.nice;
    let ton: Vec<&Faden> = faeden.iter().map(|(_, f)| f).filter(|f| f.name == "pw-data-loop").collect();
    let echtzeit = if !ton.is_empty() && ton.iter().all(|f| f.policy != 0) { ton.iter().map(|f| f.rt_prio).min() } else { None };
    Some(Prioritaet { echtzeit, nice })
}

struct Faden {
    name: String,
    nice: i32,
    rt_prio: u32,
    policy: u32,
}

// /proc/<pid>/task/<tid>/stat: Name in Klammern (darf Leerzeichen enthalten), danach ab Feld 3
// durchgezaehlt -- nice ist Feld 19, rt_priority Feld 40, policy Feld 41.
fn aus_stat(text: &str) -> Option<Faden> {
    let (start, ende) = (text.find('(')?, text.rfind(')')?);
    let felder: Vec<&str> = text[ende + 1..].split_whitespace().collect();
    Some(Faden { name: text[start + 1..ende].to_string(), nice: felder.get(19 - 3)?.parse().ok()?, rt_prio: felder.get(40 - 3)?.parse().ok()?, policy: felder.get(41 - 3)?.parse().ok()? })
}

fn ausgabe(programm: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(programm).args(args).stderr(std::process::Stdio::null()).output().ok()?;
    String::from_utf8(out.stdout).ok()
}

fn messen(pid: u32) -> Option<Messung> {
    let dump: serde_json::Value = serde_json::from_str(&ausgabe("pw-dump", &[])?).ok()?;
    let (knoten, min_quantum) = aus_dump(&dump, pid)?;
    let (quantum, rate) = aus_top(&ausgabe("pw-top", &["-b", "-n", "3"])?, knoten)?;
    let pulse = pulse_pid().and_then(|p| prioritaet(&faeden(p), p));
    Some(Messung { quantum, rate, min_quantum, eigene_grenze: eigene_grenze(), pulse })
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
            // Ein Durchlauf ohne Werte (Treiber noch bei 0) laesst den vorigen Treffer stehen.
            treffer = treiber.or(treffer);
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
    fn leerer_durchlauf_laesst_den_letzten_wert_stehen() {
        let top = "R   51     64  48000  10,6us   S32LE 2 48000 alsa_output\n\
                   R   87     64  48000   6,6us   F32LE 2 48000  + ALSA plug-in\n\
                   S   ID  QUANT   RATE    WAIT\n\
                   R   51      0      0   0,0us   S32LE 2 48000 alsa_output\n\
                   R   87      0      0   0,0us   F32LE 2 48000 ALSA plug-in\n";
        assert_eq!(aus_top(top, 87), Some((64, 48000)));
    }

    #[test]
    fn faden_aus_proc_stat() {
        let stat = "250252 (pw-data-loop) S 1652 250250 250250 0 -1 4194624 74 0 0 0 0 0 0 0 -21 0 3 0 2954398 112455680 3131 18446744073709551615 1 1 0 0 0 0 0 4096 0 0 0 0 -1 3 20 2 0 0 0 0 0 0 0 0 0 0 0\n";
        let f = aus_stat(stat).unwrap();
        assert_eq!((f.name.as_str(), f.nice, f.rt_prio, f.policy), ("pw-data-loop", 0, 20, 2));
    }

    #[test]
    fn prioritaet_braucht_echtzeit_in_allen_tonfaeden() {
        let faden = |name: &str, nice, rt_prio, policy| Faden { name: name.into(), nice, rt_prio, policy };
        let gut = [(1, faden("pipewire-pulse", -11, 0, 0)), (2, faden("module-rt", 0, 0, 0)), (3, faden("pw-data-loop", 0, 20, 2))];
        assert_eq!(prioritaet(&gut, 1), Some(Prioritaet { echtzeit: Some(20), nice: -11 }));
        let ohne = [(1, faden("pipewire-pulse", 0, 0, 0)), (3, faden("pw-data-loop", 0, 0, 0))];
        assert_eq!(prioritaet(&ohne, 1), Some(Prioritaet { echtzeit: None, nice: 0 }));
        assert_eq!(prioritaet(&ohne, 9), None);
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
