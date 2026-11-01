// Handshake, Status und Login.

use super::*;

// --- Handshaking state, serverbound, id 0x00 ---
pub struct Handshake {
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub intent: i32,
}

impl Handshake {
    #[must_use] 
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
#[must_use] 
pub fn status_request_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x00);
    w.into_bytes()
}

// Serverbound, id 0x01, Payload = beliebiger i64 (Timestamp).
#[must_use] 
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
    #[must_use] 
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
#[must_use] 
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
#[must_use] 
pub fn login_acknowledged_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(0x03);
    w.into_bytes()
}

// --- Configuration state ---
