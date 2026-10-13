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
use crate::terrain_arena::{ArenaPage, ArenaSlot, TerrainArena};

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

pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

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

// Regen und Schnee (s. app::precipitation): ein Quad je Blockspalte um die Kamera, bei
// hoechstens 16 Bloecken Radius 33 x 33 Spalten.
pub const WEATHER_VERTEX_CAPACITY: usize = 33 * 33 * 6;

// 6 x f32 -- der augenrelative Versatz einer Spalte in ganzen Bloecken und der Nachkomma-
// Anteil des Auges (s. update_column_offsets, block.wgsl::column_relative).
const COLUMN_OFFSET_STRIDE: wgpu::BufferAddress = 24;

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
                let offset = first as wgpu::BufferAddress * std::mem::size_of::<IndexedIndirect>() as wgpu::BufferAddress;
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
        SortThread { tx, rx }
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
    hitbox_buffer: wgpu::Buffer,
    hitbox_vertex_count: u32,
    // Zuletzt gesetzte Hitbox-Parameter (s. set_hitbox_params) -- update_camera schreibt sie
    // zusammen mit der Kamera in DENSELBEN Uniform-Puffer, deshalb muessen sie hier liegen
    // und nicht in einem zweiten Puffer.
    hitbox_params: [f32; 4],
    // Blockumriss im Modus Komplementaer: eigener Durchgang nach der Welt (s. OutlineComplement).
    outline_complement: OutlineComplement,
    depth_view: wgpu::TextureView,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    atlas_bind_group: wgpu::BindGroup,
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
    // Dasselbe fuer den Shader-Pfad, beleuchtet (s. render_forward_rest).
    lit_terrain_translucent_pipeline: wgpu::RenderPipeline,
    // Tageslicht 0..1 fuer den Shader-Pfad (Kamera-Uniform params.w, s. block.wgsl).
    pub daylight: f32,
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
    // Davon die letzten, die durchscheinend laufen (s. entity_translucent_pipeline).
    entity_translucent_count: u32,
    // Leuchtende Entities noch einmal, mit ihrer Umrissfarbe je Vertex (s. outline_pipeline).
    outline_pipeline: wgpu::RenderPipeline,
    outline_buffer: wgpu::Buffer,
    outline_color_buffer: wgpu::Buffer,
    outline_vertex_count: u32,
    // Entities laufen ueber ein eigenes Shader-Paar, weil Vanillas Rot NACH dem Texturzugriff
    // beigemischt wird (s. block.wgsl::fs_entity) -- die Vertexfarbe der Block-Pipeline ist
    // ein Multiplikator und kann nur abdunkeln.
    entity_pipeline: wgpu::RenderPipeline,
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
    hand_lit_pipeline: wgpu::RenderPipeline,
    // Eigene Kamera + eigener Puffer fuer die Ego-Hand. Vanilla zeichnet die Hand in einem
    // getrennten Durchgang mit geleertem Tiefenpuffer und einer EIGENEN Projektion bei festem
    // Sichtfeld von 70 Grad (s. hand_projection) -- die Hand liegt damit immer vor der Welt und
    // sieht bei jeder FOV-Einstellung gleich aus.
    hand_camera_buffer: wgpu::Buffer,
    hand_camera_bind_group: wgpu::BindGroup,
    hand_buffer: wgpu::Buffer,
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
pub fn hand_projection(aspect: f32) -> glam::Mat4 {
    glam::Mat4::perspective_rh(HAND_FOV_DEGREES.to_radians(), aspect, HAND_NEAR_Z, HAND_FAR_Z)
}

impl BlockRenderer {
    // `atlas_rgba` ist ein quadratischer RGBA8-Block der Kantenlaenge `atlas_size` (der
    // Block-/Entity-Texturatlas). Wird einmal zur GPU hochgeladen und nie wieder geaendert.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        atlas_mips: &[Vec<u8>],
        atlas_size: u32,
        // Eigener Atlas fuer Entity-Texturen (Skins, Mob-Texturen) -- gleiche Pipeline,
        // andere Textur (s. render()). Ohne Mipmaps: Entities sind meist nah, und Vanilla
        // sampelt Entity-Texturen ebenfalls ohne Mip-Kette.
        entity_atlas_rgba: &[u8],
        entity_atlas_size: u32,
        // Partikel-Sprites (assets::ParticleAtlas), ebenfalls ohne Mip-Kette.
        particle_atlas_rgba: &[u8],
        particle_atlas_size: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("block shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("block.wgsl").into()),
        });

        let atlas_bind_group_layout = create_atlas_bind_group_layout(device);
        let (atlas_bind_group, atlas_texture) =
            create_atlas_texture_and_bind_group(device, queue, atlas_mips, atlas_size, &atlas_bind_group_layout);
        // Derselbe Layout -- deshalb laesst sich die Entity-Textur ohne zweite Pipeline
        // einfach an derselben Stelle binden.
        let (entity_bind_group, entity_atlas_texture) = create_atlas_texture_and_bind_group(
            device,
            queue,
            std::slice::from_ref(&entity_atlas_rgba.to_vec()),
            entity_atlas_size,
            &atlas_bind_group_layout,
        );

        let (particle_bind_group, _particle_atlas_texture) = create_atlas_texture_and_bind_group(
            device,
            queue,
            std::slice::from_ref(&particle_atlas_rgba.to_vec()),
            particle_atlas_size,
            &atlas_bind_group_layout,
        );

        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // Auch FRAGMENT, nicht nur VERTEX: fs_main_hitbox liest camera.params.w als
                // Hitbox-Deckkraft (s. dortigen Kommentar). Alle anderen Fragment-Shader
                // dieser Datei ignorieren das Uniform ohnehin, die zusaetzliche Sichtbarkeit
                // kostet sie nichts.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera uniform buffer"),
            size: std::mem::size_of::<CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() }],
        });

        // Zweite Kamera fuer die Ego-Hand: gleiche Struktur, gleicher Layout, eigener Inhalt
        // (s. update_hand_camera). Ein zweiter Puffer statt eines Umschreibens pro Frame,
        // weil Welt und Hand im SELBEN Encoder gezeichnet werden -- ein Umschreiben zwischen
        // den beiden Durchgaengen wuerde auch rueckwirkend die schon aufgezeichneten
        // Zeichenbefehle der Welt treffen.
        let hand_camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hand camera uniform buffer"),
            size: std::mem::size_of::<CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hand_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hand camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: hand_camera_buffer.as_entire_binding() }],
        });

        let doll_camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("doll camera uniform buffer"),
            size: std::mem::size_of::<CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let doll_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("doll camera bind group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: doll_camera_buffer.as_entire_binding() }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("block pipeline layout"),
            bind_group_layouts: &[&camera_bind_group_layout, &atlas_bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BlockVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };

        // Zweiter Vertex-Puffer, INSTANZ-Rate: ein einziger vec3 je Zeichenaufruf, der sagt,
        // wo die Spalte relativ zum Auge steht (s. block.wgsl). Instanz-Rate statt eines
        // Uniforms, weil ein Uniform mit dynamischem Versatz auf 256 Byte ausgerichtet sein
        // muesste -- fuer 12 Byte Nutzlast je Spalte waere das das Zwanzigfache an Speicher
        // und ein zusaetzlicher Bindgroup-Wechsel pro Spalte.
        let offset_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: COLUMN_OFFSET_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 5, format: wgpu::VertexFormat::Float32x3 },
            ],
        };

        // Dritter Vertex-Puffer, VERTEX-Rate: die Schadens-Faerbung je Entity-Vertex
        // (rgb + Anteil, s. block.wgsl::vs_entity). Er belegt shader_location 3, dieselbe
        // Stelle, an der die Chunk-Pipeline ihren Spalten-Versatz hat -- Entities brauchen
        // den nicht, sie kommen schon augenrelativ an.
        //
        // Pro Vertex statt pro Entity, weil alle Entities in EINEM Zeichenaufruf stecken:
        // ein Uniform waere fuer alle gleich, und ein Aufruf je Entity waere teurer als die
        // 16 Byte, die hier je Vertex dazukommen.
        let entity_tint_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x4 }],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("block pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseitige Flaechen verwerfen. Grob die Haelfte aller ausgegebenen Quads
            // zeigt vom Betrachter weg (die abgewandten Seiten jeder Chunk-Spalte) und wurde
            // bisher voll rasterisiert. Vanilla macht dasselbe: RenderPipeline.Builder hat
            // `cull.orElse(true)`, und die Terrain-Pipelines (SOLID_TERRAIN/CUTOUT_TERRAIN)
            // schalten es -- anders als Entities/Wolken -- nie ab.
            //
            // Voraussetzung ist eine EINHEITLICHE Wicklung: push_face_quad gibt alle sechs
            // Wuerfelseiten gegen den Uhrzeigersinn von aussen gesehen aus (nachgerechnet,
            // s. Test wuerfelflaechen_winden_konsistent_nach_aussen im world-Crate), und
            // push_cross_quads gibt Pflanzen seit dieser Aenderung beidseitig aus.
            //
            // front_face bleibt beim Default Ccw. Stimmt die Haendigkeit der Projektion
            // nicht wie erwartet, waere die Welt sichtbar auf links gedreht (man saehe die
            // Innenseiten) -- dann ist Some(Front) die Korrektur, nicht None.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let terrain_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Uint16x4 },
                wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Unorm8x4 },
                wgpu::VertexAttribute { offset: 12, shader_location: 2, format: wgpu::VertexFormat::Uint16x2 },
            ],
        };
        let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Wie `pipeline` (s. dort zu cull_mode und Wicklung): pack_terrain behaelt die
            // Reihenfolge der Ecken, das Index-Muster 0,1,2, 0,2,3 die Wicklung.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Shader-Pfad: gleiche Geometrie und Tiefe, aber zwei Farbziele (Albedo, Normale).
        let gbuffer_terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain gbuffer pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_terrain_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: GBUFFER_ALBEDO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_NORMAL_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_LIGHT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let quad_index_capacity = 1 << 16;
        let quad_index_buffer = create_quad_index_buffer(device, queue, quad_index_capacity);

        // Translucent-Pipeline (Wasser UND Glas): echtes Alpha-Blending (SrcAlpha/
        // OneMinusSrcAlpha) statt REPLACE -- sonst identisch zur Opak-Pipeline (gleiches
        // Vertex-Layout, gleiche Bind-Groups).
        let translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("block translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main_translucent",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseiten verwerfen wie in der Opak-Pipeline -- und wie in Vanilla, dessen
            // TRANSLUCENT-Terrain-Pipeline ebenfalls kein withCull(false) setzt.
            //
            // Das ist fuer GLAS zwingend: ohne Culling zeichnet jeder Glasblock Vorder- UND
            // Rueckseite, beide blenden, und das Glas wirkt doppelt so dicht wie in Vanilla.
            // Wasser bleibt trotzdem von beiden Seiten sichtbar (man schwimmt darunter) --
            // seine Doppelseitigkeit steckt jetzt in der GEOMETRIE statt in diesem
            // Pipeline-Zustand, s. world::mesher::push_rueckseiten. Genau so macht es
            // Vanillas FluidRenderer auch.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // KEIN Tiefenschreiben -- BEWUSSTE Abweichung von Vanilla, dessen
                // TRANSLUCENT-Pipeline die Voreinstellung `writeDepth.orElse(true)` behaelt.
                //
                // Einmal auf true gestellt und sofort wieder zurueckgenommen: Vanilla kann
                // sich Tiefenschreiben nur leisten, weil es die durchsichtigen Quads JE
                // SEKTION nach Abstand sortiert. Ohne diese Sortierung blockiert die zuerst
                // gezeichnete durchsichtige Flaeche alles dahinter -- Wasser verschwand
                // hinter Glas, obwohl man hindurchsehen koennen muss. Ordnung her, DANN
                // Tiefenschreiben; nie umgekehrt.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Wasser/Glas der Spalten im kompakten Format (s. world::terrain_vertex) -- sonst wie
        // translucent_pipeline, das die Geisterbloecke weiter im alten Format benutzen.
        let terrain_translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main_translucent",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseiten verwerfen wie in der Opak-Pipeline -- und wie in Vanilla, dessen
            // TRANSLUCENT-Terrain-Pipeline ebenfalls kein withCull(false) setzt.
            //
            // Das ist fuer GLAS zwingend: ohne Culling zeichnet jeder Glasblock Vorder- UND
            // Rueckseite, beide blenden, und das Glas wirkt doppelt so dicht wie in Vanilla.
            // Wasser bleibt trotzdem von beiden Seiten sichtbar (man schwimmt darunter) --
            // seine Doppelseitigkeit steckt jetzt in der GEOMETRIE statt in diesem
            // Pipeline-Zustand, s. world::mesher::push_rueckseiten. Genau so macht es
            // Vanillas FluidRenderer auch.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // KEIN Tiefenschreiben -- BEWUSSTE Abweichung von Vanilla, dessen
                // TRANSLUCENT-Pipeline die Voreinstellung `writeDepth.orElse(true)` behaelt.
                //
                // Einmal auf true gestellt und sofort wieder zurueckgenommen: Vanilla kann
                // sich Tiefenschreiben nur leisten, weil es die durchsichtigen Quads JE
                // SEKTION nach Abstand sortiert. Ohne diese Sortierung blockiert die zuerst
                // gezeichnete durchsichtige Flaeche alles dahinter -- Wasser verschwand
                // hinter Glas, obwohl man hindurchsehen koennen muss. Ordnung her, DANN
                // Tiefenschreiben; nie umgekehrt.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Shader-Pfad: dasselbe mit Licht je Vertex (s. block.wgsl::vs_terrain_lit).
        let lit_terrain_translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lit terrain translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_terrain_lit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[terrain_vertex_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_terrain_translucent_lit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            // Rueckseiten verwerfen wie in der Opak-Pipeline -- und wie in Vanilla, dessen
            // TRANSLUCENT-Terrain-Pipeline ebenfalls kein withCull(false) setzt.
            //
            // Das ist fuer GLAS zwingend: ohne Culling zeichnet jeder Glasblock Vorder- UND
            // Rueckseite, beide blenden, und das Glas wirkt doppelt so dicht wie in Vanilla.
            // Wasser bleibt trotzdem von beiden Seiten sichtbar (man schwimmt darunter) --
            // seine Doppelseitigkeit steckt jetzt in der GEOMETRIE statt in diesem
            // Pipeline-Zustand, s. world::mesher::push_rueckseiten. Genau so macht es
            // Vanillas FluidRenderer auch.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // KEIN Tiefenschreiben -- BEWUSSTE Abweichung von Vanilla, dessen
                // TRANSLUCENT-Pipeline die Voreinstellung `writeDepth.orElse(true)` behaelt.
                //
                // Einmal auf true gestellt und sofort wieder zurueckgenommen: Vanilla kann
                // sich Tiefenschreiben nur leisten, weil es die durchsichtigen Quads JE
                // SEKTION nach Abstand sortiert. Ohne diese Sortierung blockiert die zuerst
                // gezeichnete durchsichtige Flaeche alles dahinter -- Wasser verschwand
                // hinter Glas, obwohl man hindurchsehen koennen muss. Ordnung her, DANN
                // Tiefenschreiben; nie umgekehrt.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Geisterbloecke: eigene Pipeline, weil das halbe Alpha im Fragment-Shader festgesetzt
        // wird (fs_main_ghost) statt aus der Textur zu kommen -- ein Obsidianblock hat in der
        // Textur Alpha 1.0 und waere in der Wasser-Pipeline genauso undurchsichtig wie sonst.
        // Tiefe wird getestet, aber nicht geschrieben: der Geisterblock soll hinter echter
        // Geometrie verschwinden, aber nichts dahinter wegblockieren.
        let ghost_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("block ghost pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main_ghost",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Glint (s. glint.rs): dieselbe Geometrie wie das Item (vs_main), eigener Fragment-
        // Shader, additiv gemischt, ohne Tiefenschreiben. Tiefe LESS_EQUAL mit einem kleinen
        // Bias zur Kamera hin statt Vanillas EQUAL -- warum, steht in glint.rs. Rueckseiten
        // verworfen wie beim Item selbst, damit der Glint nur auf Sichtbarem liegt.
        let glint_bind_group_layout = crate::glint::create_bind_group_layout(device);
        let glint_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glint pipeline layout"),
            bind_group_layouts: &[&camera_bind_group_layout, &atlas_bind_group_layout, &glint_bind_group_layout],
            push_constant_ranges: &[],
        });
        let glint_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glint pipeline"),
            layout: Some(&glint_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_glint",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        // BlendFunction.GLINT: Alpha (ZERO, ONE) -- das Ziel behaelt sein Alpha.
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState { constant: -2, slope_scale: -1.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let glint_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glint uniform buffer"),
            size: std::mem::size_of::<crate::glint::GlintUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glint_sampler = crate::glint::create_sampler(device);
        // Bis die App die echten Texturen setzt (set_glint_textures): weiss.
        let glint_bind_group = create_glint_bind_group(
            device,
            &glint_bind_group_layout,
            &crate::glint::upload_texture(device, queue, &[255; 4], 1, 1),
            &crate::glint::upload_texture(device, queue, &[255; 4], 1, 1),
            &glint_sampler,
            &glint_uniform_buffer,
        );

        // Hitbox-Anzeige (Modul, s. app::build_hitbox_vertices): eigenes Vertex-Format
        // (HitboxVertex) und eigene Entry-Points -- die Kanten kommen als ENDPUNKT-PAARE an
        // und werden erst nach der Projektion auf Pixelbreite gebracht (s. vs_hitbox).
        //
        // Tiefe testen, aber nicht schreiben (wie ghost_pipeline): die Anzeige verschwindet
        // hinter echten Bloecken, verdeckt aber nichts hinter sich.
        //
        // KEIN Backface-Culling: nach der Verbreiterung im Bildschirmraum haengt die
        // Wicklungsrichtung des Bandes davon ab, aus welcher Richtung man die Kante sieht --
        // mit Culling waere jede Kante aus der halben Blickrichtung unsichtbar.
        let hitbox_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<HitboxVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        };
        // Entity-Pipeline: BIS AUF den Tint-Puffer (statt des Spalten-Versatzes) und das
        // eigene Shader-Paar identisch zur Block-Pipeline -- gleiche Bind-Groups, gleiches
        // Culling, gleiches Tiefenverhalten, kein Blending. Entities liefen vorher ueber die
        // Block-Pipeline; alles ausser der Faerbung soll sich deshalb nicht aendern.
        let entity_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entity pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Silhouetten leuchtender Entities: dieselben Vertices und Bindungen wie die Entities,
        // aber in ein eigenes Bild (s. entity_outline.rs) -- ohne Tiefe, damit der Umriss durch
        // Waende sichtbar ist, und beidseitig, weil nur die Flaeche zaehlt.
        let outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entity outline silhouette pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_entity_outline",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: crate::entity_outline::FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Wie die Entity-Pipeline, aber gemischt (RenderLayers.entityTranslucent).
        let entity_translucent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("entity translucent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_entity_translucent",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Shader-Pfad: dieselben beiden Entity-Pipelines mit Licht je Vertex aus einem dritten
        // Puffer (s. entity_light_buffer).
        let entity_light_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<u32>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute { offset: 0, shader_location: 4, format: wgpu::VertexFormat::Uint32 }],
        };
        let lit_entity = |label: &str, fs: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_entity_lit",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[vertex_buffer_layout.clone(), entity_tint_layout.clone(), entity_light_layout.clone()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: fs,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let lit_entity_pipeline = lit_entity("lit entity pipeline", "fs_entity_lit", wgpu::BlendState::REPLACE);
        let lit_entity_translucent_pipeline = lit_entity("lit entity translucent pipeline", "fs_entity_translucent_lit", wgpu::BlendState::ALPHA_BLENDING);
        // Shader-Pfad: deckende Geisterbloecke in den G-Buffer, damit ein gerade gesetzter Block
        // genauso beleuchtet wird (Sonne, Schatten) wie danach sein Chunk-Mesh.
        let gbuffer_ghost_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ghost gbuffer pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_block_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), entity_light_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_terrain_gbuffer",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: GBUFFER_ALBEDO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_NORMAL_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: GBUFFER_LIGHT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Shader-Pfad: die Ego-Hand wie die Block-Pipeline, nur mit Licht und Tonemap im
        // Fragment-Shader (das Licht steht im Hand-Kamera-Uniform, s. update_hand_camera).
        let hand_lit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lit hand pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_hand_lit",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Riss-Ueberlagerung beim Abbauen -- 1:1 Vanillas RenderPipelines.CRUMBLING:
        // multiplikativ gemischt (DST_COLOR/SRC_COLOR, also 2*Quelle*Ziel, dunkle Risse
        // dunkeln ab), keine Tiefe geschrieben und ein Tiefen-Bias (-1.0 Steigung,
        // -10.0 konstant), damit die Risse vor der Blockflaeche liegen statt mit ihr zu
        // flackern. Vertex-Seite ist die gewoehnliche Block-Pipeline (die Vertices kommen
        // schon augenrelativ, der Instanz-Versatz ist null).
        let crumbling_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crumbling pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_buffer_layout.clone(), offset_buffer_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_crumbling",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Dst,
                            dst_factor: wgpu::BlendFactor::Src,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState { constant: -10, slope_scale: -1.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let outline_complement = OutlineComplement::new(device, &shader, &camera_bind_group_layout, hitbox_vertex_layout.clone(), surface_format);
        let hitbox_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hitbox pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_hitbox",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[hitbox_vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_hitbox",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Partikel: kein Backface-Culling (ein Billboard hat keine Rueckseite, aber die
        // Wicklung haengt an der Kameradrehung), Tiefe wird getestet. Die opake Variante
        // schreibt Tiefe wie Vanillas OPAQUE_PARTICLE, die durchscheinende nicht -- ungeordnet
        // gemischte Quads wuerden sich sonst gegenseitig wegschneiden (dieselbe Abwaegung wie
        // beim Wasser, s. translucent_pipeline).
        let particle_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 7]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };
        let weather_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<WeatherVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 28, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 36, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
            ],
        };
        let quad_pipeline = |label: &str, vertex: (&str, &wgpu::VertexBufferLayout), entry: &str, blend: wgpu::BlendState, write_depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: vertex.0,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: std::slice::from_ref(vertex.1),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: entry,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: write_depth,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let particle_pipeline = |label: &str, entry: &str, blend: wgpu::BlendState, write_depth: bool| {
            quad_pipeline(label, ("vs_particle", &particle_vertex_layout), entry, blend, write_depth)
        };
        let particle_opaque_pipeline = particle_pipeline("particle opaque pipeline", "fs_particle_opaque", wgpu::BlendState::REPLACE, true);
        let particle_translucent_pipeline =
            particle_pipeline("particle translucent pipeline", "fs_particle_translucent", wgpu::BlendState::ALPHA_BLENDING, false);
        let weather_pipeline =
            quad_pipeline("weather pipeline", ("vs_weather", &weather_vertex_layout), "fs_weather", wgpu::BlendState::ALPHA_BLENDING, false);
        let weather_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("weather vertex buffer"),
            size: (std::mem::size_of::<WeatherVertex>() * WEATHER_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let particle_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle vertex buffer"),
            size: (std::mem::size_of::<ParticleVertex>() * PARTICLE_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_view = create_depth_view(device, width, height);

        // Ein paar wenige Bloecke gleichzeitig -- mehr als eine Handvoll unbestaetigter
        // Platzierungen gibt es nicht, dafuer reicht ein kleiner, fest vergebener Puffer.
        let ghost_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ghost vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * GHOST_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let entity_tint_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity tint buffer"),
            size: (std::mem::size_of::<[f32; 4]>() * ENTITY_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ghost_light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ghost light buffer"),
            size: (std::mem::size_of::<u32>() * GHOST_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let entity_light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity light buffer"),
            size: (std::mem::size_of::<u32>() * ENTITY_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let outline_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity outline vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * OUTLINE_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let outline_color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity outline color buffer"),
            size: (std::mem::size_of::<[f32; 4]>() * OUTLINE_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let entity_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("entity vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * ENTITY_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let crumbling_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("crumbling vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * CRUMBLING_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let hitbox_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hitbox vertex buffer"),
            size: (std::mem::size_of::<HitboxVertex>() * HITBOX_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let hand_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hand vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * HAND_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let doll_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("doll vertex buffer"),
            size: (std::mem::size_of::<BlockVertex>() * DOLL_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            terrain_pipeline,
            quad_index_buffer,
            quad_index_capacity,
            translucent_pipeline,
            ghost_pipeline,
            ghost_buffer,
            ghost_vertex_count: 0,
            ghost_solid_count: 0,
            ghost_translucent_count: 0,
            crumbling_pipeline,
            crumbling_buffer,
            crumbling_vertex_count: 0,
            hitbox_pipeline,
            hitbox_buffer,
            hitbox_vertex_count: 0,
            hitbox_params: [0.0, HITBOX_MIN_RASTER_HALF_PX, width as f32, height as f32],
            outline_complement,
            depth_view,
            camera_buffer,
            camera_bind_group,
            atlas_bind_group,
            entity_bind_group,
            vanilla: crate::vanilla_draw::VanillaDraw::new(device, queue, surface_format, &entity_atlas_texture),
            entity_atlas_texture,
            atlas_texture: std::sync::Arc::new(atlas_texture),
            atlas_upload_buffer: None,
            atlas_bind_group_layout,
            meshes: HashMap::default(),
            terrain_arena: TerrainArena::new(device),
            terrain_draws: IndirectDraws::new(device),
            translucent_arena: TerrainArena::new(device),
            translucent_draws: IndirectDraws::new(device),
            terrain_translucent_pipeline,
            lit_terrain_translucent_pipeline,
            daylight: 1.0,
            vanilla_game_time: 0.0,
            // Beides noetig: mehrere Aufrufe aus einem Puffer, und first_instance != 0 (daran
            // haengt der Spalten-Versatz). Auf Vulkan-Desktop-Treibern praktisch immer da.
            multi_draw: device.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE),
            gbuffer_terrain_pipeline,
            occlusion: OcclusionCulling::new(),
            cave_culling: true,
            cull_radius: 32,
            culling_stats: (0, 0),
            translucent_meshes: HashMap::default(),
            draw_order: Vec::new(),
            translucent_order: Vec::new(),
            order_center: None,
            order_dirty: false,
            translucent_sort_pos: None,
            column_offset_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("column offsets"),
                size: COLUMN_OFFSET_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            column_offset_capacity: 1,
            column_offset_scratch: Vec::new(),
            zero_offset_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zero column offset"),
                size: COLUMN_OFFSET_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sort_dirty: HashSet::default(),
            sort_thread: SortThread::spawn(),
            sort_in_flight_quads: 0,
            translucent_generation: 0,
            entity_buffer,
            entity_tint_buffer,
            entity_vertex_count: 0,
            entity_translucent_count: 0,
            outline_pipeline,
            outline_buffer,
            outline_color_buffer,
            outline_vertex_count: 0,
            entity_pipeline,
            entity_translucent_pipeline,
            entity_light_buffer,
            entity_light_count: 0,
            lit_entity_pipeline,
            lit_entity_translucent_pipeline,
            hand_light: None,
            hand_lit_pipeline,
            ghost_light_buffer,
            ghost_light_count: 0,
            gbuffer_ghost_pipeline,
            hand_camera_buffer,
            hand_camera_bind_group,
            hand_buffer,
            doll_camera_buffer,
            doll_camera_bind_group,
            doll_buffer,
            doll_vertex_count: 0,
            glint_pipeline,
            glint_bind_group_layout,
            glint_bind_group,
            glint_uniform_buffer,
            glint_sampler,
            hand_glint: crate::glint::GlintBuffer::new(device, "hand glint buffer", HAND_VERTEX_CAPACITY),
            entity_glint: crate::glint::GlintBuffer::new(device, "entity glint buffer", ENTITY_GLINT_VERTEX_CAPACITY),
            doll_glint: crate::glint::GlintBuffer::new(device, "doll glint buffer", DOLL_VERTEX_CAPACITY),
            hand_vertex_count: 0,
            particle_opaque_pipeline,
            particle_translucent_pipeline,
            particle_bind_group,
            particle_buffer,
            particle_draws: ParticleDraws::default(),
            weather_pipeline,
            weather_bind_groups: None,
            weather_buffer,
            weather_draws: [0; 2],
        }
    }

    // textures/environment/rain.png und snow.png als RGBA8 (Breite, Hoehe). Neu gesetzt auch
    // nach einem Resourcepack-Wechsel. Die Textur wiederholt sich in beide Richtungen: ein
    // Quad ist eine Bildbreite breit und je Block ein Viertel Bildhoehe hoch, und die
    // Fallbewegung schiebt die UVs endlos weiter.
    pub fn set_weather_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rain: (&[u8], u32, u32), snow: (&[u8], u32, u32)) {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("weather sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let gruppe = |(rgba, w, h): (&[u8], u32, u32)| {
            let view = crate::glint::upload_texture(device, queue, rgba, w, h);
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("weather bind group"),
                layout: &self.atlas_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            })
        };
        self.weather_bind_groups = Some([gruppe(rain), gruppe(snow)]);
    }

    // Die Niederschlags-Quads dieses Frames: erst `rain` Vertices Regen, dann `snow` Schnee.
    pub fn set_weather_vertices(&mut self, queue: &wgpu::Queue, vertices: &[WeatherVertex], rain: u32, snow: u32) {
        let n = vertices.len().min(WEATHER_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.weather_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        let rain = rain.min(n as u32);
        self.weather_draws = [rain, snow.min(n as u32 - rain)];
    }

    // Resourcepack-Wechsel: der Partikel-Atlas kann dabei die Groesse aendern, deshalb eine neue
    // Textur samt Bindgroup statt eines write_texture in die alte.
    pub fn rebuild_particle_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8], size: u32) {
        let (bind_group, _texture) =
            create_atlas_texture_and_bind_group(device, queue, std::slice::from_ref(&rgba.to_vec()), size, &self.atlas_bind_group_layout);
        self.particle_bind_group = bind_group;
    }

    // Die Partikel-Quads dieses Frames. `draws` teilt `vertices` in die drei hintereinander
    // liegenden Bereiche auf (s. ParticleDraws); was ueber die Kapazitaet hinausgeht, faellt
    // von hinten weg.
    pub fn set_particle_vertices(&mut self, queue: &wgpu::Queue, vertices: &[ParticleVertex], draws: ParticleDraws) {
        let n = vertices.len().min(PARTICLE_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.particle_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        let mut rest = n as u32;
        let mut take = |want: u32| {
            let got = want.min(rest);
            rest -= got;
            got
        };
        self.particle_draws = ParticleDraws { opaque: take(draws.opaque), translucent: take(draws.translucent), block: take(draws.block) };
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.depth_view = create_depth_view(device, width, height);
    }

    // Kompletter Atlas-Austausch (Skin-Tab "Laden", s. App::reload_resource_pack) -- anders
    // als upload_atlas_tiles ersetzt das die GANZE Textur samt Mip-Kette, weil sich die
    // Kachelgroesse (und damit die Atlas-Kantenlaenge) durch ein anderes Resourcepack aendern
    // kann (HD statt 16px). Baut GEGEN DENSELBEN atlas_bind_group_layout wie beim ersten Mal
    // (s. dortigen Feldkommentar) -- die Pipelines bleiben dadurch weiter gueltig, es muss
    // nichts neu kompiliert werden. Bereits gemeshte Chunk-Vertexbuffer bleiben ebenfalls
    // gueltig: ihre UV-Bruchteile sind relativ (0..1) und haengen nur von der Kachel-INDEX-
    // Reihenfolge ab, nicht von tile_size/Atlasgroesse (s. assets::blocks::BlockAtlas::build,
    // uv_of-Kommentar) -- der Aufrufer mesht trotzdem sicherheitshalber alles neu (falls das
    // neue Pack zusaetzlich Modelle/Blockstates aendert, was die Index-Reihenfolge verschieben
    // koennte).
    pub fn rebuild_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas_mips: &[Vec<u8>], atlas_size: u32) {
        let (atlas_bind_group, atlas_texture) =
            create_atlas_texture_and_bind_group(device, queue, atlas_mips, atlas_size, &self.atlas_bind_group_layout);
        self.atlas_bind_group = atlas_bind_group;
        self.atlas_texture = std::sync::Arc::new(atlas_texture);
    }

    // Mehrere Atlas-Kacheln auf einmal ersetzen: je Kachel eine Liste von Mip-Stufen (`[0]` =
    // volle Groesse, beliebig viele Stufen -- Wasser nimmt nur Stufe 0), Ursprung und Kantenlaenge.
    //
    // EIN write_buffer plus Kopierbefehle statt je Kachel und Stufe ein write_texture: wgpu legt
    // fuer jedes write_texture einen eigenen Staging-Puffer an. Mit ~50 animierten Kacheln und
    // je sechs Stufen waren das bei gleichzeitigem Framewechsel ueber hundert Aufrufe und
    // mehrere Millisekunden in einem Frame -- im Frame-Profiler als Ausreisser alle 100 ms.
    pub fn upload_atlas_tiles(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tiles: &[(&[Vec<u8>], (u32, u32), u32)]) {
        upload_tiles(device, queue, &self.atlas_texture, &mut self.atlas_upload_buffer, tiles);
    }

    // Der Atlas selbst -- fuer den Animations-Thread der App, der seine Kacheln direkt
    // hochlaedt (s. upload_tiles).
    pub fn atlas_texture_shared(&self) -> std::sync::Arc<wgpu::Texture> {
        std::sync::Arc::clone(&self.atlas_texture)
    }

    // Setzt (oder ersetzt) die beiden Meshes (opak + Wasser-Translucent, s. world::mesher::
    // mesh_column-Kommentar) einer Chunk-Spalte. Ein leeres Mesh entfernt den jeweiligen
    // Eintrag -- sonst blieben Spalten ohne sichtbare Flaechen als leere Buffer liegen.
    pub fn set_column_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: (i32, i32),
        vertices: &[TerrainVertex],
        groups: [FaceGroup; FACE_GROUPS],
        runs: Vec<SectionRun>,
        visibility: Vec<world::visibility::SectionVis>,
        min_y: i32,
        translucent_vertices: &[TerrainVertex],
    ) {
        self.occlusion.set_column(key, visibility, min_y);
        self.set_terrain_mesh(device, queue, key, vertices, groups, runs, min_y);
        if let Some(old) = self.translucent_meshes.remove(&key) {
            self.translucent_arena.remove(old.slot);
        }
        self.ensure_quad_indices(device, queue, translucent_vertices.len().div_ceil(4) as u32);
        if let Some(slot) = self.translucent_arena.insert(device, queue, translucent_vertices) {
            self.translucent_generation += 1;
            let generation = self.translucent_generation;
            let mesh = TranslucentMesh { slot, offset_slot: 0, min_y, cpu_vertices: translucent_vertices.to_vec(), generation, in_flight: false };
            self.translucent_meshes.insert(key, mesh);
        }
        // Neue Geometrie ist unsortiert -- gezielt nur DIESE Spalte vormerken. (Frueher stand
        // hier translucent_sort_pos = None, was die Sortierung aller Spalten neu angestossen
        // hat. Mit dem Frame-Budget waere das fatal: waehrend Chunks streamen kommt praktisch
        // jeden Frame ein neues Mesh an, die Runde finge staendig von vorn an und die
        // entfernteren Spalten kaemen nie dran.)
        self.sort_dirty.insert(key);
        // Schluesselmenge kann sich geaendert haben (leere Vertices entfernen den Eintrag,
        // s. set_mesh) -- Reihenfolge neu aufbauen, s. draw_order.
        self.order_dirty = true;
    }

    // Sortiert die durchsichtigen Quads JEDER Spalte von HINTEN nach VORN, bezogen auf die
    // Kameraposition, und laedt sie neu hoch.
    //
    // WARUM das noetig ist: der Translucent-Pass schreibt keine Tiefe (s.
    // translucent_pipeline), also entscheidet allein die Zeichenreihenfolge, welche Flaeche
    // oben blendet. Der Mesher gibt die Zellen einer Spalte in fester Rasterreihenfolge aus
    // (aufsteigend nach z) -- diese Reihenfolge ist damit aus EINER Blickrichtung zufaellig
    // richtig und aus der Gegenrichtung zwangslaeufig falsch. Der Fehler war genau
    // das: nach Norden immer richtig, nach Sueden immer falsch,
    // sichtbar als blaue Glaskante an einem Wasserbecken.
    //
    // Nur die durchsichtige Geometrie wird sortiert -- sie ist ein winziger Bruchteil des
    // Gelaendes. Opake Spalten haben gar keine CPU-Kopie (s. ChunkMesh::cpu_vertices).
    //
    // `min_move` verhindert, dass bei jedem Frame alles neu sortiert und hochgeladen wird:
    // erst wenn sich die Kamera weit genug bewegt hat, kann sich die Reihenfolge ueberhaupt
    // sinnvoll aendern. Vanilla macht dasselbe (dort: Neusortierung, sobald die Kamera die
    // Sektion wechselt oder sich weit genug bewegt hat).
    // `camera` ist die ABSOLUTE Augenposition in der Welt (f64). Die Vertices der Spalten
    // sind dagegen spalten-lokal (s. ColumnOrigin), also wird die Kamera fuer jede Spalte in
    // deren lokalen Raum umgerechnet -- wieder in f64, damit die Differenz bei grossen
    // Weltkoordinaten nicht schon vor der Sortierung zerfaellt.
    pub fn sort_translucent(&mut self, queue: &wgpu::Queue, camera: DVec3, min_move: f64) {
        let bewegt = match self.translucent_sort_pos {
            Some(letzte) => letzte.distance_squared(camera) >= min_move * min_move,
            None => true,
        };
        if bewegt {
            self.translucent_sort_pos = Some(camera);
            self.sort_dirty.extend(self.translucent_meshes.keys().copied());
        }
        // Erst abholen, was der Sortier-Thread fertig hat, und hochladen -- veraltete
        // Ergebnisse (Spalte inzwischen neu gemesht oder entladen) fallen weg.
        let mut budget = SORT_QUADS_PER_FRAME;
        while budget > 0 {
            let Ok(job) = self.sort_thread.rx.try_recv() else { break };
            let quads = job.verts.len() / TerrainVertex::PER_QUAD;
            self.sort_in_flight_quads = self.sort_in_flight_quads.saturating_sub(quads);
            let Some(mesh) = self.translucent_meshes.get_mut(&job.key) else { continue };
            if mesh.generation != job.generation {
                continue;
            }
            self.translucent_arena.write(queue, mesh.slot, 0, &job.verts);
            mesh.cpu_vertices = job.verts;
            mesh.in_flight = false;
            budget = budget.saturating_sub(quads);
        }
        if self.sort_dirty.is_empty() {
            return;
        }

        // Abgearbeitet wird ueber MEHRERE Frames: translucent_order ist nach Abstand aufsteigend
        // sortiert (s. update_draw_order), also kommt zuerst dran, was am naechsten und damit am
        // sichtbarsten falsch waere. Unterwegs und hochgeladen wird je Frame nur ein Quad-Budget
        // -- ohne das schriebe ein grosser Ozean in EINEM Frame Hunderttausende Quads neu (32
        // Byte je Vertex, 192 je Quad): ein sichtbarer Ruckler bei jedem Schritt. Eine Spalte, die gerade unterwegs ist, bleibt
        // vorgemerkt und geht nach ihrer Rueckkehr noch einmal los.
        let order = std::mem::take(&mut self.translucent_order);
        for key in &order {
            if self.sort_in_flight_quads >= 2 * SORT_QUADS_PER_FRAME {
                break;
            }
            if !self.sort_dirty.contains(key) {
                continue;
            }
            let Some(mesh) = self.translucent_meshes.get_mut(key) else { continue };
            if mesh.in_flight {
                continue;
            }
            self.sort_dirty.remove(key);
            if mesh.cpu_vertices.len() <= TerrainVertex::PER_QUAD {
                continue; // ein einzelnes Quad kann sich nicht mit sich selbst vertauschen
            }
            let quads = mesh.cpu_vertices.len() / TerrainVertex::PER_QUAD;
            let base = DVec3::new((key.0 * SECTION_SIZE) as f64, mesh.min_y as f64, (key.1 * SECTION_SIZE) as f64);
            let job = SortJob { key: *key, generation: mesh.generation, verts: std::mem::take(&mut mesh.cpu_vertices), camera: (camera - base).as_vec3() };
            match self.sort_thread.tx.send(job) {
                Ok(()) => {
                    mesh.in_flight = true;
                    self.sort_in_flight_quads += quads;
                }
                Err(e) => mesh.cpu_vertices = e.0.verts,
            }
        }
        self.translucent_order = order;
        // Schluessel, die gar kein Mesh (mehr) haben, bleiben sonst ewig in der Menge liegen.
        self.sort_dirty.retain(|k| self.translucent_meshes.contains_key(k));
    }

    // Baut draw_order/translucent_order neu, wenn noetig. MUSS vor render() laufen, und zwar
    // VOR begin_render_pass: render() haelt danach `&'a self`-Verweise auf die Vertex-Buffer,
    // ein &mut self waere dort nicht mehr moeglich.
    pub fn update_draw_order(&mut self, center_chunk: (i32, i32)) {
        if !self.order_dirty && self.order_center == Some(center_chunk) {
            return;
        }
        self.order_center = Some(center_chunk);
        self.order_dirty = false;
        self.draw_order.clear();
        self.draw_order.extend(self.meshes.keys().copied());
        sortiere_nach_abstand(&mut self.draw_order, center_chunk);
        self.translucent_order.clear();
        self.translucent_order.extend(self.translucent_meshes.keys().copied());
        sortiere_nach_abstand(&mut self.translucent_order, center_chunk);
    }

    // Wirft die Meshes einer entladenen Spalte weg -- der ChunkMesh-Drop gibt den
    // wgpu::Buffer und damit den VRAM frei. Gegenstueck zu World::remove_column im Renderer.
    pub fn remove_column_mesh(&mut self, key: (i32, i32)) {
        if let Some(mesh) = self.meshes.remove(&key) {
            self.terrain_arena.remove(mesh.slot);
        }
        if let Some(mesh) = self.translucent_meshes.remove(&key) {
            self.translucent_arena.remove(mesh.slot);
        }
        self.occlusion.remove_column(key);
        self.sort_dirty.remove(&key);
        self.order_dirty = true;
    }

    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
        self.terrain_arena.clear();
        self.translucent_meshes.clear();
        self.translucent_arena.clear();
        self.draw_order.clear();
        self.translucent_order.clear();
        // Nicht nur die Listen leeren: order_center behaelt sonst die alte Spalte, und nach
        // einem Reconnect an derselben Position wuerde update_draw_order sich fuer fertig
        // halten und die neu geladenen Meshes nie einsortieren.
        self.order_center = None;
        self.order_dirty = true;
        self.translucent_sort_pos = None;
        self.sort_dirty.clear();
        self.occlusion.clear();
    }

    // Ueberschreibt einen zusammenhaengenden Vertex-Bereich im OPAKEN Mesh einer Spalte ab
    // `start` -- fuer das Einblenden der Eckabdunklung nach einer Platzierung (s.
    // app::AoFade). Fehlt der Mesh oder passt der Bereich nicht, passiert nichts.
    // Opakes Gelaende einer Spalte: Vertex-Puffer im kompakten Format, gezeichnet ueber den
    // gemeinsamen Index-Puffer (der dafuer notfalls waechst).
    fn set_terrain_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: (i32, i32),
        vertices: &[TerrainVertex],
        groups: [FaceGroup; FACE_GROUPS],
        runs: Vec<SectionRun>,
        min_y: i32,
    ) {
        if let Some(old) = self.meshes.remove(&key) {
            self.terrain_arena.remove(old.slot);
        }
        self.ensure_quad_indices(device, queue, vertices.len().div_ceil(4) as u32);
        if let Some(slot) = self.terrain_arena.insert(device, queue, vertices) {
            let sections = runs.iter().fold(0u64, |m, r| m | 1u64.checked_shl(r.section as u32).unwrap_or(0));
            self.meshes.insert(key, TerrainMesh { slot, groups, runs, sections, min_y, offset_slot: 0 });
        }
    }

    // Der gemeinsame Index-Puffer muss fuer `quads` Flaechen reichen.
    fn ensure_quad_indices(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, quads: u32) {
        if quads > self.quad_index_capacity {
            self.quad_index_capacity = quads.next_power_of_two();
            self.quad_index_buffer = create_quad_index_buffer(device, queue, self.quad_index_capacity);
        }
    }

    pub fn write_column_vertices(&self, queue: &wgpu::Queue, key: (i32, i32), start: u32, vertices: &[TerrainVertex]) {
        let Some(mesh) = self.meshes.get(&key) else { return };
        self.terrain_arena.write(queue, mesh.slot, start, vertices);
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    // Gesamtgroesse aller Chunk-Vertex-Buffer in Byte -- zum Mitschauen, wie teuer die
    // aktuelle Sichtweite wirklich ist (der Face-Culling-Mesher ist nicht sparsam).
    pub fn vertex_bytes(&self) -> usize {
        self.meshes.values().map(|m| m.slot.len as usize).sum::<usize>() * std::mem::size_of::<TerrainVertex>()
            + self.translucent_meshes.values().map(|m| m.slot.len as usize).sum::<usize>() * std::mem::size_of::<TerrainVertex>()
    }

    // Entity-Boxen fuer diesen Frame setzen (Vertices in Weltkoordinaten, wie die
    // Chunk-Meshes). Wird jeden Frame aufgerufen; ein leerer Slice blendet alle aus.
    // Einen nachgeladenen Spieler-Skin in einen reservierten Platz des Entity-Atlas
    // schreiben (s. assets::entity_atlas). Nur dieser Bereich wird ueberschrieben, die
    // Geometrie und ihre UVs bleiben unveraendert -- deshalb braucht es keinen Atlas-Neubau
    // und keinen zweiten Draw-Call.
    pub fn upload_skin(&self, queue: &wgpu::Queue, origin: (u32, u32), size: u32, rgba: &[u8]) {
        self.upload_entity_region(queue, origin, size, size, rgba);
    }

    // Umhaenge sind 64x32, nicht quadratisch -- deshalb eine Fassung mit getrennter Breite
    // und Hoehe (s. assets::entity_atlas::CAPE_WIDTH/CAPE_HEIGHT).
    pub fn upload_cape(&self, queue: &wgpu::Queue, origin: (u32, u32), width: u32, height: u32, rgba: &[u8]) {
        self.upload_entity_region(queue, origin, width, height, rgba);
    }

    // Item-Texturen (das, was Spieler in den Haenden halten). Wie beim Umhang mit getrennter
    // Breite und Hoehe, weil eine 16x16-Textur einen 32er-Platz nur teilweise belegt -- mit
    // der Platzgroesse statt der Bildgroesse wuerde write_texture ueber das Bild hinauslesen.
    pub fn upload_item(&self, queue: &wgpu::Queue, origin: (u32, u32), width: u32, height: u32, rgba: &[u8]) {
        self.upload_entity_region(queue, origin, width, height, rgba);
    }

    // Die GANZE Entity-Textur neu schreiben -- nach einem Resourcepack-Wechsel. Ohne das
    // blieben Ruestung, Schild, Mob-Texturen und Betten auf dem ALTEN Pack stehen: der Atlas
    // wird zwar neu gebaut, seine Pixel lagen aber seit dem Start unveraendert auf der GPU.
    // Schlimmer noch als "altes Bild": bringt das Pack eine andere Texturgroesse mit,
    // verschiebt sich die Packung, und die neuen Rechtecke zeigen in fremde Pixel.
    //
    // Die reservierten Nachladeplaetze (Skins, Umhaenge, Handgegenstaende) sind danach LEER --
    // der Aufrufer muss sie fuer ungueltig erklaeren, damit sie neu angefordert werden.
    pub fn rebuild_entity_atlas(&self, queue: &wgpu::Queue, rgba: &[u8], size: u32) {
        if rgba.len() < (size * size * 4) as usize {
            return;
        }
        self.upload_entity_region(queue, (0, 0), size, size, rgba);
    }

    fn upload_entity_region(&self, queue: &wgpu::Queue, origin: (u32, u32), width: u32, height: u32, rgba: &[u8]) {
        if rgba.len() < (width * height * 4) as usize {
            return;
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.entity_atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: origin.0, y: origin.1, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }

    // Kamera fuer den Hand-Durchgang. Sichtmatrix = Einheitsmatrix: die Hand wird direkt im
    // SICHTraum aufgebaut (Kamera im Ursprung, Blick entlang -Z), genau wie in Vanilla, wo der
    // Matrix-Stack fuer die Hand mit der invertierten Sichtmatrix beginnt.
    //
    // Das Sichtfeld der Hand ist FEST und hat mit der Einstellung nichts zu tun. Vanilla
    // setzt die Hud-Projektion mit getFov(..., changingFov = false),
    // und getFov gibt bei `changingFov == false` schlicht die Konstante 70 zurueck -- der
    // Einstellwert wird dort ueberhaupt nicht gelesen (GameRenderer, 1.21.11). Die Hand sieht
    // deshalb bei FOV 30 wie bei FOV 110 gleich aus.
    //
    // Frueher stand hier der eingestellte Wert. Bei 70 (Standard) faellt das nicht auf, ab
    // etwa 90 aufwaerts wird der Arm zunehmend gestreckt und wandert aus dem Bild.
    //
    // Projektionsmodus fest 0 (normal): die Warp-Projektionen dieses Clients (Panini &Co)
    // gelten fuer die Welt, nicht fuer die Hand -- Vanilla kennt dort ueberhaupt nur die
    // gewoehnliche Perspektive.
    pub fn update_hand_camera(&self, queue: &wgpu::Queue, aspect: f32) {
        let fov = HAND_FOV_DEGREES.to_radians();
        let proj = hand_projection(aspect);
        let uniform = CameraUniform {
            view: glam::Mat4::IDENTITY.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            params: [0.0, (fov * 0.5).tan(), aspect, self.daylight],
            // Die Hand zeichnet nie Hitbox-Kanten. Im Shader-Pfad steht hier ihr Licht:
            // x = gepackter Lichtwert (Bits als f32), s. block.wgsl::fs_hand_lit.
            hitbox: [f32::from_bits(self.hand_light.unwrap_or(0)), 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.hand_camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn set_hand_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let n = vertices.len().min(HAND_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.hand_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.hand_vertex_count = n as u32;
    }

    // Eigener Durchgang NACH der Welt, mit geleertem Tiefenpuffer (s. Feldkommentar) und
    // eigener Kamera. Die Hand nutzt dieselbe Pipeline und denselben Entity-Atlas wie die
    // Spielermodelle -- sie IST ein Stueck Spielermodell.
    pub fn render_hand<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.hand_vertex_count == 0 {
            return;
        }
        pass.set_pipeline(if self.hand_light.is_some() { &self.hand_lit_pipeline } else { &self.pipeline });
        pass.set_bind_group(0, &self.hand_camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        let bytes = self.hand_vertex_count as u64 * std::mem::size_of::<BlockVertex>() as u64;
        pass.set_vertex_buffer(0, self.hand_buffer.slice(..bytes));
        pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
        pass.draw(0..self.hand_vertex_count, 0..1);
        self.draw_glint(pass, &self.hand_glint);
    }

    // Glint-Twins ueber dem eben Gezeichneten. Kamera (0) und Entity-Atlas (1) sind vom
    // Aufrufer schon gebunden -- die Maske sampelt denselben Atlas wie das Item.
    fn draw_glint<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, twins: &'a crate::glint::GlintBuffer) {
        if twins.is_empty() {
            return;
        }
        pass.set_pipeline(&self.glint_pipeline);
        pass.set_bind_group(2, &self.glint_bind_group, &[]);
        twins.draw(pass, &self.zero_offset_buffer);
    }

    // Die beiden Vanilla-Glint-Texturen (textures/misc/enchanted_glint_item.png und
    // enchanted_glint_armor.png), als RGBA8. Neu gesetzt auch nach einem Resourcepack-Wechsel.
    pub fn set_glint_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, item: (&[u8], u32, u32), entity: (&[u8], u32, u32)) {
        let item = crate::glint::upload_texture(device, queue, item.0, item.1, item.2);
        let entity = crate::glint::upload_texture(device, queue, entity.0, entity.1, entity.2);
        self.glint_bind_group =
            create_glint_bind_group(device, &self.glint_bind_group_layout, &item, &entity, &self.glint_sampler, &self.glint_uniform_buffer);
    }

    // Einmal pro Frame: die wandernde Texturmatrix (s. glint::glint_matrix). `speed` und
    // `strength` sind Vanillas Optionen glintSpeed/glintStrength (0..1).
    pub fn update_glint(&self, queue: &wgpu::Queue, ms: u64, speed: f32, strength: f32) {
        let uniform = crate::glint::GlintUniform { rows: crate::glint::glint_matrix(ms, speed), params: [strength, 0.0, 0.0, 0.0] };
        queue.write_buffer(&self.glint_uniform_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn set_hand_glint_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        self.hand_glint.set(queue, vertices);
    }

    pub fn set_entity_glint_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        self.entity_glint.set(queue, vertices);
    }

    pub fn set_doll_glint_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        self.doll_glint.set(queue, vertices);
    }

    // Kamera fuer die Spielerfigur im Inventar. `view` und `proj` kommen fertig vom Aufrufer
    // (s. app::doll_camera): Vanilla baut dort keine perspektivische Kamera, sondern eine
    // ORTHOGRAFISCHE -- die Figur im Inventar hat keine Fluchtpunkte, ein Arm vorn ist
    // genauso gross wie einer hinten.
    pub fn update_doll_camera(&self, queue: &wgpu::Queue, view: glam::Mat4, proj: glam::Mat4) {
        let uniform = CameraUniform {
            view: view.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            // params.x = Projektionsmodus 0 (normal). Die uebrigen Felder braucht nur die
            // Warp-Projektion der Welt; im Orthomodus liest der Shader sie nicht.
            params: [0.0, 1.0, 1.0, 0.0],
            hitbox: [0.0; 4],
        };
        queue.write_buffer(&self.doll_camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn set_doll_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let n = vertices.len().min(DOLL_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.doll_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.doll_vertex_count = n as u32;
    }

    pub fn has_doll(&self) -> bool {
        self.doll_vertex_count > 0
    }

    // Eigener Durchgang MIT geleertem Tiefenpuffer, wie bei der Hand: die Figur ist echte
    // 3D-Geometrie und braucht den Tiefentest gegen sich selbst (ein Arm vor dem Rumpf),
    // darf aber nichts von der Welt dahinter sehen.
    pub fn render_doll<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.doll_vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.doll_camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        let bytes = self.doll_vertex_count as u64 * std::mem::size_of::<BlockVertex>() as u64;
        pass.set_vertex_buffer(0, self.doll_buffer.slice(..bytes));
        pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
        pass.draw(0..self.doll_vertex_count, 0..1);
        self.draw_glint(pass, &self.doll_glint);
    }

    // Geometrie der unbestaetigten Platzierungen (s. app::ghost_vertices). Wird nur bei
    // Aenderung geschrieben, nicht pro Frame.
    // `solid_count` ist die Anzahl FUEHRENDER Vertices, die deckend gezeichnet werden. Der
    // Aufrufer sortiert also: erst die deckenden, dann die halbdurchsichtigen.
    // Danach `translucent_count` durchscheinende (s. ghost_translucent_count), der Rest ist
    // halbdurchsichtig.
    // `lights`: Shader-Pfad, je Vertex des deckenden Teils ein Lichtwert; leer im PvP-Pfad.
    pub fn set_ghost_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex], solid_count: usize, translucent_count: usize, lights: &[u32]) {
        let n = vertices.len().min(GHOST_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.ghost_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        let l = lights.len().min(n);
        if l > 0 {
            queue.write_buffer(&self.ghost_light_buffer, 0, bytemuck::cast_slice(&lights[..l]));
        }
        self.ghost_light_count = l as u32;
        self.ghost_vertex_count = n as u32;
        self.ghost_solid_count = solid_count.min(n) as u32;
        self.ghost_translucent_count = translucent_count.min(n - self.ghost_solid_count as usize) as u32;
    }

    // `tints` muss GENAU so lang sein wie `vertices` (ein rgb+Anteil je Vertex, s.
    // block.wgsl::vs_entity). Eine kuerzere Liste liesse die GPU ueber das Pufferende hinaus
    // lesen, deshalb wird auf die kleinere der beiden Laengen gekappt statt zu vertrauen.
    //
    // `translucent`: so viele Vertices am Ende laufen durchscheinend.
    // `lights`: Shader-Pfad, je Vertex ein Lichtwert (s. entity_light_buffer); leer im PvP-Pfad.
    pub fn set_entity_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex], tints: &[[f32; 4]], lights: &[u32], translucent: usize) {
        debug_assert_eq!(vertices.len(), tints.len(), "je Entity-Vertex genau ein Tint-Eintrag");
        let n = vertices.len().min(tints.len()).min(ENTITY_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.entity_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
            queue.write_buffer(&self.entity_tint_buffer, 0, bytemuck::cast_slice(&tints[..n]));
        }
        self.entity_light_count = 0;
        if n > 0 && lights.len() >= n {
            queue.write_buffer(&self.entity_light_buffer, 0, bytemuck::cast_slice(&lights[..n]));
            self.entity_light_count = n as u32;
        }
        self.entity_vertex_count = n as u32;
        self.entity_translucent_count = translucent.min(n) as u32;
    }

    // Silhouetten leuchtender Entities, je Vertex die Umrissfarbe (rgb, 0..1 wie in Vanilla).
    pub fn set_outline_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex], colors: &[[f32; 4]]) {
        let n = vertices.len().min(colors.len()).min(OUTLINE_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.outline_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
            queue.write_buffer(&self.outline_color_buffer, 0, bytemuck::cast_slice(&colors[..n]));
        }
        self.outline_vertex_count = n as u32;
    }

    pub fn has_outlines(&self) -> bool {
        self.outline_vertex_count > 0
    }

    // Die Silhouetten in das Umriss-Bild (Pass ohne Tiefe, Ziel entity_outline::FORMAT).
    pub fn render_outline_silhouettes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.outline_vertex_count == 0 {
            return;
        }
        let n = self.outline_vertex_count as u64;
        pass.set_pipeline(&self.outline_pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.entity_bind_group, &[]);
        pass.set_vertex_buffer(0, self.outline_buffer.slice(..n * std::mem::size_of::<BlockVertex>() as u64));
        pass.set_vertex_buffer(1, self.outline_color_buffer.slice(..n * std::mem::size_of::<[f32; 4]>() as u64));
        pass.draw(0..self.outline_vertex_count, 0..1);
    }

    pub fn set_crumbling_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let n = vertices.len().min(CRUMBLING_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.crumbling_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.crumbling_vertex_count = n as u32;
    }

    pub fn set_hitbox_vertices(&mut self, queue: &wgpu::Queue, vertices: &[HitboxVertex]) {
        let n = vertices.len().min(HITBOX_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.hitbox_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.hitbox_vertex_count = n as u32;
    }

    // Kanten des Blockumrisses im Modus Komplementaer (s. OutlineComplement); leer = keiner.
    pub fn set_outline_complement_vertices(&mut self, queue: &wgpu::Queue, vertices: &[HitboxVertex]) {
        self.outline_complement.set_vertices(queue, vertices);
    }

    // Nach dem Welt-Durchgang: das Bild kopieren und den Umriss in der Gegenfarbe darueber,
    // gegen die Tiefe der Welt. `color` ist das Ziel des Welt-Durchgangs, `color_view` eine
    // Ansicht davon im Format der Pipeline.
    pub fn encode_outline_complement(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, color: &wgpu::Texture, color_view: &wgpu::TextureView) {
        self.outline_complement.encode(device, encoder, color, color_view, &self.depth_view, &self.camera_bind_group);
    }

    // Bildschirmgroesse fuer die Hitbox-Anzeige. Landet im Kamera-Uniform (s. update_camera)
    // -- der Vertex-Shader verbreitert die Kanten erst nach der Projektion und braucht sie
    // dort. Breite und Deckkraft tragen die Vertices selbst (s. HitboxVertex).
    pub fn set_hitbox_params(&mut self, screen: (f32, f32)) {
        self.hitbox_params = [0.0, HITBOX_MIN_RASTER_HALF_PX, screen.0, screen.1];
    }

    // Versaetze aller Spalten neu berechnen -- einmal pro Frame, VOR dem Zeichnen.
    //
    // Der Versatz ist `Spaltenbasis - Auge`, gerechnet in f64 und erst danach auf f32
    // verkuerzt. Genau darin liegt der ganze Gewinn: die Differenz ist hoechstens so gross
    // wie die Sichtweite (also ein paar hundert Bloecke), waehrend die beiden Summanden auf
    // einem Server wie DonutSMP sechsstellig sind. In f32 gerechnet waere die Differenz
    // schon auf 0.015625 gerastert -- in f64 liegt der Fehler bei rund 3e-11 und ueberlebt
    // die Verkuerzung auf f32 als kleine Zahl mit voller Genauigkeit.
    //
    // Die Plaetze werden JEDEN Frame neu vergeben (nicht dauerhaft an eine Spalte gebunden):
    // Spalten kommen und gehen staendig, und eine dauerhafte Vergabe braeuchte eine
    // Freiliste, die nichts brauchbares beitraegt -- die Schleife laeuft ohnehin ueber alle
    // Spalten, um die Versaetze zu schreiben.
    //
    // Geschrieben wird der Versatz in ZWEI Teilen: `Spaltenbasis - abgerundetes Auge` (ganze
    // Bloecke, in f32 exakt) und der Nachkomma-Anteil des Auges, fuer alle Spalten derselbe.
    // Der Shader rechnet erst lokal + ganzer Teil (exakt) und zieht dann den gemeinsamen
    // Anteil ab -- eine Kante zweier Nachbarspalten ergibt so auf beiden Seiten bitgleich
    // dieselbe Zahl. Mit einem fertig gerundeten Versatz je Spalte rechnete die eine Seite
    // `16 + rund(a)`, die andere `rund(a + 16)`, die um eine Binaerstelle auseinander liegen
    // koennen: beim perfekten Geradeausgucken gab es dann oft Luecken zwischen
    // den Meshes -- entlang einer Achse liegen die Spaltengrenzen genau waagerecht bzw.
    // senkrecht im Bild, und die Abweichung fehlt dann als ganze Pixelzeile.
    pub fn update_column_offsets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, eye: DVec3) {
        let mut data = std::mem::take(&mut self.column_offset_scratch);
        data.clear();
        let mut slot = 0u32;
        let mut versatz = |key: &(i32, i32), min_y: i32| {
            data.extend_from_slice(&column_offset(*key, min_y, eye));
            slot += 1;
            slot - 1
        };
        for (key, mesh) in self.meshes.iter_mut() {
            mesh.offset_slot = versatz(key, mesh.min_y);
        }
        for (key, mesh) in self.translucent_meshes.iter_mut() {
            mesh.offset_slot = versatz(key, mesh.min_y);
        }

        if slot > self.column_offset_capacity {
            // Mit Reserve wachsen, sonst wird bei jeder neu geladenen Spalte neu angelegt.
            let neu = (slot * 2).max(64);
            self.column_offset_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("column offsets"),
                size: neu as wgpu::BufferAddress * COLUMN_OFFSET_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.column_offset_capacity = neu;
        }
        if !data.is_empty() {
            queue.write_buffer(&self.column_offset_buffer, 0, bytemuck::cast_slice(&data));
        }
        self.column_offset_scratch = data;
    }

    // Die Gelaende-Aufrufe dieses Frames festlegen: sichtbare Spalten (Sichtkegel-Test wie
    // draw_visible_columns), von VORN nach HINTEN, je Speicherseite zusammengefasst, und in den
    // indirekten Puffer schreiben. Nach update_column_offsets (braucht die Instanz-Nummern).
    //
    // `viewer` = wo die Kamera WIRKLICH sitzt (Camera::view_position) -- in der 3. Person ein
    // Stueck hinter bzw. vor dem Auge. Davon haengt ab, welche Flaechen ihre Vorderseite
    // zeigen; der Sichtkegel-Test rechnet dagegen augenrelativ wie die Geometrie.
    pub fn prepare_terrain_draws(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frustum: &Frustum, eye: DVec3, viewer: DVec3) {
        // Opak: von vorn nach hinten, je Seite gesammelt (die Reihenfolge zwischen den Seiten
        // ist fuer den Tiefentest egal, innerhalb einer Seite bleibt sie erhalten).
        if self.cave_culling {
            self.occlusion.update(viewer, self.cull_radius);
        }
        let pages = self.terrain_arena.pages.len();
        let mut per_page: Vec<Vec<IndexedIndirect>> = vec![Vec::new(); pages];
        let (mut gesamt, mut gezeichnet) = (0u32, 0u32);
        for key in &self.draw_order {
            let Some(mesh) = self.meshes.get(key) else { continue };
            gesamt += mesh.sections.count_ones();
            if !column_visible(*key, frustum, eye) {
                continue;
            }
            // Hoehlen-Culling: welche Sections dieser Spalte ueberhaupt erreichbar sind.
            let sichtbar = if self.cave_culling { self.occlusion.mask(*key) & mesh.sections } else { mesh.sections };
            // Sichtkegel je Section: eine Spalte am Bildrand ragt meist weit ueber und unter
            // das Bild hinaus. Ein Block Rand ringsum, weil Modelle (Feuer, hohe Pflanzen)
            // ueber ihre Zelle hinausreichen und ihre Flaechen der Section ihres Blocks gehoeren.
            let sichtbar = sections_in_frustum(*key, mesh.min_y, sichtbar, frustum, eye);
            if sichtbar == 0 {
                continue;
            }
            gezeichnet += sichtbar.count_ones();
            let Some(list) = per_page.get_mut(mesh.slot.page as usize) else { continue };
            // Auge spalten-lokal wie die Vertex-Positionen (x/z ab Spaltenecke, y ab min_y).
            let lokal = (viewer - DVec3::new((key.0 * SECTION_SIZE) as f64, mesh.min_y as f64, (key.1 * SECTION_SIZE) as f64)).as_vec3();
            let lokal = [lokal.x, lokal.y, lokal.z];
            let mut gruppe_sichtbar = [false; FACE_GROUPS];
            for (g, group) in mesh.groups.iter().enumerate() {
                gruppe_sichtbar[g] = group.quads > 0 && group.faces_eye(g, lokal);
            }
            // Benachbarte sichtbare Bereiche zu einem Eintrag zusammenziehen.
            let mut offen: Option<(u32, u32)> = None;
            let mut nimm = |first: u32, quads: u32, list: &mut Vec<IndexedIndirect>| {
                offen = match offen {
                    Some((f, q)) if f + q == first => Some((f, q + quads)),
                    other => {
                        if let Some((f, q)) = other {
                            list.push(IndexedIndirect {
                                index_count: q * 6,
                                instance_count: 1,
                                first_index: f * 6,
                                base_vertex: mesh.slot.start as i32,
                                first_instance: mesh.offset_slot,
                            });
                        }
                        Some((first, quads))
                    }
                };
            };
            if sichtbar == mesh.sections {
                // Ganze Spalte erreichbar: ganze Gruppen, wie ohne Hoehlen-Culling.
                for (g, group) in mesh.groups.iter().enumerate() {
                    if gruppe_sichtbar[g] {
                        nimm(group.first_quad, group.quads, list);
                    }
                }
            } else {
                for run in &mesh.runs {
                    if gruppe_sichtbar[run.group as usize] && sichtbar & 1u64.checked_shl(run.section as u32).unwrap_or(u64::MAX) != 0 {
                        nimm(run.first_quad, run.quads, list);
                    }
                }
            }
            // Letzten offenen Bereich abschliessen (ein Bereich der Laenge 0 an einer Stelle,
            // an die nichts anschliesst, schiebt ihn hinaus und wird selbst verworfen).
            nimm(u32::MAX, 0, list);
        }
        self.culling_stats = (gesamt, gezeichnet);
        self.terrain_draws.clear();
        for (page, list) in per_page.into_iter().enumerate() {
            for arg in list {
                self.terrain_draws.push(page as u32, arg);
            }
        }

        // Wasser/Glas: von HINTEN nach VORN -- Pflicht, ALPHA_BLENDING ohne Tiefenschreiben ist
        // reihenfolgeabhaengig. Ein Seitenwechsel beginnt einen neuen Lauf, so bleibt die
        // Reihenfolge auch ueber Seiten hinweg erhalten.
        self.translucent_draws.clear();
        for key in self.translucent_order.iter().rev() {
            let Some(mesh) = self.translucent_meshes.get(key) else { continue };
            if !column_visible(*key, frustum, eye) {
                continue;
            }
            // Ganz unerreichbare Spalten (alle Sections hinter Fels) zeigen auch kein Wasser.
            if self.cave_culling && self.occlusion.mask(*key) == 0 {
                continue;
            }
            let quads = mesh.slot.len.div_ceil(4);
            self.translucent_draws.push(
                mesh.slot.page,
                IndexedIndirect {
                    index_count: quads * 6,
                    instance_count: 1,
                    first_index: 0,
                    base_vertex: mesh.slot.start as i32,
                    first_instance: mesh.offset_slot,
                },
            );
        }
        if self.multi_draw {
            self.terrain_draws.upload(device, queue);
            self.translucent_draws.upload(device, queue);
        }
    }

    pub fn update_camera(&self, queue: &wgpu::Queue, camera: &Camera, aspect: f32) {
        let half_fov_tan = (camera.fov_degrees.to_radians() * 0.5).tan();
        let uniform = CameraUniform {
            view: camera.view_matrix().to_cols_array_2d(),
            proj: camera.proj_matrix(aspect).to_cols_array_2d(),
            params: [camera.projection as f32, half_fov_tan, aspect, self.daylight],
            hitbox: self.hitbox_params,
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    // Die Atlas-Textur selbst -- die App braucht sie ein zweites Mal fuer die Item-Icons im
    // Inventar (Blockitems werden als isometrischer Wuerfel aus denselben Flaechen-UVs
    // gezeichnet). Eigene Bind-Group statt dieser hier, weil die 2D-Oberflaeche einen
    // Nearest-Sampler ohne Mipmaps braucht (Pixel-Art in Vergroesserung, s. app::item_icons).
    pub fn atlas_texture(&self) -> &wgpu::Texture {
        &self.atlas_texture
    }

    // Der Entity-Atlas (Skins) -- die Tab-Liste zeichnet daraus die Koepfe als 2D-Sprites.
    pub fn entity_atlas_texture(&self) -> &wgpu::Texture {
        &self.entity_atlas_texture
    }

    pub fn depth_attachment(&self) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        // Die Tiefe bleibt nur stehen, wenn der komplementaere Blockumriss sie danach braucht.
        let store = if self.outline_complement.count > 0 { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard };
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.depth_view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store }),
            stencil_ops: None,
        }
    }

    // Welche Spalten gezeichnet werden (Sichtkegel, Richtungsgruppen, Reihenfolge), hat
    // prepare_terrain_draws schon festgelegt -- hier wird nur noch aufgezeichnet.
    // `mark` kennzeichnet fuer die GPU-Zeitmessung, wo ein Abschnitt (Gelaende, Wasser, ...)
    // anfaengt -- s. app::gpu_timer. Kostet nichts, solange niemand misst.
    pub fn render<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str),
    ) {
        if self.meshes.is_empty()
            && self.translucent_meshes.is_empty()
            && self.entity_vertex_count == 0
            && self.ghost_vertex_count == 0
            && self.hitbox_vertex_count == 0
            && self.crumbling_vertex_count == 0
            && self.particle_draws.opaque + self.particle_draws.translucent + self.particle_draws.block == 0
            && self.weather_draws == [0; 2]
        {
            return;
        }
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        self.draw_terrain(pass, &self.terrain_pipeline, mark);
        self.draw_after_terrain(pass, mark, false);
    }

    // Shader-Pfad, Teil 1: nur das opake Gelaende, in den G-Buffer (Albedo + Normale).
    pub fn render_gbuffer<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str)) {
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        self.draw_terrain(pass, &self.gbuffer_terrain_pipeline, mark);
        if self.gbuffer_ghosts() {
            mark(pass, "Sonstiges");
            let bytes = self.ghost_solid_count as u64 * std::mem::size_of::<BlockVertex>() as u64;
            let light_bytes = self.ghost_solid_count as u64 * std::mem::size_of::<u32>() as u64;
            pass.set_pipeline(&self.gbuffer_ghost_pipeline);
            pass.set_vertex_buffer(0, self.ghost_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.ghost_light_buffer.slice(..light_bytes));
            pass.draw(0..self.ghost_solid_count, 0..1);
        }
    }

    // Shader-Pfad: zeichnet render_gbuffer die deckenden Geisterbloecke (statt des Forward-
    // Rests)? Nur, wenn fuer jeden ihrer Vertices Licht da ist.
    fn gbuffer_ghosts(&self) -> bool {
        self.ghost_solid_count > 0 && self.ghost_light_count >= self.ghost_solid_count
    }

    // Shader-Pfad, Teil 2: alles nach dem Gelaende wie im PvP-Pfad (Wasser, Entities,
    // Partikel, ...), ins fertig beleuchtete Bild und gegen die Tiefe aus dem G-Buffer-Pass.
    pub fn render_forward_rest<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str)) {
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        self.draw_after_terrain(pass, mark, true);
    }

    fn draw_terrain<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
        mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str),
    ) {
        mark(pass, "Gelaende");
        pass.set_pipeline(pipeline);
        pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        // Opak von VORN nach HINTEN: verdeckte Fragmente fallen so schon im Early-Z-Test
        // raus, statt erst schattiert und dann ueberschrieben zu werden. Reihenfolge und
        // Sichtbarkeit stehen schon in terrain_draws (s. prepare_terrain_draws); der
        // Spalten-Versatz kommt ueber die Instanz-Nummer aus dem ganzen Versatz-Puffer.
        pass.set_vertex_buffer(1, self.column_offset_buffer.slice(..));
        self.terrain_draws.draw(pass, &self.terrain_arena.pages, self.multi_draw);
    }

    // Alles, was im Welt-Pass nach dem opaken Gelaende kommt. Erwartet Kamera- und
    // Block-Atlas-Bindung (Gruppen 0 und 1).
    // `lit`: Shader-Pfad -- Wasser/Glas mit Licht (s. lit_terrain_translucent_pipeline).
    fn draw_after_terrain<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str), lit: bool) {

        // Geisterbloecke zwischen Opak und Wasser: sie muessen gegen die opake Tiefe testen
        // (ein Block hinter einer Wand bleibt verdeckt), sollen aber vom Wasser noch
        // ueberblendet werden koennen.
        if self.ghost_vertex_count > 0 {
            mark(pass, "Sonstiges");
            let bytes = self.ghost_vertex_count as u64 * std::mem::size_of::<BlockVertex>() as u64;
            pass.set_vertex_buffer(0, self.ghost_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
            // Der deckende Teil laeuft durch die GEWOEHNLICHE Blockpipeline -- er soll wie ein
            // echter Block aussehen, Tiefe schreiben eingeschlossen. Nur der Rest (zu lange
            // unbestaetigt) bekommt die halbdurchsichtige Geister-Pipeline.
            // Im Shader-Pfad stehen sie mit Licht schon im G-Buffer (s. render_gbuffer).
            if self.ghost_solid_count > 0 && !(lit && self.gbuffer_ghosts()) {
                pass.set_pipeline(&self.pipeline);
                pass.draw(0..self.ghost_solid_count, 0..1);
            }
            let durchscheinend_ende = self.ghost_solid_count + self.ghost_translucent_count;
            if self.ghost_translucent_count > 0 {
                pass.set_pipeline(&self.translucent_pipeline);
                pass.draw(self.ghost_solid_count..durchscheinend_ende, 0..1);
            }
            if durchscheinend_ende < self.ghost_vertex_count {
                pass.set_pipeline(&self.ghost_pipeline);
                pass.draw(durchscheinend_ende..self.ghost_vertex_count, 0..1);
            }
        }

        // Risse ueber dem abgebauten Block -- nach allem Opaken (sie mischen sich mit dessen
        // Farbe), aber vor Wasser und Partikeln. Vanilla zeichnet sie an derselben Stelle
        // (LevelRenderer: "destroyProgress" nach dem Terrain-Pass).
        if self.crumbling_vertex_count > 0 {
            mark(pass, "Sonstiges");
            pass.set_pipeline(&self.crumbling_pipeline);
            let bytes = self.crumbling_vertex_count as u64 * std::mem::size_of::<BlockVertex>() as u64;
            pass.set_vertex_buffer(0, self.crumbling_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
            pass.draw(0..self.crumbling_vertex_count, 0..1);
        }

        // Opake Partikel vor dem Wasser: sie schreiben Tiefe wie jede andere opake Geometrie.
        let d = self.particle_draws;
        let particle_bytes = |from: u32, count: u32| {
            let size = std::mem::size_of::<ParticleVertex>() as u64;
            self.particle_buffer.slice(from as u64 * size..(from + count) as u64 * size)
        };
        if d.opaque > 0 {
            mark(pass, "Partikel");
            pass.set_pipeline(&self.particle_opaque_pipeline);
            pass.set_bind_group(1, &self.particle_bind_group, &[]);
            pass.set_vertex_buffer(0, particle_bytes(0, d.opaque));
            pass.draw(0..d.opaque, 0..1);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        }

        // Wasser NACH allem Opaken (s. translucent_pipeline-Kommentar): eigener
        // Pipeline-Wechsel, sonst identischer Ablauf (dieselben Bind-Groups gelten weiter).
        if !self.translucent_meshes.is_empty() {
            mark(pass, "Wasser/Glas");
            pass.set_pipeline(if lit { &self.lit_terrain_translucent_pipeline } else { &self.terrain_translucent_pipeline });
            // Von HINTEN nach VORN (s. prepare_terrain_draws). Index- und Versatz-Puffer muessen
            // neu gesetzt werden: Geister, Risse und Partikel davor binden eigene Puffer.
            pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_vertex_buffer(1, self.column_offset_buffer.slice(..));
            self.translucent_draws.draw(pass, &self.translucent_arena.pages, self.multi_draw);
        }

        if self.entity_vertex_count > 0 {
            mark(pass, "Entities");
            // NACH dem Wasser: das mischt ohne Tiefe zu schreiben und hat Spieler und Mobs
            // darunter/dahinter blau eingefaerbt (Nutzerwunsch: nicht einfaerben). Die Tiefe der
            // opaken Welt verdeckt Entities weiter korrekt.
            // Entities sampeln den ENTITY-Atlas, nicht den Block-Atlas -- Skins und
            // Mob-Texturen liegen dort (s. new()). Danach wieder zurueckbinden, damit die
            // Partikel unten wieder die Blocktextur treffen.
            pass.set_bind_group(1, &self.entity_bind_group, &[]);
            // Eigene Pipeline: die Schadens-Faerbung mischt NACH dem Texturzugriff und laesst
            // sich deshalb nicht ueber die Vertexfarbe der Block-Pipeline machen
            // (s. block.wgsl::fs_entity).
            // Shader-Pfad mit Licht je Vertex, sofern es fuer alle Vertices vorliegt.
            let beleuchtet = lit && self.entity_light_count == self.entity_vertex_count;
            pass.set_pipeline(if beleuchtet { &self.lit_entity_pipeline } else { &self.entity_pipeline });
            let bytes = self.entity_vertex_count as u64 * std::mem::size_of::<BlockVertex>() as u64;
            let tint_bytes = self.entity_vertex_count as u64 * std::mem::size_of::<[f32; 4]>() as u64;
            pass.set_vertex_buffer(0, self.entity_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.entity_tint_buffer.slice(..tint_bytes));
            if beleuchtet {
                let light_bytes = self.entity_vertex_count as u64 * std::mem::size_of::<u32>() as u64;
                pass.set_vertex_buffer(2, self.entity_light_buffer.slice(..light_bytes));
            }
            let deckend = self.entity_vertex_count - self.entity_translucent_count;
            pass.draw(0..deckend, 0..1);
            if self.entity_translucent_count > 0 {
                pass.set_pipeline(if beleuchtet { &self.lit_entity_translucent_pipeline } else { &self.entity_translucent_pipeline });
                pass.draw(deckend..self.entity_vertex_count, 0..1);
            }

            // Glint verzauberter Items/Ruestung -- gleich danach, solange der Entity-Atlas
            // noch gebunden ist (die Maske sampelt ihn).
            self.draw_glint(pass, &self.entity_glint);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        }

        // Items ueber das Programm eines Resourcepacks (rendertype_item_entity_translucent_cull)
        // -- Vanilla zeichnet sie mit dem durchscheinenden Entity-Batch. Eigene Bindungen
        // (Gruppe 0 ist dort eine andere), danach die eigenen wieder herstellen.
        if self.vanilla.draw_items(pass) {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        }

        // Die Weltgrenze, immer ueber Vanillas Programm (aus dem Pack oder dem Jar). Additiv und
        // ohne Tiefe zu schreiben -- sie legt sich ueber alles Feste und Durchscheinende davor.
        if self.vanilla.draw_border(pass) {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        }

        // Hitbox-Anzeige (Modul): zeichnet gar keine Textur, die Farbe kommt aus dem Vertex
        // (s. fs_hitbox). Sie steht hier, weil die Kanten NACH der Projektion auf Pixelbreite
        // gebracht werden und dafuer gegen die fertige Tiefe der Welt testen muessen.
        // Nach dem Wasser, damit es sie nicht blau einfaerbt (wie bei den Entities oben).
        if self.hitbox_vertex_count > 0 {
            mark(pass, "Sonstiges");
            pass.set_pipeline(&self.hitbox_pipeline);
            let bytes = self.hitbox_vertex_count as u64 * std::mem::size_of::<HitboxVertex>() as u64;
            pass.set_vertex_buffer(0, self.hitbox_buffer.slice(..bytes));
            pass.draw(0..self.hitbox_vertex_count, 0..1);
        }

        // Durchscheinende Partikel zuletzt, ueber dem Wasser: sie schreiben keine Tiefe und
        // muessen deshalb auf allem liegen, was sie verdecken koennten.
        if d.translucent + d.block > 0 {
            mark(pass, "Partikel");
            pass.set_pipeline(&self.particle_translucent_pipeline);
            if d.block > 0 {
                // Blockstaub sampelt den Block-Atlas -- der ist hier noch gebunden.
                pass.set_vertex_buffer(0, particle_bytes(d.opaque + d.translucent, d.block));
                pass.draw(0..d.block, 0..1);
            }
            if d.translucent > 0 {
                pass.set_bind_group(1, &self.particle_bind_group, &[]);
                pass.set_vertex_buffer(0, particle_bytes(d.opaque, d.translucent));
                pass.draw(0..d.translucent, 0..1);
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            }
        }

        // Regen und Schnee ganz zuletzt, wie in Vanilla nach allem Durchscheinenden: sie
        // schreiben keine Tiefe und liegen damit ueber Wasser und Partikeln dahinter.
        if let Some(gruppen) = &self.weather_bind_groups {
            let [regen, schnee] = self.weather_draws;
            if regen + schnee > 0 {
                mark(pass, "Sonstiges");
                pass.set_pipeline(&self.weather_pipeline);
                pass.set_vertex_buffer(0, self.weather_buffer.slice(..));
                let mut start = 0;
                for (gruppe, anzahl) in gruppen.iter().zip([regen, schnee]) {
                    if anzahl > 0 {
                        pass.set_bind_group(1, gruppe, &[]);
                        pass.draw(start..start + anzahl, 0..1);
                    }
                    start += anzahl;
                }
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            }
        }
    }
}

// Sortiert Spaltenschluessel nach Abstand zu `center` AUFSTEIGEND (s. draw_order).
//
// Quadrierter Abstand statt Wurzel: spart die sqrt und ordnet identisch (beide Seiten >= 0).
// In i64 gerechnet, nicht i32: Chunk-Koordinaten reichen bis ~1.9 Mio (Weltgrenze 30 Mio
// Bloecke / 16), ein dx von wenigen Millionen quadriert liegt weit ueber i32::MAX. Im
// Release haette das still falsch herum sortiert, im Debug-Build panisch abgebrochen. Tritt
// nur auf, wenn weit entfernte Spalten noch geladen sind (Teleport), ist dann aber sicher.
//
// (cx, cz) als Tiebreaker, damit gleich weit entfernte Spalten -- ein ganzer Ring hat
// denselben Abstand -- eine STABILE Reihenfolge bekommen. Ohne das haenge sie wieder an der
// HashMap-Iterationsreihenfolge, und genau die Instabilitaet soll hier ja verschwinden.
fn sortiere_nach_abstand(ziel: &mut [(i32, i32)], center: (i32, i32)) {
    ziel.sort_unstable_by_key(|&(cx, cz)| {
        let dx = cx as i64 - center.0 as i64;
        let dz = cz as i64 - center.1 as i64;
        (dx * dx + dz * dz, cx, cz)
    });
}


// Liegt die Spalte (ganze Weltspanne in y) im Sichtkegel? AABB AUGENRELATIV, in f64
// gerechnet: der Sichtkegel kommt aus einer View-Matrix, die die Kamera im Ursprung sieht (s.
// Camera::render_view_position). Mit absoluten Weltkoordinaten waere der Test gegen den
// falschen Raum gelaufen -- und bei grossen Koordinaten obendrein in f32 zerfallen.
// Die Bits aus `mask`, deren Section (mit einem Block Rand) im Sichtkegel liegt.
fn sections_in_frustum((cx, cz): (i32, i32), min_y: i32, mask: u64, frustum: &Frustum, eye: DVec3) -> u64 {
    let (x0, z0) = ((cx * SECTION_SIZE - 1) as f64, (cz * SECTION_SIZE - 1) as f64);
    let rand = (SECTION_SIZE + 2) as f64;
    let mut rest = mask;
    let mut drin = 0;
    while rest != 0 {
        let s = rest.trailing_zeros();
        rest &= rest - 1;
        let y0 = (min_y + s as i32 * SECTION_SIZE - 1) as f64;
        let min = (DVec3::new(x0, y0, z0) - eye).as_vec3();
        let max = (DVec3::new(x0 + rand, y0 + rand, z0 + rand) - eye).as_vec3();
        if frustum.intersects_aabb(min, max) {
            drin |= 1 << s;
        }
    }
    drin
}

fn column_visible((cx, cz): (i32, i32), frustum: &Frustum, eye: DVec3) -> bool {
    let min = (DVec3::new((cx * SECTION_SIZE) as f64, WORLD_CULL_MIN_Y as f64, (cz * SECTION_SIZE) as f64) - eye).as_vec3();
    let max =
        (DVec3::new((cx * SECTION_SIZE + SECTION_SIZE) as f64, WORLD_CULL_MAX_Y as f64, (cz * SECTION_SIZE + SECTION_SIZE) as f64) - eye)
            .as_vec3();
    frustum.intersects_aabb(min, max)
}

fn create_indirect_buffer(device: &wgpu::Device, entries: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terrain indirect draws"),
        size: entries as wgpu::BufferAddress * std::mem::size_of::<IndexedIndirect>() as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

// Der gemeinsame Flaechen-Index-Puffer (s. BlockRenderer::terrain_pipeline): fuer `quads`
// Flaechen je 0,1,2, 0,2,3 auf die vier Vertices der Flaeche.
fn create_quad_index_buffer(device: &wgpu::Device, queue: &wgpu::Queue, quads: u32) -> wgpu::Buffer {
    let indices: Vec<u32> = (0..quads).flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i)).collect();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("quad index buffer"),
        size: (indices.len() * 4) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&indices));
    buffer
}


// Mittelpunkt eines Quads -- Sortierschluessel fuer die Reihenfolge durchsichtiger Flaechen.
// Ordnet die Quads in `verts` von HINTEN nach VORN (fernstes zuerst) -- allokationsfrei ueber
// die uebergebenen Arbeitspuffer. Sortiert werden nur (Abstand, Quad-Index)-Paare (8 Byte)
// statt Slices oder gar ganzer Quads (192 Byte).
// Ein Vertex-Format, dessen Liste aus gleich grossen Flaechen besteht (sortiere_quads).
trait QuadVertex: Copy {
    const PER_QUAD: usize;
    fn pos(&self) -> [f32; 3];
}

impl QuadVertex for BlockVertex {
    const PER_QUAD: usize = VERTS_PER_QUAD;
    fn pos(&self) -> [f32; 3] {
        self.position
    }
}

impl QuadVertex for TerrainVertex {
    const PER_QUAD: usize = 4;
    fn pos(&self) -> [f32; 3] {
        self.position()
    }
}

fn sortiere_quads<V: QuadVertex>(verts: &mut Vec<V>, camera: Vec3, keys: &mut Vec<(f32, u32)>, scratch: &mut Vec<V>) {
    keys.clear();
    keys.extend(verts.chunks_exact(V::PER_QUAD).enumerate().map(|(i, q)| (quad_center(q).distance_squared(camera), i as u32)));
    // total_cmp ist NaN-sicher; der Index als zweites Kriterium macht das Ergebnis bei exakt
    // gleichen Abstaenden eindeutig -- sonst koennte sort_unstable zwei deckungsgleiche Quads
    // von Frame zu Frame vertauschen und sichtbar flimmern lassen.
    keys.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    scratch.clear();
    scratch.reserve(verts.len());
    for &(_, i) in keys.iter() {
        let start = i as usize * V::PER_QUAD;
        scratch.extend_from_slice(&verts[start..start + V::PER_QUAD]);
    }
    // Tauschen statt kopieren: `verts` bekommt die sortierte Fassung, der Arbeitspuffer behaelt
    // die alte Allokation und ist beim naechsten Aufruf schon gross genug.
    std::mem::swap(verts, scratch);
}

fn quad_center<V: QuadVertex>(quad: &[V]) -> Vec3 {
    let mut c = Vec3::ZERO;
    for v in quad {
        c += Vec3::from(v.pos());
    }
    c / quad.len() as f32
}

// Laedt die Atlas-Mipmap-Kette als GPU-Textur hoch und baut die zugehoerige Bindgroup
// (Textur + Sampler). Die Atlas-PNG-Texel sind bereits sRGB-kodiert (Vanilla-Assets) --
// deshalb Rgba8UnormSrgb, damit die GPU beim Sampeln korrekt nach linear konvertiert und die
// Farben nicht ausgewaschen wirken.
//
// `mips[0]` ist die volle Aufloesung, jede weitere Stufe (aus assets::BlockAtlas, JE KACHEL
// EINZELN verkleinert -- kein Atlas-Bleeding, s. dortigen Kommentar) halb so gross.
fn create_glint_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    item: &wgpu::TextureView,
    entity: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glint bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(item) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(entity) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: uniform.as_entire_binding() },
        ],
    })
}

fn create_atlas_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("atlas bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

// Textur + Bindgroup GEGEN EINEN BESTEHENDEN Layout bauen (s. atlas_bind_group_layout-
// Feldkommentar) -- getrennt von create_atlas_bind_group_layout, damit ein Atlas-Rebuild
// (Skin-Tab "Laden") den Layout nicht anfassen muss, an den die Pipelines gebunden sind.
fn create_atlas_texture_and_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mips: &[Vec<u8>],
    size: u32,
    layout: &wgpu::BindGroupLayout,
) -> (wgpu::BindGroup, wgpu::Texture) {
    let mip_level_count = mips.len() as u32;
    let extent = wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("block atlas"),
        size: extent,
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        // Die Gamma-Sicht (Rgba8Unorm) brauchen Vanillas Core-Shader aus Resourcepacks: sie
        // rechnen mit den Texeln, wie sie in der Datei stehen (s. vanilla_draw).
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    for (level, level_rgba) in mips.iter().enumerate() {
        let level_size = (size >> level).max(1);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            level_rgba,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * level_size), rows_per_image: Some(level_size) },
            wgpu::Extent3d { width: level_size, height: level_size, depth_or_array_layers: 1 },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // Nearest INNERHALB einer Mip-Stufe (scharfe Minecraft-Pixel aus der Naehe, kein
    // Verwaschen einzelner Bloecke UND kein Bleeding an Kachelgrenzen, s. dortigen
    // Atlas-Bleeding-Kommentar), aber Linear ZWISCHEN zwei Mip-Stufen -- genau die
    // "GL_NEAREST_MIPMAP_LINEAR"-Kombination, die aus der Distanz weich zwischen den
    // Mip-Stufen ueberblendet, ohne aus der Naehe zu verschwimmen.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("block atlas sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("atlas bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });
    (bind_group, texture)
}

// Blockumriss in der Gegenfarbe des Bildes darunter (wie das Fadenkreuz im Modus
// Komplementaer). Ein Shader kann sein eigenes Ziel nicht lesen; deshalb nach dem Welt-Durchgang
// eine Kopie des Bildes und ein eigener kleiner Durchgang, der aus ihr liest. Getestet wird
// gegen die Tiefe der Welt (die dafuer stehen bleibt, s. depth_attachment) -- so verschwindet
// der Umriss weiter hinter Waenden. Die Kanten sind dieselben wie sonst (vs_hitbox).
const OUTLINE_COMPLEMENT_CAPACITY: usize = 16_384;

struct OutlineComplement {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    count: u32,
    format: wgpu::TextureFormat,
    // Die Bildkopie samt Bindung, je Groesse einmal angelegt.
    copy: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
}

impl OutlineComplement {
    fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        camera_layout: &wgpu::BindGroupLayout,
        vertex_layout: wgpu::VertexBufferLayout<'_>,
        format: wgpu::TextureFormat,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline complement layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline complement pipeline layout"),
            bind_group_layouts: &[camera_layout, &layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline complement pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_hitbox",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_hitbox_complement",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline complement vertex buffer"),
            size: (std::mem::size_of::<HitboxVertex>() * OUTLINE_COMPLEMENT_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, layout, buffer, count: 0, format, copy: None }
    }

    fn set_vertices(&mut self, queue: &wgpu::Queue, vertices: &[HitboxVertex]) {
        let n = vertices.len().min(OUTLINE_COMPLEMENT_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.count = n as u32;
    }

    fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::Texture,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        camera: &wgpu::BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        let size = (color.width(), color.height());
        if self.copy.as_ref().is_none_or(|(_, _, s)| *s != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("outline complement scene copy"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: color.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[self.format],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(self.format), ..Default::default() });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("outline complement bind group"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) }],
            });
            self.copy = Some((texture, bind_group, size));
        }
        let Some((kopie, bind_group, _)) = self.copy.as_ref() else { return };
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture { texture: color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyTexture { texture: kopie, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("outline complement pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        let bytes = self.count as u64 * std::mem::size_of::<HitboxVertex>() as u64;
        pass.set_vertex_buffer(0, self.buffer.slice(..bytes));
        pass.draw(0..self.count, 0..1);
    }
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

// Instanz-Daten einer Spalte (s. update_column_offsets): Spaltenecke auf Hoehe min_y (der
// Bezugspunkt der Vertex-Positionen, s. world::TerrainVertex) minus abgerundetes Auge -- ganze
// Bloecke, in f32 exakt --, dahinter der Nachkomma-Anteil des Auges.
fn column_offset(key: (i32, i32), min_y: i32, eye: DVec3) -> [f32; 6] {
    let auge_ganz = eye.floor();
    let base = DVec3::new((key.0 * SECTION_SIZE) as f64, min_y as f64, (key.1 * SECTION_SIZE) as f64);
    let off = (base - auge_ganz).as_vec3();
    let rest = (eye - auge_ganz).as_vec3();
    [off.x, off.y, off.z, rest.x, rest.y, rest.z]
}

#[cfg(test)]
mod column_offset_tests {
    use super::*;

    // Was block.wgsl::column_relative rechnet, in f32.
    fn im_shader(lokal: f32, o: [f32; 6], achse: usize) -> f32 {
        (lokal + o[achse]) - o[achse + 3]
    }

    // Die gemeinsame Kante zweier Nachbarspalten (x = 16 in der einen, x = 0 in der anderen)
    // muss auf beiden Seiten bitgleich herauskommen, sonst bleibt beim Rastern eine Pixelluecke.
    // Mit einem fertig gerundeten Versatz je Spalte (vorher) traf das fuer einen Teil der
    // Augenpositionen nicht zu -- s. update_column_offsets.
    #[test]
    fn nachbarspalten_teilen_ihre_kante_bitgleich() {
        let mut alt_daneben = 0;
        let mut zufall = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..20_000 {
            zufall ^= zufall << 13;
            zufall ^= zufall >> 7;
            zufall ^= zufall << 17;
            let eye = DVec3::new((zufall % 6_000_000) as f64 / 1000.0 - 3000.0, 70.3, 12.5);
            let cx = (eye.x / 16.0).floor() as i32 + (zufall >> 40) as i32 % 33 - 16;
            let (a, b) = (column_offset((cx, 0), -64, eye), column_offset((cx + 1, 0), -64, eye));
            assert_eq!(im_shader(16.0, a, 0).to_bits(), im_shader(0.0, b, 0).to_bits(), "Auge {eye}, Spalte {cx}");
            let alt = |key: i32| ((key * 16) as f64 - eye.x) as f32;
            alt_daneben += usize::from(16.0 + alt(cx) != alt(cx + 1));
        }
        println!("alte Rechnung daneben: {alt_daneben} von 20000");
        // Gegenprobe: die alte Rechnung lag tatsaechlich daneben.
        assert!(alt_daneben > 100, "{alt_daneben}");
    }
}

// Die Zeichenreihenfolge ist der einzige Grund, warum Wasser vor Wasser richtig aussieht
// (ALPHA_BLENDING ohne Tiefenschreiben, s. draw_order) -- deshalb hier ohne GPU pruefbar.
// Die Reihenfolge durchsichtiger Quads ist das, woran die blaue Glaskante hing: ohne
// Tiefenschreiben blendet schlicht das zuletzt Gezeichnete oben. Die Sortierlogik selbst
// braucht keine GPU und wird deshalb hier direkt geprueft.
#[cfg(test)]
mod translucent_sort_tests {
    use super::*;

    // Ein achsenparalleles Quad (6 Vertices wie push_quad_with_uv) auf Hoehe z.
    fn quad_bei(z: f32) -> Vec<BlockVertex> {
        let v = |x: f32, y: f32| BlockVertex { position: [x, y, z], color: [1.0; 3], uv: [0.0; 2] };
        vec![v(0.0, 0.0), v(0.0, 1.0), v(1.0, 1.0), v(0.0, 0.0), v(1.0, 1.0), v(1.0, 0.0)]
    }

    // Ruft die ECHTE Sortierung auf (sortiere_quads, dieselbe Funktion, die sort_translucent
    // benutzt) -- nur ohne Buffer-Upload. Frueher stand hier eine nachgebaute Kopie der
    // Rechnung; die haette jede kuenftige Aenderung an der echten Sortierung stillschweigend
    // mitbestanden, ohne sie je zu pruefen.
    fn sortiere(verts: &[BlockVertex], camera: Vec3) -> Vec<BlockVertex> {
        let mut v = verts.to_vec();
        sortiere_quads(&mut v, camera, &mut Vec::new(), &mut Vec::new());
        v
    }

    // Der Fehler in Reinform (nach Norden richtig, nach Sueden falsch): dieselbe
    // Geometrie muss aus entgegengesetzten Blickrichtungen in UMGEKEHRTER Reihenfolge
    // herauskommen. Eine feste Rasterreihenfolge kann das nicht leisten.
    #[test]
    fn gegenrichtungen_ergeben_umgekehrte_reihenfolge() {
        let mut verts = quad_bei(0.0);
        verts.extend(quad_bei(10.0));

        let von_sueden = sortiere(&verts, Vec3::new(0.5, 0.5, -20.0));
        let von_norden = sortiere(&verts, Vec3::new(0.5, 0.5, 30.0));

        // Von Sueden (z=-20) ist das ferne Quad z=10 -> zuerst.
        assert_eq!(von_sueden[0].position[2], 10.0, "von Sueden muss das ferne Quad zuerst kommen");
        // Von Norden (z=30) genau andersherum.
        assert_eq!(von_norden[0].position[2], 0.0, "von Norden muss das andere Quad zuerst kommen");
    }

    // Immer von HINTEN nach VORN -- das ist die Bedingung, unter der Alpha-Blending ohne
    // Tiefenschreiben ueberhaupt richtig herauskommt.
    #[test]
    fn sortiert_von_hinten_nach_vorn() {
        let mut verts = quad_bei(5.0);
        verts.extend(quad_bei(1.0));
        verts.extend(quad_bei(9.0));
        let camera = Vec3::new(0.5, 0.5, 0.0);

        let sortiert = sortiere(&verts, camera);
        let z: Vec<f32> = sortiert.chunks_exact(VERTS_PER_QUAD).map(|q| q[0].position[2]).collect();
        assert_eq!(z, vec![9.0, 5.0, 1.0], "Abstand muss abnehmen, nicht zunehmen");
    }

    // Quads duerfen beim Umsortieren nicht zerrissen werden -- ein halbes Quad ergaebe ein
    // Loch. Deshalb arbeitet die Sortierung auf 6er-Bloecken, nicht auf Dreiecken.
    #[test]
    fn quads_bleiben_unzerrissen() {
        let mut verts = quad_bei(0.0);
        verts.extend(quad_bei(10.0));
        let sortiert = sortiere(&verts, Vec3::new(0.5, 0.5, -20.0));

        assert_eq!(sortiert.len(), verts.len(), "es darf kein Vertex verlorengehen");
        for quad in sortiert.chunks_exact(VERTS_PER_QUAD) {
            let z = quad[0].position[2];
            assert!(quad.iter().all(|v| v.position[2] == z), "ein Quad wurde ueber zwei Positionen zerrissen");
        }
    }

    // Gleich weit entfernte Quads muessen IMMER dieselbe Reihenfolge bekommen. sortiere_quads
    // benutzt sort_unstable (schneller, aber ohne Stabilitaetsgarantie) und faengt das mit dem
    // Quad-Index als zweitem Sortierkriterium ab -- ohne den koennten zwei deckungsgleiche
    // Flaechen von Frame zu Frame die Plaetze tauschen und sichtbar flimmern.
    #[test]
    fn gleiche_abstaende_behalten_eine_feste_reihenfolge() {
        let mut verts = Vec::new();
        for _ in 0..8 {
            verts.extend(quad_bei(3.0));
        }
        let camera = Vec3::new(0.5, 0.5, 0.0);
        // Zweimal sortieren -- das zweite Mal auf dem schon sortierten Ergebnis, wie es beim
        // Weitergehen tatsaechlich passiert.
        let einmal = sortiere(&verts, camera);
        let zweimal = sortiere(&einmal, camera);
        let positionen = |v: &[BlockVertex]| v.iter().map(|v| v.position).collect::<Vec<_>>();
        assert_eq!(positionen(&einmal), positionen(&zweimal), "gleiche Abstaende duerfen nicht von Durchgang zu Durchgang umspringen");
    }

    // Die Arbeitspuffer werden ueber Aufrufe hinweg wiederverwendet (das ist der Sinn der
    // Auslagerung). Ein zweiter Aufruf mit denselben, bereits gefuellten Puffern muss deshalb
    // dasselbe liefern wie ein erster mit frischen -- sonst schleppt er Reste mit.
    #[test]
    fn wiederverwendete_arbeitspuffer_liefern_dasselbe() {
        let mut verts = quad_bei(1.0);
        verts.extend(quad_bei(7.0));
        verts.extend(quad_bei(4.0));
        let camera = Vec3::new(0.5, 0.5, 0.0);

        let (mut keys, mut scratch) = (Vec::new(), Vec::new());
        let mut a = verts.clone();
        sortiere_quads(&mut a, camera, &mut keys, &mut scratch);
        // Zweiter Durchgang mit denselben Puffern, aber anderer Kamera und anderer Laenge.
        let mut b = quad_bei(2.0);
        b.extend(quad_bei(9.0));
        sortiere_quads(&mut b, Vec3::new(0.5, 0.5, 20.0), &mut keys, &mut scratch);

        let positionen = |v: &[BlockVertex]| v.iter().map(|v| v.position).collect::<Vec<_>>();
        assert_eq!(positionen(&a), positionen(&sortiere(&verts, camera)));
        assert_eq!(b.len(), 2 * VERTS_PER_QUAD, "kein Rest aus dem vorherigen Durchgang");
        assert_eq!(b[0].position[2], 2.0, "von z=20 aus ist z=2 das fernere Quad");
    }
}

#[cfg(test)]
mod draw_order_tests {
    use super::*;

    #[test]
    fn naechste_spalte_kommt_zuerst() {
        let mut spalten = [(5, 0), (1, 0), (3, 0)];
        sortiere_nach_abstand(&mut spalten, (0, 0));
        assert_eq!(spalten, [(1, 0), (3, 0), (5, 0)]);
    }

    // Wasser wird als umgedrehte Liste gezeichnet (s. render) -- umgedreht muss also die
    // FERNSTE Spalte zuerst kommen, sonst mischt naeheres Wasser unter fernerem.
    #[test]
    fn umgedreht_ergibt_hinten_nach_vorn() {
        let mut spalten = [(1, 0), (9, 0), (4, 0)];
        sortiere_nach_abstand(&mut spalten, (0, 0));
        let hinten_nach_vorn: Vec<_> = spalten.iter().rev().copied().collect();
        assert_eq!(hinten_nach_vorn, vec![(9, 0), (4, 0), (1, 0)]);
    }

    // Ein ganzer Ring hat denselben Abstand -- ohne festen Tiebreaker haenge die Reihenfolge
    // wieder an der HashMap und kippte beim Laufen von Frame zu Frame.
    #[test]
    fn gleicher_abstand_wird_stabil_geordnet() {
        let ring = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        let mut a = ring;
        let mut b = [(0, -1), (-1, 0), (0, 1), (1, 0)]; // dieselbe Menge, andere Startfolge
        sortiere_nach_abstand(&mut a, (0, 0));
        sortiere_nach_abstand(&mut b, (0, 0));
        assert_eq!(a, b, "gleich weit entfernte Spalten muessen unabhaengig von der Eingabefolge gleich landen");
    }

    // Die Ordnung haengt an der Kamera-Spalte, nicht am Ursprung -- sonst waere das Sortieren
    // nach einem Spaltenwechsel wirkungslos.
    #[test]
    fn reihenfolge_folgt_der_kameraspalte() {
        let mut spalten = [(0, 0), (10, 0)];
        sortiere_nach_abstand(&mut spalten, (12, 0));
        assert_eq!(spalten, [(10, 0), (0, 0)], "von (12,0) aus ist (10,0) die naechste Spalte");
    }

    // Weit entfernte Spalten (Teleport, Weltgrenze) duerfen die Abstandsrechnung nicht zum
    // Ueberlaufen bringen -- in i32 waere 1.9 Mio quadriert laengst ueber i32::MAX.
    #[test]
    fn sehr_weite_abstaende_ueberlaufen_nicht() {
        let mut spalten = [(1_800_000, 0), (5, 0)];
        sortiere_nach_abstand(&mut spalten, (0, 0));
        assert_eq!(spalten, [(5, 0), (1_800_000, 0)]);
    }
}

#[cfg(test)]
mod hand_camera_tests {
    use super::*;

    // Die echten Vertices der Ego-Hand -- derselbe Weg wie in App::build_hand_vertices, damit
    // der Test die tatsaechliche Geometrie prueft und nicht eine Nachbildung davon. Ein erster
    // Anlauf hat den Pivot des Arm-Teils von Hand nachgerechnet und dabei uebersehen, dass der
    // Pivot die SCHULTER ist: die liegt korrekterweise unterhalb des Bildrands, der Arm ragt
    // von unten rechts herein. Gemessen wird deshalb die ganze Punktwolke.
    fn hand_vertices() -> Vec<glam::Vec3> {
        let model = world::player_arm_model(false, true);
        let pose = world::first_person_arm_pose(true);
        let base = world::first_person_arm_transform(true, 0.0, 0.0);
        let mut out = Vec::new();
        world::emit_entity_model(&model, &pose, base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
        out.into_iter().map(|v| glam::Vec3::from(v.position)).collect()
    }

    fn auf_den_schirm(punkt: glam::Vec3, proj: glam::Mat4) -> glam::Vec2 {
        let c = proj * punkt.extend(1.0);
        glam::Vec2::new(c.x / c.w, c.y / c.w)
    }

    #[test]
    // Der Arm muss im Bild ankommen, und zwar in der unteren rechten Ecke. Stimmt das
    // Sichtfeld nicht, rutscht er heraus, besonders bei hohen
    // FOV-Einstellungen.
    fn arm_ragt_von_unten_rechts_ins_bild() {
        let proj = hand_projection(16.0 / 9.0);
        let punkte: Vec<glam::Vec2> = hand_vertices().into_iter().map(|p| auf_den_schirm(p, proj)).collect();
        assert!(!punkte.is_empty(), "der Arm muss ueberhaupt Geometrie haben");
        let sichtbar = punkte.iter().filter(|s| s.x.abs() <= 1.0 && s.y.abs() <= 1.0).count();
        assert!(sichtbar > punkte.len() / 4, "zu wenig vom Arm im Bild: {sichtbar} von {}", punkte.len());
        // ... und zwar unten rechts: jeder sichtbare Punkt liegt rechts der Mitte und darunter.
        for s in punkte.iter().filter(|s| s.x.abs() <= 1.0 && s.y.abs() <= 1.0) {
            assert!(s.x > 0.0, "Arm gehoert nach rechts: {s:?}");
            assert!(s.y < 0.0, "Arm gehoert nach unten: {s:?}");
        }
    }

    #[test]
    // Die Kernaussage: die Hand haengt NICHT am eingestellten Sichtfeld. Bei FOV 110 -- ein bei
    // PvP durchaus ueblicher Wert -- rutscht der Arm sichtbar weg. Die Hand-Projektion liefert
    // unveraendert dieselbe Stelle.
    //
    // Der Vergleichswert zeigt, wie gross der Fehler war, als hier der Einstellwert
    // durchgereicht wurde.
    fn hand_folgt_der_fov_einstellung_nicht() {
        let aspect = 16.0 / 9.0;
        let punkte = hand_vertices();
        let richtig = hand_projection(aspect);
        let falsch = glam::Mat4::perspective_rh(110.0f32.to_radians(), aspect, HAND_NEAR_Z, HAND_FAR_Z);
        let versatz = punkte.iter().map(|p| (auf_den_schirm(*p, richtig) - auf_den_schirm(*p, falsch)).length()).fold(0.0f32, f32::max);
        assert!(versatz > 0.2, "der Fehler war deutlich sichtbar, Versatz war nur {versatz}");
    }

    #[test]
    // Nahebene und Fernebene stammen aus Vanillas Hud-Projektion (0.05 und 100).
    // Kein Punkt des Arms darf an einer der beiden Ebenen abgeschnitten werden.
    fn arm_liegt_zwischen_nah_und_fernebene() {
        for p in hand_vertices() {
            let tiefe = -p.z;
            assert!(tiefe > HAND_NEAR_Z, "an der Nahebene abgeschnitten: {tiefe}");
            assert!(tiefe < HAND_FAR_Z, "hinter der Fernebene: {tiefe}");
        }
    }
}

// Kacheln in `texture` ersetzen: je Kachel eine Liste von Mip-Stufen (`[0]` = volle Groesse,
// beliebig viele Stufen), Ursprung und Kantenlaenge. `staging` ist ein wiederverwendeter
// Zwischenpuffer des Aufrufers.
//
// EIN write_buffer plus Kopierbefehle statt je Kachel und Stufe ein write_texture: wgpu legt
// fuer jedes write_texture einen eigenen Staging-Puffer an.
pub fn upload_tiles(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    staging: &mut Option<wgpu::Buffer>,
    tiles: &[(&[Vec<u8>], (u32, u32), u32)],
) {
    let Some((data, regions)) = tile_layout(texture, tiles) else { return };
    let needed = data.len() as u64;
    if staging.as_ref().is_none_or(|b| b.size() < needed) {
        *staging = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("atlas upload buffer"),
            size: needed.next_power_of_two(),
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    let buffer = staging.as_ref().unwrap();
    queue.write_buffer(buffer, 0, &data);
    queue.submit(Some(tile_copies(device, buffer, texture, &regions)));
}

// Wie upload_tiles, aber nur aufgezeichnet: ein fertiger Befehlspuffer mit eigenem Zwischenpuffer,
// den ein anderer Thread zu einem selbst gewaehlten Zeitpunkt abschickt. So kann der
// Animations-Thread der App die Arbeit machen, ohne dass seine Kopien irgendwann mitten in einen
// Frame auf der GPU fallen (s. App::submit_block_animations). Ohne write_buffer: das liefe ueber
// die Warteschlange und kaeme mit dem naechsten Submit irgendeines Threads.
pub fn encode_tiles(device: &wgpu::Device, texture: &wgpu::Texture, tiles: &[(&[Vec<u8>], (u32, u32), u32)]) -> Option<wgpu::CommandBuffer> {
    let (data, regions) = tile_layout(texture, tiles)?;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("atlas animation buffer"),
        size: data.len() as u64,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    buffer.slice(..).get_mapped_range_mut().copy_from_slice(&data);
    buffer.unmap();
    Some(tile_copies(device, &buffer, texture, &regions))
}

// (Stufe, Ursprung, Kante, Offset, Zeilenlaenge im Puffer)
type TileRegion = (u32, (u32, u32), u32, u64, u32);

// Pufferinhalt und Kopierbereiche fuer upload_tiles/encode_tiles; None = nichts zu tun. Die
// Laenge ist ein Vielfaches von 512 Byte.
fn tile_layout(texture: &wgpu::Texture, tiles: &[(&[Vec<u8>], (u32, u32), u32)]) -> Option<(Vec<u8>, Vec<TileRegion>)> {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let max_level = texture.mip_level_count() as usize;
    // Gleich grosse Kacheln einer Stufe liegen im Puffer NEBENEINANDER in einem Streifen,
    // aufgefuellt auf die 256-Byte-Zeilenausrichtung wird nur die ganze Streifenzeile.
    // Frueher bekam jede Kachelzeile ihre eigene Auffuellung: eine 32er-Kachel (128 Byte je
    // Zeile) belegte mit Mips ~16 KB statt ~5 KB, und seit alle Animationen zeitversetzt
    // laufen (~150 Kacheln je Tick) waren das 2,5 MB je Tick -- Spitzen bis 3 ms.
    let mut groups: Vec<((u32, u32), Vec<(&[u8], (u32, u32))>)> = Vec::new();
    for &(mips, origin, tile_size) in tiles {
        for (level, rgba) in mips.iter().enumerate().take(max_level) {
            let size = tile_size >> level;
            if size == 0 || rgba.len() < (size * size * 4) as usize {
                break;
            }
            let key = (level as u32, size);
            let entry = (rgba.as_slice(), (origin.0 >> level, origin.1 >> level));
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => list.push(entry),
                None => groups.push((key, vec![entry])),
            }
        }
    }
    let mut regions: Vec<TileRegion> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    for ((level, size), list) in &groups {
        let tile_row = (*size * 4) as usize;
        let padded = ((list.len() * tile_row) as u32).div_ceil(align) * align;
        let base = data.len();
        data.resize(base + padded as usize * *size as usize, 0);
        for (k, (rgba, origin)) in list.iter().enumerate() {
            for y in 0..*size as usize {
                let dst = base + y * padded as usize + k * tile_row;
                data[dst..dst + tile_row].copy_from_slice(&rgba[y * tile_row..(y + 1) * tile_row]);
            }
            regions.push((*level, *origin, *size, (base + k * tile_row) as u64, padded));
        }
        // Naechster Streifen wieder auf 512 Byte -- s. frueheren Kommentar, schadet nicht.
        data.resize(data.len().div_ceil(512) * 512, 0);
    }
    (!regions.is_empty()).then_some((data, regions))
}

fn tile_copies(device: &wgpu::Device, buffer: &wgpu::Buffer, texture: &wgpu::Texture, regions: &[TileRegion]) -> wgpu::CommandBuffer {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("atlas upload") });
    for &(level, origin, size, offset, padded) in regions {
        encoder.copy_buffer_to_texture(
            wgpu::ImageCopyBuffer {
                buffer,
                layout: wgpu::ImageDataLayout { offset, bytes_per_row: Some(padded), rows_per_image: Some(size) },
            },
            wgpu::ImageCopyTexture {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d { x: origin.0, y: origin.1, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
    }
    encoder.finish()
}

#[cfg(test)]
mod section_frustum_tests {
    use super::*;
    use glam::{Mat4, Vec3};

    #[test]
    // Kamera auf Hoehe 0, Blick waagerecht nach +Z, 70 Grad: von einer Spalte 32..48 voraus
    // bleiben nur die Sections um die Augenhoehe, die hoch oben und tief unten fallen weg.
    fn nur_sections_im_bild_bleiben() {
        let proj = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.05, 1000.0);
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        let frustum = Frustum::from_view_proj(proj * view);
        let alle = (1u64 << 24) - 1;
        let drin = sections_in_frustum((0, 2), -64, alle, &frustum, DVec3::ZERO);
        // Augenhoehe 0 liegt in Section 4 (ab -64).
        assert!(drin & (1 << 4) != 0, "{drin:024b}");
        assert_eq!(drin & 1, 0, "y -64..-48 liegt unter dem Bild: {drin:024b}");
        assert_eq!(drin & (1 << 23), 0, "y 304..320 liegt ueber dem Bild: {drin:024b}");
        // Hinter der Kamera gar nichts.
        assert_eq!(sections_in_frustum((0, -3), -64, alle, &frustum, DVec3::ZERO), 0);
    }
}

#[cfg(test)]
mod shader_tests {
    // block.wgsl wird sonst erst beim Anlegen der Pipelines geprueft, also beim Start des
    // Clients. Hier schon im Test: parsen und validieren wie wgpu es tut.
    #[test]
    fn block_shader_ist_gueltiges_wgsl() {
        let modul = naga::front::wgsl::parse_str(include_str!("block.wgsl")).unwrap_or_else(|e| panic!("{}", e.emit_to_string(include_str!("block.wgsl"))));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&modul)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(modul.entry_points.iter().any(|e| e.name == "fs_weather"));
    }
}
