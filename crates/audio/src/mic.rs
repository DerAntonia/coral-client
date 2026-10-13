// Mikrofon-Aufnahme fuer den Sprachchat: Eingabegeraet oeffnen, auf Mono mischen, auf
// 48 kHz umrechnen und in Rahmen zu 960 Samples (20 ms) an den Aufrufer geben -- genau das
// Format, das Opus und das Sprachchat-Protokoll erwarten.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;

use crate::AudioError;

pub const FRAME: usize = 960;
const RATE: f64 = 48_000.0;

// Lineare Umrechnung auf 48 kHz, mit Zustand ueber Callback-Grenzen hinweg.
struct Resampler {
    // Schritt in Eingangs-Samples je Ausgangs-Sample.
    step: f64,
    pos: f64,
    last: f32,
}

impl Resampler {
    fn new(input_rate: f64) -> Resampler {
        Resampler { step: input_rate / RATE, pos: 0.0, last: 0.0 }
    }

    // Mono-Eingang -> 48-kHz-Samples in `out`.
    fn feed(&mut self, input: &[f32], out: &mut Vec<f32>) {
        // pos zaehlt relativ zum ersten Sample dieses Blocks; -1 waere `last`.
        while self.pos < input.len() as f64 {
            let i = self.pos.floor() as isize;
            let t = (self.pos - i as f64) as f32;
            let a = if i < 0 { self.last } else { input[i as usize] };
            let b = input.get((i + 1) as usize).copied().unwrap_or(a);
            out.push(a + (b - a) * t);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        if let Some(&l) = input.last() {
            self.last = l;
        }
    }
}

// Sammelt umgerechnete Samples und gibt volle Rahmen weiter.
struct Framer<F: FnMut(&[i16])> {
    resampler: Resampler,
    mono: Vec<f32>,
    rate48: Vec<f32>,
    frame: Vec<i16>,
    on_frame: F,
}

impl<F: FnMut(&[i16])> Framer<F> {
    fn feed(&mut self, interleaved: impl Iterator<Item = f32>, channels: usize) {
        self.mono.clear();
        let mut summe = 0.0;
        for (i, s) in interleaved.enumerate() {
            summe += s;
            if (i + 1) % channels == 0 {
                self.mono.push(summe / channels as f32);
                summe = 0.0;
            }
        }
        self.rate48.clear();
        self.resampler.feed(&self.mono, &mut self.rate48);
        for &s in &self.rate48 {
            self.frame.push((s * 32767.0).clamp(-32768.0, 32767.0) as i16);
            if self.frame.len() == FRAME {
                (self.on_frame)(&self.frame);
                self.frame.clear();
            }
        }
    }
}

pub fn list_input_devices() -> Vec<String> {
    cpal::default_host().input_devices().map(|ds| ds.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

// Ein offenes Mikrofon; zu, sobald es fallen gelassen wird.
pub struct Microphone {
    _stream: cpal::Stream,
    pub device_name: String,
}

impl Microphone {
    // `on_frame` laeuft im Audio-Thread des Systems -- dort nur weiterreichen (z.B. in einen
    // Kanal), nicht kodieren oder blockieren.
    pub fn open(device: Option<&str>, on_frame: impl FnMut(&[i16]) + Send + 'static) -> Result<Microphone, AudioError> {
        let host = cpal::default_host();
        let device = match device {
            Some(name) => host
                .input_devices()
                .map_err(|_| AudioError::NoOutputDevice)?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| AudioError::DeviceNotFound(name.to_string()))?,
            None => host.default_input_device().ok_or(AudioError::NoOutputDevice)?,
        };
        let supported = device.default_input_config().map_err(|e| AudioError::BuildStream(e.to_string()))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let channels = config.channels.max(1) as usize;
        let mut framer = Framer {
            resampler: Resampler::new(config.sample_rate.0 as f64),
            mono: Vec::with_capacity(4096),
            rate48: Vec::with_capacity(4096),
            frame: Vec::with_capacity(FRAME),
            on_frame,
        };
        let err_fn = |e| eprintln!("[audio] Mikrofon-Fehler: {e}");
        let stream = match format {
            SampleFormat::F32 => device.build_input_stream(&config, move |d: &[f32], _| framer.feed(d.iter().copied(), channels), err_fn, None),
            SampleFormat::I16 => {
                device.build_input_stream(&config, move |d: &[i16], _| framer.feed(d.iter().map(|s| *s as f32 / 32768.0), channels), err_fn, None)
            }
            SampleFormat::U16 => device.build_input_stream(
                &config,
                move |d: &[u16], _| framer.feed(d.iter().map(|s| (*s as f32 - 32768.0) / 32768.0), channels),
                err_fn,
                None,
            ),
            other => return Err(AudioError::BuildStream(format!("nicht unterstuetztes Sample-Format {other:?}"))),
        }
        .map_err(|e| AudioError::BuildStream(e.to_string()))?;
        stream.play().map_err(|e| AudioError::PlayStream(e.to_string()))?;
        Ok(Microphone { _stream: stream, device_name: device.name().unwrap_or_default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // 44,1 kHz Stereo hinein: eine Sekunde ergibt 50 Rahmen zu 960, und ein Gleichanteil
    // bleibt ein Gleichanteil (Mischung und Interpolation verfaelschen den Pegel nicht).
    fn rahmen_aus_44k_stereo() {
        let mut rahmen = Vec::new();
        let mut f = Framer {
            resampler: Resampler::new(44_100.0),
            mono: Vec::new(),
            rate48: Vec::new(),
            frame: Vec::new(),
            on_frame: |r: &[i16]| rahmen.push(r.to_vec()),
        };
        // In ungeraden Bloecken, wie sie aus dem Geraet kommen.
        let block = vec![0.25f32; 2 * 441];
        for _ in 0..100 {
            f.feed(block.iter().copied(), 2);
        }
        drop(f);
        assert!((49..=50).contains(&rahmen.len()), "{}", rahmen.len());
        assert!(rahmen.iter().flatten().skip(1).all(|s| (*s - 8191).abs() <= 1), "{:?}", &rahmen[0][..4]);
    }
}
