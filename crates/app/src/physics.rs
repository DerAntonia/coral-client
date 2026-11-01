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

mod ground;
mod body_state;
mod tick;
mod block_contact;
mod movement_modes;
mod pose;
mod moving;
mod fluid_motion;
mod collision;
mod raycast;
pub(crate) use fluid_motion::*;
pub(crate) use collision::*;
pub(crate) use raycast::*;

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
        let mut t: Box<[f32; 65536]> = vec![0.0f32; 65536].into_boxed_slice().try_into().unwrap();
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
    mc_sin(f64::from(pitch_deg * RADIANS_PER_DEGREE))
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
        faktor *= 1.0 + SPEED_EFFECT_MODIFIER * (f64::from(staerke) + 1.0);
    }
    if let Some(staerke) = slowness {
        faktor *= 1.0 + SLOWNESS_EFFECT_MODIFIER * (f64::from(staerke) + 1.0);
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
        return f64::from(0.66f32);
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
const fn float_or(input: &Input, faktor: f64) -> f64 {
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
        self.step_height = if v == STEP_HEIGHT { STEP_HEIGHT } else { f64::from(v as f32) };
    }

    // Eine Posenmasszahl (Breite, Hoehe, Augenhoehe) skaliert -- in float wie EntityDimensions.
    fn scaled(&self, v: f32) -> f32 {
        v * self.scale
    }

    pub fn width(&self) -> f64 {
        f64::from(self.scaled(PLAYER_WIDTH))
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
    // Weben: Spinnennetze bremsen nur halb so stark (s. world::slow_movement_multiplier).
    pub weaving: bool,
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
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod step_tests;

#[cfg(test)]
mod augenhoehe_tests;
