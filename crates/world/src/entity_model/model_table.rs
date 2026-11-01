// Modellarten, Endkristall, Zauberbuch, Windkugel, Pfeil, Modelltabelle, Katzen und Schildkroeten.

use super::*;

// Welche Grundform ein Typ hat -- bestimmt, welche Pose gerechnet wird. Reine
// Render-Einteilung, nichts Serverseitiges.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelKind {
    Biped,
    Quadruped,
    Creeper,
    Spider,
    Slime,
    Enderman,
    // Katze und Ozelot (FelineModel).
    Feline,
    Turtle,
}

// Katzen-Varianten (CatVariants): Registry-Name = Texturname unter entity/cat/. Welche eine
// Katze hat, schickt der Server als Registry-ID (s. Entity::cat_variant), die Namen dazu in
// der Registry-Reihenfolge (NamedRegistry "minecraft:cat_variant").
pub const CAT_TEXTURES: [&str; 11] = [
    "entity/cat/tabby",
    "entity/cat/black",
    "entity/cat/red",
    "entity/cat/siamese",
    "entity/cat/british_shorthair",
    "entity/cat/calico",
    "entity/cat/persian",
    "entity/cat/ragdoll",
    "entity/cat/white",
    "entity/cat/jellie",
    "entity/cat/all_black",
];

// Textur zu einem Registry-Namen wie "minecraft:calico" -- None fuer Varianten aus einem
// Datapack, deren Bild dieser Client nicht kennt.
#[must_use] 
pub fn cat_texture_for(registry_name: &str) -> Option<&'static str> {
    let name = registry_name.strip_prefix("minecraft:").unwrap_or(registry_name);
    CAT_TEXTURES.iter().copied().find(|t| t.strip_prefix("entity/cat/") == Some(name))
}

// Zusaetzliche Modellskalierung je Typ (MeshTransformer.scaling): die Katze ist ein auf 0.8
// verkleinerter Ozelot. Skaliert wird um die Fuesse, genau wie der Spielerfaktor.
#[must_use] 
pub fn model_scale(name: &str) -> f32 {
    match name {
        "cat" => 0.8,
        _ => 1.0,
    }
}

// Alle Typen, fuer die es hier ein echtes Modell gibt. Der Rest bleibt bei der Box (s.
// app::build_entity_vertices) -- Vanilla hat rund 100 Entity-Modelle, jedes eine eigene
// Klasse; diese Liste deckt die ab, die man in einem PvP-Kontext tatsaechlich sieht.
//
// Zweiter Eintrag ist der Texturpfad, dritter die Grundform. Skelett-Varianten, Zombie-
// Varianten usw. teilen sich das Biped-Modell und unterscheiden sich nur in der Textur --
// genau wie in Vanilla, wo alle von BipedEntityModel erben.
// EndCrystalEntityModel.getTexturedModelData: zwei Glaswuerfel und der Kern, ineinander
// geschachtelt und je kleiner skaliert, dazu die Bodenplatte. KEIN Lebewesen-Modell: es
// laeuft nicht durch entity_transform (keine Spiegelung, Y zeigt hier nach OBEN), sondern
// durch EndCrystalEntityRenderer (s. end_crystal_transform).
pub const END_CRYSTAL_TEXTURE: &str = "entity/end_crystal/end_crystal";

#[must_use] 
pub fn end_crystal_model(show_bottom: bool) -> Model {
    let glas = Cuboid::new([0.0, 0.0], [-4.0, -4.0, -4.0], [8.0, 8.0, 8.0]);
    let kern = Part::new("cube", [0.0; 3], vec![Cuboid::new([32.0, 0.0], [-4.0, -4.0, -4.0], [8.0, 8.0, 8.0])]).scaled(0.765625);
    let innen = Part::new("inner_glass", [0.0; 3], vec![glas]).scaled(0.875).with(kern);
    let aussen = Part::new("outer_glass", [0.0, 24.0, 0.0], vec![glas]).with(innen);
    let mut parts = vec![aussen];
    if show_bottom {
        parts.push(Part::new("base", [0.0; 3], vec![Cuboid::new([0.0, 16.0], [-6.0, 0.0, -6.0], [12.0, 4.0, 12.0])]));
    }
    Model { texture: END_CRYSTAL_TEXTURE, tex_size: [64.0, 32.0], parts }
}

// Das Buch des Zaubertischs (BookModel): zwei Deckel, Ruecken, zwei Seitenbloecke und zwei
// Blaetter, die beim Blaettern umschlagen. Gezeichnet wird es bisher nur im
// Zaubertisch-Bildschirm (s. app::build_enchant_book).
pub const ENCHANTING_BOOK_TEXTURE: &str = "entity/enchanting_table_book";

#[must_use] 
pub fn enchanting_book_model() -> Model {
    let blatt = || Cuboid::new([24.0, 10.0], [0.0, -4.0, 0.0], [5.0, 8.0, 0.005]);
    let parts = vec![
        Part::new("left_lid", [0.0, 0.0, -1.0], vec![Cuboid::new([0.0, 0.0], [-6.0, -5.0, -0.005], [6.0, 10.0, 0.005])]),
        Part::new("right_lid", [0.0, 0.0, 1.0], vec![Cuboid::new([16.0, 0.0], [0.0, -5.0, -0.005], [6.0, 10.0, 0.005])]),
        Part::new("seam", [0.0; 3], vec![Cuboid::new([12.0, 0.0], [-1.0, -5.0, 0.0], [2.0, 10.0, 0.005])]).rotated([0.0, std::f32::consts::FRAC_PI_2, 0.0]),
        Part::new("left_pages", [0.0; 3], vec![Cuboid::new([0.0, 10.0], [0.0, -4.0, -0.99], [5.0, 8.0, 1.0])]),
        Part::new("right_pages", [0.0; 3], vec![Cuboid::new([12.0, 10.0], [0.0, -4.0, -0.01], [5.0, 8.0, 1.0])]),
        Part::new("flip_page1", [0.0; 3], vec![blatt()]),
        Part::new("flip_page2", [0.0; 3], vec![blatt()]),
    ];
    Model { texture: ENCHANTING_BOOK_TEXTURE, tex_size: [64.0, 32.0], parts }
}

// BookModel.setAngles: `open` 0..1 (zu..auf), die beiden Blaetter je 0..1 (links..rechts
// umgeschlagen), `time` in Ticks (nur das leichte Atmen des Aufklappwinkels).
#[must_use] 
pub fn enchanting_book_pose(time: f32, left_flip: f32, right_flip: f32, open: f32) -> Pose {
    let g = ((time * 0.02).sin() * 0.1 + 1.25) * open;
    let mut pose = Pose::new();
    let dreh = |yaw: f32, x: f32| PartPose { rotation: [0.0, yaw, 0.0], pivot_offset: [x, 0.0, 0.0], ..Default::default() };
    pose.set("left_lid", dreh(std::f32::consts::PI + g, 0.0));
    pose.set("right_lid", dreh(-g, 0.0));
    pose.set("left_pages", dreh(g, g.sin()));
    pose.set("right_pages", dreh(-g, g.sin()));
    pose.set("flip_page1", dreh(g - g * 2.0 * left_flip, g.sin()));
    pose.set("flip_page2", dreh(g - g * 2.0 * right_flip, g.sin()));
    pose
}

// EndCrystalEntityRenderer.getYOffset: das Auf und Ab des Kristalls, in Bloecken.
#[must_use] 
pub fn end_crystal_y_offset(age: f32) -> f32 {
    let g = (age * 0.2).sin() / 2.0 + 0.5;
    (g * g + g) * 0.4 - 1.4
}

// EndCrystalEntityModel.setAngles: der Kristall steigt und sinkt (outer_glass.originY) und
// die drei Wuerfel drehen sich um eine schraege Achse. `age` in Ticks
// (EndCrystalEntity.endCrystalAge + tickProgress).
#[must_use] 
pub fn end_crystal_pose(age: f32) -> Pose {
    let f = age * 3.0;
    let g = end_crystal_y_offset(age) * 16.0;
    let s = std::f32::consts::FRAC_PI_4.sin();
    let schraeg = Affine::rotation_axis_angle(std::f32::consts::PI / 3.0, [s, 0.0, s]);
    let mut pose = Pose::new();
    // POSITIVE_Y.rotationDegrees(f).rotateAxis(PI/3, ...): erst Y, dann die schraege Achse.
    pose.set(
        "outer_glass",
        PartPose { pivot_offset: [0.0, g / 2.0, 0.0], rotation_matrix: Some(Affine::rotation_y_deg(f).then(schraeg)), ..Default::default() },
    );
    // setAngleAxis(PI/3, ...).rotateY(f): erst die schraege Achse, dann Y.
    let innen = schraeg.then(Affine::rotation_y_deg(f));
    pose.set("inner_glass", PartPose { rotation_matrix: Some(innen), ..Default::default() });
    pose.set("cube", PartPose { rotation_matrix: Some(innen), ..Default::default() });
    pose
}

// --- Windkugel ---------------------------------------------------------------------------
//
// WindChargeEntityModel.getTexturedModelData (Textur 64x32):
//
//   bone        origin(0, 0, 0)
//     wind        uv(15,20) cuboid(-4,-1,-4, 8,2,8) + uv(0,9) cuboid(-3,-2,-3, 6,4,6)
//     wind_charge uv(0,0)   cuboid(-2,-2,-2, 4,4,4)
//
// Gezeichnet von WindChargeEntityRenderer ohne eigene Verschiebung: das Modell steht mittig auf
// der Entity-Position, Y nach oben (kein LivingEntityRenderer, keine Spiegelung). Vanilla
// zeichnet es mit der durchscheinenden breeze_wind-Schicht und laufender Textur -- hier
// ausgeschnitten und ohne Lauf. Gilt ebenso fuer die Windkugel der Breeze.
pub const WIND_CHARGE_TEXTURE: &str = "entity/projectiles/wind_charge";

#[must_use] 
pub fn wind_charge_model() -> Model {
    let wind = Part::new(
        "wind",
        [0.0; 3],
        vec![Cuboid::new([15.0, 20.0], [-4.0, -1.0, -4.0], [8.0, 2.0, 8.0]), Cuboid::new([0.0, 9.0], [-3.0, -2.0, -3.0], [6.0, 4.0, 6.0])],
    );
    let kern = Part::new("wind_charge", [0.0; 3], vec![Cuboid::new([0.0, 0.0], [-2.0, -2.0, -2.0], [4.0, 4.0, 4.0])]);
    Model { texture: WIND_CHARGE_TEXTURE, tex_size: [64.0, 32.0], parts: vec![Part::new("bone", [0.0; 3], Vec::new()).with(wind).with(kern)] }
}

// WindChargeEntityModel.setAngles: Wirbel und Kern drehen gegenlaeufig, 16 Grad je Tick.
#[must_use] 
pub fn wind_charge_pose(age: f32) -> Pose {
    let mut pose = Pose::new();
    pose.set("wind", PartPose { rotation_matrix: Some(Affine::rotation_y_deg(age * 16.0)), ..Default::default() });
    pose.set("wind_charge", PartPose { rotation_matrix: Some(Affine::rotation_y_deg(-age * 16.0)), ..Default::default() });
    pose
}

// EndCrystalEntityRenderer.render: scale(2) und translate(0, -0.5, 0) um die Entity-Position.
#[must_use] 
pub fn end_crystal_transform(feet: [f32; 3]) -> Affine {
    Affine::translation(feet).then(Affine::scale([2.0; 3])).then(Affine::translation([0.0, -0.5, 0.0]))
}

// --- Pfeil ------------------------------------------------------------------------------
//
// ArrowModel.createBodyLayer (1.21.11), alle Teile mit UV (0,0):
//
//   back:    flache Platte bei x=0, 5x5 ab (-2.5, -2.5), Pivot (-11, 0, 0), um X um PI/4
//            gedreht, auf 0.8 skaliert
//   cross_1: flache Platte 16x4 ab (-12, -2), in y auf 0.8 gestaucht, um X um PI/4 gedreht
//   cross_2: dasselbe, um 3*PI/4 gedreht
//   und das ganze Modell danach auf 0.9 skaliert
//
// `transformed` multipliziert nur die SKALIERUNG jedes Teils, nicht seinen Pivot -- deshalb
// steht der Pivot des Schafts unveraendert bei -11 und nur die Skalen tragen die 0.9
// (0.8 * 0.9 = 0.72 beim Schaft).
//
// Alle drei Kaesten sind FLACH (eine Dimension 0): das Kreuz besteht aus zwei um 45 Grad
// gegeneinander verdrehten Ebenen, die Federn aus einer dritten. Ein Kasten ohne Tiefe gibt
// vier entartete Flaechen ohne Flaeche und zwei sichtbare -- genau wie in Vanilla.
pub const ARROW_TEXTURE: &str = "entity/projectiles/arrow";
pub const SPECTRAL_ARROW_TEXTURE: &str = "entity/projectiles/spectral_arrow";

#[must_use] 
pub fn arrow_model(texture: &'static str) -> Model {
    use std::f32::consts::PI;
    let kreuz = Cuboid::new([0.0, 0.0], [-12.0, -2.0, 0.0], [16.0, 4.0, 0.0]).with_tex_scale(1.0, 0.8);
    Model {
        texture,
        tex_size: [32.0, 32.0],
        parts: vec![
            Part::new("back", [-11.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [0.0, -2.5, -2.5], [0.0, 5.0, 5.0])])
                .rotated([PI / 4.0, 0.0, 0.0])
                .scaled(0.8 * 0.9),
            Part::new("cross_1", [0.0; 3], vec![kreuz]).rotated([PI / 4.0, 0.0, 0.0]).scaled(0.9),
            Part::new("cross_2", [0.0; 3], vec![kreuz]).rotated([PI * 3.0 / 4.0, 0.0, 0.0]).scaled(0.9),
        ],
    }
}

// ArrowRenderer.submit: NUR zwei Drehungen, sonst nichts -- um Y um (yRot - 90) Grad, dann
// um Z um xRot Grad.
//
// Entscheidend ist, was hier NICHT steht: ArrowRenderer erbt von EntityRenderer, nicht von
// LivingEntityRenderer -- es gibt also KEINE Spiegelung (-1,-1,1) und keine Verschiebung um
// -1.501. Das Modell steht in gewoehnlicher Weltausrichtung mit Y nach oben, sein Ursprung
// auf der Entity-Position (Unterkante der Hitbox, wie bei jedem Entity).
//
// Die -90 Grad im Yaw sind kein Schoenheitsfehler: das Modell zeigt entlang +X (Spitze bei
// x = 4, Federn bei x = -11), Minecrafts Yaw 0 aber nach +Z.
#[must_use] 
pub fn arrow_transform(feet: [f32; 3], yaw_deg: f32, pitch_deg: f32) -> Affine {
    Affine::translation(feet).then(Affine::rotation_y_deg(yaw_deg - 90.0)).then(rotation_z_deg(pitch_deg))
}

#[must_use] 
pub fn model_table() -> Vec<(&'static str, Model, ModelKind)> {
    let biped = |name: &'static str, texture: &'static str| (name, biped_model(texture, 0.0), ModelKind::Biped);
    vec![
        biped("zombie", "entity/zombie/zombie"),
        biped("drowned", "entity/zombie/drowned"),
        biped("zombified_piglin", "entity/piglin/zombified_piglin"),
        biped("piglin", "entity/piglin/piglin"),
        biped("piglin_brute", "entity/piglin/piglin_brute"),
        biped("skeleton", "entity/skeleton/skeleton"),
        biped("wither_skeleton", "entity/skeleton/wither_skeleton"),
        biped("stray", "entity/skeleton/stray"),
        biped("bogged", "entity/skeleton/bogged"),
        ("creeper", creeper_model(), ModelKind::Creeper),
        ("enderman", enderman_model(), ModelKind::Enderman),
        ("spider", spider_model("entity/spider/spider"), ModelKind::Spider),
        ("cave_spider", spider_model("entity/spider/cave_spider"), ModelKind::Spider),
        ("slime", slime_model("entity/slime/slime"), ModelKind::Slime),
        ("magma_cube", slime_model("entity/slime/magmacube"), ModelKind::Slime),
        ("cow", cow_model("entity/cow/temperate_cow"), ModelKind::Quadruped),
        ("mooshroom", cow_model("entity/cow/red_mooshroom"), ModelKind::Quadruped),
        ("pig", pig_model(), ModelKind::Quadruped),
        ("sheep", sheep_model(), ModelKind::Quadruped),
        ("cat", feline_model("entity/cat/tabby"), ModelKind::Feline),
        ("ocelot", feline_model("entity/cat/ocelot"), ModelKind::Feline),
        ("turtle", turtle_model(), ModelKind::Turtle),
    ]
}

// FelineModel.createBodyMesh(CubeDeformation.NONE), 64x32: Katze und Ozelot. Der Schwanz
// besteht aus zwei Stuecken, das hintere ist minimal (-0.02) eingezogen.
#[must_use] 
pub fn feline_model(texture: &'static str) -> Model {
    let hind_leg = |name: &'static str, x: f32| Part::new(name, [x, 18.0, 5.0], vec![Cuboid::new([8.0, 13.0], [-1.0, 0.0, 1.0], [2.0, 6.0, 2.0])]);
    let front_leg = |name: &'static str, x: f32| Part::new(name, [x, 14.1, -5.0], vec![Cuboid::new([40.0, 0.0], [-1.0, 0.0, 0.0], [2.0, 10.0, 2.0])]);
    Model {
        texture,
        tex_size: [64.0, 32.0],
        parts: vec![
            Part::new(
                "head",
                [0.0, 15.0, -9.0],
                vec![
                    Cuboid::new([0.0, 0.0], [-2.5, -2.0, -3.0], [5.0, 4.0, 5.0]),
                    Cuboid::new([0.0, 24.0], [-1.5, -0.001, -4.0], [3.0, 2.0, 2.0]),
                    Cuboid::new([0.0, 10.0], [-2.0, -3.0, 0.0], [1.0, 1.0, 2.0]),
                    Cuboid::new([6.0, 10.0], [1.0, -3.0, 0.0], [1.0, 1.0, 2.0]),
                ],
            ),
            Part::new("body", [0.0, 12.0, -10.0], vec![Cuboid::new([20.0, 0.0], [-2.0, 3.0, -8.0], [4.0, 16.0, 6.0])]).rotated([
                std::f32::consts::FRAC_PI_2,
                0.0,
                0.0,
            ]),
            Part::new("tail1", [0.0, 15.0, 8.0], vec![Cuboid::new([0.0, 15.0], [-0.5, 0.0, 0.0], [1.0, 8.0, 1.0])]).rotated([0.9, 0.0, 0.0]),
            Part::new("tail2", [0.0, 20.0, 14.0], vec![Cuboid::new([4.0, 15.0], [-0.5, 0.0, 0.0], [1.0, 8.0, 1.0]).dilated(-0.02)]),
            hind_leg("left_hind_leg", 1.1),
            hind_leg("right_hind_leg", -1.1),
            front_leg("left_front_leg", 1.2),
            front_leg("right_front_leg", -1.2),
        ],
    }
}

// TurtleModel.createBodyLayer, 128x64. Der Eierbauch (egg_belly) fehlt: ob eine Schildkroete
// ein Ei traegt, liest dieser Client nicht.
#[must_use] 
pub fn turtle_model() -> Model {
    let rumpf = [std::f32::consts::FRAC_PI_2, 0.0, 0.0];
    Model {
        texture: "entity/turtle/big_sea_turtle",
        tex_size: [128.0, 64.0],
        parts: vec![
            Part::new("head", [0.0, 19.0, -10.0], vec![Cuboid::new([3.0, 0.0], [-3.0, -1.0, -3.0], [6.0, 5.0, 6.0])]),
            Part::new(
                "body",
                [0.0, 11.0, -10.0],
                vec![
                    Cuboid::new([7.0, 37.0], [-9.5, 3.0, -10.0], [19.0, 20.0, 6.0]),
                    Cuboid::new([31.0, 1.0], [-5.5, 3.0, -13.0], [11.0, 18.0, 3.0]),
                ],
            )
            .rotated(rumpf),
            Part::new("right_hind_leg", [-3.5, 22.0, 11.0], vec![Cuboid::new([1.0, 23.0], [-2.0, 0.0, 0.0], [4.0, 1.0, 10.0])]),
            Part::new("left_hind_leg", [3.5, 22.0, 11.0], vec![Cuboid::new([1.0, 12.0], [-2.0, 0.0, 0.0], [4.0, 1.0, 10.0])]),
            Part::new("right_front_leg", [-5.0, 21.0, -4.0], vec![Cuboid::new([27.0, 30.0], [-13.0, 0.0, -2.0], [13.0, 1.0, 5.0])]),
            Part::new("left_front_leg", [5.0, 21.0, -4.0], vec![Cuboid::new([27.0, 24.0], [0.0, 0.0, -2.0], [13.0, 1.0, 5.0])]),
        ],
    }
}
