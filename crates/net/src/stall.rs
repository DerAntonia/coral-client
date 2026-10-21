//! Stockungen im Paketfluss erkennen -- das, was man im Spiel als Rubberbanding spuert (s.
//! ClientEvent::PacketStall). Frei von Netz- und Zeitquellen: bekommt Zeitpunkte, liefert die
//! Laenge einer erkannten Stockung.
//!
//! Eingehend (I): erst lange nichts, dann viele Pakete auf einmal -- der Server (oder die
//! Leitung) hat gestockt und schiebt alles nach. Eine Pause ALLEIN reicht nicht: ein ruhiger
//! Server ohne Entities in der Naehe schickt auch mal eine Sekunde nichts, danach aber nur
//! einzelne Pakete.
//!
//! Ausgehend (O): Client Tick End geht in jedem Tick raus (s. packets::SB_PLAY_CLIENT_TICK_END)
//! -- eine grosse Luecke dazwischen heisst, dass der Server so lange nichts von uns gehoert hat.

use std::time::{Duration, Instant};

/// Ab dieser Pause zaehlt ein anschliessender Schwall als eingehende Stockung.
pub const IN_STALL: Duration = Duration::from_millis(400);
/// So viele Pakete ...
pub const BURST_MIN: u32 = 6;
/// ... innerhalb dieser Zeit nach der Pause machen den Schwall aus.
pub const BURST_WINDOW: Duration = Duration::from_millis(50);
/// Ab dieser Luecke zwischen zwei Tick-Enden hat der Server zu lange nichts von uns gehoert
/// (sechs verpasste Ticks).
pub const OUT_STALL: Duration = Duration::from_millis(300);
/// Ein einzelner Schreibvorgang, der so lange haengt, staut alles hinter ihm.
pub const OUT_WRITE_STALL: Duration = Duration::from_millis(150);

#[derive(Default)]
pub struct StallWatch {
    last_in: Option<Instant>,
    // Laufender Schwall nach einer Pause: Beginn, Pausenlaenge, Pakete bisher.
    burst: Option<(Instant, Duration, u32)>,
    last_tick_end: Option<Instant>,
}

impl StallWatch {
    /// Ein Paket kam an. Liefert die Pausenlaenge, sobald der Schwall danach bestaetigt ist
    /// (einmal je Stockung).
    pub fn incoming(&mut self, now: Instant) -> Option<Duration> {
        if let Some(prev) = self.last_in {
            let pause = now.duration_since(prev);
            if pause >= IN_STALL {
                self.burst = Some((now, pause, 0));
            }
        }
        self.last_in = Some(now);
        let (start, pause, n) = self.burst.as_mut()?;
        if now.duration_since(*start) > BURST_WINDOW {
            self.burst = None;
            return None;
        }
        *n += 1;
        if *n < BURST_MIN {
            return None;
        }
        let pause = *pause;
        self.burst = None;
        Some(pause)
    }

    /// Client Tick End ging raus. Liefert die Luecke zum vorigen, wenn sie zu gross war.
    pub fn tick_end(&mut self, now: Instant) -> Option<Duration> {
        let luecke = self.last_tick_end.map(|p| now.duration_since(p));
        self.last_tick_end = Some(now);
        luecke.filter(|l| *l >= OUT_STALL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn pause_mit_schwall_ist_stockung() {
        let t0 = Instant::now();
        let mut w = StallWatch::default();
        assert_eq!(w.incoming(t0), None);
        let nach = t0 + ms(600);
        let treffer: Vec<_> = (0..BURST_MIN as u64).map(|i| w.incoming(nach + ms(i))).collect();
        assert_eq!(treffer.iter().filter(|t| t.is_some()).count(), 1);
        assert_eq!(treffer.last().copied().flatten(), Some(ms(600)));
    }

    #[test]
    fn pause_mit_einzelnen_paketen_ist_keine() {
        let t0 = Instant::now();
        let mut w = StallWatch::default();
        w.incoming(t0);
        // Ruhiger Server: nach der Pause nur alle 100 ms ein Paket.
        for i in 0..10 {
            assert_eq!(w.incoming(t0 + ms(800 + i * 100)), None);
        }
    }

    #[test]
    fn dichter_strom_ohne_pause_ist_keine() {
        let t0 = Instant::now();
        let mut w = StallWatch::default();
        for i in 0..100 {
            assert_eq!(w.incoming(t0 + ms(i * 5)), None);
        }
    }

    #[test]
    fn tick_luecke() {
        let t0 = Instant::now();
        let mut w = StallWatch::default();
        assert_eq!(w.tick_end(t0), None);
        assert_eq!(w.tick_end(t0 + ms(50)), None);
        assert_eq!(w.tick_end(t0 + ms(400)), Some(ms(350)));
    }
}
