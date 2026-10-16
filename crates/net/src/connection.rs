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
    Timeout,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Io(e) => write!(f, "IO-Fehler: {e}"),
            NetError::Protocol(msg) => write!(f, "Protokoll-Fehler: {msg}"),
            NetError::Timeout => {
                write!(f, "Timed out ({}s keine Daten vom Server)", READ_TIMEOUT.as_secs())
            }
        }
    }
}

impl std::error::Error for NetError {}

impl From<io::Error> for NetError {
    fn from(e: io::Error) -> Self {
        NetError::Io(e)
    }
}

pub struct LoginResult {
    pub uuid: Uuid,
    pub username: String,
}

// Meldungen vom (async, im Hintergrund laufenden) Netzwerk-Task an die App, damit diese
// Verbindungsstatus + Log im Client-Fenster anzeigen kann (statt nur println! aufs Terminal).
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
    // Server entlaedt eine Chunk-Spalte (Forget Level Chunk) -- der Client muss sie aus
    // Weltzustand und VRAM werfen, sonst wachsen beide beim Umherlaufen unbegrenzt.
    ForgetChunk(i32, i32),
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
    // HungerManager.canSprint -> foodLevel > 6).
    Health(f32, i32),
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
    // Die Reichweiten fuer Bloecke und Entities (im Kreativmodus vom Server angehoben).
    RangeAttributes { id: i32, block: Option<f64>, entity: Option<f64> },
    // Eigener Spielmodus: aus Login/Respawn und aus dem Game Event GAME_MODE_CHANGED.
    GameMode(u8),
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
    EntityDamaged { entity_id: i32, damage_type: i32 },
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
}

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
}

impl Events {
    pub fn new(tx: std::sync::mpsc::Sender<ClientEvent>) -> Self {
        Self { tx, immediate: None }
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
        let v = ((yaw_deg.to_bits() as u64) << 32) | pitch_deg.to_bits() as u64;
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
    stream: TcpStream,
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
        Ok(Self {
            stream,
            compression: None,
            encryptor: None,
            decryptor: None,
            rbuf: Vec::new(),
            log: crate::pktlog::PacketLog::new(),
            cookies: std::collections::HashMap::new(),
        })
    }

    // Schaltet ab sofort auf Verschluesselung: Schluessel = IV = Shared Secret. Muss GENAU nach
    // dem Senden der (noch unverschluesselten) Encryption-Response passieren.
    fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.encryptor = Some(crate::crypto::Cfb8::new(shared_secret));
        self.decryptor = Some(crate::crypto::Cfb8::new(shared_secret));
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
        self.log.record(crate::pktlog::Dir::C2S, payload);
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
        loop {
            if let Some(frame) = self.take_buffered_frame()? {
                // Wie Vanilla (Varint21FrameDecoder): ein Frame der Laenge 0 ist kaputt.
                if frame.is_empty() {
                    return Err(NetError::Protocol("Frame-Laenge 0".into()));
                }
                let payload = decompress_frame(self.compression, frame)?;
                self.log.record(crate::pktlog::Dir::S2C, &payload);
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

// Wendet (falls aktiv) die Zlib-Dekompression auf einen rohen Frame an -- identisch zur
// frueheren recv_inner-Logik, nur vom Framing entkoppelt. Bewusst eine FREIE Funktion statt
// einer Methode: sie braucht von der Verbindung nur das Kompressions-Flag, und ohne `&self`
// laesst sich die Laengenpruefung (s. MAX_UNCOMPRESSED_LEN) ohne echten TCP-Socket testen.
fn decompress_frame(compression: Option<i32>, raw: Vec<u8>) -> Result<Vec<u8>, NetError> {
    if compression.is_none() {
        return Ok(raw);
    }
    let (data_len, offset) = decode_varint(&raw, 0).map_err(|_| NetError::Protocol("ungueltiger VarInt".into()))?;
    let rest = &raw[offset..];
    if data_len == 0 {
        return Ok(rest.to_vec()); // Unter dem Threshold: unkomprimiert
    }
    // Vor der Allokation pruefen, nicht danach -- genau darin liegt der Angriff (s.
    // MAX_UNCOMPRESSED_LEN). Faengt negative Laengen gleich mit ab.
    if !(1..=MAX_UNCOMPRESSED_LEN).contains(&data_len) {
        return Err(NetError::Protocol(format!("unplausible entpackte Paketlaenge: {data_len}")));
    }
    let mut decoder = ZlibDecoder::new(rest);
    let mut out = vec![0u8; data_len as usize];
    io::Read::read_exact(&mut decoder, &mut out).map_err(NetError::Io)?;
    // read_exact fuellt genau data_len Bytes -- entpackt der Strom MEHR, war die
    // Laengenangabe gelogen. Stillschweigend abschneiden ergaebe ein halbes Paket, das der
    // Parser danach als Muell sieht; hier ist die Ursache noch benennbar.
    if io::Read::read(&mut decoder, &mut [0u8; 1]).map_err(NetError::Io)? != 0 {
        return Err(NetError::Protocol("entpacktes Paket laenger als angegeben".into()));
    }
    Ok(out)
}

// Liest die Frame-Laenge (VarInt) am Anfang von `buf`, ohne etwas zu verbrauchen. Ok(None) =
// der VarInt ist selbst noch nicht vollstaendig da (mehr Bytes noetig). Gibt (Laenge,
// VarInt-Bytelaenge) zurueck.
fn peek_frame_len(buf: &[u8]) -> Result<Option<(usize, usize)>, NetError> {
    let mut value: u32 = 0;
    for i in 0..5 {
        let Some(&byte) = buf.get(i) else {
            return Ok(None);
        };
        value |= ((byte & 0x7F) as u32) << (7 * i);
        if byte & 0x80 == 0 {
            let len = value as usize;
            if len > MAX_FRAME_LEN {
                return Err(NetError::Protocol(format!("Frame-Laenge {len} ueber dem Limit {MAX_FRAME_LEN}")));
            }
            return Ok(Some((len, i + 1)));
        }
    }
    Err(NetError::Protocol("Laengen-VarInt zu lang".into()))
}

async fn send_handshake(conn: &mut Connection, host: &str, port: u16, intent: i32) -> Result<(), NetError> {
    let handshake = Handshake { protocol_version: packets::PROTOCOL_VERSION, server_address: host.to_string(), server_port: port, intent };
    conn.send(&handshake.to_payload()).await
}

// Server-Status-Ping (Meilenstein 1): Handshake -> Status Request -> Status Response.
pub async fn ping(address: &str) -> Result<String, NetError> {
    let (host, port) = resolve_address(address).await;
    let mut conn = Connection::connect(&host, port).await?;

    send_handshake(&mut conn, &host, port, packets::INTENT_STATUS).await?;
    conn.send(&packets::status_request_payload()).await?;

    let payload = conn.recv().await?;
    let response = StatusResponse::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltige Status-Response".into()))?;
    Ok(response.json)
}

// Fuehrt Handshake + Login Start + Login-Loop aus, bis Login Success erreicht ist. Mit
// `credentials` (echtes Konto) laeuft der Online-Mode-Handshake inkl. Verschluesselung; ohne
// (None) der bisherige Offline-Mode mit frei waehlbarem Namen + Zufalls-UUID.
// --- Eingeloggte Upstream-Verbindung fuer den Diagnose-Proxy ----------------------------
//
// Gegen einen Online-Mode-Server ist ein reiner Byte-Weiterleiter blind: ab der
// Encryption-Response laeuft der ganze Strom durch AES-128-CFB8, der Proxy saehe nur
// Chiffrat. Damit der Proxy trotzdem mitlesen kann, meldet er sich SELBST an (mit denselben
// Zugangsdaten wie der Client) und spricht zum echten Vanilla-Client unverschluesselt --
// das ist genau die Aufteilung, die pktproxy im MITM-Modus braucht.
//
// Nach aussen gibt es hier nur Payloads: entschluesselt und entpackt. Framing, Kompression
// und Verschluesselung bleiben in Connection.
pub struct UpstreamLink {
    conn: Connection,
    pub username: String,
    pub uuid: protocol::uuid::Uuid,
    // Der Schwellwert, den der Server uns gesetzt hat. Der Proxy reicht GENAU denselben an
    // seinen Client weiter, dann sind beide Seiten formatgleich und er muss nichts umpacken.
    pub compression: Option<i32>,
}

impl UpstreamLink {
    // Handshake, Login Start, Verschluesselung und Session-Join -- bis EINSCHLIESSLICH Login
    // Success. Das Login Acknowledged wird bewusst NICHT geschickt: das reicht der Proxy vom
    // echten Client durch, damit die Reihenfolge unangetastet bleibt.
    pub async fn login(address: &str, credentials: &auth::Credentials) -> Result<Self, NetError> {
        let (host, port) = resolve_address(address).await;
        let mut conn = Connection::connect(&host, port).await?;
        let login = do_login(&mut conn, &host, port, packets::INTENT_LOGIN, &credentials.username, Some(credentials)).await?;
        let compression = conn.compression;
        Ok(Self { conn, username: login.username, uuid: login.uuid, compression })
    }

    pub async fn send(&mut self, payload: &[u8]) -> Result<(), NetError> {
        self.conn.send(payload).await
    }

    pub async fn recv(&mut self) -> Result<Vec<u8>, NetError> {
        self.conn.recv().await
    }
}

async fn do_login(
    conn: &mut Connection,
    host: &str,
    port: u16,
    // INTENT_LOGIN beim normalen Verbinden, INTENT_TRANSFER nach einem Transfer-Paket.
    intent: i32,
    offline_username: &str,
    credentials: Option<&auth::Credentials>,
) -> Result<LoginResult, NetError> {
    send_handshake(conn, host, port, intent).await?;

    // Online: echter Name + echte Konto-UUID. Offline: frei waehlbarer Name + Zufalls-UUID
    // (der Server leitet die UUID dort ohnehin deterministisch aus dem Namen ab).
    let (name, uuid) = match credentials {
        Some(c) => {
            let uuid =
                Uuid::from_hex(&c.profile_id).ok_or_else(|| NetError::Protocol("ungueltige Profil-UUID in den Credentials".into()))?;
            (c.username.clone(), uuid)
        }
        None => (offline_username.to_string(), Uuid::random_v4()),
    };
    let login_start = LoginStart { name, uuid };
    conn.send(&login_start.to_payload()).await?;

    loop {
        let payload = conn.recv().await?;
        let id = packets::peek_packet_id(&payload).map_err(|_| NetError::Protocol("leeres Packet".into()))?;
        match id {
            0x00 => {
                let d = LoginDisconnect::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltiges Disconnect-Packet".into()))?;
                // Der Login-State schickt JSON, kein NBT -- ausgepackt steht hier die
                // Meldung des Servers statt einer rohen Zeichenkette voller Stil-Felder
                // (etwa "You are already online!", das sonst als JSON im Log stand).
                // Die FARBEN gehen dabei verloren: der Fehlerweg traegt nur Text.
                let grund: String = protocol::nbt::json_text_to_spans(&d.reason_json).iter().map(|s| s.text.as_str()).collect();
                let grund = grund.trim();
                let grund = if grund.is_empty() { d.reason_json.clone() } else { grund.to_string() };
                return Err(NetError::Protocol(format!("Server hat Login abgelehnt: {grund}")));
            }
            0x01 => {
                // EncryptionRequest -- der Server ist im Online-Mode. Ohne Credentials koennen
                // wir den Session-Join nicht durchfuehren; klarer Hinweis statt kryptischem Fehler.
                let creds = credentials.ok_or_else(|| {
                    NetError::Protocol(
                        "Dieser Server ist im Online-Mode -- bitte zuerst mit Microsoft anmelden (oder Token eingeben)".into(),
                    )
                })?;
                do_encryption_handshake(conn, &payload, creds).await?;
            }
            0x02 => {
                let s = LoginSuccess::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltiges Login-Success-Packet".into()))?;
                return Ok(LoginResult { uuid: s.uuid, username: s.username });
            }
            0x03 => {
                let c =
                    SetCompression::from_payload(&payload).map_err(|_| NetError::Protocol("ungueltiges SetCompression-Packet".into()))?;
                conn.compression = (c.threshold >= 0).then_some(c.threshold);
            }
            packets::CB_LOGIN_COOKIE_REQUEST => {
                answer_cookie_request(conn, &payload, packets::SB_LOGIN_COOKIE_RESPONSE).await?;
            }
            other => return Err(NetError::Protocol(format!("unerwartetes Login-Packet: id=0x{other:02x}"))),
        }
    }
}

// Beantwortet EncryptionRequest: Shared Secret wuerfeln, Mojang-Session-Join (damit der Server
// uns beim Nachfragen akzeptiert), Shared Secret + Verify Token RSA-verschluesselt zurueckschicken
// und danach den Bytestrom auf AES-128-CFB8 umschalten. Reihenfolge ist kritisch: Der
// Session-Join MUSS vor der Encryption-Response passieren, und die Verschluesselung wird erst
// NACH dem (noch unverschluesselten) Senden der Response aktiv.
async fn do_encryption_handshake(conn: &mut Connection, request_payload: &[u8], credentials: &auth::Credentials) -> Result<(), NetError> {
    let req = packets::EncryptionRequest::from_payload(request_payload)
        .map_err(|_| NetError::Protocol("ungueltiges EncryptionRequest-Packet".into()))?;

    let secret = crate::crypto::random_shared_secret();
    let hash = crate::crypto::server_hash(&req.server_id, &secret, &req.public_key);

    // Bei Mojang anmelden, dass wir diesem Server beitreten (nur wenn der Server es verlangt --
    // in der Praxis immer, wenn er ueberhaupt einen EncryptionRequest schickt).
    if req.should_authenticate {
        auth::mojang_join(&credentials.access_token, &credentials.profile_id, &hash)
            .await
            .map_err(|e| NetError::Protocol(e.to_string()))?;
    }

    let enc_secret = crate::crypto::rsa_encrypt(&req.public_key, &secret).map_err(NetError::Protocol)?;
    let enc_verify = crate::crypto::rsa_encrypt(&req.public_key, &req.verify_token).map_err(NetError::Protocol)?;

    conn.send(&packets::encryption_response_payload(&enc_secret, &enc_verify)).await?;
    // Ab hier ist der komplette Strom verschluesselt.
    conn.enable_encryption(&secret);
    Ok(())
}

// Steht in diesem Metadaten-Strom ueberhaupt etwas, das die DARSTELLUNG angeht?
//
// Der Filter ist kein Luxus: Positions- und Gesundheits-Updates schicken ihren Strom pro Tick
// durch die Warteschlange, ohne dass sich daran je etwas aendert. Er ist aber genau deshalb
// auch die Stelle, an der ein neu ausgelesenes Feld leicht haengenbleibt -- als eigene
// Funktion ist sie pruefbar, ohne eine Verbindung aufzubauen.
fn meta_betrifft_anzeige(update: &packets::EntityDataUpdate) -> bool {
    update.flags.is_some()
        // Das Benutzen-Flag (Schild hoch, Bogen gespannt, am Essen) aendert sich fuer sich
        // allein -- fehlt es hier, sieht man fremde Schilde nie hochgerissen.
        || update.living_flags.is_some()
        // Die goldenen Herzen des eigenen Spielers.
        || update.absorption.is_some()
        || update.custom_name.is_some()
        || update.name_visible.is_some()
        || update.display_text.is_some()
        || update.display_item.is_some()
        || update.display_block_state.is_some()
        || update.display_translation.is_some()
        || update.display_scale.is_some()
        || update.display_left_rotation.is_some()
        || update.display_right_rotation.is_some()
        // Bodenplatte des End-Kristalls -- fehlte, deshalb stand sie auch bei gesetzten
        // Kristallen da.
        || update.end_crystal_show_bottom.is_some()
        || update.display_billboard.is_some()
        || update.orb_value.is_some()
        || update.display_view_range.is_some()
        || update.text_background.is_some()
        || update.text_opacity.is_some()
        || update.text_flags.is_some()
        // Der Stack liegender Items und Wurfgeschosse -- fehlte, deshalb blieben liegende
        // Items unsichtbar.
        || update.item_stack.is_some()
        // Schuetze einer Feuerwerksrakete: daran haengt der Elytra-Schub.
        || update.firework_shooter.is_some()
        // Steckt der Pfeil? -- fehlte, deshalb fielen steckende Pfeile aus der Wand.
        || update.arrow_in_ground.is_some()
        // Groesse der interaction-Klickflaeche -- fehlte, deshalb liess sich das Tor auf
        // mcpvp.club nicht schlagen.
        || update.interaction_width.is_some()
        || update.interaction_height.is_some()
}

#[cfg(test)]
mod meta_filter_neu_tests {
    use super::*;

    #[test]
    fn pfeil_und_interaction_kommen_durch() {
        let leer = packets::EntityDataUpdate::default();
        assert!(!meta_betrifft_anzeige(&leer));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { arrow_in_ground: Some(true), ..Default::default() }));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { interaction_width: Some(3.0), ..Default::default() }));
        assert!(meta_betrifft_anzeige(&packets::EntityDataUpdate { interaction_height: Some(3.0), ..Default::default() }));
    }
}

// Beantwortet einen Cookie Request -- in JEDEM Zustand, nur mit der passenden Antwort-ID.
//
// Wichtig ist das "immer antworten": kennen wir das Cookie nicht, geht trotzdem eine Antwort
// raus, nur eben eine leere. Vanilla macht es genauso, und ein Server, der auf die Antwort
// wartet, haengt sonst bis zu seinem Timeout und trennt die Verbindung -- fuer den Spieler
// ein grundloser Kick, fuer den Server ein Client, der sich totstellt.
async fn answer_cookie_request(conn: &mut Connection, payload: &[u8], response_id: i32) -> Result<(), NetError> {
    let key = packets::read_cookie_key(payload).map_err(|_| NetError::Protocol("Cookie Request".into()))?;
    let value = conn.cookies.get(&key).cloned();
    conn.send(&packets::cookie_response_payload(response_id, &key, value.as_deref())).await
}

// Legt ein vom Server geschicktes Cookie ab. Zu grosse verwirft read_store_cookie, wie Vanilla.
fn store_cookie(conn: &mut Connection, payload: &[u8]) {
    if let Ok(Some((key, value))) = packets::read_store_cookie(payload) {
        conn.cookies.insert(key, value);
    }
}

// Ziel eines Transfer-Pakets: Host + Port, so wie der Server sie schickt.
type TransferTarget = (String, u16);

// Configuration-State: Client Information senden, Known Packs beantworten, Keep Alive/Ping
// bedienen, bis Finish Configuration kommt -> in Play-State wechseln (None). Some = der Server
// hat uns stattdessen weitergeschickt (Transfer).
async fn do_configuration(conn: &mut Connection, events: &Events, outgoing: &mut Outgoing) -> Result<Option<TransferTarget>, NetError> {
    conn.send(&packets::client_information_payload()).await?;
    // Direkt danach der Brand -- genau die Reihenfolge, die Vanilla verwendet.
    conn.send(&packets::brand_payload(packets::CLIENT_BRAND)).await?;

    // Jede unbehandelte Config-Packet-ID EINMAL protokollieren -- dieselbe Technik wie im
    // Play-Loop. Ohne das ist ein Disconnect im Configuration-State eine Blackbox: man sieht
    // nur, dass es vorbei ist, nicht welche Anforderung des Servers wir uebergangen haben
    // (Resourcepack, Cookie, Transfer, ...).
    let mut seen_unhandled: Vec<i32> = Vec::new();
    let mut outgoing_closed = false;

    loop {
        // Auch hier den Sendekanal bedienen: Server, die ihr Resourcepack in der Konfiguration
        // schicken, warten oft auf "geladen", bevor sie die Konfiguration beenden. Die App
        // schreibt Status-Antworten mit der Play-ID; alles andere waere hier ein falsches
        // Paket und faellt weg (die App sendet ausserhalb des Spiels sonst nichts).
        let payload = tokio::select! {
            biased;
            msg = outgoing.recv(), if !outgoing_closed => {
                match msg {
                    Some(p) => {
                        if let Some(config) = packets::config_outgoing_payload(&p) {
                            conn.send(&config).await?;
                        }
                    }
                    None => outgoing_closed = true,
                }
                continue;
            }
            result = conn.recv() => result?,
        };
        let id = packets::peek_packet_id(&payload).map_err(|_| NetError::Protocol("leeres Config-Packet".into()))?;
        match id {
            packets::CB_CONFIG_COOKIE_REQUEST => {
                answer_cookie_request(conn, &payload, packets::SB_CONFIG_COOKIE_RESPONSE).await?;
            }
            packets::CB_CONFIG_STORE_COOKIE => store_cookie(conn, &payload),
            packets::CB_CONFIG_RESOURCE_PACK_PUSH => match packets::read_resource_pack_push(&payload) {
                Ok(push) => {
                    let _ = events.send(ClientEvent::ResourcePackPush(push));
                }
                Err(_) => log(events, "Resourcepack-Anfrage nicht lesbar -- ignoriert"),
            },
            packets::CB_CONFIG_RESOURCE_PACK_POP => {
                if let Ok(id) = packets::read_resource_pack_pop(&payload) {
                    let _ = events.send(ClientEvent::ResourcePackPop(id));
                }
            }
            packets::CB_CONFIG_KNOWN_PACKS => {
                conn.send(&packets::serverbound_known_packs_empty_payload()).await?;
            }
            packets::CB_CONFIG_PLUGIN_MESSAGE => {
                if let Ok((channel, data)) = packets::read_custom_payload(&payload) {
                    let _ = events.send(ClientEvent::CustomPayload { channel, data });
                }
            }
            packets::CB_CONFIG_KEEP_ALIVE => {
                let ka = packets::read_keep_alive_id(&payload).map_err(|_| NetError::Protocol("KeepAlive".into()))?;
                conn.send(&packets::keep_alive_config_payload(ka)).await?;
            }
            packets::CB_CONFIG_PING => {
                let ping = packets::read_ping_id(&payload).map_err(|_| NetError::Protocol("Ping".into()))?;
                conn.send(&packets::pong_config_payload(ping)).await?;
            }
            packets::CB_CONFIG_DISCONNECT => {
                return Err(NetError::Protocol(disconnect_message("Configuration", &payload)));
            }
            packets::CB_CONFIG_FINISH => {
                conn.send(&packets::acknowledge_finish_configuration_payload()).await?;
                return Ok(None); // -> Play-State
            }
            packets::CB_CONFIG_TRANSFER => {
                let ziel = packets::read_transfer(&payload).map_err(|_| NetError::Protocol("Transfer-Paket nicht lesbar".into()))?;
                return Ok(Some(ziel));
            }
            protocol::recipe::CB_CONFIG_UPDATE_TAGS => {
                if let Ok(tags) = protocol::recipe::read_item_tags(&payload) {
                    let _ = events.send(ClientEvent::ItemTags(tags));
                }
            }
            packets::CB_CONFIG_RESET_CHAT => {
                let _ = events.send(ClientEvent::ResetChat);
            }
            // Schaltet in Vanilla experimentelle Inhalte frei; dieser Client kennt ohnehin
            // nur die festen Item- und Blocklisten, braucht die Liste also nicht.
            packets::CB_CONFIG_UPDATE_ENABLED_FEATURES => {
                if let Ok(features) = packets::read_enabled_features(&payload) {
                    if features.iter().any(|f| f != "minecraft:vanilla") {
                        log(events, format!("Server schaltet Features frei: {}", features.join(", ")));
                    }
                }
            }
            // Zusaetze fuer Absturzberichte -- dieser Client schreibt keine.
            packets::CB_CONFIG_CUSTOM_REPORT_DETAILS => {}
            packets::CB_CONFIG_SERVER_LINKS => match packets::read_server_links(&payload) {
                Ok(links) => {
                    let _ = events.send(ClientEvent::ServerLinks(links));
                }
                Err(_) => log(events, "Server-Links nicht lesbar -- ignoriert"),
            },
            packets::CB_CONFIG_SHOW_DIALOG => match packets::read_show_dialog_config(&payload) {
                Ok(dialog) => {
                    let _ = events.send(ClientEvent::ShowDialog(dialog));
                }
                Err(_) => log(events, "Dialog nicht lesbar -- ignoriert"),
            },
            packets::CB_CONFIG_CLEAR_DIALOG => {
                let _ = events.send(ClientEvent::ClearDialog);
            }
            packets::CB_CONFIG_CODE_OF_CONDUCT => {
                let text = packets::read_code_of_conduct(&payload).map_err(|_| NetError::Protocol("Verhaltenskodex nicht lesbar".into()))?;
                let _ = events.send(ClientEvent::CodeOfConduct(text));
            }
            packets::CB_CONFIG_REGISTRY_DATA => {
                // Nur die gebrauchten Registries werden gelesen, der Rest (Painting, ...) wird
                // ignoriert -- jeder Leser liefert None, wenn dieses Paket eine andere
                // Registry trug.
                //
                // Biome: Gras-/Laub-/Wasser-Faerbung.
                match packets::read_registry_data(&payload) {
                    Ok(Some(biomes)) => {
                        let _ = events.send(ClientEvent::BiomeRegistry(biomes));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Biome) konnte nicht gelesen werden -- ignoriert"),
                }
                // Dimensionsarten: min_y und Hoehe. Ohne sie steht die ganze Welt in der
                // falschen Hoehe, sobald ein Server nicht die Overworld-Masse benutzt.
                match packets::read_registry_dimension_types(&payload) {
                    Ok(Some(dims)) => {
                        let _ = events.send(ClientEvent::DimensionRegistry(dims));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Dimensionsarten) konnte nicht gelesen werden -- ignoriert"),
                }
                // Schadensarten: welcher Klang beim eigenen Treffer gehoert wird. Der Index im
                // Schadenspaket zeigt in genau diese Liste.
                match packets::read_damage_types(&payload) {
                    Ok(Some(effects)) => {
                        let _ = events.send(ClientEvent::DamageTypes(effects));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Schadensarten) konnte nicht gelesen werden -- ignoriert"),
                }
                // Verzauberungen: Namen, Tooltips, und die Effekte, die der Client selbst
                // ausfuehrt (Schnellladen, Sog, Fluch der Bindung -- s. world::enchantment).
                match packets::read_enchantments(&payload) {
                    Ok(Some(entries)) => {
                        let _ = events.send(ClientEvent::Enchantments(entries));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Verzauberungen) konnte nicht gelesen werden -- ignoriert"),
                }
                match packets::read_named_registry(&payload) {
                    Ok(Some((registry, entries))) => {
                        let _ = events.send(ClientEvent::NamedRegistry { registry, entries });
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Trims/Banner) konnte nicht gelesen werden -- ignoriert"),
                }
            }
            // Plugin Message, Update Tags etc. -- fuer den Join ignorierbar, aber einmal
            // sichtbar machen (s. seen_unhandled).
            _ => {
                if !seen_unhandled.contains(&id) {
                    seen_unhandled.push(id);
                    log(events, format!("Config-Paket 0x{id:02X} nicht behandelt ({} Bytes)", payload.len()));
                }
            }
        }
    }
}

// Play-State: haelt die Verbindung offen und bedient Keep Alive + Teleport-Bestaetigung,
// damit der Spieler verbunden bleibt und wirklich spawnt. Laeuft bis Disconnect/Fehler oder
// bis der Server uns weiterschickt (Ok mit dem Transfer-Ziel).
// Unbekannte Packet-IDs werden geloggt (einmalig pro ID), um die versionsabhaengigen
// Play-IDs empirisch gegen den echten Server verifizieren zu koennen.
async fn run_play_loop(
    conn: &mut Connection,
    events: &Events,
    outgoing: &mut Outgoing,
    rotation: &SharedRotation,
) -> Result<TransferTarget, NetError> {
    log(events, "Play-State erreicht -- im Spiel");
    conn.log.enter_play();
    let _ = events.send(ClientEvent::Connected);

    let mut stats = PlayStats::default();
    // Chunk-Stapel-Bestaetigung: OHNE die schickt der Server nach dem ersten Stapel keinen
    // einzigen Chunk mehr (s. packets::CB_PLAY_CHUNK_BATCH_START).
    let mut chunk_batch = crate::chunk_batch::ChunkBatchSizeCalculator::new();
    // Wird true, sobald die App ihren Sender fallen laesst (z.B. beim Reconnect/Trennen).
    // Ohne diese Wache wuerde outgoing.recv() danach bei jedem select! sofort wieder mit
    // None zurueckkommen -- eine Busy-Loop, die einen Kern voll auslastet.
    let mut outgoing_closed = false;

    // Chunk-Wachhund, s. CHUNK_WATCHDOG_INTERVAL. Nach einer Rekonfiguration faengt die
    // Wartezeit von vorn an -- der Server schickt danach eine komplett neue Welt.
    let mut play_start = std::time::Instant::now();
    let mut watchdog = tokio::time::interval(CHUNK_WATCHDOG_INTERVAL);
    watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            // biased + Sendezweig ZUERST: die Reihenfolge auf der Leitung darf nicht vom
            // Zufall abhaengen.
            //
            // ECHTER BUG gewesen, sichtbar als GrimAC "Post (use item)" und
            // "PacketOrderO (type=PONG)": ohne `biased` waehlt tokio::select! zwischen zwei
            // gleichzeitig bereiten Zweigen ZUFAELLIG. Traf ein Ping des Servers ein, waehrend
            // die Pakete eines Ticks noch in der Warteschlange lagen, konnte die Pong-Antwort
            // MITTEN in diesen Stapel geschrieben werden. Grims Post-Check merkt sich alle
            // Pakete seiner Liste nach dem Bewegungspaket und flaggt, sobald die Transaktion
            // (Pong) eintrifft und die Liste nicht leer ist -- ein Benutzen-Paket, das
            // zwischen Bewegungspaket und Pong rutscht, ist genau das.
            //
            // Vanilla kann das nicht: dort gehen die Pakete eines Ticks und die Ping-Antwort
            // durch DIESELBE Netty-Event-Loop-Warteschlange, die Reihenfolge ist also die der
            // Einreihung -- ein waehrend des Tick-Stapels eintreffender Ping wird erst nach
            // dem Stapel beantwortet. Genau das stellt `biased` mit dem Sendezweig vorn her.
            //
            // Aushungern kann der Empfangszweig dadurch nicht: die App erzeugt eine Handvoll
            // Pakete je 50-ms-Tick, die Warteschlange ist praktisch immer sofort leer.
            biased;
            msg = outgoing.recv(), if !outgoing_closed => {
                match msg {
                    Some(payload) => conn.send(&payload).await?,
                    None => outgoing_closed = true,
                }
            }
            result = conn.recv() => {
                let payload = result?;
                match handle_play_packet(conn, events, &payload, &mut stats, &mut chunk_batch, rotation).await? {
                    PlayFlow::Continue => {}
                    PlayFlow::Transfer(ziel) => return Ok(ziel),
                    PlayFlow::Reconfigure => {
                        // Zurueck in den Configuration-State und danach wieder hierher --
                        // s. packets::CB_PLAY_START_CONFIGURATION.
                        log(events, "Server schickt uns zurueck in die Konfiguration");
                        conn.send(&packets::configuration_acknowledged_payload()).await?;
                        let _ = events.send(ClientEvent::Reconfiguring);
                        if let Some(ziel) = do_configuration(conn, events, outgoing).await? {
                            return Ok(ziel);
                        }
                        log(events, "Konfiguration erneut abgeschlossen -- zurueck im Spiel");
                        // Alles, was zur alten Welt gehoert, faengt von vorn an: Zaehler,
                        // Chunk-Tempo und die Frist des Wachhunds.
                        stats = PlayStats::default();
                        chunk_batch = crate::chunk_batch::ChunkBatchSizeCalculator::new();
                        play_start = std::time::Instant::now();
                    }
                }
            }
            _ = watchdog.tick() => {
                let wartet = play_start.elapsed();
                if stats.chunks_ok == 0 && wartet >= CHUNK_WATCHDOG_GRACE {
                    log(events, stats.chunk_stall_report(wartet));
                    // Erneut bestaetigen: bleibt beim Server auch nur EINE Bestaetigung aus,
                    // steht die Chunk-Lieferung fuer immer (ChunkDataSender.sendChunkBatches
                    // sendet nur solange unacknowledgedBatches < maxUnacknowledgedBatches, und
                    // maxUnacknowledgedBatches ist vor der ersten Bestaetigung 1). Eine
                    // ueberzaehlige Bestaetigung ist dagegen harmlos: onAcknowledgeChunks
                    // zieht nur den Zaehler herunter (ohne untere Schranke) und setzt das
                    // Tempo neu -- sie kann die Lieferung also nur anschieben, nie bremsen.
                    stats.acks_sent += 1;
                    conn.send(&packets::chunk_batch_received_payload(chunk_batch.desired_chunks_per_tick())).await?;
                }
            }
        }
    }
}

// Wie oft der Chunk-Wachhund nachsieht und wie lange er dem normalen Ablauf Zeit laesst,
// bevor er sich meldet. Zweck: der Client blieb gelegentlich beim Verbinden haengen -- Chat,
// Inventar und Entities kamen an, aber kein einziger Chunk, und das Verbindungs-Overlay
// (build_connecting_overlay, sichtbar solange chunks_received == 0) stand fuer immer. Ohne
// Diagnose ist von aussen nicht unterscheidbar, ob der Server nichts mehr schickt, leere
// Stapel schickt oder wir die Chunk-Pakete nicht erkennen -- genau das steht jetzt im Log.
const CHUNK_WATCHDOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const CHUNK_WATCHDOG_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

// Zaehlwerk des Play-Loops. Frueher lose &mut-Parameter von handle_play_packet -- als Struct
// zusammengefasst, weil der Wachhund oben denselben Stand braucht.
#[derive(Default)]
struct PlayStats {
    chunks_ok: usize,
    chunks_failed: usize,
    entity_errors: usize,
    // Alle Play-Pakete seit dem Betreten des Play-States -- die Zahl beantwortet die erste
    // Frage bei einem Haenger: kommt ueberhaupt noch etwas an?
    packets: usize,
    batches: usize,
    last_batch_size: i32,
    acks_sent: usize,
    // Jede nicht behandelte Packet-ID einmal (gedeckelt). Kommen Chunks unter einer ID an,
    // die wir nicht kennen, steht sie hier -- sonst waere dieser Fall unsichtbar.
    unknown_ids: Vec<i32>,
    // Nur unter CORAL_SOUND_DEBUG gefuellt: wie oft jede Paket-ID kam, und unter welchen
    // IDs ein Paket sich als Ton-Paket lesen liess (s. packets::looks_like_sound). Damit ist
    // "es kommt kein Ton an" beantwortbar, ohne IDs zu raten -- die Antwort steht dann
    // entweder in einer leeren Liste (der Server schickt wirklich nichts) oder in einer ID,
    // die wir nicht als Ton behandeln.
    id_counts: std::collections::BTreeMap<i32, u32>,
    sound_hits: std::collections::BTreeMap<i32, (u32, String)>,
    last_dump: Option<std::time::Instant>,
    // Markierung fuer die einmalige CORAL_PACKET_SCAN-Ausgabe des ersten Play-Pakets.
    first_packet_scanned: bool,
    // Welt-RegistryKey der Dimension, in der wir gerade stecken (aus Login bzw. Respawn).
    // Nur dafuer da, beim naechsten Respawn Tod (gleiche Dimension) von Portal (andere
    // Dimension) zu unterscheiden -- s. packets::CB_PLAY_RESPAWN.
    dimension: Option<String>,
}

impl PlayStats {
    const MAX_UNKNOWN_IDS: usize = 16;

    // Alle 10 Sekunden ein Lagebild. Bewusst nicht je Paket: in einer belebten Szene waeren
    // das hunderte Zeilen je Sekunde, und die eine Zeile, auf die es ankommt, ginge unter.
    fn maybe_dump_sound_report(&mut self) {
        const INTERVALL: std::time::Duration = std::time::Duration::from_secs(10);
        let jetzt = std::time::Instant::now();
        match self.last_dump {
            // Noch keine volle Runde vorbei.
            Some(t) if jetzt.duration_since(t) < INTERVALL => return,
            // Erstes Paket: die Uhr stellen, aber noch nichts berichten -- ein Lagebild ueber
            // ein einziges Paket sagt nichts.
            None => {
                self.last_dump = Some(jetzt);
                return;
            }
            _ => self.last_dump = Some(jetzt),
        }

        eprintln!("[ton] --- Lagebild: {} Play-Pakete, {} verschiedene IDs ---", self.packets, self.id_counts.len());
        if self.sound_hits.is_empty() {
            eprintln!("[ton] KEIN einziges Paket liess sich als Ton-Paket lesen.");
            // Die haeufigsten IDs mitgeben: wenn der Server ueberhaupt nichts schickt, sieht
            // man das hier sofort an einer fast leeren Liste.
            let mut top: Vec<_> = self.id_counts.iter().map(|(id, n)| (*n, *id)).collect();
            top.sort_unstable_by(|a, b| b.cmp(a));
            let liste = top.iter().take(12).map(|(n, id)| format!("0x{id:02x}x{n}")).collect::<Vec<_>>().join(" ");
            eprintln!("[ton] haeufigste IDs: {liste}");
        } else {
            for (id, (n, name)) in &self.sound_hits {
                let behandelt = *id == packets::CB_PLAY_SOUND || *id == packets::CB_PLAY_SOUND_ENTITY;
                eprintln!(
                    "[ton] ID 0x{id:02x}: {n}x als Ton lesbar, zuletzt {name}{}",
                    if behandelt { "" } else { "   <-- DIESE ID BEHANDELN WIR NICHT" }
                );
            }
        }
    }

    fn note_unknown_id(&mut self, id: i32) -> bool {
        if self.unknown_ids.contains(&id) {
            return false;
        }
        if self.unknown_ids.len() < Self::MAX_UNKNOWN_IDS {
            self.unknown_ids.push(id);
        }
        true
    }

    fn chunk_stall_report(&self, wartet: std::time::Duration) -> String {
        let ids = if self.unknown_ids.is_empty() {
            String::from("keine")
        } else {
            self.unknown_ids.iter().map(|id| format!("0x{id:02X}")).collect::<Vec<_>>().join(" ")
        };
        format!(
            "keine Chunk-Daten seit {:.0}s: {} Pakete, {} Stapel (zuletzt {} Chunks), {} Fehl-Chunks, {} Bestaetigungen, unbekannte IDs: {ids}",
            wartet.as_secs_f32(),
            self.packets,
            self.batches,
            self.last_batch_size,
            self.chunks_failed,
            self.acks_sent,
        )
    }
}

// Ausgelagert aus run_play_loop, damit die select!-Schleife dort uebersichtlich bleibt --
// diese Funktion entscheidet nur noch, was mit EINEM eingehenden Packet zu tun ist.
// Entity-Parse-Fehler: die ersten paar melden, danach schweigen (nicht das Log fluten --
// bei einem Formatfehler kommen die Pakete im 20-Hz-Takt).
// CORAL_SOUND_DEBUG=1: schreibt jedes eingehende Ton-Paket mit -- Name, Kategorie, Ort
// und ob es ueberhaupt lesbar war. Gebaut, weil "ich hoere nichts" von aussen nicht
// unterscheidbar ist zwischen: der Server schickt gar nichts, das Paket ist unlesbar, der
// Klang ist unbekannt, die Quelle ist nicht auffindbar oder die Datei fehlt. Die Antwort
// steht in genau einer dieser Zeilen (Gegenstueck in app::play_server_sound).
//
// Einmal abgefragt und gemerkt: das laeuft im Paketpfad, und dort will man keine
// Umgebungsvariable je Geraeusch nachschlagen.
pub fn sound_debug() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("CORAL_SOUND_DEBUG").is_some())
}

fn log_entity_parse_error(events: &Events, entity_errors: &mut usize, kind: &str) {
    *entity_errors += 1;
    if *entity_errors <= 3 {
        log(events, format!("FEHLER: Entity-Packet nicht dekodierbar ({kind}, Fall {entity_errors})"));
    }
}

// Was der Play-Loop nach diesem Paket tun soll. Nur der Zustandswechsel muss nach oben
// gegeben werden -- alles andere erledigt handle_play_packet selbst.
enum PlayFlow {
    Continue,
    Reconfigure,
    Transfer(TransferTarget),
}

async fn handle_play_packet(
    conn: &mut Connection,
    events: &Events,
    payload: &[u8],
    stats: &mut PlayStats,
    chunk_batch: &mut crate::chunk_batch::ChunkBatchSizeCalculator,
    rotation: &SharedRotation,
) -> Result<PlayFlow, NetError> {
    let id = packets::peek_packet_id(payload).map_err(|_| NetError::Protocol("leeres Play-Packet".into()))?;
    stats.packets += 1;

    if sound_debug() {
        *stats.id_counts.entry(id).or_default() += 1;
        // Jedes Paket versuchsweise als Ton lesen -- unabhaengig davon, unter welcher ID es
        // kam. Kommen Toene unter einer anderen ID an, steht es damit im Bericht, statt dass
        // erneut Paket-IDs abgezaehlt werden muessten.
        if let Some((entity_gebunden, name)) = packets::looks_like_sound(payload) {
            let eintrag = stats.sound_hits.entry(id).or_insert_with(|| (0, String::new()));
            eintrag.0 += 1;
            eintrag.1 = format!("{name}{}", if entity_gebunden { " (an Entity)" } else { "" });
        }
        stats.maybe_dump_sound_report();
    }

    // TEMPORAERE Diagnose (CORAL_PACKET_SCAN=1): Login (play) ist garantiert das ALLERERSTE
    // Play-Packet -- loggt dessen ID + erste Bytes, um playerId (erstes Feld, 4-Byte-Int laut
    // ClientboundLoginPacket) ohne Raten zu lokalisieren.
    if std::env::var_os("CORAL_PACKET_SCAN").is_some() && !stats.first_packet_scanned {
        eprintln!("[scan] ERSTES Play-Packet: id=0x{id:02x} bytes={:02x?}", &payload[..payload.len().min(24)]);
        stats.first_packet_scanned = true;
    }

    if id == packets::CB_PLAY_START_CONFIGURATION {
        return Ok(PlayFlow::Reconfigure);
    }
    if id == packets::CB_PLAY_TRANSFER {
        let ziel = packets::read_transfer(payload).map_err(|_| NetError::Protocol("Transfer-Paket nicht lesbar".into()))?;
        return Ok(PlayFlow::Transfer(ziel));
    }

    if id == packets::CB_PLAY_CHUNK_DATA {
        // Ein Parse-Fehler heisst hier: Format falsch verstanden (die Sections gingen
        // nicht exakt im Data-Feld auf). Nicht die Verbindung abbrechen, aber zaehlen
        // und melden -- so sehen wir sofort, ob die Dekodierung wirklich stimmt.
        match protocol::chunk::parse_chunk_data(payload) {
            Ok(chunk) => {
                if stats.chunks_ok == 0 {
                    let non_empty = chunk.sections.iter().filter(|s| !s.is_empty()).count();
                    log(
                        events,
                        format!(
                            "erster Chunk dekodiert: ({}, {}), {} Sections, {} nicht leer",
                            chunk.x,
                            chunk.z,
                            chunk.sections.len(),
                            non_empty
                        ),
                    );
                }
                stats.chunks_ok += 1;
                if stats.chunks_ok.is_multiple_of(50) {
                    log(events, format!("Chunks dekodiert: {} ok, {} Fehler", stats.chunks_ok, stats.chunks_failed));
                }
                let _ = events.send(ClientEvent::Chunk(chunk));
            }
            Err(_) => {
                stats.chunks_failed += 1;
                if stats.chunks_failed == 1 {
                    log(
                        events,
                        format!("FEHLER: Chunk nicht dekodierbar. Layout-Analyse:\n{}", protocol::chunk::debug_chunk_layout(payload)),
                    );
                }
            }
        }
    } else if id == packets::CB_PLAY_CHUNK_BATCH_START {
        chunk_batch.on_start_chunk_send();
    } else if id == packets::CB_PLAY_CHUNK_BATCH_FINISHED {
        // PFLICHT-ANTWORT, nicht Kosmetik: der Server sendet erst weiter, wenn dieser
        // Stapel bestaetigt ist (ChunkDataSender.maxUnacknowledgedBatches startet bei 1).
        // Deshalb auch bei unlesbarer Groesse antworten -- ohne Bestaetigung stuende die
        // Chunk-Lieferung fuer immer, mit einer ungenauen Tempo-Angabe nur ein Stapel.
        let batch_size = packets::read_chunk_batch_size(payload).unwrap_or(0);
        chunk_batch.on_chunk_sent(batch_size);
        let desired = chunk_batch.desired_chunks_per_tick();
        stats.batches += 1;
        stats.last_batch_size = batch_size;
        // Solange noch kein Chunk angekommen ist, JEDEN Stapel melden (nicht nur den ersten):
        // beim Haenger ist gerade die Frage entscheidend, ob der Server weiter Stapel schickt
        // -- und wenn ja, ob leere. Nach dem 3. Stapel uebernimmt der Wachhund das Zaehlen,
        // sonst waeren die 6 Zeilen des Verbindungs-Overlays sofort vollgeschrieben.
        if stats.chunks_ok == 0 && stats.batches <= 3 {
            log(events, format!("Chunk-Stapel {} bestaetigt ({batch_size} Chunks, Wunsch {desired:.1}/Tick)", stats.batches));
        }
        stats.acks_sent += 1;
        conn.send(&packets::chunk_batch_received_payload(desired)).await?;
    } else if id == packets::CB_PLAY_LOGIN {
        if let Ok(entity_id) = packets::read_login_player_id(payload) {
            let _ = events.send(ClientEvent::OwnEntityId(entity_id));
        }
        // Ausgangspunkt fuer den Dimensionsvergleich beim ersten Respawn.
        stats.dimension = packets::read_login_dimension(payload).ok();
        if let Ok((dimension, seed)) = packets::read_login_world(payload) {
            let _ = events.send(ClientEvent::WorldIdentity { dimension, seed });
        }
        if let Ok(index) = packets::read_login_dimension_type(payload) {
            let _ = events.send(ClientEvent::DimensionType(index));
        }
        if let Ok(mode) = packets::read_login_game_mode(payload) {
            let _ = events.send(ClientEvent::GameMode(mode));
        }
    } else if id == packets::CB_PLAY_RESPAWN {
        // Die Dimension entscheidet, ob es ein Weltwechsel war (s. packets::CB_PLAY_RESPAWN).
        // Laesst sie sich nicht lesen oder kennen wir die alte nicht, gilt das NICHT als
        // Wechsel: ein faelschlich zurueckgesetzter Zaehler flaggt garantiert, ein nicht
        // zurueckgesetzter nur, wenn wirklich die Dimension gewechselt hat.
        let neue_dimension = packets::read_respawn_dimension(payload).ok();
        let world_change = match (&stats.dimension, &neue_dimension) {
            (Some(alt), Some(neu)) => alt != neu,
            _ => false,
        };
        if neue_dimension.is_some() {
            stats.dimension = neue_dimension;
        }
        if let Ok(index) = packets::read_respawn_dimension_type(payload) {
            let _ = events.send(ClientEvent::DimensionType(index));
        }
        if let Ok((dimension, seed)) = packets::read_respawn_world(payload) {
            let _ = events.send(ClientEvent::WorldIdentity { dimension, seed });
        }
        let _ = events.send(ClientEvent::Respawned { world_change });
        // NACH Respawned: die App setzt dort den Spieler zurueck.
        if let Ok(mode) = packets::read_respawn_game_mode(payload) {
            let _ = events.send(ClientEvent::GameMode(mode));
        }
    } else if id == packets::CB_PLAY_GAME_EVENT {
        if let Ok((reason, value)) = packets::read_game_event(payload) {
            match reason {
                packets::GAME_EVENT_GAME_MODE_CHANGED => {
                    let _ = events.send(ClientEvent::GameMode(value as u8));
                }
                packets::GAME_EVENT_RAIN_STARTED => {
                    let _ = events.send(ClientEvent::RainGradient(0.0));
                }
                packets::GAME_EVENT_RAIN_STOPPED => {
                    let _ = events.send(ClientEvent::RainGradient(1.0));
                }
                packets::GAME_EVENT_RAIN_GRADIENT => {
                    let _ = events.send(ClientEvent::RainGradient(value.clamp(0.0, 1.0)));
                }
                packets::GAME_EVENT_THUNDER_GRADIENT => {
                    let _ = events.send(ClientEvent::ThunderGradient(value.clamp(0.0, 1.0)));
                }
                packets::GAME_EVENT_INITIAL_CHUNKS_COMING => {
                    let _ = events.send(ClientEvent::InitialChunksComing);
                }
                packets::GAME_EVENT_PROJECTILE_HIT_PLAYER => {
                    let _ = events.send(ClientEvent::ProjectileHitPlayer);
                }
                _ => {}
            }
        }
    } else if id == packets::CB_PLAY_KEEP_ALIVE {
        let ka = packets::read_keep_alive_id(payload).map_err(|_| NetError::Protocol("KeepAlive".into()))?;
        conn.send(&packets::keep_alive_play_payload(ka)).await?;
    } else if id == packets::CB_PLAY_COOKIE_REQUEST {
        answer_cookie_request(conn, payload, packets::SB_PLAY_COOKIE_RESPONSE).await?;
    } else if id == packets::CB_PLAY_STORE_COOKIE {
        store_cookie(conn, payload);
    } else if id == packets::CB_PLAY_LIGHT_UPDATE {
        if let Ok((x, z, light)) = protocol::chunk::parse_light_update(payload) {
            let _ = events.send(ClientEvent::LightUpdate { x, z, light });
        }
    } else if id == packets::CB_PLAY_RESOURCE_PACK_PUSH {
        match packets::read_resource_pack_push(payload) {
            Ok(push) => {
                let _ = events.send(ClientEvent::ResourcePackPush(push));
            }
            Err(_) => log(events, "Resourcepack-Anfrage nicht lesbar -- ignoriert"),
        }
    } else if id == packets::CB_PLAY_RESOURCE_PACK_POP {
        if let Ok(id) = packets::read_resource_pack_pop(payload) {
            let _ = events.send(ClientEvent::ResourcePackPop(id));
        }
    } else if id == packets::CB_PLAY_SERVER_LINKS {
        match packets::read_server_links(payload) {
            Ok(links) => {
                let _ = events.send(ClientEvent::ServerLinks(links));
            }
            Err(_) => log(events, "Server-Links nicht lesbar -- ignoriert"),
        }
    } else if id == packets::CB_PLAY_SHOW_DIALOG {
        match packets::read_show_dialog_play(payload) {
            Ok(dialog) => {
                let _ = events.send(ClientEvent::ShowDialog(dialog));
            }
            Err(_) => log(events, "Dialog nicht lesbar -- ignoriert"),
        }
    } else if id == packets::CB_PLAY_CLEAR_DIALOG {
        let _ = events.send(ClientEvent::ClearDialog);
    } else if id == packets::CB_PLAY_CUSTOM_REPORT_DETAILS {
        // Zusaetze fuer Absturzberichte -- dieser Client schreibt keine.
    } else if id == packets::CB_PLAY_PING {
        let ping = packets::read_ping_id(payload).map_err(|_| NetError::Protocol("Ping".into()))?;
        interaktions_log("pong");
        conn.send(&packets::pong_play_payload(ping)).await?;
    } else if id == packets::CB_PLAY_PLAYER_POSITION {
        let tp = packets::read_teleport_id(payload).map_err(|_| NetError::Protocol("TeleportId".into()))?;
        // Ein Teleport ist fuer GrimAC eine Sonder-Tick-Grenze: das Bewegungspaket, das ihn
        // annimmt, setzt seine Per-Tick-Merker NICHT zurueck (Check.isTickPacket ->
        // !lastPacketWasTeleport). Zwei unserer Ticks fallen dann fuer ihn in einen zusammen.
        interaktions_log("teleport(server)");
        conn.send(&packets::confirm_teleport_payload(tp)).await?;

        // Position erst nach dem Bestaetigen melden: die Teleport-Bestaetigung ist
        // Pflicht, ein Parse-Fehler in der Position darf sie nicht verhindern.
        if let Ok(sync) = packets::read_player_position(payload) {
            let (x, y, z) = sync.pos;
            let (yaw, pitch, flags) = (sync.yaw, sync.pitch, sync.flags);
            log(events, format!("Spawn-Position: {x:.1} / {y:.1} / {z:.1}"));

            // ECHTER BUG gewesen (BadPacketsN-Endlosschleife -> Dauer-Setback
            // zum Spawn, spaeter zusaetzlich Timer/duplicate-look). Grims Teleport-
            // Warteschlange (SetbackTeleportUtil.checkTeleportQueue) akzeptiert einen Server-
            // Teleport NUR, wenn das naechste Bewegungspaket die Zielposition bei absoluten
            // Teleports EXAKT meldet (Schwelle 0.0) UND es GENAU im Transaktionsfenster
            // ankommt: lastTransactionReceived == teleport.transaction. Grim sendet vor UND
            // nach dem Teleport je eine Transaktion (clientbound Ping 0x3B); ist unser
            // Pong fuer die NACH-Transaktion schon raus, bevor unser Bewegungspaket ankommt,
            // gilt lastTransactionReceived > teleport.transaction -> BadPacketsN + neuer
            // Setback (im Proxy-Log als Endlosschleife sichtbar).
            //
            // Genau DAS passierte, weil die Pongs (0x2C) hier synchron im Netz-Thread
            // rausgehen, das Bewegungspaket aber erst einen Frame spaeter im App-Thread --
            // also immer NACH den Pongs. Vanilla (ClientPlayNetworkHandler.onPlayerPosition-
            // Look) sendet die Bestaetigungs-Bewegung dagegen in-order direkt nach dem
            // TeleportConfirm, vor dem naechsten Pong. Hier genau nachgebaut: sofort, im selben
            // Netz-Thread.
            //
            // ZWINGEND ein Full-Paket (Position UND Rotation), KEIN reines Positions-Paket:
            // Grim ruft die Teleport-Warteschlangen-Pruefung (SetbackTeleportUtil.
            // checkTeleportQueue, die den Teleport ueberhaupt erst als akzeptiert markiert) NUR
            // fuer Pakete auf, die BEIDES aendern -- CheckManagerListener:
            //   flying.hasPositionChanged() && flying.hasRotationChanged() ? checkTeleportQueue(..) : ..
            // Ein reines Positions-Paket wird dort gar nicht geprueft, der Teleport bliebe ewig
            // unbestaetigt -> BadPacketsN-Endlosschleife (genau der zuvor beobachtete Bug, als
            // hier faelschlich move_player_pos_payload stand). Vanilla sendet an dieser Stelle
            // ebenfalls ein Full (onGround=false). Rotation kommt aus dem von der App laufend
            // gepflegten SharedRotation-Wert; bei relativer Rotation (Setbacks, Flag gesetzt)
            // ist das exakt die beizubehaltende aktuelle Blickrichtung, bei absoluter (Spawn)
            // nehmen wir die Rotation aus dem Teleport selbst. Nur fuer ABSOLUTE Position (kein
            // REL_X/Y/Z): bei relativer Position fehlt uns hier die alte -- selten und nicht die
            // Setback-/Spawn-Faelle, die die Schleife ausloesen.
            let pos_absolute = flags & (packets::POSITION_FLAG_REL_X | packets::POSITION_FLAG_REL_Y | packets::POSITION_FLAG_REL_Z) == 0;
            if pos_absolute {
                let (cur_yaw, cur_pitch) = rotation.get();
                // Sitzend nimmt Vanilla die Drehung des Pakets nicht an und meldet die eigene
                // (s. SharedLook) -- sonst stellte der Server unseren Kopf auf seinen alten
                // Stand zurueck, bei jedem Teleport des Fahrzeugs.
                let sitzt = rotation.riding();
                let send_yaw = if sitzt { cur_yaw } else if flags & packets::POSITION_FLAG_REL_YAW != 0 { cur_yaw + yaw } else { yaw };
                let send_pitch = if sitzt { cur_pitch } else if flags & packets::POSITION_FLAG_REL_PITCH != 0 { cur_pitch + pitch } else { pitch };
                conn.send(&packets::move_player_pos_rot_payload(x, y, z, send_yaw, send_pitch, false, false)).await?;
            }

            let _ = events.send(ClientEvent::PlayerPosition(sync));
        }
    } else if id == packets::CB_PLAY_PLAYER_ROTATION {
        // Der Server dreht unsere Kamera, ohne uns zu bewegen -- eigenes Paket seit 1.21.2.
        // Legitim benutzt von Plugins fuer Zwischensequenzen, "schau hierhin"-Effekte,
        // Minispiel-Kameras und Aufzuege/Fahrstuehle. Vanilla folgt dem kommentarlos.
        //
        // nach ClientPlayNetworkHandler.onPlayerRotation (1.21.11, nachgesehen):
        //   1. Relativ-Flags je Achse EINZELN aufloesen (PositionFlag.ofRot),
        //   2. Yaw/Pitch setzen, updateLastAngles() -- also ohne Nachziehen/Interpolation,
        //   3. SOFORT ein LookAndOnGround(yaw, pitch, false, false) zuruecksenden.
        //
        // Schritt 3 ist keine Kosmetik, sondern die Bestaetigung: der Server erfaehrt nur so,
        // dass wir die Drehung uebernommen haben. Genau wie bei der Teleport-Bestaetigung
        // muss sie IM NETZ-TASK rausgehen und nicht erst einen Frame spaeter aus dem
        // App-Thread -- sonst liegt sie hinter dem naechsten Pong, und fuer eine
        // transaktionsbasierte Pruefung ist die Antwort damit im falschen Fenster (derselbe
        // Fehler, der beim Teleport die BadPacketsN-Endlosschleife ausgeloest hat).
        //
        // Vanilla schickt hier fest onGround=false und horizontalCollision=false -- nicht
        // unseren echten Zustand. Bewusst uebernommen: eine Abweichung davon waere genau die
        // Art Detail, an der eine serverseitige Pruefung uns von einem echten Client
        // unterscheidet.
        if let Ok((yaw, relative_yaw, pitch, relative_pitch)) = packets::read_player_rotation(payload) {
            let (cur_yaw, cur_pitch) = rotation.get();
            let neu_yaw = if relative_yaw { cur_yaw + yaw } else { yaw };
            let neu_pitch = if relative_pitch { cur_pitch + pitch } else { pitch };
            interaktions_log("server_rotation");
            conn.send(&packets::move_player_rot_payload(neu_yaw, neu_pitch, false, false)).await?;
            // Erst NACH dem Senden in den geteilten Wert schreiben: kaeme die App vorher dazu,
            // ein eigenes Bewegungspaket zu schicken, stuende dort schon die neue Rotation,
            // bevor der Server sie bestaetigt bekommen hat.
            rotation.set(neu_yaw, neu_pitch);
            let _ = events.send(ClientEvent::ServerRotation { yaw_deg: neu_yaw, pitch_deg: neu_pitch });
        }
    } else if id == packets::CB_PLAY_FORGET_LEVEL_CHUNK {
        if let Ok((x, z)) = packets::read_forget_level_chunk(payload) {
            let _ = events.send(ClientEvent::ForgetChunk(x, z));
        }
    } else if id == packets::CB_PLAY_ADD_ENTITY {
        // Parse-Fehler bei Entity-Paketen: Format falsch verstanden. Wie bei Chunks nicht
        // die Verbindung abbrechen, aber die ersten Faelle melden, damit es auffaellt.
        match packets::read_add_entity(payload) {
            Ok(spawn) => {
                let _ = events.send(ClientEvent::EntitySpawn(spawn));
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "AddEntity"),
        }
    } else if id == packets::CB_PLAY_MOVE_ENTITY_POS {
        match packets::read_move_entity_pos(payload) {
            Ok((eid, dx, dy, dz)) => {
                let _ = events.send(ClientEvent::EntityMoveRel { id: eid, dx, dy, dz, rot: None });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "MoveEntityPos"),
        }
    } else if id == packets::CB_PLAY_MOVE_ENTITY_POS_ROT {
        match packets::read_move_entity_pos_rot(payload) {
            Ok((eid, dx, dy, dz, yaw, pitch)) => {
                let _ = events.send(ClientEvent::EntityMoveRel { id: eid, dx, dy, dz, rot: Some((yaw, pitch)) });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "MoveEntityPosRot"),
        }
    } else if id == packets::CB_PLAY_MOVE_ENTITY_ROT {
        match packets::read_move_entity_rot(payload) {
            Ok((eid, yaw, pitch)) => {
                let _ = events.send(ClientEvent::EntityRot { id: eid, yaw_deg: yaw, pitch_deg: pitch });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "MoveEntityRot"),
        }
    } else if id == packets::CB_PLAY_ENTITY_POSITION_SYNC || id == packets::CB_PLAY_TELEPORT_ENTITY {
        // Teleport Entity teilt das Layout bis einschliesslich Pitch mit Position Sync
        // (s. packets.rs); die Relativ-Flags am Ende werden vorerst nicht ausgewertet.
        match packets::read_entity_position_sync(payload) {
            Ok((eid, x, y, z, yaw, pitch)) => {
                let teleport = id == packets::CB_PLAY_TELEPORT_ENTITY;
                let _ = events.send(ClientEvent::EntityPos { id: eid, x, y, z, yaw_deg: yaw, pitch_deg: pitch, teleport });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "EntityPositionSync"),
        }
    } else if id == packets::CB_PLAY_CUSTOM_PAYLOAD {
        if let Ok((channel, data)) = packets::read_custom_payload(payload) {
            let _ = events.send(ClientEvent::CustomPayload { channel, data });
        }
    } else if let Some(update) = packets::read_border_update(payload) {
        if let Ok(update) = update {
            let _ = events.send(ClientEvent::Border(update));
        }
    } else if id == packets::CB_PLAY_SET_CAMERA {
        if let Ok(entity) = packets::read_set_camera(payload) {
            let _ = events.send(ClientEvent::SetCamera(entity));
        }
    } else if id == packets::CB_PLAY_PLAYER_ABILITIES {
        if let Ok(abilities) = packets::read_player_abilities(payload) {
            let _ = events.send(ClientEvent::Abilities(abilities));
        }
    } else if id == packets::CB_PLAY_COOLDOWN {
        if let Ok((group, ticks)) = packets::read_cooldown(payload) {
            let _ = events.send(ClientEvent::ItemCooldown { group, ticks });
        }
    } else if id == packets::CB_PLAY_BOSS_EVENT {
        if let Ok((uuid, event)) = packets::read_boss_event(payload) {
            let _ = events.send(ClientEvent::BossEvent { uuid, event });
        }
    } else if id == packets::CB_PLAY_MOVE_VEHICLE {
        if let Ok((pos, yaw_deg, pitch_deg)) = packets::read_move_vehicle(payload) {
            let _ = events.send(ClientEvent::VehicleMove { pos, yaw_deg, pitch_deg });
        }
    } else if id == packets::CB_PLAY_SET_PASSENGERS {
        if let Ok((vehicle, passengers)) = packets::read_set_passengers(payload) {
            let _ = events.send(ClientEvent::Passengers { vehicle, passengers });
        }
    } else if id == packets::CB_PLAY_REMOVE_ENTITIES {
        match packets::read_remove_entities(payload) {
            Ok(ids) => {
                let _ = events.send(ClientEvent::EntitiesRemove(ids));
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "RemoveEntities"),
        }
    } else if id == packets::CB_PLAY_BLOCK_UPDATE {
        if let Ok((x, y, z, state)) = packets::read_block_update(payload) {
            let _ = events.send(ClientEvent::BlockUpdate(vec![(x, y, z, state)]));
        }
    } else if id == packets::CB_PLAY_SECTION_BLOCKS_UPDATE {
        if let Ok(blocks) = packets::read_section_blocks_update(payload) {
            let _ = events.send(ClientEvent::BlockUpdate(blocks));
        }
    } else if id == packets::CB_PLAY_SET_ENTITY_MOTION {
        if let Ok((entity_id, vel)) = packets::read_entity_velocity(payload) {
            let _ = events.send(ClientEvent::EntityVelocity(entity_id, vel));
        }
    } else if id == packets::CB_PLAY_EXPLODE {
        // Ein Lesefehler darf hier NICHT die Verbindung abbrechen -- und der Rueckstoss wird
        // getrennt gelesen, damit ein unlesbarer Partikel dahinter ihn nicht mitreisst.
        if let Ok(Some(kb)) = packets::read_explosion_knockback(payload) {
            let _ = events.send(ClientEvent::ExplosionKnockback(kb));
        }
        if let Ok(explosion) = protocol::particle::read_explosion(payload) {
            let _ = events.send(ClientEvent::ExplosionEffects(explosion));
        }
    } else if id == protocol::particle::CB_PLAY_LEVEL_PARTICLES {
        if let Ok(p) = protocol::particle::read_level_particles(payload) {
            let _ = events.send(ClientEvent::Particles(p));
        }
    } else if id == protocol::particle::CB_PLAY_LEVEL_EVENT {
        if let Ok(e) = protocol::particle::read_level_event(payload) {
            let _ = events.send(ClientEvent::LevelEvent(e));
        }
    } else if id == packets::CB_PLAY_SET_TIME {
        if let Ok((_world_age, time_of_day, ticking)) = packets::read_world_time(payload) {
            let _ = events.send(ClientEvent::WorldTime { time_of_day, ticking });
        }
    } else if id == packets::CB_PLAY_UPDATE_MOB_EFFECT {
        if let Ok((entity_id, effect, amplifier, duration, flags)) = packets::read_mob_effect(payload) {
            let _ = events.send(ClientEvent::MobEffect { entity_id, effect, amplifier, duration, flags });
        }
    } else if id == packets::CB_PLAY_REMOVE_MOB_EFFECT {
        if let Ok((entity_id, effect)) = packets::read_remove_mob_effect(payload) {
            let _ = events.send(ClientEvent::MobEffectRemoved { entity_id, effect });
        }
    } else if id == packets::CB_PLAY_BLOCK_EVENT {
        if let Ok((pos, typ, daten)) = packets::read_block_event(payload) {
            let _ = events.send(ClientEvent::BlockEvent(pos, typ, daten));
        }
    } else if id == packets::CB_PLAY_BLOCK_CHANGED_ACK {
        if let Ok(sequence) = packets::read_block_changed_ack(payload) {
            let _ = events.send(ClientEvent::BlockChangedAck(sequence));
        }
    } else if id == packets::CB_PLAY_SET_HEALTH {
        if let Ok(health) = packets::read_set_health(payload) {
            let _ = events.send(ClientEvent::Health(health.0, health.1));
        }
    } else if id == packets::CB_PLAY_DISCONNECT {
        return Err(NetError::Protocol(disconnect_message("Play", payload)));
    } else if id == packets::CB_PLAY_SYSTEM_CHAT {
        if let Ok((spans, overlay)) = packets::read_system_chat_spans(payload) {
            let _ = events.send(ClientEvent::Chat(spans, overlay));
        }
    } else if id == packets::CB_PLAY_SET_ACTION_BAR_TEXT {
        if let Ok(spans) = packets::read_action_bar_spans(payload) {
            let _ = events.send(ClientEvent::ActionBar(spans));
        }
    } else if id == packets::CB_PLAY_SET_TITLE_TEXT {
        if let Ok(text) = packets::read_title_spans(payload) {
            let _ = events.send(ClientEvent::Title(text));
        }
    } else if id == packets::CB_PLAY_SET_SUBTITLE_TEXT {
        if let Ok(text) = packets::read_subtitle_spans(payload) {
            let _ = events.send(ClientEvent::Subtitle(text));
        }
    } else if id == packets::CB_PLAY_SET_TITLES_ANIMATION {
        if let Ok((fade_in, stay, fade_out)) = packets::read_title_fade(payload) {
            let _ = events.send(ClientEvent::TitleFade { fade_in, stay, fade_out });
        }
    } else if id == packets::CB_PLAY_CLEAR_TITLES {
        // bool reset ignorieren -- wir blenden ohnehin aus.
        let _ = events.send(ClientEvent::ClearTitles);
    } else if id == packets::CB_PLAY_SET_DISPLAY_OBJECTIVE {
        if let Ok((slot, objective)) = packets::read_display_objective(payload) {
            let _ = events.send(ClientEvent::ScoreboardDisplay { slot, objective });
        }
    } else if id == packets::CB_PLAY_SET_OBJECTIVE {
        if let Ok(o) = packets::read_objective(payload) {
            let _ = events.send(ClientEvent::ScoreboardObjective {
                name: o.name,
                mode: o.mode,
                display: o.display,
                hearts: o.hearts,
                number_format: o.number_format,
            });
        }
    } else if id == packets::CB_PLAY_SET_SCORE {
        if let Ok(s) = packets::read_score(payload) {
            let _ = events.send(ClientEvent::ScoreboardScore {
                holder: s.holder,
                objective: s.objective,
                score: s.score,
                display: s.display,
                number_format: s.number_format,
            });
        }
    } else if id == packets::CB_PLAY_SET_PLAYER_TEAM {
        if let Ok(t) = packets::read_team(payload) {
            let _ = events.send(ClientEvent::Team {
                name: t.name,
                mode: t.mode,
                prefix: t.prefix,
                suffix: t.suffix,
                name_tag_visibility: t.name_tag_visibility,
                color: t.color,
                collision_rule: t.collision_rule,
                entries: t.entries,
                display_name: t.display_name,
            });
        }
    } else if id == packets::CB_PLAY_RESET_SCORE {
        if let Ok((holder, objective)) = packets::read_reset_score(payload) {
            let _ = events.send(ClientEvent::ScoreboardResetScore { holder, objective });
        }
    } else if id == packets::CB_PLAY_CONTAINER_SET_CONTENT {
        if let Ok(content) = packets::read_container_content(payload) {
            if let Some((index, err)) = &content.truncated_at {
                // Ein Component-Typ, den item.rs noch nicht byteweise kennt: ab hier ist die
                // Slot-Liste unlesbar. Einmal ins Log, damit genau dieser Typ nachgeruestet
                // werden kann statt still einen halbleeren Container zu zeigen.
                log(events, format!("Inventar: Slot {index} unlesbar ({err:?}) -- Rest der Liste verworfen"));
                if let Some((wo, bytes)) = &content.truncated_detail {
                    log(events, format!("Inventar: {wo}"));
                    dump_unreadable_slot(content.sync_id, *index, wo, bytes);
                }
            }
            let _ = events.send(ClientEvent::ContainerContent(content));
        }
    } else if id == packets::CB_PLAY_CONTAINER_SET_SLOT {
        match packets::read_container_set_slot(payload) {
            Ok(update) => {
                let _ = events.send(ClientEvent::ContainerSlot(update));
            }
            Err(_) => report_unreadable_single_slot(events, "Container-Slot", payload),
        }
    } else if id == packets::CB_PLAY_ROTATE_HEAD {
        if let Ok((eid, head_yaw_deg)) = packets::read_rotate_head(payload) {
            let _ = events.send(ClientEvent::EntityHeadYaw { id: eid, head_yaw_deg });
        }
    } else if id == packets::CB_PLAY_COMMAND_SUGGESTIONS {
        if let Some(suggestions) = packets::read_command_suggestions(payload) {
            let _ = events.send(ClientEvent::CommandSuggestions(suggestions));
        }
    } else if id == packets::CB_PLAY_ENTITY_EVENT {
        if let Ok((entity_id, status)) = packets::read_entity_event(payload) {
            let _ = events.send(ClientEvent::EntityEvent { entity_id, status });
        }
    } else if id == packets::CB_PLAY_SOUND || id == packets::CB_PLAY_SOUND_ENTITY {
        // Beide Ton-Pakete an EINER Stelle: sie unterscheiden sich nur darin, ob der Ort als
        // Koordinate oder als Entity-ID kommt (s. packets::SoundEffect). Wo die Entity steht,
        // weiss der Netz-Task nicht -- das loest die App auf, die den Weltzustand haelt.
        //
        // Ok(None) heisst "gelesen, aber unbekannter Klang" (z.B. neuere Serverversion) und
        // ist bewusst still: das Paket war vollstaendig, es gibt nur nichts abzuspielen.
        let gelesen = if id == packets::CB_PLAY_SOUND { packets::read_sound(payload) } else { packets::read_sound_entity(payload) };
        if sound_debug() {
            match &gelesen {
                Ok(Some(s)) => eprintln!(
                    "[ton] 0x{id:02x} {} kat={:?} pos={:?} ent={:?} vol={} pitch={}",
                    s.name, s.category, s.position, s.entity_id, s.volume, s.pitch
                ),
                Ok(None) => eprintln!("[ton] 0x{id:02x} gelesen, aber Klang oder Kategorie unbekannt"),
                Err(e) => eprintln!("[ton] 0x{id:02x} NICHT LESBAR: {e:?} ({} Byte)", payload.len()),
            }
        }
        if let Ok(Some(sound)) = gelesen {
            let _ = events.send(ClientEvent::Sound(sound));
        }
    } else if id == packets::CB_PLAY_STOP_SOUND {
        if let Ok((category, name)) = packets::read_stop_sound(payload) {
            let _ = events.send(ClientEvent::StopSound { category, name });
        }
    } else if id == packets::CB_PLAY_ANIMATE {
        if let Ok((entity_id, animation)) = packets::read_entity_animation(payload) {
            let _ = events.send(ClientEvent::EntityAnimation { entity_id, animation });
        }
    } else if id == packets::CB_PLAY_DAMAGE_EVENT {
        if let Ok((entity_id, damage_type)) = packets::read_damage_event(payload) {
            let _ = events.send(ClientEvent::EntityDamaged { entity_id, damage_type });
        }
    } else if id == packets::CB_PLAY_SET_EQUIPMENT {
        // Ein unbekannter Component-Typ im Stapel macht den REST des Pakets unlesbar (die
        // Slot-Kette laeuft byteweise weiter) -- dann bleibt die Hand eben leer, statt die
        // Verbindung abzubrechen. Genau wie beim Inventar (s. CB_PLAY_CONTAINER_SET_CONTENT).
        if let Ok((entity_id, entries)) = packets::read_set_equipment(payload) {
            let _ = events.send(ClientEvent::EntityEquipment { id: entity_id, entries });
        }
    } else if id == packets::CB_PLAY_SET_EXPERIENCE {
        if let Ok((progress, level, total)) = packets::read_experience(payload) {
            let _ = events.send(ClientEvent::Experience { progress, level, total });
        }
    } else if id == packets::CB_PLAY_UPDATE_ATTRIBUTES {
        if let Ok(update) = packets::read_attributes(payload) {
            if update.armor.is_some() || update.max_health.is_some() {
                let _ = events.send(ClientEvent::Attributes { id: update.entity_id, armor: update.armor, max_health: update.max_health });
            }
            if update.movement_speed.is_some() || update.sneaking_speed.is_some() || update.water_movement_efficiency.is_some() {
                let _ = events.send(ClientEvent::MovementAttributes {
                    id: update.entity_id,
                    movement_speed: update.movement_speed,
                    sneaking_speed: update.sneaking_speed,
                    water_movement_efficiency: update.water_movement_efficiency,
                });
            }
            if update.block_interaction_range.is_some() || update.entity_interaction_range.is_some() {
                let _ = events.send(ClientEvent::RangeAttributes {
                    id: update.entity_id,
                    block: update.block_interaction_range,
                    entity: update.entity_interaction_range,
                });
            }
            if update.block_break_speed.is_some() || update.mining_efficiency.is_some() || update.submerged_mining_speed.is_some() {
                let _ = events.send(ClientEvent::MiningAttributes {
                    id: update.entity_id,
                    block_break_speed: update.block_break_speed,
                    mining_efficiency: update.mining_efficiency,
                    submerged_mining_speed: update.submerged_mining_speed,
                });
            }
        }
    } else if id == packets::CB_PLAY_SET_ENTITY_DATA {
        // Ausgewertet werden Pose (Hoehe der Trefferbox), Schlafposition, Restluft und --
        // gebuendelt als EntityMeta -- Sichtbarkeit, Namensschild und Display-Inhalt. Was
        // nicht im Paket steht, hat sich nicht geaendert: der Server schickt nur geaenderte
        // Felder.
        if let Ok(update) = packets::read_set_entity_data(payload) {
            if let Some(pose) = update.pose {
                let _ = events.send(ClientEvent::EntityPose { id: update.entity_id, pose });
            }
            if let Some(pos) = update.sleeping_pos {
                let _ = events.send(ClientEvent::EntitySleeping { id: update.entity_id, pos });
            }
            if let Some(air) = update.air {
                let _ = events.send(ClientEvent::EntityAir { id: update.entity_id, air });
            }
            // Nur senden, wenn ueberhaupt etwas Einschlaegiges drinstand -- Positions-/
            // Gesundheits-Updates schicken ihren Metadaten-Strom sonst pro Tick durch die
            // Warteschlange, ohne dass sich daran je etwas aendert.
            let betrifft_anzeige = meta_betrifft_anzeige(&update);
            if betrifft_anzeige {
                let _ = events.send(ClientEvent::EntityMeta(Box::new(update)));
            }
        }
    } else if id == packets::CB_PLAY_PLAYER_INFO_UPDATE {
        // Traegt (bei ADD_PLAYER) Name und Textur-Property jedes Spielers -- die Grundlage
        // fuer echte Skins. Nicht lesbar heisst hier: still verwerfen, das Rendern faellt
        // dann auf den Standard-Skin zurueck.
        if let Some(entries) = packets::parse_player_info_update(payload) {
            let _ = events.send(ClientEvent::PlayerInfo(entries));
        }
    } else if id == packets::CB_PLAY_PLAYER_INFO_REMOVE {
        if let Some(uuids) = packets::parse_player_info_remove(payload) {
            let _ = events.send(ClientEvent::PlayerInfoRemove(uuids));
        }
    } else if id == packets::CB_PLAY_TAB_LIST {
        if let Ok((header, footer)) = packets::read_tab_list(payload) {
            let _ = events.send(ClientEvent::TabList { header, footer });
        }
    } else if id == packets::CB_PLAY_SET_CURSOR_ITEM {
        if let Ok(stack) = packets::read_cursor_item(payload) {
            let _ = events.send(ClientEvent::CursorItem(stack));
        }
    } else if id == packets::CB_PLAY_SET_PLAYER_INVENTORY {
        match packets::read_player_inventory_slot(payload) {
            Ok((slot, stack)) => {
                let _ = events.send(ClientEvent::PlayerInventorySlot(slot, stack));
            }
            Err(_) => report_unreadable_single_slot(events, "Spieler-Slot", payload),
        }
    } else if id == packets::CB_PLAY_SET_CARRIED_ITEM {
        if let Ok(slot) = packets::read_set_carried_item(payload) {
            let _ = events.send(ClientEvent::HeldSlot(slot));
        }
    } else if id == protocol::sign::CB_PLAY_BLOCK_ENTITY_DATA {
        if let Ok(Some((pos, text))) = protocol::sign::read_block_entity_data(payload) {
            let _ = events.send(ClientEvent::SignText { pos, text });
        }
        if let Ok(Some((pos, profile))) = protocol::sign::read_block_entity_head(payload) {
            let _ = events.send(ClientEvent::HeadAt { pos, profile });
        }
    } else if id == protocol::sign::CB_PLAY_OPEN_SIGN_EDITOR {
        if let Ok((pos, front)) = protocol::sign::read_open_sign_editor(payload) {
            let _ = events.send(ClientEvent::OpenSignEditor { pos, front });
        }
    } else if id == packets::CB_PLAY_MAP_ITEM_DATA {
        if let Ok(daten) = packets::read_map_item_data(payload) {
            let _ = events.send(ClientEvent::MapData(daten));
        }
    } else if id == packets::CB_PLAY_OPEN_BOOK {
        if let Ok(hand) = packets::read_open_book(payload) {
            let _ = events.send(ClientEvent::OpenBook { hand });
        }
    } else if id == protocol::recipe::CB_PLAY_RECIPE_BOOK_ADD {
        match protocol::recipe::read_recipe_book_add(payload) {
            Ok(add) => {
                let _ = events.send(ClientEvent::RecipeBookAdd(add));
            }
            Err(_) => log(events, "Rezeptbuch-Paket nicht lesbar -- ignoriert"),
        }
    } else if id == protocol::recipe::CB_PLAY_RECIPE_BOOK_REMOVE {
        if let Ok(ids) = protocol::recipe::read_recipe_book_remove(payload) {
            let _ = events.send(ClientEvent::RecipeBookRemove(ids));
        }
    } else if id == protocol::recipe::CB_PLAY_UPDATE_TAGS {
        if let Ok(tags) = protocol::recipe::read_item_tags(payload) {
            let _ = events.send(ClientEvent::ItemTags(tags));
        }
    } else if id == protocol::recipe::CB_PLAY_RECIPE_BOOK_SETTINGS || id == protocol::recipe::CB_PLAY_PLACE_GHOST_RECIPE {
        // Buch-Einstellungen des Servers und Geister-Rezepte: das Buch hier hat eigene.
    } else if id == packets::CB_PLAY_OPEN_SCREEN {
        if let Ok(screen) = packets::read_open_screen(payload) {
            let _ = events.send(ClientEvent::OpenScreen(screen));
        }
    } else if id == packets::CB_PLAY_CONTAINER_SET_DATA {
        if let Ok((sync_id, index, value)) = packets::read_container_set_data(payload) {
            let _ = events.send(ClientEvent::ContainerData(sync_id, index, value));
        }
    } else if id == packets::CB_PLAY_MERCHANT_OFFERS {
        if let Ok(offers) = packets::read_merchant_offers(payload) {
            let _ = events.send(ClientEvent::MerchantOffers(offers));
        }
    } else if id == packets::CB_PLAY_CONTAINER_CLOSE {
        if let Ok(sync_id) = packets::read_container_close(payload) {
            let _ = events.send(ClientEvent::CloseScreen(sync_id));
        }
    } else if stats.note_unknown_id(id) && std::env::var_os("CORAL_PACKET_SCAN").is_some() {
        // Diagnose: Play-Packet-IDs sind versionsabhaengig und die Wiki dokumentiert
        // 776, nicht 774. Statt zu raten, jede unbekannte ID einmal daraufhin testen,
        // ob sich dahinter eine plausible Spielerposition verbirgt (VarInt + 3 Doubles).
        if let Ok(packets::PlayerPositionSync { pos: (x, y, z), .. }) = packets::read_player_position(payload) {
            let plausible =
                (-30_000_000.0..30_000_000.0).contains(&x) && (-1000.0..1000.0).contains(&y) && (-30_000_000.0..30_000_000.0).contains(&z);
            if plausible {
                log(events, format!("[scan] id=0x{id:02x} ({} Byte) -> {x:.1} / {y:.1} / {z:.1}", payload.len()));
            }
        }
    }
    Ok(PlayFlow::Continue)
}

// Vollstaendiger Join: Login -> Configuration -> Play, dann Verbindung offen halten.
// Laeuft bis zum Disconnect -- vom Aufrufer als Hintergrund-Task zu spawnen, nicht
// blockierend (sonst friert die Render-Loop ein). Status/Log geht per Channel an die App.
// `credentials` = echtes Microsoft-/Minecraft-Konto (fuer Online-Mode-Server). None = wie
// bisher Offline-Mode mit `offline_username`.
pub async fn join(
    address: &str,
    offline_username: &str,
    credentials: Option<auth::Credentials>,
    events: Events,
    mut outgoing: Outgoing,
    rotation: SharedRotation,
) -> Result<(), NetError> {
    let result = join_inner(address, offline_username, credentials.as_ref(), &events, &mut outgoing, &rotation).await;
    if let Err(e) = &result {
        let _ = events.send(ClientEvent::Disconnected(e.to_string()));
    }
    result
}

async fn join_inner(
    address: &str,
    offline_username: &str,
    credentials: Option<&auth::Credentials>,
    events: &Events,
    outgoing: &mut Outgoing,
    rotation: &SharedRotation,
) -> Result<(), NetError> {
    let (typed_host, typed_port) = parse_address(address);
    let (mut host, mut port) = resolve_address(address).await;
    // Der Name, den man eingegeben hat (nach einem Transfer das Ziel): weicht der verbundene
    // Host davon ab, kam er aus einem SRV-Eintrag.
    let mut eingegeben = typed_host.clone();
    // Die Weiterleitung im Log sichtbar machen: sonst wundert man sich bei einem Fehler
    // ueber einen Port/Host, den man nie eingegeben hat.
    if (host.as_str(), port) != (typed_host.as_str(), typed_port) {
        log(events, format!("SRV-Eintrag fuer {typed_host}: -> {host}:{port}"));
    }
    let mut intent = packets::INTENT_LOGIN;
    // Die Cookies wandern bei einem Transfer mit (Vanillas TransferState) -- genau darueber
    // erkennt der Zielserver, dass wir vom Netzwerk geschickt wurden.
    let mut cookies = std::collections::HashMap::new();

    // Jeder Durchlauf ist eine Verbindung. Ein Transfer beendet sie mit Ok(Ziel), dann geht
    // es mit einer neuen weiter; alles andere beendet die Sitzung.
    loop {
        // Auch hinter SRV-Eintrag und Transfer pruefen: sonst koennte ein lokaler Server
        // einen Offline-Client einfach an einen fremden weiterreichen.
        if credentials.is_none() && !is_local_address(&host) {
            return Err(NetError::Protocol(format!("{host} ist kein Server auf diesem Rechner -- ohne Anmeldung nicht erlaubt")));
        }
        log(events, format!("Verbinde zu {host}:{port} ..."));
        let mut conn = Connection::connect(&host, port).await?;
        conn.cookies = std::mem::take(&mut cookies);
        if let Ok(gegenueber) = conn.stream.peer_addr() {
            let ziel = if host == eingegeben { gegenueber.to_string() } else { format!("{host}/{gegenueber}") };
            let _ = events.send(ClientEvent::Route(ziel));
        }

        // Der Mitschnitt wird NUR bei einem Abbruch mit Fehler geschrieben. Ein normales
        // Trennen durch den Nutzer endet mit Ok und soll kein /tmp vollschreiben. Der Pfad
        // geht ins Client-Log, sonst muesste man raten, wo die Datei gelandet ist.
        let result = join_session(&mut conn, &host, port, intent, offline_username, credentials, events, outgoing, rotation).await;
        let (ziel_host, ziel_port) = match result {
            Ok(ziel) => ziel,
            Err(e) => {
                match conn.log.dump(&e.to_string()) {
                    Some(path) => log(events, format!("Paket-Mitschnitt der letzten Pakete: {path}")),
                    None => log(events, "Kein Paket-Mitschnitt (nichts aufgezeichnet)"),
                }
                return Err(e);
            }
        };
        log(events, format!("Server schickt uns weiter: {ziel_host}:{ziel_port}"));
        cookies = std::mem::take(&mut conn.cookies);
        // Erst die alte Verbindung schliessen, dann melden: der Zielserver erwartet uns nicht
        // doppelt angemeldet.
        drop(conn);
        let _ = events.send(ClientEvent::Transferring { host: ziel_host.clone(), port: ziel_port });
        // Wie Vanilla (ConnectScreen ueber ServerAddress): auch das Transfer-Ziel geht durch
        // die SRV-Aufloesung, und der Handshake traegt die aufgeloeste Adresse.
        eingegeben = ziel_host.clone();
        (host, port) = resolve_srv(&ziel_host, ziel_port).await.unwrap_or((ziel_host, ziel_port));
        intent = packets::INTENT_TRANSFER;
    }
}

// Der eigentliche Sitzungsablauf ab bestehender TCP-Verbindung. Getrennt von join_inner nur,
// damit dort ein `?` nicht am Mitschnitt-Dump vorbeispringt -- jeder Fehler von hier muss
// durch die Fehlerbehandlung des Aufrufers. Ok heisst: der Server hat uns weitergeschickt.
#[allow(clippy::too_many_arguments)]
async fn join_session(
    conn: &mut Connection,
    host: &str,
    port: u16,
    intent: i32,
    offline_username: &str,
    credentials: Option<&auth::Credentials>,
    events: &Events,
    outgoing: &mut Outgoing,
    rotation: &SharedRotation,
) -> Result<TransferTarget, NetError> {
    let login = do_login(conn, host, port, intent, offline_username, credentials).await?;
    log(events, format!("Login erfolgreich: {}", login.username));
    let _ = events.send(ClientEvent::OwnUuid(login.uuid.to_bytes()));

    conn.send(&packets::login_acknowledged_payload()).await?;
    log(events, "Konfiguration ...");
    if let Some(ziel) = do_configuration(conn, events, outgoing).await? {
        return Ok(ziel);
    }
    // Bewegungspakete kommen erst im Play-State zum Zug -- waehrend Login/Configuration
    // haelt run_play_loop den outgoing-Kanal noch nicht offen, die App schickt aber ohnehin
    // erst etwas, sobald sie den Play-Zustand ueber ClientEvent::Connected erfaehrt.
    run_play_loop(conn, events, outgoing, rotation).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Ein Paket, das NUR das Benutzen-Flag traegt, muss durchkommen -- genau das schickt der
    // Server, wenn jemand sein Schild hebt.
    fn ein_reines_benutzen_flag_gilt_als_anzeigerelevant() {
        let nur_flag = packets::EntityDataUpdate { living_flags: Some(1), ..Default::default() };
        assert!(meta_betrifft_anzeige(&nur_flag));
        // Gegenprobe: ein Strom ohne jedes Anzeigefeld wird weiterhin verworfen.
        let nur_luft = packets::EntityDataUpdate { air: Some(300), ..Default::default() };
        assert!(!meta_betrifft_anzeige(&nur_luft));
    }

    // Liegende Items tragen ihren Stack allein im Paket -- ohne ihn bleiben sie unsichtbar.
    // Dasselbe fuer den Schuetzen einer Feuerwerksrakete (Elytra-Schub).
    #[test]
    fn item_stapel_und_raketenschuetze_kommen_durch() {
        let stapel = packets::EntityDataUpdate { item_stack: Some(None), ..Default::default() };
        assert!(meta_betrifft_anzeige(&stapel));
        let schuetze = packets::EntityDataUpdate { firework_shooter: Some(Some(7)), ..Default::default() };
        assert!(meta_betrifft_anzeige(&schuetze));
    }

    #[test]
    fn unbekannte_ids_werden_einmal_und_gedeckelt_gemerkt() {
        let mut s = PlayStats::default();
        assert!(s.note_unknown_id(0x77), "erste Sichtung ist neu");
        assert!(!s.note_unknown_id(0x77), "zweite Sichtung derselben ID nicht noch einmal");
        for id in 0..100 {
            s.note_unknown_id(id);
        }
        assert_eq!(s.unknown_ids.len(), PlayStats::MAX_UNKNOWN_IDS, "Liste darf nicht unbegrenzt wachsen");
    }

    #[test]
    fn stall_bericht_nennt_die_entscheidenden_zahlen() {
        // Der Bericht ist die einzige Spur, die ein Haenger hinterlaesst -- er muss ohne
        // Nachfragen unterscheidbar machen, ob der Server schweigt, leere Stapel schickt
        // oder wir die Chunk-Pakete nicht erkennen.
        let mut s = PlayStats { packets: 412, batches: 1, last_batch_size: 2, acks_sent: 3, ..Default::default() };
        s.note_unknown_id(0x2C);
        let bericht = s.chunk_stall_report(std::time::Duration::from_secs(6));
        assert!(bericht.contains("412 Pakete"), "{bericht}");
        assert!(bericht.contains("1 Stapel (zuletzt 2 Chunks)"), "{bericht}");
        assert!(bericht.contains("3 Bestaetigungen"), "{bericht}");
        assert!(bericht.contains("0x2C"), "{bericht}");
    }

    #[test]
    fn stall_bericht_ohne_unbekannte_ids_sagt_das_ausdruecklich() {
        let bericht = PlayStats::default().chunk_stall_report(std::time::Duration::from_secs(6));
        assert!(bericht.contains("unbekannte IDs: keine"), "{bericht}");
    }

    #[test]
    fn parse_address_ergaenzt_den_standardport() {
        assert_eq!(parse_address("derantonia.de"), ("derantonia.de".to_string(), 25565));
        assert_eq!(parse_address("derantonia.de:25566"), ("derantonia.de".to_string(), 25566));
        // Kein gueltiger Port hinter dem Doppelpunkt -> alles ist der Host (so wie bisher).
        assert_eq!(parse_address("beispiel:abc"), ("beispiel:abc".to_string(), 25565));
    }

    #[test]
    fn ohne_anmeldung_nur_server_auf_diesem_rechner() {
        for lokal in ["localhost", "LocalHost:25566", "127.0.0.1", "127.0.0.1:25565", "127.1.2.3", "[::1]:25565"] {
            assert!(is_local_address(lokal), "{lokal}");
        }
        for fremd in ["derantonia.de", "192.168.0.10", "10.0.0.1:25565", "0.0.0.0", "localhost.example.com", "hypixel.net"] {
            assert!(!is_local_address(fremd), "{fremd}");
        }
    }

    // Baut einen komprimierten Frame so, wie ein Server ihn schickt: VarInt mit der
    // ENTPACKTEN Laenge, dann die Zlib-Daten. `behauptete_laenge` darf bewusst von der
    // echten abweichen -- genau das ist der Angriffsfall.
    fn komprimierter_frame(nutzdaten: &[u8], behauptete_laenge: i32) -> Vec<u8> {
        use std::io::Write;
        let mut frame = encode_varint(behauptete_laenge);
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(nutzdaten).unwrap();
        frame.extend(enc.finish().unwrap());
        frame
    }

    // Der Normalfall muss unveraendert durchlaufen -- sonst haette der Deckel echte Server
    // mitgesperrt statt nur die unsinnigen Laengen.
    #[test]
    fn ehrlicher_komprimierter_frame_kommt_unveraendert_an() {
        let nutzdaten = vec![7u8; 5000];
        let frame = komprimierter_frame(&nutzdaten, nutzdaten.len() as i32);
        assert_eq!(decompress_frame(Some(256), frame).unwrap(), nutzdaten);
    }

    // Eine negative Laenge ist der schlimmste Fall: `-1 as usize` waere usize::MAX und damit
    // eine sofortige Allokation ueber den gesamten Adressraum -- der Prozess waere weg.
    // Muss ein normaler Protokollfehler sein, den der Aufrufer als Verbindungsabbruch sieht.
    #[test]
    fn negative_entpackte_laenge_wird_abgelehnt() {
        let frame = komprimierter_frame(b"egal", -1);
        assert!(matches!(decompress_frame(Some(256), frame), Err(NetError::Protocol(_))));
    }

    // Dekompressions-Bombe: der Server behauptet eine riesige entpackte Groesse. Ohne Deckel
    // wuerde vec![0u8; ...] das sofort anfordern, BEVOR ueberhaupt entpackt wird.
    #[test]
    fn viel_zu_grosse_entpackte_laenge_wird_abgelehnt() {
        let frame = komprimierter_frame(b"egal", MAX_UNCOMPRESSED_LEN + 1);
        assert!(matches!(decompress_frame(Some(256), frame), Err(NetError::Protocol(_))));
        // Genau auf der Grenze darf es NICHT am Deckel scheitern (sondern hoechstens daran,
        // dass die Zlib-Daten kuerzer sind) -- sonst waere der Deckel um eins zu streng.
        let grenzfall = komprimierter_frame(b"egal", MAX_UNCOMPRESSED_LEN);
        assert!(!matches!(decompress_frame(Some(256), grenzfall), Err(NetError::Protocol(_))));
    }

    // Gelogene Laenge nach unten: entpackt kommen mehr Bytes heraus als angekuendigt. Ohne
    // die Nachpruefung haetten wir stillschweigend ein abgeschnittenes Paket weitergereicht.
    #[test]
    fn zu_klein_angegebene_laenge_wird_erkannt() {
        let frame = komprimierter_frame(&[9u8; 100], 10);
        assert!(matches!(decompress_frame(Some(256), frame), Err(NetError::Protocol(_))));
    }

    // Ohne ausgehandelte Kompression bleibt der Frame unangetastet -- der Deckel darf sich
    // in diesen Pfad nicht einmischen.
    #[test]
    fn ohne_kompression_bleibt_der_frame_roh() {
        let roh = vec![1u8, 2, 3];
        assert_eq!(decompress_frame(None, roh.clone()).unwrap(), roh);
        // Nur die Null fuer "unkomprimiert": leer (recv ueberspringt es, wie Vanilla).
        assert!(decompress_frame(Some(256), vec![0]).unwrap().is_empty());
    }

    // Frame-Laengen ueber Vanillas Varint21-Grenze muessen sofort auffallen, statt rbuf
    // unbegrenzt weiterwachsen zu lassen (s. MAX_FRAME_LEN).
    #[test]
    fn zu_lange_frame_laenge_wird_abgelehnt() {
        let zu_lang = encode_varint(MAX_FRAME_LEN as i32 + 1);
        assert!(matches!(peek_frame_len(&zu_lang), Err(NetError::Protocol(_))));
        // Genau auf der Grenze noch erlaubt.
        let grenzfall = encode_varint(MAX_FRAME_LEN as i32);
        assert_eq!(peek_frame_len(&grenzfall).unwrap().map(|(len, _)| len), Some(MAX_FRAME_LEN));
    }

    // Der SRV-Umweg gilt NUR fuer den Standardport (wie Vanillas RedirectResolver): wer
    // "host:25566" eintippt, meint genau diesen Port und darf nicht umgeleitet werden.
    // Ohne Netz pruefbar, weil die Abfrage vor jedem DNS-Zugriff abbricht.
    #[tokio::test]
    async fn srv_wird_bei_explizitem_port_uebersprungen() {
        assert_eq!(resolve_srv("derantonia.de", 25566).await, None);
        assert_eq!(resolve_address("derantonia.de:25566").await, ("derantonia.de".to_string(), 25566));
    }
}

#[cfg(test)]
mod meta_filter_tests {
    // Ein Metadaten-Paket, das NUR die Kristall-Bodenplatte aendert, muss durchkommen --
    // fehlte das Feld im Filter, stand die Platte auch bei gesetzten Kristallen da.
    #[test]
    fn kristall_bodenplatte_wird_weitergereicht() {
        let update = protocol::packets::EntityDataUpdate { end_crystal_show_bottom: Some(false), ..Default::default() };
        assert!(super::meta_betrifft_anzeige(&update));
        let rot = protocol::packets::EntityDataUpdate { display_left_rotation: Some([0.0, 0.0, 0.0, 1.0]), ..Default::default() };
        assert!(super::meta_betrifft_anzeige(&rot));
    }
}

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
