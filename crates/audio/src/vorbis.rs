//! Ogg-Vorbis lesen -- das Format, in dem Minecraft seine Klaenge ablegt.
//!
//! Warum ueberhaupt ein Dekoder und nicht einmal vorab nach WAV wandeln: es sind 4405 Dateien.
//! Als WAV waeren das mehrere hundert Megabyte, die entweder mitgeliefert (rechtlich nicht
//! moeglich, s. assets/locate.rs) oder beim ersten Start erzeugt werden muessten. Direkt lesen
//! kostet dagegen ein paar Millisekunden je Klang -- und zwar nur einmal, danach haelt die
//! Klangbank ihn (s. app::vanilla_sounds).
//!
//! Das Ergebnis ist ein `Wav`, also derselbe Typ wie fuer die eigenen Aufnahmen: der Mixer
//! soll nicht wissen muessen, woher ein Klang kam.

use crate::wav::Wav;

#[derive(Debug)]
pub enum VorbisError {
    Decode(lewton::VorbisError),
    /// Datei ohne Kanaele -- dann gibt es nichts abzuspielen, und das Mischen wuerde durch
    /// null teilen.
    KeineKanaele,
}

impl std::fmt::Display for VorbisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VorbisError::Decode(e) => write!(f, "Vorbis-Dekodierfehler: {e}"),
            VorbisError::KeineKanaele => write!(f, "Datei enthaelt keine Kanaele"),
        }
    }
}

impl std::error::Error for VorbisError {}

/// Eine .ogg-Datei aus dem Speicher lesen.
///
/// Mehrkanaliges wird zu MONO gemischt, wie beim WAV-Leser auch -- und hier ist das nicht nur
/// Vereinfachung, sondern noetig: die Raumklang-Stufe erzeugt die beiden Seiten selbst aus
/// Richtung und Entfernung (s. spatial.rs). Ein Klang, der seine eigene Stereobreite
/// mitbraechte, wuerde dagegenarbeiten -- sein linker Kanal landete teils im rechten Ohr, und
/// die Laufzeitdifferenz waere verschmiert.
pub fn decode(bytes: &[u8]) -> Result<Wav, VorbisError> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes)).map_err(VorbisError::Decode)?;
    let channels = reader.ident_hdr.audio_channels as usize;
    let sample_rate = reader.ident_hdr.audio_sample_rate as f32;
    if channels == 0 {
        return Err(VorbisError::KeineKanaele);
    }

    let mut samples: Vec<f32> = Vec::new();
    // lewton liefert paketweise; ein Paket enthaelt je Kanal einen Block gleicher Laenge.
    while let Some(paket) = reader.read_dec_packet_itl().map_err(VorbisError::Decode)? {
        // read_dec_packet_itl gibt die Kanaele VERSCHRAENKT (L, R, L, R, ...) -- deshalb je
        // `channels` Werte zu einem Mono-Wert mitteln.
        for frame in paket.chunks(channels) {
            let summe: f32 = frame.iter().map(|v| *v as f32 / 32768.0).sum();
            samples.push(summe / frame.len() as f32);
        }
    }

    Ok(Wav { samples, sample_rate })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Kein eingebautes Testmaterial: das einzige Vorbis, das hier vorliegt, sind Vanillas
    // Klaenge, und die duerfen nicht ins Repo (s. assets/locate.rs). Der Test nimmt sie
    // deshalb aus der lokalen Installation -- und wird uebersprungen, wenn dort keine ist,
    // statt auf einem fremden Rechner grundlos rot zu sein.
    fn eine_vanilla_ogg() -> Option<Vec<u8>> {
        let home = std::env::var_os("HOME")?;
        let root = std::path::PathBuf::from(home).join(".minecraft").join("assets");
        let index = std::fs::read_to_string(root.join("indexes").join("29.json")).ok()?;
        // Winziger Griff in den JSON-Text statt einer JSON-Abhaengigkeit im audio-Crate:
        // gesucht wird der Hash hinter irgendeinem Sound-Eintrag.
        let marke = "\"minecraft/sounds/";
        let start = index.find(marke)?;
        let hash_start = index[start..].find("\"hash\": \"")? + start + 9;
        let hash = &index[hash_start..hash_start + 40];
        std::fs::read(root.join("objects").join(&hash[0..2]).join(hash)).ok()
    }

    #[test]
    fn eine_echte_vanilla_datei_wird_zu_hoerbaren_abtastwerten() {
        let Some(bytes) = eine_vanilla_ogg() else {
            eprintln!("keine lokale Minecraft-Installation -- Test uebersprungen");
            return;
        };
        let wav = decode(&bytes).expect("Vanilla-Ogg muss lesbar sein");
        assert!(!wav.samples.is_empty(), "leer dekodiert");
        assert!(wav.sample_rate >= 8000.0, "unplausible Abtastrate: {}", wav.sample_rate);
        // Im Aussteuerbereich, sonst stimmt die Skalierung nicht (ein Faktor 32768 daneben
        // waere entweder unhoerbar leise oder ein Uebersteuern).
        assert!(wav.samples.iter().all(|s| s.abs() <= 1.0), "ausserhalb von -1..1");
        let spitze = wav.samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(spitze > 0.01, "praktisch stumm dekodiert (Spitze {spitze})");
    }

    #[test]
    fn muell_ergibt_einen_fehler_und_keinen_absturz() {
        assert!(decode(b"das ist kein ogg").is_err());
        assert!(decode(&[]).is_err());
    }
}
