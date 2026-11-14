// tokio-basierte TCP-Verbindung: Packet-Framing (VarInt-Laengenpraefix) und, sobald der
// Server es anfordert (Set Compression), symmetrische Zlib-Kompression.
//
// Deckt ab: Status-Ping (Meilenstein 1) und den vollstaendigen Join-Ablauf
// Login -> Configuration -> Play, damit der Spieler wirklich auf dem Server erscheint.

use std::io;
use std::time::Duration;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use protocol::packets::{self, Handshake, LoginDisconnect, LoginStart, LoginSuccess, SetCompression, StatusResponse};
use protocol::uuid::Uuid;
use protocol::varint::{decode_varint, encode_varint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
mod framing;
mod login_flow;
mod play;
mod session;
pub use framing::*;
use login_flow::*;
pub use play::*;
pub use session::*;

// Wie lange wir auf Daten vom Server warten, bevor wir die Verbindung als tot ansehen.
// Gleicher Wert wie Vanillas Netty-ReadTimeoutHandler in der Gegenrichtung: der Server
// schickt spaetestens alle 15s ein Keep Alive, 30s komplette Stille heissen also, dass
// mindestens eines ausgefallen ist. Ohne dieses Timeout wuerde recv() bei einem stumm
// gewordenen Server (Kabel raus, Rechner eingefroren) fuer immer haengen -- ein FIN, das
// die Verbindung von selbst beendet, kommt in dem Fall ja gerade nicht.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

// Obergrenze fuer die Laenge EINES Frames. Vanillas Varint21FrameDecoder liest die Laenge aus
// hoechstens 3 VarInt-Bytes und lehnt alles darueber als CorruptedFrame ab -- das sind 2^21-1
// Bytes. Jeder Server, der ueberhaupt mit einem Vanilla-Client reden kann, haelt das ein.
// Ohne den Deckel waechst rbuf unbegrenzt weiter, waehrend wir auf den Rumpf eines Frames
// warten, der bei einer erfundenen Laenge nie vollstaendig wird.
const MAX_FRAME_LEN: usize = (1 << 21) - 1;

// Obergrenze fuer die ENTPACKTE Groesse eines Pakets -- exakt Vanillas
// CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH (8 MiB). Ohne diesen Deckel bestimmt allein
// der Server, wie viel Speicher wir sofort anfordern: `data_len` kommt ungeprueft aus dem
// Netz, ein behaupteter Wert von 2 Mrd. ergibt eine 2-GB-Allokation, BEVOR auch nur ein Byte
// entpackt wurde (Dekompressions-Bombe). Ein negativer VarInt waere noch schlimmer -- `-1 as
// usize` ist usize::MAX und beendet den Prozess sofort. Beides muss ein sauberer
// Protokollfehler sein (nur diese Verbindung faellt weg), kein Absturz des ganzen Clients.
// Trifft nicht nur boesartige Server: ein einzelnes gekipptes Bit im Laengen-VarInt reicht.
const MAX_UNCOMPRESSED_LEN: i32 = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum NetError {
    Io(io::Error),
    Protocol(String),
    // Der Session-Server hat das Access-Token abgelehnt (403) -- s. join_refreshing.
    SessionRejected(String),
    Timeout,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO-Fehler: {e}"),
            Self::Protocol(msg) | Self::SessionRejected(msg) => write!(f, "Protokoll-Fehler: {msg}"),
            Self::Timeout => {
                write!(f, "Timed out ({}s keine Daten vom Server)", READ_TIMEOUT.as_secs())
            }
        }
    }
}

impl std::error::Error for NetError {}

impl From<io::Error> for NetError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

pub struct LoginResult {
    pub uuid: Uuid,
    pub username: String,
}

// Meldungen vom (async, im Hintergrund laufenden) Netzwerk-Task an die App, damit diese
// Verbindungsstatus + Log im Client-Fenster anzeigen kann (statt nur println! aufs Terminal).
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ClientEvent {
    // Spielerliste: UUID -> Name + Textur-Property (s. packets::parse_player_info_update).
    PlayerInfo(Vec<packets::PlayerInfoEntry>),
    Log(String),
    Connected,
    Disconnected(String),
    // Der Server schickt uns mitten im Spiel zurueck in den Configuration-State (s.
    // packets::CB_PLAY_START_CONFIGURATION). Danach kommt eine komplett neue Welt --
    // die alte muss weg, sonst stehen alte Chunks und Entities noch herum.
    Reconfiguring,
    // Stockung im Paketfluss (Rubberbanding, s. stall.rs): eingehend lange nichts und dann
    // alles auf einmal, oder ausgehend so lange nichts gesendet. Kommt vom Netz-Task selbst,
    // nicht vom Server.
    PacketStall { outgoing: bool, millis: u32 },
    // Ping des Servers im Spiel (Transaktion, s. app/transactions.rs) -- schon beantwortet.
    TransactionPing(i32),
    // KeepAlive beantwortet: so lange lag die Anfrage im Client, vom Lesen der Bytes am
    // Socket bis die Antwort geschrieben war (F3, Ping-Zeile).
    KeepAliveAnswered { micros: u32 },
    // Der Server schickt uns zu einem anderen Host (Transfer-Paket, s. packets::CB_PLAY_TRANSFER).
    // Die alte Verbindung ist zu, der Netz-Task verbindet sich gerade neu -- alles, was zur
    // alten Verbindung gehoert (Welt, Spielerliste, Skins, Chat-Signatur), muss weg. Die
    // Server-Resourcepacks bleiben: Vanilla raeumt sie bei einem Transfer nicht ab, der neue
    // Server bietet sie erneut an.
    Transferring { host: String, port: u16 },
    // Wohin die TCP-Verbindung tatsaechlich geht: "IP:Port", nach einem SRV-Eintrag
    // "Ziel/IP:Port" -- fuer die Verbindungsanzeige.
    Route(String),
    // Der Server hat uns neu aufgesetzt (Respawn/Dimensionswechsel). Vanilla baut dabei eine
    // neue ClientPlayerEntity -- alles, was "zuletzt gesendet" merkt, faengt von vorn an.
    // `world_change` = die Dimension im Paket ist eine andere als bisher (Portal, /tp in eine
    // andere Welt). Dann baut Vanilla zusaetzlich eine neue ClientWorld, und alles, was zur
    // WELT gehoert (Chunks, Entities, Block-Sequenznummer), faengt ebenfalls von vorn an --
    // ein Tod-Respawn in derselben Dimension tut das nicht (s. packets::CB_PLAY_RESPAWN).
    Respawned { world_change: bool },
    // Welche Welt das ist: Dimension und gehashter Seed aus Login bzw. Respawn (fuer das Modul
    // Chunk-Saver, das Chunks je Welt speichert). Kommt vor Respawned.
    WorldIdentity { dimension: String, seed: i64 },
    Chunk(protocol::chunk::ChunkData),
    // Ein Geraeusch (Sound 0x73 / Sound Entity 0x72). Wird im Event-Pfad abgespielt, NICHT im
    // Tick: ein Tick sind 50 ms, und ein Ton ist hier ein Reaktionssignal.
    Sound(packets::SoundEffect),
    // Stop Sound (0x75): laufende Geraeusche anhalten. Beide Filter leer = alle.
    StopSound { category: Option<u8>, name: Option<String> },
    // Vom Server gesetzte Spielerposition (Synchronize Player Position). Ohne die
    // wuesste der Client nicht, wo in der Welt er ueberhaupt steht. Yaw/Pitch (Grad, Vanilla-
    // Konvention) und Flags (s. packets::POSITION_FLAG_REL_*) -- X/Y/Z/Yaw/Pitch koennen je
    // einzeln relativ zum aktuellen Wert gemeint sein, nicht nur absolut.
    PlayerPosition(packets::PlayerPositionSync),
    // Server-Resourcepack: anbieten (Push) bzw. wieder entfernen (Pop, None = alle). Die
    // Antworten schickt die App ueber den Sendekanal (packets::resource_pack_status_payload).
    ResourcePackPush(packets::ResourcePackPush),
    // Himmelslicht einer Spalte hat sich geaendert (Update Light, s. protocol::chunk).
    LightUpdate { x: i32, z: i32, light: protocol::chunk::LightData },
    ResourcePackPop(Option<[u8; 16]>),
    // Server-Links (fuer das Pausemenue und den Dialog server_links), ersetzt die bisherigen.
    ServerLinks(Vec<packets::ServerLink>),
    // Einen Dialog zeigen bzw. den offenen schliessen (Configuration und Play).
    ShowDialog(packets::DialogRef),
    ClearDialog,
    // Verhaltenskodex des Servers (nur Configuration): die Konfiguration wartet, bis die App
    // ihn annimmt (packets::accept_code_of_conduct_payload) oder die Verbindung trennt.
    CodeOfConduct(String),
    // Reset Chat (Configuration): der Chatverlauf faengt nach der Konfiguration leer an.
    ResetChat,
    // Der Server dreht NUR unsere Blickrichtung (Player Rotation 0x47), ohne uns zu bewegen.
    // Bereits aufgeloest: beide Achsen absolut in Grad, Vanilla-Konvention. Das Aufloesen
    // passiert im Netz-Task, weil DORT auch die Bestaetigung rausgeht und beide denselben
    // Wert benutzen muessen (s. handle_play_packet).
    ServerRotation { yaw_deg: f32, pitch_deg: f32 },
    // Der Server laesst uns auf einen Punkt oder ein Entity schauen (Player Look At 0x45).
    // Die Richtung rechnet die App, sie kennt Entities und die eigene Augenhoehe.
    // Ziel-Entity: (ID, auf seine Augen); `target` gilt, wenn die App es nicht kennt.
    LookAt { from_eyes: bool, target: (f64, f64, f64), entity: Option<(i32, bool)> },
    // Server entlaedt eine Chunk-Spalte (Forget Level Chunk) -- der Client muss sie aus
    // Weltzustand und VRAM werfen, sonst wachsen beide beim Umherlaufen unbegrenzt.
    ForgetChunk(i32, i32),
    // Chunk-Paket kam, liess sich aber nicht dekodieren (Koordinaten aus den ersten 8 Bytes).
    ChunkUndecodable(i32, i32),
    // Entity betritt den Trackingbereich (Add Entity 0x01; seit 1.20.2 auch Spieler).
    EntitySpawn(packets::AddEntity),
    // Relative Bewegung (Move Entity Pos 0x33 / Pos+Rot 0x34): Delta zur letzten Position.
    EntityMoveRel { id: i32, dx: f64, dy: f64, dz: f64, rot: Option<(f32, f32)> },
    // Nur Blickrichtung (Move Entity Rot 0x36): Yaw/Pitch in Grad. Das yaw ist die
    // KOERPERdrehung -- die Kopfdrehung kommt getrennt (s. EntityHeadYaw).
    EntityRot { id: i32, yaw_deg: f32, pitch_deg: f32 },
    // Set Head Rotation (0x51): die Kopfdrehung eines Lebewesens, unabhaengig vom Koerper.
    // Ohne dieses Event schauen fremde Spieler stur in ihre Koerperrichtung.
    EntityHeadYaw { id: i32, head_yaw_deg: f32 },
    // Pose aus dem Metadaten-Strom (Set Entity Data 0x61). Entscheidet ueber die Hoehe der
    // Trefferbox -- ein schleichender Spieler ist 1.5 hoch, ein kriechender 0.6.
    EntityPose { id: i32, pose: u8 },
    // Der REST des Metadaten-Stroms: Sichtbarkeits-Flag, Namensschild und der Inhalt eines
    // Display-Entities. Anders als Pose/Luft/Schlafposition ist das kein einzelner Wert,
    // sondern ein Buendel optionaler Felder -- als eigenes Event je Feld waeren es ein
    // Dutzend fast gleicher Varianten, deshalb wandert die gelesene Struktur als Ganzes
    // durch (s. Entities::apply_metadata, das daraus nimmt, was zum Entity-Typ passt).
    EntityMeta(Box<packets::EntityDataUpdate>),
    // LivingEntity.SLEEPING_POSITION hat sich geaendert (s. packets::read_set_entity_data).
    // Some = liegt in diesem Bett, None = aufgewacht. Betrifft auch UNS SELBST -- der Server
    // schickt die Metadaten des eigenen Spielers ebenso, und nur daran erkennt der Client,
    // dass er schlaeft.
    EntitySleeping { id: i32, pos: Option<(i32, i32, i32)> },
    // Absolute Position (Entity Position Sync 0x23 / Teleport Entity 0x7B). `teleport`
    // unterscheidet beide: ein Teleport greift auch bei einem selbst gelenkten Fahrzeug.
    EntityPos { id: i32, x: f64, y: f64, z: f64, yaw_deg: f32, pitch_deg: f32, teleport: bool },
    // Bossbar hinzufuegen, aendern oder entfernen (Boss Event 0x09).
    BossEvent { uuid: [u8; 16], event: packets::BossEvent },
    // Der Server setzt das selbst gelenkte Fahrzeug zurueck (Move Vehicle 0x37).
    VehicleMove { pos: (f64, f64, f64), yaw_deg: f32, pitch_deg: f32 },
    // Entities verlassen den Trackingbereich (Remove Entities 0x4B).
    EntitiesRemove(Vec<i32>),
    // Was ein Entity in den Haenden haelt bzw. traegt (Set Equipment 0x64). Kommt nur bei
    // Aenderungen, also einmal nach dem Spawn und danach bei jedem Item-Wechsel -- der
    // Client MUSS den Stand halten, es gibt keine Abfrage.
    EntityEquipment { id: i32, entries: Vec<packets::EquipmentEntry> },
    // Leben des eigenen Spielers (Set Health 0x66). <= 0 bedeutet tot -- die App zeigt
    // dann den Gestorben-Screen.
    // Leben UND Nahrung: die Nahrung entscheidet ueber den Sprint (Vanilla:
    // HungerManager.canSprint -> foodLevel > 6). Dazu die Saettigung.
    Health(f32, i32, f32),
    // Set Experience (0x65): Fortschritt innerhalb der Stufe (0..1), Stufe, Gesamtpunkte.
    Experience { progress: f32, level: i32, total: i32 },
    // Update Attributes (0x81), reduziert auf das, was das HUD braucht: Ruestung und
    // Maximalgesundheit (s. packets::read_attributes). None = stand nicht im Paket.
    Attributes { id: i32, armor: Option<f64>, max_health: Option<f64> },
    // Die Abbau-Attribute (s. packets::AttributeUpdate).
    MiningAttributes { id: i32, block_break_speed: Option<f64>, mining_efficiency: Option<f64>, submerged_mining_speed: Option<f64> },
    // Die BEWEGUNGS-Attribute (s. packets::AttributeUpdate). Sie entscheiden ueber Grundtempo,
    // Schleichtempo (Schleichender Schritt) und Wassertempo (Wasserlaeufer) -- GrimAC rechnet
    // seine Vorhersage mit genau diesen drei Werten.
    MovementAttributes { id: i32, movement_speed: Option<f64>, sneaking_speed: Option<f64>, water_movement_efficiency: Option<f64> },
    // Angriffstempo ohne Item-Modifikator, zerlegt (s. packets::AttributeUpdate::attack_speed).
    AttackSpeed { id: i32, parts: (f64, f64, f64) },
    // Die Koerper-Attribute (s. packets::AttributeUpdate): Schwerkraft, Sprungkraft,
    // Blockbremsen-Aufhebung, Groesse, Stufenhoehe. Groesse gilt auch fuer fremde Entities.
    BodyAttributes {
        id: i32,
        gravity: Option<f64>,
        jump_strength: Option<f64>,
        movement_efficiency: Option<f64>,
        scale: Option<f64>,
        step_height: Option<f64>,
    },
    // Die Reichweiten fuer Bloecke und Entities (im Kreativmodus vom Server angehoben).
    RangeAttributes { id: i32, block: Option<f64>, entity: Option<f64> },
    // Eigener Spielmodus: aus Login/Respawn und aus dem Game Event GAME_MODE_CHANGED.
    GameMode(u8),
    // Hardcore-Welt (aus dem Login): die Herzen bekommen ihre Hardcore-Sprites.
    Hardcore(bool),
    // Eigene Faehigkeiten (Flug, Kreativ), s. packets::read_player_abilities.
    Abilities(packets::PlayerAbilities),
    // Aus wessen Augen der Client sieht (Zuschauer), s. packets::read_set_camera.
    SetCamera(i32),
    // Die Weltgrenze aendert sich (sechs Pakete, s. packets::BorderUpdate).
    Border(packets::BorderUpdate),
    // Neue Regenstaerke 0..1 (Game Events RAIN_STARTED/RAIN_STOPPED/RAIN_GRADIENT, s.
    // packets::GAME_EVENT_RAIN_STARTED).
    RainGradient(f32),
    // Neue Gewitterstaerke 0..1 (Game Event 8, s. packets::GAME_EVENT_THUNDER_GRADIENT).
    ThunderGradient(f32),
    // Game Event INITIAL_CHUNKS_COMING (s. packets::GAME_EVENT_INITIAL_CHUNKS_COMING).
    InitialChunksComing,
    // Game Event PROJECTILE_HIT_PLAYER: ein eigener Pfeil hat einen Spieler getroffen.
    ProjectileHitPlayer,
    // Set Entity Data, Feld AIR: Restluft in Ticks (300 = voll).
    EntityAir { id: i32, air: i32 },
    // Set Entity Motion (0x63): der Server SETZT die Geschwindigkeit eines Entities. Fuer uns
    // selbst ist das der Rueckstoss.
    EntityVelocity(i32, (f64, f64, f64)),
    // Explode (0x24): der auf UNS entfallende Rueckstoss der Explosion, falls die Explosion
    // uns ueberhaupt erfasst hat. Wird ADDIERT, nicht gesetzt.
    ExplosionKnockback((f64, f64, f64)),
    // Dasselbe Explode-Paket, diesmal alles HINTER dem Rueckstoss: Mittelpartikel, Ton und die
    // Blockpartikel-Liste. Getrennt vom Rueckstoss, weil der auch dann ankommen muss, wenn
    // der Rest (unbekannter Partikeltyp einer neueren Serverversion) nicht lesbar ist.
    ExplosionEffects(protocol::particle::Explosion),
    // Level Particles: ein vom Server ausgeloester Partikelausstoss (Anzahl, Streuung, Tempo).
    Particles(protocol::particle::LevelParticles),
    // Level Event (WorldEventS2CPacket): Ereignis-ID, Block-Position, Datenwert. Gebraucht
    // wird bislang nur 2001 (Block zerbrochen, Datenwert = Block-State).
    LevelEvent(protocol::particle::LevelEvent),
    // Ein Statuseffekt kam dazu oder wurde erneuert (Update Mob Effect). Die Entity-Id kommt
    // mit, weil das Paket auch fremde Spieler betrifft -- gebraucht wird bislang nur der
    // eigene Spieler (Sprintsperre durch Blindheit, Tempo durch Schnelligkeit/Langsamkeit).
    //
    // Es wird NICHT hier gefiltert, welche Effekte interessieren: das Filtern gehoert dorthin,
    // wo die Effekte auch gebraucht werden. Frueher liess dieser Zweig nur Blindheit durch --
    // mit einer um eins verschobenen ID, die niemandem auffiel, weil ein nie ausgeloester
    // Effekt sich genauso anfuehlt wie ein nicht gebrauchter.
    MobEffect { entity_id: i32, effect: i32, amplifier: i32, duration: i32, flags: u8 },
    // Ein Statuseffekt wurde entfernt (Remove Mob Effect).
    MobEffectRemoved { entity_id: i32, effect: i32 },
    // Weltzeit (Set Time, s. packets::read_world_time). `ticking` = ob die Tageszeit laeuft
    // (`doDaylightCycle`); steht sie still, darf der Client nicht selbst weiterzaehlen.
    WorldTime { time_of_day: i64, ticking: bool },
    // coral-record: die Aufnahme hat mitten im Spiel mit dieser Sitzung begonnen. Alles davor
    // ist verarbeitet, die App schreibt jetzt ihren Zustand als Snapshot dazu (s. snapshot.rs).
    RecordingStarted(u64),
    // Biom-Registry aus dem Configuration-State (Registry Data 0x07, s. packets.rs) --
    // Index in der Liste = numerische Biom-ID, exakt der Wert aus der Biom-Palette im
    // Chunk-Paket. Fuer Gras-/Laub-/Wasser-Faerbung (Colormaps).
    BiomeRegistry(Vec<packets::BiomeInfo>),
    // Die Dimensionsarten in Registrierungsreihenfolge (Configuration-State). Login und
    // Respawn verweisen danach nur noch mit einem Index hinein (s. DimensionType).
    DimensionRegistry(Vec<packets::DimensionInfo>),
    // In welche Dimensionsart wir gerade gekommen sind -- Index in die Liste oben. Daran
    // haengt die Welt-Untergrenze; ohne sie liegen alle Bloecke in der falschen Hoehe
    // (s. world::chunk::DEFAULT_MIN_Y).
    DimensionType(i32),
    // Ein oder mehrere Bloecke haben sich geaendert (Block Update 0x08: einzeln -- Platzieren/
    // Abbauen durch irgendeinen Spieler, Pflanzenwachstum, ...; Section Blocks Update 0x52:
    // gebuendelt -- Kolben, Redstone-Ketten, WorldEdit-artige Batches). Je Eintrag absolute
    // Weltkoordinaten + neuer Block-State. Ohne dieses Event blieb der Client nach dem
    // initialen Chunk-Laden dauerhaft auf dem Stand von damals.
    BlockUpdate(Vec<(i32, i32, i32, u32)>),
    // Block Changed Ack (0x04): der Server hat alle Block-Aktionen bis EINSCHLIESSLICH dieser
    // Sequenznummer abgearbeitet. Erst damit darf der Client seine Platzier-Vorhersage
    // aufloesen (s. app::PendingBlock).
    BlockChangedAck(i32),
    // Block Event (0x07): (Position, Typ, Daten), s. packets::read_block_event.
    BlockEvent((i32, i32, i32), u8, u8),
    // Block Destruction (0x05): ein Entity baut einen Block ab; Stufe 0..=9, -1 oder >= 10
    // = Riss entfernen (s. packets::read_block_destruction).
    BlockDestruction { breaker: i32, pos: (i32, i32, i32), progress: i8 },
    // Eigene EntityId aus Login (play) (0x30, immer das allererste Play-Packet, s.
    // packets::CB_PLAY_LOGIN-Kommentar) -- serverbound Pakete wie Player Command (Sprint-
    // Start/Stop) brauchen sie als erstes Feld.
    OwnEntityId(i32),
    // Eigene Spieler-UUID aus dem Login-Success-Packet. Das ist die UUID, die der SERVER
    // uns zuweist -- im Offline-Mode aus dem Namen abgeleitet, nicht die Konto-UUID. Genau
    // sie taucht auch in der Spielerliste (Player Info Update) auf, und nur darueber findet
    // der Client den eigenen Skin (Vanilla: AbstractClientPlayerEntity.getSkin() sucht den
    // PlayerListEntry per getUuid()).
    OwnUuid([u8; 16]),
    // EntityStatusS2CPacket: Byte-Ereignis fuer ein Entity (s. packets::read_entity_event).
    // Gebraucht fuer den Totem-Pop (Status 35) -- das einzige Signal, das der Server dazu
    // schickt.
    EntityEvent { entity_id: i32, status: i8 },
    // EntityDamageS2CPacket: dieses Entity hat gerade Schaden genommen (s.
    // packets::read_damage_event). Treibt das rote Aufblitzen -- ein anderes Signal dafuer
    // gibt es nicht, der Gesundheitswert fremder Entities kommt nie ueber die Leitung.
    // `damage_type` ist ein Index in die Schadensart-Registry (s. ClientEvent::DamageTypes) --
    // gebraucht fuer den EIGENEN Schadenston, den der Server bewusst nicht mitschickt.
    // `cause_id`/`direct_id`/`source_pos`: woher der Schaden kam (s. packets::DamageEvent).
    EntityDamaged { entity_id: i32, damage_type: i32, cause_id: Option<i32>, direct_id: Option<i32>, source_pos: Option<[f64; 3]> },
    // EntityAnimationS2CPacket: dieses Entity hat geschlagen (oder ist aufgewacht / hat einen
    // Kritischen gelandet, s. packets::ANIMATION_*). Wie beim Schaden ist das Paket die
    // einzige Quelle: der Armschwung folgt aus keiner anderen Groesse, die wir bekommen.
    //
    // Der eigene Schwung kommt hier NICHT an -- der Server verschickt ihn mit
    // `sendToOtherNearbyPlayers`, also an alle ausser dem Schlagenden selbst. Dieselbe
    // Ausnahme wie beim eigenen Schadenston (s. App::play_own_hurt_sound).
    EntityAnimation { entity_id: i32, animation: u8 },
    // Die Schadensart-Registry aus der Konfiguration, in Registry-Reihenfolge.
    DamageTypes(Vec<packets::DamageEffect>),
    // Die Verzauberungs-Registry (Name + Definition), in Registry-Reihenfolge -- die Position
    // ist die ID, mit der Items ihre Verzauberungen nennen.
    Enchantments(Vec<(String, Option<protocol::nbt::NbtValue>)>),
    // Trim-Materialien/-Muster und Bannermuster (s. packets::read_named_registry).
    NamedRegistry { registry: String, entries: Vec<(String, Option<protocol::nbt::NbtValue>)> },
    // Schildtext aus Block Entity Data (s. protocol::sign). Die Schilder einer Spalte kommen
    // mit dem Chunk (ChunkData::signs).
    SignText { pos: (i32, i32, i32), text: protocol::sign::SignText },
    // Profil eines platzierten Kopfes (Block Entity Data), s. protocol::sign::read_block_entity_head.
    HeadAt { pos: (i32, i32, i32), profile: protocol::item::HeadProfile },
    // Der Server oeffnet den Schild-Editor (nach dem Platzieren oder Rechtsklick).
    OpenSignEditor { pos: (i32, i32, i32), front: bool },
    // Der Server oeffnet das Buch in einer Hand zum Lesen (geschriebenes Buch).
    OpenBook { hand: i32 },
    // Kartenstand (map_item_data).
    MapData(protocol::packets::MapItemData),
    // Antwort auf eine Tab-Vervollstaendigung (s. packets::read_command_suggestions).
    CommandSuggestions(packets::CommandSuggestions),
    // System Chat (0x77): Server-/Plugin-Broadcasts, Kommandoantworten -- s. packets::
    // CB_PLAY_SYSTEM_CHAT-Kommentar. bool = Actionbar-Overlay statt normaler Chatzeile.
    // Die Nachricht MIT ihren Farben (s. packets::read_system_chat_spans) -- im Chat traegt
    // Farbe Bedeutung: Teamfarben, Warnungen, Serverpraefixe.
    Chat(Vec<protocol::nbt::TextSpan>, bool),
    // Die Chat-Zeile davor war eine Vanilla-Todesmeldung: Opfer und (wenn genannt) Taeter,
    // als reiner Text (s. nbt::death_message).
    DeathMessage { victim: String, killer: Option<String> },
    // Ein Play-Paket, wie es ankam (samt Packet-ID), als Kopie fuer Addons -- nur bei
    // gesetztem Events::with_raw. Verarbeitet wird es trotzdem normal.
    RawPacket(Vec<u8>),
    // Wer sitzt auf wem (s. packets::read_set_passengers). Eine leere Liste = alle sind
    // abgestiegen.
    Passengers { vehicle: i32, passengers: Vec<i32> },
    // Item-Abklingzeit einer Cooldown-Gruppe (s. packets::read_cooldown). ticks == 0 gibt die
    // Gruppe frei.
    ItemCooldown { group: String, ticks: i32 },
    // Plugin-Nachricht vom Server (Play- oder Configuration-State): Kanal + rohe Daten.
    CustomPayload { channel: String, data: Vec<u8> },
    // Actionbar-Text ueber der Hotbar (Set Action Bar Text 0x55) -- funktional dasselbe wie
    // Chat(text, overlay=true), nur ein eigenes Paket.
    ActionBar(Vec<protocol::nbt::TextSpan>),
    // Titel/Untertitel in der Bildschirmmitte (Set Title/Subtitle Text) + Ein-/Ausblendzeiten
    // (Set Titles Animation, Ticks) bzw. Ausblenden (Clear Titles).
    Title(Vec<protocol::nbt::TextSpan>),
    Subtitle(Vec<protocol::nbt::TextSpan>),
    TitleFade { fade_in: i32, stay: i32, fade_out: i32 },
    ClearTitles,
    // Scoreboard: welcher Objective in der Seitenleiste (Slot 1) angezeigt wird; Objective
    // anlegen/entfernen/aktualisieren (Titel der Leiste); einzelne Zeilen-Scores setzen/loeschen.
    ScoreboardDisplay { slot: i32, objective: String },
    ScoreboardObjective { name: String, mode: u8, display: Option<String>, hearts: bool, number_format: Option<packets::NumberFormat> },
    ScoreboardScore { holder: String, objective: String, score: i32, display: Option<String>, number_format: Option<packets::NumberFormat> },
    // Spieler verlassen die Spielerliste (PlayerRemoveS2CPacket).
    PlayerInfoRemove(Vec<[u8; 16]>),
    // Kopf- und Fusszeile der Tab-Liste.
    TabList { header: Vec<protocol::nbt::TextSpan>, footer: Vec<protocol::nbt::TextSpan> },
    ScoreboardResetScore { holder: String, objective: Option<String> },
    // Team-Update (Prefix/Suffix + Mitglieder); Mode-Bedeutung s. packets::TeamUpdate.
    Team {
        name: String,
        mode: u8,
        prefix: Option<String>,
        suffix: Option<String>,
        // s. packets::TeamUpdate -- Sichtbarkeitsregel und Farbe des Namensschilds.
        name_tag_visibility: Option<u8>,
        color: Option<i32>,
        collision_rule: Option<u8>,
        entries: Vec<String>,
        display_name: Option<String>,
    },
    // --- Inventar/Container (s. packets.rs, Abschnitt "Inventar / Container") ---
    // Kompletter Inhalt eines Containers. sync_id 0 ist immer das Spielerinventar, jede
    // andere Zahl gehoert zu einem per OpenScreen geoeffneten Container.
    ContainerContent(packets::ContainerContent),
    // Einzelner Slot eines Containers.
    ContainerSlot(packets::ContainerSlotUpdate),
    // Stack an der Maus (getrennt vom Container, seit 1.21.5 ein eigenes Paket).
    CursorItem(Option<protocol::item::ItemStack>),
    // Slot im Spielerinventar, unabhaengig vom gerade offenen Screen.
    PlayerInventorySlot(i32, Option<protocol::item::ItemStack>),
    // Der Server setzt den aktiven Hotbar-Slot (0..8).
    HeldSlot(i32),
    // Container oeffnen/schliessen.
    OpenScreen(packets::OpenScreen),
    CloseScreen(i32),
    // Ein Zahlenwert des offenen Containers (Container-ID, Index, Wert).
    ContainerData(i32, i16, i16),
    // Angebotsliste eines Dorfbewohners/Haendlers fuer den offenen Handelsbildschirm.
    MerchantOffers(packets::MerchantOffers),
    // Rezeptbuch: neu freigeschaltete Rezepte (bzw. alle, s. RecipeBookAdd::replace) und
    // wieder entzogene (IDs).
    RecipeBookAdd(protocol::recipe::RecipeBookAdd),
    RecipeBookRemove(Vec<i32>),
    // Item-Tags des Servers (Name ohne "#" -> Item-IDs), fuer Zutaten wie "alle Bretter".
    ItemTags(std::collections::HashMap<String, Vec<u32>>),
    // Block-Tags des Servers (Name -> Block-Roh-IDs), s. world::mining::set_server_block_tags.
    BlockTags(std::collections::HashMap<String, Vec<u32>>),
    // coral-clip (s. record::clip): ein Puffer hat begonnen bzw. hier liegt ein Haltepunkt --
    // alles davor ist verarbeitet, die App sammelt jetzt ihren Zustand ein. Am Ende der Liste,
    // damit alte Snapshots lesbar bleiben (postcard zaehlt die Varianten durch).
    ClipStarted { session: u64, from_join: bool, t_ms: f64 },
    ClipCheckpoint { session: u64, seq: u32, t_ms: f64 },
    // Wiedergabe: bis zu dieser Aufnahmezeit (ms) sind die Pakete abgearbeitet -- fuer die
    // Leiste (weiss gespielt, blau wird gerade nachgeholt). Hoechstens alle REPLAY_AT_STEP_MS.
    ReplayAt(f64),
    // Dialoge des Tags minecraft:pause_screen_additions (Registry-IDs), s. Pausemenue.
    PauseScreenDialogs(Vec<u32>),
}

// So fein meldet die Wiedergabe ihren Stand (s. ClientEvent::ReplayAt).
pub(crate) const REPLAY_AT_STEP_MS: f64 = 50.0;

// Was der Netzwerk-Task nach aussen meldet: eine Warteschlange zur App PLUS ein optionaler
// SOFORT-Haken.
//
// Die Warteschlange wird von der Fensterschleife geleert (App::drain_events in
// about_to_wait) und haengt damit an der Bildrate -- bei gesetztem FPS-Limit schlaeft die
// Schleife bis zum naechsten Frame-Termin. Fuer Anzeigen ist das richtig (frueher gezeichnet
// wird ohnehin nicht), fuer TON nicht: er ist ein Reaktionssignal und darf nicht auf ein
// Frame warten.
//
// Der Haken laeuft deshalb SYNCHRON hier im Netzwerk-Task, in dem Augenblick, in dem das
// Paket gelesen ist. Er muss entsprechend kurz sein: er blockiert das Lesen des naechsten
// Pakets. Gedacht ist er fuer genau eine Sorte Arbeit -- einen Ton in die Mixer-Queue legen
// (s. audio::CueSink), also ein Feldschreibvorgang unter einem Mutex.
#[derive(Clone)]
pub struct Events {
    tx: std::sync::mpsc::Sender<ClientEvent>,
    immediate: Option<std::sync::Arc<dyn Fn(&ClientEvent) + Send + Sync>>,
    // Gesetzt, solange ein Addon rohe Play-Pakete sehen will (s. ClientEvent::RawPacket).
    raw: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    // Gesetzt, solange die Verbindung in einem Hintergrund-Tab liegt: dann gehen nur KeepAlive
    // und Pong raus, die uebrigen Pflichtantworten warten in Connection::held.
    background: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    // Hier traegt die Verbindung ihre Sitzungsnummer bei coral-record ein (0 = keine
    // Aufnahme), damit die App Kamera und Maus derselben Datei zuordnet.
    record: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    // Der Puffer von coral-clip fuer diese Verbindung (s. record::ClipLink).
    clip: Option<std::sync::Arc<record::ClipLink>>,
}

impl Events {
    #[must_use] 
    pub fn new(tx: std::sync::mpsc::Sender<ClientEvent>) -> Self {
        Self { tx, immediate: None, raw: None, background: None, record: None, clip: None }
    }

    // Slot fuer die Sitzungsnummer der Aufnahme (s. record).
    pub fn with_record(mut self, slot: std::sync::Arc<std::sync::atomic::AtomicU64>) -> Self {
        self.record = Some(slot);
        self
    }

    pub fn with_clip(mut self, link: std::sync::Arc<record::ClipLink>) -> Self {
        self.clip = Some(link);
        self
    }

    // Schalter fuer ClientEvent::RawPacket; die App setzt ihn, wenn ein Addon packet_in hoert.
    pub fn with_raw(mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.raw = Some(flag);
        self
    }

    // Schalter fuer Hintergrund-Tabs (s. Connection::reply).
    pub fn with_background(mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.background = Some(flag);
        self
    }

    fn in_background(&self) -> bool {
        self.background.as_ref().is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
    }

    fn wants_raw(&self) -> bool {
        self.raw.as_ref().is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Haken, der JEDES Event sofort im Netzwerk-Task sieht -- vor dem Einreihen in die
    /// Warteschlange. Nur fuer latenzkritische Reaktionen (Ton), nichts Langlaufendes.
    pub fn with_immediate(mut self, hook: std::sync::Arc<dyn Fn(&ClientEvent) + Send + Sync>) -> Self {
        self.immediate = Some(hook);
        self
    }

    // Absichtlich derselbe Name/dieselbe Form wie beim Sender, damit die ~40 Aufrufstellen
    // unveraendert bleiben. Der Haken kommt VOR dem Einreihen: die Warteschlange kann warten,
    // der Ton nicht.
    // Fehler wie bei mpsc: das nicht zugestellte Ereignis kommt zurueck, gross oder nicht.
    #[allow(clippy::result_large_err)]
    pub fn send(&self, event: ClientEvent) -> Result<(), std::sync::mpsc::SendError<ClientEvent>> {
        if let Some(hook) = &self.immediate {
            hook(&event);
        }
        self.tx.send(event)
    }
}
// Rohe, bereits fertig kodierte Serverbound-Payloads (inkl. Packet-ID), die die App
// waehrend des Play-States rausschicken will -- aktuell nur Move-Player-Pakete (s.
// physics.rs in app). Roh statt eines typisierten Enums: der Netzwerk-Task muss die
// Payloads nur durchreichen, nicht verstehen, und app haengt so nicht an protocol-Typen,
// die sich mit jeder neuen Paket-Art aendern wuerden.
pub type Outgoing = tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>;

// Gegenstueck zum gleichnamigen Mitschnitt in der App (CORAL_INTERACT_DEBUG=1) -- damit
// stehen die vom NETZ-Task erzeugten Pakete (Pong, Teleport-Bestaetigung) in derselben Spur
// wie die der Tick-Schleife. Genau deren Verschraenkung ist die Frage.
fn interaktions_log(was: &str) {
    if std::env::var_os("CORAL_INTERACT_DEBUG").is_some() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        eprintln!("[interact] {}.{:03} {was}", t.as_secs() % 1000, t.subsec_millis());
    }
}

// Aktuelle Blickrichtung (Yaw/Pitch in Grad, so wie sie gesendet wird), die die App laufend
// aktualisiert und der Netz-Task lesen kann, ohne die App-Struktur zu kennen. Gebraucht wird
// sie EINZIG fuer die sofortige Teleport-Bestaetigungs-Bewegung (s. handle_play_packet,
// CB_PLAY_PLAYER_POSITION): Grim akzeptiert einen Server-Teleport NUR ueber ein Paket mit
// Position UND Rotation (CheckManagerListener: hasPositionChanged && hasRotationChanged ->
// checkTeleportQueue), also muss der Netz-Task die aktuelle Rotation mitschicken.
//
// Dazu, ob der Spieler gerade auf etwas sitzt: dann laesst Vanilla Position und Drehung aus
// dem Teleport liegen (ClientPacketListener.handleMovePlayer: nur ohne isPassenger) und
// bestaetigt mit der EIGENEN Blickrichtung.
pub type SharedRotation = std::sync::Arc<SharedLook>;

#[derive(Default)]
pub struct SharedLook {
    // Yaw in den oberen 32 Bit, Pitch in den unteren.
    rotation: std::sync::atomic::AtomicU64,
    riding: std::sync::atomic::AtomicBool,
}

impl SharedLook {
    pub fn set(&self, yaw_deg: f32, pitch_deg: f32) {
        let v = (u64::from(yaw_deg.to_bits()) << 32) | u64::from(pitch_deg.to_bits());
        self.rotation.store(v, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get(&self) -> (f32, f32) {
        let v = self.rotation.load(std::sync::atomic::Ordering::Relaxed);
        (f32::from_bits((v >> 32) as u32), f32::from_bits(v as u32))
    }

    pub fn set_riding(&self, riding: bool) {
        self.riding.store(riding, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn riding(&self) -> bool {
        self.riding.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// Disconnect-Meldung mit dem Grund, den der Server mitschickt. Laesst er ihn weg oder ist
// er nicht lesbar, bleibt es beim bisherigen Text -- ein unlesbarer Grund darf die
// eigentliche Information ("wir sind raus") nicht verschlucken.
fn disconnect_message(state: &str, payload: &[u8]) -> String {
    match packets::read_disconnect_reason(payload) {
        Ok(reason) if !reason.trim().is_empty() => format!("Server-Disconnect im {state}-State: {reason}"),
        _ => format!("Server-Disconnect im {state}-State (ohne Begruendung)"),
    }
}

fn log(events: &Events, msg: impl Into<String>) {
    let _ = events.send(ClientEvent::Log(msg.into()));
}

pub const DEFAULT_PORT: u16 = 25565;

// Zerlegt "host:port" bzw. nur "host" (dann Standard-Port 25565).
fn parse_address(address: &str) -> (String, u16) {
    match address.rsplit_once(':') {
        Some((host, port_str)) => match port_str.parse::<u16>() {
            Ok(port) => (host.to_string(), port),
            Err(_) => (address.to_string(), DEFAULT_PORT),
        },
        None => (address.to_string(), DEFAULT_PORT),
    }
}

// SRV-Weiterleitung wie Vanilla (1.21.11, RedirectResolver.createSrv() im 
// Client): steht der Port auf dem Standard 25565, wird "_minecraft._tcp.<host>" nach einem
// SRV-Eintrag gefragt und dessen Ziel-Host + Ziel-Port statt der eingegebenen Adresse
// verwendet. Genau das ermoeglicht "derantonia.de" fuer einen Server, der in Wahrheit auf
// einem anderen Port (oder einer anderen Maschine) laeuft.
//
// Bewusst identisch zu Vanilla in drei Punkten, die leicht anders wirken koennten:
//  * Die Abfrage entfaellt bei explizit angegebenem Port -- aber NUR daran erkennbar, dass
//    er nicht 25565 ist. "host:25565" von Hand getippt fragt also ebenfalls SRV ab, wie in
//    Vanilla (der Client kann "Standard" und "explizit 25565" nicht unterscheiden).
//  * Ein Fehler (kein Eintrag, kein DNS erreichbar, Timeout) ist KEIN Verbindungsfehler --
//    es bleibt einfach bei der eingegebenen Adresse.
//  * Der spaetere Handshake traegt die AUFGELOESTE Adresse, nicht die eingegebene (Vanilla
//    reicht inetSocketAddress.getHostName()/getPort() an ClientConnection.connect weiter,
//    s. ConnectScreen). Fuer Proxys/virtuelle Hosts ist das der relevante Name.
async fn resolve_srv(host: &str, port: u16) -> Option<(String, u16)> {
    if port != DEFAULT_PORT {
        return None;
    }
    // Resolver aus der Systemkonfiguration (/etc/resolv.conf). Schlaegt das fehl, gibt es
    // hier nichts zu tun -- die normale Namensaufloesung beim TcpStream::connect laeuft
    // ueber das Betriebssystem und ist davon unberuehrt.
    let resolver = hickory_resolver::Resolver::builder_tokio().ok()?.build().ok()?;
    let query = format!("_minecraft._tcp.{host}.");
    let lookup = resolver.srv_lookup(query).await.ok()?;
    // Bei mehreren Eintraegen den mit der KLEINSTEN Prioritaet nehmen (so ist SRV definiert).
    // Vanilla nimmt schlicht den ersten, den JNDI liefert -- bei genau einem Eintrag (der
    // Normalfall) ist das dasselbe, bei mehreren ist die Prioritaet die richtige Wahl.
    let record = lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            hickory_resolver::proto::rr::RData::SRV(srv) => Some(srv),
            _ => None,
        })
        .min_by_key(|srv| srv.priority)?;
    let target = record.target.to_string();
    // Der Zielname kommt als absoluter DNS-Name ("host.example.com.") -- der Punkt am Ende
    // ist fuer die Namensaufloesung egal, wuerde aber im Handshake-Feld mitgeschickt.
    let target = target.trim_end_matches('.').to_string();
    if target.is_empty() {
        return None;
    }
    Some((target, record.port))
}

// Adresse zerlegen und, wo Vanilla es auch tut, per SRV weiterleiten.
// Ohne Konto (Offline-Mode) darf nur ein Server auf diesem Rechner angesprochen werden --
// zum Testen mit einem eigenen online-mode=false-Server. Alles andere braucht eine echte
// Anmeldung, sonst waere der Client ein Weg, ohne gekauftes Spiel zu spielen.
#[must_use] 
pub fn is_local_address(address: &str) -> bool {
    let (host, _) = parse_address(address.trim());
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

async fn resolve_address(address: &str) -> (String, u16) {
    let (host, port) = parse_address(address);
    match resolve_srv(&host, port).await {
        Some(redirected) => redirected,
        None => (host, port),
    }
}

// Kapselt Stream + Kompressions-Zustand. Kompression ist symmetrisch: sobald der Server
// Set Compression schickt, nutzen sowohl Lesen als auch Schreiben das komprimierte Format.
struct Connection {
    // Der Socket, oder auf erlaubten Servern die Verzoegerungsschicht davor (s. crate::lag).
    stream: Box<dyn crate::lag::Io>,
    // Gegenstelle des Sockets (fuer ClientEvent::Route), None wenn unbekannt.
    peer: Option<std::net::SocketAddr>,
    // None = keine Kompression, Some(threshold) = aktiv ab dieser Payload-Groesse.
    compression: Option<i32>,
    // Sobald der Online-Mode-Handshake abgeschlossen ist (s. do_login, EncryptionRequest),
    // laeuft der GESAMTE Bytestrom -- Laengen-Praefix eingeschlossen -- durch AES-128-CFB8.
    // Je Richtung ein eigener Registerzustand. None = unverschluesselt (Offline-Mode).
    encryptor: Option<crate::crypto::Cfb8>,
    decryptor: Option<crate::crypto::Cfb8>,
    // BEREITS ENTSCHLUESSELTE, noch nicht zu Frames zerlegte Empfangsbytes. Persistent in der
    // Connection (nicht lokal im recv-Aufruf), DAMIT recv() cancel-safe ist: recv() wird in
    // run_play_loop per tokio::select! gegen das Senden ausgespielt und kann jederzeit
    // abgebrochen werden. Ein einzelner read() (der einzige await in recv) ist cancel-safe --
    // schon empfangene Bytes landen hier und gehen bei einem Abbruch NICHT verloren. Ohne das
    // riss ein fragmentiert ankommendes Paket (bei echten Servern normal, auf localhost nie)
    // beim gleichzeitigen Sendewunsch den Bytestrom auseinander -> "corrupt deflate stream".
    rbuf: Vec<u8>,
    // Paket-Mitschnitt (s. crate::pktlog): laeuft immer als Ringpuffer mit und wird bei einem
    // Abbruch mit Fehler nach /tmp geschrieben. Sitzt hier, weil send()/recv() die einzigen
    // beiden Stellen sind, an denen der unkomprimierte Payload garantiert vollstaendig
    // vorliegt -- vor dem Komprimieren bzw. nach dem Entpacken.
    log: crate::pktlog::PacketLog,
    // Die Cookies, die dieser Server uns abgelegt hat (Store Cookie), Schluessel -> Inhalt.
    // Lebt genau so lange wie die Verbindung -- wie in Vanilla, wo sie im Verbindungszustand
    // stehen und NICHT auf die Platte wandern. Ein Server fragt sie im selben Zug wieder ab
    // (auch nach einem Transfer), genau darauf bauen Proxy- und Anticheat-"Verbindungstests".
    cookies: std::collections::HashMap<String, Vec<u8>>,
    // Pflichtantworten (Teleport-Bestaetigung, Chunk-Quittung ...), die ein
    // Hintergrund-Tab zurueckhaelt. Sie gehen in Reihenfolge raus, sobald der Tab wieder vorn
    // ist, vor dem ersten Paket der App.
    held: Vec<Vec<u8>>,
    // coral-record: ab der Configuration gesetzt, nimmt auf, solange das Modul an ist.
    rec: Option<record::PacketRecorder>,
    // Wiedergabe einer Aufnahme (s. replay): die Pakete kommen aus der Datei statt vom
    // Socket, und was gesendet werden soll, faellt weg.
    replay: Option<record::PacketSource>,
    // Wann der letzte Schwung Bytes vom Socket kam. Ein Frame wird erst gelesen, wenn keiner
    // mehr vollstaendig im Puffer liegt -- der eben entnommene wurde also genau mit diesem
    // Schwung vollstaendig. Daraus die Antwortzeit des Clients auf KeepAlive (F3).
    last_read: std::time::Instant,
}

impl Connection {
    async fn connect(host: &str, port: u16) -> Result<Self, NetError> {
        let stream = TcpStream::connect((host, port)).await?;
        // TCP_NODELAY (Nagle aus) -- Vanilla setzt das ebenfalls (ClientConnection.class im
        // Yarn-1.21.11-Jar, Zeile 449: channel.config().setOption(ChannelOption.TCP_NODELAY,
        // true)), tokio/std dagegen NICHT (Nagle ist per Default AN).
        // Ohne das haelt der Kernel ein kleines Segment zurueck, solange noch ein
        // unbestaetigtes unterwegs ist -- und bei 20 Bewegungspaketen pro Sekunde ist immer
        // eins unterwegs. Es geht dann erst raus, wenn das Delayed-ACK der Gegenseite
        // eintrifft (typisch bis 40 ms, in Schueben mehr). Genau das schlaegt voll auf den
        // vom Server gemessenen KeepAlive-Roundtrip durch: der Ping steht deutlich hoeher
        // als bei einem Vanilla-Client auf demselben Server, ohne dass die Leitung schlechter
        // waere. Fehlschlag ist nicht fatal -- dann laeuft es eben mit Nagle weiter.
        if let Err(e) = stream.set_nodelay(true) {
            eprintln!("[net] TCP_NODELAY konnte nicht gesetzt werden ({e}) -- Ping kann dadurch hoeher ausfallen");
        }
        let peer = stream.peer_addr().ok();
        let stream: Box<dyn crate::lag::Io> = if crate::lag::allowed(host) { Box::new(crate::lag::wrap(stream)) } else { Box::new(stream) };
        Ok(Self {
            stream,
            peer,
            compression: None,
            encryptor: None,
            decryptor: None,
            rbuf: Vec::new(),
            log: crate::pktlog::PacketLog::new(),
            cookies: std::collections::HashMap::new(),
            held: Vec::new(),
            rec: None,
            replay: None,
            last_read: std::time::Instant::now(),
        })
    }

    // Verbindung fuer die Wiedergabe: kein Socket (der Platzhalter wird nie benutzt).
    fn replay(source: record::PacketSource) -> Self {
        Self {
            stream: Box::new(tokio::io::duplex(1).0),
            peer: None,
            compression: None,
            encryptor: None,
            decryptor: None,
            rbuf: Vec::new(),
            log: crate::pktlog::PacketLog::new(),
            cookies: std::collections::HashMap::new(),
            held: Vec::new(),
            rec: None,
            replay: Some(source),
            last_read: std::time::Instant::now(),
        }
    }

    const fn set_record_play(&mut self, play: bool) {
        if let Some(rec) = &mut self.rec {
            rec.set_play(play);
        }
    }

    // Automatische Antwort des Netz-Tasks. Im Hintergrund-Tab zurueckgehalten (s. held).
    async fn reply(&mut self, events: &Events, payload: &[u8]) -> Result<(), NetError> {
        if events.in_background() {
            self.held.push(payload.to_vec());
            return Ok(());
        }
        self.flush_held().await?;
        self.send(payload).await
    }

    async fn flush_held(&mut self) -> Result<(), NetError> {
        for payload in std::mem::take(&mut self.held) {
            self.send(&payload).await?;
        }
        Ok(())
    }

    // Schaltet ab sofort auf Verschluesselung: Schluessel = IV = Shared Secret. Muss GENAU nach
    // dem Senden der (noch unverschluesselten) Encryption-Response passieren.
    fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.encryptor = Some(crate::crypto::Cfb8::new(shared_secret));
        self.decryptor = Some(crate::crypto::Cfb8::new(shared_secret));
    }

    // Das naechste faellige Paket der Aufnahme. Cancel-safe wie recv: entnommen wird erst,
    // wenn es zurueckgegeben wird. Am Ende bleibt die Verbindung stehen (die Welt bleibt zu
    // sehen), bis die App sie abbricht.
    async fn recv_replay(&mut self) -> Result<Vec<u8>, NetError> {
        loop {
            let Some(src) = self.replay.as_mut() else { return Err(NetError::Protocol("keine Wiedergabe".into())) };
            match src.next_due().map_err(|e| NetError::Protocol(e.to_string()))? {
                record::Next::Packet(p) if p.is_empty() => {}
                record::Next::Packet(p) => return Ok(p),
                record::Next::Wait(d) => tokio::time::sleep(d).await,
                record::Next::End => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    }

    // Ebenso alle Schreibvorgaenge: erst verschluesseln (Kopie, damit der Aufrufer-Puffer
    // unveraendert bleibt), dann in einem Rutsch schreiben -- so bleibt der Registerzustand
    // ueber Laengen-Praefix und Nutzdaten hinweg zusammenhaengend.
    async fn write_all_enc(&mut self, bytes: &[u8]) -> Result<(), NetError> {
        if let Some(e) = &mut self.encryptor {
            let mut tmp = bytes.to_vec();
            e.encrypt(&mut tmp);
            self.stream.write_all(&tmp).await?;
        } else {
            self.stream.write_all(bytes).await?;
        }
        Ok(())
    }

    async fn write_framed(&mut self, bytes: &[u8]) -> Result<(), NetError> {
        let mut frame = encode_varint(bytes.len() as i32);
        frame.extend_from_slice(bytes);
        self.write_all_enc(&frame).await
    }

    async fn send(&mut self, payload: &[u8]) -> Result<(), NetError> {
        if self.replay.is_some() {
            return Ok(());
        }
        self.log.record(crate::pktlog::Dir::C2S, payload);
        if let Some(rec) = &mut self.rec {
            rec.c2s(payload);
        }
        match self.compression {
            None => self.write_framed(payload).await,
            Some(threshold) => {
                let mut inner = Vec::new();
                if (payload.len() as i32) >= threshold {
                    inner.extend_from_slice(&encode_varint(payload.len() as i32));
                    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                    io::Write::write_all(&mut encoder, payload).map_err(NetError::Io)?;
                    inner.extend_from_slice(&encoder.finish().map_err(NetError::Io)?);
                } else {
                    inner.extend_from_slice(&encode_varint(0)); // 0 = unkomprimiert (unter Threshold)
                    inner.extend_from_slice(payload);
                }
                self.write_framed(&inner).await
            }
        }
    }

    // Gibt den naechsten dekomprimierten Payload (Packet-ID + Felder) zurueck, oder
    // NetError::Timeout, wenn READ_TIMEOUT lang KEIN einziges Byte kam.
    //
    // CANCEL-SAFE: der einzige await ist der einzelne read() in fill_rbuf(); wird recv() per
    // tokio::select! abgebrochen, dann entweder VOR diesem read (nichts passiert) oder es ist
    // schon zurueck und die Bytes stehen in self.rbuf. Kein read_exact ueber mehrere Polls, das
    // bei Abbruch Bytes im Socket verschluckt haette. Bereits gepufferte Frames werden ohne
    // jeden await sofort zurueckgegeben.
    async fn recv(&mut self) -> Result<Vec<u8>, NetError> {
        if self.replay.is_some() {
            return self.recv_replay().await;
        }
        loop {
            if let Some(frame) = self.take_buffered_frame()? {
                // Wie Vanilla (Varint21FrameDecoder): ein Frame der Laenge 0 ist kaputt.
                if frame.is_empty() {
                    return Err(NetError::Protocol("Frame-Laenge 0".into()));
                }
                let payload = decompress_frame(self.compression, frame)?;
                self.log.record(crate::pktlog::Dir::S2C, &payload);
                if let Some(rec) = &mut self.rec {
                    rec.s2c(&payload);
                }
                // Leer entpackt (Frame nur aus der Null fuer "unkomprimiert"): Vanilla reicht so
                // etwas gar nicht an den Paket-Decoder weiter -- Netty ruft decode nur mit
                // lesbaren Bytes auf --, das Paket verschwindet also still. Manche Server
                // schicken das; trennen waere hier strenger als Vanilla.
                if payload.is_empty() {
                    continue;
                }
                return Ok(payload);
            }
            // Kein vollstaendiger Frame im Puffer -> mehr Bytes holen (der einzige await).
            match tokio::time::timeout(READ_TIMEOUT, self.fill_rbuf()).await {
                Ok(result) => result?,
                Err(_elapsed) => return Err(NetError::Timeout),
            }
        }
    }

    // Liest EINEN Schwung Bytes vom Socket (cancel-safe: ein einzelner read()), entschluesselt
    // sie sofort synchron (kein await dazwischen) und haengt sie an den persistenten rbuf.
    async fn fill_rbuf(&mut self) -> Result<(), NetError> {
        let mut tmp = [0u8; 8192];
        let n = self.stream.read(&mut tmp).await?;
        self.last_read = std::time::Instant::now();
        if n == 0 {
            return Err(NetError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "Verbindung vom Server geschlossen")));
        }
        if let Some(d) = &mut self.decryptor {
            d.decrypt(&mut tmp[..n]);
        }
        self.rbuf.extend_from_slice(&tmp[..n]);
        Ok(())
    }

    // Zerlegt genau EINEN kompletten Frame (Laengen-VarInt + so viele Bytes) aus rbuf und
    // entfernt ihn dort. None = noch nicht genug Bytes fuer einen vollstaendigen Frame.
    fn take_buffered_frame(&mut self) -> Result<Option<Vec<u8>>, NetError> {
        let Some((packet_len, header)) = peek_frame_len(&self.rbuf)? else {
            return Ok(None); // Laengen-VarInt noch unvollstaendig
        };
        let total = header + packet_len;
        if self.rbuf.len() < total {
            return Ok(None); // Frame-Rumpf noch unvollstaendig
        }
        let frame = self.rbuf[header..total].to_vec();
        self.rbuf.drain(0..total);
        Ok(Some(frame))
    }
}

// Spielt eine echte Aufnahme durch den Wiedergabe-Pfad, die Uhr gleich am Ende (alles ohne
// Warten): CORAL_REPLAY_TEST=<datei.crec> cargo test -p net --lib -- --ignored wiedergabe
#[cfg(test)]
mod replay_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod meta_filter_tests;

// Diagnose fuer unlesbare Items: Ort und Rohbytes (hex) in eine Datei neben den
// Einstellungen, damit das Format nachgesehen werden kann, statt es zu raten.
// ~/.config/coral-client/unlesbare_slots.log, angehaengt, hoechstens 4 KB Bytes je Eintrag.
fn dump_unreadable_slot(sync_id: i32, index: usize, wo: &str, bytes: &[u8]) {
    use std::io::Write;
    let Some(home) = std::env::var_os("HOME") else { return };
    let pfad = std::path::Path::new(&home).join(".config/coral-client/unlesbare_slots.log");
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&pfad) else { return };
    let hex: String = bytes.iter().take(4096).map(|b| format!("{b:02x}")).collect();
    let _ = writeln!(f, "Fenster {sync_id}, Slot {index}: {wo}\n{hex}\n");
}

// Ein einzelnes Slot-Update war unlesbar: bisher still verworfen -- das Item fehlte einfach.
fn report_unreadable_single_slot(events: &Events, art: &str, payload: &[u8]) {
    let wo = protocol::item::last_slot_failure().unwrap_or_else(|| "ohne Component-Angabe".to_string());
    log(events, format!("Inventar: {art} unlesbar -- {wo}"));
    dump_unreadable_slot(-1, 0, &format!("{art}: {wo}"), payload);
}
