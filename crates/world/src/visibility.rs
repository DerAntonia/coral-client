// Sichtverbindungen innerhalb einer Section (16x16x16) -- die Grundlage des Hoehlen-Cullings
// (s. render::occlusion).
//
// Fuer jede Section wird festgehalten, zwischen welchen ihrer sechs Seiten eine Verbindung
// durch nicht blickdichte Zellen besteht. Eine Section voller Stein verbindet keine Seiten,
// eine Hoehle darin nur die Seiten, die sie tatsaechlich beruehrt. Von der Kamera aus laeuft
// dann eine Breitensuche durch die Sections, die nur dort weitergeht, wo man durch eine
// Section hindurchsehen kann -- was sie nie erreicht, ist von hier aus unsichtbar.
//
// Seiten-Index wie assets::FACE_*: 0 unten, 1 oben, 2 Norden (-Z), 3 Sueden (+Z), 4 Westen
// (-X), 5 Osten (+X).

use crate::block_registry::block_name;
use crate::block_shapes::BlockShapes;
use crate::chunk::{is_solid, ChunkSection, SECTION_SIZE};

// Bit a*6+b: von Seite a kommt man durch die Section zu Seite b (symmetrisch gesetzt).
pub type SectionVis = u64;
pub const VIS_ALL: SectionVis = (1 << 36) - 1;

pub fn sees_through(vis: SectionVis, from: usize, to: usize) -> bool {
    vis & (1 << (from * 6 + to)) != 0
}

// Versperrt dieser State die Sicht vollstaendig? Nur ein echter, ringsum geschlossener
// Vollwuerfel, der im Opak-Pass liegt und keine durchsichtigen Stellen hat. Im Zweifel nein:
// ein faelschliches "ja" macht Dinge unsichtbar, die man sehen muesste, ein faelschliches
// "nein" kostet nur etwas Leistung.
pub(crate) fn blocks_view(shapes: &BlockShapes, state: u32) -> bool {
    if !is_solid(state) || !shapes.has_resolved_shape(state) || shapes.model_quads(state).is_some() {
        return false;
    }
    let Some(name) = block_name(state) else { return false };
    let durchsichtig = crate::biome_tint::is_water(name)
        || matches!(
            name,
            "lava" | "bubble_column" | "ice" | "frosted_ice" | "slime_block" | "honey_block" | "mangrove_roots" | "beacon"
                | "powder_snow"
        )
        || crate::mesher::is_glass(name)
        || crate::mesher::is_leaves(name)
        || crate::mesher::is_marker_block(name)
        || crate::mesher::never_occludes_neighbors(name);
    if durchsichtig {
        return false;
    }
    const EPS: f32 = 1e-4;
    shapes.shapes_of(state).iter().any(|b| {
        b.min.iter().all(|&v| v <= EPS) && b.max.iter().all(|&v| v >= 1.0 - EPS) && b.faces.iter().all(|&f| f)
    })
}

const N: usize = SECTION_SIZE as usize;
const VOLUME: usize = N * N * N;

// Welche Seiten der Section die Zelle beruehrt, als Bitmaske.
fn faces_of(x: usize, y: usize, z: usize) -> u8 {
    let mut m = 0;
    if y == 0 {
        m |= 1 << 0;
    }
    if y == N - 1 {
        m |= 1 << 1;
    }
    if z == 0 {
        m |= 1 << 2;
    }
    if z == N - 1 {
        m |= 1 << 3;
    }
    if x == 0 {
        m |= 1 << 4;
    }
    if x == N - 1 {
        m |= 1 << 5;
    }
    m
}

fn pairs(faces: u8) -> SectionVis {
    let mut v = 0;
    for a in 0..6 {
        if faces & (1 << a) == 0 {
            continue;
        }
        for b in 0..6 {
            if faces & (1 << b) != 0 {
                v |= 1 << (a * 6 + b);
            }
        }
    }
    v
}

// Wie Vanillas ChunkOcclusionDataBuilder: von jeder offenen Randzelle aus fuellen und alle
// Seiten, die dieselbe Luftblase beruehrt, miteinander verbinden. Weniger als 256 dichte
// Zellen koennen keine 16x16-Wand bilden -- dann ist ohne Rechnung alles verbunden.
pub fn section_visibility(section: &ChunkSection, blocks: &dyn Fn(u32) -> bool) -> SectionVis {
    let mut closed = vec![false; VOLUME];
    let mut dicht = 0;
    for y in 0..N {
        for z in 0..N {
            for x in 0..N {
                if blocks(section.get(x as i32, y as i32, z as i32)) {
                    closed[(y * N + z) * N + x] = true;
                    dicht += 1;
                }
            }
        }
    }
    if dicht < 256 {
        return VIS_ALL;
    }
    if dicht == VOLUME {
        return 0;
    }
    let mut vis = 0;
    let mut stack = Vec::new();
    for y in 0..N {
        for z in 0..N {
            for x in 0..N {
                let start = (y * N + z) * N + x;
                if faces_of(x, y, z) == 0 || closed[start] {
                    continue;
                }
                // Gefuellte Zellen gelten ab hier als besucht.
                closed[start] = true;
                stack.push(start);
                let mut faces = 0u8;
                while let Some(i) = stack.pop() {
                    let (x, z, y) = (i % N, (i / N) % N, i / (N * N));
                    faces |= faces_of(x, y, z);
                    let mut nachbar = |j: usize| {
                        if !closed[j] {
                            closed[j] = true;
                            stack.push(j);
                        }
                    };
                    if x > 0 {
                        nachbar(i - 1);
                    }
                    if x < N - 1 {
                        nachbar(i + 1);
                    }
                    if z > 0 {
                        nachbar(i - N);
                    }
                    if z < N - 1 {
                        nachbar(i + N);
                    }
                    if y > 0 {
                        nachbar(i - N * N);
                    }
                    if y < N - 1 {
                        nachbar(i + N * N);
                    }
                }
                vis |= pairs(faces);
            }
        }
    }
    vis
}

// Alle Sections einer Spalte, von unten nach oben. Leere Sections sind ohne Rechnung offen.
pub fn column_visibility(sections: &[ChunkSection], shapes: &BlockShapes) -> Vec<SectionVis> {
    let blocks = |s: u32| shapes.blocks_view(s);
    sections.iter().map(|s| if s.is_empty() { VIS_ALL } else { section_visibility(s, &blocks) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(fill: impl Fn(usize, usize, usize) -> bool) -> ChunkSection {
        let mut states = vec![0u32; VOLUME];
        for y in 0..N {
            for z in 0..N {
                for x in 0..N {
                    if fill(x, y, z) {
                        states[(y * N + z) * N + x] = 1;
                    }
                }
            }
        }
        ChunkSection::from_states(states)
    }

    fn dicht(s: u32) -> bool {
        s == 1
    }

    #[test]
    fn voller_stein_verbindet_nichts_luft_alles() {
        assert_eq!(section_visibility(&section(|_, _, _| true), &dicht), 0);
        assert_eq!(section_visibility(&section(|_, _, _| false), &dicht), VIS_ALL);
    }

    #[test]
    // Eine waagerechte Platte trennt oben von unten, die Seiten bleiben je Haelfte verbunden.
    fn platte_trennt_oben_und_unten() {
        let v = section_visibility(&section(|_, y, _| y == 8), &dicht);
        assert!(!sees_through(v, 0, 1) && !sees_through(v, 1, 0));
        assert!(sees_through(v, 0, 4) && sees_through(v, 1, 5) && sees_through(v, 2, 3));
    }

    #[test]
    // Ein Gang von West nach Ost durch massiven Stein: nur diese beiden Seiten.
    fn gang_verbindet_nur_seine_enden() {
        let v = section_visibility(&section(|_, y, z| !(y == 5 && z == 5)), &dicht);
        assert!(sees_through(v, 4, 5) && sees_through(v, 5, 4));
        for (a, b) in [(0, 1), (2, 3), (0, 4), (1, 5), (2, 4)] {
            assert!(!sees_through(v, a, b), "{a}->{b}");
        }
    }

    #[test]
    fn wenige_dichte_bloecke_lassen_alles_offen() {
        assert_eq!(section_visibility(&section(|x, y, _| x == 3 && y < 15), &dicht), VIS_ALL);
    }
}
