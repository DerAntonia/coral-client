// Armschwingen, Handschwung, Schwimmzug, Benutzungsposen und die Biped-Pose.

use super::*;

// BipedEntityModel.setAngles, auf das reduziert, was dieser Client ueberhaupt weiss:
// Kopfwinkel und Schrittschwingen. Ausgelassen sind die Zweige, fuer die es hier keine
// Daten gibt (Arm-Posen fuer gehaltene Gegenstaende, Klettern, Elytra, Reiten) -- die
// wuerden ohne die zugehoerigen Server-Informationen nur geraten.
//
// `head_yaw_rel` ist die Kopfdrehung RELATIV zum Koerper (Vanilla: relativeHeadYaw), beide
// in Grad. `swing_progress` ist die laufende Schrittphase, `swing_amount` deren Amplitude
// 0..1 (s. Entity::limb_swing).
// ArmPosing.swingArm, 1:1. Die Ruhe-Bewegung der Arme -- Vanilla legt sie AUF alles andere
// drauf (Schrittschwingen, Schleichen), nicht anstelle:
//
//     arm.roll  += sigma * (cos(age * 0.09)  * 0.05 + 0.05)
//     arm.pitch += sigma * (sin(age * 0.067) * 0.05)
//
// mit sigma = +1 fuer den rechten und -1 fuer den linken Arm.
//
// Der Summand 0.05 IM ROLL-TERM ist der Grund, warum die Arme in Minecraft nie am Koerper
// anliegen: der Kosinus schwingt zwischen -0.05 und +0.05, plus die Konstante ergibt 0..0.1
// rad. Der Wert wird also nie negativ, der rechte Arm zeigt immer 0 bis 5,7 Grad NACH AUSSEN
// und der linke spiegelbildlich. Ohne diesen Summanden klebten die Arme am Rumpf und die
// Figur saehe steif aus -- genau das war hier frueher der Fall.
//
// Die beiden Frequenzen sind verschieden (Roll 0.09/Tick = 3,49 s Periode, Pitch 0.067/Tick =
// 4,69 s): dadurch wiederholt sich die Gesamtbewegung nie exakt, was sie lebendig wirken
// laesst. Zwei gleiche Perioden waeren ein sichtbar mechanisches Auf und Ab.
//
// `age` ist Vanillas `state.age` -- das Alter des Entities in Ticks, mit Zwischentick-Anteil.
// Nur der Rest modulo Periode zaehlt, ein anderer Nullpunkt aendert also nur die Phase.
pub(crate) fn apply_arm_sway(pose: &mut Pose, age: f32, (rechts, links): (bool, bool)) {
    for (part, sigma, an) in [("right_arm", 1.0f32, rechts), ("left_arm", -1.0f32, links)] {
        if !an {
            continue;
        }
        let current = pose.get(part);
        let roll = sigma * ((age * 0.09).cos() * 0.05 + 0.05);
        let pitch = sigma * (age * 0.067).sin() * 0.05;
        pose.set(
            part,
            PartPose {
                rotation: [current.rotation[0] + pitch, current.rotation[1], current.rotation[2] + roll],
                pivot_offset: current.pivot_offset,
                rotation_matrix: current.rotation_matrix,
                hidden: false,
            },
        );
    }
}

// MathHelper.wrapRadians / lerpAngleRadians -- dasselbe wie die Grad-Fassungen weiter oben,
// nur im Bogenmass (die Modellwinkel sind Bogenmass).
pub(crate) fn wrap_radians(rad: f32) -> f32 {
    let mut r = rad % std::f32::consts::TAU;
    if r >= std::f32::consts::PI {
        r -= std::f32::consts::TAU;
    }
    if r < -std::f32::consts::PI {
        r += std::f32::consts::TAU;
    }
    r
}

pub(crate) fn lerp_angle_radians(delta: f32, from: f32, to: f32) -> f32 {
    from + delta * wrap_radians(to - from)
}

// BipedEntityModel.animateArms -- der SCHLAG mit der Hand (Vanillas handSwingProgress).
//
// Nicht zu verwechseln mit dem Schrittschwingen weiter oben: das kommt aus der zurueckgelegten
// Strecke und laeuft dauernd, dieses hier ist der einmalige Ausschlag eines Angriffs oder einer
// Benutzung und dauert 6 Ticks (SwingAnimationComponent.DEFAULT).
//
// Der ganze OBERKOERPER dreht sich mit, nicht nur der Arm -- das ist der Teil, den man am
// leichtesten vergisst und der den Schlag ueberhaupt erst wuchtig aussehen laesst:
//
//   body.yaw = sin(sqrt(f) * 2PI) * 0.2          (bei linkem Arm mit umgekehrtem Vorzeichen)
//
// Weil sich der Rumpf dreht, wandern auch die DREHPUNKTE der Arme mit ihm um die Koerperachse:
// Vanilla setzt sie auf (-cos(bodyYaw)*5, ., sin(bodyYaw)*5) bzw. spiegelbildlich -- bei
// bodyYaw = 0 sind das exakt die Grundwerte -5/0 und 5/0 aus biped_parts, es ist also eine
// Drehung des Armpaars um den Rumpf. Ohne das bliebe der Arm an seiner alten Schulter haengen,
// waehrend der Rumpf sich wegdreht.
//
// Erst danach kommt der eigentliche Arm (Vanillas SwingAnimationType.WHACK, der Normalfall --
// STAB gehoert dem Speer, NONE schaltet die Bewegung ganz ab):
//
//   g = outQuart(f) = 1 - (1-f)^4
//   arm.pitch -= sin(g * PI) * 1.2 + sin(f * PI) * -(head.pitch - 0.7) * 0.75
//   arm.yaw   += body.yaw * 2
//   arm.roll  += sin(f * PI) * -0.4
//
// Der zweite Summand im pitch haengt an der KOPFNEIGUNG: wer nach unten schaut, holt weiter
// aus. Deshalb muss der Kopf in der Pose schon stehen, wenn diese Funktion laeuft.
//
// `right_arm` ist Vanillas preferredArm -- der Arm, mit dem geschlagen wurde (Haupthand oder
// Zweithand), nicht etwa immer der rechte.
pub fn apply_hand_swing(pose: &mut Pose, progress: f32, right_arm: bool) {
    apply_hand_swing_typed(pose, progress, right_arm, false);
}

// Wie apply_hand_swing, mit Vanillas Wahl der Schlagart. STAB (Speer, Lancing.method_75393)
// laesst die Rumpfdrehung stehen, nimmt deren Anteil an den Armen aber wieder heraus und
// stoesst den Arm statt auszuholen:
//   pitch += (90 * inOutSine(f in 0..0.05) - 120 * inQuad(f in 0.05..0.2)
//             + 30 * inOutExpo(f in 0.4..1)) Grad
pub fn apply_hand_swing_typed(pose: &mut Pose, progress: f32, right_arm: bool, stab: bool) {
    if progress <= 0.0 {
        return;
    }
    let pi = std::f32::consts::PI;
    let f = progress;
    let body_yaw = (f.max(0.0).sqrt() * 2.0 * pi).sin() * 0.2 * if right_arm { 1.0 } else { -1.0 };

    // Rumpf: Vanilla SETZT die Gierung hier (kein Addieren) -- die Schleich-Haltung fasst nur
    // die Neigung an, die beiden kommen sich also nicht ins Gehege.
    let body = pose.get("body");
    pose.set("body", PartPose { rotation: [body.rotation[0], body_yaw, body.rotation[2]], ..body });

    // Die Drehpunkte wandern mit. Vanilla multipliziert hier mit `ageScale`, das bei einem
    // ausgewachsenen Spieler 1.0 ist -- Babyvarianten kennt dieser Client nicht.
    let (sin_yaw, cos_yaw) = (body_yaw.sin(), body_yaw.cos());
    for (part, sign) in [("right_arm", -1.0f32), ("left_arm", 1.0f32)] {
        let mut teil = pose.get(part);
        teil.pivot_offset[0] += sign * 5.0 * (cos_yaw - 1.0);
        teil.pivot_offset[2] += -sign * 5.0 * sin_yaw;
        // Beide Arme folgen der Rumpfdrehung; der linke bekommt sie zusaetzlich auf die
        // Neigung. Diese Unsymmetrie steht so im Jar.
        teil.rotation[1] += body_yaw;
        if part == "left_arm" {
            teil.rotation[0] += body_yaw;
        }
        pose.set(part, teil);
    }

    if stab {
        for part in ["right_arm", "left_arm"] {
            let mut teil = pose.get(part);
            teil.rotation[1] -= body_yaw;
            if part == "left_arm" {
                teil.rotation[0] -= body_yaw;
            }
            pose.set(part, teil);
        }
        let g = easing::in_out_sine(easing::progress(f, 0.0, 0.05));
        let h = easing::in_quad(easing::progress(f, 0.05, 0.2));
        let i = easing::in_out_expo(easing::progress(f, 0.4, 1.0));
        let arm = if right_arm { "right_arm" } else { "left_arm" };
        let mut teil = pose.get(arm);
        teil.rotation[0] += (90.0 * g - 120.0 * h + 30.0 * i).to_radians();
        pose.set(arm, teil);
        return;
    }
    let head_pitch = pose.get("head").rotation[0];
    let g = 1.0 - (1.0 - f) * (1.0 - f) * (1.0 - f) * (1.0 - f);
    let aushol = (f * pi).sin() * -(head_pitch - 0.7) * 0.75;
    let arm = if right_arm { "right_arm" } else { "left_arm" };
    let mut teil = pose.get(arm);
    teil.rotation[0] -= (g * pi).sin() * 1.2 + aushol;
    teil.rotation[1] += body_yaw * 2.0;
    teil.rotation[2] += (f * pi).sin() * -0.4;
    pose.set(arm, teil);
}

// BipedEntityModel.setAngles, Zweig `leaningPitch > 0` -- die Schwimm-/Kriechbewegung.
//
// Der Zyklus ist 26 Einheiten der Schrittphase lang (`i = g % 26`) und hat drei Abschnitte:
//
//   i <  14  Zug: die Arme liegen nach hinten gestreckt (yaw = PI) und rollen auseinander,
//            gesteuert von der Kurve e(x) = -65x + x^2 -- die faellt anfangs steil und flacht
//            ab, der Zug ist also vorne kraeftig und laeuft hinten aus.
//   14..22   Vorholen: die Arme heben sich nach vorn (pitch bis PI/2) und rollen zurueck.
//   22..26   Zuruecksetzen: pitch wieder auf 0, roll auf PI -- Ausgangslage fuer den naechsten Zug.
//
// Dazu treten die Beine unabhaengig davon mit einer eigenen, langsameren Frequenz
// (g * 1/3 statt g * 0.6662 beim Laufen) und kleinerem Ausschlag (0.3 statt 1.4).
//
// ALLE Werte werden mit `lean` eingeblendet (lerp vom bisherigen Wert zum Schwimmziel):
// beim Eintauchen waechst leaningPitch ueber ~11 Ticks von 0 auf 1, die Laufanimation geht
// also weich in die Schwimmbewegung ueber statt umzuschalten.
//
// Der linke Arm nutzt lerpAngleRadians (kurzer Weg ueber den Winkelkreis), der rechte das
// gewoehnliche lerp. Diese Unsymmetrie steht so im Jar und ist kein Tippfehler von mir: die
// Zielwinkel des linken Arms laufen ueber die PI-Grenze (5.012389 rad > PI), wo eine
// gewoehnliche Mischung den langen Weg naehme.
//
// `crawl`: die Kriech-Variante (s. BipedExtras::crawl). Der Takt laeuft 2.5-mal so schnell
// (an Land kommt man langsam voran, die Schrittphase waechst kaum), nach dem Zug heben sich
// die Arme nicht ueber den Kopf, sondern schieben flach nach vorn, und die Beine treten mit
// halbem Ausschlag, leicht gespreizt.
pub(crate) fn apply_swim_stroke(pose: &mut Pose, lean_left: f32, lean_right: f32, swing_progress: f32, crawl: bool) {
    if lean_left <= 0.0 && lean_right <= 0.0 {
        return;
    }
    const ROLL_SPAN: f32 = 1.8707964;
    const LEFT_ROLL_MID: f32 = 5.012389;
    const RIGHT_ROLL_MID: f32 = 1.2707963;
    // BipedEntityModel.method_2807 -- die Zug-Kurve.
    let ease = |x: f32| -65.0 * x + x * x;

    const CRAWL_SPEED: f32 = 2.5;
    const CRAWL_LIFT: f32 = 0.3707964;
    let swing_progress = if crawl { swing_progress * CRAWL_SPEED } else { swing_progress };
    let phase = swing_progress.rem_euclid(26.0);
    let pi = std::f32::consts::PI;
    let (left_pitch, right_pitch, left_roll, right_roll) = if phase < 14.0 {
        (0.0, 0.0, pi + ROLL_SPAN * ease(phase) / ease(14.0), pi - ROLL_SPAN * ease(phase) / ease(14.0))
    } else if crawl && phase < 24.0 {
        let l = (phase - 14.0) / 10.0;
        (-CRAWL_LIFT * l, -CRAWL_LIFT * l, LEFT_ROLL_MID - ROLL_SPAN * l, RIGHT_ROLL_MID + ROLL_SPAN * l)
    } else if crawl {
        let l = (phase - 24.0) / 2.0;
        (-CRAWL_LIFT + CRAWL_LIFT * l, -CRAWL_LIFT + CRAWL_LIFT * l, pi, pi)
    } else if phase < 22.0 {
        let l = (phase - 14.0) / 8.0;
        (pi / 2.0 * l, pi / 2.0 * l, LEFT_ROLL_MID - ROLL_SPAN * l, RIGHT_ROLL_MID + ROLL_SPAN * l)
    } else {
        let l = (phase - 22.0) / 4.0;
        (pi / 2.0 - pi / 2.0 * l, pi / 2.0 - pi / 2.0 * l, pi, pi)
    };

    // Der SCHLAGENDE Arm schwimmt nicht mit: Vanilla setzt sein Lean auf 0, wenn
    // handSwingProgress > 0 ist und dieser Arm der preferredArm ist -- der Aufrufer gibt das
    // ueber `lean_left`/`lean_right` herein. Sonst verschluckte der Zug den Schlag genau dann,
    // wenn man im Wasser kaempft. (`isUsingItem` und die Speer-Ausnahme fehlen weiterhin:
    // dieser Client kennt keine Armhaltungen.)
    let links = pose.get("left_arm").rotation;
    let rechts = pose.get("right_arm").rotation;
    pose.rotate(
        "left_arm",
        [
            lerp_angle_radians(lean_left, links[0], left_pitch),
            lerp_angle_radians(lean_left, links[1], pi),
            lerp_angle_radians(lean_left, links[2], left_roll),
        ],
    );
    pose.rotate(
        "right_arm",
        [
            rechts[0] + (right_pitch - rechts[0]) * lean_right,
            rechts[1] + (pi - rechts[1]) * lean_right,
            rechts[2] + (right_roll - rechts[2]) * lean_right,
        ],
    );

    // Beine: eigene Frequenz und eigener Ausschlag, gegenphasig zueinander.
    let lean = lean_left.max(lean_right);
    let (ausschlag, spreizen) = if crawl { (0.15, 0.1507964) } else { (0.3, 0.0) };
    for (part, phase_offset, seite) in [("left_leg", pi, -1.0), ("right_leg", 0.0, 1.0)] {
        let current = pose.get(part).rotation;
        let target = ausschlag * (swing_progress * 0.333_333_34 + phase_offset).cos();
        let roll = if crawl { spreizen * seite } else { current[2] };
        pose.rotate(part, [current[0] + (target - current[0]) * lean, current[1], roll]);
    }
}

#[must_use] 
pub fn biped_pose(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32, sneaking: bool, age: f32) -> Pose {
    biped_pose_leaning(head_yaw_rel, head_pitch, swing_progress, swing_amount, sneaking, 0.0, age)
}

// Alles, was ueber Kopfwinkel und Schrittanimation hinausgeht. Als Struktur statt als sieben
// weitere Parameter -- die Reihenfolge von f32-Argumenten verwechselt man sonst irgendwann,
// und der Fehler waere still.
#[derive(Clone, Copy, Debug, Default)]
pub struct BipedExtras {
    pub sneaking: bool,
    /// Vanillas leaningPitch 0..1 (Schwimmen/Kriechen).
    pub lean: f32,
    /// Entity-Alter in Ticks samt Zwischentick-Anteil (Ruhe-Bewegung der Arme).
    pub age: f32,
    /// Vanillas handSwingProgress 0..1. 0 = kein Schlag.
    pub hand_swing: f32,
    /// Welcher Arm schlaegt (Vanillas preferredArm).
    pub swing_right_arm: bool,
    /// Haelt die Figur etwas in der jeweiligen Hand? Hebt den Arm an (ArmPose.ITEM).
    pub item_right: bool,
    pub item_left: bool,
    /// Blockt die Figur gerade mit dem jeweiligen Arm (ArmPose.BLOCK)? Sticht die
    /// ITEM-Haltung aus -- der Arm geht hoch VOR den Koerper, nicht nur ein Stueck an.
    pub block_right: bool,
    pub block_left: bool,
    /// Haelt der jeweilige Arm einen Speer (ArmPose.SPEAR)? Der Arm zeigt dann nach vorn in
    /// Blickrichtung statt nur angehoben zu sein.
    pub spear_right: bool,
    pub spear_left: bool,
    /// Ist der Schlag ein Speerstich (SwingAnimationType.STAB) statt des normalen Ausholens?
    pub stab: bool,
    /// Gleitet die Figur mit der Elytra? Senkt den Kopf und daempft die Gliedmassen.
    pub glide: Option<Glide>,
    /// Sitzt die Figur auf etwas (hasVehicle)? Beine nach vorn, Arme ein Stueck hoch.
    pub riding: bool,
    /// Benutzungshaltung eines Arms (Bogen, Armbrust, Dreizack, Fernrohr, Horn, Pinsel):
    /// (rechter Arm?, Haltung). Sticht ITEM, Schild und Speer dieses Arms aus.
    pub use_pose: Option<(bool, UsePose)>,
    /// Kriechen statt Schwimmen (Mehr Animationen, s. `more_anims)`: an Land bleiben die
    /// Arme beim Zug flach am Boden, der Takt ist schneller und die Beine treten kleiner.
    pub crawl: bool,
}

// Wer etwas in der Hand haelt, HEBT den Arm -- BipedEntityModel.positionRightArm/-LeftArm,
// Zweig ArmPose.ITEM (1.21.11): Neigung = halbe Neigung - PI/10, Gierung = 0.
// Beachte das MAL 0.5: die Neigung aus der Laufanimation wird halbiert, nicht ersetzt -- der
// Arm schwingt beim Gehen also weiter mit, nur gedaempft und angehoben. Ein blosses Addieren
// des Winkels saehe im Stand richtig aus und beim Laufen falsch.
//
// Muss VOR dem Anhaengen des Items laufen: das Item haengt an genau dieser Armhaltung
// (s. part_transform), sonst schwebt es dort, wo der Arm ohne Item waere.
pub fn apply_item_arm_pose(pose: &mut Pose, right: bool) {
    let part = if right { "right_arm" } else { "left_arm" };
    let mut teil = pose.get(part);
    teil.rotation[0] = teil.rotation[0] * 0.5 - std::f32::consts::PI / 10.0;
    teil.rotation[1] = 0.0;
    pose.set(part, teil);
}

// Der gehobene Schild-Arm -- BipedEntityModel.positionBlockingArm (1.21.11):
//
//     Neigung = halbe Neigung - 0.9424779 + Kopfneigung (auf -4PI/9..0.43633232 begrenzt)
//     Gierung = -30 Grad (rechts) bzw. +30 Grad (links) + Kopfgierung (auf -PI/6..PI/6)
//
// Der Arm folgt also der BLICKRICHTUNG, gedeckelt: der Schild wandert mit, wenn man den Kopf
// dreht oder senkt, aber nur bis zu einem Anschlag. Deshalb braucht diese Stufe den KOPF und
// muss nach der Kopfhaltung laufen -- sie liest ihn aus der Pose zurueck statt ihn erneut
// auszurechnen, damit die Schwimm-Korrektur (s. biped_pose_full, Stufe 1) mitzaehlt.
//
// Wie bei ArmPose.ITEM wird die vorhandene Neigung HALBIERT statt ersetzt: der Arm schwingt
// beim Laufen gedaempft weiter.
pub fn apply_blocking_arm_pose(pose: &mut Pose, right: bool) {
    const DEG: f32 = std::f32::consts::PI / 180.0;
    let kopf = pose.get("head").rotation;
    let part = if right { "right_arm" } else { "left_arm" };
    let mut teil = pose.get(part);
    teil.rotation[0] = teil.rotation[0] * 0.5 - 0.9424779 + kopf[0].clamp(-std::f32::consts::PI * 4.0 / 9.0, 0.43633232);
    teil.rotation[1] = if right { -30.0 } else { 30.0 } * DEG
        + kopf[1].clamp(-std::f32::consts::FRAC_PI_6, std::f32::consts::FRAC_PI_6);
    pose.set(part, teil);
}

// ArmPose.SPEAR -- Lancing.positionArmForSpear (1.21.11), ohne den Schwank
// beim Ansturm (der braucht die Benutzungsdauer, die wir bei Fremden nicht kennen):
//   arm.yaw   = -0.1 * i + head.yaw
//   arm.pitch = -PI/2 + head.pitch + 0.8   (gleitend/schwimmend noch -0.9599311)
//   yaw auf -60..60 Grad, pitch auf -120..30 Grad begrenzt
// Anders als ITEM ERSETZT das die Laufneigung: der Speer zeigt ruhig in Blickrichtung.
pub fn apply_spear_arm_pose(pose: &mut Pose, right: bool, flach: bool) {
    let kopf = pose.get("head").rotation;
    let i = if right { 1.0 } else { -1.0 };
    let part = if right { "right_arm" } else { "left_arm" };
    let mut teil = pose.get(part);
    let mut pitch = -std::f32::consts::FRAC_PI_2 + kopf[0] + 0.8;
    if flach {
        pitch -= 0.9599311;
    }
    let yaw = -0.1 * i + kopf[1];
    teil.rotation[1] = yaw.to_degrees().clamp(-60.0, 60.0).to_radians();
    teil.rotation[0] = pitch.to_degrees().clamp(-120.0, 30.0).to_radians();
    pose.set(part, teil);
}

// Die Armhaltungen beim Benutzen eines Items (HumanoidModel.ArmPose), ausser Schild und Speer,
// die ihre eigenen Funktionen haben.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum UsePose {
    #[default]
    None,
    // Bogen spannen: beide Arme in Blickrichtung, der andere haelt den Bogen.
    Bow,
    // Dreizack zum Wurf ueber den Kopf.
    ThrowTrident,
    // Armbrust spannen: `ticks` von `max` Ticks Ladezeit.
    CrossbowCharge { ticks: f32, max: f32 },
    // Geladene Armbrust im Anschlag.
    CrossbowHold,
    Spyglass,
    TootHorn,
    Brush,
}

impl UsePose {
    // ArmPose.affectsOffhandPose: belegt die Haltung auch den anderen Arm?
    #[must_use] 
    pub const fn affects_offhand(self) -> bool {
        matches!(self, Self::Bow | Self::ThrowTrident | Self::CrossbowCharge { .. } | Self::CrossbowHold)
    }

    // ArmPose.isTwoHanded: haelt man das Item mit beiden Haenden (Armbrust)?
    #[must_use] 
    pub const fn two_handed(self) -> bool {
        matches!(self, Self::CrossbowCharge { .. } | Self::CrossbowHold)
    }
}

// HumanoidModel.poseRightArm/-LeftArm fuer die Haltungen aus UsePose (1.21.11). `rechts`: der
// Arm, der das Item benutzt bzw. haelt. Liest den Kopf aus der Pose wie die Schild-Haltung.
pub fn apply_use_pose(pose: &mut Pose, rechts: bool, haltung: UsePose, sneaking: bool) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let kopf = pose.get("head").rotation;
    let (arm, anderer) = if rechts { ("right_arm", "left_arm") } else { ("left_arm", "right_arm") };
    // Vorzeichen der Seite: Gierungen spiegeln sich zwischen rechts und links.
    let s = if rechts { 1.0 } else { -1.0 };
    let setze = |pose: &mut Pose, teil: &'static str, pitch: Option<f32>, yaw: Option<f32>| {
        let mut r = pose.get(teil).rotation;
        if let Some(p) = pitch {
            r[0] = p;
        }
        if let Some(y) = yaw {
            r[1] = y;
        }
        pose.rotate(teil, r);
    };
    match haltung {
        UsePose::None => {}
        UsePose::Bow => {
            // Der Zugarm zielt leicht nach innen, der Bogenarm 0.4 weiter nach aussen.
            setze(pose, arm, Some(-FRAC_PI_2 + kopf[0]), Some(-0.1 * s + kopf[1]));
            setze(pose, anderer, Some(-FRAC_PI_2 + kopf[0]), Some(0.1 * s + kopf[1] + 0.4 * s));
        }
        UsePose::ThrowTrident => {
            let p = pose.get(arm).rotation[0];
            setze(pose, arm, Some(p * 0.5 - PI), Some(0.0));
        }
        UsePose::CrossbowCharge { ticks, max } => {
            let anteil = if max > 0.0 { ticks.clamp(0.0, max) / max } else { 1.0 };
            let neigung = -0.970_796_35;
            setze(pose, arm, Some(neigung), Some(-0.8 * s));
            setze(pose, anderer, Some(neigung + (-FRAC_PI_2 - neigung) * anteil), Some((0.4 + 0.45 * anteil) * s));
        }
        UsePose::CrossbowHold => {
            setze(pose, arm, Some(-FRAC_PI_2 + kopf[0] + 0.1), Some(-0.3 * s + kopf[1]));
            setze(pose, anderer, Some(-1.5 + kopf[0]), Some(0.6 * s + kopf[1]));
        }
        UsePose::Spyglass => {
            let ducken = if sneaking { PI / 12.0 } else { 0.0 };
            setze(pose, arm, Some((kopf[0] - 1.919_862_2 - ducken).clamp(-2.4, 3.3)), Some(kopf[1] - PI / 12.0 * s));
        }
        UsePose::TootHorn => {
            setze(pose, arm, Some(kopf[0].clamp(-1.2, 1.2) - 1.483_529_8), Some(kopf[1] - PI / 6.0 * s));
        }
        UsePose::Brush => {
            let p = pose.get(arm).rotation[0];
            setze(pose, arm, Some(p * 0.5 - PI / 5.0), Some(0.0));
        }
    }
}

// Wie biped_pose, plus die Kopfhaltung beim Schwimmen: Vanilla zieht die Kopfneigung mit
// `lean` Richtung -45 Grad (BipedEntityModel.setAngles, Zweig f > 0). Ohne das schaut die
// Figur beim Schwimmen in den Boden, weil der ganze Koerper waagerecht liegt, der Kopf aber
// weiter der Blickneigung folgt.
#[must_use] 
pub fn biped_pose_leaning(
    head_yaw_rel: f32,
    head_pitch: f32,
    swing_progress: f32,
    swing_amount: f32,
    sneaking: bool,
    lean: f32,
    age: f32,
) -> Pose {
    biped_pose_full(head_yaw_rel, head_pitch, swing_progress, swing_amount, BipedExtras { sneaking, lean, age, ..Default::default() })
}

// Die vollstaendige Fassung von BipedEntityModel.setAngles. Die REIHENFOLGE der Stufen ist
// hier der eigentliche Inhalt, nicht die einzelnen Formeln -- sie steht 1:1 so im Jar:
//
//   1. Kopf              (Blickrichtung, beim Schwimmen Richtung -45 Grad gezogen)
//   2. Schrittschwingen  (Arme und Beine aus der Laufstrecke)
//   3. Armhaltung        positionRightArm/-LeftArm -- gehaltener Gegenstand hebt den Arm
//   4. Handschlag        animateArms
//   5. Schleichen        Rumpf kippt, alles wandert mit
//   6. Ruhe-Bewegung     ArmPosing.swingArm
//   7. Schwimmzug        blendet ueber alles bisherige
//
// Warum das zaehlt, an einem Fall, der genau so kaputt war: die Armhaltung (3) HALBIERT die
// vorhandene Neigung (`pitch * 0.5 - PI/10`). Laeuft sie nach dem Handschlag (4), wird der
// Schlag mit halbiert -- der Arm zuckt dann nur noch, sobald man etwas in der Hand haelt, und
// im Schwertkampf haelt man immer etwas in der Hand.
#[must_use] 
pub fn biped_pose_full(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32, extras: BipedExtras) -> Pose {
    let BipedExtras {
        sneaking,
        lean,
        age,
        hand_swing,
        swing_right_arm,
        item_right,
        item_left,
        block_right,
        block_left,
        spear_right,
        spear_left,
        stab,
        glide,
        riding,
        use_pose,
        crawl,
    } = extras;
    const DEG: f32 = std::f32::consts::PI / 180.0;
    let mut pose = Pose::new();
    let mut pitch = head_pitch * DEG;
    if glide.is_some_and(|g| g.head_down()) {
        pitch = -std::f32::consts::FRAC_PI_4;
    } else if lean > 0.0 {
        // MathHelper.lerpAngleRadians(delta, from, to) -- hier reicht die einfache Mischung,
        // weil beide Winkel im selben Umlauf liegen.
        pitch += (-std::f32::consts::FRAC_PI_4 - pitch) * lean.clamp(0.0, 1.0);
    }
    pose.rotate("head", [pitch, head_yaw_rel * DEG, 0.0]);

    // Exakt Vanillas Formeln (limbAmplitudeInverse ist bei einem normalen Mob 1.0).
    let g = swing_progress;
    let h = swing_amount;
    // limbAmplitudeInverse: 1 ausser im schnellen Gleitflug (s. Glide::limb_amplitude_inverse).
    let k = glide.map_or(1.0, |g| g.limb_amplitude_inverse());
    let right_arm = (g * 0.6662 + std::f32::consts::PI).cos() * 2.0 * h * 0.5 / k;
    let left_arm = (g * 0.6662).cos() * 2.0 * h * 0.5 / k;
    let right_leg = (g * 0.6662).cos() * 1.4 * h / k;
    let left_leg = (g * 0.6662 + std::f32::consts::PI).cos() * 1.4 * h / k;
    pose.rotate("right_arm", [right_arm, 0.0, 0.0]);
    pose.rotate("left_arm", [left_arm, 0.0, 0.0]);
    // Die winzigen 0.005 auf Gierung/Rollen der Beine stehen so im Jar -- sie verhindern,
    // dass zwei exakt deckungsgleiche Flaechen aneinander z-fighten.
    pose.rotate("right_leg", [right_leg, 0.005, 0.005]);
    pose.rotate("left_leg", [left_leg, -0.005, -0.005]);
    if riding {
        // Sitzhaltung (BipedEntityModel.setAngles, Zweig hasVehicle): die Arme gehen um ein
        // Fuenftel Pi hoch, die Beine stehen fast waagerecht nach vorn und leicht gespreizt.
        pose.rotate("right_arm", [right_arm - std::f32::consts::PI / 5.0, 0.0, 0.0]);
        pose.rotate("left_arm", [left_arm - std::f32::consts::PI / 5.0, 0.0, 0.0]);
        pose.rotate("right_leg", [-1.4137167, std::f32::consts::PI / 10.0, 0.07853982]);
        pose.rotate("left_leg", [-1.4137167, -std::f32::consts::PI / 10.0, -0.07853982]);
    }

    // 3. Armhaltung fuer gehaltene Gegenstaende -- VOR dem Handschlag, s. Kopfkommentar.
    // BLOCK schliesst ITEM aus: in Vanilla ist es EIN switch ueber die ArmPose, kein Stapel.
    // Eine Benutzungshaltung kommt ZUERST (HumanoidModel.setupAnim: der benutzende bzw.
    // zweihaendige Arm wird zuerst gesetzt); greift sie auf den anderen Arm ueber (Bogen,
    // Armbrust, Dreizack), bekommt der keine eigene Haltung mehr.
    let flach = glide.is_some() || lean > 0.0;
    let benutzt = use_pose.filter(|(_, p)| *p != UsePose::None);
    if let Some((rechts, haltung)) = benutzt {
        apply_use_pose(&mut pose, rechts, haltung, sneaking);
    }
    let frei = |rechts: bool| match benutzt {
        Some((arm, haltung)) => arm != rechts && !haltung.affects_offhand(),
        None => true,
    };
    if !frei(true) {
    } else if block_right {
        apply_blocking_arm_pose(&mut pose, true);
    } else if spear_right {
        apply_spear_arm_pose(&mut pose, true, flach);
    } else if item_right {
        apply_item_arm_pose(&mut pose, true);
    }
    if !frei(false) {
    } else if block_left {
        apply_blocking_arm_pose(&mut pose, false);
    } else if spear_left {
        apply_spear_arm_pose(&mut pose, false, flach);
    } else if item_left {
        apply_item_arm_pose(&mut pose, false);
    }
    // 4. Der Handschlag.
    apply_hand_swing_typed(&mut pose, hand_swing, swing_right_arm, stab);

    if sneaking {
        // Zweig isInSneakingPose aus setAngles: Rumpf kippt, Gliedmassen wandern mit.
        let body = pose.get("body");
        pose.set(
            "body",
            PartPose {
                // Vanilla SETZT die Neigung (0.5) und laesst die Gierung stehen -- die kommt
                // vom Handschlag und darf hier nicht verlorengehen.
                rotation: [0.5, body.rotation[1], body.rotation[2]],
                pivot_offset: [body.pivot_offset[0], body.pivot_offset[1] + 3.2, body.pivot_offset[2]],
                rotation_matrix: None,
                hidden: false,
            },
        );
        // ADDIEREN, nicht setzen: Vanilla schreibt hier ueberall `+=`, und der Handschlag hat
        // die Drehpunkte der Arme vorher schon um die Koerperachse verschoben. Ein Setzen
        // machte diese Verschiebung beim Schleichen wieder zunichte.
        let bump = |pose: &mut Pose, part: &'static str, extra_pitch: f32, dy: f32, dz: f32| {
            let current = pose.get(part);
            pose.set(
                part,
                PartPose {
                    rotation: [current.rotation[0] + extra_pitch, current.rotation[1], current.rotation[2]],
                    pivot_offset: [current.pivot_offset[0], current.pivot_offset[1] + dy, current.pivot_offset[2] + dz],
                    rotation_matrix: None,
                    hidden: false,
                },
            );
        };
        bump(&mut pose, "right_arm", 0.4, 3.2, 0.0);
        bump(&mut pose, "left_arm", 0.4, 3.2, 0.0);
        // Die Beine wandern nicht nur nach hinten (pivotZ 0 -> 4), sondern auch ein Zehntel
        // Modellpixel nach UNTEN: Vanilla setzt pivotY = 12.2F, der Grundwert ist 12.0
        // (s. biped_parts). Ohne die 0.2 schwebt die Figur beim Schleichen minimal ueber dem
        // Boden -- winzig, aber es ist eine echte Abweichung vom Jar, kein Rundungsrest.
        bump(&mut pose, "right_leg", 0.0, 0.2, 4.0);
        bump(&mut pose, "left_leg", 0.0, 0.2, 4.0);
        bump(&mut pose, "head", 0.0, 4.2, 0.0);
    }
    // Zuletzt, wie in Vanilla: die swingArm-Aufrufe stehen in BipedEntityModel.setAngles
    // NACH dem Schleich-Zweig und addieren auf dessen Werte.
    // Wer durchs Fernrohr schaut, haelt den Arm still (kein bobModelPart an diesem Arm).
    let fernrohr = |rechts: bool| benutzt == Some((rechts, UsePose::Spyglass));
    apply_arm_sway(&mut pose, age, (!fernrohr(true), !fernrohr(false)));
    // Zuletzt die Schwimmbewegung -- in Vanilla steht dieser Zweig am Ende von setAngles,
    // nach den swingArm-Aufrufen. Die Reihenfolge zaehlt: er blendet mit `lean` von dem weg,
    // was vorher berechnet wurde (Laufanimation und Ruhe-Bewegung), statt es zu ersetzen.
    // Der schlagende Arm bleibt vom Schwimmzug unberuehrt (s. apply_swim_stroke).
    // Ein Arm mit Speerhaltung schwimmt ebenfalls nicht mit (Vanilla: `armPose != SPEAR`).
    let schlaegt = hand_swing > 0.0;
    let lean_links = if (schlaegt && !swing_right_arm) || spear_left { 0.0 } else { lean };
    let lean_rechts = if (schlaegt && swing_right_arm) || spear_right { 0.0 } else { lean };
    apply_swim_stroke(&mut pose, lean_links, lean_rechts, swing_progress, crawl);
    pose
}

#[cfg(test)]
mod use_pose_tests {
    use super::*;

    fn mit(use_pose: Option<(bool, UsePose)>, item_left: bool) -> Pose {
        biped_pose_full(0.0, 0.0, 0.0, 0.0, BipedExtras { item_left, use_pose, ..Default::default() })
    }

    // Beim Bogen zeigen beide Arme waagerecht nach vorn, der Bogenarm 0.4 weiter aussen.
    #[test]
    fn bogen_hebt_beide_arme() {
        let p = mit(Some((true, UsePose::Bow)), true);
        let (r, l) = (p.get("right_arm").rotation, p.get("left_arm").rotation);
        assert!((r[0] + std::f32::consts::FRAC_PI_2).abs() < 0.06);
        assert!((l[0] + std::f32::consts::FRAC_PI_2).abs() < 0.06);
        assert!((l[1] - r[1] - 0.6).abs() < 1e-4);
    }

    // Das Fernrohr belegt nur seinen Arm, der andere haelt sein Item wie sonst.
    #[test]
    fn fernrohr_laesst_den_anderen_arm_frei() {
        let p = mit(Some((true, UsePose::Spyglass)), true);
        let ohne = mit(None, true);
        assert_eq!(p.get("left_arm").rotation, ohne.get("left_arm").rotation);
        assert!(p.get("right_arm").rotation[0] < -1.5);
    }

    // Die Armbrust spannt der andere Arm von halb nach ganz oben.
    #[test]
    fn armbrust_zieht_mit_der_ladezeit() {
        let anfang = mit(Some((true, UsePose::CrossbowCharge { ticks: 0.0, max: 25.0 })), false);
        let ende = mit(Some((true, UsePose::CrossbowCharge { ticks: 25.0, max: 25.0 })), false);
        assert!(ende.get("left_arm").rotation[0] < anfang.get("left_arm").rotation[0]);
        assert!(ende.get("left_arm").rotation[1] > anfang.get("left_arm").rotation[1]);
    }
}

#[cfg(test)]
mod spear_tests {
    use super::*;

    fn nah(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    // Vor und nach dem Stich steht der Speer wie ohne Schwung -- kein Sprung an den Enden.
    #[test]
    fn stich_beginnt_und_endet_in_ruhe() {
        let ruhe = first_person_equip_offset(true, 0.0).apply([0.0; 3]);
        assert!(nah(first_person_stab_transform(true, 0.0, 0.0).apply([0.0; 3]), ruhe));
        assert!(nah(first_person_stab_transform(true, 0.0, 1.0).apply([0.0; 3]), ruhe));
        // Mittendrin ist er um -70 Grad um X gekippt: die Spitze (lokal +Y) zeigt nach vorn (-Z).
        let spitze = first_person_stab_transform(true, 0.0, 0.3).apply_direction([0.0, 1.0, 0.0]);
        assert!(spitze[2] < -0.9, "{spitze:?}");
    }

    #[test]
    fn speerarm_folgt_dem_blick_und_ist_begrenzt() {
        let mut pose = Pose::new();
        pose.rotate("head", [0.0, 0.0, 0.0]);
        apply_spear_arm_pose(&mut pose, true, false);
        let r = pose.get("right_arm").rotation;
        assert!((r[0] - (-std::f32::consts::FRAC_PI_2 + 0.8)).abs() < 1e-5);
        assert!((r[1] + 0.1).abs() < 1e-5);
        let mut pose = Pose::new();
        pose.rotate("head", [-3.0, 3.0, 0.0]);
        apply_spear_arm_pose(&mut pose, false, false);
        let l = pose.get("left_arm").rotation;
        assert!((l[0] - (-120f32).to_radians()).abs() < 1e-5);
        assert!((l[1] - 60f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn ansturm_beginnt_in_ruhe() {
        let k = SpearKinetic { delay: 15, dismount: 100, knockback: 200, damage: 300 };
        let ruhe = first_person_equip_offset(true, 0.0).apply([0.0; 3]);
        assert!(nah(first_person_spear_charge(true, 0.0, k).apply([0.0; 3]), ruhe));
    }

    #[test]
    fn easing_endpunkte() {
        for f in [easing::in_out_sine, easing::out_back, easing::in_out_expo, easing::in_out_back, easing::in_out_elastic, easing::out_cubic] {
            assert!(f(0.0).abs() < 1e-5);
            assert!((f(1.0) - 1.0).abs() < 1e-5);
        }
    }
}

#[cfg(test)]
mod hand_swing_tests {
    use super::*;

    fn mit_schlag(f: f32, rechts: bool, item_rechts: bool) -> Pose {
        biped_pose_full(
            0.0,
            0.0,
            0.0,
            0.0,
            BipedExtras { hand_swing: f, swing_right_arm: rechts, item_right: item_rechts, ..Default::default() },
        )
    }

    #[test]
    fn ohne_schlag_bleibt_die_pose_unveraendert() {
        let ohne = biped_pose(0.0, 0.0, 1.0, 1.0, false, 3.0);
        let auch_ohne =
            biped_pose_full(0.0, 0.0, 1.0, 1.0, BipedExtras { age: 3.0, hand_swing: 0.0, swing_right_arm: true, ..Default::default() });
        for teil in ["right_arm", "left_arm", "body", "head"] {
            assert_eq!(ohne.get(teil).rotation, auch_ohne.get(teil).rotation, "{teil}");
            assert_eq!(ohne.get(teil).pivot_offset, auch_ohne.get(teil).pivot_offset, "{teil}");
        }
    }

    #[test]
    // Der Grundfall: fremde Spieler schwangen frueher sichtbar gar nicht. Mindestanforderung ist
    // also, dass sich ueberhaupt etwas bewegt -- und zwar der Arm UND der Rumpf.
    fn ein_schlag_bewegt_arm_und_rumpf() {
        let ruhe = mit_schlag(0.0, true, false);
        let schlag = mit_schlag(0.4, true, false);
        assert!(
            (schlag.get("right_arm").rotation[0] - ruhe.get("right_arm").rotation[0]).abs() > 0.3,
            "der schlagende Arm muss sich deutlich heben"
        );
        assert!(schlag.get("body").rotation[1].abs() > 0.01, "der Rumpf dreht sich mit");
        // Der Drehpunkt wandert mit dem Rumpf -- ohne das bliebe der Arm an der alten
        // Schulter haengen.
        assert!(schlag.get("right_arm").pivot_offset[2].abs() > 0.01, "der Drehpunkt wandert mit");
    }

    #[test]
    fn der_linke_arm_schlaegt_spiegelbildlich() {
        let rechts = mit_schlag(0.4, true, false);
        let links = mit_schlag(0.4, false, false);
        assert!((rechts.get("body").rotation[1] + links.get("body").rotation[1]).abs() < 1e-6, "die Rumpfdrehung kehrt sich um");
        let hebt_rechts = rechts.get("right_arm").rotation[0];
        let hebt_links = links.get("left_arm").rotation[0];
        assert!(hebt_rechts < -0.5 && hebt_links < -0.5, "beide Arme muessen sich deutlich heben: {hebt_rechts} / {hebt_links}");
        // Nicht exakt gleich: der linke Arm bekommt in Vanilla die Rumpfdrehung ZUSAETZLICH
        // auf die Neigung (`leftArm.pitch += body.yaw`), der rechte nicht. Diese Unsymmetrie
        // steht so im Jar; sie ist genau so gross wie die Rumpfdrehung.
        let rumpf = links.get("body").rotation[1];
        assert!((hebt_rechts - (hebt_links - rumpf)).abs() < 1e-6, "bis auf die Rumpfdrehung muessen beide gleich sein");
        // Der jeweils ANDERE Arm macht die grosse Bewegung nicht mit.
        assert!(rechts.get("left_arm").rotation[0].abs() < 0.2);
    }

    #[test]
    // ECHTER BUG gewesen und der Grund, warum diese Funktion nicht einfach hinterher
    // aufgerufen wird: die Armhaltung fuer gehaltene Gegenstaende HALBIERT die vorhandene
    // Neigung. Laeuft sie nach dem Schlag, kommt nur der halbe Schlag heraus -- und im
    // Schwertkampf haelt man immer etwas in der Hand, der Fehler waere also immer sichtbar.
    fn ein_item_in_der_hand_daempft_den_schlag_nicht() {
        let f = 0.4;
        let ohne_item = mit_schlag(f, true, false).get("right_arm").rotation[0] - mit_schlag(0.0, true, false).get("right_arm").rotation[0];
        let mit_item = mit_schlag(f, true, true).get("right_arm").rotation[0] - mit_schlag(0.0, true, true).get("right_arm").rotation[0];
        assert!((ohne_item - mit_item).abs() < 1e-6, "Schlag mit Item {mit_item} vs. ohne {ohne_item} -- Reihenfolge verdreht");
        assert!(ohne_item.abs() > 0.3, "der Schlag selbst muss deutlich sein");
    }

    #[test]
    // Beim Schleichen wandern alle Drehpunkte nach oben. Das darf die Verschiebung des
    // Schlages nicht ueberschreiben (Vanilla addiert, es setzt nicht).
    fn schleichen_und_schlagen_vertragen_sich() {
        let nur_schlag = mit_schlag(0.4, true, false);
        let beides = biped_pose_full(
            0.0,
            0.0,
            0.0,
            0.0,
            BipedExtras { sneaking: true, hand_swing: 0.4, swing_right_arm: true, ..Default::default() },
        );
        assert!(
            (beides.get("right_arm").pivot_offset[2] - nur_schlag.get("right_arm").pivot_offset[2]).abs() < 1e-6,
            "Z-Versatz des Schlages fehlt"
        );
        assert!(beides.get("right_arm").pivot_offset[1] > 3.0, "der Schleich-Versatz nach oben fehlt");
        // Die Rumpfdrehung des Schlages ueberlebt die Schleich-Neigung.
        assert!((beides.get("body").rotation[1] - nur_schlag.get("body").rotation[1]).abs() < 1e-6);
        assert!((beides.get("body").rotation[0] - 0.5).abs() < 1e-6, "Schleich-Neigung fehlt");
    }

    #[test]
    // Vanillas Formel haengt an der KOPFNEIGUNG: wer nach unten schaut, holt weiter aus.
    fn die_blickneigung_geht_in_den_ausholer_ein() {
        let geradeaus = biped_pose_full(0.0, 0.0, 0.0, 0.0, BipedExtras { hand_swing: 0.5, swing_right_arm: true, ..Default::default() });
        let nach_unten = biped_pose_full(0.0, 60.0, 0.0, 0.0, BipedExtras { hand_swing: 0.5, swing_right_arm: true, ..Default::default() });
        assert!(
            (geradeaus.get("right_arm").rotation[0] - nach_unten.get("right_arm").rotation[0]).abs() > 0.05,
            "die Kopfneigung muss den Schlag veraendern"
        );
    }

    #[test]
    // Ueber den ganzen Verlauf darf nichts springen -- ein Sprung waere im Spiel ein Zucken.
    fn der_verlauf_ist_stetig_und_endet_wo_er_beginnt() {
        let mut vorher = mit_schlag(0.0, true, false).get("right_arm").rotation[0];
        let mut groesster_sprung = 0.0f32;
        for i in 1..=1000 {
            let f = i as f32 / 1000.0;
            let jetzt = mit_schlag(f, true, false).get("right_arm").rotation[0];
            groesster_sprung = groesster_sprung.max((jetzt - vorher).abs());
            vorher = jetzt;
        }
        // Der Anfang ist die steilste Stelle: outQuart hat bei 0 die Steigung 4, der Arm
        // schnellt also los und laeuft hinten aus. Ueber ein Tausendstel des Verlaufs sind
        // das rund 0.017 rad -- alles darueber waere ein Sprung, kein steiler Anstieg.
        assert!(groesster_sprung < 0.03, "Sprung von {groesster_sprung} rad im Verlauf");
        // Bei f = 1 ist der Arm wieder in Ruhe (sin(PI) = 0 in allen drei Summanden).
        assert!(
            (vorher - mit_schlag(0.0, true, false).get("right_arm").rotation[0]).abs() < 1e-5,
            "der Schlag endet nicht in der Ruhelage"
        );
    }
}
