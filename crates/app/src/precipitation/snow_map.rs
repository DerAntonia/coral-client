// Frei/blockiert-Karte fuer den Schnee (Entwurf des Nutzers): fuer jeden Block im Umkreis des
// maximalen Schneeradius (Vorgabe 2r)
// um das Auge (Mittelpunkt des Blocks), ob dort Schnee sein kann, und der Bereich, in dem
// Schnee zu sehen ist.
//
// Blockiert ist ein Block, wenn
//   - er Schnee aufhaelt (jede Kollisionsform und Fluessigkeit, keine Barriere; s.
//     world::weather::snow_stop), oder
//   - von oben kein Schnee zu ihm kommen kann. Das wird von oben nach unten gefuellt: ueber dem
//     hoechsten aufhaltenden Block der Spalte (oder wenn die Spalte nicht geladen ist) ist alles
//     frei; darunter ist ein Block frei, wenn der Block direkt darueber frei ist. In jeder
//     SIDE_EVERY-ten Hoehenlage darf der Schnee ausserdem einen Block zur Seite, wenn der
//     Nachbar von oben Schnee bekommt -- das ergibt etwa 15 Grad unter eine Dachkante. So haengt
//     jeder freie Block ueber freie Bloecke mit dem Himmel zusammen, und zwischen zwei Stufen
//     eines Daches rutscht nichts hindurch.
//
// Die Karte ist ein Wuerfel um den Block des Auges; berechnet wird nur, was in der Kugel liegt.
// Zieht das Auge weiter, wird Bekanntes uebernommen und nur das Neue berechnet. Die hoechsten
// Bloecke der Spalten werden erst abgefragt, wenn sie gebraucht werden, und gemerkt; das Ja/Nein
// der Bloecke unter einem Dach bleibt gemerkt, auch wenn man weggeht.
// Aendert sich in einer Chunk-Spalte, aus der etwas gelesen wurde, der Stand der Welt (s.
// World::snow_revision), wird alles neu berechnet.
//
// Der Bereich (der weisse Fleck aus der Skizze des Nutzers): ein freier Block gehoert dazu, wenn
// auf der Linie von der Mitte des Augen-Blocks zu seiner Mitte hoechstens r freie Strecke
// liegt -- blockierte Stuecke zaehlen nicht. Im Freien ist das die Kugel mit Radius r; unter
// einem Dach reicht er um die blockierte Strecke weiter (hoechstens bis zum maximalen
// Schneeradius, so weit die Karte geht). Neu berechnet, wenn das Auge in einen anderen Block geht oder die Karte sich aendert.

use std::collections::HashMap;

use glam::DVec3;

const UNBEKANNT: u8 = 0;
const FREI: u8 = 1;
const BLOCKIERT: u8 = 2;

// Mehr gemerkte Spalten-Hoechstwerte als das, dann von vorn.
const TOPS_LIMIT: usize = 200_000;

// In jeder wievielten Hoehenlage der Schnee einen Block zur Seite darf (1 / tan 15 Grad = 3,7).
const SIDE_EVERY: i32 = 4;

// Fuer die Bereichsrechnung (s. SnowMap::update_region): die Zellen der Kugel nach Entfernung vom
// Auge geordnet, je Zelle die bis zu vier Zellen, zwischen denen ihre Linie zum Auge einen Block
// weiter innen liegt (einen Block zurueck entlang der groessten Achse), mit Gewichten nach Naehe.
// Einmal je Kartenradius (bei 32 etwa 6 MB).
struct MischZelle {
    zelle: u32,
    vor: [u32; 4],
    gewicht: [f32; 4],
    // Laenge der Linie vom Auge bis zur Zelle, und ihr Stueck seit den Zellen `vor`.
    laenge: f32,
    zuwachs: f32,
}

struct Misch {
    radius: i32,
    zellen: Vec<MischZelle>,
}

impl Misch {
    const fn leer() -> Self {
        Self { radius: -1, zellen: Vec::new() }
    }

    fn neu(radius: i32) -> Self {
        let n = 2 * radius + 1;
        let index = |b: [i32; 3]| (((b[1] + radius) * n + b[2] + radius) * n + b[0] + radius) as u32;
        let mitte = index([0, 0, 0]);
        let mut ordnung: Vec<(i32, [i32; 3])> = Vec::new();
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    let d2 = dx * dx + dy * dy + dz * dz;
                    if d2 <= radius * radius && d2 != 0 {
                        ordnung.push((d2, [dx, dy, dz]));
                    }
                }
            }
        }
        // Die Zellen `vor` liegen immer naeher am Auge als die Zelle selbst: nach Entfernung
        // geordnet sind sie also schon berechnet.
        ordnung.sort_unstable_by_key(|&(d2, v)| (d2, index(v)));
        let zellen = ordnung
            .iter()
            .map(|&(d2, v)| {
                let a = (0..3).max_by_key(|&k| v[k].abs()).unwrap_or(0);
                let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                let f = 1.0 - 1.0 / f64::from(v[a].abs());
                let (qb, qc) = (f64::from(v[b]) * f, f64::from(v[c]) * f);
                let (b0, c0) = (qb.floor(), qc.floor());
                let (fb, fc) = (qb - b0, qc - c0);
                let mut vor = [mitte; 4];
                let mut gewicht = [0f32; 4];
                for k in 0..4 {
                    let (ob, oc) = (k & 1 == 1, k >> 1 == 1);
                    let w = (if ob { fb } else { 1.0 - fb }) * (if oc { fc } else { 1.0 - fc });
                    // Gewicht 0: die Zelle zaehlt nicht (und kann ausserhalb der Karte liegen).
                    if w > 0.0 {
                        let mut q = [0i32; 3];
                        q[a] = v[a] - v[a].signum();
                        q[b] = b0 as i32 + i32::from(ob);
                        q[c] = c0 as i32 + i32::from(oc);
                        vor[k] = index(q);
                        gewicht[k] = w as f32;
                    }
                }
                let laenge = f64::from(d2).sqrt();
                MischZelle { zelle: index(v), vor, gewicht, laenge: laenge as f32, zuwachs: (laenge / f64::from(v[a].abs())) as f32 }
            })
            .collect();
        Self { radius, zellen }
    }
}

// Was sich in der Welt geaendert hat (s. World::snow_revision, snow_changes_since): der Stand
// einer Chunk-Spalte, und welche Bloecke ihn seit einem frueheren Stand hochgezaehlt haben (None:
// unbekannt oder die ganze Spalte). Eine blosse Funktion (cx, cz) -> Stand gilt als "immer die
// ganze Spalte".
pub trait SnowChanges {
    fn revision(&self, cx: i32, cz: i32) -> u64;
    fn since(&self, cx: i32, cz: i32, stand: u64) -> Option<Vec<(i32, i32, i32)>>;
}

impl<F: Fn(i32, i32) -> u64> SnowChanges for F {
    fn revision(&self, cx: i32, cz: i32) -> u64 {
        self(cx, cz)
    }

    fn since(&self, _: i32, _: i32, _: u64) -> Option<Vec<(i32, i32, i32)>> {
        None
    }
}

pub(super) struct SnowMap {
    // Block des Auges und Radius; der Wuerfel hat die Kante 2 * radius + 1.
    center: (i32, i32, i32),
    radius: i32,
    cells: Vec<u8>,
    // Spalte -> Y des hoechsten aufhaltenden Blocks (None: keiner oder nicht geladen).
    tops: HashMap<(i32, i32), Option<i32>>,
    // Block unter dem hoechsten seiner Spalte -> ob er frei ist (s. oben).
    filled: HashMap<(i32, i32, i32), bool>,
    // Tiefstes Y, fuer das je etwas in `filled` oder `low` kam (seit dem letzten Leeren).
    memo_min_y: i32,
    // Niedriger Block (Stufe, Teppich, ...) -> Hoehe seiner Oberkante ueber der Unterseite des Feldes.
    low: HashMap<(i32, i32, i32), f32>,
    // Chunk-Spalte -> Stand der Welt, als zuerst daraus gelesen wurde.
    seen: HashMap<(i32, i32), u64>,
    // r (die Karte reicht bis zum maximalen Schneeradius).
    r: i32,
    // Je Block wie `cells`: ob er zum Bereich gehoert.
    region: Vec<bool>,
    // Der Bereich ist veraltet.
    region_dirty: bool,
    // Es gibt Zellen in der Kugel, die noch nicht berechnet sind.
    unberechnet: bool,
    // Zellen (Index wie `cells`), die in diesem update durch einen Blockwechsel anders wurden.
    geaendert: Vec<usize>,
    misch: Misch,
    // Je Block wie `cells`: die verdeckte Strecke auf seiner Linie zum Auge (s. update_region).
    verdeckt: Vec<f32>,
    // Zaehlt jede Neuberechnung des Bereichs.
    region_version: u64,
    // DIAG Schnee: Gruende der letzten Neuberechnung des Bereichs.
    pub(super) diag: Vec<String>,
    // DIAG Zeit: Fuellen und Bereich, aufsummiert.
    pub(super) diag_zeit: [std::time::Duration; 3],
}

impl SnowMap {
    pub(super) fn new() -> Self {
        Self { center: (0, 0, 0), radius: -1, cells: Vec::new(), tops: HashMap::new(), filled: HashMap::new(), memo_min_y: i32::MAX, low: HashMap::new(), seen: HashMap::new(), r: 0, region: Vec::new(), region_dirty: true, unberechnet: true, geaendert: Vec::new(), misch: Misch::leer(), verdeckt: Vec::new(), region_version: 0, diag: Vec::new(), diag_zeit: [std::time::Duration::ZERO; 3] }
    }

    const fn size(&self) -> i32 {
        2 * self.radius + 1
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (dx, dy, dz) = (x - self.center.0 + self.radius, y - self.center.1 + self.radius, z - self.center.2 + self.radius);
        let n = self.size();
        if self.radius < 0 || !(0..n).contains(&dx) || !(0..n).contains(&dy) || !(0..n).contains(&dz) {
            return None;
        }
        Some(((dy * n + dz) * n + dx) as usize)
    }

    // Ob der Block blockiert ist; None, wo (noch) nichts berechnet ist.
    pub(super) fn blocked(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        match self.cells[self.index(x, y, z)?] {
            FREI => Some(false),
            BLOCKIERT => Some(true),
            _ => None,
        }
    }

    pub(super) const fn region_version(&self) -> u64 {
        self.region_version
    }

    // Der Bereich als Tabelle: je Block des Wuerfels (Kante 2 * Halbkante + 1 um `mitte`, Index
    // ((dy * n + dz) * n + dx)) ob er dazugehoert.
    pub(super) fn region_table(&self) -> (&[bool], (i32, i32, i32), i32) {
        (&self.region, self.center, self.radius)
    }

    // Alle Bloecke des Bereichs.
    #[cfg(test)]
    pub(super) fn region_blocks(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        let (n, radius, c) = (self.size(), self.radius, self.center);
        self.region.iter().enumerate().filter(|&(_, &drin)| drin).map(move |(i, _)| {
            let i = i as i32;
            (c.0 - radius + i % n, c.1 - radius + i / (n * n), c.2 - radius + (i / n) % n)
        })
    }

    // Ob der Block zum Bereich gehoert (s. oben).
    pub(super) fn in_region(&self, x: i32, y: i32, z: i32) -> bool {
        self.index(x, y, z).and_then(|i| self.region.get(i).copied()).unwrap_or(false)
    }

    // Ob an dieser Stelle (kann auch im Feld eines niedrigen Blocks oberhalb seiner Oberkante sein)
    // eine Flocke sein darf: im Bereich, oder ueber einem niedrigen Block, ueber dem der Bereich
    // weitergeht.
    pub(super) fn holds(&self, p: DVec3) -> bool {
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        self.in_region(x, y, z) || (self.in_region(x, y + 1, z) && self.low.get(&(x, y, z)).is_some_and(|&h| p.y >= f64::from(y) + f64::from(h)))
    }

    // Die Karte auf das Auge `eye` mit Schneeradius `r` und maximalem Schneeradius `max` (Halbkante
    // des Wuerfels) bringen. `top(x, z)` Y des hoechsten Blocks der
    // Spalte, der Schnee aufhaelt, `solid(x, y, z)` ob dort einer ist (Some = ja),
    // `revision(cx, cz)` der Stand der Chunk-Spalte.
    pub(super) fn update(
        &mut self,
        eye: DVec3,
        r: i32,
        max: i32,
        top: &impl Fn(i32, i32) -> Option<i32>,
        solid: &impl Fn(i32, i32, i32) -> Option<f32>,
        revision: &impl SnowChanges,
    ) {
        let radius = max;
        self.diag.clear();
        self.geaendert.clear();
        if r != self.r {
            self.diag.push(format!("radius {} -> {r}", self.r));
            self.r = r;
            self.region_dirty = true;
        }
        // Geaendert, was gelesen wurde: nur verwerfen, was davon abhaengen kann (s. invalidate).
        let geaendert: Vec<((i32, i32), u64, u64)> =
            self.seen.iter().map(|(&c, &stand)| (c, stand, revision.revision(c.0, c.1))).filter(|&(_, stand, jetzt)| stand != jetzt).collect();
        if self.tops.len() > TOPS_LIMIT {
            self.diag.push("tops-limit".into());
            self.tops.clear();
            self.filled.clear();
            self.low.clear();
            self.memo_min_y = i32::MAX;
            self.seen.clear();
            self.cells.fill(UNBEKANNT);
            self.unberechnet = true;
            self.region_dirty = true;
        } else {
            for (c, stand, jetzt) in geaendert {
                self.seen.insert(c, jetzt);
                match revision.since(c.0, c.1, stand) {
                    Some(bloecke) => {
                        self.diag.push(format!("chunk {c:?}: {} bloecke geaendert", bloecke.len()));
                        for (x, y, z) in bloecke {
                            self.block_changed(x, y, z, top, solid, revision);
                        }
                    }
                    None => {
                        // Die ganze Spalte: bis zum hoechsten Block, den es in ihr gab oder gibt.
                        let (x0, z0) = (c.0 * 16, c.1 * 16);
                        let mut hoch = None::<i32>;
                        for z in z0..z0 + 16 {
                            for x in x0..x0 + 16 {
                                let alt = self.tops.get(&(x, z)).copied().flatten();
                                hoch = [hoch, alt, top(x, z)].into_iter().flatten().max();
                            }
                        }
                        self.diag.push(format!("chunk {c:?} ganz, bis y {hoch:?}"));
                        if let Some(hoch) = hoch {
                            self.invalidate((x0, x0 + 15), (z0, z0 + 15), hoch);
                        } else {
                            self.forget_tops((x0, x0 + 15), (z0, z0 + 15));
                        }
                    }
                }
            }
        }

        let center = (eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
        if center != self.center {
            self.diag.push(format!("augenblock {:?} -> {center:?} (auge {:.3} {:.3} {:.3})", self.center, eye.x, eye.y, eye.z));
            self.region_dirty = true;
        }
        if radius != self.radius {
            self.diag.push(format!("kartenradius {} -> {radius}", self.radius));
            self.center = center;
            self.radius = radius;
            self.region_dirty = true;
            self.cells = vec![UNBEKANNT; (self.size() as usize).pow(3)];
            self.unberechnet = true;
        } else if center != self.center {
            self.unberechnet = true;
            let alt = std::mem::take(&mut self.cells);
            let alt_center = self.center;
            self.center = center;
            let n = self.size();
            self.cells = vec![UNBEKANNT; (n as usize).pow(3)];
            for dy in 0..n {
                for dz in 0..n {
                    for dx in 0..n {
                        let (x, y, z) = (center.0 - radius + dx, center.1 - radius + dy, center.2 - radius + dz);
                        let (ax, ay, az) = (x - alt_center.0 + radius, y - alt_center.1 + radius, z - alt_center.2 + radius);
                        if (0..n).contains(&ax) && (0..n).contains(&ay) && (0..n).contains(&az) {
                            self.cells[((dy * n + dz) * n + dx) as usize] = alt[((ay * n + az) * n + ax) as usize];
                        }
                    }
                }
            }
        }

        let n = self.size();
        let r2 = radius * radius;
        let mut neu_berechnet = 0;
        let zeit_fill = std::time::Instant::now();
        // Im Stehen ohne neue Chunks ist alles berechnet: nicht den ganzen Wuerfel durchsehen.
        let dy_bereich = if self.unberechnet { -radius..=radius } else { radius + 1..=radius };
        self.unberechnet = false;
        for dy in dy_bereich {
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    if dx * dx + dy * dy + dz * dz > r2 {
                        continue;
                    }
                    let i = (((dy + radius) * n + dz + radius) * n + dx + radius) as usize;
                    if self.cells[i] != UNBEKANNT {
                        continue;
                    }
                    let (x, y, z) = (center.0 + dx, center.1 + dy, center.2 + dz);
                    self.cells[i] = if self.free(x, y, z, top, solid, revision) { FREI } else { BLOCKIERT };
                    if let Some(hoehe) = solid(x, y, z).filter(|&h| h < 1.0) {
                        self.memo_min_y = self.memo_min_y.min(y);
                        self.low.insert((x, y, z), hoehe);
                    }
                    neu_berechnet += 1;
                    self.region_dirty = true;
                }
            }
        }
        if neu_berechnet > 0 {
            self.diag.push(format!("{neu_berechnet} zellen neu berechnet"));
        }
        self.diag_zeit[0] += zeit_fill.elapsed();
        // Zellen durch einen Blockwechsel anders: den Bereich ganz neu.
        if !self.geaendert.is_empty() {
            self.diag.push(format!("{} zellen durch blockwechsel anders", self.geaendert.len()));
            self.region_dirty = true;
        }
        if self.region_dirty {
            self.region_dirty = false;
            let zeit = std::time::Instant::now();
            self.update_region();
            self.diag_zeit[1] += zeit.elapsed();
        }
    }

    // Den Bereich neu berechnen (s. oben), angenaehert: von innen nach aussen bekommt jede Zelle
    // die verdeckte (nicht freie) Strecke ihrer Linie zum Auge aus den Zellen, zwischen denen die
    // Linie einen Block weiter innen liegt (nach Naehe gemischt, s. Misch), plus ihr eigenes
    // Stueck -- das zaehlt je zur Haelfte nach ihr selbst und nach jenen Zellen als verdeckt. Die
    // freie Strecke ist die Laenge der Linie minus das Verdeckte. Im Freien ist nichts verdeckt,
    // dort stimmt es genau; sonst weicht der Rand um bis zu etwa einen Block von der genauen
    // Linie ab (s. Test bereich_weicht_nur_am_rand_ab).
    fn update_region(&mut self) {
        if self.misch.radius != self.radius {
            self.misch = Misch::neu(self.radius);
        }
        let (radius, n) = (self.radius, self.size());
        let r = self.r as f32 + 1e-4;
        self.region.clear();
        self.region.resize(self.cells.len(), false);
        self.verdeckt.clear();
        self.verdeckt.resize(self.cells.len(), 0.0);
        self.region_version += 1;
        let mitte = (((radius * n) + radius) * n + radius) as usize;
        let (cells, verdeckt, region) = (&self.cells, &mut self.verdeckt, &mut self.region);
        region[mitte] = cells[mitte] == FREI;
        for m in &self.misch.zellen {
            let i = m.zelle as usize;
            let frei = cells[i] == FREI;
            let (mut v, mut vor_frei) = (0.0, 0.0);
            for k in 0..4 {
                let z = m.vor[k] as usize;
                v += m.gewicht[k] * verdeckt[z];
                if cells[z] == FREI {
                    vor_frei += m.gewicht[k];
                }
            }
            v += m.zuwachs * (1.0 - 0.5 * vor_frei - if frei { 0.5 } else { 0.0 });
            verdeckt[i] = v;
            region[i] = frei && m.laenge - v <= r;
        }
    }

    // Der Block (x, y, z) haelt jetzt Schnee auf oder nicht mehr (oder anders hoch): nachrechnen,
    // was sich dadurch aendert. Lage fuer Lage abwaerts -- ob ein Block frei ist, haengt nur von
    // Bloecken auf seiner Hoehe und der Lage darueber ab, zur Seite nur in jeder SIDE_EVERY-ten
    // Lage um eine Spalte (s. free). In jeder Lage alle Bloecke, die es treffen kann; ist in
    // einer ganzen Lage nichts anders geworden, dann auch darunter nichts (dort sind alle
    // Eingaben dieselben). Ein Block auf flachem Boden: er selbst und die Lage darunter.
    fn block_changed(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        top: &impl Fn(i32, i32) -> Option<i32>,
        solid: &impl Fn(i32, i32, i32) -> Option<f32>,
        revision: &impl SnowChanges,
    ) {
        // Nie nach dieser Spalte gefragt: nichts haengt von ihr ab.
        let Some(alt_top) = self.tops.remove(&(x, z)) else { return };
        self.column_top(x, z, top, revision);
        self.low.remove(&(x, y, z));
        if let Some(hoehe) = solid(x, y, z).filter(|&h| h < 1.0) {
            self.memo_min_y = self.memo_min_y.min(y);
            self.low.insert((x, y, z), hoehe);
        }
        let unten = self.memo_min_y.min(self.center.1 - self.radius);
        let mut w = 0;
        for k in (unten..=y).rev() {
            if k.rem_euclid(SIDE_EVERY) == 0 {
                w += 1;
            }
            let mut anders = false;
            for zz in z - w..=z + w {
                for xx in x - w..=x + w {
                    let zelle = self.index(xx, k, zz).filter(|&i| self.cells[i] != UNBEKANNT);
                    // Was vorher galt: die Zelle der Karte, sonst das Gemerkte, sonst "ueber dem
                    // hoechsten Block der Spalte: frei". Nichts davon: nie berechnet, nichts
                    // haengt davon ab.
                    let spalte = if (xx, zz) == (x, z) { Some(alt_top) } else { self.tops.get(&(xx, zz)).copied() };
                    let vorher = match (zelle, self.filled.get(&(xx, k, zz))) {
                        (Some(i), _) => self.cells[i] == FREI,
                        (None, Some(&frei)) => frei,
                        (None, None) => match spalte {
                            Some(t) if t.is_none_or(|t| k > t) => true,
                            _ => continue,
                        },
                    };
                    self.filled.remove(&(xx, k, zz));
                    let frei = self.free(xx, k, zz, top, solid, revision);
                    if frei != vorher {
                        anders = true;
                        if let Some(i) = zelle {
                            self.cells[i] = if frei { FREI } else { BLOCKIERT };
                            self.geaendert.push(i);
                        }
                    }
                }
            }
            if !anders {
                break;
            }
        }
    }

    fn forget_tops(&mut self, x: (i32, i32), z: (i32, i32)) {
        for zz in z.0..=z.1 {
            for xx in x.0..=x.1 {
                self.tops.remove(&(xx, zz));
            }
        }
    }

    // In den Spalten x.0..=x.1, z.0..=z.1 hat sich bis hinauf zu `hoch` etwas geaendert: alles
    // Gemerkte verwerfen, das davon abhaengen kann. Ob ein Block frei ist, haengt nur von Bloecken
    // auf seiner Hoehe und darueber ab, und zur Seite nur in jeder SIDE_EVERY-ten Lage um eine
    // Spalte (s. free): betroffen ist also alles ab `hoch` abwaerts, seitlich je solcher Lage
    // zwischen `hoch` und der eigenen Hoehe eine Spalte weiter.
    fn invalidate(&mut self, x: (i32, i32), z: (i32, i32), hoch: i32) {
        self.forget_tops(x, z);
        let unten = self.memo_min_y.min(self.center.1 - self.radius);
        if self.radius < 0 || hoch < unten {
            return;
        }
        // So viele Lagen mit Seitenschritt liegen in k..=hoch.
        let weite = |k: i32| hoch.div_euclid(SIDE_EVERY) - (k + SIDE_EVERY - 1).div_euclid(SIDE_EVERY) + 1;
        let w = weite(unten).max(0);
        let menge = i64::from(x.1 - x.0 + 1 + 2 * w) * i64::from(z.1 - z.0 + 1 + 2 * w) * i64::from(hoch - unten + 1);
        let trifft = |&(bx, by, bz): &(i32, i32, i32)| {
            by <= hoch && {
                let w = weite(by).max(0);
                bx >= x.0 - w && bx <= x.1 + w && bz >= z.0 - w && bz <= z.1 + w
            }
        };
        if menge > (self.filled.len() + self.low.len()) as i64 {
            // Mehr Stellen als Gemerktes: das Gemerkte durchgehen.
            self.filled.retain(|k, _| !trifft(k));
            self.low.retain(|k, _| !trifft(k));
        } else {
            for y in unten..=hoch {
                let w = weite(y).max(0);
                for zz in z.0 - w..=z.1 + w {
                    for xx in x.0 - w..=x.1 + w {
                        self.filled.remove(&(xx, y, zz));
                        self.low.remove(&(xx, y, zz));
                    }
                }
            }
        }
        // Die Zellen der Karte.
        let (c, r) = (self.center, self.radius);
        for y in (c.1 - r).max(unten)..=(c.1 + r).min(hoch) {
            let w = weite(y).max(0);
            for zz in (z.0 - w).max(c.2 - r)..=(z.1 + w).min(c.2 + r) {
                for xx in (x.0 - w).max(c.0 - r)..=(x.1 + w).min(c.0 + r) {
                    if let Some(i) = self.index(xx, y, zz) {
                        if self.cells[i] != UNBEKANNT {
                            self.cells[i] = UNBEKANNT;
                            self.unberechnet = true;
                        }
                    }
                }
            }
        }
    }

    // Freie Strecke auf der Linie von `von` nach `bis`: Block fuer Block das Stueck, das in
    // einem freien Block liegt (nicht Berechnetes zaehlt als blockiert).
    #[cfg(test)]
    fn open_length(&self, von: DVec3, bis: DVec3) -> f64 {
        let d = bis - von;
        let laenge = d.length();
        if laenge < 1e-9 {
            return 0.0;
        }
        let mut block = [von.x.floor() as i32, von.y.floor() as i32, von.z.floor() as i32];
        let (vo, dd) = (von.to_array(), d.to_array());
        let mut schritt = [0i32; 3];
        let mut t_max = [f64::INFINITY; 3];
        let mut t_delta = [f64::INFINITY; 3];
        for a in 0..3 {
            if dd[a] > 0.0 {
                schritt[a] = 1;
                t_max[a] = (f64::from(block[a]) + 1.0 - vo[a]) / dd[a];
                t_delta[a] = 1.0 / dd[a];
            } else if dd[a] < 0.0 {
                schritt[a] = -1;
                t_max[a] = (f64::from(block[a]) - vo[a]) / dd[a];
                t_delta[a] = -1.0 / dd[a];
            }
        }
        let (mut t, mut offen) = (0.0, 0.0);
        loop {
            let a = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] { 0 } else if t_max[1] <= t_max[2] { 1 } else { 2 };
            let weiter = t_max[a].min(1.0);
            if self.blocked(block[0], block[1], block[2]) == Some(false) {
                offen += (weiter - t) * laenge;
            }
            if weiter >= 1.0 {
                return offen;
            }
            t = weiter;
            block[a] += schritt[a];
            t_max[a] += t_delta[a];
        }
    }

    fn column_top(&mut self, x: i32, z: i32, top: &impl Fn(i32, i32) -> Option<i32>, revision: &impl SnowChanges) -> Option<i32> {
        if let Some(&t) = self.tops.get(&(x, z)) {
            return t;
        }
        let chunk = (x.div_euclid(16), z.div_euclid(16));
        self.seen.entry(chunk).or_insert_with(|| revision.revision(chunk.0, chunk.1));
        let t = top(x, z);
        self.tops.insert((x, z), t);
        t
    }

    // Ob Schnee zu diesem Block kommen kann (s. oben).
    fn free(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        top: &impl Fn(i32, i32) -> Option<i32>,
        solid: &impl Fn(i32, i32, i32) -> Option<f32>,
        revision: &impl SnowChanges,
    ) -> bool {
        if self.column_top(x, z, top, revision).is_none_or(|t| y > t) {
            return true;
        }
        if let Some(&frei) = self.filled.get(&(x, y, z)) {
            return frei;
        }
        let frei = self.ist_frei_ueber(x, y, z, top, solid, revision)
            || (solid(x, y, z).is_none()
                && y.rem_euclid(SIDE_EVERY) == 0
                && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dz)| self.ist_frei_ueber(x + dx, y, z + dz, top, solid, revision)));
        self.memo_min_y = self.memo_min_y.min(y);
        self.filled.insert((x, y, z), frei);
        frei
    }

    // Ob dieser Block kein Hindernis ist und der direkt darueber frei.
    fn ist_frei_ueber(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        top: &impl Fn(i32, i32) -> Option<i32>,
        solid: &impl Fn(i32, i32, i32) -> Option<f32>,
        revision: &impl SnowChanges,
    ) -> bool {
        self.column_top(x, z, top, revision);
        solid(x, y, z).is_none() && self.free(x, y + 1, z, top, solid, revision)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    // Eine kleine Welt aus Bloecken (alles andere Luft), Stand je Chunk-Spalte.
    pub(in crate::precipitation) struct Welt {
        bloecke: std::collections::HashSet<(i32, i32, i32)>,
        staende: HashMap<(i32, i32), u64>,
        // Chunk-Spalte, Stand danach, geaenderter Block (None: die ganze Spalte).
        log: Vec<((i32, i32), u64, Option<(i32, i32, i32)>)>,
    }

    // Die Welt mit dem, was sich geaendert hat (statt nur dem Stand je Spalte).
    struct MitLog<'a>(&'a Welt);

    impl SnowChanges for MitLog<'_> {
        fn revision(&self, cx: i32, cz: i32) -> u64 {
            self.0.staende.get(&(cx, cz)).copied().unwrap_or(0)
        }

        fn since(&self, cx: i32, cz: i32, stand: u64) -> Option<Vec<(i32, i32, i32)>> {
            self.0.log.iter().filter(|&&(c, s, _)| c == (cx, cz) && s > stand).map(|&(_, _, b)| b).collect()
        }
    }

    impl Welt {
        pub(in crate::precipitation) fn new() -> Self {
            Self { bloecke: Default::default(), staende: HashMap::new(), log: Vec::new() }
        }

        pub(in crate::precipitation) fn setze(&mut self, x: i32, y: i32, z: i32) {
            self.bloecke.insert((x, y, z));
            self.zaehle(x, z, Some((x, y, z)));
        }

        fn entferne(&mut self, x: i32, y: i32, z: i32) {
            self.bloecke.remove(&(x, y, z));
            self.zaehle(x, z, Some((x, y, z)));
        }

        fn zaehle(&mut self, x: i32, z: i32, block: Option<(i32, i32, i32)>) {
            let c = (x.div_euclid(16), z.div_euclid(16));
            let stand = self.staende.entry(c).or_insert(0);
            *stand += 1;
            self.log.push((c, *stand, block));
        }

        // Ein waagerechtes Dach von x0..x1, z0..z1 (ohne Ende) in Hoehe y.
        pub(in crate::precipitation) fn dach(&mut self, x0: i32, x1: i32, z0: i32, z1: i32, y: i32) {
            for x in x0..x1 {
                for z in z0..z1 {
                    self.setze(x, y, z);
                }
            }
        }

        pub(in crate::precipitation) fn karte(&self, eye: DVec3, radius: i32, karte: &mut SnowMap) {
            let mut tops: HashMap<(i32, i32), i32> = HashMap::new();
            for b in &self.bloecke {
                let t = tops.entry((b.0, b.2)).or_insert(b.1);
                *t = (*t).max(b.1);
            }
            let top = |x: i32, z: i32| tops.get(&(x, z)).copied();
            let solid = |x: i32, y: i32, z: i32| self.bloecke.contains(&(x, y, z)).then_some(1.0);
            let revision = |cx: i32, cz: i32| self.staende.get(&(cx, cz)).copied().unwrap_or(0);
            karte.update(eye, radius, 2 * radius, &top, &solid, &revision);
        }
    }

    // Boden bei y 60 (ohne Boden waere ueber nichts zu sagen, was "frei" heisst; er zaehlt aber
    // als Block und damit selbst als blockiert).
    pub(in crate::precipitation) fn mit_boden(welt: &mut Welt, um: (i32, i32), weite: i32) {
        welt.dach(um.0 - weite, um.0 + weite, um.1 - weite, um.1 + weite, 60);
    }

    // Der Bereich (angenaehert, s. update_region) weicht nur am Rand von dem ab, den das
    // Ablaufen der genauen Linie vom Auge zu jeder Zelle ergibt -- in einer zufaelligen Welt mit
    // Daechern, Platten und Saeulen, an mehreren Augen und Radien, auch nach dem Laufen.
    #[test]
    fn bereich_weicht_nur_am_rand_ab() {
        let mut zufall = 0x5eed_u64;
        let mut wuerfel = |n: i32| {
            zufall = zufall.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ((zufall >> 33) % n as u64) as i32
        };
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 40);
        for _ in 0..60 {
            let (x, z, y, b, t) = (wuerfel(60) - 30, wuerfel(60) - 30, 61 + wuerfel(25), 1 + wuerfel(8), 1 + wuerfel(8));
            welt.dach(x, x + b, z, z + t, y);
        }
        for _ in 0..80 {
            let (x, z, h) = (wuerfel(60) - 30, wuerfel(60) - 30, wuerfel(12));
            for y in 61..61 + h {
                welt.setze(x, y, z);
            }
        }
        for (r, max) in [(10, 20), (6, 16), (10, 12), (4, 24)] {
            let mut karte = SnowMap::new();
            for schritt in 0..4 {
                let eye = DVec3::new(f64::from(schritt) * 1.3 - 2.0, 66.5 + f64::from(schritt), 0.5 + f64::from(schritt) * 0.7);
                let mut tops: HashMap<(i32, i32), i32> = HashMap::new();
                for b in &welt.bloecke {
                    let t = tops.entry((b.0, b.2)).or_insert(b.1);
                    *t = (*t).max(b.1);
                }
                let top = |x: i32, z: i32| tops.get(&(x, z)).copied();
                let solid = |x: i32, y: i32, z: i32| welt.bloecke.contains(&(x, y, z)).then_some(1.0);
                let revision = |cx: i32, cz: i32| welt.staende.get(&(cx, cz)).copied().unwrap_or(0);
                karte.update(eye, r, max, &top, &solid, &revision);
                let genaehert = &karte.region;
                let mut genau = vec![false; genaehert.len()];
                let c = karte.center;
                let auge = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5);
                let (mut drin, mut weit_drin) = (0, 0);
                for dy in -max..=max {
                    for dz in -max..=max {
                        for dx in -max..=max {
                            let (x, y, z) = (c.0 + dx, c.1 + dy, c.2 + dz);
                            let d2 = dx * dx + dy * dy + dz * dz;
                            let erwartet = d2 <= max * max
                                && karte.blocked(x, y, z) == Some(false)
                                && (d2 <= r * r || karte.open_length(auge, auge + DVec3::new(f64::from(dx), f64::from(dy), f64::from(dz))) <= f64::from(r));
                            if let Some(i) = karte.index(x, y, z) {
                                genau[i] = erwartet;
                            }
                            drin += usize::from(erwartet);
                            weit_drin += usize::from(erwartet && d2 > r * r);
                        }
                    }
                }
                assert!(drin > 100 && weit_drin > 0, "r {r} max {max}: {drin} im Bereich, {weit_drin} weiter als r");
                // "tief" = keine
                // der 26 Nachbarzellen hat im genauen Bereich den Zustand, den die Zelle hier hat.
                let n = karte.size();
                let (mut anders, mut tief) = (0, 0);
                for i in 0..genaehert.len() {
                    if genaehert[i] == genau[i] {
                        continue;
                    }
                    anders += 1;
                    let j = i as i32;
                    let (x, y, z) = (j % n, j / (n * n), (j / n) % n);
                    let mut nah = false;
                    for (dx, dy, dz) in (-1..=1).flat_map(|a| (-1..=1).flat_map(move |b| (-1..=1).map(move |c| (a, b, c)))) {
                        let (a, b, c) = (x + dx, y + dy, z + dz);
                        if a >= 0 && b >= 0 && c >= 0 && a < n && b < n && c < n && genau[((b * n + c) * n + a) as usize] == genaehert[i] {
                            nah = true;
                        }
                    }
                    tief += usize::from(!nah);
                }
                // Gemessen: hier bis 15 % der Zellen anders und keine tief, in der nachgebauten
                // echten Welt etwa 6 % anders und davon unter 1 % tief (Messung 2026-11-10).
                assert!(anders * 5 <= drin, "r {r} max {max} schritt {schritt}: {anders} von {drin} Zellen anders als genau");
                assert!(tief * 20 <= anders, "r {r} max {max} schritt {schritt}: {tief} von {anders} abweichenden Zellen nicht am Rand");
            }
        }
    }

    // Nach Aenderungen in der Welt wird nur verworfen, was davon abhaengen kann: die Karte ist
    // danach Zelle fuer Zelle dieselbe wie eine ganz neu berechnete. Einzelne Bloecke (gesetzt und
    // abgebaut, hoch oben, am Boden, in Lagen mit Seitenschritt), ganze Chunk-Spalten, dazwischen
    // laufen.
    #[test]
    fn aenderung_wie_neu_berechnet() {
        let mut zufall = 0xb10c_u64;
        let mut wuerfel = |n: i32| {
            zufall = zufall.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ((zufall >> 33) % n as u64) as i32
        };
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 40);
        for _ in 0..25 {
            let (x, z, y, b, t) = (wuerfel(50) - 25, wuerfel(50) - 25, 61 + wuerfel(22), 1 + wuerfel(7), 1 + wuerfel(7));
            welt.dach(x, x + b, z, z + t, y);
        }
        let (r, max) = (6, 12);
        let mut karte = SnowMap::new();
        let (mut einzeln, mut ganze, mut verschieden, mut nachgefuehrt) = (0, 0, 0, 0);
        // Stellen, die immer wieder gesetzt und abgebaut werden (auch direkt auf dem Boden und
        // der Boden selbst).
        let oft: Vec<(i32, i32, i32)> = (0..14).map(|i| (i * 2 - 9, [61, 60, 64, 68, 72, 76, 61][i as usize % 7], (i * 5) % 9 - 4)).collect();
        for schritt in 0..480 {
            // Mal laufen, mal lange stehen.
            let weg = f64::from(schritt / 40 * 40 + (schritt % 40).min(12));
            let eye = DVec3::new(weg * 0.05 - 12.0, 66.5 + f64::from((schritt % 40).min(12) % 7) * 0.4, 0.5 + weg * 0.02);
            match schritt % 4 {
                // Ein Block irgendwo in Reichweite, gesetzt oder (wenn da) abgebaut.
                0 | 1 => {
                    let p = if wuerfel(2) == 0 { oft[wuerfel(oft.len() as i32) as usize] } else { (eye.x as i32 + wuerfel(30) - 15, 60 + wuerfel(27), eye.z as i32 + wuerfel(30) - 15) };
                    if welt.bloecke.contains(&p) { welt.entferne(p.0, p.1, p.2) } else { welt.setze(p.0, p.1, p.2) }
                    einzeln += 1;
                }
                // Eine ganze Chunk-Spalte: viele Bloecke anders, gemeldet nur als "die Spalte".
                2 if schritt % 12 == 2 => {
                    let c = ((eye.x as i32 + wuerfel(30) - 15).div_euclid(16), (eye.z as i32 + wuerfel(30) - 15).div_euclid(16));
                    for _ in 0..40 {
                        let p = (c.0 * 16 + wuerfel(16), 61 + wuerfel(26), c.1 * 16 + wuerfel(16));
                        if !welt.bloecke.remove(&p) {
                            welt.bloecke.insert(p);
                        }
                    }
                    welt.zaehle(c.0 * 16, c.1 * 16, None);
                    ganze += 1;
                }
                _ => {}
            }
            let mut tops: HashMap<(i32, i32), i32> = HashMap::new();
            for b in &welt.bloecke {
                let t = tops.entry((b.0, b.2)).or_insert(b.1);
                *t = (*t).max(b.1);
            }
            let top = |x: i32, z: i32| tops.get(&(x, z)).copied();
            let solid = |x: i32, y: i32, z: i32| welt.bloecke.contains(&(x, y, z)).then_some(1.0);
            let vorher = karte.cells.clone();
            karte.update(eye, r, max, &top, &solid, &MitLog(&welt));
            verschieden += usize::from(vorher.len() == karte.cells.len() && vorher != karte.cells);
            nachgefuehrt += usize::from(karte.diag.iter().any(|d| d.contains("durch blockwechsel")) && !karte.diag.iter().any(|d| d.contains("augenblock")));
            let mut neu = SnowMap::new();
            neu.update(eye, r, max, &top, &solid, &MitLog(&welt));
            let c = karte.center;
            for dy in -max..=max {
                for dz in -max..=max {
                    for dx in -max..=max {
                        let (x, y, z) = (c.0 + dx, c.1 + dy, c.2 + dz);
                        // Ausserhalb der Kugel kennt die neue nichts, die alte noch von vorher.
                        if dx * dx + dy * dy + dz * dz > max * max {
                            continue;
                        }
                        assert_eq!(karte.blocked(x, y, z), neu.blocked(x, y, z), "Schritt {schritt}: frei/blockiert in ({x}, {y}, {z})");
                        assert_eq!(karte.in_region(x, y, z), neu.in_region(x, y, z), "Schritt {schritt}: Bereich in ({x}, {y}, {z})");
                    }
                }
            }
        }
        assert!(einzeln > 200 && ganze > 30 && verschieden > 150 && nachgefuehrt > 80, "{einzeln} Bloecke, {ganze} Spalten, {verschieden} Mal anders, {nachgefuehrt} Mal nachgefuehrt");
    }

    #[test]
    fn im_freien_ist_alles_frei() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 30);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 70.5, 0.5), 10, &mut karte);
        let mut frei = 0;
        for y in 61..=90 {
            for z in -20..=20 {
                for x in -20..=20 {
                    if let Some(b) = karte.blocked(x, y, z) {
                        assert!(!b, "({x}, {y}, {z}) blockiert, obwohl ueber dem Boden nichts ist");
                        frei += 1;
                    }
                }
            }
        }
        assert!(frei > 10_000, "nur {frei} Bloecke berechnet");
        assert_eq!(karte.blocked(0, 60, 0), Some(true), "der Boden selbst ist blockiert");
        assert_eq!(karte.blocked(0, 70, 25), None, "ausserhalb der Kugel nichts");
    }

    // Breites Dach (21 x 21 bei y 80): darunter ein tiefer Keil. Von der Mitte (Abstand 10,5 zur
    // Kante) reichen die schraegen Linien bis 10,5 / 0,268 = 39 Bloecke tief ans Dach.
    #[test]
    fn breites_dach_tiefer_keil() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 60);
        welt.dach(-10, 11, -10, 11, 80);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 70.5, 0.5), 10, &mut karte);
        assert_eq!(karte.blocked(0, 79, 0), Some(true));
        assert_eq!(karte.blocked(0, 65, 0), Some(true), "15 unter der Mitte noch im Keil");
        assert_eq!(karte.blocked(8, 75, 0), Some(true), "nah am Rand, knapp unter dem Dach");
        assert_eq!(karte.blocked(8, 65, 0), Some(false), "nah am Rand, tief unten schneit es");
        assert_eq!(karte.blocked(11, 79, 0), Some(false), "neben dem Dach");
        // Der Keil ist genau die Regel: ein Block ist blockiert, wenn sein Abstand zur
        // naechsten Dachkante (waagerecht) groesser ist als die Neigung mal Hoehe darunter.
        for y in 61..80 {
            for x in -10..=10 {
                let cx = f64::from(x) + 0.5;
                let kante = (cx + 10.0).min(11.0 - cx).min(10.5);
                let tiefe = 80.0 - (f64::from(y) + 0.5);
                let soll = kante > tiefe / f64::from(SIDE_EVERY) + 0.5;
                let sicher_frei = kante < tiefe / f64::from(SIDE_EVERY) - 0.5;
                if let Some(b) = karte.blocked(x, y, 0) {
                    if soll {
                        assert!(b, "({x}, {y}) muss im Keil liegen");
                    }
                    if sicher_frei {
                        assert!(!b, "({x}, {y}) liegt ausserhalb des Keils");
                    }
                }
            }
        }
    }

    // Schmaler Balken (1 x 1): nur bis zur naechsten Lage, in der Schnee zur Seite darf (y 76),
    // sind die Bloecke darunter geschuetzt.
    #[test]
    fn schmaler_balken_flacher_keil() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 30);
        welt.setze(0, 80, 0);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 75.5, 0.5), 10, &mut karte);
        assert_eq!(karte.blocked(0, 79, 0), Some(true));
        assert_eq!(karte.blocked(0, 77, 0), Some(true));
        assert_eq!(karte.blocked(0, 76, 0), Some(false));
        assert_eq!(karte.blocked(1, 79, 0), Some(false), "neben dem Balken");
    }

    // Eine Decke 100 Bloecke ueber dem Auge schuetzt genauso (gesucht wird bis ganz oben).
    // Breit genug, dass auch seitlich kein Schnee darunter kommt (100 / 4 = 25 seitlich).
    #[test]
    fn hohe_decke_blockiert() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 30);
        welt.dach(-40, 41, -40, 41, 170);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 70.5, 0.5), 10, &mut karte);
        assert_eq!(karte.blocked(0, 70, 0), Some(true));
        assert_eq!(karte.blocked(10, 85, 0), Some(true));
    }

    // Laufen: Uebernommenes und neu Berechnetes ergeben dasselbe wie eine frische Karte.
    #[test]
    fn laufen_gleich_wie_neu() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 60);
        welt.dach(-10, 11, -10, 11, 80);
        welt.setze(15, 75, 3);
        let mut karte = SnowMap::new();
        for schritt in 0..12 {
            welt.karte(DVec3::new(f64::from(schritt) * 1.7 - 8.0, 70.5, 0.5), 10, &mut karte);
        }
        let mut neu = SnowMap::new();
        let eye = DVec3::new(11.0 * 1.7 - 8.0, 70.5, 0.5);
        welt.karte(eye, 10, &mut neu);
        assert_eq!(karte.center, neu.center);
        let c = neu.center;
        for y in c.1 - 20..=c.1 + 20 {
            for z in c.2 - 20..=c.2 + 20 {
                for x in c.0 - 20..=c.0 + 20 {
                    if let Some(b) = neu.blocked(x, y, z) {
                        assert_eq!(karte.blocked(x, y, z), Some(b), "({x}, {y}, {z}) nach dem Laufen anders als frisch berechnet");
                    }
                }
            }
        }
    }

    // Ein neuer Block (gemeldet ueber den Stand) wirkt beim naechsten Mal sofort.
    #[test]
    fn neuer_block_wirkt_sofort() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 30);
        let mut karte = SnowMap::new();
        let eye = DVec3::new(0.5, 70.5, 0.5);
        welt.karte(eye, 10, &mut karte);
        assert_eq!(karte.blocked(3, 74, 3), Some(false));
        welt.setze(3, 75, 3);
        welt.karte(eye, 10, &mut karte);
        assert_eq!(karte.blocked(3, 75, 3), Some(true), "der Block selbst");
        assert_eq!(karte.blocked(3, 74, 3), Some(true), "darunter geschuetzt");
    }

    // Im Freien ist der Bereich die Kugel mit Radius r um die Mitte des Augen-Blocks.
    #[test]
    fn bereich_im_freien_ist_kugel() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 30);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.3, 75.8, 0.9), 10, &mut karte);
        let mut anzahl = 0;
        for y in 55..=96 {
            for z in -21..=21 {
                for x in -21..=21 {
                    let d = (f64::from(x * x + (y - 75) * (y - 75) + z * z)).sqrt();
                    let drin = karte.in_region(x, y, z);
                    anzahl += u32::from(drin);
                    if y > 60 && d <= 10.0 {
                        assert!(drin, "({x}, {y}, {z}) fehlt, Abstand {d:.1}");
                    }
                    if d > 10.0 {
                        assert!(!drin, "({x}, {y}, {z}) zu weit, Abstand {d:.1}");
                    }
                }
            }
        }
        // Die Kugel reicht bis y 65, der Boden liegt bei 60: sie ist ganz in der Luft.
        assert_eq!(anzahl, 4169, "Bloecke mit Mitte im Radius 10");
        assert!(!karte.in_region(0, 60, 0), "Boden gehoert nicht dazu");
    }

    // Gleichmaessig verteilte Richtungen (Fibonacci-Kugel).
    fn richtungen(n: usize) -> Vec<DVec3> {
        (0..n)
            .map(|i| {
                let y = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
                let w = i as f64 * std::f64::consts::PI * (3.0 - 5f64.sqrt());
                let s = (1.0 - y * y).sqrt();
                DVec3::new(s * w.cos(), y, s * w.sin())
            })
            .collect()
    }

    // Unter einem Dach (11 x 11 bei y 80, Auge 3 darunter im Keil): auf jeder Linie vom Auge
    // liegen r freie Strecke im Bereich -- wo die Linie ueberhaupt so viel freie Strecke hat,
    // bevor die Karte bei 2r endet. Der Bereich reicht dafuer weiter als r.
    #[test]
    fn bereich_unter_dach_hat_r_freie_strecke() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 40);
        welt.dach(-5, 6, -5, 6, 80);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 77.5, 0.5), 10, &mut karte);
        assert_eq!(karte.blocked(0, 77, 0), Some(true), "das Auge steht im Keil");
        let auge = DVec3::new(0.5, 77.5, 0.5);
        let (mut geprueft, mut schlimmste) = (0, 0.0f64);
        for u in richtungen(2000) {
            let (mut frei, mut im_bereich, mut t) = (0.0, 0.0, 0.0);
            let schritt = 0.01;
            while t < 20.0 {
                let p = auge + u * (t + schritt / 2.0);
                let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
                if karte.blocked(x, y, z) == Some(false) {
                    frei += schritt;
                    if karte.in_region(x, y, z) {
                        im_bereich += schritt;
                    }
                }
                t += schritt;
            }
            if frei < 11.5 {
                continue;
            }
            geprueft += 1;
            schlimmste = schlimmste.max((im_bereich - 10.0f64).abs());
        }
        assert!(geprueft > 500, "nur {geprueft} Linien mit genug freier Strecke");
        // Bis 2 Bloecke Abweichung: jeder Block wird auf der Linie zu SEINER Mitte gemessen; an
        // der Treppenkante des Keils verlassen die Linien benachbarter Bloecke ihn an
        // verschiedenen Stufen (Nutzer 2026-11-07: so lassen).
        assert!(schlimmste <= 2.0, "eine Linie hat {schlimmste:.2} mehr oder weniger als r im Bereich");
        // Nach +x sind x 0..=5 im Keil (5,5 Bloecke blockiert): bis zur Mitte von x 15 liegen
        // 9,5 frei, bis x 16 schon 10,5.
        assert!(karte.in_region(15, 77, 0), "15 Bloecke weit, weil der Keil uebersprungen wird");
        assert!(!karte.in_region(16, 77, 0), "16 ist mehr als r freie Strecke");
    }

    // Ein Dach als Treppe, deren Bloecke sich nur an den Kanten beruehren: dicht. Schnee rutscht
    // nicht zwischen zwei Stufen hindurch, darunter ist alles geschuetzt.
    #[test]
    fn treppendach_ist_dicht() {
        let mut welt = Welt::new();
        for i in -14..=14 {
            for z in -30..=30 {
                welt.setze(i, 85 - i32::abs(i), z);
            }
        }
        for z in -14..=14 {
            for x in -30..=30 {
                welt.setze(x, 85 - i32::abs(z), z);
            }
        }
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 66.5, 0.5), 10, &mut karte);
        let mut frei = Vec::new();
        for x in -4..=4 {
            for z in -4..=4 {
                for y in 62..=74 {
                    if karte.blocked(x, y, z) == Some(false) {
                        frei.push((x, y, z));
                    }
                }
            }
        }
        assert!(frei.is_empty(), "frei unter dem Dach: {} Bloecke, z. B. {:?}", frei.len(), &frei[..frei.len().min(5)]);
    }
}
