// Packet-Definitionen fuer Handshaking/Status/Login-States, Protokoll-Version 774
// (Minecraft 1.21.11). Referenz: https://minecraft.wiki/w/Java_Edition_protocol/Packets

use crate::buffer::{PacketReader, PacketWriter, ReadError};
use crate::item::{read_optional_slot, ItemStack, SlotError};
use crate::nbt;
use crate::uuid::Uuid;

pub const PROTOCOL_VERSION: i32 = 774;

pub const INTENT_STATUS: i32 = 1;
pub const INTENT_LOGIN: i32 = 2;

// --- Handshaking state, serverbound, id 0x00 ---
pub struct Handshake {
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub intent: i32,
}

impl Handshake {
    pub fn to_payload(&self) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_varint(0x00)
            .write_varint(self.protocol_version)
            .write_string(&self.server_address)
            .write_u16(self.server_port)
            .write_varint(self.intent);
        w.into_bytes()
    }
}

// --- Status state ---

// Serverbound, id 0x00, keine Felder.
pub fn status_request_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x00);
    w.into_bytes()
}

// Serverbound, id 0x01, Payload = beliebiger i64 (Timestamp).
pub fn ping_request_payload(payload: i64) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x01).write_i64(payload);
    w.into_bytes()
}

// Clientbound, id 0x00.
pub struct StatusResponse {
    pub json: String,
}

impl StatusResponse {
    pub fn from_payload(payload: &[u8]) -> Result<Self, ReadError> {
        let mut r = PacketReader::new(payload);
        let _id = r.read_varint()?;
        Ok(Self { json: r.read_string()? })
    }
}

// --- Login state ---

// Serverbound, id 0x00.
pub struct LoginStart {
    pub name: String,
    pub uuid: Uuid,
}

impl LoginStart {
    pub fn to_payload(&self) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_varint(0x00).write_string(&self.name).write_uuid(self.uuid);
        w.into_bytes()
    }
}

// Clientbound, id 0x02.
pub struct LoginSuccess {
    pub uuid: Uuid,
    pub username: String,
}

impl LoginSuccess {
    pub fn from_payload(payload: &[u8]) -> Result<Self, ReadError> {
        let mut r = PacketReader::new(payload);
        let _id = r.read_varint()?;
        let uuid = r.read_uuid()?;
        let username = r.read_string()?;
        // Properties-Array (Texturen etc.) wird hier bewusst ignoriert -- fuer den
        // Offline-Mode-Login-Nachweis in Meilenstein 1 nicht relevant.
        Ok(Self { uuid, username })
    }
}

// Clientbound, id 0x01 (Login state) -- Online-Mode-Server verlangt Verschluesselung.
// server_id ist in modernen Versionen praktisch immer leer (Teil des Session-Hashes),
// public_key ist der DER-kodierte RSA-Public-Key (SubjectPublicKeyInfo), verify_token ein
// vom Server erzeugter Zufallswert, den wir verschluesselt zuruueckspiegeln.
pub struct EncryptionRequest {
    pub server_id: String,
    pub public_key: Vec<u8>,
    pub verify_token: Vec<u8>,
    // Seit 1.20.5: ob der Server die Mojang-Session-Authentifizierung erzwingt. Fuer den
    // Client-Ablauf unerheblich (wir authentifizieren immer, wenn Credentials vorliegen).
    pub should_authenticate: bool,
}

impl EncryptionRequest {
    pub fn from_payload(payload: &[u8]) -> Result<Self, ReadError> {
        let mut r = PacketReader::new(payload);
        let _id = r.read_varint()?;
        let server_id = r.read_string()?;
        let public_key = r.read_prefixed_bytes()?.to_vec();
        let verify_token = r.read_prefixed_bytes()?.to_vec();
        // should_authenticate ist optional (fehlt bei aelteren Servern) -- Default true.
        let should_authenticate = r.read_bool().unwrap_or(true);
        Ok(Self { server_id, public_key, verify_token, should_authenticate })
    }
}

// Serverbound, id 0x01 (Login state) -- Antwort auf EncryptionRequest: das mit dem
// Server-Public-Key (PKCS#1 v1.5) verschluesselte Shared Secret und Verify Token.
pub fn encryption_response_payload(encrypted_secret: &[u8], encrypted_verify_token: &[u8]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x01).write_bytes(encrypted_secret).write_bytes(encrypted_verify_token);
    w.into_bytes()
}

// Clientbound, id 0x00 (Login state) -- Server lehnt Login ab (z.B. Whitelist).
pub struct LoginDisconnect {
    pub reason_json: String,
}

impl LoginDisconnect {
    pub fn from_payload(payload: &[u8]) -> Result<Self, ReadError> {
        let mut r = PacketReader::new(payload);
        let _id = r.read_varint()?;
        Ok(Self { reason_json: r.read_string()? })
    }
}

// Clientbound, id 0x03 (Login state) -- ab hier sind Packets ggf. komprimiert.
pub struct SetCompression {
    pub threshold: i32,
}

impl SetCompression {
    pub fn from_payload(payload: &[u8]) -> Result<Self, ReadError> {
        let mut r = PacketReader::new(payload);
        let _id = r.read_varint()?;
        Ok(Self { threshold: r.read_varint()? })
    }
}

// Serverbound, id 0x03 (Login state) -- Bestaetigt Login Success, wechselt in den
// Configuration-State. Ohne dieses Packet joint der Spieler nie wirklich.
pub fn login_acknowledged_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x03);
    w.into_bytes()
}

// --- Configuration state ---

pub const CB_CONFIG_PLUGIN_MESSAGE: i32 = 0x01;
pub const CB_CONFIG_DISCONNECT: i32 = 0x02;
pub const CB_CONFIG_FINISH: i32 = 0x03;
pub const CB_CONFIG_KEEP_ALIVE: i32 = 0x04;
pub const CB_CONFIG_PING: i32 = 0x05;
pub const CB_CONFIG_KNOWN_PACKS: i32 = 0x0E;
// DynamicRegistriesS2CPacket. Per javap/Vineflower gegen ConfigurationStates (Fabric-Loom-
// Cache) verifiziert: Index 7 in der S2C-Registrierungsreihenfolge -- alle bereits bekannten
// Konstanten oben (0x01..0x05, 0x0E) stimmen exakt mit derselben Reihenfolge ueberein.
pub const CB_CONFIG_REGISTRY_DATA: i32 = 0x07;

// Angeforderte Sichtweite in Chunks. Der Server liefert effektiv min(dieser Wert,
// server.properties:view-distance) -- beide muessen also hoch stehen, damit weiter
// gerendert wird. 32 = Vanillas Hoechstwert. Wie weit tatsaechlich gezeichnet wird, begrenzt
// der Client selbst (Settings::max_render_distance). Erfordert Chunk-Entladen
// (CB_PLAY_FORGET_LEVEL_CHUNK), sonst wachsen Speicher und Draw-Aufwand beim Umherlaufen
// unbegrenzt.
pub const REQUESTED_VIEW_DISTANCE: u8 = 32;

// Locale, die der Client dem Server meldet (Einstellungen -> Sprache, Bereich Chat). Vanilla
// schickt die gewaehlte Sprache; manche Server uebersetzen ihre eigenen Texte danach.
static LOCALE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

pub fn set_locale(code: &str) {
    if let Ok(mut l) = LOCALE.write() {
        *l = code.to_string();
    }
}

fn locale() -> String {
    LOCALE.read().ok().filter(|l| !l.is_empty()).map(|l| l.clone()).unwrap_or_else(|| "en_us".to_string())
}

// Serverbound, id 0x00. Pflicht-Packet, das der Server im Configuration-State erwartet.
pub fn client_information_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x00)
        .write_string(&locale()) // Locale
        .write_u8(REQUESTED_VIEW_DISTANCE) // View Distance (Chunks)
        .write_varint(0) // Chat Mode: 0 = enabled
        .write_bool(true) // Chat Colors
        .write_u8(0x7F) // Displayed Skin Parts: alle an
        .write_varint(1) // Main Hand: 1 = rechts
        .write_bool(false) // Enable Text Filtering
        .write_bool(true) // Allow Server Listings
        .write_varint(0); // Particle Status: 0 = all
    w.into_bytes()
}

// Serverbound Plugin Message (Configuration-State, id 0x02): der Client-"Brand".
//
// Vanilla schickt das direkt nach der Client Information -- Kanal "minecraft:brand", Inhalt
// der Brand-String. Dieser Client hat es bisher gar nicht gesendet, was eine echte
// Abweichung von Vanilla ist: Server (und gerade Anticheat-Plugins) lesen den Brand aus, und
// eine Verbindung ganz ohne ist auffaellig bis unzulaessig.
pub fn brand_payload(brand: &str) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x02).write_string("minecraft:brand").write_string(brand);
    w.into_bytes()
}

// Was wir als Brand melden. Der Wert wird im `minecraft:brand`-Payload an den Server gesendet.
pub const CLIENT_BRAND: &str = "fabric";

// Serverbound, id 0x07. Antwort auf Clientbound Known Packs -- leeres Array bedeutet
// "kenne keine Packs", woraufhin der Server die vollen Registry-Daten schickt (die wir
// fuer den reinen Join-Nachweis ignorieren).
pub fn serverbound_known_packs_empty_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x07).write_varint(0); // Array-Laenge 0
    w.into_bytes()
}

// Serverbound, id 0x03. Bestaetigt Finish Configuration -> wechselt in den Play-State.
pub fn acknowledge_finish_configuration_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x03);
    w.into_bytes()
}

// Serverbound, id 0x04 (Configuration). Antwort auf Keep Alive.
pub fn keep_alive_config_payload(id: i64) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x04).write_i64(id);
    w.into_bytes()
}

// Serverbound, id 0x05 (Configuration). Antwort auf Ping.
pub fn pong_config_payload(id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x05).write_i32(id);
    w.into_bytes()
}

// Fuer die Gras-/Laub-/Wasser-Faerbung (Colormaps) relevante Felder eines Bioms. Die
// Position in der Liste, die read_registry_data() zurueckgibt, IST die numerische Biom-ID
// (Registrierungsreihenfolge = Registry-Index, wie bei Block-States) -- exakt der Wert, den
// die Biom-Palette im Chunk-Paket pro 4x4x4-Zelle traegt.
#[derive(Debug, Clone)]
pub struct BiomeInfo {
    pub name: String,
    pub temperature: f32,
    pub downfall: f32,
    // 0xRRGGBB, direkt aus effects.water_color -- Wasser wird (anders als Gras/Laub) nicht
    // ueber eine Colormap-Grafik, sondern direkt mit dieser Farbe eingefaerbt (s.
    // BiomeEffects).
    pub water_color: u32,
    // Biome.Weather.hasPrecipitation -- ob es hier ueberhaupt regnet/schneit (Wueste, Savanne,
    // Nether: nein). Gebraucht fuer die Regenpruefung beim Dreizack (s. world::weather).
    pub has_precipitation: bool,
}

// Registry Data (Configuration-State, 0x07/CB_CONFIG_REGISTRY_DATA): liest NUR die
// Biom-Registry ("minecraft:worldgen/biome") aus, alle anderen Registries (Damage Type,
// Enchantment, ...) werden uebersprungen (None). Format s. Modul-Kommentar an nbt.rs bzw.
// DynamicRegistriesS2CPacket/SerializedRegistryEntry (per javap verifiziert):
// Identifier (VarInt-String) Registry-Name, dann VarInt-Anzahl Eintraege, je Eintrag ein
// Identifier (Name, hier verworfen -- die Position IN der Liste ist die eigentliche ID) und
// ein Optional<NBT> (Bool-Flag, dann Netzwerk-NBT-Wurzel-Tag falls vorhanden).
// Eine Dimensionsart aus der Registry: wie tief ihre unterste Blockreihe liegt und wie hoch
// sie insgesamt ist. Genau diese beiden Zahlen entscheiden, auf welcher Welt-Hoehe die
// Sections eines Chunks landen (s. world::chunk::DEFAULT_MIN_Y).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionInfo {
    pub name: String,
    pub min_y: i32,
    pub height: i32,
}

// Die bekannten Vanilla-Werte, falls der Server einen Eintrag ohne Daten schickt (dann gilt
// das, was der Client von Haus aus kennt).
fn vanilla_dimension_defaults(name: &str) -> (i32, i32) {
    match name {
        "minecraft:the_nether" | "minecraft:the_end" => (0, 256),
        _ => (-64, 384),
    }
}

// Dieselbe Registry-Daten-Nachricht wie read_registry_data, nur fuer die Dimensionsarten.
// Die Reihenfolge ist bedeutsam: das Login- und das Respawn-Paket verweisen mit einem
// blossen Index in genau diese Liste (s. read_login_dimension_type).
pub fn read_registry_dimension_types(payload: &[u8]) -> Result<Option<Vec<DimensionInfo>>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    let count = r.read_varint()? as usize;
    if registry != "minecraft:dimension_type" {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let name = r.read_string()?;
        let has_data = r.read_bool()?;
        let data = if has_data { nbt::read_root(&mut r)? } else { None };
        let (vorgabe_min, vorgabe_hoehe) = vanilla_dimension_defaults(&name);
        let min_y = data.as_ref().and_then(|d| d.get("min_y")).and_then(|v| v.as_i32()).unwrap_or(vorgabe_min);
        let height = data.as_ref().and_then(|d| d.get("height")).and_then(|v| v.as_i32()).unwrap_or(vorgabe_hoehe);
        out.push(DimensionInfo { name, min_y, height });
    }
    Ok(Some(out))
}

// Der Index der Dimensionsart aus dem Login-Paket -- das erste Feld des
// CommonPlayerSpawnInfo-Blocks (s. read_spawn_info_dimension, das ihn ueberspringt).
pub fn read_login_dimension_type(payload: &[u8]) -> Result<i32, ReadError> {
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
    r.read_varint()
}

// Dasselbe aus dem Respawn-Paket: dort steht der Block direkt hinter der Packet-Id.
pub fn read_respawn_dimension_type(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

pub fn read_registry_data(payload: &[u8]) -> Result<Option<Vec<BiomeInfo>>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    let count = r.read_varint()? as usize;

    if registry != "minecraft:worldgen/biome" {
        return Ok(None);
    }

    let mut biomes = Vec::with_capacity(count);
    for _ in 0..count {
        let name = r.read_string()?;
        let has_data = r.read_bool()?;
        let data = if has_data { nbt::read_root(&mut r)? } else { None };

        // Fehlende/unerwartete Felder -> plausible Vanilla-Standardwerte (gemaessigtes
        // Klima, Standard-Wasserblau 0x3F76E4) statt Abbruch -- ein einzelnes kaputtes
        // Biom soll nicht die ganze Faerbung lahmlegen.
        let temperature = data.as_ref().and_then(|d| d.get("temperature")).and_then(|v| v.as_f32()).unwrap_or(0.8);
        let downfall = data.as_ref().and_then(|d| d.get("downfall")).and_then(|v| v.as_f32()).unwrap_or(0.4);
        let water_color = data
            .as_ref()
            .and_then(|d| d.get("effects"))
            .and_then(|e| e.get("water_color"))
            .and_then(|v| v.as_i32())
            .map(|v| v as u32)
            .unwrap_or(0x3F76E4);

        // Pflichtfeld in Vanillas Codec -- fehlt es trotzdem, lieber "regnet" als nie.
        let has_precipitation = data.as_ref().and_then(|d| d.get("has_precipitation")).and_then(|v| v.as_i32()).is_none_or(|v| v != 0);

        biomes.push(BiomeInfo { name, temperature, downfall, water_color, has_precipitation });
    }
    Ok(Some(biomes))
}

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

pub fn player_loaded_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_LOADED);
    w.into_bytes()
}

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
fn read_spawn_info_dimension(r: &mut PacketReader) -> Result<String, ReadError> {
    let _dimension_type = r.read_varint()?;
    r.read_string()
}

// Dimension und (gehashter) Seed aus demselben Block -- zusammen kennzeichnen sie eine Welt
// (s. chunk_saver::world_dir).
fn read_spawn_info_world(r: &mut PacketReader) -> Result<(String, i64), ReadError> {
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
fn read_spawn_info_game_mode(r: &mut PacketReader) -> Result<u8, ReadError> {
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

// System Chat (0x77): GameMessageS2CPacket -- Text-Component (Netzwerk-NBT) + bool overlay
// (Actionbar statt normaler Chat). Deckt Server-/Plugin-Broadcasts und Kommandoantworten ab
// (NICHT signierten Spieler-Chat -- das ist ein eigenes Paket, PLAYER_CHAT, hier nicht
// implementiert). ID direkt aus GameProtocols.CLIENTBOUND_TEMPLATE (echtes, Mojang-
// gemapptes Paper-1.21.11-Server-Jar) abgezaehlt -- dieselbe Quelle, die oben acht weitere
// IDs (davon zwei empirisch verifiziert: KEEP_ALIVE=0x2B, CHUNK_DATA=0x2C) exakt bestaetigt.
pub const CB_PLAY_SYSTEM_CHAT: i32 = 0x77;

pub fn read_system_chat(payload: &[u8]) -> Result<(String, bool), ReadError> {
    let (spans, overlay) = read_system_chat_spans(payload)?;
    Ok((spans.iter().map(|s| s.text.as_str()).collect::<String>(), overlay))
}

// Dasselbe Paket MIT seinen Farben. Der Chat ist der Ort, an dem Farbe Bedeutung traegt:
// Teamfarben, Warnungen, Serverpraefixe. Sie beim Lesen wegzuwerfen (wie es
// read_text_component tut) macht aus jeder Nachricht graue Einheitssuppe.
pub fn read_system_chat_spans(payload: &[u8]) -> Result<(Vec<crate::nbt::TextSpan>, bool), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let spans = read_text_spans(&mut r)?;
    let overlay = r.read_bool()?;
    Ok((spans, overlay))
}

// Ein Text-Component als farbige Abschnitte statt als nackter String.
fn read_text_spans(r: &mut PacketReader) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    Ok(nbt::read_root(r)?.as_ref().map(nbt::text_to_spans).unwrap_or_default())
}

// Grund eines Disconnects im Configuration- ODER Play-State. Beide Pakete tragen genau ein
// Text-Component -- dieselbe Kodierung wie System-Chat (anders als der Login-State, der
// noch JSON schickt, s. LoginDisconnect).
//
// Ohne das stand im Log nur "Server-Disconnect", und man musste raten, warum: fehlender
// Resourcepack, Anticheat, Whitelist, Versionskonflikt -- der Server SAGT es, wir haben es
// bloss weggeworfen.
pub fn read_disconnect_reason(payload: &[u8]) -> Result<String, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_text_component(&mut r)
}

// Ein Text-Component (Netzwerk-NBT, TextCodecs.UNLIMITED_REGISTRY_PACKET_CODEC) als
// Klartext -- dieselbe Kodierung fuer System-Chat, Actionbar, Titel/Untertitel und die
// Scoreboard-Texte. Farben/Formatierung werden (noch) verworfen, nur der reine Text bleibt.
// Wie read_text_component, aber mit den Farben als Paragraphen-Codes im Text (s.
// nbt::spans_to_legacy) -- fuer Scoreboard und Teams, deren Texte erst im Client zu einer
// Zeile zusammengesetzt werden. Ohne das kam das Scoreboard farblos an.
fn read_text_component_legacy(r: &mut PacketReader) -> Result<String, ReadError> {
    Ok(nbt::read_root(r)?.as_ref().map(|v| nbt::spans_to_legacy(&nbt::text_to_spans(v))).unwrap_or_default())
}

fn read_text_component(r: &mut PacketReader) -> Result<String, ReadError> {
    Ok(nbt::read_root(r)?.as_ref().map(nbt::text_to_plain_string).unwrap_or_default())
}

// --- Titel / Untertitel / Actionbar (IDs aus PlayStateFactories, gegen 6 bekannte Anker
// verifiziert: ADD_ENTITY 0x01, PLAYER_POSITION 0x46, ROTATE_HEAD 0x51, SET_HEALTH 0x66,
// SYSTEM_CHAT 0x77, TELEPORT_ENTITY 0x7B) --------------------------------------------------

// OverlayMessageS2CPacket: ein Text ueber der Hotbar (Actionbar). Manche Server nutzen dies,
// andere System-Chat mit overlay=true -- der Client behandelt beide gleich.
pub const CB_PLAY_SET_ACTION_BAR_TEXT: i32 = 0x55;
// TitleS2CPacket / SubtitleS2CPacket: je ein Text (gross in der Bildschirmmitte).
pub const CB_PLAY_SET_TITLE_TEXT: i32 = 0x70;
pub const CB_PLAY_SET_SUBTITLE_TEXT: i32 = 0x6E;
// TitleFadeS2CPacket: Einblend-/Halte-/Ausblendzeit in Ticks (je i32).
pub const CB_PLAY_SET_TITLES_ANIMATION: i32 = 0x71;
// ClearTitleS2CPacket: Titel ausblenden (bool reset = auch Zeiten zuruecksetzen).
pub const CB_PLAY_CLEAR_TITLES: i32 = 0x0E;

// Manche Server schicken die Actionbar als eigenes Paket statt als System-Chat mit
// overlay=true -- beide Wege muessen dieselben Farben liefern.
pub fn read_action_bar_spans(payload: &[u8]) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_text_spans(&mut r)
}

pub fn read_action_bar(payload: &[u8]) -> Result<String, ReadError> {
    Ok(read_action_bar_spans(payload)?.iter().map(|s| s.text.as_str()).collect())
}

// Titel und Untertitel MIT Farben, als Abschnitte wie Chat und Actionbar. Vorher als nackter
// Text gelesen -- ein farbiger Titel kam weiss an.
pub fn read_title_spans(payload: &[u8]) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_text_spans(&mut r)
}

pub fn read_subtitle_spans(payload: &[u8]) -> Result<Vec<crate::nbt::TextSpan>, ReadError> {
    read_title_spans(payload)
}

// (fadeIn, stay, fadeOut) in Ticks.
pub fn read_title_fade(payload: &[u8]) -> Result<(i32, i32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    Ok((r.read_i32()?, r.read_i32()?, r.read_i32()?))
}

// --- Erfahrung und Attribute (Rueckgrat der HUD-Leisten) ------------------------------------

// ExperienceBarUpdateS2CPacket (Index 101 in PlayStateFactories.S2C -- direkter Nachbar des
// bereits verifizierten SET_HEALTH 0x66 = Index 102).
pub const CB_PLAY_SET_EXPERIENCE: i32 = 0x65;

// Auf der Leitung: Fortschritt, STUFE, Gesamterfahrung -- am Lesecode des 1.21.11-Jars
// nachgesehen (ClientboundSetExperiencePacket(FriendlyByteBuf) liest experienceLevel vor
// totalExperience). Hier stand frueher Gesamt vor Stufe; die Leiste zeigte dann die
// Gesamtpunkte als Stufe.
//
// Rueckgabe: (Fortschritt 0..1 innerhalb der Stufe, Stufe, Gesamterfahrung).
pub fn read_experience(payload: &[u8]) -> Result<(f32, i32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let progress = r.read_f32()?;
    let level = r.read_varint()?;
    let total = r.read_varint()?;
    Ok((progress, level, total))
}

// ServerboundTeleportToEntityPacket (Index 61, direkt nach SWING 0x3C): Zuschauer springen zu
// einem Spieler (Zuschauer-Menue). Nur die UUID.
pub const SB_PLAY_TELEPORT_TO_ENTITY: i32 = 0x3D;

pub fn teleport_to_entity_payload(uuid: [u8; 16]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_TELEPORT_TO_ENTITY).write_raw_bytes(&uuid);
    w.into_bytes()
}

// ServerboundSetCreativeModeSlotPacket (Index 55, zwischen SET_COMMAND_MINECART und
// SET_JIGSAW_BLOCK; Nachbar der verifizierten SET_CARRIED_ITEM 0x34 und SWING 0x3C): Slot als
// short im Spielerinventar (ScreenHandler 0, -1 = aus dem Inventar werfen) und der Stapel in
// der Kreativ-Form (s. item::write_untrusted_slot). Nur im Kreativmodus; der Server
// uebernimmt den Stapel ohne Rueckmeldung.
pub const SB_PLAY_SET_CREATIVE_MODE_SLOT: i32 = 0x37;

pub fn set_creative_mode_slot_payload(slot: i16, stack: Option<&crate::item::ItemStack>) -> Option<Vec<u8>> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SET_CREATIVE_MODE_SLOT).write_u16(slot as u16);
    crate::item::write_untrusted_slot(&mut w, stack).ok()?;
    Some(w.into_bytes())
}

// ServerboundPickItemFromBlockPacket / ...FromEntityPacket (Index 35/36, direkt nach
// PADDLE_BOAT 0x22): der Mittelklick. Seit 1.21.4 sucht der SERVER das passende Item (im
// Kreativmodus legt er es neu an), der Client nennt nur den Block bzw. das Entity und ob
// die Daten mitkommen sollen (Kreativ mit Strg).
pub const SB_PLAY_PICK_ITEM_FROM_BLOCK: i32 = 0x23;
pub const SB_PLAY_PICK_ITEM_FROM_ENTITY: i32 = 0x24;

pub fn pick_item_from_block_payload(pos: (i32, i32, i32), include_data: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PICK_ITEM_FROM_BLOCK).write_i64(encode_block_pos(pos.0, pos.1, pos.2)).write_bool(include_data);
    w.into_bytes()
}

pub fn pick_item_from_entity_payload(entity_id: i32, include_data: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PICK_ITEM_FROM_ENTITY).write_varint(entity_id).write_bool(include_data);
    w.into_bytes()
}

// ClientboundSetCameraPacket (Index 91, zwischen SET_BORDER_WARNING_DISTANCE und
// SET_CHUNK_CACHE_CENTER; Nachbar der verifizierten SET_CURSOR_ITEM 0x5E): nur die
// Entity-ID, aus deren Augen der Client sieht. Zuschauer bekommen sie per Klick auf ein
// Entity, die eigene ID heisst "zurueck zu sich selbst".
pub const CB_PLAY_SET_CAMERA: i32 = 0x5B;

pub fn read_set_camera(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

// ClientboundPlayerAbilitiesPacket (Index 62, zwischen PLACE_GHOST_RECIPE und PLAYER_CHAT;
// Nachbar der verifizierten PING 0x3B und PLAYER_INFO_REMOVE 0x43). Auf der Leitung ein
// Flag-Byte (unverwundbar 1, fliegt 2, darf fliegen 4, sofort bauen 8) und die Flug- und
// Laufgeschwindigkeit als float.
pub const CB_PLAY_PLAYER_ABILITIES: i32 = 0x3E;
pub const ABILITY_INVULNERABLE: u8 = 1;
pub const ABILITY_FLYING: u8 = 2;
pub const ABILITY_MAY_FLY: u8 = 4;
pub const ABILITY_INSTABUILD: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerAbilities {
    pub flags: u8,
    pub fly_speed: f32,
    pub walk_speed: f32,
}

pub fn read_player_abilities(payload: &[u8]) -> Result<PlayerAbilities, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let flags = r.read_u8()?;
    let fly_speed = r.read_f32()?;
    let walk_speed = r.read_f32()?;
    Ok(PlayerAbilities { flags, fly_speed, walk_speed })
}

// ServerboundPlayerAbilitiesPacket (Index 39, direkt vor PLAYER_ACTION 0x28): nur das
// Flug-Bit, alles andere entscheidet der Server. Vanilla schickt es, wenn der Client den
// Flug selbst umschaltet (LocalPlayer.onUpdateAbilities).
pub const SB_PLAY_PLAYER_ABILITIES: i32 = 0x27;

pub fn player_abilities_payload(flying: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_ABILITIES).write_u8(if flying { ABILITY_FLYING } else { 0 });
    w.into_bytes()
}

// EntityAttributesS2CPacket (Index 129 in derselben Zaehlung).
pub const CB_PLAY_UPDATE_ATTRIBUTES: i32 = 0x81;

// Registry-Index von `minecraft:armor`. Attribute liegen in einer EINGEBAUTEN Registry (nicht
// datapack-erweiterbar), ihre IDs sind also die Registrierungsreihenfolge in
// EntityAttributes.class -- und dort steht `armor` als allererster register()-Aufruf.
// Uebertragen wird die nackte Roh-ID als VarInt: EntityAttribute.PACKET_CODEC ist
// PacketCodecs.registryEntry(RegistryKeys.ATTRIBUTE) und das ist die EINARGUMENTIGE Fassung
// (registry(..., Registry::getIndexedEntries)) ohne das "0 = folgt inline"-Schema der
// zweiargumentigen Variante -- hier gibt es also KEIN +1.
const ATTRIBUTE_ID_ARMOR: i32 = 0;
// `minecraft:max_health` -- Index 19 in derselben (alphabetisch registrierten) Reihenfolge.
// Bestimmt, WIE VIELE Herzen ueberhaupt gezeichnet werden; Server mit angehobener Maximal-
// gesundheit zeigen sonst zu wenige.
const ATTRIBUTE_ID_MAX_HEALTH: i32 = 19;
// Die drei Attribute der Abbaugeschwindigkeit (PlayerEntity.getBlockBreakingSpeed), in
// derselben Zaehlung: block_break_speed 5, mining_efficiency 20, submerged_mining_speed 29.
const ATTRIBUTE_ID_BLOCK_BREAK_SPEED: i32 = 5;
const ATTRIBUTE_ID_MINING_EFFICIENCY: i32 = 20;
const ATTRIBUTE_ID_SUBMERGED_MINING_SPEED: i32 = 29;
// Die drei BEWEGUNGS-Attribute in derselben Zaehlung. GrimAC liest bei jedem
// Bewegungs-Tick genau diese drei aus (MovementCheckRunner: MOVEMENT_SPEED,
// WATER_MOVEMENT_EFFICIENCY, SNEAKING_SPEED) -- wer sie ignoriert, rechnet ab der ersten
// Verzauberung, die sie setzt, eine andere Bewegung als der Server.
//   movement_speed             22  Grundtempo (Plugin-Walkspeed, Trank-Modifikatoren)
//   sneaking_speed             26  Schleichtempo, Grundwert 0.3 -- Schleichender Schritt hebt es an
//   water_movement_efficiency  32  Wassertempo, Grundwert 0.0 -- Wasserlaeufer hebt es an
const ATTRIBUTE_ID_MOVEMENT_SPEED: i32 = 22;
const ATTRIBUTE_ID_SNEAKING_SPEED: i32 = 26;
const ATTRIBUTE_ID_WATER_MOVEMENT_EFFICIENCY: i32 = 32;
// Die Reichweiten in derselben Zaehlung (am 1.21.11-Jar abgelesen: camera_distance und
// explosion_knockback_resistance stehen dazwischen). Der Server legt im Kreativmodus
// +0.5 bzw. +2.0 an (ServerPlayer.updatePlayerAttributes) und schickt sie hier mit.
const ATTRIBUTE_ID_BLOCK_INTERACTION_RANGE: i32 = 6;
const ATTRIBUTE_ID_ENTITY_INTERACTION_RANGE: i32 = 10;

// Modifikatoren auf MOVEMENT_SPEED, die der CLIENT selbst anlegt (LivingEntity/MobEffects,
// IDs am Jar geprueft) -- der Server schickt sie zusaetzlich im Attribut-Paket mit. Wir
// rechnen sie in der Physik ohnehin selbst (Sprint x1.3, Schnelligkeit/Langsamkeit, s.
// physics::effect_speed_factor), deshalb muessen sie hier UEBERSPRUNGEN werden, sonst
// zaehlen sie doppelt. Vanilla hat dasselbe Problem nicht, weil seine Modifikatoren nach ID
// in einer Map liegen und der doppelte Eintrag den ersten schlicht ersetzt.
const LOCAL_SPEED_MODIFIERS: [&str; 3] = ["minecraft:sprinting", "minecraft:effect.speed", "minecraft:effect.slowness"];

// EntityAttributeModifier.Operation, Reihenfolge der Enum-Konstanten.
const ATTRIBUTE_OP_ADD_VALUE: i32 = 0;
const ATTRIBUTE_OP_ADD_MULTIPLIED_BASE: i32 = 1;
const ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL: i32 = 2;

// Sicherheitsdeckel: ein kaputter Zaehler soll nicht in eine Endlosschleife laufen.
const MAX_ATTRIBUTE_ENTRIES: i32 = 128;
const MAX_ATTRIBUTE_MODIFIERS: i32 = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AttributeUpdate {
    pub entity_id: i32,
    // None = stand nicht in diesem Paket (der Server schickt nur geaenderte Attribute).
    pub armor: Option<f64>,
    pub max_health: Option<f64>,
    pub block_break_speed: Option<f64>,
    pub mining_efficiency: Option<f64>,
    pub submerged_mining_speed: Option<f64>,
    // OHNE die Modifikatoren aus LOCAL_SPEED_MODIFIERS -- die Physik legt sie selbst an.
    pub movement_speed: Option<f64>,
    pub sneaking_speed: Option<f64>,
    pub water_movement_efficiency: Option<f64>,
    pub block_interaction_range: Option<f64>,
    pub entity_interaction_range: Option<f64>,
}

// Ruestung und Maximalgesundheit aus dem Attribut-Paket; alles andere wird ueberlesen.
//
// Warum ueber das Attribut und nicht ueber die getragenen Teile: `PlayerEntity.getArmor()`
// IST in Vanilla nichts anderes als der Attributwert. Eine eigene Tabelle "Eisenharnisch = 6"
// waere eine zweite Wahrheit, die bei jedem Verzauberungs-/Plugin-Modifikator falsch liegt.
pub fn read_attributes(payload: &[u8]) -> Result<AttributeUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let count = r.read_varint()?;
    if !(0..=MAX_ATTRIBUTE_ENTRIES).contains(&count) {
        return Err(ReadError);
    }
    let mut out = AttributeUpdate { entity_id, ..Default::default() };
    for _ in 0..count {
        let attribute_id = r.read_varint()?;
        let base = r.read_f64()?;
        let modifier_count = r.read_varint()?;
        if !(0..=MAX_ATTRIBUTE_MODIFIERS).contains(&modifier_count) {
            return Err(ReadError);
        }
        // EntityAttributeInstance.computeValue(): erst alle ADD_VALUE auf den Grundwert,
        // dann ADD_MULTIPLIED_BASE auf DIESE Zwischensumme (jeweils mit dem Grundwert nach
        // Stufe 1 multipliziert), zuletzt ADD_MULTIPLIED_TOTAL multiplikativ.
        let mut sum = base;
        let mut multiplied_base = 0.0;
        let mut total_factor = 1.0;
        for _ in 0..modifier_count {
            let id = r.read_string()?;
            let amount = r.read_f64()?;
            let operation = r.read_varint()?;
            // s. LOCAL_SPEED_MODIFIERS: nur beim Tempo-Attribut, und nur fuer die IDs, die
            // die Physik selbst anlegt.
            if attribute_id == ATTRIBUTE_ID_MOVEMENT_SPEED && LOCAL_SPEED_MODIFIERS.contains(&id.as_str()) {
                continue;
            }
            match operation {
                ATTRIBUTE_OP_ADD_VALUE => sum += amount,
                ATTRIBUTE_OP_ADD_MULTIPLIED_BASE => multiplied_base += amount,
                ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL => total_factor *= 1.0 + amount,
                _ => {}
            }
        }
        let value = (sum + sum * multiplied_base) * total_factor;
        match attribute_id {
            ATTRIBUTE_ID_ARMOR => out.armor = Some(value),
            ATTRIBUTE_ID_MAX_HEALTH => out.max_health = Some(value),
            ATTRIBUTE_ID_BLOCK_BREAK_SPEED => out.block_break_speed = Some(value),
            ATTRIBUTE_ID_MINING_EFFICIENCY => out.mining_efficiency = Some(value),
            ATTRIBUTE_ID_SUBMERGED_MINING_SPEED => out.submerged_mining_speed = Some(value),
            ATTRIBUTE_ID_MOVEMENT_SPEED => out.movement_speed = Some(value),
            ATTRIBUTE_ID_SNEAKING_SPEED => out.sneaking_speed = Some(value),
            ATTRIBUTE_ID_WATER_MOVEMENT_EFFICIENCY => out.water_movement_efficiency = Some(value),
            ATTRIBUTE_ID_BLOCK_INTERACTION_RANGE => out.block_interaction_range = Some(value),
            ATTRIBUTE_ID_ENTITY_INTERACTION_RANGE => out.entity_interaction_range = Some(value),
            _ => {}
        }
    }
    Ok(out)
}

// --- Scoreboard ---------------------------------------------------------------------------

// ScoreboardDisplayS2CPacket: welcher Objective in welchem Slot angezeigt wird. Slot 1 =
// Seitenleiste (die uns interessiert), 0 = Spielerliste, 2 = unter dem Namen, 3..18 =
// teamgefaerbte Seitenleisten.
pub const CB_PLAY_SET_DISPLAY_OBJECTIVE: i32 = 0x60;
// ScoreboardObjectiveUpdateS2CPacket: Objective anlegen/entfernen/aktualisieren.
pub const CB_PLAY_SET_OBJECTIVE: i32 = 0x68;
// ScoreboardScoreUpdateS2CPacket: ein Zeilen-Score setzen.
pub const CB_PLAY_SET_SCORE: i32 = 0x6C;
// ScoreboardScoreResetS2CPacket: einen Score-Eintrag entfernen.
pub const CB_PLAY_RESET_SCORE: i32 = 0x4D;

// (slot, objective_name)
pub fn read_display_objective(payload: &[u8]) -> Result<(i32, String), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let slot = r.read_varint()?;
    let name = r.read_string()?;
    Ok((slot, name))
}

pub struct ObjectiveUpdate {
    pub name: String,
    // 0 = anlegen, 1 = entfernen, 2 = aktualisieren.
    pub mode: u8,
    // Anzeigename (nur bei anlegen/aktualisieren) -- der Titel der Seitenleiste.
    pub display: Option<String>,
    // ScoreboardCriterion.RenderType: HEARTS (1) statt INTEGER (0). Die Spielerliste zeichnet
    // dann Herzen statt einer Zahl.
    pub hearts: bool,
    // Das Zahlenformat des Objectives (gilt fuer jeden Score ohne eigenes).
    pub number_format: Option<NumberFormat>,
}

// NumberFormatTypes, Registrierungsreihenfolge blank/styled/fixed (am 1.21.11-Jar
// abgelesen) -- so, wie ein Score neben dem Namen erscheint.
#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    // Gar nichts anzeigen.
    Blank,
    // Die Zahl, in dieser Farbe (None = Standardfarbe des Aufrufers).
    Styled(Option<u32>),
    // Ein fester Text statt der Zahl.
    Fixed(Vec<crate::nbt::TextSpan>),
}

// NumberFormatTypes.OPTIONAL_PACKET_CODEC: Bool + VarInt-Typ + Inhalt.
fn read_optional_number_format(r: &mut PacketReader) -> Result<Option<NumberFormat>, ReadError> {
    if !r.read_bool()? {
        return Ok(None);
    }
    Ok(Some(match r.read_varint()? {
        0 => NumberFormat::Blank,
        // StyledNumberFormat: ein Style als NBT-Compound.
        1 => NumberFormat::Styled(nbt::read_root(r)?.as_ref().and_then(nbt::style_color)),
        2 => NumberFormat::Fixed(read_text_spans(r)?),
        _ => return Err(ReadError),
    }))
}

pub fn read_objective(payload: &[u8]) -> Result<ObjectiveUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let name = r.read_string()?;
    let mode = r.read_u8()?;
    // Bei anlegen(0)/aktualisieren(2) folgen Anzeigename + RenderType + Optional<NumberFormat>.
    let (mut display, mut hearts, mut number_format) = (None, false, None);
    if mode == 0 || mode == 2 {
        display = Some(read_text_component_legacy(&mut r)?);
        hearts = r.read_varint()? == 1;
        number_format = read_optional_number_format(&mut r)?;
    }
    Ok(ObjectiveUpdate { name, mode, display, hearts, number_format })
}

pub struct ScoreUpdate {
    pub holder: String,
    pub objective: String,
    pub score: i32,
    // Optionaler Anzeige-Text der Zeile (seit 1.20.3); ohne = der Holder-Name wird angezeigt.
    pub display: Option<String>,
    // Eigenes Zahlenformat dieses Scores (vor dem des Objectives).
    pub number_format: Option<NumberFormat>,
}

pub fn read_score(payload: &[u8]) -> Result<ScoreUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let holder = r.read_string()?;
    let objective = r.read_string()?;
    let score = r.read_varint()?;
    // Optional<Text> display (bool praesent + ggf. Text); Optional<NumberFormat> danach ignoriert.
    let display = if r.read_bool()? { Some(read_text_component_legacy(&mut r)?) } else { None };
    let number_format = read_optional_number_format(&mut r)?;
    Ok(ScoreUpdate { holder, objective, score, display, number_format })
}

// TeamS2CPacket (SET_PLAYER_TEAM, Index 107 in PlayStateFactories.S2C -- direkt zwischen den
// bereits verifizierten SET_OBJECTIVE 0x68 (104) und SET_SCORE 0x6C (108)). Viele Server bauen
// ihre Seitenleiste aus Dummy-Holdern ("0".."15") und schreiben den sichtbaren Text in
// Prefix/Suffix eines Teams, dem der Holder angehoert -- ohne dieses Paket zeigt die
// Seitenleiste nur die nackten Holder-Namen.
pub const CB_PLAY_SET_PLAYER_TEAM: i32 = 0x6B;

pub struct TeamUpdate {
    pub name: String,
    // 0 = anlegen (mit Team-Info UND Mitgliedern), 1 = entfernen, 2 = Info aktualisieren,
    // 3 = Mitglieder hinzufuegen, 4 = Mitglieder entfernen.
    pub mode: u8,
    // Nur bei 0/2 vorhanden (SerializableTeam).
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    // AbstractTeam.VisibilityRule als Index: 0 ALWAYS, 1 NEVER, 2 HIDE_FOR_OTHER_TEAMS,
    // 3 HIDE_FOR_OWN_TEAM (die Reihenfolge steht so im Enum, der Codec ist PacketCodecs
    // .indexed). Daran haengt, ob ueber einem Spieler ueberhaupt ein Name stehen darf --
    // Server mit eigenen Rang-Hologrammen stellen ihn auf NEVER.
    pub name_tag_visibility: Option<u8>,
    // Formatting-Index der Teamfarbe (dieselben 16 Codes wie die Paragraphen-Farben);
    // Werte ausserhalb 0..15 heissen "keine Farbe" (Vanilla: RESET).
    pub color: Option<i32>,
    // AbstractTeam.CollisionRule als Index: 0 ALWAYS, 1 NEVER, 2 und 3 je nach Teamzugehoerigkeit
    // (s. app::team_allows_push). Entscheidet, ob Entities einander schubsen.
    pub collision_rule: Option<u8>,
    // Nur bei 0/3/4 vorhanden.
    pub entries: Vec<String>,
    // Anzeigename (Legacy-Codes), nur bei 0/2 -- das Zuschauer-Menue nennt Teams damit.
    pub display_name: Option<String>,
}

pub fn read_team(payload: &[u8]) -> Result<TeamUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let name = r.read_string()?;
    let mode = r.read_u8()?;
    let (mut prefix, mut suffix) = (None, None);
    // SerializableTeam: Text displayName, Byte friendlyFlags, VarInt nameTagVisibility,
    // VarInt collisionRule (beide PacketCodecs.indexed, seit 1.21.5 keine Strings mehr),
    // VarInt color (writeEnumConstant), Text prefix, Text suffix.
    let (mut name_tag_visibility, mut color, mut collision_rule, mut display_name) = (None, None, None, None);
    if mode == 0 || mode == 2 {
        display_name = Some(read_text_component_legacy(&mut r)?);
        let _friendly_flags = r.read_u8()?;
        name_tag_visibility = Some(r.read_varint()?.clamp(0, 255) as u8);
        collision_rule = Some(r.read_varint()?.clamp(0, 255) as u8);
        color = Some(r.read_varint()?);
        prefix = Some(read_text_component_legacy(&mut r)?);
        suffix = Some(read_text_component_legacy(&mut r)?);
    }
    let mut entries = Vec::new();
    if mode == 0 || mode == 3 || mode == 4 {
        let count = r.read_varint()?.max(0);
        entries.reserve(count.min(256) as usize);
        for _ in 0..count {
            entries.push(r.read_string()?);
        }
    }
    Ok(TeamUpdate { name, mode, prefix, suffix, name_tag_visibility, color, collision_rule, entries, display_name })
}

// (holder, optional objective_name -- None = aus allen Objectives entfernen)
pub fn read_reset_score(payload: &[u8]) -> Result<(String, Option<String>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let holder = r.read_string()?;
    let objective = if r.read_bool()? { Some(r.read_string()?) } else { None };
    Ok((holder, objective))
}

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
pub fn player_action_block_payload(action: i32, x: i32, y: i32, z: i32, face: i32, sequence: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_ACTION)
        .write_varint(action)
        .write_i64(encode_block_pos(x, y, z))
        .write_u8(face as u8)
        .write_varint(sequence);
    w.into_bytes()
}

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
pub fn use_item_payload(hand: i32, sequence: i32, yaw_deg: f32, pitch_deg: f32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_USE_ITEM).write_varint(hand).write_varint(sequence).write_f32(yaw_deg).write_f32(pitch_deg);
    w.into_bytes()
}

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
const PLAYER_INPUT_FORWARD: u8 = 1;
const PLAYER_INPUT_BACKWARD: u8 = 2;
const PLAYER_INPUT_LEFT: u8 = 4;
const PLAYER_INPUT_RIGHT: u8 = 8;
const PLAYER_INPUT_JUMP: u8 = 16;
const PLAYER_INPUT_SNEAK: u8 = 32;
const PLAYER_INPUT_SPRINT: u8 = 64;

// Player Input Payload: EIN Byte Bitflags -- nach PlayerInput.PACKET_CODEC::encode des
// echten Clients (net.minecraft.util.PlayerInput).
#[allow(clippy::too_many_arguments)]
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
const PLAYER_COMMAND_MODE_STOP_SLEEPING: i32 = 0;
const PLAYER_COMMAND_MODE_START_SPRINTING: i32 = 1;
const PLAYER_COMMAND_MODE_STOP_SPRINTING: i32 = 2;
const PLAYER_COMMAND_MODE_START_FALL_FLYING: i32 = 6;

// Aufstehen: ClientCommandC2SPacket mit Mode STOP_SLEEPING (0). Genau das schickt Vanilla,
// wenn man im Schlaf-Bildschirm "Bett verlassen" waehlt -- ein eigenes Aufwach-Paket gibt es
// nicht, und ohne dieses Paket bleibt der Server der Meinung, wir liegen weiter im Bett.
pub fn player_command_stop_sleeping_payload(entity_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_COMMAND).write_varint(entity_id).write_varint(PLAYER_COMMAND_MODE_STOP_SLEEPING).write_varint(0);
    w.into_bytes()
}

// Elytra-Flug beginnen: ClientPlayerEntity.tickMovement schickt das, nachdem es den Flug
// lokal schon gestartet hat (s. physics::PlayerPhysics::gliding).
pub fn player_command_start_fall_flying_payload(entity_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_COMMAND).write_varint(entity_id).write_varint(PLAYER_COMMAND_MODE_START_FALL_FLYING).write_varint(0);
    w.into_bytes()
}

pub fn player_command_sprint_payload(entity_id: i32, sprinting: bool) -> Vec<u8> {
    let mode = if sprinting { PLAYER_COMMAND_MODE_START_SPRINTING } else { PLAYER_COMMAND_MODE_STOP_SPRINTING };
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_COMMAND).write_varint(entity_id).write_varint(mode).write_varint(0);
    w.into_bytes()
}

const MOVE_FLAG_ON_GROUND: u8 = 1;
const MOVE_FLAG_HORIZONTAL_COLLISION: u8 = 2;

// Serverbound Move Player Pos+Rot: X/Y/Z (Double), Yaw/Pitch (Float, Grad), Flags (Byte) --
// Feldreihenfolge nach ServerboundMovePlayerPacket.PosRot::write des echten Clients.
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
pub fn confirm_teleport_payload(teleport_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONFIRM_TELEPORT).write_varint(teleport_id);
    w.into_bytes()
}

// Serverbound Swing (0x3C): Hand als Enum-VarInt, 0 = Haupthand. Der Arm-Schwung, den
// andere Spieler sehen -- gehoert zu JEDEM Klick, ob er trifft oder nicht (Vanilla
// schickt ihn auch bei Luftschlaegen).
pub fn swing_payload() -> Vec<u8> {
    swing_hand_payload(false)
}

// Dasselbe mit waehlbarer Hand (1 = Nebenhand).
pub fn swing_hand_payload(off_hand: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SWING).write_varint(off_hand as i32);
    w.into_bytes()
}

// Serverbound Interact (0x19), Variante ATTACK: EntityId (VarInt), InteractType (VarInt,
// 1 = ATTACK laut Enum-Reihenfolge INTERACT/ATTACK/INTERACT_AT), Sneaking (Bool). ATTACK
// schreibt keine weiteren Felder (anders als INTERACT/INTERACT_AT, die Hand/Position
// anhaengen) -- Aufbau aus PlayerInteractEntityC2SPacket::write (nachgesehen, yarn).
pub fn attack_entity_payload(entity_id: i32, sneaking: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_INTERACT).write_varint(entity_id).write_varint(1).write_bool(sneaking);
    w.into_bytes()
}

// Serverbound Interact, Variante INTERACT (0): EntityId, Typ 0, Hand (VarInt), Sneaking --
// der Rechtsklick auf ein Entity (PlayerInteractEntityC2SPacket.interact).
pub fn interact_entity_payload(entity_id: i32, hand: i32, sneaking: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_INTERACT).write_varint(entity_id).write_varint(0).write_varint(hand).write_bool(sneaking);
    w.into_bytes()
}

// Serverbound Interact, Variante INTERACT_AT (2): EntityId, Typ 2, Trefferstelle relativ zur
// Entity-Position (3 x f32), Hand (VarInt), Sneaking (PlayerInteractEntityC2SPacket.interactAt,
// InteractAtHandler.write). Vanilla schickt es vor dem INTERACT (doItemUse, case ENTITY).
pub fn interact_at_entity_payload(entity_id: i32, at: (f32, f32, f32), hand: i32, sneaking: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_INTERACT).write_varint(entity_id).write_varint(2);
    w.write_f32(at.0).write_f32(at.1).write_f32(at.2);
    w.write_varint(hand).write_bool(sneaking);
    w.into_bytes()
}

pub fn keep_alive_play_payload(id: i64) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_KEEP_ALIVE).write_i64(id);
    w.into_bytes()
}

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
#[derive(Clone, Copy, Debug, PartialEq)]
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
pub fn decode_block_pos(packed: i64) -> (i32, i32, i32) {
    let x = (packed >> 38) as i32;
    let y = ((packed << 52) >> 52) as i32;
    let z = ((packed << 26) >> 38) as i32;
    (x, y, z)
}

pub fn encode_block_pos(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & 0x3FF_FFFF) << 38) | ((z as i64 & 0x3FF_FFFF) << 12) | (y as i64 & 0xFFF)
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
fn angle_byte_to_deg(b: u8) -> f32 {
    b as i8 as f32 * (360.0 / 256.0)
}

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
    let dx = r.read_i16()? as f64 / 4096.0;
    let dy = r.read_i16()? as f64 / 4096.0;
    let dz = r.read_i16()? as f64 / 4096.0;
    Ok((entity_id, dx, dy, dz))
}

// Move Entity Pos+Rot (0x34): wie 0x33, plus Yaw/Pitch (je 1 Byte Winkel) vor OnGround.
pub fn read_move_entity_pos_rot(payload: &[u8]) -> Result<(i32, f64, f64, f64, f32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let dx = r.read_i16()? as f64 / 4096.0;
    let dy = r.read_i16()? as f64 / 4096.0;
    let dz = r.read_i16()? as f64 / 4096.0;
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
// HungerManager.canSprint(), und das ist `getFoodLevel() > 6`.
pub fn read_set_health(payload: &[u8]) -> Result<(f32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let health = r.read_f32()?;
    let food = r.read_varint()?;
    let _saturation = r.read_f32()?;
    Ok((health, food))
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
    let i = r.read_u8()? as u64;
    if i == 0 {
        return Ok((0.0, 0.0, 0.0));
    }
    let j = r.read_u8()? as u64;
    let l = r.read_i32()? as u32 as u64;
    let m = (l << 16) | (j << 8) | i;
    let mut betrag = i & 3;
    // Bit 2 gesetzt: der Betrag passt nicht in zwei Bits und steht als VarInt dahinter.
    if i & 4 == 4 {
        betrag |= (r.read_varint()? as u32 as u64) << 2;
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
/// Beide Zeiten sind LONGS, keine VarInts. `tick_day_time` ist neu ab 1.21.9 -- vorher zeigte
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

#[derive(Clone, Copy, Debug, PartialEq)]
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
// Effekte mit Wirkung auf die eigene Bewegung (s. app physics::Input).
pub const MOB_EFFECT_JUMP_BOOST: i32 = 7;
pub const MOB_EFFECT_LEVITATION: i32 = 24;
pub const MOB_EFFECT_SLOW_FALLING: i32 = 27;
pub const MOB_EFFECT_DOLPHINS_GRACE: i32 = 29;

/// Alle Statuseffekte in REGISTRY-REIHENFOLGE -- der Index IST die Zahl auf der Leitung.
///
/// Aus dem statischen Initialisierer von StatusEffects gelesen (maschinell, nicht
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
pub fn mob_effect_name(id: i32) -> String {
    usize::try_from(id).ok().and_then(|i| MOB_EFFECTS.get(i)).map_or_else(|| format!("#{id}"), |name| (*name).to_string())
}

/// Eine Dauer von -1 heisst UNENDLICH (StatusEffectInstance.INFINITE) -- ein Beacon oder ein
/// Kommando kann das schicken. Wer sie als gewoehnliche Zahl herunterzaehlt, laesst den Effekt
/// im naechsten Tick ablaufen.
pub const MOB_EFFECT_INFINITE: i32 = -1;

/// Update Mob Effect: Entity-Id, Effekt, Staerke, Dauer und das Flag-Byte (s. MOB_EFFECT_FLAG_*).
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

// Ein Eintrag des Pakets: Slot und (falls belegt) der Stapel darin. None = Slot leer, das
// ist die normale Art, ein Item aus der Hand verschwinden zu lassen.
#[derive(Clone, Debug, PartialEq)]
pub struct EquipmentEntry {
    pub slot: u8,
    pub item: Option<ItemStack>,
}

// Sicherheitsdeckel gegen ein absichtlich endloses Fortsetzungsbit: mehr als alle acht
// Slots kann ein ehrlicher Server nicht schicken, ein paar Reserve schadet aber nicht.
const MAX_EQUIPMENT_ENTRIES: usize = 16;

pub fn read_set_equipment(payload: &[u8]) -> Result<(i32, Vec<EquipmentEntry>), SlotError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let mut entries = Vec::new();
    loop {
        let raw = r.read_u8()?;
        entries.push(EquipmentEntry { slot: raw & 0x7F, item: read_optional_slot(&mut r)? });
        if raw & 0x80 == 0 {
            return Ok((entity_id, entries));
        }
        if entries.len() >= MAX_EQUIPMENT_ENTRIES {
            return Err(SlotError::Read);
        }
    }
}

// Set Entity Data (0x61): der Metadaten-Strom einer Entity. Aufbau (EntityTrackerUpdate-
// S2CPacket / DataTracker.SerializedEntry): VarInt entityId, dann beliebig viele Eintraege
// aus Index (u8), Serializer-ID (VarInt) und Wert, beendet durch den Index 0xFF. Der Server
// schickt nur GEAENDERTE Felder, nicht den ganzen Satz.
//
// ID aus der vollstaendig gegen PlayStateFactories abgezaehlten Clientbound-Tabelle -- die
// direkten Nachbarn Set Entity Link 0x62, Set Entity Motion 0x63, Set Equipment 0x64 und Set
// Health 0x66 waren bereits verifiziert und werden exakt getroffen.
pub const CB_PLAY_SET_ENTITY_DATA: i32 = 0x61;

// Feld-Indizes aus Entity.class (1.21.11 yarn, Reihenfolge der registerData-Aufrufe):
// 0 FLAGS (Byte), 1 AIR (Int), 2 CUSTOM_NAME (Optional<Text>), 3 NAME_VISIBLE (Bool),
// 4 SILENT (Bool), 5 NO_GRAVITY (Bool), 6 POSE (EntityPose), 7 FROZEN_TICKS (Int).
const ENTITY_DATA_INDEX_FLAGS: u8 = 0;
// Entity.AIR (Int): Restluft in Ticks, 300 = voll (Entity.getMaxAir). Fuer die Blasenleiste
// im HUD -- der Server schickt den Wert nur, wenn er sich aendert.
const ENTITY_DATA_INDEX_AIR: u8 = 1;
const ENTITY_DATA_INDEX_POSE: u8 = 6;
// LivingEntity.SLEEPING_POSITION. Der Index folgt der REIHENFOLGE DER STATISCHEN
// registerData-Felder (nicht der von initDataTracker!): Entity belegt 0..7, LivingEntity
// deklariert dann LIVING_FLAGS(8), HEALTH(9), POTION_SWIRLS(10), POTION_SWIRLS_AMBIENT(11),
// STUCK_ARROW_COUNT(12), STINGER_COUNT(13), SLEEPING_POSITION(14).
// Genau dieses Feld IST der Schlafzustand: LivingEntity.isSleeping() ist nichts anderes als
// `getSleepingPosition().isPresent()` -- ein eigenes Schlaf-Paket gibt es nicht mehr.
const ENTITY_DATA_INDEX_SLEEPING_POS: u8 = 14;
// LivingEntity.POTION_SWIRLS / POTION_SWIRLS_AMBIENT, s. EntityDataUpdate::potion_swirls.
const ENTITY_DATA_INDEX_POTION_SWIRLS: u8 = 10;
const ENTITY_DATA_INDEX_POTION_SWIRLS_AMBIENT: u8 = 11;
// PassiveEntity.CHILD und ZombieEntity.BABY: das erste eigene Feld nach MobEntity (Index 15),
// beide ein Boolean. Andere Typen haben an derselben Stelle andere Booleans (Guardian, Raider,
// Ghast, ...) -- welcher Typ gemeint ist, entscheidet der Empfaenger (s. world::can_be_baby).
const AGEABLE_INDEX_BABY: u8 = 16;
// Entity.CUSTOM_NAME (Optional<Text>) und Entity.NAME_VISIBLE (Bool). Zusammen sind das die
// klassischen "Hologramme": ein unsichtbarer Armorstand, dessen Namensschild dauerhaft
// eingeblendet ist (Entity.shouldRenderName() == isCustomNameVisible()). Dieselben zwei
// Felder tragen auch den Namen eines umbenannten Mobs.
const ENTITY_DATA_INDEX_CUSTOM_NAME: u8 = 2;
const ENTITY_DATA_INDEX_NAME_VISIBLE: u8 = 3;

// ArmorStandEntity.ARMOR_STAND_FLAGS -- erstes eigenes Feld nach LivingEntity (0..14), also
// Index 15. Nur zwei seiner Bits interessieren, aber die entscheiden ueber die HOEHE des
// Standes und damit darueber, wo sein Namensschild sitzt: MARKER ist 0x0 gross, SMALL die
// halbe Groesse, sonst 0.5 x 1.975 (ArmorStandEntity.MARKER_DIMENSIONS/SMALL_DIMENSIONS).
const ARMOR_STAND_INDEX_FLAGS: u8 = 15;
pub const ARMOR_STAND_FLAG_SMALL: u8 = 1;
pub const ARMOR_STAND_FLAG_MARKER: u8 = 16;
// Beide Namen meinen denselben Index (s. den Byte-Zweig in read_set_entity_data). Hier
// festgeschrieben, damit ein Versionswechsel, der einen der beiden verschiebt, den Bau
// abbricht -- statt still das falsche Feld zu fuellen.
const _: () = assert!(ARMOR_STAND_INDEX_FLAGS == DISPLAY_INDEX_BILLBOARD);
pub const ARMOR_STAND_FLAG_SHOW_ARMS: u8 = 4;
pub const ARMOR_STAND_FLAG_NO_BASEPLATE: u8 = 8;
// Die sechs Gelenkwinkel (Rotations, Grad) danach: Kopf, Rumpf, linker Arm, rechter Arm, linkes
// Bein, rechtes Bein (ArmorStand.DATA_HEAD_POSE .. DATA_RIGHT_LEG_POSE). Andere Typen fuehren an
// diesen Indizes andere Serializer -- ROTATION gibt es nur beim Armorstand.
const ARMOR_STAND_INDEX_HEAD_POSE: u8 = 16;
const ARMOR_STAND_POSE_COUNT: u8 = 6;

// --- DisplayEntity (1.21.11 yarn, net/minecraft/entity/decoration/DisplayEntity.class) -----
//
// Dieselbe Zaehlregel wie oben: Entity belegt 0..7, DisplayEntity deklariert danach in
// STATISCHER Feldreihenfolge START_INTERPOLATION(8), INTERPOLATION_DURATION(9),
// TELEPORT_DURATION(10), TRANSLATION(11), SCALE(12), LEFT_ROTATION(13), RIGHT_ROTATION(14),
// BILLBOARD(15), BRIGHTNESS(16), VIEW_RANGE(17), SHADOW_RADIUS(18), SHADOW_STRENGTH(19),
// WIDTH(20), HEIGHT(21), GLOW_COLOR_OVERRIDE(22).
//
// ACHTUNG, haeufige Fehlerquelle: `initDataTracker` listet die Felder in einer ANDEREN
// Reihenfolge (TELEPORT_DURATION zuerst). Massgeblich ist die Reihenfolge der
// registerData-Aufrufe, nicht die des Builders.
const DISPLAY_INDEX_START_INTERPOLATION: u8 = 8;
const DISPLAY_INDEX_INTERPOLATION_DURATION: u8 = 9;
const DISPLAY_INDEX_TRANSLATION: u8 = 11;
const DISPLAY_INDEX_SCALE: u8 = 12;
const DISPLAY_INDEX_LEFT_ROTATION: u8 = 13;
const DISPLAY_INDEX_RIGHT_ROTATION: u8 = 14;
const DISPLAY_INDEX_BILLBOARD: u8 = 15;
const DISPLAY_INDEX_VIEW_RANGE: u8 = 17;
// PlayerEntity.ABSORPTION_AMOUNT (Float). LivingEntity belegt bis 14, PlayerLikeEntity 15
// (MAIN_ARM_ID) und 16 (PLAYER_MODE_CUSTOMIZATION_ID), PlayerEntity beginnt mit diesem Feld
// (Reihenfolge der statischen registerData-Aufrufe im Jar). Derselbe Index ist bei einem
// Display die Sichtweite -- welches Feld gemeint ist, weiss erst der Empfaenger.
const PLAYER_INDEX_ABSORPTION: u8 = 17;

// Die drei Unterklassen setzen alle bei 23 auf. Welches Feld dort steht, haengt am
// Entity-Typ -- die drei sind aber an ihrem SERIALIZER unterscheidbar (Text vs. ItemStack
// vs. BlockState), deshalb kommt dieser Parser ohne den Typ aus und der Aufrufer entscheidet
// anhand von `world::entity_type_name`, was er davon glaubt.
//   TextDisplayEntity:  TEXT(23), LINE_WIDTH(24), BACKGROUND(25), TEXT_OPACITY(26), FLAGS(27)
//   ItemDisplayEntity:  ITEM(23), ITEM_DISPLAY(24)
//   BlockDisplayEntity: BLOCK_STATE(23)
const DISPLAY_INDEX_CONTENT: u8 = 23;
// ItemDisplayEntity.ITEM_DISPLAY: der ItemDisplayContext als Byte (0 NONE, 1/2 dritte Person
// links/rechts, 3/4 Ego links/rechts, 5 HEAD, 6 GUI, 7 GROUND, 8 FIXED, 9 ON_SHELF). An
// Index 24 steht beim Text-Display die Zeilenbreite -- ein INT, also unverwechselbar.
const ITEM_DISPLAY_INDEX_MODE: u8 = 24;
// Der getragene Stack an Index 8: ItemEntity.STACK, ThrownItemEntity.ITEM (Ei, Schneeball,
// Enderperle, Traenke, Erfahrungsflasche), EyeOfEnderEntity.ITEM, AbstractFireballEntity.ITEM
// und FireworkRocketEntity.ITEM.
const ENTITY_INDEX_ITEM: u8 = 8;
// Gegenstandsrahmen: HangingEntity belegt 8 mit DATA_DIRECTION (seit 1.21.2), ItemFrame setzt
// mit DATA_ITEM (9) und DATA_ROTATION (10) dahinter auf. Frueher hier als 8 und 9 gelesen --
// dann kam bei keinem Rahmen je ein Inhalt an.
const HANGING_INDEX_DIRECTION: u8 = 8;
const ITEM_FRAME_INDEX_ITEM: u8 = 9;
const ITEM_FRAME_INDEX_ROTATION: u8 = 10;
const TEXT_DISPLAY_INDEX_LINE_WIDTH: u8 = 24;
const TEXT_DISPLAY_INDEX_BACKGROUND: u8 = 25;
const TEXT_DISPLAY_INDEX_TEXT_OPACITY: u8 = 26;
const TEXT_DISPLAY_INDEX_FLAGS: u8 = 27;

// TextDisplayEntity-Flags (dieselbe Klasse): Schatten, Durchsehbarkeit, Standard-Hintergrund
// und die beiden Ausrichtungs-Bits.
pub const TEXT_DISPLAY_FLAG_SHADOW: u8 = 1;
pub const TEXT_DISPLAY_FLAG_SEE_THROUGH: u8 = 2;
pub const TEXT_DISPLAY_FLAG_DEFAULT_BACKGROUND: u8 = 4;
pub const TEXT_DISPLAY_FLAG_ALIGN_LEFT: u8 = 8;
pub const TEXT_DISPLAY_FLAG_ALIGN_RIGHT: u8 = 16;
// TextDisplayEntity.INITIAL_BACKGROUND = 0x40000000: 25 % Schwarz, dasselbe wie hinter einem
// gewoehnlichen Namensschild.
pub const TEXT_DISPLAY_INITIAL_BACKGROUND: i32 = 0x4000_0000;

// Serializer-IDs = Registrierungsreihenfolge in TrackedDataHandlerRegistry.class:
// 0 BYTE, 1 INTEGER, 2 LONG, 3 FLOAT, 4 STRING, 5 TEXT_COMPONENT, 6 OPTIONAL_TEXT_COMPONENT,
// 7 ITEM_STACK, 8 BOOLEAN, ... 20 ENTITY_POSE.
const DATA_TYPE_BYTE: i32 = 0;
const DATA_TYPE_INT: i32 = 1;
const DATA_TYPE_LONG: i32 = 2;
const DATA_TYPE_FLOAT: i32 = 3;
const DATA_TYPE_STRING: i32 = 4;
const DATA_TYPE_TEXT: i32 = 5;
const DATA_TYPE_OPTIONAL_TEXT: i32 = 6;
const DATA_TYPE_ITEM_STACK: i32 = 7;
const DATA_TYPE_BOOLEAN: i32 = 8;
// EndCrystalEntity: Entity belegt 0..7, danach BEAM_TARGET (8) und SHOW_BOTTOM (9) in der
// Reihenfolge der registerData-Aufrufe. Ein von Spielern gesetzter Kristall hat KEINEN
// Boden (EndCrystalItem: setShowBottom(false)).
const END_CRYSTAL_INDEX_SHOW_BOTTOM: u8 = 9;
// AbstractArrow: ID_FLAGS (8), PIERCE_LEVEL (9), IN_GROUND (10, Boolean) -- steckt der Pfeil?
const ARROW_INDEX_IN_GROUND: u8 = 10;
// Interaction: WIDTH (8) und HEIGHT (9) als Float, RESPONSE (10) -- die unsichtbare Klickflaeche,
// mit der Server Display-Entities anklickbar machen.
const INTERACTION_INDEX_WIDTH: u8 = 8;
const INTERACTION_INDEX_HEIGHT: u8 = 9;
// 9 ROTATION, 10 BLOCK_POS, 11 OPTIONAL_BLOCK_POS -- gegengeprueft daran, dass mit derselben
// Zaehlung ENTITY_POSE auf 20 faellt, was schon verifiziert war.
const DATA_TYPE_ROTATION: i32 = 9;
const DATA_TYPE_BLOCK_POS: i32 = 10;
const DATA_TYPE_OPTIONAL_BLOCK_POS: i32 = 11;
const DATA_TYPE_FACING: i32 = 12;
const DATA_TYPE_BLOCK_STATE: i32 = 14;
const DATA_TYPE_OPTIONAL_BLOCK_STATE: i32 = 15;
const DATA_TYPE_OPTIONAL_INT: i32 = 19;
const DATA_TYPE_ENTITY_POSE: i32 = 20;
// TamableAnimal.DATA_OWNERUUID_ID: optional(UUID) -- ein Bool, dann 16 Bytes. Steht vor der
// Variante einer Katze; ohne ihn brach der Strom dort ab.
const DATA_TYPE_OPTIONAL_LIVING_ENTITY_REFERENCE: i32 = 13;
// Die Tier-Varianten (Katze, Kuh, Wolf, Wolfsklang, Frosch, Schwein, Huhn): alle
// ByteBufCodecs.holderRegistry, also eine VarInt-Registry-ID.
const DATA_TYPE_CAT_VARIANT: i32 = 21;
const DATA_TYPE_CHICKEN_VARIANT: i32 = 27;
// Die uebrigen Serializer bis 34, in Registry-Reihenfolge (TrackedDataHandlerRegistry, per
// Bootstrap des 1.21.11-Clients ausgelesen): Dorfbewohner-Daten, Zombie-Nautilus-Variante,
// optionale GlobalPos, Gemaelde-Variante und vier Zustands-Enums.
const DATA_TYPE_VILLAGER_DATA: i32 = 18;
const DATA_TYPE_ZOMBIE_NAUTILUS_VARIANT: i32 = 28;
const DATA_TYPE_OPTIONAL_GLOBAL_POS: i32 = 29;
const DATA_TYPE_PAINTING_VARIANT: i32 = 30;
const DATA_TYPE_SNIFFER_STATE: i32 = 31;
const DATA_TYPE_OXIDATION_LEVEL: i32 = 34;
// TamableAnimal.DATA_FLAGS_ID (Bit 1 = sitzt). AgeableMob belegt 16, Animal nichts.
const TAMABLE_INDEX_FLAGS: u8 = 17;
pub const TAMABLE_FLAG_SITTING: u8 = 1;
// ParticleTypes.PACKET_CODEC bzw. dessen Liste (VarInt-Anzahl, dann die Partikel). Die Liste
// ist LivingEntity.POTION_SWIRLS (Index 10) -- sie steht VOR der Absorption (17) und kommt
// beim Goldapfel im selben Paket mit. Solange sie unlesbar war, brach der Strom dort ab und
// die goldenen Herzen kamen nie an.
const DATA_TYPE_PARTICLE: i32 = 16;
// AreaEffectCloud (Trankwolke am Boden): Entity belegt 0..7, danach RADIUS (8, Float),
// WAITING (9, Boolean) und PARTICLE (10, ein einzelner Partikel).
const CLOUD_INDEX_RADIUS: u8 = 8;
const CLOUD_INDEX_WAITING: u8 = 9;
const CLOUD_INDEX_PARTICLE: u8 = 10;
const DATA_TYPE_PARTICLE_LIST: i32 = 17;
// PlayerLikeEntity.MAIN_ARM (Index 15), PacketCodecs.indexed = VarInt: 0 links, 1 rechts.
const DATA_TYPE_ARM: i32 = 38;
const PLAYER_INDEX_MAIN_ARM: u8 = 15;
// VECTOR_3F (3 Floats) und QUATERNION_F (4 Floats) stehen ganz hinten in derselben Liste.
// Sie sind fuer Display-Entities NOTWENDIG, auch wenn uns ihr Inhalt teils gar nicht
// interessiert: TRANSLATION/SCALE/LEFT_ROTATION/RIGHT_ROTATION liegen VOR dem eigentlichen
// Inhalt (Index 23). Ohne ihre Laenge bricht der Parser vorher ab und der Text kommt nie an.
const DATA_TYPE_VECTOR_3F: i32 = 35;
const DATA_TYPE_QUATERNION_F: i32 = 36;

// Entity.FLAGS, Bit 1 (0x02) -- isSneaking(). NICHT dasselbe wie die Pose: gedrueckte
// Schleichtaste setzt das Flag sofort, die Pose CROUCHING kommt nur, wenn der Spieler auch
// wirklich in die geduckte Box wechselt. Fuer die Hitbox zaehlt die POSE, das Flag ist nur
// fuer die Darstellung interessant.
pub const ENTITY_FLAG_SNEAKING: u8 = 0x02;
// Entity.FLAGS, Bit 0 (0x01) -- brennt (Entity.isOnFire auf dem Client).
pub const ENTITY_FLAG_ON_FIRE: u8 = 0x01;
// Entity.FLAGS, Bit 5 (0x20) -- isInvisible(). Ein unsichtbarer Armorstand ist der Traeger
// fast jedes Hologramms: sein Modell darf nicht gezeichnet werden, sein Namensschild schon.
pub const ENTITY_FLAG_INVISIBLE: u8 = 0x20;
// Entity.FLAGS, Bit 6 (0x40) -- leuchtet (Effekt Leuchten, Spektralpfeil, /effect, Teams mit
// Plugins): der Client zeichnet einen Umriss in Teamfarbe, sichtbar durch Waende.
pub const ENTITY_FLAG_GLOWING: u8 = 0x40;
// Entity.FLAGS, Bit 7 (0x80) -- LivingEntity.isGliding(): Elytra-Flug.
pub const ENTITY_FLAG_GLIDING: u8 = 0x80;
// FireworkRocketEntity: Entity belegt 0..7, danach ITEM (8), SHOOTER_ENTITY_ID (9, OptionalInt)
// und SHOT_AT_ANGLE (10). SHOOTER_ENTITY_ID ist nur gesetzt, wenn die Rakete an einem
// Schuetzen HAENGT (Elytra-Schub), nicht bei einer frei abgeschossenen.
const FIREWORK_INDEX_SHOOTER: u8 = 9;

// LivingEntity.LIVING_FLAGS -- das ERSTE von LivingEntity registrierte Feld, also Index 8
// (Entity belegt 0..7). Gegengeprueft an der schon verifizierten SLEEPING_POSITION auf 14:
// 8 LIVING_FLAGS, 9 HEALTH, 10 POTION_SWIRLS, 11 POTION_SWIRLS_AMBIENT, 12 STUCK_ARROW_COUNT,
// 13 STINGER_COUNT, 14 SLEEPING_POSITION -- die Zaehlung geht exakt auf.
//
// Daran haengt, ob jemand gerade ein Item BENUTZT: das Schild oben, den Bogen gespannt, am
// Essen. Fuer einen PvP-Client ist das keine Feinheit -- ob der Gegner blockt, entscheidet,
// ob man zuschlaegt oder wartet.
const ENTITY_DATA_INDEX_LIVING_FLAGS: u8 = 8;
// LivingEntity.USING_ITEM_FLAG / OFF_HAND_ACTIVE_FLAG.
pub const LIVING_FLAG_USING_ITEM: u8 = 0x01;
pub const LIVING_FLAG_OFFHAND_ACTIVE: u8 = 0x02;
// LivingEntity.isUsingRiptide(): Sog-Angriff laeuft (setzt nur der Server, s. physics).
pub const LIVING_FLAG_USING_RIPTIDE: u8 = 0x04;

// Was uns aus dem Metadaten-Strom interessiert. Alles andere wird uebersprungen; None heisst
// "stand nicht in diesem Paket", nicht "auf Standard zurueckgesetzt".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityDataUpdate {
    pub entity_id: i32,
    // Spieler (und Puppen): Haupthand links (Vanillas Option "Haupthand")? Dann haelt der
    // linke Arm das Haupthand-Item.
    pub main_arm_left: Option<bool>,
    // Boolean an Index 16 (Baby bei Tieren, Dorfbewohnern, Zombies; s. AGEABLE_INDEX_BABY).
    pub baby: Option<bool>,
    // VillagerData (Dorfbewohner, Zombie-Dorfbewohner): (Biom-Typ, Beruf, Stufe) als
    // Registry-IDs bzw. 1..5.
    pub villager_data: Option<(i32, i32, i32)>,
    pub pose: Option<u8>,
    pub flags: Option<u8>,
    // Restluft in Ticks (s. ENTITY_DATA_INDEX_AIR).
    pub air: Option<i32>,
    // Doppeltes Option mit Absicht: das AEUSSERE None heisst "stand nicht im Paket", das
    // innere None heisst "der Server hat das Feld geleert" -- also aufgewacht.
    pub sleeping_pos: Option<Option<(i32, i32, i32)>>,
    // Entity.CUSTOM_NAME, ebenfalls doppelt: aussen "stand nicht im Paket", innen "Name
    // geloescht". Die Abschnitte tragen die Farben mit (s. nbt::text_to_spans).
    pub custom_name: Option<Option<Vec<crate::nbt::TextSpan>>>,
    pub name_visible: Option<bool>,
    // LivingEntity.LIVING_FLAGS, s. LIVING_FLAG_USING_ITEM.
    pub living_flags: Option<u8>,
    // Absorptionspunkte eines Spielers (s. PLAYER_INDEX_ABSORPTION).
    pub absorption: Option<f32>,
    // Dasselbe Byte wie `display_billboard` (s. ARMOR_STAND_INDEX_FLAGS).
    pub armor_stand_flags: Option<u8>,
    // Gelenkwinkel eines Armorstands (s. ARMOR_STAND_INDEX_HEAD_POSE), je Gelenk nur, wenn er im
    // Paket stand.
    pub armor_stand_poses: [Option<[f32; 3]>; 6],
    // --- DisplayEntity ---------------------------------------------------------------
    pub display_translation: Option<[f32; 3]>,
    pub display_scale: Option<[f32; 3]>,
    // Quaternionen (x, y, z, w) der AffineTransformation -- Reihenfolge wie
    // PacketByteBuf.writeQuaternionf.
    pub display_left_rotation: Option<[f32; 4]>,
    // EndCrystalEntity.SHOW_BOTTOM (Index 9, Boolean) -- nur bei einem End-Kristall gemeint.
    pub end_crystal_show_bottom: Option<bool>,
    // AbstractArrow.IN_GROUND (Index 10, Boolean) -- nur bei Pfeil, Spektralpfeil und
    // Dreizack gemeint.
    pub arrow_in_ground: Option<bool>,
    // Interaction.WIDTH/HEIGHT (Index 8/9, Float) -- nur bei einem interaction-Entity gemeint
    // (Index 9 ist bei Lebewesen die Gesundheit).
    pub interaction_width: Option<f32>,
    pub interaction_height: Option<f32>,
    // LivingEntity.POTION_SWIRLS (Index 10): die Farben (ARGB) der entity_effect-Wirbel, die
    // das Entity wegen seiner Trankeffekte verstreut. Leer = keine. Andere Partikelarten in
    // der Liste werden hier weggelassen -- Vanilla legt dort nur entity_effect ab.
    pub potion_swirls: Option<Vec<u32>>,
    // LivingEntity.POTION_SWIRLS_AMBIENT (Index 11): nur Umgebungs-Effekte (Leuchtfeuer) --
    // dann streut das Entity fuenfmal seltener.
    pub potion_swirls_ambient: Option<bool>,
    pub display_right_rotation: Option<[f32; 4]>,
    pub display_billboard: Option<u8>,
    // Interpolation einer Transformations-Aenderung: Verzoegerung und Dauer in Ticks.
    pub display_interp_start: Option<i32>,
    // ExperienceOrb.DATA_VALUE (ebenfalls Index 8, Int) -- nur bei einer XP-Kugel gemeint.
    pub orb_value: Option<i32>,
    pub display_interp_duration: Option<i32>,
    pub item_display_mode: Option<u8>,
    pub display_view_range: Option<f32>,
    // Der Inhalt eines Display-Entities (Index 23), unterschieden am Serializer.
    pub display_text: Option<Vec<crate::nbt::TextSpan>>,
    pub display_item: Option<Option<ItemStack>>,
    // Der Stack an Index 8 (s. ENTITY_INDEX_ITEM): aussen "stand nicht im Paket", innen leer.
    pub item_stack: Option<Option<ItemStack>>,
    // Gegenstandsrahmen: Inhalt (Index 9, wie item_stack doppelt), Drehung des Inhalts (Index 10,
    // 0..7) und die Wand, an der er haengt (Index 8, Direction-Ordinal wie im Spawn-Paket).
    pub frame_item: Option<Option<ItemStack>>,
    pub frame_rotation: Option<i32>,
    pub hanging_direction: Option<u8>,
    pub display_block_state: Option<u32>,
    // FireworkRocketEntity.SHOOTER_ENTITY_ID, doppelt wie custom_name: aussen "stand nicht im
    // Paket", innen "kein Schuetze". Welcher Typ gemeint ist, weiss erst der Empfaenger.
    pub firework_shooter: Option<Option<i32>>,
    // Nur bei text_display belegt.
    pub text_line_width: Option<i32>,
    pub text_background: Option<i32>,
    pub text_opacity: Option<u8>,
    pub text_flags: Option<u8>,
    // TamableAnimal-Flags (Index 17, Byte) -- nur bei zaehmbaren Tieren gemeint.
    pub tamable_flags: Option<u8>,
    // Registry-ID der Katzenvariante (Serializer CAT_VARIANT).
    pub cat_variant: Option<i32>,
    // Nur bei einer Trankwolke gemeint (s. CLOUD_INDEX_RADIUS).
    pub cloud_radius: Option<f32>,
    pub cloud_waiting: Option<bool>,
    pub cloud_particle: Option<crate::particle::ParticleEffect>,
}

// Sicherheitsdeckel gegen einen Strom ohne Endmarke.
const MAX_ENTITY_DATA_FIELDS: usize = 64;

// Liest den Metadaten-Strom, bis die Endmarke kommt ODER ein Serializer auftaucht, dessen
// Laenge wir nicht kennen. Der zweite Fall ist bewusst KEIN Fehler: ab da wissen wir nicht
// mehr, wo das naechste Feld anfaengt, also hoeren wir auf und liefern, was bis dahin
// dastand. Die uns wichtigen Felder (0 und 6) stehen am Anfang und benutzen alle
// Serializer, die wir kennen -- ein unbekannter Typ dahinter kostet uns nichts.
pub fn read_set_entity_data(payload: &[u8]) -> Result<EntityDataUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let mut out = EntityDataUpdate { entity_id: r.read_varint()?, ..Default::default() };
    for _ in 0..MAX_ENTITY_DATA_FIELDS {
        let index = r.read_u8()?;
        if index == 0xFF {
            break;
        }
        let data_type = r.read_varint()?;
        match data_type {
            DATA_TYPE_BYTE => {
                let v = r.read_u8()?;
                match index {
                    ENTITY_DATA_INDEX_FLAGS => out.flags = Some(v),
                    ENTITY_DATA_INDEX_LIVING_FLAGS => out.living_flags = Some(v),
                    // Index 15 ist doppelt belegt: bei einem DisplayEntity der
                    // Billboard-Modus, bei einem ArmorStandEntity dessen Flags. Beide sind
                    // Bytes und im Strom nicht zu unterscheiden -- welches Feld gemeint ist,
                    // entscheidet erst der Entity-TYP, und den kennt dieser Parser nicht
                    // (s. Entities::apply_metadata, das genau eines der beiden uebernimmt).
                    DISPLAY_INDEX_BILLBOARD => {
                        out.display_billboard = Some(v);
                        out.armor_stand_flags = Some(v);
                    }
                    TEXT_DISPLAY_INDEX_TEXT_OPACITY => out.text_opacity = Some(v),
                    TEXT_DISPLAY_INDEX_FLAGS => out.text_flags = Some(v),
                    ITEM_DISPLAY_INDEX_MODE => out.item_display_mode = Some(v),
                    _ => {}
                }
                if index == TAMABLE_INDEX_FLAGS {
                    out.tamable_flags = Some(v);
                }
            }
            DATA_TYPE_ENTITY_POSE => {
                let v = r.read_varint()?;
                if index == ENTITY_DATA_INDEX_POSE {
                    // OutOfBoundsHandling.ZERO: ein unbekannter Index ist in Vanilla
                    // STANDING, kein Fehler.
                    out.pose = Some(u8::try_from(v).unwrap_or(0));
                }
            }
            DATA_TYPE_INT => {
                let v = r.read_varint()?;
                match index {
                    ENTITY_DATA_INDEX_AIR => out.air = Some(v),
                    // Bei anderen Typen stehen hier andere Ints (Boot: Wackel-Ticks) -- der
                    // Empfaenger nimmt sie nur bei einem Display.
                    DISPLAY_INDEX_START_INTERPOLATION => {
                        out.display_interp_start = Some(v);
                        out.orb_value = Some(v);
                    }
                    DISPLAY_INDEX_INTERPOLATION_DURATION => out.display_interp_duration = Some(v),
                    ITEM_FRAME_INDEX_ROTATION => out.frame_rotation = Some(v),
                    TEXT_DISPLAY_INDEX_LINE_WIDTH => out.text_line_width = Some(v),
                    TEXT_DISPLAY_INDEX_BACKGROUND => out.text_background = Some(v),
                    _ => {}
                }
            }
            DATA_TYPE_LONG => {
                r.read_varlong()?;
            }
            DATA_TYPE_FLOAT => {
                let v = r.read_f32()?;
                if index == DISPLAY_INDEX_VIEW_RANGE {
                    out.display_view_range = Some(v);
                }
                if index == PLAYER_INDEX_ABSORPTION {
                    out.absorption = Some(v);
                }
                if index == INTERACTION_INDEX_WIDTH {
                    out.interaction_width = Some(v);
                }
                if index == INTERACTION_INDEX_HEIGHT {
                    out.interaction_height = Some(v);
                }
                if index == CLOUD_INDEX_RADIUS {
                    out.cloud_radius = Some(v);
                }
            }
            DATA_TYPE_BOOLEAN => {
                let v = r.read_bool()?;
                if index == ENTITY_DATA_INDEX_NAME_VISIBLE {
                    out.name_visible = Some(v);
                }
                // Welcher Typ das ist, weiss erst der Empfaenger -- s. world::Entity.
                if index == END_CRYSTAL_INDEX_SHOW_BOTTOM {
                    out.end_crystal_show_bottom = Some(v);
                }
                if index == ARROW_INDEX_IN_GROUND {
                    out.arrow_in_ground = Some(v);
                }
                if index == ENTITY_DATA_INDEX_POTION_SWIRLS_AMBIENT {
                    out.potion_swirls_ambient = Some(v);
                }
                if index == AGEABLE_INDEX_BABY {
                    out.baby = Some(v);
                }
                if index == CLOUD_INDEX_WAITING {
                    out.cloud_waiting = Some(v);
                }
            }
            DATA_TYPE_VECTOR_3F => {
                let v = [r.read_f32()?, r.read_f32()?, r.read_f32()?];
                match index {
                    DISPLAY_INDEX_TRANSLATION => out.display_translation = Some(v),
                    DISPLAY_INDEX_SCALE => out.display_scale = Some(v),
                    _ => {}
                }
            }
            // Quaternion: vier Floats, x y z w (PacketByteBuf.writeQuaternionf).
            DATA_TYPE_QUATERNION_F => {
                let v = [r.read_f32()?, r.read_f32()?, r.read_f32()?, r.read_f32()?];
                match index {
                    DISPLAY_INDEX_LEFT_ROTATION => out.display_left_rotation = Some(v),
                    DISPLAY_INDEX_RIGHT_ROTATION => out.display_right_rotation = Some(v),
                    _ => {}
                }
            }
            DATA_TYPE_BLOCK_STATE => {
                let v = r.read_varint()?;
                if index == DISPLAY_INDEX_CONTENT {
                    out.display_block_state = Some(v.max(0) as u32);
                }
            }
            // PacketCodecs.optional(BLOCK_STATE): 0 = leer, sonst StateId + 1.
            DATA_TYPE_OPTIONAL_BLOCK_STATE => {
                r.read_varint()?;
            }
            // PacketCodecs.OPTIONAL_INT: 0 = leer, sonst Wert + 1.
            DATA_TYPE_OPTIONAL_INT => {
                let v = r.read_varint()?;
                if index == FIREWORK_INDEX_SHOOTER {
                    out.firework_shooter = Some((v != 0).then(|| v - 1));
                }
            }
            DATA_TYPE_FACING => {
                let v = r.read_varint()?;
                if index == HANGING_INDEX_DIRECTION {
                    out.hanging_direction = u8::try_from(v).ok();
                }
            }
            DATA_TYPE_ARM => {
                let v = r.read_varint()?;
                if index == PLAYER_INDEX_MAIN_ARM {
                    out.main_arm_left = Some(v == 0);
                }
            }
            DATA_TYPE_OPTIONAL_LIVING_ENTITY_REFERENCE => {
                if r.read_bool()? {
                    r.read_i64()?;
                    r.read_i64()?;
                }
            }
            // VarInt Biom-Typ, VarInt Beruf (beides Registry-IDs), VarInt Stufe.
            DATA_TYPE_VILLAGER_DATA => {
                let v = (r.read_varint()?, r.read_varint()?, r.read_varint()?);
                out.villager_data = Some(v);
            }
            // Registry-ID bzw. Enum-Index, je ein VarInt.
            DATA_TYPE_ZOMBIE_NAUTILUS_VARIANT | DATA_TYPE_SNIFFER_STATE..=DATA_TYPE_OXIDATION_LEVEL => {
                r.read_varint()?;
            }
            // Bool, dann Dimension (Identifier als String) und Blockposition (Long).
            DATA_TYPE_OPTIONAL_GLOBAL_POS => {
                if r.read_bool()? {
                    r.read_string()?;
                    r.read_i64()?;
                }
            }
            // RegistryEntry: VarInt id + 1, 0 hiesse "direkt mitgeschickt" -- dessen Inhalt
            // laesst sich nicht ueberspringen, also dann abbrechen.
            DATA_TYPE_PAINTING_VARIANT => {
                if r.read_varint()? == 0 {
                    break;
                }
            }
            DATA_TYPE_CAT_VARIANT..=DATA_TYPE_CHICKEN_VARIANT => {
                let v = r.read_varint()?;
                if data_type == DATA_TYPE_CAT_VARIANT {
                    out.cat_variant = Some(v);
                }
            }
            // Ein unlesbarer Partikel (neuere Serverversion) macht die Position unbrauchbar --
            // dann abbrechen wie bei einem unbekannten Serializer.
            DATA_TYPE_PARTICLE => {
                let Ok(p) = crate::particle::read_particle(&mut r) else { break };
                if index == CLOUD_INDEX_PARTICLE {
                    out.cloud_particle = Some(p);
                }
            }
            DATA_TYPE_PARTICLE_LIST => {
                let n = r.read_varint()?;
                if !(0..=256).contains(&n) {
                    break;
                }
                let mut farben = Vec::new();
                let mut kaputt = false;
                for _ in 0..n {
                    match crate::particle::read_particle(&mut r) {
                        Ok(crate::particle::ParticleEffect { type_id: crate::particle::PARTICLE_ENTITY_EFFECT, options: crate::particle::ParticleOptions::Tinted(argb) }) => farben.push(argb),
                        Ok(_) => {}
                        Err(_) => kaputt = true,
                    }
                    if kaputt {
                        break;
                    }
                }
                if kaputt {
                    break;
                }
                if index == ENTITY_DATA_INDEX_POTION_SWIRLS {
                    out.potion_swirls = Some(farben);
                }
            }
            DATA_TYPE_ITEM_STACK => {
                // Ein Component, den dieser Client nicht kennt, macht die Leseposition
                // unbrauchbar -- dann bricht der Strom hier ab wie bei einem unbekannten
                // Serializer, statt danach Muell zu lesen.
                let Ok(stack) = crate::item::read_optional_slot(&mut r) else { break };
                if index == DISPLAY_INDEX_CONTENT {
                    out.display_item = Some(stack);
                } else if index == ENTITY_INDEX_ITEM {
                    out.item_stack = Some(stack);
                } else if index == ITEM_FRAME_INDEX_ITEM {
                    out.frame_item = Some(stack);
                }
            }
            DATA_TYPE_OPTIONAL_BLOCK_POS => {
                let vorhanden = r.read_bool()?;
                let pos = if vorhanden { Some(decode_block_pos(r.read_i64()?)) } else { None };
                if index == ENTITY_DATA_INDEX_SLEEPING_POS {
                    out.sleeping_pos = Some(pos);
                }
            }
            DATA_TYPE_BLOCK_POS => {
                r.read_i64()?;
            }
            DATA_TYPE_ROTATION => {
                let v = [r.read_f32()?, r.read_f32()?, r.read_f32()?];
                if let Some(i) = index.checked_sub(ARMOR_STAND_INDEX_HEAD_POSE).filter(|&i| i < ARMOR_STAND_POSE_COUNT) {
                    out.armor_stand_poses[i as usize] = Some(v);
                }
            }
            DATA_TYPE_STRING => {
                r.read_string()?;
            }
            DATA_TYPE_TEXT => {
                let value = crate::nbt::read_root(&mut r)?;
                if index == DISPLAY_INDEX_CONTENT {
                    out.display_text = Some(value.as_ref().map(crate::nbt::text_to_spans).unwrap_or_default());
                }
            }
            DATA_TYPE_OPTIONAL_TEXT => {
                let value = if r.read_bool()? { crate::nbt::read_root(&mut r)? } else { None };
                if index == ENTITY_DATA_INDEX_CUSTOM_NAME {
                    out.custom_name = Some(value.as_ref().map(crate::nbt::text_to_spans));
                }
            }
            // Unbekannte Laenge -- ab hier ist der Strom fuer uns nicht mehr zerlegbar.
            _ => break,
        }
    }
    Ok(out)
}

// Serverbound Client Command mit Aktion PERFORM_RESPAWN (0) -- nach dem Tod abschicken,
// damit der Server den Spieler neu setzt (Position + Chunks folgen als eigene Pakete).
pub fn perform_respawn_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CLIENT_COMMAND).write_varint(0);
    w.into_bytes()
}

// Remove Entities (0x4B): Anzahl (VarInt), dann EntityIds (je VarInt) -- readIntList.
// EntityPassengersSetS2CPacket: VarInt Fahrzeug-Id, dann ein VarInt-Array der Passagier-Ids
// (buf.readIntArray = Laenge als VarInt, danach je ein VarInt). ID aus PlayStateFactories.
//
// Frueherer Fehler: beim Spawn eines Spielers kamen 2 Nametags, einer blieb stehen.
// Namensschild-Plugins setzen dem Spieler ein text_display auf den Kopf. Ein Passagier
// bekommt vom Server KEINE eigenen Bewegungspakete -- er wird vom Fahrzeug mitgezogen. Wer
// dieses Paket nicht auswertet, laesst das Display fuer immer an seinem Spawnpunkt stehen,
// waehrend der Spieler weggeht: genau das "statische, in der Luft schwebende" Schild.
pub const CB_PLAY_SET_PASSENGERS: i32 = 0x69;

pub fn read_set_passengers(payload: &[u8]) -> Result<(i32, Vec<i32>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let vehicle = r.read_varint()?;
    let count = r.read_varint()?;
    if count < 0 {
        return Err(ReadError);
    }
    let mut ids = Vec::with_capacity(count.min(256) as usize);
    for _ in 0..count {
        ids.push(r.read_varint()?);
    }
    Ok((vehicle, ids))
}

// --- Bossbar ---------------------------------------------------------------------------------
//
// BossBarS2CPacket: UUID, Aktion (VarInt, Reihenfolge von BossBarS2CPacket.Type), dann je
// Aktion ihre Felder. Farbe (pink, blue, red, green, yellow, purple, white) und Stil
// (progress, notched_6/10/12/20) als VarInt-Ordinal, die drei Schalter als Bitmaske
// (1 Himmel verdunkeln, 2 Drachenmusik, 4 Nebel verdichten). ID: Platz 9 in PlayStateFactories.
pub const CB_PLAY_BOSS_EVENT: i32 = 0x09;

#[derive(Clone, Debug, PartialEq)]
pub enum BossEvent {
    Add { name: Vec<crate::nbt::TextSpan>, progress: f32, color: i32, style: i32, flags: u8 },
    Remove,
    Progress(f32),
    Name(Vec<crate::nbt::TextSpan>),
    Style { color: i32, style: i32 },
    Flags(u8),
}

pub fn read_boss_event(payload: &[u8]) -> Result<([u8; 16], BossEvent), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let uuid = r.read_uuid()?.0;
    let event = match r.read_varint()? {
        0 => {
            let name = read_text_spans(&mut r)?;
            let progress = r.read_f32()?;
            let color = r.read_varint()?;
            let style = r.read_varint()?;
            BossEvent::Add { name, progress, color, style, flags: r.read_u8()? }
        }
        1 => BossEvent::Remove,
        2 => BossEvent::Progress(r.read_f32()?),
        3 => BossEvent::Name(read_text_spans(&mut r)?),
        4 => {
            let color = r.read_varint()?;
            BossEvent::Style { color, style: r.read_varint()? }
        }
        5 => BossEvent::Flags(r.read_u8()?),
        _ => return Err(ReadError),
    };
    Ok((uuid, event))
}

// --- Reiten --------------------------------------------------------------------------------
//
// Lenkt der Client sein Fahrzeug selbst (Boot, gesatteltes Pferd ...), meldet er dessen Stand
// je Tick mit Move Vehicle; der Server antwortet nur, wenn er nicht einverstanden ist, mit
// seinem eigenen Move Vehicle (Stelle, auf die der Client zuruecksetzen soll). IDs aus der
// Registrierungsreihenfolge in PlayStateFactories (1.21.11): serverwaerts direkt hinter
// MOVE_PLAYER_STATUS_ONLY (0x20) MOVE_VEHICLE und PADDLE_BOAT, clientwaerts Platz 55.
pub const SB_PLAY_MOVE_VEHICLE: i32 = 0x21;
pub const SB_PLAY_PADDLE_BOAT: i32 = 0x22;
pub const CB_PLAY_MOVE_VEHICLE: i32 = 0x37;

// VehicleMoveC2SPacket: Vec3d (drei Doubles), Yaw, Pitch (Floats), onGround.
pub fn move_vehicle_payload(pos: (f64, f64, f64), yaw_deg: f32, pitch_deg: f32, on_ground: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_MOVE_VEHICLE).write_f64(pos.0).write_f64(pos.1).write_f64(pos.2).write_f32(yaw_deg).write_f32(pitch_deg).write_bool(on_ground);
    w.into_bytes()
}

// BoatPaddleStateC2SPacket: linkes, dann rechtes Ruder in Bewegung.
pub fn paddle_boat_payload(left: bool, right: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PADDLE_BOAT).write_bool(left).write_bool(right);
    w.into_bytes()
}

// VehicleMoveS2CPacket: Vec3d, Yaw, Pitch.
pub fn read_move_vehicle(payload: &[u8]) -> Result<((f64, f64, f64), f32, f32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let pos = (r.read_f64()?, r.read_f64()?, r.read_f64()?);
    Ok((pos, r.read_f32()?, r.read_f32()?))
}

pub fn read_remove_entities(payload: &[u8]) -> Result<Vec<i32>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let count = r.read_varint()?;
    if count < 0 {
        return Err(ReadError);
    }
    let mut ids = Vec::with_capacity(count.min(4096) as usize);
    for _ in 0..count {
        ids.push(r.read_varint()?);
    }
    Ok(ids)
}

// --- Inventar / Container -----------------------------------------------------------------
//
// IDs aus PlayStateFactories des 1.21.11-Jars (Registrierungsreihenfolge = Packet-ID, beim
// S2C-Block +1 wegen des vorangestellten Bundle-Packets 0x00), gegengeprueft an den bereits
// verifizierten Ankern SWING 0x3C (C2S) und SET_HEALTH 0x66 (S2C).
//
// Slots kommen als ItemStack.OPTIONAL_PACKET_CODEC (s. item.rs). Ein unlesbarer Stack
// beendet nur die Liste -- die vorher gelesenen Slots bleiben gueltig.

// InventoryS2CPacket: der komplette Inhalt eines Containers (syncId 0 = Spielerinventar).
pub const CB_PLAY_CONTAINER_SET_CONTENT: i32 = 0x12;
// ScreenHandlerSlotUpdateS2CPacket: ein einzelner Slot.
pub const CB_PLAY_CONTAINER_SET_SLOT: i32 = 0x14;
// SetCursorItemS2CPacket: der Stack an der Maus.
pub const CB_PLAY_SET_CURSOR_ITEM: i32 = 0x5E;
// SetPlayerInventoryS2CPacket: ein Slot im Spielerinventar, unabhaengig vom offenen Screen.
pub const CB_PLAY_SET_PLAYER_INVENTORY: i32 = 0x6A;
// UpdateSelectedSlotS2CPacket: der Server setzt den aktiven Hotbar-Slot.
pub const CB_PLAY_SET_CARRIED_ITEM: i32 = 0x67;
// OpenScreenS2CPacket: Container oeffnen (Kiste, Ofen, ...).
pub const CB_PLAY_OPEN_SCREEN: i32 = 0x39;
// ClientboundOpenBookPacket: das Buch in dieser Hand (0 Haupt-, 1 Nebenhand) zum Lesen oeffnen.
pub const CB_PLAY_OPEN_BOOK: i32 = 0x38;

pub fn read_open_book(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}
// CloseScreenS2CPacket: Container schliessen.
pub const CB_PLAY_CONTAINER_CLOSE: i32 = 0x11;

// UpdateSelectedSlotC2SPacket: aktiver Hotbar-Slot (Short!).
pub const SB_PLAY_SET_CARRIED_ITEM: i32 = 0x34;
// ClickSlotC2SPacket.
pub const SB_PLAY_CONTAINER_CLICK: i32 = 0x11;
// CloseHandledScreenC2SPacket.
pub const SB_PLAY_CONTAINER_CLOSE: i32 = 0x12;
// ClientboundContainerSetDataPacket: ein Zahlenwert des offenen Containers (Zaubertisch:
// Kosten/Hinweise, Amboss: Erfahrungskosten, Ofen: Brennzeit, ...).
pub const CB_PLAY_CONTAINER_SET_DATA: i32 = 0x13;
// ServerboundContainerButtonClickPacket: Knopf im Container (Zaubertisch: Option 0..2).
pub const SB_PLAY_CONTAINER_BUTTON_CLICK: i32 = 0x10;
// ServerboundRenameItemPacket: Name im Amboss.
pub const SB_PLAY_RENAME_ITEM: i32 = 0x2F;

// (Container-ID, Index, Wert). Container-ID als VarInt (FriendlyByteBuf.readContainerId),
// Index und Wert als Short.
pub fn read_container_set_data(payload: &[u8]) -> Result<(i32, i16, i16), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let index = r.read_u16()? as i16;
    let value = r.read_u16()? as i16;
    Ok((sync_id, index, value))
}

// --- Handel mit Dorfbewohnern ---
// IDs aus dem packets.json-Report des 1.21.11-Datengenerators.
pub const CB_PLAY_MERCHANT_OFFERS: i32 = 0x32;
pub const SB_PLAY_SELECT_TRADE: i32 = 0x32;

// Ein Angebot (TradeOffer.PACKET_CODEC). Die Preise kommen als Grundpreis; was der Bildschirm
// zeigt, rechnet displayed_cost_a daraus (Nachfrage, Rabatt).
#[derive(Clone, Debug, PartialEq)]
pub struct TradeOffer {
    pub cost_a: ItemStack,
    pub cost_b: Option<ItemStack>,
    pub result: Option<ItemStack>,
    pub out_of_stock: bool,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub special_price: i32,
    pub price_multiplier: f32,
    pub demand: i32,
}

impl TradeOffer {
    // Ausverkauft: Vanilla prueft dafuer nur uses >= max_uses (TradeOffer.isDisabled). Das
    // mitgeschickte Flag setzt beim Lesen genau diesen Zustand.
    pub fn disabled(&self) -> bool {
        self.out_of_stock || self.uses >= self.max_uses
    }

    // Angezeigter erster Preis: Grundpreis plus Nachfrage-Aufschlag (nie negativ) plus
    // Sonderpreis (Rabatt nach Heldentat/Heilung negativ), auf 1..Stapelgroesse begrenzt.
    pub fn displayed_cost_a(&self, max_stack: i32) -> i32 {
        let basis = self.cost_a.count;
        let aufschlag = (((basis * self.demand) as f32 * self.price_multiplier).floor() as i32).max(0);
        (basis + aufschlag + self.special_price).clamp(1, max_stack.max(1))
    }
}

pub struct MerchantOffers {
    pub sync_id: i32,
    pub offers: Vec<TradeOffer>,
    pub level: i32,
    pub xp: i32,
    pub show_progress: bool,
    pub can_restock: bool,
}

fn read_trade_offer(r: &mut PacketReader) -> Result<TradeOffer, SlotError> {
    let cost_a = crate::item::read_item_cost(r)?;
    let result = read_optional_slot(r)?;
    let cost_b = if r.read_bool()? { Some(crate::item::read_item_cost(r)?) } else { None };
    let out_of_stock = r.read_bool()?;
    let uses = r.read_i32()?;
    let max_uses = r.read_i32()?;
    let xp = r.read_i32()?;
    let special_price = r.read_i32()?;
    let price_multiplier = r.read_f32()?;
    let demand = r.read_i32()?;
    Ok(TradeOffer { cost_a, cost_b, result, out_of_stock, uses, max_uses, xp, special_price, price_multiplier, demand })
}

// ClientboundMapItemDataPacket: Kartenstand -- Massstab, Sperre, (wenn mitgeschickt) die
// Markierungen und ein Rechteck neuer Farben.
pub const CB_PLAY_MAP_ITEM_DATA: i32 = 0x31;

#[derive(Clone, Debug, PartialEq)]
pub struct MapDecoration {
    // Index in der Registry map_decoration_type (eingebaut, feste Reihenfolge).
    pub kind: i32,
    // -128..127 je Achse, Kartenmitte 0 (ein Schritt = halber Kartenpixel).
    pub x: i8,
    pub y: i8,
    // Drehung in 16teln einer Umdrehung.
    pub rot: u8,
    pub name: Option<Vec<nbt::TextSpan>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapPatch {
    pub width: u8,
    pub height: u8,
    pub x: u8,
    pub y: u8,
    // Farb-IDs (MapColor packed: Grundfarbe * 4 + Helligkeit), zeilenweise.
    pub colors: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapItemData {
    pub id: i32,
    pub scale: u8,
    pub locked: bool,
    pub decorations: Option<Vec<MapDecoration>>,
    pub patch: Option<MapPatch>,
}

pub fn read_map_item_data(payload: &[u8]) -> Result<MapItemData, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let id = r.read_varint()?;
    let scale = r.read_u8()?;
    let locked = r.read_bool()?;
    let decorations = if r.read_bool()? {
        let n = r.read_varint()?;
        if !(0..=4096).contains(&n) {
            return Err(ReadError);
        }
        let mut liste = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let kind = r.read_varint()?;
            let x = r.read_i8()?;
            let y = r.read_i8()?;
            let rot = r.read_u8()? & 15;
            let name = if r.read_bool()? { Some(read_text_spans(&mut r)?) } else { None };
            liste.push(MapDecoration { kind, x, y, rot, name });
        }
        Some(liste)
    } else {
        None
    };
    // MapPatch: Breite 0 heisst "kein Rechteck".
    let width = r.read_u8()?;
    let patch = if width > 0 {
        let height = r.read_u8()?;
        let x = r.read_u8()?;
        let y = r.read_u8()?;
        let len = r.read_varint()?;
        if len < 0 || len as usize != width as usize * height as usize {
            return Err(ReadError);
        }
        let colors = r.read_bytes(len as usize)?.to_vec();
        Some(MapPatch { width, height, x, y, colors })
    } else {
        None
    };
    Ok(MapItemData { id, scale, locked, decorations, patch })
}

pub fn read_merchant_offers(payload: &[u8]) -> Result<MerchantOffers, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let n = r.read_varint()?;
    if n < 0 {
        return Err(ReadError);
    }
    let mut offers = Vec::with_capacity(n.min(64) as usize);
    for _ in 0..n {
        offers.push(read_trade_offer(&mut r).map_err(|_| ReadError)?);
    }
    let level = r.read_varint()?;
    let xp = r.read_varint()?;
    let show_progress = r.read_bool()?;
    let can_restock = r.read_bool()?;
    Ok(MerchantOffers { sync_id, offers, level, xp, show_progress, can_restock })
}

// Handel auswaehlen (Index in der Angebotsliste); der Server legt dann die passenden Items
// aus dem Inventar in die Bezahlslots.
pub fn select_trade_payload(index: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SELECT_TRADE).write_varint(index);
    w.into_bytes()
}

pub fn container_button_click_payload(sync_id: i32, button: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONTAINER_BUTTON_CLICK).write_varint(sync_id).write_varint(button);
    w.into_bytes()
}

// ServerboundEditBookPacket: das Buch mit Feder in Inventarplatz `slot` (Hotbar 0..8, Zweithand
// 40) bekommt diese Seiten (hoechstens 100 zu je 1024 Zeichen); mit Titel (hoechstens 32) wird
// es signiert.
pub const SB_PLAY_EDIT_BOOK: i32 = 0x17;

pub fn edit_book_payload(slot: i32, pages: &[String], title: Option<&str>) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_EDIT_BOOK).write_varint(slot).write_varint(pages.len() as i32);
    for seite in pages {
        w.write_string(seite);
    }
    w.write_bool(title.is_some());
    if let Some(t) = title {
        w.write_string(t);
    }
    w.into_bytes()
}

pub fn rename_item_payload(name: &str) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_RENAME_ITEM).write_string(name);
    w.into_bytes()
}

pub struct ContainerContent {
    pub sync_id: i32,
    // "Revision" (state id): muss bei jedem Klick zurueckgeschickt werden, sonst schickt der
    // Server den ganzen Container erneut.
    pub revision: i32,
    pub slots: Vec<Option<ItemStack>>,
    pub cursor: Option<ItemStack>,
    // Erster Slot, an dem das Lesen abgebrochen ist (unbekannter Component-Typ) -- die App
    // loggt das einmal, damit fehlende Components auffallen statt still zu verschwinden.
    pub truncated_at: Option<(usize, SlotError)>,
    // Beim Abbruch: wo genau (s. item::last_slot_failure) und die Rohbytes ab dem
    // unlesbaren Slot -- zum Nachsehen, welches Format dort wirklich steht.
    pub truncated_detail: Option<(String, Vec<u8>)>,
}

pub fn read_container_content(payload: &[u8]) -> Result<ContainerContent, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let revision = r.read_varint()?;
    let count = r.read_varint()?;
    if count < 0 {
        return Err(ReadError);
    }
    let mut slots = Vec::with_capacity(count.min(256) as usize);
    let mut truncated_at = None;
    let mut truncated_detail = None;
    for index in 0..count as usize {
        let ab_hier = r.remaining().to_vec();
        match read_optional_slot(&mut r) {
            Ok(stack) => slots.push(stack),
            Err(err) => {
                let wo = crate::item::last_slot_failure().unwrap_or_else(|| "ohne Component-Angabe".to_string());
                truncated_detail = Some((wo, ab_hier));
                truncated_at = Some((index, err));
                break;
            }
        }
    }
    // Der Cursor steht hinter der Liste -- nach einem Abbruch ist die Leseposition dahin,
    // dann bleibt er leer und wird ohnehin gleich per Set Cursor Item nachgereicht.
    let cursor = if truncated_at.is_none() { read_optional_slot(&mut r).unwrap_or(None) } else { None };
    Ok(ContainerContent { sync_id, revision, slots, cursor, truncated_at, truncated_detail })
}

pub struct ContainerSlotUpdate {
    pub sync_id: i32,
    pub revision: i32,
    pub slot: i16,
    pub stack: Option<ItemStack>,
}

pub fn read_container_set_slot(payload: &[u8]) -> Result<ContainerSlotUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let revision = r.read_varint()?;
    let slot = r.read_i16()?;
    let stack = read_optional_slot(&mut r).map_err(|_| ReadError)?;
    Ok(ContainerSlotUpdate { sync_id, revision, slot, stack })
}

pub fn read_cursor_item(payload: &[u8]) -> Result<Option<ItemStack>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    read_optional_slot(&mut r).map_err(|_| ReadError)
}

// (Slot im Spielerinventar, Stack)
pub fn read_player_inventory_slot(payload: &[u8]) -> Result<(i32, Option<ItemStack>), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let slot = r.read_varint()?;
    let stack = read_optional_slot(&mut r).map_err(|_| ReadError)?;
    Ok((slot, stack))
}

pub fn read_set_carried_item(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

pub struct OpenScreen {
    pub sync_id: i32,
    // ScreenHandlerType-Registry-ID -- bestimmt Groesse und Aufbau des Containers.
    pub screen_type: i32,
    pub title: String,
}

pub fn read_open_screen(payload: &[u8]) -> Result<OpenScreen, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let sync_id = r.read_varint()?;
    let screen_type = r.read_varint()?;
    // Mit Paragraphen-Codes, damit die Farben des Titels ueberleben (s. legacy_to_spans).
    let title = read_text_component_legacy(&mut r)?;
    Ok(OpenScreen { sync_id, screen_type, title })
}

pub fn read_container_close(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

// ServerboundSelectBundleItemPacket: im Buendel in Slot `slot` (Menue-Slot) ist Item `index`
// gewaehlt, -1 = keins.
pub const SB_PLAY_SELECT_BUNDLE_ITEM: i32 = 0x02;

pub fn select_bundle_item_payload(slot: i32, index: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SELECT_BUNDLE_ITEM).write_varint(slot).write_varint(index);
    w.into_bytes()
}

// Aktiven Hotbar-Slot melden (0..8). Vanilla schreibt hier einen Short, keinen VarInt.
pub fn set_carried_item_payload(slot: u16) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SET_CARRIED_ITEM).write_u16(slot);
    w.into_bytes()
}

pub fn close_container_payload(sync_id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONTAINER_CLOSE).write_varint(sync_id);
    w.into_bytes()
}

// Klick-Modus (SlotActionType).
pub const CLICK_MODE_PICKUP: i32 = 0;
pub const CLICK_MODE_QUICK_MOVE: i32 = 1;
pub const CLICK_MODE_SWAP: i32 = 2;
pub const CLICK_MODE_CLONE: i32 = 3;
pub const CLICK_MODE_THROW: i32 = 4;
pub const CLICK_MODE_QUICK_CRAFT: i32 = 5;
pub const CLICK_MODE_PICKUP_ALL: i32 = 6;

// ClickSlotC2SPacket. Seit 1.21.5 beschreibt der Client zu jedem Klick, WIE das Ergebnis
// seiner Meinung nach aussieht: fuer jeden Slot, den er selbst veraendert glaubt, und fuer
// den Cursor je ein "ItemStackHash". Der Server fuehrt den Klick ohnehin aus; die Hashes
// gehen nur in seine Abgleichspruefung (TrackedSlot::isInSync). Stimmen sie, schweigt er --
// stimmen sie nicht, schickt er den betroffenen Slot korrigierend nach.
//
// ItemStackHash (leer):     bool false
// ItemStackHash (gefuellt): bool true, varint item_id, varint count,
//                           varint n_added + je (varint type_id, i32 hash),
//                           varint n_removed + je varint type_id
//
// Den Prüfwert eines GESETZTEN Components koennen wir nicht bilden (er laeuft ueber
// Vanillas HashOps auf dem Component-INHALT, den item.rs bewusst nicht aufhebt) -- solche
// Stapel sind nicht exakt hashbar (s. ItemStack::hashable).
//
// Trotzdem wird auch fuer sie der GEFUELLTE Hash geschrieben, nur eben mit leerer
// Component-Liste. Das ist eine bewusst falsche Angabe: der Server bildet den Hash MIT
// seinen Components, findet eine Abweichung und schickt den Slot korrigierend nach -- genau
// das, was wir wollen, wenn wir nichts vorhersagen koennen.
//
// ECHTER BUG gewesen (nach dem Ablegen blieb der Mauszeiger noch das Item, etwa bei
// gefaerbter Lederruestung): frueher fiel dieser Fall auf den LEEREN Hash zurueck
// -- und "leer" heisst nicht "weiss ich nicht", sondern "der Cursor ist danach leer". Legt
// man ein solches Item ab, ist der Server-Cursor danach tatsaechlich leer, die Falschaussage
// stimmte also zufaellig, die Korrektur blieb aus, und das Item klebte lokal am Zeiger fest.
// "Leer" darf deshalb nur noch heissen, dass wirklich nichts da ist.
fn write_item_stack_hash(w: &mut PacketWriter, stack: Option<&ItemStack>) {
    match stack {
        Some(stack) => {
            w.write_bool(true).write_varint(stack.item_id as i32).write_varint(stack.count);
            // Gesetzte Components: bei hashbaren Stapeln gibt es tatsaechlich keine, bei den
            // uebrigen ist die leere Liste die absichtliche Abweichung von oben.
            w.write_varint(0);
            w.write_varint(stack.removed_components.len() as i32);
            for type_id in &stack.removed_components {
                w.write_varint(*type_id);
            }
        }
        None => {
            w.write_bool(false);
        }
    }
}

// `modified` sind die Slots, die der Client nach dem Klick veraendert erwartet (Slotnummer
// + vorhergesagter Inhalt), `cursor` der erwartete Stapel an der Maus. Wer nichts
// vorhersagen kann/will, uebergibt eine leere Liste und None -- dann schickt der Server
// nach jedem Klick den neuen Zustand, was ebenfalls korrekt ist, nur gespraechiger.
pub fn click_container_payload(
    sync_id: i32,
    revision: i32,
    slot: i16,
    button: i8,
    mode: i32,
    modified: &[(i16, Option<ItemStack>)],
    cursor: Option<&ItemStack>,
) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CONTAINER_CLICK)
        .write_varint(sync_id)
        .write_varint(revision)
        .write_u16(slot as u16)
        .write_u8(button as u8)
        .write_varint(mode)
        .write_varint(modified.len() as i32);
    for (slot, stack) in modified {
        w.write_u16(*slot as u16);
        write_item_stack_hash(&mut w, stack.as_ref());
    }
    write_item_stack_hash(&mut w, cursor);
    w.into_bytes()
}

#[cfg(test)]
mod chat_signing_tests {
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
}

#[cfg(test)]
mod inventory_tests {
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
}

#[cfg(test)]
mod custom_payload_tests {
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
}

#[cfg(test)]
mod tests {
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
        let packed = (x as i64 & 0xFFFF_FFFF) | ((z as i64 & 0xFFFF_FFFF) << 32);
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
        let packed = ((x as i64 & 0x3FF_FFFF) << 38) | ((z as i64 & 0x3FF_FFFF) << 12) | (y as i64 & 0xFFF);
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
        let packed = ((sx as i64 & 0x3F_FFFF) << 42) | (sy as i64 & 0xF_FFFF) | ((sz as i64 & 0x3F_FFFF) << 20);

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
}

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
#[derive(Clone, Debug, Default, PartialEq)]
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

/// Entity-ID und Animationsart. Die Art ist ein UNSIGNED BYTE, kein VarInt -- Vanilla schreibt
/// sie mit `writeByte` und liest sie mit `readUnsignedByte`.
pub fn read_entity_animation(payload: &[u8]) -> Result<(i32, u8), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let animation = r.read_u8()?;
    Ok((entity_id, animation))
}

// EntityDamageS2CPacket ("Damage Event"): der Server meldet, dass ein Entity Schaden genommen
// hat. Das ist die EINZIGE Quelle fuer Vanillas rotes Aufblitzen -- ClientPlayNetworkHandler.
// onEntityDamage ruft Entity.onDamaged, und LivingEntity.onDamaged setzt daraufhin hurtTime.
//
// Die ID stammt wie bei CB_PLAY_ENTITY_EVENT aus der S2C-Registrierungsreihenfolge in
// PlayStateFactories, diesmal gegen ZEHN bekannte Pakete gegengeprueft (BLOCK_UPDATE 0x08,
// CONTAINER_SET_SLOT 0x14, DISCONNECT 0x20, ENTITY_EVENT 0x22, KEEP_ALIVE 0x2B,
// LEVEL_CHUNK_WITH_LIGHT 0x2C, PLAYER_POSITION 0x46, SET_HEALTH 0x66, SYSTEM_CHAT 0x77,
// TELEPORT_ENTITY 0x7B -- alle zehn treffen). Wichtig beim Nachzaehlen: die Liste enthaelt
// nicht nur `PlayPackets.*`, sondern auch `CommonPackets.*` und `CookiePackets.*` an ihrer
// jeweiligen Stelle -- wer die ueberspringt, zaehlt sich um neun Positionen daneben.
pub const CB_PLAY_DAMAGE_EVENT: i32 = 0x19;

/// Entity-ID und Schadensart.
///
/// Die Schadensart ist eine schlichte VarInt-Registry-ID -- `DamageType.ENTRY_PACKET_CODEC` ist
/// `PacketCodecs.registryEntry(RegistryKey)` in der EINARGUMENTIGEN Form, also OHNE
/// Inline-Codec und damit ohne die +1-Verschiebung der Klang-IDs (am Bytecode geprueft; ein
/// frueherer Kommentar hier vermutete das Gegenteil). Sie zeigt in die vom Server geschickte
/// `minecraft:damage_type`-Registry, s. `read_damage_types`.
///
/// Gebraucht wird sie fuer den EIGENEN Schadenston: den schickt der Server bewusst nicht mit
/// (s. App::play_own_hurt_sound), der Client muss ihn selbst spielen -- und welcher es ist,
/// haengt an der Schadensart.
///
/// Danach folgen noch Verursacher- und Quell-ID (beide als VarInt+1, damit -1 kodierbar
/// bleibt) und optional eine Position. Die zaehlen erst fuer Todesnachrichten und
/// Knockback-Richtung und werden weiterhin nicht gelesen.
pub fn read_damage_event(payload: &[u8]) -> Result<(i32, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let damage_type = r.read_varint()?;
    Ok((entity_id, damage_type))
}

/// Welchen Klang eine Schadensart ausloest -- Vanillas `DamageEffects`.
///
/// Die Werte kommen als Zeichenkette im Feld `effects` der Schadensart-Registry; fehlt es,
/// gilt `Hurt`. Beachtenswert: `Thorns` hat KEINEN eigenen Klang, es teilt sich den mit
/// `Hurt` (am Bytecode von DamageEffects abgelesen, nicht angenommen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DamageEffect {
    #[default]
    Hurt,
    Thorns,
    Drowning,
    Burning,
    Poking,
    Freezing,
}

impl DamageEffect {
    pub fn from_name(name: &str) -> DamageEffect {
        match name {
            "thorns" => DamageEffect::Thorns,
            "drowning" => DamageEffect::Drowning,
            "burning" => DamageEffect::Burning,
            "poking" => DamageEffect::Poking,
            "freezing" => DamageEffect::Freezing,
            // Auch fuer unbekannte Werte: Vanillas Standard ist HURT, und ein neuer Effekt in
            // einer spaeteren Version soll den Ton nicht verstummen lassen.
            _ => DamageEffect::Hurt,
        }
    }

    /// Der Klang, den ein SPIELER dabei von sich gibt (PlayerEntity::getHurtSound ->
    /// DamageType.effects().getSound()).
    pub fn player_hurt_sound(self) -> &'static str {
        match self {
            DamageEffect::Hurt | DamageEffect::Thorns => "minecraft:entity.player.hurt",
            DamageEffect::Drowning => "minecraft:entity.player.hurt_drown",
            DamageEffect::Burning => "minecraft:entity.player.hurt_on_fire",
            DamageEffect::Poking => "minecraft:entity.player.hurt_sweet_berry_bush",
            DamageEffect::Freezing => "minecraft:entity.player.hurt_freeze",
        }
    }
}

/// Die Schadensart-Registry aus den Konfigurationsdaten lesen -- in REGISTRY-REIHENFOLGE, denn
/// das Schadenspaket nennt sie als Index in genau diese Liste.
///
/// `None` = dieses Registry-Paket war eine andere Registry (es kommen viele).
pub fn read_damage_types(payload: &[u8]) -> Result<Option<Vec<DamageEffect>>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    let count = r.read_varint()? as usize;

    if registry != "minecraft:damage_type" {
        return Ok(None);
    }

    let mut effects = Vec::with_capacity(count);
    for _ in 0..count {
        let _name = r.read_string()?;
        let has_data = r.read_bool()?;
        let data = if has_data { nbt::read_root(&mut r)? } else { None };
        // Fehlendes `effects` heisst HURT -- so steht es in Vanillas Codec
        // (optionalFieldOf("effects", HURT)), es ist kein Ausweichwert von uns.
        let effect = data.as_ref().and_then(|d| d.get("effects")).and_then(|v| v.as_str()).map(DamageEffect::from_name).unwrap_or_default();
        effects.push(effect);
    }
    Ok(Some(effects))
}

/// Die Verzauberungs-Registry aus den Konfigurationsdaten: je Eintrag der Name und die
/// vollstaendige Definition (Beschreibung, Maximalstufe, Effekte). Die POSITION in der Liste
/// ist die ID, mit der Items ihre Verzauberungen nennen (s. item::Enchantment).
///
/// Die Definition kommt immer mit, weil dieser Client keine bekannten Packs meldet (s.
/// serverbound_known_packs_empty_payload) -- sonst schickte der Server nur die Namen.
///
/// `None` = dieses Registry-Paket war eine andere Registry.
pub fn read_enchantments(payload: &[u8]) -> Result<Option<Vec<(String, Option<nbt::NbtValue>)>>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    if registry != "minecraft:enchantment" {
        return Ok(None);
    }
    let count = r.read_varint()?.max(0) as usize;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let name = r.read_string()?;
        let data = if r.read_bool()? { nbt::read_root(&mut r)? } else { None };
        out.push((name, data));
    }
    Ok(Some(out))
}

/// Die Registries, deren Eintraege nur als NAMEN gebraucht werden (Items nennen sie per
/// Position): Trim-Materialien, Trim-Muster, Bannermuster. Liefert (Registry, je Eintrag Name
/// und Definition) -- `None` fuer jede andere Registry.
pub fn read_named_registry(payload: &[u8]) -> Result<Option<(String, Vec<(String, Option<nbt::NbtValue>)>)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let registry = r.read_string()?;
    if !matches!(registry.as_str(), "minecraft:trim_material" | "minecraft:trim_pattern" | "minecraft:banner_pattern" | "minecraft:jukebox_song" | "minecraft:cat_variant" | "minecraft:painting_variant") {
        return Ok(None);
    }
    let count = r.read_varint()?.max(0) as usize;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let name = r.read_string()?;
        let data = if r.read_bool()? { nbt::read_root(&mut r)? } else { None };
        out.push((name, data));
    }
    Ok(Some((registry, out)))
}

#[cfg(test)]
mod animate_tests {
    use super::*;

    #[test]
    // Die Animationsart ist ein einzelnes BYTE hinter einer VarInt-Entity-ID. Als VarInt
    // gelesen ginge es fuer die Werte 0..5 zufaellig gut -- bis zu einer ID mit gesetztem
    // Fortsetzungsbit, ab der alles verrutscht.
    fn liest_id_als_varint_und_die_art_als_byte() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_ANIMATE);
        w.write_varint(300);
        w.write_u8(ANIMATION_SWING_OFF_HAND);
        assert_eq!(read_entity_animation(&w.into_bytes()).expect("lesbar"), (300, ANIMATION_SWING_OFF_HAND));
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_entity_animation(&[CB_PLAY_ANIMATE as u8]).is_err());
    }

    #[test]
    // Die 1 ist in Vanilla nicht vergeben. Steht sie hier doch in der Liste, hat jemand die
    // Werte durchnummeriert statt sie abzuschreiben -- und ab der 1 waere alles verschoben.
    fn die_luecke_bei_eins_bleibt_eine_luecke() {
        for art in [ANIMATION_SWING_MAIN_HAND, ANIMATION_WAKE_UP, ANIMATION_SWING_OFF_HAND, ANIMATION_CRIT, ANIMATION_ENCHANTED_HIT] {
            assert_ne!(art, 1, "1 ist in EntityAnimationS2CPacket unbelegt");
        }
        assert_eq!(ANIMATION_SWING_MAIN_HAND, 0);
        assert_eq!(ANIMATION_SWING_OFF_HAND, 3);
    }
}

#[cfg(test)]
mod damage_event_tests {
    use super::*;

    #[test]
    // Die Entity-ID ist hier ein VarInt -- anders als beim Entity-Event daneben, das sie als
    // festen i32 schreibt. Eine als i32 gelesene VarInt-ID waere stiller Muell.
    fn liest_die_entity_id_als_varint() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_DAMAGE_EVENT);
        w.write_varint(300);
        w.write_varint(7); // Schadensart
        assert_eq!(read_damage_event(&w.into_bytes()).expect("lesbar"), (300, 7));
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_damage_event(&[CB_PLAY_DAMAGE_EVENT as u8]).is_err());
    }

    // Die Zuordnung stammt aus Vanillas DamageEffects-Enum. Besonders `thorns`: es hat KEINEN
    // eigenen Klang, sondern denselben wie ein gewoehnlicher Treffer -- das sieht wie ein
    // Fehler aus und ist keiner.
    #[test]
    fn die_schadensarten_treffen_vanillas_klaenge() {
        use DamageEffect::*;
        assert_eq!(Hurt.player_hurt_sound(), "minecraft:entity.player.hurt");
        assert_eq!(Thorns.player_hurt_sound(), "minecraft:entity.player.hurt");
        assert_eq!(Drowning.player_hurt_sound(), "minecraft:entity.player.hurt_drown");
        assert_eq!(Burning.player_hurt_sound(), "minecraft:entity.player.hurt_on_fire");
        assert_eq!(Poking.player_hurt_sound(), "minecraft:entity.player.hurt_sweet_berry_bush");
        assert_eq!(Freezing.player_hurt_sound(), "minecraft:entity.player.hurt_freeze");
    }

    // Ein unbekannter Effekt darf nicht verstummen, sondern faellt auf HURT zurueck -- sonst
    // waere der eigene Schadenston nach dem naechsten Vanilla-Update weg.
    #[test]
    fn ein_unbekannter_effekt_faellt_auf_hurt_zurueck() {
        assert_eq!(DamageEffect::from_name("gibtsnochnicht"), DamageEffect::Hurt);
        assert_eq!(DamageEffect::default(), DamageEffect::Hurt);
    }

    // Die Registry kommt in Reihenfolge; der Index aus dem Schadenspaket zeigt genau hierhin.
    // Ein Eintrag ohne `effects` ist HURT (Vanillas optionalFieldOf-Standard), einer ganz ohne
    // Daten ebenfalls -- beides kommt in echten Registry-Paketen vor.
    #[test]
    fn die_schadensart_registry_wird_in_reihenfolge_gelesen() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_CONFIG_REGISTRY_DATA).write_string("minecraft:damage_type").write_varint(3);
        w.write_string("minecraft:in_fire").write_bool(true);
        nbt_mit_effekt(&mut w, "burning");
        w.write_string("minecraft:generic").write_bool(true);
        nbt_ohne_effekt(&mut w);
        w.write_string("minecraft:drown").write_bool(false);
        let effekte = read_damage_types(&w.into_bytes()).expect("lesbar").expect("ist die Schadensart-Registry");
        assert_eq!(effekte, vec![DamageEffect::Burning, DamageEffect::Hurt, DamageEffect::Hurt]);
    }

    // Es kommen viele Registry-Pakete -- die anderen muessen sauber None ergeben.
    #[test]
    fn eine_andere_registry_wird_uebergangen() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_CONFIG_REGISTRY_DATA).write_string("minecraft:worldgen/biome").write_varint(0);
        assert_eq!(read_damage_types(&w.into_bytes()).expect("lesbar"), None);
    }

    // Ein NBT-Compound mit genau einem String-Feld "effects" (Netzwerkform: kein Wurzelname).
    //
    // NICHT write_bytes benutzen: das ist Minecrafts "Prefixed Array of Byte" und schreibt
    // eine VarInt-Laenge davor. NBT-Zeichenketten haben eine u16-Laenge und danach die
    // ROHBYTES -- mit write_bytes stuende dort eine Laenge zu viel.
    fn nbt_string(w: &mut PacketWriter, text: &str) {
        w.write_u16(text.len() as u16);
        for b in text.bytes() {
            w.write_u8(b);
        }
    }

    fn nbt_mit_effekt(w: &mut PacketWriter, effekt: &str) {
        w.write_u8(10); // TAG_Compound
        w.write_u8(8); // TAG_String
        nbt_string(w, "effects");
        nbt_string(w, effekt);
        w.write_u8(0); // TAG_End
    }

    fn nbt_ohne_effekt(w: &mut PacketWriter) {
        w.write_u8(10);
        w.write_u8(0);
    }

    #[test]
    // Beide Pakete betreffen Entities und liegen im Strom dicht beieinander -- eine
    // Verwechslung der IDs waere teuer zu finden.
    fn ist_ein_anderes_paket_als_das_entity_event() {
        assert_ne!(CB_PLAY_DAMAGE_EVENT, CB_PLAY_ENTITY_EVENT);
    }
}

// EntityId ist hier ein fester i32, KEIN VarInt (EntityStatusS2CPacket: buf.writeInt) --
// eines der wenigen Pakete, die aus der VarInt-Regel ausbrechen.
pub fn read_entity_event(payload: &[u8]) -> Result<(i32, i8), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_i32()?;
    let status = r.read_u8()? as i8;
    Ok((entity_id, status))
}

#[cfg(test)]
mod entity_event_tests {
    use super::*;

    #[test]
    // Der Aufbau des Pakets, und vor allem: die EntityId ist ein fester i32. Liest man sie
    // als VarInt, kommt bei kleinen IDs zufaellig das Richtige heraus und ab EntityId 128
    // dauerhaft Muell -- ein Fehler, der erst auf einem laufenden Server auffaellt.
    fn liest_id_als_festen_i32_und_den_status() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_ENTITY_EVENT);
        w.write_i32(300);
        w.write_u8(ENTITY_STATUS_USE_TOTEM as u8);
        let (id, status) = read_entity_event(&w.into_bytes()).expect("lesbar");
        assert_eq!(id, 300, "EntityId 300 braucht 4 feste Bytes -- als VarInt gelesen waere es 172");
        assert_eq!(status, ENTITY_STATUS_USE_TOTEM);
    }

    #[test]
    // Statuswerte sind VORZEICHENBEHAFTET (byte). Vanilla nutzt auch negative (z.B. -1 gibt
    // es nicht, aber Werte > 127 gaebe es als negative Bytes) -- als u8 gelesen waere ein
    // Vergleich mit einer negativen Konstante nie wahr.
    fn status_ist_vorzeichenbehaftet() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_ENTITY_EVENT);
        w.write_i32(1);
        w.write_u8(0xFF);
        let (_, status) = read_entity_event(&w.into_bytes()).expect("lesbar");
        assert_eq!(status, -1);
    }

    #[test]
    fn abgeschnittenes_paket_ist_ein_fehler() {
        assert!(read_entity_event(&[0x22, 0x00]).is_err());
    }
}

// --- Befehls-Vervollstaendigung (Tab) ------------------------------------------------------
//
// Die Vorschlaege kommen vom SERVER, nicht aus einem lokalen Befehlsbaum: der Client fragt
// mit dem bisher getippten Text an, der Server antwortet mit den passenden Ergaenzungen.
// Das deckt Befehlsnamen UND Argumente ab (Spielernamen, Item-IDs, Warps eines Plugins) --
// ein lokal nachgebauter Baum koennte das Zweite gar nicht wissen.
//
// IDs: C2S ist die 0-basierte Position in der C2S-Registrierung (COMMAND_SUGGESTION an
// Position 15 der 1-basierten Liste -> 0x0E), S2C die 1-basierte (COMMAND_SUGGESTIONS an 15
// -> 0x0F; die S2C-Liste hat bei 0 den Bundle-Delimiter). Beide Konventionen gegen vorhandene,
// nachweislich funktionierende Konstanten geprueft: SB_PLAY_CHAT (0x08, Position 9) und
// SB_PLAY_CHAT_COMMAND (0x06, Position 7) fuer C2S, CONTAINER_SET_SLOT (0x14, Position 20)
// fuer S2C.
pub const SB_PLAY_COMMAND_SUGGESTION: i32 = 0x0E;
pub const CB_PLAY_COMMAND_SUGGESTIONS: i32 = 0x0F;

// RequestCommandCompletionsC2SPacket: VarInt-Transaktions-ID + der komplette getippte Text.
//
// `text` enthaelt den fuehrenden Schraegstrich. Das ist nicht willkuerlich: Vanilla schickt
// `context.getInput()`, und der Brigadier-Reader wird mit dem GANZEN Feldinhalt gebaut (der
// skip() danach bewegt nur den Cursor, nicht den Puffer). Genauso wichtig fuer die Antwort:
// deren `start` ist ein Offset in eben diesen String -- schickt man ihn ohne Schraegstrich,
// landet jede Ergaenzung um ein Zeichen verschoben.
pub fn command_suggestion_payload(completion_id: i32, text: &str) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_COMMAND_SUGGESTION).write_varint(completion_id).write_string(text);
    w.into_bytes()
}

// CommandSuggestionsS2CPacket: die Antwort. `start`/`length` bezeichnen den Bereich im
// angefragten Text, der durch einen Vorschlag ERSETZT wird -- nicht das, was man anhaengt.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSuggestions {
    pub id: i32,
    pub start: i32,
    pub length: i32,
    pub matches: Vec<String>,
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub fn from_id(id: i32) -> Option<SoundCategory> {
        Some(match id {
            0 => SoundCategory::Master,
            1 => SoundCategory::Music,
            2 => SoundCategory::Records,
            3 => SoundCategory::Weather,
            4 => SoundCategory::Blocks,
            5 => SoundCategory::Hostile,
            6 => SoundCategory::Neutral,
            7 => SoundCategory::Players,
            8 => SoundCategory::Ambient,
            9 => SoundCategory::Voice,
            10 => SoundCategory::Ui,
            _ => return None,
        })
    }
}

/// Ein abzuspielendes Geraeusch, so wie es vom Server kommt.
#[derive(Debug, Clone, PartialEq)]
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

fn read_sound_event(r: &mut PacketReader) -> Result<Option<String>, ReadError> {
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

/// PlaySoundS2CPacket lesen.
///
/// `Ok(None)` = Paket war formal in Ordnung, aber der Klang ist uns unbekannt (s.
/// read_sound_event).
pub fn read_sound(payload: &[u8]) -> Result<Option<SoundEffect>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let name = read_sound_event(&mut r)?;
    let category = r.read_varint()?;
    // Die Position steckt als GANZE ZAHL in Achteln eines Blocks drin (Vanilla:
    // `(int)(x * 8.0)`), nicht als Gleitkomma. Das reicht voellig -- ein Achtel Block ist
    // weit unter dem, was sich raeumlich unterscheiden laesst -- spart aber gegenueber drei
    // f64 zwoelf Byte pro Geraeusch.
    let x = r.read_i32()? as f64 / 8.0;
    let y = r.read_i32()? as f64 / 8.0;
    let z = r.read_i32()? as f64 / 8.0;
    let volume = r.read_f32()?;
    let pitch = r.read_f32()?;
    let seed = r.read_i64()?;
    let Some(name) = name else { return Ok(None) };
    let Some(category) = SoundCategory::from_id(category) else { return Ok(None) };
    Ok(Some(SoundEffect { name, category, position: Some((x, y, z)), entity_id: None, volume, pitch, seed }))
}

/// PlaySoundFromEntityS2CPacket lesen. Wie `read_sound`, nur dass statt der Position eine
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

/// Clientbound Stop Sound -- direkt hinter START_CONFIGURATION (0x74), vor STORE_COOKIE (0x76);
/// im 1.21.11-Client (GameProtocols) nachgesehen.
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
/// lokalisiert hat (s. CORAL_PACKET_SCAN in net::handle_play_packet).
///
/// Der Test ist bewusst STRENG, sonst meldet er falsche Treffer: das Paket muss restlos
/// aufgehen (kein Byte bleibt uebrig), der Klang muss ein bekannter Registry-Eintrag sein, und
/// Lautstaerke und Tonhoehe muessen in den Bereichen liegen, die Vanilla tatsaechlich schickt.
/// Ein zufaellig passendes Paket ueberlebt diese vier Huerden praktisch nie.
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

// (Kanal, Daten) einer Plugin-Nachricht -- im Play- wie im Configuration-State dasselbe Layout.
pub fn read_custom_payload(payload: &[u8]) -> Result<(String, Vec<u8>), ReadError> {
    let mut r = PacketReader::new(payload);
    r.read_varint()?;
    let channel = r.read_string()?;
    Ok((channel, r.remaining().to_vec()))
}

pub fn custom_payload(channel: &str, data: &[u8]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CUSTOM_PAYLOAD).write_string(channel).write_raw_bytes(data);
    w.into_bytes()
}

// "minecraft:register": die Kanaele, auf denen wir Plugin-Nachrichten annehmen, durch NUL
// getrennt. Bukkit/Paper schicken einem Client NUR auf angemeldeten Kanaelen etwas
// (Player.sendPluginMessage prueft getListeningPluginChannels) -- ohne diese Meldung kaeme
// z.B. ein Opt-out eines Paper-Plugins nie an. Fabric API meldet seine Kanaele genauso an.
pub fn register_channels_payload(channels: &[&str]) -> Vec<u8> {
    custom_payload("minecraft:register", channels.join("\0").as_bytes())
}

pub fn read_cooldown(payload: &[u8]) -> Result<(String, i32), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let group = r.read_string()?;
    let ticks = r.read_varint()?;
    Ok((group, ticks))
}

// --- Cookies (CookiePackets: Cookie Request / Cookie Response / Store Cookie) ---
//
// Ein Server kann in JEDEM Zustand (Login, Configuration, Play) nach einem Cookie fragen; der
// Vanilla-Client antwortet IMMER, notfalls mit "habe ich nicht". Genau darauf bauen Proxys und
// Anticheat-Plugins ihre "Verbindungstests": sie legen dem Client per Store Cookie eine Marke
// ab und fragen sie beim naechsten Verbindungsaufbau wieder ab. Wer nicht antwortet, faellt
// durch -- fuer den Server sieht es aus, als ob das Paket im Nichts verschwindet.
//
// IDs aus dem Yarn-gemappten 1.21.11-Jar (LoginStates, ConfigurationStates, PlayStateFactories),
// Registrierungsindex je Zustand -- im Play-S2C-Zustand mit dem +1 durch `addBundle` an
// Index 0, wie bei allen anderen CB_PLAY_*-Konstanten hier.
pub const CB_LOGIN_COOKIE_REQUEST: i32 = 0x05;
pub const SB_LOGIN_COOKIE_RESPONSE: i32 = 0x04;
pub const CB_CONFIG_COOKIE_REQUEST: i32 = 0x00;
pub const CB_CONFIG_STORE_COOKIE: i32 = 0x0A;
pub const SB_CONFIG_COOKIE_RESPONSE: i32 = 0x01;
pub const CB_PLAY_COOKIE_REQUEST: i32 = 0x15;
pub const CB_PLAY_STORE_COOKIE: i32 = 0x76;
pub const SB_PLAY_COOKIE_RESPONSE: i32 = 0x14;

// Transfer (seit 1.20.5): der Server schickt uns zu einem anderen Host -- Netzwerke wie
// mcpvp.club verteilen so auf Regionen. Der Client trennt, verbindet sich neu (Handshake mit
// INTENT_TRANSFER) und nimmt dabei die Cookies mit. Wer das Paket ignoriert, bleibt einfach
// stehen, bis der Server ihn wegen des nie angekommenen Transfers kickt.
// Registrierungsreihenfolge: Config zwischen STORE_COOKIE (0x0A) und UPDATE_ENABLED_FEATURES,
// Play zwischen TICKING_STEP und UPDATE_ADVANCEMENTS (TELEPORT_ENTITY 0x7B + 4).
pub const CB_CONFIG_TRANSFER: i32 = 0x0B;
pub const CB_PLAY_TRANSFER: i32 = 0x7F;
pub const INTENT_TRANSFER: i32 = 3;

// Transfer: Host (String) + Port (VarInt). Ein Port ausserhalb von u16 ist ein kaputtes Paket.
pub fn read_transfer(payload: &[u8]) -> Result<(String, u16), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let host = r.read_string()?;
    let port = u16::try_from(r.read_varint()?).map_err(|_| ReadError)?;
    Ok((host, port))
}

// StoreCookieS2CPacket.MAX_COOKIE_LENGTH. Groesseres verwirft Vanilla, statt es abzulegen.
pub const MAX_COOKIE_LEN: usize = 5120;

// Der Schluessel eines Cookie Request: nur ein Identifier hinter der Paket-ID.
pub fn read_cookie_key(payload: &[u8]) -> Result<String, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_string()
}

// Store Cookie: Identifier + Byte-Array (VarInt-Laenge). Zu lange Cookies liefern None --
// dieselbe Entscheidung wie in Vanilla, nur ohne Ausnahme.
pub fn read_store_cookie(payload: &[u8]) -> Result<Option<(String, Vec<u8>)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let key = r.read_string()?;
    let len = r.read_varint()?;
    if len < 0 || len as usize > MAX_COOKIE_LEN {
        return Ok(None);
    }
    let bytes = r.read_bytes(len as usize)?.to_vec();
    Ok(Some((key, bytes)))
}

// Die Antwort: Identifier + Optional-Byte-Array. `None` ist KEIN Weglassen, sondern ein
// gesetztes false-Byte -- ein "kenne ich nicht", das der Server auch als solches liest.
pub fn cookie_response_payload(packet_id: i32, key: &str, value: Option<&[u8]>) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(packet_id).write_string(key);
    match value {
        Some(bytes) => {
            // write_bytes IST das "Prefixed Array of Byte" (VarInt-Laenge + Rohbytes) --
            // keine eigene Laenge davor, die stuende sonst doppelt im Strom.
            w.write_bool(true).write_bytes(bytes);
        }
        None => {
            w.write_bool(false);
        }
    }
    w.into_bytes()
}

#[cfg(test)]
mod cookie_tests {
    use super::*;

    #[test]
    fn cooldown_paket_liefert_gruppe_und_ticks() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_COOLDOWN).write_string("minecraft:shield").write_varint(100);
        assert_eq!(read_cooldown(&w.into_bytes()).unwrap(), ("minecraft:shield".to_string(), 100));
    }

    #[test]
    fn ein_gespeichertes_cookie_kommt_unveraendert_zurueck() {
        let store = store_cookie_bytes("ac:pass", &[1, 2, 3, 250]);
        let (key, value) = read_store_cookie(&store).unwrap().unwrap();
        assert_eq!(key, "ac:pass");
        assert_eq!(value, vec![1, 2, 3, 250]);

        let antwort = cookie_response_payload(SB_CONFIG_COOKIE_RESPONSE, &key, Some(&value));
        let mut r = PacketReader::new(&antwort);
        assert_eq!(r.read_varint().unwrap(), SB_CONFIG_COOKIE_RESPONSE);
        assert_eq!(r.read_string().unwrap(), "ac:pass");
        assert!(r.read_bool().unwrap(), "vorhandenes Cookie muss als vorhanden markiert sein");
        let len = r.read_varint().unwrap();
        assert_eq!(r.read_bytes(len as usize).unwrap(), &[1, 2, 3, 250]);
    }

    #[test]
    // "Kenne ich nicht" ist eine ECHTE Antwort -- ein false-Byte, kein Schweigen.
    fn ein_unbekanntes_cookie_wird_als_leer_beantwortet() {
        let antwort = cookie_response_payload(SB_PLAY_COOKIE_RESPONSE, "ac:pass", None);
        let mut r = PacketReader::new(&antwort);
        assert_eq!(r.read_varint().unwrap(), SB_PLAY_COOKIE_RESPONSE);
        assert_eq!(r.read_string().unwrap(), "ac:pass");
        assert!(!r.read_bool().unwrap());
        assert!(r.remaining().is_empty(), "nach dem false-Byte darf nichts mehr folgen");
    }

    #[test]
    fn der_schluessel_eines_cookie_request_wird_gelesen() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_CONFIG_COOKIE_REQUEST).write_string("velocity:player_info");
        assert_eq!(read_cookie_key(&w.into_bytes()).unwrap(), "velocity:player_info");
    }

    #[test]
    // Ein zu grosses Cookie wird verworfen statt abgelegt -- sonst laesst sich der Client als
    // Speicher fuer beliebige Daten missbrauchen.
    fn zu_grosse_cookies_werden_verworfen() {
        let store = store_cookie_bytes("ac:pass", &vec![0u8; MAX_COOKIE_LEN + 1]);
        assert!(read_store_cookie(&store).unwrap().is_none());
    }

    #[test]
    fn transfer_liefert_host_und_port() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_TRANSFER).write_string("frankfurt.example.net").write_varint(25577);
        assert_eq!(read_transfer(&w.into_bytes()).unwrap(), ("frankfurt.example.net".to_string(), 25577));
        let mut w = PacketWriter::new();
        w.write_varint(CB_CONFIG_TRANSFER).write_string("x").write_varint(70000);
        assert!(read_transfer(&w.into_bytes()).is_err(), "Port ueber u16 ist kaputt");
    }

    #[test]
    fn transfer_liegt_zwischen_seinen_nachbarn() {
        assert_eq!(CB_CONFIG_TRANSFER, CB_CONFIG_STORE_COOKIE + 1);
        assert_eq!(CB_PLAY_TRANSFER, CB_PLAY_TELEPORT_ENTITY + 4);
        assert_eq!(CB_PLAY_TRANSFER, CB_PLAY_UPDATE_ATTRIBUTES - 2);
    }

    // Baut ein Store-Cookie-Paket so, wie der Server es schickt.
    fn store_cookie_bytes(key: &str, value: &[u8]) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_varint(CB_CONFIG_STORE_COOKIE).write_string(key).write_bytes(value);
        w.into_bytes()
    }
}

// --- Server-Resourcepacks ------------------------------------------------------------------
//
// IDs gegen ConfigurationStates/PlayStateFactories (1.21.11) gezaehlt, abgeglichen an den schon
// verifizierten Nachbarn (Cookie, Keep Alive, Player Loaded). Der Server schickt "Push" mit
// Adresse und SHA-1, der Client meldet ueber "Status" jeden Schritt; "Pop" nimmt ein Pack (oder
// ohne UUID alle) wieder weg.
pub const CB_CONFIG_RESOURCE_PACK_POP: i32 = 0x08;
pub const CB_CONFIG_RESOURCE_PACK_PUSH: i32 = 0x09;
pub const SB_CONFIG_RESOURCE_PACK: i32 = 0x06;
pub const CB_PLAY_RESOURCE_PACK_POP: i32 = 0x4E;
pub const CB_PLAY_RESOURCE_PACK_PUSH: i32 = 0x4F;
pub const SB_PLAY_RESOURCE_PACK: i32 = 0x30;

#[derive(Clone, Debug, PartialEq)]
pub struct ResourcePackPush {
    pub id: [u8; 16],
    pub url: String,
    // SHA-1 als Hex (40 Zeichen) oder leer.
    pub hash: String,
    pub required: bool,
    pub prompt: Option<Vec<crate::nbt::TextSpan>>,
}

pub fn read_resource_pack_push(payload: &[u8]) -> Result<ResourcePackPush, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let id = r.read_uuid()?.to_bytes();
    let url = r.read_string()?;
    let hash = r.read_string()?;
    let required = r.read_bool()?;
    let prompt = if r.read_bool()? { Some(read_text_spans(&mut r)?) } else { None };
    Ok(ResourcePackPush { id, url, hash, required, prompt })
}

// None = alle Packs entfernen.
pub fn read_resource_pack_pop(payload: &[u8]) -> Result<Option<[u8; 16]>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    Ok(if r.read_bool()? { Some(r.read_uuid()?.to_bytes()) } else { None })
}

// Status-Werte (ResourcePackStatusC2SPacket.Status, Reihenfolge = Wert).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourcePackStatus {
    SuccessfullyLoaded = 0,
    Declined = 1,
    FailedDownload = 2,
    Accepted = 3,
    Downloaded = 4,
    InvalidUrl = 5,
    FailedReload = 6,
    Discarded = 7,
}

// Mit der Play-ID gebaut; im Configuration-State tauscht net die ID aus (s.
// config_resource_pack_status).
pub fn resource_pack_status_payload(id: [u8; 16], status: ResourcePackStatus) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_RESOURCE_PACK).write_raw_bytes(&id).write_varint(status as i32);
    w.into_bytes()
}

// Eine Status-Antwort in Play-Form fuer den Configuration-State umschreiben; None, wenn das
// Paket keine Status-Antwort ist.
pub fn config_resource_pack_status(play_payload: &[u8]) -> Option<Vec<u8>> {
    let mut r = PacketReader::new(play_payload);
    if r.read_varint().ok()? != SB_PLAY_RESOURCE_PACK {
        return None;
    }
    let id = r.read_uuid().ok()?.to_bytes();
    let status = r.read_varint().ok()?;
    let mut w = PacketWriter::new();
    w.write_varint(SB_CONFIG_RESOURCE_PACK).write_raw_bytes(&id).write_varint(status);
    Some(w.into_bytes())
}

#[cfg(test)]
mod resource_pack_tests {
    use super::*;

    #[test]
    fn push_und_status() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_RESOURCE_PACK_PUSH).write_raw_bytes(&[7; 16]).write_string("https://x/p.zip").write_string("ab").write_bool(true).write_bool(false);
        let p = read_resource_pack_push(&w.into_bytes()).unwrap();
        assert_eq!((p.id, p.url.as_str(), p.hash.as_str(), p.required, p.prompt), ([7; 16], "https://x/p.zip", "ab", true, None));

        let play = resource_pack_status_payload([7; 16], ResourcePackStatus::Downloaded);
        let config = config_resource_pack_status(&play).unwrap();
        assert_eq!(config[0] as i32, SB_CONFIG_RESOURCE_PACK);
        assert_eq!(&config[1..], &play[1..]);
        assert!(config_resource_pack_status(&[0x05, 1, 2]).is_none());

        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_RESOURCE_PACK_POP).write_bool(false);
        assert_eq!(read_resource_pack_pop(&w.into_bytes()).unwrap(), None);
    }
}
