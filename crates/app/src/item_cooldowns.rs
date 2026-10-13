// Item-Abklingzeiten -- Vanillas ItemCooldownManager, fuer den eigenen Spieler.
//
// Ein Cooldown haengt nicht am Stack, sondern an einer GRUPPE (Identifier): der Item-ID oder
// der cooldown_group seiner use_cooldown-Component (ItemCooldownManager.getGroup). Gesetzt
// wird er vom Server per CooldownUpdateS2CPacket -- etwa 5 Sekunden Schild nach einem
// Axthieb (BlocksAttacksComponent.applyShieldCooldown) -- und bei Items mit use_cooldown
// zusaetzlich vom Client selbst, sobald `ItemStack.use` durchgeht
// (applyRemainderAndCooldown).
//
// Waehrend der Abklingzeit meldet ClientPlayerInteractionManager.interactItem PASS, ohne das
// Item zu benutzen -- das USE_ITEM-Paket geht aber TROTZDEM raus (es wird in
// sendSequencedPacket vor der Pruefung gebaut und immer zurueckgegeben).

use std::collections::HashMap;

use protocol::item::{ItemStack, COMPONENT_USE_COOLDOWN};

// ItemCooldownManager.Entry: Start- und End-Tick auf dem eigenen Tickzaehler.
#[derive(Clone, Copy, Debug)]
struct Entry {
    start: i32,
    end: i32,
}

#[derive(Default)]
pub struct ItemCooldowns {
    entries: HashMap<String, Entry>,
    tick: i32,
}

impl ItemCooldowns {
    // ItemCooldownManager.set: ersetzt einen laufenden Eintrag, statt ihn zu verlaengern.
    pub fn set(&mut self, group: &str, ticks: i32) {
        self.entries.insert(normalize(group), Entry { start: self.tick, end: self.tick + ticks });
    }

    pub fn remove(&mut self, group: &str) {
        self.entries.remove(&normalize(group));
    }

    // Einmal pro Spieler-Tick (PlayerEntity.tick -> itemCooldownManager.update).
    pub fn update(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        self.entries.retain(|_, e| e.end > tick);
    }

    // ItemCooldownManager.getCooldownProgress: 1 direkt nach dem Setzen, 0 = frei.
    pub fn progress(&self, stack: &ItemStack, tick_progress: f32) -> f32 {
        let Some(group) = group_of(stack) else { return 0.0 };
        let Some(e) = self.entries.get(&group) else { return 0.0 };
        let gesamt = (e.end - e.start) as f32;
        let rest = e.end as f32 - (self.tick as f32 + tick_progress);
        (rest / gesamt).clamp(0.0, 1.0)
    }

    // Restzeit in Ticks (mit Zwischentick-Anteil), 0 = frei.
    pub fn remaining_ticks(&self, stack: &ItemStack, tick_progress: f32) -> f32 {
        let Some(group) = group_of(stack) else { return 0.0 };
        let Some(e) = self.entries.get(&group) else { return 0.0 };
        (e.end as f32 - (self.tick as f32 + tick_progress)).max(0.0)
    }

    pub fn is_cooling_down(&self, stack: &ItemStack) -> bool {
        self.progress(stack, 0.0) > 0.0
    }

    // Laeuft irgendeine Abklingzeit? Nur ein billiger Vorab-Test fuers Zeichnen.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// Identifier.of ohne Namespace heisst "minecraft:".
fn normalize(group: &str) -> String {
    if group.contains(':') {
        group.to_string()
    } else {
        format!("minecraft:{group}")
    }
}

// Die wirksame use_cooldown-Component: gesetzt am Stack, entfernt, oder der Item-Default.
fn use_cooldown(stack: &ItemStack) -> Option<(f32, Option<String>)> {
    if let Some(c) = &stack.use_cooldown {
        return Some((c.seconds, c.group.clone()));
    }
    if stack.removed_components.contains(&COMPONENT_USE_COOLDOWN) {
        return None;
    }
    let name = world::item_registry::item_name(stack.item_id)?;
    default_use_cooldown_seconds(name).map(|s| (s, None))
}

// Die Items mit use_cooldown ab Werk -- `.useCooldown(...)` in Items.java (1.21.11).
fn default_use_cooldown_seconds(item: &str) -> Option<f32> {
    match item {
        "ender_pearl" | "chorus_fruit" => Some(1.0),
        "wind_charge" => Some(0.5),
        _ => None,
    }
}

// ItemCooldownManager.getGroup.
pub fn group_of(stack: &ItemStack) -> Option<String> {
    let name = world::item_registry::item_name(stack.item_id)?;
    let item_group = format!("minecraft:{name}");
    Some(match use_cooldown(stack) {
        Some((_, Some(g))) => normalize(&g),
        _ => item_group,
    })
}

// UseCooldownComponent.getCooldownTicks: `(int)(seconds * 20)` -- abgeschnitten, nicht
// gerundet. None, wenn das Item keine use_cooldown-Component hat.
pub fn use_cooldown_ticks(stack: &ItemStack) -> Option<i32> {
    use_cooldown(stack).map(|(s, _)| (s * 20.0) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::item::UseCooldown;

    fn item(name: &str) -> ItemStack {
        ItemStack::new(world::item_registry::item_id_by_name(name).unwrap(), 1)
    }

    #[test]
    fn schild_cooldown_laeuft_ab() {
        let mut c = ItemCooldowns::default();
        let schild = item("shield");
        c.set("minecraft:shield", 100);
        assert!(c.is_cooling_down(&schild));
        assert!((c.progress(&schild, 0.0) - 1.0).abs() < 1e-6);
        for _ in 0..50 {
            c.update();
        }
        assert!((c.progress(&schild, 0.0) - 0.5).abs() < 1e-6);
        for _ in 0..50 {
            c.update();
        }
        assert!(!c.is_cooling_down(&schild), "nach genau 100 Ticks frei");
        assert!(c.is_empty());
    }

    #[test]
    fn null_ticks_und_andere_items() {
        let mut c = ItemCooldowns::default();
        c.set("shield", 100);
        assert!(c.is_cooling_down(&item("shield")), "Gruppe ohne Namespace = minecraft:");
        assert!(!c.is_cooling_down(&item("ender_pearl")));
        c.remove("minecraft:shield");
        assert!(!c.is_cooling_down(&item("shield")));
    }

    #[test]
    fn gruppe_aus_use_cooldown_component() {
        let mut perle = item("ender_pearl");
        assert_eq!(use_cooldown_ticks(&perle), Some(20));
        assert_eq!(use_cooldown_ticks(&item("wind_charge")), Some(10));
        assert_eq!(use_cooldown_ticks(&item("shield")), None);
        perle.use_cooldown = Some(UseCooldown { seconds: 2.0, group: Some("pvp:werfen".into()) });
        assert_eq!(group_of(&perle).as_deref(), Some("pvp:werfen"));
        assert_eq!(use_cooldown_ticks(&perle), Some(40));
        let mut ohne = item("ender_pearl");
        ohne.removed_components.push(COMPONENT_USE_COOLDOWN);
        assert_eq!(use_cooldown_ticks(&ohne), None);
    }
}
