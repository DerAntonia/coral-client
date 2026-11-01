// Datenschutz (s. Plan): Schluessel und Signaturen aus chat_signing kommen nicht in die
// Aufnahme. Betroffen sind nur drei Play-Pakete an den Server:
// - Chat Session Update (oeffentlicher Schluessel samt Mojang-Signatur): faellt ganz weg.
// - Chat Command Signed (Signaturen je Argument): faellt ganz weg; was der Befehl bewirkt,
//   steht ohnehin in den Paketen vom Server.
// - Chat: bleibt, aber ohne die 256 Byte Signatur (das Flag davor wird 0).

use protocol::packets::{SB_PLAY_CHAT, SB_PLAY_CHAT_SESSION_UPDATE};
use protocol::varint::decode_varint;

const SB_PLAY_CHAT_COMMAND_SIGNED: i32 = 0x07;
const SIGNATURE_LEN: usize = 256;

#[derive(Debug, PartialEq, Eq)]
pub enum Scrubbed<'a> {
    Keep,
    // Aufnehmen als a + [0] + b: a endet vor dem Signatur-Flag, b beginnt hinter der Signatur.
    Strip(&'a [u8], &'a [u8]),
    Drop,
}

// `payload` ist ein Play-Paket an den Server (Packet-ID + Felder).
#[must_use] 
pub fn scrub_c2s_play(payload: &[u8]) -> Scrubbed<'_> {
    let Ok((id, p)) = decode_varint(payload, 0) else { return Scrubbed::Keep };
    match id {
        SB_PLAY_CHAT_SESSION_UPDATE | SB_PLAY_CHAT_COMMAND_SIGNED => Scrubbed::Drop,
        SB_PLAY_CHAT => {
            // Nachricht (String), Zeitstempel und Salt (je i64), dann das Flag.
            let Ok((n, p)) = decode_varint(payload, p) else { return Scrubbed::Drop };
            let flag = p + n.max(0) as usize + 16;
            match payload.get(flag) {
                Some(0) => Scrubbed::Keep,
                Some(1) if payload.len() >= flag + 1 + SIGNATURE_LEN => Scrubbed::Strip(&payload[..flag], &payload[flag + 1 + SIGNATURE_LEN..]),
                // Unerwarteter Aufbau: lieber gar nicht aufnehmen als eine Signatur.
                _ => Scrubbed::Drop,
            }
        }
        _ => Scrubbed::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::packets::{chat_session_update_payload, signed_chat_message_payload};

    #[test]
    fn signatur_faellt_aus_dem_chat() {
        let sig = [0xAB; 256];
        let mit = signed_chat_message_payload("hallo", 1_700_000_000_000, 42, Some(&sig));
        let ohne = signed_chat_message_payload("hallo", 1_700_000_000_000, 42, None);
        let Scrubbed::Strip(a, b) = scrub_c2s_play(&mit) else { panic!("nicht gekuerzt") };
        assert_eq!([a, &[0], b].concat(), ohne);
        assert_eq!(scrub_c2s_play(&ohne), Scrubbed::Keep);
    }

    #[test]
    fn schluessel_paket_faellt_weg() {
        let p = chat_session_update_payload([1; 16], 0, &[2; 300], &[3; 512]);
        assert_eq!(scrub_c2s_play(&p), Scrubbed::Drop);
        assert_eq!(scrub_c2s_play(&[SB_PLAY_CHAT_COMMAND_SIGNED as u8, 1, 2]), Scrubbed::Drop);
        assert_eq!(scrub_c2s_play(&[0x00, 5]), Scrubbed::Keep);
    }
}
