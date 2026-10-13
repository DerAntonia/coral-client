// Aus einem Knoten die Flaechen fuer die GPU.
//
// Je sichtbarer Voxelseite eine Flaeche, gleiche Nachbarn (gleicher State, Toenung, Form,
// Licht und Eckabdunklung) in derselben Ebene zu Rechtecken zusammengelegt. Eine Flaeche sind
// vier u32 (s. Quad), die Ecken baut erst der Vertex-Shader (render::lod_renderer).
//
// Form: jeder Voxel hat eine Huellbox (s. node). Eine Seite liegt auf deren Rand -- die
// Oberseite einer Schneedecke also 2/16 ueber dem Boden, ihre Seiten sind 2/16 hoch. Verdeckt
// ist eine Seite nur, wenn sie auf der Voxelgrenze liegt und der deckende Nachbar dort eine
// Seite hat, die sie ganz ueberdeckt.
//
// Eckabdunklung wie Vanillas weiche Beleuchtung (s. world::ambient_occlusion): je Ecke zaehlen
// die beiden Nachbarn in der Ebene vor der Flaeche, ihre Diagonale und der Voxel davor; jeder
// abdunkelnde Block darunter nimmt 0,2 Helligkeit (s. LodStates::darkens). So bekommen Kanten,
// Nischen und Hangfuesse auch in der Ferne ihre Tiefe. Die Staerke aus "Weiche Beleuchtung"
// rechnet der Shader ein, wie beim nahen Gelaende.
//
// Durchscheinendes (Wasser, Glas, Eis) zeigt Seiten zu allem, was nicht derselbe Block ist und
// nicht deckt -- Eis auf Wasser hat also eine Unterseite, und das Wasser darunter eine
// Oberflaeche, wie in Vanilla.
//
// Jenseits des Knotens gelten die sechs Nachbarknoten derselben Stufe (s. Neighbors): ihre
// Randlage entscheidet ueber Sichtbarkeit, Licht und Eckabdunklung wie im Inneren. Frueher
// wurde dort nichts nachgeschlagen -- durchscheinende Seiten am Knotenrand fehlten dann immer,
// und auf Stufe 1 liegt die Meeresoberflaeche (y 62/63) genau in der obersten Lage eines
// Knotens: das Meer war in diesem Entfernungsband durchsichtig; ausserdem fehlte an jeder Knotenkante die Abdunklung.
// Fehlt ein Nachbar (nie gespeichert), gilt das alte: deckende Voxel bekommen dort eine Seite,
// durchscheinende nicht (sonst stuende unter Wasser eine Wand). Eine Seite, die nicht auf der
// Voxelgrenze liegt (halbhohes Wasser, Schneedecke), haengt ohnehin nicht am Nachbarn.

use world::biome_palette::BiomePalette;

use std::sync::Arc;

use crate::node::{biome_of, index, light_of, state_of, unpack_shape, Node, Voxel, FULL_SHAPE, N};
use crate::states::{LodStates, CAT_OPAQUE, CAT_TRANSLUCENT, TINT_DRY_FOLIAGE, TINT_FOLIAGE, TINT_GRASS, TINT_NONE, TINT_WATER};

// Je Seite (Reihenfolge wie world::ambient_occlusion::FACE_DIRS: unten, oben, Norden, Sueden,
// Westen, Osten) die Normalenachse, ob sie in Plus-Richtung zeigt, und die beiden
// Flaechenachsen u und v. Muss mit lod.wgsl uebereinstimmen.
const FACES: [(usize, bool, usize, usize); 6] = [(1, false, 0, 2), (1, true, 0, 2), (2, false, 0, 1), (2, true, 0, 1), (0, false, 2, 1), (0, true, 2, 1)];

// Wort 0: x, y, z der Ecke (je 5 Bit), Seite (3 Bit), Breite - 1 und Hoehe - 1 (je 5 Bit).
// Wort 1: Block-State (16 Bit), Eckabdunklung (je Ecke 3 Bit = Zahl der abdunkelnden
//         Nachbarn, Ecken (0,0) (1,0) (0,1) (1,1) in u/v).
// Wort 2: Toenung RGB8 (sRGB, wie Vanillas Vertexfarbe), Licht davor (Blocklicht << 4 | Himmel).
// Wort 3: Huellbox (s. node::pack_shape).
pub type Quad = [u32; 4];

#[derive(Default)]
pub struct NodeMesh {
    pub opaque: Vec<Quad>,
    pub translucent: Vec<Quad>,
}

impl NodeMesh {
    pub fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.translucent.is_empty()
    }
}

fn pack_rgb(c: [f32; 3]) -> u32 {
    let k = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
    k(c[0]) | k(c[1]) << 8 | k(c[2]) << 16
}

const WHITE: u32 = 0xFF_FFFF;

fn tint(states: &LodStates, palette: &BiomePalette, state: u32, biome: u32, face: usize) -> u32 {
    let t = states.tint_of(state, face);
    if t == TINT_NONE {
        return WHITE;
    }
    let c = palette.colors_of(biome);
    pack_rgb(match t {
        TINT_GRASS => c.grass,
        TINT_FOLIAGE => c.foliage,
        TINT_DRY_FOLIAGE => c.dry_foliage,
        TINT_WATER => c.water,
        _ => [1.0; 3],
    })
}

// Die Nachbarknoten derselben Stufe in der Reihenfolge von FACES (unten, oben, Norden, Sueden,
// Westen, Osten), None = nicht gespeichert.
pub type Neighbors = [Option<Arc<Node>>; 6];

// Was je Voxel mehrfach gebraucht wird. Die Huellbox bleibt gepackt -- ausgepackt wird sie
// nur fuer sichtbare Flaechen (die meisten Voxel sind Luft oder liegen innen).
#[derive(Clone, Copy)]
struct Cell {
    cat: u8,
    state: u32,
    shape: u32,
}

// Welcher Nachbarknoten (Index in Neighbors) und welche Stelle darin zu `p` gehoert -- nur
// fuer Stellen, die genau eine Lage ueber eine Seite hinaus liegen. Kanten und Ecken (zwei
// Achsen draussen) kennt keiner der sechs.
fn neighbor_slot(p: [i32; 3]) -> Option<(usize, [usize; 3])> {
    let n = N as i32;
    let mut slot = None;
    let mut q = [0usize; 3];
    for a in 0..3 {
        let c = p[a];
        if (0..n).contains(&c) {
            q[a] = c as usize;
            continue;
        }
        if slot.is_some() || (c != -1 && c != n) {
            return None;
        }
        // Achse 0 = x (Westen/Osten), 1 = y (unten/oben), 2 = z (Norden/Sueden).
        let face = [4, 0, 2][a] + usize::from(c == n);
        q[a] = if c == n { 0 } else { N - 1 };
        slot = Some(face);
    }
    slot.map(|f| (f, q))
}

pub fn mesh_node(node: &Node, nachbarn: &Neighbors, states: &LodStates, palette: &BiomePalette) -> NodeMesh {
    let mut out = NodeMesh::default();
    let cell_of = |v: Voxel| {
        let state = state_of(v);
        let cat = states.category(state);
        let shape = if cat == CAT_OPAQUE || cat == CAT_TRANSLUCENT { states.shape_of(v) } else { 0 };
        Cell { cat, state, shape }
    };
    let cells: Vec<Cell> = node.voxels.iter().map(|&v| cell_of(v)).collect();
    // Der Voxel an `p`, auch eine Lage jenseits einer Seite (s. neighbor_slot).
    let voxel_at = |p: [i32; 3]| -> Option<Voxel> {
        if p.iter().all(|&c| (0..N as i32).contains(&c)) {
            return Some(node.get(p[0] as usize, p[1] as usize, p[2] as usize));
        }
        let (face, q) = neighbor_slot(p)?;
        Some(nachbarn[face].as_ref()?.get(q[0], q[1], q[2]))
    };
    let at = |p: [i32; 3]| -> Option<Cell> {
        if p.iter().all(|&c| (0..N as i32).contains(&c)) {
            return Some(cells[index(p[0] as usize, p[1] as usize, p[2] as usize)]);
        }
        voxel_at(p).map(cell_of)
    };
    // Dunkelt diese Stelle Ecken ab? Als Raster mit einer Zelle Rand ringsum (gefuellt aus den
    // Nachbarknoten), damit die zwoelf Abfragen je Flaeche ohne Bereichspruefung gehen.
    const P: usize = N + 2;
    let mut dunkel = vec![false; P * P * P];
    let n = N as i32;
    for y in -1..=n {
        for z in -1..=n {
            for x in -1..=n {
                let p = [x, y, z];
                let innen = p.iter().all(|&c| (0..N as i32).contains(&c));
                // Im Inneren jede Zelle, am Rand nur die sechs Seitenlagen (s. neighbor_slot).
                if !innen && neighbor_slot(p).is_none() {
                    continue;
                }
                if voxel_at(p).is_some_and(|v| states.darkens(v)) {
                    dunkel[(((y + 1) as usize * P + (z + 1) as usize) * P) + (x + 1) as usize] = true;
                }
            }
        }
    }
    let dunkelt = |p: [i32; 3]| dunkel[(((p[1] + 1) as usize * P + (p[2] + 1) as usize) * P) + (p[0] + 1) as usize];
    // Erster Durchgang in Speicherreihenfolge: je Voxel ein Byte mit seinen sichtbaren Seiten.
    // Die Flaechenschleife unten laeuft je Seite quer durch das Raster; dort nur noch dieses
    // Byte zu lesen statt der Zellen ist mehrfach schneller (gemessen).
    let sieht = |p: [i32; 3], a: Cell, face: usize| -> bool {
        let (na, plus, ua, va) = FACES[face];
        let mut q = p;
        q[na] += if plus { 1 } else { -1 };
        let Some(b) = at(q) else {
            // Unbekannt: deckend immer, durchscheinend nur, wenn die Seite gar nicht auf der
            // Voxelgrenze liegt (s. Modulkommentar).
            let form = unpack_shape(a.shape);
            let am_rand = if plus { form[na + 3] == 16 } else { form[na] == 0 };
            return a.cat == CAT_OPAQUE || !am_rand;
        };
        // Schneller Weg: zwei Vollwuerfel aneinander, der haeufigste Fall im Inneren.
        if b.cat == CAT_OPAQUE && b.shape == FULL_SHAPE && a.shape == FULL_SHAPE {
            return false;
        }
        if a.cat == CAT_TRANSLUCENT && b.state == a.state {
            return false;
        }
        if b.cat != CAT_OPAQUE {
            return true;
        }
        let (form, bf) = (unpack_shape(a.shape), unpack_shape(b.shape));
        let am_rand = if plus { form[na + 3] == 16 } else { form[na] == 0 };
        let deckt = am_rand
            && (if plus { bf[na] == 0 } else { bf[na + 3] == 16 })
            && bf[ua] <= form[ua]
            && bf[ua + 3] >= form[ua + 3]
            && bf[va] <= form[va]
            && bf[va + 3] >= form[va + 3];
        !deckt
    };
    let mut vis = vec![0u8; cells.len()];
    // Nachbar je Seite als Indexabstand (x 1, z N, y N*N) und ob er noch im Knoten liegt.
    const SCHRITT: [isize; 6] = [-((N * N) as isize), (N * N) as isize, -(N as isize), N as isize, -1, 1];
    for (i, &a) in cells.iter().enumerate() {
        if a.cat != CAT_OPAQUE && a.cat != CAT_TRANSLUCENT {
            continue;
        }
        let (x, y, z) = (i % N, i / (N * N), (i / N) % N);
        let innen = [y > 0, y + 1 < N, z > 0, z + 1 < N, x > 0, x + 1 < N];
        let voll = a.shape == FULL_SHAPE;
        for face in 0..6 {
            if voll && innen[face] {
                let b = cells[(i as isize + SCHRITT[face]) as usize];
                if b.cat == CAT_OPAQUE && b.shape == FULL_SHAPE {
                    continue;
                }
            }
            if sieht([x as i32, y as i32, z as i32], a, face) {
                vis[i] |= 1 << face;
            }
        }
    }
    let mut mask = vec![0u128; N * N];
    for (face, &(na, plus, ua, va)) in FACES.iter().enumerate() {
        for s in 0..N {
            let mut any = false;
            for v in 0..N {
                for u in 0..N {
                    let mut p = [0i32; 3];
                    p[na] = s as i32;
                    p[ua] = u as i32;
                    p[va] = v as i32;
                    let i = index(p[0] as usize, p[1] as usize, p[2] as usize);
                    mask[v * N + u] = 0;
                    if vis[i] & (1 << face) == 0 {
                        continue;
                    }
                    let a = cells[i];
                    let mut q = p;
                    q[na] += if plus { 1 } else { -1 };
                    let nachbar = voxel_at(q);
                    let form = unpack_shape(a.shape);
                    let am_rand = if plus { form[na + 3] == 16 } else { form[na] == 0 };
                    // Die Lage, deren Nachbarn abdunkeln: vor der Flaeche, wenn sie auf der
                    // Voxelgrenze liegt, sonst die eigene (eine eingerueckte Schneeoberseite hat
                    // ihre Nachbarn neben sich). Ist davor nichts bekannt (Nachbarknoten nie
                    // gespeichert), ohne Abdunklung und mit Himmelslicht.
                    let mut ao = 0u32;
                    let mut licht = 15u8;
                    if (nachbar.is_some() || !am_rand) && !states.no_ao(a.state) {
                        let f = if am_rand { q } else { p };
                        for cv in 0..2 {
                            for cu in 0..2 {
                                let (du, dv) = (cu * 2 - 1, cv * 2 - 1);
                                let mut s1 = f;
                                s1[ua] += du;
                                let mut s2 = f;
                                s2[va] += dv;
                                let mut d = s1;
                                d[va] += dv;
                                let k = [s1, s2, d, f].iter().filter(|&&c| dunkelt(c)).count() as u32;
                                ao |= k << (3 * (cu + 2 * cv) as u32);
                            }
                        }
                    }
                    if let Some(v) = nachbar {
                        licht = light_of(v);
                    }
                    let vox = node.voxels[i];
                    let t = tint(states, palette, a.state, biome_of(vox), face);
                    let shape = a.shape;
                    mask[v * N + u] = 1
                        | (a.state as u128) << 1
                        | (t as u128) << 17
                        | (licht as u128) << 41
                        | (ao as u128) << 49
                        | (shape as u128) << 61
                        | (u128::from(a.cat == CAT_TRANSLUCENT)) << 91;
                    any = true;
                }
            }
            if !any {
                continue;
            }
            // Gierig zu Rechtecken zusammenlegen: erst so breit wie moeglich, dann so hoch,
            // wie die ganze Breite mitgeht. In eine Richtung nur, wenn die Form dort die ganze
            // Zelle fuellt und die Eckabdunklung in dieser Richtung gleich bleibt -- sonst
            // stimmte die Form bzw. der Verlauf ueber das Rechteck nicht mehr.
            for v in 0..N {
                let mut u = 0;
                while u < N {
                    let key = mask[v * N + u];
                    if key == 0 {
                        u += 1;
                        continue;
                    }
                    let b = unpack_shape(((key >> 61) & 0x3FFF_FFFF) as u32);
                    let ao = ((key >> 49) & 0xFFF) as u32;
                    let ecke = |cu: u32, cv: u32| (ao >> (3 * (cu + 2 * cv))) & 7;
                    let breit_ok = b[ua] == 0 && b[ua + 3] == 16 && ecke(0, 0) == ecke(1, 0) && ecke(0, 1) == ecke(1, 1);
                    let hoch_ok = b[va] == 0 && b[va + 3] == 16 && ecke(0, 0) == ecke(0, 1) && ecke(1, 0) == ecke(1, 1);
                    let mut w = 1;
                    while breit_ok && u + w < N && mask[v * N + u + w] == key {
                        w += 1;
                    }
                    let mut h = 1;
                    'hoch: while hoch_ok && v + h < N {
                        for k in 0..w {
                            if mask[(v + h) * N + u + k] != key {
                                break 'hoch;
                            }
                        }
                        h += 1;
                    }
                    for dv in 0..h {
                        for k in 0..w {
                            mask[(v + dv) * N + u + k] = 0;
                        }
                    }
                    let mut p = [0usize; 3];
                    p[na] = s;
                    p[ua] = u;
                    p[va] = v;
                    let w0 = p[0] as u32 | (p[1] as u32) << 5 | (p[2] as u32) << 10 | (face as u32) << 15 | ((w - 1) as u32) << 18 | ((h - 1) as u32) << 23;
                    let state = ((key >> 1) & 0xFFFF) as u32;
                    let tint = ((key >> 17) & 0xFF_FFFF) as u32;
                    let licht = ((key >> 41) & 0xFF) as u32;
                    let quad = [w0, state | ao << 16, tint | licht << 24, ((key >> 61) & 0x3FFF_FFFF) as u32];
                    if key >> 91 != 0 {
                        out.translucent.push(quad);
                    } else {
                        out.opaque.push(quad);
                    }
                    u += w;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{pack_shape, voxel};
    use crate::states::{CAT_AIR, CAT_PLANT};

    fn states() -> LodStates {
        // 1 Stein, 2 Wasser, 3 Halm, 4 Erde, 5 Schneedecke (2/16), 6 Eis.
        let mut s = LodStates::from_categories(vec![CAT_AIR, CAT_OPAQUE, CAT_TRANSLUCENT, CAT_PLANT, CAT_OPAQUE, CAT_OPAQUE, CAT_TRANSLUCENT]);
        s.set_shape(5, pack_shape([0, 0, 0, 16, 2, 16]));
        s
    }

    fn seite(q: &Quad) -> u32 {
        (q[0] >> 15) & 7
    }

    fn flaeche(q: &Quad) -> u32 {
        (((q[0] >> 18) & 31) + 1) * (((q[0] >> 23) & 31) + 1)
    }

    #[test]
    fn einzelner_block_hat_sechs_seiten() {
        let mut n = Node::empty();
        n.set(3, 4, 5, voxel(1, 0));
        let m = mesh_node(&n, &Default::default(), &states(), &BiomePalette::fallback());
        assert_eq!(m.opaque.len(), 6);
        for f in 0..6 {
            assert!(m.opaque.iter().any(|q| seite(q) == f));
        }
        assert!(m.opaque.iter().all(|q| q[0] & 0x7FFF == 3 | 4 << 5 | 5 << 10));
        // Frei in der Luft: nichts dunkelt ab.
        assert!(m.opaque.iter().all(|q| (q[1] >> 16) & 0xFFF == 0));
    }

    #[test]
    fn boden_wird_zusammengelegt_und_innen_bleibt_nichts() {
        let mut n = Node::empty();
        for z in 0..N {
            for x in 0..N {
                for y in 0..4 {
                    n.set(x, y, z, voxel(1, 0));
                }
                // Ein Halm obendrauf verdeckt nichts.
                n.set(x, 4, z, voxel(3, 0));
            }
        }
        let m = mesh_node(&n, &Default::default(), &states(), &BiomePalette::fallback());
        let oben: Vec<&Quad> = m.opaque.iter().filter(|q| seite(q) == 1).collect();
        assert_eq!(oben.len(), 1);
        assert_eq!(flaeche(oben[0]), (N * N) as u32);
        assert_eq!(m.opaque.len(), 6);
        assert!(m.translucent.is_empty());
    }

    #[test]
    fn wasser_nur_zur_luft_und_nicht_am_rand() {
        let mut n = Node::empty();
        for z in 0..N {
            for x in 0..N {
                n.set(x, 0, z, voxel(4, 0));
                n.set(x, 1, z, voxel(2, 0));
            }
        }
        let m = mesh_node(&n, &Default::default(), &states(), &BiomePalette::fallback());
        assert_eq!(m.translucent.len(), 1);
        assert_eq!(seite(&m.translucent[0]), 1);
        // Der Boden unter dem Wasser bleibt sichtbar (durchs Wasser hindurch).
        assert_eq!(m.opaque.iter().filter(|q| seite(q) == 1).count(), 1);
    }

    #[test]
    fn eis_auf_wasser_hat_beide_grenzflaechen() {
        let mut n = Node::empty();
        n.set(5, 5, 5, voxel(2, 0));
        n.set(5, 6, 5, voxel(6, 0));
        let m = mesh_node(&n, &Default::default(), &states(), &BiomePalette::fallback());
        // Wasser: Oberseite zum Eis; Eis: Unterseite zum Wasser -- plus die Seiten zur Luft.
        let wasser_oben = m.translucent.iter().filter(|q| q[1] & 0xFFFF == 2 && seite(q) == 1).count();
        let eis_unten = m.translucent.iter().filter(|q| q[1] & 0xFFFF == 6 && seite(q) == 0).count();
        assert_eq!((wasser_oben, eis_unten), (1, 1));
    }

    #[test]
    fn schneedecke_ist_flach_und_verdeckt_den_boden() {
        let mut n = Node::empty();
        n.set(5, 5, 5, voxel(4, 0));
        n.set(5, 6, 5, voxel(5, 0));
        let m = mesh_node(&n, &Default::default(), &states(), &BiomePalette::fallback());
        // Die Erde hat keine Oberseite mehr, der Schnee alle sechs Seiten mit seiner Form.
        assert!(!m.opaque.iter().any(|q| q[1] & 0xFFFF == 4 && seite(q) == 1));
        let schnee: Vec<&Quad> = m.opaque.iter().filter(|q| q[1] & 0xFFFF == 5).collect();
        assert_eq!(schnee.len(), 5);
        assert!(schnee.iter().all(|q| unpack_shape(q[3]) == [0, 0, 0, 16, 2, 16]));
        // Die Erde verdeckt die Unterseite des Schnees.
        assert!(!schnee.iter().any(|q| seite(q) == 0));
    }

    // Sonst werden Oberflaechen manchmal einfach unsichtbar. Auf Stufe 1 liegt die
    // Meeresoberflaeche in der obersten Lage eines Knotens, als halbhoher Wasser-Voxel (Wasser
    // bei y 62, Luft bei 63). Seine Oberseite liegt im Voxel und haengt nicht am Nachbarn --
    // sie fehlte trotzdem, weil jenseits des Knotens nichts nachgeschlagen wurde.
    #[test]
    fn meer_in_der_obersten_lage_hat_eine_oberflaeche() {
        let mut s = states();
        s.set_shape(2, pack_shape([0, 0, 0, 16, 8, 16]));
        let mut n = Node::empty();
        for z in 0..N {
            for x in 0..N {
                n.set(x, N - 2, z, voxel(4, 0));
                n.set(x, N - 1, z, voxel(2, 0));
            }
        }
        // Ohne Nachbarn (nie gespeichert): die eingerueckte Oberseite bleibt sichtbar.
        let m = mesh_node(&n, &Default::default(), &s, &BiomePalette::fallback());
        assert_eq!(m.translucent.iter().filter(|q| seite(q) == 1).count(), 1, "Meeresoberflaeche fehlt");

        // Volles Wasser bis zur Knotenkante, darueber ein gespeicherter Nachbar mit Luft: die
        // Oberflaeche ist da. Mit Wasser darueber nicht (keine Wand im Wasser).
        let mut n = Node::empty();
        for z in 0..N {
            for x in 0..N {
                n.set(x, N - 1, z, voxel(2, 0));
            }
        }
        let mut oben: Neighbors = Default::default();
        oben[1] = Some(Arc::new(Node::empty()));
        let m = mesh_node(&n, &oben, &states(), &BiomePalette::fallback());
        assert_eq!(m.translucent.iter().filter(|q| seite(q) == 1).count(), 1);
        let mut wasser = Node::empty();
        for z in 0..N {
            for x in 0..N {
                wasser.set(x, 0, z, voxel(2, 0));
            }
        }
        oben[1] = Some(Arc::new(wasser));
        let m = mesh_node(&n, &oben, &states(), &BiomePalette::fallback());
        assert_eq!(m.translucent.iter().filter(|q| seite(q) == 1).count(), 0);
    }

    // Die Abdunklung reicht ueber die Knotengrenze: eine Stufe im Nachbarknoten dunkelt den
    // Boden am Rand ab, genau wie innerhalb eines Knotens.
    #[test]
    fn abdunklung_ueber_die_knotengrenze() {
        let mut n = Node::empty();
        for z in 0..N {
            n.set(N - 1, 0, z, voxel(1, 0));
        }
        let mut ost = Node::empty();
        for z in 0..N {
            ost.set(0, 0, z, voxel(1, 0));
            ost.set(0, 1, z, voxel(1, 0));
        }
        let mut nachbarn: Neighbors = Default::default();
        nachbarn[5] = Some(Arc::new(ost));
        let m = mesh_node(&n, &nachbarn, &states(), &BiomePalette::fallback());
        let boden = m.opaque.iter().find(|q| seite(q) == 1).expect("Boden");
        let ao = (boden[1] >> 16) & 0xFFF;
        assert!((ao >> 3) & 7 > ao & 7, "Ecken zur Stufe im Nachbarknoten muessen dunkler sein: {ao:o}");
        // Die Ostseite des Bodens liegt am festen Nachbarn an und faellt weg.
        assert!(!m.opaque.iter().any(|q| seite(q) == 5));
    }

    #[test]
    fn ecke_am_hangfuss_wird_dunkler() {
        // Boden mit einer Stufe: die Bodenflaeche direkt vor der Stufe dunkelt an der Stufe ab.
        let mut n = Node::empty();
        for z in 0..4 {
            for x in 0..4 {
                n.set(x, 0, z, voxel(1, 0));
            }
            n.set(4, 1, z, voxel(1, 0));
            n.set(4, 0, z, voxel(1, 0));
        }
        let m = mesh_node(&n, &Default::default(), &states(), &BiomePalette::fallback());
        let boden = m.opaque.iter().find(|q| seite(q) == 1 && (q[0] & 31) == 3 && ((q[0] >> 5) & 31) == 0).expect("Bodenflaeche vor der Stufe");
        let ao = (boden[1] >> 16) & 0xFFF;
        // Die Ecken auf der Seite der Stufe (u = 1) sind dunkler als die anderen.
        assert!((ao >> 3) & 7 > ao & 7, "{ao:o}");
    }
}
