// Nachschlagetabelle State-ID -> sechs Atlas-UV-Rechtecke (eine pro Wuerfelseite), die der
// Mesher pro Flaeche abfragt. Reine Daten: der Atlas selbst (die GPU-Textur) und das
// Aufloesen der Modelle liegen im assets-Crate, gebaut wird die Tabelle in der App aus
// beidem (atlas.uvs je Blockname + block_registry state->name). So bleibt der Mesher frei
// von Asset-Abhaengigkeiten und laeuft weiter als reine Funktion auf Worker-Threads.

// UV-Rechteck (u0, v0, u1, v1), normiert 0..1.
pub type UvRect = [f32; 4];

pub struct BlockUvs {
    // Pro State-ID die sechs Flaechen-UVs (Index wie assets::FACE_*).
    per_state: Vec<[UvRect; 6]>,
    // Pro State-ID die sechs Inhaltsflaechen-UVs, falls vorhanden (z.B. Kesselinhalt --
    // s. assets::shapes::ElementBox::content). Leer fuer Aufrufer ohne echte Atlas-Daten.
    content_per_state: Vec<[Option<UvRect>; 6]>,
    // Pro State-ID die sechs Overlay-Flaechen-UVs, falls vorhanden (z.B. grass_block-Seiten
    // -- s. assets::BlockAtlas::overlay_uvs-Feldkommentar). Der Mesher zeichnet sie als
    // zweite, per Biom eingefaerbte Geometrie-Ebene ueber die Translucent-Pipeline. Leer
    // fuer Aufrufer ohne echte Atlas-Daten.
    overlay_per_state: Vec<[Option<UvRect>; 6]>,
    // Pro State-ID die alternativen Flaechen-UV-Saetze fuer Vanillas pseudozufaellige
    // Block-Rotation (Erde/Sand/Stein/Grasblock/..., s. world::render_rng-Modulkommentar) --
    // ein leerer/1-elementiger Eintrag bedeutet "kein RNG fuer diesen State", per_state
    // reicht dann. Index 0 (falls vorhanden) ist IMMER identisch mit per_state[state]
    // (dieselbe kanonische, unrotierte Zuordnung aus first_model()). Leer fuer Aufrufer
    // ohne echte Atlas-Daten.
    rng_variants_per_state: Vec<Vec<[UvRect; 6]>>,
    // Laufzeit-Umschalter (Settings::block_rng_rotation) -- AtomicBool statt eines
    // gewoehnlichen bool, weil BlockUvs hinter einem gemeinsamen Arc an die Mesh-
    // Worker-Threads gereicht wird (s. Modulkommentar); ein Klick auf den Schalter im
    // Menue muss den Wert AENDERN koennen, ohne das ganze BlockUvs neu zu bauen und den
    // Arc bei allen Haltern auszutauschen. Relaxed reicht: es gibt keine andere Variable,
    // deren Reihenfolge dazu synchronisiert werden muesste, nur ein einzelnes Flag.
    rng_rotation_enabled: std::sync::atomic::AtomicBool,
    // UV der Wassertextur, unabhaengig von einer State-ID -- braucht der Mesher fuer die
    // Wasserfuellung waterlogged-Bloecke (s. mesher::push_waterlogged_fill): DIESE Bloecke
    // sind selbst z.B. ein Zaun, nicht "water", haben also kein eigenes Wasser-UV in
    // per_state. Gleich fuer alle Flaechen (Wasser hat keine Ausrichtung).
    water_uv: [UvRect; 6],
    // UV der FLIESS-Wassertextur (water_flow), falls im Atlas vorhanden. Der Mesher waehlt
    // pro Zelle zwischen dieser und water_uv (s. mesher::push_fluid_quads) und dreht sie in
    // Fliessrichtung. None = kein water_flow im Atlas -> ueberall die Standtextur, wie bisher.
    water_flow_uv: Option<UvRect>,
    // UV von water_overlay -- die flache Textur fuer Wasserseiten an Glas/Laub, s.
    // mesher::push_fluid_quads. None = nicht im Atlas -> es bleibt bei der Standtextur.
    water_overlay_uv: Option<UvRect>,
    // UV der FLIESS-Lavatextur (lava_flow), falls im Atlas vorhanden -- das Gegenstueck zu
    // water_flow_uv fuer den zweiten Fluessigkeitstyp. Die RUHIGE Lavatextur braucht kein
    // eigenes Feld: anders als bei der Wasserfuellung waterlogged-Bloecke gibt es Lava nur
    // als echten "lava"-Block, ihr Stand-Sprite haengt also ganz normal an der State-ID
    // (face_rect). None = kein lava_flow im Atlas -> ueberall die Standtextur.
    lava_flow_uv: Option<UvRect>,
    // UVs der zehn Riss-Stufen beim Abbauen (Index = Stufe 0..9, s.
    // assets::DESTROY_STAGE_COUNT). Haengt an keiner State-ID: die Riss-Textur ist fuer jeden
    // Block dieselbe, nur die Stufe wechselt. None = Textur fehlt im Atlas.
    destroy_stage_uvs: [Option<UvRect>; 10],
    // Fuer States jenseits der Tabelle (unbekannt/neuer als der Backvorgang).
    missing: [UvRect; 6],
    // Zeitversetzte Kopien animierter Kacheln (s. assets::ANIM_VARIANTS): je Atlas-Kachel
    // (Index Zeile * anim_grid + Spalte) die UV-Verschiebungen zu ihren Kopien, leer fuer
    // nicht animierte. Leer insgesamt = keine Versetzung (Tests, Aufrufer ohne Atlas).
    anim_shifts: Vec<Box<[[f32; 2]]>>,
    anim_grid: u32,
}

impl BlockUvs {
    pub fn new(per_state: Vec<[UvRect; 6]>, missing: [UvRect; 6]) -> Self {
        Self {
            per_state,
            content_per_state: Vec::new(),
            overlay_per_state: Vec::new(),
            rng_variants_per_state: Vec::new(),
            rng_rotation_enabled: std::sync::atomic::AtomicBool::new(true),
            water_uv: [missing[0]; 6],
            water_flow_uv: None,
            water_overlay_uv: None,
            lava_flow_uv: None,
            destroy_stage_uvs: [None; 10],
            missing,
            anim_shifts: Vec::new(),
            anim_grid: 0,
        }
    }

    pub fn with_content(
        per_state: Vec<[UvRect; 6]>,
        content_per_state: Vec<[Option<UvRect>; 6]>,
        water_uv: [UvRect; 6],
        missing: [UvRect; 6],
    ) -> Self {
        Self {
            per_state,
            content_per_state,
            overlay_per_state: Vec::new(),
            rng_variants_per_state: Vec::new(),
            rng_rotation_enabled: std::sync::atomic::AtomicBool::new(true),
            water_uv,
            water_flow_uv: None,
            water_overlay_uv: None,
            lava_flow_uv: None,
            destroy_stage_uvs: [None; 10],
            missing,
            anim_shifts: Vec::new(),
            anim_grid: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_content_and_overlay(
        per_state: Vec<[UvRect; 6]>,
        content_per_state: Vec<[Option<UvRect>; 6]>,
        overlay_per_state: Vec<[Option<UvRect>; 6]>,
        rng_variants_per_state: Vec<Vec<[UvRect; 6]>>,
        water_uv: [UvRect; 6],
        missing: [UvRect; 6],
    ) -> Self {
        Self {
            per_state,
            content_per_state,
            overlay_per_state,
            rng_variants_per_state,
            rng_rotation_enabled: std::sync::atomic::AtomicBool::new(true),
            water_uv,
            water_flow_uv: None,
            water_overlay_uv: None,
            lava_flow_uv: None,
            destroy_stage_uvs: [None; 10],
            missing,
            anim_shifts: Vec::new(),
            anim_grid: 0,
        }
    }

    // Ein einheitliches UV fuer alle States/Flaechen -- fuer Tests und Aufrufer ohne echten
    // Atlas (die Mesh-Geometrie stimmt, nur die Textur ist einheitlich).
    pub fn single_tile() -> Self {
        let tile = [[0.0, 0.0, 1.0, 1.0]; 6];
        Self {
            per_state: Vec::new(),
            content_per_state: Vec::new(),
            overlay_per_state: Vec::new(),
            rng_variants_per_state: Vec::new(),
            rng_rotation_enabled: std::sync::atomic::AtomicBool::new(true),
            water_uv: tile,
            water_flow_uv: None,
            water_overlay_uv: None,
            lava_flow_uv: None,
            destroy_stage_uvs: [None; 10],
            missing: tile,
            anim_shifts: Vec::new(),
            anim_grid: 0,
        }
    }

    pub fn face_rect(&self, state: u32, face: usize) -> UvRect {
        self.per_state.get(state as usize).map(|f| f[face]).unwrap_or(self.missing[face])
    }

    // face_rect(), aber mit Vanillas pseudozufaelliger Block-Rotation (s. world::
    // render_rng-Modulkommentar), falls fuer diesen State Varianten hinterlegt sind UND der
    // Umschalter an ist -- sonst identisch zu face_rect(). x/y/z sind Weltkoordinaten (nicht
    // Chunk-lokal), gehen 1:1 in die Vanilla-Seed-Formel ein.
    pub fn face_rect_rng(&self, state: u32, face: usize, x: i32, y: i32, z: i32) -> UvRect {
        if self.rng_rotation_enabled.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(variants) = self.rng_variants_per_state.get(state as usize) {
                if variants.len() > 1 {
                    let idx = crate::render_rng::pick_variant(x, y, z, variants.len());
                    return variants[idx][face];
                }
            }
        }
        self.face_rect(state, face)
    }

    pub fn set_rng_rotation_enabled(&self, enabled: bool) {
        self.rng_rotation_enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    // UV der Inhaltsflaeche (z.B. Kesselinhalt), falls dieser State/diese Richtung eine
    // hat -- sonst None (der Aufrufer faellt dann auf face_rect zurueck).
    pub fn content_rect(&self, state: u32, face: usize) -> Option<UvRect> {
        self.content_per_state.get(state as usize).and_then(|f| f[face])
    }

    // UV der Overlay-Flaeche (z.B. grass_block-Seiten), falls dieser State/diese Richtung
    // eine hat -- sonst None (der Aufrufer zeichnet dann keine zweite Geometrie-Ebene).
    pub fn overlay_rect(&self, state: u32, face: usize) -> Option<UvRect> {
        self.overlay_per_state.get(state as usize).and_then(|f| f[face])
    }

    // `tiles`: je animierter Basiskachel die Kachelindizes ihrer Kopien, `grid` = Kacheln je
    // Atlaszeile (s. assets::BlockAtlas::anim_variant_tiles).
    pub fn with_anim_variants(mut self, grid: u32, tiles: &[(u32, Vec<u32>)]) -> Self {
        if grid == 0 || tiles.is_empty() {
            return self;
        }
        let s = 1.0 / grid as f32;
        let pos = |i: u32| [(i % grid) as f32 * s, (i / grid) as f32 * s];
        let mut shifts: Vec<Box<[[f32; 2]]>> = vec![Box::new([]); (grid * grid) as usize];
        for (base, copies) in tiles {
            let b = pos(*base);
            if let Some(slot) = shifts.get_mut(*base as usize) {
                *slot = copies.iter().map(|&c| pos(c)).map(|p| [p[0] - b[0], p[1] - b[1]]).collect();
            }
        }
        self.anim_shifts = shifts;
        self.anim_grid = grid;
        self
    }

    pub fn has_anim_variants(&self) -> bool {
        !self.anim_shifts.is_empty()
    }

    // Verschiebungen zu den Kopien der Kachel, in der `uv` liegt -- leer, wenn sie nicht
    // animiert ist. `uv` sollte INNERHALB der Kachel liegen (z.B. ein Dreiecksschwerpunkt):
    // eine Ecke auf der Kachelgrenze gehoert rechnerisch schon zur Nachbarkachel.
    pub fn anim_shifts_at(&self, uv: [f32; 2]) -> &[[f32; 2]] {
        let g = self.anim_grid as f32;
        let (tx, ty) = ((uv[0] * g) as i64, (uv[1] * g) as i64);
        if tx < 0 || ty < 0 || tx >= self.anim_grid as i64 || ty >= self.anim_grid as i64 {
            return &[];
        }
        self.anim_shifts.get((ty * self.anim_grid as i64 + tx) as usize).map_or(&[], |b| b)
    }

    pub fn water_rect(&self, face: usize) -> UvRect {
        self.water_uv[face]
    }

    // Bewusst ein Builder statt eines fuenften Konstruktor-Parameters: die Fliesstextur ist
    // optional (fehlt sie, sieht Wasser aus wie bisher), und alle vier bestehenden
    // Konstruktoren haetten sie sonst durchreichen muessen, ohne sie zu brauchen.
    pub fn with_water_flow(mut self, uv: Option<UvRect>) -> Self {
        self.water_flow_uv = uv;
        self
    }

    pub fn with_water_overlay(mut self, uv: Option<UvRect>) -> Self {
        self.water_overlay_uv = uv;
        self
    }

    // None = keine Overlay-Textur verfuegbar; der Aufrufer nimmt dann water_rect().
    pub fn water_overlay_rect(&self) -> Option<UvRect> {
        self.water_overlay_uv
    }

    // None = keine Fliesstextur verfuegbar; der Aufrufer nimmt dann water_rect().
    pub fn water_flow_rect(&self) -> Option<UvRect> {
        self.water_flow_uv
    }

    pub fn with_lava_flow(mut self, uv: Option<UvRect>) -> Self {
        self.lava_flow_uv = uv;
        self
    }

    // None = keine Lava-Fliesstextur verfuegbar; der Aufrufer nimmt dann das Stand-Sprite
    // des Lava-States (face_rect).
    pub fn lava_flow_rect(&self) -> Option<UvRect> {
        self.lava_flow_uv
    }

    pub fn with_destroy_stages(mut self, uvs: [Option<UvRect>; 10]) -> Self {
        self.destroy_stage_uvs = uvs;
        self
    }

    // UV der Riss-Textur fuer diese Abbaustufe (0..9). None = Stufe ausserhalb oder Textur
    // fehlt im Atlas -- der Aufrufer zeichnet dann keine Risse.
    pub fn destroy_stage_rect(&self, stage: usize) -> Option<UvRect> {
        self.destroy_stage_uvs.get(stage).copied().flatten()
    }
}
