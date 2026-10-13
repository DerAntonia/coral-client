// Schilder: der Text aus den Block-Entity-Daten (Chunk-Paket und Block Entity Data) und die
// beiden Pakete des Bearbeitens (Open Sign Editor / Sign Update).
//
// SignBlockEntity.writeNbt (1.21.11): "front_text" und "back_text" je ein SignText-Compound
// mit "messages" (vier Text-Components), "color" (DyeColor-Name, Vorgabe "black") und
// "has_glowing_text" (Byte). Haengeschilder haben dasselbe Format.

use crate::buffer::{PacketReader, PacketWriter, ReadError};
use crate::nbt::{self, NbtValue, TextSpan};
use crate::packets::{decode_block_pos, encode_block_pos};

// Clientbound Block Entity Data: Registrierungsindex 6 (s. packets::CB_PLAY_BLOCK_CHANGED_ACK,
// dort ist die Reihenfolge ADD_ENTITY 1 ... BLOCK_ENTITY_DATA 6 aufgezaehlt).
pub const CB_PLAY_BLOCK_ENTITY_DATA: i32 = 0x06;
// Open Sign Editor: zwischen OPEN_SCREEN (0x39) und PING (0x3B), beide verifiziert.
pub const CB_PLAY_OPEN_SIGN_EDITOR: i32 = 0x3A;
// Serverbound Sign Update: direkt vor SWING (0x3C, verifiziert).
pub const SB_PLAY_SIGN_UPDATE: i32 = 0x3B;

// Eine Seite eines Schildes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SignSide {
    pub lines: [Vec<TextSpan>; 4],
    // DyeColor-Index (0 weiss .. 15 schwarz).
    pub color: u8,
    pub glowing: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SignText {
    pub front: SignSide,
    pub back: SignSide,
}

// Die DyeColor-Namen in Index-Reihenfolge.
pub const DYE_NAMES: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red",
    "black",
];

fn side_from_nbt(v: Option<&NbtValue>) -> SignSide {
    let mut side = SignSide { color: 15, ..Default::default() };
    let Some(v) = v else { return side };
    if let Some(NbtValue::List(messages)) = v.get("messages") {
        for (i, m) in messages.iter().take(4).enumerate() {
            // Gemischte Listen (Zeichenkette neben Compound) packt 1.21.5+ in {"": wert}.
            let m = match m.as_compound() {
                Some(map) if map.len() == 1 && map.contains_key("") => &map[""],
                _ => m,
            };
            side.lines[i] = nbt::text_to_spans(m);
        }
    }
    if let Some(c) = v.get("color").and_then(NbtValue::as_str) {
        side.color = DYE_NAMES.iter().position(|n| *n == c).unwrap_or(15) as u8;
    }
    side.glowing = v.get("has_glowing_text").and_then(NbtValue::as_i32).unwrap_or(0) != 0;
    side
}

// Schildtext aus dem NBT eines Block-Entities, oder None, wenn es kein Schild ist.
pub fn sign_from_nbt(v: &NbtValue) -> Option<SignText> {
    if v.get("front_text").is_none() && v.get("back_text").is_none() {
        return None;
    }
    Some(SignText { front: side_from_nbt(v.get("front_text")), back: side_from_nbt(v.get("back_text")) })
}

// Block Entity Data: Position, Typ, NBT. Nur Schilder interessieren -- alles andere None.
pub fn read_block_entity_data(payload: &[u8]) -> Result<Option<((i32, i32, i32), SignText)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let pos = decode_block_pos(r.read_i64()?);
    let _typ = r.read_varint()?;
    let data = nbt::read_root(&mut r)?;
    Ok(data.as_ref().and_then(sign_from_nbt).map(|s| (pos, s)))
}

// Dasselbe Paket fuer einen platzierten Kopf: Position und Profil, sonst None.
pub fn read_block_entity_head(payload: &[u8]) -> Result<Option<((i32, i32, i32), crate::item::HeadProfile)>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let pos = decode_block_pos(r.read_i64()?);
    let _typ = r.read_varint()?;
    let data = nbt::read_root(&mut r)?;
    Ok(data.as_ref().and_then(crate::item::head_profile_from_nbt).map(|p| (pos, p)))
}

// Open Sign Editor: Position + ob die Vorderseite bearbeitet wird.
pub fn read_open_sign_editor(payload: &[u8]) -> Result<((i32, i32, i32), bool), ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let pos = decode_block_pos(r.read_i64()?);
    let front = r.read_bool()?;
    Ok((pos, front))
}

// Sign Update (UpdateSignC2SPacket): Position, Seite, vier Zeilen als Klartext.
pub fn sign_update_payload(pos: (i32, i32, i32), front: bool, lines: &[String; 4]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SIGN_UPDATE);
    w.write_i64(encode_block_pos(pos.0, pos.1, pos.2));
    w.write_bool(front);
    for line in lines {
        w.write_string(line);
    }
    w.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_sign_editor_liest_position_und_seite() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_OPEN_SIGN_EDITOR);
        w.write_i64(encode_block_pos(10, 64, -5));
        w.write_bool(false);
        assert_eq!(read_open_sign_editor(&w.into_bytes()).unwrap(), ((10, 64, -5), false));
    }

    #[test]
    fn sign_update_schreibt_vier_zeilen() {
        let lines = ["a".to_string(), String::new(), "c".to_string(), String::new()];
        let bytes = sign_update_payload((1, 2, 3), true, &lines);
        let mut r = PacketReader::new(&bytes);
        assert_eq!(r.read_varint().unwrap(), SB_PLAY_SIGN_UPDATE);
        assert_eq!(decode_block_pos(r.read_i64().unwrap()), (1, 2, 3));
        assert!(r.read_bool().unwrap());
        assert_eq!(r.read_string().unwrap(), "a");
        assert_eq!(r.read_string().unwrap(), "");
        assert_eq!(r.read_string().unwrap(), "c");
    }
}
