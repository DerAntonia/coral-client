// Was ein Stapel TUT, aus seinen wirksamen Components -- wie Vanilla: gleiten (glider), vor dem
// Tod retten (death_protection), blocken (blocks_attacks), Schwung und Speer (swing_animation,
// piercing_weapon, kinetic_weapon), Tempo beim Benutzen (use_effects), essen (consumable, food).
// Ein Server kann so jedes Item zur Elytra, zum Totem oder zum Schild machen, und eines dieser
// Items per entfernter Component zu einem gewoehnlichen.
//
// Ohne die extrahierten Standards (Launcher nie gelaufen) gelten die Namenstabellen in
// item_registry, also Vanillas Items mit Vanillas Werten.

use protocol::item::{BlocksAttacks, ItemStack, UseEffects};

use crate::item_components::{loaded, Components};
use crate::item_registry as reg;

pub const COMPONENT_USE_EFFECTS: i32 = 5;
pub const COMPONENT_MINIMUM_ATTACK_CHARGE: i32 = 7;
pub const COMPONENT_GLIDER: i32 = 34;
pub const COMPONENT_DEATH_PROTECTION: i32 = 36;
pub const COMPONENT_BLOCKS_ATTACKS: i32 = 37;
pub const COMPONENT_PIERCING_WEAPON: i32 = 38;
pub const COMPONENT_KINETIC_WEAPON: i32 = 39;
pub const COMPONENT_SWING_ANIMATION: i32 = 40;

// Vanillas Schwungdauer ohne swing_animation (LivingEntity.getCurrentSwingDuration).
pub const DEFAULT_SWING_TICKS: u32 = 6;
// Tempo beim Benutzen ohne use_effects (UseEffects.DEFAULT).
pub const DEFAULT_USE_SPEED: f32 = 0.2;
// ItemUseAnimation.BLOCK.
const ANIMATION_BLOCK: i32 = 3;

fn name(stack: &ItemStack) -> &'static str {
    reg::item_name(stack.item_id).unwrap_or("")
}

// Hat der Stapel Haltbarkeit (max_damage, nicht unzerbrechlich)?
pub fn damageable(stack: &ItemStack) -> bool {
    let c = Components::of(stack);
    c.max_damage() > 0 && !c.unbreakable()
}

// ItemStack.nextDamageWillBreak: beschaedigbar und nur noch ein Punkt uebrig.
pub fn next_damage_will_break(stack: &ItemStack) -> bool {
    let c = Components::of(stack);
    damageable(stack) && c.damage() >= c.max_damage() - 1
}

// Gleitet man damit (LivingEntity.canGlideUsing)? Die Component, im Slot, in den das Item
// gehoert, und nicht kurz vor dem Zerbrechen. `slot` ist die EquipmentSlot-Ordinalzahl.
pub fn can_glide_with(stack: &ItemStack, slot: u8) -> bool {
    let c = Components::of(stack);
    let gleiter = if loaded() { c.has(COMPONENT_GLIDER) } else { name(stack) == "elytra" };
    let passt = match c.equippable() {
        Some(e) => e.slot == slot,
        None => !loaded() && slot == 4,
    };
    gleiter && passt && !next_damage_will_break(stack)
}

// Rettet der Stapel vor dem Tod (Totem)?
pub fn death_protection(stack: &ItemStack) -> bool {
    if loaded() {
        Components::of(stack).has(COMPONENT_DEATH_PROTECTION)
    } else {
        name(stack) == "totem_of_undying"
    }
}

// Blockt der Stapel Angriffe (Schild), und mit welcher Verzoegerung?
pub fn blocks_attacks(stack: &ItemStack) -> Option<BlocksAttacks> {
    let c = Components::of(stack);
    if !loaded() {
        return (name(stack) == "shield").then_some(BlocksAttacks { block_delay_seconds: 0.25, disable_cooldown_scale: 1.0 });
    }
    if let Some(b) = stack.blocks_attacks {
        return Some(b);
    }
    if !c.has(COMPONENT_BLOCKS_ATTACKS) {
        return None;
    }
    crate::item_components::defaults(stack.item_id).and_then(|d| d.blocks_attacks)
}

// Die Art, wie der Arm schwingt (SwingAnimationType): 0 keine, 1 Schlag, 2 Stich.
pub const SWING_NONE: i32 = 0;
pub const SWING_WHACK: i32 = 1;
pub const SWING_STAB: i32 = 2;

// swing_animation: Art und Dauer in Ticks.
pub fn swing(stack: &ItemStack) -> (i32, u32) {
    if !loaded() {
        let n = name(stack);
        return (if reg::is_spear(n) { SWING_STAB } else { SWING_WHACK }, reg::swing_duration_ticks(n));
    }
    match pick(stack, COMPONENT_SWING_ANIMATION, |s| s.swing_animation) {
        Some(a) => (a.kind, a.duration.max(1) as u32),
        None => (SWING_WHACK, DEFAULT_SWING_TICKS),
    }
}

// Wie weit der Angriff mindestens aufgeladen sein muss, bevor ein Linksklick ueberhaupt etwas
// tut (minimum_attack_charge, Speere 1.0; ohne die Component 0 = jederzeit).
pub fn minimum_attack_charge(stack: &ItemStack) -> f32 {
    if loaded() {
        pick(stack, COMPONENT_MINIMUM_ATTACK_CHARGE, |s| s.minimum_attack_charge).unwrap_or(0.0)
    } else if reg::is_spear(name(stack)) {
        1.0
    } else {
        0.0
    }
}

// Sticht der Linksklick statt zu schlagen (piercing_weapon, die Speere)?
pub fn piercing(stack: &ItemStack) -> bool {
    if loaded() {
        Components::of(stack).has(COMPONENT_PIERCING_WEAPON)
    } else {
        reg::is_spear(name(stack))
    }
}

// Der Ansturm beim gehaltenen Rechtsklick (kinetic_weapon), in den Zeitmarken der Ego-Haltung.
pub fn kinetic(stack: &ItemStack) -> Option<crate::entity_model::SpearKinetic> {
    if !loaded() {
        return reg::spear_kinetic(name(stack));
    }
    let k = pick(stack, COMPONENT_KINETIC_WEAPON, |s| s.kinetic_weapon)?;
    let dauer = |c: Option<protocol::item::KineticCondition>| c.map_or(0, |c| c.max_duration_ticks);
    Some(crate::entity_model::SpearKinetic { delay: k.delay_ticks, dismount: dauer(k.dismount), knockback: dauer(k.knockback), damage: dauer(k.damage) })
}

// use_effects: darf man beim Benutzen sprinten, und mit welchem Tempo-Faktor geht man?
pub fn use_effects(stack: &ItemStack) -> UseEffects {
    let standard = UseEffects { can_sprint: false, interact_vibrations: true, speed_multiplier: DEFAULT_USE_SPEED };
    if !loaded() {
        return if reg::is_spear(name(stack)) { UseEffects { can_sprint: true, interact_vibrations: false, speed_multiplier: 1.0 } } else { standard };
    }
    pick(stack, COMPONENT_USE_EFFECTS, |s| s.use_effects).unwrap_or(standard)
}

// Wie lange Essen/Trinken dauert (consumable), in Ticks -- None, wenn das Item nicht verzehrt
// wird. Ohne Standards gilt fuer nicht gesetzte Werte die Namenstabelle.
pub fn consume_ticks(stack: &ItemStack) -> Option<u32> {
    let id = protocol::item::COMPONENT_CONSUMABLE;
    if !loaded() && stack.consume_seconds.is_none() && !stack.removed_components.contains(&id) {
        return reg::eat_duration_ticks(name(stack));
    }
    let sekunden = pick(stack, id, |s| s.consume_seconds)?;
    Some((sekunden * 20.0).max(0.0) as u32)
}

// Braucht das Essen Hunger (food ohne can_always_eat)? Ohne food-Component gar nicht.
pub fn needs_hunger(stack: &ItemStack) -> bool {
    let id = protocol::item::COMPONENT_FOOD;
    if !loaded() && stack.food_always_edible.is_none() && !stack.removed_components.contains(&id) {
        return reg::needs_hunger(name(stack));
    }
    match pick(stack, id, |s| s.food_always_edible) {
        Some(immer) => consume_ticks(stack).is_some() && !immer,
        None => false,
    }
}

// Wird es getrunken statt gegessen (ItemUseAnimation DRINK)?
pub fn is_drink(stack: &ItemStack) -> bool {
    const DRINK: i32 = 2;
    match pick(stack, protocol::item::COMPONENT_CONSUMABLE, |s| s.consume_animation) {
        Some(a) => a == DRINK,
        None => matches!(name(stack), "potion" | "milk_bucket" | "honey_bottle" | "ominous_bottle"),
    }
}

// Kruemelt es beim Verzehr (consumable.has_consume_particles)? Ohne Standards: alles, was
// gegessen statt getrunken wird.
pub fn consume_particles(stack: &ItemStack) -> bool {
    if !loaded() && stack.consume_particles.is_none() {
        return consume_ticks(stack).is_some() && !is_drink(stack);
    }
    pick(stack, protocol::item::COMPONENT_CONSUMABLE, |s| s.consume_particles).unwrap_or(false)
}

// Schwingt der Arm bei einem Rechtsklick ins Leere (Item.use mit SUCCESS)? Werfen, Auswerfen und
// Oeffnen haengen an der Item-Klasse, das Anziehen an der equippable-Component.
pub fn air_use_swings(stack: &ItemStack) -> bool {
    if !loaded() {
        return reg::air_use_swings(name(stack));
    }
    reg::throws_or_opens(name(stack)) || equips_on_use(stack)
}

// Dasselbe mit einem Block im Fadenkreuz (s. item_registry::block_hit_use_swings).
pub fn block_hit_use_swings(stack: &ItemStack) -> bool {
    reg::empties_bucket(name(stack)) || air_use_swings(stack)
}

// Nimmt Item.use die Benutzung an (s. item_registry::item_use_accepted)?
pub fn item_use_accepted(stack: &ItemStack, block_hit: bool) -> bool {
    let schwingt = if block_hit { block_hit_use_swings(stack) } else { air_use_swings(stack) };
    schwingt || used_continuously(stack)
}

// Haelt man das Item durchgehend (Rechtsklick gehalten = Benutzung bis zum Loslassen)? Verzehren,
// Blocken und Ansturm kommen aus Components; Bogen, Armbrust, Dreizack, Fernrohr, Ziegenhorn und
// Pinsel sind Item-Klassen ohne eigene Component.
pub fn used_continuously(stack: &ItemStack) -> bool {
    if !loaded() {
        return reg::is_used_continuously(name(stack));
    }
    const KLASSEN: &[&str] = &["bow", "crossbow", "trident", "spyglass", "goat_horn", "brush"];
    consume_ticks(stack).is_some() || blocks_attacks(stack).is_some() || kinetic(stack).is_some() || KLASSEN.contains(&name(stack))
}

// Blockt die Benutzung (ItemUseAnimation BLOCK)? Das Schild, aber auch alles, dem ein Server
// blocks_attacks gibt.
pub fn use_animation_block(stack: &ItemStack) -> bool {
    blocks_attacks(stack).is_some() || pick(stack, protocol::item::COMPONENT_CONSUMABLE, |s| s.consume_animation) == Some(ANIMATION_BLOCK)
}

// Zieht ein Rechtsklick das Item an (equippable, swappable, fuer Spieler erlaubt)? Wie in
// Item.use geht Verzehren vor.
pub fn equips_on_use(stack: &ItemStack) -> bool {
    equip_slot_on_use(stack).is_some()
}

// Derselbe Test, mit dem Ziel: EquipmentSlot-Ordinal 2 (Fuesse) bis 5 (Kopf).
pub fn equip_slot_on_use(stack: &ItemStack) -> Option<u8> {
    if consume_ticks(stack).is_some() {
        return None;
    }
    Components::of(stack).equippable().filter(|e| e.swappable && !e.restricted && (2..=5).contains(&e.slot)).map(|e| e.slot)
}

// Die wirksame Component als Kopie: gesetzt, sonst (nicht entfernt) der Standard.
fn pick<T: Copy>(stack: &ItemStack, id: i32, feld: impl Fn(&ItemStack) -> Option<T>) -> Option<T> {
    if let Some(v) = feld(stack) {
        return Some(v);
    }
    if stack.removed_components.contains(&id) {
        return None;
    }
    crate::item_components::defaults(stack.item_id).and_then(feld)
}

// Abenteuermodus (Player.blockActionRestricted / ItemStack.canPlaceOnBlockInAdventureMode):
// abbauen nur mit can_break, das den Block nennt, platzieren und benutzen nur mit
// can_place_on fuer den ANGEKLICKTEN Block. Ein Praedikat aus der Liste genuegt.
pub fn adventure_allows(preds: Option<&[protocol::item::BlockPredicate]>, state: u32) -> bool {
    preds.is_some_and(|p| p.iter().any(|p| block_predicate_matches(p, state)))
}

// BlockPredicate.test ohne Blockobjekt-Daten (s. BlockPredicate::needs_block_entity).
fn block_predicate_matches(p: &protocol::item::BlockPredicate, state: u32) -> bool {
    use protocol::item::StateValueMatcher;
    if p.needs_block_entity {
        return false;
    }
    if let Some((tag, ids)) = &p.blocks {
        let set = match tag {
            Some(t) => crate::mining::BlockSet::Tag(t.clone()),
            None => crate::mining::BlockSet::Ids(ids.iter().map(|&i| i as u32).collect()),
        };
        if !set.contains_state(state) {
            return false;
        }
    }
    p.properties.iter().all(|(name, matcher)| {
        // Eine Property, die der Block nicht hat, passt nie.
        let Some(wert) = crate::state_props::property(state, name) else { return false };
        match matcher {
            StateValueMatcher::Exact(v) => wert == v,
            StateValueMatcher::Range(min, max) => {
                min.as_deref().is_none_or(|m| property_order(wert, m).is_some_and(|o| o.is_ge()))
                    && max.as_deref().is_none_or(|m| property_order(wert, m).is_some_and(|o| o.is_le()))
            }
        }
    })
}

// Reihenfolge zweier Property-Werte wie Comparable in Vanilla: Zahlen numerisch, false < true.
// Aufzaehlungen vergleicht Vanilla nach Deklaration -- die kennen wir nicht, dort zaehlt nur
// Gleichheit.
fn property_order(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    if let (Ok(x), Ok(y)) = (a.parse::<i64>(), b.parse::<i64>()) {
        return Some(x.cmp(&y));
    }
    if let (Ok(x), Ok(y)) = (a.parse::<bool>(), b.parse::<bool>()) {
        return Some(x.cmp(&y));
    }
    (a == b).then_some(std::cmp::Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mit dem echten Dump: die Standards der Vanilla-Items muessen dieselben Werte ergeben wie
    // die Namenstabellen, die bisher galten. Braucht die extrahierten Daten, deshalb #[ignore].
    // Kampfwerte aus den Standards: Diamantschwert +6 Schaden, Diamant-Brustpanzer 8 Ruestung
    // und 2 Haerte. Braucht die extrahierten Daten.
    #[test]
    #[ignore]
    fn standards_tragen_attribute() {
        let pfad = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share/coral-client/storage/1.21.11/item_components.dat");
        crate::item_components::load(&std::fs::read_to_string(pfad).unwrap());
        let mods = |n: &str| {
            let st = ItemStack::new(reg::item_id_by_name(n).unwrap(), 1);
            crate::item_components::Components::of(&st).attribute_modifiers().iter().map(|m| (m.attribute, m.amount, m.slot)).collect::<Vec<_>>()
        };
        let schwert = mods("diamond_sword");
        assert!(schwert.contains(&(protocol::item::ATTRIBUTE_ATTACK_DAMAGE, 6.0, protocol::item::SLOT_GROUP_MAINHAND)), "{schwert:?}");
        let brust = mods("diamond_chestplate");
        assert!(brust.iter().any(|&(a, v, _)| a == protocol::item::ATTRIBUTE_ARMOR && v == 8.0), "{brust:?}");
        assert!(brust.iter().any(|&(a, v, _)| a == protocol::item::ATTRIBUTE_ARMOR_TOUGHNESS && v == 2.0), "{brust:?}");
        // Speere tragen eine eigene Reichweite, Schwerter nicht.
        let speer = ItemStack::new(reg::item_id_by_name("diamond_spear").unwrap(), 1);
        let r = crate::item_components::Components::of(&speer).attack_range().copied();
        assert!(r.is_some_and(|r| r.max_reach > 3.0));
        let schwert = ItemStack::new(reg::item_id_by_name("diamond_sword").unwrap(), 1);
        assert!(crate::item_components::Components::of(&schwert).attack_range().is_none());
        // Essen kruemelt, Getraenke nicht.
        let st = |n: &str| ItemStack::new(reg::item_id_by_name(n).unwrap(), 1);
        assert!(consume_particles(&st("bread")));
        assert!(!consume_particles(&st("potion")));
        assert!(!consume_particles(&st("milk_bucket")));
        assert!(!consume_particles(&st("diamond_sword")));
    }

    #[test]
    #[ignore]
    fn standards_stimmen_mit_den_tabellen_ueberein() {
        let pfad = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share/coral-client/storage/1.21.11/item_components.dat");
        crate::item_components::load(&std::fs::read_to_string(pfad).unwrap());
        assert!(loaded());
        let s = |n: &str| ItemStack::new(reg::item_id_by_name(n).unwrap(), 1);
        for n in reg::all_item_names() {
            let st = s(n);
            assert_eq!(used_continuously(&st), reg::is_used_continuously(n), "{n}: Dauerbenutzung");
            assert_eq!(consume_ticks(&st), reg::eat_duration_ticks(n), "{n}: Essdauer");
            assert_eq!(needs_hunger(&st), reg::needs_hunger(n), "{n}: braucht Hunger");
            assert_eq!(piercing(&st), reg::is_spear(n), "{n}: Stich");
            assert_eq!(minimum_attack_charge(&st), if reg::is_spear(n) { 1.0 } else { 0.0 }, "{n}: Mindestladung");
            assert_eq!(kinetic(&st), reg::spear_kinetic(n), "{n}: Ansturm");
            assert_eq!(swing(&st).1, reg::swing_duration_ticks(n), "{n}: Schwungdauer");
            assert_eq!(death_protection(&st), n == "totem_of_undying", "{n}: Totem");
            assert_eq!(is_drink(&st), consume_ticks(&st).is_some() && matches!(n.as_str(), "potion" | "milk_bucket" | "honey_bottle" | "ominous_bottle"), "{n}: trinkt");
            // Bis auf Koepfe und Kuerbis (swappable false) dieselben Schwuenge wie die Tabelle.
            let kopf = n.ends_with("_head") || n.ends_with("_skull") || n == "carved_pumpkin";
            assert_eq!(air_use_swings(&st), reg::air_use_swings(n) && !kopf, "{n}: Schwung ins Leere");
            assert_eq!(blocks_attacks(&st).is_some(), n == "shield", "{n}: blockt");
            assert_eq!(can_glide_with(&st, 4), n == "elytra", "{n}: gleitet");
        }
        // Speere sprinten beim Ansturm, alles andere wird auf ein Fuenftel gebremst.
        assert!(use_effects(&s("iron_spear")).can_sprint);
        assert_eq!(use_effects(&s("bow")).speed_multiplier, DEFAULT_USE_SPEED);
        // Vom Server veraendert: ein Totem ohne death_protection rettet nicht, ein Apfel mit
        // glider gleitet (wenn er auf der Brust sitzt).
        let mut totem = s("totem_of_undying");
        totem.removed_components.push(COMPONENT_DEATH_PROTECTION);
        assert!(!death_protection(&totem));
        let mut elytra = s("elytra");
        elytra.damage = Some(431);
        assert!(!can_glide_with(&elytra, 4), "kurz vor dem Zerbrechen");
        elytra.damage = Some(430);
        assert!(can_glide_with(&elytra, 4));
        assert!(!can_glide_with(&elytra, 5), "nur im eigenen Slot");
        assert_eq!(reg::max_stack_size(s("ender_pearl").item_id), Components::of(&s("ender_pearl")).max_stack_size());
        // Ruestung zieht der Rechtsklick an, Pferderuestung nicht -- und Koepfe und der
        // geschnitzte Kuerbis auch nicht (swappable false: Item.use gibt PASS).
        assert!(equips_on_use(&s("diamond_helmet")) && equips_on_use(&s("elytra")) && equips_on_use(&s("turtle_helmet")));
        assert!(!equips_on_use(&s("carved_pumpkin")) && !equips_on_use(&s("player_head")));
        assert!(!equips_on_use(&s("diamond_horse_armor")) && !equips_on_use(&s("wolf_armor")) && !equips_on_use(&s("diamond_sword")));
    }

    // can_break mit Blockliste und Property-Bereich: Stein ja, Erde nein; Weizen erst ab Alter 7.
    #[test]
    fn abenteuer_praedikate() {
        use protocol::item::{BlockPredicate, StateValueMatcher};
        let state = |name: &str, props: &[(&str, &str)]| {
            (0..crate::block_registry::state_count() as u32)
                .find(|&s| crate::block_name(s) == Some(name) && props.iter().all(|(k, v)| crate::state_props::property(s, k) == Some(*v)))
                .unwrap()
        };
        let stein = state("stone", &[]);
        let id = crate::mining::block_id(stein).unwrap() as i32;
        let nur_stein = [BlockPredicate { blocks: Some((None, vec![id])), properties: vec![], needs_block_entity: false }];
        assert!(adventure_allows(Some(&nur_stein), stein));
        assert!(!adventure_allows(Some(&nur_stein), state("dirt", &[])));
        assert!(!adventure_allows(None, stein), "ohne Component nichts");
        let reif = [BlockPredicate { blocks: None, properties: vec![("age".into(), StateValueMatcher::Range(Some("7".into()), None))], needs_block_entity: false }];
        assert!(adventure_allows(Some(&reif), state("wheat", &[("age", "7")])));
        assert!(!adventure_allows(Some(&reif), state("wheat", &[("age", "3")])));
        assert!(!adventure_allows(Some(&reif), stein), "Stein hat kein Alter");
        let tag = [BlockPredicate { blocks: Some((Some("minecraft:mineable/pickaxe".into()), vec![])), properties: vec![], needs_block_entity: false }];
        assert!(adventure_allows(Some(&tag), stein));
        assert!(!adventure_allows(Some(&tag), state("dirt", &[])));
    }
}
