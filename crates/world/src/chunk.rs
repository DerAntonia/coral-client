// Client-seitiger Weltzustand: Chunk-Spalten aus Block-State-IDs, so wie sie vom Server
// kommen (protocol::chunk). Wir uebersetzen sie bewusst NICHT in ein eigenes Block-Enum:
// welche State-ID welcher Block ist, steht in der generierten blocks.json des Servers und
// ist noch nicht eingebunden. Bis dahin gilt nur die eine sichere Zusage: State 0 = Luft.

use std::sync::Arc;

// FxHashMap/FxHashSet statt der Standard-Varianten: identische API, aber ein schneller
// nicht-kryptografischer Hash. Spalten werden pro Frame mehrere tausend Mal nachgeschlagen
// (Mesher, Nachbarsuche, enforce_render_limits) -- SipHash kostet dort spuerbar mehr, ohne
// hier etwas zu bringen (s. Kommentar an rustc-hash in der Workspace-Cargo.toml).
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

pub const SECTION_SIZE: i32 = 16;
pub const SECTION_VOLUME: usize = 16 * 16 * 16;

pub const STATE_AIR: u32 = 0;

// Unterste Bauhoehe der Overworld -- und nur die VORGABE, bis der Server seine echte sagt.
//
// Sie steht in den Registry-Daten (`minecraft:dimension_type`, Feld `min_y`), die im
// Configuration-State kommen. Frueher stand hier eine harte Konstante fuer die ganze Welt,
// und genau das war ein echter Bug: die Pruefwelt von modern.anticheat-test.com hat
// min_y = 0 und 28 Sections (448 Bloecke hoch). Mit -64 landete dort JEDER Block 64 Bloecke
// zu tief -- gemessen am 2026-09-22 gegen einen echten Vanilla-Client durch den MITM-Proxy:
// Vanilla stand auf der Pruefplattform bei y=91.0, wir bei y=27.0. Die Physik rechnete dabei
// voellig richtig, nur eben an der falschen Stelle, und der Server warf uns hinaus.
//
// Deshalb traegt jetzt jede Spalte ihre eigene Untergrenze (s. ChunkColumn::min_y).
pub const DEFAULT_MIN_Y: i32 = -64;
// Alter Name, damit Tests und Fremdcode weiterlaufen.
pub const MIN_Y: i32 = DEFAULT_MIN_Y;

pub fn is_solid(state: u32) -> bool {
    state != STATE_AIR
}

#[derive(Clone)]
pub struct ChunkSection {
    // 4096 State-IDs, Reihenfolge wie im Protokoll: Index = (y * 16 + z) * 16 + x.
    states: Vec<u32>,
    // 4x4x4-Biom-Raster (64 Eintraege, s. protocol::chunk::ChunkSectionData::biomes) fuer
    // die Gras-/Laub-/Wasser-Faerbung. Default (leer -> alles Biom 0) fuer Aufrufer/Tests,
    // die sich nicht um Biome scheren (s. from_states).
    biomes: Vec<u32>,
}

impl ChunkSection {
    // Fuer Tests/Aufrufer ohne echte Biom-Daten: alles Biom 0 (das erste, per Server-
    // Registrierungsreihenfolge -- typischerweise die Ozean-Variante, aber fuer reine
    // Geometrie-Tests ohne Faerbung irrelevant, welches).
    pub fn from_states(states: Vec<u32>) -> Self {
        debug_assert_eq!(states.len(), SECTION_VOLUME);
        Self { states, biomes: vec![0; 64] }
    }

    pub fn from_states_and_biomes(mut states: Vec<u32>, biomes: Vec<u32>) -> Self {
        debug_assert_eq!(states.len(), SECTION_VOLUME);
        // Hoehlen-/Leerluft wird Luft, s. block_registry::air_variants.
        let luft = crate::block_registry::air_variants();
        for s in &mut states {
            *s = crate::block_registry::normalize_air(*s, luft);
        }
        debug_assert_eq!(biomes.len(), 64);
        Self { states, biomes }
    }

    // Nur fuer Koordinaten innerhalb der Sektion; Bereichspruefung macht der Aufrufer
    // (World::block_at), weil ausserhalb der Nachbar-Chunk zustaendig ist, nicht "Luft".
    pub fn get(&self, x: i32, y: i32, z: i32) -> u32 {
        self.states[((y * SECTION_SIZE + z) * SECTION_SIZE + x) as usize]
    }

    // Fuer Block Update/Section Blocks Update (s. World::set_block) -- Bereichspruefung
    // macht dort der Aufrufer.
    pub fn set(&mut self, x: i32, y: i32, z: i32, state: u32) {
        let state = crate::block_registry::normalize_air(state, crate::block_registry::air_variants());
        self.states[((y * SECTION_SIZE + z) * SECTION_SIZE + x) as usize] = state;
    }

    // Biom-ID an einer sektionslokalen Koordinate -- eine 4x4x4-Zelle deckt vier
    // Block-Koordinaten je Achse ab, daher die Division.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> u32 {
        let (bx, by, bz) = (x / 4, y / 4, z / 4);
        self.biomes[((by * 4 + bz) * 4 + bx) as usize]
    }

    // Alle 4096 State-IDs in Protokollreihenfolge (Index = (y * 16 + z) * 16 + x).
    pub fn states(&self) -> &[u32] {
        &self.states
    }

    pub fn is_empty(&self) -> bool {
        self.states.iter().all(|&s| !is_solid(s))
    }
}

#[derive(Clone)]
pub struct ChunkColumn {
    // Von unten (min_y) nach oben.
    pub sections: Vec<ChunkSection>,
    // Weltkoordinate der untersten Blockreihe dieser Spalte. Haengt an der Dimension, nicht
    // am Client (s. DEFAULT_MIN_Y). Sitzt an der SPALTE und nicht nur an der World, weil der
    // Mesher auf Spalten-Snapshots arbeitet und dabei keine &World hat.
    pub min_y: i32,
    // Himmelslicht vom Server, je Section eine Ebene, dazu eine darunter und eine darueber
    // (s. light::SkyLayer). Nur der Shader-Pfad liest es.
    pub sky: Vec<crate::light::SkyLayer>,
}

impl ChunkColumn {
    // x/z sind chunk-lokal (0..16), y ist eine absolute Welt-Hoehe. Oeffentlich, weil der
    // Mesher (s. mesher.rs) direkt auf Arc<ChunkColumn>-Nachbarn zugreift, ohne den Umweg
    // ueber World::block_at -- der bräuchte eine &World, die ein Worker-Thread nicht
    // borgen kann, waehrend der Hauptthread parallel weiter an der World arbeitet.
    pub fn get(&self, x: i32, y: i32, z: i32) -> u32 {
        let index = (y - self.min_y).div_euclid(SECTION_SIZE);
        if index < 0 || index as usize >= self.sections.len() {
            return STATE_AIR; // ueber oder unter der Welt
        }
        self.sections[index as usize].get(x, (y - self.min_y).rem_euclid(SECTION_SIZE), z)
    }

    // Biom-ID an einer Spalten-lokalen Koordinate (x/z 0..16, y absolute Welt-Hoehe). 0 fuer
    // ausserhalb der geladenen Sections -- ein plausibler, wenn auch nicht immer exakter
    // Default (echte Randfaerbung braucht ohnehin den Nachbarn).
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> u32 {
        let index = (y - self.min_y).div_euclid(SECTION_SIZE);
        if index < 0 || index as usize >= self.sections.len() {
            return 0;
        }
        self.sections[index as usize].biome_at(x, (y - self.min_y).rem_euclid(SECTION_SIZE), z)
    }
}

pub struct World {
    // Arc statt ChunkColumn direkt: der Mesher braucht Kopien (genauer: geteiltes
    // Eigentum) der Zielspalte plus ihrer vier Nachbarn, um auf einem Worker-Thread zu
    // laufen, ohne die World zu leihen (s. mesher.rs). Ein Arc-Klon ist nur ein
    // Referenzzaehler -- keine 384-KB-Kopie der Spaltendaten pro Mesh-Job.
    columns: HashMap<(i32, i32), Arc<ChunkColumn>>,
    // Positionen aller geladenen Betten samt State. Betten haben kein Blockmodell und werden
    // deshalb nicht mit dem Chunk gemesht, sondern einzeln im Entity-Pass gezeichnet (s.
    // block_entity.rs) -- dieser Index erspart dem Zeichenpfad, dafuer jeden Frame die ganze
    // Welt abzusuchen. Gepflegt an genau den drei Stellen, an denen sich Bloecke aendern:
    // insert_column, remove_column und set_block.
    beds: HashMap<(i32, i32, i32), u32>,
    // Spalten, deren Mesh (neu) gebaut werden muss.
    dirty: HashSet<(i32, i32)>,
    // Zaehlt jede Aenderung der SPALTENMENGE mit (laden/entladen/leeren) -- nicht Aenderungen
    // an Blockdaten innerhalb einer Spalte. Damit koennen Aufrufer teure Arbeit ueber die
    // gesamte Spaltenmenge ueberspringen, solange sich nichts geaendert hat (s.
    // App::enforce_render_limits, das sonst jeden Frame alle ~2400 Spalten neu sortierte).
    // column_count() allein reicht dafuer NICHT: eine Spalte rein und gleichzeitig eine raus
    // laesst die Anzahl unveraendert, die Menge aber nicht.
    column_generation: u64,
    // VORHERGESAGTE Bloecke: was der Client selbst gesetzt hat, bevor der Server es
    // bestaetigt hat (Platzieren, s. app::PendingBlock). Bewusst NEBEN den Spaltendaten und
    // nicht darin:
    //
    //   * block_at() liefert sie mit -- Kollision, Strahl und Fluessigkeitspruefung sehen den
    //     Block also sofort. Genau darum geht es: ohne das laeuft man in einen gerade
    //     gesetzten Block hinein, waehrend der Server ihn schon hat, und GrimAC flaggt die
    //     Abweichung.
    //   * Der Mesher sieht sie NICHT -- der arbeitet auf Spalten-Snapshots
    //     (column_neighbors), nicht ueber block_at. Damit taucht ein vorhergesagter Block
    //     nicht im normalen, undurchsichtigen Chunk-Mesh auf und kann als halbdurchsichtiger
    //     Geisterblock gezeichnet werden, bis der Server ihn bestaetigt.
    pending: HashMap<(i32, i32, i32), u32>,
    // Untergrenze DIESER Dimension (s. DEFAULT_MIN_Y). Wird aus den Registry-Daten gesetzt,
    // bevor die ersten Chunks kommen.
    min_y: i32,
    // Shader-Pfad aktiv: die Meshes tragen Licht. Dann aendert ein leuchtender oder
    // lichtdaempfender Block auch die Nachbarspalten (Licht reicht 14 Bloecke weit).
    pub light_remesh: bool,
    // Spalten, deren Bloecke sich seit dem letzten take_voxel_changes geaendert haben (geladen,
    // entladen, Block gesetzt) -- fuer das Voxel-Abbild des Shader-Pfads (s. voxel.rs). Nur
    // die Spalte selbst, nicht ihre Nachbarn wie bei `dirty`.
    voxel_dirty: HashSet<(i32, i32)>,
    // clear() seit dem letzten take_voxel_changes: das ganze Abbild ist veraltet.
    voxel_cleared: bool,
    // Die Weltgrenze gehoert zur Welt: eine neue Welt faengt mit der Grenze ohne Einschraenkung
    // an, bis der Server ihre eigene schickt.
    pub border: crate::world_border::WorldBorder,
    // Kollisionsformen der Bloecke, die gerade ein Kolben bewegt (s. app::pistons). Ihre Zelle
    // traegt dabei moving_piston ohne eigene Form; wo der Block steht, haengt am Fortschritt
    // der Bewegung -- deshalb jeden Tick neu gesetzt statt aus der Formtabelle gelesen.
    pub moving_boxes: Vec<MovingBox>,
}

// Eine Box (Weltkoordinaten) eines vom Kolben bewegten Blocks. `noclip`: die Bewegungsrichtung
// (Richtungs-Index 0..6 wie Vanillas 3D-Datenwert), solange der Block noch faehrt -- wen der
// Kolben in genau diese Richtung schiebt, der stoesst sich waehrend des Schubs nicht an ihm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovingBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub noclip: Option<u8>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            columns: HashMap::default(),
            beds: HashMap::default(),
            dirty: HashSet::default(),
            column_generation: 0,
            pending: HashMap::default(),
            min_y: DEFAULT_MIN_Y,
            light_remesh: false,
            voxel_dirty: HashSet::default(),
            voxel_cleared: false,
            border: Default::default(),
            moving_boxes: Vec::new(),
        }
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    // Die Untergrenze der Dimension setzen. Aendert sie sich, sind alle geladenen Spalten
    // hinfaellig -- sie haengen in der falschen Hoehe. Der Aufrufer meldet die neue
    // Dimension, bevor deren Chunks kommen, in der Praxis ist die Welt hier also leer.
    pub fn set_min_y(&mut self, min_y: i32) {
        if self.min_y == min_y {
            return;
        }
        self.min_y = min_y;
        self.clear();
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    // s. Feldkommentar an column_generation.
    pub fn column_generation(&self) -> u64 {
        self.column_generation
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    pub fn column(&self, x: i32, z: i32) -> Option<&ChunkColumn> {
        self.columns.get(&(x, z)).map(Arc::as_ref)
    }

    // Fuer den Mesher: ein eigenes Arc, das laenger lebt als der Borrow auf `self` und
    // damit an einen Worker-Thread uebergeben werden kann (s. mesher::ColumnNeighbors).
    pub fn column_arc(&self, x: i32, z: i32) -> Option<Arc<ChunkColumn>> {
        self.columns.get(&(x, z)).cloned()
    }

    // Buendelt Zielspalte + die vier orthogonalen Nachbarn zu einem Mesh-Job, der als
    // Ganzes an einen Worker-Thread geschickt werden kann -- None, wenn die Zielspalte
    // selbst noch nicht geladen ist (fehlende NACHBARN sind dagegen normal und zaehlen
    // im Mesher als Luft, s. dort).
    pub fn column_neighbors(&self, x: i32, z: i32) -> Option<crate::mesher::ColumnNeighbors> {
        let target = self.column_arc(x, z)?;
        Some(crate::mesher::ColumnNeighbors {
            target,
            north: self.column_arc(x, z - 1),
            south: self.column_arc(x, z + 1),
            east: self.column_arc(x + 1, z),
            west: self.column_arc(x - 1, z),
            // Diagonale Nachbarn: bis vor Kurzem brauchte block_at() sie nie (FACES sind
            // ausschliesslich Einheitsvektoren, s. mesher.rs-Modulkommentar), seit Wasser
            // seine Eck-Hoehen aus bis zu 8 Nachbarn mittelt (s. world::fluid::
            // calculate_corner_height) aber schon -- eine Wasserzelle direkt an einer
            // Spalten-ECKE braucht z.B. den Nachbarn bei (x-1,z-1), der weder in `north`
            // noch in `west` liegt.
            northeast: self.column_arc(x + 1, z - 1),
            northwest: self.column_arc(x - 1, z - 1),
            southeast: self.column_arc(x + 1, z + 1),
            southwest: self.column_arc(x - 1, z + 1),
        })
    }

    pub fn insert_column(&mut self, x: i32, z: i32, sections: Vec<ChunkSection>) {
        self.reindex_beds(x, z, &sections);
        let sky = crate::light::default_sky(sections.len());
        self.columns.insert((x, z), Arc::new(ChunkColumn { sections, min_y: self.min_y, sky }));
        self.column_generation = self.column_generation.wrapping_add(1);
        self.dirty.insert((x, z));
        self.voxel_dirty.insert((x, z));
        // Die schon geladenen Nachbarn ebenfalls neu meshen: deren Rand wurde bisher
        // gegen "nicht geladen = Luft" gemesht und traegt daher eine komplette Wand
        // unsichtbarer Quads zu uns hin. Die faellt jetzt weg. Diagonalen zaehlen mit
        // (nicht nur orthogonale) -- eine Wasserzelle an einer Spalten-ECKE beim Nachbarn
        // mittelt ihre Eck-Hoehen auch aus DIESER neu geladenen Spalte (s. push_fluid_quads).
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let neighbor = (x + dx, z + dz);
            if self.columns.contains_key(&neighbor) {
                self.dirty.insert(neighbor);
            }
        }
    }

    // Entlaedt eine Spalte (Server: Forget Level Chunk). Gibt true zurueck, wenn wirklich
    // eine Spalte entfernt wurde -- dann muss der Aufrufer auch ihr Mesh aus dem Renderer
    // werfen (VRAM freigeben, s. App). Die verbleibenden Nachbarn werden neu gemesht: ihre
    // zu uns zeigende Seite war bisher als "an solidem Nachbarn -> verdeckt" weggecullt und
    // muss jetzt wieder Flaechen bekommen (Gegenstueck zu insert_column).
    // Himmelslicht aus einem Chunk- oder Licht-Paket uebernehmen (s. light::apply_sky).
    // false, wenn die Spalte nicht geladen ist.
    pub fn apply_sky_light(&mut self, x: i32, z: i32, light: &protocol::chunk::LightData) -> bool {
        let Some(col) = self.columns.get_mut(&(x, z)) else { return false };
        crate::light::apply_sky(&mut Arc::make_mut(col).sky, light);
        true
    }

    pub fn remove_column(&mut self, x: i32, z: i32) -> bool {
        if self.columns.remove(&(x, z)).is_none() {
            return false;
        }
        self.beds.retain(|pos, _| pos.0.div_euclid(SECTION_SIZE) != x || pos.2.div_euclid(SECTION_SIZE) != z);
        self.column_generation = self.column_generation.wrapping_add(1);
        self.dirty.remove(&(x, z));
        self.voxel_dirty.insert((x, z));
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let neighbor = (x + dx, z + dz);
            if self.columns.contains_key(&neighbor) {
                self.dirty.insert(neighbor);
            }
        }
        true
    }

    // Block an absoluten Weltkoordinaten. Nicht geladene Chunks zaehlen als Luft -- an
    // solchen Raendern entstehen dadurch ueberfluessige Quads, die verschwinden, sobald
    // der Nachbar ankommt und die Spalte neu gemesht wird.
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> u32 {
        // Vorhergesagte Bloecke gehen vor (s. Feldkommentar an `pending`).
        if let Some(&state) = self.pending.get(&(x, y, z)) {
            return state;
        }
        let key = (x.div_euclid(SECTION_SIZE), z.div_euclid(SECTION_SIZE));
        match self.columns.get(&key) {
            Some(column) => column.get(x.rem_euclid(SECTION_SIZE), y, z.rem_euclid(SECTION_SIZE)),
            None => STATE_AIR,
        }
    }

    // Biom-ID an absoluten Weltkoordinaten; 0 fuer nicht geladene Spalten.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> u32 {
        let key = (x.div_euclid(SECTION_SIZE), z.div_euclid(SECTION_SIZE));
        self.columns.get(&key).map_or(0, |c| c.biome_at(x.rem_euclid(SECTION_SIZE), y, z.rem_euclid(SECTION_SIZE)))
    }

    // Vanillas Heightmap MOTION_BLOCKING an (x, z): der oberste Block, der Bewegung blockiert
    // (collision::blocks_movement) oder eine Fluessigkeit enthaelt -- als Y des BLOCKS, nicht
    // wie in Vanilla eins darueber. None = nichts in der Spalte (oder nicht geladen).
    //
    // Gerechnet statt als gepflegte Heightmap mitgefuehrt. Gebraucht von der Regenpruefung
    // beim Dreizack und dem Niederschlag (einige hundert Spalten je Tick) -- deshalb liest die
    // Suche die Spalte direkt statt je Block ueber block_at (zwei Hash-Zugriffe). Nur wenn in
    // dieser Spalte ein vorhergesagter Block steht, geht sie den langsamen Weg.
    pub fn motion_blocking_top(&self, x: i32, z: i32, shapes: &crate::block_shapes::BlockShapes) -> Option<i32> {
        self.motion_blocking_top_except(x, z, shapes, |_| false)
    }

    // Wie motion_blocking_top, aber Bloecke, fuer die `skip` gilt, zaehlen als Luft -- der
    // Niederschlag im Bild faellt so durch Barrieren (s. app::precipitation).
    pub fn motion_blocking_top_except(
        &self,
        x: i32,
        z: i32,
        shapes: &crate::block_shapes::BlockShapes,
        skip: impl Fn(u32) -> bool,
    ) -> Option<i32> {
        let key = (x.div_euclid(SECTION_SIZE), z.div_euclid(SECTION_SIZE));
        let column = self.columns.get(&key)?;
        let top = column.min_y + column.sections.len() as i32 * SECTION_SIZE - 1;
        let blockiert = |state: u32| {
            state != STATE_AIR && (crate::collision::blocks_movement(state, shapes) || crate::fluid::fluid_kind(state).is_some()) && !skip(state)
        };
        if self.pending.keys().any(|&(px, _, pz)| px == x && pz == z) {
            return (column.min_y..=top).rev().find(|&y| blockiert(self.block_at(x, y, z)));
        }
        let (lx, lz) = (x.rem_euclid(SECTION_SIZE), z.rem_euclid(SECTION_SIZE));
        (column.min_y..=top).rev().find(|&y| blockiert(column.get(lx, y, lz)))
    }

    // Der Block OHNE die Vorhersage -- was zuletzt wirklich vom Server kam. Fuer den
    // Aufrufer, der wissen muss, worauf er zuruecknehmen wuerde.
    pub fn server_block_at(&self, x: i32, y: i32, z: i32) -> u32 {
        let key = (x.div_euclid(SECTION_SIZE), z.div_euclid(SECTION_SIZE));
        match self.columns.get(&key) {
            Some(column) => column.get(x.rem_euclid(SECTION_SIZE), y, z.rem_euclid(SECTION_SIZE)),
            None => STATE_AIR,
        }
    }

    // Einen vorhergesagten Block setzen bzw. wieder wegnehmen. Kein dirty-Flag: der Mesher
    // sieht diese Bloecke ohnehin nicht (s. Feldkommentar), gezeichnet werden sie als
    // Geisterblock ueber pending_blocks().
    pub fn set_pending_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        self.pending.insert((x, y, z), state);
    }

    pub fn clear_pending_block(&mut self, x: i32, y: i32, z: i32) {
        self.pending.remove(&(x, y, z));
    }

    // Der vorhergesagte Zustand an dieser Stelle, falls es einen gibt.
    // Der Zustand in den echten Chunk-Daten, OHNE die Vorhersage-Schicht.
    pub fn chunk_block_at(&self, x: i32, y: i32, z: i32) -> u32 {
        let key = (x.div_euclid(SECTION_SIZE), z.div_euclid(SECTION_SIZE));
        match self.columns.get(&key) {
            Some(column) => column.get(x.rem_euclid(SECTION_SIZE), y, z.rem_euclid(SECTION_SIZE)),
            None => STATE_AIR,
        }
    }

    pub fn pending_block_at(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        self.pending.get(&(x, y, z)).copied()
    }

    pub fn pending_blocks(&self) -> impl Iterator<Item = ((i32, i32, i32), u32)> + '_ {
        self.pending.iter().map(|(&pos, &state)| (pos, state))
    }

    pub fn has_pending_blocks(&self) -> bool {
        !self.pending.is_empty()
    }

    // Einzelner Block-Update (Block Update/Section Blocks Update, s. net::ClientEvent::
    // BlockUpdate) -- setzt den State an absoluten Weltkoordinaten und meldet die
    // betroffene Spalte (und ggf. den Rand-Nachbarn, s.u.) dirty, damit der Mesher sie neu
    // baut. Nicht geladene Chunks verwerfen den Update stillschweigend: ein Block, den wir
    // eh nicht geladen haben, kommt beim naechsten Chunk-Laden ohnehin aktuell an.
    //
    // Arc::make_mut statt direkter Mutation: eine Mesh-Worker-Thread haelt womoeglich noch
    // ein Arc-Klon dieser Spalte (s. column_neighbors) -- make_mut klont die Spalte bei
    // Bedarf (copy-on-write), der Worker rechnet dann in Ruhe mit dem alten Snapshot zu
    // Ende, statt mitten im Zugriff auf veraenderte Daten zu stossen.
    // Betten einer frisch geladenen Spalte erfassen. Verglichen werden reine State-Ids (s.
    // block_entity::bed_states) -- dieselbe Groessenordnung Arbeit wie das Meshen, das ohnehin
    // gleich danach folgt.
    fn reindex_beds(&mut self, x: i32, z: i32, sections: &[ChunkSection]) {
        self.beds.retain(|pos, _| pos.0.div_euclid(SECTION_SIZE) != x || pos.2.div_euclid(SECTION_SIZE) != z);
        // Laeuft im Haupt-Thread fuer JEDE ankommende Spalte ueber ~100.000 Bloecke -- deshalb
        // eine Tabelle je State statt mehrerer HashSet-Nachschlaege pro Block. Mit vier
        // Nachschlaegen (seit Shulker/Truhen/Koepfe) froren sonst Bilder beim Laden ein.
        let tabelle = crate::block_entity::model_block_table();
        let ist_modell = |s: u32| tabelle.get(s as usize).copied().unwrap_or(false);
        for (index, section) in sections.iter().enumerate() {
            let base_y = self.min_y + index as i32 * SECTION_SIZE;
            for (i, &state) in section.states().iter().enumerate() {
                if ist_modell(state) {
                    let (lx, lz, ly) = ((i % 16) as i32, ((i / 16) % 16) as i32, (i / 256) as i32);
                    self.beds.insert((x * SECTION_SIZE + lx, base_y + ly, z * SECTION_SIZE + lz), state);
                }
            }
        }
    }

    // Alle geladenen Bloecke mit BlockEntity-Modell (Betten, Shulkerkisten; Position -> State)
    // fuer den Zeichenpfad -- der Aufrufer sortiert per bed_instance/shulker_instance.
    pub fn beds(&self) -> impl Iterator<Item = ((i32, i32, i32), u32)> + '_ {
        self.beds.iter().map(|(pos, state)| (*pos, *state))
    }

    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        let key = (x.div_euclid(SECTION_SIZE), z.div_euclid(SECTION_SIZE));
        let Some(column) = self.columns.get_mut(&key) else { return };
        let index = (y - self.min_y).div_euclid(SECTION_SIZE);
        if index < 0 {
            return;
        }
        let index = index as usize;
        if index >= column.sections.len() {
            return;
        }
        let local_x = x.rem_euclid(SECTION_SIZE);
        let local_y = (y - self.min_y).rem_euclid(SECTION_SIZE);
        let local_z = z.rem_euclid(SECTION_SIZE);
        let state = crate::block_registry::normalize_air(state, crate::block_registry::air_variants());
        let vorher = column.sections[index].get(local_x, local_y, local_z);
        Arc::make_mut(column).sections[index].set(local_x, local_y, local_z, state);
        if self.light_remesh
            && (crate::light::luminance(vorher) != crate::light::luminance(state) || crate::light::opacity(vorher) != crate::light::opacity(state))
        {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let k = (key.0 + dx, key.1 + dz);
                    if self.columns.contains_key(&k) {
                        self.dirty.insert(k);
                    }
                }
            }
        }
        if crate::block_entity::is_model_block(state) {
            self.beds.insert((x, y, z), state);
        } else {
            self.beds.remove(&(x, y, z));
        }
        self.dirty.insert(key);
        self.voxel_dirty.insert(key);
        // Am Spaltenrand geaenderte Bloecke beeinflussen das Culling der Grenzflaeche im
        // Nachbarn (s. insert_column) -- der muss also ebenfalls neu gemesht werden.
        let mut mark_neighbor = |dx: i32, dz: i32| {
            let neighbor = (key.0 + dx, key.1 + dz);
            if self.columns.contains_key(&neighbor) {
                self.dirty.insert(neighbor);
            }
        };
        if local_x == 0 {
            mark_neighbor(-1, 0);
        }
        if local_x == SECTION_SIZE - 1 {
            mark_neighbor(1, 0);
        }
        if local_z == 0 {
            mark_neighbor(0, -1);
        }
        if local_z == SECTION_SIZE - 1 {
            mark_neighbor(0, 1);
        }
    }

    // Gibt bis zu `limit` Spalten zurueck, die neu gemesht werden muessen, und streicht
    // sie von der Liste. Begrenzt, damit ein Schwall neuer Chunks nicht die Frame-Zeit
    // sprengt -- der Rest kommt in den naechsten Frames dran.
    pub fn take_dirty(&mut self, limit: usize) -> Vec<(i32, i32)> {
        let taken: Vec<(i32, i32)> = self.dirty.iter().copied().take(limit).collect();
        for key in &taken {
            self.dirty.remove(key);
        }
        taken
    }

    // Fuer Render-Distanz-/Chunk-Anzahl-Limiter (s. App::enforce_render_limits): alle
    // geladenen Spalten, um sie nach Distanz zur Spielerposition zu sortieren -- OHNE die
    // (u.U. grosse) ChunkColumn-Nutzlast zu klonen, nur die Koordinaten.
    pub fn column_keys(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.columns.keys().copied()
    }

    // Nimmt eine geladene, aber (noch) nicht dirty-markierte Spalte erneut in die
    // Mesh-Warteschlange auf -- fuer Spalten, die ein Render-Limiter zuvor absichtlich
    // ausgelassen hat und die jetzt (Spieler naeher dran, Limit erhoeht) wieder gebaut werden
    // sollen. No-op, wenn die Spalte gar nicht geladen ist.
    pub fn mark_dirty(&mut self, x: i32, z: i32) {
        if self.columns.contains_key(&(x, z)) {
            self.dirty.insert((x, z));
        }
    }

    // Steht diese Spalte gerade in der Mesh-Warteschlange?
    pub fn is_dirty(&self, x: i32, z: i32) -> bool {
        self.dirty.contains(&(x, z))
    }

    // Gegenstueck zu mark_dirty: eine Spalte bewusst NICHT (mehr) meshen, ohne sie wie
    // remove_column komplett zu entladen -- die Blockdaten bleiben (kein Loch in der Welt,
    // z.B. fuer block_at()), nur der Mesh-Job entfaellt. Fuer Spalten, die gerade erst als
    // dirty markiert wurden (frisch eingetroffen oder durch einen Nachbar-Update), aber noch
    // nicht gebaut sind.
    pub fn clear_dirty(&mut self, x: i32, z: i32) {
        self.dirty.remove(&(x, z));
    }

    // Fuer das Voxel-Abbild (s. voxel_dirty): ob die Welt geleert wurde, und welche Spalten
    // sich seitdem geaendert haben. Leert beides.
    pub fn take_voxel_changes(&mut self) -> (bool, Vec<(i32, i32)>) {
        let cleared = std::mem::take(&mut self.voxel_cleared);
        (cleared, self.voxel_dirty.drain().collect())
    }

    pub fn clear(&mut self) {
        self.columns.clear();
        self.beds.clear();
        self.column_generation = self.column_generation.wrapping_add(1);
        self.dirty.clear();
        self.voxel_dirty.clear();
        self.voxel_cleared = true;
        // Offene Vorhersagen gehoeren zur alten Welt -- ihre Bestaetigung kommt nie mehr.
        self.pending.clear();
        self.border = Default::default();
        self.moving_boxes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_column() -> Vec<ChunkSection> {
        vec![ChunkSection::from_states(vec![STATE_AIR; SECTION_VOLUME])]
    }

    fn stein_section() -> ChunkSection {
        ChunkSection::from_states(vec![1u32; SECTION_VOLUME])
    }

    // Der Bug vom 2026-09-22: die Welt-Untergrenze war als -64 festverdrahtet. Auf einer
    // Dimension mit min_y = 0 landete damit jeder Block 64 Bloecke zu tief -- gemessen
    // gegen einen echten Vanilla-Client (Vanilla stand auf der Pruefplattform bei y=91.0,
    // wir bei y=27.0).
    #[test]
    fn die_untergrenze_entscheidet_wo_die_sections_liegen() {
        let mut welt = World::new();
        assert_eq!(welt.min_y(), DEFAULT_MIN_Y, "ohne Angabe gilt die Overworld-Vorgabe");
        welt.insert_column(0, 0, vec![stein_section()]);
        assert_eq!(welt.block_at(0, -64, 0), 1, "unterste Section beginnt bei -64");
        assert_eq!(welt.block_at(0, 0, 0), STATE_AIR, "und reicht nur bis -49");

        // Dieselben Daten in einer Dimension, die bei 0 anfaengt.
        let mut welt = World::new();
        welt.set_min_y(0);
        welt.insert_column(0, 0, vec![stein_section()]);
        assert_eq!(welt.block_at(0, 0, 0), 1, "jetzt beginnt die unterste Section bei 0");
        assert_eq!(welt.block_at(0, 15, 0), 1);
        assert_eq!(welt.block_at(0, -64, 0), STATE_AIR, "und unter der Welt ist nichts mehr");
    }

    // Die Spalte traegt ihre Untergrenze selbst mit -- der Mesher hat keine &World und
    // liest sie von dort (s. mesher::mesh_column).
    #[test]
    fn die_spalte_kennt_ihre_eigene_untergrenze() {
        let mut welt = World::new();
        welt.set_min_y(0);
        welt.insert_column(0, 0, vec![stein_section()]);
        let spalte = welt.column(0, 0).expect("geladen");
        assert_eq!(spalte.min_y, 0);
        assert_eq!(spalte.get(0, 0, 0), 1);
        assert_eq!(spalte.get(0, -1, 0), STATE_AIR);
    }

    // Eine neue Untergrenze macht alles Geladene hinfaellig: die Spalten haengen sonst in
    // der falschen Hoehe weiter.
    #[test]
    fn ein_wechsel_der_untergrenze_leert_die_welt() {
        let mut welt = World::new();
        welt.insert_column(0, 0, vec![stein_section()]);
        assert_eq!(welt.column_count(), 1);
        welt.set_min_y(0);
        assert_eq!(welt.column_count(), 0, "alte Spalten sind nach dem Wechsel wertlos");
        welt.set_min_y(0);
        assert_eq!(welt.min_y(), 0, "derselbe Wert nochmal ist ein No-Op");
    }

    // Regressionstest fuer die Render-Limiter-Helfer (s. App::enforce_render_limits):
    // mark_dirty() ist ein No-Op fuer eine nicht geladene Spalte -- sonst koennte ein
    // Limiter versehentlich Phantom-Eintraege in die Dirty-Menge schreiben.
    #[test]
    fn mark_dirty_ignoriert_nicht_geladene_spalten() {
        let mut world = World::new();
        world.mark_dirty(5, 5);
        assert_eq!(world.dirty_count(), 0, "eine nicht geladene Spalte darf nicht dirty werden");
    }

    // clear_dirty() nimmt eine Spalte aus der Mesh-Warteschlange, OHNE sie zu entladen -- der
    // ganze Punkt gegenueber remove_column() (s. Funktionskommentar): Blockdaten bleiben fuer
    // block_at() erhalten, nur der Mesh-Job entfaellt.
    #[test]
    fn clear_dirty_entlaedt_die_spalte_nicht() {
        let mut world = World::new();
        world.insert_column(0, 0, empty_column());
        assert_eq!(world.dirty_count(), 1, "insert_column muss die neue Spalte selbst dirty markieren");

        world.clear_dirty(0, 0);
        assert_eq!(world.dirty_count(), 0);
        assert!(world.column(0, 0).is_some(), "clear_dirty darf die Spalte nicht entladen, nur ihren Mesh-Job");
        assert_eq!(world.column_count(), 1);
    }

    // mark_dirty() nach clear_dirty() muss die Spalte wieder in die Warteschlange bringen --
    // das ist der Fall "Spieler kommt zurueck in den Render-Radius".
    #[test]
    fn mark_dirty_nach_clear_dirty_reiht_wieder_ein() {
        let mut world = World::new();
        world.insert_column(0, 0, empty_column());
        world.clear_dirty(0, 0);
        world.mark_dirty(0, 0);
        assert_eq!(world.take_dirty(10), vec![(0, 0)]);
    }

    #[test]
    fn column_keys_listet_alle_geladenen_spalten() {
        let mut world = World::new();
        world.insert_column(0, 0, empty_column());
        world.insert_column(1, 0, empty_column());
        world.insert_column(0, 1, empty_column());
        let mut keys: Vec<(i32, i32)> = world.column_keys().collect();
        keys.sort();
        assert_eq!(keys, vec![(0, 0), (0, 1), (1, 0)]);
    }
}

#[cfg(test)]
mod pending_und_dirty_tests {
    use super::*;

    // Der vorhergesagte Zustand muss einzeln abfragbar sein: beim Ack entscheidet der
    // Vergleich mit der Server-Aufloesung darueber, ob der Geisterblock noch stehen bleibt
    // (s. app::resolve_pending_blocks).
    #[test]
    fn ein_vorhergesagter_block_ist_einzeln_abfragbar() {
        let mut w = World::new();
        assert_eq!(w.pending_block_at(3, 64, -7), None);
        w.set_pending_block(3, 64, -7, 42);
        assert_eq!(w.pending_block_at(3, 64, -7), Some(42));
        assert_eq!(w.pending_block_at(3, 64, -6), None, "Nachbarstelle bleibt leer");
        w.clear_pending_block(3, 64, -7);
        assert_eq!(w.pending_block_at(3, 64, -7), None);
    }

    // Die schnelle Suche (direkt in der Spalte) und der Weg ueber vorhergesagte Bloecke muessen
    // dasselbe liefern; ein vorhergesagt abgebauter Block zaehlt nicht mehr.
    #[test]
    fn hoechster_blockierender_block_mit_und_ohne_vorhersage() {
        let stein = (0..crate::block_registry::state_count() as u32).find(|&s| crate::block_registry::block_name(s) == Some("stone")).expect("stone");
        let mut w = World::new();
        let mut sections = Vec::new();
        for i in 0..24 {
            let mut states = vec![STATE_AIR; SECTION_VOLUME];
            if i == 4 {
                // Section 4 beginnt bei y = 0: Stein bei (2, 5, 3) und (2, 9, 3).
                for y in [5, 9] {
                    states[((y * 16 + 3) * 16 + 2) as usize] = stein;
                }
            }
            sections.push(ChunkSection::from_states(states));
        }
        w.insert_column(0, 0, sections);
        let shapes = crate::BlockShapes::all_full_cubes();
        assert_eq!(w.motion_blocking_top(2, 3, &shapes), Some(9));
        assert_eq!(w.motion_blocking_top(4, 3, &shapes), None, "leere Spalte");
        assert_eq!(w.motion_blocking_top(40, 3, &shapes), None, "nicht geladen");
        w.set_pending_block(2, 9, 3, STATE_AIR);
        assert_eq!(w.motion_blocking_top(2, 3, &shapes), Some(5));
        // Uebersprungene Bloecke zaehlen als Luft.
        assert_eq!(w.motion_blocking_top_except(2, 3, &shapes, |s| s == stein), None);
    }

    // is_dirty meldet NUR fuer geladene Spalten -- mark_dirty tut fuer unbekannte nichts.
    #[test]
    fn is_dirty_folgt_der_warteschlange() {
        let mut w = World::new();
        assert!(!w.is_dirty(0, 0));
        w.mark_dirty(0, 0);
        assert!(!w.is_dirty(0, 0), "eine nicht geladene Spalte kommt nicht in die Schlange");
        w.clear_dirty(0, 0);
        assert!(!w.is_dirty(0, 0));
    }
}
