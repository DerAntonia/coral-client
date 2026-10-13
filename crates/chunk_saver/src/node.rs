// Ein Knoten der Fernsicht: ein Wuerfel aus 32 x 32 x 32 Voxeln auf einer LOD-Stufe.
//
// Stufe 0 ist blockgenau (ein Voxel = ein Block, ein Knoten = 2 x 2 Chunks und zwei Sections
// hoch). Jede Stufe darueber fasst 2 x 2 x 2 Voxel der Stufe darunter zu einem zusammen: auf
// Stufe L ist ein Voxel 2^L Bloecke gross und ein Knoten 32 * 2^L Bloecke. Damit liegen die
// acht Kinder eines Knotens genau in seinen acht Oktanten, und ein Oktant entsteht allein aus
// seinem Kind (s. downsample_octant).
//
// Ein Voxel ist ein u64:
//   Bit  0..16  Block-State (feste Registry des Clients, rund 28000),
//   Bit 16..24  Biom (Registry des Servers, je Welt fest),
//   Bit 24..32  Licht: Blocklicht << 4 | Himmelslicht, je 0..15 -- auch in Luft, denn das Licht
//               einer Flaeche ist das des Voxels davor,
//   Bit 32..62  Form: Huellbox in 1/16 des Voxels (min x, y, z, max x, y, z je 5 Bit). 0 heisst
//               "die Form des States" (Stufe 0); hoehere Stufen tragen die Huelle ihrer Kinder,
//               damit eine Schneedecke auch zusammengefasst flach bleibt.
// Luft ist jeder Voxel mit State 0.

use crate::states::{LodStates, CAT_OPAQUE, CAT_TRANSLUCENT};

pub const N: usize = 32;
pub const VOLUME: usize = N * N * N;
// Hoechste Stufe: ein Knoten ist dort 4096 Bloecke gross, ein Voxel 128. Weiter als 650 Chunks
// muss nichts reichen.
pub const MAX_LEVEL: u8 = 7;

pub type Voxel = u64;
// Luft unter freiem Himmel -- so gilt auch, was nie gespeichert wurde.
pub const AIR: Voxel = 15 << 24;

pub const FULL_SHAPE: u32 = 16 << 15 | 16 << 20 | 16 << 25;

pub fn voxel(state: u32, biome: u32) -> Voxel {
    (state.min(0xFFFF) as u64) | ((biome.min(0xFF) as u64) << 16)
}

pub fn with_light(v: Voxel, block: u8, sky: u8) -> Voxel {
    (v & !(0xFF << 24)) | ((((block.min(15) << 4) | sky.min(15)) as u64) << 24)
}

pub fn with_shape(v: Voxel, shape: u32) -> Voxel {
    (v & !(0x3FFF_FFFF << 32)) | ((shape as u64 & 0x3FFF_FFFF) << 32)
}

pub fn state_of(v: Voxel) -> u32 {
    (v & 0xFFFF) as u32
}

pub fn biome_of(v: Voxel) -> u32 {
    ((v >> 16) & 0xFF) as u32
}

pub fn light_of(v: Voxel) -> u8 {
    ((v >> 24) & 0xFF) as u8
}

pub fn is_air(v: Voxel) -> bool {
    state_of(v) == 0
}

// Gespeicherte Form (0 = die des States, s. LodStates::shape).
pub fn shape_bits(v: Voxel) -> u32 {
    ((v >> 32) & 0x3FFF_FFFF) as u32
}

// Huellbox als [min x, min y, min z, max x, max y, max z] in 1/16.
pub fn unpack_shape(s: u32) -> [u32; 6] {
    std::array::from_fn(|i| (s >> (i * 5)) & 31)
}

pub fn pack_shape(b: [u32; 6]) -> u32 {
    b.iter().enumerate().fold(0, |acc, (i, &v)| acc | (v.min(16) << (i * 5)))
}

pub fn index(x: usize, y: usize, z: usize) -> usize {
    (y * N + z) * N + x
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NodeKey {
    pub level: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl NodeKey {
    pub fn new(level: u8, x: i32, y: i32, z: i32) -> Self {
        Self { level, x, y, z }
    }

    // Kantenlaenge in Bloecken.
    pub fn size(&self) -> i64 {
        (N as i64) << self.level
    }

    // Groesse eines Voxels in Bloecken.
    pub fn voxel_size(&self) -> i64 {
        1 << self.level
    }

    // Untere Ecke in Blockkoordinaten.
    pub fn origin(&self) -> [i64; 3] {
        let s = self.size();
        [self.x as i64 * s, self.y as i64 * s, self.z as i64 * s]
    }

    // Arithmetisches Schieben rundet nach unten -- auch fuer negative Koordinaten richtig.
    pub fn parent(&self) -> NodeKey {
        NodeKey { level: self.level + 1, x: self.x >> 1, y: self.y >> 1, z: self.z >> 1 }
    }

    // In welchem Oktanten des Elternknotens dieser Knoten liegt (je Achse 0 oder 1).
    pub fn octant(&self) -> [usize; 3] {
        [(self.x & 1) as usize, (self.y & 1) as usize, (self.z & 1) as usize]
    }

    pub fn children(&self) -> [NodeKey; 8] {
        debug_assert!(self.level > 0);
        let l = self.level - 1;
        let (x, y, z) = (self.x * 2, self.y * 2, self.z * 2);
        std::array::from_fn(|i| NodeKey { level: l, x: x + (i & 1) as i32, y: y + ((i >> 1) & 1) as i32, z: z + ((i >> 2) & 1) as i32 })
    }

    // Die sechs Nachbarn derselben Stufe in der Reihenfolge von mesh::FACES (unten, oben,
    // Norden, Sueden, Westen, Osten).
    pub fn neighbors(&self) -> [NodeKey; 6] {
        let k = |dx: i32, dy: i32, dz: i32| NodeKey { level: self.level, x: self.x + dx, y: self.y + dy, z: self.z + dz };
        [k(0, -1, 0), k(0, 1, 0), k(0, 0, -1), k(0, 0, 1), k(-1, 0, 0), k(1, 0, 0)]
    }

    // Der Knoten der Stufe 0, in dem ein Block liegt.
    pub fn of_block(x: i64, y: i64, z: i64) -> NodeKey {
        let n = N as i64;
        NodeKey { level: 0, x: x.div_euclid(n) as i32, y: y.div_euclid(n) as i32, z: z.div_euclid(n) as i32 }
    }
}

#[derive(Clone)]
pub struct Node {
    pub voxels: Box<[Voxel]>,
}

impl Default for Node {
    fn default() -> Self {
        Self::empty()
    }
}

impl Node {
    pub fn empty() -> Self {
        Self { voxels: vec![AIR; VOLUME].into_boxed_slice() }
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> Voxel {
        self.voxels[index(x, y, z)]
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, v: Voxel) {
        self.voxels[index(x, y, z)] = v;
    }

    // Nur Luft? Dann wird der Knoten nicht gespeichert (sein Licht gilt dann als Himmel).
    pub fn is_empty(&self) -> bool {
        self.voxels.iter().all(|&v| is_air(v))
    }

    // Einen Oktanten dieses Knotens aus seinem Kind neu berechnen. Je 2 x 2 x 2 Kind-Voxel
    // wird einer: zuerst zaehlt die obere Lage, denn von weitem sieht man die Oberflaeche --
    // Wasser ueber Sand bleibt Wasser, Schnee ueber Gras bleibt Schnee. Innerhalb einer Lage
    // gewinnt Deckendes vor Durchscheinendem. Pflanzen und Luft zaehlen nicht: ein Grashalm
    // auf Erde soll die Erde nicht verschwinden lassen.
    //
    // Die Form ist die Huelle aller zaehlenden Kinder: eine Lage Schnee auf Gras ergibt einen
    // Voxel, der etwas mehr als halb hoch ist, statt eines vollen. Bleibt nur Luft, bekommt der
    // Voxel das hellste Licht seiner Kinder -- von weitem sieht man, wo es hell ist.
    pub fn downsample_octant(&mut self, child: &Node, octant: [usize; 3], states: &LodStates) {
        let h = N / 2;
        let [ox, oy, oz] = octant.map(|o| o * h);
        for py in 0..h {
            for pz in 0..h {
                for px in 0..h {
                    let (cx, cy, cz) = (px * 2, py * 2, pz * 2);
                    let mut best: Option<Voxel> = None;
                    let mut huelle = [16u32, 16, 16, 0, 0, 0];
                    let (mut block, mut sky) = (0u8, 0u8);
                    for dy in [1, 0] {
                        let mut lage: Option<(u8, Voxel)> = None;
                        for dz in 0..2 {
                            for dx in 0..2 {
                                let v = child.get(cx + dx, cy + dy, cz + dz);
                                let r = rang_of(v, states);
                                if r == 0 {
                                    let l = light_of(v);
                                    block = block.max(l >> 4);
                                    sky = sky.max(l & 15);
                                    continue;
                                }
                                let b = unpack_shape(states.shape_of(v));
                                let o = [dx as u32 * 8, dy as u32 * 8, dz as u32 * 8];
                                for a in 0..3 {
                                    huelle[a] = huelle[a].min(o[a] + b[a] / 2);
                                    huelle[a + 3] = huelle[a + 3].max(o[a] + b[a + 3].div_ceil(2));
                                }
                                if lage.is_none_or(|(lr, _)| r > lr) {
                                    lage = Some((r, v));
                                }
                            }
                        }
                        if best.is_none() {
                            best = lage.map(|(_, v)| v);
                        }
                    }
                    let out = match best {
                        Some(v) => with_shape(v, pack_shape(huelle)),
                        None => with_light(0, block, sky),
                    };
                    self.set(ox + px, oy + py, oz + pz, out);
                }
            }
        }
    }

    // Platzsparend fuer die Platte: Palette der vorkommenden Voxel, dann je Voxel ein Index
    // (ein oder zwei Byte), alles mit Deflate gepackt. Ein typischer Knoten schrumpft so von
    // 256 KB auf wenige KB.
    pub fn encode(&self) -> Vec<u8> {
        let mut palette: Vec<Voxel> = Vec::new();
        let mut lookup: rustc_hash::FxHashMap<Voxel, u16> = rustc_hash::FxHashMap::default();
        let mut indices: Vec<u16> = Vec::with_capacity(VOLUME);
        for &v in self.voxels.iter() {
            let i = *lookup.entry(v).or_insert_with(|| {
                palette.push(v);
                (palette.len() - 1) as u16
            });
            indices.push(i);
        }
        let mut raw = Vec::with_capacity(8 + palette.len() * 8 + VOLUME * 2);
        raw.push(FORMAT_VERSION);
        raw.extend_from_slice(&(palette.len() as u32).to_le_bytes());
        for v in &palette {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        if palette.len() > 256 {
            for i in indices {
                raw.extend_from_slice(&i.to_le_bytes());
            }
        } else if palette.len() > 1 {
            raw.extend(indices.into_iter().map(|i| i as u8));
        }
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut enc, &raw).expect("in den Speicher schreiben");
        enc.finish().expect("in den Speicher schreiben")
    }

    pub fn decode(bytes: &[u8]) -> Option<Node> {
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(bytes), &mut raw).ok()?;
        if raw.len() < 5 {
            return None;
        }
        let count = u32::from_le_bytes(raw[1..5].try_into().ok()?) as usize;
        let mut at = 5;
        let palette: Vec<Voxel> = match raw[0] {
            // Fassung 1: u32 je Voxel (State, Biom), ohne Licht und Form.
            1 => (0..count)
                .map(|i| {
                    let v = u32::from_le_bytes(raw.get(at + i * 4..at + i * 4 + 4)?.try_into().ok()?);
                    Some(if v & 0xFFFF == 0 { AIR } else { v as u64 })
                })
                .collect::<Option<_>>()?,
            FORMAT_VERSION => (0..count).map(|i| Some(u64::from_le_bytes(raw.get(at + i * 8..at + i * 8 + 8)?.try_into().ok()?))).collect::<Option<_>>()?,
            _ => return None,
        };
        at += count * if raw[0] == 1 { 4 } else { 8 };
        let mut node = Node::empty();
        match count {
            0 => {}
            1 => node.voxels.fill(palette[0]),
            2..=256 => {
                let idx = raw.get(at..at + VOLUME)?;
                for (v, &i) in node.voxels.iter_mut().zip(idx) {
                    *v = *palette.get(i as usize)?;
                }
            }
            _ => {
                let idx = raw.get(at..at + VOLUME * 2)?;
                for (v, i) in node.voxels.iter_mut().zip(idx.chunks_exact(2)) {
                    *v = *palette.get(u16::from_le_bytes([i[0], i[1]]) as usize)?;
                }
            }
        }
        Some(node)
    }
}

fn rang_of(v: Voxel, states: &LodStates) -> u8 {
    match states.category(state_of(v)) {
        CAT_OPAQUE => 2,
        CAT_TRANSLUCENT => 1,
        _ => 0,
    }
}

const FORMAT_VERSION: u8 = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::{CAT_AIR, CAT_PLANT};

    fn states() -> LodStates {
        // 1 Stein, 2 Wasser, 3 Gras-Pflanze, 4 Erde, 5 Schneedecke (2/16 hoch).
        let mut s = LodStates::from_categories(vec![CAT_AIR, CAT_OPAQUE, CAT_TRANSLUCENT, CAT_PLANT, CAT_OPAQUE, CAT_OPAQUE]);
        s.set_shape(5, pack_shape([0, 0, 0, 16, 2, 16]));
        s
    }

    #[test]
    fn eltern_und_kinder_passen_zusammen() {
        let k = NodeKey::new(0, -3, 1, 5);
        assert_eq!(k.parent(), NodeKey::new(1, -2, 0, 2));
        assert!(k.parent().children().contains(&k));
        assert_eq!(k.octant(), [1, 1, 1]);
        assert_eq!(NodeKey::of_block(-1, -64, 31), NodeKey::new(0, -1, -2, 0));
    }

    #[test]
    fn obere_lage_gewinnt_und_pflanzen_zaehlen_nicht() {
        let s = states();
        let mut kind = Node::empty();
        // Wasser oben, Stein unten -> Wasser.
        kind.set(0, 1, 0, voxel(2, 0));
        kind.set(0, 0, 0, voxel(1, 0));
        // Daneben: oben nur ein Halm, unten Erde -> Erde.
        kind.set(2, 1, 0, voxel(3, 0));
        kind.set(2, 0, 0, voxel(4, 7));
        // Luft mit Fackellicht.
        kind.set(4, 0, 0, with_light(0, 12, 3));
        let mut eltern = Node::empty();
        eltern.downsample_octant(&kind, [1, 0, 0], &s);
        assert_eq!(state_of(eltern.get(16, 0, 0)), 2);
        assert_eq!(state_of(eltern.get(17, 0, 0)), 4);
        assert_eq!(biome_of(eltern.get(17, 0, 0)), 7);
        assert!(is_air(eltern.get(18, 0, 0)));
        assert_eq!(light_of(eltern.get(18, 0, 0)), 12 << 4 | 15);
    }

    #[test]
    fn schneedecke_bleibt_zusammengefasst_flach() {
        let s = states();
        let mut kind = Node::empty();
        for z in 0..2 {
            for x in 0..2 {
                kind.set(x, 0, z, voxel(4, 0));
                kind.set(x, 1, z, voxel(5, 0));
            }
        }
        let mut eltern = Node::empty();
        eltern.downsample_octant(&kind, [0, 0, 0], &s);
        let v = eltern.get(0, 0, 0);
        assert_eq!(state_of(v), 5);
        // Erde (0..8) plus eine Lage Schnee (8..9) in Sechzehnteln des Eltern-Voxels.
        assert_eq!(unpack_shape(shape_bits(v)), [0, 0, 0, 16, 9, 16]);
    }

    #[test]
    fn kodieren_und_dekodieren() {
        let mut n = Node::empty();
        assert_eq!(Node::decode(&n.encode()).unwrap().voxels, n.voxels);
        n.set(1, 2, 3, voxel(1, 2));
        assert_eq!(Node::decode(&n.encode()).unwrap().voxels, n.voxels);
        for i in 0..400 {
            n.voxels[i * 7] = with_shape(voxel(i as u32, 1), i as u32);
        }
        let bytes = n.encode();
        assert_eq!(Node::decode(&bytes).unwrap().voxels, n.voxels);
        assert!(Node::decode(&bytes[..bytes.len() / 2]).is_none());
    }

    #[test]
    fn fassung_eins_bleibt_lesbar() {
        let mut raw = vec![1u8];
        raw.extend_from_slice(&2u32.to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&(7u32 | 3 << 16).to_le_bytes());
        raw.extend((0..VOLUME).map(|i| (i == 5) as u8));
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut enc, &raw).unwrap();
        let n = Node::decode(&enc.finish().unwrap()).unwrap();
        assert_eq!(n.voxels[0], AIR);
        assert_eq!(n.voxels[5], voxel(7, 3));
    }
}
