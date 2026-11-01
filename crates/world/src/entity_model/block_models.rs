// Biped, Schild, Elytren, Creeper, Enderman, Spinne, Betten, Shulkerkisten, Truhen und Statuen.

use super::*;

// // Zombies, Skelette & Co: das nackte Biped mit eigener Textur. `tex_size` ist bei diesen
// Modellen 64x32 (die alte Skin-Aufteilung ohne zweite Lage).
#[must_use] 
pub fn biped_model(texture: &'static str, dilation: f32) -> Model {
    Model { texture, tex_size: [64.0, 32.0], parts: biped_parts(dilation, 0.0) }
}

// ShieldEntityModel.getTexturedModelData -- Platte und Griff, Texturgroesse 64x64:
//
//     plate:  uv(0,0)  cuboid(-6, -11, -2, 12, 22, 1)
//     handle: uv(26,0) cuboid(-1,  -3, -1,  2,  6, 6)
//
// Das Schild ist in Vanilla KEIN gewoehnliches Item-Modell: shield.json enthaelt gar keine
// Geometrie, nur Halte-Transformationen und eine Partikeltextur. Gezeichnet wird es von
// einem SpecialModelRenderer mit genau diesem Entity-Modell (ShieldModelRenderer). Ohne den
// Sonderweg bleibt in der Hand entweder nichts uebrig oder -- schlimmer -- die
// Partikeltextur "block/dark_oak_planks" als flaches Brett.
//
// Banner-Muster bleiben bewusst aussen vor: sie stecken in der BANNER_PATTERNS-Komponente
// der Item-Daten, die dieser Client nicht ausliest. Deshalb immer shield_base_nopattern --
// dieselbe Textur, die Vanilla fuer ein ungemustertes Schild nimmt.
#[must_use] 
pub fn shield_model() -> Model {
    Model {
        texture: "entity/shield_base_nopattern",
        tex_size: [64.0, 64.0],
        parts: vec![
            Part::new("plate", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-6.0, -11.0, -2.0], [12.0, 22.0, 1.0])]),
            Part::new("handle", [0.0, 0.0, 0.0], vec![Cuboid::new([26.0, 0.0], [-1.0, -3.0, -1.0], [2.0, 6.0, 6.0])]),
        ],
    }
}

// Die Elytra-Flaechen (ElytraEntityModel.getTexturedModelData), Textur 64x32:
//
//     left_wing:  uv(22,0)          cuboid(-10, 0, 0, 10, 20, 2, Dilation 1.0)  Pivot ( 5, 0, 0)
//     right_wing: uv(22,0) mirrored cuboid(  0, 0, 0, 10, 20, 2, Dilation 1.0)  Pivot (-5, 0, 0)
//
// Die Grunddrehung aus ModelTransform.of ueberschreibt setAngles in jedem Bild vollstaendig --
// deshalb steht sie nicht hier, sondern kommt ganz aus elytra_pose.
pub const ELYTRA_TEXTURE: &str = "entity/equipment/wings/elytra";

#[must_use] 
pub fn elytra_model() -> Model {
    Model {
        texture: ELYTRA_TEXTURE,
        tex_size: [64.0, 32.0],
        parts: vec![
            Part::new("left_wing", [5.0, 0.0, 0.0], vec![Cuboid::new([22.0, 0.0], [-10.0, 0.0, 0.0], [10.0, 20.0, 2.0]).dilated(1.0)]),
            Part::new("right_wing", [-5.0, 0.0, 0.0], vec![Cuboid::new([22.0, 0.0], [0.0, 0.0, 0.0], [10.0, 20.0, 2.0]).dilated(1.0).mirrored()]),
        ],
    }
}

// ElytraEntityModel.setAngles. Angelegt haengen die Fluegel leicht schraeg nach aussen; beim
// Gleiten breiten sie sich aus -- je steiler der Sturzflug, desto mehr legen sie sich wieder an
// (j faellt mit der Sinkrate). Beim Schleichen klappen sie nach hinten.
//
// Vanilla glaettet die Winkel bei Spielern zusaetzlich (leftWingPitch += (f - leftWingPitch) *
// 0.1 je Bild); hier stehen sie direkt auf dem Zielwert, der Uebergang ist dadurch haerter.
#[must_use] 
pub fn elytra_pose(glide: Option<&Glide>, sneaking: bool) -> Pose {
    use std::f32::consts::PI;
    let (mut pitch, mut roll, mut origin_y, mut yaw) = (PI / 12.0, -PI / 12.0, 0.0, 0.0);
    if let Some(glide) = glide {
        let [x, y, z] = glide.velocity;
        let mut j = 1.0;
        if y < 0.0 {
            let len = (x * x + y * y + z * z).sqrt();
            j = 1.0 - (-y / len).powf(1.5);
        }
        pitch = j * (PI / 9.0) + (1.0 - j) * pitch;
        roll = j * (-PI / 2.0) + (1.0 - j) * roll;
    } else if sneaking {
        pitch = PI * 2.0 / 9.0;
        roll = -PI / 4.0;
        origin_y = 3.0;
        yaw = 0.087_266_46;
    }
    let mut pose = Pose::new();
    pose.set("left_wing", PartPose { rotation: [pitch, yaw, roll], pivot_offset: [0.0, origin_y, 0.0], rotation_matrix: None, hidden: false });
    pose.set("right_wing", PartPose { rotation: [pitch, -yaw, -roll], pivot_offset: [0.0, origin_y, 0.0], rotation_matrix: None, hidden: false });
    pose
}

// CreeperEntityModel.getTexturedModelData
#[must_use] 
pub fn creeper_model() -> Model {
    let leg = |name: &'static str, x: f32, z: f32| {
        Part::new(name, [x, 18.0, z], vec![Cuboid::new([0.0, 16.0], [-2.0, 0.0, -2.0], [4.0, 6.0, 4.0])])
    };
    Model {
        texture: "entity/creeper/creeper",
        tex_size: [64.0, 32.0],
        parts: vec![
            Part::new("head", [0.0, 6.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0])]),
            Part::new("body", [0.0, 6.0, 0.0], vec![Cuboid::new([16.0, 16.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0])]),
            leg("right_hind_leg", -2.0, 4.0),
            leg("left_hind_leg", 2.0, 4.0),
            leg("right_front_leg", -2.0, -4.0),
            leg("left_front_leg", 2.0, -4.0),
        ],
    }
}

// EndermanEntityModel.getTexturedModelData -- Biped mit pivotOffsetY -14 und sehr langen
// Gliedmassen (2x30 statt 4x12).
#[must_use] 
pub fn enderman_model() -> Model {
    let limb = |name: &'static str, pivot: [f32; 3], y: f32, mirror: bool| {
        let mut cuboid = Cuboid::new([56.0, 0.0], [-1.0, y, -1.0], [2.0, 30.0, 2.0]);
        if mirror {
            cuboid = cuboid.mirrored();
        }
        Part::new(name, pivot, vec![cuboid])
    };
    Model {
        texture: "entity/enderman/enderman",
        tex_size: [64.0, 32.0],
        parts: vec![
            Part::new("head", [0.0, -13.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0])]).with(Part::new(
                "hat",
                [0.0, 0.0, 0.0],
                vec![Cuboid::new([0.0, 16.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]).dilated(-0.5)],
            )),
            Part::new("body", [0.0, -14.0, 0.0], vec![Cuboid::new([32.0, 16.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0])]),
            limb("right_arm", [-5.0, -12.0, 0.0], -2.0, false),
            limb("left_arm", [5.0, -12.0, 0.0], -2.0, true),
            limb("right_leg", [-2.0, -5.0, 0.0], 0.0, false),
            limb("left_leg", [2.0, -5.0, 0.0], 0.0, true),
        ],
    }
}

// SpiderEntityModel.getTexturedModelData
#[must_use] 
pub fn spider_model(texture: &'static str) -> Model {
    // Die acht Beine unterscheiden sich nur in Pivot und Grunddrehung; die rechten nutzen
    // einen Kasten, der nach -X wegsteht, die linken den gespiegelten nach +X.
    let right = Cuboid::new([18.0, 0.0], [-15.0, -1.0, -1.0], [16.0, 2.0, 2.0]);
    let left = Cuboid::new([18.0, 0.0], [-1.0, -1.0, -1.0], [16.0, 2.0, 2.0]).mirrored();
    let leg = |name: &'static str, x: f32, z: f32, yaw: f32, roll: f32, mirrored: bool| {
        Part::new(name, [x, 15.0, z], vec![if mirrored { left } else { right }]).rotated([0.0, yaw, roll])
    };
    use std::f32::consts::{FRAC_PI_4, FRAC_PI_8};
    Model {
        texture,
        tex_size: [64.0, 32.0],
        parts: vec![
            Part::new("head", [0.0, 15.0, -3.0], vec![Cuboid::new([32.0, 4.0], [-4.0, -4.0, -8.0], [8.0, 8.0, 8.0])]),
            Part::new("body0", [0.0, 15.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-3.0, -3.0, -3.0], [6.0, 6.0, 6.0])]),
            Part::new("body1", [0.0, 15.0, 9.0], vec![Cuboid::new([0.0, 12.0], [-5.0, -4.0, -6.0], [10.0, 8.0, 12.0])]),
            leg("right_hind_leg", -4.0, 2.0, FRAC_PI_4, -FRAC_PI_4, false),
            leg("left_hind_leg", 4.0, 2.0, -FRAC_PI_4, FRAC_PI_4, true),
            leg("right_middle_hind_leg", -4.0, 1.0, FRAC_PI_8, -0.581_194_64, false),
            leg("left_middle_hind_leg", 4.0, 1.0, -FRAC_PI_8, 0.581_194_64, true),
            leg("right_middle_front_leg", -4.0, 0.0, -FRAC_PI_8, -0.581_194_64, false),
            leg("left_middle_front_leg", 4.0, 0.0, FRAC_PI_8, 0.581_194_64, true),
            leg("right_front_leg", -4.0, -1.0, -FRAC_PI_4, -FRAC_PI_4, false),
            leg("left_front_leg", 4.0, -1.0, FRAC_PI_4, FRAC_PI_4, true),
        ],
    }
}

// Bett -- ein BLOCK, aber mit Entity-Modell und Entity-Textur (s. block_entity.rs). Die
// Zahlen stammen aus BedBlockEntityRenderer.getHeadTexturedModelData/getFootTexturedModelData
// (1.21.11), Texturgroesse 64x64:
//
//   Kopfteil: main uv(0,0)  cuboid(0,0,0, 16,16,6)
//             left_leg  uv(50,6)  cuboid(0,6,0, 3,3,3)   rot(90°, 0, 90°)
//             right_leg uv(50,18) cuboid(-16,6,0, 3,3,3) rot(90°, 0, 180°)
//   Fussteil: main uv(0,22) cuboid(0,0,0, 16,16,6)
//             left_leg  uv(50,0)  cuboid(0,6,-16, 3,3,3)  rot(90°, 0, 0)
//             right_leg uv(50,12) cuboid(-16,6,-16, 3,3,3) rot(90°, 0, 270°)
//
// Die Beine stehen bewusst auf NEGATIVEN Koordinaten und werden per Grunddrehung an ihren
// Platz gekippt -- so steht es im Jar, und nur so treffen die Textur-Offsets.
use std::f32::consts::{FRAC_PI_2, PI};

#[must_use] 
pub fn bed_head_model() -> Model {
    Model {
        texture: BED_MODEL_TEXTURE,
        tex_size: [64.0, 64.0],
        parts: vec![
            Part::new("main", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [0.0, 0.0, 0.0], [16.0, 16.0, 6.0])]),
            Part::new("left_leg", [0.0, 0.0, 0.0], vec![Cuboid::new([50.0, 6.0], [0.0, 6.0, 0.0], [3.0, 3.0, 3.0])])
                .rotated([FRAC_PI_2, 0.0, FRAC_PI_2]),
            Part::new("right_leg", [0.0, 0.0, 0.0], vec![Cuboid::new([50.0, 18.0], [-16.0, 6.0, 0.0], [3.0, 3.0, 3.0])])
                .rotated([FRAC_PI_2, 0.0, PI]),
        ],
    }
}

#[must_use] 
pub fn bed_foot_model() -> Model {
    Model {
        texture: BED_MODEL_TEXTURE,
        tex_size: [64.0, 64.0],
        parts: vec![
            Part::new("main", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 22.0], [0.0, 0.0, 0.0], [16.0, 16.0, 6.0])]),
            Part::new("left_leg", [0.0, 0.0, 0.0], vec![Cuboid::new([50.0, 0.0], [0.0, 6.0, -16.0], [3.0, 3.0, 3.0])])
                .rotated([FRAC_PI_2, 0.0, 0.0]),
            Part::new("right_leg", [0.0, 0.0, 0.0], vec![Cuboid::new([50.0, 12.0], [-16.0, 6.0, -16.0], [3.0, 3.0, 3.0])]).rotated([
                FRAC_PI_2,
                0.0,
                3.0 * FRAC_PI_2,
            ]),
        ],
    }
}

// Shulkerkiste: ShulkerEntityModel.getShulkerBoxTexturedModelData (1.21.11) -- Deckel und
// Unterteil, ohne Kopf, Texturgroesse 64x64:
//   lid  uv(0,0)  cuboid(-8,-16,-8, 16,12,16) origin(0,24,0)
//   base uv(0,28) cuboid(-8,-8,-8,  16,8,16)  origin(0,24,0)
// Gecacht, weil es fuer jede Kiste in jedem Bild gebraucht wird und sich nie aendert.
pub fn shulker_box_model() -> &'static Model {
    static MODEL: std::sync::OnceLock<Model> = std::sync::OnceLock::new();
    MODEL.get_or_init(|| Model {
        texture: "entity/shulker/shulker",
        tex_size: [64.0, 64.0],
        parts: vec![
            Part::new("lid", [0.0, 24.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-8.0, -16.0, -8.0], [16.0, 12.0, 16.0])]),
            Part::new("base", [0.0, 24.0, 0.0], vec![Cuboid::new([0.0, 28.0], [-8.0, -8.0, -8.0], [16.0, 8.0, 16.0])]),
        ],
    })
}

// ShulkerBoxBlockModel.setAngles: der Deckel hebt sich um bis zu 8 Pixel und dreht sich dabei
// um 270 Grad. `openness` 0 = zu, 1 = offen (ShulkerBoxBlockEntity.getAnimationProgress).
#[must_use] 
pub fn shulker_box_pose(openness: f32) -> Pose {
    let mut pose = Pose::new();
    pose.set("lid", PartPose { rotation: [0.0, (270.0 * openness).to_radians(), 0.0], pivot_offset: [0.0, -openness * 0.5 * 16.0, 0.0], rotation_matrix: None, hidden: false });
    pose
}

// ShulkerBoxBlockEntityRenderer.setTransforms:
//   translate(0.5, 0.5, 0.5); scale(0.9995); multiply(facing.getRotationQuaternion());
//   scale(1, -1, -1); translate(0, -1, 0)
// Die 0.9995 halten die Kiste minimal innerhalb des Blocks (kein Z-Fighting mit Nachbarn).
#[must_use] 
pub fn shulker_box_transform(block: [f32; 3], facing: &str) -> Affine {
    // Direction.getRotationQuaternion (rotationXYZ = Rx * Ry * Rz).
    let drehung = match facing {
        "down" => rotation_x_deg(180.0),
        "north" => Affine::rotation_xyz_deg(90.0, 0.0, 180.0),
        "south" => rotation_x_deg(90.0),
        "west" => Affine::rotation_xyz_deg(90.0, 0.0, 90.0),
        "east" => Affine::rotation_xyz_deg(90.0, 0.0, -90.0),
        _ => Affine::IDENTITY,
    };
    Affine::translation([block[0] + 0.5, block[1] + 0.5, block[2] + 0.5])
        .then(Affine::scale([0.9995; 3]))
        .then(drehung)
        .then(Affine::scale([1.0, -1.0, -1.0]))
        .then(Affine::translation([0.0, -1.0, 0.0]))
}

// Truhe: ChestBlockModel (1.21.11), Texturgroesse 64x64. Anders als die Lebewesen-Modelle
// steht sie mit Y nach OBEN im Block (keine Spiegelung), Einheiten Blockpixel:
//   einzeln: bottom uv(0,19) (1,0,1, 14,10,14); lid uv(0,0) (1,0,0, 14,5,14) origin(0,9,1);
//            lock uv(0,0) (7,-2,14, 2,4,1) origin(0,9,1)
//   rechts:  15 breit ab x=1, Schloss (15,-2,14, 1,4,1)
//   links:   15 breit ab x=0, Schloss (0,-2,14, 1,4,1)
pub fn chest_model(half: crate::block_entity::ChestHalf) -> &'static Model {
    use crate::block_entity::ChestHalf;
    static MODELS: std::sync::OnceLock<[Model; 3]> = std::sync::OnceLock::new();
    let baue = |x: f32, breite: f32, schloss_x: f32, schloss_breite: f32| Model {
        texture: "entity/chest/normal",
        tex_size: [64.0, 64.0],
        parts: vec![
            Part::new("bottom", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 19.0], [x, 0.0, 1.0], [breite, 10.0, 14.0])]),
            Part::new("lid", [0.0, 9.0, 1.0], vec![Cuboid::new([0.0, 0.0], [x, 0.0, 0.0], [breite, 5.0, 14.0])]),
            Part::new("lock", [0.0, 9.0, 1.0], vec![Cuboid::new([0.0, 0.0], [schloss_x, -2.0, 14.0], [schloss_breite, 4.0, 1.0])]),
        ],
    };
    let modelle = MODELS.get_or_init(|| [baue(1.0, 14.0, 7.0, 2.0), baue(1.0, 15.0, 15.0, 1.0), baue(0.0, 15.0, 0.0, 1.0)]);
    &modelle[match half {
        ChestHalf::Single => 0,
        ChestHalf::Right => 1,
        ChestHalf::Left => 2,
    }]
}

// ChestBlockEntityRenderer.render + ChestBlockModel.setAngles: der Fortschritt wird erst
// ausgebremst (f = 1 - (1-p)^3), dann klappen Deckel und Schloss um bis zu 90 Grad hoch.
#[must_use] 
pub fn chest_pose(openness: f32) -> Pose {
    let f = 1.0 - (1.0 - openness).powi(3);
    let winkel = -(f * std::f32::consts::FRAC_PI_2);
    let mut pose = Pose::new();
    pose.rotate("lid", [winkel, 0.0, 0.0]);
    pose.rotate("lock", [winkel, 0.0, 0.0]);
    pose
}

// translate(0.5, 0.5, 0.5); rotateY(-yaw); translate(-0.5, -0.5, -0.5) -- ab der Blockecke.
#[must_use] 
pub fn chest_transform(block: [f32; 3], yaw_deg: f32) -> Affine {
    Affine::translation([block[0] + 0.5, block[1] + 0.5, block[2] + 0.5])
        .then(Affine::rotation_y_deg(-yaw_deg))
        .then(Affine::translation([-0.5, -0.5, -0.5]))
}

// CopperGolemStatueModel.setupAnim, fuer Block wie Item: die Wurzel auf y = 0 und um Z halb
// gedreht, in JEDER Haltung. Die Layer sind verschieden gebacken -- Stehen mit der Wurzel bei
// 24, Sitzen, Laufen und Stern bei 0. Wie ein Lebewesen eingebettet (Wurzel 24 = Fusshoehe)
// schwebten die drei anderen Haltungen deshalb 24 Pixel = 1,5 Bloecke hoch.
#[must_use] 
pub fn copper_golem_statue_pose(model: &Model) -> Pose {
    let mut pose = Pose::new();
    if let Some(wurzel) = model.parts.iter().find(|p| p.name == "root") {
        // Pose-Werte kommen zur Grundlage dazu; Vanilla SETZT zRot und y.
        let rotation = [0.0, 0.0, std::f32::consts::PI - wurzel.rotation[2]];
        pose.set("root", PartPose { rotation, pivot_offset: [0.0, -wurzel.pivot[1], 0.0], ..PartPose::default() });
    }
    pose
}

// CopperGolemStatueBlockRenderer.modelTransformation: translation(0.5, 0, 0.5), dann um Y
// gedreht (StatueInstance::yaw_deg) -- ab der Blockecke.
#[must_use] 
pub fn copper_golem_statue_transform(block: [f32; 3], yaw_deg: f32) -> Affine {
    Affine::translation([block[0] + 0.5, block[1], block[2] + 0.5]).then(Affine::rotation_y_deg(yaw_deg))
}

// Platzhalter: die tatsaechliche Textur haengt an der Bettfarbe und wird erst beim Zeichnen
// gewaehlt (emit bekommt das Atlas-Rechteck uebergeben, s. block_entity::BedInstance).
pub(crate) const BED_MODEL_TEXTURE: &str = "entity/bed/red";

// BedBlockEntityRenderer.setTransforms(): die Einbettung EINER Betthaelfte, ausgehend von der
// Blockecke.
//
// `is_foot` ist in der WELT immer false -- BedBlockEntityRenderer.render() uebergibt dort fest
// `false`, weil jede Betthaelfte ihr eigenes BlockEntity und damit ihren eigenen Ursprung hat.
// Der -1-Versatz gilt nur fuer renderAsItem(), das beide Haelften aus EINEM Ursprung zeichnet.
// (Beim ersten Anlauf hatte ich hier `!head` uebergeben -- die Betten waeren einen Block zu
// weit noerdlich gelandet.)
//
// Vanilla:
//
//   translate(0, 0.5625, isFoot ? -1 : 0)
//   rotateX(90°)
//   translate(0.5, 0.5, 0.5)
//   rotateZ(180° + facing.getPositiveHorizontalDegrees())
//   translate(-0.5, -0.5, -0.5)
//
// Kein scale(-1,-1,1) wie bei Lebewesen: BlockEntity-Modelle sind nicht auf dem Kopf
// definiert, sie werden nur um die X-Achse flach gelegt. Die 0.5625 sind die 9/16 Matratzen-
// hoehe -- dieselbe Zahl wie in der Kollisionsform (s. vanilla_shapes::bed_boxes).
#[must_use] 
pub fn bed_transform(block: [f32; 3], facing_deg: f32, is_foot: bool) -> Affine {
    Affine::translation([block[0], block[1] + 0.5625, block[2] + if is_foot { -1.0 } else { 0.0 }])
        .then(rotation_x_deg(90.0))
        .then(Affine::translation([0.5, 0.5, 0.5]))
        .then(rotation_z_deg(180.0 + facing_deg))
        .then(Affine::translation([-0.5, -0.5, -0.5]))
}

#[cfg(test)]
mod shulker_box_tests {
    use super::*;

    fn nah(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    // Geschlossen fuellt die Kiste genau den Block: Deckel oben bei 16/16, Boden bei 0.
    #[test]
    fn geschlossen_nach_oben_fuellt_den_block() {
        let t = shulker_box_transform([0.0; 3], "up");
        // Unterseite des base-Quaders (Modell-y 24 px = 1.5) und Oberseite des Deckels (8 px =
        // 0.5); scale(1,-1,-1) spiegelt dabei auch Z.
        let im_block = |v: [f32; 3]| v.map(|c| 0.5 + (c - 0.5) * 0.9995);
        assert!(nah(t.apply([-0.5, 1.5, -0.5]), im_block([0.0, 0.0, 1.0])));
        assert!(nah(t.apply([0.5, 0.5, 0.5]), im_block([1.0, 1.0, 0.0])));
    }

    // Nach Norden: der Deckel (Modell-Oberseite) zeigt nach -Z.
    #[test]
    fn nach_norden_zeigt_der_deckel_nach_minus_z() {
        let t = shulker_box_transform([0.0; 3], "north");
        let deckel = t.apply([0.0, 0.5, 0.0]);
        let boden = t.apply([0.0, 1.5, 0.0]);
        assert!(deckel[2] < boden[2], "{deckel:?} vs {boden:?}");
        let t = shulker_box_transform([0.0; 3], "down");
        assert!(t.apply([0.0, 0.5, 0.0])[1] < t.apply([0.0, 1.5, 0.0])[1]);
    }
}

#[cfg(test)]
mod eat_tests {
    use super::*;

    fn nah(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    // Zu Beginn (volle Restzeit) steht das Item genau wie ohne Essen -- es springt also nicht,
    // wenn das Essen anfaengt. Auch nicht im ersten Bild, in dem die Restzeit ueber der Dauer liegt.
    #[test]
    fn essen_beginnt_in_der_ruhelage() {
        let ruhe = first_person_equip_offset(true, 0.0).apply([0.0, 0.0, 0.0]);
        for f in [32.0, 32.9] {
            assert!(nah(first_person_eat_pose(true, 0.0, f, 32.0).apply([0.0, 0.0, 0.0]), ruhe), "f = {f}");
        }
    }

    // Mitten im Essen liegt das Item zur Mitte hin (vor dem Mund), fuer beide Haende gespiegelt.
    #[test]
    fn essen_zieht_zum_mund() {
        let rechts = first_person_eat_pose(true, 0.0, 16.0, 32.0).apply([0.0, 0.0, 0.0]);
        let links = first_person_eat_pose(false, 0.0, 16.0, 32.0).apply([0.0, 0.0, 0.0]);
        let ruhe = first_person_equip_offset(true, 0.0).apply([0.0, 0.0, 0.0]);
        assert!(rechts[0] != ruhe[0]);
        assert!((rechts[0] + links[0]).abs() < 1e-4, "gespiegelt: {rechts:?} / {links:?}");
    }
}

#[cfg(test)]
mod bed_tests {
    use super::*;

    // Die Matratze muss in ihrer eigenen Zelle liegen und oben auf 9/16 abschliessen -- genau
    // der Hoehe, mit der sie auch kollidiert (s. world::vanilla_shapes::bed_boxes). Geprueft
    // fuer alle vier Ausrichtungen, weil die Drehung um die Zellenmitte laeuft und ein
    // Vorzeichenfehler dort das Bett in den Nachbarblock schoebe.
    #[test]
    fn bett_matratze_liegt_in_ihrer_zelle() {
        // main-Kasten aus dem Jar: cuboid(0,0,0, 16,16,6), hier in Bloecken.
        let ecken = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0, 6.0 / 16.0],
            [1.0, 0.0, 6.0 / 16.0],
            [0.0, 1.0, 6.0 / 16.0],
            [1.0, 1.0, 6.0 / 16.0],
        ];
        for facing_deg in [0.0, 90.0, 180.0, 270.0] {
            let base = bed_transform([10.0, 64.0, -3.0], facing_deg, false);
            let mut max_y = f32::MIN;
            for ecke in ecken {
                let p = base.apply(ecke);
                assert!((10.0 - 1e-4..=11.0 + 1e-4).contains(&p[0]), "X ausserhalb der Zelle bei {facing_deg}: {p:?}");
                assert!((-3.0 - 1e-4..=-2.0 + 1e-4).contains(&p[2]), "Z ausserhalb der Zelle bei {facing_deg}: {p:?}");
                max_y = max_y.max(p[1]);
            }
            assert!((max_y - (64.0 + 0.5625)).abs() < 1e-4, "Matratzenoberkante {max_y} statt 9/16 bei {facing_deg}");
        }
    }
}
