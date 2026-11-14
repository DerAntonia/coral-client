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
// Farbe (z.B. Sumpf-spezifische Ranken-Sonderfaerbung) -- kein Colormap-Fall, andere
// Mechanik. Die Redstone-Leitung haengt am State (Signalstaerke), s. tint_kind_of_state.
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

#[must_use] 
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

// Wie tint_kind, dazu die Faerbung, die am State haengt: die Redstone-Leitung nach ihrer
// Signalstaerke (Property "power"). Ihre Overlay-Textur ist in Vanilla ganz durchsichtig, die
// Faerbung darf also alle Flaechen treffen.
#[must_use]
pub fn tint_kind_of_state(state: u32, block_name: &str) -> TintKind {
    if block_name == "redstone_wire" {
        let power = crate::state_props::property(state, "power").and_then(|p| p.parse::<u8>().ok()).unwrap_or(0);
        return TintKind::Fixed(redstone_wire_rgb(power));
    }
    tint_kind(block_name)
}

// Farbe der Redstone-Leitung je Signalstaerke 0..15 als 0xRRGGBB (RedStoneWireBlock): Rot
// waechst von 0,3 (aus) bzw. 0,4 linear bis 1,0; Gruen und Blau kommen erst bei hoher Staerke
// quadratisch dazu, darum wird sie zum Ende hin heller und gelblicher.
#[must_use]
pub fn redstone_wire_rgb(power: u8) -> u32 {
    let f = f32::from(power.min(15)) / 15.0;
    let rot = f * 0.6 + if f > 0.0 { 0.4 } else { 0.3 };
    let gruen = (f * f * 0.7 - 0.5).clamp(0.0, 1.0);
    let blau = (f * f * 0.6 - 0.7).clamp(0.0, 1.0);
    let kanal = |c: f32| (c * 255.0).floor() as u32;
    (kanal(rot) << 16) | (kanal(gruen) << 8) | kanal(blau)
}

// Wasser (kein Colormap-Fall, direkte Biom-Farbe) -- eigener Name statt einer weiteren
// TintKind-Variante, damit der Mesher Wasser klar getrennt von der Colormap-Logik behandeln
// kann (andere Datenquelle: BiomeColors::water statt grass/foliage).
#[must_use] 
pub fn is_water(block_name: &str) -> bool {
    block_name == "water" || block_name == "flowing_water"
}

#[cfg(test)]
mod redstone_tests {
    // Aus: dunkles Rot (0,3); volle Staerke: hell, mit etwas Gruen und Blau.
    #[test]
    fn redstone_farbe_nach_staerke() {
        assert_eq!(super::redstone_wire_rgb(0), 0x4C_00_00);
        assert_eq!(super::redstone_wire_rgb(15), 0xFF_32_00);
        assert!(super::redstone_wire_rgb(8) >> 16 > 0x4C, "heller als aus");
    }
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
