// Updates ueber GitHub-Releases: neuestes Release abfragen, mit der installierten Version
// vergleichen, das Client-Binary fuer diese Plattform herunterladen.
//
// Erwartete Release-Form (s. scripts/release.sh): Tag "v<version>", Assets
// `coral-client-linux-x86_64` und `coral-client-windows-x86_64.exe`, dazu der Launcher selbst als
// `coral-client-launcher-linux-x86_64` bzw. `coral-client-launcher-windows-x86_64.exe`.
//
// Jede Datei wird gegen Groesse und SHA-256 geprueft, die GitHub zum Asset angibt (Feld
// "digest", "sha256:<hex>"), und nur bei Uebereinstimmung eingesetzt. Das faengt kaputte und
// unvollstaendige Downloads ab -- nicht aber jemanden mit Zugriff aufs Repo, der Datei und
// Pruefsumme gemeinsam austauschen koennte (dafuer braeuchte es eine Signatur).
//
// Das Repo ist oeffentlich: Release-Liste und Downloads gehen ohne Anmeldung.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPO: &str = "DerAntonia/coral-client";

#[cfg(windows)]
pub const ASSET_NAME: &str = "coral-client-windows-x86_64.exe";
#[cfg(not(windows))]
pub const ASSET_NAME: &str = "coral-client-linux-x86_64";

#[cfg(windows)]
pub const LAUNCHER_ASSET_NAME: &str = "coral-client-launcher-windows-x86_64.exe";
#[cfg(not(windows))]
pub const LAUNCHER_ASSET_NAME: &str = "coral-client-launcher-linux-x86_64";

#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
    // Auf GitHub als "Pre-release" markiert = Beta (s. scripts/release.sh: Tags mit "-").
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    // Direkter Download-Link. Zaehlt anders als die API-URL des Assets nicht gegen GitHubs
    // Anfragegrenze ohne Anmeldung (60 je Stunde).
    pub browser_download_url: String,
    pub size: u64,
    // "sha256:<hex>" -- fehlt bei Releases, die GitHub vor Einfuehrung des Felds angelegt hat.
    #[serde(default)]
    pub digest: Option<String>,
}

impl ReleaseAsset {
    /// Die erwartete SHA-256 als Kleinbuchstaben-Hex, falls GitHub eine nennt.
    pub fn sha256(&self) -> Option<String> {
        Some(self.digest.as_deref()?.strip_prefix("sha256:")?.to_ascii_lowercase())
    }
}

/// SHA-256 einer Datei als Hex. Fuer die Pruefung, ob der vorhandene Client zum Release passt.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Release {
    pub fn asset_for_platform(&self) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| a.name == ASSET_NAME)
    }

    pub fn launcher_asset(&self) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| a.name == LAUNCHER_ASSET_NAME)
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("coral-client-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// Alle veroeffentlichten Releases (ohne Entwuerfe), neueste Version zuerst -- stabile wie
/// Betas; welche davon angezeigt werden, entscheidet der Aufrufer (s. visible/newest). Leer =
/// das Repo hat noch keins.
pub fn fetch_releases() -> Result<Vec<Release>, String> {
    let response = client()?
        .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=50"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("GitHub nicht erreichbar: {e}"))?;
    match response.status().as_u16() {
        200 => {
            let mut releases: Vec<Release> = response.json().map_err(|e| format!("Antwort von GitHub unlesbar: {e}"))?;
            releases.retain(|r| !r.draft);
            releases.sort_by(|a, b| compare_versions(&b.tag_name, &a.tag_name));
            Ok(releases)
        }
        404 => Err(format!("{REPO} nicht gefunden.")),
        // Ohne Anmeldung erlaubt GitHub 60 API-Anfragen je Stunde und Adresse.
        403 | 429 => Err("GitHub-Anfragegrenze erreicht -- in einer Stunde nochmal versuchen.".to_string()),
        status => Err(format!("GitHub antwortet mit {status}")),
    }
}

/// Die Releases, die der Kanal zeigt: nur stabile, oder mit `beta` auch die Vorabversionen.
pub fn visible(releases: &[Release], beta: bool) -> impl Iterator<Item = &Release> {
    releases.iter().filter(move |r| beta || !r.prerelease)
}

/// Neueste Version im Kanal (die Liste ist schon absteigend sortiert, s. fetch_releases).
pub fn newest(releases: &[Release], beta: bool) -> Option<&Release> {
    visible(releases, beta).next()
}

/// Fortschritt eines Downloads, zwischen Download-Thread und Oberflaeche geteilt. Bei mehreren
/// Dateien nacheinander traegt der Aufrufer vorab die Gesamtgroesse ein; `done` zaehlt weiter.
#[derive(Default)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// Laedt `asset` nach `target`. Erst in eine Nachbardatei und dann umbenennen: ein
/// abgebrochener Download hinterlaesst so kein halbes Binary, und ein Hardlink auf `target`
/// (z.B. coral-client-linux/coral-client -> target/release/coral-client) wird nicht mit ueberschrieben.
pub fn download(asset: &ReleaseAsset, target: &Path, progress: Arc<Mutex<Progress>>) -> Result<(), String> {
    let mut response = client()?
        .get(&asset.browser_download_url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Download fehlgeschlagen: {e}"))?;
    {
        let mut p = progress.lock().unwrap();
        if p.total == 0 {
            p.total = response.content_length().unwrap_or(asset.size);
        }
    }

    let partial = partial_path(target);
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = std::fs::File::create(&partial)?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = response.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            written += n as u64;
            progress.lock().unwrap().done += n as u64;
        }
        file.sync_all()?;
        drop(file);
        // Erst pruefen, dann einsetzen: eine falsche Datei ersetzt nie die vorhandene.
        if written != asset.size {
            return Err(io_err(format!("{} ist {written} statt {} Byte gross", asset.name, asset.size)));
        }
        let actual = hex(&hasher.clone().finalize());
        if let Some(expected) = asset.sha256() {
            if actual != expected {
                return Err(io_err(format!("Pruefsumme von {} stimmt nicht (erwartet {}, bekommen {})", asset.name, &expected[..12], &actual[..12])));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))?;
        }
        replace_file(&partial, target)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("{} nicht eingesetzt: {e}", target.display()));
    }
    Ok(())
}

// Die fertige Datei an ihren Platz. Unter Windows laesst sich eine laufende .exe weder
// ueberschreiben noch loeschen, wohl aber umbenennen: die alte wird zu "<name>.old" (raeumt
// cleanup_old_files beim naechsten Start weg). Unter Linux ersetzt rename auch eine laufende
// Datei -- das laufende Programm behaelt seine alte Fassung, bis es neu startet.
fn replace_file(new: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    if target.exists() {
        let old = sibling(target, ".old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(target, &old)?;
    }
    std::fs::rename(new, target)
}

/// Reste eines frueheren Selbst-Updates entfernen (s. replace_file).
pub fn cleanup_old_files(exe: &Path) {
    let _ = std::fs::remove_file(sibling(exe, ".old"));
}

fn sibling(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    target.with_file_name(name)
}

fn io_err(msg: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

fn partial_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".download");
    target.with_file_name(name)
}

/// Versionsnummern aus Tags wie "v0.2.10" -- Zahlen statt Text vergleichen, sonst waere
/// "0.10" kleiner als "0.9". Ein Vorabteil ("-beta.2") gehoert NICHT dazu, s. compare_versions.
pub fn parse_version(tag: &str) -> Vec<u64> {
    let kern = tag.trim().trim_start_matches(['v', 'V']);
    let kern = kern.split(['-', '+']).next().unwrap_or("");
    kern.split('.').map_while(|part| part.parse().ok()).collect()
}

// Der Vorabteil nach dem ersten "-" (ohne Build-Angabe nach "+"), z.B. "beta.2".
fn prerelease_part(tag: &str) -> Option<&str> {
    let ohne_build = tag.trim().split('+').next().unwrap_or("");
    ohne_build.split_once('-').map(|(_, pre)| pre)
}

/// Vergleich wie bei SemVer: erst die Zahlen; bei gleichen Zahlen ist die Vorabversion aelter
/// als die fertige (0.3.0-beta.1 < 0.3.0), und Vorabversionen untereinander werden Teil fuer
/// Teil verglichen -- Zahlen als Zahlen (beta.10 > beta.9), Woerter als Text (alpha < beta).
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    parse_version(a).cmp(&parse_version(b)).then_with(|| match (prerelease_part(a), prerelease_part(b)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let (mut xs, mut ys) = (x.split('.'), y.split('.'));
            loop {
                match (xs.next(), ys.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(p), Some(q)) => {
                        let o = match (p.parse::<u64>(), q.parse::<u64>()) {
                            (Ok(m), Ok(n)) => m.cmp(&n),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => p.cmp(q),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                }
            }
        }
    })
}

pub fn is_newer(candidate: &str, installed: &str) -> bool {
    compare_versions(candidate, installed) == std::cmp::Ordering::Greater
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruefsumme_wie_sha256sum() {
        let path = std::env::temp_dir().join(format!("mc-launcher-hash-{}", std::process::id()));
        std::fs::write(&path, "abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_file(&path);
        let asset = ReleaseAsset { name: "x".into(), browser_download_url: String::new(), size: 3, digest: Some("sha256:BA78".into()) };
        assert_eq!(asset.sha256().as_deref(), Some("ba78"));
        let ohne = ReleaseAsset { digest: None, ..asset };
        assert_eq!(ohne.sha256(), None);
    }

    #[test]
    fn datei_wird_ersetzt_und_reste_aufgeraeumt() {
        let dir = std::env::temp_dir().join(format!("mc-launcher-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("launcher");
        let new = dir.join("launcher.download");
        std::fs::write(&target, "alt").unwrap();
        std::fs::write(&new, "neu").unwrap();
        replace_file(&new, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "neu");
        assert!(!new.exists());
        cleanup_old_files(&target);
        assert!(!sibling(&target, ".old").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versionen_werden_als_zahlen_verglichen() {
        assert!(is_newer("v0.10.0", "v0.9.3"));
        assert!(is_newer("v1.0", "0.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "v0.2.0"));
        assert_eq!(parse_version("v1.2.3-beta"), vec![1, 2, 3]);
    }

    #[test]
    fn betas_sind_aelter_als_die_fertige_version() {
        assert!(is_newer("v0.3.0", "v0.3.0-beta.1"));
        assert!(!is_newer("v0.3.0-beta.1", "v0.3.0"));
        assert!(is_newer("v0.3.0-beta.1", "v0.2.0"));
        assert!(is_newer("v0.3.0-beta.10", "v0.3.0-beta.9"));
        assert!(is_newer("v0.3.0-beta.1", "v0.3.0-alpha.3"));
        assert!(!is_newer("v0.3.0-beta.1", "v0.3.0-beta.1"));
    }

    #[test]
    fn beta_kanal_zeigt_vorabversionen() {
        let r = |tag: &str, prerelease: bool| Release {
            tag_name: tag.into(),
            name: None,
            body: None,
            html_url: String::new(),
            assets: Vec::new(),
            prerelease,
            draft: false,
        };
        let liste = vec![r("v0.4.0-beta.1", true), r("v0.3.0", false), r("v0.3.0-beta.2", true)];
        assert_eq!(newest(&liste, false).map(|r| r.tag_name.as_str()), Some("v0.3.0"));
        assert_eq!(newest(&liste, true).map(|r| r.tag_name.as_str()), Some("v0.4.0-beta.1"));
        assert_eq!(visible(&liste, false).count(), 1);
    }
}
