// Wie glatt und metallisch ein Block ist -- fuer Spiegelungen im Shader-Pfad (s.
// render::deferred_light.wgsl, fs_reflexion). Alles, was hier fehlt, ist matt: es streut
// Licht nur (wie bisher) und spiegelt nichts.
//
// Metalle spiegeln in ihrer eigenen Farbe (die ihrer Textur) und streuen kein Licht; alles
// andere spiegelt farblos und nur wenig, senkrecht so viel wie `reflexion` sagt, flach
// darueber mehr (Fresnel). Die Werte folgen aus der Physik, nicht aus einer Vorlage:
// Grundreflexion eines Nichtmetalls ((n - 1) / (n + 1))^2 aus seiner Brechzahl n (Eis 1,31,
// Quarz und Glas um 1,5, Diamant 2,42), die Rauheit nach Augenmass fuer Minecrafts Texturen.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    // 0 = Nichtmetall, 1 = Metall (dazwischen: angelaufenes Kupfer).
    pub metall: f32,
    // Rauheit 0 (Spiegel) .. 1 (matt), wahrgenommen (GGX-Alpha = Rauheit^2).
    pub rauheit: f32,
    // Reflexion eines Nichtmetalls senkrecht von vorn (F0).
    pub reflexion: f32,
}

const fn m(metall: f32, rauheit: f32, reflexion: f32) -> Option<Material> {
    Some(Material { metall, rauheit, reflexion })
}

// Kupfer je Stufe der Patina: blank, angelaufen, verwittert, oxidiert.
fn kupfer(name: &str) -> Option<Material> {
    let rest = name.strip_prefix("waxed_").unwrap_or(name);
    let (stufe, art) = if let Some(r) = rest.strip_prefix("exposed_") {
        (1, r)
    } else if let Some(r) = rest.strip_prefix("weathered_") {
        (2, r)
    } else if let Some(r) = rest.strip_prefix("oxidized_") {
        (3, r)
    } else {
        (0, rest)
    };
    let kupfern = matches!(art, "copper_block" | "copper" | "cut_copper" | "cut_copper_slab" | "cut_copper_stairs" | "chiseled_copper" | "copper_grate" | "copper_door" | "copper_trapdoor" | "copper_bulb" | "lightning_rod")
        || art.starts_with("copper_") && !art.contains("ore");
    if !kupfern {
        return None;
    }
    match stufe {
        0 => m(1.0, 0.4, 0.04),
        1 => m(0.7, 0.5, 0.04),
        2 => m(0.35, 0.65, 0.04),
        _ => None,
    }
}

pub fn material(name: &str) -> Option<Material> {
    if let Some(k) = kupfer(name) {
        return Some(k);
    }
    match name {
        // Metalle.
        "iron_block" => m(1.0, 0.3, 0.04),
        "gold_block" => m(1.0, 0.25, 0.04),
        "netherite_block" => m(1.0, 0.45, 0.04),
        "iron_bars" | "chain" | "iron_chain" | "anvil" | "chipped_anvil" | "damaged_anvil" | "cauldron" | "water_cauldron" | "lava_cauldron" | "powder_snow_cauldron" | "hopper" | "iron_door"
        | "iron_trapdoor" | "heavy_weighted_pressure_plate" | "rail" | "powered_rail" | "detector_rail" | "activator_rail" | "lantern" | "soul_lantern" => m(1.0, 0.45, 0.04),
        "light_weighted_pressure_plate" | "bell" => m(1.0, 0.3, 0.04),
        // Glatte Nichtmetalle.
        "ice" | "packed_ice" | "blue_ice" => m(0.0, 0.08, 0.018),
        "obsidian" | "crying_obsidian" => m(0.0, 0.15, 0.04),
        "diamond_block" => m(0.0, 0.1, 0.17),
        "emerald_block" => m(0.0, 0.15, 0.05),
        "amethyst_block" | "budding_amethyst" => m(0.0, 0.3, 0.045),
        "quartz_block" | "smooth_quartz" | "quartz_pillar" | "chiseled_quartz_block" | "quartz_bricks" | "quartz_slab" | "quartz_stairs" | "smooth_quartz_slab" | "smooth_quartz_stairs" => {
            m(0.0, 0.3, 0.045)
        }
        n if n.ends_with("_glazed_terracotta") => m(0.0, 0.15, 0.04),
        n if n.starts_with("polished_") && !n.contains("button") && !n.contains("pressure_plate") => m(0.0, 0.35, 0.04),
        n if n.ends_with("_concrete") => m(0.0, 0.55, 0.04),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metalle_und_matte_bloecke() {
        assert_eq!(material("iron_block").map(|m| m.metall), Some(1.0));
        assert_eq!(material("stone"), None);
        assert_eq!(material("dirt"), None);
        assert!(material("waxed_exposed_cut_copper").is_some_and(|m| m.metall > 0.5 && m.metall < 1.0));
        assert_eq!(material("oxidized_copper"), None);
        assert_eq!(material("copper_ore"), None);
        assert!(material("white_glazed_terracotta").is_some_and(|m| m.metall == 0.0 && m.rauheit < 0.3));
        assert!(material("polished_andesite").is_some());
        assert_eq!(material("polished_blackstone_button"), None);
    }
}
