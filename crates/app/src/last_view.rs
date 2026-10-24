//! Letzter Weltblick: das letzte Weltbild (ohne HUD) beim Verlassen einer Welt, als Hintergrund
//! des HUD-Editors ohne Verbindung -- so richtet man die Elemente vor einer echten Spielszene
//! aus statt vor Schwarz. Jeder sieht seine eigene Welt; mitgeliefert wird kein Bild.
//!
//! Liegt neben settings.conf, damit es einen Neustart uebersteht. Speichern und Laden laufen in
//! eigenen Threads: ein PNG in Fenstergroesse zu kodieren kostet sonst ein Standbild.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::wallpaper::Decoded;

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join("last-view.png")
}

/// Der letzte Blick je Servereintrag (die Adresse aus der Liste, nicht die eines Transfers):
/// das Bild hinter dem Verbindungsbildschirm.
pub fn server_path(config_dir: &Path, server: &str) -> PathBuf {
    let name: String = server.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c.to_ascii_lowercase() } else { '_' }).collect();
    config_dir.join("server-views").join(format!("{name}.png"))
}

/// Bild im Hintergrund als PNG schreiben -- erst in eine Nebendatei, dann umbenennen, damit ein
/// Abbruch mitten im Schreiben nie ein halbes Bild hinterlaesst.
pub fn save(path: PathBuf, bild: Decoded) {
    std::thread::spawn(move || {
        if let Some(ordner) = path.parent() {
            let _ = std::fs::create_dir_all(ordner);
        }
        let neu = path.with_extension("png.neu");
        match image::save_buffer_with_format(&neu, &bild.rgba, bild.width, bild.height, image::ColorType::Rgba8, image::ImageFormat::Png) {
            Ok(()) => {
                let _ = std::fs::rename(&neu, &path);
            }
            Err(e) => eprintln!("Letzter Weltblick nicht gespeichert: {e}"),
        }
    });
}

/// Das gespeicherte Bild im Hintergrund laden; None, wenn es keins gibt (noch nie gespielt).
pub fn load(path: PathBuf) -> mpsc::Receiver<Option<Decoded>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Ziel "unendlich": cover_size verkleinert dann nie, das Bild hat ja Fenstergroesse.
        let bild = path.is_file().then(|| crate::wallpaper::decode(&path, (u32::MAX, u32::MAX)).ok()).flatten();
        let _ = tx.send(bild);
    });
    rx
}
