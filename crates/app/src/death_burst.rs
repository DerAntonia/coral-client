// Todesanimation Explosion (Video > Tod, DEATH_ANIM_EXPLOSION): stirbt ein anderer Spieler, faellt er nicht
// wie in Vanilla um (LivingEntityRenderer.setupRotations, deathTime), sondern fliegt in seine
// sechs Koerperteile auseinander. Die Leiche selbst wird nicht mehr gezeichnet.
//
// Zur Explosion gehoert nur der Skin, keine Ruestung und nichts, was der Spieler hielt. Jedes
// Teil ist ein starrer Quader in der Groesse, die man sieht (Skin samt Aussenlage): er fliegt im Bogen mit echter Fallbeschleunigung (9,81 m/s^2, ein Block
// = ein Meter), dreht sich um eine zufaellige Achse und stoesst mit seinen Ecken gegen die
// Kollisionsboxen der Welt -- mit Rueckprall, Reibung und dem Drehmoment, das ein Stoss an
// einer Ecke erzeugt. Kommt er zur Ruhe, kippt er auf die naechste Seite und liegt dann flach
// auf: eine Seite genau waagerecht, keine Ecke im Boden. Liegt es still, blendet es in
// FADE_TICKS aus. Was den Boden nie erreicht (Abgrund), fliegt bis AIR_TICKS weiter und blendet
// dann aus.
//
// Gerechnet wird in Bloecken relativ zu den Fuessen beim Tod (y nach oben), im Takt der
// Spielticks mit SUBSTEPS Unterschritten; gezeichnet wird zwischen zwei Ticks interpoliert und
// dafuer in den Modellraum des Spielers (Pixel, y nach unten) zurueckgerechnet.

use glam::{Mat3, Quat, Vec3};
use world::entity_model::{Affine, Model, Part, PartPose, Pose};

// Bis hierhin hat sich jedes Teil mit Kontakt hingelegt (s. SETTLE_BEFORE_END).
const LIFE_TICKS: u32 = 60;
// So lange (10 Sekunden) darf ein Teil ohne Boden fliegen, dann blendet es aus.
const AIR_TICKS: u32 = 200;
// Bloecke je Tick^2: 9,81 m/s^2 bei 20 Ticks je Sekunde.
const GRAVITY: f32 = 9.81 / 400.0;
// Luftwiderstand je Tick (Weg und Drehung), fuer so kleine, schwere Teile fast nichts.
const AIR_DRAG: f32 = 0.995;
// Rollwiderstand je Tick mit Bodenkontakt: ohne ihn rollte ein Kopf ewig weiter.
const ROLL_DRAG: f32 = 0.85;
const SUBSTEPS: u32 = 8;
// Darunter gilt ein Stoss als Aufliegen, nicht als Aufprall: kein Rueckprall (sonst zittert ein
// liegendes Teil). Bloecke je Tick, etwa 0,6 m/s.
const REST_SPEED: f32 = 0.03;
// So ruhig (Bloecke bzw. Radiant je Tick) und so viele Ticks lang mit Kontakt: das Teil legt
// sich hin.
const CALM_SPEED: f32 = 0.015;
const CALM_SPIN: f32 = 0.08;
const CALM_TICKS: u32 = 3;
// Spaetestens so viele Ticks vor dem Ende legt sich jedes Teil mit Kontakt hin.
const SETTLE_BEFORE_END: u32 = 20;
// Anteil der Restdrehung bis zur flachen Lage je Tick beim Hinlegen.
const SETTLE_RATE: f32 = 0.4;
// So tief unter einem Teil wird noch nach Boden gesucht (Bloecke).
const SUPPORT_DEPTH: f32 = 8.0;
// Ein Teil blendet in so vielen Ticks (eine Sekunde) ganz aus.
const FADE_TICKS: u32 = 20;

// Die sechs Teile, die auseinanderfliegen (BipedEntityModel).
const PART_NAMES: [&str; 6] = ["head", "body", "right_arm", "left_arm", "right_leg", "left_leg"];
// Von hier aus fliegen die Teile weg: etwa die Hueften (Modellpixel).
const CENTER: [f32; 3] = [0.0, 12.0, 0.0];

// Ein Teil in Modellpixeln: Gelenk und der Kasten um alles, was daran haengt, relativ zum Gelenk.
#[derive(Clone, Copy, Debug)]
pub struct PartBox {
    pub name: &'static str,
    pub pivot: [f32; 3],
    pub min: [f32; 3],
    pub max: [f32; 3],
}

// Die Kaesten der sechs Teile ueber alle `models` zusammen (das Spielermodell zuerst): je Teil die Huelle aller seiner Quader samt Dilation und Kindern
// (Hut, Jacke, Aermel, Hosenbeine). Das Gelenk kommt aus dem ersten Modell mit diesem Teil.
pub fn part_boxes(models: &[&Model]) -> Vec<PartBox> {
    fn finde<'a>(parts: &'a [Part], name: &str) -> Option<&'a Part> {
        parts.iter().find_map(|p| if p.name == name { Some(p) } else { finde(&p.children, name) })
    }
    fn huelle(part: &Part, versatz: [f32; 3], min: &mut [f32; 3], max: &mut [f32; 3]) {
        for c in &part.cuboids {
            for i in 0..3 {
                min[i] = min[i].min(versatz[i] + c.offset[i] - c.dilation[i]);
                max[i] = max[i].max(versatz[i] + c.offset[i] + c.size[i] + c.dilation[i]);
            }
        }
        for kind in &part.children {
            huelle(kind, [versatz[0] + kind.pivot[0], versatz[1] + kind.pivot[1], versatz[2] + kind.pivot[2]], min, max);
        }
    }
    PART_NAMES
        .iter()
        .filter_map(|&name| {
            let pivot = models.iter().find_map(|m| finde(&m.parts, name))?.pivot;
            let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
            for m in models {
                if let Some(p) = finde(&m.parts, name) {
                    huelle(p, [p.pivot[0] - pivot[0], p.pivot[1] - pivot[1], p.pivot[2] - pivot[2]], &mut min, &mut max);
                }
            }
            (min[0] <= max[0]).then_some(PartBox { name, pivot, min, max })
        })
        .collect()
}

// Eine Kollisionsbox der Welt relativ zu den Fuessen.
#[derive(Clone, Copy)]
struct Kiste {
    min: Vec3,
    max: Vec3,
}

impl Kiste {
    fn enthaelt(&self, p: Vec3) -> bool {
        p.cmpgt(self.min).all() && p.cmplt(self.max).all()
    }
}

struct Piece {
    name: &'static str,
    // Modellpixel: Gelenk und Kastenmitte relativ zum Gelenk.
    pivot: Vec3,
    local_center: Vec3,
    // Halbe Kantenlaengen in Bloecken (Achsen des Teils) und die umgekehrte Traegheit je
    // Achse (Masse 1, Quader: I = (b^2 + c^2) / 3 aus den halben Kanten).
    half: Vec3,
    inv_inertia: Vec3,
    // Mitte (Bloecke relativ zu den Fuessen) und Lage: Achsen des Teils -> Welt.
    pos: Vec3,
    prev_pos: Vec3,
    rot: Quat,
    prev_rot: Quat,
    // Bloecke bzw. Radiant (Weltachsen) je Tick.
    vel: Vec3,
    omega: Vec3,
    restitution: f32,
    friction: f32,
    calm: u32,
    // Legt sich gerade hin (keine Physik mehr, nur noch Kippen und Absetzen).
    settling: bool,
    // Liegt flach, aendert sich nicht mehr.
    asleep: bool,
    // Ganze Ticks, seit es ausblendet (still liegt oder AIR_TICKS ohne Boden flog).
    fade: Option<u32>,
}

pub struct Burst {
    // Das gestorbene Entity (Schluessel fuer den Motion Blur).
    pub entity_id: i32,
    pub uuid: [u8; 16],
    // Fuesse in Weltkoordinaten, Koerperdrehung und Groesse beim Tod.
    pub feet: (f64, f64, f64),
    pub yaw: f32,
    pub scale: f32,
    age: u32,
    // Modellraum (Bloecke = Pixel / 16) -> Welt relativ zu den Fuessen, und seine reine Drehung.
    basis: Affine,
    axes: Mat3,
    // Waagerechte Richtung (normiert) von den Fuessen zur Quelle des Kills (Schlagender,
    // Schuetze, Explosion), wenn er eine hat: dann ist nur die Haelfte der Richtungen erlaubt,
    // die von ihr weg zeigt -- kein Teil fliegt, prallt oder rollt je auf sie zu.
    away: Option<Vec3>,
    pieces: Vec<Piece>,
}

// Den Anteil von `v` entlang `d` (waagerecht, normiert) wegnehmen, wenn er zu `d` hin zeigt.
fn ohne_anteil_zu(v: Vec3, d: Vec3) -> Vec3 {
    let t = v.dot(d);
    if t > 0.0 {
        v - d * t
    } else {
        v
    }
}

// Kleiner Zufall ohne Abhaengigkeit (xorshift), je Explosion neu gesaet.
struct Rng(u64);

impl Rng {
    // Den Seed erst durchmischen (splitmix64): benachbarte Seeds ergaeben sonst fast dieselbe
    // Explosion, und xorshift darf nie bei 0 starten.
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)).max(1))
    }

    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }

    // Gleichverteilte Richtung auf der Kugel.
    fn unit(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, std::f32::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), z, r * a.sin())
    }
}

// Affine (Zeilen) aus einer glam-Matrix (Spalten).
fn affine_of(m: Mat3) -> Affine {
    let c = [m.x_axis, m.y_axis, m.z_axis];
    Affine { m: [[c[0].x, c[1].x, c[2].x], [c[0].y, c[1].y, c[2].y], [c[0].z, c[1].z, c[2].z]], t: [0.0; 3] }
}

fn mat_of(a: Affine) -> Mat3 {
    Mat3::from_cols(Vec3::new(a.m[0][0], a.m[1][0], a.m[2][0]), Vec3::new(a.m[0][1], a.m[1][1], a.m[2][1]), Vec3::new(a.m[0][2], a.m[1][2], a.m[2][2]))
}

const ECKEN: [Vec3; 8] = [
    Vec3::new(-1.0, -1.0, -1.0),
    Vec3::new(1.0, -1.0, -1.0),
    Vec3::new(-1.0, 1.0, -1.0),
    Vec3::new(1.0, 1.0, -1.0),
    Vec3::new(-1.0, -1.0, 1.0),
    Vec3::new(1.0, -1.0, 1.0),
    Vec3::new(-1.0, 1.0, 1.0),
    Vec3::new(1.0, 1.0, 1.0),
];

impl Piece {
    fn corners(&self) -> [Vec3; 8] {
        ECKEN.map(|e| self.rot * (e * self.half))
    }

    fn inv_inertia_world(&self) -> Mat3 {
        let r = Mat3::from_quat(self.rot);
        r * Mat3::from_diagonal(self.inv_inertia) * r.transpose()
    }

    // Ecken in den Kisten: herausschieben und den Stoss auf Weg und Drehung geben. true bei
    // Kontakt.
    fn resolve(&mut self, kisten: &[Kiste]) -> bool {
        let mut kontakt = false;
        let inv_i = self.inv_inertia_world();
        for r in self.corners() {
            let p = self.pos + r;
            let Some((n, tiefe)) = eindringen(p, kisten) else { continue };
            kontakt = true;
            self.pos += n * tiefe;
            let vc = self.vel + self.omega.cross(r);
            let vn = vc.dot(n);
            if vn >= 0.0 {
                continue;
            }
            let e = if -vn < REST_SPEED { 0.0 } else { self.restitution };
            let k = 1.0 + n.dot((inv_i * r.cross(n)).cross(r));
            let j = -(1.0 + e) * vn / k;
            self.vel += n * j;
            self.omega += inv_i * r.cross(n * j);
            // Reibung entlang der Flaeche, hoechstens friction * Normalstoss (Coulomb).
            let vc = self.vel + self.omega.cross(r);
            let vt = vc - n * vc.dot(n);
            let lt = vt.length();
            if lt > 1e-6 {
                let t = vt / lt;
                let kt = 1.0 + t.dot((inv_i * r.cross(t)).cross(r));
                let jt = (lt / kt).min(self.friction * j);
                self.vel -= t * jt;
                self.omega -= inv_i * r.cross(t * jt);
            }
        }
        kontakt
    }

    // Abstand der tiefsten Ecke zu ihrer Unterlage (darunter liegende Oberseite einer Kiste),
    // None ohne Unterlage unter allen Ecken.
    fn gap(&self, kisten: &[Kiste]) -> Option<f32> {
        self.corners()
            .into_iter()
            .filter_map(|r| {
                let p = self.pos + r;
                kisten
                    .iter()
                    .filter(|k| p.x > k.min.x && p.x < k.max.x && p.z > k.min.z && p.z < k.max.z && k.max.y <= p.y + 0.02 && k.max.y >= p.y - SUPPORT_DEPTH)
                    .map(|k| p.y - k.max.y)
                    .reduce(f32::min)
            })
            .reduce(f32::min)
    }

    // Die flache Lage, die der jetzigen am naechsten ist: die Achse des Teils, die am
    // steilsten steht, genau senkrecht.
    fn flat_target(&self) -> Quat {
        let m = Mat3::from_quat(self.rot);
        let achse = [m.x_axis, m.y_axis, m.z_axis].into_iter().max_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap_or(Vec3::Y);
        let achse = if achse.y < 0.0 { -achse } else { achse };
        (Quat::from_rotation_arc(achse.normalize(), Vec3::Y) * self.rot).normalize()
    }
}

// Wie tief und in welche Richtung `p` in einer Kiste steckt: die Seite mit dem kuerzesten Weg
// hinaus, deren Ausgang nicht gleich in der naechsten Kiste liegt (zwischen zwei Bodenbloecken
// soll eine Ecke nach oben, nicht in den Nachbarblock).
fn eindringen(p: Vec3, kisten: &[Kiste]) -> Option<(Vec3, f32)> {
    let k = kisten.iter().find(|k| k.enthaelt(p))?;
    let mut wege = [
        (Vec3::Y, k.max.y - p.y),
        (Vec3::NEG_Y, p.y - k.min.y),
        (Vec3::X, k.max.x - p.x),
        (Vec3::NEG_X, p.x - k.min.x),
        (Vec3::Z, k.max.z - p.z),
        (Vec3::NEG_Z, p.z - k.min.z),
    ];
    wege.sort_by(|a, b| a.1.total_cmp(&b.1));
    wege.into_iter().find(|(n, t)| !kisten.iter().any(|k| k.enthaelt(p + *n * (*t + 1e-4)))).or(Some((Vec3::Y, k.max.y - p.y)))
}

impl Burst {
    // `parts`: die Kaesten aus part_boxes; `away_from`: (x, z) der Quelle des Kills, wenn er
    // eine Richtung hat; `seed`: Zufall dieser Explosion.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        entity_id: i32,
        uuid: [u8; 16],
        feet: (f64, f64, f64),
        yaw: f32,
        scale: f32,
        parts: &[PartBox],
        away_from: Option<(f64, f64)>,
        seed: u64,
    ) -> Self {
        let mut rng = Rng::new(seed);
        let basis = world::entity_model::entity_transform([0.0; 3], yaw, scale, world::entity_model::PLAYER_MODEL_SCALE).then(Affine::scale([1.0 / 16.0; 3]));
        let bm = mat_of(basis);
        let px = bm.x_axis.length();
        let axes = bm * (1.0 / px);
        let start = Quat::from_mat3(&axes).normalize();
        let welt = |m: Vec3| Vec3::from(basis.apply(m.to_array()));
        // Mitte des Knalls: die Hueften, etwas verrueckt, und ein gemeinsamer Stoss zur Seite.
        let knall = welt(Vec3::from(CENTER)) + Vec3::new(rng.range(-0.15, 0.15), rng.range(-0.1, 0.05), rng.range(-0.15, 0.15));
        let away = away_from
            .map(|(x, z)| Vec3::new((x - feet.0) as f32, 0.0, (z - feet.2) as f32))
            .filter(|d| d.length() > 1e-3)
            .map(Vec3::normalize);
        // Der gemeinsame Stoss: zufaellig, bei einem Kill mit Richtung von der Quelle weg (bis
        // 60 Grad zur Seite), wie ein Rueckstoss.
        let schub_winkel = match away {
            Some(d) => (-d.z).atan2(-d.x) + rng.range(-1.05, 1.05),
            None => rng.range(0.0, std::f32::consts::TAU),
        };
        let schub = Vec3::new(schub_winkel.cos(), 0.0, schub_winkel.sin()) * rng.range(0.0, 1.2) / 20.0;
        let pieces = parts
            .iter()
            .map(|b| {
                let (min, max) = (Vec3::from(b.min), Vec3::from(b.max));
                let local_center = (min + max) * 0.5;
                let pivot = Vec3::from(b.pivot);
                let half = (max - min) * 0.5 * px;
                let inv_inertia = Vec3::new(3.0 / (half.y * half.y + half.z * half.z), 3.0 / (half.x * half.x + half.z * half.z), 3.0 / (half.x * half.x + half.y * half.y));
                let pos = welt(pivot + local_center);
                // Weg vom Knall, waagerecht mit Streuung, und nach oben (m/s, dann je Tick).
                let weg = pos - knall;
                let flach = Vec3::new(weg.x, 0.0, weg.z);
                let flach = if flach.length() > 1e-3 { flach.normalize() } else { Vec3::X };
                let drehen = rng.range(-0.45, 0.45);
                let (s, c) = drehen.sin_cos();
                let flach = Vec3::new(flach.x * c - flach.z * s, 0.0, flach.x * s + flach.z * c);
                let hoch = rng.range(2.5, 5.5) + if b.name == "head" { 1.0 } else { 0.0 };
                // Zeigt die Richtung zur Quelle hin, an der Trennlinie spiegeln: die Streuung
                // bleibt, nur auf der abgewandten Seite.
                let flach = match away {
                    Some(d) if flach.dot(d) > 0.0 => flach - d * (2.0 * flach.dot(d)),
                    _ => flach,
                };
                let vel = (flach * rng.range(1.5, 4.0) + Vec3::Y * hoch) / 20.0 + schub;
                let omega = rng.unit() * rng.range(4.0, 15.0) / 20.0;
                Piece {
                    name: b.name,
                    pivot,
                    local_center,
                    half,
                    inv_inertia,
                    pos,
                    prev_pos: pos,
                    rot: start,
                    prev_rot: start,
                    vel,
                    omega,
                    restitution: rng.range(0.15, 0.35),
                    friction: rng.range(0.45, 0.75),
                    calm: 0,
                    settling: false,
                    asleep: false,
                    fade: None,
                }
            })
            .collect();
        Self { entity_id, uuid, feet, yaw, scale, age: 0, basis, axes, away, pieces }
    }

    // Ein Spieltick. `boxes(min, max)`: die Kollisionsboxen der Welt in diesem Bereich (absolute
    // Weltkoordinaten). false, wenn die Explosion vorbei ist.
    pub fn tick(&mut self, boxes: &dyn Fn([f64; 3], [f64; 3]) -> Vec<([f64; 3], [f64; 3])>) -> bool {
        self.age += 1;
        let fuss = glam::DVec3::new(self.feet.0, self.feet.1, self.feet.2);
        let spaet = self.age + SETTLE_BEFORE_END >= LIFE_TICKS;
        let away = self.away;
        let age = self.age;
        for p in &mut self.pieces {
            p.prev_pos = p.pos;
            p.prev_rot = p.rot;
            if let Some(f) = p.fade.as_mut() {
                *f = f.saturating_add(1);
            }
            // Ohne Boden: nach AIR_TICKS ausblenden, waehrend es weiterfaellt.
            if p.fade.is_none() && age >= AIR_TICKS {
                p.fade = Some(0);
            }
            if p.asleep {
                continue;
            }
            // Die Kisten um das Teil herum, so weit es in diesem Tick kommen kann, und unter ihm
            // tief genug fuer die Unterlage.
            let reichweite = p.half.length() + p.vel.length() + 0.1;
            let mitte = fuss + p.pos.as_dvec3();
            let r = f64::from(reichweite);
            let min = mitte - glam::DVec3::new(r, r + f64::from(SUPPORT_DEPTH), r);
            let max = mitte + glam::DVec3::splat(r);
            let kisten: Vec<Kiste> = boxes(min.to_array(), max.to_array())
                .into_iter()
                .map(|(a, b)| Kiste { min: (glam::DVec3::from(a) - fuss).as_vec3(), max: (glam::DVec3::from(b) - fuss).as_vec3() })
                .collect();
            if p.settling {
                settle(p, &kisten);
                continue;
            }
            let dt = 1.0 / SUBSTEPS as f32;
            let mut kontakt = false;
            for _ in 0..SUBSTEPS {
                p.vel.y -= GRAVITY * dt;
                p.pos += p.vel * dt;
                p.rot = (Quat::from_scaled_axis(p.omega * dt) * p.rot).normalize();
                kontakt |= p.resolve(&kisten);
                // Auch Abpraller und Rollen duerfen nicht zur Quelle fuehren.
                if let Some(d) = away {
                    p.vel = ohne_anteil_zu(p.vel, d);
                }
            }
            p.vel *= AIR_DRAG;
            p.omega *= if kontakt { ROLL_DRAG } else { AIR_DRAG };
            p.calm = if kontakt && p.vel.length() < CALM_SPEED && p.omega.length() < CALM_SPIN { p.calm + 1 } else { 0 };
            if p.calm >= CALM_TICKS || (kontakt && spaet) {
                p.settling = true;
            }
        }
        self.pieces.iter().any(|p| p.fade.is_none_or(|f| f < FADE_TICKS))
    }

    // Deckkraft je Teil zwischen zwei Ticks: 1, bis es ausblendet, dann in FADE_TICKS auf 0.
    pub fn alphas(&self, progress: f32) -> Vec<(&'static str, f32)> {
        self.pieces
            .iter()
            .map(|p| (p.name, p.fade.map_or(1.0, |f| (1.0 - (f as f32 + progress) / FADE_TICKS as f32).clamp(0.0, 1.0))))
            .collect()
    }

    // Die Pose zwischen dem letzten und dem naechsten Tick (`progress` 0..1): jedes Teil dreht
    // sich um seine eigene Mitte, nicht um sein Gelenk.
    #[cfg(test)]
    pub fn pose(&self, progress: f32) -> Pose {
        self.pose_of(progress, &|_| true)
    }

    // Wie pose, aber nur die Teile, fuer die `zeigen(name)` gilt; die anderen sind ausgeblendet.
    pub fn pose_of(&self, progress: f32, zeigen: &dyn Fn(&str) -> bool) -> Pose {
        let mut pose = Pose::new();
        let zurueck = self.basis.inverse();
        for p in &self.pieces {
            let mitte = p.prev_pos.lerp(p.pos, progress);
            let rot = p.prev_rot.slerp(p.rot, progress);
            // Drehung im Modellraum: Achsen des Modells -> Achsen des Teils jetzt.
            let r = self.axes.transpose() * Mat3::from_quat(rot);
            let mitte_px = Vec3::from(zurueck.apply(mitte.to_array()));
            let offset = mitte_px - p.pivot - r * p.local_center;
            pose.set(p.name, PartPose { pivot_offset: offset.to_array(), rotation_matrix: Some(affine_of(r)), hidden: !zeigen(p.name), ..Default::default() });
        }
        pose
    }
}

// Hinlegen: zur naechsten flachen Lage kippen, dann auf die Unterlage absetzen. Ohne Unterlage
// (Rand eines Abgrunds) bleibt es, wo es ist, bis die Explosion vorbei ist.
fn settle(p: &mut Piece, kisten: &[Kiste]) {
    let ziel = p.flat_target();
    let rest = p.rot.angle_between(ziel);
    p.rot = if rest < 0.2f32.to_radians() { ziel } else { p.rot.slerp(ziel, SETTLE_RATE).normalize() };
    // Was beim Kippen in Boden oder Wand geraten ist, erst hinausschieben, dann bis auf die
    // Unterlage absenken.
    p.resolve(kisten);
    p.vel = Vec3::ZERO;
    p.omega = Vec3::ZERO;
    if let Some(abstand) = p.gap(kisten) {
        p.pos.y -= abstand;
    }
    if p.rot == ziel {
        p.asleep = true;
        p.fade.get_or_insert(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ein ebener Boden aus Vollbloecken, Oberseite bei y = 64.
    fn boden(min: [f64; 3], max: [f64; 3]) -> Vec<([f64; 3], [f64; 3])> {
        let mut out = Vec::new();
        for x in min[0].floor() as i32..=max[0].floor() as i32 {
            for z in min[2].floor() as i32..=max[2].floor() as i32 {
                for y in min[1].floor() as i32..=(max[1].floor() as i32).min(63) {
                    out.push(([f64::from(x), f64::from(y), f64::from(z)], [f64::from(x + 1), f64::from(y + 1), f64::from(z + 1)]));
                }
            }
        }
        out
    }

    fn spieler() -> Vec<PartBox> {
        let m = world::entity_model::player_model(false);
        part_boxes(&[&m])
    }

    fn burst(seed: u64) -> Burst {
        Burst::new(1, [0; 16], (0.5, 64.0, 0.5), 30.0, 1.0, &spieler(), None, seed)
    }

    fn ecken_welt(b: &Burst, p: &Piece) -> Vec<Vec3> {
        let fuss = Vec3::new(b.feet.0 as f32, b.feet.1 as f32, b.feet.2 as f32);
        p.corners().iter().map(|r| fuss + p.pos + *r).collect()
    }

    #[test]
    fn kaesten_mit_aussenlage() {
        let k = spieler();
        assert_eq!(k.len(), 6);
        let kopf = k.iter().find(|b| b.name == "head").unwrap();
        // 8 Pixel Kopf plus Hut mit Dilation 0,5 je Seite.
        assert!((kopf.max[0] - kopf.min[0] - 9.0).abs() < 1e-4, "{kopf:?}");
    }

    #[test]
    fn pose_am_anfang_wie_im_stand() {
        let b = burst(3);
        let pose = b.pose(0.0);
        for p in &b.pieces {
            let pp = pose.get(p.name);
            for i in 0..3 {
                assert!(pp.pivot_offset[i].abs() < 1e-3, "{} versetzt: {:?}", p.name, pp.pivot_offset);
            }
            let m = pp.rotation_matrix.unwrap().m;
            for (i, zeile) in m.iter().enumerate() {
                for (j, w) in zeile.iter().enumerate() {
                    assert!((w - if i == j { 1.0 } else { 0.0 }).abs() < 1e-4, "{} gedreht", p.name);
                }
            }
        }
    }

    // Viele Zufaelle: alle Teile fliegen erst hoch, landen und liegen am Ende flach auf dem
    // Boden, keine Ecke darin, und jedes hat eine Seite genau waagerecht.
    #[test]
    fn teile_landen_flach_und_nicht_im_boden() {
        for seed in 1..40 {
            let mut b = burst(seed);
            let start: Vec<f32> = b.pieces.iter().map(|p| p.pos.y).collect();
            for _ in 0..4 {
                b.tick(&boden);
            }
            assert!(b.pieces.iter().zip(&start).all(|(p, s)| p.pos.y > *s), "seed {seed}: nicht hochgeflogen");
            while b.tick(&boden) {}
            for p in &b.pieces {
                assert!(p.asleep, "seed {seed}: {} liegt nicht still", p.name);
                let ecken = ecken_welt(&b, p);
                let tiefste = ecken.iter().map(|e| e.y).fold(f32::MAX, f32::min);
                assert!((tiefste - 64.0).abs() < 2e-3, "seed {seed}: {} tiefste Ecke bei {tiefste}", p.name);
                // Flach: vier Ecken auf dem Boden.
                let unten = ecken.iter().filter(|e| (e.y - 64.0).abs() < 2e-3).count();
                assert_eq!(unten, 4, "seed {seed}: {} liegt nicht flach", p.name);
                let m = Mat3::from_quat(p.rot);
                let steil = [m.x_axis, m.y_axis, m.z_axis].iter().map(|a| a.y.abs()).fold(0.0, f32::max);
                assert!((steil - 1.0).abs() < 1e-5, "seed {seed}: {} Achse nicht senkrecht", p.name);
            }
        }
    }

    // Kill mit Richtung: in keinem Tick bewegt sich ein Teil waagerecht auf die Quelle zu, und
    // am Ende liegt keins auf ihrer Seite.
    #[test]
    fn nichts_fliegt_zum_schlagenden() {
        for seed in 1..40 {
            for (x, z) in [(3.5, 0.5), (0.5, -2.0), (-1.5, 2.5)] {
                let mut b = Burst::new(1, [0; 16], (0.5, 64.0, 0.5), (seed * 37) as f32, 1.0, &spieler(), Some((x, z)), seed);
                let d = Vec3::new(x as f32 - 0.5, 0.0, z as f32 - 0.5).normalize();
                let start: Vec<f32> = b.pieces.iter().map(|p| p.pos.dot(d)).collect();
                while b.tick(&boden) {
                    for p in &b.pieces {
                        let schritt = (p.pos - p.prev_pos).dot(d);
                        assert!(schritt < 0.02, "seed {seed}: {} {schritt} auf den Spieler zu", p.name);
                    }
                }
                for (p, s) in b.pieces.iter().zip(&start) {
                    assert!(p.pos.dot(d) < *s + 0.05, "seed {seed}: {} landet auf der Seite des Spielers", p.name);
                }
            }
        }
    }

    // Still liegende Teile blenden in einer Sekunde aus; erst wenn alle weg sind, ist die
    // Explosion vorbei.
    #[test]
    fn liegende_teile_blenden_aus() {
        for seed in 1..20 {
            let mut b = burst(seed);
            let mut gesehen = false;
            while b.tick(&boden) {
                for (p, (name, a)) in b.pieces.iter().zip(b.alphas(0.5)) {
                    assert_eq!(p.name, name);
                    if p.asleep {
                        let soll = 1.0 - (p.fade.unwrap() as f32 + 0.5) / FADE_TICKS as f32;
                        assert!((a - soll.clamp(0.0, 1.0)).abs() < 1e-6, "seed {seed}: {name} {a}");
                        gesehen |= a > 0.0 && a < 1.0;
                    } else {
                        assert_eq!(a, 1.0, "seed {seed}: {name} blendet im Flug aus");
                    }
                }
            }
            assert!(gesehen, "seed {seed}: nichts ausgeblendet");
            assert!(b.alphas(0.0).iter().all(|(_, a)| *a == 0.0), "seed {seed}: am Ende noch sichtbar");
        }
    }

    // Ohne Boden fliegt alles 10 Sekunden weiter, blendet dann in einer Sekunde aus, und
    // danach ist die Explosion vorbei.
    #[test]
    fn ohne_boden_zehn_sekunden() {
        let mut b = burst(4);
        let nichts = |_: [f64; 3], _: [f64; 3]| Vec::new();
        let mut ticks = 0;
        while b.tick(&nichts) {
            ticks += 1;
            let a = b.alphas(0.0);
            if b.age < AIR_TICKS {
                assert!(a.iter().all(|(_, a)| *a == 1.0), "Tick {}: blendet zu frueh aus", b.age);
            }
        }
        assert_eq!(ticks + 1, AIR_TICKS + FADE_TICKS, "Ende nach {} Ticks", ticks + 1);
    }

    #[test]
    fn nachbar_seeds_verschieden() {
        let (a, b) = (burst(12), burst(13));
        assert!(a.pieces.iter().zip(&b.pieces).any(|(x, y)| (x.vel - y.vel).length() > 1e-3));
    }

    #[test]
    fn mitte_bleibt_beim_drehen_stehen() {
        let mut b = burst(5);
        for _ in 0..7 {
            b.tick(&boden);
        }
        let pose = b.pose(1.0);
        let zurueck = b.basis.inverse();
        for p in &b.pieces {
            let pp = pose.get(p.name);
            let r = mat_of(pp.rotation_matrix.unwrap());
            let mitte = p.pivot + Vec3::from(pp.pivot_offset) + r * p.local_center;
            let soll = Vec3::from(zurueck.apply(p.pos.to_array()));
            assert!((mitte - soll).length() < 1e-3, "{}", p.name);
        }
    }

    #[test]
    fn fallen_auf_tieferen_boden() {
        // Starb zwei Bloecke ueber dem Boden: die Teile landen unten, nicht in der Luft.
        let mut b = Burst::new(1, [0; 16], (0.5, 66.0, 0.5), 0.0, 1.0, &spieler(), None, 9);
        while b.tick(&boden) {}
        for p in &b.pieces {
            let tiefste = ecken_welt(&b, p).iter().map(|e| e.y).fold(f32::MAX, f32::min);
            assert!((tiefste - 64.0).abs() < 2e-3, "{} bei {tiefste}", p.name);
        }
    }
}
