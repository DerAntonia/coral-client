// wgpu-Pipeline fuer Chunk-Meshes: Tiefenpuffer (erstes Mal in diesem Projekt --
// bisher nur 2D-Overlay ohne Depth-Test) + Kamera-Uniform-Buffer.

// FxHashMap statt std::collections::HashMap: identische API, schneller nicht-kryptografischer
// Hash. draw_visible_columns schlaegt pro Frame jede sichtbare Spalte nach (bei Sichtweite 24
// ~2400 Lookups) -- s. Kommentar an rustc-hash in der Workspace-Cargo.toml.
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use glam::{DVec3, Vec3};
use world::{BlockVertex, FaceGroup, SectionRun, TerrainVertex, FACE_GROUPS, SECTION_SIZE};

use crate::camera::Camera;
use crate::frustum::Frustum;
use crate::occlusion::OcclusionCulling;
use crate::render_order::{self, cover};
use crate::terrain_arena::{ArenaPage, ArenaSlot, TerrainArena};

mod init;
mod meshes;
mod overlays;
mod drawing;
mod buffers;
mod tiles;
use buffers::*;
pub use tiles::*;

// Hoehengrenzen fuer den Frustum-Test der Chunk-AABB. Eine Spalte reicht immer ueber die
// volle Hoehe, unabhaengig davon, wie viele ihrer Sections tatsaechlich Bloecke enthalten.
//
// BEWUSST GROSSZUEGIG statt genau: wie hoch eine Dimension wirklich ist, steht in den
// Registry-Daten und ist je Server verschieden (s. world::chunk::DEFAULT_MIN_Y -- die
// Pruefwelt von anticheat-test.com geht von 0 bis 448). Hier haengt nichts an Genauigkeit:
// ein zu grosser Kasten laesst hoechstens eine Spalte durch den Test, die man gar nicht
// sieht -- ein zu kleiner wuerde sichtbare Geometrie wegschneiden. Deshalb die Spanne so
// weit, dass jede vorstellbare Dimension hineinpasst.
const WORLD_CULL_MIN_Y: i32 = -128;
const WORLD_CULL_MAX_Y: i32 = 1024;

// Mit Stencil: die Render-Order (s. crate::render_order) maskiert darueber je Pixel.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

// G-Buffer des Shader-Pfads (s. crate::shader_path): Albedo als sRGB (dunkle Toene behalten
// ihre Abstufung), Normale im Sichtraum als 10-Bit-Kanaele.
pub const GBUFFER_ALBEDO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const GBUFFER_NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgb10a2Unorm;
// Licht je Pixel: Blocklicht RGB, Himmelslicht in Alpha (s. world::light).
pub const GBUFFER_LIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    // Getrennt statt vorberechnetem view_proj: der Vertex-Shader braucht die reine
    // Sichtraum-Position (view * pos) fuer die nichtlinearen Projektionen und wendet die
    // Perspektive erst danach an. Normal-Modus multipliziert einfach proj*view wie zuvor.
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    // x = Projektionsmodus (0=Normal,1=Panini,2=Stereografisch,3=Zylindrisch),
    // y = tan(fov/2) (halbe Bildhoehe in Sichtraum-Einheiten), z = Seitenverhaeltnis,
    // w = ungenutzt (Deckkraft und Breite der Hitbox-Anzeige tragen die Vertices selbst).
    params: [f32; 4],
    // Hitbox-Anzeige (s. vs_hitbox): x = ungenutzt, y = Mindest-Halbbreite der Geometrie
    // (HITBOX_MIN_RASTER_HALF_PX), z/w = Bildschirmgroesse in Pixeln.
    hitbox: [f32; 4],
    // x = Entities nah an der Kamera rastern ab diesem Abstand in Bloecken (0 = aus, s.
    // block.wgsl::entity_near_discard). Nur die Welt-Kamera; Hand und Inventar-Puppe 0.
    // y = Mip-Bias der Block-Texturen (0 .. -1, s. mip_bias).
    near: [f32; 4],
}

// Ein Vertex der Hitbox-Anzeige. Traegt BEIDE Endpunkte der Kante (augenrelativ, identisch
// fuer alle sechs Vertices des Bandes) -- die Verbreiterung auf Pixelbreite passiert erst im
// Vertex-Shader nach der Projektion (s. block.wgsl::vs_hitbox), nicht hier.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HitboxVertex {
    pub pos_a: [f32; 3],
    pub pos_b: [f32; 3],
    // x = Seite (-1/+1), y = an welchem Endpunkt dieser Vertex sitzt (0 = a, 1 = b).
    pub side_and_end: [f32; 2],
    // rgb + Deckkraft. Je Kante statt als Uniform, weil sich Hitbox-Anzeige und Blockumriss
    // denselben Puffer teilen, aber unabhaengig einstellbar sind (s. app::push_hitbox_edge).
    pub color: [f32; 4],
    // Gewuenschte halbe Linienbreite in Bildschirmpixeln -- aus demselben Grund je Kante.
    pub half_width: f32,
}

// Mindest-Halbbreite der GEOMETRIE in Bildschirmpixeln, unabhaengig von der eingestellten
// Dichte: ohne MSAA testet der Rasterizer pro Pixel nur, ob dessen MITTELPUNKT im Dreieck
// liegt -- ein exakt 1px breites Band trifft je nach Sub-Pixel-Lage mal einen, mal zwei,
// manchmal keinen Pixelmittelpunkt. Die Geometrie wird deshalb immer mindestens so breit
// gezeichnet; die eingestellte Breite stellt stattdessen die analytische Kantenglaettung im
// Fragment-Shader her (s. fs_hitbox).
pub const HITBOX_MIN_RASTER_HALF_PX: f32 = 1.0;

// Ein Eckpunkt eines Partikel-Quads (s. app::particles). Augenrelativ und schon zur Kamera
// gedreht -- der Shader projiziert nur noch. rgba statt rgb wie bei BlockVertex, weil Partikel
// ausblenden (Totem-Glitzer, Blockstaub).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ParticleVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}

// Ein Eckpunkt eines Niederschlags-Quads (s. app::precipitation): wie ParticleVertex, dazu
// die Kugelgrenzen (innen, aussen) in Bloecken um die Kamera. Der Fragment-Shader blendet
// nach dem echten 3D-Abstand aus (s. fs_weather); 0 = diese Grenze gibt es nicht.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WeatherVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    pub radii: [f32; 2],
}

// Welche Vertices des Partikelpuffers mit welcher Pipeline und welchem Atlas gezeichnet
// werden -- Vanillas BillboardParticle.RenderType. Die Bereiche liegen hintereinander im
// selben Puffer, in dieser Reihenfolge.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParticleDraws {
    // PARTICLE_ATLAS_OPAQUE (Krit, Explosion, Rauch, Staub, ...).
    pub opaque: u32,
    // PARTICLE_ATLAS_TRANSLUCENT (Totem und alles, was ausblendet).
    pub translucent: u32,
    // BLOCK_ATLAS_TRANSLUCENT (Blockstaub beim Abbauen und Zerbrechen).
    pub block: u32,
}

// Vanillas ParticleManager haelt je Blatt hoechstens 16384 Partikel (EvictingQueue); ein Quad
// sind 6 Vertices. Drei Blaetter teilen sich hier einen Puffer, rund 3,5 MB.
pub const PARTICLE_VERTEX_CAPACITY: usize = 16_384 * 6;

// Regen und Schnee (s. app::precipitation): ein Quad je Teilchen, hoechstens 4300 Teilchen
// (1500 bei 70 % Menge, bis 200 %).
pub const WEATHER_VERTEX_CAPACITY: usize = 4300 * 6;

// 6 x f32 -- der augenrelative Versatz einer Spalte in ganzen Bloecken und der Nachkomma-
// Anteil des Auges (s. update_column_offsets, block.wgsl::column_relative).
const COLUMN_OFFSET_STRIDE: wgpu::BufferAddress = 28;

// Ein Quad sind zwei Dreiecke = 6 Vertices (s. mesher::push_quad_with_uv). Die Sortierung
// arbeitet auf ganzen Quads, nicht auf Dreiecken -- ein zerrissenes Quad ergaebe Luecken.
// Opakes Gelaende einer Spalte: liegt im gemeinsamen Speicher (s. terrain_arena), gezeichnet
// ueber den Flaechen-Index-Puffer.
struct TerrainMesh {
    slot: ArenaSlot,
    // Flaechen nach Richtung (s. world::group_by_direction) -- abgewandte Gruppen werden gar
    // nicht erst gezeichnet. Je Flaeche 6 Indizes.
    groups: [FaceGroup; FACE_GROUPS],
    // Dieselben Flaechen je Gruppe nach Section unterteilt, in Pufferreihenfolge -- fuer das
    // Hoehlen-Culling, das einzelne Sections weglaesst (s. occlusion).
    runs: Vec<SectionRun>,
    // Bit n: Section n hat opake Flaechen.
    sections: u64,
    // Unterkante der Spalte (Dimension) -- Section n beginnt bei min_y + 16 n.
    min_y: i32,
    // Wie ChunkMesh::offset_slot -- hier zugleich die Instanz-Nummer im indirekten Aufruf.
    offset_slot: u32,
    // Flickbare Spalte (s. world::layout_patchable): je Platz im Puffer der Block, der die
    // Flaeche erzeugt hat, world::FREE_QUAD = frei. None: nur als Ganzes ersetzbar.
    quad_block: Option<Vec<u32>>,
}

// Ein Eintrag im indirekten Zeichenpuffer, Layout wie VkDrawIndexedIndirectCommand
// (= wgpu::util::DrawIndexedIndirectArgs).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct IndexedIndirect {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

// Wasser/Glas einer Spalte: im eigenen gemeinsamen Speicher (translucent_arena), dazu eine
// CPU-Kopie zum Umsortieren nach Abstand (s. sort_translucent). Keine Richtungsgruppen: die
// Flaechen muessen innerhalb der Spalte als Ganzes von hinten nach vorn liegen.
struct TranslucentMesh {
    slot: ArenaSlot,
    offset_slot: u32,
    // Bezugshoehe der Vertex-Positionen (s. world::TerrainVertex).
    min_y: i32,
    // Leer, solange die Spalte beim Sortier-Thread ist (`in_flight`).
    cpu_vertices: Vec<TerrainVertex>,
    generation: u64,
    in_flight: bool,
}

// Die indirekten Aufrufe eines Frames fuer einen gemeinsamen Speicher: `runs` = (Seite, erster
// Eintrag, Anzahl) in Zeichenreihenfolge, je Lauf EIN Mehrfach-Aufruf.
struct IndirectDraws {
    runs: Vec<(u32, u32, u32)>,
    args: Vec<IndexedIndirect>,
    buffer: wgpu::Buffer,
    capacity: u32,
}

impl IndirectDraws {
    fn new(device: &wgpu::Device) -> Self {
        Self { runs: Vec::new(), args: Vec::new(), buffer: create_indirect_buffer(device, 4096), capacity: 4096 }
    }

    fn clear(&mut self) {
        self.runs.clear();
        self.args.clear();
    }

    // Einen Eintrag anhaengen; ein neuer Lauf beginnt, wenn die Seite wechselt.
    fn push(&mut self, page: u32, arg: IndexedIndirect) {
        match self.runs.last_mut() {
            Some((p, _, count)) if *p == page => *count += 1,
            _ => self.runs.push((page, self.args.len() as u32, 1)),
        }
        self.args.push(arg);
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let needed = self.args.len() as u32;
        if needed == 0 {
            return;
        }
        if needed > self.capacity {
            self.capacity = needed.next_power_of_two();
            self.buffer = create_indirect_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.args));
    }

    // Erwartet Pipeline, Index-Puffer und Versatz-Puffer (Slot 1, ganz) schon gesetzt.
    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, pages: &'a [ArenaPage], multi_draw: bool) {
        for &(page, first, count) in &self.runs {
            let Some(page) = pages.get(page as usize) else { continue };
            pass.set_vertex_buffer(0, page.buffer.slice(..));
            if multi_draw {
                let offset = wgpu::BufferAddress::from(first) * std::mem::size_of::<IndexedIndirect>() as wgpu::BufferAddress;
                pass.multi_draw_indexed_indirect(&self.buffer, offset, count);
            } else {
                for a in &self.args[first as usize..(first + count) as usize] {
                    pass.draw_indexed(a.first_index..a.first_index + a.index_count, a.base_vertex, a.first_instance..a.first_instance + 1);
                }
            }
        }
    }
}

const VERTS_PER_QUAD: usize = 6;

// Wie viele frisch sortierte durchsichtige Quads pro Frame hoechstens hochgeladen werden (s.
// sort_translucent). Bewusst ein QUAD-Budget und keine Mesh-Anzahl: die Kosten haengen an der
// Quad-Menge, und eine einzelne Ozean-Spalte kann mehr Quads haben als zwanzig Landspalten
// zusammen. 8192 Quads sind rund 1,5 MB Buffer-Upload. Sortiert wird im eigenen Thread (s.
// SortThread); unterwegs sind hoechstens doppelt so viele.
const SORT_QUADS_PER_FRAME: usize = 8_192;

// Eine Spalte unterwegs zum Sortier-Thread und zurueck. `generation` erkennt, ob die Spalte
// inzwischen neu gemesht wurde -- dann ist das Ergebnis veraltet und wird verworfen.
struct SortJob {
    key: (i32, i32),
    generation: u64,
    verts: Vec<TerrainVertex>,
    camera: Vec3,
}

// Sortiert durchsichtige Quads ausserhalb des Render-Threads. Dort kostete das Sortieren samt
// Umkopieren beim Gehen in jedem neunten Frame ~0,35 ms (Profiler) -- und weil die
// Riss-Steuerung ihren Vorlauf nach den Spitzen bemisst, war das Latenz in jedem Frame. Die
// Vertices wandern in den Thread und zurueck (keine Kopie); hochgeladen wird im Render-Thread.
struct SortThread {
    tx: std::sync::mpsc::Sender<SortJob>,
    rx: std::sync::mpsc::Receiver<SortJob>,
}

impl SortThread {
    fn spawn() -> Self {
        let (tx, jobs) = std::sync::mpsc::channel::<SortJob>();
        let (done, rx) = std::sync::mpsc::channel();
        // Endet von selbst, sobald der Renderer (und damit der Sender) weg ist.
        let _ = std::thread::Builder::new().name("translucent-sort".into()).spawn(move || {
            // Wiederverwendete Arbeitspuffer: bei einem Ozean waeren frische Vecs Dutzende
            // Megabyte Allokation je Kameraschritt.
            let (mut keys, mut scratch) = (Vec::new(), Vec::new());
            while let Ok(mut job) = jobs.recv() {
                sortiere_quads(&mut job.verts, job.camera, &mut keys, &mut scratch);
                if done.send(job).is_err() {
                    return;
                }
            }
        });
        Self { tx, rx }
    }
}

pub struct BlockRenderer {
    // depth_compare: LessEqual statt Less -- die grass_block-Seiten-Ueberlagerung (s.
    // world::mesher::mesh_column-Kommentar) landet HIER im Opak-Pass (discard() statt
    // echtem Blending, s. block.wgsl::fs_main): die echte grass_block_side_overlay.png ist
    // ein HARTER Cutout (jedes Texel Alpha=0 oder Alpha=255, per PIL geprueft, keine
    // weichen Kanten), braucht also kein Blending. Ihre Flaeche ist DECKUNGSGLEICH mit der
    // schon gezeichneten Basisflaeche darunter (exakt dieselben Eckpunkte/Transform, also
    // bit-identische interpolierte Tiefe) -- mit dem alten strikten "Less" wuerde der
    // Tiefentest diese zweite Flaeche IMMER verwerfen. LessEqual laesst sie durch, ohne
    // echtes Z-Fighting zu riskieren (keine numerisch abweichenden, nur zufaellig knapp
    // verschiedenen Tiefen wie bei zwei leicht versetzten Geometrien -- hier ist es dieselbe
    // Geometrie zweimal gezeichnet). Ein FRUEHERER Versuch dieser Sitzung hat das Overlay
    // stattdessen als eigene Translucent-Flaeche (echtes Blending, wie Wasser) behandelt --
    // zwei ungeordnete Translucent-Flaechen (Wasser + Overlay) OHNE Tiefenschreiben
    // sortieren sich aber gegeneinander nicht zuverlaessig (kein Sortieren nach Blickpunkt),
    // egal in welcher Reihenfolge sie gezeichnet werden (das Gras-Overlay wirkte
    // blau -- zwei Reihenfolge-Fixes halfen nichts, weil das nie das eigentliche
    // Problem war). Im Opak-Pass schreibt das Overlay dagegen normal Tiefe, Wasser
    // (Translucent-Pass danach) testet korrekt dagegen.
    pipeline: wgpu::RenderPipeline,
    // Opakes Gelaende im kompakten Format (s. world::terrain_vertex): eigenes Vertex-Layout,
    // sonst wie `pipeline`. Dazu der gemeinsame Index-Puffer, je Flaeche 0,1,2, 0,2,3 --
    // waechst mit der groessten Spalte (s. ensure_quad_indices).
    terrain_pipeline: wgpu::RenderPipeline,
    quad_index_buffer: wgpu::Buffer,
    quad_index_capacity: u32,
    // Zweite Pipeline fuer Wasser (echte Bloecke + Waterlogged-Fuellung, s.
    // world::mesher-Kommentar zu opaque/translucent): echtes Alpha-Blending statt
    // Cutout-Discard, KEIN Tiefenpuffer-Schreiben (sonst wuerde eine Wasserflaeche die
    // dahinterliegende, ebenfalls transparente Wasserflaeche wegschneiden -- bei nur EINER
    // Wasserschicht pro Sichtlinie in der Praxis kaum sichtbar, aber korrekt ist es so:
    // Opak-Pass zuerst, Wasser bleibt immer "obendrauf" sichtbar). Gleicher Vertex-Layout/
    // Bind-Groups wie die Opak-Pipeline, nur anderer Fragment-Entry-Point (kein discard,
    // echtes tex.a statt hart auf 1.0).
    translucent_pipeline: wgpu::RenderPipeline,
    // Halbdurchsichtige Geisterbloecke (s. fs_main_ghost / app::PendingBlock).
    ghost_pipeline: wgpu::RenderPipeline,
    ghost_buffer: wgpu::Buffer,
    ghost_vertex_count: u32,
    // Wie viele der Vertices im Puffer VOLL DECKEND gezeichnet werden (sie stehen vorn, s.
    // set_ghost_vertices). Das sind die Platzierungen innerhalb der Vorhersage-Toleranz und
    // die schon bestaetigten -- halbdurchsichtig wird nur, was der Server zu lange
    // unbeantwortet laesst (s. app::ghost_vertices).
    ghost_solid_count: u32,
    // Danach so viele DURCHSCHEINENDE Vertices (Wasser, Glas), die wie echte gezeichnet werden
    // -- mit der Wasser-Pipeline statt der deckenden. Durch die deckende verlor platziertes
    // Wasser seine Transparenz, bis der Chunk-Mesh kam.
    ghost_translucent_count: u32,
    // Riss-Ueberlagerung beim Abbauen (s. world::mesher::block_breaking_vertices): eigene
    // Pipeline, weil Vanilla sie multiplikativ mischt (RenderPipelines.CRUMBLING:
    // DST_COLOR/SRC_COLOR) und mit einem Tiefen-Bias nach vorn zieht, statt sie wie
    // gewoehnliche Geometrie zu zeichnen.
    crumbling_pipeline: wgpu::RenderPipeline,
    crumbling_buffer: wgpu::Buffer,
    crumbling_vertex_count: u32,
    // Hitbox-Anzeige (Modul, s. app::build_hitbox_vertices): eigene Pipeline UND ein eigenes
    // Vertex-Format (HitboxVertex statt BlockVertex) -- die Kanten werden erst im
    // Vertex-Shader auf Pixelbreite gebracht und brauchen dafuer beide Endpunkte je Vertex.
    hitbox_pipeline: wgpu::RenderPipeline,
    // Nur Stencil bzw. nur auf dem Merker (s. crate::render_order, render_hitboxes).
    hitbox_mark_pipeline: wgpu::RenderPipeline,
    hitbox_sharp_pipeline: wgpu::RenderPipeline,
    // Render-Order (Einstellungen > Render), s. crate::render_order.
    pub render_order: crate::render_order::RenderOrder,
    // Klassen der deckenden Entity-Vertices (s. set_entity_classes): aufsteigende,
    // lueckenlose Bereiche mit ihrer Klasse (render_order::CLASS_*).
    entity_classes: Vec<(std::ops::Range<u32>, u8)>,
    hitbox_buffer: wgpu::Buffer,
    hitbox_vertex_count: u32,
    // Zuletzt gesetzte Hitbox-Parameter (s. set_hitbox_params) -- update_camera schreibt sie
    // zusammen mit der Kamera in DENSELBEN Uniform-Puffer, deshalb muessen sie hier liegen
    // und nicht in einem zweiten Puffer.
    hitbox_params: [f32; 4],
    // Blockumriss im Modus Komplementaer: eigener Durchgang nach der Welt (s. OutlineComplement).
    outline_complement: OutlineComplement,
    depth_view: wgpu::TextureView,
    // Dieselbe Textur, nur die Tiefe -- zum Lesen als Textur (Bewegungsunschaerfe).
    depth_sample_view: wgpu::TextureView,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    atlas_bind_group: wgpu::BindGroup,
    // Derselbe Atlas ohne Mip-Stufen (nur volle Aufloesung), s. mipmapping.
    atlas_bind_group_flat: wgpu::BindGroup,
    // Einstellung Mipmapping: aus = Gelaende und Bloecke immer aus der vollen Aufloesung.
    pub mipmapping: bool,
    entity_bind_group: wgpu::BindGroup,
    // Bleibt als Handle liegen, damit nachgeladene Skins per write_texture hineingeschrieben
    // werden koennen (s. upload_skin).
    entity_atlas_texture: wgpu::Texture,
    // Vanillas Core-Shader aus Resourcepacks (s. vanilla_draw) -- Items in der Welt, wenn ein
    // Pack ihr Programm ersetzt.
    pub vanilla: crate::vanilla_draw::VanillaDraw,
    // Gehalten, damit upload_atlas_tiles() spaeter gezielt eine einzelne Kachelregion
    // (Wasser-Animation, s. dortigen Kommentar) per Kopierbefehl nachladen kann --
    // ohne dieses Handle waere die Textur nach create_atlas_binding() nicht mehr erreichbar
    // (nur noch die View steckt in der Bindgroup).
    // Arc: der Animations-Thread der App laedt animierte Kacheln selbst hinein.
    atlas_texture: std::sync::Arc<wgpu::Texture>,
    // Zwischenpuffer fuer upload_atlas_tiles, waechst bei Bedarf und wird wiederverwendet.
    atlas_upload_buffer: Option<wgpu::Buffer>,
    // Gehalten (statt nur lokal in new()), damit rebuild_atlas() (Skin-Tab "Laden", s.
    // App::reload_resource_pack) eine neue Bindgroup gegen DENSELBEN Layout bauen kann --
    // die Pipelines (pipeline/translucent_pipeline) sind fest an dieses Layout-Objekt
    // gebunden, ein komplett NEUER Layout wuerde sie inkompatibel machen.
    atlas_bind_group_layout: wgpu::BindGroupLayout,
    meshes: HashMap<(i32, i32), TerrainMesh>,
    terrain_arena: TerrainArena,
    // Die Gelaende-Aufrufe dieses Frames (s. prepare_terrain_draws): je Speicherseite der
    // Bereich in terrain_draws_args. Mit Mehrfach-Indirekt-Unterstuetzung (multi_draw) EIN
    // Aufruf je Seite aus terrain_indirect, sonst ein draw_indexed je Eintrag.
    terrain_draws: IndirectDraws,
    // Wasser/Glas ebenso, aber von HINTEN nach VORN und ohne Richtungsgruppen.
    translucent_arena: TerrainArena,
    translucent_draws: IndirectDraws,
    terrain_translucent_pipeline: wgpu::RenderPipeline,
    // Nur das Wasser bzw. alles ausser Wasser (Render-Order, s. draw_after_terrain).
    terrain_translucent_water_pipeline: wgpu::RenderPipeline,
    terrain_translucent_rest_pipeline: wgpu::RenderPipeline,
    // Dasselbe fuer den Shader-Pfad, beleuchtet (s. render_forward_rest).
    lit_terrain_translucent_pipeline: wgpu::RenderPipeline,
    // Tageslicht 0..1 fuer den Shader-Pfad (Kamera-Uniform params.w, s. block.wgsl).
    pub daylight: f32,
    // Entities nah an der Kamera rastern ab diesem Abstand in Bloecken, 0 = aus (Kamera-Uniform
    // near.x, s. block.wgsl::entity_near_discard).
    pub entity_near_dither: f32,
    // Mip-Bias der Welt-Texturen, 0 .. -1 (negativ = schaerfer, aber mehr Moire in der Ferne).
    pub mip_bias: f32,
    // GameTime fuer Vanillas Core-Shader (s. vanilla_draw): Tageszeit 0..1.
    pub vanilla_game_time: f32,
    multi_draw: bool,
    // Shader-Pfad: dasselbe Gelaende in den G-Buffer statt direkt ins Bild (s. render_gbuffer).
    gbuffer_terrain_pipeline: wgpu::RenderPipeline,
    // Hoehlen-Culling (s. occlusion). `cave_culling` schaltet es ab (Vergleichsmessung),
    // `cull_radius` ist die Sichtweite in Spalten.
    occlusion: OcclusionCulling,
    pub cave_culling: bool,
    pub cull_radius: i32,
    // Sichtlinien-Culling fuer Entities und Blockentities (s. sight). Die Spaltendaten kommen
    // ueber set_column_opacity, Entladen und Leeren raeumt es hier mit auf.
    pub sight: crate::sight::SightCulling,
    // Fuer die Debug-Anzeige: Sections mit Gelaende, davon gezeichnet (nach Sichtkegel und
    // Hoehlen-Culling), im letzten Frame.
    pub culling_stats: (u32, u32),
    translucent_meshes: HashMap<(i32, i32), TranslucentMesh>,
    // Zeichenreihenfolge, nach Abstand zur Kamera-Spalte AUFSTEIGEND sortiert (s.
    // update_draw_order). Frueher wurde direkt ueber die HashMap iteriert -- das war gleich
    // zweimal falsch:
    //
    // 1. WASSER (translucent_pipeline) mischt mit ALPHA_BLENDING und OHNE Tiefenschreiben.
    //    Bei dieser Kombination bestimmt allein die Zeichenreihenfolge das Ergebnis, und die
    //    muss von HINTEN nach VORN laufen. HashMap-Reihenfolge ist nicht nur beliebig, sie
    //    aendert sich beim Laufen staendig durch Rehashing (Chunks laden/entladen) -- wo
    //    mehrere Wasserspalten hintereinanderliegen, kippte die Farbe dadurch von Frame zu
    //    Frame.
    // 2. Opak profitiert vom Gegenteil: von VORN nach HINTEN verwirft der Early-Z-Test
    //    verdeckte Fragmente, bevor sie schattiert werden.
    //
    // Getrennte Listen pro Karte statt einer gemeinsamen, damit die Iteration keine
    // fehlschlagenden Lookups macht (die Schluesselmengen sind verschieden).
    draw_order: Vec<(i32, i32)>,
    translucent_order: Vec<(i32, i32)>,
    // Spalte, fuer die die Reihenfolge gilt. Neu sortiert wird nur, wenn der Spieler die
    // Chunk-Spalte wechselt (alle 16 Bloecke) oder sich die Mesh-Menge aendert -- NICHT jeden
    // Frame: die relative Abstandsordnung haengt an nichts anderem.
    order_center: Option<(i32, i32)>,
    order_dirty: bool,
    // Kameraposition, fuer die die durchsichtigen Quads zuletzt sortiert wurden (s.
    // sort_translucent). None = noch nie sortiert.
    translucent_sort_pos: Option<DVec3>,
    // Gemeinsamer Versatz-Puffer: ein vec3 je Spalte, augenrelativ, jeden Frame neu
    // geschrieben (s. update_column_offsets). EIN write_buffer fuer alle Spalten statt eines
    // winzigen Puffers je Spalte -- bei ein paar tausend Spalten waere Letzteres ein paar
    // tausend Treiberaufrufe pro Frame.
    column_offset_buffer: wgpu::Buffer,
    // Wie viele Plaetze der Puffer aktuell fasst. Waechst nur, schrumpft nie -- die
    // Spaltenzahl schwankt beim Laufen staendig, und ein Neuanlegen kostet mehr als der
    // Speicher (12 Byte je Platz).
    column_offset_capacity: u32,
    // Arbeitspuffer fuer die Versaetze, damit pro Frame nichts neu alloziert wird.
    column_offset_scratch: Vec<f32>,
    // Spalten, die gerade einblenden (s. start_column_fade): Beginn und Dauer in Sekunden.
    column_fade: HashMap<(i32, i32), (std::time::Instant, f32)>,
    // Ein einzelner Null-Versatz fuer Puffer, die schon augenrelativ ankommen (Entities,
    // Hand, Geisterbloecke). Dieselben Pipelines verlangen den zweiten Vertex-Puffer, also
    // muss auch dort etwas gebunden sein.
    zero_offset_buffer: wgpu::Buffer,
    // Spalten, deren durchsichtige Quads noch neu sortiert werden muessen. Gefuellt, wenn sich
    // die Kamera weit genug bewegt hat (alle Spalten) oder eine einzelne Spalte neu gemesht
    // wurde -- abgearbeitet ueber mehrere Frames (s. SORT_QUADS_PER_FRAME).
    sort_dirty: HashSet<(i32, i32)>,
    sort_thread: SortThread,
    // Quads, die gerade beim Sortier-Thread sind.
    sort_in_flight_quads: usize,
    // Zaehler fuer TranslucentMesh::generation.
    translucent_generation: u64,
    // Dynamischer Buffer fuer Entity-Boxen: wird jeden Frame neu beschrieben (Entities
    // bewegen sich staendig -- ein Buffer-Neubau pro Frame wie bei Chunks waere Unsinn).
    // Feste Kapazitaet; mehr Entities als reinpassen werden abgeschnitten.
    entity_buffer: wgpu::Buffer,
    // Schadens-Faerbung je Entity-Vertex (rgb + Anteil), gleiche Laenge wie entity_buffer.
    entity_tint_buffer: wgpu::Buffer,
    entity_vertex_count: u32,
    // Davon die letzten, die durchscheinend laufen (s. entity_translucent_pipeline), und die
    // ersten, die das ebenfalls tun (der eigene Spieler, wenn er ausblendet).
    entity_translucent_count: u32,
    entity_head_translucent: u32,
    // Leuchtende Entities noch einmal, mit ihrer Umrissfarbe je Vertex (s. outline_pipeline).
    outline_pipeline: wgpu::RenderPipeline,
    outline_buffer: wgpu::Buffer,
    outline_color_buffer: wgpu::Buffer,
    outline_vertex_count: u32,
    // Entities laufen ueber ein eigenes Shader-Paar, weil Vanillas Rot NACH dem Texturzugriff
    // beigemischt wird (s. block.wgsl::fs_entity) -- die Vertexfarbe der Block-Pipeline ist
    // ein Multiplikator und kann nur abdunkeln.
    entity_pipeline: wgpu::RenderPipeline,
    // Die Flammen an Entities: wie entity_pipeline, aber verdeckbar (Render-Order).
    entity_flame_pipeline: wgpu::RenderPipeline,
    // GPU-Skinning der festen Entity-Modelle (s. skinning), gezeichnet direkt nach dem
    // deckenden Teil der CPU-Entities.
    pub skinning: crate::skinning::Skinning,
    // Vanillas entityTranslucent: Alpha-Blending statt Schnitt bei 0.5 (fs_entity_translucent).
    entity_translucent_pipeline: wgpu::RenderPipeline,
    // Shader-Pfad: Licht je Entity-Vertex (gepackt wie world::light::pack_vertex_light, Bit 31
    // = unbeleuchtet) und die beiden Pipelines, die es lesen (s. block.wgsl::vs_entity_lit).
    // Im PvP-Pfad wird der Puffer weder beschrieben noch gebunden.
    entity_light_buffer: wgpu::Buffer,
    entity_light_count: u32,
    lit_entity_pipeline: wgpu::RenderPipeline,
    lit_entity_translucent_pipeline: wgpu::RenderPipeline,
    // Shader-Pfad: Licht je Vertex fuer den deckenden Teil der Geisterbloecke (dazu fallende
    // Bloecke und block_displays, s. app::ghost_vertices) und die Pipeline, die es liest.
    ghost_light_buffer: wgpu::Buffer,
    ghost_light_count: u32,
    gbuffer_ghost_pipeline: wgpu::RenderPipeline,
    // Shader-Pfad: Licht an der Augenposition fuer die Ego-Hand (None = PvP-Pfad, unbeleuchtet).
    pub hand_light: Option<u32>,
    // Maske des eigenen Spielers (s. render_own_mask) und wie viele Vertices am Anfang der
    // Entity-Liste ihm gehoeren.
    own_mask_pipeline: wgpu::RenderPipeline,
    own_vertex_count: u32,
    // Motion Blur bewegter Entities: je Ecke die Lage im vorigen Bild (s. set_entity_prev_positions)
    // und die Sicht-Projektion des vorigen Bildes; ob sich ueberhaupt eins bewegt hat.
    entity_motion_pipeline: wgpu::RenderPipeline,
    entity_prev_buffer: wgpu::Buffer,
    entity_motion_params: wgpu::Buffer,
    entity_motion_bind_group: wgpu::BindGroup,
    entity_prev_count: u32,
    entity_moving: bool,
    // Dasselbe fuer den deckenden Teil des Geisterblock-Puffers (fallende Bloecke, TNT,
    // Block-Displays, Kolbenbloecke; s. set_ghost_prev_positions).
    ghost_prev_buffer: wgpu::Buffer,
    ghost_prev_count: u32,
    ghost_moving: bool,
    // Dasselbe fuer die Partikel: je Ecke die Lage im vorigen Bild (s.
    // set_particle_prev_positions), gezeichnet in dasselbe Bild der Objektbewegung.
    particle_motion_pipeline: wgpu::RenderPipeline,
    particle_prev_buffer: wgpu::Buffer,
    particle_prev_count: u32,
    particle_moving: bool,
    // Lesbare Entity-Bereiche, s. set_sharp_entities.
    sharp_entity_ranges: Vec<std::ops::Range<u32>>,
    // Die Tiefe des Welt-Durchgangs stehen lassen: die Bewegungsunschaerfe liest sie danach
    // (s. crate::motion_blur, depth_view).
    pub keep_depth: bool,
    // Die Tiefe wird nach dem Welt-Durchgang (und dem scharfen Durchgang nach der
    // Bewegungsunschaerfe) noch gelesen -- von den verdeckbaren Weltbeschriftungen der App.
    // Anders als keep_depth aendert das nichts daran, wo was gezeichnet wird.
    pub depth_read_later: bool,
    hand_lit_pipeline: wgpu::RenderPipeline,
    // Eigene Kamera + eigener Puffer fuer die Ego-Hand. Vanilla zeichnet die Hand in einem
    // getrennten Durchgang mit geleertem Tiefenpuffer und einer EIGENEN Projektion bei festem
    // Sichtfeld von 70 Grad (s. hand_projection) -- die Hand liegt damit immer vor der Welt und
    // sieht bei jeder FOV-Einstellung gleich aus.
    hand_camera_buffer: wgpu::Buffer,
    hand_camera_bind_group: wgpu::BindGroup,
    hand_buffer: wgpu::Buffer,
    // Unschaerfe der Hand (Motion Blur > Hand Blur, s. crate::hand_blur).
    hand_blur: crate::hand_blur::HandBlur,
    // Dritte Kamera samt Puffer fuer die Spielerfigur im Inventar (Vanillas
    // InventoryScreen.drawEntity). Eigene statt der Hand-Kamera, weil beide im selben Frame
    // gebraucht werden: die Hand liegt hinter dem geoeffneten Inventar und wird trotzdem
    // gezeichnet.
    doll_camera_buffer: wgpu::Buffer,
    doll_camera_bind_group: wgpu::BindGroup,
    doll_buffer: wgpu::Buffer,
    doll_vertex_count: u32,
    // Verzauberungs-Glint (s. glint.rs): Pipeline, Texturen + Zeitmatrix (Bindgroup 2) und
    // je ein Twin-Puffer fuer Hand, Welt-Entities und die Figur im Inventar.
    glint_pipeline: wgpu::RenderPipeline,
    glint_bind_group_layout: wgpu::BindGroupLayout,
    glint_bind_group: wgpu::BindGroup,
    glint_uniform_buffer: wgpu::Buffer,
    glint_sampler: wgpu::Sampler,
    hand_glint: crate::glint::GlintBuffer,
    entity_glint: crate::glint::GlintBuffer,
    doll_glint: crate::glint::GlintBuffer,
    hand_vertex_count: u32,
    // Partikel (s. ParticleVertex): zwei Pipelines, ein Puffer, ein eigener Atlas fuer die
    // Partikel-Sprites. Blockstaub sampelt dagegen den BLOCK-Atlas.
    particle_opaque_pipeline: wgpu::RenderPipeline,
    particle_translucent_pipeline: wgpu::RenderPipeline,
    particle_bind_group: wgpu::BindGroup,
    particle_buffer: wgpu::Buffer,
    particle_draws: ParticleDraws,
    // Niederschlag: dieselben Vertices und dieselbe Kamera wie die Partikel, aber eine
    // wiederholende Textur je Art (Regen, Schnee) und keine Mindestdeckkraft (s. fs_weather).
    // Ohne Texturen (vor set_weather_textures) faellt nichts.
    weather_pipeline: wgpu::RenderPipeline,
    weather_bind_groups: Option<[wgpu::BindGroup; 2]>,
    weather_buffer: wgpu::Buffer,
    // Vertices Regen, dann Schnee -- hintereinander im Puffer.
    weather_draws: [u32; 2],
}

// 36 Vertices pro Box (12 Dreiecke), Platz fuer 256 Entities = ~220 KB VRAM.
// Vertex-Budget fuer ALLE Entities eines Frames. Fruehere Fassung: 256 * 36, also genau 256
// einfarbige Boxen. Mit echten Modellen sind es rund 30 Kaesten pro Spieler (Grundschicht +
// Aussenlagen), also ~1080 Vertices je Figur -- damit waeren nach acht Spielern Schluss
// gewesen und der Rest waere ohne Vorwarnung verschwunden.
const ENTITY_VERTEX_CAPACITY: usize = 200_000;
// Leuchtende Entities (Glowing) ein zweites Mal als Silhouette, s. render_outline_silhouettes.
const OUTLINE_VERTEX_CAPACITY: usize = 100_000;

// Glint-Twins der Welt-Entities (s. glint.rs): nur verzauberte Items und Ruestungsteile,
// also ein Bruchteil von ENTITY_VERTEX_CAPACITY.
const ENTITY_GLINT_VERTEX_CAPACITY: usize = 50_000;

// Die Ego-Hand ist EIN Arm mit Aermel: zwei Kaesten, 12 Flaechen, 72 Vertices. Reichlich Luft
// nach oben fuer das spaeter gehaltene Item.
const HAND_VERTEX_CAPACITY: usize = 4_096;
// Spielermodell samt Ruestung, Handgegenstaenden und Umhang -- grosszuegiger als die Hand
// (dort ist es genau ein Arm), aber immer noch ein einziges Entity.
const DOLL_VERTEX_CAPACITY: usize = 8_192;

// Geisterbloecke (unbestaetigte Platzierungen, s. app::PendingBlock): ein Wuerfel sind 36
// Vertices. Mehr als ein paar Dutzend offene Platzierungen gibt es nicht -- selbst beim
// schnellsten Bauen sind die aeltesten laengst bestaetigt. Im selben Puffer stehen aber auch
// alle block_displays (s. app::display_block_vertices), und Server bauen daraus ganze
// Dekorationen -- deshalb reichlich Platz (65536 Vertices, gut 2 MB).
const GHOST_VERTEX_CAPACITY: usize = 65_536;

// Ein einzelner Block in Riss-Ueberlagerung: bei 6 Vertices je Flaeche reicht das auch fuer
// vielteilige Formen wie Zaeune oder Treppen weit aus.
const CRUMBLING_VERTEX_CAPACITY: usize = 2_048;

// Hitbox-Anzeige: 6 Vertices je Kanten-Segment (ein Band aus zwei Dreiecken), 12 Kanten je
// Box, bei gestrichelter Darstellung mehrere Segmente je Kante. 131072 Vertices sind rund
// 21800 Segmente -- gut 1800 vollstaendige Boxen, also weit mehr, als je gleichzeitig
// sichtbar sein koennen.
const HITBOX_VERTEX_CAPACITY: usize = 131_072;

// Sichtfeld und Tiefenbereich der Ego-Hand, aus Vanilla uebernommen (s. update_hand_camera):
//   GameRenderer.getFov(..., changingFov = false) -> 70 (Konstante, KEIN Einstellwert)
//   Hud-Projektion mit Nahebene 0.05 und Fernebene 100
pub const HAND_FOV_DEGREES: f32 = 70.0;
const HAND_NEAR_Z: f32 = 0.05;
const HAND_FAR_Z: f32 = 100.0;

// Die Projektion des Hand-Durchgangs. Nimmt bewusst NUR das Seitenverhaeltnis entgegen: dass
// hier kein Sichtfeld hineingereicht werden kann, ist die eigentliche Absicherung gegen den
// Fehler, den diese Funktion behebt (s. update_hand_camera).
#[must_use] 
pub fn hand_projection(aspect: f32) -> glam::Mat4 {
    glam::Mat4::perspective_rh(HAND_FOV_DEGREES.to_radians(), aspect, HAND_NEAR_Z, HAND_FAR_Z)
}

impl BlockRenderer {
}

#[cfg(test)]
mod column_offset_tests;

// Die Zeichenreihenfolge ist der einzige Grund, warum Wasser vor Wasser richtig aussieht
// (ALPHA_BLENDING ohne Tiefenschreiben, s. draw_order) -- deshalb hier ohne GPU pruefbar.
// Die Reihenfolge durchsichtiger Quads ist das, woran die blaue Glaskante hing: ohne
// Tiefenschreiben blendet schlicht das zuletzt Gezeichnete oben. Die Sortierlogik selbst
// braucht keine GPU und wird deshalb hier direkt geprueft.
#[cfg(test)]
mod translucent_sort_tests;

#[cfg(test)]
mod draw_order_tests;

#[cfg(test)]
mod hand_camera_tests;

#[cfg(test)]
mod section_frustum_tests;

#[cfg(test)]
mod shader_tests;
