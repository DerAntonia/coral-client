// Ein RIFF/WAVE-Leser fuer genau das Format, das wir selbst ablegen: PCM, 16 Bit.
//
// Warum von Hand und nicht per Crate: eine WAV-Datei ist ein Kopf und danach die rohen
// Abtastwerte -- das sind die paar Dutzend Zeilen hier. Ein Dekoder waere noetig gewesen, wenn
// wir die Klaenge als OGG mitliefern wuerden (Kenneys Originalpakete sind OGG); sie werden aber
// schon beim Ablegen ins Repo nach WAV gewandelt, s. assets/sounds/README.md. Damit bleibt der
// Client ohne Audio-Abhaengigkeit.
//
// Bewusst NICHT unterstuetzt: 8/24/32 Bit, Float-WAV, komprimierte Formate. Alles davon waere
// toter Code -- unsere Dateien haben ein einziges Format, und ein Leser, der mehr verspricht,
// als je gebraucht wird, ist nur mehr Flaeche fuer Fehler.

#[derive(Debug, PartialEq)]
pub enum WavError {
    ZuKurz,
    KeinRiffWave,
    ChunkFehlt(&'static str),
    NichtPcm16(u16, u16),
    KanaeleUngueltig(u16),
}

#[derive(Debug)]
pub struct Wav {
    /// Abtastwerte in -1..1. Mehrkanalige Dateien werden gemischt (s. from_bytes).
    pub samples: Vec<f32>,
    pub sample_rate: f32,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Wav {
    pub fn from_bytes(bytes: &[u8]) -> Result<Wav, WavError> {
        if bytes.len() < 12 {
            return Err(WavError::ZuKurz);
        }
        if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(WavError::KeinRiffWave);
        }

        // Die Chunks durchlaufen statt "fmt liegt bei 12, data bei 36" anzunehmen: manche
        // Encoder schieben LIST/fact-Chunks dazwischen, und dann waeren feste Offsets falsch.
        let mut fmt: Option<(u16, u16, u32, u16)> = None;
        let mut daten: Option<&[u8]> = None;
        let mut pos = 12;
        while pos + 8 <= bytes.len() {
            let kennung = &bytes[pos..pos + 4];
            let laenge = u32_at(bytes, pos + 4) as usize;
            let inhalt_ab = pos + 8;
            let inhalt_bis = inhalt_ab.saturating_add(laenge).min(bytes.len());
            match kennung {
                b"fmt " if laenge >= 16 => {
                    fmt = Some((
                        u16_at(bytes, inhalt_ab),      // Format-Kennung, 1 = PCM
                        u16_at(bytes, inhalt_ab + 2),  // Kanaele
                        u32_at(bytes, inhalt_ab + 4),  // Abtastrate
                        u16_at(bytes, inhalt_ab + 14), // Bits je Wert
                    ));
                }
                b"data" => daten = Some(&bytes[inhalt_ab..inhalt_bis]),
                _ => {}
            }
            // Chunks sind auf gerade Laengen ausgerichtet -- die ungerade Fuellbyte-Regel zu
            // uebersehen verschiebt alle folgenden Chunks um eins.
            pos = inhalt_ab + laenge + (laenge & 1);
        }

        let (format, kanaele, rate, bits) = fmt.ok_or(WavError::ChunkFehlt("fmt "))?;
        let daten = daten.ok_or(WavError::ChunkFehlt("data"))?;
        if format != 1 || bits != 16 {
            return Err(WavError::NichtPcm16(format, bits));
        }
        if kanaele == 0 {
            return Err(WavError::KanaeleUngueltig(kanaele));
        }

        // Mehrkanaliges wird zu Mono gemittelt: die Stereo-Lage machen wir selbst (s.
        // pan_gains), eine mitgelieferte Lage wuerde damit nur in Konflikt geraten.
        let kanaele = kanaele as usize;
        let werte: Vec<f32> = daten.chunks_exact(2).map(|p| i16::from_le_bytes([p[0], p[1]]) as f32 / 32_768.0).collect();
        let samples = if kanaele == 1 { werte } else { werte.chunks(kanaele).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect() };

        Ok(Wav { samples, sample_rate: rate as f32 })
    }

    /// Wert an einer Position in Sekunden, linear zwischen den Abtastwerten interpoliert.
    ///
    /// Die Interpolation ist das Resampling: unsere Dateien liegen mit 48 kHz vor, das Geraet
    /// laeuft aber nicht zwingend mit 48 kHz (s. low_latency_config -- die bevorzugte Rate ist
    /// nur ein Wunsch). Ohne Umrechnung waere ein Klang auf einem 44,1-kHz-Geraet um knapp einen
    /// Halbton zu hoch und entsprechend zu kurz.
    pub fn sample_at(&self, sekunden: f32) -> f32 {
        if self.samples.is_empty() || sekunden < 0.0 {
            return 0.0;
        }
        let pos = sekunden * self.sample_rate;
        let i = pos.floor() as usize;
        if i + 1 >= self.samples.len() {
            return *self.samples.last().filter(|_| i < self.samples.len()).unwrap_or(&0.0);
        }
        let bruch = pos - i as f32;
        self.samples[i] * (1.0 - bruch) + self.samples[i + 1] * bruch
    }

    pub fn duration(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate.max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Eine WAV-Datei im Speicher bauen -- damit braucht der Test keine Datei auf der Platte.
    fn baue_wav(kanaele: u16, rate: u32, werte: &[i16]) -> Vec<u8> {
        let daten: Vec<u8> = werte.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36u32 + daten.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&kanaele.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * kanaele as u32 * 2).to_le_bytes());
        b.extend_from_slice(&(kanaele * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(daten.len() as u32).to_le_bytes());
        b.extend_from_slice(&daten);
        b
    }

    #[test]
    fn liest_mono_pcm16() {
        let w = Wav::from_bytes(&baue_wav(1, 48_000, &[0, 16_384, -16_384, 32_767])).expect("WAV");
        assert_eq!(w.sample_rate, 48_000.0);
        assert_eq!(w.samples.len(), 4);
        assert!((w.samples[1] - 0.5).abs() < 1e-3, "{}", w.samples[1]);
        assert!((w.samples[2] + 0.5).abs() < 1e-3);
    }

    #[test]
    // Stereo wird zu Mono gemittelt -- die Stereo-Lage vergibt der Mixer selbst.
    fn mischt_stereo_zu_mono() {
        // Zwei Frames: (1.0, 0.0) und (-1.0, -1.0) -> 0.5 und -1.0
        let w = Wav::from_bytes(&baue_wav(2, 48_000, &[32_767, 0, -32_768, -32_768])).expect("WAV");
        assert_eq!(w.samples.len(), 2);
        assert!((w.samples[0] - 0.5).abs() < 1e-3, "{}", w.samples[0]);
        assert!((w.samples[1] + 1.0).abs() < 1e-3, "{}", w.samples[1]);
    }

    #[test]
    // Chunks muessen durchlaufen werden: ein LIST-Chunk vor dem data-Chunk darf den Leser nicht
    // aus dem Tritt bringen. Mit fest angenommenen Offsets waere genau das passiert.
    fn ueberspringt_fremde_chunks() {
        let mut b = baue_wav(1, 48_000, &[1000, 2000]);
        // Einen LIST-Chunk hinter "WAVE" (Position 12) einschieben.
        let mut list = Vec::new();
        list.extend_from_slice(b"LIST");
        list.extend_from_slice(&6u32.to_le_bytes());
        list.extend_from_slice(b"INFOxy");
        b.splice(12..12, list);
        let w = Wav::from_bytes(&b).expect("WAV mit LIST");
        assert_eq!(w.samples.len(), 2);
    }

    #[test]
    // Ungerade Chunk-Laengen haben ein Fuellbyte. Wer das uebersieht, verschiebt alles danach.
    fn beachtet_das_fuellbyte_bei_ungerader_laenge() {
        let mut b = baue_wav(1, 48_000, &[1000, 2000]);
        let mut list = Vec::new();
        list.extend_from_slice(b"LIST");
        list.extend_from_slice(&3u32.to_le_bytes());
        list.extend_from_slice(b"abc\0"); // 3 Byte Inhalt + 1 Fuellbyte
        b.splice(12..12, list);
        let w = Wav::from_bytes(&b).expect("WAV mit ungeradem Chunk");
        assert_eq!(w.samples.len(), 2, "der data-Chunk wurde verfehlt");
    }

    #[test]
    fn weist_fremde_formate_ab() {
        assert_eq!(Wav::from_bytes(b"nicht mal ein Kopf").unwrap_err(), WavError::KeinRiffWave);
        assert_eq!(Wav::from_bytes(b"kurz").unwrap_err(), WavError::ZuKurz);
        let mut b = baue_wav(1, 48_000, &[0]);
        b[34] = 24; // Bits je Wert auf 24 setzen
        assert!(matches!(Wav::from_bytes(&b).unwrap_err(), WavError::NichtPcm16(_, 24)));
    }

    #[test]
    // Das Resampling: zwischen zwei Abtastwerten wird linear interpoliert. Genau in der Mitte
    // muss der Mittelwert herauskommen.
    fn interpoliert_zwischen_abtastwerten() {
        let w = Wav::from_bytes(&baue_wav(1, 2, &[0, 32_767])).expect("WAV");
        assert!((w.sample_at(0.0) - 0.0).abs() < 1e-3);
        assert!((w.sample_at(0.25) - 0.5).abs() < 1e-2, "{}", w.sample_at(0.25));
        assert!((w.sample_at(0.5) - 1.0).abs() < 1e-2);
        assert_eq!(w.sample_at(-1.0), 0.0, "vor dem Anfang ist Stille");
        assert_eq!(w.sample_at(99.0), 0.0, "nach dem Ende ist Stille");
        assert!((w.duration() - 1.0).abs() < 1e-6);
    }
}
