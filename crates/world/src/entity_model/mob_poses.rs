// Posen der Vierbeiner, Katzen, Schildkroeten, Creeper und Spinnen.

use super::*;

// QuadrupedEntityModel.setAngles: Kopf wie beim Biped, die vier Beine schwingen paarweise
// gegenlaeufig (Diagonalgang).
#[must_use] 
pub fn quadruped_pose(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32) -> Pose {
    const DEG: f32 = std::f32::consts::PI / 180.0;
    let mut pose = Pose::new();
    pose.rotate("head", [head_pitch * DEG, head_yaw_rel * DEG, 0.0]);
    let g = swing_progress;
    let h = swing_amount;
    pose.rotate("right_hind_leg", [(g * 0.6662).cos() * 1.4 * h, 0.0, 0.0]);
    pose.rotate("left_hind_leg", [(g * 0.6662 + std::f32::consts::PI).cos() * 1.4 * h, 0.0, 0.0]);
    pose.rotate("right_front_leg", [(g * 0.6662 + std::f32::consts::PI).cos() * 1.4 * h, 0.0, 0.0]);
    pose.rotate("left_front_leg", [(g * 0.6662).cos() * 1.4 * h, 0.0, 0.0]);
    pose
}

// FelineModel.setupAnim fuer ein erwachsenes Tier (ageScale 1). Die Pose ADDIERT auf die
// Grunddrehung (s. emit_part) -- wo Vanilla einen Winkel absolut setzt, steht hier die
// Differenz zur Grunddrehung (Rumpf pi/2, erstes Schwanzstueck 0.9). Das Hinlegen auf ein Bett
// und der Sprint-Schwanz fehlen: dafuer fehlen die Daten vom Server.
#[must_use] 
pub fn feline_pose(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32, sitting: bool, crouching: bool) -> Pose {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    const DEG: f32 = PI / 180.0;
    const TAIL1_BASE: f32 = 0.9;
    let mut pose = Pose::new();
    let schieb = |pose: &mut Pose, part: &'static str, rotation: [f32; 3], offset: [f32; 3]| {
        pose.set(part, PartPose { rotation, pivot_offset: offset, rotation_matrix: None, hidden: false });
    };
    let mut body_off = [0.0f32; 3];
    let mut head_off = [0.0f32; 3];
    let mut tail1_off = [0.0f32; 3];
    let mut tail2_off = [0.0f32; 3];
    let mut tail1_rot = 0.0f32;
    if crouching {
        body_off[1] += 1.0;
        head_off[1] += 2.0;
        tail1_off[1] += 1.0;
        tail2_off[1] -= 4.0;
        tail2_off[2] += 2.0;
        // Vanilla setzt hier auch tail2 auf pi/2 -- das ueberschreibt der Gang unten wieder.
        tail1_rot = FRAC_PI_2 - TAIL1_BASE;
    }
    let head = [head_pitch * DEG, head_yaw_rel * DEG, 0.0];
    let g = swing_progress;
    let h = swing_amount;
    if sitting {
        body_off[1] -= 4.0;
        body_off[2] += 5.0;
        head_off[1] -= 3.3;
        head_off[2] += 1.0;
        tail1_off[1] += 8.0;
        tail1_off[2] -= 2.0;
        tail2_off[1] += 2.0;
        tail2_off[2] -= 0.8;
        schieb(&mut pose, "body", [FRAC_PI_4 - FRAC_PI_2, 0.0, 0.0], body_off);
        schieb(&mut pose, "head", head, head_off);
        schieb(&mut pose, "tail1", [1.727_876_1 - TAIL1_BASE, 0.0, 0.0], tail1_off);
        schieb(&mut pose, "tail2", [2.670_354, 0.0, 0.0], tail2_off);
        schieb(&mut pose, "left_front_leg", [-PI / 20.0, 0.0, 0.0], [0.0, 2.0, -2.0]);
        schieb(&mut pose, "right_front_leg", [-PI / 20.0, 0.0, 0.0], [0.0, 2.0, -2.0]);
        schieb(&mut pose, "left_hind_leg", [-FRAC_PI_2, 0.0, 0.0], [0.0, 3.0, -4.0]);
        schieb(&mut pose, "right_hind_leg", [-FRAC_PI_2, 0.0, 0.0], [0.0, 3.0, -4.0]);
        return pose;
    }
    let schwanz = if crouching { 0.471_238_94 } else { FRAC_PI_4 };
    let tail2_rot = 1.727_876_1 + schwanz * g.cos() * h;
    schieb(&mut pose, "body", [0.0; 3], body_off);
    schieb(&mut pose, "head", head, head_off);
    schieb(&mut pose, "tail1", [tail1_rot, 0.0, 0.0], tail1_off);
    schieb(&mut pose, "tail2", [tail2_rot, 0.0, 0.0], tail2_off);
    pose.rotate("left_hind_leg", [(g * 0.6662).cos() * h, 0.0, 0.0]);
    pose.rotate("right_hind_leg", [(g * 0.6662 + PI).cos() * h, 0.0, 0.0]);
    pose.rotate("left_front_leg", [(g * 0.6662 + PI).cos() * h, 0.0, 0.0]);
    pose.rotate("right_front_leg", [(g * 0.6662).cos() * h, 0.0, 0.0]);
    pose
}

// TurtleModel.setupAnim: erst die Vierbeiner-Pose, dann an Land ein Paddeln um die
// Hochachse, im Wasser ein Schlagen der Flossen.
#[must_use] 
pub fn turtle_pose(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32, on_land: bool) -> Pose {
    let mut pose = quadruped_pose(head_yaw_rel, head_pitch, swing_progress, swing_amount);
    let (g, h) = (swing_progress, swing_amount);
    let x = |pose: &Pose, part: &str| pose.get(part).rotation[0];
    if on_land {
        let c = (g * 5.0).cos();
        let (rf, lf, rh, lh) = (x(&pose, "right_front_leg"), x(&pose, "left_front_leg"), x(&pose, "right_hind_leg"), x(&pose, "left_hind_leg"));
        pose.rotate("right_front_leg", [rf, -c * 8.0 * h, 0.0]);
        pose.rotate("left_front_leg", [lf, c * 8.0 * h, 0.0]);
        pose.rotate("right_hind_leg", [rh, -c * 3.0 * h, 0.0]);
        pose.rotate("left_hind_leg", [lh, c * 3.0 * h, 0.0]);
    } else {
        let schlag = (g * 0.6662 * 0.6).cos() * 0.5 * h;
        let (rf, lf) = (x(&pose, "right_front_leg"), x(&pose, "left_front_leg"));
        pose.rotate("right_hind_leg", [schlag, 0.0, 0.0]);
        pose.rotate("left_hind_leg", [-schlag, 0.0, 0.0]);
        pose.rotate("right_front_leg", [rf, 0.0, -schlag]);
        pose.rotate("left_front_leg", [lf, 0.0, schlag]);
    }
    pose
}

// CreeperEntityModel.setAngles -- dieselbe Beinformel wie beim Vierbeiner, plus Kopf.
#[must_use] 
pub fn creeper_pose(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32) -> Pose {
    quadruped_pose(head_yaw_rel, head_pitch, swing_progress, swing_amount)
}

// SpiderEntityModel.setAngles: der Kopf dreht mit, die acht Beine schwingen in vier
// gegenlaeufigen Paaren um ihre Grunddrehung (die bleibt erhalten, s. Pose-Semantik).
#[must_use] 
pub fn spider_pose(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32) -> Pose {
    const DEG: f32 = std::f32::consts::PI / 180.0;
    let mut pose = Pose::new();
    pose.rotate("head", [head_pitch * DEG, head_yaw_rel * DEG, 0.0]);
    let g = swing_progress;
    let h = swing_amount;
    // Vanilla: -(cos(g*0.6662*2 + phase) * 0.4) * h je Beinpaar, mit vier Phasenlagen.
    let leg = |phase: f32| -((g * 0.6662 * 2.0 + phase).cos() * 0.4) * h;
    let pairs: [(&'static str, &'static str, f32); 4] = [
        ("right_hind_leg", "left_hind_leg", 0.0),
        ("right_middle_hind_leg", "left_middle_hind_leg", std::f32::consts::PI),
        ("right_middle_front_leg", "left_middle_front_leg", std::f32::consts::FRAC_PI_2),
        ("right_front_leg", "left_front_leg", std::f32::consts::PI * 1.5),
    ];
    for (right, left, phase) in pairs {
        let amount = leg(phase);
        pose.rotate(right, [0.0, 0.0, -amount]);
        pose.rotate(left, [0.0, 0.0, amount]);
    }
    pose
}
