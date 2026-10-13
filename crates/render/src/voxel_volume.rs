// Voxel-Abbild der Welt um die Kamera, fuer Strahlen im Licht-Pass des Shader-Pfads
// (Sonnen- und Mondschatten, s. deferred_light.wgsl). Je Block eine 16-Bit-Klasse aus
// world::voxel.
//
// Ein Wuerfel von 16x16x16 Sections (256 Bloecke je Achse) um die Section der Kamera, als
// Ringpuffer: Section (sx, sy, sz) liegt immer im Slot (sx & 15, sy & 15, sz & 15) der 3D-
// Textur. Bewegt sich die Kamera, werden nur die Sections neu geschrieben, die in den Wuerfel
// hineinwandern. Welche Section ein Slot gerade haelt, steht im Slot-Puffer -- der Shader
// prueft das bei jedem Schritt und behandelt alles andere als unbekannt (kein Schatten).
// So zeigt ein noch nicht geschriebener Slot nie Bloecke von einer anderen Stelle.
//
// Texturachsen: (x, z, y) statt (x, y, z), damit eine Section in ihrer eigenen
// Speicherreihenfolge ((y * 16 + z) * 16 + x) ohne Umsortieren hochgeht.
//
// Speicher: 256^3 * 2 Byte = 32 MB, nur solange der Shader-Pfad laeuft; dieselbe Groesse noch
// einmal fuer die States (Farbe beim Rueckwurf).
//
// Dazu die Blocklicht-Quellen (Fackeln, Laternen, Glowstone ...) fuer die Blocklicht-Schatten:
// je Slot eine Liste, daraus ein Raster aus 8^3-Zellen um die Kamera, das je Zelle die
// staerksten Quellen nennt, die sie erreichen (s. deferred_light.wgsl::block_light_visibility).

use world::voxel::VoxelClasses;
use world::{ChunkSection, SECTION_SIZE};

pub const SECTIONS: i32 = 16;
const SLOT_COUNT: usize = (SECTIONS * SECTIONS * SECTIONS) as usize;
const SIZE: u32 = (SECTIONS * SECTION_SIZE) as u32;

// Slot-Zustand im Puffer (w-Komponente): leer/unbekannt, Bloecke darin, nur Luft (der Shader
// springt dann ueber die ganze Section).
const SLOT_UNKNOWN: i32 = 0;
const SLOT_BLOCKS: i32 = 1;
const SLOT_AIR: i32 = 2;

// Licht-Raster: LIGHT_CELLS^3 Zellen zu je LIGHT_CELL Bloecken, also 128 Bloecke um die Kamera.
pub const LIGHT_CELL: i32 = 8;
pub const LIGHT_CELLS: i32 = 16;
// Quellen je Zelle. Leuchten mehr hinein (Lavasee, beleuchtetes Dorf), zaehlen die staerksten;
// die anderen bleiben im Blocklicht, nur ihr Schatten fehlt.
pub const LIGHTS_PER_CELL: usize = 16;
const LIGHT_GRID_LEN: usize = (LIGHT_CELLS * LIGHT_CELLS * LIGHT_CELLS) as usize * LIGHTS_PER_CELL;
// Quellen-Positionen im Raster: je Achse 10 Bit, um diesen Wert verschoben (Quellen
// ausserhalb des Rasters leuchten noch hinein, Fernlichter bis 255 Bloecke weit).
const LIGHT_POS_BIAS: i32 = 256;
// Fernlichter: Quellen, die ueber Stufe 15 hinaus leuchten (Lichtstaerken ueber 100 %). Sie
// stehen zusaetzlich in einer eigenen Liste, die jeder Pixel ganz durchgeht -- ihr Licht
// reicht bis zu 255 Bloecke, weit ueber das Licht-Raster hinaus. Die naechsten so vielen.
pub const FAR_LIGHTS_MAX: usize = 32;
// Lichtkarten (Format s. deferred_light.wgsl, lm und lm_lagen): Seiten zu je einer Section,
// je Seite bis LM_FACES Flaechen mit je LM_LAGEN Ebenen des Blocklichts (eine je Quelle, ihre
// Schattengrenzen als Linien) und der Sicht auf den Himmel. Jede Section mit Bloecken im
// Licht-Raster bekommt eine; so viele Seiten, wie Sections das Raster schneiden koennen
// (9^3) -- es geht keiner der Platz aus. Zusammen etwa 300 MB VRAM; die Ebenen allein
// brauchen mehr als die 128 MB, die wgpu ohne Weiteres an einem Stueck bindet (s.
// required_storage_binding).
const LM_PAGES: usize = 768;
pub const LM_FACES: usize = 1024;
pub const LM_LAGEN: usize = 16;
const LM_INDEX_BASE: usize = SLOT_COUNT;
const LM_BAKE_BASE: usize = LM_INDEX_BASE + LM_PAGES * 4096 * 6;
pub const LM_HIMMEL_BASE: usize = LM_BAKE_BASE + LM_PAGES + LM_PAGES * LM_FACES;
pub const LM_RUECK_BASE: usize = LM_HIMMEL_BASE + LM_PAGES * LM_FACES * 4;
// Je Flaeche das zurueckgeworfene Blocklicht an 4x4 Punkten, je ein Wort (s.
// deferred_light.wgsl, back_rueckwurf).
pub const LM_RUECK_WORTE: usize = 16;
// Sicht-Raster (16x16 zu 8 Bit, 64 Worte) fuer Flaechen, die eine Quelle nur zum Teil sehen: je
// Seite ein Zaehler und LM_FEIN_PLAETZE davon (s. deferred_light.wgsl, cs_lm_fein).
pub const LM_FEIN_ZAEHLER_BASE: usize = LM_RUECK_BASE + LM_PAGES * LM_FACES * LM_RUECK_WORTE;
pub const LM_FEIN_BASE: usize = LM_FEIN_ZAEHLER_BASE + LM_PAGES;
pub const LM_FEIN_PLAETZE: usize = 512;
pub const LM_FEIN_WORTE: usize = 64;
// Je feinem Raster zwei Worte Kopf (s. deferred_light.wgsl, LM_FEIN_KOPF_BASE).
const LM_FEIN_KOPF_BASE: usize = LM_FEIN_BASE + LM_PAGES * LM_FEIN_PLAETZE * LM_FEIN_WORTE;
const LM_LEN: usize = LM_FEIN_KOPF_BASE + LM_PAGES * LM_FEIN_PLAETZE * 2;
const LM_LAGEN_LEN: usize = LM_PAGES * LM_FACES * LM_LAGEN * 4;
// Eine Seite wird in LM_SCHRITTE Schritten gebacken, je LM_FACES / LM_SCHRITTE Flaechen
// (s. deferred_light.wgsl, LM_SCHRITT_FLAECHEN) -- ein ganzer Schritt passt in die Zeit eines
// Frames, eine ganze Seite (20-35 ms) nicht: frueher ruckelte jeder Frame, der eine backte.
pub const LM_SCHRITTE: u32 = 16;
// Platz fuer so viele Schritte je Frame (s. BakeJobs im Shader): eine ganze Seite ...
pub const LM_BAKE_PER_FRAME: usize = 16;
// ... genutzt wird im Spiel einer (set_bake_budget): ein Schritt kostet im Mittel etwa 3,5 ms
// GPU, hoechstens etwa 8 (gemessen im Dorf, s. Test benchmark_welt_laden). Die Zeitmessung des
// BakeTimers taugt dafuer nicht -- sie meldete auf RADV ein Fuenfzigstel der echten Zeit.

pub struct VoxelVolume {
    view: wgpu::TextureView,
    texture: wgpu::Texture,
    // Daneben je Block sein State (16 Bit), fuer Farbe und Material beim Rueckwurf des Lichts
    // und in Spiegelungen, und je State seine Seiten im Atlas und sein Material (s.
    // world::voxel::VoxelClasses::state_info) -- als Textur, 256 States je Zeile; hochgeladen
    // beim ersten upload_state_info (new hat keine Queue).
    state_view: wgpu::TextureView,
    state_texture: wgpu::Texture,
    state_info_view: wgpu::TextureView,
    state_info_texture: wgpu::Texture,
    state_info_offen: bool,
    slots_buffer: wgpu::Buffer,
    headers_buffer: wgpu::Buffer,
    boxes_buffer: wgpu::Buffer,
    cross_uvs_buffer: wgpu::Buffer,
    light_grid_buffer: wgpu::Buffer,
    // Fernlichter (Format s. deferred_light.wgsl, far_lights).
    far_lights_buffer: wgpu::Buffer,
    far_light_count: usize,
    // Lichtkarten: die Daten selbst, der Zustand beim Backen, die Auftraege dieses Frames.
    lm_buffer: wgpu::Buffer,
    lm_layer_buffer: wgpu::Buffer,
    bake_jobs_buffer: wgpu::Buffer,
    // Je Slot seine Seite; je Seite die Section, fuer die sie gilt; freie Seiten.
    page_of: Vec<Option<u32>>,
    page_section: Vec<[i32; 3]>,
    free_pages: Vec<u32>,
    // Slots, deren Seite neu gebacken werden muss, und die Auftraege dieses Frames (Slot, Seite).
    bake_queue: Vec<usize>,
    // Je Slot: muss beim naechsten Backen auch die Himmelssicht neu (neue Seite, Bloecke
    // geaendert)? Sonst nur die Ebenen des Blocklichts.
    sky_stale: Vec<bool>,
    // Je Slot: Ursprung des Licht-Rasters beim letzten Backen (s. rueckwurf_ganz).
    gebacken_mit: Vec<Option<[i32; 3]>>,
    // Unterster Block der Welt: Flaechen, die darunter zeigen, sieht niemand.
    world_bottom: i32,
    // Je Slot ein Hash seines Inhalts: ein neu geschriebener Slot mit gleichem Inhalt (die
    // ganze Spalte wird bei jeder Blockaenderung neu geschrieben) laesst nichts neu backen.
    slot_hash: Vec<u64>,
    // Auftraege dieses Frames: (Slot, Seite, Schritt).
    bake_jobs: Vec<(usize, u32, u32)>,
    // Die Seite, die gerade in Schritten gebacken wird: Slot, Seite, naechster Schritt, nur
    // Blocklicht.
    bake_laufend: Option<(usize, u32, u32, bool)>,
    // Je Slot hoechstens so viele Flaechen (s. flaechen_hoechstens): so viele Schritte braucht
    // seine Seite -- leere Schritte kosten fast nichts und verfaelschten die Zeitmessung (das
    // Budget liess dann eine ganze Seite in einen Frame).
    flaechen_bound: Vec<u32>,
    // Je Slot fuer seine sechs Aussenseiten (Reihenfolge wie RAND_RICHTUNGEN), je 16 x 16
    // Bit: Block nicht Luft / voller Block an diesem Rand. Damit zaehlt flaechen_zahl die
    // Flaechen am Rand genau, sobald der Nachbar geschrieben ist.
    rand_belegt: Vec<[[u64; 4]; 6]>,
    rand_voll: Vec<[[u64; 4]; 6]>,
    // Slots, deren Bloecke sich geaendert haben (nicht neu geladen): ihre Seite und die um sie
    // werden ganz in einem Frame gebacken, wenn sie nah sind (s. plan_lightmaps).
    bloecke_geaendert: Vec<bool>,
    // Seiten, die wegen einer Blockaenderung neben ihnen ganz in einem Frame gebacken werden
    // (wenn nah, s. plan_lightmaps).
    sofort: Vec<bool>,
    // Wie viele Sections je Frame gebacken werden (s. set_bake_budget).
    bake_budget: usize,
    // In update neu geschriebene Slots (fuer die Lichtkarten in der Naehe).
    changed_slots: Vec<usize>,
    // Slots mit gleichen Bloecken, aber anderen Quellen (Lichtstaerken geaendert).
    light_changed_slots: Vec<usize>,
    // Slots mit anderen Quellen (gesetzt, entfernt, andere Staerke) und wie weit die alten und
    // neuen dort reichen: so weit um sie wird neu gebacken.
    light_reach_changes: Vec<(usize, i32)>,
    // Hat sich in diesem Frame etwas am Abbild geaendert? (Der Verlauf der Schatten faengt
    // dann neu an, s. ShaderPath::prepare.)
    changed_this_frame: bool,
    // Lichtkarten an (Einstellung Blocklicht-Schatten) und mit welchem Kegel gebacken.
    lightmaps: Option<f32>,
    classes: VoxelClasses,
    // Je Slot: leuchtende Bloecke als (Index in der Section, Emission wie world::light::emission
    // | echte Stufe (world::light::emission_level) << 12).
    emitters: Vec<Vec<(u16, u32)>>,
    lights_changed: bool,
    // Das Licht-Raster, wie zuletzt hochgeladen (s. LightGrid).
    light_grid_cpu: Option<LightGrid>,
    // Kleinste Weltblock-Koordinate des Licht-Rasters, wie zuletzt hochgeladen.
    light_origin: Option<[i32; 3]>,
    // Licht-Raster im Hintergrund bauen (s. set_async_lights): der laufende Bau.
    async_lights: bool,
    light_job: Option<std::thread::JoinHandle<LightBuild>>,
    // Je Slot: welche Section er haelt und ob nur Luft (w wie im Puffer).
    slots: Vec<[i32; 4]>,
    // Slots, deren Section sich geaendert hat und neu geschrieben werden muss.
    stale: Vec<bool>,
    slots_changed: bool,
    // Section-Versaetze um die Kamera-Section, nach Abstand sortiert: nahe Sections zuerst.
    order: Vec<[i32; 3]>,
    scratch: Box<[u16; world::chunk::SECTION_VOLUME]>,
    scratch_states: Box<[u16; world::chunk::SECTION_VOLUME]>,
}

fn slot_index(s: [i32; 3]) -> usize {
    let m = |v: i32| (v & (SECTIONS - 1)) as usize;
    (m(s[1]) * SECTIONS as usize + m(s[2])) * SECTIONS as usize + m(s[0])
}

fn storage_buffer(device: &wgpu::Device, label: &str, data: &[u8]) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (data.len() as u64).max(16).next_multiple_of(4),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    buffer.slice(..).get_mapped_range_mut()[..data.len()].copy_from_slice(data);
    buffer.unmap();
    buffer
}

impl VoxelVolume {
    // So gross muss ein Speicher-Puffer an einem Stueck gebunden werden koennen (der groesste
    // der Lichtkarten); beim Anfordern des Device mit angeben.
    pub fn required_storage_binding() -> u64 {
        (LM_LAGEN_LEN.max(LM_LEN) * 4) as u64
    }

    // Die Grenzen, die der Shader-Pfad braucht: wgpus Standard, Speicher-Puffer so gross wie
    // required_storage_binding (soweit der Adapter kann).
    pub fn required_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
        let kann = adapter.limits();
        let noetig = Self::required_storage_binding();
        let standard = wgpu::Limits::default();
        wgpu::Limits {
            max_storage_buffer_binding_size: (standard.max_storage_buffer_binding_size as u64).max(noetig).min(kann.max_storage_buffer_binding_size as u64) as u32,
            max_buffer_size: standard.max_buffer_size.max(noetig).min(kann.max_buffer_size),
            ..standard
        }
    }

    pub fn new(device: &wgpu::Device, classes: VoxelClasses) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("voxel volume"),
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: SIZE },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let state_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("voxel states"),
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: SIZE },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let state_view = state_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let state_info_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("voxel state info"),
            size: wgpu::Extent3d { width: (256 * world::voxel::STATE_INFO_WORTE) as u32, height: Self::state_info_rows(&classes), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let state_info_view = state_info_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let slots = vec![[0, 0, 0, SLOT_UNKNOWN]; SLOT_COUNT];
        let slots_buffer = storage_buffer(device, "voxel slots", bytemuck::cast_slice(&slots));
        let headers_buffer = storage_buffer(device, "voxel class headers", bytemuck::cast_slice(&classes.headers));
        let boxes_buffer = storage_buffer(device, "voxel class boxes", bytemuck::cast_slice(&classes.boxes));
        let cross_uvs_buffer = storage_buffer(device, "voxel cross uvs", bytemuck::cast_slice(&classes.cross_uvs));
        let light_grid_buffer = storage_buffer(device, "voxel light grid", bytemuck::cast_slice(&vec![[0u32; 2]; LIGHT_GRID_LEN]));
        let far_lights_buffer = storage_buffer(device, "voxel far lights", bytemuck::cast_slice(&[[0i32; 4]; FAR_LIGHTS_MAX]));
        // Von wgpu genullt: keine Seite, keine Flaeche.
        let leer = |label, len: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (len * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let lm_buffer = leer("voxel lightmaps", LM_LEN);
        let lm_layer_buffer = leer("voxel lightmap layers", LM_LAGEN_LEN);
        let bake_jobs_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voxel lightmap jobs"),
            size: (LM_BAKE_PER_FRAME * 16 + 16) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let half = SECTIONS / 2;
        let mut order: Vec<[i32; 3]> = Vec::with_capacity(SLOT_COUNT);
        for y in -half..half {
            for z in -half..half {
                for x in -half..half {
                    order.push([x, y, z]);
                }
            }
        }
        // Abstand zur Mitte der Kamera-Section (die Versaetze reichen -8..7, Mitte also -0,5
        // daneben -- ohne Belang fuer die Reihenfolge).
        order.sort_by_key(|d| d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        Self {
            view,
            texture,
            state_view,
            state_texture,
            state_info_view,
            state_info_texture,
            state_info_offen: true,
            slots_buffer,
            headers_buffer,
            boxes_buffer,
            cross_uvs_buffer,
            light_grid_buffer,
            far_lights_buffer,
            far_light_count: 0,
            lm_buffer,
            lm_layer_buffer,
            bake_jobs_buffer,
            page_of: vec![None; SLOT_COUNT],
            page_section: vec![[0; 3]; LM_PAGES],
            free_pages: (0..LM_PAGES as u32).rev().collect(),
            bake_queue: Vec::new(),
            sky_stale: vec![false; SLOT_COUNT],
            gebacken_mit: vec![None; SLOT_COUNT],
            world_bottom: i32::MIN / 2,
            slot_hash: vec![0; SLOT_COUNT],
            bake_jobs: Vec::new(),
            bake_laufend: None,
            flaechen_bound: vec![LM_FACES as u32; SLOT_COUNT],
            rand_belegt: vec![[[0; 4]; 6]; SLOT_COUNT],
            rand_voll: vec![[[0; 4]; 6]; SLOT_COUNT],
            bloecke_geaendert: vec![false; SLOT_COUNT],
            sofort: vec![false; SLOT_COUNT],
            bake_budget: 1,
            changed_slots: Vec::new(),
            light_changed_slots: Vec::new(),
            light_reach_changes: Vec::new(),
            changed_this_frame: false,
            lightmaps: None,
            classes,
            emitters: vec![Vec::new(); SLOT_COUNT],
            lights_changed: true,
            light_grid_cpu: None,
            light_origin: None,
            async_lights: false,
            light_job: None,
            slots,
            stale: vec![false; SLOT_COUNT],
            slots_changed: false,
            order,
            scratch: Box::new([0; world::chunk::SECTION_VOLUME]),
            scratch_states: Box::new([0; world::chunk::SECTION_VOLUME]),
        }
    }

    pub fn texture_view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub fn state_view(&self) -> &wgpu::TextureView {
        &self.state_view
    }

    pub fn state_info_view(&self) -> &wgpu::TextureView {
        &self.state_info_view
    }

    fn state_info_rows(classes: &VoxelClasses) -> u32 {
        (classes.state_info.len() / world::voxel::STATE_INFO_WORTE).div_ceil(256).max(1) as u32
    }

    // Die State-Tabelle einmal hochladen (s. state_info_view).
    pub fn upload_state_info(&mut self, queue: &wgpu::Queue) {
        if !std::mem::take(&mut self.state_info_offen) {
            return;
        }
        let breite = 256 * world::voxel::STATE_INFO_WORTE;
        let zeilen = Self::state_info_rows(&self.classes) as usize;
        let mut daten = self.classes.state_info.clone();
        daten.resize(breite * zeilen, 0);
        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &self.state_info_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&daten),
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(breite as u32 * 4), rows_per_image: Some(zeilen as u32) },
            wgpu::Extent3d { width: breite as u32, height: zeilen as u32, depth_or_array_layers: 1 },
        );
    }

    pub fn slots_buffer(&self) -> &wgpu::Buffer {
        &self.slots_buffer
    }

    pub fn headers_buffer(&self) -> &wgpu::Buffer {
        &self.headers_buffer
    }

    pub fn boxes_buffer(&self) -> &wgpu::Buffer {
        &self.boxes_buffer
    }

    pub fn cross_uvs_buffer(&self) -> &wgpu::Buffer {
        &self.cross_uvs_buffer
    }

    pub fn light_grid_buffer(&self) -> &wgpu::Buffer {
        &self.light_grid_buffer
    }

    pub fn far_lights_buffer(&self) -> &wgpu::Buffer {
        &self.far_lights_buffer
    }

    pub fn lightmap_buffer(&self) -> &wgpu::Buffer {
        &self.lm_buffer
    }

    pub fn lightmap_layer_buffer(&self) -> &wgpu::Buffer {
        &self.lm_layer_buffer
    }

    pub fn bake_jobs_buffer(&self) -> &wgpu::Buffer {
        &self.bake_jobs_buffer
    }

    // Lichtkarten an/aus und Kegel der Quellen (tan des halben Winkels). Aendert sich etwas,
    // wird alles neu gebacken.
    pub fn set_lightmaps(&mut self, queue: &wgpu::Queue, cone: Option<f32>) {
        if self.lightmaps == cone {
            return;
        }
        self.lightmaps = cone;
        // Alle Seiten frei; mit Lichtkarten verteilt sie update_lights neu.
        for i in 0..SLOT_COUNT {
            self.free_page(queue, i);
        }
        self.bake_queue.clear();
        self.lights_changed = true;
    }

    // Wie weit die Quellen reichen, deren Licht die Section `s` trifft (aus dem Licht-Raster).
    fn reach_at_section(&self, s: [i32; 4]) -> i32 {
        let Some(g) = &self.light_grid_cpu else { return 0 };
        let mut weit = 0;
        for cy in 0..2 {
            for cz in 0..2 {
                for cx in 0..2 {
                    let zelle = [s[0] * 16 + cx * 8, s[1] * 16 + cy * 8, s[2] * 16 + cz * 8];
                    let rel: Vec<i32> = (0..3).map(|k| (zelle[k] - g.origin[k]).div_euclid(LIGHT_CELL)).collect();
                    if rel.iter().any(|&v| !(0..LIGHT_CELLS).contains(&v)) {
                        continue;
                    }
                    let i = ((rel[1] * LIGHT_CELLS + rel[2]) * LIGHT_CELLS + rel[0]) as usize * LIGHTS_PER_CELL;
                    for e in &g.grid[i..i + LIGHTS_PER_CELL] {
                        weit = weit.max(emitter_reach(e[1]));
                    }
                }
            }
        }
        weit
    }

    // Slot `i` bekommt eine freie Seite und wird ganz gebacken; false, wenn keine mehr frei ist.
    fn assign_page(&mut self, i: usize) -> bool {
        let Some(seite) = self.free_pages.pop() else { return false };
        self.page_of[i] = Some(seite);
        let s = self.slots[i];
        self.page_section[seite as usize] = [s[0], s[1], s[2]];
        self.sky_stale[i] = true;
        self.bake_queue.push(i);
        true
    }

    fn free_page(&mut self, queue: &wgpu::Queue, slot: usize) {
        if let Some(seite) = self.page_of[slot].take() {
            self.free_pages.push(seite);
            self.gebacken_mit[slot] = None;
            queue.write_buffer(&self.lm_buffer, (slot * 4) as u64, bytemuck::bytes_of(&0u32));
        }
    }

    pub fn set_world_bottom(&mut self, y: i32) {
        self.world_bottom = y;
    }

    // Wie viele Schritte je Frame (s. LM_SCHRITTE).
    pub fn set_bake_budget(&mut self, schritte: usize) {
        self.bake_budget = schritte.clamp(1, LM_BAKE_PER_FRAME);
    }

    // Back-Auftraege dieses Frames: vor dem Backen ihre Seiten leeren (s. ShaderPath::shadows).
    pub fn bake_jobs(&self) -> &[(usize, u32, u32)] {
        &self.bake_jobs
    }

    // Byte-Bereiche in lightmap_buffer, die vor dem Backen einer Seite geleert werden: ihre
    // Flaechen-Indizes, ihr Flaechen-Zaehler und der Zaehler ihrer Sicht-Raster.
    pub fn page_clear_ranges(seite: u32) -> [(u64, u64); 3] {
        let s = seite as u64;
        [(((LM_INDEX_BASE as u64) + s * 4096 * 6) * 4, 4096 * 6 * 4), (((LM_BAKE_BASE as u64) + s) * 4, 4), (((LM_FEIN_ZAEHLER_BASE as u64) + s) * 4, 4)]
    }

    // Wie viele Gruppen der Flaechen-Schritt (je 64 Flaechen) und der Ebenen-Schritt (je eine
    // Flaeche eines Back-Schritts) brauchen.
    pub fn bake_groups() -> (u32, u32) {
        ((4096 * 6 / 64) as u32, LM_FACES as u32 / LM_SCHRITTE)
    }

    // Gruppen des Schritts fuer die Sicht-Raster: je vier Zeilen und Platz (s. cs_lm_fein).
    pub fn bake_fine_groups() -> (u32, u32) {
        (4, LM_FEIN_PLAETZE as u32)
    }

    pub fn changed_this_frame(&self) -> bool {
        self.changed_this_frame
    }

    // Gibt es gerade Fernlichter? Sonst kann der Shader-Pfad ihre Passes auslassen.
    pub fn has_far_lights(&self) -> bool {
        self.far_light_count > 0
    }

    // Ursprung des Licht-Rasters (Weltblock), None vor dem ersten update_lights.
    pub fn light_origin(&self) -> Option<[i32; 3]> {
        self.light_origin
    }

    // Kleinste Section-Koordinate des Wuerfels um die Kamera-Section `center`.
    pub fn window_min(center: [i32; 3]) -> [i32; 3] {
        center.map(|c| c - SECTIONS / 2)
    }

    // Alle geschriebenen Slots neu schreiben (z.B. nach geaenderten Lichtstaerken: die
    // Quellen-Listen haengen an world::light::emission).
    pub fn mark_all_stale(&mut self) {
        for (slot, stale) in self.slots.iter().zip(&mut self.stale) {
            if slot[3] != SLOT_UNKNOWN {
                *stale = true;
            }
        }
    }

    // Weltaenderungen aus world::World::take_voxel_changes vormerken.
    pub fn note_changes(&mut self, cleared: bool, columns: &[(i32, i32)]) {
        if cleared {
            for (slot, stale) in self.slots.iter_mut().zip(&mut self.stale) {
                slot[3] = SLOT_UNKNOWN;
                *stale = false;
            }
            self.emitters.iter_mut().for_each(Vec::clear);
            self.slots_changed = true;
            self.lights_changed = true;
        }
        for &(cx, cz) in columns {
            for (i, slot) in self.slots.iter().enumerate() {
                if slot[3] != SLOT_UNKNOWN && slot[0] == cx && slot[2] == cz {
                    self.stale[i] = true;
                }
            }
        }
    }

    // Bis zu `budget` Sections schreiben: zuerst die, die in den Wuerfel um `center`
    // hineingewandert sind, dann geaenderte -- jeweils von innen nach aussen.
    // `section(sx, sy, sz)` liefert die Section an diesen Section-Koordinaten, falls geladen.
    pub fn update<'a>(&mut self, queue: &wgpu::Queue, center: [i32; 3], section: impl Fn(i32, i32, i32) -> Option<&'a ChunkSection>, budget: usize) {
        self.upload_state_info(queue);
        let mut written = 0;
        for k in 0..self.order.len() {
            if written >= budget {
                break;
            }
            let d = self.order[k];
            let s = [center[0] + d[0], center[1] + d[1], center[2] + d[2]];
            let i = slot_index(s);
            let slot = self.slots[i];
            let gleich = slot[3] != SLOT_UNKNOWN && slot[..3] == s;
            if gleich && !self.stale[i] {
                continue;
            }
            self.stale[i] = false;
            let sec = section(s[0], s[1], s[2]);
            let belegt = self.classes.fill_section(sec, &mut self.scratch);
            let (innen, belegt_r, voll_r) = flaechen_innen(&self.scratch);
            self.flaechen_bound[i] = innen;
            self.rand_belegt[i] = belegt_r;
            self.rand_voll[i] = voll_r;
            VoxelClasses::fill_states(sec, &mut self.scratch_states);
            // Mit den States: ein anders gefaerbter Block wirft anderes Licht zurueck.
            let hash = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                self.scratch.hash(&mut h);
                self.scratch_states.hash(&mut h);
                belegt.hash(&mut h);
                h.finish()
            };
            let bloecke_neu = !gleich || self.slot_hash[i] != hash;
            if bloecke_neu {
                self.changed_slots.push(i);
                if gleich {
                    self.bloecke_geaendert[i] = true;
                }
            }
            self.slot_hash[i] = hash;
            let quellen: Vec<(u16, u32)> = sec
                .map(|sec| {
                    sec.states()
                        .iter()
                        .enumerate()
                        .filter_map(|(j, &st)| {
                            let e = world::light::emission(st);
                            (e != 0).then(|| (j as u16, e as u32 | (world::light::emission_level(st) as u32) << 12))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if quellen != self.emitters[i] {
                self.lights_changed = true;
                let reichweite = quellen.iter().chain(&self.emitters[i]).map(|&(_, e)| emitter_reach(e)).max().unwrap_or(0);
                self.light_reach_changes.push((i, reichweite));
                // Gleiche Bloecke, andere Lichtstaerken: nur das Blocklicht neu backen.
                if !bloecke_neu {
                    self.light_changed_slots.push(i);
                }
            }
            self.emitters[i] = quellen;
            // Luft ueber Luft muss nicht geschrieben werden: der Shader liest in einem
            // Luft-Slot keine Texel.
            if belegt {
                let origin = wgpu::Origin3d {
                    x: (s[0] & (SECTIONS - 1)) as u32 * SECTION_SIZE as u32,
                    y: (s[2] & (SECTIONS - 1)) as u32 * SECTION_SIZE as u32,
                    z: (s[1] & (SECTIONS - 1)) as u32 * SECTION_SIZE as u32,
                };
                queue.write_texture(
                    wgpu::ImageCopyTexture { texture: &self.texture, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                    bytemuck::cast_slice(&self.scratch[..]),
                    wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(SECTION_SIZE as u32 * 2), rows_per_image: Some(SECTION_SIZE as u32) },
                    wgpu::Extent3d { width: SECTION_SIZE as u32, height: SECTION_SIZE as u32, depth_or_array_layers: SECTION_SIZE as u32 },
                );
                queue.write_texture(
                    wgpu::ImageCopyTexture { texture: &self.state_texture, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                    bytemuck::cast_slice(&self.scratch_states[..]),
                    wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(SECTION_SIZE as u32 * 2), rows_per_image: Some(SECTION_SIZE as u32) },
                    wgpu::Extent3d { width: SECTION_SIZE as u32, height: SECTION_SIZE as u32, depth_or_array_layers: SECTION_SIZE as u32 },
                );
                written += 1;
            }
            self.slots[i] = [s[0], s[1], s[2], if belegt { SLOT_BLOCKS } else { SLOT_AIR }];
            self.slots_changed = true;
        }
        if std::mem::take(&mut self.slots_changed) {
            queue.write_buffer(&self.slots_buffer, 0, bytemuck::cast_slice(&self.slots));
        }
    }

    // Licht-Raster um den Block `eye` neu bauen, wenn die Kamera in eine andere Zelle gewandert
    // ist oder sich Quellen geaendert haben.
    pub fn update_lights(&mut self, queue: &wgpu::Queue, eye: [i32; 3]) {
        // Ein Bau im Hintergrund: fertig -> uebernehmen, sonst diesen Frame nichts backen (die
        // Aenderungen bleiben vorgemerkt) -- gebacken wird nur mit dem Raster, das sie kennt.
        if let Some(job) = self.light_job.take() {
            if !job.is_finished() {
                self.light_job = Some(job);
                return;
            }
            let fertig = job.join().expect("Licht-Raster-Bau");
            self.apply_lights(queue, fertig);
            self.plan_lightmaps(queue, eye, true);
            return;
        }
        let half = LIGHT_CELLS * LIGHT_CELL / 2;
        let origin = eye.map(|e| e.div_euclid(LIGHT_CELL) * LIGHT_CELL - half);
        let neu = self.light_origin != Some(origin) || self.lights_changed;
        if !neu {
            self.plan_lightmaps(queue, eye, false);
            return;
        }
        // Nur verschoben (dieselben Quellen): die alten Zellen weiterverwenden.
        let alt = if self.lights_changed { None } else { self.light_grid_cpu.clone() };
        self.lights_changed = false;
        let (slots, emitters) = (self.slots.clone(), self.emitters.clone());
        let bauen = move || LightBuild { grid: build_light_grid(&slots, &emitters, origin, alt.as_ref()), far: far_lights(&slots, &emitters, eye), origin };
        if self.async_lights {
            self.light_job = Some(std::thread::spawn(bauen));
        } else {
            self.apply_lights(queue, bauen());
            self.plan_lightmaps(queue, eye, true);
        }
    }

    // Hoechstens so viele Flaechen bekommt die Seite des Slots `i` (s. flaechen_innen): die in
    // der Section und die an ihrem Rand, vor denen im Nachbarn kein voller Block liegt (ist der
    // Nachbar nicht geschrieben: alle).
    fn flaechen_zahl(&self, i: usize) -> u32 {
        let s = self.slots[i];
        let mut zahl = self.flaechen_bound[i];
        for (r, d) in RAND_RICHTUNGEN.iter().enumerate() {
            let ns = [s[0] + d[0], s[1] + d[1], s[2] + d[2]];
            let n = slot_index(ns);
            let bekannt = self.slots[n][..3] == ns && self.slots[n][3] != SLOT_UNKNOWN;
            for w in 0..4 {
                let voll = if bekannt { self.rand_voll[n][r ^ 1][w] } else { 0 };
                zahl += (self.rand_belegt[i][r][w] & !voll).count_ones();
            }
        }
        zahl.min(LM_FACES as u32)
    }

    // Das Licht-Raster im Hintergrund bauen statt im Frame: nach einer Zellgrenze (alle acht
    // Bloecke) oder einer neuen Quelle braucht es mit vielen Quellen (Lavaseen, Fernlichter)
    // zig Millisekunden -- ein Ruckler. Bis es fertig ist, gilt das alte. Aus, wird es sofort
    // gebaut (Tests: dort soll ein Frame alles fertig haben).
    pub fn set_async_lights(&mut self, an: bool) {
        self.async_lights = an;
    }

    fn apply_lights(&mut self, queue: &wgpu::Queue, b: LightBuild) {
        let origin = b.origin;
        queue.write_buffer(&self.light_grid_buffer, 0, bytemuck::cast_slice(&b.grid.grid));
        self.light_grid_cpu = Some(b.grid);
        self.far_light_count = b.far.iter().take_while(|l| l[3] != 0).count();
        // Sections, die ueber den Rand des Rasters ragen, kannten dort keine Quellen (s.
        // deferred_light.wgsl, LM_AUSSEN): mit dem Raster gewandert, neu backen -- die am Rand
        // des neuen Rasters und die am Rand des alten (die liegen jetzt vielleicht ganz darin
        // und trugen sonst die Marke weiter: dort rauschte das Fernlicht aus dem Strahl-Pass,
        // bis eine Blockaenderung neu backen liess).
        if self.light_origin != Some(origin) {
            let span = LIGHT_CELLS * LIGHT_CELL;
            let rand = |s: [i32; 4], o: [i32; 3]| (0..3).any(|k| s[k] * 16 < o[k] || s[k] * 16 + 16 > o[k] + span);
            let alt = self.light_origin;
            // Dazu die, deren Rueckwurf erst mit diesem Raster ganz bekannt ist (s. rueckwurf_ganz).
            let jetzt_ganz = |i: usize| rueckwurf_ganz(self.slots[i], origin) && !self.gebacken_mit[i].is_some_and(|b| rueckwurf_ganz(self.slots[i], b));
            self.bake_queue.extend((0..SLOT_COUNT).filter(|&i| {
                self.page_of[i].is_some() && (rand(self.slots[i], origin) || alt.is_some_and(|a| rand(self.slots[i], a)) || jetzt_ganz(i))
            }));
        }
        queue.write_buffer(&self.far_lights_buffer, 0, bytemuck::cast_slice(&b.far));
        self.light_origin = Some(origin);
    }

    // Lichtkarten verwalten: nach neuem Licht-Raster (`neu`) bekommt jede Section in Reichweite
    // einer Quelle eine Seite, die anderen geben ihre ab. Geaenderte Sections lassen die Seiten
    // um sich neu backen (eine Aenderung wirft Schatten bis 15 Bloecke weit). Dann die
    // naechsten Auftraege fuer diesen Frame.
    fn plan_lightmaps(&mut self, queue: &wgpu::Queue, eye: [i32; 3], neu: bool) {
        let geaendert = std::mem::take(&mut self.changed_slots);
        let licht_geaendert = std::mem::take(&mut self.light_changed_slots);
        let reichweiten = std::mem::take(&mut self.light_reach_changes);
        self.changed_this_frame = !geaendert.is_empty() || !licht_geaendert.is_empty();
        self.bake_jobs.clear();
        let (Some(_), Some(origin)) = (self.lightmaps, self.light_origin) else {
            return;
        };
        if neu {
            let gewollt = grid_slots(&self.slots, origin);
            for i in 0..SLOT_COUNT {
                let falsch = self.page_of[i].is_some_and(|p| self.page_section[p as usize] != self.slots[i][..3]);
                if !gewollt[i] || falsch {
                    self.free_page(queue, i);
                }
            }
            // Die naechsten zuerst, falls die Seiten doch einmal nicht reichen.
            let kamera = eye.map(|e| e.div_euclid(16));
            let mut neue: Vec<usize> = (0..SLOT_COUNT).filter(|&i| gewollt[i] && self.page_of[i].is_none()).collect();
            neue.sort_by_key(|&i| (0..3).map(|k| (self.slots[i][k] - kamera[k]).abs()).max().unwrap_or(0));
            for i in neue {
                if !self.assign_page(i) {
                    break;
                }
            }
        }
        for &c in &geaendert {
            let s = self.slots[c];
            // Neu geladen: eine Seite, wenn sie ins Raster faellt.
            if self.page_of[c].is_none() && s[3] == SLOT_BLOCKS && in_grid(s, origin) {
                self.assign_page(c);
            }
            let geaendert_hier = std::mem::take(&mut self.bloecke_geaendert[c]);
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let n = slot_index([s[0] + dx, s[1] + dy, s[2] + dz]);
                        if self.page_of[n].is_some() {
                            self.sky_stale[n] = true;
                            self.bake_queue.push(n);
                            if geaendert_hier {
                                self.sofort[n] = true;
                            }
                        }
                    }
                }
            }
        }
        for c in licht_geaendert {
            let s = self.slots[c];
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let n = slot_index([s[0] + dx, s[1] + dy, s[2] + dz]);
                        if self.page_of[n].is_some() {
                            self.bake_queue.push(n);
                        }
                    }
                }
            }
        }
        // Weiter als die Nachbarn: eine Quelle leuchtet so weit wie ihre Stufe (Fernlicht bis
        // 255), ein Block wirft Schatten so weit wie das Licht, das ihn trifft. Diese Seiten nur
        // fuers Blocklicht neu (die Himmelssicht haengt an den Bloecken in der Naehe).
        let mut weit: Vec<(usize, i32)> = reichweiten;
        for &c in &geaendert {
            weit.push((c, self.reach_at_section(self.slots[c])));
        }
        for (c, reichweite) in weit {
            let s = self.slots[c];
            let r = ((reichweite + 15) / 16 + 1).min(SECTIONS / 2);
            for dy in -r..=r {
                for dz in -r..=r {
                    for dx in -r..=r {
                        let ziel = [s[0] + dx, s[1] + dy, s[2] + dz];
                        let n = slot_index(ziel);
                        if self.page_of[n].is_some() && self.slots[n][..3] == ziel {
                            self.bake_queue.push(n);
                        }
                    }
                }
            }
        }
        if self.bake_queue.is_empty() && self.bake_laufend.is_none() {
            return;
        }
        // Die naechsten zur Kamera zuerst.
        let kamera = eye.map(|e| e.div_euclid(16));
        let slots = &self.slots;
        let abstand = |i: &usize| (0..3).map(|k| (slots[*i][k] - kamera[k]).abs()).max().unwrap_or(0);
        self.bake_queue.sort_unstable_by_key(|i| (std::cmp::Reverse(abstand(i)), *i));
        self.bake_queue.dedup();
        let mut jobs = [[0i32; 4]; LM_BAKE_PER_FRAME + 1];
        // Bit je Auftrag: nur das Blocklicht, die Himmelssicht bleibt.
        let mut nur_licht = 0u32;
        loop {
            if self.bake_jobs.len() >= self.bake_budget {
                break;
            }
            // Eine geaenderte Section ganz nah an der Kamera (Block gesetzt) geht vor und wird in
            // einem Frame ganz gebacken: in Schritten saehen ihre Flaechen ein paar Frames lang
            // fremdes Licht (ihre Plaetze verschieben sich). Alles andere -- das Licht-Raster
            // wandert, eine Quelle weit weg -- in Schritten, ohne Ruckeln.
            let nah = self.bake_queue.last().is_some_and(|i| self.sofort[*i] && abstand(i) <= 1);
            let (i, seite, schritt, nur) = match self.bake_laufend.take() {
                Some(l) if !nah => l,
                laufend => {
                    if laufend.is_some() {
                        self.bake_laufend = laufend;
                    }
                    let Some(i) = self.bake_queue.pop() else { break };
                    let Some(seite) = self.page_of[i] else { continue };
                    let nur = !std::mem::take(&mut self.sky_stale[i]);
                    self.gebacken_mit[i] = Some(origin);
                    queue.write_buffer(&self.lm_buffer, (i * 4) as u64, bytemuck::bytes_of(&(seite + 1)));
                    (i, seite, 0, nur)
                }
            };
            if self.page_of[i] != Some(seite) {
                continue;
            }
            let schritte = self.flaechen_zahl(i).div_ceil(LM_FACES as u32 / LM_SCHRITTE).clamp(1, LM_SCHRITTE);
            let ganz = schritt == 0 && std::mem::take(&mut self.sofort[i]) && abstand(&i) <= 1;
            if ganz && self.bake_jobs.len() + schritte as usize > LM_BAKE_PER_FRAME {
                // Passt diesen Frame nicht mehr: im naechsten zuerst.
                self.bake_queue.push(i);
                self.sky_stale[i] = true;
                self.sofort[i] = true;
                break;
            }
            let bis = if ganz { schritte } else { schritt + 1 };
            let s = self.slots[i];
            for st in schritt..bis {
                if nur {
                    nur_licht |= 1 << self.bake_jobs.len();
                }
                jobs[self.bake_jobs.len()] = [s[0], s[1], s[2], (seite | st << 16) as i32];
                self.bake_jobs.push((i, seite, st));
            }
            if bis < schritte {
                // Ein naher Auftrag kann eine laufende Seite unterbrochen haben: die macht danach
                // weiter (sie liegt dann schon in bake_laufend).
                if self.bake_laufend.is_none() {
                    self.bake_laufend = Some((i, seite, bis, nur));
                } else {
                    self.bake_queue.push(i);
                }
            }
        }
        jobs[LM_BAKE_PER_FRAME] = [self.bake_jobs.len() as i32, nur_licht as i32, self.world_bottom, 0];
        queue.write_buffer(&self.bake_jobs_buffer, 0, bytemuck::cast_slice(&jobs));
    }
}

// Das Licht-Raster ab `origin`: je Zelle die bis zu LIGHTS_PER_CELL staerksten Quellen, deren
// Licht sie erreicht (Format s. deferred_light.wgsl, light_grid).
// Slots, die eine Lichtkarten-Seite brauchen: mit Bloecken geschrieben und im Licht-Raster ab
// `origin` -- auch ohne Quelle in Reichweite, fuer die Sicht auf den Himmel.
fn grid_slots(slots: &[[i32; 4]], origin: [i32; 3]) -> Vec<bool> {
    slots.iter().map(|&s| s[3] == SLOT_BLOCKS && in_grid(s, origin)).collect()
}

// Reichweite des Rueckwurfs ueber die Section hinaus: zwei Treffer zu je RUECK_WEITE (16, s.
// deferred_light.wgsl), dazu der Startpunkt bis einen Block daneben (Flaechenfilter).
const RUECK_REICHWEITE: i32 = 33;

// Liegt alles, was der Rueckwurf der Section `s` erreicht, im Licht-Raster ab `origin`? Das
// Licht an einem Treffer kennt er nur dort (light_cell): gebacken, als das Raster noch am Rand
// vorbeiging, blieb er zu dunkel -- im Spiel ein Strich an der Section-Grenze nach dem
// Zuruecklaufen, gemessen bis ein Drittel zu wenig (Test chunk_neu_laden).
fn rueckwurf_ganz(s: [i32; 4], origin: [i32; 3]) -> bool {
    let span = LIGHT_CELLS * LIGHT_CELL;
    (0..3).all(|k| s[k] * 16 - RUECK_REICHWEITE >= origin[k] && s[k] * 16 + 16 + RUECK_REICHWEITE <= origin[k] + span)
}

// Schneidet die Section des Slots `s` das Licht-Raster ab `origin`?
fn in_grid(s: [i32; 4], origin: [i32; 3]) -> bool {
    let span = LIGHT_CELLS * LIGHT_CELL;
    (0..3).all(|k| s[k] * 16 + 16 > origin[k] && s[k] * 16 < origin[k] + span)
}

// Wie weit ein Emitter-Eintrag (R | G << 4 | B << 8 | echte Stufe << 12) reicht, in Bloecken.
fn emitter_reach(e: u32) -> i32 {
    (e & 15).max(e >> 4 & 15).max(e >> 8 & 15).max(e >> 12 & 255) as i32
}

// Echte Stufe eines Emitter-Eintrags groesser als die der Ausbreitung (hoechster Kanal)?
fn is_far_light(e: u32) -> bool {
    e >> 12 > (e & 15).max(e >> 4 & 15).max(e >> 8 & 15)
}

// Die FAR_LIGHTS_MAX naechsten Fernlichter um `eye`: xyz = Weltblock, w = Emitter-Eintrag;
// danach w = 0.
fn far_lights(slots: &[[i32; 4]], emitters: &[Vec<(u16, u32)>], eye: [i32; 3]) -> [[i32; 4]; FAR_LIGHTS_MAX] {
    let mut alle: Vec<(i64, [i32; 4])> = Vec::new();
    for (i, liste) in emitters.iter().enumerate() {
        let slot = slots[i];
        if slot[3] != SLOT_BLOCKS {
            continue;
        }
        for &(j, e) in liste.iter().filter(|(_, e)| is_far_light(*e)) {
            let j = j as i32;
            let p = [slot[0] * 16 + j % 16, slot[1] * 16 + j / 256, slot[2] * 16 + (j / 16) % 16];
            let d: i64 = (0..3).map(|k| ((p[k] - eye[k]) as i64).pow(2)).sum();
            alle.push((d, [p[0], p[1], p[2], e as i32]));
        }
    }
    alle.sort_unstable_by_key(|&(d, _)| d);
    let mut out = [[0i32; 4]; FAR_LIGHTS_MAX];
    for (o, (_, l)) in out.iter_mut().zip(alle) {
        *o = l;
    }
    out
}

// Die sechs Aussenseiten einer Section (Index in rand_belegt/rand_voll): -x, +x, -y, +y, -z,
// +z; die Gegenseite ist Index ^ 1.
const RAND_RICHTUNGEN: [[i32; 3]; 6] = [[-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1]];

// Bit eines Blocks (x, y, z) in der Maske der Aussenseite `r`: die beiden anderen Achsen, in
// derselben Reihenfolge fuer beide Seiten einer Grenze.
fn rand_bit(r: usize, x: i32, y: i32, z: i32) -> usize {
    let (a, b) = match r / 2 {
        0 => (y, z),
        1 => (x, z),
        _ => (x, y),
    };
    (a * 16 + b) as usize
}

// Blockflaechen einer Section, die eine Lichtkarte bekommen koennen (s. deferred_light.wgsl,
// cs_lm_faces: vor ihr kein voller Block und nicht derselbe), soweit sie in der Section liegen;
// dazu je Aussenseite, welche Bloecke dort belegt und welche voll sind (fuer die Flaechen am
// Rand, s. flaechen_zahl).
fn flaechen_innen(klassen: &[u16; world::chunk::SECTION_VOLUME]) -> (u32, [[u64; 4]; 6], [[u64; 4]; 6]) {
    let mut zahl = 0;
    let mut belegt = [[0u64; 4]; 6];
    let mut voll = [[0u64; 4]; 6];
    for y in 0..16i32 {
        for z in 0..16i32 {
            for x in 0..16i32 {
                let k = klassen[((y * 16 + z) * 16 + x) as usize];
                if k == world::voxel::CLASS_AIR {
                    continue;
                }
                for (r, d) in RAND_RICHTUNGEN.iter().enumerate() {
                    let (nx, ny, nz) = (x + d[0], y + d[1], z + d[2]);
                    if !(0..16).contains(&nx) || !(0..16).contains(&ny) || !(0..16).contains(&nz) {
                        let bit = rand_bit(r, x, y, z);
                        belegt[r][bit / 64] |= 1 << (bit % 64);
                        if k == world::voxel::CLASS_FULL {
                            voll[r][bit / 64] |= 1 << (bit % 64);
                        }
                        continue;
                    }
                    let n = klassen[((ny * 16 + nz) * 16 + nx) as usize];
                    if n != world::voxel::CLASS_FULL && n != k {
                        zahl += 1;
                    }
                }
            }
        }
    }
    (zahl, belegt, voll)
}

// Ergebnis eines Baus des Licht-Rasters (s. VoxelVolume::update_lights).
struct LightBuild {
    grid: LightGrid,
    far: [[i32; 4]; FAR_LIGHTS_MAX],
    origin: [i32; 3],
}

// Das Licht-Raster auf der CPU, wie es auf der GPU liegt, samt Wertungen: verschiebt es sich
// mit der Kamera, bleiben die Zellen, die schon drin waren -- neu gerechnet wird nur der
// Streifen, der hinzukommt. Bei vielen Fernlichtern (jedes reicht in fast jede Zelle) ist
// der ganze Aufbau sonst mehrere Millisekunden je acht Bloecke Weg.
#[derive(Clone)]
struct LightGrid {
    origin: [i32; 3],
    grid: Vec<[u32; 2]>,
    scores: Vec<f32>,
    counts: Vec<usize>,
    // Je volle Zelle: Wertung und Platz ihrer schwaechsten Quelle.
    schwaechste: Vec<(f32, usize)>,
}

// Position einer Quelle relativ zum Raster-Ursprung, gepackt (s. deferred_light.wgsl,
// light_grid); None, wenn sie nicht hineinpasst.
fn pack_light_pos(rel: [i32; 3]) -> Option<u32> {
    if rel.iter().any(|&r| r + LIGHT_POS_BIAS < 0 || r + LIGHT_POS_BIAS > 1023) {
        return None;
    }
    Some(((rel[0] + LIGHT_POS_BIAS) | (rel[1] + LIGHT_POS_BIAS) << 10 | (rel[2] + LIGHT_POS_BIAS) << 20) as u32)
}

fn unpack_light_pos(p: u32) -> [i32; 3] {
    [0, 10, 20].map(|s| (p >> s & 1023) as i32 - LIGHT_POS_BIAS)
}

// Das Licht-Raster ab `origin`: je Zelle die bis zu LIGHTS_PER_CELL staerksten Quellen, deren
// Licht sie erreicht (Format s. deferred_light.wgsl, light_grid). Aus `alt` (dieselben
// Quellen, anderer Ursprung) wird uebernommen, was in beiden liegt.
fn build_light_grid(slots: &[[i32; 4]], emitters: &[Vec<(u16, u32)>], origin: [i32; 3], alt: Option<&LightGrid>) -> LightGrid {
    let n = LIGHT_CELLS as usize;
    let mut g = LightGrid {
        origin,
        grid: vec![[0u32; 2]; LIGHT_GRID_LEN],
        scores: vec![f32::NEG_INFINITY; LIGHT_GRID_LEN],
        counts: vec![0usize; n * n * n],
        schwaechste: vec![(f32::NEG_INFINITY, 0usize); n * n * n],
    };
    let zelle_index = |c: [i32; 3]| ((c[1] * LIGHT_CELLS + c[2]) * LIGHT_CELLS + c[0]) as usize;
    let mut fertig = vec![false; n * n * n];
    if let Some(alt) = alt {
        let versatz = [0, 1, 2].map(|k| (origin[k] - alt.origin[k]) / LIGHT_CELL);
        let pos_versatz = [0, 1, 2].map(|k| alt.origin[k] - origin[k]);
        'zellen: for cy in 0..LIGHT_CELLS {
            for cz in 0..LIGHT_CELLS {
                for cx in 0..LIGHT_CELLS {
                    let c = [cx, cy, cz];
                    let a = [0, 1, 2].map(|k| c[k] + versatz[k]);
                    if a.iter().any(|&v| !(0..LIGHT_CELLS).contains(&v)) {
                        continue;
                    }
                    let (zn, za) = (zelle_index(c), zelle_index(a));
                    let (bn, ba) = (zn * LIGHTS_PER_CELL, za * LIGHTS_PER_CELL);
                    for k in 0..alt.counts[za] {
                        let e = alt.grid[ba + k];
                        let r = unpack_light_pos(e[0]);
                        let Some(p) = pack_light_pos([0, 1, 2].map(|i| r[i] + pos_versatz[i])) else {
                            // Passt nicht mehr hinein: die Zelle neu rechnen.
                            g.counts[zn] = 0;
                            continue 'zellen;
                        };
                        g.grid[bn + k] = [p, e[1]];
                        g.scores[bn + k] = alt.scores[ba + k];
                    }
                    g.counts[zn] = alt.counts[za];
                    g.schwaechste[zn] = alt.schwaechste[za];
                    fertig[zn] = true;
                }
            }
        }
    }
    // Je Zeile (cy, cz) die Spalten, die noch zu rechnen sind (nach einer Verschiebung ein
    // Stueck am Rand oder die ganze Zeile).
    let offen: Vec<Option<(i32, i32)>> = (0..n * n)
        .map(|z| {
            let (cy, cz) = ((z / n) as i32, (z % n) as i32);
            let xs: Vec<i32> = (0..LIGHT_CELLS).filter(|&cx| !fertig[zelle_index([cx, cy, cz])]).collect();
            Some((*xs.first()?, *xs.last()?))
        })
        .collect();
    if offen.iter().all(Option::is_none) {
        return g;
    }
    let span = LIGHT_CELLS * LIGHT_CELL;
    // Der Bereich der noch zu rechnenden Zellen (Bloecke relativ zu `origin`): nach einer
    // Verschiebung nur ein Streifen am Rand. Quellen, deren Licht ihn nicht erreicht, und ganze
    // Sections ohne solche Quellen werden uebersprungen -- sonst ging jeder Schritt ueber eine
    // Zellgrenze alle Quellen im Wuerfel durch (ein Lavasee: zig Millisekunden in einem Frame).
    let mut offen_lo = [i32::MAX; 3];
    let mut offen_hi = [i32::MIN; 3];
    for (z, o) in offen.iter().enumerate() {
        let Some((x0, x1)) = *o else { continue };
        let c = [[x0, (z / n) as i32, (z % n) as i32], [x1, (z / n) as i32, (z % n) as i32]];
        for k in 0..3 {
            offen_lo[k] = offen_lo[k].min(c[0][k] * LIGHT_CELL);
            offen_hi[k] = offen_hi[k].max(c[1][k] * LIGHT_CELL + LIGHT_CELL);
        }
    }
    let erreicht = |lo: [i32; 3], hi: [i32; 3], r: i32| (0..3).all(|k| hi[k] + r > offen_lo[k] && lo[k] - r < offen_hi[k]);
    // Fernlichter reichen bis Stufe 255 (s. world::light::emission_level).
    let reichweite = 255;
    let sec_lo = origin.map(|o| (o - reichweite).div_euclid(16));
    let sec_hi = origin.map(|o| (o + span + reichweite).div_euclid(16));
    for sy in sec_lo[1]..=sec_hi[1] {
        for sz in sec_lo[2]..=sec_hi[2] {
            for sx in sec_lo[0]..=sec_hi[0] {
                let s = [sx, sy, sz];
                let i = slot_index(s);
                let slot = slots[i];
                if slot[3] != SLOT_BLOCKS || slot[..3] != s || emitters[i].is_empty() {
                    continue;
                }
                let sec_rel = [sx * 16 - origin[0], sy * 16 - origin[1], sz * 16 - origin[2]];
                let sec_weit = emitters[i].iter().map(|&(_, e)| emitter_reach(e)).max().unwrap_or(0);
                if !erreicht(sec_rel, sec_rel.map(|v| v + 16), sec_weit) {
                    continue;
                }
                for &(j, e) in &emitters[i] {
                    let j = j as i32;
                    let rel = [sx * 16 + j % 16 - origin[0], sy * 16 + j / 256 - origin[1], sz * 16 + (j / 16) % 16 - origin[2]];
                    // Echte Stufe: Fernlichter reichen so weit wie sie, nicht nur 15.
                    let level = (e & 15).max(e >> 4 & 15).max(e >> 8 & 15).max(e >> 12) as f32;
                    if !erreicht(rel, rel.map(|v| v + 1), level as i32) {
                        continue;
                    }
                    let Some(pos) = pack_light_pos(rel) else { continue };
                    let packed = [pos, e];
                    let mitte = rel.map(|r| r as f32 + 0.5);
                    let r = level as i32;
                    let lo = rel.map(|v| (v - r).div_euclid(LIGHT_CELL).clamp(0, LIGHT_CELLS - 1));
                    let hi = rel.map(|v| (v + r).div_euclid(LIGHT_CELL).clamp(0, LIGHT_CELLS - 1));
                    // Je Achse und Zellenspalte: Abstand der Quelle zur Zelle (0 innerhalb) und
                    // zu ihrer Mitte, quadriert -- einmal je Quelle statt je Zelle.
                    let mut naechst_achse = [[0.0f32; LIGHT_CELLS as usize]; 3];
                    let mut mitte_achse = [[0.0f32; LIGHT_CELLS as usize]; 3];
                    for k in 0..3 {
                        for c in lo[k]..=hi[k] {
                            let a = (c * LIGHT_CELL) as f32;
                            let d = (a - mitte[k]).max(mitte[k] - a - LIGHT_CELL as f32).max(0.0);
                            naechst_achse[k][c as usize] = d * d;
                            let m = a + LIGHT_CELL as f32 / 2.0 - mitte[k];
                            mitte_achse[k][c as usize] = m * m;
                        }
                    }
                    for cy in lo[1]..=hi[1] {
                        for cz in lo[2]..=hi[2] {
                            let Some((x0, x1)) = offen[cy as usize * n + cz as usize] else { continue };
                            let naechst_yz = naechst_achse[1][cy as usize] + naechst_achse[2][cz as usize];
                            if naechst_yz >= level * level {
                                continue;
                            }
                            for cx in lo[0].max(x0)..=hi[0].min(x1) {
                                let zelle = zelle_index([cx, cy, cz]);
                                // Schon uebernommen, oder erreicht ihr Licht die Zelle gar nicht?
                                if fertig[zelle] || naechst_yz + naechst_achse[0][cx as usize] >= level * level {
                                    continue;
                                }
                                let zur_mitte = mitte_achse[0][cx as usize] + mitte_achse[1][cy as usize] + mitte_achse[2][cz as usize];
                                let score = level - zur_mitte.sqrt();
                                let basis = zelle * LIGHTS_PER_CELL;
                                let platz = if g.counts[zelle] < LIGHTS_PER_CELL {
                                    g.counts[zelle] += 1;
                                    g.counts[zelle] - 1
                                } else {
                                    // Voll: die schwaechste ersetzen, falls diese staerker ist.
                                    // Die schwaechste ist gemerkt -- bei vielen Fernlichtern ist
                                    // fast jede Zelle voll, und meist bleibt es beim Vergleich.
                                    if score <= g.schwaechste[zelle].0 {
                                        continue;
                                    }
                                    g.schwaechste[zelle].1
                                };
                                g.grid[basis + platz] = packed;
                                g.scores[basis + platz] = score;
                                if g.counts[zelle] == LIGHTS_PER_CELL {
                                    let (k, &min) = g.scores[basis..basis + LIGHTS_PER_CELL]
                                        .iter()
                                        .enumerate()
                                        .min_by(|a, b| a.1.total_cmp(b.1))
                                        .expect("LIGHTS_PER_CELL > 0");
                                    g.schwaechste[zelle] = (min, k);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_sind_eindeutig_im_wuerfel() {
        let center = [-3, 4, 100];
        let min = VoxelVolume::window_min(center);
        let mut seen = vec![false; SLOT_COUNT];
        for y in 0..SECTIONS {
            for z in 0..SECTIONS {
                for x in 0..SECTIONS {
                    let i = slot_index([min[0] + x, min[1] + y, min[2] + z]);
                    assert!(!seen[i]);
                    seen[i] = true;
                }
            }
        }
    }

    // Eine Fackel (Stufe 14) mitten in Zelle (8, 8, 8): die Zelle selbst und die Nachbarn
    // fuehren sie, Zellen weiter als 14 Bloecke nicht.
    #[test]
    fn licht_raster_fuehrt_quellen_in_reichweite() {
        let origin = [0, 0, 0];
        let mut slots = vec![[0, 0, 0, SLOT_UNKNOWN]; SLOT_COUNT];
        let mut emitters = vec![Vec::new(); SLOT_COUNT];
        // Block (68, 68, 68) liegt in Section (4, 4, 4), dort bei (4, 4, 4).
        let s = [4, 4, 4];
        slots[slot_index(s)] = [4, 4, 4, SLOT_BLOCKS];
        let j = (4 * 16 + 4) * 16 + 4;
        let e: u32 = 14 | 10 << 4 | 6 << 8;
        emitters[slot_index(s)].push((j as u16, e));
        let grid = build_light_grid(&slots, &emitters, origin, None).grid;
        let zelle = |c: [i32; 3]| ((c[1] * LIGHT_CELLS + c[2]) * LIGHT_CELLS + c[0]) as usize * LIGHTS_PER_CELL;
        let erwartet = [(68 + LIGHT_POS_BIAS) as u32 * (1 | 1 << 10 | 1 << 20), e];
        assert_eq!(grid[zelle([8, 8, 8])], erwartet);
        assert_eq!(grid[zelle([7, 8, 8])], erwartet);
        assert_eq!(grid[zelle([8, 8, 8]) + 1], [0, 0]);
        // Zelle 10 beginnt bei 80: 11,5 Bloecke entfernt, erreicht.
        assert_eq!(grid[zelle([10, 8, 8])], erwartet);
        // Zelle 11 beginnt bei 88: 19,5 Bloecke, zu weit.
        assert_eq!(grid[zelle([11, 8, 8])], [0, 0]);
        // Diagonal: Ecke von Zelle (10, 10, 10) ist 11,5 * sqrt(3) = 19,9 entfernt.
        assert_eq!(grid[zelle([10, 10, 10])], [0, 0]);
    }

    // Mehr Quellen als Platz: die naechsten bleiben.
    #[test]
    fn licht_raster_behaelt_die_staerksten() {
        let mut slots = vec![[0, 0, 0, SLOT_UNKNOWN]; SLOT_COUNT];
        let mut emitters = vec![Vec::new(); SLOT_COUNT];
        // 20 gleich starke Quellen in einer Reihe, Welt-x 60..80, beiderseits einer Section-Grenze.
        for (s, xs) in [([3, 4, 4], 12..16u16), ([4, 4, 4], 0..16u16)] {
            slots[slot_index(s)] = [s[0], s[1], s[2], SLOT_BLOCKS];
            for x in xs {
                emitters[slot_index(s)].push(((4 * 16 + 4) * 16 + x, 15));
            }
        }
        let grid = build_light_grid(&slots, &emitters, [0, 0, 0], None).grid;
        let basis = ((8 * LIGHT_CELLS + 8) * LIGHT_CELLS + 8) as usize * LIGHTS_PER_CELL;
        let mut xs: Vec<i32> = grid[basis..basis + LIGHTS_PER_CELL].iter().map(|e| (e[0] & 1023) as i32 - LIGHT_POS_BIAS).collect();
        xs.sort();
        // Zelle 8 hat ihre Mitte bei x = 68: die 16 naechsten sind 60..=75.
        assert_eq!(xs, (60..76).collect::<Vec<_>>());
    }

    // Nur Quellen ueber ihrer Ausbreitungs-Stufe sind Fernlichter, die naechsten zuerst.
    #[test]
    fn fernlichter_naechste_zuerst() {
        let mut slots = vec![[0, 0, 0, SLOT_UNKNOWN]; SLOT_COUNT];
        let mut emitters = vec![Vec::new(); SLOT_COUNT];
        let s = [1, 4, 0];
        slots[slot_index(s)] = [s[0], s[1], s[2], SLOT_BLOCKS];
        let fern = |stufe: u32| 15 | 11 << 4 | 6 << 8 | stufe << 12;
        emitters[slot_index(s)] = vec![(5, fern(15)), (9, fern(40)), (2, fern(60))];
        let liste = far_lights(&slots, &emitters, [16, 64, 0]);
        assert_eq!(liste[0], [18, 64, 0, fern(60) as i32]);
        assert_eq!(liste[1], [25, 64, 0, fern(40) as i32]);
        assert_eq!(liste[2][3], 0);
    }

    // Verschoben ergibt dasselbe Raster wie neu gebaut (je Zelle dieselben Quellen; die
    // Reihenfolge darf abweichen).
    #[test]
    fn licht_raster_verschoben_wie_neu() {
        let mut slots = vec![[0, 0, 0, SLOT_UNKNOWN]; SLOT_COUNT];
        let mut emitters = vec![Vec::new(); SLOT_COUNT];
        for sx in 0..8 {
            for sz in 0..8 {
                let s = [sx, 4, sz];
                slots[slot_index(s)] = [s[0], s[1], s[2], SLOT_BLOCKS];
                for k in 0..3u16 {
                    let stufe = [14, 40, 140][k as usize];
                    emitters[slot_index(s)].push(((4 * 16 + k * 5) * 16 + k * 4 + sx as u16, 14 | 10 << 4 | 6 << 8 | stufe << 12));
                }
            }
        }
        let alt = build_light_grid(&slots, &emitters, [0, 0, 0], None);
        for ziel in [[8, 0, 0], [-16, 8, 24], [64, -8, 0]] {
            let verschoben = build_light_grid(&slots, &emitters, ziel, Some(&alt));
            let neu = build_light_grid(&slots, &emitters, ziel, None);
            for z in 0..(LIGHT_CELLS * LIGHT_CELLS * LIGHT_CELLS) as usize {
                let zelle = |g: &LightGrid| {
                    let mut v = g.grid[z * LIGHTS_PER_CELL..z * LIGHTS_PER_CELL + g.counts[z]].to_vec();
                    v.sort();
                    v
                };
                assert_eq!(zelle(&verschoben), zelle(&neu), "Zelle {z} nach {ziel:?}");
            }
        }
    }
}
