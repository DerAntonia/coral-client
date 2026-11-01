// Quader, Teile, Modelle, Posen, Affine Transformationen und das Ausgeben der Vertices.

use super::*;

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
    #[must_use] 
    pub const fn new(uv: [f32; 2], offset: [f32; 3], size: [f32; 3]) -> Self {
        Self { offset, size, dilation: [0.0; 3], uv, mirror: false, tex_scale: [1.0, 1.0], faces: 0b11_1111 }
    }

    #[must_use] 
    pub const fn with_tex_scale(mut self, u: f32, v: f32) -> Self {
        self.tex_scale = [u, v];
        self
    }

    #[must_use] 
    pub const fn dilated(mut self, dilation: f32) -> Self {
        self.dilation = [dilation; 3];
        self
    }

    #[must_use] 
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
    pub children: Vec<Self>,
}

impl Part {
    #[must_use] 
    pub const fn new(name: &'static str, pivot: [f32; 3], cuboids: Vec<Cuboid>) -> Self {
        Self { name, pivot, rotation: [0.0; 3], scale: [1.0; 3], cuboids, children: Vec::new() }
    }

    #[must_use] 
    pub const fn scaled(mut self, scale: f32) -> Self {
        self.scale = [scale; 3];
        self
    }

    #[must_use] 
    pub const fn rotated(mut self, rotation: [f32; 3]) -> Self {
        self.rotation = rotation;
        self
    }

    #[must_use] 
    pub fn with(mut self, child: Self) -> Self {
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
    pub(crate) entries: Vec<(&'static str, PartPose)>,
}

impl Pose {
    #[must_use] 
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

    #[must_use] 
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
    pub const IDENTITY: Self = Self { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    #[must_use] 
    pub const fn translation(t: [f32; 3]) -> Self {
        Self { m: Self::IDENTITY.m, t }
    }

    #[must_use] 
    pub const fn scale(s: [f32; 3]) -> Self {
        Self { m: [[s[0], 0.0, 0.0], [0.0, s[1], 0.0], [0.0, 0.0, s[2]]], t: [0.0; 3] }
    }

    // Drehung um die Y-Achse in Grad.
    #[must_use] 
    pub fn rotation_y_deg(degrees: f32) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Self { m: [[cos, 0.0, sin], [0.0, 1.0, 0.0], [-sin, 0.0, cos]], t: [0.0; 3] }
    }

    // ModelPart.applyTransform: Quaternionf().rotationZYX(roll, yaw, pitch) -- also erst um
    // X (pitch), dann Y (yaw), dann Z (roll) angewandt, als Matrix Rz * Ry * Rx.
    #[must_use] 
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
    #[must_use] 
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
    #[must_use] 
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
    #[must_use] 
    pub fn then(self, other: Self) -> Self {
        let mut m = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = (0..3).map(|k| self.m[i][k] * other.m[k][j]).sum();
            }
        }
        Self { m, t: self.apply(other.t) }
    }

    // Die Umkehrung: inverse.then(self) ist die Einheit. Die Matrix darf skalieren.
    #[must_use] 
    pub fn inverse(self) -> Self {
        let a = self.m;
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        let d = if det.abs() > 1e-12 { 1.0 / det } else { 0.0 };
        let m = [
            [
                (a[1][1] * a[2][2] - a[1][2] * a[2][1]) * d,
                (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * d,
                (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * d,
            ],
            [
                (a[1][2] * a[2][0] - a[1][0] * a[2][2]) * d,
                (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * d,
                (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * d,
            ],
            [
                (a[1][0] * a[2][1] - a[1][1] * a[2][0]) * d,
                (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * d,
                (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * d,
            ],
        ];
        let inv = Self { m, t: [0.0; 3] };
        let t = inv.apply(self.t);
        Self { m, t: [-t[0], -t[1], -t[2]] }
    }

    // Eine RICHTUNG transformieren: nur die Drehmatrix, ohne die Verschiebung. Fuer
    // Flaechen-Normalen (Vanilla: MatrixStack.Entry.transformNormal).
    #[must_use] 
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

    #[must_use] 
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
#[must_use] 
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
#[must_use] 
pub fn rotation_x_deg(degrees: f32) -> Affine {
    let (sin, cos) = degrees.to_radians().sin_cos();
    Affine { m: [[1.0, 0.0, 0.0], [0.0, cos, -sin], [0.0, sin, cos]], t: [0.0; 3] }
}

// Drehung um die Z-Achse in Grad -- gebraucht fuer die Seitwaertsneigung des Umhangs.
#[must_use] 
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
    pub const NONE: Self = Self { amount: 0.0, pitch_deg: 0.0, in_water: false, swimming: false };
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
#[must_use] 
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

// Ein sterbendes Lebewesen (LivingEntityRenderer.setupRotations, Zweig deathTime > 0): nach
// der Koerperdrehung um Z gekippt, in gut 12 Ticks auf die Seite (sqrt-Kurve, bis 90 Grad).
#[must_use] 
pub fn entity_transform_dying(feet: [f32; 3], body_yaw_deg: f32, scale: f32, model_scale: f32, death_time: f32) -> Affine {
    let h = ((death_time - 1.0) / 20.0 * 1.6).max(0.0).sqrt().min(1.0);
    Affine::translation(feet)
        .then(Affine::scale([scale, scale, scale]))
        .then(Affine::rotation_y_deg(180.0 - body_yaw_deg))
        .then(rotation_z_deg(h * 90.0))
        .then(Affine::scale([-1.0, -1.0, 1.0]))
        .then(Affine::scale([model_scale, model_scale, model_scale]))
        .then(Affine::translation([0.0, -1.501, 0.0]))
}

// Elytra-Gleitflug eines Lebewesens -- alles, was Vanilla dafuer aus dem Entity liest
// (LivingEntityRenderState: glidingTicks, pitch, velocity; PlayerEntityRenderer.updateRenderState:
// flyingRotation).
#[derive(Clone, Copy, Debug)]
pub struct Glide {
    /// `LivingEntity.getGlidingTicks()` samt Zwischentick-Anteil: Ticks seit Beginn des Flugs.
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
    #[must_use] 
    pub fn progress(&self) -> f32 {
        (self.ticks * self.ticks / 100.0).clamp(0.0, 1.0)
    }

    /// BipedEntityModel.setAngles: nach vier Ticks Flug schaut der Kopf fest 45 Grad nach vorn
    /// (in Flugrichtung), statt der Blickneigung zu folgen.
    #[must_use] 
    pub fn head_down(&self) -> bool {
        self.ticks > 4.0
    }

    /// BipedEntityRenderer.getLimbAmplitudeInverse: je schneller der Flug, desto weniger
    /// schlenkern Arme und Beine (die Schrittanimation wird durch diesen Wert geteilt).
    #[must_use] 
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
    #[must_use] 
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
#[must_use] 
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
pub(crate) const DIFFUSE_LIGHT_0: [f32; 3] = [0.161_690_2, 0.808_451, -0.565_915_7];
pub(crate) const DIFFUSE_LIGHT_1: [f32; 3] = [-0.161_690_2, 0.808_451, 0.565_915_7];
pub(crate) const LIGHT_POWER: f32 = 0.6;
pub(crate) const AMBIENT_LIGHT: f32 = 0.4;

#[must_use] 
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
        emit_part(part, pose, base, model.tex_size, tex, color, false, out);
    }
}

// Wie emit_tinted, aber beidseitig (s. emit_two_sided): Vanilla zeichnet getragene Ruestung und
// ihren Besatz mit armorCutoutNoCull (EquipmentLayerRenderer), also ohne Culling.
#[allow(clippy::too_many_arguments)]
pub fn emit_tinted_two_sided(model: &Model, pose: &Pose, base: Affine, tex: TexRect, tint: [f32; 3], brightness: f32, out: &mut Vec<BlockVertex>) {
    let color = [tint[0] * brightness, tint[1] * brightness, tint[2] * brightness];
    for part in &model.parts {
        emit_part(part, pose, base, model.tex_size, tex, color, true, out);
    }
}

// Wie emit(), aber beidseitig: Vanilla zeichnet Spieler (auch die Ego-Hand) mit
// entity_translucent, ohne Culling (withCull(false)). Durch leere Pixel der aeusseren Schicht
// sieht man so die Innenseite der gegenueberliegenden Flaeche. Die Rueckseite bekommt wie in
// Vanillas entity.fsh (PER_FACE_LIGHTING, gl_FrontFacing) das Licht der umgedrehten Normale.
// Hier als zweites Dreieck mit umgekehrter Wicklung, die Pipeline verwirft weiter Rueckseiten.
pub fn emit_two_sided(model: &Model, pose: &Pose, base: Affine, tex: TexRect, brightness: f32, out: &mut Vec<BlockVertex>) {
    for part in &model.parts {
        emit_part(part, pose, base, model.tex_size, tex, [brightness; 3], true, out);
    }
}

// Wie emit(), aber jeder Kasten in einer eigenen Liste und ohne Toenung -- fuer Inventar-Icons,
// die je Kasten die Aussenseite ihrer Flaechen bestimmen (s. app::item_icons).
#[must_use] 
pub fn emit_boxes(model: &Model, pose: &Pose, base: Affine, tex: TexRect) -> Vec<Vec<BlockVertex>> {
    fn teil(part: &Part, pose: &Pose, parent: Affine, tex_size: [f32; 2], tex: TexRect, out: &mut Vec<Vec<BlockVertex>>) {
        if pose.get(part.name).hidden {
            return;
        }
        let world = part_world_transform(part, pose, parent);
        for cuboid in &part.cuboids {
            let mut kasten = Vec::new();
            emit_cuboid(cuboid, world, tex_size, tex, [1.0; 3], false, &mut kasten);
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
#[must_use] 
pub fn part_transform(model: &Model, pose: &Pose, base: Affine, name: &str) -> Option<Affine> {
    model.parts.iter().find_map(|part| find_part_transform(part, pose, base, name))
}

pub(crate) fn find_part_transform(part: &Part, pose: &Pose, parent: Affine, name: &str) -> Option<Affine> {
    let world = part_world_transform(part, pose, parent);
    if part.name == name {
        return Some(world);
    }
    part.children.iter().find_map(|child| find_part_transform(child, pose, world, name))
}

// Der gemeinsame Kern von emit_part und part_transform: Pivot + Drehung eines Teils, auf die
// Elterntransformation gesetzt. Bewusst EINE Stelle -- liefen die beiden auseinander, saesse
// das Item neben der Hand statt darin, und zwar nur in bestimmten Posen.
pub(crate) fn part_world_transform(part: &Part, pose: &Pose, parent: Affine) -> Affine {
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
    let local = if part.scale == [1.0; 3] { local } else { local.then(Affine::scale(part.scale)) };
    parent.then(local)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_part(part: &Part, pose: &Pose, parent: Affine, tex_size: [f32; 2], tex: TexRect, color: [f32; 3], two_sided: bool, out: &mut Vec<BlockVertex>) {
    if pose.get(part.name).hidden {
        return;
    }
    let world = part_world_transform(part, pose, parent);

    for cuboid in &part.cuboids {
        emit_cuboid(cuboid, world, tex_size, tex, color, two_sided, out);
    }
    for child in &part.children {
        emit_part(child, pose, world, tex_size, tex, color, two_sided, out);
    }
}

// ModelPart$Cuboid: acht Ecken, sechs Seiten, UV-Spalten/-Zeilen aus der "Kreuz"-Abwicklung
// eines Kastens. Vertex- und UV-Zuordnung exakt wie in Vanilla -- eine andere Zuordnung
// dreht oder spiegelt einzelne Seiten, was man bei einem Skin sofort sieht (Gesicht am
// Hinterkopf).
pub(crate) fn emit_cuboid(cuboid: &Cuboid, world: Affine, tex_size: [f32; 2], tex: TexRect, base_color: [f32; 3], two_sided: bool, out: &mut Vec<BlockVertex>) {
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
        let normale = world.apply_direction(normals[face_index]);
        let shade = diffuse_shade(normale);
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
        if two_sided {
            let hinten = diffuse_shade([-normale[0], -normale[1], -normale[2]]);
            let b = e.map(|v| BlockVertex { color: [base_color[0] * hinten, base_color[1] * hinten, base_color[2] * hinten], ..v });
            out.extend_from_slice(&[b[0], b[2], b[1], b[0], b[3], b[2]]);
        }
    }
}

// Modelltextur-Pixel -> normierte Atlas-UV.
pub(crate) fn atlas_uv(u: f32, v: f32, tex_size: [f32; 2], tex: TexRect) -> [f32; 2] {
    let [u0, v0, u1, v1] = tex;
    [u0 + (u / tex_size[0]) * (u1 - u0), v0 + (v / tex_size[1]) * (v1 - v0)]
}
