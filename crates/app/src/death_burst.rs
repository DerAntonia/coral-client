// Todesanimation Explosion (Video > Tod, DEATH_ANIM_EXPLOSION): stirbt ein anderer Spieler, faellt er nicht
// wie in Vanilla um (LivingEntityRenderer.setupRotations, deathTime), sondern fliegt in seine
// sechs Koerperteile auseinander. Die Teile drehen sich, fallen, prallen am Boden ab und sind
// nach LIFE_TICKS weg; die Leiche selbst wird nicht mehr gezeichnet.
//
// Gerechnet wird im Modellraum des Spielers (Pixel, y zeigt nach UNTEN, die Fuesse liegen bei
// y = 24), im Takt der Spielticks; gezeichnet wird zwischen zwei Ticks interpoliert. Die
// Ruestung haengt an denselben Teilnamen und fliegt deshalb von selbst mit.

use world::entity_model::{Affine, PartPose, Pose};

const LIFE_TICKS: u32 = 40;
// Pixel je Tick bzw. Tick^2 (16 Pixel = ein Block).
const GRAVITY: f32 = 1.1;
const DRAG: f32 = 0.98;
// Beim Aufprall: was von der Fallgeschwindigkeit (umgekehrt), dem Schwung und der Drehung bleibt.
const BOUNCE: f32 = 0.35;
const GROUND_FRICTION: f32 = 0.6;
// Fuesse im Modellraum (BipedEntityModel: Beine enden bei y = 24).
const FEET_Y: f32 = 24.0;

// Je Teil: Name, Pivot und die Mitte des Kastens relativ zum Pivot (BipedEntityModel), dazu
// der halbe Durchmesser fuer den Boden.
const PARTS: [(&str, [f32; 3], [f32; 3], f32); 6] = [
    ("head", [0.0, 0.0, 0.0], [0.0, -4.0, 0.0], 4.0),
    ("body", [0.0, 0.0, 0.0], [0.0, 6.0, 0.0], 2.0),
    ("right_arm", [-5.0, 2.0, 0.0], [-1.0, 4.0, 0.0], 2.0),
    ("left_arm", [5.0, 2.0, 0.0], [1.0, 4.0, 0.0], 2.0),
    ("right_leg", [-1.9, 12.0, 0.0], [0.0, 6.0, 0.0], 2.0),
    ("left_leg", [1.9, 12.0, 0.0], [0.0, 6.0, 0.0], 2.0),
];
// Von hier aus fliegen die Teile weg: etwa die Hueften.
const CENTER: [f32; 3] = [0.0, 12.0, 0.0];

struct Piece {
    name: &'static str,
    pivot: [f32; 3],
    local_center: [f32; 3],
    radius: f32,
    pos: [f32; 3],
    prev_pos: [f32; 3],
    vel: [f32; 3],
    axis: [f32; 3],
    angle: f32,
    prev_angle: f32,
    spin: f32,
}

pub struct Burst {
    pub uuid: [u8; 16],
    // Fuesse in Weltkoordinaten, Koerperdrehung und Groesse beim Tod.
    pub feet: (f64, f64, f64),
    pub yaw: f32,
    pub scale: f32,
    pub armor: [Option<world::HeldItem>; 4],
    age: u32,
    // Boden im Modellraum (FEET_Y, oder tiefer, wenn der Spieler in der Luft starb).
    ground: f32,
    pieces: Vec<Piece>,
}

// Kleiner Zufall ohne Abhaengigkeit (xorshift), je Explosion neu gesaet.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

impl Burst {
    // `drop_blocks`: so tief liegt der Boden unter den Fuessen; `model_scale`: Bloecke je
    // Modellpixel mal 16 (Spieler: entity_model::PLAYER_MODEL_SCALE mal Entity-Groesse).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        uuid: [u8; 16],
        feet: (f64, f64, f64),
        yaw: f32,
        scale: f32,
        model_scale: f32,
        armor: [Option<world::HeldItem>; 4],
        drop_blocks: f32,
        seed: u64,
    ) -> Self {
        let mut rng = Rng(seed | 1);
        let pieces = PARTS
            .iter()
            .map(|&(name, pivot, local_center, radius)| {
                let pos = [pivot[0] + local_center[0], pivot[1] + local_center[1], pivot[2] + local_center[2]];
                // Weg von der Mitte, mit etwas Streuung, und nach oben (y negativ).
                let mut dir = [pos[0] - CENTER[0] + rng.range(-3.0, 3.0), 0.0, pos[2] - CENTER[2] + rng.range(-3.0, 3.0)];
                let len = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt().max(0.01);
                let speed = rng.range(2.5, 5.0);
                dir = [dir[0] / len * speed, -rng.range(4.0, 8.0), dir[2] / len * speed];
                let axis = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)];
                Piece { name, pivot, local_center, radius, pos, prev_pos: pos, vel: dir, axis, angle: 0.0, prev_angle: 0.0, spin: rng.range(0.25, 0.6) }
            })
            .collect();
        let ground = FEET_Y + drop_blocks.max(0.0) * 16.0 / model_scale.max(0.01);
        Burst { uuid, feet, yaw, scale, armor, age: 0, ground, pieces }
    }

    // Ein Spieltick. false, wenn die Explosion vorbei ist.
    pub fn tick(&mut self) -> bool {
        self.age += 1;
        for p in &mut self.pieces {
            p.prev_pos = p.pos;
            p.prev_angle = p.angle;
            p.vel[1] += GRAVITY;
            for (pos, vel) in p.pos.iter_mut().zip(p.vel.iter_mut()) {
                *vel *= DRAG;
                *pos += *vel;
            }
            p.angle += p.spin;
            let boden = self.ground - p.radius;
            if p.pos[1] > boden {
                p.pos[1] = boden;
                if p.vel[1] > 0.0 {
                    p.vel = [p.vel[0] * GROUND_FRICTION, -p.vel[1] * BOUNCE, p.vel[2] * GROUND_FRICTION];
                    p.spin *= GROUND_FRICTION;
                }
            }
        }
        self.age < LIFE_TICKS
    }

    // Die Pose zwischen dem letzten und dem naechsten Tick (`progress` 0..1): jedes Teil dreht
    // sich um seine eigene Mitte, nicht um sein Gelenk.
    pub fn pose(&self, progress: f32) -> Pose {
        let mut pose = Pose::new();
        for p in &self.pieces {
            let lerp = |a: f32, b: f32| a + (b - a) * progress;
            let mitte = [lerp(p.prev_pos[0], p.pos[0]), lerp(p.prev_pos[1], p.pos[1]), lerp(p.prev_pos[2], p.pos[2])];
            let r = Affine::rotation_axis_angle(lerp(p.prev_angle, p.angle), p.axis);
            let rc = r.then(Affine::translation(p.local_center)).t;
            let offset = [mitte[0] - p.pivot[0] - rc[0], mitte[1] - p.pivot[1] - rc[1], mitte[2] - p.pivot[2] - rc[2]];
            pose.set(p.name, PartPose { pivot_offset: offset, rotation_matrix: Some(r), ..Default::default() });
        }
        pose
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn burst() -> Burst {
        Burst::new([0; 16], (0.0, 64.0, 0.0), 0.0, 1.0, 0.9375, [None; 4], 0.0, 42)
    }

    #[test]
    fn teile_fliegen_auseinander_und_landen() {
        let mut b = burst();
        let start: Vec<[f32; 3]> = b.pieces.iter().map(|p| p.pos).collect();
        for _ in 0..5 {
            b.tick();
        }
        // Erst nach oben (y kleiner) und weg von der Mitte.
        assert!(b.pieces.iter().zip(&start).all(|(p, s)| p.pos[1] < s[1]));
        while b.tick() {}
        // Am Ende liegt jedes Teil auf dem Boden, keins darunter.
        for p in &b.pieces {
            assert!(p.pos[1] <= b.ground - p.radius + 1e-3, "{} unter dem Boden", p.name);
            assert!(p.pos[1] > b.ground - p.radius - 3.0, "{} schwebt", p.name);
        }
        assert_eq!(b.age, LIFE_TICKS);
    }

    #[test]
    fn mitte_bleibt_beim_drehen_stehen() {
        let mut b = burst();
        b.pieces[0].angle = 1.3;
        b.pieces[0].prev_angle = 1.3;
        let pose = b.pose(1.0);
        let p = &b.pieces[0];
        let pp = pose.get("head");
        let r = pp.rotation_matrix.unwrap();
        // Pivot + Versatz + gedrehte Kastenmitte = die simulierte Mitte.
        let rc = r.then(Affine::translation(p.local_center)).t;
        for i in 0..3 {
            assert!((p.pivot[i] + pp.pivot_offset[i] + rc[i] - p.pos[i]).abs() < 1e-4);
        }
    }

    #[test]
    fn boden_tiefer_wenn_in_der_luft() {
        let b = Burst::new([0; 16], (0.0, 64.0, 0.0), 0.0, 1.0, 0.9375, [None; 4], 2.0, 7);
        assert!((b.ground - (FEET_Y + 2.0 * 16.0 / 0.9375)).abs() < 1e-4);
    }
}
