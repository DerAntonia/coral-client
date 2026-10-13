// Pro Biom-ID (Registry-Index, s. protocol::packets::BiomeInfo) die drei fuer Faerbung
// relevanten Farben: fertig aus der Colormap gesampelte Gras-/Laub-Farbe (s.
// assets::Colormap) und die direkte Wasserfarbe. Reine Laufzeit-Daten, wie BlockUvs/
// BlockShapes: das Sampling selbst (Colormap-Bild + Bioms Temperatur/Niederschlag) passiert
// beim Empfang der Registry-Data-Nachricht in der App, bevor diese Tabelle gebaut wird.

pub struct BiomeColors {
    pub grass: [f32; 3],
    pub foliage: [f32; 3],
    // Falllaub-Bodenbedeckung (leaf_litter, s. biome_tint::TintKind::DryFoliageAll) --
    // eigene dritte Colormap (dry_foliage.png), nicht dasselbe wie foliage (Laub-Baelle).
    pub dry_foliage: [f32; 3],
    pub water: [f32; 3],
}

// Vanilla-Standard (gemaessigtes Klima, Wasserblau 0x3F76E4) -- fuer unbekannte/fehlende
// Biom-IDs (z.B. bevor die Registry-Data ankommt, oder ein Server mit abweichender Biom-
// Liste) sicherer Fallback statt Absturz/Schwarz. dry_foliage bei denselben Plains-
// Klimawerten (temperature=0.8, downfall=0.4) aus dry_foliage.png gesampelt wie grass/
// foliage (per PIL gegen die echte Colormap verifiziert: Pixel (51,173) = (163,117,70)).
const FALLBACK: BiomeColors = BiomeColors {
    grass: [0.5686275, 0.7411765, 0.34901962],
    foliage: [0.46666667, 0.67058825, 0.18431373],
    dry_foliage: [0.6392157, 0.4588235, 0.2745098],
    water: [0.247, 0.4627, 0.8941],
};

pub struct BiomePalette {
    per_biome: Vec<BiomeColors>,
}

impl BiomePalette {
    pub fn new(per_biome: Vec<BiomeColors>) -> Self {
        Self { per_biome }
    }

    // Fallback-Palette (ein Eintrag, immer Vanilla-Standardfarben) -- fuer Tests und die
    // Zeit vor der ersten empfangenen Biom-Registry.
    pub fn fallback() -> Self {
        Self { per_biome: Vec::new() }
    }

    pub fn colors_of(&self, biome_id: u32) -> &BiomeColors {
        self.per_biome.get(biome_id as usize).unwrap_or(&FALLBACK)
    }
}
