// Woher die Spieldateien kommen: offizielle .minecraft-Installation oder eine Prism-Launcher-
// Instanz. Geteilt zwischen Client (liest die Auswahl beim Start) und Launcher (zeigt die
// Kandidaten an und speichert die Auswahl) -- deshalb hier und nicht in einem der beiden.
//
// Die beiden Layouts unterscheiden sich nur darin, WO drei Dinge liegen:
//
//   | was          | offiziell                         | Prism                                                     |
//   |--------------|-----------------------------------|-----------------------------------------------------------|
//   | Client-Jar   | versions/<v>/<v>.jar              | libraries/com/mojang/minecraft/<v>/minecraft-<v>-client.jar |
//   | Version-JSON | versions/<v>/<v>.json             | meta/net.minecraft/<v>.json                                |
//   | Assets       | assets/{indexes,objects}          | assets/{indexes,objects} (im Prism-Datenordner, nicht in der Instanz) |
//
// Prism teilt Jar und Assets zwischen allen Instanzen; die Instanz bestimmt nur, WELCHE
// Version benutzt wird. Eine Instanz mit einer anderen Minecraft-Version ist deshalb schlicht
// unbrauchbar, nicht "halb" brauchbar.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::installation::AssetsError;
use crate::locate::locate_minecraft_dir;

/// Eine auswaehlbare Quelle der Spieldateien.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameSource {
    /// Ordner im Layout des offiziellen Launchers (.minecraft oder ein Ordner, der genauso
    /// aufgebaut ist).
    Official { dir: PathBuf },
    /// Instanz im Prism-Launcher. `root` ist Prisms Datenordner (enthaelt libraries/, meta/,
    /// assets/), `instance` der Ordnername unter instances/.
    Prism { root: PathBuf, instance: String },
}

/// Die drei Dinge, die der Client aus einer Installation braucht -- unabhaengig vom Layout.
#[derive(Clone, Debug)]
pub struct GameFiles {
    /// Enthaelt indexes/ und objects/.
    pub assets_dir: PathBuf,
    pub client_jar: PathBuf,
    /// Dateiname (ohne .json) unter assets_dir/indexes/.
    pub asset_index: String,
}

/// Ein gefundener Kandidat fuer die Auswahlliste im Launcher.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub source: GameSource,
    /// Anzeigename, z.B. "Offizieller Launcher" oder "Prism: PvP".
    pub label: String,
    /// Minecraft-Version der Quelle, falls bekannt (bei Prism aus der Instanz, beim offiziellen
    /// Launcher die gesuchte, falls installiert).
    pub version: Option<String>,
    /// Ok = fuer die gesuchte Version benutzbar, Err = Grund, warum nicht.
    pub files: Result<GameFiles, String>,
}

#[derive(Deserialize)]
struct VersionJson {
    #[serde(rename = "assetIndex")]
    asset_index: AssetIndexRef,
}

#[derive(Deserialize)]
struct AssetIndexRef {
    id: String,
}

#[derive(Deserialize)]
struct MmcPack {
    components: Vec<MmcComponent>,
}

#[derive(Deserialize)]
struct MmcComponent {
    uid: String,
    version: Option<String>,
}

/// Was es braucht, um Klassen aus dem Client-Jar mit Java zu laden (s. entity_models).
#[derive(Clone, Debug)]
pub struct JavaLaunch {
    pub version_json: PathBuf,
    pub client_jar: PathBuf,
    pub libraries_dir: PathBuf,
    /// Ordner, in denen der Launcher seine Java-Laufzeiten ablegt (offiziell: runtime/,
    /// Prism: java/).
    pub runtime_dirs: Vec<PathBuf>,
}

// Der offizielle Launcher legt seine Java-Laufzeiten unter Windows meist NICHT in
// .minecraft/runtime ab: der aktuelle Installer (Xbox-App/Store-Paket) nimmt seinen
// Paketordner unter %LOCALAPPDATA%, der alte MSI-Launcher seinen Programmordner.
fn official_runtime_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![dir.join("runtime")];
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(local.join("Packages").join("Microsoft.4297127D64EC6_8wekyb3d8bbwe").join("LocalCache").join("Local").join("runtime"));
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(programme) = std::env::var_os(var).map(PathBuf::from) {
                dirs.push(programme.join("Minecraft Launcher").join("runtime"));
            }
        }
    }
    dirs
}

impl GameSource {
    /// Pfade fuer einen Java-Start gegen das Client-Jar von `version`.
    pub fn java_launch(&self, version: &str) -> JavaLaunch {
        match self {
            GameSource::Official { dir } => {
                let version_dir = dir.join("versions").join(version);
                JavaLaunch {
                    version_json: version_dir.join(format!("{version}.json")),
                    client_jar: version_dir.join(format!("{version}.jar")),
                    libraries_dir: dir.join("libraries"),
                    runtime_dirs: official_runtime_dirs(dir),
                }
            }
            GameSource::Prism { root, .. } => JavaLaunch {
                version_json: root.join("meta").join("net.minecraft").join(format!("{version}.json")),
                client_jar: root.join("libraries").join("com/mojang/minecraft").join(version).join(format!("minecraft-{version}-client.jar")),
                libraries_dir: root.join("libraries"),
                runtime_dirs: vec![root.join("java")],
            },
        }
    }

    /// Pfade der Spieldateien fuer `version` -- prueft, dass alles, was der Client beim Start
    /// liest, auch wirklich da ist, damit ein Fehler im Launcher auffaellt und nicht erst
    /// beim Spielstart.
    pub fn resolve(&self, version: &str) -> Result<GameFiles, AssetsError> {
        let (version_json_path, client_jar, assets_dir) = match self {
            GameSource::Official { dir } => {
                let version_dir = dir.join("versions").join(version);
                (version_dir.join(format!("{version}.json")), version_dir.join(format!("{version}.jar")), dir.join("assets"))
            }
            GameSource::Prism { root, instance } => {
                let instance_version = prism_instance_version(&prism_instances_dir(root).join(instance));
                if instance_version.as_deref() != Some(version) {
                    return Err(AssetsError::VersionNotFound {
                        version: version.to_string(),
                        path: prism_instances_dir(root).join(instance).join("mmc-pack.json"),
                    });
                }
                (
                    root.join("meta").join("net.minecraft").join(format!("{version}.json")),
                    root.join("libraries").join("com/mojang/minecraft").join(version).join(format!("minecraft-{version}-client.jar")),
                    root.join("assets"),
                )
            }
        };

        let version_json_bytes = std::fs::read(&version_json_path)
            .map_err(|_| AssetsError::VersionNotFound { version: version.to_string(), path: version_json_path.clone() })?;
        let version_json: VersionJson = serde_json::from_slice(&version_json_bytes)?;
        let asset_index = version_json.asset_index.id;

        if !client_jar.is_file() {
            return Err(AssetsError::VersionNotFound { version: version.to_string(), path: client_jar });
        }
        let index_path = assets_dir.join("indexes").join(format!("{asset_index}.json"));
        if !index_path.is_file() {
            return Err(AssetsError::AssetIndexNotFound { index_id: asset_index, path: index_path });
        }

        Ok(GameFiles { assets_dir, client_jar, asset_index })
    }

    /// Einzeilige Beschreibung fuer Anzeige und Log.
    pub fn describe(&self) -> String {
        match self {
            GameSource::Official { dir } => format!("offizielles Layout in {}", dir.display()),
            GameSource::Prism { root, instance } => format!("Prism-Instanz \"{instance}\" in {}", root.display()),
        }
    }

    fn serialize(&self) -> String {
        match self {
            GameSource::Official { dir } => format!("kind=official\ndir={}\n", dir.display()),
            GameSource::Prism { root, instance } => format!("kind=prism\nroot={}\ninstance={instance}\n", root.display()),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let mut kind = None;
        let mut dir = None;
        let mut root = None;
        let mut instance = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim().to_string();
            match key.trim() {
                "kind" => kind = Some(value),
                "dir" => dir = Some(PathBuf::from(value)),
                "root" => root = Some(PathBuf::from(value)),
                "instance" => instance = Some(value),
                _ => {}
            }
        }
        match kind.as_deref()? {
            "official" => Some(GameSource::Official { dir: dir? }),
            "prism" => Some(GameSource::Prism { root: root?, instance: instance? }),
            _ => None,
        }
    }
}

// Ordnername des Clients in allen drei Basisordnern. Bis 2026-10 hiess der Client mc-client;
// LEGACY_DIR_NAME ist der alte Name, den migrate_legacy_dirs einmalig umbenennt.
pub const DIR_NAME: &str = "coral-client";
const LEGACY_DIR_NAME: &str = "mc-client";

fn config_base() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        // Windows: %APPDATA%.
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
}

fn data_base() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        // Windows: %APPDATA%.
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
}

fn home_base() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Konfigurationsordner des Clients (~/.config/coral-client bzw. %APPDATA%\coral-client) --
/// derselbe, in dem settings.conf liegt.
pub fn config_dir() -> Option<PathBuf> {
    Some(config_base()?.join(DIR_NAME))
}

/// Datenordner des Clients (~/.local/share/coral-client bzw. %APPDATA%\coral-client) -- fuer
/// Dateien, die der Client selbst sammelt und die sich nicht neu erzeugen lassen (gespeicherte
/// Chunks des Moduls Chunk-Saver).
pub fn data_dir() -> Option<PathBuf> {
    Some(data_base()?.join(DIR_NAME))
}

/// Sichtbarer Ordner im Benutzerverzeichnis (~/coral-client bzw. %USERPROFILE%\coral-client) --
/// fuer alles, was man selbst anfasst: Skin-Packs in skins/, das Client-Programm des Launchers
/// in binary/.
pub fn user_dir() -> Option<PathBuf> {
    Some(home_base()?.join(DIR_NAME))
}

/// Einmaliger Umzug nach der Umbenennung von mc-client zu Coral Client: jeder alte Ordner wird
/// umbenannt, solange es den neuen noch nicht gibt. Gibt es beide, bleibt alles, wie es ist --
/// dann hat schon ein neuer Start Daten angelegt, und Zusammenfuehren waere Raten. Launcher und
/// Client rufen das als Erstes auf; der zweite Aufruf findet nichts mehr zu tun.
pub fn migrate_legacy_dirs() {
    for base in [config_base(), data_base(), home_base()].into_iter().flatten() {
        let (alt, neu) = (base.join(LEGACY_DIR_NAME), base.join(DIR_NAME));
        if alt.is_dir() && !neu.exists() {
            if let Err(e) = std::fs::rename(&alt, &neu) {
                eprintln!("[ordner] {} nicht nach {} umbenannt: {e}", alt.display(), neu.display());
            }
        }
    }
    // Das Client-Programm im Benutzerordner trug den alten Namen ebenfalls.
    if let Some(binary) = user_dir().map(|d| d.join("binary")) {
        let exe = std::env::consts::EXE_SUFFIX;
        let (alt, neu) = (binary.join(format!("{LEGACY_DIR_NAME}{exe}")), binary.join(format!("{DIR_NAME}{exe}")));
        if alt.is_file() && !neu.exists() {
            if let Err(e) = std::fs::rename(&alt, &neu) {
                eprintln!("[ordner] {} nicht nach {} umbenannt: {e}", alt.display(), neu.display());
            }
        }
    }
}

fn selection_path() -> Option<PathBuf> {
    Some(config_dir()?.join("installation.conf"))
}

/// Die im Launcher gewaehlte Quelle, None = nie gewaehlt (bzw. Datei unlesbar).
pub fn load_selection() -> Option<GameSource> {
    GameSource::parse(&std::fs::read_to_string(selection_path()?).ok()?)
}

pub fn save_selection(source: &GameSource) -> std::io::Result<()> {
    let path = selection_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "kein Konfigurationsordner"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, source.serialize())
}

/// Die Quelle, die der Client beim Start benutzt: CORAL_MINECRAFT_DIR (Tests/Sonderfaelle)
/// vor der Auswahl im Launcher vor der Standard-.minecraft.
pub fn active_source() -> Option<GameSource> {
    if let Ok(override_path) = std::env::var("CORAL_MINECRAFT_DIR") {
        return Some(GameSource::Official { dir: PathBuf::from(override_path) });
    }
    load_selection().or_else(|| locate_minecraft_dir().map(|dir| GameSource::Official { dir }))
}

/// Alle gefundenen Installationen, jeweils mit dem Ergebnis fuer `version`. Die offizielle
/// zuerst, danach die Prism-Instanzen alphabetisch.
pub fn discover(version: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    if let Some(dir) = locate_minecraft_dir() {
        let source = GameSource::Official { dir };
        let files = source.resolve(version).map_err(|e| e.to_string());
        let version = files.is_ok().then(|| version.to_string());
        candidates.push(Candidate { source, label: "Offizieller Launcher".into(), version, files });
    }

    for root in prism_roots() {
        let mut instances: Vec<(String, Option<String>)> = std::fs::read_dir(prism_instances_dir(&root))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().join("mmc-pack.json").is_file())
            .map(|entry| (entry.file_name().to_string_lossy().into_owned(), prism_instance_version(&entry.path())))
            .collect();
        instances.sort_by_key(|(name, _)| name.to_lowercase());

        for (instance, instance_version) in instances {
            let label = format!("Prism: {}", prism_instance_name(&prism_instances_dir(&root).join(&instance)).unwrap_or_else(|| instance.clone()));
            let source = GameSource::Prism { root: root.clone(), instance };
            let files = match instance_version.as_deref() {
                Some(v) if v == version => source.resolve(version).map_err(|e| e.to_string()),
                Some(v) => Err(format!("Instanz benutzt Minecraft {v}, gebraucht wird {version}")),
                None => Err("Minecraft-Version der Instanz unbekannt".into()),
            };
            candidates.push(Candidate { source, label, version: instance_version, files });
        }
    }

    candidates
}

/// Moegliche Datenordner des Prism-Launchers, nur die, die existieren.
fn prism_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let mut roots = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        if let Some(data_home) = data_home {
            roots.push(data_home.join("PrismLauncher"));
        }
        // Flatpak-Variante (Flathub), eigener Datenordner in der Sandbox.
        if let Some(home) = &home {
            roots.push(home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            roots.push(PathBuf::from(appdata).join("PrismLauncher"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = &home {
            roots.push(home.join("Library/Application Support/PrismLauncher"));
        }
    }

    // Auf Windows wird `home` oben nicht gebraucht.
    let _ = &home;
    roots.retain(|root| root.is_dir());
    roots
}

/// instances/ -- oder der in prismlauncher.cfg eingestellte Ordner (InstanceDir=, relativ zum
/// Datenordner oder absolut).
fn prism_instances_dir(root: &Path) -> PathBuf {
    let configured = std::fs::read_to_string(root.join("prismlauncher.cfg")).ok().and_then(|cfg| {
        cfg.lines().find_map(|line| line.strip_prefix("InstanceDir=").map(|v| v.trim().to_string())).filter(|v| !v.is_empty())
    });
    match configured {
        Some(dir) => root.join(dir),
        None => root.join("instances"),
    }
}

/// Minecraft-Version einer Instanz aus mmc-pack.json (Komponente net.minecraft).
fn prism_instance_version(instance_dir: &Path) -> Option<String> {
    let pack: MmcPack = serde_json::from_slice(&std::fs::read(instance_dir.join("mmc-pack.json")).ok()?).ok()?;
    pack.components.into_iter().find(|c| c.uid == "net.minecraft")?.version
}

/// Anzeigename aus instance.cfg (name=), der Ordnername kann davon abweichen.
fn prism_instance_name(instance_dir: &Path) -> Option<String> {
    let cfg = std::fs::read_to_string(instance_dir.join("instance.cfg")).ok()?;
    cfg.lines().find_map(|line| line.strip_prefix("name=").map(|v| v.trim().to_string())).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("coral-client-source-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: PathBuf, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn auswahl_ueberlebt_speichern_und_laden() {
        for source in [
            GameSource::Official { dir: PathBuf::from("/a/b/.minecraft") },
            GameSource::Prism { root: PathBuf::from("/x/PrismLauncher"), instance: "PvP 1.21".into() },
        ] {
            assert_eq!(GameSource::parse(&source.serialize()), Some(source));
        }
        assert_eq!(GameSource::parse("kind=unbekannt\n"), None);
        assert_eq!(GameSource::parse("kind=prism\nroot=/x\n"), None, "Instanz fehlt");
    }

    #[test]
    fn offizielles_layout_wird_aufgeloest() {
        let dir = temp_dir("official");
        write(dir.join("versions/1.21.11/1.21.11.json"), r#"{"assetIndex": {"id": "29"}}"#);
        write(dir.join("versions/1.21.11/1.21.11.jar"), "");
        write(dir.join("assets/indexes/29.json"), "{}");

        let files = GameSource::Official { dir: dir.clone() }.resolve("1.21.11").unwrap();
        assert_eq!(files.asset_index, "29");
        assert_eq!(files.assets_dir, dir.join("assets"));
        assert!(GameSource::Official { dir: dir.clone() }.resolve("1.8.9").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prism_layout_wird_aufgeloest_und_falsche_version_abgelehnt() {
        let root = temp_dir("prism");
        write(root.join("meta/net.minecraft/1.21.11.json"), r#"{"assetIndex": {"id": "29"}, "mainJar": {}}"#);
        write(root.join("libraries/com/mojang/minecraft/1.21.11/minecraft-1.21.11-client.jar"), "");
        write(root.join("assets/indexes/29.json"), "{}");
        let pack = |v: &str| format!(r#"{{"components": [{{"uid": "org.lwjgl3", "version": "3.3.3"}}, {{"uid": "net.minecraft", "version": "{v}"}}]}}"#);
        write(root.join("instances/pvp/mmc-pack.json"), &pack("1.21.11"));
        write(root.join("instances/alt/mmc-pack.json"), &pack("1.8.9"));

        let files = GameSource::Prism { root: root.clone(), instance: "pvp".into() }.resolve("1.21.11").unwrap();
        assert_eq!(files.client_jar, root.join("libraries/com/mojang/minecraft/1.21.11/minecraft-1.21.11-client.jar"));
        assert!(GameSource::Prism { root: root.clone(), instance: "alt".into() }.resolve("1.21.11").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
