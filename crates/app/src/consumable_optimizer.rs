// Consumable Optimizer (Fabric-Mod von Walksy, MIT), nachgebaut -- dazu die Zeitrechnung fuer
// den Benutzungs-Timer am Fadenkreuz.
//
// Was die Mod tut: Vanilla wartet nach dem letzten Bissen auf den Server -- erst sein
// "fertig" beendet die Essanimation. Mit Ping steht man so eine volle Rundreise lang weiter
// kauend da. Die Mod beendet das Essen im Client genau in dem Tick, in dem es fertig ist (der
// Server rechnet dieselben Ticks und kommt zum selben Ergebnis), spielt Ess- und Ruelpsklang
// selbst und verwirft die verspaeteten Meldungen des Servers:
//   - sein "benutzt Item"-Flag, solange wir selbst essen (sonst setzt ein veraltetes Flag das
//     Essen neu an -- das "Neustarten" bei hohem Ping),
//   - nach dem Fertigessen dasselbe Flag noch einmal, bis er "fertig" meldet,
//   - seinen Ess- und Ruelpsklang, die wir schon gespielt haben.
// Die Wirkung (Absorption, Regeneration, Saettigung) setzt weiter allein der Server.
//
// Die Ess-Verlangsamung endet mit dem lokalen Ende, ohne auf den Server zu warten -- auch wenn
// er (und mit ihm Grim, s. PacketSelfMetadataListener) uns bis zu seiner Meldung noch fuer
// essend haelt. Auf pvphq kam diese Meldung gemessen nie innerhalb von 500 ms.
//
// Server duerfen die Mod abschalten: beim Beitreten meldet sich der Client auf
// `consumable_optimizer:handshake_payload`, ein Plugin antwortet auf
// `consumable_optimizer:disable_payload` -- genau wie die Mod.

use std::time::{Duration, Instant};

pub const CHANNEL_HANDSHAKE: &str = "consumable_optimizer:handshake_payload";
pub const CHANNEL_DISABLE: &str = "consumable_optimizer:disable_payload";

// So lange wartet die Mod hoechstens auf die Bestaetigung des Servers.
const SERVER_WAIT: Duration = Duration::from_millis(500);
// So lange gilt ein vorgemerkter Klang als "schon gespielt".
const SOUND_SUPPRESS: Duration = Duration::from_millis(1500);

// Essdauer in Ticks: die consumable-Component des Stapels (Vanilla: Sekunden mal 20, abgeschnitten),
// sonst der Standard des Items. None = wird nicht gegessen/getrunken.
pub fn consume_ticks(name: &str, stack: Option<&protocol::item::ItemStack>) -> Option<u32> {
    match stack {
        Some(st) => world::item_behavior::consume_ticks(st),
        None => world::item_registry::eat_duration_ticks(name),
    }
}

// Getraenke (UseAction DRINK) -- die anderen werden gegessen und enden mit einem Ruelpser.
fn is_drink(name: &str) -> bool {
    matches!(name, "potion" | "milk_bucket" | "honey_bottle" | "ominous_bottle")
}

// Der Klang am Ende (Consumable.sound): Vanillas Standards je Art.
pub fn finish_sound(name: &str) -> &'static str {
    match name {
        "honey_bottle" => "item.honey_bottle.drink",
        "ominous_bottle" => "item.ominous_bottle.dispose",
        n if is_drink(n) => "entity.generic.drink",
        _ => "entity.generic.eat",
    }
}

// Ruelpst man danach? Wie die Mod: bei allem, was gegessen wird, und bei der Honigflasche.
pub fn burps(name: &str) -> bool {
    !is_drink(name) || name == "honey_bottle"
}

pub const BURP_SOUND: &str = "entity.player.burp";

#[derive(Default)]
pub struct State {
    // Lokal fertig gegessen, Bestaetigung des Servers steht noch aus.
    waiting_until: Option<Instant>,
    // Wann lokal fertig gegessen -- fuer die Messung, wie lange der Server danach braucht
    // (s. confirm_delay, timed_out).
    finished_at: Option<Instant>,
    // Klaenge, die wir schon gespielt haben und die der Server gleich noch einmal schickt.
    suppressed: Vec<(&'static str, Instant)>,
    // Der Server hat die Mod abgeschaltet (bis zum Trennen).
    pub disabled_by_server: bool,
}

impl State {
    pub fn finished_locally(&mut self, sounds: &[&'static str], now: Instant) {
        self.waiting_until = Some(now + SERVER_WAIT);
        self.finished_at = Some(now);
        self.suppressed = sounds.iter().map(|s| (*s, now + SOUND_SUPPRESS)).collect();
    }

    pub fn waiting(&self, now: Instant) -> bool {
        self.waiting_until.is_some_and(|t| now < t)
    }

    pub fn stop_waiting(&mut self) {
        self.waiting_until = None;
    }

    /// Der Server meldet "fertig": wie lange nach dem lokalen Ende (None, wenn wir nicht
    /// darauf gewartet haben).
    pub fn confirm_delay(&mut self, now: Instant) -> Option<Duration> {
        let seit = self.finished_at.take()?;
        self.waiting_until.is_some_and(|t| now < t).then(|| now.duration_since(seit))
    }

    /// Ist das Warten gerade ohne Meldung des Servers abgelaufen? Einmal je Essen true.
    pub fn timed_out(&mut self, now: Instant) -> bool {
        if self.finished_at.is_some() && self.waiting_until.is_some_and(|t| now >= t) {
            self.finished_at = None;
            return true;
        }
        false
    }

    // Ein Klang vom Server: true = schon selbst gespielt, verwerfen (je Eintrag einmal).
    pub fn swallow_sound(&mut self, name: &str, now: Instant) -> bool {
        self.suppressed.retain(|(_, bis)| now < *bis);
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        match self.suppressed.iter().position(|(s, _)| *s == name) {
            Some(i) => {
                self.suppressed.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn reset(&mut self) {
        *self = State::default();
    }
}

// --- Benutzungs-Timer (Modul "Timer") ---------------------------------------------------------

// Wie viele Ticks Benutzung bis "voll": der Bogen bis zum vollen Spannen (BowItem.getPullProgress
// erreicht 1 nach 20 Ticks -- erst dann ist der Pfeil kritisch), die Armbrust bis sie geladen
// ist, der Dreizack bis zum Wurf, Essen und Trinken bis zum letzten Bissen. None = kein Timer.
pub fn timer_ticks(name: &str, stack: Option<&protocol::item::ItemStack>, crossbow_pull: u32) -> Option<u32> {
    match name {
        "bow" => Some(20),
        "crossbow" => Some(crossbow_pull),
        "trident" => Some(10),
        _ => consume_ticks(name, stack),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dauern_und_klaenge() {
        assert_eq!(consume_ticks("golden_apple", None), Some(32));
        assert_eq!(consume_ticks("dried_kelp", None), Some(16));
        assert_eq!(consume_ticks("diamond_sword", None), None);
        let mut kopf = protocol::item::ItemStack::new(1, 1);
        kopf.consume_seconds = Some(0.8);
        assert_eq!(consume_ticks("honey_bottle", Some(&kopf)), Some(16));
        kopf.consume_seconds = None;
        kopf.removed_components.push(protocol::item::COMPONENT_CONSUMABLE);
        assert_eq!(consume_ticks("apple", Some(&kopf)), None);

        assert_eq!(finish_sound("golden_apple"), "entity.generic.eat");
        assert_eq!(finish_sound("potion"), "entity.generic.drink");
        assert!(burps("golden_apple") && burps("honey_bottle") && !burps("potion"));
        assert_eq!(timer_ticks("bow", None, 25), Some(20));
        assert_eq!(timer_ticks("crossbow", None, 15), Some(15));
        assert_eq!(timer_ticks("shield", None, 25), None);
    }

    #[test]
    fn klaenge_werden_je_einmal_verschluckt() {
        let mut s = State::default();
        let t = Instant::now();
        s.finished_locally(&["entity.generic.eat", BURP_SOUND], t);
        assert!(s.waiting(t));
        assert!(!s.waiting(t + Duration::from_millis(600)));
        assert!(s.swallow_sound("minecraft:entity.generic.eat", t));
        assert!(!s.swallow_sound("minecraft:entity.generic.eat", t), "nur einmal");
        assert!(!s.swallow_sound("minecraft:entity.player.burp", t + Duration::from_secs(2)), "abgelaufen");
    }
}
