// Modul "Chunk-Saver": Fernsicht aus gespeicherten Chunks, im Funktionsumfang wie die Mod Voxy
// (deren Code wird nicht angesehen, die Umsetzung ist eigen).
//
// Jeder Chunk, den der Server schickt, wird lokal gespeichert -- je Server, Dimension und
// Welt-Seed (s. world_dir). Daraus entstehen Stufen immer groeberer Voxel (s. node), und
// jenseits der Sichtweite des Servers zeichnet der Client diese Stufen, nah fein und fern grob
// (s. select). Wo man nie war, bleibt es leer.
//
// Threads:
// - ein Aufnahme-Thread schreibt neue Chunks in die Stufen und auf die Platte (s. ingest),
// - einstellbar viele Mesh-Threads bauen die Flaechen der gewaehlten Knoten (s. mesh).
// Der Hauptthread fragt jeden Frame update(): er waehlt die Knoten, vergibt Mesh-Auftraege und
// bekommt die fertigen Flaechen zum Hochladen. Die GPU-Seite steht in render::lod_renderer.

pub mod lod_world;
pub mod mesh;
pub mod node;
pub mod select;
pub mod states;
pub mod store;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use world::biome_palette::BiomePalette;
use world::mesher::ColumnNeighbors;

pub use lod_world::LodWorld;
pub use mesh::{NodeMesh, Quad};
pub use node::{NodeKey, MAX_LEVEL};
pub use select::View;
pub use states::LodStates;

// Ordner der gespeicherten Welten: <Datenordner>/chunk-saver/<Server>/<Dimension>_<Seed>.
// Der Seed (gehasht, aus Login/Respawn) trennt Welten, die hinter einem Proxy dieselbe
// Dimension melden (Lobby und Spielserver heissen beide minecraft:overworld).
pub fn world_dir(base: &Path, server: &str, dimension: &str, seed: i64) -> PathBuf {
    server_dir(base, server).join(format!("{}_{:016x}", sauber(dimension), seed as u64))
}

// Ordner eines Servers mit seinen gespeicherten Welten.
pub fn server_dir(base: &Path, server: &str) -> PathBuf {
    base.join("chunk-saver").join(sauber(server))
}

fn sauber(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c.to_ascii_lowercase() } else { '_' }).collect()
}

enum IngestJob {
    Column { world: Arc<LodWorld>, gen: u64, cx: i32, cz: i32, neighbors: Box<ColumnNeighbors> },
    Flush(Arc<LodWorld>),
    Stop,
}

struct MeshJob {
    world: Arc<LodWorld>,
    gen: u64,
    key: NodeKey,
    palette: Arc<BiomePalette>,
}

struct MeshResult {
    gen: u64,
    key: NodeKey,
    mesh: NodeMesh,
}

#[derive(Default)]
struct JobQueue {
    jobs: Mutex<VecDeque<MeshJob>>,
    wake: Condvar,
}

struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Default, Clone, Copy)]
struct NodeState {
    // Liegt ein Mesh auf der GPU?
    has_mesh: bool,
    // Fertig gemesht, aber nichts zu zeichnen.
    empty: bool,
    // Die Daten haben sich geaendert, ein neues Mesh steht aus (das alte wird so lange gezeichnet).
    stale: bool,
    in_flight: bool,
    // Zuletzt gewollt (gewaehlt oder als Ersatz gezeichnet).
    wanted: Option<Instant>,
}

// Was die App nach einem update() an die GPU gibt.
#[derive(Default)]
pub struct FrameOut {
    pub upload: Vec<(NodeKey, NodeMesh)>,
    pub remove: Vec<NodeKey>,
    // Die Knoten, die in diesem Frame zu zeichnen sind (alle mit Mesh auf der GPU).
    pub draw: Vec<NodeKey>,
}

// So lange bleibt ein nicht mehr gewolltes Mesh noch liegen (zurueckdrehen soll nicht gleich
// alles neu bauen).
const KEEP_UNWANTED: Duration = Duration::from_secs(20);

pub struct ChunkSaver {
    states: Arc<LodStates>,
    world: Option<Arc<LodWorld>>,
    gen: u64,
    palette: Arc<BiomePalette>,
    ingest_tx: Sender<IngestJob>,
    ingest_thread: Option<std::thread::JoinHandle<()>>,
    ingest_queued: Arc<AtomicUsize>,
    // Beim Beenden gesetzt: der Aufnahme-Thread nimmt danach nur noch kurz Chunks auf.
    ingest_stopping: Arc<AtomicBool>,
    changed_rx: Receiver<(u64, Vec<NodeKey>)>,
    queue: Arc<JobQueue>,
    workers: Vec<Worker>,
    results_tx: Sender<MeshResult>,
    results_rx: Receiver<MeshResult>,
    nodes: HashMap<NodeKey, NodeState>,
    // Die gewaehlten Knoten (s. select), nahe zuerst, und dieselben als Menge.
    wanted: Vec<NodeKey>,
    wanted_set: HashSet<NodeKey>,
    // Gewaehlte Knoten, deren Mesh noch fehlt oder veraltet ist, nahe zuerst. Je Frame werden
    // nur hiervon Auftraege vergeben -- die ganze Auswahl (bei feiner Schwelle zehntausende
    // Knoten, die meisten leer) wird nur beim Neuwaehlen durchgegangen.
    todo: VecDeque<NodeKey>,
    in_flight: usize,
    selected_at: Option<([f64; 3], f64, f64, (i64, i64))>,
    reselect: bool,
    // Die Zeichenliste, neu gebaut nur, wenn sich Auswahl oder Meshes geaendert haben.
    draw: Vec<NodeKey>,
    draw_dirty: bool,
    draw_built: Option<Instant>,
    pending_remove: Vec<NodeKey>,
}

impl ChunkSaver {
    pub fn new(states: Arc<LodStates>, threads: usize) -> Self {
        let (ingest_tx, ingest_rx) = mpsc::channel();
        let (changed_tx, changed_rx) = mpsc::channel();
        let (results_tx, results_rx) = mpsc::channel();
        let ingest_queued = Arc::new(AtomicUsize::new(0));
        let ingest_stopping = Arc::new(AtomicBool::new(false));
        let ingest_thread = {
            let states = states.clone();
            let queued = ingest_queued.clone();
            let stopping = ingest_stopping.clone();
            std::thread::Builder::new()
                .name("chunk-saver ingest".into())
                .spawn(move || ingest_loop(ingest_rx, changed_tx, states, queued, stopping))
                .ok()
        };
        let mut saver = Self {
            states,
            world: None,
            gen: 0,
            palette: Arc::new(BiomePalette::fallback()),
            ingest_tx,
            ingest_thread,
            ingest_queued,
            ingest_stopping,
            changed_rx,
            queue: Arc::new(JobQueue::default()),
            workers: Vec::new(),
            results_tx,
            results_rx,
            nodes: HashMap::default(),
            wanted: Vec::new(),
            wanted_set: HashSet::default(),
            todo: VecDeque::new(),
            in_flight: 0,
            selected_at: None,
            reselect: true,
            draw: Vec::new(),
            draw_dirty: true,
            draw_built: None,
            pending_remove: Vec::new(),
        };
        saver.set_threads(threads);
        saver
    }

    pub fn set_threads(&mut self, threads: usize) {
        let threads = threads.clamp(1, 16);
        self.workers.retain(|w| !w.stop.load(Ordering::Relaxed));
        while self.workers.len() > threads {
            let w = self.workers.pop().unwrap();
            w.stop.store(true, Ordering::Relaxed);
            self.queue.wake.notify_all();
        }
        while self.workers.len() < threads {
            let stop = Arc::new(AtomicBool::new(false));
            let (queue, results, states, s) = (self.queue.clone(), self.results_tx.clone(), self.states.clone(), stop.clone());
            let handle = std::thread::Builder::new().name("chunk-saver mesh".into()).spawn(move || mesh_loop(queue, results, states, s)).ok();
            self.workers.push(Worker { stop, handle });
        }
    }

    pub fn threads(&self) -> usize {
        self.workers.len()
    }

    // Auf eine (andere) gespeicherte Welt umschalten. Alles Gezeichnete der alten faellt weg.
    pub fn open_world(&mut self, dir: &Path) {
        if self.world.as_ref().is_some_and(|w| w.store.dir() == dir) {
            return;
        }
        self.close_world();
        match store::Store::open(dir) {
            Ok(store) => self.world = Some(Arc::new(LodWorld::new(store))),
            Err(e) => eprintln!("chunk-saver: {} nicht zu oeffnen: {e}", dir.display()),
        }
    }

    pub fn close_world(&mut self) {
        if let Some(alt) = self.world.take() {
            let _ = self.ingest_tx.send(IngestJob::Flush(alt));
        }
        self.gen += 1;
        self.queue.jobs.lock().unwrap().clear();
        self.forget_meshes();
    }

    pub fn has_world(&self) -> bool {
        self.world.is_some()
    }

    // Alle Meshes verwerfen (Zeichnen ausgeschaltet, Welt gewechselt). Die App entfernt sie
    // beim naechsten update() von der GPU -- oder selbst, wenn sie keins mehr aufruft.
    pub fn forget_meshes(&mut self) {
        for (k, s) in self.nodes.drain() {
            if s.has_mesh {
                self.pending_remove.push(k);
            }
        }
        self.wanted.clear();
        self.wanted_set.clear();
        self.todo.clear();
        self.in_flight = 0;
        self.draw.clear();
        self.reselect = true;
    }

    // Neue Biomfarben (Verbindung gewechselt): alle Meshes neu bauen.
    pub fn set_palette(&mut self, palette: Arc<BiomePalette>) {
        if Arc::ptr_eq(&self.palette, &palette) {
            return;
        }
        self.palette = palette;
        for s in self.nodes.values_mut() {
            s.stale = true;
        }
        self.reselect = true;
    }

    // Eine Chunk-Spalte speichern (laeuft im Aufnahme-Thread). Die Nachbarn braucht das Licht.
    pub fn ingest(&self, cx: i32, cz: i32, neighbors: ColumnNeighbors) {
        let Some(world) = self.world.clone() else { return };
        self.ingest_queued.fetch_add(1, Ordering::Relaxed);
        let _ = self.ingest_tx.send(IngestJob::Column { world, gen: self.gen, cx, cz, neighbors: Box::new(neighbors) });
    }

    // Wie viele Chunks noch auf die Aufnahme warten, wie viele Knoten-Meshes auf der GPU liegen,
    // wie viele Mesh-Auftraege laufen.
    pub fn stats(&self) -> (usize, usize, usize) {
        let meshes = self.nodes.values().filter(|s| s.has_mesh).count();
        (self.ingest_queued.load(Ordering::Relaxed), meshes, self.in_flight)
    }

    // Strahl durch die gespeicherte Welt (s. LodWorld::raycast); None ohne offene Welt.
    pub fn raycast(&self, from: [f64; 3], dir: [f64; 3], reach: f64) -> Option<([i64; 3], usize, i64)> {
        self.world.as_ref()?.raycast(from, dir, reach)
    }

    pub fn disk_bytes(&self) -> Option<u64> {
        self.world.as_ref().map(|w| w.store.disk_bytes())
    }

    // Einmal je Frame. `covered`: liegt eine Knotenspalte (Stufe, x, z) ganz in fertig
    // gemeshten echten Chunks? `covered_changed`: hat sich das seit dem letzten Aufruf geaendert?
    pub fn update(&mut self, view: &View, covered: &dyn Fn(u8, i32, i32) -> bool, covered_changed: bool) -> FrameOut {
        let mut out = FrameOut { remove: std::mem::take(&mut self.pending_remove), ..FrameOut::default() };
        let Some(world) = self.world.clone() else { return out };
        let now = Instant::now();

        while let Ok((gen, keys)) = self.changed_rx.try_recv() {
            if gen != self.gen {
                continue;
            }
            // Mit einem Knoten aendern sich auch die Raender seiner Nachbarn (s. mesh::Neighbors).
            for k in keys.iter().flat_map(|k| std::iter::once(*k).chain(k.neighbors())) {
                if let Some(s) = self.nodes.get_mut(&k) {
                    s.stale = true;
                    if self.wanted_set.contains(&k) {
                        self.todo.push_front(k);
                    }
                }
            }
        }
        while let Ok(r) = self.results_rx.try_recv() {
            if r.gen != self.gen {
                continue;
            }
            let Some(s) = self.nodes.get_mut(&r.key) else { continue };
            if s.in_flight {
                s.in_flight = false;
                self.in_flight -= 1;
            }
            let hatte = s.has_mesh;
            if r.mesh.is_empty() {
                s.empty = true;
                s.has_mesh = false;
                if hatte {
                    out.remove.push(r.key);
                }
            } else {
                s.empty = false;
                s.has_mesh = true;
                out.upload.push((r.key, r.mesh));
            }
            if hatte != s.has_mesh || !hatte {
                self.draw_dirty = true;
            }
            // Waehrend des Auftrags veraltet: gleich noch einmal.
            if s.stale && self.wanted_set.contains(&r.key) {
                self.todo.push_front(r.key);
            }
        }

        // Neu waehlen, wenn sich das Auge um ein Viertel des feinsten Knotens bewegt hat
        // (8 Bloecke), die Einstellungen oder die geladenen Chunks sich geaendert haben.
        let moved = self.selected_at.is_none_or(|(eye, r, t, y)| {
            let d = (0..3).map(|i| (eye[i] - view.eye[i]).powi(2)).sum::<f64>().sqrt();
            d > 8.0 || r != view.radius || t != view.threshold_px2 || y != view.y_range
        });
        if moved || covered_changed || self.reselect {
            self.reselect = false;
            self.selected_at = Some((view.eye, view.radius, view.threshold_px2, view.y_range));
            self.wanted.clear();
            for (level, x, z, _) in select::select_columns(view, covered) {
                self.wanted.extend(select::column_nodes(level, x, z, view.y_range));
            }
            self.wanted_set = self.wanted.iter().copied().collect();
            self.todo.clear();
            for &k in &self.wanted {
                let s = self.nodes.entry(k).or_default();
                s.wanted = Some(now);
                if ((!s.has_mesh && !s.empty) || s.stale) && !s.in_flight {
                    self.todo.push_back(k);
                }
            }
            // Lange nicht Gewolltes freigeben.
            let mut weg = Vec::new();
            self.nodes.retain(|k, s| {
                let behalten = s.in_flight || s.wanted.is_some_and(|t| now.duration_since(t) < KEEP_UNWANTED);
                if !behalten && s.has_mesh {
                    weg.push(*k);
                }
                behalten
            });
            out.remove.extend(weg);
            self.draw_dirty = true;
            self.draw_built = None;
        }

        // Auftraege vergeben, nahe zuerst, nie mehr als ein paar je Thread auf einmal.
        let limit = self.workers.len() * 4;
        let mut neu = Vec::new();
        while self.in_flight < limit {
            let Some(k) = self.todo.pop_front() else { break };
            let Some(s) = self.nodes.get_mut(&k) else { continue };
            let fertig = (s.has_mesh || s.empty) && !s.stale;
            if fertig || s.in_flight {
                continue;
            }
            s.in_flight = true;
            s.stale = false;
            self.in_flight += 1;
            neu.push(MeshJob { world: world.clone(), gen: self.gen, key: k, palette: self.palette.clone() });
        }
        if !neu.is_empty() {
            self.queue.jobs.lock().unwrap().extend(neu);
            self.queue.wake.notify_all();
        }

        // Waehrend Meshes eintreffen hoechstens zehnmal je Sekunde -- die Auswahl kann zehntausende
        // Knoten haben.
        if self.draw_dirty && self.draw_built.is_none_or(|t| now.duration_since(t) > Duration::from_millis(100)) {
            self.draw_dirty = false;
            self.draw_built = Some(now);
            self.rebuild_draw(now);
        }
        out.draw = self.draw.clone();
        out
    }

    // Zeichnen: was fertig ist. Fehlt ein Mesh noch, springt der naechste Vorfahr mit Mesh ein
    // (er deckt dann seine ganze Flaeche, seine schon fertigen Nachfahren ruhen so lange),
    // sonst die Kinder, die noch eins haben.
    fn rebuild_draw(&mut self, now: Instant) {
        let mut ersatz: HashSet<NodeKey> = HashSet::default();
        let mut kinder: Vec<NodeKey> = Vec::new();
        for &k in &self.wanted {
            let s = self.nodes.get(&k).copied().unwrap_or_default();
            if s.has_mesh || s.empty {
                continue;
            }
            let mut p = k;
            let mut gefunden = false;
            while p.level < MAX_LEVEL {
                p = p.parent();
                if self.nodes.get(&p).is_some_and(|s| s.has_mesh) {
                    ersatz.insert(p);
                    gefunden = true;
                    break;
                }
            }
            if !gefunden && k.level > 0 {
                kinder.extend(k.children().into_iter().filter(|c| self.nodes.get(c).is_some_and(|s| s.has_mesh)));
            }
        }
        let unter_ersatz = |k: NodeKey| {
            let mut p = k;
            while p.level < MAX_LEVEL {
                p = p.parent();
                if ersatz.contains(&p) {
                    return true;
                }
            }
            false
        };
        self.draw.clear();
        if ersatz.is_empty() {
            self.draw.extend(self.wanted.iter().copied().filter(|k| self.nodes.get(k).is_some_and(|s| s.has_mesh)));
        } else {
            self.draw.extend(self.wanted.iter().copied().filter(|&k| self.nodes.get(&k).is_some_and(|s| s.has_mesh) && !unter_ersatz(k)));
        }
        self.draw.extend(ersatz.iter().copied());
        self.draw.extend(kinder);
        for k in &self.draw {
            if let Some(s) = self.nodes.get_mut(k) {
                s.wanted = Some(now);
            }
        }
    }
}

impl Drop for ChunkSaver {
    fn drop(&mut self) {
        self.ingest_stopping.store(true, Ordering::Relaxed);
        if let Some(w) = self.world.take() {
            let _ = self.ingest_tx.send(IngestJob::Flush(w));
        }
        let _ = self.ingest_tx.send(IngestJob::Stop);
        if let Some(t) = self.ingest_thread.take() {
            let _ = t.join();
        }
        for w in &self.workers {
            w.stop.store(true, Ordering::Relaxed);
        }
        self.queue.wake.notify_all();
        for w in &mut self.workers {
            if let Some(h) = w.handle.take() {
                let _ = h.join();
            }
        }
    }
}

// Wie lange beim Beenden noch wartende Chunks aufgenommen werden. Danach wird nur noch
// geschrieben, was im Speicher steht -- sonst wartete das Schliessen des Fensters auf die ganze
// Warteschlange (bis zu 5 s). Was fehlt, kommt beim naechsten Besuch wieder.
const STOP_GRACE: Duration = Duration::from_millis(500);

fn ingest_loop(rx: Receiver<IngestJob>, changed: Sender<(u64, Vec<NodeKey>)>, states: Arc<LodStates>, queued: Arc<AtomicUsize>, stopping: Arc<AtomicBool>) {
    let mut stop_seit: Option<Instant> = None;
    // Geschrieben wird gesammelt: nach zwei ruhigen Sekunden oder spaetestens alle 30 s.
    let mut offen: Option<Arc<LodWorld>> = None;
    let mut seit = Instant::now();
    loop {
        let job = match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(job) => Some(job),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match job {
            Some(IngestJob::Column { world, gen, cx, cz, neighbors }) => {
                queued.fetch_sub(1, Ordering::Relaxed);
                if stopping.load(Ordering::Relaxed) && stop_seit.get_or_insert_with(Instant::now).elapsed() > STOP_GRACE {
                    continue;
                }
                let keys = world.ingest_column(cx, cz, &neighbors, &states);
                world.trim();
                if !keys.is_empty() {
                    let _ = changed.send((gen, keys));
                }
                if offen.as_ref().is_some_and(|o| !Arc::ptr_eq(o, &world)) {
                    offen.take().unwrap().flush();
                }
                if offen.is_none() {
                    seit = Instant::now();
                }
                offen = Some(world);
                if seit.elapsed() > Duration::from_secs(30) {
                    offen.take().unwrap().flush();
                }
            }
            Some(IngestJob::Flush(world)) => {
                world.flush();
                if offen.as_ref().is_some_and(|o| Arc::ptr_eq(o, &world)) {
                    offen = None;
                }
            }
            Some(IngestJob::Stop) => break,
            None => {
                if let Some(w) = offen.take() {
                    w.flush();
                }
            }
        }
    }
    if let Some(w) = offen {
        w.flush();
    }
}

// Die sechs Nachbarknoten derselben Stufe fuers Meshen der Raender (s. mesh::Neighbors).
fn neighbor_nodes(world: &LodWorld, key: NodeKey) -> mesh::Neighbors {
    key.neighbors().map(|k| world.peek(k))
}

fn mesh_loop(queue: Arc<JobQueue>, results: Sender<MeshResult>, states: Arc<LodStates>, stop: Arc<AtomicBool>) {
    loop {
        let job = {
            let mut jobs = queue.jobs.lock().unwrap();
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(j) = jobs.pop_front() {
                    break j;
                }
                jobs = queue.wake.wait(jobs).unwrap();
            }
        };
        let mesh = match job.world.peek(job.key) {
            Some(node) => mesh::mesh_node(&node, &neighbor_nodes(&job.world, job.key), &states, &job.palette),
            None => NodeMesh::default(),
        };
        if results.send(MeshResult { gen: job.gen, key: job.key, mesh }).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::{CAT_AIR, CAT_OPAQUE, CAT_PLANT, CAT_TRANSLUCENT};
    use world::chunk::{ChunkColumn, ChunkSection};

    // Huegel aus Stein, Erde und Gras, Wasser bis y = 62, ab und zu ein Halm.
    fn gelaende(cx: i32, cz: i32) -> ChunkColumn {
        let hoehe = |x: i32, z: i32| (64.0 + 12.0 * ((x as f64) * 0.05).sin() * ((z as f64) * 0.04).cos() + 6.0 * ((x + z) as f64 * 0.13).sin()) as i32;
        let sections = (0..24)
            .map(|i| {
                let mut s = vec![0u32; 4096];
                for y in 0..16 {
                    for z in 0..16 {
                        for x in 0..16 {
                            let (wx, wy, wz) = (cx * 16 + x, -64 + i * 16 + y, cz * 16 + z);
                            let h = hoehe(wx, wz);
                            s[((y * 16 + z) * 16 + x) as usize] = if wy < h - 4 {
                                1
                            } else if wy < h {
                                4
                            } else if wy == h {
                                5
                            } else if wy <= 62 {
                                2
                            } else if wy == h + 1 && (wx ^ wz) % 7 == 0 {
                                3
                            } else {
                                0
                            };
                        }
                    }
                }
                ChunkSection::from_states(s)
            })
            .collect();
        ChunkColumn { sections, min_y: -64, sky: Vec::new() }
    }

    // Frueher schloss das Fenster erst nach 5 s, weil der Drop auf die ganze Warteschlange
    // wartete. Jetzt hoechstens STOP_GRACE plus Schreiben.
    #[test]
    fn beenden_wartet_nicht_auf_die_ganze_warteschlange() {
        let dir = std::env::temp_dir().join(format!("chunk_saver_beenden_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let states = Arc::new(LodStates::from_categories(vec![CAT_AIR, CAT_OPAQUE, CAT_TRANSLUCENT, CAT_PLANT, CAT_OPAQUE, CAT_OPAQUE]));
        let mut saver = ChunkSaver::new(states, 1);
        saver.open_world(&dir);
        let spalte = Arc::new(gelaende(0, 0));
        for i in 0..3000 {
            let n = ColumnNeighbors { target: spalte.clone(), north: None, south: None, east: None, west: None, northeast: None, northwest: None, southeast: None, southwest: None };
            saver.ingest(i % 60, i / 60, n);
        }
        let t = Instant::now();
        drop(saver);
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Diagnose: was steht in einer gespeicherten Welt hoch oben?
    // CHUNK_SAVER_WELT=<Ordner> cargo test --release -p chunk_saver hoch_oben -- --ignored --nocapture
    #[test]
    #[ignore]
    fn hoch_oben() {
        let Ok(dir) = std::env::var("CHUNK_SAVER_WELT") else { return };
        let w = LodWorld::new(store::Store::open(std::path::Path::new(&dir)).unwrap());
        let ab: i64 = std::env::var("AB_Y").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
        let keys = w.store.keys(0);
        println!("{} Knoten auf Stufe 0", keys.len());
        let mut treffer: std::collections::BTreeMap<(i32, i32, i32), (usize, std::collections::BTreeMap<String, usize>)> = Default::default();
        for k in keys {
            let o = k.origin();
            if o[1] + 32 <= ab {
                continue;
            }
            let Some(n) = w.peek(k) else { continue };
            for y in 0..32 {
                if o[1] + (y as i64) < ab {
                    continue;
                }
                for z in 0..32 {
                    for x in 0..32 {
                        let v = n.get(x, y, z);
                        if node::is_air(v) {
                            continue;
                        }
                        let e = treffer.entry((k.x, k.y, k.z)).or_default();
                        e.0 += 1;
                        let name = world::block_registry::block_name(node::state_of(v)).unwrap_or("?").to_string();
                        *e.1.entry(name).or_default() += 1;
                    }
                }
            }
        }
        for ((x, y, z), (anzahl, namen)) in treffer {
            println!("Knoten {x} {y} {z} (Bloecke ab {} {} {}): {anzahl} -- {namen:?}", x * 32, y * 32, z * 32);
        }
    }

    // Was Aufnahme und Meshing kosten (cargo test --release -p chunk_saver messung -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn messung_aufnahme_und_meshing() {
        let dir = std::env::temp_dir().join(format!("chunk_saver_messung_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let states = LodStates::from_categories(vec![CAT_AIR, CAT_OPAQUE, CAT_TRANSLUCENT, CAT_PLANT, CAT_OPAQUE, CAT_OPAQUE]);
        let w = LodWorld::new(store::Store::open(&dir).unwrap());
        let cols: Vec<(i32, i32, ChunkColumn)> = (-16..16).flat_map(|x| (-16..16).map(move |z| (x, z, gelaende(x, z)))).collect();
        let t = Instant::now();
        for (x, z, c) in &cols {
            let n = ColumnNeighbors { target: Arc::new(c.clone()), north: None, south: None, east: None, west: None, northeast: None, northwest: None, southeast: None, southwest: None };
            w.ingest_column(*x, *z, &n, &states);
            w.trim();
        }
        let aufnahme = t.elapsed();
        let t = Instant::now();
        w.flush();
        let schreiben = t.elapsed();
        println!(
            "Aufnahme: {} Chunks in {:.0} ms ({:.2} ms/Chunk), Schreiben {:.0} ms, Platte {} KB",
            cols.len(),
            aufnahme.as_secs_f64() * 1e3,
            aufnahme.as_secs_f64() * 1e3 / cols.len() as f64,
            schreiben.as_secs_f64() * 1e3,
            w.store.disk_bytes() / 1024
        );
        let palette = BiomePalette::fallback();
        for level in 0..=4u8 {
            let n = 16 >> level.min(4);
            // Einmal ohne Nachbarn, einmal mit (samt deren Laden) -- was die Raender kosten.
            for mit_nachbarn in [false, true] {
                let t = Instant::now();
                let (mut knoten, mut flaechen) = (0, 0);
                for x in -n.max(1)..n.max(1) {
                    for z in -n.max(1)..n.max(1) {
                        for k in select::column_nodes(level, x, z, (-64, 320)) {
                            if let Some(node) = w.peek(k) {
                                let nachbarn = if mit_nachbarn { neighbor_nodes(&w, k) } else { Default::default() };
                                let m = mesh::mesh_node(&node, &nachbarn, &states, &palette);
                                knoten += 1;
                                flaechen += m.opaque.len() + m.translucent.len();
                            }
                        }
                    }
                }
                let ms = t.elapsed().as_secs_f64() * 1e3;
                println!(
                    "Stufe {level} {}: {knoten} Knoten in {ms:.0} ms ({:.2} ms/Knoten), {:.0} Flaechen/Knoten",
                    if mit_nachbarn { "mit Nachbarn " } else { "ohne Nachbarn" },
                    ms / knoten.max(1) as f64,
                    flaechen as f64 / knoten.max(1) as f64
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
