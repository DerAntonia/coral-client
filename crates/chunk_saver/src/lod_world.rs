// Eine gespeicherte Welt: die Knoten auf der Platte (store) und davor ein Zwischenspeicher der
// zuletzt benutzten. Geaendert wird nur vom Aufnahme-Thread (s. ingest_column), gelesen
// zusaetzlich von den Mesh-Threads. Geaenderte Knoten bleiben im Speicher und werden gesammelt
// geschrieben (flush) -- ein Knoten der Stufe 0 gehoert zu vier Chunks, und beim Betreten
// einer Welt kommen die meist kurz hintereinander.

use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap as HashMap;
use world::chunk::ChunkColumn;
use world::mesher::ColumnNeighbors;

use crate::node::{is_air, voxel, with_light, Node, NodeKey, MAX_LEVEL, N};
use crate::states::LodStates;
use crate::store::Store;

// Wie viele Knoten im Speicher bleiben (je 256 KB).
const CACHE_NODES: usize = 384;

struct Entry {
    // None = bekannt leer (nur Luft, oder nie gesehen).
    node: Option<Arc<Node>>,
    dirty: bool,
    used: u64,
}

#[derive(Default)]
struct Cache {
    nodes: HashMap<NodeKey, Entry>,
    clock: u64,
}

pub struct LodWorld {
    pub store: Store,
    cache: Mutex<Cache>,
}

impl LodWorld {
    pub fn new(store: Store) -> Self {
        Self { store, cache: Mutex::new(Cache::default()) }
    }

    pub fn get(&self, key: NodeKey) -> Option<Arc<Node>> {
        {
            let mut cache = self.cache.lock().unwrap();
            cache.clock += 1;
            let clock = cache.clock;
            if let Some(e) = cache.nodes.get_mut(&key) {
                e.used = clock;
                return e.node.clone();
            }
        }
        let node = self.store.read(key).and_then(|b| Node::decode(&b)).map(Arc::new);
        let mut cache = self.cache.lock().unwrap();
        let clock = cache.clock;
        // Waehrenddessen kann der Aufnahme-Thread ihn geaendert haben -- dann gilt seiner.
        cache.nodes.entry(key).or_insert(Entry { node: node.clone(), dirty: false, used: clock }).node.clone()
    }

    fn put(&self, key: NodeKey, node: Option<Arc<Node>>) {
        let mut cache = self.cache.lock().unwrap();
        cache.clock += 1;
        let used = cache.clock;
        cache.nodes.insert(key, Entry { node, dirty: true, used });
    }

    // Alles Geaenderte auf die Platte. Gepackt wird ausserhalb der Sperre.
    pub fn flush(&self) {
        let dirty: Vec<(NodeKey, Option<Arc<Node>>)> = {
            let mut cache = self.cache.lock().unwrap();
            cache
                .nodes
                .iter_mut()
                .filter(|(_, e)| e.dirty)
                .map(|(k, e)| {
                    e.dirty = false;
                    (*k, e.node.clone())
                })
                .collect()
        };
        for (key, node) in dirty {
            let bytes = node.map(|n| n.encode()).unwrap_or_default();
            let _ = self.store.write(key, &bytes);
        }
    }

    // Den Zwischenspeicher auf seine Groesse bringen: die am laengsten unbenutzten Knoten
    // fallen heraus, geaenderte werden dabei geschrieben. Die oberen Stufen aendern sich mit
    // fast jedem Chunk und bleiben so im Speicher, statt jedes Mal auf die Platte zu gehen.
    // Nur vom Aufnahme-Thread aufzurufen -- er ist der einzige, der Knoten aendert.
    pub fn trim(&self) {
        let raus: Vec<(NodeKey, Option<Arc<Node>>, bool)> = {
            let mut cache = self.cache.lock().unwrap();
            let over = cache.nodes.len().saturating_sub(CACHE_NODES);
            if over == 0 {
                return;
            }
            let mut by_age: Vec<(u64, NodeKey)> = cache.nodes.iter().map(|(k, e)| (e.used, *k)).collect();
            by_age.sort_unstable();
            by_age
                .into_iter()
                .take(over + CACHE_NODES / 8)
                .filter_map(|(_, k)| cache.nodes.remove(&k).map(|e| (k, e.node, e.dirty)))
                .collect()
        };
        for (key, node, dirty) in raus {
            if dirty {
                let bytes = node.map(|n| n.encode()).unwrap_or_default();
                let _ = self.store.write(key, &bytes);
            }
        }
    }

    // Strahl durch die gespeicherten Bloecke (Stufe 0), Block fuer Block: der erste nicht
    // leere, dazu die Achse, auf der der Strahl in ihn eintritt, und die Schrittrichtung darauf.
    // Fuer den Zuschauer-Teleport in die Fernsicht. Startet der Strahl in festem Gestein, zaehlt
    // erst ein Block hinter der ersten Luft. Knoten werden mit peek gelesen (der Zwischenspeicher
    // gehoert dem Aufnahme-Thread, s. trim).
    pub fn raycast(&self, from: [f64; 3], dir: [f64; 3], reach: f64) -> Option<([i64; 3], usize, i64)> {
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if len < 1e-9 {
            return None;
        }
        let d = [dir[0] / len, dir[1] / len, dir[2] / len];
        let mut zelle = [from[0].floor() as i64, from[1].floor() as i64, from[2].floor() as i64];
        let mut schritt = [0i64; 3];
        let mut naechste = [f64::INFINITY; 3];
        let mut abstand = [f64::INFINITY; 3];
        for a in 0..3 {
            if d[a] > 0.0 {
                schritt[a] = 1;
                naechste[a] = ((zelle[a] + 1) as f64 - from[a]) / d[a];
                abstand[a] = 1.0 / d[a];
            } else if d[a] < 0.0 {
                schritt[a] = -1;
                naechste[a] = (zelle[a] as f64 - from[a]) / d[a];
                abstand[a] = -1.0 / d[a];
            }
        }
        let n = N as i64;
        let mut knoten: Option<(NodeKey, Option<Arc<Node>>)> = None;
        let mut voxel = |z: [i64; 3]| -> bool {
            let key = NodeKey::of_block(z[0], z[1], z[2]);
            if knoten.as_ref().is_none_or(|(k, _)| *k != key) {
                knoten = Some((key, self.peek(key)));
            }
            let Some((_, Some(node))) = knoten.as_ref() else { return false };
            !is_air(node.get(z[0].rem_euclid(n) as usize, z[1].rem_euclid(n) as usize, z[2].rem_euclid(n) as usize))
        };
        let mut im_fels = voxel(zelle);
        let mut achse = 0;
        let mut t = 0.0;
        while t <= reach {
            let fest = voxel(zelle);
            if !fest {
                im_fels = false;
            } else if !im_fels {
                return Some((zelle, achse, schritt[achse]));
            }
            achse = if naechste[0] <= naechste[1] && naechste[0] <= naechste[2] {
                0
            } else if naechste[1] <= naechste[2] {
                1
            } else {
                2
            };
            t = naechste[achse];
            naechste[achse] += abstand[achse];
            zelle[achse] += schritt[achse];
        }
        None
    }

    // Lesen ohne den Zwischenspeicher zu fuellen (fuer die Mesh-Threads, s. trim).
    pub fn peek(&self, key: NodeKey) -> Option<Arc<Node>> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some(e) = cache.nodes.get(&key) {
                return e.node.clone();
            }
        }
        self.store.read(key).and_then(|b| Node::decode(&b)).map(Arc::new)
    }

    // Eine Chunk-Spalte uebernehmen: in die Knoten der Stufe 0 schreiben und die Aenderung
    // durch alle Stufen nach oben tragen. Zurueck kommen alle Knoten, die sich geaendert haben.
    // Die Nachbarn braucht das Licht: Blocklicht wird wie im Shader-Pfad aus den leuchtenden
    // Bloecken der neun Spalten ausgebreitet, Himmelslicht kommt vom Server.
    pub fn ingest_column(&self, cx: i32, cz: i32, neighbors: &ColumnNeighbors, states: &LodStates) -> Vec<NodeKey> {
        let column: &ChunkColumn = &neighbors.target;
        let licht = world::light::light_field(neighbors);
        let (bx, bz) = (cx as i64 * 16, cz as i64 * 16);
        let mut changed: Vec<NodeKey> = Vec::new();
        for (i, section) in column.sections.iter().enumerate() {
            let by = column.min_y as i64 + i as i64 * 16;
            let key = NodeKey::of_block(bx, by, bz);
            let n = N as i64;
            let (ox, oy, oz) = (bx.rem_euclid(n) as usize, by.rem_euclid(n) as usize, bz.rem_euclid(n) as usize);
            let alt = self.get(key);
            let leer = Node::empty();
            let basis = alt.as_deref().unwrap_or(&leer);
            let s = section.states();
            let wert = |x: usize, y: usize, z: usize| {
                let st = s[(y * 16 + z) * 16 + x];
                let (block, sky) = licht.at(x as i32, by as i32 + y as i32, z as i32);
                let v = if st == world::chunk::STATE_AIR { 0 } else { voxel(st, section.biome_at(x as i32, y as i32, z as i32)) };
                with_light(v, world::light::brightest(block), sky)
            };
            let gleich = (0..16).all(|y| (0..16).all(|z| (0..16).all(|x| basis.get(ox + x, oy + y, oz + z) == wert(x, y, z))));
            if gleich {
                continue;
            }
            let mut neu = basis.clone();
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        neu.set(ox + x, oy + y, oz + z, wert(x, y, z));
                    }
                }
            }
            self.put(key, (!neu.is_empty()).then(|| Arc::new(neu)));
            if !changed.contains(&key) {
                changed.push(key);
            }
        }
        let mut level_keys = changed.clone();
        let leer = Node::empty();
        for _ in 0..MAX_LEVEL {
            let mut parents: Vec<NodeKey> = Vec::new();
            for &child in &level_keys {
                let parent = child.parent();
                let kind = self.get(child);
                let alt = self.get(parent);
                let mut neu = alt.as_deref().cloned().unwrap_or_default();
                neu.downsample_octant(kind.as_deref().unwrap_or(&leer), child.octant(), states);
                if alt.as_deref().map_or(neu.is_empty(), |a| a.voxels == neu.voxels) {
                    continue;
                }
                self.put(parent, (!neu.is_empty()).then(|| Arc::new(neu)));
                if !parents.contains(&parent) {
                    parents.push(parent);
                }
            }
            changed.extend_from_slice(&parents);
            level_keys = parents;
            if level_keys.is_empty() {
                break;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::state_of;
    use crate::states::{CAT_AIR, CAT_OPAQUE};
    use world::chunk::ChunkSection;

    fn spalte(fill: impl Fn(i32, i32, i32) -> u32) -> ChunkColumn {
        let sections = (0..4)
            .map(|i| {
                let mut s = vec![0u32; 4096];
                for y in 0..16 {
                    for z in 0..16 {
                        for x in 0..16 {
                            s[((y * 16 + z) * 16 + x) as usize] = fill(x, i * 16 + y, z);
                        }
                    }
                }
                ChunkSection::from_states(s)
            })
            .collect();
        ChunkColumn { sections, min_y: 0, sky: Vec::new() }
    }

    fn nachbarn(c: ChunkColumn) -> ColumnNeighbors {
        ColumnNeighbors { target: Arc::new(c), north: None, south: None, east: None, west: None, northeast: None, northwest: None, southeast: None, southwest: None }
    }

    #[test]
    fn aufnahme_traegt_bis_nach_oben_und_ist_beim_zweiten_mal_still() {
        let dir = std::env::temp_dir().join(format!("chunk_saver_world_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let states = LodStates::from_categories(vec![CAT_AIR, CAT_OPAQUE]);
        let w = LodWorld::new(Store::open(&dir).unwrap());
        // Boden bis y = 9, darueber Luft.
        let col = nachbarn(spalte(|_, y, _| u32::from(y < 10)));
        let changed = w.ingest_column(1, -1, &col, &states);
        assert!(changed.contains(&NodeKey::new(0, 0, 0, -1)));
        assert!(changed.iter().any(|k| k.level == MAX_LEVEL));
        let l0 = w.get(NodeKey::new(0, 0, 0, -1)).unwrap();
        assert_eq!(state_of(l0.get(16, 9, 16)), 1);
        assert!(crate::node::is_air(l0.get(16, 10, 16)));
        assert!(crate::node::is_air(l0.get(0, 0, 16)));
        let l1 = w.get(NodeKey::new(1, 0, 0, -1)).unwrap();
        assert_eq!(state_of(l1.get(8, 4, 24)), 1);
        assert!(crate::node::is_air(l1.get(8, 5, 24)));
        assert!(w.ingest_column(1, -1, &col, &states).is_empty());
        w.flush();
        let w2 = LodWorld::new(Store::open(&dir).unwrap());
        assert_eq!(w2.get(NodeKey::new(0, 0, 0, -1)).unwrap().voxels, l0.voxels);
        // Alles weg -> die Knoten verschwinden auch auf der Platte.
        w2.ingest_column(1, -1, &nachbarn(spalte(|_, _, _| 0)), &states);
        w2.flush();
        assert!(!w2.store.contains(NodeKey::new(0, 0, 0, -1)));
        assert!(!w2.store.contains(NodeKey::new(3, 0, 0, -1)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strahl_trifft_gespeicherte_bloecke() {
        let dir = std::env::temp_dir().join(format!("chunk_saver_strahl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let states = LodStates::from_categories(vec![CAT_AIR, CAT_OPAQUE]);
        let w = LodWorld::new(Store::open(&dir).unwrap());
        // Chunk (1, -1): x 16..31, z -16..-1, Boden bis y = 9.
        w.ingest_column(1, -1, &nachbarn(spalte(|_, y, _| u32::from(y < 10))), &states);
        assert_eq!(w.raycast([20.5, 40.5, -5.5], [0.0, -1.0, 0.0], 100.0), Some(([20, 9, -6], 1, -1)));
        assert_eq!(w.raycast([0.5, 5.5, -5.5], [1.0, 0.0, 0.0], 100.0), Some(([16, 5, -6], 0, 1)));
        // Zu kurz, und im Fels nach unten (darunter nichts gespeichert).
        assert_eq!(w.raycast([0.5, 5.5, -5.5], [1.0, 0.0, 0.0], 10.0), None);
        assert_eq!(w.raycast([20.5, 5.5, -5.5], [0.0, -1.0, 0.0], 100.0), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
