// Spielphase: Keep-Alive, Login, Respawn, Spielereignisse, Position, Chunk-Batches und Bloecke.

use super::*;

// --- Play state ---
// ACHTUNG: Play-State-Packet-IDs verschieben sich zwischen Versionen. Die minecraft.wiki
// dokumentiert 776 (26.2), unser Ziel ist 774 (1.21.11).
//
// Der Versatz ist NICHT einheitlich -- das war eine falsche Annahme, die uns eine
// Fehlersuche gekostet hat: Keep Alive und Chunk Data liegen -1 daneben, Synchronize
// Player Position dagegen -2 (0x48 -> 0x46). Eine ID aus der Wiki abzuleiten ist hier
// also Raten. Jede ID gehoert gegen den echten Server geprueft, bevor man ihr glaubt
// (s. CORAL_PACKET_SCAN in connection.rs -- scannt unbekannte IDs und faellt auf,
// wenn eine davon eine plausible Spielerposition enthaelt).
pub const CB_PLAY_KEEP_ALIVE: i32 = 0x2B; // verifiziert: Server akzeptiert unsere Antwort
pub const CB_PLAY_CHUNK_DATA: i32 = 0x2C; // verifiziert: 473 Chunks dekodiert, 0 Fehler
// "Update Light" -- in PlayStateFactories drei Plaetze nach CHUNK_DATA (LIGHT_UPDATE).
pub const CB_PLAY_LIGHT_UPDATE: i32 = 0x2F;

// Ping/Pong (Play-State) -- GENAU wie im Configuration-State (s. CB_CONFIG_PING/
// pong_config_payload), nur mit anderer Packet-ID auf beiden Seiten. IDs ueber
// GameProtocols verifiziert: CLIENTBOUND_PING liegt im gemeinsamen
// Clientbound-Play-Index direkt vor CLIENTBOUND_LEVEL_CHUNK_WITH_LIGHT (0x2C, bereits
// verifiziert) -> 0x3B; SERVERBOUND_PONG liegt im Serverbound-Play-Index direkt nach
// SERVERBOUND_PLAYER_LOADED (0x2B) -> 0x2C.
//
// ECHTER BUG gewesen (per pktproxy-Mitschnitt gefunden): der Server schickt
// dieses Ping WAEHREND DES GESAMTEN PLAY-STATES sehr haeufig (mehrmals pro Sekunde, nicht
// nur einmalig wie Keep Alive), und ohne Pong-Antwort blieb Grims eigene Simulation fuer
// uns dauerhaft desynchron (Simulation/GroundSpoof-Flags ab dem ersten Tick, nie erholt) --
// ein echter Vanilla-Client beantwortet jedes einzelne Ping sofort mit Pong, unser Client
// hat das Play-Ping bis hierhin komplett ignoriert (nur das Config-Ping war behandelt).
pub const CB_PLAY_PING: i32 = 0x3B;
pub const SB_PLAY_PONG: i32 = 0x2C;

// PlayerLoadedC2SPacket (SERVERBOUND 0x2B, gegen PlayStateFactories verifiziert: zwischen
// PLAYER_INPUT 0x2A und PONG 0x2C). Ohne Inhalt. Vanilla schickt es, sobald nach dem Beitreten
// bzw. einem Respawn der Chunk unter dem Spieler da ist (ClientPlayNetworkHandler
// .setPlayerLoaded). Bis dahin laesst der Server Interaktionen liegen und nimmt erst nach
// einer Frist von selbst an, dass geladen ist -- das Queue-Schwert ging deshalb beim
// Beitreten erst nach 3-4 Sekunden.
pub const SB_PLAY_PLAYER_LOADED: i32 = 0x2B;

#[must_use] 
pub fn player_loaded_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_LOADED);
    w.into_bytes()
}

#[must_use] 
pub fn pong_play_payload(id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PONG).write_i32(id);
    w.into_bytes()
}

// Login (play), IMMER das allererste Play-Packet -- empirisch verifiziert per
// CORAL_PACKET_SCAN: id=0x30, danach playerId als 4-Byte-Int (big
// endian), dann hardcore (Bool), dann die "levels"-Menge (VarInt-Anzahl + je ein
// laengenpraefixiertes ResourceKey-String) -- Byte-fuer-Byte gegen die echte Nutzlast
// gegengeprueft: nach playerId+hardcore folgt exakt "\x03\x13minecraft:overworld..." (3
// Dimensionen, erste ist 19 Zeichen lang = "minecraft:overworld"). Wir brauchen nur
// playerId, um unsere eigene EntityId fuer serverbound Pakete (z.B. Player Command) zu
// kennen -- der Rest des Pakets (Dimensionsliste, CommonPlayerSpawnInfo, ...) bleibt
// ungeparst.
pub const CB_PLAY_LOGIN: i32 = 0x30;

// PlayerRespawnS2CPacket. Interessiert uns aus ZWEI Gruenden -- erstens (und urspruenglich)
// nicht wegen der Welt-Daten darin, sondern weil der
// Server damit den Spieler NEU AUFSETZT: Vanilla baut dabei eine neue ClientPlayerEntity, und
// deren `lastPlayerInput` faengt bei DEFAULT (alles false) an -- der naechste Tick meldet den
// Tastenzustand also zwangslaeufig neu. Genau darauf verlaesst sich GrimAC: sein
// PacketPlayerRespawn setzt `knownInput = KnownInput.DEFAULT` und `isSprinting = false`
// (aus dem Grim-Jar nachgesehen). Wer den Tastenzustand danach nicht neu meldet, bewegt sich
// fuer Grim ohne jede Eingabe.
// Zweitens WEGEN der Dimension darin (s. read_respawn_dimension): Vanilla baut in
// ClientPlayNetworkHandler.onPlayerRespawn nur dann eine neue ClientWorld, wenn sich der
// Welt-RegistryKey aendert -- und mit ihr einen
// neuen PendingUpdateManager, dessen Sequenznummer wieder bei 0 anfaengt. GrimAC zieht
// dieselbe Grenze: PacketPlayerRespawn.isWorldChange() vergleicht ab 1.16 den worldName und
// ruft nur dann BadPacketsH.onWorldChange() (setzt `lastSequence = 0`). Ein Tod-Respawn in
// derselben Dimension darf die Sequenz also NICHT zuruecksetzen, ein Portalwechsel MUSS.
// Registrierungsindex 80 (Bundle-Delimiter als 0), gegengeprueft an Player Position (0x46),
// Set Equipment (0x64) und System Chat (0x77).
pub const CB_PLAY_RESPAWN: i32 = 0x50;

pub fn read_login_player_id(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_i32()
}

// Hardcore-Welt? Das Feld direkt nach der Entity-ID (nur im Login, nicht im Respawn).
pub fn read_login_hardcore(payload: &[u8]) -> Result<bool, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let _player_id = r.read_i32()?;
    r.read_bool()
}

// Die Dimension (Welt-RegistryKey, z.B. "minecraft:the_nether") aus einem
// CommonPlayerSpawnInfo -- dem gemeinsamen Block, mit dem sowohl Login (play) als auch
// Respawn beginnen bzw. enden. Aus CommonPlayerSpawnInfo.<init> des 1.21.11-Jars
// nachgesehen:
//   VarInt dimensionType  -- DimensionType.PACKET_CODEC ist PacketCodecs.registryEntry(key)
//     in der EINARGUMENTIGEN Fassung, und die ist `registry(...)`: nur die rohe Registry-Id
//     als VarInt. NICHT die zweiargumentige Fassung mit 0-Markierung + Inline-Wert.
//   Identifier dimension  <- genau das brauchen wir
//   long seed, byte gameMode, byte lastGameMode, bool isDebug, bool isFlat,
//   Optional<GlobalPos> lastDeathLocation, VarInt portalCooldown, VarInt seaLevel (ungelesen)
pub(crate) fn read_spawn_info_dimension(r: &mut PacketReader) -> Result<String, ReadError> {
    let _dimension_type = r.read_varint()?;
    r.read_string()
}

// Dimension und (gehashter) Seed aus demselben Block -- zusammen kennzeichnen sie eine Welt
// (s. chunk_saver::world_dir).
pub(crate) fn read_spawn_info_world(r: &mut PacketReader) -> Result<(String, i64), ReadError> {
    let dimension = read_spawn_info_dimension(r)?;
    Ok((dimension, r.read_i64()?))
}

pub fn read_respawn_world(payload: &[u8]) -> Result<(String, i64), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_spawn_info_world(&mut r)
}

pub fn read_login_world(payload: &[u8]) -> Result<(String, i64), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let _player_id = r.read_i32()?;
    let _hardcore = r.read_bool()?;
    let levels = r.read_varint()?;
    for _ in 0..levels.max(0) {
        let _level = r.read_string()?;
    }
    let _max_players = r.read_varint()?;
    let _view_distance = r.read_varint()?;
    let _simulation_distance = r.read_varint()?;
    let _reduced_debug_info = r.read_bool()?;
    let _show_death_screen = r.read_bool()?;
    let _do_limited_crafting = r.read_bool()?;
    read_spawn_info_world(&mut r)
}

// Der Spielmodus aus demselben Block: hinter der Dimension kommen `long seed` und
// `byte gameMode` (GameMode-Index: 0 Survival, 1 Creative, 2 Adventure, 3 Spectator).
pub(crate) fn read_spawn_info_game_mode(r: &mut PacketReader) -> Result<u8, ReadError> {
    read_spawn_info_dimension(r)?;
    let _seed = r.read_i64()?;
    r.read_u8()
}

pub fn read_respawn_game_mode(payload: &[u8]) -> Result<u8, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_spawn_info_game_mode(&mut r)
}

pub fn read_login_game_mode(payload: &[u8]) -> Result<u8, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let _player_id = r.read_i32()?;
    let _hardcore = r.read_bool()?;
    let levels = r.read_varint()?;
    for _ in 0..levels.max(0) {
        let _level = r.read_string()?;
    }
    let _max_players = r.read_varint()?;
    let _view_distance = r.read_varint()?;
    let _simulation_distance = r.read_varint()?;
    let _reduced_debug_info = r.read_bool()?;
    let _show_death_screen = r.read_bool()?;
    let _do_limited_crafting = r.read_bool()?;
    read_spawn_info_game_mode(&mut r)
}

// GameStateChangeS2CPacket: Reason-Byte + Float. In PlayStateFactories Index 38 (GAME_EVENT,
// direkt nach FORGET_LEVEL_CHUNK; gegengeprueft an KEEP_ALIVE = 43 = 0x2B).
pub const CB_PLAY_GAME_EVENT: i32 = 0x26;
// GameStateChangeS2CPacket.GAME_MODE_CHANGED -- der Wert ist der neue GameMode-Index.
pub const GAME_EVENT_GAME_MODE_CHANGED: u8 = 3;
// Regen (ClientPlayNetworkHandler.onGameStateChange): 1 = Regen beginnt (Staerke auf 0, sie
// steigt danach per 7), 2 = Regen endet (Staerke auf 1, sie faellt danach per 7), 7 = die
// neue Regenstaerke 0..1. Ob es "regnet", entscheidet allein die Staerke (> 0.2).
pub const GAME_EVENT_RAIN_STARTED: u8 = 1;
pub const GAME_EVENT_RAIN_STOPPED: u8 = 2;
pub const GAME_EVENT_RAIN_GRADIENT: u8 = 7;
// 8 = neue Gewitterstaerke 0..1 (wirkt nur zusammen mit Regen, s. App::thunder_level).
pub const GAME_EVENT_THUNDER_GRADIENT: u8 = 8;
// GameStateChangeS2CPacket.INITIAL_CHUNKS_COMING (13): der Server beginnt, die Chunks um den
// Spieler zu schicken. Vanillas Ladebildschirm faengt erst danach an, auf den Chunk unter dem
// Spieler zu warten (ClientChunkLoadProgress: Start -> LoadChunks).
pub const GAME_EVENT_INITIAL_CHUNKS_COMING: u8 = 13;
// GameStateChangeS2CPacket.PROJECTILE_HIT_PLAYER (6): ein EIGENES Geschoss hat einen Spieler
// getroffen. Der "Ding"-Klang dazu entsteht nur im Client -- der Server schickt kein Geraeusch.
pub const GAME_EVENT_PROJECTILE_HIT_PLAYER: u8 = 6;

pub fn read_game_event(payload: &[u8]) -> Result<(u8, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    Ok((r.read_u8()?, r.read_f32()?))
}

// Dimension aus Login (play). Layout bis zum CommonPlayerSpawnInfo laut GameJoinS2CPacket:
// int playerId, bool hardcore, Menge von Welt-RegistryKeys (VarInt-Anzahl + Strings),
// VarInt maxPlayers/viewDistance/simulationDistance, bool reducedDebugInfo/showDeathScreen/
// doLimitedCrafting.
pub fn read_login_dimension(payload: &[u8]) -> Result<String, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let _player_id = r.read_i32()?;
    let _hardcore = r.read_bool()?;
    let levels = r.read_varint()?;
    for _ in 0..levels.max(0) {
        let _level = r.read_string()?;
    }
    let _max_players = r.read_varint()?;
    let _view_distance = r.read_varint()?;
    let _simulation_distance = r.read_varint()?;
    let _reduced_debug_info = r.read_bool()?;
    let _show_death_screen = r.read_bool()?;
    let _do_limited_crafting = r.read_bool()?;
    read_spawn_info_dimension(&mut r)
}

// Dimension aus dem Respawn-Paket: PlayerRespawnS2CPacket ist CommonPlayerSpawnInfo + ein
// Flag-Byte, der Spawn-Block steht also direkt hinter der Packet-Id.
pub fn read_respawn_dimension(payload: &[u8]) -> Result<String, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_spawn_info_dimension(&mut r)
}
// Forget Level Chunk: Server entlaedt eine Chunk-Spalte (Spieler zu weit weg). Aus
// GameProtocols.class des 1.21.11-Clients abgeleitet (0-basierter Registrierungsindex) und
// gegen zwei bereits verifizierte IDs gegengeprueft: derselbe Index liefert fuer Chunk Data
// exakt 0x2C und fuer Keep Alive exakt 0x2B -- das Schema stimmt also, damit ist Forget
// Level Chunk 0x25. Payload: ein Long (ChunkPos), x = untere 32 Bit, z = obere 32 Bit.
pub const CB_PLAY_FORGET_LEVEL_CHUNK: i32 = 0x25;
// Verifiziert: Paket 0x46 lieferte 104.5/64.0/209.5 -- exakt die Position, die der Server
// im eigenen Log als Login-Position meldete.
pub const CB_PLAY_PLAYER_POSITION: i32 = 0x46;

// Player Rotation (S2C): der Server dreht NUR unsere Blickrichtung, ohne uns zu bewegen.
// Eigenes Paket seit 1.21.2, unabhaengig vom Teleport oben.
//
// ID 0x47 aus PlayStateFactories (1.21.11, yarn-gemapptes Jar mit Vineflower nachgesehen):
// Registrierungsindex im S2C-Builder, GANZE Tabelle abgeglichen -- 14 andere bereits gegen
// echte Server verifizierte IDs (0x20 Disconnect, 0x2C Chunk, 0x3B Ping, 0x46 Position,
// 0x51 RotateHead, ...) stimmen bei derselben Zaehlung exakt. Wer nur bis PLAYER_ROTATION
// zaehlt, verzaehlt sich leicht: der Builder mischt vier Konstanten-Klassen (PlayPackets,
// CommonPackets, CookiePackets, PingPackets), und wer eine davon uebersieht, landet
// systematisch daneben.
//
// Wire-Format nach PlayerRotationS2CPacket.CODEC: yaw (f32), relativeYaw (bool),
// pitch (f32), relativePitch (bool). Die beiden Bools sind je Achse einzeln -- anders als
// beim Teleport, der seine Relativ-Angaben in ein gemeinsames Flag-Feld packt.
pub const CB_PLAY_PLAYER_ROTATION: i32 = 0x47;

// Liest Player Rotation: (yaw, yaw_relativ, pitch, pitch_relativ).
pub fn read_player_rotation(payload: &[u8]) -> Result<(f32, bool, f32, bool), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let yaw = r.read_f32()?;
    let relative_yaw = r.read_bool()?;
    let pitch = r.read_f32()?;
    let relative_pitch = r.read_bool()?;
    Ok((yaw, relative_yaw, pitch, relative_pitch))
}

// --- Chunk-Stapel (seit 1.20.2) -------------------------------------------------------
//
// Der Server schickt Chunks in STAPELN und wartet auf eine Empfangsbestaetigung, bevor er
// weitersendet. ChunkDataSender (server/network/ChunkDataSender.java, aus dem 1.21.11-Jar
// nachgesehen) sendet nur, solange `unacknowledgedBatches < maxUnacknowledgedBatches` --
// und `maxUnacknowledgedBatches` startet bei **1** und wird ERST in onAcknowledgeChunks auf
// 10 hochgesetzt. Ohne unsere Bestaetigung kommt also nach dem allerersten Stapel
// (desiredBatchSize startet bei 9.0, also bis zu 9 Chunks) NIE WIEDER ein Chunk: die
// Verbindung bleibt offen, Keep Alive laeuft weiter, die Welt hoert einfach auf zu wachsen.
// Genau dieses Bild ("kommt nicht voran", haengt beim Verbinden) hatte der Client vor
// diesem Paket -- er hat Batch Start/Finished stillschweigend ignoriert.
//
// IDs nach demselben Registrierungsindex-Schema wie CB_PLAY_DISCONNECT aus
// PlayStateFactories.class (S2C-Kette, Bundle-Delimiter als Index 0).
//
// ECHTER BUG gewesen -- diese beiden standen hier eins zu NIEDRIG (0x0A/0x0B) und haben den
// Haenger beim Verbinden verursacht, den das Stapel-Paket eigentlich beheben sollte. Die
// Registrierungskette lautet ab Index 10 (aus dem PlayStateFactories, gegen
// die verifizierten Nachbarn Block Update 0x08, Clear Titles 0x0E, Keep Alive 0x2B, Chunk
// Data 0x2C gegengeprueft):
//   0x0A CHANGE_DIFFICULTY_S2C, 0x0B CHUNK_BATCH_FINISHED, 0x0C CHUNK_BATCH_START
// Mit den alten Werten galt also das DIFFICULTY-Paket als "Stapel beendet". Dessen erstes
// Feld ist die Schwierigkeit als ein Byte -- read_chunk_batch_size las das als Stapelgroesse
// und meldete "2 Chunks" (2 = NORMAL). Daher genau EINE (falsche, viel zu fruehe)
// Bestaetigung, die serverseitig maxUnacknowledgedBatches von 1 auf 10 hob; das echte
// CHUNK_BATCH_FINISHED galt als "Start" und wurde nie bestaetigt. Der Server schickte danach
// noch bis zu 10 Stapel und stellte die Chunk-Lieferung dann fuer immer ein -- je nach
// Zeitpunkt der Difficulty-Nachricht mal mit halb geladener Welt, mal mit gar keiner.
pub const CB_PLAY_CHUNK_BATCH_FINISHED: i32 = 0x0B;
pub const CB_PLAY_CHUNK_BATCH_START: i32 = 0x0C;
// Nur zur Abgrenzung/Dokumentation: der frueher faelschlich als "Stapel beendet" behandelte
// Nachbar. Wird nicht ausgewertet (die Schwierigkeit beeinflusst hier nichts), steht aber
// hier, damit die Verwechslung nicht ein zweites Mal passiert.
pub const CB_PLAY_CHANGE_DIFFICULTY: i32 = 0x0A;

// ChunkSentS2CPacket: ein einziger VarInt = Anzahl der Chunks im gerade beendeten Stapel.
pub fn read_chunk_batch_size(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

// AcknowledgeChunksC2SPacket: ein einziger f32 = wie viele Chunks pro Tick wir haben
// wollen. Serverbound-Registrierungsindex 10 (0-basiert, s. SB_PLAY_CHAT-Kommentar):
// ... chat=0x08, chat_session_update=0x09, chunk_batch_received=0x0A, client_command=0x0B
// (bereits verifiziert), client_tick_end=0x0C (bereits verifiziert).
pub const SB_PLAY_CHUNK_BATCH_RECEIVED: i32 = 0x0A;

#[must_use] 
pub fn chunk_batch_received_payload(desired_chunks_per_tick: f32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CHUNK_BATCH_RECEIVED).write_f32(desired_chunks_per_tick);
    w.into_bytes()
}

// --- Rueckkehr in den Configuration-State ----------------------------------------------
//
// EnterReconfigurationS2CPacket: der Server schickt den Spieler MITTEN IM SPIEL zurueck in
// den Configuration-State (Serverwechsel hinter einem Proxy, Datenpaket-Neuladen, "Relog"
// von Test-/Anticheat-Servern). Vanilla (ClientPlayNetworkHandler.onEnterReconfiguration)
// antwortet sofort mit AcknowledgeReconfigurationC2SPacket und wechselt selbst in den
// Configuration-State.
//
// Wer das ignoriert, haengt fuer immer: der Server wartet auf die Bestaetigung und schickt
// bis dahin nichts Spielbares mehr, waehrend die Verbindung offen bleibt. Der Client sieht
// nur, dass ploetzlich nichts mehr kommt -- kein Fehler, kein Disconnect.
//
// IDs aus PlayStateFactories.class: clientbound Registrierungsindex 116 (Bundle-Delimiter
// als Index 0), serverbound Index 15.
pub const CB_PLAY_START_CONFIGURATION: i32 = 0x74;
pub const SB_PLAY_CONFIGURATION_ACKNOWLEDGED: i32 = 0x0F;

// Reines Signalpaket: nur die ID, kein Rumpf.
#[must_use] 
pub fn configuration_acknowledged_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONFIGURATION_ACKNOWLEDGED);
    w.into_bytes()
}
// Aus GameProtocols.class abgeleitet (0-basierter Registrierungsindex im CLIENTBOUND_TEMPLATE-
// Builder, s. Kommentar bei CB_PLAY_ADD_ENTITY) -- gegen alle bereits verifizierten IDs
// (Add Entity, Entity Position Sync, Forget Level Chunk, Keep Alive, Chunk Data, Move Entity
// Pos/Rot, Player Position, Remove Entities, Rotate Head, Set Health, Teleport Entity)
// gegengeprueft, alle exakt getroffen -- ersetzt den frueheren geratenen Wiki-Wert 0x1F.
pub const CB_PLAY_DISCONNECT: i32 = 0x20;
// Block Update (0x08) / Section Blocks Update (0x52): einzelne bzw. gebuendelte
// Blockaenderungen (Platzieren/Abbauen, Kolben, Redstone, ...) -- ohne diese beiden bleibt
// der Client nach dem initialen Chunk-Laden staendig auf dem Stand von damals, weil er nie
// erfaehrt, wenn sich ein Block aendert. Nach demselben Registrierungsindex-Schema wie
// CB_PLAY_DISCONNECT abgeleitet und gegen dieselben bekannten IDs verifiziert.
pub const CB_PLAY_BLOCK_UPDATE: i32 = 0x08;
pub const CB_PLAY_SECTION_BLOCKS_UPDATE: i32 = 0x52;

// PlayerActionResponseS2CPacket ("Block Changed Ack"): der Server spiegelt die Sequenznummer
// unserer Block-Aktion zurueck und sagt damit "alles bis einschliesslich dieser Nummer ist
// bei mir durch". Erst dann darf der Client seine Vorhersage aufgeben (s. app::PendingBlock).
// Index 4 in derselben Registrierungsreihenfolge wie CB_PLAY_BLOCK_UPDATE, direkt gegen
// PlayStateFactories geprueft: ADD_ENTITY 1, ANIMATE 2, AWARD_STATS 3, BLOCK_CHANGED_ACK 4,
// BLOCK_DESTRUCTION 5, BLOCK_ENTITY_DATA 6, BLOCK_EVENT 7, BLOCK_UPDATE 8 -- die 8 ist
// bereits verifiziert, damit stimmt auch die 4.
pub const CB_PLAY_BLOCK_CHANGED_ACK: i32 = 0x04;

// Block Event (BlockEventS2CPacket, Index 7 derselben Kette wie oben): Position, Typ (u8),
// Daten (u8), Block (Registry-VarInt). Gebraucht fuer Truhen-/Shulkerdeckel: Typ 1 traegt
// die Zahl der Betrachter (ShulkerBoxBlockEntity.onSyncedBlockEvent).
pub const CB_PLAY_BLOCK_EVENT: i32 = 0x07;

pub fn read_block_event(payload: &[u8]) -> Result<((i32, i32, i32), u8, u8), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let pos = decode_block_pos(r.read_i64()?);
    let typ = r.read_u8()?;
    let daten = r.read_u8()?;
    Ok((pos, typ, daten))
}

// Nutzlast ist genau eine VarInt-Sequenznummer.
pub fn read_block_changed_ack(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

// Entity-Pakete: aus PlayStateFactories.class (1.21.11, yarn) abgeleitet -- die
// Clientbound-Play-Packets werden dort in fester Reihenfolge registriert, die ID ist der
// 0-basierte Index (addBundle fuer den Bundle-Delimiter zaehlt als Index 0). Das Schema ist
// mit DREI bereits empirisch verifizierten IDs gegengeprueft: Forget Level Chunk landet auf
// exakt 0x25, Keep Alive auf 0x2B, Chunk Data auf 0x2C. Seit 1.20.2 spawnen auch SPIELER
// ueber Add Entity (ein eigenes Spawn-Player-Paket gibt es nicht mehr).
pub const CB_PLAY_ADD_ENTITY: i32 = 0x01;
pub const CB_PLAY_ENTITY_POSITION_SYNC: i32 = 0x23;
pub const CB_PLAY_MOVE_ENTITY_POS: i32 = 0x33;
pub const CB_PLAY_MOVE_ENTITY_POS_ROT: i32 = 0x34;
pub const CB_PLAY_MOVE_ENTITY_ROT: i32 = 0x36;
pub const CB_PLAY_REMOVE_ENTITIES: i32 = 0x4B;
pub const CB_PLAY_ROTATE_HEAD: i32 = 0x51;
pub const CB_PLAY_TELEPORT_ENTITY: i32 = 0x7B;
// Set Health (0x66): Leben (Float), Nahrung (VarInt), Saettigung (Float). Leben <= 0 = tot.
// Nach demselben verifizierten Registrierungsindex-Schema abgeleitet (SET_HEALTH = Index
// 0x66 im S2C-Block, gegen vier bekannte IDs gegengeprueft).
pub const CB_PLAY_SET_HEALTH: i32 = 0x66;
