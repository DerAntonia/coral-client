// Zips im Skin-Ordner von selbst entpacken: wer ein heruntergeladenes Pack in den Ordner
// legt, soll es ohne weiteren Schritt in der Skin-Liste finden. Das Zip wandert danach in
// den Papierkorb (nicht geloescht -- wer es doch noch braucht, holt es zurueck).
//
// Der Ordner wird einmal pro Sekunde angesehen, aber nur, solange der Skin-Tab offen ist
// (s. Aufruf in main.rs; ohne Ordner ruht die Suche und ein laufendes Entpacken laeuft zu
// Ende). Ein Zip wird erst angefasst, wenn Groesse
// und Aenderungszeit zwei Blicke lang gleich geblieben sind: ein Browser oder Dateimanager,
// der gerade noch schreibt, wuerde sonst ein halbes Archiv liefern. Entpackt wird in einem
// eigenen Thread, immer nur ein Zip zur Zeit.
//
// Ein Zip, das kein Pack ist oder sich nicht in den Papierkorb legen laesst, bleibt liegen
// und wird in dieser Sitzung nicht noch einmal versucht (sonst entstuende jede Sekunde eine
// weitere Kopie bzw. Meldung).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

use crate::notifications::Notification;

const SCAN_INTERVAL: Duration = Duration::from_secs(1);

type Stamp = (u64, Option<SystemTime>);

pub struct Done {
    pub zip: String,
    // Ordnername des entpackten Packs, oder warum es nicht ging.
    pub result: Result<String, String>,
    // Fehler beim Verschieben in den Papierkorb (das Pack ist dann trotzdem da).
    pub trash_error: Option<String>,
}

impl Done {
    pub fn notification(&self) -> Notification {
        match (&self.result, &self.trash_error) {
            (Ok(name), None) => Notification::success("Skin entpackt", format!("{name} -- das Zip liegt im Papierkorb.")),
            (Ok(name), Some(e)) => Notification::warning("Skin entpackt", format!("{name}, aber das Zip blieb liegen: {e}")),
            (Err(e), _) => Notification::warning("Zip nicht entpackt", format!("{}: {e}", self.zip)),
        }
    }
}

#[derive(Default)]
pub struct SkinZips {
    last_scan: Option<Instant>,
    seen: HashMap<PathBuf, Stamp>,
    tried: HashSet<(PathBuf, Stamp)>,
    job: Option<Receiver<Done>>,
}

impl SkinZips {
    // Einmal pro Frame aufrufen; liefert ein fertiges Ergebnis, sobald eines da ist.
    pub fn poll(&mut self, dir: Option<&Path>) -> Option<Done> {
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok(done) => {
                    self.job = None;
                    return Some(done);
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => self.job = None,
            }
        }
        let now = Instant::now();
        if self.last_scan.is_some_and(|t| now.duration_since(t) < SCAN_INTERVAL) {
            return None;
        }
        let Some(dir) = dir else {
            // Tab zu: beim naechsten Oeffnen wieder zwei Blicke abwarten.
            self.seen.clear();
            return None;
        };
        self.last_scan = Some(now);
        let ready = self.scan(dir);
        if let Some((zip, stamp)) = ready {
            self.tried.insert((zip.clone(), stamp));
            self.start(zip, dir.to_path_buf());
        }
        None
    }

    // Merkt sich die Zips des Ordners und gibt das erste zurueck, das seit dem letzten Blick
    // unveraendert ist und noch nicht versucht wurde.
    fn scan(&mut self, dir: &Path) -> Option<(PathBuf, Stamp)> {
        let mut now_seen = HashMap::new();
        let mut ready = None;
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if !is_zip(&path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let stamp = (meta.len(), meta.modified().ok());
            if ready.is_none() && self.seen.get(&path) == Some(&stamp) && !self.tried.contains(&(path.clone(), stamp)) {
                ready = Some((path.clone(), stamp));
            }
            now_seen.insert(path, stamp);
        }
        self.seen = now_seen;
        ready
    }

    fn start(&mut self, zip: PathBuf, dir: PathBuf) {
        let (tx, rx) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new().name("skin-zip".into()).spawn(move || {
            let name = zip.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let result = assets::extract_resource_pack(&zip, &dir)
                .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let trash_error = if result.is_ok() { move_to_trash(&zip).err() } else { None };
            let _ = tx.send(Done { zip: name, result, trash_error });
        });
        if started.is_ok() {
            self.job = Some(rx);
        }
    }
}

fn is_zip(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

// In den Papierkorb des Systems, nicht loeschen.
fn move_to_trash(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("powershell");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($env:CORAL_TRASH, 'OnlyErrorDialogs', 'SendToRecycleBin')",
        ]);
        c.env("CORAL_TRASH", path);
        // Kein Konsolenfenster aufblitzen lassen (CREATE_NO_WINDOW).
        std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("gio");
        c.arg("trash").arg(path);
        c
    };
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() && !path.exists() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if msg.is_empty() { format!("Papierkorb meldet {}", out.status) } else { msg })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erst_nach_zwei_gleichen_blicken_und_nur_einmal() {
        let d = std::env::temp_dir().join(format!("coral-client-skin-zips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let zip = d.join("Pack.ZIP");
        std::fs::write(&zip, b"abc").unwrap();
        std::fs::write(d.join("notiz.txt"), b"x").unwrap();
        let mut s = SkinZips::default();
        assert_eq!(s.scan(&d), None, "erster Blick: noch nicht stabil");
        std::fs::write(&zip, b"abcd").unwrap();
        assert_eq!(s.scan(&d), None, "gewachsen: wird noch geschrieben");
        let bereit = s.scan(&d).expect("unveraendert: bereit");
        assert_eq!(bereit.0, zip);
        s.tried.insert(bereit);
        assert_eq!(s.scan(&d), None, "schon versucht");
        let _ = std::fs::remove_dir_all(&d);
    }
}
