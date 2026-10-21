// Vanilla-getreue Bewegungsphysik, komplett in Rust nachgebaut (kein Server-Code, keine
// Java-Klassen zur Laufzeit). Grund: ein serverseitiges Anticheat vergleicht die
// Positions-Pakete des Clients gegen genau die Formeln, die der echte Vanilla-Client
// benutzt -- jede eigene Vereinfachung (andere Beschleunigung, anderer Luftwiderstand,
// anderer Sprung) waere ein Erkennungsmerkmal fuer "das ist kein normaler Spieler".
//
// Konstanten und Ablaufreihenfolge sind am entpackten 1.21.11-Client nachgesehen
// (LivingEntity.travelInAir/travelInWater/travelInLava/jumpFromGround/jumpInLiquid/
// goDownInWater/moveRelative/getFrictionInfluencedSpeed/getFluidFallingAdjustedMovement,
// Entity.getInputVector/collide/updateFluidHeightAndDoFluidPushing, FlowingFluid.getHeight/
// getOwnHeight/getLegacyLevel -- via javap/Vineflower aus dem lokalen Fabric-Loom-Cache
// nachgesehen). Was noch NICHT nachgebildet ist:
// - Block-spezifische Reibung (Eis o.ae.): jeder Block zaehlt hier mit Standardreibung 0.6
//   (Erde/Stein/Gras), unabhaengig von seiner Form. Die Form selbst (s. world::block_shapes,
//   aus den echten Vanilla-Modell-Elementen) wird dagegen beruecksichtigt -- eine Stufe
//   kollidiert nur mit ihrer halben Hoehe, nicht mehr als Vollwuerfel.
// - Auto-Step (vanilla steigt pro Tick bis zu 0.6 Bloecke automatisch hoch, z.B. Stufen):
//   fehlt bewusst, macht die Bewegung nur unbequemer (man muss ueber Stufen springen),
//   nicht unplausibler -- also kein Anticheat-Risiko, nur Komfort. Kann bei Bedarf ergaenzt
//   werden.
// - Wasserstroemung (FluidState.getFlow -- schiebt seitlich in Fliessrichtung): fehlt
//   weiterhin.
// jumpOutOfFluid() (LivingEntity.resetVerticalVelocityInFluid(), s. jump_out_of_fluid) war
// hier lange als "Komfort/Kosmetik, kein Anticheat-Risiko" eingestuft und deshalb bewusst
// weggelassen -- FALSCH: echter Vanilla-Mechanismus ("water hopping"), bei dem
// wiederholte Horizontalkollision im Wasser (z.B. gegen eine Kante schwimmen) den festen
// 0.3-Vertikal-Kick jeden Tick erneut ausloest, kein Sprung-Cooldown gilt dafuer. Jetzt
// nachgebildet.
// - Elytra: Gleitflug (s. tick_glide) und Raketen-Schub (firework_boost) sind nachgebildet.

use glam::DVec3;
use std::sync::OnceLock;
use world::World;

// Ein Tick = 1/20s, exakt wie der Server. Der Aufrufer haengt das per Akkumulator an
// echte Zeit (s. main.rs) -- Geschwindigkeiten hier sind "pro Tick", nicht "pro Sekunde",
// wie in Vanillas eigenem Modell.
pub const TICK_DT: f32 = 1.0 / 20.0;

// MathHelper.sin (Vanilla, net.minecraft.util.math.MathHelper): eine 65536-Eintrag-FLOAT-
// Lookup-Tabelle, KEIN exaktes sin. Vanilla nutzt das ueberall in der Physik, wo aus einem
// Winkel ein Vektor wird (getRotationVector fuer den Schwimm-Pitch-Pull, movementInputToVelocity
// fuer die Yaw-Rotation). Der Unterschied zum exakten f64-sin ist bis zu ~2.6e-5 (Tabellen-
// aufloesung) -- pro Tick winzig, aber ueber viele Schwimm-Ticks akkumuliert er sich in der
// Y-Position und kippt am Wasseroberflaechen-Rand den Wasser/Luft-Dispatch einen Tick zu frueh
// gegenueber Grim (das exakt dieselbe Tabelle spiegelt) -> reproduzierbarer Z-Offset-Flag.
// Deshalb muss unser Client fuer anticheat-genaue Physik DIESELBE Naeherung verwenden.
// Tabelle: SINE_TABLE[i] = (float)Math.sin(i / 10430.378350470453).
fn mc_sine_table() -> &'static [f32; 65536] {
    static TABLE: OnceLock<Box<[f32; 65536]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = Box::new([0.0f32; 65536]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = (i as f64 / 10430.378350470453).sin() as f32;
        }
        t
    })
}

// sin(value) exakt wie MathHelper.sin(double): Index = (long)(value*10430.378350470453) & 65535.
// `as i64` trunkiert Richtung 0 wie Javas (long)-Cast; `& 65535` liefert (auch fuer negative
// Werte, dank Zweierkomplement) einen Index 0..65535, exakt wie `& 65535L` in Java.
pub fn mc_sin(value: f64) -> f32 {
    mc_sine_table()[((value * 10430.378350470453) as i64 & 65535) as usize]
}

// cos(value) exakt wie MathHelper.cos(double): derselbe Tabellen-Zugriff, nur um einen
// Viertel-Umlauf (16384 Eintraege = pi/2) verschoben.
pub fn mc_cos(value: f64) -> f32 {
    mc_sine_table()[((value * 10430.378350470453 + 16384.0) as i64 & 65535) as usize]
}

// getRotationVector().y = -MathHelper.sin(pitch_grad * RADIANS_PER_DEGREE), alles in float.
// Unser camera.pitch ist vorzeichen-invertiert gegenueber Vanillas Pitch (+ = nach oben statt
// unten), deshalb faellt das Minus mit der Vorzeichen-Umkehr weg: look_dir_y = mc_sin(pitch_rad).
// RADIANS_PER_DEGREE ist in Vanilla ein float (0.017453292F), die Multiplikation pitch*rad also
// float -- deshalb hier bewusst ueber f32 gerechnet, nicht exakt f64.
pub fn look_dir_y_from_pitch(pitch_deg: f32) -> f32 {
    const RADIANS_PER_DEGREE: f32 = (std::f64::consts::PI / 180.0) as f32;
    mc_sin((pitch_deg * RADIANS_PER_DEGREE) as f64)
}

const GRAVITY: f64 = 0.08;
const VERTICAL_DRAG: f64 = 0.98;
const JUMP_VELOCITY: f64 = 0.42;
// travelInWater: mit Gunst des Delfins ist die Wasserbremse 0.96 (als float).
const DOLPHINS_GRACE_SLOW_DOWN: f64 = 0.96f32 as f64;
const SPRINT_JUMP_BOOST: f64 = 0.2;
// MOVEMENT_SPEED, Grundwert des Spielers. Nur noch der VORGABEWERT: was wirklich gilt, sagt
// das Attribut-Paket des Servers (Input::movement_speed).
pub const BASE_SPEED: f64 = 0.1;
const SPRINT_SPEED_MULT: f64 = 1.3; // Sprint-Attributmodifikator: ADD_MULTIPLIED_TOTAL 0.3 -> *1.3
// SNEAKING_SPEED, Grundwert des Spielers -- Schleichender Schritt hebt ihn je Stufe um 0.15
// an. Auch das entscheidet das Attribut-Paket (Input::sneaking_speed), nicht diese Konstante.
pub const DEFAULT_SNEAKING_SPEED: f64 = 0.3;
const DEFAULT_BLOCK_FRICTION: f64 = 0.6;
const AIR_ACCEL: f64 = 0.02; // getFlyingSpeed() fuer einen nicht gerittenen Spieler: konstant 0.02

// --- Schnelligkeit und Langsamkeit ------------------------------------------------------
//
// Beide sind KEINE Sonderregel der Bewegung, sondern Modifikatoren auf dasselbe Attribut, aus
// dem auch das Grundtempo kommt (MOVEMENT_SPEED, Grundwert 0.1 = BASE_SPEED). Aus
// StatusEffects.<clinit> abgelesen:
//
//   speed     +0.2  je Stufe, Operation ADD_MULTIPLIED_TOTAL
//   slowness  -0.15 je Stufe, Operation ADD_MULTIPLIED_TOTAL
//
// Beide Werte stehen im Jar als FLOAT und werden erst beim Aufruf zu double geweitet -- als
// double sind das 0.20000000298023224 und -0.15000000596046448, nicht 0.2 und -0.15. Dieselbe
// Falle wie bei CLIMB_SPEED weiter oben, und dieselbe Loesung: den float hinschreiben und
// weiten lassen.
const SPEED_EFFECT_MODIFIER: f64 = 0.2f32 as f64;
const SLOWNESS_EFFECT_MODIFIER: f64 = -0.15f32 as f64;

// EntityAttributes.MOVEMENT_SPEED ist eine ClampedEntityAttribute mit den Grenzen 0 und 1024
// (aus dem Jar). Die Untergrenze ist keine Formsache: ab Langsamkeit VII ist der Faktor
// negativ, und ohne die Klemmung liefe man RUECKWAERTS statt still zu stehen.
const MOVEMENT_SPEED_MIN: f64 = 0.0;
const MOVEMENT_SPEED_MAX: f64 = 1024.0;

/// Der Faktor, mit dem Schnelligkeit und Langsamkeit das Bewegungstempo multiplizieren.
///
/// `speed`/`slowness` sind die Staerken aus dem Paket (0 = Stufe I), `None` = Effekt nicht
/// aktiv. Vanilla rechnet den Modifikator als `grundwert * (staerke + 1)` aus
/// (StatusEffect$EffectAttributeModifierCreator.createAttributeModifier) und legt ihn dann
/// multiplikativ an: `wert *= 1 + modifikator` je Modifikator
/// (EntityAttributeInstance.computeValue). Zwei Effekte multiplizieren sich also, sie addieren
/// sich nicht -- und der Sprint-Zuschlag (+0.3) ist derselbe Operationstyp und multipliziert
/// sich genauso dazu.
pub fn effect_speed_factor(speed: Option<i32>, slowness: Option<i32>) -> f64 {
    let mut faktor = 1.0;
    if let Some(staerke) = speed {
        faktor *= 1.0 + SPEED_EFFECT_MODIFIER * (staerke as f64 + 1.0);
    }
    if let Some(staerke) = slowness {
        faktor *= 1.0 + SLOWNESS_EFFECT_MODIFIER * (staerke as f64 + 1.0);
    }
    faktor
}

// LivingEntity.applyClimbingSpeed(): waagerecht auf +/-0.15 geklemmt, senkrecht nach unten auf
// -0.15 begrenzt. Und LivingEntity.applyMovementInput(): wer beim Klettern gegen die Wand
// drueckt oder springt, steigt mit 0.2 auf. Alle drei sind im Jar FLOATS -- als double sind das
// 0.15000000596046448 bzw. 0.20000000298023224, und genau damit rechnet auch Grim.
const CLIMB_SPEED: f64 = 0.15f32 as f64;
const CLIMB_UP_SPEED: f64 = 0.2f32 as f64;

// Reibung (Slipperiness) je Blockart -- ECHTER BUG gewesen (Grim-Simulation
// flaggte staendig ~2% zu schnelle Bewegung, obwohl unsere Formel nachweislich exakt mit dem
// Vanilla-Code uebereinstimmt): tick_air() nahm bisher IMMER
// DEFAULT_BLOCK_FRICTION (0.6) an, unabhaengig vom tatsaechlichen Block unter den Fuessen --
// die Reibung steckt aber KUBISCH im Nenner der Beschleunigungsformel
// (0.21600002/f^3), eine kleine Abweichung von f wird also stark verstaerkt. Vollstaendige
// Liste aller Vanilla-Bloecke mit abweichender Reibung, aus Blocks nachgesehen
// -- das sind WIRKLICH alle, kein Ausschnitt: die komplette Eis-Familie und
// Slime Block. Jeder andere Block bleibt beim Standardwert 0.6.
fn block_friction(name: &str) -> f64 {
    match name {
        "ice" | "packed_ice" | "frosted_ice" => 0.98,
        "blue_ice" => 0.989,
        "slime_block" => 0.8,
        _ => DEFAULT_BLOCK_FRICTION,
    }
}

// BlockTags.CLIMBABLE, woertlich aus data/minecraft/tags/block/climbable.json des
// 1.21.11-Jars -- keine Auswahl, das ist die vollstaendige Liste. GrimAC liest denselben Tag
// vom Server (SyncedTags.CLIMBABLE), beide Seiten meinen also exakt diese neun Bloecke.
const CLIMBABLE: &[&str] = &[
    "ladder",
    "vine",
    "scaffolding",
    "weeping_vines",
    "weeping_vines_plant",
    "twisting_vines",
    "twisting_vines_plant",
    "cave_vines",
    "cave_vines_plant",
];

// Ist der Block in BlockTags.CLIMBABLE? (Auch fuer die Schrittgeraeusche gebraucht.)
pub fn is_climbable(name: &str) -> bool {
    CLIMBABLE.contains(&name)
}

fn block_at_pos(world: &World, pos: (i32, i32, i32)) -> u32 {
    world.block_at(pos.0, pos.1, pos.2)
}

// AbstractBlock.Settings.velocityMultiplier: bremst die Horizontalgeschwindigkeit am ENDE
// jeder Bewegung (Entity.move, s. move_and_zero). Wie bei der Reibung ist das die
// VOLLSTAENDIGE Liste aus Blocks.java des 1.21.11-Jars -- ein `grep velocityMultiplier(`
// ueber die ganze Datei liefert genau zwei Treffer: Seelensand und Honig, beide 0.4F.
// 0.4F, nicht 0.4: der Faktor ist im Jar ein float und wird als solcher in
// Vec3d.multiply gereicht (0.4000000059604645 als double).
const SOUL_SAND_HONEY_SPEED: f64 = 0.4f32 as f64;

fn block_velocity_multiplier(name: &str) -> f64 {
    match name {
        "soul_sand" | "honey_block" => SOUL_SAND_HONEY_SPEED,
        _ => 1.0,
    }
}

// Block.onEntityLand: die Standardfassung nullt die Y-Geschwindigkeit (`multiply(1, 0, 1)`),
// SchleimBLOCK und BETT drehen sie stattdessen um. Der Rueckgabewert ist der Faktor, mit dem
// zurueckgeworfen wird -- 0.0 heisst "kein Absprung, Standardfassung".
//
//   SlimeBlock.bounce():   setVelocity(x, -y * d, z)            -> Faktor 1.0
//   BedBlock.bounceEntity(): vel.y wird zu -vel.y * 0.66 * d     -> Faktor 0.66
//
// `d` ist 1.0 fuer LivingEntity (also den Spieler) und 0.8 fuer alles andere. 0.66F ist ein
// float, als double 0.6600000262260437 -- GrimAC fuehrt dieselben zwei Werte in
// BlockProperties.getBlockBounceRestitution.
fn block_bounce_restitution(name: &str) -> f64 {
    if name == "slime_block" {
        return 1.0;
    }
    if name.ends_with("_bed") {
        return 0.66f32 as f64;
    }
    0.0
}

// AbstractBlock.Settings.jumpVelocityMultiplier: skaliert die Absprunggeschwindigkeit
// (LivingEntity.getJumpVelocity). Einziger Block im ganzen Jar ist der Honigblock mit 0.5F.
fn block_jump_multiplier(name: &str) -> f32 {
    match name {
        "honey_block" => 0.5,
        _ => 1.0,
    }
}

// travelInWater()/travelInLava(). WATER_MOVEMENT_EFFICIENCY ist ohne Wasserlaeufer 0.0, dann
// gelten die festen Konstanten unten; mit der Verzauberung greift zusaetzlich der
// "h > 0"-Zweig (s. tick_water).
const WATER_SLOW_DOWN: f64 = 0.8; // getWaterSlowDown()
// travelInWater(): Zielwert, auf den WATER_MOVEMENT_EFFICIENCY die Wasserbremse zubewegt
// (die Bremse rueckt um den Anteil h auf 0.54600006 zu). Es ist genau die Landreibung 0.546 -- mit Wasserlaeufer auf
// hoechster Stufe schwimmt man deshalb so, als liefe man an Land.
const WATER_EFFICIENCY_TARGET_DRAG: f64 = 0.54600006f32 as f64;
const WATER_SPRINT_SLOW_DOWN: f64 = 0.9; // travelInWater(): isSprinting() ? 0.9F : ...
const FLUID_ACCEL: f64 = 0.02; // travelInWater/travelInLava: g = 0.02F (moveRelative-Staerke)
const FLUID_VERTICAL_DRAG: f64 = 0.8; // travelInWater: vec32.multiply(f, 0.8F, f)
const LAVA_SLOW_DOWN: f64 = 0.5; // travelInLava: 0.5/0.8F/0.5 bzw. reines 0.5 (s. dort)
                                 // jumpInLiquid()/goDownInWater(): +/-0.04 pro Tick, ohne Cooldown, solange die Taste haelt.
const FLUID_VERTICAL_KICK: f64 = 0.04;
// getFluidJumpThreshold(): eyeHeight<0.4 ? 0.0 : 0.4 -- beim Spieler (EYE_HEIGHT=1.62) immer 0.4.
const FLUID_JUMP_THRESHOLD: f64 = 0.4;
// Entity.travel(): updateMovementInFluid(FluidTags.WATER, 0.014) -- Staerke, mit der die
// Wasserstroemung pro Tick auf die Geschwindigkeit addiert wird.
const WATER_PUSH_SPEED: f64 = 0.014;

// Mit Input::float_drag (Modul Botcheck-Pass) ein Reibungsfaktor so, wie Vanilla ihn als float
// hinschreibt und als double liest (0.98F = 0.9800000190734863), sonst der exakte Wert.
fn float_or(input: &Input, faktor: f64) -> f64 {
    if input.float_drag {
        faktor as f32 as f64
    } else {
        faktor
    }
}
// Lava schiebt in der Overworld deutlich schwaecher, s. Entity.travel (der Wert haengt dort
// an der Dimension; 0.0023333333333333335 ist der Overworld-Fall).
const LAVA_PUSH_SPEED: f64 = 0.0023333333333333335;
// updateMovementInFluid(): unterhalb dieser Eigengeschwindigkeit gilt man als "steht still",
// und ein winziger Schub wird auf FLUID_PUSH_MIN_SPEED hochgezogen (Grenze 0.003).
const FLUID_PUSH_STILL_EPS: f64 = 0.003;
const FLUID_PUSH_MIN_SPEED: f64 = 0.0045000000000000005;

// EntityPose.SLEEPING aus PlayerLikeEntity.POSE_DIMENSIONS: 0.2 x 0.2, Augenhoehe 0.2 --
// dieselbe Box, die world::entity fuer fremde schlafende Spieler nutzt.
pub const SLEEPING_HEIGHT: f64 = 0.2;

pub const PLAYER_WIDTH: f32 = 0.6;
pub const PLAYER_HEIGHT: f32 = 1.8;
pub const EYE_HEIGHT: f32 = 1.62;
// f64-Spiegel der Vorgabemasse, nur noch fuer Tests -- die Physik nimmt die skalierten Masse
// aus BodyAttributes.
#[cfg(test)]
const WIDTH_F64: f64 = PLAYER_WIDTH as f64;
#[cfg(test)]
const HEIGHT_F64: f64 = PLAYER_HEIGHT as f64;

// Pose-abhaengige Kollisionsgroesse -- NICHT (wie eine fruehere Fassung annahm) eine
// dynamische Anpassung an die verfuegbare Deckenhoehe, sondern eine FESTE Tabelle pro Pose,
// Nach PlayerLikeEntity.POSE_DIMENSIONS (der tatsaechlichen Oberklasse von PlayerEntity --
// eine fruehere Suche in PlayerEntity/LivingEntity direkt hat sie deshalb uebersehen) aus dem
// lokalen Yarn-1.21.11-Jar nachgesehen:
//   STANDING:  0.6 x 1.8, Augenhoehe 1.62
//   CROUCHING: 0.6 x 1.5, Augenhoehe 1.27
//   SWIMMING:  0.6 x 0.6, Augenhoehe 0.4  (auch GLIDING/SPIN_ATTACK, hier nicht relevant)
// SWIMMING ist aktiv, solange isSwimming() true ist (s. Entity.updateSwimming(), per
// PlayerEntity.isSwimming() zusaetzlich um Fliegen/Spectator bereinigt, hier nicht relevant):
// EINSTIEG braucht Sprinten UND vollstaendig untergetaucht (isUnderWater()), FORTSETZEN nur
// noch Sprinten UND Wasserberuehrung (isInWater()) -- deshalb bleibt man beim Auftauchen an
// der Oberflaeche in der Schwimm-Pose, solange man weiter sprintet.
// Breite bleibt bei ALLEN drei Posen 0.6 (WIDTH_F64) -- nur die Hoehe (und Augenhoehe)
// unterscheidet sich, deshalb keine eigene Breiten-Konstante pro Pose noetig.
const SWIMMING_HEIGHT: f32 = 0.6;
pub const SWIMMING_EYE_HEIGHT: f32 = 0.4;
#[cfg(test)]
const SWIMMING_HEIGHT_F64: f64 = SWIMMING_HEIGHT as f64;
// TridentItem.onStoppedUsing: useRiptide(20, ...) und ein Versatz um 1.1999999 nach oben.
const RIPTIDE_TICKS: u32 = 20;
const RIPTIDE_GROUND_LIFT: f64 = 1.1999999f32 as f64;
const CROUCHING_HEIGHT: f32 = 1.5;
// Ein Tick der Kamera-Augenhoehe (Vanillas Camera.updateEyeHeight): die Hoehe rueckt je Tick
// um die Haelfte des Abstands zur Steh-Augenhoehe heran, also nie ein harter Sprung. Als eigene Funktion, damit die
// Formel pruefbar ist statt nur in der Tick-Schleife der App zu stehen.
pub fn approach_eye_height(aktuell: f32, ziel: f32) -> f32 {
    aktuell + (ziel - aktuell) * 0.5
}

pub const CROUCHING_EYE_HEIGHT: f32 = 1.27;
#[cfg(test)]
const CROUCHING_HEIGHT_F64: f64 = CROUCHING_HEIGHT as f64;

// Verhindert, dass Fliesskomma-Rundung an einer exakten Blockkante haengen bleibt (0.0000001
// statt 0 Bewegung -> "klebt" am Nachbarblock). Gleiche Groessenordnung wie Vanillas 1.0E-7
// in collide() -- seit die Physik in f64 rechnet (s.u.) ist kein grosszuegigerer Wert mehr
// noetig. WICHTIG f64 statt f32: in f32 rundet die Position beim buendigen Anliegen an einer
// Wand (pos = Blockkante - 0.3) bei Koordinaten ab ~256 um bis zu 6e-5 -- teils IN den Block.
// Der 1.21.2+-Server prueft die Client-Box aber mit nur 1e-5 Toleranz gegen die Blockwelt
// (isPlayerCollidingWithAnythingNew) und teleportiert bei Ueberlappung kommentarlos zurueck:
// genau der beobachtete "Setback beim Wandkontakt". Vanilla rechnet f64 -- wir jetzt auch.
const EPS: f64 = 1.0e-7;

// Groesste bekannte Kollisionshoehe minus 1 Zelle: Zaun/Mauer/Zauntor reichen bis 1.5
// Bloecke hoch (s. world::vanilla_shapes), 0.5 davon ragt in die Zelle darueber. Der
// Block-Scan in sweep_axis muss diese darunterliegende Zelle deshalb mit abdecken, s. dort.
const MAX_COLLISION_OVERFLOW_BLOCKS: f64 = 0.5;

// Maximale Kantenhoehe, die der Spieler im Laufen ohne Sprung ueberwindet ("Stepping").
// Vanillas EntityAttributes.STEP_HEIGHT -- ClampedEntityAttribute mit Default 0.6, vom
// Spieler NICHT ueberschrieben (PlayerEntity.createPlayerAttributes fuegt kein step_height
// hinzu, LivingEntity.getStepHeight liest schlicht das Attribut). 0.6 deckt Stufen/Slabs
// (0.5), Teppiche und Trapdoors ab, aber keinen ganzen Block (1.0) -- daher muss man auf
// einen vollen Block weiterhin springen.
const STEP_HEIGHT: f64 = 0.6;
// Vorgabe von JUMP_STRENGTH: 0.42 als float (Attributes, am Jar abgelesen).
const DEFAULT_JUMP_STRENGTH: f64 = 0.42f32 as f64;

// Die Koerper-Attribute vom Server (s. packets::AttributeUpdate). Vanilla setzt sie selbst
// kaum (movement_efficiency: Seelenlaeufigkeit auf Seelensand), Plugins und /attribute
// dagegen gern -- und GrimAC rechnet mit allen. Liegen in PlayerPhysics statt in Input, weil
// Groesse und Stufenhoehe auch ausserhalb des Ticks gebraucht werden (Kamera, Trefferbox).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyAttributes {
    // GRAVITY, LivingEntity.getDefaultGravity.
    pub gravity: f64,
    // JUMP_STRENGTH, LivingEntity.getJumpPower: als float mit dem Blockfaktor multipliziert.
    pub jump_strength: f64,
    // MOVEMENT_EFFICIENCY, LivingEntity.getBlockSpeedFactor: Mth.lerp(eff, Blockfaktor, 1).
    pub movement_efficiency: f64,
    // SCALE, LivingEntity.getScale: EntityDimensions.scale -- Breite, Hoehe und Augenhoehe
    // jeder Pose (ausser Schlafen) mal diesen float.
    pub scale: f32,
    // STEP_HEIGHT, LivingEntity.maxUpStep: Stufen UND Kantenschutz beim Schleichen.
    pub step_height: f64,
}

impl Default for BodyAttributes {
    fn default() -> Self {
        Self { gravity: GRAVITY, jump_strength: DEFAULT_JUMP_STRENGTH, movement_efficiency: 0.0, scale: 1.0, step_height: STEP_HEIGHT }
    }
}

impl BodyAttributes {
    // STEP_HEIGHT aus dem Paket (double). maxUpStep liefert es als float -- die Vorgabe bleibt
    // aber beim exakten STEP_HEIGHT, damit sich ohne Plugin nichts aendert.
    pub fn set_step_height(&mut self, v: f64) {
        self.step_height = if v == STEP_HEIGHT { STEP_HEIGHT } else { v as f32 as f64 };
    }

    // Eine Posenmasszahl (Breite, Hoehe, Augenhoehe) skaliert -- in float wie EntityDimensions.
    fn scaled(&self, v: f32) -> f32 {
        v * self.scale
    }

    pub fn width(&self) -> f64 {
        self.scaled(PLAYER_WIDTH) as f64
    }
}

#[derive(Clone, Copy)]
pub struct Input {
    // -1..1, unnormalisiert wie Vanillas getMoveVector() (W=+1/S=-1 usw.) -- die
    // Laengenbegrenzung auf 1 passiert erst in tick(), nicht hier.
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint_held: bool,
    // Ob gerade ein Item DURCHGEHEND benutzt wird (Essen, Bogen, Schild -- s.
    // world::item_registry::is_used_continuously). Vanilla daempft die Bewegungseingabe dann
    // zusaetzlich mit getActiveItemSpeedMultiplier(), Standard 0.2.
    pub using_item: bool,
    // use_effects des benutzten Items: Tempo-Faktor (Standard ITEM_USE_SPEED_MULT) und ob man
    // dabei sprinten darf (Speer-Ansturm).
    pub item_use_speed: f64,
    pub item_use_can_sprint: bool,
    // Nahrungsstand des Servers (Set Health). Vanillas Sprint-Bedingung dahinter ist
    // PlayerEntity.canSprintOrFly() -> HungerManager.canSprint() -> `foodLevel > 6`.
    pub food_level: i32,
    // Blindheit (Update/Remove Mob Effect). ClientPlayerEntity.canSprint() faengt mit
    // `!hasBlindnessEffect()` an -- blind laesst sich weder starten noch weitersprinten.
    pub blindness: bool,
    // Der Tempofaktor aus Schnelligkeit/Langsamkeit (s. effect_speed_factor). 1.0 = keiner von
    // beiden aktiv. Er kommt fertig ausgerechnet herein, weil die Effektliste beim Spieler
    // liegt und nicht in der Physik.
    pub effect_speed_factor: f64,
    // Die drei Bewegungs-Attribute vom Server (s. app::attr_movement_speed). Sie kommen
    // fertig herein, weil die Attributverwaltung beim Spieler liegt und nicht in der Physik:
    //   movement_speed             MOVEMENT_SPEED, Vorgabe BASE_SPEED (0.1). OHNE Sprint und
    //                              ohne Schnelligkeit/Langsamkeit -- die stehen weiter in
    //                              effect_speed_factor bzw. im Sprint-Zweig.
    //   sneaking_speed             SNEAKING_SPEED, Vorgabe 0.3. Schleichender Schritt.
    //   water_movement_efficiency  WATER_MOVEMENT_EFFICIENCY, Vorgabe 0.0. Wasserlaeufer.
    pub movement_speed: f64,
    pub sneaking_speed: f64,
    pub water_movement_efficiency: f64,
    // Bewegungs-Effekte (Staerke 0 = Stufe I), alle aus LivingEntity, am Jar nachgesehen:
    //   slow_falling   getEffectiveGravity: fallend (vel.y <= 0) hoechstens 0.01 statt 0.08
    //   levitation     travelMidAir: statt der Schwerkraft Zug auf 0.05*(Stufe) nach oben
    //   jump_boost     getJumpBoostVelocityModifier: +0.1 je Stufe auf den Sprung
    //   dolphins_grace travelInWater: Wasserbremse 0.96 statt 0.8/0.9
    // Vorher fehlten alle vier -- der Client fiel mit Sanftem Fall z.B. normal schnell und
    // wurde vom Server laufend zurueckgesetzt.
    pub slow_falling: bool,
    pub levitation: Option<i32>,
    pub jump_boost: Option<i32>,
    pub dolphins_grace: bool,
    // Modul Botcheck-Pass: die Reibungsfaktoren als Vanillas float-Konstanten (0.98F, 0.91F,
    // 0.8F, 0.9F, als double gelesen) statt exakter doubles. Der Unterschied liegt bei 1e-9 je
    // Tick -- Grim stoert das nicht, der Fallcheck von anticheat-test.com (LimboAPI)
    // vergleicht die Hoehen aber bitgenau. Gemessen mit pktproxy: Fabric 99.92159999847412,
    // wir 99.9216.
    pub float_drag: bool,
    // Radiant, in Camera::yaw's Konvention (0 = Blick nach +Z, s. camera.rs) -- exakt
    // Vanillas eigene Yaw-Konvention, deshalb keine Vorzeichenumkehr noetig.
    pub yaw_rad: f32,
    // Y-Komponente des normierten Blickvektors (camera.forward().y), positiv = nach oben --
    // entspricht Vanillas getRotationVector().y, s. apply_swim_pitch_pull. Nur die Y-
    // Komponente wird gebraucht, deshalb kein voller Pitch-Winkel als Feld.
    pub look_dir_y: f32,
    // Vanillas Blickwinkel in Grad (Pitch positiv = nach UNTEN), fuer den vollen
    // Rotationsvektor beim Gleiten (s. rotation_vector) -- dort geht Vanilla von den Grad aus,
    // nicht von yaw_rad/look_dir_y.
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    // Traegt der Spieler etwas, womit er gleiten kann? LivingEntity.canGlideWith: ein Item mit
    // GLIDER-Komponente im passenden Slot (die Elytra auf der Brust), das beim naechsten
    // Gebrauch nicht zerbricht. Kommt fertig herein, weil das Inventar beim Spieler liegt.
    pub glider_equipped: bool,
    // Zuschauermodus: Vanillas Player.tick setzt dann noPhysics -- keine Kollision, nie am
    // Boden, kein Herausschieben aus Bloecken.
    pub spectator: bool,
    // Zuschauer-Flug in Bloecken je Tick, ohne Anlauf und Auslauf (s. spectator_velocity).
    // 0 = Vanillas Flug mit Beschleunigung.
    pub spectator_speed: f64,
}

// Vanillas Abilities, vom Server per PlayerAbilities-Paket gesetzt (Login, Spielmodus-Wechsel,
// /fly-Plugins). `flying` aendert auch der Client selbst (Doppelsprung, Landung) und meldet es
// dann zurueck (s. PlayerPhysics::abilities_changed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub may_fly: bool,
    pub instabuild: bool,
    pub fly_speed: f32,
    pub walk_speed: f32,
}

impl Default for Abilities {
    fn default() -> Self {
        Self { invulnerable: false, flying: false, may_fly: false, instabuild: false, fly_speed: 0.05, walk_speed: 0.1 }
    }
}

// LocalPlayer.aiStep: zwei Sprung-Druecke innerhalb dieser Ticks schalten den Flug um.
const FLY_TOGGLE_WINDOW: u32 = 7;

pub struct PlayerPhysics {
    // Fuesse (Boxmitte X/Z, Boxboden Y) -- nicht Augenhoehe, s. eye_pos().
    pub pos: DVec3,
    pub vel: DVec3,
    pub on_ground: bool,
    pub sprinting: bool,
    pub horizontal_collision: bool,
    // Entity.minorHorizontalCollision: horizontalCollision, bei dem die TATSAECHLICH
    // zurueckgelegte Bewegung trotzdem noch fast in die gewollte Richtung zeigt (< 8 Grad) --
    // also ein Streifen an einer Wand, kein Anprall. Nur LocalPlayer berechnet das ueberhaupt
    // (Entity.isHorizontalCollisionMinor gibt sonst immer false zurueck), und es entscheidet
    // allein darueber, ob der Sprint abbricht (s. shouldStopRunSprinting).
    pub minor_horizontal_collision: bool,
    // Rohes isSwimming() (s. Konstanten-Kommentar oben): Einstieg braucht Sprinten +
    // vollstaendig untergetaucht, Fortsetzen nur noch Sprinten + Wasserberuehrung. Eingabe
    // fuer getExpectedPose() UND das apply_swim_pitch_pull-Gate -- NICHT direkt fuer
    // Hitbox/Augenhoehe, s. `swimming` (oeffentliches Feld) unten.
    // isSwimming() -- der ECHTE Sprint-Schwimm-Zustand, nicht die blosse Pose (s.
    // apply_swim_pitch_pull). Die Schwimm-Haltung im Bild haengt dagegen an der Pose
    // (`swimming`).
    pub raw_swimming: bool,
    // isTouchingWater(): beruehrt die Kollisionsbox in diesem Tick Wasser? Fuer die
    // Schwimm-Haltung beim Rendern -- nur im Wasser richtet sich der Koerper nach der
    // Blickneigung, beim Kriechen an Land liegt er waagerecht (s. world::Lean).
    pub touching_water: bool,
    // updatePose()/getExpectedPose() + canChangeIntoPose()-Fitting-Fallback (s.
    // pose_fits-Kommentar): die TATSAECHLICH wirksame Pose, bestimmt Hitbox
    // (current_height()) UND Augenhoehe (eye_pos()). Kann von raw_swimming/Schleichen
    // abweichen, wenn die eigentlich gewuenschte (groessere) Pose an der aktuellen Position
    // nicht passt (z.B. eine Decke direkt ueber dem geschrumpften Schwimm-Koerper) -- OHNE
    // diesen Fallback wuerde das Aufhoeren des Sprintens die Hitbox sofort auf Stehhoehe
    // zurueckspringen lassen, obwohl dafuer physisch kein Platz ist: "man kann seinen Kopf
    // in die Decke clippen, wenn man aufhoert zu sprinten".
    pub swimming: bool,
    pub crouching: bool,
    // shouldSlowDown() == isInSneakingPose() -- das FELD `inSneakingPose`, NICHT die Hitbox-Pose.
    // Vanilla setzt dieses Feld in ClientPlayerEntity.tickMovement() (Zeile 733) VOR
    // input.tick() (Zeile 738), also aus der Sneak-Taste vom ENDE des LETZTEN Ticks --
    // die Bewegungs-Verlangsamung laggt der Taste damit um genau einen Tick nach. Beim
    // Loslassen von Sneak wird also noch EINEN Tick lang mit 0.3 verlangsamt, obwohl die
    // Hitbox (self.crouching, aus updatePose()/getExpectedPose() mit dem AKTUELLEN isSneaking())
    // schon auf Stehen zurueck ist. Genau dieser Ein-Tick-Versatz war der grosse
    // Crouch->Sprint-Uebergangs-Flag (Grim erwartet 0.294*accel, wir schickten 0.98*accel).
    in_sneaking_pose_move: bool,
    // Sneak-Taste vom letzten Tick -- Eingabe fuer den Ein-Tick-Lag von in_sneaking_pose_move.
    prev_sneak: bool,
    // Entity.fallDistance (in 1.21.11 ein double): seit dem letzten Bodenkontakt gefallene
    // Hoehe. Hier NICHT fuer Fallschaden gefuehrt (den rechnet der Server), sondern weil der
    // Sneak-Kantenschutz sie braucht -- s. is_standing_on_surface.
    pub fall_distance: f64,
    // LivingEntity.jumpingCooldown (10 Tick / 0.5s Abklingzeit nach jedem echten Sprung,
    // s. try_jump-Kommentar) -- EIN gemeinsamer Zaehler fuer Boden- UND Wasser-/Lava-
    // Sprung, exakt wie in Vanilla, wo beide durch denselben LivingEntity.jump()-Aufruf
    // laufen.
    jump_cooldown: u8,
    // Settings::jump_cooldown_enabled, vom Aufrufer (main.rs) vor jedem tick() gesetzt.
    // AUS ignoriert jump_cooldown komplett (Sprung immer sofort wieder moeglich) --
    // manche Spieler empfinden die Wasser-Verzoegerung als stoerend.
    // Standard AN (wie Vanilla), damit `new()` (z.B. in Tests) unveraendertes
    // Vanilla-Verhalten liefert, ohne dass jeder Aufrufer das Feld extra setzen muss.
    pub jump_cooldown_enabled: bool,
    // Settings::sneak_edge_protection_enabled, vom Aufrufer (main.rs) vor jedem tick() gesetzt.
    // AUS ueberspringt adjust_for_sneaking komplett (man kann schleichend ueber Kanten fallen).
    // Standard AN (wie Vanilla), damit `new()` (z.B. in Tests) unveraendertes Vanilla-Verhalten
    // liefert, ohne dass jeder Aufrufer das Feld extra setzen muss.
    pub sneak_edge_protection_enabled: bool,
    // LivingEntity.isSleeping() -- vom Aufrufer aus den Metadaten gesetzt (main.rs), weil der
    // Schlafzustand ausschliesslich vom Server kommt (SLEEPING_POSITION). Wirkt hier auf die
    // Pose: EntityPose.SLEEPING ist 0.2 x 0.2 mit 0.2 Augenhoehe. Die Bewegungssperre selbst
    // (PlayerEntity.isImmobile()) macht der Aufrufer, indem er die Eingabe nullt -- genau wie
    // Vanilla, das in tickMovement forwardSpeed/sidewaysSpeed/jumping auf 0 setzt und travel()
    // trotzdem weiterlaufen laesst.
    pub sleeping: bool,
    // LivingEntity.isUsingRiptide(): Bit 0x04 der LIVING_FLAGS, vom Aufrufer aus den eigenen
    // Metadaten gesetzt (main.rs). Vanilla setzt dieses Flag NUR auf dem Server
    // (LivingEntity.useRiptide: `if (!world.isClient) setLivingFlag(4, true)`) -- der Client
    // erfaehrt es also ausschliesslich ueber die Metadaten, genau wie hier. Es entscheidet
    // ueber die Pose SPIN_ATTACK (0.6 hoch, Augenhoehe 0.4).
    pub using_riptide: bool,
    // Die aus dem Flag abgeleitete Pose SPIN_ATTACK (s. update_pose) -- getrennt von
    // `swimming`, weil daran auch Schwimmhaltung und -verhalten haengen.
    pub spin_attack: bool,
    // LivingEntity.riptideTicks: nach dem Loslassen eines Dreizacks mit Sog 20 Ticks lang
    // gesetzt (s. start_riptide), zaehlt in tick_riptide herunter. Solange es laeuft, prallt man
    // an Entities ab und endet an Waenden.
    pub riptide_ticks: u32,
    // LivingEntity.isGliding(): Bit 0x80 der Entity-FLAGS. Der Client setzt es beim Start
    // SELBST (PlayerEntity.startGliding, s. tick) und schickt START_FALL_FLYING; beendet wird
    // es vom Server ueber die Metadaten (main.rs), bei einer Leiter auch lokal. Lehnt der
    // Server den Start ab, schickt er das Flag ausdruecklich wieder als "aus"
    // (LivingEntity.stopGliding setzt es kurz an und wieder aus, damit es als geaendert gilt).
    pub gliding: bool,
    // Die Pose GLIDING (0.6 x 0.6, Augenhoehe 0.4) -- wie bei SPIN_ATTACK getrennt vom Flag,
    // weil die Pose erst am Tick-Ende nachzieht (s. update_pose).
    pub glide_pose: bool,
    // Main.rs schickt daraufhin START_FALL_FLYING (und setzt es zurueck).
    pub glide_start_requested: bool,
    // Sprungtaste vom letzten Tick: gestartet wird nur beim DRUECKEN, nicht beim Halten.
    prev_jump: bool,
    // Entity.supportingBlockPos / forceUpdateSupportingBlockPos: der Block, auf dem man
    // TATSAECHLICH steht -- ermittelt aus einer hauchduennen Box unter den Fuessen, nicht aus
    // floor(y - 0.5). Beides ist nur dasselbe, solange alle Bloecke ringsum gleich hoch sind.
    // Genau deshalb steht das hier: Honig (15/16) und Seelensand (14/16) liegen tiefer als ihre
    // Nachbarn, und wer mit einem Fuss ueber der Mulde steht, wird von der naiven Abfrage dem
    // falschen Block zugeordnet -- s. supporting_block_pos.
    supporting_block: Option<(i32, i32, i32)>,
    force_update_supporting_block: bool,
    // Sneak-TASTE dieses Ticks (ClientPlayerEntity.isSneaking()). Als Feld, weil Vanillas
    // ShapeContext.of(entity) sie an JEDER Kollisionsabfrage mitliest -- auch in Pfaden ohne
    // Input-Parameter (Posen-Pruefung, Teleport-Reset). Gesetzt ganz am Anfang von tick().
    sneak_key: bool,
    // Entity.movementMultiplier: von slowMovement() gesetzt (Spinnennetz), im NAECHSTEN
    // move() auf die Bewegung angewandt und dabei sofort wieder geleert. ZERO = nichts
    // anhaengig.
    //
    // Der Ein-Tick-Versatz ist kein Versehen, sondern der Kern der Mechanik: gesetzt wird der
    // Faktor in tickBlockCollision() NACH travel(), verbraucht wird er im move() des naechsten
    // Ticks. Wer ihn stattdessen sofort anwendet, ist einen Tick zu frueh dran und weicht bei
    // JEDEM Netz-Eintritt und -Austritt von der Server-Simulation ab.
    movement_multiplier: DVec3,
    pub abilities: Abilities,
    // Koerper-Attribute vom Server; App setzt sie vor jedem Tick (s. BodyAttributes).
    pub body: BodyAttributes,
    // Hat der Tick `abilities.flying` umgestellt? Main.rs schickt dann Vanillas
    // ServerboundPlayerAbilities (LocalPlayer.onUpdateAbilities) und setzt es zurueck.
    pub abilities_changed: bool,
    // Player.jumpTriggerTime: Restzeit des Doppelsprung-Fensters (s. FLY_TOGGLE_WINDOW).
    jump_trigger_time: u32,
    // Entity.pistonDeltas/pistonDeltasGameTime: wie weit Kolben einen in DIESEM Tick schon je
    // Achse geschoben haben (s. piston_move).
    piston_deltas: [f64; 3],
    piston_deltas_tick: u64,
    // Die Eingabe des letzten Ticks -- ein Kolbenschub laeuft ausserhalb von tick() und
    // braucht sie fuer dieselben Abfragen wie die eigene Bewegung (s. finish_move).
    last_input: Option<Input>,
}

// Der Teil des Spielerzustands, der einen Server-Teleport ueberlebt (s.
// PlayerPhysics::carry_over_state). Undurchsichtig nach aussen -- main.rs reicht den Wert
// nur durch, statt einzelne Felder zu kennen.
#[derive(Clone, Copy)]
pub struct CarryOverState {
    prev_sneak: bool,
    prev_jump: bool,
    gliding: bool,
    glide_pose: bool,
    swimming: bool,
    crouching: bool,
    on_ground: bool,
    supporting_block: Option<(i32, i32, i32)>,
    force_update_supporting_block: bool,
    abilities: Abilities,
    jump_trigger_time: u32,
    // Vanillas handleMovePlayer fasst den Sprint nicht an. Ging er hier verloren, schickten
    // wir nach jedem Positions-Paket STOP_SPRINTING und liefen langsamer weiter -- auf
    // applepvp (Teleports mit Versatz 0 im Sekundentakt) wie Rubberbanding.
    sprinting: bool,
}

impl PlayerPhysics {
    pub fn new(feet_pos: DVec3) -> Self {
        Self {
            pos: feet_pos,
            vel: DVec3::ZERO,
            on_ground: false,
            sprinting: false,
            horizontal_collision: false,
            minor_horizontal_collision: false,
            raw_swimming: false,
            touching_water: false,
            swimming: false,
            crouching: false,
            in_sneaking_pose_move: false,
            prev_sneak: false,
            fall_distance: 0.0,
            jump_cooldown: 0,
            jump_cooldown_enabled: true,
            sneak_edge_protection_enabled: true,
            sleeping: false,
            using_riptide: false,
            spin_attack: false,
            riptide_ticks: 0,
            gliding: false,
            glide_pose: false,
            glide_start_requested: false,
            prev_jump: false,
            supporting_block: None,
            force_update_supporting_block: false,
            sneak_key: false,
            movement_multiplier: DVec3::ZERO,
            abilities: Abilities::default(),
            body: BodyAttributes::default(),
            abilities_changed: false,
            jump_trigger_time: 0,
            piston_deltas: [0.0; 3],
            piston_deltas_tick: 0,
            last_input: None,
        }
    }

    // Nach einem Server-Teleport (/tp, Setback-Korrektur, Respawn) setzt new() on_ground
    // hart auf false, obwohl die Zielposition sehr oft schon auf einem Block steht (z.B.
    // eine Korrektur waehrend ganz normalen Laufens am Boden). Der naechste echte Tick
    // wuerde das zwar von selbst richtigstellen, aber GENAU dieser eine "false"-Report
    // direkt nach dem Teleport wurde von Grim als GroundSpoof gewertet ("claimed false" --
    // wir behaupten in der Luft zu sein, obwohl wir es nicht sind, und das kaskadierte
    // bei jeder Server-Korrektur zusaetzlich). Deshalb sofort nach dem Reset pruefen, ohne
    // auf den naechsten Tick zu warten -- Sonde mit winziger Abwaertsgeschwindigkeit durch
    // dieselbe Kollisionslogik wie ein echter Tick, aber ohne Position/Geschwindigkeit zu
    // veraendern.
    // Entity.getVelocityAffectingPos() ohne den supportingBlockPos-Cache (den wir nicht
    // fuehren): floor(x), floor(y - 0.500001), floor(z) -- Fallback-Pfad aus
    // getPosWithYOffset(), der greift, wann immer keine gecachte Standflaeche vorliegt.
    // Bei einem Spieler exakt auf einem vollen Block trifft das denselben Block wie der
    // Cache getroffen haette.
    fn ground_friction(&self, world: &World) -> f64 {
        let state = block_at_pos(world, self.velocity_affecting_pos(world));
        world::block_registry::block_name(state).map(block_friction).unwrap_or(DEFAULT_BLOCK_FRICTION)
    }

    // Entity.getVelocityAffectingPos() = getPosWithYOffset(0.500001F): der Block, dessen
    // Reibung und Geschwindigkeitsfaktor gelten.
    fn velocity_affecting_pos(&self, world: &World) -> (i32, i32, i32) {
        self.pos_with_y_offset(0.500001f32 as f64, world)
    }

    // Vanillas ShapeContext.of(entity), auf das reduziert, was eine Form aendern kann: die
    // Fusshoehe (Box-Unterkante) und isDescending() == isSneaking(). Nur das Geruest liest ihn.
    fn shape_context(&self) -> world::ShapeContext {
        world::ShapeContext { feet_y: self.pos.y, descending: self.sneak_key }
    }

    // Entity.getBlockPos(): die Zelle, IN der die Fuesse stecken (BlockPos.ofFloored(getPos())).
    // Die einzige der drei Abfragen OHNE Standblock-Bezug.
    fn feet_block_pos(&self) -> (i32, i32, i32) {
        (self.pos.x.floor() as i32, self.pos.y.floor() as i32, self.pos.z.floor() as i32)
    }

    // Entity.getLandingPos() = getPosWithYOffset(0.2F): der Block, auf dem man gelandet ist
    // bzw. steht. Fuer den Schleimblock-Absprung und onSteppedOn.
    fn landing_pos(&self, world: &World) -> (i32, i32, i32) {
        self.pos_with_y_offset(0.2f32 as f64, world)
    }

    // Entity.getPosWithYOffset(offset). Der Kern der ganzen Standblock-Buchhaltung:
    //
    //   - OHNE bekannten Standblock der naive Fallback floor(x)/floor(y - offset)/floor(z).
    //   - MIT Standblock kommen X und Z VOM STANDBLOCK, nur Y wird weiterhin aus der
    //     Spielerposition gerechnet. Genau das ist der Unterschied, der bei Honig zaehlt: wer
    //     auf Stein steht und mit einem Fuss ueber die 1/16 tiefere Honig-Mulde ragt, bekommt
    //     naiv den Honig untergeschoben (X/Z aus der Spielermitte), tatsaechlich traegt ihn
    //     aber der Stein. Vanilla UND GrimAC rechnen mit dem Standblock (Grim:
    //     BlockProperties.getOnPos ueber MainSupportingBlockData) -- die naive Fassung wich
    //     an jedem Uebergang auf einen Honigblock um den vollen Faktor 0.4 ab.
    //   - Zaun/Mauer/Zauntor sind ausgenommen: dort zaehlt der Standblock selbst, weil ihre
    //     Kollisionsbox hoeher ist als ihre Zelle und floor(y - offset) sonst darunter zielt.
    //
    // Der Versatz ist in Vanilla ein FLOAT (0.500001F/0.2F), deshalb ueberall ueber f32
    // gereicht -- 0.500001F ist als double 0.5000010132789612, nicht 0.500001.
    fn pos_with_y_offset(&self, offset: f64, world: &World) -> (i32, i32, i32) {
        let Some(p) = self.supporting_block else {
            return (self.pos.x.floor() as i32, (self.pos.y - offset).floor() as i32, self.pos.z.floor() as i32);
        };
        if offset <= 1.0e-5 {
            return p;
        }
        let name = world::block_registry::block_name(block_at_pos(world, p));
        let ist_zaun = name.is_some_and(|n| n.ends_with("_fence") || n == "fence");
        let ist_mauer = name.is_some_and(|n| n.ends_with("_wall"));
        let ist_tor = name.is_some_and(|n| n.ends_with("_fence_gate"));
        if (offset > 0.5 || !ist_zaun) && !ist_mauer && !ist_tor {
            (p.0, (self.pos.y - offset).floor() as i32, p.2)
        } else {
            p
        }
    }

    // Entity.updateSupportingBlockPos(): nach JEDER Bewegung aufgerufen, mit dem Bodenkontakt
    // und der TATSAECHLICH zurueckgelegten Bewegung dieses Ticks.
    //
    // Der zweite Anlauf mit der um die Bewegung zurueckgeschobenen Box ist kein Beiwerk: wer
    // ueber eine Kante hinauslaeuft, hat am Ende des Ticks unter sich nichts mehr, gilt aber
    // noch als am Boden -- dann traegt ihn der Block, ueber dem er zu Beginn des Ticks stand.
    fn update_supporting_block(&mut self, on_ground: bool, movement: DVec3, world: &World, shapes: &world::BlockShapes) {
        if !on_ground {
            self.force_update_supporting_block = false;
            self.supporting_block = None;
            return;
        }
        let half = self.width() / 2.0;
        let min = DVec3::new(self.pos.x - half, self.pos.y - 1.0e-6, self.pos.z - half);
        let max = DVec3::new(self.pos.x + half, self.pos.y, self.pos.z + half);
        let mut gefunden = self.find_supporting_block(min, max, world, shapes);
        if gefunden.is_some() || self.force_update_supporting_block {
            self.supporting_block = gefunden;
        } else {
            let versatz = DVec3::new(-movement.x, 0.0, -movement.z);
            gefunden = self.find_supporting_block(min + versatz, max + versatz, world, shapes);
            self.supporting_block = gefunden;
        }
        self.force_update_supporting_block = gefunden.is_none();
    }

    // CollisionView.findSupportingBlockPos(): unter allen Bloecken, deren Kollisionsform die
    // duenne Box unter den Fuessen schneidet, gewinnt der mit dem kleinsten Abstand von der
    // Spielerposition zur BLOCKMITTE (Vec3i.getSquaredDistance(Position) rechnet ueber
    // getSquaredDistanceFromCenter, also +0.5 je Achse). Bei Gleichstand entscheidet Vanillas
    // BlockPos-Ordnung (erst Y, dann Z, dann X), und zwar so, dass der GROESSERE gewinnt --
    // deshalb hier `bisher < neu` als Ersetzungsbedingung, 1:1 wie im Jar.
    fn find_supporting_block(&self, min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> Option<(i32, i32, i32)> {
        let mut best: Option<(i32, i32, i32)> = None;
        let mut best_dist = f64::MAX;
        for bx in min.x.floor() as i32..=max.x.floor() as i32 {
            for by in min.y.floor() as i32..=max.y.floor() as i32 {
                for bz in min.z.floor() as i32..=max.z.floor() as i32 {
                    let state = world.block_at(bx, by, bz);
                    let (odx, odz) = world::block_name(state)
                        .map(|name| world::block_xz_offset(name, bx, bz))
                        .unwrap_or((0.0, 0.0));
                    let trifft = world::collision_boxes_at(state, shapes, by, self.shape_context()).iter().any(|cbox| {
                        let bmin = DVec3::new(
                            bx as f64 + odx as f64 + cbox.min[0] as f64,
                            by as f64 + cbox.min[1] as f64,
                            bz as f64 + odz as f64 + cbox.min[2] as f64,
                        );
                        let bmax = DVec3::new(
                            bx as f64 + odx as f64 + cbox.max[0] as f64,
                            by as f64 + cbox.max[1] as f64,
                            bz as f64 + odz as f64 + cbox.max[2] as f64,
                        );
                        // Box.intersects: echte Ueberschneidung, blosses Anliegen zaehlt nicht.
                        bmin.x < max.x && bmax.x > min.x && bmin.y < max.y && bmax.y > min.y && bmin.z < max.z && bmax.z > min.z
                    });
                    if !trifft {
                        continue;
                    }
                    let dx = bx as f64 + 0.5 - self.pos.x;
                    let dy = by as f64 + 0.5 - self.pos.y;
                    let dz = bz as f64 + 0.5 - self.pos.z;
                    let dist = dx * dx + dy * dy + dz * dz;
                    let besser = dist < best_dist || (dist == best_dist && best.is_none_or(|b| (b.1, b.2, b.0) < (by, bz, bx)));
                    if besser {
                        best = Some((bx, by, bz));
                        best_dist = dist;
                    }
                }
            }
        }
        best
    }

    // LivingEntity.isClimbing(): entscheidend ist ALLEIN der Block, IN dem die Fuesse stecken
    // (getBlockPos()) -- nicht, ob man eine Leiter beruehrt. Deshalb klettert man auch an einer
    // Ranke, die man nur durchquert, und deshalb hoert das Klettern sofort auf, sobald die
    // Fusszelle wieder Luft ist.
    //
    // Zweiter Fall: eine OFFENE Falltuer direkt ueber einer Leiter, die in dieselbe Richtung
    // zeigt (canEnterTrapdoor) -- der bekannte Vanilla-Trick, um eine Leiter oben mit einer
    // Falltuer abzuschliessen und trotzdem durchzukommen.
    //
    // Gleitend zaehlen Ranken nicht (Block-Tag can_glide_through) -- man fliegt durch sie
    // hindurch, statt an ihnen haengen zu bleiben. NICHT nachgebildet: isSpectator(), weil der
    // Client keinen Spectator-Modus modelliert.
    pub fn is_climbing(&self, world: &World) -> bool {
        let pos = self.feet_block_pos();
        let state = block_at_pos(world, pos);
        let Some(name) = world::block_registry::block_name(state) else { return false };
        if self.gliding && CAN_GLIDE_THROUGH.contains(&name) {
            return false;
        }
        if CLIMBABLE.contains(&name) {
            return true;
        }
        if !name.ends_with("trapdoor") || world::state_props::property(state, "open") != Some("true") {
            return false;
        }
        let unten = block_at_pos(world, (pos.0, pos.1 - 1, pos.2));
        world::block_registry::block_name(unten) == Some("ladder")
            && world::state_props::property(unten, "facing") == world::state_props::property(state, "facing")
    }

    // LivingEntity.applyClimbingSpeed(), aufgerufen ZWISCHEN Beschleunigung und Bewegung:
    //
    //   onLanding();                                  // Fallhoehe zurueck -> kein Fallschaden
    //   x, z auf +/-0.15 geklemmt, y nach unten auf -0.15 begrenzt
    //   if (y < 0 && Fusszelle != Geruest && isHoldingOntoLadder() && Spieler) y = 0;
    //
    // isHoldingOntoLadder() ist beim Spieler isSneaking(), also die Sneak-TASTE: schleichend
    // haelt man sich an der Leiter fest. Das Geruest ist davon ausdruecklich ausgenommen --
    // dort ist Schleichen genau das Mittel zum Absteigen (und laesst zusaetzlich die
    // Deckplatte der Kollisionsform wegfallen, s. vanilla_shapes::scaffolding_boxes).
    fn apply_climbing_speed(&mut self, world: &World) {
        if !self.is_climbing(world) {
            return;
        }
        self.fall_distance = 0.0;
        self.vel.x = self.vel.x.clamp(-CLIMB_SPEED, CLIMB_SPEED);
        self.vel.z = self.vel.z.clamp(-CLIMB_SPEED, CLIMB_SPEED);
        self.vel.y = self.vel.y.max(-CLIMB_SPEED);
        let auf_geruest = world::block_registry::block_name(block_at_pos(world, self.feet_block_pos())) == Some("scaffolding");
        if self.vel.y < 0.0 && !auf_geruest && self.sneak_key {
            self.vel.y = 0.0;
        }
    }

    // Entity.getVelocityMultiplier(): erst der Block, IN dem man steht -- nur wenn der neutral
    // ist (1.0), zaehlt der Block darunter. Steht man in Wasser oder einer Blasensaeule, wird
    // der Block darunter GAR NICHT befragt: Seelensand unter Wasser bremst also nicht (dort
    // schiebt stattdessen die Blasensaeule, s. bubble_columns).
    fn velocity_multiplier(&self, world: &World) -> f64 {
        // PlayerEntity.getVelocityMultiplier: gleitend bremst weder Seelensand noch Honig.
        if self.gliding {
            return 1.0;
        }
        let f = self.block_speed_factor(world);
        // LivingEntity.getBlockSpeedFactor: MOVEMENT_EFFICIENCY hebt die Bremse anteilig auf
        // (Mth.lerp in float). Ohne Attribut bleibt der Faktor unberuehrt.
        if self.body.movement_efficiency == 0.0 {
            return f;
        }
        let eff = self.body.movement_efficiency as f32;
        let f = f as f32;
        (f + eff * (1.0 - f)) as f64
    }

    // Entity.getBlockSpeedFactor: der Bremsfaktor des Blocks, ohne Attribut.
    fn block_speed_factor(&self, world: &World) -> f64 {
        let feet = block_at_pos(world, self.feet_block_pos());
        let name = world::block_registry::block_name(feet);
        let f = name.map(block_velocity_multiplier).unwrap_or(1.0);
        if matches!(name, Some("water") | Some("bubble_column")) {
            return f;
        }
        if f != 1.0 {
            return f;
        }
        world::block_registry::block_name(block_at_pos(world, self.velocity_affecting_pos(world)))
            .map(block_velocity_multiplier)
            .unwrap_or(1.0)
    }

    // Entity.getJumpVelocityMultiplier(): dieselbe Zwei-Stufen-Abfrage wie oben, aber OHNE die
    // Wasser-Ausnahme.
    fn jump_multiplier(&self, world: &World) -> f32 {
        let f = world::block_registry::block_name(block_at_pos(world, self.feet_block_pos())).map(block_jump_multiplier).unwrap_or(1.0);
        if f != 1.0 {
            return f;
        }
        world::block_registry::block_name(block_at_pos(world, self.velocity_affecting_pos(world))).map(block_jump_multiplier).unwrap_or(1.0)
    }

    // Ein neuer Spieler (Login, auch nach einem Serverwechsel hinter einem Proxy, und
    // Respawn): Vanilla baut dann einen frischen LocalPlayer -- er gleitet nicht, steht in
    // normaler Pose, hat keinen Schwung und ist nicht gefallen. Sonst flog man nach einem
    // Wechsel mitten im Elytra-Flug auf dem neuen Server weiter.
    pub fn reset_for_new_player(&mut self) {
        self.gliding = false;
        self.glide_pose = false;
        self.glide_start_requested = false;
        self.vel = DVec3::ZERO;
        self.fall_distance = 0.0;
    }

    // Zustand, den ein Server-Teleport NICHT anfasst (s. Aufrufstelle in main.rs): die
    // Position kommt vom Server, der Tastenzustand und die Koerperhaltung nicht.
    //
    // Sneak-Taste: PlayerPhysics::new() setzt prev_sneak auf false -- der naechste Tick sah
    // nach JEDEM Setback wie ein frischer Tastendruck aus und liess die Schleich-
    // Verlangsamung einen Tick lang weg (daraus wurde eine Setback-Endlosschleife).
    //
    // Pose: GrimAC behaelt player.pose ueber einen Teleport hinweg und baut nur die Box an
    // der neuen Position neu auf (MovementCheckRunner.handleTeleport: `player.boundingBox =
    // getCollisionBoxForPlayer(player, x, y, z)`, KEIN updatePlayerPose). Vanilla kennt
    // ebenfalls keinen Posen-Reset beim Teleport. Wuerden wir hier auf Stehen zuruecksetzen,
    // rechnete der erste Tick nach einem Setback mit 1.8, waehrend Grim weiter 1.5 (bzw. 0.6
    // beim Kriechen) ansetzt -- genau der Hitbox-Versatz, den ein Setback eigentlich
    // beenden soll.
    //
    // BODENKONTAKT (2026-09-22, aus einem Grim-consoledebug hergeleitet): genauso wenig darf
    // ein Teleport on_ground neu bestimmen. Vanillas setValuesFromPositionPacket setzt NUR
    // Position, Delta und Rotation (`entity.setPos`, `setDeltaMovement`, `setYRot/setXRot`,
    // `setOldPosAndRot`) -- `onGround` und `mainSupportingBlockPos` fasst es nicht an, die
    // aendert erst der naechste `move()`. Wir haben hier frueher mit einem Probe-Sweep neu
    // gemessen; nach einer Enderperle landet man in der Luft, also stand on_ground auf false,
    // waehrend Vanilla (und damit Grim) den Bodenkontakt von VOR dem Wurf behielt. Die Folge
    // war ein einziger Tick mit der falschen Reibung: wir 0.91 statt 0.546.
    //
    // Gemessen im Flag: die waagerechte Bewegung im Tick davor war 0.177512968, und
    //     0.177512968 * (0.91 - 0.546) = 0.06461472
    // gegen Grims angezeigte 0.06461471 -- der Rest der Reihe zerfiel danach sauber mit 0.546.
    pub fn carry_over_state(&self) -> CarryOverState {
        CarryOverState {
            prev_sneak: self.prev_sneak,
            prev_jump: self.prev_jump,
            gliding: self.gliding,
            glide_pose: self.glide_pose,
            swimming: self.swimming,
            crouching: self.crouching,
            on_ground: self.on_ground,
            supporting_block: self.supporting_block,
            force_update_supporting_block: self.force_update_supporting_block,
            abilities: self.abilities,
            jump_trigger_time: self.jump_trigger_time,
            sprinting: self.sprinting,
        }
    }

    pub fn restore_carry_over_state(&mut self, state: CarryOverState) {
        self.prev_sneak = state.prev_sneak;
        self.prev_jump = state.prev_jump;
        self.gliding = state.gliding;
        self.glide_pose = state.glide_pose;
        self.swimming = state.swimming;
        self.crouching = state.crouching;
        self.on_ground = state.on_ground;
        self.supporting_block = state.supporting_block;
        self.force_update_supporting_block = state.force_update_supporting_block;
        self.abilities = state.abilities;
        self.jump_trigger_time = state.jump_trigger_time;
        self.sprinting = state.sprinting;
    }

    // PlayerEntity.isStandingOnSurface(stepHeight) -- die Bedingung, unter der der
    // Sneak-Kantenschutz ueberhaupt greift:
    //   isOnGround() || fallDistance < stepHeight && !isSpaceAroundPlayerEmpty(0, 0, stepHeight - fallDistance)
    //
    // ECHTER BUG gewesen: hier stand nur `self.on_ground`. Der zweite Zweig ist aber genau
    // der, der beim Schleichen ueber eine Kante zaehlt -- wer sich schon einen Moment in der
    // Luft befindet (Stufe hinunter, Kante gerade verlassen), aber noch keine halbe
    // Schritthoehe gefallen ist, wird von Vanilla WEITERHIN festgehalten. Ohne diesen Zweig
    // laeuft man in genau diesen Ticks weiter, waehrend die Server-Simulation klemmt.
    fn is_standing_on_surface(&self, world: &World, shapes: &world::BlockShapes) -> bool {
        if self.on_ground {
            return true;
        }
        let step = self.body.step_height;
        self.fall_distance < step && !space_around_player_empty(self.pos, 0.0, 0.0, step - self.fall_distance, world, shapes, self.shape_context(), self.body)
    }

    pub fn refresh_on_ground(&mut self, world: &World, shapes: &world::BlockShapes) {
        let height = self.current_height();
        // on_ground=false: dieser Probe-Sweep darf NIE steppen -- er fragt nur "steht unter
        // mir etwas", und ein Schritt daraus wuerde die Position verfaelschen.
        let (_, _, hit_y_down, _, _) =
            move_and_collide(self.pos, DVec3::new(0.0, -0.01, 0.0), height, false, world, shapes, self.shape_context(), self.body);
        self.on_ground = hit_y_down;
        // Nach einem Teleport gehoert auch der Standblock neu bestimmt -- der alte liegt in
        // aller Regel ganz woanders. Ohne Bewegung dieses Ticks entfaellt der zweite Anlauf.
        self.force_update_supporting_block = true;
        self.update_supporting_block(self.on_ground, DVec3::ZERO, world, shapes);
    }

    // Augenhoehe der aktuellen Pose -- Vanillas Entity.getStandingEyeHeight(), also der
    // eyeHeight-Eintrag der Pose in PlayerLikeEntity.POSE_DIMENSIONS.
    //
    // ACHTUNG: das ist der ZIELWERT. Die KAMERA nimmt ihn nicht direkt, sondern naehert sich
    // ihm pro Tick um die Haelfte an (s. App::tick_camera_eye_height) -- fuer alles andere
    // (Fluessigkeits-/Blickpruefungen) ist dagegen dieser harte Wert der richtige, genau wie
    // in Vanilla.
    pub fn eye_height(&self) -> f32 {
        if self.sleeping {
            return SLEEPING_HEIGHT as f32;
        }
        // SPIN_ATTACK und GLIDING haben dieselben Masse wie SWIMMING (EntityPose-Tabelle in
        // PlayerEntity).
        self.body.scaled(if self.swimming || self.spin_attack || self.glide_pose {
            SWIMMING_EYE_HEIGHT
        } else if self.crouching {
            CROUCHING_EYE_HEIGHT
        } else {
            EYE_HEIGHT
        })
    }

    pub fn eye_pos(&self) -> DVec3 {
        self.pos + DVec3::new(0.0, self.eye_height() as f64, 0.0)
    }

    pub fn current_height(&self) -> f64 {
        if self.sleeping {
            return SLEEPING_HEIGHT;
        }
        self.pose_height(if self.swimming || self.spin_attack || self.glide_pose {
            SWIMMING_HEIGHT
        } else if self.crouching {
            CROUCHING_HEIGHT
        } else {
            PLAYER_HEIGHT
        })
    }

    // Hoehe einer (nicht schlafenden) Pose samt SCALE.
    fn pose_height(&self, h: f32) -> f64 {
        self.body.scaled(h) as f64
    }

    pub fn width(&self) -> f64 {
        self.body.width()
    }

    // isUnderWater(): Augenposition liegt innerhalb einer Wasserzelle, UNTER deren
    // tatsaechlichem Fuellstand (nicht nur "beruehrt" wie fluid_height_scan fuer den Koerper
    // -- der Kopf muss wirklich eingetaucht sein, s. updateSwimming()-Kommentar).
    fn is_underwater(&self, world: &World) -> bool {
        let eye = self.eye_pos();
        let (bx, by, bz) = (eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
        let state = world.block_at(bx, by, bz);
        if world::fluid_kind(state) != Some(world::FluidKind::Water) {
            return false;
        }
        let Some(h) = world::fluid_height_at(world, bx, by, bz) else {
            return false;
        };
        eye.y - by as f64 <= h
    }

    // Ein einzelner Vanilla-Tick. Muss mit fester Schrittweite (TICK_DT) aufgerufen werden.
    pub fn tick(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        // Muss als Allererstes stehen: jede Kollisionsabfrage dieses Ticks baut ihren
        // ShapeContext daraus (s. shape_context).
        self.sneak_key = input.sneak;
        self.last_input = Some(*input);
        // Player.tick, vor aiStep: im Zuschauermodus nie am Boden.
        if input.spectator {
            self.on_ground = false;
        }
        // Flugzustand vom Anfang des Ticks: LocalPlayer.aiStep liest ihn fuer die Schleich-Pose
        // und den Sprint, BEVOR der Doppelsprung ihn umschalten kann.
        let was_flying = self.abilities.flying;
        // shouldTravelInFluid(): (isInWater() || isInLava()) && isAffectedByFluids() &&
        // !canStandOnFluid() -- die letzten beiden sind fuer einen normalen Spieler immer
        // wahr/falsch (nur Spectator bzw. Strider weichen ab, beides hier nicht modelliert),
        // bleibt also nur die Fluessigkeits-Beruehrung. isInWater() gewinnt bei Ueberlappung
        // (travelInFluid() prueft erst Wasser), s. updateFluidHeightAndDoFluidPushing im
        // Client: Fuellstand wird VOR der Bewegung dieses Ticks anhand der Position vom Ende
        // des LETZTEN Ticks bestimmt (baseTick() laeuft vor aiStep()/travel()).
        //
        // WICHTIG, echter Bug gewesen (Flackern, wenn man beim Schwimmen mit dem Kopf
        // anstoesst): der Scan MUSS mit der Box vom Ende des LETZTEN Ticks
        // laufen (self.current_height(), noch VOR dem Pose-Update unten) -- NICHT hart mit
        // der Stehhoehe, wie eine fruehere Fassung annahm. Vanillas eigener
        // updateFluidHeightAndDoFluidPushing() nutzt die tatsaechliche,
        // aktuelle Box, nie eine hartkodierte Stehhoehe. Mit der zu grossen Stehhoehe-Box
        // reichte der Scan bei einer knapp ueber der Schwimm-Pose (0.6) liegenden
        // Unterwasser-Decke manchmal noch in die Deckenzelle (keine Wasserzelle mehr
        // gefunden) und manchmal knapp nicht (Rundungsrauschen an der Zellgrenze) -- die
        // Schwimm-Pose kippte dadurch JEDEN Tick zwischen 0.6 und 1.8, was Position und
        // Kamera wild durcheinanderwarf.
        //
        // Diese Box wird jetzt AUCH fuer die Sprint-Startbedingung unten gebraucht
        // (isPartlyTouchingWater()), muss also VOR der Sprint-Entscheidung stehen.
        let scan_height = self.current_height();
        let half = self.width() / 2.0;
        let box_min = DVec3::new(self.pos.x - half, self.pos.y, self.pos.z - half);
        let box_max = DVec3::new(self.pos.x + half, self.pos.y + scan_height, self.pos.z + half);
        let water_height = fluid_height_scan(box_min, box_max, world, shapes, world::FluidKind::Water);
        let lava_height = fluid_height_scan(box_min, box_max, world, shapes, world::FluidKind::Lava);

        // Stroemungsschub, bevor irgendetwas anderes an der Geschwindigkeit passiert --
        // Vanilla macht das in Entity.updateWaterState(), also in baseTick() und damit vor
        // travel(). Reihenfolge WASSER, dann LAVA: updateWaterState() ruft erst
        // checkWaterState() (Wasser, 0.014) und danach updateMovementInFluid(LAVA, ...).
        // Beide wirken unabhaengig, es ist kein Entweder-Oder -- in einem Block, der beides
        // beruehrt, addieren sich die Schuebe.
        // Player.isPushedByFluid: fliegend treibt keine Stroemung.
        if !was_flying {
            if let Some(scan) = &water_height {
                apply_fluid_push(&mut self.vel, scan, WATER_PUSH_SPEED);
            }
            if let Some(scan) = &lava_height {
                apply_fluid_push(&mut self.vel, scan, LAVA_PUSH_SPEED);
            }
        }

        // updateSwimming(): einmal drin (Kopf war untergetaucht), reicht Sprinten + blosse
        // Wasserberuehrung zum Weiterschwimmen -- deshalb bleibt man beim Auftauchen an der
        // Oberflaeche in der Schwimm-Pose, solange man weiter sprintet.
        let underwater = self.is_underwater(world);

        // ClientPlayerEntity.canStartSprinting()/canSprint(): ein NEUER Sprint laesst sich
        // nicht beginnen, waehrend man Entity.isPartlyTouchingWater() ist -- das ist NICHT
        // "beruehrt ueberhaupt Wasser", sondern per Definition (isTouchingWater() &&
        // !isSubmergedInWater()) NUR die Oberflaechenberuehrung (Fuesse/Koerper im Wasser,
        // Kopf noch drueber). Vollstaendig untergetaucht (Kopf unter Wasser, isSubmergedInWater()
        // true) ist davon ausdruecklich AUSGENOMMEN -- Sprint-Schwimmen mit bereits
        // eingetauchtem Kopf darf also jederzeit neu beginnen, nur an-der-Oberflaeche-Stehen
        // blockiert den Start. Am Yarn-1.21.11-Jar nachgesehen (ClientPlayerEntity.
        // canStartSprinting()/canSprint()); canSprintOrFly() (Hunger) und
        // Blindness/Elytra/Item-Nutzung sind hier nicht modelliert (immer erlaubt), s.
        // Klassenkommentar oben zu bewusst fehlenden Systemen.
        //
        // WICHTIG, echter Bug gewesen (mit Sprintlock liess sich an der
        // Wasseroberflaeche sprinten): ein schon laufender Sprint darf NICHT
        // pauschal beim Hineinlaufen ins Wasser weiterlaufen -- das gilt nur, solange man
        // bereits in der Schwimm-Pose ist (shouldStopSwimSprinting(), deutlich permissiver:
        // reine Wasserberuehrung reicht). Ausserhalb der Schwimm-Pose (normales Laufen, z.B.
        // nur knietief im Wasser ohne dass die Pose kippt) gilt stattdessen
        // shouldStopSprinting() -> canSprint(false), und DAS verlangt genau wie beim Start
        // !isPartlyTouchingWater() -- ein an Land begonnener Sprint stoppt also sofort, sobald
        // auch nur die Fuesse Wasser beruehren, falls die Schwimm-Pose (noch) nicht aktiv ist.
        // was_swimming_pose ist bewusst die Pose vom ENDE des LETZTEN Ticks (self.swimming ist
        // an dieser Stelle noch nicht fuer diesen Tick aktualisiert) -- genau wie Vanillas
        // eigenes isSwimming() beim Sprint-Entscheid VOR der Pose-Neuberechnung dieses Ticks
        // abgefragt wird.
        let partly_touching_water = water_height.is_some() && !underwater;
        let touching_water = water_height.is_some();

        // updateSwimming() (Entity.java:535) laeuft in baseTick() -- und baseTick() laeuft in
        // super.tick() (LivingEntity.tick Zeile 2633) VOR tickMovement() (Zeile 2672), in dem
        // erst die Sprint-Entscheidung (setSprinting) faellt und danach travel() die Bewegung
        // rechnet. isSwimming fuer Physik UND Pose DIESES Ticks wird also noch mit isSprinting
        // vom ENDE des LETZTEN Ticks bestimmt (self.sprinting ist hier absichtlich noch NICHT
        // fuer diesen Tick aktualisiert). Schwimmen greift damit erst genau EINEN Tick NACH dem
        // Sprint-Start.
        //
        // ECHTER BUG gewesen (Wasser-Setback beim schleichenden Reinlaufen, laut Grim
        // consoledebug): vorher stand diese Zeile NACH der Sprint-Entscheidung und nutzte
        // das frische self.sprinting -> wir wandten Schwimm-Physik (Pitch-Pull) im selben Tick
        // an, in dem Sprint startete. Grim (= Vanilla) rechnete fuer diesen Tick noch Crouch-Sink
        // (-0.19992), wir schon Schwimmen (-0.04) -> Offset ~0.16, der sich bei jeder Wieder-
        // holung zum Setback akkumulierte. updateSwimming-CONTINUE (bereits am Schwimmen) braucht
        // nur Sprint + Wasserberuehrung, -START zusaetzlich volles Untertauchen (isSubmergedInWater).
        // Player.updateSwimming: fliegend nie (und Player.isSwimming ist fuer Zuschauer immer aus).
        self.raw_swimming = if was_flying || input.spectator {
            false
        } else if self.raw_swimming {
            self.sprinting && touching_water
        } else {
            self.sprinting && touching_water && underwater
        };

        // ACHTUNG, hier steht bewusst KEIN Pose-Update mehr. Die Hitbox-Pose wird am ENDE
        // von tick() gesetzt (s. update_pose): Vanillas PlayerEntity.tick() ruft updatePose()
        // als allerletzte Anweisung, nach super.tick() und damit nach travel(). Die Pose, mit
        // der ein Tick seine Bewegung rechnet, ist also immer die vom ENDE des VORIGEN Ticks.
        // self.swimming/self.crouching sind an dieser Stelle folglich genau das -- was fuer
        // die Fluessigkeits-Box oben (current_height) und crawling_move unten auch gewollt ist.
        //
        // ECHTER BUG gewesen (Flags beim Schleichen direkt in einen 1.5-Bloecke-Gang und beim
        // Aufstehen aus dem Kriechen): vorher stand die Pose-Aufloesung HIER, also VOR der Bewegung und mit der
        // Sneak-Taste DIESES Ticks. Damit wechselte unsere Hitbox einen ganzen Tick frueher
        // als bei Vanilla/GrimAC. Am Umschalt-Tick rechneten wir mit 1.5, waehrend Grims
        // Simulation (GrimPlayer.boundingBox stammt aus updatePlayerPose vom Ende des VORIGEN
        // Ticks, MovementCheckRunner Zeile 436) noch 1.8 ansetzte: wir kamen in den Gang
        // hinein, Grims Vorhersage stiess an die Decke -> Offset in X/Z -> Simulation-Flag.
        // Beim Aufstehen aus dem Crawlen dasselbe mit umgekehrtem Vorzeichen.
        //
        // Bewegungs-Verlangsamung (in_sneaking_pose_move, s. Feld-Kommentar) hat davon
        // unabhaengig ihren eigenen Zeitpunkt: sie gehoert zu tickMovement, laeuft also VOR
        // der Bewegung, mit den Posen-Pruefungen an der Position vom Ende des letzten Ticks.
        // isInSneakingPose, nach GrimACs PlayerBaseTick (im Grim-Jar nachgesehen) --
        // und die deckt sich mit Vanillas ClientPlayerEntity.tickMovement:
        //
        //   isSlowMovement = !wasFlying && !isSwimming
        //       && canEnterPose(CROUCHING, lastX, lastY, lastZ)
        //       && (wasSneaking || !isInBed && !canEnterPose(STANDING, lastX, lastY, lastZ))
        //     || (pose == SWIMMING || ...) && !wasTouchingWater
        //
        // Zwei Dinge daran sind entscheidend, und beide hatte ich zwischendurch falsch:
        //   - `wasSneaking`, also die Taste vom LETZTEN Tick. Der Faktor laggt der Taste in
        //     BEIDE Richtungen um einen Tick nach, nicht nur beim Loslassen. Ich hatte hier
        //     zeitweise `input.sneak || prev_sneak` stehen ("beim Druecken sofort") -- das war
        //     aus einer Messung geraten, die eine andere Ursache hatte.
        //   - Die Posen-Pruefungen laufen auf der Position vom ENDE des letzten Ticks. Genau
        //     das ist self.pos an dieser Stelle: bewegt wird erst weiter unten.
        let steh_passt = pose_fits(self.pos, self.pose_height(PLAYER_HEIGHT), world, shapes, self.shape_context(), self.body);
        let hock_passt = pose_fits(self.pos, self.pose_height(CROUCHING_HEIGHT), world, shapes, self.shape_context(), self.body);
        // LocalPlayer.aiStep: `crouching = !abilities.flying && ...` -- fliegend schleicht niemand.
        let in_sneaking_pose = !was_flying && !self.raw_swimming && hock_passt && (self.prev_sneak || !steh_passt);
        // shouldSlowDown() = isInSneakingPose() || isCrawling() (ClientPlayerEntity Zeile 641-642)
        // -- BEIDE loesen in applyMovementSpeedFactors den ×0.3-Bewegungsfaktor (SNEAKING_SPEED)
        // aus, nicht nur Sneak. isCrawling() = isInSwimmingPose() && !isTouchingWater() (Entity
        // Zeile 2666-2667): die Pose ist noch SWIMMING, die Fuesse aber schon aus dem Wasser --
        // genau der Wasseraustritts-Tick beim Sprint-Rausschwimmen. Vanillas updatePose() laeuft
        // am ENDE des Ticks (PlayerEntity.tick Zeile 292, NACH super.tick()->tickMovement); die
        // fuer die Bewegung gelesene Pose ist also IMMER die vom ENDE des LETZTEN Ticks -- und
        // genau das ist self.swimming an dieser Stelle (aktualisiert wird es erst unten in
        // update_pose). Deshalb reicht der aktuelle touching_water-Stand (water_height dieses
        // Ticks) als !isTouchingWater.
        //
        // ECHTER BUG gewesen (s. test 13, 0.0178 Z-Offset beim Sprint-Rausschwimmen, algebraisch
        // aus Grims P/A hergeleitet): wir liessen den ×0.3 am Austritts-Tick weg und schickten
        // 0.026×0.98×1.0 = 0.02548 Accel, waehrend Grim (= Vanilla, gecrawlt) 0.026×0.98×0.3 =
        // 0.007644 erwartete -> genau 0.0178 Differenz in Z, danach mit 0.91/Tick abklingend.
        //
        // Dasselbe nach dem Elytra-Flug: LivingEntity.isInSwimmingPose zaehlt auch die Pose
        // GLIDING, sobald das Gleit-Flag aus ist. Beendet der Server den Flug (Landung), steht
        // die Pose im naechsten Tick noch auf GLIDING -- der Tick gilt als Kriechen, mit
        // ×0.3. Sichtbar als Grim-Simulation beim Elytra-Hoppen: 0.0178 zu viel Vorwaerts-
        // Beschleunigung im ersten Luft-Tick nach dem Absprung, danach abklingend.
        let glide_pose_ohne_flug = self.glide_pose && !self.gliding;
        let crawling_move = (self.swimming || glide_pose_ohne_flug) && water_height.is_none();
        self.in_sneaking_pose_move = in_sneaking_pose || crawling_move;
        self.prev_sneak = input.sneak;

        // Diagnose CORAL_SNEAK_DEBUG=1 -- schreibt GENAU die Terme, aus denen sich der
        // ×0.3-Faktor ergibt (s. add_move_input). Anlass: ein consoledebug-Mitschnitt zeigte
        // schleichend dauerhaft "Client input 0.098 / Simulated input 0.0294", der Faktor
        // fehlte also bei uns -- auf ebenem Boden stimmt er aber exakt (s. Test
        // schleichen_ist_exakt_drei_zehntel_der_gehgeschwindigkeit). Die Abweichung haengt
        // folglich am Ort, und nur diese Zeile sagt, welcher der Zweige dort kippt: die
        // Sneak-Taste selbst, die Posen-Aufloesung (passt 1.8/1.5 ueberhaupt?) oder die
        // Schwimm-/Kriech-Sonderfaelle.
        if std::env::var_os("CORAL_SNEAK_DEBUG").is_some() {
            eprintln!(
                "[tick] pos=({:.4},{:.4},{:.4}) vor={:.1} strafe={:.1} sneak={} benutzt={} prev={} crouch={} swim={} raw_swim={} kriechen={} pose_move={} passt1.8={} passt1.5={} faktor={:.4}",
                self.pos.x,
                self.pos.y,
                self.pos.z,
                input.forward,
                input.strafe,
                input.sneak,
                input.using_item,
                self.prev_sneak,
                self.crouching,
                self.swimming,
                self.raw_swimming,
                crawling_move,
                self.in_sneaking_pose_move,
                steh_passt,
                hock_passt,
                0.98 * if input.using_item { input.item_use_speed } else { 1.0 } * if self.in_sneaking_pose_move { DEFAULT_SNEAKING_SPEED } else { 1.0 },
            );
        }

        // Vanillas Herausschieben aus einem Block (LocalPlayer.aiStep, moveTowardsClosestSpace):
        // ohne noPhysics wird an den vier Ecken (x +/- 0.35*Breite, z +/- 0.35*Breite) je einmal
        // Richtung freien Platz geschoben.
        //
        // Die PHASE ist festgelegt: in aiStep direkt nach input.tick() und dem Auto-Sprung,
        // also VOR der Sprint-Entscheidung und lange vor travel() -- s. Modulkommentar zu den
        // vier Tick-Phasen.
        //
        // Nicht mit noPhysics (Zuschauer).
        if !input.spectator {
            for (dx, dz) in [(-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (1.0, 1.0)] {
                let ecke = self.width() * 0.35;
                self.move_towards_closest_space(self.pos.x + dx * ecke, self.pos.z + dz * ecke, world, shapes);
            }
        }

        // Das `!sneak` gilt NUR am Boden/ausserhalb der Schwimm-Pose. ECHTER BUG gewesen (im
        // Schwimmen schleichend schwamm man langsamer als in Vanilla):
        // beim Sprint-Schwimmen stoppt Sneak den Sprint in Vanilla NICHT -- ClientPlayer-
        // Entity.tickMovement prueft waehrend isSwimming() ausschliesslich shouldStopSwim-
        // Sprinting(), und das kennt bei Vorwaertsbewegung keinen Sneak-Stop (nur
        // !canSprint || !touchingWater). Sneak zieht einen also per knockDownwards nach unten,
        // OHNE die Schwimm-Geschwindigkeit zu killen. Vorher haben wir den Sprint faelschlich
        // beendet -> zu langsam -> GrimAC-Simulation-Flag beim Abtauchen.
        //
        // `|| underwater`: ECHTER BUG gewesen (im Wasser Sneak + W fuehrt in Vanilla in den
        // Schwimm-/Sprint-Modus, hier nicht). canStartSprinting() hebt die
        // Sneak-Sperre auf, sobald man vollstaendig untergetaucht ist:
        // `(!shouldSlowDown() || isSubmergedInWater())` (ClientPlayerEntity Zeile 1091). Ohne
        // dieses Oder konnte man untergetaucht + Sneak nie einen Sprint starten -> kam nie in die
        // Schwimm-Pose. underwater == is_underwater() == isSubmergedInWater() (Auge im Wasser).
        // Sprint-Stop-Zweig an Vanillas isSwimming() (= self.raw_swimming, oben mit Ein-Tick-Lag
        // gesetzt) haengen, NICHT an der Schwimm-POSE (self.swimming). ECHTER BUG gewesen (Grim
        // consoledebug 01:29, O~0.0137 am Wasser->Luft-Austritt): am Austritts-Tick ist die Pose
        // noch Schwimmen, isSwimming (raw_swimming) aber schon aus (baseTick sah touching_water=
        // false). Vanilla nutzt dann die LAND-Regel (shouldStopSprinting -> Sprint laeuft weiter),
        // wir nutzten faelschlich die Schwimm-Regel (shouldStopSwimSprinting, touching_water=false
        // -> Stop) und schalteten den naechsten Tick wieder an -> ein-Tick-Sprint-Flacker ->
        // kurzer horizontaler Geschwindigkeitsversatz gegen Grim.
        // ECHTER BUG gewesen (Grim "SprintE", stableKey grim.sprint.wall, aus dem Grim-Jar
        // nachgesehen -- "Sprinting while colliding with a wall"): hier stand `!input.sneak`,
        // also die SNEAK-TASTE. Vanilla fragt an dieser Stelle aber die POSE ab
        // (canStartSprinting: `!shouldSlowDown() || isSubmergedInWater()`, und shouldSlowDown()
        // ist isInSneakingPose() || isCrawling()). Der Unterschied zaehlt genau dort, wo es
        // geflaggt hat: unter einer zu niedrigen Decke erzwingt Vanilla die Schleich-Pose auch
        // OHNE gedrueckte Taste und kann dort folglich keinen Sprint starten. Wir sahen nur die
        // Taste, starteten den Sprint an der Stufe immer wieder neu und meldeten ihn beim
        // Dagegendruecken weiter -- genau das Bild, das SprintE sucht.
        // Deshalb steht die Posen-Aufloesung jetzt VOR dieser Entscheidung: Vanillas
        // tickMovement setzt inSneakingPose als Allererstes, lange vor dem Sprint-Block.
        //
        // Und die Posen-Sperre gehoert AUSSCHLIESSLICH in den Start-Zweig: Vanilla hat sie nur
        // in canStartSprinting(), shouldStopSprinting() kennt keine Sneak-/Posen-Bedingung. Ein
        // LAUFENDER Sprint endet also nicht dadurch, dass man in die Schleich-/Kriech-Pose
        // kommt -- er endet an der Wand, im Wasser oder ohne Vorwaertseingabe.
        // VOLLSTAENDIGER Nachbau von Vanillas Sprint-Block. Zuvor stand hier
        // `wants_sprint = sprint_held && forward > 0` als GEMEINSAME Bedingung fuers Starten
        // UND Stoppen. Das war an drei Stellen falsch:
        //
        //   1. Vanillas Stopp-Bedingungen kennen die SPRINT-TASTE ueberhaupt nicht
        //      (shouldStopSprinting/shouldStopSwimSprinting fragen sie nicht ab). Wer im
        //      Lauf die Taste loslaesst, sprintet in Vanilla WEITER, bis er stehen bleibt
        //      oder gegen etwas laeuft. Wir hoerten sofort auf -- und meldeten das auch so
        //      an den Server.
        //   2. Nahrung und Blindheit fehlten ganz (canSprint -> !hasBlindnessEffect() &&
        //      canSprintOrFly(), und canSprintOrFly ist `foodLevel > 6`).
        //   3. Beim Schwimm-Sprint ist Vanillas Stopp milder: shouldStopSwimSprinting()
        //      beendet ihn ohne Vorwaertseingabe NUR, wenn man dabei weder am Boden ist noch
        //      schleicht (`!hasForwardMovement && !isOnGround && !sneak`).
        //
        // canSprint(allowTouchingWater) aus ClientPlayerEntity, ohne Reittier (kein Fahrzeug
        // im Client) -- die Wasserbedingung kommt je Aufrufstelle dazu.
        // hasEnoughFoodToDoExhaustiveManoeuvres: wer fliegen darf, braucht keinen Hunger-Vorrat.
        let kann_sprinten_grund = !input.blindness && (input.food_level > 6 || self.abilities.may_fly);
        // isSprintingPossible(abilities.flying): fliegend zaehlt die Wasserberuehrung nicht.
        let wasser_sperrt = partly_touching_water && !was_flying;
        // canStartSprinting(): zusaetzlich Vorwaertseingabe, kein benutztes Item (Vanillas
        // isBlockedFromSprinting; UseEffectsComponent.DEFAULT hat can_sprint = false, also
        // sperrt jede laufende Benutzung), nicht in Schleich-/Kriech-Pose, nicht im Wasser.
        let kann_starten = kann_sprinten_grund
            && input.forward > 0.0
            && (!input.using_item || input.item_use_can_sprint)
            && !wasser_sperrt
            && (!self.in_sneaking_pose_move || underwater)
            // canStartSprinting: `(!isGliding() || isSubmergedInWater())`.
            && (!self.gliding || underwater);
        // Nur der START haengt an der Taste -- Vanilla setzt den Sprint nur, wenn
        // canStartSprinting() gilt und die Sprinttaste gehalten wird.
        if kann_starten && input.sprint_held {
            self.sprinting = true;
        }
        if self.sprinting {
            self.sprinting = if self.raw_swimming {
                // shouldStopSwimSprinting(): !canSprint(true) || !touchingWater
                //   || (!forward && !onGround && !sneak). canSprint(true) laesst die
                // Wasserbedingung weg.
                kann_sprinten_grund && touching_water && (input.forward > 0.0 || self.on_ground || input.sneak)
            } else {
                // shouldStopRunSprinting(): !isSprintingPossible(flying) || !hasForwardImpulse()
                //   || (horizontalCollision && !minorHorizontalCollision). isSprintingPossible(false)
                // enthaelt die Wasserbedingung.
                //
                // ECHTER BUG gewesen (GrimAC "SprintE" = grim.sprint.wall, etwa im
                // Kaktus-Labyrinth auf anticheat-test.com): hier stand nur
                // `!self.horizontal_collision`, das minorHorizontalCollision fehlte ganz. Wir
                // brachen den Sprint also auch beim blossen STREIFEN einer Wand ab, wo Vanilla
                // weitersprintet -- und starteten ihn bei gehaltener Sprinttaste im naechsten Tick
                // sofort neu. Dieses Aufblinken (Stop/Start je Tick) ist genau das Muster, das
                // SprintE als "sprintet gegen eine Wand" auslegt, und die Geschwindigkeits-
                // differenz Gehen/Sprinten ging zusaetzlich jeden Tick in Simulation ein.
                kann_sprinten_grund
                    && input.forward > 0.0
                    && !wasser_sperrt
                    && (!self.horizontal_collision || self.minor_horizontal_collision)
            };
        }

        // ClientPlayerEntity.tickMovement, direkt nach dem Sprint-Block: Sprung in der Luft
        // DRUECKEN (nicht halten -- die Taste vom letzten Tick muss los gewesen sein), nicht an
        // einer Leiter, und PlayerEntity.checkGliding: noch nicht am Gleiten, gleitfaehig
        // (canGlide) und nicht im Wasser. Dann startGliding() SOFORT lokal -- die Bewegung
        // dieses Ticks laeuft schon als Gleitflug -- und START_FALL_FLYING an den Server.
        //
        // canGlide: nicht am Boden, nicht auf einem Reittier (gibt es hier nicht), kein
        // Schwebeeffekt, nicht im Kreativflug (gibt es hier nicht), Elytra angelegt.
        let sprung_gedrueckt = input.jump && !self.prev_jump;
        self.prev_jump = input.jump;
        // LocalPlayer.aiStep, direkt nach dem Sprint-Block: Flug umschalten. Zuschauer fliegen
        // immer; sonst schalten zwei Sprung-Druecke im Fenster um (nicht schwimmend), und wer
        // vom Boden aus losfliegt, springt dabei ab.
        let mut flug_umgeschaltet = false;
        if self.abilities.may_fly {
            if input.spectator {
                if !self.abilities.flying {
                    self.abilities.flying = true;
                    flug_umgeschaltet = true;
                    self.abilities_changed = true;
                }
            } else if sprung_gedrueckt {
                if self.jump_trigger_time == 0 {
                    self.jump_trigger_time = FLY_TOGGLE_WINDOW;
                } else if !self.raw_swimming {
                    self.abilities.flying = !self.abilities.flying;
                    if self.abilities.flying && self.on_ground {
                        self.try_jump(input, world);
                    }
                    flug_umgeschaltet = true;
                    self.abilities_changed = true;
                    self.jump_trigger_time = 0;
                }
            }
        }
        // Player.canGlide: nicht im Kreativflug -- und nicht in dem Tick, der ihn umschaltet.
        if sprung_gedrueckt
            && !flug_umgeschaltet
            && !self.abilities.flying
            && !self.is_climbing(world)
            && !self.gliding
            && !self.on_ground
            && input.levitation.is_none()
            && input.glider_equipped
            && !touching_water
        {
            self.gliding = true;
            self.glide_start_requested = true;
        }

        // LocalPlayer.aiStep: fliegend heben Springen und Schleichen um je das Dreifache der
        // Fluggeschwindigkeit (float wie in Vanilla). Danach Player.aiStep: das Fenster laeuft
        // ab, und fliegend gibt es keine Fallhoehe.
        if self.abilities.flying {
            let richtung = input.jump as i32 - input.sneak as i32;
            if richtung != 0 {
                self.vel.y += (richtung as f32 * self.abilities.fly_speed * 3.0) as f64;
            }
        }
        self.jump_trigger_time = self.jump_trigger_time.saturating_sub(1);
        if self.abilities.flying {
            self.fall_distance = 0.0;
        }

        // LivingEntity.tickMovement(): jumpingCooldown wird JEDEN Tick unbedingt
        // runtergezaehlt, danach -- unabhaengig von Wasser/Lava/Luft -- sofort auf 0
        // zurueckgesetzt, sobald Springen nicht (mehr) gehalten wird (s. try_jump-
        // Kommentar). Muss vor dem Wasser/Lava/Luft-Dispatch stehen, weil beide Zweige
        // denselben Zaehler lesen.
        if self.jump_cooldown > 0 {
            self.jump_cooldown -= 1;
        }
        if !input.jump {
            self.jump_cooldown = 0;
        }

        // LivingEntity.tickMovement(): sehr kleine Restgeschwindigkeit JEDEN Tick hart auf
        // 0 klemmen, BEVOR travel() sie weiterverarbeitet -- sonst naehert sich Reibung nur
        // asymptotisch an 0 an und bleibt fuer immer ein winziges Stueck von 0 entfernt.
        // Spieler sind dabei ein Sonderfall: X/Z werden GEMEINSAM genullt (nicht pro Achse
        // einzeln wie bei anderen Entities), sobald die HORIZONTALE Geschwindigkeit
        // (Quadrat) unter 9.0E-6 faellt (= Betrag < 0.003); Y unabhaengig bei |vel.y| <
        // 0.003. Bisher komplett gefehlt -- echter Bug gewesen (dauerhafter
        // kleiner Simulation-Restfehler bei Grim, ~0.004, weil Grims eigene Simulation
        // diesen Schnitt macht und unsere nicht).
        if self.vel.x * self.vel.x + self.vel.z * self.vel.z < 9.0e-6 {
            self.vel.x = 0.0;
            self.vel.z = 0.0;
        }
        if self.vel.y.abs() < 0.003 {
            self.vel.y = 0.0;
        }

        // ZURUECKGENOMMEN: ein Ein-Tick-Lag auf dem Medium-Dispatch
        // (Wasser/Luft) wurde versucht, um einen sehr schmalen Oberflaechen-Sonderfall zu
        // fixen -- hat aber SYMMETRISCH auch den viel haeufigeren Wasser-EINTRITT verzoegert
        // und dadurch insgesamt MEHR Flags ausgeloest, nicht weniger. Nur an einem einzelnen
        // synthetischen Fall verifiziert, nicht gegen normales Spielverhalten getestet -- war
        // ein echter Fehler, nicht nur unbestaetigt. Dispatch bleibt bis auf Weiteres live
        // (water_height/lava_height von diesem Tick), wie vor diesem Versuch.
        // Fuer die Posen-Diagnose (s. unten): Position VOR der Bewegung dieses Ticks.
        let pos_vor_bewegung = self.pos;
        // LivingEntity.tickGliding -> limitFallDistance, vor travel(): solange man nicht
        // steil stuerzt, zaehlt die Fallhoehe hoechstens einen Block.
        if self.gliding && self.vel.y > -0.5 && self.fall_distance > 1.0 {
            self.fall_distance = 1.0;
        }
        self.touching_water = water_height.is_some();
        // Player.travel: fliegend laeuft das normale travel, danach gilt aber die vertikale
        // Geschwindigkeit von VORHER, mal 0.6 -- Schwerkraft und Luftbremse fallen damit weg.
        // Fluessigkeiten wirken fliegend nicht (isAffectedByFluids).
        let fliegt = self.abilities.flying;
        let y_vorher = self.vel.y;
        let zuschauer_flug = input.spectator && input.spectator_speed > 0.0;
        if zuschauer_flug {
            // Entity.move mit noPhysics: einfach versetzen.
            self.vel = spectator_velocity(input);
            self.pos += self.vel;
            self.horizontal_collision = false;
            self.minor_horizontal_collision = false;
        } else if let (Some(g), false) = (&water_height, fliegt) {
            self.tick_water(input, world, shapes, g.height);
        } else if let (Some(g), false) = (&lava_height, fliegt) {
            self.tick_lava(input, world, shapes, g.height);
        } else if self.gliding {
            self.tick_glide(input, world, shapes);
        } else {
            self.tick_air(input, world, shapes);
        }
        if fliegt && !zuschauer_flug {
            self.vel.y = y_vorher * 0.6;
        }

        // LivingEntity.tickMovement(): direkt NACH travel() -- diese Reihenfolge ist der
        // ganze Punkt (s. movement_multiplier). Zuschauer beruehren keine Bloecke.
        if !input.spectator {
            self.tick_block_collision(input, world, shapes);
        }
        // Ende von LocalPlayer.aiStep: wer fliegend aufsetzt, hoert auf zu fliegen (Zuschauer nie).
        if self.on_ground && self.abilities.flying && !input.spectator {
            self.abilities.flying = false;
            self.abilities_changed = true;
        }

        // updatePose() als ALLERLETZTES im Tick, an der Position NACH der Bewegung -- genau
        // wie PlayerEntity.tick() (Zeile 292 im Yarn-1.21.11-Jar) und wie GrimAC
        // (MovementCheckRunner Zeile 436: doBaseTick -> livingEntityAIStep -> updatePlayerPose).
        let was_swimming = self.swimming;
        let was_crouching = self.crouching;
        self.update_pose(input, world, shapes);

        // Posen-Diagnose (CORAL_POSE_DEBUG=1): genau EINE Zeile pro Posenwechsel, mit
        // allem, was noetig ist, um sie gegen GrimACs consoledebug zu legen -- Grims A-Zeile
        // (actualMovement) gehoert zu derselben Position wie `nach` hier, und die Pose, mit
        // der Grim DIESEN Tick gerechnet hat, ist die Spalte `vor`. Wer beim Posenwechsel
        // flaggt, sieht hier sofort, ob unsere Umschaltung einen Tick vor oder hinter Grims
        // liegt, statt dass wieder geraten wird.
        if (was_swimming != self.swimming || was_crouching != self.crouching) && std::env::var_os("CORAL_POSE_DEBUG").is_some() {
            let name = |swim: bool, crouch: bool| {
                if swim {
                    "SWIMMING"
                } else if crouch {
                    "CROUCHING"
                } else {
                    "STANDING"
                }
            };
            eprintln!(
                "[pose] {} -> {} (sneak={} raw_swim={}) hoehe {:.1} -> {:.1} vor=({:.5},{:.5},{:.5}) nach=({:.5},{:.5},{:.5})",
                name(was_swimming, was_crouching),
                name(self.swimming, self.crouching),
                input.sneak,
                self.raw_swimming,
                self.pose_height(if was_swimming {
                    SWIMMING_HEIGHT
                } else if was_crouching {
                    CROUCHING_HEIGHT
                } else {
                    PLAYER_HEIGHT
                }),
                self.current_height(),
                pos_vor_bewegung.x,
                pos_vor_bewegung.y,
                pos_vor_bewegung.z,
                self.pos.x,
                self.pos.y,
                self.pos.z,
            );
        }

        // TEMPORAERES Debug-Logging (CORAL_SWIM_DEBUG=1), um das Flackern beim
        // Kopfanstossen im Schwimmen an echten Spieldaten nachzuvollziehen, statt weiter blind
        // Szenarien zu konstruieren: mehrere synthetische Repros blieben stabil, der Bug trat im echten
        // Spiel aber weiterhin auf.
        if was_swimming != self.swimming && std::env::var_os("CORAL_SWIM_DEBUG").is_some() {
            eprintln!(
                "[swim] {} -> {} pos={:?} sprinting={} water_height={:?} underwater={} eye_y={:.4}",
                was_swimming,
                self.swimming,
                self.pos,
                self.sprinting,
                water_height.as_ref().map(|s| s.height),
                underwater,
                self.eye_pos().y
            );
        }
    }

    // Entity.tickBlockCollision() -> checkBlockCollision(): jede Blockzelle, die die
    // Spielerbox am ENDE der Bewegung beruehrt, bekommt onEntityCollision. Nachgebildet ist
    // davon nur der Teil, der die Bewegung betrifft (slowMovement, s.
    // world::slow_movement_multiplier) -- Feuer, Portale, Trichter und der ganze Rest sind
    // Server-Sache und hier ohne Wirkung.
    //
    // Die Box wird wie in Vanilla um 1.0E-5 nach INNEN geschrumpft
    // (calculateDefaultBoundingBox, dann contract): buendiges Anliegen an einer
    // Zellgrenze zaehlt damit NICHT als Beruehrung. Ohne das Schrumpfen loeste ein Netz
    // schon aus, wenn man exakt an seiner Kante steht -- der Server sieht es dann nicht, und
    // die Simulationen laufen auseinander.
    //
    // Die Formpruefung aus Vanilla faellt weg, weil sie fuer unseren einzigen Fall immer
    // wahr ist: getInsideCollisionShape() ist in AbstractBlock der VOLLE Wuerfel, und
    // CobwebBlock ueberschreibt das nicht -- `voxelShape == VoxelShapes.fullCube()` greift
    // also sofort. Kaeme hier je ein Block mit kleinerer Innenform dazu, muesste die
    // Ueberlappung wirklich gegen dessen Form geprueft werden.
    fn tick_block_collision(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        // Entity.tickBlockCollisions(): VOR der Zellen-Schleife und nur am Boden bekommt der
        // Block unter den Fuessen (getLandingPos) onSteppedOn.
        if self.on_ground {
            self.stepped_on_block(input, world);
        }
        const SHRINK: f64 = 1.0e-5;
        let half = self.width() / 2.0;
        let min = DVec3::new(self.pos.x - half + SHRINK, self.pos.y + SHRINK, self.pos.z - half + SHRINK);
        let max = DVec3::new(self.pos.x + half - SHRINK, self.pos.y + self.current_height() - SHRINK, self.pos.z + half - SHRINK);
        // Leere Box (Schrumpfen groesser als die Box selbst) beruehrt nichts.
        if min.x > max.x || min.y > max.y || min.z > max.z {
            return;
        }
        // Netz und Honig einmal gemeinsam einsammeln, statt beim ersten Netz auszusteigen:
        // Vanilla ruft onEntityCollision fuer JEDE beruehrte Zelle, ein Netz darf ein Honig-
        // Rutschen in einer anderen Zelle also nicht verschlucken. Innerhalb einer Blockart
        // reicht die erste Zelle (mehrere setzen denselben Wert nacheinander).
        let mut netz: Option<[f64; 3]> = None;
        let mut honig: Vec<(i32, i32, i32)> = Vec::new();
        for bx in min.x.floor() as i32..=max.x.floor() as i32 {
            for by in min.y.floor() as i32..=max.y.floor() as i32 {
                for bz in min.z.floor() as i32..=max.z.floor() as i32 {
                    let state = world.block_at(bx, by, bz);
                    if netz.is_none() {
                        netz = world::slow_movement_multiplier(state);
                    }
                    if world::block_registry::block_name(state) == Some("honey_block") {
                        honig.push((bx, by, bz));
                    }
                }
            }
        }
        // Player.makeStuckInBlock: fliegend haelt kein Netz fest.
        if let (Some(m), false) = (netz, self.abilities.flying) {
            // Entity.slowMovement(): setzt den Faktor UND ruft onLanding() -- deshalb
            // schuetzt ein Netz vor Fallschaden, auch wenn man nur hindurchfaellt.
            self.movement_multiplier = DVec3::new(m[0], m[1], m[2]);
            self.fall_distance = 0.0;
        }
        // Jede beruehrte Honigzelle darf es versuchen: isSliding prueft die Hoehe GEGEN DIE
        // JEWEILIGE ZELLE, eine tiefere kann also ablehnen, waehrend die daruberliegende
        // zustimmt. Die erste, die greift, gewinnt -- ein zweiter Durchlauf waere ohnehin
        // wirkungslos (danach liegt die Geschwindigkeit ueber der -0.08-Schwelle).
        for pos in honig {
            if self.slide_down_honey(pos) {
                break;
            }
        }
        self.bubble_columns(min, max, world, shapes);
    }

    // BubbleColumnBlock.entityInside -> Entity.onInside/onAboveBubbleColumn (1.21.11,
    // nachgesehen): JEDE beruehrte Saeulenzelle schiebt einmal, in Vanillas Reihenfolge der
    // Zellen (BlockPos.betweenClosed: z aussen, y, x innen) -- die Obergrenzen greifen sonst
    // an anderer Stelle. Ist ueber der Zelle weder Kollisionsform noch Fluid, gilt sie als
    // Oberkante: dort staerker, und ohne Ruecksetzen der Fallstrecke.
    //
    // ECHTER BUG gewesen (Grim "Simulation" x26 auf anticheat-test.com): Blasensaeulen
    // fehlten ganz -- der Server schob uns, wir nicht.
    fn bubble_columns(&mut self, min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) {
        // Player.onInside/onAboveBubbleColumn: fliegend nichts.
        if self.abilities.flying {
            return;
        }
        for bz in min.z.floor() as i32..=max.z.floor() as i32 {
            for by in min.y.floor() as i32..=max.y.floor() as i32 {
                for bx in min.x.floor() as i32..=max.x.floor() as i32 {
                    let state = world.block_at(bx, by, bz);
                    if world::block_registry::block_name(state) != Some("bubble_column") {
                        continue;
                    }
                    let runter = world::state_props::property(state, "drag") == Some("true");
                    let oben = world.block_at(bx, by + 1, bz);
                    let oberkante = world::collision_boxes(oben, shapes).is_empty() && world::fluid_kind(oben).is_none();
                    let y = self.vel.y;
                    self.vel.y = match (oberkante, runter) {
                        (true, true) => (y - 0.03).max(-0.9),
                        (true, false) => (y + 0.1).min(1.8),
                        (false, true) => (y - 0.03).max(-0.3),
                        (false, false) => (y + 0.06).min(0.7),
                    };
                    if !oberkante {
                        self.fall_distance = 0.0;
                    }
                }
            }
        }
    }

    // SlimeBlock.onSteppedOn: wer ueber einen Schleimblock LAEUFT, wird horizontal stark
    // gebremst -- unabhaengig von der Reibung (0.8), die zusaetzlich wirkt. Nur bei
    // betragsmaessig kleiner Vertikalgeschwindigkeit (< 0.1), also im Gehen und nicht mitten
    // im Absprung, und nicht beim Schleichen (bypassesSteppingEffects() == isSneaking(), beim
    // Client die reine Taste). Genau deshalb kommt man schleichend normal ueber Schleim.
    fn stepped_on_block(&mut self, input: &Input, world: &World) {
        if world::block_registry::block_name(block_at_pos(world, self.landing_pos(world))) != Some("slime_block") {
            return;
        }
        let d = self.vel.y.abs();
        if d < 0.1 && !input.sneak {
            let e = 0.4 + d * 0.2;
            self.vel.x *= e;
            self.vel.z *= e;
        }
    }

    // HoneyBlock.onEntityCollision/isSliding/updateSlidingVelocity: an der SEITE eines
    // Honigblocks nach unten rutschen statt frei zu fallen.
    //
    // Die beiden Umrechnungen sind der Kern und leicht zu uebersehen: Vanilla prueft und setzt
    // die Geschwindigkeit, NACHDEM travel() Schwerkraft und Luftwiderstand schon draufgerechnet
    // hat, rechnet fuer seine Grenzwerte aber mit dem Wert von DAVOR --
    //   getOldVelocityY(d) = d / 0.98F + 0.08   (Schwerkraft/Daempfung herausgerechnet)
    //   getNewVelocityY(d) = (d - 0.08) * 0.98F (Zielwert wieder hineingerechnet)
    // -- damit die Rutschgeschwindigkeit nach dem naechsten Tick exakt -0.05 ist. Unsere
    // tick_block_collision laeuft an derselben Stelle (nach tick_air), die Umrechnung passt
    // also unveraendert. 0.98F ist ein float, deshalb hier ueber f32 gerechnet.
    fn slide_down_honey(&mut self, pos: (i32, i32, i32)) -> bool {
        const DRAG_F32: f64 = 0.98f32 as f64;
        let old_y = |v: f64| v / DRAG_F32 + 0.08;
        // isSliding(): am Boden rutscht niemand; wer schon fast oben auf dem Block steht
        // ebenso wenig; und es braucht echten Fall (> 0.08 nach Herausrechnen).
        if self.on_ground || self.pos.y > pos.1 as f64 + 0.9375 - 1.0e-7 || old_y(self.vel.y) >= -0.08 {
            return false;
        }
        // Der Mittelpunktsabstand: nur an der SEITE des Blocks wird gerutscht, nicht wenn man
        // mittig ueber/in ihm haengt. 0.4375 + halbe Spielerbreite = 0.7375.
        let dx = (pos.0 as f64 + 0.5 - self.pos.x).abs();
        let dz = (pos.2 as f64 + 0.5 - self.pos.z).abs();
        let grenze = 0.4375 + self.width() / 2.0;
        if !(dx + 1.0e-7 > grenze || dz + 1.0e-7 > grenze) {
            return false;
        }
        // updateSlidingVelocity(): oberhalb von -0.13 nur die Y-Bremse, darunter zusaetzlich
        // die Horizontalgeschwindigkeit im selben Verhaeltnis stauchen (sonst wuerde man beim
        // Abbremsen seitlich wegschiessen).
        let vor = old_y(self.vel.y);
        if vor < -0.13 {
            let d = -0.05 / vor;
            self.vel.x *= d;
            self.vel.z *= d;
        }
        self.vel.y = (-0.05 - 0.08) * DRAG_F32;
        // entity.onLanding(): Rutschen setzt die Fallhoehe zurueck -- deshalb nimmt man an
        // einer Honigwand keinen Fallschaden.
        self.fall_distance = 0.0;
        true
    }

    fn tick_air(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        // `f`: Reibung des Blocks unter den Fuessen, wie in travelInAir() VOR der Bewegung
        // dieses Ticks gelesen (also der Bodenkontakt-Zustand vom Ende des letzten Ticks).
        // In der Luft zaehlt immer 1.0.
        let f = if self.on_ground { self.ground_friction(world) } else { 1.0 };
        // Vanilla: Blockreibung (float) × 0.91F, das Produkt noch als float.
        let horizontal_drag = if input.float_drag { (f as f32 * 0.91f32) as f64 } else { f * 0.91 };

        let speed = self.movement_speed(input);
        // Sneak NICHT mehr hier auf die accel: die Sneak-Verlangsamung (SNEAKING_SPEED = 0.3)
        // gehoert in Vanilla auf den BEWEGUNGS-INPUT (ClientPlayerEntity.applyMovementSpeed-
        // Factors), nicht auf die Geschwindigkeit -- und zwar VOR der Richtungs-Skalierung, s.
        // add_move_input. Sprint (×1.3) bleibt dagegen ein echter Speed-Attribut-Modifikator.
        // getFrictionInfluencedSpeed(): am Boden aus der Reibung rueckgerechnet (das
        // Zusammenspiel mit horizontal_drag unten erzeugt die Zielgeschwindigkeit von
        // selbst als Gleichgewicht, ohne dass wir eine Hoechstgeschwindigkeit fest
        // verdrahten), in der Luft die Luftkontroll-Basis (AIR_ACCEL = 0.02).
        //
        // ECHTER BUG gewesen (Flags nur noch beim Springen): der
        // Sprint-Multiplikator (×1.3) -- und analog Sneak -- wirkt AUCH auf die Luftkontrolle,
        // nicht nur am Boden. GrimAC bestaetigte es im consoledebug eindeutig: waehrend eines
        // Sprint-Sprungs war A/P = 1/1.3 = 0.769 in BEIDEN Horizontalachsen (wir zu langsam,
        // weil AIR_ACCEL fest bei 0.02 blieb statt 0.026). Deshalb hier denselben
        // Geschwindigkeits-Faktor (speed/BASE_SPEED) auf AIR_ACCEL anwenden wie am Boden.
        //
        // WICHTIG fuer die Luft: dort steht KEIN Attributwert, sondern ein festes Paar.
        // PlayerEntity.getOffGroundSpeed() gibt schlicht 0.026 beim Sprinten und sonst 0.02
        // (als float) zurueck (am Bytecode nachgelesen) -- dass der Sprintwert genau AIR_ACCEL * 1.3 ist,
        // ist ein Zufall der gewaehlten Zahlen und keine Regel. Schnelligkeit und Langsamkeit
        // aendern die Luftkontrolle deshalb NICHT. Frueher stand hier `AIR_ACCEL * (speed /
        // BASE_SPEED)`, was mit dem Effektfaktor still falsch geworden waere.
        // Player.getFlyingSpeed: fliegend die Fluggeschwindigkeit, sprintend doppelt.
        let accel = if self.on_ground {
            speed * (0.21600002 / (f * f * f))
        } else if self.abilities.flying {
            (self.abilities.fly_speed * if self.sprinting { 2.0 } else { 1.0 }) as f64
        } else if self.sprinting {
            AIR_ACCEL * SPRINT_SPEED_MULT
        } else {
            AIR_ACCEL
        };

        // Sprung: Vanilla erlaubt pro gehaltener Taste jeden Tick erneut zu springen,
        // sobald wieder Bodenkontakt besteht -- kein Einzel-Press noetig. jump_cooldown
        // ist an Land praktisch immer schon 0 (die Sprung-Flugzeit ist laenger als die
        // 10-Tick-Abklingzeit, s. try_jump-Kommentar), die Pruefung hier ist trotzdem
        // noetig, weil beide Zweige denselben Zaehler teilen.
        // Fliegend kein Sprung: LivingEntity.aiStep springt nur mit isAffectedByFluids().
        if input.jump && self.on_ground && self.jump_ready() && !self.abilities.flying {
            self.try_jump(input, world);
        }

        self.add_move_input(input, accel);
        // LivingEntity.applyMovementInput(): updateVelocity -> applyClimbingSpeed -> move.
        // Die Klemmung gehoert also ZWISCHEN Beschleunigung und Bewegung, nicht davor oder
        // danach -- sonst legt man im Kletter-Tick eine andere Strecke zurueck als der Server.
        // Zuschauer klettern nie (onClimbable).
        if !input.spectator {
            self.apply_climbing_speed(world);
        }
        self.move_and_zero(input, world, shapes);
        // ... und direkt NACH move(): bei horizontaler Kollision oder gehaltenem Sprung an einer
        // Leiter (isClimbing) wird vel.y auf 0.2 gesetzt. `jumping` ist beim Client schlicht
        // die gehaltene Sprungtaste. Das ist der eigentliche Aufstieg: gegen die Leiter druecken
        // ODER springen. isClimbing() wird hier ERNEUT gefragt, mit der Position NACH der
        // Bewegung -- wer die Kletterzelle in diesem Tick verlassen hat, bekommt den Schub nicht
        // mehr.
        if (self.horizontal_collision || input.jump) && !input.spectator && self.is_climbing(world) {
            self.vel.y = CLIMB_UP_SPEED;
        }

        // Gravitation + Daempfung: NACH der Kollision, wie Vanillas travelInAir -- betrifft
        // die Geschwindigkeit fuer den NAECHSTEN Tick, nicht die gerade zurueckgelegte
        // Strecke dieses Ticks.
        // travelMidAir: Schwebeeffekt ERSETZT die Schwerkraft, Sanfter Fall schwaecht sie ab
        // (s. effective_gravity -- gefragt wird mit der Geschwindigkeit NACH move()).
        match input.levitation {
            Some(stufe) => self.vel.y += (0.05 * (stufe + 1) as f64 - self.vel.y) * 0.2,
            None => self.vel.y -= effective_gravity(self.vel.y, input, self.body.gravity),
        }
        self.vel.x *= horizontal_drag;
        self.vel.z *= horizontal_drag;
        self.vel.y *= float_or(input, VERTICAL_DRAG);
    }

    // LivingEntity.travelGliding: an einer Leiter endet der Flug (dann gilt die normale
    // Luftbewegung), sonst rechnet calc_gliding_velocity die neue Geschwindigkeit und der
    // Spieler bewegt sich damit -- ohne Eingabe-Beschleunigung und ohne Luftreibung, die
    // Daempfung steckt schon in der Formel.
    fn tick_glide(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        if self.is_climbing(world) {
            self.tick_air(input, world, shapes);
            self.gliding = false;
            return;
        }
        // Das Springen steht in LivingEntity.tickMovement vor travel() und gilt auch gleitend
        // -- relevant nur am Boden, bevor der Server das Flag geloescht hat.
        if input.jump && self.on_ground && self.jump_ready() {
            self.try_jump(input, world);
        }
        let rotation = rotation_vector(input.pitch_deg, input.yaw_deg);
        self.vel = calc_gliding_velocity(self.vel, rotation, input.pitch_deg, effective_gravity(self.vel.y, input, self.body.gravity));
        self.move_and_zero(input, world, shapes);
    }

    // travelInWater(): Schwimmen -- Bremsfaktor statt Bodenreibung, konstante (statt
    // reibungsabhaengige) Beschleunigung, gedaempfte statt volle Gravitation.
    fn tick_water(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes, water_height: f64) {
        // Y-Position VOR der Bewegung dieses Ticks merken (wie Vanillas travelInFluid), fuer resetVerticalVelocityInFluid am Ende (s. jump_out_of_fluid).
        let y_before = self.pos.y;

        // TEMPORAERES Debug-Logging (CORAL_WATER_DEBUG=1), um den Z-Offset beim
        // Sprint-Schwimmen an echten Spieldaten nachzuvollziehen (0.0178 Offset an einem
        // reproduzierbaren Tick, synthetisch nicht nachstellbar).
        if std::env::var_os("CORAL_WATER_DEBUG").is_some() {
            let (bx, by, bz) = (self.pos.x.floor() as i32, (self.pos.y + 0.9).floor() as i32, self.pos.z.floor() as i32);
            let water_above_head = world::fluid_kind(world.block_at(bx, by, bz)).is_some();
            eprintln!(
                "[water] pos=({:.4},{:.4},{:.4}) vel=({:.5},{:.5},{:.5}) water_height={:.4} raw_sw={} swim={} sprint={} on_ground={} hcol={} water_above_head={} sneak={} jump={} look_y={:.3}",
                self.pos.x,
                self.pos.y,
                self.pos.z,
                self.vel.x,
                self.vel.y,
                self.vel.z,
                water_height,
                self.raw_swimming,
                self.swimming,
                self.sprinting,
                self.on_ground,
                self.horizontal_collision,
                water_above_head,
                input.sneak,
                input.jump,
                input.look_dir_y
            );
        }

        self.apply_swim_input(input, world, water_height, true);
        self.apply_swim_pitch_pull(input, world);

        let was_falling = self.vel.y <= 0.0; // travelInFluid(): bl VOR moveRelative/move
        // travelInFluid holt die Schwerkraft ebenfalls VOR der Bewegung.
        let gravity = effective_gravity(self.vel.y, input, self.body.gravity);
        // Wasserlaeufer (WATER_MOVEMENT_EFFICIENCY). travelInWater():
        //   h = Attributwert, ausserhalb des Bodenkontakts halbiert
        //   wenn h > 0:  f rueckt um h auf 0.54600006 zu    (Bremse Richtung Landreibung)
        //                g += (getSpeed() - g) * h          (Beschleunigung Richtung Landtempo)
        // Ohne das schwimmt man mit Wasserlaeufer III so langsam wie ganz ohne -- und der
        // Server (bzw. GrimAC, der dasselbe Attribut liest) rechnet die ganze Zeit schneller.
        let efficiency = if self.on_ground { input.water_movement_efficiency } else { input.water_movement_efficiency * 0.5 };
        let accel = if efficiency > 0.0 {
            FLUID_ACCEL + (self.movement_speed(input) - FLUID_ACCEL) * efficiency
        } else {
            FLUID_ACCEL
        };
        self.add_move_input(input, accel);
        self.move_and_zero(input, world, shapes);
        // travelInWater() hat denselben Kletter-Aufstieg wie die Luft, aber in ZWEI Punkten
        // schmaler: nur ueber die Horizontalkollision (die Sprungtaste zaehlt hier nicht) und
        // ohne die 0.15er Klemmung, denn travelInWater ruft applyMovementInput gar nicht auf.
        // Er steht VOR der Wasserbremse -- die 0.8 wirken also noch auf die 0.2.
        // travelInLava kennt gar keinen Kletterzweig (im Jar nachgesehen), deshalb steht das
        // hier und nicht in tick_lava.
        if self.horizontal_collision && self.is_climbing(world) {
            self.vel.y = CLIMB_UP_SPEED;
        }

        // Reihenfolge wie in travelInWater: erst Sprint/Grundbremse, dann der Wasserlaeufer,
        // und die Gunst des Delfins setzt am Ende HART 0.96 (sie ersetzt, sie mischt nicht).
        let mut f = float_or(input, if self.sprinting { WATER_SPRINT_SLOW_DOWN } else { WATER_SLOW_DOWN });
        if efficiency > 0.0 {
            f += (WATER_EFFICIENCY_TARGET_DRAG - f) * efficiency;
        }
        if input.dolphins_grace {
            f = DOLPHINS_GRACE_SLOW_DOWN;
        }
        self.vel.x *= f;
        self.vel.y *= float_or(input, FLUID_VERTICAL_DRAG);
        self.vel.z *= f;
        self.vel = fluid_falling_adjusted_movement(self.vel, gravity, was_falling, self.sprinting);
        // resetVerticalVelocityInFluid() -- letzter Schritt von travelInWater(), s. dort.
        self.jump_out_of_fluid(world, shapes, y_before);
    }

    // travelInLava(): staerker gebremst als Wasser, kein Schwimm-Feingefuehl (kein
    // getFluidFallingAdjustedMovement ausserhalb der Flach-Verzweigung), dafuer ein fester
    // Sink-Zug -d/4 auf JEDEN Tick obendrauf -- der eigentliche Grund, warum man in Lava
    // absackt statt zu schweben.
    fn tick_lava(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes, lava_height: f64) {
        let y_before = self.pos.y;
        self.apply_swim_input(input, world, lava_height, false);

        let was_falling = self.vel.y <= 0.0;
        let gravity = effective_gravity(self.vel.y, input, self.body.gravity);
        self.add_move_input(input, FLUID_ACCEL);
        self.move_and_zero(input, world, shapes);

        if lava_height <= FLUID_JUMP_THRESHOLD {
            self.vel.x *= LAVA_SLOW_DOWN;
            self.vel.y *= FLUID_VERTICAL_DRAG;
            self.vel.z *= LAVA_SLOW_DOWN;
            self.vel = fluid_falling_adjusted_movement(self.vel, gravity, was_falling, self.sprinting);
        } else {
            self.vel *= LAVA_SLOW_DOWN;
        }
        self.vel.y -= gravity / 4.0;
        // resetVerticalVelocityInFluid() -- letzter Schritt von travelInLava(), identisch zu
        // Wasser (derselbe Kick, dieselbe Bedingung).
        self.jump_out_of_fluid(world, shapes, y_before);
    }

    // aiStep()s Sprung-/Schwimm-Verzweigung, auf den Wasser-/Lava-Fall reduziert (der
    // "onGround -> jumpFromGround()"-Zweig bleibt unveraendert wie an Land): in flachem
    // Wasser/Lava am Boden ganz normal hochspringen (echter Sprung, s. try_jump/
    // jump_cooldown), sonst hoch- bzw. (beim Schleichen) herunterschwimmen -- Letzteres
    // OHNE Cooldown, jeden Tick um FLUID_VERTICAL_KICK.
    //
    // Der echte Sprung hier respektiert denselben jump_cooldown-Gate wie an Land (s.
    // try_jump-Kommentar) -- korrekt fuer sich genommen, war aber NICHT die Ursache des
    // "water hopping" (s. jump_out_of_fluid-Kommentar fuer die
    // tatsaechliche Ursache).
    // is_water: knockDownwards() (Sneak = abtauchen) ist in Vanilla WASSER-spezifisch
    // (ClientPlayerEntity.tickMovement: `isTouchingWater() && sneak && shouldSwimInFluids()`),
    // NICHT in Lava -- deshalb der Parameter. Der Swim-Up-Kick (knockUpwards/Jump-in-Fluid)
    // gilt dagegen fuer beide Fluide (LivingEntity.tickMovement).
    fn apply_swim_input(&mut self, input: &Input, world: &World, fluid_height: f64, is_water: bool) {
        // Sneak = abtauchen (knockDownwards, -0.04): ClientPlayerEntity.tickMovement (Zeile 811),
        // WASSER-spezifisch und voellig UNABHAENGIG vom Springen. Laeuft in Vanilla VOR dem
        // Swim-Up (super.tickMovement -> LivingEntity Zeile 2955), deshalb hier zuerst.
        //
        // ECHTER BUG gewesen (Jump und Sneak gleichzeitig im Wasser flaggte heftig): vorher ein if/else if, bei dem der Jump-Zweig den Sneak-Kick
        // verschluckte. Vanilla feuert beide Checks getrennt im selben Tick -> Jump+Sneak =
        // +0.04 (swimUpward) - 0.04 (knockDownwards) = netto 0. Wir schickten netto +0.04 pro
        // Tick -> die Y-Position lief Tick fuer Tick von Grims Simulation weg -> harter Flag.
        // knockDownwards() selbst hat KEINE Tiefen-Bedingung; am Boden in flachem Wasser tut es
        // nichts (Bodenkollision nullt vel.y), es driftet also nicht.
        if is_water && input.sneak {
            self.vel.y -= FLUID_VERTICAL_KICK;
        }
        // Jump = hochschwimmen (swimUpward, +0.04) bzw. echter Sprung in flachem Wasser/Lava am
        // Boden (LivingEntity.tickMovement Zeile 2955-2976). shouldSwimInFluids() ist fuer einen
        // normalen Spieler immer true (Zeile 565), deshalb nicht modelliert. Der echte Sprung
        // ueberschreibt einen vorher gesetzten Sneak-Kick per vel.y.max(JUMP_VELOCITY) -- exakt
        // wie Vanilla (knockDownwards vor jump(), jump() nutzt Math.max).
        if input.jump {
            let swim_up = fluid_height > 0.0 && !(self.on_ground && fluid_height <= FLUID_JUMP_THRESHOLD);
            if swim_up {
                self.vel.y += FLUID_VERTICAL_KICK;
            } else if self.on_ground && self.jump_ready() {
                self.try_jump(input, world);
            }
        }
    }

    // LivingEntity.jump(): setzt die volle Sprunggeschwindigkeit (nie NIEDRIGER als eine
    // schon vorhandene, s. Math.max) und -- unabhaengig davon, ob an Land oder im Wasser/
    // Lava gesprungen wurde -- den Sprint-Schub in Blickrichtung, falls gerade gesprintet
    // wird. Startet danach die 10-Tick-Abklingzeit (s. jump_cooldown-Feldkommentar), aus
    // dem lokalen Yarn-1.21.11-Jar nachgesehen (LivingEntity.jump()/tickMovement()).
    fn try_jump(&mut self, input: &Input, world: &World) {
        // LivingEntity.getJumpVelocity(): (float)JUMP_STRENGTH * strength *
        // getJumpVelocityMultiplier(). Der Multiplikator ist nur beim Honigblock != 1.0.
        // Die Multiplikation absichtlich in f32 (Vanilla rechnet die ganze Zeile in float),
        // der neutrale Fall aber unveraendert ueber die f64-Konstante -- sonst wuerde JEDER
        // Sprung um die float-Rundung von 0.42 (1.3e-8) verschoben, ohne dass hier irgendein
        // Honig im Spiel ist.
        let mult = self.jump_multiplier(world);
        // Sprungkraft kommt in derselben float-Rechnung obendrauf: 0.1 * (Stufe + 1).
        // JUMP_STRENGTH vom Server (s. BodyAttributes), Vorgabe 0.42F.
        let staerke = self.body.jump_strength as f32;
        let jump_velocity = match input.jump_boost {
            Some(stufe) => (staerke * mult + 0.1f32 * (stufe as f32 + 1.0)) as f64,
            None if mult == 1.0 && self.body.jump_strength == DEFAULT_JUMP_STRENGTH => JUMP_VELOCITY,
            None => (staerke * mult) as f64,
        };
        self.vel.y = self.vel.y.max(jump_velocity);
        if self.sprinting {
            // MathHelper.sin/cos (Float-Tabelle) wie Vanillas LivingEntity.jump(), NICHT exaktes
            // sin/cos -- sonst weicht der Sprint-Schub minimal ab (s. add_move_input-Kommentar).
            self.vel.x -= mc_sin(input.yaw_rad as f64) as f64 * SPRINT_JUMP_BOOST;
            self.vel.z += mc_cos(input.yaw_rad as f64) as f64 * SPRINT_JUMP_BOOST;
        }
        self.jump_cooldown = 10; // wirkungslos, solange jump_cooldown_enabled aus ist, s. jump_ready()
    }

    // Steckt die Spielerbox in dieser Blockspalte in einem erstickenden Block?
    //
    // ClientPlayerEntity.wouldCollideAt (1.21.11-Jar): eine Box ueber die GANZE Zelle in X/Z,
    // in Y ueber die eigene Hoehe, um 1e-7 geschrumpft, und dann World.canCollide -- also die
    // KOLLISIONSFORMEN der erstickenden Bloecke (BlockCollisionSpliterator mit forEntity), die
    // diese Box wirklich schneiden. Nicht bloss "in der Zelle steht ein erstickender Block":
    // Trampelpfad und Ackerboden ersticken, sind aber nur 15/16 hoch. Wer auf einem Pfad steht,
    // hat daneben einen gleich hohen Pfad UNTER den Fuessen, nicht in der Box. Wir zaehlten die
    // ganze Zelle und schoben dort mit 0.1 weg, Vanilla (und Grim) nicht -- sichtbar als
    // Simulation-Flags beim Springen von Pfad zu Pfad.
    fn suffocates_at(&self, bx: i32, bz: i32, world: &World, shapes: &world::BlockShapes) -> bool {
        const EPS: f64 = 1.0e-7;
        let min = DVec3::new(bx as f64 + EPS, self.pos.y + EPS, bz as f64 + EPS);
        let max = DVec3::new(bx as f64 + 1.0 - EPS, self.pos.y + self.current_height() - EPS, bz as f64 + 1.0 - EPS);
        let boxen = collect_boxes(min, max, world, &|state, by| {
            if world::is_suffocating(state, shapes) {
                world::collision_boxes_at(state, shapes, by, world::ShapeContext::ABSENT)
            } else {
                Vec::new()
            }
        });
        !boxen.is_empty()
    }

    // Vanillas LocalPlayer.moveTowardsClosestSpace(): steckt diese Ecke der Spielerbox in
    // einem erstickenden Block, bekommt die Geschwindigkeit 0.1 in die Richtung der NAECHSTEN
    // freien Nachbarspalte gesetzt (gesetzt, nicht addiert) -- so schiebt es einen in Vanilla
    // aus einem Block heraus, in den man hineingeraten ist oder in den jemand gebaut hat.
    //
    // ECHTE ABWEICHUNG GEWESEN: der Client hatte das gar nicht. Wer uns
    // in Obsidian einmauerte, bekam damit nicht Vanillas Ergebnis -- ein Vorteil, den es nicht
    // geben soll. GrimAC flaggt beides nicht: es simuliert das Herausschieben nicht, sondern
    // macht das erlaubte Fenster auf der betroffenen Achse um +-0.15 auf (PlayerBaseTick.
    // moveTowardsClosestSpaceModern), und 0.1 liegt darin.
    //
    // Die Richtungsreihenfolge (West, Ost, Nord, Sued) und der ECHT-kleiner-Vergleich sind
    // Vanillas -- bei gleichem Abstand gewinnt die zuerst gepruefte Richtung.
    fn move_towards_closest_space(&mut self, x: f64, z: f64, world: &World, shapes: &world::BlockShapes) {
        let (bx, bz) = (x.floor() as i32, z.floor() as i32);
        if !self.suffocates_at(bx, bz, world, shapes) {
            return;
        }
        let (f, g) = (x - bx as f64, z - bz as f64);
        let mut ziel: Option<(i32, i32)> = None;
        let mut kleinster = f64::MAX;
        // (Schritt x, Schritt z, Abstand bis zur Zellgrenze in dieser Richtung)
        for (sx, sz, abstand) in [(-1, 0, f), (1, 0, 1.0 - f), (0, -1, g), (0, 1, 1.0 - g)] {
            if abstand < kleinster && !self.suffocates_at(bx + sx, bz + sz, world, shapes) {
                kleinster = abstand;
                ziel = Some((sx, sz));
            }
        }
        match ziel {
            Some((sx, 0)) => self.vel.x = 0.1 * sx as f64,
            Some((_, sz)) => self.vel.z = 0.1 * sz as f64,
            None => {}
        }
    }

    // s. jump_cooldown_enabled-Feldkommentar: AUS macht jeden Sprung sofort wieder
    // moeglich, unabhaengig vom Zaehlerstand.
    fn jump_ready(&self) -> bool {
        !self.jump_cooldown_enabled || self.jump_cooldown == 0
    }

    // LivingEntity.resetVerticalVelocityInFluid(): am Ende von travelInWater()/
    // travelInLava() automatisch ein Stueck herausklettern, wenn man im Wasser/in Lava
    // horizontal gegen eine Kante/Wand schwimmt UND darueber Platz ist -- ohne das wuerde
    // man hilflos gegen jede Kante schwimmen, statt drueberzukommen. Setzt vel.y HART auf
    // 0.3 (kein max() wie beim Sprung -- eine echte Ersetzung), OHNE jump_cooldown und
    // OHNE gehaltenes Springen als Voraussetzung: reine Horizontalkollision reicht.
    //
    // Das ist der TATSAECHLICHE Grund fuer "water hopping" (echter Vanilla-Mechanismus): schwimmt man kontinuierlich gegen dieselbe Kante, loest JEDER Tick mit
    // Wandkontakt erneut den vollen 0.3-Kick aus -- viel oefter, als ein normaler
    // Sprung-Cooldown erlauben wuerde, weil hier gar kein Cooldown greift.
    //
    // Vanilla (1.21.11-Jar, LivingEntity): bei horizontaler Kollision wird geprueft, ob die
    // Box um (vel.x, vel.y + 0.6 - aktuelle Hoehe + y, vel.z) verschoben frei waere; dann wird
    // vel.y auf 0.3 gesetzt. `y` ist die Y-Position VOR der Bewegung dieses Ticks (in
    // travelInFluid vor updateVelocity/move gemerkt), self.pos ist hier -- nach move_and_zero, ganz am Ende von
    // tick_water/tick_lava -- die Position DANACH. Der Versatz peilt also ab der alten Position
    // 0.6 Bloecke hoch, nicht ab der schon fortgeschrittenen.
    //
    // Drei Punkte, an denen das vorher nicht Vanilla war (deshalb war es testweise ganz
    // abgeschaltet, statt zu oft zu feuern):
    //   1. Fluid-Test. doesNotCollide() ist isSpaceEmpty(box) && !containsFluid(box).
    //      containsFluid zaehlt JEDE Zelle mit nicht-leerem FluidState -- unabhaengig vom
    //      Fuellstand. Wir hatten stattdessen fluid_height_scan benutzt, das die Box um 0.001
    //      schrumpft und nur anschlaegt, wenn die Fluid-OBERFLAECHE ueber der Boxunterkante
    //      liegt. Genau der Fall am Beckenrand: die Zielbox liegt in halbhohem Fliesswasser,
    //      Vanilla sagt "belegt, kein Hop", wir sagten "frei, Hop" -- ein 0.3-Kick, den Grim
    //      nicht mitrechnet, also grim.simulation. Das war der eigentliche Fehler.
    //   2. Rechenreihenfolge des Versatzes. Java rechnet ((v.y + 0.6f) - getY()) + y und legt
    //      das auf box.minY == getY() drauf. Algebraisch y + v.y + 0.6, in f64 aber nicht
    //      bitgleich -- und Grim rechnet Javas Reihenfolge. Hier jetzt genauso Schritt fuer
    //      Schritt.
    //   3. 0.6F und 0.3F sind FLOATS. Als double sind das 0.6000000238418579 bzw.
    //      0.30000001192092896, nicht 0.6/0.3. Der Kick war um 1.2e-8 zu klein -- pro Tick
    //      nichts, ueber eine Hop-Serie gegen dieselbe Kante aber genau die Art Drift, die
    //      am Ende als Offset auffaellt.
    //
    // Kein jump_cooldown und kein gehaltenes Springen: reine Horizontalkollision reicht. Das
    // ist der Grund, warum "water hopping" ueberhaupt geht -- schwimmt man dauerhaft gegen
    // dieselbe Kante, feuert JEDER Tick mit Wandkontakt den vollen Kick erneut. Das ist echte
    // Vanilla-Mechanik, kein Zusatz von uns, und wird deshalb auch nicht per Einstellung
    // gedrosselt (Settings::jump_cooldown_enabled betrifft nur echte Spruenge).
    fn jump_out_of_fluid(&mut self, world: &World, shapes: &world::BlockShapes, y_before: f64) {
        if !self.horizontal_collision {
            return;
        }
        // Exakt Javas Term- und Klammerfolge, s. Punkt 2/3 oben.
        let offset_y = self.vel.y + 0.6f32 as f64 - self.pos.y + y_before;
        let target = DVec3::new(self.pos.x + self.vel.x, self.pos.y + offset_y, self.pos.z + self.vel.z);
        let height = self.current_height();
        let half = self.width() / 2.0;
        let box_min = DVec3::new(target.x - half, target.y, target.z - half);
        let box_max = DVec3::new(target.x + half, target.y + height, target.z + half);
        if box_is_empty(box_min, box_max, world, shapes, self.shape_context()) && !contains_fluid(box_min, box_max, world) {
            self.vel.y = 0.3f32 as f64;
        }
    }

    // PlayerEntity.travel(): waehrend der Schwimm-Pose (s. self.swimming) wird die
    // Vertikalgeschwindigkeit VOR der eigentlichen travelInWater()-Bewegung Richtung
    // Blickrichtung gezogen -- deshalb schwimmt man beim Sprint-Schwimmen tatsaechlich in die
    // Richtung, in die man schaut (nach oben/unten schauen + vorwaerts sprinten reicht zum
    // Auf-/Abtauchen, ohne zusaetzlich Springen/Schleichen zu halten). NICHT geraten, sondern
    // aus dem lokalen Yarn-1.21.11-Jar nachgesehen: d ist die Y-Komponente des Blickvektors
    // (+ = hoch), der Faktor e ist 0.085 bei d < -0.2 (steil nach unten = schnellere
    // Annaeherung), sonst 0.06. Ist d <= 0, springt man, oder steht Fluessigkeit direkt ueber
    // dem Kopf, rueckt vel.y um e * (d - vel.y) Richtung d.
    // Der dritte Bedingungszweig (Wasser direkt ueber dem Kopf) verhindert, dass blosses
    // Nach-oben-Schauen ohne Springen einen schon aus dem Wasser zieht, sobald die Oberflaeche
    // erreicht ist -- solange noch Wasser ueber dem Kopf ist, greift der Zug trotzdem, auch
    // ohne gehaltenes Springen.
    fn apply_swim_pitch_pull(&mut self, input: &Input, world: &World) {
        // ECHTER BUG gewesen (nach oben schwimmen ohne Leertaste
        // -> Server-Setback beim Reinspringen/Auftauchen). Vanilla PlayerEntity.travel gated
        // diesen Blick-nach-oben-Zug auf `isSwimming()` -- das ist der ECHTE Sprint-Schwimm-
        // Zustand (bei uns `raw_swimming`), NICHT die blosse Schwimm-POSE (`self.swimming`).
        // Die Pose kann per Fallback aktiv sein (untergetaucht, wo Steh-/Crouch-Pose nicht
        // passt), ohne dass isSwimming() gilt. Vorher zogen wir dann faelschlich vel.y Richtung
        // Blickrichtung nach oben (im consoledebug als A.y = +0.04, waehrend Grim weiter sank
        // -> Offset > 0.1 -> sofortiger Setback). raw_swimming == isSwimming(), swimming == Pose.
        if !self.raw_swimming {
            return;
        }
        let d = input.look_dir_y as f64;
        let e = if d < -0.2 { 0.085 } else { 0.06 };
        let (bx, by, bz) = ((self.pos.x).floor() as i32, (self.pos.y + 0.9).floor() as i32, (self.pos.z).floor() as i32);
        let water_above_head = world::fluid_kind(world.block_at(bx, by, bz)).is_some();
        if d <= 0.0 || input.jump || water_above_head {
            self.vel.y += (d - self.vel.y) * e;
        }
    }

    // Kompletter Vanilla-Bewegungs-Input-Pfad (nach dem Yarn-1.21.11-Client):
    //
    //   KeyboardInput.tick():          movementVector = normiert(sideways, forward)
    //   ClientPlayerEntity.applyMovementSpeedFactors():
    //                                  vec = movementVector * 0.98 (float)
    //                                  if (shouldSlowDown()) vec *= SNEAKING_SPEED (=0.3)
    //                                  vec = applyDirectionalMovementSpeedFactors(vec)
    //   ClientPlayerEntity.applyDirectionalMovementSpeedFactors(vec):
    //                                  f = |vec|; dir = vec/f
    //                                  g = getDirectionalMovementSpeedMultiplier(dir)   // 1 (kardinal), sqrt2 (diagonal)
    //                                  vec = dir * min(f*g, 1.0)
    //   Entity.movementInputToVelocity(input, speed, yaw):
    //                                  if (|input|^2 > 1) input = normalize(input)      // hier nie, |input| <= 1
    //                                  vel += rotate(input * speed, yaw)
    //
    // ECHTE BUGS gewesen:
    //   - "Vorwaerts 2% zu schnell": das 0.98 fehlte ganz.
    //   - "Sneak in der Luft flaggt": Sneak (0.3) lag auf der accel statt auf dem Input VOR der
    //     Richtungs-Skalierung. Dadurch war Diagonal-Sneak bei uns 0.294*accel statt Vanillas
    //     0.416*accel (Ratio 0.72). Der Richtungs-Multiplikator skaliert Diagonalen wieder hoch
    //     (gedeckelt auf 1.0), was eine reine Nach-der-Normalisierung-Daempfung nie reproduziert.
    //   - Sneak-Verlangsamung haengt am isInSneakingPose()-FELD (shouldSlowDown()), das Vanilla
    //     VOR input.tick() setzt -> es laggt der Taste um einen Tick nach: in_sneaking_pose_move,
    //     NICHT die aktuelle Hitbox-Pose self.crouching und NICHT input.sneak.
    // updatePose()/getExpectedPose() + canChangeIntoPose()-Fallback-Kette als reine Funktion
    // von `want_crouch` (der gewuenschten Schleich-Absicht) -> (swimming, crouching). Wird
    // zweimal aufgerufen: mit der aktuellen Sneak-Taste fuer die Hitbox (self.crouching) und
    // mit der Taste vom letzten Tick fuer die Bewegungs-Verlangsamung (in_sneaking_pose_move).
    // SWIMMING (kleinste Pose) passt immer; sonst gewuenschte Pose, dann Crouch-Fallback unter
    // einer Decke, sonst zwingend SWIMMING.
    // Vanillas PlayerEntity.updatePose() (Zeile 354), 1:1 -- inklusive der beiden Details,
    // die eine fruehere Fassung nicht hatte:
    //
    //   1. Der ZEITPUNKT. Aufgerufen wird das als letzte Anweisung von PlayerEntity.tick(),
    //      also NACH der Bewegung und an der Position nach der Bewegung. Deshalb steht der
    //      Aufruf am Ende von tick() und nicht mehr am Anfang (s. dortiger Kommentar).
    //   2. Das SWIMMING-Gate: `if (canChangeIntoPose(SWIMMING))`. Passt nicht einmal die
    //      kleinste Pose (0.6), wird GAR NICHTS geaendert -- die alte Pose bleibt stehen.
    //      Vorher fielen wir in so einem Fall auf SWIMMING zurueck, was in einer 0.5 hohen
    //      Luecke (z.B. unter einer Bodenplatte auf einer Bodenplatte) eine Pose erzeugte,
    //      die Vanilla dort nie annimmt.
    fn update_pose(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        if !pose_fits(self.pos, self.pose_height(SWIMMING_HEIGHT), world, shapes, self.shape_context(), self.body) {
            return;
        }
        // getDesiredPose() (vereinfacht, s. Feld-Kommentar): Schleichen zaehlt nur, wenn nicht
        // geschwommen wird (SWIMMING steht in Vanillas Prioritaetskette vor CROUCHING), und
        // liest die AKTUELLE Sneak-Taste -- die zu diesem Zeitpunkt, am Tick-Ende, dieselbe
        // ist, die Vanillas isSneaking() nach tickMovement dieses Ticks liefert.
        // SLEEPING steht ganz oben in der Kette: solange der Server uns im Bett fuehrt, wird
        // gar keine andere Pose ausgehandelt.
        // PlayerEntity.getExpectedPose: ... SWIMMING, GLIDING, dann SPIN_ATTACK, erst danach
        // CROUCHING. SPIN_ATTACK ist so klein wie SWIMMING und passt damit immer, sobald das
        // Gate oben durch ist -- ein Rueckfall wie bei STANDING/CROUCHING entfaellt.
        let glide = !self.sleeping && !self.raw_swimming && self.gliding;
        let spin = !self.sleeping && !self.raw_swimming && !glide && self.using_riptide;
        // getDesiredPose: fliegend kein Schleichen. Zuschauer bekommen die gewuenschte Pose,
        // ohne dass sie passen muss (updatePlayerPose).
        let schleichen = !self.raw_swimming && input.sneak && !self.abilities.flying;
        let (swim, crouch) = if self.sleeping || spin || glide {
            (false, false)
        } else if input.spectator {
            (self.raw_swimming, schleichen)
        } else {
            self.resolve_pose(schleichen, world, shapes)
        };
        self.swimming = swim;
        self.crouching = crouch;
        self.spin_attack = spin;
        self.glide_pose = glide;
    }

    // Loslassen eines Dreizacks mit Sog (TridentItem.onStoppedUsing, Client-Zweig): Schub in
    // Blickrichtung mit der Staerke aus der Verzauberung, dazu 20 Riptide-Ticks. Steht man,
    // hebt Vanilla einen zusaetzlich um 1.1999999 an (`move(MovementType.SELF, ...)`), damit
    // der Schub nicht am Boden verpufft.
    //
    // Alles in float wie in Vanilla: Winkel mal (float) PI/180, MathHelper.sin/cos aus der
    // Tabelle, MathHelper.sqrt als float. Erst addVelocity macht double daraus.
    //
    // `yaw_deg`/`pitch_deg` sind VANILLAS Winkel (Pitch positiv = nach unten).
    pub fn start_riptide(&mut self, yaw_deg: f32, pitch_deg: f32, strength: f32, world: &World, shapes: &world::BlockShapes) {
        const RADIANS_PER_DEGREE: f32 = (std::f64::consts::PI / 180.0) as f32;
        let yaw = (yaw_deg * RADIANS_PER_DEGREE) as f64;
        let pitch = (pitch_deg * RADIANS_PER_DEGREE) as f64;
        let mut x = -mc_sin(yaw) * mc_cos(pitch);
        let mut y = -mc_sin(pitch);
        let mut z = mc_cos(yaw) * mc_cos(pitch);
        let laenge = (x * x + y * y + z * z).sqrt();
        x *= strength / laenge;
        y *= strength / laenge;
        z *= strength / laenge;
        self.vel += DVec3::new(x as f64, y as f64, z as f64);
        self.riptide_ticks = RIPTIDE_TICKS;
        if self.on_ground {
            // Entity.move(SELF) nach oben: gegen Decken geklemmt; eine Bewegung nach oben ist
            // nie ein Bodenkontakt. Stoesst man oben an, nullt Block.onEntityLand die
            // Y-Geschwindigkeit -- wie bei jeder Vertikalkollision.
            let hub = DVec3::new(0.0, RIPTIDE_GROUND_LIFT, 0.0);
            let (neu, _, _, oben_an, _) =
                move_and_collide(self.pos, hub, self.current_height(), self.on_ground, world, shapes, self.shape_context(), self.body);
            self.pos = neu;
            self.on_ground = false;
            if oben_an {
                self.vel.y = 0.0;
            }
        }
    }

    // LivingEntity.tickMovement -> tickRiptide, direkt nach der Bewegung eines Ticks: der
    // Aufrufer ruft das unmittelbar nach tick() auf (dazwischen haengt in Vanilla nichts davon
    // ab). `vorher` ist die Position VOR der Bewegung dieses Ticks -- Vanilla prueft die
    // VEREINIGUNG der Box davor und danach. `andere` sind die Boxen aller anderen Entities
    // (min, max, ist ein Lebewesen) -- Vanillas getOtherEntities(this, box).
    //
    // Trifft die Box ein LEBEWESEN, endet der Sog und man prallt mit -0.2 zurueck. Trifft sie
    // nur Nicht-Lebewesen (Item, Pfeil ...), passiert gar nichts -- auch der Wand-Abbruch
    // entfaellt dann, das ist Vanillas else-if. Sonst beendet eine Wand den Sog.
    pub fn tick_riptide(&mut self, vorher: DVec3, andere: &[(DVec3, DVec3, bool)]) {
        if self.riptide_ticks == 0 {
            return;
        }
        self.riptide_ticks -= 1;
        let half = self.width() / 2.0;
        let hoehe = self.current_height();
        let min = DVec3::new(vorher.x.min(self.pos.x) - half, vorher.y.min(self.pos.y), vorher.z.min(self.pos.z) - half);
        let max = DVec3::new(vorher.x.max(self.pos.x) + half, vorher.y.max(self.pos.y) + hoehe, vorher.z.max(self.pos.z) + half);
        let getroffen: Vec<bool> = andere
            .iter()
            .filter(|(a, b, _)| a.x < max.x && b.x > min.x && a.y < max.y && b.y > min.y && a.z < max.z && b.z > min.z)
            .map(|&(_, _, lebt)| lebt)
            .collect();
        if !getroffen.is_empty() {
            if getroffen.contains(&true) {
                self.riptide_ticks = 0;
                self.vel *= -0.2;
            }
        } else if self.horizontal_collision {
            self.riptide_ticks = 0;
        }
    }

    fn resolve_pose(&self, want_crouch: bool, world: &World, shapes: &world::BlockShapes) -> (bool, bool) {
        if self.raw_swimming {
            return (true, false);
        }
        let desired_height = self.pose_height(if want_crouch { CROUCHING_HEIGHT } else { PLAYER_HEIGHT });
        if pose_fits(self.pos, desired_height, world, shapes, self.shape_context(), self.body) {
            (false, want_crouch)
        } else if !want_crouch && pose_fits(self.pos, self.pose_height(CROUCHING_HEIGHT), world, shapes, self.shape_context(), self.body) {
            (false, true)
        } else {
            (true, false)
        }
    }

    // Der aktuelle Wert von MOVEMENT_SPEED (Vanillas getSpeed(), gesetzt in
    // PlayerEntity.aiStep aus dem Attribut).
    //
    // Der Grundwert kommt vom Server (Input::movement_speed, Vorgabe 0.1) -- ein Plugin, das
    // die Laufgeschwindigkeit setzt, oder eine Verzauberung mit Tempo-Modifikator aendert ihn.
    // Sprint (x1.3) und Schnelligkeit/Langsamkeit legt die Physik weiter selbst an, genau wie
    // Vanillas Client: sie sind aus dem Paket herausgerechnet (s. protocol LOCAL_SPEED_MODIFIERS),
    // sonst zaehlten sie doppelt.
    //
    // Geklemmt wie Vanillas ClampedEntityAttribute (0..1024) -- sonst liefe man ab
    // Langsamkeit VII rueckwaerts.
    fn movement_speed(&self, input: &Input) -> f64 {
        let mut speed = input.movement_speed;
        if self.sprinting {
            speed *= SPRINT_SPEED_MULT;
        }
        (speed * input.effect_speed_factor).clamp(MOVEMENT_SPEED_MIN, MOVEMENT_SPEED_MAX)
    }

    fn add_move_input(&mut self, input: &Input, accel: f64) {
        let raw_fwd = input.forward as f64;
        let raw_strafe = input.strafe as f64;
        let raw_len = (raw_fwd * raw_fwd + raw_strafe * raw_strafe).sqrt();
        if raw_len <= 1.0e-7 {
            return;
        }
        // KeyboardInput: Rohinput auf Einheitslaenge normalisieren.
        let (u_fwd, u_strafe) = (raw_fwd / raw_len, raw_strafe / raw_len);
        // applyMovementSpeedFactors: × 0.98, × 0.3 nur bei shouldSlowDown() == isInSneakingPose().
        // Das ist das um einen Tick nachlaufende Bewegungs-Feld (in_sneaking_pose_move), NICHT
        // die aktuelle Hitbox-Pose (self.crouching) -- s. Feld-Kommentar. Ohne diesen Lag schickt
        // der Client beim Sneak-Loslassen im selben Tick schon 0.98*accel, waehrend Grim (wie
        // Vanilla) noch 0.294*accel erwartet -> grosser Simulation-Flag im Crouch->Sprint-Uebergang.
        // Reihenfolge wie in applyMovementSpeedFactors: erst 0.98, dann der Item-Faktor,
        // dann der Schleich-Faktor. Der Item-Faktor fehlte -- GrimAC rechnete ihn mit (es
        // sieht unsere USE_ITEM-Pakete), wir nicht, und die Differenz war im consoledebug als
        // Verhaeltnis 0.3/0.2 = 1.5 messbar.
        // Der Schleich-Faktor ist Vanillas SNEAKING_SPEED-ATTRIBUT, kein fester Wert: die
        // Verzauberung "Schleichender Schritt" hebt ihn je Stufe um 0.15 ueber die 0.3
        // hinaus. Ohne das schleicht man mit Schleichendem Schritt III mit 0.3 statt 0.75 --
        // GrimAC rechnet mit dem Attribut und flaggt die Differenz.
        let factor = 0.98
            * if input.using_item { input.item_use_speed } else { 1.0 }
            * if self.in_sneaking_pose_move { input.sneaking_speed } else { 1.0 };
        // applyDirectionalMovementSpeedFactors: Richtungs-Multiplikator, Laenge auf <=1 gedeckelt.
        let (af, as_) = (u_fwd.abs(), u_strafe.abs());
        let ratio = if af > as_ {
            as_ / af
        } else if as_ > 0.0 {
            af / as_
        } else {
            0.0
        };
        let dir_mult = (1.0 + ratio * ratio).sqrt();
        let h = (factor * dir_mult).min(1.0);
        let (fwd, strafe) = (u_fwd * h, u_strafe * h);
        // movementInputToVelocity: |input| <= 1, also keine erneute Normalisierung; × accel, drehen.
        // Vanilla nutzt MathHelper.sin/cos (Float-Tabelle), NICHT exaktes sin/cos -- sonst weicht
        // die Bewegungsrichtung um ~1e-15 ab (sichtbar an anticheat-test.coms Strafe/GCD-Anzeige:
        // xE-15 statt vanilla xE-30). yaw_rad ist bereits yaw_grad*(pi/180) in float (to_radians).
        let (sin_y, cos_y) = (mc_sin(input.yaw_rad as f64) as f64, mc_cos(input.yaw_rad as f64) as f64);
        self.vel.x += strafe * accel * cos_y - fwd * accel * sin_y;
        self.vel.z += fwd * accel * cos_y + strafe * accel * sin_y;
    }

    // move(MoverType.SELF, ...): Position anhand von self.vel fortschreiben, an Kollisionen
    // klemmen und die entsprechenden Geschwindigkeitskomponenten nullen -- identisch fuer
    // Luft/Wasser/Lava (Teil der Bewegungs-Engine, nicht von travelIn*() selbst).
    fn move_and_zero(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) -> (bool, bool, bool, bool) {
        // Sneak-Kantenschutz VOR der Kollisionsbewegung (wie Vanillas move(): erst
        // adjustMovementForSneaking, dann collide). clipAtLedge() == isSneaking() -> self.crouching;
        // isStandingOnSurface ~ on_ground; nur bei nicht-aufwaerts gerichteter Bewegung.
        // WICHTIG: nur die BEWEGUNG dieses Ticks beschneiden, NICHT self.vel -- Vanilla behaelt die
        // Geschwindigkeit (nur das Positions-Delta wird geklemmt), sonst weicht der naechste Tick
        // von Grims Simulation ab. Die Kollisions-Nullung unten greift dadurch an der Kante nicht
        // (geklemmte Achse kollidiert nicht), self.vel laeuft also unveraendert in Reibung/naechsten
        // Tick weiter -- exakt wie Vanilla.
        // Entity.move(), ganz am Anfang und VOR adjustMovementForSneaking: ist ein
        // movementMultiplier gesetzt (Laengenquadrat ueber 1e-7, etwa durch Spinnweben), wird
        // die Bewegung damit multipliziert, der Multiplikator geleert und die Geschwindigkeit
        // auf null gesetzt.
        // Zwei Dinge auf einmal: die Strecke DIESES Ticks wird gestaucht, und die
        // Geschwindigkeit wird auf null gesetzt -- der naechste Tick faengt also bei 0 an und
        // baut nur die eine Tick-Beschleunigung (Schwerkraft, Eingabe) neu auf. Genau daher
        // kommt das Kriechen im Netz: kein Aufsummieren ueber mehrere Ticks.
        //
        // Das Nullen betrifft self.vel, das Stauchen NUR die Bewegung -- deshalb wird der
        // Faktor auf `requested` gelegt und nicht auf self.vel: unten haengt
        // horizontal_collision am Vergleich von angefragter zu erreichter Bewegung, und
        // Vanilla vergleicht dort ebenfalls gegen die bereits gestauchte Bewegung.
        // Entity.move mit noPhysics (Zuschauer): einfach versetzen, keine Kollision.
        if input.spectator {
            self.pos += self.vel;
            self.horizontal_collision = false;
            self.minor_horizontal_collision = false;
            return (false, false, false, false);
        }
        let mut requested = self.vel;
        if self.movement_multiplier.length_squared() > 1.0e-7 {
            requested *= self.movement_multiplier;
            self.movement_multiplier = DVec3::ZERO;
            self.vel = DVec3::ZERO;
        }
        let mut move_vel = requested;
        // Auf `requested` statt self.vel: Vanillas adjustMovementForSneaking bekommt die
        // Bewegung uebergeben, also die schon gestauchte -- und self.vel ist nach dem Nullen
        // oben ohnehin nicht mehr die Bewegung dieses Ticks.
        // Player.maybeBackOffFromEdge: nicht fliegend.
        if self.sneak_edge_protection_enabled
            && self.crouching
            && !self.abilities.flying
            && self.is_standing_on_surface(world, shapes)
            && requested.y <= 0.0
        {
            let (dx, dz) = adjust_for_sneaking(self.pos, requested.x, requested.z, world, shapes, self.shape_context(), self.body);
            move_vel.x = dx;
            move_vel.z = dz;
        }
        self.finish_move(requested, move_vel, Some(input), input.sneak, world, shapes)
    }

    // Ein Kolben schiebt den Spieler (Entity.move mit MoverType.PISTON, s. app::pistons): kein
    // Kantenschutz, sonst derselbe Weg wie die eigene Bewegung. Vorher begrenzt Vanilla den
    // Schub je Tick und Achse auf insgesamt 0.51 -- mehrere Kolben (oder ein Block, der zwei
    // Zellen beruehrt) schieben zusammen also nie weiter als ein einzelner.
    pub fn piston_move(&mut self, movement: DVec3, game_tick: u64, world: &World, shapes: &world::BlockShapes) {
        let movement = self.limit_piston_movement(movement, game_tick);
        if movement == DVec3::ZERO {
            return;
        }
        let mut requested = movement;
        if self.movement_multiplier.length_squared() > 1.0e-7 {
            requested *= self.movement_multiplier;
            self.movement_multiplier = DVec3::ZERO;
            self.vel = DVec3::ZERO;
        }
        let input = self.last_input;
        self.finish_move(requested, requested, input.as_ref(), self.sneak_key, world, shapes);
    }

    // Entity.limitPistonMovement: nur EINE Achse (die erste von x, y, z mit Bewegung), deren
    // Summe ueber den Tick auf +-0.51 geklemmt wird.
    fn limit_piston_movement(&mut self, movement: DVec3, game_tick: u64) -> DVec3 {
        if movement.length_squared() <= 1.0e-7 {
            return movement;
        }
        if game_tick != self.piston_deltas_tick {
            self.piston_deltas = [0.0; 3];
            self.piston_deltas_tick = game_tick;
        }
        let Some(achse) = (0..3).find(|&i| movement[i] != 0.0) else { return DVec3::ZERO };
        let summe = (movement[achse] + self.piston_deltas[achse]).clamp(-0.51, 0.51);
        let schritt = summe - self.piston_deltas[achse];
        self.piston_deltas[achse] = summe;
        if schritt.abs() <= 1.0e-5 {
            return DVec3::ZERO;
        }
        let mut out = DVec3::ZERO;
        out[achse] = schritt;
        out
    }

    // Der Rest von Entity.move ab der Kollision: `requested` ist die angefragte Bewegung (nach
    // dem Stauchen im Netz), `move_vel` die nach dem Kantenschutz. `input` fehlt nur, solange
    // noch kein Tick lief; `sneak` ist die Sneak-Taste (Schleim federt dann nicht).
    fn finish_move(
        &mut self,
        requested: DVec3,
        move_vel: DVec3,
        input: Option<&Input>,
        sneak: bool,
        world: &World,
        shapes: &world::BlockShapes,
    ) -> (bool, bool, bool, bool) {
        let (new_pos, hit_x, hit_y_down, hit_y_up, hit_z) =
            move_and_collide(self.pos, move_vel, self.current_height(), self.on_ground, world, shapes, self.shape_context(), self.body);
        // Vanillas `vec3d` in Entity.move: die TATSAECHLICH zurueckgelegte Bewegung, also nach
        // Kantenschutz UND Kollision. Genau daran haengen unten horizontalCollision und die
        // Fallhoehe -- beide duerfen NICHT aus den Kollisions-Flags abgeleitet werden.
        let achieved = new_pos - self.pos;
        self.pos = new_pos;
        // Entity.move (Zeile 762-765): `if (Math.abs(movement.y) > 0.0 ||
        // isLogicalSideForUpdatingMovement()) { verticalCollision = movement.y != vec3d.y;
        // groundCollision = verticalCollision && movement.y < 0.0; setMovement(groundCollision,
        // ...) }`. Fuer den EIGENEN Spieler ist isLogicalSideForUpdatingMovement() wahr, der
        // Zweig laeuft also IMMER -- auch bei movement.y == 0, und dann ist das Ergebnis
        // zwangslaeufig `false`.
        //
        // ECHTER BUG gewesen (Grim "Simulation" beim Aufhoeren zu
        // crawlen, Offsets 0.009800 / 0.005351 / 0.002922): hier stand `if move_vel.y != 0.0`
        // um genau diese Zuweisung, d.h. bei movement.y == 0 blieb der ALTE Bodenkontakt
        // stehen. Anlass war eine Kosmetik-Meldung ("am Boden: false" im F3-Menue direkt nach
        // einer Server-Korrektur) -- die Anzeige stimmte damit, die Physik nicht mehr.
        //
        // Wirkung, aus Grims Offsets zurueckgerechnet: die drei Werte zerfallen mit
        // exakt 0.54602 bzw. 0.54607 pro Tick, also der Bodenreibung 0.6 × 0.91 = 0.546. Es ist
        // damit EINE einmalige Geschwindigkeits-Injektion, die danach nur noch ausklingt. Vier
        // Ticks Zerfall zurueckgerechnet ergibt 0.11027 -- und die Differenz zwischen
        // Boden- und Luftbeschleunigung bei vollem Vorwaerts-Input ist
        // 0.98 × (0.1 × 0.21600002/0.546³ − 0.02) = 0.11045, also 0.16 % daneben. Genau ein
        // Tick, in dem wir mit Boden- und Grim mit Luftbeschleunigung gerechnet hat.
        //
        // Der Fall trat nach JEDEM Teleport/Setback auf (dort ist vel exakt 0, also
        // movement.y == 0): der Setback erzeugte selbst den naechsten Flag, der zum naechsten
        // Setback fuehrte -- die Schleife, die auch nach den Posen-Korrekturen blieb.
        //
        // Der eine Tick "am Boden: false" nach einem Teleport ist also KEIN Fehler, sondern
        // genau das, was Vanilla dort meldet (und was GrimAC erwartet). Bitte nicht wieder
        // "reparieren".
        self.on_ground = hit_y_down;
        // Entity.move() ruft setMovement(groundCollision, horizontalCollision, vec3d) genau
        // hier -- also NACH dem Setzen von onGround und VOR getLandingPos/onEntityLand/
        // getVelocityMultiplier, die alle den Standblock brauchen. `achieved` ist Vanillas
        // vec3d, die tatsaechlich zurueckgelegte Bewegung.
        self.update_supporting_block(self.on_ground, achieved, world, shapes);
        // ECHTER BUG gewesen: hier stand `hit_x || hit_z`, also NUR die Kollisions-Flags des
        // Sweeps. Vanilla (Entity.move) vergleicht stattdessen die ANGEFRAGTE mit der
        // tatsaechlichen Bewegung:
        //   horizontalCollision = !approximatelyEquals(movement.x, vec3d.x) || (... .z)
        // Der Unterschied ist genau der Sneak-Kantenschutz: der beschneidet die Bewegung, ohne
        // dass irgendetwas kollidiert. Vanilla meldet dabei horizontalCollision = true, wir
        // meldeten false -- und dieses Bit geht in JEDEM Bewegungspaket an den Server.
        self.horizontal_collision = !approximately_equals(requested.x, achieved.x) || !approximately_equals(requested.z, achieved.z);
        // Entity.move(), direkt hinter horizontalCollision: minorHorizontalCollision gilt nur bei
        // horizontaler Kollision, und dann nach isHorizontalCollisionMinor(Bewegung).
        // ACHTUNG: Vanilla uebergibt die TATSAECHLICHE Bewegung (vec3d == `achieved`), nicht
        // die Differenz zur angefragten.
        self.minor_horizontal_collision = self.horizontal_collision && input.is_some_and(|input| is_horizontal_collision_minor(input, achieved));
        // Entity.move() ruft hier checkFallDamage auf, und LivingEntity holt darin den
        // Wasserschub nach (updateInWaterStateAndDoWaterCurrentPushing), solange isInWater()
        // noch falsch ist.
        // Das ist KEINE Doppelung des Schubs aus baseTick, sondern ein NACHSCHLAG fuer genau
        // den einen Tick, in dem man ins Wasser EINTAUCHT: isInWater() stammt aus baseTick,
        // wurde also an der Position VOR der Bewegung gemessen. Wer diesen Tick von oberhalb
        // der Wasseroberflaeche hineinfaellt, war dort noch trocken -- der Schub wird deshalb
        // erst jetzt, mit der neuen Box, nachgeholt.
        //
        // Der Nachschlag wirkt sich in DIESEM Tick seitlich nicht mehr aus (bewegt wurde schon),
        // aber handleRelativeFrictionAndCalculateMovement liest die Geschwindigkeit erst NACH
        // move() aus, travelInAir multipliziert sie also noch mit g = 0.91. Der Schub landet
        // damit um 0.91 gedaempft im naechsten Tick, wo dann der regulaere baseTick-Schub
        // dazukommt.
        //
        // Gemessen auf der Eis-/Wasserplattform von anticheat-test.com (Vanilla gegen uns,
        // beide durch denselben MITM-Proxy): Vanilla bewegt sich im Landetick um 0.026740,
        // wir bewegten uns um 0.014000. Die Differenz 0.012740 ist exakt 0.014 × 0.91, und
        // 0.014 × 0.91 + 0.014 = 0.026740. Danach folgen beide Reihen identisch
        // v <- 0.8 * v + 0.014.
        if !self.touching_water {
            let half = self.width() / 2.0;
            let hoehe = self.current_height();
            let box_min = DVec3::new(self.pos.x - half, self.pos.y, self.pos.z - half);
            let box_max = DVec3::new(self.pos.x + half, self.pos.y + hoehe, self.pos.z + half);
            if let Some(scan) = fluid_height_scan(box_min, box_max, world, shapes, world::FluidKind::Water) {
                apply_fluid_push(&mut self.vel, &scan, WATER_PUSH_SPEED);
                // updateInWaterStateAndDoWaterCurrentPushing() setzt bei Treffer beides:
                // resetFallDistance() und wasTouchingWater = true.
                self.fall_distance = 0.0;
                self.touching_water = true;
            }
        }
        // Entity.fall(): erst die Fallhoehe fortschreiben, dann beim Bodenkontakt nullen --
        // in dieser Reihenfolge, sonst stuende nach der Landung noch der letzte Zuwachs drin.
        // heightDifference ist die tatsaechliche Vertikalbewegung, und Vanilla castet sie auf
        // float, bevor sie von der (doppelt genauen) Fallhoehe abgezogen wird.
        if !self.touching_water && achieved.y < 0.0 {
            self.fall_distance -= achieved.y as f32 as f64;
        }
        if self.on_ground {
            self.fall_distance = 0.0;
        }
        if hit_x {
            self.vel.x = 0.0;
        }
        // Die Y-Nullung IST Vanillas Block.onEntityLand (`setVelocity(vel.multiply(1, 0, 1))`),
        // aufgerufen in Entity.move, sobald die Vertikalbewegung beschnitten wurde. Genau diese
        // Standardfassung ueberschreibt der Schleimblock -- deshalb steht die Fallunterscheidung
        // hier und nicht irgendwo daneben.
        //
        // Ausloeser bleibt bewusst das Kollisions-Flag des Sweeps (nicht Vanillas Vergleich
        // `movement.y != vec3d.y`): dieser Zweig ist seit Langem gegen Grim eingemessen, und
        // die Stufenhilfe veraendert die erreichte Y-Bewegung, ohne dass etwas kollidiert
        // waere. Fuer die Landung selbst ist beides deckungsgleich.
        if hit_y_down || hit_y_up {
            let landing = block_at_pos(world, self.landing_pos(world));
            let restitution = world::block_registry::block_name(landing).map(block_bounce_restitution).unwrap_or(0.0);
            // onEntityLand: Schleichen (bypassesLandingEffects() == isSneaking(), und
            // ClientPlayerEntity.isSneaking() ist die reine Sneak-TASTE) faellt auf die
            // Standardfassung zurueck -- daher das lautlose Landen mit gedruecktem Shift, auf
            // Schleim wie auf dem Bett.
            if restitution > 0.0 && !sneak {
                // bounce(): NUR eine negative Geschwindigkeit wird umgedreht. Sonst bleibt sie
                // stehen -- Vanilla nullt in diesem Zweig ausdruecklich nicht, weshalb ein
                // Deckenstoss ueber einem Schleimblock den Aufwaertsschwung behaelt.
                if self.vel.y < 0.0 {
                    self.vel.y = -self.vel.y * restitution;
                }
            } else {
                self.vel.y = 0.0;
            }
        }
        if hit_z {
            self.vel.z = 0.0;
        }
        // Entity.move(), allerletzter Schritt: die Geschwindigkeit wird waagerecht mit
        // getVelocityMultiplier() multipliziert. Steht damit VOR Schwerkraft/Reibung des
        // aufrufenden travel() -- der Faktor bremst also die Geschwindigkeit fuer den naechsten
        // Tick, nicht die Strecke dieses Ticks. Genau das erzeugt das zaehe Gefuehl auf
        // Seelensand/Honig: pro Tick 0.4 auf beide Horizontalachsen, waehrend die Eingabe
        // jeden Tick nur den kleinen Beschleunigungsbetrag wieder draufgibt.
        let f = self.velocity_multiplier(world);
        if f != 1.0 {
            self.vel.x *= f;
            self.vel.z *= f;
        }
        (hit_x, hit_y_down, hit_y_up, hit_z)
    }
}

// updateFluidHeightAndDoFluidPushing() ohne den Stroemungs-Schub-Vektor (s. Modul-Kommentar):
// AABB um 0.001 nach innen verkleinert, jede ueberlappte Fluessigkeits-Zelle derselben Art
// gegen die Boxunterkante geprueft, hoechste Ueberschneidung gewinnt. None = nicht beruehrt,
// Some(h) = Fuellstand in Blockeinheiten oberhalb der Boxunterkante (kann 0.0 sein: gerade so
// beruehrt).
// Ergebnis eines Fluessigkeits-Scans ueber die Spielerbox.
struct FluidScan {
    // Wie weit die Fluessigkeitsoberflaeche ueber der Boxunterkante steht (Vanillas `d`).
    height: f64,
    // Aufsummierter, noch UNgewichteter Stroemungsvektor aller beruehrten Zellen, plus deren
    // Anzahl -- die Mittelung passiert erst beim Anwenden (s. apply_fluid_push).
    push: DVec3,
    push_cells: u32,
}

fn fluid_height_scan(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes, kind: world::FluidKind) -> Option<FluidScan> {
    let min = min + DVec3::splat(0.001);
    let max = max - DVec3::splat(0.001);
    if min.x >= max.x || min.y >= max.y || min.z >= max.z {
        return None;
    }

    let (bx0, bx1) = (min.x.floor() as i32, max.x.ceil() as i32);
    let (by0, by1) = (min.y.floor() as i32, max.y.ceil() as i32);
    let (bz0, bz1) = (min.z.floor() as i32, max.z.ceil() as i32);

    let mut touching = false;
    let mut e = 0.0f64;
    let mut push = DVec3::ZERO;
    let mut push_cells = 0u32;
    for bx in bx0..bx1 {
        for by in by0..by1 {
            for bz in bz0..bz1 {
                let state = world.block_at(bx, by, bz);
                if world::fluid_kind(state) != Some(kind) {
                    continue;
                }
                let Some(h) = world::fluid_height_at(world, bx, by, bz) else {
                    continue;
                };
                let f = by as f64 + h;
                if f >= min.y {
                    touching = true;
                    e = e.max(f - min.y);
                    // Stroemungsschub, exakt wie Entity.updateMovementInFluid: der Vektor
                    // JEDER beruehrten Zelle zaehlt einzeln, flach eintauchende Zellen
                    // (e < 0.4) nur anteilig. `e` ist hier bewusst das LAUFENDE Maximum und
                    // nicht das Endergebnis -- Vanilla skaliert ebenfalls mit dem Stand
                    // waehrend der Schleife, das Ergebnis haengt damit an der
                    // Iterationsreihenfolge. Nachgebaut statt "verbessert", weil der Server
                    // (Anticheat) genau diese Reihenfolge rechnet.
                    if let Some(v) = world::fluid_flow_velocity(world, shapes, bx, by, bz) {
                        let mut v = DVec3::new(v[0], v[1], v[2]);
                        if e < 0.4 {
                            v *= e;
                        }
                        push += v;
                        push_cells += 1;
                    }
                }
            }
        }
    }
    touching.then_some(FluidScan { height: e, push, push_cells })
}

// Wendet den in `scan` gesammelten Stroemungsschub auf die Geschwindigkeit an -- zweite
// Haelfte von Entity.updateMovementInFluid.
//
// Zwei Feinheiten, die man leicht uebersieht und die beide fuer Anticheat-Gleichlauf zaehlen:
//   * Der gemittelte Vektor wird fuer SPIELER NICHT normiert (Vanilla normiert nur fuer
//     Nicht-Spieler). Ein Spieler in schwacher Stroemung wird also
//     wirklich schwaecher geschoben, ein Mob nicht.
//   * Steht man praktisch still und waere der Schub extrem klein, wird er auf eine
//     Mindestlaenge hochgezogen -- sonst bliebe man in ganz flachem Wasser ewig kleben.
fn apply_fluid_push(velocity: &mut DVec3, scan: &FluidScan, speed: f64) {
    if scan.push_cells == 0 || scan.push.length() <= 0.0 {
        return;
    }
    let mut v = scan.push / scan.push_cells as f64;
    v *= speed;
    if velocity.x.abs() < FLUID_PUSH_STILL_EPS && velocity.z.abs() < FLUID_PUSH_STILL_EPS && v.length() < FLUID_PUSH_MIN_SPEED {
        let len = v.length();
        if len > 0.0 {
            v = v / len * FLUID_PUSH_MIN_SPEED;
        }
    }
    *velocity += v;
}

// WorldView.containsFluid(box): beruehrt die Box IRGENDEINE Zelle mit nicht-leerem
// FluidState? Bewusst etwas ganz anderes als fluid_height_scan darueber:
//   * kein 0.001-Schrumpfen der Box,
//   * KEINE Fuellstands-Bedingung -- schon eine Zelle mit level=7 (Fuellhoehe ~0.1) zaehlt,
//     auch wenn ihre Oberflaeche weit unter der Boxunterkante liegt,
//   * Zellbereich floor(min)..ceil(max) mit AUSSCHLIESSENDER Obergrenze.
// Genau diese Unterschiede waren der Grund, warum der Wasser-Hop bisher zu oft feuerte
// (s. jump_out_of_fluid). fluid_kind() deckt Quell-/Fliessbloecke, waterlogged Bloecke und
// die immer-nassen (Seegras & Co.) ab -- also dasselbe wie !getFluidState().isEmpty().
fn contains_fluid(min: DVec3, max: DVec3, world: &World) -> bool {
    let (x0, x1) = (min.x.floor() as i32, max.x.ceil() as i32);
    let (y0, y1) = (min.y.floor() as i32, max.y.ceil() as i32);
    let (z0, z1) = (min.z.floor() as i32, max.z.ceil() as i32);
    for bx in x0..x1 {
        for by in y0..y1 {
            for bz in z0..z1 {
                if world::fluid_kind(world.block_at(bx, by, bz)).is_some() {
                    return true;
                }
            }
        }
    }
    false
}

// getFluidFallingAdjustedMovement(): Gravitation im Wasser/Lava NICHT voll, sondern nur
// d/16 pro Tick abziehen (und beim Sprinten GAR NICHT -- Sprint-Schwimmen bleibt auf Hoehe),
// plus ein Sonderfall, der beim Wiedereintauchen einen winzigen konstanten Sinkwert
// (-0.003) statt der Formel nimmt, um Ruckeln an der Wasseroberflaeche zu vermeiden.
// LivingEntity.getEffectiveGravity: mit Sanftem Fall faellt man hoechstens mit 0.01 --
// aber nur FALLEND (vel.y <= 0). Nach oben (Sprung) wirkt die volle Schwerkraft.
fn effective_gravity(vel_y: f64, input: &Input, gravity: f64) -> f64 {
    if vel_y <= 0.0 && input.slow_falling {
        gravity.min(0.01)
    } else {
        gravity
    }
}

// Block-Tag can_glide_through (Datenpaket des Jars), aufgeloest: gleitend kein Klettern.
const CAN_GLIDE_THROUGH: [&str; 7] =
    ["vine", "twisting_vines", "twisting_vines_plant", "weeping_vines", "weeping_vines_plant", "cave_vines", "cave_vines_plant"];

// Entity.getRotationVector(pitch, yaw): alles in float und mit der Sinus-Tabelle, erst das
// Ergebnis wird double. `pitch_deg` positiv = nach unten (Vanillas Konvention).
pub fn rotation_vector(pitch_deg: f32, yaw_deg: f32) -> DVec3 {
    const RADIANS_PER_DEGREE: f32 = (std::f64::consts::PI / 180.0) as f32;
    let f = pitch_deg * RADIANS_PER_DEGREE;
    let g = -yaw_deg * RADIANS_PER_DEGREE;
    let h = mc_cos(g as f64);
    let i = mc_sin(g as f64);
    let j = mc_cos(f as f64);
    let k = mc_sin(f as f64);
    DVec3::new((i * j) as f64, (-k) as f64, (h * j) as f64)
}

// LivingEntity.calcGlidingVelocity, in eigenen Worten:
//   - Schwerkraft, abgeschwaecht je flacher man blickt (cos^2 des Pitch, NICHT aus der
//     Tabelle, sondern Math.cos): waagerecht wirkt nur ein Viertel.
//   - Sinkt man, wird ein Teil des Sinkens in Vorwaertsfahrt umgesetzt.
//   - Blickt man nach OBEN, wird Fahrt in Hoehe umgesetzt (Tabellen-Sinus).
//   - Die waagerechte Geschwindigkeit dreht sich zu 10 % pro Tick in die Blickrichtung.
//   - Zum Schluss Daempfung 0.99 / 0.98 / 0.99 (float-Konstanten).
fn calc_gliding_velocity(alt: DVec3, rot: DVec3, pitch_deg: f32, gravity: f64) -> DVec3 {
    const RADIANS_PER_DEGREE: f32 = (std::f64::consts::PI / 180.0) as f32;
    let f = pitch_deg * RADIANS_PER_DEGREE;
    let d = (rot.x * rot.x + rot.z * rot.z).sqrt();
    let e = (alt.x * alt.x + alt.z * alt.z).sqrt();
    let h = (f as f64).cos().powi(2);
    let mut v = alt + DVec3::new(0.0, gravity * (-1.0 + h * 0.75), 0.0);
    if v.y < 0.0 && d > 0.0 {
        let i = v.y * -0.1 * h;
        v += DVec3::new(rot.x * i / d, i, rot.z * i / d);
    }
    if f < 0.0 && d > 0.0 {
        let i = e * (-mc_sin(f as f64)) as f64 * 0.04;
        v += DVec3::new(-rot.x * i / d, i * 3.2, -rot.z * i / d);
    }
    if d > 0.0 {
        v.x += (rot.x / d * e - v.x) * 0.1;
        v.z += (rot.z / d * e - v.z) * 0.1;
    }
    DVec3::new(v.x * 0.99f32 as f64, v.y * 0.98f32 as f64, v.z * 0.99f32 as f64)
}

// FireworkRocketEntity.tick, der Teil fuer eine Rakete am gleitenden Schuetzen: die
// Geschwindigkeit zieht zur Haelfte auf 1.5 Bloecke/Tick in Blickrichtung, dazu 0.1 Schub.
pub fn firework_boost(vel: DVec3, rot: DVec3) -> DVec3 {
    vel + DVec3::new(
        rot.x * 0.1 + (rot.x * 1.5 - vel.x) * 0.5,
        rot.y * 0.1 + (rot.y * 1.5 - vel.y) * 0.5,
        rot.z * 0.1 + (rot.z * 1.5 - vel.z) * 0.5,
    )
}

fn fluid_falling_adjusted_movement(vel: DVec3, gravity: f64, was_falling: bool, sprinting: bool) -> DVec3 {
    if gravity == 0.0 || sprinting {
        return vel;
    }
    let e = if was_falling && (vel.y - 0.005).abs() >= 0.003 && (vel.y - gravity / 16.0).abs() < 0.003 {
        -0.003
    } else {
        vel.y - gravity / 16.0
    };
    DVec3::new(vel.x, e, vel.z)
}

// canChangeIntoPose(pose): wuerde eine Box der gegebenen Hoehe an der aktuellen Position
// IRGENDETWAS Solides ueberlappen? Anders als sweep_axis (das eine BEWEGUNG um ein Delta
// klemmt) hier ein reiner STATISCHER Ueberlappungstest -- sweep_axis selbst kann das nicht
// leisten, weil es bei delta=0 sofort abbricht (s. dortigen fruehen Return). Dieselbe
// Block-Scan-/Kollisionsbox-Logik wie sweep_axis, nur ohne Bewegungsrichtung.
fn pose_fits(pos: DVec3, height: f64, world: &World, shapes: &world::BlockShapes, ctx: world::ShapeContext, body: BodyAttributes) -> bool {
    let half = body.width() / 2.0;
    // Vanilla verkleinert die Posen-Box vor der Pruefung um 1e-7 je Seite
    // (canChangeIntoPose: `...getBoxAt(pos).contract(1.0E-7)`, und contract(v) ist
    // expand(-v) auf allen sechs Seiten). GrimAC macht exakt dasselbe (canEnterPose:
    // `getBoundingBoxForPose(...).expand(-1.0E-7)`). Ein Block, der die Box nur genau
    // beruehrt, zaehlt damit NICHT als Kollision -- ohne diese Verkleinerung haengt das
    // Ergebnis an der letzten Nachkommastelle der Spielerposition, und genau dort liefen
    // wir und Grim auseinander.
    const POSE_SHRINK: f64 = 1.0e-7;
    box_is_empty(
        DVec3::new(pos.x - half + POSE_SHRINK, pos.y + POSE_SHRINK, pos.z - half + POSE_SHRINK),
        DVec3::new(pos.x + half - POSE_SHRINK, pos.y + height - POSE_SHRINK, pos.z + half - POSE_SHRINK),
        world,
        shapes,
        ctx,
    )
}

// true, wenn die AABB [min,max] keinerlei Block-Kollisionsbox ueberlappt (World.isSpaceEmpty).
// Aus pose_fits herausgezogen, damit auch der Sneak-Kantenschutz denselben Ueberlappungstest
// nutzt (s. adjust_for_sneaking).
fn box_is_empty(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes, ctx: world::ShapeContext) -> bool {
    let bx0 = min.x.floor() as i32;
    let bx1 = (max.x - EPS).floor() as i32;
    let by0 = min.y.floor() as i32;
    let by1 = (max.y - EPS).floor() as i32;
    let bz0 = min.z.floor() as i32;
    let bz1 = (max.z - EPS).floor() as i32;

    for bx in bx0..=bx1 {
        for by in by0..=by1 {
            for bz in bz0..=bz1 {
                let state = world.block_at(bx, by, bz);
                let (odx, odz) = world::block_name(state)
                    .map(|name| world::block_xz_offset(name, bx, bz))
                    .unwrap_or((0.0, 0.0));
                for cbox in world::collision_boxes_at(state, shapes, by, ctx) {
                    let bmin = DVec3::new(
                        bx as f64 + odx as f64 + cbox.min[0] as f64,
                        by as f64 + cbox.min[1] as f64,
                        bz as f64 + odz as f64 + cbox.min[2] as f64,
                    );
                    let bmax = DVec3::new(
                        bx as f64 + odx as f64 + cbox.max[0] as f64,
                        by as f64 + cbox.max[1] as f64,
                        bz as f64 + odz as f64 + cbox.max[2] as f64,
                    );
                    let overlaps = min.x < bmax.x - EPS
                        && max.x > bmin.x + EPS
                        && min.y < bmax.y - EPS
                        && max.y > bmin.y + EPS
                        && min.z < bmax.z - EPS
                        && max.z > bmin.z + EPS;
                    if overlaps {
                        return false;
                    }
                }
            }
        }
    }
    true
}

// Sneak-Kantenschutz -- PlayerEntity.adjustMovementForSneaking(), byte-getreu aus dem
// Yarn-1.21.11-Jar. Beim Schleichen (clipAtLedge() == isSneaking()) auf einer Flaeche
// (isStandingOnSurface, hier ueber on_ground) und bei Abwaerts-/Horizontalbewegung
// (movement.y <= 0) werden X und Z in 0.05-Schritten reduziert, solange der Boden-Streifen
// unter der um (dx,dz) verschobenen Box LEER waere -- man clippt also an der Blockkante
// statt hinunterzufallen. Kein Anticheat-Belang (Grim flaggt das Fehlen nicht), reines
// Vanilla-Spielgefuehl.
// UseEffectsComponent.DEFAULT.speedMultiplier() -- der Faktor, mit dem Vanilla die
// Bewegungseingabe waehrend einer Item-Benutzung daempft (wirksam: Input::item_use_speed).
#[cfg(test)]
pub const ITEM_USE_SPEED_MULT: f64 = 0.2;
const SNEAK_STEP: f64 = 0.05; // STEPPING_APPROACH_DISTANCE

// MathHelper.approximatelyEquals -- Vanillas Vergleich fuer horizontalCollision.
fn approximately_equals(a: f64, b: f64) -> bool {
    (b - a).abs() < 1.0e-5
}

// ClientPlayerEntity.isHorizontalCollisionMinor(movement), nach dem 1.21.11-Client: die
// gewollte Richtung entsteht aus den Eingabe-Impulsen, um den Yaw gedreht (Vanillas sin/cos).
// Ist ihr Laengenquadrat oder das der tatsaechlichen waagerechten Bewegung unter 1e-5, ist der
// Kontakt nicht "minor". Sonst entscheidet der Winkel zwischen beiden (acos des normierten
// Skalarprodukts) gegen 0.13962634 rad.
//
// Gelesen: zeigt die tatsaechlich zurueckgelegte Bewegung noch innerhalb von 8 Grad
// (0.13962634 rad) in die gewollte Richtung, war der Wandkontakt nur ein Streifen.
//
// `sidewaysSpeed`/`forwardSpeed` sind die ROHEN Eingabe-Impulse (-1/0/1), nicht die
// normierten aus add_move_input -- fuer den Winkel ist die Laenge ohnehin egal, sie geht nur
// in die beiden 1e-5-Schwellen ein, und dort sind die Rohwerte das Richtige.
// sin/cos kommen aus Vanillas Float-Tabelle (mc_sin/mc_cos), damit der Winkel bitgleich faellt.
fn is_horizontal_collision_minor(input: &Input, achieved: DVec3) -> bool {
    let (sin_y, cos_y) = (mc_sin(input.yaw_rad as f64) as f64, mc_cos(input.yaw_rad as f64) as f64);
    let (strafe, forward) = (input.strafe as f64, input.forward as f64);
    let g = strafe * cos_y - forward * sin_y;
    let h = forward * cos_y + strafe * sin_y;
    let i = g * g + h * h;
    let j = achieved.x * achieved.x + achieved.z * achieved.z;
    // Die Schwellen sind in Vanilla float-Literale, der Vergleich laeuft aber in double.
    if i < 1.0e-5f32 as f64 || j < 1.0e-5f32 as f64 {
        return false;
    }
    let k = g * achieved.x + h * achieved.z;
    (k / (i * j).sqrt()).acos() < 0.13962634f32 as f64
}

// PlayerEntity.isSpaceAroundPlayerEmpty(offsetX, offsetZ, stepHeight): der Streifen
// minY-stepHeight .. minY unter der um (dx,dz) verschobenen Fussflaeche, ringsum um 1e-7 nach
// innen geschrumpft. LEER heisst: dort unten ist kein Block -- also eine Kante.
// Die Schritthoehe ist ein PARAMETER, kein fester Wert: isStandingOnSurface reicht
// stepHeight - fallDistance herein.
fn space_around_player_empty(
    pos: DVec3,
    dx: f64,
    dz: f64,
    step_height: f64,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    body: BodyAttributes,
) -> bool {
    let half = body.width() / 2.0;
    let e = 1.0e-7;
    let min = DVec3::new(pos.x - half + e + dx, pos.y - step_height - e, pos.z - half + e + dz);
    let max = DVec3::new(pos.x + half - e + dx, pos.y, pos.z + half - e + dz);
    box_is_empty(min, max, world, shapes, ctx)
}

fn adjust_for_sneaking(
    pos: DVec3,
    vel_x: f64,
    vel_z: f64,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    body: BodyAttributes,
) -> (f64, f64) {
    let space_empty = |dx: f64, dz: f64| -> bool { space_around_player_empty(pos, dx, dz, body.step_height, world, shapes, ctx, body) };
    let mut d = vel_x;
    let mut e = vel_z;
    let hx = d.signum() * SNEAK_STEP;
    let hz = e.signum() * SNEAK_STEP;
    // Erst X allein, dann Z allein, dann beide gemeinsam abbauen -- exakt Vanillas drei Schleifen.
    while d != 0.0 && space_empty(d, 0.0) {
        if d.abs() <= SNEAK_STEP {
            d = 0.0;
            break;
        }
        d -= hx;
    }
    while e != 0.0 && space_empty(0.0, e) {
        if e.abs() <= SNEAK_STEP {
            e = 0.0;
            break;
        }
        e -= hz;
    }
    while d != 0.0 && e != 0.0 && space_empty(d, e) {
        if d.abs() <= SNEAK_STEP {
            d = 0.0;
        } else {
            d -= hx;
        }
        if e.abs() <= SNEAK_STEP {
            e = 0.0;
        } else {
            e -= hz;
        }
    }
    (d, e)
}

// Bewegt die Spieler-Boundingbox achsenweise (Y, dann X, dann Z) gegen die echten
// Kollisionsboxen der ueberlappenden Bloecke (s. world::collision_boxes -- ein Vollwuerfel
// fuer die meisten Terrain-Bloecke, aber z.B. nur die halbe Hoehe fuer eine Stufe), klemmt
// jede Achse einzeln an der ersten Kollision. Gibt die neue Fuessposition und je Achse
// zurueck, ob dort eine Kollision die Bewegung verkuerzt hat (down/up getrennt fuer Y, weil
// nur "nach unten blockiert" == Bodenkontakt bedeutet).
// Eine achsenparallele Kollisionsbox in WELTkoordinaten (Blockposition + xz-Offset schon
// eingerechnet, s. collect_collision_boxes) -- Vanillas VoxelShape-Liste entspricht hier
// einer flachen Liste solcher Boxen.
#[derive(Clone, Copy)]
struct Aabb {
    min: DVec3,
    max: DVec3,
}

// Box.stretch(dx,dy,dz): die Box in Bewegungsrichtung aufziehen, ohne die Gegenseite zu
// verschieben -- der Bereich, den die Box auf ihrem Weg ueberstreicht.
fn stretch(min: DVec3, max: DVec3, delta: DVec3) -> (DVec3, DVec3) {
    let (mut lo, mut hi) = (min, max);
    for axis in 0..3 {
        if delta[axis] < 0.0 {
            lo[axis] += delta[axis];
        } else {
            hi[axis] += delta[axis];
        }
    }
    (lo, hi)
}

// Alle Block-Kollisionsboxen, die den Bereich [min,max] tatsaechlich schneiden -- Vanillas
// World.getBlockCollisions(entity, box). EINMAL eingesammelt statt pro Achse neu gescannt:
// Vanillas Step-Up befragt denselben Satz Boxen mehrfach (adjustMovementForCollisions laeuft
// je Kandidatenhoehe erneut ueber dieselbe Liste `list2`), und die Kandidatenhoehen selbst
// werden ebenfalls aus genau dieser Liste gezogen.
// Entity.adjustMovementForCollisions(null, movement, box, world, List.of()) -- die Fassung
// OHNE Entity und ohne Stufensteigen, mit der Partikel sich bewegen (s. particles.rs).
pub fn collide_box(min: DVec3, max: DVec3, movement: DVec3, world: &World, shapes: &world::BlockShapes) -> DVec3 {
    let (lo, hi) = stretch(min, max, movement);
    let boxes = collect_collision_boxes(lo, hi, world, shapes, world::ShapeContext::ABSENT);
    adjust_movement(min, max, movement, &boxes)
}

// --- Fuer andere Koerper als den Spieler (s. vehicle.rs) ------------------------------------

// Steht IRGENDEIN Block mit seiner Kollisionsform in der Box? World.isSpaceEmpty ohne Entities.
pub(crate) fn space_empty(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> bool {
    collect_collision_boxes(min, max, world, shapes, world::ShapeContext::ABSENT).is_empty()
}

// Schneidet die Kollisionsform des Blocks (bx, by, bz) die Box mit echtem Volumen?
// VoxelShapes.matchesAnywhere(Form, Box, AND).
pub(crate) fn block_shape_overlaps(bx: i32, by: i32, bz: i32, min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> bool {
    let state = world.block_at(bx, by, bz);
    let (odx, odz) = world::block_name(state).map(|name| world::block_xz_offset(name, bx, bz)).unwrap_or((0.0, 0.0));
    world::collision_boxes_at(state, shapes, by, world::ShapeContext::ABSENT).iter().any(|b| {
        let lo = DVec3::new(bx as f64 + odx as f64 + b.min[0] as f64, by as f64 + b.min[1] as f64, bz as f64 + odz as f64 + b.min[2] as f64);
        let hi = DVec3::new(bx as f64 + odx as f64 + b.max[0] as f64, by as f64 + b.max[1] as f64, bz as f64 + odz as f64 + b.max[2] as f64);
        lo.x < max.x && hi.x > min.x && lo.y < max.y && hi.y > min.y && lo.z < max.z && hi.z > min.z
    })
}

// Block.getSlipperiness je Blockname (s. block_friction).
pub(crate) fn friction_of(name: &str) -> f64 {
    block_friction(name)
}

pub(crate) fn velocity_multiplier_of(name: &str) -> f64 {
    block_velocity_multiplier(name)
}

pub(crate) fn bounce_restitution_of(name: &str) -> f64 {
    block_bounce_restitution(name)
}

// Entity.updateMovementInFluid(WATER, 0.014) fuer alles, was kein Spieler ist: wie
// apply_fluid_push, nur wird der gemittelte Stroemungsvektor vorher auf Laenge 1 gebracht.
// Liefert, ob die Box Wasser beruehrt.
pub(crate) fn push_by_water(min: DVec3, max: DVec3, vel: &mut DVec3, world: &World, shapes: &world::BlockShapes) -> bool {
    let Some(scan) = fluid_height_scan(min, max, world, shapes, world::FluidKind::Water) else { return false };
    if scan.push_cells > 0 && scan.push.length() > 0.0 {
        let gemittelt = (scan.push / scan.push_cells as f64).normalize_or_zero();
        let normiert = FluidScan { height: scan.height, push: gemittelt, push_cells: 1 };
        apply_fluid_push(vel, &normiert, WATER_PUSH_SPEED);
    }
    true
}

thread_local! {
    // PistonMovingBlockEntity.NOCLIP: die Richtung, in die ein Kolben gerade schiebt, solange
    // er schiebt (s. piston_move). Bloecke, die in genau diese Richtung fahren, halten den
    // Geschobenen dabei nicht auf -- sonst stiesse er sich an dem Block, der ihn schiebt.
    static PISTON_NOCLIP: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

// Fuer die Dauer von `f` gilt `richtung` als NOCLIP-Richtung (s. PISTON_NOCLIP).
pub fn with_piston_noclip<R>(richtung: u8, f: impl FnOnce() -> R) -> R {
    PISTON_NOCLIP.with(|c| c.set(Some(richtung)));
    let r = f();
    PISTON_NOCLIP.with(|c| c.set(None));
    r
}

fn collect_collision_boxes(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes, ctx: world::ShapeContext) -> Vec<Aabb> {
    let mut out = collect_boxes(min, max, world, &|state, by| world::collision_boxes_at(state, shapes, by, ctx));
    // Vom Kolben bewegte Bloecke: ihre Zelle (moving_piston) hat keine Form, die Boxen haengen
    // an der Bewegung (s. World::moving_boxes).
    let noclip = PISTON_NOCLIP.with(|c| c.get());
    for b in &world.moving_boxes {
        if b.noclip.is_some() && b.noclip == noclip {
            continue;
        }
        let (bmin, bmax) = (DVec3::from_array(b.min), DVec3::from_array(b.max));
        if bmin.x < max.x && bmax.x > min.x && bmin.y < max.y && bmax.y > min.y && bmin.z < max.z && bmax.z > min.z {
            out.push(Aabb { min: bmin, max: bmax });
        }
    }
    out
}

// Dasselbe fuer die Verfolgerkamera: Vanilla misst sie gegen getCameraCollisionShape, die sich
// nur bei Seelensand von der Kollisionsform unterscheidet (dort Vollwuerfel, s.
// world::camera_collision_boxes) -- ohne das saenke die Kamera in den Sand ein, in dem der
// Spieler steht.
fn collect_camera_boxes(min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> Vec<Aabb> {
    collect_boxes(min, max, world, &|state, _| world::camera_collision_boxes(state, shapes))
}

fn collect_boxes(min: DVec3, max: DVec3, world: &World, boxes_of: &dyn Fn(u32, i32) -> Vec<world::ShapeBox>) -> Vec<Aabb> {
    let bx0 = min.x.floor() as i32;
    let bx1 = (max.x - EPS).floor() as i32;
    // Y nach unten um MAX_COLLISION_OVERFLOW_BLOCKS erweitert: ein Zaun/eine Mauer/ein Tor
    // steht in Zelle Y, seine Kollisionsbox reicht aber bis Y+1.5 (s. vanilla_shapes.rs) --
    // ragt also in die Zelle DARUEBER hinein. Ohne die Erweiterung faellt genau diese
    // Zelle (Y) aus dem Scan, sobald der Spieler hoch genug ist (z.B. mitten im Sprung),
    // und man laeuft/springt mitten durch den eigentlich noch blockierenden oberen Teil --
    // beobachtet als "kann ueber den Zaun laufen, wird vom Server dafuer zurueckgesetzt".
    let by0 = (min.y - MAX_COLLISION_OVERFLOW_BLOCKS).floor() as i32;
    let by1 = (max.y - EPS).floor() as i32;
    let bz0 = min.z.floor() as i32;
    let bz1 = (max.z - EPS).floor() as i32;

    let mut out = Vec::new();
    for bx in bx0..=bx1 {
        for by in by0..=by1 {
            for bz in bz0..=bz1 {
                let state = world.block_at(bx, by, bz);
                // Gegen minecraft.wiki/w/Block_offset verifiziert (s. world::block_offset-
                // Kommentar): getOffset() verschiebt in Vanilla NICHT nur die Renderposition,
                // sondern auch die Kollisionsbox -- ohne das kollidiert z.B. Bambus immer an
                // der Zellenmitte statt an seiner tatsaechlichen (leicht schiefen) Position,
                // waehrend der echte Server dort schon verschoben kollidiert (Server-Flag
                // beim Durchlaufen von Bambus).
                let (odx, odz) = world::block_name(state)
                    .map(|name| world::block_xz_offset(name, bx, bz))
                    .unwrap_or((0.0, 0.0));
                for cbox in boxes_of(state, by) {
                    let bmin = DVec3::new(
                        bx as f64 + odx as f64 + cbox.min[0] as f64,
                        by as f64 + cbox.min[1] as f64,
                        bz as f64 + odz as f64 + cbox.min[2] as f64,
                    );
                    let bmax = DVec3::new(
                        bx as f64 + odx as f64 + cbox.max[0] as f64,
                        by as f64 + cbox.max[1] as f64,
                        bz as f64 + odz as f64 + cbox.max[2] as f64,
                    );
                    // Nur Boxen, die den gesuchten Bereich wirklich schneiden. Ohne diesen
                    // Filter kaemen aus einer nur teilweise erfassten Zelle Boxen mit, die
                    // gar nicht im Weg liegen -- fuer die Achsen-Klemmung harmlos, fuer die
                    // Stufen-Kandidaten aber nicht: die zieht ihre Hoehen aus genau dieser
                    // Liste und wuerde sonst auf Kanten steigen, die nirgends beruehrt werden.
                    if bmin.x < max.x && bmax.x > min.x && bmin.y < max.y && bmax.y > min.y && bmin.z < max.z && bmax.z > min.z {
                        out.push(Aabb { min: bmin, max: bmax });
                    }
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    X,
    Y,
    Z,
}

// Direction.getCollisionOrder(): Y zuerst, danach die betragsmaessig KLEINERE Horizontale,
// zuletzt die groessere (`|x| < |z| ? YZX : YXZ`). Das ist keine Kosmetik -- in einer
// Innenecke gibt erst die eine Achse die andere frei, das Ergebnis haengt also an der
// Reihenfolge. Vorher stand hier fest Y,X,Z.
fn collision_order(movement: DVec3) -> [Axis; 3] {
    if movement.x.abs() < movement.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    }
}

// VoxelShapes.calculateMaxOffset fuer EINE Achse: wie weit sich [min,max] entlang `axis` um
// `delta` bewegen laesst, bevor sie in eine der Boxen liefe. Nur Boxen, die auf den beiden
// ANDEREN Achsen mit der Spieler-Box ueberlappen, koennen ueberhaupt blockieren.
fn clamp_axis(min: DVec3, max: DVec3, delta: f64, axis: Axis, boxes: &[Aabb]) -> f64 {
    if delta.abs() < 1.0e-9 {
        return 0.0;
    }
    let mut allowed = delta;
    for b in boxes {
        let (bmin, bmax) = (b.min, b.max);
        let overlaps_other_axes = match axis {
            Axis::X => min.y < bmax.y && max.y > bmin.y && min.z < bmax.z && max.z > bmin.z,
            Axis::Y => min.x < bmax.x && max.x > bmin.x && min.z < bmax.z && max.z > bmin.z,
            Axis::Z => min.x < bmax.x && max.x > bmin.x && min.y < bmax.y && max.y > bmin.y,
        };
        if !overlaps_other_axes {
            continue;
        }

        let (cur_min, cur_max, box_min, box_max) = match axis {
            Axis::X => (min.x, max.x, bmin.x, bmax.x),
            Axis::Y => (min.y, max.y, bmin.y, bmax.y),
            Axis::Z => (min.z, max.z, bmin.z, bmax.z),
        };

        if delta > 0.0 && box_min >= cur_max - EPS {
            allowed = allowed.min((box_min - cur_max).max(0.0));
        } else if delta < 0.0 && box_max <= cur_min + EPS {
            allowed = allowed.max(-((cur_min - box_max).max(0.0)));
        }
    }
    allowed
}

// Entity.adjustMovementForCollisions(Vec3d, Box, List): die drei Achsen NACHEINANDER klemmen,
// jede gegen die um das bisher Erreichte verschobene Box.
//
// Die Reihenfolge kommt aus collision_order() -- s. dortigen Kommentar.
fn adjust_movement(min: DVec3, max: DVec3, movement: DVec3, boxes: &[Aabb]) -> DVec3 {
    if boxes.is_empty() {
        return movement;
    }
    let mut result = DVec3::ZERO;
    for axis in collision_order(movement) {
        let d = match axis {
            Axis::X => movement.x,
            Axis::Y => movement.y,
            Axis::Z => movement.z,
        };
        if d == 0.0 {
            continue;
        }
        let e = clamp_axis(min + result, max + result, d, axis, boxes);
        match axis {
            Axis::X => result.x = e,
            Axis::Y => result.y = e,
            Axis::Z => result.z = e,
        }
    }
    result
}

// Entity.collectStepHeights: die in Frage kommenden Stufenhoehen, aufsteigend sortiert und
// ohne Dubletten. Kandidat ist jede Y-Kante einer Kollisionsbox, relativ zur Unterkante der
// Spielerbox -- also genau "wie hoch muesste ich steigen, damit meine Fuesse auf dieser
// Kante stehen".
//
// Zwei Vanilla-Details, die leicht untergehen:
//  * Die Hoehen werden in FLOAT gerechnet und verglichen (Vanilla rechnet den Abstand
//    zur Box-Unterkante als float), nicht in double. Fuer Grim muss die resultierende Bewegung
//    bit-genau passen, deshalb hier ebenso f32.
//  * Die Hoehe, die die Basisbewegung ohnehin schon erreicht (base_dy), faellt raus -- sie
//    wuerde dieselbe Bewegung nur nochmal liefern.
// Vanilla bricht die Schleife bei der ersten Hoehe ueber der Stufenhoehe ab (seine
// Punktliste ist aufsteigend); bei unserer unsortierten Boxliste ist das gleichbedeutend
// mit "solche Hoehen weglassen".
fn collect_step_heights(box_min_y: f64, boxes: &[Aabb], step_height: f64, base_dy: f64) -> Vec<f32> {
    let exclude = base_dy as f32;
    let limit = step_height as f32;
    let mut out: Vec<f32> = Vec::new();
    for b in boxes {
        for edge in [b.min.y, b.max.y] {
            let g = (edge - box_min_y) as f32;
            if g < 0.0 || g == exclude || g > limit {
                continue;
            }
            if !out.contains(&g) {
                out.push(g);
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).expect("Stufenhöhen sind nie NaN"));
    out
}

// Die Weltgrenze als Wand dazu, sobald der Koerper (Mitte aus box_min/box_max) nahe genug
// davor steht -- gemessen an der ueberstrichenen Box swept_min/swept_max (s.
// world::world_border::WorldBorder::collision_boxes). Ihre Kanten liegen weit ober- und
// unterhalb jeder Stufenhoehe und fallen bei den Stufen-Kandidaten deshalb heraus.
fn add_border_boxes(boxes: &mut Vec<Aabb>, box_min: DVec3, box_max: DVec3, swept_min: DVec3, swept_max: DVec3, world: &World) {
    let mitte = (box_min + box_max) * 0.5;
    let ausdehnung = (swept_max.x - swept_min.x, swept_max.z - swept_min.z);
    for (min, max) in world.border.collision_boxes(mitte.x, mitte.z, ausdehnung) {
        boxes.push(Aabb { min: DVec3::from_array(min), max: DVec3::from_array(max) });
    }
}

fn horizontal_length_squared(v: DVec3) -> f64 {
    v.x * v.x + v.z * v.z
}

// Entity.adjustMovementForCollisions(Vec3d): Basisbewegung klemmen und, falls sie horizontal
// blockiert wurde, versuchen, das Hindernis zu ERSTEIGEN statt daran haengenzubleiben.
//
// Genau das ist Minecrafts "Stepping": Stufen, Slabs, Teppiche und jede andere Kante bis
// STEP_HEIGHT ueberwindet man im Laufen, ohne zu springen. Die Bedingung dafuer ist eng --
// nur wenn man am Boden ist (oder in genau diesem Tick gelandet ist) UND horizontal
// angestossen ist. Sonst koennte man an einer Wand hochlaufen.
fn adjust_movement_with_step(
    box_min: DVec3,
    box_max: DVec3,
    movement: DVec3,
    on_ground: bool,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    step_height: f64,
) -> DVec3 {
    let (r_min, r_max) = stretch(box_min, box_max, movement);
    let mut boxes = collect_collision_boxes(r_min, r_max, world, shapes, ctx);
    add_border_boxes(&mut boxes, box_min, box_max, r_min, r_max, world);
    let base = if movement.length_squared() == 0.0 { movement } else { adjust_movement(box_min, box_max, movement, &boxes) };

    let clipped_x = movement.x != base.x;
    let clipped_y = movement.y != base.y;
    let clipped_z = movement.z != base.z;
    // "In diesem Tick auf etwas gelandet" -- dann wird von der GELANDETEN Hoehe aus
    // gestiegen, nicht von der Ausgangshoehe.
    let landed = clipped_y && movement.y < 0.0;

    if !((landed || on_ground) && (clipped_x || clipped_z)) {
        return base;
    }

    let lift = if landed { DVec3::new(0.0, base.y, 0.0) } else { DVec3::ZERO };
    let (b2_min, b2_max) = (box_min + lift, box_max + lift);

    let (mut s_min, s_max) = stretch(b2_min, b2_max, DVec3::new(movement.x, step_height, movement.z));
    // Ohne Landung in diesem Tick zusaetzlich ein Hauch nach unten (Vanilla: stretch(0,
    // -1.0E-5, 0)) -- sonst faende der Scan die Box, auf der man GERADE STEHT, nicht mehr,
    // und ihre Oberkante fiele als Kandidat weg.
    if !landed {
        s_min.y -= 1.0e-5;
    }
    let mut step_boxes = collect_collision_boxes(s_min, s_max, world, shapes, ctx);
    add_border_boxes(&mut step_boxes, box_min, box_max, s_min, s_max, world);

    for g in collect_step_heights(b2_min.y, &step_boxes, step_height, base.y) {
        let attempt = DVec3::new(movement.x, g as f64, movement.z);
        let stepped = adjust_movement(b2_min, b2_max, attempt, &step_boxes);
        // Nur uebernehmen, wenn man dadurch horizontal WEITER kommt -- eine Stufe, die einen
        // nur anhebt, ohne das Hindernis zu loesen, waere ein Aufzug an der Wand.
        if horizontal_length_squared(stepped) > horizontal_length_squared(base) {
            // Ergebnis ist relativ zu box2; bei einer Landung lag die schon um base.y
            // tiefer, das muss wieder drauf.
            return stepped - DVec3::new(0.0, box_min.y - b2_min.y, 0.0);
        }
    }
    base
}

// Vanillas Camera.clipToSpace(): wie weit die Kamera in der 3.-Person-Ansicht wirklich nach
// hinten darf, ohne in eine Wand zu rutschen. Acht Strahlen von den Ecken eines kleinen
// Wuerfels um das Auge (+-0.1 je Achse) entlang `back_dir`; der naechste Treffer verkuerzt
// die Distanz. Die acht Ecken statt eines einzelnen Strahls sind der Grund, warum Vanillas
// Kamera an Ecken nicht durch die Wand blitzt.
//
// Vanilla misst gegen ShapeType.VISUAL (die Umriss-Form); hier sind es die Kollisionsboxen --
// fuer Vollbloecke identisch, bei Sonderformen (Zaun, Treppe) einen Hauch anders. Der
// Unterschied ist rein optisch: die Kamera ist Client-Sache, kein Server sieht sie.
pub fn clip_camera_distance(eye: DVec3, back_dir: DVec3, distance: f64, world: &World, shapes: &world::BlockShapes) -> f64 {
    let mut distance = distance;
    for i in 0..8 {
        let g = ((i & 1) * 2 - 1) as f64;
        let h = ((i >> 1 & 1) * 2 - 1) as f64;
        let j = ((i >> 2 & 1) * 2 - 1) as f64;
        let from = eye + DVec3::new(g * 0.1, h * 0.1, j * 0.1);
        let to = from + back_dir * distance;
        let Some(hit) = raycast_blocks(from, to, world, shapes) else {
            continue;
        };
        // Gemessen wird ab dem AUGE, nicht ab der (leicht versetzten) Strahl-Ecke -- wie
        // Vanilla (Abstandsquadrat vom Trefferpunkt zur eigenen Position).
        let k = (hit - eye).length_squared();
        if k < distance * distance {
            distance = k.sqrt();
        }
    }
    distance
}

// Was ein Rechtsklick getroffen hat -- Vanillas BlockHitResult, auf die Felder reduziert,
// die ins Paket gehen (s. protocol::packets::use_item_on_payload).
#[derive(Clone, Copy)]
pub struct BlockRayHit {
    pub block: (i32, i32, i32),
    // Richtungs-Index wie im Vanilla-Enum: 0 unten, 1 oben, 2 Nord, 3 Sued, 4 West, 5 Ost.
    pub face: i32,
    // Trefferstelle RELATIV zur Blockecke (0..1 je Achse), so wie sie ins Paket geht.
    pub cursor: (f32, f32, f32),
    pub inside: bool,
}

// Strahl vom Auge in Blickrichtung gegen die Blockformen -- die Vorlage fuer den Rechtsklick
// (Vanillas World.raycast mit RaycastContext).
//
// TEILWEISE BEWUSSTE ABWEICHUNG: gemessen wird gegen world::outline_boxes -- Vanillas
// UMRISS-Form (ShapeType.OUTLINE), soweit wir sie fuehren. Bloecke, deren Kollisionsform
// kleiner ist als ihr Umriss (Seelensand, Honig), trifft der Strahl dadurch als vollen
// Wuerfel, genau wie in Vanilla. Was weiterhin abweicht, sind Bloecke ganz OHNE Kollision,
// aber mit Umriss (Blumen, hohes Gras): die zaehlen bei uns als begehbar und damit als
// umrisslos, wir schauen also durch sie hindurch auf den Boden dahinter, waehrend Vanilla sie
// selbst trifft. Beim Platzieren ist das Ergebnis meist identisch, weil solche Bloecke
// ersetzbar sind -- ausgeschlossen ist ein Unterschied aber nicht.
//
// Der Strahl misst gegen World::block_at, sieht die eigene Platzier-Vorhersage also MIT --
// wie Vanilla, das gegen seine ClientWorld samt Vorhersage misst. Damit laesst sich auf einen
// gerade gesetzten Block sofort weiterbauen, ohne auf die Bestaetigung zu warten.
//
// VORGESCHICHTE, damit niemand im Kreis laeuft: hier stand eine Zeitlang server_block_at, der
// Strahl ignorierte die eigene Vorhersage also bewusst. Anlass war eine Kette von GrimAC-Flags
// "AirLiquidPlace" (x0 bis x4 am Stueck) beim schnellen Bauen. Die Diagnose war unvollstaendig:
// im Grim-Jar (2.3.74) prueft AirLiquidPlace ausschliesslich, ob der Block, GEGEN
// den geklickt wurde, in GRIMS Welt (compensatedWorld) Luft oder Fluessigkeit ist -- und Grim
// fuehrt die Platzierungs-Vorhersage des Clients selbst mit (CheckManagerListener umschliesst
// jede Platzierung mit compensatedWorld.startPredicting()/stopPredicting(), BlockPlaceResult
// setzt den Block). Gegen einen frisch gesetzten eigenen Block zu bauen ist fuer Grim also
// voellig regulaer, solange die ERSTE Platzierung durchging.
//
// Geflaggt hat folglich nur der Fall, in dem die erste Platzierung ABGELEHNT wurde
// (geschuetztes Gebiet, Plugin, ein von Grim selbst gecanceltes Paket): dann steht bei uns ein
// Geisterblock, in Grims Welt aber Luft. Genau dieser Fall trifft einen Vanilla-Client
// identisch -- auch der baut gegen seinen eigenen, noch unbestaetigten Block. Dafuer das
// normale Bauen auszubremsen war der falsche Tausch (auf Geisterbloecken liess sich sonst nichts
// weiter platzieren).
//
// Die Vorhersage selbst bleibt bewusst konservativ (s. block_place::default_state_for_item:
// nur eindeutige Bloecke, nur auf ersetzbare Stellen, nie in den Spieler hinein) -- sie ist
// damit eine TEILMENGE dessen, was Vanilla vorhersagt, und kann keine Geisterbloecke erzeugen,
// die ein Vanilla-Client nicht auch haette.
pub fn raycast_block(eye: DVec3, dir: DVec3, reach: f64, world: &World, shapes: &world::BlockShapes) -> Option<BlockRayHit> {
    let to = eye + dir * reach;
    let region_min = eye.min(to);
    let region_max = eye.max(to);
    let (bx0, by0, bz0) = (region_min.x.floor() as i32, region_min.y.floor() as i32, region_min.z.floor() as i32);
    let (bx1, by1, bz1) = (region_max.x.floor() as i32, region_max.y.floor() as i32, region_max.z.floor() as i32);

    let ray = to - eye;
    let mut best: Option<(f64, BlockRayHit)> = None;

    for bx in bx0..=bx1 {
        for by in by0..=by1 {
            for bz in bz0..=bz1 {
                // block_at: inklusive eigener Platzier-Vorhersage, s. Kommentar ueber
                // raycast_block.
                let state = world.block_at(bx, by, bz);
                let (odx, odz) = world::block_name(state)
                    .map(|name| world::outline_xz_offset(name, bx, bz))
                    .unwrap_or((0.0, 0.0));
                for cbox in world::outline_boxes(state, shapes) {
                    let bmin = DVec3::new(
                        bx as f64 + odx as f64 + cbox.min[0] as f64,
                        by as f64 + cbox.min[1] as f64,
                        bz as f64 + odz as f64 + cbox.min[2] as f64,
                    );
                    let bmax = DVec3::new(
                        bx as f64 + odx as f64 + cbox.max[0] as f64,
                        by as f64 + cbox.max[1] as f64,
                        bz as f64 + odz as f64 + cbox.max[2] as f64,
                    );
                    let Some((t, achse)) = slab_entry(eye, ray, bmin, bmax) else {
                        continue;
                    };
                    if best.as_ref().is_some_and(|(bt, _)| *bt <= t) {
                        continue;
                    }
                    let punkt = eye + ray * t.max(0.0);
                    // Die Seite, durch die der Strahl eintritt: laeuft er auf der Achse nach
                    // oben, trifft er die untere Seite -- und umgekehrt.
                    let face = match (achse, ray[achse] > 0.0) {
                        (0, true) => protocol::packets::FACE_WEST,
                        (0, false) => protocol::packets::FACE_EAST,
                        (1, true) => protocol::packets::FACE_DOWN,
                        (1, false) => protocol::packets::FACE_UP,
                        (_, true) => protocol::packets::FACE_NORTH,
                        (_, false) => protocol::packets::FACE_SOUTH,
                    };
                    best = Some((
                        t,
                        BlockRayHit {
                            block: (bx, by, bz),
                            face,
                            cursor: ((punkt.x - bx as f64) as f32, (punkt.y - by as f64) as f32, (punkt.z - bz as f64) as f32),
                            inside: t < 0.0,
                        },
                    ));
                }
            }
        }
    }
    best.map(|(_, hit)| hit)
}

// Zuschauer-Flug: genau die eingestellte Geschwindigkeit in Richtung der gedrueckten Tasten,
// ab dem ersten Tick und ohne Nachlaufen. Waagerecht nach Blickrichtung gedreht wie Vanillas
// moveRelative, Springen hebt, Schleichen senkt; schraeg nicht schneller als geradeaus.
pub fn spectator_velocity(input: &Input) -> DVec3 {
    let (sin, cos) = (input.yaw_rad as f64).sin_cos();
    let (vor, seit) = (input.forward as f64, input.strafe as f64);
    let hoch = input.jump as i32 as f64 - input.sneak as i32 as f64;
    let v = DVec3::new(seit * cos - vor * sin, hoch, vor * cos + seit * sin);
    if v.length_squared() < 1.0e-9 {
        return DVec3::ZERO;
    }
    v.normalize() * input.spectator_speed
}

// Wie raycast_block, aber fuer weite Strecken (Zuschauer-Teleport, s. app::spectator): Zelle
// fuer Zelle entlang des Strahls statt aller Bloecke im ueberstrichenen Quader, und nur so weit,
// wie Spalten geladen sind. Steckt das Auge im Fels, zaehlt erst der Block hinter der ersten
// Luft -- sonst traefe man immer den Stein, in dem man steckt.
pub fn raycast_block_far(eye: DVec3, dir: DVec3, reach: f64, world: &World, shapes: &world::BlockShapes) -> Option<BlockRayHit> {
    let dir = dir.normalize_or_zero();
    if dir == DVec3::ZERO {
        return None;
    }
    let ray = dir * reach;
    let mut zelle = [eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32];
    let mut schritt = [0i32; 3];
    let mut naechste = [f64::INFINITY; 3];
    let mut abstand = [f64::INFINITY; 3];
    for a in 0..3 {
        if dir[a] > 0.0 {
            schritt[a] = 1;
            naechste[a] = ((zelle[a] + 1) as f64 - eye[a]) / dir[a];
            abstand[a] = 1.0 / dir[a];
        } else if dir[a] < 0.0 {
            schritt[a] = -1;
            naechste[a] = (zelle[a] as f64 - eye[a]) / dir[a];
            abstand[a] = -1.0 / dir[a];
        }
    }
    let unten = world.min_y();
    let lokal = eye - DVec3::new(zelle[0] as f64, zelle[1] as f64, zelle[2] as f64);
    let mut im_fels = world::outline_boxes(world.block_at(zelle[0], zelle[1], zelle[2]), shapes)
        .into_iter()
        .any(|b| (0..3).all(|a| b.min[a] as f64 <= lokal[a] && lokal[a] <= b.max[a] as f64));
    let mut t = 0.0;
    while t <= reach {
        let (bx, by, bz) = (zelle[0], zelle[1], zelle[2]);
        if world.column(bx.div_euclid(16), bz.div_euclid(16)).is_none() {
            return None;
        }
        // Unter der Welt kommt nichts mehr; nach oben kann der Strahl wieder herunterkommen.
        if by < unten && dir.y <= 0.0 {
            return None;
        }
        let state = world.block_at(bx, by, bz);
        if state == world::STATE_AIR {
            im_fels = false;
        } else if !im_fels {
            let (odx, odz) = world::block_name(state).map(|name| world::outline_xz_offset(name, bx, bz)).unwrap_or((0.0, 0.0));
            let mut best: Option<(f64, usize)> = None;
            for cbox in world::outline_boxes(state, shapes) {
                let bmin = DVec3::new(bx as f64 + odx as f64 + cbox.min[0] as f64, by as f64 + cbox.min[1] as f64, bz as f64 + odz as f64 + cbox.min[2] as f64);
                let bmax = DVec3::new(bx as f64 + odx as f64 + cbox.max[0] as f64, by as f64 + cbox.max[1] as f64, bz as f64 + odz as f64 + cbox.max[2] as f64);
                if let Some((tb, achse)) = slab_entry(eye, ray, bmin, bmax) {
                    if tb >= 0.0 && best.is_none_or(|(bt, _)| tb < bt) {
                        best = Some((tb, achse));
                    }
                }
            }
            if let Some((tb, achse)) = best {
                let punkt = eye + ray * tb;
                let face = match (achse, ray[achse] > 0.0) {
                    (0, true) => protocol::packets::FACE_WEST,
                    (0, false) => protocol::packets::FACE_EAST,
                    (1, true) => protocol::packets::FACE_DOWN,
                    (1, false) => protocol::packets::FACE_UP,
                    (_, true) => protocol::packets::FACE_NORTH,
                    (_, false) => protocol::packets::FACE_SOUTH,
                };
                return Some(BlockRayHit {
                    block: (bx, by, bz),
                    face,
                    cursor: ((punkt.x - bx as f64) as f32, (punkt.y - by as f64) as f32, (punkt.z - bz as f64) as f32),
                    inside: false,
                });
            }
        }
        // In die Nachbarzelle, deren Grenze der Strahl als naechste kreuzt.
        let a = if naechste[0] <= naechste[1] && naechste[0] <= naechste[2] {
            0
        } else if naechste[1] <= naechste[2] {
            1
        } else {
            2
        };
        t = naechste[a];
        naechste[a] += abstand[a];
        zelle[a] += schritt[a];
    }
    None
}

// Slab-Verfahren fuer EINE Box: liefert den Eintritts-Parameter auf der Strecke (kann
// negativ sein, wenn der Startpunkt schon drinsteckt) und die Achse, auf der der Strahl
// eintritt -- genau die bestimmt die getroffene Seite.
fn slab_entry(from: DVec3, ray: DVec3, bmin: DVec3, bmax: DVec3) -> Option<(f64, usize)> {
    let mut t_enter = f64::NEG_INFINITY;
    let mut t_exit = 1.0f64;
    let mut achse = 0usize;
    for a in 0..3 {
        let d = ray[a];
        if d.abs() < 1.0e-12 {
            // Parallel zur Achse: nur ein Treffer, wenn der Startpunkt schon im Schlitz liegt.
            if from[a] < bmin[a] || from[a] > bmax[a] {
                return None;
            }
            continue;
        }
        let (mut t0, mut t1) = ((bmin[a] - from[a]) / d, (bmax[a] - from[a]) / d);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        if t0 > t_enter {
            t_enter = t0;
            achse = a;
        }
        t_exit = t_exit.min(t1);
        if t_enter > t_exit {
            return None;
        }
    }
    (t_enter.is_finite() && t_exit >= 0.0).then_some((t_enter, achse))
}

// Naechster Schnittpunkt der Strecke from->to mit einer Block-Kollisionsbox, oder None.
// Slab-Verfahren gegen die Boxen im ueberstrichenen Bereich; die Strecke ist hier nur
// wenige Bloecke lang (Kameradistanz), ein voller Voxel-Marsch waere Aufwand ohne Gewinn.
fn raycast_blocks(from: DVec3, to: DVec3, world: &World, shapes: &world::BlockShapes) -> Option<DVec3> {
    let dir = to - from;
    let len = dir.length();
    if len < 1.0e-9 {
        return None;
    }
    let region_min = from.min(to);
    let region_max = from.max(to);
    let boxes = collect_camera_boxes(region_min, region_max, world, shapes);

    let mut nearest: Option<f64> = None;
    for b in &boxes {
        let mut t_enter = 0.0f64;
        let mut t_exit = 1.0f64;
        let mut hit = true;
        for axis in 0..3 {
            let d = dir[axis];
            let (lo, hi) = (b.min[axis], b.max[axis]);
            if d.abs() < 1.0e-12 {
                // Parallel zur Achse: nur ein Treffer, wenn der Startpunkt schon im Schlitz liegt.
                if from[axis] < lo || from[axis] > hi {
                    hit = false;
                    break;
                }
                continue;
            }
            let mut t0 = (lo - from[axis]) / d;
            let mut t1 = (hi - from[axis]) / d;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            t_enter = t_enter.max(t0);
            t_exit = t_exit.min(t1);
            if t_enter > t_exit {
                hit = false;
                break;
            }
        }
        if hit && t_enter >= 0.0 {
            nearest = Some(nearest.map_or(t_enter, |n: f64| n.min(t_enter)));
        }
    }
    nearest.map(|t| from + dir * t)
}

fn move_and_collide(
    pos: DVec3,
    vel: DVec3,
    height: f64,
    on_ground: bool,
    world: &World,
    shapes: &world::BlockShapes,
    ctx: world::ShapeContext,
    body: BodyAttributes,
) -> (DVec3, bool, bool, bool, bool) {
    let half = body.width() / 2.0;
    let box_min = DVec3::new(pos.x - half, pos.y, pos.z - half);
    let box_max = DVec3::new(pos.x + half, pos.y + height, pos.z + half);

    let moved = adjust_movement_with_step(box_min, box_max, vel, on_ground, world, shapes, ctx, body.step_height);

    let hit_x = (moved.x - vel.x).abs() > 1.0e-6;
    let hit_z = (moved.z - vel.z).abs() > 1.0e-6;
    // Ein geglueckter Schritt zaehlt bewusst als Vertikalkollision nach unten: die Bewegung
    // ging statt nach unten nach OBEN, was Vanilla ebenso als "movement.y != vec3d.y" sieht
    // und ueber Block.onEntityLand die Y-Geschwindigkeit nullt. Ohne das behielte man den
    // Fallimpuls und saenke im naechsten Tick wieder von der Stufe.
    let hit_y = (moved.y - vel.y).abs() > 1.0e-6;
    // Vanillas groundCollision: `verticalCollision && movement.y < 0.0` (Entity.move,
    // Zeile 764) -- STRIKT kleiner null. Bei movement.y == 0 (z.B. der erste Tick nach einem
    // Teleport, dessen Geschwindigkeit genullt wurde) zaehlt ein Treffer NICHT als
    // Bodenkontakt, auch wenn die Stufenlogik dabei ein Stueck nach oben geschoben hat.
    let hit_y_down = hit_y && vel.y < 0.0;
    let hit_y_up = hit_y && vel.y > 0.0;

    (pos + moved, hit_x, hit_y_down, hit_y_up, hit_z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::ChunkSection;

    // Baut eine Testwelt mit einer durchgehend soliden Flaeche bei y=floor_y (Zustand 1 =
    // "solide", s. world::is_solid) ueber mehrere Chunk-Spalten (5x5, also 80x80 Bloecke um
    // den Ursprung) -- eine einzelne 16x16-Spalte ist bei ~100 Ticks Sprint/Lauf-Strecke zu
    // klein, der Spieler laeuft sonst mitten im Test ueber die Kante ins Leere.
    fn world_with_floor(floor_y: i32) -> World {
        let mut w = World::new();
        let section_index = (floor_y - world::MIN_Y).div_euclid(16);
        let local_y = (floor_y - world::MIN_Y).rem_euclid(16);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == section_index {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((local_y * 16 + z) * 16 + x) as usize] = 1;
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Boden NUR fuer Welt-X < edge_x -> eine Abbruchkante bei x=edge_x (fuer den Sneak-Test).
    fn world_with_floor_up_to_x(floor_y: i32, edge_x: i32) -> World {
        let mut w = World::new();
        let section_index = (floor_y - world::MIN_Y).div_euclid(16);
        let local_y = (floor_y - world::MIN_Y).rem_euclid(16);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == section_index {
                        for x in 0..16 {
                            let world_x = cx * 16 + x;
                            if world_x < edge_x {
                                for z in 0..16 {
                                    states[((local_y * 16 + z) * 16 + x) as usize] = 1;
                                }
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // TridentItem.onStoppedUsing, Sog III (Staerke 3.0): waagerecht nach Sueden (Yaw 0,
    // Pitch 0) gibt es genau 3.0 in +Z; am Boden hebt Vanilla zusaetzlich um 1.1999999 an.
    #[test]
    fn sog_schub_und_anheben_wie_vanilla() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        p.on_ground = true;
        p.start_riptide(0.0, 0.0, 3.0, &world, &shapes);
        assert!((p.vel.z - 3.0).abs() < 1e-3, "Schub in Blickrichtung: {:?}", p.vel);
        assert!(p.vel.x.abs() < 1e-3 && p.vel.y.abs() < 1e-3);
        assert!((p.pos.y - (1.0 + RIPTIDE_GROUND_LIFT)).abs() < 1e-9, "am Boden angehoben: {}", p.pos.y);
        assert!(!p.on_ground);
        assert_eq!(p.riptide_ticks, RIPTIDE_TICKS);

        // Senkrecht nach oben (Vanilla-Pitch -90), in der Luft: kein Anheben, voller Schub hoch.
        let mut q = PlayerPhysics::new(DVec3::new(8.5, 10.0, 8.5));
        q.on_ground = false;
        q.start_riptide(0.0, -90.0, 1.5, &world, &shapes);
        assert!((q.vel.y - 1.5).abs() < 1e-3, "{:?}", q.vel);
        assert_eq!(q.pos.y, 10.0);
    }

    // tickRiptide: ein Lebewesen in der Bahn beendet den Sog mit Rueckprall, ein Nicht-
    // Lebewesen tut nichts (und verhindert sogar den Wand-Abbruch), sonst beendet eine Wand ihn.
    #[test]
    fn sog_endet_an_lebewesen_und_waenden() {
        let mut p = PlayerPhysics::new(DVec3::new(0.0, 5.0, 0.0));
        p.riptide_ticks = 20;
        p.vel = DVec3::new(0.0, 0.0, 2.0);
        let vorher = DVec3::new(0.0, 5.0, -2.0);
        let gegner = (DVec3::new(-0.3, 5.0, -1.3), DVec3::new(0.3, 6.8, -0.7), true);
        p.tick_riptide(vorher, &[gegner]);
        assert_eq!(p.riptide_ticks, 0);
        assert!((p.vel.z - -0.4).abs() < 1e-9, "Rückprall mit -0.2: {:?}", p.vel);

        let mut q = PlayerPhysics::new(DVec3::new(0.0, 5.0, 0.0));
        q.riptide_ticks = 20;
        q.horizontal_collision = true;
        let pfeil = (DVec3::new(-0.1, 5.0, -1.1), DVec3::new(0.1, 5.2, -0.9), false);
        q.tick_riptide(vorher, &[pfeil]);
        assert_eq!(q.riptide_ticks, 19, "nur Nicht-Lebewesen getroffen: weder Abbruch noch Wand-Abbruch");

        q.tick_riptide(vorher, &[]);
        assert_eq!(q.riptide_ticks, 0, "Wand beendet den Sog");
    }

    // Die Pose SPIN_ATTACK haengt am Server-Flag, ist 0.6 hoch und geht vor dem Schleichen.
    #[test]
    fn sog_pose_aus_dem_flag() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        p.using_riptide = true;
        let mut input = idle_input(0.0);
        input.sneak = true;
        p.tick(&input, &world, &shapes);
        assert!(p.spin_attack && !p.crouching);
        assert_eq!(p.current_height(), SWIMMING_HEIGHT_F64);
        assert_eq!(p.eye_height(), SWIMMING_EYE_HEIGHT);
        p.using_riptide = false;
        p.tick(&input, &world, &shapes);
        assert!(!p.spin_attack && p.crouching);
    }

    // Vanillas bekannte Endgeschwindigkeiten: 4.317 Bloecke/s gehend = 0.2159 pro Tick,
    // 5.612 sprintend = 0.2806. Beides muss die eigene Physik treffen -- daran haengt nicht
    // nur das Spielgefuehl, sondern auch die Schrittanimation der Modelle
    // (world::LimbAnimator rechnet min(Strecke * 4, 1)).
    #[test]
    fn strecke_pro_tick_trifft_vanilla() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let messe = |sprint: bool| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut input = idle_input(0.0);
            input.forward = 1.0;
            input.sprint_held = sprint;
            // Genug Ticks, um die Endgeschwindigkeit zu erreichen.
            for _ in 0..80 {
                p.tick(&input, &world, &shapes);
            }
            let before = p.pos;
            p.tick(&input, &world, &shapes);
            ((p.pos.x - before.x).powi(2) + (p.pos.z - before.z).powi(2)).sqrt()
        };
        let gehen = messe(false);
        let sprinten = messe(true);
        assert!((gehen - 0.2159).abs() < 0.002, "Gehen: {gehen:.4} Blöcke/Tick, erwartet 0.2159");
        assert!((sprinten - 0.2806).abs() < 0.002, "Sprinten: {sprinten:.4} Blöcke/Tick, erwartet 0.2806");
        // Und die Folge fuer die Animation: Sprinten deckelt die Amplitude, Gehen nicht.
        assert!((gehen as f32 * 4.0) < 1.0, "gehend darf die Amplitude NICHT ausreizen");
        assert!((sprinten as f32 * 4.0) >= 1.0, "sprintend muss sie ausreizen");
    }

    // --- Schnelligkeit und Langsamkeit ---------------------------------------------------

    #[test]
    fn der_faktor_trifft_die_vanilla_stufen() {
        // Stufe I ist Staerke 0. Vanilla rechnet grundwert * (staerke + 1).
        assert!((effect_speed_factor(Some(0), None) - 1.2).abs() < 1e-6, "Schnelligkeit I = +20 %");
        assert!((effect_speed_factor(Some(1), None) - 1.4).abs() < 1e-6, "Schnelligkeit II = +40 %");
        assert!((effect_speed_factor(None, Some(0)) - 0.85).abs() < 1e-6, "Langsamkeit I = -15 %");
        assert!((effect_speed_factor(None, Some(1)) - 0.7).abs() < 1e-6, "Langsamkeit II = -30 %");
        assert_eq!(effect_speed_factor(None, None), 1.0, "ohne Effekt darf sich nichts ändern");
    }

    #[test]
    // Beide Effekte sind ADD_MULTIPLIED_TOTAL, und die legt Vanilla NACHEINANDER multiplikativ
    // an (`wert *= 1 + modifikator`), nicht als Summe. Wer die Prozente addiert, kommt bei
    // 1.05 statt 1.02 heraus -- knapp daneben und im Spiel kaum zu bemerken, fuer GrimAC aber
    // ein Unterschied.
    fn zwei_effekte_multiplizieren_sich_sie_addieren_sich_nicht() {
        let beides = effect_speed_factor(Some(0), Some(0));
        assert!((beides - 1.2 * 0.85).abs() < 1e-6, "{beides} statt {}", 1.2 * 0.85);
        assert!((beides - 1.05).abs() > 0.02, "das wäre die addierte Rechnung");
    }

    #[test]
    // Ab Langsamkeit VII (Staerke 6) ist der Faktor NEGATIV: 1 - 0.15*7 = -0.05. Ungeklemmt
    // liefe man rueckwaerts, statt stillzustehen -- das ist der Grund fuer die Untergrenze 0
    // der ClampedEntityAttribute.
    fn langsamkeit_sieben_haelt_still_statt_rueckwaerts_zu_laufen() {
        assert!(effect_speed_factor(None, Some(6)) < 0.0, "der reine Faktor ist negativ");

        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.effect_speed_factor = effect_speed_factor(None, Some(6));
        // Erst absetzen lassen: frisch erzeugt gilt der Spieler als nicht am Boden, und in der
        // LUFT wirkt der Effekt bewusst nicht (s. die_luftkontrolle_bleibt_von_den_effekten_
        // unberuehrt). Vanilla verhaelt sich genauso -- auch mit Langsamkeit VII laesst sich
        // im Sprung noch steuern.
        for _ in 0..5 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.on_ground, "Testaufbau: der Spieler muss am Boden stehen");
        input.forward = 1.0;
        let start = p.pos;
        for _ in 0..40 {
            p.tick(&input, &world, &shapes);
        }
        let strecke = ((p.pos.x - start.x).powi(2) + (p.pos.z - start.z).powi(2)).sqrt();
        assert!(strecke < 1e-6, "mit Langsamkeit VII darf man sich nicht bewegen, lief aber {strecke}");
    }

    #[test]
    // Die Gegenprobe zu strecke_pro_tick_trifft_vanilla, mit denselben bekannten Zahlen:
    // 4.317 Bloecke/s gehend, mal dem Effektfaktor.
    fn strecke_pro_tick_folgt_dem_effektfaktor() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let messe = |sprint: bool, faktor: f64| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut input = idle_input(0.0);
            input.forward = 1.0;
            input.sprint_held = sprint;
            input.effect_speed_factor = faktor;
            for _ in 0..80 {
                p.tick(&input, &world, &shapes);
            }
            let before = p.pos;
            p.tick(&input, &world, &shapes);
            ((p.pos.x - before.x).powi(2) + (p.pos.z - before.z).powi(2)).sqrt()
        };
        let schnell_1 = messe(false, effect_speed_factor(Some(0), None));
        assert!((schnell_1 - 0.2159 * 1.2).abs() < 0.003, "Gehen mit Schnelligkeit I: {schnell_1:.4}, erwartet {:.4}", 0.2159 * 1.2);
        let langsam_1 = messe(false, effect_speed_factor(None, Some(0)));
        assert!((langsam_1 - 0.2159 * 0.85).abs() < 0.003, "Gehen mit Langsamkeit I: {langsam_1:.4}, erwartet {:.4}", 0.2159 * 0.85);
        // Sprint und Effekt greifen ineinander, ohne sich gegenseitig zu verschlucken.
        let sprint_schnell = messe(true, effect_speed_factor(Some(1), None));
        assert!((sprint_schnell - 0.2806 * 1.4).abs() < 0.005, "Sprinten mit Schnelligkeit II: {sprint_schnell:.4}");
    }

    #[test]
    // PlayerEntity.getOffGroundSpeed() gibt ein FESTES Paar zurueck (0.02 / 0.026 sprintend),
    // nicht das Bewegungstempo-Attribut. Schnelligkeit darf die Luftkontrolle deshalb NICHT
    // veraendern -- und das ist keine Kleinigkeit: die Luftbeschleunigung ist genau die
    // Groesse, an der GrimAC waehrend eines Sprungs rechnet.
    fn die_luftkontrolle_bleibt_von_den_effekten_unberuehrt() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let messe = |faktor: f64| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut input = idle_input(0.0);
            input.effect_speed_factor = faktor;
            // Erst springen (ohne Vorwaertsdrang, damit der Sprung nichts mitnimmt), dann in
            // der Luft Gas geben.
            input.jump = true;
            p.tick(&input, &world, &shapes);
            input.jump = false;
            input.forward = 1.0;
            let vorher = (p.vel.x, p.vel.z);
            p.tick(&input, &world, &shapes);
            ((p.vel.x - vorher.0).powi(2) + (p.vel.z - vorher.1).powi(2)).sqrt()
        };
        let ohne = messe(1.0);
        let mit = messe(effect_speed_factor(Some(1), None));
        assert!(ohne > 0.0, "Testaufbau: in der Luft muss überhaupt beschleunigt werden");
        assert!((ohne - mit).abs() < 1e-9, "Luftbeschleunigung ohne {ohne} vs. mit Schnelligkeit {mit}");
    }

    // GrimACs SprintE (grim.sprint.wall) flaggte im Kaktus-Labyrinth auf anticheat-test.com.
    // Ursache: minorHorizontalCollision fehlte. Hier die beiden Enden der Vanilla-Regel --
    // geradewegs in die Wand ist ein Anprall (Sprint bricht ab), schraeg daran entlang ist
    // ein Streifen (Sprint laeuft weiter, < 8 Grad Abweichung).
    #[test]
    fn streifender_wandkontakt_bricht_den_sprint_nicht_ab() {
        // Blick nach +Z (yaw_rad = 0), Vorwaertsdrang: gewollte Richtung ist +Z.
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        // Frontal: die Z-Achse ist weg, es bleibt nur ein Rest auf X -- 90 Grad daneben.
        assert!(!is_horizontal_collision_minor(&input, DVec3::new(0.02, 0.0, 0.0)));
        // Streifen: fast die volle gewollte Bewegung, nur leicht zur Seite abgelenkt
        // (atan(0.01/0.2) = 2.9 Grad < 8).
        assert!(is_horizontal_collision_minor(&input, DVec3::new(0.01, 0.0, 0.2)));
        // Knapp jenseits der Grenze: atan(0.2/1.0) = 11.3 Grad > 8.
        assert!(!is_horizontal_collision_minor(&input, DVec3::new(0.2, 0.0, 1.0)));
        // Steht man praktisch still, ist nichts "minor" -- Vanillas 1e-5-Ausstieg.
        assert!(!is_horizontal_collision_minor(&input, DVec3::new(0.0, 0.0, 0.001)));
        // Ohne Eingabe ebenfalls nicht (i < 1e-5).
        let leer = idle_input(0.0);
        assert!(!is_horizontal_collision_minor(&leer, DVec3::new(0.0, 0.0, 0.2)));
    }

    // Die Grenze selbst: exakt Vanillas 0.13962634 rad. Ein Winkel knapp darunter zaehlt als
    // Streifen, knapp darueber nicht -- damit ist auch das Vorzeichen der Drehung mitgeprueft.
    #[test]
    fn die_acht_grad_grenze_liegt_auf_vanillas_wert() {
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        let bei = |grad: f64| {
            let r = grad.to_radians();
            is_horizontal_collision_minor(&input, DVec3::new(r.sin(), 0.0, r.cos()))
        };
        let grenze_grad = 0.13962634f32 as f64 * 180.0 / std::f64::consts::PI;
        assert!(bei(grenze_grad - 0.01), "knapp innerhalb der Grenze muss streifend sein");
        assert!(!bei(grenze_grad + 0.01), "knapp außerhalb der Grenze darf es nicht sein");
        // Seitenverkehrt darf es keinen Unterschied machen.
        assert!(bei(-(grenze_grad - 0.01)));
        assert!(!bei(-(grenze_grad + 0.01)));
    }

    // Fall in einer mitgeschnittenen Welt: alle Chunk-Pakete aus einem Rohmitschnitt
    // (pktproxy, CORAL_PKTRAW; Format s. net::write_raw), min_y der Dimension, Startposition.
    // Druckt die Position je Tick -- zum Vergleich mit dem, was ein Client dort schickt.
    // CORAL_FALL_RAW=<datei> CORAL_FALL_MIN_Y=0 CORAL_FALL_START=0.5,100,0.5 \
    //   cargo test -p app --bins fall_auf_mitschnitt -- --ignored --nocapture
    #[test]
    #[ignore]
    fn fall_auf_mitschnitt() {
        let roh = std::fs::read(std::env::var("CORAL_FALL_RAW").expect("CORAL_FALL_RAW")).unwrap();
        let min_y: i32 = std::env::var("CORAL_FALL_MIN_Y").ok().and_then(|v| v.parse().ok()).unwrap_or(-64);
        let start: Vec<f64> = std::env::var("CORAL_FALL_START").unwrap_or("0.5,100,0.5".into()).split(',').map(|v| v.parse().unwrap()).collect();
        let mut w = World::new();
        w.set_min_y(min_y);
        let mut i = 0;
        while i + 17 <= roh.len() {
            let len = u32::from_be_bytes(roh[i + 13..i + 17].try_into().unwrap()) as usize;
            let payload = &roh[i + 17..i + 17 + len];
            if roh[i] == b'S' && payload.first() == Some(&(protocol::packets::CB_PLAY_CHUNK_DATA as u8)) {
                if let Ok(c) = protocol::chunk::parse_chunk_data(payload) {
                    let sections = c.sections.iter().map(|s| ChunkSection::from_states_and_biomes(s.block_states.clone(), s.biomes.clone())).collect();
                    w.insert_column(c.x, c.z, sections);
                }
            }
            i += 17 + len;
        }
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(start[0], start[1], start[2]));
        let mut input = idle_input(0.0);
        // CORAL_FALL_FLOAT=1: wie mit Modul Botcheck-Pass (s. Input::float_drag).
        input.float_drag = std::env::var_os("CORAL_FALL_FLOAT").is_some();
        for t in 0..30 {
            p.tick(&input, &w, &shapes);
            println!("{t:2} pos=({:?}, {:?}, {:?}) boden={}", p.pos.x, p.pos.y, p.pos.z, p.on_ground);
        }
    }

    fn idle_input(yaw_rad: f32) -> Input {
        Input {
            forward: 0.0,
            strafe: 0.0,
            jump: false,
            sneak: false,
            sprint_held: false,
            using_item: false,
            item_use_speed: ITEM_USE_SPEED_MULT,
            item_use_can_sprint: false,
            food_level: 20,
            blindness: false,
            effect_speed_factor: 1.0,
            movement_speed: BASE_SPEED,
            sneaking_speed: DEFAULT_SNEAKING_SPEED,
            water_movement_efficiency: 0.0,
            slow_falling: false,
            levitation: None,
            jump_boost: None,
            dolphins_grace: false,
            float_drag: false,
            yaw_rad,
            look_dir_y: 0.0,
            yaw_deg: yaw_rad.to_degrees(),
            pitch_deg: 0.0,
            glider_equipped: false,
            spectator: false,
            spectator_speed: 0.0,
        }
    }

    fn cobweb_state() -> u32 {
        (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("cobweb")).expect("cobweb unbekannt")
    }

    // Wie all_full_cubes(), aber mit LEERER Form fuers Spinnennetz -- sonst waere es in den
    // Tests eine Wand, auf der man steht, statt eines Blocks, durch den man sich bewegt.
    //
    // Im echten Spiel kommt das von allein: die Formtabelle wird aus den MODELLEN gebaut, und
    // das Netz ist ein Kreuz-Block (zwei diagonale Ebenen), dessen Boxliste damit leer ist --
    // s. cross_raw in main.rs. Nur der Test-Stub all_full_cubes() macht aus jedem Block einen
    // Vollwuerfel und muesste hier sonst als Boden herhalten.
    // Vanillas Herausschieben aus einem Block (moveTowardsClosestSpace). Getestet wird die
    // Regel selbst, nicht der ganze Tick: steckt man drin, setzt sie 0.1 Richtung der
    // naechsten freien Spalte -- und wer frei steht oder in Glas steht, wird nicht geschoben
    // (Glas erstickt in Vanilla ausdruecklich nicht).
    #[test]
    fn stecken_im_block_schiebt_mit_null_komma_eins_heraus() {
        let bloeck = |name: &str| {
            (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap_or_else(|| panic!("{name} unbekannt"))
        };
        let shapes = world::BlockShapes::all_full_cubes();

        // Frei stehen: keine Verschiebung.
        let welt = world_with_floor(0);
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        p.move_towards_closest_space(8.5, 8.5, &welt, &shapes);
        assert_eq!(p.vel, DVec3::ZERO, "ohne Block darf nichts geschoben werden");

        // In Obsidian: die naechste Zellgrenze gewinnt. Bei x=8.8 ist das Osten (0.2 statt
        // 0.8 nach Westen), also +0.1 auf X.
        let mut welt = world_with_floor(0);
        welt.set_block(8, 1, 8, bloeck("obsidian"));
        welt.set_block(8, 2, 8, bloeck("obsidian"));
        let mut p = PlayerPhysics::new(DVec3::new(8.8, 1.0, 8.5));
        p.move_towards_closest_space(8.8, 8.5, &welt, &shapes);
        assert!((p.vel.x - 0.1).abs() < 1e-12 && p.vel.z == 0.0, "nach Osten heraus, war {:?}", p.vel);

        // Dieselbe Lage, aber Fuss in Richtung Norden am naechsten: dann Z.
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.1));
        p.move_towards_closest_space(8.5, 8.1, &welt, &shapes);
        assert!((p.vel.z + 0.1).abs() < 1e-12 && p.vel.x == 0.0, "nach Norden heraus, war {:?}", p.vel);

        // Glas erstickt nicht -- kein Herausschieben.
        let mut welt = world_with_floor(0);
        welt.set_block(8, 1, 8, bloeck("glass"));
        welt.set_block(8, 2, 8, bloeck("glass"));
        let mut p = PlayerPhysics::new(DVec3::new(8.8, 1.0, 8.5));
        p.move_towards_closest_space(8.8, 8.5, &welt, &shapes);
        assert_eq!(p.vel, DVec3::ZERO, "in Glas wird nicht geschoben");
    }

    // Trampelpfad erstickt, ist aber nur 15/16 hoch. Wer auf einem Pfad steht und mit der
    // Ecke ueber einen gleich hohen Nachbarpfad ragt, steckt NICHT darin -- Vanilla prueft die
    // Kollisionsform, nicht die Zelle. Ein voller Block daneben (bis 1.0) schiebt dagegen.
    #[test]
    fn pfad_neben_pfad_schiebt_nicht_aber_gras_daneben_schon() {
        let bloeck = |name: &str| {
            (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap_or_else(|| panic!("{name} unbekannt"))
        };
        let shapes = world::BlockShapes::all_full_cubes();
        let mut welt = world_with_floor(-5);
        welt.set_block(8, 0, 8, bloeck("dirt_path"));
        welt.set_block(9, 0, 8, bloeck("dirt_path"));
        // Auf dem Pfad (Oberkante 15/16), nah an der Ostkante: die Ecke ragt nach x=9.
        let mut p = PlayerPhysics::new(DVec3::new(8.85, 0.9375, 8.5));
        p.move_towards_closest_space(9.06, 8.5, &welt, &shapes);
        assert_eq!(p.vel, DVec3::ZERO, "gleich hoher Pfad daneben liegt unter den Fuessen");

        welt.set_block(9, 0, 8, bloeck("grass_block"));
        let mut p = PlayerPhysics::new(DVec3::new(8.85, 0.9375, 8.5));
        p.move_towards_closest_space(9.06, 8.5, &welt, &shapes);
        assert!((p.vel.x + 0.1).abs() < 1e-12, "Gras ragt 1/16 in die Box, zurueck nach Westen: {:?}", p.vel);
    }

    // Blasensaeule aus Seelensand, drei Zellen hoch, Luft darueber. Der Spieler (1.8 hoch)
    // steht in den unteren beiden: zwei Innenzellen, keine Oberkante -- also zweimal +0.06.
    // Mit dem Kopf in der obersten Zelle kommt deren Oberkanten-Schub (+0.1) dazu.
    #[test]
    fn blasensaeule_schiebt_je_beruehrter_zelle() {
        let hoch = (0..world::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("bubble_column") && world::state_props::property(s, "drag") == Some("false"))
            .unwrap();
        let welt = world_with_floor_and_blocks(0, &[((8, 1, 8), hoch), ((8, 2, 8), hoch), ((8, 3, 8), hoch)]);
        let shapes = world::BlockShapes::new(vec![None; world::state_count()]);
        let input = idle_input(0.0);

        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        p.fall_distance = 3.0;
        p.tick_block_collision(&input, &welt, &shapes);
        assert!((p.vel.y - 0.12).abs() < 1e-12, "zwei Innenzellen: {}", p.vel.y);
        assert_eq!(p.fall_distance, 0.0);

        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.5, 8.5));
        p.tick_block_collision(&input, &welt, &shapes);
        assert!((p.vel.y - 0.22).abs() < 1e-12, "zwei Innenzellen + Oberkante: {}", p.vel.y);

        // Fliegend wirkt nichts.
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        p.abilities.flying = true;
        p.tick_block_collision(&input, &welt, &shapes);
        assert_eq!(p.vel.y, 0.0);
    }

    fn shapes_ohne_netz_kollision() -> world::BlockShapes {
        let mut per_state: Vec<Option<Vec<world::ShapeBox>>> = vec![None; world::state_count()];
        per_state[cobweb_state() as usize] = Some(Vec::new());
        world::BlockShapes::new(per_state)
    }

    // Welt mit Boden bei floor_y und einer Saeule aus Spinnennetz darueber, an der Stelle,
    // an der die Tests den Spieler hinsetzen.
    fn world_with_cobweb_column(floor_y: i32, at: (i32, i32), hoehe: i32) -> World {
        let mut w = world_with_floor(floor_y);
        for dy in 1..=hoehe {
            w.set_block(at.0, floor_y + dy, at.1, cobweb_state());
        }
        w
    }

    // Der Faktor greift erst im FOLGENDEN Tick (Vanilla setzt ihn in tickBlockCollision nach
    // travel und verbraucht ihn im move des naechsten Ticks). Ein Tick zu frueh angewandt
    // waere ein Simulationsunterschied bei jedem Netz-Eintritt.
    //
    // Waagerecht geprueft, nicht im Fall: die Schwerkraft wirkt in travelInAir() erst NACH
    // der Bewegung, ein aus dem Stand fallender Spieler legt im ersten Tick also ohnehin
    // nichts zurueck -- daran waere der Ein-Tick-Versatz nicht zu erkennen.
    #[test]
    fn spinnennetz_bremst_erst_den_naechsten_tick() {
        let shapes = shapes_ohne_netz_kollision();
        // Boden bei y=0, Netz in den beiden Zellen darueber -- der Spieler steht IM Netz.
        let mit_netz = world_with_cobweb_column(0, (8, 8), 2);
        let ohne_netz = world_with_floor(0);

        // Gegen denselben Lauf ohne Netz gemessen: der Faktor ist damit exakt die einzige
        // Differenz, ohne dass der Test Vanillas Beschleunigungsformel nachrechnen muesste.
        let schritte = |welt: &World| {
            let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
            p.on_ground = true;
            let mut input = idle_input(0.0);
            input.forward = 1.0;
            let mut out = Vec::new();
            for _ in 0..2 {
                let vorher = p.pos;
                p.tick(&input, welt, &shapes);
                out.push((p.pos - vorher).with_y(0.0).length());
            }
            out
        };
        let netz = schritte(&mit_netz);
        let frei = schritte(&ohne_netz);

        // Tick 1: das Netz wurde in diesem Tick noch nicht beruehrt -- identische Strecke.
        assert!((netz[0] - frei[0]).abs() < 1.0e-12, "erster Tick unverändert: {} vs {}", netz[0], frei[0]);
        assert!(frei[0] > 0.09, "Vorbedingung: es wurde überhaupt gelaufen ({})", frei[0]);
        // Tick 2 verbraucht den Faktor: exakt ein Viertel der freien Strecke.
        assert!((netz[1] - frei[1] * 0.25).abs() < 1.0e-12, "zweiter Tick ist ein Viertel: {} vs {}", netz[1], frei[1]);
    }

    // Kein Aufsummieren: weil move() die Geschwindigkeit nullt, faengt jeder Tick im Netz bei
    // 0 an und faellt immer wieder nur die eine gestauchte Schwerkraft-Strecke. Genau das
    // unterscheidet ein Netz von "etwas langsamer fallen".
    #[test]
    fn im_spinnennetz_beschleunigt_der_fall_nicht() {
        let world = world_with_cobweb_column(0, (8, 8), 8);
        let shapes = shapes_ohne_netz_kollision();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 6.0, 8.5));
        let input = idle_input(0.0);
        // Zwei Ticks Vorlauf, damit der Faktor sicher anliegt.
        p.tick(&input, &world, &shapes);
        p.tick(&input, &world, &shapes);

        let mut strecken = Vec::new();
        for _ in 0..4 {
            let vor = p.pos.y;
            p.tick(&input, &world, &shapes);
            strecken.push(vor - p.pos.y);
        }
        let erste = strecken[0];
        for s in &strecken {
            assert!((s - erste).abs() < 1.0e-9, "jeder Netz-Tick fällt gleich weit: {strecken:?}");
        }
        // Gegen Vanilla nachgerechnet: die Geschwindigkeit startet dank des Nullens bei 0,
        // travelInAir macht daraus -GRAVITY * VERTICAL_DRAG, davon bleiben 5%.
        let erwartet = GRAVITY * VERTICAL_DRAG * 0.05f32 as f64;
        assert!((erste - erwartet).abs() < 1.0e-9, "Fallstrecke im Netz: {erste}, erwartet {erwartet}");
    }

    // slowMovement() ruft onLanding() -- deshalb schuetzt ein Netz vor Fallschaden, selbst
    // wenn man nur hindurchfaellt.
    #[test]
    fn spinnennetz_setzt_die_fallhoehe_zurueck() {
        let world = world_with_cobweb_column(0, (8, 8), 1);
        let shapes = shapes_ohne_netz_kollision();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 20.0, 8.5));
        let input = idle_input(0.0);
        // Frei fallen, bis Fallhoehe da ist, aber noch weit ueber dem Netz.
        for _ in 0..10 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.fall_distance > 1.0, "Vorbedingung: es wurde gefallen ({})", p.fall_distance);

        // In die Netz-Zelle setzen und einen Tick laufen lassen.
        p.pos = DVec3::new(8.5, 1.0, 8.5);
        p.tick(&input, &world, &shapes);
        assert_eq!(p.fall_distance, 0.0, "das Netz nullt die Fallhöhe");
    }

    // Buendiges Anliegen an der Zellgrenze ist KEINE Beruehrung -- Vanilla schrumpft die Box
    // dafuer um 1.0E-5. Ohne das loeste ein Netz eine Zelle zu frueh aus.
    #[test]
    fn netz_an_der_zellgrenze_bremst_nicht() {
        let shapes = shapes_ohne_netz_kollision();
        // Die Box endet exakt bei x=9.0, das Netz fuellt die Zelle 9.0..10.0.
        let start = DVec3::new(9.0 - WIDTH_F64 / 2.0, 1.0, 8.5);
        let input = idle_input(0.0);

        let mut mit_netz = world_with_floor(0);
        mit_netz.set_block(9, 1, 8, cobweb_state());
        let ohne_netz = world_with_floor(0);

        // Gegen den netzfreien Lauf gepruefen statt gegen eine Zahl: was hier zaehlt, ist
        // allein, dass das Netz nebenan KEINEN Unterschied macht.
        let laufe = |welt: &World| {
            let mut p = PlayerPhysics::new(start);
            let mut i = input;
            i.forward = 1.0;
            for _ in 0..5 {
                p.tick(&i, welt, &shapes);
            }
            p.pos
        };
        let a = laufe(&mit_netz);
        let b = laufe(&ohne_netz);
        assert!((a - b).length() < 1.0e-12, "Netz an der Zellgrenze darf nichts ändern: {a} vs {b}");
    }

    // mc_sin muss Vanillas MathHelper.sin (Float-Tabelle) treffen, NICHT das exakte f64-sin --
    // sonst driftet die Schwimm-Physik ueber viele Ticks von Grim weg. Referenzwert 0.783306598
    // stammt aus der per Hand nachgebauten Vanilla-Tabelle (pitch=-51.57 Grad).
    #[test]
    fn mc_sin_trifft_vanilla_tabelle_nicht_exaktes_sin() {
        // pitch so, dass Blick-Y ~0.7833 (steil nach oben). look_dir_y_from_pitch nutzt +pitch
        // (unsere Konvention) -> mc_sin(pitch_rad).
        let pitch_deg = 51.5668_f32;
        let d = look_dir_y_from_pitch(pitch_deg) as f64;
        let exact = (pitch_deg as f64).to_radians().sin();
        // Muss die Tabellen-Naeherung sein, messbar (~2.6e-5) vom exakten sin entfernt.
        assert!((d - 0.7833066).abs() < 1e-5, "mc_sin-Wert {d} weicht von der Vanilla-Tabelle ab");
        assert!((d - exact).abs() > 1e-5, "mc_sin darf NICHT das exakte f64-sin sein (war {d} vs exakt {exact})");
    }

    #[test]
    fn mc_cos_ist_tabellen_cos_und_bei_null_genau_eins() {
        // MathHelper.cos(0) muss exakt 1.0 sein (Tabellen-Index 16384 = pi/2 -> sin(pi/2)=1).
        assert_eq!(mc_cos(0.0), 1.0);
        assert_eq!(mc_sin(0.0), 0.0);
        // Bei einem schraegen Yaw die Tabellen-Naeherung, messbar vom exakten cos entfernt.
        let yaw_rad = 40.0_f32.to_radians() as f64;
        let table = mc_cos(yaw_rad) as f64;
        let exact = yaw_rad.cos();
        assert!((table - exact).abs() > 1e-6, "mc_cos muss die Vanilla-Tabelle sein (war {table} vs exakt {exact})");
    }

    fn kreativ(p: &mut PlayerPhysics) {
        p.abilities = Abilities { may_fly: true, instabuild: true, invulnerable: true, ..Abilities::default() };
    }

    // Auf dem Boden stehend (Boden bei y 0, Fuesse auf 1), Kreativ.
    fn kreativ_am_boden(world: &World, shapes: &world::BlockShapes) -> PlayerPhysics {
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        kreativ(&mut p);
        for _ in 0..3 {
            p.tick(&idle_input(0.0), world, shapes);
        }
        assert!(p.on_ground);
        p
    }

    #[test]
    fn doppelsprung_startet_den_flug_und_springt_ab() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = kreativ_am_boden(&world, &shapes);
        let sprung = Input { jump: true, ..idle_input(0.0) };
        // Erster Druck: ein normaler Sprung, das Fenster beginnt.
        p.tick(&sprung, &world, &shapes);
        assert!(!p.abilities.flying);
        p.tick(&idle_input(0.0), &world, &shapes);
        // Zweiter Druck im Fenster: Flug an.
        p.tick(&sprung, &world, &shapes);
        assert!(p.abilities.flying);
        assert!(p.abilities_changed);
        // Ohne Taste haelt der Flug die Hoehe (0.6 je Tick statt Schwerkraft).
        p.abilities_changed = false;
        for _ in 0..40 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(p.abilities.flying && !p.abilities_changed);
        assert!(p.vel.y.abs() < 1e-6, "{}", p.vel.y);
    }

    #[test]
    fn zwei_druecke_ausserhalb_des_fensters_schalten_nicht() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = kreativ_am_boden(&world, &shapes);
        let sprung = Input { jump: true, ..idle_input(0.0) };
        p.tick(&sprung, &world, &shapes);
        for _ in 0..FLY_TOGGLE_WINDOW {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        p.tick(&sprung, &world, &shapes);
        assert!(!p.abilities.flying);
        // Ohne Erlaubnis nie.
        let mut s = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        for i in 0..6 {
            s.tick(&Input { jump: i % 2 == 0, ..idle_input(0.0) }, &world, &shapes);
        }
        assert!(!s.abilities.flying);
    }

    #[test]
    fn steigen_und_sinken_mit_dreifacher_fluggeschwindigkeit() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 20.0, 8.5));
        kreativ(&mut p);
        p.abilities.flying = true;
        let hoch = Input { jump: true, ..idle_input(0.0) };
        // v <- (v + 0.15) * 0.6 strebt gegen 0.225.
        for _ in 0..60 {
            p.tick(&hoch, &world, &shapes);
        }
        assert!((p.vel.y - 0.225).abs() < 1e-6, "{}", p.vel.y);
        let runter = Input { sneak: true, ..idle_input(0.0) };
        for _ in 0..60 {
            p.tick(&runter, &world, &shapes);
        }
        assert!((p.vel.y + 0.225).abs() < 1e-6, "{}", p.vel.y);
        assert!(!p.crouching, "fliegend schleicht man nicht");
    }

    #[test]
    fn landen_beendet_den_flug() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 3.0, 8.5));
        kreativ(&mut p);
        p.abilities.flying = true;
        let runter = Input { sneak: true, ..idle_input(0.0) };
        for _ in 0..40 {
            p.tick(&runter, &world, &shapes);
            if !p.abilities.flying {
                break;
            }
        }
        assert!(!p.abilities.flying && p.on_ground && p.abilities_changed);
    }

    #[test]
    fn horizontal_fliegend_schneller_und_sprintend_doppelt() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let tempo = |sprint: bool| {
            let mut p = PlayerPhysics::new(DVec3::new(0.5, 20.0, 0.5));
            kreativ(&mut p);
            p.abilities.flying = true;
            let vor = Input { forward: 1.0, sprint_held: sprint, ..idle_input(0.0) };
            for _ in 0..80 {
                p.tick(&vor, &world, &shapes);
            }
            p.vel.z
        };
        // Gleichgewicht: a * 0.98 * 0.91 / (1 - 0.91) mit a = 0.05 bzw. 0.1.
        let erwartet = 0.05 * 0.98 * 0.91 / 0.09;
        assert!((tempo(false) - erwartet).abs() < 1e-3, "{}", tempo(false));
        assert!((tempo(true) - 2.0 * erwartet).abs() < 2e-3, "{}", tempo(true));
    }

    #[test]
    fn im_flug_startet_keine_elytra() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 20.0, 8.5));
        kreativ(&mut p);
        p.abilities.flying = true;
        p.tick(&gleit_input(false, true), &world, &shapes);
        p.tick(&gleit_input(true, true), &world, &shapes);
        assert!(!p.gliding && !p.glide_start_requested);
    }

    #[test]
    fn zuschauer_fliegt_durch_den_boden() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 3.0, 8.5));
        p.abilities = Abilities { may_fly: true, invulnerable: true, ..Abilities::default() };
        let runter = Input { sneak: true, spectator: true, ..idle_input(0.0) };
        for _ in 0..40 {
            p.tick(&runter, &world, &shapes);
        }
        assert!(p.abilities.flying, "Zuschauer fliegen immer");
        assert!(p.pos.y < -3.0, "{}", p.pos.y);
        assert!(!p.on_ground);
    }

    fn gleit_input(jump: bool, elytra: bool) -> Input {
        Input { jump, glider_equipped: elytra, ..idle_input(0.0) }
    }

    // Fallen lassen, bis man sicher in der Luft ist (kein Bodenkontakt, Taste los).
    fn in_der_luft(world: &World, shapes: &world::BlockShapes) -> PlayerPhysics {
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 40.0, 8.5));
        for _ in 0..3 {
            p.tick(&gleit_input(false, true), world, shapes);
        }
        assert!(!p.on_ground);
        p
    }

    #[test]
    fn elytra_startet_nur_beim_druecken_in_der_luft_und_mit_elytra() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut ohne = in_der_luft(&world, &shapes);
        ohne.tick(&gleit_input(true, false), &world, &shapes);
        assert!(!ohne.gliding, "ohne Elytra kein Gleiten");

        let mut p = in_der_luft(&world, &shapes);
        p.tick(&gleit_input(true, true), &world, &shapes);
        assert!(p.gliding && p.glide_start_requested, "Drücken in der Luft startet den Flug");
        // Die Pose zieht am Tick-Ende nach: 0.6 hoch, Augen auf 0.4.
        assert!((p.current_height() - SWIMMING_HEIGHT_F64).abs() < 1e-9);

        // Gehaltene Taste startet nicht erneut (nach einer Server-Absage z.B.).
        p.gliding = false;
        p.glide_start_requested = false;
        p.tick(&gleit_input(true, true), &world, &shapes);
        assert!(!p.gliding, "Halten ist kein neuer Druck");

        // Am Boden nie.
        let mut boden = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        boden.refresh_on_ground(&world, &shapes);
        boden.tick(&gleit_input(true, true), &world, &shapes);
        assert!(!boden.gliding);
    }

    // Ein Tick Gleitformel, von Hand nach LivingEntity.calcGlidingVelocity gerechnet: Blick
    // waagerecht nach +Z, Start mit 0.5 vorwaerts und 0.5 abwaerts.
    #[test]
    // Der Server beendet den Flug, die Pose steht aber noch einen Tick auf GLIDING: Vanilla
    // zaehlt das als Kriechen und nimmt die Eingabe mal 0.3 (Grim-Flag beim Elytra-Hoppen).
    fn erster_tick_nach_dem_gleiten_kriecht() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = in_der_luft(&world, &shapes);
        p.tick(&gleit_input(true, true), &world, &shapes);
        p.tick(&gleit_input(false, true), &world, &shapes);
        assert!(p.gliding && p.glide_pose);
        // Metadaten vom Server: Flug aus.
        p.gliding = false;
        let mut zwilling = PlayerPhysics::new(p.pos);
        zwilling.vel = p.vel;
        zwilling.fall_distance = p.fall_distance;
        let vor = Input { forward: 1.0, glider_equipped: true, ..idle_input(0.0) };
        let (v0, z0) = (p.vel, zwilling.vel);
        p.tick(&vor, &world, &shapes);
        zwilling.tick(&vor, &world, &shapes);
        let schub = |a: DVec3, b: DVec3| ((b.x - a.x * 0.91).powi(2) + (b.z - a.z * 0.91).powi(2)).sqrt();
        let verhaeltnis = schub(v0, p.vel) / schub(z0, zwilling.vel);
        assert!((verhaeltnis - 0.3).abs() < 1e-6, "Faktor war {verhaeltnis}");
        assert!(!p.glide_pose, "danach steht die Pose wieder");
    }

    // Serverwechsel mitten im Flug: der neue Spieler gleitet nicht und hat keinen Schwung, auch
    // nicht nach dem Teleport, der Pose und Haltung uebernimmt.
    #[test]
    fn neuer_spieler_fliegt_nicht_weiter() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = in_der_luft(&world, &shapes);
        p.tick(&gleit_input(true, true), &world, &shapes);
        p.tick(&gleit_input(false, true), &world, &shapes);
        assert!(p.gliding && p.glide_pose);
        p.reset_for_new_player();
        let uebernommen = p.carry_over_state();
        let mut neu = PlayerPhysics::new(p.pos);
        neu.restore_carry_over_state(uebernommen);
        assert!(!neu.gliding && !neu.glide_pose && !neu.glide_start_requested);
        assert_eq!(p.vel, DVec3::ZERO);
    }

    #[test]
    fn gleitformel_trifft_die_handrechnung() {
        let rot = rotation_vector(0.0, 0.0);
        assert_eq!(rot, DVec3::new(0.0, 0.0, 1.0));
        let v = calc_gliding_velocity(DVec3::new(0.0, -0.5, 0.5), rot, 0.0, GRAVITY);
        // y: -0.5 + 0.08*(-1+0.75) = -0.52; Umsetzung ins Vorwaerts: +0.052 -> -0.468.
        // z: 0.5 + 0.052 = 0.552, dann 10 % zurueck auf die alte Waagerechte 0.5 -> 0.5468.
        let erwartet = DVec3::new(0.0, -0.468 * 0.98f32 as f64, 0.5468 * 0.99f32 as f64);
        assert!((v - erwartet).length() < 1e-12, "{v:?} != {erwartet:?}");
    }

    #[test]
    fn gleiten_traegt_weiter_als_fallen() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut frei = in_der_luft(&world, &shapes);
        let mut gleiter = in_der_luft(&world, &shapes);
        gleiter.tick(&gleit_input(true, true), &world, &shapes);
        frei.tick(&gleit_input(true, false), &world, &shapes);
        for _ in 0..30 {
            gleiter.tick(&gleit_input(false, true), &world, &shapes);
            frei.tick(&gleit_input(false, false), &world, &shapes);
        }
        assert!(gleiter.gliding);
        assert!(gleiter.pos.y > frei.pos.y + 5.0, "gleitend deutlich hoeher: {} vs {}", gleiter.pos.y, frei.pos.y);
        assert!(gleiter.pos.z > 8.5 + 3.0, "gleitend nach vorn getragen: z={}", gleiter.pos.z);
    }

    #[test]
    fn rakete_zieht_auf_die_blickrichtung() {
        let rot = rotation_vector(0.0, 0.0);
        // Aus dem Stand: 0.1 Schub + halber Weg zu 1.5.
        let v = firework_boost(DVec3::ZERO, rot);
        assert!((v - DVec3::new(0.0, 0.0, 0.85)).length() < 1e-12);
        // Dauerschub ohne Luftwiderstand pendelt sich bei 1.7 ein.
        let mut v = DVec3::ZERO;
        for _ in 0..60 {
            v = firework_boost(v, rot);
        }
        assert!((v.z - 1.7).abs() < 1e-9);
    }

    #[test]
    fn gleitend_haelt_keine_ranke_fest() {
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 5.0, 8.5));
        let mut world = world_with_floor(0);
        let vine = state_of_block("vine");
        world.set_block(8, 5, 8, vine);
        assert!(p.is_climbing(&world));
        p.gliding = true;
        assert!(!p.is_climbing(&world));
    }

    #[test]
    fn faellt_und_landet_auf_dem_boden() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
        for _ in 0..200 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(p.on_ground, "sollte nach 10s Fallzeit auf dem Boden stehen");
        assert!((p.pos.y - 1.0).abs() < 1.0e-3, "Fuesse sollten exakt auf der Blockoberkante stehen, y={}", p.pos.y);
        // Kein exaktes 0.0: Vanilla haelt im Stand einen kleinen negativen "Rest" (Gravitation
        // minus Daempfung EINES Ticks), der beim naechsten Tick sofort wieder an der Kollision
        // abgeschnitten wird -- -0.0784 ist also der korrekte, vanilla-getreue Ruhewert.
        assert!((-0.1..=0.0).contains(&p.vel.y), "vel.y im Stand unplausibel: {}", p.vel.y);
    }

    #[test]
    fn sprung_erreicht_plausible_vanilla_hoehe() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        // Erst ankommen lassen (on_ground = true).
        for _ in 0..5 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(p.on_ground);

        let mut input = idle_input(0.0);
        input.jump = true;
        let mut max_height = p.pos.y;
        for i in 0..40 {
            if i == 1 {
                input.jump = false; // nur einmal springen, nicht bei jedem Bodenkontakt erneut
            }
            p.tick(&input, &world, &shapes);
            max_height = max_height.max(p.pos.y);
        }
        // Bekannte Vanilla-Sprunghoehe: ca. 1.25 Bloecke.
        assert!((1.0..1.4).contains(&(max_height - 1.0)), "Sprunghöhe unplausibel: {}", max_height - 1.0);
        assert!(p.on_ground, "sollte nach dem Sprung wieder landen");
    }

    #[test]
    fn laufen_naehert_sich_der_bekannten_vanilla_gehgeschwindigkeit() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        for _ in 0..100 {
            p.tick(&input, &world, &shapes); // bis zum Geschwindigkeits-Gleichgewicht laufen lassen
        }
        // Tatsaechliche Positionsaenderung messen, nicht self.vel: die gespeicherte
        // Geschwindigkeit ist bereits das Tick-Ende-Ergebnis NACH der Daempfung, waehrend
        // die Bewegung selbst mit der Geschwindigkeit VOR der Daempfung erfolgt (s.
        // tick()) -- self.vel unterschaetzt die reale Schrittweite um genau diesen Faktor.
        let before = p.pos;
        p.tick(&input, &world, &shapes);
        let speed_per_second = (p.pos - before).length() / TICK_DT as f64;
        // Bekannte Vanilla-Gehgeschwindigkeit: ca. 4.3 Bloecke/s.
        assert!((4.0..4.6).contains(&speed_per_second), "Gehgeschwindigkeit unplausibel: {speed_per_second}");
    }

    // Vanillas Stopp-Bedingungen fragen die SPRINT-TASTE nicht ab
    // (shouldStopSprinting/shouldStopSwimSprinting). Ein laufender Sprint ueberlebt das
    // Loslassen -- vorher endete er bei uns sofort.
    #[test]
    fn losgelassene_sprinttaste_beendet_den_sprint_nicht() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting, "mit gedrückter Taste muss der Sprint starten");

        input.sprint_held = false;
        for _ in 0..20 {
            p.tick(&input, &world, &shapes);
            assert!(p.sprinting, "ohne Taste, aber mit Vorwärtseingabe läuft der Sprint weiter");
        }
        // Ohne Vorwaertseingabe endet er -- das ist Vanillas `!hasForwardMovement`.
        input.forward = 0.0;
        p.tick(&input, &world, &shapes);
        assert!(!p.sprinting, "ohne Vorwärtseingabe endet der Sprint");
    }

    // canSprintOrFly() -> HungerManager.canSprint() -> foodLevel > 6. Bei 6 ist Schluss,
    // und zwar sowohl fuers Starten als auch fuer den laufenden Sprint.
    #[test]
    fn hunger_sperrt_den_sprint_ab_sechs() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;

        input.food_level = 7;
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting, "7 ist noch über der Grenze");

        // Waehrend des Laufens leer werden beendet den Sprint.
        input.food_level = 6;
        p.tick(&input, &world, &shapes);
        assert!(!p.sprinting, "bei 6 endet auch ein laufender Sprint");

        let mut p2 = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        p2.tick(&input, &world, &shapes);
        assert!(!p2.sprinting, "bei 6 laesst sich gar nicht erst starten");
    }

    // canSprint() faengt mit !hasBlindnessEffect() an -- blind geht nichts.
    #[test]
    fn blindheit_sperrt_den_sprint() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting);
        input.blindness = true;
        p.tick(&input, &world, &shapes);
        assert!(!p.sprinting, "Blindheit beendet auch einen laufenden Sprint");
    }

    // isBlockedFromSprinting(): eine laufende Item-Benutzung sperrt den START
    // (UseEffectsComponent.DEFAULT hat can_sprint = false). Ein LAUFENDER Sprint bleibt
    // davon unberuehrt -- shouldStopSprinting() kennt die Bedingung nicht.
    #[test]
    fn item_benutzung_sperrt_nur_den_sprintstart() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        input.using_item = true;
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        p.tick(&input, &world, &shapes);
        assert!(!p.sprinting, "während einer Item-Benutzung startet kein Sprint");

        input.using_item = false;
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting);
        input.using_item = true;
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting, "ein laufender Sprint überlebt den Beginn einer Benutzung");
    }

    #[test]
    fn sprinten_ist_schneller_als_gehen() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut walk = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut sprint = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut walk_input = idle_input(0.0);
        walk_input.forward = 1.0;
        let mut sprint_input = walk_input;
        sprint_input.sprint_held = true;
        for _ in 0..100 {
            walk.tick(&walk_input, &world, &shapes);
            sprint.tick(&sprint_input, &world, &shapes);
        }
        let walk_speed = (walk.vel.x * walk.vel.x + walk.vel.z * walk.vel.z).sqrt();
        let sprint_speed = (sprint.vel.x * sprint.vel.x + sprint.vel.z * sprint.vel.z).sqrt();
        assert!(sprint_speed > walk_speed * 1.15, "Sprint ({sprint_speed}) sollte spürbar schneller sein als Gehen ({walk_speed})");
    }

    // Gegenstueck zum Loslass-Test: die Verlangsamung laggt der Taste in BEIDE Richtungen um
    // einen Tick nach. GrimACs PlayerBaseTick liest dafuer `wasSneaking`, also die Taste vom
    // letzten Tick -- genau wie Vanilla, das inSneakingPose VOR input.tick() setzt.
    //
    // Ich hatte hier zwischenzeitlich "greift sofort beim Druecken" stehen, aus einer Messung
    // geraten, die eine andere Ursache hatte. Der Test steht jetzt auf der Formel aus Grims
    // Quelltext statt auf einer Vermutung.
    #[test]
    fn sneak_verlangsamung_laggt_einen_tick_beim_druecken() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut gehen = idle_input(0.0);
        gehen.forward = 1.0;
        for _ in 0..40 {
            p.tick(&gehen, &world, &shapes);
        }
        let mut mit_sneak = gehen;
        mit_sneak.sneak = true;
        p.tick(&mit_sneak, &world, &shapes);
        assert!(!p.in_sneaking_pose_move, "Druck-Tick rechnet noch OHNE Verlangsamung (wasSneaking war false)");
        p.tick(&mit_sneak, &world, &shapes);
        assert!(p.in_sneaking_pose_move, "ab dem Tick danach greift sie");
    }

    // Ohne gedrueckte Taste, aber unter zu niedriger Decke: dort erzwingt die Formel die
    // Schleich-Pose SOFORT -- der Zweig `!canEnterPose(STANDING)` kennt keinen Nachlauf.
    #[test]
    fn niedrige_decke_verlangsamt_ohne_taste_und_ohne_nachlauf() {
        let (world, shapes, pos) = welt_der_schleich_luecke();
        let mut p = PlayerPhysics::new(pos);
        p.refresh_on_ground(&world, &shapes);
        let mut ohne = idle_input(181.0f32.to_radians());
        ohne.forward = 1.0;
        p.tick(&ohne, &world, &shapes);
        assert!(!pose_fits(p.pos, HEIGHT_F64, &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default()), "Test setzt die niedrige Decke voraus");
        assert!(p.in_sneaking_pose_move, "unter der Decke gilt die Verlangsamung ab dem ersten Tick");
    }

    // Ein Server-Teleport (Setback) darf den Tastenzustand nicht vergessen -- sonst sieht der
    // Tick danach wie ein frischer Tastendruck aus und die Verlangsamung setzt einen Tick
    // lang aus. Genau daraus wurde im Mitschnitt eine Setback-Endlosschleife.
    // Der Tick direkt nach einem Teleport/Setback: dort ist die Geschwindigkeit genullt, die
    // angefragte Bewegung also y = 0 -- und Vanilla setzt onGround dann auf FALSE (Entity.move
    // Zeile 762-765: der Zweig laeuft fuer den eigenen Spieler immer, groundCollision verlangt
    // movement.y < 0). Der Folgetick rechnet damit mit LUFT-Beschleunigung.
    //
    // Genau das war die Ursache der Simulation-Flags (0.009800 / 0.005351 /
    // 0.002922, Zerfall 0.546 = Bodenreibung): wir behielten den Bodenkontakt, beschleunigten
    // mit dem Bodenwert und lagen damit einmalig 0.98 × (0.1 × 0.216/0.546³ − 0.02) = 0.1104
    // ueber Grims Vorhersage. Weil jeder Flag einen Setback ausloest und jeder Setback diesen
    // Tick erneut erzeugt, hielt sich die Schleife selbst am Leben.
    #[test]
    fn erster_tick_nach_teleport_meldet_keinen_bodenkontakt() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        // So sieht der Zustand direkt nach einem Server-Teleport aus (s. main.rs: neues
        // PlayerPhysics + refresh_on_ground).
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        p.refresh_on_ground(&world, &shapes);
        assert!(p.on_ground, "refresh_on_ground sieht den Boden unter den Füßen");
        assert_eq!(p.vel, DVec3::ZERO, "ein Teleport nullt die Geschwindigkeit");

        let mut lauf = idle_input(0.0);
        lauf.forward = 1.0;
        p.tick(&lauf, &world, &shapes);
        assert!(!p.on_ground, "movement.y == 0 -> kein Bodenkontakt, genau wie in Vanilla");

        // Und der Folgetick beschleunigt deshalb mit dem LUFT-Wert. Ohne diesen Tick waere die
        // Beschleunigung um rund 0.11 hoeher -- die gemessene Differenz zu Grim.
        let vor = p.vel.z;
        p.tick(&lauf, &world, &shapes);
        let zugewachsen = p.vel.z / 0.91 - vor; // 0.91 = Luftwiderstand, in der Luft ohne Bodenreibung
        let luft = 0.98 * AIR_ACCEL;
        assert!((zugewachsen - luft).abs() < 1.0e-6, "Luft-Beschleunigung erwartet ({luft}), war {zugewachsen}");

        // Danach ist der Bodenkontakt wieder da (die Schwerkraft hat vel.y auf -0.0784 gezogen).
        assert!(p.on_ground, "im nächsten Tick steht er wieder");
    }

    // Nach einer Enderperle teleportiert der Server uns mitten in die Luft. Vanilla laesst
    // onGround dabei unveraendert (setValuesFromPositionPacket fasst es nicht an), der
    // naechste Tick rechnet also noch mit BODENreibung 0.546 -- nicht mit Luft 0.91.
    //
    // Genau das zeigte GrimAC (consoledebug): ein einziger Tick daneben,
    // Offset 0.06461471, danach sauberer Zerfall mit 0.546. Der Betrag ist die waagerechte
    // Bewegung des Ticks davor mal der Reibungsdifferenz:
    //     0.177512968 * (0.91 - 0.546) = 0.06461472
    #[test]
    fn ein_teleport_laesst_den_bodenkontakt_stehen() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut laufen = idle_input(0.0);
        laufen.forward = 1.0;
        laufen.sprint_held = true;
        for _ in 0..20 {
            p.tick(&laufen, &world, &shapes);
        }
        assert!(p.on_ground, "nach zwanzig Lauf-Ticks steht er");
        let vorher = DVec3::new(p.vel.x, 0.0, p.vel.z).length();

        // So macht es der PlayerPosition-Handler in main.rs: Ziel hoch in der Luft, der
        // eigene Schwung bleibt (Enderperle, alle DELTA-Bits relativ).
        let gerettet = p.carry_over_state();
        let schwung = p.vel;
        let mut nach_perle = PlayerPhysics::new(DVec3::new(40.0, 8.0, 40.0));
        nach_perle.restore_carry_over_state(gerettet);
        nach_perle.vel = schwung;
        assert!(nach_perle.on_ground, "der Teleport darf den Bodenkontakt nicht löschen");

        // Der erste Tick nach dem Teleport traegt die waagerechte Geschwindigkeit mit der
        // BODENreibung weiter. Ohne Eingabe kommt keine Beschleunigung dazu, der Faktor ist
        // damit direkt messbar: Vanilla nimmt f * 0.91 mit f = 0.6, also 0.546.
        let mut ohne_eingabe = idle_input(0.0);
        ohne_eingabe.forward = 0.0;
        ohne_eingabe.sprint_held = false;
        nach_perle.tick(&ohne_eingabe, &world, &shapes);
        let danach = DVec3::new(nach_perle.vel.x, 0.0, nach_perle.vel.z).length();
        let faktor = danach / vorher;
        assert!(
            (faktor - 0.546).abs() < 1.0e-6,
            "nach dem Teleport muss die Bodenreibung 0.546 greifen, war {faktor} (Luft wäre 0.91)"
        );
    }

    #[test]
    fn setback_vergisst_die_sneak_taste_nicht() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut mit_sneak = idle_input(0.0);
        mit_sneak.forward = 1.0;
        mit_sneak.sneak = true;
        for _ in 0..10 {
            p.tick(&mit_sneak, &world, &shapes);
        }
        assert!(p.carry_over_state().prev_sneak, "Sneak lief, der Zustand muss gesetzt sein");
        assert!(p.crouching, "nach zehn Sneak-Ticks ist die Schleich-Pose gesetzt");

        // So macht es der PlayerPosition-Handler in main.rs.
        let gerettet = p.carry_over_state();
        let mut nach_setback = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        nach_setback.restore_carry_over_state(gerettet);
        // Auch die Pose gehoert zum geretteten Zustand: Grim behaelt seine ueber den Teleport
        // hinweg, ein Reset auf Stehen waere direkt nach dem Setback wieder ein Hitbox-Versatz.
        assert!(nach_setback.crouching, "der Setback lässt niemanden aufstehen");
        nach_setback.refresh_on_ground(&world, &shapes);
        nach_setback.tick(&mit_sneak, &world, &shapes);
        assert!(nach_setback.in_sneaking_pose_move, "direkt nach dem Setback muss weiter geschlichen werden");
    }

    // Vanillas applyMovementSpeedFactors multipliziert waehrend einer Item-Benutzung
    // zusaetzlich mit 0.2 (UseEffectsComponent.DEFAULT.speedMultiplier). Das fehlte, und
    // GrimAC rechnete es mit -- im consoledebug als Verhaeltnis 0.3/0.2 = 1.5 messbar
    // (unsere Beschleunigung 0.03823018 gegen Grims 0.02549019).
    #[test]
    fn item_benutzung_daempft_die_eingabe_auf_ein_fuenftel() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let schritt = |using_item: bool| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut i = idle_input(0.0);
            i.forward = 1.0;
            i.using_item = using_item;
            for _ in 0..40 {
                p.tick(&i, &world, &shapes);
            }
            let vor = p.pos;
            p.tick(&i, &world, &shapes);
            let d = p.pos - vor;
            (d.x * d.x + d.z * d.z).sqrt()
        };
        let verhaeltnis = schritt(true) / schritt(false);
        assert!(
            (verhaeltnis - ITEM_USE_SPEED_MULT).abs() < 1.0e-6,
            "Benutzung muss exakt {ITEM_USE_SPEED_MULT} der normalen Geschwindigkeit sein, war {verhaeltnis}"
        );
    }

    // Und die beiden Faktoren multiplizieren sich, genau wie in applyMovementSpeedFactors.
    #[test]
    fn schleichen_und_benutzen_multiplizieren_sich() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let schritt = |sneak: bool, using_item: bool| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut i = idle_input(0.0);
            i.forward = 1.0;
            i.sneak = sneak;
            i.using_item = using_item;
            for _ in 0..40 {
                p.tick(&i, &world, &shapes);
            }
            let vor = p.pos;
            p.tick(&i, &world, &shapes);
            let d = p.pos - vor;
            (d.x * d.x + d.z * d.z).sqrt()
        };
        let verhaeltnis = schritt(true, true) / schritt(false, false);
        let erwartet = DEFAULT_SNEAKING_SPEED * ITEM_USE_SPEED_MULT;
        assert!((verhaeltnis - erwartet).abs() < 1.0e-6, "erwartet {erwartet}, war {verhaeltnis}");
    }

    // Grims SprintE ("grim.sprint.wall", Sprinting while colliding with a wall) flaggt,
    // wenn der Spieler im Tick NACH einer HARTEN Horizontalkollision immer noch als
    // sprintend gesendet wird. Vanilla beendet den Sprint dort (ClientPlayerEntity.
    // shouldStopRunSprinting: horizontalCollision && !minorHorizontalCollision).
    //
    // Massgeblich ist die HARTE Kollision, nicht jede: der allererste Beruehrungs-Tick
    // beschneidet die Bewegung oft nur um einen Bruchteil (knapp ueber der 1e-5-Schwelle von
    // approximatelyEquals). Die zurueckgelegte Richtung bleibt dabei praktisch die gewollte,
    // Vanilla wertet das als Streifen und sprintet weiter -- und Grim rechnet genauso
    // (wasHardHorizontalCollision = horizontalCollision && !softHorizontalCollision).
    // Der frueher hier stehende Anspruch "nach der ERSTEN Kollision nie wieder sprinten" war
    // strenger als Vanilla und ist genau die Abweichung, die den Sprint aufblinken liess.
    #[test]
    fn sprint_endet_an_der_wand() {
        let world = world_with_floor_and_wall(0, 10);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.strafe = 1.0; // yaw 0: +X, auf die Wand bei x=10 zu
        input.forward = 1.0;
        input.sprint_held = true;

        let mut sprint_nach_hartem_anprall = 0;
        let mut prallte_hart_an = false;
        for _ in 0..80 {
            p.tick(&input, &world, &shapes);
            if prallte_hart_an && p.sprinting {
                sprint_nach_hartem_anprall += 1;
            }
            prallte_hart_an |= p.horizontal_collision && !p.minor_horizontal_collision;
        }
        assert!(prallte_hart_an, "Test setzt voraus, dass die Wand überhaupt hart getroffen wird");
        assert_eq!(sprint_nach_hartem_anprall, 0, "nach dem harten Anprall darf kein Tick mehr sprintend gesendet werden");
    }

    // Die Gegenprobe zum Test darueber: an einer Wand ENTLANG (Blick 5 Grad in die Wand
    // hinein, Vorwaertsdrang) bleibt der Sprint stehen. Genau das konnte unsere alte Fassung
    // nicht -- sie brach bei jedem Wandkontakt ab, startete bei gehaltener Taste im naechsten
    // Tick neu, und dieses Aufblinken war der SprintE-Flag im Kaktus-Labyrinth.
    #[test]
    fn an_der_wand_entlang_bleibt_der_sprint_stehen() {
        let world = world_with_floor_and_wall(0, 10);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        // Blick fast parallel zur Wand (Wand liegt bei x=10, wir laufen nach +Z), nur 5 Grad
        // in sie hinein -- unterhalb von Vanillas 8-Grad-Grenze. Vorzeichen: bei yaw 0 zeigt
        // der Vorwaertsdrang nach +Z, und vel.x = -sin(yaw) -- fuer +X (zur Wand hin) muss
        // yaw also NEGATIV sein.
        let mut input = idle_input(-5.0f32.to_radians());
        input.forward = 1.0;
        input.sprint_held = true;

        // Erst mal an die Wand heranlaufen.
        for _ in 0..120 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.horizontal_collision, "Testaufbau: wir müssen an der Wand anliegen");
        assert!(p.minor_horizontal_collision, "5 Grad in die Wand ist ein Streifen, kein Anprall");
        assert!(p.sprinting, "beim Entlangstreifen darf der Sprint nicht abbrechen");
    }

    // Auf den eigenen, noch unbestaetigten Block muss man sofort weiterbauen koennen -- der
    // Strahl sieht die Vorhersage-Schicht, genau wie Vanillas Strahl seine ClientWorld sieht
    // (s. Kommentar ueber raycast_block: hier stand zwischenzeitlich server_block_at, was das
    // Weiterbauen bis zur Bestaetigung blockierte).
    #[test]
    fn strahl_trifft_den_eigenen_vorhergesagten_block() {
        let mut world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let eye = DVec3::new(8.5, 3.0, 8.5);
        let nach_unten = DVec3::new(0.0, -1.0, 0.0);

        // Ohne Vorhersage trifft der Strahl den Boden bei y=0.
        let boden = raycast_block(eye, nach_unten, 4.5, &world, &shapes).expect("Boden");
        assert_eq!(boden.block, (8, 0, 8));

        // Jetzt ein vorhergesagter Block direkt darueber: DEN muss der Strahl treffen, und
        // zwar auf der Oberseite -- sonst landet der naechste Block in seiner Position statt
        // darauf.
        let stein = (0..world::block_registry::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).expect("Stein");
        world.set_pending_block(8, 1, 8, stein);
        let geist = raycast_block(eye, nach_unten, 4.5, &world, &shapes).expect("Geisterblock");
        assert_eq!(geist.block, (8, 1, 8), "der vorhergesagte Block liegt vor dem Boden");
        assert_eq!(geist.face, protocol::packets::FACE_UP);
    }

    // Rechtsklick-Strahl: Trefferseite und Trefferstelle sind das, was im Paket landet --
    // eine falsche Seite platziert den Block auf der falschen Wuerfelflaeche.
    #[test]
    fn strahl_von_oben_trifft_die_oberseite() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        // Auge ueber der Bodenplatte bei y=0 (also Oberkante y=1), Blick gerade nach unten.
        let eye = DVec3::new(8.25, 3.0, 8.75);
        let hit = raycast_block(eye, DVec3::new(0.0, -1.0, 0.0), 4.5, &world, &shapes).expect("Boden muss getroffen werden");
        assert_eq!(hit.block, (8, 0, 8));
        assert_eq!(hit.face, protocol::packets::FACE_UP, "von oben kommend ist es die Oberseite");
        assert!((hit.cursor.1 - 1.0).abs() < 1.0e-5, "Oberkante des Blocks: {:?}", hit.cursor);
        // Die waagerechte Trefferstelle ist der Bruchteil innerhalb des Blocks.
        assert!((hit.cursor.0 - 0.25).abs() < 1.0e-5, "{:?}", hit.cursor);
        assert!((hit.cursor.2 - 0.75).abs() < 1.0e-5, "{:?}", hit.cursor);
        assert!(!hit.inside);
    }

    #[test]
    fn strahl_von_der_seite_trifft_die_seitenflaeche() {
        let world = world_with_floor_and_wall(0, 10);
        let shapes = world::BlockShapes::all_full_cubes();
        // Auge links der Wand bei x=10, Blick in +X.
        let eye = DVec3::new(8.0, 2.0, 8.5);
        let hit = raycast_block(eye, DVec3::new(1.0, 0.0, 0.0), 4.5, &world, &shapes).expect("Wand muss getroffen werden");
        assert_eq!(hit.block, (10, 2, 8));
        assert_eq!(hit.face, protocol::packets::FACE_WEST, "in +X laufend trifft man die Westseite");
        assert!((hit.cursor.0 - 0.0).abs() < 1.0e-5, "genau auf der Blockkante: {:?}", hit.cursor);
    }

    #[test]
    fn strahl_ins_leere_trifft_nichts() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let eye = DVec3::new(8.0, 20.0, 8.0);
        assert!(raycast_block(eye, DVec3::new(0.0, 1.0, 0.0), 4.5, &world, &shapes).is_none());
        // Und der Boden liegt ausserhalb der Reichweite, obwohl der Strahl dorthin zeigt.
        assert!(raycast_block(eye, DVec3::new(0.0, -1.0, 0.0), 4.5, &world, &shapes).is_none());
    }

    // Die geflaggte Stelle aus Grims consoledebug ("test sneak", 2026-08-05), genau aus dessen
    // Blockdump nachgebaut:
    //   y=-11, x=7,   z=8 : oak_slab[type=top]   -> Decke der Luecke, belegt -10.5 .. -10.0
    //   y=-12, x=6/8, z=8 : dirt                 -> die Wangen daneben
    //   y=-13             : Boden
    // Unter der Stufe bleiben von -12 bis -10.5 GENAU 1.5 Bloecke -- die Schleich-Hitbox.
    // Grim rechnete dort mit Sneak-Faktor (Simulated input 0.0294), unser Client ohne
    // (Client input 0.098) -> Offset 0.0686, Setback-Schleife.
    fn welt_der_schleich_luecke() -> (World, world::BlockShapes, DVec3) {
        let state_von = |name: &str, props: &[(&str, &str)]| -> u32 {
            (0..world::block_registry::state_count() as u32)
                .find(|&s| world::block_name(s) == Some(name) && props.iter().all(|(k, v)| world::state_props::property(s, k) == Some(v)))
                .unwrap_or_else(|| panic!("Blockstate {name}{props:?} fehlt"))
        };
        let dirt = state_von("dirt", &[]);
        let slab = state_von("oak_slab", &[("type", "top")]);

        // Nur die obere Stufe braucht eine echte Form; alles andere ist Vollwuerfel (None).
        let mut per_state: Vec<Option<Vec<world::ShapeBox>>> = vec![None; world::block_registry::state_count()];
        per_state[slab as usize] = Some(vec![world::ShapeBox {
            min: [0.0, 0.5, 0.0],
            max: [1.0, 1.0, 1.0],
            faces: [true; 6],
            content: [false; 6],
            uv: [None; 6],
        }]);
        let shapes = world::BlockShapes::new(per_state);

        let setze = |states: &mut [u32], x: i32, y: i32, z: i32, state: u32| {
            let local_y = (y - world::MIN_Y).rem_euclid(16);
            states[((local_y * 16 + z) * 16 + x) as usize] = state;
        };
        let mut w = World::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == (-13 - world::MIN_Y).div_euclid(16) {
                        for x in 0..16 {
                            for z in 0..16 {
                                setze(&mut states, x, -13, z, dirt);
                            }
                        }
                        if cx == 0 && cz == 0 {
                            setze(&mut states, 6, -12, 8, dirt);
                            setze(&mut states, 8, -12, 8, dirt);
                            setze(&mut states, 6, -11, 8, dirt);
                            setze(&mut states, 8, -11, 8, dirt);
                            setze(&mut states, 7, -11, 8, slab);
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        (w, shapes, DVec3::new(7.473631490959559, -12.0, 9.202014757858826))
    }

    // Der ZEITPUNKT des Posenwechsels (sonst flaggt Schleichen direkt in einen
    // 1.5-Bloecke-Gang). Vanilla ruft updatePose() als letzte
    // Anweisung von PlayerEntity.tick(), GrimAC genauso (MovementCheckRunner: doBaseTick ->
    // livingEntityAIStep -> updatePlayerPose). Die Hitbox eines Ticks ist damit IMMER die Pose
    // vom Ende des VORIGEN Ticks -- auf dem Tick, an dem Sneak gedrueckt wird, gilt also noch
    // die Stehbox. Vorher aktualisierten wir die Pose am Tick-ANFANG und schlupften dadurch
    // einen Tick zu frueh unter die Stufe, waehrend Grims Vorhersage dort noch anstiess.
    #[test]
    fn posenwechsel_gilt_erst_ab_dem_naechsten_tick() {
        let (world, shapes, pos) = welt_der_schleich_luecke();
        // Knapp VOR der Luecke: die Stufe steht bei z=8 (Zelle 8.0..9.0), die Spielerbox ist
        // 0.6 breit -- ab z < 9.3 ragt sie unter die Stufe.
        const KANTE: f64 = 9.3;
        let mut p = PlayerPhysics::new(DVec3::new(pos.x, pos.y, 9.35));
        p.refresh_on_ground(&world, &shapes);
        let mut i = idle_input(181.0f32.to_radians());
        i.forward = 1.0;
        i.sneak = true;

        // Erster Tick MIT gedruecktem Sneak: die Pose ist noch STANDING, die Stufe blockt.
        p.tick(&i, &world, &shapes);
        assert!(p.pos.z >= KANTE - 1.0e-9, "am Sneak-Tick gilt noch die Stehbox, die Stufe muss blocken -- z={} (Kante {KANTE})", p.pos.z);
        // Aber die Pose steht am ENDE dieses Ticks bereits auf Schleichen.
        assert!(p.crouching, "am Tick-Ende ist die Schleich-Pose gesetzt");

        // Ab dem naechsten Tick passt die Schleichbox unter die Stufe -- sonst waere der Test
        // oben auch dann gruen, wenn wir gar nicht mehr hineinkaemen.
        for _ in 0..20 {
            p.tick(&i, &world, &shapes);
        }
        assert!(p.pos.z < KANTE, "schleichend muss man unter die Stufe kommen -- z={}", p.pos.z);
    }

    // Der Kern des Mitschnitts: unter der Stufe MUSS der Sneak-Faktor greifen, sonst laufen
    // wir mit 0.098 los, waehrend Grim 0.0294 erwartet.
    #[test]
    fn in_der_schleich_luecke_greift_der_sneak_faktor() {
        let (world, shapes, pos) = welt_der_schleich_luecke();
        let mut p = PlayerPhysics::new(pos);
        p.refresh_on_ground(&world, &shapes);
        let mut i = idle_input(181.0f32.to_radians());
        i.forward = 1.0;
        i.sneak = true;
        p.tick(&i, &world, &shapes);
        assert!(
            p.in_sneaking_pose_move,
            "unter der oberen Stufe (1.5 Bloecke Luft) muss die Schleich-Pose greifen -- crouch={} swim={} passt1.8={} passt1.5={}",
            p.crouching,
            p.swimming,
            pose_fits(p.pos, HEIGHT_F64, &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default()),
            pose_fits(p.pos, CROUCHING_HEIGHT_F64, &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default())
        );

        // Und OHNE Sneak-Taste ebenso: unter einer zu niedrigen Decke erzwingt Vanilla die
        // Schleich-Pose von sich aus (inSneakingPose = ... || !canChangeIntoPose(STANDING)),
        // der ×0.3 gilt dann auch ungedrueckt. Ein frisch gebautes PlayerPhysics ist zugleich
        // der Zustand direkt nach einem Server-Setback -- auch dort muss es sofort stimmen.
        let mut q = PlayerPhysics::new(pos);
        q.refresh_on_ground(&world, &shapes);
        let mut ohne = idle_input(181.0f32.to_radians());
        ohne.forward = 1.0;
        q.tick(&ohne, &world, &shapes);
        assert!(q.in_sneaking_pose_move, "unter der Stufe gilt der Schleich-Faktor auch ohne gedrückte Taste");
    }

    // Regressionstest fuer den grossen Crouch->Sprint-Uebergangs-Flag (s. "test 13",
    // Offset 0.089): Vanilla setzt das isInSneakingPose()-FELD (shouldSlowDown()) in
    // ClientPlayerEntity.tickMovement() VOR input.tick(), die Bewegungs-Verlangsamung
    // (×0.3) laggt der Sneak-Taste also um genau EINEN Tick nach. Auf dem Tick, an dem Sneak
    // losgelassen wird, muss die Verlangsamung darum noch greifen -- der Client darf NICHT
    // schon im selben Tick auf volle Geschwindigkeit (0.98) springen. Vorher tat er genau das
    // und ueberschoss Grims Simulation massiv.
    // Grim rechnet mit GENAU dem Faktor 0.3 auf dem Bewegungs-Input (EntityAttributes
    // SNEAKING_SPEED). Aus einem echten consoledebug-Mitschnitt ("test sneak"):
    //   Client input:    0.09800001233816147
    //   Simulated input: 0.02940000407397747   = 0.098 * 0.3
    // Weil die Beschleunigung linear in den Input eingeht, muss sich das Verhaeltnis 1:1 in
    // der Schrittweite im Gleichgewicht wiederfinden -- ein Test, der die Zahl festnagelt,
    // statt nur einen Bereich abzustecken.
    // Schleichender Schritt: der Faktor ist das SNEAKING_SPEED-Attribut, kein fester Wert.
    // Mit Stufe III (0.3 + 3 x 0.15 = 0.75) muss die Schrittweite genau 0.75 der
    // Gehgeschwindigkeit sein -- rechnete der Client weiter mit 0.3, waere er in jedem
    // Schleich-Tick weniger als halb so schnell wie der Server (und wie GrimAC) erwartet.
    #[test]
    fn schleichtempo_folgt_dem_attribut() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let schritt = |sneak: bool, sneaking_speed: f64| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut i = idle_input(0.0);
            i.forward = 1.0;
            i.sneak = sneak;
            i.sneaking_speed = sneaking_speed;
            for _ in 0..40 {
                p.tick(&i, &world, &shapes);
            }
            let before = p.pos;
            p.tick(&i, &world, &shapes);
            let d = p.pos - before;
            (d.x * d.x + d.z * d.z).sqrt()
        };
        let gehen = schritt(false, DEFAULT_SNEAKING_SPEED);
        let verhaeltnis = schritt(true, 0.75) / gehen;
        assert!((verhaeltnis - 0.75).abs() < 1.0e-6, "Schleichender Schritt III = 0.75, war {verhaeltnis}");
    }

    // Grundtempo aus dem Attribut-Paket (Plugin-Walkspeed): der Client muss damit rechnen,
    // nicht mit den fest verdrahteten 0.1. Ein Server, der das Tempo verdoppelt, erwartet
    // auch die doppelte Schrittweite.
    #[test]
    fn grundtempo_folgt_dem_attribut() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let schritt = |tempo: f64| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut i = idle_input(0.0);
            i.forward = 1.0;
            i.movement_speed = tempo;
            for _ in 0..60 {
                p.tick(&i, &world, &shapes);
            }
            let before = p.pos;
            p.tick(&i, &world, &shapes);
            let d = p.pos - before;
            (d.x * d.x + d.z * d.z).sqrt()
        };
        let verhaeltnis = schritt(BASE_SPEED * 2.0) / schritt(BASE_SPEED);
        assert!((verhaeltnis - 2.0).abs() < 1.0e-3, "doppeltes Grundtempo = doppelte Schrittweite, war {verhaeltnis}");
    }

    #[test]
    fn schleichen_ist_exakt_drei_zehntel_der_gehgeschwindigkeit() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let schritt = |sneak: bool| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
            let mut i = idle_input(0.0);
            i.forward = 1.0;
            i.sneak = sneak;
            for _ in 0..40 {
                p.tick(&i, &world, &shapes);
            }
            let before = p.pos;
            p.tick(&i, &world, &shapes);
            let d = p.pos - before;
            assert!(p.on_ground, "Messung nur im Gleichgewicht am Boden gültig");
            (d.x * d.x + d.z * d.z).sqrt()
        };
        let verhaeltnis = schritt(true) / schritt(false);
        assert!(
            (verhaeltnis - DEFAULT_SNEAKING_SPEED).abs() < 1.0e-6,
            "Schleichen muss exakt {DEFAULT_SNEAKING_SPEED} der Gehgeschwindigkeit sein, war {verhaeltnis}"
        );
    }

    #[test]
    fn sneak_verlangsamung_laggt_einen_tick_beim_loslassen() {
        let world = world_with_floor(0);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut sneak_input = idle_input(0.0);
        sneak_input.forward = 1.0;
        sneak_input.sneak = true;
        // Ins Schleich-Gleichgewicht laufen (~0.065 Bloecke/Tick Schrittweite).
        for _ in 0..80 {
            p.tick(&sneak_input, &world, &shapes);
        }
        // Schrittweite (Positionsdelta) messen, nicht self.vel: self.vel ist schon der
        // gedaempfte Tick-Ende-Wert, die Bewegung selbst (und damit das, was Grim sieht)
        // laeuft mit der Geschwindigkeit VOR der Daempfung -- s. laufen_naehert_sich_...
        let before = p.pos;
        p.tick(&sneak_input, &world, &shapes);
        let sneak_step = (p.pos - before).length();
        assert!((0.05..0.08).contains(&sneak_step), "Schleich-Schrittweite unplausibel: {sneak_step}");

        // Tick 1 nach dem Loslassen: Verlangsamung laggt -> Schrittweite bleibt auf
        // Schleich-Niveau, springt NICHT auf ~0.13 (das waere der Bug ohne Lag).
        let mut release_input = sneak_input;
        release_input.sneak = false;
        let before = p.pos;
        p.tick(&release_input, &world, &shapes);
        let release_step = (p.pos - before).length();
        assert!(
            release_step < 0.09,
            "auf dem Loslass-Tick muss die Sneak-Verlangsamung noch greifen (erwartet ~0.065), war {release_step}"
        );

        // Tick 2: jetzt ist die Verlangsamung weg, Schrittweite klettert Richtung Geh-Terminal.
        let before = p.pos;
        p.tick(&release_input, &world, &shapes);
        let next_step = (p.pos - before).length();
        assert!(next_step > 0.11, "einen Tick nach dem Loslassen muss die volle Geschwindigkeit greifen, war {next_step}");
    }

    // Baut world_with_floor() zusaetzlich eine 3 Bloecke hohe Wand bei Welt-X=10 ein --
    // in EINEM Rutsch, denn World::insert_column() ERSETZT eine Spalte komplett statt sie
    // zu mischen; ein zweiter Aufruf fuer dieselbe Spalte wuerde den zuvor gesetzten Boden
    // wieder loeschen.
    fn world_with_floor_and_wall(floor_y: i32, wall_x: i32) -> World {
        let mut w = World::new();
        let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
        let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
        let wall_bottom = floor_y + 1;
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == floor_section {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                            }
                        }
                    }
                    let section_base_y = world::MIN_Y + i * 16;
                    for wy in wall_bottom..wall_bottom + 3 {
                        if wy >= section_base_y && wy < section_base_y + 16 {
                            let local_x = wall_x.rem_euclid(16);
                            if wall_x.div_euclid(16) == cx {
                                let ly = wy - section_base_y;
                                for z in 0..16 {
                                    states[((ly * 16 + z) * 16 + local_x) as usize] = 1;
                                }
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    #[test]
    fn wand_blockiert_horizontale_bewegung() {
        // Wand direkt vor dem Spieler in +X-Richtung, bei Welt-X=10.
        let world = world_with_floor_and_wall(0, 10);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0); // yaw 0 = Blick nach +Z, wir wollen +X -> strafe
        input.strafe = 1.0;
        for _ in 0..200 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.pos.x <= 9.7 + 1.0e-3, "sollte an der Wand bei x=10 stehen bleiben, x={}", p.pos.x);
        assert!(p.horizontal_collision);
    }

    // Vanilla (Entity.move) leitet horizontalCollision aus dem Vergleich "angefragte gegen
    // tatsaechliche Bewegung" ab, NICHT aus den Kollisions-Flags. Der Sneak-Kantenschutz
    // beschneidet die Bewegung ohne jede Kollision -- Vanilla meldet trotzdem true. Das Bit
    // geht in jedem Bewegungspaket an den Server, eine Abweichung faellt dort auf.
    #[test]
    fn kantenschutz_meldet_horizontale_kollision_wie_vanilla() {
        let world = world_with_floor_up_to_x(0, 10);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut sneaker = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.0));
        let mut sneak_in = idle_input(0.0);
        sneak_in.strafe = 1.0; // yaw 0: +X, also auf die Kante bei x=10 zu
        sneak_in.sneak = true;
        for _ in 0..200 {
            sneaker.tick(&sneak_in, &world, &shapes);
        }
        // An der Kante angekommen: der Schutz klemmt die Bewegung jeden Tick weg, obwohl
        // nichts kollidiert -- genau dann muss das Bit gesetzt sein.
        assert!(sneaker.horizontal_collision, "an der Kante geklemmt, aber horizontalCollision=false (x={})", sneaker.pos.x);
    }

    // Fallhoehe: Entity.fall() schreibt sie mit der TATSAECHLICHEN Vertikalbewegung fort und
    // nullt sie beim Bodenkontakt. Sie ist hier kein Selbstzweck -- der Sneak-Kantenschutz
    // haengt daran (s. is_standing_on_surface).
    #[test]
    fn fallhoehe_waechst_beim_fallen_und_wird_bei_der_landung_genullt() {
        let world = world_with_floor_up_to_x(0, 32);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(8.0, 6.0, 8.0));
        for _ in 0..5 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(!p.on_ground, "Test setzt voraus, dass er noch fällt");
        assert!(p.fall_distance > 0.5, "Fallhöhe müsste nach 5 Ticks freiem Fall wachsen: {}", p.fall_distance);

        for _ in 0..60 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(p.on_ground, "haette laengst landen muessen");
        assert_eq!(p.fall_distance, 0.0, "Bodenkontakt nullt die Fallhöhe");
    }

    // Der zweite Zweig von isStandingOnSurface: kurz nach dem Verlassen der Kante ist man
    // nicht mehr on_ground, aber noch keine Schritthoehe gefallen -- Vanilla haelt einen
    // weiterhin fest. Vorher griff der Schutz hier nicht, und genau in diesen Ticks lief man
    // weiter, waehrend die Server-Simulation klemmte.
    #[test]
    fn kantenschutz_greift_auch_kurz_nach_dem_abheben() {
        let world = world_with_floor_up_to_x(0, 10);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(9.0, 1.0, 8.0));
        p.crouching = true;
        // Nicht am Boden, aber erst ein winziges Stueck gefallen: der Block liegt noch
        // innerhalb der verbleibenden Schritthoehe.
        p.on_ground = false;
        p.fall_distance = 0.1;
        assert!(p.is_standing_on_surface(&world, &shapes), "Boden in Reichweite -> Vanilla hält fest");

        // Schon weiter gefallen als eine ganze Schritthoehe: jetzt ist Schluss.
        p.fall_distance = 0.7;
        assert!(!p.is_standing_on_surface(&world, &shapes), "über der Schritthöhe darf nichts mehr klemmen");
    }

    #[test]
    fn sneaken_verhindert_das_fallen_ueber_die_kante() {
        // Boden nur bis Welt-X < 10 (Abbruchkante bei x=10). Spieler links davon, laeuft nach +X.
        let world = world_with_floor_up_to_x(0, 10);
        let shapes = world::BlockShapes::all_full_cubes();

        // Mit Sneak: darf ueber die Kante lehnen, aber NICHT herunterfallen (bleibt am Boden, y~1).
        let mut sneaker = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.0));
        let mut sneak_in = idle_input(0.0);
        sneak_in.strafe = 1.0; // yaw 0: +X
        sneak_in.sneak = true;
        for _ in 0..200 {
            sneaker.tick(&sneak_in, &world, &shapes);
        }
        assert!(sneaker.on_ground, "Schleicher muss auf dem Block bleiben, ist aber gefallen (y={})", sneaker.pos.y);
        assert!(sneaker.pos.y > 0.5, "Schleicher darf nicht herunterfallen, y={}", sneaker.pos.y);
        // Er darf leicht ueber die Kante lehnen, aber nicht weit ins Leere laufen.
        assert!(sneaker.pos.x < 10.5, "Schleicher darf nicht ueber die Kante hinauslaufen, x={}", sneaker.pos.x);

        // Kontrolle OHNE Sneak: laeuft ueber die Kante und faellt.
        let mut walker = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.0));
        let mut walk_in = idle_input(0.0);
        walk_in.strafe = 1.0;
        for _ in 0..200 {
            walker.tick(&walk_in, &world, &shapes);
        }
        assert!(walker.pos.y < 0.0, "ohne Sneak muss man ueber die Kante fallen, y={}", walker.pos.y);
    }

    // Regressionstest fuer einen echten Bug: sweep_axis hat die zu
    // scannenden Block-Reihen bisher nur aus der Spieler-Y-Position berechnet. Ein Zaun
    // (oder eine Mauer/ein Zauntor) sitzt zwar in EINER Zelle, seine Kollisionsbox reicht
    // aber bis 1.5 Bloecke hoch (s. world::vanilla_shapes) -- ragt also in die Zelle
    // DARUEBER. Sobald der Spieler beim Springen hoch genug war, fiel die Zaun-Zelle aus
    // dem Scan-Bereich, und man ist mitten durch den eigentlich noch blockierenden oberen
    // Teil gelaufen (auf dem Server als "moved wrongly" geflaggt worden). Simuliert genau
    // dieses Szenario: mehrfach wiederholt gegen einen einzelnen, unverbundenen Zaunpfosten
    // anspringen -- muss auch nach vielen Sprung-Versuchen an Ort und Stelle bleiben.
    // Baut world_with_floor() zusaetzlich eine Wassersaeule (echte "water"-Quellblock-States,
    // level=0, s. world::fluid) von water_bottom bis water_top (inklusive) ein.
    fn world_with_floor_and_water(floor_y: i32, water_bottom: i32, water_top: i32) -> World {
        let water_state = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
            .expect("water-Quellblock-State nicht in der Registry gefunden");

        let mut w = World::new();
        let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
        let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == floor_section {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                            }
                        }
                    }
                    let section_base_y = world::MIN_Y + i * 16;
                    for wy in water_bottom..=water_top {
                        if wy >= section_base_y && wy < section_base_y + 16 {
                            let ly = wy - section_base_y;
                            for x in 0..16usize {
                                for z in 0..16usize {
                                    states[(ly as usize * 16 + z) * 16 + x] = water_state;
                                }
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Wie world_with_floor_and_water, aber mit EINER flachen Wasserschicht (fliessendes
    // Wasser mit "level"-Property statt Quellblock, s. world::fluid::amount_from_level) auf
    // dem Boden -- fuer den Wasser-Sprung-Cooldown-Test unten braucht es echt seichtes
    // Wasser (Hoehe <= FLUID_JUMP_THRESHOLD=0.4), ein normaler Quellblock (level=0, Hoehe
    // ~0.89) waere zu tief fuer den "an Land springen"-Zweig.
    // Wasserlaeufer (WATER_MOVEMENT_EFFICIENCY): mit der Verzauberung schwimmt man spuerbar
    // schneller -- travelInWater zieht sowohl die Bremse (0.8 -> 0.546) als auch die
    // Beschleunigung (0.02 -> Landtempo) Richtung Land. Ohne das Attribut bliebe es beim
    // langsamen Grundwert, waehrend Server und GrimAC mit dem schnellen rechnen.
    #[test]
    fn wasserlaeufer_macht_schwimmen_schneller() {
        let world = world_with_floor_and_water(0, 1, 8);
        let shapes = world::BlockShapes::all_full_cubes();
        let schritt = |effizienz: f64| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 4.0, 8.0));
            let mut i = idle_input(0.0);
            i.forward = 1.0;
            i.water_movement_efficiency = effizienz;
            for _ in 0..60 {
                p.tick(&i, &world, &shapes);
            }
            let before = p.pos;
            p.tick(&i, &world, &shapes);
            let d = p.pos - before;
            (d.x * d.x + d.z * d.z).sqrt()
        };
        let ohne = schritt(0.0);
        let mit = schritt(1.0);
        assert!(mit > ohne * 1.5, "Wasserläufer III muss deutlich schneller sein: {mit} gegen {ohne}");
    }

    fn world_with_floor_and_shallow_water(floor_y: i32, level: u8) -> World {
        let water_state = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some(&level.to_string()))
            .expect("fließender water-State mit diesem level nicht in der Registry gefunden");

        let mut w = World::new();
        let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
        let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
        let water_section = (floor_y + 1 - world::MIN_Y).div_euclid(16);
        let water_local_y = (floor_y + 1 - world::MIN_Y).rem_euclid(16);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == floor_section {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                            }
                        }
                    }
                    if i == water_section {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((water_local_y * 16 + z) * 16 + x) as usize] = water_state;
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Echter Vanilla-Mechanismus ("water hopping", s. try_jump-Kommentar):
    // haelt man Springen in seichtem Wasser (Hoehe <= 0.4) gedrueckt, bekommt man in
    // Vanilla nur alle 10 Ticks (0.5s) einen echten Sprung (jumpingCooldown), nicht jeden
    // Tick, in dem man gerade wieder Bodenkontakt hat -- Wasser bremst die
    // Aufwaertsgeschwindigkeit staerker als Luft, man waere sonst ohne dieses Gate viel
    // schneller wieder sprungbereit als an Land.
    #[test]
    fn wasser_sprung_hat_dieselbe_abklingzeit_wie_landsprung() {
        let world = world_with_floor_and_shallow_water(0, 6); // Hoehe 2/9 ~ 0.22, klar seicht
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        // Erst absetzen lassen (on_ground wird erst durch echte Kollision im ersten Tick
        // gesetzt, s. andere Tests dieser Datei), sonst wuerde der allererste Tick
        // faelschlich noch als "in der Luft" gelten und den Kick- statt den Sprung-Zweig
        // nehmen.
        for _ in 0..5 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(p.on_ground, "sollte nach dem Absetzen auf dem Boden stehen");
        let mut input = idle_input(0.0);
        input.jump = true;

        // Erster Tick: sollte sofort einen echten Sprung ausloesen (Cooldown startet bei 0).
        p.tick(&input, &world, &shapes);
        assert!(p.vel.y > 0.3, "erster Sprung sollte volle Hoehe geben, vel.y={}", p.vel.y);

        // Waehrend der Spieler noch in der Luft ist (jump weiter gehalten), darf KEIN
        // zweiter voller Sprung dazukommen -- ohne den Cooldown wuerde jedes erneute
        // "on_ground"-Antippen sofort wieder JUMP_VELOCITY setzen.
        let mut max_vel_y_after_first_tick = f64::MIN;
        for _ in 0..8 {
            p.tick(&input, &world, &shapes);
            max_vel_y_after_first_tick = max_vel_y_after_first_tick.max(p.vel.y);
        }
        assert!(
            max_vel_y_after_first_tick < 0.3,
            "innerhalb der 10-Tick-Abklingzeit sollte kein zweiter voller Sprung passieren, max vel.y={max_vel_y_after_first_tick}"
        );
    }

    // Tiefes Wasser (weit ueber und unter dem Spieler, keine Bodenberuehrung), mit einem
    // einzelnen 1 Block hohen, untergetauchten Wandsegment quer zur Schwimmrichtung -- fuer
    // den jump_out_of_fluid-Test unten: schwimmt man dagegen, MUSS horizontal_collision
    // greifen, waehrend darueber noch reichlich Wasser (kein Fels) fuer den Frei-Test ist.
    fn world_with_deep_water_and_underwater_wall(wall_x: i32, wall_y: i32) -> World {
        let water_state = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
            .expect("water-Quellblock-State nicht in der Registry gefunden");

        let mut w = World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    let section_base_y = world::MIN_Y + i * 16;
                    for wy in -20..=40 {
                        if wy >= section_base_y && wy < section_base_y + 16 {
                            let ly = wy - section_base_y;
                            for x in 0..16usize {
                                for z in 0..16usize {
                                    states[(ly as usize * 16 + z) * 16 + x] = water_state;
                                }
                            }
                        }
                    }
                    if wall_x.div_euclid(16) == cx && wall_y >= section_base_y && wall_y < section_base_y + 16 {
                        let ly = (wall_y - section_base_y) as usize;
                        let local_x = wall_x.rem_euclid(16) as usize;
                        for z in 0..16usize {
                            states[(ly * 16 + z) * 16 + local_x] = 1;
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Vanilla-Mechanismus "water hopping" (resetVerticalVelocityInFluid): schwimmt man OHNE zu
    // springen horizontal gegen eine Kante und ist DARUEBER LUFT, klettert man automatisch ein
    // Stueck heraus (fester vel.y=0.3-Kick). ENTSCHEIDEND (als
    // GrimAC-Flag-Ursache erkannt): Vanillas Bedingung ist doesNotCollide(...) und das verlangt
    // die Zielbox frei von Bloecken UND FLUID (Entity.doesNotCollide: isSpaceEmpty && !contains-
    // Fluid). In TIEFEM Wasser gegen eine voll untergetauchte Wand ist ueber der Kante wieder
    // Wasser -> KEIN Hop. Frueher (nur pose_fits, ohne Fluid-Check) hopte der Client hier
    // faelschlich und wich horizontal von Grims Prediction ab.
    #[test]
    fn schwimmen_gegen_untergetauchte_wand_in_tiefem_wasser_klettert_nicht_heraus() {
        let wall_x = 10;
        let wall_y = 5;
        let world = world_with_deep_water_and_underwater_wall(wall_x, wall_y);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(5.0, wall_y as f64, 5.0));
        let mut input = idle_input(0.0); // yaw 0 = Blick nach +Z, wir wollen +X -> strafe
        input.strafe = 1.0;

        let mut saw_kick = false;
        for _ in 0..150 {
            p.tick(&input, &world, &shapes);
            if p.horizontal_collision && (p.vel.y - 0.3).abs() < 1e-9 {
                saw_kick = true;
                break;
            }
        }
        assert!(
            !saw_kick,
            "unter Wasser (Ziel über der Kante ist wieder Wasser) darf KEIN 0.3-Hop feuern -- das war die GrimAC-Abweichung"
        );
    }

    // Ein Becken mit Ufer: Boden bei y=0, ab x >= shore_x Fels bis zur Wasseroberkante, davor
    // Wasser -- ueber beidem Luft. Genau die Geometrie, an der "water hopping" in Vanilla
    // stattfindet (Gegenstueck zu world_with_deep_water_and_underwater_wall, wo ueber der
    // Kante WIEDER Wasser steht und der Hop deshalb ausbleiben MUSS).
    fn world_with_pool_and_shore(shore_x: i32, water_top: i32) -> World {
        let water_state = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
            .expect("water-Quellblock-State nicht in der Registry gefunden");

        let mut w = World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    let section_base_y = world::MIN_Y + i * 16;
                    for wy in 0..=water_top {
                        if wy < section_base_y || wy >= section_base_y + 16 {
                            continue;
                        }
                        let ly = (wy - section_base_y) as usize;
                        for lx in 0..16usize {
                            let world_x = cx * 16 + lx as i32;
                            // y=0 ist ueberall Beckenboden, darueber Ufer-Fels bzw. Wasser.
                            let state = if wy == 0 || world_x >= shore_x { 1 } else { water_state };
                            for lz in 0..16usize {
                                states[(ly * 16 + lz) * 16 + lx] = state;
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Positive Seite von resetVerticalVelocityInFluid: schwimmt man am Beckenrand gegen das
    // Ufer und ist ueber der Kante LUFT (kein Block, kein Fluid), setzt Vanilla vel.y hart auf
    // 0.3F und man klettert heraus -- ohne Sprung-Cooldown, weshalb sich das bei anhaltendem
    // Wandkontakt jeden Tick wiederholen kann ("water hopping"). Der Kick ist exakt der
    // double-Wert des FLOATS 0.3F; 0.3 waere um 1.2e-8 daneben und driftet ueber eine
    // Hop-Serie von Grims Prediction weg.
    #[test]
    fn schwimmen_gegen_das_ufer_klettert_heraus() {
        let shore_x = 10;
        let water_top = 9; // Wasseroberflaeche bei y=10.0, darueber Luft
        let world = world_with_pool_and_shore(shore_x, water_top);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(5.0, 8.0, 5.0));
        let mut input = idle_input(0.0); // yaw 0 = Blick nach +Z, +X kommt ueber strafe
        input.strafe = 1.0;
        input.jump = true; // hochschwimmen, bis die Fuesse die Kante erreichen

        let mut saw_kick = false;
        for _ in 0..200 {
            p.tick(&input, &world, &shapes);
            if p.vel.y == 0.3f32 as f64 {
                saw_kick = true;
            }
            if p.pos.x >= shore_x as f64 && p.on_ground {
                break;
            }
        }
        assert!(saw_kick, "am Ufer mit Luft darüber MUSS der 0.3F-Kick feuern");
        assert!(p.pos.y > water_top as f64, "der Hop muss ueber die Kante bringen, nicht darunter haengen bleiben: y={}", p.pos.y);
    }

    // Regressionstest: Wasser fehlte in der
    // begehbaren Blockliste, der Spieler stand oben auf dem Wasserspiegel wie auf festem
    // Boden. Jetzt muss er einsinken -- Fuesse deutlich unter der Wasseroberflaeche.
    #[test]
    fn spieler_sinkt_ins_wasser_statt_obendrauf_zu_stehen() {
        let world = world_with_floor_and_water(0, 1, 10);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 15.0, 8.0));
        for _ in 0..60 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(p.pos.y < 9.0, "sollte im Wasser eingesunken sein, nicht obendrauf stehen: y={}", p.pos.y);
    }

    // Wie world_with_floor_and_water(), aber mit Seegras statt echten Wasser-Quellbloecken
    // -- Seegras traegt in Vanilla keine "waterlogged"-Property (existiert nie trocken),
    // muss fuer die Schwimmphysik aber trotzdem als Wasser zaehlen (s. world::fluid::
    // always_wet). Regressionstest: im Seegras lief man,
    // als sei da kein Wasser.
    fn world_with_floor_and_seagrass(floor_y: i32, grass_bottom: i32, grass_top: i32) -> World {
        let seagrass_state = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("seagrass"))
            .expect("seagrass-State nicht in der Registry gefunden");

        let mut w = World::new();
        let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
        let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == floor_section {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                            }
                        }
                    }
                    let section_base_y = world::MIN_Y + i * 16;
                    for wy in grass_bottom..=grass_top {
                        if wy >= section_base_y && wy < section_base_y + 16 {
                            let ly = wy - section_base_y;
                            for x in 0..16usize {
                                for z in 0..16usize {
                                    states[(ly as usize * 16 + z) * 16 + x] = seagrass_state;
                                }
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    #[test]
    fn spieler_sinkt_im_seegras_statt_frei_zu_fallen() {
        let world = world_with_floor_and_seagrass(-50, -10, 40);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 30.0, 8.0));
        for _ in 0..40 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(
            p.vel.y.abs() < GRAVITY,
            "im Seegras sollte genauso gebremst gesunken werden wie im Wasser, nicht frei gefallen: vel.y={}",
            p.vel.y
        );
    }

    // travelInWater(): Gravitation im Wasser ist nur d/16 statt d -- deutlich langsameres
    // Sinken als der freie Fall. Vergleicht die Sinkgeschwindigkeit nach Erreichen des
    // eingeschwungenen Zustands gegen die volle Luft-Gravitation.
    #[test]
    fn sinken_im_wasser_ist_viel_langsamer_als_freier_fall() {
        let world = world_with_floor_and_water(-50, -10, 40);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 30.0, 8.0));
        for _ in 0..40 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(
            p.vel.y.abs() < GRAVITY,
            "Sink-Geschwindigkeit im Wasser sollte deutlich unter der vollen Gravitation liegen: vel.y={}",
            p.vel.y
        );
    }

    // jumpInLiquid(): Space gedrueckt halten in tiefem Wasser muss nach oben schwimmen
    // lassen (+0.04/Tick, kein Cooldown) statt wie an Land nur einmal abzuspringen.
    #[test]
    fn hochschwimmen_mit_gehaltenem_sprung_in_tiefem_wasser() {
        let world = world_with_floor_and_water(-50, -10, 40);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
        // Erst ein paar Ticks ohne Eingabe, damit der Spieler wirklich mitten im Wasser
        // treibt (nicht gerade erst eingetaucht).
        for _ in 0..10 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        let before = p.pos.y;
        let mut input = idle_input(0.0);
        input.jump = true;
        for _ in 0..20 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.pos.y > before + 1.0, "sollte durch Hochschwimmen deutlich aufgestiegen sein: vorher={before}, nachher={}", p.pos.y);
    }

    // Regressionstest: im Wasser Jump+Sneak flaggte heftig.
    // swimUpward (+0.04, Jump) und knockDownwards (-0.04, Sneak) sind in Vanilla zwei getrennte
    // Checks und heben sich bei gleichzeitigem Druecken zu netto 0 auf. Vorher verschluckte ein
    // if/else if den Sneak-Kick -> netto +0.04/Tick -> die Y-Position lief von Grims Simulation
    // weg. Ergo muss Jump+Sneak vertikal IDENTISCH zu voelligem Nichtstun (Idle) treiben.
    #[test]
    fn jump_und_sneak_im_wasser_heben_sich_auf() {
        let world = world_with_floor_and_water(-50, -10, 40);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut idle = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
        let mut jump_sneak = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
        // Erst frei treiben lassen (nicht gerade erst eingetaucht).
        for _ in 0..10 {
            idle.tick(&idle_input(0.0), &world, &shapes);
            jump_sneak.tick(&idle_input(0.0), &world, &shapes);
        }
        let mut js_input = idle_input(0.0);
        js_input.jump = true;
        js_input.sneak = true;
        for _ in 0..30 {
            idle.tick(&idle_input(0.0), &world, &shapes);
            jump_sneak.tick(&js_input, &world, &shapes);
        }
        // Beide Kicks heben sich exakt auf -> vertikal identisch zu Idle (kein Aufstieg).
        assert!(
            (idle.pos.y - jump_sneak.pos.y).abs() < 1.0e-6,
            "Jump+Sneak muss vertikal exakt wie Idle treiben (Kicks heben sich auf): idle={}, jump+sneak={}",
            idle.pos.y,
            jump_sneak.pos.y
        );
    }

    #[test]
    fn zaun_bleibt_auch_beim_springen_undurchdringlich() {
        let fence_state = (0..world::block_registry::state_count() as u32)
            .find(|&s| {
                world::block_name(s) == Some("oak_fence")
                    && world::state_props::property(s, "north") == Some("false")
                    && world::state_props::property(s, "south") == Some("false")
                    && world::state_props::property(s, "east") == Some("false")
                    && world::state_props::property(s, "west") == Some("false")
                    && world::state_props::property(s, "waterlogged") == Some("false")
            })
            .expect("oak_fence-State nicht in der Registry gefunden");

        let mut world = World::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == 4 {
                        // Index innerhalb der Section: (y * 16 + z) * 16 + x.
                        let index = |x: usize, y: usize, z: usize| (y * 16 + z) * 16 + x;
                        // Boden bei Welt-Y=0 (Section 4 beginnt bei MIN_Y + 4*16 = 0).
                        for x in 0..16 {
                            for z in 0..16 {
                                states[index(x, 0, z)] = 1;
                            }
                        }
                        // Einzelner Zaunpfosten bei Welt (8,1,8) -- lokal x=8,z=8, y=1.
                        if cx == 0 && cz == 0 {
                            states[index(8, 1, 8)] = fence_state;
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                world.insert_column(cx, cz, sections);
            }
        }
        let shapes = world::BlockShapes::all_full_cubes();

        // Mittig durch die Pfosten-Zelle (Welt-Z=8.5), Anlauf in +X.
        let mut p = PlayerPhysics::new(DVec3::new(5.0, 1.0, 8.5));
        let mut input = idle_input(0.0);
        input.strafe = 1.0;
        input.sprint_held = true;
        for tick in 0..300 {
            input.jump = tick % 5 == 0; // alle 0.25s erneut anspringen
            p.tick(&input, &world, &shapes);
        }
        assert!(p.pos.x < 8.375, "Spieler haette am Zaunpfosten (x=8.375) haengen bleiben muessen, ist aber bei x={}", p.pos.x);
    }

    // Regressionstest fuer isSwimming() (s. PlayerLikeEntity.POSE_DIMENSIONS-Kommentar):
    // Einstieg braucht sowohl Sprinten als auch VOLLSTAENDIG untergetaucht zu sein -- bloss
    // an der Wasseroberflaeche zu sprinten (Kopf noch in der Luft) reicht NICHT.
    #[test]
    fn schwimm_pose_braucht_sprinten_und_vollstaendiges_untertauchen() {
        // Wasser von y=1 bis y=10 -- Spieler bei y=1 (Fuesse), Augenhoehe 1.62 -> Kopf bei
        // y=2.62, also klar unter Wasser (bis y=10).
        let world = world_with_floor_and_water(0, 1, 10);
        let shapes = world::BlockShapes::all_full_cubes();

        // Vollstaendig untergetaucht ist NICHT "isPartlyTouchingWater()" (s. Kommentar in
        // tick()) -- ein frischer Sprint darf hier also direkt ab Tick 0 starten.
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        // Zwei Ticks: Tick 1 startet den Sprint, die Schwimm-Pose (updateSwimming aus baseTick,
        // s. tick()) greift erst EINEN Tick nach dem Sprint-Start -- also ab Tick 2.
        p.tick(&input, &world, &shapes);
        p.tick(&input, &world, &shapes);
        assert!(p.swimming, "sprintend UND vollständig untergetaucht muss die Schwimm-Pose auslösen");

        // Ohne Sprinten (nur im Wasser) keine Schwimm-Pose.
        let mut p2 = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let idle = idle_input(0.0);
        p2.tick(&idle, &world, &shapes);
        assert!(!p2.swimming, "ohne Sprinten keine Schwimm-Pose, nur normales Plantschen");
    }

    // Regressionstest: auf der Wasseroberflaeche liess sich sprinten, in Vanilla nicht. ClientPlayerEntity.canStartSprinting()/canSprint() lassen einen
    // NEUEN Sprint nicht zu, waehrend man (auch nur oberflaechlich) Wasser beruehrt.
    #[test]
    fn kein_neuer_sprint_an_der_wasseroberflaeche() {
        // Duennes Wasser nur bei y=1 -- Spieler steht mit den Fuessen genau darin, wie beim
        // Stehen an der Oberflaeche.
        let world = world_with_floor_and_water(0, 1, 1);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        for _ in 0..10 {
            p.tick(&input, &world, &shapes);
            assert!(!p.sprinting, "ein NEUER Sprint darf nicht beginnen, während man Wasser berührt");
        }
    }

    // Regressionstest fuer den NACHFOLGENDEN Bug-Report ("kann sprinten an der
    // Wasseroberflaeche, wenn Sprintlock an ist"): ausserhalb der Schwimm-Pose gilt fuer einen
    // bereits laufenden Sprint shouldStopSprinting() -> canSprint(false), NICHT das
    // permissivere shouldStopSwimSprinting() -- ein an Land begonnener Sprint muss also sofort
    // stoppen, sobald man (ohne in die Schwimm-Pose zu kippen) auch nur oberflaechlich Wasser
    // beruehrt. Mit Sprint-Umschalten (sprint_held bleibt dauerhaft true, kein Loslassen an der
    // Wasserkante wie bei gehaltener Taste) fiel das zuvor bei praktisch jedem Wasserkontakt auf.
    #[test]
    fn laufender_sprint_stoppt_an_der_wasseroberflaeche_ausserhalb_der_schwimm_pose() {
        let world = world_with_floor_and_water(0, 1, 1);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut already = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        already.sprinting = true; // schon VOR dem Wasserkontakt am Sprinten, an Land (nicht Schwimm-Pose)
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        already.tick(&input, &world, &shapes);
        assert!(!already.sprinting, "ein an Land laufender Sprint muss stoppen, sobald man (außerhalb der Schwimm-Pose) Wasser berührt");
    }

    // Regressionstest fuer den NACHFOLGENDEN Bug-Report ("kann nicht mehr sprint-schwimmen,
    // auch wenn der Kopf im Wasser ist"): isPartlyTouchingWater() (Entity.java) ist NICHT
    // "beruehrt Wasser", sondern explizit "beruehrt Wasser UND ist NICHT untergetaucht" --
    // vollstaendig untergetaucht (Kopf unter Wasser) darf ein frischer Sprint jederzeit
    // starten, nur die reine Oberflaechenberuehrung (s. Test oben) blockiert den Start.
    #[test]
    fn neuer_sprint_darf_vollstaendig_untergetaucht_starten() {
        let world = world_with_floor_and_water(0, 1, 10);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        assert!(!p.sprinting);
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting, "vollständig untergetaucht muss ein frischer Sprint sofort starten dürfen");
    }

    // Regressionstest fuer den Sprint-Flacker am Wasser->Luft-Austritt (Grim consoledebug 01:29,
    // O~0.0137): beim Hochschwimmen und Austauchen darf der Sprint NICHT fuer einen Tick abfallen
    // und sofort wieder anspringen (das schickte ein spurious STOP_SPRINTING und desynchronisierte
    // Grims Sprint-Zustand). Sobald man beim Sprint-Schwimmen austaucht, muss der Sprint als
    // Land-Sprint nahtlos weiterlaufen (Vanilla: isSwimming schon aus -> shouldStopSprinting-Zweig).
    #[test]
    fn sprint_flackert_nicht_beim_austauchen() {
        let world = world_with_floor_and_water(-30, -29, 0); // Oberflaeche y=1, Luft darueber
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, -6.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        input.jump = true;
        input.look_dir_y = 0.7; // Blick nach oben -> schwimmt hoch und taucht aus
        let mut states = Vec::new();
        for _ in 0..40 {
            p.tick(&input, &world, &shapes);
            states.push(p.sprinting);
        }
        // Sobald Sprint einmal lief, darf es keinen true->false->true-Flacker geben, solange
        // vorwaerts + sprint_held gehalten wird und man aus dem Wasser austaucht.
        let mut flicker = false;
        for w in states.windows(3) {
            if w[0] && !w[1] && w[2] {
                flicker = true;
            }
        }
        assert!(!flicker, "Sprint darf am Wasser->Luft-Austritt nicht für einen Tick abfallen (Flacker): {states:?}");
    }

    // Regressionstest fuer den 0.0178-Z-Offset-Flag beim Sprint-Rausschwimmen (test 13,
    // algebraisch aus Grims P/A hergeleitet: A.z=0.20172798 = 0.026×0.98×1.0-Accel, P.z=0.18389193
    // = 0.026×0.98×0.3-Accel). Am Austritts-Tick ist der Spieler isCrawling() (Schwimm-Pose vom
    // ENDE des Vorticks -- updatePose laeuft in Vanilla am Tick-ENDE --, Fuesse aber schon aus dem
    // Wasser), Vanilla wendet deshalb den ×0.3-Bewegungsfaktor an, OHNE dass Sneak gehalten wird.
    // Geprueft ueber das direkte Verhaeltnis der Horizontal-Beschleunigung mit vs. ohne
    // Schwimm-Pose-Rest -- muss DEFAULT_SNEAKING_SPEED (0.3) sein.
    #[test]
    fn crawling_beim_rausschwimmen_verlangsamt_horizontal_um_030() {
        // Wasser tief unten, Spieler klar in der Luft darueber -> water_height dieses Ticks = None
        // (Luft-Dispatch), waehrend was_swimming aus dem Vortick-Pose-Rest kommt.
        let world = world_with_floor_and_water(-30, -29, -20);
        let shapes = world::BlockShapes::all_full_cubes();

        let run = |swim_pose_rest: bool| -> f64 {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 0.0, 8.0));
            p.sprinting = true; // Sprint laeuft bereits (beeinflusst beide Laeufe gleich)
            p.swimming = swim_pose_rest; // Pose-Rest vom ENDE des letzten Ticks (isInSwimmingPose)
            let mut input = idle_input(0.0);
            input.forward = 1.0;
            input.sprint_held = true;
            assert!(!input.sneak, "Test setzt voraus, dass Sneak NICHT gehalten wird");
            let vz_before = p.vel.z;
            p.tick(&input, &world, &shapes);
            p.vel.z - vz_before
        };

        let dz_crawling = run(true);
        let dz_normal = run(false);
        assert!(dz_normal.abs() > 1e-9, "Kontroll-Lauf muss überhaupt beschleunigen: {dz_normal}");
        let ratio = dz_crawling / dz_normal;
        assert!(
            (ratio - DEFAULT_SNEAKING_SPEED).abs() < 1e-6,
            "Crawling-Austritt muss die Horizontal-Accel um DEFAULT_SNEAKING_SPEED (={DEFAULT_SNEAKING_SPEED}) \
             daempfen (isCrawling->shouldSlowDown), Verhaeltnis war {ratio} \
             (crawling={dz_crawling}, normal={dz_normal})"
        );
    }

    // Regressionstest fuer den Wasser-Setback beim schleichenden Reinlaufen (Grim consoledebug
    // 00:43, P.y=-0.19992 Crouch-Sink vs A.y=-0.04 Schwimmen, O~0.16): Vanillas updateSwimming
    // laeuft in baseTick() VOR der Sprint-Entscheidung/travel() (LivingEntity.tick 2633 vs 2672),
    // die Schwimm-Pose greift also erst EINEN Tick NACH dem Sprint-Start. Auf dem Sprint-Start-
    // Tick muss darum noch Crouch-Physik laufen (raw_swimming/swimming noch aus), erst der
    // naechste Tick schwimmt.
    #[test]
    fn schwimm_pose_laggt_einen_tick_nach_dem_sprint_start() {
        let world = world_with_floor_and_water(0, 1, 10);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;

        // Tick 1: Sprint startet (untergetaucht erlaubt), aber Schwimm-Pose noch NICHT --
        // updateSwimming sah isSprinting vom Vortick (false).
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting, "Sprint muss untergetaucht sofort starten");
        assert!(!p.swimming, "auf dem Sprint-Start-Tick darf die Schwimm-Pose noch NICHT aktiv sein (Ein-Tick-Lag)");

        // Tick 2: jetzt sah updateSwimming isSprinting=true -> Schwimm-Pose an.
        p.tick(&input, &world, &shapes);
        assert!(p.swimming, "einen Tick nach dem Sprint-Start muss die Schwimm-Pose greifen");
    }

    // Regressionstest: im Wasser fuehrt Sneak + W in Vanilla in den Schwimm-Modus. canStartSprinting() hebt die Sneak-Sperre auf, sobald man vollstaendig
    // untergetaucht ist (`!shouldSlowDown() || isSubmergedInWater()`). Trotz gedruecktem Sneak
    // muss ein frischer Sprint (und damit die Schwimm-Pose) untergetaucht also starten duerfen.
    #[test]
    fn untergetaucht_startet_sprint_auch_mit_gedruecktem_sneak() {
        let world = world_with_floor_and_water(0, 1, 10);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        input.sneak = true;
        // Ein paar Ticks, damit der Spieler sicher untergetaucht ist und die Schwimm-Pose greift.
        for _ in 0..5 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.sprinting, "untergetaucht muss der Sprint trotz Sneak starten (canStartSprinting-Ausnahme)");
        assert!(p.swimming, "mit laufendem Sprint + untergetaucht muss die Schwimm-Pose aktiv sein");
    }

    // Sobald die Schwimm-Pose einmal aktiv ist, reicht zum WEITERSCHWIMMEN blosse
    // Wasserberuehrung (nicht mehr volles Untertauchen) -- man bleibt beim Auftauchen an der
    // Oberflaeche also in der Pose, solange man weiter sprintet (s. updateSwimming()).
    #[test]
    fn bleibt_beim_auftauchen_in_der_schwimm_pose() {
        // Duennes Wasser nur bei y=1 (Fuesse gerade so drin, Kopf bei y=2.62 klar drueber --
        // "aufgetaucht", aber die Fuesse beruehren noch Wasser).
        let world = world_with_floor_and_water(0, 1, 1);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        p.swimming = true; // simuliert "war schon am Schwimmen, bevor aufgetaucht wurde"
        p.sprinting = true; // s. Kommentar oben: ein NEUER Sprint startet nicht im Wasser
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        p.tick(&input, &world, &shapes);
        assert!(p.swimming, "an der Oberfläche weitersprintend muss die Pose erhalten bleiben");
    }

    // Regressionstest fuer PlayerLikeEntity.POSE_DIMENSIONS: Schwimm-Pose ist 0.6x0.6 statt
    // 0.6x1.8, Augenhoehe 0.4 statt 1.62 -- NICHT geraten, sondern aus dem lokalen
    // Yarn-1.21.11-Jar nachgesehen (PlayerEntity erbt von PlayerLikeEntity,
    // das die Pose-Tabelle traegt -- eine fruehere Suche nur in PlayerEntity/LivingEntity hat
    // sie deshalb uebersehen).
    #[test]
    fn schwimm_pose_hat_die_richtige_kollisionsgroesse() {
        let world = world_with_floor_and_water(0, 1, 10);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        // Ein-Tick-Lag der Schwimm-Pose nach dem Sprint-Start (s. tick()) -> zwei Ticks.
        p.tick(&input, &world, &shapes);
        p.tick(&input, &world, &shapes);
        assert!(p.swimming);
        assert!((p.eye_pos().y - p.pos.y - 0.4).abs() < 1.0e-5, "Augenhöhe beim Schwimmen muss 0.4 sein, war {}", p.eye_pos().y - p.pos.y);
    }

    // Regressionstest fuer PlayerEntity.travel()s Blickrichtungs-Zug (s. apply_swim_pitch_pull-
    // Kommentar): beim Sprint-Schwimmen mit Wasser rundum steuert allein die Blickrichtung die
    // Vertikalgeschwindigkeit -- nach unten schauen muss abwaerts ziehen, nach oben schauen
    // (bei noch Wasser ueber dem Kopf) aufwaerts, jeweils OHNE Springen/Schleichen zu halten.
    #[test]
    fn sprint_schwimmen_folgt_der_blickrichtung() {
        let world = world_with_floor_and_water(-10, -9, 20); // tiefes, allseitig umschlossenes Wasser
        let shapes = world::BlockShapes::all_full_cubes();

        let mut down = PlayerPhysics::new(DVec3::new(8.0, 0.0, 8.0));
        down.swimming = true;
        let mut input_down = idle_input(0.0);
        input_down.forward = 1.0;
        input_down.sprint_held = true;
        input_down.look_dir_y = -1.0; // senkrecht nach unten geschaut
        for _ in 0..5 {
            down.tick(&input_down, &world, &shapes);
        }
        assert!(down.vel.y < -0.05, "beim Sprint-Schwimmen nach unten geschaut muss abwaerts gezogen werden, vel.y={}", down.vel.y);

        let mut up = PlayerPhysics::new(DVec3::new(8.0, 0.0, 8.0));
        up.swimming = true;
        let mut input_up = idle_input(0.0);
        input_up.forward = 1.0;
        input_up.sprint_held = true;
        input_up.look_dir_y = 1.0; // senkrecht nach oben geschaut, aber noch Wasser ueber dem Kopf
        for _ in 0..5 {
            up.tick(&input_up, &world, &shapes);
        }
        assert!(
            up.vel.y > 0.02,
            "beim Sprint-Schwimmen nach oben geschaut (mit Wasser ueber dem Kopf) muss aufwaerts gezogen werden, vel.y={}",
            up.vel.y
        );
    }

    // Regressionstest fuer "wenn ich beim Schwimmen meinen Kopf hitte buggt es voll rum":
    // der Fluessigkeits-BERUEHRUNGS-Scan lief frueher immer mit der hartkodierten Stehhoehe
    // (1.8) statt der TATSAECHLICHEN aktuellen Boxhoehe -- unter Dauerdruck gegen eine knapp
    // ueber der Schwimm-Pose (0.6) liegende Unterwasser-Decke soll die Schwimm-Pose stabil
    // bleiben, nicht zwischen 0.6 und 1.8 hin- und herkippen.
    fn thin_water_pocket(floor_y: i32, water_y: i32, ceiling_y: i32) -> World {
        let water_state = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
            .expect("water state");
        let mut w = World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    let section_base_y = world::MIN_Y + i * 16;
                    for (y, state) in [(floor_y, 1u32), (water_y, water_state), (ceiling_y, 1u32)] {
                        if y >= section_base_y && y < section_base_y + 16 {
                            let ly = y - section_base_y;
                            for x in 0..16usize {
                                for z in 0..16usize {
                                    states[(ly as usize * 16 + z) * 16 + x] = state;
                                }
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Die Eis-/Wasserplattform von anticheat-test.com: Eis bei y=90 (Oberkante 91.0),
    // darueber EINE Schicht Wasser bei y=91. Damit die Stroemung eine Richtung hat, sind die
    // Zellen ab x=9 fliessendes Wasser (level=1) statt Quelle -- das Gefaelle zeigt nach +x,
    // der Schub ist damit die volle 0.014 in +x, genau wie im echten Mitschnitt gemessen.
    fn eis_wasser_plattform() -> World {
        let quelle = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
            .expect("water level=0");
        let fliessend = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("1"))
            .expect("water level=1");
        let eis = (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("ice"))
            .expect("ice");
        let mut w = World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    let section_base_y = world::MIN_Y + i * 16;
                    for (y, welt_x_quelle) in [(90, None), (91, Some(9))] {
                        if y < section_base_y || y >= section_base_y + 16 {
                            continue;
                        }
                        let ly = (y - section_base_y) as usize;
                        for x in 0..16usize {
                            for z in 0..16usize {
                                let welt_x = cx * 16 + x as i32;
                                let state = match welt_x_quelle {
                                    None => eis,
                                    Some(grenze) if welt_x >= grenze => fliessend,
                                    Some(_) => quelle,
                                };
                                states[(ly * 16 + z) * 16 + x] = state;
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Gemessen mit pktproxy --mitm auf modern.anticheat-test.com: Vanilla und Coral Client fallen
    // von y=100 auf die Plattform. Der Fall ist auf vier Nachkommastellen deckungsgleich, aber
    // im Landetick bewegte sich Vanilla um 0.0267 nach +x und wir nur um 0.0140.
    //
    // Ursache war der fehlende Nachschlag in Entity.move()/LivingEntity.checkFallDamage
    // (Wasserschub nachholen, solange isInWater() noch falsch ist).
    // Im Tick des Eintauchens war isInWater() noch falsch (aus baseTick, vor der Bewegung),
    // der Schub kam also erst nach move() -- seitlich wirkungslos, aber noch mit g = 0.91
    // gedaempft und damit im naechsten Tick wirksam. 0.014 × 0.91 + 0.014 = 0.02674.
    #[test]
    fn landehoehe_ist_exakt_die_blockoberkante() {
        let world = eis_wasser_plattform();
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 100.0, 8.5));
        let input = idle_input(0.0);
        for _ in 0..40 {
            p.tick(&input, &world, &shapes);
            if p.on_ground {
                break;
            }
        }
        // Sonar vergleicht die Landehoehe EXAKT:
        //   collisionOffsetY = (PLATFORM_Y_POSITION + blockHeight) - y; if (collisionOffsetY != 0) fail
        // Ein Ergebnis von 91.00000000000001 faellt dort durch, sieht im Log aber wie 91.0000 aus.
        assert_eq!(p.pos.y.to_bits(), 91.0f64.to_bits(), "Landehöhe ist {:.17}, nicht exakt 91.0", p.pos.y);
    }

    #[test]
    fn eintauchen_holt_den_stroemungsschub_noch_im_falltick_nach() {
        let world = eis_wasser_plattform();
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 100.0, 8.5));
        let input = idle_input(0.0);

        let mut landetick_dx = None;
        let mut vorher_x = p.pos.x;
        for _ in 0..40 {
            let war_in_der_luft = !p.on_ground;
            p.tick(&input, &world, &shapes);
            if war_in_der_luft && p.on_ground {
                landetick_dx = Some(p.pos.x - vorher_x);
                break;
            }
            vorher_x = p.pos.x;
        }

        let dx = landetick_dx.expect("der Spieler muss auf der Plattform landen");
        assert!(
            (p.pos.y - 91.0).abs() < 1e-9,
            "die Eisoberkante liegt bei y=91.0, gelandet wurde bei {}",
            p.pos.y
        );
        assert!(
            (dx - 0.02674).abs() < 1e-6,
            "im Landetick erwartet Vanilla 0.014 × 0.91 + 0.014 = 0.02674, gemessen {dx}"
        );

        // Danach nur noch ein Schub pro Tick: v <- 0.8 * v + 0.014. Der Nachschlag darf sich
        // NICHT wiederholen, sobald man schon im Wasser steht.
        let mut erwartet = dx;
        for tick in 0..6 {
            let vorher = p.pos.x;
            p.tick(&input, &world, &shapes);
            erwartet = 0.8 * erwartet + 0.014;
            let gemessen = p.pos.x - vorher;
            assert!(
                (gemessen - erwartet).abs() < 1e-6,
                "Tick {tick} nach der Landung: erwartet {erwartet}, gemessen {gemessen}"
            );
        }
    }

    #[test]
    fn schwimm_pose_flackert_nicht_unter_niedriger_decke() {
        let world = thin_water_pocket(0, 1, 2); // Wasser NUR bei y=1..2, Decke bei y=2..3
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        p.swimming = true;
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        input.look_dir_y = 1.0; // staendig nach oben gegen die Decke druecken

        let mut flips = 0;
        let mut was_swimming = true;
        for _ in 0..100 {
            p.tick(&input, &world, &shapes);
            if p.swimming != was_swimming {
                flips += 1;
            }
            was_swimming = p.swimming;
        }
        assert_eq!(flips, 0, "Schwimm-Pose sollte unter Dauerdruck gegen eine niedrige Decke NICHT flackern");
    }

    // Regressionstest: an der Decke geschwommen darf der Kopf beim Sprint-Ende nicht in
    // die Decke clippen. Ohne den
    // canChangeIntoPose()-Fitting-Fallback (s. pose_fits-Kommentar) wuerde raw_swimming beim
    // Loslassen von Sprint sofort false werden und current_height() auf Stehhoehe (1.8)
    // zurueckspringen -- die dann sofort in die Decke ragt (Kollisionsbox ueberlappt
    // Solides). Mit dem Fallback muss die Pose zwangsweise bei SWIMMING (0.6) bleiben.
    #[test]
    fn kein_decken_clipping_beim_aufhoeren_zu_sprinten() {
        let world = thin_water_pocket(0, 1, 2); // Wasser NUR bei y=1..2, Decke bei y=2..3
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        input.look_dir_y = 1.0; // gegen die Decke schwimmen
        for _ in 0..30 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.swimming, "sollte vor dem Loslassen von Sprint in der Schwimm-Pose sein");

        // Sprint loslassen, weiter gegen die Decke gedrueckt -- die Pose MUSS wegen fehlenden
        // Platzes bei SWIMMING bleiben, nicht auf Stehhoehe zurueckspringen.
        input.sprint_held = false;
        for _ in 0..10 {
            p.tick(&input, &world, &shapes);
            assert!(p.swimming, "Pose sollte trotz losgelassenem Sprint bei SWIMMING bleiben (kein Platz zum Stehen)");
            assert!(
                pose_fits(p.pos, p.current_height(), &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default()),
                "die tatsächlich benutzte Hitbox darf nie in die Decke clippen"
            );
        }
    }
    // ---------------------------------------------------------------------------------
    // Seelensand / Honig / Schleim -- die drei Bloecke mit eigener Bewegungsmechanik.
    // Alle Zahlen aus SoulSandBlock/HoneyBlock/SlimeBlock + Blocks (1.21.11).
    // ---------------------------------------------------------------------------------

    fn state_of_block(name: &str) -> u32 {
        (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap_or_else(|| panic!("{name} unbekannt"))
    }

    // Wie world_with_floor, aber der Boden besteht aus einem benannten Block.
    fn world_with_floor_of(floor_y: i32, name: &str) -> World {
        let state = state_of_block(name);
        let mut w = World::new();
        let section_index = (floor_y - world::MIN_Y).div_euclid(16);
        let local_y = (floor_y - world::MIN_Y).rem_euclid(16);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == section_index {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((local_y * 16 + z) * 16 + x) as usize] = state;
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Nach Osten laufen (Strafe, damit die Blickrichtung egal bleibt) -- step_tests hat ein
    // gleichnamiges Gegenstueck, das in diesem Modul aber nicht sichtbar ist.
    fn lauf_input() -> Input {
        let mut i = idle_input(0.0);
        i.strafe = 1.0;
        i
    }

    // Auf einem Schleimblock kommt man NICHT durch Hinfallen zur Ruhe -- jeder Aufprall
    // springt zurueck. Deshalb hier von vornherein aufsetzen und nur ausschwingen lassen,
    // bis die Restbewegung unter der Vanilla-Nullungsschwelle liegt.
    fn ruht_auf(world: &World, shapes: &world::BlockShapes, start: DVec3) -> PlayerPhysics {
        let mut p = PlayerPhysics::new(start);
        p.refresh_on_ground(world, shapes);
        for _ in 0..30 {
            p.tick(&idle_input(0.0), world, shapes);
        }
        assert!(p.on_ground && p.vel.y.abs() < 0.1, "Testaufbau: sollte ruhig aufliegen, vel.y={}", p.vel.y);
        p
    }

    fn steht_still_auf(world: &World, shapes: &world::BlockShapes, start: DVec3) -> PlayerPhysics {
        let mut p = PlayerPhysics::new(start);
        for _ in 0..10 {
            p.tick(&idle_input(0.0), world, shapes);
        }
        assert!(p.on_ground, "Testaufbau: sollte am Boden stehen, y={}", p.pos.y);
        p
    }

    // SoulSandBlock.getCollisionShape: Block.createColumnShape(16, 0, 14) -- die Oberkante
    // liegt bei 14/16, man sinkt also 2 Pixel ein. Nur die KOLLISION ist niedriger, der
    // Umriss bleibt der Vollwuerfel (sonst zielte man daneben).
    #[test]
    fn seelensand_laesst_zwei_pixel_einsinken() {
        let world = world_with_floor_of(0, "soul_sand");
        let shapes = world::BlockShapes::all_full_cubes();
        let p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
        assert!((p.pos.y - 0.875).abs() < 1.0e-6, "Fuesse sollten auf 14/16 stehen, y={}", p.pos.y);

        let sand = state_of_block("soul_sand");
        let kollision = world::collision_boxes(sand, &shapes);
        assert_eq!(kollision.len(), 1);
        assert!((kollision[0].max[1] - 0.875).abs() < 1.0e-6, "Kollisionshöhe 14/16");
        let umriss = world::outline_boxes(sand, &shapes);
        assert!((umriss[0].max[1] - 1.0).abs() < 1.0e-6, "Umriss bleibt der Vollwürfel");
        // getCameraCollisionShape ist bei Seelensand ausdruecklich wieder der Vollwuerfel.
        let kamera = world::camera_collision_boxes(sand, &shapes);
        assert!((kamera[0].max[1] - 1.0).abs() < 1.0e-6, "Kameraform ist der Vollwürfel");
    }

    // velocityMultiplier(0.4F) auf Seelensand UND Honig: die Endgeschwindigkeit im Gehen muss
    // deutlich unter der auf Stein liegen. Der Faktor wirkt pro Tick auf die Geschwindigkeit,
    // die Eingabe baut sie nur mit dem kleinen Beschleunigungsbetrag wieder auf.
    #[test]
    fn seelensand_und_honig_bremsen_das_gehen() {
        let shapes = world::BlockShapes::all_full_cubes();
        let tempo = |block: &str| {
            let world = world_with_floor_of(0, block);
            let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
            for _ in 0..60 {
                p.tick(&lauf_input(), &world, &shapes);
            }
            (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt()
        };
        let stein = tempo("stone");
        let sand = tempo("soul_sand");
        let honig = tempo("honey_block");
        assert!(sand < stein * 0.6, "Seelensand muss deutlich bremsen: {sand} gegen {stein}");
        assert!(honig < stein * 0.6, "Honig muss deutlich bremsen: {honig} gegen {stein}");
    }

    // Sprungkraft II: 0.42 + 0.1 * 2 in float, dann wie jeder Sprung Schwerkraft und Daempfung.
    #[test]
    fn sprungkraft_erhoeht_den_absprung() {
        let shapes = world::BlockShapes::all_full_cubes();
        let world = world_with_floor_of(0, "stone");
        let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
        let mut input = idle_input(0.0);
        input.jump = true;
        input.jump_boost = Some(1);
        p.tick(&input, &world, &shapes);
        let erwartet = ((0.42f32 + 0.1f32 * 2.0) as f64 - GRAVITY) * VERTICAL_DRAG;
        assert!((p.vel.y - erwartet).abs() < 1.0e-9, "{} statt {erwartet}", p.vel.y);
    }

    // Sanfter Fall: fallend nur 0.01 Schwerkraft -- die Endgeschwindigkeit liegt bei
    // 0.01*0.98/0.02 = 0.49 statt ~3.9. Nach oben wirkt die volle Schwerkraft.
    #[test]
    fn sanfter_fall_bremst_nur_das_fallen() {
        let shapes = world::BlockShapes::all_full_cubes();
        let world = world_with_floor(0);
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 200.0, 8.0));
        let mut input = idle_input(0.0);
        input.slow_falling = true;
        for _ in 0..200 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.vel.y < 0.0 && p.vel.y > -0.5, "Endgeschwindigkeit mit Sanftem Fall: {}", p.vel.y);
        // Aufwaerts: dieselbe Rechnung wie ohne Effekt.
        let mut hoch = PlayerPhysics::new(DVec3::new(8.0, 100.0, 8.0));
        hoch.vel.y = 0.5;
        hoch.tick(&input, &world, &shapes);
        assert!((hoch.vel.y - (0.5 - GRAVITY) * VERTICAL_DRAG).abs() < 1.0e-9, "{}", hoch.vel.y);
    }

    // Schwebeeffekt I: statt Schwerkraft zieht es auf 0.05 hoch -- man steigt.
    #[test]
    fn schwebeeffekt_hebt_an() {
        let shapes = world::BlockShapes::all_full_cubes();
        let world = world_with_floor(0);
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 50.0, 8.0));
        let mut input = idle_input(0.0);
        input.levitation = Some(0);
        let start = p.pos.y;
        for _ in 0..40 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.pos.y > start + 1.0, "Schwebeeffekt muss heben: {} -> {}", start, p.pos.y);
        assert!(p.vel.y > 0.0 && p.vel.y < 0.05, "{}", p.vel.y);
    }

    // jumpVelocityMultiplier(0.5F) beim Honigblock: (float)(0.42 * 0.5) = 0.21 statt 0.42.
    #[test]
    fn honig_halbiert_die_absprunggeschwindigkeit() {
        let shapes = world::BlockShapes::all_full_cubes();
        let absprung = |block: &str| {
            let world = world_with_floor_of(0, block);
            let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
            let mut input = idle_input(0.0);
            input.jump = true;
            p.tick(&input, &world, &shapes);
            p.vel.y
        };
        // Nach dem Sprungtick hat travelInAir Schwerkraft und Daempfung schon angewandt:
        // (v - 0.08) * 0.98.
        let erwartet = |v: f64| (v - GRAVITY) * VERTICAL_DRAG;
        assert!((absprung("stone") - erwartet(JUMP_VELOCITY)).abs() < 1.0e-9);
        assert!(
            (absprung("honey_block") - erwartet((JUMP_VELOCITY as f32 * 0.5) as f64)).abs() < 1.0e-9,
            "Honigsprung sollte halbe Absprunggeschwindigkeit haben, war {}",
            absprung("honey_block")
        );
    }

    // SlimeBlock.onEntityLand: die Standardfassung (Block.onEntityLand) nullt vel.y, der
    // Schleimblock dreht sie stattdessen um -- man springt mit voller Fallgeschwindigkeit
    // zurueck (Faktor 1.0 fuer LivingEntity).
    #[test]
    fn schleimblock_wirft_den_fall_zurueck() {
        let world = world_with_floor_of(0, "slime_block");
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 6.0, 8.0));
        let mut hoechster_rueckstoss: f64 = 0.0;
        for _ in 0..40 {
            p.tick(&idle_input(0.0), &world, &shapes);
            hoechster_rueckstoss = hoechster_rueckstoss.max(p.vel.y);
        }
        assert!(hoechster_rueckstoss > 0.3, "hätte zurückspringen müssen, größtes vel.y = {hoechster_rueckstoss}");
        assert!(p.pos.y > 1.5, "sollte nach dem Absprung wieder deutlich ueber dem Block sein, y={}", p.pos.y);
    }

    // bypassesLandingEffects() == isSneaking(): mit gehaltener Sneak-Taste faellt der
    // Schleimblock auf die Standardfassung zurueck -- kein Absprung, ruhige Landung.
    #[test]
    fn schleichen_verhindert_den_schleim_absprung() {
        let world = world_with_floor_of(0, "slime_block");
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 6.0, 8.0));
        let mut input = idle_input(0.0);
        input.sneak = true;
        for _ in 0..40 {
            p.tick(&input, &world, &shapes);
            assert!(p.vel.y <= 0.0, "mit Sneak darf es keinen Rueckstoss geben, vel.y={}", p.vel.y);
        }
        assert!(p.on_ground, "sollte still auf dem Schleim liegen bleiben");
    }

    // SlimeBlock.onSteppedOn: horizontal stark gebremst, solange |vel.y| < 0.1. Der Effekt
    // dreht das Vorzeichen der Reibung um -- Schleim ist mit 0.8 GLATTER als Stein (0.6), wer
    // darueber laeuft, muesste also eigentlich SCHNELLER werden. Genau umgekehrt kommt es
    // heraus, und nur onSteppedOn erklaert das.
    #[test]
    fn schleim_bremst_das_laufen_trotz_glatterer_reibung() {
        let shapes = world::BlockShapes::all_full_cubes();
        let tempo = |block: &str| {
            let world = world_with_floor_of(0, block);
            let mut p = ruht_auf(&world, &shapes, DVec3::new(8.0, 1.0, 8.0));
            for _ in 0..60 {
                p.tick(&lauf_input(), &world, &shapes);
            }
            (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt()
        };
        let schleim = tempo("slime_block");
        let stein = tempo("stone");
        assert!(schleim < stein * 0.7, "Schleim muss trotz 0.8er Reibung deutlich langsamer sein: {schleim} gegen {stein}");
    }

    // Und der Sneak-Zweig davon, ohne die Sneak-Verlangsamung der EINGABE als Stoergroesse:
    // eine mitgegebene Geschwindigkeit ohne Bewegungseingabe ausrollen lassen. Ohne Sneak
    // kommt zur Reibung (0.8 * 0.91) noch onSteppedOn dazu, mit Sneak nicht.
    #[test]
    fn schleichen_schaltet_die_schleim_bremse_ab() {
        let world = world_with_floor_of(0, "slime_block");
        let shapes = world::BlockShapes::all_full_cubes();
        let ausrollen = |sneak: bool| {
            let mut p = ruht_auf(&world, &shapes, DVec3::new(8.0, 1.0, 8.0));
            p.vel.x = 0.2;
            // Auf Schleim pendelt ein ruhender Spieler in einem winzigen Restsprung
            // (vel.y ~ -0.0016) -- das liegt UNTER der 0.003-Klemmung aus tickMovement, die
            // Bewegung dieses Ticks waere also y = 0 und damit (wie in Vanilla, s.
            // move_and_collide) gar kein Bodenkontakt. onSteppedOn braucht aber genau den.
            // Deshalb hier die normale Schwerkraft-Geschwindigkeit setzen: der Tick, in dem
            // man tatsaechlich auf den Schleim gedrueckt wird.
            p.vel.y = -GRAVITY * VERTICAL_DRAG;
            let mut input = idle_input(0.0);
            input.sneak = sneak;
            p.tick(&input, &world, &shapes);
            p.vel.x
        };
        let reibung = 0.8 * 0.91;
        assert!((ausrollen(true) - 0.2 * reibung).abs() < 1.0e-9, "mit Sneak nur die Reibung, war {}", ausrollen(true));
        // Der Unterschied ist genau der Faktor e = 0.4 + |vel.y| * 0.2 aus onSteppedOn. Der
        // Ruhewert von vel.y liegt knapp ueber 0, e also dicht ueber 0.4.
        let faktor = ausrollen(false) / ausrollen(true);
        assert!((0.4..=0.42).contains(&faktor), "onSteppedOn-Faktor unplausibel: {faktor}");
    }

    // ---------------------------------------------------------------------------------
    // Bett: 9/16 hohe Matratze mit zwei Pfosten, und ein Absprung mit Faktor 0.66.
    // Zahlen aus BedBlock (1.21.11), gegengeprueft an GrimACs CollisionData.BED und
    // BlockProperties.getBlockBounceRestitution.
    // ---------------------------------------------------------------------------------

    // Ohne eigene Form bekaeme das Bett den Vollwuerfel-Fallback (das Bettmodell wird in
    // Vanilla von einem BlockEntity gezeichnet, aus den Modelldateien kommt nichts).
    // `ends_with("_bed")` als Erkennung: die Registry darf keinen anderen Block so nennen,
    // sonst bekaeme der still eine Bettform und einen Absprung.
    #[test]
    fn nur_echte_betten_heissen_auf_bed() {
        let farben = [
            "white",
            "orange",
            "magenta",
            "light_blue",
            "yellow",
            "lime",
            "pink",
            "gray",
            "light_gray",
            "cyan",
            "purple",
            "blue",
            "brown",
            "green",
            "red",
            "black",
        ];
        let mut gefunden: Vec<&str> = Vec::new();
        for state in 0..world::state_count() as u32 {
            if let Some(name) = world::block_name(state) {
                if name.ends_with("_bed") && !gefunden.contains(&name) {
                    gefunden.push(name);
                }
            }
        }
        assert_eq!(gefunden.len(), farben.len(), "unerwartete Blöcke auf _bed: {gefunden:?}");
        for farbe in farben {
            let name = format!("{farbe}_bed");
            assert!(gefunden.contains(&name.as_str()), "{name} fehlt");
        }
    }

    // Geprueft wird die FORM, nicht ihre Zerlegung in Kaesten: seit die Formen aus dem Jar
    // kommen (world::vanilla_collision), zerlegt Vanillas VoxelShape das Bett in vier Kaesten
    // statt in "Matratze plus zwei Pfosten". Dieselbe Form, andere Aufteilung -- ein Test auf
    // die Kastenzahl wuerde nur die Zerlegung festschreiben. Deshalb Stichproben im Raum.
    #[test]
    fn bett_ist_neun_sechzehntel_hoch_mit_zwei_pfosten() {
        let shapes = world::BlockShapes::all_full_cubes();
        let drin = |state: u32, p: [f32; 3]| {
            world::collision_boxes(state, &shapes)
                .iter()
                .any(|b| (0..3).all(|i| p[i] > b.min[i] && p[i] < b.max[i]))
        };
        let bett = state_with("red_bed", &[("facing", "north"), ("part", "foot"), ("occupied", "false")]);

        // Die Matratze: 3/16 bis 9/16, ueber die ganze Zelle.
        assert!(drin(bett, [0.5, 6.0 / 16.0, 0.5]), "Mitte der Matratze fehlt");
        assert!(!drin(bett, [0.5, 10.0 / 16.0, 0.5]), "ueber 9/16 darf nichts mehr sein");
        assert!(!drin(bett, [0.5, 1.0 / 16.0, 0.5]), "unter der Matratze ist Luft, außer an den Pfosten");

        // foot + facing=north -> die Pfosten stehen im Sueden (hohes z), und zwar an beiden
        // Ecken bis hinunter auf 0.
        assert!(drin(bett, [1.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0]), "Pfosten Südwest fehlt");
        assert!(drin(bett, [15.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0]), "Pfosten Südost fehlt");
        assert!(!drin(bett, [1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0]), "im Norden darf kein Pfosten stehen");

        // Der Kopfteil desselben Betts traegt seine Pfosten am anderen Ende (niedriges z).
        let kopf = state_with("red_bed", &[("facing", "north"), ("part", "head"), ("occupied", "false")]);
        assert!(drin(kopf, [1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0]), "Pfosten Nordwest fehlt");
        assert!(drin(kopf, [15.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0]), "Pfosten Nordost fehlt");
        assert!(!drin(kopf, [1.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0]), "im Süden darf kein Pfosten stehen");
    }

    // Und die Hoehe muss sich auch in der Physik zeigen: Fuesse auf 9/16, nicht auf 1.0.
    #[test]
    fn man_steht_auf_dem_bett_auf_neun_sechzehnteln() {
        let bett = state_with("white_bed", &[("facing", "east"), ("part", "foot"), ("occupied", "false")]);
        let world = world_with_floor_and_blocks(0, &[((9, 1, 9), bett)]);
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 4.0, 9.5));
        let mut input = idle_input(0.0);
        input.sneak = true; // ohne Sneak springt man auf dem Bett, s. naechster Test
        for _ in 0..40 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.on_ground, "sollte auf dem Bett liegen");
        assert!((p.pos.y - (1.0 + 9.0 / 16.0)).abs() < 1.0e-9, "Fuesse auf 9/16 ueber der Zelle, y={}", p.pos.y);
    }

    // BedBlock.onEntityLand: wie der Schleimblock, nur mit 0.66 -- und ebenso per Sneak
    // abschaltbar (bypassesLandingEffects).
    #[test]
    fn bett_wirft_mit_zwei_dritteln_zurueck() {
        let bett = state_with("blue_bed", &[("facing", "north"), ("part", "foot"), ("occupied", "false")]);
        // Eine kleine Bettflaeche, damit der Spieler beim Auf und Ab nicht daneben landet.
        let cells: Vec<((i32, i32, i32), u32)> = (-2..=2).flat_map(|dx| (-2..=2).map(move |dz| ((9 + dx, 1, 9 + dz), bett))).collect();
        let world = world_with_floor_and_blocks(0, &cells);
        let shapes = world::BlockShapes::all_full_cubes();

        let absprung = |sneak: bool| {
            let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
            let mut input = idle_input(0.0);
            input.sneak = sneak;
            let mut vor_der_landung = 0.0;
            let mut hoechster: f64 = 0.0;
            for _ in 0..30 {
                let vorher = p.vel.y;
                p.tick(&input, &world, &shapes);
                if p.vel.y > 0.0 && hoechster == 0.0 {
                    vor_der_landung = vorher;
                }
                hoechster = hoechster.max(p.vel.y);
            }
            (vor_der_landung, hoechster)
        };
        let (vorher, rueckstoss) = absprung(false);
        assert!(rueckstoss > 0.0, "ohne Sneak muss das Bett zurückwerfen");
        // Der Rueckstoss ist -vel.y * 0.66, danach wirken Schwerkraft und Daempfung des Ticks.
        let erwartet = (-vorher * (0.66f32 as f64) - GRAVITY) * VERTICAL_DRAG;
        assert!((rueckstoss - erwartet).abs() < 1.0e-9, "Rückstoß {rueckstoss} statt {erwartet}");
        assert_eq!(absprung(true).1, 0.0, "mit Sneak kein Absprung");
    }

    // Schlafen: die Pose ist 0.2 x 0.2 mit 0.2 Augenhoehe (PlayerLikeEntity.POSE_DIMENSIONS),
    // und sie hat Vorrang -- auch mit gedrueckter Schleichtaste wird nicht in die Hockpose
    // gewechselt. Die Bewegungssperre selbst sitzt im Aufrufer (isImmobile, s. main.rs).
    #[test]
    fn schlafpose_ist_zwei_zehntel_hoch_und_hat_vorrang() {
        let world = world_with_floor_of(0, "stone");
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        p.refresh_on_ground(&world, &shapes);
        p.sleeping = true;
        let mut input = idle_input(0.0);
        input.sneak = true;
        p.tick(&input, &world, &shapes);
        assert_eq!(p.current_height(), SLEEPING_HEIGHT);
        assert_eq!(p.eye_height(), SLEEPING_HEIGHT as f32);
        assert!(!p.crouching, "SLEEPING geht der Hockpose vor");
        assert!(!p.swimming);

        // Und wach ist wieder alles normal.
        p.sleeping = false;
        p.tick(&input, &world, &shapes);
        assert_eq!(p.current_height(), CROUCHING_HEIGHT_F64);
    }

    // ---------------------------------------------------------------------------------
    // Klettern: Leiter, Ranken, Geruest, Falltuer-ueber-Leiter.
    // Zahlen aus LivingEntity.applyClimbingSpeed/applyMovementInput + ScaffoldingBlock/
    // LadderBlock (1.21.11), gegengeprueft an GrimACs
    // PredictionEngineNormal.handleOnClimbable und CollisionData.
    // ---------------------------------------------------------------------------------

    fn state_with(name: &str, props: &[(&str, &str)]) -> u32 {
        (0..world::state_count() as u32)
            .find(|&s| world::block_name(s) == Some(name) && props.iter().all(|(k, v)| world::state_props::property(s, k) == Some(*v)))
            .unwrap_or_else(|| panic!("{name} mit {props:?} unbekannt"))
    }

    // Steinboden bei floor_y, darauf einzeln gesetzte Bloecke.
    fn world_with_floor_and_blocks(floor_y: i32, cells: &[((i32, i32, i32), u32)]) -> World {
        let stein = state_of_block("stone");
        let mut w = World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    let base_y = world::MIN_Y + i * 16;
                    if floor_y >= base_y && floor_y < base_y + 16 {
                        let ly = floor_y - base_y;
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((ly * 16 + z) * 16 + x) as usize] = stein;
                            }
                        }
                    }
                    for ((bx, by, bz), state) in cells {
                        if by >= &base_y && by < &(base_y + 16) && bx.div_euclid(16) == cx && bz.div_euclid(16) == cz {
                            let (lx, ly, lz) = (bx.rem_euclid(16), by - base_y, bz.rem_euclid(16));
                            states[((ly * 16 + lz) * 16 + lx) as usize] = *state;
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    fn leiter_welt() -> (World, world::BlockShapes) {
        // Leiter mit facing=north: die 3/16-Platte liegt bei z 13..16 der Zelle, der Spieler
        // steht mit seiner 0.6er Box (z 9.2..9.8) davor in derselben Zelle.
        let leiter = state_with("ladder", &[("facing", "north"), ("waterlogged", "false")]);
        let cells: Vec<_> = (1..8).map(|y| ((9, y, 9), leiter)).collect();
        (world_with_floor_and_blocks(0, &cells), world::BlockShapes::all_full_cubes())
    }

    // LadderBlock.getOutlineShape: createCuboidZShape(16, 13, 16), pro Richtung gedreht. Eigene
    // Kollisionsform gibt es nicht, Umriss und Kollision sind dasselbe -- und beide duerfen
    // NICHT aus dem (dickenlosen) Modell kommen.
    #[test]
    fn leiter_hat_die_vanilla_kollisionsform() {
        let shapes = world::BlockShapes::all_full_cubes();
        let boxen = |facing: &str| {
            let s = state_with("ladder", &[("facing", facing), ("waterlogged", "false")]);
            let k = world::collision_boxes(s, &shapes);
            assert_eq!(k, world::outline_boxes(s, &shapes), "Umriss und Kollision sind bei der Leiter dieselbe Form");
            assert_eq!(k.len(), 1);
            (k[0].min, k[0].max)
        };
        assert_eq!(boxen("north"), ([0.0, 0.0, 13.0 / 16.0], [1.0, 1.0, 1.0]));
        assert_eq!(boxen("south"), ([0.0, 0.0, 0.0], [1.0, 1.0, 3.0 / 16.0]));
        assert_eq!(boxen("west"), ([13.0 / 16.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
        assert_eq!(boxen("east"), ([0.0, 0.0, 0.0], [3.0 / 16.0, 1.0, 1.0]));
    }

    // Mit gehaltener Sprungtaste steigt man auf: applyMovementInput setzt vel.y auf 0.2, davon
    // bleiben nach Schwerkraft und Daempfung (0.2 - 0.08) * 0.98 uebrig.
    #[test]
    fn leiter_wird_mit_sprungtaste_erklettert() {
        let (world, shapes) = leiter_welt();
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 1.0, 9.5));
        p.refresh_on_ground(&world, &shapes);
        let mut input = idle_input(0.0);
        input.jump = true;
        let start = p.pos.y;
        for _ in 0..20 {
            p.tick(&input, &world, &shapes);
        }
        assert!(p.pos.y > start + 1.5, "haette die Leiter hochsteigen muessen, y={} statt >{}", p.pos.y, start + 1.5);
        let erwartet = (CLIMB_UP_SPEED - GRAVITY) * VERTICAL_DRAG;
        assert!((p.vel.y - erwartet).abs() < 1.0e-9, "Steiggeschwindigkeit {} statt {erwartet}", p.vel.y);
    }

    // Ohne Eingabe rutscht man mit genau 0.15 pro Tick hinunter (y wird auf -0.15 begrenzt,
    // BEVOR die Bewegung stattfindet) -- deutlich langsamer als der freie Fall.
    #[test]
    fn leiter_bremst_das_hinabrutschen_auf_015() {
        let (world, shapes) = leiter_welt();
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
        for _ in 0..5 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        let vorher = p.pos.y;
        p.tick(&idle_input(0.0), &world, &shapes);
        let strecke = vorher - p.pos.y;
        assert!((strecke - CLIMB_SPEED).abs() < 1.0e-9, "Rutschstrecke {strecke} statt {CLIMB_SPEED}");
        // applyClimbingSpeed ruft onLanding() VOR der Bewegung, Entity.fall() schreibt danach
        // die Strecke DIESES Ticks wieder hinein. Die Fallhoehe kann also nie ueber einen
        // einzelnen Kletter-Tick hinauswachsen -- deshalb kein Fallschaden von der Leiter.
        assert!(p.fall_distance <= CLIMB_SPEED + 1.0e-9, "Fallhöhe darf sich nicht aufsummieren, war {}", p.fall_distance);
    }

    // isHoldingOntoLadder() == isSneaking(): schleichend haelt man sich fest und steht still.
    #[test]
    fn schleichen_haelt_an_der_leiter_fest() {
        let (world, shapes) = leiter_welt();
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
        let mut input = idle_input(0.0);
        input.sneak = true;
        for _ in 0..3 {
            p.tick(&input, &world, &shapes);
        }
        let gehalten = p.pos.y;
        for _ in 0..20 {
            p.tick(&input, &world, &shapes);
        }
        assert_eq!(p.pos.y, gehalten, "mit Sneak darf sich an der Leiter nichts bewegen");
    }

    // Ranken sind derselbe Mechanismus (BlockTags.CLIMBABLE), obwohl sie gar keine
    // Kollisionsbox haben -- man klettert IN ihnen, nicht an ihnen.
    #[test]
    fn ranken_werden_genauso_erklettert() {
        let ranke = state_of_block("vine");
        let cells: Vec<_> = (1..8).map(|y| ((9, y, 9), ranke)).collect();
        let world = world_with_floor_and_blocks(0, &cells);
        let shapes = world::BlockShapes::all_full_cubes();
        assert!(world::collision_boxes(ranke, &shapes).is_empty(), "Ranken kollidieren in Vanilla nicht");
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
        // Ein paar Ticks, bis die Schwerkraft die Geschwindigkeit ueberhaupt an die Klemmung
        // herangefuehrt hat -- vorher ist die Bewegung noch langsamer als 0.15.
        for _ in 0..5 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        let vorher = p.pos.y;
        p.tick(&idle_input(0.0), &world, &shapes);
        assert!((vorher - p.pos.y - CLIMB_SPEED).abs() < 1.0e-9, "auch in Ranken gilt die 0.15er Bremse");
    }

    // Der Fall, der bei Ranken anders liegt als bei der Leiter: Ranken haben GAR KEINE
    // Kollisionsbox, der Aufstieg haengt in Vanilla aber an `horizontalCollision`. Die noetige
    // Kollision liefert die Wand HINTER der Ranke -- der Spieler steht in der Rankenzelle und
    // drueckt gegen den Stein dahinter. Ohne diesen Test waere unbelegt, ob das bei uns
    // zusammenpasst.
    #[test]
    fn an_der_wandranke_klettert_man_durch_druecken_hoch() {
        let ranke = state_of_block("vine");
        let stein = state_of_block("stone");
        let mut cells: Vec<((i32, i32, i32), u32)> = Vec::new();
        for y in 1..10 {
            cells.push(((9, y, 9), ranke)); // Ranke, in der der Spieler steht
            cells.push(((10, y, 9), stein)); // die tragende Wand oestlich davon
        }
        let world = world_with_floor_and_blocks(0, &cells);
        let shapes = world::BlockShapes::all_full_cubes();

        let mut p = PlayerPhysics::new(DVec3::new(9.5, 1.0, 9.5));
        p.refresh_on_ground(&world, &shapes);
        let start = p.pos.y;
        // Nur nach Osten druecken, NICHT springen -- der Aufstieg muss allein aus der
        // Wandkollision kommen.
        for _ in 0..30 {
            p.tick(&lauf_input(), &world, &shapes);
        }
        assert!(p.horizontal_collision, "Testaufbau: sollte an der Wand anliegen");
        assert!(p.pos.y > start + 2.0, "haette an der Ranke hochklettern muessen, y={} statt >{}", p.pos.y, start + 2.0);
        let erwartet = (CLIMB_UP_SPEED - GRAVITY) * VERTICAL_DRAG;
        assert!((p.vel.y - erwartet).abs() < 1.0e-9, "Steiggeschwindigkeit {} statt {erwartet}", p.vel.y);
    }

    // Eine OFFENE Falltuer ueber einer gleich ausgerichteten Leiter zaehlt als kletterbar
    // (canEnterTrapdoor) -- geschlossen oder falsch gedreht dagegen nicht.
    #[test]
    fn offene_falltuer_ueber_leiter_ist_kletterbar() {
        let shapes = world::BlockShapes::all_full_cubes();
        let leiter = state_with("ladder", &[("facing", "north"), ("waterlogged", "false")]);
        let luke = |offen: &str, facing: &str| {
            state_with(
                "oak_trapdoor",
                &[("open", offen), ("facing", facing), ("half", "bottom"), ("powered", "false"), ("waterlogged", "false")],
            )
        };
        let pruefe = |luken_state: u32| {
            let world = world_with_floor_and_blocks(0, &[((9, 1, 9), leiter), ((9, 2, 9), luken_state)]);
            let mut p = PlayerPhysics::new(DVec3::new(9.5, 2.5, 9.5));
            p.sneak_key = false;
            p.is_climbing(&world)
        };
        assert!(pruefe(luke("true", "north")), "offen und gleich ausgerichtet: kletterbar");
        assert!(!pruefe(luke("false", "north")), "geschlossen: nicht kletterbar");
        assert!(!pruefe(luke("true", "south")), "andere Richtung als die Leiter: nicht kletterbar");
        let _ = shapes;
    }

    // Geruest: oben drauf traegt die Deckplatte (Fuesse bei y+1), beim Schleichen faellt sie
    // weg und man steigt mit Kletter-Geschwindigkeit hinunter -- und zwar OHNE das
    // Festhalten, das die Leiter beim Schleichen bewirkt (ScaffoldingBlock ist davon
    // ausdruecklich ausgenommen).
    #[test]
    fn geruest_traegt_oben_und_laesst_beim_schleichen_hinab() {
        let geruest = state_with("scaffolding", &[("bottom", "false"), ("waterlogged", "false")]);
        let cells: Vec<_> = (1..5).map(|y| ((9, y, 9), geruest)).collect();
        let world = world_with_floor_and_blocks(0, &cells);
        let shapes = world::BlockShapes::all_full_cubes();

        // Oben aufsetzen: die Deckplatte des obersten Geruests (Zelle y=4) endet bei y=5.
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 5.0, 9.5));
        p.refresh_on_ground(&world, &shapes);
        for _ in 0..5 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        assert!(p.on_ground && (p.pos.y - 5.0).abs() < 1.0e-9, "sollte oben auf dem Geruest stehen, y={}", p.pos.y);

        // Schleichen: Deckplatte weg, kontrolliert hinabsteigen.
        let mut input = idle_input(0.0);
        input.sneak = true;
        let mut voriges_y = p.pos.y;
        let mut schritte = Vec::new();
        for _ in 0..10 {
            p.tick(&input, &world, &shapes);
            schritte.push(voriges_y - p.pos.y);
            voriges_y = p.pos.y;
        }
        assert!(p.pos.y < 4.0, "haette absteigen muessen, y={}", p.pos.y);
        let groesster = schritte.iter().cloned().fold(0.0f64, f64::max);
        assert!(groesster <= CLIMB_SPEED + 1.0e-9, "nie schneller als Kletter-Geschwindigkeit, war {groesster}");
    }

    // DER Grim-Fall, der das Standblock-Modell erzwungen hat (Honig flaggte beim Betreten
    // und Verlassen): ein Honigblock im Steinboden ist eine 1/16 tiefe
    // Mulde. Wer mit einem Fuss darueber steht, aber noch vom Stein getragen wird, darf NICHT
    // gebremst werden -- Vanilla und Grim fragen den Standblock, nicht floor(y - 0.5).
    #[test]
    fn honigmulde_bremst_erst_wenn_man_wirklich_darauf_steht() {
        let honig = state_of_block("honey_block");
        let stein = state_of_block("stone");
        let mut world = World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if world::MIN_Y + i * 16 <= 0 && 0 < world::MIN_Y + (i + 1) * 16 {
                        let ly = (0 - world::MIN_Y).rem_euclid(16);
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((ly * 16 + z) * 16 + x) as usize] = if x == 10 && cx == 0 { honig } else { stein };
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                world.insert_column(cx, cz, sections);
            }
        }
        let shapes = world::BlockShapes::all_full_cubes();

        // Mit einem Fuss ueber der Mulde (Box 9.8..10.4), getragen vom Stein bei x=9.
        let mut p = PlayerPhysics::new(DVec3::new(10.1, 1.0, 8.5));
        p.refresh_on_ground(&world, &shapes);
        assert!(p.on_ground);
        assert_eq!(p.supporting_block, Some((9, 0, 8)), "der Stein trägt, nicht der Honig");
        assert_eq!(p.velocity_multiplier(&world), 1.0, "über der Mulde stehend darf nichts bremsen");
        assert_eq!(p.jump_multiplier(&world), 1.0);
        // Die naive Abfrage haette hier den Honig erwischt -- genau das war der Flag.
        assert_eq!(world::block_name(world.block_at(10, 0, 8)), Some("honey_block"));

        // Und wirklich auf dem Honig stehend greift der Faktor dann auch.
        let mut auf_honig = PlayerPhysics::new(DVec3::new(10.5, 0.9375, 8.5));
        auf_honig.refresh_on_ground(&world, &shapes);
        assert_eq!(auf_honig.supporting_block, Some((10, 0, 8)));
        assert_eq!(auf_honig.velocity_multiplier(&world), SOUL_SAND_HONEY_SPEED);
        assert_eq!(auf_honig.jump_multiplier(&world), 0.5);
    }

    // Ohne Standblock (in der Luft) bleibt der naive Fallback -- sonst haette ein Sprung ueber
    // die Mulde gar keinen Bezugsblock mehr.
    #[test]
    fn ohne_standblock_gilt_der_naive_fallback() {
        let world = world_with_floor_of(0, "stone");
        let shapes = world::BlockShapes::all_full_cubes();
        let mut p = PlayerPhysics::new(DVec3::new(8.3, 10.0, 8.7));
        p.tick(&idle_input(0.0), &world, &shapes);
        assert!(!p.on_ground);
        assert_eq!(p.supporting_block, None);
        assert_eq!(p.velocity_affecting_pos(&world), (8, (p.pos.y - 0.500001f32 as f64).floor() as i32, 8));
    }

    // HoneyBlock.isSliding/updateSlidingVelocity: an der SEITE einer Honigwand rutscht man mit
    // konstanter Geschwindigkeit herunter statt frei zu fallen, und die Fallhoehe bleibt bei 0
    // (onLanding -> kein Fallschaden).
    #[test]
    fn honigwand_bremst_den_fall_auf_rutschgeschwindigkeit() {
        // Wand aus Honig bei x >= 9, Spieler faellt links davor.
        let honig = state_of_block("honey_block");
        let mut world = World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    let section_base_y = world::MIN_Y + i * 16;
                    for ly in 0..16 {
                        let wy = section_base_y + ly;
                        if (1..40).contains(&wy) && cx == 0 {
                            for z in 0..16 {
                                states[((ly * 16 + z) * 16 + 9) as usize] = honig;
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                world.insert_column(cx, cz, sections);
            }
        }
        let shapes = world::BlockShapes::all_full_cubes();
        // Direkt an die Wand gedrueckt: die Honigkollision ist um 1/16 eingerueckt, die
        // Spielerbox steht also ein Stueck IN der Honigzelle -- genau das laesst das
        // Rutschen ueberhaupt greifen.
        // Ein Tausendstel Luft zur Wand: nach einer echten Kollisionsbewegung bleibt genau so
        // ein Spalt stehen, und der Test haengt nicht daran, ob eine exakt buendige Box als
        // Beruehrung zaehlt. Der Mittelpunktsabstand (0.7385) liegt weiter ueber der
        // Rutschschwelle 0.7375.
        let mut p = PlayerPhysics::new(DVec3::new(9.0 + 0.0625 - 0.3 - 0.001, 30.0, 8.5));
        // Erst ein paar Ticks fallen lassen, damit die Rutschbedingung (< -0.08) greift.
        for _ in 0..10 {
            p.tick(&idle_input(0.0), &world, &shapes);
        }
        // getNewVelocityY(-0.05) = (-0.05 - 0.08) * 0.98F
        let rutsch = (-0.05 - 0.08) * 0.98f32 as f64;
        assert!((p.vel.y - rutsch).abs() < 1.0e-9, "sollte auf Rutschgeschwindigkeit geklemmt sein, vel.y={}", p.vel.y);
        assert_eq!(p.fall_distance, 0.0, "Rutschen ruft onLanding, die Fallhöhe bleibt 0");
        // Und ohne Wand faellt derselbe Spieler in derselben Zeit viel schneller.
        let leer = world_with_floor_of(-40, "stone");
        let mut frei = PlayerPhysics::new(DVec3::new(8.5, 30.0, 8.5));
        for _ in 0..10 {
            frei.tick(&idle_input(0.0), &leer, &shapes);
        }
        assert!(frei.vel.y < rutsch * 3.0, "freier Fall muss viel schneller sein: {}", frei.vel.y);
    }

    // MOVEMENT_EFFICIENCY (Seelenlaeufigkeit auf Seelensand): Mth.lerp(eff, Blockfaktor, 1)
    // in float. Ohne Attribut bleibt die Bremse 0.4F.
    #[test]
    fn seelenlaeufigkeit_hebt_die_seelensand_bremse_auf() {
        let shapes = world::BlockShapes::all_full_cubes();
        let world = world_with_floor_of(0, "soul_sand");
        let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.5, 2.0, 8.5));
        assert_eq!(p.velocity_multiplier(&world), SOUL_SAND_HONEY_SPEED);
        p.body.movement_efficiency = 1.0;
        assert_eq!(p.velocity_multiplier(&world), 1.0);
        p.body.movement_efficiency = 0.5;
        assert_eq!(p.velocity_multiplier(&world), (0.4f32 + 0.5f32 * (1.0f32 - 0.4f32)) as f64);
    }

    // SCALE: Breite, Hoehe und Augenhoehe der Pose mal den float (EntityDimensions.scale).
    #[test]
    fn groesse_skaliert_box_und_augenhoehe() {
        let mut p = PlayerPhysics::new(DVec3::ZERO);
        assert_eq!(p.width(), WIDTH_F64);
        assert_eq!(p.current_height(), HEIGHT_F64);
        p.body.scale = 2.0;
        assert_eq!(p.width(), (PLAYER_WIDTH * 2.0) as f64);
        assert_eq!(p.current_height(), (PLAYER_HEIGHT * 2.0) as f64);
        assert_eq!(p.eye_height(), EYE_HEIGHT * 2.0);
    }

    // JUMP_STRENGTH: (float)Attribut * Blockfaktor, dann Schwerkraft und Daempfung des Ticks.
    #[test]
    fn sprungkraft_vom_server() {
        let shapes = world::BlockShapes::all_full_cubes();
        let world = world_with_floor_of(0, "stone");
        let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
        p.body.jump_strength = 0.84;
        let mut input = idle_input(0.0);
        input.jump = true;
        p.tick(&input, &world, &shapes);
        let erwartet = (0.84f32 as f64 - GRAVITY) * VERTICAL_DRAG;
        assert!((p.vel.y - erwartet).abs() < 1.0e-9, "{} statt {}", p.vel.y, erwartet);
    }

    // GRAVITY: der Fall rechnet mit dem Attribut statt mit 0.08.
    #[test]
    fn schwerkraft_vom_server() {
        let shapes = world::BlockShapes::all_full_cubes();
        let world = world_with_floor_of(0, "stone");
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 20.0, 8.0));
        p.body.gravity = 0.04;
        p.tick(&idle_input(0.0), &world, &shapes);
        assert!((p.vel.y - -0.04 * VERTICAL_DRAG).abs() < 1.0e-12, "{}", p.vel.y);
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;
    use world::ChunkSection;

    // Zustands-IDs fuer die Testwelten unten. Frueher waren das die willkuerlichen IDs 1 und
    // 2, deren Form der Test sich selbst zurechtlegte. Seit die Kollisionsformen aus dem Jar
    // kommen (world::vanilla_collision) geht das nicht mehr -- und es soll auch nicht: ein
    // Test, der einem Block eine Wunschform unterschiebt, prueft nicht mehr das, was im Spiel
    // passiert. Deshalb echte Bloecke.
    const SOLID: u32 = 1; // stone -- in dieser Registry ohnehin der erste Vollwuerfel
    fn slab_state() -> u32 {
        (0..world::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("stone_slab") && world::state_props::property(s, "type") == Some("bottom"))
            .expect("stone_slab[type=bottom] unbekannt")
    }

    // Boden bei floor_y ueber 5x5 Spalten, darauf ab Welt-X >= ledge_x eine Kante aus
    // `ledge_state` -- also genau die Situation "man laeuft gegen eine Stufe".
    fn world_with_ledge(floor_y: i32, ledge_x: i32, ledge_state: u32) -> World {
        let mut w = World::new();
        let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
        let floor_local = (floor_y - world::MIN_Y).rem_euclid(16);
        let ledge_y = floor_y + 1;
        let ledge_section = (ledge_y - world::MIN_Y).div_euclid(16);
        let ledge_local = (ledge_y - world::MIN_Y).rem_euclid(16);
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut sections = Vec::new();
                for i in 0..24 {
                    let mut states = vec![0u32; 16 * 16 * 16];
                    if i == floor_section {
                        for x in 0..16 {
                            for z in 0..16 {
                                states[((floor_local * 16 + z) * 16 + x) as usize] = SOLID;
                            }
                        }
                    }
                    if i == ledge_section {
                        for x in 0..16 {
                            if cx * 16 + x >= ledge_x {
                                for z in 0..16 {
                                    states[((ledge_local * 16 + z) * 16 + x) as usize] = ledge_state;
                                }
                            }
                        }
                    }
                    sections.push(ChunkSection::from_states(states));
                }
                w.insert_column(cx, cz, sections);
            }
        }
        w
    }

    // Die Formen kommen jetzt aus dem Jar-Dump, die Tabelle muss nichts mehr beisteuern.
    fn shapes_with_slab() -> world::BlockShapes {
        world::BlockShapes::all_full_cubes()
    }

    fn walk_east(yaw_rad: f32) -> Input {
        Input {
            forward: 0.0,
            strafe: 1.0,
            jump: false,
            sneak: false,
            sprint_held: false,
            using_item: false,
            item_use_speed: ITEM_USE_SPEED_MULT,
            item_use_can_sprint: false,
            food_level: 20,
            blindness: false,
            effect_speed_factor: 1.0,
            movement_speed: BASE_SPEED,
            sneaking_speed: DEFAULT_SNEAKING_SPEED,
            water_movement_efficiency: 0.0,
            slow_falling: false,
            levitation: None,
            jump_boost: None,
            dolphins_grace: false,
            float_drag: false,
            yaw_rad,
            look_dir_y: 0.0,
            yaw_deg: yaw_rad.to_degrees(),
            pitch_deg: 0.0,
            glider_equipped: false,
            spectator: false,
            spectator_speed: 0.0,
        }
    }

    // Der eigentliche Zweck des Features: eine halbhohe Stufe (0.5 < STEP_HEIGHT) wird im
    // Laufen ueberwunden, OHNE zu springen. Vorher blieb man daran haengen wie an einer Wand.
    #[test]
    fn halbe_stufe_wird_ohne_sprung_erstiegen() {
        let world = world_with_ledge(0, 10, slab_state());
        let shapes = shapes_with_slab();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let input = walk_east(0.0); // yaw 0 = Blick +Z, strafe schiebt nach +X
        for _ in 0..80 {
            p.tick(&input, &world, &shapes);
        }
        // Oberkante der Stufe liegt bei y = 1 (Blockzelle) + 0.5 = 1.5.
        assert!((p.pos.y - 1.5).abs() < 1.0e-6, "haette auf der Stufe stehen muessen, y={}", p.pos.y);
        assert!(p.pos.x > 10.5, "haette ueber die Kante hinauslaufen muessen, x={}", p.pos.x);
        assert!(p.on_ground, "nach dem Schritt steht man wieder am Boden");
    }

    // Gegenprobe: ein GANZER Block ist 1.0 hoch und damit ueber STEP_HEIGHT (0.6) -- da muss
    // man weiterhin springen. Ohne diese Grenze waere aus dem Feature ein Wandlauf geworden.
    #[test]
    fn ganzer_block_wird_nicht_erstiegen() {
        let world = world_with_ledge(0, 10, SOLID);
        let shapes = shapes_with_slab();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let input = walk_east(0.0);
        for _ in 0..80 {
            p.tick(&input, &world, &shapes);
        }
        assert!((p.pos.y - 1.0).abs() < 1.0e-6, "darf nicht hochsteigen, y={}", p.pos.y);
        assert!(p.pos.x < 10.0, "muss vor dem Block stehen bleiben, x={}", p.pos.x);
        assert!(p.horizontal_collision, "der Block muss weiterhin horizontal blockieren");
    }

    // Die Weltgrenze haelt wie eine Wand: Grenze 20 um den Ursprung, Ostkante bei x = 10. Der
    // Spieler (halbe Breite 0.3) bleibt mit der Mitte bei 9.7 stehen und steigt nicht hinauf.
    #[test]
    fn weltgrenze_haelt_wie_eine_wand() {
        let mut world = world_with_ledge(0, 40, SOLID);
        world.border.apply(protocol::packets::BorderUpdate::Center(0.0, 0.0));
        world.border.apply(protocol::packets::BorderUpdate::Size(20.0));
        let shapes = shapes_with_slab();
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let input = walk_east(0.0);
        for _ in 0..80 {
            p.tick(&input, &world, &shapes);
        }
        assert!((p.pos.x - 9.7).abs() < 1.0e-6, "muss an der Grenze stehen, x={}", p.pos.x);
        assert!((p.pos.y - 1.0).abs() < 1.0e-6, "die Grenze ist keine Stufe, y={}", p.pos.y);
        assert!(p.horizontal_collision);
    }

    // Stepping haengt an "am Boden ODER in diesem Tick gelandet". Mitten im Aufwaertssprung
    // gegen die Kante gilt beides nicht -- sonst koennte man sich an einer Wand hochziehen.
    #[test]
    fn kein_schritt_im_aufwaertsflug() {
        let world = world_with_ledge(0, 10, slab_state());
        let shapes = shapes_with_slab();
        let box_min = DVec3::new(9.7, 1.4, 8.0);
        let box_max = box_min + DVec3::new(PLAYER_WIDTH as f64, PLAYER_HEIGHT as f64, PLAYER_WIDTH as f64);
        // Nach oben UND nach +X gegen die Stufe, ohne Bodenkontakt.
        let movement = DVec3::new(0.5, 0.2, 0.0);
        let moved = adjust_movement_with_step(box_min, box_max, movement, false, &world, &shapes, world::ShapeContext::ABSENT, STEP_HEIGHT);
        assert!(moved.y <= movement.y + 1.0e-9, "ohne Bodenkontakt darf kein Schritt angehoben werden");
    }

    // STEP_HEIGHT 1.0 (Plugin): ein ganzer Block wird im Laufen erstiegen, mit der Vorgabe
    // 0.6 bleibt man davor stehen.
    #[test]
    fn stufenhoehe_vom_server() {
        let world = world_with_ledge(0, 10, SOLID);
        let shapes = shapes_with_slab();
        let lauf = |step: f64| {
            let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.5));
            p.body.set_step_height(step);
            for _ in 0..40 {
                p.tick(&walk_east(0.0), &world, &shapes);
            }
            p.pos.y
        };
        assert!((lauf(STEP_HEIGHT) - 1.0).abs() < 1.0e-6);
        assert!((lauf(1.0) - 2.0).abs() < 1.0e-6, "{}", lauf(1.0));
    }

    // collectStepHeights: Kandidaten sind die Y-Kanten der Kollisionsboxen relativ zur
    // Fussunterkante, aufsteigend, ohne Dubletten, ohne alles ueber der Stufenhoehe -- und
    // ohne die Hoehe, welche die Basisbewegung ohnehin schon erreicht.
    #[test]
    fn stufenhoehen_werden_gefiltert_und_sortiert() {
        let boxes = [
            Aabb { min: DVec3::new(0.0, 0.0, 0.0), max: DVec3::new(1.0, 0.5, 1.0) },
            Aabb { min: DVec3::new(1.0, 0.0, 0.0), max: DVec3::new(2.0, 0.25, 1.0) },
            // Ueber der Stufenhoehe -> faellt komplett raus.
            Aabb { min: DVec3::new(2.0, 0.0, 0.0), max: DVec3::new(3.0, 0.9, 1.0) },
        ];
        let heights = collect_step_heights(0.0, &boxes, STEP_HEIGHT, -1.0);
        assert_eq!(heights, vec![0.0, 0.25, 0.5], "sortiert, entdoppelt, oberhalb 0.6 verworfen");
        // base_dy = 0.5 schliesst genau diese Hoehe aus.
        let heights = collect_step_heights(0.0, &boxes, STEP_HEIGHT, 0.5);
        assert_eq!(heights, vec![0.0, 0.25]);
    }

    // Direction.getCollisionOrder: Y immer zuerst, dann die betragsmaessig KLEINERE
    // Horizontale, zuletzt die groessere. Direkt an der Regel geprueft statt ueber eine
    // Geometrie -- in einer konstruierten Ecke haengt das Ergebnis an zu vielen anderen
    // Groessen, um die Reihenfolge sauber nachzuweisen.
    #[test]
    fn achsenreihenfolge_folgt_vanilla() {
        use Axis::{X, Y, Z};
        assert_eq!(collision_order(DVec3::new(0.5, 0.0, 0.1)), [Y, X, Z], "|x| > |z| -> Y,X,Z");
        assert_eq!(collision_order(DVec3::new(0.1, 0.0, 0.5)), [Y, Z, X], "|x| < |z| -> Y,Z,X");
        // Vorzeichen zaehlen nicht, nur der Betrag.
        assert_eq!(collision_order(DVec3::new(-0.1, 0.0, 0.5)), [Y, Z, X]);
        assert_eq!(collision_order(DVec3::new(0.5, 0.0, -0.1)), [Y, X, Z]);
        // Gleichstand faellt wie in Vanilla auf den else-Zweig (Y,X,Z).
        assert_eq!(collision_order(DVec3::new(0.3, 0.0, 0.3)), [Y, X, Z]);
    }
}

#[cfg(test)]
mod augenhoehe_tests {
    use super::*;

    // Die drei Pose-Augenhoehen stammen aus PlayerLikeEntity.POSE_DIMENSIONS (Yarn-1.21.11-Jar):
    // STANDING 1.62, CROUCHING 1.27 (bei 1.5 Koerperhoehe), SWIMMING/GLIDING/
    // SPIN_ATTACK 0.4. Als Test festgehalten, damit die Werte nicht unbemerkt "aufgerundet"
    // werden -- sie bestimmen unmittelbar, was der Spieler sieht.
    #[test]
    fn pose_augenhoehen_entsprechen_vanilla() {
        assert_eq!(EYE_HEIGHT, 1.62);
        assert_eq!(CROUCHING_EYE_HEIGHT, 1.27);
        assert_eq!(SWIMMING_EYE_HEIGHT, 0.4);
    }

    // Vanillas halbe Annaeherung pro Tick. Der erste Schritt beim Losschleichen ist der
    // auffaelligste: 1.62 -> 1.445, NICHT sofort 1.27.
    #[test]
    fn erster_tick_geht_nur_die_halbe_strecke() {
        let nach_einem_tick = approach_eye_height(EYE_HEIGHT, CROUCHING_EYE_HEIGHT);
        assert!((nach_einem_tick - 1.445).abs() < 1e-6, "war {nach_einem_tick}");
    }

    // Ueber mehrere Ticks laeuft sie monoton auf den Zielwert zu und schiesst nie darueber
    // hinaus -- ein Ueberschwinger waere als Wippen der Kamera sichtbar.
    #[test]
    fn naehert_sich_monoton_ohne_ueberschwinger() {
        let mut h = EYE_HEIGHT;
        for _ in 0..20 {
            let vorher = h;
            h = approach_eye_height(h, SWIMMING_EYE_HEIGHT);
            assert!(h < vorher, "muss fallen: {vorher} -> {h}");
            assert!(h >= SWIMMING_EYE_HEIGHT, "darf das Ziel nie unterschreiten: {h}");
        }
        assert!((h - SWIMMING_EYE_HEIGHT).abs() < 1e-3, "nach 20 Ticks praktisch angekommen, war {h}");
    }

    // Steht die Pose still, darf sich nichts mehr bewegen -- sonst zitterte die Kamera dauerhaft.
    #[test]
    fn am_ziel_bleibt_sie_stehen() {
        assert_eq!(approach_eye_height(EYE_HEIGHT, EYE_HEIGHT), EYE_HEIGHT);
    }

    // Gegenrichtung (Aufstehen) muss genauso funktionieren wie das Abtauchen.
    #[test]
    fn aufstehen_laeuft_symmetrisch() {
        let runter = EYE_HEIGHT - approach_eye_height(EYE_HEIGHT, CROUCHING_EYE_HEIGHT);
        let hoch = approach_eye_height(CROUCHING_EYE_HEIGHT, EYE_HEIGHT) - CROUCHING_EYE_HEIGHT;
        assert!((runter - hoch).abs() < 1e-6, "runter {runter}, hoch {hoch}");
    }
}
