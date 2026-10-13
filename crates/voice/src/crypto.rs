// Verschluesselung der UDP-Bloecke: AES-128-GCM mit dem 16-Byte-Secret aus voicechat:secret.
// Ein Block ist IV (12 zufaellige Bytes) + Chiffrat + Tag (16 Bytes), ohne Zusatzdaten.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use rand::RngCore;

const IV_LEN: usize = 12;

pub struct Cipher(Aes128Gcm);

impl Cipher {
    pub fn new(secret: &[u8; 16]) -> Self {
        Cipher(Aes128Gcm::new(secret.into()))
    }

    pub fn encrypt(&self, plain: &[u8]) -> Vec<u8> {
        let mut iv = [0u8; IV_LEN];
        rand::thread_rng().fill_bytes(&mut iv);
        let chiffrat = self.0.encrypt(Nonce::from_slice(&iv), plain).unwrap_or_default();
        let mut out = Vec::with_capacity(IV_LEN + chiffrat.len());
        out.extend_from_slice(&iv);
        out.extend_from_slice(&chiffrat);
        out
    }

    // None = zu kurz oder Tag stimmt nicht (falsches Secret, verfaelscht).
    pub fn decrypt(&self, block: &[u8]) -> Option<Vec<u8>> {
        if block.len() < IV_LEN + 16 {
            return None;
        }
        let (iv, rest) = block.split_at(IV_LEN);
        self.0.decrypt(Nonce::from_slice(iv), rest).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hin_und_zurueck_und_falscher_schluessel() {
        let a = Cipher::new(&[1; 16]);
        let block = a.encrypt(b"hallo");
        assert_eq!(block.len(), 12 + 5 + 16);
        assert_eq!(a.decrypt(&block).as_deref(), Some(&b"hallo"[..]));
        assert!(Cipher::new(&[2; 16]).decrypt(&block).is_none());
        // Zwei Verschluesselungen desselben Texts unterscheiden sich (zufaelliger IV).
        assert_ne!(a.encrypt(b"x"), a.encrypt(b"x"));
    }

    #[test]
    // Ein Block, den Javas AES/GCM/NoPadding erzeugt hat (Schluessel 00..0f, IV 00..0b,
    // Klartext "voice", ohne AAD) -- IV vorn, Tag hinten, genau wie bei der Mod.
    fn entschluesselt_was_java_verschluesselt() {
        let key: [u8; 16] = core::array::from_fn(|i| i as u8);
        let mut block: Vec<u8> = (0..12).collect();
        block.extend_from_slice(&[
            0xe5, 0x03, 0xce, 0xad, 0x03, 0xf3, 0xbd, 0xa0, 0x8f, 0xc5, 0x38, 0x71, 0xb4, 0x78, 0xa7, 0xd2, 0xf9, 0xca, 0x01, 0xfb, 0x74,
        ]);
        assert_eq!(Cipher::new(&key).decrypt(&block).as_deref(), Some(&b"voice"[..]));
    }
}
