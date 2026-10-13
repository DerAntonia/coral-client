// Echte Entity-Modelle statt einfarbiger Boxen: derselbe Quader-Aufbau, den Vanilla
// benutzt, nach dem yarn-gemappten 1.21.11-Jar nachgebaut (net.minecraft.client.model.
// ModelPart$Cuboid/$Quad, ModelPart.applyTransform, net.minecraft.client.render.entity.
// model.* fuer die einzelnen Modelle, LivingEntityRenderer.render fuer die Einbettung in
// die Welt). Keine geratenen Zahlen -- jede Kantenlaenge, jeder Pivot und jeder
// Textur-Offset stimmt mit Vanilla ueberein.
//
// Aufbau in drei Schichten:
//   1. Cuboid  -- ein Kasten in MODELLPIXELN (1/16 Block), mit Textur-Offset und Dilation
//   2. Part    -- ein Gelenk: Pivot, Grunddrehung, Kaesten und Kind-Teile
//   3. Model   -- Wurzel-Teil + Texturname + Texturgroesse
// Die Animation (Kopfdrehung, Schrittschwingen) kommt nicht ins Modell, sondern als Pose
// obendrauf -- die Modelldaten bleiben damit reine, unveraenderliche Vanilla-Daten.
//
// Koordinaten: Vanillas Modellraum hat Y NACH UNTEN und den Ursprung am Kopf-Pivot; erst
// LivingEntityRenderer dreht das gerade (s. entity_transform).

use crate::mesher::BlockVertex;

// Ein Kasten in Modellpixeln. `offset`/`size` sind Vanillas cuboid(x,y,z,sizeX,sizeY,sizeZ),
// `dilation` das Dilation-Argument (blaeht den Kasten nach ALLEN Seiten auf, ohne die UVs zu
// aendern -- so entstehen die Aussenlagen Hut/Jacke/Aermel), `uv` der Textur-Offset aus
// ModelPartBuilder::uv, `mirror` das mirrored()-Flag.
#[derive(Clone, Copy, Debug)]
pub struct Cuboid {
    pub offset: [f32; 3],
    pub size: [f32; 3],
    // Je Achse (x, y, z): fast immer gleich, nur einige Babymodelle dehnen ungleichmaessig.
    pub dilation: [f32; 3],
    pub uv: [f32; 2],
    pub mirror: bool,
    // CubeListBuilder.addBox(..., texScaleU, texScaleV): streckt die Textur auf DIESEM Kasten.
    // Vanilla reicht das als `i * texScale.u()` / `j * texScale.v()` an ModelPart$Cube weiter,
    // also als EFFEKTIVE Texturgroesse fuer die UV-Normierung (s. CubeDefinition.bake und
    // ModelPart$Polygon, das durch genau diese Werte teilt). [1,1] = unveraendert.
    pub tex_scale: [f32; 2],
    // Welche Seiten gezeichnet werden (Bit je Seite in der Reihenfolge von emit_cuboid:
    // unten, oben, West, Nord, Ost, Sued). Vanilla laesst bei wenigen Kaesten Seiten weg
    // (ModelCuboidData.visibleFaces).
    pub faces: u8,
}

impl Cuboid {
    pub const fn new(uv: [f32; 2], offset: [f32; 3], size: [f32; 3]) -> Self {
        Self { offset, size, dilation: [0.0; 3], uv, mirror: false, tex_scale: [1.0, 1.0], faces: 0b11_1111 }
    }

    pub const fn with_tex_scale(mut self, u: f32, v: f32) -> Self {
        self.tex_scale = [u, v];
        self
    }

    pub const fn dilated(mut self, dilation: f32) -> Self {
        self.dilation = [dilation; 3];
        self
    }

    pub const fn mirrored(mut self) -> Self {
        self.mirror = true;
        self
    }
}

// Ein Gelenk des Modells. `rotation` ist die Grunddrehung aus ModelTransform (Radiant,
// Reihenfolge pitch/yaw/roll wie in Vanilla); die Animation addiert darauf (s. Pose).
#[derive(Clone, Debug)]
pub struct Part {
    pub name: &'static str,
    pub pivot: [f32; 3],
    pub rotation: [f32; 3],
    // ModelTransform.withScale: je Achse, nach der Drehung (ModelPart.applyTransform).
    pub scale: [f32; 3],
    pub cuboids: Vec<Cuboid>,
    pub children: Vec<Part>,
}

impl Part {
    pub fn new(name: &'static str, pivot: [f32; 3], cuboids: Vec<Cuboid>) -> Self {
        Self { name, pivot, rotation: [0.0; 3], scale: [1.0; 3], cuboids, children: Vec::new() }
    }

    pub fn scaled(mut self, scale: f32) -> Self {
        self.scale = [scale; 3];
        self
    }

    pub fn rotated(mut self, rotation: [f32; 3]) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with(mut self, child: Part) -> Self {
        self.children.push(child);
        self
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    // Pfad unter textures/, ohne Endung -- z.B. "entity/creeper/creeper".
    pub texture: &'static str,
    pub tex_size: [f32; 2],
    pub parts: Vec<Part>,
}

// Laufzeit-Zusatz je Teil: Drehung (addiert auf die Grunddrehung) und Pivot-Verschiebung.
// Beides braucht Vanilla auch: setAngles setzt Winkel, das Schleichen verschiebt zusaetzlich
// Pivots (BipedEntityModel.setAngles, Zweig isInSneakingPose).
#[derive(Clone, Copy, Debug, Default)]
pub struct PartPose {
    pub rotation: [f32; 3],
    pub pivot_offset: [f32; 3],
    // Ersetzt (statt ergaenzt) die Drehung dieses Teils durch eine fertige Matrix. Gebraucht
    // fuer den Umhang: dessen Drehung ist bei Vanilla ein QUATERNIONEN-Produkt
    // (ModelPart.rotate, s. cape_pose), kein Winkeltripel. Vanilla rechnet es hinterher per
    // getEulerAnglesZYX in ein Tripel zurueck -- das ist dieselbe Drehung, nur einmal
    // zerlegt und wieder zusammengesetzt. Wir nehmen direkt die Matrix: exakt dasselbe
    // Ergebnis, ohne die Zerlegung und ihre Randfaelle (Gimbal-Lock) nachbauen zu muessen.
    pub rotation_matrix: Option<Affine>,
    // Teil samt Kindern nicht zeichnen (ModelPart.visible = false), z.B. die verschraenkten
    // Arme eines Illagers, sobald er etwas haelt.
    pub hidden: bool,
}

// Die Pose eines ganzen Modells. Eine kurze Liste statt einer HashMap: kein Modell hier hat
// mehr als ein paar Dutzend Teile, und lineares Suchen in einem Slice ist dabei schneller
// als jedes Hashing.
#[derive(Clone, Debug, Default)]
pub struct Pose {
    entries: Vec<(&'static str, PartPose)>,
}

impl Pose {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, part: &'static str, pose: PartPose) {
        match self.entries.iter_mut().find(|(name, _)| *name == part) {
            Some((_, existing)) => *existing = pose,
            None => self.entries.push((part, pose)),
        }
    }

    // Nur die Drehung setzen (der haeufigste Fall). Eine schon gesetzte Drehmatrix wird
    // dabei GELOESCHT -- sonst haette die Winkelangabe hier keine Wirkung mehr und der
    // Aufrufer suchte lange, warum sein rotate() ignoriert wird.
    pub fn rotate(&mut self, part: &'static str, rotation: [f32; 3]) {
        let pivot_offset = self.get(part).pivot_offset;
        self.set(part, PartPose { rotation, pivot_offset, rotation_matrix: None, hidden: false });
    }

    pub fn get(&self, part: &str) -> PartPose {
        self.entries.iter().find(|(name, _)| *name == part).map(|(_, pose)| *pose).unwrap_or_default()
    }
}

// Affine Transformation (3x3-Drehung + Verschiebung). Eigene Mini-Implementierung, damit das
// world-Crate keine Matrix-Bibliothek braucht -- gebraucht werden genau Verketten und
// Punkt-Transformieren.
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub m: [[f32; 3]; 3],
    pub t: [f32; 3],
}

impl Affine {
    pub const IDENTITY: Affine = Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    pub fn translation(t: [f32; 3]) -> Self {
        Self { m: Self::IDENTITY.m, t }
    }

    pub fn scale(s: [f32; 3]) -> Self {
        Self { m: [[s[0], 0.0, 0.0], [0.0, s[1], 0.0], [0.0, 0.0, s[2]]], t: [0.0; 3] }
    }

    // Drehung um die Y-Achse in Grad.
    pub fn rotation_y_deg(degrees: f32) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Self { m: [[cos, 0.0, sin], [0.0, 1.0, 0.0], [-sin, 0.0, cos]], t: [0.0; 3] }
    }

    // ModelPart.applyTransform: Quaternionf().rotationZYX(roll, yaw, pitch) -- also erst um
    // X (pitch), dann Y (yaw), dann Z (roll) angewandt, als Matrix Rz * Ry * Rx.
    pub fn rotation_zyx(pitch: f32, yaw: f32, roll: f32) -> Self {
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        let (sr, cr) = roll.sin_cos();
        // Rz(roll) * Ry(yaw) * Rx(pitch), ausmultipliziert.
        Self {
            m: [
                [cr * cy, cr * sy * sp - sr * cp, cr * sy * cp + sr * sp],
                [sr * cy, sr * sy * sp + cr * cp, sr * sy * cp - cr * sp],
                [-sy, cy * sp, cy * cp],
            ],
            t: [0.0; 3],
        }
    }

    // Quaternionf().rotationXYZ(x, y, z) -- als Matrix Rx * Ry * Rz, also die UMGEKEHRTE
    // Reihenfolge zu rotation_zyx oben. Vanilla benutzt beide nebeneinander: Modellteile
    // drehen mit ZYX (ModelPart.applyTransform), die Halte-Transformation eines Items
    // dagegen mit XYZ (Transformation.apply). Winkel in GRAD, weil die Modell-JSONs sie so
    // angeben.
    pub fn rotation_xyz_deg(x_deg: f32, y_deg: f32, z_deg: f32) -> Self {
        let (sx, cx) = x_deg.to_radians().sin_cos();
        let (sy, cy) = y_deg.to_radians().sin_cos();
        let (sz, cz) = z_deg.to_radians().sin_cos();
        // Rx(x) * Ry(y) * Rz(z), ausmultipliziert.
        Self {
            m: [
                [cy * cz, -cy * sz, sy],
                [sx * sy * cz + cx * sz, -sx * sy * sz + cx * cz, -sx * cy],
                [-cx * sy * cz + sx * sz, cx * sy * sz + sx * cz, cx * cy],
            ],
            t: [0.0; 3],
        }
    }

    // Drehung um eine Achse (Rodrigues) -- JOMLs Quaternionf.setAngleAxis als Matrix. Die
    // Achse muss nicht normiert sein.
    pub fn rotation_axis_angle(angle: f32, axis: [f32; 3]) -> Self {
        let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt().max(1e-9);
        let [x, y, z] = [axis[0] / len, axis[1] / len, axis[2] / len];
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;
        Self {
            m: [
                [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
                [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
                [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
            ],
            t: [0.0; 3],
        }
    }

    // self danach other (also self * other als Matrixprodukt: erst other, dann self).
    pub fn then(self, other: Affine) -> Affine {
        let mut m = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = (0..3).map(|k| self.m[i][k] * other.m[k][j]).sum();
            }
        }
        Affine { m, t: self.apply(other.t) }
    }

    // Eine RICHTUNG transformieren: nur die Drehmatrix, ohne die Verschiebung. Fuer
    // Flaechen-Normalen (Vanilla: MatrixStack.Entry.transformNormal).
    pub fn apply_direction(self, d: [f32; 3]) -> [f32; 3] {
        let out = [
            self.m[0][0] * d[0] + self.m[0][1] * d[1] + self.m[0][2] * d[2],
            self.m[1][0] * d[0] + self.m[1][1] * d[1] + self.m[1][2] * d[2],
            self.m[2][0] * d[0] + self.m[2][1] * d[1] + self.m[2][2] * d[2],
        ];
        let len = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
        if len > 1e-6 {
            [out[0] / len, out[1] / len, out[2] / len]
        } else {
            out
        }
    }

    pub fn apply(self, p: [f32; 3]) -> [f32; 3] {
        [
            self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.m[0][2] * p[2] + self.t[0],
            self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.m[1][2] * p[2] + self.t[1],
            self.m[2][0] * p[0] + self.m[2][1] * p[1] + self.m[2][2] * p[2] + self.t[2],
        ]
    }
}

// Vanillas Einbettung eines LivingEntity in die Welt (LivingEntityRenderer.render):
//   rotateY(180 - bodyYaw); scale(-1, -1, 1); translate(0, -1.501, 0)
// danach ist der Modellraum in BLOECKEN -- die Kastenkoordinaten werden beim Erzeugen der
// Vertices durch 16 geteilt (ModelPart$Vertex::worldX). `feet` ist die Entity-Position.
//
// Das scale(-1,-1,1) ist der Grund, warum Modelle "auf dem Kopf" definiert sind: Vanillas
// Modelldaten haben Y nach unten (der Kopf sitzt bei y=-8..0), erst diese Spiegelung dreht
// sie richtig. Und die 1.501 statt 1.5 sind kein Tippfehler von uns, sondern stehen so im
// Jar (ein Tausendstel Block, damit Fuesse und Boden nicht exakt gleich hoch liegen und
// z-fighten).
pub fn entity_transform(feet: [f32; 3], body_yaw_deg: f32, scale: f32, model_scale: f32) -> Affine {
    entity_transform_leaning(feet, body_yaw_deg, scale, model_scale, &Lean::NONE)
}

// Vanillas PlayerEntityRenderer.scale() -- das Spielermodell wird gleichmaessig um diesen
// Faktor VERKLEINERT.
// Das rohe Modell ist 32 Modellpixel = 2.0 Bloecke hoch; erst mit diesem Faktor wird es
// 1.875 Bloecke hoch und passt zur Spielerhoehe von 1.8.
//
// WARUM DAS SICHTBAR IST: Ohne den Faktor liegen die Augen des Modells bei rund 1.72 statt
// 1.62 Bloecken -- der Kopf ist 8 Modellpixel hoch, die Augenreihe sitzt gut die Haelfte
// davon unter dem Scheitel. Man schaut dann bei Blickneigung 0 einem gleich hohen Spieler
// nicht in die Augen, sondern rund 1.6 Kopfpixel darunter auf die Nase.
// Der Fehler war NICHT die eigene Augenhoehe (1.62 stimmt), sondern das zu grosse Gegenueber.
pub const PLAYER_MODEL_SCALE: f32 = 0.9375;

// Drehung um die X-Achse in Grad -- gebraucht fuer die Schwimm-/Kriech-Neigung und die
// Ego-Hand.
pub fn rotation_x_deg(degrees: f32) -> Affine {
    let (sin, cos) = degrees.to_radians().sin_cos();
    Affine { m: [[1.0, 0.0, 0.0], [0.0, cos, -sin], [0.0, sin, cos]], t: [0.0; 3] }
}

// Drehung um die Z-Achse in Grad -- gebraucht fuer die Seitwaertsneigung des Umhangs.
pub fn rotation_z_deg(degrees: f32) -> Affine {
    let (sin, cos) = degrees.to_radians().sin_cos();
    Affine { m: [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] }
}

// Schwimm-/Kriech-Haltung. Vanilla: PlayerEntityRenderer.setupTransforms, Zweig
// leaningPitch > 0.
//
//   amount        = leaningPitch (0..1) -- laeuft in der Schwimm-Pose mit 0.09 pro Tick auf 1
//                   und wieder zurueck (LivingEntity.updateLeaningPitch)
//   pitch_deg     = Blickneigung in VANILLA-Konvention (positiv = nach unten)
//   in_water      = isTouchingWater; nur im Wasser richtet sich der Koerper nach dem Blick,
//                   beim Kriechen an Land liegt er stur waagerecht
//   swimming      = isSwimming (echtes Sprint-Schwimmen) -- verschiebt den Koerper noch
//                   zusaetzlich, sonst steckte der Kopf in der Wasseroberflaeche
#[derive(Clone, Copy, Debug)]
pub struct Lean {
    pub amount: f32,
    pub pitch_deg: f32,
    pub in_water: bool,
    pub swimming: bool,
}

impl Lean {
    pub const NONE: Lean = Lean { amount: 0.0, pitch_deg: 0.0, in_water: false, swimming: false };
}

// Wie entity_transform, aber mit der Schwimm-Neigung dazwischen. Die Reihenfolge ist exakt die
// von LivingEntityRenderer.render:
//     1. skalieren um `scale` (baseScale der Entity)
//     2. setupTransforms (Koerperdrehung, hier dazu die Neigung)
//     3. spiegeln (-1, -1, 1)
//     4. skalieren um `model_scale` (beim Spieler 0.9375)
//     5. um 1.501 nach unten verschieben
//
// `model_scale` steht damit ZWISCHEN der Spiegelung und dem 1.501-Versatz, nicht davor: der
// Versatz wird also mitskaliert (0.9375 * 1.501). Steht er an der falschen Stelle, schwebt oder
// versinkt das Modell -- deshalb hier so ausdruecklich.
pub fn entity_transform_leaning(feet: [f32; 3], body_yaw_deg: f32, scale: f32, model_scale: f32, lean: &Lean) -> Affine {
    let mut t = Affine::translation(feet).then(Affine::scale([scale, scale, scale])).then(Affine::rotation_y_deg(180.0 - body_yaw_deg));
    if lean.amount > 0.0 {
        // j = touchingWater ? -90 - pitch : -90; k = lerp(amount, 0, j) -- also amount * j.
        let j = if lean.in_water { -90.0 - lean.pitch_deg } else { -90.0 };
        t = t.then(rotation_x_deg(lean.amount * j));
        if lean.swimming {
            t = t.then(Affine::translation([0.0, -1.0, 0.3]));
        }
    }
    t.then(Affine::scale([-1.0, -1.0, 1.0]))
        .then(Affine::scale([model_scale, model_scale, model_scale]))
        .then(Affine::translation([0.0, -1.501, 0.0]))
}

// Elytra-Gleitflug eines Lebewesens -- alles, was Vanilla dafuer aus dem Entity liest
// (LivingEntityRenderState: glidingTicks, pitch, velocity; PlayerEntityRenderer.updateRenderState:
// flyingRotation).
#[derive(Clone, Copy, Debug)]
pub struct Glide {
    /// LivingEntity.getGlidingTicks() samt Zwischentick-Anteil: Ticks seit Beginn des Flugs.
    pub ticks: f32,
    /// Blickneigung in Vanilla-Konvention (positiv = nach unten).
    pub pitch_deg: f32,
    /// Blickrichtung (Kopf-Gierung) in Vanilla-Konvention.
    pub yaw_deg: f32,
    /// Geschwindigkeit in Bloecken pro Tick.
    pub velocity: [f32; 3],
}

impl Glide {
    /// LivingEntityRenderState.getGlidingProgress: zieht in den ersten zehn Ticks quadratisch
    /// von aufrecht in die Flughaltung.
    pub fn progress(&self) -> f32 {
        (self.ticks * self.ticks / 100.0).clamp(0.0, 1.0)
    }

    /// BipedEntityModel.setAngles: nach vier Ticks Flug schaut der Kopf fest 45 Grad nach vorn
    /// (in Flugrichtung), statt der Blickneigung zu folgen.
    pub fn head_down(&self) -> bool {
        self.ticks > 4.0
    }

    /// BipedEntityRenderer.getLimbAmplitudeInverse: je schneller der Flug, desto weniger
    /// schlenkern Arme und Beine (die Schrittanimation wird durch diesen Wert geteilt).
    pub fn limb_amplitude_inverse(&self) -> f32 {
        if !self.head_down() {
            return 1.0;
        }
        let [x, y, z] = self.velocity;
        let f = (x * x + y * y + z * z) / 0.2;
        (f * f * f).max(1.0)
    }

    /// PlayerEntityRenderer.updateRenderState, flyingRotation: um wie viel die Flugbahn
    /// (waagerecht) von der Blickrichtung abweicht -- im Kurvenflug legt sich die Figur so
    /// in die Bewegungsrichtung. None, solange eine der beiden Richtungen senkrecht ist.
    pub fn flying_rotation_rad(&self) -> Option<f32> {
        // Entity.getRotationVector(pitch, yaw), nur der waagerechte Teil.
        let (p, y) = (self.pitch_deg.to_radians(), -self.yaw_deg.to_radians());
        let (rx, rz) = (y.sin() * p.cos(), y.cos() * p.cos());
        let (vx, vz) = (self.velocity[0], self.velocity[2]);
        let (d, e) = (vx * vx + vz * vz, rx * rx + rz * rz);
        if d <= 0.0 || e <= 0.0 {
            return None;
        }
        let l = ((vx * rx + vz * rz) / (d * e).sqrt()).min(1.0);
        let m = vx * rz - vz * rx;
        // Javas Math.signum: 0 bleibt 0 (Rust f32::signum gaebe 1).
        let sign = if m > 0.0 { 1.0 } else if m < 0.0 { -1.0 } else { 0.0 };
        Some(sign * l.max(-1.0).acos())
    }
}

// Wie entity_transform, aber in Gleitflug-Haltung -- PlayerEntityRenderer.setupTransforms,
// Zweig isGliding (ersetzt dort den Schwimm-Zweig):
//     1. Koerperdrehung wie immer
//     2. um X drehen: h * (-90 - pitch) Grad
//     3. um Y drehen: um die Flug-Rollung, falls vorhanden
// Danach wie immer Spiegelung, Modellskalierung und der 1.501-Versatz.
pub fn entity_transform_gliding(feet: [f32; 3], body_yaw_deg: f32, scale: f32, model_scale: f32, glide: &Glide) -> Affine {
    let mut t = Affine::translation(feet)
        .then(Affine::scale([scale, scale, scale]))
        .then(Affine::rotation_y_deg(180.0 - body_yaw_deg))
        .then(rotation_x_deg(glide.progress() * (-90.0 - glide.pitch_deg)));
    if let Some(rotation) = glide.flying_rotation_rad() {
        t = t.then(Affine::rotation_y_deg(rotation.to_degrees()));
    }
    t.then(Affine::scale([-1.0, -1.0, 1.0]))
        .then(Affine::scale([model_scale, model_scale, model_scale]))
        .then(Affine::translation([0.0, -1.501, 0.0]))
}

// Vanillas Richtungslicht fuer Entities in der Welt. Zwei feste Lichtrichtungen, aus
// DiffuseLighting (Typ LEVEL, CardinalLightType.DEFAULT -- die Oberwelt; der Nether dreht das
// zweite Licht nach unten, was dieser Client noch nicht unterscheidet):
//
//     DEFAULT_DIFFUSION_LIGHT_0 = normalize( 0.2, 1.0, -0.7)
//     DEFAULT_DIFFUSION_LIGHT_1 = normalize(-0.2, 1.0,  0.7)
//
// und die Mischformel aus assets/minecraft/shaders/include/light.glsl:
//
//     lightAccum = min(1, (max(0, n*L0) + max(0, n*L1)) * 0.6 + 0.4)
//
// Die beiden Konstanten heissen dort MINECRAFT_LIGHT_POWER (0.6) und
// MINECRAFT_AMBIENT_LIGHT (0.4) -- letztere ist der Grund, warum keine Flaeche ganz schwarz
// wird, auch wenn sie von beiden Lichtern weg zeigt.
//
// Fuer die sechs achsenparallelen Flaechen ergibt das:
//   oben 1.0, unten 0.4, Nord/Sued 0.7395, Ost/West 0.4970
//
// Wichtig: die Normale muss in WELTkoordinaten stehen, nach der Koerperdrehung. Nur dann
// wandert die Helligkeit einer Flaeche, waehrend sich das Entity dreht (0.74 -> 0.50 -> 0.74
// ueber eine Vierteldrehung). Eine fest je Modellflaeche eingebackene Tabelle -- wie sie fuer
// BLOECKE richtig ist, die sich nie drehen -- waere hier falsch: die Figur haette dann beim
// Drehen eine starre Helligkeitsverteilung, was genau den plastischen Eindruck zerstoert.
const DIFFUSE_LIGHT_0: [f32; 3] = [0.161_690_2, 0.808_451, -0.565_915_7];
const DIFFUSE_LIGHT_1: [f32; 3] = [-0.161_690_2, 0.808_451, 0.565_915_7];
const LIGHT_POWER: f32 = 0.6;
const AMBIENT_LIGHT: f32 = 0.4;

pub fn diffuse_shade(normal: [f32; 3]) -> f32 {
    let dot = |l: [f32; 3]| (l[0] * normal[0] + l[1] * normal[1] + l[2] * normal[2]).max(0.0);
    ((dot(DIFFUSE_LIGHT_0) + dot(DIFFUSE_LIGHT_1)) * LIGHT_POWER + AMBIENT_LIGHT).min(1.0)
}

// Textur-Rechteck im Entity-Atlas (u0, v0, u1, v1, normiert).
pub type TexRect = [f32; 4];

// Erzeugt die Dreiecke eines Modells. `base` ist die Einbettung (s. entity_transform),
// `tex` das Atlas-Rechteck der Modelltextur, `brightness` eine gleichmaessige Helligkeit
// (dieser Client hat noch kein Entity-Licht, s. Modulkommentar in mesher.rs).
pub fn emit(model: &Model, pose: &Pose, base: Affine, tex: TexRect, brightness: f32, out: &mut Vec<BlockVertex>) {
    emit_tinted(model, pose, base, tex, [1.0, 1.0, 1.0], brightness, out);
}

// Wie emit(), aber mit einer FARBE auf der Textur. Gebraucht fuer Lederruestung: ihre
// Grundtextur ist eine Graustufenvorlage, die Vanilla mit der Faerbung des Items
// multipliziert (leather.json, Layer mit `dyeable`) -- ungefaerbt gezeichnet waere sie fast
// weiss statt braun.
#[allow(clippy::too_many_arguments)]
pub fn emit_tinted(model: &Model, pose: &Pose, base: Affine, tex: TexRect, tint: [f32; 3], brightness: f32, out: &mut Vec<BlockVertex>) {
    let color = [tint[0] * brightness, tint[1] * brightness, tint[2] * brightness];
    for part in &model.parts {
        emit_part(part, pose, base, model.tex_size, tex, color, out);
    }
}

// Wie emit(), aber jeder Kasten in einer eigenen Liste und ohne Toenung -- fuer Inventar-Icons,
// die je Kasten die Aussenseite ihrer Flaechen bestimmen (s. app::item_icons).
pub fn emit_boxes(model: &Model, pose: &Pose, base: Affine, tex: TexRect) -> Vec<Vec<BlockVertex>> {
    fn teil(part: &Part, pose: &Pose, parent: Affine, tex_size: [f32; 2], tex: TexRect, out: &mut Vec<Vec<BlockVertex>>) {
        if pose.get(part.name).hidden {
            return;
        }
        let world = part_world_transform(part, pose, parent);
        for cuboid in &part.cuboids {
            let mut kasten = Vec::new();
            emit_cuboid(cuboid, world, tex_size, tex, [1.0; 3], &mut kasten);
            out.push(kasten);
        }
        for child in &part.children {
            teil(child, pose, world, tex_size, tex, out);
        }
    }
    let mut out = Vec::new();
    for part in &model.parts {
        teil(part, pose, base, model.tex_size, tex, &mut out);
    }
    out
}

// Die aufgelaufene Transformation EINES benannten Teils -- ohne etwas zu zeichnen.
//
// Das ist Vanillas `ModelPart.applyTransform(matrices)`: dieselbe Kette aus Pivot und
// Drehung wie in emit_part, nur bis zum gesuchten Teil und dann herausgereicht. Gebraucht
// fuer alles, was AM Modell haengt, aber nicht Teil davon ist -- bislang das Item in der
// Hand (s. app::held_item), das Vanilla mit genau diesem Aufruf ansetzt
// (HeldItemFeatureRenderer.renderItem -> ModelWithArms.setArmAngle).
pub fn part_transform(model: &Model, pose: &Pose, base: Affine, name: &str) -> Option<Affine> {
    model.parts.iter().find_map(|part| find_part_transform(part, pose, base, name))
}

fn find_part_transform(part: &Part, pose: &Pose, parent: Affine, name: &str) -> Option<Affine> {
    let world = part_world_transform(part, pose, parent);
    if part.name == name {
        return Some(world);
    }
    part.children.iter().find_map(|child| find_part_transform(child, pose, world, name))
}

// Der gemeinsame Kern von emit_part und part_transform: Pivot + Drehung eines Teils, auf die
// Elterntransformation gesetzt. Bewusst EINE Stelle -- liefen die beiden auseinander, saesse
// das Item neben der Hand statt darin, und zwar nur in bestimmten Posen.
fn part_world_transform(part: &Part, pose: &Pose, parent: Affine) -> Affine {
    let extra = pose.get(part.name);
    // ModelPart.applyTransform: erst der Pivot (in Modellpixeln, also /16), dann die Drehung.
    let pivot = [
        (part.pivot[0] + extra.pivot_offset[0]) / 16.0,
        (part.pivot[1] + extra.pivot_offset[1]) / 16.0,
        (part.pivot[2] + extra.pivot_offset[2]) / 16.0,
    ];
    let rotation = [part.rotation[0] + extra.rotation[0], part.rotation[1] + extra.rotation[1], part.rotation[2] + extra.rotation[2]];
    // Eine vorgegebene Matrix ersetzt BEIDE Winkelquellen (Grunddrehung des Teils und
    // Pose-Drehung) -- sie ist schon das vollstaendige Ergebnis, s. PartPose::rotation_matrix.
    let local = match extra.rotation_matrix {
        Some(matrix) => Affine::translation(pivot).then(matrix),
        None => Affine::translation(pivot).then(Affine::rotation_zyx(rotation[0], rotation[1], rotation[2])),
    };
    let local = if part.scale != [1.0; 3] { local.then(Affine::scale(part.scale)) } else { local };
    parent.then(local)
}

#[allow(clippy::too_many_arguments)]
fn emit_part(part: &Part, pose: &Pose, parent: Affine, tex_size: [f32; 2], tex: TexRect, color: [f32; 3], out: &mut Vec<BlockVertex>) {
    if pose.get(part.name).hidden {
        return;
    }
    let world = part_world_transform(part, pose, parent);

    for cuboid in &part.cuboids {
        emit_cuboid(cuboid, world, tex_size, tex, color, out);
    }
    for child in &part.children {
        emit_part(child, pose, world, tex_size, tex, color, out);
    }
}

// ModelPart$Cuboid: acht Ecken, sechs Seiten, UV-Spalten/-Zeilen aus der "Kreuz"-Abwicklung
// eines Kastens. Vertex- und UV-Zuordnung exakt wie in Vanilla -- eine andere Zuordnung
// dreht oder spiegelt einzelne Seiten, was man bei einem Skin sofort sieht (Gesicht am
// Hinterkopf).
fn emit_cuboid(cuboid: &Cuboid, world: Affine, tex_size: [f32; 2], tex: TexRect, base_color: [f32; 3], out: &mut Vec<BlockVertex>) {
    // s. Cuboid::tex_scale -- Vanilla teilt die UVs durch die SKALIERTE Texturgroesse.
    let tex_size = [tex_size[0] * cuboid.tex_scale[0], tex_size[1] * cuboid.tex_scale[1]];
    let [sx, sy, sz] = cuboid.size;
    let [dx, dy, dz] = cuboid.dilation;
    // Die undilatierten Kanten bestimmen die UVs, die dilatierten die Geometrie.
    let (mut x0, y0, z0) = (cuboid.offset[0] - dx, cuboid.offset[1] - dy, cuboid.offset[2] - dz);
    let (mut x1, y1, z1) = (cuboid.offset[0] + sx + dx, cuboid.offset[1] + sy + dy, cuboid.offset[2] + sz + dz);
    if cuboid.mirror {
        std::mem::swap(&mut x0, &mut x1);
    }

    // Vanillas vertex..vertex8, in derselben Reihenfolge.
    let v = [
        [x0, y0, z0], // 1
        [x1, y0, z0], // 2
        [x1, y1, z0], // 3
        [x0, y1, z0], // 4
        [x0, y0, z1], // 5
        [x1, y0, z1], // 6
        [x1, y1, z1], // 7
        [x0, y1, z1], // 8
    ];
    // Jede Ecke einmal in die Welt, nicht fuer jeden der 36 Vertices neu -- bei vielen
    // Entities war das der groesste Posten im Frame (Profiler: emit_part).
    let welt = v.map(|p| world.apply([p[0] / 16.0, p[1] / 16.0, p[2] / 16.0]));
    let [u, vv] = cuboid.uv;
    let (j, k, l, m, n, o) = (u, u + sz, u + sz + sx, u + sz + sx + sx, u + sz + sx + sz, u + sz + sx + sz + sx);
    let (p, q, r) = (vv, vv + sz, vv + sz + sy);

    // (Ecken-Indices, u1, v1, u2, v2) -- Reihenfolge: down, up, west, north, east, south.
    let faces: [([usize; 4], f32, f32, f32, f32); 6] = [
        ([5, 4, 0, 1], k, p, l, q),
        ([2, 3, 7, 6], l, q, m, p),
        ([0, 4, 7, 3], j, q, k, r),
        ([1, 0, 3, 2], k, q, l, r),
        ([5, 1, 2, 6], l, q, n, r),
        ([4, 5, 6, 7], n, q, o, r),
    ];

    // Modellraum-Normalen in derselben Reihenfolge wie `faces` (down, up, west, north, east,
    // south -- Vanillas Direction-Namen im Modellraum, wo Y nach UNTEN zeigt).
    //
    // Bei gespiegelten Kaesten (linker Arm, linkes Bein) kippt die X-Normale: dort sind x0 und
    // x1 vertauscht, die "west"-Flaeche liegt also rechts. Vanilla macht genau das in
    // ModelPart$Quad.getMirrorDirection -- Achse X wird umgedreht, die anderen bleiben.
    let mirror_x = if cuboid.mirror { -1.0 } else { 1.0 };
    let normals: [[f32; 3]; 6] =
        [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [-mirror_x, 0.0, 0.0], [0.0, 0.0, -1.0], [mirror_x, 0.0, 0.0], [0.0, 0.0, 1.0]];

    for (face_index, (corners, u1, v1, u2, v2)) in faces.into_iter().enumerate() {
        if cuboid.faces & (1 << face_index) == 0 {
            continue;
        }
        // Richtungslicht je Flaeche, in WELTkoordinaten (s. diffuse_shade).
        let shade = diffuse_shade(world.apply_direction(normals[face_index]));
        let color = [base_color[0] * shade, base_color[1] * shade, base_color[2] * shade];
        // ModelPart$Quad: Ecke 0 bekommt (u2,v1), 1 (u1,v1), 2 (u1,v2), 3 (u2,v2).
        let mut quad = [(corners[0], u2, v1), (corners[1], u1, v1), (corners[2], u1, v2), (corners[3], u2, v2)];
        // mirror dreht die Eckenreihenfolge NACH der UV-Zuweisung um (s. Quad-Konstruktor).
        if cuboid.mirror {
            quad.reverse();
        }
        let e = quad.map(|(ecke, u, v)| BlockVertex { position: welt[ecke], color, uv: atlas_uv(u, v, tex_size, tex) });
        // Zwei Dreiecke pro Viereck. Die Umlaufrichtung ist NICHT egal: die Pipeline
        // verwirft rueckwaertsgewandte Dreiecke (cull_mode: Face::Back, s.
        // render::BlockRenderer). Diese Reihenfolge hier ist die geeichte -- wer neue
        // Geometrie in denselben Zeichenaufruf haengt, muss sich nach ihr richten (s.
        // item_model::push_quad und dessen winding_tests).
        out.extend_from_slice(&[e[0], e[1], e[2], e[0], e[2], e[3]]);
    }
}

// Modelltextur-Pixel -> normierte Atlas-UV.
fn atlas_uv(u: f32, v: f32, tex_size: [f32; 2], tex: TexRect) -> [f32; 2] {
    let [u0, v0, u1, v1] = tex;
    [u0 + (u / tex_size[0]) * (u1 - u0), v0 + (v / tex_size[1]) * (v1 - v0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gleiten(ticks: f32, pitch_deg: f32, velocity: [f32; 3]) -> Glide {
        Glide { ticks, pitch_deg, yaw_deg: 0.0, velocity }
    }

    // Voll in der Flughaltung, geradeaus geschaut, ohne Seitendrift: dieselbe Drehung um
    // -90 - pitch wie beim Schwimmen im Wasser -- die Figur muss exakt gleich liegen.
    #[test]
    fn gleitflug_liegt_wie_schwimmen() {
        let glide = gleiten(20.0, 0.0, [0.0, -0.1, 0.0]);
        assert_eq!(glide.progress(), 1.0);
        let lean = Lean { amount: 1.0, pitch_deg: 0.0, in_water: true, swimming: false };
        let a = entity_transform_gliding([0.0; 3], 0.0, 1.0, PLAYER_MODEL_SCALE, &glide);
        let b = entity_transform_leaning([0.0; 3], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean);
        for p in [[0.0, -24.0 / 16.0, 0.0], [0.3, 0.0, 0.2]] {
            let (pa, pb) = (a.apply(p), b.apply(p));
            assert!((0..3).all(|i| (pa[i] - pb[i]).abs() < 1e-5), "{pa:?} != {pb:?}");
        }
    }

    #[test]
    fn gleitflug_setzt_langsam_ein_und_senkt_den_kopf() {
        assert!(gleiten(2.0, 0.0, [0.0; 3]).progress() < 0.1, "am Anfang noch fast aufrecht");
        assert!(!gleiten(4.0, 0.0, [0.0; 3]).head_down());
        assert!(gleiten(5.0, 0.0, [0.0; 3]).head_down());
        let pose = biped_pose_full(0.0, 30.0, 0.0, 0.0, BipedExtras { glide: Some(gleiten(10.0, 30.0, [0.0; 3])), ..Default::default() });
        assert_eq!(pose.get("head").rotation[0], -std::f32::consts::FRAC_PI_4);
    }

    #[test]
    fn schneller_flug_daempft_die_gliedmassen() {
        assert_eq!(gleiten(10.0, 0.0, [0.1, 0.0, 0.0]).limb_amplitude_inverse(), 1.0, "langsam: ungedaempft");
        assert!(gleiten(10.0, 0.0, [1.5, 0.0, 0.0]).limb_amplitude_inverse() > 100.0);
    }

    // Blick nach Sueden (+Z), Flug nach Osten (+X): eine Vierteldrehung. Senkrechter Flug hat
    // keine waagerechte Richtung -- dann gar keine Drehung.
    #[test]
    fn flugrichtung_dreht_die_figur() {
        let quer = gleiten(10.0, 0.0, [1.0, 0.0, 0.0]).flying_rotation_rad().unwrap();
        assert!((quer.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "{quer}");
        assert!(gleiten(10.0, 0.0, [0.0, 0.0, 1.0]).flying_rotation_rad().unwrap().abs() < 1e-3, "geradeaus");
        assert_eq!(gleiten(10.0, 0.0, [0.0, -1.0, 0.0]).flying_rotation_rad(), None);
    }

    #[test]
    fn elytra_breitet_sich_im_gleitflug_aus() {
        let angelegt = elytra_pose(None, false).get("left_wing").rotation;
        let waagerecht = elytra_pose(Some(&gleiten(10.0, 0.0, [1.0, 0.0, 0.0])), false).get("left_wing").rotation;
        let sturz = elytra_pose(Some(&gleiten(10.0, 90.0, [0.0, -2.0, 0.0])), false).get("left_wing").rotation;
        assert!((waagerecht[2] + std::f32::consts::FRAC_PI_2).abs() < 1e-5, "waagerecht: ganz ausgebreitet");
        assert!((sturz[2] - angelegt[2]).abs() < 1e-5, "senkrechter Sturz: wieder angelegt");
        assert_eq!(elytra_pose(None, false).get("right_wing").rotation[2], -angelegt[2], "rechts gespiegelt");
    }

    // Der Kopf des Spielermodells ist ein 8x8x8-Kasten mit Pivot (0,0,0) und Offset
    // (-4,-8,-4). Nach der Einbettung (s. entity_transform) muss er GENAU oben am Koerper
    // sitzen. Mit PLAYER_MODEL_SCALE sind das Unterkante 0.9375 * 1.501 = 1.4072 und Oberkante
    // 1.4072 + 0.9375 * 8/16 = 1.8759 Bloecke.
    // Y ist im Modellraum nach unten, das scale(-1,-1,1) dreht es -- genau deshalb wird aus
    // dem Kopf-Offset -8 eine Oberkante ueber dem Pivot und nicht darunter.
    #[test]
    fn kopf_sitzt_nach_der_einbettung_oben() {
        let model = crate::entity_model::player_model(false);
        let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
        let mut out = Vec::new();
        let head = model.parts.iter().find(|p| p.name == "head").expect("head fehlt");
        emit_part(
            &Part::new("head", head.pivot, head.cuboids.clone()),
            &Pose::new(),
            base,
            model.tex_size,
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            &mut out,
        );

        let min_y = out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let max_y = out.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        assert!((min_y - 1.4072).abs() < 1e-3, "Kopf-Unterkante {min_y}, erwartet 1.4072");
        assert!((max_y - 1.8759).abs() < 1e-3, "Kopf-Oberkante {max_y}, erwartet 1.8759");
    }

    // DER Test fuer die Blickhoehe: die Augenhoehe des Spielers (1.62, s.
    // app::physics::EYE_HEIGHT) muss auf der AUGENREIHE der Skin-Textur landen.
    //
    // Der Kopf ist 8 Modellpixel hoch, die Augen sitzen in der Standard-Textur rund 4.4 Pixel
    // unter dem Scheitel. Mit PLAYER_MODEL_SCALE liegt der Scheitel bei 1.876, also 0.256
    // Bloecke ueber der Augenhoehe -- das sind genau jene 4.4 Kopfpixel. OHNE den Faktor
    // stuende der Scheitel bei 2.001, der Abstand waere 0.381 Bloecke bzw. 6.1 Kopfpixel, und
    // man saehe einem gleich hohen Spieler bei Blickneigung 0 auf die Nase statt in die Augen.
    #[test]
    fn augenhoehe_trifft_die_augenreihe_des_skins() {
        const AUGENHOEHE: f32 = 1.62; // app::physics::EYE_HEIGHT, hier nicht erreichbar
        let model = crate::entity_model::player_model(false);
        let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
        let mut out = Vec::new();
        let head = model.parts.iter().find(|p| p.name == "head").expect("head fehlt");
        emit_part(
            &Part::new("head", head.pivot, head.cuboids.clone()),
            &Pose::new(),
            base,
            model.tex_size,
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            &mut out,
        );

        let unterkante = out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let oberkante = out.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
        assert!(
            AUGENHOEHE > unterkante && AUGENHOEHE < oberkante,
            "die Augenhoehe {AUGENHOEHE} muss ueberhaupt im Kopf liegen ({unterkante}..{oberkante})"
        );
        // In Kopfpixeln unter dem Scheitel gerechnet -- so ist die Zahl direkt mit der
        // Skin-Textur vergleichbar (Kopf = 8 Pixel).
        let pixel_unter_scheitel = (oberkante - AUGENHOEHE) / (oberkante - unterkante) * 8.0;
        assert!(
            (pixel_unter_scheitel - 4.4).abs() < 0.5,
            "Augen sollten ~4.4 Kopfpixel unter dem Scheitel liegen, sind {pixel_unter_scheitel}"
        );
    }

    // Die Fuesse muessen auf der Entity-Position stehen (y=0), nicht darueber schweben oder
    // im Boden stecken: Beine sind 12 Pixel lang mit Pivot y=12, also 1.501 - 24/16 = 0.001.
    //
    // Geprueft wird das BASISBEIN. Der Gesamt-Tiefstpunkt liegt bewusst etwas darunter: die
    // Hosen-Aussenlage ist um 0.25 Pixel dilatiert und ragt entsprechend unter den Fuss --
    // das ist Vanilla-Verhalten, keine Abweichung (der erste Anlauf dieses Tests hat genau
    // deshalb angeschlagen).
    #[test]
    fn fuesse_stehen_auf_der_entity_position() {
        let model = player_model(false);
        let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
        let leg = model.parts.iter().find(|p| p.name == "right_leg").expect("right_leg fehlt");

        let mut ohne_hose = Vec::new();
        emit_part(
            &Part::new("right_leg", leg.pivot, leg.cuboids.clone()),
            &Pose::new(),
            base,
            model.tex_size,
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            &mut ohne_hose,
        );
        let sohle = ohne_hose.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        assert!((sohle - 0.001).abs() < 1e-4, "Fusssohle {sohle}, erwartet 0.001");

        let mut alles = Vec::new();
        emit(&model, &Pose::new(), base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut alles);
        let tiefster = alles.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let hosen_dilation = 0.25 / 16.0;
        assert!(
            tiefster >= 0.001 - hosen_dilation - 1e-4,
            "tiefster Punkt {tiefster} darf hoechstens um die Hosen-Dilation unter die Sohle reichen"
        );
    }

    // Gierung: bei bodyYaw 0 schaut ein Entity in Vanilla nach SUEDEN (+Z). Die Nase des
    // Kopfes sitzt im Modellraum bei z=-4, das 180-Grad-Drehen von entity_transform muss sie
    // also auf +Z bringen.
    #[test]
    fn gierung_null_schaut_nach_sueden() {
        let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
        // Punkt vor dem Gesicht im Modellraum (Pixel), z=-4 ist die Vorderseite.
        let front = base.apply([0.0, 0.0, -4.0 / 16.0]);
        assert!(front[2] > 0.1, "Vorderseite muss bei Gierung 0 nach +Z zeigen, ist z={}", front[2]);
    }

    // Eine Drehung um 90 Grad muss die Vorderseite nach WESTEN (-X) bringen -- Vanillas
    // Gierung waechst im Uhrzeigersinn von Sueden aus.
    #[test]
    fn gierung_neunzig_schaut_nach_westen() {
        let base = entity_transform([0.0, 0.0, 0.0], 90.0, 1.0, PLAYER_MODEL_SCALE);
        let front = base.apply([0.0, 0.0, -4.0 / 16.0]);
        assert!(front[0] < -0.1, "Gierung 90 muss nach -X zeigen, ist x={}", front[0]);
    }

    // Der duenne Arm (Slim-Skin) ist 3 statt 4 Pixel breit -- sonst waere die Alex-Variante
    // nur eine andere Textur auf demselben Koerper.
    #[test]
    fn slim_hat_schmalere_arme_als_classic() {
        let breite = |slim: bool| {
            let model = player_model(slim);
            let arm = model.parts.iter().find(|p| p.name == "right_arm").expect("right_arm fehlt");
            arm.cuboids[0].size[0]
        };
        assert_eq!(breite(false), 4.0);
        assert_eq!(breite(true), 3.0);
    }

    // Die Aussenlagen (Hut, Jacke, Aermel, Hosen) sind dilatierte Kopien -- ohne Dilation
    // laegen sie exakt auf der Basisschicht und wuerden z-fighten.
    #[test]
    fn aussenlagen_sind_dilatiert() {
        let model = player_model(false);
        let head = model.parts.iter().find(|p| p.name == "head").unwrap();
        let hat = head.children.iter().find(|p| p.name == "hat").expect("hat fehlt");
        assert_eq!(hat.cuboids[0].dilation, [0.5; 3], "Hut nutzt Vanillas Dilation 0.5");
        let body = model.parts.iter().find(|p| p.name == "body").unwrap();
        let jacket = body.children.iter().find(|p| p.name == "jacket").expect("jacket fehlt");
        assert_eq!(jacket.cuboids[0].dilation, [0.25; 3]);
    }

    // Schwimmen legt die Figur waagerecht. Pruefung: ohne Neigung ist der Koerper hoeher als
    // lang, mit voller Neigung laenger als hoch.
    #[test]
    fn schwimmen_legt_den_koerper_waagerecht() {
        let model = player_model(false);
        let masse = |lean: &Lean| {
            let base = entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, lean);
            let mut out = Vec::new();
            emit(&model, &Pose::new(), base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
            let hoehe =
                out.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max) - out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let laenge =
                out.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max) - out.iter().map(|v| v.position[2]).fold(f32::MAX, f32::min);
            (hoehe, laenge)
        };
        let (h_stehend, l_stehend) = masse(&Lean::NONE);
        assert!(h_stehend > l_stehend, "stehend muss hoeher als lang sein ({h_stehend} / {l_stehend})");

        let schwimmend = Lean { amount: 1.0, pitch_deg: 0.0, in_water: true, swimming: true };
        let (h_schwimmend, l_schwimmend) = masse(&schwimmend);
        assert!(l_schwimmend > h_schwimmend, "schwimmend muss laenger als hoch sein ({h_schwimmend} / {l_schwimmend})");
        assert!(l_schwimmend > l_stehend * 2.0, "die Figur muss sich wirklich strecken");
    }

    // Die Neigung muss GLEITEND greifen -- Vanilla laeuft mit 0.09 pro Tick hoch. Bei halbem
    // Wert muss die Figur genau dazwischen liegen, sonst klappt sie in einem Frame um.
    #[test]
    fn neigung_greift_gleitend() {
        let winkel = |amount: f32| {
            let lean = Lean { amount, pitch_deg: 0.0, in_water: true, swimming: false };
            let base = entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean);
            // Ein Punkt ueber dem Kopf im Modellraum: je waagerechter der Koerper, desto
            // weiter wandert er nach vorn und desto weniger nach oben.
            base.apply([0.0, -24.0 / 16.0, 0.0])
        };
        let aufrecht = winkel(0.0);
        let halb = winkel(0.5);
        let voll = winkel(1.0);
        assert!(aufrecht[1] > halb[1] && halb[1] > voll[1], "die Hoehe muss monoton fallen");
        assert!(voll[1].abs() < 0.05, "bei voller Neigung liegt der Kopf praktisch auf Fusshoehe");
    }

    // Im Wasser richtet sich der Koerper nach der Blickneigung, beim Kriechen an Land NICHT
    // (dort liegt er stur waagerecht) -- genau die Unterscheidung, die Vanilla ueber
    // isTouchingWater macht.
    #[test]
    fn blickneigung_wirkt_nur_im_wasser() {
        let punkt = |in_water: bool, pitch: f32| {
            let lean = Lean { amount: 1.0, pitch_deg: pitch, in_water, swimming: false };
            entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean).apply([0.0, -24.0 / 16.0, 0.0])
        };
        // Im Wasser: nach unten schauen (positiver Vanilla-Pitch) aendert die Haltung.
        assert!((punkt(true, 0.0)[1] - punkt(true, 40.0)[1]).abs() > 0.1, "im Wasser muss der Blick die Haltung drehen");
        // An Land: die Blickneigung darf nichts aendern.
        let a = punkt(false, 0.0);
        let b = punkt(false, 40.0);
        assert!((a[1] - b[1]).abs() < 1e-5 && (a[2] - b[2]).abs() < 1e-5, "beim Kriechen liegt der Koerper unabhaengig vom Blick");
    }

    // Der Kopf muss beim Schwimmen Richtung -45 Grad gezogen werden, sonst schaut die Figur
    // bei waagerechtem Koerper in den Boden.
    #[test]
    fn kopf_wird_beim_schwimmen_angehoben() {
        let ohne = biped_pose_leaning(0.0, 0.0, 0.0, 0.0, false, 0.0, 0.0).get("head").rotation[0];
        let voll = biped_pose_leaning(0.0, 0.0, 0.0, 0.0, false, 1.0, 0.0).get("head").rotation[0];
        assert_eq!(ohne, 0.0);
        assert!((voll + std::f32::consts::FRAC_PI_4).abs() < 1e-5, "bei voller Neigung genau -45 Grad, ist {voll}");
        let halb = biped_pose_leaning(0.0, 0.0, 0.0, 0.0, false, 0.5, 0.0).get("head").rotation[0];
        assert!(halb < 0.0 && halb > voll, "die Haelfte muss dazwischen liegen ({halb})");
    }

    // Sprint-Schwimmen verschiebt den Koerper zusaetzlich (Vanilla: translate(0,-1,0.3)) --
    // reines Kriechen nicht.
    #[test]
    fn sprintschwimmen_verschiebt_zusaetzlich() {
        let punkt = |swimming: bool| {
            let lean = Lean { amount: 1.0, pitch_deg: 0.0, in_water: true, swimming };
            entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean).apply([0.0, 0.0, 0.0])
        };
        assert_ne!(punkt(true), punkt(false), "isSwimming muss einen Versatz erzeugen");
    }

    // Ein 8x8x8-Kasten bei UV (0,0) auf einer 64x64-Textur: die Nordseite (das Gesicht) muss
    // auf dem Ausschnitt u 8..16, v 8..16 liegen -- genau dort sitzt das Gesicht in jedem
    // Minecraft-Skin.
    #[test]
    fn kopf_uv_trifft_das_gesichtsfeld() {
        let cuboid = Cuboid::new([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]);
        let mut out = Vec::new();
        emit_cuboid(&cuboid, Affine::IDENTITY, [64.0, 64.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], &mut out);
        // Nordseite ist die vierte der sechs Seiten, je 6 Vertices.
        let north = &out[18..24];
        let us: Vec<f32> = north.iter().map(|v| v.uv[0] * 64.0).collect();
        let vs: Vec<f32> = north.iter().map(|v| v.uv[1] * 64.0).collect();
        assert_eq!(us.iter().cloned().fold(f32::MAX, f32::min), 8.0);
        assert_eq!(us.iter().cloned().fold(f32::MIN, f32::max), 16.0);
        assert_eq!(vs.iter().cloned().fold(f32::MAX, f32::min), 8.0);
        assert_eq!(vs.iter().cloned().fold(f32::MIN, f32::max), 16.0);
    }

    // Jede Seite jedes Kastens muss zwei Dreiecke liefern -- ein fehlendes Face ist im Spiel
    // ein Loch, durch das man ins Modell sieht.
    #[test]
    fn jeder_kasten_liefert_sechs_vollstaendige_seiten() {
        for slim in [false, true] {
            let model = player_model(slim);
            let mut kaesten = 0;
            fn zaehle(part: &Part, kaesten: &mut usize) {
                *kaesten += part.cuboids.len();
                for child in &part.children {
                    zaehle(child, kaesten);
                }
            }
            for part in &model.parts {
                zaehle(part, &mut kaesten);
            }
            let mut out = Vec::new();
            emit(&model, &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
            assert_eq!(out.len(), kaesten * 36, "slim={slim}: 6 Seiten * 2 Dreiecke * 3 Vertices pro Kasten");
        }
    }

    // Verkettung: erst drehen, dann verschieben ist NICHT dasselbe wie umgekehrt. Ohne
    // korrektes then() sitzen Gliedmassen an der falschen Stelle, sobald sie gedreht sind.
    #[test]
    fn verkettung_haelt_die_reihenfolge_ein() {
        let drehen = Affine::rotation_y_deg(90.0);
        let schieben = Affine::translation([1.0, 0.0, 0.0]);
        // erst schieben, dann drehen
        let a = drehen.then(schieben).apply([0.0, 0.0, 0.0]);
        // erst drehen, dann schieben
        let b = schieben.then(drehen).apply([0.0, 0.0, 0.0]);
        assert!((a[0]).abs() < 1e-5 && (a[2] + 1.0).abs() < 1e-5, "erst schieben, dann drehen: {a:?}");
        assert!((b[0] - 1.0).abs() < 1e-5 && b[2].abs() < 1e-5, "erst drehen, dann schieben: {b:?}");
    }

    // Eine Pose-Drehung muss auf die GRUNDdrehung addieren, nicht sie ersetzen -- sonst
    // verliert z.B. der Kuh-Koerper seine feste 90-Grad-Kippung, sobald er animiert wird.
    #[test]
    fn pose_addiert_auf_die_grunddrehung() {
        let part = Part::new("test", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])]).rotated([
            std::f32::consts::FRAC_PI_2,
            0.0,
            0.0,
        ]);
        let mut pose = Pose::new();
        pose.rotate("test", [std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
        let mut mit = Vec::new();
        emit_part(&part, &pose, Affine::IDENTITY, [64.0, 64.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], &mut mit);
        let mut ohne = Vec::new();
        emit_part(&part, &Pose::new(), Affine::IDENTITY, [64.0, 64.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], &mut ohne);
        // 90 + 90 Grad um X: der Punkt (0,0,1/16) landet bei -1/16 in Z, nicht bei +1/16 in Y.
        let punkt_mit = mit.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
        let punkt_ohne = ohne.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
        assert!(punkt_mit < punkt_ohne, "zwei mal 90 Grad muss weiter drehen als einmal");
    }
}

// ---------------------------------------------------------------------------------------
// Modelldaten. Ab hier reine Uebersetzung der Vanilla-Modellklassen -- Zeilen in Vanilla
// stehen jeweils im Kommentar.
// ---------------------------------------------------------------------------------------

// BipedEntityModel.getModelData(dilation, pivotOffsetY). `pivot_offset_y` verschiebt ALLE
// Pivots (der Enderman nutzt -14, weil sein Koerper viel hoeher sitzt).
fn biped_parts(dilation: f32, pivot_offset_y: f32) -> Vec<Part> {
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
pub fn default_skin(uuid: &[u8; 16]) -> (&'static str, bool) {
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
pub fn player_arm_model(slim: bool, right: bool) -> Model {
    let full = player_model(slim);
    let name = if right { "right_arm" } else { "left_arm" };
    let part = full.parts.into_iter().find(|p| p.name == name).expect("Arm im Spielermodell");
    Model { texture: full.texture, tex_size: full.tex_size, parts: vec![part] }
}

// Die Haltung des Arms in der Ego-Ansicht: PlayerEntityRenderer.renderArm setzt den Arm auf
// seine Grundstellung zurueck und gibt ihm nur ein leichtes Rollen nach aussen
// (rightArm.roll = 0.1, leftArm.roll = -0.1).
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
    pub fn progress(f: f32, von: f32, bis: f32) -> f32 {
        if bis == von {
            return if f >= bis { 1.0 } else { 0.0 };
        }
        ((f - von) / (bis - von)).clamp(0.0, 1.0)
    }
    pub fn in_quad(t: f32) -> f32 {
        t * t
    }
    pub fn in_out_sine(t: f32) -> f32 {
        -((std::f32::consts::PI * t).cos() - 1.0) / 2.0
    }
    pub fn out_back(t: f32) -> f32 {
        1.0 + 2.70158 * (t - 1.0).powi(3) + 1.70158 * (t - 1.0).powi(2)
    }
    pub fn out_quart(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(4)
    }
    pub fn out_cubic(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(3)
    }
    pub fn in_out_expo(t: f32) -> f32 {
        if t < 0.5 {
            if t == 0.0 {
                0.0
            } else {
                (2f64.powf(20.0 * t as f64 - 10.0) / 2.0) as f32
            }
        } else if t == 1.0 {
            1.0
        } else {
            ((2.0 - 2f64.powf(-20.0 * t as f64 + 10.0)) / 2.0) as f32
        }
    }
    pub fn out_circ(t: f32) -> f32 {
        (1.0 - (t - 1.0) * (t - 1.0)).max(0.0).sqrt()
    }
    pub fn in_circ(t: f32) -> f32 {
        1.0 - (1.0 - t * t).max(0.0).sqrt()
    }
    pub fn in_out_elastic(t: f32) -> f32 {
        if t == 0.0 {
            return 0.0;
        }
        if t == 1.0 {
            return 1.0;
        }
        let t = t as f64;
        let d = ((20.0 * t - 11.125) * std::f32::consts::PI as f64 * 4.0 / 9.0).sin();
        if t < 0.5 {
            (-(2f64.powf(20.0 * t - 10.0) * d) / 2.0) as f32
        } else {
            (2f64.powf(-20.0 * t + 10.0) * d / 2.0 + 1.0) as f32
        }
    }
    pub fn in_out_back(t: f32) -> f32 {
        if t < 0.5 {
            return 4.0 * t * t * (7.189819 * t - 2.5949094) / 2.0;
        }
        let h = 2.0 * t - 2.0;
        (h * h * (3.5949094 * h + 2.5949094) + 2.0) / 2.0
    }
}

// Drehung um einen Punkt: Vanillas MatrixStack.multiply(quaternion, ox, oy, oz).
fn um_punkt(drehung: Affine, o: [f32; 3]) -> Affine {
    Affine::translation(o).then(drehung).then(Affine::translation([-o[0], -o[1], -o[2]]))
}

// Speerstich in der Ego-Ansicht (HeldItemRenderer, SwingAnimationType.STAB ->
// Lancing.method_75391) statt des normalen Schwungs:
//   g = inOutSine(f in 0..0.05), h = outBack(f in 0.05..0.2), j = inOutExpo(f in 0.4..1)
//   translate(i*0.1*(g-h), -0.075*(g-j), 0.65*(g-h)); rotateX(-70*(g-j)); translate(0,0,-0.25*(j-h))
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpearKinetic {
    pub delay: i32,
    pub dismount: i32,
    pub knockback: i32,
    pub damage: i32,
}

// Lancing.class_12153.method_75397: alle Teilfortschritte des Ansturms aus der Benutzungsdauer.
#[derive(Clone, Copy, Debug)]
struct LanceState {
    raise: f32,
    raise_start: f32,
    raise_middle: f32,
    raise_end: f32,
    sway: f32,
    lower: f32,
    raise_back: f32,
    sway_scale_slow: f32,
    sway_scale_fast: f32,
}

fn lance_state(k: SpearKinetic, f: f32) -> LanceState {
    use easing::*;
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

fn pull_zoom(right: bool, pull: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    Affine::translation([0.0, 0.0, pull * 0.04]).then(Affine::scale([1.0, 1.0, 1.0 + pull * 0.2])).then(Affine::rotation_y_deg(-i * 45.0))
}

// Anlege-Verschiebung allein (applyEquipOffset), ohne den Schwung.
pub fn first_person_equip_offset(right: bool, equip_progress: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    Affine::translation([i * 0.56, -0.52 + equip_progress * -0.6, -0.72])
}

// UseAction.BOW.
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
pub fn first_person_eat_pose(right: bool, equip_progress: f32, f: f32, max_use: f32) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    // Oben gekappt: im allerersten Bild liegt f noch einen Tick ueber der Dauer, und die
    // 27. Potenz wuerde daraus einen Sprung machen.
    let g = (f / max_use.max(1.0)).clamp(0.0, 1.0);
    let wippen = if g < 0.8 { ((f / 4.0 * std::f32::consts::PI).cos() * 0.1).abs() } else { 0.0 };
    let h = 1.0 - g.powi(27);
    Affine::translation([0.0, wippen, 0.0])
        .then(Affine::translation([h * 0.6 * i, h * -0.5, 0.0]))
        .then(Affine::rotation_y_deg(i * h * 90.0))
        .then(rotation_x_deg(h * 10.0))
        .then(rotation_z_deg(i * h * 30.0))
        .then(first_person_equip_offset(right, equip_progress))
}

// Die LADENDE Armbrust (eigener Zweig in Vanilla, andere Zahlen als der Bogen).
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
pub fn first_person_crossbow_charged_offset(right: bool) -> Affine {
    let i = if right { 1.0 } else { -1.0 };
    Affine::translation([i * -0.641864, 0.0, 0.0]).then(Affine::rotation_y_deg(i * 10.0))
}

// // Zombies, Skelette & Co: das nackte Biped mit eigener Textur. `tex_size` ist bei diesen
// Modellen 64x32 (die alte Skin-Aufteilung ohne zweite Lage).
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
pub fn shulker_box_pose(openness: f32) -> Pose {
    let mut pose = Pose::new();
    pose.set("lid", PartPose { rotation: [0.0, (270.0 * openness).to_radians(), 0.0], pivot_offset: [0.0, -openness * 0.5 * 16.0, 0.0], rotation_matrix: None, hidden: false });
    pose
}

// ShulkerBoxBlockEntityRenderer.setTransforms:
//   translate(0.5, 0.5, 0.5); scale(0.9995); multiply(facing.getRotationQuaternion());
//   scale(1, -1, -1); translate(0, -1, 0)
// Die 0.9995 halten die Kiste minimal innerhalb des Blocks (kein Z-Fighting mit Nachbarn).
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
pub fn chest_pose(openness: f32) -> Pose {
    let f = 1.0 - (1.0 - openness).powi(3);
    let winkel = -(f * std::f32::consts::FRAC_PI_2);
    let mut pose = Pose::new();
    pose.rotate("lid", [winkel, 0.0, 0.0]);
    pose.rotate("lock", [winkel, 0.0, 0.0]);
    pose
}

// translate(0.5, 0.5, 0.5); rotateY(-yaw); translate(-0.5, -0.5, -0.5) -- ab der Blockecke.
pub fn chest_transform(block: [f32; 3], yaw_deg: f32) -> Affine {
    Affine::translation([block[0] + 0.5, block[1] + 0.5, block[2] + 0.5])
        .then(Affine::rotation_y_deg(-yaw_deg))
        .then(Affine::translation([-0.5, -0.5, -0.5]))
}

// Platzhalter: die tatsaechliche Textur haengt an der Bettfarbe und wird erst beim Zeichnen
// gewaehlt (emit bekommt das Atlas-Rechteck uebergeben, s. block_entity::BedInstance).
const BED_MODEL_TEXTURE: &str = "entity/bed/red";

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

// SlimeEntityModel: Aussenhuelle (8er-Kasten) plus Innenkern mit Augen und Mund.
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
fn quadruped_parts(stance: f32, head: Part, body: Part) -> Vec<Part> {
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
    last_speed: f32,
    speed: f32,
    animation_progress: f32,
    time_scale: f32,
}

impl LimbAnimator {
    // Vanillas Aenderungsrate aus LivingEntity.updateLimbs.
    pub const RATE: f32 = 0.4;

    pub fn new() -> Self {
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

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    // Amplitude, zwischen den beiden letzten Ticks interpoliert (LimbAnimator.getAmplitude).
    pub fn amplitude(&self, tick_progress: f32) -> f32 {
        (self.last_speed + (self.speed - self.last_speed) * tick_progress).min(1.0)
    }

    // Phase, um den noch nicht vergangenen Teil des laufenden Ticks zurueckgerechnet
    // (LimbAnimator.getAnimationProgress) -- ohne das springt die Schwingung 20 mal pro
    // Sekunde weiter statt zu gleiten.
    pub fn animation_progress(&self, tick_progress: f32) -> f32 {
        (self.animation_progress - self.speed * (1.0 - tick_progress)) * self.time_scale
    }
}

// MathHelper.wrapDegrees: einen Winkel auf -180..180 bringen.
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
    yaw: f32,
    last_yaw: f32,
    head_yaw: f32,
    last_head_yaw: f32,
    // Schild oben (PlayerEntity.isBlocking): der Kopf darf dann nur 15 statt 50 Grad verdreht sein.
    blocking: bool,
}

// LivingEntity.tick (Zielwahl) + turnHead, als reine Funktion -- fuer den eigenen Spieler
// (BodyYaw) UND fremde Lebewesen (world::Entity) dieselbe Rechnung. Liefert die neue
// Koerperdrehung. `look_yaw_deg` ist getYaw(), `dx`/`dz` die Strecke dieses Ticks,
// `max_relative` getMaxRelativeHeadRotation (50, beim blockenden Spieler 15).
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

    pub fn new(yaw: f32) -> Self {
        Self { yaw, last_yaw: yaw, head_yaw: yaw, last_head_yaw: yaw, blocking: false }
    }

    pub fn set_blocking(&mut self, blocking: bool) {
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

    // Den Koerper von aussen ausrichten (nach tick): ein Boot oder Reittier dreht seinen
    // Mitfahrer mit (AbstractBoatEntity.clampPassengerYaw, AbstractHorseEntity.
    // updatePassengerPosition setzen dessen bodyYaw).
    pub fn set_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
    }

    // Die interpolierte Koerperdrehung -- damit wird das ganze Modell gedreht
    // (LivingEntityRenderer.setupTransforms).
    pub fn body_yaw(&self, tick_progress: f32) -> f32 {
        lerp_angle_degrees(tick_progress, self.last_yaw, self.yaw)
    }

    // Die Kopfdrehung RELATIV zum Koerper -- das ist der Wert, den setAngles fuer den Kopf
    // bekommt (LivingEntityRenderer.updateRenderState).
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
    x: f64,
    y: f64,
    z: f64,
    last_x: f64,
    last_y: f64,
    last_z: f64,
    movement: f32,
    last_movement: f32,
    distance_moved: f32,
    last_distance_moved: f32,
}

impl CapeState {
    // Wie schnell die nachhinkende Position aufholt (ClientPlayerLikeState.setPos).
    const FOLLOW: f64 = 0.25;
    // Ab dieser Differenz wird hart gesetzt statt nachgezogen -- sonst wuerde der Umhang
    // nach einem Teleport quer durch die Welt gezogen.
    const SNAP: f64 = 10.0;
    // Aenderungsrate der Laufgroesse (ClientPlayerLikeState.tickMovement) -- dieselbe 0.4
    // wie beim LimbAnimator, aber ein eigener Zustand.
    const MOVEMENT_RATE: f32 = 0.4;

    pub fn new() -> Self {
        Self::default()
    }

    // ClientPlayerLikeState.tick -> setPos. `pos` ist die Spielerposition zu Tickbeginn.
    pub fn tick(&mut self, pos: [f64; 3]) {
        self.last_distance_moved = self.distance_moved;
        let follow = |current: &mut f64, last: &mut f64, target: f64| {
            *last = *current;
            let delta = target - *current;
            if !(-Self::SNAP..=Self::SNAP).contains(&delta) {
                *current = target;
                *last = *current;
            } else {
                *current += delta * Self::FOLLOW;
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
    pub fn lerp_pos(&self, tick_progress: f32) -> [f64; 3] {
        let t = tick_progress as f64;
        [self.last_x + (self.x - self.last_x) * t, self.last_y + (self.y - self.last_y) * t, self.last_z + (self.z - self.last_z) * t]
    }

    pub fn lerp_movement(&self, tick_progress: f32) -> f32 {
        self.last_movement + (self.movement - self.last_movement) * tick_progress
    }

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
pub fn cape_angles(state: &CapeState, pos: [f64; 3], last_pos: [f64; 3], body_yaw_deg: f32, tick_progress: f32) -> (f32, f32, f32) {
    let t = tick_progress as f64;
    let lerped = state.lerp_pos(tick_progress);
    let d = lerped[0] - (last_pos[0] + (pos[0] - last_pos[0]) * t);
    let e = lerped[1] - (last_pos[1] + (pos[1] - last_pos[1]) * t);
    let f = lerped[2] - (last_pos[2] + (pos[2] - last_pos[2]) * t);
    let h = (body_yaw_deg.to_radians().sin()) as f64;
    let i = -(body_yaw_deg.to_radians().cos()) as f64;

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
pub fn cat_texture_for(registry_name: &str) -> Option<&'static str> {
    let name = registry_name.strip_prefix("minecraft:").unwrap_or(registry_name);
    CAT_TEXTURES.iter().copied().find(|t| t.strip_prefix("entity/cat/") == Some(name))
}

// Zusaetzliche Modellskalierung je Typ (MeshTransformer.scaling): die Katze ist ein auf 0.8
// verkleinerter Ozelot. Skaliert wird um die Fuesse, genau wie der Spielerfaktor.
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
pub fn end_crystal_y_offset(age: f32) -> f32 {
    let g = (age * 0.2).sin() / 2.0 + 0.5;
    (g * g + g) * 0.4 - 1.4
}

// EndCrystalEntityModel.setAngles: der Kristall steigt und sinkt (outer_glass.originY) und
// die drei Wuerfel drehen sich um eine schraege Achse. `age` in Ticks
// (EndCrystalEntity.endCrystalAge + tickProgress).
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
pub fn wind_charge_pose(age: f32) -> Pose {
    let mut pose = Pose::new();
    pose.set("wind", PartPose { rotation_matrix: Some(Affine::rotation_y_deg(age * 16.0)), ..Default::default() });
    pose.set("wind_charge", PartPose { rotation_matrix: Some(Affine::rotation_y_deg(-age * 16.0)), ..Default::default() });
    pose
}

// EndCrystalEntityRenderer.render: scale(2) und translate(0, -0.5, 0) um die Entity-Position.
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
pub fn arrow_transform(feet: [f32; 3], yaw_deg: f32, pitch_deg: f32) -> Affine {
    Affine::translation(feet).then(Affine::rotation_y_deg(yaw_deg - 90.0)).then(rotation_z_deg(pitch_deg))
}

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
fn apply_arm_sway(pose: &mut Pose, age: f32) {
    for (part, sigma) in [("right_arm", 1.0f32), ("left_arm", -1.0f32)] {
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
fn wrap_radians(rad: f32) -> f32 {
    let mut r = rad % std::f32::consts::TAU;
    if r >= std::f32::consts::PI {
        r -= std::f32::consts::TAU;
    }
    if r < -std::f32::consts::PI {
        r += std::f32::consts::TAU;
    }
    r
}

fn lerp_angle_radians(delta: f32, from: f32, to: f32) -> f32 {
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
fn apply_swim_stroke(pose: &mut Pose, lean_left: f32, lean_right: f32, swing_progress: f32) {
    if lean_left <= 0.0 && lean_right <= 0.0 {
        return;
    }
    const ROLL_SPAN: f32 = 1.8707964;
    const LEFT_ROLL_MID: f32 = 5.012389;
    const RIGHT_ROLL_MID: f32 = 1.2707963;
    // BipedEntityModel.method_2807 -- die Zug-Kurve.
    let ease = |x: f32| -65.0 * x + x * x;

    let phase = swing_progress.rem_euclid(26.0);
    let pi = std::f32::consts::PI;
    let (left_pitch, right_pitch, left_roll, right_roll) = if phase < 14.0 {
        (0.0, 0.0, pi + ROLL_SPAN * ease(phase) / ease(14.0), pi - ROLL_SPAN * ease(phase) / ease(14.0))
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
    for (part, phase_offset) in [("left_leg", pi), ("right_leg", 0.0)] {
        let current = pose.get(part).rotation;
        let target = 0.3 * (swing_progress * 0.333_333_34 + phase_offset).cos();
        pose.rotate(part, [current[0] + (target - current[0]) * lean, current[1], current[2]]);
    }
}

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

// Wie biped_pose, plus die Kopfhaltung beim Schwimmen: Vanilla zieht die Kopfneigung mit
// `lean` Richtung -45 Grad (BipedEntityModel.setAngles, Zweig f > 0). Ohne das schaut die
// Figur beim Schwimmen in den Boden, weil der ganze Koerper waagerecht liegt, der Kopf aber
// weiter der Blickneigung folgt.
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
    let flach = glide.is_some() || lean > 0.0;
    if block_right {
        apply_blocking_arm_pose(&mut pose, true);
    } else if spear_right {
        apply_spear_arm_pose(&mut pose, true, flach);
    } else if item_right {
        apply_item_arm_pose(&mut pose, true);
    }
    if block_left {
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
    apply_arm_sway(&mut pose, age);
    // Zuletzt die Schwimmbewegung -- in Vanilla steht dieser Zweig am Ende von setAngles,
    // nach den swingArm-Aufrufen. Die Reihenfolge zaehlt: er blendet mit `lean` von dem weg,
    // was vorher berechnet wurde (Laufanimation und Ruhe-Bewegung), statt es zu ersetzen.
    // Der schlagende Arm bleibt vom Schwimmzug unberuehrt (s. apply_swim_stroke).
    // Ein Arm mit Speerhaltung schwimmt ebenfalls nicht mit (Vanilla: `armPose != SPEAR`).
    let schlaegt = hand_swing > 0.0;
    let lean_links = if (schlaegt && !swing_right_arm) || spear_left { 0.0 } else { lean };
    let lean_rechts = if (schlaegt && swing_right_arm) || spear_right { 0.0 } else { lean };
    apply_swim_stroke(&mut pose, lean_links, lean_rechts, swing_progress);
    pose
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

// QuadrupedEntityModel.setAngles: Kopf wie beim Biped, die vier Beine schwingen paarweise
// gegenlaeufig (Diagonalgang).
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
pub fn creeper_pose(head_yaw_rel: f32, head_pitch: f32, swing_progress: f32, swing_amount: f32) -> Pose {
    quadruped_pose(head_yaw_rel, head_pitch, swing_progress, swing_amount)
}

// SpiderEntityModel.setAngles: der Kopf dreht mit, die acht Beine schwingen in vier
// gegenlaeufigen Paaren um ihre Grunddrehung (die bleibt erhalten, s. Pose-Semantik).
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

#[cfg(test)]
mod limb_tests {
    use super::*;

    // Der Kern: Sprinten muss die Schrittfrequenz ERHOEHEN. Gefuettert
    // mit Vanillas echten Strecken pro Tick (s. physics.rs::strecke_pro_tick_trifft_vanilla).
    const GEHEN: f32 = 0.2159;
    const SPRINTEN: f32 = 0.2806;

    fn eingelaufen(moved: f32) -> LimbAnimator {
        let mut a = LimbAnimator::new();
        // Genug Ticks, damit speed den Zielwert praktisch erreicht hat (Rate 0.4).
        for _ in 0..40 {
            a.tick(moved, 1.0);
        }
        a
    }

    #[test]
    fn sprinten_beschleunigt_die_schrittfrequenz() {
        let gehen = eingelaufen(GEHEN);
        let sprinten = eingelaufen(SPRINTEN);
        // Frequenz = Phasenzuwachs pro Tick.
        let f_gehen = gehen.animation_progress(1.0) - gehen.animation_progress(0.0);
        let f_sprinten = sprinten.animation_progress(1.0) - sprinten.animation_progress(0.0);
        assert!(f_sprinten > f_gehen, "Sprinten muss schneller schwingen ({f_sprinten} vs {f_gehen})");
        // Vanillas Verhaeltnis: 1.0 / 0.8636 = 1.158.
        let verhaeltnis = f_sprinten / f_gehen;
        assert!((verhaeltnis - 1.158).abs() < 0.01, "Verhaeltnis {verhaeltnis:.3}, erwartet 1.158");
    }

    #[test]
    fn amplitude_deckelt_bei_eins() {
        assert!((eingelaufen(GEHEN).amplitude(1.0) - 0.8636).abs() < 0.002);
        // Exakt 1.0 wird per Annaeherung nie ganz erreicht (Rate 0.4 halbiert den Rest je
        // Tick) -- geprueft wird deshalb "praktisch am Deckel", nicht Gleichheit.
        assert!(eingelaufen(SPRINTEN).amplitude(1.0) > 0.9999, "sprintend ist die Amplitude ausgereizt");
        // Auch bei absurd hoher Geschwindigkeit (Elytra, Knockback) nie UEBER 1.
        assert!(eingelaufen(5.0).amplitude(1.0) <= 1.0);
        assert!(eingelaufen(5.0).amplitude(1.0) > 0.9999);
    }

    // Der Fehler, der zum Bug fuehrte: die gedaempfte GESCHWINDIGKEIT statt der Strecke. Auf
    // Boden daempft Vanilla mit 0.6 * 0.91 = 0.546 -- damit landet man weit unter dem Deckel
    // und der Unterschied zwischen Gehen und Sprinten verschwindet in einer Mini-Bewegung.
    #[test]
    fn gedaempfte_geschwindigkeit_waere_viel_zu_klein() {
        let daempfung = 0.6 * 0.91;
        let falsch = eingelaufen(GEHEN * daempfung).amplitude(1.0);
        let richtig = eingelaufen(GEHEN).amplitude(1.0);
        assert!(falsch < richtig * 0.6, "die falsche Groesse war grob halb so gross ({falsch} vs {richtig})");
    }

    // Stehenbleiben muss die Schwingung auslaufen lassen, nicht abrupt stoppen.
    #[test]
    fn stehenbleiben_laeuft_aus() {
        let mut a = eingelaufen(SPRINTEN);
        let voll = a.amplitude(1.0);
        a.tick(0.0, 1.0);
        let nach_einem_tick = a.amplitude(1.0);
        assert!(nach_einem_tick < voll && nach_einem_tick > 0.0, "muss abklingen, nicht springen ({nach_einem_tick})");
        for _ in 0..40 {
            a.tick(0.0, 1.0);
        }
        assert!(a.amplitude(1.0) < 1e-4, "nach genug Ticks steht sie still");
    }

    // Die Phase muss zwischen zwei Ticks GLEITEN -- ohne die Rueckrechnung springt sie 20 mal
    // pro Sekunde weiter, was bei hoher Framerate als Zucken sichtbar ist.
    #[test]
    fn phase_gleitet_zwischen_den_ticks() {
        let a = eingelaufen(SPRINTEN);
        let anfang = a.animation_progress(0.0);
        let mitte = a.animation_progress(0.5);
        let ende = a.animation_progress(1.0);
        assert!(anfang < mitte && mitte < ende, "monoton ueber den Tick ({anfang} / {mitte} / {ende})");
        assert!(((mitte - anfang) - (ende - mitte)).abs() < 1e-5, "gleichmaessig, nicht sprunghaft");
    }

    // Jungtiere trippeln dreimal so schnell (Vanillas timeScale) -- dieselbe Strecke, dreifache
    // Phase.
    #[test]
    fn jungtiere_trippeln_dreifach() {
        let mut erwachsen = LimbAnimator::new();
        let mut jung = LimbAnimator::new();
        for _ in 0..40 {
            erwachsen.tick(GEHEN, 1.0);
            jung.tick(GEHEN, 3.0);
        }
        assert!((jung.animation_progress(1.0) / erwachsen.animation_progress(1.0) - 3.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod pose_tests {
    use super::*;

    // Ohne Bewegung darf nichts schwingen -- sonst zappeln stehende Mobs.
    #[test]
    fn ohne_bewegung_keine_schwingung() {
        let pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        for part in ["right_arm", "left_arm"] {
            assert_eq!(pose.get(part).rotation[0], 0.0, "{part} darf stillstehen");
        }
    }

    // Arme und Beine muessen GEGENLAEUFIG schwingen (rechter Arm mit linkem Bein) -- laufen
    // sie gleichphasig, sieht die Figur aus wie ein huepfender Zinnsoldat.
    #[test]
    fn arme_und_beine_schwingen_gegenlaeufig() {
        let pose = biped_pose(0.0, 0.0, 1.0, 1.0, false, 0.0);
        let ra = pose.get("right_arm").rotation[0];
        let la = pose.get("left_arm").rotation[0];
        let rl = pose.get("right_leg").rotation[0];
        assert!(ra * la < 0.0, "die beiden Arme muessen entgegengesetzt schwingen ({ra} / {la})");
        assert!(ra * rl < 0.0, "rechter Arm und rechtes Bein muessen entgegengesetzt schwingen ({ra} / {rl})");
    }

    // Die Amplitude muss durchskalieren: halbe Geschwindigkeit, halber Ausschlag.
    #[test]
    fn amplitude_skaliert_den_ausschlag() {
        let voll = biped_pose(0.0, 0.0, 2.0, 1.0, false, 0.0).get("right_leg").rotation[0];
        let halb = biped_pose(0.0, 0.0, 2.0, 0.5, false, 0.0).get("right_leg").rotation[0];
        assert!((voll / 2.0 - halb).abs() < 1e-6, "halbe Amplitude = halber Ausschlag ({voll} / {halb})");
    }

    // Kopfwinkel kommen in GRAD herein und muessen als Radiant herauskommen.
    #[test]
    fn kopfwinkel_werden_nach_radiant_umgerechnet() {
        let pose = biped_pose(90.0, -45.0, 0.0, 0.0, false, 0.0);
        let r = pose.get("head").rotation;
        assert!((r[1] - std::f32::consts::FRAC_PI_2).abs() < 1e-5, "90 Grad Gierung = PI/2, ist {}", r[1]);
        assert!((r[0] + std::f32::consts::FRAC_PI_4).abs() < 1e-5, "-45 Grad Neigung = -PI/4, ist {}", r[0]);
    }

    // Schleichen kippt den Rumpf und hebt den Kopf-Pivot -- ohne das steckt der Kopf beim
    // Schleichen im Boden statt sich zu senken.
    #[test]
    fn schleichen_kippt_den_rumpf_und_verschiebt_pivots() {
        let pose = biped_pose(0.0, 0.0, 0.0, 0.0, true, 0.0);
        assert_eq!(pose.get("body").rotation[0], 0.5);
        assert_eq!(pose.get("body").pivot_offset[1], 3.2);
        assert_eq!(pose.get("head").pivot_offset[1], 4.2);
        assert_eq!(pose.get("right_leg").pivot_offset[2], 4.0);
        // pivotY 12.0 -> 12.2 (s. Schleich-Zweig): die Beine senken sich mit.
        assert_eq!(pose.get("right_leg").pivot_offset[1], 0.2);
        assert_eq!(pose.get("left_leg").pivot_offset[1], 0.2);
        // Die Arme behalten ihre Schwingung UND bekommen den Schleich-Zuschlag.
        let arm = biped_pose(0.0, 0.0, 1.0, 1.0, true, 0.0).get("right_arm").rotation[0];
        let arm_ohne = biped_pose(0.0, 0.0, 1.0, 1.0, false, 0.0).get("right_arm").rotation[0];
        assert!((arm - arm_ohne - 0.4).abs() < 1e-6, "Schleichen addiert 0.4 auf die Armschwingung");
    }

    // Jeder Eintrag der Tabelle muss ein Modell mit Teilen und einer Textur haben, und jeder
    // Typname muss es in der echten Registry geben -- ein Tippfehler waere sonst still.
    #[test]
    fn modelltabelle_ist_konsistent() {
        let table = model_table();
        assert!(table.len() >= 20, "Tabelle sollte die gaengigen Mobs abdecken, hat {}", table.len());
        for (name, model, _kind) in &table {
            assert!(crate::entity_types::ENTITY_TYPE_NAMES.contains(name), "'{name}' steht nicht in der Entity-Registry");
            assert!(!model.parts.is_empty(), "'{name}' hat keine Teile");
            assert!(model.texture.starts_with("entity/"), "'{name}' hat einen unplausiblen Texturpfad '{}'", model.texture);
            assert!(model.tex_size[0] > 0.0 && model.tex_size[1] > 0.0);
        }
        // Keine Doppeleintraege -- der zweite wuerde stillschweigend gewinnen.
        let mut names: Vec<&str> = table.iter().map(|(n, _, _)| *n).collect();
        names.sort_unstable();
        let vorher = names.len();
        names.dedup();
        assert_eq!(names.len(), vorher, "doppelter Eintrag in der Modelltabelle");
    }

    // Katze, Ozelot und Schildkroete haben echte Modelle (frueher kamen sie als Box).
    #[test]
    fn katze_und_schildkroete_haben_modelle() {
        let table = model_table();
        for name in ["cat", "ocelot", "turtle"] {
            assert!(table.iter().any(|(n, _, _)| *n == name), "{name} fehlt");
        }
        assert_eq!(cat_texture_for("minecraft:all_black"), Some("entity/cat/all_black"));
        assert_eq!(cat_texture_for("meinpack:tiger"), None);
        assert_eq!(model_scale("cat"), 0.8);
        assert_eq!(model_scale("ocelot"), 1.0);
        // Sitzend kippt der Rumpf auf pi/4 -- also pi/4 - pi/2 auf die Grunddrehung.
        let sitzt = feline_pose(0.0, 0.0, 0.0, 0.0, true, false);
        assert!((sitzt.get("body").rotation[0] + std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        assert_eq!(feline_pose(0.0, 0.0, 0.0, 0.0, false, false).get("body").rotation[0], 0.0);
    }

    // Die Spinnenbeine muessen ihre GRUNDdrehung behalten (sie stehen fest schraeg ab) und
    // nur zusaetzlich schwingen -- sonst klappen alle acht Beine in eine Ebene.
    #[test]
    fn spinnenbeine_behalten_ihre_grunddrehung() {
        let model = spider_model("entity/spider/spider");
        let leg = model.parts.iter().find(|p| p.name == "right_front_leg").unwrap();
        assert_ne!(leg.rotation[1], 0.0, "Grunddrehung der Gierung fehlt");
        assert_ne!(leg.rotation[2], 0.0, "Grunddrehung des Rollens fehlt");
        let pose = spider_pose(0.0, 0.0, 1.0, 1.0);
        assert_ne!(pose.get("right_front_leg").rotation[2], 0.0, "Schwingung fehlt");
        // Die Pose ersetzt nichts, sie addiert (s. emit_part).
        assert_eq!(pose.get("right_front_leg").rotation[1], 0.0);
    }
}

#[cfg(test)]
mod default_skin_tests {
    use super::*;

    // Hex-UUID -> Bytes, nur fuer die Testdaten.
    fn uuid(hex: &str) -> [u8; 16] {
        let hex: String = hex.chars().filter(|c| *c != '-').collect();
        let mut bytes = [0u8; 16];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    }

    #[test]
    // Die Erwartungswerte stammen NICHT aus meinem Kopf, sondern aus einem Lauf von
    // java.util.UUID.hashCode() + Math.floorMod(hash, 18) mit genau diesen UUIDs -- der
    // Punkt der Uebersetzung ist, dass sie mit Javas Definition uebereinstimmt, und das
    // kann nur ein Vergleich mit Javas Ausgabe zeigen.
    fn hash_stimmt_mit_java_ueberein() {
        // "0000...0" und "ffff...f" ergeben beide Hash 0: die beiden Haelften sind gleich,
        // ihr XOR also 0. Ein Grenzfall, der ein falsches Vorzeichen sofort zeigen wuerde.
        assert_eq!(default_skin(&uuid("00000000-0000-0000-0000-000000000000")).0, DEFAULT_SKINS[0].0);
        assert_eq!(default_skin(&uuid("ffffffff-ffff-ffff-ffff-ffffffffffff")).0, DEFAULT_SKINS[0].0);
        // Hash -369792882 -- floorMod ergibt 0, Rusts `%` waere -6 und damit ein Panic beim
        // Indexzugriff. Genau der Fall, fuer den rem_euclid da ist.
        assert_eq!(default_skin(&uuid("069a79f4-44e9-4726-a5be-fca90e38aaf5")).0, DEFAULT_SKINS[0].0);
        // Hash 1946714239 -> 1, Hash 1548061430 -> 14 (die zweite Haelfte der Tabelle).
        assert_eq!(default_skin(&uuid("853c80ef-3c37-49fd-aa49-938b674adae6")).0, DEFAULT_SKINS[1].0);
        assert_eq!(default_skin(&uuid("b0d3d8a1-3aa1-42c2-bd1a-6c0e6b2d7c9b")).0, DEFAULT_SKINS[14].0);
    }

    #[test]
    // Die Tabelle ist indexbehaftet: die ersten neun Eintraege MUESSEN slim sein, die letzten
    // neun classic (DefaultSkinHelper.SKINS). Wer sie alphabetisch "aufraeumt", verschiebt
    // damit still jedem Spieler ohne Skin das Gesicht und die Armbreite.
    fn tabelle_ist_erst_slim_dann_classic() {
        assert_eq!(DEFAULT_SKINS.len(), 18);
        assert!(DEFAULT_SKINS[..9].iter().all(|(_, slim)| *slim), "erste Haelfte slim");
        assert!(DEFAULT_SKINS[9..].iter().all(|(_, slim)| !*slim), "zweite Haelfte classic");
        assert_eq!(DEFAULT_SKINS[6], ("entity/player/slim/steve", true));
        assert_eq!(DEFAULT_SKINS[15], ("entity/player/wide/steve", false));
    }

    #[test]
    // Der Standard-Skin bestimmt auch die Armbreite: ein Spieler, der auf einen Slim-Eintrag
    // faellt, muss das schmale Modell bekommen. Vorher war das hart classic/Steve.
    fn armbreite_kommt_aus_dem_eintrag() {
        let (texture, slim) = default_skin(&uuid("853c80ef-3c37-49fd-aa49-938b674adae6"));
        assert!(slim, "Index 1 ist ein Slim-Eintrag");
        assert!(texture.contains("/slim/"), "Pfad und Flag muessen zusammenpassen: {texture}");
        for (texture, slim) in DEFAULT_SKINS {
            assert_eq!(slim, texture.contains("/slim/"), "{texture} widerspricht seinem Flag");
        }
    }
}

#[cfg(test)]
mod cape_tests {
    use super::*;

    fn nahe(a: f32, b: f32, was: &str) {
        assert!((a - b).abs() < 1e-4, "{was}: {a} != {b}");
    }

    fn matrizen_gleich(a: Affine, b: Affine, was: &str) {
        for i in 0..3 {
            for j in 0..3 {
                nahe(a.m[i][j], b.m[i][j], &format!("{was} [{i}][{j}]"));
            }
        }
    }

    #[test]
    // Die Grunddrehung des Teils (180 Grad) und die erste Drehung im Quaternion (-180 Grad)
    // heben sich auf -- das ist die Begruendung dafuer, dass cape_model die Grunddrehung
    // weglaesst und cape_pose die komplette Matrix liefert. Waere die Annahme falsch, haenge
    // der Umhang seitenverkehrt vor dem Koerper.
    fn die_beiden_y_drehungen_heben_sich_auf() {
        let leer = Pose::new();
        let pose = cape_pose(&leer, 3.0, 40.0, -8.0);
        let matrix = pose.get("cape").rotation_matrix.expect("cape braucht eine Matrix");
        let ohne_die_zwei =
            rotation_x_deg(6.0 + 40.0 / 2.0 + 3.0).then(rotation_z_deg(-8.0 / 2.0)).then(Affine::rotation_y_deg(180.0 - -8.0 / 2.0));
        matrizen_gleich(matrix, ohne_die_zwei, "Y-Drehungen kuerzen sich");
    }

    #[test]
    // Der Umhang haengt am Rumpf: eine Rumpfdrehung aus der Pose (Schleichen) MUSS ihn
    // mitnehmen. cape_pose baut deshalb auf der Spieler-Pose auf statt eine leere zu
    // erzeugen -- genau das prueft dieser Test.
    fn uebernimmt_die_rumpfhaltung() {
        let schleichend = biped_pose(0.0, 0.0, 0.0, 0.0, true, 0.0);
        let pose = cape_pose(&schleichend, 0.0, 0.0, 0.0);
        let body = pose.get("body");
        nahe(body.rotation[0], 0.5, "Rumpfneigung beim Schleichen");
        nahe(body.pivot_offset[1], 3.2, "Rumpf-Pivot beim Schleichen");
    }

    #[test]
    // Stillstehen: die nachhinkende Position holt die echte ein, alle drei Winkelgroessen
    // gehen gegen 0 (bis auf die 6 Grad Grundneigung in cape_pose). Ein Umhang, der im
    // Stehen wegweht, waere der auffaelligste Fehler hier.
    fn im_stehen_haengt_der_umhang_herunter() {
        let mut state = CapeState::new();
        let pos = [100.0, 64.0, -200.0];
        for _ in 0..200 {
            state.tick(pos);
            state.tick_movement(0.0);
        }
        let (vertical, swing, side) = cape_angles(&state, pos, pos, 90.0, 1.0);
        nahe(vertical, 0.0, "keine Hoehendifferenz");
        nahe(swing, 0.0, "kein Wehen");
        nahe(side, 0.0, "kein Seitwaertsversatz");
    }

    #[test]
    // Der erste Tick muss hart setzen, nicht nachziehen: Vanilla startet die nachhinkende
    // Position bei 0, und die 10-Blocks-Regel ist genau dafuer da. Ohne sie wuerde der
    // Umhang beim Betreten der Welt aus dem Weltursprung herangezogen.
    fn erster_tick_setzt_hart() {
        let mut state = CapeState::new();
        state.tick([1000.0, 70.0, -1000.0]);
        let p = state.lerp_pos(1.0);
        assert_eq!(p, [1000.0, 70.0, -1000.0], "erster Tick muss springen, nicht kriechen");
        // Und der Sprung darf keine Bewegung vortaeuschen: last == aktuell.
        assert_eq!(state.lerp_pos(0.0), p, "kein Nachziehen ueber den Sprung hinweg");
    }

    #[test]
    // Vorwaerts laufen laesst den Umhang nach HINTEN wehen (swing > 0), rueckwaerts laufen
    // darf ihn NICHT nach vorne durch den Koerper klappen -- dafuer ist die Untergrenze 0.
    fn vorwaerts_weht_nach_hinten_rueckwaerts_klappt_nicht_um() {
        // Blickrichtung 0 Grad = nach Sueden (+Z) in Minecraft.
        let laufen = |dz: f64| {
            let mut state = CapeState::new();
            let mut pos = [0.0, 64.0, 0.0];
            state.tick(pos);
            let mut last = pos;
            for _ in 0..20 {
                last = pos;
                pos[2] += dz;
                state.tick(pos);
                state.tick_movement(0.1);
            }
            cape_angles(&state, pos, last, 0.0, 1.0)
        };
        let (_, swing_vor, _) = laufen(0.216);
        let (_, swing_zurueck, _) = laufen(-0.216);
        assert!(swing_vor > 5.0, "vorwaerts muss deutlich wehen, war {swing_vor}");
        assert_eq!(swing_zurueck, 0.0, "rueckwaerts darf nicht negativ werden, war {swing_zurueck}");
    }

    #[test]
    // Das Lauf-Schwingen haengt an add_distance_moved, das nur der eigene Spieler aufruft.
    // Ohne diese Aufrufe (fremde Spieler) muss der Term wegfallen -- sonst wackelt ein
    // fremder Umhang anders als in Vanilla.
    fn lauf_schwingen_nur_mit_zurueckgelegter_strecke() {
        let bauen = |mit_strecke: bool| {
            let mut state = CapeState::new();
            let mut pos = [0.0, 64.0, 0.0];
            state.tick(pos);
            for _ in 0..40 {
                pos[2] += 0.216;
                state.tick(pos);
                state.tick_movement(0.1);
                if mit_strecke {
                    state.add_distance_moved(0.216);
                }
            }
            state
        };
        let eigen = bauen(true);
        let fremd = bauen(false);
        assert_eq!(fremd.lerp_distance_moved(1.0), 0.0, "ohne Aufruf bleibt die Strecke 0");
        assert!(eigen.lerp_distance_moved(1.0) > 0.0, "der eigene Spieler summiert Strecke");
        // Die Laufgroesse (Amplitude des Schwingens) laeuft in beiden Faellen hoch -- der
        // Unterschied steckt allein in der Phase.
        nahe(eigen.lerp_movement(1.0), fremd.lerp_movement(1.0), "Laufgroesse identisch");
    }

    #[test]
    // Ein Teleport (mehr als 10 Blocks) setzt hart, statt den Umhang quer durch die Welt zu
    // ziehen. Genau dafuer ist die Schwelle da.
    fn teleport_setzt_hart_statt_zu_ziehen() {
        let mut state = CapeState::new();
        let start = [0.0, 64.0, 0.0];
        for _ in 0..50 {
            state.tick(start);
        }
        let ziel = [500.0, 64.0, 500.0];
        state.tick(ziel);
        assert_eq!(state.lerp_pos(1.0), ziel, "Teleport muss springen");
        let (_, swing, side) = cape_angles(&state, ziel, ziel, 0.0, 1.0);
        nahe(swing, 0.0, "nach dem Sprung kein Wehen");
        nahe(side, 0.0, "nach dem Sprung kein Versatz");
    }

    #[test]
    // Ein kleiner Schritt (unter der Schwelle) wird NACHGEZOGEN, und zwar um genau ein
    // Viertel der Differenz. Das ist die Zahl, die die gesamte Umhangbewegung bestimmt.
    fn kleiner_schritt_wird_um_ein_viertel_nachgezogen() {
        let mut state = CapeState::new();
        state.tick([0.0, 0.0, 0.0]);
        state.tick([0.0, 0.0, 4.0]);
        nahe(state.lerp_pos(1.0)[2] as f32, 1.0, "ein Viertel von 4");
        state.tick([0.0, 0.0, 4.0]);
        nahe(state.lerp_pos(1.0)[2] as f32, 1.75, "ein Viertel des Rests");
    }

    #[test]
    // Die Kastenmasse und die Texturgroesse des Umhangs. Vanilla baut ihn mit
    // textureScale (1.0, 0.5) auf eine 64x64-Vorlage -- was rechnerisch 64x32 ist, und
    // genau so gross sind die Cape-Bilder von Mojang (nachgeprueft an einem echten Abruf).
    fn umhang_masse_und_texturgroesse() {
        let model = cape_model();
        assert_eq!(model.tex_size, [64.0, 32.0]);
        let body = &model.parts[0];
        assert_eq!(body.name, "body");
        assert!(body.cuboids.is_empty(), "der Rumpf ist nur ein Gelenk, keine Geometrie");
        let cape = &body.children[0];
        assert_eq!(cape.name, "cape");
        assert_eq!(cape.pivot, [0.0, 0.0, 2.0]);
        assert_eq!(cape.cuboids[0].offset, [-5.0, 0.0, -1.0]);
        assert_eq!(cape.cuboids[0].size, [10.0, 16.0, 1.0]);
        assert_eq!(cape.cuboids[0].uv, [0.0, 0.0]);
    }
}

#[cfg(test)]
mod arm_sway_tests {
    use super::*;

    fn arme(age: f32) -> ([f32; 3], [f32; 3]) {
        let pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, age);
        (pose.get("right_arm").rotation, pose.get("left_arm").rotation)
    }

    #[test]
    // Der Punkt: die Arme liegen NIE am Koerper an. Der Roll-Term ist
    // cos(...)*0.05 + 0.05, also 0..0.1 rad -- nie negativ. Ohne den Summanden 0.05 waere er
    // symmetrisch um Null und die Arme klebten die halbe Zeit am Rumpf.
    fn arme_zeigen_immer_nach_aussen() {
        // Ueber mehrere vollstaendige Perioden abtasten (Roll-Periode ~70 Ticks).
        for i in 0..2000 {
            let age = i as f32 * 0.25;
            let (rechts, links) = arme(age);
            assert!(rechts[2] >= -1e-6, "rechter Arm bei age={age} nach innen: {}", rechts[2]);
            assert!(links[2] <= 1e-6, "linker Arm bei age={age} nach innen: {}", links[2]);
        }
    }

    #[test]
    // Der Ausschlag: 0 bis 0.1 rad (5,7 Grad), und beide Grenzen werden tatsaechlich erreicht.
    // Ein zu kleiner Wert waere unsichtbar, ein zu grosser saehe nach Fluegeln aus.
    fn ausschlag_trifft_die_vanilla_grenzen() {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for i in 0..2000 {
            let roll = arme(i as f32 * 0.25).0[2];
            min = min.min(roll);
            max = max.max(roll);
        }
        assert!(min.abs() < 1e-3, "Minimum muss 0 sein, war {min}");
        assert!((max - 0.1).abs() < 1e-3, "Maximum muss 0.1 rad sein, war {max}");
    }

    #[test]
    // Die Arme bewegen sich spiegelbildlich (sigma = +1 / -1). Waere das nicht so, ruderten
    // beide in dieselbe Richtung.
    fn beide_arme_sind_spiegelbildlich() {
        for i in 0..200 {
            let age = i as f32 * 0.5;
            let (rechts, links) = arme(age);
            assert!((rechts[2] + links[2]).abs() < 1e-6, "Roll nicht spiegelbildlich bei age={age}");
            assert!((rechts[0] + links[0]).abs() < 1e-6, "Pitch nicht spiegelbildlich bei age={age}");
        }
    }

    #[test]
    // Zwei VERSCHIEDENE Perioden (Roll 0.09/Tick, Pitch 0.067/Tick) -- daran haengt, dass die
    // Bewegung sich nie exakt wiederholt. Waeren sie gleich, saehe man ein mechanisches Auf
    // und Ab. Geprueft am Abstand der jeweiligen Maxima.
    fn roll_und_pitch_haben_verschiedene_perioden() {
        let periode = |werte: &dyn Fn(f32) -> f32| {
            // Erstes Maximum nach age=0 suchen (feine Abtastung, die Perioden sind ~70/94 Ticks).
            let mut bestes = (0.0f32, f32::MIN);
            for i in 1..4000 {
                let age = i as f32 * 0.05;
                let v = werte(age);
                if v > bestes.1 {
                    bestes = (age, v);
                }
            }
            bestes.0
        };
        // Roll ist bei age=0 schon maximal (cos(0)=1), also das ZWEITE Maximum: eine Periode.
        let roll_periode = periode(&|age| arme(age).0[2]);
        let pitch_max = periode(&|age| arme(age).0[0]);
        assert!((roll_periode - 69.8).abs() < 1.0, "Roll-Periode war {roll_periode}, erwartet ~69.8 Ticks");
        // Pitch ist ein Sinus: erstes Maximum nach einer Viertelperiode, ~23.4 Ticks.
        assert!((pitch_max - 23.4).abs() < 1.0, "Pitch-Maximum war {pitch_max}, erwartet ~23.4 Ticks");
    }

    #[test]
    // Die Ruhe-Bewegung ADDIERT auf alles andere, sie ersetzt es nicht: Schrittschwingen und
    // Schleichen muessen erhalten bleiben. In Vanilla stehen die swingArm-Aufrufe deshalb am
    // Ende von setAngles.
    fn sway_addiert_auf_schritt_und_schleichen() {
        // Laufen: das Schrittschwingen sitzt im Pitch und ist viel groesser als die Sway.
        let laufen = biped_pose(0.0, 0.0, 5.0, 1.0, false, 0.0);
        let stehen = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        assert!(
            (laufen.get("right_arm").rotation[0] - stehen.get("right_arm").rotation[0]).abs() > 0.1,
            "das Schrittschwingen darf von der Sway nicht verschluckt werden"
        );
        // Schleichen: +0.4 Pitch plus die Sway obendrauf, und der Pivot bleibt verschoben.
        let schleichen = biped_pose(0.0, 0.0, 0.0, 0.0, true, 0.0);
        let sway_pitch = stehen.get("right_arm").rotation[0];
        assert!((schleichen.get("right_arm").rotation[0] - (0.4 + sway_pitch)).abs() < 1e-5);
        assert_eq!(schleichen.get("right_arm").pivot_offset[1], 3.2, "Schleich-Pivot muss bleiben");
        // Und auch beim Schleichen zeigen die Arme nach aussen.
        assert!(schleichen.get("right_arm").rotation[2] >= 0.0);
    }
}

#[cfg(test)]
mod body_yaw_tests {
    use super::*;

    #[test]
    // Der kurze Weg beim Interpolieren: von 350 nach 10 Grad sind es 20 Grad ueber die Null,
    // nicht 340 Grad rueckwaerts. Ohne wrap_degrees drehte sich die Figur bei jedem
    // Nulldurchgang einmal komplett um.
    fn winkel_interpolation_nimmt_den_kurzen_weg() {
        let mitte = lerp_angle_degrees(0.5, 350.0, 10.0);
        assert!((wrap_degrees(mitte) - 0.0).abs() < 1e-4, "Mitte war {mitte}");
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(180.0), -180.0);
    }

    #[test]
    // Das Kernverhalten, das der Nutzer beschrieben hat: im STAND folgt der Kopf der Maus
    // sofort, der Koerper bleibt stehen -- bis zur 50-Grad-Grenze.
    fn im_stand_dreht_sich_nur_der_kopf() {
        let mut b = BodyYaw::new(0.0);
        // 40 Grad umschauen, ohne sich zu bewegen.
        b.tick(40.0, 0.0, 0.0, false);
        assert_eq!(b.body_yaw(1.0), 0.0, "der Koerper darf sich nicht bewegen");
        assert!((b.relative_head_yaw(1.0) - 40.0).abs() < 1e-4, "der Kopf muss voll folgen");
    }

    #[test]
    // Ueber 50 Grad hinaus wird der Koerper MITGEZOGEN -- genau das fuehlt sich an wie
    // "traege": erst gibt der Koerper nicht nach, dann schleppt der Kopf ihn mit.
    fn ueber_funfzig_grad_zieht_der_kopf_den_koerper_mit() {
        let mut b = BodyYaw::new(0.0);
        b.tick(120.0, 0.0, 0.0, false);
        let relativ = b.relative_head_yaw(1.0);
        assert!((relativ.abs() - 50.0).abs() < 1e-3, "die Verdrehung muss auf 50 Grad begrenzt sein, war {relativ}");
        assert!((b.body_yaw(1.0) - 70.0).abs() < 1e-3, "der Koerper muss auf 70 Grad nachgezogen sein");
    }

    #[test]
    // Laufen: der Koerper dreht sich zur BEWEGUNGSRICHTUNG, nicht zur Blickrichtung. Wer
    // seitwaerts laeuft und nach vorn schaut, dreht den Koerper zur Laufrichtung.
    //
    // Minecraft-Konvention: yaw 0 = Sued (+Z), 90 = West (-X).
    fn beim_laufen_folgt_der_koerper_der_bewegungsrichtung() {
        let mut b = BodyYaw::new(0.0);
        // Nach Sueden laufen (+Z) und dabei nach Sueden schauen -> Ziel 0, bleibt 0.
        for _ in 0..20 {
            b.tick(0.0, 0.0, 0.2, false);
        }
        assert!(b.body_yaw(1.0).abs() < 1e-2, "nach Sueden laufen -> Koerper nach Sueden");

        // Nach Westen laufen (-X), weiter nach Sueden schauen. Bewegungsrichtung ist yaw 90,
        // aber die 50-Grad-Grenze laesst den Koerper nur bis 50 kommen.
        let mut b = BodyYaw::new(0.0);
        for _ in 0..40 {
            b.tick(0.0, -0.2, 0.0, false);
        }
        let koerper = b.body_yaw(1.0);
        assert!((koerper - 50.0).abs() < 1e-2, "Koerper muss an der 50-Grad-Grenze haengen, war {koerper}");
        assert!((b.relative_head_yaw(1.0) + 50.0).abs() < 1e-2, "der Kopf schaut 50 Grad zurueck");
    }

    #[test]
    // Rueckwaerts laufen: Blick und Bewegung mehr als 95 Grad auseinander -> der Koerper
    // dreht sich um 180 Grad, statt sich zu verrenken. Ohne diesen Zweig liefe die Figur
    // rueckwaerts mit verdrehtem Oberkoerper.
    fn rueckwaerts_laufen_dreht_den_koerper_um() {
        let mut b = BodyYaw::new(0.0);
        // Blick nach Sueden (0), Bewegung nach Norden (-Z) -> 180 Grad Unterschied.
        for _ in 0..40 {
            b.tick(0.0, 0.0, -0.2, false);
        }
        // Ziel waere 180-180 = 0, der Koerper bleibt also bei 0 und schaut mit dem Blick
        // in dieselbe Richtung -- man laeuft rueckwaerts.
        assert!(b.body_yaw(1.0).abs() < 1e-2, "Koerper bleibt in Blickrichtung, war {}", b.body_yaw(1.0));
        assert!(b.relative_head_yaw(1.0).abs() < 1e-2, "kein verdrehter Hals beim Rueckwaertslaufen");
    }

    #[test]
    // Die 30 % pro Tick: der Koerper springt nicht auf sein Ziel, sondern kriecht dorthin.
    // Das ist die eigentliche "Traegheit".
    fn koerper_holt_dreissig_prozent_pro_tick_auf() {
        let mut b = BodyYaw::new(0.0);
        // Zuschlagen setzt das Ziel auf die Blickrichtung -- so laesst sich der reine
        // Nachlauf ohne den Bewegungszweig pruefen. 40 Grad bleiben unter der 50-Grad-Grenze.
        b.tick(40.0, 0.0, 0.0, true);
        assert!((b.body_yaw(1.0) - 12.0).abs() < 1e-3, "erster Tick: 30 % von 40 = 12, war {}", b.body_yaw(1.0));
        b.tick(40.0, 0.0, 0.0, true);
        assert!((b.body_yaw(1.0) - 20.4).abs() < 1e-3, "zweiter Tick: +30 % vom Rest, war {}", b.body_yaw(1.0));
    }

    #[test]
    // Zwischentick-Interpolation: der Koerper gleitet, statt 20 mal pro Sekunde zu springen.
    fn koerper_gleitet_zwischen_den_ticks() {
        let mut b = BodyYaw::new(0.0);
        b.tick(40.0, 0.0, 0.0, true);
        let anfang = b.body_yaw(0.0);
        let mitte = b.body_yaw(0.5);
        let ende = b.body_yaw(1.0);
        assert!(anfang < mitte && mitte < ende, "{anfang} / {mitte} / {ende}");
        assert!((mitte - ende / 2.0).abs() < 1e-3, "die Mitte muss genau dazwischen liegen");
    }

    #[test]
    // Ein Nulldurchgang darf nicht zu einer Volldrehung fuehren. Blick von 350 auf 10 Grad,
    // der Koerper folgt -- und muss dabei ueber die Null gehen, nicht 340 Grad zurueck.
    fn nulldurchgang_dreht_nicht_komplett_herum() {
        let mut b = BodyYaw::new(350.0);
        b.tick(10.0, 0.0, 0.0, true);
        let bewegung = wrap_degrees(b.body_yaw(1.0) - 350.0);
        assert!(bewegung > 0.0 && bewegung < 20.0, "Koerper muss vorwaerts ueber die Null, war {bewegung}");
    }
}

#[cfg(test)]
mod diffuse_tests {
    use super::*;

    fn nahe(a: f32, b: f32, was: &str) {
        assert!((a - b).abs() < 1e-3, "{was}: {a} != {b}");
    }

    #[test]
    // Die sechs achsenparallelen Flaechen, nachgerechnet aus Vanillas Lichtvektoren und der
    // Formel aus light.glsl. Die Zahlen sind ausgerechnet, nicht gewaehlt.
    fn achsenflaechen_treffen_die_vanilla_werte() {
        nahe(diffuse_shade([0.0, 1.0, 0.0]), 1.0, "oben");
        nahe(diffuse_shade([0.0, -1.0, 0.0]), 0.4, "unten");
        nahe(diffuse_shade([0.0, 0.0, -1.0]), 0.7395, "nord");
        nahe(diffuse_shade([0.0, 0.0, 1.0]), 0.7395, "sued");
        nahe(diffuse_shade([1.0, 0.0, 0.0]), 0.4970, "ost");
        nahe(diffuse_shade([-1.0, 0.0, 0.0]), 0.4970, "west");
    }

    #[test]
    // Zwei Plausibilitaeten, an denen ich bei der GUI-Beleuchtung schon einmal gescheitert bin:
    // oben MUSS die hellste Flaeche sein und unten die dunkelste. Kommt es anders heraus, ist
    // ein Vorzeichen verdreht -- und das faellt am fertigen Bild kaum auf, weil es "auch
    // irgendwie schattiert" aussieht.
    fn oben_ist_am_hellsten_unten_am_dunkelsten() {
        let richtungen = [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]];
        let werte: Vec<f32> = richtungen.iter().map(|n| diffuse_shade(*n)).collect();
        let max = werte.iter().cloned().fold(f32::MIN, f32::max);
        let min = werte.iter().cloned().fold(f32::MAX, f32::min);
        nahe(diffuse_shade([0.0, 1.0, 0.0]), max, "oben ist die hellste Flaeche");
        nahe(diffuse_shade([0.0, -1.0, 0.0]), min, "unten ist die dunkelste Flaeche");
    }

    #[test]
    // Keine Flaeche wird ganz schwarz -- dafuer ist MINECRAFT_AMBIENT_LIGHT (0.4) da -- und
    // keine ueberstrahlt.
    fn helligkeit_bleibt_im_erlaubten_bereich() {
        for i in 0..64 {
            for j in 0..64 {
                let (a, b) = (i as f32 / 63.0 * std::f32::consts::TAU, j as f32 / 63.0 * std::f32::consts::PI);
                let n = [a.cos() * b.sin(), b.cos(), a.sin() * b.sin()];
                let shade = diffuse_shade(n);
                assert!(shade >= AMBIENT_LIGHT - 1e-6, "zu dunkel: {shade}");
                assert!(shade <= 1.0 + 1e-6, "zu hell: {shade}");
            }
        }
    }

    #[test]
    // Der Kern des Ganzen: dreht sich das Entity, WANDERT die Helligkeit einer Flaeche. Eine
    // fest je Modellflaeche eingebackene Tabelle (wie bei Bloecken) waere ueber die Drehung
    // konstant -- genau das soll dieser Test verhindern.
    fn helligkeit_wandert_beim_drehen() {
        let shade_bei = |grad: f32| {
            let base = Affine::rotation_y_deg(grad);
            diffuse_shade(base.apply_direction([0.0, 0.0, 1.0]))
        };
        nahe(shade_bei(0.0), 0.7395, "unverdreht");
        nahe(shade_bei(90.0), 0.4970, "eine Vierteldrehung");
        assert!(shade_bei(0.0) > shade_bei(60.0), "die Helligkeit muss sich beim Drehen aendern");
        // Und sie muss stetig verlaufen, nicht springen.
        let mut vorher = shade_bei(0.0);
        for grad in 1..=360 {
            let jetzt = shade_bei(grad as f32);
            assert!((jetzt - vorher).abs() < 0.02, "Sprung bei {grad} Grad: {vorher} -> {jetzt}");
            vorher = jetzt;
        }
    }

    #[test]
    // Die Einbettung dreht das Modell mit scale(-1,-1,1) (s. entity_transform). Die
    // Modellraum-Normale "unten" (0,-1,0) muss dadurch in der WELT nach oben zeigen und die
    // hellste Flaeche werden. Waere das falsch, waere die Figur von unten beleuchtet.
    fn modellraum_unten_wird_in_der_welt_oben() {
        let base = entity_transform([0.0, 64.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
        let welt = base.apply_direction([0.0, -1.0, 0.0]);
        nahe(welt[1], 1.0, "Modellraum-unten zeigt in der Welt nach oben");
        nahe(diffuse_shade(welt), 1.0, "und ist damit die hellste Flaeche");
    }
}

#[cfg(test)]
mod swim_tests {
    use super::*;

    const PI: f32 = std::f32::consts::PI;

    fn schwimmend(swing: f32, lean: f32) -> Pose {
        biped_pose_leaning(0.0, 0.0, swing, 1.0, false, lean, 0.0)
    }

    #[test]
    // Ohne Neigung darf die Schwimmbewegung NICHTS tun -- sonst wuerde sie an Land in die
    // Laufanimation hineinpfuschen.
    fn an_land_keine_wirkung() {
        let laufen = biped_pose_leaning(0.0, 0.0, 5.0, 1.0, false, 0.0, 0.0);
        let nur_laufen = biped_pose(0.0, 0.0, 5.0, 1.0, false, 0.0);
        assert_eq!(laufen.get("right_arm").rotation, nur_laufen.get("right_arm").rotation);
        assert_eq!(laufen.get("left_leg").rotation, nur_laufen.get("left_leg").rotation);
    }

    #[test]
    // Voll eingetaucht zeigen beide Arme nach HINTEN (yaw = PI). Das ist die Grundhaltung des
    // Kraulens -- ohne sie ruderte die Figur seitlich statt nach vorn.
    fn voll_eingetaucht_zeigen_die_arme_nach_hinten() {
        for swing in [0.0, 5.0, 13.9, 14.0, 20.0, 25.9] {
            let pose = schwimmend(swing, 1.0);
            let links = pose.get("left_arm").rotation[1];
            let rechts = pose.get("right_arm").rotation[1];
            assert!((wrap_radians(links - PI)).abs() < 1e-4, "linker Arm bei {swing}: yaw {links}");
            assert!((wrap_radians(rechts - PI)).abs() < 1e-4, "rechter Arm bei {swing}: yaw {rechts}");
        }
    }

    #[test]
    // Die drei Abschnitte des Zyklus, an ihren Grenzen geprueft. Der Zug (i < 14) haelt die
    // Arme flach (pitch 0), das Vorholen hebt sie bis PI/2, das Zuruecksetzen senkt sie wieder.
    fn die_drei_abschnitte_stimmen() {
        let pitch = |swing: f32| schwimmend(swing, 1.0).get("right_arm").rotation[0];
        assert!(pitch(0.0).abs() < 1e-4, "Zuganfang: Arme flach");
        assert!(pitch(13.9).abs() < 1e-4, "Zugende: immer noch flach");
        assert!(pitch(14.0).abs() < 1e-4, "Vorholen beginnt bei 0");
        assert!((pitch(18.0) - PI / 4.0).abs() < 1e-3, "Vorholen Mitte: PI/4, war {}", pitch(18.0));
        assert!((pitch(21.99) - PI / 2.0).abs() < 1e-2, "Vorholen Ende: fast PI/2");
        assert!((pitch(22.0) - PI / 2.0).abs() < 1e-4, "Zuruecksetzen beginnt oben");
        assert!(pitch(25.99).abs() < 1e-2, "Zuruecksetzen endet unten");
    }

    #[test]
    // Der Zyklus wiederholt sich alle 26 Einheiten -- und rem_euclid muss auch mit negativen
    // Phasen umgehen (die Schrittphase wird zwischen den Ticks zurueckgerechnet und kann kurz
    // unter null liegen, s. LimbAnimator::animation_progress).
    fn zyklus_wiederholt_sich_und_vertraegt_negative_phasen() {
        let a = schwimmend(3.0, 1.0).get("right_arm").rotation;
        let b = schwimmend(3.0 + 26.0, 1.0).get("right_arm").rotation;
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-4, "Achse {i}: {} != {}", a[i], b[i]);
        }
        // Darf nicht panicken und muss im gueltigen Zyklus landen.
        let negativ = schwimmend(-1.0, 1.0).get("right_arm").rotation;
        assert!(negativ.iter().all(|v| v.is_finite()));
    }

    #[test]
    // Halb eingetaucht muss die Haltung ZWISCHEN Laufen und Schwimmen liegen. Das ist der
    // weiche Uebergang beim Eintauchen -- ohne ihn wuerde die Figur umspringen.
    fn halbe_neigung_mischt_zwischen_laufen_und_schwimmen() {
        let land = schwimmend(5.0, 0.0).get("right_arm").rotation[1];
        let wasser = schwimmend(5.0, 1.0).get("right_arm").rotation[1];
        let halb = schwimmend(5.0, 0.5).get("right_arm").rotation[1];
        assert!((halb - (land + wasser) / 2.0).abs() < 1e-4, "halb war {halb}, erwartet zwischen {land} und {wasser}");
    }

    #[test]
    // Die Beine treten gegenphasig und deutlich langsamer als beim Laufen (Frequenz 1/3 statt
    // 0.6662, Ausschlag 0.3 statt 1.4). Beide Beine gleichphasig waere ein Delfinschlag statt
    // Kraulen.
    fn beine_treten_gegenphasig_und_flach() {
        let mut max_ausschlag: f32 = 0.0;
        for i in 0..200 {
            let swing = i as f32 * 0.25;
            let pose = schwimmend(swing, 1.0);
            let links = pose.get("left_leg").rotation[0];
            let rechts = pose.get("right_leg").rotation[0];
            assert!((links + rechts).abs() < 1e-4, "Beine muessen gegenphasig sein bei {swing}");
            max_ausschlag = max_ausschlag.max(rechts.abs());
        }
        assert!((max_ausschlag - 0.3).abs() < 1e-3, "Ausschlag muss 0.3 sein, war {max_ausschlag}");
    }

    #[test]
    // Der Kopf richtet sich beim Schwimmen nach vorn aus (-PI/4), sonst schaute die
    // waagerecht liegende Figur in den Boden. Das war schon vorher da -- der Test haelt fest,
    // dass die Schwimmbewegung es nicht ueberschreibt.
    fn kopf_bleibt_nach_vorn_gerichtet() {
        let pose = schwimmend(5.0, 1.0);
        assert!((pose.get("head").rotation[0] + PI / 4.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod first_person_tests {
    use super::*;

    // Wo der Arm in der Ego-Ansicht landet: die Kette auf den Mittelpunkt des Arm-Kastens
    // angewandt. Der Arm-Pivot liegt bei (-5, 2, 0) Modellpixeln, der Kasten reicht von dort
    // 12 Pixel nach unten.
    fn arm_position(right: bool, equip: f32, swing: f32) -> [f32; 3] {
        let base = first_person_arm_transform(right, equip, swing);
        // JEDER Arm hat seinen eigenen Pivot: der rechte bei x = -5, der linke bei x = +5
        // Modellpixeln. Der erste Anlauf dieses Tests hat fuer beide den rechten genommen und
        // deshalb faelschlich einen Spiegelfehler angezeigt.
        let pivot_x = if right { -5.0 / 16.0 } else { 5.0 / 16.0 };
        base.apply([pivot_x, 2.0 / 16.0, 0.0])
    }

    #[test]
    // Der rechte Arm muss rechts vom Auge und unterhalb sitzen, und VOR der Kamera (negatives
    // Z im Sichtraum -- dorthin schaut man). Das ist die Grundprobe: stimmt eines der
    // Vorzeichen in der Kette nicht, landet der Arm hinter dem Kopf und man sieht nichts.
    fn rechter_arm_sitzt_rechts_unten_vor_der_kamera() {
        let p = arm_position(true, 0.0, 0.0);
        assert!(p[0] > 0.0, "muss rechts der Mitte liegen, war x={}", p[0]);
        assert!(p[1] < 0.0, "muss unterhalb der Augenhoehe liegen, war y={}", p[1]);
        assert!(p[2] < 0.0, "muss VOR der Kamera liegen, war z={}", p[2]);
    }

    #[test]
    // Der linke Arm ist das Spiegelbild: gleiche Hoehe und Tiefe, x umgekehrt.
    fn linker_arm_ist_das_spiegelbild() {
        let r = arm_position(true, 0.0, 0.0);
        let l = arm_position(false, 0.0, 0.0);
        assert!((r[0] + l[0]).abs() < 1e-5, "x muss gespiegelt sein: {} / {}", r[0], l[0]);
        assert!((r[1] - l[1]).abs() < 1e-5, "y muss gleich sein");
        assert!((r[2] - l[2]).abs() < 1e-5, "z muss gleich sein");
    }

    #[test]
    // Die Wechsel-Animation senkt die Hand: bei equipProgress 1 liegt sie tiefer als bei 0.
    // Der Term ist `equipProgress * -0.6` auf der Y-Achse.
    fn wechsel_animation_senkt_die_hand() {
        let oben = arm_position(true, 0.0, 0.0);
        let unten = arm_position(true, 1.0, 0.0);
        assert!(unten[1] < oben[1], "bei laufendem Wechsel muss die Hand tiefer sein");
        assert!((oben[1] - unten[1] - 0.6).abs() < 1e-4, "der Versatz muss genau 0.6 sein");
    }

    #[test]
    // Waehrend eines Schlags bewegt sich die Hand tatsaechlich -- und kehrt am Ende des
    // Schwungs (Fortschritt 1) wieder in die Ruhelage zurueck.
    fn schlag_bewegt_die_hand_und_kehrt_zurueck() {
        let ruhe = arm_position(true, 0.0, 0.0);
        let mitte = arm_position(true, 0.0, 0.5);
        let ende = arm_position(true, 0.0, 1.0);
        let abstand = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        assert!(abstand(ruhe, mitte) > 0.1, "in der Schwungmitte muss sich die Hand deutlich bewegt haben");
        assert!(abstand(ruhe, ende) < 1e-4, "am Schwungende ist sie wieder in Ruhelage");
    }

    #[test]
    // Stetigkeit ohne willkuerliche Schwelle: verdoppelt man die Abtastdichte, muss sich der
    // groesste Einzelschritt etwa HALBIEREN. Das ist genau die Aussage "die Bewegung ist
    // stetig" -- an einem echten Sprung wuerde feineres Abtasten den groessten Schritt gar
    // nicht verkleinern.
    //
    // Ein erster Anlauf hat stattdessen "Schritt < 0.1 bei 100 Stuetzstellen" geprueft und
    // fehlgeschlagen: der groesste Schritt liegt dort bei rund 0.8. Die Hand bewegt sich
    // waehrend eines Schlags einfach schnell -- die Schwelle war geraten, nicht gemessen.
    fn schlagbewegung_ist_stetig() {
        let groesster_schritt = |stuetzstellen: usize| {
            let mut max = 0.0f32;
            let mut vorher = arm_position(true, 0.0, 0.0);
            for i in 1..=stuetzstellen {
                let jetzt = arm_position(true, 0.0, i as f32 / stuetzstellen as f32);
                let d = ((jetzt[0] - vorher[0]).powi(2) + (jetzt[1] - vorher[1]).powi(2) + (jetzt[2] - vorher[2]).powi(2)).sqrt();
                max = max.max(d);
                vorher = jetzt;
            }
            max
        };
        let grob = groesster_schritt(500);
        let fein = groesster_schritt(1000);
        let verhaeltnis = fein / grob;
        // Erwartet wird 1/sqrt(2) = 0.707, NICHT 1/2. Grund: der Schwung laeuft ueber
        // g = sqrt(swingProgress), und die Wurzel hat bei 0 eine unendliche Steigung -- die
        // Hand schnellt zu Beginn des Schlags sehr schnell heraus und laeuft zum Ende hin
        // aus. Der groesste Schritt liegt damit immer direkt am Anfang und schrumpft nur mit
        // der Wurzel der Abtastdichte.
        //
        // Die obere Schranke ist die eigentliche Pruefung: an einem echten SPRUNG bliebe der
        // groesste Schritt beim Verfeinern gleich (Verhaeltnis ~1.0). Die untere haelt fest,
        // dass es der Wurzel-Anlauf ist und keine glatte Bewegung.
        assert!(verhaeltnis < 0.8, "kein Sprung erlaubt: {grob} -> {fein} (Verhaeltnis {verhaeltnis})");
        assert!(verhaeltnis > 0.6, "der Wurzel-Anlauf muss erhalten bleiben: Verhaeltnis {verhaeltnis}");
    }

    #[test]
    // Das Arm-Modell enthaelt genau den einen Arm samt Aermel -- nicht den ganzen Spieler.
    fn arm_modell_enthaelt_nur_den_arm() {
        for slim in [false, true] {
            let model = player_arm_model(slim, true);
            assert_eq!(model.parts.len(), 1, "nur ein Teil");
            assert_eq!(model.parts[0].name, "right_arm");
            assert_eq!(model.parts[0].children.len(), 1, "der Aermel gehoert dazu");
            assert_eq!(model.parts[0].children[0].name, "right_sleeve");
            assert_eq!(model.tex_size, [64.0, 64.0], "Skin-Format");
        }
        assert_eq!(player_arm_model(false, false).parts[0].name, "left_arm");
        // Schmal: der Arm ist 3 statt 4 Pixel breit.
        assert_eq!(player_arm_model(true, true).parts[0].cuboids[0].size[0], 3.0);
        assert_eq!(player_arm_model(false, true).parts[0].cuboids[0].size[0], 4.0);
    }

    #[test]
    // Die Haltung: nur ein leichtes Rollen nach aussen, sonst Grundstellung.
    fn haltung_ist_grundstellung_mit_leichtem_rollen() {
        let rechts = first_person_arm_pose(true).get("right_arm").rotation;
        assert_eq!(rechts, [0.0, 0.0, 0.1]);
        let links = first_person_arm_pose(false).get("left_arm").rotation;
        assert_eq!(links, [0.0, 0.0, -0.1]);
    }
}

#[cfg(test)]
mod rotation_xyz_tests {
    use super::*;

    fn nah(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    // Einzelachsen: eine 90-Grad-Drehung um X schickt +Y nach +Z (Rechte-Hand-Regel).
    #[test]
    fn einzelne_achsen_drehen_richtig() {
        assert!(nah(Affine::rotation_xyz_deg(90.0, 0.0, 0.0).apply([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(nah(Affine::rotation_xyz_deg(0.0, 90.0, 0.0).apply([0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]));
        assert!(nah(Affine::rotation_xyz_deg(0.0, 0.0, 90.0).apply([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
    }

    // Der springende Punkt gegenueber rotation_zyx: bei gemischten Winkeln kommt eine ANDERE
    // Drehung heraus. Waere die Reihenfolge egal, koennte man eine der beiden weglassen --
    // und das Item laege bei jedem Schwert schief in der Hand.
    #[test]
    fn xyz_ist_nicht_zyx() {
        let xyz = Affine::rotation_xyz_deg(0.0, -90.0, 55.0).apply([1.0, 0.0, 0.0]);
        let zyx = Affine::rotation_zyx(0.0, (-90.0f32).to_radians(), 55.0f32.to_radians()).apply([1.0, 0.0, 0.0]);
        assert!(!nah(xyz, zyx), "XYZ und ZYX duerfen nicht dasselbe liefern: {xyz:?} / {zyx:?}");
    }

    // Zusammengesetzt muss die Matrix genau Rx * Ry * Rz sein -- gegen die Einzelmatrizen
    // nachgerechnet, damit die ausmultiplizierte Fassung oben nicht still danebenliegt.
    #[test]
    fn entspricht_dem_produkt_der_einzeldrehungen() {
        let (x, y, z) = (23.0f32, -41.0, 67.0);
        let einzeln =
            Affine::rotation_xyz_deg(x, 0.0, 0.0).then(Affine::rotation_xyz_deg(0.0, y, 0.0)).then(Affine::rotation_xyz_deg(0.0, 0.0, z));
        let zusammen = Affine::rotation_xyz_deg(x, y, z);
        for probe in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.3, -0.7, 0.5]] {
            assert!(nah(einzeln.apply(probe), zusammen.apply(probe)), "Produkt weicht ab bei {probe:?}");
        }
    }
}

#[cfg(test)]
mod blocking_arm_pose_tests {
    use super::*;

    fn pose_mit_kopf(pitch: f32, yaw: f32) -> Pose {
        let mut pose = Pose::new();
        pose.rotate("head", [pitch, yaw, 0.0]);
        pose.rotate("right_arm", [0.0, 0.0, 0.0]);
        pose.rotate("left_arm", [0.0, 0.0, 0.0]);
        pose
    }

    #[test]
    // Der Arm geht deutlich HOCH (negative Neigung) und dreht zur Koerpermitte -- sonst
    // haenge der Schild weiter an der Seite, wie frueher.
    fn der_blockarm_geht_hoch_und_zur_mitte() {
        let mut pose = pose_mit_kopf(0.0, 0.0);
        apply_blocking_arm_pose(&mut pose, true);
        let rechts = pose.get("right_arm").rotation;
        assert!((rechts[0] + 0.9424779).abs() < 1e-5, "Neigung {}", rechts[0]);
        assert!((rechts[1] + 30.0 * std::f32::consts::PI / 180.0).abs() < 1e-5, "Gierung {}", rechts[1]);

        let mut pose = pose_mit_kopf(0.0, 0.0);
        apply_blocking_arm_pose(&mut pose, false);
        // Die linke Hand dreht spiegelbildlich, die Neigung bleibt dieselbe.
        assert!((pose.get("left_arm").rotation[1] - 30.0 * std::f32::consts::PI / 180.0).abs() < 1e-5);
    }

    #[test]
    // Der Schild folgt dem Blick -- aber nur bis zum Anschlag. Ohne die Deckel liefe der Arm
    // beim Blick senkrecht nach unten durch den Koerper.
    fn der_blockarm_folgt_dem_blick_bis_zum_anschlag() {
        let flach = {
            let mut p = pose_mit_kopf(0.0, 0.0);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        let leicht_hoch = {
            let mut p = pose_mit_kopf(-0.3, 0.2);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        assert!(leicht_hoch[0] < flach[0], "beim Blick nach oben muss der Arm mitgehen");
        assert!(leicht_hoch[1] > flach[1], "beim Blick nach rechts ebenso");

        // Weit ueber den Anschlag hinaus: dieselben Werte wie genau AM Anschlag.
        let am_anschlag = {
            let mut p = pose_mit_kopf(0.43633232, std::f32::consts::FRAC_PI_6);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        let weit_darueber = {
            let mut p = pose_mit_kopf(1.5, 1.5);
            apply_blocking_arm_pose(&mut p, true);
            p.get("right_arm").rotation
        };
        assert!((am_anschlag[0] - weit_darueber[0]).abs() < 1e-6, "Neigung nicht gedeckelt");
        assert!((am_anschlag[1] - weit_darueber[1]).abs() < 1e-6, "Gierung nicht gedeckelt");
    }

    #[test]
    // BLOCK sticht ITEM aus: ein Schild in der Hand darf nicht BEIDE Haltungen bekommen.
    fn blocken_sticht_die_gewoehnliche_item_haltung_aus() {
        let mit_block = biped_pose_full(
            0.0,
            0.0,
            0.0,
            0.0,
            BipedExtras { item_left: true, block_left: true, ..Default::default() },
        );
        let nur_item = biped_pose_full(0.0, 0.0, 0.0, 0.0, BipedExtras { item_left: true, ..Default::default() });
        let hoch = mit_block.get("left_arm").rotation;
        let normal = nur_item.get("left_arm").rotation;
        assert!(hoch[0] < normal[0] - 0.5, "der blockende Arm muss deutlich hoeher sein: {hoch:?} gegen {normal:?}");
        assert!(hoch[1].abs() > 0.3, "und zur Mitte gedreht: {}", hoch[1]);
    }
}

#[cfg(test)]
mod item_arm_pose_tests {
    use super::*;

    // Der Arm wird ANGEHOBEN: die Neigung sinkt um PI/10 (im Modellraum zeigt X-Rotation
    // negativ nach vorn/oben). Aus dem Stand heraus (pitch 0) also genau -PI/10.
    #[test]
    fn item_hebt_den_arm() {
        let mut pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        let vorher = pose.get("right_arm").rotation[0];
        apply_item_arm_pose(&mut pose, true);
        let nachher = pose.get("right_arm").rotation[0];
        assert!(nachher < vorher, "der Arm muss sich heben, nicht senken");
        assert!((nachher - (vorher * 0.5 - std::f32::consts::PI / 10.0)).abs() < 1e-6);
    }

    // Die Laufanimation bleibt erhalten, nur halbiert -- genau das unterscheidet Vanillas
    // Formel von einem simplen "Winkel drauf". Bei vollem Ausschlag muss die Differenz
    // zwischen zwei Schwungphasen nach dem Anheben noch da, aber kleiner sein.
    #[test]
    fn schwung_bleibt_gedaempft_erhalten() {
        let messe = |swing: f32| {
            let mut pose = biped_pose(0.0, 0.0, swing, 1.0, false, 0.0);
            let roh = pose.get("right_arm").rotation[0];
            apply_item_arm_pose(&mut pose, true);
            (roh, pose.get("right_arm").rotation[0])
        };
        let (roh_a, mit_a) = messe(0.0);
        let (roh_b, mit_b) = messe(1.5);
        let roh_spanne = (roh_b - roh_a).abs();
        let mit_spanne = (mit_b - mit_a).abs();
        assert!(mit_spanne > 0.0, "der Arm darf beim Laufen nicht einfrieren");
        assert!(mit_spanne < roh_spanne, "der Ausschlag muss gedaempft sein");
        assert!((mit_spanne - roh_spanne * 0.5).abs() < 1e-5, "genau halbiert");
    }

    // Die linke Hand betrifft den linken Arm -- und NUR den.
    #[test]
    fn nebenhand_hebt_nur_den_linken_arm() {
        let mut pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        let rechts_vorher = pose.get("right_arm").rotation[0];
        apply_item_arm_pose(&mut pose, false);
        assert_eq!(pose.get("right_arm").rotation[0], rechts_vorher);
        assert!(pose.get("left_arm").rotation[0] < rechts_vorher);
    }
}

// Ruestung und Schild -- beides Modelle, die es vorher gar nicht gab.
#[cfg(test)]
mod ausruestungs_tests {
    use super::*;
    use crate::armor::ArmorSlot;

    fn teilnamen(model: &Model) -> Vec<&'static str> {
        model.parts.iter().map(|p| p.name).collect()
    }

    // ArmorFeatureRenderer.setVisible, Zeile fuer Zeile. Besonders die HOSE: sie zeichnet den
    // Rumpf MIT (das ist der Guertel), die Schuhe nicht.
    #[test]
    fn jedes_ruestungsteil_deckt_die_richtigen_koerperteile() {
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Head)), ["head"]);
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Chest)), ["body", "right_arm", "left_arm"]);
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Legs)), ["body", "right_leg", "left_leg"]);
        assert_eq!(teilnamen(&armor_model(ArmorSlot::Feet)), ["right_leg", "left_leg"]);
        // Der Helm bringt die zweite Kopfebene ("hat") als Kind mit -- sie traegt in Vanilla
        // den ueberstehenden Teil des Helms.
        let head = &armor_model(ArmorSlot::Head).parts[0];
        assert_eq!(head.children.iter().map(|c| c.name).collect::<Vec<_>>(), ["hat"]);
    }

    // Die Hose ist die INNERE Ebene (Dilation 0.5), alles andere die aeussere (1.0). Ohne den
    // Unterschied steckten Brustpanzer und Hose an der Huefte ineinander.
    #[test]
    fn die_hose_ist_duenner_als_der_brustpanzer() {
        let dicke = |slot: ArmorSlot| armor_model(slot).parts.iter().find(|p| p.name == "body").unwrap().cuboids[0].dilation[0];
        assert_eq!(dicke(ArmorSlot::Legs), 0.5);
        assert_eq!(dicke(ArmorSlot::Chest), 1.0);
    }

    // Und die aeussere Ebene muss WIRKLICH aussen liegen: der Brustpanzer bei Dilation 1.0
    // steht ueberall einen Pixel weiter als der nackte Rumpf.
    #[test]
    fn die_ruestung_liegt_ausserhalb_des_koerpers() {
        let panzer = armor_model(ArmorSlot::Chest);
        let rumpf_panzer = panzer.parts.iter().find(|p| p.name == "body").unwrap();
        let nackt = player_model(false);
        let rumpf_nackt = nackt.parts.iter().find(|p| p.name == "body").unwrap();
        assert!(
            rumpf_panzer.cuboids[0].dilation > rumpf_nackt.cuboids[0].dilation,
            "der Brustpanzer muss weiter aussen liegen als der Rumpf"
        );
        // Gleiche Pivots -- sonst folgte die Ruestung der Pose nicht mehr.
        assert_eq!(rumpf_panzer.pivot, rumpf_nackt.pivot);
    }

    // ShieldEntityModel.getTexturedModelData: zwei Teile, feste Masse, Textur 64x64.
    #[test]
    fn das_schild_hat_platte_und_griff() {
        let model = shield_model();
        assert_eq!(model.tex_size, [64.0, 64.0]);
        assert_eq!(teilnamen(&model), ["plate", "handle"]);
        let plate = &model.parts[0].cuboids[0];
        assert_eq!(plate.uv, [0.0, 0.0]);
        assert_eq!(plate.offset, [-6.0, -11.0, -2.0]);
        assert_eq!(plate.size, [12.0, 22.0, 1.0]);
        let handle = &model.parts[1].cuboids[0];
        assert_eq!(handle.uv, [26.0, 0.0]);
        assert_eq!(handle.offset, [-1.0, -3.0, -1.0]);
        assert_eq!(handle.size, [2.0, 6.0, 6.0]);
    }

    // Das Schild steht UM seinen Ursprung, nicht in der 0..1-Zelle eines Item-Modells --
    // genau deshalb faellt bei ihm das `translate(-0.5)` weg (s. App::emit_shield). Waere es
    // in einer Zelle, laege der Schwerpunkt bei +0.5 statt bei 0.
    #[test]
    fn das_schild_steht_um_seinen_ursprung() {
        let mut out = Vec::new();
        emit(&shield_model(), &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
        let (min_x, max_x) = out.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v.position[0]), b.max(v.position[0])));
        assert!(min_x < 0.0 && max_x > 0.0, "das Schild muss den Ursprung umschliessen, war {min_x}..{max_x}");
        // 12 Pixel breit, also -6/16 .. 6/16.
        assert!((min_x + 6.0 / 16.0).abs() < 1e-5, "linke Kante {min_x}");
        assert!((max_x - 6.0 / 16.0).abs() < 1e-5, "rechte Kante {max_x}");
    }

    // Die Faerbung darf NUR multiplizieren, nicht die Helligkeit ersetzen: Lederruestung
    // bekommt sie, alle anderen Materialien die neutrale Weissfaerbung, und beide muessen bei
    // gleicher Flaeche dasselbe Verhaeltnis behalten.
    #[test]
    fn die_faerbung_multipliziert_die_helligkeit() {
        let model = armor_model(ArmorSlot::Chest);
        let mut weiss = Vec::new();
        let mut braun = Vec::new();
        emit_tinted(&model, &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], 1.0, &mut weiss);
        emit_tinted(&model, &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], crate::armor::UNDYED_LEATHER, 1.0, &mut braun);
        assert_eq!(weiss.len(), braun.len());
        for (w, b) in weiss.iter().zip(&braun) {
            for k in 0..3 {
                assert!((b.color[k] - w.color[k] * crate::armor::UNDYED_LEATHER[k]).abs() < 1e-5);
            }
        }
    }
}

#[cfg(test)]
mod end_crystal_tests {
    use super::*;

    #[test]
    // getYOffset pendelt zwischen -1.4 und -0.6 Bloecken.
    fn der_kristall_schwebt_im_vanilla_bereich() {
        for i in 0..400 {
            let y = end_crystal_y_offset(i as f32 * 0.37);
            assert!((-1.4 - 1e-4..=-0.6 + 1e-4).contains(&y), "{y}");
        }
    }

    #[test]
    // Bodenplatte: 12x4x12 Pixel ab y=0, mal 2 und um -0.5 verschoben -> 1.5 Bloecke breit,
    // von einem Block unter bis einen halben Block unter der Entity-Position.
    fn bodenplatte_sitzt_wie_in_vanilla() {
        let mut out = Vec::new();
        emit(&end_crystal_model(true), &Pose::new(), end_crystal_transform([0.0; 3]), [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
        let ohne = {
            let mut o = Vec::new();
            emit(&end_crystal_model(false), &Pose::new(), end_crystal_transform([0.0; 3]), [0.0, 0.0, 1.0, 1.0], 1.0, &mut o);
            o.len()
        };
        let platte = &out[ohne..];
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in platte {
            for i in 0..3 {
                min[i] = min[i].min(v.position[i]);
                max[i] = max[i].max(v.position[i]);
            }
        }
        assert!((min[1] + 1.0).abs() < 1e-4 && (max[1] + 0.5).abs() < 1e-4, "{min:?} {max:?}");
        assert!((max[0] - min[0] - 1.5).abs() < 1e-4);
    }
}

#[cfg(test)]
mod pfeil_tests {
    use super::*;
    use std::f32::consts::PI;

    // Die Zahlen stehen so in ArrowModel.createBodyLayer. Sie von Hand nachzuziehen ist die
    // haeufigste Fehlerquelle bei einem nachgebauten Modell, deshalb hier festgenagelt --
    // inklusive der beiden Dinge, die man leicht uebersieht: die Lagen-Skalierung 0.9 geht
    // MULTIPLIKATIV auf die 0.8 des Federteils, und das Kreuz traegt eine Textur-Streckung.
    #[test]
    fn pfeilmodell_trifft_vanillas_masse() {
        let m = arrow_model(ARROW_TEXTURE);
        assert_eq!(m.tex_size, [32.0, 32.0]);
        let namen: Vec<&str> = m.parts.iter().map(|p| p.name).collect();
        assert_eq!(namen, vec!["back", "cross_1", "cross_2"]);

        let back = &m.parts[0];
        assert_eq!(back.pivot, [-11.0, 0.0, 0.0]);
        assert!((back.rotation[0] - PI / 4.0).abs() < 1e-6);
        assert!((back.scale[0] - 0.72).abs() < 1e-6, "0.8 aus withScale mal 0.9 der Lage, war {}", back.scale[0]);
        // Flach auf X: eine Ebene, kein Kasten.
        assert_eq!(back.cuboids[0].size, [0.0, 5.0, 5.0]);
        assert_eq!(back.cuboids[0].offset, [0.0, -2.5, -2.5]);

        for teil in &m.parts[1..] {
            assert_eq!(teil.pivot, [0.0; 3]);
            assert!((teil.scale[0] - 0.9).abs() < 1e-6);
            // Flach auf Z, 16 lang und 4 hoch -- der Schaft.
            assert_eq!(teil.cuboids[0].size, [16.0, 4.0, 0.0]);
            assert_eq!(teil.cuboids[0].offset, [-12.0, -2.0, 0.0]);
            assert_eq!(teil.cuboids[0].tex_scale, [1.0, 0.8], "texScale des Schafts");
        }
        // Die beiden Ebenen stehen um 90 Grad gegeneinander -- daraus wird das Kreuz.
        let d = m.parts[2].rotation[0] - m.parts[1].rotation[0];
        assert!((d - PI / 2.0).abs() < 1e-6, "Kreuzwinkel {d}");
    }

    // ArrowRenderer dreht nur -- keine Spiegelung, keine -1.501-Verschiebung. Die Probe:
    // bei Yaw 0 und Pitch 0 muss die Spitze (Modell-+X) nach +Z zeigen, Minecrafts Yaw-0-
    // Richtung. Ein vergessenes -90 faellt hier sofort auf.
    #[test]
    fn pfeil_zeigt_bei_yaw_null_nach_plus_z() {
        let t = arrow_transform([0.0, 64.0, 0.0], 0.0, 0.0);
        let spitze = t.apply_direction([1.0, 0.0, 0.0]);
        assert!(spitze[2] > 0.99, "Spitze zeigt nach {spitze:?}, erwartet +Z");
        assert!(spitze[0].abs() < 1e-6 && spitze[1].abs() < 1e-6, "{spitze:?}");
        // Und der Ursprung sitzt auf der Entity-Position, ohne Versatz.
        assert_eq!(t.apply([0.0, 0.0, 0.0]), [0.0, 64.0, 0.0]);
    }
}

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
pub fn sign_base(block: [f32; 3], kind: SignKind, degrees: f32) -> Affine {
    let t = Affine::translation(block);
    match kind {
        SignKind::Hanging(_) => t
            .then(Affine::translation([0.5, 0.9375, 0.5]))
            .then(Affine::rotation_y_deg(-degrees))
            .then(Affine::translation([0.0, -0.3125, 0.0])),
        _ => {
            let b = t.then(Affine::translation([0.5, 0.75 * (2.0 / 3.0), 0.5])).then(Affine::rotation_y_deg(-degrees));
            if kind == SignKind::Wall {
                b.then(Affine::translation([0.0, -0.3125, -0.4375]))
            } else {
                b
            }
        }
    }
}

// renderSign: die Modellskalierung auf sign_base (2/3 bzw. 1, jeweils y und z gespiegelt).
pub fn sign_model_transform(base: Affine, kind: SignKind) -> Affine {
    let s = if matches!(kind, SignKind::Hanging(_)) { 1.0 } else { 2.0 / 3.0 };
    base.then(Affine::scale([s, -s, -s]))
}
