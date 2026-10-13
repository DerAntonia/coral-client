// Welche Knoten in welcher Stufe gezeichnet werden.
//
// Von der hoechsten Stufe aus wird jede Knotenspalte so lange in ihre vier Kinder geteilt,
// wie ein Voxel ihrer Stufe auf dem Bildschirm groesser waere als die eingestellte Schwelle
// (Pixel^2, wie bei Voxy). Gemessen wird am naechsten Punkt der Spalte. Spalten, die ganz in
// den geladenen Chunks liegen, faellt weg -- dort zeichnet das echte Gelaende.

use crate::node::{NodeKey, MAX_LEVEL, N};

pub struct View {
    // Augenposition in Bloecken.
    pub eye: [f64; 3],
    // Sichtweite in Bloecken.
    pub radius: f64,
    // Unterteilungsschwelle in Pixel^2.
    pub threshold_px2: f64,
    // Brennweite in Pixeln: Bildhoehe / (2 tan(fov/2)).
    pub focal_px: f64,
    // Hoehenbereich der Welt in Bloecken, [unten, oben).
    pub y_range: (i64, i64),
}

// Abstand vom Auge zur Spalte (ueber die ganze Welthoehe).
fn distance(view: &View, level: u8, x: i32, z: i32) -> f64 {
    let size = (N as f64) * f64::from(1u32 << level);
    let gap = |p: f64, lo: f64, hi: f64| (lo - p).max(0.0).max(p - hi);
    let dx = gap(view.eye[0], x as f64 * size, (x + 1) as f64 * size);
    let dz = gap(view.eye[2], z as f64 * size, (z + 1) as f64 * size);
    let dy = gap(view.eye[1], view.y_range.0 as f64, view.y_range.1 as f64);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

// Die gewaehlten Spalten als (Stufe, x, z), grob nach Abstand sortiert (nahe zuerst).
pub fn select_columns(view: &View, covered: &dyn Fn(u8, i32, i32) -> bool) -> Vec<(u8, i32, i32, f64)> {
    let top = (N as f64) * f64::from(1u32 << MAX_LEVEL);
    let lo = |p: f64| ((p - view.radius) / top).floor() as i32;
    let hi = |p: f64| ((p + view.radius) / top).floor() as i32;
    let mut out = Vec::new();
    let mut stack: Vec<(u8, i32, i32)> = Vec::new();
    for x in lo(view.eye[0])..=hi(view.eye[0]) {
        for z in lo(view.eye[2])..=hi(view.eye[2]) {
            stack.push((MAX_LEVEL, x, z));
        }
    }
    while let Some((level, x, z)) = stack.pop() {
        let d = distance(view, level, x, z);
        if d > view.radius || covered(level, x, z) {
            continue;
        }
        let px = f64::from(1u32 << level) * view.focal_px / d.max(1.0);
        if level > 0 && px * px > view.threshold_px2 {
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                stack.push((level - 1, x * 2 + dx, z * 2 + dz));
            }
        } else {
            out.push((level, x, z, d));
        }
    }
    out.sort_by(|a, b| a.3.total_cmp(&b.3));
    out
}

// Die Knoten einer Spalte ueber den Hoehenbereich der Welt.
pub fn column_nodes(level: u8, x: i32, z: i32, y_range: (i64, i64)) -> impl Iterator<Item = NodeKey> {
    let size = (N as i64) << level;
    let (y0, y1) = (y_range.0.div_euclid(size), (y_range.1 - 1).div_euclid(size));
    (y0..=y1).map(move |y| NodeKey::new(level, x, y as i32, z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        View { eye: [100.0, 70.0, -50.0], radius: 2000.0, threshold_px2: 28.0, focal_px: 1080.0 / (2.0 * 55f64.to_radians().tan()), y_range: (-64, 320) }
    }

    #[test]
    fn nah_fein_fern_grob_und_lueckenlos() {
        let v = view();
        let cols = select_columns(&v, &|_, _, _| false);
        let naechste = cols.first().unwrap();
        assert_eq!(naechste.0, 0);
        assert!(cols.iter().any(|c| c.0 >= 3));
        // Kein Punkt doppelt oder gar nicht: jede Blockspalte im Umkreis liegt in genau einer.
        for (bx, bz) in [(100i64, -50i64), (700, 300), (-1500, 900), (1300, -1300)] {
            let treffer = cols
                .iter()
                .filter(|&&(l, x, z, _)| {
                    let s = (N as i64) << l;
                    bx.div_euclid(s) == x as i64 && bz.div_euclid(s) == z as i64
                })
                .count();
            assert_eq!(treffer, 1, "{bx} {bz}");
        }
    }

    #[test]
    fn geladenes_faellt_weg() {
        let v = view();
        // Alles bis 256 Bloecke um das Auge gilt als geladen.
        let geladen = |l: u8, x: i32, z: i32| {
            let s = (N as f64) * f64::from(1u32 << l);
            let (x0, z0) = (x as f64 * s, z as f64 * s);
            x0 >= -156.0 && x0 + s <= 356.0 && z0 >= -306.0 && z0 + s <= 206.0
        };
        let cols = select_columns(&v, &geladen);
        assert!(cols.iter().all(|&(l, x, z, _)| !geladen(l, x, z)));
        assert_eq!(column_nodes(0, 0, 0, (-64, 320)).count(), 12);
        assert_eq!(column_nodes(4, 0, 0, (-64, 320)).count(), 2);
    }
}

#[cfg(test)]
mod messung {
    use super::*;

    #[test]
    #[ignore]
    fn knoten_je_schwelle() {
        let focal = 1080.0 / (2.0 * 35f64.to_radians().tan());
        for t in [28.0, 8.0, 4.0, 2.0, 1.0] {
            let v = View { eye: [0.0, 80.0, 0.0], radius: 2048.0, threshold_px2: t, focal_px: focal, y_range: (-64, 320) };
            // Sichtweite 32 geladen.
            let geladen = |l: u8, x: i32, z: i32| {
                let n = 2i32 << l;
                x * n >= -32 && (x + 1) * n <= 32 && z * n >= -32 && (z + 1) * n <= 32
            };
            let cols = select_columns(&v, &geladen);
            let mut je = [0usize; 8];
            let mut knoten = 0;
            for &(l, x, z, _) in &cols {
                je[l as usize] += 1;
                knoten += column_nodes(l, x, z, v.y_range).count();
            }
            // Voxelgroesse am Rand der Sichtweite 32 (512 Bloecke) in Pixeln.
            let l_rand: u32 = (0..8u32).find(|&l| { let px = f64::from(1u32 << l) * focal / 512.0; px * px > t }).unwrap_or(8);
            println!("Schwelle {t:>4}: Spalten je Stufe {je:?}, Knoten {knoten}, am Rand Stufe ~{} ({:.1} px je Voxel bei Stufe {})", l_rand.saturating_sub(1), f64::from(1u32 << l_rand.saturating_sub(1)) * focal / 512.0, l_rand.saturating_sub(1));
        }
    }
}
