// Die Schneeflocken im Bereich (Entwurf des Nutzers; Bereich s. snow_map): jede Flocke ist ein
// eigenes Teilchen in der Welt.
//   - Kommt ein Block neu in den Bereich (am Anfang alle, sonst weil man laeuft oder sich
//     Bloecke aendern), bekommt er sofort Flocken an zufaelligen Stellen in seinem Inneren --
//     wenn ueber ihm die Spalte bis zu ihrem Spawnblock durchgehend im Bereich liegt (unter
//     einem Dach waeren es sonst viel mehr als dort sonst ankommen, s. Nachschub): im
//     Schnitt `per_block` (ausgewuerfelt: der ganzzahlige Teil sicher, der Rest mit seiner
//     Wahrscheinlichkeit).
//   - Gehoert ein Block nicht mehr zum Bereich, verschwinden seine Flocken sofort. Flocken in
//     Bloecken, die drin bleiben, bleiben unveraendert.
//   - Jeden Tick faellt jede Flocke um `fall` Bloecke und wandert dabei zufaellig zur Seite (wie
//     eine Kugel im Plinko, aber weich: die Seitwaertsbewegung aendert sich jeden Tick ein
//     Stueck, nach etwa DRIFT_LENGTH Bloecken Fall hat sie ihre Richtung vergessen). Im Mittel
//     ist eine Flocke nach 4 Bloecken Fall knapp 1 Block seitlich abgekommen, nach 16 etwa 2
//     (Startwert, Nutzer 2026-11-07).
//   - Wuerde die Seitwaertsbewegung sie aus dem Bereich tragen (Block, rote Zone, Rand, Spalte
//     ohne Schnee), faellt sie in diesem Tick senkrecht weiter, wie die Wand beim Plinko.
//   - Eine Explosion stoesst die Flocken weg wie Partikel (s. particles::ExplosionPush, Nutzer
//     2026-11-11): der Stoss kommt zu Fall und Wandern dazu (auch nach oben) und wird je Tick
//     schwaecher (mal PUSH_KEEP, wie Vanillas Partikelreibung). Wuerde er sie aus dem Bereich
//     tragen, gilt dasselbe wie beim Wandern: senkrecht weiter, Stoss und Wandern sind weg.
//   - Kommt sie beim Fallen aus dem Bereich (auf einen Block, unten hinaus), verschwindet sie;
//     ausserhalb entstehen auch keine. Ueber einem niedrigen Block (Stufe, Teppich) erst an
//     dessen Oberkante (s. SnowMap::holds).
//   - Nachschub: der oberste Block des Bereichs jeder Spalte ist ein Spawnblock (zusammen die
//     nach aussen geschobene Hohlhalbkugel) -- aber nur, wenn der Block darueber nicht
//     blockiert ist. Sonst (an einer Dachkante: unten frei, darueber geschuetzt) kaemen
//     Flocken unter dem Dach aus dem Nichts; dorthin wandern sie nur. Jeder bekommt je Tick im Mittel `per_block` mal
//     `fall` neue Flocken -- so viele, wie bei dieser Dichte unten aus ihm hinausfallen --, an
//     einer zufaelligen Stelle in der obersten Scheibe des Blocks, die in einem Tick
//     durchfallen wird.

#[cfg(test)]
use std::collections::HashSet;

use glam::DVec3;

use super::snow_map::SnowMap;
use super::Rng;

// Seitwaertsbewegung je Block Fall (Streuung je Achse, wenn sie sich eingependelt hat) und nach
// wie vielen Bloecken Fall sie ihre Richtung weitgehend vergessen hat.
const DRIFT: f64 = 0.25;
const DRIFT_LENGTH: f64 = 1.0;
// So viel eines Explosionsstosses bleibt je Tick (Vanillas Partikelreibung 0,98), und darunter
// (Bloecke je Tick) ist er ganz weg.
const PUSH_KEEP: f64 = 0.98;
const PUSH_MIN: f64 = 1e-3;

pub(super) struct SnowFlake {
    pub(super) pos: DVec3,
    // Wo sie im vorigen Tick war (zum Zeichnen zwischen zwei Ticks).
    pub(super) prev: DVec3,
    // Fortlaufende Nummer, an der man dieselbe Flocke wiedererkennt.
    pub(super) id: u64,
    // Seitwaertsbewegung (x, z) in Bloecken je Block Fall.
    drift: [f64; 2],
    // Stoss einer Explosion in Bloecken je Tick (s. explode), meist null.
    push: DVec3,
}

pub(super) struct SnowFlakes {
    pub(super) flakes: Vec<SnowFlake>,
    // Der Bereich beim letzten Abgleich (Tabelle wie SnowMap::region_table) und dessen Stand
    // (s. SnowMap::region_version).
    alt: Vec<bool>,
    alt_mitte: (i32, i32, i32),
    alt_halb: i32,
    // Arbeitsspeicher des Abgleichs: ein Wert je Spalte.
    oben: Vec<i32>,
    // Nur fuer den Vergleich im Test (s. update_mengen).
    #[cfg(test)]
    blocks: HashSet<(i32, i32, i32)>,
    // Die Spawnbloecke dazu (oberster Block des Bereichs je Spalte).
    pub(super) spawn: Vec<(i32, i32, i32)>,
    version: Option<u64>,
    next_id: u64,
    rng: Rng,
    // DIAG Schnee: Tick-Zaehler fuer die Sekunden-Zeile.
    diag_tick: u64,
    // DIAG Zeit: abgleichen, bewegen, entfernen, Nachschub, Diag-Zeile.
    pub(super) diag_zeit: [std::time::Duration; 5],
}

// Ob eine Flocke hier sein darf.
fn holds(map: &SnowMap, p: DVec3, snow_column: &impl Fn(i32, i32) -> bool) -> bool {
    map.holds(p) && snow_column(p.x.floor() as i32, p.z.floor() as i32)
}

#[cfg(test)]
fn block_of(p: DVec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

impl SnowFlakes {
    pub(super) fn new() -> Self {
        Self { flakes: Vec::new(), alt: Vec::new(), alt_mitte: (0, 0, 0), alt_halb: -1, oben: Vec::new(), #[cfg(test)] blocks: HashSet::new(), spawn: Vec::new(), version: None, next_id: 0, rng: Rng(0x534e_4f57_464c_414b), diag_tick: 0, diag_zeit: Default::default() }
    }

    // Ein Tick (s. oben): mit dem Bereich abgleichen, fallen und wandern, was hinaus ist weg,
    // Nachschub oben.
    pub(super) fn tick(&mut self, map: &SnowMap, per_block: f64, fall: f64, snow_column: &impl Fn(i32, i32) -> bool) {
        let t = std::time::Instant::now();
        self.update(map, per_block, snow_column);
        self.diag_zeit[0] += t.elapsed();
        let t = std::time::Instant::now();
        self.move_flakes(map, fall, snow_column);
        self.diag_zeit[1] += t.elapsed();
        let t = std::time::Instant::now();
        let raus = self.drop_outside(map, snow_column);
        self.diag_zeit[2] += t.elapsed();
        let t = std::time::Instant::now();
        self.spawn_top(per_block, fall, snow_column);
        self.diag_zeit[3] += t.elapsed();
        let t = std::time::Instant::now();
        // DIAG Schnee: einmal je Sekunde.
        self.diag_tick += 1;
        if cfg!(test) && self.diag_tick.is_multiple_of(20) {
            let unter_block = self.spawn.iter().filter(|&&(x, y, z)| map.blocked(x, y + 1, z) == Some(true)).count();
            let ohne_bereich_darunter = self.spawn.iter().filter(|&&(x, y, z)| !map.in_region(x, y - 1, z)).count();
            eprintln!(
                "[schnee-diag] sekunde: {} flocken, {} spawnbloecke, davon {unter_block} mit blockiertem block drueber, {ohne_bereich_darunter} ohne bereich darunter; diesen tick {raus} rausgefallen",
                self.flakes.len(),
                self.spawn.len()
            );
        }
        self.diag_zeit[4] += t.elapsed();
    }

    // Etwa normalverteilt, Mittel 0, Streuung 1 (Summe dreier gleichverteilter Zahlen).
    fn gauss(&mut self) -> f64 {
        (f64::from(self.rng.next()) + f64::from(self.rng.next()) + f64::from(self.rng.next()) - 1.5) * 2.0
    }

    fn new_flake(&mut self, pos: DVec3) {
        let drift = [DRIFT * self.gauss(), DRIFT * self.gauss()];
        self.flakes.push(SnowFlake { pos, prev: pos, id: self.next_id, drift, push: DVec3::ZERO });
        self.next_id += 1;
    }

    fn move_flakes(&mut self, map: &SnowMap, fall: f64, snow_column: &impl Fn(i32, i32) -> bool) {
        let halten = (-fall / DRIFT_LENGTH).exp();
        let neu = (1.0 - halten * halten).sqrt() * DRIFT;
        let mut flakes = std::mem::take(&mut self.flakes);
        for f in &mut flakes {
            f.prev = f.pos;
            for a in 0..2 {
                f.drift[a] = f.drift[a] * halten + neu * self.gauss();
            }
            let ziel = f.pos + DVec3::new(f.drift[0] * fall, -fall, f.drift[1] * fall) + f.push;
            if holds(map, ziel, snow_column) {
                f.pos = ziel;
                f.push *= PUSH_KEEP;
                if f.push.length_squared() < PUSH_MIN * PUSH_MIN {
                    f.push = DVec3::ZERO;
                }
            } else {
                f.pos.y -= fall;
                f.drift = [0.0; 2];
                f.push = DVec3::ZERO;
            }
        }
        self.flakes = flakes;
    }

    // Explosion: jede Flocke bekommt den Stoss, den `push` fuer ihre Stelle liefert (None =
    // ausser Reichweite oder abgeschirmt), zu ihrem bisherigen dazu.
    pub(super) fn explode(&mut self, mut push: impl FnMut(DVec3) -> Option<DVec3>) {
        for f in &mut self.flakes {
            if let Some(p) = push(f.pos) {
                f.push += p;
            }
        }
    }

    // Flocken entfernen, die nicht mehr sein duerfen; gibt zurueck, wie viele.
    fn drop_outside(&mut self, map: &SnowMap, snow_column: &impl Fn(i32, i32) -> bool) -> usize {
        let vorher = self.flakes.len();
        self.flakes.retain(|f| holds(map, f.pos, snow_column));
        vorher - self.flakes.len()
    }

    fn spawn_top(&mut self, per_block: f64, fall: f64, snow_column: &impl Fn(i32, i32) -> bool) {
        let erwartet = per_block * fall;
        let ganz = erwartet.floor();
        let rest = (erwartet - ganz) as f32;
        for i in 0..self.spawn.len() {
            let (x, y, z) = self.spawn[i];
            if !snow_column(x, z) {
                continue;
            }
            let anzahl = ganz as u32 + u32::from(self.rng.next() < rest);
            for _ in 0..anzahl {
                // In der obersten Scheibe der Dicke `fall`, unterhalb der Oberkante.
                let pos = DVec3::new(
                    f64::from(x) + f64::from(self.rng.next()),
                    f64::from(y) + 1.0 - fall.min(1.0) * (1.0 - f64::from(self.rng.next())),
                    f64::from(z) + f64::from(self.rng.next()),
                );
                self.new_flake(pos);
            }
        }
    }

    // Mit dem Bereich der Karte abgleichen (s. oben); nur, wenn er sich geaendert hat. Arbeitet
    // auf der Tabelle der Karte und der gemerkten vom letzten Mal (keine Mengen von Bloecken).
    fn update(&mut self, map: &SnowMap, per_block: f64, snow_column: &impl Fn(i32, i32) -> bool) {
        if self.version == Some(map.region_version()) {
            return;
        }
        self.version = Some(map.region_version());
        let vorher = self.flakes.len();
        self.flakes.retain(|f| map.holds(f.pos));
        let weg_flocken = vorher - self.flakes.len();
        let vorher_id = self.next_id;
        let (tabelle, mitte, halb) = map.region_table();
        let n = 2 * halb + 1;
        let (alt, alt_mitte, alt_halb) = (&self.alt, self.alt_mitte, self.alt_halb);
        let alt_n = 2 * alt_halb + 1;
        let war_drin = |x: i32, y: i32, z: i32| {
            let (dx, dy, dz) = (x - alt_mitte.0 + alt_halb, y - alt_mitte.1 + alt_halb, z - alt_mitte.2 + alt_halb);
            alt_halb >= 0 && (0..alt_n).contains(&dx) && (0..alt_n).contains(&dy) && (0..alt_n).contains(&dz) && alt[((dy * alt_n + dz) * alt_n + dx) as usize]
        };
        // Ein Durchgang von unten nach oben: der oberste Block des Bereichs je Spalte, die neu
        // dazugekommenen Bloecke und die Zahl aller.
        let mut oben = std::mem::take(&mut self.oben);
        oben.clear();
        oben.resize((n * n).max(0) as usize, i32::MIN);
        let mut dazu: Vec<(i32, i32, i32)> = Vec::new();
        let mut anzahl = 0usize;
        let mut i = 0usize;
        for dy in 0..n {
            let y = mitte.1 - halb + dy;
            for dz in 0..n {
                let z = mitte.2 - halb + dz;
                for dx in 0..n {
                    if tabelle[i] {
                        let x = mitte.0 - halb + dx;
                        oben[(dz * n + dx) as usize] = y;
                        anzahl += 1;
                        if !war_drin(x, y, z) {
                            dazu.push((x, y, z));
                        }
                    }
                    i += 1;
                }
            }
        }
        // Nur mit Himmel darueber: ist der Block darueber blockiert (fest oder geschuetzt), kein Nachschub.
        self.spawn.clear();
        for dz in 0..n {
            for dx in 0..n {
                let y = oben[(dz * n + dx) as usize];
                let (x, z) = (mitte.0 - halb + dx, mitte.2 - halb + dz);
                if y != i32::MIN && map.blocked(x, y + 1, z) != Some(true) {
                    self.spawn.push((x, y, z));
                }
            }
        }
        self.spawn.sort_unstable();
        // Je Spalte mit Spawnblock: bis wie weit hinunter der Bereich vom Spawnblock aus
        // durchgehend reicht (in `oben`, sonst i32::MAX). Nur dort fallen Flocken von oben direkt
        // hinein; nur dort bekommt ein neuer Block sofort Flocken (unter einem Dach kommen sie nur
        // durch Wandern hin).
        oben.fill(i32::MAX);
        for &(x, y, z) in &self.spawn {
            let mut unten = y;
            while map.in_region(x, unten - 1, z) {
                unten -= 1;
            }
            oben[((z - mitte.2 + halb) * n + x - mitte.0 + halb) as usize] = unten;
        }
        let ganz = per_block.floor();
        let rest = (per_block - ganz) as f32;
        // Sortiert, damit derselbe Ablauf dieselben Flocken ergibt.
        dazu.sort_unstable();
        let dazu_bloecke = dazu.len();
        for (x, y, z) in dazu {
            if !snow_column(x, z) || y < oben[((z - mitte.2 + halb) * n + x - mitte.0 + halb) as usize] {
                continue;
            }
            let anzahl = ganz as u32 + u32::from(self.rng.next() < rest);
            for _ in 0..anzahl {
                let pos = DVec3::new(
                    f64::from(x) + f64::from(self.rng.next()),
                    f64::from(y) + f64::from(self.rng.next()),
                    f64::from(z) + f64::from(self.rng.next()),
                );
                self.new_flake(pos);
            }
        }
        self.oben = oben;
        // Messausgabe nur in den Tests (im Spiel kein Log).
        if cfg!(test) {
        eprintln!(
            "[schnee-diag] bereich v{}: {:?}; bloecke +{dazu_bloecke} (jetzt {anzahl}), flocken +{} -{weg_flocken}",
            map.region_version(),
            map.diag,
            self.next_id - vorher_id
        );
        }
        self.alt.clear();
        self.alt.extend_from_slice(tabelle);
        self.alt_mitte = mitte;
        self.alt_halb = halb;
    }

    // Der Abgleich ueber Mengen (erste Fassung), nur noch als Vergleich fuer den Test.
    #[cfg(test)]
    fn update_mengen(&mut self, map: &SnowMap, per_block: f64, snow_column: &impl Fn(i32, i32) -> bool) {
        if self.version == Some(map.region_version()) {
            return;
        }
        self.version = Some(map.region_version());
        let neu: HashSet<(i32, i32, i32)> = map.region_blocks().collect();
        self.flakes.retain(|f| map.holds(f.pos));
        let mut oben: std::collections::HashMap<(i32, i32), i32> = std::collections::HashMap::new();
        for &(x, y, z) in &neu {
            let o = oben.entry((x, z)).or_insert(y);
            *o = (*o).max(y);
        }
        // Nur mit Himmel darueber: ist der Block darueber blockiert (fest oder geschuetzt), kein Nachschub.
        self.spawn = oben.into_iter().map(|((x, z), y)| (x, y, z)).filter(|&(x, y, z)| map.blocked(x, y + 1, z) != Some(true)).collect();
        self.spawn.sort_unstable();
        // Je Spalte mit Spawnblock: bis wie weit hinunter der Bereich vom Spawnblock aus
        // durchgehend reicht. Nur dort fallen Flocken von oben direkt hinein; nur dort bekommt ein
        // neuer Block sofort Flocken (unter einem Dach kommen sie nur durch Wandern hin).
        let mut himmel_ab: std::collections::HashMap<(i32, i32), i32> = std::collections::HashMap::new();
        for &(x, y, z) in &self.spawn {
            let mut unten = y;
            while neu.contains(&(x, unten - 1, z)) {
                unten -= 1;
            }
            himmel_ab.insert((x, z), unten);
        }
        let ganz = per_block.floor();
        let rest = (per_block - ganz) as f32;
        // Sortiert, damit derselbe Ablauf dieselben Flocken ergibt.
        let mut dazu: Vec<(i32, i32, i32)> = neu.difference(&self.blocks).copied().collect();
        dazu.sort_unstable();
        for (x, y, z) in dazu {
            if !snow_column(x, z) || himmel_ab.get(&(x, z)).is_none_or(|&ab| y < ab) {
                continue;
            }
            let anzahl = ganz as u32 + u32::from(self.rng.next() < rest);
            for _ in 0..anzahl {
                let pos = DVec3::new(
                    f64::from(x) + f64::from(self.rng.next()),
                    f64::from(y) + f64::from(self.rng.next()),
                    f64::from(z) + f64::from(self.rng.next()),
                );
                self.new_flake(pos);
            }
        }
        self.blocks = neu;
    }
}

#[cfg(test)]
mod tests {
    use super::super::snow_map::tests::{mit_boden, Welt};
    use super::*;

    const JE_BLOCK: f64 = 0.36;

    // Ueberall Schnee-Biom.
    fn ueberall(_: i32, _: i32) -> bool {
        true
    }

    // Der Abgleich auf den Tabellen ergibt dasselbe wie der ueber Mengen: beim Laufen unter
    // Daechern durch, auch hoch und runter und mit wechselndem Radius, nach jedem Tick dieselben
    // Spawnbloecke und dieselben Flocken an denselben Stellen.
    #[test]
    fn abgleich_wie_mit_mengen() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 60);
        welt.dach(-8, 9, -8, 9, 80);
        welt.dach(10, 30, -3, 4, 74);
        welt.dach(-30, -12, -20, 20, 90);
        for y in 61..78 {
            welt.setze(3, y, 2);
            welt.setze(-14, y, -5);
        }
        let mut karte = SnowMap::new();
        let (mut a, mut b) = (SnowFlakes::new(), SnowFlakes::new());
        let (mut abgleiche, mut flocken_gesehen) = (0, 0);
        for tick in 0..400 {
            let t = f64::from(tick);
            let eye = DVec3::new(-35.0 + t * 0.19, 72.5 + (t * 0.11).sin() * 4.0, 0.5 + (t * 0.07).sin() * 6.0);
            let radius = if (150..250).contains(&tick) { 6 } else { 10 };
            welt.karte(eye, radius, &mut karte);
            abgleiche += usize::from(a.version != Some(karte.region_version()));
            a.update(&karte, JE_BLOCK, &ueberall);
            b.update_mengen(&karte, JE_BLOCK, &ueberall);
            for f in [&mut a, &mut b] {
                f.move_flakes(&karte, 0.05, &ueberall);
                f.drop_outside(&karte, &ueberall);
                f.spawn_top(JE_BLOCK, 0.05, &ueberall);
            }
            assert_eq!(a.spawn, b.spawn, "Spawnbloecke, Tick {tick}");
            assert_eq!(a.flakes.len(), b.flakes.len(), "Zahl der Flocken, Tick {tick}");
            for (x, y) in a.flakes.iter().zip(&b.flakes) {
                assert!(x.id == y.id && x.pos == y.pos && x.prev == y.prev, "Flocke {} / {}, Tick {tick}", x.id, y.id);
            }
            flocken_gesehen = flocken_gesehen.max(a.flakes.len());
        }
        assert!(abgleiche > 80 && flocken_gesehen > 500, "{abgleiche} Abgleiche, hoechstens {flocken_gesehen} Flocken");
    }

    // Laufen (verschieden schnell, unter einem Dach durch): die Zahl der Flocken bleibt bei
    // JE_BLOCK mal der Zahl der Bloecke mit durchgehender Spalte bis zum Spawnblock, jede Flocke liegt in einem Block des
    // Bereichs, und die Flocken gebliebener Bloecke sind noch dieselben.
    #[test]
    fn laufen_haelt_die_dichte() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 70);
        welt.dach(-8, 9, -8, 9, 80);
        for tempo in [0.1, 0.5, 2.0] {
            let mut karte = SnowMap::new();
            let mut flocken = SnowFlakes::new();
            let mut x = -30.0;
            while x < 30.0 {
                let eye = DVec3::new(x, 76.5, 0.5);
                welt.karte(eye, 10, &mut karte);
                let vorher: std::collections::HashMap<u64, (i32, i32, i32)> = flocken.flakes.iter().map(|f| (f.id, block_of(f.pos))).collect();
                flocken.update(&karte, JE_BLOCK, &ueberall);
                // Nur Bloecke, ueber denen die Spalte bis zum Spawnblock durchgehend im Bereich
                // liegt, bekommen beim Hineinkommen Flocken (unter dem Dach keine).
                let bloecke = flocken
                    .spawn
                    .iter()
                    .map(|&(sx, sy, sz)| (0..).take_while(|&d| karte.in_region(sx, sy - d, sz)).count())
                    .sum::<usize>() as f64;
                let soll = bloecke * JE_BLOCK;
                let ist = flocken.flakes.len() as f64;
                // Jeder Block wuerfelt fuer sich: Streuung sqrt(n * p * (1 - p)), hier etwa 30.
                assert!((ist - soll).abs() < 5.0 * (bloecke * JE_BLOCK * (1.0 - JE_BLOCK)).sqrt(), "Tempo {tempo}, x {x:.1}: {ist} Flocken, erwartet {soll:.0}");
                for f in &flocken.flakes {
                    let b = block_of(f.pos);
                    assert!(karte.in_region(b.0, b.1, b.2), "Flocke in {b:?} ausserhalb des Bereichs");
                }
                let jetzt: HashSet<u64> = flocken.flakes.iter().map(|f| f.id).collect();
                for (id, b) in vorher {
                    if karte.in_region(b.0, b.1, b.2) {
                        assert!(jetzt.contains(&id), "Flocke {id} in {b:?} verschwunden, obwohl der Block im Bereich blieb");
                    }
                }
                x += tempo;
            }
        }
    }

    // Im Mittel genau JE_BLOCK je Block (viele Bloecke, ein Abgleich).
    #[test]
    fn dichte_im_mittel() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 30);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 75.5, 0.5), 10, &mut karte);
        let mut flocken = SnowFlakes::new();
        flocken.update(&karte, 2.36, &ueberall);
        let je_block = flocken.flakes.len() as f64 / karte.region_blocks().count() as f64;
        assert!((je_block - 2.36).abs() < 0.03, "{je_block:.3} je Block statt 2,36");
    }

    // Wandern: nach 4 Bloecken Fall im Mittel knapp 1 Block seitlich, nach 16 etwa 2 (s. oben;
    // Erwartung aus der Rechnung 0,87 und 1,94 als Mittel der Quadrate).
    #[test]
    fn wandern_staerke() {
        // Grosser Bereich ohne Hindernis, damit keine an den Rand kommt.
        let mut karte = SnowMap::new();
        Welt::new().karte(DVec3::new(0.5, -8.5, 0.5), 24, &mut karte);
        let mut flocken = SnowFlakes::new();
        for _ in 0..20_000 {
            flocken.new_flake(DVec3::ZERO);
        }
        let mittel = |f: &SnowFlakes| (f.flakes.iter().map(|f| f.pos.x * f.pos.x + f.pos.z * f.pos.z).sum::<f64>() / f.flakes.len() as f64).sqrt();
        for _ in 0..80 {
            flocken.move_flakes(&karte, 0.05, &ueberall);
        }
        let nach_4 = mittel(&flocken);
        for _ in 80..320 {
            flocken.move_flakes(&karte, 0.05, &ueberall);
        }
        let nach_16 = mittel(&flocken);
        eprintln!("WERT seitlich nach 4 Bloecken {nach_4:.2}, nach 16 {nach_16:.2}");
        assert!((nach_4 - 0.87).abs() < 0.1, "nach 4 Bloecken {nach_4:.2}");
        assert!((nach_16 - 1.94).abs() < 0.15, "nach 16 Bloecken {nach_16:.2}");
    }

    // Explosion: eine Flocke in Reichweite fliegt weg vom Zentrum, der Stoss wird je Tick um
    // PUSH_KEEP schwaecher; hinter einem Block (grobe Deckung, s. particles::ExplosionPush)
    // passiert nichts.
    #[test]
    fn explosion_stoesst_flocken_weg() {
        let mut w = world::World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let sections = (0..24).map(|_| world::ChunkSection::from_states(vec![0u32; 4096])).collect();
                w.insert_column(cx, cz, sections);
            }
        }
        let stein = (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).expect("stone");
        w.set_block(-4, 10, 2, stein);
        let shapes = world::BlockShapes::new(vec![None; world::state_count()]);
        let mut karte = SnowMap::new();
        Welt::new().karte(DVec3::new(0.5, 10.5, 0.5), 24, &mut karte);
        let mut flocken = SnowFlakes::new();
        let frei = DVec3::new(0.5, 10.5, 0.5);
        let gedeckt = DVec3::new(-3.5, 10.5, 4.5);
        flocken.new_flake(frei);
        flocken.new_flake(gedeckt);
        for f in &mut flocken.flakes {
            f.drift = [0.0; 2];
        }
        let mitte = DVec3::new(-3.5, 10.5, 0.5);
        let mut stoss = crate::particles::ExplosionPush::new(mitte, 4.0, &w, &shapes);
        flocken.explode(|p| stoss.at(p));
        assert!((flocken.flakes[0].push.x - 1.0).abs() < 1e-9, "Stoss {:?}", flocken.flakes[0].push);
        assert_eq!(flocken.flakes[1].push, DVec3::ZERO, "hinter dem Stein kein Stoss");
        let mut schritte = Vec::new();
        for _ in 0..3 {
            let vorher = flocken.flakes[0].pos.x;
            flocken.move_flakes(&karte, 0.05, &ueberall);
            schritte.push(flocken.flakes[0].pos.x - vorher);
        }
        assert!(schritte[0] > 0.95 && schritte[1] < schritte[0] && schritte[2] < schritte[1], "Schritte {schritte:?}");
        assert!((schritte[1] / schritte[0] - PUSH_KEEP).abs() < 0.02, "Schritte {schritte:?}");
        assert!((flocken.flakes[1].pos.x - gedeckt.x).abs() < 0.2, "gedeckte Flocke nur gewandert: {:?}", flocken.flakes[1].pos);
    }

    // Im Stehen unter freiem Himmel: nach dem Einschwingen bleibt die Zahl der Flocken ruhig.
    // Gemessen wird, wie weit sie unter JE_BLOCK mal Bereich liegt (am Kugelrand wandern
    // Flocken hinaus, aber keine herein).
    #[test]
    fn stehen_im_freien() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 30);
        let mut karte = SnowMap::new();
        let eye = DVec3::new(0.5, 75.5, 0.5);
        welt.karte(eye, 10, &mut karte);
        let mut flocken = SnowFlakes::new();
        for _ in 0..600 {
            flocken.tick(&karte, JE_BLOCK, 0.05, &ueberall);
        }
        let soll = karte.region_blocks().count() as f64 * JE_BLOCK;
        let (mut summe, mut min, mut max) = (0.0, f64::MAX, 0.0f64);
        for _ in 0..600 {
            flocken.tick(&karte, JE_BLOCK, 0.05, &ueberall);
            let n = flocken.flakes.len() as f64;
            summe += n;
            min = min.min(n);
            max = max.max(n);
        }
        let mittel = summe / 600.0;
        eprintln!("WERT im Freien: Soll {soll:.0}, Mittel {mittel:.0} ({:.1} %), kleinste {min}, groesste {max}", 100.0 * mittel / soll);
        assert!(mittel > 0.8 * soll && mittel < 1.05 * soll, "Mittel {mittel:.0}, Soll {soll:.0}");
        assert!(max - min < 0.1 * soll, "schwankt von {min} bis {max}");
    }

    // Eine Flocke verschwindet nur, wenn sie in einem Block ausserhalb des Bereichs ist.
    #[test]
    fn weg_nur_ausserhalb() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 50);
        welt.dach(-5, 6, -5, 6, 80);
        let mut karte = SnowMap::new();
        let mut flocken = SnowFlakes::new();
        for t in 0..800 {
            let eye = DVec3::new(-12.0 + f64::from(t.min(400)) * 0.03, 77.5, 0.5);
            welt.karte(eye, 10, &mut karte);
            flocken.update(&karte, JE_BLOCK, &ueberall);
            flocken.move_flakes(&karte, 0.05, &ueberall);
            let draussen = flocken.flakes.iter().filter(|f| {
                let b = block_of(f.pos);
                !karte.in_region(b.0, b.1, b.2)
            });
            let erwartet = draussen.count();
            assert_eq!(flocken.drop_outside(&karte, &ueberall), erwartet);
            flocken.spawn_top(JE_BLOCK, 0.05, &ueberall);
        }
    }

    // Unter dem Dach (11 x 11 bei y 80, Auge 3 darunter): in die Bloecke unter der Dachkante, in
    // die senkrecht nie eine Flocke fiele (im Bild des Nutzers pink), koennen durch das Wandern
    // Flocken kommen -- voll muessen sie nicht werden (Nutzer 2026-11-07). Gemessen je Abstand
    // zur Dachkante.
    #[test]
    fn unter_dem_dach_wandern_flocken_hinein() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 50);
        welt.dach(-5, 6, -5, 6, 80);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 77.5, 0.5), 10, &mut karte);
        // Pink: Bereich unter dem Dach (x und z in -5..=5, y < 80).
        let pink: Vec<(i32, i32, i32)> = karte.region_blocks().filter(|b| (-5..=5).contains(&b.0) && (-5..=5).contains(&b.2) && b.1 < 80).collect();
        assert!(!pink.is_empty());
        let mut flocken = SnowFlakes::new();
        for _ in 0..800 {
            flocken.tick(&karte, JE_BLOCK, 0.05, &ueberall);
        }
        // Abstand zur Dachkante (waagerecht) -> (Flocken summiert ueber die Messung, Bloecke).
        let tiefe = |b: (i32, i32, i32)| (b.0 + 6).min(6 - b.0).min(b.2 + 6).min(6 - b.2);
        let mut je_tiefe: std::collections::BTreeMap<i32, (f64, usize)> = std::collections::BTreeMap::new();
        for &b in &pink {
            je_tiefe.entry(tiefe(b)).or_insert((0.0, 0)).1 += 1;
        }
        let pink_set: HashSet<(i32, i32, i32)> = pink.iter().copied().collect();
        for _ in 0..800 {
            flocken.tick(&karte, JE_BLOCK, 0.05, &ueberall);
            for f in &flocken.flakes {
                let b = block_of(f.pos);
                if pink_set.contains(&b) {
                    je_tiefe.get_mut(&tiefe(b)).unwrap().0 += 1.0 / 800.0;
                }
            }
        }
        for (t, (n, bloecke)) in &je_tiefe {
            eprintln!("WERT unter dem Dach, {t} Bloecke hinter der Kante: {bloecke} Bloecke, {:.3} Flocken je Block (Soll {JE_BLOCK})", n / *bloecke as f64);
        }
        let (n, bloecke) = je_tiefe[&1];
        assert!(n / bloecke as f64 > 0.0, "direkt hinter der Kante kommt keine Flocke an");
    }

    // An einer Dachkante kommt Schnee von der Seite in einen Block, ueber dem es geschuetzt ist.
    // Der freie Block ist dann der oberste des Bereichs seiner Spalte, hat aber keinen Himmel
    // ueber sich -- dort kein Nachschub.
    #[test]
    fn kein_spawnblock_unter_dachkante() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 50);
        // Dickes Vordach bis x = 4, so hoch, dass ueber ihm nichts mehr im Bereich liegt.
        for y in 90..=101 {
            welt.dach(-30, 5, -30, 31, y);
        }
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 80.5, 0.5), 10, &mut karte);
        assert_eq!(karte.blocked(4, 88, 0), Some(false), "unten an der Kante frei");
        assert_eq!(karte.blocked(4, 89, 0), Some(true), "darueber geschuetzt");
        let mut flocken = SnowFlakes::new();
        flocken.tick(&karte, JE_BLOCK, 0.05, &ueberall);
        let unter: Vec<_> = flocken.spawn.iter().filter(|&&(x, y, z)| karte.blocked(x, y + 1, z) == Some(true)).collect();
        assert!(unter.is_empty(), "Spawnbloecke mit blockiertem Block darueber: {unter:?}");
    }

    // Beim Hineinkommen bekommen Bloecke unter einem Dach keine Flocken (dort kaemen sonst viel
    // mehr als durch Wandern); im Freien weiter alle.
    #[test]
    fn neue_bloecke_unter_dem_dach_ohne_flocken() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 50);
        welt.dach(-5, 6, -5, 6, 80);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 77.5, 0.5), 10, &mut karte);
        let mut flocken = SnowFlakes::new();
        flocken.tick(&karte, JE_BLOCK, 0.05, &ueberall);
        // Wo sie entstanden sind (vor dem ersten Fall).
        let unter_dach = flocken.flakes.iter().filter(|f| {
            let b = block_of(f.prev);
            (-5..=5).contains(&b.0) && (-5..=5).contains(&b.2) && b.1 < 80
        });
        assert_eq!(unter_dach.count(), 0);
        assert!(flocken.flakes.len() > 500, "im Freien drumherum entstehen weiter welche: {}", flocken.flakes.len());
    }

    // Am Rand des Bereichs verschwindet keine Flocke seitwaerts: im Freien nur unten hinaus (und
    // alle, die drin sind, bleiben drin).
    #[test]
    fn seitlich_bleibt_drin() {
        let mut welt = Welt::new();
        mit_boden(&mut welt, (0, 0), 40);
        let mut karte = SnowMap::new();
        welt.karte(DVec3::new(0.5, 90.5, 0.5), 10, &mut karte);
        let mut flocken = SnowFlakes::new();
        for _ in 0..400 {
            flocken.tick(&karte, JE_BLOCK, 0.05, &ueberall);
        }
        for _ in 0..400 {
            flocken.update(&karte, JE_BLOCK, &ueberall);
            flocken.move_flakes(&karte, 0.05, &ueberall);
            for f in &flocken.flakes {
                if !karte.holds(f.pos) {
                    let oben = DVec3::new(f.pos.x, f.prev.y, f.pos.z);
                    assert!(karte.holds(oben), "Flocke {:?} ist seitlich hinausgekommen, von {:?}", f.pos, f.prev);
                }
            }
            flocken.drop_outside(&karte, &ueberall);
            flocken.spawn_top(JE_BLOCK, 0.05, &ueberall);
        }
    }

    // Ueber einer Stufe (halbe Hoehe) faellt die Flocke bis zur Oberkante, nicht nur bis zum Feld.
    #[test]
    fn stufe_haelt_an_der_oberkante() {
        let top = |_: i32, _: i32| Some(60);
        let solid = |_: i32, y: i32, _: i32| (y <= 59).then_some(1.0).or((y == 60).then_some(0.5));
        let revision = |_: i32, _: i32| 0u64;
        let mut karte = SnowMap::new();
        karte.update(DVec3::new(0.5, 65.5, 0.5), 10, 20, &top, &solid, &revision);
        assert!(karte.holds(DVec3::new(0.5, 60.6, 0.5)), "ueber der Oberkante darf sie noch");
        assert!(!karte.holds(DVec3::new(0.5, 60.4, 0.5)), "darunter nicht mehr");
        assert!(!karte.holds(DVec3::new(0.5, 59.5, 0.5)), "ganzer Block");
    }

    #[test]
    #[ignore]
    fn diag_echte_welt() {
        use std::collections::HashMap;
        let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share/coral-client/chunk-saver/mcpvp.club/minecraft_overworld_bd07dbbc4a3089f4");
        let auge = DVec3::new(48.074, 102.620, -15.814);
        let store = chunk_saver::store::Store::open(&dir).unwrap();
        let mut knoten: HashMap<(i32, i32, i32), Option<chunk_saver::node::Node>> = HashMap::new();
        let mut bloecke: HashMap<(i32, i32, i32), u32> = HashMap::new();
        let (cx, cz) = (auge.x as i32, auge.z as i32);
        for x in cx - 60..cx + 60 { for z in cz - 60..cz + 60 { for y in 60i32..200 {
            let k = (x.div_euclid(32), y.div_euclid(32), z.div_euclid(32));
            let n = knoten.entry(k).or_insert_with(|| store.read(chunk_saver::node::NodeKey::new(0, k.0, k.1, k.2)).and_then(|b| chunk_saver::node::Node::decode(&b)));
            let st = n.as_ref().map_or(0, |n| (n.get(x.rem_euclid(32) as usize, y.rem_euclid(32) as usize, z.rem_euclid(32) as usize) & 0xFFFF) as u32);
            if st != 0 { bloecke.insert((x, y, z), st); }
        }}}
        let mut tops: HashMap<(i32, i32), i32> = HashMap::new();
        for (&(x, y, z), &st) in &bloecke {
            if world::weather::snow_stop(st).is_some() { let t = tops.entry((x, z)).or_insert(y); *t = (*t).max(y); }
        }
        let top = |x: i32, z: i32| tops.get(&(x, z)).copied();
        let solid = |x: i32, y: i32, z: i32| bloecke.get(&(x, y, z)).and_then(|&st| world::weather::snow_stop(st));
        let revision = |_: i32, _: i32| 0u64;
        // DIAG Blockwechsel: was kostet alles neu, was ein gesetzter Block (Radius 12, max 32).
        {
            struct Gesetzt<'a>(&'a std::cell::Cell<Option<(i32, i32, i32)>>, &'a std::cell::Cell<u64>);
            impl super::super::snow_map::SnowChanges for Gesetzt<'_> {
                fn revision(&self, cx: i32, cz: i32) -> u64 {
                    if self.0.get().is_some_and(|b| (b.0.div_euclid(16), b.2.div_euclid(16)) == (cx, cz)) { self.1.get() } else { 0 }
                }
                fn since(&self, _: i32, _: i32, _: u64) -> Option<Vec<(i32, i32, i32)>> {
                    self.0.get().map(|b| vec![b])
                }
            }
            let (block, stand) = (std::cell::Cell::new(None::<(i32, i32, i32)>), std::cell::Cell::new(0u64));
            let mit = |b: Option<(i32, i32, i32)>, x: i32, y: i32, z: i32| b == Some((x, y, z));
            let top2 = |x: i32, z: i32| [top(x, z), block.get().filter(|b| (b.0, b.2) == (x, z)).map(|b| b.1)].into_iter().flatten().max();
            let solid2 = |x: i32, y: i32, z: i32| if mit(block.get(), x, y, z) { Some(1.0) } else { solid(x, y, z) };
            let auge = DVec3::new(40.5, 102.62, -12.5);
            let mut karte = SnowMap::new();
            karte.update(auge, 12, 32, &top2, &solid2, &Gesetzt(&block, &stand));
            karte.diag_zeit = Default::default();
            // Alles neu: eine frische Karte (mit Aufbau der Tabelle fuer den Bereich).
            let mut frisch = SnowMap::new();
            frisch.update(auge, 12, 32, &top2, &solid2, &Gesetzt(&block, &stand));
            eprintln!("DIAG blockwechsel: alles neu (max 32): fuellen {:?}, bereich mit Aufbau der Tabelle {:?}", frisch.diag_zeit[0], frisch.diag_zeit[1]);
            for (name, b) in [("am Boden vor dir", (42, 101, -12)), ("3 hoch", (42, 104, -12)), ("12 hoch", (43, 113, -10)), ("25 hoch", (44, 126, -8))] {
                block.set(Some(b));
                stand.set(stand.get() + 1);
                karte.diag_zeit = Default::default();
                let t = std::time::Instant::now();
                karte.update(auge, 12, 32, &top2, &solid2, &Gesetzt(&block, &stand));
                eprintln!("DIAG blockwechsel: Block {name}: gesamt {:?} (fuellen {:?}, bereich {:?}); {:?}", t.elapsed(), karte.diag_zeit[0], karte.diag_zeit[1], karte.diag);
            }
        }
        // DIAG Chunk geladen: Stand einer Chunk-Spalte zaehlt hoch, ohne Angabe von Bloecken.
        {
            let stand = std::cell::Cell::new(0u64);
            let rev = |cx: i32, cz: i32| if (cx, cz) == (2, -1) { stand.get() } else { 0 };
            let auge = DVec3::new(40.5, 102.62, -12.5);
            let mut karte = SnowMap::new();
            karte.update(auge, 12, 32, &top, &solid, &rev);
            stand.set(1);
            karte.diag_zeit = Default::default();
            let t = std::time::Instant::now();
            karte.update(auge, 12, 32, &top, &solid, &rev);
            eprintln!("DIAG chunk ganz: gesamt {:?} (fuellen {:?}, bereich {:?}); {:?}", t.elapsed(), karte.diag_zeit[0], karte.diag_zeit[1], karte.diag);
        }
        // DIAG Laufen: 30 Bloecke (Weg aus der Aufnahme), Zeit von Bereich und Fuellen je Schritt.
        for max in [20, 32] {
            let mut karte = SnowMap::new();
            karte.update(DVec3::new(58.5, 102.62, -15.8), 10, max, &top, &solid, &revision);
            karte.diag_zeit = Default::default();
            for schritt in 1..=30 {
                karte.update(DVec3::new(58.5 - f64::from(schritt), 102.62, -15.8 + f64::from(schritt) * 0.3), 10, max, &top, &solid, &revision);
            }
            eprintln!("DIAG laufen max {max}: bereich je schritt {:?}, fuellen je schritt {:?}", karte.diag_zeit[1] / 30, karte.diag_zeit[0] / 30);
        }
        for (name, auge) in [("pavillon", auge), ("spawn", DVec3::new(0.5, 103.62, 16.5))] {
            let a = (auge.x.floor() as i32, auge.y.floor() as i32, auge.z.floor() as i32);
            let mut karte = SnowMap::new();
            karte.update(auge, 8, 16, &top, &solid, &revision);
            let mut f = SnowFlakes::new();
            for _ in 0..1000 { f.tick(&karte, JE_BLOCK, 0.05, &ueberall); }
            let (mut fest, mut rot, mut rand) = (0u32, 0u32, 0u32);
            let (mut nah_rand, mut rand_seite, mut rand_unten) = (0u32, 0u32, 0u32);
            let mut geboren: HashMap<u64, u32> = f.flakes.iter().map(|x| (x.id, 0)).collect();
            let mut lebensdauer = [0u32; 6];
            let n = 2000;
            let mut arten: HashMap<&str, u32> = HashMap::new();
            for tick in 0..n {
                f.update(&karte, JE_BLOCK, &ueberall);
                f.move_flakes(&karte, 0.05, &ueberall);
                for x in &f.flakes {
                    let b = block_of(x.pos);
                    if karte.holds(x.pos) { continue; }
                    match karte.blocked(b.0, b.1, b.2) {
                        Some(true) if solid(b.0, b.1, b.2).is_some() => { fest += 1; *arten.entry(world::block_registry::block_name(bloecke[&b]).unwrap_or("?")).or_insert(0u32) += 1; }
                        Some(true) => rot += 1,
                        _ => {
                            rand += 1;
                            let dist = (x.pos - auge).length();
                            if dist < 8.0 { nah_rand += 1; }
                            let oben = block_of(x.prev);
                            if oben.1 != b.1 { rand_unten += 1; } else { rand_seite += 1; }
                        }
                    }
                    let alter = tick - geboren.get(&x.id).copied().unwrap_or(tick);
                    let k = match alter { 0..=40 => 0, 41..=100 => 1, 101..=200 => 2, 201..=400 => 3, 401..=800 => 4, _ => 5 };
                    lebensdauer[k] += 1;
                }
                f.drop_outside(&karte, &ueberall);
                f.spawn_top(JE_BLOCK, 0.05, &ueberall);
                for x in &f.flakes { geboren.entry(x.id).or_insert(tick); }
            }
            let sek = f64::from(n) / 20.0;
            let mut l: Vec<_> = arten.into_iter().collect(); l.sort_by_key(|x| std::cmp::Reverse(x.1));
            eprintln!("DIAG feste bloecke: {:?}", &l[..l.len().min(14)]);
            eprintln!("DIAG {name} auge {a:?}: {} flocken; verschwinden je sekunde: in festem block {:.1}, in rotem block {:.1}, am bereichsrand {:.1} (davon unten {:.1}, seitlich {:.1}, naeher als r {:.1}); alter <=2s/5s/10s/20s/40s/mehr {lebensdauer:?}", f.flakes.len(), f64::from(fest) / sek, f64::from(rot) / sek, f64::from(rand) / sek, f64::from(rand_unten) / sek, f64::from(rand_seite) / sek, f64::from(nah_rand) / sek);
        }
    }
}
