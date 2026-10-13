// Aufbereitung des eigenen Mikrofons vor dem Senden, mit denselben Stellschrauben wie die
// Mod: Verstaerkung in dB, automatische Verstaerkung (AGC), Rauschunterdrueckung (RNNoise),
// und die Entscheidung, ob gerade gesendet wird -- per Sprechtaste oder Sprachaktivierung,
// jeweils mit Nachlauf, damit Wortenden nicht abgeschnitten werden.

use nnnoiseless::DenoiseState;

use crate::packets::FRAME_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    PushToTalk,
    Voice,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MicSettings {
    pub gain_db: f32,
    pub agc: bool,
    pub denoise: bool,
    pub activation: Activation,
    // Schwelle der Sprachaktivierung in dBFS (-127 .. 0).
    pub threshold_db: f32,
    // Nachlauf in Rahmen (20 ms) nach dem Unterschreiten der Schwelle bzw. Loslassen der Taste.
    pub voice_delay_frames: u32,
    pub ptt_delay_frames: u32,
}

// Pegel eines Rahmens in dBFS (RMS); -127 fuer Stille.
pub fn level_db(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return -127.0;
    }
    let sum: f64 = pcm.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / pcm.len() as f64).sqrt() / 32768.0;
    if rms <= 0.0 {
        return -127.0;
    }
    (20.0 * rms.log10()).max(-127.0) as f32
}

// Ziel der automatischen Verstaerkung und ihre Grenzen.
const AGC_TARGET_DB: f32 = -18.0;
const AGC_MAX_GAIN_DB: f32 = 24.0;
// Unter diesem Pegel ist es Rauschen -- dann wird die Verstaerkung nicht hochgezogen.
const AGC_NOISE_FLOOR_DB: f32 = -55.0;

pub struct MicProcessor {
    denoiser: Option<Box<DenoiseState<'static>>>,
    agc_gain_db: f32,
    // Rahmen seit dem letzten "aktiv" (fuer den Nachlauf); u32::MAX = lange her.
    since_active: u32,
}

impl Default for MicProcessor {
    fn default() -> Self {
        MicProcessor { denoiser: None, agc_gain_db: 0.0, since_active: u32::MAX }
    }
}

fn apply_gain_db(pcm: &mut [i16], db: f32) {
    if db.abs() < 0.01 {
        return;
    }
    let f = 10f32.powf(db / 20.0);
    for s in pcm {
        *s = (*s as f32 * f).clamp(-32768.0, 32767.0) as i16;
    }
}

impl MicProcessor {
    // Einen Rahmen (960 Samples) aufbereiten. Rueckgabe: Pegel danach in dBFS.
    pub fn process(&mut self, pcm: &mut [i16], s: &MicSettings) -> f32 {
        apply_gain_db(pcm, s.gain_db);
        if s.denoise && pcm.len() == FRAME_SIZE {
            let d = self.denoiser.get_or_insert_with(DenoiseState::new);
            let mut ein = [0f32; DenoiseState::FRAME_SIZE];
            let mut aus = [0f32; DenoiseState::FRAME_SIZE];
            for haelfte in pcm.chunks_mut(DenoiseState::FRAME_SIZE) {
                for (e, &x) in ein.iter_mut().zip(haelfte.iter()) {
                    *e = x as f32;
                }
                d.process_frame(&mut aus, &ein);
                for (x, &a) in haelfte.iter_mut().zip(aus.iter()) {
                    *x = a.clamp(-32768.0, 32767.0) as i16;
                }
            }
        } else if !s.denoise {
            self.denoiser = None;
        }
        if s.agc {
            let pegel = level_db(pcm);
            if pegel > AGC_NOISE_FLOOR_DB {
                // Langsam zum Ziel; nach unten schneller als nach oben (kein Pumpen bei lauten
                // Spitzen).
                let soll = (AGC_TARGET_DB - pegel).clamp(-AGC_MAX_GAIN_DB, AGC_MAX_GAIN_DB);
                let tempo = if soll < self.agc_gain_db { 0.3 } else { 0.05 };
                self.agc_gain_db += (soll - self.agc_gain_db) * tempo;
            }
            apply_gain_db(pcm, self.agc_gain_db);
        } else {
            self.agc_gain_db = 0.0;
        }
        level_db(pcm)
    }

    // Wird dieser Rahmen gesendet? `key_held` = Sprechtaste gedrueckt, `level` aus process.
    pub fn active(&mut self, s: &MicSettings, key_held: bool, level: f32) -> bool {
        let (an, nachlauf) = match s.activation {
            Activation::PushToTalk => (key_held, s.ptt_delay_frames),
            Activation::Voice => (level >= s.threshold_db, s.voice_delay_frames),
        };
        if an {
            self.since_active = 0;
            return true;
        }
        self.since_active = self.since_active.saturating_add(1);
        self.since_active <= nachlauf
    }

    pub fn reset_activation(&mut self) {
        self.since_active = u32::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn einstellungen() -> MicSettings {
        MicSettings {
            gain_db: 0.0,
            agc: false,
            denoise: false,
            activation: Activation::Voice,
            threshold_db: -50.0,
            voice_delay_frames: 2,
            ptt_delay_frames: 1,
        }
    }

    #[test]
    fn pegel_in_dbfs() {
        assert_eq!(level_db(&[0; 960]), -127.0);
        // Vollaussteuerung als Rechteck: 0 dB.
        assert!(level_db(&[32767; 960]).abs() < 0.01);
        assert!((level_db(&[3277; 960]) + 20.0).abs() < 0.1);
    }

    #[test]
    fn sprachaktivierung_mit_nachlauf() {
        let mut p = MicProcessor::default();
        let s = einstellungen();
        assert!(!p.active(&s, false, -60.0));
        assert!(p.active(&s, false, -30.0));
        // Zwei Rahmen Nachlauf, dann aus.
        assert!(p.active(&s, false, -60.0));
        assert!(p.active(&s, false, -60.0));
        assert!(!p.active(&s, false, -60.0));
    }

    #[test]
    fn sprechtaste_mit_nachlauf() {
        let mut p = MicProcessor::default();
        let s = MicSettings { activation: Activation::PushToTalk, ..einstellungen() };
        assert!(!p.active(&s, false, 0.0), "Pegel zaehlt bei der Sprechtaste nicht");
        assert!(p.active(&s, true, -127.0));
        assert!(p.active(&s, false, -127.0));
        assert!(!p.active(&s, false, -127.0));
    }

    #[test]
    fn verstaerkung_und_agc() {
        let mut p = MicProcessor::default();
        let mut r = [1000i16; 960];
        p.process(&mut r, &MicSettings { gain_db: 6.0206, ..einstellungen() });
        assert!((r[0] - 2000).abs() <= 1);
        // AGC zieht einen leisen Ton ueber einige Rahmen Richtung Ziel.
        let s = MicSettings { agc: true, ..einstellungen() };
        let mut letzter = -127.0;
        for _ in 0..60 {
            let mut r: Vec<i16> = (0..960).map(|i| ((i as f32 * 0.1).sin() * 1500.0) as i16).collect();
            letzter = p.process(&mut r, &s);
        }
        assert!((letzter - AGC_TARGET_DB).abs() < 3.0, "{letzter}");
    }

    #[test]
    fn rauschunterdrueckung_daempft_rauschen() {
        let mut p = MicProcessor::default();
        let s = MicSettings { denoise: true, ..einstellungen() };
        let mut x: u32 = 1;
        let mut zuletzt = 0.0;
        for _ in 0..50 {
            let mut r: Vec<i16> = (0..960)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    ((x % 2000) as i32 - 1000) as i16
                })
                .collect();
            let vorher = level_db(&r);
            let nachher = p.process(&mut r, &s);
            zuletzt = vorher - nachher;
        }
        assert!(zuletzt > 10.0, "Rauschen nur um {zuletzt} dB gedaempft");
    }
}
