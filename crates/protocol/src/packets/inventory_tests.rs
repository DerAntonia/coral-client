use super::*;

// Set Container Content mit zwei Slots + Cursor: der Aufbau muss exakt aufgehen, sonst
// liest sich die Slot-Liste schief (der haeufigste Fehlerfall bei Inventar-Paketen).
#[test]
fn container_content_liest_slots_und_cursor() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_CONTAINER_SET_CONTENT).write_varint(0).write_varint(7).write_varint(2);
    // Slot 0: 64x Stein ohne Components, Slot 1: leer
    w.write_varint(64).write_varint(1).write_varint(0).write_varint(0);
    w.write_varint(0);
    // Cursor: 1x Diamantschwert
    w.write_varint(1).write_varint(936).write_varint(0).write_varint(0);
    let content = read_container_content(&w.into_bytes()).unwrap();
    assert_eq!(content.sync_id, 0);
    assert_eq!(content.revision, 7);
    assert_eq!(content.slots.len(), 2);
    assert_eq!(content.slots[0].as_ref().map(|s| (s.item_id, s.count)), Some((1, 64)));
    assert!(content.slots[1].is_none());
    assert_eq!(content.cursor.as_ref().map(|s| s.item_id), Some(936));
    assert!(content.truncated_at.is_none());
}

// Ein unbekannter Component-Typ darf nur den Rest der Liste kosten, nicht das Paket --
// die vorher gelesenen Slots bleiben erhalten und der Abbruch wird zurueckgegeben.
#[test]
fn unlesbarer_slot_bricht_nur_den_rest_ab() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_CONTAINER_SET_CONTENT).write_varint(1).write_varint(3).write_varint(3);
    w.write_varint(1).write_varint(1).write_varint(0).write_varint(0);
    w.write_varint(1).write_varint(1).write_varint(1).write_varint(0).write_varint(9999);
    w.write_varint(0);
    let content = read_container_content(&w.into_bytes()).unwrap();
    assert_eq!(content.slots.len(), 1);
    assert!(matches!(content.truncated_at, Some((1, _))));
    assert!(content.cursor.is_none());
}

// Slot-Nummer ist ein Short (nicht VarInt) -- bei Slot 300 wuerde ein VarInt anders
// aussehen und der Server den falschen Slot anfassen.
#[test]
fn container_set_slot_liest_short_slotnummer() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_CONTAINER_SET_SLOT).write_varint(2).write_varint(11).write_u16(300);
    w.write_varint(5).write_varint(1).write_varint(0).write_varint(0);
    let update = read_container_set_slot(&w.into_bytes()).unwrap();
    assert_eq!((update.sync_id, update.revision, update.slot), (2, 11, 300));
    assert_eq!(update.stack.map(|s| s.count), Some(5));
}

// Der aktive Hotbar-Slot geht serverbound als Short raus (UpdateSelectedSlotC2SPacket),
// nicht als VarInt -- verwechselt man das, landet man auf Slot 0.
#[test]
fn set_carried_item_schreibt_short() {
    let bytes = set_carried_item_payload(8);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_SET_CARRIED_ITEM);
    assert_eq!(r.read_u16().unwrap(), 8);
    assert!(r.is_exhausted());
}

// Klick-Paket ohne Vorhersage: Reihenfolge stimmt und beide Hash-Felder sind leer.
#[test]
fn click_container_payload_ohne_vorhersage() {
    let bytes = click_container_payload(3, 42, 17, 1, CLICK_MODE_QUICK_MOVE, &[], None);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CONTAINER_CLICK);
    assert_eq!(r.read_varint().unwrap(), 3); // syncId
    assert_eq!(r.read_varint().unwrap(), 42); // revision
    assert_eq!(r.read_i16().unwrap(), 17); // Slot
    assert_eq!(r.read_u8().unwrap(), 1); // Button
    assert_eq!(r.read_varint().unwrap(), CLICK_MODE_QUICK_MOVE);
    assert_eq!(r.read_varint().unwrap(), 0); // keine veraenderten Slots
    assert!(!r.read_bool().unwrap()); // Cursor-Hash: leer
    assert!(r.is_exhausted());
}

// Mit Vorhersage: ein veraenderter Slot samt Inhalt und ein gefuellter Cursor-Hash.
// Genau dieses Feld entscheidet, ob der Server den Slot korrigierend nachschickt.
#[test]
fn click_container_payload_mit_vorhersage() {
    let slot_stack = ItemStack::new(1, 32); // 32x Stein bleiben im Slot
    let cursor = ItemStack::new(1, 32); // 32x Stein an der Maus
    let bytes = click_container_payload(0, 5, 9, 0, CLICK_MODE_PICKUP, &[(9, Some(slot_stack))], Some(&cursor));
    let mut r = PacketReader::new(&bytes);
    for _ in 0..3 {
        let _ = r.read_varint();
    }
    assert_eq!(r.read_i16().unwrap(), 9);
    assert_eq!(r.read_u8().unwrap(), 0);
    assert_eq!(r.read_varint().unwrap(), CLICK_MODE_PICKUP);
    assert_eq!(r.read_varint().unwrap(), 1); // ein veraenderter Slot
    assert_eq!(r.read_i16().unwrap(), 9); // dessen Nummer (Short)
    assert!(r.read_bool().unwrap()); // Hash vorhanden
    assert_eq!(r.read_varint().unwrap(), 1); // Item-ID
    assert_eq!(r.read_varint().unwrap(), 32); // Anzahl
    assert_eq!(r.read_varint().unwrap(), 0); // keine gesetzten Components
    assert_eq!(r.read_varint().unwrap(), 0); // keine entfernten Components
    assert!(r.read_bool().unwrap()); // Cursor-Hash vorhanden
    assert_eq!(r.read_varint().unwrap(), 1);
    assert_eq!(r.read_varint().unwrap(), 32);
    assert_eq!(r.read_varint().unwrap(), 0);
    assert_eq!(r.read_varint().unwrap(), 0);
    assert!(r.is_exhausted());
}

// Entfernte Components brauchen keinen Prüfwert, nur ihre ID -- solche Stapel bleiben
// also exakt hashbar.
#[test]
fn entfernte_components_landen_im_hash() {
    let mut stack = ItemStack::new(1, 1);
    stack.removed_components = vec![3, 19];
    let bytes = click_container_payload(0, 1, 9, 0, CLICK_MODE_PICKUP, &[], Some(&stack));
    let mut r = PacketReader::new(&bytes);
    for _ in 0..3 {
        let _ = r.read_varint();
    }
    let _ = r.read_i16();
    let _ = r.read_u8();
    let _ = r.read_varint();
    let _ = r.read_varint();
    assert!(r.read_bool().unwrap());
    assert_eq!(r.read_varint().unwrap(), 1);
    assert_eq!(r.read_varint().unwrap(), 1);
    assert_eq!(r.read_varint().unwrap(), 0); // gesetzte Components: keine
    assert_eq!(r.read_varint().unwrap(), 2); // entfernte: zwei IDs
    assert_eq!(r.read_varint().unwrap(), 3);
    assert_eq!(r.read_varint().unwrap(), 19);
    assert!(r.is_exhausted());
}

// Ein Stapel mit GESETZTEN Components ist nicht exakt hashbar -- er muss trotzdem als
// GEFUELLTER Hash rausgehen (mit leerer Component-Liste), damit der Server die Abweichung
// sieht und korrigiert. Der leere Hash waere hier die Aussage "Cursor ist leer" und damit
// eine Luege, die zufaellig stimmen und die Korrektur verschlucken kann (s.
// write_item_stack_hash).
#[test]
fn stapel_mit_components_meldet_sich_trotzdem_als_vorhanden() {
    let mut stack = ItemStack::new(936, 1);
    stack.has_added_components = true;
    assert!(!stack.hashable());
    let bytes = click_container_payload(0, 1, 9, 0, CLICK_MODE_PICKUP, &[], Some(&stack));
    let mut r = PacketReader::new(&bytes);
    for _ in 0..3 {
        let _ = r.read_varint();
    }
    let _ = r.read_i16();
    let _ = r.read_u8();
    let _ = r.read_varint();
    let _ = r.read_varint();
    assert!(r.read_bool().unwrap(), "vorhandener Stapel darf nie als leer gelesen werden");
    assert_eq!(r.read_varint().unwrap(), 936);
    assert_eq!(r.read_varint().unwrap(), 1);
    assert_eq!(r.read_varint().unwrap(), 0, "leere Component-Liste = die absichtliche Abweichung");
    assert_eq!(r.read_varint().unwrap(), 0);
    assert!(r.is_exhausted());
}

// Und die Gegenprobe: nur ein WIRKLICH leerer Cursor meldet sich als leer.
#[test]
fn leerer_cursor_meldet_sich_als_leer() {
    let bytes = click_container_payload(0, 1, 9, 0, CLICK_MODE_PICKUP, &[], None);
    let mut r = PacketReader::new(&bytes);
    for _ in 0..3 {
        let _ = r.read_varint();
    }
    let _ = r.read_i16();
    let _ = r.read_u8();
    let _ = r.read_varint();
    let _ = r.read_varint();
    assert!(!r.read_bool().unwrap());
    assert!(r.is_exhausted());
}

// Werfen benutzt Vanillas Sonder-Slot -999; als Short muss das negativ ankommen.
#[test]
fn wurf_slot_bleibt_negativ() {
    let bytes = click_container_payload(0, 1, -999, 0, CLICK_MODE_PICKUP, &[], None);
    let mut r = PacketReader::new(&bytes);
    let _ = r.read_varint();
    let _ = r.read_varint();
    let _ = r.read_varint();
    assert_eq!(r.read_i16().unwrap(), -999);
}
