// Chunk-Saver in der App: Chunks an das Crate chunk_saver weiterreichen, die Fernsicht je
// Frame waehlen lassen und an render::lod_renderer geben.
//
// Das Modul "Chunk-Saver" SPEICHERT (Schalter und Threads). Ob und wie weit die Fernsicht
// gezeichnet wird, steht im Video-Tab (LOD-Render-Distance, Detail, Nebel) -- sie zeichnet,
// was bisher gespeichert wurde, auch wenn das Modul gerade aus ist.
//
// Dazu die Modul-Option "Gespeicherte Chunks zeichnen": das Gespeicherte auch innerhalb der
// normalen Sichtweite zeigen, wo (noch) keine echten Chunks sind. Beim Verbinden steht die
// Kamera dafuer schon an der Stelle, an der man diesen Server zuletzt verlassen hat (s.
// lod_preview_saved) -- man sieht die Welt, bevor der Server den ersten Chunk schickt. Die
// erste Position vom Server setzt einen dann dorthin, wo man wirklich ist.
//
// Gespeichert wird je Server, Dimension und Seed (s. chunk_saver::world_dir) im Datenordner.
// Ein Chunk geht beim Eintreffen in den Speicher, und noch einmal beim Entladen, wenn sich in
// ihm etwas geaendert hat (Block-Updates) -- so steht auch Gebautes in der Fernsicht.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashSet as HashSet;

use crate::{App, ButtonAction, ModuleRow, Settings};

pub const THREADS_MAX: u32 = 8;
pub const THREADS_DEFAULT: u32 = 3;
// Unterteilungsschwelle: wie gross ein Voxel hoechstens auf dem Bildschirm sein darf, in
// Pixel^2. Bei 4 sind es 1 bis 2 Pixel -- gemessen waren bei Voxys 28 am Rand der Sichtweite
// 32 schon 3 Pixel, weiter draussen bis 5, und das sah grobpixelig aus.
pub const THRESHOLD_DEFAULT: u32 = 4;
pub const THRESHOLD_MIN: u32 = 1;
pub const THRESHOLD_MAX: u32 = 32;
const THRESHOLDS: [u32; 10] = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32];
// LOD-Render-Distance in Chunks, bis 650 wie bei Voxy. 0 = aus, sonst ab DISTANCE_STEP.
pub const DISTANCE_OFF: u32 = 0;
pub const DISTANCE_MAX: u32 = 650;
pub const DISTANCE_STEP: u32 = 32;
// Was eine alte settings.conf mit eingeschalteter Fernsicht ohne eigene Weite bekommt.
pub const DISTANCE_LEGACY_DEFAULT: u32 = 128;
// Geaenderte Chunks werden spaetestens so oft gespeichert, auch wenn sie geladen bleiben.
const SAVE_DIRTY_EVERY: Duration = Duration::from_secs(60);
// Datei im Server-Ordner (s. chunk_saver::server_dir) mit dem letzten Standort.
const LAST_POSE_FILE: &str = "last_position.txt";

// Wo man auf einem Server zuletzt war: Welt, Fuesse und Blick.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LastPose {
    server: String,
    dimension: String,
    seed: i64,
    pos: [f64; 3],
    yaw: f32,
    pitch: f32,
}

impl LastPose {
    fn to_text(&self) -> String {
        format!(
            "dimension={}\nseed={}\npos={} {} {}\nlook={} {}\n",
            self.dimension, self.seed, self.pos[0], self.pos[1], self.pos[2], self.yaw, self.pitch
        )
    }

    fn parse(server: &str, text: &str) -> Option<Self> {
        let (mut dimension, mut seed, mut pos, mut look) = (None, None, None, None);
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let zahlen = |v: &str| v.split_whitespace().map(|z| z.parse::<f64>().ok().filter(|z| z.is_finite())).collect::<Option<Vec<f64>>>();
            match k.trim() {
                "dimension" => dimension = Some(v.trim().to_string()),
                "seed" => seed = v.trim().parse::<i64>().ok(),
                "pos" => pos = zahlen(v).filter(|z| z.len() == 3).map(|z| [z[0], z[1], z[2]]),
                "look" => look = zahlen(v).filter(|z| z.len() == 2).map(|z| (z[0] as f32, z[1] as f32)),
                _ => {}
            }
        }
        let (yaw, pitch) = look?;
        Some(Self { server: server.to_string(), dimension: dimension.filter(|d| !d.is_empty())?, seed: seed?, pos: pos?, yaw, pitch })
    }
}

#[derive(Default)]
pub(crate) struct LodState {
    saver: Option<chunk_saver::ChunkSaver>,
    // Dimension und Seed der aktuellen Welt (s. net::ClientEvent::WorldIdentity).
    identity: Option<(String, i64)>,
    // Chunks mit Block-Updates seit ihrer letzten Aufnahme.
    pub dirty_columns: HashSet<(i32, i32)>,
    // Die Chunks, die in der Maske der GPU als "echtes Gelaende" stehen, und ihr Rechteck.
    covered: HashSet<(i32, i32)>,
    covered_box: Option<(i32, i32, i32, i32)>,
    // render_limit_meshed hat sich geaendert -- Maske und Auswahl nachziehen.
    pub mask_changed: bool,
    // Fuer welchen Atlas die Rechtecke auf der GPU gelten (Zeiger, nur zum Vergleich).
    rects_atlas: usize,
    dirty_saved_at: Option<Instant>,
    // Ob beim letzten Frame gespeichert wurde -- beim Einschalten des Moduls (auch bei schon
    // laufender Fernsicht) wird alles bereits Geladene einmal nachgeholt.
    ingest_active: bool,
    // Der letzte Standort in dieser Verbindung, geschrieben beim Trennen (s. lod_remember_pose).
    pose: Option<LastPose>,
}

impl LodState {
    // Strahl durch die gespeicherte Welt der Fernsicht (s. chunk_saver::LodWorld::raycast).
    pub fn raycast(&self, from: [f64; 3], dir: [f64; 3], reach: f64) -> Option<([i64; 3], usize, i64)> {
        self.saver.as_ref()?.raycast(from, dir, reach)
    }
}

pub fn apply_action(settings: &mut Settings, action: ButtonAction) {
    let umschalten = |b: &mut bool| *b = !*b;
    match action {
        ButtonAction::ToggleModuleChunkSaver => umschalten(&mut settings.module_chunk_saver),
        ButtonAction::ToggleChunkSaverRenderSaved => umschalten(&mut settings.chunk_saver_render_saved),
        ButtonAction::ChunkSaverThreadsPrev => settings.chunk_saver_threads = settings.chunk_saver_threads.saturating_sub(1).max(1),
        ButtonAction::ChunkSaverThreadsNext => settings.chunk_saver_threads = (settings.chunk_saver_threads + 1).min(THREADS_MAX),
        ButtonAction::ChunkSaverThresholdPrev => {
            settings.chunk_saver_threshold = THRESHOLDS.iter().rev().copied().find(|&t| t < settings.chunk_saver_threshold).unwrap_or(THRESHOLD_MIN)
        }
        ButtonAction::ChunkSaverThresholdNext => {
            settings.chunk_saver_threshold = THRESHOLDS.iter().copied().find(|&t| t > settings.chunk_saver_threshold).unwrap_or(THRESHOLD_MAX)
        }
        ButtonAction::ChunkSaverDistancePrev => settings.lod_render_distance = distance_step(settings.lod_render_distance, false),
        ButtonAction::ChunkSaverDistanceNext => settings.lod_render_distance = distance_step(settings.lod_render_distance, true),
        _ => {}
    }
}

// Naechste Stufe der LOD-Render-Distance: aus, dann 32er-Schritte bis zum Maximum (das selbst
// keine 32 teilt). Ein frei eingetragener Wert dazwischen rastet auf die naechste Stufe ein.
pub fn distance_step(d: u32, up: bool) -> u32 {
    let letzte = DISTANCE_MAX - DISTANCE_MAX % DISTANCE_STEP;
    if up {
        ((d / DISTANCE_STEP + 1) * DISTANCE_STEP).min(DISTANCE_MAX)
    } else if d > letzte {
        letzte
    } else {
        (d.div_ceil(DISTANCE_STEP) * DISTANCE_STEP).saturating_sub(DISTANCE_STEP)
    }
}

// Anzeige der LOD-Render-Distance.
pub fn distance_label(d: u32) -> String {
    if d == DISTANCE_OFF { "Aus".to_string() } else { format!("{d} Chunks") }
}

impl App {
    pub(crate) fn chunk_saver_rows(&self) -> Vec<ModuleRow> {
        let set = &self.settings;
        vec![
            ModuleRow::Stepper {
                label: "Threads".to_string(),
                prev: ButtonAction::ChunkSaverThreadsPrev,
                next: ButtonAction::ChunkSaverThreadsNext,
                text: set.chunk_saver_threads.to_string(),
                can_prev: set.chunk_saver_threads > 1,
                can_next: set.chunk_saver_threads < THREADS_MAX,
                slider: Some((set.chunk_saver_threads as f64, 1.0, THREADS_MAX as f64)),
            },
            ModuleRow::Toggle {
                label: "Gespeicherte Chunks zeichnen".to_string(),
                action: ButtonAction::ToggleChunkSaverRenderSaved,
                on: set.chunk_saver_render_saved,
            },
        ]
    }

    fn render_saved(&self) -> bool {
        self.settings.module_chunk_saver && self.settings.chunk_saver_render_saved
    }

    // Wie weit die Fernsicht zeichnet, in Chunks: die LOD-Render-Distance, mit "Gespeicherte
    // Chunks zeichnen" mindestens die normale Sichtweite.
    fn lod_radius_chunks(&self) -> u32 {
        let gespeichert = if self.render_saved() { self.settings.max_render_distance } else { DISTANCE_OFF };
        self.settings.lod_render_distance.max(gespeichert)
    }

    // Beim Verbinden (s. start_connect): die alte Welt schliessen und, wenn gewuenscht, die
    // Welt oeffnen, in der man diesen Server zuletzt verlassen hat, mit der Kamera an derselben
    // Stelle. Meldet der Server eine andere Welt, wechselt lod_world_identity dorthin.
    pub(crate) fn lod_preview_saved(&mut self) {
        self.lod.identity = None;
        if let Some(saver) = self.lod.saver.as_mut() {
            saver.close_world();
        }
        if !self.render_saved() {
            return;
        }
        let Some(base) = assets::data_dir() else { return };
        let datei = chunk_saver::server_dir(&base, &self.server_address).join(LAST_POSE_FILE);
        let Some(pose) = std::fs::read_to_string(datei).ok().and_then(|t| LastPose::parse(&self.server_address, &t)) else { return };
        self.lod.identity = Some((pose.dimension.clone(), pose.seed));
        if let Some(dir) = self.lod_dir() {
            if let Some(saver) = self.lod.saver.as_mut() {
                saver.open_world(&dir);
            }
        }
        let fuesse = glam::DVec3::from_array(pose.pos);
        self.physics.pos = fuesse;
        self.physics_render_prev = fuesse;
        self.camera.yaw = pose.yaw;
        self.camera.pitch = pose.pitch;
    }

    // Den letzten Standort dieser Verbindung in den Ordner ihres Servers schreiben (Trennen,
    // neue Verbindung, Beenden).
    pub(crate) fn lod_remember_pose(&mut self) {
        let Some(pose) = self.lod.pose.take() else { return };
        let Some(base) = assets::data_dir() else { return };
        let ordner = chunk_saver::server_dir(&base, &pose.server);
        if std::fs::create_dir_all(&ordner).is_ok() {
            let _ = std::fs::write(ordner.join(LAST_POSE_FILE), pose.to_text());
        }
    }

    // Jeden Frame, sobald der Server uns platziert hat: Standort fuer lod_remember_pose.
    fn lod_track_pose(&mut self) {
        if !self.settings.module_chunk_saver || !self.camera_placed {
            return;
        }
        let Some((dimension, seed)) = self.lod.identity.as_ref() else { return };
        let (pos, yaw, pitch) = (self.physics.pos.to_array(), self.camera.yaw, self.camera.pitch);
        match self.lod.pose.as_mut() {
            Some(p) if p.server == self.server_address && p.dimension == *dimension && p.seed == *seed => (p.pos, p.yaw, p.pitch) = (pos, yaw, pitch),
            _ => {
                self.lod.pose =
                    Some(LastPose { server: self.server_address.clone(), dimension: dimension.clone(), seed: *seed, pos, yaw, pitch })
            }
        }
    }

    fn lod_dir(&self) -> Option<std::path::PathBuf> {
        let (dimension, seed) = self.lod.identity.as_ref()?;
        Some(chunk_saver::world_dir(&assets::data_dir()?, &self.server_address, dimension, *seed))
    }

    fn lod_ingesting(&self) -> bool {
        self.settings.module_chunk_saver && self.lod.saver.as_ref().is_some_and(|s| s.has_world())
    }

    // Login oder Respawn: welche Welt. Erst noch Geaendertes der alten speichern -- deren
    // Chunks sind beim Respawn noch geladen (Respawned kommt danach).
    pub(crate) fn lod_world_identity(&mut self, dimension: String, seed: i64) {
        self.lod_save_dirty();
        self.lod.identity = Some((dimension, seed));
        if let Some(dir) = self.lod_dir() {
            if let Some(saver) = self.lod.saver.as_mut() {
                saver.open_world(&dir);
            }
        }
    }

    pub(crate) fn lod_ingest_column(&mut self, cx: i32, cz: i32) {
        self.lod.dirty_columns.remove(&(cx, cz));
        if !self.lod_ingesting() {
            return;
        }
        if let (Some(saver), Some(nachbarn)) = (self.lod.saver.as_ref(), self.world.column_neighbors(cx, cz)) {
            saver.ingest(cx, cz, nachbarn);
        }
    }

    // Ein Chunk wird entladen: hat sich in ihm etwas geaendert, den letzten Stand speichern.
    pub(crate) fn lod_before_forget(&mut self, cx: i32, cz: i32) {
        if self.lod.dirty_columns.contains(&(cx, cz)) {
            self.lod_ingest_column(cx, cz);
        }
    }

    // Alle geaenderten, noch geladenen Chunks speichern (Weltwechsel, Trennen, regelmaessig).
    pub(crate) fn lod_save_dirty(&mut self) {
        let dirty: Vec<(i32, i32)> = self.lod.dirty_columns.iter().copied().collect();
        for (cx, cz) in dirty {
            self.lod_ingest_column(cx, cz);
        }
        self.lod.dirty_columns.clear();
        self.lod.dirty_saved_at = Some(Instant::now());
    }

    // Beim Beenden: der Chunk-Saver schreibt alles weg und beendet seine Threads (Drop).
    pub(crate) fn lod_shutdown(&mut self) {
        self.lod_remember_pose();
        self.lod.saver = None;
    }

    pub(crate) fn lod_debug_line(&self) -> String {
        let Some(saver) = self.lod.saver.as_ref() else { return String::new() };
        let (wartend, knoten, laufend) = saver.stats();
        let flaechen = self.gpu.as_ref().and_then(|g| g.lod.as_ref()).map_or(0, |l| l.quad_count());
        format!(
            "Fernsicht: {knoten} Knoten, {:.2} Mio. Flaechen ({} MB)  |  Aufnahme wartet: {wartend}  |  Mesh-Auftraege: {laufend}",
            flaechen as f64 / 1e6,
            (flaechen * std::mem::size_of::<chunk_saver::Quad>() as u64) >> 20
        )
    }

    // Einmal je Frame vor dem Zeichnen der Welt.
    pub(crate) fn update_chunk_saver(&mut self) {
        // Gebraucht wird er zum Speichern (Modul an) oder zum Zeichnen der Fernsicht.
        if !self.settings.module_chunk_saver && self.settings.lod_render_distance == DISTANCE_OFF {
            // Beim Abschalten speichert der Chunk-Saver noch alles und beendet seine Threads.
            self.lod.saver = None;
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.lod_active = false;
                gpu.lod = None;
            }
            return;
        }
        if self.lod.saver.is_none() {
            let classes = world::voxel::VoxelClasses::build(&self.block_shapes, &self.cross_blocks, &self.block_uvs);
            let mut states = chunk_saver::LodStates::from_voxel_classes(&classes);
            // Dieselben Abdunklungs-Regeln wie das nahe Gelaende (s. LodStates::set_ao_flags).
            states.set_ao_flags(|s| self.block_shapes.ao_flags(s));
            let states = Arc::new(states);
            let mut saver = chunk_saver::ChunkSaver::new(states, self.settings.chunk_saver_threads as usize);
            if let Some(dir) = self.lod_dir() {
                saver.open_world(&dir);
            }
            self.lod.saver = Some(saver);
            self.lod.ingest_active = false;
        }
        let ingest = self.lod_ingesting();
        if ingest && !self.lod.ingest_active {
            // Was schon geladen ist, gleich mitnehmen.
            let keys: Vec<(i32, i32)> = self.world.column_keys().collect();
            for (cx, cz) in keys {
                self.lod_ingest_column(cx, cz);
            }
        }
        self.lod.ingest_active = ingest;
        self.lod_track_pose();
        if self.lod.dirty_saved_at.is_none_or(|t| t.elapsed() > SAVE_DIRTY_EVERY) {
            self.lod_save_dirty();
        }
        let radius_chunks = self.lod_radius_chunks();
        let Some(saver) = self.lod.saver.as_mut() else { return };
        saver.set_threads(self.settings.chunk_saver_threads as usize);
        let Some(gpu) = self.gpu.as_mut() else { return };
        // Der Shader-Pfad zeichnet die Fernsicht noch nicht; gespeichert wird trotzdem.
        if radius_chunks == DISTANCE_OFF || gpu.use_shader_path || !saver.has_world() {
            saver.forget_meshes();
            gpu.lod_active = false;
            gpu.lod = None;
            return;
        }
        if gpu.lod.is_none() {
            gpu.lod = Some(render::lod_renderer::LodRenderer::new(&gpu.device, gpu.config.format, gpu.config.width, gpu.config.height));
            self.lod.covered.clear();
            self.lod.mask_changed = true;
            self.lod.rects_atlas = 0;
            saver.forget_meshes();
        }
        let Some(lod) = gpu.lod.as_mut() else { return };
        let atlas = gpu.block_renderer.atlas_texture_shared();
        let atlas_id = Arc::as_ptr(&atlas) as usize;
        if self.lod.rects_atlas != atlas_id {
            // Neuer Atlas (Resourcepack): die Kacheln liegen woanders.
            let classes = world::voxel::VoxelClasses::build(&self.block_shapes, &self.cross_blocks, &self.block_uvs);
            lod.set_state_rects(&gpu.device, &gpu.queue, &classes.state_info, world::voxel::STATE_INFO_WORTE);
            self.lod.rects_atlas = atlas_id;
        }
        lod.set_atlas(atlas);

        let mask_changed = std::mem::take(&mut self.lod.mask_changed);
        if mask_changed {
            let meshed = &self.render_limit_meshed;
            for &k in self.lod.covered.iter().filter(|k| !meshed.contains(k)) {
                lod.set_column_covered(k.0, k.1, false);
            }
            for &k in meshed.iter().filter(|k| !self.lod.covered.contains(k)) {
                lod.set_column_covered(k.0, k.1, true);
            }
            self.lod.covered = meshed.clone();
            self.lod.covered_box = meshed.iter().fold(None, |b, &(x, z)| match b {
                None => Some((x, z, x, z)),
                Some((x0, z0, x1, z1)) => Some((x0.min(x), z0.min(z), x1.max(x), z1.max(z))),
            });
        }
        saver.set_palette(self.biome_palette.clone());
        let height = gpu.config.height.max(1) as f64;
        let fov = self.camera.fov_degrees as f64;
        let min_y = self.world.min_y() as i64;
        let view = chunk_saver::View {
            eye: [self.camera.position.x, self.camera.position.y, self.camera.position.z],
            radius: radius_chunks as f64 * 16.0,
            threshold_px2: self.settings.chunk_saver_threshold as f64,
            focal_px: height / (2.0 * (fov.to_radians() * 0.5).tan()),
            y_range: (min_y, min_y + self.world_height.max(16) as i64),
        };
        let (covered, bbox) = (&self.lod.covered, self.lod.covered_box);
        let ganz_geladen = |level: u8, x: i32, z: i32| -> bool {
            let Some((bx0, bz0, bx1, bz1)) = bbox else { return false };
            let n = 2i32 << level;
            let (x0, z0) = (x * n, z * n);
            if x0 < bx0 || z0 < bz0 || x0 + n - 1 > bx1 || z0 + n - 1 > bz1 {
                return false;
            }
            (x0..x0 + n).all(|cx| (z0..z0 + n).all(|cz| covered.contains(&(cx, cz))))
        };
        let out = saver.update(&view, &ganz_geladen, mask_changed);
        for k in &out.remove {
            lod.remove(*k);
        }
        for (k, mesh) in &out.upload {
            lod.upload(&gpu.device, &gpu.queue, *k, mesh);
        }
        let aspect = gpu.config.width as f32 / gpu.config.height.max(1) as f32;
        // Mitte des Kreises wie bei enforce_render_limits: der Chunk des Spielers, nicht der Kamera.
        let mitte = ((self.physics.pos.x / 16.0).floor() as i32, (self.physics.pos.z / 16.0).floor() as i32);
        lod.prepare(
            &gpu.device,
            &gpu.queue,
            &render::lod_renderer::LodFrame {
                draw: &out.draw,
                camera: &self.camera,
                aspect,
                mask: true,
                ao_strength: self.settings.smooth_lighting_percent as f32 / 100.0,
                clip: (mitte, radius_chunks),
            },
        );
        gpu.lod_active = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letzter_standort_hin_und_zurueck() {
        let pose = LastPose { server: "a.b".into(), dimension: "minecraft:overworld".into(), seed: -42, pos: [1.5, 64.0, -300.25], yaw: 91.5, pitch: -12.0 };
        assert_eq!(LastPose::parse("a.b", &pose.to_text()), Some(pose));
        // Kaputte oder unvollstaendige Dateien geben nichts.
        assert_eq!(LastPose::parse("a.b", "dimension=x\nseed=1\npos=1 2\nlook=0 0\n"), None);
        assert_eq!(LastPose::parse("a.b", "dimension=x\nseed=1\npos=1 2 NaN\nlook=0 0\n"), None);
    }
}
