// Bausteine des Online-Mode-Verschluesselungs-Handshakes.
//
// Ablauf (s. connection.rs, do_login, EncryptionRequest): Der Server schickt seinen
// RSA-Public-Key + ein Verify Token. Der Client wuerfelt ein 16-Byte Shared Secret, bildet
// daraus + serverId + Public-Key den Mojang-Server-Hash (fuer den Session-Join), verschluesselt
// Shared Secret und Verify Token mit dem Public-Key und schaltet danach den gesamten
// TCP-Bytestrom auf AES-128-CFB8 um (Schluessel = IV = Shared Secret).

use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;
use num_bigint::BigInt;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use sha1::{Digest, Sha1};

// AES-128 im CFB8-Modus als fortlaufende Byte-Chiffre. Minecraft verschluesselt jedes Byte
// einzeln: das 16-Byte-Schieberegister (Start = IV) wird per AES-Block verschluesselt, das
// oberste Keystream-Byte mit dem Klartext/Ciphertext-Byte ver-XOR-t, dann das Ciphertext-Byte
// hinten ins Register geschoben. Ver- und Entschluesselung nutzen dieselbe AES-Vorwaertsrichtung,
// schieben aber jeweils das Ciphertext-Byte nach -- der einzige Unterschied liegt darin, ob das
// ein- oder ausgehende Byte das Ciphertext-Byte ist.
pub struct Cfb8 {
    cipher: Aes128,
    register: [u8; 16],
}

impl Cfb8 {
    // In Minecraft ist der Initialisierungsvektor identisch mit dem Schluessel (dem Shared
    // Secret) -- fuer beide Richtungen je eine eigene Instanz mit eigenem Registerzustand.
    pub fn new(secret: &[u8; 16]) -> Self {
        Self { cipher: Aes128::new(secret.into()), register: *secret }
    }

    fn keystream_byte(&self) -> u8 {
        let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(&self.register);
        self.cipher.encrypt_block(&mut block);
        block[0]
    }

    fn shift_in(&mut self, cipher_byte: u8) {
        self.register.copy_within(1..16, 0);
        self.register[15] = cipher_byte;
    }

    pub fn encrypt(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            let c = *b ^ self.keystream_byte();
            self.shift_in(c);
            *b = c;
        }
    }

    pub fn decrypt(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            let cipher_byte = *b;
            let p = cipher_byte ^ self.keystream_byte();
            self.shift_in(cipher_byte);
            *b = p;
        }
    }
}

// Mojang-Server-Hash: SHA-1 ueber serverId(ASCII) + Shared Secret + Public-Key, dann als
// VORZEICHENBEHAFTETE Hex-Zahl (Zweierkomplement, ggf. mit fuehrendem '-') formatiert -- eine
// Eigenheit von Minecraft, kein normaler Hex-Digest.
pub fn server_hash(server_id: &str, shared_secret: &[u8], public_key: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(server_id.as_bytes());
    hasher.update(shared_secret);
    hasher.update(public_key);
    let digest = hasher.finalize();
    BigInt::from_signed_bytes_be(&digest).to_str_radix(16)
}

// Verschluesselt beliebige Daten (Shared Secret / Verify Token) mit dem DER-kodierten
// RSA-Public-Key des Servers (SubjectPublicKeyInfo), PKCS#1 v1.5 -- so wie der Vanilla-Client.
pub fn rsa_encrypt(public_key_der: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let key = RsaPublicKey::from_public_key_der(public_key_der).map_err(|e| format!("Server-Public-Key nicht lesbar: {e}"))?;
    let mut rng = rand::thread_rng();
    key.encrypt(&mut rng, Pkcs1v15Encrypt, data).map_err(|e| format!("RSA-Verschluesselung fehlgeschlagen: {e}"))
}

// 16 Zufallsbytes als Shared Secret (AES-128-Schluessel).
pub fn random_shared_secret() -> [u8; 16] {
    use rand::RngCore;
    let mut secret = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut secret);
    secret
}

#[cfg(test)]
mod tests {
    use super::*;

    // CFB8 muss sich selbst invertieren: encrypt gefolgt von decrypt (mit frischer Instanz,
    // gleichem Schluessel) ergibt wieder den Klartext -- auch ueber Blockgrenzen hinweg.
    #[test]
    fn cfb8_roundtrip() {
        let secret = [7u8; 16];
        let plain: Vec<u8> = (0..100u16).map(|i| i as u8).collect();
        let mut data = plain.clone();
        Cfb8::new(&secret).encrypt(&mut data);
        assert_ne!(data, plain, "verschluesselt darf nicht gleich Klartext sein");
        Cfb8::new(&secret).decrypt(&mut data);
        assert_eq!(data, plain, "decrypt(encrypt(x)) muss x sein");
    }

    // Bekannter Testvektor fuer den Minecraft-Server-Hash (aus der offiziellen wiki.vg-Doku):
    // "Notch" -> 4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48, ein negativer Wert (fuehrendes '-').
    #[test]
    fn server_hash_notch_vektor() {
        // Der dokumentierte Vektor entsteht aus leerem serverId + secret "Notch" (als Rohbytes)
        // ohne Public-Key -- wir bilden ihn hier direkt aus den Eingaberohbytes nach.
        let h = server_hash("Notch", &[], &[]);
        assert_eq!(h, "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48");
    }

    #[test]
    fn server_hash_jeb_negativ() {
        let h = server_hash("jeb_", &[], &[]);
        assert_eq!(h, "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1");
    }
}
