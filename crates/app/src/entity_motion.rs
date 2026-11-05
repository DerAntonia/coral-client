// Motion Blur fuer bewegte Entities: wo jede Ecke der Entity-Geometrie im vorigen Bild lag (s.
// render::motion_blur). Ein Entity, das in beiden Bildern gleich viele Ecken hat, bekommt seine
// alten Ecken (so verwischen auch schwingende Arme und Beine); alles andere gilt als ruhender
// Weltpunkt und verwischt nur mit der Kamera. Gespeichert wird nur die Geometrie des vorigen
// Bildes, kein Bild.

use std::collections::HashMap;
use std::ops::Range;

use glam::DVec3;

// Weiter als so viele Bloecke in einem Bild: ein Teleport, kein Verwischen ueber den Sprung.
const MAX_JUMP_BLOCKS: f32 = 4.0;

// Schluessel eines Bereichs: Art (oben) und Entity-ID (unten). Dasselbe Entity hat so getrennte
// Bereiche fuer Modell, Feuer und Todesexplosion, die sich nicht gegenseitig verdraengen.
pub(crate) const KIND_ENTITY: u64 = 0;
pub(crate) const KIND_FIRE: u64 = 1;
pub(crate) const KIND_DEATH_BURST: u64 = 2;
// Bloecke im Geisterblock-Puffer (fallende Bloecke, TNT, Block-Displays, Kolbenbloecke).
pub(crate) const KIND_BLOCK: u64 = 3;
// Vom Kolben bewegte Bloecke (kein Entity: Schluessel aus der Zelle).
pub(crate) const KIND_PISTON: u64 = 4;
// Block-Entities per GPU-Skinning (Truhen, Koepfe, ...; s. block_entities.rs), aus der Zelle.
pub(crate) const KIND_BLOCK_ENTITY: u64 = 5;

pub(crate) const fn key(kind: u64, id: i32) -> u64 {
    kind << 32 | id as u32 as u64
}

#[derive(Default)]
pub(crate) struct EntityMotion {
    ranges: HashMap<u64, Range<usize>>,
    positions: Vec<[f32; 3]>,
    eye: Option<DVec3>,
}

impl EntityMotion {
    // Alles vergessen (Motion Blur aus): das naechste Bild verwischt nicht ueber die Luecke.
    pub(crate) fn reset(&mut self) {
        self.ranges.clear();
        self.positions.clear();
        self.eye = None;
    }

    // Ein Bild weiter. `positions` sind augenrelativ zu `eye`, `ranges` die Bereiche je Entity.
    // Rueckgabe: je Ecke die Lage im vorigen Bild (relativ zum vorigen Auge) und ob sich
    // ueberhaupt ein Entity bewegt hat.
    pub(crate) fn step(&mut self, positions: &[[f32; 3]], ranges: &[(u64, Range<usize>)], eye: DVec3) -> (Vec<[f32; 3]>, bool) {
        // Laeuft je Bild ueber alle Ecken aller Entities (zehntausende): deshalb schlichte
        // Schleifen ueber Arrays ohne Wurzel und ohne Iterator-Ketten -- im Fast-Build (app mit
        // opt-level 0) kostete die alte Fassung ein Viertel der Hauptthread-Zeit.
        //
        // Ruhender Weltpunkt: im vorigen Bild um die Augenbewegung verschoben.
        let [sx, sy, sz] = self.eye.map_or([0.0; 3], |alt| (eye - alt).as_vec3().to_array());
        let mut vorher: Vec<[f32; 3]> = Vec::with_capacity(positions.len());
        for p in positions {
            vorher.push([p[0] + sx, p[1] + sy, p[2] + sz]);
        }
        let abstand2 = |a: &[f32; 3], b: &[f32; 3]| {
            let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
            dx * dx + dy * dy + dz * dz
        };
        let sprung2 = MAX_JUMP_BLOCKS * MAX_JUMP_BLOCKS;
        let mut bewegt = false;
        if self.eye.is_some() {
            for (id, bereich) in ranges {
                let Some(alt) = self.ranges.get(id).filter(|a| a.len() == bereich.len()) else { continue };
                let (Some(alt), Some(ruhend)) = (self.positions.get(alt.clone()), vorher.get(bereich.clone())) else { continue };
                let n = alt.len();
                if n == 0 {
                    continue;
                }
                // Teleport: dann springt das ganze Entity, erste und letzte Ecke genuegen.
                if abstand2(&alt[0], &ruhend[0]) > sprung2 || abstand2(&alt[n - 1], &ruhend[n - 1]) > sprung2 {
                    continue;
                }
                // Hat sich ueberhaupt etwas bewegt? Nur bis zum ersten Treffer suchen.
                if !bewegt {
                    let mut i = 0;
                    while i < n {
                        if abstand2(&alt[i], &ruhend[i]) > 1e-8 {
                            bewegt = true;
                            break;
                        }
                        i += 1;
                    }
                }
                vorher[bereich.clone()].copy_from_slice(alt);
            }
        }
        self.ranges.clear();
        self.ranges.extend(ranges.iter().cloned());
        self.positions.clear();
        self.positions.extend_from_slice(positions);
        self.eye = Some(eye);
        (vorher, bewegt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bewegtes_entity_bekommt_seine_alten_ecken() {
        let mut m = EntityMotion::default();
        let eye = DVec3::new(10.0, 64.0, 10.0);
        m.step(&[[1.0, 0.0, 0.0], [5.0, 0.0, 0.0]], &[(7, 0..1)], eye);
        let (vorher, bewegt) = m.step(&[[1.5, 0.0, 0.0], [5.0, 0.0, 0.0]], &[(7, 0..1)], eye);
        assert!(bewegt);
        assert_eq!(vorher, vec![[1.0, 0.0, 0.0], [5.0, 0.0, 0.0]]);
    }

    // Ohne Partner (neu, andere Eckenzahl) ein ruhender Weltpunkt: nur das Auge hat sich bewegt.
    #[test]
    fn ohne_partner_ruhend() {
        let mut m = EntityMotion::default();
        m.step(&[[1.0, 0.0, 0.0]], &[(7, 0..1)], DVec3::ZERO);
        let (vorher, bewegt) = m.step(&[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], &[(7, 0..2)], DVec3::new(0.5, 0.0, 0.0));
        assert!(!bewegt);
        assert_eq!(vorher, vec![[1.5, 0.0, 0.0], [2.5, 0.0, 0.0]]);
    }

    #[test]
    fn teleport_verwischt_nicht() {
        let mut m = EntityMotion::default();
        m.step(&[[0.0, 0.0, 0.0]], &[(1, 0..1)], DVec3::ZERO);
        let (vorher, bewegt) = m.step(&[[20.0, 0.0, 0.0]], &[(1, 0..1)], DVec3::ZERO);
        assert!(!bewegt);
        assert_eq!(vorher, vec![[20.0, 0.0, 0.0]]);
    }
}
