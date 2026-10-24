// Spieldaten aus dem Client-Jar der Installation, einmal extrahiert in einen festen Ordner
// (~/.local/share/coral-client/storage/<version>/). Manches kennt das Jar nur als Java-Code --
// die Geometrie der Entity-Modelle, die Inhalte der Kreativ-Tabs, die Standard-Components der
// Items. Dafuer laeuft je ein kleines Java-Programm (EntityModelDump.java, CreativeTabsDump.java,
// ItemComponentsDump.java) mit dem Java, das der Launcher fuer
// die Version mitbringt, und schreibt Zahlen bzw. Text. Im Repo liegt davon nichts.
//
// Extrahiert wird im LAUNCHER (beim ersten Start von selbst, sonst auf Knopfdruck, z.B. nach
// einem Update der Version durch Mojang). Der Client liest nur noch die fertigen Dateien und
// startet nie selbst Java. Eine Stempeldatei haelt fest, aus welchem Jar und mit welchen
// Dumpern extrahiert wurde -- daran erkennt game_data_status, ob die Daten noch passen.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::installation::AssetsError;
use crate::source::{active_source, data_dir, JavaLaunch};

// Die Dumper: (Dateiname im Speicher, Quelldatei, Quelltext).
const DUMPS: [(&str, &str, &str); 3] = [
    ("entity_models.dat", "EntityModelDump.java", include_str!("EntityModelDump.java")),
    ("creative_tabs.dat", "CreativeTabsDump.java", include_str!("CreativeTabsDump.java")),
    ("item_components.dat", "ItemComponentsDump.java", include_str!("ItemComponentsDump.java")),
];

const STAMP: &str = "stamp.conf";

#[derive(Deserialize)]
struct VersionJson {
    #[serde(default)]
    libraries: Vec<Library>,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersion>,
    // Prism-Meta: statt javaVersion.component steht der Laufzeitname hier.
    #[serde(rename = "compatibleJavaName")]
    compatible_java_name: Option<String>,
}

#[derive(Deserialize)]
struct Library {
    name: Option<String>,
    downloads: Option<Downloads>,
}

#[derive(Deserialize)]
struct Downloads {
    artifact: Option<Artifact>,
}

// Die Mojang-Versions-JSON nennt den Pfad, die Prism-Meta nicht -- dort steht nur die
// Maven-Koordinate im `name` der Bibliothek.
#[derive(Deserialize)]
struct Artifact {
    path: Option<String>,
}

// "group:artifact:version[:classifier]" -> "group/mit/slashes/artifact/version/artifact-version[-classifier].jar"
fn maven_path(name: &str) -> Option<String> {
    let mut teile = name.split(':');
    let (gruppe, artefakt, version) = (teile.next()?, teile.next()?, teile.next()?);
    let klassifizierer = teile.next().map(|k| format!("-{k}")).unwrap_or_default();
    Some(format!("{}/{artefakt}/{version}/{artefakt}-{version}{klassifizierer}.jar", gruppe.replace('.', "/")))
}

#[derive(Deserialize)]
struct JavaVersion {
    component: Option<String>,
}

// FNV-1a: Kennung eines Dumper-Quelltexts im Stempel.
fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

// Das Java der Installation: die Laufzeit, die der Launcher fuer diese Version vorsieht
// (javaVersion.component), dann irgendeine andere seiner Laufzeiten, dann JAVA_HOME, zuletzt
// `java` aus dem Suchpfad.
fn find_java(launch: &JavaLaunch, component: Option<&str>) -> PathBuf {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut kandidaten: Vec<PathBuf> = Vec::new();
    for dir in &launch.runtime_dirs {
        let Ok(eintraege) = std::fs::read_dir(dir) else { continue };
        let mut eintraege: Vec<PathBuf> = eintraege.flatten().map(|e| e.path()).collect();
        // Die passende Laufzeit zuerst.
        eintraege.sort_by_key(|p| component.is_none_or(|c| p.file_name().and_then(|n| n.to_str()) != Some(c)));
        for laufzeit in eintraege {
            // offiziell: runtime/<komp>/<os>/<komp>/bin/java; Prism: java/<name>/bin/java
            kandidaten.push(laufzeit.join("bin").join(exe));
            if let Ok(os) = std::fs::read_dir(&laufzeit) {
                for os in os.flatten() {
                    if let Some(name) = laufzeit.file_name() {
                        kandidaten.push(os.path().join(name).join("bin").join(exe));
                    }
                }
            }
        }
    }
    // Sicherheitsnetz, falls ein Launcher seine Laufzeiten woanders ablegt: das Java eines
    // gerade laufenden Minecraft. Neben javaw (ohne Konsole) liegt immer auch java.
    for laufend in running_java() {
        if let Some(bin) = laufend.parent() {
            kandidaten.push(bin.join(exe));
        }
        kandidaten.push(laufend);
    }
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        kandidaten.push(PathBuf::from(home).join("bin").join(exe));
    }
    kandidaten.into_iter().find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(exe))
}

// Programmdateien laufender Java-Prozesse, die Minecraft ausfuehren.
#[cfg(target_os = "linux")]
fn running_java() -> Vec<PathBuf> {
    let Ok(prozesse) = std::fs::read_dir("/proc") else { return Vec::new() };
    prozesse
        .flatten()
        .filter(|p| std::fs::read(p.path().join("cmdline")).is_ok_and(|c| c.windows(13).any(|w| w == b"net.minecraft")))
        .filter_map(|p| std::fs::read_link(p.path().join("exe")).ok())
        .filter(|exe| exe.file_name().is_some_and(|n| n == "java"))
        .collect()
}

// Windows: die Befehlszeile fremder Prozesse ist ohne Umwege nicht lesbar, deshalb jedes
// java/javaw -- ein falsches faellt spaeter beim Start der Dumper auf.
#[cfg(windows)]
fn running_java() -> Vec<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};

    let mut gefunden = Vec::new();
    // SAFETY: Snapshot und Prozess-Handles werden nur hier benutzt und wieder geschlossen;
    // die Puffer sind so gross, wie den Funktionen gesagt wird.
    unsafe {
        let schnappschuss = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if schnappschuss == INVALID_HANDLE_VALUE {
            return gefunden;
        }
        let mut eintrag: PROCESSENTRY32W = std::mem::zeroed();
        eintrag.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut weiter = Process32FirstW(schnappschuss, &mut eintrag) != 0;
        while weiter {
            let laenge = eintrag.szExeFile.iter().position(|&c| c == 0).unwrap_or(eintrag.szExeFile.len());
            let name = String::from_utf16_lossy(&eintrag.szExeFile[..laenge]).to_ascii_lowercase();
            if name == "javaw.exe" || name == "java.exe" {
                let prozess = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, eintrag.th32ProcessID);
                if !prozess.is_null() {
                    let mut puffer = [0u16; 1024];
                    let mut n = puffer.len() as u32;
                    if QueryFullProcessImageNameW(prozess, 0, puffer.as_mut_ptr(), &mut n) != 0 {
                        gefunden.push(PathBuf::from(OsString::from_wide(&puffer[..n as usize])));
                    }
                    CloseHandle(prozess);
                }
            }
            weiter = Process32NextW(schnappschuss, &mut eintrag) != 0;
        }
        CloseHandle(schnappschuss);
    }
    gefunden
}

#[cfg(not(any(target_os = "linux", windows)))]
fn running_java() -> Vec<PathBuf> {
    Vec::new()
}


/// Ordner der extrahierten Daten fuer `version`.
pub fn game_data_dir(version: &str) -> Option<PathBuf> {
    Some(data_dir()?.join("storage").join(version))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameDataStatus {
    /// Noch nie extrahiert (oder unvollstaendig).
    Missing,
    /// Passt zum Jar der gewaehlten Installation und zu den Dumpern dieses Programms.
    Current,
    /// Das Jar hat sich geaendert (Mojang hat die Version aktualisiert) oder es ist eine
    /// andere Installation gewaehlt -- die Daten gehen noch, sollten aber neu extrahiert werden.
    JarChanged,
    /// Die Dumper sind neuer als die Daten (Client-Update) -- das Format kann sich geaendert
    /// haben, neu extrahieren ist noetig.
    DumperChanged,
}

fn jar_stamp(jar: &Path) -> Option<String> {
    let meta = std::fs::metadata(jar).ok()?;
    let zeit = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("jar {} {} {}", jar.display(), meta.len(), zeit))
}

fn dumper_stamp(datei: &str, quelle: &str) -> String {
    format!("{datei} {:016x}", fnv(quelle.as_bytes(), 0xcbf2_9ce4_8422_2325))
}

/// Stimmen die extrahierten Daten noch? Fragt nur Dateien ab, startet nichts.
pub fn game_data_status(version: &str) -> GameDataStatus {
    let Some(dir) = game_data_dir(version) else { return GameDataStatus::Missing };
    let Ok(stempel) = std::fs::read_to_string(dir.join(STAMP)) else { return GameDataStatus::Missing };
    if DUMPS.iter().any(|(name, _, _)| !dir.join(name).is_file()) {
        return GameDataStatus::Missing;
    }
    let zeilen: Vec<&str> = stempel.lines().collect();
    if DUMPS.iter().any(|(_, datei, quelle)| !zeilen.contains(&dumper_stamp(datei, quelle).as_str())) {
        return GameDataStatus::DumperChanged;
    }
    let jar = active_source().map(|s| s.java_launch(version).client_jar).and_then(|j| jar_stamp(&j));
    match jar {
        Some(j) if zeilen.contains(&j.as_str()) => GameDataStatus::Current,
        _ => GameDataStatus::JarChanged,
    }
}

/// Alle Spieldaten aus der gewaehlten Installation extrahieren (dauert ein paar Sekunden --
/// nicht im UI-Thread). `progress` bekommt je Schritt eine kurze Beschreibung.
pub fn extract_game_data(version: &str, progress: &dyn Fn(&str)) -> Result<(), AssetsError> {
    let source = active_source().ok_or(AssetsError::MinecraftDirNotFound)?;
    let launch = source.java_launch(version);
    let jar = jar_stamp(&launch.client_jar).ok_or_else(|| AssetsError::VersionNotFound { version: version.to_string(), path: launch.client_jar.clone() })?;
    let dir = game_data_dir(version).ok_or(AssetsError::MinecraftDirNotFound)?;
    std::fs::create_dir_all(&dir)?;
    let mut stempel = vec![jar];
    for (name, datei, quelle) in DUMPS {
        progress(datei);
        let text = generate(&launch, &dir, datei, quelle)?;
        // Erst daneben schreiben, dann umbenennen: ein gleichzeitig startender Client sieht nie
        // eine halbe Datei.
        let neu = dir.join(format!("{name}.{}.tmp", std::process::id()));
        std::fs::write(&neu, text)?;
        std::fs::rename(&neu, dir.join(name))?;
        stempel.push(dumper_stamp(datei, quelle));
    }
    std::fs::write(dir.join(STAMP), stempel.join("\n") + "\n")?;
    Ok(())
}

fn stored(version: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(game_data_dir(version)?.join(name)).ok()
}

/// Die extrahierten Entity-Modelle (Format von world::model_data), None = nicht extrahiert.
pub fn stored_entity_models(version: &str) -> Option<String> {
    stored(version, DUMPS[0].0)
}

/// Die extrahierten Kreativ-Tabs (Format s. CreativeTabsDump.java), None = nicht extrahiert.
pub fn stored_creative_tabs(version: &str) -> Option<String> {
    stored(version, DUMPS[1].0)
}

/// Die extrahierten Standard-Components aller Items (Format s. ItemComponentsDump.java), None =
/// nicht extrahiert.
pub fn stored_item_components(version: &str) -> Option<String> {
    stored(version, DUMPS[2].0)
}

fn generate(launch: &JavaLaunch, dir: &Path, datei: &str, quelle_text: &str) -> Result<String, AssetsError> {
    let json: VersionJson = serde_json::from_slice(&std::fs::read(&launch.version_json)?)?;
    let trenner = if cfg!(windows) { ";" } else { ":" };
    let mut cp = vec![launch.client_jar.to_string_lossy().into_owned()];
    for lib in &json.libraries {
        if let Some(a) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            let Some(rel) = a.path.clone().or_else(|| lib.name.as_deref().and_then(maven_path)) else { continue };
            let pfad = launch.libraries_dir.join(rel);
            if pfad.is_file() {
                cp.push(pfad.to_string_lossy().into_owned());
            }
        }
    }
    // Eigene Dateinamen je Lauf: zwei gleichzeitige Erzeugungen (zwei Clients, parallele
    // Tests) duerfen sich die Zwischendateien nicht gegenseitig wegnehmen.
    static LAUF: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let kennung = format!("{}-{}", std::process::id(), LAUF.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let arbeit = dir.join(format!("dump-{kennung}"));
    std::fs::create_dir_all(&arbeit)?;
    let quelle = arbeit.join(datei);
    std::fs::write(&quelle, quelle_text)?;
    let ziel = arbeit.join("ausgabe.dat");
    let komponente = json.java_version.as_ref().and_then(|j| j.component.as_deref()).or(json.compatible_java_name.as_deref());
    let java = find_java(launch, komponente);
    let mut befehl = Command::new(&java);
    // Windows: kein Konsolenfenster aufblitzen lassen (CREATE_NO_WINDOW).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        befehl.creation_flags(0x0800_0000);
    }
    // Im Arbeitsordner starten: Minecrafts Bootstrap legt im aktuellen Verzeichnis einen
    // logs/-Ordner an, der sonst neben dem Client liegen bliebe.
    let ausgabe = befehl
        .current_dir(&arbeit)
        .arg("-cp")
        .arg(cp.join(trenner))
        .arg(&quelle)
        .arg(&ziel)
        .output();
    let ausgabe = ausgabe.map_err(|e| AssetsError::Io(std::io::Error::new(e.kind(), format!("Java ({}) nicht startbar: {e}", java.display()))))?;
    if !ausgabe.status.success() {
        let _ = std::fs::remove_dir_all(&arbeit);
        let fehler = String::from_utf8_lossy(&ausgabe.stderr);
        let zeile = fehler.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
        return Err(AssetsError::Io(std::io::Error::other(format!("{datei} fehlgeschlagen ({}): {zeile}", java.display()))));
    }
    let text = std::fs::read_to_string(&ziel);
    let _ = std::fs::remove_dir_all(&arbeit);
    Ok(text?)
}

#[cfg(test)]
mod tests {
    // Extrahiert wirklich aus der gewaehlten Installation (wie der Knopf im Launcher) und prueft
    // ein paar Standard-Components, die der Client braucht. Braucht Minecraft + Java, deshalb
    // #[ignore].
    #[test]
    #[ignore]
    fn standard_components_werden_extrahiert() {
        super::extract_game_data("1.21.11", &|schritt| eprintln!("{schritt}")).unwrap();
        let text = super::stored_item_components("1.21.11").unwrap();
        let item = |name: &str| -> String {
            let start = text.find(&format!("I minecraft:{name}\n")).unwrap_or_else(|| panic!("{name}"));
            let rest = &text[start + 1..];
            rest[..rest.find("\nI ").unwrap_or(rest.len())].to_string()
        };
        assert!(item("elytra").contains("C minecraft:glider "), "{}", item("elytra"));
        assert!(item("totem_of_undying").contains("C minecraft:death_protection "));
        assert!(item("shield").contains("C minecraft:blocks_attacks "));
        assert!(item("diamond_helmet").contains("C minecraft:equippable "));
        let ohne_netzform = text.lines().filter(|l| l.starts_with("C ") && l.split('\t').nth(1) == Some("-")).count();
        eprintln!("{} Zeilen, {} ohne Netzform", text.lines().count(), ohne_netzform);
    }
}
