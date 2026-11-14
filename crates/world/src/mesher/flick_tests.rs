// Flicken (s. terrain_vertex, "Flicken"): nach jeder Blockaenderung muessen die geflickten
// Flaechen einer Spalte genau die eines ganzen Neubaus sein.

use super::*;
use crate::terrain_vertex::{layout_patchable, pack_terrain, place_quads, plan_patch, PlacedQuad, TerrainVertex, FREE_QUAD};

const STEIN: u32 = 1;

struct Zufall(u64);

impl Zufall {
    fn naechste(&mut self, grenze: u32) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as u32) % grenze
    }
}

// Drei mal drei Spalten, in den unteren zwei Sections zufaellig mit Stein gefuellt.
fn welt(z: &mut Zufall) -> World {
    let mut w = World::new();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut sections = Vec::new();
            for i in 0..4 {
                let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
                if i < 2 {
                    for s in &mut states {
                        if z.naechste(3) == 0 {
                            *s = STEIN;
                        }
                    }
                }
                sections.push(crate::chunk::ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

fn mesh(n: &ColumnNeighbors, region: Option<&MeshRegion>) -> (Vec<PlacedQuad>, usize) {
    let (opak, durchsichtig, anfaenge) = mesh_column_blocks(
        0,
        0,
        n,
        &BlockUvs::single_tile(),
        &BlockShapes::all_full_cubes(),
        &BiomePalette::fallback(),
        &CrossBlocks::none(),
        &BlockShapes::all_full_cubes(),
        1.0,
        false,
        region,
    );
    let mut starts: Vec<u32> = anfaenge.iter().map(|a| a.0).collect();
    let packed = pack_terrain(&opak, &mut starts, n.target.min_y);
    let bloecke: Vec<(u32, u32)> = starts.iter().zip(&anfaenge).map(|(s, a)| (*s, a.1)).collect();
    (place_quads(&packed, &bloecke), durchsichtig.len())
}

// Die belegten Flaechen eines Puffers als sortierte Liste (Block, Vertices).
fn belegt(vertices: &[TerrainVertex], quad_block: &[u32]) -> Vec<(u32, Vec<u8>)> {
    let mut out: Vec<(u32, Vec<u8>)> = vertices
        .chunks_exact(4)
        .zip(quad_block)
        .filter(|(_, &b)| b != FREE_QUAD)
        .map(|(q, &b)| (b, bytemuck::cast_slice::<TerrainVertex, u8>(q).to_vec()))
        .collect();
    out.sort();
    out
}

#[test]
fn geflickt_gleicht_dem_neubau() {
    let mut z = Zufall(7);
    let mut w = welt(&mut z);
    let min_y = w.column(0, 0).unwrap().min_y;
    let n = w.column_neighbors(0, 0).unwrap();
    let (mut vertices, _, mut runs, mut quad_block) = layout_patchable(&mesh(&n, None).0, 0b1111);
    let (mut geflickt, mut neu_gebaut) = (0, 0);
    for _ in 0..120 {
        // Eine Stelle in der Spalte oder am Rand der Nachbarn umschalten (Stein <-> Luft).
        let (x, y, zz) = (z.naechste(18) as i32 - 1, min_y + z.naechste(34) as i32, z.naechste(18) as i32 - 1);
        let vorher = w.block_at(x, y, zz);
        w.set_block(x, y, zz, if vorher == STEIN { 0 } else { STEIN });
        let n = w.column_neighbors(0, 0).unwrap();
        let region = MeshRegion::around(0, 0, min_y, n.target.sections.len(), &[(x, y, zz)]);
        let (added, _) = mesh(&n, Some(&region));
        match plan_patch(&runs, &quad_block, region.blocks(), &added) {
            Some(ziel) => {
                geflickt += 1;
                for (platz, block) in quad_block.iter_mut().enumerate() {
                    if *block != FREE_QUAD && region.contains(*block) {
                        *block = FREE_QUAD;
                        vertices[platz * 4..platz * 4 + 4].fill(bytemuck::Zeroable::zeroed());
                    }
                }
                for (q, &platz) in added.iter().zip(&ziel) {
                    let platz = platz as usize;
                    quad_block[platz] = q.block;
                    vertices[platz * 4..platz * 4 + 4].copy_from_slice(&q.verts);
                    let lauf = runs.iter().find(|r| (r.first_quad..r.first_quad + r.quads).contains(&(platz as u32))).unwrap();
                    assert_eq!((lauf.group, lauf.section), (q.group, q.section), "Flaeche im falschen Lauf");
                }
            }
            None => {
                neu_gebaut += 1;
                (vertices, _, runs, quad_block) = layout_patchable(&mesh(&n, None).0, 0b1111);
            }
        }
        let (ganz, _) = mesh(&n, None);
        let (v, _, _, b) = layout_patchable(&ganz, 0);
        assert_eq!(belegt(&vertices, &quad_block), belegt(&v, &b), "nach Aenderung bei ({x},{y},{zz})");
    }
    assert!(geflickt > 90, "die meisten Aenderungen muessen sich flicken lassen ({geflickt} geflickt, {neu_gebaut} neu gebaut)");
}

// Der Stand vorher (s. ColumnNeighbors::with_blocks) meldet Durchsichtiges, das es jetzt nicht
// mehr gibt -- daran erkennt die App, dass sie nicht flicken darf.
#[test]
fn vorher_zeigt_abgebautes_wasser() {
    let wasser = (0..crate::block_registry::state_count() as u32).find(|&s| block_name(s) == Some("water")).unwrap();
    let mut z = Zufall(3);
    let mut w = welt(&mut z);
    let min_y = w.column(0, 0).unwrap().min_y;
    let pos = (5, min_y + 40, 5);
    w.set_block(pos.0, pos.1, pos.2, wasser);
    let alt = w.block_at(pos.0, pos.1, pos.2);
    w.set_block(pos.0, pos.1, pos.2, 0);
    let n = w.column_neighbors(0, 0).unwrap();
    let region = MeshRegion::around(0, 0, min_y, n.target.sections.len(), &[pos]);
    assert_eq!(mesh(&n, Some(&region)).1, 0, "jetzt ist dort nur Luft");
    let vorher = n.with_blocks(0, 0, &[(pos, alt)]);
    assert!(mesh(&vorher, Some(&region)).1 > 0, "vorher stand dort Wasser");
}
