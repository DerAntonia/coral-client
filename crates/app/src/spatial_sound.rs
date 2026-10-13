//! Die Bruecke zwischen Welt und Ohrenmodell: aus Weltkoordinaten wird eine Kopflage.
//!
//! Die Psychoakustik selbst steht in `audio::spatial` und kennt weder Bloecke noch Kamera --
//! hier wird nur ausgerechnet, WO eine Quelle relativ zum Kopf steht.

use audio::spatial::Placement;
use glam::{DVec3, Vec3};

/// Wo der Kopf steht und wie er ausgerichtet ist.
pub struct Listener {
    /// Weltposition des Kopfes.
    pub position: DVec3,
    /// Einheitsvektor nach rechts aus Kopfsicht -- die Ohrachse
    /// (s. `render::Camera::right`).
    pub right: Vec3,
}

impl Listener {
    /// Der Kopf sitzt am AUGE DES SPIELERS und dreht sich mit dessen Blickrichtung -- auch
    /// dann, wenn die Kamera gerade woanders steht.
    ///
    /// Bewusst anders als Vanilla, das den Zuhoerer an die Kamera haengt: in der Frontansicht
    /// (F5 zweimal) blickt die Kamera dem Spieler entgegen, und Vanilla vertauscht damit
    /// links und rechts. Fuer eine huebsche Aussenansicht mag das stimmig sein; hier ist der
    /// Ton ein Spielsignal, und ein Signal darf seine Bedeutung nicht mit der Kameraeinstellung
    /// wechseln. Wer nach dem Geraeusch nach rechts dreht, soll immer richtig liegen.
    ///
    /// Das Nicken bleibt aussen vor, und zwar nicht aus Bequemlichkeit: eine Drehung um die
    /// Ohrachse bewegt die Ohrachse nicht. Genau deshalb reicht `right`, und genau deshalb
    /// aendert sich die Ortung nicht, wenn man beim Zuhoeren nach oben schaut -- so wie beim
    /// echten Kopf auch.
    pub fn at_eye(eye: DVec3, yaw_degrees: f32) -> Listener {
        let yaw = yaw_degrees.to_radians();
        // Blickrichtung waagerecht (Camera::forward bei pitch 0), dann forward x Y wie in
        // render::Camera::right -- hier direkt ausgeschrieben, damit die Ohrachse nicht
        // versehentlich an der VIEW-Richtung haengt (s. view_forward/Frontansicht).
        let forward = Vec3::new(-yaw.sin(), 0.0, yaw.cos());
        Listener { position: eye, right: forward.cross(Vec3::Y).normalize() }
    }

    /// Wo steht diese Quelle fuer meine Ohren?
    pub fn placement(&self, source: DVec3) -> Placement {
        let offset = source - self.position;
        let distance = offset.length();
        // Genau auf dem Kopf gibt es keine Richtung -- dann in die Mitte legen statt durch
        // null zu teilen (das gaebe NaN, und NaN im Mixer ist ein Dauerknall).
        if distance < 1e-6 {
            return Placement { lateral: 0.0, distance: 0.0 };
        }
        let direction = (offset / distance).as_vec3();
        Placement { lateral: direction.dot(self.right).clamp(-1.0, 1.0), distance: distance as f32 }
    }
}

/// Ein Hoertest: eine Quelle laeuft einmal im Kreis um den Kopf.
///
/// Warum das ueberhaupt in den Client gehoert und nicht nur in einen Unit-Test: ob eine Ortung
/// funktioniert, entscheidet das Ohr, nicht die Zahl. Die Tests belegen, dass Versatz und
/// Kopfschatten rechnerisch stimmen -- ob daraus eine hoerbare Richtung wird, haengt am Geraet
/// (ueber Lautsprecher deutlich schwaecher als ueber Kopfhoerer) und am Zuhoerer. Dieser Test
/// beantwortet das in drei Sekunden, ohne dass ein Server, ein Gegner oder ein Geraeusch-
/// Ereignis dafuer existieren muss.
///
/// Der Kreis liegt waagerecht in Ohrhoehe und beginnt VORNE, laeuft dann nach rechts herum.
/// Vorne als Start ist Absicht: der erste Klick ist damit der neutrale Bezugspunkt, gegen den
/// man die folgenden hoert.
pub struct OrbitTest {
    /// Wie viele Klicks schon gespielt wurden.
    played: usize,
    /// Sekunden seit dem letzten Klick.
    elapsed: f32,
}

/// Klicks fuer eine volle Runde. Zwoelf sind fein genug, um die Bewegung als Bewegung zu
/// hoeren, und grob genug, dass jeder einzelne Klick als eigene Richtung wahrnehmbar bleibt.
const STEPS: usize = 12;
/// Abstand zwischen zwei Klicks. Schneller verschwimmt die Runde zu einem Zittern, langsamer
/// verliert man den Zusammenhang zwischen den Richtungen.
const STEP_SECONDS: f32 = 0.25;
/// Radius des Kreises in Bloecken. Weit genug, dass die Entfernungsdaempfung nicht alles
/// erschlaegt, nah genug, dass der Klang klar bleibt.
const RADIUS: f64 = 4.0;

impl OrbitTest {
    /// Startet die Runde. Der erste Klick kommt sofort beim naechsten `advance`.
    pub fn start() -> OrbitTest {
        OrbitTest { played: 0, elapsed: STEP_SECONDS }
    }

    /// Zeit weiterdrehen. Gibt die Position des faelligen Klicks zurueck -- ein Klick je
    /// Aufruf, damit ein Ruckler nicht mehrere Klicks in denselben Augenblick schiebt.
    ///
    /// `None` heisst: gerade nichts zu spielen. Ob die Runde vorbei ist, sagt `finished`.
    pub fn advance(&mut self, dt: f32, listener: &Listener) -> Option<DVec3> {
        if self.finished() {
            return None;
        }
        self.elapsed += dt;
        if self.elapsed < STEP_SECONDS {
            return None;
        }
        self.elapsed -= STEP_SECONDS;
        let position = self.step_position(self.played, listener);
        self.played += 1;
        Some(position)
    }

    pub fn finished(&self) -> bool {
        self.played >= STEPS
    }

    /// Schritt `n` als Weltposition: im Kreis um den Kopf, beginnend vorne, im Uhrzeigersinn
    /// von oben gesehen (also zuerst nach rechts).
    fn step_position(&self, n: usize, listener: &Listener) -> DVec3 {
        let angle = n as f64 / STEPS as f64 * std::f64::consts::TAU;
        // Vorne = Ohrachse um 90 Grad gedreht. Aus `right` (x, 0, z) wird die Blickrichtung
        // (-z, 0, x) -- so braucht der Test den Gierwinkel nicht noch einmal.
        let right = listener.right.as_dvec3();
        let front = DVec3::new(-right.z, 0.0, right.x);
        listener.position + (front * angle.cos() + right * angle.sin()) * RADIUS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Gierwinkel 0 = Blick nach +Z (Sueden), rechts ist dann -X (Westen) -- dieselbe
    // Konvention wie render::Camera. Steht hier als Test, weil ein Vorzeichendreher an dieser
    // Stelle die ganze Ortung spiegeln wuerde, ohne irgendwo anders aufzufallen.
    #[test]
    fn die_ohrachse_folgt_der_blickrichtung() {
        let l = Listener::at_eye(DVec3::ZERO, 0.0);
        assert!((l.right - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5, "war {:?}", l.right);
        // Eine Vierteldrehung nach rechts (Blick nach Westen) dreht die Ohrachse mit.
        let l = Listener::at_eye(DVec3::ZERO, 90.0);
        assert!((l.right - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5, "war {:?}", l.right);
    }

    // Eine Quelle rechts vom Spieler muss auch rechts landen -- der Test, der einen
    // seitenverkehrten Client sofort auffliegen laesst.
    #[test]
    fn rechts_ist_rechts() {
        let l = Listener::at_eye(DVec3::ZERO, 0.0);
        // Blick nach +Z, rechts ist -X.
        let p = l.placement(DVec3::new(-10.0, 0.0, 0.0));
        assert!(p.lateral > 0.99, "lateral war {}", p.lateral);
        assert!((p.distance - 10.0).abs() < 1e-4);

        let p = l.placement(DVec3::new(10.0, 0.0, 0.0));
        assert!(p.lateral < -0.99, "lateral war {}", p.lateral);
    }

    // Geradeaus und geradeaus-hinten liegen beide in der Mittelebene.
    #[test]
    fn vorne_und_hinten_liegen_in_der_mitte() {
        let l = Listener::at_eye(DVec3::ZERO, 0.0);
        assert!(l.placement(DVec3::new(0.0, 0.0, 5.0)).lateral.abs() < 1e-5);
        assert!(l.placement(DVec3::new(0.0, 0.0, -5.0)).lateral.abs() < 1e-5);
    }

    // Senkrecht ueber dem Kopf ist ebenfalls Mittelebene -- der Kugelkopf kennt kein Oben,
    // aber er darf es auch nicht faelschlich auf eine Seite legen.
    #[test]
    fn ueber_dem_kopf_liegt_in_der_mitte() {
        let l = Listener::at_eye(DVec3::ZERO, 0.0);
        let p = l.placement(DVec3::new(0.0, 5.0, 0.0));
        assert!(p.lateral.abs() < 1e-5);
        assert!((p.distance - 5.0).abs() < 1e-4);
    }

    // Eine Quelle genau am Ohr darf kein NaN erzeugen.
    #[test]
    fn eine_quelle_im_kopf_ergibt_keine_ungueltige_lage() {
        let l = Listener::at_eye(DVec3::new(3.0, 4.0, 5.0), 37.0);
        let p = l.placement(DVec3::new(3.0, 4.0, 5.0));
        assert!(p.lateral.is_finite() && p.distance.is_finite());
    }

    // Der Hoertest laeuft genau einmal herum, einen Klick je Schritt.
    #[test]
    fn die_runde_spielt_jeden_schritt_genau_einmal() {
        let l = Listener::at_eye(DVec3::ZERO, 0.0);
        let mut test = OrbitTest::start();
        let mut klicks = Vec::new();
        // Reichlich Zeit, in kleinen Schritten -- so wie die Bildschleife sie liefert.
        for _ in 0..1000 {
            if let Some(pos) = test.advance(1.0 / 60.0, &l) {
                klicks.push(pos);
            }
        }
        assert_eq!(klicks.len(), STEPS);
        assert!(test.finished());
    }

    // Die Runde muss auch als Runde HOERBAR sein: erst rechts, dann hinten, dann links.
    // Geprueft an der Seitenlage, die das Ohrenmodell tatsaechlich zu sehen bekommt.
    #[test]
    fn die_runde_laeuft_nach_rechts_herum() {
        let l = Listener::at_eye(DVec3::ZERO, 0.0);
        let mut test = OrbitTest::start();
        let mut seiten = Vec::new();
        for _ in 0..1000 {
            if let Some(pos) = test.advance(1.0 / 60.0, &l) {
                seiten.push(l.placement(pos).lateral);
            }
        }
        assert!(seiten[0].abs() < 1e-5, "der erste Klick steht vorne");
        assert!(seiten[STEPS / 4] > 0.99, "nach einer Vierteldrehung rechts");
        assert!(seiten[STEPS / 2].abs() < 1e-5, "nach der Hälfte hinten");
        assert!(seiten[STEPS * 3 / 4] < -0.99, "nach drei Vierteln links");
    }

    // Der Kreis bleibt ein Kreis um den Kopf, egal wie der Spieler steht -- sonst waere die
    // gehoerte Richtung von der zufaelligen Blickrichtung beim Start abhaengig.
    #[test]
    fn der_kreis_haelt_den_abstand_und_die_hoehe() {
        let l = Listener::at_eye(DVec3::new(-120.5, 64.0, 900.25), 213.0);
        let mut test = OrbitTest::start();
        for _ in 0..1000 {
            if let Some(pos) = test.advance(1.0 / 60.0, &l) {
                // Toleranz nicht schaerfer: die Ohrachse kommt als f32 aus der Kamera, ihre
                // Laenge stimmt also nur auf rund sieben Stellen. Die Hoehe dagegen ist
                // exakt -- `right` hat von Bauart her y = 0 (s. step_position).
                assert!(((pos - l.position).length() - RADIUS).abs() < 1e-5);
                assert_eq!(pos.y, l.position.y, "der Kreis liegt in Ohrhöhe");
            }
        }
    }
}
