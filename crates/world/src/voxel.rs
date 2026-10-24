// Voxel-Abbild der Welt fuer den Shader-Pfad: je Block eine 16-Bit-Klasse, die sagt, wie der
// Block Licht aufhaelt. Die GPU verfolgt darin Strahlen (Sonnen-/Mondschatten, s.
// render::voxel_volume und deferred_light.wgsl).
//
// Eine Klasse ist eine Form (bis zu 15 achsenparallele Boxen in 1/16 Block, aus der echten
// Blockform) plus ein Material: undurchsichtig, Laub (lichtdurchlaessig gesprenkelt), Wasser
// oder Glas mit Toenung. Gleiche Form und gleiches Material teilen sich eine Klasse -- aus
// rund 27000 States werden so wenige hundert.
//
// Klasse 0 ist Luft, Klasse 1 der undurchsichtige Vollwuerfel (der haeufigste Fall, im Shader
// ohne Boxtest). Was keine aufgeloeste Blockform hat (Kreuz-Modelle wie Gras, Blumen und
// Tropfstein, Blockentities wie Truhen und Betten), bekommt Vanillas Umrissform: mit Kollision
// undurchsichtig (Tropfstein, Truhe), ohne Kollision als Pflanze, die einen Teil des Lichts
// durchlaesst (Gras, Blumen, Spinnweben).

use std::collections::HashMap;

use crate::block_shapes::BlockShapes;
use crate::chunk::{ChunkSection, SECTION_VOLUME};

pub const CLASS_AIR: u16 = 0;
pub const CLASS_FULL: u16 = 1;

// Materialien (unterstes Byte des Klassenkopfs, s. VoxelClasses::headers).
pub const MAT_OPAQUE: u32 = 0;
pub const MAT_LEAVES: u32 = 1;
pub const MAT_WATER: u32 = 2;
pub const MAT_GLASS: u32 = 3;
pub const MAT_PLANT: u32 = 4;
// Kreuz-Modell (Gras, Blumen, Tropfstein): zwei diagonale Ebenen, durchlaessig wo die Textur
// durchsichtig ist. Im Toenungsfeld steht der Index der Texturkachel in `cross_uvs`.
pub const MAT_CROSS: u32 = 5;
// Modelle aus flachen Ebenen ohne Dicke (Seegras): je Ebene drei vec4 in `cross_uvs` ab der
// "ersten Box" des Kopfs -- [Achse, Lage, min, max der ersten freien Achse], [min, max der
// zweiten freien Achse, 0, 0], Atlas-Kachel. Durchlaessig, wo die Textur durchsichtig ist.
pub const MAT_PLANES: u32 = 6;
const MAX_PLANES: usize = 15;

// Hoechstens so viele Boxen je Klasse (4 Bit im Kopf). Formen mit mehr Boxen behalten die
// groessten -- fuer Schatten genuegt das.
const MAX_BOXES: usize = 15;

// Worte je State in VoxelClasses::state_info:
//   0..6   je Seite (Reihenfolge wie FACE_DIRS) die Ecke ihres Atlas-Rechtecks, u und v je
//          16 Bit (0..1),
//   6..12  je Seite seine Groesse, ebenso,
//   12     je Seite 2 Bit Biom-Toenung (0 keine, 1 Gras, 2 Laub, 3 trockenes Laub),
//   13     Material (s. material::Material): Metall, Rauheit, Reflexion je 8 Bit, Bit 24 =
//          glaenzt ueberhaupt (sonst matt, keine Spiegelung),
//   14, 15 frei.
pub const STATE_INFO_WORTE: usize = 16;

pub struct VoxelClasses {
    // State-ID -> Klasse.
    class_of_state: Vec<u16>,
    // Je Klasse zwei u32: [Material | Boxanzahl << 8 | erste Box << 12, Toenung RGB8].
    pub headers: Vec<[u32; 2]>,
    // Je Box ein u32: min x/y/z, max x/y/z in 1/16 Block (0..16), je 5 Bit ab Bit 0.
    pub boxes: Vec<u32>,
    // Atlas-Kacheln der Kreuz-Modelle [u0, v0, u1, v1] (s. MAT_CROSS).
    pub cross_uvs: Vec<[f32; 4]>,
    // Je State STATE_INFO_WORTE Worte (s. STATE_INFO_WORTE): wo seine Seiten im Atlas liegen
    // (Farbe beim Rueckwurf des Lichts, Textur in Spiegelungen) und sein Material.
    pub state_info: Vec<u32>,
}

// Durchlass-Farbe von Glas, gefaerbt nach Farbstoff (linear grob, hellster Kanal ~0,9).
fn glass_tint(name: &str) -> Option<[f32; 3]> {
    const DYES: [(&str, [f32; 3]); 16] = [
        ("white", [0.9, 0.9, 0.9]),
        ("orange", [0.9, 0.45, 0.12]),
        ("magenta", [0.8, 0.3, 0.8]),
        ("light_blue", [0.4, 0.65, 0.9]),
        ("yellow", [0.9, 0.85, 0.2]),
        ("lime", [0.5, 0.85, 0.15]),
        ("pink", [0.9, 0.5, 0.65]),
        ("gray", [0.35, 0.35, 0.35]),
        ("light_gray", [0.6, 0.6, 0.6]),
        ("cyan", [0.2, 0.6, 0.65]),
        ("purple", [0.5, 0.22, 0.75]),
        ("blue", [0.2, 0.28, 0.85]),
        ("brown", [0.5, 0.33, 0.2]),
        ("green", [0.35, 0.5, 0.15]),
        ("red", [0.75, 0.18, 0.15]),
        ("black", [0.12, 0.12, 0.12]),
    ];
    if let Some(farbe) = name.strip_suffix("_stained_glass").or_else(|| name.strip_suffix("_stained_glass_pane")) {
        return DYES.iter().find(|(n, _)| *n == farbe).map(|(_, c)| *c);
    }
    match name {
        "glass" | "glass_pane" => Some([0.92, 0.95, 0.95]),
        "tinted_glass" => Some([0.12, 0.1, 0.14]),
        "ice" | "frosted_ice" => Some([0.7, 0.85, 0.95]),
        "slime_block" => Some([0.45, 0.85, 0.35]),
        "honey_block" => Some([0.95, 0.65, 0.15]),
        _ => None,
    }
}

fn pack_tint(c: [f32; 3]) -> u32 {
    let k = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
    k(c[0]) | k(c[1]) << 8 | k(c[2]) << 16
}

fn pack_box(min: [f32; 3], max: [f32; 3]) -> u32 {
    let q = |x: f32| (x * 16.0).round().clamp(0.0, 16.0) as u32;
    q(min[0]) | q(min[1]) << 5 | q(min[2]) << 10 | q(max[0]) << 15 | q(max[1]) << 20 | q(max[2]) << 25
}

const FULL_BOX: u32 = 16 << 15 | 16 << 20 | 16 << 25;

// Besteht die Blockform nur aus flachen Ebenen (Seegras), die Ebenen fuer MAT_PLANES.
fn flat_planes(shapes: &BlockShapes, uvs: &crate::BlockUvs, state: u32) -> Option<Vec<[f32; 4]>> {
    if !shapes.has_resolved_shape(state) {
        return None;
    }
    let form = shapes.shapes_of(state);
    let flach = |b: &crate::block_shapes::ShapeBox| (0..3).filter(|&a| b.max[a] <= b.min[a]).count() == 1;
    if form.is_empty() || !form.iter().all(flach) {
        return None;
    }
    let mut out = Vec::new();
    for b in form.iter().take(MAX_PLANES) {
        let achse = (0..3).find(|&a| b.max[a] <= b.min[a]).unwrap();
        // Freie Achsen: fuer x- und z-Ebenen die waagerechte und die Hoehe, fuer y-Ebenen x und z.
        let (s, q, face) = match achse {
            0 => (2, 1, 5),
            1 => (0, 2, 1),
            _ => (0, 1, 3),
        };
        let uv = crate::mesher::crop_face_uv(uvs.face_rect(state, face), b.uv[face]);
        out.push([achse as f32, b.min[achse], b.min[s], b.max[s]]);
        out.push([b.min[q], b.max[q], 0.0, 0.0]);
        out.push(uv);
    }
    Some(out)
}

// Boxen einer Form, gepackt, die groessten zuerst (hoechstens MAX_BOXES).
fn boxes_of(boxes: &[crate::block_shapes::ShapeBox]) -> Vec<u32> {
    let mut liste: Vec<(f32, u32)> = boxes
        .iter()
        .filter(|b| (0..3).all(|a| b.max[a] > b.min[a]))
        .map(|b| {
            let vol = (0..3).map(|a| b.max[a].min(1.0) - b.min[a].max(0.0)).product::<f32>();
            (vol, pack_box(b.min, b.max))
        })
        .collect();
    liste.sort_by(|a, b| b.0.total_cmp(&a.0));
    liste.truncate(MAX_BOXES);
    liste.into_iter().map(|(_, b)| b).collect()
}

impl VoxelClasses {
    pub fn build(shapes: &BlockShapes, cross: &crate::CrossBlocks, uvs: &crate::BlockUvs) -> Self {
        let count = crate::block_registry::state_count();
        let mut headers = vec![[0, 0], [MAT_OPAQUE | 1 << 8, 0]];
        let mut boxes = vec![FULL_BOX];
        let mut known: HashMap<(u32, u32, Vec<u32>), u16> = HashMap::new();
        known.insert((MAT_OPAQUE, 0, vec![FULL_BOX]), CLASS_FULL);
        let mut class_of_state = vec![CLASS_AIR; count];
        let mut cross_uvs: Vec<[f32; 4]> = Vec::new();
        let mut cross_index: HashMap<[u32; 4], u32> = HashMap::new();
        for state in 1..count as u32 {
            let name = crate::block_registry::block_name(state).unwrap_or("");
            let (material, tint, mut form) = if name == "water" || name == "bubble_column" {
                (MAT_WATER, pack_tint([0.55, 0.8, 0.9]), vec![FULL_BOX])
            } else if name == "lava" {
                (MAT_OPAQUE, 0, vec![FULL_BOX])
            } else if cross.is_cross(state) {
                // Wie im Mesher: eine Kachel fuer beide Ebenen (s. mesher::push_cross_quads).
                let uv = uvs.face_rect(state, 2);
                let index = *cross_index.entry(uv.map(f32::to_bits)).or_insert_with(|| {
                    cross_uvs.push(uv);
                    cross_uvs.len() as u32 - 1
                });
                (MAT_CROSS, index, vec![FULL_BOX])
            } else if let Some(ebenen) = flat_planes(shapes, uvs, state) {
                let start = cross_uvs.len() as u32;
                let count = ebenen.len() as u32 / 3;
                cross_uvs.extend(ebenen);
                headers.push([MAT_PLANES | count << 8 | start << 12, 0]);
                class_of_state[state as usize] = headers.len() as u16 - 1;
                continue;
            } else if matches!(name, "barrier" | "light" | "structure_void" | "moving_piston") {
                // Unsichtbar: Barrieren um PvP-Arenen duerfen keine Schatten werfen.
                continue;
            } else {
                // Kreuz-Modelle und Schilder stehen in der Formtabelle als aufgeloest, aber
                // ohne Boxen -- auch sie bekommen den Umriss.
                let form = if shapes.has_resolved_shape(state) { boxes_of(shapes.shapes_of(state)) } else { Vec::new() };
                if form.is_empty() {
                    let Some(umriss) = crate::vanilla_collision::outline_boxes(state) else { continue };
                    let form = boxes_of(umriss);
                    if form.is_empty() {
                        continue;
                    }
                    let fest = crate::vanilla_collision::collision_boxes(state).is_some_and(|b| !b.is_empty());
                    (if fest { MAT_OPAQUE } else { MAT_PLANT }, 0, form)
                } else if name.ends_with("_leaves") {
                    (MAT_LEAVES, 0, form)
                } else if let Some(c) = glass_tint(name) {
                    (MAT_GLASS, pack_tint(c), form)
                } else {
                    (MAT_OPAQUE, 0, form)
                }
            };
            form.sort_unstable();
            form.dedup();
            let next = headers.len() as u16;
            let class = *known.entry((material, tint, form.clone())).or_insert_with(|| {
                headers.push([material | (form.len() as u32) << 8 | (boxes.len() as u32) << 12, tint]);
                boxes.extend_from_slice(&form);
                next
            });
            class_of_state[state as usize] = class;
        }
        let mut state_info = vec![0u32; count * STATE_INFO_WORTE];
        let q = |x: f32| (x.clamp(0.0, 1.0) * 65535.0).round() as u32;
        for state in 0..count as u32 {
            let name = crate::block_registry::block_name(state);
            let toenung = name.map_or(crate::biome_tint::TintKind::None, crate::biome_tint::tint_kind);
            let info = &mut state_info[state as usize * STATE_INFO_WORTE..][..STATE_INFO_WORTE];
            for face in 0..6 {
                let r = uvs.face_rect(state, face);
                info[face] = q(r[0].min(r[2])) | q(r[1].min(r[3])) << 16;
                info[6 + face] = q((r[2] - r[0]).abs()) | q((r[3] - r[1]).abs()) << 16;
                let tint = match toenung {
                    crate::biome_tint::TintKind::GrassTop if face == 1 => 1,
                    crate::biome_tint::TintKind::GrassAll => 1,
                    crate::biome_tint::TintKind::FoliageAll => 2,
                    crate::biome_tint::TintKind::DryFoliageAll => 3,
                    // Feste Farben passen nicht in die zwei Bit: in der Fernsicht wie Laub.
                    crate::biome_tint::TintKind::Fixed(_) => 2,
                    _ => 0,
                };
                info[12] |= tint << (2 * face);
            }
            if let Some(m) = name.and_then(crate::material::material) {
                let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
                info[13] = b(m.metall) | b(m.rauheit) << 8 | b(m.reflexion) << 16 | 1 << 24;
            }
        }
        Self { class_of_state, headers, boxes, cross_uvs, state_info }
    }

    pub fn class(&self, state: u32) -> u16 {
        self.class_of_state.get(state as usize).copied().unwrap_or(CLASS_AIR)
    }

    // Klassen einer Section in deren eigener Reihenfolge (Index = (y * 16 + z) * 16 + x).
    // Fehlt die Section (nicht geladen, ausserhalb der Welt), ist alles Luft. true, wenn
    // etwas anderes als Luft darin steht.
    pub fn fill_section(&self, section: Option<&ChunkSection>, out: &mut [u16; SECTION_VOLUME]) -> bool {
        let Some(section) = section else {
            out.fill(CLASS_AIR);
            return false;
        };
        let mut belegt = 0u16;
        for (o, &s) in out.iter_mut().zip(section.states()) {
            *o = self.class(s);
            belegt |= *o;
        }
        belegt != 0
    }

    // Die States einer Section (fuer die Farbe, s. state_faces), in derselben Reihenfolge;
    // fehlt sie, Luft.
    pub fn fill_states(section: Option<&ChunkSection>, out: &mut [u16; SECTION_VOLUME]) {
        match section {
            Some(section) => {
                for (o, &s) in out.iter_mut().zip(section.states()) {
                    *o = s.min(u16::MAX as u32) as u16;
                }
            }
            None => out.fill(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vollwuerfel_ist_klasse_eins() {
        let classes = VoxelClasses::build(&BlockShapes::all_full_cubes(), &crate::CrossBlocks::none(), &crate::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
        assert_eq!(classes.class(0), CLASS_AIR);
        assert_eq!(classes.class(1), CLASS_FULL);
    }

    // Kreuz-Pflanzen haben in der Formtabelle eine leere Boxliste (s. app: cross_raw) --
    // sie muessen trotzdem eine Klasse bekommen (Umriss, Pflanze), sonst kein Grasschatten.
    #[test]
    fn leere_form_nimmt_den_umriss() {
        let mut per_state: Vec<Option<Vec<crate::block_shapes::ShapeBox>>> = vec![None; crate::block_registry::state_count()];
        let gras = (0..crate::block_registry::state_count() as u32).find(|&s| crate::block_registry::block_name(s) == Some("short_grass")).unwrap();
        per_state[gras as usize] = Some(Vec::new());
        let classes = VoxelClasses::build(&BlockShapes::new(per_state), &crate::CrossBlocks::none(), &crate::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
        let c = classes.class(gras);
        assert_ne!(c, CLASS_AIR);
        assert_eq!(classes.headers[c as usize][0] & 255, MAT_PLANT);
    }

    // Seegras: nur flache Ebenen -- eigene Ebenen-Klasse statt einer Umrissbox.
    #[test]
    fn flache_ebenen_werden_ebenen() {
        let mut per_state: Vec<Option<Vec<crate::block_shapes::ShapeBox>>> = vec![None; crate::block_registry::state_count()];
        let seegras = (0..crate::block_registry::state_count() as u32).find(|&s| crate::block_registry::block_name(s) == Some("seagrass")).unwrap();
        let ebene = crate::block_shapes::ShapeBox { min: [0.0, 0.0, 0.25], max: [1.0, 1.0, 0.25], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        per_state[seegras as usize] = Some(vec![ebene]);
        let classes = VoxelClasses::build(&BlockShapes::new(per_state), &crate::CrossBlocks::none(), &crate::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
        let h = classes.headers[classes.class(seegras) as usize][0];
        assert_eq!(h & 255, MAT_PLANES);
        assert_eq!((h >> 8) & 15, 1);
        assert_eq!(classes.cross_uvs[(h >> 12) as usize], [2.0, 0.25, 0.0, 1.0]);
    }

    #[test]
    fn box_packen() {
        assert_eq!(pack_box([0.0; 3], [1.0; 3]), FULL_BOX);
        let b = pack_box([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        assert_eq!((b >> 5) & 31, 8);
    }
}
