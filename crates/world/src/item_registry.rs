// Zuordnung Item-ID -> Itemname (z.B. 1 -> "stone", 936 -> "diamond_sword").
//
// Wie bei den Blockstates (s. block_registry.rs) sind die IDs global und versionsabhaengig;
// die Wahrheit steht in der registries.json des Vanilla-Datengenerators. Daraus wurde
// items.bin gebacken (scripts/bake_items.py), damit zur Laufzeit weder JSON geparst noch
// eine JVM gestartet werden muss. Bei einem Versionswechsel neu erzeugen.
//
// Format (Little-Endian): u32 item_count, dann item_count * (u16 len + utf8-Bytes +
// u8 max_stack_size), Index = Item-ID.

use std::sync::OnceLock;

struct ItemRegistry {
    names: Vec<String>,
    // Maximale Stapelgroesse je Item-ID (1, 16 oder 64 in Vanilla) -- braucht die
    // Klick-Vorhersage im Inventar, um zu wissen, wie viel beim Zusammenlegen passt.
    max_stack: Vec<u8>,
}

static REGISTRY: OnceLock<ItemRegistry> = OnceLock::new();

fn registry() -> &'static ItemRegistry {
    REGISTRY.get_or_init(|| {
        let data: &[u8] = include_bytes!("items.bin");
        let mut p = 0usize;
        let count = u32::from_le_bytes(data[p..p + 4].try_into().unwrap()) as usize;
        p += 4;
        let mut names = Vec::with_capacity(count);
        let mut max_stack = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u16::from_le_bytes(data[p..p + 2].try_into().unwrap()) as usize;
            p += 2;
            names.push(String::from_utf8(data[p..p + len].to_vec()).expect("Itemname kein UTF-8"));
            p += len;
            max_stack.push(data[p]);
            p += 1;
        }
        ItemRegistry { names, max_stack }
    })
}

// Itemname (ohne "minecraft:"-Praefix) zu einer Item-ID, oder None fuer unbekannte IDs
// (z.B. neuere Items, als die Tabelle gebacken wurde -- der Aufrufer zeigt dann ein
// Platzhalter-Icon statt zu panicken).
pub fn item_name(id: u32) -> Option<&'static str> {
    registry().names.get(id as usize).map(|s| s.as_str())
}

// Rueckrichtung: Itemname -> Item-ID. Gebraucht, um einen BLOCK-State auf sein Item
// abzubilden (block_display zeichnet einen Blockzustand, die Texturen dafuer haengen in
// diesem Client aber am Item -- s. held_item::HeldItems). Die Tabelle wird beim ersten
// Aufruf einmal aufgebaut; ein linearer Durchlauf ueber ~1400 Namen pro Frame und
// Display-Entity waere Verschwendung.
pub fn item_id_by_name(name: &str) -> Option<u32> {
    static BY_NAME: OnceLock<std::collections::HashMap<&'static str, u32>> = OnceLock::new();
    let map = BY_NAME.get_or_init(|| registry().names.iter().enumerate().map(|(i, n)| (n.as_str(), i as u32)).collect());
    map.get(name).copied()
}

// Maximale Stapelgroesse. Unbekannte IDs bekommen Vanillas Default 64 -- das ist der Wert,
// mit dem sich die Vorhersage im Zweifel am wenigsten verrechnet.
pub fn max_stack_size(id: u32) -> i32 {
    registry().max_stack.get(id as usize).copied().unwrap_or(64) as i32
}

// Alle Itemnamen in ID-Reihenfolge -- fuer den Bau des Item-Icon-Atlas (einmal jede
// Textur aufloesen, nicht pro Stack erneut).
pub fn all_item_names() -> &'static [String] {
    registry().names.as_slice()
}

pub fn item_count() -> usize {
    registry().names.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bekannte_items_stimmen() {
        assert_eq!(item_name(0), Some("air"));
        assert_eq!(item_name(1), Some("stone"));
        assert_eq!(item_name(936), Some("diamond_sword"));
    }

    #[test]
    fn stapelgroessen_stimmen() {
        assert_eq!(max_stack_size(1), 64); // Stein
        let ender_pearl = all_item_names().iter().position(|n| n == "ender_pearl").unwrap() as u32;
        assert_eq!(max_stack_size(ender_pearl), 16);
        assert_eq!(max_stack_size(936), 1); // Diamantschwert
        assert_eq!(max_stack_size(u32::MAX), 64); // unbekannt -> Vanilla-Default
    }

    #[test]
    fn unbekannte_item_id_ist_none_statt_panic() {
        assert_eq!(item_name(u32::MAX), None);
    }
}

// Items, die man DURCHGEHEND benutzt: gedrueckt gehaltener Rechtsklick startet eine
// Benutzung, die bis zum Loslassen laeuft (Essen, Trinken, Bogen spannen, Schild halten,
// Fernrohr, Ziegenhorn, Pinsel). Alles andere wird mit einem Klick abgehandelt (Bloecke
// platzieren, Perlen werfen, Tueren oeffnen).
//
// WOFUER: zwei Dinge haengen daran, beide nach Vanilla.
//   1. ClientPlayerEntity.applyMovementSpeedFactors multipliziert waehrend der Benutzung mit
//      getActiveItemSpeedMultiplier() (Standard 0.2) -- zusaetzlich zum Schleich-Faktor.
//   2. MinecraftClient.handleInputEvents wiederholt die Benutzung bei gehaltener Taste NUR
//      solange `!player.isUsingItem()`. Wer das nicht kennt, schickt alle 4 Ticks ein neues
//      USE_ITEM -- und GrimAC setzt bei jedem davon seinen Item-Verlangsamungs-Zustand neu.
pub fn is_used_continuously(name: &str) -> bool {
    // Werkzeuge/Waffen mit Benutzungsanimation.
    const HALTEN: &[&str] = &["bow", "crossbow", "trident", "shield", "spyglass", "goat_horn", "brush"];
    // Speere: KineticWeaponComponent -> Item.use setzt die laufende Hand (Ansturm), 72000 Ticks.
    ESSEN.contains(&name) || HALTEN.contains(&name) || is_spear(name)
}

// Die Speere aus 1.21.11 (Item.Settings.spear): Stich per Linksklick (PiercingWeaponComponent,
// PlayerAction STAB), Ansturm per gehaltenem Rechtsklick (KineticWeaponComponent). Ihre
// use_effects erlauben dabei volles Tempo und Sprinten (UseEffectsComponent(true, false, 1.0)).
pub fn is_spear(name: &str) -> bool {
    matches!(
        name,
        "wooden_spear" | "stone_spear" | "copper_spear" | "iron_spear" | "golden_spear" | "diamond_spear" | "netherite_spear"
    )
}

// Item.Settings.spear(...) aus Items.java (1.21.11): Schwungdauer in Sekunden, dann Ansturm-
// Verzoegerung und die Hoechstdauern der drei Ansturm-Bedingungen (Absitzen, Rueckstoss,
// Schaden) in Sekunden. Umgerechnet wird wie in Vanilla: Sekunden mal 20, abgeschnitten -- in f32,
// damit das Abschneiden exakt dieselben Ticks ergibt.
fn spear_settings(name: &str) -> Option<[f32; 5]> {
    Some(match name {
        "wooden_spear" => [0.65, 0.75, 5.0, 10.0, 15.0],
        "stone_spear" => [0.75, 0.7, 4.5, 9.0, 13.75],
        "copper_spear" => [0.85, 0.65, 4.0, 8.25, 12.5],
        "iron_spear" => [0.95, 0.6, 2.5, 6.75, 11.25],
        "golden_spear" => [0.95, 0.7, 3.5, 8.5, 13.75],
        "diamond_spear" => [1.05, 0.5, 3.0, 6.5, 10.0],
        "netherite_spear" => [1.15, 0.4, 2.5, 5.5, 8.75],
        _ => return None,
    })
}

fn ticks(sekunden: f32) -> i32 {
    (sekunden * 20.0) as i32
}

// SwingAnimationComponent(STAB, swingSeconds * 20) der Speere; alle anderen Items schwingen
// mit Vanillas Standard von 6 Ticks (LivingEntity.getHandSwingDuration ohne Komponente).
pub fn swing_duration_ticks(name: &str) -> u32 {
    spear_settings(name).map_or(6, |s| ticks(s[0]) as u32)
}

// Die Zeitmarken des Ansturms (KineticWeaponComponent: delayTicks und die maxDurationTicks
// der drei Bedingungen), s. entity_model::first_person_spear_charge.
pub fn spear_kinetic(name: &str) -> Option<crate::entity_model::SpearKinetic> {
    let s = spear_settings(name)?;
    Some(crate::entity_model::SpearKinetic { delay: ticks(s[1]), dismount: ticks(s[2]), knockback: ticks(s[3]), damage: ticks(s[4]) })
}

// Wie lange Essen/Trinken dauert (consumable.consume_seconds * 20), oder None fuer alles,
// was nicht gegessen oder getrunken wird. Die Vanilla-Standarddauer ist 1,6 s; abweichend
// sind nur getrockneter Seetang (schneller) und die Honigflasche (langsamer).
pub fn eat_duration_ticks(name: &str) -> Option<u32> {
    if !ESSEN.contains(&name) {
        return None;
    }
    Some(match name {
        "dried_kelp" => 16,
        "honey_bottle" => 40,
        _ => 32,
    })
}

// Braucht dieses Item (mit seinen Standard-Components) Hunger, um gegessen zu werden?
// Consumable.canConsume -> PlayerEntity.canConsume(food.canAlwaysEat()): mit FoodComponent nur,
// wenn der Hungerbalken nicht voll ist -- ausser can_always_eat. Ohne FoodComponent (Trank,
// Milch, unheilvolle Flasche) gibt es die Pruefung gar nicht. can_always_eat haben laut
// FoodComponents (1.21.11) goldener und verzauberter goldener Apfel, Chorusfrucht,
// verdaechtiger Eintopf und die Honigflasche (aus dem Jar gezogen: alle Builder mit
// alwaysEdible()). Die Honigflasche fehlte -- und damit der Golden Head von mcpvp.club, der
// eine umgebaute Honigflasche ist.
pub fn needs_hunger(name: &str) -> bool {
    const IMMER: &[&str] =
        &["golden_apple", "enchanted_golden_apple", "chorus_fruit", "suspicious_stew", "honey_bottle", "milk_bucket", "potion", "ominous_bottle"];
    ESSEN.contains(&name) && !IMMER.contains(&name)
}

// Nahrung und Getraenke (UseAction EAT/DRINK).
const ESSEN: &[&str] = &[
    "apple",
    "golden_apple",
    "enchanted_golden_apple",
    "bread",
    "cookie",
    "melon_slice",
    "carrot",
    "golden_carrot",
    "potato",
    "baked_potato",
    "poisonous_potato",
    "beetroot",
    "dried_kelp",
    "pumpkin_pie",
    "rotten_flesh",
    "spider_eye",
    "chorus_fruit",
    "sweet_berries",
    "glow_berries",
    "beef",
    "cooked_beef",
    "porkchop",
    "cooked_porkchop",
    "chicken",
    "cooked_chicken",
    "mutton",
    "cooked_mutton",
    "rabbit",
    "cooked_rabbit",
    "cod",
    "cooked_cod",
    "salmon",
    "cooked_salmon",
    "tropical_fish",
    "pufferfish",
    "mushroom_stew",
    "rabbit_stew",
    "beetroot_soup",
    "suspicious_stew",
    "honey_bottle",
    "milk_bucket",
    "potion",
    "ominous_bottle",
];

// Nimmt `Item.use` die Benutzung AN? Das ist die Frage, an der Vanillas doItemUse entscheidet,
// ob es die naechste Hand ueberhaupt noch probiert: liefert interactItem fuer einen
// nicht-leeren Stapel ein ActionResult.Success, wird (ggf. geschwungen und) aufgehoert.
//
// Angenommen ist alles, was ein `Success` ist -- also SUCCESS (mit Schwung) UND CONSUME (ohne
// Schwung, s. air_use_swings-Kommentar). Genau deshalb reicht der Schwung als Kriterium
// nicht: ein Schild, ein Bogen oder ein Apfel schwingen nicht, beenden die Schleife aber
// sehr wohl -- sonst wuerde ein Schild in der Nebenhand nie zum Zug kommen, weil die
// Haupthand vorher schon "durchgefallen" waere, oder umgekehrt die Haupthand-Nahrung von der
// Nebenhand ueberholt.
//
// `block_hit` unterscheidet die beiden Strahl-Faelle (s. block_hit_use_swings).
pub fn item_use_accepted(name: &str, block_hit: bool) -> bool {
    let schwingt = if block_hit { block_hit_use_swings(name) } else { air_use_swings(name) };
    schwingt || is_used_continuously(name)
}

#[cfg(test)]
mod benutzungs_tests {
    use super::*;

    #[test]
    fn essdauer() {
        assert_eq!(eat_duration_ticks("cooked_beef"), Some(32));
        assert_eq!(eat_duration_ticks("dried_kelp"), Some(16));
        assert_eq!(eat_duration_ticks("honey_bottle"), Some(40));
        assert_eq!(eat_duration_ticks("bow"), None);
    }

    // Ein Tippfehler in der Liste waere unsichtbar -- er wuerde einfach nie greifen. Deshalb
    // gegen die echte Item-Registry pruefen: jeder Name muss es dort geben.
    #[test]
    fn jeder_name_der_liste_existiert_in_der_registry() {
        let alle = all_item_names();
        for name in ["golden_apple", "bow", "shield", "potion", "milk_bucket", "spyglass", "goat_horn", "brush", "cooked_beef"] {
            assert!(is_used_continuously(name), "{name} muss als Dauerbenutzung gelten");
            assert!(alle.iter().any(|n| n == name), "{name} steht nicht in der Item-Registry");
        }
    }

    #[test]
    fn bloecke_und_wurfitems_sind_keine_dauerbenutzung() {
        for name in ["stone", "obsidian", "end_crystal", "ender_pearl", "snowball", "diamond_sword"] {
            assert!(!is_used_continuously(name), "{name} darf nicht als Dauerbenutzung gelten");
        }
    }
}

// --- Was passiert beim Rechtsklick? ---------------------------------------------------
//
// Vanilla unterscheidet, ob eine Benutzung ANGENOMMEN wurde: nur dann schwingt der Arm.
// `MinecraftClient.doItemUse` schaut sich das Ergebnis an und schwingt bei
// `ActionResult.Success` mit `SwingSource.CLIENT` -- und sonst nicht.
//
// Die drei Faelle, die es dabei gibt (aus ActionResult, am Bytecode abgelesen):
//   SUCCESS         Schwung vom Client        -> Arm bewegt sich
//   SUCCESS_SERVER  Schwung vom Server        -> der Server schickt das Animate-Paket, wir
//                                                schwingen NICHT selbst (sonst doppelt)
//   CONSUME         gar kein Schwung          -> Essen, Bogen spannen, Schild heben
//   PASS / FAIL     keine Benutzung           -> kein Schwung
//
// Diese Funktion beantwortet den Fall "Benutzung ins LEERE" (kein Block getroffen): schwingt
// der Arm? Die Liste ist aus dem Jar abgeleitet, nicht geraten -- es sind genau die Items,
// deren Klasse `use(World, PlayerEntity, Hand)` mit `ActionResult.SUCCESS` beantwortet, und
// die Zuordnung Klasse -> Itemname stammt aus den Fabrikmethoden in `Items.<clinit>`.
//
// Nicht dabei, obwohl Vanilla dort schwingt: Eimer, Glasflasche und Boote. Die machen ihren
// EIGENEN Strahl gegen Fluessigkeiten (Wasser ist fuer den gewoehnlichen Strahl kein Treffer),
// und den kennt dieser Client nicht. Ebenso die Raketen-Ausnahme (nur beim Gleiten) und
// Abklingzeiten (ein Item in der Abklingzeit gibt PASS).
pub fn air_use_swings(name: &str) -> bool {
    throws_or_opens(name) || equips_by_name(name)
}

// Der Teil von air_use_swings, der an der Item-KLASSE haengt (werfen, auswerfen, oeffnen) --
// das Anziehen kommt bei geladenen Standards aus der equippable-Component (s.
// item_behavior::air_use_swings).
pub fn throws_or_opens(name: &str) -> bool {
    const WURF_UND_OEFFNEN: &[&str] = &[
        // Wurfgeschosse (SnowballItem, EggItem, EnderPearlItem, ExperienceBottleItem,
        // WindChargeItem, ThrowablePotionItem).
        "snowball",
        "egg",
        "blue_egg",
        "brown_egg",
        "ender_pearl",
        "experience_bottle",
        "wind_charge",
        "splash_potion",
        "lingering_potion",
        // Auswerfen/Einholen (FishingRodItem).
        "fishing_rod",
        // Oeffnet einen Bildschirm (BundleItem, EmptyMapItem, Buecher).
        "map",
        "writable_book",
        "written_book",
        "knowledge_book",
    ];
    WURF_UND_OEFFNEN.contains(&name) || name.ends_with("bundle")
}

fn equips_by_name(name: &str) -> bool {
    // Ruestung anziehen: der Standardzweig von `Item.use` gibt fuer ein EQUIPPABLE-Item mit
    // freiem Platz SUCCESS zurueck (EquippableComponent.equip). Pferde- und Wolfsruestung
    // gehoert bewusst NICHT dazu -- sie passt nur an das jeweilige Tier, `allows(entityType)`
    // schlaegt beim Spieler fehl und liefert PASS.
    const KOPF: &[&str] = &[
        "elytra",
        "carved_pumpkin",
        "player_head",
        "zombie_head",
        "creeper_head",
        "dragon_head",
        "piglin_head",
        "skeleton_skull",
        "wither_skeleton_skull",
    ];
    KOPF.contains(&name)
        || name.ends_with("_helmet")
        || name.ends_with("_chestplate")
        || name.ends_with("_leggings")
        || name.ends_with("_boots")
}

// Tut dieses Item etwas, wenn man damit auf einen BLOCK klickt? Das ist die zweite Haelfte
// derselben Frage (s. air_use_swings), fuer den Fall MIT Blocktreffer.
//
// Bloecke selbst stehen hier NICHT drin -- die erkennt der Aufrufer daran, dass es einen
// gleichnamigen Block gibt (block_registry::is_block), und prueft zusaetzlich, ob am Zielplatz
// ueberhaupt Raum ist. Hier stehen nur die Items, die keinen Block setzen und trotzdem wirken.
pub fn acts_on_block(name: &str) -> bool {
    const EINZELN: &[&str] = &[
        "flint_and_steel",
        "fire_charge",
        "bone_meal",
        "shears",
        "honeycomb",
        "brush",
        "ink_sac",
        "glow_ink_sac",
        "item_frame",
        "glow_item_frame",
        "painting",
        "armor_stand",
        "end_crystal",
        "lead",
        "firework_rocket",
        "glass_bottle",
        // PowderSnowBucketItem ist als EINZIGER Eimer ein `extends BlockItem` und hat damit
        // ein echtes useOnBlock. Alle anderen Eimer (BucketItem und das davon abgeleitete
        // EntityBucketItem) haben NUR `use` -- die gehoeren deshalb nicht hierher, sondern
        // nach block_hit_use_swings, s. dort.
        "powder_snow_bucket",
    ];
    const ENDUNGEN: &[&str] = &["_hoe", "_shovel", "_axe", "_spawn_egg", "_minecart", "_boat", "_raft", "_dye"];
    // "_axe" trifft auch die Spitzhacke ("pickaxe") -- die STREIFT aber keinen Stamm ab,
    // sondern tut auf einem Block nichts. Ohne diese Ausnahme schwaenge der Arm bei jedem
    // Rechtsklick mit der Spitzhacke, und mit der klickt man beim Bauen dauernd herum.
    if name.ends_with("pickaxe") {
        return false;
    }
    EINZELN.contains(&name) || ENDUNGEN.iter().any(|e| name.ends_with(e))
}

// Schwingt der Arm, wenn `Item.use` NACH einem Blocktreffer laeuft? Das ist der Fall, den
// `MinecraftClient.doItemUse` durch DURCHFALLEN erreicht: der `case BLOCK:`-Zweig kehrt nur
// bei Success oder Fail zurueck, bei PASS laeuft die Schleife weiter in
// `interactionManager.interactItem(...)` -- ein zweites, eigenes Paket.
//
// Er umfasst alles aus air_use_swings (dieselben Wurf-/Anzieh-Items schwingen mit oder ohne
// Block im Fadenkreuz) und ZUSAETZLICH die gefuellten Eimer. Deren Sonderrolle steht in
// BucketItem.use: ein leerer Eimer strahlt mit FluidHandling.SOURCE_ONLY, ein gefuellter
// mit FluidHandling.NONE.
//
// Ein GEFUELLTER Eimer strahlt mit FluidHandling.NONE -- also mit genau derselben
// Fluessigkeitsbehandlung wie unser gewoehnlicher Blockstrahl, und trifft damit denselben
// Block. Gelingt das Ausleeren, ist das Ergebnis SUCCESS (Schwung vom Client).
//
// Der LEERE Eimer ("bucket") bleibt bewusst draussen: er strahlt mit SOURCE_ONLY, trifft also
// Fluessigkeitsquellen, die unser Strahl gar nicht als Treffer meldet (s. air_use_swings).
// "milk_bucket" ebenfalls nicht -- das ist gar kein BucketItem, sondern ein gewoehnliches
// Item mit CONSUMABLE-Komponente, also Essen (CONSUME, kein Schwung). Und
// "powder_snow_bucket" nicht, weil es als einziger Eimer ein BlockItem ist: es wird schon
// eine Stufe frueher angenommen (acts_on_block) und kommt hier gar nicht an.
pub fn block_hit_use_swings(name: &str) -> bool {
    empties_bucket(name) || air_use_swings(name)
}

// Ein gefuellter Fluessigkeitseimer (s. block_hit_use_swings).
pub fn empties_bucket(name: &str) -> bool {
    const KEIN_FLUESSIGKEITSEIMER: &[&str] = &["bucket", "milk_bucket", "powder_snow_bucket"];
    name.ends_with("_bucket") && !KEIN_FLUESSIGKEITSEIMER.contains(&name)
}

#[cfg(test)]
mod schwung_tests {
    use super::*;

    // Wie bei is_used_continuously: ein Tippfehler waere unsichtbar. Jeder Name muss es in der
    // echten Item-Registry geben.
    #[test]
    fn jeder_gelistete_name_existiert() {
        let alle = all_item_names();
        for name in [
            "snowball",
            "ender_pearl",
            "experience_bottle",
            "wind_charge",
            "splash_potion",
            "lingering_potion",
            "fishing_rod",
            "map",
            "writable_book",
            "knowledge_book",
            "bundle",
            "elytra",
            "carved_pumpkin",
            "player_head",
            "flint_and_steel",
            "bone_meal",
            "shears",
            "honeycomb",
            "end_crystal",
            "armor_stand",
            "firework_rocket",
            "diamond_helmet",
            "netherite_boots",
        ] {
            assert!(alle.iter().any(|n| n == name), "{name} steht nicht in der Item-Registry");
        }
    }

    #[test]
    fn wurfgeschosse_und_ruestung_schwingen_in_der_luft() {
        for name in ["snowball", "ender_pearl", "egg", "wind_charge", "fishing_rod", "bundle", "red_bundle", "diamond_helmet", "elytra"] {
            assert!(air_use_swings(name), "{name} muss in der Luft schwingen");
        }
    }

    #[test]
    // Der wichtige Fall: Rechtsklick ins Leere mit dem, was man im Kampf haelt. Vanilla
    // bewegt dabei GAR NICHTS -- weder die eigene Hand noch das Modell fuer die anderen.
    fn schwert_essen_und_bloecke_schwingen_nicht_in_der_luft() {
        for name in
            ["diamond_sword", "netherite_axe", "bow", "shield", "golden_apple", "potion", "obsidian", "ender_chest", "totem_of_undying"]
        {
            assert!(!air_use_swings(name), "{name} darf in der Luft nicht schwingen");
        }
    }

    #[test]
    // Pferde- und Wolfsruestung sieht wie Ruestung aus, laesst sich am Spieler aber nicht
    // anlegen -- Vanilla gibt dort PASS.
    fn tierruestung_ist_keine_ruestung() {
        for name in ["iron_horse_armor", "diamond_horse_armor", "wolf_armor"] {
            assert!(!air_use_swings(name), "{name} darf nicht als anlegbar gelten");
        }
    }

    // Der Punkt, an dem der Schwung als Kriterium NICHT reicht: ein Schild schwingt nicht,
    // beendet Vanillas Hand-Schleife aber sehr wohl (CONSUME ist ein `Success`). Haette man
    // nur nach dem Schwung entschieden, waere die Benutzung weitergereicht worden -- und ein
    // Schild in der Nebenhand haette nie geblockt.
    #[test]
    fn schild_und_essen_nehmen_an_ohne_zu_schwingen() {
        for name in ["shield", "bow", "crossbow", "golden_apple", "potion", "spyglass"] {
            assert!(item_use_accepted(name, false), "{name} muss die Benutzung annehmen");
            assert!(!air_use_swings(name), "{name} darf dabei nicht schwingen");
        }
    }

    // Und die Gegenprobe: was WIRKLICH durchfaellt, muss durchfallen -- sonst kommt die
    // zweite Hand nie dran. Ein Schwert in der Haupthand ist genau der Alltagsfall, in dem
    // das Schild in der Nebenhand zum Zug kommen muss.
    #[test]
    fn schwert_und_bloecke_reichen_an_die_naechste_hand_weiter() {
        for name in ["diamond_sword", "netherite_axe", "stone", "dirt", "stick", "arrow"] {
            assert!(!item_use_accepted(name, false), "{name} muss die Benutzung durchreichen");
        }
    }

    // Mit Block im Fadenkreuz zaehlt der andere Schwung-Zweig -- ein gefuellter Eimer nimmt
    // dort an, in der Luft nicht (sein Strahl trifft dann nichts).
    #[test]
    fn der_gefuellte_eimer_nimmt_nur_mit_block_an() {
        assert!(item_use_accepted("water_bucket", true));
        assert!(!item_use_accepted("water_bucket", false));
        // Ein Schneeball dagegen fliegt in beiden Faellen.
        assert!(item_use_accepted("snowball", true));
        assert!(item_use_accepted("snowball", false));
    }

    #[test]
    fn werkzeuge_wirken_auf_bloecke_die_spitzhacke_aber_nicht() {
        for name in ["diamond_hoe", "iron_shovel", "netherite_axe", "flint_and_steel", "zombie_spawn_egg", "bone_meal"] {
            assert!(acts_on_block(name), "{name} muss auf einem Block wirken");
        }
        for name in ["diamond_pickaxe", "netherite_pickaxe", "wooden_pickaxe"] {
            assert!(!acts_on_block(name), "{name} tut auf einem Block nichts");
        }
        for name in ["diamond_sword", "golden_apple", "stick", "arrow"] {
            assert!(!acts_on_block(name), "{name} tut auf einem Block nichts");
        }
    }

    // Eimer gehoeren NICHT hierher, sonst lassen sie sich nicht platzieren. Sie standen hier als
    // "wirkt auf einen Block" -- das las sich richtig (er tut ja sichtbar etwas), war aber die
    // falsche Schublade: BucketItem hat gar kein useOnBlock, nur `use`. Mit dem Eintrag galt
    // die Blockbenutzung als ANGENOMMEN, und der Aufrufer schickte deshalb nie das zweite
    // Paket, an dem die ganze Eimerlogik haengt (s. App::do_item_use).
    //
    // powder_snow_bucket ist die einzige Ausnahme -- als einziger Eimer ein BlockItem.
    #[test]
    fn fluessigkeitseimer_wirken_nicht_ueber_use_on_block() {
        for name in ["water_bucket", "lava_bucket", "bucket", "cod_bucket", "axolotl_bucket", "milk_bucket"] {
            assert!(!acts_on_block(name), "{name} hat kein useOnBlock -- darf die Blockbenutzung nicht annehmen");
        }
        assert!(acts_on_block("powder_snow_bucket"), "PowderSnowBucketItem ist ein BlockItem und HAT useOnBlock");
    }

    // Die andere Haelfte derselben Frage: nach dem Durchreichen schwingt der Arm fuer einen
    // gefuellten Eimer (sein eigener Strahl laeuft mit FluidHandling.NONE und trifft denselben
    // Block), fuer den leeren nicht (SOURCE_ONLY -- Fluessigkeitsquellen, die unser Strahl
    // gar nicht meldet), und fuer Milch nicht (das ist Essen).
    #[test]
    fn gefuellte_eimer_schwingen_nach_einem_blocktreffer() {
        for name in ["water_bucket", "lava_bucket", "cod_bucket", "tropical_fish_bucket"] {
            assert!(block_hit_use_swings(name), "{name} muss nach einem Blocktreffer schwingen");
        }
        for name in ["bucket", "milk_bucket", "diamond_sword", "golden_apple"] {
            assert!(!block_hit_use_swings(name), "{name} darf nach einem Blocktreffer nicht schwingen");
        }
        // Und alles, was schon in der Luft schwingt, schwingt mit Block im Fadenkreuz genauso.
        for name in ["snowball", "ender_pearl", "diamond_helmet", "elytra"] {
            assert!(block_hit_use_swings(name), "{name} schwingt in der Luft -- mit Block genauso");
        }
    }
}

// Angriffsgeschwindigkeit des Items in der Haupthand (Attribut ATTACK_SPEED). Daraus faellt
// Vanillas Angriffs-Aufladung: getAttackCooldownProgressPerTick() = 20 / attackSpeed, und
// getAttackCooldownProgress(0.5) = clamp((ticksSeitAngriff + 0.5) / dieser Wert, 0, 1).
//
// Die Zahlen sind KEINE Erinnerungswerte, sondern aus Items.class des 1.21.11-Jars gelesen:
// dort steht je Werkzeug ein ADD_VALUE-Modifikator auf den Grundwert 4.0, z.B.
// Diamantschwert mit -2.4 -> 4.0 - 2.4 = 1.6. Alles, was
// keinen Modifikator traegt (Bloecke, Kristalle, blosse Hand), bleibt bei 4.0.
pub fn attack_speed(name: &str) -> f32 {
    // Materialpraefix abschneiden -- die Modifikatoren haengen fast ueberall nur an der
    // Werkzeugart, nicht am Material. Die drei Ausnahmen (Aexte, Hacken) stehen darunter.
    if let Some(art) = name.rsplit('_').next() {
        match art {
            "sword" => return 4.0 - 2.4,
            "pickaxe" => return 4.0 - 2.8,
            "shovel" => return 4.0 - 3.0,
            _ => {}
        }
    }
    match name {
        // Aexte: Holz/Kupfer/Stein -3.2, Eisen -3.1, Gold/Diamant/Netherit -3.0.
        "wooden_axe" | "copper_axe" | "stone_axe" => 4.0 - 3.2,
        "iron_axe" => 4.0 - 3.1,
        "golden_axe" | "diamond_axe" | "netherite_axe" => 4.0 - 3.0,
        // Hacken: pro Material verschieden, hier ist das Praefix wirklich noetig.
        "wooden_hoe" | "golden_hoe" => 4.0 - 3.0,
        "copper_hoe" | "stone_hoe" => 4.0 - 2.0,
        "iron_hoe" => 4.0 - 1.0,
        "diamond_hoe" | "netherite_hoe" => 4.0,
        "trident" => 4.0 - 2.9,
        "mace" => 4.0 - 3.4,
        // Speere: Modifikator 1/Schwungdauer - 4 (Item.Settings.spear), also 1/Schwungdauer.
        "wooden_spear" => 1.0 / 0.65,
        "stone_spear" => 1.0 / 0.75,
        "copper_spear" => 1.0 / 0.85,
        "iron_spear" | "golden_spear" => 1.0 / 0.95,
        "diamond_spear" => 1.0 / 1.05,
        "netherite_spear" => 1.0 / 1.15,
        // Grundwert des Spielers (EntityAttributes.ATTACK_SPEED) ohne jeden Modifikator.
        _ => 4.0,
    }
}

#[cfg(test)]
mod angriffsgeschwindigkeit_tests {
    use super::attack_speed;

    #[test]
    fn speere_laden_so_lange_wie_ihr_schwung_dauert() {
        // getAttackCooldownProgressPerTick = 20 / Tempo = Schwungdauer in Ticks.
        assert!((20.0 / attack_speed("wooden_spear") - 13.0).abs() < 1e-4);
        assert!((20.0 / attack_speed("netherite_spear") - 23.0).abs() < 1e-4);
        assert!(super::is_spear("copper_spear") && !super::is_spear("spear"));
        assert!(super::is_used_continuously("diamond_spear"), "Ansturm per gehaltenem Rechtsklick");
    }

    // Die bekannten Vanilla-Werte -- und die Faelle, in denen das Materialpraefix zaehlt.
    #[test]
    fn angriffsgeschwindigkeiten_stimmen() {
        assert!((attack_speed("diamond_sword") - 1.6).abs() < 1.0e-6);
        assert!((attack_speed("netherite_sword") - 1.6).abs() < 1.0e-6);
        assert!((attack_speed("diamond_pickaxe") - 1.2).abs() < 1.0e-6);
        assert!((attack_speed("iron_shovel") - 1.0).abs() < 1.0e-6);
        // Aexte unterscheiden sich je nach Material.
        assert!((attack_speed("stone_axe") - 0.8).abs() < 1.0e-6);
        assert!((attack_speed("iron_axe") - 0.9).abs() < 1.0e-6);
        assert!((attack_speed("netherite_axe") - 1.0).abs() < 1.0e-6);
        // Hacken ebenso -- und die Diamanthacke bleibt beim Grundwert.
        assert!((attack_speed("stone_hoe") - 2.0).abs() < 1.0e-6);
        assert!((attack_speed("iron_hoe") - 3.0).abs() < 1.0e-6);
        assert!((attack_speed("diamond_hoe") - 4.0).abs() < 1.0e-6);
        assert!((attack_speed("mace") - 0.6).abs() < 1.0e-6);
        assert!((attack_speed("trident") - 1.1).abs() < 1.0e-6);
        // Alles ohne Modifikator: der Grundwert des Spielers.
        assert!((attack_speed("obsidian") - 4.0).abs() < 1.0e-6);
        assert!((attack_speed("air") - 4.0).abs() < 1.0e-6);
    }
}

#[cfg(test)]
mod spear_timing_tests {
    use super::*;

    #[test]
    fn schwungdauer_der_speere() {
        assert_eq!(swing_duration_ticks("wooden_spear"), 13);
        assert_eq!(swing_duration_ticks("netherite_spear"), 23);
        assert_eq!(swing_duration_ticks("diamond_sword"), 6);
        let k = spear_kinetic("iron_spear").unwrap();
        assert_eq!((k.delay, k.dismount, k.knockback, k.damage), (12, 50, 135, 225));
    }

    #[test]
    fn hunger_pflicht_nur_fuer_normales_essen() {
        assert!(needs_hunger("cooked_beef"));
        assert!(!needs_hunger("honey_bottle"), "Honigflasche ist immer trinkbar (FoodComponents.HONEY_BOTTLE alwaysEdible)");
        assert!(needs_hunger("sweet_berries"));
        for immer in ["golden_apple", "enchanted_golden_apple", "chorus_fruit", "suspicious_stew", "potion", "milk_bucket"] {
            assert!(!needs_hunger(immer), "{immer}");
        }
        assert!(!needs_hunger("bow"));
    }
}
