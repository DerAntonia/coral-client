use super::*;

// Signierte Nachricht: der Zeitstempel steht als Millis im Paket (die Signatur selbst
// deckt nur die Sekunden ab, s. net::chat_signing) und die Signatur sind 256 rohe
// Bytes ohne Laengenpraefix.
#[test]
fn signierte_nachricht_hat_256_byte_signatur() {
    let signature = [7u8; 256];
    let bytes = signed_chat_message_payload("hallo", 1_700_000_123_456, 99, Some(&signature));
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CHAT);
    assert_eq!(r.read_string().unwrap(), "hallo");
    assert_eq!(r.read_i64().unwrap(), 1_700_000_123_456);
    assert_eq!(r.read_i64().unwrap(), 99);
    assert!(r.read_bool().unwrap());
    assert_eq!(r.read_bytes(256).unwrap(), &signature[..]);
    assert_eq!(r.read_varint().unwrap(), 0); // Quittungs-Offset
    assert_eq!(r.read_bytes(4).unwrap(), &[0, 0, 0, 0]); // BitSet(20) + Checksum
    assert!(r.is_exhausted());
}

// Set Equipment hat KEINE Anzahl vorweg: das oberste Bit des Slot-Bytes traegt das
// "es folgt noch einer". Hier zwei Eintraege (Haupthand belegt, Nebenhand leer), damit
// beide Faelle einmal durchlaufen -- ein leerer Slot ist count<=0 und liefert None.
#[test]
fn set_equipment_liest_kette_bis_zum_letzten_eintrag() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_EQUIPMENT).write_varint(42);
    // Haupthand mit Fortsetzungsbit: Stapel aus einem Item 936 ohne Components.
    w.write_u8(EQUIPMENT_SLOT_MAINHAND | 0x80).write_varint(1).write_varint(936).write_varint(0).write_varint(0);
    // Nebenhand ohne Fortsetzungsbit, leer.
    w.write_u8(EQUIPMENT_SLOT_OFFHAND).write_varint(0);

    let (id, entries) = read_set_equipment(&w.into_bytes()).unwrap();
    assert_eq!(id, 42);
    assert_eq!(entries.len(), 2, "die Kette endet erst beim Byte ohne 0x80");
    assert_eq!(entries[0].slot, EQUIPMENT_SLOT_MAINHAND);
    assert_eq!(entries[0].item.as_ref().unwrap().item_id, 936);
    assert_eq!(entries[1].slot, EQUIPMENT_SLOT_OFFHAND);
    assert_eq!(entries[1].item, None, "leerer Slot = Item aus der Hand genommen");
}

// Ein nie endendes Fortsetzungsbit darf nicht in eine Endlosschleife laufen.
#[test]
fn set_equipment_deckelt_endlose_kette() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_EQUIPMENT).write_varint(1);
    for _ in 0..MAX_EQUIPMENT_ENTRIES + 2 {
        w.write_u8(0x80).write_varint(0);
    }
    assert!(read_set_equipment(&w.into_bytes()).is_err());
}

// Chunk-Stapel: die Groesse kommt als VarInt hinter der Paket-ID, die Bestaetigung
// besteht aus ID + einem einzigen f32 (kein VarInt -- ein Fehler hier laesst den Server
// die Chunk-Lieferung einstellen, s. CB_PLAY_CHUNK_BATCH_START).
#[test]
fn chunk_stapel_groesse_und_bestaetigung() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_CHUNK_BATCH_FINISHED).write_varint(9);
    assert_eq!(read_chunk_batch_size(&w.into_bytes()).unwrap(), 9);

    let bytes = chunk_batch_received_payload(3.5);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CHUNK_BATCH_RECEIVED);
    assert_eq!(r.read_f32().unwrap(), 3.5);
    assert!(r.is_exhausted());
}

// Die 1.21.11-Geschwindigkeitskodierung ist NICHT mehr "drei Shorts mal 8000" -- ein nach
// altem Format gelesener Rueckstoss waere blanker Unsinn. Die Testvektoren sind mit
// Vanillas eigenem writeVelocity (VelocityEncoding) gerechnet, nicht von Hand gesetzt:
// damit prueft der Test die Dekodierung gegen den echten Encoder, nicht gegen sich selbst.
#[test]
fn geschwindigkeit_wird_nach_der_1_21_11_kodierung_gelesen() {
    let faelle: [([u8; 6], (f64, f64, f64)); 3] = [
        ([193, 204, 102, 101, 102, 98], (0.4, 0.4, -0.2)),
        ([249, 255, 127, 255, 107, 129], (0.0, 0.42, 0.0)),
        ([2, 128, 185, 151, 38, 96], (-1.5, 0.3, 0.9)),
    ];
    for (bytes, erwartet) in faelle {
        let mut r = PacketReader::new(&bytes);
        let (x, y, z) = read_velocity(&mut r).unwrap();
        // Die Kodierung quantisiert auf 15 Bit je Achse, mal dem ganzzahlig
        // aufgerundeten Betrag -- die Abweichung waechst also mit dem Betrag.
        let toleranz = 1.0e-4 * erwartet.0.abs().max(erwartet.1.abs()).max(erwartet.2.abs()).max(1.0);
        assert!((x - erwartet.0).abs() < toleranz, "x: {x} statt {}", erwartet.0);
        assert!((y - erwartet.1).abs() < toleranz, "y: {y} statt {}", erwartet.1);
        assert!((z - erwartet.2).abs() < toleranz, "z: {z} statt {}", erwartet.2);
    }
}

// Erstes Byte 0 heisst "keine Bewegung" -- und beendet das Paket, es folgt nichts mehr.
#[test]
fn geschwindigkeit_null_ist_ein_einzelnes_byte() {
    let mut r = PacketReader::new(&[0]);
    assert_eq!(read_velocity(&mut r).unwrap(), (0.0, 0.0, 0.0));
    assert!(r.is_exhausted());
}

// RELEASE_USE_ITEM beendet fuer GrimAC die Item-Benutzung -- ohne das haelt es uns
// dauerhaft fuer "benutzt gerade etwas" und rechnet mit dem Item-Faktor 0.2 statt unserem
// Sneak-Faktor 0.3. Der Enum-Index und die NULL-Sequenz sind hier das Wesentliche: eine
// echte Sequenznummer flaggt GrimACs BadPacketsL (verlangt pos=0/0/0, face=0, sequence=0)
// und, weil der Zaehler dann uebersprang, auch BadPacketsH.
#[test]
fn release_use_item_hat_index_fuenf_und_keine_sequenz() {
    assert_eq!(PLAYER_ACTION_DROP_ITEM, 4, "verifizierter Nachbar");
    assert_eq!(PLAYER_ACTION_RELEASE_USE_ITEM, PLAYER_ACTION_DROP_ITEM + 1);
    assert_eq!(PLAYER_ACTION_SWAP_OFFHAND, PLAYER_ACTION_RELEASE_USE_ITEM + 1);

    let bytes = player_action_payload(PLAYER_ACTION_RELEASE_USE_ITEM);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_PLAYER_ACTION);
    assert_eq!(r.read_varint().unwrap(), PLAYER_ACTION_RELEASE_USE_ITEM);
    assert_eq!(r.read_i64().unwrap(), 0, "BlockPos.ORIGIN");
    assert_eq!(r.read_u8().unwrap(), 0, "Richtung DOWN");
    assert_eq!(r.read_varint().unwrap(), 0, "nicht sequenziert");
    assert!(r.is_exhausted());
}

// Respawn: die ID selbst ist die ganze Information, die wir daraus brauchen -- an den
// verifizierten Nachbarn festgenagelt, damit eine Verschiebung nicht unbemerkt dazu
// fuehrt, dass wir den Tastenzustand nach dem Respawn NICHT neu melden.
#[test]
fn respawn_liegt_zwischen_seinen_verifizierten_nachbarn() {
    assert_eq!(CB_PLAY_PLAYER_POSITION, 0x46, "verifizierter Anker");
    assert_eq!(CB_PLAY_RESPAWN, 0x50);
    assert_eq!(CB_PLAY_SET_EQUIPMENT, CB_PLAY_RESPAWN + 20, "Set Equipment (0x64) ist verifiziert");
}

// Die Dimension aus Login und Respawn: beide enden bzw. beginnen mit demselben
// CommonPlayerSpawnInfo, dessen erste zwei Felder VarInt dimensionType + Identifier sind.
// Nachgebaut nach GameJoinS2CPacket/PlayerRespawnS2CPacket (1.21.11).
#[test]
fn dimension_aus_login_und_respawn() {
    let mut login = PacketWriter::new();
    login.write_varint(CB_PLAY_LOGIN).write_i32(42).write_bool(false);
    login.write_varint(3);
    for level in ["minecraft:overworld", "minecraft:the_nether", "minecraft:the_end"] {
        login.write_string(level);
    }
    login.write_varint(20).write_varint(12).write_varint(12);
    login.write_bool(false).write_bool(true).write_bool(false);
    // CommonPlayerSpawnInfo: dimensionType-Id, dann der Welt-RegistryKey.
    login.write_varint(0).write_string("minecraft:overworld").write_i64(1234);
    let login = login.into_bytes();
    assert_eq!(read_login_player_id(&login).unwrap(), 42);
    assert_eq!(read_login_dimension(&login).unwrap(), "minecraft:overworld");

    let mut respawn = PacketWriter::new();
    respawn.write_varint(CB_PLAY_RESPAWN);
    respawn.write_varint(1).write_string("minecraft:the_nether").write_i64(1234);
    assert_eq!(read_respawn_dimension(&respawn.into_bytes()).unwrap(), "minecraft:the_nether");
}

// Rechtsklick-Pakete: IDs an den verifizierten Nachbarn festgenagelt, Wire-Format 1:1
// gegen PacketByteBuf.writeBlockHitResult geprueft.
#[test]
fn benutzen_pakete_liegen_bei_ihren_verifizierten_nachbarn() {
    assert_eq!(SB_PLAY_PLAYER_ACTION, 0x28, "verifizierter Anker");
    assert_eq!(SB_PLAY_SWING, 0x3C, "verifizierter Anker");
    assert_eq!(SB_PLAY_USE_ITEM_ON, SB_PLAY_SWING + 3);
    assert_eq!(SB_PLAY_USE_ITEM, SB_PLAY_USE_ITEM_ON + 1);
}

#[test]
fn set_entity_data_liegt_bei_seinen_verifizierten_nachbarn() {
    assert_eq!(CB_PLAY_SET_EQUIPMENT, 0x64, "verifizierter Anker");
    assert_eq!(CB_PLAY_SET_HEALTH, 0x66, "verifizierter Anker");
    // Reihenfolge der Clientbound-Tabelle: SET_ENTITY_DATA, SET_ENTITY_LINK,
    // SET_ENTITY_MOTION, SET_EQUIPMENT.
    assert_eq!(CB_PLAY_SET_ENTITY_DATA, CB_PLAY_SET_EQUIPMENT - 3);
}

// Der Schlafzustand steckt in LivingEntity.SLEEPING_POSITION (Index 14, Serializer
// OPTIONAL_BLOCK_POS = 11) -- daran und nur daran erkennt der Client, dass er im Bett
// liegt. Der Test schiebt bewusst ein Feld davor, das wir ueberspringen muessen.
#[test]
// Sonst fehlen die Absorptionsherzen: der Goldapfel bringt im
// selben Paket die Trank-Wirbel (Index 10, Partikelliste) VOR der Absorption (17). Die
// Liste war unlesbar, der Strom brach ab, die Absorption ging verloren.
fn absorption_hinter_partikelliste_und_arm_kommt_an() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(7);
    // Index 10: zwei entity_effect-Wirbel (TintedParticleEffect, ARGB).
    w.write_u8(10).write_varint(DATA_TYPE_PARTICLE_LIST).write_varint(2);
    w.write_varint(crate::particle::PARTICLE_ENTITY_EFFECT).write_i32(0xFF2E_5299u32 as i32);
    w.write_varint(crate::particle::PARTICLE_ENTITY_EFFECT).write_i32(0xFFFF_D700u32 as i32);
    // Index 15: Haupthand (ARM), rechts = 1.
    w.write_u8(15).write_varint(DATA_TYPE_ARM).write_varint(1);
    w.write_u8(PLAYER_INDEX_ABSORPTION).write_varint(DATA_TYPE_FLOAT).write_f32(4.0);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.absorption, Some(4.0));
    // Die Wirbel selbst kommen jetzt auch an (Trank-Partikel an Entities).
    assert_eq!(update.potion_swirls, Some(vec![0xFF2E_5299, 0xFFFF_D700]));
    assert_eq!(update.main_arm_left, Some(false));
    // Linkshaender: 0.
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(7);
    w.write_u8(15).write_varint(DATA_TYPE_ARM).write_varint(0);
    w.write_u8(0xFF);
    assert_eq!(read_set_entity_data(&w.into_bytes()).unwrap().main_arm_left, Some(true));
}

#[test]
// Zaubertisch & Co.: Container-ID als VarInt, dann zwei Shorts (auch negativ: -1 heisst
// beim Zaubertisch "kein Hinweis").
fn container_daten_und_knoepfe() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_CONTAINER_SET_DATA).write_varint(3).write_u16(4).write_u16(0xFFFF);
    assert_eq!(read_container_set_data(&w.into_bytes()).unwrap(), (3, 4, -1));
    assert_eq!(container_button_click_payload(3, 2), vec![SB_PLAY_CONTAINER_BUTTON_CLICK as u8, 3, 2]);
    assert_eq!(rename_item_payload("ab"), vec![SB_PLAY_RENAME_ITEM as u8, 2, b'a', b'b']);
    let seiten = vec!["a".to_string(), String::new()];
    assert_eq!(edit_book_payload(40, &seiten, None), vec![SB_PLAY_EDIT_BOOK as u8, 40, 2, 1, b'a', 0, 0]);
    assert_eq!(edit_book_payload(3, &[], Some("T")), vec![SB_PLAY_EDIT_BOOK as u8, 3, 0, 1, 1, b'T']);
}

#[test]
// Zwei Angebote: eines mit zweitem Preis (samt einer geforderten Component, max_damage = 2)
// und eines ausverkauft. Danach Stufe, XP und die beiden Flags -- verrutscht ein Feld,
// stimmen diese am Ende nicht mehr.
fn haendler_angebote_lesen() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_MERCHANT_OFFERS).write_varint(7).write_varint(2);
    // Angebot 1: 20 Item(5) + 1 Item(9, max_damage 30) -> 3x Item(11)
    w.write_varint(5).write_varint(20).write_varint(0);
    w.write_varint(3).write_varint(11).write_varint(0).write_varint(0);
    w.write_bool(true).write_varint(9).write_varint(1).write_varint(1).write_varint(2).write_varint(30);
    w.write_bool(false).write_i32(2).write_i32(12).write_i32(5).write_i32(-3).write_f32(0.05).write_i32(4);
    // Angebot 2: ausverkauft
    w.write_varint(5).write_varint(1).write_varint(0);
    w.write_varint(1).write_varint(12).write_varint(0).write_varint(0);
    w.write_bool(false);
    w.write_bool(true).write_i32(0).write_i32(16).write_i32(1).write_i32(0).write_f32(0.2).write_i32(0);
    w.write_varint(3).write_varint(85).write_bool(true).write_bool(true);
    let m = read_merchant_offers(&w.into_bytes()).unwrap();
    assert_eq!((m.sync_id, m.offers.len(), m.level, m.xp, m.show_progress, m.can_restock), (7, 2, 3, 85, true, true));
    let a = &m.offers[0];
    assert_eq!((a.cost_a.item_id, a.cost_a.count), (5, 20));
    let b = a.cost_b.as_ref().unwrap();
    assert_eq!((b.item_id, b.max_damage), (9, Some(30)));
    assert_eq!(a.result.as_ref().map(|s| (s.item_id, s.count)), Some((11, 3)));
    assert_eq!((a.uses, a.max_uses, a.xp, a.special_price, a.demand), (2, 12, 5, -3, 4));
    assert!(!a.disabled());
    // 20 + floor(20 * 4 * 0.05) - 3 = 21
    assert_eq!(a.displayed_cost_a(64), 21);
    assert!(m.offers[1].disabled() && m.offers[1].cost_b.is_none());
    assert_eq!(select_trade_payload(3), vec![SB_PLAY_SELECT_TRADE as u8, 3]);
}

#[test]
// Eine sitzende Katze: Baby-Flag (16), Zaehm-Flags (17), Besitzer (18, optionale UUID)
// und die Variante (19). Besitzer und Variante haben Serializer, an denen der Strom
// frueher abbrach.
fn katze_liefert_sitzflag_und_variante() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(9);
    w.write_u8(16).write_varint(DATA_TYPE_BOOLEAN).write_bool(false);
    w.write_u8(TAMABLE_INDEX_FLAGS).write_varint(DATA_TYPE_BYTE).write_u8(TAMABLE_FLAG_SITTING | 4);
    w.write_u8(18).write_varint(DATA_TYPE_OPTIONAL_LIVING_ENTITY_REFERENCE).write_bool(true).write_i64(1).write_i64(2);
    w.write_u8(19).write_varint(DATA_TYPE_CAT_VARIANT).write_varint(3);
    w.write_u8(20).write_varint(DATA_TYPE_BOOLEAN).write_bool(false);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.tamable_flags, Some(TAMABLE_FLAG_SITTING | 4));
    assert_eq!(update.cat_variant, Some(3));
}

#[test]
fn set_entity_data_liest_den_rahmen_hinter_der_haengerichtung() {
    // Wie ein Gegenstandsrahmen mit Karte: Wand (8), Inhalt (9), Drehung (10).
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(5);
    w.write_u8(HANGING_INDEX_DIRECTION).write_varint(DATA_TYPE_FACING).write_varint(4);
    // Stapel: Anzahl 1, Item, eine Component (map_id = 44) mit Wert 7, keine entfernt.
    w.write_u8(ITEM_FRAME_INDEX_ITEM).write_varint(DATA_TYPE_ITEM_STACK);
    w.write_varint(1).write_varint(900).write_varint(1).write_varint(0).write_varint(44).write_varint(7);
    w.write_u8(ITEM_FRAME_INDEX_ROTATION).write_varint(DATA_TYPE_INT).write_varint(3);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.hanging_direction, Some(4));
    assert_eq!(update.frame_item.flatten().and_then(|s| s.map_id), Some(7));
    assert_eq!(update.frame_rotation, Some(3));
    assert!(update.item_stack.is_none());
}

#[test]
fn set_entity_data_liest_die_schlafposition() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(42);
    // Index 9, Serializer FLOAT (LivingEntity.HEALTH) -- nur zum Ueberspringen.
    w.write_u8(9).write_varint(DATA_TYPE_FLOAT).write_f32(20.0);
    // Index 14, Serializer OPTIONAL_BLOCK_POS, vorhanden.
    w.write_u8(ENTITY_DATA_INDEX_SLEEPING_POS).write_varint(DATA_TYPE_OPTIONAL_BLOCK_POS);
    w.write_bool(true).write_i64(encode_block_pos(-12, 63, 300));
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.entity_id, 42);
    assert_eq!(update.sleeping_pos, Some(Some((-12, 63, 300))));

    // Leeres Optional = aufgewacht, und das muss sich von "stand nicht im Paket"
    // unterscheiden lassen.
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(42);
    w.write_u8(ENTITY_DATA_INDEX_SLEEPING_POS).write_varint(DATA_TYPE_OPTIONAL_BLOCK_POS).write_bool(false);
    w.write_u8(0xFF);
    assert_eq!(read_set_entity_data(&w.into_bytes()).unwrap().sleeping_pos, Some(None));

    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(42);
    w.write_u8(0xFF);
    assert_eq!(read_set_entity_data(&w.into_bytes()).unwrap().sleeping_pos, None, "kein Feld = keine Aenderung");
}

// Auf der Leitung steht Fortschritt, Stufe, Gesamterfahrung (Lesecode des 1.21.11-Jars).
// Vertauscht faellt es erst bei Stufe > 0 auf.
#[test]
fn experience_liest_die_wire_reihenfolge() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_EXPERIENCE);
    w.write_f32(0.75);
    w.write_varint(30); // Stufe
    w.write_varint(1395); // Gesamterfahrung
    let (progress, level, total) = read_experience(&w.into_bytes()).unwrap();
    assert!((progress - 0.75).abs() < 1e-6);
    assert_eq!(level, 30, "Stufe steht VOR den Gesamtpunkten");
    assert_eq!(total, 1395);
}

// Die drei Bewegungs-Attribute, die GrimAC fuer seine Vorhersage liest. Das Tempo muss
// dabei OHNE die Modifikatoren ankommen, die die Physik selbst anlegt (Sprint,
// Schnelligkeit, Langsamkeit) -- sonst zaehlen sie doppelt.
#[test]
fn bewegungsattribute_ohne_die_selbst_angelegten_modifikatoren() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_UPDATE_ATTRIBUTES);
    w.write_varint(7);
    w.write_varint(3);
    // Tempo: Grundwert 0.1, dazu ein Plugin-Zuschlag (+20 %) UND die beiden, die wir
    // selbst rechnen.
    w.write_varint(ATTRIBUTE_ID_MOVEMENT_SPEED).write_f64(0.1).write_varint(3);
    w.write_string("minecraft:sprinting").write_f64(0.3).write_varint(ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL);
    w.write_string("minecraft:effect.speed").write_f64(0.2).write_varint(ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL);
    w.write_string("server:kit").write_f64(0.2).write_varint(ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL);
    // Schleichender Schritt III: 0.3 + 3 x 0.15.
    w.write_varint(ATTRIBUTE_ID_SNEAKING_SPEED).write_f64(0.3).write_varint(1);
    w.write_string("minecraft:enchantment.swift_sneak").write_f64(0.45).write_varint(ATTRIBUTE_OP_ADD_VALUE);
    // Wasserlaeufer III.
    w.write_varint(ATTRIBUTE_ID_WATER_MOVEMENT_EFFICIENCY).write_f64(0.0).write_varint(1);
    w.write_string("minecraft:enchantment.depth_strider").write_f64(1.0).write_varint(ATTRIBUTE_OP_ADD_VALUE);
    let update = read_attributes(&w.into_bytes()).unwrap();
    let tempo = update.movement_speed.expect("Tempo gelesen");
    assert!((tempo - 0.12).abs() < 1e-9, "nur der Plugin-Zuschlag zaehlt, war {tempo}");
    assert!((update.sneaking_speed.unwrap() - 0.75).abs() < 1e-9);
    assert!((update.water_movement_efficiency.unwrap() - 1.0).abs() < 1e-9);
}

// anticheat-test.com schickt den Sprint-Modifikator ohne Namensraum ("sprinting"). Vanilla
// liest das als minecraft:sprinting -- er darf also ebensowenig doppelt zaehlen.
#[test]
fn sprint_modifikator_ohne_namensraum_zaehlt_nicht_doppelt() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_UPDATE_ATTRIBUTES);
    w.write_varint(7);
    w.write_varint(1);
    w.write_varint(ATTRIBUTE_ID_MOVEMENT_SPEED).write_f64(0.1).write_varint(1);
    w.write_string("sprinting").write_f64(0.3).write_varint(ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL);
    let tempo = read_attributes(&w.into_bytes()).unwrap().movement_speed.expect("Tempo gelesen");
    assert!((tempo - 0.1).abs() < 1e-9, "Sprint gehoert der Physik, war {tempo}");
}

// anticheat-test.com (1.8-Kampf): Grundwert 20, dazu der Item-Modifikator des Schwerts.
// Der Item-Teil bleibt draussen, der Rest kommt zerlegt an.
#[test]
fn angriffstempo_ohne_item_modifikator() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_UPDATE_ATTRIBUTES);
    w.write_varint(7);
    w.write_varint(1);
    w.write_varint(ATTRIBUTE_ID_ATTACK_SPEED).write_f64(20.0).write_varint(2);
    w.write_string("base_attack_speed").write_f64(-2.4).write_varint(ATTRIBUTE_OP_ADD_VALUE);
    w.write_string("minecraft:effect.haste").write_f64(0.1).write_varint(ATTRIBUTE_OP_ADD_MULTIPLIED_BASE);
    let (a, b, c) = read_attributes(&w.into_bytes()).unwrap().attack_speed.expect("Angriffstempo gelesen");
    assert_eq!((a, b, c), (20.0, 0.1, 1.0));
}

// Die Koerper-Attribute kommen an, geklemmt auf Vanillas Grenzen (scale 0.0625..16).
#[test]
fn koerper_attribute_werden_gelesen_und_geklemmt() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_UPDATE_ATTRIBUTES);
    w.write_varint(7);
    w.write_varint(5);
    w.write_varint(ATTRIBUTE_ID_GRAVITY).write_f64(0.08).write_varint(1);
    w.write_string("test:leicht").write_f64(-0.5).write_varint(ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL);
    w.write_varint(ATTRIBUTE_ID_JUMP_STRENGTH).write_f64(0.42).write_varint(0);
    w.write_varint(ATTRIBUTE_ID_MOVEMENT_EFFICIENCY).write_f64(0.0).write_varint(1);
    w.write_string("minecraft:enchantment.soul_speed").write_f64(1.0).write_varint(ATTRIBUTE_OP_ADD_VALUE);
    w.write_varint(ATTRIBUTE_ID_SCALE).write_f64(1.0).write_varint(1);
    w.write_string("test:riese").write_f64(40.0).write_varint(ATTRIBUTE_OP_ADD_VALUE);
    w.write_varint(ATTRIBUTE_ID_STEP_HEIGHT).write_f64(0.6).write_varint(0);
    let u = read_attributes(&w.into_bytes()).unwrap();
    assert_eq!(u.gravity, Some(0.04));
    assert_eq!(u.jump_strength, Some(0.42));
    assert_eq!(u.movement_efficiency, Some(1.0));
    assert_eq!(u.scale, Some(16.0));
    assert_eq!(u.step_height, Some(0.6));
}

// EntityAttributeInstance.computeValue(): ADD_VALUE auf den Grundwert, dann
// ADD_MULTIPLIED_BASE auf diese Zwischensumme, zuletzt ADD_MULTIPLIED_TOTAL multiplikativ.
#[test]
fn attribute_rechnen_die_drei_operationen_in_der_richtigen_reihenfolge() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_UPDATE_ATTRIBUTES);
    w.write_varint(42); // Entity-ID
    w.write_varint(2); // zwei Attribute
                       // Ruestung: Grundwert 0, +6 (Brustpanzer) und +3 (Helm) als ADD_VALUE, dann +10 %.
    w.write_varint(ATTRIBUTE_ID_ARMOR).write_f64(0.0).write_varint(3);
    w.write_string("minecraft:armor.body").write_f64(6.0).write_varint(ATTRIBUTE_OP_ADD_VALUE);
    w.write_string("minecraft:armor.head").write_f64(3.0).write_varint(ATTRIBUTE_OP_ADD_VALUE);
    w.write_string("test:prozent").write_f64(0.1).write_varint(ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL);
    // Maximalgesundheit ohne Modifikatoren.
    w.write_varint(ATTRIBUTE_ID_MAX_HEALTH).write_f64(24.0).write_varint(0);
    let update = read_attributes(&w.into_bytes()).unwrap();
    assert_eq!(update.entity_id, 42);
    assert!((update.armor.unwrap() - 9.9).abs() < 1e-9, "armor={:?}", update.armor);
    assert!((update.max_health.unwrap() - 24.0).abs() < 1e-9);
}

// Restluft steckt als Feld 1 (Int) im Metadaten-Strom -- dasselbe Paket, aus dem auch
// Pose und Schlafposition kommen.
#[test]
fn set_entity_data_liest_die_restluft() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(7);
    w.write_u8(ENTITY_DATA_INDEX_AIR).write_varint(DATA_TYPE_INT).write_varint(140);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.air, Some(140));

    // Ein anderes Int-Feld darf die Luft NICHT setzen (sonst zeigte das HUD z.B. die
    // Frost-Ticks als Restluft an).
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(7);
    w.write_u8(7).write_varint(DATA_TYPE_INT).write_varint(99); // FROZEN_TICKS
    w.write_u8(0xFF);
    assert_eq!(read_set_entity_data(&w.into_bytes()).unwrap().air, None);
}

// Wurzel-Tag im Netzwerk-NBT: Typ-Byte TAG_String (8), Laenge u16, Bytes.
fn nbt_string(w: &mut PacketWriter, text: &str) {
    w.write_u8(8).write_u16(text.len() as u16);
    for b in text.as_bytes() {
        w.write_u8(*b);
    }
}

// Gelenkwinkel eines Armorstands: Rotations an 16..21 landen beim passenden Gelenk, ein
// Rotations-Feld an anderem Index nirgends.
#[test]
fn set_entity_data_liest_armorstand_posen() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(7);
    for (index, v) in [(16u8, [10.0f32, 20.0, 30.0]), (19, [-90.0, 0.0, 5.0]), (22, [1.0, 1.0, 1.0])] {
        w.write_u8(index).write_varint(DATA_TYPE_ROTATION);
        for f in v {
            w.write_f32(f);
        }
    }
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.armor_stand_poses, [Some([10.0, 20.0, 30.0]), None, None, Some([-90.0, 0.0, 5.0]), None, None]);
}

// Das klassische Hologramm: unsichtbarer Armorstand mit dauerhaft eingeblendetem Namen.
// Alle drei Felder stehen im SELBEN Strom und muessen zusammen ankommen.
#[test]
fn set_entity_data_liest_unsichtbarkeit_und_namensschild() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(4711);
    w.write_u8(ENTITY_DATA_INDEX_FLAGS).write_varint(DATA_TYPE_BYTE).write_u8(ENTITY_FLAG_INVISIBLE);
    w.write_u8(ENTITY_DATA_INDEX_CUSTOM_NAME).write_varint(DATA_TYPE_OPTIONAL_TEXT).write_bool(true);
    nbt_string(&mut w, "\u{a7}aShop");
    w.write_u8(ENTITY_DATA_INDEX_NAME_VISIBLE).write_varint(DATA_TYPE_BOOLEAN).write_bool(true);
    w.write_u8(ARMOR_STAND_INDEX_FLAGS).write_varint(DATA_TYPE_BYTE).write_u8(ARMOR_STAND_FLAG_MARKER);
    w.write_u8(0xFF);

    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.flags, Some(ENTITY_FLAG_INVISIBLE));
    assert_eq!(update.name_visible, Some(true));
    assert_eq!(update.armor_stand_flags, Some(ARMOR_STAND_FLAG_MARKER));
    let spans = update.custom_name.expect("Name stand im Paket").expect("Name nicht geleert");
    // Der Paragraphen-Code wird zur Farbe, nicht mitgezeichnet.
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "Shop");
    assert_eq!(spans[0].color, Some(0x55FF55));
}

// Ein text_display: der Text steht an Index 23, also HINTER den Vector3f-/Quaternion-
// Feldern der Transformation. Genau daran scheiterte der Parser vorher -- er kannte
// deren Laenge nicht und brach vor dem Text ab.
#[test]
fn set_entity_data_liest_text_display_hinter_der_transformation() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(9);
    w.write_u8(DISPLAY_INDEX_TRANSLATION).write_varint(DATA_TYPE_VECTOR_3F);
    w.write_f32(0.0).write_f32(1.5).write_f32(0.0);
    w.write_u8(DISPLAY_INDEX_LEFT_ROTATION).write_varint(DATA_TYPE_QUATERNION_F);
    w.write_f32(0.1).write_f32(0.2).write_f32(0.3).write_f32(0.9);
    w.write_u8(DISPLAY_INDEX_SCALE).write_varint(DATA_TYPE_VECTOR_3F);
    w.write_f32(2.0).write_f32(2.0).write_f32(2.0);
    w.write_u8(DISPLAY_INDEX_CONTENT).write_varint(DATA_TYPE_TEXT);
    nbt_string(&mut w, "Zeile");
    w.write_u8(TEXT_DISPLAY_INDEX_FLAGS).write_varint(DATA_TYPE_BYTE).write_u8(TEXT_DISPLAY_FLAG_SEE_THROUGH);
    w.write_u8(0xFF);

    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.display_translation, Some([0.0, 1.5, 0.0]));
    assert_eq!(update.display_scale, Some([2.0, 2.0, 2.0]));
    assert_eq!(update.display_left_rotation, Some([0.1, 0.2, 0.3, 0.9]));
    assert_eq!(update.display_right_rotation, None);
    assert_eq!(update.display_text.as_deref().map(|s| s[0].text.clone()), Some("Zeile".to_string()));
    assert_eq!(update.text_flags, Some(TEXT_DISPLAY_FLAG_SEE_THROUGH));
}

// block_display traegt an Index 23 eine State-Id, item_display einen ItemStack -- beide
// an derselben Stelle wie der Text oben, unterschieden allein am Serializer.
// Ein farbiger Titel behaelt seine Farbe (vorher kam er als nackter Text an).
#[test]
fn titel_behaelt_seine_farbe() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_TITLE_TEXT);
    let mut bytes = w.into_bytes();
    // Netzwerk-NBT: Compound ohne Namen { text: "Sieg", color: "gold" }.
    bytes.push(0x0A);
    for (k, v) in [("text", "Sieg"), ("color", "gold")] {
        bytes.push(0x08);
        bytes.extend((k.len() as u16).to_be_bytes());
        bytes.extend(k.as_bytes());
        bytes.extend((v.len() as u16).to_be_bytes());
        bytes.extend(v.as_bytes());
    }
    bytes.push(0x00);
    let spans = read_title_spans(&bytes).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "Sieg");
    assert_eq!(spans[0].color, Some(0xFFAA00));
}

#[test]
fn set_entity_data_unterscheidet_block_und_item_display_am_serializer() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(1);
    w.write_u8(DISPLAY_INDEX_CONTENT).write_varint(DATA_TYPE_BLOCK_STATE).write_varint(1234);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.display_block_state, Some(1234));
    assert_eq!(update.display_text, None);

    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(2);
    // ItemStack: count, item_id, dann die (hier leeren) Component-Listen.
    w.write_u8(DISPLAY_INDEX_CONTENT).write_varint(DATA_TYPE_ITEM_STACK);
    w.write_varint(1).write_varint(936).write_varint(0).write_varint(0);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.display_item.flatten().map(|s| s.item_id), Some(936));

    // Index 8: der Stack eines liegenden Items / Wurfgeschosses, mit Anzahl.
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(3);
    w.write_u8(ENTITY_INDEX_ITEM).write_varint(DATA_TYPE_ITEM_STACK);
    w.write_varint(17).write_varint(936).write_varint(0).write_varint(0);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    let stack = update.item_stack.flatten().unwrap();
    assert_eq!((stack.item_id, stack.count), (936, 17));
    assert_eq!(update.display_item, None);
    assert_eq!(update.display_block_state, None);
}

// Seitenleisten-Text steckt bei vielen Servern im Team-Prefix/-Suffix, nicht im Holder.
// Feldreihenfolge aus TeamS2CPacket.SerializableTeam(RegistryByteBuf) 1.21.11.
#[test]
fn team_liest_prefix_und_suffix_mit_mitgliedern() {
    // Wurzel-Tag im Netzwerk-NBT: Typ-Byte TAG_String (8), Laenge u16, Bytes.
    fn nbt_text(w: &mut PacketWriter, text: &str) {
        w.write_u8(8).write_u16(text.len() as u16);
        for b in text.as_bytes() {
            w.write_u8(*b);
        }
    }
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_PLAYER_TEAM);
    w.write_string("board_3");
    w.write_u8(0); // Mode 0: Team-Info UND Mitglieder
    nbt_text(&mut w, "Zeile 3"); // displayName
    w.write_u8(0x03); // friendlyFlags
    w.write_varint(0); // nameTagVisibility
    w.write_varint(0); // collisionRule
    w.write_varint(15); // color
    nbt_text(&mut w, "Kills: ");
    nbt_text(&mut w, " 12");
    w.write_varint(1).write_string("3"); // Mitgliederliste
    let t = read_team(&w.into_bytes()).unwrap();
    assert_eq!(t.name, "board_3");
    assert_eq!(t.mode, 0);
    // Ungefaerbter Text beginnt mit Reset (= geerbte Teamfarbe), s. nbt::spans_to_legacy.
    assert_eq!(t.prefix.as_deref(), Some("\u{a7}rKills: "));
    assert_eq!(t.suffix.as_deref(), Some("\u{a7}r 12"));
    assert_eq!(t.entries, vec!["3".to_string()]);

    // Mode 4 (Mitglieder entfernen) traegt KEINE Team-Info -- ein Fehlgriff hier wuerde
    // die Namensliste als Prefix-NBT lesen.
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_PLAYER_TEAM);
    w.write_string("board_3").write_u8(4).write_varint(2).write_string("3").write_string("4");
    let t = read_team(&w.into_bytes()).unwrap();
    assert!(t.prefix.is_none() && t.suffix.is_none());
    assert_eq!(t.entries, vec!["3".to_string(), "4".to_string()]);
}

// Wire-Format aus PlayerRotationS2CPacket.CODEC: f32, bool, f32, bool. Die beiden Bools
// sind je Achse EINZELN -- ein gemeinsames Flag-Feld wie beim Teleport waere hier falsch,
// und der Fehler faele nur bei gemischt relativ/absoluten Paketen auf.
#[test]
fn player_rotation_liest_beide_achsen_mit_eigenem_relativ_flag() {
    let mut payload = vec![CB_PLAY_PLAYER_ROTATION as u8];
    payload.extend_from_slice(&90.5f32.to_be_bytes());
    payload.push(1); // yaw relativ
    payload.extend_from_slice(&(-12.25f32).to_be_bytes());
    payload.push(0); // pitch absolut
    let (yaw, rel_yaw, pitch, rel_pitch) = read_player_rotation(&payload).unwrap();
    assert_eq!(yaw, 90.5);
    assert!(rel_yaw, "yaw war als relativ markiert");
    assert_eq!(pitch, -12.25);
    assert!(!rel_pitch, "pitch war als absolut markiert");
}

#[test]
fn player_rotation_meldet_abgeschnittenen_payload() {
    let mut payload = vec![CB_PLAY_PLAYER_ROTATION as u8];
    payload.extend_from_slice(&1.0f32.to_be_bytes());
    // Rest fehlt -- muss ein Err geben statt zu panicken.
    assert!(read_player_rotation(&payload).is_err());
}

#[test]
fn aufstehen_ist_player_command_null() {
    let bytes = player_command_stop_sleeping_payload(7);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_PLAYER_COMMAND);
    assert_eq!(r.read_varint().unwrap(), 7, "eigene EntityId");
    assert_eq!(r.read_varint().unwrap(), 0, "STOP_SLEEPING");
    assert_eq!(r.read_varint().unwrap(), 0, "Zusatzdaten ungenutzt");
    assert!(r.is_exhausted());
}

#[test]
fn kreativ_slot_mit_laengen_vor_den_components() {
    // Ein Trank (potion_contents 49: Trank-ID 5, ohne Farbe/Effekte/Namen) im Hotbar-Slot 36.
    let teile = vec![(49, vec![1, 5, 0, 0, 0])];
    let stack = crate::item::stack_from_parts(1003, 1, &teile, &[]).unwrap();
    assert_eq!(stack.potion.as_ref().and_then(|p| p.potion), Some(5));
    let bytes = set_creative_mode_slot_payload(36, Some(&stack)).unwrap();
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_SET_CREATIVE_MODE_SLOT);
    assert_eq!(r.read_i16().unwrap(), 36);
    assert_eq!([r.read_varint().unwrap(), r.read_varint().unwrap()], [1, 1003]);
    assert_eq!([r.read_varint().unwrap(), r.read_varint().unwrap()], [1, 0]);
    assert_eq!(r.read_varint().unwrap(), 49);
    assert_eq!(r.read_prefixed_bytes().unwrap(), &[1, 5, 0, 0, 0]);
    assert!(r.is_exhausted());
    // Leer heisst Anzahl 0.
    assert_eq!(set_creative_mode_slot_payload(-1, None).unwrap()[1..], [0xFF, 0xFF, 0]);
}

#[test]
fn faehigkeiten_lesen_und_flug_melden() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_PLAYER_ABILITIES).write_u8(ABILITY_INVULNERABLE | ABILITY_MAY_FLY | ABILITY_INSTABUILD).write_f32(0.05).write_f32(0.1);
    let a = read_player_abilities(&w.into_bytes()).unwrap();
    assert_eq!(a, PlayerAbilities { flags: 13, fly_speed: 0.05, walk_speed: 0.1 });
    assert_eq!(player_abilities_payload(true), vec![SB_PLAY_PLAYER_ABILITIES as u8, 2]);
    assert_eq!(player_abilities_payload(false), vec![SB_PLAY_PLAYER_ABILITIES as u8, 0]);
}

#[test]
fn elytra_start_ist_player_command_sechs() {
    let bytes = player_command_start_fall_flying_payload(7);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_PLAYER_COMMAND);
    assert_eq!(r.read_varint().unwrap(), 7);
    assert_eq!(r.read_varint().unwrap(), 6, "START_FALL_FLYING");
    assert_eq!(r.read_varint().unwrap(), 0);
    assert!(r.is_exhausted());
}

#[test]
fn raketen_schuetze_aus_optional_int() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(12);
    // Index 8: das Raketen-Item (leer), Index 9: Schuetze 41 (auf der Leitung 42).
    w.write_u8(8).write_varint(DATA_TYPE_ITEM_STACK).write_varint(0);
    w.write_u8(FIREWORK_INDEX_SHOOTER).write_varint(DATA_TYPE_OPTIONAL_INT).write_varint(42);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.firework_shooter, Some(Some(41)));

    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(12);
    w.write_u8(FIREWORK_INDEX_SHOOTER).write_varint(DATA_TYPE_OPTIONAL_INT).write_varint(0);
    w.write_u8(0xFF);
    assert_eq!(read_set_entity_data(&w.into_bytes()).unwrap().firework_shooter, Some(None));
}

#[test]
fn set_entity_data_liest_die_pose_hinter_uebersprungenen_feldern() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(77);
    // Index 0, Serializer BYTE: die Entity-Flags (hier: schleichend).
    w.write_u8(0).write_varint(DATA_TYPE_BYTE).write_u8(ENTITY_FLAG_SNEAKING);
    // Index 1, Serializer INTEGER: Luft -- interessiert uns nicht, muss aber korrekt
    // uebersprungen werden, sonst verrutscht alles dahinter.
    w.write_u8(1).write_varint(DATA_TYPE_INT).write_varint(300);
    // Index 3, Serializer BOOLEAN.
    w.write_u8(3).write_varint(DATA_TYPE_BOOLEAN).write_bool(false);
    // Index 6, Serializer ENTITY_POSE: CROUCHING (5).
    w.write_u8(6).write_varint(DATA_TYPE_ENTITY_POSE).write_varint(5);
    w.write_u8(0xFF);

    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.entity_id, 77);
    assert_eq!(update.pose, Some(5));
    assert_eq!(update.flags, Some(ENTITY_FLAG_SNEAKING));
    assert_eq!(update.firework_shooter, None);
}

#[test]
fn set_entity_data_bricht_bei_unbekanntem_serializer_ab_statt_zu_raten() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(5);
    w.write_u8(6).write_varint(DATA_TYPE_ENTITY_POSE).write_varint(3);
    // Ein Typ, dessen Laenge wir nicht kennen (hier: ITEM_STACK). Ab hier ist der Strom
    // nicht mehr zerlegbar -- was davor stand, bleibt trotzdem gueltig.
    w.write_u8(8).write_varint(7).write_u8(0);
    w.write_u8(0xFF);

    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.pose, Some(3), "das Feld VOR dem unbekannten Typ zaehlt weiter");
}

#[test]
// Ein Dorfbewohner: Baby-Flag (16), Kopfschuettel-Ticks (17), VillagerData (18) -- danach
// muss der Strom weiterlaufen (Zustands-Enum und GlobalPos stehen hier als Probe dahinter).
fn set_entity_data_liest_dorfbewohner_daten() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_SET_ENTITY_DATA).write_varint(9);
    w.write_u8(16).write_varint(DATA_TYPE_BOOLEAN).write_bool(false);
    w.write_u8(18).write_varint(DATA_TYPE_VILLAGER_DATA).write_varint(4).write_varint(9).write_varint(3);
    w.write_u8(19).write_varint(DATA_TYPE_SNIFFER_STATE).write_varint(2);
    w.write_u8(20).write_varint(DATA_TYPE_OPTIONAL_GLOBAL_POS).write_bool(true).write_string("minecraft:overworld").write_i64(5);
    w.write_u8(6).write_varint(DATA_TYPE_ENTITY_POSE).write_varint(1);
    w.write_u8(0xFF);
    let update = read_set_entity_data(&w.into_bytes()).unwrap();
    assert_eq!(update.villager_data, Some((4, 9, 3)));
    assert_eq!(update.pose, Some(1), "nach den neuen Typen geht es weiter");
}

#[test]
fn use_item_on_hat_das_blockhitresult_format() {
    let bytes = use_item_on_payload(HAND_MAIN, -7, -12, 300, FACE_UP, 0.5, 1.0, 0.25, false, 42);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_USE_ITEM_ON);
    assert_eq!(r.read_varint().unwrap(), HAND_MAIN);
    // Negative Koordinaten sind der interessante Fall: die Packung schneidet auf 26 bzw.
    // 12 Bit zu, das Vorzeichen muss beim Entpacken wieder herauskommen.
    assert_eq!(decode_block_pos(r.read_i64().unwrap()), (-7, -12, 300));
    assert_eq!(r.read_varint().unwrap(), FACE_UP);
    assert_eq!(r.read_f32().unwrap(), 0.5);
    assert_eq!(r.read_f32().unwrap(), 1.0);
    assert_eq!(r.read_f32().unwrap(), 0.25);
    assert!(!r.read_bool().unwrap(), "im Block");
    assert!(!r.read_bool().unwrap(), "an der Weltgrenze");
    assert_eq!(r.read_varint().unwrap(), 42);
    assert!(r.is_exhausted());
}

#[test]
fn use_item_traegt_blickrichtung_und_sequenz() {
    let bytes = use_item_payload(HAND_OFF, 7, -90.5, 12.25);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_USE_ITEM);
    assert_eq!(r.read_varint().unwrap(), HAND_OFF);
    assert_eq!(r.read_varint().unwrap(), 7);
    assert_eq!(r.read_f32().unwrap(), -90.5);
    assert_eq!(r.read_f32().unwrap(), 12.25);
    assert!(r.is_exhausted());
}

// Die beiden Stapel-IDs standen einmal eins zu niedrig und haben damit den Haenger beim
// Verbinden verursacht (s. CB_PLAY_CHUNK_BATCH_FINISHED). Hier festgenagelt an den
// Nachbarn aus der Registrierungskette, die empirisch am echten Server verifiziert sind
// -- eine erneute Verschiebung faellt damit im Test auf, nicht erst im Spiel.
#[test]
fn chunk_stapel_ids_liegen_zwischen_ihren_verifizierten_nachbarn() {
    assert_eq!(CB_PLAY_BLOCK_UPDATE, 0x08, "verifizierter Anker vor der Kette");
    assert_eq!(CB_PLAY_CHANGE_DIFFICULTY, CB_PLAY_BLOCK_UPDATE + 2);
    assert_eq!(CB_PLAY_CHUNK_BATCH_FINISHED, CB_PLAY_CHANGE_DIFFICULTY + 1);
    assert_eq!(CB_PLAY_CHUNK_BATCH_START, CB_PLAY_CHUNK_BATCH_FINISHED + 1);
    // CHUNKS_BIOMES liegt dazwischen, dann Clear Titles -- ebenfalls verifiziert.
    assert_eq!(CB_PLAY_CLEAR_TITLES, CB_PLAY_CHUNK_BATCH_START + 2);
}

// Rueckkehr in die Konfiguration: dieselbe Absicherung ueber die Nachbarn. Zwischen
// SET_TITLES_ANIMATION (0x71, verifiziert) und SYSTEM_CHAT (0x77, verifiziert) liegen
// Sound Entity, Sound, Start Configuration, Stop Sound und Store Cookie.
#[test]
fn start_configuration_liegt_zwischen_seinen_verifizierten_nachbarn() {
    assert_eq!(CB_PLAY_START_CONFIGURATION, CB_PLAY_SET_TITLES_ANIMATION + 3);
    assert_eq!(CB_PLAY_SYSTEM_CHAT, CB_PLAY_START_CONFIGURATION + 3);
    // Serverbound direkt hinter Command Suggestion (0x0E, verifiziert).
    assert_eq!(SB_PLAY_CONFIGURATION_ACKNOWLEDGED, SB_PLAY_COMMAND_SUGGESTION + 1);
}

// Reines Signalpaket -- nur die ID, sonst nichts. Ein zusaetzliches Feld waere ein
// Protokollfehler und der Server wuerde die Verbindung trennen.
#[test]
fn konfigurations_bestaetigung_hat_keinen_rumpf() {
    let bytes = configuration_acknowledged_payload();
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CONFIGURATION_ACKNOWLEDGED);
    assert!(r.is_exhausted());
}

// Warum die Verwechslung so lange unauffaellig blieb: das Difficulty-Paket laesst sich
// klaglos als Stapel-Ende lesen. Sein erstes Feld ist ein Byte (Schwierigkeit), das als
// VarInt eine voellig plausible Stapelgroesse ergibt -- im Log stand deshalb
// "2 Chunks", obwohl 2 nur NORMAL bedeutet.
#[test]
fn difficulty_paket_sieht_wie_ein_stapel_ende_aus() {
    let mut w = PacketWriter::new();
    w.write_varint(CB_PLAY_CHANGE_DIFFICULTY).write_u8(2).write_u8(0); // NORMAL, nicht gesperrt
    assert_eq!(read_chunk_batch_size(&w.into_bytes()).unwrap(), 2);
}

// Ohne Schluessel bleibt das Signaturfeld leer -- genau das lehnen Server mit
// enforce-secure-profile ab, auf allen anderen funktioniert es weiterhin.
#[test]
fn unsignierte_nachricht_bleibt_moeglich() {
    let bytes = signed_chat_message_payload("hi", 1, 0, None);
    let mut r = PacketReader::new(&bytes);
    let _ = r.read_varint();
    let _ = r.read_string();
    let _ = r.read_i64();
    let _ = r.read_i64();
    assert!(!r.read_bool().unwrap());
}

// Wegwerfen/Zweithand: Aktion als VarInt, danach der (hier bedeutungslose) Block.
#[test]
fn player_action_paket_hat_die_richtige_reihenfolge() {
    let bytes = player_action_payload(PLAYER_ACTION_SWAP_OFFHAND);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_PLAYER_ACTION);
    assert_eq!(r.read_varint().unwrap(), 6);
    assert_eq!(r.read_i64().unwrap(), 0); // BlockPos.ORIGIN
    assert_eq!(r.read_u8().unwrap(), 0); // Richtung DOWN
    assert_eq!(r.read_varint().unwrap(), 0); // Sequenznummer
    assert!(r.is_exhausted());
}

// Player Session: UUID, Ablaufzeit, dann beide Byte-Felder MIT VarInt-Laenge.
#[test]
fn chat_session_paket_hat_laengenpraefigierte_schluesselfelder() {
    let session = [3u8; 16];
    let public_key = vec![1, 2, 3, 4, 5];
    let key_signature = vec![9, 9];
    let bytes = chat_session_update_payload(session, 1_800_000_000_000, &public_key, &key_signature);
    let mut r = PacketReader::new(&bytes);
    assert_eq!(r.read_varint().unwrap(), SB_PLAY_CHAT_SESSION_UPDATE);
    assert_eq!(r.read_bytes(16).unwrap(), &session[..]);
    assert_eq!(r.read_i64().unwrap(), 1_800_000_000_000);
    assert_eq!(r.read_varint().unwrap(), 5);
    assert_eq!(r.read_bytes(5).unwrap(), &public_key[..]);
    assert_eq!(r.read_varint().unwrap(), 2);
    assert_eq!(r.read_bytes(2).unwrap(), &key_signature[..]);
    assert!(r.is_exhausted());
}
