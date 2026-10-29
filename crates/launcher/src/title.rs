// Der Hinweis oben links im Startbild, in Minecrafts Schrift. Die Schrift liegt nicht im
// Launcher: sie wird wie beim Client aus der gewaehlten Installation gelesen (s.
// assets::font). Gerastert wird einmal in Font-Pixeln, egui vergroessert ohne Glaettung.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use eframe::egui;

pub const LINES: [&str; 2] = ["Linksklick für Start,", "Rechtsklick für Einstellungen"];

// Die Zeilen in der Sprache des Launchers.
pub fn lines() -> Vec<String> {
    LINES.iter().map(|l| crate::tr(l).into_owned()).collect()
}
// Zeilenhoehe in Font-Pixeln (wie Vanilla: 9).
const ZEILE: i32 = 9;

pub enum Title {
    Loading(Receiver<Option<egui::ColorImage>>),
    Ready(egui::TextureHandle),
    // Keine nutzbare Installation: der Hinweis steht dann in der Schrift des Launchers.
    Missing,
}

impl Title {
    pub fn start(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(render(&lines()));
            ctx.request_repaint();
        });
        Title::Loading(rx)
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let Title::Loading(rx) = self else { return };
        let Ok(image) = rx.try_recv() else { return };
        *self = match image {
            Some(image) => Title::Ready(ctx.load_texture("title", image, egui::TextureOptions::NEAREST)),
            None => Title::Missing,
        };
    }

    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        match self {
            Title::Ready(texture) => Some(texture),
            _ => None,
        }
    }
}

// Weisse Glyphen auf durchsichtigem Grund, Zeile fuer Zeile. Zeichen ausserhalb von ASCII (ü)
// liegen auf anderen Seiten mit anderer Zellhoehe; alle stehen auf derselben Grundlinie.
// Unbekannte Zeichen fallen weg.
fn render(lines: &[String]) -> Option<egui::ColorImage> {
    let font = assets::MinecraftInstallation::locate(crate::MC_VERSION, Vec::<PathBuf>::new()).ok()?.load_default_font().ok()?;
    let base = font.base_page().ascent;
    let find = |c: char| {
        font.pages.iter().rev().find_map(|p| p.chars.iter().enumerate().find_map(|(row, zeile)| zeile.iter().position(|&z| z == c).map(|col| (p, col as u32, row as u32))))
    };
    // (x, y oben, Seite, Spalte, Zeile im Raster, Breite)
    let mut glyphen = Vec::new();
    for (i, text) in lines.iter().enumerate() {
        let mut pen = 0.0f32;
        for c in text.chars() {
            if c == ' ' {
                pen += font.space_advance;
            } else if let Some((page, col, row)) = find(c) {
                let oben = i as i32 * ZEILE + base - page.ascent;
                glyphen.push((pen as i32, oben, page, col, row, page.glyph_width(col, row)));
                pen += page.advance(col, row);
            }
        }
    }
    let min_y = glyphen.iter().map(|g| g.1).min()?;
    let max_y = glyphen.iter().map(|g| g.1 + g.2.cell_size().1 as i32).max()?;
    let breite = glyphen.iter().map(|g| g.0 + g.5 as i32).max()?;
    let mut image = egui::ColorImage::new([breite as usize, (max_y - min_y) as usize], egui::Color32::TRANSPARENT);
    for (x0, oben, page, col, row, w) in glyphen {
        let (cw, ch) = page.cell_size();
        for y in 0..ch {
            for x in 0..w.min(cw) {
                let (px, py) = (col * cw + x, row * ch + y);
                let alpha = page.texture.rgba[((py * page.texture.width + px) * 4 + 3) as usize];
                if alpha != 0 {
                    image[((x0 + x as i32) as usize, (oben - min_y + y as i32) as usize)] = egui::Color32::from_white_alpha(alpha);
                }
            }
        }
    }
    Some(image)
}
