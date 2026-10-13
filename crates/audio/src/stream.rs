// Laufende Tonstroeme (Sprachchat): anders als eine Aufnahme ist ein Strom nicht fertig, wenn
// er anfaengt -- die Rahmen kommen alle 20 ms aus dem Netz nach. Jeder Strom hat einen festen
// Ringpuffer (48 kHz, mono), der beim Anlegen des Mixers reserviert wird; im Audio-Callback
// wird nichts allokiert.
//
// Damit Netzschwankungen nicht als Knacken zu hoeren sind, faengt ein Strom erst an zu
// spielen, wenn drei Rahmen (60 ms) im Puffer liegen, und laeuft er leer, wartet er wieder auf
// diesen Vorrat. Ausgegeben wird mit linearer Interpolation auf die Rate des Geraets, mit Ort
// ueber das Ohrenmodell (s. spatial) oder kopffest.

use crate::spatial::{self, Placement};
use crate::Ears;

pub(crate) const MAX_STREAMS: usize = 16;
// Eine Sekunde Puffer: mehr Rueckstand als das ist ohnehin verloren.
const RING: usize = 48_000;
const STREAM_RATE: f32 = 48_000.0;
// Vorrat vor dem (Wieder-)Anlaufen, Standard (s. CueSink::set_stream_buffer).
pub(crate) const DEFAULT_START_FILL: usize = 3 * 960;

pub(crate) struct StreamSlot {
    pub id: u64,
    pub active: bool,
    ring: Box<[f32]>,
    write: usize,
    read: usize,
    avail: usize,
    // Bruchteil zwischen `read` und dem naechsten Sample.
    frac: f32,
    playing: bool,
    // Kein Nachschub mehr zu erwarten: leer spielen, dann frei machen.
    ending: bool,
    ears: Option<Ears>,
    gain: f32,
    gain_target: f32,
}

impl StreamSlot {
    pub fn new() -> StreamSlot {
        StreamSlot {
            id: 0,
            active: false,
            ring: vec![0.0; RING].into_boxed_slice(),
            write: 0,
            read: 0,
            avail: 0,
            frac: 0.0,
            playing: false,
            ending: false,
            ears: None,
            gain: 0.0,
            gain_target: 0.0,
        }
    }

    fn reset(&mut self, id: u64) {
        self.id = id;
        self.active = true;
        self.write = 0;
        self.read = 0;
        self.avail = 0;
        self.frac = 0.0;
        self.playing = false;
        self.ending = false;
        self.ears = None;
    }

    fn place(&mut self, placement: Option<&Placement>, gain: f32) {
        self.gain_target = gain.max(0.0);
        match (placement, self.ears.as_mut()) {
            (Some(p), Some(ears)) => ears.target = spatial::spatial_for(p),
            (Some(p), None) => self.ears = Some(Ears::new(spatial::spatial_for(p))),
            (None, _) => self.ears = None,
        }
    }

    fn push(&mut self, pcm: &[i16], start_fill: usize) {
        for &s in pcm {
            self.ring[self.write] = s as f32 / 32768.0;
            self.write = (self.write + 1) % RING;
            if self.avail == RING {
                // Voll: das Aelteste faellt heraus.
                self.read = (self.read + 1) % RING;
            } else {
                self.avail += 1;
            }
        }
        self.ending = false;
        if !self.playing && self.avail >= start_fill {
            self.playing = true;
        }
    }

    // Ein Ausgabe-Sample (links, rechts) bei Geraeterate `sample_rate`.
    pub fn next(&mut self, sample_rate: f32) -> (f32, f32) {
        if !self.playing {
            return (0.0, 0.0);
        }
        if self.avail < 2 {
            if self.ending {
                self.active = false;
            }
            self.playing = false;
            return (0.0, 0.0);
        }
        let a = self.ring[self.read];
        let b = self.ring[(self.read + 1) % RING];
        let wert = a + (b - a) * self.frac;
        self.frac += STREAM_RATE / sample_rate;
        while self.frac >= 1.0 && self.avail > 1 {
            self.frac -= 1.0;
            self.read = (self.read + 1) % RING;
            self.avail -= 1;
        }
        // Pegel gleitet (ca. 20 ms), damit Entfernungsaenderungen nicht knacken.
        self.gain += (self.gain_target - self.gain) * (1.0 - (-1.0 / (0.02 * sample_rate)).exp());
        let wert = wert * self.gain;
        match self.ears.as_mut() {
            Some(ears) => ears.hoere(wert, sample_rate),
            None => (wert * std::f32::consts::FRAC_1_SQRT_2, wert * std::f32::consts::FRAC_1_SQRT_2),
        }
    }
}

// Den Strom `id` finden oder einen freien Platz dafuer nehmen. None = alle Plaetze belegt.
pub(crate) fn slot_for(slots: &mut [StreamSlot], id: u64, anlegen: bool) -> Option<&mut StreamSlot> {
    if let Some(i) = slots.iter().position(|s| s.active && s.id == id) {
        return Some(&mut slots[i]);
    }
    if !anlegen {
        return None;
    }
    let i = slots.iter().position(|s| !s.active)?;
    slots[i].reset(id);
    Some(&mut slots[i])
}

pub(crate) fn push(slots: &mut [StreamSlot], id: u64, pcm: &[i16], placement: Option<&Placement>, gain: f32, start_fill: usize) {
    let Some(s) = slot_for(slots, id, true) else { return };
    let neu = s.avail == 0 && !s.playing;
    s.place(placement, gain);
    if neu {
        // Der erste Rahmen setzt den Pegel direkt, statt von 0 hochzugleiten.
        s.gain = s.gain_target;
    }
    s.push(pcm, start_fill);
}

pub(crate) fn place(slots: &mut [StreamSlot], id: u64, placement: Option<&Placement>, gain: f32) {
    if let Some(s) = slot_for(slots, id, false) {
        s.place(placement, gain);
    }
}

pub(crate) fn end(slots: &mut [StreamSlot], id: u64) {
    if let Some(s) = slot_for(slots, id, false) {
        s.ending = true;
        if s.avail > 0 {
            s.playing = true;
        } else {
            s.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spiele(slots: &mut [StreamSlot], n: usize, rate: f32) -> Vec<f32> {
        (0..n).map(|_| slots.iter_mut().filter(|s| s.active).map(|s| s.next(rate).0).sum()).collect()
    }

    #[test]
    // Erst nach drei Rahmen Vorrat hoerbar, dann leer gespielt und nach dem Ende frei.
    fn puffert_spielt_und_gibt_frei() {
        let mut slots: Vec<StreamSlot> = (0..2).map(|_| StreamSlot::new()).collect();
        let rahmen = vec![16384i16; 960];
        push(&mut slots, 7, &rahmen, None, 1.0, DEFAULT_START_FILL);
        push(&mut slots, 7, &rahmen, None, 1.0, DEFAULT_START_FILL);
        assert!(spiele(&mut slots, 100, 48_000.0).iter().all(|x| *x == 0.0), "noch kein Vorrat");
        push(&mut slots, 7, &rahmen, None, 1.0, DEFAULT_START_FILL);
        let aus = spiele(&mut slots, 100, 48_000.0);
        assert!(aus.iter().all(|x| (*x - 0.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01), "{:?}", &aus[..3]);
        end(&mut slots, 7);
        spiele(&mut slots, 3000, 48_000.0);
        assert!(!slots[0].active, "nach dem Ende frei");
    }

    #[test]
    // Bei 44,1 kHz dauern 2880 Samples Strom 2646 Ausgabe-Samples (Rate umgerechnet).
    fn rechnet_die_rate_um() {
        let mut slots = vec![StreamSlot::new()];
        push(&mut slots, 1, &vec![1000i16; 2880], None, 1.0, DEFAULT_START_FILL);
        end(&mut slots, 1);
        let mut n = 0;
        while slots[0].active && n < 10_000 {
            slots[0].next(44_100.0);
            n += 1;
        }
        assert!((2640..=2650).contains(&n), "{n}");
    }

    #[test]
    fn mehr_stroeme_als_plaetze_werden_verworfen() {
        let mut slots: Vec<StreamSlot> = (0..2).map(|_| StreamSlot::new()).collect();
        for id in 1..=3 {
            push(&mut slots, id, &[1; 960], None, 1.0, DEFAULT_START_FILL);
        }
        assert_eq!(slots.iter().filter(|s| s.active).count(), 2);
        assert!(slot_for(&mut slots, 3, false).is_none());
    }
}
