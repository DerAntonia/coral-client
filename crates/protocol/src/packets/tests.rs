use super::*;

// Rechtsklick auf ein Entity: Aufbau aus PlayerInteractEntityC2SPacket.write --
// EntityId, Typ, typabhaengige Felder, zuletzt Sneaking.
#[test]
fn interact_pakete_haben_vanillas_aufbau() {
    let p = interact_entity_payload(300, HAND_OFF, true);
    assert_eq!(p, vec![SB_PLAY_INTERACT as u8, 0xAC, 0x02, 0, 1, 1]);
    let p = interact_at_entity_payload(5, (0.5, 1.0, -0.25), HAND_MAIN, false);
    let mut erwartet = vec![SB_PLAY_INTERACT as u8, 5, 2];
    for f in [0.5f32, 1.0, -0.25] {
        erwartet.extend_from_slice(&f.to_be_bytes());
    }
    erwartet.extend_from_slice(&[0, 0]);
    assert_eq!(p, erwartet);
}

// Gegenprobe zur Bit-Aufteilung aus ChunkPos.asLong (verifiziert am 
// 1.21.11-Client): x in die unteren, z in die oberen 32 Bit, beide signed. Negative
// Koordinaten sind der eigentliche Testfall -- ein falscher Cast (u32 statt i32) faellt
// nur dort auf.
#[test]
fn forget_level_chunk_entpackt_negative_koordinaten() {
    let (x, z) = (-5i32, 7i32);
    let packed = (i64::from(x) & 0xFFFF_FFFF) | ((i64::from(z) & 0xFFFF_FFFF) << 32);
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_FORGET_LEVEL_CHUNK).write_i64(packed);
    let payload = w.into_bytes();

    assert_eq!(read_forget_level_chunk(&payload).unwrap(), (-5, 7));
}

// Gegenprobe zur Bit-Aufteilung aus BlockPos.asLong (X 26 Bit @ Shift 38, Z 26 Bit @
// Shift 12, Y 12 Bit @ Shift 0, verifiziert am 1.21.11-Client). Negative
// Koordinaten in allen drei Achsen, damit ein falscher Cast/Shift auffaellt.
#[test]
fn block_update_entpackt_position_und_state() {
    let (x, y, z, state) = (-12i32, -37i32, 200i32, 42i32);
    let packed = ((i64::from(x) & 0x3FF_FFFF) << 38) | ((i64::from(z) & 0x3FF_FFFF) << 12) | (i64::from(y) & 0xFFF);
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_BLOCK_UPDATE).write_i64(packed).write_varint(state);
    let payload = w.into_bytes();

    assert_eq!(read_block_update(&payload).unwrap(), (-12, -37, 200, 42));
}

// Gegenprobe zur Bit-Aufteilung aus SectionPos.asLong (X 22 Bit @ Shift 42, Y 20 Bit @
// Shift 0, Z 22 Bit @ Shift 20 -- ANDERE Aufteilung als bei BlockPos, s. Kommentar an
// read_section_blocks_update) und zum VarLong-Eintragsformat (State<<12 | X<<8|Z<<4|Y).
#[test]
fn section_blocks_update_entpackt_mehrere_bloecke() {
    let (sx, sy, sz) = (-2i32, 3i32, 100i32);
    let packed = ((i64::from(sx) & 0x3F_FFFF) << 42) | (i64::from(sy) & 0xF_FFFF) | ((i64::from(sz) & 0x3F_FFFF) << 20);

    // write_varint schreibt nur 32 Bit -- fuer den Test die VarLong-Bytes von Hand
    // anhaengen (dieselbe 7-Bit-Continuation-Kodierung, nur mit i64-Werten).
    fn varlong_bytes(mut v: i64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (v as u64 & 0x7F) as u8;
            v = ((v as u64) >> 7) as i64;
            if v != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if v == 0 {
                break;
            }
        }
        out
    }

    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SECTION_BLOCKS_UPDATE).write_i64(packed).write_varint(2);
    let mut payload = w.into_bytes();
    let entry1 = (7i64 << 12) | (1i64 << 8) | (3i64 << 4) | 2i64;
    let entry2 = (99i64 << 12) | (15i64 << 8) | 15i64; // lokales z = 0
    payload.extend(varlong_bytes(entry1));
    payload.extend(varlong_bytes(entry2));

    let blocks = read_section_blocks_update(&payload).unwrap();
    assert_eq!(blocks, vec![(sx * 16 + 1, sy * 16 + 2, sz * 16 + 3, 7), (sx * 16 + 15, sy * 16 + 15, sz * 16, 99)]);
}

// Baut von Hand ein Registry-Data-Paket mit zwei Biomen nach (eines mit vollen NBT-
// Daten, eines ohne -- der "hat der Server ueberhaupt Daten mitgeschickt"-Fall aus dem
// echten Protokoll), um read_registry_data() end-to-end zu pruefen.
#[test]
fn liest_biom_registry_mit_und_ohne_nbt_daten() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_CONFIG_REGISTRY_DATA);
    w.write_string("minecraft:worldgen/biome");
    w.write_varint(2);

    // Biom 0: "minecraft:swamp" mit temperature/downfall/effects.water_color.
    w.write_string("minecraft:swamp");
    w.write_bool(true);
    let mut nbt_bytes = vec![10u8]; // TAG_Compound (Wurzel, ohne Namen)
    nbt_bytes.push(5); // TAG_Float
    nbt_bytes.extend_from_slice(&11u16.to_be_bytes());
    nbt_bytes.extend_from_slice(b"temperature");
    nbt_bytes.extend_from_slice(&0.8f32.to_be_bytes());
    nbt_bytes.push(5); // TAG_Float
    nbt_bytes.extend_from_slice(&8u16.to_be_bytes());
    nbt_bytes.extend_from_slice(b"downfall");
    nbt_bytes.extend_from_slice(&0.9f32.to_be_bytes());
    nbt_bytes.push(10); // TAG_Compound "effects"
    nbt_bytes.extend_from_slice(&7u16.to_be_bytes());
    nbt_bytes.extend_from_slice(b"effects");
    nbt_bytes.push(3); // TAG_Int "water_color"
    nbt_bytes.extend_from_slice(&11u16.to_be_bytes());
    nbt_bytes.extend_from_slice(b"water_color");
    nbt_bytes.extend_from_slice(&0x617B64i32.to_be_bytes());
    nbt_bytes.push(0); // TAG_End (effects)
    nbt_bytes.push(0); // TAG_End (Wurzel)
    for b in nbt_bytes {
        w.write_u8(b);
    }

    // Biom 1: "minecraft:plains", keine NBT-Daten -> Standardwerte.
    w.write_string("minecraft:plains");
    w.write_bool(false);

    let payload = w.into_bytes();
    let biomes = read_registry_data(&payload).unwrap().expect("Biom-Registry erwartet");

    assert_eq!(biomes.len(), 2);
    assert_eq!(biomes[0].name, "minecraft:swamp");
    assert!((biomes[0].temperature - 0.8).abs() < 1e-6);
    assert!((biomes[0].downfall - 0.9).abs() < 1e-6);
    assert_eq!(biomes[0].water_color, 0x617B64);

    assert_eq!(biomes[1].name, "minecraft:plains");
    assert!((biomes[1].temperature - 0.8).abs() < 1e-6, "Standardwert erwartet");
    assert_eq!(biomes[1].water_color, 0x3F76E4, "Standard-Wasserblau erwartet");
}

#[test]
fn andere_registries_werden_uebersprungen() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_CONFIG_REGISTRY_DATA);
    w.write_string("minecraft:damage_type");
    w.write_varint(0);
    let payload = w.into_bytes();
    assert!(read_registry_data(&payload).unwrap().is_none());
}

// Gegenprobe zur Bit-Belegung aus PlayerInput.PACKET_CODEC::encode (verifiziert am
// 1.21.11-Client): FORWARD=1, BACKWARD=2, LEFT=4, RIGHT=8,
// JUMP=16, SNEAK=32, SPRINT=64 in genau einem Byte nach der VarInt-Packet-ID.
#[test]
fn player_input_payload_setzt_die_richtigen_bits() {
    let payload = player_input_payload(true, false, false, true, false, false, true);
    let mut r = PacketReader::new(&payload);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_PLAYER_INPUT);
    let flags = r.read_u8().unwrap();
    assert_eq!(flags, 1 | 8 | 64); // forward | right | sprint

    let none = player_input_payload(false, false, false, false, false, false, false);
    let mut r2 = PacketReader::new(&none);
    let _ = r2.read_varint().unwrap();
    assert_eq!(r2.read_u8().unwrap(), 0);
}

// Gegenprobe zum Wire-Format aus ClientCommandC2SPacket::write des echten Clients
// (verifiziert am Server-Handler): EntityId, dann Mode
// (START_SPRINTING=1/STOP_SPRINTING=2), dann mountJumpHeight (hier immer 0).
#[test]
fn player_command_sprint_payload_setzt_den_richtigen_modus() {
    let start = player_command_sprint_payload(11724, true);
    let mut r = PacketReader::new(&start);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_PLAYER_COMMAND);
    assert_eq!(r.read_varint().unwrap(), 11724);
    assert_eq!(r.read_varint().unwrap(), PLAYER_COMMAND_MODE_START_SPRINTING);
    assert_eq!(r.read_varint().unwrap(), 0);

    let stop = player_command_sprint_payload(11724, false);
    let mut r2 = PacketReader::new(&stop);
    let _ = r2.read_varint().unwrap();
    let _ = r2.read_varint().unwrap();
    assert_eq!(r2.read_varint().unwrap(), PLAYER_COMMAND_MODE_STOP_SPRINTING);
}

// Gegenprobe zum Login-(play)-Byte-Layout (empirisch per CORAL_PACKET_SCAN
// verifiziert): playerId liegt als 4-Byte-Int direkt nach der Packet-ID.
#[test]
fn read_login_player_id_liest_den_richtigen_wert() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_LOGIN).write_i32(11724);
    // Rest des Pakets (hardcore, levels, ...) ist fuer diesen Test irrelevant, w wird
    // trotzdem mit ein paar Fuellbytes verlaengert, um ein realistisches Payload zu simulieren.
    w.write_u8(0).write_varint(0);
    let payload = w.into_bytes();
    assert_eq!(read_login_player_id(&payload).unwrap(), 11724);
}

#[test]
fn chat_message_payload_hat_die_richtige_paket_id_und_laesst_sich_lesen() {
    let payload = chat_message_payload("hallo welt");
    let mut r = PacketReader::new(&payload);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CHAT);
    assert_eq!(r.read_string().unwrap(), "hallo welt");
    let _timestamp = r.read_i64().unwrap();
    assert_eq!(r.read_i64().unwrap(), 0); // salt
    assert!(!r.read_bool().unwrap()); // keine Signatur
    assert_eq!(r.read_varint().unwrap(), 0); // Acknowledgment-Offset
}

#[test]
fn chat_command_payload_nutzt_eigene_id_ohne_schraegstrich() {
    // Befehl OHNE fuehrenden '/', eigene Paket-ID 0x06 (nicht die Chat-ID 0x08) -- sonst ChatB-Flag.
    let payload = chat_command_payload("spawn");
    let mut r = PacketReader::new(&payload);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CHAT_COMMAND);
    assert_ne!(SB_PLAY_CHAT_COMMAND, SB_PLAY_CHAT);
    assert_eq!(r.read_string().unwrap(), "spawn");
    assert!(r.is_exhausted(), "unsignierter Chat-Command hat NUR den String, keine weiteren Felder");
}

#[test]
fn read_system_chat_liest_text_und_overlay() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SYSTEM_CHAT);
    // Netzwerk-NBT-Wurzel: TAG_Compound { "text": "Hallo" }
    let mut nbt_bytes: Vec<u8> = Vec::new();
    nbt_bytes.push(10); // TAG_Compound (Wurzel)
    nbt_bytes.push(8); // TAG_String "text"
    nbt_bytes.extend_from_slice(&4u16.to_be_bytes());
    nbt_bytes.extend_from_slice(b"text");
    nbt_bytes.extend_from_slice(&5u16.to_be_bytes());
    nbt_bytes.extend_from_slice(b"Hallo");
    nbt_bytes.push(0); // TAG_End
    for b in nbt_bytes {
        w.write_u8(b);
    }
    w.write_bool(true); // overlay
    let payload = w.into_bytes();

    let (text, overlay) = read_system_chat(&payload).unwrap();
    assert_eq!(text, "Hallo");
    assert!(overlay);
}

// Gegenprobe zu den drei "Teil"-Move-Player-Varianten (s. App::send_position_update):
// jede traegt nur ihre eigenen Felder plus die gemeinsamen Flags, in exakt der
// Reihenfolge aus PlayerMoveC2SPacket.{PositionAndOnGround,LookAndOnGround,OnGroundOnly}::write.
#[test]
fn move_player_teilpakete_haben_die_richtige_id_und_feldreihenfolge() {
    let pos = move_player_pos_payload(1.5, 2.5, 3.5, true, false);
    let mut r = PacketReader::new(&pos);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_MOVE_PLAYER_POS);
    assert_eq!(r.read_f64().unwrap(), 1.5);
    assert_eq!(r.read_f64().unwrap(), 2.5);
    assert_eq!(r.read_f64().unwrap(), 3.5);
    assert_eq!(r.read_u8().unwrap(), MOVE_FLAG_ON_GROUND);

    let rot = move_player_rot_payload(90.0, -10.0, false, true);
    let mut r = PacketReader::new(&rot);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_MOVE_PLAYER_ROT);
    assert_eq!(r.read_f32().unwrap(), 90.0);
    assert_eq!(r.read_f32().unwrap(), -10.0);
    assert_eq!(r.read_u8().unwrap(), MOVE_FLAG_HORIZONTAL_COLLISION);

    let status = move_player_status_only_payload(true, true);
    let mut r = PacketReader::new(&status);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_MOVE_PLAYER_STATUS_ONLY);
    assert_eq!(r.read_u8().unwrap(), MOVE_FLAG_ON_GROUND | MOVE_FLAG_HORIZONTAL_COLLISION);
}
