// Pakete an den Server: Befehle, Chat, Spieleraktionen, Items benutzen, Bewegung und Interaktion.

use super::*;

// Client Command (0x0B), Aktion PERFORM_RESPAWN = 0: nach dem Tod den Respawn anfordern.
pub const SB_PLAY_CLIENT_COMMAND: i32 = 0x0B;

// Client Tick End (0x0C): leeres Signal-Paket (seit der 1.21.2-Netzwerk-Ueberarbeitung,
// ClientTickEndC2SPacket -- reines record() ohne Felder), das der Client GENAU EINMAL PRO
// EIGENEM Tick senden muss, direkt nach den Bewegungspaketen dieses Ticks. Server/Anticheat
// zaehlen daran die tatsaechlich vergangenen Client-Ticks mit. Ohne dieses Paket faellt
// z.B. Grims TickTimer-Check sofort durch ("did not send client tick end packet") -- und
// reisst darueber auch dessen Zeitreferenz fuer ALLE anderen Bewegungs-Checks (Simulation,
// GroundSpoof, ...) mit, selbst wenn die eigentliche Bewegungsphysik korrekt ist
// (Grim flaggte dann fast alles). ID = Index 12 in
// GameProtocols.SERVERBOUND_TEMPLATE (direkt aus net/minecraft/network/protocol/game/
// GameProtocols.class im echten, Mojang-gemappten Paper-1.21.11-Server-Jar nachgesehen --
// die exakte, bereits mit den "common" Paketen gemergte Registrierungsreihenfolge, nicht
// nur eine PlayPackets-Teilliste). Gegengeprueft: nach demselben Schema liegen ACHT
// weitere, teils empirisch verifizierte IDs (SB_PLAY_CHAT=8, CLIENT_COMMAND=0x0B,
// INTERACT=0x19, MOVE_PLAYER_POS=0x1D, PLAYER_COMMAND=0x29, PLAYER_INPUT=0x2A, SWING=0x3C,
// sowie CB_PLAY_KEEP_ALIVE=0x2B und CB_PLAY_CHUNK_DATA=0x2C) exakt richtig.
pub const SB_PLAY_CLIENT_TICK_END: i32 = 0x0C;

#[must_use] 
pub fn client_tick_end_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CLIENT_TICK_END);
    w.into_bytes()
}

// Chat (0x08): dieselbe GameProtocols.SERVERBOUND_TEMPLATE-Quelle wie Client Tick End oben.
// ChatMessageC2SPacket-Felder (Yarn-1.21.11-Jar nachgesehen): String (max 256), Instant
// Timestamp (i64 Epoch-Millis), i64 Salt, Optional<MessageSignatureData> Signatur,
// LastSeenMessageList.Acknowledgment. Unsigniert gesendet (Signatur=None, Salt=0) -- ohne
// Player-Session-Paket (eigener Signierschluessel, hier nicht implementiert) kann ohnehin
// nicht signiert werden; funktioniert nur, wenn der Server keine "secure profiles"
// erzwingt (Standard bei den meisten privaten/Test-Servern, inkl. offline-mode).
pub const SB_PLAY_CHAT: i32 = 0x08;

// Chatnachricht, wahlweise signiert. Ohne Signatur (None) nehmen nur Server mit
// enforce-secure-profile=false sie an -- alle anderen antworten mit
// "chat.disabled.missingProfileKey". Signatur und Zeitstempel/Salt gehoeren zusammen: die
// Signatur wird ueber GENAU diese Werte gebildet (s. net::chat_signing), deshalb kommen
// sie hier als Parameter statt hier drin erzeugt zu werden.
#[must_use] 
pub fn signed_chat_message_payload(message: &str, timestamp_millis: i64, salt: i64, signature: Option<&[u8; 256]>) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CHAT).write_string(message).write_i64(timestamp_millis).write_i64(salt);
    match signature {
        Some(bytes) => {
            // MessageSignatureData::write schreibt die 256 Byte roh, ohne Laengenangabe.
            w.write_bool(true).write_raw_bytes(bytes);
        }
        None => {
            w.write_bool(false);
        }
    }
    // LastSeenMessageList.Acknowledgment: VarInt Offset=0, 20-Bit-BitSet (leer -> 3 Byte
    // 0x00, s. PacketByteBuf.writeFixedBitSet), Byte Checksum=0 (NO_CHECKSUM).
    w.write_varint(0).write_u8(0).write_u8(0).write_u8(0).write_u8(0);
    w.into_bytes()
}

#[must_use] 
pub fn chat_message_payload(message: &str) -> Vec<u8> {
    let now_millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
    signed_chat_message_payload(message, now_millis, 0, None)
}

// PlayerSessionC2SPacket: meldet dem Server den Profilschluessel dieser Sitzung. MUSS vor
// der ersten signierten Nachricht raus, sonst kennt der Server den Schluessel nicht, mit
// dem er die Signaturen pruefen soll.
//
// Aufbau (PublicPlayerSession.Serialized): UUID der Sitzung, Ablaufzeit in Millis,
// oeffentlicher Schluessel als laengenpraefigiertes X.509-DER, danach Mojangs Signatur
// darueber (ebenfalls laengenpraefigiert).
pub const SB_PLAY_CHAT_SESSION_UPDATE: i32 = 0x09;

// PlayerActionC2SPacket: VarInt-Aktion, BlockPos (gepackter i64), Byte-Richtung, VarInt-
// Sequenznummer. Fuer die beiden Aktionen hier (Wegwerfen, Zweithand-Tausch) spielt der
// Block keine Rolle -- Vanilla schickt dort BlockPos.ORIGIN und Richtung DOWN (Index 0).
pub const SB_PLAY_PLAYER_ACTION: i32 = 0x28;
// Reihenfolge des Action-Enums im 1.21.11-Jar.
pub const PLAYER_ACTION_DROP_ALL_ITEMS: i32 = 3;
pub const PLAYER_ACTION_DROP_ITEM: i32 = 4;
// RELEASE_USE_ITEM: beendet die Benutzung des Items in der Hand. Vanilla schickt das beim
// LOSLASSEN der Rechtsklick-Taste (ClientPlayerInteractionManager.stopUsingItem). Ohne dieses
// Paket bleibt GrimAC dauerhaft der Meinung, wir wuerden noch ein Item benutzen.
//
// Es ist NICHT sequenziert: stopUsingItem baut das Paket mit dem DREI-Argument-Konstruktor
// (Action, BlockPos.ORIGIN, Direction.DOWN), und der setzt die Sequenz auf 0 -- es laeuft
// nicht ueber sendSequencedPacket. Deshalb geht es hier durch player_action_payload wie
// Wegwerfen und Zweithand-Tausch. Eine eigene Sequenz kostete zwei GrimAC-Flags zugleich:
// BadPacketsL ("Sent impossible dig packet") verlangt fuer jede Nicht-Grab-Aktion
// pos=0/0/0, face=0 UND sequence=0, und weil unser Zaehler dabei hochlief, sprang die
// naechste ECHTE Sequenz um 2 -- BadPacketsH ("unexpected sequence id", erwartet
// lastSequence+1) flaggte jedes Mal hinterher.
pub const PLAYER_ACTION_RELEASE_USE_ITEM: i32 = 5;
pub const PLAYER_ACTION_SWAP_OFFHAND: i32 = 6;
// STAB (neu in 1.21.11): Linksklick mit einer Stichwaffe (Speer, PiercingWeaponComponent).
// Statt Interact(ATTACK) auf ein Ziel -- welche Entities getroffen werden, rechnet der
// Server selbst aus (PiercingWeaponComponent.stab). Unsequenziert wie RELEASE_USE_ITEM
// (ClientPlayerInteractionManager.attackWithPiercingWeapon, Drei-Argument-Konstruktor).
pub const PLAYER_ACTION_STAB: i32 = 7;

// Abbauen (PlayerActionC2SPacket.Action): START/ABORT/STOP_DESTROY_BLOCK sind 0/1/2. START
// und STOP gehen SEQUENZIERT raus (sendSequencedPacket), ABORT mit Sequenz 0
// (ClientPlayerInteractionManager.cancelBlockBreaking schickt es direkt).
pub const PLAYER_ACTION_START_DESTROY_BLOCK: i32 = 0;
pub const PLAYER_ACTION_ABORT_DESTROY_BLOCK: i32 = 1;
pub const PLAYER_ACTION_STOP_DESTROY_BLOCK: i32 = 2;

// Player Action mit Blockposition, Seite (Direction-Index, 0 = unten) und Sequenz.
#[must_use] 
pub fn player_action_block_payload(action: i32, x: i32, y: i32, z: i32, face: i32, sequence: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_ACTION)
        .write_varint(action)
        .write_i64(encode_block_pos(x, y, z))
        .write_u8(face as u8)
        .write_varint(sequence);
    w.into_bytes()
}

#[must_use] 
pub fn player_action_payload(action: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_ACTION).write_varint(action).write_i64(0).write_u8(0).write_varint(0);
    w.into_bytes()
}

// --- Rechtsklick: Benutzen / Platzieren ------------------------------------------------
//
// Zwei getrennte Pakete, genau wie in Vanilla (ClientPlayerInteractionManager.interactBlock
// bzw. interactItem):
//   USE_ITEM_ON  -- der Strahl hat einen Block getroffen (Platzieren, Kiste oeffnen, ...)
//   USE_ITEM     -- ins Leere geklickt (Bogen spannen, Perle werfen, Essen, ...)
// IDs aus PlayStateFactories.class (serverbound-Kette, 0-basiert), gegengeprueft an den
// bereits verifizierten Nachbarn Player Action (0x28), Set Carried Item (0x34) und Swing
// (0x3C) -- alle exakt getroffen.
pub const SB_PLAY_USE_ITEM_ON: i32 = 0x3F;
pub const SB_PLAY_USE_ITEM: i32 = 0x40;

// Hand-Enum (Yarn Hand): Haupthand 0, Zweithand 1.
pub const HAND_MAIN: i32 = 0;
pub const HAND_OFF: i32 = 1;

// Direction-Enum in Deklarationsreihenfolge (Yarn Direction) -- die Zahl, die als VarInt in
// den BlockHitResult geht.
pub const FACE_DOWN: i32 = 0;
pub const FACE_UP: i32 = 1;
pub const FACE_NORTH: i32 = 2;
pub const FACE_SOUTH: i32 = 3;
pub const FACE_WEST: i32 = 4;
pub const FACE_EAST: i32 = 5;

// PlayerInteractBlockC2SPacket: Hand (VarInt-Enum), BlockHitResult, Sequenznummer (VarInt).
// BlockHitResult ist laut PacketByteBuf.writeBlockHitResult: BlockPos (gepackter i64),
// Richtung (VarInt-Enum), drei f32 mit der Trefferstelle RELATIV zur Blockecke, dann zwei
// Bools (im Block / an der Weltgrenze).
//
// Die Sequenznummer ist Pflicht, kein Beiwerk: der Server schickt sie im Block-Changed-Ack
// zurueck, und Vanilla verwirft damit seine eigene Vorhersage. Sie muss STRENG steigen --
// ein Wiederholen laesst den Server die Aktion verwerfen (s. App::next_block_sequence).
#[allow(clippy::too_many_arguments)]
#[must_use] 
pub fn use_item_on_payload(
    hand: i32,
    x: i32,
    y: i32,
    z: i32,
    face: i32,
    cursor_x: f32,
    cursor_y: f32,
    cursor_z: f32,
    inside_block: bool,
    sequence: i32,
) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_USE_ITEM_ON)
        .write_varint(hand)
        .write_i64(encode_block_pos(x, y, z))
        .write_varint(face)
        .write_f32(cursor_x)
        .write_f32(cursor_y)
        .write_f32(cursor_z)
        .write_bool(inside_block)
        // "an der Weltgrenze getroffen" -- das kann nur ein Strahl auf die Weltbarriere sein,
        // fuer einen Blocktreffer immer false.
        .write_bool(false)
        .write_varint(sequence);
    w.into_bytes()
}

// PlayerInteractItemC2SPacket: Hand, Sequenznummer, Yaw, Pitch (beide in Grad, Vanilla-
// Konvention). Yaw/Pitch stehen hier zusaetzlich drin, weil der Server die Blickrichtung im
// Moment des Benutzens braucht (Perle/Bogen fliegen danach).
#[must_use] 
pub fn use_item_payload(hand: i32, sequence: i32, yaw_deg: f32, pitch_deg: f32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_USE_ITEM).write_varint(hand).write_varint(sequence).write_f32(yaw_deg).write_f32(pitch_deg);
    w.into_bytes()
}

#[must_use] 
pub fn chat_session_update_payload(session_id: [u8; 16], expires_at_millis: i64, public_key_der: &[u8], key_signature: &[u8]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CHAT_SESSION_UPDATE)
        .write_raw_bytes(&session_id)
        .write_i64(expires_at_millis)
        .write_bytes(public_key_der)
        .write_bytes(key_signature);
    w.into_bytes()
}

// Chat Command (0x06, "chat_command" = CommandExecutionC2SPacket, UNSIGNIERT). Seit 1.19
// MUESSEN Befehle ueber ein eigenes Paket raus, NICHT als Chat-Nachricht -- sonst flaggt Grim
// (ChatB: eine Chat-Nachricht, die mit '/' beginnt, ist fuer einen echten Client unmoeglich).
// ID aus PlayPackets.class abgeleitet (c2s-Kette accept_teleportation=0x00 .. chat=0x08
// lueckenlos, chat_command liegt bei 0x06). `command` OHNE fuehrenden Schraegstrich.
// CommandExecutionC2SPacket-Rumpf (Yarn-1.21.11 nachgesehen): nur ein String, keine
// Timestamps/Signaturen (das ist die unsignierte Variante).
pub const SB_PLAY_CHAT_COMMAND: i32 = 0x06;

#[must_use] 
pub fn chat_command_payload(command: &str) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CHAT_COMMAND).write_string(command);
    w.into_bytes()
}

pub const SB_PLAY_CONFIRM_TELEPORT: i32 = 0x00; // erstes Packet, versionsstabil
pub const SB_PLAY_KEEP_ALIVE: i32 = 0x1B; // 776: 0x1C

// Move-Player-Varianten: nicht empirisch verifiziert wie die IDs oben (dafuer gibt es
// serverbound keine Scan-Technik -- die Wiki-Referenz ist versionsabhaengig unzuverlaessig,
// s. Kommentar oben), sondern aus GameProtocols.class des echten 1.21.11-Client-Jars
// (net.minecraft.network.protocol.game, aus dem Fabric-Loom-Cache nachgesehen) abgelesen:
// die Serverbound-Play-Packets werden dort in genau dieser Reihenfolge registriert, die ID
// ist der 0-basierte Index. Gegengeprueft: nach demselben Schema liegt Confirm Teleportation
// exakt auf Index 0 (0x00, oben verifiziert) und Keep Alive exakt auf Index 27 (0x1B, oben
// verifiziert) -- beide stimmen exakt mit den empirisch bestaetigten IDs ueberein, das
// Indexierungsschema ist also vertrauenswuerdig.
pub const SB_PLAY_MOVE_PLAYER_POS: i32 = 0x1D;
pub const SB_PLAY_MOVE_PLAYER_POS_ROT: i32 = 0x1E;
pub const SB_PLAY_MOVE_PLAYER_ROT: i32 = 0x1F;
pub const SB_PLAY_MOVE_PLAYER_STATUS_ONLY: i32 = 0x20;
// Kampf: nach demselben Registrierungsindex-Schema abgeleitet (alle sechs oben stehenden
// C2S-IDs stimmen als Gegenprobe exakt).
pub const SB_PLAY_INTERACT: i32 = 0x19;
pub const SB_PLAY_SWING: i32 = 0x3C;

// Player Input (0x2A): forward/backward/left/right/jump/sneak/sprint als Bitflags in EINEM
// Byte -- Index 42 in GameProtocols.java (paper-1.21.11.jar, echtes Server-Jar mit Mojang-
// Mapping), nach demselben Registrierungsindex-Schema wie oben abgeleitet
// UND explizit gegen VIER bereits bekannte IDs gegengeprueft (Index 0 = Confirm
// Teleportation = 0x00, Index 25 = Interact = 0x19, Index 27 = Keep Alive = 0x1B, Index 60 =
// Swing = 0x3C -- alle vier stimmen exakt). WICHTIG: der Registrierungs-Index zaehlt auch
// "common"/Cookie/Ping-Pakete mit, die NICHT in PlayPackets.java (Yarn) auftauchen -- eine
// erste Yarn-only-Zaehlung ohne diese landete faelschlich einen Index zu niedrig.
// ACHTUNG, urspruenglicher Irrtum: dieses Paket ist NICHT die Quelle fuer
// isSprinting() serverseitig -- ServerGamePacketListenerImpl.handlePlayerInput() (echter
// Paper-Server-Code, nachgesehen) liest es NUR fuer Bewegungs-Vorhersage/-Validierung, ruft
// aber nie setSprinting() auf. Trotzdem sinnvoll zu senden (Server-seitige Bewegungspruefung
// nutzt es), aber allein reicht es NICHT -- s. SB_PLAY_PLAYER_COMMAND darunter fuer die
// tatsaechliche Sprint-Pose-Quelle.
pub const SB_PLAY_PLAYER_INPUT: i32 = 0x2A;
pub(crate) const PLAYER_INPUT_FORWARD: u8 = 1;
pub(crate) const PLAYER_INPUT_BACKWARD: u8 = 2;
pub(crate) const PLAYER_INPUT_LEFT: u8 = 4;
pub(crate) const PLAYER_INPUT_RIGHT: u8 = 8;
pub(crate) const PLAYER_INPUT_JUMP: u8 = 16;
pub(crate) const PLAYER_INPUT_SNEAK: u8 = 32;
pub(crate) const PLAYER_INPUT_SPRINT: u8 = 64;

// Player Input Payload: EIN Byte Bitflags -- nach PlayerInput.PACKET_CODEC::encode des
// echten Clients (net.minecraft.util.PlayerInput).
#[allow(clippy::too_many_arguments)]
#[must_use] 
pub fn player_input_payload(forward: bool, backward: bool, left: bool, right: bool, jump: bool, sneak: bool, sprint: bool) -> Vec<u8> {
    let mut flags = 0u8;
    if forward {
        flags |= PLAYER_INPUT_FORWARD;
    }
    if backward {
        flags |= PLAYER_INPUT_BACKWARD;
    }
    if left {
        flags |= PLAYER_INPUT_LEFT;
    }
    if right {
        flags |= PLAYER_INPUT_RIGHT;
    }
    if jump {
        flags |= PLAYER_INPUT_JUMP;
    }
    if sneak {
        flags |= PLAYER_INPUT_SNEAK;
    }
    if sprint {
        flags |= PLAYER_INPUT_SPRINT;
    }
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_INPUT).write_u8(flags);
    w.into_bytes()
}

// Player Command (0x29, Index 41 -- direkt vor Player Input, s. dortigen Kommentar zur
// Verifikation): die TATSAECHLICHE Quelle fuer LivingEntity.setSprinting() serverseitig.
// ServerGamePacketListenerImpl.handlePlayerCommand() (echter Paper-Server-Code,
// nachgesehen) setzt bei Action=START_SPRINTING/STOP_SPRINTING direkt
// den Sprint des Spielers -- GENAU das speist Entity.updateSwimming()
// (isSprinting() && isInWater()/isUnderWater()), also die Pose/Kollisionsbox, die der
// Server fuer uns verwendet. Wire-Format nach ClientCommandC2SPacket::write des echten
// Clients: EntityId (VarInt), Mode (VarInt-Enum: STOP_SLEEPING=0, START_SPRINTING=1,
// STOP_SPRINTING=2, START_RIDING_JUMP=3, STOP_RIDING_JUMP=4, OPEN_INVENTORY=5,
// START_FALL_FLYING=6), mountJumpHeight (VarInt, hier immer 0 -- nur fuer
// START_RIDING_JUMP relevant).
pub const SB_PLAY_PLAYER_COMMAND: i32 = 0x29;
pub(crate) const PLAYER_COMMAND_MODE_STOP_SLEEPING: i32 = 0;
pub(crate) const PLAYER_COMMAND_MODE_START_SPRINTING: i32 = 1;
pub(crate) const PLAYER_COMMAND_MODE_STOP_SPRINTING: i32 = 2;
pub(crate) const PLAYER_COMMAND_MODE_START_FALL_FLYING: i32 = 6;

// Aufstehen: ClientCommandC2SPacket mit Mode STOP_SLEEPING (0). Genau das schickt Vanilla,
// wenn man im Schlaf-Bildschirm "Bett verlassen" waehlt -- ein eigenes Aufwach-Paket gibt es
// nicht, und ohne dieses Paket bleibt der Server der Meinung, wir liegen weiter im Bett.
#[must_use] 
pub fn player_command_stop_sleeping_payload(entity_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_COMMAND).write_varint(entity_id).write_varint(PLAYER_COMMAND_MODE_STOP_SLEEPING).write_varint(0);
    w.into_bytes()
}

// Elytra-Flug beginnen: ClientPlayerEntity.tickMovement schickt das, nachdem es den Flug
// lokal schon gestartet hat (s. physics::PlayerPhysics::gliding).
#[must_use] 
pub fn player_command_start_fall_flying_payload(entity_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_COMMAND).write_varint(entity_id).write_varint(PLAYER_COMMAND_MODE_START_FALL_FLYING).write_varint(0);
    w.into_bytes()
}

#[must_use] 
pub fn player_command_sprint_payload(entity_id: i32, sprinting: bool) -> Vec<u8> {
    let mode = if sprinting { PLAYER_COMMAND_MODE_START_SPRINTING } else { PLAYER_COMMAND_MODE_STOP_SPRINTING };
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_COMMAND).write_varint(entity_id).write_varint(mode).write_varint(0);
    w.into_bytes()
}

pub(crate) const MOVE_FLAG_ON_GROUND: u8 = 1;
pub(crate) const MOVE_FLAG_HORIZONTAL_COLLISION: u8 = 2;

// Serverbound Move Player Pos+Rot: X/Y/Z (Double), Yaw/Pitch (Float, Grad), Flags (Byte) --
// Feldreihenfolge nach ServerboundMovePlayerPacket.PosRot::write des echten Clients.
#[must_use] 
pub fn move_player_pos_rot_payload(
    x: f64,
    y: f64,
    z: f64,
    yaw_deg: f32,
    pitch_deg: f32,
    on_ground: bool,
    horizontal_collision: bool,
) -> Vec<u8> {
    let mut flags = 0u8;
    if on_ground {
        flags |= MOVE_FLAG_ON_GROUND;
    }
    if horizontal_collision {
        flags |= MOVE_FLAG_HORIZONTAL_COLLISION;
    }
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_MOVE_PLAYER_POS_ROT)
        .write_f64(x)
        .write_f64(y)
        .write_f64(z)
        .write_f32(yaw_deg)
        .write_f32(pitch_deg)
        .write_u8(flags);
    w.into_bytes()
}

// Serverbound Move Player Pos (nur Position, KEINE Rotation geaendert) -- Feldlayout aus
// PlayerMoveC2SPacket.PositionAndOnGround::write.
#[must_use] 
pub fn move_player_pos_payload(x: f64, y: f64, z: f64, on_ground: bool, horizontal_collision: bool) -> Vec<u8> {
    let mut flags = 0u8;
    if on_ground {
        flags |= MOVE_FLAG_ON_GROUND;
    }
    if horizontal_collision {
        flags |= MOVE_FLAG_HORIZONTAL_COLLISION;
    }
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_MOVE_PLAYER_POS).write_f64(x).write_f64(y).write_f64(z).write_u8(flags);
    w.into_bytes()
}

// Serverbound Move Player Rot (nur Blickrichtung, KEINE Position geaendert) -- Feldlayout
// aus PlayerMoveC2SPacket.LookAndOnGround::write.
#[must_use] 
pub fn move_player_rot_payload(yaw_deg: f32, pitch_deg: f32, on_ground: bool, horizontal_collision: bool) -> Vec<u8> {
    let mut flags = 0u8;
    if on_ground {
        flags |= MOVE_FLAG_ON_GROUND;
    }
    if horizontal_collision {
        flags |= MOVE_FLAG_HORIZONTAL_COLLISION;
    }
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_MOVE_PLAYER_ROT).write_f32(yaw_deg).write_f32(pitch_deg).write_u8(flags);
    w.into_bytes()
}

// Serverbound Move Player Status Only (weder Position noch Blickrichtung geaendert, nur
// on_ground/horizontal_collision) -- Feldlayout aus PlayerMoveC2SPacket.OnGroundOnly::write.
#[must_use] 
pub fn move_player_status_only_payload(on_ground: bool, horizontal_collision: bool) -> Vec<u8> {
    let mut flags = 0u8;
    if on_ground {
        flags |= MOVE_FLAG_ON_GROUND;
    }
    if horizontal_collision {
        flags |= MOVE_FLAG_HORIZONTAL_COLLISION;
    }
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_MOVE_PLAYER_STATUS_ONLY).write_u8(flags);
    w.into_bytes()
}

// Serverbound Confirm Teleportation. Die Teleport-ID steht in 1.21.2+ als erstes Feld
// der clientbound Synchronize Player Position, kann also stabil als erste VarInt nach
// der Packet-ID gelesen werden -- unabhaengig vom restlichen Packet-Aufbau.
#[must_use] 
pub fn confirm_teleport_payload(teleport_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONFIRM_TELEPORT).write_varint(teleport_id);
    w.into_bytes()
}

// Serverbound Swing (0x3C): Hand als Enum-VarInt, 0 = Haupthand. Der Arm-Schwung, den
// andere Spieler sehen -- gehoert zu JEDEM Klick, ob er trifft oder nicht (Vanilla
// schickt ihn auch bei Luftschlaegen).
#[must_use] 
pub fn swing_payload() -> Vec<u8> {
    swing_hand_payload(false)
}

// Dasselbe mit waehlbarer Hand (1 = Nebenhand).
#[must_use] 
pub fn swing_hand_payload(off_hand: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SWING).write_varint(i32::from(off_hand));
    w.into_bytes()
}

// Serverbound Interact (0x19), Variante ATTACK: EntityId (VarInt), InteractType (VarInt,
// 1 = ATTACK laut Enum-Reihenfolge INTERACT/ATTACK/INTERACT_AT), Sneaking (Bool). ATTACK
// schreibt keine weiteren Felder (anders als INTERACT/INTERACT_AT, die Hand/Position
// anhaengen) -- Aufbau aus PlayerInteractEntityC2SPacket::write (nachgesehen, yarn).
#[must_use] 
pub fn attack_entity_payload(entity_id: i32, sneaking: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_INTERACT).write_varint(entity_id).write_varint(1).write_bool(sneaking);
    w.into_bytes()
}

// Serverbound Interact, Variante INTERACT (0): EntityId, Typ 0, Hand (VarInt), Sneaking --
// der Rechtsklick auf ein Entity (PlayerInteractEntityC2SPacket.interact).
#[must_use] 
pub fn interact_entity_payload(entity_id: i32, hand: i32, sneaking: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_INTERACT).write_varint(entity_id).write_varint(0).write_varint(hand).write_bool(sneaking);
    w.into_bytes()
}

// Serverbound Interact, Variante INTERACT_AT (2): EntityId, Typ 2, Trefferstelle relativ zur
// Entity-Position (3 x f32), Hand (VarInt), Sneaking (PlayerInteractEntityC2SPacket.interactAt,
// InteractAtHandler.write). Vanilla schickt es vor dem INTERACT (doItemUse, case ENTITY).
#[must_use] 
pub fn interact_at_entity_payload(entity_id: i32, at: (f32, f32, f32), hand: i32, sneaking: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_INTERACT).write_varint(entity_id).write_varint(2);
    w.write_f32(at.0).write_f32(at.1).write_f32(at.2);
    w.write_varint(hand).write_bool(sneaking);
    w.into_bytes()
}

#[must_use] 
pub fn keep_alive_play_payload(id: i64) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_KEEP_ALIVE).write_i64(id);
    w.into_bytes()
}
