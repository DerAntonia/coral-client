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
use crate::biome_tint::{is_water, tint_kind_of_state, TintKind};
use crate::block_registry::block_name;
use crate::block_shapes::{BlockShapes, ShapeBox};
use crate::block_uvs::BlockUvs;
use crate::chunk::{is_solid, ChunkColumn, World, SECTION_SIZE, STATE_AIR};
use crate::cross_shapes::CrossBlocks;
use crate::water_blend::WaterBlend;
use crate::fluid::{always_wet, FluidKind};
use crate::state_props::property;
mod column;
mod fluids;
mod ghosts;
mod faces;
pub use column::*;
use fluids::*;
pub use ghosts::*;
pub use faces::*;

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
    #[must_use] 
    pub const fn of_column(chunk_x: i32, chunk_z: i32) -> Self {
        Self { x: chunk_x * SECTION_SIZE, z: chunk_z * SECTION_SIZE }
    }

    // Weltkoordinaten eines Blocks -> spalten-lokale Gleitkommaposition. Die Subtraktion
    // passiert in i32, VOR der f32-Wandlung -- genau darin liegt der Gewinn: (x - base_x)
    // ist eine kleine Zahl und damit in f32 exakt, x allein ist es bei grossen Koordinaten
    // laengst nicht mehr.
    #[must_use] 
    pub const fn local(self, x: i32, y: i32, z: i32) -> (f32, f32, f32) {
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
    #[must_use] 
    pub fn with_white_flash(mut self) -> Self {
        self.color = self.color.map(|c| c + 2.0);
        self
    }

    // Kennzeichen "hart abschneiden" (Feuer im durchscheinenden Pass, s. fs_main_translucent):
    // Farbe c wird als -1 - c gespeichert, also immer unter -0,5. Linear interpoliert bleibt
    // das ueber das ganze Viereck erhalten, und der Shader holt c mit -1 - x zurueck.
    #[must_use] 
    pub fn with_cutout_mark(self) -> Self {
        Self { color: self.color.map(|c| -1.0 - c), ..self }
    }

    // Kennzeichen "Wasser" (durchscheinender Pass, s. block.wgsl::fs_translucent_water): Farbe
    // c wird als c + 4 gespeichert, also immer ueber 3,5 -- ueber dem "weiss ueberblenden"
    // (c + 2) der Entities, das es im Gelaende nicht gibt.
    #[must_use]
    pub fn with_water_mark(self) -> Self {
        Self { color: self.color.map(|c| c + 4.0), ..self }
    }

    #[must_use]
    pub fn is_water_marked(&self) -> bool {
        self.color[0] > 3.5
    }

    // Die Farbe ohne Kennzeichen (fuer alles ausser dem PvP-Shader).
    #[must_use] 
    pub fn plain_color(&self) -> [f32; 3] {
        if self.color[0] < -0.5 {
            self.color.map(|c| -1.0 - c)
        } else if self.is_water_marked() {
            self.color.map(|c| c - 4.0)
        } else {
            self.color
        }
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
#[derive(Clone)]
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

impl ColumnNeighbors {
    // Dieselbe Umgebung der Spalte (chunk_x, chunk_z), aber mit den Bloecken aus `blocks`
    // (Weltposition, Zustand) -- der Stand VOR einer Reihe von Aenderungen (s. World::
    // take_mesh_request). Kopiert nur die betroffenen Spalten.
    #[must_use]
    pub fn with_blocks(&self, chunk_x: i32, chunk_z: i32, blocks: &[crate::BlockChange]) -> Self {
        let mut out = self.clone();
        for &((x, y, z), state) in blocks {
            let (dx, dz) = (x.div_euclid(SECTION_SIZE) - chunk_x, z.div_euclid(SECTION_SIZE) - chunk_z);
            let spalte = match (dx, dz) {
                (0, 0) => Some(&mut out.target),
                (0, -1) => out.north.as_mut(),
                (0, 1) => out.south.as_mut(),
                (1, 0) => out.east.as_mut(),
                (-1, 0) => out.west.as_mut(),
                (1, -1) => out.northeast.as_mut(),
                (-1, -1) => out.northwest.as_mut(),
                (1, 1) => out.southeast.as_mut(),
                (-1, 1) => out.southwest.as_mut(),
                _ => None,
            };
            let Some(spalte) = spalte else { continue };
            let index = (y - spalte.min_y).div_euclid(SECTION_SIZE);
            if index < 0 || index as usize >= spalte.sections.len() {
                continue;
            }
            let ly = (y - spalte.min_y).rem_euclid(SECTION_SIZE);
            Arc::make_mut(spalte).sections[index as usize].set(x.rem_euclid(SECTION_SIZE), ly, z.rem_euclid(SECTION_SIZE), state);
        }
        out
    }
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
#[must_use] 
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
mod flick_tests;

#[cfg(test)]
mod glass_tests;

#[cfg(test)]
mod water_uv_tests;

#[cfg(test)]
mod winding_tests;

#[cfg(test)]
mod tests;

// Waterlogged-Bloecke (Stufe, Zaun, Seegras, ...) bekommen ihre Wasserfuellung ueber
// push_waterlogged_fill -- einen ANDEREN Pfad als echte Wasserbloecke. Genau dort war die
// Oberflaeche laengst richtig (Vanillas Eck-Hoehen), die WAENDE aber noch bei der flachen
// Vollbox: sie reichten bis zur Zellkante, waehrend das Wasser darunter endete.
#[cfg(test)]
mod waterlogged_wand_tests;

#[cfg(test)]
mod grosse_koordinaten_tests;

#[cfg(test)]
mod coplanar_conflict_tests;

// Lava geht seit dem Fluid-Umbau durch EXAKT denselben Zeichenpfad wie Wasser
// (push_fluid_quads) -- diese Tests halten die drei Stellen fest, an denen Vanillas
// FluidRenderer sie dann doch unterscheidet, plus den eigentlichen Fehler:
// die Lava war einen ganzen Block hoch.
#[cfg(test)]
mod lava_tests;

// Modell-Vierecke (BlockShapes::model_quads) kommen in der Eckreihenfolge von assets::ModelQuad
// -- andersherum als face_quad_corners. Der Mesher verbindet sie deshalb 0,2,1 / 0,3,2. Hier
// nachgerechnet, dass beide Wege fuer dieselbe Oberseite dieselbe Vorderseite ergeben: sonst
// verwirft das Backface-Culling jede Feuerflaeche von vorn.
#[cfg(test)]
mod model_face_tests;

#[cfg(test)]
mod pane_cull_tests;
