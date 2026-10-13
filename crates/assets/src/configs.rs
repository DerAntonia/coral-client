// Mehrere benannte Configs (Einstellungs-Dateien) statt einer einzigen settings.conf. Geteilt
// zwischen Launcher (auswaehlen, anlegen, umbenennen, loeschen) und Client (laedt und speichert
// die beim Start gewaehlte) -- deshalb hier neben config_dir().
//
// Ablage: <config_dir>/configs/<Name>.conf, Format wie bisher settings.conf. Was fuer alle
// Configs gleich ist (Server-Liste, Chat-Verlauf, eigene Klaenge, Hintergrundbild), bleibt
// direkt in <config_dir>.

use std::path::PathBuf;

use crate::source::config_dir;

pub const DEFAULT_CONFIG: &str = "Standard";

fn configs_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("configs"))
}

/// Datei einer Config (muss nicht existieren -- eine fehlende Config startet mit Standardwerten
/// und wird beim ersten Speichern angelegt).
pub fn config_path(name: &str) -> Option<PathBuf> {
    Some(configs_dir()?.join(format!("{name}.conf")))
}

/// Die frueher einzige settings.conf wird zur Config "Standard", sobald es noch keine Configs
/// gibt. Verschieben statt kopieren: zwei Dateien mit denselben Einstellungen liefen sonst
/// auseinander, ohne dass klar waere, welche gilt.
pub fn migrate_legacy() {
    let (Some(base), Some(dir)) = (config_dir(), configs_dir()) else { return };
    let legacy = base.join("settings.conf");
    if !legacy.is_file() || !list_raw(&dir).is_empty() {
        return;
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::rename(&legacy, dir.join(format!("{DEFAULT_CONFIG}.conf")));
    }
}

fn list_raw(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension()? == "conf").then(|| path.file_stem()?.to_str().map(str::to_string)).flatten()
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Alle Configs, alphabetisch. Nie leer: ohne eine einzige Datei gibt es trotzdem "Standard".
pub fn list_configs() -> Vec<String> {
    migrate_legacy();
    let names = configs_dir().map(|dir| list_raw(&dir)).unwrap_or_default();
    if names.is_empty() {
        vec![DEFAULT_CONFIG.to_string()]
    } else {
        names
    }
}

/// Prueft einen vom Nutzer eingegebenen Namen. Er wird zum Dateinamen, also nichts, was ein
/// Dateisystem (Windows eingeschlossen) ablehnt oder als Pfad versteht.
pub fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name fehlt".into());
    }
    if name.chars().count() > 40 {
        return Err("Name zu lang (hoechstens 40 Zeichen)".into());
    }
    if name.starts_with('.') || name.ends_with('.') {
        return Err("Name darf nicht mit einem Punkt beginnen oder enden".into());
    }
    if let Some(c) = name.chars().find(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
        return Err(format!("Zeichen '{c}' ist nicht erlaubt"));
    }
    Ok(name.to_string())
}

fn exists(name: &str) -> bool {
    list_configs().iter().any(|n| n.eq_ignore_ascii_case(name))
}

fn io_err(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::other(msg.into())
}

/// Neue Config anlegen -- leer (Standardwerte) oder als Kopie von `copy_from`.
pub fn create_config(name: &str, copy_from: Option<&str>) -> std::io::Result<String> {
    let name = validate_name(name).map_err(io_err)?;
    if exists(&name) {
        return Err(io_err(format!("\"{name}\" gibt es schon")));
    }
    let target = config_path(&name).ok_or_else(|| io_err("kein Konfigurationsordner"))?;
    std::fs::create_dir_all(target.parent().unwrap())?;
    let content = match copy_from.and_then(config_path) {
        // Eine Quelle, die nie gespeichert wurde, hat nur Standardwerte -- dann leer anlegen.
        Some(source) => std::fs::read(source).unwrap_or_default(),
        None => Vec::new(),
    };
    std::fs::write(target, content)?;
    Ok(name)
}

pub fn rename_config(old: &str, new: &str) -> std::io::Result<String> {
    let new = validate_name(new).map_err(io_err)?;
    // Nur Gross-/Kleinschreibung geaendert ist erlaubt, sonst waere der Name belegt.
    if !old.eq_ignore_ascii_case(&new) && exists(&new) {
        return Err(io_err(format!("\"{new}\" gibt es schon")));
    }
    let (from, to) = (config_path(old), config_path(&new));
    let (Some(from), Some(to)) = (from, to) else { return Err(io_err("kein Konfigurationsordner")) };
    if from.is_file() {
        std::fs::rename(from, to)?;
    } else {
        std::fs::create_dir_all(to.parent().unwrap())?;
        std::fs::write(to, "")?;
    }
    if last_used().as_deref() == Some(old) {
        set_last_used(&new);
    }
    Ok(new)
}

/// Loescht eine Config. Die letzte bleibt stehen -- ohne Config gaebe es nichts zu starten.
pub fn delete_config(name: &str) -> std::io::Result<()> {
    if list_configs().len() <= 1 {
        return Err(io_err("die letzte Config kann nicht geloescht werden"));
    }
    if let Some(path) = config_path(name) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn last_used_path() -> Option<PathBuf> {
    Some(config_dir()?.join("active_config"))
}

/// Zuletzt im Launcher gewaehlte Config -- damit auch ein Start ohne Launcher (Skript,
/// cargo run) dieselbe benutzt.
pub fn last_used() -> Option<String> {
    let name = std::fs::read_to_string(last_used_path()?).ok()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

pub fn set_last_used(name: &str) {
    if let Some(path) = last_used_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, format!("{name}\n"));
    }
}

/// Welche Config der Client beim Start benutzt: `--config` vor der zuletzt gewaehlten vor
/// "Standard". Eine zuletzt gewaehlte, inzwischen geloeschte Config zaehlt nicht.
pub fn startup_config(arg: Option<&str>) -> String {
    migrate_legacy();
    if let Some(name) = arg.and_then(|a| validate_name(a).ok()) {
        return name;
    }
    let configs = list_configs();
    last_used()
        .filter(|name| configs.contains(name))
        .unwrap_or_else(|| configs.into_iter().next().unwrap_or_else(|| DEFAULT_CONFIG.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbrauchbare_namen_werden_abgelehnt() {
        assert_eq!(validate_name("  PvP  "), Ok("PvP".into()));
        assert!(validate_name("").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("x?").is_err());
        assert!(validate_name(&"x".repeat(41)).is_err());
    }
}
