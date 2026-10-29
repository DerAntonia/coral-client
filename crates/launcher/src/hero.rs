// Hintergrundbild der Startseite. Es steckt nicht im Launcher und nicht im Repo: der Client
// rendert es beim ersten Start aus einer kleinen Szene mit den Texturen der eigenen Installation
// und legt es im Storage-Ordner ab (s. crates/app/src/hero_scene.rs). Bis es da ist, bleibt die
// Flaeche einfarbig; der Launcher schaut alle paar Sekunden nach, ob es inzwischen fertig ist.
//
// Gezeichnet wird es 1:1 in Bildschirmpixeln: ein Hintergrund-Thread schneidet es auf das
// Seitenverhaeltnis des Fensters zu und rechnet es auf dessen Pixelgroesse herunter, wobei jeder
// Zielpixel die Quellpixel mittelt, die er ueberdeckt (leichtes AA). Die GPU vergroessert oder
// verkleinert dann nichts mehr -- linear verwischte das die Pixelkanten, Nearest gab Moire-Ringe.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use image::RgbaImage;

const FILE: &str = "launcher-hero.png";
// So oft wird nachgesehen, solange es das Bild noch nicht gibt.
const RETRY: Duration = Duration::from_secs(3);

pub enum Hero {
    Loading(Receiver<Option<RgbaImage>>),
    Ready(Fitted),
    Missing(Instant),
}

pub struct Fitted {
    source: Arc<RgbaImage>,
    // Fuer welche Fenstergroesse (Pixel) die Textur gerechnet ist.
    size: [u32; 2],
    texture: Option<egui::TextureHandle>,
    // Laufende Neuberechnung fuer eine neue Fenstergroesse.
    job: Option<([u32; 2], Receiver<egui::ColorImage>)>,
}

impl Hero {
    pub fn start(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load());
            ctx.request_repaint();
        });
        Hero::Loading(rx)
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        match self {
            Hero::Loading(rx) => {
                let Ok(image) = rx.try_recv() else { return };
                *self = match image {
                    Some(image) => Hero::Ready(Fitted { source: Arc::new(image), size: [0, 0], texture: None, job: None }),
                    None => Hero::Missing(Instant::now()),
                };
            }
            Hero::Missing(seit) => {
                if seit.elapsed() >= RETRY {
                    *self = Hero::start(ctx);
                } else {
                    ctx.request_repaint_after(RETRY - seit.elapsed());
                }
            }
            Hero::Ready(fitted) => fitted.poll(ctx),
        }
    }

    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        match self {
            Hero::Ready(fitted) => fitted.texture.as_ref(),
            _ => None,
        }
    }
}

impl Fitted {
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some((size, rx)) = &self.job {
            let Ok(image) = rx.try_recv() else { return };
            self.size = *size;
            self.job = None;
            self.texture = Some(ctx.load_texture("hero", image, egui::TextureOptions::NEAREST));
        }
        let pixel = ctx.screen_rect().size() * ctx.pixels_per_point();
        let ziel = [pixel.x.round().max(1.0) as u32, pixel.y.round().max(1.0) as u32];
        if ziel == self.size {
            return;
        }
        // Waehrend des Rechnens bleibt die alte Textur stehen (paint_hero streckt sie solange).
        let (tx, rx) = channel();
        let (source, ctx) = (self.source.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(fit(&source, ziel));
            ctx.request_repaint();
        });
        self.job = Some((ziel, rx));
    }
}

// Mittig auf das Seitenverhaeltnis von `ziel` zuschneiden und auf genau diese Groesse bringen.
// Triangle mittelt beim Verkleinern ueber die ueberdeckten Quellpixel, ohne stark zu verwischen.
fn fit(source: &RgbaImage, ziel: [u32; 2]) -> egui::ColorImage {
    let (w, h) = (source.width() as f32, source.height() as f32);
    let verhaeltnis = ziel[0] as f32 / ziel[1] as f32;
    let (cw, ch) = if w / h > verhaeltnis { (h * verhaeltnis, h) } else { (w, w / verhaeltnis) };
    let (cw, ch) = (cw.round().max(1.0) as u32, ch.round().max(1.0) as u32);
    let ausschnitt = image::imageops::crop_imm(source, (source.width() - cw) / 2, (source.height() - ch) / 2, cw, ch).to_image();
    let bild = image::imageops::resize(&ausschnitt, ziel[0], ziel[1], image::imageops::FilterType::Triangle);
    egui::ColorImage::from_rgba_unmultiplied([ziel[0] as usize, ziel[1] as usize], bild.as_raw())
}

fn path() -> Option<PathBuf> {
    Some(assets::storage_dir()?.join(FILE))
}

fn load() -> Option<RgbaImage> {
    let bytes = std::fs::read(path()?).ok()?;
    Some(image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?.to_rgba8())
}
