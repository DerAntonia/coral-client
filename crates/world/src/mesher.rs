// Face-Culling-Mesher: erzeugt fuer jeden Block nur die Quads der Seiten, die an Luft
// grenzen. Das ist die minimale Optimierung gegenueber "jeder Block = eigener Wuerfel" --
// Greedy Meshing (Zusammenfassen gleichartiger Nachbar-Quads) waere der naechste Schritt,
// falls das hier nicht reicht.
//
// Nachbarn werden ueber die Spaltengrenze hinaus nachgeschlagen, nicht nur innerhalb der
// Sektion: sonst bekaeme jede Sektion an ihren sechs Aussenflaechen eine geschlossene Wand
// aus unsichtbaren Quads zur Nachbar-Sektion hin. Bei 24 Sektionen pro Spalte waere das der
// Grossteil der gesamten Geometrie -- unsichtbar, aber voll bezahlt.
//
// mesh_column_from_neighbors() ist bewusst eine reine Funktion ohne &World-Parameter: sie
// nimmt nur die 5 Arc<ChunkColumn> (Ziel + 4 Nachbarn), die sie tatsaechlich braucht, und
// laeuft damit auf einem Worker-Thread, waehrend der Hauptthread parallel weiter an der
// World arbeitet (neue Chunks einfuegen etc.) -- keine Sperre noetig, weil die Arcs ein
// unveraenderliches Abbild des Zeitpunkts sind, zu dem der Job erstellt wurde. mesh_column()
// bleibt als bequemer Wrapper fuer Aufrufer erhalten, die eine &World zur Hand haben
// (Tests, einmalige Aufrufe).

use std::sync::Arc;

use crate::biome_palette::BiomePalette;
use crate::biome_tint::{is_water, tint_kind, TintKind};
use crate::block_registry::block_name;
use crate::block_shapes::{BlockShapes, ShapeBox};
use crate::block_uvs::BlockUvs;
use crate::chunk::{is_solid, ChunkColumn, World, SECTION_SIZE, STATE_AIR};
use crate::cross_shapes::CrossBlocks;
use crate::fluid::{always_wet, FluidKind};
use crate::state_props::property;

// Ursprung, auf den sich die Vertex-Positionen einer Spalte beziehen.
//
// ECHTER BUG gewesen (beim Nach-unten-Schauen drehte sich die Kamera scheinbar nur noch in
// Achtelschritten, danach zitterte die Geometrie): frueher standen hier ABSOLUTE
// Weltkoordinaten in f32. Bei x = -186634 (DonutSMP) ist die f32-Aufloesung schon 0.015625 --
// jeder Blockeckpunkt rastete also bereits BEIM MESHEN auf ein 1/64-Gitter ein, und zwar
// unwiderruflich: die Zahl steht so im GPU-Puffer, keine Matrix holt sie zurueck.
//
// Deshalb sind Vertex-Positionen jetzt SPALTEN-LOKAL: x und z laufen von 0..16, y bleibt
// absolut (der Wertebereich -64..320 ist in f32 ohnehin exakt). Wo die Spalte in der Welt
// steht, sagt erst beim Zeichnen ein Versatz pro Spalte (s. render::BlockRenderer) -- und der
// wird kamerarelativ in f64 gerechnet, ist also nie gross.
//
// Der Y-Wert bleibt bewusst absolut: er in der Hoehe zu verschieben braechte nichts (f32 ist
// dort schon exakt) und wuerde jede Hoehenrechnung im Mesher unnoetig verbiegen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColumnOrigin {
    pub x: i32,
    pub z: i32,
}

impl ColumnOrigin {
    pub fn of_column(chunk_x: i32, chunk_z: i32) -> Self {
        Self { x: chunk_x * SECTION_SIZE, z: chunk_z * SECTION_SIZE }
    }

    // Weltkoordinaten eines Blocks -> spalten-lokale Gleitkommaposition. Die Subtraktion
    // passiert in i32, VOR der f32-Wandlung -- genau darin liegt der Gewinn: (x - base_x)
    // ist eine kleine Zahl und damit in f32 exakt, x allein ist es bei grossen Koordinaten
    // laengst nicht mehr.
    pub fn local(self, x: i32, y: i32, z: i32) -> (f32, f32, f32) {
        ((x - self.x) as f32, y as f32, (z - self.z) as f32)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlockVertex {
    // SPALTEN-LOKAL, nicht in Weltkoordinaten -- s. ColumnOrigin.
    pub position: [f32; 3],
    // Richtungs-Shade (Helligkeit als [b,b,b]) -- der Shader multipliziert die
    // Atlas-Textur damit. Fuer die Entity-Boxen (box_vertices) traegt es die volle Farbe.
    pub color: [f32; 3],
    // Atlas-UV (0..1). Fuer die einfarbigen Entity-Boxen zeigt es auf eine
    // Volltextur-Kachel (s. box_vertices) -- der Shader sampelt immer.
    pub uv: [f32; 2],
}

impl BlockVertex {
    // Kennzeichen "weiss ueberblenden" (blinkendes TNT, TntRenderer): Farbe c wird als c + 2
    // gespeichert, der Shader (tint_srgb) erkennt es an Werten ueber 1,5.
    pub fn with_white_flash(mut self) -> Self {
        self.color = self.color.map(|c| c + 2.0);
        self
    }

    // Kennzeichen "hart abschneiden" (Feuer im durchscheinenden Pass, s. fs_main_translucent):
    // Farbe c wird als -1 - c gespeichert, also immer unter -0,5. Linear interpoliert bleibt
    // das ueber das ganze Viereck erhalten, und der Shader holt c mit -1 - x zurueck.
    pub fn with_cutout_mark(self) -> Self {
        Self { color: self.color.map(|c| -1.0 - c), ..self }
    }

    // Die Farbe ohne Kennzeichen (fuer alles ausser dem PvP-Shader).
    pub fn plain_color(&self) -> [f32; 3] {
        if self.color[0] < -0.5 { self.color.map(|c| -1.0 - c) } else { self.color }
    }
}

// Face-Richtungen mit Normalen-Offset, Basis-Helligkeit (grobe, vanilla-aehnliche
// Richtungs-Beleuchtung) und dem zugehoerigen Atlas-Flaechenindex (assets::FACE_*:
// down=0,up=1,north=2,south=3,west=4,east=5) -- damit jede Wuerfelseite ihre richtige
// Textur bekommt (oben Gras, Seite Rinde usw.).
// Sentinel-State fuer "unterhalb der Welt": jeder Wert != STATE_AIR gilt als solide
// (s. is_solid), wird aber nie fuer eine Textur nachgeschlagen -- er kommt nur als
// Nachbar-Ergebnis in den is_solid()-Vergleich, nie als das zu zeichnende `state` selbst.
const SOLID_BELOW_WORLD: u32 = u32::MAX;

// Atlas-Flaechenindex "oben" (s. assets::FACE_UP) -- fuer die GrassTop-Sonderregel (nur die
// Oben-Flaeche faerben), s. push_face_quad-Aufrufer.
const FACE_UP: usize = 1;
// Die uebrigen Atlas-Flaechenindizes (s. assets::FACE_*) -- gebraucht in
// push_waterlogged_fill, um je Seite die richtigen Eckhoehen zuzuordnen.
const FACE_DOWN: usize = 0;
const FACE_NORTH: usize = 2;
const FACE_SOUTH: usize = 3;
const FACE_WEST: usize = 4;

// Mauern/Zaeune bestehen aus MEHREREN Boxen desselben States (Pfosten + bis zu vier Arme,
// s. assets::shapes/template_wall_post.json/template_wall_side_tall.json). Ihre Ober-/
// Untenflaechen liegen bei "tall" verbundenen Armen exakt auf derselben Hoehe wie die des
// Pfostens UND ueberlappen sich im Grundriss (der Arm reicht bis zur Blockmitte, ueber den
// Pfostenrand hinaus) -- zwei deckungsgleiche, aber NICHT bit-identische Flaechen (andere
// Vertices), macht Z-Fighting (flackernde Mauer-Oberseiten). Um DIESEN Betrag
// wird jede zusaetzliche ueberlappende Flaeche nach innen verschoben (s.
// coplanar_conflict_count) -- unsichtbar, weil die Boxen massiv sind und der Bereich ohnehin
// von der jeweils anderen, nicht verschobenen Box ausgefuellt bleibt.
const WALL_ZFIGHT_EPS: f32 = 1.0 / 1024.0;

// Wie viele FRUEHERE Boxen (kleinerer Index) IM SELBEN State dieselbe Richtung (`atlas_face`,
// nur FACE_UP/FACE_DOWN kommen infrage) auf EXAKT derselben Hoehe deklarieren UND sich im
// Grundriss (X/Z) mit `this` ueberlappen. Die Box mit dem kleineren Index "gewinnt" (bleibt
// an der echten Zellgrenze); jede mit Konflikt wird vom Aufrufer um `WALL_ZFIGHT_EPS` je
// Treffer nach innen verschoben -- deterministisch IMMER hinter der gewinnenden Flaeche,
// statt zufaellig je nach Rundungsfehler der beiden (unterschiedlich berechneten) Flaechen.
fn coplanar_conflict_count(state_shapes: &[ShapeBox], this_index: usize, atlas_face: usize) -> u32 {
    let this = &state_shapes[this_index];
    let up = atlas_face == FACE_UP;
    let this_y = if up { this.max[1] } else { this.min[1] };
    let mut count = 0;
    for other in &state_shapes[..this_index] {
        if !other.faces[atlas_face] {
            continue;
        }
        let other_y = if up { other.max[1] } else { other.min[1] };
        if (other_y - this_y).abs() > 1e-4 {
            continue;
        }
        let overlap_x = this.min[0].max(other.min[0]) < this.max[0].min(other.max[0]) - 1e-4;
        let overlap_z = this.min[2].max(other.min[2]) < this.max[2].min(other.max[2]) - 1e-4;
        if overlap_x && overlap_z {
            count += 1;
        }
    }
    count
}

// Die Helligkeiten sind Vanillas ClientWorld.getBrightness(direction, shaded) fuer die
// Oberwelt: unten 0.5, oben 1.0, NORD/SUED 0.8, WEST/OST 0.6 -- dieselben Werte stehen auch
// als Konstruktorargument in BlockModelRenderer.NeighborData. Bis 2026-09-16 standen Ost/West
// und Nord/Sued hier VERTAUSCHT (Ost/West 0.8, Nord/Sued 0.6); aufgefallen beim Einbau der
// weichen Beleuchtung, die genau diese Werte mit der Eckabdunklung multipliziert.
// Shader-Pfad: keine feste Richtungs-Helligkeit im Mesh. Dort beleuchtet der Licht-Pass nach
// der echten Sonnenrichtung; Vanillas feste Werte (Osten/Westen 60 %, ...) liessen eine
// Wand, auf die die Sonne scheint, trotzdem dunkel aussehen. Prozessweit, weil die
// Mesh-Worker keinen Zugriff auf die Einstellungen haben; beim Umschalten wird alles neu
// gemesht. Wasser und block_displays behalten die Vanilla-Werte (Forward-Licht).
static FLAT_FACE_SHADE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_flat_face_shade(flat: bool) {
    FLAT_FACE_SHADE.store(flat, std::sync::atomic::Ordering::Relaxed);
}

fn face_shade(brightness: f32) -> f32 {
    if FLAT_FACE_SHADE.load(std::sync::atomic::Ordering::Relaxed) {
        1.0
    } else {
        brightness
    }
}

const FACES: [([i32; 3], f32, usize); 6] = [
    ([1, 0, 0], 0.6, 5),  // Ost   -> FACE_EAST
    ([-1, 0, 0], 0.6, 4), // West  -> FACE_WEST
    ([0, 1, 0], 1.0, 1),  // Oben  -> FACE_UP
    ([0, -1, 0], 0.5, 0), // Unten -> FACE_DOWN
    ([0, 0, 1], 0.8, 3),  // Sued  -> FACE_SOUTH
    ([0, 0, -1], 0.8, 2), // Nord  -> FACE_NORTH
];

// Ziel-Spalte + die vier orthogonalen UND vier diagonalen Nachbarn. Frueher genuegten die
// vier orthogonalen (FACES sind Einheitsvektoren, es aenderte sich nie mehr als eine Achse
// gleichzeitig) -- seit Wasser seine Eck-Hoehen aus bis zu 8 Nachbarn mittelt (s. world::
// fluid::calculate_corner_height/push_fluid_quads) braucht block_at() auch Positionen, die
// BEIDE Achsen gleichzeitig verlassen (eine Wasserzelle an einer Spalten-ECKE). None = Nachbar
// noch nicht geladen, zaehlt als Luft -- wie bei den orthogonalen Nachbarn auch.
pub struct ColumnNeighbors {
    pub target: Arc<ChunkColumn>,
    pub north: Option<Arc<ChunkColumn>>,     // z - 1
    pub south: Option<Arc<ChunkColumn>>,     // z + 1
    pub east: Option<Arc<ChunkColumn>>,      // x + 1
    pub west: Option<Arc<ChunkColumn>>,      // x - 1
    pub northeast: Option<Arc<ChunkColumn>>, // x + 1, z - 1
    pub northwest: Option<Arc<ChunkColumn>>, // x - 1, z - 1
    pub southeast: Option<Arc<ChunkColumn>>, // x + 1, z + 1
    pub southwest: Option<Arc<ChunkColumn>>, // x - 1, z + 1
}

// Bequemer Wrapper um mesh_column_from_neighbors() fuer Aufrufer mit einer &World zur
// Hand (Tests, Einzelaufrufe). Fuer den Dauerbetrieb (App::mesh_pending_columns) wird
// stattdessen World::column_neighbors() + mesh_column_from_neighbors() direkt auf einem
// Worker-Thread verwendet, s. Modul-Kommentar oben.
// Rueckgabe (opaque, translucent): getrennte Vertex-Listen, weil Wasser (echte Wasserbloecke
// UND die Fuellung waterlogged-Bloecke) einen eigenen Render-Pass mit echtem Alpha-Blending
// braucht (s. render::BlockRenderer) -- der Hauptpass zeichnet weiter blend-los mit
// Alpha-Cutout (Glas-/Laubraender, UND, damit das Gras-Overlay nicht blau wirkt, auch
// die grass_block-Seiten-Ueberlagerung, s. Aufrufer-Kommentar bei uvs.overlay_rect weiter
// unten: die war testweise ein eigener Translucent-Pass, aber zwei ungeordnete
// Translucent-Flaechen -- Wasser und Overlay -- ohne Tiefenschreiben sortieren sich
// gegeneinander nicht zuverlaessig; im Opak-Pass testet Wasser stattdessen ganz normal
// gegen die davor geschriebene Overlay-Tiefe).
pub fn mesh_column(
    world: &World,
    chunk_x: i32,
    chunk_z: i32,
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    biomes: &BiomePalette,
    cross: &CrossBlocks,
    cross_extra_shapes: &BlockShapes,
) -> (Vec<BlockVertex>, Vec<BlockVertex>) {
    let Some(neighbors) = world.column_neighbors(chunk_x, chunk_z) else {
        return (Vec::new(), Vec::new());
    };
    mesh_column_from_neighbors(chunk_x, chunk_z, &neighbors, uvs, shapes, biomes, cross, cross_extra_shapes)
}

// Meshed eine ganze Chunk-Spalte in Weltkoordinaten. Leere Sektionen (nur Luft --
// bei einer normalen Spalte ist das der Grossteil, naemlich der ganze Himmel) werden
// uebersprungen, ohne ihre 4096 Bloecke einzeln anzufassen.
pub fn mesh_column_from_neighbors(
    chunk_x: i32,
    chunk_z: i32,
    neighbors: &ColumnNeighbors,
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    biomes: &BiomePalette,
    cross: &CrossBlocks,
    cross_extra_shapes: &BlockShapes,
) -> (Vec<BlockVertex>, Vec<BlockVertex>) {
    mesh_column_with_lighting(chunk_x, chunk_z, neighbors, uvs, shapes, biomes, cross, cross_extra_shapes, 0.0, false)
}

// Barriere, Lichtblock und Strukturleere: in Vanilla unsichtbar (ihre Modelle haben keine
// Elemente, nur eine Partikeltextur). Ohne Sonderfall fielen sie auf den Vollwuerfel-Notnagel
// mit eben dieser Partikeltextur und waeren ueberall
// sichtbar. Sichtbar nur noch mit dem Debug-Modul "Barrieren anzeigen" (`show_markers`).
pub fn is_marker_block(name: &str) -> bool {
    matches!(name, "barrier" | "light" | "structure_void")
}

// Wie mesh_column_from_neighbors, mit Vanillas weicher Beleuchtung (Eckabdunklung, s.
// ambient_occlusion) in waehlbarer Staerke: `ao_strength` 0 = aus, 1 = wie Vanilla,
// dazwischen wird die Abdunklung anteilig eingemischt. Die flache Variante bleibt der Standard der bestehenden
// Aufrufer und Tests -- sie ist der Fall, gegen den die uebrige Geometrie geprueft ist.
#[allow(clippy::too_many_arguments)]
pub fn mesh_column_with_lighting(
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
) -> (Vec<BlockVertex>, Vec<BlockVertex>) {
    let (opaque, translucent, _) = mesh_column_ao_fading(chunk_x, chunk_z, neighbors, uvs, shapes, biomes, cross, cross_extra_shapes, ao_strength, show_markers, &[]);
    (opaque, translucent)
}

// Eine gerade geaenderte Stelle fuers Einblenden der Eckabdunklung: Position und der Zustand
// VORHER (s. mesh_column_ao_fading).
pub type AoFadeSource = ((i32, i32, i32), u32);

// Ein Quad im OPAKEN Mesh, dessen Eckabdunklung sich durch eine AoFadeSource geaendert hat:
// erster Vertex und die Vertices wie vorher bzw. wie jetzt (gleich lang).
#[derive(Clone, Debug)]
pub struct AoFadeQuad {
    pub start: u32,
    // Liegt im DURCHSICHTIGEN Puffer (Wasser, Glas) statt im opaken -- `start` zaehlt dann
    // dort. Die App blendet nur opake Flaechen ein.
    pub translucent: bool,
    pub before: Vec<BlockVertex>,
    pub after: Vec<BlockVertex>,
}

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

    let mut opaque = Vec::new();
    let mut translucent = Vec::new();
    // Zeitversetzte Animationen (s. desync_animations): der zuletzt begonnene Block und wo
    // seine Vertices in beiden Listen anfangen. Abgeschlossen wird er erst beim NAECHSTEN
    // Block (oder nach der Schleife) -- der Rumpf hat zu viele `continue`, um es am Ende zu tun.
    let desync = uvs.has_anim_variants();
    let mut pending: Option<((i32, i32, i32), usize, usize, usize)> = None;
    for (index, section) in neighbors.target.sections.iter().enumerate() {
        if section.is_empty() {
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
                            push_waterlogged_fill(&mut translucent, origin, x, y, z, colors.water, uvs, shapes, &block_at, [false; 6]);
                        }
                        continue;
                    }
                    let (tint, tint_only_up) = if is_water(name) {
                        (colors.water, false)
                    } else {
                        match tint_kind(name) {
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
                            push_waterlogged_fill(&mut translucent, origin, x, y, z, colors.water, uvs, shapes, &block_at, [false; 6]);
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
                            push_waterlogged_fill(&mut translucent, origin, x, y, z, colors.water, uvs, shapes, &block_at, [false; 6]);
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
                        push_fluid_quads(&mut translucent, origin, x, y, z, state, FluidKind::Water, colors.water, uvs, shapes, &block_at);
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
                                let content_tint = if name == "water_cauldron" { colors.water } else { [1.0, 1.0, 1.0] };
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
                            let ao = (ao_strength > 0.0 && shapes.ao_flags(state) & crate::ambient_occlusion::AO_SKIP == 0).then(|| {
                                let ecken = crate::ambient_occlusion::face_corners(
                                    (x, y, z),
                                    atlas_face,
                                    // Vanillas field_58160: Quad auf der Zellgrenze ODER der
                                    // Block ist ein Vollwuerfel (dann zaehlt auch ein nach
                                    // innen versetztes Quad als Aussenflaeche).
                                    touches_cell_boundary(shape_box, offset)
                                        || shapes.ao_flags(state) & crate::ambient_occlusion::AO_FULL_CUBE != 0,
                                    |bx, by, bz| shapes.ao_flags(block_at(bx, by, bz)),
                                );
                                face_quad_corners(shape_box.min, shape_box.max, offset).map(|lokal| {
                                    let voll = crate::ambient_occlusion::vertex_ao(atlas_face, ecken, lokal);
                                    1.0 - (1.0 - voll) * ao_strength
                                })
                            });
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
                                        let ecken = crate::ambient_occlusion::face_corners(
                                            (x, y, z),
                                            atlas_face,
                                            touches_cell_boundary(shape_box, offset)
                                                || shapes.ao_flags(state) & crate::ambient_occlusion::AO_FULL_CUBE != 0,
                                            |bx, by, bz| shapes.ao_flags(alt(bx, by, bz)),
                                        );
                                        Some(face_quad_corners(shape_box.min, shape_box.max, offset).map(|lokal| {
                                            let voll = crate::ambient_occlusion::vertex_ao(atlas_face, ecken, lokal);
                                            1.0 - (1.0 - voll) * ao_strength
                                        }))
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
                                        push_face_quad_ao(&mut before, quad_min, quad_max, offset, shade, vorher, uv);
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
                                                push_face_quad_ao(&mut before, quad_min, quad_max, offset, overlay_shade, vorher, overlay_uv);
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
                        push_waterlogged_fill(&mut translucent, origin, x, y, z, colors.water, uvs, shapes, &block_at, own_faces);
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
fn finish_desync(
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
fn desync_animations(verts: &mut [BlockVertex], uvs: &BlockUvs, (x, y, z): (i32, i32, i32)) {
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

// Fuellt eine waterlogged-Zelle mit Wasser -- eigenstaendiger Wuerfel ueber den gesamten
// Zellraum (0..1), unabhaengig von der tatsaechlichen (oft duenneren) Form des eigentlichen
// Blocks: eine exakte Aussparung um z.B. einen Zaunpfosten waere ungleich aufwaendiger und
// der optische Unterschied bei einer duennen Geometrie kaum sichtbar. `own_faces` (s.
// Aufrufer) blendet Richtungen aus, die der Block selbst schon deckt (Z-Fighting-Vermeidung,
// z.B. eine waterlogged Treppenstufe deckt ihre eigene Unterseite bereits opak ab).
#[allow(clippy::too_many_arguments)] // wie push_fluid_quads: Zellposition, Farbe, UVs, Formen
                                     // und Nachbarzugriff gehoeren alle zwingend dazu
fn push_waterlogged_fill(
    out: &mut Vec<BlockVertex>,
    origin: ColumnOrigin,
    x: i32,
    y: i32,
    z: i32,
    tint: [f32; 3],
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    block_at: &impl Fn(i32, i32, i32) -> u32,
    own_faces: [bool; 6],
) {
    use crate::fluid::fluid_kind;

    let same_fluid = |s: u32| fluid_kind(s) == Some(FluidKind::Water);
    let name_at = |s: u32| block_name(s).unwrap_or("");

    let (lx, ly, lz) = origin.local(x, y, z);
    let world_min = [lx, ly, lz];
    let world_max = [lx + 1.0, ly + 1.0, lz + 1.0];

    // Eckhoehen einmal vorab: sie werden sowohl fuer die Geometrie (Oberflaeche UND Waende)
    // als auch fuer die Verdeckungspruefung gebraucht -- Vanilla testet je Seite gegen die
    // tatsaechliche WASSERHOEHE, nicht gegen einen vollen Wuerfel (s. water_side_box).
    let (o, p, q, r) = fluid_corner_heights(x, y, z, FluidKind::Water, block_at);
    let hoehe_der_seite = |atlas_face: usize| match atlas_face {
        FACE_DOWN => FLUID_DOWN_HEIGHT,
        FACE_UP => p.min(r).min(q.min(o)),
        FACE_NORTH => p.max(o),
        FACE_SOUTH => r.max(q),
        FACE_WEST => p.max(r),
        _ => o.max(q),
    };

    // DIESELBE Verdeckungspruefung wie bei echtem Wasser (push_fluid_quads::occludes).
    //
    // Hier stand frueher nur `is_solid(neighbor)`, also "Nachbar ist nicht Luft" -- eine viel
    // zu grobe Regel, die JEDE Wasserflaeche an JEDEM beliebigen Nachbarn verschluckt hat.
    // Eine waterlogged Stufe neben einer normalen Stufe verlor so ihre Wasserwand komplett,
    // obwohl die Nachbarstufe nur bis zur halben Hoehe reicht und Vanilla dort sehr wohl die
    // Fliessflaeche zeichnet; dasselbe an Zaeunen, Fackeln, Pflanzen, Treppen. Echtes Wasser
    // macht es seit laengerem richtig, dieser Pfad war der letzte mit der groben Regel --
    // und er ist genau der Pfad, der waterlogged Stufen zeichnet.
    let occludes = |neighbor: u32, offset: [i32; 3], atlas_face: usize| {
        is_solid(neighbor)
            && shapes.has_resolved_shape(neighbor)
            && !never_occludes_neighbors(name_at(neighbor))
            // Glas ausgenommen wie bei echtem Wasser -- seine Culling-Form ist leer, weil es
            // nicht opak ist (ausfuehrlich s. push_fluid_quads::occludes).
            && !is_glass(name_at(neighbor))
            && neighbor_covers_face(
                shapes.shapes_of(neighbor),
                offset,
                atlas_face,
                &water_side_box(hoehe_der_seite(atlas_face)),
            )
    };

    for (offset, brightness, atlas_face) in FACES {
        if own_faces[atlas_face] {
            continue;
        }
        let neighbor = block_at(x + offset[0], y + offset[1], z + offset[2]);
        // Nachbar gehoert zum selben Wasserkoerper (echtes Wasser, andere waterlogged Bloecke,
        // Seegras/Kelp) -> gemeinsame Flaeche entfaellt, sonst saehe man eine Naht mitten im
        // Wasser.
        if same_fluid(neighbor) || occludes(neighbor, offset, atlas_face) {
            continue;
        }
        // Oben-Flaeche: NICHT flach auf voller Zellhoehe wie die anderen fuenf Seiten,
        // sondern mit demselben Eck-Hoehen-Gefaelle wie richtiges Wasser (s.
        // fluid_corner_heights/push_fluid_quads) -- sonst haengt genau an dieser Zelle
        // (z.B. Seegras/Kelp mit always_wet-Wasserfuellung, s. fluid::always_wet, oder ein
        // waterlogged Zaunpfosten) die Oberflaeche IMMER auf y+1, waehrend das echte
        // Nachbarwasser ringsum ganz normal absackt (isolierte Quelle, Rand einer Flaeche,
        // ...) -- optisch eine hochgezogene Kante/Wand genau an dieser Stelle (etwa
        // bei Seegras).
        if atlas_face == FACE_UP {
            const EPS: f32 = 0.001;
            let corners = [
                [world_min[0], world_min[1] + p - EPS, world_min[2]],
                [world_min[0], world_min[1] + r - EPS, world_max[2]],
                [world_max[0], world_min[1] + q - EPS, world_max[2]],
                [world_max[0], world_min[1] + o - EPS, world_min[2]],
            ];
            let uv = uvs.water_rect(FACE_UP);
            let oberseite_start = out.len();
            push_quad_with_uv(out, corners, [brightness * tint[0], brightness * tint[1], brightness * tint[2]], uv);
            // Nur die Oberseite doppelseitig, wie bei echtem Wasser (s. push_rueckseiten).
            push_rueckseiten(out, oberseite_start);
            continue;
        }
        let uv = uvs.water_rect(atlas_face);
        let shade = [brightness * tint[0], brightness * tint[1], brightness * tint[2]];
        if atlas_face == FACE_DOWN {
            // Unterseite liegt flach auf der Zellunterkante -- da gibt es nichts anzupassen.
            push_face_quad(out, world_min, world_max, offset, shade, uv);
            continue;
        }
        // SEITENFLAECHEN muessen oben an derselben Eck-Hoehe enden wie die Oberflaeche.
        // Vorher lief hier push_face_quad ueber die VOLLE Zellbox: die Waende reichten bis
        // zur Zellkante (y+1), waehrend die Wasseroberflaeche darueber bei z.B. y+0.74 lag --
        // ein 0.26 hoher Wasserkragen rund um die Zelle, der ins Leere stand (etwa an
        // einer waterlogged Stufe). Echtes Wasser macht es laengst richtig, s.
        // push_fluid_quads; nur dieser Pfad war noch bei der flachen Vollbox geblieben.
        let (x0, y0, z0) = (world_min[0], world_min[1], world_min[2]);
        let (x1, z1) = (world_max[0], world_max[2]);
        // SEITENFLAECHEN UM EPS NACH INNEN -- derselbe Versatz, den Vanilla und (seit laengerem)
        // auch echtes Wasser hier machen (FluidRenderer: 0.001 von der Zellkante weg).
        // Dieser Pfad hatte ihn als einziger nie, und genau hier faellt er am staerksten ins
        // Gewicht: die Wasserwand einer waterlogged Stufe liegt ueber ihre untere Haelfte EXAKT
        // auf der opaken Seitenflaeche der Stufe selbst (beide bei x=0/x=1 bzw. z=0/z=1, die
        // Stufe reicht bis y+0.5). Zwei deckungsgleiche Flaechen haben bit-identische
        // interpolierte Tiefe, und der Translucent-Pass testet mit LESS (nicht LessEqual, s.
        // render::block_renderer) -- die Wasserwand faellt dort komplett durch den Tiefentest.
        // Mit dem Versatz liegt sie knapp VOR der Stufenflaeche und besteht ihn.
        const EPS: f32 = 0.001;
        let (zn, zs) = (z0 + EPS, z1 - EPS);
        let (xw, xe) = (x0 + EPS, x1 - EPS);
        // Eckzuordnung wie in push_fluid_quads: p=(x0,z0), r=(x0,z1), q=(x1,z1), o=(x1,z0).
        let (corners, h1, h2) = match atlas_face {
            FACE_NORTH => ([[x0, y0, zn], [x0, y0 + p, zn], [x1, y0 + o, zn], [x1, y0, zn]], p, o),
            FACE_SOUTH => ([[x1, y0, zs], [x1, y0 + q, zs], [x0, y0 + r, zs], [x0, y0, zs]], q, r),
            FACE_WEST => ([[xw, y0, z1], [xw, y0 + r, z1], [xw, y0 + p, z0], [xw, y0, z0]], r, p),
            _ => ([[xe, y0, z0], [xe, y0 + o, z0], [xe, y0 + q, z1], [xe, y0, z1]], o, q),
        };
        // Seitenflaechen tragen die Fliesstextur (water_flow) bzw. an Glas/Laub das
        // water_overlay -- exakt dieselbe Auswahl und dieselbe Halbkachel-UV wie bei echtem
        // Wasser, s. push_fluid_quads::seite und water_side_corner_uvs.
        let n = block_name(neighbor).unwrap_or("");
        let overlay = is_glass(n) || is_leaves(n);
        let rect = if overlay { uvs.water_overlay_rect().unwrap_or(uv) } else { uvs.water_flow_rect().unwrap_or(uv) };
        let start = out.len();
        push_quad_with_corner_uvs(out, corners, shade, water_side_corner_uvs(rect, h1, h2));
        if !overlay {
            push_rueckseiten(out, start);
        }
    }
}

// Die vier Eck-Hoehen (Nordwest/Nordost/Suedost/Suedwest, s. push_fluid_quads-Aufrufer fuer
// die genaue o/p/q/r-Zuordnung) einer Fluessigkeitszelle bei (x,y,z) -- ausgelagert aus
// push_fluid_quads, damit push_waterlogged_fill (Seegras/Kelp/waterlogged-Bloecke) fuer ihre
// eigene Oberflaeche exakt dieselbe Vanilla-Formel (FluidRenderer.getFluidHeight, s.
// world::fluid-Modulkommentar) nutzen kann statt einer immer vollen, flachen Zelle.
fn fluid_corner_heights(x: i32, y: i32, z: i32, kind: FluidKind, block_at: &impl Fn(i32, i32, i32) -> u32) -> (f32, f32, f32, f32) {
    use crate::fluid::{calculate_corner_height, render_height_at};

    let rh = |px: i32, py: i32, pz: i32| render_height_at(block_at(px, py, pz), block_at(px, py + 1, pz), kind, is_solid);
    let state = block_at(x, y, z);
    let above = block_at(x, y + 1, z);
    let n = render_height_at(state, above, kind, is_solid);
    if n >= 1.0 {
        return (1.0, 1.0, 1.0, 1.0);
    }
    let s_h = rh(x, y, z - 1);
    let t_h = rh(x, y, z + 1);
    let u_h = rh(x + 1, y, z);
    let v_h = rh(x - 1, y, z);
    let ne_h = rh(x + 1, y, z - 1);
    let nw_h = rh(x - 1, y, z - 1);
    let se_h = rh(x + 1, y, z + 1);
    let sw_h = rh(x - 1, y, z + 1);
    (
        calculate_corner_height(n, s_h, u_h, ne_h),
        calculate_corner_height(n, s_h, v_h, nw_h),
        calculate_corner_height(n, t_h, u_h, se_h),
        calculate_corner_height(n, t_h, v_h, sw_h),
    )
}

// "Meine eigene Form" fuer neighbor_covers_face() beim Wasser -- eine Zellbox, die nur bis
// zur tatsaechlichen Fluessigkeitshoehe `height` reicht, NICHT bis 1.0.
//
// Das ist der Unterschied, der frueher ein sichtbares Z-Fighting erzeugt hat (da wo das Wasser
// das Farmland beruehrt, flimmerte eine fremde Textur): Vanillas
// FluidRenderer prueft die Nachbar-Abdeckung gegen die WASSERHOEHE, nicht gegen einen vollen
// Wuerfel -- shouldSkipRendering(direction, Math.max(adx, yx), neighborState) fuer die Seiten,
// mit dem Festwert 8/9 fuer unten (s. Yarn-1.21.11-Fassung von FluidRenderer.render).
// Farmland ist nur 15/16 hoch (template_farmland.json: Element [0,0,0]..[16,15,16]) und deckt
// damit zwar die 8/9 hohe Wasserflaeche komplett ab, aber eben keinen vollen Wuerfel. Gegen
// eine 1.0-Box getestet galt der Nachbar deshalb als "deckt nicht", die Wasser-Seitenflaeche
// wurde gezeichnet -- exakt koplanar mit der Erdseite des Farmlands, die an derselben
// Zellgrenze liegt. Zwei koplanare Flaechen = Z-Fighting; man sah abwechselnd Wasser- und
// Dirt-Textur ueber die gemeinsame Flaeche gesprenkelt.
fn water_side_box(height: f32) -> ShapeBox {
    ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, height, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] }
}

// Vanillas Festwert fuer die Unterseite (FluidRenderer.render prueft die Unterseite mit
// 0.8888889) -- bewusst NICHT die tatsaechliche Eckhoehe: nach unten ist die Zelle
// immer bis zur Zellgrenze gefuellt, die 8/9 stehen dort nur fuer "eine Quelle hoch".
const FLUID_DOWN_HEIGHT: f32 = 8.0 / 9.0;

// Vanilla-getreue Fluessigkeitsoberflaeche MIT Eck-Hoehen-Gefaelle (FluidRenderer.render()
// aus dem Yarn-1.21.11-Jar nachgesehen, s. world::fluid::calculate_corner_height/
// render_height_at) statt eines immer vollen, flachen Wuerfels -- der bisherige Stand
// (Level-Property nur fuer die Schwimmphysik, nie fuers Aussehen). Eigener Zeichenpfad statt der generischen Box-Schleife oben: eine
// Fluessigkeitszelle hat IMMER genau eine volle Zellbox (kein Content/xz-Offset), aber VIER
// unabhaengige Eckhoehen statt einer einzigen Flaeche -- ein [u0,v0,u1,v1]-UV-Rechteck kann
// das nicht ausdruecken, push_face_quad bleibt hier deshalb nur fuer die flache Unterseite
// nutzbar (s. render_down unten).
//
// GILT FUER BEIDE FLUESSIGKEITEN. Vanillas FluidRenderer.render zeichnet Wasser und Lava mit
// derselben Methode und derselben Geometrie; die Frage "ist das Lava?"
// entscheidet dort nur ueber drei Dinge, und genau die drei haengen hier an `kind`:
//   1. die Sprites (lava_still/lava_flow statt water_still/water_flow),
//   2. den Tint (Lava weiss, Wasser die Biom-Wasserfarbe),
//   3. die water_overlay-Ausnahme an Glas/Laub, die nur im Nicht-Lava-Zweig greift.
// Lava lief frueher stattdessen ueber den Vollwuerfel-Fallback der Box-Schleife und stand
// damit einen ganzen Block hoch statt 8/9. Der Aufrufer legt Lava
// ausserdem in den OPAKEN Puffer statt in den Translucent-Pass -- auch das ist Vanilla
// (RenderLayers.getFluidLayer: nur FluidTags.WATER bekommt translucent).
//
// BEWUSST NICHT nachgebildet (bewusste Vereinfachung, s. Klassenkommentar-Stil dieses
// Moduls): "isSolid()" (Vanilla-Materialeigenschaft) wird durch
// is_solid() angenaehert (alles ausser Luft zaehlt als Deckungs-Kandidat).
#[allow(clippy::too_many_arguments)]
fn push_fluid_quads(
    out: &mut Vec<BlockVertex>,
    origin: ColumnOrigin,
    x: i32,
    y: i32,
    z: i32,
    state: u32,
    kind: FluidKind,
    tint: [f32; 3],
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    block_at: &impl Fn(i32, i32, i32) -> u32,
) {
    use crate::fluid::fluid_kind;

    let same_fluid = |s: u32| fluid_kind(s) == Some(kind);

    let above = block_at(x, y + 1, z);
    let below = block_at(x, y - 1, z);
    let north = block_at(x, y, z - 1);
    let south = block_at(x, y, z + 1);
    let west = block_at(x - 1, y, z);
    let east = block_at(x + 1, y, z);

    // FRUEHER AUSSTIEG VOR den Eckhoehen: eine Zelle, deren sechs Nachbarn alle dieselbe
    // Fluessigkeit sind, zeichnet garantiert nichts (jede der sechs render_*-Bedingungen unten
    // beginnt mit !same_fluid). In einem Ozean ist das die grosse Mehrheit aller Zellen, und
    // fluid_corner_heights kostet je Zelle 18 block_at-Aufrufe (9 Positionen mal Zelle+darueber)
    // plus vier gewichtete Mittelungen. Die standen vorher IMMER an, auch fuer jede
    // vollstaendig eingeschlossene Zelle -- ein grosser Wasserblock laggte deshalb total.
    if same_fluid(above) && same_fluid(below) && same_fluid(north) && same_fluid(south) && same_fluid(west) && same_fluid(east) {
        return;
    }

    let (o, p, q, r) = fluid_corner_heights(x, y, z, kind, block_at);

    let name_at = |s: u32| block_name(s).unwrap_or("");
    // `height` = die Fluessigkeitshoehe, die der Nachbar abdecken muss, damit die Flaeche
    // entfaellt (s. water_side_box-Kommentar). Je Seite das Maximum der beiden Eckhoehen an
    // ihrer Kante -- genau Vanillas Math.max(adx, yx).
    let occludes = |neighbor: u32, offset: [i32; 3], atlas_face: usize, height: f32| {
        is_solid(neighbor)
            && shapes.has_resolved_shape(neighbor)
            && !never_occludes_neighbors(name_at(neighbor))
            // Glas verdeckt auch Wasser nicht. Der Weg dorthin ist in Vanilla anders als bei
            // Bloecken, das Ergebnis aber dasselbe -- und beim ersten Anlauf hatte ich es
            // falsch herum:
            //
            // FluidRenderer.isSideCovered fragt die FORM ab (BlockState.getCullingFace) und
            // nicht die Deckkraft. Naheliegend, aber falsch, waere der Schluss "Glas ist ein
            // Vollwuerfel, also verdeckt es". Die Form wird naemlich erst gar nicht gefuellt,
            // wenn der Block nicht opak ist -- AbstractBlock, ShapeCache:
            //     cullingShape = opaque ? owner.getCullingShape(state) : VoxelShapes.empty()
            // Glas ist nonOpaque, seine Culling-Form ist also LEER, und isSideCovered steigt
            // bei leerer Form sofort mit false aus.
            && !is_glass(name_at(neighbor))
            && neighbor_covers_face(shapes.shapes_of(neighbor), offset, atlas_face, &water_side_box(height))
    };

    // Eckzuordnung wie beim Oberflaechen-Quad weiter unten: p=(x0,z0), r=(x0,z1), q=(x1,z1),
    // o=(x1,z0). Eine Seite liegt jeweils an zwei dieser Ecken.
    let render_up = !same_fluid(above);
    let render_down = !same_fluid(below) && !occludes(below, [0, -1, 0], 0, FLUID_DOWN_HEIGHT);
    let render_north = !same_fluid(north) && !occludes(north, [0, 0, -1], 2, p.max(o));
    let render_south = !same_fluid(south) && !occludes(south, [0, 0, 1], 3, r.max(q));
    let render_west = !same_fluid(west) && !occludes(west, [-1, 0, 0], 4, p.max(r));
    let render_east = !same_fluid(east) && !occludes(east, [1, 0, 0], 5, o.max(q));

    if !(render_up || render_down || render_north || render_south || render_west || render_east) {
        return;
    }

    const EPS: f32 = 0.001;
    let w = if render_down { EPS } else { 0.0 };
    let (o, p, q, r) = if render_up { (o - EPS, p - EPS, q - EPS, r - EPS) } else { (o, p, q, r) };

    let (x0, y0, z0) = origin.local(x, y, z);
    let (x1, z1) = (x0 + 1.0, z0 + 1.0);
    // Stand-Sprite: bei Wasser ueber das eigenstaendige water_rect (das auch die Fuellung
    // waterlogged-Bloecke braucht, die selbst gar kein "water"-State sind), bei Lava ganz
    // normal ueber die State-ID -- der Block "lava" traegt lava_still auf allen sechs Seiten
    // (s. assets::fixed_fluid_texture). Der Flaechenindex ist in beiden Faellen egal, beide
    // Sprites haben keine Ausrichtung.
    let (up_uv, side_uv, flow_rect) = match kind {
        FluidKind::Water => (uvs.water_rect(FACE_UP), uvs.water_rect(2), uvs.water_flow_rect()),
        FluidKind::Lava => (uvs.face_rect(state, FACE_UP), uvs.face_rect(state, 2), uvs.lava_flow_rect()),
    };

    // Grenzt eine Wasser-SEITE an Glas oder Laub, nimmt Vanilla eine eigene, deutlich
    // flachere Textur (water_overlay) statt der normalen Wasseroberflaeche -- FluidRenderer nimmt sie, wenn der Nachbar ein
    // TranslucentBlock oder LeavesBlock ist.
    // Das ist der Grund, warum Wasser hinter einer Glasscheibe in Vanilla als ruhige blaue
    // Flaeche erscheint und nicht als volle Wassergrafik. Sonst kommt die FLIESSTEXTUR (water_flow) zum Einsatz -- die senkrecht nach
    // unten laufende Grafik, die man an jeder Wasserkante sieht. Fehlt eine Kachel im Atlas,
    // bleibt es bei der Standtextur.
    //
    // Rueckgabe: (Eck-UVs, "ist Overlay"). Das zweite Feld entscheidet ueber die Rueckseite:
    // Vanilla gibt jede Seitenflaeche ein zweites Mal mit vertauschter Wicklung aus -- ausser
    // im Overlay-Fall.
    //
    // Fuer LAVA entfaellt der Overlay-Zweig komplett -- Vanilla haengt ihn an `if (!bl)`,
    // Lava bekommt an Glas/Laub also ganz normal ihr Fliess-Sprite (und damit auch die
    // doppelseitige Ausgabe unten).
    let flow_tile = flow_rect.unwrap_or(side_uv);
    let seite = |neighbor: u32, h1: f32, h2: f32| -> ([[f32; 2]; 4], bool) {
        let n = name_at(neighbor);
        if kind == FluidKind::Water && (is_glass(n) || is_leaves(n)) {
            (water_side_corner_uvs(uvs.water_overlay_rect().unwrap_or(side_uv), h1, h2), true)
        } else {
            (water_side_corner_uvs(flow_tile, h1, h2), false)
        }
    };

    let shade = |brightness: f32| [brightness * tint[0], brightness * tint[1], brightness * tint[2]];

    if render_up {
        let corners = [[x0, y0 + p, z0], [x0, y0 + r, z1], [x1, y0 + q, z1], [x1, y0 + o, z0]];
        // Fliessrichtung dieser Zelle -- dieselbe Funktion, die auch den Schub auf den
        // Spieler liefert (world::fluid), damit Optik und Physik nie auseinanderlaufen
        // koennen. Nur die OBERSEITE bekommt bisher die gedrehte Fliesstextur; die
        // Seitenflaechen bleiben bei der Standtextur (Vanilla texturiert auch sie mit dem
        // Fliess-Sprite, das ist hier noch offen).
        let flow = crate::fluid::flow_velocity_at(block_at, shapes, x, y, z);
        let corner_uv = water_top_corner_uvs(flow, up_uv, flow_rect);
        let oberseite_start = out.len();
        push_quad_with_corner_uvs(out, corners, shade(1.0), corner_uv);
        // NUR die Oberseite doppelseitig, damit man die Wasseroberflaeche auch von unten
        // sieht (man schwimmt darunter). Genau das -- und nur das -- macht Vanillas
        // FluidRenderer ebenfalls: dort werden dieselben vier Vertices ein zweites Mal in
        // umgekehrter Reihenfolge ausgegeben, s. den canFlowTo(up)-Zweig.
        push_rueckseiten(out, oberseite_start);
    }
    if render_down {
        push_face_quad(out, [x0, y0 + w, z0], [x1, y0 + w, z1], [0, -1, 0], shade(0.5), up_uv);
    }
    // Seitenflaechen um EPS NACH INNEN geruecken -- exakt wie Vanilla (FluidRenderer:
    // 0.001 von der Zellkante weg, je Richtung). Ohne das liegt die Wasserflaeche
    // deckungsgleich auf der Flaeche des Nachbarblocks. Bei einem opaken Nachbarn faellt das
    // nicht auf (die Flaeche wird ohnehin geculled), bei GLAS aber sind dann zwei
    // durchsichtige Flaechen exakt gleich tief -- und weil der Translucent-Pass keine Tiefe
    // schreibt, entscheidet die Zeichenreihenfolge zufaellig, welche oben liegt. Sichtbar
    // als blau flackernde Glaskante.
    let (zn, zs) = (z0 + EPS, z1 - EPS);
    let (xw, xe) = (x0 + EPS, x1 - EPS);
    let mut seitenflaeche = |corners: [[f32; 3]; 4], helligkeit: f32, neighbor: u32, h1: f32, h2: f32| {
        let (uv, overlay) = seite(neighbor, h1, h2);
        let start = out.len();
        push_quad_with_corner_uvs(out, corners, shade(helligkeit), uv);
        if !overlay {
            push_rueckseiten(out, start);
        }
    };
    if render_north {
        let corners = [[x0, y0 + w, zn], [x0, y0 + p, zn], [x1, y0 + o, zn], [x1, y0 + w, zn]];
        seitenflaeche(corners, 0.6, north, p, o);
    }
    if render_south {
        let corners = [[x1, y0 + w, zs], [x1, y0 + q, zs], [x0, y0 + r, zs], [x0, y0 + w, zs]];
        seitenflaeche(corners, 0.6, south, q, r);
    }
    if render_west {
        let corners = [[xw, y0 + w, z1], [xw, y0 + r, z1], [xw, y0 + p, z0], [xw, y0 + w, z0]];
        seitenflaeche(corners, 0.8, west, r, p);
    }
    if render_east {
        let corners = [[xe, y0 + w, z0], [xe, y0 + o, z0], [xe, y0 + q, z1], [xe, y0 + w, z1]];
        seitenflaeche(corners, 0.8, east, o, q);
    }
}

// Ergaenzt alle ab `start` erzeugten Dreiecke um ihre Rueckseite (vertauschte Wicklung).
//
// Noetig, seit die Translucent-Pipeline Rueckseiten verwirft (s. render-Crate). Angewandt auf
// die Wasser-OBERSEITE und auf die SEITENFLAECHEN -- genau die zwei Stellen, an denen Vanillas
// FluidRenderer dieselben vier Vertices ein zweites Mal in umgekehrter Reihenfolge ausgibt
// (canFlowTo(up)-Zweig bzw. ohne Overlay am Ende der
// HORIZONTAL-Schleife). Ohne die Seiten-Rueckseite fehlt die Wasserkante, sobald man IM Wasser
// steht und von innen dagegen schaut: die Fliesstextur an den Raendern fehlt.
//
// AUSGENOMMEN bleibt der Overlay-Fall (Nachbar Glas/Laub) -- auch das ist Vanilla, und es war
// schon einmal ein ECHTER FEHLER: eine fruehere Fassung verdoppelte JEDE Wasserflaeche
// bedingungslos. Von aussen sieht man dann durch die Scheibe zwei Wasserschichten uebereinander
// (die nahe Vorderseite plus die Rueckseite der fernen Flaeche), und das Blending laeuft
// doppelt -- in Vanilla ist dort nur ruhiges Blau.
//
// Glas bekommt diese Behandlung gar nicht: doppelseitiges Glas blendet ebenfalls zweimal und
// wirkt dadurch doppelt so dicht.
fn push_rueckseiten(out: &mut Vec<BlockVertex>, start: usize) {
    let mut rueck: Vec<BlockVertex> = Vec::with_capacity(out.len() - start);
    for tri in out[start..].chunks_exact(3) {
        rueck.extend_from_slice(&[tri[0], tri[2], tri[1]]);
    }
    out.extend_from_slice(&rueck);
}

// Achsenparallele Box zwischen `min` und `max` (Weltkoordinaten) als 36 Vertices
// (12 Dreiecke), mit derselben Richtungs-Helligkeit wie die Chunk-Faces -- fuer
// Entity-Boxen im Renderer. Bewusst hier statt im render-Crate: BlockVertex und die
// FACES-Konvention leben in diesem Modul.
pub fn box_vertices(min: [f32; 3], max: [f32; 3], color: [f32; 3], uv: [f32; 4]) -> Vec<BlockVertex> {
    let mut out = Vec::with_capacity(36);
    for (dir, brightness, _atlas_face) in FACES {
        let shaded = [color[0] * brightness, color[1] * brightness, color[2] * brightness];
        push_face_quad(&mut out, min, max, dir, shaded, uv);
    }
    out
}

// Ein einzelner Block als freistehende Geometrie -- alle sechs Seiten, ohne Nachbar-Culling,
// mit den echten Flaechentexturen des States. Fuer die halbdurchsichtigen Geisterbloecke
// (unbestaetigte Platzierungen, s. app::PendingBlock): die stecken bewusst NICHT in den
// Chunk-Daten und werden deshalb auch nicht vom Spalten-Mesher erfasst.
//
// Kein Culling gegen die Nachbarn: ein Geisterblock steht per Definition auf einem freien
// Platz, und die halbe Deckkraft macht die Rueckseiten ohnehin sichtbar. Die Form kommt aus
// denselben ShapeBoxes wie im Chunk-Mesh, ein Vollwuerfel ist also wirklich ein Wuerfel und
// eine Stufe wirklich eine Stufe.
// `base` ist die untere Ecke des Blocks, BEREITS relativ zum Bezugspunkt des Aufrufers
// (beim Client: die Kameraposition, in f64 abgezogen -- s. render::BlockRenderer zum Grund).
// Frueher stand hier die absolute Blockposition; bei grossen Weltkoordinaten war die in f32
// schon zerstoert, bevor sie ueberhaupt in den Puffer kam.
// Geisterblock an einer echten Weltposition -- wie block_ghost_vertices, aber auch fuer die
// beiden Blocksorten, die KEINE Quader-Formen haben und dort deshalb unsichtbar blieben:
// Kreuzmodelle (Cobweb, Pflanzen) und Fluessigkeiten (Wasser/Lava aus dem Eimer). Beide laufen
// durch DENSELBEN Zeichenpfad wie im Chunk-Mesh, damit der Geist genau so aussieht wie der
// Block, den der Server gleich bestaetigt. `block_at` liefert die Nachbarn (fuer Fluid-
// Eckhoehen und Culling), `water_tint` die Biomfarbe des Wassers.
#[allow(clippy::too_many_arguments)]
pub fn block_ghost_vertices_at(
    base: [f32; 3],
    pos: (i32, i32, i32),
    state: u32,
    uvs: &BlockUvs,
    shapes: &BlockShapes,
    cross: &CrossBlocks,
    water_tint: [f32; 3],
    block_at: &impl Fn(i32, i32, i32) -> u32,
) -> Vec<BlockVertex> {
    let name = block_name(state).unwrap_or("");
    let origin = ColumnOrigin { x: pos.0, z: pos.2 };
    let mut out = Vec::new();
    if cross.is_cross(state) {
        push_cross_quads(&mut out, origin, pos.0, pos.1, pos.2, [1.0, 1.0, 1.0], uvs.face_rect(state, 2));
    } else if name == "water" {
        push_fluid_quads(&mut out, origin, pos.0, pos.1, pos.2, state, FluidKind::Water, water_tint, uvs, shapes, block_at);
    } else if name == "lava" {
        push_fluid_quads(&mut out, origin, pos.0, pos.1, pos.2, state, FluidKind::Lava, [1.0, 1.0, 1.0], uvs, shapes, block_at);
    } else {
        return ghost_vertices_with_model(base, pos, state, uvs, shapes);
    }
    // Spalten-lokal mit Ursprung genau auf dem Block: x/z liegen schon bei 0..1, y ist die
    // absolute Hoehe -- auf den augenrelativen Ursprung umrechnen.
    for v in &mut out {
        v.position = [v.position[0] + base[0], v.position[1] - pos.1 as f32 + base[1], v.position[2] + base[2]];
    }
    out
}

// 0xRRGGBB als Farbe 0..1.
fn rgb_f32(rgb: u32) -> [f32; 3] {
    [((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0]
}

// Ein Block fuer ein block_display (BlockRenderManager.renderBlockAsEntity): die echte Form des
// States im Einheitswuerfel 0..1, ohne Welt -- also ohne Culling und ohne Biom. Die Faerbung
// ist deshalb Vanillas Vorgabe ohne Welt (BlockColors mit world == null): Gras 0x91BD59,
// Laub 0x48B518, trockenes Laub 0xA0A69C. Vorher stand an der Stelle das ITEM-Modell des
// Blocks -- ein Zaun als Inventarbild mit zwei Latten statt als der Pfosten mit genau den
// Armen, die der State nennt.
pub fn block_display_vertices(state: u32, uvs: &BlockUvs, shapes: &BlockShapes, cross: &CrossBlocks) -> Vec<BlockVertex> {
    let name = block_name(state).unwrap_or("");
    let farbe = |rgb: u32| [((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0];
    let (tint, nur_oben) = match tint_kind(name) {
        TintKind::None => ([1.0; 3], false),
        TintKind::GrassTop => (farbe(0x91BD59), true),
        TintKind::GrassAll => (farbe(0x91BD59), false),
        TintKind::FoliageAll => (farbe(0x48B518), false),
        TintKind::DryFoliageAll => (farbe(0xA0A69C), false),
        // Ohne Welt nimmt Vanilla fuer die Seerose ihre Item-Farbe.
        TintKind::Fixed(0x208030) => (farbe(0x71C35C), false),
        TintKind::Fixed(rgb) => (farbe(rgb), false),
    };
    let mut out = Vec::new();
    if cross.is_cross(state) {
        push_cross_quads(&mut out, ColumnOrigin { x: 0, z: 0 }, 0, 0, 0, tint, uvs.face_rect(state, 2));
        return out;
    }
    if let Some(quads) = shapes.model_quads_at(state, 0, 0, 0) {
        for q in quads {
            let color = [tint[0] * q.shade, tint[1] * q.shade, tint[2] * q.shade];
            let vert = |i: usize| BlockVertex { position: q.corners[i], color, uv: q.uvs[i] };
            out.extend_from_slice(&[vert(0), vert(2), vert(1), vert(0), vert(3), vert(2)]);
        }
        return out;
    }
    if is_marker_block(name) || name == "water" || name == "lava" {
        return out;
    }
    for shape_box in shapes.shapes_of(state) {
        for (dir, brightness, atlas_face) in FACES {
            if !shape_box.faces[atlas_face] {
                continue;
            }
            let t = if !nur_oben || atlas_face == FACE_UP { tint } else { [1.0; 3] };
            let uv = crop_face_uv(uvs.face_rect(state, atlas_face), shape_box.uv[atlas_face]);
            push_face_quad(&mut out, shape_box.min, shape_box.max, dir, [brightness * t[0], brightness * t[1], brightness * t[2]], uv);
        }
    }
    out
}

pub fn block_ghost_vertices(base: [f32; 3], state: u32, uvs: &BlockUvs, shapes: &BlockShapes) -> Vec<BlockVertex> {
    ghost_vertices_with_model(base, (0, 0, 0), state, uvs, shapes)
}

// Modelle mit gedrehten Elementen (Wandfackel, Laterne, Feuer, ...) als ihre echten Vierecke
// wie im Chunk-Mesh, sonst die Boxen. Frueher immer die Boxen: eine vorhergesagte Wandfackel
// stand dann einen Frame lang ungedreht quer im Block. `pos` waehlt die Fassung
// (s. BlockShapes::model_quads_at).
fn ghost_vertices_with_model(base: [f32; 3], pos: (i32, i32, i32), state: u32, uvs: &BlockUvs, shapes: &BlockShapes) -> Vec<BlockVertex> {
    let mut out = Vec::new();
    if let Some(quads) = shapes.model_quads_at(state, pos.0, pos.1, pos.2) {
        for q in quads {
            let color = [face_shade(q.shade); 3];
            let vert = |i: usize| BlockVertex {
                position: [base[0] + q.corners[i][0], base[1] + q.corners[i][1], base[2] + q.corners[i][2]],
                color,
                uv: q.uvs[i],
            };
            // Umlauf wie im Chunk-Mesh (s. dort).
            out.extend_from_slice(&[vert(0), vert(2), vert(1), vert(0), vert(3), vert(2)]);
        }
        return out;
    }
    for shape_box in shapes.shapes_of(state) {
        let min = [base[0] + shape_box.min[0], base[1] + shape_box.min[1], base[2] + shape_box.min[2]];
        let max = [base[0] + shape_box.max[0], base[1] + shape_box.max[1], base[2] + shape_box.max[2]];
        for (dir, brightness, atlas_face) in FACES {
            let uv = crop_face_uv(uvs.face_rect(state, atlas_face), shape_box.uv[atlas_face]);
            let shade = [face_shade(brightness); 3];
            push_face_quad(&mut out, min, max, dir, shade, uv);
        }
    }
    out
}

// Die Riss-Ueberlagerung beim Abbauen -- Vanillas LevelRenderer.renderBlockDestroyAnimation.
//
// Gezeichnet wird KEIN geschrumpfter Kasten und kein Drahtgitter, sondern die echte Form des
// Blocks noch einmal, mit der Textur destroy_stage_0..9 darueber (Pipeline "crumbling",
// s. render::BlockRenderer). Genau wie in Vanilla:
//
//  - Die UV kommt NICHT aus der Flaeche, sondern aus der Position des Eckpunkts
//    (SheetedDecalTextureGenerator): der Riss wird wie eine Abziehfolie in Blockkoordinaten
//    auf die Flaeche projiziert, eine Kachel je Block. Eine Stufe zeigt deshalb nur die
//    untere Haelfte des Rissbilds statt ein gestauchtes Ganzes.
//  - `textureScale` ist 1.0, die Drehung je Richtung steckt in Direction.getRotation().
//    Ausmultipliziert (s. Test riss_uv_folgt_vanillas_projektion) ergeben sich die sechs
//    Achsenzuordnungen unten; negative Werte wiederholen sich in Vanilla ueber GL_REPEAT,
//    hier genuegt +1, weil eine Blockform nie breiter als eine Kachel ist.
//  - Flaechen, die ein Nachbar ohnehin vollstaendig verdeckt, fallen weg. Ohne das laegen
//    die Risse (durch den Tiefen-Bias nach vorn gezogen) auf der Nachbarwand.
//
// `base` ist der augenrelative Ursprung des Blocks, `stage_uv` die Atlas-Kachel der Stufe
// (s. BlockUvs::destroy_stage_rect).
pub fn block_breaking_vertices(
    base: [f32; 3],
    pos: (i32, i32, i32),
    state: u32,
    stage_uv: [f32; 4],
    shapes: &BlockShapes,
    block_at: &impl Fn(i32, i32, i32) -> u32,
) -> Vec<BlockVertex> {
    let [su0, sv0, su1, sv1] = stage_uv;
    let mut out = Vec::new();
    for shape_box in shapes.shapes_of(state) {
        for (dir, _brightness, atlas_face) in FACES {
            if !shape_box.faces[atlas_face] {
                continue;
            }
            let neighbor = block_at(pos.0 + dir[0], pos.1 + dir[1], pos.2 + dir[2]);
            if is_solid(neighbor) && shapes.has_resolved_shape(neighbor) && neighbor_covers_face(shapes.shapes_of(neighbor), dir, atlas_face, shape_box)
            {
                continue;
            }
            let local = face_quad_corners(shape_box.min, shape_box.max, dir);
            let raw = local.map(|c| breaking_uv(dir, c));
            // Die Verschiebung in den Kachelbereich gilt fuer das GANZE Viereck, nicht je
            // Ecke -- sonst risse eine Flaeche mit u/v von -0,5 bis 0 (z.B. die Seite einer
            // Stufe) in zwei verschiedene Kachelhaelften auseinander.
            let shift = |werte: [f32; 4]| -werte.iter().copied().fold(f32::INFINITY, f32::min).floor();
            let (du, dv) = (shift(raw.map(|(u, _)| u)), shift(raw.map(|(_, v)| v)));
            let verts: [BlockVertex; 4] = std::array::from_fn(|i| {
                let [x, y, z] = local[i];
                let (u, v) = (raw[i].0 + du, raw[i].1 + dv);
                BlockVertex {
                    position: [base[0] + x, base[1] + y, base[2] + z],
                    color: [1.0, 1.0, 1.0],
                    uv: [su0 + u * (su1 - su0), sv0 + v * (sv1 - sv0)],
                }
            });
            out.extend_from_slice(&[verts[0], verts[1], verts[2], verts[0], verts[2], verts[3]]);
        }
    }
    out
}

// Die Riss-UV eines Eckpunkts in Blockkoordinaten (0..1), s. block_breaking_vertices.
fn breaking_uv(dir: [i32; 3], p: [f32; 3]) -> (f32, f32) {
    let [x, y, z] = p;
    let (u, v) = match dir {
        [0, -1, 0] => (x, -z),
        [0, 1, 0] => (x, z),
        [0, 0, -1] => (-x, -y),
        [0, 0, 1] => (x, -y),
        [-1, 0, 0] => (-z, -y),
        [1, 0, 0] => (z, -y),
        _ => unreachable!("Face-Richtung muss ein Einheitsvektor sein"),
    };
    // Roh, also noch ohne die Verschiebung in den Kachelbereich -- die macht der Aufrufer
    // fuer das ganze Viereck auf einmal (in Vanilla uebernimmt das GL_REPEAT).
    (u, v)
}

// Waehlt den passenden Ausschnitt INNERHALB einer Atlas-Flaechenkachel (s. ShapeBox::uv) --
// `crop` ist normiert (0..1) relativ zur Kachel, `crop=None` liefert die volle Kachel
// unveraendert. Ohne das wird die ganze Textur ueber jede Flaeche gestreckt, egal wie klein
// sie ist (Beispiel: Falllaub mit segment_amount zeigte immer die komplette, gestauchte
// Grafik statt eines kleinen, dichten Ausschnitts -- betrifft genauso Stufen/Treppen, deren
// Modelle ebenfalls "uv"-Ausschnitte nutzen, um das Holzmaser-/Steinmuster ueber die Kanten
// hinweg fortzusetzen statt es zu verzerren). u0>u1 bzw. v0>v1 im Ausschnitt sind gueltig
// (Vanilla-Spiegeltrick, z.B. eine gespiegelte Unterseite) -- reine lineare Interpolation
// ohne Sortierung erhaelt das.
pub(crate) fn crop_face_uv(tile: [f32; 4], crop: Option<[f32; 4]>) -> [f32; 4] {
    let Some([u0, v0, u1, v1]) = crop else { return tile };
    let [tu0, tv0, tu1, tv1] = tile;
    let (tw, th) = (tu1 - tu0, tv1 - tv0);
    [tu0 + u0 * tw, tv0 + v0 * th, tu0 + u1 * tw, tv0 + v1 * th]
}

// Haengt die 6 Vertices (2 Dreiecke) fuer eine Seite einer achsenparallelen Box
// zwischen `min` und `max` (Weltkoordinaten) an `out` an. `dir` ist die Face-Normale,
// `uv` das Atlas-Rechteck (u0,v0,u1,v1) fuer diese Seite. Fuer volle Bloecke ist die Box
// genau eine Zelle (min=Blockposition, max=min+1); fuer Nicht-Wuerfel-Formen (Stufen,
// Zaeune, ...) eine der kleineren Boxen aus assets::shapes.
#[cfg(test)]
pub(crate) fn push_face_quad_for_test(out: &mut Vec<BlockVertex>, min: [f32; 3], max: [f32; 3], dir: [i32; 3]) {
    push_face_quad(out, min, max, dir, [1.0; 3], [0.0, 0.0, 1.0, 1.0]);
}

fn push_face_quad(out: &mut Vec<BlockVertex>, min: [f32; 3], max: [f32; 3], dir: [i32; 3], color: [f32; 3], uv: [f32; 4]) {
    push_quad_with_uv(out, face_quad_corners(min, max, dir), color, uv);
}

// Wie push_face_quad, aber mit einem Helligkeitsfaktor JE ECKE (weiche Beleuchtung). Die
// Faktoren stehen in derselben Reihenfolge wie face_quad_corners -- beide Funktionen
// muessen dieselbe Eckreihenfolge benutzen, deshalb gibt es sie nur einmal.
fn push_face_quad_ao(out: &mut Vec<BlockVertex>, min: [f32; 3], max: [f32; 3], dir: [i32; 3], color: [f32; 3], ao: [f32; 4], uv: [f32; 4]) {
    let corners = face_quad_corners(min, max, dir);
    let [u0, v0, u1, v1] = uv;
    let corner_uv = [[u0, v1], [u0, v0], [u1, v0], [u1, v1]];
    let vert = |i: usize| BlockVertex { position: corners[i], color: [color[0] * ao[i], color[1] * ao[i], color[2] * ao[i]], uv: corner_uv[i] };
    out.extend_from_slice(&[vert(0), vert(1), vert(2), vert(0), vert(2), vert(3)]);
}

// Die vier Eckpunkte einer Quaderseite, in der Reihenfolge, in der push_quad_with_uv sie zu
// zwei Dreiecken verbindet.
fn face_quad_corners(min: [f32; 3], max: [f32; 3], dir: [i32; 3]) -> [[f32; 3]; 4] {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;

    // Vier Eckpunkte der jeweiligen Seite, in einer Reihenfolge, die zwei Dreiecke
    // ergibt. Cull-Mode ist bewusst deaktiviert (s. render-Crate), daher ist die
    // Wicklungsrichtung hier unkritisch fuer die Sichtbarkeit.
    match dir {
        [1, 0, 0] => [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
        [-1, 0, 0] => [[x0, y0, z1], [x0, y1, z1], [x0, y1, z0], [x0, y0, z0]],
        [0, 1, 0] => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        [0, -1, 0] => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        [0, 0, 1] => [[x1, y0, z1], [x1, y1, z1], [x0, y1, z1], [x0, y0, z1]],
        [0, 0, -1] => [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
        _ => unreachable!("Face-Richtung muss ein Einheitsvektor sein"),
    }
}

// Zwei diagonale Ebenen ("X"-Form) fuer Kreuz-Bloecke (Zuckerrohr, Gras, Blumen, ...) --
// entspricht Vanillas "cross"/"tinted_cross"-Modell: zwei um 45 Grad gegeneinander
// gedrehte, volle Ebenen von Eckpunkt zu gegenueberliegendem Eckpunkt. `tint` wird direkt
// als Farbe verwendet (kein Richtungs-Shading, s. Vanillas "shade": false in cross.json).
//
// JEDE Ebene wird ZWEIMAL ausgegeben, einmal pro Wicklungsrichtung. Frueher genuegte ein
// Quad pro Ebene, weil der Cull-Mode aus war -- seit die Opak-Pipeline rueckseitige Flaechen
// verwirft (s. render-Crate), waere eine Pflanze sonst von genau einer Seite unsichtbar.
// Vanilla macht dasselbe: das cross-Modell definiert pro Ebene beide Faces (north UND
// south). Die Verdopplung kostet praktisch nichts -- Kreuz-Bloecke sind gegenueber
// Vollwuerfeln verschwindend selten, und ohne sie waere Culling gar nicht erst moeglich.
fn push_cross_quads(out: &mut Vec<BlockVertex>, origin: ColumnOrigin, x: i32, y: i32, z: i32, tint: [f32; 3], uv: [f32; 4]) {
    let (x0, y0, z0) = origin.local(x, y, z);
    let (x1, y1, z1) = (x0 + 1.0, y0 + 1.0, z0 + 1.0);
    let plane_a = [[x0, y0, z0], [x0, y1, z0], [x1, y1, z1], [x1, y0, z1]];
    let plane_b = [[x0, y0, z1], [x0, y1, z1], [x1, y1, z0], [x1, y0, z0]];
    for plane in [plane_a, plane_b] {
        push_quad_with_uv(out, plane, tint, uv);
        // Umgekehrte Eckenreihenfolge = umgekehrte Wicklung = die Rueckseite derselben
        // Ebene. Die UVs laufen dadurch gespiegelt, was bei den symmetrischen
        // Kreuz-Texturen nicht auffaellt -- und Vanilla spiegelt die Rueckseite ebenso.
        let rueckseite = [plane[3], plane[2], plane[1], plane[0]];
        push_quad_with_uv(out, rueckseite, tint, uv);
    }
}

// Bloecke, deren echte Vanilla-Kollisions-/Verdeckungsform NIE eine Nachbar-Flaeche voll
// abdeckt, obwohl ihr RENDER-Modell (assets::shapes, aus dem BLOCK_SHAPES kommt) das an
// einzelnen Grenzflaechen so aussehen laesst. Kaktus ist der bekannte Fall: sein Modell hat
// ein volles 0..16-Element mit "cullface":"up"/"down" (siehe cactus.json) -- das ist aber
// NUR eine Modellierungs-Abkuerzung, damit KAKTUS SELBST seine eigene Ober-/Unterseite
// gegen einen sturen Nachbarn (Sand) culln kann. Die tatsaechliche Vanilla-Form ist auf
// allen Seiten um 1px eingerueckt (nie randstaendig), verdeckt also NIE die Flaeche eines
// Nachbarn -- weder von oben/unten (der Sand UNTER einem Kaktus zeigt in echtem Minecraft
// einen sichtbaren schmalen Rand seiner Oberseite) noch seitlich. Ohne diese Ausnahme
// culln wir faelschlich anhand des vollen Render-Elements und der Nachbarblock (z.B. Sand
// unter dem Kaktus) wird komplett unsichtbar.
//
// Dazu alle Bloecke, die Vanilla als NICHT deckend fuehrt (Block.Settings.nonOpaque):
// Block.shouldDrawSide cullt nur gegen `isOpaque()`-Nachbarn. Falltueren und Tueren liegen
// als volle 16x16-Platte an der Zellkante und verdeckten hier die Wand dahinter -- durch ihre
// Loecher sah man dann ins Leere. Ebenso Spawner, Kupfergitter und Geruest.
pub(crate) fn never_occludes_neighbors(name: &str) -> bool {
    name == "cactus"
        || name.ends_with("_trapdoor")
        || name.ends_with("_door")
        || name.ends_with("_grate")
        || matches!(name, "spawner" | "trial_spawner" | "vault" | "scaffolding")
}

// Ob eine Boxflaeche an der Aussenkante der Block-Zelle (0..1 lokal) liegt -- nur dann
// ist der Nachbarblock ueberhaupt fuer das Culling relevant (s. Aufrufer). Eine Flaeche
// mitten in der Zelle (z.B. die Seite eines duennen Zaunpfostens) ist immer sichtbar.
fn touches_cell_boundary(b: &ShapeBox, offset: [i32; 3]) -> bool {
    const EPS: f32 = 1e-4;
    match offset {
        [1, 0, 0] => b.max[0] > 1.0 - EPS,
        [-1, 0, 0] => b.min[0] < EPS,
        [0, 1, 0] => b.max[1] > 1.0 - EPS,
        [0, -1, 0] => b.min[1] < EPS,
        [0, 0, 1] => b.max[2] > 1.0 - EPS,
        [0, 0, -1] => b.min[2] < EPS,
        _ => unreachable!("Face-Richtung muss ein Einheitsvektor sein"),
    }
}

// FACE_DOWN=0,FACE_UP=1,FACE_NORTH=2,FACE_SOUTH=3,FACE_WEST=4,FACE_EAST=5 (s. assets::FACE_*)
// -- fuer jede Richtung die gegenueberliegende (fuer den Nachbar-Deckungstest: meine Ost-
// Flaeche kann nur durch die WEST-Flaeche des Nachbarn verdeckt werden).
const OPPOSITE_FACE: [usize; 6] = [1, 0, 3, 2, 5, 4];

// Welche Koordinatenachse (Index in [f32;3]) die Flaechen-Normale bildet -- fuer den
// Nullstaerke-Ebenen-Test (s. Aufrufer): down/up=Y(1), north/south=Z(2), west/east=X(0).
// Nur noch von Tests gebraucht, seit die Nullstaerke-Kuerzung entfallen ist (s. dortigen
// Kommentar) -- bleibt stehen, weil die Zuordnung Flaeche->Achse jederzeit wieder gebraucht
// wird und sie nirgends sonst ausgeschrieben ist.
#[allow(dead_code)]
fn face_normal_axis(atlas_face: usize) -> usize {
    match atlas_face {
        0 | 1 => 1,
        2 | 3 => 2,
        _ => 0,
    }
}

// Die zwei Achsen-Indices (in [f32;3]), die in der Ebene der jeweiligen Flaeche liegen --
// z.B. die Ost/West-Flaeche (Normale = X-Achse) hat ihr Rechteck in Y/Z.
fn face_plane_axes(atlas_face: usize) -> (usize, usize) {
    match atlas_face {
        0 | 1 => (0, 2), // down/up: Normale Y -> Rechteck in X/Z
        2 | 3 => (0, 1), // north/south: Normale Z -> Rechteck in X/Y
        _ => (1, 2),     // west/east: Normale X -> Rechteck in Y/Z
    }
}

// Deckt IRGENDEINE Box des Nachbarn meine Flaeche `atlas_face` (meiner Box `my`, an der
// Grenze in Richtung `offset`) vollstaendig ab? Verdeckung braucht mehr als "Nachbar ist
// nicht Luft": der Nachbar muss an der GLEICHEN Grenze eine eigene Flaeche haben, die die
// volle Breite/Hoehe meiner Flaeche ueberdeckt -- eine Treppe, Stufe oder ein Zaun deckt
// eine Zellgrenze meist nur teilweise ab, s. Aufrufer-Kommentar.
// Liegt ein Modell-Viereck ganz in einer Grenzebene der Zelle? Dann: Richtung zum Nachbarn,
// die Flaeche (FACE_*) und seine Ausdehnung als flache Box fuer neighbor_covers_face.
pub(crate) fn boundary_quad(corners: &[[f32; 3]; 4]) -> Option<([i32; 3], usize, ShapeBox)> {
    const EPS: f32 = 1e-4;
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for c in corners {
        for i in 0..3 {
            min[i] = min[i].min(c[i]);
            max[i] = max[i].max(c[i]);
        }
    }
    // (Achse, Grenzwert) -> (Nachbar-Versatz, Flaeche)
    let (offset, face) = [
        (0, 0.0, [-1, 0, 0], 4usize),
        (0, 1.0, [1, 0, 0], 5),
        (1, 0.0, [0, -1, 0], 0),
        (1, 1.0, [0, 1, 0], 1),
        (2, 0.0, [0, 0, -1], 2),
        (2, 1.0, [0, 0, 1], 3),
    ]
    .into_iter()
    .find(|&(achse, grenze, _, _)| (min[achse] - grenze).abs() < EPS && (max[achse] - grenze).abs() < EPS)
    .map(|(_, _, o, f)| (o, f))?;
    Some((offset, face, ShapeBox { min, max, faces: [false; 6], content: [false; 6], uv: [None; 6] }))
}

fn neighbor_covers_face(neighbor_boxes: &[ShapeBox], offset: [i32; 3], atlas_face: usize, my: &ShapeBox) -> bool {
    const EPS: f32 = 1e-4;
    let opposite = OPPOSITE_FACE[atlas_face];
    let back = [-offset[0], -offset[1], -offset[2]];
    let (a0, a1) = face_plane_axes(atlas_face);
    neighbor_boxes.iter().any(|nb| {
        nb.faces[opposite]
            && touches_cell_boundary(nb, back)
            && nb.min[a0] <= my.min[a0] + EPS
            && nb.max[a0] >= my.max[a0] - EPS
            && nb.min[a1] <= my.min[a1] + EPS
            && nb.max[a1] >= my.max[a1] - EPS
    })
}

// Gemeinsamer Kern: vier Eckpunkte -> zwei Dreiecke, mit Standard-UV-Zuordnung. Die
// Eckreihenfolge ist bei allen FACES gleich aufgebaut (0 = unten-nah, 1 = oben-nah, 2 =
// oben-fern, 3 = unten-fern), daher passt eine feste Zuordnung: v0 ist die obere Kachelkante
// (RGBA-Zeilen laufen von oben), sodass Texturen an Seitenflaechen aufrecht stehen.
// Bloecke, die Vanilla in die TRANSLUCENT-Schicht legt und die deshalb echtes
// Alpha-Blending statt eines Alpha-Tests brauchen -- aus BlockRenderLayers des
// 1.21.11-Clients abgelesen, nicht geraten. Dort stehen ALLE Glasarten unter
// demselben Layer:
//   glass, glass_pane, tinted_glass,
//   <farbe>_stained_glass, <farbe>_stained_glass_pane
//
// Wichtig: auch KLARES Glas und klare Scheiben gehoeren dazu. Die naheliegende Annahme, nur
// gefaerbtes Glas sei "richtig" transparent und klares Glas ein Alpha-Cutout, ist falsch --
// Vanilla behandelt beide gleich.
//
// Laub steht bewusst NICHT hier: dessen Alpha wird beim Atlas-Bau ohnehin hart auf voll
// gesetzt (s. assets::is_leaves, feste Nutzerentscheidung "Grafik: Schnell").
// Feuer und Seelenfeuer (Vanillas AbstractFireBlock).
pub fn is_fire(name: &str) -> bool {
    name == "fire" || name == "soul_fire"
}

// Vanillas IronBarsBlock.skipRendering (Glasscheiben und Gitter): gegen denselben Block -- bei
// Gittern gegen jedes Gitter -- faellt die Flaeche nach oben und unten immer weg, zur Seite
// nur, wenn beide Bloecke zueinander verbunden sind. Sonst saehe man in einer Scheibenwand an
// jeder Blockgrenze den Kantenstreifen.
fn pane_skips_face(state: u32, neighbor: u32, offset: [i32; 3]) -> bool {
    let (Some(name), Some(other)) = (block_name(state), block_name(neighbor)) else { return false };
    let bars = |n: &str| n.ends_with("_bars");
    let pane = |n: &str| n.ends_with("_pane") || bars(n);
    if !pane(name) || !(name == other || (bars(name) && bars(other))) {
        return false;
    }
    let (mine, theirs) = match offset {
        [0, 0, -1] => ("north", "south"),
        [0, 0, 1] => ("south", "north"),
        [-1, 0, 0] => ("west", "east"),
        [1, 0, 0] => ("east", "west"),
        _ => return true,
    };
    property(state, mine) == Some("true") && property(neighbor, theirs) == Some("true")
}

pub fn is_glass(name: &str) -> bool {
    matches!(name, "glass" | "glass_pane" | "tinted_glass") || name.ends_with("_stained_glass") || name.ends_with("_stained_glass_pane")
}

// Laub -- gebraucht fuer dieselbe Wasser-Sonderregel wie Glas (s. push_fluid_quads:
// Vanillas FluidRenderer prueft `TranslucentBlock || LeavesBlock`). Absichtlich getrennt von
// is_glass: fuer die Pass-Zuordnung zaehlt Laub NICHT als transparent (sein Alpha wird beim
// Atlas-Bau hart auf voll gesetzt, s. assets::is_leaves), nur fuer die Wassertextur daneben.
pub fn is_leaves(name: &str) -> bool {
    name.ends_with("_leaves")
}

// Wie push_quad_with_uv, aber mit FREI gewaehlten UVs je Ecke statt eines achsenparallelen
// Rechtecks. Nur so laesst sich eine Textur GEDREHT auf eine Flaeche legen -- ein
// [u0,v0,u1,v1]-Rechteck kann eine Drehung nicht ausdruecken (derselbe Grund, aus dem die
// RNG-Blockrotation vorab gedrehte Kacheln backt, s. assets::rng_face_uvs). Gebraucht fuer
// die Wasser-Fliesstextur, die in Stroemungsrichtung zeigen muss.
fn push_quad_with_corner_uvs(out: &mut Vec<BlockVertex>, corners: [[f32; 3]; 4], color: [f32; 3], corner_uv: [[f32; 2]; 4]) {
    let vert = |i: usize| BlockVertex { position: corners[i], color, uv: corner_uv[i] };
    out.extend_from_slice(&[vert(0), vert(1), vert(2), vert(0), vert(2), vert(3)]);
}

// UVs der vier Ecken der Wasser-OBERSEITE -- Nachbau von FluidRenderer (Yarn-1.21.11-Jar
// nachgesehen). Die Eckenreihenfolge entspricht der von push_fluid_quads erzeugten:
// (x0,z0), (x0,z1), (x1,z1), (x1,z0) -- exakt Vanillas Vertex-Reihenfolge.
//
// Steht das Wasser (Stroemung 0/0), wird die Standtextur flaechig aufgezogen. Fliesst es,
// kommt die Fliesstextur um den Stroemungswinkel gedreht zum Einsatz -- und zwar nur ein
// HALBER Kachelausschnitt (+-0.25 um die Kachelmitte). Das ist kein Sparen, sondern noetig:
// beim Drehen eines vollen Ausschnitts liefen die Ecken aus der Kachel heraus und zoegen
// Nachbarkacheln aus dem Atlas herein.
fn water_top_corner_uvs(flow: Option<[f64; 3]>, still: [f32; 4], flow_tile: Option<[f32; 4]>) -> [[f32; 2]; 4] {
    // Punkt innerhalb einer Atlas-Kachel: (0,0) = obere linke Ecke der Kachel, (1,1) = untere
    // rechte -- Vanillas Sprite.getFrameU/getFrameV.
    let punkt = |rect: [f32; 4], fu: f32, fv: f32| [rect[0] + (rect[2] - rect[0]) * fu, rect[1] + (rect[3] - rect[1]) * fv];

    if let (Some(v), Some(rect)) = (flow, flow_tile) {
        if v[0] != 0.0 || v[2] != 0.0 {
            // atan2(z, x) - PI/2, s. FluidRenderer. Der Versatz um PI/2 bringt die Textur-
            // richtung mit Minecrafts Achsen zur Deckung.
            let winkel = (v[2] as f32).atan2(v[0] as f32) - std::f32::consts::FRAC_PI_2;
            let s = winkel.sin() * 0.25;
            let c = winkel.cos() * 0.25;
            return [
                punkt(rect, 0.5 + (-c - s), 0.5 + (-c + s)),
                punkt(rect, 0.5 + (-c + s), 0.5 + (c + s)),
                punkt(rect, 0.5 + (c + s), 0.5 + (c - s)),
                punkt(rect, 0.5 + (c - s), 0.5 + (-c - s)),
            ];
        }
    }
    // Ruhiges Wasser (oder keine Fliesstextur im Atlas): volle Kachel, Reihenfolge wie
    // Vanillas Still-Zweig.
    [punkt(still, 0.0, 0.0), punkt(still, 0.0, 1.0), punkt(still, 1.0, 1.0), punkt(still, 1.0, 0.0)]
}

// DIAGNOSE (nur aktiv mit gesetzter Umgebungsvariable MC_DEBUG_WATER_SIDES): faerbt jede
// Wasser-SEITENFLAECHE knallrot statt in der Biomfarbe. Damit laesst sich in EINEM Durchlauf
// unterscheiden, ob die Waende gar nicht auf dem Bildschirm ankommen (kein Rot) oder ob sie
// gezeichnet werden und nur nicht so aussehen wie erwartet (Rot sichtbar).
//
// Hintergrund: Mesher-Ausgabe, Atlas-UVs und Fliesstextur sind gegen die echte Installation
// nachgemessen und alle drei korrekt -- der Fehler muss also zwischen Vertexdaten und Bildschirm
// liegen, und das laesst sich nicht mehr aus dem Quelltext ablesen. Wieder rausnehmen, sobald
// die Frage beantwortet ist.
// UVs der vier Ecken einer Wasser-SEITENFLAECHE -- Nachbau von FluidRenderer (Yarn-1.21.11,
// Schleife ueber Type.HORIZONTAL):
//     u1 = sprite.getFrameU(0.0);          u2 = sprite.getFrameU(0.5);
//     v1 = sprite.getFrameV((1-h1)*0.5);   v2 = sprite.getFrameV((1-h2)*0.5);
//     v3 = sprite.getFrameV(0.5);
// Also nur die LINKE OBERE Vierteldeckung der Kachel: waagerecht die halbe Breite, senkrecht
// von (1-h)/2 bis 1/2. Eine volle Zelle (h=1) bekommt damit genau die obere Kachelhaelfte,
// eine halbhohe genau ein Viertel -- die Textur wird also NICHT auf die Wasserhoehe
// gestaucht, sondern beschnitten. Genau daran erkennt man in Vanilla die nach unten laufende
// Fliessgrafik an jeder Wasserkante.
//
// `h1`/`h2` sind die Hoehen der beiden oberen Ecken in der Reihenfolge, die push_fluid_quads
// erzeugt: Ecke 0 unten-links, 1 oben-links (h1), 2 oben-rechts (h2), 3 unten-rechts. Das ist
// Vanillas Vertex-Reihenfolge um eins gedreht (dort steht die obere Ecke zuerst) -- dieselbe
// Flaeche, nur ein anderer Startpunkt im Umlauf.
fn water_side_corner_uvs(rect: [f32; 4], h1: f32, h2: f32) -> [[f32; 2]; 4] {
    let punkt = |fu: f32, fv: f32| [rect[0] + (rect[2] - rect[0]) * fu, rect[1] + (rect[3] - rect[1]) * fv];
    let unten = 0.5;
    [punkt(0.0, unten), punkt(0.0, (1.0 - h1) * 0.5), punkt(0.5, (1.0 - h2) * 0.5), punkt(0.5, unten)]
}

fn push_quad_with_uv(out: &mut Vec<BlockVertex>, corners: [[f32; 3]; 4], color: [f32; 3], uv: [f32; 4]) {
    let [u0, v0, u1, v1] = uv;
    let corner_uv = [[u0, v1], [u0, v0], [u1, v0], [u1, v1]];
    let vert = |i: usize| BlockVertex { position: corners[i], color, uv: corner_uv[i] };
    out.extend_from_slice(&[vert(0), vert(1), vert(2), vert(0), vert(2), vert(3)]);
}

// Backface-Culling der Opak-Pipeline (s. render-Crate) steht und faellt mit einer
// EINHEITLICHEN Wicklungsrichtung. Ein einzelnes falsch herum ausgegebenes Quad waere im
// Spiel ein Loch in der Landschaft, das nur aus einer Richtung sichtbar ist -- die Art
// Fehler, die man beim Draufschauen fuer ein Chunk-Problem haelt. Deshalb hier nachgerechnet
// statt im Spiel gesucht.
// Die Fliesstextur ist der sichtbare Unterschied zwischen "Wasser steht" und "Wasser
// fliesst". Dreht sie sich falsch herum, sieht Stroemung aus, als liefe sie bergauf --
// deshalb hier gegen Vanillas Formel geprueft statt im Spiel geschaetzt.
// Welche Bloecke ins echte Alpha-Blending gehoeren, entscheidet is_glass -- eine falsche
// Antwort hier ist im Spiel sofort sichtbar (Glas wird deckend oder verschwindet ganz).
#[cfg(test)]
mod glass_tests {
    use super::*;

    // Vanillas BlockRenderLayers legt ALLE Glasarten in dieselbe TRANSLUCENT-Schicht --
    // inklusive KLAREM Glas und klaren Scheiben. Die naheliegende Annahme, nur gefaerbtes
    // Glas sei echt transparent, ist falsch.
    #[test]
    fn alle_glasarten_zaehlen_als_glas() {
        for name in [
            "glass",
            "glass_pane",
            "tinted_glass",
            "white_stained_glass",
            "black_stained_glass",
            "light_blue_stained_glass",
            "red_stained_glass_pane",
            "lime_stained_glass_pane",
        ] {
            assert!(is_glass(name), "{name} muss als Glas gelten");
        }
    }

    // Baut eine Spalte mit zwei Bloecken uebereinander bei (0,0,0) und (0,1,0).
    fn welt_mit_zwei(unten: u32, oben: u32) -> World {
        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = unten; // lokal (0,0,0) -> Welt-y=0
                states[16 * 16] = oben; // lokal (0,1,0) -> Welt-y=1
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        w
    }

    fn state_von(block: &str) -> u32 {
        (0..crate::block_registry::state_count() as u32)
            .find(|&s| block_name(s) == Some(block))
            .unwrap_or_else(|| panic!("{block}-State nicht gefunden"))
    }

    // Man darf nicht durch das Gras gucken koennen: unter einer Glasscheibe muss
    // die Oberseite des Blocks darunter sichtbar bleiben. Glas ist zwar `is_solid` (= nicht
    // Luft), verdeckt aber nichts -- vor dem Fix wurde die Flaeche weggeculled und man sah
    // durch den Grasblock hindurch ins Nichts.
    #[test]
    fn block_unter_glas_behaelt_seine_oberseite() {
        let stein = state_von("stone");
        let glas = state_von("glass");
        let mit_glas = welt_mit_zwei(stein, glas);
        let mit_luft = welt_mit_zwei(stein, 0);

        let flaechen = |w: &World| {
            let (opaque, _) = mesh_column(
                w,
                0,
                0,
                &BlockUvs::single_tile(),
                &BlockShapes::all_full_cubes(),
                &BiomePalette::fallback(),
                &CrossBlocks::none(),
                &BlockShapes::all_full_cubes(),
            );
            opaque.len()
        };
        assert_eq!(
            flaechen(&mit_glas),
            flaechen(&mit_luft),
            "der Stein unter Glas muss genauso viele Flaechen haben wie unter freiem Himmel -- Glas verdeckt nichts"
        );
    }

    // Auch Wasser wird von Glas nicht verdeckt -- der Weg dorthin ist in Vanilla aber ein
    // anderer als bei Bloecken, und die naheliegende Herleitung fuehrt in die Irre:
    // FluidRenderer.isSideCovered prueft die FORM, nicht die Deckkraft, und Glas sieht wie
    // ein Vollwuerfel aus. Die Form wird jedoch erst gar nicht gefuellt, wenn ein Block nicht
    // opak ist (AbstractBlock: `cullingShape = opaque ? ... : empty()`), und bei leerer Form
    // steigt isSideCovered sofort mit false aus.
    #[test]
    fn wasser_neben_glas_behaelt_seine_seitenflaeche() {
        let wasser = state_von("water");
        let glas = state_von("glass");

        // Wasser bei (0,0,0), Nachbar in +X bei (1,0,0).
        let welt_mit_nachbar = |nachbar: u32| {
            let mut w = World::new();
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
                if i == 4 {
                    states[0] = wasser;
                    states[1] = nachbar; // lokal (1,0,0)
                }
                sections.push(crate::chunk::ChunkSection::from_states(states));
            }
            w.insert_column(0, 0, sections);
            w
        };
        let wasserflaechen = |w: &World| {
            let (_, translucent) = mesh_column(
                w,
                0,
                0,
                &BlockUvs::single_tile(),
                &BlockShapes::all_full_cubes(),
                &BiomePalette::fallback(),
                &CrossBlocks::none(),
                &BlockShapes::all_full_cubes(),
            );
            translucent.len()
        };
        // ACHTUNG: Glas liegt seit dem Wechsel ins echte Alpha-Blending SELBST im
        // Translucent-Ergebnis -- ein blosser Vergleich der Gesamtzahl misst also Wasser UND
        // Glas. Deshalb wird der Eigenanteil des Glases abgezogen, statt eine Zahl zu raten:
        // was uebrigbleibt, muss exakt dem Wasser neben Luft entsprechen.
        let nur_glas = {
            let mut w = World::new();
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
                if i == 4 {
                    states[1] = glas;
                }
                sections.push(crate::chunk::ChunkSection::from_states(states));
            }
            w.insert_column(0, 0, sections);
            wasserflaechen(&w)
        };
        let wasser_neben_glas = wasserflaechen(&welt_mit_nachbar(glas)) - nur_glas;
        let wasser_neben_luft = wasserflaechen(&welt_mit_nachbar(0));
        let wasser_neben_stein = wasserflaechen(&welt_mit_nachbar(state_von("stone")));

        // Gleiche ANZAHL Flaechen wie neben Luft, aber 6 Vertices weniger: die Flaeche zum Glas
        // traegt das water_overlay und ist als einzige NICHT doppelseitig (s. push_rueckseiten
        // -- wie in Vanilla). Entscheidend fuer diesen Test
        // ist, dass die Flaeche ueberhaupt noch da ist: Glas hat keine Culling-Form.
        assert_eq!(
            wasser_neben_glas,
            wasser_neben_luft - 6,
            "Wasser neben Glas muss dieselbe Flaeche zeigen wie neben Luft (nur einseitig) -- Glas hat keine Culling-Form"
        );
        // Gegenprobe, damit der Test nicht bloss "nichts wird je geculled" bestaetigt:
        // gegen einen echten opaken Vollwuerfel faellt die gemeinsame Flaeche weiterhin weg.
        assert!(wasser_neben_stein < wasser_neben_luft, "gegen Stein muss die Wasserflaeche weiterhin weggeculled werden");
    }

    // Gegenprobe zum Fix: GLEICHES Glas an gleichem Glas culled die gemeinsame Flaeche sehr
    // wohl (Vanillas skipRendering) -- sonst saehe man in einer Glaswand jede innere Naht.
    #[test]
    fn glas_an_gleichem_glas_culled_die_naht() {
        let glas = state_von("glass");
        let (_, zwei_glas) = mesh_column(
            &welt_mit_zwei(glas, glas),
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        let (_, ein_glas) = mesh_column(
            &welt_mit_zwei(glas, 0),
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        // Zwei gestapelte Glasbloecke: 12 Flaechen minus die 2 an der gemeinsamen Naht = 10,
        // also weniger als das Doppelte eines einzelnen (6).
        assert!(
            zwei_glas.len() < ein_glas.len() * 2,
            "die Naht zwischen zwei gleichen Glasbloecken muss wegfallen (zwei: {}, einzeln: {})",
            zwei_glas.len(),
            ein_glas.len()
        );
        assert!(zwei_glas.len() > ein_glas.len(), "zwei Glasbloecke muessen trotzdem mehr Flaechen haben als einer");
    }

    // DER Punkt, an dem ich dreimal danebenlag: warum Wasser hinter Glas in Vanilla "nur
    // blau" ist. Nicht Culling, nicht Doppelseitigkeit -- eine EIGENE, flachere Textur
    // (water_overlay), die Vanilla nimmt, sobald der Nachbar Glas oder Laub ist.
    #[test]
    fn wasserseite_an_glas_und_laub_nutzt_die_overlay_textur() {
        const STILL: [f32; 4] = [0.0, 0.0, 0.25, 0.25];
        const OVERLAY: [f32; 4] = [0.5, 0.5, 0.75, 0.75];
        let uvs = BlockUvs::single_tile().with_water_overlay(Some(OVERLAY));

        // Die Auswahllogik selbst -- dieselbe Bedingung wie in push_fluid_quads.
        let waehle = |name: &str| if is_glass(name) || is_leaves(name) { uvs.water_overlay_rect().unwrap_or(STILL) } else { STILL };

        for name in ["glass", "white_stained_glass", "tinted_glass", "oak_leaves", "birch_leaves"] {
            assert_eq!(waehle(name), OVERLAY, "{name}: Wasser daneben muss die Overlay-Textur bekommen");
        }
        for name in ["stone", "dirt", "sand", "water"] {
            assert_eq!(waehle(name), STILL, "{name}: Wasser daneben muss die normale Textur behalten");
        }
    }

    // Fehlt water_overlay im Atlas (Resourcepack ohne die Datei), muss sauber die
    // Standtextur herauskommen statt eines Griffs in eine fremde Kachel.
    #[test]
    fn ohne_overlay_kachel_bleibt_die_standtextur() {
        const STILL: [f32; 4] = [0.0, 0.0, 0.25, 0.25];
        let uvs = BlockUvs::single_tile(); // kein with_water_overlay
        assert_eq!(uvs.water_overlay_rect().unwrap_or(STILL), STILL);
    }

    // Gegenprobe: nichts anderes darf hineinrutschen. Besonders Laub nicht -- dessen Alpha
    // wird beim Atlas-Bau bewusst auf voll gesetzt (s. assets::is_leaves), es gehoert in den
    // Opak-Pass. Und kein Block, der bloss "glass" im Namen traegt.
    #[test]
    fn andere_bloecke_sind_kein_glas() {
        for name in ["stone", "oak_leaves", "water", "glowstone", "glass_bottle", "ice", "sea_lantern"] {
            assert!(!is_glass(name), "{name} darf NICHT als Glas gelten");
        }
    }
}

#[cfg(test)]
mod water_uv_tests {
    use super::*;

    const STILL: [f32; 4] = [0.0, 0.0, 0.25, 0.25];
    const FLOW: [f32; 4] = [0.5, 0.5, 0.75, 0.75];

    fn in_kachel(uv: [f32; 2], rect: [f32; 4]) -> bool {
        uv[0] >= rect[0] - 1e-5 && uv[0] <= rect[2] + 1e-5 && uv[1] >= rect[1] - 1e-5 && uv[1] <= rect[3] + 1e-5
    }

    // Ohne Stroemung: Standtextur, volle Kachel, Reihenfolge wie Vanillas Still-Zweig.
    #[test]
    fn ruhiges_wasser_nutzt_die_standkachel() {
        let uv = water_top_corner_uvs(Some([0.0, 0.0, 0.0]), STILL, Some(FLOW));
        assert_eq!(uv, [[0.0, 0.0], [0.0, 0.25], [0.25, 0.25], [0.25, 0.0]]);
    }

    // Fehlt water_flow im Atlas (Resourcepack ohne die Datei), muss sauber die Standtextur
    // herauskommen -- nicht etwa ein Griff in eine fremde Atlas-Kachel.
    #[test]
    fn ohne_fliesskachel_bleibt_es_bei_der_standtextur() {
        let uv = water_top_corner_uvs(Some([1.0, 0.0, 0.0]), STILL, None);
        assert!(uv.iter().all(|&p| in_kachel(p, STILL)), "muss in der Standkachel bleiben, war {uv:?}");
    }

    // Bei Stroemung wird die FLIESSKACHEL benutzt, und alle vier Ecken muessen INNERHALB von
    // ihr liegen -- Vanillas +-0.25-Ausschnitt sorgt genau dafuer. Liefen sie heraus, zoege
    // das benachbarte Atlas-Kacheln ins Wasser (sichtbar als fremde Textur-Fetzen).
    #[test]
    fn stroemung_bleibt_innerhalb_der_fliesskachel() {
        // Rundum durch alle Richtungen, nicht nur eine -- der Ausschnitt muss bei JEDEM
        // Winkel passen.
        for schritt in 0..16 {
            let winkel = schritt as f64 * std::f64::consts::TAU / 16.0;
            let uv = water_top_corner_uvs(Some([winkel.cos(), 0.0, winkel.sin()]), STILL, Some(FLOW));
            for p in uv {
                assert!(in_kachel(p, FLOW), "Winkel {winkel}: UV {p:?} liegt ausserhalb der Fliesskachel {FLOW:?}");
            }
        }
    }

    // Entgegengesetzte Stroemungen muessen unterschiedliche UVs ergeben -- sonst waere die
    // Drehung wirkungslos und Wasser sae flussauf wie flussab gleich aus.
    #[test]
    fn gegenrichtungen_ergeben_verschiedene_uvs() {
        let nach_osten = water_top_corner_uvs(Some([1.0, 0.0, 0.0]), STILL, Some(FLOW));
        let nach_westen = water_top_corner_uvs(Some([-1.0, 0.0, 0.0]), STILL, Some(FLOW));
        assert_ne!(nach_osten, nach_westen, "die Drehung muss die Richtung wirklich abbilden");
    }

    // Eine Vierteldrehung der Stroemung muss die UVs zyklisch weiterschieben: die Ecken
    // rotieren gegeneinander, statt sich neu zu erfinden. Damit steht fest, dass wirklich
    // GEDREHT und nicht bloss verschoben wird.
    #[test]
    fn vierteldrehung_rotiert_die_ecken() {
        let a = water_top_corner_uvs(Some([1.0, 0.0, 0.0]), STILL, Some(FLOW));
        let b = water_top_corner_uvs(Some([0.0, 0.0, 1.0]), STILL, Some(FLOW));
        let gleich = |p: [f32; 2], q: [f32; 2]| (p[0] - q[0]).abs() < 1e-5 && (p[1] - q[1]).abs() < 1e-5;
        // Jede Ecke von b muss einer Ecke von a entsprechen (dieselbe Punktmenge, andere
        // Zuordnung) -- genau das Kennzeichen einer Drehung um die Kachelmitte.
        for q in b {
            assert!(a.iter().any(|&p| gleich(p, q)), "Ecke {q:?} kommt in der ungedrehten Menge {a:?} nicht vor");
        }
    }

    // Die SEITENFLAECHEN tragen die nach unten laufende Fliessgrafik -- Vanilla nimmt dort
    // immer sprite2 (water_flow), nie die Standtextur. Genau das hat gefehlt: "die yx- und
    // yz-Planes, also die 'Wasser fliesst nach unten'-Textur, wurden nicht angezeigt".
    //
    // Geprueft wird die exakte Vanilla-Formel: waagerecht getFrameU(0.0)..getFrameU(0.5) --
    // also die LINKE HAELFTE der Kachel --, senkrecht getFrameV((1-h)*0.5)..getFrameV(0.5).
    #[test]
    fn volle_seitenflaeche_nimmt_die_obere_linke_viertelkachel() {
        let uv = water_side_corner_uvs(FLOW, 1.0, 1.0);
        // Kachel 0.5..0.75: halbe Breite = 0.5..0.625, obere Haelfte = 0.5..0.625.
        assert_eq!(uv, [[0.5, 0.625], [0.5, 0.5], [0.625, 0.5], [0.625, 0.625]]);
    }

    // Eine halbhohe Wasserflaeche darf die Textur NICHT stauchen, sondern muss sie
    // beschneiden: die Unterkante bleibt bei 0.5, nur die Oberkante rutscht nach unten.
    // Waere es eine Stauchung, liefe die Fliessgrafik an einer Stufe schneller als daneben.
    #[test]
    fn halbe_hoehe_beschneidet_statt_zu_stauchen() {
        let voll = water_side_corner_uvs(FLOW, 1.0, 1.0);
        let halb = water_side_corner_uvs(FLOW, 0.5, 0.5);
        assert_eq!(halb[0], voll[0], "die Unterkante liegt bei jeder Hoehe auf derselben V-Koordinate");
        assert_eq!(halb[3], voll[3]);
        let hoehe = |q: [[f32; 2]; 4]| q[0][1] - q[1][1];
        assert!(
            (hoehe(halb) - hoehe(voll) / 2.0).abs() < 1e-6,
            "halbe Wasserhoehe muss genau den halben V-Ausschnitt nehmen: {halb:?} vs {voll:?}"
        );
    }

    // Ungleiche Eckhoehen (fliessendes Wasser hat vier verschiedene) muessen sich in
    // UNTERSCHIEDLICHEN V-Koordinaten der beiden oberen Ecken niederschlagen -- sonst laege
    // die Textur schief zur schraegen Oberkante.
    #[test]
    fn schraege_oberkante_ergibt_verschiedene_oberkanten_uvs() {
        let uv = water_side_corner_uvs(FLOW, 1.0, 0.5);
        assert_ne!(uv[1][1], uv[2][1], "die beiden oberen Ecken muessen verschiedene V-Werte haben");
    }
}

#[cfg(test)]
mod winding_tests {
    use super::*;

    // Flaechennormale aus den ersten drei Eckpunkten (Kreuzprodukt der beiden Kanten).
    // Zeigt bei Wicklung gegen den Uhrzeigersinn (von aussen gesehen) nach aussen.
    fn normale(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
        [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]
    }

    fn skalarprodukt(a: [f32; 3], b: [i32; 3]) -> f32 {
        a[0] * b[0] as f32 + a[1] * b[1] as f32 + a[2] * b[2] as f32
    }

    // Alle sechs Wuerfelseiten muessen nach AUSSEN winden -- und zwar BEIDE Dreiecke jedes
    // Quads (push_quad_with_uv gibt v0,v1,v2 + v0,v2,v3 aus; ein Fehler nur im zweiten
    // Dreieck ergaebe ein halb durchsichtiges Quad).
    #[test]
    fn wuerfelflaechen_winden_konsistent_nach_aussen() {
        for dir in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
            let mut out = Vec::new();
            push_face_quad(&mut out, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], dir, [1.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(out.len(), 6, "ein Quad sind zwei Dreiecke");
            for (i, ecken) in [[0, 1, 2], [3, 4, 5]].into_iter().enumerate() {
                let n = normale(out[ecken[0]].position, out[ecken[1]].position, out[ecken[2]].position);
                assert!(
                    skalarprodukt(n, dir) > 0.0,
                    "Seite {dir:?}, Dreieck {i}: Normale {n:?} zeigt nach innen -- diese Flaeche waere mit Culling unsichtbar"
                );
            }
        }
    }

    // Pflanzen sind schraege Ebenen ohne Aussenseite -- sie MUESSEN beidseitig ausgegeben
    // werden, sonst verschwinden sie mit Culling aus genau einer Blickrichtung.
    #[test]
    fn kreuzbloecke_sind_beidseitig() {
        let mut out = Vec::new();
        push_cross_quads(&mut out, ColumnOrigin { x: 0, z: 0 }, 0, 0, 0, [1.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        // Zwei Ebenen, jede doppelt, jedes Quad zwei Dreiecke = 4 * 6 Vertices.
        assert_eq!(out.len(), 24, "jede der beiden Ebenen muss in beiden Wicklungen vorliegen");

        // Zu jedem Dreieck muss es eines mit entgegengesetzter Normale geben.
        let normalen: Vec<[f32; 3]> =
            (0..out.len() / 3).map(|t| normale(out[3 * t].position, out[3 * t + 1].position, out[3 * t + 2].position)).collect();
        for n in &normalen {
            let gegenstueck = normalen.iter().any(|m| {
                let summe = [n[0] + m[0], n[1] + m[1], n[2] + m[2]];
                summe.iter().all(|c| c.abs() < 1e-3)
            });
            assert!(gegenstueck, "zur Normale {n:?} fehlt die Rueckseite -- die Pflanze waere von einer Seite unsichtbar");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_registry::{all_block_names, state_count};

    // Vertexzahl fuer `flaechen` sichtbare Wasserflaechen, davon `doppelseitig` viele mit
    // Rueckseite. Jede Flaeche sind 2 Dreiecke a 3 Vertices; doppelseitig sind die Oberseiten
    // (man schaut beim Schwimmen von unten dagegen) und alle Seitenflaechen ausser denen zu
    // Glas/Laub (s. push_rueckseiten) -- also praktisch: alles ausser Unterseiten und
    // Overlay-Seiten. Als Funktion statt als nackte Zahl, damit an jeder Zusicherung ablesbar
    // bleibt, WOHER die Zahl kommt -- und damit eine kuenftige Aenderung an der
    // Doppelseitigkeit genau einmal auffaellt statt in fuenf fremden Zahlen.
    fn wasser_vertices(flaechen: usize, doppelseitig: usize) -> usize {
        (flaechen + doppelseitig) * 6
    }

    fn state_of(block: &str, matches: impl Fn(u32) -> bool) -> u32 {
        (0..state_count() as u32).find(|&s| block_name(s) == Some(block) && matches(s)).expect("State nicht gefunden")
    }

    // Baut eine Spalte mit genau einem einzelnen Block bei Welt (0,0,0), sonst nur Luft --
    // isoliert genug, dass alle sechs Flaechen des Blocks an Luft grenzen (keine Kulissen).
    fn single_block_world(state: u32) -> World {
        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                // Section 4 beginnt bei MIN_Y + 4*16 = 0 -- lokal (0,0,0) ist also Welt-y=0.
                states[0] = state;
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        w
    }

    // Regressionstest: is_solid() sagt nur "kein
    // Luft", nicht "blockiert die Sicht" -- Wasser ist solid, aber transparent. Vor dem Fix
    // hat ein Stein neben Wasser seine gemeinsame Flaeche wie gegen einen echten Vollwuerfel
    // geculled (unsichtbar, solange Wasser selbst noch opak gezeichnet wurde) -- seit Wasser
    // echtes Alpha-Blending hat, waere das ein sichtbares Loch. Stein bei (0,0,0), Wasser bei
    // (1,0,0): Stein muss trotzdem alle sechs Seiten zeigen (auch die zum Wasser hin).
    #[test]
    fn stein_neben_wasser_behaelt_seine_flaeche() {
        let stone = state_of("stone", |_| true);
        let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = stone; // lokal (0,0,0)
                states[1] = water; // lokal (1,0,0)
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);

        let (opaque, translucent) = mesh_column(
            &w,
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        // 6 Seiten * 6 Vertices (2 Dreiecke) = 36 -- KEINE darf gegen das Wasser geculled sein.
        assert_eq!(opaque.len(), 36, "Stein muss trotz Wassernachbar alle sechs Seiten zeigen");
        // Wasser selbst culled weiterhin normal gegen den soliden Steinnachbarn: nur 5 der 6
        // Seiten (die zum Stein hin nicht).
        assert_eq!(translucent.len(), wasser_vertices(5, 4), "Wasser muss weiterhin gegen den soliden Nachbarn culln");
    }

    // Regressionstest fuer das sichtbare Kastennaht-Raster um jede Seegras-Zelle unter
    // Wasser: eine echte Wasserzelle direkt
    // neben Seegras deckte dessen duenne Ebenen-Form (neighbor_covers_face) nie voll ab und
    // zeichnete deshalb ihre eigene Grenzflaeche -- obwohl Seegras (always_wet, s.
    // world::fluid) genauso "nass" ist wie echtes Wasser und beide denselben Wasserkoerper
    // bilden sollten, ohne sichtbare interne Naht.
    #[test]
    fn wasser_neben_seegras_culled_die_gemeinsame_flaeche() {
        let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
        let seagrass = state_of("seagrass", |_| true);
        let z_plane = ShapeBox {
            min: [0.0, 0.0, 0.25],
            max: [1.0, 1.0, 0.25],
            faces: [false, false, true, true, false, false],
            content: [false; 6],
            uv: [None; 6],
        };
        let x_plane = ShapeBox {
            min: [0.75, 0.0, 0.0],
            max: [0.75, 1.0, 1.0],
            faces: [false, false, false, false, true, true],
            content: [false; 6],
            uv: [None; 6],
        };
        let mut per_state = vec![None; state_count()];
        per_state[seagrass as usize] = Some(vec![z_plane, x_plane]);
        let shapes = BlockShapes::new(per_state);

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = water; // lokal (0,0,0)
                states[1] = seagrass; // lokal (1,0,0)
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);

        let (_opaque, translucent) = mesh_column(
            &w,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        // translucent enthaelt sowohl Wasser (5 Seiten -- die zum Seegras hin geculled, sonst
        // ueberall Luft) als auch die Seegras-Wasserfuellung (5 Seiten -- die zum Wasser hin
        // von push_waterlogged_fill schon vorher korrekt geculled, sonst ueberall Luft):
        // 10 Flaechen, davon 8 doppelseitig (je Zelle Oberseite + 3 Seitenflaechen, s.
        // wasser_vertices). OHNE den Fix waere Wassers Ost-Seite (zum Seegras) NICHT geculled
        // (die duennen Ebenen decken die Grenze ja nie voll ab) -- eine Flaeche mehr, mit
        // einer sichtbaren Naht genau an dieser Grenze.
        assert_eq!(translucent.len(), wasser_vertices(10, 8), "Wasser darf keine sichtbare Naht gegen Seegras zeichnen");
    }

    // Ende-zu-Ende-Gegenstueck zu water_uv_tests: ein einzelner Wasserblock in Luft muss seine
    // vier SEITENFLAECHEN aus der Fliesskachel texturieren (und nur die -- Ober- und Unterseite
    // bleiben bei der Standkachel), und jede davon doppelseitig. Damit ist die ganze Kette
    // abgesichert, nicht bloss die UV-Formel: frueher fehlte die "Wasser fliesst nach unten"-Textur an den
    // Seiten komplett, auch von innen, wenn man im Wasser stand.
    #[test]
    fn wasser_seitenflaechen_kommen_aus_der_fliesskachel() {
        const FLOW: [f32; 4] = [0.5, 0.5, 0.75, 0.75];
        let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
        let uvs = BlockUvs::single_tile().with_water_flow(Some(FLOW));
        let world = single_block_world(water);
        let (_opaque, translucent) = mesh_column(
            &world,
            0,
            0,
            &uvs,
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );

        let aus_fliesskachel = |v: &BlockVertex| {
            v.uv[0] >= FLOW[0] - 1e-5 && v.uv[0] <= FLOW[2] + 1e-5 && v.uv[1] >= FLOW[1] - 1e-5 && v.uv[1] <= FLOW[3] + 1e-5
        };
        let seiten: Vec<&BlockVertex> = translucent.iter().filter(|v| aus_fliesskachel(v)).collect();
        // 4 Seiten * 6 Vertices, doppelseitig = 48. Die Standkachel liegt bei [0,0,1,1] und
        // umschliesst die Fliesskachel; Ober-/Unterseite nutzen davon aber nur die Eckpunkte
        // 0 und 1, fallen also nicht in den Bereich 0.5..0.75.
        assert_eq!(seiten.len(), 48, "vier doppelseitige Seitenflaechen aus der Fliesskachel erwartet");
        // Und sie muessen waagerecht wirklich nur die halbe Kachel nutzen (Vanillas
        // getFrameU(0.0)..getFrameU(0.5)) -- sonst waere die Grafik doppelt so breit gezogen.
        let max_u = seiten.iter().fold(f32::MIN, |a, v| a.max(v.uv[0]));
        assert!((max_u - 0.625).abs() < 1e-5, "die Seitenflaeche darf nur die halbe Kachelbreite nutzen, war bis {max_u}");
    }

    #[test]
    fn wasser_landet_im_translucent_pass() {
        let _ = all_block_names(); // Registry sicher geladen
        let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
        let world = single_block_world(water);
        let (opaque, translucent) = mesh_column(
            &world,
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert!(!translucent.is_empty(), "ein Wasserblock muss Vertices in den Translucent-Pass liefern");
        assert!(opaque.is_empty(), "ein reiner Wasserblock darf keine Opak-Vertices erzeugen");
    }

    // Regressionstest fuer das Loch um Seegras unter Wasser: Kelp ist ein Kreuz-Block (Modell "block/cross") OHNE
    // "waterlogged"-Property -- der fruehe `continue` im Kreuz-Zweig hat die Wasserfuellung
    // komplett uebersprungen, weil sie vorher nur im Box-Pfad haengte.
    #[test]
    fn kelp_hat_wasserfuellung_trotz_kreuz_pfad() {
        let kelp = state_of("kelp", |_| true);
        let cross = CrossBlocks::new({
            let mut v = vec![false; state_count()];
            v[kelp as usize] = true;
            v
        });
        let world = single_block_world(kelp);
        let (opaque, translucent) = mesh_column(
            &world,
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &cross,
            &BlockShapes::all_full_cubes(),
        );
        assert!(!opaque.is_empty(), "Kelp selbst muss weiterhin als Kreuz gezeichnet werden");
        assert!(!translucent.is_empty(), "Kelp muss trotz Kreuz-Pfad eine Wasserfuellung bekommen");
    }

    // Zeitversetzte Animationen (desync_animations): ein ebenes Seegrasfeld (alle Bloecke auf
    // derselben Hoehe) muss ueber alle Kopien streuen, und Nachbarn duerfen nicht gehaeuft
    // dieselbe nehmen.
    #[test]
    fn animierte_kacheln_streuen_ueber_ein_ebenes_feld() {
        let seagrass = state_of("seagrass", |_| true);
        let mut per_state = vec![[[0.0f32; 4]; 6]; state_count()];
        // Raster 4x4: Basis Kachel 1, Kopien 4..=10 -- jede Kopie in eigener Kachel.
        per_state[seagrass as usize] = [[0.25, 0.0, 0.5, 0.25]; 6];
        let uvs = BlockUvs::new(per_state, [[0.0; 4]; 6]).with_anim_variants(4, &[(1, (4..=10).collect())]);
        let plane = ShapeBox { min: [0.0, 0.0, 0.25], max: [1.0, 1.0, 0.25], faces: [false, false, true, true, false, false], content: [false; 6], uv: [None; 6] };
        let mut shapes = vec![None; state_count()];
        shapes[seagrass as usize] = Some(vec![plane]);
        let shapes = BlockShapes::new(shapes);

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[..256].fill(seagrass); // eine volle Lage bei y = 0
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        let (opaque, _) = mesh_column(&w, 0, 0, &uvs, &shapes, &BiomePalette::fallback(), &CrossBlocks::none(), &BlockShapes::all_full_cubes());

        // Kachel je Block ueber die Schwerpunkte seiner Dreiecke; alle eines Blocks muessen
        // dieselbe Kopie zeigen.
        let mut kachel = [[None::<usize>; 16]; 16];
        for tri in opaque.chunks_exact(3) {
            let mitte = |f: &dyn Fn(&BlockVertex) -> f32| tri.iter().map(f).sum::<f32>() / 3.0;
            let (u, v) = (mitte(&|v| v.uv[0]), mitte(&|v| v.uv[1]));
            let (x, z) = (mitte(&|v| v.position[0]).floor() as usize, mitte(&|v| v.position[2]).floor() as usize);
            let k = (v * 4.0) as usize * 4 + (u * 4.0) as usize;
            assert!(kachel[x][z].is_none_or(|alt| alt == k), "Block ({x}, {z}) mischt Kopien");
            kachel[x][z] = Some(k);
        }
        let kachel = kachel.map(|reihe| reihe.map(|k| k.expect("Block ohne Flaeche")));
        let alle: std::collections::BTreeSet<usize> = kachel.iter().flatten().copied().collect();
        assert_eq!(alle, std::iter::once(1).chain(4..=10).collect(), "jede Kopie muss vorkommen");
        let gleich = (0..15).flat_map(|x| (0..16).map(move |z| (x, z))).filter(|&(x, z)| kachel[x][z] == kachel[x + 1][z]).count();
        assert!(gleich < 240 / 4, "zu viele Nachbarn mit derselben Kopie: {gleich} von 240");
    }

    // Regressionstest fuer denselben Bug bei Seegras (Screenshot): sein Modell
    // "template_seagrass" nutzt NICHT rotierte Nullstaerke-Ebenen (zwei duenne, versetzte
    // Platten, KEINE an der Zellgrenze), ist also KEIN Kreuz-Block in dieser Klassifikation
    // (state_is_cross verlangt rotation.is_some() bei ALLEN Elementen) -- laeuft durch den
    // Box-Pfad, hat aber ebenfalls keine "waterlogged"-Property (existiert in Vanilla nie
    // trocken). Braucht wie der Zaun-Test echte (nicht randstaendige) Boxen statt
    // all_full_cubes(), sonst waere own_faces wieder faelschlich ueberall true.
    #[test]
    fn seegras_hat_wasserfuellung_ohne_waterlogged_property() {
        let seagrass = state_of("seagrass", |_| true);
        assert_eq!(crate::state_props::property(seagrass, "waterlogged"), None, "Seegras hat in Vanilla keine waterlogged-Property");

        let z_plane = ShapeBox {
            min: [0.0, 0.0, 0.25],
            max: [1.0, 1.0, 0.25],
            faces: [false, false, true, true, false, false],
            content: [false; 6],
            uv: [None; 6],
        };
        let x_plane = ShapeBox {
            min: [0.75, 0.0, 0.0],
            max: [0.75, 1.0, 1.0],
            faces: [false, false, false, false, true, true],
            content: [false; 6],
            uv: [None; 6],
        };
        let mut per_state = vec![None; state_count()];
        per_state[seagrass as usize] = Some(vec![z_plane, x_plane]);
        let shapes = BlockShapes::new(per_state);

        let world = single_block_world(seagrass);
        let (opaque, translucent) = mesh_column(
            &world,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert!(!opaque.is_empty(), "Seegras selbst muss weiterhin gezeichnet werden");
        assert!(!translucent.is_empty(), "Seegras muss trotz fehlender waterlogged-Property eine Wasserfuellung bekommen");
    }

    // Braucht eine ECHTE (duenne) Zaunpfosten-Box statt BlockShapes::all_full_cubes(): ein
    // Vollwuerfel-Fallback wuerde jede Richtung als "voll abgedeckt" zaehlen und die
    // Wasserfuellung faelschlich komplett unterdruecken (own_faces waere ueberall true) --
    // genau der Fall, den own_faces' Vollabdeckungs-Check (s. Aufrufer) unterscheiden soll.
    #[test]
    fn waterlogged_zaun_hat_wasserfuellung_und_bleibt_opak() {
        let fence = state_of("oak_fence", |s| {
            crate::state_props::property(s, "waterlogged") == Some("true")
                && crate::state_props::property(s, "north") == Some("false")
                && crate::state_props::property(s, "south") == Some("false")
                && crate::state_props::property(s, "east") == Some("false")
                && crate::state_props::property(s, "west") == Some("false")
        });
        let post = ShapeBox { min: [0.375, 0.0, 0.375], max: [0.625, 1.0, 0.625], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        let mut per_state = vec![None; state_count()];
        per_state[fence as usize] = Some(vec![post]);
        let shapes = BlockShapes::new(per_state);

        let world = single_block_world(fence);
        let (opaque, translucent) = mesh_column(
            &world,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert!(!opaque.is_empty(), "der Zaun selbst muss weiterhin opak gezeichnet werden");
        assert!(!translucent.is_empty(), "waterlogged muss eine Wasserfuellung in den Translucent-Pass liefern");
    }

    // Regressionstest: own_faces hat bisher nur
    // geprueft, ob IRGENDEINE Box die Flaeche voll abdeckt -- nicht, ob das AN DER
    // ZELLGRENZE passiert. Die untere Haelfte einer Stufe/Slab deklariert eine volle
    // "up"-Flaeche bei y=0.5 (ihre eigene, sichtbare Oberseite) -- das hat faelschlich die
    // Wasserfuellung an der eigentlichen Zellkante y=1 unterdrueckt, obwohl dort (0.5..1)
    // gar keine Blockgeometrie liegt und man dort auf die Wasseroberflaeche schauen muss
    // (z.B. eine waterlogged untere Stufe/Slab unter Wasser).
    #[test]
    fn waterlogged_untere_stufe_zeigt_wasser_darueber() {
        let slab = state_of("oak_slab", |s| {
            crate::state_props::property(s, "waterlogged") == Some("true") && crate::state_props::property(s, "type") == Some("bottom")
        });
        // Untere Haelfte: volle X/Z-Abdeckung, aber nur y 0..0.5 -- deklariert eine volle
        // "up"-Flaeche bei y=0.5 (NICHT an der Zellgrenze y=1).
        let bottom_half = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        let mut per_state = vec![None; state_count()];
        per_state[slab as usize] = Some(vec![bottom_half]);
        let shapes = BlockShapes::new(per_state);

        let world = single_block_world(slab);
        let (opaque, translucent) = mesh_column(
            &world,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert!(!opaque.is_empty(), "die Slab selbst muss weiterhin opak gezeichnet werden");
        // Die Wasserfuellung muss GENAU 5 Seiten zeigen: alle bis auf "unten" (dort deckt
        // die Slab selbst an der echten Zellgrenze y=0 voll ab, s. own_faces[DOWN]) -- vor
        // dem Fix waeren es nur 4 gewesen (auch "oben" faelschlich unterdrueckt).
        assert_eq!(translucent.len(), wasser_vertices(5, 5), "Wasserfuellung muss oben sichtbar sein, nur unten fehlen");
    }

    // Regressionstest: da wo das Wasser das Farmland beruehrt, flimmerte eine fremde
    // Textur. Farmland ist nur 15/16 hoch, deckt damit die
    // 8/9 hohe Wasseroberflaeche aber vollstaendig ab. Vanilla verwirft die Wasser-Seiten-
    // flaeche deshalb (FluidRenderer: Abdeckungstest gegen die WASSERHOEHE, s.
    // water_side_box-Kommentar). Vorher lief der Test gegen einen vollen Wuerfel, die Flaeche
    // wurde gezeichnet und lag koplanar auf der Erdseite des Farmlands -> Z-Fighting.
    //
    // Genau 5 Quads: oben, unten, und die drei Seiten an Luft -- die vierte (zum Farmland)
    // muss fehlen. Vor dem Fix waren es 6.
    #[test]
    fn wasser_neben_farmland_zeigt_keine_flaeche_zum_farmland() {
        let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
        let farmland = state_of("farmland", |_| true);

        // Farmlands echte Vanilla-Box: volle Grundflaeche, 15/16 hoch.
        let column = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 15.0 / 16.0, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        let mut per_state = vec![None; state_count()];
        per_state[farmland as usize] = Some(vec![column]);
        let shapes = BlockShapes::new(per_state);

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = water; // Welt (0,0,0)
                states[1] = farmland; // Welt (1,0,0), also oestlich daneben
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);

        let (_opaque, translucent) = mesh_column(
            &w,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert_eq!(
            translucent.len(),
            wasser_vertices(5, 4),
            "Wasser darf keine Flaeche zum (nur 15/16 hohen, aber voll abdeckenden) Farmland zeichnen"
        );
    }

    // Gegenprobe zum Test darueber: deckt der Nachbar die Wasserhoehe NICHT ab (halbhohe
    // Box, 0.5 < 8/9), muss die Seitenflaeche sehr wohl gezeichnet werden -- sonst haette
    // der Fix schlicht jede Seitenflaeche unterdrueckt statt nur die verdeckten.
    #[test]
    fn wasser_neben_halbhohem_block_zeigt_seine_flaeche() {
        let water = state_of("water", |s| crate::state_props::property(s, "level") == Some("0"));
        let slab = state_of("oak_slab", |s| {
            crate::state_props::property(s, "waterlogged") == Some("false") && crate::state_props::property(s, "type") == Some("bottom")
        });

        let bottom_half = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        let mut per_state = vec![None; state_count()];
        per_state[slab as usize] = Some(vec![bottom_half]);
        let shapes = BlockShapes::new(per_state);

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = water;
                states[1] = slab;
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);

        let (_opaque, translucent) = mesh_column(
            &w,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert_eq!(translucent.len(), wasser_vertices(6, 5), "ueber der halbhohen Stufe muss die Wasserflaeche sichtbar bleiben");
    }

    #[test]
    fn nicht_waterlogged_zaun_hat_keine_wasserfuellung() {
        let fence = state_of("oak_fence", |s| {
            crate::state_props::property(s, "waterlogged") == Some("false")
                && crate::state_props::property(s, "north") == Some("false")
                && crate::state_props::property(s, "south") == Some("false")
                && crate::state_props::property(s, "east") == Some("false")
                && crate::state_props::property(s, "west") == Some("false")
        });
        let world = single_block_world(fence);
        let (_opaque, translucent) = mesh_column(
            &world,
            0,
            0,
            &BlockUvs::single_tile(),
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert!(translucent.is_empty(), "ohne waterlogged darf keine Wasserfuellung entstehen");
    }

    // Regressionstest: Bloecke unter Kakteen fehlten im Mesh. Kaktus' Render-Modell hat ein volles 0..16-
    // Element mit cullface up/down (s. never_occludes_neighbors-Kommentar) -- das darf NUR
    // Kaktus' EIGENE Ober-/Unterseite gegen einen sturen Nachbarn culln, nicht umgekehrt
    // den Nachbarn gegen Kaktus. Sand bei (0,0,0), Kaktus direkt darueber bei (0,1,0), mit
    // der echten (an den Seiten eingerueckten) Kaktus-Form aus drei Boxen wie im echten
    // Der Fehler nach dem Bett-Renderer (man konnte durch den Boden gucken):
    // Betten haben KEIN Blockmodell, shapes_of liefert fuer sie den Vollwuerfel-Notnagel --
    // und mit dem hat der Boden unter dem Bett seine Oberseite verloren, waehrend das Bett
    // selbst nur eine flache Matratze zeichnet. Das Loch ging quer durch die Welt.
    #[test]
    fn boden_unter_einem_bett_behaelt_seine_oberseite() {
        let stein = state_of("stone", |_| true);
        let bett = state_of("red_bed", |_| true);
        let vollwuerfel = ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        // Genau die Lage aus dem echten Spiel: Stein hat ein Modell, das Bett nicht.
        let mut per_state = vec![None; state_count()];
        per_state[stein as usize] = Some(vec![vollwuerfel]);
        let shapes = BlockShapes::new(per_state);

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = stein; // (0,0,0)
                states[256] = bett; // (0,1,0)
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        let (opaque, _) = mesh_column(
            &w,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );

        // Die Oberseite des Steins liegt bei y = 1.0 (lokal), Sektion 4 beginnt bei y = 0.
        let basis_y = (crate::chunk::MIN_Y + 4 * 16) as f32;
        let oberseiten_quads = opaque.chunks_exact(6).filter(|q| q.iter().all(|v| (v.position[1] - (basis_y + 1.0)).abs() < 0.01)).count();
        assert!(oberseiten_quads > 0, "der Boden unter dem Bett muss seine Oberseite behalten");
    }

    // cactus.json (aus dem 1.21.11-Jar).
    #[test]
    fn sand_unter_kaktus_behaelt_seine_oberseite() {
        let sand = state_of("sand", |_| true);
        let cactus = state_of("cactus", |_| true);

        let full_updown = ShapeBox {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
            faces: [true, true, false, false, false, false],
            content: [false; 6],
            uv: [None; 6],
        };
        let north_south_wall = ShapeBox {
            min: [0.0, 0.0, 1.0 / 16.0],
            max: [1.0, 1.0, 15.0 / 16.0],
            faces: [false, false, true, true, false, false],
            content: [false; 6],
            uv: [None; 6],
        };
        let west_east_wall = ShapeBox {
            min: [1.0 / 16.0, 0.0, 0.0],
            max: [15.0 / 16.0, 1.0, 1.0],
            faces: [false, false, false, false, true, true],
            content: [false; 6],
            uv: [None; 6],
        };
        let mut per_state = vec![None; state_count()];
        per_state[cactus as usize] = Some(vec![full_updown, north_south_wall, west_east_wall]);
        // Sand braucht seinen (trivialen) Vollwuerfel-Eintrag: seit BlockShapes zwischen
        // "Modell sagt Vollwuerfel" und "gar kein Modell" unterscheidet (s.
        // has_resolved_shape), verdeckt ein EINTRAGSLOSER Nachbar nichts mehr -- und Sand hat
        // in Wirklichkeit sehr wohl ein Modell.
        per_state[sand as usize] =
            Some(vec![ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
        let shapes = BlockShapes::new(per_state);

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = sand; // lokal (0,0,0)
                states[256] = cactus; // lokal (0,1,0) -- (1*16+0)*16+0
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);

        let (opaque, _translucent) = mesh_column(
            &w,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        // Sand: alle 6 Seiten (36 Vertices) -- Kaktus deckt seine Oberseite trotz vollem
        // Cullface-Element NICHT ab. Kaktus: 5 der 6 Seiten (30 Vertices) -- seine EIGENE
        // Unterseite bleibt weiterhin gegen den sturen Sand-Nachbarn geculled (unveraendert
        // richtig, betrifft die andere Blickrichtung). Macht 66 zusammen -- ohne den Fix
        // waeren es nur 60 (Sand nur 5 Seiten, seine Oberseite faelschlich unsichtbar).
        assert_eq!(opaque.len(), 66, "Sand unter Kaktus muss seine Oberseite behalten");
    }
}

// Waterlogged-Bloecke (Stufe, Zaun, Seegras, ...) bekommen ihre Wasserfuellung ueber
// push_waterlogged_fill -- einen ANDEREN Pfad als echte Wasserbloecke. Genau dort war die
// Oberflaeche laengst richtig (Vanillas Eck-Hoehen), die WAENDE aber noch bei der flachen
// Vollbox: sie reichten bis zur Zellkante, waehrend das Wasser darunter endete.
#[cfg(test)]
mod waterlogged_wand_tests {
    use super::*;

    fn state_von(block: &str, passt: impl Fn(u32) -> bool) -> u32 {
        (0..crate::block_registry::state_count() as u32)
            .find(|&s| block_name(s) == Some(block) && passt(s))
            .unwrap_or_else(|| panic!("{block}-State nicht gefunden"))
    }

    // Zwei nebeneinanderliegende waterlogged Stufen auf Gras -- die typische Lage am
    // Ufer. Nebeneinander ist wichtig: bei einer EINZELNEN Stufe sind alle vier
    // Eckhoehen gleich, ein zu hoher Rand faellt dort weit weniger auf.
    fn zwei_waterlogged_stufen() -> (World, BlockShapes) {
        let slab = state_von("oak_slab", |s| {
            crate::state_props::property(s, "waterlogged") == Some("true") && crate::state_props::property(s, "type") == Some("bottom")
        });
        let gras = state_von("grass_block", |_| true);
        let untere_haelfte = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        let mut per_state = vec![None; crate::block_registry::state_count()];
        per_state[slab as usize] = Some(vec![untere_haelfte]);

        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                states[0] = gras;
                states[1] = gras;
                states[16 * 16] = slab;
                states[16 * 16 + 1] = slab;
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        (w, BlockShapes::new(per_state))
    }

    // DER Kern: keine Wasserwand darf hoeher enden als die hoechste Stelle der
    // Wasseroberflaeche. Vorher liefen die Waende bis zur Zellkante (y+1) und standen damit
    // rund einen Viertelblock ueber dem Wasser ins Leere.
    #[test]
    fn waende_ragen_nicht_ueber_die_wasseroberflaeche() {
        let (welt, shapes) = zwei_waterlogged_stufen();
        let (_opaque, translucent) = mesh_column(
            &welt,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        assert!(!translucent.is_empty(), "die Stufen muessen ueberhaupt eine Wasserfuellung liefern");

        let hoechste_flaeche = translucent.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        // Die Zellkante liegt bei y=2 (Stufe steht auf y=1). Wasser darf spuerbar darunter
        // bleiben -- sonst ist wieder die flache Vollbox am Werk.
        assert!(hoechste_flaeche < 1.95, "kein Wasser-Vertex darf bis an die Zellkante y=2 reichen, hoechster war {hoechste_flaeche}");
    }

    // Gegenprobe, damit der Test oben nicht bloss "irgendwas ist niedriger" bestaetigt: die
    // Waende muessen GENAU bis zur Oberflaeche reichen, nicht darunter aufhoeren (sonst
    // klaffte zwischen Wand und Wasserspiegel eine Luecke).
    #[test]
    fn waende_reichen_bis_an_die_wasseroberflaeche() {
        let (welt, shapes) = zwei_waterlogged_stufen();
        let (_opaque, translucent) = mesh_column(
            &welt,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        // Waagerechte Quads (alle vier Ecken nahezu gleich hoch) sind die Oberflaeche, der
        // Rest sind Waende. Beide muessen oben auf derselben Hoehe enden.
        let mut oben_flaeche = f32::MIN;
        let mut oben_wand = f32::MIN;
        for q in translucent.chunks_exact(6) {
            let ymin = q.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let ymax = q.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            if ymin > 1.5 {
                oben_flaeche = oben_flaeche.max(ymax); // liegt ganz oben -> Oberflaeche
            } else {
                oben_wand = oben_wand.max(ymax);
            }
        }
        // Toleranz etwas ueber EPS (0.001): die Oberflaeche wird bewusst um genau diesen
        // Betrag abgesenkt, damit sie nicht koplanar auf der Wandoberkante liegt und
        // z-fightet. Ein groesserer Abstand waere dagegen eine echte Luecke.
        assert!(
            (oben_flaeche - oben_wand).abs() < 0.005,
            "Wand endet bei {oben_wand}, Oberflaeche bei {oben_flaeche} -- das ist mehr als der EPS-Versatz"
        );
    }

    // Die Wasserfuellung muss GENAUSO verdecken wie echtes Wasser: nach der tatsaechlichen
    // Abdeckung, nicht nach "Nachbar ist nicht Luft". Vorher stand in push_waterlogged_fill
    // nur `is_solid(neighbor)` -- damit verschluckte JEDER beliebige Nachbar die Wasserwand,
    // auch eine halbhohe Stufe, ein Zaun oder eine Fackel. Genau daran fehlte die
    // Fliessflaeche an waterlogged Stufen.
    // Vanillas Riss-Ueberlagerung projiziert die Textur in Blockkoordinaten auf die Flaeche
    // (SheetedDecalTextureGenerator), sie wird NICHT ueber die Flaeche gestreckt. Auf einem
    // vollen Wuerfel deckt deshalb jede Seite genau eine Kachel ab -- und eine untere Stufe
    // zeigt an ihren Seiten nur die UNTERE Haelfte des Rissbilds (in der Textur ist das die
    // untere Haelfte, also v von 0,5 bis 1,0).
    #[test]
    fn riss_uv_folgt_vanillas_projektion() {
        let stein = state_von("stone", |_| true);
        let mut per_state = vec![None; crate::block_registry::state_count()];
        per_state[stein as usize] = Some(vec![ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
        let shapes = BlockShapes::new(per_state);
        let luft = |_: i32, _: i32, _: i32| STATE_AIR;

        let voll = block_breaking_vertices([0.0; 3], (0, 0, 0), stein, [0.0, 0.0, 1.0, 1.0], &shapes, &luft);
        assert_eq!(voll.len(), 36, "sechs Seiten mit je zwei Dreiecken");
        for v in &voll {
            assert!((0.0..=1.0).contains(&v.uv[0]) && (0.0..=1.0).contains(&v.uv[1]), "UV bleibt in der Kachel: {:?}", v.uv);
        }
        // Jede Seite spannt die Kachel voll auf.
        let (umin, umax) = voll.iter().fold((1.0f32, 0.0f32), |(a, b), v| (a.min(v.uv[0]), b.max(v.uv[0])));
        assert!((umin - 0.0).abs() < 1e-6 && (umax - 1.0).abs() < 1e-6);

        let mut per_state = vec![None; crate::block_registry::state_count()];
        per_state[stein as usize] =
            Some(vec![ShapeBox { min: [0.0; 3], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
        let halb = BlockShapes::new(per_state);
        let stufe = block_breaking_vertices([0.0; 3], (0, 0, 0), stein, [0.0, 0.0, 1.0, 1.0], &halb, &luft);
        // Nordseite: das eine Sechsergespann, dessen Ecken ALLE auf z = 0 liegen (die
        // Deckel-/Bodenflaechen beruehren z = 0 nur mit zwei Ecken). Ihr v kommt aus der Hoehe.
        let nord: Vec<[f32; 2]> = stufe
            .chunks_exact(6)
            .find(|q| q.iter().all(|v| v.position[2] == 0.0))
            .expect("Nordflaeche vorhanden")
            .iter()
            .map(|v| v.uv)
            .collect();
        let (vmin, vmax) = nord.iter().fold((1.0f32, 0.0f32), |(a, b), uv| (a.min(uv[1]), b.max(uv[1])));
        assert!((vmin - 0.5).abs() < 1e-6 && (vmax - 1.0).abs() < 1e-6, "untere Blockhaelfte -> untere Texturhaelfte, war {vmin}..{vmax}");
    }

    #[test]
    fn fuellung_culled_nach_abdeckung_nicht_nach_luft() {
        let waterlogged = state_von("oak_slab", |s| {
            crate::state_props::property(s, "waterlogged") == Some("true") && crate::state_props::property(s, "type") == Some("bottom")
        });
        let trocken = state_von("oak_slab", |s| {
            crate::state_props::property(s, "waterlogged") == Some("false") && crate::state_props::property(s, "type") == Some("bottom")
        });
        let stein = state_von("stone", |_| true);
        let untere_haelfte = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 0.5, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };
        let mut per_state = vec![None; crate::block_registry::state_count()];
        per_state[waterlogged as usize] = Some(vec![untere_haelfte]);
        per_state[trocken as usize] = Some(vec![untere_haelfte]);
        // Stein steht hier fuer "voller Wuerfel deckt ab" -- der Eintrag muss also da sein,
        // seit ein eintragsloser Nachbar nichts mehr verdeckt (s. has_resolved_shape).
        per_state[stein as usize] =
            Some(vec![ShapeBox { min: [0.0; 3], max: [1.0; 3], faces: [true; 6], content: [false; 6], uv: [None; 6] }]);
        let shapes = BlockShapes::new(per_state);

        // Zaehlt die Quads der Ost-Wand (x nahe der Zellgrenze 1.0) der Fuellung bei (0,0,0).
        let ostwand = |nachbar: u32| {
            let mut w = World::new();
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
                if i == 4 {
                    states[0] = waterlogged;
                    states[1] = nachbar;
                }
                sections.push(crate::chunk::ChunkSection::from_states(states));
            }
            w.insert_column(0, 0, sections);
            let (_o, tr) = mesh_column(
                &w,
                0,
                0,
                &BlockUvs::single_tile(),
                &shapes,
                &BiomePalette::fallback(),
                &CrossBlocks::none(),
                &BlockShapes::all_full_cubes(),
            );
            tr.chunks_exact(6).filter(|q| q.iter().all(|v| (v.position[0] - 1.0).abs() < 0.01)).count()
        };

        assert!(ostwand(0) > 0, "an Luft muss die Wand da sein");
        assert_eq!(ostwand(trocken), ostwand(0), "eine halbhohe Nachbarstufe deckt die Wasserhoehe nicht ab -- die Wand muss bleiben");
        assert_eq!(ostwand(stein), 0, "ein voller Wuerfel deckt die Flaeche ab -- die Wand muss weg");
    }

    // Keine Wasserwand darf EXAKT auf einer Zellgrenze liegen. Genau dort liegt naemlich die
    // opake Seitenflaeche der Stufe selbst, und zwei deckungsgleiche Flaechen haben
    // bit-identische interpolierte Tiefe -- der Translucent-Pass testet mit LESS (s.
    // render::block_renderer), die Wasserwand faellt dann durch den Tiefentest und ist
    // unsichtbar. Echtes Wasser hat diesen Versatz seit laengerem, die waterlogged-Fuellung
    // war der letzte Pfad ohne ihn (die Geometrie war da, sie kam nur nie durch die
    // Tiefenpruefung).
    #[test]
    fn waende_liegen_nicht_auf_der_zellgrenze() {
        let (welt, shapes) = zwei_waterlogged_stufen();
        let (_opaque, translucent) = mesh_column(
            &welt,
            0,
            0,
            &BlockUvs::single_tile(),
            &shapes,
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        );
        // Waende erkennt man daran, dass sie in x ODER z keine Ausdehnung haben.
        for q in translucent.chunks_exact(6) {
            let spanne = |achse: usize| {
                let min = q.iter().map(|v| v.position[achse]).fold(f32::MAX, f32::min);
                let max = q.iter().map(|v| v.position[achse]).fold(f32::MIN, f32::max);
                (min, max)
            };
            for achse in [0, 2] {
                let (min, max) = spanne(achse);
                if (max - min).abs() > 1e-6 {
                    continue; // in dieser Achse ausgedehnt -> keine Wand senkrecht dazu
                }
                // Die beiden Stufen liegen bei x=0..2, z=0..1 -- ganzzahlige Ebenen sind hier
                // die Zellgrenzen. Die einzige erlaubte Ausnahme waere eine Wand zwischen zwei
                // Zellen, die es hier gar nicht gibt (beide Stufen grenzen aneinander und
                // cullen die gemeinsame Flaeche weg).
                let abstand_zur_ganzzahl = (min - min.round()).abs();
                assert!(
                    abstand_zur_ganzzahl > 1e-4,
                    "Wand liegt bei Achse {achse} exakt auf der Zellgrenze {min} -- ohne Versatz verliert sie den Tiefentest"
                );
            }
        }
    }
}

#[cfg(test)]
mod grosse_koordinaten_tests {
    use super::*;

    // Der Kern der ganzen Umstellung: an einer echten DonutSMP-Position muss ein Blockeckpunkt
    // noch exakt getroffen werden. Frueher standen hier absolute Weltkoordinaten in f32 --
    // bei x = -186634 ist deren Aufloesung 0.015625, jeder Eckpunkt rastete also schon beim
    // Meshen auf ein 1/64-Gitter (sichtbar als zitternde Geometrie).
    #[test]
    fn spalten_lokale_position_bleibt_bei_riesigen_koordinaten_exakt() {
        // Spalte, in der x = -186634 liegt: floor(-186634 / 16) = -11665.
        let origin = ColumnOrigin::of_column(-11665, 9624);
        assert_eq!(origin.x, -186640);
        assert_eq!(origin.z, 153984);

        // Ein Block irgendwo in dieser Spalte, plus ein feiner Modell-Versatz (z.B. die
        // Kante einer Treppenstufe bei 1/16 Block).
        let (lx, ly, lz) = origin.local(-186634, 75, 153997);
        assert_eq!((lx, ly, lz), (6.0, 75.0, 13.0), "lokale Koordinaten muessen klein und exakt sein");

        // Die f32-Aufloesung bei x = -186634 betraegt 0.015625. Alles Feinere geht dort
        // verloren -- zum Beispiel das EPS von 0.001, mit dem push_fluid_quads die
        // Wasseroberflaeche gegen die Blockkante absetzt (s. dortige Konstante).
        const EPS: f32 = 0.001;
        let lokal_mit_eps = lx + EPS;
        assert_ne!(lokal_mit_eps, lx, "lokal muss das EPS erhalten bleiben");
        assert!((lokal_mit_eps - 6.001).abs() < 1e-6, "lokal exakt: {lokal_mit_eps}");

        // Derselbe Punkt in absoluten f32-Weltkoordinaten: das EPS verschwindet spurlos --
        // genau das war der Bug (Wasserflaechen z-fighteten mit ihrer eigenen Blockkante).
        let absolut = -186634.0f32;
        assert_eq!(absolut + EPS, absolut, "absolut in f32 faellt das EPS unter den Tisch");

        // Und die Aufloesung selbst, als Beleg fuer die Groessenordnung.
        let naechster = f32::from_bits(absolut.abs().to_bits() + 1) - absolut.abs();
        assert!((naechster - 0.015625).abs() < 1e-9, "f32-Aufloesung dort: {naechster}");
    }

    // Der Versatz, den der Renderer je Spalte hochlaedt, wird in f64 gerechnet und erst danach
    // auf f32 verkuerzt (s. BlockRenderer::update_column_offsets). Hier nachgestellt: das
    // Ergebnis muss die Spalte auf den Block genau platzieren.
    #[test]
    fn augenrelativer_versatz_bleibt_genau() {
        let origin = ColumnOrigin::of_column(-11665, 9624);
        let auge = (-186634.2972f64, 75.8339, 153997.0023);

        let versatz_x = (origin.x as f64 - auge.0) as f32;
        let versatz_z = (origin.z as f64 - auge.2) as f32;

        // Lokale Position des Blocks + Versatz = Blockposition relativ zum Auge.
        let (lx, _, lz) = origin.local(-186634, 75, 153997);
        let rel_x = lx + versatz_x;
        let rel_z = lz + versatz_z;

        // Erwartung direkt in f64 gerechnet.
        let soll_x = (-186634.0f64 - auge.0) as f32;
        let soll_z = (153997.0f64 - auge.2) as f32;

        assert!((rel_x - soll_x).abs() < 1e-4, "x: {rel_x} statt {soll_x}");
        assert!((rel_z - soll_z).abs() < 1e-4, "z: {rel_z} statt {soll_z}");
    }
}

#[cfg(test)]
mod coplanar_conflict_tests {
    use super::*;

    fn box_at(min: [f32; 3], max: [f32; 3]) -> ShapeBox {
        ShapeBox { min, max, faces: [true; 6], content: [false; 6], uv: [None; 6] }
    }

    // Nachgestellt: Mauer-Pfosten (4..12 in X/Z, Y 0..16) + ein "tall" verbundener Arm
    // (X 5..11, Z 0..8, Y 0..16 -- reicht ueber die Blockmitte hinaus in den Pfosten hinein,
    // s. template_wall_post.json/template_wall_side_tall.json). Beide haben ihre Oben-
    // Flaeche exakt bei y=1 UND ueberlappen sich im Grundriss -- genau der Z-Fighting-Fall.
    #[test]
    fn pfosten_und_verbundener_arm_haben_einen_konflikt() {
        let pfosten = box_at([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]);
        let arm = box_at([0.3125, 0.0, 0.0], [0.6875, 1.0, 0.5]);
        let shapes = [pfosten, arm];
        // Der Pfosten (Index 0) hat keine FRUEHEREN Boxen zum Konfligieren -- er gewinnt.
        assert_eq!(coplanar_conflict_count(&shapes, 0, FACE_UP), 0);
        // Der Arm (Index 1) konfligiert mit dem Pfosten (Index 0) -- genau einmal.
        assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_UP), 1);
        // Dieselbe Logik gilt fuer die Untenflaeche (beide Boxen beginnen bei y=0).
        assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_DOWN), 1);
    }

    // Zwei Boxen, die sich NICHT im Grundriss ueberlappen (z.B. Pfosten + Arm nach Norden UND
    // Arm nach Sueden, die beide nur bis zur Mitte reichen und sich in Z nicht beruehren),
    // duerfen keinen Konflikt melden.
    #[test]
    fn nicht_ueberlappende_boxen_haben_keinen_konflikt() {
        let a = box_at([0.3125, 0.0, 0.0], [0.6875, 1.0, 0.5]);
        let b = box_at([0.3125, 0.0, 0.5], [0.6875, 1.0, 1.0]);
        // Dieselbe X-Spanne, direkt aneinandergrenzende (nicht ueberlappende) Z-Spannen.
        let shapes = [a, b];
        assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_UP), 0);
    }

    // Verschiedene Hoehen (z.B. ein "low" statt "tall" verbundener Arm, Oben-Flaeche bei
    // y=14/16 statt y=16/16) duerfen ebenfalls keinen Konflikt melden -- der Arm liegt dann
    // vollstaendig UNTER der Pfostenflaeche und ist ohnehin unsichtbar, kein Z-Fighting.
    #[test]
    fn unterschiedliche_hoehen_haben_keinen_konflikt() {
        let pfosten = box_at([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]);
        let niedriger_arm = box_at([0.3125, 0.0, 0.0], [0.6875, 14.0 / 16.0, 0.5]);
        let shapes = [pfosten, niedriger_arm];
        assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_UP), 0);
    }
}

// Lava geht seit dem Fluid-Umbau durch EXAKT denselben Zeichenpfad wie Wasser
// (push_fluid_quads) -- diese Tests halten die drei Stellen fest, an denen Vanillas
// FluidRenderer sie dann doch unterscheidet, plus den eigentlichen Fehler:
// die Lava war einen ganzen Block hoch.
#[cfg(test)]
mod lava_tests {
    use super::*;
    use crate::block_registry::state_count;

    const FLOW: [f32; 4] = [0.5, 0.5, 0.75, 0.75];
    const OVERLAY: [f32; 4] = [0.25, 0.25, 0.5, 0.5];

    fn state_von(block: &str) -> u32 {
        (0..state_count() as u32)
            .find(|&s| block_name(s) == Some(block) && crate::state_props::property(s, "level").is_none_or(|l| l == "0"))
            .expect("State nicht gefunden")
    }

    // Eine Spalte mit einem Block bei Welt (0,0,0) und optional einem Nachbarn bei (1,0,0).
    fn welt(block: u32, nachbar: Option<u32>) -> World {
        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![0u32; crate::chunk::SECTION_VOLUME];
            if i == 4 {
                // Section 4 beginnt bei MIN_Y + 4*16 = 0 -- lokal (0,0,0) ist Welt-y=0.
                states[0] = block;
                if let Some(n) = nachbar {
                    states[1] = n; // lokal (1,0,0)
                }
            }
            sections.push(crate::chunk::ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        w
    }

    fn mesh(w: &World, uvs: &BlockUvs) -> (Vec<BlockVertex>, Vec<BlockVertex>) {
        mesh_column(
            w,
            0,
            0,
            uvs,
            &BlockShapes::all_full_cubes(),
            &BiomePalette::fallback(),
            &CrossBlocks::none(),
            &BlockShapes::all_full_cubes(),
        )
    }

    // DER Kern. Eine freistehende Lavaquelle darf NICHT bis zur Zellkante reichen:
    // Vanillas Quellhoehe ist 8/9, und weil die vier Ecken gegen die Luftnachbarn gemittelt
    // werden (FluidRenderer.calculateFluidHeight, Gewicht 10 fuer die Quelle gegen je 1 fuer
    // die beiden Luftseiten), sackt sie auf (10*8/9)/12 ab -- exakt dieselbe Zahl, die
    // fluid::tests::isolierte_quelle_sackt_an_den_ecken_ab fuer Wasser festhaelt.
    #[test]
    fn lavaquelle_reicht_nicht_bis_zur_zellkante() {
        let lava = state_von("lava");
        let (opaque, _) = mesh(&welt(lava, None), &BlockUvs::single_tile());
        assert!(!opaque.is_empty(), "eine Lavaquelle muss Geometrie liefern");

        let min_y = opaque.iter().fold(f32::MAX, |a, v| a.min(v.position[1]));
        let max_y = opaque.iter().fold(f32::MIN, |a, v| a.max(v.position[1]));
        // Unterseite liegt um EPS hoch, Oberseite um EPS tief (beides Vanillas 0.001, s.
        // push_fluid_quads) -- die sichtbare Spanne ist also Eckhoehe minus 2*EPS.
        let erwartet = crate::fluid::calculate_corner_height(8.0 / 9.0, 0.0, 0.0, 0.0) - 0.002;
        assert!(
            (max_y - min_y - erwartet).abs() < 1e-4,
            "Lava-Spanne war {}, erwartet {erwartet} (vorher stand sie als Vollwuerfel bei 1.0)",
            max_y - min_y
        );
        assert!(max_y - min_y < 0.9, "Lava darf keinesfalls einen ganzen Block hoch sein");
    }

    // Vanilla legt nur FluidTags.WATER in die durchsichtige Schicht (RenderLayers.
    // getFluidLayer) -- Lava ist blickdicht und gehoert in den Opak-Pass. Gegenstueck zu
    // wasser_landet_im_translucent_pass.
    #[test]
    fn lava_landet_im_opaken_pass() {
        let lava = state_von("lava");
        let (opaque, translucent) = mesh(&welt(lava, None), &BlockUvs::single_tile());
        assert!(!opaque.is_empty(), "ein Lavablock muss Opak-Vertices liefern");
        assert!(translucent.is_empty(), "Lava darf nichts in den Translucent-Pass legen");
    }

    // Die Seitenflaechen tragen lava_flow, nicht lava_still -- dieselbe Halbkachel-UV wie
    // bei Wasser (Vanillas getFrameU(0.0)..getFrameU(0.5)).
    #[test]
    fn lava_seitenflaechen_kommen_aus_der_fliesskachel() {
        let lava = state_von("lava");
        let uvs = BlockUvs::single_tile().with_lava_flow(Some(FLOW));
        let (opaque, _) = mesh(&welt(lava, None), &uvs);

        let aus_fliesskachel = |v: &BlockVertex| {
            v.uv[0] >= FLOW[0] - 1e-5 && v.uv[0] <= FLOW[2] + 1e-5 && v.uv[1] >= FLOW[1] - 1e-5 && v.uv[1] <= FLOW[3] + 1e-5
        };
        // 4 Seiten * 6 Vertices, doppelseitig (s. push_rueckseiten) = 48. Die Standkachel
        // liegt bei [0,0,1,1] und umschliesst die Fliesskachel; Ober-/Unterseite nutzen davon
        // aber nur die Eckpunkte 0 und 1, fallen also nicht in den Bereich 0.5..0.75.
        assert_eq!(
            opaque.iter().filter(|v| aus_fliesskachel(v)).count(),
            48,
            "vier doppelseitige Lava-Seitenflaechen aus der Fliesskachel erwartet"
        );
    }

    // Der water_overlay-Zweig haengt in Vanilla an `if (!bl)` -- Lava an Glas/Laub bekommt
    // ihr ganz normales Fliess-Sprite. Ohne die kind-Abfrage in push_fluid_quads::seite
    // saehe man an jeder Glasscheibe neben Lava eine WASSER-Textur.
    #[test]
    fn lava_neben_glas_nimmt_kein_wasser_overlay() {
        let lava = state_von("lava");
        let glas = state_von("glass");
        let uvs = BlockUvs::single_tile().with_lava_flow(Some(FLOW)).with_water_overlay(Some(OVERLAY));
        let (opaque, _) = mesh(&welt(lava, Some(glas)), &uvs);

        let in_kachel = |v: &BlockVertex, r: [f32; 4]| {
            v.uv[0] >= r[0] - 1e-5 && v.uv[0] <= r[2] + 1e-5 && v.uv[1] >= r[1] - 1e-5 && v.uv[1] <= r[3] + 1e-5
        };
        // OVERLAY (0.25..0.5) und FLOW (0.5..0.75) beruehren sich bei 0.5; nur Vertices
        // ECHT innerhalb der Overlay-Kachel zaehlen als Treffer.
        let overlay_treffer = opaque.iter().filter(|v| in_kachel(v, OVERLAY) && !in_kachel(v, FLOW)).count();
        assert_eq!(overlay_treffer, 0, "Lava darf nie die water_overlay-Kachel sampeln");
        assert!(opaque.iter().any(|v| in_kachel(v, FLOW)), "die Lavaseite zum Glas muss die Fliesskachel tragen");
    }

    // Zwei Lavazellen nebeneinander duerfen ihre gemeinsame Flaeche nicht zeichnen. Vor dem
    // Umbau lief Lava ueber die Box-Schleife, und weil has_resolved_shape("lava") false ist
    // (lava.json hat keine "elements"), galt der Lava-Nachbar dort als "verdeckt nichts" --
    // eine Lavasee zeichnete JEDE innere Flaeche mit.
    #[test]
    fn lava_neben_lava_culled_die_gemeinsame_flaeche() {
        let lava = state_von("lava");
        let (allein, _) = mesh(&welt(lava, None), &BlockUvs::single_tile());
        let (zu_zweit, _) = mesh(&welt(lava, Some(lava)), &BlockUvs::single_tile());
        // Ohne Culling waeren es glatt 2 * allein. Genau die zwei gemeinsamen Flaechen
        // (je doppelseitig, also 2 * 12 Vertices) muessen fehlen.
        assert_eq!(zu_zweit.len(), 2 * allein.len() - 2 * 12, "die Naht zwischen zwei Lavazellen muss verschwinden");
    }
}

// Modell-Vierecke (BlockShapes::model_quads) kommen in der Eckreihenfolge von assets::ModelQuad
// -- andersherum als face_quad_corners. Der Mesher verbindet sie deshalb 0,2,1 / 0,3,2. Hier
// nachgerechnet, dass beide Wege fuer dieselbe Oberseite dieselbe Vorderseite ergeben: sonst
// verwirft das Backface-Culling jede Feuerflaeche von vorn.
#[cfg(test)]
mod model_face_tests {
    use super::*;

    fn normale(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    }

    #[test]
    fn modell_oberseite_zeigt_wie_die_wuerfel_oberseite() {
        let w = face_quad_corners([0.0; 3], [1.0; 3], [0, 1, 0]);
        let wuerfel = normale(w[0], w[1], w[2]);
        // assets::ModelQuad, Oberseite: (x0,z0), (x1,z0), (x1,z1), (x0,z1).
        let m = [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]];
        let modell = normale(m[0], m[2], m[1]);
        assert!(wuerfel[1] > 0.0 && modell[1] > 0.0, "beide Umlaeufe muessen dieselbe Seite als vorn werten: {wuerfel:?} / {modell:?}");
    }
}

#[cfg(test)]
mod pane_cull_tests {
    use super::*;

    fn scheibe(block: &str, verbunden: &[&str]) -> u32 {
        (0..crate::block_registry::state_count() as u32)
            .find(|&s| {
                block_name(s) == Some(block)
                    && ["north", "south", "east", "west"].iter().all(|r| (property(s, r) == Some("true")) == verbunden.contains(r))
                    && property(s, "waterlogged") == Some("false")
            })
            .unwrap_or_else(|| panic!("{block} {verbunden:?} nicht gefunden"))
    }

    #[test]
    // IronBarsBlock.skipRendering: zur Seite nur, wenn beide zueinander verbunden sind, nach
    // oben und unten gegen denselben Block immer.
    fn verbundene_scheiben_verdecken_ihre_gemeinsame_flaeche() {
        let a = scheibe("glass_pane", &["east", "west"]);
        let b = scheibe("glass_pane", &["east", "west"]);
        assert!(pane_skips_face(a, b, [1, 0, 0]));
        assert!(pane_skips_face(a, b, [0, 1, 0]));
        let einzeln = scheibe("glass_pane", &[]);
        assert!(!pane_skips_face(einzeln, b, [1, 0, 0]), "ohne Verbindung bleibt die Flaeche");
        let rot = scheibe("red_stained_glass_pane", &["east", "west"]);
        assert!(!pane_skips_face(a, rot, [1, 0, 0]), "andere Scheibe: Flaeche bleibt wie in Vanilla");
        assert!(!pane_skips_face(a, rot, [0, 1, 0]));
        let gitter = scheibe("iron_bars", &["east", "west"]);
        let kupfer = scheibe("copper_bars", &["east", "west"]);
        assert!(pane_skips_face(gitter, kupfer, [1, 0, 0]), "Gitter untereinander (BlockTags.BARS)");
    }
}

