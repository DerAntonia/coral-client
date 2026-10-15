// Client-seitiger Entity-Zustand aus den Server-Packets (Add Entity, Move Entity *,
// Entity Position Sync, Teleport Entity, Remove Entities).
//
// Interpolation: der Server aktualisiert Entities NICHT mit 20 Hz -- Vanillas Entity-
// Tracker sendet Positions-Updates nur alle `updateInterval`-Ticks (Spieler: alle 2 Ticks
// = 10 Hz, die meisten Mobs: alle 3 Ticks = ~6.7 Hz), gerendert wird aber mit z.B. 240 FPS.
// Ohne Glaettung ruckeln fremde Entities sichtbar in diesen ~100ms-Spruengen.
//
// Geglaettet wird GENAU WIE VANILLA, in zwei getrennten Stufen -- frueher stand hier
// stattdessen eine selbstgebaute Wanduhr-Interpolation mit adaptiver Lerp-Dauer, die neben
// der Trefferposition eine ZWEITE, abweichende Position fuehrte:
//
//   1. Pro CLIENT-TICK (PositionInterpolator.tick, s. tick_interpolation): die Position
//      wandert in INTERPOLATION_STEPS diskreten Schritten auf die zuletzt empfangene
//      Serverposition zu. Der Lerp-Nenner ist die Zahl der NOCH offenen Schritte, damit der
//      letzte Schritt exakt auf dem Ziel landet. Das Ergebnis ist Vanillas
//      `entity.getEntityPos()` -- die Position, gegen die auch der Server und GrimAC rechnen.
//   2. Pro FRAME (Entity.getLerpedPos, s. render_pos): zwischen dem Stand zu Beginn des
//      laufenden Ticks (`last_pos`, Vanillas lastX/lastY/lastZ aus baseTick -> tickLastPos)
//      und dem aktuellen Stand, mit dem Bruchteil des laufenden Ticks.
//
// Der grosse Gewinn gegenueber der frueheren Fassung: Bild und Trefferbox sind wieder
// DIESELBE Groesse. Was man sieht, ist genau das, worauf man zielt (bis auf den
// Zwischentick-Anteil, den Vanilla ebenso hat) -- vorher liefen beide auseinander, weil die
// eine an der Wanduhr und die andere an Ticks haengte.
//
// Rotation laeuft ueber dieselben Schritte (Winkel-Lerp), die Kopfdrehung ueber ihren
// eigenen Zaehler (LivingEntity.lerpHeadYaw, HEAD_YAW_STEPS Schritte).

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use crate::entity_types::ENTITY_TYPE_NAMES;

pub fn entity_type_name(type_index: i32) -> Option<&'static str> {
    usize::try_from(type_index).ok().and_then(|i| ENTITY_TYPE_NAMES.get(i).copied())
}

// Vanilla kickt den Client mit "Attempting to attack an invalid entity", wenn das Ziel
// ein ItemEntity, ExperienceOrb oder AbstractArrow (Pfeil/Spektralpfeil/Dreizack) ist --
// exakt diese Liste aus ServerGamePacketListenerImpl. Solche Ziele darf der Raycast fuer
// den Angriff also gar nicht erst hergeben.
pub fn is_attackable(type_index: i32) -> bool {
    !matches!(entity_type_name(type_index), Some("item" | "experience_orb" | "arrow" | "spectral_arrow" | "trident") | None)
}

pub fn is_player(type_index: i32) -> bool {
    entity_type_name(type_index) == Some("player")
}

// --- Display-Entities ------------------------------------------------------------------
//
// Die drei Unterklassen von DisplayEntity (text_display, item_display, block_display) sind
// keine Lebewesen, haben kein Modell und keine Trefferbox -- sie sind reine Anzeige. Server
// bauen daraus Schilder, schwebende Items und Deko. Ohne eigene Behandlung fielen sie in den
// "kein Modell"-Zweig des Renderers und stuenden als orangefarbene, spielergrosse Kaesten in
// der Welt, obwohl an der Stelle in Wahrheit z.B. eine Zeile Text steht.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayKind {
    Text,
    Item,
    Block,
}

pub fn display_kind(type_index: i32) -> Option<DisplayKind> {
    match entity_type_name(type_index)? {
        "text_display" => Some(DisplayKind::Text),
        "item_display" => Some(DisplayKind::Item),
        "block_display" => Some(DisplayKind::Block),
        _ => None,
    }
}

// Was ein Display-Entity anzeigt. Nur die Felder, die dieser Client auch zeichnet -- die
// Helligkeits-/Schatten-Werte werden zwar gelesen (sie muessen ueberlesen werden, damit der
// Inhalt dahinter ankommt), aber nicht gefuehrt.
#[derive(Clone, Debug, Default)]
pub struct DisplayInfo {
    // AffineTransformation: Verschiebung und Skalierung relativ zur Entity-Position.
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    // Linke/rechte Drehung der AffineTransformation als Quaternion (x, y, z, w):
    // Matrix = translation * left_rotation * scale * right_rotation.
    pub left_rotation: [f32; 4],
    pub right_rotation: [f32; 4],
    // DisplayEntity.BillboardMode: 0 FIXED, 1 VERTICAL, 2 HORIZONTAL, 3 CENTER.
    pub billboard: u8,
    // Sichtweite als Vielfaches von 64 Bloecken (DisplayEntity.shouldRender).
    pub view_range: f32,
    pub content: DisplayContent,
    // item_display: ItemDisplayContext als Byte (s. packets::ITEM_DISPLAY_INDEX_MODE).
    pub item_mode: u8,
    // Laufende Interpolation einer Transformations-Aenderung (DisplayEntity mit
    // INTERPOLATION_DURATION != 0): Ausgangslage, Beginn als Entity-Alter in Ticks und Dauer.
    // Ohne sie sprangen alle Server-Animationen aus Displays von Stufe zu Stufe.
    pub interp_from: Option<DisplayTransform>,
    pub interp_start: f32,
    pub interp_duration: i32,
}

// Die vier Teile der AffineTransformation eines Displays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayTransform {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub left_rotation: [f32; 4],
    pub right_rotation: [f32; 4],
}

// Quaternionen (x, y, z, w) auf dem kuerzeren Weg interpolieren (JOML slerp).
fn quat_slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let b = if dot < 0.0 {
        dot = -dot;
        b.map(|v| -v)
    } else {
        b
    };
    let (wa, wb) = if dot > 0.9995 {
        (1.0 - t, t)
    } else {
        let theta = dot.clamp(-1.0, 1.0).acos();
        let sin = theta.sin();
        (((1.0 - t) * theta).sin() / sin, (t * theta).sin() / sin)
    };
    let q = [0, 1, 2, 3].map(|i| a[i] * wa + b[i] * wb);
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-9);
    q.map(|v| v / len)
}

impl DisplayInfo {
    pub fn target(&self) -> DisplayTransform {
        DisplayTransform { translation: self.translation, scale: self.scale, left_rotation: self.left_rotation, right_rotation: self.right_rotation }
    }

    // Die Transformation zum Entity-Alter `age` (Ticks samt Zwischenbild-Anteil):
    // DisplayEntity.getLerpProgress + AbstractInterpolator -- Verschiebung und Skalierung
    // linear, Drehungen per slerp.
    pub fn transform_at(&self, age: f32) -> DisplayTransform {
        let ziel = self.target();
        let Some(von) = self.interp_from.filter(|_| self.interp_duration > 0) else { return ziel };
        let t = ((age - self.interp_start) / self.interp_duration as f32).clamp(0.0, 1.0);
        let lerp3 = |a: [f32; 3], b: [f32; 3]| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
        DisplayTransform {
            translation: lerp3(von.translation, ziel.translation),
            scale: lerp3(von.scale, ziel.scale),
            left_rotation: quat_slerp(von.left_rotation, ziel.left_rotation, t),
            right_rotation: quat_slerp(von.right_rotation, ziel.right_rotation, t),
        }
    }

    // Vanillas Vorgabewerte aus DisplayEntity.initDataTracker -- der Server schickt nur
    // GEAENDERTE Felder, alles andere muss auf diesen Werten stehen.
    pub fn new(content: DisplayContent) -> Self {
        Self {
            translation: [0.0; 3],
            scale: [1.0; 3],
            left_rotation: [0.0, 0.0, 0.0, 1.0],
            right_rotation: [0.0, 0.0, 0.0, 1.0],
            billboard: 0,
            view_range: 1.0,
            content,
            item_mode: 0,
            interp_from: None,
            interp_start: 0.0,
            interp_duration: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DisplayContent {
    // Abschnitte je Zeile: die Zeilen entstehen an den Zeilenumbruechen im Text.
    Text(TextDisplay),
    // Das Item wie in einer Hand: Registry-ID, Modell (item_model), Trankfarbe ...
    Item(Option<HeldItem>),
    // Block-State-Id.
    Block(u32),
}

impl Default for DisplayContent {
    fn default() -> Self {
        DisplayContent::Text(TextDisplay::default())
    }
}

#[derive(Clone, Debug)]
pub struct TextDisplay {
    pub spans: Vec<protocol::nbt::TextSpan>,
    // TextDisplayEntity.BACKGROUND, ARGB. Vanillas Startwert ist 0x40000000 (25 % Schwarz).
    pub background: i32,
    pub opacity: u8,
    pub flags: u8,
}

impl Default for TextDisplay {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            background: protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND,
            // TextDisplayEntity.initDataTracker: TEXT_OPACITY = -1 (als Byte 0xFF = voll).
            opacity: 0xFF,
            flags: 0,
        }
    }
}

// Ist der Typ in Vanilla ein LivingEntity? Entscheidet, wer bei Schaden rot aufblitzt:
// `Entity.onDamaged` ist LEER, erst `LivingEntity.onDamaged` setzt hurtTime. Ein Boot oder
// eine Lore nimmt zwar Schaden (der Server schickt dafuer auch ein Damage-Paket), blitzt in
// Vanilla aber nicht.
//
// Die Tabelle ist aus der Klassenhierarchie im Jar abgeleitet, nicht von Hand geschrieben --
// s. entity_types::ENTITY_TYPE_IS_LIVING.
pub fn is_living(type_index: i32) -> bool {
    usize::try_from(type_index).ok().and_then(|i| crate::entity_types::ENTITY_TYPE_IS_LIVING.get(i).copied()).unwrap_or(false)
}

// Vanillas LivingEntity.onDamaged: `maxHurtTime = 10; hurtTime = maxHurtTime`. Der Zaehler
// laeuft danach in der Tick-Schleife um 1 pro Tick herunter (LivingEntity.tick), und der
// Renderer fragt nur `hurtTime > 0` ab -- die Faerbung ist also KONSTANT und blendet nicht
// aus (s. OverlayTexture: die Hurt-Zeile hat fuer jede Spalte denselben Wert).
pub const MAX_HURT_TIME: u8 = 10;

// --- Schrittzahl der Angleichung ----------------------------------------------------------
//
// Vanilla bewegt fremde Entities in genau drei diskreten Tick-Schritten
// (PositionInterpolator.DEFAULT_INTERPOLATION_DURATION = 3, s. Modul-Kommentar). Diese Zahl
// ist nicht frei waehlbar: GrimAC bildet exakt dasselbe Modell nach (ReachInterpolationData,
// interpolationSteps = 3 fuer Lebewesen, Low-/High-Bound je einen Schritt pro Tick). Wer
// anders interpoliert, zielt auf eine Box, die weder Server noch Grim dort annehmen -- und
// das flaggt nicht als "Reach" (zu weit weg), sondern als "Hitboxes": in Reach.checkReach
// bleibt minDistance auf Double.MAX_VALUE stehen, weil der Strahl die Box aus KEINER der
// moeglichen Augenhoehen und mit KEINER der drei erlaubten Blickrichtungen schneidet.
// Dauer eines Handschlags in Ticks. Vanilla liest sie seit 1.21.9 aus dem Item
// (SwingAnimationComponent), dessen Vorgabe DEFAULT = (WHACK, 6) ist -- 6 Ticks gelten fuer
// alles ausser Sonderfaellen. Hast und Abbaulaehmung verkuerzen bzw. verlaengern sie in
// Vanilla zusaetzlich; beide Effekte verfolgt dieser Client fuer FREMDE Entities nicht.
pub const HAND_SWING_TICKS: i32 = 6;

// Wie lange eine Crit-Meldung auf ihr Schadenspaket wartet, falls sie ihm voraus ist. Zwei
// Ticks: die beiden Pakete entstehen in Vanilla im SELBEN Tick (s. Entity::crit_grace), mehr
// als ein Tick Versatz waere schon kein Reihenfolge-Problem mehr, sondern ein anderer Treffer.
const CRIT_GRACE_TICKS: u8 = 2;

const INTERPOLATION_STEPS: u32 = 3;

// ExperienceOrb: Schwerkraft je Tick und Reichweite, in der er einem Spieler folgt.
const ORB_GRAVITY: f64 = 0.03;
const ORB_FOLLOW_RANGE: f64 = 8.0;

// Ein Spieler, dem Erfahrungsorbs folgen koennen: Entity-ID, Fussposition, Augenhoehe.
#[derive(Clone, Copy, Debug)]
pub struct OrbPlayer {
    pub id: i32,
    pub pos: (f64, f64, f64),
    pub eye_height: f64,
}

// Kopfdrehung hat in Vanilla ihren eigenen Zaehler: ClientPlayNetworkHandler.onEntitySetHeadYaw
// ruft updateTrackedHeadRotation(headYaw, 3), LivingEntity.tick() zieht daraus pro Tick
// lerpAngleDegrees(1/headTrackingIncrements, headYaw, serverHeadYaw) und zaehlt herunter.
const HEAD_YAW_STEPS: u32 = 3;

// Wie viele Update-Abstaende je Entity fuer die Diagnose (s. update_interval_ms) behalten
// werden. 40 Pakete sind bei ~10 Hz etwa 4 Sekunden -- lang genug, dass ein einzelner
// Lag-Spike den Mittelwert nicht dominiert, kurz genug, dass die Anzeige noch reagiert.
const INTERVAL_SAMPLES: usize = 40;

// Pose-Indizes aus EntityPose.class (1.21.11 yarn). Nur die, die die Hoehe eines Spielers
// aendern -- der Rest ist entweder STANDING-gross oder betrifft Mobs, fuer die wir ohnehin
// keine eigenen Masse fuehren.
const POSE_GLIDING: u8 = 1;
const POSE_SLEEPING: u8 = 2;
const POSE_SWIMMING: u8 = 3;
const POSE_SPIN_ATTACK: u8 = 4;
const POSE_CROUCHING: u8 = 5;

// Spielermasse je Pose, nach PlayerLikeEntity.POSE_DIMENSIONS -- dieselbe Tabelle, aus der
// die eigene Physik ihre Kollisionshoehe nimmt (s. physics.rs). Breite ist ausser im Schlaf
// immer 0.6.
const PLAYER_BOX_WIDTH: f64 = 0.6;
const PLAYER_BOX_HEIGHT: f64 = 1.8;
const PLAYER_BOX_HEIGHT_CROUCHING: f64 = 1.5;
// SWIMMING gilt genauso fuers Gleiten und den Dreizack-Wirbel: 0.6 x 0.6.
const PLAYER_BOX_HEIGHT_SWIMMING: f64 = 0.6;
const PLAYER_BOX_SLEEPING: (f64, f64) = (0.2, 0.2);

// Breite und Hoehe der Box, gegen die ein Angriff geprueft wird.
//
// Fuer Spieler haengt sie an der Pose -- und genau daran hing der zweite Teil des
// Hitboxes-Flags: ein schleichendes Ziel ist 1.5 hoch, ein kriechendes/schwimmendes 0.6. Wer
// pauschal mit 1.8 zielt, schiesst dem geduckten Gegner ueber den Kopf, waehrend Grim mit den
// ECHTEN Massen aus expandBoundingBoxByEntityDimensions rechnet.
//
// Fuer alles andere bleibt es bei Spielermassen: das Modell zeichnet bislang jedes Entity als
// Biped, und eine erfundene Tabelle waere hier keine Verbesserung, sondern nur eine zweite
// Fehlerquelle. Ein Mob mit abweichender Groesse ist damit weiterhin ungenau -- das ist eine
// bekannte Luecke, keine Annahme, die hier stillschweigend als richtig verkauft wird.
// Typen, deren Boolean an Metadaten-Index 16 "Baby" heisst: alle PassiveEntity (Tiere,
// Dorfbewohner, Haendler) und die Zombie-Familie (ZombieEntity.BABY liegt am selben Index).
// Andere Typen haben dort andere Booleans (Guardian, Raider, Ghast, Fische, Allay, ...).
pub fn can_be_baby(name: &str) -> bool {
    matches!(
        name,
        "villager"
            | "wandering_trader"
            | "zombie"
            | "zombie_villager"
            | "husk"
            | "drowned"
            | "zombified_piglin"
            | "cow"
            | "mooshroom"
            | "pig"
            | "sheep"
            | "chicken"
            | "horse"
            | "donkey"
            | "mule"
            | "skeleton_horse"
            | "zombie_horse"
            | "camel"
            | "llama"
            | "trader_llama"
            | "goat"
            | "cat"
            | "ocelot"
            | "wolf"
            | "fox"
            | "rabbit"
            | "panda"
            | "polar_bear"
            | "turtle"
            | "bee"
            | "frog"
            | "axolotl"
            | "sniffer"
            | "armadillo"
            | "hoglin"
            | "strider"
            | "happy_ghast"
    )
}

pub fn hit_box_size(entity_type: i32, pose: u8) -> (f64, f64) {
    // Alles ausser Spielern: die festen Vanilla-Masse des Typs. Frueher hatte JEDES Wesen die
    // Spielerbox -- ein End-Kristall (2 x 2) war damit viel zu schmal und zu niedrig,
    // was auch das Anvisieren beim Angriff betraf.
    if !is_player(entity_type) {
        return entity_type_name(entity_type)
            .and_then(crate::entity_types::vanilla_dimensions)
            .unwrap_or((PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT));
    }
    match pose {
        POSE_SLEEPING => PLAYER_BOX_SLEEPING,
        POSE_SWIMMING | POSE_GLIDING | POSE_SPIN_ATTACK => (PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT_SWIMMING),
        POSE_CROUCHING => (PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT_CROUCHING),
        _ => (PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT),
    }
}

pub struct Entity {
    pub uuid: [u8; 16],
    // Index in der ENTITY_TYPE-Registry (1.21.11). Wird vorerst nur durchgereicht;
    // fuer das Box-Rendering ist der konkrete Typ egal.
    pub entity_type: i32,
    // AKTUELLE, tick-quantisierte Position (Fuesse) -- Vanillas entity.getEntityPos(). Das
    // ist die Groesse, gegen die gezielt und getroffen wird, NICHT die zuletzt vom Server
    // geschickte (die steht in target_pos).
    pub pos: (f64, f64, f64),
    // Stand zu Beginn des laufenden Ticks (Vanillas lastX/lastY/lastZ, gesetzt in
    // baseTick -> tickLastPos) -- der Startpunkt der Frame-Interpolation.
    last_pos: (f64, f64, f64),
    // Autoritative Serverposition (Vanillas TrackedPosition). Relative Bewegungspakete
    // rechnen ihre Deltas hierauf, NICHT auf die interpolierte Position -- sonst summierte
    // sich der Interpolationsrueckstand von Paket zu Paket auf.
    tracked_pos: (f64, f64, f64),
    // Ziel der laufenden Angleichung samt offener Schritte (PositionInterpolator.Data).
    target_pos: (f64, f64, f64),
    target_yaw_deg: f32,
    target_pitch_deg: f32,
    interp_steps: u32,
    // Nur fuer Projektile (s. projectile_kind): ihre eigene Geschwindigkeit. Der Server
    // schickt sie beim Spawn und danach selten -- geflogen wird clientseitig.
    vel: (f64, f64, f64),
    // Steckt das Projektil in einem Block? Dann bewegt es sich nicht mehr
    // (AbstractArrow.inGround); ein Wurfgeschoss ist an der Stelle ohnehin zu Ende.
    in_ground: bool,
    // Groesse eines interaction-Entities (Breite, Hoehe) aus den Metadaten; Vanilla-Vorgabe 1 x 1.
    interaction_size: (f64, f64),
    // Einmal beim Spawn aufgeloest, damit nicht jeder Tick den Typnamen nachschlaegt.
    projectile: Option<ProjectileKind>,
    // Ein liegender/fallender Gegenstand (Typ "item"): der Client simuliert ihn selbst wie
    // Vanillas ItemEntity.tick, s. tick_item. Der Server schickt seine Position nur alle 20
    // Ticks -- ohne eigene Simulation sprang ein geworfenes Item von Paket zu Paket.
    item_physics: bool,
    // Ein fallender Block (Typ "falling_block"): Vanillas FallingBlockEntity.tick laeuft auf
    // dem Client mit (Schwerkraft, Bewegung, Reibung), der Server meldet die Position nur
    // selten. Ohne eigene Simulation hinge der Block zwischen den Meldungen in der Luft.
    block_physics: bool,
    // Ein Erfahrungsorb (Typ "experience_orb"): wie in Vanillas ExperienceOrb.tick fliegt er auf
    // dem Client selbst zum naechsten Spieler, s. tick_orb. Der Server meldet ihn nur alle 20
    // Ticks -- ohne eigene Simulation kam ein Orb erst mit dem naechsten Paket ruckartig an.
    orb_physics: bool,
    // Der Spieler, dem der Orb gerade folgt (Entity-ID), s. tick_orb.
    orb_target: Option<i32>,
    // Der Blockzustand eines fallenden Blocks (Data-Feld des Spawn-Pakets), sonst None.
    pub falling_block: Option<u32>,
    // Gegenstandsrahmen: zeigt in diese Richtung (Direction-ID aus dem Spawn-Paket: 0 unten,
    // 1 oben, 2 Norden, 3 Sueden, 4 Westen, 5 Osten) und dreht seinen Inhalt in Achteln.
    pub frame_facing: u8,
    pub frame_rotation: i32,
    // Jungtier (Metadaten Index 16, nur bei Typen aus can_be_baby): halbe Hitbox.
    pub baby: bool,
    // Dorfbewohner-Daten (Biom-Typ, Beruf, Stufe), s. mob_catalog::villager_layers.
    pub villager_data: Option<(i32, i32, i32)>,
    on_ground: bool,
    // REIN OPTISCH: der Versatz vom echten Spawnpunkt zur Hand des Schuetzen, plus die Zahl
    // der Ticks, ueber die er wieder auf Null zurueckgeht (s. launch_render_offset).
    launch_offset: (f32, f32, f32),
    launch_ticks: u32,
    // Tick-quantisierte Rotation (wie pos) plus ihr Stand zu Tickbeginn.
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    last_yaw_deg: f32,
    last_pitch_deg: f32,
    // Ankunftszeit des letzten Positions-Updates.
    updated_at: Instant,
    // Reine Diagnose (F3-Overlay): die Abstaende der letzten Positions-Updates in
    // Millisekunden, juengster zuletzt. Das ist die Groesse, die bestimmt, wie alt die
    // angezeigte Position eines fremden Entities im schlechtesten Fall ist -- der Server
    // sendet nicht mit 20 Hz, sondern im updateInterval seines Entity-Trackers.
    update_intervals: VecDeque<f32>,
    // Dauer der AKTUELLEN Lerp-Strecke (s. Modul-Kommentar: adaptiv statt fest 1/20s).
    // Ob schon ein echtes Bewegungs-Update nach dem Spawn ankam -- nur noch fuer die
    // Diagnose (der Abstand SPAWN->erstes Update ist kein Paket-Takt und wuerde die
    // Messreihe verzerren).
    has_moved: bool,
    // Schrittanimation (Vanillas limbSwingAnimationProgress / limbSwingAmplitude, gefuehrt
    // von LimbAnimator). Der Server schickt das NICHT -- Vanilla rechnet es genauso lokal aus
    // der zurueckgelegten Strecke aus. Anders als Vanilla haben wir aber keine 20 Ticks pro
    // Sekunde an Positions-Updates, sondern die Update-Rate des Trackers (~10 Hz bei
    // Spielern, ~6.7 Hz bei Mobs), deshalb wird die Strecke auf Tick-Aequivalente
    // umgerechnet statt pro Paket einmal gezaehlt zu werden.
    limbs: crate::entity_model::LimbAnimator,
    // Nachhinkende Position fuer den Umhang (s. entity_model::CapeState). Wie die
    // Schrittanimation rein clientseitig gerechnet und, wie sie, auf Tick-Aequivalente
    // umgerechnet -- der Zustand ist eine Tick-Groesse, unsere Updates sind es nicht.
    //
    // Nur fuer Spieler ueberhaupt von Bedeutung; ein paar Byte pro Mob sind billiger als eine
    // zweite Tabelle daneben, die man synchron halten muesste.
    cape: crate::entity_model::CapeState,
    // Alter in Ticks, seit wir dieses Entity zum ersten Mal gesehen haben. Treibt die
    // Ruhe-Bewegung der Arme (s. entity_model::apply_arm_sway).
    //
    // Vanilla nimmt das ECHTE Entity-Alter (Ticks seit dem Spawn in der Welt), das wir nicht
    // kennen. Fuer die Bewegung ist das gleichwertig: sie ist periodisch (~70 Ticks), es zaehlt
    // also nur der Rest modulo Periode, und ein anderer Nullpunkt aendert nur die Phase. Weil
    // Entities zu verschiedenen Zeiten auftauchen, ergibt sich die Phasenstreuung von selbst --
    // sie atmen nicht im Gleichschritt.
    age_ticks: f32,
    // Kopfdrehung. Geglaettet ueber einen EIGENEN Zaehler (s. HEAD_YAW_STEPS) -- der Server
    // schickt Kopf- und Blickdrehung (`yaw_deg`) in getrennten Paketen, nicht im selben Takt.
    head_yaw_deg: f32,
    last_head_yaw_deg: f32,
    server_head_yaw_deg: f32,
    head_steps: u32,
    // KOERPERdrehung eines Lebewesens (Vanillas bodyYaw). Die kommt NICHT vom Server: `yaw_deg`
    // ist getYaw(), also die Blickrichtung -- den Koerper rechnet jeder Client selbst, in
    // LivingEntity.tick (Ziel = Laufrichtung) + turnHead (s. entity_model::turn_body). Frueher
    // stand hier die Annahme, beide kaemen fertig vom Server; die Figur zeigte dadurch immer
    // in Blickrichtung und lief beim Seitwaertslaufen seitlich, waehrend die Beine vor und
    // zurueck schwangen.
    body_yaw_deg: f32,
    last_body_yaw_deg: f32,
    // Wie lange das Wesen schon ununterbrochen ein Item benutzt -- fuer isBlocking, das erst
    // nach der Block-Verzoegerung des Schildes greift (s. is_blocking).
    use_ticks: u32,
    // Was das Entity in den Haenden haelt (Set Equipment, s. packets::CB_PLAY_SET_EQUIPMENT).
    // None = leere Hand. Der Server schickt das nur bei AENDERUNGEN, deshalb wird der Wert
    // hier gehalten und nicht pro Frame neu erfragt.
    pub main_hand: Option<HeldItem>,
    pub off_hand: Option<HeldItem>,
    // Getragene Ruestung, nach ArmorSlot::index() abgelegt (Fuesse, Beine, Brust, Kopf).
    // Kommt aus demselben Set-Equipment-Paket wie die Haende -- frueher wurden diese vier
    // Slots verworfen, und ein Gegner in voller Diamantruestung sah aus wie in Zivil.
    pub armor: [Option<HeldItem>; 4],
    // Pose-Index aus dem Metadaten-Strom (Entity.POSE, s. packets::read_set_entity_data).
    // Bestimmt die Hoehe der Trefferbox; Standard ist STANDING (0), denn genau das ist auch
    // Vanillas Vorgabewert, solange der Server nichts anderes schickt.
    pub pose: u8,
    // Restliche Ticks des Schadens-Aufblitzens (Vanillas LivingEntity.hurtTime). 0 = kein
    // Blitz. Gesetzt aus dem Damage-Paket, heruntergezaehlt im Tick -- genau wie in Vanilla.
    hurt_ticks: u8,
    // Ticks seit dem letzten Treffer (u32::MAX = nie). Fuer die Schadens-Faerbung, deren
    // Dauer einstellbar ist; hurt_ticks bleibt Vanillas Zaehler.
    ticks_since_hurt: u32,
    // Der Handschlag (LivingEntity.handSwinging / handSwingTicks / handSwingProgress). Anders
    // als das Schrittschwingen ist das KEINE lokale Rechnung: der Server schickt jeden Schlag
    // als eigenes Paket (EntityAnimationS2CPacket, s. packets::CB_PLAY_ANIMATE), weil er nicht
    // aus der Bewegung folgt. Der Verlauf danach ist dann wieder rein clientseitig.
    //
    // `hand_swing_ticks` ist bewusst VORZEICHENBEHAFTET: Vanilla setzt beim Anstossen -1, damit
    // der erste Tick danach auf 0 landet und der Schlag mit progress 0 beginnt statt schon
    // einen Schritt weit fortgeschritten zu sein.
    // Stammt der laufende Trefferblitz von einem KRITISCHEN Treffer? Der Server schickt dafuer
    // ein eigenes Paket (EntityAnimationS2CPacket mit CRIT), und zwar an alle in der Naehe
    // EINSCHLIESSLICH des Getroffenen selbst (ServerPlayerEntity.addCritParticles ->
    // sendToNearbyPlayers -> sendToSelfAndListeners) -- anders als beim Armschwung gibt es hier
    // also keine Selbst-Ausnahme, man sieht auch den Crit auf sich selbst.
    hurt_by_crit: bool,
    // Ein Crit-Paket, das VOR dem Schadenspaket eintrifft, wartet hier ein paar Ticks auf
    // seinen Treffer. In Vanilla ist die Reihenfolge umgekehrt (PlayerEntity.attack ruft erst
    // sidedDamage und nur bei Erfolg addAttackParticlesAndSounds), aber sich auf eine
    // Paketreihenfolge zu verlassen, die man nicht erzwingt, ist eine Wette -- und ein
    // vertauschtes Paar faerbte den Blitz still in der falschen Farbe.
    crit_grace: u8,
    // Vom eigenen Client gelenkt (das Boot, Pferd ... unter dem eigenen Spieler, s.
    // Entities::set_driven): Vanillas isLogicalSideForUpdatingMovement. Solange das gilt,
    // verschieben Positionspakete des Servers nur noch tracked_pos, die Stelle selbst setzt der
    // Client (driven_next) -- sonst zoege die Interpolation das Fahrzeug laufend auf den
    // Stand zurueck, den der Server vor einem halben Ping von uns gehoert hat.
    driven: bool,
    // Die Stelle und Drehung, die der naechste Tick uebernimmt (s. Entities::drive).
    driven_next: Option<((f64, f64, f64), f32, f32)>,
    // Sattel (Slot SADDLE) und Koerper-Slot (BODY: Pferderuestung, Geschirr des gluecklichen
    // Ghasts) -- entscheiden, ob ein Reittier sich lenken laesst (s. riding::Steering).
    pub saddled: bool,
    pub body_item: Option<HeldItem>,
    hand_swinging: bool,
    hand_swing_ticks: i32,
    hand_swing_progress: f32,
    last_hand_swing_progress: f32,
    // Mit welcher Hand geschlagen wurde (Vanillas preferredHand).
    swing_off_hand: bool,
    // Entity.CUSTOM_NAME / Entity.NAME_VISIBLE. Zusammen ergeben sie das Namensschild eines
    // Entities -- und in der Praxis fast jedes Server-"Hologramm": ein unsichtbarer
    // Armorstand mit dauerhaft eingeblendetem Namen. Vanillas Entity.shouldRenderName() ist
    // genau `isCustomNameVisible()`.
    pub custom_name: Option<Vec<protocol::nbt::TextSpan>>,
    pub name_visible: bool,
    // Nur beim End-Kristall: Bodenplatte zeichnen (EndCrystalEntity.SHOW_BOTTOM, Vorgabe an).
    pub end_crystal_show_bottom: bool,
    // TamableAnimal-Flags einer Katze (Bit 1 = sitzt, s. packets::TAMABLE_FLAG_SITTING).
    pub tamable_flags: u8,
    // Registry-ID der Katzenvariante; None, solange der Server keine geschickt hat.
    pub cat_variant: Option<i32>,
    // Nur bei Lebewesen: Farben der Trank-Wirbel und ob es nur Umgebungs-Effekte sind (s.
    // packets::EntityDataUpdate::potion_swirls).
    pub potion_swirls: Vec<u32>,
    pub potion_swirls_ambient: bool,
    // Nur bei einer Trankwolke (area_effect_cloud): Radius, ob sie noch wartet, und welcher
    // Partikel sie fuellt (Vorgabe: weisse Effekt-Wirbel).
    pub cloud_radius: f32,
    pub cloud_waiting: bool,
    pub cloud_particle: protocol::particle::ParticleEffect,
    // Nur bei einer Feuerwerksrakete: an welchem Schuetzen sie haengt (Elytra-Schub), s.
    // packets::FIREWORK_INDEX_SHOOTER.
    pub firework_shooter: Option<i32>,
    // Entity.FLAGS Bit 7: gleitet mit Elytra.
    pub gliding: bool,
    // LivingEntity.glidingTicks: Ticks seit Beginn des Gleitflugs, 0 am Boden. Daran haengt,
    // wie weit sich die Figur schon in die Flughaltung gelegt hat (s. entity_model::Glide).
    gliding_ticks: u32,
    // LivingEntity.swimAmount / swimAmountO: wie weit sich die Figur in die Schwimmhaltung
    // gelegt hat (0..1, je Tick 0.09 in der Pose SWIMMING hoch, sonst runter).
    swim_amount: f32,
    last_swim_amount: f32,
    // Entity.isInWater: im Wasser richtet sich die schwimmende Figur nach dem Blick, an Land
    // (Kriechen) liegt sie waagerecht. Clientseitig je Tick bestimmt wie in Vanillas baseTick.
    in_water: bool,
    // Entity.FLAGS Bit 0: brennt -- dann zeichnet Vanilla das Feuer um das Entity.
    pub on_fire: bool,
    // Der getragene Gegenstand von Item-Entities und Wurfgeschossen (s. item_render_kind),
    // mit Stapelgroesse -- die entscheidet beim liegenden Item, wie viele Kopien man sieht.
    // None = noch nichts empfangen; dann gilt die Vorgabe des Typs.
    pub item: Option<HeldItem>,
    pub item_count: i32,
    // XP-Kugel: ihr Wert (ExperienceOrb.DATA_VALUE), bestimmt das Bild.
    pub orb_value: i32,
    // ItemStackEntityRenderState.getSeed: Item-Id + Schaden. Streut die Kopien eines Stapels
    // immer gleich, damit sie nicht flackern.
    pub item_seed: i32,
    // Entity.FLAGS Bit 5. Ein unsichtbares Entity bekommt KEIN Modell, sein Namensschild
    // aber sehr wohl -- sonst verschwaende der Traeger eines Hologramms samt Text.
    pub invisible: bool,
    // Entity.FLAGS Bit 6: leuchtet -- Umriss in Teamfarbe, auch durch Waende.
    pub glowing: bool,
    // LivingEntity.LIVING_FLAGS: benutzt dieses Wesen gerade ein Item, und mit welcher Hand?
    // Daran haengt das gehobene Schild (s. blocking_hand).
    pub living_flags: u8,
    // PlayerLikeEntity.MAIN_ARM: Haupthand links? Dann haelt der LINKE Arm das Haupthand-Item
    // und die Nebenhand den rechten (s. arm_items) -- frueher war die Haupthand hier immer
    // rechts, und bei Linkshaendern sass z.B. das Essen da, wo Vanilla das Schild zeigt.
    pub left_handed: bool,
    // Nur bei den drei Display-Typen belegt (s. display_kind). Angelegt wird der Eintrag
    // schon beim Spawn, damit ein Display-Entity auch dann nicht als Kasten erscheint, wenn
    // sein Metadaten-Paket noch unterwegs ist.
    pub display: Option<DisplayInfo>,
    // ArmorStandEntity.ARMOR_STAND_FLAGS, nur bei Armorstaenden gefuellt. Entscheidet ueber
    // die Hoehe (s. armor_stand_height) und damit darueber, wo das Namensschild sitzt.
    armor_stand_flags: u8,
    // Gelenkwinkel in Grad, s. ARMOR_STAND_DEFAULT_POSES.
    pub armor_stand_poses: [[f32; 3]; 6],
}

// Grundhaltung eines Armorstands, solange der Server nichts anderes schickt (ArmorStand.
// DEFAULT_*_POSE): Kopf, Rumpf, linker Arm, rechter Arm, linkes Bein, rechtes Bein, je
// (x, y, z) in Grad.
pub const ARMOR_STAND_DEFAULT_POSES: [[f32; 3]; 6] =
    [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [-10.0, 0.0, -10.0], [-15.0, 0.0, 10.0], [-1.0, 0.0, -1.0], [1.0, 0.0, 1.0]];

// Wie Vanilla ein Entity mit getragenem Gegenstand zeichnet.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ItemRenderKind {
    // ItemEntityRenderer: liegend, wippend, drehend, je nach Stapel mehrere Kopien.
    Dropped,
    // FlyingItemEntityRenderer: zur Kamera gedreht, mit Skalierung. `default_item` ist der
    // Gegenstand, solange der Server keinen Stack geschickt hat (getDefaultItem).
    Flying { scale: f32, default_item: &'static str },
}

// EntityRendererFactories: welche Typen als Gegenstand gezeichnet werden. Die Feuerwerksrakete
// hat einen eigenen Renderer, zeigt aber ebenso nur ihr Item zur Kamera -- hier wie ein
// fliegendes Item behandelt.
pub fn item_render_kind(entity_type: i32) -> Option<ItemRenderKind> {
    let flying = |scale, default_item| Some(ItemRenderKind::Flying { scale, default_item });
    match entity_type_name(entity_type)? {
        "item" => Some(ItemRenderKind::Dropped),
        "egg" => flying(1.0, "egg"),
        "snowball" => flying(1.0, "snowball"),
        "ender_pearl" => flying(1.0, "ender_pearl"),
        "experience_bottle" => flying(1.0, "experience_bottle"),
        "splash_potion" => flying(1.0, "splash_potion"),
        "lingering_potion" => flying(1.0, "lingering_potion"),
        "eye_of_ender" => flying(1.0, "ender_eye"),
        "fireball" => flying(3.0, "fire_charge"),
        "small_fireball" => flying(0.75, "fire_charge"),
        "firework_rocket" => flying(1.0, "firework_rocket"),
        _ => None,
    }
}

// Ein Item in der Hand, reduziert auf das, was zum Zeichnen reicht. Bewusst nicht der ganze
// ItemStack: Anzahl, Haltbarkeit und Name spielen am Modell in der Hand keine Rolle, und so
// bleibt die Entity-Tabelle klein.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeldItem {
    // Index in der ITEM-Registry (s. item_registry::item_name) -- NICHT die Block-State-ID.
    pub item_id: u32,
    // Welches Item-MODELL gezeichnet wird: die item_model-Komponente, wenn sie auf ein
    // Vanilla-Item zeigt, sonst item_id. Server bauen so eigene Items -- der Golden Head auf
    // mcpvp.club ist eine Honigflasche (trinkbar) mit item_model player_head und Skin.
    // Spiellogik (Bogen, Schild, Essen) bleibt an item_id, nur das Bild folgt model_id.
    pub model_id: u32,
    // Glint-Schimmer (ItemStack.hasGlint, s. enchantment::has_glint) -- nicht nur "hat
    // Verzauberungen": auch Buch, Netherstern & Co. sowie der Override zaehlen.
    pub enchanted: bool,
    // Trankfarbe (potion::stack_color) -- ein Trank in der Hand ist sonst farblos.
    pub potion_color: Option<u32>,
    // Bei Spielerkoepfen: unter welchem Schluessel der Skin aus der profile-Komponente steht
    // (s. head_skin_key). None bei allen anderen Items.
    pub head_skin: Option<[u8; 16]>,
    // dyed_color (gefaerbtes Leder), 0xRRGGBB.
    pub dyed: Option<u32>,
    // trim: (Material-ID, Muster-ID), s. protocol::item::ItemStack::trim.
    pub trim: Option<(i32, i32)>,
    // Banner auf einem Schild (base_color / banner_patterns), s. ShieldBanner.
    pub banner: Option<ShieldBanner>,
    // Was es getragen zeigt (equippable-Component), s. armor::worn_asset.
    pub equip: crate::armor::Equip,
    // Der ganze Stapel fuer die Item-Modelle (s. item_components::StackRef).
    pub stack: crate::item_components::StackRef,
    // Gefuellte Karte: ihre map_id. Eigens hier und nicht nur im Stapel -- dessen Ring ist auf
    // einem vollen Server irgendwann ueberschrieben, und die Karte im Rahmen fiele dann aufs
    // gewoehnliche Item-Modell zurueck.
    pub map_id: Option<i32>,
}

// Das Banner eines Schildes, in Copy-Form fuer die Entity-Tabelle: Grundfarbe und bis zu acht
// Lagen (Index in der banner_pattern-Registry, Farbe) -- mehr zeigt auch Vanillas Banner
// nicht ueber normale Wege, und ein Server mit mehr verliert nur die obersten.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShieldBanner {
    pub base: u8,
    pub layers: [(u16, u8); 8],
    pub len: u8,
}

impl ShieldBanner {
    // Vanilla (ShieldModelRenderer) zeigt ein Banner, sobald Grundfarbe ODER Muster gesetzt
    // sind; fehlt die Grundfarbe, ist sie Weiss.
    pub fn from_stack(s: &protocol::item::ItemStack) -> Option<Self> {
        if s.base_color.is_none() && s.banner_patterns.is_empty() {
            return None;
        }
        let mut layers = [(0u16, 0u8); 8];
        let mut len = 0u8;
        for (id, farbe) in s.banner_patterns.iter().filter(|(id, _)| *id >= 0).take(8) {
            layers[len as usize] = (*id as u16, *farbe);
            len += 1;
        }
        Some(ShieldBanner { base: s.base_color.unwrap_or(0), layers, len })
    }

    pub fn layers(&self) -> &[(u16, u8)] {
        &self.layers[..self.len as usize]
    }
}

impl HeldItem {
    pub fn from_stack(s: &protocol::item::ItemStack) -> Self {
        HeldItem {
            item_id: s.item_id,
            model_id: model_item_id(s),
            enchanted: crate::enchantment::has_glint(s),
            potion_color: crate::potion::stack_color(s),
            head_skin: s.profile.as_ref().and_then(head_skin_key),
            dyed: s.dyed_color,
            trim: s.trim,
            banner: ShieldBanner::from_stack(s),
            equip: crate::armor::Equip::from_stack(s),
            stack: crate::item_components::intern(s),
            map_id: s.map_id,
        }
    }

    pub fn plain(item_id: u32) -> Self {
        HeldItem {
            item_id,
            model_id: item_id,
            enchanted: false,
            potion_color: None,
            head_skin: None,
            dyed: None,
            trim: None,
            banner: None,
            equip: crate::armor::Equip::Default,
            stack: crate::item_components::StackRef::NONE,
            map_id: None,
        }
    }
}

// Blockt dieses Hand-Item (blocks_attacks)? Ueber den ganzen Stapel, sonst nach Namen.
pub fn held_blocks_attacks(h: &HeldItem) -> Option<protocol::item::BlocksAttacks> {
    match crate::item_components::resolve(h.stack) {
        Some(s) => crate::item_behavior::blocks_attacks(&s),
        None => (crate::item_registry::item_name(h.item_id) == Some("shield"))
            .then_some(protocol::item::BlocksAttacks { block_delay_seconds: 0.25, disable_cooldown_scale: 1.0 }),
    }
}

// BlocksAttacks.blockDelayTicks: die Verzoegerung auf ganze Ticks gerundet (Schild: 5).
pub fn block_delay_ticks(b: &protocol::item::BlocksAttacks) -> u32 {
    (b.block_delay_seconds * 20.0).round().max(0.0) as u32
}

// Gegenstandsrahmen (auch der leuchtende)?
pub fn is_item_frame(entity_type: i32) -> bool {
    matches!(entity_type_name(entity_type), Some("item_frame" | "glow_item_frame"))
}

// Sticht dieses Hand-Item (piercing_weapon)? Ueber den ganzen Stapel, sonst nach Namen.
pub fn held_piercing(h: &HeldItem) -> bool {
    match crate::item_components::resolve(h.stack) {
        Some(s) => crate::item_behavior::piercing(&s),
        None => crate::item_registry::item_name(h.item_id).is_some_and(crate::item_registry::is_spear),
    }
}

// Item-ID des Modells, das die item_model-Komponente nennt (s. HeldItem::model_id). Nennt sie
// eine Item-Definition, die es in Vanilla nicht gibt (Server-Resourcepacks: "uhc:storm_wall"
// steht in assets/uhc/items/storm_wall.json), bekommt sie eine eigene Nummer ab
// CUSTOM_MODEL_BASE (s. custom_model_id) -- frueher fiel so ein Modell auf das Item selbst
// zurueck, und eine Sturmwand-Markierung erschien als Trankflasche.
pub fn model_item_id(s: &protocol::item::ItemStack) -> u32 {
    match s.item_model.as_deref() {
        None => s.item_id,
        Some(m) => crate::item_registry::item_id_by_name(m.strip_prefix("minecraft:").unwrap_or(m)).unwrap_or_else(|| custom_model_id(m)),
    }
}

// Ab hier zaehlen Modell-Nummern als Item-Definitionen aus Resourcepacks, nicht als Items der
// Registry (die hat rund 1500).
pub const CUSTOM_MODEL_BASE: u32 = 1 << 20;

static CUSTOM_MODELS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

// Die Nummer einer Item-Definition aus einem Resourcepack ("ns:pfad"), beim ersten Auftauchen
// vergeben und dann fest.
pub fn custom_model_id(id: &str) -> u32 {
    let id = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    let mut liste = CUSTOM_MODELS.lock().unwrap_or_else(|e| e.into_inner());
    let index = match liste.iter().position(|m| *m == id) {
        Some(i) => i,
        None => {
            liste.push(id);
            liste.len() - 1
        }
    };
    CUSTOM_MODEL_BASE + index as u32
}

// Die ID ("ns:pfad") zu einer Nummer aus custom_model_id.
pub fn custom_model_name(model_id: u32) -> Option<String> {
    let index = model_id.checked_sub(CUSTOM_MODEL_BASE)? as usize;
    CUSTOM_MODELS.lock().unwrap_or_else(|e| e.into_inner()).get(index).cloned()
}

// Unter welchem Schluessel der Skin eines Spielerkopfes steht: die UUID aus der
// profile-Komponente, ohne sie ein Hash der Textur-Property (viele Server-Koepfe tragen nur
// die Textur). None, wenn der Kopf gar keinen Skin nennt.
pub fn head_skin_key(profil: &protocol::item::HeadProfile) -> Option<[u8; 16]> {
    if let Some(uuid) = profil.uuid.filter(|u| *u != [0; 16]) {
        return Some(uuid);
    }
    // Ohne UUID: die Textur, sonst der Name (ein Kopf, den der Client erst ueber die
    // Mojang-Profile aufloesen muss, s. auth::resolve_profile_textures). Eigene Salze, damit
    // ein Name nie denselben Schluessel ergibt wie eine gleichlautende Textur.
    let (salze, inhalt) = match (&profil.textures, &profil.name) {
        (Some(t), _) => ([0x6b6f_7066_u64, 0x0073_6368_6164_656c], t.clone()),
        (None, Some(n)) if !n.is_empty() => ([0x6e61_6d65_u64, 0x006b_6f70_666e_616d], n.to_ascii_lowercase()),
        _ => return None,
    };
    use std::hash::{Hash, Hasher};
    let mut key = [0u8; 16];
    for (i, salz) in salze.into_iter().enumerate() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        salz.hash(&mut h);
        inhalt.hash(&mut h);
        key[i * 8..i * 8 + 8].copy_from_slice(&h.finish().to_be_bytes());
    }
    Some(key)
}

impl Entity {
    /// Schleicht dieses Entity gerade? Vanillas `isInSneakingPose()` -- der Zustand haengt an
    /// der POSE, nicht am gedrueckten Shift: wer schleicht, aber wegen einer Decke ueber sich
    /// nicht aufstehen kann, ist ebenfalls CROUCHING.
    pub fn sneaking(&self) -> bool {
        self.pose == POSE_CROUCHING
    }

    // Stand das Entity beim letzten Tick im Wasser (s. touches_water)?
    pub fn is_in_water(&self) -> bool {
        self.in_water
    }

    // TamableAnimal.isInSittingPose (nur bei Katzen gepflegt, s. apply_metadata).
    pub fn sitting(&self) -> bool {
        self.tamable_flags & protocol::packets::TAMABLE_FLAG_SITTING != 0
    }

    // Benutzt dieses Wesen gerade ein Item, und wenn ja: mit dem RECHTEN Arm?
    //
    // Vanilla (LivingEntity.getActiveHand) liest die Hand aus demselben Byte: Bit 2 gesetzt
    // heisst Nebenhand. Welcher Arm das ist, sagt die Haupthand (s. left_handed).
    pub fn using_item_right_arm(&self) -> Option<bool> {
        let haupthand = self.using_main_hand()?;
        Some(haupthand != self.left_handed)
    }

    // Das Item, das gerade benutzt wird, und seit wie vielen Ticks (s. use_ticks).
    pub fn used_item(&self) -> Option<(HeldItem, u32)> {
        let item = if self.using_main_hand()? { self.main_hand } else { self.off_hand };
        Some((item?, self.use_ticks))
    }

    // Benutzt dieses Wesen gerade ein Item, und wenn ja: das der Haupthand?
    fn using_main_hand(&self) -> Option<bool> {
        if self.living_flags & protocol::packets::LIVING_FLAG_USING_ITEM == 0 {
            return None;
        }
        Some(self.living_flags & protocol::packets::LIVING_FLAG_OFFHAND_ACTIVE == 0)
    }

    // Was der rechte und was der linke Arm haelt (ArmedEntityRenderState: die Haupthand am
    // Arm der Haupthand, die Nebenhand am anderen).
    pub fn arm_items(&self) -> (Option<HeldItem>, Option<HeldItem>) {
        if self.left_handed { (self.off_hand, self.main_hand) } else { (self.main_hand, self.off_hand) }
    }

    // Speerhaltung je Arm (rechts, links), s. main_hand_spear/off_hand_spear.
    pub fn arm_spears(&self) -> (bool, bool) {
        let (haupt, neben) = (self.main_hand_spear(), self.off_hand_spear());
        if self.left_handed { (neben, haupt) } else { (haupt, neben) }
    }

    // REIN OPTISCH: wie weit das Modell des Projektils gerade noch von seiner echten
    // Position weg gezeichnet wird, damit es aus der Hand des Schuetzen zu kommen scheint
    // statt aus dessen Brustmitte.
    //
    // Vanilla tut das NICHT -- dort spawnt der Pfeil auf Augenhoehe genau in der
    // Koerperachse (Projectile.shootFromRotation auf `player.getX(), getEyeY() - 0.1,
    // getZ()`), und genau das sieht man dem Flug an. Der Versatz ist deshalb reine
    // Darstellung: die Flugbahn, die Trefferbox und alles, was zum Server geht, bleiben auf
    // der echten Position. Er geht ueber LAUNCH_BLEND_TICKS auf Null zurueck, quadratisch,
    // damit er am Ende ohne Knick ankommt.
    // Steckt das Geschoss (Pfeil im Block)? Bei allem anderen immer false.
    // Breite und Hoehe der Trefferbox: wie hit_box_size, nur traegt ein interaction-Entity
    // seine Groesse in den Metadaten (s. interaction_size) -- damit machen Server Display-
    // Entities anklickbar und schlagbar. Mit den festen Typmassen (0 x 0) liess es sich nie
    // treffen (etwa auf mcpvp.club).
    pub fn hit_size(&self) -> (f64, f64) {
        if entity_type_name(self.entity_type) == Some("interaction") {
            self.interaction_size
        } else {
            hit_box_size(self.entity_type, self.pose)
        }
    }

    pub fn in_ground(&self) -> bool {
        self.in_ground
    }

    pub fn launch_render_offset(&self, tick_progress: f32) -> [f32; 3] {
        if self.launch_ticks == 0 {
            return [0.0; 3];
        }
        let rest = (self.launch_ticks as f32 - tick_progress.clamp(0.0, 1.0)).max(0.0) / LAUNCH_BLEND_TICKS as f32;
        let f = rest * rest;
        [self.launch_offset.0 * f, self.launch_offset.1 * f, self.launch_offset.2 * f]
    }

    // Bild-Position: zwischen dem Stand zu Tickbeginn und dem aktuellen Stand, mit dem
    // Bruchteil des laufenden Ticks -- Vanillas Entity.getLerpedPos(deltaTicks).
    // (Breite, Hoehe) der Hitbox samt Baby-Halbierung (LivingEntity.getScaleFactor: 0.5).
    pub fn box_size(&self) -> (f64, f64) {
        let (w, h) = hit_box_size(self.entity_type, self.pose);
        if self.baby {
            (w * 0.5, h * 0.5)
        } else {
            (w, h)
        }
    }

    pub fn render_pos(&self, tick_progress: f32) -> (f64, f64, f64) {
        let t = tick_progress.clamp(0.0, 1.0) as f64;
        (
            self.last_pos.0 + (self.pos.0 - self.last_pos.0) * t,
            self.last_pos.1 + (self.pos.1 - self.last_pos.1) * t,
            self.last_pos.2 + (self.pos.2 - self.last_pos.2) * t,
        )
    }

    // Drehung des Modells fuers Bild: bei Lebewesen die KOERPERdrehung (LivingEntityRenderer:
    // lerpAngleDegrees(delta, lastBodyYaw, bodyYaw)), sonst getLerpedYaw. Ueber den Winkel-Lerp,
    // damit der Sprung 359 -> 1 nicht einmal komplett herum geht.
    pub fn render_yaw(&self, tick_progress: f32) -> f32 {
        let t = tick_progress.clamp(0.0, 1.0);
        if is_living(self.entity_type) {
            lerp_angle(t, self.last_body_yaw_deg, self.body_yaw_deg)
        } else {
            lerp_angle(t, self.last_yaw_deg, self.yaw_deg)
        }
    }

    // Blickrichtung fuer eine Kamera in diesem Entity (Entity.getViewYRot): die Drehung des
    // Entity selbst, nicht die des Koerpers.
    pub fn view_yaw(&self, tick_progress: f32) -> f32 {
        lerp_angle(tick_progress.clamp(0.0, 1.0), self.last_yaw_deg, self.yaw_deg)
    }

    // Augenhoehe ueber den Fuessen: bei Spielern je Pose, sonst der Vanilla-Standard von 85 %
    // der Hoehe (einzelne Typen weichen in Vanilla davon ab).
    pub fn eye_height(&self) -> f64 {
        if is_player(self.entity_type) {
            return match self.pose {
                POSE_SLEEPING => 0.2,
                POSE_SWIMMING | POSE_GLIDING | POSE_SPIN_ATTACK => 0.4,
                POSE_CROUCHING => 1.27,
                _ => 1.62,
            };
        }
        self.box_size().1 * 0.85
    }

    pub fn render_pitch(&self, tick_progress: f32) -> f32 {
        self.last_pitch_deg + (self.pitch_deg - self.last_pitch_deg) * tick_progress.clamp(0.0, 1.0)
    }

    // Trefferposition = die tick-quantisierte Position. Seit die Bild-Interpolation dem
    // Vanilla-Modell folgt, ist das DIESELBE Groesse, auf der auch das Bild sitzt (bis auf
    // den Zwischentick-Anteil) -- Vanilla hat hier ebenfalls keine zwei getrennten Werte,
    // ProjectileUtil.raycast schiesst gegen entity.getBoundingBox() an genau dieser Position.
    //
    // Gemeint ist die UNTERKANTE der Box (Mitte unten). Bei fast allem ist das die Position;
    // Windkugeln haengen ihre Box 0.15 tiefer (AbstractWindChargeEntity.
    // calculateDefaultBoundingBox), damit sie mittig um das Modell liegt.
    pub fn hit_pos(&self) -> (f64, f64, f64) {
        let (x, y, z) = self.pos;
        (x, y + self.box_y_offset(), z)
    }

    // Versatz der Box-Unterkante gegenueber der Position (s. hit_pos).
    pub fn box_y_offset(&self) -> f64 {
        match crate::entity_type_name(self.entity_type) {
            Some("wind_charge" | "breeze_wind_charge") => -(0.15f32 as f64),
            _ => 0.0,
        }
    }

    // Ein CLIENT-Tick dieses Entities. Reihenfolge wie in Vanilla:
    //   Entity.baseTick      -> tickLastPos()      (last_* = aktueller Stand)
    //   LivingEntity.tick    -> interpolator.tick() (ein Schritt Richtung Ziel)
    //   LivingEntity.tick    -> lerpHeadYaw()       (ein Schritt der Kopfdrehung)
    // Danach die rein clientseitigen Animationen, die Vanilla ebenfalls pro Tick aus der in
    // DIESEM Tick zurueckgelegten Strecke speist (LimbAnimator, Umhang).
    fn tick(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, spieler: &[OrbPlayer]) {
        self.last_pos = self.pos;
        self.last_yaw_deg = self.yaw_deg;
        self.last_pitch_deg = self.pitch_deg;
        self.last_head_yaw_deg = self.head_yaw_deg;
        self.last_body_yaw_deg = self.body_yaw_deg;
        // LivingEntity.baseTick, allererste Zeile.
        self.last_hand_swing_progress = self.hand_swing_progress;

        // Projektile fliegen selbst, alle anderen werden zur Serverposition interpoliert.
        match self.projectile {
            // Selbst gelenkt: die Stelle, die der Client eben ausgerechnet hat (s. driven).
            None if self.driven => {
                if let Some((pos, yaw, pitch)) = self.driven_next.take() {
                    self.pos = pos;
                    self.target_pos = pos;
                    self.yaw_deg = yaw;
                    self.target_yaw_deg = yaw;
                    self.pitch_deg = pitch;
                    self.target_pitch_deg = pitch;
                }
                self.interp_steps = 0;
            }
            Some(kind) => self.tick_projectile(kind, world, shapes),
            None if self.item_physics => self.tick_item(world, shapes),
            None if self.block_physics => self.tick_falling_block(world, shapes),
            None if self.orb_physics => self.tick_orb(world, shapes, spieler),
            None => self.tick_interpolation(),
        }
        self.tick_head_yaw();
        self.tick_animations();
        // Erst NACH tickMovement (Bewegung, Handschwung), wie in LivingEntity.tick.
        self.tick_body_yaw();
        // LivingEntity.tick: `if (isGliding()) glidingTicks++; else glidingTicks = 0;`
        self.gliding_ticks = if self.gliding { self.gliding_ticks.saturating_add(1) } else { 0 };
        // LivingEntity.updateSwimAmount -- haengt an isVisuallySwimming, also nur an der Pose.
        self.last_swim_amount = self.swim_amount;
        let step = if self.pose == POSE_SWIMMING { 0.09 } else { -0.09 };
        self.swim_amount = (self.swim_amount + step).clamp(0.0, 1.0);
        self.in_water = self.touches_water(world);
        self.age_ticks += 1.0;
        // LivingEntity.tick: `if (hurtTime > 0) hurtTime--;` -- ein Tick, ein Schritt.
        self.hurt_ticks = self.hurt_ticks.saturating_sub(1);
        self.ticks_since_hurt = self.ticks_since_hurt.saturating_add(1);
        self.crit_grace = self.crit_grace.saturating_sub(1);
    }

    // Der Server hat mitgeteilt, dass dieses Entity Schaden genommen hat
    // (EntityDamageS2CPacket -> Entity.onDamaged). Nur Lebewesen blitzen auf, s. is_living.
    fn on_damaged(&mut self) {
        if is_living(self.entity_type) {
            self.hurt_ticks = MAX_HURT_TIME;
            self.ticks_since_hurt = 0;
            // Ein Crit, der eben erst angekommen ist, gehoert zu DIESEM Treffer. Ohne das
            // Zuruecksetzen bliebe die Crit-Markierung am naechsten, gewoehnlichen Treffer
            // haengen.
            self.hurt_by_crit = self.crit_grace > 0;
        }
    }

    // Der Server meldet einen kritischen Treffer auf dieses Entity (Animation CRIT).
    fn on_crit(&mut self) {
        // Der Normalfall: das Schadenspaket kam schon, der Blitz laeuft bereits.
        if self.hurt_ticks > 0 {
            self.hurt_by_crit = true;
        }
        self.crit_grace = CRIT_GRACE_TICKS;
    }

    // Blitzt das Entity gerade IN CRIT-FARBE? Immer nur zusammen mit is_hurt() zu gebrauchen:
    // die Markierung ueberlebt den Blitz, ausgewertet wird sie nur waehrend er laeuft.
    pub fn is_hurt_by_crit(&self) -> bool {
        self.hurt_ticks > 0 && self.hurt_by_crit
    }

    // Blitzt das Entity gerade rot? Vanillas Renderer fragt `hurtTime > 0 || deathTime > 0` --
    // den Todes-Anteil kennen wir nicht (deathTime wird nie ueber das Netz geschickt, Vanilla
    // zaehlt ihn lokal nach dem Tod-Status hoch), der Schadens-Anteil ist derselbe.
    pub fn is_hurt(&self) -> bool {
        self.hurt_ticks > 0
    }

    // Blitzt das Entity bei einer Blitzdauer von `ticks` gerade auf? Bei 10 dasselbe wie
    // is_hurt(); die Dauer ist einstellbar (Settings::damage_tint_ticks).
    pub fn damage_flash(&self, ticks: u32) -> bool {
        self.ticks_since_hurt < ticks
    }

    // Stammt der letzte Treffer von einem Crit? Nur zusammen mit damage_flash auszuwerten.
    pub fn hurt_by_crit(&self) -> bool {
        self.hurt_by_crit
    }

    // PositionInterpolator.tick(): der Lerp-Nenner ist die Zahl der NOCH offenen Schritte,
    // nicht die Gesamtzahl -- damit ist der letzte Schritt eine volle Angleichung und die
    // Position landet exakt auf dem Ziel, statt sich ihm nur zu naehern.
    //
    // Vanilla addiert an dieser Stelle zusaetzlich die Strecke, die das Entity im selben Tick
    // SELBST zurueckgelegt hat (der `lastPos`-Zweig, fuer Fahrzeuge, die der Client mit
    // simuliert). Fuer fremde Entities simulieren wir nichts -- diese Differenz ist bei uns
    // immer null, der Zweig damit wirkungslos und bewusst weggelassen.
    fn tick_interpolation(&mut self) {
        if self.interp_steps == 0 {
            return;
        }
        let d = 1.0 / self.interp_steps as f64;
        self.pos = (
            self.pos.0 + (self.target_pos.0 - self.pos.0) * d,
            self.pos.1 + (self.target_pos.1 - self.pos.1) * d,
            self.pos.2 + (self.target_pos.2 - self.pos.2) * d,
        );
        self.yaw_deg = lerp_angle(d as f32, self.yaw_deg, self.target_yaw_deg);
        self.pitch_deg += (self.target_pitch_deg - self.pitch_deg) * d as f32;
        self.interp_steps -= 1;
    }

    // Ein Tick Projektilflug -- s. ProjectileKind fuer Herkunft und Reihenfolge.
    fn tick_projectile(&mut self, kind: ProjectileKind, world: &crate::chunk::World, shapes: &crate::BlockShapes) {
        self.launch_ticks = self.launch_ticks.saturating_sub(1);
        // AbstractArrow.tick: steht der Pfeil in der Kollisionsform seines Blocks, steckt er.
        if kind == ProjectileKind::Arrow && !self.in_ground && point_in_collision(self.pos, world, shapes) {
            self.vel = (0.0, 0.0, 0.0);
            self.in_ground = true;
        }
        if self.in_ground {
            // AbstractArrow: steckt der Pfeil, ruehrt sich nichts mehr. Ein Wurfgeschoss ist
            // an der Stelle schon zu Ende, der Server raeumt es gleich weg.
            return;
        }
        let im_wasser = self.in_water(world);
        match kind {
            ProjectileKind::Throwable => {
                // applyGravity() VOR applyInertia() -- beides vor der Bewegung.
                self.vel.1 -= kind.gravity();
                let f = if im_wasser { kind.water_inertia() } else { 0.99 };
                self.vel = (self.vel.0 * f, self.vel.1 * f, self.vel.2 * f);
                self.move_projectile(world, shapes, false);
            }
            // AbstractHurtingProjectile.tick: applyInertia (hier wirkungslos), dann die Bewegung
            // bis zum ersten Block -- dort explodiert sie auf dem Server und wird entfernt.
            ProjectileKind::WindCharge => self.move_projectile(world, shapes, false),
            ProjectileKind::Arrow => {
                // Die Wasserreibung steht VOR der Bewegung, die Luftreibung DAHINTER.
                if im_wasser {
                    let f = kind.water_inertia();
                    self.vel = (self.vel.0 * f, self.vel.1 * f, self.vel.2 * f);
                }
                self.move_projectile(world, shapes, true);
                if !im_wasser {
                    self.vel = (self.vel.0 * 0.99, self.vel.1 * 0.99, self.vel.2 * 0.99);
                }
                if !self.in_ground {
                    self.vel.1 -= kind.gravity();
                }
            }
        }
        // Projectile.updateRotation: die Nase zeigt in Flugrichtung.
        let waagerecht = (self.vel.0 * self.vel.0 + self.vel.2 * self.vel.2).sqrt();
        if waagerecht > 1.0e-7 || self.vel.1.abs() > 1.0e-7 {
            let ziel_yaw = self.vel.0.atan2(self.vel.2).to_degrees() as f32;
            let ziel_pitch = self.vel.1.atan2(waagerecht).to_degrees() as f32;
            self.yaw_deg = lerp_rotation(self.yaw_deg, ziel_yaw);
            self.pitch_deg = lerp_rotation(self.pitch_deg, ziel_pitch);
            self.target_yaw_deg = self.yaw_deg;
            self.target_pitch_deg = self.pitch_deg;
        }
        // Die Vorhersage IST ab jetzt die Wahrheit, bis der Server widerspricht -- sonst
        // zoege tick_interpolation das Projektil beim naechsten Paket wieder zurueck.
        //
        // tracked_pos bleibt dagegen die SERVER-Position: auf sie rechnet der Server seine
        // relativen Bewegungspakete (EntityTrackerEntry: Delta zur zuletzt GESENDETEN
        // Position). ECHTER BUG gewesen: hier stand auch tracked_pos = pos. Jedes Delta landete
        // dann auf der schon vorausgeflogenen Vorhersage -- die Bewegung zaehlte doppelt.
        // Windkugeln (alle zehn Ticks ein Paket) schossen davon und flogen weit ueber die
        // Trackingweite hinaus, Pfeile blieben mit dem letzten Paket vor der Wand in der
        // Luft stehen.
        self.target_pos = self.pos;
        self.interp_steps = 0;
    }

    // ItemEntity.tick, der Teil, den auch der Client rechnet: Auftrieb in Wasser/Lava oder
    // Schwerkraft 0.04, dann -- solange es faellt, rutscht oder jeden vierten Tick zur Probe --
    // die Bewegung mit Blockkollision, Reibung 0.98 (am Boden mal Rutschigkeit des Blocks
    // darunter) und Aufprall (onEntityLand: vy = 0).
    fn tick_item(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes) {
        let (x, y, z) = self.pos;
        // Ohne geladene Spalte gibt es keinen Boden -- dann nicht ins Leere fallen lassen,
        // sondern auf die naechste Serverposition warten.
        if world.column((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16)).is_none() {
            return;
        }
        let fluid = crate::fluid::fluid_kind(world.block_at(x.floor() as i32, (y + 0.1).floor() as i32, z.floor() as i32));
        match fluid {
            Some(crate::fluid::FluidKind::Water) | Some(crate::fluid::FluidKind::Lava) => {
                let f = if fluid == Some(crate::fluid::FluidKind::Water) { 0.99 } else { 0.95 };
                let auftrieb = if self.vel.1 < 0.06 { 5.0e-4 } else { 0.0 };
                self.vel = (self.vel.0 * f, self.vel.1 + auftrieb, self.vel.2 * f);
            }
            None => self.vel.1 -= 0.04,
        }
        let waagerecht = self.vel.0 * self.vel.0 + self.vel.2 * self.vel.2;
        if !self.on_ground || waagerecht > 1.0e-5 || (self.age_ticks as u32).is_multiple_of(4) {
            let (neu, stoss) = move_with_collisions(self.pos, self.vel, 0.125, 0.25, world, shapes);
            self.on_ground = stoss[1] && self.vel.1 < 0.0;
            self.pos = neu;
            if stoss[0] {
                self.vel.0 = 0.0;
            }
            if stoss[2] {
                self.vel.2 = 0.0;
            }
            if stoss[1] {
                self.vel.1 = 0.0;
            }
            let f = if self.on_ground {
                let unten = world.block_at(self.pos.0.floor() as i32, (self.pos.1 - 0.500001).floor() as i32, self.pos.2.floor() as i32);
                block_slipperiness(unten) * 0.98
            } else {
                0.98
            };
            self.vel = (self.vel.0 * f, self.vel.1 * 0.98, self.vel.2 * f);
        }
        // tracked_pos bleibt die Serverbasis fuer relative Pakete (s. tick_projectile).
        self.target_pos = self.pos;
        self.interp_steps = 0;
    }

    // ExperienceOrb.tick, wie ihn auch der Client rechnet. Zuerst ein Schritt zur Serverposition
    // (Orbs haben in Vanilla einen eigenen Interpolator), dann die eigene Bewegung:
    // - unter Wasser (Augenhoehe 0.425) Auftrieb, sonst Schwerkraft 0.03 -- nicht, solange er
    //   in einem Block steckt;
    // - Lava wirft ihn hoch (in Vanilla mit zufaelligem seitlichem Anteil um null, den der
    //   Server mit seinem eigenen Zufall bestimmt -- hier nur der senkrechte Teil);
    // - Sog zum verfolgten Spieler: (1 - Abstand/8)^2 * 0.1 in Richtung seiner halben
    //   Augenhoehe. Verfolgt wird der naechste Spieler unter 8 Bloecken, bis er weiter weg ist;
    // - Bewegung mit Blockkollision (Hitbox 0,5), Reibung 0.98 auf allen Achsen, am Boden
    //   mal Rutschigkeit des Blocks darunter;
    // - kam er schneller als die Schwerkraft auf, springt er mit 0.4 der Fallgeschwindigkeit
    //   zurueck.
    fn tick_orb(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, spieler: &[OrbPlayer]) {
        self.tick_interpolation();
        let (x, y, z) = self.pos;
        if world.column((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16)).is_none() {
            return;
        }
        let steckt = point_in_collision((x, y + 0.25, z), world, shapes);
        let block = |by: f64| world.block_at(x.floor() as i32, by.floor() as i32, z.floor() as i32);
        let fluid = |by: f64| crate::fluid::fluid_kind(block(by));
        if fluid(y + 0.425) == Some(crate::fluid::FluidKind::Water) {
            self.vel = (self.vel.0 * 0.99, (self.vel.1 + 5.0e-4).min(0.06), self.vel.2 * 0.99);
        } else if !steckt {
            self.vel.1 -= ORB_GRAVITY;
        }
        if fluid(y) == Some(crate::fluid::FluidKind::Lava) {
            self.vel = (0.0, 0.2, 0.0);
        }
        // Dem bisherigen Spieler folgen, solange er unter 8 Bloecken bleibt, sonst den naechsten
        // suchen.
        let abstand2 = |p: &OrbPlayer| (p.pos.0 - x).powi(2) + (p.pos.1 - y).powi(2) + (p.pos.2 - z).powi(2);
        let bisher = self.orb_target.and_then(|id| spieler.iter().find(|p| p.id == id)).filter(|p| abstand2(p) <= ORB_FOLLOW_RANGE * ORB_FOLLOW_RANGE);
        let ziel = bisher.or_else(|| {
            spieler.iter().filter(|p| abstand2(p) < ORB_FOLLOW_RANGE * ORB_FOLLOW_RANGE).min_by(|a, b| abstand2(a).total_cmp(&abstand2(b)))
        });
        self.orb_target = ziel.map(|p| p.id);
        if let Some(p) = ziel {
            let d = (p.pos.0 - x, p.pos.1 + p.eye_height / 2.0 - y, p.pos.2 - z);
            let laenge = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt();
            if laenge > 1.0e-4 {
                let sog = (1.0 - laenge / ORB_FOLLOW_RANGE).powi(2) * 0.1 / laenge;
                self.vel = (self.vel.0 + d.0 * sog, self.vel.1 + d.1 * sog, self.vel.2 + d.2 * sog);
            }
        }
        let vy = self.vel.1;
        let (neu, stoss) = move_with_collisions(self.pos, self.vel, 0.25, 0.5, world, shapes);
        let aufgekommen = stoss[1] && vy < 0.0;
        self.on_ground = aufgekommen;
        self.pos = neu;
        if stoss[0] {
            self.vel.0 = 0.0;
        }
        if stoss[2] {
            self.vel.2 = 0.0;
        }
        if stoss[1] {
            self.vel.1 = 0.0;
        }
        let f = if self.on_ground {
            let unten = world.block_at(self.pos.0.floor() as i32, (self.pos.1 - 0.999999).floor() as i32, self.pos.2.floor() as i32);
            block_slipperiness(unten) * 0.98
        } else {
            0.98
        };
        self.vel = (self.vel.0 * f, self.vel.1 * f, self.vel.2 * f);
        if aufgekommen && vy < -ORB_GRAVITY {
            self.vel.1 = -vy * 0.4;
        }
        // Ist die Angleichung an die letzte Meldung durch, gilt die eigene Bahn (s. tick_item).
        if self.interp_steps == 0 {
            self.target_pos = self.pos;
        }
    }

    // FallingBlockEntity.tick, der Teil, den auch der Client rechnet: Schwerkraft 0.04, Bewegung
    // mit Blockkollision (Hitbox 0,98 x 0,98), danach Reibung 0.98. Abgesetzt wird der Block
    // vom Server -- der schickt dann das Block-Update und entfernt das Entity.
    fn tick_falling_block(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes) {
        let (x, z) = (self.pos.0.floor() as i32, self.pos.2.floor() as i32);
        if world.column(x.div_euclid(16), z.div_euclid(16)).is_none() {
            return;
        }
        self.vel.1 -= 0.04;
        let (neu, stoss) = move_with_collisions(self.pos, self.vel, 0.49, 0.98, world, shapes);
        self.on_ground = stoss[1] && self.vel.1 < 0.0;
        self.pos = neu;
        if stoss[0] {
            self.vel.0 = 0.0;
        }
        if stoss[2] {
            self.vel.2 = 0.0;
        }
        if stoss[1] {
            self.vel.1 = 0.0;
        }
        self.vel = (self.vel.0 * 0.98, self.vel.1 * 0.98, self.vel.2 * 0.98);
        self.target_pos = self.pos;
        self.interp_steps = 0;
    }

    // Steht der Mittelpunkt des Projektils in einer Fluessigkeit? Vanilla fragt isInWater()
    // gegen die ganze Hitbox; fuer ein Geschoss von 0.25 Bloecken Kantenlaenge ist der
    // Mittelpunkt nah genug und spart den Boxdurchlauf pro Tick.
    fn in_water(&self, world: &crate::chunk::World) -> bool {
        let (x, y, z) = (self.pos.0.floor() as i32, self.pos.1.floor() as i32, self.pos.2.floor() as i32);
        matches!(crate::fluid::fluid_kind(world.block_at(x, y, z)), Some(crate::fluid::FluidKind::Water))
    }

    // Ein Bewegungsschritt samt Blocktreffer. Vanilla schiesst dafuer einen Strahl von der
    // aktuellen Position zur angestrebten (ClipContext mit Block.COLLIDER) und bleibt am
    // Treffer stehen; der Pfeil steckt dann, das Wurfgeschoss zerplatzt dort.
    // `pfeil`: AbstractArrow.onBlockHit schiebt den Pfeil danach um 0.05 je Achse gegen die
    // Flugrichtung zurueck (Vorzeichen der Geschwindigkeit) -- die Position liegt knapp vor
    // der Flaeche, die Spitze des Modells (4 Pixel voraus) steckt im Block.
    fn move_projectile(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, pfeil: bool) {
        let ziel = (self.pos.0 + self.vel.0, self.pos.1 + self.vel.1, self.pos.2 + self.vel.2);
        if let Some(t) = block_hit_along(self.pos, self.vel, world, shapes) {
            self.pos = (self.pos.0 + self.vel.0 * t, self.pos.1 + self.vel.1 * t, self.pos.2 + self.vel.2 * t);
            if pfeil {
                let schub = |v: f64| if v > 0.0 { 0.05f32 as f64 } else if v < 0.0 { -(0.05f32 as f64) } else { 0.0 };
                self.pos = (self.pos.0 - schub(self.vel.0), self.pos.1 - schub(self.vel.1), self.pos.2 - schub(self.vel.2));
            }
            self.vel = (0.0, 0.0, 0.0);
            self.in_ground = true;
            return;
        }
        self.pos = ziel;
    }

    // LivingEntity.tick: Koerper zur Laufrichtung (s. entity_model::turn_body). Nur fuer
    // Lebewesen -- ein Boot, eine Lore oder ein Display dreht sich genau so, wie der Server sagt.
    fn tick_body_yaw(&mut self) {
        self.use_ticks = if self.using_item_right_arm().is_some() { self.use_ticks.saturating_add(1) } else { 0 };
        if !is_living(self.entity_type) {
            self.body_yaw_deg = self.yaw_deg;
            return;
        }
        let max_relative = if self.is_blocking() {
            crate::entity_model::BLOCKING_MAX_RELATIVE
        } else {
            crate::entity_model::BodyYaw::MAX_RELATIVE
        };
        let (dx, dz) = (self.pos.0 - self.last_pos.0, self.pos.2 - self.last_pos.2);
        self.body_yaw_deg = crate::entity_model::turn_body(self.body_yaw_deg, self.yaw_deg, dx, dz, self.hand_swing_progress > 0.0, max_relative);
    }

    // PlayerEntity.isBlocking: ein Spieler, der einen Schild benutzt, und zwar schon laenger als
    // dessen Block-Verzoegerung (BlocksAttacksComponent, Schild: 0,25 s = 5 Ticks). Nur hierfuer
    // gebraucht -- das Hochreissen im BILD bleibt absichtlich sofort (s. coral-client-schild-sofort).
    fn is_blocking(&self) -> bool {
        let Some(haupthand) = self.using_main_hand() else { return false };
        let item = if haupthand { self.main_hand } else { self.off_hand };
        let Some(blockt) = item.and_then(|i| held_blocks_attacks(&i)) else { return false };
        is_player(self.entity_type) && self.use_ticks >= block_delay_ticks(&blockt)
    }

    // LivingEntity.tick(): lerpAngleDegrees(1/headTrackingIncrements, headYaw, serverHeadYaw),
    // danach herunterzaehlen.
    fn tick_head_yaw(&mut self) {
        if self.head_steps == 0 {
            return;
        }
        self.head_yaw_deg = lerp_angle(1.0 / self.head_steps as f32, self.head_yaw_deg, self.server_head_yaw_deg);
        self.head_steps -= 1;
    }

    // Schrittanimation und Umhang -- beides rechnet Vanilla pro Tick aus der Strecke dieses
    // Ticks (LivingEntity.tickMovement -> LimbAnimator.updateLimbs mit getX() - lastX).
    //
    // Frueher lief das hier beim PAKETEINTREFFEN und musste die Zahl der verstrichenen Ticks
    // aus der Wanduhr schaetzen, weil die Update-Rate des Trackers (10 Hz / 6.7 Hz) nicht die
    // Tickrate ist. Seit die Position tick-quantisiert ist, entfaellt die Schaetzung: die
    // Strecke pro Tick steht direkt da.
    fn tick_animations(&mut self) {
        let (dx, dz) = ((self.pos.0 - self.last_pos.0) as f32, (self.pos.2 - self.last_pos.2) as f32);
        let per_tick = (dx * dx + dz * dz).sqrt();
        self.limbs.tick(per_tick, 1.0);
        self.cape.tick([self.pos.0, self.pos.1, self.pos.2]);
        // ClientPlayerLikeState.tickMovement: am Boden die (gedeckelte) horizontale
        // Geschwindigkeit. Ob ein fremdes Entity am Boden ist, sagt uns der Server nicht --
        // die Strecke pro Tick ist die beste vorhandene Entsprechung, und sie ist genau dann
        // 0, wenn das Entity steht. add_distance_moved bleibt ungerufen: das tut Vanilla fuer
        // fremde Spieler auch nicht (s. CapeState::add_distance_moved).
        self.cape.tick_movement(per_tick.min(0.1));
        self.tick_hand_swing();
    }

    // LivingEntity.tickHandSwing, 1:1. In Vanilla ruft das nur PlayerEntity.tickMovement (und
    // HostileEntity) auf -- hier laeuft es fuer jedes Entity, was nichts kostet und nichts
    // aendert: wer nie ein Schlag-Paket bekommt, hat handSwinging = false und bleibt bei 0.
    fn tick_hand_swing(&mut self) {
        let dauer = self.hand_swing_duration();
        if self.hand_swinging {
            self.hand_swing_ticks += 1;
            if self.hand_swing_ticks >= dauer {
                self.hand_swing_ticks = 0;
                self.hand_swinging = false;
            }
        } else {
            self.hand_swing_ticks = 0;
        }
        self.hand_swing_progress = self.hand_swing_ticks as f32 / dauer as f32;
    }

    // LivingEntity.getHandSwingDuration: die SwingAnimationComponent des Haupthand-Items
    // (Speere 13..23 Ticks), sonst 6. Haste/Abbaulaehmung fehlen wie bisher.
    fn hand_swing_duration(&self) -> i32 {
        let Some(h) = self.main_hand else { return HAND_SWING_TICKS };
        match crate::item_components::resolve(h.stack) {
            Some(s) => crate::item_behavior::swing(&s).1 as i32,
            None => crate::item_registry::item_name(h.item_id).map_or(HAND_SWING_TICKS, |n| crate::item_registry::swing_duration_ticks(n) as i32),
        }
    }

    // Sticht die Figur mit einem Speer (SwingAnimationType.STAB) statt auszuholen, und haelt
    // sie ihn (ArmPose.SPEAR)? Beides haengt am Haupthand-Item.
    pub fn main_hand_spear(&self) -> bool {
        self.main_hand.is_some_and(|h| held_piercing(&h))
    }

    pub fn off_hand_spear(&self) -> bool {
        self.off_hand.and_then(|h| crate::item_registry::item_name(h.item_id)).is_some_and(crate::item_registry::is_spear)
    }

    // LivingEntity.swingHand: ein neuer Schlag verdraengt einen laufenden NUR, wenn der schon
    // ueber die Haelfte ist. Wer schneller klickt, als die Animation laeuft, bekommt also
    // keinen Ruckel-Neustart bei jedem Klick, sondern der laufende Schwung wird zu Ende
    // gefuehrt -- genau das sieht man in Vanilla beim Spam-Klicken.
    fn swing_hand(&mut self, off_hand: bool) {
        if !self.hand_swinging || self.hand_swing_ticks >= self.hand_swing_duration() / 2 || self.hand_swing_ticks < 0 {
            self.hand_swing_ticks = -1;
            self.hand_swinging = true;
            self.swing_off_hand = off_hand;
        }
    }

    // PositionInterpolator.refreshPositionAndAngles: neues Ziel, Schrittzaehler auf Anfang.
    // Ein Update, das mitten in eine laufende Angleichung faellt, setzt sie damit fort statt
    // sie zu verwerfen -- der Startpunkt ist die aktuelle (Teil-)Position.
    //
    // `pos`/`yaw`/`pitch` sind optional: ein reines Positionspaket laesst die Rotation stehen,
    // ein reines Rotationspaket die Position -- genau wie Vanillas
    // updateTrackedPositionAndAngles(Optional, Optional, Optional), das fehlende Werte aus dem
    // laufenden Ziel des Interpolators auffuellt.
    fn refresh(&mut self, pos: Option<(f64, f64, f64)>, yaw: Option<f32>, pitch: Option<f32>, now: Instant) {
        // ClientPlayNetworkHandler.onEntity / onEntityPositionSync: ist das Entity selbst
        // gelenkt, merkt sich der Client nur die Serverposition (tracked_pos, schon gesetzt)
        // und laesst Stelle und Drehung, wie sie sind.
        if self.driven {
            return;
        }
        // Projektile werden NICHT interpoliert. Vanillas Entity.moveOrInterpolateTo greift nur
        // dann zum Interpolator, wenn das Entity einen hat -- und den hat nur Lebendiges.
        // Fuer alles andere setzt es hart:
        //
        //     optional.ifPresent(this::setPos);
        //
        // Genau das ist bei einem Pfeil auch richtig: zwischen den seltenen Server-Paketen
        // fliegt er selbst (s. tick_projectile), und ein Paket ist eine KORREKTUR, kein Ziel,
        // auf das man sich ueber mehrere Ticks zubewegt.
        if self.projectile.is_some() || self.item_physics || self.block_physics {
            if let Some(pos) = pos {
                self.pos = pos;
                self.target_pos = pos;
                self.tracked_pos = pos;
                self.note_update_interval(now);
            }
            if let Some(yaw) = yaw {
                self.yaw_deg = yaw;
                self.target_yaw_deg = yaw;
            }
            if let Some(pitch) = pitch {
                self.pitch_deg = pitch;
                self.target_pitch_deg = pitch;
            }
            self.interp_steps = 0;
            return;
        }
        if let Some(pos) = pos {
            self.target_pos = pos;
            self.note_update_interval(now);
        }
        if let Some(yaw) = yaw {
            self.target_yaw_deg = yaw;
        }
        if let Some(pitch) = pitch {
            self.target_pitch_deg = pitch;
        }
        self.interp_steps = INTERPOLATION_STEPS;
    }

    // Reine Diagnose (F3): Abstand zwischen zwei Positions-Updates mitschreiben.
    fn note_update_interval(&mut self, now: Instant) {
        if self.has_moved {
            if self.update_intervals.len() == INTERVAL_SAMPLES {
                self.update_intervals.pop_front();
            }
            self.update_intervals.push_back(now.duration_since(self.updated_at).as_secs_f32() * 1000.0);
        }
        self.has_moved = true;
        self.updated_at = now;
    }

    // Diagnose: (letzter, mittlerer, groesster) Abstand zwischen zwei Positions-Updates in
    // Millisekunden. None, solange noch kein Abstand gemessen werden konnte (Entity gerade
    // erst gespawnt oder steht still -- ein stehendes Entity sendet keine Updates).
    pub fn update_interval_ms(&self) -> Option<(f32, f32, f32)> {
        if self.update_intervals.is_empty() {
            return None;
        }
        let last = *self.update_intervals.back()?;
        let sum: f32 = self.update_intervals.iter().sum();
        let avg = sum / self.update_intervals.len() as f32;
        let max = self.update_intervals.iter().copied().fold(f32::MIN, f32::max);
        Some((last, avg, max))
    }

    // Diagnose: wie alt die zuletzt empfangene Serverposition gerade ist. Das ist die
    // Verzoegerung, die allein durch das Warten auf das naechste Paket entsteht -- OHNE die
    // Laufzeit im Netz davor, die wir client-seitig nicht messen koennen (s. F3-Overlay).
    pub fn position_age_ms(&self, now: Instant) -> f32 {
        now.duration_since(self.updated_at).as_secs_f32() * 1000.0
    }

    // Diagnose: die zuletzt vom Server empfangene Position (das Ziel der Angleichung).
    pub fn target_pos(&self) -> (f64, f64, f64) {
        self.target_pos
    }

    // Laufende Schrittphase und deren Amplitude fuer die Modell-Pose. `tick_progress` ist der
    // Bruchteil des laufenden Ticks (0..1) -- damit gleitet die Schwingung zwischen den Ticks
    // statt 20 mal pro Sekunde weiterzuspringen.
    pub fn limb_swing(&self, tick_progress: f32) -> (f32, f32) {
        (self.limbs.animation_progress(tick_progress), self.limbs.amplitude(tick_progress))
    }

    // Der laufende Handschlag mit Zwischentick-Anteil und der Arm, der schlaegt
    // (LivingEntity.getHandSwingProgress). Der Sprung von 1 zurueck auf 0 am Ende des Schwungs
    // wird dabei UEBERSPRUNGEN: Vanilla addiert bei negativer Differenz eine 1 dazu, sonst
    // liefe der Arm im letzten Zwischenschritt rueckwaerts durch die ganze Bewegung.
    pub fn hand_swing(&self, tick_progress: f32) -> (f32, bool) {
        let mut delta = self.hand_swing_progress - self.last_hand_swing_progress;
        if delta < 0.0 {
            delta += 1.0;
        }
        // Der Arm der geschwungenen Hand (s. left_handed).
        (self.last_hand_swing_progress + delta * tick_progress.clamp(0.0, 1.0), self.swing_off_hand == self.left_handed)
    }

    // Set Head Rotation: Ziel setzen, Schrittzaehler auf Anfang (Vanilla:
    // updateTrackedHeadRotation(yaw, 3)).
    pub fn set_head_yaw(&mut self, head_yaw_deg: f32) {
        self.server_head_yaw_deg = head_yaw_deg;
        self.head_steps = HEAD_YAW_STEPS;
    }

    // Kopfdrehung RELATIV zum Koerper -- der Wert, den die Modell-Pose fuer den Kopf braucht
    // (Vanilla: LivingEntityRenderer.updateRenderState, wrapDegrees(headYaw - bodyYaw)).
    //
    // Beide Winkel mit DEMSELBEN Zwischentick-Anteil: interpolierte man nur einen von beiden,
    // zappelte ihre Differenz -- und die Differenz ist genau das, was hier herauskommt.
    pub fn relative_head_yaw(&self, tick_progress: f32) -> f32 {
        let head = lerp_angle(tick_progress.clamp(0.0, 1.0), self.last_head_yaw_deg, self.head_yaw_deg);
        crate::entity_model::wrap_degrees(head - self.render_yaw(tick_progress))
    }

    // Beruehrt die Trefferbox Wasser? Eine Naeherung an Vanillas updateInWaterState: die Ecken
    // und die Mitte der Box in Fuss- und Kopfhoehe gegen die Bloecke dort. Die Fuellhoehe
    // fliessenden Wassers bleibt aussen vor -- fuer die Schwimmhaltung reicht "da ist Wasser".
    fn touches_water(&self, world: &crate::chunk::World) -> bool {
        if !is_living(self.entity_type) {
            return false;
        }
        let (w, h) = hit_box_size(self.entity_type, self.pose);
        let r = w / 2.0 - 0.001;
        let (x, y, z) = self.pos;
        for dy in [0.001, h * 0.5, h - 0.001] {
            for (dx, dz) in [(0.0, 0.0), (-r, -r), (r, -r), (-r, r), (r, r)] {
                let state = world.block_at((x + dx).floor() as i32, (y + dy).floor() as i32, (z + dz).floor() as i32);
                if crate::fluid::fluid_kind(state) == Some(crate::fluid::FluidKind::Water) {
                    return true;
                }
            }
        }
        false
    }

    /// Schwimm-/Kriechhaltung fuers Bild (s. entity_model::Lean) -- Vanilla
    /// AvatarRenderer.setupRotations, Zweig swimAmount > 0.
    pub fn lean(&self, tick_progress: f32) -> crate::entity_model::Lean {
        let t = tick_progress.clamp(0.0, 1.0);
        crate::entity_model::Lean {
            amount: self.last_swim_amount + (self.swim_amount - self.last_swim_amount) * t,
            pitch_deg: self.render_pitch(t),
            in_water: self.in_water,
            swimming: self.pose == POSE_SWIMMING,
        }
    }

    // Gleitflug fuers Bild (s. entity_model::Glide), None am Boden. Die Geschwindigkeit ist die
    // Bewegung des letzten Ticks -- fremde Spieler bewegen sich bei uns nur ueber ihre
    // Positionspakete, eine eigene Geschwindigkeit schickt der Server fuer sie nicht mit.
    pub fn glide(&self, tick_progress: f32) -> Option<crate::entity_model::Glide> {
        if !self.gliding || !is_living(self.entity_type) {
            return None;
        }
        let t = tick_progress.clamp(0.0, 1.0);
        Some(crate::entity_model::Glide {
            ticks: self.gliding_ticks as f32 + t,
            pitch_deg: self.render_pitch(t),
            yaw_deg: lerp_angle(t, self.last_yaw_deg, self.yaw_deg),
            velocity: [
                (self.pos.0 - self.last_pos.0) as f32,
                (self.pos.1 - self.last_pos.1) as f32,
                (self.pos.2 - self.last_pos.2) as f32,
            ],
        })
    }

    // Alter fuer die Ruhe-Bewegung, mit Zwischentick-Anteil.
    pub fn age_ticks(&self, tick_progress: f32) -> f32 {
        self.age_ticks + tick_progress
    }

    // Hoehe eines Armorstandes nach seinen Flags (ArmorStandEntity: MARKER_DIMENSIONS =
    // 0x0, SMALL_DIMENSIONS = die halbe Normalgroesse, sonst EntityType.ARMOR_STAND mit
    // 0.5 x 1.975). Nur fuer den Ankerpunkt des Namensschildes gedacht -- der Aufrufer prueft
    // vorher, dass es sich ueberhaupt um einen Armorstand handelt.
    pub fn armor_stand_flags(&self) -> u8 {
        self.armor_stand_flags
    }

    pub fn armor_stand_height(&self) -> f64 {
        const ARMOR_STAND_HEIGHT: f64 = 1.975;
        if self.armor_stand_flags & protocol::packets::ARMOR_STAND_FLAG_MARKER != 0 {
            0.0
        } else if self.armor_stand_flags & protocol::packets::ARMOR_STAND_FLAG_SMALL != 0 {
            ARMOR_STAND_HEIGHT * 0.5
        } else {
            ARMOR_STAND_HEIGHT
        }
    }

    // Die drei Umhang-Winkel dieses Entities (s. entity_model::cape_angles). Vanilla mischt
    // dort zwischen lastX und getX() -- genau die beiden Werte, die es hier jetzt auch gibt
    // (frueher wurde zweimal dieselbe interpolierte Position uebergeben, weil es keinen
    // Tick-Vorgaenger gab).
    pub fn cape_angles(&self, tick_progress: f32) -> (f32, f32, f32) {
        crate::entity_model::cape_angles(
            &self.cape,
            [self.pos.0, self.pos.1, self.pos.2],
            [self.last_pos.0, self.last_pos.1, self.last_pos.2],
            self.render_yaw(tick_progress),
            tick_progress,
        )
    }
}

// MathHelper.lerpAngleDegrees: ueber den KUERZEREN Weg zwischen zwei Winkeln mischen, sonst
// dreht sich ein Entity beim Sprung von 359 auf 1 Grad einmal komplett herum.
fn lerp_angle(delta: f32, start: f32, end: f32) -> f32 {
    start + delta * crate::entity_model::wrap_degrees(end - start)
}

#[derive(Default)]
pub struct Entities {
    // Passagier-Id -> (Fahrzeug-Id, Sitz) (s. set_passengers). Vanilla haelt die Beziehung in
    // beide Richtungen; hier reicht diese, weil nur die Position des Passagiers daran haengt.
    vehicle_of: HashMap<i32, (i32, crate::riding::Seat)>,
    // Wie oft ein Spawn wegen doppelter UUID abgewiesen wurde -- reine Diagnose fuers
    // F3-Overlay, damit so ein Fall sichtbar ist statt still zu verschwinden.
    duplicate_uuids: u32,
    by_id: HashMap<i32, Entity>,
}

impl Entities {
    pub fn new() -> Self {
        Self::default()
    }

    // Wer sitzt auf wem (EntityPassengersSetS2CPacket). Eine LEERE Liste heisst: alle
    // bisherigen Passagiere dieses Fahrzeugs sind abgestiegen.
    //
    // Wie Vanillas onEntityPassengersSet: ein unbekanntes Fahrzeug wird ignoriert, und nur
    // Passagiere, die schon existieren, steigen auf (`if (entity2 != null) startRiding`).
    // Das zaehlt, weil has_passengers ueber Namensschilder entscheidet.
    pub fn set_passengers(&mut self, vehicle: i32, passengers: &[i32]) {
        if !self.by_id.contains_key(&vehicle) {
            return;
        }
        self.vehicle_of.retain(|_, (v, _)| *v != vehicle);
        // Der Sitz zaehlt in der Liste des SERVERS, auch wenn wir einen Mitfahrer davor nicht
        // kennen (etwa den eigenen Spieler, der hier nicht steht) -- so steht es auch in
        // Vanillas Passagierliste.
        for (index, p) in passengers.iter().enumerate() {
            if self.by_id.contains_key(p) {
                self.vehicle_of.insert(*p, (vehicle, crate::riding::Seat { index, count: passengers.len() }));
            }
        }
    }

    // Wo ein Mitfahrer auf diesem Fahrzeug sitzt (Fusspunkt), s. riding::riding_pos.
    pub fn seat_pos(&self, vehicle: i32, seat: crate::riding::Seat, passenger_type: &str) -> Option<(f64, f64, f64)> {
        let v = self.by_id.get(&vehicle)?;
        let name = entity_type_name(v.entity_type)?;
        let hoehe = hit_box_size(v.entity_type, v.pose).1;
        let p = crate::riding::riding_pos(name, hoehe, [v.pos.0, v.pos.1, v.pos.2], v.yaw_deg, seat, passenger_type);
        Some((p[0], p[1], p[2]))
    }

    // Das Fahrzeug, das der eigene Client lenkt (None = keins), s. Entity::driven.
    pub fn set_driven(&mut self, id: Option<i32>) {
        for (eid, e) in self.by_id.iter_mut() {
            let jetzt = Some(*eid) == id;
            if e.driven && !jetzt {
                // Zurueck an den Server: von der eigenen Stelle aus auf seine zuletzt empfangene.
                e.driven_next = None;
                e.target_pos = e.tracked_pos;
                e.interp_steps = INTERPOLATION_STEPS;
            }
            e.driven = jetzt;
        }
    }

    // Die neue Stelle des selbst gelenkten Fahrzeugs; der naechste Tick uebernimmt sie, damit
    // die Frame-Interpolation vom alten zum neuen Stand gleitet wie bei jedem anderen Entity.
    pub fn drive(&mut self, id: i32, pos: (f64, f64, f64), yaw_deg: f32, pitch_deg: f32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            if e.driven {
                e.driven_next = Some((pos, yaw_deg, pitch_deg));
            }
        }
    }

    // Stand eines selbst gelenkten Fahrzeugs sofort setzen -- fuer Korrekturen des Servers
    // (VehicleMove, Teleport), die nicht interpoliert werden.
    pub fn place_driven(&mut self, id: i32, pos: (f64, f64, f64), yaw_deg: f32, pitch_deg: f32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.pos = pos;
            e.last_pos = pos;
            e.target_pos = pos;
            e.yaw_deg = yaw_deg;
            e.last_yaw_deg = yaw_deg;
            e.target_yaw_deg = yaw_deg;
            e.pitch_deg = pitch_deg;
            e.target_pitch_deg = pitch_deg;
            e.driven_next = None;
            e.interp_steps = 0;
        }
    }

    // Vanillas Entity.hasPassengers -- LivingEntityRenderer.hasLabel blendet damit den Namen
    // eines Traegers aus (s. App::world_label).
    // Sitzt dieses Entity (z.B. der eigene Spieler) gerade auf einem anderen?
    pub fn is_riding(&self, id: i32) -> bool {
        self.vehicle_of.contains_key(&id)
    }

    pub fn has_passengers(&self, vehicle: i32) -> bool {
        self.vehicle_of.values().any(|(v, _)| *v == vehicle)
    }

    // Die Passagiere an ihr Fahrzeug setzen -- Vanillas Entity.updatePassengerPosition, mit
    // den Sitzplaetzen aus riding::riding_pos.
    fn tick_passengers(&mut self) {
        if self.vehicle_of.is_empty() {
            return;
        }
        let ziele: Vec<(i32, (f64, f64, f64))> = self
            .vehicle_of
            .iter()
            .filter_map(|(&passagier, &(fahrzeug, sitz))| {
                let typ = entity_type_name(self.by_id.get(&passagier)?.entity_type)?;
                Some((passagier, self.seat_pos(fahrzeug, sitz, typ)?))
            })
            .collect();
        for (passagier, pos) in ziele {
            let Some(e) = self.by_id.get_mut(&passagier) else { continue };
            // Hart setzen, nicht interpolieren: der Passagier HAT keine eigene Bewegung, er
            // ist da, wo sein Fahrzeug ist. last_pos bleibt der Stand des vorigen Ticks,
            // damit die Frame-Interpolation weiterhin glatt laeuft.
            e.pos = pos;
            e.tracked_pos = pos;
            e.target_pos = pos;
            e.interp_steps = 0;
        }
    }

    // Abgewiesene Doppel-Spawns (s. spawn) -- fuers F3-Overlay.
    pub fn duplicate_uuid_count(&self) -> u32 {
        self.duplicate_uuids
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&i32, &Entity)> {
        self.by_id.iter()
    }

    pub fn get(&self, id: i32) -> Option<&Entity> {
        self.by_id.get(&id)
    }

    pub fn get_mut(&mut self, id: i32) -> Option<&mut Entity> {
        self.by_id.get_mut(&id)
    }

    pub fn spawn(
        &mut self,
        id: i32,
        uuid: [u8; 16],
        entity_type: i32,
        pos: (f64, f64, f64),
        yaw_deg: f32,
        pitch_deg: f32,
        vel: (f64, f64, f64),
    ) {
        let now = Instant::now();
        // EINE UUID, EIN Entity -- exakt Vanillas EntityIndex.add: ist die UUID schon
        // vergeben, gibt es nur eine Warnung im Log.
        //
        // Vanilla legt den Zweiten also GAR NICHT an. Ohne diese Regel stand jedes doppelt
        // gespawnte Entity zweimal in der Welt -- mit zwei Koerpern und zwei Namensschildern
        // uebereinander.
        //
        // Die Null-UUID ist ausgenommen: sie ist kein echter Schluessel, sondern das, was
        // hier steht, wenn ein Paket gar keine mitbringt.
        if uuid != [0u8; 16] && self.by_id.iter().any(|(&vid, e)| e.uuid == uuid && vid != id) {
            self.duplicate_uuids = self.duplicate_uuids.saturating_add(1);
            return;
        }
        // insert statt Entry-Pruefung: schickt der Server dieselbe ID erneut (Respawn/
        // Wiederbetreten des Trackingbereichs), ist der neue Zustand massgeblich.
        self.by_id.insert(
            id,
            Entity {
                uuid,
                entity_type,
                pos,
                last_pos: pos,
                tracked_pos: pos,
                target_pos: pos,
                target_yaw_deg: yaw_deg,
                target_pitch_deg: pitch_deg,
                // Beim Spawn steht das Entity schon dort, wo es hingehoert -- keine
                // Angleichung offen (Vanilla: der Interpolator startet mit step = 0).
                interp_steps: 0,
                vel,
                in_ground: false,
                interaction_size: (1.0, 1.0),
                projectile: projectile_kind(entity_type),
                item_physics: entity_type_name(entity_type) == Some("item"),
                block_physics: entity_type_name(entity_type) == Some("falling_block"),
                orb_physics: entity_type_name(entity_type) == Some("experience_orb"),
                orb_target: None,
                falling_block: None,
                frame_facing: 3,
                frame_rotation: 0,
                baby: false,
                villager_data: None,
                on_ground: false,
                launch_offset: (0.0, 0.0, 0.0),
                launch_ticks: 0,
                yaw_deg,
                pitch_deg,
                last_yaw_deg: yaw_deg,
                last_pitch_deg: pitch_deg,
                updated_at: now,
                update_intervals: VecDeque::new(),
                has_moved: false,
                limbs: crate::entity_model::LimbAnimator::new(),
                cape: crate::entity_model::CapeState::new(),
                age_ticks: 0.0,
                head_yaw_deg: yaw_deg,
                last_head_yaw_deg: yaw_deg,
                body_yaw_deg: yaw_deg,
                last_body_yaw_deg: yaw_deg,
                use_ticks: 0,
                server_head_yaw_deg: yaw_deg,
                head_steps: 0,
                // Leere Haende, bis das erste Set Equipment kommt. Der Server schickt es
                // fuer jedes Entity direkt nach dem Spawn, sofern ueberhaupt etwas getragen
                // wird -- ein Entity ohne Ausruestung bekommt gar kein Paket.
                main_hand: None,
                off_hand: None,
                armor: [None, None, None, None],
                // Vanillas Vorgabewert, bis der Server etwas anderes meldet.
                pose: 0,
                hurt_ticks: 0,
                ticks_since_hurt: u32::MAX,
                hurt_by_crit: false,
                crit_grace: 0,
                driven: false,
                driven_next: None,
                saddled: false,
                body_item: None,
                hand_swinging: false,
                hand_swing_ticks: 0,
                hand_swing_progress: 0.0,
                last_hand_swing_progress: 0.0,
                swing_off_hand: false,
                custom_name: None,
                name_visible: false,
                end_crystal_show_bottom: true,
                tamable_flags: 0,
                cat_variant: None,
                potion_swirls: Vec::new(),
                potion_swirls_ambient: false,
                cloud_radius: 3.0,
                cloud_waiting: false,
                cloud_particle: protocol::particle::ParticleEffect {
                    type_id: protocol::particle::PARTICLE_ENTITY_EFFECT,
                    options: protocol::particle::ParticleOptions::Tinted(0xFFFF_FFFF),
                },
                firework_shooter: None,
                gliding: false,
                gliding_ticks: 0,
                swim_amount: 0.0,
                last_swim_amount: 0.0,
                in_water: false,
                on_fire: false,
                item: None,
                item_count: 1,
                orb_value: 0,
                item_seed: 187,
                invisible: false,
                glowing: false,
                living_flags: 0,
                left_handed: false,
                // Ein Display-Entity bekommt seinen Eintrag sofort, mit Vanillas
                // Vorgabewerten und leerem Inhalt: dadurch weiss der Renderer schon vor dem
                // ersten Metadaten-Paket, dass hier KEIN Kasten hingehoert.
                display: display_kind(entity_type).map(|kind| {
                    DisplayInfo::new(match kind {
                        DisplayKind::Text => DisplayContent::Text(TextDisplay::default()),
                        DisplayKind::Item => DisplayContent::Item(None),
                        // BlockDisplayEntity.initDataTracker: Luft, bis der Server etwas
                        // anderes meldet -- und Luft zeichnet nichts.
                        DisplayKind::Block => DisplayContent::Block(0),
                    })
                }),
                armor_stand_flags: 0,
                armor_stand_poses: ARMOR_STAND_DEFAULT_POSES,
            },
        );
    }

    // Set Entity Data anwenden -- bislang nur die Pose, weil nur sie die Trefferbox aendert.
    pub fn set_pose(&mut self, id: i32, pose: u8) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.pose = pose;
        }
    }

    // Die uebrigen Metadaten: Sichtbarkeit, Namensschild und der Inhalt eines
    // Display-Entities. Der Server schickt nur GEAENDERTE Felder, deshalb wird hier jedes
    // Feld einzeln geprueft und der Rest stehen gelassen.
    //
    // Der Inhalt (Paket-Index 23) ist bei den drei Display-Typen jeweils etwas anderes und
    // wird im Parser nur am Serializer unterschieden (s. packets::read_set_entity_data).
    // Hier faellt die Entscheidung endgueltig: uebernommen wird nur, was zum TYP passt --
    // ein Text-Feld an Index 23 eines ganz anderen Entities landet damit nirgends.
    pub fn apply_metadata(&mut self, id: i32, update: &protocol::packets::EntityDataUpdate) {
        let Some(e) = self.by_id.get_mut(&id) else { return };
        if let Some(flags) = update.flags {
            e.invisible = flags & protocol::packets::ENTITY_FLAG_INVISIBLE != 0;
            e.glowing = flags & protocol::packets::ENTITY_FLAG_GLOWING != 0;
            e.gliding = flags & protocol::packets::ENTITY_FLAG_GLIDING != 0;
            e.on_fire = flags & protocol::packets::ENTITY_FLAG_ON_FIRE != 0;
        }
        if entity_type_name(e.entity_type) == Some("firework_rocket") {
            if let Some(v) = update.firework_shooter {
                e.firework_shooter = v;
            }
        }
        if is_item_frame(e.entity_type) {
            if let Some(stack) = &update.frame_item {
                e.item = stack.as_ref().map(HeldItem::from_stack);
            }
            if let Some(r) = update.frame_rotation {
                e.frame_rotation = r;
            }
            if let Some(d) = update.hanging_direction.filter(|d| *d < 6) {
                e.frame_facing = d;
            }
        }
        if entity_type_name(e.entity_type) == Some("experience_orb") {
            if let Some(v) = update.orb_value {
                e.orb_value = v;
            }
        }
        if item_render_kind(e.entity_type).is_some() {
            if let Some(stack) = &update.item_stack {
                e.item = stack.as_ref().map(HeldItem::from_stack);
                e.item_count = stack.as_ref().map_or(0, |s| s.count);
                e.item_seed = stack.as_ref().map_or(187, |s| s.item_id as i32 + s.damage.unwrap_or(0));
            }
        }
        if let Some(links) = update.main_arm_left {
            e.left_handed = links;
        }
        if let Some(flags) = update.living_flags {
            e.living_flags = flags;
        }
        if update.villager_data.is_some() {
            e.villager_data = update.villager_data;
        }
        if let Some(b) = update.baby {
            if entity_type_name(e.entity_type).is_some_and(can_be_baby) {
                e.baby = b;
            }
        }
        if let Some(name) = &update.custom_name {
            e.custom_name = name.clone().filter(|spans| !spans.is_empty());
        }
        if let Some(visible) = update.name_visible {
            e.name_visible = visible;
        }
        if is_living(e.entity_type) {
            if let Some(farben) = &update.potion_swirls {
                e.potion_swirls = farben.clone();
            }
            if let Some(v) = update.potion_swirls_ambient {
                e.potion_swirls_ambient = v;
            }
        }
        if entity_type_name(e.entity_type) == Some("cat") {
            if let Some(v) = update.tamable_flags {
                e.tamable_flags = v;
            }
            if let Some(v) = update.cat_variant {
                e.cat_variant = Some(v);
            }
        }
        // AbstractArrow.IN_GROUND: ohne das flog ein Pfeil, der beim Laden der Gegend schon
        // steckte, bei uns los -- die Schwerkraft zog ihn aus der Wand, und er fehlte
        // dort.
        if projectile_kind(e.entity_type) == Some(ProjectileKind::Arrow) {
            if let Some(v) = update.arrow_in_ground {
                e.in_ground = v;
                if v {
                    e.vel = (0.0, 0.0, 0.0);
                }
            }
        }
        if entity_type_name(e.entity_type) == Some("interaction") {
            if let Some(v) = update.interaction_width {
                e.interaction_size.0 = v as f64;
            }
            if let Some(v) = update.interaction_height {
                e.interaction_size.1 = v as f64;
            }
        }
        if entity_type_name(e.entity_type) == Some("area_effect_cloud") {
            if let Some(v) = update.cloud_radius {
                e.cloud_radius = v;
            }
            if let Some(v) = update.cloud_waiting {
                e.cloud_waiting = v;
            }
            if let Some(p) = &update.cloud_particle {
                e.cloud_particle = p.clone();
            }
        }
        if entity_type_name(e.entity_type) == Some("end_crystal") {
            if let Some(v) = update.end_crystal_show_bottom {
                e.end_crystal_show_bottom = v;
            }
        }
        // Dasselbe Byte tragen bei einem Display-Entity die Billboard-Flags -- deshalb nur
        // bei einem echten Armorstand uebernehmen (s. packets::ARMOR_STAND_INDEX_FLAGS).
        if entity_type_name(e.entity_type) == Some("armor_stand") {
            if let Some(flags) = update.armor_stand_flags {
                e.armor_stand_flags = flags;
            }
            for (pose, neu) in e.armor_stand_poses.iter_mut().zip(update.armor_stand_poses) {
                if let Some(neu) = neu {
                    *pose = neu;
                }
            }
        }
        let Some(kind) = display_kind(e.entity_type) else { return };
        let display = e.display.get_or_insert_with(|| {
            DisplayInfo::new(match kind {
                DisplayKind::Text => DisplayContent::Text(TextDisplay::default()),
                DisplayKind::Item => DisplayContent::Item(None),
                DisplayKind::Block => DisplayContent::Block(0),
            })
        });
        // DisplayEntity.tick: START_INTERPOLATION legt den Beginn auf Alter + Verzoegerung;
        // eine Transformations-Aenderung startet bei Dauer != 0 von der GERADE gezeigten Lage.
        let alter = e.age_ticks;
        if let Some(v) = update.display_interp_duration {
            display.interp_duration = v;
        }
        if let Some(v) = update.display_interp_start {
            display.interp_start = alter + v as f32;
        }
        let aendert = update.display_translation.is_some()
            || update.display_scale.is_some()
            || update.display_left_rotation.is_some()
            || update.display_right_rotation.is_some();
        if aendert {
            let jetzt = display.transform_at(alter);
            display.interp_from = (display.interp_duration != 0).then_some(jetzt);
        }
        if let Some(v) = update.display_translation {
            display.translation = v;
        }
        if let Some(v) = update.display_scale {
            display.scale = v;
        }
        if let Some(v) = update.display_left_rotation {
            display.left_rotation = v;
        }
        if let Some(v) = update.display_right_rotation {
            display.right_rotation = v;
        }
        if let Some(v) = update.display_billboard {
            display.billboard = v;
        }
        if let Some(v) = update.display_view_range {
            display.view_range = v;
        }
        match (kind, &mut display.content) {
            (DisplayKind::Text, DisplayContent::Text(text)) => {
                if let Some(spans) = &update.display_text {
                    text.spans = spans.clone();
                }
                if let Some(v) = update.text_background {
                    text.background = v;
                }
                if let Some(v) = update.text_opacity {
                    text.opacity = v;
                }
                if let Some(v) = update.text_flags {
                    text.flags = v;
                }
            }
            (DisplayKind::Item, content) => {
                if let Some(m) = update.item_display_mode {
                    display.item_mode = m;
                }
                if let Some(stack) = &update.display_item {
                    *content = DisplayContent::Item(stack.as_ref().map(HeldItem::from_stack));
                }
            }
            (DisplayKind::Block, content) => {
                if let Some(state) = update.display_block_state {
                    *content = DisplayContent::Block(state);
                }
            }
            // Typ und bereits gefuehrter Inhalt passen nicht zusammen -- kann nur durch ein
            // widerspruechliches Paket entstehen; dann lieber nichts aendern.
            _ => {}
        }
    }

    // Einmal pro CLIENT-Tick aufrufen, nicht pro Frame: Position, Rotation, Schrittanimation
    // und Umhang sind allesamt Tick-Groessen (s. Entity::tick). Pro Frame gerechnet haengen
    // sie an der Bildrate -- genau das, was hier vermieden wird.
    //
    // Auch fuer STILLSTEHENDE Entities: die schicken keine Bewegungspakete, und gerade im
    // Stillstand faellt eine eingefrorene Armhaltung auf.
    // `eigener`: der eigene Spieler, wenn ihm Erfahrungsorbs folgen koennen (lebt, kein
    // Zuschauer) -- er steht nicht in dieser Liste, zaehlt fuer die Orbs aber mit (s. tick_orb).
    pub fn tick(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, eigener: Option<OrbPlayer>) {
        let spieler: Vec<OrbPlayer> = if self.by_id.values().any(|e| e.orb_physics) {
            eigener
                .into_iter()
                .chain(
                    self.by_id
                        .iter()
                        .filter(|(id, e)| is_player(e.entity_type) && Some(**id) != eigener.map(|p| p.id))
                        .map(|(&id, e)| OrbPlayer { id, pos: e.pos, eye_height: e.eye_height() }),
                )
                .collect()
        } else {
            Vec::new()
        };
        for e in self.by_id.values_mut() {
            e.tick(world, shapes, &spieler);
        }
        // NACH dem Ticken der Fahrzeuge: der Passagier soll die Position DIESES Ticks
        // uebernehmen, nicht die des vorigen.
        self.tick_passengers();
    }

    // Schadens-Meldung des Servers auf ein Entity anwenden. Unbekannte IDs werden still
    // uebergangen: das Paket kann ein Entity betreffen, das wir gar nicht verfolgen.
    pub fn damaged(&mut self, id: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.on_damaged();
        }
    }

    // Ein Schlag-Paket des Servers auf ein Entity anwenden. Unbekannte IDs werden still
    // uebergangen -- wie bei `damaged` kann das Paket ein Entity betreffen, das wir nicht
    // verfolgen.
    pub fn swing_hand(&mut self, id: i32, off_hand: bool) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.swing_hand(off_hand);
        }
    }

    // Meldung eines kritischen Treffers auf ein Entity (s. Entity::on_crit).
    pub fn crit(&mut self, id: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.on_crit();
        }
    }

    // Set Equipment anwenden. Von den Tier-Slots zaehlen Sattel und Koerper fuers Reiten.
    pub fn set_equipment(&mut self, id: i32, slot: u8, item: Option<HeldItem>) {
        let Some(entity) = self.by_id.get_mut(&id) else { return };
        match slot {
            protocol::packets::EQUIPMENT_SLOT_MAINHAND => entity.main_hand = item,
            protocol::packets::EQUIPMENT_SLOT_OFFHAND => entity.off_hand = item,
            protocol::packets::EQUIPMENT_SLOT_BODY => entity.body_item = item,
            protocol::packets::EQUIPMENT_SLOT_SADDLE => entity.saddled = item.is_some(),
            _ => {
                if let Some(armor_slot) = crate::armor::ArmorSlot::from_equipment_slot(slot) {
                    entity.armor[armor_slot.index()] = item;
                }
            }
        }
    }

    pub fn remove(&mut self, ids: &[i32]) {
        for id in ids {
            self.by_id.remove(id);
            // Mit dem Entity auch seine Mitfahr-Beziehungen aufloesen -- in beide
            // Richtungen, sonst haengt ein Passagier an einem Fahrzeug, das es nicht mehr
            // gibt (und bliebe fuer immer an dessen letzter Stelle stehen).
            self.vehicle_of.remove(id);
            self.vehicle_of.retain(|_, (v, _)| v != id);
        }
    }

    pub fn clear(&mut self) {
        self.by_id.clear();
        self.vehicle_of.clear();
    }

    // Relative Bewegung (Move Entity Pos / Pos+Rot). Das Delta bezieht sich auf die
    // autoritative SERVER-Position (Vanillas TrackedPosition.withDelta), NICHT auf die
    // gerade interpolierte -- sonst zoege sich der Interpolationsrueckstand durch jedes
    // weitere Paket und das Entity bliebe dauerhaft hinter seiner echten Position zurueck.
    pub fn move_relative(&mut self, id: i32, dx: f64, dy: f64, dz: f64) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.tracked_pos = (e.tracked_pos.0 + dx, e.tracked_pos.1 + dy, e.tracked_pos.2 + dz);
            e.refresh(Some(e.tracked_pos), None, None, Instant::now());
        }
    }

    // Set Entity Motion (0x63) fuer ein fremdes Entity -- Vanillas Entity.setVelocityClient.
    // Zaehlt nur fuer Projektile, alle anderen bewegen sich bei uns ueber ihre Positionspakete.
    pub fn set_velocity(&mut self, id: i32, vel: (f64, f64, f64)) {
        if let Some(e) = self.by_id.get_mut(&id) {
            if e.projectile.is_some() || e.item_physics || e.block_physics || e.orb_physics {
                e.vel = vel;
                // Eine neue Geschwindigkeit heisst: es fliegt wieder -- aber nur eine echte
                // (AbstractArrow.lerpMotion: lengthSqr > 0). Null laesst den Pfeil stecken.
                if vel.0 * vel.0 + vel.1 * vel.1 + vel.2 * vel.2 > 0.0 {
                    e.in_ground = false;
                }
            }
        }
    }

    // Blockzustand eines fallenden Blocks aus dem Spawn-Paket (s. Entity::falling_block).
    pub fn set_frame_facing(&mut self, id: i32, facing: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.frame_facing = facing.clamp(0, 5) as u8;
        }
    }

    pub fn set_falling_block(&mut self, id: i32, state: u32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            if e.block_physics {
                e.falling_block = Some(state);
            }
        }
    }

    // Den optischen Abschuss-Versatz setzen (s. Entity::launch_render_offset). Nur fuer
    // Projektile -- alles andere hat keinen Schuetzen.
    pub fn set_launch_offset(&mut self, id: i32, offset: (f32, f32, f32)) {
        if let Some(e) = self.by_id.get_mut(&id) {
            if e.projectile.is_some() {
                e.launch_offset = offset;
                e.launch_ticks = LAUNCH_BLEND_TICKS;
            }
        }
    }

    // Absolute Position (Entity Position Sync / Teleport Entity).
    pub fn set_position(&mut self, id: i32, x: f64, y: f64, z: f64) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.tracked_pos = (x, y, z);
            e.refresh(Some((x, y, z)), None, None, Instant::now());
        }
    }

    // Teleport Entity: anders als Position Sync greift er auch bei einem selbst gelenkten
    // Fahrzeug (ClientPlayNetworkHandler.onEntityPosition setzt die Stelle in jedem Fall).
    pub fn teleport(&mut self, id: i32, x: f64, y: f64, z: f64, yaw_deg: f32, pitch_deg: f32) {
        if self.by_id.get(&id).is_some_and(|e| e.driven) {
            if let Some(e) = self.by_id.get_mut(&id) {
                e.tracked_pos = (x, y, z);
            }
            self.place_driven(id, (x, y, z), yaw_deg, pitch_deg);
            return;
        }
        self.set_position(id, x, y, z);
        self.set_rotation(id, yaw_deg, pitch_deg);
    }

    // Reine Rotation (Move Entity Rot): Position bleibt, wo sie ist -- Vanillas
    // updateTrackedAngles fuellt den Positionsteil aus dem laufenden Ziel auf.
    pub fn set_rotation(&mut self, id: i32, yaw_deg: f32, pitch_deg: f32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.refresh(None, Some(yaw_deg), Some(pitch_deg), Instant::now());
        }
    }

    // Raycast gegen die Entity-Trefferboxen: fusszentrierte AABB an der TICK-quantisierten
    // Position (s. INTERPOLATION_STEPS), Groesse nach Typ und Pose (s. hit_box_size). Liefert
    // die Entity-ID mit dem NAECHSTEN Eintrittspunkt innerhalb von `max_dist` Bloecken, sonst
    // None.
    //
    // Genommen wird der Tick-Stand, nicht der Zwischentick-Stand des Bildes: das ist die Box,
    // auf die sich Server und GrimAC geeinigt haben.
    pub fn pick(&self, origin: (f64, f64, f64), dir: (f64, f64, f64), max_dist: f64) -> Option<i32> {
        self.pick_hit(origin, dir, max_dist).map(|(id, _)| id)
    }

    // Wie pick, zusaetzlich mit der Strahllaenge bis zum Eintritt in die Box (in Einheiten
    // von `dir`) -- der Rechtsklick braucht die Trefferstelle (INTERACT_AT, s. app).
    pub fn pick_hit(&self, origin: (f64, f64, f64), dir: (f64, f64, f64), max_dist: f64) -> Option<(i32, f64)> {
        let mut best: Option<(i32, f64)> = None;
        for (&id, e) in &self.by_id {
            // Nicht-angreifbare Ziele (Items, XP-Kugeln, Pfeile) ueberspringen -- ein
            // Attack darauf waere ein sofortiger Server-Kick (s. is_attackable).
            if !is_attackable(e.entity_type) {
                continue;
            }
            // Display-Entities sind reine Anzeige: Vanillas Entity.canHit() ist ohne
            // LivingEntity-Ueberschreibung `false`, und ihre Box hat mit WIDTH/HEIGHT = 0
            // ohnehin kein Volumen. Ohne diese Zeile schluckte ein Hologramm, das vor einem
            // Gegner haengt, den Schlag -- und zwar unsichtbar, weil man nur den Text sieht.
            if e.display.is_some() {
                continue;
            }
            // Ein interaction-Entity traegt seine Groesse in den Metadaten (s. interaction_size):
            // damit machen Server Display-Entities anklickbar und schlagbar. Mit den festen
            // Typmassen (0 x 0) liess es sich nie treffen (etwa auf mcpvp.club).
            let (width, height) = e.hit_size();
            let half = width / 2.0;
            let (x, y, z) = e.hit_pos();
            let min = (x - half, y, z - half);
            let max = (x + half, y + height, z + half);
            if let Some(t) = ray_aabb(origin, dir, min, max) {
                if t <= max_dist && best.is_none_or(|(_, bt)| t < bt) {
                    best = Some((id, t));
                }
            }
        }
        best
    }

    // Die Reichweite eines Items mit attack_range (Speere, Server-Waffen), wie Vanillas
    // ProjectileUtil.getHitEntitiesAlong: getroffen wird, was der Abschnitt des Strahls von
    // `t0` bis `t1` beruehrt (Einheiten von `dir`, das normiert sein muss) -- ein Gegner naeher
    // als `t0` also nicht. Mit `margin` > 0 zaehlt auch ein knapper Vorbeischuss an der um
    // `margin` vergroesserten Box, aber nur, wenn vom Beruehrpunkt zur Boxmitte kein Block vor
    // dem Gegner liegt: `free(von, nach)` sagt, welcher Anteil der Strecke frei ist (1 = ganz).
    // Zurueck: das naechste Ziel und der Abstand seines Trefferpunkts vom Ursprung.
    pub fn pick_segment(
        &self,
        origin: (f64, f64, f64),
        dir: (f64, f64, f64),
        t0: f64,
        t1: f64,
        margin: f64,
        free: &dyn Fn((f64, f64, f64), (f64, f64, f64)) -> f64,
    ) -> Option<(i32, f64)> {
        let punkt = |o: (f64, f64, f64), d: (f64, f64, f64), t: f64| (o.0 + d.0 * t, o.1 + d.1 * t, o.2 + d.2 * t);
        let start = punkt(origin, dir, t0);
        let mut best: Option<(i32, f64)> = None;
        for (&id, e) in &self.by_id {
            // Dieselbe Auswahl wie pick_hit.
            if !is_attackable(e.entity_type) || e.display.is_some() {
                continue;
            }
            let (width, height) = e.hit_size();
            let half = width / 2.0;
            let (x, y, z) = e.hit_pos();
            let (min, max) = ((x - half, y, z - half), (x + half, y + height, z + half));
            let innen = |p: (f64, f64, f64)| p.0 >= min.0 && p.0 <= max.0 && p.1 >= min.1 && p.1 <= max.1 && p.2 >= min.2 && p.2 <= max.2;
            let dist = if innen(start) {
                Some(t0)
            } else if let Some(t) = ray_aabb(origin, dir, min, max).filter(|t| (t0..=t1).contains(t)) {
                Some(t)
            } else if margin > 0.0 {
                let gross = ((min.0 - margin, min.1 - margin, min.2 - margin), (max.0 + margin, max.1 + margin, max.2 + margin));
                ray_aabb(origin, dir, gross.0, gross.1).filter(|t| (t0..=t1).contains(t)).and_then(|t| {
                    let p = punkt(origin, dir, t);
                    let mitte = (x, y + height / 2.0, z);
                    let f = free(p, mitte).clamp(0.0, 1.0);
                    let weg = ((mitte.0 - p.0) * f, (mitte.1 - p.1) * f, (mitte.2 - p.2) * f);
                    let s = ray_aabb(p, weg, min, max).filter(|s| *s <= 1.0)?;
                    let q = punkt(p, weg, s);
                    Some(((q.0 - origin.0).powi(2) + (q.1 - origin.1).powi(2) + (q.2 - origin.2).powi(2)).sqrt())
                })
            } else {
                None
            };
            if let Some(d) = dist {
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((id, d));
                }
            }
        }
        best
    }
}

// Klassischer Slab-Test: pro Achse das Parameterintervall [t_min, t_max] schneiden, in
// dem der Strahl zwischen den beiden Ebenen liegt. Bleibt ein nichtleeres Intervall mit
// t_max >= 0 uebrig, trifft der Strahl; Eintritt bei max(t_min, 0) (Start IN der Box
// zaehlt als Treffer bei 0). `dir` muss nicht normiert sein -- max_dist ist dann in
// Einheiten der dir-Laenge; der Aufrufer uebergibt einen Einheitsvektor.
// --- Projektile -----------------------------------------------------------------------
//
// Pfeile und Wurfgeschosse fliegen in Vanilla auf dem CLIENT. Der Server schickt beim Spawn
// die Anfangsgeschwindigkeit und danach nur noch grobe Korrekturen; dazwischen rechnet jeder
// Client dieselbe Bahn. Deshalb sehen Pfeile fluessig aus, obwohl kaum Pakete kommen.
//
// Fuer fremde Entities interpolieren wir sonst nur zwischen Server-Positionen -- bei einem
// Pfeil ist das sichtbar falsch: er ruckelt, haengt hinterher und landet optisch woanders.
//
// WICHTIG ist die REIHENFOLGE, und sie ist fuer die beiden Familien VERSCHIEDEN
// (1.21.11 nachgesehen):
//
//   ThrowableProjectile.tick():  applyGravity() -> applyInertia() -> bewegen
//   AbstractArrow.tick():        (Wasserreibung) -> bewegen -> applyInertia(0.99) -> applyGravity()
//
// Wer beides gleich behandelt, bekommt eine Bahn, die pro Tick um genau einen
// Schwerkraftschritt danebenliegt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectileKind {
    // AbstractArrow.getDefaultGravity() = 0.05, Reibung 0.99, im Wasser getWaterInertia() = 0.6.
    Arrow,
    // ThrowableProjectile.getDefaultGravity() = 0.03, Reibung 0.99, im Wasser 0.8.
    Throwable,
    // AbstractWindCharge (Windkugel, auch die der Breeze): AbstractHurtingProjectile ohne
    // Beschleunigung (accelerationPower = 0) und mit Traegheit 1.0 in Luft wie Wasser -- sie
    // fliegt also schnurgerade mit ihrer Startgeschwindigkeit weiter. Der Server schickt ihre
    // Position nur alle zehn Ticks (updateInterval 10); ohne eigenen Flug stand sie dazwischen
    // still und sprang dann.
    WindCharge,
}

impl ProjectileKind {
    fn gravity(self) -> f64 {
        match self {
            Self::Arrow => 0.05,
            Self::Throwable => 0.03,
            Self::WindCharge => 0.0,
        }
    }

    fn water_inertia(self) -> f64 {
        match self {
            Self::Arrow => 0.6,
            Self::Throwable => 0.8,
            Self::WindCharge => 1.0,
        }
    }
}

// Block.getSlipperiness: 0.6, Eis 0.98, Blaueis 0.989, Schleim 0.8.
fn block_slipperiness(state: u32) -> f64 {
    match crate::block_name(state).unwrap_or("") {
        "ice" | "packed_ice" | "frosted_ice" => 0.98,
        "blue_ice" => 0.989,
        "slime_block" => 0.8,
        _ => 0.6,
    }
}

// Entity.move fuer eine fusszentrierte Box (halbe Breite `half`, Hoehe `height`) gegen die
// Kollisionsformen der Bloecke: erst Y, dann die groessere der beiden waagerechten Achsen
// (Entity.adjustMovementForCollisions). Rueckgabe: neue Position und je Achse, ob sie
// angestossen ist.
pub fn move_with_collisions(
    pos: (f64, f64, f64),
    vel: (f64, f64, f64),
    half: f64,
    height: f64,
    world: &crate::chunk::World,
    shapes: &crate::BlockShapes,
) -> ((f64, f64, f64), [bool; 3]) {
    let mut min = [pos.0 - half, pos.1, pos.2 - half];
    let mut max = [pos.0 + half, pos.1 + height, pos.2 + half];
    let bewegung = [vel.0, vel.1, vel.2];
    // Alle Kollisionsboxen im ueberstrichenen Bereich (plus ein Block Rand fuer hohe Formen
    // wie Zaeune).
    let mut boxen: Vec<([f64; 3], [f64; 3])> = Vec::new();
    let lo = [0, 1, 2].map(|a| (min[a] + bewegung[a].min(0.0)).floor() as i32 - 1);
    let hi = [0, 1, 2].map(|a| (max[a] + bewegung[a].max(0.0)).floor() as i32 + 1);
    if (0..3).any(|a| hi[a] - lo[a] > 16) {
        // Unplausibel schnell -- lieber ohne Kollision als tausende Bloecke abfragen.
        return ((pos.0 + vel.0, pos.1 + vel.1, pos.2 + vel.2), [false; 3]);
    }
    for bx in lo[0]..=hi[0] {
        for by in lo[1]..=hi[1] {
            for bz in lo[2]..=hi[2] {
                let state = world.block_at(bx, by, bz);
                if state == crate::STATE_AIR {
                    continue;
                }
                for b in crate::collision_boxes(state, shapes) {
                    let o = [bx as f64, by as f64, bz as f64];
                    boxen.push(([0, 1, 2].map(|i| o[i] + b.min[i] as f64), [0, 1, 2].map(|i| o[i] + b.max[i] as f64)));
                }
            }
        }
    }
    let reihenfolge = if bewegung[0].abs() < bewegung[2].abs() { [1, 2, 0] } else { [1, 0, 2] };
    let mut stoss = [false; 3];
    for achse in reihenfolge {
        let mut d = bewegung[achse];
        if d == 0.0 {
            continue;
        }
        let (a1, a2) = match achse {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        for (bmin, bmax) in &boxen {
            // Nur Boxen, die sich auf den beiden anderen Achsen mit uns ueberlappen.
            if bmax[a1] <= min[a1] + 1.0e-7 || bmin[a1] >= max[a1] - 1.0e-7 || bmax[a2] <= min[a2] + 1.0e-7 || bmin[a2] >= max[a2] - 1.0e-7 {
                continue;
            }
            if d > 0.0 && bmin[achse] >= max[achse] - 1.0e-7 {
                d = d.min(bmin[achse] - max[achse]);
            } else if d < 0.0 && bmax[achse] <= min[achse] + 1.0e-7 {
                d = d.max(bmax[achse] - min[achse]);
            }
        }
        if d != bewegung[achse] {
            stoss[achse] = true;
        }
        min[achse] += d;
        max[achse] += d;
    }
    ((min[0] + half, min[1], min[2] + half), stoss)
}

// Welche Entity-Typen der Client selbst fliegen laesst.
//
// BEWUSST NICHT dabei sind die Feuerkugeln (fireball, small_fireball, wither_skull,
// dragon_fireball): die haben eine ganz andere Bahn (keine Schwerkraft, dafuer eine
// Beschleunigung entlang der Anfangsrichtung) und gehoeren erst dazu, wenn ihre Physik
// genauso belegt ist wie die hier.
pub fn projectile_kind(entity_type: i32) -> Option<ProjectileKind> {
    match entity_type_name(entity_type)? {
        "arrow" | "spectral_arrow" | "trident" => Some(ProjectileKind::Arrow),
        "ender_pearl" | "snowball" | "egg" | "experience_bottle" | "splash_potion" | "lingering_potion" => {
            Some(ProjectileKind::Throwable)
        }
        "wind_charge" | "breeze_wind_charge" => Some(ProjectileKind::WindCharge),
        _ => None,
    }
}

// Ueber so viele Ticks laeuft der optische Abschuss-Versatz wieder auf Null.
// Drei Ticks sind rund 150 ms; ein Pfeil hat in der Zeit schon rund neun Bloecke zurueck-
// gelegt, der Versatz von gut einem Drittel Block faellt dann nicht mehr auf.
pub const LAUNCH_BLEND_TICKS: u32 = 3;

// Womit geschossen wurde. Bestimmt, WO die Hand sitzt: die Armbrust liegt beim Schuss
// deutlich tiefer als der gespannte Bogen, und ein geworfener Gegenstand wieder anders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchWeapon {
    Bow,
    Crossbow,
    Hand,
}

// Handposition relativ zum Spawnpunkt des Geschosses, in der Reihenfolge
// (rechts, hoch, vorwaerts) und in Bloecken.
//
// Diese Zahlen stehen in KEINER Vanilla-Klasse -- Vanilla hat den Versatz gar nicht, es gibt
// also nichts nachzuschlagen. Sie sind nach dem Spielermodell abgeschaetzt: der Armdrehpunkt
// liegt bei 5/16 Block neben der Koerperachse, der ausgestreckte Arm reicht rund einen
// Drittelblock nach vorn, und der Spawnpunkt liegt auf Augenhoehe minus 0.1. Wer sie
// nachjustieren will, aendert sie hier und nirgends sonst.
impl LaunchWeapon {
    fn hand(self) -> [f32; 3] {
        match self {
            // Gespannter Bogen: die Hand ist fast auf Augenhoehe, weit vorn.
            Self::Bow => [0.28, -0.10, 0.36],
            // Armbrust: im Anschlag vor der Brust, also deutlich tiefer.
            Self::Crossbow => [0.30, -0.32, 0.34],
            // Geworfen (Perle, Schneeball, Ei): Wurf aus der Schulter, dazwischen.
            Self::Hand => [0.32, -0.22, 0.30],
        }
    }
}

// Was in dieser Hand steckt -- None, wenn es kein Fernkampfgegenstand ist.
fn weapon_of(item: Option<&HeldItem>) -> Option<LaunchWeapon> {
    match crate::item_registry::item_name(item?.item_id)? {
        "bow" => Some(LaunchWeapon::Bow),
        "crossbow" => Some(LaunchWeapon::Crossbow),
        _ => None,
    }
}

// Aus welcher Hand kam der Schuss und womit.
//
// Der Bogen bleibt nach dem Schuss in der Hand, also reicht ein Blick auf die Ausruestung --
// und der ist verlaesslicher als das Benutzungs-Bit in den Metadaten, das der Server im
// selben Tick schon wieder loescht. Liegt in beiden Haenden eine Waffe, gewinnt die
// Haupthand, wie in Vanilla auch (Player.getItemInHand wird von der Haupthand aus geprueft).
// Ohne Bogen oder Armbrust wurde geworfen.
pub fn shooting_hand(main_hand: Option<&HeldItem>, off_hand: Option<&HeldItem>) -> (bool, LaunchWeapon) {
    if let Some(w) = weapon_of(main_hand) {
        return (false, w);
    }
    if let Some(w) = weapon_of(off_hand) {
        return (true, w);
    }
    (false, LaunchWeapon::Hand)
}

// Der optische Versatz in WELTkoordinaten: die Handposition des Schuetzen, gedreht in seine
// Blickrichtung.
//
// Basis wie in Vanilla: Blickrichtung ist (-sin y * cos p, -sin p, cos y * cos p), der
// Aufwaertsvektor dieselbe Formel mit p - 90 Grad, und "rechts" steht bei Gierwinkel 0
// (Blick nach +Z) auf -X. `offhand` spiegelt nur die Seite -- deshalb wandert der Versatz
// mit, wenn man den Bogen mit F in die andere Hand legt.
pub fn launch_hand_offset(yaw_deg: f32, pitch_deg: f32, offhand: bool, weapon: LaunchWeapon) -> (f32, f32, f32) {
    let [rechts_anteil, hoch_anteil, vor_anteil] = weapon.hand();
    let rechts_anteil = if offhand { -rechts_anteil } else { rechts_anteil };
    let (y, p) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let (sy, cy) = (y.sin(), y.cos());
    let (sp, cp) = (p.sin(), p.cos());
    let vor = [-sy * cp, -sp, cy * cp];
    let hoch = [-sy * sp, cp, cy * sp];
    let rechts = [-cy, 0.0, -sy];
    let achse = |i: usize| rechts[i] * rechts_anteil + hoch[i] * hoch_anteil + vor[i] * vor_anteil;
    (achse(0), achse(1), achse(2))
}

// Projectile.lerpRotation: ein 20-%-Schritt auf den Zielwinkel, ueber die 360-Grad-Naht hinweg.
fn lerp_rotation(current: f32, target: f32) -> f32 {
    let mut c = current;
    while target - c < -180.0 {
        c -= 360.0;
    }
    while target - c >= 180.0 {
        c += 360.0;
    }
    c + (target - c) * 0.2
}

// Der erste Blocktreffer auf der Strecke origin -> origin+delta, als Anteil 0..1 von delta.
// None = freie Bahn. Gemessen wird gegen die KOLLISIONSform (Vanillas Block.COLLIDER), nicht
// gegen den Umriss: ein Pfeil fliegt durch hohes Gras und bleibt an einer Stufe haengen.
// Liegt der Punkt in der Kollisionsform des Blocks, in dem er steht (AABB.contains)?
fn point_in_collision(p: (f64, f64, f64), world: &crate::chunk::World, shapes: &crate::BlockShapes) -> bool {
    let (bx, by, bz) = (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
    let state = world.block_at(bx, by, bz);
    if state == crate::chunk::STATE_AIR {
        return false;
    }
    let (lx, ly, lz) = (p.0 - bx as f64, p.1 - by as f64, p.2 - bz as f64);
    crate::collision::collision_boxes(state, shapes).iter().any(|b| {
        lx >= b.min[0] as f64 && lx < b.max[0] as f64 && ly >= b.min[1] as f64 && ly < b.max[1] as f64 && lz >= b.min[2] as f64 && lz < b.max[2] as f64
    })
}

fn block_hit_along(
    origin: (f64, f64, f64),
    delta: (f64, f64, f64),
    world: &crate::chunk::World,
    shapes: &crate::BlockShapes,
) -> Option<f64> {
    let ziel = (origin.0 + delta.0, origin.1 + delta.1, origin.2 + delta.2);
    let min = (origin.0.min(ziel.0), origin.1.min(ziel.1), origin.2.min(ziel.2));
    let max = (origin.0.max(ziel.0), origin.1.max(ziel.1), origin.2.max(ziel.2));
    let mut naechster: Option<f64> = None;
    for bx in min.0.floor() as i32..=max.0.floor() as i32 {
        for by in min.1.floor() as i32..=max.1.floor() as i32 {
            for bz in min.2.floor() as i32..=max.2.floor() as i32 {
                let state = world.block_at(bx, by, bz);
                if state == crate::chunk::STATE_AIR {
                    continue;
                }
                for b in crate::collision::collision_boxes(state, shapes) {
                    let kasten_min = (bx as f64 + b.min[0] as f64, by as f64 + b.min[1] as f64, bz as f64 + b.min[2] as f64);
                    let kasten_max = (bx as f64 + b.max[0] as f64, by as f64 + b.max[1] as f64, bz as f64 + b.max[2] as f64);
                    if let Some(t) = ray_aabb(origin, delta, kasten_min, kasten_max) {
                        if (0.0..=1.0).contains(&t) && naechster.is_none_or(|bisher| t < bisher) {
                            naechster = Some(t);
                        }
                    }
                }
            }
        }
    }
    naechster
}

fn ray_aabb(origin: (f64, f64, f64), dir: (f64, f64, f64), min: (f64, f64, f64), max: (f64, f64, f64)) -> Option<f64> {
    let mut t_min = f64::NEG_INFINITY;
    let mut t_max = f64::INFINITY;
    for ((o, d), (lo, hi)) in
        [((origin.0, dir.0), (min.0, max.0)), ((origin.1, dir.1), (min.1, max.1)), ((origin.2, dir.2), (min.2, max.2))]
    {
        if d.abs() < 1e-12 {
            // Strahl parallel zu diesen Ebenen: trifft nur, wenn er dazwischen startet.
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo - o) / d, (hi - o) / d);
        let (t0, t1) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
        t_min = t_min.max(t0);
        t_max = t_max.min(t1);
        if t_min > t_max {
            return None;
        }
    }
    (t_max >= 0.0).then_some(t_min.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Seitwaerts laufende Spieler duerfen keine komischen Sidesteps machen.
    // Ein Spieler, der nach Sueden schaut (Yaw 0) und seitwaerts nach Osten laeuft: in Vanilla
    // dreht sich der KOERPER Richtung Laufweg (-90), bis der Kopf 50 Grad verdreht ist -- er
    // zeigt also nicht mehr stur in Blickrichtung.
    #[test]
    fn seitwaerts_laufender_spieler_dreht_den_koerper_mit() {
        let mut entities = Entities::new();
        entities.spawn(3, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        for schritt in 1..=20 {
            entities.set_position(3, schritt as f64 * 0.2, 64.0, 0.0);
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        let e = entities.get(3).unwrap();
        assert!((e.render_yaw(1.0) - -50.0).abs() < 0.5, "Koerper bei -50 Grad erwartet, war {}", e.render_yaw(1.0));
        // Rueckwaerts laufen dreht den Koerper NICHT um: Laufrichtung Norden, Blick Sueden.
        let mut entities = Entities::new();
        entities.spawn(4, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        for schritt in 1..=20 {
            entities.set_position(4, 0.0, 64.0, -(schritt as f64) * 0.2);
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        assert!(entities.get(4).unwrap().render_yaw(1.0).abs() < 0.5);
    }

    // Kein Lebewesen (Boot, Lore, Display): die Drehung bleibt die vom Server.
    #[test]
    fn nicht_lebewesen_drehen_sich_wie_vom_server() {
        let mut entities = Entities::new();
        entities.spawn(5, [0u8; 16], typ("minecart"), (0.0, 64.0, 0.0), 30.0, 0.0, (0.0, 0.0, 0.0));
        for schritt in 1..=10 {
            entities.set_position(5, schritt as f64 * 0.3, 64.0, 0.0);
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        assert!((entities.get(5).unwrap().render_yaw(1.0) - 30.0).abs() < 1e-3);
    }

    // Leere Welt fuer die Tests: nur Luft, also keine Blockkollision fuer Projektile.
    fn test_welt() -> crate::chunk::World {
        crate::chunk::World::new()
    }

    fn spawn_one(entities: &mut Entities) {
        entities.spawn(7, [0u8; 16], 1, (10.0, 64.0, -5.0), 90.0, 0.0, (0.0, 0.0, 0.0));
    }

    // --- Projektile ---------------------------------------------------------------------

    fn typ_index(name: &str) -> i32 {
        (0..500).find(|&i| entity_type_name(i) == Some(name)).unwrap_or_else(|| panic!("Entitytyp {name} unbekannt"))
    }

    fn leer() -> (crate::chunk::World, crate::BlockShapes) {
        (crate::chunk::World::new(), crate::BlockShapes::all_full_cubes())
    }

    // --- Abschuss-Versatz ----------------------------------------------------------------

    fn item(name: &str) -> HeldItem {
        let id = (0..2000)
            .find(|&i| crate::item_registry::item_name(i) == Some(name))
            .unwrap_or_else(|| panic!("Item {name} unbekannt"));
        HeldItem::plain(id)
    }

    #[test]
    fn die_schusshand_kommt_aus_der_ausruestung() {
        let bogen = item("bow");
        let armbrust = item("crossbow");
        let schwert = item("diamond_sword");
        assert_eq!(shooting_hand(Some(&bogen), None), (false, LaunchWeapon::Bow));
        assert_eq!(shooting_hand(Some(&schwert), Some(&armbrust)), (true, LaunchWeapon::Crossbow));
        assert_eq!(shooting_hand(Some(&bogen), Some(&armbrust)), (false, LaunchWeapon::Bow), "Haupthand gewinnt");
        assert_eq!(shooting_hand(None, None), (false, LaunchWeapon::Hand), "ohne Waffe wurde geworfen");
    }

    // Gierwinkel 0 heisst Blick nach +Z. Die rechte Hand liegt dann auf -X, "vorwaerts" auf
    // +Z, und die Hoehe bleibt die Hoehe. Die Zweithand spiegelt nur die Seite.
    #[test]
    fn der_versatz_liegt_bei_gierwinkel_null_auf_den_erwarteten_achsen() {
        let (x, y, z) = launch_hand_offset(0.0, 0.0, false, LaunchWeapon::Bow);
        assert!((x - -0.28).abs() < 1e-6, "x={x}");
        assert!((y - -0.10).abs() < 1e-6, "y={y}");
        assert!((z - 0.36).abs() < 1e-6, "z={z}");

        let (zx, _, zz) = launch_hand_offset(0.0, 0.0, true, LaunchWeapon::Bow);
        assert!((zx - 0.28).abs() < 1e-6, "Zweithand spiegelt die Seite: x={zx}");
        assert!((zz - z).abs() < 1e-6, "vorwaerts bleibt gleich");
    }

    // Die Armbrust liegt tiefer als der Bogen -- genau darum ging es bei der Anforderung.
    #[test]
    fn die_armbrust_liegt_tiefer_als_der_bogen() {
        let (_, bogen_y, _) = launch_hand_offset(0.0, 0.0, false, LaunchWeapon::Bow);
        let (_, armbrust_y, _) = launch_hand_offset(0.0, 0.0, false, LaunchWeapon::Crossbow);
        assert!(armbrust_y < bogen_y, "Armbrust {armbrust_y} muss unter Bogen {bogen_y} liegen");
    }

    // Beim Blick gerade nach unten (Pitch 90) zeigt "vorwaerts" nach -Y und "hoch" nach +Z.
    #[test]
    fn der_versatz_dreht_mit_dem_pitch_mit() {
        let (_, y, z) = launch_hand_offset(0.0, 90.0, false, LaunchWeapon::Bow);
        assert!((y - -0.36).abs() < 1e-6, "vorwaerts zeigt nach unten: y={y}");
        assert!((z - -0.10).abs() < 1e-6, "hoch zeigt nach hinten: z={z}");
    }

    // Der Versatz ist nur eine Einblendung: nach LAUNCH_BLEND_TICKS Ticks ist das Geschoss
    // wieder genau auf seiner echten Bahn.
    #[test]
    fn der_abschuss_versatz_laeuft_auf_null_zurueck() {
        let (welt, formen) = leer();
        let mut entities = Entities::new();
        entities.spawn(1, [2u8; 16], typ_index("arrow"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 3.0));
        entities.set_launch_offset(1, (0.3, -0.1, 0.4));
        let voll = entities.get(1).expect("Pfeil da").launch_render_offset(0.0);
        assert!((voll[0] - 0.3).abs() < 1e-6, "direkt beim Spawn der ganze Versatz");

        for _ in 0..LAUNCH_BLEND_TICKS {
            entities.tick(&welt, &formen, None);
        }
        assert_eq!(entities.get(1).expect("Pfeil da").launch_render_offset(0.0), [0.0; 3]);
    }

    // Die Enderperle: ThrowableProjectile.tick macht applyGravity() VOR applyInertia(), und
    // beides VOR der Bewegung. Nach einem Tick ist sie also schon gefallen.
    //
    //   vy = (0 - 0.03) * 0.99 = -0.0297
    //   vz = 1.5 * 0.99        =  1.485
    #[test]
    fn enderperle_faellt_im_ersten_tick_schon() {
        let (welt, formen) = leer();
        let mut entities = Entities::new();
        entities.spawn(1, [1u8; 16], typ_index("ender_pearl"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 1.5));
        entities.tick(&welt, &formen, None);
        let e = entities.get(1).expect("Perle da");
        assert!((e.pos.1 - (64.0 - 0.0297)).abs() < 1e-9, "y={}", e.pos.1);
        assert!((e.pos.2 - 1.485).abs() < 1e-9, "z={}", e.pos.2);
        assert!((e.vel.1 - -0.0297).abs() < 1e-12, "vy={}", e.vel.1);
        assert!((e.vel.2 - 1.485).abs() < 1e-12, "vz={}", e.vel.2);
    }

    // Der Pfeil: AbstractArrow.tick bewegt ZUERST und wendet Reibung und Schwerkraft erst
    // DANACH an. Nach einem Tick ist er deshalb noch auf Starthoehe -- genau der Unterschied
    // zur Perle, und die Probe darauf, dass die Reihenfolge stimmt.
    #[test]
    // Die Windkugel fliegt zwischen den Serverpaketen (nur alle zehn Ticks) selbst weiter:
    // schnurgerade, ohne Schwerkraft und ohne Reibung (AbstractWindCharge: Traegheit 1.0,
    // accelerationPower 0).
    fn windkugel_fliegt_gerade_mit_gleicher_geschwindigkeit() {
        let (welt, formen) = leer();
        let mut entities = Entities::new();
        entities.spawn(5, [5u8; 16], typ_index("wind_charge"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.5, 0.25, -1.0));
        for _ in 0..10 {
            entities.tick(&welt, &formen, None);
        }
        let e = entities.get(5).expect("Windkugel da");
        assert!((e.pos.0 - 5.0).abs() < 1e-9 && (e.pos.1 - 66.5).abs() < 1e-9 && (e.pos.2 + 10.0).abs() < 1e-9, "{:?}", e.pos);
        assert_eq!(e.vel, (0.5, 0.25, -1.0));
    }

    // Ein relatives Serverpaket rechnet auf die zuletzt GESENDETE Serverposition, nicht auf
    // die eigene Vorhersage -- sonst zaehlt die Bewegung doppelt (Windkugeln
    // schossen davon, Pfeile blieben vor der Wand stehen).
    #[test]
    fn relatives_paket_rechnet_auf_die_serverposition() {
        let (welt, formen) = leer();
        let mut entities = Entities::new();
        entities.spawn(5, [5u8; 16], typ_index("wind_charge"), (0.0, 64.0, 0.0), 0.0, 0.0, (1.0, 0.0, 0.0));
        for _ in 0..10 {
            entities.tick(&welt, &formen, None);
        }
        // Der Server meldet nach zehn Ticks dieselben zehn Bloecke.
        entities.move_relative(5, 10.0, 0.0, 0.0);
        let e = entities.get(5).expect("Windkugel da");
        assert!((e.pos.0 - 10.0).abs() < 1e-9, "doppelt gezaehlt: {:?}", e.pos);
    }

    #[test]
    fn pfeil_faellt_im_ersten_tick_noch_nicht() {
        let (welt, formen) = leer();
        let mut entities = Entities::new();
        entities.spawn(2, [2u8; 16], typ_index("arrow"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 3.0));
        entities.tick(&welt, &formen, None);
        let e = entities.get(2).expect("Pfeil da");
        assert_eq!(e.pos.1, 64.0, "der Pfeil darf im ersten Tick nicht fallen");
        assert!((e.pos.2 - 3.0).abs() < 1e-9, "z={}", e.pos.2);
        // Reibung nach der Bewegung, dann Schwerkraft.
        assert!((e.vel.2 - 2.97).abs() < 1e-12, "vz={}", e.vel.2);
        assert!((e.vel.1 - -0.05).abs() < 1e-12, "vy={}", e.vel.1);

        // Zweiter Tick: jetzt faellt er um genau die Geschwindigkeit des ersten.
        entities.tick(&welt, &formen, None);
        let e = entities.get(2).expect("Pfeil da");
        assert!((e.pos.1 - (64.0 - 0.05)).abs() < 1e-9, "y={}", e.pos.1);
    }

    // Ein geworfenes Item fliegt zwischen den (seltenen) Serverpaketen selbst: es faellt, landet
    // auf dem Block darunter und bleibt dort liegen, statt zu versinken.
    #[test]
    fn item_faellt_und_landet_auf_dem_block() {
        let (mut welt, formen) = leer();
        let luft = (0..24).map(|_| crate::chunk::ChunkSection::from_states(vec![0; crate::chunk::SECTION_VOLUME])).collect();
        welt.insert_column(0, 0, luft);
        for x in 0..=2 {
            for z in 0..=4 {
                welt.set_pending_block(x, 63, z, 1);
            }
        }
        let mut entities = Entities::new();
        entities.spawn(7, [7u8; 16], typ_index("item"), (0.5, 66.0, 0.5), 0.0, 0.0, (0.0, 0.2, 0.1));
        entities.tick(&welt, &formen, None);
        let e = entities.get(7).expect("Item da");
        assert!(e.pos.1 > 66.0, "erster Tick steigt noch: y={}", e.pos.1);
        assert!(e.pos.2 > 0.5, "und fliegt vorwaerts: z={}", e.pos.2);
        for _ in 0..60 {
            entities.tick(&welt, &formen, None);
        }
        let e = entities.get(7).expect("Item da");
        assert!((e.pos.1 - 64.0).abs() < 1e-6, "liegt auf dem Boden: y={}", e.pos.1);
        assert!(e.vel.2.abs() < 0.01, "Reibung hat es angehalten: vz={}", e.vel.2);
    }

    // Ein Erfahrungsorb fliegt ohne Serverpaket zum Spieler unter 8 Bloecken; einen weiter
    // entfernten laesst er liegen.
    #[test]
    fn orb_fliegt_zum_spieler() {
        let (mut welt, formen) = leer();
        let luft = (0..24).map(|_| crate::chunk::ChunkSection::from_states(vec![0; crate::chunk::SECTION_VOLUME])).collect();
        welt.insert_column(0, 0, luft);
        for x in 0..16 {
            for z in 0..16 {
                welt.set_pending_block(x, 63, z, 1);
            }
        }
        let mut entities = Entities::new();
        entities.spawn(3, [3u8; 16], typ_index("experience_orb"), (2.5, 64.0, 2.5), 0.0, 0.0, (0.0, 0.0, 0.0));
        let nah = OrbPlayer { id: 100, pos: (7.5, 64.0, 2.5), eye_height: 1.62 };
        let fern = OrbPlayer { id: 100, pos: (12.5, 64.0, 2.5), eye_height: 1.62 };
        let mut x = Vec::new();
        for _ in 0..100 {
            entities.tick(&welt, &formen, Some(nah));
            x.push(entities.get(3).expect("Orb da").pos.0);
        }
        assert!(x[10] > 2.6, "setzt sich gleich in Bewegung: x={}", x[10]);
        assert!(x[99] > 7.0, "beim Spieler angekommen: x={}", x[99]);
        let mut weit = Entities::new();
        weit.spawn(4, [4u8; 16], typ_index("experience_orb"), (2.5, 64.0, 2.5), 0.0, 0.0, (0.0, 0.0, 0.0));
        for _ in 0..20 {
            weit.tick(&welt, &formen, Some(fern));
        }
        let e = weit.get(4).expect("Orb da");
        assert!((e.pos.0 - 2.5).abs() < 1e-9, "zu weit weg, liegt still: x={}", e.pos.0);
        assert!((e.pos.1 - 64.0).abs() < 1e-6, "auf dem Boden: y={}", e.pos.1);
    }

    // Ein fallender Sandblock faellt zwischen den Serverpaketen selbst und bleibt auf dem Block
    // darunter stehen; seinen Zustand bekommt er aus dem Spawn-Paket.
    #[test]
    fn fallender_block_faellt_und_landet() {
        let (mut welt, formen) = leer();
        let luft = (0..24).map(|_| crate::chunk::ChunkSection::from_states(vec![0; crate::chunk::SECTION_VOLUME])).collect();
        welt.insert_column(0, 0, luft);
        welt.set_pending_block(0, 63, 0, 1);
        let mut entities = Entities::new();
        entities.spawn(8, [8u8; 16], typ_index("falling_block"), (0.5, 70.0, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_falling_block(8, 118);
        assert_eq!(entities.get(8).unwrap().falling_block, Some(118));
        entities.tick(&welt, &formen, None);
        assert!(entities.get(8).unwrap().pos.1 < 70.0, "faellt schon im ersten Tick");
        for _ in 0..40 {
            entities.tick(&welt, &formen, None);
        }
        let e = entities.get(8).unwrap();
        assert!((e.pos.1 - 64.0).abs() < 1e-6, "steht auf dem Block: y={}", e.pos.1);
        // Andere Typen bekommen keinen Blockzustand.
        entities.spawn(9, [9u8; 16], typ_index("item"), (0.5, 70.0, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_falling_block(9, 118);
        assert_eq!(entities.get(9).unwrap().falling_block, None);
    }

    // Ein interaction-Entity ist mit seiner Metadaten-Groesse treffbar (Vorgabe 1 x 1).
    #[test]
    fn interaction_ist_treffbar() {
        let mut entities = Entities::new();
        entities.spawn(9, [9u8; 16], typ_index("interaction"), (0.0, 64.0, 5.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let blick = (0.0, 0.0, 1.0);
        assert_eq!(entities.pick((0.0, 64.5, 0.0), blick, 6.0), Some(9));
        // Hoeher als die Vorgabe: erst mit HEIGHT = 3 trifft der Strahl auf 66.5.
        assert_eq!(entities.pick((0.0, 66.5, 0.0), blick, 6.0), None);
        entities.apply_metadata(9, &protocol::packets::EntityDataUpdate { interaction_height: Some(3.0), ..Default::default() });
        assert_eq!(entities.pick((0.0, 66.5, 0.0), blick, 6.0), Some(9));
    }

    // Ein Pfeil, der beim Laden der Gegend schon steckt (IN_GROUND aus den Metadaten), faellt
    // nicht aus der Wand -- auch nicht durch ein Bewegungspaket mit Geschwindigkeit null.
    #[test]
    fn steckender_pfeil_aus_metadaten_bleibt_haengen() {
        let (welt, formen) = leer();
        let mut entities = Entities::new();
        entities.spawn(4, [4u8; 16], typ_index("arrow"), (0.0, 70.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.apply_metadata(4, &protocol::packets::EntityDataUpdate { arrow_in_ground: Some(true), ..Default::default() });
        entities.set_velocity(4, (0.0, 0.0, 0.0));
        for _ in 0..20 {
            entities.tick(&welt, &formen, None);
        }
        let e = entities.get(4).expect("Pfeil da");
        assert!(e.in_ground);
        assert_eq!(e.pos, (0.0, 70.0, 0.0));
    }

    // Steht ein Pfeil in der Kollisionsform eines Blocks, steckt er (AbstractArrow.tick).
    #[test]
    fn pfeil_im_block_steckt() {
        let (mut welt, formen) = leer();
        welt.set_pending_block(0, 64, 0, 1);
        let mut entities = Entities::new();
        entities.spawn(5, [5u8; 16], typ_index("arrow"), (0.5, 64.5, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.tick(&welt, &formen, None);
        let e = entities.get(5).expect("Pfeil da");
        assert!(e.in_ground);
        assert_eq!(e.pos, (0.5, 64.5, 0.5));
    }

    // Trifft ein Pfeil einen Block, bleibt er stecken: Geschwindigkeit null, und auch die
    // folgenden Ticks bewegen ihn nicht mehr.
    #[test]
    fn pfeil_bleibt_im_block_stecken() {
        let (mut welt, formen) = leer();
        // Ein Block direkt in der Flugbahn. set_pending_block genuegt -- block_at gibt der
        // Vorhersageschicht Vorrang, und geladene Chunkdaten braucht der Test so nicht.
        welt.set_pending_block(0, 64, 3, 1);
        let mut entities = Entities::new();
        entities.spawn(3, [3u8; 16], typ_index("arrow"), (0.0, 64.5, 0.0), 0.0, 0.0, (0.0, 0.0, 5.0));
        entities.tick(&welt, &formen, None);
        let e = entities.get(3).expect("Pfeil da");
        assert!(e.in_ground, "haette stecken bleiben muessen");
        assert_eq!(e.vel, (0.0, 0.0, 0.0), "steckender Pfeil hat keine Geschwindigkeit mehr");
        // An der Blockkante, nicht dahinter.
        assert!(e.pos.2 <= 3.0 + 1e-9, "z={} liegt hinter dem Block", e.pos.2);
        let stecken = e.pos;
        entities.tick(&welt, &formen, None);
        assert_eq!(entities.get(3).expect("Pfeil da").pos, stecken, "ein steckender Pfeil bewegt sich nicht mehr");
    }

    // Ein Positionspaket ist fuer ein Projektil eine KORREKTUR und gilt sofort -- Vanilla
    // interpoliert nur Lebendiges (Entity.moveOrInterpolateTo ohne InterpolationHandler).
    // Beim Spieler bleibt die Angleichung dagegen ueber mehrere Ticks verteilt.
    #[test]
    fn projektile_werden_hart_gesetzt_spieler_interpoliert() {
        let (welt, formen) = leer();
        let mut entities = Entities::new();
        entities.spawn(4, [4u8; 16], typ_index("ender_pearl"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(5, [5u8; 16], typ_index("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_position(4, 10.0, 64.0, 0.0);
        entities.set_position(5, 10.0, 64.0, 0.0);
        assert_eq!(entities.get(4).expect("Perle da").pos.0, 10.0, "die Perle steht sofort am neuen Ort");
        assert_eq!(entities.get(5).expect("Spieler da").pos.0, 0.0, "der Spieler naehert sich ihm erst");
        entities.tick(&welt, &formen, None);
        let spieler = entities.get(5).expect("Spieler da").pos.0;
        assert!(spieler > 0.0 && spieler < 10.0, "der Spieler soll interpolieren, war {spieler}");
    }

    // --- Handschlag ---------------------------------------------------------------------

    #[test]
    fn ein_schlag_laeuft_genau_sechs_ticks_und_endet_bei_null() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.swing_hand(7, false);
        let e = entities.get(7).expect("Entity da");
        // Vor dem ersten Tick steht der Zaehler auf -1 -- der Schlag hat noch nicht begonnen.
        assert_eq!(e.hand_swing(0.0).0, 0.0);

        let mut werte = Vec::new();
        for _ in 0..HAND_SWING_TICKS {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
            werte.push(entities.get(7).expect("Entity da").hand_swing(1.0).0);
        }
        // 0, 1/6, ... 5/6 -- und erst der SIEBTE Tick setzt zurueck. Die zusaetzliche Stufe
        // kommt von der -1 beim Anstossen und ist kein Zaehlfehler: sie sorgt dafuer, dass der
        // Schlag bei 0 beginnt statt schon einen Schritt weit fortgeschritten zu sein.
        assert!((werte[0] - 0.0).abs() < 1e-6, "{werte:?}");
        assert!((werte[5] - 5.0 / 6.0).abs() < 1e-6, "{werte:?}");
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        // Der siebte Tick setzt den Zaehler zurueck. Der ZWISCHENSCHRITT laeuft trotzdem noch
        // sauber bis 1 durch (s. der_letzte_zwischenschritt_laeuft_vorwaerts_weiter) -- erst
        // ab dem Tick danach steht der Arm still.
        assert_eq!(entities.get(7).expect("Entity da").hand_swing(1.0).0, 1.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert_eq!(entities.get(7).expect("Entity da").hand_swing(0.5).0, 0.0, "danach ist der Schlag vorbei");
    }

    #[test]
    // LivingEntity.swingHand laesst einen laufenden Schwung nur ab der HAELFTE verdraengen.
    // Ohne das setzte schnelles Klicken die Animation dauernd zurueck, und der Arm zuckte
    // bei jedem Klick am Anfang herum, statt durchzuschwingen.
    fn ein_zweiter_schlag_verdraengt_den_ersten_erst_ab_der_haelfte() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.swing_hand(7, false);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None); // Zaehler steht jetzt auf 1

        entities.swing_hand(7, false);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        // Waere neu gestartet worden, staende der Fortschritt wieder bei 0.
        assert!((entities.get(7).expect("Entity da").hand_swing(1.0).0 - 2.0 / 6.0).abs() < 1e-6);

        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None); // 3 -- ab hier ist die Haelfte erreicht
        entities.swing_hand(7, false);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        // Neu begonnen heisst: wieder am Anfang der Bewegung (1/6), nicht kurz vor dem Ende.
        assert!(
            (entities.get(7).expect("Entity da").hand_swing(1.0).0 - 1.0 / 6.0).abs() < 1e-6,
            "ab der Haelfte darf neu begonnen werden"
        );
    }

    #[test]
    // Am Ende springt der Fortschritt von 5/6 auf 0 zurueck. Wer da schlicht zwischen last und
    // aktuell mischt, laesst den Arm im letzten Zwischenschritt die ganze Bewegung RUECKWAERTS
    // durchlaufen -- ein sichtbares Zucken. Vanilla addiert deshalb bei negativer Differenz 1.
    fn der_letzte_zwischenschritt_laeuft_vorwaerts_weiter() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.swing_hand(7, false);
        // Ein Tick mehr als die Schwungdauer: genau der, in dem der Zaehler auf 0 zurueckfaellt.
        for _ in 0..=HAND_SWING_TICKS {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        let e = entities.get(7).expect("Entity da");
        let (anfang, _) = e.hand_swing(0.0);
        let (mitte, _) = e.hand_swing(0.5);
        let (ende, _) = e.hand_swing(1.0);
        assert!((anfang - 5.0 / 6.0).abs() < 1e-6, "{anfang}");
        assert!(mitte > anfang && ende > mitte, "{anfang} -> {mitte} -> {ende} laeuft nicht vorwaerts");
        assert!(ende <= 1.0 + 1e-6, "{ende} laeuft ueber 1 hinaus");
    }

    #[test]
    fn die_zweithand_schlaegt_mit_dem_linken_arm() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.swing_hand(7, true);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert!(!entities.get(7).expect("Entity da").hand_swing(0.5).1, "Zweithand = linker Arm");
        entities.swing_hand(7, false);
        for _ in 0..HAND_SWING_TICKS {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        entities.swing_hand(7, false);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert!(entities.get(7).expect("Entity da").hand_swing(0.5).1, "Haupthand = rechter Arm");
    }

    // --- Kritische Treffer ---------------------------------------------------------------

    fn ein_spieler(entities: &mut Entities) {
        entities.spawn(7, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    }

    #[test]
    // Die Reihenfolge, die Vanilla wirklich schickt: erst der Schaden, dann der Crit.
    fn ein_crit_nach_dem_schaden_faerbt_den_laufenden_blitz() {
        let mut entities = Entities::new();
        ein_spieler(&mut entities);
        entities.damaged(7);
        assert!(entities.get(7).expect("da").is_hurt());
        assert!(!entities.get(7).expect("da").is_hurt_by_crit(), "ohne Crit-Paket keine Crit-Farbe");
        entities.crit(7);
        assert!(entities.get(7).expect("da").is_hurt_by_crit());
    }

    #[test]
    // Die umgekehrte Reihenfolge darf genauso gehen -- darauf zu wetten, dass zwei Pakete
    // immer in derselben Folge ankommen, waere eine unnoetige Annahme.
    fn ein_crit_kurz_vor_dem_schaden_zaehlt_auch_noch() {
        let mut entities = Entities::new();
        ein_spieler(&mut entities);
        entities.crit(7);
        entities.damaged(7);
        assert!(entities.get(7).expect("da").is_hurt_by_crit());
    }

    #[test]
    // Und ein Crit, der lange her ist, darf den naechsten gewoehnlichen Treffer NICHT
    // einfaerben. Genau dafuer ist das Nachlaufzeitfenster eng.
    fn ein_alter_crit_faerbt_den_naechsten_treffer_nicht() {
        let mut entities = Entities::new();
        ein_spieler(&mut entities);
        entities.crit(7);
        for _ in 0..=CRIT_GRACE_TICKS {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        entities.damaged(7);
        assert!(entities.get(7).expect("da").is_hurt());
        assert!(!entities.get(7).expect("da").is_hurt_by_crit());
    }

    #[test]
    // Nach einem Crit-Treffer folgt ein gewoehnlicher: die Markierung muss weg sein.
    fn ein_gewoehnlicher_treffer_loescht_die_crit_markierung() {
        let mut entities = Entities::new();
        ein_spieler(&mut entities);
        entities.damaged(7);
        entities.crit(7);
        assert!(entities.get(7).expect("da").is_hurt_by_crit());
        for _ in 0..MAX_HURT_TIME {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        entities.damaged(7);
        assert!(entities.get(7).expect("da").is_hurt());
        assert!(!entities.get(7).expect("da").is_hurt_by_crit(), "der alte Crit darf nicht nachwirken");
    }

    #[test]
    // Ausserhalb des Blitzes ist die Markierung bedeutungslos -- sie darf nicht allein
    // faerben, sonst blieben getroffene Spieler dauerhaft bunt.
    fn ohne_laufenden_blitz_gibt_es_keine_crit_farbe() {
        let mut entities = Entities::new();
        ein_spieler(&mut entities);
        entities.damaged(7);
        entities.crit(7);
        for _ in 0..MAX_HURT_TIME {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        assert!(!entities.get(7).expect("da").is_hurt());
        assert!(!entities.get(7).expect("da").is_hurt_by_crit());
    }

    #[test]
    fn ein_crit_fuer_ein_unbekanntes_entity_wird_still_uebergangen() {
        let mut entities = Entities::new();
        entities.crit(4711);
    }

    #[test]
    fn ein_schlag_fuer_ein_unbekanntes_entity_wird_still_uebergangen() {
        let mut entities = Entities::new();
        entities.swing_hand(4711, false);
    }

    fn span(text: &str) -> protocol::nbt::TextSpan {
        protocol::nbt::TextSpan { text: text.to_string(), ..Default::default() }
    }

    // Index 15 traegt bei einem Armorstand dessen Flags, bei einem Display den
    // Billboard-Modus. Das Paket sieht in beiden Faellen gleich aus -- erst der Entity-Typ
    // entscheidet. Ginge das schief, saesse das Namensschild eines Marker-Standes fast zwei
    // Bloecke zu hoch.
    #[test]
    fn armorstand_flags_und_billboard_teilen_sich_index_15_ohne_sich_zu_stoeren() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("armor_stand"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [1u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let update = protocol::packets::EntityDataUpdate {
            armor_stand_flags: Some(protocol::packets::ARMOR_STAND_FLAG_MARKER),
            display_billboard: Some(protocol::packets::ARMOR_STAND_FLAG_MARKER),
            ..Default::default()
        };
        entities.apply_metadata(1, &update);
        entities.apply_metadata(2, &update);

        // Der Armorstand nimmt die Flags (Marker -> Hoehe 0), das Display den Billboard-Modus.
        assert_eq!(entities.get(1).unwrap().armor_stand_height(), 0.0);
        assert_eq!(entities.get(2).unwrap().display.as_ref().unwrap().billboard, protocol::packets::ARMOR_STAND_FLAG_MARKER);
        // Und umgekehrt: der Armorstand hat gar keinen Display-Eintrag.
        assert!(entities.get(1).unwrap().display.is_none());
    }

    // Das Benutzen-Flag samt Hand kommt im LIVING_FLAGS-Byte und wirkt sofort -- daran
    // haengt das gehobene Schild eines fremden Spielers.
    #[test]
    fn benutzungsflag_nennt_die_hand_und_wirkt_sofort() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        assert_eq!(entities.get(1).unwrap().using_item_right_arm(), None);
        let an = protocol::packets::EntityDataUpdate {
            living_flags: Some(protocol::packets::LIVING_FLAG_USING_ITEM | protocol::packets::LIVING_FLAG_OFFHAND_ACTIVE),
            ..Default::default()
        };
        entities.apply_metadata(1, &an);
        // Nebenhand -> linker Arm, ohne einen Tick abzuwarten.
        assert_eq!(entities.get(1).unwrap().using_item_right_arm(), Some(false));
        entities.apply_metadata(1, &protocol::packets::EntityDataUpdate { living_flags: Some(0), ..Default::default() });
        assert_eq!(entities.get(1).unwrap().using_item_right_arm(), None);
    }

    // Linkshaender (MAIN_ARM links): die Haupthand sitzt am linken Arm, die Nebenhand am
    // rechten -- Halten, Blocken und Schwingen folgen. Sonst hielten Linkshaender
    // sichtbar ihr Essen, wo eigentlich ein Schild ist.
    #[test]
    fn linkshaender_halten_die_haupthand_links() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let apfel = HeldItem::plain(crate::item_registry::item_id_by_name("golden_apple").unwrap());
        let schild = HeldItem::plain(crate::item_registry::item_id_by_name("shield").unwrap());
        entities.set_equipment(1, protocol::packets::EQUIPMENT_SLOT_MAINHAND, Some(apfel));
        entities.set_equipment(1, protocol::packets::EQUIPMENT_SLOT_OFFHAND, Some(schild));
        assert_eq!(entities.get(1).unwrap().arm_items(), (Some(apfel), Some(schild)), "Rechtshaender wie bisher");

        entities.apply_metadata(1, &protocol::packets::EntityDataUpdate { main_arm_left: Some(true), ..Default::default() });
        let e = entities.get(1).unwrap();
        assert_eq!(e.arm_items(), (Some(schild), Some(apfel)), "Schild rechts, Apfel links");

        // Schild (Nebenhand) hoch: der RECHTE Arm blockt.
        let an = protocol::packets::EntityDataUpdate {
            living_flags: Some(protocol::packets::LIVING_FLAG_USING_ITEM | protocol::packets::LIVING_FLAG_OFFHAND_ACTIVE),
            ..Default::default()
        };
        entities.apply_metadata(1, &an);
        assert_eq!(entities.get(1).unwrap().using_item_right_arm(), Some(true));

        // Ein Schlag der Haupthand schwingt den linken Arm.
        entities.swing_hand(1, false);
        assert!(!entities.get(1).unwrap().hand_swing(0.0).1);
    }

    // Vanillas EntityIndex.add: eine UUID, ein Entity. Der zweite Spawn wird abgewiesen --
    // sonst standen Koerper und Namensschild doppelt da.
    #[test]
    fn ein_zweiter_spawn_mit_derselben_uuid_wird_abgewiesen() {
        let mut entities = Entities::new();
        let uuid = [7u8; 16];
        entities.spawn(1, uuid, typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, uuid, typ("player"), (5.0, 64.0, 5.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        assert_eq!(entities.len(), 1, "der zweite Spawn darf nicht dazukommen");
        assert!(entities.get(1).is_some(), "der ERSTE bleibt stehen, genau wie in Vanilla");
        assert!(entities.get(2).is_none());
        assert_eq!(entities.duplicate_uuid_count(), 1);

        // Dieselbe ID erneut zu spawnen bleibt erlaubt -- das ist ein Neuaufbau desselben
        // Entities, kein Doppel.
        entities.spawn(1, uuid, typ("player"), (9.0, 64.0, 9.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        assert_eq!(entities.len(), 1);
        assert_eq!(entities.get(1).unwrap().pos.0, 9.0, "der neue Zustand gilt");

        // Und nach dem Entfernen darf die UUID wieder vergeben werden (Spieler betritt den
        // Trackingbereich erneut) -- sonst waere der Spieler danach unsichtbar.
        entities.remove(&[1]);
        entities.spawn(3, uuid, typ("player"), (1.0, 64.0, 1.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        assert_eq!(entities.len(), 1);
        assert!(entities.get(3).is_some(), "nach dem Entfernen muss die UUID wieder gehen");
    }

    // Die Null-UUID ist kein echter Schluessel -- Entities ohne UUID duerfen sich nicht
    // gegenseitig verdraengen.
    #[test]
    fn die_null_uuid_verdraengt_nichts() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("arrow"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [0u8; 16], typ("arrow"), (1.0, 64.0, 1.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        assert_eq!(entities.len(), 2);
    }

    // Ein Passagier hat KEINE eigene Bewegung -- er sitzt da, wo sein Fahrzeug ist. Genau
    // daran hing das statische zweite Namensschild (Display auf dem Kopf eines Spielers).
    #[test]
    fn ein_passagier_folgt_seinem_fahrzeug() {
        let mut entities = Entities::new();
        entities.spawn(1, [1u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [2u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_passengers(1, &[2]);

        // Das Fahrzeug bewegt sich -- der Passagier muss mitkommen, ohne eigenes Paket.
        entities.move_relative(1, 10.0, 0.0, 5.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        let spieler = entities.get(1).unwrap().pos;
        let display = entities.get(2).unwrap().pos;
        assert!((display.0 - spieler.0).abs() < 1e-9, "x folgt: {display:?} gegen {spieler:?}");
        assert!((display.2 - spieler.2).abs() < 1e-9, "z folgt");
        // PASSENGER-Anker ist AT_HEIGHT -- also die Spielerhoehe ueber dessen Fuessen.
        assert!((display.1 - (spieler.1 + PLAYER_BOX_HEIGHT)).abs() < 1e-9, "y = Fahrzeughoehe: {}", display.1);

        // Abgestiegen: der Passagier bleibt stehen, wo er war, statt weiter zu folgen.
        entities.set_passengers(1, &[]);
        let vorher = entities.get(2).unwrap().pos;
        entities.move_relative(1, 10.0, 0.0, 0.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert_eq!(entities.get(2).unwrap().pos, vorher);
    }

    // Verschwindet das Fahrzeug, darf der Passagier nicht an dessen letzter Stelle kleben.
    #[test]
    fn ein_entferntes_fahrzeug_loest_die_mitfahrt() {
        let mut entities = Entities::new();
        entities.spawn(1, [1u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [2u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_passengers(1, &[2]);
        entities.remove(&[1]);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert!(entities.get(2).is_some(), "der Passagier selbst bleibt");
    }

    // Vanilla laesst nur existierende Passagiere aufsteigen -- ein Paket fuer eine noch
    // unbekannte Id darf keinen Traeger erzeugen, der dann keinen Namen mehr zeigt.
    #[test]
    fn passagiere_zaehlen_nur_wenn_sie_existieren() {
        let mut entities = Entities::new();
        entities.spawn(1, [1u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_passengers(1, &[2]);
        assert!(!entities.has_passengers(1));
        entities.spawn(2, [2u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_passengers(1, &[2]);
        assert!(entities.has_passengers(1));
        entities.remove(&[2]);
        assert!(!entities.has_passengers(1), "entfernter Passagier steigt ab");
    }

    // Das klassische Hologramm: unsichtbarer Traeger, sichtbarer Name.
    #[test]
    fn unsichtbarer_armorstand_behaelt_seinen_namen() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("armor_stand"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.apply_metadata(
            1,
            &protocol::packets::EntityDataUpdate {
                flags: Some(protocol::packets::ENTITY_FLAG_INVISIBLE),
                custom_name: Some(Some(vec![span("Shop")])),
                name_visible: Some(true),
                ..Default::default()
            },
        );
        let e = entities.get(1).unwrap();
        assert!(e.invisible);
        assert!(e.name_visible);
        assert_eq!(e.custom_name.as_ref().unwrap()[0].text, "Shop");
    }

    // Das Leuchten steht im selben Flag-Byte und geht mit dem naechsten Byte ohne Bit 6 aus.
    #[test]
    fn leuchten_kommt_aus_bit_6() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("zombie"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let flags = |f| protocol::packets::EntityDataUpdate { flags: Some(f), ..Default::default() };
        entities.apply_metadata(1, &flags(protocol::packets::ENTITY_FLAG_GLOWING | protocol::packets::ENTITY_FLAG_ON_FIRE));
        assert!(entities.get(1).unwrap().glowing);
        assert!(!entities.get(1).unwrap().invisible);
        entities.apply_metadata(1, &flags(protocol::packets::ENTITY_FLAG_ON_FIRE));
        assert!(!entities.get(1).unwrap().glowing);
    }

    // Ein Text-Feld an Index 23 gehoert nur einem text_display. Landete es bei einem
    // item_display, zeigte dieses ploetzlich Text statt seines Gegenstandes.
    #[test]
    fn display_inhalt_wird_nur_beim_passenden_typ_uebernommen() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("item_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.apply_metadata(
            1,
            &protocol::packets::EntityDataUpdate { display_text: Some(vec![span("nicht hierhin")]), ..Default::default() },
        );
        let content = &entities.get(1).unwrap().display.as_ref().unwrap().content;
        assert!(matches!(content, DisplayContent::Item(None)), "item_display darf keinen Text annehmen");

        entities.apply_metadata(
            1,
            &protocol::packets::EntityDataUpdate { display_item: Some(Some(protocol::item::ItemStack::new(936, 1))), ..Default::default() },
        );
        let content = &entities.get(1).unwrap().display.as_ref().unwrap().content;
        assert!(matches!(content, DisplayContent::Item(Some(item)) if item.item_id == 936));
    }

    // Ein Display-Entity darf den Angriffs-Raycast nicht abfangen: in Vanilla ist es nicht
    // anvisierbar, und seine Box hat mit WIDTH/HEIGHT = 0 kein Volumen.
    #[test]
    fn display_entity_faengt_keinen_treffer_ab() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("text_display"), (0.0, 64.0, 2.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [1u8; 16], typ("zombie"), (0.0, 64.0, 4.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        // Strahl von hinten durch das Display hindurch auf den Zombie.
        assert_eq!(entities.pick((0.0, 64.9, 0.0), (0.0, 0.0, 1.0), 10.0), Some(2));
    }

    #[test]
    fn spawn_und_remove_verwalten_die_liste() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        assert_eq!(entities.len(), 1);
        assert!(entities.get(7).is_some());
        entities.remove(&[7]);
        assert!(entities.is_empty());
    }

    #[test]
    fn relative_bewegung_addiert_auf_serverposition() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.move_relative(7, 0.5, 0.0, -0.25);
        entities.move_relative(7, 0.5, 0.0, -0.25);
        let e = entities.get(7).unwrap();
        // Beide Deltas muessen sich auf die SERVER-Position beziehen, nicht auf die gerade
        // interpolierte -- sonst bliebe das Entity dauerhaft hinter seiner echten Position
        // zurueck, weil sich der Interpolationsrueckstand von Paket zu Paket aufsummiert.
        assert!((e.target_pos().0 - 11.0).abs() < 1e-9, "x={}", e.target_pos().0);
        assert!((e.target_pos().2 - -5.5).abs() < 1e-9, "z={}", e.target_pos().2);
    }

    #[test]
    fn drei_ticks_erreichen_die_serverposition_exakt() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.set_position(7, 20.0, 64.0, -5.0);
        for _ in 0..INTERPOLATION_STEPS {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        let e = entities.get(7).unwrap();
        // Der Lerp-Nenner ist die Zahl der NOCH offenen Schritte -- der letzte Schritt ist
        // damit eine volle Angleichung und landet exakt, nicht nur nahe am Ziel.
        assert!((e.pos.0 - 20.0).abs() < 1e-9, "x={}", e.pos.0);
        assert!((e.pos.2 - -5.0).abs() < 1e-9, "z={}", e.pos.2);
    }

    #[test]
    fn erster_tick_geht_genau_ein_drittel_der_strecke() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.set_position(7, 13.0, 64.0, -5.0); // Start x=10 -> Strecke 3
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        let e = entities.get(7).unwrap();
        assert!((e.pos.0 - 11.0).abs() < 1e-9, "1/3 von 3 Bloecken erwartet, x={}", e.pos.0);
    }

    #[test]
    fn render_pos_liegt_zwischen_den_beiden_tickstaenden() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.set_position(7, 13.0, 64.0, -5.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        let e = entities.get(7).unwrap();
        // Zu Tickbeginn steht sie noch auf 10, nach dem Tick auf 11 -- das Bild mischt
        // dazwischen (Vanilla: Entity.getLerpedPos).
        assert!((e.render_pos(0.0).0 - 10.0).abs() < 1e-9);
        assert!((e.render_pos(1.0).0 - 11.0).abs() < 1e-9);
        assert!((e.render_pos(0.5).0 - 10.5).abs() < 1e-9);
    }

    #[test]
    fn treffer_und_bildposition_stammen_aus_derselben_quelle() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.set_position(7, 13.0, 64.0, -5.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        let e = entities.get(7).unwrap();
        // Am Tickende sind beide identisch. Frueher liefen sie auseinander (Wanduhr fuers
        // Bild, Ticks fuer die Trefferbox) -- genau das flaggte bei Grim als "Hitboxes".
        assert_eq!(e.hit_pos(), e.render_pos(1.0));
    }

    #[test]
    fn kopfdrehung_laeuft_ueber_den_kuerzeren_weg() {
        let mut entities = Entities::new();
        entities.spawn(7, [0u8; 16], 1, (10.0, 64.0, -5.0), 350.0, 0.0, (0.0, 0.0, 0.0));
        entities.get_mut(7).unwrap().set_head_yaw(10.0);
        for _ in 0..HEAD_YAW_STEPS {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        let e = entities.get(7).unwrap();
        // 350 -> 10 Grad sind 20 Grad vorwaerts, nicht 340 rueckwaerts. Ohne den
        // Winkel-Lerp drehte sich der Kopf einmal komplett herum.
        let ziel = crate::entity_model::wrap_degrees(e.relative_head_yaw(1.0) + e.render_yaw(1.0) - 10.0);
        assert!(ziel.abs() < 1e-3, "Kopf muss auf 10 Grad stehen, Abweichung {ziel}");
    }

    #[test]
    fn update_mitten_in_der_angleichung_setzt_sie_fort() {
        let mut entities = Entities::new();
        spawn_one(&mut entities);
        entities.set_position(7, 13.0, 64.0, -5.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None); // x = 11
                         // Neues Ziel, bevor die alte Strecke fertig ist: der Startpunkt ist die AKTUELLE
                         // Teilposition, nicht die alte Serverposition -- sonst sprigt das Bild zurueck.
        entities.set_position(7, 14.0, 64.0, -5.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        let e = entities.get(7).unwrap();
        assert!((e.pos.0 - 12.0).abs() < 1e-9, "11 + 1/3 von 3 erwartet, x={}", e.pos.0);
    }

    #[test]
    fn pick_trifft_nur_bei_blick_auf_die_box_und_in_reichweite() {
        let mut entities = Entities::new();
        // Entity bei x=13, Fuesse auf 64 -- Augenhoehe des Angreifers bei 65.62.
        entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let eye = (10.0, 65.62, 0.0);

        // Blick geradeaus (+x): trifft (Distanz 2.7 zur Box-Vorderkante bei 12.7).
        assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 3.0), Some(9));
        // Blick in die Gegenrichtung: kein Treffer.
        assert_eq!(entities.pick(eye, (-1.0, 0.0, 0.0), 3.0), None);
        // Zu kurze Reichweite: kein Treffer.
        assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 2.0), None);
    }

    #[test]
    // Reichweite eines Speers: nur zwischen Mindest- und Hoechstweite; mit Zuschlag zaehlt ein
    // knapper Vorbeischuss, aber nicht, wenn ein Block zwischen Beruehrpunkt und Gegner liegt.
    fn pick_segment_mit_mindestweite_und_zuschlag() {
        let mut entities = Entities::new();
        // Box x 12.7..13.3, z -0.3..0.3.
        entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let eye = (10.0, 65.62, 0.0);
        let frei = |_: (f64, f64, f64), _: (f64, f64, f64)| 1.0;
        let (id, d) = entities.pick_segment(eye, (1.0, 0.0, 0.0), 2.0, 4.5, 0.0, &frei).unwrap();
        assert_eq!(id, 9);
        assert!((d - 2.7).abs() < 1e-9);
        assert_eq!(entities.pick_segment(eye, (1.0, 0.0, 0.0), 2.0, 2.5, 0.0, &frei), None, "zu kurz");
        assert_eq!(entities.pick_segment(eye, (1.0, 0.0, 0.0), 3.5, 6.0, 0.0, &frei), None, "naeher als die Mindestweite");
        // Startpunkt in der Box: Treffer am Start.
        assert_eq!(entities.pick_segment(eye, (1.0, 0.0, 0.0), 3.0, 6.0, 0.0, &frei).map(|(_, d)| d), Some(3.0));
        // 0.2 seitlich vorbei: nur mit Zuschlag, und nur ohne Block dazwischen.
        let daneben = (10.0, 65.62, 0.5);
        assert_eq!(entities.pick_segment(daneben, (1.0, 0.0, 0.0), 0.0, 4.5, 0.0, &frei), None);
        assert_eq!(entities.pick_segment(daneben, (1.0, 0.0, 0.0), 0.0, 4.5, 0.3, &frei).map(|(id, _)| id), Some(9));
        let wand = |_: (f64, f64, f64), _: (f64, f64, f64)| 0.0;
        assert_eq!(entities.pick_segment(daneben, (1.0, 0.0, 0.0), 0.0, 4.5, 0.3, &wand), None);
    }

    #[test]
    fn pick_ueberspringt_items_und_trifft_den_mob_dahinter() {
        let item_type = ENTITY_TYPE_NAMES.iter().position(|n| *n == "item").unwrap() as i32;
        let pig_type = ENTITY_TYPE_NAMES.iter().position(|n| *n == "pig").unwrap() as i32;
        assert!(!is_attackable(item_type));
        assert!(is_attackable(pig_type));

        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], item_type, (12.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [0u8; 16], pig_type, (14.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        // Auf Schweinehoehe (0.9 Bloecke, s. vanilla_dimensions).
        let eye = (10.0, 64.5, 0.0);
        // Das Item liegt naeher in der Schusslinie -- gewaehlt werden muss trotzdem der
        // Mob dahinter, sonst kickt der Server ("attack an invalid entity").
        assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 6.0), Some(2));
    }

    #[test]
    // Der End-Kristall ist 2 x 2 Bloecke -- mit der frueheren Spielerbox (0.6 breit) ging ein
    // Blick knapp neben die Mitte daneben.
    fn end_kristall_hat_seine_vanilla_box() {
        let mut entities = Entities::new();
        entities.spawn(3, [0u8; 16], typ("end_crystal"), (14.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        // 0.8 Bloecke seitlich der Mitte, in Hoehe 1.9: ausserhalb einer Spielerbox, innerhalb
        // der Kristallbox.
        assert_eq!(entities.pick((10.0, 65.9, 0.8), (1.0, 0.0, 0.0), 6.0), Some(3));
    }

    #[test]
    fn pick_waehlt_die_naechste_von_zwei_boxen() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], player_type(), (12.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [0u8; 16], player_type(), (14.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let eye = (10.0, 65.0, 0.0);
        assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 6.0), Some(1));
    }

    fn player_type() -> i32 {
        ENTITY_TYPE_NAMES.iter().position(|n| *n == "player").unwrap() as i32
    }

    #[test]
    fn geduckter_spieler_wird_nicht_ueber_den_kopf_getroffen() {
        let mut entities = Entities::new();
        entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        // Auf 65.6 zielt man beim STEHENDEN Ziel (1.8 hoch, also bis 65.8) noch auf den Kopf.
        let eye = (10.0, 65.6, 0.0);
        assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 3.0), Some(9));

        // Geduckt reicht die Box nur bis 65.5 -- der Strahl geht darueber hinweg. Genau das
        // ist der Angriff, den GrimAC als Hitboxes flaggt, wenn der Client trotzdem sendet.
        entities.set_pose(9, POSE_CROUCHING);
        assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 3.0), None);

        // Auf Brusthoehe trifft er auch geduckt.
        assert_eq!(entities.pick((10.0, 65.2, 0.0), (1.0, 0.0, 0.0), 3.0), Some(9));
    }

    #[test]
    // Fremde Spieler legen sich in der Schwimm-Pose waagerecht, in 0.09-Schritten je Tick wie
    // LivingEntity.updateSwimAmount -- und richten sich danach genauso langsam wieder auf.
    fn fremder_spieler_legt_sich_in_der_schwimmpose_hin() {
        let (world, shapes) = (crate::chunk::World::new(), crate::BlockShapes::all_full_cubes());
        let mut entities = Entities::new();
        entities.spawn(4, [0u8; 16], player_type(), (0.5, 64.0, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_pose(4, POSE_SWIMMING);
        entities.tick(&world, &shapes, None);
        let lean = entities.get(4).unwrap().lean(1.0);
        assert!((lean.amount - 0.09).abs() < 1e-6);
        assert!(lean.swimming && !lean.in_water, "leere Welt: kriechend, nicht im Wasser");
        assert!((entities.get(4).unwrap().lean(0.5).amount - 0.045).abs() < 1e-6, "zwischen den Ticks interpoliert");
        for _ in 0..20 {
            entities.tick(&world, &shapes, None);
        }
        assert_eq!(entities.get(4).unwrap().lean(1.0).amount, 1.0);
        entities.set_pose(4, 0);
        entities.tick(&world, &shapes, None);
        assert!((entities.get(4).unwrap().lean(1.0).amount - 0.91).abs() < 1e-6);
    }

    #[test]
    fn schwimmender_spieler_ist_nur_eine_halbe_box_hoch() {
        let mut entities = Entities::new();
        entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_pose(9, POSE_SWIMMING);
        // 0.6 hoch -> bis 64.6.
        assert_eq!(entities.pick((10.0, 64.3, 0.0), (1.0, 0.0, 0.0), 3.0), Some(9));
        assert_eq!(entities.pick((10.0, 64.9, 0.0), (1.0, 0.0, 0.0), 3.0), None);
    }

    #[test]
    fn trefferposition_laeuft_in_drei_tick_schritten_und_landet_exakt() {
        let mut entities = Entities::new();
        entities.spawn(9, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.set_position(9, 3.0, 64.0, 0.0);

        // Vor dem ersten Tick steht die Trefferbox noch am Ausgangspunkt -- die Wanduhr
        // bewegt sie NICHT (das tut nur die Render-Position).
        assert_eq!(entities.get(9).unwrap().hit_pos().0, 0.0);
        // Nenner ist die Zahl der offenen Schritte: 1/3, dann 1/2, dann 1/1.
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert!((entities.get(9).unwrap().hit_pos().0 - 1.0).abs() < 1e-9);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert!((entities.get(9).unwrap().hit_pos().0 - 2.0).abs() < 1e-9);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        // Letzter Schritt: exakt auf dem Ziel, nicht bloss in der Naehe -- sonst driftete die
        // Box bei jedem Update ein Stueck weiter hinter dem Server her.
        assert_eq!(entities.get(9).unwrap().hit_pos().0, 3.0);
        // Weitere Ticks ohne neues Update aendern nichts mehr.
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert_eq!(entities.get(9).unwrap().hit_pos().0, 3.0);
    }

    fn typ(name: &str) -> i32 {
        ENTITY_TYPE_NAMES.iter().position(|n| *n == name).unwrap_or_else(|| panic!("Typ {name} fehlt")) as i32
    }

    // In Vanilla blitzt NUR ein LivingEntity rot auf: Entity.onDamaged ist leer, erst
    // LivingEntity.onDamaged setzt hurtTime. Die Tabelle stammt aus der Klassenhierarchie im
    // Jar -- diese Stichproben sind die Gegenprobe, dass sie richtig ausgerichtet ist
    // (ein Versatz um einen Index waere sonst schwer zu bemerken).
    #[test]
    fn lebendig_erkennt_die_vanilla_klassenhierarchie() {
        for name in ["player", "zombie", "pig", "villager", "ender_dragon"] {
            assert!(is_living(typ(name)), "{name} ist in Vanilla ein LivingEntity");
        }
        // Der Ruestungsstaender ist der Fall, den eine Handliste typischerweise falsch
        // einsortiert: er sieht aus wie Deko, IST aber ein LivingEntity.
        assert!(is_living(typ("armor_stand")), "armor_stand ist ein LivingEntity");
        for name in ["item", "arrow", "oak_boat", "minecart", "tnt", "item_frame", "painting", "end_crystal"] {
            assert!(!is_living(typ(name)), "{name} ist KEIN LivingEntity");
        }
        // Unbekannte/negative Indizes duerfen nicht in Panik enden -- der Typ kommt roh vom
        // Server, ein neuerer Server kann Typen schicken, die diese Tabelle nicht kennt.
        assert!(!is_living(-1));
        assert!(!is_living(i32::MAX));
    }

    // LivingEntity.onDamaged setzt hurtTime = 10, LivingEntity.tick zaehlt es um 1 herunter.
    // Nach genau 10 Ticks ist Schluss -- nicht frueher (Blitz zu kurz) und nicht spaeter.
    #[test]
    fn schadens_blitz_haelt_genau_zehn_ticks() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        assert!(!entities.get(1).unwrap().is_hurt(), "ungetroffen blitzt nichts");

        entities.damaged(1);
        for tick in 0..MAX_HURT_TIME {
            assert!(entities.get(1).unwrap().is_hurt(), "Tick {tick}: muss noch blitzen");
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        assert!(!entities.get(1).unwrap().is_hurt(), "nach {MAX_HURT_TIME} Ticks ist der Blitz vorbei");
    }

    // Die einstellbare Blitzdauer zaehlt ab dem Treffer, unabhaengig von Vanillas hurtTime.
    #[test]
    fn schadens_blitz_dauer_ist_einstellbar() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        assert!(!entities.get(1).unwrap().damage_flash(40), "ungetroffen blitzt nichts");
        entities.damaged(1);
        for _ in 0..20 {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        let e = entities.get(1).unwrap();
        assert!(!e.damage_flash(10), "Vanilla-Dauer ist vorbei");
        assert!(e.damage_flash(21), "laengere Dauer laeuft noch");
        assert!(!e.damage_flash(20));
    }

    // Ein erneuter Treffer setzt den Zaehler ZURUECK auf 10 (Vanilla weist neu zu, es wird
    // nichts aufaddiert) -- sonst waere schnelles Trommelfeuer nach kurzer Zeit ein
    // Dauerrot statt eines Flackerns.
    #[test]
    fn erneuter_treffer_setzt_den_zaehler_zurueck_statt_zu_addieren() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.damaged(1);
        for _ in 0..5 {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        entities.damaged(1);
        for _ in 0..(MAX_HURT_TIME - 1) {
            entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        }
        assert!(entities.get(1).unwrap().is_hurt(), "nach dem zweiten Treffer laufen wieder volle 10 Ticks");
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        assert!(!entities.get(1).unwrap().is_hurt());
    }

    // Ein Boot nimmt Schaden (der Server schickt dafuer auch ein Damage-Paket), blitzt in
    // Vanilla aber nicht -- Entity.onDamaged tut nichts.
    #[test]
    fn nicht_lebewesen_blitzen_nicht() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("oak_boat"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.damaged(1);
        assert!(!entities.get(1).unwrap().is_hurt(), "ein Boot blitzt nicht");
    }

    // Ein Paket fuer eine unbekannte ID darf nichts tun und nicht in Panik enden -- es kann
    // ein Entity betreffen, das wir gar nicht verfolgen.
    #[test]
    fn schaden_an_unbekannter_id_ist_folgenlos() {
        let mut entities = Entities::new();
        entities.damaged(4711);
    }

    // Jeder Typ mit Item-Darstellung steht in der Typtabelle (sonst griffe der Renderer nie).
    #[test]
    fn item_darstellung_kennt_die_vanilla_typen() {
        for name in ["egg", "snowball", "ender_pearl", "experience_bottle", "splash_potion", "lingering_potion", "eye_of_ender", "firework_rocket"] {
            assert!(matches!(item_render_kind(typ(name)), Some(ItemRenderKind::Flying { scale, .. }) if scale == 1.0), "{name}");
        }
        assert!(matches!(item_render_kind(typ("fireball")), Some(ItemRenderKind::Flying { scale, .. }) if scale == 3.0));
        assert_eq!(item_render_kind(typ("item")), Some(ItemRenderKind::Dropped));
        assert_eq!(item_render_kind(typ("zombie")), None);
    }

    // Der Stack an Index 8 landet nur bei Typen mit Item-Darstellung, mit Anzahl und Trankfarbe.
    #[test]
    fn item_stapel_aus_den_metadaten() {
        let mut entities = Entities::new();
        entities.spawn(1, [0u8; 16], typ("item"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        entities.spawn(2, [1u8; 16], typ("zombie"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
        let update = protocol::packets::EntityDataUpdate { item_stack: Some(Some(protocol::item::ItemStack::new(936, 20))), ..Default::default() };
        entities.apply_metadata(1, &update);
        entities.apply_metadata(2, &update);
        let item = entities.get(1).unwrap();
        assert_eq!(item.item.map(|i| i.item_id), Some(936));
        assert_eq!(item.item_count, 20);
        assert_eq!(item.item_seed, 936);
        assert_eq!(entities.get(2).unwrap().item, None);
    }
}
