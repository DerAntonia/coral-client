// Welche Bloecke Gras-/Laub-Faerbung (Biom-Colormap) bekommen, und mit welcher der beiden
// Karten (grass.png vs. foliage.png -- zwei unterschiedliche Farbverlaeufe in Vanilla).
// Wasser wird NICHT hierueber behandelt (eigene, direkte Biom-Farbe ohne Colormap, s.
// mesher.rs).
//
// Datenquelle fuer die Laubliste: der Vanilla-Datengenerator klassifiziert Blaetter explizit
// in "tinted_particle_leaves" (6 Bloecke: Akazie/Birke/Schwarzeiche/Dschungel/Eiche/Fichte)
// vs. "untinted_particle_leaves" (4 Bloecke: Azalee/Kirsche/Blueten-Azalee/Blass-Eiche, s.
// blocks.json-Report) -- keine Vermutung, echte Daten. Gras-Pflanzen (Cross-Modelle) werden
// als Vollwuerfel gerendert (s. assets::shapes-Kommentar zu Element::rotation) und deshalb
// hier komplett (alle Seiten) statt nur eine Fläche getoent.
//
// Bewusst NICHT abgebildet: die Handvoll Bloecke mit einer FESTEN statt biomeabhaengigen
// Farbe (z.B. Sumpf-spezifische Ranken-Sonderfaerbung, Redstone-Wire-Faerbung nach
// Staerke) -- kein Colormap-Fall, andere Mechanik.
pub enum TintKind {
    None,
    // Nur die Oben-Flaeche (grass_block: Seiten bleiben braun/Erdfarben, wie ein
    // unaufwaendiger, aber korrekter Vanilla-Look ohne die duenne Ueberlagerungs-Textur an
    // den Seiten).
    GrassTop,
    // Alle Flaechen (Gras-/Farn-Pflanzen als Vollwuerfel-Fallback, s.o.).
    GrassAll,
    FoliageAll,
    // Eigene dritte Colormap (dry_foliage.png, seit 1.21.2/"Winter Drop") -- Falllaub-
    // Bodenbedeckung (leaf_litter). Deren Textur ist bewusst reines Graustufenbild (jeder
    // Pixel R=G=B, per PIL gegen die echte leaf_litter.png verifiziert), macht ohne diesen
    // Tint immer denselben grauen Eindruck, unabhaengig vom Biom.
    DryFoliageAll,
    // Feste Farbe 0xRRGGBB auf allen Flaechen, unabhaengig vom Biom: Fichten- und Birkenlaub,
    // Seerose (BlockColors gibt ihr in der Welt 0x208030, ohne Welt 0x71C35C).
    Fixed(u32),
}

pub fn tint_kind(block_name: &str) -> TintKind {
    match block_name {
        "grass_block" => TintKind::GrassTop,
        // Busch und Zuckerrohr faerbt Vanillas BlockColors ebenfalls mit der Grasfarbe
        // (ihre Textur ist grau, ohne Tint haetten sie keine Farbe).
        "short_grass" | "fern" | "tall_grass" | "large_fern" | "bush" | "sugar_cane" => TintKind::GrassAll,
        "acacia_leaves" | "dark_oak_leaves" | "jungle_leaves" | "oak_leaves" | "mangrove_leaves" | "vine" | "glow_lichen" => TintKind::FoliageAll,
        // Fichte und Birke faerbt Vanilla fest, nicht nach Biom (BlockColors).
        "spruce_leaves" => TintKind::Fixed(0x619961),
        "birch_leaves" => TintKind::Fixed(0x80A755),
        "leaf_litter" => TintKind::DryFoliageAll,
        "lily_pad" => TintKind::Fixed(0x208030),
        _ => TintKind::None,
    }
}

// Wasser (kein Colormap-Fall, direkte Biom-Farbe) -- eigener Name statt einer weiteren
// TintKind-Variante, damit der Mesher Wasser klar getrennt von der Colormap-Logik behandeln
// kann (andere Datenquelle: BiomeColors::water statt grass/foliage).
pub fn is_water(block_name: &str) -> bool {
    block_name == "water" || block_name == "flowing_water"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bekannte_klassifikation_stimmt() {
        assert!(matches!(tint_kind("grass_block"), TintKind::GrassTop));
        assert!(matches!(tint_kind("oak_leaves"), TintKind::FoliageAll));
        assert!(matches!(tint_kind("spruce_leaves"), TintKind::Fixed(0x619961)), "Fichte: feste Farbe");
        assert!(matches!(tint_kind("birch_leaves"), TintKind::Fixed(0x80A755)), "Birke: feste Farbe");
        assert!(matches!(tint_kind("azalea_leaves"), TintKind::None), "Azalee hat eine feste Farbe, keine Biom-Toenung");
        assert!(matches!(tint_kind("stone"), TintKind::None));
        assert!(matches!(tint_kind("bush"), TintKind::GrassAll));
        assert!(matches!(tint_kind("lily_pad"), TintKind::Fixed(0x208030)), "Seerose: feste Farbe statt grauer Textur");
        assert!(is_water("water"));
        assert!(!is_water("stone"));
    }
}
