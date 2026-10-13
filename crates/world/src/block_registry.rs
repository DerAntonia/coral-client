// Zuordnung Block-State-ID -> Blockname (z.B. 1 -> "stone", 9 -> "grass_block").
//
// Die State-IDs sind global und versionsabhaengig; die Wahrheit steht in der blocks.json
// des Vanilla-Datengenerators (java net.minecraft.data.Main --reports). Aus dieser wurde
// state_blocks.bin gebacken -- ein kompaktes Binaerabbild, damit wir zur Laufzeit weder
// die 6-MB-JSON parsen noch eine JVM starten muessen. Bei einem Versionswechsel neu
// erzeugen.
//
// Format (Little-Endian): u32 name_count, dann name_count * (u16 len + utf8-Bytes),
// dann u32 state_count, dann state_count * u16 (Index in die Namensliste, per State-ID).

use std::sync::OnceLock;

static REGISTRY: OnceLock<BlockRegistry> = OnceLock::new();

struct BlockRegistry {
    names: Vec<String>,
    // per State-ID der Index in `names`.
    state_to_name: Vec<u16>,
}

fn registry() -> &'static BlockRegistry {
    REGISTRY.get_or_init(|| {
        let data: &[u8] = include_bytes!("state_blocks.bin");
        let mut p = 0usize;
        let read_u32 = |p: &mut usize| {
            let v = u32::from_le_bytes(data[*p..*p + 4].try_into().unwrap());
            *p += 4;
            v
        };
        let read_u16 = |p: &mut usize| {
            let v = u16::from_le_bytes(data[*p..*p + 2].try_into().unwrap());
            *p += 2;
            v
        };

        let name_count = read_u32(&mut p) as usize;
        let mut names = Vec::with_capacity(name_count);
        for _ in 0..name_count {
            let len = read_u16(&mut p) as usize;
            names.push(String::from_utf8(data[p..p + len].to_vec()).expect("Blockname kein UTF-8"));
            p += len;
        }
        let state_count = read_u32(&mut p) as usize;
        let mut state_to_name = Vec::with_capacity(state_count);
        for _ in 0..state_count {
            state_to_name.push(read_u16(&mut p));
        }
        BlockRegistry { names, state_to_name }
    })
}

// Blockname (ohne "minecraft:"-Praefix) zu einer State-ID, oder None fuer unbekannte IDs
// (z.B. neuere States, als die Tabelle gebacken wurde -- dann faellt der Aufrufer sinnvoll
// zurueck statt zu panicken).
pub fn block_name(state: u32) -> Option<&'static str> {
    let reg = registry();
    reg.state_to_name.get(state as usize).map(|&i| reg.names[i as usize].as_str())
}

// Anzahl bekannter State-IDs (0..state_count sind belegt) -- zum Aufbau der UV-Tabelle.
pub fn state_count() -> usize {
    registry().state_to_name.len()
}

// Alle eindeutigen Blocknamen (fuer den Atlas-Bau: einmal jede Textur aufloesen, nicht pro
// State erneut).
pub fn all_block_names() -> Vec<&'static str> {
    registry().names.iter().map(|s| s.as_str()).collect()
}

// Gibt es einen Block dieses Namens? Damit laesst sich ein Item als BLOCK-Item erkennen:
// Vanilla vergibt fuer jeden platzierbaren Block ein gleichnamiges Item.
pub fn is_block(name: &str) -> bool {
    registry().names.iter().any(|n| n == name)
}

// Hoehlenluft und Leerluft: in Vanilla Luft wie jede andere (BlockState.isAir), nur mit
// eigener State-ID -- die Carver-Hoehlen der Weltgenerierung sind damit gefuellt. Hier zaehlt
// ueberall nur State 0 als Luft (chunk::is_solid), und ein Modell haben beide nicht: ohne
// Umrechnung stand jede Zelle als Vollwuerfel mit Fehltextur da, sichtbar sobald man eine
// solche Hoehle von normaler Luft aus anschaute (etwa als magenta-schwarzer Boden
// ueber einem Lavasee). Deshalb wird beim Einlesen alles davon zu State 0.
pub fn air_variants() -> [u32; 2] {
    static IDS: OnceLock<[u32; 2]> = OnceLock::new();
    *IDS.get_or_init(|| {
        let reg = registry();
        let id = |name: &str| {
            reg.names
                .iter()
                .position(|n| n == name)
                .and_then(|i| reg.state_to_name.iter().position(|&s| s as usize == i))
                .map_or(0, |s| s as u32)
        };
        [id("cave_air"), id("void_air")]
    })
}

#[inline]
pub fn normalize_air(state: u32, variants: [u32; 2]) -> u32 {
    if state == variants[0] || state == variants[1] {
        0
    } else {
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoehlenluft_wird_luft() {
        let v = air_variants();
        assert_eq!(block_name(v[0]), Some("cave_air"));
        assert_eq!(block_name(v[1]), Some("void_air"));
        assert_eq!(normalize_air(v[0], v), 0);
        assert_eq!(normalize_air(v[1], v), 0);
        assert_eq!(normalize_air(1, v), 1);
    }

    #[test]
    fn block_items_erkennt_man_am_namen() {
        assert!(is_block("stone"));
        assert!(is_block("oak_stairs") || is_block("oak_planks"));
        assert!(!is_block("diamond_sword"));
        assert!(!is_block("gibtsnicht"));
    }

    #[test]
    fn bekannte_states_stimmen() {
        assert_eq!(block_name(0), Some("air"));
        assert_eq!(block_name(1), Some("stone"));
        assert_eq!(block_name(9), Some("grass_block"));
    }

    #[test]
    fn unbekannte_state_id_ist_none_statt_panic() {
        assert_eq!(block_name(u32::MAX), None);
    }
}
