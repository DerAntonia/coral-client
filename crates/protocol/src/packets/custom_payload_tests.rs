use super::*;

#[test]
fn plugin_nachricht_hin_und_zurueck() {
    let bytes = custom_payload("marlowcrystal:version", &[1, 0, 5, 0]);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CUSTOM_PAYLOAD);
    // Clientbound hat dasselbe Layout, nur eine andere Nummer vorn.
    let mut cb = PacketWriter::new();
    cb.write_varint(CB_PLAY_CUSTOM_PAYLOAD).write_string("marlowcrystal:opt_out");
    assert_eq!(read_custom_payload(&cb.into_bytes()).unwrap(), ("marlowcrystal:opt_out".to_string(), vec![]));
    let reg = register_channels_payload(&["a:b", "c:d"]);
    let (kanal, daten) = read_custom_payload(&reg).unwrap();
    assert_eq!((kanal.as_str(), daten.as_slice()), ("minecraft:register", b"a:b\0c:d".as_slice()));
}
