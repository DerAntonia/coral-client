// VERALTET: der Shader-Pfad wird vorerst nicht weiterentwickelt und ist bis auf Weiteres
// ueberfluessig. Neue Funktionen kommen nur in den PvP-Pfad; hier nichts nachziehen, solange
// das nicht ausdruecklich wieder aufgenommen wird.
//
// Der Shader-Pfad: ein verzoegerter (deferred) Renderer fuer Survival, neben dem unveraenderten
// Forward-Renderer des PvP-Pfads.
//
// Ablauf je Frame (die App ruft die Schritte in dieser Reihenfolge):
//   0. Voxel-Abbild (voxels_mut().update): Weltaenderungen um die Kamera auf die GPU.
//   1. G-Buffer: das opake Gelaende schreibt Albedo und Normale (BlockRenderer::render_gbuffer)
//      und behaelt seine Tiefe (eigener, abtastbarer Tiefenpuffer, s. depth_attachment_keep).
//   2. Schatten (shadows): zwei Strahlen je Pixel ueber die Sonnenscheibe durch das
//      Voxel-Abbild, jeden Frame an anderen Stellen, gemittelt mit den letzten Frames
//      (Verlauf, s. shadow_temporal.wgsl) -- daraus wird der Halbschatten.
//   2b. Blocklicht (ebenfalls in shadows): Lichtkarten je Blockflaeche, nur neu gebacken, wo
//      sich etwas geaendert hat (s. deferred_light.wgsl, cs_lm_faces). Fernlicht: je Quelle
//      ein Strahl in ihren Kegel, derselbe Verlauf wie die Sonne.
//   3. Licht (light): aus dem G-Buffer wird ein HDR-Bild, wo kein Gelaende liegt der Himmel.
//   3. Tonemap (tonemap): HDR -> Bildschirmformat, ins selbe Ziel wie der PvP-Pfad.
//   4. Forward-Rest: Wasser, Entities, Partikel usw. wie im PvP-Pfad darueber, gegen die Tiefe
//      aus Schritt 1 (BlockRenderer::render_forward_rest).
//
// Gemeinsam mit dem PvP-Pfad bleiben Chunk-Verwaltung, Meshes und Texturen. Die Ziele hier
// entstehen erst, wenn der Shader-Pfad zum ersten Mal zeichnet; im PvP-Pfad kostet dieses
// Modul weder Speicher noch Zeit.
//
// Speicher bei 1920x1080: Albedo 8 MB, Normale 8 MB, Licht 8 MB, HDR 16 MB, Tiefe 8 MB; dazu
// das Voxel-Abbild mit 32 MB.

use glam::{DVec3, Mat4, Vec3};

use crate::block_renderer::{GBUFFER_ALBEDO_FORMAT, GBUFFER_LIGHT_FORMAT, GBUFFER_NORMAL_FORMAT};
use crate::voxel_volume::VoxelVolume;

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
// Sonnenschatten: rgb = ankommendes Licht, a = Abstand zum Hindernis in Bloecken.
const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
// Wie im PvP-Pfad (block_renderer::DEPTH_FORMAT), die Pipelines muessen zusammenpassen.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

// Abweichung der Sonnenbahn vom Zenit nach Sueden: so werfen Bloecke auch mittags Schatten.
const SUN_PATH_TILT_DEGREES: f32 = 30.0;

// Gerichtetes Licht eines Frames (s. celestial_light).
pub struct CelestialLight {
    // Richtung ZUR Lichtquelle, Welt.
    pub dir: Vec3,
    // Richtung zur Sonne, auch nachts (der Mond steht gegenueber) -- fuer die Scheiben am
    // Himmel.
    pub sun: Vec3,
    // Farbe * Staerke.
    pub color: [f32; 3],
}

// Senkrechte auf der Ebene der Sonnenbahn (Osten x Mittagssonne).
pub fn sun_path_axis() -> Vec3 {
    let tilt = SUN_PATH_TILT_DEGREES.to_radians();
    Vec3::X.cross(Vec3::new(0.0, tilt.cos(), tilt.sin()))
}

// Sonne oder Mond nach dem Sonnenstand (world::sky::sky_angle, 0 = Mittag). Die Sonne
// laeuft von Osten (+x) ueber Sueden nach Westen; unter dem Horizont uebernimmt der Mond,
// schwach und blaeulich. Am Horizont blenden beide aus, statt umzuspringen.
pub fn celestial_light(sky_angle: f32) -> CelestialLight {
    let a = sky_angle * std::f32::consts::TAU;
    let tilt = SUN_PATH_TILT_DEGREES.to_radians();
    let (x, y) = (-a.sin(), a.cos());
    let sun = Vec3::new(x, y * tilt.cos(), y * tilt.sin());
    let smooth = |e0: f32, e1: f32, v: f32| {
        let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    if sun.y >= 0.0 {
        let s = smooth(-0.02, 0.15, sun.y) * 0.75;
        let warm = smooth(0.0, 0.4, sun.y);
        let mix = |a: f32, b: f32| a + (b - a) * warm;
        CelestialLight { dir: sun, sun, color: [mix(1.0, 1.0) * s, mix(0.55, 0.95) * s, mix(0.3, 0.88) * s] }
    } else {
        let s = smooth(-0.02, 0.15, -sun.y) * 0.12;
        CelestialLight { dir: -sun, sun, color: [0.6 * s, 0.7 * s, 1.0 * s] }
    }
}

// Was der Licht-Pass je Frame braucht.
pub struct LightFrame {
    // Himmelsfarbe, linear.
    pub sky: wgpu::Color,
    // Tageslicht 0..1.
    pub daylight: f32,
    pub view: Mat4,
    pub proj: Mat4,
    // Auge in Weltkoordinaten (Ursprung des Renderraums).
    pub eye: DVec3,
    pub celestial: CelestialLight,
    // Schatten verfolgen (sonst gerichtetes Licht ohne Schatten).
    pub shadows: bool,
    // Himmel als physikalische Atmosphaere (nur Oberwelt; Nether und End ohne Lufthimmel).
    pub atmosphere: bool,
    // Lichtstreuung der Atmosphaere, 1.0 = Erde.
    pub scattering: f32,
    // Scheinbarer Durchmesser der Sonne in Grad: bestimmt, wie schnell der Halbschatten mit
    // dem Abstand zum Hindernis breiter wird (echte Sonne 0,53).
    pub sun_size_deg: f32,
    // Blocklicht-Schatten: Fackeln und andere Quellen werden per Strahl auf Sicht geprueft.
    pub block_shadows: bool,
    // Scheinbare Groesse der Blocklicht-Quellen in Grad (wie sun_size_deg): Halbschatten der
    // Blocklicht-Schatten.
    pub block_softness_deg: f32,
    // Faktor auf Sonnen-/Mond- und Himmelslicht (Lichtstaerken, 1 = unveraendert).
    pub sun_factor: f32,
    // Statt des Bildes: was in den Lichtkarten steht (Fehlersuche, s. lightmap_debug): 0 aus,
    // 1 Arten, 2 Abweichung von echten Strahlen.
    pub lightmap_debug: u8,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LightParams {
    sky: [f32; 4],
    inv_proj: [[f32; 4]; 4],
    inv_view: [[f32; 4]; 4],
    light_dir: [f32; 4],
    light_color: [f32; 4],
    cam_block: [i32; 4],
    cam_frac: [f32; 4],
    sun_dir: [f32; 4],
    sun_axis: [f32; 4],
    // Licht-Raster (s. VoxelVolume::update_lights): xyz = Ursprung (Weltblock), w = 1 an.
    light_grid: [i32; 4],
    // x = tan(halber Winkel der Blocklicht-Quellen).
    block_soft: [f32; 4],
}

// Schattenarten (Index in Targets::raw/history). Blocklicht hat keine: es kommt aus den
// Lichtkarten (s. deferred_light.wgsl, cs_lm_faces).
const SUN: usize = 0;
const FAR: usize = 1;
// Spiegelungen (s. deferred_light.wgsl, fs_reflexion): dieselbe Mittelung ueber die Frames.
const REFL: usize = 2;
const ARTEN: usize = 3;
// Anteil eines neuen Frames am Verlauf in Bewegung: etwa die letzten 1 / VERLAUF_NEU Frames
// zaehlen. Steht die Kamera still und aendert sich nichts, mittelt der Verlauf immer laenger
// (Anteil 1 / (Frames im Stand + 1 / VERLAUF_NEU), hoechstens VERLAUF_STAND Frames) -- erst
// dann steht das Bild ganz ruhig.
const VERLAUF_NEU: f32 = 0.1;
const VERLAUF_STAND: f32 = 256.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TemporalParams {
    inv_proj: [[f32; 4]; 4],
    inv_view: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    bewegung: [f32; 4],
    tiefe: [f32; 4],
}

// Lichtkarten backen: so viel GPU-Zeit je Frame (ms). Wie viele Sections hineinpassen, misst
// BakeTimer.
const LM_BAKE_MS: f32 = 1.5;

// Misst die GPU-Zeit des Backens (Zeitstempel um den Compute-Pass) und schaetzt daraus, wie
// lange eine Section braucht. Gelesen wird asynchron, ein paar Frames spaeter -- die CPU
// wartet nie. Ohne Zeitstempel-Unterstuetzung gibt es keinen und das Budget bleibt fest.
struct BakeTimer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    // Auslese-Puffer im Umlauf, je mit dem Zustand: wie viele Sections er gemessen hat
    // (0 = frei), ob map_async schon angefordert ist und ob es fertig ist.
    slots: Vec<(wgpu::Buffer, usize, bool, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
    // Nanosekunden je Zeitstempel-Takt (Queue::get_timestamp_period), 0 = noch unbekannt.
    period_ns: f32,
    // Geschaetzte Millisekunden je Back-Schritt (s. voxel_volume::LM_SCHRITTE; gleitender
    // Mittelwert).
    ms_per_section: f32,
}

impl BakeTimer {
    fn new(device: &wgpu::Device) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("lightmap bake time"), ty: wgpu::QueryType::Timestamp, count: 2 });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lightmap bake time resolve"),
            size: 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..4)
            .map(|_| {
                let b = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lightmap bake time readback"),
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                (b, 0, false, std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
            })
            .collect();
        Some(Self { set, resolve, slots, period_ns: 0.0, ms_per_section: 0.0 })
    }

    // Fertige Messungen einlesen und fuer die, deren Frame inzwischen abgeschickt ist, das
    // Lesen anfordern. Rueckgabe: wie viele Sections ins Zeitbudget passen.
    fn budget(&mut self) -> usize {
        for (buffer, sections, angefordert, fertig) in &mut self.slots {
            if *sections == 0 {
                continue;
            }
            if !*angefordert {
                let f = fertig.clone();
                buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| f.store(r.is_ok(), std::sync::atomic::Ordering::Release));
                *angefordert = true;
                continue;
            }
            if !fertig.swap(false, std::sync::atomic::Ordering::Acquire) {
                continue;
            }
            let t: [u64; 2] = {
                let daten = buffer.slice(..).get_mapped_range();
                bytemuck::pod_read_unaligned(&daten[..16])
            };
            buffer.unmap();
            let ms = t[1].saturating_sub(t[0]) as f32 * self.period_ns / 1e6 / *sections as f32;
            if ms > 0.0 && ms.is_finite() {
                self.ms_per_section = if self.ms_per_section == 0.0 { ms } else { self.ms_per_section * 0.8 + ms * 0.2 };
            }
            *sections = 0;
            *angefordert = false;
        }
        if self.ms_per_section <= 0.0 {
            return 4;
        }
        ((LM_BAKE_MS / self.ms_per_section) as usize).clamp(1, crate::voxel_volume::LM_BAKE_PER_FRAME)
    }

    // Ein freier Auslese-Puffer fuer diesen Frame, None wenn alle unterwegs sind.
    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.1 == 0)
    }
}

// Anteil des Himmelslichts, der als Umgebung bleibt; der Rest kommt gerichtet.
const AMBIENT_SHARE: f32 = 0.55;

struct Targets {
    width: u32,
    height: u32,
    albedo: wgpu::TextureView,
    normal: wgpu::TextureView,
    light: wgpu::TextureView,
    hdr: wgpu::TextureView,
    depth: wgpu::TextureView,
    // Je Schattenart (SUN, FAR): roh aus dem Strahl-Pass und zwei Verlaufsbilder im
    // Wechsel -- eins schreibt dieser Frame, das andere hat der letzte geschrieben (s.
    // shadow_temporal.wgsl).
    raw: [wgpu::TextureView; ARTEN],
    history: [[wgpu::TextureView; 2]; ARTEN],
    // Je Art und Seite: liest raw[art] und history[art][1 - seite], schreibt history[art][seite].
    temporal_groups: [[wgpu::BindGroup; 2]; ARTEN],
    // Je Seite: Strahl-Passes und Licht-Pass mit history[*][seite].
    light_bind_groups: [wgpu::BindGroup; 2],
    tonemap_bind_group: wgpu::BindGroup,
}

pub struct ShaderPath {
    shadow_pipeline: wgpu::RenderPipeline,
    // Lichtkarten backen (s. deferred_light.wgsl, cs_lm_faces/cs_lm_texels).
    bake_faces_pipeline: wgpu::ComputePipeline,
    bake_assign_pipeline: wgpu::ComputePipeline,
    bake_texels_pipeline: wgpu::ComputePipeline,
    bake_fine_pipeline: wgpu::ComputePipeline,
    bake_group: wgpu::BindGroup,
    bake_timer: Option<BakeTimer>,
    far_light_pipeline: wgpu::RenderPipeline,
    reflexion_pipeline: wgpu::RenderPipeline,
    temporal_pipeline: wgpu::RenderPipeline,
    temporal_layout: wgpu::BindGroupLayout,
    // Verlauf-Parameter je Art (SUN, FAR, REFL; eigene Gueltigkeit).
    temporal_params: [wgpu::Buffer; ARTEN],
    // Welche Verlaufsbilder dieser Frame schreibt (wechselt in prepare).
    seite: usize,
    // Zaehlt die Frames: verschiebt das Rauschen der Strahlen.
    frame: u32,
    // Ansicht * Projektion und Auge des letzten Frames.
    prev: Option<(Mat4, DVec3)>,
    // Frames, in denen Kamera und Welt gleich geblieben sind.
    still_frames: u32,
    // Haben die Verlaufsbilder Inhalt vom letzten Frame? (Nach Groessenaenderung nicht; das
    // Fernlicht nur, wenn es im letzten Frame lief.)
    history_valid: bool,
    far_history_valid: bool,
    refl_history_valid: bool,
    light_pipeline: wgpu::RenderPipeline,
    light_layout: wgpu::BindGroupLayout,
    light_params: wgpu::Buffer,
    tonemap_pipeline: wgpu::RenderPipeline,
    tonemap_layout: wgpu::BindGroupLayout,
    tonemap_params: wgpu::Buffer,
    targets: Option<Targets>,
    voxels: VoxelVolume,
    // Block-Atlas: die Schattenstrahlen lesen daraus das Alpha der Kreuz-Modelle (Gras).
    atlas: wgpu::TextureView,
}

fn uint_texture_entry(binding: u32, view_dimension: wgpu::TextureViewDimension, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension, multisampled: false },
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    fragment: &str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: "vs_fullscreen",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: fragment,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn storage_ro_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn storage_rw_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn uniform_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn render_target(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl ShaderPath {
    // `output_format`: das Format, in das auch der PvP-Pfad zeichnet (Bildschirm/Offscreen).
    // `classes`: die Voxel-Klassen der Block-States (world::voxel), einmal je Formtabelle.
    // `atlas`: der Block-Atlas des BlockRenderers (wird er neu gebaut, muss auch der
    // Shader-Pfad neu entstehen).
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat, classes: world::voxel::VoxelClasses, atlas: wgpu::TextureView) -> Self {
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader path light layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                uniform_entry(2),
                texture_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                storage_entry(6),
                storage_entry(7),
                storage_entry(8),
                storage_entry(9),
                texture_entry(10),
                texture_entry(11),
                storage_entry(12),
                storage_entry(14),
                texture_entry(15),
                storage_rw_entry(16, wgpu::ShaderStages::FRAGMENT),
                storage_rw_entry(19, wgpu::ShaderStages::FRAGMENT),
                uint_texture_entry(20, wgpu::TextureViewDimension::D3, wgpu::ShaderStages::FRAGMENT),
                uint_texture_entry(21, wgpu::TextureViewDimension::D2, wgpu::ShaderStages::FRAGMENT),
                texture_entry(22),
            ],
        });
        // Backen der Lichtkarten (Compute): dieselben Voxel-Daten wie der Licht-Pass.
        let bake_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader path lightmap bake layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                storage_ro_entry(6, wgpu::ShaderStages::COMPUTE),
                storage_ro_entry(7, wgpu::ShaderStages::COMPUTE),
                storage_ro_entry(8, wgpu::ShaderStages::COMPUTE),
                storage_ro_entry(9, wgpu::ShaderStages::COMPUTE),
                storage_ro_entry(12, wgpu::ShaderStages::COMPUTE),
                wgpu::BindGroupLayoutEntry {
                    binding: 20,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                uint_texture_entry(21, wgpu::TextureViewDimension::D2, wgpu::ShaderStages::COMPUTE),
                storage_rw_entry(16, wgpu::ShaderStages::COMPUTE),
                storage_rw_entry(19, wgpu::ShaderStages::COMPUTE),
                wgpu::BindGroupLayoutEntry {
                    binding: 18,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let bake_pipeline = |entry: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shader path lightmap bake"),
                source: wgpu::ShaderSource::Wgsl(include_str!("deferred_light.wgsl").into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shader path lightmap bake"),
                bind_group_layouts: &[&bake_layout],
                push_constant_ranges: &[],
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                module: &module,
                entry_point: entry,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };
        let bake_faces_pipeline = bake_pipeline("cs_lm_faces");
        let bake_assign_pipeline = bake_pipeline("cs_lm_assign");
        let bake_texels_pipeline = bake_pipeline("cs_lm_layers");
        let bake_fine_pipeline = bake_pipeline("cs_lm_fein");
        let temporal_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader path temporal layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                uniform_entry(3),
            ],
        });
        let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader path tonemap layout"),
            entries: &[texture_entry(0), uniform_entry(1)],
        });
        let voxels = VoxelVolume::new(device, classes);
        let light_params = uniform_buffer(device, "shader path light params", std::mem::size_of::<LightParams>() as u64);
        let bake_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader path lightmap bake"),
            layout: &bake_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 2, resource: light_params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(voxels.texture_view()) },
                wgpu::BindGroupEntry { binding: 6, resource: voxels.slots_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: voxels.headers_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: voxels.boxes_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 9, resource: voxels.cross_uvs_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(&atlas) },
                wgpu::BindGroupEntry { binding: 12, resource: voxels.light_grid_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 16, resource: voxels.lightmap_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 18, resource: voxels.bake_jobs_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 19, resource: voxels.lightmap_layer_buffer().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 20, resource: wgpu::BindingResource::TextureView(voxels.state_view()) },
                wgpu::BindGroupEntry { binding: 21, resource: wgpu::BindingResource::TextureView(voxels.state_info_view()) },
            ],
        });
        Self {
            shadow_pipeline: fullscreen_pipeline(device, "shader path shadow", include_str!("deferred_light.wgsl"), "fs_shadow", &light_layout, SHADOW_FORMAT),
            bake_faces_pipeline,
            bake_assign_pipeline,
            bake_texels_pipeline,
            bake_fine_pipeline,
            bake_group,
            bake_timer: BakeTimer::new(device),
            far_light_pipeline: fullscreen_pipeline(device, "shader path far light", include_str!("deferred_light.wgsl"), "fs_far_light", &light_layout, SHADOW_FORMAT),
            reflexion_pipeline: fullscreen_pipeline(device, "shader path reflections", include_str!("deferred_light.wgsl"), "fs_reflexion", &light_layout, SHADOW_FORMAT),
            temporal_pipeline: fullscreen_pipeline(device, "shader path temporal", include_str!("shadow_temporal.wgsl"), "fs_temporal", &temporal_layout, SHADOW_FORMAT),
            temporal_layout,
            temporal_params: std::array::from_fn(|_| uniform_buffer(device, "shader path temporal params", std::mem::size_of::<TemporalParams>() as u64)),
            seite: 0,
            frame: 0,
            prev: None,
            still_frames: 0,
            history_valid: false,
            far_history_valid: false,
            refl_history_valid: false,
            light_pipeline: fullscreen_pipeline(device, "shader path light", include_str!("deferred_light.wgsl"), "fs_light", &light_layout, HDR_FORMAT),
            light_layout,
            light_params,
            tonemap_pipeline: fullscreen_pipeline(device, "shader path tonemap", include_str!("tonemap.wgsl"), "fs_tonemap", &tonemap_layout, output_format),
            tonemap_layout,
            tonemap_params: uniform_buffer(device, "shader path tonemap params", 16),
            targets: None,
            voxels,
            atlas,
        }
    }

    // Ziele in Bildschirmgroesse anlegen bzw. nach einer Groessenaenderung neu anlegen.
    pub fn ensure_size(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.targets.as_ref().is_some_and(|t| t.width == width && t.height == height) {
            return;
        }
        let albedo = render_target(device, "gbuffer albedo", width, height, GBUFFER_ALBEDO_FORMAT);
        let normal = render_target(device, "gbuffer normal", width, height, GBUFFER_NORMAL_FORMAT);
        let light = render_target(device, "gbuffer light", width, height, GBUFFER_LIGHT_FORMAT);
        let hdr = render_target(device, "hdr", width, height, HDR_FORMAT);
        let depth = render_target(device, "shader path depth", width, height, DEPTH_FORMAT);
        let namen = ["shadow", "far light", "reflections"];
        let raw: [wgpu::TextureView; ARTEN] = std::array::from_fn(|art| render_target(device, namen[art], width, height, SHADOW_FORMAT));
        let history: [[wgpu::TextureView; 2]; ARTEN] =
            std::array::from_fn(|art| std::array::from_fn(|_| render_target(device, namen[art], width, height, SHADOW_FORMAT)));
        let temporal_groups: [[wgpu::BindGroup; 2]; ARTEN] = std::array::from_fn(|art| {
            std::array::from_fn(|seite| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shader path temporal"),
                    layout: &self.temporal_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&depth) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&raw[art]) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&history[art][1 - seite]) },
                        wgpu::BindGroupEntry { binding: 3, resource: self.temporal_params[art].as_entire_binding() },
                    ],
                })
            })
        });
        let light_bind_groups: [wgpu::BindGroup; 2] = std::array::from_fn(|seite| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shader path light"),
                layout: &self.light_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&albedo) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&normal) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.light_params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&light) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&depth) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(self.voxels.texture_view()) },
                    wgpu::BindGroupEntry { binding: 6, resource: self.voxels.slots_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: self.voxels.headers_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 8, resource: self.voxels.boxes_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 9, resource: self.voxels.cross_uvs_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(&self.atlas) },
                    wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::TextureView(&history[SUN][seite]) },
                    wgpu::BindGroupEntry { binding: 12, resource: self.voxels.light_grid_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 14, resource: self.voxels.far_lights_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 15, resource: wgpu::BindingResource::TextureView(&history[FAR][seite]) },
                    wgpu::BindGroupEntry { binding: 16, resource: self.voxels.lightmap_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 19, resource: self.voxels.lightmap_layer_buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 20, resource: wgpu::BindingResource::TextureView(self.voxels.state_view()) },
                    wgpu::BindGroupEntry { binding: 21, resource: wgpu::BindingResource::TextureView(self.voxels.state_info_view()) },
                    wgpu::BindGroupEntry { binding: 22, resource: wgpu::BindingResource::TextureView(&history[REFL][seite]) },
                ],
            })
        });
        let tonemap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader path tonemap"),
            layout: &self.tonemap_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hdr) },
                wgpu::BindGroupEntry { binding: 1, resource: self.tonemap_params.as_entire_binding() },
            ],
        });
        self.targets = Some(Targets {
            width,
            height,
            albedo,
            normal,
            light,
            hdr,
            depth,
            raw,
            history,
            temporal_groups,
            light_bind_groups,
            tonemap_bind_group,
        });
        self.history_valid = false;
        self.far_history_valid = false;
        self.refl_history_valid = false;
    }

    // Gemessene GPU-Zeit je Back-Schritt (ms, eine Seite hat voxel_volume::LM_SCHRITTE),
    // None ohne Messung (s. BakeTimer).
    pub fn bake_ms_per_section(&self) -> Option<f32> {
        self.bake_timer.as_ref().map(|t| t.ms_per_section).filter(|&ms| ms > 0.0)
    }

    pub fn voxels_mut(&mut self) -> &mut VoxelVolume {
        &mut self.voxels
    }

    // Tiefe fuer den G-Buffer-Pass: geleert und behalten -- der Licht-Pass liest sie, der
    // Forward-Rest testet gegen sie (depth_attachment_load). Ein eigener Puffer statt dem des
    // PvP-Pfads, damit der dort nicht abtastbar sein muss.
    pub fn depth_attachment_keep(&self) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        let t = self.targets.as_ref().expect("ShaderPath::ensure_size vor dem Zeichnen");
        wgpu::RenderPassDepthStencilAttachment {
            view: &t.depth,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
            stencil_ops: None,
        }
    }

    pub fn depth_attachment_load(&self) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        let t = self.targets.as_ref().expect("ShaderPath::ensure_size vor dem Zeichnen");
        wgpu::RenderPassDepthStencilAttachment {
            view: &t.depth,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
            stencil_ops: None,
        }
    }

    // Die Farbziele des G-Buffer-Passes (Albedo, Normale, Licht), alle geleert. Nur nach ensure_size.
    pub fn gbuffer_attachments(&self) -> [Option<wgpu::RenderPassColorAttachment<'_>>; 3] {
        let t = self.targets.as_ref().expect("ShaderPath::ensure_size vor dem Zeichnen");
        let clear = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })
        };
        [clear(&t.albedo), clear(&t.normal), clear(&t.light)]
    }

    // Parameter fuer Schatten- und Licht-Pass dieses Frames.
    pub fn prepare(&mut self, queue: &wgpu::Queue, frame: &LightFrame) {
        if self.targets.is_none() {
            return;
        }
        self.seite = 1 - self.seite;
        self.frame = self.frame.wrapping_add(1);
        let sonne_tan = (frame.sun_size_deg.to_radians() * 0.5).tan();
        let block_tan = (frame.block_softness_deg.to_radians() * 0.5).tan();
        self.voxels.set_lightmaps(queue, frame.block_shadows.then_some(block_tan));
        if let Some(timer) = self.bake_timer.as_mut() {
            timer.period_ns = queue.get_timestamp_period();
            // Nur noch gemessen (bake_ms_per_section), nicht mehr fuers Budget: die Zeitstempel um
            // den Compute-Pass melden auf RADV ein Fuenfzigstel der echten Zeit (s.
            // voxel_volume::LM_BAKE_PER_FRAME).
            timer.budget();
        }
        let block = frame.eye.floor();
        let frac = (frame.eye - block).as_vec3();
        let c = &frame.celestial;
        let params = LightParams {
            sky: [frame.sky.r as f32, frame.sky.g as f32, frame.sky.b as f32, frame.daylight],
            inv_proj: frame.proj.inverse().to_cols_array_2d(),
            inv_view: frame.view.inverse().to_cols_array_2d(),
            light_dir: [c.dir.x, c.dir.y, c.dir.z, if frame.shadows { 1.0 } else { 0.0 }],
            light_color: [c.color[0] * frame.sun_factor, c.color[1] * frame.sun_factor, c.color[2] * frame.sun_factor, AMBIENT_SHARE],
            cam_block: [block.x as i32, block.y as i32, block.z as i32, (self.frame % 1024) as i32],
            cam_frac: [frac.x, frac.y, frac.z, sonne_tan],
            sun_dir: [c.sun.x, c.sun.y, c.sun.z, frame.scattering],
            sun_axis: {
                let a = sun_path_axis();
                [a.x, a.y, a.z, if frame.atmosphere { 1.0 } else { 0.0 }]
            },
            light_grid: match self.voxels.light_origin() {
                Some(o) if frame.block_shadows => [o[0], o[1], o[2], 1],
                _ => [0; 4],
            },
            block_soft: [block_tan, frame.sun_factor, if self.voxels.has_far_lights() { 1.0 } else { 0.0 }, frame.lightmap_debug as f32],
        };
        queue.write_buffer(&self.light_params, 0, bytemuck::bytes_of(&params));
        // Projektion (glam perspective_rh, Tiefe 0..1): near/far daraus.
        let (zz, wz) = (frame.proj.z_axis.z, frame.proj.w_axis.z);
        let (near, far) = (wz / zz, wz / (zz + 1.0));
        let view_proj = frame.proj * frame.view;
        let (prev_view_proj, bewegung) = match self.prev {
            Some((vp, eye)) => (vp, (frame.eye - eye).as_vec3()),
            None => (view_proj, Vec3::ZERO),
        };
        let still = self.prev.is_some() && prev_view_proj == view_proj && bewegung == Vec3::ZERO && !self.voxels.changed_this_frame();
        self.still_frames = if still { self.still_frames.saturating_add(1) } else { 0 };
        let anteil = 1.0 / (self.still_frames as f32 + 1.0 / VERLAUF_NEU).min(VERLAUF_STAND);
        let verlauf = |gilt: bool| TemporalParams {
            inv_proj: params.inv_proj,
            inv_view: params.inv_view,
            prev_view_proj: prev_view_proj.to_cols_array_2d(),
            bewegung: [bewegung.x, bewegung.y, bewegung.z, if gilt && self.prev.is_some() { anteil } else { 1.0 }],
            // z: Verlauf an die Nachbarn binden (nur in Bewegung, im Stand haelt das Rauschen
            // der Nachbarn ihn sonst unruhig).
            tiefe: [near, far, if still { 0.0 } else { 1.0 }, 0.0],
        };
        queue.write_buffer(&self.temporal_params[0], 0, bytemuck::bytes_of(&verlauf(self.history_valid)));
        queue.write_buffer(&self.temporal_params[1], 0, bytemuck::bytes_of(&verlauf(self.far_history_valid)));
        queue.write_buffer(&self.temporal_params[2], 0, bytemuck::bytes_of(&verlauf(self.refl_history_valid)));
        self.prev = Some((view_proj, frame.eye));
    }

    // Sonnen-, Blocklicht-Schatten und Fernlicht: je ein Strahl-Pass, dann der Verlauf (s.
    // shadow_temporal.wgsl). `stamps` gibt je Pass die Zeitstempel fuer den Frame-Profiler.
    // `stamps` vergibt je Pass Zeitstempel (QuerySet, Anfang, Ende) fuer den Frame-Profiler, None = nicht gemessen.
    pub fn shadows<'q>(&mut self, encoder: &mut wgpu::CommandEncoder, stamps: &mut dyn FnMut(&'static str) -> Option<(&'q wgpu::QuerySet, u32, u32)>) {
        let Some(t) = self.targets.as_ref() else { return };
        let seite = self.seite;
        // Lichtkarten backen, deren Section sich geaendert hat (s. VoxelVolume::update_lights):
        // Seite leeren, Flaechen vormerken, Plaetze verteilen, Texel rechnen.
        let jobs = self.voxels.bake_jobs();
        if !jobs.is_empty() {
            for &(_, seite, schritt) in jobs {
                if schritt != 0 {
                    continue;
                }
                for (offset, size) in VoxelVolume::page_clear_ranges(seite) {
                    encoder.clear_buffer(self.voxels.lightmap_buffer(), offset, Some(size));
                }
            }
            let (flaechen, texel) = VoxelVolume::bake_groups();
            // Misst der Frame-Profiler, gehoeren die Zeitstempel ihm (die Frame-Zeit der GPU
            // schliesst so das Backen ein), sonst dem BakeTimer, falls ein Auslese-Puffer frei ist.
            let profil = stamps("Lichtkarten");
            let messung = if profil.is_some() { None } else { self.bake_timer.as_ref().and_then(BakeTimer::free_slot) };
            let timestamp_writes = match profil {
                Some((query_set, begin, end)) => {
                    Some(wgpu::ComputePassTimestampWrites { query_set, beginning_of_pass_write_index: Some(begin), end_of_pass_write_index: Some(end) })
                }
                None => self.bake_timer.as_ref().filter(|_| messung.is_some()).map(|t| wgpu::ComputePassTimestampWrites {
                    query_set: &t.set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            };
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("Lichtkarten"), timestamp_writes });
            cpass.set_bind_group(0, &self.bake_group, &[]);
            cpass.set_pipeline(&self.bake_faces_pipeline);
            cpass.dispatch_workgroups(flaechen, jobs.len() as u32, 1);
            cpass.set_pipeline(&self.bake_assign_pipeline);
            cpass.dispatch_workgroups(1, jobs.len() as u32, 1);
            cpass.set_pipeline(&self.bake_texels_pipeline);
            cpass.dispatch_workgroups(texel, jobs.len() as u32, 1);
            let (zeilen, plaetze) = VoxelVolume::bake_fine_groups();
            cpass.set_pipeline(&self.bake_fine_pipeline);
            cpass.dispatch_workgroups(zeilen, plaetze, jobs.len() as u32);
            drop(cpass);
            let anzahl = jobs.len();
            if let (Some(timer), Some(i)) = (self.bake_timer.as_mut(), messung) {
                encoder.resolve_query_set(&timer.set, 0..2, &timer.resolve, 0);
                encoder.copy_buffer_to_buffer(&timer.resolve, 0, &timer.slots[i].0, 0, 16);
                timer.slots[i].1 = anzahl;
            }
        }
        let mut pass = |label: &'static str, view: &wgpu::TextureView, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: stamps(label).map(|(query_set, begin, end)| wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(begin),
                    end_of_pass_write_index: Some(end),
                }),
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        let licht = &t.light_bind_groups[seite];
        pass("Sonnenschatten", &t.raw[SUN], &self.shadow_pipeline, licht);
        pass("Sonnenschatten Verlauf", &t.history[SUN][seite], &self.temporal_pipeline, &t.temporal_groups[SUN][seite]);
        // Fernlicht nur, wenn es welche gibt (fs_light liest es dann auch nicht).
        let fern = self.voxels.has_far_lights();
        if fern {
            pass("Fernlicht", &t.raw[FAR], &self.far_light_pipeline, licht);
            pass("Fernlicht Verlauf", &t.history[FAR][seite], &self.temporal_pipeline, &t.temporal_groups[FAR][seite]);
        }
        pass("Spiegelungen", &t.raw[REFL], &self.reflexion_pipeline, licht);
        pass("Spiegelungen Verlauf", &t.history[REFL][seite], &self.temporal_pipeline, &t.temporal_groups[REFL][seite]);
        self.history_valid = true;
        self.far_history_valid = fern;
        self.refl_history_valid = true;
    }

    // Licht-Pass: G-Buffer -> HDR (nach prepare und shadows).
    pub fn light(&self, encoder: &mut wgpu::CommandEncoder, timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>) {
        let Some(t) = self.targets.as_ref() else { return };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shader path light"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &t.hdr,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.light_pipeline);
        pass.set_bind_group(0, &t.light_bind_groups[self.seite], &[]);
        pass.draw(0..3, 0..1);
    }

    // Tonemap: HDR -> `output` (dasselbe Ziel, in das der PvP-Pfad seine Welt zeichnet).
    pub fn tonemap(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        exposure: f32,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let Some(t) = self.targets.as_ref() else { return };
        queue.write_buffer(&self.tonemap_params, 0, bytemuck::cast_slice(&[exposure, 0.0, 0.0, 0.0]));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shader path tonemap"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.tonemap_pipeline);
        pass.set_bind_group(0, &t.tonemap_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
