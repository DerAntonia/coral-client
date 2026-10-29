//! Gleitender Bildlauf fuer die Listen der Oberflaeche. Das Mausrad verschiebt das Ziel, die
//! Anzeige gleitet hinterher (Rate aus "Trägheit beim Scrollen"). Wie in osu! geht es ueber den
//! Rand hinaus, aber gegen Widerstand: draussen bringt eine Einheit Eingabe nur
//! 1 / (2 W + d²) Einheiten Weg -- W der gewoehnliche Widerstand in der Liste (1), d wie weit man
//! schon draussen ist, in Rasten. Danach federt das Ziel zum Rand zurueck.
//!
//! Einheiten sind die der Liste (Zeilen, Karten, Pixel); `raste` sagt, wie viele davon eine
//! Mausrad-Raste ohne Scrollweite ausmacht, damit der Widerstand ueberall gleich wirkt.

/// Gewoehnlicher Widerstand innerhalb der Liste.
const WIDERSTAND: f64 = 1.0;
/// Wie schnell ein Ziel jenseits des Rands zum Rand zurueckfedert (Rate je Sekunde).
const RUECKFEDER: f32 = 12.0;
/// Naeher als das (in Einheiten) gilt als angekommen.
const RUHE: f32 = 0.002;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SmoothScroll {
    /// Wohin das Mausrad will; ausserhalb von 0..=max, solange man ueber den Rand gezogen hat.
    pub ziel: f32,
    /// Was gezeichnet wird.
    pub stand: f32,
}

// Weg in Rasten, den man fuer die Tiefe `d` jenseits des Rands braucht: das Integral von
// 2 W + d².
fn tiefe_zu_weg(d: f64) -> f64 {
    2.0 * WIDERSTAND * d + d * d * d / 3.0
}

// Umkehrung von tiefe_zu_weg: die reelle Loesung von d³ + 6 W d - 3 y = 0 (Cardano).
fn weg_zu_tiefe(y: f64) -> f64 {
    let p3 = 2.0 * WIDERSTAND;
    let s = (2.25 * y * y + p3 * p3 * p3).sqrt();
    (1.5 * y + s).cbrt() + (1.5 * y - s).cbrt()
}

impl SmoothScroll {
    pub fn at(v: f32) -> Self {
        SmoothScroll { ziel: v, stand: v }
    }

    /// Erste (ganz oder teilweise) sichtbare Einheit beim gezeichneten Stand.
    pub fn first(&self) -> usize {
        self.stand.max(0.0).floor() as usize
    }

    /// Das Ziel, in die Liste geklemmt -- was gespeichert wird und wovon Tastatur-Schritte ausgehen.
    pub fn target(&self, max: f32) -> f32 {
        self.ziel.clamp(0.0, max.max(0.0))
    }

    /// Sofort dorthin, ohne Gleiten.
    pub fn jump(&mut self, v: f32) {
        *self = SmoothScroll::at(v);
    }

    /// Gleitend dorthin (in die Liste geklemmt).
    pub fn glide_to(&mut self, v: f32, max: f32) {
        self.ziel = v.clamp(0.0, max.max(0.0));
    }

    /// Um `weg` Einheiten Richtung Ende (negativ: Richtung Anfang). Jenseits des Rands gegen den
    /// Widerstand, `raste` Einheiten je Raste.
    pub fn push(&mut self, weg: f32, raste: f32, max: f32) {
        let max = max.max(0.0) as f64;
        let raste = raste.max(1e-3) as f64;
        let mut z = self.ziel as f64;
        let mut rest = weg as f64;
        // Hoechstens: zurueck in die Liste, durch sie hindurch, am anderen Ende hinaus.
        for _ in 0..3 {
            if rest == 0.0 {
                break;
            }
            let rand = z.clamp(0.0, max);
            let draussen = z - rand;
            if draussen == 0.0 {
                let neu = z + rest;
                if (0.0..=max).contains(&neu) {
                    z = neu;
                    break;
                }
                // Bis zum Rand ohne Widerstand, der Rest dahinter.
                let kante = if rest > 0.0 { max } else { 0.0 };
                rest -= kante - z;
                z = kante + rest.signum() * weg_zu_tiefe(rest.abs() / raste) * raste;
                break;
            }
            let richtung = draussen.signum();
            let y = tiefe_zu_weg(draussen.abs() / raste) + richtung * rest / raste;
            if y >= 0.0 {
                z = rand + richtung * weg_zu_tiefe(y) * raste;
                break;
            }
            // Ueber den Rand zurueck: was uebrig ist, geht in die Liste.
            z = rand;
            rest = -richtung * -y * raste;
        }
        self.ziel = z as f32;
    }

    /// Ein Bild weiter: ein Ziel jenseits des Rands federt zurueck, der Stand gleitet mit `rate`
    /// (je Sekunde, unendlich = sofort) zum Ziel. Ein geschrumpftes `max` zieht das Ziel mit.
    pub fn tick(&mut self, dt: f32, rate: f32, max: f32) {
        let rand = self.ziel.clamp(0.0, max.max(0.0));
        if self.ziel != rand {
            self.ziel = rand + (self.ziel - rand) * (-dt * RUECKFEDER).exp();
            if (self.ziel - rand).abs() < RUHE {
                self.ziel = rand;
            }
        }
        if !rate.is_finite() {
            self.stand = self.ziel;
            return;
        }
        self.stand += (self.ziel - self.stand) * (1.0 - (-dt * rate).exp());
        if (self.ziel - self.stand).abs() < RUHE {
            self.stand = self.ziel;
        }
    }
}

/// So weit (Bildschirmpixel) muss der Zeiger bei gedrueckter Taste wandern, bis aus dem Druck
/// ein Ziehen wird; darunter bleibt er ein Klick (wie in osu!).
pub const ZIEH_SCHWELLE: f32 = 10.0;
/// Die Bewegung der letzten so vielen Sekunden gibt den Schwung beim Loslassen.
const SCHWUNG_FENSTER: f32 = 0.05;

/// Linksziehen in einer Liste: der Inhalt haengt am Zeiger, ueber den Rand gegen denselben
/// Widerstand wie beim Mausrad, beim Loslassen gleitet er mit dem letzten Tempo weiter.
#[derive(Clone, Debug)]
pub struct DragGrip {
    /// Wo gedrueckt wurde (Bildschirm).
    pub press: (f32, f32),
    /// Schon ueber ZIEH_SCHWELLE hinaus: ab dann kein Klick mehr.
    pub moved: bool,
    /// Gezogener Stand, ungeklemmt.
    pos: f32,
    last_y: f32,
    /// (Zeitpunkt, Stand) der juengsten Bewegungen, fuer den Schwung.
    spur: Vec<(std::time::Instant, f32)>,
}

impl DragGrip {
    pub fn new(press: (f32, f32)) -> Self {
        DragGrip { press, moved: false, pos: 0.0, last_y: press.1, spur: Vec::new() }
    }

    /// Zeiger bewegt: erst die Schwelle, danach folgt die Liste dem Zeiger (`px` Pixel je
    /// Einheit, Widerstand jenseits des Rands wie SmoothScroll::push mit `raste`).
    pub fn motion(&mut self, cursor: (f32, f32), jetzt: std::time::Instant, s: &mut SmoothScroll, px: f32, raste: f32, max: f32) {
        if !self.moved {
            if (cursor.0 - self.press.0).hypot(cursor.1 - self.press.1) <= ZIEH_SCHWELLE {
                return;
            }
            // Gepackt: was gerade noch gleitet, bleibt stehen, wo es ist.
            self.moved = true;
            self.pos = s.stand;
            self.last_y = cursor.1;
            self.spur.push((jetzt, self.pos));
            *s = SmoothScroll::at(self.pos);
            return;
        }
        let mut z = SmoothScroll::at(self.pos);
        z.push((self.last_y - cursor.1) / px.max(1e-3), raste, max);
        self.last_y = cursor.1;
        self.pos = z.ziel;
        self.spur.retain(|&(t, _)| jetzt.duration_since(t).as_secs_f32() <= SCHWUNG_FENSTER);
        self.spur.push((jetzt, self.pos));
        *s = SmoothScroll::at(self.pos);
    }

    /// Je Bild nach SmoothScroll::tick: solange gezogen wird, federt und gleitet nichts.
    pub fn hold(&self, s: &mut SmoothScroll) {
        if self.moved {
            *s = SmoothScroll::at(self.pos);
        }
    }

    /// Losgelassen: mit dem Tempo der letzten SCHWUNG_FENSTER Sekunden weiter, so weit, dass
    /// das Gleiten mit `rate` (s. SmoothScroll::tick) genau dieses Tempo anfaengt -- in die
    /// Liste geklemmt. Ohne Traegheit (rate unendlich) oder jenseits des Rands kein Schwung.
    pub fn release(&self, jetzt: std::time::Instant, s: &mut SmoothScroll, rate: f32, max: f32) {
        if !self.moved {
            return;
        }
        let max = max.max(0.0);
        let tempo = match self.spur.iter().find(|&&(t, _)| jetzt.duration_since(t).as_secs_f32() <= SCHWUNG_FENSTER) {
            Some(&(t, p)) => {
                let dt = jetzt.duration_since(t).as_secs_f32();
                if dt > 1e-3 { (self.pos - p) / dt } else { 0.0 }
            }
            None => 0.0,
        };
        let ziel = if rate.is_finite() && (0.0..=max).contains(&self.pos) { (self.pos + tempo / rate).clamp(0.0, max) } else { self.pos };
        *s = SmoothScroll { ziel, stand: self.pos };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn ziehen_erst_nach_der_schwelle() {
        let t = Instant::now();
        let mut s = SmoothScroll { ziel: 4.0, stand: 3.0 };
        let mut g = DragGrip::new((100.0, 100.0));
        g.motion((100.0, 95.0), t, &mut s, 10.0, 1.0, 10.0);
        assert!(!g.moved);
        assert_eq!((s.ziel, s.stand), (4.0, 3.0));
        // Ueber die Schwelle: gepackt, wo die Liste gerade steht.
        g.motion((100.0, 80.0), t, &mut s, 10.0, 1.0, 10.0);
        assert!(g.moved);
        assert_eq!((s.ziel, s.stand), (3.0, 3.0));
        // Zeiger 20 px hoch = Inhalt 2 Einheiten weiter Richtung Ende.
        g.motion((100.0, 60.0), t, &mut s, 10.0, 1.0, 10.0);
        assert_eq!((s.ziel, s.stand), (5.0, 5.0));
    }

    #[test]
    fn ziehen_ueber_den_rand_gegen_widerstand() {
        let t = Instant::now();
        let mut s = SmoothScroll::at(0.0);
        let mut g = DragGrip::new((0.0, 0.0));
        g.motion((0.0, 20.0), t, &mut s, 1.0, 1.0, 10.0);
        g.motion((0.0, 21.0), t, &mut s, 1.0, 1.0, 10.0);
        // Wie eine Raste Mausrad ueber den oberen Rand.
        let mut rad = SmoothScroll::at(0.0);
        rad.push(-1.0, 1.0, 10.0);
        assert_eq!(s.ziel, rad.ziel);
        // Gehalten federt nichts zurueck.
        s.tick(0.1, 16.0, 10.0);
        g.hold(&mut s);
        assert_eq!((s.ziel, s.stand), (rad.ziel, rad.ziel));
    }

    #[test]
    fn schwung_beim_loslassen() {
        let t = Instant::now();
        let mut s = SmoothScroll::at(5.0);
        let mut g = DragGrip::new((0.0, 0.0));
        g.motion((0.0, -20.0), t, &mut s, 1.0, 1.0, 100.0);
        g.motion((0.0, -21.0), t + Duration::from_millis(10), &mut s, 1.0, 1.0, 100.0);
        g.motion((0.0, -41.0), t + Duration::from_millis(30), &mut s, 1.0, 1.0, 100.0);
        // 21 Einheiten in 30 ms = 700 je Sekunde; bei Rate 16 gleitet es 43,75 weiter.
        g.release(t + Duration::from_millis(30), &mut s, 16.0, 100.0);
        assert_eq!(s.stand, 26.0);
        assert!((s.ziel - (26.0 + 700.0 / 16.0)).abs() < 0.01, "{}", s.ziel);
        // Lange stillgehalten: kein Schwung; ohne Traegheit auch nicht.
        g.release(t + Duration::from_millis(500), &mut s, 16.0, 100.0);
        assert_eq!((s.ziel, s.stand), (26.0, 26.0));
        g.release(t + Duration::from_millis(30), &mut s, f32::INFINITY, 100.0);
        assert_eq!((s.ziel, s.stand), (26.0, 26.0));
    }

    #[test]
    fn innen_ohne_widerstand() {
        let mut s = SmoothScroll::at(2.0);
        s.push(3.0, 1.0, 10.0);
        assert_eq!(s.ziel, 5.0);
        s.push(-5.0, 1.0, 10.0);
        assert_eq!(s.ziel, 0.0);
    }

    #[test]
    fn draussen_nach_formel() {
        // Kleine Schritte gegen 1 / (2 + d²) aufsummiert muessen dasselbe ergeben.
        let mut naeherung = 0.0f64;
        for _ in 0..100_000 {
            naeherung += 3.0 / 100_000.0 / (2.0 + naeherung * naeherung);
        }
        let mut s = SmoothScroll::at(10.0);
        s.push(3.0, 1.0, 10.0);
        assert!((s.ziel as f64 - 10.0 - naeherung).abs() < 1e-3, "{} gegen {}", s.ziel, 10.0 + naeherung);
        // Die erste Raste draussen bringt knapp eine halbe.
        let mut s = SmoothScroll::at(0.0);
        s.push(-1.0, 1.0, 10.0);
        assert!(s.ziel < -0.4 && s.ziel > -0.5, "{}", s.ziel);
    }

    #[test]
    fn raste_skaliert_den_widerstand() {
        let mut zeilen = SmoothScroll::at(0.0);
        zeilen.push(-2.0, 1.0, 5.0);
        let mut pixel = SmoothScroll::at(0.0);
        pixel.push(-80.0, 40.0, 200.0);
        assert!((zeilen.ziel * 40.0 - pixel.ziel).abs() < 1e-3);
    }

    #[test]
    fn zurueck_ueber_den_rand_in_die_liste() {
        let mut s = SmoothScroll::at(10.0);
        s.push(2.0, 1.0, 10.0);
        s.push(-4.0, 1.0, 10.0);
        // 2 hinaus, 2 zurueck heben sich auf, die anderen 2 gehen in die Liste.
        assert!((s.ziel - 8.0).abs() < 1e-4, "{}", s.ziel);
    }

    #[test]
    fn federt_zurueck_und_gleitet() {
        let mut s = SmoothScroll::at(0.0);
        s.push(-2.0, 1.0, 10.0);
        for _ in 0..240 {
            s.tick(1.0 / 60.0, 16.0, 10.0);
        }
        assert_eq!((s.ziel, s.stand), (0.0, 0.0));
        let mut s = SmoothScroll::at(0.0);
        s.push(4.0, 1.0, 10.0);
        s.tick(1.0 / 60.0, f32::INFINITY, 10.0);
        assert_eq!(s.stand, 4.0);
    }
}
