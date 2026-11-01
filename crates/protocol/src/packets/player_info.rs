// Spielerliste.

use super::*;

// --- Player Info Update (Spielerliste) ---
//
// ID aus der Registrierungsreihenfolge in PlayStateFactories.S2C abgeleitet und gegen SECHS
// bereits verifizierte IDs gegengeprueft (ADD_ENTITY 0x01, LEVEL_CHUNK_WITH_LIGHT 0x2C,
// PING 0x3B, PLAYER_POSITION 0x46, SET_HEALTH 0x66 -- alle stimmen mit derselben Zaehlung).
pub const CB_PLAY_PLAYER_INFO_UPDATE: i32 = 0x44;
pub const CB_PLAY_PLAYER_INFO_REMOVE: i32 = 0x43;

// Ein Eintrag der Spielerliste, soweit wir ihn brauchen: wer es ist und (falls mitgeschickt)
// der rohe Wert der "textures"-Property. Der ist Base64 mit JSON darin und wird NICHT hier
// dekodiert -- protocol bleibt reines Wire-Parsen, das Auspacken macht der Aufrufer.
//
// Die uebrigen Felder sind die Aktionen fuer die Tab-Liste. None = die Aktion stand nicht im
// Paket, der bisherige Wert bleibt (Vanilla wendet nur die mitgeschickten Aktionen an).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PlayerInfoEntry {
    pub uuid: [u8; 16],
    pub name: Option<String>,
    pub textures_property: Option<String>,
    pub game_mode: Option<i32>,
    pub listed: Option<bool>,
    pub latency: Option<i32>,
    // Some(None) = Anzeigename ausdruecklich entfernt, None = nicht im Paket.
    pub display_name: Option<Option<Vec<crate::nbt::TextSpan>>>,
    pub list_order: Option<i32>,
    pub show_hat: Option<bool>,
}

// PlayerRemoveS2CPacket: eine Liste von UUIDs, die aus der Spielerliste verschwinden.
#[must_use] 
pub fn parse_player_info_remove(bytes: &[u8]) -> Option<Vec<[u8; 16]>> {
    let mut r = PacketReader::new(bytes);
    let _id = r.read_varint().ok()?;
    let count = r.read_varint().ok()?;
    if !(0..=4096).contains(&count) {
        return None;
    }
    (0..count).map(|_| r.read_uuid().ok().map(|u| u.0)).collect()
}

// PlayerListHeaderS2CPacket (TAB_LIST): Kopf- und Fusszeile der Tab-Liste, zwei Text-Components.
// ID abgezaehlt aus PlayStateFactories.S2C: direkt nach SYSTEM_CHAT (0x77).
pub const CB_PLAY_TAB_LIST: i32 = 0x78;

pub fn read_tab_list(payload: &[u8]) -> Result<(Vec<crate::nbt::TextSpan>, Vec<crate::nbt::TextSpan>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let header = read_text_spans(&mut r)?;
    let footer = read_text_spans(&mut r)?;
    Ok((header, footer))
}

// PlayerListS2CPacket, aus dem 1.21.11-Jar abgelesen:
//   VarInt  Paket-ID (CB_PLAY_PLAYER_INFO_UPDATE -- `bytes` ist die ganze Nutzlast INKLUSIVE
//           ID, wie bei jedem read_*/parse_* hier)
//   1 Byte  Aktions-Bitmaske (readBitSet(8) -> genau ein Byte, Bit 0 = ADD_PLAYER)
//   VarInt  Anzahl Eintraege
//   je Eintrag: 16 Byte UUID, dann fuer JEDE gesetzte Aktion in Enum-Reihenfolge ihre Felder
//
// Alle Aktionen muessen mitgelesen werden, auch die, die uns nicht interessieren: die
// Eintraege liegen hintereinander im selben Puffer, ein uebersprungenes Feld verschiebt
// alles danach.
#[must_use] 
pub fn parse_player_info_update(bytes: &[u8]) -> Option<Vec<PlayerInfoEntry>> {
    let mut r = PacketReader::new(bytes);
    let _id = r.read_varint().ok()?;
    let actions = r.read_u8().ok()?;
    let has = |bit: u8| actions & (1 << bit) != 0;
    let count = r.read_varint().ok()?;
    if !(0..=4096).contains(&count) {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let uuid = r.read_uuid().ok()?.0;
        let mut entry = PlayerInfoEntry { uuid, ..Default::default() };
        if has(0) {
            // ADD_PLAYER: Name + PropertyMap (Name, Wert, optionale Signatur).
            entry.name = Some(r.read_string().ok()?);
            let properties = r.read_varint().ok()?;
            if !(0..=64).contains(&properties) {
                return None;
            }
            for _ in 0..properties {
                let key = r.read_string().ok()?;
                let value = r.read_string().ok()?;
                if r.read_bool().ok()? {
                    let _signature = r.read_string().ok()?;
                }
                if key == "textures" {
                    entry.textures_property = Some(value);
                }
            }
        }
        if has(1) {
            // INITIALIZE_CHAT: optionale Sitzung (UUID, Ablaufzeit, oeffentlicher Schluessel,
            // Signatur) -- nur ueberlesen.
            if r.read_bool().ok()? {
                let _session = r.read_uuid().ok()?;

                let _expires = r.read_i64().ok()?;
                for _ in 0..2 {
                    // oeffentlicher Schluessel und Signatur, je VarInt-Laenge + Bytes
                    let len = r.read_varint().ok()?;
                    if len < 0 {
                        return None;
                    }
                    r.read_bytes(len as usize).ok()?;
                }
            }
        }
        if has(2) {
            entry.game_mode = Some(r.read_varint().ok()?);
        }
        if has(3) {
            entry.listed = Some(r.read_bool().ok()?);
        }
        if has(4) {
            entry.latency = Some(r.read_varint().ok()?);
        }
        if has(5) {
            // UPDATE_DISPLAY_NAME: optionale Text-Komponente als NBT.
            entry.display_name = Some(if r.read_bool().ok()? { Some(read_text_spans(&mut r).ok()?) } else { None });
        }
        if has(6) {
            entry.list_order = Some(r.read_varint().ok()?);
        }
        if has(7) {
            entry.show_hat = Some(r.read_bool().ok()?);
        }
        out.push(entry);
    }
    Some(out)
}

// EntityStatusS2CPacket ("Entity Event"): ein Byte-Ereignis fuer ein Entity. Vanilla schickt
// darueber Dinge, die keinen eigenen Pakettyp verdienen -- Schaden-Blitzen, Explosionen,
// Zaehmen, und den Fall, um den es hier geht: ein Totem hat gepoppt.
//
// Die ID (0x22 = 34) stammt aus der S2C-Registrierungsreihenfolge in PlayStateFactories,
// gegengeprueft an CONTAINER_SET_SLOT (Position 20 = 0x14) und SYSTEM_CHAT (119 = 0x77) --
// beides Pakete, die dieser Client nachweislich richtig empfaengt.
pub const CB_PLAY_ENTITY_EVENT: i32 = 0x22;

// EntityStatuses.USE_TOTEM_OF_UNDYING. Der Server schickt das an alle Spieler, die das
// Entity sehen -- also auch an einen selbst (LivingEntity.tryUseDeathProtector ruft
// sendEntityStatus(this, 35), nachdem das Totem verbraucht ist).
//
// WICHTIG: der Status sagt NICHT, aus welcher Hand das Totem kam. Das Paket besteht nur aus
// EntityId und Status. Welche Hand es war, ist aber trotzdem eindeutig bestimmbar, s.
// app::modules::TotemTracker.
pub const ENTITY_STATUS_USE_TOTEM: i8 = 35;

// EntityAnimationS2CPacket ("Animate"): ein Entity hat mit der Hand geschlagen, ist aufgewacht
// oder hat einen Kritischen gelandet.
//
// DIE einzige Quelle fuer den Armschwung fremder Spieler. Vanilla rechnet ihn NICHT aus der
// Bewegung aus (anders als das Schrittschwingen) -- ohne dieses Paket steht der Arm eines
// Gegners im Nahkampf vollkommen still, egal wie schnell er klickt.
//
// Die ID 0x02 ist die dritte Position der S2C-Registrierungsreihenfolge in PlayStateFactories
// (0 = BundleDelimiter, 1 = EntitySpawn, 2 = EntityAnimation, 3 = Statistics, 4 =
// PlayerActionResponse). Position 4 ist unser bereits verifiziertes BLOCK_CHANGED_ACK = 0x04
// und Position 8 das ebenfalls verifizierte BLOCK_UPDATE = 0x08 -- die Zaehlung sitzt also.
pub const CB_PLAY_ANIMATE: i32 = 0x02;

// Die Animationsarten aus EntityAnimationS2CPacket. Die 1 fehlt in Vanilla ebenfalls (frueher
// "Schaden genommen", heute ein eigenes Paket) -- die Luecke ist keine Zaehlpanne.
pub const ANIMATION_SWING_MAIN_HAND: u8 = 0;
pub const ANIMATION_WAKE_UP: u8 = 2;
pub const ANIMATION_SWING_OFF_HAND: u8 = 3;
pub const ANIMATION_CRIT: u8 = 4;
pub const ANIMATION_ENCHANTED_HIT: u8 = 5;

#[cfg(test)]
mod player_info_tests {
    use super::*;

    fn write_string(out: &mut Vec<u8>, text: &str) {
        let mut w = PacketWriter::new();
        w.write_string(text);
        out.extend_from_slice(&w.into_bytes());
    }

    fn write_varint(out: &mut Vec<u8>, value: i32) {
        let mut w = PacketWriter::new();
        w.write_varint(value);
        out.extend_from_slice(&w.into_bytes());
    }

    // Der Anfang eines echten Pakets: ID, dann die Aktions-Bitmaske. Die ID gehoert
    // ZWINGEND dazu -- `parse_player_info_update` bekommt (wie jeder read_*/parse_* hier)
    // die vollstaendige Nutzlast. Genau daran ist die erste Fassung gescheitert: die Tests
    // bauten den Puffer ohne ID, der Parser las ohne ID, beide waren sich einig -- und im
    // Spiel landete die Paket-ID 0x44 in der Bitmaske und ergab "kein ADD_PLAYER".
    // Deshalb baut dieser Helfer den Kopf aus der KONSTANTE, nicht aus einer Zahl im Test.
    fn packet_head(actions: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_varint(&mut bytes, CB_PLAY_PLAYER_INFO_UPDATE);
        bytes.push(actions);
        bytes
    }

    // Der Normalfall: ADD_PLAYER zusammen mit den Aktionen, die ein echter Server
    // mitschickt. Alle muessen mitgelesen werden, sonst verschiebt sich der Lesezeiger und
    // der zweite Eintrag kommt als Muell heraus -- deshalb hier ZWEI Eintraege.
    #[test]
    fn liest_zwei_eintraege_mit_mehreren_aktionen() {
        // ADD_PLAYER | UPDATE_GAME_MODE | UPDATE_LISTED | UPDATE_LATENCY | UPDATE_HAT
        let actions = (1 << 0) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 7);
        let mut bytes = packet_head(actions as u8);
        write_varint(&mut bytes, 2);
        for (uuid_byte, name, textures) in [(1u8, "Spieler1", "AAAA"), (2u8, "Spieler2", "BBBB")] {
            bytes.extend_from_slice(&[uuid_byte; 16]);
            write_string(&mut bytes, name);
            write_varint(&mut bytes, 2); // zwei Properties
            write_string(&mut bytes, "irgendwas");
            write_string(&mut bytes, "wert");
            bytes.push(0); // keine Signatur
            write_string(&mut bytes, "textures");
            write_string(&mut bytes, textures);
            bytes.push(1); // mit Signatur
            write_string(&mut bytes, "sig");
            write_varint(&mut bytes, 0); // game mode
            bytes.push(1); // listed
            write_varint(&mut bytes, 42); // latency
            bytes.push(1); // show hat
        }

        let entries = parse_player_info_update(&bytes).expect("Paket muss lesbar sein");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].uuid, [1u8; 16]);
        assert_eq!(entries[0].name.as_deref(), Some("Spieler1"));
        assert_eq!(entries[0].textures_property.as_deref(), Some("AAAA"));
        assert_eq!(entries[1].uuid, [2u8; 16]);
        assert_eq!(entries[1].name.as_deref(), Some("Spieler2"));
        assert_eq!(entries[1].textures_property.as_deref(), Some("BBBB"));
    }

    // Die Tab-Liste braucht die Werte der uebrigen Aktionen: Spielmodus, gelistet, Ping,
    // Anzeigename (als farbige Abschnitte), Reihenfolge und Hut. Ein zweiter Eintrag danach
    // prueft, dass der Lesezeiger stimmt.
    #[test]
    fn liest_die_tab_listen_felder() {
        // GAME_MODE | LISTED | LATENCY | DISPLAY_NAME | LIST_ORDER | HAT
        let mut bytes = packet_head(0b1111_1100);
        write_varint(&mut bytes, 2);
        for (uuid_byte, mit_name) in [(3u8, true), (4u8, false)] {
            bytes.extend_from_slice(&[uuid_byte; 16]);
            write_varint(&mut bytes, 3); // Zuschauer
            bytes.push(1); // gelistet
            write_varint(&mut bytes, 250);
            if mit_name {
                bytes.push(1);
                // NBT-String-Tag (Netzwerk-NBT ohne Namen): Typ 8, Laenge, Text mit Farbcode.
                bytes.push(8);
                let text = "\u{a7}cRot";
                bytes.extend_from_slice(&(text.len() as u16).to_be_bytes());
                bytes.extend_from_slice(text.as_bytes());
            } else {
                bytes.push(0);
            }
            write_varint(&mut bytes, 7);
            bytes.push(0); // Hut aus
        }
        let entries = parse_player_info_update(&bytes).expect("muss lesbar sein");
        assert_eq!(entries.len(), 2);
        let e = &entries[0];
        assert_eq!((e.game_mode, e.listed, e.latency, e.list_order, e.show_hat), (Some(3), Some(true), Some(250), Some(7), Some(false)));
        let name = e.display_name.clone().flatten().expect("Anzeigename fehlt");
        assert_eq!(name.iter().map(|s| s.text.as_str()).collect::<String>(), "Rot");
        assert_eq!(name[0].color, Some(0xFF5555));
        assert_eq!(entries[1].display_name, Some(None), "ausdruecklich entfernter Anzeigename");
        assert_eq!(entries[1].list_order, Some(7));
    }

    #[test]
    fn liest_das_entfernen_aus_der_liste() {
        let mut bytes = Vec::new();
        write_varint(&mut bytes, CB_PLAY_PLAYER_INFO_REMOVE);
        write_varint(&mut bytes, 2);
        bytes.extend_from_slice(&[5u8; 16]);
        bytes.extend_from_slice(&[6u8; 16]);
        assert_eq!(parse_player_info_remove(&bytes), Some(vec![[5u8; 16], [6u8; 16]]));
    }

    #[test]
    fn liest_kopf_und_fusszeile() {
        let mut bytes = Vec::new();
        write_varint(&mut bytes, CB_PLAY_TAB_LIST);
        for text in ["\u{a7}6Server", ""] {
            bytes.push(8);
            bytes.extend_from_slice(&(text.len() as u16).to_be_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        let (kopf, fuss) = read_tab_list(&bytes).expect("muss lesbar sein");
        assert_eq!(kopf.len(), 1);
        assert_eq!(kopf[0].text, "Server");
        assert_eq!(kopf[0].color, Some(0xFFAA00));
        assert!(fuss.iter().all(|s| s.text.is_empty()));
    }

    // Objective mit HEARTS und festem Zahlenformat: beides muss ankommen, sonst zeigt die
    // Tab-Liste Zahlen statt Herzen.
    #[test]
    fn liest_darstellung_und_zahlenformat_eines_objectives() {
        let mut bytes = Vec::new();
        write_varint(&mut bytes, CB_PLAY_SET_OBJECTIVE);
        write_string(&mut bytes, "leben");
        bytes.push(0); // anlegen
        bytes.push(8);
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(b"HP");
        write_varint(&mut bytes, 1); // HEARTS
        bytes.push(1); // Zahlenformat vorhanden
        write_varint(&mut bytes, 0); // blank
        let o = read_objective(&bytes).expect("muss lesbar sein");
        assert!(o.hearts);
        assert_eq!(o.number_format, Some(NumberFormat::Blank));
    }

    // Ein Paket OHNE ADD_PLAYER (nur eine Latenz-Aktualisierung) traegt keine Texturen --
    // es darf aber auch nicht als Fehler gelten, sonst verliert man die Liste.
    #[test]
    fn latenz_aktualisierung_ohne_add_player() {
        let mut bytes = packet_head(1 << 4);
        write_varint(&mut bytes, 1);
        bytes.extend_from_slice(&[7u8; 16]);
        write_varint(&mut bytes, 15);
        let entries = parse_player_info_update(&bytes).expect("muss lesbar sein");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uuid, [7u8; 16]);
        assert!(entries[0].name.is_none());
        assert!(entries[0].textures_property.is_none());
    }

    // Das Paket, mit dem es im Spiel tatsaechlich schiefgegangen ist:
    // PlayerListS2CPacket.entryFromPlayer setzt ALLE acht Aktionen (Bitmaske 0xFF), und
    // genau dieses Paket schickt der Server beim Join -- also das einzige, aus dem der Skin
    // ueberhaupt kommen kann. Die alten Tests deckten nur Teilmengen ab und bauten den
    // Puffer ohne Paket-ID, deshalb ist der Fehler durchgerutscht.
    //
    // Reihenfolge der Felder ist die Enum-Reihenfolge aus dem 1.21.11-Code. Achtung:
    // UPDATE_LIST_ORDER (Bit 6) kommt VOR UPDATE_HAT (Bit 7) -- vertauscht man die zwei,
    // faellt es bei einem Eintrag nicht auf und beim zweiten wird alles Muell.
    #[test]
    fn vollstaendiges_join_paket_liefert_texturen() {
        let mut bytes = packet_head(0xFF);
        write_varint(&mut bytes, 2);
        for (uuid_byte, name, textures) in [(3u8, "Erster", "AAAA"), (4u8, "Zweiter", "BBBB")] {
            bytes.extend_from_slice(&[uuid_byte; 16]);
            // ADD_PLAYER
            write_string(&mut bytes, name);
            write_varint(&mut bytes, 1);
            write_string(&mut bytes, "textures");
            write_string(&mut bytes, textures);
            bytes.push(1); // signiert, wie auf einem Online-Mode-Server
            write_string(&mut bytes, "signatur");
            // INITIALIZE_CHAT: keine Sitzung
            bytes.push(0);
            write_varint(&mut bytes, 0); // UPDATE_GAME_MODE
            bytes.push(1); // UPDATE_LISTED
            write_varint(&mut bytes, 42); // UPDATE_LATENCY
                                          // UPDATE_DISPLAY_NAME: vorhanden, als NBT-String (Netzwerk-NBT, ohne Wurzelnamen)
            bytes.push(1);
            bytes.push(8);
            bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            write_varint(&mut bytes, 0); // UPDATE_LIST_ORDER
            bytes.push(1); // UPDATE_HAT
        }

        // Der Puffer muss auch fuer den Dispatcher wie dieses Paket aussehen -- sonst
        // testet man einen Fall, der nie beim Parser ankommt.
        assert_eq!(peek_packet_id(&bytes).unwrap(), CB_PLAY_PLAYER_INFO_UPDATE);

        let entries = parse_player_info_update(&bytes).expect("Join-Paket muss lesbar sein");
        assert_eq!(entries.len(), 2, "der zweite Eintrag verraet einen verschobenen Lesezeiger");
        assert_eq!(entries[0].name.as_deref(), Some("Erster"));
        assert_eq!(entries[0].textures_property.as_deref(), Some("AAAA"));
        assert_eq!(entries[1].uuid, [4u8; 16]);
        assert_eq!(entries[1].name.as_deref(), Some("Zweiter"));
        assert_eq!(entries[1].textures_property.as_deref(), Some("BBBB"));
    }

    // Abgeschnittenes Paket: None statt Panik. Ein halb gelesener Puffer darf den Client
    // nicht umbringen.
    #[test]
    fn abgeschnittenes_paket_gibt_none() {
        let mut bytes = packet_head(1);
        write_varint(&mut bytes, 3);
        bytes.extend_from_slice(&[1u8; 10]); // UUID unvollstaendig
        assert!(parse_player_info_update(&bytes).is_none());
    }

    // Unsinnige Eintragszahl darf keine riesige Allokation ausloesen.
    #[test]
    fn absurde_eintragszahl_gibt_none() {
        let mut bytes = packet_head(1);
        write_varint(&mut bytes, 1_000_000);
        assert!(parse_player_info_update(&bytes).is_none());
    }
}
