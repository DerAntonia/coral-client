// Eintrag fuer die App-Suche (GNOME, KDE, ...): eine .desktop-Datei in
// ~/.local/share/applications plus das Icon. Bei jedem Start mit dem aktuellen Pfad des
// Launchers -- wer ihn verschiebt, startet ihn einmal von Hand, danach stimmt der Eintrag wieder.

use std::path::{Path, PathBuf};

// Auch Wayland-app_id / X11-WM_CLASS des Fensters (s. main): darueber ordnet das Dock das
// offene Fenster diesem Eintrag und seinem Icon zu.
pub const APP_ID: &str = "coral-client-launcher";

const ICON_SVG: &str = include_str!("../../../assets/icon.svg");

pub fn install(exe: &Path) {
    // Ein Build aus dem Quellbaum (target/release) soll den Eintrag nicht auf sich umbiegen.
    if exe.ancestors().any(|a| a.join("Cargo.toml").is_file()) {
        return;
    }
    let Some(data) = data_home() else { return };
    let icon = data.join("icons/hicolor/scalable/apps").join(format!("{APP_ID}.svg"));
    let entry = data.join("applications").join(format!("{APP_ID}.desktop"));
    for (path, content) in [(icon, ICON_SVG.to_string()), (entry, desktop_file(exe))] {
        if let Err(e) = write_if_changed(&path, &content) {
            eprintln!("[launcher] {} nicht geschrieben: {e}", path.display());
        }
    }
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

fn desktop_file(exe: &Path) -> String {
    let pfad = exe.to_string_lossy();
    // TryExec ist ein schlichter Pfad: Anfuehrungszeichen dort gehoerten zum Dateinamen, die
    // Datei gaebe es nicht, und der Eintrag bliebe in der Suche unsichtbar.
    let (exe, try_exec) = (quote(&pfad), pfad.replace('\\', "\\\\"));
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Coral Client\n\
         GenericName=Minecraft-Client\n\
         Comment=Launcher und Updates fuer Coral Client\n\
         Exec={exe}\n\
         TryExec={try_exec}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Game;\n\
         Keywords=minecraft;mc;launcher;\n\
         StartupWMClass={APP_ID}\n"
    )
}

// Exec-Argument nach der Desktop-Entry-Spezifikation: in doppelten Anfuehrungszeichen, darin
// ", `, $ und \ mit Backslash. Prozentzeichen sind Feldcodes und werden verdoppelt.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|alt| alt == content) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfad_mit_leerzeichen_und_sonderzeichen() {
        assert_eq!(quote("/home/a b/coral-client-launcher"), "\"/home/a b/coral-client-launcher\"");
        assert_eq!(quote("/x/$y\"z%"), "\"/x/\\$y\\\"z%%\"");
        let datei = desktop_file(Path::new("/home/a/Schreibtisch/coral-client-launcher"));
        assert!(datei.contains("Exec=\"/home/a/Schreibtisch/coral-client-launcher\"\n"));
        assert!(datei.contains("TryExec=/home/a/Schreibtisch/coral-client-launcher\n"));
        assert!(datei.contains("Icon=coral-client-launcher\n"));
    }
}
