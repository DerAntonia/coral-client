// Prueft die Rohbytes aus ~/.config/coral-client/unlesbare_slots.log (s. net::dump_unreadable_slot)
// gegen den aktuellen Leser. Ignoriert, weil die Datei nur auf dem Rechner des Nutzers liegt:
//   cargo test -p protocol --test unlesbare_slots_real -- --ignored --nocapture
use protocol::buffer::PacketReader;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

#[test]
#[ignore]
fn alle_gespeicherten_slots_sind_jetzt_lesbar() {
    let pfad = std::path::Path::new(&std::env::var("HOME").unwrap()).join(".config/coral-client/unlesbare_slots.log");
    let text = std::fs::read_to_string(pfad).expect("Log fehlt");
    let mut zeilen = text.lines();
    let mut n = 0;
    while let Some(kopf) = zeilen.next() {
        if !kopf.starts_with("Fenster") {
            continue;
        }
        let bytes = hex(zeilen.next().unwrap_or(""));
        let mut r = PacketReader::new(&bytes);
        if kopf.contains("Container-Slot") {
            // Ganzes Paket: ID, Fenster, Revision, Slot (i16), dann der Stack.
            r.read_varint().unwrap();
            r.read_varint().unwrap();
            r.read_varint().unwrap();
            r.read_i16().unwrap();
        }
        let ergebnis = protocol::item::read_optional_slot(&mut r);
        assert!(ergebnis.is_ok(), "{kopf}\n-> {:?} {:?}", ergebnis.err(), protocol::item::last_slot_failure());
        n += 1;
    }
    println!("{n} Slots gelesen");
}
