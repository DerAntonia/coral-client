// Die Knoten einer Welt auf der Platte.
//
// Je Stufe und Region (16 x 16 Knotenspalten) eine Datei, die nur angehaengt wird: jeder
// Eintrag ist Kopf + gepackter Knoten, der juengste Eintrag eines Knotens gilt. Ein leerer
// Eintrag (Laenge 0) loescht ihn. Beim Oeffnen wird die Datei einmal durchgelesen, um den
// Index aufzubauen; ein abgerissener letzter Eintrag (Absturz beim Schreiben) faellt dabei
// an seiner Pruefsumme auf und wird abgeschnitten -- alles davor bleibt gueltig. Waechst eine
// Datei auf mehr als das Doppelte ihres lebenden Inhalts, wird sie neu geschrieben.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap as HashMap;

use crate::node::NodeKey;

const MAGIC: u32 = 0x4C4F_4431; // "LOD1"
const HEADER: usize = 4 + 4 + 1 + 1 + 2 + 4 + 4;
const REGION_SHIFT: i32 = 4;
// Hoechstens so viele offene Regionsdateien; die am laengsten unbenutzte wird geschlossen.
const MAX_OPEN: usize = 96;

struct Region {
    file: File,
    path: PathBuf,
    // (lx, lz, y) -> (Versatz der Daten, Laenge)
    index: HashMap<(u8, u8, i32), (u64, u32)>,
    end: u64,
    live: u64,
}

impl Region {
    fn open(path: PathBuf) -> std::io::Result<Region> {
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
        let len = file.metadata()?.len();
        let mut bytes = Vec::with_capacity(len as usize);
        file.read_to_end(&mut bytes)?;
        let mut index = HashMap::default();
        let mut at = 0usize;
        while at + HEADER <= bytes.len() {
            let h = &bytes[at..at + HEADER];
            let magic = u32::from_le_bytes(h[0..4].try_into().unwrap());
            let y = i32::from_le_bytes(h[4..8].try_into().unwrap());
            let (lx, lz) = (h[8], h[9]);
            let size = u32::from_le_bytes(h[12..16].try_into().unwrap()) as usize;
            let crc = u32::from_le_bytes(h[16..20].try_into().unwrap());
            let data = at + HEADER;
            if magic != MAGIC || data + size > bytes.len() || crc32fast::hash(&bytes[data..data + size]) != crc {
                break;
            }
            if size == 0 {
                index.remove(&(lx, lz, y));
            } else {
                index.insert((lx, lz, y), (data as u64, size as u32));
            }
            at = data + size;
        }
        if at as u64 != len {
            file.set_len(at as u64)?;
        }
        let live = index.values().map(|&(_, s)| s as u64 + HEADER as u64).sum();
        Ok(Region { file, path, index, end: at as u64, live })
    }

    fn read(&mut self, key: (u8, u8, i32)) -> Option<Vec<u8>> {
        let &(offset, size) = self.index.get(&key)?;
        let mut buf = vec![0; size as usize];
        self.file.seek(SeekFrom::Start(offset)).ok()?;
        self.file.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    fn write(&mut self, key: (u8, u8, i32), data: &[u8]) -> std::io::Result<()> {
        if data.is_empty() && !self.index.contains_key(&key) {
            return Ok(());
        }
        let mut rec = Vec::with_capacity(HEADER + data.len());
        rec.extend_from_slice(&MAGIC.to_le_bytes());
        rec.extend_from_slice(&key.2.to_le_bytes());
        rec.extend_from_slice(&[key.0, key.1, 0, 0]);
        rec.extend_from_slice(&(data.len() as u32).to_le_bytes());
        rec.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
        rec.extend_from_slice(data);
        self.file.seek(SeekFrom::Start(self.end))?;
        self.file.write_all(&rec)?;
        if let Some((_, alt)) = self.index.remove(&key) {
            self.live -= alt as u64 + HEADER as u64;
        }
        if !data.is_empty() {
            self.index.insert(key, (self.end + HEADER as u64, data.len() as u32));
            self.live += rec.len() as u64;
        }
        self.end += rec.len() as u64;
        if self.end > 1 << 20 && self.end > self.live * 2 {
            self.compact()?;
        }
        Ok(())
    }

    // Nur die lebenden Eintraege in eine neue Datei, dann gegen die alte tauschen.
    fn compact(&mut self) -> std::io::Result<()> {
        let tmp = self.path.with_extension("tmp");
        let mut out = File::create(&tmp)?;
        let mut neu = HashMap::default();
        let mut end = 0u64;
        let keys: Vec<_> = self.index.keys().copied().collect();
        for key in keys {
            let Some(data) = self.read(key) else { continue };
            let mut rec = Vec::with_capacity(HEADER + data.len());
            rec.extend_from_slice(&MAGIC.to_le_bytes());
            rec.extend_from_slice(&key.2.to_le_bytes());
            rec.extend_from_slice(&[key.0, key.1, 0, 0]);
            rec.extend_from_slice(&(data.len() as u32).to_le_bytes());
            rec.extend_from_slice(&crc32fast::hash(&data).to_le_bytes());
            rec.extend_from_slice(&data);
            out.write_all(&rec)?;
            neu.insert(key, (end + HEADER as u64, data.len() as u32));
            end += rec.len() as u64;
        }
        out.sync_all()?;
        drop(out);
        std::fs::rename(&tmp, &self.path)?;
        self.file = OpenOptions::new().read(true).write(true).open(&self.path)?;
        self.index = neu;
        self.end = end;
        self.live = end;
        Ok(())
    }
}

type RegionKey = (u8, i32, i32);

pub struct Store {
    dir: PathBuf,
    open: Mutex<OpenRegions>,
}

#[derive(Default)]
struct OpenRegions {
    regions: HashMap<RegionKey, (Arc<Mutex<Region>>, u64)>,
    // Regionen ohne Datei -- Lesen legt keine an, und die Fernsicht fragt viele leere ab.
    missing: rustc_hash::FxHashSet<RegionKey>,
    clock: u64,
}

fn split(key: NodeKey) -> (RegionKey, (u8, u8, i32)) {
    let region = (key.level, key.x >> REGION_SHIFT, key.z >> REGION_SHIFT);
    let mask = (1 << REGION_SHIFT) - 1;
    (region, ((key.x & mask) as u8, (key.z & mask) as u8, key.y))
}

impl Store {
    pub fn open(dir: &Path) -> std::io::Result<Store> {
        std::fs::create_dir_all(dir)?;
        Ok(Store { dir: dir.to_path_buf(), open: Mutex::new(OpenRegions::default()) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn region(&self, key: RegionKey, create: bool) -> Option<Arc<Mutex<Region>>> {
        let mut open = self.open.lock().unwrap();
        open.clock += 1;
        let clock = open.clock;
        if let Some((r, used)) = open.regions.get_mut(&key) {
            *used = clock;
            return Some(r.clone());
        }
        if !create && open.missing.contains(&key) {
            return None;
        }
        let level_dir = self.dir.join(format!("L{}", key.0));
        let path = level_dir.join(format!("{}_{}.lod", key.1, key.2));
        if !create && !path.exists() {
            open.missing.insert(key);
            return None;
        }
        std::fs::create_dir_all(&level_dir).ok()?;
        open.missing.remove(&key);
        let region = Region::open(path).ok()?;
        if open.regions.len() >= MAX_OPEN {
            if let Some(alt) = open.regions.iter().min_by_key(|(_, (_, used))| *used).map(|(k, _)| *k) {
                open.regions.remove(&alt);
            }
        }
        let r = Arc::new(Mutex::new(region));
        open.regions.insert(key, (r.clone(), clock));
        Some(r)
    }

    pub fn read(&self, key: NodeKey) -> Option<Vec<u8>> {
        let (region, local) = split(key);
        let r = self.region(region, false)?;
        let mut r = r.lock().unwrap();
        r.read(local)
    }

    // Leere Daten loeschen den Knoten.
    pub fn write(&self, key: NodeKey, data: &[u8]) -> std::io::Result<()> {
        let (region, local) = split(key);
        if data.is_empty() && self.region(region, false).is_none() {
            return Ok(());
        }
        let r = self.region(region, true).ok_or_else(|| std::io::Error::other("Region nicht zu oeffnen"))?;
        let mut r = r.lock().unwrap();
        r.write(local, data)
    }

    // Gibt es den Knoten auf der Platte? Nur aus dem Index, ohne ihn zu lesen.
    pub fn contains(&self, key: NodeKey) -> bool {
        let (region, local) = split(key);
        self.region(region, false).is_some_and(|r| r.lock().unwrap().index.contains_key(&local))
    }

    // Alle gespeicherten Knoten einer Stufe (zur Diagnose; liest jede Regionsdatei einmal).
    pub fn keys(&self, level: u8) -> Vec<NodeKey> {
        let Ok(entries) = std::fs::read_dir(self.dir.join(format!("L{level}"))) else { return Vec::new() };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some((rx, rz)) = name.strip_suffix(".lod").and_then(|n| n.split_once('_')) else { continue };
            let (Ok(rx), Ok(rz)) = (rx.parse::<i32>(), rz.parse::<i32>()) else { continue };
            let Some(r) = self.region((level, rx, rz), false) else { continue };
            let r = r.lock().unwrap();
            out.extend(r.index.keys().map(|&(lx, lz, y)| NodeKey::new(level, (rx << REGION_SHIFT) | lx as i32, y, (rz << REGION_SHIFT) | lz as i32)));
        }
        out
    }

    // Belegter Platz aller Regionsdateien in Bytes (fuer die Anzeige im Modul).
    pub fn disk_bytes(&self) -> u64 {
        fn walk(dir: &Path) -> u64 {
            let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
            entries
                .flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => walk(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        }
        walk(&self.dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chunk_saver_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn schreiben_lesen_loeschen_und_wieder_oeffnen() {
        let dir = tmp("store");
        let a = NodeKey::new(0, -1, 2, 17);
        let b = NodeKey::new(3, 5, -1, -40);
        {
            let s = Store::open(&dir).unwrap();
            s.write(a, b"eins").unwrap();
            s.write(b, b"zwei").unwrap();
            s.write(a, b"drei").unwrap();
            assert_eq!(s.read(a).unwrap(), b"drei");
            s.write(b, b"").unwrap();
            assert!(s.read(b).is_none());
        }
        let s = Store::open(&dir).unwrap();
        assert_eq!(s.read(a).unwrap(), b"drei");
        assert!(!s.contains(b));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn abgerissener_eintrag_wird_abgeschnitten() {
        let dir = tmp("torn");
        let a = NodeKey::new(0, 0, 0, 0);
        {
            let s = Store::open(&dir).unwrap();
            s.write(a, b"gut").unwrap();
            s.write(a, b"auch gut, aber gleich kaputt").unwrap();
        }
        let datei = dir.join("L0").join("0_0.lod");
        let len = std::fs::metadata(&datei).unwrap().len();
        OpenOptions::new().write(true).open(&datei).unwrap().set_len(len - 3).unwrap();
        let s = Store::open(&dir).unwrap();
        assert_eq!(s.read(a).unwrap(), b"gut");
        s.write(a, b"neu").unwrap();
        drop(s);
        let s = Store::open(&dir).unwrap();
        assert_eq!(s.read(a).unwrap(), b"neu");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verdichten_behaelt_den_juengsten_stand() {
        let dir = tmp("compact");
        let s = Store::open(&dir).unwrap();
        let gross = vec![7u8; 64 * 1024];
        for i in 0..40 {
            let mut d = gross.clone();
            d[0] = i;
            s.write(NodeKey::new(0, 1, 0, 1), &d).unwrap();
        }
        s.write(NodeKey::new(0, 2, 0, 1), b"x").unwrap();
        let r = s.read(NodeKey::new(0, 1, 0, 1)).unwrap();
        assert_eq!(r[0], 39);
        assert!(std::fs::metadata(dir.join("L0").join("0_0.lod")).unwrap().len() < 3 << 20);
        drop(s);
        let s = Store::open(&dir).unwrap();
        assert_eq!(s.read(NodeKey::new(0, 1, 0, 1)).unwrap()[0], 39);
        assert_eq!(s.read(NodeKey::new(0, 2, 0, 1)).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
