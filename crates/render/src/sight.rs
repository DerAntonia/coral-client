// Sichtlinien-Culling fuer Entities und Blockentities (Stufe 2 des Entity-Cullings, s.
// app/render/culling.rs): von der Kamera aus laufen Strahlen durch das Blockraster zu Punkten
// auf den Seiten der Box, die der Kamera zugewandt sind. Erreicht keiner sein Ziel, ohne
// vorher in einen sichtdichten Block (world::visibility::blocks_view) zu laufen, ist das Ziel
// verdeckt.
//
// Die Rechnung laeuft auf einem eigenen Thread. Pro Frame geht hoechstens eine Anfrage raus;
// solange sie laeuft, gilt das letzte Ergebnis -- aber nur, solange die Kamera kaum weiter ist
// als dort, wo es gerechnet wurde. Im Zweifel ist alles sichtbar: unbekannte Ziele, Spalten
// ohne Daten, eine zu weit gewanderte Kamera.

use std::cell::Cell;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use glam::{DVec3, IVec3};
use rustc_hash::{FxHashMap, FxHashSet};
use world::visibility::SectionOpacity;

type Key = (i32, i32);

// Ziel einer Anfrage: ein Entity (ID) oder ein Blockentity (Blockposition), mit seiner Box.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SightTarget {
    Entity(i32),
    Block(i32, i32, i32),
}

enum Msg {
    Column(Key, i32, Arc<[SectionOpacity]>),
    Remove(Key),
    Clear,
    Query { viewer: DVec3, targets: Vec<(SightTarget, DVec3, DVec3)> },
}

struct Answer {
    viewer: DVec3,
    hidden: FxHashSet<SightTarget>,
    // Die Liste kommt zum Wiederverwenden zurueck.
    targets: Vec<(SightTarget, DVec3, DVec3)>,
}

// So weit darf sich die Kamera vom Rechenpunkt entfernen, bevor das Ergebnis nicht mehr gilt.
const VIEWER_TOLERANCE: f64 = 0.5;
// Weiter laeuft kein Strahl; dahinter gilt ein Ziel als sichtbar.
const MAX_RAY_BLOCKS: f64 = 256.0;

pub struct SightCulling {
    tx: Sender<Msg>,
    rx: Receiver<Answer>,
    result: Option<(DVec3, FxHashSet<SightTarget>)>,
    in_flight: bool,
    spare: Vec<(SightTarget, DVec3, DVec3)>,
    // Ob das letzte Ergebnis fuer diesen Frame gilt (s. begin_frame).
    valid: bool,
}

impl Default for SightCulling {
    fn default() -> Self {
        Self::new()
    }
}

impl SightCulling {
    #[must_use]
    pub fn new() -> Self {
        let (tx, worker_rx) = channel::<Msg>();
        let (worker_tx, rx) = channel::<Answer>();
        std::thread::Builder::new()
            .name("sicht-culling".into())
            .spawn(move || worker(&worker_rx, &worker_tx))
            .expect("Thread sicht-culling");
        Self { tx, rx, result: None, in_flight: false, spare: Vec::new(), valid: false }
    }

    pub fn set_column(&self, key: Key, min_y: i32, opacity: Arc<[SectionOpacity]>) {
        let _ = self.tx.send(Msg::Column(key, min_y, opacity));
    }

    pub fn remove_column(&self, key: Key) {
        let _ = self.tx.send(Msg::Remove(key));
    }

    pub fn clear(&mut self) {
        let _ = self.tx.send(Msg::Clear);
        self.result = None;
        self.valid = false;
    }

    // Zu Beginn jedes Frames: fertiges Ergebnis abholen und entscheiden, ob es fuer `viewer`
    // noch gilt. Liefert die (geleerte) Liste fuer die naechste Anfrage, wenn gerade keine
    // laeuft -- sonst None, dann wird diesen Frame nichts gefragt.
    pub fn begin_frame(&mut self, viewer: DVec3) -> Option<Vec<(SightTarget, DVec3, DVec3)>> {
        while let Ok(a) = self.rx.try_recv() {
            self.in_flight = false;
            self.result = Some((a.viewer, a.hidden));
            self.spare = a.targets;
        }
        self.valid = self.result.as_ref().is_some_and(|(v, _)| v.distance(viewer) <= VIEWER_TOLERANCE);
        if self.in_flight {
            return None;
        }
        let mut list = std::mem::take(&mut self.spare);
        list.clear();
        Some(list)
    }

    pub fn query(&mut self, viewer: DVec3, targets: Vec<(SightTarget, DVec3, DVec3)>) {
        if self.tx.send(Msg::Query { viewer, targets }).is_ok() {
            self.in_flight = true;
        }
    }

    // Verdeckt laut gueltigem Ergebnis?
    #[must_use]
    pub fn hidden(&self, target: SightTarget) -> bool {
        self.valid && self.result.as_ref().is_some_and(|(_, h)| h.contains(&target))
    }
}

type ColumnRef = Option<(Key, Option<(i32, Arc<[SectionOpacity]>)>)>;

struct Grid {
    columns: FxHashMap<Key, (i32, Arc<[SectionOpacity]>)>,
    // Die Zelle, an der zuletzt ein Strahl haengen blieb (s. ray_free). Strahlen zu
    // benachbarten Punkten laufen meist durch dieselbe Wandzelle.
    last_hit: Cell<Option<IVec3>>,
    // Nur fuer den Vergleichstest abschaltbar.
    shortcut: bool,
}

impl Grid {
    fn new(columns: FxHashMap<Key, (i32, Arc<[SectionOpacity]>)>) -> Self {
        Self { columns, last_hit: Cell::new(None), shortcut: true }
    }

    // Sichtdicht? Ohne Daten (nicht gemeshte Spalte, ausserhalb der Bauhoehe) nein.
    fn blocks(&self, cache: &mut ColumnRef, x: i32, y: i32, z: i32) -> bool {
        let key = (x.div_euclid(16), z.div_euclid(16));
        if cache.as_ref().is_none_or(|(k, _)| *k != key) {
            *cache = Some((key, self.columns.get(&key).cloned()));
        }
        let Some((_, Some((min_y, sections)))) = cache.as_ref() else { return false };
        let index = (y - min_y).div_euclid(16);
        let Some(section) = usize::try_from(index).ok().and_then(|i| sections.get(i)) else { return false };
        section.blocks(x.rem_euclid(16) as usize, (y - min_y).rem_euclid(16) as usize, z.rem_euclid(16) as usize)
    }

    // Laeuft ein Strahl von `from` nach `to` frei? Zelle fuer Zelle (3D-DDA). Die Zielzelle
    // selbst zaehlt nicht (das Ziel darf in einer Wand stecken), und sichtdichte Zellen direkt
    // an der Kamera auch nicht, bis der Strahl einmal durch Luft lief (Kamera im Block).
    fn ray_free(&self, cache: &mut ColumnRef, from: DVec3, to: DVec3) -> bool {
        let d = to - from;
        let mut cell = from.floor().as_ivec3();
        let end = to.floor().as_ivec3();
        let mut in_luft = !self.blocks(cache, cell.x, cell.y, cell.z);
        // Abkuerzung: geht die Strecke durch die zuletzt treffende Wandzelle, ist sie
        // verdeckt, ohne Zelle fuer Zelle zu laufen. Nur mit der Kamera in Luft -- sonst
        // koennte die Zelle zu den sichtdichten direkt an der Kamera gehoeren.
        if in_luft && self.shortcut {
            if let Some(h) = self.last_hit.get().filter(|&h| h != end && h != cell) {
                if segment_hits_cell(from, d, h) {
                    return false;
                }
            }
        }
        let step = d.signum().as_ivec3();
        let t_delta = DVec3::new(
            if d.x == 0.0 { f64::INFINITY } else { 1.0 / d.x.abs() },
            if d.y == 0.0 { f64::INFINITY } else { 1.0 / d.y.abs() },
            if d.z == 0.0 { f64::INFINITY } else { 1.0 / d.z.abs() },
        );
        let next = |p: f64, c: i32, s: i32, dt: f64| {
            if s > 0 {
                (f64::from(c) + 1.0 - p) * dt
            } else if s < 0 {
                (p - f64::from(c)) * dt
            } else {
                f64::INFINITY
            }
        };
        let mut t_max = DVec3::new(next(from.x, cell.x, step.x, t_delta.x), next(from.y, cell.y, step.y, t_delta.y), next(from.z, cell.z, step.z, t_delta.z));
        // Hoechstens so viele Schritte, wie Zellen zwischen Start und Ziel liegen.
        let schritte = (end - cell).abs().element_sum();
        for _ in 0..schritte {
            if t_max.x <= t_max.y && t_max.x <= t_max.z {
                cell.x += step.x;
                t_max.x += t_delta.x;
            } else if t_max.y <= t_max.z {
                cell.y += step.y;
                t_max.y += t_delta.y;
            } else {
                cell.z += step.z;
                t_max.z += t_delta.z;
            }
            if cell == end {
                return true;
            }
            if self.blocks(cache, cell.x, cell.y, cell.z) {
                if in_luft {
                    self.last_hit.set(Some(cell));
                    return false;
                }
            } else {
                in_luft = true;
            }
        }
        true
    }

    // Ist irgendein Punkt der kamerazugewandten Seiten der Box frei zu sehen?
    fn box_visible(&self, viewer: DVec3, min: DVec3, max: DVec3) -> bool {
        // Kamera in der Box oder Ziel sehr weit weg: sichtbar.
        if viewer.cmpge(min).all() && viewer.cmple(max).all() {
            return true;
        }
        if viewer.distance((min + max) * 0.5) > MAX_RAY_BLOCKS {
            return true;
        }
        let mut cache = None;
        const EPS: f64 = 0.05;
        for achse in 0..3 {
            // Seite dieser Achse, die zur Kamera zeigt -- liegt die Kamera innerhalb der
            // Ausdehnung, sieht sie keine der beiden Seiten dieser Achse.
            let ebene = if viewer[achse] < min[achse] {
                min[achse]
            } else if viewer[achse] > max[achse] {
                max[achse]
            } else {
                continue;
            };
            let (a, b) = ((achse + 1) % 3, (achse + 2) % 3);
            // Punkte im Raster, je Richtung ein Punkt mehr als die Seite Bloecke lang ist.
            let n = |k: usize| ((max[k] - min[k]).ceil() as usize + 1).clamp(2, 6);
            let (na, nb) = (n(a), n(b));
            for i in 0..na {
                for j in 0..nb {
                    let mut p = DVec3::ZERO;
                    p[achse] = ebene;
                    let lage = |k: usize, idx: usize, anzahl: usize| {
                        let lo = min[k] + EPS;
                        let hi = (max[k] - EPS).max(lo);
                        lo + (hi - lo) * idx as f64 / (anzahl - 1) as f64
                    };
                    p[a] = lage(a, i, na);
                    p[b] = lage(b, j, nb);
                    // Ein Hauch in die Box hinein, damit die Zielzelle die der Box ist.
                    p[achse] += if ebene == min[achse] { EPS } else { -EPS };
                    if self.ray_free(&mut cache, viewer, p) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

// Schneidet die Strecke from..from+d (ohne ihre Enden) die Einheitszelle `c`? Slab-Test.
fn segment_hits_cell(from: DVec3, d: DVec3, c: IVec3) -> bool {
    let lo = c.as_dvec3();
    let hi = lo + DVec3::ONE;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for k in 0..3 {
        if d[k].abs() < 1e-12 {
            if from[k] < lo[k] || from[k] > hi[k] {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo[k] - from[k]) / d[k], (hi[k] - from[k]) / d[k]);
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        t0 = t0.max(a);
        t1 = t1.min(b);
        // Nur Streifen an einer Kante oder Ecke zaehlt nicht (der Strahl liefe daran vorbei).
        if t1 - t0 < 1e-6 {
            return false;
        }
    }
    t1 > 0.0 && t0 < 1.0
}

// Letztes Ergebnis je Ziel, solange Kamera und Spaltendaten gleich bleiben: steht man still,
// muss nur neu gerechnet werden, was sich bewegt hat (Blockentities nie).
struct Memo {
    viewer: DVec3,
    results: FxHashMap<SightTarget, (DVec3, DVec3, bool)>,
}

fn worker(rx: &Receiver<Msg>, tx: &Sender<Answer>) {
    let mut grid = Grid::new(FxHashMap::default());
    let mut memo: Option<Memo> = None;
    while let Ok(msg) = rx.recv() {
        // Liegen mehrere Anfragen an, zaehlt nur die letzte; Spaltendaten gehen alle durch.
        let mut query = None;
        let mut neue_daten = false;
        let mut handle = |m: Msg, grid: &mut Grid, query: &mut Option<(DVec3, Vec<(SightTarget, DVec3, DVec3)>)>| match m {
            Msg::Column(key, min_y, data) => {
                grid.columns.insert(key, (min_y, data));
                neue_daten = true;
            }
            Msg::Remove(key) => {
                grid.columns.remove(&key);
                neue_daten = true;
            }
            Msg::Clear => {
                grid.columns.clear();
                neue_daten = true;
                *query = None;
            }
            Msg::Query { viewer, targets } => *query = Some((viewer, targets)),
        };
        handle(msg, &mut grid, &mut query);
        while let Ok(m) = rx.try_recv() {
            handle(m, &mut grid, &mut query);
        }
        if neue_daten {
            memo = None;
        }
        let Some((viewer, targets)) = query else { continue };
        let alt = memo.take().filter(|m| m.viewer.distance_squared(viewer) < 1e-8);
        let mut results = FxHashMap::default();
        results.reserve(targets.len());
        grid.last_hit.set(None);
        let mut hidden = FxHashSet::default();
        for &(t, min, max) in &targets {
            let gemerkt = alt.as_ref().and_then(|m| m.results.get(&t)).filter(|(a, b, _)| *a == min && *b == max).map(|r| r.2);
            let sichtbar = gemerkt.unwrap_or_else(|| grid.box_visible(viewer, min, max));
            results.insert(t, (min, max, sichtbar));
            if !sichtbar {
                hidden.insert(t);
            }
        }
        memo = Some(Memo { viewer, results });
        if tx.send(Answer { viewer, hidden, targets }).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wand_bei_x(x0: i32) -> Grid {
        // Eine Spalte (0,0) mit einer vollen Wand bei x = x0 (ganze Section 0 sonst leer).
        let mut bits = Box::new([0u64; 64]);
        for y in 0..16 {
            for z in 0..16 {
                let i = (y * 16 + z) * 16 + x0 as usize;
                bits[i / 64] |= 1 << (i % 64);
            }
        }
        let mut columns = FxHashMap::default();
        columns.insert((0, 0), (0, Arc::from(vec![SectionOpacity::Bits(bits)])));
        Grid::new(columns)
    }

    #[test]
    fn wand_verdeckt_box_dahinter() {
        let g = wand_bei_x(8);
        let viewer = DVec3::new(2.5, 5.5, 8.5);
        assert!(!g.box_visible(viewer, DVec3::new(12.0, 4.0, 7.0), DVec3::new(13.0, 6.0, 9.0)));
    }

    #[test]
    fn ohne_wand_sichtbar() {
        let g = wand_bei_x(8);
        let viewer = DVec3::new(2.5, 5.5, 8.5);
        assert!(g.box_visible(viewer, DVec3::new(4.0, 4.0, 7.0), DVec3::new(5.0, 6.0, 9.0)));
    }

    #[test]
    fn ueber_die_wand_sichtbar() {
        // Die Wand endet bei y = 16 (nur Section 0); Box ragt darueber hinaus.
        let g = wand_bei_x(8);
        let viewer = DVec3::new(2.5, 15.5, 8.5);
        assert!(g.box_visible(viewer, DVec3::new(12.0, 15.0, 7.0), DVec3::new(13.0, 18.0, 9.0)));
    }

    #[test]
    fn kamera_in_der_wand_sieht_durch_die_eigene_zelle() {
        let g = wand_bei_x(8);
        let viewer = DVec3::new(8.5, 5.5, 8.5);
        assert!(g.box_visible(viewer, DVec3::new(12.0, 4.0, 7.0), DVec3::new(13.0, 6.0, 9.0)));
    }

    #[test]
    fn ziel_in_der_wand_zaehlt_nicht() {
        let g = wand_bei_x(8);
        let viewer = DVec3::new(2.5, 5.5, 8.5);
        assert!(g.box_visible(viewer, DVec3::new(8.2, 5.0, 8.0), DVec3::new(8.8, 6.0, 9.0)));
    }

    // Die Abkuerzung ueber die letzte Trefferzelle darf nie etwas anderes ergeben als der
    // volle Strahl: zufaellige Bloecke, Kameras und Ziele, beide Wege verglichen.
    #[test]
    fn abkuerzung_wie_voller_strahl() {
        let mut zustand: u64 = 0x2545_f491_4f6c_dd1d;
        let mut zufall = move || {
            zustand ^= zustand << 13;
            zustand ^= zustand >> 7;
            zustand ^= zustand << 17;
            zustand
        };
        for _ in 0..40 {
            let mut columns = FxHashMap::default();
            for cx in 0..2 {
                for cz in 0..2 {
                    let mut bits = Box::new([0u64; 64]);
                    for w in bits.iter_mut() {
                        // Etwa jede achte Zelle dicht.
                        *w = zufall() & zufall() & zufall();
                    }
                    columns.insert((cx, cz), (0, Arc::from(vec![SectionOpacity::Bits(bits)])));
                }
            }
            let mit = Grid::new(columns.clone());
            let mut ohne = Grid::new(columns);
            ohne.shortcut = false;
            for _ in 0..200 {
                let mut pos = || DVec3::new((zufall() % 3000) as f64 / 100.0 + 1.0, (zufall() % 1400) as f64 / 100.0 + 1.0, (zufall() % 3000) as f64 / 100.0 + 1.0);
                let viewer = pos();
                let min = pos();
                let max = min + DVec3::new(0.6, 1.8, 0.6);
                assert_eq!(mit.box_visible(viewer, min, max), ohne.box_visible(viewer, min, max), "viewer {viewer} box {min}..{max}");
            }
        }
    }

    #[test]
    fn unbekannte_spalte_ist_durchsichtig() {
        let g = Grid::new(FxHashMap::default());
        assert!(g.box_visible(DVec3::new(0.5, 70.0, 0.5), DVec3::new(30.0, 70.0, 30.0), DVec3::new(31.0, 72.0, 31.0)));
    }
}
