// Blockstate-Properties pro State-ID (z.B. State X -> {"half": "top", "waterlogged":
// "false"}). Getrennt von block_registry (das kennt nur den Blocknamen): fuer die
// Formauswahl (Blockstate-Variante bzw. Multipart-"when"-Klauseln in assets::shapes)
// braucht es die echten Property-Werte, die der Server im State mitschickt -- ein Zaun
// mit north=true sieht anders aus (und kollidiert anders) als einer mit north=false,
// obwohl beide denselben Blocknamen "oak_fence" tragen.
//
// Datenquelle wie state_blocks.bin: blocks.json des Vanilla-Datengenerators, gebacken mit
// scripts/bake_state_props.py. Bei einem Versionswechsel neu erzeugen.
//
// Format (Little-Endian): u32 key_count, dann key_count * (u16 len + utf8) -- eindeutige
// Property-Namen. Dann u32 value_count, dieselbe Kodierung -- eindeutige Property-Werte.
// Dann u32 state_count, dann je State: u8 prop_count, dann prop_count * (u16 key_index,
// u16 value_index).

use std::sync::OnceLock;

static TABLE: OnceLock<PropsTable> = OnceLock::new();

struct PropsTable {
    keys: Vec<String>,
    values: Vec<String>,
    // Pro State-ID die Liste ihrer (key_index, value_index)-Paare.
    state_props: Vec<Vec<(u16, u16)>>,
}

fn table() -> &'static PropsTable {
    TABLE.get_or_init(|| {
        let data: &[u8] = include_bytes!("state_props.bin");
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
        let read_u8 = |p: &mut usize| {
            let v = data[*p];
            *p += 1;
            v
        };
        let read_string = |p: &mut usize| -> String {
            let len = read_u16(p) as usize;
            let s = String::from_utf8(data[*p..*p + len].to_vec()).expect("Property-String kein UTF-8");
            *p += len;
            s
        };

        let key_count = read_u32(&mut p) as usize;
        let keys: Vec<String> = (0..key_count).map(|_| read_string(&mut p)).collect();

        let value_count = read_u32(&mut p) as usize;
        let values: Vec<String> = (0..value_count).map(|_| read_string(&mut p)).collect();

        let state_count = read_u32(&mut p) as usize;
        let mut state_props = Vec::with_capacity(state_count);
        for _ in 0..state_count {
            let prop_count = read_u8(&mut p) as usize;
            let mut pairs = Vec::with_capacity(prop_count);
            for _ in 0..prop_count {
                let ki = read_u16(&mut p);
                let vi = read_u16(&mut p);
                pairs.push((ki, vi));
            }
            state_props.push(pairs);
        }

        PropsTable { keys, values, state_props }
    })
}

// Iterator ueber (key, value) eines States, jeweils als &str -- leer fuer States ohne
// Properties (z.B. "stone") oder unbekannte State-IDs (ausserhalb der gebackenen Tabelle).
pub fn properties_of(state: u32) -> impl Iterator<Item = (&'static str, &'static str)> {
    let t = table();
    t.state_props
        .get(state as usize)
        .into_iter()
        .flatten()
        .map(move |&(ki, vi)| (t.keys[ki as usize].as_str(), t.values[vi as usize].as_str()))
}

// Wert einer einzelnen Property, oder None (Property nicht gesetzt / State unbekannt).
pub fn property(state: u32, key: &str) -> Option<&'static str> {
    properties_of(state).find(|(k, _)| *k == key).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stein_hat_keine_properties() {
        assert_eq!(properties_of(1).count(), 0);
    }

    #[test]
    fn bekannte_property_ist_auffindbar() {
        // State 0 ist Luft (kein Property-Traeger); irgendein spaeterer State mit "waterlogged"
        // sollte existieren -- Stichprobe, dass die Tabelle ueberhaupt etwas liefert.
        let has_any_waterlogged = (0..2000u32).any(|s| property(s, "waterlogged").is_some());
        assert!(has_any_waterlogged, "keine Property in den ersten 2000 States gefunden");
    }
}
