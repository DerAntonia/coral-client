// Wasserfarbe weich ueber Biomgrenzen: je Block der Mittelwert der Biom-Wasserfarben in einem
// Quadrat von (2 * RADIUS + 1) Bloecken um ihn, auf seiner Hoehe. Ohne das bekaeme jede
// 4x4-Biomzelle ihre eigene harte Farbe, und an Grenzen stuende eine Treppe im Wasser.
// Vanillas Biom-Mischung geht bis 7 (15x15); hier bewusst breiter, die Grenze soll kaum zu
// sehen sein.
//
// Gerechnet wird je Biom-Hoehenstufe (4 Bloecke) einmal fuer die ganze Spalte, ueber eine
// Summentabelle -- erst wenn dort Wasser vorkommt. Fehlende Nachbarspalten zaehlen nicht mit.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::biome_palette::BiomePalette;
use crate::chunk::{ChunkColumn, SECTION_SIZE};
use crate::mesher::ColumnNeighbors;

pub const RADIUS: i32 = 12;

const S: i32 = SECTION_SIZE;
const W: i32 = S + 2 * RADIUS;

pub struct WaterBlend<'a> {
    neighbors: &'a ColumnNeighbors,
    biomes: &'a BiomePalette,
    // Biom-Hoehenstufe (y div 4) -> Farbe je Block der Spalte, Index z * 16 + x.
    stufen: RefCell<HashMap<i32, Box<[[f32; 3]]>>>,
}

impl<'a> WaterBlend<'a> {
    pub fn new(neighbors: &'a ColumnNeighbors, biomes: &'a BiomePalette) -> Self {
        Self { neighbors, biomes, stufen: RefCell::new(HashMap::new()) }
    }

    // x/z spaltenlokal (0..16), y Welthoehe.
    pub fn at(&self, x: i32, y: i32, z: i32) -> [f32; 3] {
        let stufe = y.div_euclid(4);
        let mut stufen = self.stufen.borrow_mut();
        let farben = stufen.entry(stufe).or_insert_with(|| self.stufe(stufe * 4));
        farben[(z * S + x) as usize]
    }

    // Spalte fuer eine spaltenlokale Position, die bis RADIUS ueber den Rand reicht.
    fn spalte(&self, x: i32, z: i32) -> Option<(&ChunkColumn, i32, i32)> {
        let n = self.neighbors;
        let spalte = match (x.div_euclid(S), z.div_euclid(S)) {
            (0, 0) => Some(&n.target),
            (0, -1) => n.north.as_ref(),
            (0, 1) => n.south.as_ref(),
            (1, 0) => n.east.as_ref(),
            (-1, 0) => n.west.as_ref(),
            (1, -1) => n.northeast.as_ref(),
            (-1, -1) => n.northwest.as_ref(),
            (1, 1) => n.southeast.as_ref(),
            (-1, 1) => n.southwest.as_ref(),
            _ => None,
        }?;
        Some((spalte, x.rem_euclid(S), z.rem_euclid(S)))
    }

    fn stufe(&self, y: i32) -> Box<[[f32; 3]]> {
        // Summentabelle ueber Farbe und Anzahl (vierter Wert), eine Zeile/Spalte Vorlauf.
        let b = (W + 1) as usize;
        let mut summe = vec![[0.0f32; 4]; b * b];
        for tz in 0..W {
            for tx in 0..W {
                let wert = match self.spalte(tx - RADIUS, tz - RADIUS) {
                    Some((spalte, lx, lz)) => {
                        let c = self.biomes.colors_of(spalte.biome_at(lx, y, lz)).water;
                        [c[0], c[1], c[2], 1.0]
                    }
                    None => [0.0; 4],
                };
                let (i, oben, links, diag) = ((tz as usize + 1) * b + tx as usize + 1, tz as usize * b + tx as usize + 1, (tz as usize + 1) * b + tx as usize, tz as usize * b + tx as usize);
                for k in 0..4 {
                    summe[i][k] = wert[k] + summe[oben][k] + summe[links][k] - summe[diag][k];
                }
            }
        }
        let mut farben = vec![[0.0f32; 3]; (S * S) as usize].into_boxed_slice();
        let d = (2 * RADIUS + 1) as usize;
        for z in 0..S as usize {
            for x in 0..S as usize {
                // Fenster [x, x + d) in der Tabelle (x ist dort schon um RADIUS verschoben).
                let at = |zz: usize, xx: usize| summe[zz * b + xx];
                let (a, o, l, u) = (at(z + d, x + d), at(z, x + d), at(z + d, x), at(z, x));
                let n = a[3] - o[3] - l[3] + u[3];
                farben[z * S as usize + x] = std::array::from_fn(|k| (a[k] - o[k] - l[k] + u[k]) / n.max(1.0));
            }
        }
        farben
    }
}
