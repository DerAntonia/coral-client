// Spielermodell, Ruestung, Umhang und die Ich-Sicht: Arm, Item, Speer, Bogen, Essen, Armbrust.

use super::*;

// BipedEntityModel.getModelData(dilation, pivotOffsetY). `pivot_offset_y` verschiebt ALLE
// Pivots (der Enderman nutzt -14, weil sein Koerper viel hoeher sitzt).
pub(crate) fn biped_parts(dilation: f32, pivot_offset_y: f32) -> Vec<Part> {
    let head =
        Part::new("head", [0.0, pivot_offset_y, 0.0], vec![Cuboid::new([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]).dilated(dilation)])
            .with(Part::new(
                "hat",
                [0.0, 0.0, 0.0],
                vec![Cuboid::new([32.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]).dilated(dilation + 0.5)],
            ));
    let body = Part::new(
        "body",
        [0.0, pivot_offset_y, 0.0],
        vec![Cuboid::new([16.0, 16.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0]).dilated(dilation)],
    );
    let right_arm = Part::new(
        "right_arm",
        [-5.0, 2.0 + pivot_offset_y, 0.0],
        vec![Cuboid::new([40.0, 16.0], [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0]).dilated(dilation)],
    );
    let left_arm = Part::new(
        "left_arm",
        [5.0, 2.0 + pivot_offset_y, 0.0],
        vec![Cuboid::new([40.0, 16.0], [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0]).dilated(dilation).mirrored()],
    );
    let right_leg = Part::new(
        "right_leg",
        [-1.9, 12.0 + pivot_offset_y, 0.0],
        vec![Cuboid::new([0.0, 16.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]).dilated(dilation)],
    );
    let left_leg = Part::new(
        "left_leg",
        [1.9, 12.0 + pivot_offset_y, 0.0],
        vec![Cuboid::new([0.0, 16.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]).dilated(dilation).mirrored()],
    );
    vec![head, body, right_arm, left_arm, right_leg, left_leg]
}

// Das Modell einer RUESTUNGSEBENE -- dasselbe Biped, nur aufgeblaeht und auf die Teile
// beschraenkt, die dieses Ruestungsteil bedeckt.
//
// Vanilla haelt dafuer zwei fertige BipedEntityModel (EntityModelLayers.PLAYER_INNER_ARMOR
// mit Dilation(0.5), PLAYER_OUTER_ARMOR mit Dilation(1.0)) und blendet je Slot Teile aus:
//
//     ArmorFeatureRenderer.setVisible:
//         HEAD  -> head, hat
//         CHEST -> body, rightArm, leftArm
//         LEGS  -> body, rightLeg, leftLeg
//         FEET  -> rightLeg, leftLeg
//
// Dass die HOSE den Rumpf mitzeichnet, ist kein Versehen: das ist der Guertel. Und dass sie
// als einzige die INNERE (duennere) Ebene nimmt, ist genau der Grund, warum sie unter dem
// Brustpanzer verschwindet, statt mit ihm zu flimmern.
//
// `texture` ist hier nur Dokumentation: gezeichnet wird mit dem Atlas-Rechteck, das der
// Aufrufer an emit()/emit_tinted() uebergibt -- ein Modell je Slot reicht deshalb fuer alle
// acht Materialien, statt 32 Modelle vorzuhalten.
#[must_use] 
pub fn armor_model(slot: crate::armor::ArmorSlot) -> Model {
    use crate::armor::ArmorSlot;
    let teile: &[&str] = match slot {
        // "hat" ist bei uns ein KIND von "head" (bei Vanilla ein Geschwisterteil, dessen
        // Transformation kopiert wird -- dasselbe Ergebnis), kommt also automatisch mit.
        ArmorSlot::Head => &["head"],
        ArmorSlot::Chest => &["body", "right_arm", "left_arm"],
        ArmorSlot::Legs => &["body", "right_leg", "left_leg"],
        ArmorSlot::Feet => &["right_leg", "left_leg"],
    };
    let dilation = if slot.inner() { 0.5 } else { 1.0 };
    Model {
        texture: "entity/equipment/humanoid/iron",
        tex_size: [64.0, 32.0],
        parts: biped_parts(dilation, 0.0).into_iter().filter(|p| teile.contains(&p.name)).collect(),
    }
}

// Ruestungsebenen eines Armorstands (ArmorStandArmorModel): dasselbe Biped wie armor_model,
// nur sitzt der Kopf einen Pixel tiefer und die Beine setzen einen Pixel hoeher an, mit um 0.1
// duenneren Kaesten -- passend zu den duennen Staeben des Standes. Einmal gebaut, je Slot.
//
// `small`: die Ebenen eines kleinen Standes, mit derselben Baby-Umformung wie sein Koerper
// (armor_stand_small#main): der Kopf auf 3/4 und 16 Pixel tiefer angesetzt, alles andere halb
// so gross und um 12 Pixel nach unten geschoben.
pub fn armor_stand_armor_models(small: bool) -> &'static [Model; 4] {
    static MODELS: std::sync::OnceLock<[[Model; 4]; 2]> = std::sync::OnceLock::new();
    &MODELS.get_or_init(|| [false, true].map(armor_stand_armor_set))[usize::from(small)]
}

pub(crate) fn armor_stand_armor_set(small: bool) -> [Model; 4] {
    std::array::from_fn(|i| {
        let slot = crate::armor::ArmorSlot::ALL.iter().copied().find(|s| s.index() == i).expect("Slot");
        let mut model = armor_model(slot);
        for part in &mut model.parts {
            match part.name {
                "head" => part.pivot[1] = 1.0,
                "right_leg" | "left_leg" => {
                    part.pivot[1] = 11.0;
                    for cuboid in &mut part.cuboids {
                        cuboid.dilation = cuboid.dilation.map(|d| d - 0.1);
                    }
                }
                _ => {}
            }
            if small {
                let (faktor, versatz) = if part.name == "head" { (0.75, 16.0 * 0.75) } else { (0.5, 12.0) };
                part.pivot = [part.pivot[0] * faktor, part.pivot[1] * faktor + versatz, part.pivot[2] * faktor];
                part.scale = part.scale.map(|s| s * faktor);
            }
        }
        model
    })
}

// DefaultSkinHelper.SKINS -- Reihenfolge ist BEDEUTUNGSTRAGEND (der Index kommt aus dem
// UUID-Hash, s. default_skin), also genau die von Vanilla: erst alle neun Slim-Skins
// alphabetisch, dann dieselben neun als Classic.
pub const DEFAULT_SKINS: [(&str, bool); 18] = [
    ("entity/player/slim/alex", true),
    ("entity/player/slim/ari", true),
    ("entity/player/slim/efe", true),
    ("entity/player/slim/kai", true),
    ("entity/player/slim/makena", true),
    ("entity/player/slim/noor", true),
    ("entity/player/slim/steve", true),
    ("entity/player/slim/sunny", true),
    ("entity/player/slim/zuri", true),
    ("entity/player/wide/alex", false),
    ("entity/player/wide/ari", false),
    ("entity/player/wide/efe", false),
    ("entity/player/wide/kai", false),
    ("entity/player/wide/makena", false),
    ("entity/player/wide/noor", false),
    ("entity/player/wide/steve", false),
    ("entity/player/wide/sunny", false),
    ("entity/player/wide/zuri", false),
];

// Der Standard-Skin eines Spielers, wenn der Server keine Textur-Property schickt (Offline-
// Mode) oder das Bild noch nicht geladen ist: DefaultSkinHelper.getSkinTextures(uuid) waehlt
// SKINS[floorMod(uuid.hashCode(), 18)]. `uuid.hashCode()` ist java.util.UUID's eigene
// Definition -- die obere und die untere 64-Bit-Haelfte werden ver-XOR-t, dann die beiden
// 32-Bit-Haelften des Ergebnisses.
//
// Das Nachrechnen (statt einfach immer Steve zu nehmen) ist noetig, weil sonst auf einem
// Offline-Mode-Server ALLE Spieler gleich aussehen, waehrend Vanilla sie nach UUID auf 18
// Gesichter verteilt -- und die Haelfte davon hat schmale Arme.
#[must_use] 
pub const fn default_skin(uuid: &[u8; 16]) -> (&'static str, bool) {
    let most = u64::from_be_bytes([uuid[0], uuid[1], uuid[2], uuid[3], uuid[4], uuid[5], uuid[6], uuid[7]]);
    let least = u64::from_be_bytes([uuid[8], uuid[9], uuid[10], uuid[11], uuid[12], uuid[13], uuid[14], uuid[15]]);
    let hilo = most ^ least;
    let hash = ((hilo >> 32) as i32) ^ (hilo as i32);
    // Javas Math.floorMod: das Ergebnis hat das Vorzeichen des Divisors, ist hier also nie
    // negativ -- Rusts `%` liefert dagegen den Rest mit dem Vorzeichen des Dividenden.
    let index = hash.rem_euclid(DEFAULT_SKINS.len() as i32) as usize;
    DEFAULT_SKINS[index]
}

// PlayerEntityModel.getTexturedModelData(dilation, slim): Biped plus die vier Aussenlagen,
// und im Slim-Fall schmalere Arme mit eigenen UVs. Die Textur ist der Skin (64x64), sie
// wird vom Aufrufer gesetzt -- hier steht nur der Standard-Skinpfad.
#[must_use] 
pub fn player_model(slim: bool) -> Model {
    let mut parts = biped_parts(0.0, 0.0);
    let arm_width = if slim { 3.0 } else { 4.0 };
    // Rechter Arm: Slim ersetzt den Kasten, Classic behaelt ihn (wie in Vanillas Zweigen).
    if let Some(right) = parts.iter_mut().find(|p| p.name == "right_arm") {
        if slim {
            right.cuboids = vec![Cuboid::new([40.0, 16.0], [-2.0, -2.0, -2.0], [arm_width, 12.0, 4.0])];
        }
        let sleeve_offset = if slim { -2.0 } else { -3.0 };
        right.children.push(Part::new(
            "right_sleeve",
            [0.0, 0.0, 0.0],
            vec![Cuboid::new([40.0, 32.0], [sleeve_offset, -2.0, -2.0], [arm_width, 12.0, 4.0]).dilated(0.25)],
        ));
    }
    // Linker Arm wird in beiden Faellen NEU angelegt (uv 32,48) -- der aus biped_parts
    // traegt die gespiegelte rechte Textur und ist beim Spieler falsch.
    if let Some(left) = parts.iter_mut().find(|p| p.name == "left_arm") {
        left.cuboids = vec![Cuboid::new([32.0, 48.0], [-1.0, -2.0, -2.0], [arm_width, 12.0, 4.0])];
        left.children = vec![Part::new(
            "left_sleeve",
            [0.0, 0.0, 0.0],
            vec![Cuboid::new([48.0, 48.0], [-1.0, -2.0, -2.0], [arm_width, 12.0, 4.0]).dilated(0.25)],
        )];
    }
    if let Some(left) = parts.iter_mut().find(|p| p.name == "left_leg") {
        left.cuboids = vec![Cuboid::new([16.0, 48.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0])];
        left.children = vec![Part::new(
            "left_pants",
            [0.0, 0.0, 0.0],
            vec![Cuboid::new([0.0, 48.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]).dilated(0.25)],
        )];
    }
    if let Some(right) = parts.iter_mut().find(|p| p.name == "right_leg") {
        right.children.push(Part::new(
            "right_pants",
            [0.0, 0.0, 0.0],
            vec![Cuboid::new([0.0, 32.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]).dilated(0.25)],
        ));
    }
    if let Some(body) = parts.iter_mut().find(|p| p.name == "body") {
        body.children.push(Part::new(
            "jacket",
            [0.0, 0.0, 0.0],
            vec![Cuboid::new([16.0, 32.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0]).dilated(0.25)],
        ));
    }
    Model { texture: if slim { "entity/player/slim/alex" } else { "entity/player/wide/steve" }, tex_size: [64.0, 64.0], parts }
}

// PlayerCapeModel.getTexturedModelData. Der Umhang haengt am Rumpf, wird aber mit einer
// EIGENEN Textur gezeichnet (dem Cape-Bild, nicht dem Skin) -- deshalb ist er hier ein
// eigenes Modell und kein weiteres Kind von player_model. Vanilla macht es genauso: die
// Klasse erbt vom Spielermodell und leert per resetChildrenParts alle Kaesten, sodass nur
// der Umhang uebrig bleibt.
//
// Der Rumpf ist hier deshalb mit dabei, aber leer: er traegt keine Geometrie und dient nur
// als Gelenk, damit der Umhang die Rumpfhaltung mitmacht (Schleichen kippt den Rumpf, und
// der Umhang muss mitkippen). Er wird mit DERSELBEN Pose gefuettert wie das Spielermodell.
//
// tex_size ist 64x32, nicht 64x64: Vanilla baut den Kasten mit textureScale (1.0, 0.5) auf
// eine 64x64-Vorlage, was auf genau dieselben UVs hinauslaeuft (ModelCuboidData.createCuboid
// multipliziert die Texturgroesse mit der Skalierung). Cape-Bilder sind 64x32.
#[must_use] 
pub fn cape_model() -> Model {
    // ModelTransform.of(0, 0, 2, 0, PI, 0) -- der Pivot sitzt zwei Pixel HINTER dem Rumpf,
    // die Grunddrehung um PI dreht den Kasten nach hinten. Die Grunddrehung steht bewusst
    // nicht hier, sondern in cape_pose: dort wird sie (wie in Vanilla) Teil des
    // Quaternionen-Produkts und von dessen erster Drehung wieder aufgehoben.
    let cape = Part::new("cape", [0.0, 0.0, 2.0], vec![Cuboid::new([0.0, 0.0], [-5.0, 0.0, -1.0], [10.0, 16.0, 1.0])]);
    Model { texture: "", tex_size: [64.0, 32.0], parts: vec![Part::new("body", [0.0, 0.0, 0.0], Vec::new()).with(cape)] }
}

// PlayerCapeModel.setAngles: die Umhangdrehung ist ein Quaternionen-Produkt, das an die
// Grunddrehung des Teils MULTIPLIZIERT wird (ModelPart.rotate: M = rotationZYX(...) dann
// M.rotate(q)):
//
//   R = Ry(180 Grad) * [ Ry(-180 Grad) * Rx(6 + swing/2 + vertical) * Rz(side/2) * Ry(180 - side/2) ]
//       ^ Grunddrehung   ^ hebt sie wieder auf
//
// Die beiden Y-Drehungen kuerzen sich weg -- das ist kein Zufall, sondern der Grund, warum
// die erste Drehung im Quaternion -180 Grad ist. Trotzdem stehen sie hier beide drin: so
// laesst sich die Zeile Stueck fuer Stueck gegen Vanilla pruefen, und ein Test haelt
// fest, dass sie sich wirklich aufheben (s. cape_tests).
//
// Bedeutung der drei Groessen (PlayerEntityRenderer.updateCape):
//   vertical -- Hoehendifferenz zwischen nachhinkender Umhangposition und Spieler, *10,
//               geklemmt auf -6..32, PLUS das Schwingen beim Laufen
//   swing    -- wie weit der Umhang nach hinten weht, 0..150
//   side     -- Seitwaertsversatz, -20..20
#[must_use] 
pub fn cape_pose(same_pose: &Pose, vertical: f32, swing: f32, side: f32) -> Pose {
    let mut pose = same_pose.clone();
    let rotation = Affine::rotation_y_deg(180.0)
        .then(Affine::rotation_y_deg(-180.0))
        .then(rotation_x_deg(6.0 + swing / 2.0 + vertical))
        .then(rotation_z_deg(side / 2.0))
        .then(Affine::rotation_y_deg(180.0 - side / 2.0));
    pose.set("cape", PartPose { rotation: [0.0; 3], pivot_offset: [0.0; 3], rotation_matrix: Some(rotation), hidden: false });
    pose
}

// Ein Modell aus genau EINEM Arm (samt Aermel) -- fuer die Ego-Hand. Vanilla macht dasselbe
// ueber PlayerEntityRenderer.renderArm: dort wird das ganze Spielermodell unsichtbar geschaltet
// und nur der eine Arm gezeichnet.
#[must_use] 
pub fn player_arm_model(slim: bool, right: bool) -> Model {
    let full = player_model(slim);
    let name = if right { "right_arm" } else { "left_arm" };
    let part = full.parts.into_iter().find(|p| p.name == name).expect("Arm im Spielermodell");
    Model { texture: full.texture, tex_size: full.tex_size, parts: vec![part] }
}

// Die Haltung des Arms in der Ego-Ansicht: PlayerEntityRenderer.renderArm setzt den Arm auf
// seine Grundstellung zurueck und gibt ihm nur ein leichtes Rollen nach aussen
// (rightArm.roll = 0.1, leftArm.roll = -0.1).
#[must_use] 
pub fn first_person_arm_pose(right: bool) -> Pose {
    let mut pose = Pose::new();
    pose.rotate(if right { "right_arm" } else { "left_arm" }, [0.0, 0.0, if right { 0.1 } else { -0.1 }]);
    pose
}

// HeldItemRenderer.renderArmHoldingItem, 1:1 -- die Kette, die den Arm in der Ego-Ansicht
// platziert. Sie steht in VANILLAS Reihenfolge und wird bewusst nicht vereinfacht: die Zahlen
// (3.6, 3.5, 5.6, 200 Grad ...) ergeben einzeln keinen Sinn, erst ihre Verkettung setzt den
// Arm an den Bildrand. Wer hier "aufraeumt", bekommt einen Arm irgendwo im Nichts.
//
// `swing_progress` ist 0..1 waehrend eines Schlags, `equip_progress` 0..1 waehrend des
// Item-Wechsels (1 = ganz nach unten weggetaucht).
//
// Der Ausgangsraum ist der SICHTraum: Kamera im Ursprung, Blick entlang -Z, +X rechts,
// +Y oben (Vanilla: renderHand startet den Stack mit der invertierten Sichtmatrix).
#[must_use] 
pub fn first_person_arm_transform(right: bool, equip_progress: f32, swing_progress: f32) -> Affine {
    let f = if right { 1.0 } else { -1.0 };
    let pi = std::f32::consts::PI;
    let g = swing_progress.max(0.0).sqrt();
    let h = -0.3 * (g * pi).sin();
    let i = 0.4 * (g * pi * 2.0).sin();
    let j = -0.4 * (swing_progress * pi).sin();
    let k = (swing_progress * swing_progress * pi).sin();
    let l = (g * pi).sin();

    Affine::translation([f * (h + 0.640_000_05), i - 0.6 + equip_progress * -0.6, j - 0.719_999_97])
        .then(Affine::rotation_y_deg(f * 45.0))
        .then(Affine::rotation_y_deg(f * l * 70.0))
        .then(rotation_z_deg(f * k * -20.0))
        .then(Affine::translation([-f, 3.6, 3.5]))
        .then(rotation_z_deg(f * 120.0))
        .then(rotation_x_deg(200.0))
        .then(Affine::rotation_y_deg(f * -135.0))
        .then(Affine::translation([f * 5.6, 0.0, 0.0]))
}

// Abweichung von Vanilla: das Item schwingt nur so weit aus (Verschiebung und Drehung). Bei
// 1.0 kippt es wie in Vanilla weit nach vorn.
pub const FIRST_PERSON_ITEM_SWING_SCALE: f32 = 0.6;

// Wo das ITEM in der Ego-Ansicht sitzt -- der Arm oben ist eine eigene Kette, das Item
// haengt NICHT an ihm. Vanilla setzt es unabhaengig in denselben Sichtraum
// (HeldItemRenderer.renderFirstPersonItem, Zweig ohne Benutzung):
//     applyEquipOffset(matrices, arm, equipProgress);
//     swingArm(swingProgress, matrices, i, arm);
// danach folgt die firstperson-Halte-Transformation des Item-Modells.
//
// applyEquipOffset:  translate(i * 0.56, -0.52 + equip * -0.6, -0.72)
// swingArm:          translate(i * f, g, h) mit
//                      f = -0.4 * sin(sqrt(swing) * PI)
//                      g =  0.2 * sin(sqrt(swing) * 2PI)
//                      h = -0.2 * sin(swing * PI)
//                    danach applySwingOffset (vier Drehungen, s. unten).
#[must_use] 
pub fn first_person_item_transform(right: bool, equip_progress: f32, swing_progress: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    let pi = std::f32::consts::PI;
    let wurzel = swing_progress.max(0.0).sqrt();
    let k = FIRST_PERSON_ITEM_SWING_SCALE;
    let f = k * -0.4 * (wurzel * pi).sin();
    let g = k * 0.2 * (wurzel * pi * 2.0).sin();
    let h = k * -0.2 * (swing_progress * pi).sin();
    // applySwingOffset
    let dreh_a = k * (swing_progress * swing_progress * pi).sin();
    let dreh_b = k * (wurzel * pi).sin();

    Affine::translation([i * 0.56, -0.52 + equip_progress * -0.6, -0.72])
        .then(Affine::translation([i * f, g, h]))
        .then(Affine::rotation_y_deg(i * (45.0 + dreh_a * -20.0)))
        .then(rotation_z_deg(i * dreh_b * -20.0))
        .then(rotation_x_deg(dreh_b * -80.0))
        .then(Affine::rotation_y_deg(i * -45.0))
}

// Vanillas net.minecraft.util.math.Easing, soweit der Speer sie braucht (nach dem Jar).
pub mod easing {
    // Lancing.method_75390: clamp(getLerpProgress(f, von, bis), 0, 1).
    #[must_use] 
    pub fn progress(f: f32, von: f32, bis: f32) -> f32 {
        if bis == von {
            return if f >= bis { 1.0 } else { 0.0 };
        }
        ((f - von) / (bis - von)).clamp(0.0, 1.0)
    }
    #[must_use] 
    pub fn in_quad(t: f32) -> f32 {
        t * t
    }
    #[must_use] 
    pub fn in_out_sine(t: f32) -> f32 {
        -((std::f32::consts::PI * t).cos() - 1.0) / 2.0
    }
    #[must_use] 
    pub fn out_back(t: f32) -> f32 {
        1.0 + 2.70158 * (t - 1.0).powi(3) + 1.70158 * (t - 1.0).powi(2)
    }
    #[must_use] 
    pub fn out_quart(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(4)
    }
    #[must_use] 
    pub fn out_cubic(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(3)
    }
    #[must_use] 
    pub fn in_out_expo(t: f32) -> f32 {
        if t < 0.5 {
            if t == 0.0 {
                0.0
            } else {
                (2f64.powf(20.0 * f64::from(t) - 10.0) / 2.0) as f32
            }
        } else if t == 1.0 {
            1.0
        } else {
            ((2.0 - 2f64.powf(-20.0 * f64::from(t) + 10.0)) / 2.0) as f32
        }
    }
    #[must_use] 
    pub fn out_circ(t: f32) -> f32 {
        (1.0 - (t - 1.0) * (t - 1.0)).max(0.0).sqrt()
    }
    #[must_use] 
    pub fn in_circ(t: f32) -> f32 {
        1.0 - (1.0 - t * t).max(0.0).sqrt()
    }
    #[must_use] 
    pub fn in_out_elastic(t: f32) -> f32 {
        if t == 0.0 {
            return 0.0;
        }
        if t == 1.0 {
            return 1.0;
        }
        let t = f64::from(t);
        let d = ((20.0 * t - 11.125) * f64::from(std::f32::consts::PI) * 4.0 / 9.0).sin();
        if t < 0.5 {
            (-(2f64.powf(20.0 * t - 10.0) * d) / 2.0) as f32
        } else {
            (2f64.powf(-20.0 * t + 10.0) * d / 2.0 + 1.0) as f32
        }
    }
    #[must_use] 
    pub fn in_out_back(t: f32) -> f32 {
        if t < 0.5 {
            return 4.0 * t * t * (7.189819 * t - 2.5949094) / 2.0;
        }
        let h = 2.0 * t - 2.0;
        f32::midpoint(h * h * (3.5949094 * h + 2.5949094), 2.0)
    }
}

// Drehung um einen Punkt: Vanillas MatrixStack.multiply(quaternion, ox, oy, oz).
pub(crate) fn um_punkt(drehung: Affine, o: [f32; 3]) -> Affine {
    Affine::translation(o).then(drehung).then(Affine::translation([-o[0], -o[1], -o[2]]))
}

// Speerstich in der Ego-Ansicht (HeldItemRenderer, SwingAnimationType.STAB ->
// Lancing.method_75391) statt des normalen Schwungs:
//   g = inOutSine(f in 0..0.05), h = outBack(f in 0.05..0.2), j = inOutExpo(f in 0.4..1)
//   translate(i*0.1*(g-h), -0.075*(g-j), 0.65*(g-h)); rotateX(-70*(g-j)); translate(0,0,-0.25*(j-h))
#[must_use] 
pub fn first_person_stab_transform(right: bool, equip_progress: f32, swing_progress: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    let f = swing_progress;
    let g = easing::in_out_sine(easing::progress(f, 0.0, 0.05));
    let h = easing::out_back(easing::progress(f, 0.05, 0.2));
    let j = easing::in_out_expo(easing::progress(f, 0.4, 1.0));
    first_person_equip_offset(right, equip_progress)
        .then(Affine::translation([i * 0.1 * (g - h), -0.075 * (g - j), 0.65 * (g - h)]))
        .then(rotation_x_deg(-70.0 * (g - j)))
        .then(Affine::translation([0.0, 0.0, -0.25 * (j - h)]))
}

// Die Zeitmarken des Ansturms (KineticWeaponComponent, s. item_registry::spear_kinetic):
// Ausholen bis `delay`, danach die drei Bedingungen, die nacheinander auslaufen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpearKinetic {
    pub delay: i32,
    pub dismount: i32,
    pub knockback: i32,
    pub damage: i32,
}

// Lancing.class_12153.method_75397: alle Teilfortschritte des Ansturms aus der Benutzungsdauer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LanceState {
    pub(crate) raise: f32,
    pub(crate) raise_start: f32,
    pub(crate) raise_middle: f32,
    pub(crate) raise_end: f32,
    pub(crate) sway: f32,
    pub(crate) lower: f32,
    pub(crate) raise_back: f32,
    pub(crate) sway_scale_slow: f32,
    pub(crate) sway_scale_fast: f32,
}

pub(crate) fn lance_state(k: SpearKinetic, f: f32) -> LanceState {
    use easing::{progress, out_cubic, in_out_elastic, out_circ, in_circ};
    let i = k.delay;
    let j = k.dismount + i;
    let kk = j - 20;
    let l = k.knockback + i;
    let m = l - 40;
    let n = k.damage + i;
    let g = progress(f, 0.0, i as f32);
    let q = progress(f, kk as f32, m as f32);
    let r = out_cubic(in_out_elastic(progress(f - 20.0, m as f32, l as f32)));
    let s = progress(f, (n - 5) as f32, n as f32);
    let t = 2.0 * out_circ(q) - 2.0 * in_circ(s);
    LanceState {
        raise: g,
        raise_start: progress(g, 0.0, 0.5),
        raise_middle: progress(g, 0.5, 0.8),
        raise_end: progress(g, 0.8, 1.0),
        sway: q,
        lower: r,
        raise_back: s,
        sway_scale_slow: (f * 19.0f32.to_radians()).sin() * t,
        sway_scale_fast: (f * 30.0f32.to_radians()).sin() * t,
    }
}

// Speer-Ansturm in der Ego-Ansicht (UseAction.SPEAR -> Lancing.method_75396). `f` ist die
// Benutzungsdauer in Ticks samt Zwischenbild. Der Rueckstoss nach einem Ansturm-Treffer
// (method_75916 ueber timeSinceLastKineticAttack) fehlt: den Treffer meldet uns niemand.
#[must_use] 
pub fn first_person_spear_charge(right: bool, f: f32, k: SpearKinetic) -> Affine {
    let lv = lance_state(k, f);
    let i = if right { 1.0 } else { -1.0 };
    let x = i * (lv.raise * 0.15 + lv.raise_end * -0.05 + lv.sway * -0.1 + lv.sway_scale_slow * 0.005);
    let y = lv.raise * -0.075 + lv.raise_middle * 0.075 + lv.sway_scale_fast * 0.01;
    let z = lv.raise_start * 0.05 + lv.raise_end * -0.05 + lv.sway_scale_slow * 0.005;
    let kipp = -65.0 * easing::in_out_back(lv.raise) - 35.0 * lv.lower + 100.0 * lv.raise_back + -0.5 * lv.sway_scale_fast;
    let dreh = i * (-90.0 * easing::progress(lv.raise, 0.5, 0.55) + 90.0 * lv.sway + 2.0 * lv.sway_scale_slow);
    Affine::translation([i * 0.56, -0.52, -0.72])
        .then(Affine::translation([x, y, z]))
        .then(um_punkt(rotation_x_deg(kipp), [0.0, 0.1, 0.0]))
        // NEGATIVE_Y.rotationDegrees(a) = Drehung um +Y mit -a.
        .then(um_punkt(Affine::rotation_y_deg(-dreh), [i * 0.15, 0.0, 0.0]))
}

// Der Speer in der Hand einer Figur beim Stich (HeldItemFeatureRenderer ->
// Lancing.method_75395), NACH der ueblichen Hand-Vortransformation:
//   rotate(NEGATIVE_X, 70*(i-j)) um (0, -0.125, 0.125); translate(0, 0.38*(i-j), 0)
// mit i = inQuad(f in 0.05..0.2), j = inOutExpo(f in 0.4..1); 0.38 = forwardMovement.
#[must_use] 
pub fn third_person_stab_item_offset(swing: f32) -> Affine {
    if swing <= 0.0 {
        return Affine::IDENTITY;
    }
    let i = easing::in_quad(easing::progress(swing, 0.05, 0.2));
    let j = easing::in_out_expo(easing::progress(swing, 0.4, 1.0));
    um_punkt(rotation_x_deg(-70.0 * (i - j)), [0.0, -0.125, 0.125]).then(Affine::translation([0.0, 0.38 * (i - j), 0.0]))
}

// --- Haltungen waehrend einer Benutzung (HeldItemRenderer.renderFirstPersonItem) ----------
//
// Wer spannt, haelt anders: Vanilla schiebt Bogen und ladende Armbrust quer vor die Kamera
// und zieht sie mit dem Spannfortschritt heran. Diese Ketten ersetzen den Schwung-Teil von
// first_person_item_transform -- die Anlege-Verschiebung (applyEquipOffset) bleibt, der
// Schwung entfaellt (wer spannt, schlaegt nicht).
//
// `f` ist Vanillas Benutzungsdauer in Ticks (samt Zwischenbild), `pull` der Fortschritt
// 0..1. Beide kommen vom Aufrufer, weil die Umrechnung je Item anders ist (Bogen: f/20 und
// dann (g*g+2g)/3; Armbrust: f geteilt durch ihre Ladezeit).
// Vanillas leichtes Auf-und-ab der gespannten Sehne (`sin((f - 0.1) * 1.3) * (pull - 0.1)`
// auf der Y-Achse) ist hier BEWUSST NICHT nachgebaut -- weder beim Bogen noch bei der
// Armbrust (Nutzerwunsch: "mach dieses bogen-zittern raus", danach "die soll bei der
// armbrust auch weg"). Deshalb tragen beide Spann-Haltungen ihre Benutzungsdauer nur noch
// als ungenutzten Parameter: sie waere das Einzige, wofuer sie gebraucht wuerde.

pub(crate) fn pull_zoom(right: bool, pull: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    Affine::translation([0.0, 0.0, pull * 0.04]).then(Affine::scale([1.0, 1.0, 1.0 + pull * 0.2])).then(Affine::rotation_y_deg(-i * 45.0))
}

// Anlege-Verschiebung allein (applyEquipOffset), ohne den Schwung.
#[must_use] 
pub fn first_person_equip_offset(right: bool, equip_progress: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    Affine::translation([i * 0.56, -0.52 + equip_progress * -0.6, -0.72])
}

// UseAction.BOW.
#[must_use] 
pub fn first_person_bow_pose(right: bool, equip_progress: f32, _f: f32, pull: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    first_person_equip_offset(right, equip_progress)
        .then(Affine::translation([i * -0.2785682, 0.18344387, 0.15731531]))
        .then(rotation_x_deg(-13.935))
        .then(Affine::rotation_y_deg(i * 35.3))
        .then(rotation_z_deg(i * -9.785))
        .then(pull_zoom(right, pull))
}

// UseAction.EAT / DRINK: das Item wandert zum Mund, wippt dort und kippt kurz vor dem Ende
// schlagartig zurueck (die 27. Potenz). `f` ist Vanillas VERBLEIBENDE Benutzungszeit in
// Ticks samt Zwischenbild, `max_use` die ganze Dauer. Auf die Essbewegung folgt nur noch die
// Anlege-Verschiebung -- kein Schwung (UseAction EAT/DRINK sind "ohne Versatz").
// Wie weit das Item beim Essen/Trinken vor dem Mund ist (0 = noch in der Haltung, 1 = ganz),
// f = verbleibende Zeit wie in first_person_eat_pose.
#[must_use] 
pub fn eat_lift(f: f32, max_use: f32) -> f32 {
    // Oben gekappt: im allerersten Bild liegt f noch einen Tick ueber der Dauer, und die
    // 27. Potenz wuerde daraus einen Sprung machen.
    let g = (f / max_use.max(1.0)).clamp(0.0, 1.0);
    1.0 - g.powi(27)
}

#[must_use] 
pub fn first_person_eat_pose(right: bool, equip_progress: f32, f: f32, max_use: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    let g = (f / max_use.max(1.0)).clamp(0.0, 1.0);
    let wippen = if g < 0.8 { ((f / 4.0 * std::f32::consts::PI).cos() * 0.1).abs() } else { 0.0 };
    let h = eat_lift(f, max_use);
    Affine::translation([0.0, wippen, 0.0])
        .then(Affine::translation([h * 0.6 * i, h * -0.5, 0.0]))
        .then(Affine::rotation_y_deg(i * h * 90.0))
        .then(rotation_x_deg(h * 10.0))
        .then(rotation_z_deg(i * h * 30.0))
        .then(first_person_equip_offset(right, equip_progress))
}

// Die LADENDE Armbrust (eigener Zweig in Vanilla, andere Zahlen als der Bogen).
#[must_use] 
pub fn first_person_crossbow_pull_pose(right: bool, equip_progress: f32, _f: f32, pull: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    first_person_equip_offset(right, equip_progress)
        .then(Affine::translation([i * -0.4785682, -0.094387, 0.05731531]))
        .then(rotation_x_deg(-11.935))
        .then(Affine::rotation_y_deg(i * 65.3))
        .then(rotation_z_deg(i * -9.785))
        .then(pull_zoom(right, pull))
}

// Die GELADENE Armbrust in der Haupthand, solange nicht geschlagen wird: Vanilla schiebt sie
// nach aussen und dreht sie ein wenig auf, damit der Pfeil auf dem Lauf zu sehen ist.
#[must_use] 
pub fn first_person_crossbow_charged_offset(right: bool) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    Affine::translation([i * -0.641864, 0.0, 0.0]).then(Affine::rotation_y_deg(i * 10.0))
}
