// Ladebildschirm beim Start: das Fenster steht sofort, waehrend ein Thread die Assets baut
// (Atlas, Modelle, Item-Bilder -- gut zwei Sekunden, s. WorldAssets::build_with_progress).
// Frueher entstand das Fenster erst danach, und solange sah man nichts.
//
// Gezeichnet wird ohne eigene Pipeline: das ganze Bild entsteht auf der CPU (Hintergrund wie im
// Verbinden-Menue, also Theme-Farbe und Hintergrundbild, darauf Schritt, Prozent und Balken) und
// wird in die Swapchain kopiert. Es aendert sich nur ein paar Dutzend Mal.
//
// Ist alles gebaut, wird die Oberflaeche wieder freigegeben und dasselbe Fenster an die App
// uebergeben (s. App::startup_window), die darauf wie bisher ihren GpuState aufsetzt.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::monitor::VideoModeHandle;
use winit::window::{Window, WindowId};

use crate::{i18n, theme, wallpaper, App, Settings, WorldAssets};

enum Msg {
    Progress(u32),
    Done(Box<(assets::MinecraftInstallation, Result<WorldAssets, String>)>),
}

pub(crate) struct Startup {
    phase: Phase,
}

enum Phase {
    Laden(Box<Laden>),
    Laeuft(Box<App>),
    // Nur waehrend der Uebergabe.
    Leer,
}

struct Laden {
    settings: Settings,
    font: Option<assets::BitmapFont>,
    rx: Receiver<Msg>,
    prozent: u32,
    fenster: Option<(Arc<Window>, VideoModeHandle)>,
    schirm: Option<Schirm>,
    wallpaper: wallpaper::Loader,
    // Hintergrundbild, schon genau auf Bildschirmgroesse (RGBA, sRGB).
    hintergrund: Option<(u32, u32, Vec<u8>)>,
    neu_zeichnen: bool,
}

impl Startup {
    pub(crate) fn new(mut installation: assets::MinecraftInstallation, settings: Settings) -> Self {
        theme::apply(&settings.theme);
        // Die Sprache schon jetzt: App::new setzt sie erst nach dem Laden.
        i18n::set_ui_language(i18n::Language::from_index(settings.lang_ui));
        i18n::set_latin_macrons(settings.latin_macrons);
        i18n::set_english_loanwords(settings.english_loanwords);
        let font = installation.load_default_font().ok();
        let (tx, rx) = channel();
        let fortschritt = tx.clone();
        std::thread::Builder::new()
            .name("start-assets".into())
            .spawn(move || {
                let ergebnis = WorldAssets::build_with_progress(&mut installation, &mut |p| {
                    let _ = fortschritt.send(Msg::Progress(p));
                })
                .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Done(Box::new((installation, ergebnis))));
            })
            .expect("Lade-Thread konnte nicht starten");
        let laden = Laden {
            settings,
            font,
            rx,
            prozent: 0,
            fenster: None,
            schirm: None,
            wallpaper: wallpaper::Loader::default(),
            hintergrund: None,
            neu_zeichnen: true,
        };
        Startup { phase: Phase::Laden(Box::new(laden)) }
    }

    // Assets fertig: Oberflaeche freigeben, App bauen und ihr das Fenster geben.
    fn uebergeben(&mut self, event_loop: &ActiveEventLoop, installation: assets::MinecraftInstallation, assets: Result<WorldAssets, String>) {
        let Phase::Laden(mut laden) = std::mem::replace(&mut self.phase, Phase::Leer) else { return };
        let assets = match assets {
            Ok(assets) => assets,
            Err(e) => {
                eprintln!("Coral Client konnte den Block-Textur-Atlas nicht bauen: {e}");
                eprintln!("  Die Minecraft-Installation von {} scheint unvollständig.", crate::MC_VERSION);
                std::process::exit(1);
            }
        };
        // Letztes Bild, bevor die GPU der App aufgesetzt wird (das blockiert kurz).
        laden.prozent = 100;
        laden.zeichnen();
        // Die Swapchain muss weg sein, bevor die App eine neue auf demselben Fenster anlegt.
        laden.schirm = None;
        let mut app = App::new(installation, laden.settings, assets);
        app.startup_window = laden.fenster.take();
        app.resumed(event_loop);
        self.phase = Phase::Laeuft(Box::new(app));
    }
}

impl Laden {
    fn schritt(&self) -> &'static str {
        match self.prozent {
            0..25 => "Block-Texturen",
            25..35 => "Blockzustaende",
            35..60 => "Blockmodelle",
            60..70 => "Items",
            70..85 => "Entity-Texturen",
            85..100 => "Partikel",
            _ => "Grafik wird vorbereitet",
        }
    }

    fn zeichnen(&mut self) {
        let text = format!("{}  {} %", i18n::tr(self.schritt()), self.prozent);
        let Some(schirm) = self.schirm.as_mut() else { return };
        let (w, h) = (schirm.config.width, schirm.config.height);
        let mut bild = match &self.hintergrund {
            Some((bw, bh, rgba)) if (*bw, *bh) == (w, h) => rgba.clone(),
            _ => {
                let c = theme::srgb(theme::Slot::BgApp);
                [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255].repeat((w * h) as usize)
            }
        };
        let mut leinwand = Leinwand { rgba: &mut bild, w, h };
        // Schriftpixel so gross wie bei Vanillas automatischer GUI-Skalierung ungefaehr.
        let s = (h / 270).max(1) as i32;
        let text_w = self.font.as_ref().map_or(0, |f| text_breite(f, &text)) * s;
        let balken_w = (w as i32 * 2 / 5).max(text_w);
        let (mx, my) = (w as i32 / 2, h as i32 / 2);
        let pad = 6 * s;
        let kasten = (mx - balken_w / 2 - pad, my - 12 * s - pad, balken_w + 2 * pad, 18 * s + 2 * pad);
        leinwand.rechteck(kasten, theme::srgb(theme::Slot::BgPanel));
        let balken = (mx - balken_w / 2, my + 2 * s, balken_w, 4 * s);
        leinwand.rechteck(balken, theme::srgb(theme::Slot::Surface2));
        leinwand.rechteck((balken.0, balken.1, balken_w * self.prozent.min(100) as i32 / 100, balken.3), theme::srgb(theme::Slot::Accent));
        if let Some(font) = &self.font {
            leinwand.text(font, &text, (mx - text_w / 2, my - 11 * s), s, theme::srgb(theme::Slot::Text1));
        }
        schirm.zeigen(&bild);
        self.neu_zeichnen = false;
    }

    fn poll(&mut self, event_loop: &ActiveEventLoop) -> Option<Box<(assets::MinecraftInstallation, Result<WorldAssets, String>)>> {
        if self.fenster.is_none() {
            let fenster = crate::create_game_window(event_loop);
            let groesse = fenster.0.inner_size();
            self.schirm = Schirm::new(fenster.0.clone());
            let modus = wallpaper::WallpaperMode::from_index(self.settings.wallpaper);
            let gespeichert = crate::shared_config_dir().map(|d| wallpaper::stored_path(&d));
            self.wallpaper.request(modus, gespeichert, crate::hero_scene::path(), (groesse.width, groesse.height), true);
            self.fenster = Some(fenster);
            self.neu_zeichnen = true;
        }
        for event in self.wallpaper.poll() {
            if let (wallpaper::Event::Loaded(b), Some(schirm)) = (event, &self.schirm) {
                self.hintergrund = Some(auf_schirm(&b, schirm.config.width, schirm.config.height));
                self.neu_zeichnen = true;
            }
        }
        let mut fertig = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Progress(p) => {
                    self.prozent = p;
                    self.neu_zeichnen = true;
                }
                Msg::Done(d) => fertig = Some(d),
            }
        }
        if self.neu_zeichnen && fertig.is_none() {
            self.zeichnen();
        }
        fertig
    }
}

impl ApplicationHandler for Startup {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.phase {
            Phase::Laden(laden) => {
                if let Some(d) = laden.poll(event_loop) {
                    let (installation, assets) = *d;
                    self.uebergeben(event_loop, installation, assets);
                }
            }
            Phase::Laeuft(app) => app.resumed(event_loop),
            Phase::Leer => {}
        }
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Phase::Laeuft(app) = &mut self.phase {
            app.exiting(event_loop);
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if let Phase::Laeuft(app) = &mut self.phase {
            app.new_events(event_loop, cause);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match &mut self.phase {
            Phase::Laden(laden) => {
                if let Some(d) = laden.poll(event_loop) {
                    let (installation, assets) = *d;
                    self.uebergeben(event_loop, installation, assets);
                    return;
                }
                // Der Lade-Thread weckt die Schleife nicht -- regelmaessig nachsehen.
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
            }
            Phase::Laeuft(app) => app.about_to_wait(event_loop),
            Phase::Leer => {}
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        if let Phase::Laeuft(app) = &mut self.phase {
            app.device_event(event_loop, device_id, event);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match &mut self.phase {
            Phase::Laeuft(app) => app.window_event(event_loop, id, event),
            Phase::Laden(laden) => match event {
                WindowEvent::CloseRequested => std::process::exit(0),
                WindowEvent::Resized(groesse) => {
                    if let Some(schirm) = laden.schirm.as_mut() {
                        schirm.groesse(groesse.width, groesse.height);
                    }
                    laden.neu_zeichnen = true;
                }
                WindowEvent::RedrawRequested => laden.zeichnen(),
                _ => {}
            },
            Phase::Leer => {}
        }
    }
}

// Das Bild mittig zuschneiden und genau auf den Schirm bringen (wie cover_uv beim Zeichnen).
fn auf_schirm(b: &wallpaper::Decoded, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let [u0, v0, u1, v1] = wallpaper::cover_uv((b.width, b.height), (w as f32, h as f32));
    let x = (u0 * b.width as f32) as u32;
    let y = (v0 * b.height as f32) as u32;
    let cw = (((u1 - u0) * b.width as f32) as u32).clamp(1, b.width - x);
    let ch = (((v1 - v0) * b.height as f32) as u32).clamp(1, b.height - y);
    let quelle = image::RgbaImage::from_raw(b.width, b.height, b.rgba.clone()).unwrap_or_else(|| image::RgbaImage::new(1, 1));
    let ausschnitt = image::imageops::crop_imm(&quelle, x, y, cw, ch).to_image();
    let bild = image::imageops::resize(&ausschnitt, w, h, image::imageops::FilterType::Triangle);
    (w, h, bild.into_raw())
}

// Breite eines Textes in Schriftpixeln (Minecrafts Bitmap-Schrift).
fn text_breite(font: &assets::BitmapFont, text: &str) -> i32 {
    text.chars().map(|c| glyphe(font, c).map_or(font.space_advance, |(p, col, row)| p.advance(col, row)) as i32).sum()
}

fn glyphe(font: &assets::BitmapFont, c: char) -> Option<(&assets::FontPage, u32, u32)> {
    font.pages.iter().rev().find_map(|p| p.chars.iter().enumerate().find_map(|(row, z)| z.iter().position(|&x| x == c).map(|col| (p, col as u32, row as u32))))
}

struct Leinwand<'a> {
    rgba: &'a mut [u8],
    w: u32,
    h: u32,
}

impl Leinwand<'_> {
    fn rechteck(&mut self, (x, y, w, h): (i32, i32, i32, i32), farbe: u32) {
        let px = [(farbe >> 16) as u8, (farbe >> 8) as u8, farbe as u8, 255];
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                let i = ((yy as u32 * self.w + xx as u32) * 4) as usize;
                self.rgba[i..i + 4].copy_from_slice(&px);
            }
        }
    }

    // `pos`: linke obere Ecke einer Zeile (Grundlinie 7 Schriftpixel darunter), `s` Pixel je
    // Schriftpixel.
    fn text(&mut self, font: &assets::BitmapFont, text: &str, pos: (i32, i32), s: i32, farbe: u32) {
        let base = font.base_page().ascent;
        let mut x = pos.0;
        for c in text.chars() {
            let Some((page, col, row)) = glyphe(font, c) else {
                x += font.space_advance as i32 * s;
                continue;
            };
            let (cw, ch) = page.cell_size();
            let oben = pos.1 + (base - page.ascent) * s;
            for gy in 0..ch {
                for gx in 0..cw {
                    let a = page.texture.rgba[(((row * ch + gy) * page.texture.width + col * cw + gx) * 4 + 3) as usize];
                    if a != 0 {
                        self.rechteck((x + gx as i32 * s, oben + gy as i32 * s, s, s), farbe);
                    }
                }
            }
            x += page.advance(col, row) as i32 * s;
        }
    }
}

// Eigene kleine wgpu-Oberflaeche nur fuer den Ladebildschirm. Das Bild geht erst in eine
// gewoehnliche Textur und wird von dort in die Swapchain kopiert (wie beim GpuState) --
// write_texture direkt in die Swapchain-Textur bringt wgpu 22 zum Absturz.
struct Schirm {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    zwischen: wgpu::Texture,
}

impl Schirm {
    fn new(window: Arc<Window>) -> Option<Self> {
        let size = window.inner_size();
        let backends = if cfg!(windows) { wgpu::Backends::DX12 } else { wgpu::Backends::VULKAN };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..Default::default() });
        let surface = instance.create_surface(window).ok()?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).ok()?;
        let caps = surface.get_capabilities(&adapter);
        if !caps.usages.contains(wgpu::TextureUsages::COPY_DST) {
            return None;
        }
        let format = caps.formats.iter().copied().find(|f| matches!(f, wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Bgra8UnormSrgb))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::COPY_DST,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let zwischen = zwischen_textur(&device, &config);
        Some(Schirm { surface, device, queue, config, zwischen })
    }

    fn groesse(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        self.surface.configure(&self.device, &self.config);
        self.zwischen = zwischen_textur(&self.device, &self.config);
    }

    // `rgba`: ein Bild in Schirmgroesse, sRGB.
    fn zeigen(&mut self, rgba: &[u8]) {
        let Ok(frame) = self.surface.get_current_texture() else { return };
        let (w, h) = (self.config.width, self.config.height);
        if frame.texture.width() != w || frame.texture.height() != h || rgba.len() != (w * h * 4) as usize {
            return;
        }
        let daten = if self.config.format == wgpu::TextureFormat::Bgra8UnormSrgb {
            let mut b = rgba.to_vec();
            b.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
            std::borrow::Cow::Owned(b)
        } else {
            std::borrow::Cow::Borrowed(rgba)
        };
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let ziel = |texture| wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All };
        self.queue.write_texture(ziel(&self.zwischen), &daten, wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, extent);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ladebildschirm") });
        encoder.copy_texture_to_texture(ziel(&self.zwischen), ziel(&frame.texture), extent);
        self.queue.submit([encoder.finish()]);
        frame.present();
    }
}

fn zwischen_textur(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ladebildschirm"),
        size: wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
