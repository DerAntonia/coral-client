// VarInt-Encoding nach Java-Edition-Protokoll: 7 Nutzbits pro Byte, MSB = Continuation-Flag,
// maximal 5 Bytes fuer ein i32. Siehe https://minecraft.wiki/w/Java_Edition_protocol/Data_types

pub fn encode_varint(mut value: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    loop {
        let mut byte = (value as u32 & 0x7F) as u8;
        value = ((value as u32) >> 7) as i32;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

#[derive(Debug)]
pub struct VarIntTooLong;

// Liest einen VarInt aus `buf` beginnend bei `pos`; gibt (Wert, neue Position) zurueck.
pub fn decode_varint(buf: &[u8], pos: usize) -> Result<(i32, usize), VarIntTooLong> {
    let mut value: i32 = 0;
    let mut offset = pos;
    for i in 0..5 {
        let byte = *buf.get(offset).ok_or(VarIntTooLong)?;
        offset += 1;
        value |= ((byte & 0x7F) as i32) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((value, offset));
        }
    }
    Err(VarIntTooLong)
}

// Wie decode_varint, aber 64-Bit (max. 10 Bytes) -- braucht z.B. Update Section Blocks fuer
// die (BlockState<<12 | Position)-Eintraege.
pub fn decode_varlong(buf: &[u8], pos: usize) -> Result<(i64, usize), VarIntTooLong> {
    let mut value: i64 = 0;
    let mut offset = pos;
    for i in 0..10 {
        let byte = *buf.get(offset).ok_or(VarIntTooLong)?;
        offset += 1;
        value |= ((byte & 0x7F) as i64) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((value, offset));
        }
    }
    Err(VarIntTooLong)
}
