// Feld-Serialisierung fuer Packet-Payloads (ohne Laengen-Praefix -- der kommt erst
// beim Framing in net/ dazu, siehe protocol::varint fuer den Rahmen selbst).

use crate::uuid::Uuid;
use crate::varint::{decode_varint, decode_varlong, encode_varint};

#[derive(Default)]
pub struct PacketWriter {
    buf: Vec<u8>,
}

impl PacketWriter {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn write_varint(&mut self, value: i32) -> &mut Self {
        self.buf.extend_from_slice(&encode_varint(value));
        self
    }

    pub fn write_string(&mut self, s: &str) -> &mut Self {
        self.write_varint(s.len() as i32);
        self.buf.extend_from_slice(s.as_bytes());
        self
    }

    pub fn write_u16(&mut self, value: u16) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn write_uuid(&mut self, uuid: Uuid) -> &mut Self {
        self.buf.extend_from_slice(&uuid.to_bytes());
        self
    }

    // Rohbytes OHNE Laengenangabe -- fuer Felder fester Groesse (UUID, 256-Byte-
    // Chatsignatur). Nicht mit write_bytes verwechseln, das ein VarInt-Praefix schreibt.
    pub fn write_raw_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    // Minecraft-"Prefixed Array of Byte": VarInt-Laenge + Rohbytes. Fuer die
    // Encryption-Response (verschluesseltes Shared Secret / Verify Token).
    pub fn write_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.write_varint(bytes.len() as i32);
        self.buf.extend_from_slice(bytes);
        self
    }

    pub fn write_i64(&mut self, value: i64) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn write_i32(&mut self, value: i32) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn write_u8(&mut self, value: u8) -> &mut Self {
        self.buf.push(value);
        self
    }

    pub fn write_bool(&mut self, value: bool) -> &mut Self {
        self.buf.push(value as u8);
        self
    }

    pub fn write_f64(&mut self, value: f64) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn write_f32(&mut self, value: f32) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

#[derive(Debug)]
pub struct ReadError;

pub struct PacketReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> PacketReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn read_varint(&mut self) -> Result<i32, ReadError> {
        let (value, new_pos) = decode_varint(self.buf, self.pos).map_err(|_| ReadError)?;
        self.pos = new_pos;
        Ok(value)
    }

    pub fn read_varlong(&mut self) -> Result<i64, ReadError> {
        let (value, new_pos) = decode_varlong(self.buf, self.pos).map_err(|_| ReadError)?;
        self.pos = new_pos;
        Ok(value)
    }

    pub fn read_string(&mut self) -> Result<String, ReadError> {
        let len = self.read_varint()? as usize;
        let bytes = self.buf.get(self.pos..self.pos + len).ok_or(ReadError)?;
        self.pos += len;
        String::from_utf8(bytes.to_vec()).map_err(|_| ReadError)
    }

    pub fn read_uuid(&mut self) -> Result<Uuid, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 16).ok_or(ReadError)?;
        self.pos += 16;
        let mut arr = [0u8; 16];
        arr.copy_from_slice(bytes);
        Ok(Uuid::from_bytes(arr))
    }

    pub fn read_i64(&mut self) -> Result<i64, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 8).ok_or(ReadError)?;
        self.pos += 8;
        Ok(i64::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_i32(&mut self) -> Result<i32, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 4).ok_or(ReadError)?;
        self.pos += 4;
        Ok(i32::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_i16(&mut self) -> Result<i16, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 2).ok_or(ReadError)?;
        self.pos += 2;
        Ok(i16::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_f64(&mut self) -> Result<f64, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 8).ok_or(ReadError)?;
        self.pos += 8;
        Ok(f64::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_f32(&mut self) -> Result<f32, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 4).ok_or(ReadError)?;
        self.pos += 4;
        Ok(f32::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_u32(&mut self) -> Result<u32, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 4).ok_or(ReadError)?;
        self.pos += 4;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_u8(&mut self) -> Result<u8, ReadError> {
        let byte = *self.buf.get(self.pos).ok_or(ReadError)?;
        self.pos += 1;
        Ok(byte)
    }

    pub fn read_i8(&mut self) -> Result<i8, ReadError> {
        Ok(self.read_u8()? as i8)
    }

    pub fn read_u16(&mut self) -> Result<u16, ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + 2).ok_or(ReadError)?;
        self.pos += 2;
        Ok(u16::from_be_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_bool(&mut self) -> Result<bool, ReadError> {
        Ok(self.read_u8()? != 0)
    }

    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], ReadError> {
        let bytes = self.buf.get(self.pos..self.pos + len).ok_or(ReadError)?;
        self.pos += len;
        Ok(bytes)
    }

    // Minecraft-"Prefixed Array of Byte": VarInt-Laenge, dann so viele Rohbytes. Fuer die
    // Encryption-Request (Server-Public-Key + Verify Token).
    pub fn read_prefixed_bytes(&mut self) -> Result<&'a [u8], ReadError> {
        let len = self.read_varint()?;
        if len < 0 {
            return Err(ReadError);
        }
        self.read_bytes(len as usize)
    }

    pub fn is_exhausted(&self) -> bool {
        self.pos >= self.buf.len()
    }

    pub fn remaining(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    // Aktuelle Leseposition -- zusammen mit read_since, um ein gerade gelesenes Stueck
    // unveraendert aufzuheben.
    pub fn position(&self) -> usize {
        self.pos
    }

    // Die Bytes zwischen `start` (eine fruehere position()) und der aktuellen Position.
    pub fn read_since(&self, start: usize) -> &'a [u8] {
        &self.buf[start.min(self.pos)..self.pos]
    }
}
