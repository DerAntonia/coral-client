// GPU-Zustand: Texturen, Bind-Groups und die Methoden von GpuState.

use crate::*;

mod init;
mod surface;
mod text_shaping;
mod uploads;
mod ui_draw;
mod frames;
mod sign_text;
pub(crate) use sign_text::{SignGpu, SignInstance};
pub(crate) use text_shaping::sign_border_mode;

// Fuer render_world (s. dortigen Kommentar zu offscreen_texture) -- RENDER_ATTACHMENT zum
// Hineinzeichnen (Welt+HUD), COPY_SRC fuer den Blit zur Swapchain-Textur, TEXTURE_BINDING
// weil der Crosshair-Shader direkt daraus sampelt (s. crosshair_vertices-Kommentar). Bei
// jedem resize() mit neuer Groesse neu erzeugt.
pub(crate) fn create_offscreen_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen color texture"),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
        // Dasselbe Bild ohne sRGB-Umrechnung: darin mischt der Pack-Himmel im Gamma-Raum
        // (s. render::custom_sky).
        view_formats: &[format.remove_srgb_suffix()],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

// Ausgabetextur des CMAA2-Compute-Durchgangs (s. cmaa2.wgsl) -- bewusst Rgba8Unorm (linear,
// KEIN sRGB-Suffix): WebGPU erlaubt keine Speicher-Texturen in sRGB-Formaten, anders als
// offscreen_texture. STORAGE_BINDING zum Hineinschreiben (Compute), TEXTURE_BINDING zum
// Herauslesen (Blit-Fragment-Shader, s. cmaa2_blit.wgsl). Bei jedem resize() neu erzeugt.
pub(crate) fn create_cmaa2_output_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cmaa2 output texture"),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

// Haengt an den konkreten offscreen_view/cmaa2_output_view-Identitaeten -- bei jedem
// resize() (neue Texturen, neue Views) neu erzeugt.
pub(crate) fn create_cmaa2_compute_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    offscreen_view: &wgpu::TextureView,
    cmaa2_output_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cmaa2 compute bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(offscreen_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(cmaa2_output_view) },
        ],
    })
}

// Fuer den Blit-Durchgang (s. cmaa2_blit.wgsl) -- haengt an der cmaa2_output_view-Identitaet,
// bei jedem resize() neu erzeugt.
pub(crate) fn create_cmaa2_blit_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    cmaa2_output_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cmaa2 blit bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(cmaa2_output_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

// Fuer den Farb-Durchgang (s. color.wgsl) -- haengt an der offscreen_view-Identitaet, bei
// jedem resize() neu erzeugt.
pub(crate) fn create_color_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    offscreen_view: &wgpu::TextureView,
    settings_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("color bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(offscreen_view) },
            wgpu::BindGroupEntry { binding: 1, resource: settings_buffer.as_entire_binding() },
        ],
    })
}

// Haengt an einer konkreten offscreen_view-Identitaet -- bei jedem resize() (neue Textur,
// neue View) neu erzeugt.
pub(crate) fn create_crosshair_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    offscreen_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    mode_buffer: &wgpu::Buffer,
    fade_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("crosshair bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(offscreen_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: mode_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: fade_buffer.as_entire_binding() },
        ],
    })
}

pub(crate) struct GpuState {
    // Fuer eine frische Surface, wenn sich die alte nicht mehr einrichten laesst (s.
    // configure_surface).
    pub(crate) instance: wgpu::Instance,
    pub(crate) surface: wgpu::Surface<'static>,
    // false: das letzte configure ist gescheitert, acquire_frame versucht es ab
    // surface_retry_at erneut.
    pub(crate) surface_ok: bool,
    pub(crate) surface_retry_at: Instant,
    // Arc: der Animations-Thread laedt selbst in den Atlas hoch (s. block_anim::UploadTarget).
    pub(crate) device: std::sync::Arc<wgpu::Device>,
    pub(crate) queue: std::sync::Arc<wgpu::Queue>,
    pub(crate) config: wgpu::SurfaceConfiguration,
    pub(crate) window: Arc<Window>,
    pub(crate) offscreen_texture: wgpu::Texture,
    pub(crate) offscreen_view: wgpu::TextureView,
    pub(crate) quad_pipeline: wgpu::RenderPipeline,
    pub(crate) quad_vertex_buffer: wgpu::Buffer,
    pub(crate) line_vertex_buffer: wgpu::Buffer,
    // Mauszeiger: eigene Pipeline mit analytischer Kantenglaettung (s. cursor.wgsl) --
    // ein Quad ueber der Bounding-Box, die Form entsteht im Fragment-Shader aus dem Uniform.
    pub(crate) cursor_pipeline: wgpu::RenderPipeline,
    pub(crate) cursor_uniform_buffer: wgpu::Buffer,
    pub(crate) cursor_bind_group: wgpu::BindGroup,
    // Crosshair-Invertierung per direktem Sampling aus offscreen_view + Fragment-Shader
    // statt GPU-Blend-Trick (s. crosshair_vertices-Kommentar): die Pipeline sampelt
    // offscreen_view direkt (kein Kopierschritt mehr) und gibt 1-Farbe aus. Der Bind-Group
    // haengt an einer konkreten TextureView-Identitaet -- bei jedem resize() neu erzeugt.
    pub(crate) crosshair_pipeline: wgpu::RenderPipeline,
    pub(crate) crosshair_vertex_buffer: wgpu::Buffer,
    pub(crate) crosshair_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) crosshair_sampler: wgpu::Sampler,
    pub(crate) crosshair_mode_buffer: wgpu::Buffer,
    // Komplementaer-Farbe: Zustand ueber mehrere Frames und der Compute-Schritt, der sie je
    // Frame je Farbplatz waehlt und ueberblendet (crosshair.wgsl::cs_fade). Die Uhr misst die Zeit
    // seit dem letzten Schritt.
    pub(crate) crosshair_fade_buffer: wgpu::Buffer,
    pub(crate) crosshair_fade_pipeline: wgpu::ComputePipeline,
    pub(crate) crosshair_fade_clock: Instant,
    pub(crate) crosshair_bind_group: wgpu::BindGroup,
    // CMAA2 (s. cmaa2.wgsl-Kommentar): Compute-Infrastruktur + gather-basierte,
    // kontrastadaptive Kantenglaettung.
    pub(crate) cmaa2_output_texture: wgpu::Texture,
    pub(crate) cmaa2_output_view: wgpu::TextureView,
    pub(crate) cmaa2_compute_pipeline: wgpu::ComputePipeline,
    pub(crate) cmaa2_compute_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) cmaa2_compute_bind_group: wgpu::BindGroup,
    pub(crate) cmaa2_blit_pipeline: wgpu::RenderPipeline,
    pub(crate) cmaa2_blit_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) cmaa2_blit_sampler: wgpu::Sampler,
    pub(crate) cmaa2_blit_bind_group: wgpu::BindGroup,
    // Modul "Farbe" (s. color.wgsl): ersetzt die Schlusskopie in die Swapchain, solange die
    // Saettigung nicht 100 % ist. Der Puffer haelt die Saettigung (16 Byte, s. Shader).
    pub(crate) color_pipeline: wgpu::RenderPipeline,
    pub(crate) color_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) color_settings_buffer: wgpu::Buffer,
    pub(crate) color_bind_group: wgpu::BindGroup,
    pub(crate) text_pipeline: wgpu::RenderPipeline,
    pub(crate) text_vertex_buffer: wgpu::Buffer,
    // Schildtext im Welt-Durchgang, mit Tiefentest (s. App::build_sign_texts): im VRAM
    // gesetzte Seiten samt Pipelines (s. SignGpu) und ihre Schrift (Bereich Schilder).
    pub(crate) sign_gpu: std::cell::RefCell<SignGpu>,
    pub(crate) sign_font: std::cell::Cell<Option<FontKind>>,
    // Schrift der Weltbeschriftungen (Bereich Nametags), s. world_label_glyphs.
    pub(crate) label_font: std::cell::Cell<Option<FontKind>>,
    pub(crate) font_atlas: FontAtlas,
    // Weitere Atlanten in anderen Schriften, nur solange eine Anzeige oder ein Bereich sie
    // will (s. App::wanted_fonts und ensure_fonts). Teilen das Layout des ersten.
    pub(crate) extra_fonts: Vec<FontAtlas>,
    // Gewollte Atlanten, die noch im Hintergrund gerastert werden; bis dahin misst und zeichnet
    // ihr Bereich in der allgemeinen Schrift (s. hud_font).
    pub(crate) pending_fonts: Vec<font_atlas::PendingFont>,
    // Schriften der Resourcepacks (Server-HUD, Symbole), geteilt von allen Atlanten.
    pub(crate) pack_glyphs: Option<Arc<font_atlas::PackGlyphCache>>,
    // Mit welcher Schrift gerade gemessen wird (None = allgemeine): App setzt sie fuer die
    // Dauer eines Bereichs mit eigener Schrift (s. App::begin_text_area), shape_text folgt
    // den Schrift-Markern. So stimmen Layout und Zeichnen ueberein.
    pub(crate) layout_font: std::cell::Cell<Option<FontKind>>,
    // Gerade ruhiger Text (s. push_plain_numbers_marker): Messen ohne reservierten Platz.
    pub(crate) plain_numbers: std::cell::Cell<bool>,
    // Wie viele Sprites am Anfang der HUD-Liste UNTER allen Flaechen liegen (das Feuer vor
    // den Augen, s. App::build_fire_overlay) -- sonst verdeckte es Chat-Hintergrund und
    // Keystrokes. Von build_hud gesetzt, von render_world verbraucht.
    pub(crate) under_sprites: std::cell::Cell<usize>,
    // Allgemeiner Textrand (Settings::global_text_shadow), s. shape_text.
    pub(crate) text_shadow_style: TextShadow,
    // Staerke des allgemeinen Rands (s. Settings::shadow_strength).
    pub(crate) text_shadow_strength: f32,
    // Der allgemeine Vanilla-Schatten nur einen halben Schriftpixel versetzt.
    pub(crate) text_shadow_half: bool,
    // Wo die Zeichen des zuletzt gezeichneten Textes stehen (s. shape_text, TextRun).
    pub(crate) text_runs: std::cell::RefCell<Vec<TextRun>>,
    // Aus dem letzten shape_text: ab welchem Vertex auf content_clip beschnitten wird (s.
    // push_clip_marker).
    pub(crate) text_clips: std::cell::RefCell<Vec<(u32, bool)>>,
    // Welche Zahlen wechseln und wie viel Platz sie brauchen (s. stable_numbers). Lernt in
    // shape_text, wirkt in measure_text und shape_text.
    pub(crate) stable_numbers: std::cell::RefCell<stable_numbers::StableNumbers>,
    // Text-Displays als echte Flaechen in der Welt (s. App::build_display_text): schon auf
    // den Bildschirm projizierte Glyphen und Hintergruende. render_world nimmt sie je Bild
    // heraus und zeichnet sie VOR dem HUD (Glyphen vor dem HUD-Text, Hintergruende vor den
    // HUD-Flaechen).
    pub(crate) world_label_glyphs: std::cell::RefCell<Vec<TextVertex>>,
    pub(crate) world_label_quads: std::cell::RefCell<Vec<Vertex>>,
    // Verdeckbare Weltbeschriftungen (schleichende Namensschilder, Text-Displays ohne
    // Durchscheinen): statt des Sichtlinien-Tests gegen die Tiefe der Welt gezeichnet, nach der
    // Welt und vor der Hand (s. App::build_world_labels). Namensschilder als Bildschirmtext je
    // Schild (Ende in occluded_label_texts, NDC-Tiefe des Ankers), Text-Displays schon
    // projiziert mit Tiefe je Ecke, Hintergruende mit der Tiefe in local.w.
    pub(crate) occluded_label_texts: std::cell::RefCell<Vec<UiText>>,
    pub(crate) occluded_label_ends: std::cell::RefCell<Vec<(usize, f32)>>,
    pub(crate) occluded_label_quads: std::cell::RefCell<Vec<Vertex>>,
    pub(crate) occluded_label_glyphs: std::cell::RefCell<Vec<TextVertex>>,
    pub(crate) label_text_pipeline: wgpu::RenderPipeline,
    pub(crate) label_text_buffer: wgpu::Buffer,
    pub(crate) label_quad_pipeline: wgpu::RenderPipeline,
    pub(crate) label_quad_buffer: wgpu::Buffer,
    // Texturierte 2D-Quads (Hotbar-/Inventar-Sheets, Item-Icons -- s. sprite.wgsl). Zwei
    // Bind-Groups gegen DENSELBEN Layout: der UI-Atlas (GUI + flache Item-Sprites) und der
    // Blockatlas (isometrische Blockitems). Eigener Nearest-Sampler ohne Mipmaps, damit
    // Pixel-Art in der Vergroesserung scharf bleibt -- der Welt-Sampler filtert linear.
    pub(crate) sprite_pipeline: wgpu::RenderPipeline,
    // Verzauberungs-Glint ueber Item-Icons (s. sprite.wgsl::fs_glint): additive Pipeline mit
    // demselben Vertex-Layout, dazu Glint-Textur, Sampler und die wandernde Matrix.
    pub(crate) glint_sprite_pipeline: wgpu::RenderPipeline,
    pub(crate) glint_gui_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) glint_gui_bind_group: wgpu::BindGroup,
    pub(crate) glint_gui_uniform: wgpu::Buffer,
    pub(crate) glint_gui_sampler: wgpu::Sampler,
    pub(crate) sprite_vertex_buffer: wgpu::Buffer,
    pub(crate) sprite_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) sprite_sampler: wgpu::Sampler,
    pub(crate) ui_atlas_bind_group: wgpu::BindGroup,
    // Die Textur dahinter, fuer die animierten Item-Bilder (s. App::advance_atlas_animations).
    pub(crate) ui_atlas_texture: wgpu::Texture,
    pub(crate) block_atlas_sprite_bind_group: wgpu::BindGroup,
    pub(crate) entity_atlas_sprite_bind_group: wgpu::BindGroup,
    // Hintergrundbild des Startbildschirms samt Pixelgroesse (s. wallpaper.rs), None = keins.
    // Eigener, LINEARER Sampler: ein Foto wird fast nie pixelgenau gezeichnet, Nearest waere
    // dort Treppchen.
    pub(crate) wallpaper: Option<(wgpu::BindGroup, (u32, u32))>,
    pub(crate) wallpaper_sampler: wgpu::Sampler,
    // Letzter Weltblick (s. last_view.rs): Hintergrund des HUD-Editors ohne Verbindung.
    pub(crate) last_view: Option<(wgpu::BindGroup, (u32, u32))>,
    // Letzter Blick des Servereintrags, mit dem gerade verbunden wird (s. App::connect_view_shown).
    pub(crate) connect_view: Option<(wgpu::BindGroup, (u32, u32))>,
    // Steht in offscreen_texture gerade ein Weltbild OHNE HUD (hinter einem Menue gezeichnet)?
    // Nur so eins taugt als letzter Weltblick -- wer mitten im Spiel getrennt wird, haette sonst
    // Hotbar und Herzen im Hintergrund des HUD-Editors.
    pub(crate) offscreen_world_only: bool,
    // Server-Icons der Server-Liste: ein Atlas mit SERVER_ICON_GRID^2 Plaetzen zu 64x64,
    // angelegt beim ersten Icon (s. upload_server_icon).
    pub(crate) server_icons: Option<(wgpu::Texture, wgpu::BindGroup)>,
    // Pack-Bilder des Skin-Tabs: skin_tab::ICON_GRID^2 Plaetze zu skin_tab::ICON_PX.
    pub(crate) pack_icons: Option<(wgpu::Texture, wgpu::BindGroup)>,
    // GitHub- und Discord-Logo der Kategorienleiste (s. upload_logos).
    pub(crate) logos: Option<(wgpu::Texture, wgpu::BindGroup)>,
    // Das zuletzt gezeichnete Weltbild (offscreen_view) als Sprite-Textur: Hintergrund der
    // Menues, solange man verbunden ist -- wie Vanillas Pausemenue ueber der Welt. None, bis
    // die Welt einmal gezeichnet wurde bzw. nach einem Resize (die Textur ist dann neu/leer).
    pub(crate) world_backdrop: Option<wgpu::BindGroup>,
    pub(crate) block_renderer: render::BlockRenderer,
    // Vom Treiber gelieferte Present-Modi, in der Reihenfolge der Auswahl.
    // Adapter-Info (Name, Hersteller) -- wgpu liefert das ohnehin mit dem Adapter, kein
    // separates Tool (lspci/glxinfo) noetig. Bestimmt, ob TearFree ueberhaupt Sinn ergibt:
    // die Eigenschaft existiert nur beim amdgpu-Treiber (PCI-Vendor-ID 0x1002).
    pub(crate) adapter_info: wgpu::AdapterInfo,
    // Fuer das echte Render-Ahead-Limit 0 (s. frame_latency-Kommentar bei Settings): der
    // Submission-Index des LETZTEN Frames. Bei aktivem Zero-Modus wartet der naechste Frame
    // per device.poll(WaitForSubmissionIndex) explizit darauf, BEVOR er anfaengt -- das ist
    // Sodiums Ansatz (Fence-Wait im Renderer selbst) statt sich auf den Swapchain-Parameter
    // zu verlassen, den wgpu bei 0 ohnehin auf einen hoeheren Wert clamped (s.
    // wgpu-types-Dokumentation zu desired_maximum_frame_latency).
    pub(crate) last_submission: Option<wgpu::SubmissionIndex>,
    // Debug-Flag CORAL_SCREENSHOT (s. screenshot.rs): ob die Swapchain lesbar ist, und ob der
    // naechste Frame kopiert werden soll (F2).
    pub(crate) screenshot_supported: bool,
    pub(crate) screenshot_requested: bool,
    // Ob der Zero-Modus gerade aktiv ist (settings.frame_latency == 0), s. set_frame_latency.
    pub(crate) fence_wait_zero: bool,
    // Fuer den Frame-Profiler: Zeit in render_world fuer Kamera, Zeichenreihenfolge,
    // Translucent-Sortierung und Spalten-Versaetze (vor dem Encoding), letzter Frame.
    pub(crate) last_sort_ms: f32,
    // Zeit in shape_text seit dem letzten Abholen (Profiler-Phase Textsatz), ms.
    pub(crate) text_shape_ms: std::cell::Cell<f32>,
    // Fertig gesetzte Texte vom letzten Mal (s. text_shaping::ShapedCache).
    pub(crate) shaped_cache: std::cell::RefCell<text_shaping::ShapedCache>,
    // Profiler: nach dem Present auf das GPU-Ende dieses Frames warten und den Zeitpunkt
    // festhalten. Erst dann schaltet bei Immediate das Bild um -- das ist der Moment, der
    // ueber die Risszeile entscheidet, nicht der Present-Aufruf. Aendert das Timing leicht
    // (die CPU wartet frueher), deshalb nur, solange der Profiler laeuft.
    pub(crate) measure_gpu_done: bool,
    // GPU-Zeit je Pass (s. gpu_timer), None ohne Timestamp-Unterstuetzung. Misst immer: die
    // Gesamtzeit geht in die Leistungsanzeige (s. FrameStats), die Passes in den Profiler.
    pub(crate) gpu_timer: Option<gpu_timer::GpuTimer>,
    // Shader-Pfad (veraltet, nur mit Feature shader_path, s. crate::shader_path_on): seine
    // Ziele entstehen erst bei seinem ersten Frame.
    #[cfg(feature = "shader_path")]
    pub(crate) shader_path: Option<render::shader_path::ShaderPath>,
    // Fernsicht des Moduls Chunk-Saver (s. lod.rs); `lod_active`: in diesem Frame zeichnen.
    pub(crate) lod: Option<render::lod_renderer::LodRenderer>,
    pub(crate) lod_active: bool,
    // Sonnenstand dieses Frames (world::sky::sky_angle) fuer Sonne/Mond im Shader-Pfad.
    pub(crate) sky_angle: f32,
    // Himmel aus Resourcepacks (s. custom_sky) und seine Ebenen in diesem Frame.
    pub(crate) custom_sky: render::custom_sky::SkyRenderer,
    pub(crate) sky_layers: Vec<render::custom_sky::SkyLayerDraw>,
    // Leucht-Umriss leuchtender Entities (s. render::entity_outline).
    pub(crate) entity_outline: render::entity_outline::EntityOutline,
    // Bewegungsunschaerfe (Render-Tab, s. render::motion_blur); Staerke dieses Frames mit der
    // Kamera und fuer die eigene Bewegung der Entities (je 0 = aus) und wie lange der Monitor
    // ein Bild zeigt, in Sekunden.
    pub(crate) motion_blur: render::motion_blur::MotionBlur,
    pub(crate) motion_blur_strength: f32,
    pub(crate) motion_blur_entity_strength: f32,
    pub(crate) motion_blur_interval_s: f32,
    // Unschaerfe der Hand (Motion Blur > Hand Blur, s. render::hand_blur): Staerke wie oben
    // (0 = aus) und Gewichtung der Teilbilder.
    pub(crate) hand_blur_strength: f32,
    pub(crate) hand_blur_curve: render::hand_blur::Curve,
    // Modul Himmel, Art Verlauf: der Verlaufshimmel dieses Frames (None = Loeschfarbe).
    pub(crate) gradient_sky: render::gradient_sky::GradientSky,
    pub(crate) sky_gradient: Option<render::gradient_sky::GradientFrame>,
    // Shader-Pfad: Oberwelt (der Himmel als Atmosphaere) und seine festen Vorgaben fuer
    // Lichtstreuung (1.0 = Erde), Sonnengroesse und Blocklicht-Schatten in Grad, Lichtkarten-
    // Ansicht (0 = aus). Einstellungen dafuer gibt es nicht mehr.
    #[cfg(feature = "shader_path")]
    pub(crate) atmosphere: bool,
    #[cfg(feature = "shader_path")]
    pub(crate) scattering: f32,
    #[cfg(feature = "shader_path")]
    pub(crate) sun_size_deg: f32,
    #[cfg(feature = "shader_path")]
    pub(crate) block_shadows: bool,
    #[cfg(feature = "shader_path")]
    pub(crate) block_softness_deg: f32,
    #[cfg(feature = "shader_path")]
    pub(crate) lightmap_debug: u8,
    // Riss-Steuerung: genau zu diesem Zeitpunkt (MONOTONIC, ns) praesentieren, s.
    // submit_and_present. Gilt fuer einen Frame.
    pub(crate) present_at_ns: Option<i64>,
}

// Den UI-Atlas (GUI-Sheets + flache Item-Sprites) als GPU-Textur hochladen und daran eine
// Bind-Group fuer die Sprite-Pipeline haengen. Ohne Mipmaps: die Oberflaeche zeichnet nie
// verkleinert.
// Vanillas Glint-Texturen (ItemRenderer.ITEM_ENCHANTMENT_GLINT / ENTITY_ENCHANTMENT_GLINT).
// Fehlt eine (kaputte Installation), bleibt der Glint weiss statt den Start zu verhindern.
pub(crate) fn load_glint_textures(installation: &mut assets::MinecraftInstallation) -> [Option<assets::DecodedTexture>; 2] {
    ["enchanted_glint_item", "enchanted_glint_armor"]
        .map(|name| installation.load_texture(&format!("minecraft/textures/misc/{name}.png")).ok())
}

// Vanillas Niederschlags-Texturen (WeatherEffectRenderer: environment/rain.png, snow.png),
// je Art die des obersten Packs und die oberste nicht leere darunter (ein tieferes Pack, sonst
// Vanilla). Viele PvP-Packs schalten das Wetter ab, indem sie beide Bilder voll durchsichtig
// machen -- das respektiert der Client, solange das Modul "Wetter" aus ist. Ist es an, will man
// den Niederschlag sehen: dann ersetzt ein leeres Bild das naechste darunter, das etwas zeigt
// (s. WeatherTexture::chosen).
pub(crate) struct WeatherTexture {
    pub(crate) pack: Option<assets::DecodedTexture>,
    pub(crate) below: Option<assets::DecodedTexture>,
}

impl WeatherTexture {
    pub(crate) fn pack_is_empty(&self) -> bool {
        self.pack.as_ref().is_some_and(|t| t.rgba.chunks_exact(4).all(|p| p[3] == 0))
    }

    pub(crate) fn chosen(&self, module: bool) -> Option<&assets::DecodedTexture> {
        if module && self.pack_is_empty() { self.below.as_ref() } else { self.pack.as_ref() }
    }
}

// Das erste Bild der Reihe (oberstes Pack zuerst), das nicht ganz durchsichtig ist.
pub(crate) fn first_visible(bilder: impl Iterator<Item = assets::DecodedTexture>) -> Option<assets::DecodedTexture> {
    bilder.into_iter().find(|t| t.rgba.chunks_exact(4).any(|p| p[3] != 0))
}

pub(crate) fn load_weather_textures(installation: &mut assets::MinecraftInstallation) -> [WeatherTexture; 2] {
    let texturen = ["rain", "snow"].map(|name| {
        let pfad = format!("minecraft/textures/environment/{name}.png");
        let dekodiert = |bytes: &[u8]| {
            let img = image::load_from_memory(bytes).ok()?.to_rgba8();
            let (width, height) = img.dimensions();
            Some(assets::DecodedTexture { width, height, rgba: img.into_raw() })
        };
        // load_layers: Vanilla zuerst, das oberste Pack zuletzt. Von oben das erste Bild, das
        // nicht ganz durchsichtig ist.
        let below = first_visible(installation.load_layers(&pfad).iter().rev().filter_map(|b| dekodiert(b)));
        WeatherTexture { pack: installation.load_texture(&pfad).ok(), below }
    });
    let zustand = |t: &WeatherTexture| match &t.pack {
        None => "fehlt".to_string(),
        Some(p) if t.pack_is_empty() => format!("{}x{}, im Pack leer", p.width, p.height),
        Some(p) => format!("{}x{}", p.width, p.height),
    };
    eprintln!("[wetter] Texturen: Regen {}, Schnee {}", zustand(&texturen[0]), zustand(&texturen[1]));
    texturen
}

pub(crate) fn weather_texture_part(t: Option<&assets::DecodedTexture>) -> (&[u8], u32, u32) {
    match t {
        Some(t) => (&t.rgba[..], t.width, t.height),
        None => (&[0, 0, 0, 0][..], 1, 1),
    }
}

pub(crate) fn set_glint_textures(
    renderer: &mut render::BlockRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texturen: &[Option<assets::DecodedTexture>; 2],
) {
    renderer.set_glint_textures(device, queue, glint_texture_part(&texturen[0]), glint_texture_part(&texturen[1]));
}

pub(crate) fn create_gui_glint_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gui glint bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
        ],
    })
}

pub(crate) fn glint_texture_part(t: &Option<assets::DecodedTexture>) -> (&[u8], u32, u32) {
    match t {
        Some(t) => (&t.rgba[..], t.width, t.height),
        None => (&[255, 255, 255, 255][..], 1, 1),
    }
}

pub(crate) fn create_ui_atlas_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    rgba: &[u8],
    size: u32,
) -> (wgpu::BindGroup, wgpu::Texture) {
    let extent = wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui atlas"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: Some(size) },
        extent,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ui atlas bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    (group, texture)
}

// Zweite Bind-Group auf DIESELBE Blockatlas-Textur, die auch die Welt benutzt -- nur mit
// dem Nearest-Sampler der Oberflaeche und ohne Mipmaps (s. sprite_sampler). Blockitems im
// Inventar brauchen keine eigene Textur.
pub(crate) fn create_entity_sprite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    block_renderer: &render::BlockRenderer,
) -> wgpu::BindGroup {
    let view = block_renderer.entity_atlas_texture().create_view(&wgpu::TextureViewDescriptor {
        label: Some("entity atlas sprite view"),
        mip_level_count: Some(1),
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("entity atlas sprite bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

pub(crate) fn create_block_sprite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    block_renderer: &render::BlockRenderer,
) -> wgpu::BindGroup {
    let view = block_renderer.atlas_texture().create_view(&wgpu::TextureViewDescriptor {
        label: Some("block atlas sprite view"),
        mip_level_count: Some(1),
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("block atlas sprite bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

// PCI-Vendor-IDs der grossen GPU-Hersteller, wie sie wgpu/Vulkan meldet.
pub(crate) const PCI_VENDOR_AMD: u32 = 0x1002;
pub(crate) const PCI_VENDOR_NVIDIA: u32 = 0x10de;
pub(crate) const PCI_VENDOR_INTEL: u32 = 0x8086;

impl GpuState {
}
