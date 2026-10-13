// Nachschlagetabelle State-ID -> Liste von Kollisions-/Render-Boxen (statt der bisherigen
// Annahme "jeder solide Block ist ein 1x1x1-Wuerfel"). Reine Daten, wie block_uvs.rs: das
// Aufloesen aus den echten Vanilla-Modell-/Blockstate-JSONs liegt im assets-Crate
// (assets::shapes), gebaut wird die Tabelle in der App. So bleiben Mesher, Kollision und
// Physik frei von Asset-Abhaengigkeiten.

// Eine achsenparallele Box in Block-lokalen Einheiten (0..1). `faces` sagt, ob diese Box
// fuer die jeweilige Richtung (Index wie assets::FACE_*: down,up,north,south,west,east)
// ueberhaupt eine Flaeche hat -- ein Zaunpfosten laesst z.B. die Seite weg, an der ein Arm
// andockt, weil sie nie sichtbar ist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub faces: [bool; 6],
    // Ob die jeweilige Flaeche eine eigenstaendige Inhaltstextur braucht statt der
    // gewoehnlichen Block-Flaechentextur (z.B. Kesselinhalt: Wasser/Lava/Pulverschnee auf
    // der Oben-Flaeche eines separaten, kleineren Elements) -- s. assets::ElementBox::content
    // und world::BlockUvs::content_rect.
    pub content: [bool; 6],
    // Texturausschnitt (0..1 normiert, aus dem Modell-JSON "uv":[x0,y0,x1,y1] / 16.0) INNER-
    // HALB der Flaechen-Kachel -- None = volle Kachel. Sehr haeufig bei Nicht-Vollwuerfel-
    // Formen (Stufen, Treppen, Falllaub, ...), damit die Textur nicht ueber die kleinere
    // Flaeche gestreckt wird, sondern nur der passende Ausschnitt erscheint (s.
    // assets::ElementBox::uv und world::mesher::crop_face_uv). Werte koennen invertiert sein
    // (u0>u1 bzw. v0>v1) -- das ist ein bewusster Vanilla-Spiegel-Trick (z.B. eine Unterseite,
    // die denselben Ausschnitt gespiegelt zeigt), keine ungueltige Eingabe.
    pub uv: [Option<[f32; 4]>; 6],
}

pub(crate) const FULL_CUBE: ShapeBox = ShapeBox { min: [0.0, 0.0, 0.0], max: [1.0, 1.0, 1.0], faces: [true; 6], content: [false; 6], uv: [None; 6] };

// Ein fertiges Modell-Viereck fuer den Mesher (s. BlockShapes::model_quads): Ecken in
// Block-Lokalkoordinaten (0..1, darf darueber hinausragen -- Feuer ist 1,4 hoch), je Ecke die
// Atlas-UV und eine feste Helligkeit (Richtungslicht, oder 1.0 bei "shade": false).
// Eckreihenfolge wie assets::ModelQuad: von aussen gesehen links oben, rechts oben, rechts
// unten, links unten.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelFace {
    pub corners: [[f32; 3]; 4],
    pub uvs: [[f32; 2]; 4],
    pub shade: f32,
}

pub struct BlockShapes {
    // Je State die Modell-Vierecke, falls sein Modell gedrehte Elemente hat, die sich nicht
    // als Boxen darstellen lassen (Feuer, Laterne, ...). Leer = keine solchen States. Der
    // Mesher zeichnet sie statt der Boxen; Kollision/Umriss bleiben bei per_state.
    //
    // Je State eine oder mehrere gleichwertige Fassungen, die sich nur in der Textur-Kachel
    // unterscheiden (Feuer mit Zeitversatz, s. assets::FIRE_VARIANTS). Welche ein Block
    // bekommt, entscheidet seine Position (s. model_quads_at). [0] ist die Grundfassung.
    model_quads: Vec<Option<Vec<Vec<ModelFace>>>>,
    // Je State die AO-Eigenschaften (s. ambient_occlusion::AO_*). Leer = noch nicht
    // berechnet; dann rechnet ao_flags sie im Vorbeigehen aus der Form aus (langsamer, aber
    // fuer Tests und den Vollwuerfel-Stub korrekt).
    ao: Vec<u8>,
    // Pro State-ID: None = keine Form aufgeloest (unbekannt, oder ein nicht darstellbares
    // Modell wie die Kreuz-Ebenen von Blumen/Gras) -> Vollwuerfel-Fallback. Some(leer) kommt
    // in der Praxis nicht vor, wird aber genauso behandelt wie eine echte leere Liste
    // (keine Boxen -- weder sichtbar noch kollidierbar).
    per_state: Vec<Option<Vec<ShapeBox>>>,
    // Je State: versperrt er die Sicht ganz (s. visibility::blocks_view)? Beim ersten Zugriff
    // berechnet, danach nur noch nachgeschlagen -- die Mesh-Worker fragen das fuer jede Zelle.
    view_blockers: std::sync::OnceLock<Vec<bool>>,
}

impl BlockShapes {
    pub fn new(per_state: Vec<Option<Vec<ShapeBox>>>) -> Self {
        Self { per_state, ao: Vec::new(), model_quads: Vec::new(), view_blockers: std::sync::OnceLock::new() }
    }

    pub fn blocks_view(&self, state: u32) -> bool {
        let table = self.view_blockers.get_or_init(|| {
            (0..crate::block_registry::state_count() as u32).map(|s| crate::visibility::blocks_view(self, s)).collect()
        });
        table.get(state as usize).copied().unwrap_or(false)
    }

    // VOR build_ao_flags setzen: ein State mit Modell-Vierecken zaehlt nie als Vollwuerfel.
    pub fn set_model_quads(&mut self, per_state: Vec<Option<Vec<Vec<ModelFace>>>>) {
        self.model_quads = per_state;
        self.view_blockers = std::sync::OnceLock::new();
    }

    pub fn model_quads(&self, state: u32) -> Option<&[ModelFace]> {
        self.model_quads.get(state as usize).and_then(|q| q.as_ref()).and_then(|v| v.first()).map(|v| v.as_slice())
    }

    // Die Fassung fuer den Block an (x, y, z): fest je Position, zwischen Nachbarn gestreut.
    pub fn model_quads_at(&self, state: u32, x: i32, y: i32, z: i32) -> Option<&[ModelFace]> {
        let variants = self.model_quads.get(state as usize)?.as_ref()?;
        if variants.len() <= 1 {
            return variants.first().map(|v| v.as_slice());
        }
        // Vanillas MathHelper.hashCode(x, y, z), damit benachbarte Bloecke nicht gleich ausfallen.
        let mut h = (x as i64).wrapping_mul(3129871) ^ (z as i64).wrapping_mul(116129781) ^ (y as i64);
        h = h.wrapping_mul(h).wrapping_mul(42317861).wrapping_add(h.wrapping_mul(11));
        let index = ((h >> 16) as u64 % variants.len() as u64) as usize;
        Some(variants[index].as_slice())
    }

    // Die AO-Eigenschaften aller States einmal vorberechnen (s. ambient_occlusion). Der Mesher
    // schlaegt sie fuer jede sichtbare Flaeche bis zu zwoelfmal nach -- aus der Kollisionsform
    // jedes Mal neu ausgerechnet, kostete das pro Spalte Zehntausende Allokationen.
    //
    // `ao_disabled` sind die States, deren Modell "ambientocclusion": false traegt (s.
    // assets::build_ao_disabled).
    pub fn build_ao_flags(&mut self, ao_disabled: &std::collections::HashSet<u32>) {
        let count = crate::block_registry::state_count() as u32;
        let ao = (0..count).map(|state| self.compute_ao_flags(state, ao_disabled.contains(&state))).collect();
        self.ao = ao;
    }

    fn compute_ao_flags(&self, state: u32, model_disables: bool) -> u8 {
        use crate::ambient_occlusion::{ao_darkens, blocks_vision, emits_light, AO_BLOCKS_VISION, AO_DARKENS, AO_FULL_CUBE, AO_SKIP};
        // isFullCube haengt in Vanilla an der KOLLISIONSform (Block.isShapeFullCube auf
        // getCollisionShape): Glas und Laub zaehlen also als Vollwuerfel, Wasser nicht.
        //
        // Vollwuerfel heisst: die VEREINIGUNG der Boxen ist die ganze Zelle -- nicht "genau
        // eine Box". Der Grasblock hat zwei deckungsgleiche Vollwuerfel-Elemente (Basis und
        // Seiten-Overlay) und galt mit der alten Pruefung nicht als Vollwuerfel: er warf
        // keinen AO-Schatten (Sand daneben schon). Genuegt: eine Box fuellt die Zelle, und keine ragt hinaus.
        let boxes = crate::collision::collision_boxes(state, self);
        let voll = |b: &ShapeBox| b.min.iter().all(|v| v.abs() < 1e-4) && b.max.iter().all(|v| (v - 1.0).abs() < 1e-4);
        let innen = |b: &ShapeBox| b.min.iter().all(|&v| v >= -1e-4) && b.max.iter().all(|&v| v <= 1.0 + 1e-4);
        // Ein State mit Modell-Vierecken (s. model_quads) ist kein Vollwuerfel, auch wenn
        // seine Kollision mangels aufgeloester Form auf den Wuerfel-Notnagel faellt -- sonst
        // warfe eine Laterne den AO-Schatten eines ganzen Blocks. AUSSER alle Vierecke liegen
        // auf der Zellgrenze: Pilzbloecke sind echte Wuerfel, nur aus sechs Einzelflaechen mit
        // eigenen Texturen gebaut -- sie warfen sonst keinen Schatten.
        let wuerfelflaechen = self.model_quads(state).is_none_or(|q| q.iter().all(|f| crate::mesher::boundary_quad(&f.corners).is_some()));
        let full = boxes.iter().any(voll) && boxes.iter().all(innen) && wuerfelflaechen;
        let mut flags = 0;
        if full {
            flags |= AO_FULL_CUBE;
        }
        if state != crate::chunk::STATE_AIR && ao_darkens(state, full) {
            flags |= AO_DARKENS;
        }
        if state != crate::chunk::STATE_AIR && blocks_vision(state, full) {
            flags |= AO_BLOCKS_VISION;
        }
        if model_disables || emits_light(state) {
            flags |= AO_SKIP;
        }
        flags
    }

    pub fn ao_flags(&self, state: u32) -> u8 {
        match self.ao.get(state as usize) {
            Some(&f) => f,
            // Ausserhalb der Tabelle (Sentinel unterhalb der Welt) oder nicht vorberechnet.
            None if state == u32::MAX => {
                crate::ambient_occlusion::AO_FULL_CUBE | crate::ambient_occlusion::AO_BLOCKS_VISION | crate::ambient_occlusion::AO_DARKENS
            }
            None => self.compute_ao_flags(state, false),
        }
    }

    // Immer Vollwuerfel fuer jeden State -- fuer Tests und Aufrufer ohne echte Formdaten
    // (das bisherige Verhalten, bevor es diese Tabelle gab).
    pub fn all_full_cubes() -> Self {
        Self { per_state: Vec::new(), ao: Vec::new(), model_quads: Vec::new(), view_blockers: std::sync::OnceLock::new() }
    }

    // Ob fuer diesen State eine ECHTE Form aus dem Modell aufgeloest werden konnte. false
    // heisst: es gibt kein brauchbares Modell, shapes_of liefert nur den Vollwuerfel-Notnagel.
    //
    // Das ist NICHT dasselbe wie "ist ein Vollwuerfel": Stein hat ein Modell, das einen
    // Vollwuerfel beschreibt (true). false bekommen die Bloecke, die Vanilla ueberhaupt nicht
    // aus einem Blockmodell zeichnet -- BlockEntities wie Betten, Truhen, Schilder, Banner,
    // Koepfe (272 Blocknamen, s. app::tests/block_shape_audit). Fuer die darf der Vollwuerfel
    // NIE zum Verdecken von Nachbarflaechen benutzt werden, sonst fehlt der Boden unter einem
    // Bett und man schaut durch die Welt.
    //
    // all_full_cubes() (Tests, Aufrufer ohne Formdaten) meint den Vollwuerfel ausdruecklich --
    // dort ist die Antwort deshalb true.
    pub fn has_resolved_shape(&self, state: u32) -> bool {
        if self.per_state.is_empty() {
            return true;
        }
        matches!(self.per_state.get(state as usize), Some(Some(_)))
    }

    pub fn shapes_of(&self, state: u32) -> &[ShapeBox] {
        match self.per_state.get(state as usize) {
            Some(Some(boxes)) => boxes,
            _ => std::slice::from_ref(&FULL_CUBE),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbekannter_state_ist_vollwuerfel() {
        let shapes = BlockShapes::new(vec![Some(vec![ShapeBox {
            min: [0.0; 3],
            max: [0.5, 1.0, 1.0],
            faces: [true; 6],
            content: [false; 6],
            uv: [None; 6],
        }])]);
        assert_eq!(shapes.shapes_of(0)[0].max, [0.5, 1.0, 1.0]);
        assert_eq!(shapes.shapes_of(99), &[FULL_CUBE]);
    }

    #[test]
    fn all_full_cubes_liefert_immer_vollwuerfel() {
        let shapes = BlockShapes::all_full_cubes();
        assert_eq!(shapes.shapes_of(5), &[FULL_CUBE]);
    }
}

#[cfg(test)]
mod ao_vollwuerfel_tests {
    use super::*;
    use crate::ambient_occlusion::{AO_DARKENS, AO_FULL_CUBE};

    // Der Grasblock besteht aus zwei deckungsgleichen Vollwuerfel-Elementen (Basis und
    // Seiten-Overlay) -- er ist trotzdem EIN Vollwuerfel und muss seine Nachbarn abdunkeln.
    #[test]
    fn zwei_deckungsgleiche_vollwuerfel_sind_ein_vollwuerfel() {
        let gras = (0..crate::block_registry::state_count() as u32).find(|&s| crate::block_registry::block_name(s) == Some("grass_block")).unwrap();
        let mut per_state = vec![None; crate::block_registry::state_count()];
        per_state[gras as usize] = Some(vec![FULL_CUBE, FULL_CUBE]);
        let mut shapes = BlockShapes::new(per_state);
        shapes.build_ao_flags(&std::collections::HashSet::new());
        let flags = shapes.ao_flags(gras);
        assert!(flags & AO_FULL_CUBE != 0, "Gras zaehlt nicht als Vollwuerfel");
        assert!(flags & AO_DARKENS != 0, "Gras dunkelt seine Nachbarn nicht ab");
    }
}
