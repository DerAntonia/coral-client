// Minimale UUID-Implementierung (kein externes uuid-Crate noetig fuer den einen
// Anwendungsfall hier). Laut Protokoll-Referenz ist die im Login-Start gesendete
// Player-UUID im Offline-Mode beliebig -- der Server leitet seine eigene ohnehin
// deterministisch aus dem Usernamen ab.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    pub fn random_v4() -> Self {
        let mut bytes = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
        bytes[6] = (bytes[6] & 0x0F) | 0x40; // Version 4
        bytes[8] = (bytes[8] & 0x3F) | 0x80; // Variant RFC 4122
        Self(bytes)
    }

    // 16 Bytes big-endian (obere 64 Bit, dann untere 64 Bit), wie im Protokoll gefordert.
    pub fn to_bytes(self) -> [u8; 16] {
        self.0
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    // Parst die 32-stellige Hex-UUID (mit oder ohne Bindestriche), wie sie der
    // Mojang-Profil-Endpunkt liefert (`id`). Fuer den Online-Mode-Login-Start, wo -- anders
    // als offline -- die ECHTE Konto-UUID gesendet werden muss.
    pub fn from_hex(s: &str) -> Option<Self> {
        let hex: String = s.chars().filter(|c| *c != '-').collect();
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
}
