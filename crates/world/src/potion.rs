// Traenke: Anzeigename und Flaschenfarbe aus der potion_contents-Component.
//
// Beides steht in Vanilla nicht am Item, sondern am Stack: ein Trank ist IMMER das Item
// "potion", welcher es ist, sagt erst potion_contents. Ohne das hiess jeder Trank "potion"
// und hatte die blaue Grundfarbe aus items/potion.json.
//
// Die Tabellen sind aus dem 1.21.11-Jar gelesen (Potions, MobEffects, en_us.json). Beide
// Registries sind fest eingebaut (BuiltInRegistries), nicht vom Server synchronisiert -- die
// IDs sind also die Registrierreihenfolge.

use protocol::item::{ItemStack, PotionContents};

// Potions.java in Registrierreihenfolge: (Registry-Name, Namens-Schluessel Potion.name(),
// Effekte als (MobEffect-ID, Verstaerkung, Dauer in Ticks)). Die Langfassungen teilen sich den Schluessel
// mit der Grundform ("long_swiftness" heisst "Potion of Swiftness").
#[rustfmt::skip]
static POTIONS: [(&str, &str, &[(u32, i32, i32)]); 46] = [
    ("water", "water", &[]),
    ("mundane", "mundane", &[]),
    ("thick", "thick", &[]),
    ("awkward", "awkward", &[]),
    ("night_vision", "night_vision", &[(15, 0, 3600)]),
    ("long_night_vision", "night_vision", &[(15, 0, 9600)]),
    ("invisibility", "invisibility", &[(13, 0, 3600)]),
    ("long_invisibility", "invisibility", &[(13, 0, 9600)]),
    ("leaping", "leaping", &[(7, 0, 3600)]),
    ("long_leaping", "leaping", &[(7, 0, 9600)]),
    ("strong_leaping", "leaping", &[(7, 1, 1800)]),
    ("fire_resistance", "fire_resistance", &[(11, 0, 3600)]),
    ("long_fire_resistance", "fire_resistance", &[(11, 0, 9600)]),
    ("swiftness", "swiftness", &[(0, 0, 3600)]),
    ("long_swiftness", "swiftness", &[(0, 0, 9600)]),
    ("strong_swiftness", "swiftness", &[(0, 1, 1800)]),
    ("slowness", "slowness", &[(1, 0, 1800)]),
    ("long_slowness", "slowness", &[(1, 0, 4800)]),
    ("strong_slowness", "slowness", &[(1, 3, 400)]),
    ("turtle_master", "turtle_master", &[(1, 3, 400), (10, 2, 400)]),
    ("long_turtle_master", "turtle_master", &[(1, 3, 800), (10, 2, 800)]),
    ("strong_turtle_master", "turtle_master", &[(1, 5, 400), (10, 3, 400)]),
    ("water_breathing", "water_breathing", &[(12, 0, 3600)]),
    ("long_water_breathing", "water_breathing", &[(12, 0, 9600)]),
    ("healing", "healing", &[(5, 0, 1)]),
    ("strong_healing", "healing", &[(5, 1, 1)]),
    ("harming", "harming", &[(6, 0, 1)]),
    ("strong_harming", "harming", &[(6, 1, 1)]),
    ("poison", "poison", &[(18, 0, 900)]),
    ("long_poison", "poison", &[(18, 0, 1800)]),
    ("strong_poison", "poison", &[(18, 1, 432)]),
    ("regeneration", "regeneration", &[(9, 0, 900)]),
    ("long_regeneration", "regeneration", &[(9, 0, 1800)]),
    ("strong_regeneration", "regeneration", &[(9, 1, 450)]),
    ("strength", "strength", &[(4, 0, 3600)]),
    ("long_strength", "strength", &[(4, 0, 9600)]),
    ("strong_strength", "strength", &[(4, 1, 1800)]),
    ("weakness", "weakness", &[(17, 0, 1800)]),
    ("long_weakness", "weakness", &[(17, 0, 4800)]),
    ("luck", "luck", &[(25, 0, 6000)]),
    ("slow_falling", "slow_falling", &[(27, 0, 1800)]),
    ("long_slow_falling", "slow_falling", &[(27, 0, 4800)]),
    ("wind_charged", "wind_charged", &[(35, 0, 3600)]),
    ("weaving", "weaving", &[(36, 0, 3600)]),
    ("oozing", "oozing", &[(37, 0, 3600)]),
    ("infested", "infested", &[(38, 0, 3600)]),
];

// MobEffect.getColor() je Effekt-ID (Reihenfolge wie protocol::packets::MOB_EFFECTS).
#[rustfmt::skip]
static EFFECT_COLORS: [u32; 40] = [
    3402751, 9154528, 14270531, 4866583, 16762624, 16262179, 11101546, 16646020, 5578058,
    13458603, 9520880, 16750848, 10017472, 16185078, 2039587, 12779366, 5797459,
    4738376, 8889187, 7561558, 16284963, 2445989, 16262179, 9740385, 13565951, 5882118, 12624973,
    15978425, 1950417, 8954814, 745784, 4521796, 2696993, 1484454, 14565464,
    12438015, 7891290, 10092451, 9214860, 65518,
];

// MobEffect.getColor() eines Effekts; unbekannte (neuere) Effekte bekommen Schwarz.
pub fn effect_color(effect: i32) -> u32 {
    usize::try_from(effect).ok().and_then(|i| EFFECT_COLORS.get(i)).copied().unwrap_or(0)
}

// MobEffectCategory je Effekt-ID (MobEffects im Jar): nur BENEFICIAL zaehlt fuer
// MobEffect.isBeneficial -- die neutralen (Leuchten, die Omen) stehen in der HUD-Anzeige
// deshalb in der Reihe der schaedlichen.
#[rustfmt::skip]
static EFFECT_BENEFICIAL: [bool; 40] = [
    true, false, true, false, true, true, false, true, false,
    true, true, true, true, true, false, true, false,
    false, false, false, true, true, true, false, false, true, false,
    true, true, true, false, true, false, false, false,
    false, false, false, false, true,
];

pub fn effect_beneficial(effect: i32) -> bool {
    usize::try_from(effect).ok().and_then(|i| EFFECT_BENEFICIAL.get(i)).copied().unwrap_or(false)
}

// PotionContents.getAllEffects: die des Basistranks, dann die zusaetzlichen, je (Effekt,
// Verstaerkung, Dauer in Ticks; -1 = unendlich).
pub fn all_effects(contents: &PotionContents) -> Vec<(u32, i32, i32)> {
    let basis = contents.potion.and_then(|id| POTIONS.get(id as usize)).map(|p| p.2).unwrap_or(&[]);
    basis.iter().copied().chain(contents.custom_effects.iter().map(|e| (e.effect, e.amplifier, e.duration))).collect()
}

// potion_duration_scale: was der Stack mitbringt, sonst die Vorgabe des Items (Verweiltrank
// ein Viertel, getippter Pfeil ein Achtel der Trankdauer).
pub fn duration_scale(item: &str, stack: &ItemStack) -> f32 {
    stack.potion_duration_scale.unwrap_or(match item {
        "lingering_potion" => 0.25,
        "tipped_arrow" => 0.125,
        _ => 1.0,
    })
}

// Buchstabe in der Slotecke: S fuer verstaerkte, L fuer verlaengerte Traenke. Normale und
// eigene (mit Zusatzeffekten oder eigenem Namen) bekommen keinen.
//
// Heil- und Schadenstraenke umgekehrt: im PvP ist Stufe II der Normalfall, eine Reihe "S"
// saehe nur unruhig aus. Deshalb markiert dort W die SCHWACHE Stufe I, Stufe II bleibt frei.
pub fn variant_mark(stack: &ItemStack) -> Option<char> {
    let c = stack.potion.as_ref()?;
    if !c.custom_effects.is_empty() || c.custom_name.is_some() {
        return None;
    }
    let name = potion_registry_name(c.potion?)?;
    if name == "healing" || name == "harming" {
        Some('W')
    } else if name == "strong_healing" || name == "strong_harming" {
        None
    } else if name.starts_with("strong_") {
        Some('S')
    } else if name.starts_with("long_") {
        Some('L')
    } else {
        None
    }
}

// PotionContents.BASE_POTION_COLOR (-13083194): ein Trank ohne sichtbare Effekte.
pub const BASE_POTION_COLOR: u32 = 0x385DC6;

pub fn potion_registry_name(id: u32) -> Option<&'static str> {
    POTIONS.get(id as usize).map(|p| p.0)
}

// PotionContents.getColorOr: feste Farbe, sonst das mit (Verstaerkung + 1) gewichtete Mittel
// aller sichtbaren Effekte (getColorOptional, ganzzahlig geteilt wie in Vanilla), sonst None
// -- dann gilt der "default" der Toenungsquelle.
pub fn color(contents: &PotionContents) -> Option<u32> {
    if let Some(c) = contents.custom_color {
        return Some(c as u32 & 0xFFFFFF);
    }
    let basis = contents.potion.and_then(|id| POTIONS.get(id as usize)).map(|p| p.2).unwrap_or(&[]);
    let effekte = basis.iter().map(|&(e, a, _)| (e, a, true)).chain(contents.custom_effects.iter().map(|e| (e.effect, e.amplifier, e.visible)));
    let (mut r, mut g, mut b, mut n) = (0i64, 0i64, 0i64, 0i64);
    for (effect, amplifier, visible) in effekte {
        if !visible {
            continue;
        }
        // Unbekannter Effekt (neuer als die Tabelle): Vanilla haette eine Farbe, wir nicht --
        // weglassen verfaelscht das Mittel weniger als eine erfundene Farbe.
        let Some(&c) = EFFECT_COLORS.get(effect as usize) else { continue };
        let w = amplifier as i64 + 1;
        r += w * ((c >> 16) & 0xff) as i64;
        g += w * ((c >> 8) & 0xff) as i64;
        b += w * (c & 0xff) as i64;
        n += w;
    }
    (n != 0).then(|| (((r / n) as u32) << 16) | (((g / n) as u32) << 8) | (b / n) as u32)
}

// Die Trankfarbe des Stacks als 0xRRGGBB, oder None ohne potion_contents.
pub fn stack_color(stack: &ItemStack) -> Option<u32> {
    Some(color(stack.potion.as_ref()?).unwrap_or(BASE_POTION_COLOR))
}

// Dieselbe Farbe als Toenungsfaktor fuer den Stack, oder None, wenn er keine potion_contents hat
// (dann bleibt die Vorgabe des Item-Modells).
pub fn stack_tint(stack: &ItemStack) -> Option<[f32; 3]> {
    let c = stack_color(stack)?;
    Some([((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0])
}

// PotionItem/TippedArrowItem.getName: Schluessel descriptionId + ".effect." + (custom_name
// oder Potion.name() oder "empty"), uebersetzt mit en_us.json. None fuer alles, was kein
// Trank-Item ist.
pub fn display_name(item: &str, contents: Option<&PotionContents>) -> Option<String> {
    if !matches!(item, "potion" | "splash_potion" | "lingering_potion" | "tipped_arrow") {
        return None;
    }
    let schluessel = match contents {
        Some(c) => c
            .custom_name
            .as_deref()
            .or_else(|| c.potion.and_then(|id| POTIONS.get(id as usize)).map(|p| p.1))
            .unwrap_or("empty"),
        // Ganz ohne Component nimmt Vanilla den Itemnamen selbst.
        None => {
            if let Some(name) = protocol::nbt::translate(&format!("item.minecraft.{item}")) {
                return Some(name);
            }
            return Some(
                match item {
                    "potion" => "Potion",
                    "splash_potion" => "Splash Potion",
                    "lingering_potion" => "Lingering Potion",
                    _ => "Tipped Arrow",
                }
                .to_string(),
            )
        }
    };
    // Erst die geladene Sprachdatei (Einstellungen -> Sprache), sonst die eingebaute
    // en_us-Tabelle unten.
    if let Some(name) = protocol::nbt::translate(&format!("item.minecraft.{item}.effect.{schluessel}")) {
        return Some(name);
    }
    Some(translate(item, schluessel).unwrap_or_else(|| format!("{} ({schluessel})", item.replace('_', " "))))
}

// en_us.json, item.minecraft.<item>.effect.<schluessel>.
fn translate(item: &str, key: &str) -> Option<String> {
    let wirkung = match key {
        "fire_resistance" => "Fire Resistance",
        "harming" => "Harming",
        "healing" => "Healing",
        "infested" => "Infestation",
        "invisibility" => "Invisibility",
        "leaping" => "Leaping",
        "levitation" => "Levitation",
        "luck" => "Luck",
        "night_vision" => "Night Vision",
        "oozing" => "Oozing",
        "poison" => "Poison",
        "regeneration" => "Regeneration",
        "slow_falling" => "Slow Falling",
        "slowness" => "Slowness",
        "strength" => "Strength",
        "swiftness" => "Swiftness",
        "turtle_master" => "the Turtle Master",
        "water_breathing" => "Water Breathing",
        "weakness" => "Weakness",
        "weaving" => "Weaving",
        "wind_charged" => "Wind Charging",
        // Die Sonderfaelle ohne "of": je Item eigene Woerter.
        _ => {
            let text = match (item, key) {
                ("potion", "water") => "Water Bottle",
                ("potion", "mundane") => "Mundane Potion",
                ("potion", "thick") => "Thick Potion",
                ("potion", "awkward") => "Awkward Potion",
                ("potion", "empty") => "Uncraftable Potion",
                ("splash_potion", "water") => "Splash Water Bottle",
                ("splash_potion", "mundane") => "Mundane Splash Potion",
                ("splash_potion", "thick") => "Thick Splash Potion",
                ("splash_potion", "awkward") => "Awkward Splash Potion",
                ("splash_potion", "empty") => "Splash Uncraftable Potion",
                ("lingering_potion", "water") => "Lingering Water Bottle",
                ("lingering_potion", "mundane") => "Mundane Lingering Potion",
                ("lingering_potion", "thick") => "Thick Lingering Potion",
                ("lingering_potion", "awkward") => "Awkward Lingering Potion",
                ("lingering_potion", "empty") => "Lingering Uncraftable Potion",
                ("tipped_arrow", "water") => "Arrow of Splashing",
                ("tipped_arrow", "mundane" | "thick" | "awkward") => "Tipped Arrow",
                ("tipped_arrow", "empty") => "Uncraftable Tipped Arrow",
                _ => return None,
            };
            return Some(text.to_string());
        }
    };
    let praefix = match item {
        "potion" => "Potion",
        "splash_potion" => "Splash Potion",
        "lingering_potion" => "Lingering Potion",
        _ => "Arrow",
    };
    Some(format!("{praefix} of {wirkung}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::item::PotionEffect;

    fn trank(name: &str) -> PotionContents {
        let id = POTIONS.iter().position(|p| p.0 == name).unwrap() as u32;
        PotionContents { potion: Some(id), ..Default::default() }
    }

    #[test]
    fn ids_folgen_der_registrierreihenfolge() {
        assert_eq!(potion_registry_name(0), Some("water"));
        assert_eq!(potion_registry_name(25), Some("strong_healing"));
        assert_eq!(potion_registry_name(45), Some("infested"));
        assert_eq!(EFFECT_COLORS.len(), protocol::packets::MOB_EFFECTS.len());
        assert_eq!(EFFECT_BENEFICIAL.len(), protocol::packets::MOB_EFFECTS.len());
        assert!(effect_beneficial(protocol::packets::MOB_EFFECT_SPEED));
        assert!(!effect_beneficial(protocol::packets::MOB_EFFECT_SLOWNESS));
        // Leuchten (23) ist neutral, Atem des Nautilus (39) gut.
        assert!(!effect_beneficial(23));
        assert!(effect_beneficial(39));
    }

    #[test]
    fn namen_wie_en_us() {
        assert_eq!(display_name("potion", Some(&trank("strong_swiftness"))).unwrap(), "Potion of Swiftness");
        assert_eq!(display_name("splash_potion", Some(&trank("strong_healing"))).unwrap(), "Splash Potion of Healing");
        assert_eq!(display_name("lingering_potion", Some(&trank("long_turtle_master"))).unwrap(), "Lingering Potion of the Turtle Master");
        assert_eq!(display_name("potion", Some(&trank("water"))).unwrap(), "Water Bottle");
        assert_eq!(display_name("tipped_arrow", Some(&trank("infested"))).unwrap(), "Arrow of Infestation");
        assert_eq!(display_name("potion", Some(&PotionContents::default())).unwrap(), "Uncraftable Potion");
        assert_eq!(display_name("diamond_sword", None), None);
    }

    #[test]
    fn farben_wie_vanilla() {
        // Ein Effekt: genau dessen Farbe.
        assert_eq!(color(&trank("swiftness")), Some(3402751));
        assert_eq!(color(&trank("strong_healing")), Some(16262179));
        // Ohne Effekte: keine eigene Farbe -> Grundfarbe des Modells.
        assert_eq!(color(&trank("water")), None);
        // Schildkroete: Langsamkeit IV (Gewicht 4) + Resistenz III (Gewicht 3), ganzzahlig.
        let (s, w) = (9154528u32, 9520880u32);
        let mix = |sh: u32| (4 * ((s >> sh) & 0xff) + 3 * ((w >> sh) & 0xff)) / 7;
        assert_eq!(color(&trank("turtle_master")), Some((mix(16) << 16) | (mix(8) << 8) | mix(0)));
        // Feste Farbe schlaegt alles, unsichtbare Effekte faerben nicht.
        let mut c = trank("swiftness");
        c.custom_effects.push(PotionEffect { effect: 4, amplifier: 0, duration: 200, visible: false });
        assert_eq!(color(&c), Some(3402751));
        c.custom_color = Some(-1);
        assert_eq!(color(&c), Some(0xFFFFFF));
    }

    #[test]
    fn buchstabe_und_dauer() {
        let trank = |id: u32| {
            let mut st = ItemStack::new(1, 1);
            st.potion = Some(PotionContents { potion: Some(id), ..Default::default() });
            st
        };
        // 13 swiftness, 14 long_swiftness, 15 strong_swiftness
        assert_eq!(variant_mark(&trank(13)), None);
        assert_eq!(variant_mark(&trank(14)), Some('L'));
        assert_eq!(variant_mark(&trank(15)), Some('S'));
        // Heilung und Schaden: W fuer Stufe I, nichts fuer Stufe II.
        let heil = |name: &str| trank(POTIONS.iter().position(|p| p.0 == name).unwrap() as u32);
        assert_eq!(variant_mark(&heil("healing")), Some('W'));
        assert_eq!(variant_mark(&heil("strong_healing")), None);
        assert_eq!(variant_mark(&heil("harming")), Some('W'));
        assert_eq!(variant_mark(&heil("strong_harming")), None);
        let mut eigen = trank(15);
        eigen.potion.as_mut().unwrap().custom_effects.push(PotionEffect { effect: 4, amplifier: 0, duration: 200, visible: true });
        assert_eq!(variant_mark(&eigen), None);
        assert_eq!(all_effects(trank(15).potion.as_ref().unwrap()), vec![(0, 1, 1800)]);
        assert_eq!(all_effects(eigen.potion.as_ref().unwrap()).len(), 2);
        assert_eq!(duration_scale("lingering_potion", &trank(13)), 0.25);
    }
}
