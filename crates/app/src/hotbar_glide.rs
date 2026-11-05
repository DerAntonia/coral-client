// Hotbar > Auswahl gleitet: der Auswahlrahmen gleitet in Settings::hotbar_glide_ms zum neuen
// Slot, statt zu springen. Blaettert das Mausrad ueber den Rand (9 -> 1 oder 1 -> 9), faehrt er
// auf der einen Seite hinaus und kommt auf der anderen herein; Zifferntasten nehmen den
// direkten Weg. Die Lage zaehlt in Slots und darf dafuer ueber 0..9 hinausgehen.

use crate::Instant;

#[derive(Default)]
pub(crate) struct HotbarGlide {
    // Der Slot, zu dem gerade geglitten wird; None = noch nie gezeichnet.
    target: Option<usize>,
    from: f32,
    to: f32,
    start: Option<Instant>,
    // Mausrad-Schritte seit dem letzten Bild (s. scrolled).
    pending: i32,
}

impl HotbarGlide {
    // Das Mausrad hat um einen Slot weitergeblaettert (-1 nach links, +1 nach rechts).
    pub(crate) fn scrolled(&mut self, step: i32) {
        self.pending += step;
    }

    // Ein Bild weiter; Rueckgabe: wo der Rahmen steht, in Slots.
    pub(crate) fn step(&mut self, held: usize, now: Instant, ms: u32) -> f32 {
        let schritte = std::mem::take(&mut self.pending);
        let Some(alt) = self.target else {
            self.target = Some(held);
            (self.from, self.to) = (held as f32, held as f32);
            return self.to;
        };
        let jetzt = self.shown(now, ms);
        if alt != held {
            let held_f = held as f32;
            // Mit dem Rad ueber den Rand: im selben Zug weiter; sonst der direkte Weg in der
            // Runde, in der der Rahmen gerade steht.
            let gerollt = self.to + schritte as f32;
            self.to = if schritte != 0 && gerollt.rem_euclid(9.0) == held_f { gerollt } else { held_f + 9.0 * (jetzt / 9.0).floor() };
            self.from = jetzt;
            self.start = Some(now);
            self.target = Some(held);
        }
        let jetzt = self.shown(now, ms);
        // Angekommen: zurueck in 0..9, damit die naechste Runde dort anfaengt.
        if jetzt == self.to {
            self.to = self.to.rem_euclid(9.0);
            self.from = self.to;
            self.start = None;
            return self.to;
        }
        jetzt
    }

    fn shown(&self, now: Instant, ms: u32) -> f32 {
        let Some(start) = self.start.filter(|_| ms > 0) else { return self.to };
        let t = (now.saturating_duration_since(start).as_secs_f32() * 1000.0 / ms as f32).min(1.0);
        if t >= 1.0 {
            return self.to;
        }
        self.from + (self.to - self.from) * (1.0 - (1.0 - t).powi(3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn gleitet_zum_neuen_slot() {
        let t0 = Instant::now();
        let mut g = HotbarGlide::default();
        assert_eq!(g.step(2, t0, 100), 2.0);
        let mitte = g.step(4, t0, 100);
        assert_eq!(mitte, 2.0);
        let x = g.step(4, t0 + Duration::from_millis(50), 100);
        assert!(x > 3.0 && x < 4.0);
        assert_eq!(g.step(4, t0 + Duration::from_millis(100), 100), 4.0);
    }

    #[test]
    fn ohne_dauer_springt_er() {
        let t0 = Instant::now();
        let mut g = HotbarGlide::default();
        g.step(0, t0, 0);
        assert_eq!(g.step(5, t0, 0), 5.0);
    }

    // Rad von 9 nach 1: nach rechts hinaus (ueber 8 hinaus), danach wieder bei 0.
    #[test]
    fn rad_ueber_den_rand() {
        let t0 = Instant::now();
        let mut g = HotbarGlide::default();
        g.step(8, t0, 100);
        g.scrolled(1);
        g.step(0, t0, 100);
        let x = g.step(0, t0 + Duration::from_millis(50), 100);
        assert!(x > 8.0 && x < 9.0);
        assert_eq!(g.step(0, t0 + Duration::from_millis(100), 100), 0.0);
    }

    // Zifferntaste von 1 nach 9: der direkte Weg nach rechts, kein Umlauf.
    #[test]
    fn zifferntaste_direkt() {
        let t0 = Instant::now();
        let mut g = HotbarGlide::default();
        g.step(0, t0, 100);
        g.step(8, t0, 100);
        let x = g.step(8, t0 + Duration::from_millis(50), 100);
        assert!(x > 0.0 && x < 8.0);
    }
}
