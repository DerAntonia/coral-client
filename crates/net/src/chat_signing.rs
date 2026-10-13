// Signieren von Chatnachrichten (seit 1.19.1 "secure chat").
//
// Server mit enforce-secure-profile=true nehmen nur Nachrichten an, die mit dem
// Profilschluessel des Kontos unterschrieben sind -- fehlt der, antwortet der Server mit
// "chat.disabled.missingProfileKey". Der Schluessel kommt von Mojang (s.
// auth::fetch_profile_key), hier wird damit unterschrieben.
//
// Signiert wird NICHT die Nachricht allein, sondern eine Kette: jede Nachricht traegt einen
// laufenden Index und die Sitzungs-UUID mit, damit der Server Nachrichten weder umsortieren
// noch aus einer anderen Sitzung einspielen kann. Byte-Layout exakt wie
// SignedMessage.update + MessageLink.update + MessageBody.update im 1.21.11-Jar:
//
//   i32  1                     (Version des Formats)
//   16B  Absender-UUID         (Most/Least Significant Bits, big endian)
//   16B  Sitzungs-UUID
//   i32  Index                 (0 fuer die erste Nachricht der Sitzung, dann +1)
//   i64  Salt
//   i64  Zeitstempel in SEKUNDEN (das Paket selbst traegt Millisekunden -- nicht verwechseln)
//   i32  Laenge des Textes in UTF-8-Bytes
//   ..   der Text
//   i32  Anzahl der quittierten Nachrichten, dann je deren 256-Byte-Signatur
//
// Darueber laeuft SHA256withRSA (PKCS#1 v1.5), das Ergebnis sind 256 Byte.

use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::sha2::Sha256;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;

pub struct ChatSigner {
    signing_key: SigningKey<Sha256>,
    sender: [u8; 16],
    session_id: [u8; 16],
    // Laufender Index der Kette. Jede signierte Nachricht erhoeht ihn -- Luecken oder
    // Wiederholungen brechen die Kette serverseitig.
    index: i32,
}

// Frische Sitzungs-UUID (Version 4). Pro Verbindung eine neue -- der Server bindet die
// Signaturkette daran, eine wiederverwendete Sitzung mit zurueckgesetztem Index wuerde er
// als gebrochene Kette werten.
pub fn random_session_id() -> [u8; 16] {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // Version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // Variante RFC 4122
    bytes
}

#[derive(Debug)]
pub struct SigningError(pub String);

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Chat-Signatur: {}", self.0)
    }
}

impl ChatSigner {
    // `private_key_der` ist PKCS#8 (s. auth::ProfileKey), `sender` die Konto-UUID,
    // `session_id` eine pro Verbindung frisch gewuerfelte UUID.
    pub fn new(private_key_der: &[u8], sender: [u8; 16], session_id: [u8; 16]) -> Result<Self, SigningError> {
        let key = RsaPrivateKey::from_pkcs8_der(private_key_der).map_err(|e| SigningError(format!("privater Schluessel unlesbar: {e}")))?;
        Ok(Self { signing_key: SigningKey::<Sha256>::new(key), sender, session_id, index: 0 })
    }

    pub fn session_id(&self) -> [u8; 16] {
        self.session_id
    }

    // Signiert eine Nachricht und ruecke die Kette einen Schritt weiter. `timestamp_millis`
    // muss derselbe Wert sein, der ins Paket geschrieben wird -- signiert werden davon aber
    // nur die vollen Sekunden.
    pub fn sign(&mut self, message: &str, timestamp_millis: i64, salt: i64) -> Result<[u8; 256], SigningError> {
        let mut payload = Vec::with_capacity(64 + message.len());
        payload.extend_from_slice(&1i32.to_be_bytes());
        payload.extend_from_slice(&self.sender);
        payload.extend_from_slice(&self.session_id);
        payload.extend_from_slice(&self.index.to_be_bytes());
        payload.extend_from_slice(&salt.to_be_bytes());
        payload.extend_from_slice(&timestamp_millis.div_euclid(1000).to_be_bytes());
        let content = message.as_bytes();
        payload.extend_from_slice(&(content.len() as i32).to_be_bytes());
        payload.extend_from_slice(content);
        // Wir quittieren (noch) keine empfangenen Nachrichten -- die Liste ist leer, im
        // Paket steht dieselbe leere Quittung (s. packets::chat_message_payload).
        payload.extend_from_slice(&0i32.to_be_bytes());

        let signature = self.signing_key.sign(&payload).to_vec();
        let signature: [u8; 256] =
            signature.try_into().map_err(|v: Vec<u8>| SigningError(format!("unerwartete Signaturlaenge {} (erwartet 256)", v.len())))?;
        self.index += 1;
        Ok(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1v15::VerifyingKey;
    use rsa::pkcs8::EncodePrivateKey;
    use rsa::signature::Verifier;
    use rsa::RsaPublicKey;

    fn test_key() -> (Vec<u8>, RsaPublicKey) {
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048).expect("Testschluessel");
        let der = private.to_pkcs8_der().expect("PKCS#8").as_bytes().to_vec();
        let public = RsaPublicKey::from(&private);
        (der, public)
    }

    // Gegenprobe mit demselben Byte-Layout, das ein Server pruefen wuerde: signiert wird
    // die Kette, nicht der blosse Text.
    #[test]
    fn signatur_passt_zum_erwarteten_payload() {
        let (der, public) = test_key();
        let sender = [1u8; 16];
        let session = [2u8; 16];
        let mut signer = ChatSigner::new(&der, sender, session).unwrap();
        let signature = signer.sign("hallo", 1_700_000_123_456, 42).unwrap();

        let mut expected = Vec::new();
        expected.extend_from_slice(&1i32.to_be_bytes());
        expected.extend_from_slice(&sender);
        expected.extend_from_slice(&session);
        expected.extend_from_slice(&0i32.to_be_bytes()); // erster Index
        expected.extend_from_slice(&42i64.to_be_bytes());
        expected.extend_from_slice(&1_700_000_123i64.to_be_bytes()); // Sekunden, nicht Millis
        expected.extend_from_slice(&5i32.to_be_bytes());
        expected.extend_from_slice(b"hallo");
        expected.extend_from_slice(&0i32.to_be_bytes());

        let verifying = VerifyingKey::<Sha256>::new(public);
        let parsed = rsa::pkcs1v15::Signature::try_from(signature.as_slice()).unwrap();
        verifying.verify(&expected, &parsed).expect("Signatur muss zum erwarteten Payload passen");
    }

    // Der Index MUSS je Nachricht wachsen -- sonst bricht die Kette und der Server verwirft
    // alles Folgende.
    #[test]
    fn index_waechst_je_nachricht() {
        let (der, public) = test_key();
        let mut signer = ChatSigner::new(&der, [0; 16], [0; 16]).unwrap();
        let first = signer.sign("a", 1000, 0).unwrap();
        let second = signer.sign("a", 1000, 0).unwrap();
        assert_ne!(first, second, "gleiche Nachricht darf nicht dieselbe Signatur ergeben");

        let mut expected = Vec::new();
        expected.extend_from_slice(&1i32.to_be_bytes());
        expected.extend_from_slice(&[0u8; 32]);
        expected.extend_from_slice(&1i32.to_be_bytes()); // zweite Nachricht -> Index 1
        expected.extend_from_slice(&0i64.to_be_bytes());
        expected.extend_from_slice(&1i64.to_be_bytes());
        expected.extend_from_slice(&1i32.to_be_bytes());
        expected.extend_from_slice(b"a");
        expected.extend_from_slice(&0i32.to_be_bytes());
        let verifying = VerifyingKey::<Sha256>::new(public);
        let parsed = rsa::pkcs1v15::Signature::try_from(second.as_slice()).unwrap();
        verifying.verify(&expected, &parsed).expect("zweite Signatur nutzt Index 1");
    }
}
