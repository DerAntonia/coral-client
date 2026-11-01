// Entities: Positionen, Bewegung, Gesundheit, Geschwindigkeit, Weltzeit, Weltgrenze und Effekte.

use super::*;

// Liest die Keep-Alive-ID (i64) aus einem Keep-Alive-Payload (nach der Packet-ID).
pub fn read_keep_alive_id(payload: &[u8]) -> Result<i64, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_i64()
}

// Liest die Ping-ID (i32) aus einem Config-Ping-Payload.
pub fn read_ping_id(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_i32()
}

// Liest die Teleport-ID (erste VarInt nach der Packet-ID) aus Synchronize Player Position.
pub fn read_teleport_id(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

// Bits im Flags-Feld von Synchronize Player Position (aus PositionFlag.java nachgesehen,
// s. read_player_position-Kommentar): X/Y/Z GESETZT bedeutet die jeweilige Koordinate ist
// RELATIV (zur aktuellen Position addieren), NICHT gesetzt bedeutet ABSOLUT (Paket-Wert
// direkt uebernehmen) -- s. EntityPosition.apply() im echten Client.
pub const POSITION_FLAG_REL_X: i32 = 1 << 0;
pub const POSITION_FLAG_REL_Y: i32 = 1 << 1;
pub const POSITION_FLAG_REL_Z: i32 = 1 << 2;
pub const POSITION_FLAG_REL_YAW: i32 = 1 << 3;
pub const POSITION_FLAG_REL_PITCH: i32 = 1 << 4;
// Seit 1.21.2 traegt der Teleport eine GESCHWINDIGKEIT, und diese vier Bits sagen, was damit
// geschieht (Relative.java: DELTA_X 5, DELTA_Y 6, DELTA_Z 7, ROTATE_DELTA 8).
//
// GESETZT heisst wie oben "relativ": die eigene Geschwindigkeit BLEIBT und der Paketwert
// kommt dazu. NICHT gesetzt heisst "absolut": der Paketwert ERSETZT sie.
//
// Genau daran haengt die Enderperle. ThrownEnderpearl teleportiert einen Spieler mit
// Geschwindigkeit 0, Rotation relativ und allen DELTA-Bits, also Geschwindigkeit 0 mit allen vier DELTA-Bits GESETZT -- 0 dazuaddiert laesst die
// eigene Geschwindigkeit unveraendert. Man behaelt seinen Schwung ueber die Perle hinweg.
pub const POSITION_FLAG_REL_DELTA_X: i32 = 1 << 5;
pub const POSITION_FLAG_REL_DELTA_Y: i32 = 1 << 6;
pub const POSITION_FLAG_REL_DELTA_Z: i32 = 1 << 7;
pub const POSITION_FLAG_ROTATE_DELTA: i32 = 1 << 8;

// Liest die Spielerposition aus Synchronize Player Position. Aufbau seit 1.21.2:
// Teleport-ID (VarInt), X/Y/Z (Double), Velocity X/Y/Z (Double), Yaw/Pitch (Float),
// Flags (Int, s. POSITION_FLAG_*).
//
// DRITTER ECHTER BUG gewesen (Perlen flaggten auf mcpvp.club):
// die drei Geschwindigkeitsfelder wurden gelesen und WEGGEWORFEN, und der Client nullte bei
// jedem Teleport seine Geschwindigkeit. Vanilla setzt sie stattdessen IMMER aus dem Paket
// (ClientPacketListener.setValuesFromPositionPacket -> setDeltaMovement), und bei der
// Enderperle heisst das: Schwung behalten (s. POSITION_FLAG_REL_DELTA_*). Wer stattdessen
// nullt, steht nach jeder Perle schlagartig still, waehrend der Server weiterfliegt -- ein
// Treffer fuer jede Bewegungspruefung, und zwar bei jeder einzelnen Perle.
//
// ECHTER BUG gewesen (per pktproxy-Mitschnitt + Grim-BadPacketsN gefunden,
// Tooltip "ignored or failed to accept a required server teleport"): fruehere Fassung hat
// die Flags komplett ignoriert und X/Y/Z IMMER absolut uebernommen. Bei einem relativen
// Teleport (z.B. manche Server-/Anticheat-Korrekturen) landete unser Client dadurch an der
// falschen Position -- der Server/Grim sah das als nicht (korrekt) akzeptierten Teleport.
//
// ZWEITER ECHTER BUG gewesen (wer Maus und Tastatur gleichzeitig bewegte, wurde sofort
// an den Spawnpunkt zurueckteleportiert): Yaw/Pitch aus
// diesem Paket wurden bisher komplett verworfen -- unsere Kamera behielt einfach ihren
// alten Wert. Grims Teleport-Warteschlange verlangt aber, dass das naechste Bewegungspaket
// (sobald es ueberhaupt eine Blickrichtung meldet) zur vom Server erwarteten Rotation passt.
// Reine Tastatureingabe sendet nie ein Rotationspaket, das mismatchen koennte -- erst sobald
// die Maus dazukommt, faellt die nie uebernommene Server-Rotation auf.
pub fn read_player_position(payload: &[u8]) -> Result<PlayerPositionSync, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let _teleport_id = r.read_varint()?;
    let x = r.read_f64()?;
    let y = r.read_f64()?;
    let z = r.read_f64()?;
    let vel_x = r.read_f64()?;
    let vel_y = r.read_f64()?;
    let vel_z = r.read_f64()?;
    let yaw = r.read_f32()?;
    let pitch = r.read_f32()?;
    let flags = r.read_i32()?;
    Ok(PlayerPositionSync { pos: (x, y, z), vel: (vel_x, vel_y, vel_z), yaw, pitch, flags })
}

// Der Inhalt von Synchronize Player Position. Eigener Typ statt eines Sechser-Tupels, seit
// die Geschwindigkeit dazugekommen ist -- ein Neunertupel verwechselt man zu leicht.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PlayerPositionSync {
    pub pos: (f64, f64, f64),
    pub vel: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
    pub flags: i32,
}

// Liest die Chunk-Koordinaten aus Forget Level Chunk. Payload nach der Packet-ID: ein Long
// (ChunkPos.toLong), x = untere 32 Bit, z = obere 32 Bit -- beide als signed i32
// interpretiert (s. CB_PLAY_FORGET_LEVEL_CHUNK).
pub fn read_forget_level_chunk(payload: &[u8]) -> Result<(i32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let packed = r.read_i64()?;
    let x = packed as i32; // untere 32 Bit
    let z = (packed >> 32) as i32; // obere 32 Bit
    Ok((x, z))
}

// Packt/entpackt die klassische Minecraft-"Position" (Block Update, Player-Aktionen, ...):
// ein Long, X in den oberen 26 Bit, Z in den mittleren 26 Bit, Y in den unteren 12 Bit (s.
// BlockPos.asLong/PACKED_X_MASK/X_OFFSET=38/Z_OFFSET=12 im 1.21.11-Client). Bewusst als
// eigene Funktion statt inline, weil dieselbe Kodierung an mehreren Stellen vorkommt
// (Block Update jetzt, potenziell weitere Block-Aktionspakete spaeter).
#[must_use] 
pub const fn decode_block_pos(packed: i64) -> (i32, i32, i32) {
    let x = (packed >> 38) as i32;
    let y = ((packed << 52) >> 52) as i32;
    let z = ((packed << 26) >> 38) as i32;
    (x, y, z)
}

#[must_use] 
pub fn encode_block_pos(x: i32, y: i32, z: i32) -> i64 {
    ((i64::from(x) & 0x3FF_FFFF) << 38) | ((i64::from(z) & 0x3FF_FFFF) << 12) | (i64::from(y) & 0xFFF)
}

// Block Update (0x08): Position (Long, s. decode_block_pos) + neuer Block-State (VarInt).
pub fn read_block_update(payload: &[u8]) -> Result<(i32, i32, i32, u32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let packed = r.read_i64()?;
    let (x, y, z) = decode_block_pos(packed);
    let state = r.read_varint()?;
    Ok((x, y, z, state as u32))
}

// Section Blocks Update (0x52): Chunk-Section-Position (Long: X 22 Bit @ Shift 42, Y 20 Bit
// @ Shift 0, Z 22 Bit @ Shift 20 -- s. SectionPos.asLong im 1.21.11-Client, ANDERE Aufteilung
// als die normale Block-Position oben), dann VarInt-Anzahl, dann je Eintrag ein VarLong:
// (BlockState << 12) | (lokale Position, 4 Bit je Achse als X<<8|Z<<4|Y gepackt). Fuer
// gebuendelte Blockaenderungen (Kolben, Redstone, WorldEdit-artige Batches).
pub fn read_section_blocks_update(payload: &[u8]) -> Result<Vec<(i32, i32, i32, u32)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let packed = r.read_i64()?;
    let sx = (packed >> 42) as i32;
    let sy = ((packed << 44) >> 44) as i32;
    let sz = ((packed << 22) >> 42) as i32;

    let count = r.read_varint()?;
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count {
        let entry = r.read_varlong()?;
        let state = (entry >> 12) as u32;
        let rel = entry & 0xFFF;
        let local_x = (rel >> 8) & 0xF;
        let local_z = (rel >> 4) & 0xF;
        let local_y = rel & 0xF;
        out.push((sx * 16 + local_x as i32, sy * 16 + local_y as i32, sz * 16 + local_z as i32, state));
    }
    Ok(out)
}

// Liest nur die Packet-ID aus einem rohen (bereits dekomprimierten) Payload, ohne den
// Rest zu parsen -- zum Dispatchen anhand der ID.
pub fn peek_packet_id(payload: &[u8]) -> Result<i32, ReadError> {
    PacketReader::new(payload).read_varint()
}

// ---- Entity-Pakete ----------------------------------------------------------------

// Winkel kommen in den Move/Spawn-Paketen als Byte: 256 Schritte = 360 Grad
// (Entity#getYaw wird beim Schreiben mit 256/360 multipliziert).
pub(crate) fn angle_byte_to_deg(b: u8) -> f32 {
    f32::from(b as i8) * (360.0 / 256.0)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AddEntity {
    pub entity_id: i32,
    pub uuid: [u8; 16],
    // Index in der ENTITY_TYPE-Registry (versionsabhaengig; wir reichen ihn nur durch).
    pub entity_type: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    // KOERPERdrehung.
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    // Kopfdrehung, unabhaengig vom Koerper (s. CB_PLAY_ROTATE_HEAD).
    pub head_yaw_deg: f32,
    // Anfangsgeschwindigkeit. Fuer Projektile ist das der WICHTIGSTE Wert im Paket: der
    // Client fliegt sie damit selbst weiter (s. world::entity::ProjectileKind), der Server
    // schickt danach nur noch grobe Korrekturen.
    pub vel: (f64, f64, f64),
    // Das Data-Feld. Was drinsteht, haengt am Typ; fuer Projektile ist es die Entity-ID des
    // SCHUETZEN (Projectile.getAddEntityPacket: `entity == null ? 0 : entity.getId()`), 0
    // wenn es keinen gibt.
    pub data: i32,
}

// Add Entity (0x01): EntityId (VarInt), UUID (16 Bytes), Typ (VarInt, Registry-Index),
// X/Y/Z (Double), Velocity (Spezial-Encoding s.u.), Pitch/Yaw/HeadYaw (je 1 Byte Winkel),
// Data (VarInt). Feldreihenfolge nach EntitySpawnS2CPacket (nachgesehen, yarn).
pub fn read_add_entity(payload: &[u8]) -> Result<AddEntity, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let uuid: [u8; 16] = r.read_bytes(16)?.try_into().map_err(|_| ReadError)?;
    let entity_type = r.read_varint()?;
    let x = r.read_f64()?;
    let y = r.read_f64()?;
    let z = r.read_f64()?;
    let vel = read_velocity(&mut r)?;
    let pitch_deg = angle_byte_to_deg(r.read_u8()?);
    let yaw_deg = angle_byte_to_deg(r.read_u8()?);
    // Die Kopfdrehung ist bei Lebewesen NICHT dasselbe wie yaw_deg: yaw ist die
    // KOERPERdrehung, der Kopf dreht sich unabhaengig davon (s. CB_PLAY_ROTATE_HEAD).
    let head_yaw_deg = angle_byte_to_deg(r.read_u8()?);
    let data = r.read_varint()?;
    Ok(AddEntity { entity_id, uuid, entity_type, x, y, z, yaw_deg, pitch_deg, head_yaw_deg, vel, data })
}

// Entity Position Sync (0x23): EntityId (VarInt), Pos X/Y/Z (Double), Delta-Movement
// X/Y/Z (Double), Yaw/Pitch (Float, Grad), OnGround (Bool) -- EntityPosition.PACKET_CODEC.
pub fn read_entity_position_sync(payload: &[u8]) -> Result<(i32, f64, f64, f64, f32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let x = r.read_f64()?;
    let y = r.read_f64()?;
    let z = r.read_f64()?;
    let _vel = (r.read_f64()?, r.read_f64()?, r.read_f64()?);
    let yaw_deg = r.read_f32()?;
    let pitch_deg = r.read_f32()?;
    Ok((entity_id, x, y, z, yaw_deg, pitch_deg))
}

// Teleport Entity (0x7B): EntityId (VarInt), EntityPosition (wie 0x23), Relativ-Flags
// (Int-Bitfeld, PositionFlag.PACKET_CODEC), OnGround (Bool). Der lokale Paper-Server
// schickt fuer Spieler absolute Teleports (Flags 0); Relativ-Bits werten wir vorerst
// nicht aus und behandeln die Position als absolut.
pub fn read_teleport_entity(payload: &[u8]) -> Result<(i32, f64, f64, f64, f32, f32), ReadError> {
    // identisches Layout bis einschliesslich Pitch -- danach kommen nur Felder, die wir
    // ohnehin ignorieren, also denselben Reader verwenden.
    read_entity_position_sync(payload)
}

// Move Entity Pos (0x33): EntityId (VarInt), Delta X/Y/Z (je Short, Bloecke * 4096),
// OnGround (Bool). Delta bezieht sich auf die letzte bekannte Position.
pub fn read_move_entity_pos(payload: &[u8]) -> Result<(i32, f64, f64, f64), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let dx = f64::from(r.read_i16()?) / 4096.0;
    let dy = f64::from(r.read_i16()?) / 4096.0;
    let dz = f64::from(r.read_i16()?) / 4096.0;
    Ok((entity_id, dx, dy, dz))
}

// Move Entity Pos+Rot (0x34): wie 0x33, plus Yaw/Pitch (je 1 Byte Winkel) vor OnGround.
pub fn read_move_entity_pos_rot(payload: &[u8]) -> Result<(i32, f64, f64, f64, f32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let dx = f64::from(r.read_i16()?) / 4096.0;
    let dy = f64::from(r.read_i16()?) / 4096.0;
    let dz = f64::from(r.read_i16()?) / 4096.0;
    let yaw_deg = angle_byte_to_deg(r.read_u8()?);
    let pitch_deg = angle_byte_to_deg(r.read_u8()?);
    Ok((entity_id, dx, dy, dz, yaw_deg, pitch_deg))
}

// Move Entity Rot (0x36): EntityId (VarInt), Yaw/Pitch (je 1 Byte Winkel), OnGround (Bool).
pub fn read_move_entity_rot(payload: &[u8]) -> Result<(i32, f32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let yaw_deg = angle_byte_to_deg(r.read_u8()?);
    let pitch_deg = angle_byte_to_deg(r.read_u8()?);
    Ok((entity_id, yaw_deg, pitch_deg))
}

// Set Health (0x66): Leben (Float), Nahrung (VarInt), Saettigung (Float). Das Leben erkennt
// den Tod, die NAHRUNG entscheidet ueber den Sprint: PlayerEntity.canSprintOrFly() haengt an
// HungerManager.canSprint(), und das ist `getFoodLevel() > 6`. Die Saettigung zeigt nur das
// Saettigungs-Modul.
pub fn read_set_health(payload: &[u8]) -> Result<(f32, i32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let health = r.read_f32()?;
    let food = r.read_varint()?;
    let saturation = r.read_f32()?;
    Ok((health, food, saturation))
}

// --- Rueckstoss ------------------------------------------------------------------------
//
// Zwei Wege, auf denen der Server unsere Geschwindigkeit setzt, und beide fehlten dem Client
// komplett -- man wurde also lokal NIE zurueckgestossen, waehrend der Server (und GrimAC) es
// laengst eingerechnet hatten:
//
//   Set Entity Motion (0x63)  ERSETZT die Geschwindigkeit (Entity.setVelocityClient)
//   Explode (0x24)            ADDIERT sie (ClientPlayNetworkHandler.onExplosion ->
//                             playerKnockback().ifPresent(player::addVelocityInternal))
//
// IDs aus derselben PlayStateFactories-Tabelle wie CB_PLAY_BLOCK_UPDATE, im selben Durchlauf
// gegen alle bereits verifizierten IDs geprueft (ADD_ENTITY 0x01, BLOCK_UPDATE 0x08,
// ENTITY_POSITION_SYNC 0x23, FORGET_LEVEL_CHUNK 0x25, KEEP_ALIVE 0x2B, CHUNK_DATA 0x2C,
// PLAYER_POSITION 0x46, SET_HEALTH 0x66 -- alle acht stimmen).
pub const CB_PLAY_SET_ENTITY_MOTION: i32 = 0x63;
pub const CB_PLAY_EXPLODE: i32 = 0x24;

// Geschwindigkeit auf der Leitung, 1.21.11-Fassung (net.minecraft.network.encoding.
// VelocityEncoding). NICHT mehr die alten drei Shorts mit Faktor 8000, sondern drei auf
// 15 Bit normierte Richtungsanteile in einem 48-Bit-Wort plus ein ganzzahliger Betrag:
//
//   Byte 0 (i)    : Bits 0..1 = Betrag (klein), Bit 2 = "Betrag folgt als VarInt"
//   Byte 1 (j)    : naechste 8 Bits des Worts
//   Bytes 2..5 (l): oberste 32 Bits, big-endian
//   m = l<<16 | j<<8 | i, danach x = f(m>>3), y = f(m>>18), z = f(m>>33), jeweils * Betrag
//   f(v) = min(v & 32767, 32766) * 2/32766 - 1     (also -1..+1)
//
// i == 0 heisst "keine Bewegung" und beendet das Paket sofort.
pub fn read_velocity(r: &mut PacketReader) -> Result<(f64, f64, f64), ReadError> {
    let i = u64::from(r.read_u8()?);
    if i == 0 {
        return Ok((0.0, 0.0, 0.0));
    }
    let j = u64::from(r.read_u8()?);
    let l = u64::from(r.read_i32()? as u32);
    let m = (l << 16) | (j << 8) | i;
    let mut betrag = i & 3;
    // Bit 2 gesetzt: der Betrag passt nicht in zwei Bits und steht als VarInt dahinter.
    if i & 4 == 4 {
        betrag |= u64::from(r.read_varint()? as u32) << 2;
    }
    let anteil = |v: u64| ((v & 32767).min(32766) as f64) * 2.0 / 32766.0 - 1.0;
    let betrag = betrag as f64;
    Ok((anteil(m >> 3) * betrag, anteil(m >> 18) * betrag, anteil(m >> 33) * betrag))
}

// Set Entity Motion: VarInt Entity-Id, dann die Geschwindigkeit.
pub fn read_entity_velocity(payload: &[u8]) -> Result<(i32, (f64, f64, f64)), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let vel = read_velocity(&mut r)?;
    Ok((entity_id, vel))
}

// Explode: Mittelpunkt (3x f64), Radius (f32), Blockanzahl (i32), dann OPTIONAL der
// Rueckstoss fuer uns (bool + 3x f64). Danach kommen nur noch Partikel und Ton -- die
// interessieren hier nicht, also wird ab da nicht weitergelesen.
//
// Der Rueckstoss fehlt, wenn die Explosion uns gar nicht erfasst hat; dann ist nichts zu tun.
pub fn read_explosion_knockback(payload: &[u8]) -> Result<Option<(f64, f64, f64)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let _x = r.read_f64()?;
    let _y = r.read_f64()?;
    let _z = r.read_f64()?;
    let _radius = r.read_f32()?;
    let _block_count = r.read_i32()?;
    if !r.read_bool()? {
        return Ok(None);
    }
    Ok(Some((r.read_f64()?, r.read_f64()?, r.read_f64()?)))
}

// --- Weltzeit ---------------------------------------------------------------------------
//
// WorldTimeUpdateS2CPacket: Weltalter, Tageszeit und ob die Tageszeit ueberhaupt laeuft
// (`doDaylightCycle`). Der Server schickt es alle 20 Ticks; dazwischen zaehlt der Client
// selbst hoch.
//
// Die ID 0x6F ist Position 111 der S2C-Registrierungsreihenfolge in PlayStateFactories. Die
// Zaehlung ist diesmal nicht geschaetzt, sondern maschinell aus dem Bytecode gezogen (die
// `NetworkStateBuilder.add`-Aufrufe in Reihenfolge) und gegen ACHTZEHN im Client bereits
// bestaetigte IDs geprueft -- von BUNDLE_DELIMITER 0x00 bis UPDATE_MOB_EFFECT 0x82 trifft
// jede einzelne. Wichtig beim Nachzaehlen: manche Pakete heissen ihr Codec-Feld PACKET_CODEC
// oder PLAY_CODEC statt CODEC; wer nur auf CODEC filtert, ueberspringt sie und zaehlt sich
// danach daneben.
pub const CB_PLAY_SET_TIME: i32 = 0x6F;

/// Weltalter, Tageszeit und ob die Tageszeit laeuft.
///
/// Beide Zeiten sind LONGS, keine `VarInts`. `tick_day_time` ist neu ab 1.21.9 -- vorher zeigte
/// ein NEGATIVES `time_of_day` an, dass die Zeit stillsteht, und der Betrag war die Zeit. Wer
/// den alten Trick nachbaut, bekommt auf einem heutigen Server bei angehaltener Zeit eine
/// Tageszeit von 0 statt der eingefrorenen.
pub fn read_world_time(payload: &[u8]) -> Result<(i64, i64, bool), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let world_age = r.read_i64()?;
    let time_of_day = r.read_i64()?;
    let tick_day_time = r.read_u8()? != 0;
    Ok((world_age, time_of_day, tick_day_time))
}

#[cfg(test)]
mod world_time_tests {
    use super::*;

    #[test]
    fn liest_beide_zeiten_als_longs() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SET_TIME);
        w.write_i64(123_456_789);
        w.write_i64(6000); // Mittag
        w.write_u8(1);
        assert_eq!(read_world_time(&w.into_bytes()).expect("lesbar"), (123_456_789, 6000, true));
    }

    #[test]
    // Angehaltene Zeit ist seit 1.21.9 ein eigenes Flag, KEINE negative Tageszeit mehr.
    fn angehaltene_zeit_behaelt_ihren_wert() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SET_TIME);
        w.write_i64(10);
        w.write_i64(18000); // Mitternacht, eingefroren
        w.write_u8(0);
        assert_eq!(read_world_time(&w.into_bytes()).expect("lesbar"), (10, 18000, false));
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_world_time(&[CB_PLAY_SET_TIME as u8]).is_err());
    }
}

// --- Weltgrenze ------------------------------------------------------------------------
//
// Sechs Pakete; die IDs auf dieselbe Weise aus dem Bytecode gezaehlt wie oben, ihre
// Nachbarn SET_CAMERA 0x5B und SET_CURSOR_ITEM 0x5E stimmen dabei mit den hier bereits
// bestaetigten IDs ueberein. Groessen und Mitte als double, die Dauer einer Groessenaenderung
// als VarLong in TICKS (seit 1.21.11 tickt die Grenze; frueher waren es Millisekunden).
pub const CB_PLAY_INITIALIZE_BORDER: i32 = 0x2A;
pub const CB_PLAY_SET_BORDER_CENTER: i32 = 0x56;
pub const CB_PLAY_SET_BORDER_LERP_SIZE: i32 = 0x57;
pub const CB_PLAY_SET_BORDER_SIZE: i32 = 0x58;
pub const CB_PLAY_SET_BORDER_WARNING_DELAY: i32 = 0x59;
pub const CB_PLAY_SET_BORDER_WARNING_DISTANCE: i32 = 0x5A;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum BorderUpdate {
    // Alles auf einmal (nach Login, Respawn und Weltwechsel). `lerp_ticks` > 0: von
    // `old_size` nach `new_size` wandern, sonst sofort `new_size`.
    Initialize { center: (f64, f64), old_size: f64, new_size: f64, lerp_ticks: i64, absolute_max_size: i32, warning_blocks: i32, warning_time: i32 },
    Center(f64, f64),
    LerpSize { old_size: f64, new_size: f64, lerp_ticks: i64 },
    Size(f64),
    WarningTime(i32),
    WarningBlocks(i32),
}

// None = keins der Grenz-Pakete.
#[must_use] 
pub fn read_border_update(payload: &[u8]) -> Option<Result<BorderUpdate, ReadError>> {
    let mut r = PacketReader::new(payload);
    let id = r.read_varint().ok()?;
    let mut lesen = || -> Result<BorderUpdate, ReadError> {
        Ok(match id {
            CB_PLAY_INITIALIZE_BORDER => BorderUpdate::Initialize {
                center: (r.read_f64()?, r.read_f64()?),
                old_size: r.read_f64()?,
                new_size: r.read_f64()?,
                lerp_ticks: r.read_varlong()?,
                absolute_max_size: r.read_varint()?,
                warning_blocks: r.read_varint()?,
                warning_time: r.read_varint()?,
            },
            CB_PLAY_SET_BORDER_CENTER => BorderUpdate::Center(r.read_f64()?, r.read_f64()?),
            CB_PLAY_SET_BORDER_LERP_SIZE => BorderUpdate::LerpSize { old_size: r.read_f64()?, new_size: r.read_f64()?, lerp_ticks: r.read_varlong()? },
            CB_PLAY_SET_BORDER_SIZE => BorderUpdate::Size(r.read_f64()?),
            CB_PLAY_SET_BORDER_WARNING_DELAY => BorderUpdate::WarningTime(r.read_varint()?),
            _ => BorderUpdate::WarningBlocks(r.read_varint()?),
        })
    };
    match id {
        CB_PLAY_INITIALIZE_BORDER
        | CB_PLAY_SET_BORDER_CENTER
        | CB_PLAY_SET_BORDER_LERP_SIZE
        | CB_PLAY_SET_BORDER_SIZE
        | CB_PLAY_SET_BORDER_WARNING_DELAY
        | CB_PLAY_SET_BORDER_WARNING_DISTANCE => Some(lesen()),
        _ => None,
    }
}

#[cfg(test)]
mod border_tests {
    use super::*;

    #[test]
    fn initialize_liest_alle_felder_in_reihenfolge() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_INITIALIZE_BORDER);
        w.write_f64(100.5).write_f64(-20.0).write_f64(500.0).write_f64(50.0);
        w.write_varint(1200); // VarLong, unter 2^31 gleich kodiert wie ein VarInt
        w.write_varint(29_999_984).write_varint(5).write_varint(15);
        let u = read_border_update(&w.into_bytes()).expect("Grenzpaket").expect("lesbar");
        assert_eq!(
            u,
            BorderUpdate::Initialize { center: (100.5, -20.0), old_size: 500.0, new_size: 50.0, lerp_ticks: 1200, absolute_max_size: 29_999_984, warning_blocks: 5, warning_time: 15 }
        );
    }

    #[test]
    fn einzelne_pakete() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SET_BORDER_LERP_SIZE).write_f64(10.0).write_f64(20.0).write_varint(40);
        assert_eq!(read_border_update(&w.into_bytes()).unwrap().unwrap(), BorderUpdate::LerpSize { old_size: 10.0, new_size: 20.0, lerp_ticks: 40 });
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SET_BORDER_WARNING_DISTANCE).write_varint(7);
        assert_eq!(read_border_update(&w.into_bytes()).unwrap().unwrap(), BorderUpdate::WarningBlocks(7));
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_SET_BORDER_SIZE).write_f64(64.0);
        assert_eq!(read_border_update(&w.into_bytes()).unwrap().unwrap(), BorderUpdate::Size(64.0));
    }

    #[test]
    fn fremde_pakete_und_abgeschnittene() {
        assert!(read_border_update(&[CB_PLAY_SET_CAMERA as u8, 1]).is_none());
        assert!(read_border_update(&[CB_PLAY_SET_BORDER_CENTER as u8, 0, 0]).unwrap().is_err());
    }
}

// --- Statuseffekte ---------------------------------------------------------------------
//
// Gebraucht werden drei: Blindheit sperrt den Sprint (ClientPlayerEntity.canSprint ->
// !hasBlindnessEffect()), Schnelligkeit und Langsamkeit aendern das Bewegungstempo
// (s. physics::effect_speed_factor).
pub const CB_PLAY_UPDATE_MOB_EFFECT: i32 = 0x82;
pub const CB_PLAY_REMOVE_MOB_EFFECT: i32 = 0x4C;

// Die Effekt-ID ist die SCHLICHTE Registry-ID als VarInt, ohne Verschiebung.
//
// ECHTER BUG GEWESEN, und ein lehrreicher: hier stand `MOB_EFFECT_BLINDNESS = 15` mit der
// Begruendung, die Registry-Eintrag-Kodierung schreibe `rawId + 1`. Das gilt aber nur fuer die
// ZWEIARGUMENTIGE Form `PacketCodecs.registryEntry(key, direktCodec)`, bei der die 0 einen
// eingebetteten Eintrag ankuendigt (so bei den Klaengen). `StatusEffect.ENTRY_PACKET_CODEC` ist
// die EINARGUMENTIGE Form, und die schreibt in PacketCodecs$22.encode schlicht
// `VarInts.write(getRawIdOrThrow(entry))` -- am Bytecode nachgelesen. Blindheit hat rawId 14,
// steht also auch als 14 auf der Leitung; die 15 traf Nachtsicht. Blind zu sein hat den Sprint
// deshalb nie gesperrt, dafuer sperrte ihn Nachtsicht.
//
// Die rawIds sind die Reihenfolge der register()-Aufrufe in StatusEffects (maschinell aus dem
// Bytecode gezaehlt, nicht abgeschrieben): 0 speed, 1 slowness, 2 haste, 3 mining_fatigue,
// 4 strength, 5 instant_health, 6 instant_damage, 7 jump_boost, 8 nausea, 9 regeneration,
// 10 resistance, 11 fire_resistance, 12 water_breathing, 13 invisibility, 14 blindness.
pub const MOB_EFFECT_SPEED: i32 = 0;
pub const MOB_EFFECT_SLOWNESS: i32 = 1;
pub const MOB_EFFECT_BLINDNESS: i32 = 14;
pub const MOB_EFFECT_FIRE_RESISTANCE: i32 = 11;
// Faerben die Herzen ein (Gui.HeartType.forPlayer).
pub const MOB_EFFECT_POISON: i32 = 18;
pub const MOB_EFFECT_WITHER: i32 = 19;
// Effekte mit Wirkung auf die eigene Bewegung (s. app physics::Input).
pub const MOB_EFFECT_JUMP_BOOST: i32 = 7;
pub const MOB_EFFECT_LEVITATION: i32 = 24;
pub const MOB_EFFECT_SLOW_FALLING: i32 = 27;
pub const MOB_EFFECT_DOLPHINS_GRACE: i32 = 29;
pub const MOB_EFFECT_WEAVING: i32 = 36;

/// Alle Statuseffekte in REGISTRY-REIHENFOLGE -- der Index IST die Zahl auf der Leitung.
///
/// Aus dem statischen Initialisierer von `StatusEffects` gelesen (maschinell, nicht
/// abgeschrieben). Gebraucht wird die Tabelle nur zum ANZEIGEN im Debug-Overlay; die drei
/// Effekte, auf die der Client reagiert, haben ihre eigenen Konstanten darueber. Genau dafuer
/// ist sie aber wichtig: ein falsch zugeordneter Effekt sieht man sonst nirgends.
#[rustfmt::skip]
pub static MOB_EFFECTS: [&str; 40] = [
    "speed", "slowness", "haste", "mining_fatigue", "strength", "instant_health", "instant_damage", "jump_boost", "nausea",
    "regeneration", "resistance", "fire_resistance", "water_breathing", "invisibility", "blindness", "night_vision", "hunger",
    "weakness", "poison", "wither", "health_boost", "absorption", "saturation", "glowing", "levitation", "luck", "unluck",
    "slow_falling", "conduit_power", "dolphins_grace", "bad_omen", "hero_of_the_village", "darkness", "trial_omen", "raid_omen",
    "wind_charged", "weaving", "oozing", "infested", "breath_of_the_nautilus",
];

/// Name eines Effekts, oder seine Zahl als Text, wenn er neuer ist als diese Tabelle.
#[must_use] 
pub fn mob_effect_name(id: i32) -> String {
    usize::try_from(id).ok().and_then(|i| MOB_EFFECTS.get(i)).map_or_else(|| format!("#{id}"), |name| (*name).to_string())
}

/// Eine Dauer von -1 heisst UNENDLICH (StatusEffectInstance.INFINITE) -- ein Beacon oder ein
/// Kommando kann das schicken. Wer sie als gewoehnliche Zahl herunterzaehlt, laesst den Effekt
/// im naechsten Tick ablaufen.
pub const MOB_EFFECT_INFINITE: i32 = -1;

/// Update Mob Effect: Entity-Id, Effekt, Staerke, Dauer und das Flag-Byte (s. `MOB_EFFECT_FLAG`_*).
/// Fehlt das Byte, gilt 0.
///
/// `amplifier` ist die um eins verringerte Stufe: Schnelligkeit I steht als 0 drin. Vanilla
/// rechnet ueberall mit `amplifier + 1`.
pub fn read_mob_effect(payload: &[u8]) -> Result<(i32, i32, i32, i32, u8), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let effect = r.read_varint()?;
    let amplifier = r.read_varint()?;
    let duration = r.read_varint()?;
    let flags = r.read_u8().unwrap_or(0);
    Ok((entity_id, effect, amplifier, duration, flags))
}

/// Die Bits im Flag-Byte von Update Mob Effect (ClientboundUpdateMobEffectPacket): von einem
/// Leuchtfeuer bzw. Conduit, Partikel sichtbar, Symbol im HUD, Ueberblendung.
pub const MOB_EFFECT_FLAG_AMBIENT: u8 = 0x01;
pub const MOB_EFFECT_FLAG_VISIBLE: u8 = 0x02;
pub const MOB_EFFECT_FLAG_SHOW_ICON: u8 = 0x04;
pub const MOB_EFFECT_FLAG_BLEND: u8 = 0x08;

/// Remove Mob Effect: nur Entity-Id und Effekt.
pub fn read_remove_mob_effect(payload: &[u8]) -> Result<(i32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let effect = r.read_varint()?;
    Ok((entity_id, effect))
}

#[cfg(test)]
mod mob_effect_tests {
    use super::*;

    #[test]
    fn bewegungs_effekte_passen_zur_tabelle() {
        assert_eq!(mob_effect_name(MOB_EFFECT_JUMP_BOOST), "jump_boost");
        assert_eq!(mob_effect_name(MOB_EFFECT_LEVITATION), "levitation");
        assert_eq!(mob_effect_name(MOB_EFFECT_SLOW_FALLING), "slow_falling");
        assert_eq!(mob_effect_name(MOB_EFFECT_DOLPHINS_GRACE), "dolphins_grace");
        assert_eq!(mob_effect_name(MOB_EFFECT_BLINDNESS), "blindness");
    }

    #[test]
    fn liest_staerke_und_dauer_mit() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_UPDATE_MOB_EFFECT);
        w.write_varint(42);
        w.write_varint(MOB_EFFECT_SPEED);
        w.write_varint(1); // Schnelligkeit II
        w.write_varint(600);
        w.write_u8(MOB_EFFECT_FLAG_AMBIENT | MOB_EFFECT_FLAG_SHOW_ICON);
        assert_eq!(read_mob_effect(&w.into_bytes()).expect("lesbar"), (42, MOB_EFFECT_SPEED, 1, 600, 0x05));
    }

    #[test]
    // Das Entfernen-Paket hat KEINE Staerke und keine Dauer. Wer denselben Leser nimmt, laeuft
    // ins Leere -- frueher tat er das und verliess sich darauf, dass der Fehler verschluckt wird.
    fn das_entfernen_paket_endet_nach_der_effekt_id() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_REMOVE_MOB_EFFECT);
        w.write_varint(42);
        w.write_varint(MOB_EFFECT_SLOWNESS);
        let bytes = w.into_bytes();
        assert_eq!(read_remove_mob_effect(&bytes).expect("lesbar"), (42, MOB_EFFECT_SLOWNESS));
        assert!(read_mob_effect(&bytes).is_err(), "als Update gelesen muss es fehlschlagen");
    }

    #[test]
    // Die Namenstabelle und die drei Konstanten muessen dieselbe Reihenfolge meinen -- sonst
    // zeigt das Overlay etwas anderes an, als der Client rechnet, und die Anzeige waere
    // schlimmer als keine.
    fn die_namenstabelle_deckt_sich_mit_den_konstanten() {
        assert_eq!(mob_effect_name(MOB_EFFECT_SPEED), "speed");
        assert_eq!(mob_effect_name(MOB_EFFECT_SLOWNESS), "slowness");
        assert_eq!(mob_effect_name(MOB_EFFECT_BLINDNESS), "blindness");
        assert_eq!(mob_effect_name(15), "night_vision", "die alte, falsche Blindheits-ID");
        assert_eq!(mob_effect_name(999), "#999", "unbekannte Effekte duerfen nicht panisch werden");
        assert_eq!(mob_effect_name(-1), "#-1");
    }

    #[test]
    // Die drei IDs sind Registry-Positionen. Eine Verschiebung um eins ist der wahrscheinlichste
    // Fehler und faellt im Spiel nur als "wirkt nicht" auf.
    fn die_registry_ids_stehen_fest() {
        assert_eq!(MOB_EFFECT_SPEED, 0);
        assert_eq!(MOB_EFFECT_SLOWNESS, 1);
        assert_eq!(MOB_EFFECT_BLINDNESS, 14, "14 = blindness, 15 waere night_vision");
    }
}

// --- Ausruestung anderer Entities -----------------------------------------------------
//
// Set Equipment (0x64): was ein Entity in den Haenden haelt bzw. traegt. Ohne dieses Paket
// steht jeder fremde Spieler mit leeren Haenden da -- der Client erfaehrt sonst NIRGENDS,
// was ein anderer haelt (das eigene Inventar sagt nur ueber einen selbst etwas).
//
// ID nach dem Registrierungsindex-Schema aus PlayStateFactories.class (s.
// CB_PLAY_DISCONNECT). Gegengeprueft an den direkten Nachbarn, die alle schon verifiziert
// waren: Set Display Objective 0x60, Set Health 0x66, Set Carried Item 0x67, Set Objective
// 0x68, Set Player Inventory 0x6A -- alle exakt getroffen.
//
// Aufbau (EntityEquipmentUpdateS2CPacket, 1.21.11 nachgesehen): VarInt entityId, dann eine
// Schleife aus je einem Byte und einem optionalen ItemStack. Das oberste Bit des Bytes
// (0x80) heisst "es folgt noch ein Eintrag", die unteren sieben sind der Slot-Index. Es gibt
// KEINE Anzahl vorweg -- der letzte Eintrag ist der ohne gesetztes Fortsetzungsbit.
pub const CB_PLAY_SET_EQUIPMENT: i32 = 0x64;

// Slot-Indizes = ordinal() von EquipmentSlot (1.21.11): MAINHAND, OFFHAND, FEET, LEGS,
// CHEST, HEAD, BODY, SADDLE. Der Packet-Codec nimmt ordinal(), NICHT das separate
// `index`-Feld des Enums (das dort andere Werte hat) -- verwechselt man beides, landet die
// Haupthand auf dem Kopf.
pub const EQUIPMENT_SLOT_MAINHAND: u8 = 0;
pub const EQUIPMENT_SLOT_OFFHAND: u8 = 1;
pub const EQUIPMENT_SLOT_BODY: u8 = 6;
pub const EQUIPMENT_SLOT_SADDLE: u8 = 7;
