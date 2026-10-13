// Findet die lokale .minecraft-Installation des Nutzers (kein Bundling von
// Mojang-Assets -- siehe Briefing "Rechtliches"). Ueberschreibbar per Env-Var
// fuer Tests/nicht-Standard-Installationsorte.

use std::path::PathBuf;

pub fn locate_minecraft_dir() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var("CORAL_MINECRAFT_DIR") {
        return Some(PathBuf::from(override_path));
    }

    // Windows setzt HOME meist nicht -- dort heisst das Benutzerverzeichnis USERPROFILE.
    // Gebraucht wird es dort nicht (.minecraft liegt unter %APPDATA%).
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)?;

    #[cfg(target_os = "linux")]
    {
        let path = home.join(".minecraft");
        path.is_dir().then_some(path)
    }

    #[cfg(target_os = "macos")]
    {
        let path = home.join("Library/Application Support/minecraft");
        return path.is_dir().then_some(path);
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
        let path = appdata.join(".minecraft");
        return path.is_dir().then_some(path);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let path = home.join(".minecraft");
        path.is_dir().then_some(path)
    }
}
