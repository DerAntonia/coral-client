// Schildmodelle.

use super::*;

// --- Schilder (SignBlockEntityRenderer / HangingSignBlockEntityRenderer, 1.21.11) ---------

// Die Holzarten mit Schildern; Textur entity/signs/<holz> bzw. entity/signs/hanging/<holz>.
pub const SIGN_WOODS: [&str; 12] =
    ["oak", "spruce", "birch", "acacia", "cherry", "jungle", "dark_oak", "pale_oak", "mangrove", "bamboo", "crimson", "warped"];

pub const SIGN_TEXTURES: [&str; 12] = [
    "entity/signs/oak",
    "entity/signs/spruce",
    "entity/signs/birch",
    "entity/signs/acacia",
    "entity/signs/cherry",
    "entity/signs/jungle",
    "entity/signs/dark_oak",
    "entity/signs/pale_oak",
    "entity/signs/mangrove",
    "entity/signs/bamboo",
    "entity/signs/crimson",
    "entity/signs/warped",
];

pub const HANGING_SIGN_TEXTURES: [&str; 12] = [
    "entity/signs/hanging/oak",
    "entity/signs/hanging/spruce",
    "entity/signs/hanging/birch",
    "entity/signs/hanging/acacia",
    "entity/signs/hanging/cherry",
    "entity/signs/hanging/jungle",
    "entity/signs/hanging/dark_oak",
    "entity/signs/hanging/pale_oak",
    "entity/signs/hanging/mangrove",
    "entity/signs/hanging/bamboo",
    "entity/signs/hanging/crimson",
    "entity/signs/hanging/warped",
];

// SignModel.getTexturedModelData: das Brett und (nur stehend) der Stiel.
#[must_use] 
pub fn sign_model(standing: bool) -> Model {
    let mut parts = vec![Part::new("sign", [0.0; 3], vec![Cuboid::new([0.0, 0.0], [-12.0, -14.0, -1.0], [24.0, 12.0, 2.0])])];
    if standing {
        parts.push(Part::new("stick", [0.0; 3], vec![Cuboid::new([0.0, 14.0], [-1.0, -2.0, -1.0], [2.0, 14.0, 2.0])]));
    }
    Model { texture: SIGN_TEXTURES[0], tex_size: [64.0, 32.0], parts }
}

// Wie ein Haengeschild befestigt ist (HangingSignBlockEntityRenderer.AttachmentType).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HangingAttachment {
    Wall,
    Ceiling,
    CeilingMiddle,
}

// HangingSignModel.getTexturedModelData: Brett, bei Wandbefestigung die Stange, und die
// Ketten -- schraeg gekreuzt an zwei Punkten, bei "attached" (mittig) ein gerades Stueck.
#[must_use] 
pub fn hanging_sign_model(attachment: HangingAttachment) -> Model {
    use std::f32::consts::FRAC_PI_4;
    let mut parts = vec![Part::new("board", [0.0; 3], vec![Cuboid::new([0.0, 12.0], [-7.0, 0.0, -1.0], [14.0, 10.0, 2.0])])];
    if attachment == HangingAttachment::Wall {
        parts.push(Part::new("plank", [0.0; 3], vec![Cuboid::new([0.0, 0.0], [-8.0, -6.0, -2.0], [16.0, 2.0, 4.0])]));
    }
    if attachment == HangingAttachment::CeilingMiddle {
        parts.push(Part::new("vChains", [0.0; 3], vec![Cuboid::new([14.0, 6.0], [-6.0, -6.0, 0.0], [12.0, 6.0, 0.0])]));
    } else {
        let kette = |name: &'static str, u: f32, x: f32, gier: f32| {
            Part::new(name, [x, -6.0, 0.0], vec![Cuboid::new([u, 6.0], [-1.5, 0.0, 0.0], [3.0, 6.0, 0.0])]).rotated([0.0, gier, 0.0])
        };
        parts.push(kette("chainL1", 0.0, -5.0, -FRAC_PI_4));
        parts.push(kette("chainL2", 6.0, -5.0, FRAC_PI_4));
        parts.push(kette("chainR1", 0.0, 5.0, -FRAC_PI_4));
        parts.push(kette("chainR2", 6.0, 5.0, FRAC_PI_4));
    }
    Model { texture: HANGING_SIGN_TEXTURES[0], tex_size: [64.0, 32.0], parts }
}

// Die Art eines Schildblocks: stehend, an der Wand, haengend (Decke/Wand).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignKind {
    Standing,
    Wall,
    Hanging(HangingAttachment),
}

// Holzindex (in SIGN_WOODS) und Art eines Schildblocks, oder None fuer alles andere.
#[must_use] 
pub fn sign_kind(block: &str) -> Option<(usize, SignKind)> {
    let (holz, art) = if let Some(h) = block.strip_suffix("_wall_hanging_sign") {
        (h, SignKind::Hanging(HangingAttachment::Wall))
    } else if let Some(h) = block.strip_suffix("_hanging_sign") {
        (h, SignKind::Hanging(HangingAttachment::Ceiling))
    } else if let Some(h) = block.strip_suffix("_wall_sign") {
        (h, SignKind::Wall)
    } else if let Some(h) = block.strip_suffix("_sign") {
        (h, SignKind::Standing)
    } else {
        return None;
    };
    Some((SIGN_WOODS.iter().position(|w| *w == holz)?, art))
}

// Vanillas Matrixkette bis vor das Modell bzw. den Text (AbstractSignBlockEntityRenderer.render
// -> applyTransforms), in Bloecken ab der Blockecke:
//   stehend/Wand: translate(0.5, 0.75 * 2/3, 0.5), rotY(-grad), Wand zusaetzlich
//                 translate(0, -0.3125, -0.4375)
//   haengend:     translate(0.5, 0.9375, 0.5), rotY(-grad), translate(0, -0.3125, 0)
#[must_use] 
pub fn sign_base(block: [f32; 3], kind: SignKind, degrees: f32) -> Affine {
    let t = Affine::translation(block);
    if let SignKind::Hanging(_) = kind { t
    .then(Affine::translation([0.5, 0.9375, 0.5]))
    .then(Affine::rotation_y_deg(-degrees))
    .then(Affine::translation([0.0, -0.3125, 0.0])) } else {
        let b = t.then(Affine::translation([0.5, 0.75 * (2.0 / 3.0), 0.5])).then(Affine::rotation_y_deg(-degrees));
        if kind == SignKind::Wall {
            b.then(Affine::translation([0.0, -0.3125, -0.4375]))
        } else {
            b
        }
    }
}

// renderSign: die Modellskalierung auf sign_base (2/3 bzw. 1, jeweils y und z gespiegelt).
#[must_use] 
pub fn sign_model_transform(base: Affine, kind: SignKind) -> Affine {
    let s = if matches!(kind, SignKind::Hanging(_)) { 1.0 } else { 2.0 / 3.0 };
    base.then(Affine::scale([s, -s, -s]))
}
