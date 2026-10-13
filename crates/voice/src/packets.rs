// Die Nachrichten des Sprachchats, kompatibel zu Simple Voice Chat (Protokoll-Kompatibilitaet
// 20, Mod 2.6.x). Zwei Wege:
//
// 1. Plugin-Kanaele ueber die Minecraft-Verbindung (voicechat:<name>): Secret anfordern und
//    erhalten, eigener Zustand, Zustaende der anderen Spieler.
// 2. UDP direkt zum Sprachserver: ein Byte 0xFF, beim Client davor noch die Spieler-UUID, dann
//    ein laengenpraefixierter, mit AES-128-GCM verschluesselter Block (s. crypto). Im Block
//    steht ein Typ-Byte und das Paket.
//
// Feldformate wie in Minecrafts PacketByteBuf: Byte-Arrays und Strings mit VarInt-Laenge,
// UUIDs als zwei Longs, alles Big Endian.

use protocol::buffer::{PacketReader, PacketWriter, ReadError};
use protocol::uuid::Uuid;

pub const COMPATIBILITY_VERSION: i32 = 20;

// Client -> Server.
pub const CH_REQUEST_SECRET: &str = "voicechat:request_secret";
pub const CH_UPDATE_STATE: &str = "voicechat:update_state";

// Server -> Client. Paper liefert Plugin-Nachrichten nur auf Kanaelen, die der Client per
// minecraft:register angemeldet hat -- deshalb die Liste.
pub const CH_SECRET: &str = "voicechat:secret";
pub const CH_STATE: &str = "voicechat:state";
pub const CH_STATES: &str = "voicechat:states";
pub const CH_REMOVE_STATE: &str = "voicechat:remove_state";
pub const CLIENTBOUND_CHANNELS: [&str; 9] = [
    CH_SECRET,
    CH_STATE,
    CH_STATES,
    CH_REMOVE_STATE,
    "voicechat:add_group",
    "voicechat:remove_group",
    "voicechat:joined_group",
    "voicechat:add_category",
    "voicechat:remove_category",
];

// Opus-Abtastrate und Rahmenlaenge (20 ms) -- fest im Protokoll.
pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SIZE: usize = 960;
pub const MAX_OPUS_PAYLOAD: usize = 1275;

pub fn request_secret_payload() -> Vec<u8> {
    COMPATIBILITY_VERSION.to_be_bytes().to_vec()
}

// Eigener Zustand: `disabled` = Sprachchat stumm geschaltet (nichts hoeren, nichts senden).
pub fn update_state_payload(disabled: bool) -> Vec<u8> {
    vec![disabled as u8]
}

// Antwort des Servers auf request_secret: alles, was die UDP-Verbindung braucht.
#[derive(Clone, Debug, PartialEq)]
pub struct SecretInfo {
    pub secret: [u8; 16],
    pub port: i32,
    pub player: Uuid,
    // 0 VOIP, 1 AUDIO, 2 RESTRICTED_LOWDELAY (Opus-Anwendung).
    pub codec: u8,
    pub mtu: i32,
    pub distance: f64,
    // Millisekunden zwischen den Keep-Alives des Servers.
    pub keep_alive: i32,
    pub groups_enabled: bool,
    // Leer = gleicher Host wie der Minecraft-Server; nur eine Zahl = anderer Port; sonst
    // "host[:port]".
    pub voice_host: String,
    pub allow_recording: bool,
}

pub fn read_secret(data: &[u8]) -> Result<SecretInfo, ReadError> {
    let mut r = PacketReader::new(data);
    let mut secret = [0u8; 16];
    secret.copy_from_slice(r.read_bytes(16)?);
    Ok(SecretInfo {
        secret,
        port: r.read_i32()?,
        player: r.read_uuid()?,
        codec: r.read_u8()?,
        mtu: r.read_i32()?,
        distance: r.read_f64()?,
        keep_alive: r.read_i32()?,
        groups_enabled: r.read_bool()?,
        voice_host: r.read_string()?,
        allow_recording: r.read_bool()?,
    })
}

impl SecretInfo {
    // Wohin die UDP-Pakete gehen (InitializationData.parseAddress): ohne voice_host der
    // Minecraft-Server mit dem mitgeteilten Port; eine reine Zahl ersetzt nur den Port; sonst
    // Host und optional Port aus "host[:port]".
    pub fn voice_address(&self, server_host: &str) -> (String, u16) {
        let port = u16::try_from(self.port).unwrap_or(24454);
        let host = self.voice_host.trim();
        if host.is_empty() {
            return (server_host.to_string(), port);
        }
        if let Ok(p) = host.parse::<i64>() {
            return (server_host.to_string(), u16::try_from(p).ok().filter(|p| *p > 0).unwrap_or(port));
        }
        let ohne_schema = host.split_once("://").map_or(host, |(_, rest)| rest);
        // IPv6 in eckigen Klammern: [::1]:24454
        if let Some(rest) = ohne_schema.strip_prefix('[') {
            if let Some((h, nach)) = rest.split_once(']') {
                let p = nach.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(port);
                return (h.to_string(), p);
            }
        }
        match ohne_schema.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h.to_string(), p.parse().ok().filter(|p| *p > 0).unwrap_or(port)),
            _ => (ohne_schema.to_string(), port),
        }
    }
}

// Zustand eines Spielers im Sprachchat.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub disabled: bool,
    pub disconnected: bool,
    pub uuid: Uuid,
    pub name: String,
    pub group: Option<Uuid>,
}

fn read_player_state(r: &mut PacketReader) -> Result<PlayerState, ReadError> {
    let disabled = r.read_bool()?;
    let disconnected = r.read_bool()?;
    let uuid = r.read_uuid()?;
    let name = r.read_string()?;
    let group = if r.read_bool()? { Some(r.read_uuid()?) } else { None };
    Ok(PlayerState { disabled, disconnected, uuid, name, group })
}

pub fn read_state(data: &[u8]) -> Result<PlayerState, ReadError> {
    read_player_state(&mut PacketReader::new(data))
}

pub fn read_states(data: &[u8]) -> Result<Vec<PlayerState>, ReadError> {
    let mut r = PacketReader::new(data);
    let n = r.read_i32()?;
    (0..n.clamp(0, 10_000)).map(|_| read_player_state(&mut r)).collect()
}

pub fn read_remove_state(data: &[u8]) -> Result<Uuid, ReadError> {
    PacketReader::new(data).read_uuid()
}

// --- UDP ------------------------------------------------------------------------------------

pub const MAGIC: u8 = 0xFF;

const ID_MIC: u8 = 1;
const ID_PLAYER_SOUND: u8 = 2;
const ID_GROUP_SOUND: u8 = 3;
const ID_LOCATION_SOUND: u8 = 4;
const ID_AUTHENTICATE: u8 = 5;
const ID_AUTHENTICATE_ACK: u8 = 6;
const ID_PING: u8 = 7;
const ID_KEEP_ALIVE: u8 = 8;
const ID_CONNECTION_CHECK: u8 = 9;
const ID_CONNECTION_CHECK_ACK: u8 = 10;

// Flags der Sound-Pakete.
const FLAG_WHISPER: u8 = 1;
const FLAG_CATEGORY: u8 = 2;

// Woher ein empfangener Ton kommt.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundSource {
    // Von einem Spieler in der Naehe: Position = dessen Entity, Reichweite `distance`.
    Player { distance: f32, whisper: bool },
    // Aus der eigenen Gruppe: ohne Ort.
    Group,
    // Von einem festen Punkt (z.B. ein Plugin-Lautsprecher).
    Location { pos: [f64; 3], distance: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub channel: Uuid,
    pub sender: Uuid,
    // Opus-Rahmen; leer = der Sprecher hat aufgehoert.
    pub data: Vec<u8>,
    pub sequence: i64,
    pub source: SoundSource,
    pub category: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UdpPacket {
    Mic { data: Vec<u8>, sequence: i64, whisper: bool },
    Sound(Sound),
    Authenticate { player: Uuid, secret: [u8; 16] },
    AuthenticateAck,
    Ping { id: Uuid, timestamp: i64 },
    KeepAlive,
    ConnectionCheck,
    ConnectionCheckAck,
}

fn write_bytes(w: &mut PacketWriter, b: &[u8]) {
    w.write_varint(b.len() as i32).write_raw_bytes(b);
}

fn read_bytes(r: &mut PacketReader, max: usize) -> Result<Vec<u8>, ReadError> {
    let n = r.read_varint()?;
    if n < 0 || n as usize > max {
        return Err(ReadError);
    }
    Ok(r.read_bytes(n as usize)?.to_vec())
}

impl UdpPacket {
    // Klartext des verschluesselten Blocks: Typ-Byte + Felder.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = PacketWriter::new();
        match self {
            UdpPacket::Mic { data, sequence, whisper } => {
                w.write_u8(ID_MIC);
                write_bytes(&mut w, data);
                w.write_i64(*sequence).write_bool(*whisper);
            }
            UdpPacket::Sound(s) => {
                let id = match s.source {
                    SoundSource::Player { .. } => ID_PLAYER_SOUND,
                    SoundSource::Group => ID_GROUP_SOUND,
                    SoundSource::Location { .. } => ID_LOCATION_SOUND,
                };
                w.write_u8(id).write_uuid(s.channel).write_uuid(s.sender);
                if let SoundSource::Location { pos, .. } = s.source {
                    w.write_f64(pos[0]).write_f64(pos[1]).write_f64(pos[2]);
                }
                write_bytes(&mut w, &s.data);
                w.write_i64(s.sequence);
                let mut flags = if s.category.is_some() { FLAG_CATEGORY } else { 0 };
                match s.source {
                    SoundSource::Player { distance, whisper } => {
                        w.write_f32(distance);
                        if whisper {
                            flags |= FLAG_WHISPER;
                        }
                    }
                    SoundSource::Location { distance, .. } => {
                        w.write_f32(distance);
                    }
                    SoundSource::Group => {}
                }
                w.write_u8(flags);
                if let Some(c) = &s.category {
                    w.write_string(c);
                }
            }
            UdpPacket::Authenticate { player, secret } => {
                w.write_u8(ID_AUTHENTICATE).write_uuid(*player).write_raw_bytes(secret);
            }
            UdpPacket::AuthenticateAck => {
                w.write_u8(ID_AUTHENTICATE_ACK);
            }
            UdpPacket::Ping { id, timestamp } => {
                w.write_u8(ID_PING).write_uuid(*id).write_i64(*timestamp);
            }
            UdpPacket::KeepAlive => {
                w.write_u8(ID_KEEP_ALIVE);
            }
            UdpPacket::ConnectionCheck => {
                w.write_u8(ID_CONNECTION_CHECK);
            }
            UdpPacket::ConnectionCheckAck => {
                w.write_u8(ID_CONNECTION_CHECK_ACK);
            }
        }
        w.into_bytes()
    }

    pub fn decode(plain: &[u8]) -> Result<UdpPacket, ReadError> {
        let mut r = PacketReader::new(plain);
        let id = r.read_u8()?;
        Ok(match id {
            ID_MIC => UdpPacket::Mic { data: read_bytes(&mut r, MAX_OPUS_PAYLOAD)?, sequence: r.read_i64()?, whisper: r.read_bool()? },
            ID_PLAYER_SOUND | ID_GROUP_SOUND | ID_LOCATION_SOUND => {
                let channel = r.read_uuid()?;
                let sender = r.read_uuid()?;
                let pos = if id == ID_LOCATION_SOUND { Some([r.read_f64()?, r.read_f64()?, r.read_f64()?]) } else { None };
                let data = read_bytes(&mut r, MAX_OPUS_PAYLOAD)?;
                let sequence = r.read_i64()?;
                let distance = if id == ID_GROUP_SOUND { 0.0 } else { r.read_f32()? };
                let flags = r.read_u8()?;
                let category = if flags & FLAG_CATEGORY != 0 { Some(r.read_string()?) } else { None };
                let source = match (id, pos) {
                    (ID_PLAYER_SOUND, _) => SoundSource::Player { distance, whisper: flags & FLAG_WHISPER != 0 },
                    (ID_LOCATION_SOUND, Some(pos)) => SoundSource::Location { pos, distance },
                    _ => SoundSource::Group,
                };
                UdpPacket::Sound(Sound { channel, sender, data, sequence, source, category })
            }
            ID_AUTHENTICATE => {
                let player = r.read_uuid()?;
                let mut secret = [0u8; 16];
                secret.copy_from_slice(r.read_bytes(16)?);
                UdpPacket::Authenticate { player, secret }
            }
            ID_AUTHENTICATE_ACK => UdpPacket::AuthenticateAck,
            ID_PING => UdpPacket::Ping { id: r.read_uuid()?, timestamp: r.read_i64()? },
            ID_KEEP_ALIVE => UdpPacket::KeepAlive,
            ID_CONNECTION_CHECK => UdpPacket::ConnectionCheck,
            ID_CONNECTION_CHECK_ACK => UdpPacket::ConnectionCheckAck,
            _ => return Err(ReadError),
        })
    }
}

// Datagramm Client -> Server: 0xFF, eigene UUID, verschluesselter Block mit Laenge.
pub fn client_datagram(player: Uuid, encrypted: &[u8]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_u8(MAGIC).write_uuid(player);
    write_bytes(&mut w, encrypted);
    w.into_bytes()
}

// Datagramm Server -> Client: 0xFF, verschluesselter Block mit Laenge. Rueckgabe: der Block.
pub fn read_server_datagram(datagram: &[u8]) -> Result<Vec<u8>, ReadError> {
    let mut r = PacketReader::new(datagram);
    if r.read_u8()? != MAGIC {
        return Err(ReadError);
    }
    read_bytes(&mut r, 2048)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(n: u8) -> Uuid {
        Uuid([n; 16])
    }

    #[test]
    fn udp_pakete_hin_und_zurueck() {
        let pakete = [
            UdpPacket::Mic { data: vec![1, 2, 3], sequence: 77, whisper: true },
            UdpPacket::Sound(Sound {
                channel: uuid(1),
                sender: uuid(2),
                data: vec![9; 40],
                sequence: -3,
                source: SoundSource::Player { distance: 48.0, whisper: true },
                category: Some("musik".into()),
            }),
            UdpPacket::Sound(Sound { channel: uuid(3), sender: uuid(4), data: vec![], sequence: 5, source: SoundSource::Group, category: None }),
            UdpPacket::Sound(Sound {
                channel: uuid(5),
                sender: uuid(6),
                data: vec![7],
                sequence: 8,
                source: SoundSource::Location { pos: [1.5, -2.0, 300.25], distance: 16.0 },
                category: None,
            }),
            UdpPacket::Authenticate { player: uuid(7), secret: [3; 16] },
            UdpPacket::AuthenticateAck,
            UdpPacket::Ping { id: uuid(8), timestamp: 123_456 },
            UdpPacket::KeepAlive,
            UdpPacket::ConnectionCheck,
            UdpPacket::ConnectionCheckAck,
        ];
        for p in pakete {
            assert_eq!(UdpPacket::decode(&p.encode()).unwrap(), p);
        }
    }

    #[test]
    // Das Mikrofon-Paket genau so, wie die Mod es schreibt: Typ 1, VarInt-Laenge, Opus-Bytes,
    // Long-Sequenz, Bool.
    fn mikrofon_paket_layout() {
        let b = UdpPacket::Mic { data: vec![0xAA, 0xBB], sequence: 1, whisper: false }.encode();
        assert_eq!(b, [1, 2, 0xAA, 0xBB, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn secret_und_zustaende() {
        let mut w = PacketWriter::new();
        w.write_raw_bytes(&[9; 16]).write_i32(24454).write_uuid(uuid(1)).write_u8(0).write_i32(1024).write_f64(48.0).write_i32(1000);
        w.write_bool(true).write_string("").write_bool(false);
        let s = read_secret(&w.into_bytes()).unwrap();
        assert_eq!((s.port, s.player, s.mtu, s.distance, s.keep_alive, s.groups_enabled), (24454, uuid(1), 1024, 48.0, 1000, true));

        let mut w = PacketWriter::new();
        w.write_i32(2);
        w.write_bool(false).write_bool(true).write_uuid(uuid(2)).write_string("Anna").write_bool(false);
        w.write_bool(true).write_bool(false).write_uuid(uuid(3)).write_string("Ben").write_bool(true).write_uuid(uuid(4));
        let st = read_states(&w.into_bytes()).unwrap();
        assert_eq!(st.len(), 2);
        assert!(st[0].disconnected && st[0].group.is_none());
        assert_eq!((st[1].name.as_str(), st[1].group), ("Ben", Some(uuid(4))));
    }

    #[test]
    fn sprachserver_adresse() {
        let mut s = SecretInfo {
            secret: [0; 16],
            port: 24454,
            player: uuid(0),
            codec: 0,
            mtu: 1024,
            distance: 48.0,
            keep_alive: 1000,
            groups_enabled: true,
            voice_host: String::new(),
            allow_recording: false,
        };
        assert_eq!(s.voice_address("play.example.net"), ("play.example.net".into(), 24454));
        s.voice_host = "25000".into();
        assert_eq!(s.voice_address("1.2.3.4"), ("1.2.3.4".into(), 25000));
        s.voice_host = "voice.example.net:26000".into();
        assert_eq!(s.voice_address("x"), ("voice.example.net".into(), 26000));
        s.voice_host = "voice.example.net".into();
        assert_eq!(s.voice_address("x"), ("voice.example.net".into(), 24454));
        s.voice_host = "[::1]:27000".into();
        assert_eq!(s.voice_address("x"), ("::1".into(), 27000));
    }

    #[test]
    fn datagramm_rahmen() {
        let d = client_datagram(uuid(5), &[1, 2, 3]);
        assert_eq!(d[0], 0xFF);
        assert_eq!(&d[1..17], &[5; 16]);
        assert_eq!(&d[17..], &[3, 1, 2, 3]);
        assert_eq!(read_server_datagram(&[0xFF, 2, 7, 8]).unwrap(), vec![7, 8]);
        assert!(read_server_datagram(&[0x00, 0]).is_err());
    }
}
