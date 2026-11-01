// Schleim, Vierbeiner, Schwein, Kuh, Schaf, Beinbewegung, Koerperdrehung und Umhang-Zustand.

use super::*;

// SlimeEntityModel: Aussenhuelle (8er-Kasten) plus Innenkern mit Augen und Mund.
#[must_use] 
pub fn slime_model(texture: &'static str) -> Model {
    Model {
        texture,
        tex_size: [64.0, 32.0],
        parts: vec![
            Part::new("cube", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 16.0], [-3.0, 17.0, -3.0], [6.0, 6.0, 6.0])]),
            Part::new("right_eye", [0.0, 0.0, 0.0], vec![Cuboid::new([32.0, 0.0], [-3.25, 18.0, -3.5], [2.0, 2.0, 2.0])]),
            Part::new("left_eye", [0.0, 0.0, 0.0], vec![Cuboid::new([32.0, 4.0], [1.25, 18.0, -3.5], [2.0, 2.0, 2.0])]),
            Part::new("mouth", [0.0, 0.0, 0.0], vec![Cuboid::new([32.0, 8.0], [0.0, 21.0, -3.5], [1.0, 1.0, 1.0])]),
            // Aussenhuelle zuletzt -- sie ist durchscheinend und muss ueber dem Kern liegen.
            Part::new("outer_cube", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-4.0, 16.0, -4.0], [8.0, 8.0, 8.0])]),
        ],
    }
}

// QuadrupedEntityModel.getModelData(stanceWidth, ...) -- Rumpf/Beine, den Kopf setzt die
// jeweilige Tierart selbst (Kuh und Schwein haben verschiedene Schnauzen).
pub(crate) fn quadruped_parts(stance: f32, head: Part, body: Part) -> Vec<Part> {
    let leg = |name: &'static str, x: f32, z: f32, mirror: bool| {
        let mut cuboid = Cuboid::new([0.0, 16.0], [-2.0, 0.0, -2.0], [4.0, stance, 4.0]);
        if mirror {
            cuboid = cuboid.mirrored();
        }
        Part::new(name, [x, 24.0 - stance, z], vec![cuboid])
    };
    vec![
        head,
        body,
        leg("right_hind_leg", -3.0, 7.0, false),
        leg("left_hind_leg", 3.0, 7.0, true),
        leg("right_front_leg", -3.0, -5.0, false),
        leg("left_front_leg", 3.0, -5.0, true),
    ]
}

// PigEntityModel: Quadruped mit stanceWidth 6 und eigener Schnauze.
//
// Textur: Kuehe und Schweine haben seit 1.21.5 Klima-Varianten (temperate/warm/cold), die an
// einer Entity-Datenkomponente haengen, die dieser Client nicht liest. Genommen wird die
// temperate-Variante -- Vanillas Standard. Ein Pfad "entity/pig/pig" existiert NICHT mehr;
// der Installations-Test hat genau das aufgedeckt (s. tests/entity_atlas_smoke.rs).
#[must_use] 
pub fn pig_model() -> Model {
    let head = Part::new(
        "head",
        [0.0, 12.0, -6.0],
        vec![Cuboid::new([0.0, 0.0], [-4.0, -4.0, -8.0], [8.0, 8.0, 8.0]), Cuboid::new([16.0, 16.0], [-2.0, 0.0, -9.0], [4.0, 3.0, 1.0])],
    );
    let body = Part::new("body", [0.0, 11.0, 2.0], vec![Cuboid::new([28.0, 8.0], [-5.0, -10.0, -7.0], [10.0, 16.0, 8.0])]).rotated([
        std::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    ]);
    Model { texture: "entity/pig/temperate_pig", tex_size: [64.0, 64.0], parts: quadruped_parts(6.0, head, body) }
}

// CowEntityModel: eigene Beine (12 lang, ±4) und ein Kopf mit Hoernern.
#[must_use] 
pub fn cow_model(texture: &'static str) -> Model {
    let leg = |name: &'static str, x: f32, z: f32, mirror: bool| {
        let mut cuboid = Cuboid::new([0.0, 16.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]);
        if mirror {
            cuboid = cuboid.mirrored();
        }
        Part::new(name, [x, 12.0, z], vec![cuboid])
    };
    Model {
        texture,
        tex_size: [64.0, 64.0],
        parts: vec![
            Part::new(
                "head",
                [0.0, 4.0, -8.0],
                vec![
                    Cuboid::new([0.0, 0.0], [-4.0, -4.0, -6.0], [8.0, 8.0, 6.0]),
                    Cuboid::new([1.0, 33.0], [-3.0, 1.0, -7.0], [6.0, 3.0, 1.0]),
                    Cuboid::new([22.0, 0.0], [-5.0, -5.0, -5.0], [1.0, 3.0, 1.0]),
                    Cuboid::new([22.0, 0.0], [4.0, -5.0, -5.0], [1.0, 3.0, 1.0]),
                ],
            ),
            Part::new(
                "body",
                [0.0, 5.0, 2.0],
                vec![
                    Cuboid::new([18.0, 4.0], [-6.0, -10.0, -7.0], [12.0, 18.0, 10.0]),
                    Cuboid::new([52.0, 0.0], [-2.0, 2.0, -8.0], [4.0, 6.0, 1.0]),
                ],
            )
            .rotated([std::f32::consts::FRAC_PI_2, 0.0, 0.0]),
            leg("right_hind_leg", -4.0, 7.0, false),
            leg("left_hind_leg", 4.0, 7.0, true),
            leg("right_front_leg", -4.0, -5.0, false),
            leg("left_front_leg", 4.0, -5.0, true),
        ],
    }
}

// SheepEntityModel: Quadruped mit stanceWidth 12, eigenem (kleinerem) Kopf und Rumpf.
#[must_use] 
pub fn sheep_model() -> Model {
    let head = Part::new("head", [0.0, 6.0, -8.0], vec![Cuboid::new([0.0, 0.0], [-3.0, -4.0, -6.0], [6.0, 6.0, 8.0])]);
    let body = Part::new("body", [0.0, 5.0, 2.0], vec![Cuboid::new([28.0, 8.0], [-4.0, -10.0, -7.0], [8.0, 16.0, 6.0])]).rotated([
        std::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    ]);
    Model { texture: "entity/sheep/sheep", tex_size: [64.0, 32.0], parts: quadruped_parts(12.0, head, body) }
}

// net.minecraft.entity.LimbAnimator, 1:1. Fuehrt die Schrittanimation eines Entities:
//
//   update_limbs(target, rate, time_scale):
//       last_speed = speed
//       speed     += (target - speed) * rate
//       progress  += speed
//
// Gefuettert wird `target` mit min(zurueckgelegte Strecke * 4, 1) und rate 0.4
// (LivingEntity.updateLimbs). `time_scale` ist 1 fuer erwachsene Entities und 3 fuer
// Jungtiere -- deren Beine trippeln dreimal so schnell.
//
// ECHTER BUG gewesen (Sprinten aenderte am Armschwingen nichts, in Vanilla schwingen die
// Arme dabei schneller): gefuettert wurde vorher
// die GESCHWINDIGKEIT nach der Reibungsdaempfung statt der tatsaechlich zurueckgelegten
// Strecke. Das ist rund die Haelfte davon (auf Boden daempft Vanilla mit 0.6 * 0.91), also
// war die Schwingung halb so weit UND halb so schnell wie in Vanilla -- und der Unterschied
// zwischen Gehen und Sprinten steckte in einer so kleinen Bewegung, dass er unsichtbar war.
//
// Die echten Zahlen (an der eigenen Physik gemessen, s. physics.rs::
// strecke_pro_tick_trifft_vanilla): 0.2159 Bloecke/Tick beim Gehen -> target 0.864,
// 0.2806 beim Sprinten -> target 1.0 (gedeckelt). Das sind 16 % mehr Frequenz UND die
// vollen 100 % Amplitude statt 86 %.
#[derive(Clone, Copy, Debug, Default)]
pub struct LimbAnimator {
    pub(crate) last_speed: f32,
    pub(crate) speed: f32,
    pub(crate) animation_progress: f32,
    pub(crate) time_scale: f32,
}

impl LimbAnimator {
    // Vanillas Aenderungsrate aus LivingEntity.updateLimbs.
    pub const RATE: f32 = 0.4;

    #[must_use] 
    pub const fn new() -> Self {
        Self { last_speed: 0.0, speed: 0.0, animation_progress: 0.0, time_scale: 1.0 }
    }

    // Ein Tick. `moved` ist die HORIZONTAL zurueckgelegte Strecke dieses Ticks in Bloecken.
    pub fn tick(&mut self, moved: f32, time_scale: f32) {
        let target = (moved * 4.0).min(1.0);
        self.last_speed = self.speed;
        self.speed += (target - self.speed) * Self::RATE;
        self.animation_progress += self.speed;
        self.time_scale = time_scale;
    }

    pub const fn reset(&mut self) {
        *self = Self::new();
    }

    // Amplitude, zwischen den beiden letzten Ticks interpoliert (LimbAnimator.getAmplitude).
    #[must_use] 
    pub fn amplitude(&self, tick_progress: f32) -> f32 {
        (self.last_speed + (self.speed - self.last_speed) * tick_progress).min(1.0)
    }

    // Phase, um den noch nicht vergangenen Teil des laufenden Ticks zurueckgerechnet
    // (LimbAnimator.getAnimationProgress) -- ohne das springt die Schwingung 20 mal pro
    // Sekunde weiter statt zu gleiten.
    #[must_use] 
    pub fn animation_progress(&self, tick_progress: f32) -> f32 {
        (self.animation_progress - self.speed * (1.0 - tick_progress)) * self.time_scale
    }
}

// MathHelper.wrapDegrees: einen Winkel auf -180..180 bringen.
#[must_use] 
pub fn wrap_degrees(deg: f32) -> f32 {
    let mut d = deg % 360.0;
    if d >= 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d
}

// MathHelper.lerpAngleDegrees: zwischen zwei Winkeln interpolieren, und zwar auf dem KURZEN
// Weg. Eine gewoehnliche Mischung von 350 nach 10 Grad liefe ueber 180 statt ueber 0 -- die
// Figur wuerde sich einmal ganz herumdrehen, statt 20 Grad zu wandern.
#[must_use] 
pub fn lerp_angle_degrees(delta: f32, from: f32, to: f32) -> f32 {
    from + delta * wrap_degrees(to - from)
}

// net.minecraft.entity.LivingEntity: bodyYaw/headYaw und turnHead, 1:1.
//
// Das ist die Traegheit, die man in Minecraft sieht: der KOPF folgt der Maus sofort
// (PlayerEntity.tickMovement setzt `headYaw = getYaw()` jeden Tick), der KOERPER laeuft
// hinterher. Rumpf, Arme und Beine haengen am Koerper, wirken also mit.
//
// Zwei Regeln steuern das (LivingEntity.tick + turnHead):
//
//  1. Ziel des Koerpers ist die BEWEGUNGSRICHTUNG, nicht die Blickrichtung -- und nur, wenn
//     man sich ueberhaupt bewegt (mehr als 0.05 Bloecke pro Tick). Im Stand bleibt der Koerper
//     einfach stehen, egal wohin man schaut. Laeuft man rueckwaerts (Blick und Bewegung mehr
//     als 95 Grad auseinander), dreht sich der Koerper um 180 Grad -- man laeuft rueckwaerts,
//     ohne sich zu verrenken.
//  2. Pro Tick holt der Koerper 30 % der Differenz auf, und der Kopf darf maximal 50 Grad
//     verdreht sein. Reisst man die Maus weiter, wird der Koerper mitgezogen.
//
// Regel 2 ist der Grund, warum ein schnelles Umschauen den Koerper erst gar nicht und dann
// doch bewegt: bis 50 Grad dreht sich nur der Kopf, danach schleppt er den Rumpf mit.
// PlayerEntity.getMaxRelativeHeadRotation: 15 Grad beim Blocken, sonst 50.
pub const BLOCKING_MAX_RELATIVE: f32 = 15.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct BodyYaw {
    pub(crate) yaw: f32,
    pub(crate) last_yaw: f32,
    pub(crate) head_yaw: f32,
    pub(crate) last_head_yaw: f32,
    // Schild oben (PlayerEntity.isBlocking): der Kopf darf dann nur 15 statt 50 Grad verdreht sein.
    pub(crate) blocking: bool,
}

// LivingEntity.tick (Zielwahl) + turnHead, als reine Funktion -- fuer den eigenen Spieler
// (BodyYaw) UND fremde Lebewesen (world::Entity) dieselbe Rechnung. Liefert die neue
// Koerperdrehung. `look_yaw_deg` ist getYaw(), `dx`/`dz` die Strecke dieses Ticks,
// `max_relative` getMaxRelativeHeadRotation (50, beim blockenden Spieler 15).
#[must_use] 
pub fn turn_body(body_yaw: f32, look_yaw_deg: f32, dx: f64, dz: f64, hand_swinging: bool, max_relative: f32) -> f32 {
    // LivingEntity.tick: ab 0.05 Bloecken pro Tick (quadriert, der krumme Wert steht so im Jar).
    const MOVING_THRESHOLD: f32 = 0.0025000002;
    let mut target = body_yaw;
    if ((dx * dx + dz * dz) as f32) > MOVING_THRESHOLD {
        let direction = (dz.atan2(dx) as f32).to_degrees() - 90.0;
        // Wie im Jar: nur getYaw() wird eingepackt, nicht die Differenz.
        let spread = (wrap_degrees(look_yaw_deg) - direction).abs();
        target = if spread > 95.0 && spread < 265.0 { direction - 180.0 } else { direction };
    }
    if hand_swinging {
        target = look_yaw_deg;
    }
    // turnHead: 30 % der Differenz, dann den Kopf auf max_relative begrenzen.
    let mut yaw = body_yaw + wrap_degrees(target - body_yaw) * 0.3;
    let relative = wrap_degrees(look_yaw_deg - yaw);
    if relative.abs() > max_relative {
        yaw += relative - relative.signum() * max_relative;
    }
    yaw
}

impl BodyYaw {
    // LivingEntity.getMaxRelativeHeadRotation.
    pub const MAX_RELATIVE: f32 = 50.0;

    #[must_use] 
    pub const fn new(yaw: f32) -> Self {
        Self { yaw, last_yaw: yaw, head_yaw: yaw, last_head_yaw: yaw, blocking: false }
    }

    pub const fn set_blocking(&mut self, blocking: bool) {
        self.blocking = blocking;
    }

    // Ein Tick. `look_yaw_deg` ist die Blickrichtung (Vanillas getYaw), `dx`/`dz` die in
    // diesem Tick zurueckgelegte Strecke, `hand_swinging` ob gerade zugeschlagen wird.
    pub fn tick(&mut self, look_yaw_deg: f32, dx: f64, dz: f64, hand_swinging: bool) {
        self.last_head_yaw = self.head_yaw;
        self.last_yaw = self.yaw;
        // PlayerEntity.tickMovement: der Kopf folgt der Maus ohne Verzoegerung.
        self.head_yaw = look_yaw_deg;

        let max_relative = if self.blocking { BLOCKING_MAX_RELATIVE } else { Self::MAX_RELATIVE };
        self.yaw = turn_body(self.yaw, look_yaw_deg, dx, dz, hand_swinging, max_relative);
    }

    // Mehr Animationen, BODY_TURN: der Koerper holt die Haelfte des Wegs zum Kopf auf (nach tick).
    pub fn turn_to_head(&mut self) {
        self.yaw += wrap_degrees(self.head_yaw - self.yaw) * 0.5;
    }

    // Den Koerper von aussen ausrichten (nach tick): ein Boot oder Reittier dreht seinen
    // Mitfahrer mit (AbstractBoatEntity.clampPassengerYaw, AbstractHorseEntity.
    // updatePassengerPosition setzen dessen bodyYaw).
    pub const fn set_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
    }

    // Die interpolierte Koerperdrehung -- damit wird das ganze Modell gedreht
    // (LivingEntityRenderer.setupTransforms).
    #[must_use] 
    pub fn body_yaw(&self, tick_progress: f32) -> f32 {
        lerp_angle_degrees(tick_progress, self.last_yaw, self.yaw)
    }

    // Die Kopfdrehung RELATIV zum Koerper -- das ist der Wert, den setAngles fuer den Kopf
    // bekommt (LivingEntityRenderer.updateRenderState).
    #[must_use] 
    pub fn relative_head_yaw(&self, tick_progress: f32) -> f32 {
        let head = lerp_angle_degrees(tick_progress, self.last_head_yaw, self.head_yaw);
        wrap_degrees(head - self.body_yaw(tick_progress))
    }
}

// net.minecraft.client.network.ClientPlayerLikeState, 1:1. Der Zustand, aus dem die
// Umhangbewegung entsteht: eine Position, die der echten Spielerposition NACHHINKT (pro Tick
// nur ein Viertel der Differenz), plus zwei Laufgroessen.
//
// Die Umhangbewegung ist genau diese Differenz. Steht der Spieler, holt die nachhinkende
// Position auf und der Umhang haengt herunter; laeuft er los, bleibt sie zurueck und der
// Umhang weht nach hinten. Deshalb braucht es keinerlei Physiksimulation fuer den Stoff.
//
// Die 10-Blocks-Regel ist gleichzeitig die Initialisierung: Vanilla startet alle drei
// Koordinaten bei 0, sodass der erste Tick ueber die Schwelle springt und hart setzt. Genau
// darauf verlassen wir uns auch -- kein Sonderfall fuer "erster Tick".
#[derive(Clone, Copy, Debug, Default)]
pub struct CapeState {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    pub(crate) last_x: f64,
    pub(crate) last_y: f64,
    pub(crate) last_z: f64,
    pub(crate) movement: f32,
    pub(crate) last_movement: f32,
    pub(crate) distance_moved: f32,
    pub(crate) last_distance_moved: f32,
}

impl CapeState {
    // Wie schnell die nachhinkende Position aufholt (ClientPlayerLikeState.setPos).
    pub(crate) const FOLLOW: f64 = 0.25;
    // Ab dieser Differenz wird hart gesetzt statt nachgezogen -- sonst wuerde der Umhang
    // nach einem Teleport quer durch die Welt gezogen.
    pub(crate) const SNAP: f64 = 10.0;
    // Aenderungsrate der Laufgroesse (ClientPlayerLikeState.tickMovement) -- dieselbe 0.4
    // wie beim LimbAnimator, aber ein eigener Zustand.
    pub(crate) const MOVEMENT_RATE: f32 = 0.4;

    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    // ClientPlayerLikeState.tick -> setPos. `pos` ist die Spielerposition zu Tickbeginn.
    pub fn tick(&mut self, pos: [f64; 3]) {
        self.last_distance_moved = self.distance_moved;
        let follow = |current: &mut f64, last: &mut f64, target: f64| {
            *last = *current;
            let delta = target - *current;
            if (-Self::SNAP..=Self::SNAP).contains(&delta) {
                *current += delta * Self::FOLLOW;
            } else {
                *current = target;
                *last = *current;
            }
        };
        follow(&mut self.x, &mut self.last_x, pos[0]);
        follow(&mut self.y, &mut self.last_y, pos[1]);
        follow(&mut self.z, &mut self.last_z, pos[2]);
    }

    // ClientPlayerLikeState.tickMovement. `movement` ist bei Vanilla
    // `onGround && !dead && !swimming ? min(0.1, horizontale Geschwindigkeit) : 0`.
    pub fn tick_movement(&mut self, movement: f32) {
        self.last_movement = self.movement;
        self.movement += (movement - self.movement) * Self::MOVEMENT_RATE;
    }

    // ClientPlayerEntity.move: addDistanceMoved(hypot(dx, dz) * 0.6). Nur der EIGENE Spieler
    // ruft das auf -- OtherClientPlayerEntity ueberschreibt move() nicht. Fremde Umhaenge
    // schwingen in Vanilla deshalb nur durch die nachhinkende Position, ohne das
    // Lauf-Schwingen. Diese Asymmetrie ist echt und wird hier nicht "verbessert".
    pub fn add_distance_moved(&mut self, moved: f32) {
        self.distance_moved += moved * 0.6;
    }

    // Die nachhinkende Position, zwischen den letzten zwei Ticks interpoliert.
    #[must_use] 
    pub fn lerp_pos(&self, tick_progress: f32) -> [f64; 3] {
        let t = f64::from(tick_progress);
        [self.last_x + (self.x - self.last_x) * t, self.last_y + (self.y - self.last_y) * t, self.last_z + (self.z - self.last_z) * t]
    }

    #[must_use] 
    pub fn lerp_movement(&self, tick_progress: f32) -> f32 {
        self.last_movement + (self.movement - self.last_movement) * tick_progress
    }

    #[must_use] 
    pub fn lerp_distance_moved(&self, tick_progress: f32) -> f32 {
        self.last_distance_moved + (self.distance_moved - self.last_distance_moved) * tick_progress
    }
}

// PlayerEntityRenderer.updateCape -- rechnet aus dem CapeState die drei Winkelgroessen, die
// cape_pose braucht. `pos`/`last_pos` sind die ECHTE Spielerposition dieses und des letzten
// Ticks, `body_yaw_deg` die interpolierte Koerperdrehung.
//
// Die Klemmgrenzen stehen so im Jar. Vor allem die Untergrenze 0 fuer `swing` ist wichtig:
// laeuft man rueckwaerts, waere der Wert negativ und der Umhang wuerde nach VORNE durch den
// Koerper klappen.
#[must_use] 
pub fn cape_angles(state: &CapeState, pos: [f64; 3], last_pos: [f64; 3], body_yaw_deg: f32, tick_progress: f32) -> (f32, f32, f32) {
    let t = f64::from(tick_progress);
    let lerped = state.lerp_pos(tick_progress);
    let d = lerped[0] - (last_pos[0] + (pos[0] - last_pos[0]) * t);
    let e = lerped[1] - (last_pos[1] + (pos[1] - last_pos[1]) * t);
    let f = lerped[2] - (last_pos[2] + (pos[2] - last_pos[2]) * t);
    let h = f64::from(body_yaw_deg.to_radians().sin());
    let i = f64::from(-(body_yaw_deg.to_radians().cos()));

    let mut vertical = ((e * 10.0) as f32).clamp(-6.0, 32.0);
    // Der Gleitflug-Faktor (1 - glidingProgress) fehlt hier: wir haben keinen Elytren-
    // Zustand vom Server. Ohne Elytren ist er 1, also fuer alles, was dieser Client heute
    // darstellt, wirkungslos.
    let swing = ((d * h + f * i) * 100.0).clamp(0.0, 150.0) as f32;
    let side = ((d * i - f * h) * 100.0).clamp(-20.0, 20.0) as f32;

    // Das Lauf-Schwingen: die Phase ist die aufsummierte Laufstrecke, die Amplitude die
    // gedaempfte Laufgroesse. Bei fremden Spielern ist distance_moved immer 0 (s.
    // add_distance_moved), damit fällt der Term dort weg -- wie in Vanilla.
    vertical += (state.lerp_distance_moved(tick_progress) * 6.0).sin() * 32.0 * state.lerp_movement(tick_progress);
    (vertical, swing, side)
}

// ---------------------------------------------------------------------------------------
// Zuordnung Entity-Typ -> Modell, und die Posen (Vanillas setAngles).
// ---------------------------------------------------------------------------------------
