// Befehlsvorschlaege, Kopfdrehung und Geraeusche.

use super::*;

// RequestCommandCompletionsC2SPacket: VarInt-Transaktions-ID + der komplette getippte Text.
//
// `text` enthaelt den fuehrenden Schraegstrich. Das ist nicht willkuerlich: Vanilla schickt
// `context.getInput()`, und der Brigadier-Reader wird mit dem GANZEN Feldinhalt gebaut (der
// skip() danach bewegt nur den Cursor, nicht den Puffer). Genauso wichtig fuer die Antwort:
// deren `start` ist ein Offset in eben diesen String -- schickt man ihn ohne Schraegstrich,
// landet jede Ergaenzung um ein Zeichen verschoben.
#[must_use] 
pub fn command_suggestion_payload(completion_id: i32, text: &str) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_COMMAND_SUGGESTION).write_varint(completion_id).write_string(text);
    w.into_bytes()
}

// CommandSuggestionsS2CPacket: die Antwort. `start`/`length` bezeichnen den Bereich im
// angefragten Text, der durch einen Vorschlag ERSETZT wird -- nicht das, was man anhaengt.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommandSuggestions {
    pub id: i32,
    pub start: i32,
    pub length: i32,
    pub matches: Vec<String>,
}

#[must_use] 
pub fn read_command_suggestions(payload: &[u8]) -> Option<CommandSuggestions> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint().ok()?;
    let id = r.read_varint().ok()?;
    let start = r.read_varint().ok()?;
    let length = r.read_varint().ok()?;
    let count = r.read_varint().ok()?;
    // Eine Obergrenze wie bei der Spielerliste: ein manipulierter Server soll keine
    // Riesenallokation ausloesen koennen. Vanillas Vorschlagsliste zeigt ohnehin nur eine
    // Handvoll auf einmal.
    if !(0..=4096).contains(&count) {
        return None;
    }
    let mut matches = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let text = r.read_string().ok()?;
        // Optionaler Tooltip als Text-Komponente (NBT). Wir zeigen ihn nicht an, muessen ihn
        // aber vollstaendig lesen -- sonst verschiebt sich der Lesezeiger und alle folgenden
        // Vorschlaege werden Muell.
        if r.read_bool().ok()? {
            crate::nbt::read_root(&mut r).ok()?;
        }
        matches.push(text);
    }
    Some(CommandSuggestions { id, start, length, matches })
}

#[cfg(test)]
mod command_suggestion_tests {
    use super::*;

    #[test]
    // Die Anfrage: ID + Text, und der Text MIT Schraegstrich (s. Funktionskommentar).
    fn anfrage_enthaelt_den_schraegstrich() {
        let bytes = command_suggestion_payload(7, "/tp Der");
        let mut r = PacketReader::new(&bytes);
        assert_eq!(r.read_varint().unwrap(), SB_PLAY_COMMAND_SUGGESTION);
        assert_eq!(r.read_varint().unwrap(), 7);
        assert_eq!(r.read_string().unwrap(), "/tp Der");
    }

    fn antwort(id: i32, start: i32, length: i32, eintraege: &[(&str, bool)]) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_COMMAND_SUGGESTIONS).write_varint(id).write_varint(start).write_varint(length);
        w.write_varint(eintraege.len() as i32);
        for (text, mit_tooltip) in eintraege {
            w.write_string(text);
            w.write_u8(u8::from(*mit_tooltip));
            if *mit_tooltip {
                // Netzwerk-NBT: ein blanker String-Tag (Typ 8, Laenge, Bytes), ohne Wurzelname.
                w.write_u8(8);
                w.write_u8(0);
                w.write_u8(4);
                for b in b"Info" {
                    w.write_u8(*b);
                }
            }
        }
        w.into_bytes()
    }

    #[test]
    fn liest_vorschlaege_mit_und_ohne_tooltip() {
        let bytes = antwort(7, 4, 3, &[("DerAntonia", false), ("Derp", true), ("Dersch", false)]);
        let s = read_command_suggestions(&bytes).expect("lesbar");
        assert_eq!((s.id, s.start, s.length), (7, 4, 3));
        assert_eq!(s.matches, vec!["DerAntonia", "Derp", "Dersch"]);
    }

    #[test]
    // Der Tooltip MUSS mitgelesen werden, auch wenn er nicht angezeigt wird: sonst
    // verschiebt sich der Lesezeiger und der Vorschlag DAHINTER wird Muell. Genau deshalb
    // steht in diesem Test ein Eintrag mit Tooltip VOR einem ohne.
    fn ein_uebersprungener_tooltip_wuerde_den_rest_zerstoeren() {
        let bytes = antwort(1, 0, 0, &[("mit", true), ("ohne", false)]);
        let s = read_command_suggestions(&bytes).expect("lesbar");
        assert_eq!(s.matches, vec!["mit", "ohne"], "der zweite Eintrag verraet einen verschobenen Lesezeiger");
    }

    #[test]
    fn leere_liste_ist_gueltig() {
        let s = read_command_suggestions(&antwort(3, 1, 0, &[])).expect("lesbar");
        assert!(s.matches.is_empty());
    }

    #[test]
    fn absurde_anzahl_und_abgeschnittenes_paket_geben_none() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_COMMAND_SUGGESTIONS).write_varint(1).write_varint(0).write_varint(0).write_varint(1_000_000);
        assert!(read_command_suggestions(&w.into_bytes()).is_none());
        assert!(read_command_suggestions(&[0x0F, 0x01]).is_none());
    }
}

// EntitySetHeadYawS2CPacket ("Set Head Rotation"): VarInt EntityId + ein Winkelbyte.
//
// Bei Lebewesen ist die Kopfdrehung eine EIGENE Groesse, getrennt von der Koerperdrehung aus
// den Bewegungspaketen. Genau deshalb gibt es dieses Paket ueberhaupt: der Server schickt es
// zusaetzlich zu Move Entity Rot, weil ein Mob (und ein Spieler) den Kopf drehen kann, ohne
// den Koerper mitzudrehen. Ohne dieses Paket schauen fremde Spieler immer stur in ihre
// Koerperrichtung.
//
// Die Konstante stand schon oben bei den Entity-IDs (0x51), war aber bisher ungenutzt -- die
// Ableitung aus der Registrierungsreihenfolge (Position 81, 1-basiert) bestaetigt sie.
pub fn read_rotate_head(payload: &[u8]) -> Result<(i32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let head_yaw_deg = angle_byte_to_deg(r.read_u8()?);
    Ok((entity_id, head_yaw_deg))
}

#[cfg(test)]
mod rotate_head_tests {
    use super::*;

    #[test]
    // Aufbau des Pakets und die Winkelumrechnung. Das Byte ist VORZEICHENBEHAFTET (256
    // Schritte = 360 Grad): als u8 gelesen wuerde aus -90 Grad plus 270.
    fn liest_id_und_winkel() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_ROTATE_HEAD);
        w.write_varint(4242);
        // -64 als Byte = -90 Grad.
        w.write_u8((-64i8) as u8);
        let (id, yaw) = read_rotate_head(&w.into_bytes()).expect("lesbar");
        assert_eq!(id, 4242);
        assert!((yaw - -90.0).abs() < 1e-4, "Winkel war {yaw}");
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_rotate_head(&[0x51]).is_err());
    }
}

// --- Ton (SOUND_ENTITY 0x72, SOUND 0x73) ---------------------------------------------------
//
// Beide IDs sind am Jar geprueft, nicht abgezaehlt: die .add()-Kette in
// net.minecraft.network.state.PlayStateFactories liefert sie zwischen den bereits
// verifizierten Nachbarn SET_TITLES_ANIMATION (0x71) und START_CONFIGURATION (0x74).

// PlaySoundS2CPacket: ein Geraeusch an einem festen ORT in der Welt.
pub const CB_PLAY_SOUND: i32 = 0x73;
// PlaySoundFromEntityS2CPacket: dasselbe, aber an eine Entity gebunden -- die Position muss
// der Client selbst nachschlagen (die Entity bewegt sich ja).
pub const CB_PLAY_SOUND_ENTITY: i32 = 0x72;

/// Die Lautstaerke-Kategorie eines Geraeuschs, in der Reihenfolge von
/// net.minecraft.sound.SoundCategory (die Zahl im Paket ist der Ordinalwert).
///
/// Gebraucht wird sie, damit spaeter je Kategorie geregelt werden kann, und fuer das
/// Stop-Sound-Paket, das nach Kategorie filtert (`as u8` ist die Paketzahl).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundCategory {
    Master,
    Music,
    Records,
    Weather,
    Blocks,
    Hostile,
    Neutral,
    Players,
    Ambient,
    Voice,
    // Klaenge der Oberflaeche (seit 1.21.6 eine eigene Kategorie).
    Ui,
}

impl SoundCategory {
    #[must_use] 
    pub const fn from_id(id: i32) -> Option<Self> {
        Some(match id {
            0 => Self::Master,
            1 => Self::Music,
            2 => Self::Records,
            3 => Self::Weather,
            4 => Self::Blocks,
            5 => Self::Hostile,
            6 => Self::Neutral,
            7 => Self::Players,
            8 => Self::Ambient,
            9 => Self::Voice,
            10 => Self::Ui,
            _ => return None,
        })
    }
}

/// Ein abzuspielendes Geraeusch, so wie es vom Server kommt.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct SoundEffect {
    /// Voller Registry-Name, z.B. "minecraft:entity.generic.explode".
    pub name: String,
    pub category: SoundCategory,
    /// Wo es passiert. `None` bei einem entity-gebundenen Geraeusch -- dort steht statt der
    /// Position eine Entity-ID (s. `entity_id`), deren Ort nur der Aufrufer kennt.
    pub position: Option<(f64, f64, f64)>,
    pub entity_id: Option<i32>,
    /// Lautstaerke, 1.0 = normal. Server schicken auch Werte ueber 1 -- das ist in Vanilla
    /// KEINE hoehere Lautstaerke, sondern eine groessere Reichweite.
    pub volume: f32,
    /// Wiedergabegeschwindigkeit, 1.0 = unveraendert.
    pub pitch: f32,
    /// Zufallssaat des Servers. Damit waehlen alle Clients DIESELBE Variante eines Klangs
    /// (ein Steinschritt hat sechs Aufnahmen) -- ohne sie wuerde jeder etwas anderes hoeren.
    pub seed: i64,
}

/// Den Klangnamen zu einer Paketzahl nachschlagen.
///
/// Die Zahl ist um EINS VERSCHOBEN: 0 bedeutet "kein Registry-Eintrag, die Beschreibung folgt
/// direkt im Paket" (so schicken Server eigene, nicht in Vanilla registrierte Klaenge), alles
/// darueber ist Registry-Index + 1. Das ist Vanillas `PacketCodecs.registryEntry`, kein
/// Sonderfall dieses Clients.
#[must_use] 
pub fn sound_event_name(index: usize) -> Option<&'static str> {
    crate::sound_events::SOUND_EVENTS.get(index).copied()
}

// Den Klang-Teil lesen (Vanillas SoundEvent.ENTRY_PACKET_CODEC).
//
// Unbekannte Registry-Zahlen sind KEIN Lesefehler: sie treten auf, sobald der Server eine
// neuere Version spricht als unsere generierte Tabelle kennt. Der Rest des Pakets muss
// trotzdem sauber zu Ende gelesen werden, sonst verliert die Verbindung den Tritt -- deshalb
// Ok(None) statt eines Fehlers.
pub(crate) fn read_sound_event_entry(r: &mut PacketReader) -> Result<Option<String>, ReadError> {
    read_sound_event(r)
}

pub(crate) fn read_sound_event(r: &mut PacketReader) -> Result<Option<String>, ReadError> {
    let id = r.read_varint()?;
    if id == 0 {
        // Eigener Klang des Servers: Name, dann optional eine feste Reichweite.
        let name = r.read_string()?;
        if r.read_bool()? {
            let _fixed_range = r.read_f32()?;
        }
        return Ok(Some(name));
    }
    Ok(sound_event_name((id - 1) as usize).map(|n| format!("minecraft:{n}")))
}

/// `PlaySoundS2CPacket` lesen.
///
/// `Ok(None)` = Paket war formal in Ordnung, aber der Klang ist uns unbekannt (s.
/// `read_sound_event`).
pub fn read_sound(payload: &[u8]) -> Result<Option<SoundEffect>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let name = read_sound_event(&mut r)?;
    let category = r.read_varint()?;
    // Die Position steckt als GANZE ZAHL in Achteln eines Blocks drin (Vanilla:
    // `(int)(x * 8.0)`), nicht als Gleitkomma. Das reicht voellig -- ein Achtel Block ist
    // weit unter dem, was sich raeumlich unterscheiden laesst -- spart aber gegenueber drei
    // f64 zwoelf Byte pro Geraeusch.
    let x = f64::from(r.read_i32()?) / 8.0;
    let y = f64::from(r.read_i32()?) / 8.0;
    let z = f64::from(r.read_i32()?) / 8.0;
    let volume = r.read_f32()?;
    let pitch = r.read_f32()?;
    let seed = r.read_i64()?;
    let Some(name) = name else { return Ok(None) };
    let Some(category) = SoundCategory::from_id(category) else { return Ok(None) };
    Ok(Some(SoundEffect { name, category, position: Some((x, y, z)), entity_id: None, volume, pitch, seed }))
}

/// `PlaySoundFromEntityS2CPacket` lesen. Wie `read_sound`, nur dass statt der Position eine
/// Entity-ID kommt.
pub fn read_sound_entity(payload: &[u8]) -> Result<Option<SoundEffect>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let name = read_sound_event(&mut r)?;
    let category = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let volume = r.read_f32()?;
    let pitch = r.read_f32()?;
    let seed = r.read_i64()?;
    let Some(name) = name else { return Ok(None) };
    let Some(category) = SoundCategory::from_id(category) else { return Ok(None) };
    Ok(Some(SoundEffect { name, category, position: None, entity_id: Some(entity_id), volume, pitch, seed }))
}

/// Clientbound Stop Sound -- direkt hinter `START_CONFIGURATION` (0x74), vor `STORE_COOKIE` (0x76);
/// im 1.21.11-Client (`GameProtocols`) nachgesehen.
pub const CB_PLAY_STOP_SOUND: i32 = 0x75;

/// Stop Sound lesen: Flags-Byte, bei Bit 0 die Kategorie (VarInt-Enum), bei Bit 1 der
/// Klangname. Beide leer = alles anhalten (Vanilla `SoundManager.stop(null, null)`).
///
/// Rueckgabe: (Kategorie-Nummer, Klangname), jeweils nur wenn gesetzt.
pub fn read_stop_sound(payload: &[u8]) -> Result<(Option<u8>, Option<String>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let flags = r.read_u8()?;
    let category = if flags & 1 != 0 { Some(r.read_varint()?.clamp(0, 255) as u8) } else { None };
    let name = if flags & 2 != 0 { Some(r.read_string()?) } else { None };
    Ok((category, name))
}

#[cfg(test)]
mod sound_tests {
    use super::*;
    use crate::sound_events;

    // Die beiden IDs an ihren verifizierten Nachbarn festnageln, wie bei den Stapel-IDs:
    // eine Verschiebung faellt damit im Test auf und nicht erst im Spiel. Und im Spiel waere
    // sie besonders unangenehm zu finden -- ein falsch getroffenes Nachbarpaket laesst sich
    // oft klaglos lesen (s. chunk_stapel_ids-Test).
    #[test]
    fn die_ton_ids_liegen_zwischen_ihren_verifizierten_nachbarn() {
        assert_eq!(CB_PLAY_SOUND_ENTITY, CB_PLAY_SET_TITLES_ANIMATION + 1);
        assert_eq!(CB_PLAY_SOUND, CB_PLAY_SOUND_ENTITY + 1);
        assert_eq!(CB_PLAY_START_CONFIGURATION, CB_PLAY_SOUND + 1);
        assert_eq!(CB_PLAY_STOP_SOUND, CB_PLAY_START_CONFIGURATION + 1);
        assert_eq!(CB_PLAY_STORE_COOKIE, CB_PLAY_STOP_SOUND + 1);
    }

    // Die Tabelle ist erzeugt (s. scripts/gen-sound-events.sh) -- hier nur die Anker, an
    // denen eine versehentlich neu erzeugte, verschobene Tabelle sofort auffiele.
    #[test]
    fn die_registry_faengt_und_endet_dort_wo_sie_soll() {
        assert_eq!(sound_event_name(0), Some("entity.allay.ambient_with_item"));
        assert_eq!(sound_events::SOUND_EVENTS.len(), 1838);
        assert_eq!(sound_event_name(1838), None, "hinter dem Ende gibt es nichts");
    }

    // ECHTER BUG GEWESEN: zwei Bloecke registriert Vanilla per SCHLEIFE statt mit einem
    // Namensliteral (Ziegenhoerner ab 808, Wolfsvarianten ab 1729). Die erste Fassung des
    // Generators hat sie uebersehen -- und weil eine fehlende Registrierung ALLES DAHINTER
    // verschiebt, spielten im Schwertkampf Wasser- und Zombie-Geraeusche: die Namen kamen aus
    // der Nachbarschaft des richtigen.
    //
    // Diese Anker sind der Test dagegen. Sie liegen bewusst HINTER beiden Einfuegestellen --
    // ein Anker davor waere auch mit der kaputten Tabelle gruen gewesen.
    #[test]
    fn die_per_schleife_registrierten_bloecke_stehen_an_ihrer_stelle() {
        assert_eq!(sound_event_name(808), Some("item.goat_horn.sound.0"), "Ziegenhoerner");
        assert_eq!(sound_event_name(815), Some("item.goat_horn.sound.7"), "acht Stueck");
        // CLASSIC hat als einzige Wolfsvariante ein LEERES Suffix.
        assert_eq!(sound_event_name(1729), Some("entity.wolf.ambient"), "Wolfsvarianten");
        assert_eq!(sound_event_name(1770), Some("entity.wolf_cute.whine"), "7 Varianten x 6 Klaenge");
    }

    // Die Klaenge, um die es im Spiel tatsaechlich geht -- beide liegen HINTER der ersten
    // Einfuegestelle und waren deshalb vom Verschiebungsfehler betroffen.
    #[test]
    fn die_kampfklaenge_stehen_an_ihrer_stelle() {
        assert_eq!(sound_event_name(1244), Some("entity.player.attack.strong"));
        assert_eq!(sound_event_name(1251), Some("entity.player.hurt"));
    }

    #[test]
    fn stop_sound_liest_beide_filter_nur_wenn_ihr_bit_gesetzt_ist() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_STOP_SOUND).write_u8(3).write_varint(2).write_string("minecraft:music_disc.cat");
        assert_eq!(read_stop_sound(&w.into_bytes()).unwrap(), (Some(2), Some("minecraft:music_disc.cat".into())));
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_STOP_SOUND).write_u8(2).write_string("ambient.cave");
        assert_eq!(read_stop_sound(&w.into_bytes()).unwrap(), (None, Some("ambient.cave".into())));
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_STOP_SOUND).write_u8(0);
        assert_eq!(read_stop_sound(&w.into_bytes()).unwrap(), (None, None));
    }

    fn sound_payload(sound_id: i32, category: i32, x: f64, y: f64, z: f64, volume: f32, pitch: f32) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SOUND)
            .write_varint(sound_id)
            .write_varint(category)
            .write_i32((x * 8.0) as i32)
            .write_i32((y * 8.0) as i32)
            .write_i32((z * 8.0) as i32)
            .write_f32(volume)
            .write_f32(pitch)
            .write_i64(1234);
        w.into_bytes()
    }

    #[test]
    fn ein_vanilla_klang_wird_ueber_die_registry_aufgeloest() {
        // +1, weil 0 fuer "eigener Klang" reserviert ist.
        let index = sound_events::SOUND_EVENTS.iter().position(|n| *n == "entity.generic.explode").unwrap();
        let bytes = sound_payload(index as i32 + 1, 4, 10.5, 64.0, -3.25, 1.0, 1.0);
        let sound = read_sound(&bytes).unwrap().unwrap();
        assert_eq!(sound.name, "minecraft:entity.generic.explode");
        assert_eq!(sound.category, SoundCategory::Blocks);
        assert_eq!(sound.position, Some((10.5, 64.0, -3.25)));
        assert_eq!(sound.seed, 1234);
    }

    // Die Verschiebung um eins ist der Fehler, den man hier am leichtesten macht -- und er
    // waere im Spiel fast unsichtbar: es kaeme ein Ton, nur der falsche.
    #[test]
    fn die_zahl_im_paket_ist_um_eins_verschoben() {
        let bytes = sound_payload(1, 0, 0.0, 0.0, 0.0, 1.0, 1.0);
        assert_eq!(read_sound(&bytes).unwrap().unwrap().name, "minecraft:entity.allay.ambient_with_item");
    }

    // Ein Achtel Block ist die Aufloesung des Pakets -- feiner geht es nicht, und das ist
    // Absicht (s. Kommentar in read_sound).
    #[test]
    fn die_position_kommt_in_achtelbloecken() {
        let bytes = sound_payload(1, 0, 1.0 / 8.0, 0.0, 0.0, 1.0, 1.0);
        assert_eq!(read_sound(&bytes).unwrap().unwrap().position, Some((0.125, 0.0, 0.0)));
    }

    // Server duerfen eigene Klaenge schicken, die in keiner Vanilla-Registry stehen.
    #[test]
    fn ein_eigener_klang_des_servers_kommt_als_name() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SOUND)
            .write_varint(0) // 0 = Beschreibung folgt direkt
            .write_string("meinserver:kiste_auf")
            .write_bool(false) // keine feste Reichweite
            .write_varint(7)
            .write_i32(0)
            .write_i32(0)
            .write_i32(0)
            .write_f32(1.0)
            .write_f32(1.0)
            .write_i64(0);
        let sound = read_sound(&w.into_bytes()).unwrap().unwrap();
        assert_eq!(sound.name, "meinserver:kiste_auf");
        assert_eq!(sound.category, SoundCategory::Players);
    }

    // Mit fester Reichweite -- das optionale Feld MUSS mitgelesen werden, sonst verrutscht
    // alles dahinter (Kategorie, Position, Lautstaerke) um vier Byte.
    #[test]
    fn die_optionale_reichweite_verschiebt_den_rest_nicht() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SOUND)
            .write_varint(0)
            .write_string("meinserver:fern")
            .write_bool(true)
            .write_f32(48.0)
            .write_varint(4)
            .write_i32(8 * 8)
            .write_i32(0)
            .write_i32(0)
            .write_f32(0.5)
            .write_f32(2.0)
            .write_i64(0);
        let sound = read_sound(&w.into_bytes()).unwrap().unwrap();
        assert_eq!(sound.category, SoundCategory::Blocks);
        assert_eq!(sound.position, Some((8.0, 0.0, 0.0)));
        assert_eq!(sound.volume, 0.5);
        assert_eq!(sound.pitch, 2.0);
    }

    // Ein Klang aus einer neueren Version darf die Verbindung nicht stoeren -- lieber still
    // bleiben als den Lesezeiger verlieren.
    #[test]
    fn ein_unbekannter_klang_ist_kein_lesefehler() {
        let bytes = sound_payload(999_999, 0, 0.0, 0.0, 0.0, 1.0, 1.0);
        assert_eq!(read_sound(&bytes).unwrap(), None);
    }

    #[test]
    fn ein_klang_an_einer_entity_traegt_die_entity_id_statt_der_position() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SOUND_ENTITY).write_varint(1).write_varint(7).write_varint(4242).write_f32(1.0).write_f32(1.0).write_i64(9);
        let sound = read_sound_entity(&w.into_bytes()).unwrap().unwrap();
        assert_eq!(sound.entity_id, Some(4242));
        assert_eq!(sound.position, None);
    }

    // Die Reihenfolge der Kategorien ist net.minecraft.sound.SoundCategory, am Bytecode
    // abgelesen. Music und Records sind die beiden, die spaeter NICHT raeumlich gespielt
    // werden duerfen -- ein Vertauschen faellt sonst erst als seltsam wanderndes Lied auf.
    #[test]
    fn die_kategorien_stehen_in_vanillas_reihenfolge() {
        assert_eq!(SoundCategory::from_id(0), Some(SoundCategory::Master));
        assert_eq!(SoundCategory::from_id(1), Some(SoundCategory::Music));
        assert_eq!(SoundCategory::from_id(2), Some(SoundCategory::Records));
        assert_eq!(SoundCategory::from_id(9), Some(SoundCategory::Voice));
        assert_eq!(SoundCategory::from_id(10), Some(SoundCategory::Ui));
        assert_eq!(SoundCategory::from_id(11), None);
    }
}

/// Sieht dieses Paket wie ein Ton-Paket aus -- ganz gleich, unter welcher ID es kam?
///
/// Gebaut fuer den Fall "es kommt kein einziger Ton an": von aussen ist nicht zu
/// unterscheiden, ob der Server nichts schickt oder ob wir unter der falschen ID suchen. Statt
/// die Paket-IDs erneut abzuzaehlen, wird hier jedes eintreffende Paket versuchsweise als
/// Ton-Paket gelesen -- dieselbe Methode, mit der der Client schon die Spielerposition
/// lokalisiert hat (s. `CORAL_PACKET_SCAN` in `net::handle_play_packet`).
///
/// Der Test ist bewusst STRENG, sonst meldet er falsche Treffer: das Paket muss restlos
/// aufgehen (kein Byte bleibt uebrig), der Klang muss ein bekannter Registry-Eintrag sein, und
/// Lautstaerke und Tonhoehe muessen in den Bereichen liegen, die Vanilla tatsaechlich schickt.
/// Ein zufaellig passendes Paket ueberlebt diese vier Huerden praktisch nie.
#[must_use] 
pub fn looks_like_sound(payload: &[u8]) -> Option<(bool, String)> {
    for entity_gebunden in [false, true] {
        let mut r = PacketReader::new(payload);
        if r.read_varint().is_err() {
            continue;
        }
        let Ok(Some(name)) = read_sound_event(&mut r) else { continue };
        // Nur Vanilla-Namen zaehlen als Treffer: ein Servername waere zwar moeglich, taugt
        // aber nicht als Beweis -- er entstuende auch aus zufaelligen Bytes.
        if !name.starts_with("minecraft:") {
            continue;
        }
        let Ok(category) = r.read_varint() else { continue };
        if SoundCategory::from_id(category).is_none() {
            continue;
        }
        let gelesen = if entity_gebunden {
            r.read_varint().map(|_| ())
        } else {
            r.read_i32().and_then(|_| r.read_i32()).and_then(|_| r.read_i32()).map(|_| ())
        };
        if gelesen.is_err() {
            continue;
        }
        let (Ok(volume), Ok(pitch), Ok(_seed)) = (r.read_f32(), r.read_f32(), r.read_i64()) else { continue };
        if !r.is_exhausted() || !(0.0..=10.0).contains(&volume) || !(0.0..=3.0).contains(&pitch) {
            continue;
        }
        return Some((entity_gebunden, name));
    }
    None
}

#[cfg(test)]
mod sound_probe_tests {
    use super::*;

    #[test]
    fn ein_echtes_ton_paket_wird_erkannt() {
        let mut w = PacketWriter::new();
        w.write_varint(0x73)
            .write_varint(2)
            .write_varint(4)
            .write_i32(80)
            .write_i32(512)
            .write_i32(-24)
            .write_f32(1.0)
            .write_f32(1.0)
            .write_i64(77);
        let (entity_gebunden, name) = looks_like_sound(&w.into_bytes()).expect("haette erkannt werden muessen");
        assert!(!entity_gebunden);
        assert_eq!(name, "minecraft:entity.allay.ambient_without_item");
    }

    #[test]
    fn ein_entity_ton_paket_wird_als_solches_erkannt() {
        let mut w = PacketWriter::new();
        w.write_varint(0x72).write_varint(1).write_varint(7).write_varint(4242).write_f32(1.0).write_f32(1.0).write_i64(3);
        let (entity_gebunden, _) = looks_like_sound(&w.into_bytes()).expect("haette erkannt werden muessen");
        assert!(entity_gebunden);
    }

    // Der Test taugt nur etwas, wenn er NICHT auf alles anspringt -- sonst meldet er beim
    // Suchen die halbe Paketliste als Ton.
    #[test]
    fn andere_pakete_werden_nicht_faelschlich_als_ton_erkannt() {
        // Ein Chunk-Stapel-Ende, ein Systemchat und ein Klang mit unsinniger Tonhoehe.
        assert_eq!(
            looks_like_sound(&{
                let mut w = PacketWriter::new();
                w.write_varint(CB_PLAY_CHANGE_DIFFICULTY).write_u8(2).write_u8(0);
                w.into_bytes()
            }),
            None
        );
        assert_eq!(looks_like_sound(b""), None);
        let mut w = PacketWriter::new();
        w.write_varint(0x73)
            .write_varint(2)
            .write_varint(4)
            .write_i32(0)
            .write_i32(0)
            .write_i32(0)
            .write_f32(1.0)
            .write_f32(99.0) // unmoegliche Tonhoehe
            .write_i64(0);
        assert_eq!(looks_like_sound(&w.into_bytes()), None);
    }

    // Ueberzaehlige Bytes = es ist ein anderes Paket, das zufaellig weit genug passt.
    #[test]
    fn ein_paket_mit_resten_zaehlt_nicht_als_treffer() {
        let mut w = PacketWriter::new();
        w.write_varint(0x73)
            .write_varint(2)
            .write_varint(4)
            .write_i32(0)
            .write_i32(0)
            .write_i32(0)
            .write_f32(1.0)
            .write_f32(1.0)
            .write_i64(0)
            .write_u8(0xff);
        assert_eq!(looks_like_sound(&w.into_bytes()), None);
    }
}

// --- Item-Abklingzeit (CooldownUpdateS2CPacket) ---
//
// Traegt eine Cooldown-GRUPPE (Identifier) und die Dauer in Ticks; 0 heisst "Gruppe
// freigeben" (ClientPlayNetworkHandler.onCooldownUpdate: remove statt set). Die Gruppe ist
// die Item-ID oder die cooldown_group der use_cooldown-Component (ItemCooldownManager.getGroup).
//
// ID: in PlayStateFactories direkt nach COOKIE_REQUEST (0x15) und vor DAMAGE_EVENT (0x19,
// davor noch CUSTOM_CHAT_COMPLETIONS und CUSTOM_PAYLOAD).
pub const CB_PLAY_COOLDOWN: i32 = 0x16;

// Plugin-Nachrichten (CustomPayloadS2C/C2S) im Play-State -- am Jar geprueft
// (PlayStateFactories: S2C nach BUNDLE auf Platz 0x18, C2S auf 0x15). Kanal als Identifier-
// String, danach der Rest des Pakets als rohe Bytes.
pub const CB_PLAY_CUSTOM_PAYLOAD: i32 = 0x18;
pub const SB_PLAY_CUSTOM_PAYLOAD: i32 = 0x15;
