// Zuschauermodus, abweichend von Vanilla:
//  - Kein Zuschauer-Menue auf den Hotbar-Ziffern; zum Teleportieren gibt es /tp im Chat.
//  - Feste Fluggeschwindigkeit in Bloecken je Sekunde, ohne Anlauf und Auslauf, mit zwei
//    eigenen Tasten (und dem Mausrad) eine Stufe langsamer bzw. schneller.
//  - Linksklick auf einen Block teleportiert dorthin, so weit die Welt geladen ist.
//
// Der Teleport ist kein Befehl (ohne OP-Rechte gibt es /tp nicht), sondern ein schneller Flug:
// je Tick ein Bewegungspaket. Gegen Paper gemessen (net/tests/spectator_real.rs): bis knapp
// unter 10 Bloecken je Paket nimmt der Server es an, ab 10,5 setzt er zurueck ("moved too
// quickly"); mehrere Pakete in einem Tick bringen kaum mehr. Also SPRUNG Bloecke je Tick.

use glam::DVec3;

use crate::{physics, App, CrosshairTarget, GAME_MODE_SPECTATOR};

pub const SPEED_STEPS: [u32; 20] = [1, 2, 3, 4, 5, 6, 8, 10, 12, 15, 20, 25, 30, 40, 50, 60, 70, 80, 90, 100];
// Etwa Vanillas Zuschauerflug geradeaus (Fluggeschwindigkeit 0.05 bei Luftbremse 0.91).
pub const DEFAULT_SPEED: u32 = 10;
// Weg je Tick beim Klick-Teleport, mit Abstand unter der Grenze des Servers.
const SPRUNG: f64 = 9.0;
// Durch die geladene Welt so weit; dahinter geht es in der gespeicherten Welt der Fernsicht
// weiter, so weit wie die LOD-Render-Distance reicht (mindestens so weit wie hier).
const TELEPORT_REACH: f64 = 4096.0;
// So hoch ueber der Stelle, auf die man klickt, kommt man an -- man landet ueber dem Ziel und
// sieht es, statt mit der Nase davor.
const LANDING_HEIGHT: f64 = 5.0;

pub fn nearest_speed(v: u32) -> u32 {
    *SPEED_STEPS.iter().min_by_key(|&&s| s.abs_diff(v)).unwrap_or(&DEFAULT_SPEED)
}

pub fn step_speed(v: u32, schneller: bool) -> u32 {
    let i = SPEED_STEPS.iter().position(|&s| s == nearest_speed(v)).unwrap_or(0);
    let i = if schneller { (i + 1).min(SPEED_STEPS.len() - 1) } else { i.saturating_sub(1) };
    SPEED_STEPS[i]
}

// Wo die Fuesse nach dem Teleport stehen: ueber der Zelle vor der getroffenen Seite, mittig,
// LANDING_HEIGHT hoeher. Von unten getroffen haengt der Kopf in dieser Zelle statt der Fuesse.
pub fn landing_spot(hit: &physics::BlockRayHit) -> DVec3 {
    let (x, y, z) = hit.block;
    let (dx, dy, dz) = match hit.face {
        protocol::packets::FACE_DOWN => (0, -1, 0),
        protocol::packets::FACE_UP => (0, 1, 0),
        protocol::packets::FACE_NORTH => (0, 0, -1),
        protocol::packets::FACE_SOUTH => (0, 0, 1),
        protocol::packets::FACE_WEST => (-1, 0, 0),
        _ => (1, 0, 0),
    };
    let fuss = if hit.face == protocol::packets::FACE_DOWN { (y + dy) as f64 + 1.0 - physics::PLAYER_HEIGHT as f64 } else { (y + dy) as f64 };
    DVec3::new((x + dx) as f64 + 0.5, fuss + LANDING_HEIGHT, (z + dz) as f64 + 0.5)
}

// Ein Tick des Klick-Teleports: von `pos` hoechstens SPRUNG Bloecke Richtung `ziel`. Dazu, ob
// das Ziel damit erreicht ist.
pub fn flight_step(pos: DVec3, ziel: DVec3) -> (DVec3, bool) {
    let weg = ziel - pos;
    let laenge = weg.length();
    if laenge <= SPRUNG {
        (ziel, true)
    } else {
        (pos + weg / laenge * SPRUNG, false)
    }
}

impl App {
    pub(crate) fn spectator_speed_step(&mut self, schneller: bool) {
        let neu = step_speed(self.settings.spectator_speed, schneller);
        if neu != self.settings.spectator_speed {
            self.settings.spectator_speed = neu;
            let _ = self.settings.save();
        }
        let text = format!("Fluggeschwindigkeit: {neu} Blöcke/s");
        self.action_bar = Some((vec![protocol::nbt::TextSpan { text, color: None, link: None, action: None, style: 0, shadow: None }], std::time::Instant::now()));
    }

    // Linksklick als Zuschauer. false = Vanilla soll weitermachen (ein Entity im Fadenkreuz:
    // der Server setzt die Kamera hinein). Sonst true, ob ein Block getroffen wurde oder nicht.
    pub(crate) fn spectator_teleport_click(&mut self) -> bool {
        if matches!(self.crosshair_target(), CrosshairTarget::Entity(_)) || self.spectated_entity().is_some() {
            return false;
        }
        let eye = self.physics.eye_pos();
        let dir = self.camera.forward();
        let dir = DVec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
        if let Some(hit) = physics::raycast_block_far(eye, dir, TELEPORT_REACH, &self.world, &self.block_shapes).or_else(|| self.lod_teleport_hit(eye, dir)) {
            self.spectator_flight = Some(landing_spot(&hit));
        }
        true
    }

    // Hinter den geladenen Chunks: die gespeicherten Bloecke der Fernsicht (Modul Chunk-Saver).
    fn lod_teleport_hit(&self, eye: DVec3, dir: DVec3) -> Option<physics::BlockRayHit> {
        let reach = TELEPORT_REACH.max(self.settings.lod_render_distance as f64 * 16.0);
        let (block, achse, schritt) = self.lod.raycast(eye.to_array(), dir.to_array(), reach)?;
        let face = match (achse, schritt > 0) {
            (0, true) => protocol::packets::FACE_WEST,
            (0, false) => protocol::packets::FACE_EAST,
            (1, true) => protocol::packets::FACE_DOWN,
            (1, false) => protocol::packets::FACE_UP,
            (_, true) => protocol::packets::FACE_NORTH,
            (_, false) => protocol::packets::FACE_SOUTH,
        };
        let pos = (i32::try_from(block[0]).ok()?, i32::try_from(block[1]).ok()?, i32::try_from(block[2]).ok()?);
        Some(physics::BlockRayHit { block: pos, face, cursor: (0.5, 0.5, 0.5), inside: false })
    }

    // Statt der Physik, solange ein Klick-Teleport laeuft: ein Stueck naeher ans Ziel, ohne
    // Geschwindigkeit. Jede Bewegungstaste bricht ab, dann fliegt man von hier aus selbst.
    pub(crate) fn tick_spectator_flight(&mut self, input: &physics::Input) -> bool {
        let Some(ziel) = self.spectator_flight else { return false };
        let gelenkt = input.forward != 0.0 || input.strafe != 0.0 || input.jump || input.sneak;
        if self.game_mode != GAME_MODE_SPECTATOR || gelenkt || self.spectated_entity().is_some() {
            self.spectator_flight = None;
            return false;
        }
        let (pos, da) = flight_step(self.physics.pos, ziel);
        self.physics.pos = pos;
        self.physics.vel = DVec3::ZERO;
        if da {
            self.spectator_flight = None;
        }
        true
    }

    // Rechtsklick als Zuschauer (MultiPlayerGameMode im Zuschauermodus): ein Block bekommt
    // sein USE_ITEM_ON und nimmt an (der Server oeffnet Kisten und andere Menues nur zum
    // Ansehen), ein Entity bekommt seine beiden Interaktionspakete je Hand. Ein USE_ITEM
    // schickt Vanilla als Zuschauer nie, und lokal passiert nichts.
    pub(crate) fn spectator_use(&mut self, treffer: Option<physics::BlockRayHit>, entity_ziel: Option<(i32, (f32, f32, f32))>) {
        let Some(tx) = &self.outgoing_tx else { return };
        if let Some((id, at)) = entity_ziel {
            for hand in [protocol::packets::HAND_MAIN, protocol::packets::HAND_OFF] {
                let _ = tx.send(protocol::packets::interact_at_entity_payload(id, at, hand, self.sneak_held));
                let _ = tx.send(protocol::packets::interact_entity_payload(id, hand, self.sneak_held));
            }
            return;
        }
        let Some(hit) = treffer else { return };
        self.block_sequence = self.block_sequence.wrapping_add(1);
        let _ = tx.send(protocol::packets::use_item_on_payload(
            protocol::packets::HAND_MAIN,
            hit.block.0,
            hit.block.1,
            hit.block.2,
            hit.face,
            hit.cursor.0,
            hit.cursor.1,
            hit.cursor.2,
            hit.inside,
            self.block_sequence,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempostufen() {
        assert_eq!(nearest_speed(0), 1);
        assert_eq!(nearest_speed(11), 10);
        assert_eq!(nearest_speed(500), 100);
        assert_eq!(step_speed(10, true), 12);
        assert_eq!(step_speed(10, false), 8);
        assert_eq!(step_speed(100, true), 100);
        assert_eq!(step_speed(1, false), 1);
    }

    #[test]
    fn landeplatz_vor_der_getroffenen_seite() {
        let hit = |face| physics::BlockRayHit { block: (10, 64, -3), face, cursor: (0.5, 0.5, 0.5), inside: false };
        assert_eq!(landing_spot(&hit(protocol::packets::FACE_UP)), DVec3::new(10.5, 70.0, -2.5));
        assert_eq!(landing_spot(&hit(protocol::packets::FACE_NORTH)), DVec3::new(10.5, 69.0, -3.5));
        // Von unten: der Kopf steckt in der Zelle darunter, die Fuesse haengen tiefer.
        let unten = landing_spot(&hit(protocol::packets::FACE_DOWN));
        assert!((unten.y - (64.0 - physics::PLAYER_HEIGHT as f64 + 5.0)).abs() < 1e-9);
    }

    #[test]
    fn teleport_in_schritten_unter_der_servergrenze() {
        let mut pos = DVec3::ZERO;
        let ziel = DVec3::new(100.0, 20.0, 0.0);
        let mut ticks = 0;
        loop {
            let (neu, da) = flight_step(pos, ziel);
            assert!(neu.distance(pos) < 10.0);
            pos = neu;
            ticks += 1;
            if da {
                break;
            }
        }
        assert_eq!(pos, ziel);
        assert_eq!(ticks, 12);
    }
}
