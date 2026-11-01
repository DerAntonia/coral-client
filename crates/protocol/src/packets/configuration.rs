// Konfigurationsphase: Client-Infos, Marke, Registry-Daten und Dimensionen.

use super::*;

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
pub(crate) static LOCALE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

pub fn set_locale(code: &str) {
    if let Ok(mut l) = LOCALE.write() {
        *l = code.to_string();
    }
}

pub(crate) fn locale() -> String {
    LOCALE.read().ok().filter(|l| !l.is_empty()).map_or_else(|| "en_us".to_string(), |l| l.clone())
}

// Serverbound, id 0x00. Pflicht-Packet, das der Server im Configuration-State erwartet.
#[must_use] 
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
#[must_use] 
pub fn brand_payload(brand: &str) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x02).write_string("minecraft:brand").write_string(brand);
    w.into_bytes()
}

// Was wir als Brand melden. Der Wert wird im `minecraft:brand`-Payload an den Server gesendet.
pub const CLIENT_BRAND: &str = "coral";

// Serverbound, id 0x07. Antwort auf Clientbound Known Packs -- leeres Array bedeutet
// "kenne keine Packs", woraufhin der Server die vollen Registry-Daten schickt (die wir
// fuer den reinen Join-Nachweis ignorieren).
#[must_use] 
pub fn serverbound_known_packs_empty_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x07).write_varint(0); // Array-Laenge 0
    w.into_bytes()
}

// Serverbound, id 0x03. Bestaetigt Finish Configuration -> wechselt in den Play-State.
#[must_use] 
pub fn acknowledge_finish_configuration_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x03);
    w.into_bytes()
}

// Serverbound, id 0x04 (Configuration). Antwort auf Keep Alive.
#[must_use] 
pub fn keep_alive_config_payload(id: i64) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x04).write_i64(id);
    w.into_bytes()
}

// Serverbound, id 0x05 (Configuration). Antwort auf Ping.
#[must_use] 
pub fn pong_config_payload(id: i32) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x05).write_i32(id);
    w.into_bytes()
}

// Fuer die Gras-/Laub-/Wasser-Faerbung (Colormaps) relevante Felder eines Bioms. Die
// Position in der Liste, die read_registry_data() zurueckgibt, IST die numerische Biom-ID
// (Registrierungsreihenfolge = Registry-Index, wie bei Block-States) -- exakt der Wert, den
// die Biom-Palette im Chunk-Paket pro 4x4x4-Zelle traegt.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DimensionInfo {
    pub name: String,
    pub min_y: i32,
    pub height: i32,
}

// Die bekannten Vanilla-Werte, falls der Server einen Eintrag ohne Daten schickt (dann gilt
// das, was der Client von Haus aus kennt).
pub(crate) fn vanilla_dimension_defaults(name: &str) -> (i32, i32) {
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
        let min_y = data.as_ref().and_then(|d| d.get("min_y")).and_then(super::super::nbt::NbtValue::as_i32).unwrap_or(vorgabe_min);
        let height = data.as_ref().and_then(|d| d.get("height")).and_then(super::super::nbt::NbtValue::as_i32).unwrap_or(vorgabe_hoehe);
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
        let temperature = data.as_ref().and_then(|d| d.get("temperature")).and_then(super::super::nbt::NbtValue::as_f32).unwrap_or(0.8);
        let downfall = data.as_ref().and_then(|d| d.get("downfall")).and_then(super::super::nbt::NbtValue::as_f32).unwrap_or(0.4);
        let water_color = data
            .as_ref()
            .and_then(|d| d.get("effects"))
            .and_then(|e| e.get("water_color"))
            .and_then(super::super::nbt::NbtValue::as_i32)
            .map_or(0x3F76E4, |v| v as u32);

        // Pflichtfeld in Vanillas Codec -- fehlt es trotzdem, lieber "regnet" als nie.
        let has_precipitation = data.as_ref().and_then(|d| d.get("has_precipitation")).and_then(super::super::nbt::NbtValue::as_i32).is_none_or(|v| v != 0);

        biomes.push(BiomeInfo { name, temperature, downfall, water_color, has_precipitation });
    }
    Ok(Some(biomes))
}
