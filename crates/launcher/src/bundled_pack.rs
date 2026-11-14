//! Das Resourcepack "CPVP Aesthetic" (von DerAntonia, CC BY-NC-SA 4.0) kommt mit dem Client:
//! der Launcher holt bei jedem Start die neueste Version von Modrinth und entpackt sie als
//! Skin-Ordner. Bewusst von Modrinth und nicht aus dem eigenen Release, damit
//! der Download-Zaehler des Packs mitzaehlt.
//!
//! Welche Version liegt, steht in einer Datei im Pack-Ordner; nur bei einer anderen Version wird
//! neu geladen. Ohne Netz bleibt das vorhandene Pack einfach stehen.

use std::io::{Read, Write};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha512};

const PROJECT: &str = "cpvp-aesthetic";
/// Ordnername unter ~/coral-client/skins.
const FOLDER: &str = "CPVP Aesthetic";
/// Im Pack-Ordner: die Modrinth-ID der entpackten Version.
const STAMP: &str = ".modrinth-version";

#[derive(Deserialize)]
struct Version {
    id: String,
    date_published: String,
    files: Vec<VersionFile>,
}

#[derive(Deserialize)]
struct VersionFile {
    url: String,
    size: u64,
    primary: bool,
    hashes: Hashes,
}

#[derive(Deserialize)]
struct Hashes {
    sha512: Option<String>,
}

/// Laedt das Pack, falls Modrinth eine andere Version als die entpackte hat. Ok(true): neu
/// entpackt, Ok(false): schon aktuell.
pub fn update() -> Result<bool, String> {
    let skins = assets::user_dir().ok_or("kein Benutzerordner")?.join("skins");
    let ziel = skins.join(FOLDER);
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("coral-client-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut versions: Vec<Version> = client
        .get(format!("https://api.modrinth.com/v2/project/{PROJECT}/version"))
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::json)
        .map_err(|e| format!("Modrinth: {e}"))?;
    // ISO-8601 mit gleicher Laenge: als Text sortiert = nach Zeit sortiert.
    versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    let neueste = versions.first().ok_or("keine Version auf Modrinth")?;
    let vorhanden = std::fs::read_to_string(ziel.join(STAMP)).unwrap_or_default();
    if vorhanden.trim() == neueste.id && ziel.join("assets").is_dir() {
        return Ok(false);
    }
    let datei = neueste.files.iter().find(|f| f.primary).or_else(|| neueste.files.first()).ok_or("Version ohne Datei")?;
    std::fs::create_dir_all(&skins).map_err(|e| format!("{}: {e}", skins.display()))?;
    // Nicht auf .zip enden: sonst entpackt der Client die halbe Datei selbst (s. skin_zips).
    let zip = skins.join(format!(".{FOLDER}.download"));
    let ergebnis = download(&client, datei, &zip)
        .and_then(|()| assets::replace_resource_pack(&zip, &ziel, &|_| false))
        .and_then(|()| std::fs::write(ziel.join(STAMP), &neueste.id).map_err(|e| format!("{STAMP}: {e}")));
    let _ = std::fs::remove_file(&zip);
    ergebnis.map(|()| true)
}

fn download(client: &reqwest::blocking::Client, datei: &VersionFile, ziel: &Path) -> Result<(), String> {
    let mut response = client
        .get(&datei.url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("Download: {e}"))?;
    let mut file = std::fs::File::create(ziel).map_err(|e| format!("{}: {e}", ziel.display()))?;
    let mut hasher = Sha512::new();
    let mut written = 0u64;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = response.read(&mut buf).map_err(|e| format!("Download: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", ziel.display()))?;
        hasher.update(&buf[..n]);
        written += n as u64;
    }
    if written != datei.size {
        return Err(format!("{written} statt {} Byte geladen", datei.size));
    }
    if let Some(erwartet) = &datei.hashes.sha512 {
        let ist: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !ist.eq_ignore_ascii_case(erwartet) {
            return Err("Pruefsumme stimmt nicht".to_string());
        }
    }
    Ok(())
}
