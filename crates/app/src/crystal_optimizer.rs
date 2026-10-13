// Marlow's Crystal Optimizer (Fabric-Mod von Bram/Marlow, MIT, 1.0.5 fuer 1.21.11), nachgebaut.
//
// Was die Mod tut: sobald der Client ein ATTACK-Paket auf einen End-Kristall schickt,
// verschwindet der Kristall SOFORT aus der eigenen Welt -- statt eine volle Rundreise lang
// auf das Entfernen-Paket des Servers zu warten. Der naechste Klick trifft damit schon das,
// was dahinter liegt (typisch: der Obsidian fuer den naechsten Kristall).
//
// Nur wenn der Schlag den Kristall auch zerstoeren kann: mit Schwaeche kann ein Faustschlag
// (oder eine zu schwache Waffe) 0 Schaden machen, dann bleibt der Kristall auf dem Server
// stehen -- und waere hier verschwunden, obwohl er noch da ist.
//
// Server duerfen die Mod abschalten: sie schicken `marlowcrystal:opt_out`, der Client antwortet
// mit `marlowcrystal:opt_out_ack`. Beim Beitreten meldet er seine Version auf
// `marlowcrystal:version` -- genau wie die Mod.

// Kanaele der Mod (ModPackets.id: Namensraum "marlowcrystal").
pub const CHANNEL_VERSION: &str = "marlowcrystal:version";
pub const CHANNEL_OPT_OUT: &str = "marlowcrystal:opt_out";
pub const CHANNEL_OPT_OUT_ACK: &str = "marlowcrystal:opt_out_ack";

// Die Version, die wir melden -- die der nachgebauten Mod. Ein Server, der Versionen prueft,
// soll dasselbe Verhalten erwarten koennen.
pub const VERSION: (i32, i32, i32) = (1, 0, 5);

// Inhalt von `marlowcrystal:version` (VersionPacket.STREAM_CODEC): drei VarInts + Snapshot-Flag.
pub fn version_data() -> Vec<u8> {
    let mut w = protocol::buffer::PacketWriter::new();
    w.write_varint(VERSION.0).write_varint(VERSION.1).write_varint(VERSION.2).write_bool(false);
    w.into_bytes()
}

// Die Attributmodifikatoren fuer ATTACK_DAMAGE in der Haupthand, je Item -- MASCHINELL aus dem
// 1.21.11-Jar gedumpt (Bootstrap + ItemStack.applyAttributeModifiers(MAINHAND)), nicht
// abgeschrieben. Alles, was hier fehlt, hat keinen solchen Modifikator (Summe 0).
//
// Wie die Mod (InteractHandler.getWeaponDamage) nur die STANDARD-Werte des Items: eigene
// attribute_modifiers-Komponenten auf einem Stapel liest dieser Client nicht mit.
const WEAPON_DAMAGE: [(&str, f64); 35] = [
    ("wooden_sword", 3.0),
    ("wooden_shovel", 1.5),
    ("wooden_pickaxe", 1.0),
    ("wooden_axe", 6.0),
    ("copper_sword", 4.0),
    ("copper_shovel", 2.5),
    ("copper_pickaxe", 2.0),
    ("copper_axe", 8.0),
    ("stone_sword", 4.0),
    ("stone_shovel", 2.5),
    ("stone_pickaxe", 2.0),
    ("stone_axe", 8.0),
    ("golden_sword", 3.0),
    ("golden_shovel", 1.5),
    ("golden_pickaxe", 1.0),
    ("golden_axe", 6.0),
    ("iron_sword", 5.0),
    ("iron_shovel", 3.5),
    ("iron_pickaxe", 3.0),
    ("iron_axe", 8.0),
    ("diamond_sword", 6.0),
    ("diamond_shovel", 4.5),
    ("diamond_pickaxe", 4.0),
    ("diamond_axe", 8.0),
    ("netherite_sword", 7.0),
    ("netherite_shovel", 5.5),
    ("netherite_pickaxe", 5.0),
    ("netherite_axe", 9.0),
    ("mace", 5.0),
    ("stone_spear", 1.0),
    ("copper_spear", 1.0),
    ("iron_spear", 2.0),
    ("diamond_spear", 3.0),
    ("netherite_spear", 4.0),
    ("trident", 8.0),
];

// Rohe IDs der Statuseffekte auf der Leitung (s. protocol::packets::MOB_EFFECTS).
pub const MOB_EFFECT_STRENGTH: i32 = 4;
pub const MOB_EFFECT_WEAKNESS: i32 = 17;

// Grundwert des Attributs ATTACK_DAMAGE beim Spieler (getAttributeBaseValue).
const BASE_ATTACK_DAMAGE: f64 = 1.0;

pub fn weapon_damage(item: Option<&str>) -> f64 {
    item.and_then(|name| WEAPON_DAMAGE.iter().find(|(n, _)| *n == name)).map_or(0.0, |(_, d)| *d)
}

// InteractHandler.canDestroyCrystal. `weakness`/`strength` sind die Verstaerkungsstufen
// (0 = Stufe I), None = Effekt nicht aktiv.
pub fn can_destroy_crystal(main_hand: Option<&str>, weakness: Option<i32>, strength: Option<i32>) -> bool {
    let Some(weakness) = weakness else { return true };
    let weakness_penalty = 4.0 * (weakness + 1) as f64;
    if BASE_ATTACK_DAMAGE > weakness_penalty + 5.0 {
        return true;
    }
    let strength_bonus = strength.map_or(0.0, |s| 3.0 * (s + 1) as f64);
    let total = (BASE_ATTACK_DAMAGE + weapon_damage(main_hand) + strength_bonus - weakness_penalty).max(0.0);
    total > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ohne_schwaeche_immer() {
        assert!(can_destroy_crystal(None, None, None));
        assert!(can_destroy_crystal(Some("stick"), None, Some(1)));
    }

    #[test]
    fn schwaeche_braucht_genug_waffe() {
        // Schwaeche I: 1 + Waffe - 4 > 0.
        assert!(!can_destroy_crystal(None, Some(0), None), "Faust gegen Schwäche I");
        assert!(!can_destroy_crystal(Some("wooden_sword"), Some(0), None), "1 + 3 - 4 = 0 reicht nicht");
        assert!(can_destroy_crystal(Some("stone_sword"), Some(0), None));
        assert!(can_destroy_crystal(Some("diamond_sword"), Some(0), None));
        // Staerke gleicht aus.
        assert!(!can_destroy_crystal(None, Some(0), Some(0)), "Faust + Stärke I: 1 + 0 + 3 - 4 = 0");
        assert!(can_destroy_crystal(None, Some(0), Some(1)), "Faust + Stärke II: 1 + 6 - 4 = 3");
    }

    #[test]
    fn effekt_ids_passen_zur_registry() {
        assert_eq!(protocol::packets::mob_effect_name(MOB_EFFECT_STRENGTH), "strength");
        assert_eq!(protocol::packets::mob_effect_name(MOB_EFFECT_WEAKNESS), "weakness");
    }

    #[test]
    fn versionspaket_wie_die_mod() {
        assert_eq!(version_data(), vec![1, 0, 5, 0]);
    }
}
