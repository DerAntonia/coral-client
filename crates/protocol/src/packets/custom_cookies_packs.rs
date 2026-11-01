// Plugin-Kanaele, Abklingzeiten, Transfer, Cookies und Ressourcenpakete.

use super::*;

// (Kanal, Daten) einer Plugin-Nachricht -- im Play- wie im Configuration-State dasselbe Layout.
pub fn read_custom_payload(payload: &[u8]) -> Result<(String, Vec<u8>), ReadError> {
    let mut r = PacketReader::new(payload);
    r.read_varint()?;
    let channel = r.read_string()?;
    Ok((channel, r.remaining().to_vec()))
}

#[must_use] 
pub fn custom_payload(channel: &str, data: &[u8]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CUSTOM_PAYLOAD).write_string(channel).write_raw_bytes(data);
    w.into_bytes()
}

// "minecraft:register": die Kanaele, auf denen wir Plugin-Nachrichten annehmen, durch NUL
// getrennt. Bukkit/Paper schicken einem Client NUR auf angemeldeten Kanaelen etwas
// (Player.sendPluginMessage prueft getListeningPluginChannels) -- ohne diese Meldung kaeme
// z.B. ein Opt-out eines Paper-Plugins nie an. Fabric API meldet seine Kanaele genauso an.
#[must_use] 
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
#[must_use] 
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
pub(crate) mod cookie_tests {
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

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
#[must_use] 
pub fn resource_pack_status_payload(id: [u8; 16], status: ResourcePackStatus) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_RESOURCE_PACK).write_raw_bytes(&id).write_varint(status as i32);
    w.into_bytes()
}

// Eine Status-Antwort in Play-Form fuer den Configuration-State umschreiben; None, wenn das
// Paket keine Status-Antwort ist.
#[must_use] 
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
pub(crate) mod resource_pack_tests {
    use super::*;

    #[test]
    fn push_und_status() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_RESOURCE_PACK_PUSH).write_raw_bytes(&[7; 16]).write_string("https://x/p.zip").write_string("ab").write_bool(true).write_bool(false);
        let p = read_resource_pack_push(&w.into_bytes()).unwrap();
        assert_eq!((p.id, p.url.as_str(), p.hash.as_str(), p.required, p.prompt), ([7; 16], "https://x/p.zip", "ab", true, None));

        let play = resource_pack_status_payload([7; 16], ResourcePackStatus::Downloaded);
        let config = config_resource_pack_status(&play).unwrap();
        assert_eq!(i32::from(config[0]), SB_CONFIG_RESOURCE_PACK);
        assert_eq!(&config[1..], &play[1..]);
        assert!(config_resource_pack_status(&[0x05, 1, 2]).is_none());

        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_RESOURCE_PACK_POP).write_bool(false);
        assert_eq!(read_resource_pack_pop(&w.into_bytes()).unwrap(), None);
    }
}

// --- Dialoge, Server-Links, Verhaltenskodex und der Rest des Configuration-State -----------
//
// IDs aus der Registrierungsreihenfolge von ConfigurationProtocols bzw. GameProtocols
// (1.21.11). Ein Teil davon kommt in beiden States vor (Server-Links, Dialoge,
// Report-Details), mit eigener ID je State.

pub const CB_CONFIG_RESET_CHAT: i32 = 0x06;
pub const CB_CONFIG_UPDATE_ENABLED_FEATURES: i32 = 0x0C;
pub const CB_CONFIG_CUSTOM_REPORT_DETAILS: i32 = 0x0F;
pub const CB_CONFIG_SERVER_LINKS: i32 = 0x10;
pub const CB_CONFIG_CLEAR_DIALOG: i32 = 0x11;
pub const CB_CONFIG_SHOW_DIALOG: i32 = 0x12;
pub const CB_CONFIG_CODE_OF_CONDUCT: i32 = 0x13;
pub const SB_CONFIG_CUSTOM_CLICK_ACTION: i32 = 0x08;
pub const SB_CONFIG_ACCEPT_CODE_OF_CONDUCT: i32 = 0x09;

pub const CB_PLAY_CUSTOM_REPORT_DETAILS: i32 = 0x86;
pub const CB_PLAY_SERVER_LINKS: i32 = 0x87;
pub const CB_PLAY_CLEAR_DIALOG: i32 = 0x89;
pub const CB_PLAY_SHOW_DIALOG: i32 = 0x8A;
pub const SB_PLAY_CUSTOM_CLICK_ACTION: i32 = 0x41;
