// Eine Spalte meshen, mit AO-Ueberblendung, und Desync-Animationen.

use super::*;

// Wie mesh_column_with_lighting, dazu fuer jede Flaeche, deren Eckabdunklung von einer der
// `fades` abhaengt, die Fassung "wie vorher" (s. AoFadeQuad) -- damit blendet die App den
// Schatten nach einer Platzierung weich ein, statt ihn springen zu lassen. Die Flaechen der
// geaenderten Stelle selbst waren vorher der flache Geisterblock: ihr "vorher" ist ohne
// Abdunklung.
#[allow(clippy::too_many_arguments)]
pub fn mesh_column_ao_fading(
    chunk_x: i32,
    chunk_z: i32,
    neighbors: &ColumnNeighbors,
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    biomes: &BiomePalette,
    cross: &CrossBlocks,
    cross_extra_shapes: &BlockShapes,
    ao_strength: f32,
    show_markers: bool,
    fades: &[AoFadeSource],
) -> (Vec<BlockVertex>, Vec<BlockVertex>, Vec<AoFadeQuad>) {
    mesh_column_impl(chunk_x, chunk_z, neighbors, uvs, shapes, biomes, cross, cross_extra_shapes, ao_strength, show_markers, fades, None, None)
}

// Die Bloecke einer Spalte, auf die sich ein Teil-Meshen beschraenkt (s. mesh_column_blocks):
// Block-Kennungen wie block_id, aufsteigend sortiert.
#[derive(Clone, Debug, Default)]
pub struct MeshRegion {
    blocks: Vec<u32>,
}

// Kennung eines Blocks innerhalb seiner Spalte: x und z spaltenlokal (0..16), y ab min_y.
#[must_use]
pub const fn block_id(local_x: i32, y_ab_min: i32, local_z: i32) -> u32 {
    (local_x as u32) | (local_z as u32) << 4 | (y_ab_min as u32) << 8
}

impl MeshRegion {
    // Alle Bloecke der Spalte (chunk_x, chunk_z), die hoechstens einen Block (auch diagonal)
    // von einer der `positions` (Weltkoordinaten) entfernt liegen -- so weit reicht eine
    // Blockaenderung: die Flaechen der sechs Nachbarn und die Eckabdunklung ringsum.
    #[must_use]
    pub fn around(chunk_x: i32, chunk_z: i32, min_y: i32, sections: usize, positions: &[(i32, i32, i32)]) -> Self {
        let (base_x, base_z) = (chunk_x * SECTION_SIZE, chunk_z * SECTION_SIZE);
        let hoehe = sections as i32 * SECTION_SIZE;
        let mut blocks = Vec::with_capacity(positions.len() * 27);
        for &(px, py, pz) in positions {
            for y in py - 1..=py + 1 {
                for z in pz - 1..=pz + 1 {
                    for x in px - 1..=px + 1 {
                        let (lx, ly, lz) = (x - base_x, y - min_y, z - base_z);
                        if (0..SECTION_SIZE).contains(&lx) && (0..SECTION_SIZE).contains(&lz) && (0..hoehe).contains(&ly) {
                            blocks.push(block_id(lx, ly, lz));
                        }
                    }
                }
            }
        }
        blocks.sort_unstable();
        blocks.dedup();
        Self { blocks }
    }

    #[must_use]
    pub fn contains(&self, id: u32) -> bool {
        self.blocks.binary_search(&id).is_ok()
    }

    #[must_use]
    pub fn blocks(&self) -> &[u32] {
        &self.blocks
    }

    fn touches_section(&self, index: usize) -> bool {
        let (von, bis) = ((index as u32 * SECTION_SIZE as u32) << 8, ((index as u32 + 1) * SECTION_SIZE as u32) << 8);
        let i = self.blocks.partition_point(|&b| b < von);
        self.blocks.get(i).is_some_and(|&b| b < bis)
    }
}

// Nur die Bloecke aus `region` meshen, mit derselben Umgebung wie beim ganzen Meshen (Nachbarn
// entscheiden ueber verdeckte Flaechen und die Eckabdunklung). Gibt opake und durchsichtige
// Vertices zurueck und fuer jeden gemeshten Block (Anfang in den opaken Vertices, block_id),
// in Reihenfolge der Vertices: die opaken Flaechen eines Blocks reichen von seinem Anfang bis
// zum Anfang des naechsten.
#[allow(clippy::too_many_arguments)]
pub fn mesh_column_blocks(
    chunk_x: i32,
    chunk_z: i32,
    neighbors: &ColumnNeighbors,
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    biomes: &BiomePalette,
    cross: &CrossBlocks,
    cross_extra_shapes: &BlockShapes,
    ao_strength: f32,
    show_markers: bool,
    region: Option<&MeshRegion>,
) -> (Vec<BlockVertex>, Vec<BlockVertex>, Vec<(u32, u32)>) {
    let mut starts = Vec::new();
    let (opaque, translucent, _) =
        mesh_column_impl(chunk_x, chunk_z, neighbors, uvs, shapes, biomes, cross, cross_extra_shapes, ao_strength, show_markers, &[], region, Some(&mut starts));
    (opaque, translucent, starts)
}

#[allow(clippy::too_many_arguments)]
fn mesh_column_impl(
    chunk_x: i32,
    chunk_z: i32,
    neighbors: &ColumnNeighbors,
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    biomes: &BiomePalette,
    cross: &CrossBlocks,
    cross_extra_shapes: &BlockShapes,
    ao_strength: f32,
    show_markers: bool,
    fades: &[AoFadeSource],
    region: Option<&MeshRegion>,
    mut block_starts: Option<&mut Vec<(u32, u32)>>,
) -> (Vec<BlockVertex>, Vec<BlockVertex>, Vec<AoFadeQuad>) {
    let mut fade_quads: Vec<AoFadeQuad> = Vec::new();
    let ao_strength = ao_strength.clamp(0.0, 1.0);
    let base_x = chunk_x * SECTION_SIZE;
    let base_z = chunk_z * SECTION_SIZE;
    // Alle Vertex-Positionen dieser Spalte beziehen sich hierauf (s. ColumnOrigin).
    let origin = ColumnOrigin::of_column(chunk_x, chunk_z);

    // Block an absoluten Weltkoordinaten -- wie World::block_at, aber beschraenkt auf die
    // 9 mitgegebenen Spalten statt einer ganzen HashMap-Lookup-Kette. (x,z) liegt hoechstens
    // 1 Block ausserhalb der Zielspalte auf JEDER Achse -- die vier fruehen Faelle unten
    // muessen deshalb VOR den einachsigen Faellen geprueft werden, sonst wuerde ein Aufruf,
    // der BEIDE Achsen gleichzeitig verlaesst (z.B. Wassers Eck-Hoehen-Mittelung an einer
    // Spalten-ECKE, s. push_fluid_quads), faelschlich in einen orthogonalen Nachbarn
    // hineinindizieren -- der dortige z-base_z/x-base_x-Term waere dann negativ und hat
    // frueher zu einem Index-Panic gefuehrt.
    // Die Untergrenze kommt von der Spalte selbst, nicht aus einer Konstante: sie haengt an
    // der Dimension (s. chunk::DEFAULT_MIN_Y).
    let welt_min_y = neighbors.target.min_y;
    let block_at = |x: i32, y: i32, z: i32| -> u32 {
        // Unterhalb der Welt existiert keine Section -- fuer y < min_y wuerde
        // ChunkColumn::get() "Luft" liefern, wodurch die unterste Blockreihe jeder
        // Spalte (meist Bedrock/Deepslate) eine Boden-Flaeche bekaeme. Die ist aber nie
        // sichtbar: dieser Client kennt keinen Weg unter die Welt (kein Void-Fall durch
        // die Bodenschicht, kein Fluggeraet). Als "solide" behandelt spart das bei hoher
        // Sichtweite tausende nie sichtbare Quads (eine ganze Bodenflaeche pro Spalte).
        if y < welt_min_y {
            return SOLID_BELOW_WORLD;
        }
        if x < base_x && z < base_z {
            return neighbors.northwest.as_ref().map_or(STATE_AIR, |c| c.get(SECTION_SIZE - 1, y, SECTION_SIZE - 1));
        }
        if x >= base_x + SECTION_SIZE && z < base_z {
            return neighbors.northeast.as_ref().map_or(STATE_AIR, |c| c.get(0, y, SECTION_SIZE - 1));
        }
        if x < base_x && z >= base_z + SECTION_SIZE {
            return neighbors.southwest.as_ref().map_or(STATE_AIR, |c| c.get(SECTION_SIZE - 1, y, 0));
        }
        if x >= base_x + SECTION_SIZE && z >= base_z + SECTION_SIZE {
            return neighbors.southeast.as_ref().map_or(STATE_AIR, |c| c.get(0, y, 0));
        }
        if x < base_x {
            return neighbors.west.as_ref().map_or(STATE_AIR, |c| c.get(SECTION_SIZE - 1, y, z - base_z));
        }
        if x >= base_x + SECTION_SIZE {
            return neighbors.east.as_ref().map_or(STATE_AIR, |c| c.get(0, y, z - base_z));
        }
        if z < base_z {
            return neighbors.north.as_ref().map_or(STATE_AIR, |c| c.get(x - base_x, y, SECTION_SIZE - 1));
        }
        if z >= base_z + SECTION_SIZE {
            return neighbors.south.as_ref().map_or(STATE_AIR, |c| c.get(x - base_x, y, 0));
        }
        neighbors.target.get(x - base_x, y, z - base_z)
    };

    // Wasserfarbe ueber Biomgrenzen gemischt (s. water_blend).
    let wasser = WaterBlend::new(neighbors, biomes);
    let mut opaque = Vec::new();
    let mut translucent = Vec::new();
    // Zeitversetzte Animationen (s. desync_animations): der zuletzt begonnene Block und wo
    // seine Vertices in beiden Listen anfangen. Abgeschlossen wird er erst beim NAECHSTEN
    // Block (oder nach der Schleife) -- der Rumpf hat zu viele `continue`, um es am Ende zu tun.
    let desync = uvs.has_anim_variants();
    let mut pending: Option<((i32, i32, i32), usize, usize, usize)> = None;
    for (index, section) in neighbors.target.sections.iter().enumerate() {
        if section.is_empty() || region.is_some_and(|r| !r.touches_section(index)) {
            continue;
        }
        let base_y = welt_min_y + index as i32 * SECTION_SIZE;

        for local_y in 0..SECTION_SIZE {
            for local_z in 0..SECTION_SIZE {
                for local_x in 0..SECTION_SIZE {
                    if let Some(p) = pending.take() {
                        finish_desync(p, &mut opaque, &mut translucent, &mut fade_quads, uvs);
                    }
                    let state = section.get(local_x, local_y, local_z);
                    if !is_solid(state) {
                        continue;
                    }

                    let kennung = block_id(local_x, index as i32 * SECTION_SIZE + local_y, local_z);
                    if region.is_some_and(|r| !r.contains(kennung)) {
                        continue;
                    }
                    if let Some(starts) = block_starts.as_deref_mut() {
                        starts.push((opaque.len() as u32, kennung));
                    }

                    let (x, y, z) = (base_x + local_x, base_y + local_y, base_z + local_z);
                    if desync {
                        pending = Some(((x, y, z), opaque.len(), translucent.len(), fade_quads.len()));
                    }

                    // Biom-Faerbung: Gras/Laub ueber die Colormap, Wasser direkt -- fuer
                    // alle anderen Bloecke bleibt tint [1,1,1] (Textur unveraendert). Der
                    // Biom-Lookup ist immer lokal in der Zielspalte (biome_at gehoert zur
                    // Section DIESES Blocks, nicht zum Nachbarn).
                    let name = block_name(state).unwrap_or("");
                    let biome_id = section.biome_at(local_x, local_y, local_z);
                    let colors = biomes.colors_of(biome_id);
                    // moving_piston hat kein Modell: den fahrenden Block zeichnet der Kolben selbst
                    // (s. app::pistons), an seiner jeweiligen Zwischenposition.
                    if name == "moving_piston" {
                        continue;
                    }
                    if !show_markers && is_marker_block(name) {
                        // Eine wassergefuellte Barriere zeigt trotzdem ihr Wasser.
                        if property(state, "waterlogged") == Some("true") {
                            let wasser_ab = translucent.len();
                            push_waterlogged_fill(&mut translucent, origin, x, y, z, wasser.at(local_x, y, local_z), uvs, shapes, &block_at, [false; 6]);
                            mark_water(&mut translucent, wasser_ab);
                        }
                        continue;
                    }
                    let (tint, tint_only_up) = if is_water(name) {
                        (wasser.at(local_x, y, local_z), false)
                    } else {
                        match tint_kind_of_state(state, name) {
                            TintKind::None => ([1.0, 1.0, 1.0], false),
                            TintKind::GrassTop => (colors.grass, true),
                            TintKind::GrassAll => (colors.grass, false),
                            TintKind::FoliageAll => (colors.foliage, false),
                            TintKind::DryFoliageAll => (colors.dry_foliage, false),
                            TintKind::Fixed(rgb) => (rgb_f32(rgb), false),
                        }
                    };

                    // Kreuz-Bloecke (Zuckerrohr, Gras, Blumen, ...): Vanilla nutzt hier zwei
                    // diagonale Ebenen statt eines Quaders (s. assets::shapes-Kommentar zu
                    // Element::rotation) -- eigener Zeichenpfad statt der Box-Schleife
                    // unten, und ohne Nachbar-Culling (Kreuz-Bloecke sind immer von allen
                    // Seiten sichtbar, unabhaengig vom Nachbarblock).
                    if cross.is_cross(state) {
                        // Kreuz-Bloecke haben nur eine Partikeltextur (s. assets::blocks::
                        // resolve_particle-Fallback), die auf allen 6 "Flaechen" identisch
                        // hinterlegt ist -- welcher Index hier steht, ist daher egal.
                        let uv = uvs.face_rect(state, 2);
                        push_cross_quads(&mut opaque, origin, x, y, z, tint, uv);
                        // Kelp (kelp/kelp_plant) ist ein Kreuz-Block OHNE "waterlogged"-
                        // Property -- es ist in Vanilla schlicht IMMER von Wasser umgeben
                        // (keine trockene Variante existiert, s. always_wet-Kommentar). Ohne
                        // diesen Zweig bliebe die Zelle komplett trocken/leer, weil der
                        // normale waterlogged-Check unten (Box-Schleife) fuer Kreuz-Bloecke
                        // nie erreicht wird (continue oben).
                        if always_wet(name) {
                            let wasser_ab = translucent.len();
                            push_waterlogged_fill(&mut translucent, origin, x, y, z, wasser.at(local_x, y, local_z), uvs, shapes, &block_at, [false; 6]);
                            mark_water(&mut translucent, wasser_ab);
                        }
                        // Zusaetzliche, unrotierte Geometrie eines Kreuz-Modells (z.B. die
                        // Hitbox-Deko-Ebenen eines PvP-Cobweb-Packs, s. assets::shapes::
                        // build_cross_extras) -- wie Vanilla JEDES Element eines Custom-
                        // Modells rendert, nicht nur die als Kreuz darstellbare Ebene.
                        // Rein additiv, KEINE Nachbar-Verdeckungspruefung (wie die
                        // Kreuz-Ebenen selbst) und eine von `shapes` komplett getrennte
                        // Tabelle -- die muss fuer Kreuz-States leer bleiben, s. dortigen
                        // main.rs-Kommentar zum Nachbar-Deckungstest.
                        for shape_box in cross_extra_shapes.shapes_of(state) {
                            let (lx, ly, lz) = origin.local(x, y, z);
                            let world_min = [lx + shape_box.min[0], ly + shape_box.min[1], lz + shape_box.min[2]];
                            let world_max = [lx + shape_box.max[0], ly + shape_box.max[1], lz + shape_box.max[2]];
                            for (offset, brightness, atlas_face) in FACES {
                                if !shape_box.faces[atlas_face] {
                                    continue;
                                }
                                let Some(extra_uv) = uvs.content_rect(state, atlas_face) else { continue };
                                let shade = [face_shade(brightness); 3];
                                push_face_quad(&mut opaque, world_min, world_max, offset, shade, extra_uv);
                            }
                        }
                        continue;
                    }

                    // Modelle mit gedrehten Elementen (Feuer, Laterne, Kerzen, ...): ihre echten
                    // Vierecke statt Boxen (s. BlockShapes::model_quads). Ohne Nachbar-Culling
                    // wie die Kreuz-Ebenen -- die Flaechen liegen fast nie auf der Zellgrenze.
                    if let Some(quads) = shapes.model_quads_at(state, x, y, z) {
                        let (lx, ly, lz) = origin.local(x, y, z);
                        for q in quads {
                            // Liegt ein Viereck doch auf der Zellgrenze (Pilzbloecke, s.
                            // assets::shapes::needs_model_quads), gilt dasselbe Culling wie in
                            // der Box-Schleife -- sonst stuende jede Innenflaeche eines
                            // Riesenpilzes im Mesh.
                            let grenze = boundary_quad(&q.corners);
                            // ...und dieselbe Eckabdunklung wie eine Wuerfelflaeche. Ohne sie
                            // hatten Pilzbloecke als einzige volle Bloecke keinen weichen
                            // Schatten.
                            let ao = grenze.as_ref().filter(|_| ao_strength > 0.0 && shapes.ao_flags(state) & crate::ambient_occlusion::AO_SKIP == 0).map(|(_, face, _)| {
                                let ecken = crate::ambient_occlusion::face_corners((x, y, z), *face, true, |bx, by, bz| shapes.ao_flags(block_at(bx, by, bz)));
                                q.corners.map(|lokal| {
                                    let voll = crate::ambient_occlusion::vertex_ao(*face, ecken, lokal);
                                    1.0 - (1.0 - voll) * ao_strength
                                })
                            });
                            if let Some((offset, face, my)) = grenze {
                                let neighbor = block_at(x + offset[0], y + offset[1], z + offset[2]);
                                let neighbor_name = block_name(neighbor).unwrap_or("");
                                if pane_skips_face(state, neighbor, offset) {
                                    continue;
                                }
                                if is_solid(neighbor)
                                    && shapes.has_resolved_shape(neighbor)
                                    && !is_water(neighbor_name)
                                    && !is_glass(neighbor_name)
                                    && !never_occludes_neighbors(neighbor_name)
                                    && neighbor_covers_face(shapes.shapes_of(neighbor), offset, face, &my)
                                {
                                    continue;
                                }
                            }
                            let q_shade = face_shade(q.shade);
                            let color = [tint[0] * q_shade, tint[1] * q_shade, tint[2] * q_shade];
                            let vert = |i: usize| {
                                let a = ao.map_or(1.0, |f| f[i]);
                                BlockVertex {
                                    position: [lx + q.corners[i][0], ly + q.corners[i][1], lz + q.corners[i][2]],
                                    color: [color[0] * a, color[1] * a, color[2] * a],
                                    uv: q.uvs[i],
                                }
                            };
                            // Umlauf 0,2,1 / 0,3,2: die Eckreihenfolge der Modell-Vierecke ist
                            // die umgekehrte von face_quad_corners (s. model_face_tests).
                            let quad = [vert(0), vert(2), vert(1), vert(0), vert(3), vert(2)];
                            // Feuer im durchscheinenden Pass: das Modul "Dynamisches Feuer" blendet
                            // es ueber das Alpha seiner Atlas-Kacheln aus (s. app::block_anim).
                            // Die Feuertexel sind voll deckend oder ganz leer; damit das auch
                            // zwischen und in den kleinen Mip-Stufen so bleibt, schneidet der
                            // Shader Feuer hart ab (Kennzeichen s. BlockVertex::with_cutout_mark).
                            if is_fire(name) {
                                translucent.extend(quad.map(BlockVertex::with_cutout_mark));
                            } else if is_glass(name) {
                                translucent.extend_from_slice(&quad);
                            } else {
                                opaque.extend_from_slice(&quad);
                            }
                        }
                        if property(state, "waterlogged") == Some("true") {
                            let wasser_ab = translucent.len();
                            push_waterlogged_fill(&mut translucent, origin, x, y, z, wasser.at(local_x, y, local_z), uvs, shapes, &block_at, [false; 6]);
                            mark_water(&mut translucent, wasser_ab);
                        }
                        continue;
                    }

                    // Wasser UND Glas brauchen echtes Alpha-Blending -- eigene Vertex-Liste
                    // statt der normalen Opak-Geometrie. Laub bleibt im Opak-Pass (dessen
                    // Alpha wird beim Atlas-Bau ohnehin hart auf voll gesetzt, s.
                    // assets::is_leaves).
                    //
                    // Glas lag frueher im Opak-Pass, und seine Transparenz wurde durch einen
                    // in die Kachel eingebrannten Bayer-Dither vorgetaeuscht (jedes zweite
                    // Texel auf Alpha 0). Das ergab Fliegengitter statt Glas. Vanilla legt
                    // ALLES Glas -- klar, gefaerbt, getoent, Scheiben -- in die TRANSLUCENT-
                    // Schicht (BlockRenderLayers, nachgesehen), also echtes Blending.
                    // Die Blasensaeule (ueber Seelensand/Magma) hat kein Modell und IST in Vanilla
                    // eine Wasserquelle (BubbleColumnBlock.getFluidState, s. fluid::fluid_kind) --
                    // ohne sie hier stand an ihrer Stelle ein Loch in der Wasseroberflaeche.
                    // Die Blasen selbst sind Partikel, die es noch nicht gibt.
                    let is_water_block = is_water(name) || name == "bubble_column";
                    let is_glass_block = is_glass(name);

                    // Echtes Wasser (nicht waterlogged/always_wet -- die bleiben unten bei der
                    // Box-Schleife/push_waterlogged_fill) bekommt Vanillas Eck-Hoehen-Gefaelle
                    // statt eines flachen Vollwuerfels -- eigener Zeichenpfad, s.
                    // push_fluid_quads-Kommentar. Wasser hat nie Content/Offset/eine zweite
                    // Box, ein fruehes continue ist deshalb sicher (anders als bei Kreuz-
                    // Bloecken oben gibt es hier keine always_wet-Wasserfuellung zusaetzlich
                    // noetig -- "water" IST schon die Fuellung).
                    if is_water_block {
                        let wasser_ab = translucent.len();
                        push_fluid_quads(&mut translucent, origin, x, y, z, state, FluidKind::Water, wasser.at(local_x, y, local_z), uvs, shapes, &block_at);
                        mark_water(&mut translucent, wasser_ab);
                        continue;
                    }

                    // LAVA: exakt derselbe Zeichenpfad -- Vanillas FluidRenderer.render
                    // behandelt beide Fluessigkeiten in einer Methode (s. push_fluid_quads).
                    // Vorher fiel Lava in die Box-Schleife unten und bekam den Vollwuerfel-
                    // Notnagel (lava.json hat keine "elements", s. assets::blocks): sie stand
                    // einen ganzen Block hoch statt 8/9, ohne Eck-Gefaelle, ohne Fliesstextur
                    // und ohne Culling gegen Lava-Nachbarn (has_resolved_shape ist fuer Lava
                    // false, also zeichnete eine Lavasee JEDE innere Flaeche mit).
                    //
                    // Ziel ist der OPAKE Puffer, nicht der Translucent-Pass -- Vanilla legt
                    // nur FluidTags.WATER in die durchsichtige Schicht (RenderLayers.
                    // getFluidLayer), Lava ist blickdicht. Der Tint bleibt das neutrale
                    // [1,1,1] von oben (Lava hat keine Biom-Faerbung: `bl ? 16777215 : ...`).
                    if name == "lava" {
                        push_fluid_quads(&mut opaque, origin, x, y, z, state, FluidKind::Lava, tint, uvs, shapes, &block_at);
                        continue;
                    }

                    // Vollwuerfel (die grosse Mehrheit: Stein, Erde, ...) liefert genau eine
                    // Box ueber die volle Zelle -- fuer die bleibt das Verhalten identisch
                    // zu vorher. Nicht-Wuerfel-Bloecke (Stufen, Zaeune, ...) liefern mehrere
                    // kleinere Boxen aus den echten Vanilla-Modell-Elementen (s. assets::shapes).
                    // Bambus, Setzlinge, Gras, Blumen, ... stehen in Vanilla nie exakt
                    // mittig, sondern um einen pro (x,z)-Spalte deterministischen
                    // Zufallswert verschoben (s. world::block_offset) -- rein optisch, die
                    // Culling-/Nachbar-Logik unten arbeitet weiterhin auf den unverschobenen
                    // shape_box-Koordinaten (Zellenraster bleibt fuer Sichtbarkeitstests
                    // unveraendert, nur die tatsaechlichen Vertex-Weltkoordinaten wandern).
                    let (odx, odz) =
                        crate::block_offset::block_xz_offset(name, x, z);
                    let mut own_faces = [false; 6];
                    let state_shapes = shapes.shapes_of(state);
                    for (shape_index, shape_box) in state_shapes.iter().enumerate() {
                        let (lx, ly, lz) = origin.local(x, y, z);
                        let world_min = [lx + odx + shape_box.min[0], ly + shape_box.min[1], lz + odz + shape_box.min[2]];
                        let world_max = [lx + odx + shape_box.max[0], ly + shape_box.max[1], lz + odz + shape_box.max[2]];
                        for (offset, brightness, atlas_face) in FACES {
                            let brightness = face_shade(brightness);
                            if !shape_box.faces[atlas_face] {
                                continue; // Modell deklariert diese Seite gar nicht (z.B.
                                          // die Andock-Seite eines Zaunpfostens).
                            }
                            // Merken, wenn diese Box die Flaeche VOLLSTAENDIG UND GENAU AN
                            // DER ZELLGRENZE abdeckt (nicht nur "deklariert" -- ein duenner
                            // Zaunpfosten deklariert alle sechs Richtungen, deckt sie aber nur
                            // zu einem Bruchteil ab; die Wasserfuellung waterlogged-Bloecke
                            // (s. push_waterlogged_fill) soll um ihn herum trotzdem sichtbar
                            // sein). Die Zellgrenzen-Bedingung (touches_cell_boundary) ist
                            // genauso wichtig wie die Flaechenabdeckung: die UNTERE Haelfte
                            // einer Treppe deklariert z.B. eine volle "up"-Flaeche bei y=0.5
                            // (ihre eigene Stufenoberflaeche) -- OHNE den Grenzentest wuerde
                            // das faelschlich die Wasserfuellung an der eigentlichen Zellkante
                            // y=1 unterdruecken, obwohl dort gar keine echte Geometrie liegt
                            // und das Wasser genau dort sichtbar sein muss. Nur bei echter
                            // Vollabdeckung AN DER GRENZE (Stufen-Unterseite bei y=0,
                            // Zaunpfosten-Oben/Unten, ...) wuerde ein zusaetzliches
                            // Wasser-Quad exakt zusammenfallen (Z-Fighting) -- deshalb dort
                            // ueberspringen.
                            let (a0, a1) = face_plane_axes(atlas_face);
                            if touches_cell_boundary(shape_box, offset)
                                && shape_box.min[a0] <= 1e-4
                                && shape_box.max[a0] >= 1.0 - 1e-4
                                && shape_box.min[a1] <= 1e-4
                                && shape_box.max[a1] >= 1.0 - 1e-4
                            {
                                own_faces[atlas_face] = true;
                            }
                            // Nullstaerke-Ebenen (z.B. Ranken: from/to identisch auf der
                            // Normalenachse, s. vine.json) deklarieren beide Seiten (hier:
                            // north UND south) auf EXAKT derselben Flaeche. Beide Kopien
                            // bleiben erhalten -- das ist seit dem Backface-Culling der
                            // Opak-Pipeline nicht nur unschaedlich, sondern noetig:
                            //
                            // Frueher (ohne Culling) wurden beide gezeichnet und flackerten
                            // gegeneinander (deckungsgleiche Dreiecke, der Tiefentest
                            // entschied zufaellig), deshalb wurde hier eine davon verworfen.
                            // MIT Culling ist von zwei entgegengesetzt gewickelten Kopien
                            // immer genau EINE vorderseitig -- kein Flackern, und die Ebene
                            // bleibt aus beiden Richtungen sichtbar. Die alte Kuerzung machte
                            // sie dagegen von einer Seite unsichtbar.
                            // Nur an der Aussenkante der Zelle gegen den Nachbarn testen --
                            // eine innenliegende Flaeche (z.B. die Seite eines Zaunpfostens)
                            // ist immer sichtbar, unabhaengig vom Nachbarblock.
                            if touches_cell_boundary(shape_box, offset) {
                                let neighbor = block_at(x + offset[0], y + offset[1], z + offset[2]);
                                // Wasser gegen "nass" (echtes Wasser, waterlogged, always_wet-
                                // Wasserpflanzen wie Seegras/Kelp, s. world::fluid::fluid_kind):
                                // IMMER culln, unabhaengig von der Nachbar-FORM. Seegras deckt
                                // seine Zelle geometrisch nur zu einem Bruchteil ab (duenne
                                // Ebenen) -- ohne diesen fluid-bewussten Kurzschluss wuerde die
                                // normale formbasierte Pruefung unten fuer JEDE einzelne
                                // Wasserzelle neben Seegras eine eigene, sichtbare Grenzflaeche
                                // zeichnen, obwohl beide Seiten derselbe zusammenhaengende
                                // Wasserkoerper sind -- im Spiel als Kastennaht-Raster um jede
                                // Seegras-Zelle sichtbar.
                                if is_water_block && crate::fluid::fluid_kind(neighbor) == Some(crate::fluid::FluidKind::Water) {
                                    continue;
                                }
                                // is_solid() sagt nur "kein Luft", NICHT "blockiert die
                                // Sicht" -- Wasser ist solid (kein Air), aber transparent
                                // (s. render::BlockRenderer::translucent_pipeline). Ein
                                // NICHT-Wasser-Block darf seine Flaeche gegen einen
                                // Wassernachbarn deshalb nie culln, sonst fehlt genau die
                                // Geometrie, die man jetzt DURCH das Wasser hindurch sehen
                                // koennte (Loch statt Meeresboden/Kellerwand). Wasser gegen
                                // Wasser bleibt weiterhin geculled (keine sichtbare interne
                                // Naht in einem zusammenhaengenden Gewaesser), ebenso jeder
                                // Block gegen einen ECHT opaken Nachbarn (Stein, Lava, ...).
                                // Glas verdeckt nichts: man sieht hindurch, also muss die
                                // Flaeche dahinter gezeichnet werden. Genau derselbe Fall wie
                                // Wasser eine Zeile darueber -- und derselbe sichtbare Fehler,
                                // solange er offen war (man konnte durch das Gras
                                // gucken: die Grasoberseite UNTER einer Glasscheibe fehlte,
                                // seit Glas echt durchsichtig ist).
                                //
                                // Ausnahme ist GLEICHES Glas an gleichem Glas -- dort culled
                                // Vanilla die gemeinsame Flaeche sehr wohl
                                // (Block.isSideInvisible/skipRendering), sonst saehe man in
                                // einer Glaswand jede innere Naht. Verglichen wird der
                                // BLOCKNAME, nicht bloss "ist Glas": rotes neben blauem
                                // Buntglas sind verschiedene Bloecke, da bleibt die Flaeche
                                // stehen -- genau wie in Vanilla.
                                let neighbor_name = block_name(neighbor).unwrap_or("");
                                let neighbor_can_occlude = is_solid(neighbor)
                                    // Ohne aufgeloeste Form wissen wir gar nicht, wie viel der
                                    // Nachbar abdeckt -- der Vollwuerfel-Notnagel darf hier
                                    // nichts verdecken (s. BlockShapes::has_resolved_shape).
                                    && shapes.has_resolved_shape(neighbor)
                                    && (!is_water(neighbor_name) || is_water_block)
                                    && !(is_glass(neighbor_name) && neighbor_name != name)
                                    && !never_occludes_neighbors(neighbor_name);
                                if neighbor_can_occlude {
                                    // Nur "solid", nichts darueber, wie viel Flaeche der
                                    // Nachbar an dieser Grenze tatsaechlich abdeckt -- eine
                                    // Treppe/Stufe/Zaun deckt die Grenzflaeche oft nur
                                    // TEILWEISE ab. Ungeprueft verdeckt zu behandeln reisst
                                    // Loecher in echte Geometrie (z.B. eine volle Stufe ueber
                                    // einer Treppenstufe: die Unterseite waere unsichtbar,
                                    // obwohl die Treppe darunter nicht die ganze Flaeche deckt).
                                    let neighbor_shapes = shapes.shapes_of(neighbor);
                                    if neighbor_covers_face(neighbor_shapes, offset, atlas_face, shape_box) {
                                        continue; // Seite verdeckt -- kein Quad noetig
                                    }
                                }
                            }
                            // Shade als Graustufe (Richtungs-Beleuchtung) * Biom-Tint (weiss
                            // = unveraendert fuer alles ausser Gras/Laub/Wasser). Bei
                            // GrassTop (grass_block) faerbt die BASIS-Flaeche nur oben (die
                            // Seiten bleiben ungetoentes Erdbraun) -- die eigentliche
                            // Gras-Faerbung der Seiten kommt separat unten aus der
                            // Overlay-Flaeche (echte, per-Biom eingefaerbte zweite
                            // Geometrie-Ebene, s. push_face_quad-Aufruf mit overlay_uv weiter
                            // unten). Inhaltsflaeche (z.B. Kesselinhalt, s. assets::shapes::
                            // ElementBox::content): eigene Atlas-Kachel statt der normalen
                            // Flaechentextur, UND eigene Faerbung (nur Wasser bekommt die
                            // Biom-Wasserfarbe -- der Block selbst ("water_cauldron") ist
                            // ueber tint_kind/is_water nicht erfasst, das gilt nur fuer
                            // echte Wasserbloecke, waere sonst auch faelschlich auf dem
                            // Kesselrand angewendet).
                            let (uv, t) = if shape_box.content[atlas_face] {
                                let content_uv = uvs.content_rect(state, atlas_face).unwrap_or_else(|| uvs.face_rect(state, atlas_face));
                                let content_tint = if name == "water_cauldron" { wasser.at(local_x, y, local_z) } else { [1.0, 1.0, 1.0] };
                                (content_uv, content_tint)
                            } else {
                                let apply_tint = !tint_only_up || atlas_face == FACE_UP;
                                // face_rect_rng() statt face_rect(): Vanillas pseudozufaellige
                                // Block-Rotation (Erde/Sand/Stein/Grasblock/..., s. world::
                                // render_rng-Modulkommentar) haengt von der WELTPOSITION ab,
                                // nicht nur vom State -- fuer States ohne hinterlegte
                                // Varianten oder abgeschaltetem Umschalter identisch zu
                                // face_rect(). Die Overlay-Flaeche (unten) bleibt bewusst
                                // unrotiert: eine eigene Variantentabelle nur fuer den duennen
                                // Gras-Rand waere fuer den kaum sichtbaren Unterschied nicht
                                // wert.
                                let face_uv = crop_face_uv(uvs.face_rect_rng(state, atlas_face, x, y, z), shape_box.uv[atlas_face]);
                                (face_uv, if apply_tint { tint } else { [1.0, 1.0, 1.0] })
                            };
                            let shade = [brightness * t[0], brightness * t[1], brightness * t[2]];
                            // Mauern/Zaeune: mehrere Boxen desselben States (Pfosten + Arme)
                            // koennen sich in der Oben-/Untenflaeche exakt ueberdecken (s.
                            // WALL_ZFIGHT_EPS-Kommentar) -- die unterlegene Flaeche wird dann
                            // um ihren Konflikt-Anteil nach innen verschoben, damit sie
                            // deterministisch (nicht zufaellig je nach Rundung) hinter der
                            // fuehrenden liegt.
                            let (quad_min, quad_max) = if atlas_face == FACE_UP || atlas_face == FACE_DOWN {
                                match coplanar_conflict_count(state_shapes, shape_index, atlas_face) {
                                    0 => (world_min, world_max),
                                    n => {
                                        let shift = n as f32 * WALL_ZFIGHT_EPS;
                                        if atlas_face == FACE_UP {
                                            (world_min, [world_max[0], world_max[1] - shift, world_max[2]])
                                        } else {
                                            ([world_min[0], world_min[1] + shift, world_min[2]], world_max)
                                        }
                                    }
                                }
                            } else {
                                (world_min, world_max)
                            };
                            // Weiche Beleuchtung: vier Eckwerte der Flaeche aus den Nachbarn,
                            // dann je Vertex bilinear an seiner Lage in der Flaeche (s.
                            // ambient_occlusion). Ohne sie bleibt es bei einer Farbe je Quad.
                            // Treppe aus Schneeschichten (s. ambient_occlusion::snow_riser_bottom):
                            // die Seitenflaeche beginnt erst an der Oberseite der niedrigeren
                            // Schicht davor, und ihre Abdunklung laeuft nur ueber diesen Teil.
                            let schnee = crate::ambient_occlusion::snow_layers(state).filter(|&l| l < 8);
                            let schnee_seite = schnee.filter(|_| offset[1] == 0 && touches_cell_boundary(shape_box, offset));
                            let mut ao_box = (shape_box.min, shape_box.max);
                            let (quad_min, uv) = match schnee_seite {
                                Some(l) => {
                                    let unten = crate::ambient_occlusion::snow_riser_bottom(l, block_at(x + offset[0], y, z + offset[2]));
                                    let hoehe = shape_box.max[1] - shape_box.min[1];
                                    if unten > shape_box.min[1] && hoehe > 0.0 {
                                        ao_box.0[1] = unten;
                                        let [u0, v0, u1, v1] = uv;
                                        let anteil = (shape_box.max[1] - unten) / hoehe;
                                        ([quad_min[0], quad_min[1] + (unten - shape_box.min[1]), quad_min[2]], [u0, v0, u1, v0 + (v1 - v0) * anteil])
                                    } else {
                                        (quad_min, uv)
                                    }
                                }
                                None => (quad_min, uv),
                            };
                            let ao_ecken = |nachschlagen: &dyn Fn(i32, i32, i32) -> u32| {
                                let ecken = crate::ambient_occlusion::face_corners(
                                    (x, y, z),
                                    atlas_face,
                                    // Vanillas field_58160: Quad auf der Zellgrenze ODER der
                                    // Block ist ein Vollwuerfel (dann zaehlt auch ein nach
                                    // innen versetztes Quad als Aussenflaeche).
                                    touches_cell_boundary(shape_box, offset)
                                        || shapes.ao_flags(state) & crate::ambient_occlusion::AO_FULL_CUBE != 0,
                                    |bx, by, bz| {
                                        let nachbar = nachschlagen(bx, by, bz);
                                        let flags = shapes.ao_flags(nachbar);
                                        match schnee {
                                            Some(l) if atlas_face == FACE_UP => crate::ambient_occlusion::snow_top_flags(l, nachbar, flags),
                                            _ => flags,
                                        }
                                    },
                                );
                                face_quad_corners(ao_box.0, ao_box.1, offset).map(|mut lokal| {
                                    if schnee_seite.is_some() {
                                        lokal[1] = (lokal[1] - ao_box.0[1]) / (ao_box.1[1] - ao_box.0[1]).max(1e-4);
                                    }
                                    let voll = crate::ambient_occlusion::vertex_ao(atlas_face, ecken, lokal);
                                    1.0 - (1.0 - voll) * ao_strength
                                })
                            };
                            let ao = (ao_strength > 0.0 && shapes.ao_flags(state) & crate::ambient_occlusion::AO_SKIP == 0).then(|| ao_ecken(&block_at));
                            // Eckabdunklung wie VOR der Aenderung, falls diese Flaeche an einer
                            // der `fades` haengt (s. mesh_column_ao_fading). Nur opak.
                            let ao_vorher = match ao {
                                Some(faktoren) if !(fades.is_empty() || is_water_block || is_glass_block) => {
                                    let nah = fades.iter().any(|((fx, fy, fz), _)| (fx - x).abs() <= 1 && (fy - y).abs() <= 1 && (fz - z).abs() <= 1);
                                    let vorher = if !nah {
                                        None
                                    } else if fades.iter().any(|(p, _)| *p == (x, y, z)) {
                                        Some([1.0; 4])
                                    } else {
                                        let alt = |bx: i32, by: i32, bz: i32| {
                                            fades.iter().find(|(p, _)| *p == (bx, by, bz)).map_or_else(|| block_at(bx, by, bz), |(_, s)| *s)
                                        };
                                        Some(ao_ecken(&alt))
                                    };
                                    vorher.filter(|v| *v != faktoren)
                                }
                                _ => None,
                            };
                            let verts = if is_water_block || is_glass_block { &mut translucent } else { &mut opaque };
                            match ao {
                                Some(faktoren) => {
                                    let start = verts.len();
                                    push_face_quad_ao(verts, quad_min, quad_max, offset, shade, faktoren, uv);
                                    if let Some(vorher) = ao_vorher {
                                        let mut before = Vec::new();
                                        push_face_quad_ao_geteilt(&mut before, quad_min, quad_max, offset, shade, vorher, uv, ao_diagonal_gedreht(faktoren));
                                        fade_quads.push(AoFadeQuad { start: start as u32, translucent: is_water_block || is_glass_block, before, after: verts[start..].to_vec() });
                                    }
                                }
                                None => push_face_quad(verts, quad_min, quad_max, offset, shade, uv),
                            }

                            // Overlay-Flaeche (z.B. grass_block-Seiten, s. assets::
                            // BlockAtlas::overlay_uvs-Kommentar): deckungsgleiche zweite
                            // Geometrie-Ebene, IMMER (nicht nur bei tint_only_up==false) mit
                            // dem echten Biom-Tint gefaerbt. Landet im OPAK-Pass (nicht im
                            // Translucent-Pass wie ein frueherer Versuch dieser Sitzung) --
                            // per PIL gegen die echte grass_block_side_overlay.png geprueft:
                            // die Textur ist ein HARTER Cutout (jedes Texel entweder Alpha=0
                            // oder Alpha=255, keine weichen Kanten), braucht also gar kein
                            // echtes Blending. Im Translucent-Pass zusammen mit Wasser fuehrte
                            // das dazu, dass das Gras-Overlay blau wirkte: zwei
                            // ungeordnete Translucent-Flaechen (Wasser + Overlay) OHNE
                            // Tiefenschreiben sortieren sich gegeneinander nicht zuverlaessig
                            // -- weder Reihenfolge INNERHALB einer Spalte noch ein separater,
                            // spalten-uebergreifender Overlay-Pass halfen (beide Versuche
                            // aendern nichts an echten Tiefenkonflikten zwischen zwei
                            // Translucent-Flaechen). Im Opak-Pass (discard() statt Blending,
                            // s. block.wgsl::fs_main) schreibt das Overlay stattdessen
                            // NORMAL Tiefe -- Wasser (Translucent-Pass, danach gezeichnet)
                            // testet dagegen dann ganz regulaer und korrekt. depth_compare
                            // der Opak-Pipeline ist deshalb jetzt LessEqual statt Less (s.
                            // render::BlockRenderer::pipeline-Kommentar) -- exakt dieselbe
                            // Deckungsgleich-Begruendung wie zuvor bei der Translucent-Pipeline.
                            if !shape_box.content[atlas_face] {
                                if let Some(overlay_rect) = uvs.overlay_rect(state, atlas_face) {
                                    let overlay_uv = crop_face_uv(overlay_rect, shape_box.uv[atlas_face]);
                                    let overlay_shade = [brightness * tint[0], brightness * tint[1], brightness * tint[2]];
                                    // Die Overlay-Ebene liegt deckungsgleich auf der Basis --
                                    // dieselben Eckfaktoren, sonst waere der Grasrand an den
                                    // Kanten heller als die Erde darunter.
                                    match ao {
                                        Some(faktoren) => {
                                            let start = opaque.len();
                                            push_face_quad_ao(&mut opaque, quad_min, quad_max, offset, overlay_shade, faktoren, overlay_uv);
                                            if let Some(vorher) = ao_vorher {
                                                let mut before = Vec::new();
                                                push_face_quad_ao_geteilt(&mut before, quad_min, quad_max, offset, overlay_shade, vorher, overlay_uv, ao_diagonal_gedreht(faktoren));
                                                fade_quads.push(AoFadeQuad { start: start as u32, translucent: false, before, after: opaque[start..].to_vec() });
                                            }
                                        }
                                        None => push_face_quad(&mut opaque, quad_min, quad_max, offset, overlay_shade, overlay_uv),
                                    }
                                }
                            }
                        }
                    }

                    // Waterlogged (z.B. ein volllaufender Zaun/eine Treppe/eine Stufe) ODER
                    // always_wet (Seegras: benutzt zwar die Box-Schleife hier -- sein Modell
                    // "template_seagrass" hat vier NICHT rotierte Nullstaerke-Ebenen, ist also
                    // technisch kein Kreuz-Block, s. always_wet-Kommentar -- traegt aber keine
                    // "waterlogged"-Property, weil es in Vanilla nie trocken vorkommt). Der
                    // Block selbst bleibt opak (oben behandelt), zusaetzlich eine Wasser-
                    // fuellung ueber die volle Zelle in den Translucent-Pass -- echte
                    // Wasserbloecke ("water" selbst) sind hier ausgeschlossen (waeren sonst
                    // doppelt gezeichnet, s. is_water_block oben).
                    if !is_water_block && (property(state, "waterlogged") == Some("true") || always_wet(name)) {
                        let wasser_ab = translucent.len();
                        push_waterlogged_fill(&mut translucent, origin, x, y, z, wasser.at(local_x, y, local_z), uvs, shapes, &block_at, own_faces);
                        mark_water(&mut translucent, wasser_ab);
                    }
                }
            }
        }
    }
    if let Some(p) = pending {
        finish_desync(p, &mut opaque, &mut translucent, &mut fade_quads, uvs);
    }

    (opaque, translucent, fade_quads)
}

// Schliesst einen Block fuer desync_animations ab: seine Vertices in beiden Listen und die
// Vorher/Nachher-Kopien seiner eingeblendeten Flaechen -- die ueberschreiben beim Einblenden
// die Vertices im Puffer und brachten ohne Versatz nach jedem Neu-Meshen die Basiskachel zurueck.
pub(super) fn finish_desync(
    (pos, o, t, f): ((i32, i32, i32), usize, usize, usize),
    opaque: &mut [BlockVertex],
    translucent: &mut [BlockVertex],
    fade_quads: &mut [AoFadeQuad],
    uvs: &BlockUvs,
) {
    desync_animations(&mut opaque[o..], uvs, pos);
    desync_animations(&mut translucent[t..], uvs, pos);
    for q in &mut fade_quads[f..] {
        desync_animations(&mut q.before, uvs, pos);
        desync_animations(&mut q.after, uvs, pos);
    }
}

// Biegt die Dreiecke EINES Blocks, die auf eine animierte Kachel zeigen, auf eine ihrer
// zeitversetzten Kopien um (s. assets::ANIM_VARIANTS) -- welche, entscheidet die
// Blockposition. Gewaehlt wird je Dreieck ueber dessen UV-Schwerpunkt (liegt sicher in der
// Kachel), alle Dreiecke eines Blocks landen fuer dieselbe Kachel auf derselben Kopie.
pub(super) fn desync_animations(verts: &mut [BlockVertex], uvs: &BlockUvs, (x, y, z): (i32, i32, i32)) {
    if verts.is_empty() {
        return;
    }
    // Eigener Hash statt render_rng::pick_variant: der bestimmt schon die Block-Rotation,
    // beide sollen nicht miteinander korrelieren.
    // Volles Durchmischen (zwei Multiplikationen), damit auch die untersten Bits -- sie
    // waehlen die Kopie -- ueber Reihen gleicher Hoehe gleichmaessig streuen.
    let h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ (z as u32).wrapping_mul(0xC2B2_AE3D);
    let h = (h ^ (h >> 16)).wrapping_mul(0x85EB_CA6B);
    let h = (h ^ (h >> 13)).wrapping_mul(0xC2B2_AE35);
    let h = h ^ (h >> 16);
    for tri in verts.chunks_exact_mut(3) {
        let centroid = [(tri[0].uv[0] + tri[1].uv[0] + tri[2].uv[0]) / 3.0, (tri[0].uv[1] + tri[1].uv[1] + tri[2].uv[1]) / 3.0];
        let shifts = uvs.anim_shifts_at(centroid);
        if shifts.is_empty() {
            continue;
        }
        // Kopie 0 ist die Basiskachel selbst, also eine Wahl aus len + 1.
        let pick = (h % (shifts.len() as u32 + 1)) as usize;
        if pick == 0 {
            continue;
        }
        let [du, dv] = shifts[pick - 1];
        for v in tri {
            v.uv[0] += du;
            v.uv[1] += dv;
        }
    }
}

// Alles ab `from` als Wasser kennzeichnen (s. BlockVertex::with_water_mark): die Render-Order
// behandelt Wasser anders als das uebrige Durchscheinende.
fn mark_water(out: &mut [BlockVertex], from: usize) {
    for v in &mut out[from..] {
        *v = v.with_water_mark();
    }
}
