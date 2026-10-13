// Hoehlen-Culling: welche Sections (16er-Wuerfel) von der Kamera aus ueberhaupt sichtbar sein
// koennen.
//
// Jede Spalte meldet je Section, zwischen welchen Seiten man durch sie hindurchsieht (s.
// world::visibility). Eine Breitensuche von der Section der Kamera aus geht nur dort weiter,
// wo die Section eine ihrer Eingangsseiten mit der Ausgangsseite verbindet, und nur von der
// Kamera weg (wie Sodium). Eingangsseiten sind ALLE, durch die sie erreicht wurde: frueher
// zaehlte nur der erste Weg, und kam der durch die falsche Seite, fehlte alles dahinter --
// je nach Kamera-Section mal ja, mal nein (ein Streifen Wand verschwand). Was sie nicht erreicht, liegt
// hinter Fels -- Hoehlen unter der Oberflaeche, aber auch die Oberflaeche, wenn man selbst in
// einer Hoehle steht.
//
// Die Suche laeuft auf einem eigenen Thread und ignoriert die Blickrichtung (die prueft der
// Renderer jeden Frame selbst). Neu gerechnet wird nur, wenn die Kamera die Section wechselt
// oder sich die Sichtdaten einer Spalte aendern; das Ergebnis kommt ein, zwei Frames spaeter.
// Bis dahin gilt das alte -- und Spalten, deren Daten sich seitdem geaendert haben, werden
// samt ihren Nachbarn vorsichtshalber ganz gezeichnet.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use world::visibility::{sees_through, SectionVis, VIS_ALL};

type Key = (i32, i32);

enum Msg {
    Column(Key, Option<Arc<[SectionVis]>>),
    Clear,
    Compute { camera: (i32, i32, i32), radius: i32, generation: u64 },
}

struct Result {
    generation: u64,
    // Je Spalte: Bit n = Section n ist erreichbar. Spalten ohne Eintrag gelten als sichtbar.
    masks: HashMap<Key, u64>,
}

pub struct OcclusionCulling {
    tx: Sender<Msg>,
    rx: Receiver<Result>,
    result: Option<Result>,
    // Zaehlt jede Datenaenderung; eine Rechnung kennt alle Aenderungen bis zu ihrer Nummer.
    generation: u64,
    // Spalten mit Daten, die die letzte fertige Rechnung noch nicht kannte (und ab wann).
    fresh: HashMap<Key, u64>,
    sent: Option<((i32, i32, i32), i32, u64)>,
    in_flight: bool,
    min_y: i32,
}

impl OcclusionCulling {
    pub fn new() -> Self {
        let (tx, worker_rx) = channel::<Msg>();
        let (worker_tx, rx) = channel::<Result>();
        std::thread::Builder::new()
            .name("hoehlen-culling".into())
            .spawn(move || worker(worker_rx, worker_tx))
            .expect("Thread fuer das Hoehlen-Culling");
        Self { tx, rx, result: None, generation: 0, fresh: HashMap::new(), sent: None, in_flight: false, min_y: world::MIN_Y }
    }

    pub fn set_column(&mut self, key: Key, vis: Vec<SectionVis>, min_y: i32) {
        self.min_y = min_y;
        self.generation += 1;
        self.fresh.insert(key, self.generation);
        let _ = self.tx.send(Msg::Column(key, Some(vis.into())));
    }

    pub fn remove_column(&mut self, key: Key) {
        self.generation += 1;
        self.fresh.remove(&key);
        let _ = self.tx.send(Msg::Column(key, None));
    }

    pub fn clear(&mut self) {
        self.generation += 1;
        self.fresh.clear();
        self.result = None;
        self.sent = None;
        let _ = self.tx.send(Msg::Clear);
    }

    // Einmal je Frame vor dem Zeichnen: neues Ergebnis abholen, bei Bedarf neu rechnen lassen.
    // `camera`: Weltposition der Kamera, `radius`: Sichtweite in Spalten.
    pub fn update(&mut self, camera: glam::DVec3, radius: i32) {
        while let Ok(r) = self.rx.try_recv() {
            self.in_flight = false;
            self.fresh.retain(|_, g| *g > r.generation);
            self.result = Some(r);
        }
        let section = (
            (camera.x / 16.0).floor() as i32,
            ((camera.y.floor() as i32 - self.min_y).div_euclid(16)),
            (camera.z / 16.0).floor() as i32,
        );
        let changed = match self.sent {
            Some((s, r, g)) => s != section || r != radius || (g != self.generation && !self.in_flight),
            None => true,
        };
        if changed {
            self.sent = Some((section, radius, self.generation));
            self.in_flight = true;
            let _ = self.tx.send(Msg::Compute { camera: section, radius, generation: self.generation });
        }
    }

    // Sichtbare Sections der Spalte als Bitmaske; alle, solange nichts Gesichertes vorliegt.
    pub fn mask(&self, key: Key) -> u64 {
        let Some(r) = &self.result else { return u64::MAX };
        if !self.fresh.is_empty() {
            let (x, z) = key;
            if [(x, z), (x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)].iter().any(|k| self.fresh.contains_key(k)) {
                return u64::MAX;
            }
        }
        r.masks.get(&key).copied().unwrap_or(u64::MAX)
    }
}

impl Default for OcclusionCulling {
    fn default() -> Self {
        Self::new()
    }
}

fn worker(rx: Receiver<Msg>, tx: Sender<Result>) {
    let mut columns: HashMap<Key, Arc<[SectionVis]>> = HashMap::new();
    while let Ok(first) = rx.recv() {
        // Alles Angestaute auf einmal einarbeiten; von mehreren Rechenauftraegen zaehlt nur der
        // letzte.
        let mut compute = None;
        for msg in std::iter::once(first).chain(rx.try_iter()) {
            match msg {
                Msg::Column(key, Some(vis)) => {
                    columns.insert(key, vis);
                }
                Msg::Column(key, None) => {
                    columns.remove(&key);
                }
                Msg::Clear => columns.clear(),
                Msg::Compute { camera, radius, generation } => compute = Some((camera, radius, generation)),
            }
        }
        if let Some((camera, radius, generation)) = compute {
            let masks = reachable(&columns, camera, radius);
            if tx.send(Result { generation, masks }).is_err() {
                return;
            }
        }
    }
}

// Richtungen wie die Seiten in world::visibility: 0 -Y, 1 +Y, 2 -Z, 3 +Z, 4 -X, 5 +X.
const STEP: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];
// Eingangsseiten je Section als Bitmaske; Bit 6 = die Section der Kamera (sieht in alle
// Richtungen).
const FROM_CAMERA: u8 = 1 << 6;

// Die Breitensuche. `camera` = (Spalte x, Section-Index, Spalte z).
fn reachable(columns: &HashMap<Key, Arc<[SectionVis]>>, camera: (i32, i32, i32), radius: i32) -> HashMap<Key, u64> {
    let sections = columns.values().map(|v| v.len()).max().unwrap_or(0);
    if sections == 0 || sections > 64 || radius < 0 {
        return HashMap::new();
    }
    let (cx, cy, cz) = camera;
    let cy = cy.clamp(0, sections as i32 - 1);
    let side = (2 * radius + 1) as usize;
    let s = sections;
    // Spaltendaten einmal ins Raster, damit die Suche selbst keine Hash-Zugriffe braucht.
    let mut grid: Vec<Option<&[SectionVis]>> = vec![None; side * side];
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let i = ((dx + radius) as usize) * side + (dz + radius) as usize;
            grid[i] = columns.get(&(cx + dx, cz + dz)).map(|v| &v[..]);
        }
    }
    let index = |dx: i32, dz: i32, y: i32| (((dx + radius) as usize) * side + (dz + radius) as usize) * s + y as usize;
    // Eingangsseiten je Section (0 = nicht erreicht). Jeder Schritt fuehrt eine Section weiter
    // von der Kamera weg, die Suche bearbeitet also erst alle Sections eines Abstands, dann die
    // des naechsten -- wenn eine Section drankommt, sind alle Wege zu ihr schon eingetragen.
    let mut entries = vec![0u8; side * side * s];
    let mut queue: std::collections::VecDeque<(i32, i32, i32)> = std::collections::VecDeque::new();
    entries[index(0, 0, cy)] = FROM_CAMERA;
    queue.push_back((0, 0, cy));
    while let Some((dx, dz, y)) = queue.pop_front() {
        let column = grid[((dx + radius) as usize) * side + (dz + radius) as usize];
        // Fehlende Daten (noch nicht gemesht) sperren nichts.
        let vis = column.and_then(|c| c.get(y as usize)).copied().unwrap_or(VIS_ALL);
        let eingang = entries[index(dx, dz, y)];
        let offset = [y - cy, dz, dx];
        for (d, &(sx, sy, sz)) in STEP.iter().enumerate() {
            // Nur von der Kamera weg: auf jeder Achse nie zurueck Richtung ihrer Section.
            let (achse, plus) = (d / 2, d % 2 == 1);
            if (plus && offset[achse] < 0) || (!plus && offset[achse] > 0) {
                continue;
            }
            if eingang & FROM_CAMERA == 0 && !(0..6).any(|e| eingang & (1 << e) != 0 && sees_through(vis, e, d)) {
                continue;
            }
            let (nx, ny, nz) = (dx + sx, y + sy, dz + sz);
            if nx.abs() > radius || nz.abs() > radius || ny < 0 || ny >= s as i32 {
                continue;
            }
            let i = index(nx, nz, ny);
            if entries[i] == 0 {
                queue.push_back((nx, nz, ny));
            }
            entries[i] |= 1 << (d ^ 1);
        }
    }
    let mut masks = HashMap::new();
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if grid[((dx + radius) as usize) * side + (dz + radius) as usize].is_none() {
                continue;
            }
            let mut m = 0u64;
            for y in 0..s {
                if entries[index(dx, dz, y as i32)] != 0 {
                    m |= 1 << y;
                }
            }
            masks.insert((cx + dx, cz + dz), m);
        }
    }
    masks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spalte(vis: &[SectionVis]) -> Arc<[SectionVis]> {
        vis.to_vec().into()
    }

    #[test]
    // Ueber massivem Fels (Sections 0..2 dicht) sieht man nur die Oberflaechen-Section und den
    // Himmel -- die Hoehlenschicht darunter bleibt unerreicht.
    fn fels_verdeckt_die_hoehle_darunter() {
        // Section 0: Hoehle (alles verbunden), 1: massiv, 2: Oberflaeche, 3: Luft.
        let col = spalte(&[VIS_ALL, 0, VIS_ALL, VIS_ALL]);
        let mut columns = HashMap::new();
        for x in -2..=2 {
            for z in -2..=2 {
                columns.insert((x, z), col.clone());
            }
        }
        let masks = reachable(&columns, (0, 3, 0), 2);
        for m in masks.values() {
            assert_eq!(*m & 0b0001, 0, "Hoehle sichtbar: {m:04b}");
            assert_eq!(*m & 0b0010, 0b0010, "die Fels-Section selbst wird erreicht (ihre Oberseite ist sichtbar)");
        }
    }

    #[test]
    // Steht man in der Hoehle, ist die Oberflaeche ueber dem Fels unsichtbar.
    fn aus_der_hoehle_keine_oberflaeche() {
        let col = spalte(&[VIS_ALL, 0, VIS_ALL, VIS_ALL]);
        let mut columns = HashMap::new();
        for x in -2..=2 {
            for z in -2..=2 {
                columns.insert((x, z), col.clone());
            }
        }
        let masks = reachable(&columns, (0, 0, 0), 2);
        for m in masks.values() {
            assert_eq!(*m & 0b1100, 0, "{m:04b}");
        }
    }

    #[test]
    // Ein senkrechter Schacht in einer Spalte oeffnet die Hoehle darunter.
    fn schacht_oeffnet_die_hoehle() {
        let mut columns = HashMap::new();
        for x in -2..=2 {
            for z in -2..=2 {
                let fels = if (x, z) == (1, 0) { VIS_ALL } else { 0 };
                columns.insert((x, z), spalte(&[VIS_ALL, fels, VIS_ALL, VIS_ALL]));
            }
        }
        let masks = reachable(&columns, (0, 3, 0), 2);
        assert_eq!(masks[&(1, 0)] & 1, 1, "unter dem Schacht");
        // Durch den Schacht im Osten sieht man nur weiter nach Osten/Sueden/Norden in die
        // Hoehle -- zurueck nach Westen unter einen selbst nicht (die Suche kehrt nie um).
        assert_eq!(masks[&(2, 2)] & 1, 1, "hinter dem Schacht");
        assert_eq!(masks[&(-2, 0)] & 1, 0, "unter einem selbst, auf der anderen Seite");
    }

    // Frueher: ein schmaler Streifen einer Sandsteinwand fehlte mal, mal nicht --
    // man sah den Baum dahinter. Eine Section, die nur von EINER ihrer Eintrittsseiten nach
    // hinten durchlaesst, wurde vom zuerst ankommenden Weg belegt; kam der durch die falsche
    // Seite, war alles dahinter verloren, auch wenn ein zweiter Weg durch die richtige Seite
    // kam. Welcher zuerst kommt, haengt an der Kamera-Section.
    #[test]
    fn zweiter_weg_durch_eine_andere_seite_zaehlt_auch() {
        // T (1, 1) laesst nur von Norden (-Z) nach Osten (+X) durch. Weg 1 kommt ueber (0, 1)
        // von Westen herein, Weg 2 ueber (1, 0) von Norden. Dahinter liegt U (2, 1); alles
        // andere ist Fels, U ist also nur durch T zu sehen.
        let nord_ost = 1 << (2 * 6 + 5);
        let mut columns = HashMap::new();
        for x in -1..=3 {
            for z in -1..=3 {
                let vis = match (x, z) {
                    (1, 1) => nord_ost,
                    (0, 0) | (1, 0) | (0, 1) | (2, 1) => VIS_ALL,
                    _ => 0,
                };
                columns.insert((x, z), spalte(&[vis]));
            }
        }
        let masks = reachable(&columns, (0, 0, 0), 3);
        assert_eq!(masks[&(1, 1)] & 1, 1, "T selbst");
        assert_eq!(masks[&(2, 1)] & 1, 1, "hinter T, ueber den Weg von Norden");
    }

    #[test]
    fn ohne_daten_ist_alles_sichtbar() {
        let c = OcclusionCulling { result: None, ..OcclusionCulling::new() };
        assert_eq!(c.mask((0, 0)), u64::MAX);
    }
}

#[cfg(test)]
mod zeit {
    use super::*;

    #[test]
    #[ignore]
    // cargo test --release -p render bfs_dauer -- --ignored --nocapture
    fn bfs_dauer() {
        let col: Arc<[SectionVis]> = vec![VIS_ALL; 24].into();
        let mut columns = HashMap::new();
        for x in -33..=33 {
            for z in -33..=33 {
                columns.insert((x, z), col.clone());
            }
        }
        let t = std::time::Instant::now();
        let n = 10;
        for _ in 0..n {
            std::hint::black_box(reachable(&columns, (0, 8, 0), 33));
        }
        eprintln!("BFS r=33, 24 Sections, alles offen: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    }
}
