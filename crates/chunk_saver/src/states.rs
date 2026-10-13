// Was die Fernsicht je Block-State wissen muss: ob er von weitem eine deckende Flaeche ist,
// eine durchscheinende (Wasser, Glas, Eis) oder gar nichts (Luft, Pflanzen, Fackeln,
// Schienen), welche Seite mit welcher Biomfarbe getoent wird, und seine Form als Huellbox.
//
// Abgeleitet aus world::voxel::VoxelClasses, also aus denselben Formen und Materialien wie
// das Voxel-Abbild des Shader-Pfads. Die Huellbox reicht fuer die Ferne: eine Schneedecke ist
// flach, eine Stufe halb hoch, eine Treppe ein Wuerfel.

use world::voxel::{VoxelClasses, MAT_GLASS, MAT_LEAVES, MAT_OPAQUE, MAT_WATER, STATE_INFO_WORTE};

use crate::node::{shape_bits, state_of, Voxel, FULL_SHAPE};

pub const CAT_AIR: u8 = 0;
pub const CAT_PLANT: u8 = 1;
pub const CAT_TRANSLUCENT: u8 = 2;
pub const CAT_OPAQUE: u8 = 3;

// Toenung einer Seite (s. tint_of).
pub const TINT_NONE: u8 = 0;
pub const TINT_GRASS: u8 = 1;
pub const TINT_FOLIAGE: u8 = 2;
pub const TINT_DRY_FOLIAGE: u8 = 3;
pub const TINT_WATER: u8 = 4;

pub struct LodStates {
    category: Vec<u8>,
    // Je State und Seite (Reihenfolge wie world::ambient_occlusion::FACE_DIRS) eine Toenung.
    tint: Vec<[u8; 6]>,
    // Huellbox je State (s. node::pack_shape).
    shape: Vec<u32>,
    // Eckabdunklung wie im nahen Gelaende (world::ambient_occlusion): dunkelt dieser State
    // seine Nachbarn ab, und bleiben seine eigenen Seiten davon frei (leuchtende Bloecke,
    // Modelle ohne Abdunklung, Wasser)? Ohne set_ao_flags: deckende Vollwuerfel dunkeln ab.
    darkens: Vec<bool>,
    no_ao: Vec<bool>,
}

impl LodStates {
    pub fn from_voxel_classes(classes: &VoxelClasses) -> Self {
        let count = world::block_registry::state_count();
        let mut category = vec![CAT_AIR; count];
        let mut tint = vec![[TINT_NONE; 6]; count];
        let mut shape = vec![FULL_SHAPE; count];
        for state in 0..count {
            let class = classes.class(state as u32);
            if class == world::voxel::CLASS_AIR {
                continue;
            }
            let [kopf, _] = classes.headers[class as usize];
            let material = kopf & 255;
            let name = world::block_registry::block_name(state as u32).unwrap_or("");
            category[state] = match material {
                MAT_OPAQUE | MAT_LEAVES => CAT_OPAQUE,
                MAT_WATER => CAT_TRANSLUCENT,
                // Durchscheinend nur, was auch das nahe Gelaende durchscheinend zeichnet (echtes
                // Glas, s. world::mesher::is_glass). Eis, Schleim und Honig zeichnet es deckend --
                // durchscheinend lag in der Ferne das Eis ueber Wasser und Grund und wurde dunkel
                // und fleckig.
                MAT_GLASS if world::mesher::is_glass(name) => CAT_TRANSLUCENT,
                MAT_GLASS => CAT_OPAQUE,
                // Eine einzelne Lage Schnee hat keine Kollision und zaehlt im Voxel-Abbild als
                // Pflanze -- von weitem ist eine verschneite Ebene aber weiss.
                _ if name == "snow" => CAT_OPAQUE,
                _ => CAT_PLANT,
            };
            let (anzahl, erste) = (((kopf >> 8) & 15) as usize, (kopf >> 12) as usize);
            if matches!(material, MAT_OPAQUE | MAT_LEAVES | MAT_GLASS | world::voxel::MAT_PLANT) && anzahl > 0 {
                let mut b = [16u32, 16, 16, 0, 0, 0];
                for &packed in classes.boxes.iter().skip(erste).take(anzahl) {
                    for a in 0..3 {
                        b[a] = b[a].min((packed >> (a * 5)) & 31);
                        b[a + 3] = b[a + 3].max((packed >> ((a + 3) * 5)) & 31);
                    }
                }
                if (0..3).all(|a| b[a + 3] > b[a]) {
                    shape[state] = crate::node::pack_shape(b);
                }
            }
            let bits = classes.state_info[state * STATE_INFO_WORTE + 12];
            for (face, t) in tint[state].iter_mut().enumerate() {
                *t = if material == MAT_WATER { TINT_WATER } else { ((bits >> (2 * face)) & 3) as u8 };
            }
        }
        let darkens = (0..count).map(|s| category[s] == CAT_OPAQUE && shape[s] == FULL_SHAPE).collect();
        let no_ao = (0..count)
            .map(|s| {
                let class = classes.class(s as u32);
                class != world::voxel::CLASS_AIR && classes.headers[class as usize][0] & 255 == MAT_WATER
            })
            .collect();
        Self { category, tint, shape, darkens, no_ao }
    }

    // Dieselben Regeln wie das nahe Gelaende (world::BlockShapes::ao_flags je State): wer
    // abdunkelt (AO_DARKENS -- Glas nicht, Schlamm und Seelensand schon) und wer selbst keine
    // Abdunklung bekommt (AO_SKIP). Wasser bleibt ohne, wie die Fluessigkeiten dort.
    pub fn set_ao_flags(&mut self, flags: impl Fn(u32) -> u8) {
        for state in 0..self.category.len() {
            let f = flags(state as u32);
            self.darkens[state] = f & world::ambient_occlusion::AO_DARKENS != 0;
            self.no_ao[state] |= f & world::ambient_occlusion::AO_SKIP != 0;
        }
    }

    // Dunkelt dieser Voxel Ecken ab? Auf Stufe 0 nach seinem State; zusammengefasst nur, wenn
    // er dazu seine ganze Zelle fuellt.
    pub fn darkens(&self, v: Voxel) -> bool {
        self.darkens.get(state_of(v) as usize).copied().unwrap_or(false) && (shape_bits(v) == 0 || shape_bits(v) == FULL_SHAPE)
    }

    pub fn no_ao(&self, state: u32) -> bool {
        self.no_ao.get(state as usize).copied().unwrap_or(false)
    }

    // Fuer Tests: nur Kategorien, nichts getoent, alles Vollwuerfel.
    pub fn from_categories(category: Vec<u8>) -> Self {
        let tint = vec![[TINT_NONE; 6]; category.len()];
        let shape = vec![FULL_SHAPE; category.len()];
        let darkens = category.iter().map(|&c| c == CAT_OPAQUE).collect();
        let no_ao = vec![false; category.len()];
        Self { category, tint, shape, darkens, no_ao }
    }

    pub fn set_shape(&mut self, state: u32, shape: u32) {
        self.shape[state as usize] = shape;
        if shape != FULL_SHAPE {
            self.darkens[state as usize] = false;
        }
    }

    // Huellbox eines Voxels: seine eigene (hoehere Stufen) oder die seines States.
    pub fn shape_of(&self, v: Voxel) -> u32 {
        match shape_bits(v) {
            0 => self.shape.get(state_of(v) as usize).copied().unwrap_or(FULL_SHAPE),
            s => s,
        }
    }

    pub fn category(&self, state: u32) -> u8 {
        self.category.get(state as usize).copied().unwrap_or(CAT_AIR)
    }

    pub fn tint_of(&self, state: u32, face: usize) -> u8 {
        self.tint.get(state as usize).map_or(TINT_NONE, |t| t[face])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eis_ist_deckend_glas_durchscheinend() {
        let classes = VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
        let s = LodStates::from_voxel_classes(&classes);
        let state = |name: &str| (0..world::block_registry::state_count() as u32).find(|&s| world::block_registry::block_name(s) == Some(name)).unwrap();
        assert_eq!(s.category(state("ice")), CAT_OPAQUE);
        assert_eq!(s.category(state("packed_ice")), CAT_OPAQUE);
        assert_eq!(s.category(state("glass")), CAT_TRANSLUCENT);
        assert_eq!(s.category(state("water")), CAT_TRANSLUCENT);
    }
}
