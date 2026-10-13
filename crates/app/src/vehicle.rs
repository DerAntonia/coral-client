// Fahrzeuge, die der eigene Client lenkt.
//
// Sitzt man vorn in einem Boot (oder auf einem gesattelten Reittier), rechnet in Vanilla der
// CLIENT dessen Bewegung aus (Entity.isLogicalSideForUpdatingMovement) und meldet sie dem
// Server je Tick mit VehicleMove -- der Server bewegt so ein Fahrzeug selbst gar nicht. Ohne
// diese Rechnung bliebe das Boot einfach liegen.
//
// Hier steht das Verhalten von AbstractBoatEntity.tick nachgebaut (Lage im Wasser bestimmen,
// Auftrieb und Reibung, Ruder, Bewegung mit Kollision). Die Box des Boots ist 1.375 breit und
// 0.5625 hoch (EntityType), Floesse genauso.

use glam::DVec3;
use world::World;

use crate::physics;

pub const BOAT_WIDTH: f64 = 1.375;
pub const BOAT_HEIGHT: f64 = 0.5625;

// AbstractBoatEntity.getGravity.
const BOAT_GRAVITY: f64 = 0.04;

// Wo das Boot gerade ist (AbstractBoatEntity.Location).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoatLocation {
    InWater,
    UnderWater,
    UnderFlowingWater,
    OnLand,
    InAir,
}

// Die Tasten, die das Boot lenken -- ClientPlayerEntity.tickRiding reicht sie ans Boot weiter
// (setInputs), und zwar NACH dessen Tick: das Boot faehrt also immer mit den Tasten des
// vorigen Ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoatInput {
    pub left: bool,
    pub right: bool,
    pub forward: bool,
    pub back: bool,
}

// Worauf der eigene Spieler sitzt. Der eigene Spieler steht nicht in world::Entities (der
// Server schickt fuer ihn kein Spawn), deshalb fuehrt die App diese Beziehung selbst.
#[derive(Clone, Debug)]
pub struct OwnRide {
    pub vehicle: i32,
    pub seat: world::riding::Seat,
    // Lenkt der eigene Client das Fahrzeug in diesem Tick (vorn sitzend, Sattel/Rute passen)?
    pub controlling: bool,
    pub boat: BoatSim,
    // Tasten des VORIGEN Ticks (s. BoatInput).
    pub boat_input: BoatInput,
    // Stelle und Drehung, die die Simulation dem Fahrzeug in diesem Tick gegeben hat -- geht
    // als VehicleMove an den Server.
    pub sim_pos: DVec3,
    pub sim_yaw: f32,
    pub sim_pitch: f32,
}

impl OwnRide {
    pub fn new(vehicle: i32, seat: world::riding::Seat) -> Self {
        Self {
            vehicle,
            seat,
            controlling: false,
            boat: BoatSim::default(),
            boat_input: BoatInput::default(),
            sim_pos: DVec3::ZERO,
            sim_yaw: 0.0,
            sim_pitch: 0.0,
        }
    }
}

// AbstractBoatEntity.clampPassengerYaw: der Kopf darf sich hoechstens 105 Grad gegen das Boot
// drehen. Liefert den geklemmten Yaw.
pub fn clamp_boat_passenger_yaw(passenger_yaw: f32, boat_yaw: f32) -> f32 {
    let f = world::wrap_degrees(passenger_yaw - boat_yaw);
    let g = f.clamp(-105.0, 105.0);
    passenger_yaw + g - f
}

#[derive(Clone, Debug)]
pub struct BoatSim {
    pub vel: DVec3,
    // Drehgeschwindigkeit in Grad je Tick; die Mitfahrer drehen sich um denselben Betrag mit.
    pub yaw_velocity: f32,
    location: Option<BoatLocation>,
    last_location: Option<BoatLocation>,
    water_level: f64,
    nearby_slipperiness: f32,
    // Senkrechte Geschwindigkeit beim letzten Aufsetzen (AbstractBoatEntity.fall): wie tief
    // getWaterHeightBelow nach der Oberflaeche sucht.
    fall_velocity: f64,
    pub on_ground: bool,
    // Welche Ruder sich bewegen (links, rechts) -- geht je Tick als PaddleBoat an den Server.
    pub paddles: (bool, bool),
    // Wie lange das Boot schon untergetaucht ist; ab 60 wirft der Server alle ab.
    pub ticks_underwater: f32,
}

impl Default for BoatSim {
    fn default() -> Self {
        Self {
            vel: DVec3::ZERO,
            yaw_velocity: 0.0,
            location: None,
            last_location: None,
            water_level: 0.0,
            nearby_slipperiness: 0.0,
            fall_velocity: 0.0,
            on_ground: false,
            paddles: (false, false),
            ticks_underwater: 0.0,
        }
    }
}

fn boat_box(pos: DVec3) -> (DVec3, DVec3) {
    let h = BOAT_WIDTH / 2.0;
    (DVec3::new(pos.x - h, pos.y, pos.z - h), DVec3::new(pos.x + h, pos.y + BOAT_HEIGHT, pos.z + h))
}

// Wasserzelle: (Oberflaeche ueber der Zellunterkante 0..1, ist es eine Quelle).
fn water_at(world: &World, x: i32, y: i32, z: i32) -> Option<(f64, bool)> {
    let state = world.block_at(x, y, z);
    if world::fluid_kind(state) != Some(world::FluidKind::Water) {
        return None;
    }
    let h = world::fluid_height_at(world, x, y, z)?;
    // FluidState.isStill: eine Quelle -- Fuellstufe 0, oder gar keine (waterlogged,
    // Wasserpflanzen: immer volle Quelle).
    let still = world::state_props::property(state, "level").is_none_or(|l| l == "0");
    Some((h, still))
}

impl BoatSim {
    #[cfg(test)]
    pub fn location(&self) -> Option<BoatLocation> {
        self.location
    }

    // Ein Tick, wie AbstractBoatEntity.tick fuer ein vom Client gelenktes Boot. `pos`/`yaw`
    // sind Stelle und Drehung des Boots, `driver_is_player` ob vorn ein Spieler sitzt (sonst
    // ruhen die Ruder).
    pub fn tick(&mut self, pos: &mut DVec3, yaw: &mut f32, input: BoatInput, driver_is_player: bool, world: &World, shapes: &world::BlockShapes) {
        self.last_location = self.location;
        let loc = self.check_location(*pos, world, shapes);
        self.location = Some(loc);
        if matches!(loc, BoatLocation::UnderWater | BoatLocation::UnderFlowingWater) {
            self.ticks_underwater += 1.0;
        } else {
            self.ticks_underwater = 0.0;
        }

        // Entity.baseTick -> updateWaterState: die Stroemung schiebt das Boot (vor allem
        // anderen an der Geschwindigkeit).
        let (lo, hi) = boat_box(*pos);
        physics::push_by_water(lo, hi, &mut self.vel, world, shapes);

        if !driver_is_player {
            self.paddles = (false, false);
        }
        self.update_velocity(pos, driver_is_player, world, shapes);
        self.update_paddles(yaw, input);
        self.do_move(pos, world, shapes);
    }

    // AbstractBoatEntity.checkLocation.
    fn check_location(&mut self, pos: DVec3, world: &World, shapes: &world::BlockShapes) -> BoatLocation {
        let (lo, hi) = boat_box(pos);
        if let Some(loc) = self.under_water_location(lo, hi, world) {
            self.water_level = hi.y;
            return loc;
        }
        if self.check_in_water(lo, hi, world) {
            return BoatLocation::InWater;
        }
        let f = nearby_slipperiness(lo, hi, world, shapes);
        if f > 0.0 {
            self.nearby_slipperiness = f;
            return BoatLocation::OnLand;
        }
        BoatLocation::InAir
    }

    // getUnderWaterLocation: steht Wasser ueber der Oberkante des Boots?
    fn under_water_location(&self, lo: DVec3, hi: DVec3, world: &World) -> Option<BoatLocation> {
        let d = hi.y + 0.001;
        let mut unter = false;
        for x in lo.x.floor() as i32..hi.x.ceil() as i32 {
            for y in hi.y.floor() as i32..d.ceil() as i32 {
                for z in lo.z.floor() as i32..hi.z.ceil() as i32 {
                    if let Some((h, still)) = water_at(world, x, y, z) {
                        if d < y as f64 + h {
                            if !still {
                                return Some(BoatLocation::UnderFlowingWater);
                            }
                            unter = true;
                        }
                    }
                }
            }
        }
        unter.then_some(BoatLocation::UnderWater)
    }

    // checkBoatInWater: reicht Wasser ueber die Unterkante? Merkt sich die hoechste Oberflaeche.
    fn check_in_water(&mut self, lo: DVec3, hi: DVec3, world: &World) -> bool {
        let mut drin = false;
        self.water_level = -f64::MAX;
        for x in lo.x.floor() as i32..hi.x.ceil() as i32 {
            for y in lo.y.floor() as i32..(lo.y + 0.001).ceil() as i32 {
                for z in lo.z.floor() as i32..hi.z.ceil() as i32 {
                    if let Some((h, _)) = water_at(world, x, y, z) {
                        // Vanilla rechnet die Oberflaeche als float.
                        let f = (y as f32 + h as f32) as f64;
                        self.water_level = self.water_level.max(f);
                        drin |= lo.y < f;
                    }
                }
            }
        }
        drin
    }

    // getWaterHeightBelow: die erste nicht ganz volle Wasserschicht von oben -- dort landet ein
    // Boot, das aus der Luft ins Wasser faellt.
    fn water_height_below(&self, pos: DVec3, world: &World) -> f64 {
        let (lo, hi) = boat_box(pos);
        let (x0, x1) = (lo.x.floor() as i32, hi.x.ceil() as i32);
        let (z0, z1) = (lo.z.floor() as i32, hi.z.ceil() as i32);
        let k = hi.y.floor() as i32;
        let l = (hi.y - self.fall_velocity).ceil() as i32;
        'schicht: for y in k..l {
            let mut f = 0.0f32;
            for x in x0..x1 {
                for z in z0..z1 {
                    if let Some((h, _)) = water_at(world, x, y, z) {
                        f = f.max(h as f32);
                    }
                    if f >= 1.0 {
                        continue 'schicht;
                    }
                }
            }
            if f < 1.0 {
                return (y as f32 + f) as f64;
            }
        }
        (l + 1) as f64
    }

    // updateVelocity: Schwerkraft, Auftrieb und Reibung je nach Lage.
    fn update_velocity(&mut self, pos: &mut DVec3, driver_is_player: bool, world: &World, shapes: &world::BlockShapes) {
        let mut d = -BOAT_GRAVITY;
        let mut e = 0.0f64;
        let f: f32;
        let loc = self.location.unwrap_or(BoatLocation::InAir);
        if self.last_location == Some(BoatLocation::InAir) && loc != BoatLocation::InAir && loc != BoatLocation::OnLand {
            // Aus der Luft aufs Wasser: direkt auf die Oberflaeche setzen, wenn dort Platz ist.
            self.water_level = pos.y + BOAT_HEIGHT;
            let g = self.water_height_below(*pos, world) - BOAT_HEIGHT + 0.101;
            let (lo, hi) = boat_box(DVec3::new(pos.x, g, pos.z));
            if physics::space_empty(lo, hi, world, shapes) {
                pos.y = g;
                self.vel.y = 0.0;
                self.fall_velocity = 0.0;
            }
            self.location = Some(BoatLocation::InWater);
            return;
        }
        match loc {
            BoatLocation::InWater => {
                e = (self.water_level - pos.y) / BOAT_HEIGHT;
                f = 0.9;
            }
            BoatLocation::UnderFlowingWater => {
                d = -7.0e-4;
                f = 0.9;
            }
            BoatLocation::UnderWater => {
                e = 0.01f32 as f64;
                f = 0.45;
            }
            BoatLocation::InAir => f = 0.9,
            BoatLocation::OnLand => {
                f = self.nearby_slipperiness;
                // Gelenkt rutscht ein Boot an Land nur halb so weit nach (gilt fuer den
                // naechsten Tick, falls der nicht wieder an Land ist).
                if driver_is_player {
                    self.nearby_slipperiness /= 2.0;
                }
            }
        }
        self.vel = DVec3::new(self.vel.x * f as f64, self.vel.y + d, self.vel.z * f as f64);
        self.yaw_velocity *= f;
        if e > 0.0 {
            self.vel.y = (self.vel.y + e * (BOAT_GRAVITY / 0.65)) * 0.75;
        }
    }

    // updatePaddles: Drehen mit links/rechts, Schub mit vor/zurueck.
    fn update_paddles(&mut self, yaw: &mut f32, input: BoatInput) {
        let mut f = 0.0f32;
        if input.left {
            self.yaw_velocity -= 1.0;
        }
        if input.right {
            self.yaw_velocity += 1.0;
        }
        if input.right != input.left && !input.forward && !input.back {
            f += 0.005;
        }
        *yaw += self.yaw_velocity;
        if input.forward {
            f += 0.04;
        }
        if input.back {
            f -= 0.005;
        }
        let rad = std::f32::consts::PI / 180.0;
        let sx = physics::mc_sin((-*yaw * rad) as f64) * f;
        let cz = physics::mc_cos((*yaw * rad) as f64) * f;
        self.vel += DVec3::new(sx as f64, 0.0, cz as f64);
        self.paddles = (input.right && !input.left || input.forward, input.left && !input.right || input.forward);
    }

    // Entity.move(SELF, velocity) fuer das Boot: Kollision mit Bloecken (ohne Stufensteigen,
    // Boote haben keins), dann die Folgen fuer die Geschwindigkeit.
    fn do_move(&mut self, pos: &mut DVec3, world: &World, shapes: &world::BlockShapes) {
        let wunsch = self.vel;
        let (lo, hi) = boat_box(*pos);
        let ist = physics::collide_box(lo, hi, wunsch, world, shapes);
        let d = ist.length_squared();
        if d > 1.0e-7 || wunsch.length_squared() - d < 1.0e-7 {
            *pos += ist;
        }
        let x_stoss = !approx(wunsch.x, ist.x);
        let z_stoss = !approx(wunsch.z, ist.z);
        let senkrecht = wunsch.y != ist.y;
        self.on_ground = senkrecht && wunsch.y < 0.0;
        // AbstractBoatEntity.fall: die Fallgeschwindigkeit vor allen Korrekturen.
        self.fall_velocity = self.vel.y;
        if x_stoss || z_stoss {
            self.vel = DVec3::new(if x_stoss { 0.0 } else { self.vel.x }, self.vel.y, if z_stoss { 0.0 } else { self.vel.z });
        }
        // getLandingPos: der Block 0.2 unter der Unterkante.
        let landung = world.block_at(pos.x.floor() as i32, (pos.y - 0.2).floor() as i32, pos.z.floor() as i32);
        let landung_name = world::block_name(landung).unwrap_or("air");
        if senkrecht {
            // Block.onEntityLand: Schleim und Betten werfen zurueck (ein Boot ist kein
            // Lebewesen: Faktor 0.8), alles andere nimmt die Fallgeschwindigkeit weg.
            let r = physics::bounce_restitution_of(landung_name);
            if r > 0.0 {
                if self.vel.y < 0.0 {
                    self.vel.y = -self.vel.y * r * 0.8;
                }
            } else {
                self.vel.y = 0.0;
            }
        }
        // getVelocityMultiplier: der Block, in dem das Boot steht, sonst der darunter.
        let drin = world::block_name(world.block_at(pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32)).unwrap_or("air");
        let mut m = physics::velocity_multiplier_of(drin);
        if m == 1.0 && !matches!(drin, "water" | "bubble_column") {
            let unter = world::block_name(world.block_at(pos.x.floor() as i32, (pos.y - 0.500001).floor() as i32, pos.z.floor() as i32)).unwrap_or("air");
            m = physics::velocity_multiplier_of(unter);
        }
        self.vel.x *= m;
        self.vel.z *= m;
    }
}

// MathHelper.approximatelyEquals.
fn approx(a: f64, b: f64) -> bool {
    (b - a).abs() < 1.0e-5
}

// getNearbySlipperiness: mittlere Reibung der Bloecke, die eine hauchduenne Schicht unter dem
// Boot beruehren (ohne Seerosen, ohne die vier Eckspalten des erweiterten Rasters).
fn nearby_slipperiness(lo: DVec3, hi: DVec3, world: &World, shapes: &world::BlockShapes) -> f32 {
    let (min, max) = (DVec3::new(lo.x, lo.y - 0.001, lo.z), DVec3::new(hi.x, lo.y, hi.z));
    let (i, j) = (min.x.floor() as i32 - 1, max.x.ceil() as i32 + 1);
    let (k, l) = (min.y.floor() as i32 - 1, max.y.ceil() as i32 + 1);
    let (m, n) = (min.z.floor() as i32 - 1, max.z.ceil() as i32 + 1);
    let mut summe = 0.0f32;
    let mut zahl = 0u32;
    for x in i..j {
        for z in m..n {
            let rand = (x == i || x == j - 1) as u8 + (z == m || z == n - 1) as u8;
            if rand == 2 {
                continue;
            }
            for y in k..l {
                if rand > 0 && (y == k || y == l - 1) {
                    continue;
                }
                let name = world::block_name(world.block_at(x, y, z)).unwrap_or("air");
                if name != "lily_pad" && physics::block_shape_overlaps(x, y, z, min, max, world, shapes) {
                    summe += physics::friction_of(name) as f32;
                    zahl += 1;
                }
            }
        }
    }
    // Vanilla teilt ohne Pruefung -- 0/0 ist dort NaN, und "NaN > 0" ist falsch: In der Luft.
    if zahl == 0 {
        0.0
    } else {
        summe / zahl as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zustand(name: &str) -> u32 {
        (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap()
    }

    fn welt_mit(bloecke: &[((i32, i32, i32), &str)]) -> World {
        let mut w = World::new();
        for cx in -1..=0 {
            for cz in -1..=0 {
                let leer = (0..24).map(|_| world::ChunkSection::from_states(vec![0u32; 16 * 16 * 16])).collect();
                w.insert_column(cx, cz, leer);
            }
        }
        for &((x, y, z), name) in bloecke {
            w.set_block(x, y, z, zustand(name));
        }
        w
    }

    // Ein Wasserbecken 6x6, Oberflaeche bei y = 63 + 8/9.
    fn becken() -> World {
        let mut b = Vec::new();
        for x in -3..3 {
            for z in -3..3 {
                b.push(((x, 62, z), "stone"));
                b.push(((x, 63, z), "water"));
            }
        }
        welt_mit(&b)
    }

    #[test]
    fn ein_boot_schwimmt_auf_der_oberflaeche() {
        let w = becken();
        let shapes = world::BlockShapes::all_full_cubes();
        let mut sim = BoatSim::default();
        let (mut pos, mut yaw) = (DVec3::new(0.0, 64.5, 0.0), 0.0f32);
        for _ in 0..200 {
            sim.tick(&mut pos, &mut yaw, BoatInput::default(), true, &w, &shapes);
        }
        // Gleichgewicht: (-0.04 + e * 0.04 / 0.65) * 0.75 = 0, also e = 0.65 -- die Unterkante
        // liegt 0.65 Bootshoehen unter der Oberflaeche.
        let oberflaeche = (63.0f32 + 8.0 / 9.0) as f64;
        assert!((pos.y - (oberflaeche - 0.65 * BOAT_HEIGHT)).abs() < 1e-3, "y = {}", pos.y);
        assert_eq!(sim.location(), Some(BoatLocation::InWater));
        assert!(sim.vel.y.abs() < 0.01);
    }

    #[test]
    fn vorwaerts_faehrt_in_blickrichtung() {
        let w = becken();
        let shapes = world::BlockShapes::all_full_cubes();
        let mut sim = BoatSim::default();
        let (mut pos, mut yaw) = (DVec3::new(0.0, 63.4, -2.0), 0.0f32);
        let vor = BoatInput { forward: true, ..Default::default() };
        for _ in 0..20 {
            sim.tick(&mut pos, &mut yaw, vor, true, &w, &shapes);
        }
        // Yaw 0 = +Z.
        assert!(pos.z > -2.0 + 0.5, "z = {}", pos.z);
        assert!(pos.x.abs() < 1e-6);
        assert_eq!(sim.paddles, (true, true));
    }

    #[test]
    fn links_dreht_gegen_den_uhrzeigersinn() {
        let w = becken();
        let shapes = world::BlockShapes::all_full_cubes();
        let mut sim = BoatSim::default();
        let (mut pos, mut yaw) = (DVec3::new(0.0, 63.4, 0.0), 0.0f32);
        let links = BoatInput { left: true, ..Default::default() };
        sim.tick(&mut pos, &mut yaw, links, true, &w, &shapes);
        assert!(yaw < 0.0 && sim.yaw_velocity < 0.0);
        // Nur links: das RECHTE Ruder arbeitet.
        assert_eq!(sim.paddles, (false, true));
    }

    #[test]
    fn an_land_bremst_der_boden() {
        let mut b = Vec::new();
        for x in -3..3 {
            for z in -3..3 {
                b.push(((x, 63, z), "stone"));
            }
        }
        let w = welt_mit(&b);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut sim = BoatSim::default();
        let (mut pos, mut yaw) = (DVec3::new(0.0, 64.0, 0.0), 0.0f32);
        sim.vel = DVec3::new(0.0, 0.0, 0.5);
        sim.tick(&mut pos, &mut yaw, BoatInput::default(), true, &w, &shapes);
        assert_eq!(sim.location(), Some(BoatLocation::OnLand));
        // Stein: Reibung 0.6.
        assert!((sim.vel.z - 0.3).abs() < 1e-6, "{}", sim.vel.z);
        assert!(sim.on_ground);
    }
}
