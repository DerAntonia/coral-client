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

mod spawning;
mod updates;

mod ticking;
mod motion_state;
mod entity_core;
mod projectiles;
mod anim_debug;
pub use anim_debug::{anim_debug_an, anim_log};
pub use entity_core::*;
pub use projectiles::*;

#[must_use] 
pub fn entity_type_name(type_index: i32) -> Option<&'static str> {
    usize::try_from(type_index).ok().and_then(|i| ENTITY_TYPE_NAMES.get(i).copied())
}

// Vanilla kickt den Client mit "Attempting to attack an invalid entity", wenn das Ziel
// ein ItemEntity, ExperienceOrb oder AbstractArrow (Pfeil/Spektralpfeil/Dreizack) ist --
// exakt diese Liste aus ServerGamePacketListenerImpl. Solche Ziele darf der Raycast fuer
// den Angriff also gar nicht erst hergeben.
#[must_use] 
pub fn is_attackable(type_index: i32) -> bool {
    !matches!(entity_type_name(type_index), Some("item" | "experience_orb" | "arrow" | "spectral_arrow" | "trident") | None)
}

#[must_use] 
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

#[must_use] 
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
    // Ueber wie viele Ticks das Display einer neuen Position folgt (teleport_duration,
    // Display.interpolation). 0 = springt sofort -- Vanillas Vorgabe fuer Displays.
    pub teleport_duration: i32,
    // Der Fortschritt (0..1), mit dem das Display zuletzt GEZEICHNET wurde -- Vanillas
    // Display.lastProgress. Eine neue Transformation startet von der Lage bei genau diesem
    // Fortschritt, also von dem, was gerade zu sehen war.
    pub last_progress: std::cell::Cell<f32>,
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
    #[must_use] 
    pub const fn target(&self) -> DisplayTransform {
        DisplayTransform { translation: self.translation, scale: self.scale, left_rotation: self.left_rotation, right_rotation: self.right_rotation }
    }

    // Die Transformation zum Entity-Alter `age` (Ticks samt Zwischenbild-Anteil):
    // DisplayEntity.getLerpProgress + AbstractInterpolator -- Verschiebung und Skalierung
    // linear, Drehungen per slerp.
    // Merkt sich den Fortschritt wie Vanillas calculateInterpolationProgress (s. last_progress).
    #[must_use] 
    pub fn transform_at(&self, age: f32) -> DisplayTransform {
        if self.interp_duration <= 0 {
            return self.target();
        }
        let t = ((age - self.interp_start) / self.interp_duration as f32).clamp(0.0, 1.0);
        self.last_progress.set(t);
        self.transform_with(t)
    }

    // Die Transformation beim Fortschritt `t` (0..1) zwischen interp_from und Ziel.
    #[must_use] 
    pub fn transform_with(&self, t: f32) -> DisplayTransform {
        let ziel = self.target();
        let Some(von) = self.interp_from.filter(|_| self.interp_duration > 0) else { return ziel };
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
    #[must_use] 
    pub const fn new(content: DisplayContent) -> Self {
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
            teleport_duration: 0,
            last_progress: std::cell::Cell::new(0.0),
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
        Self::Text(TextDisplay::default())
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
#[must_use] 
pub fn is_living(type_index: i32) -> bool {
    usize::try_from(type_index).ok().and_then(|i| crate::entity_types::ENTITY_TYPE_IS_LIVING.get(i).copied()).unwrap_or(false)
}

// Hat der Typ in Vanilla einen Positions-Interpolator (Entity.getInterpolation != null)?
// Nur Lebewesen, Boote, Loren und XP-Kugeln; Display-Entities interpolieren eigens.
#[must_use] 
pub fn has_interpolator(type_index: i32) -> bool {
    if is_living(type_index) {
        return true;
    }
    entity_type_name(type_index).is_some_and(|n| n.ends_with("_boat") || n.ends_with("_raft") || n.ends_with("minecart") || n == "experience_orb")
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
pub const POSE_SLEEPING: u8 = 2;
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
#[must_use] 
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

impl Entity {
    /// Schleicht dieses Entity gerade? Vanillas `isInSneakingPose()` -- der Zustand haengt an
    /// der POSE, nicht am gedrueckten Shift: wer schleicht, aber wegen einer Decke ueber sich
    /// nicht aufstehen kann, ist ebenfalls CROUCHING.
    #[must_use] 
    pub const fn sneaking(&self) -> bool {
        self.pose == POSE_CROUCHING
    }

    // Stand das Entity beim letzten Tick im Wasser (s. touches_water)?
    #[must_use] 
    pub const fn is_in_water(&self) -> bool {
        self.in_water
    }

    // TamableAnimal.isInSittingPose (nur bei Katzen gepflegt, s. apply_metadata).
    #[must_use] 
    pub const fn sitting(&self) -> bool {
        self.tamable_flags & protocol::packets::TAMABLE_FLAG_SITTING != 0
    }

    // Benutzt dieses Wesen gerade ein Item, und wenn ja: mit dem RECHTEN Arm?
    //
    // Vanilla (LivingEntity.getActiveHand) liest die Hand aus demselben Byte: Bit 2 gesetzt
    // heisst Nebenhand. Welcher Arm das ist, sagt die Haupthand (s. left_handed).
    #[must_use] 
    pub fn using_item_right_arm(&self) -> Option<bool> {
        let haupthand = self.using_main_hand()?;
        Some(haupthand != self.left_handed)
    }

    // Das Item, das gerade benutzt wird, und seit wie vielen Ticks (s. use_ticks).
    #[must_use] 
    pub fn used_item(&self) -> Option<(HeldItem, u32)> {
        let item = if self.using_main_hand()? { self.main_hand } else { self.off_hand };
        Some((item?, self.use_ticks))
    }

    // Wie weit das Entity im letzten Tick gefallen ist (positiv = nach unten), in Bloecken.
    #[must_use] 
    pub fn last_drop(&self) -> f64 {
        self.last_pos.1 - self.pos.1
    }

    // Benutzt dieses Wesen gerade ein Item, und wenn ja: das der Haupthand?
    const fn using_main_hand(&self) -> Option<bool> {
        if self.living_flags & protocol::packets::LIVING_FLAG_USING_ITEM == 0 {
            return None;
        }
        Some(self.living_flags & protocol::packets::LIVING_FLAG_OFFHAND_ACTIVE == 0)
    }

    // Was der rechte und was der linke Arm haelt (ArmedEntityRenderState: die Haupthand am
    // Arm der Haupthand, die Nebenhand am anderen).
    #[must_use] 
    pub const fn arm_items(&self) -> (Option<HeldItem>, Option<HeldItem>) {
        if self.left_handed { (self.off_hand, self.main_hand) } else { (self.main_hand, self.off_hand) }
    }

    // Speerhaltung je Arm (rechts, links), s. main_hand_spear/off_hand_spear.
    #[must_use] 
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
    #[must_use] 
    pub fn hit_size(&self) -> (f64, f64) {
        if entity_type_name(self.entity_type) == Some("interaction") {
            self.interaction_size
        } else {
            self.box_size()
        }
    }

    #[must_use] 
    pub const fn in_ground(&self) -> bool {
        self.in_ground
    }

    // Ein fliegender Krit-Pfeil: (Position, Geschwindigkeit) fuer die Partikelspur, die
    // AbstractArrow.tick im Client jeden Tick streut. None, wenn er nicht kritisch ist oder
    // schon steckt.
    // Auch None im ersten Tick: die Spur liegt eine bis zwei Tick-Strecken HINTER dem Pfeil
    // (s. App::tick_crit_trails), nach einem Tick also hinter dem Abschusspunkt, wo er nie
    // war -- beim eigenen Schuss hinter dem Schuetzen (Nutzer 2026-11-12). Ab dem zweiten
    // Tick beginnt sie genau am Abschusspunkt.
    #[must_use] 
    pub fn crit_trail(&self) -> Option<((f64, f64, f64), (f64, f64, f64))> {
        (self.crit_arrow && !self.in_ground && self.age_ticks >= 2.0 && self.projectile == Some(ProjectileKind::Arrow))
            .then_some((self.pos, self.vel))
    }

    #[must_use] 
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
    // (Breite, Hoehe) der Hitbox samt Baby-Halbierung (LivingEntity.getAgeScale: 0.5) und
    // Attribut SCALE (LivingEntity.getDimensions: EntityDimensions.scale(getScale())).
    #[must_use] 
    pub fn box_size(&self) -> (f64, f64) {
        let (w, h) = hit_box_size(self.entity_type, self.pose);
        if self.pose == POSE_SLEEPING && is_player(self.entity_type) {
            return (w, h);
        }
        let f = if self.baby { 0.5 } else { 1.0 } * f64::from(self.scale);
        (w * f, h * f)
    }

    #[must_use] 
    pub fn render_pos(&self, tick_progress: f32) -> (f64, f64, f64) {
        let t = f64::from(tick_progress.clamp(0.0, 1.0));
        (
            self.last_pos.0 + (self.pos.0 - self.last_pos.0) * t,
            self.last_pos.1 + (self.pos.1 - self.last_pos.1) * t,
            self.last_pos.2 + (self.pos.2 - self.last_pos.2) * t,
        )
    }

    // Drehung des Modells fuers Bild: bei Lebewesen die KOERPERdrehung (LivingEntityRenderer:
    // lerpAngleDegrees(delta, lastBodyYaw, bodyYaw)), sonst getLerpedYaw. Ueber den Winkel-Lerp,
    // damit der Sprung 359 -> 1 nicht einmal komplett herum geht.
    #[must_use] 
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
    #[must_use] 
    pub fn view_yaw(&self, tick_progress: f32) -> f32 {
        lerp_angle(tick_progress.clamp(0.0, 1.0), self.last_yaw_deg, self.yaw_deg)
    }

    // Augenhoehe ueber den Fuessen: bei Spielern je Pose, sonst der Vanilla-Standard von 85 %
    // der Hoehe (einzelne Typen weichen in Vanilla davon ab).
    #[must_use] 
    pub fn eye_height(&self) -> f64 {
        if is_player(self.entity_type) {
            // Schlafend gilt die feste Box ohne SCALE (LivingEntity.getDimensions).
            return match self.pose {
                POSE_SLEEPING => 0.2,
                POSE_SWIMMING | POSE_GLIDING | POSE_SPIN_ATTACK => 0.4 * f64::from(self.scale),
                POSE_CROUCHING => 1.27 * f64::from(self.scale),
                _ => 1.62 * f64::from(self.scale),
            };
        }
        self.box_size().1 * 0.85
    }

    #[must_use] 
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
    #[must_use] 
    pub fn hit_pos(&self) -> (f64, f64, f64) {
        let (x, y, z) = self.pos;
        (x, y + self.box_y_offset(), z)
    }

    // Versatz der Box-Unterkante gegenueber der Position (s. hit_pos).
    #[must_use] 
    pub fn box_y_offset(&self) -> f64 {
        match crate::entity_type_name(self.entity_type) {
            Some("wind_charge" | "breeze_wind_charge") => -f64::from(0.15f32),
            _ => 0.0,
        }
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
    // Die eigene Figur (Id, Fusspunkt, Blickrichtung, Hoehe): sie steht nicht in by_id, kann
    // aber Fahrzeug sein -- Server setzen ihr Namensschild-Displays auf, die mitlaufen
    // muessen (s. set_own_body).
    own_body: Option<(i32, (f64, f64, f64), f32, f64)>,
    // Mehr Animationen, Schalter BODY_TURN: wer isst, trinkt oder blockt, dreht den Koerper
    // zum Blick (s. Entity::tick_body_yaw). Von der App je Tick gesetzt.
    pub body_turn_on_use: bool,
}

impl Entities {
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    // Wer sitzt auf wem (EntityPassengersSetS2CPacket). Eine LEERE Liste heisst: alle
    // bisherigen Passagiere dieses Fahrzeugs sind abgestiegen.
    //
    // Wie Vanillas onEntityPassengersSet: ein unbekanntes Fahrzeug wird ignoriert, und nur
    // Passagiere, die schon existieren, steigen auf (`if (entity2 != null) startRiding`).
    // Das zaehlt, weil has_passengers ueber Namensschilder entscheidet.
    // Wo die eigene Figur gerade steht -- jeden Tick und vor jedem Passagier-Paket.
    pub const fn set_own_body(&mut self, id: i32, pos: (f64, f64, f64), yaw_deg: f32, height: f64) {
        self.own_body = Some((id, pos, yaw_deg, height));
    }

    pub fn set_passengers(&mut self, vehicle: i32, passengers: &[i32]) {
        if !self.by_id.contains_key(&vehicle) && self.own_body.map(|o| o.0) != Some(vehicle) {
            return;
        }
        if anim_debug::anim_debug_an() {
            anim_log(format_args!("mitfahrer fahrzeug={vehicle} typ={} passagiere={passengers:?}", self.by_id.get(&vehicle).and_then(|e| entity_type_name(e.entity_type)).unwrap_or("eigener/unbekannt")));
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
    #[must_use] 
    pub fn seat_pos(&self, vehicle: i32, seat: crate::riding::Seat, passenger_type: &str) -> Option<(f64, f64, f64)> {
        if let Some((_, pos, yaw, hoehe)) = self.own_body.filter(|o| o.0 == vehicle) {
            let p = crate::riding::riding_pos("player", hoehe, [pos.0, pos.1, pos.2], yaw, seat, passenger_type);
            return Some((p[0], p[1], p[2]));
        }
        let v = self.by_id.get(&vehicle)?;
        let name = entity_type_name(v.entity_type)?;
        let hoehe = v.box_size().1;
        let p = crate::riding::riding_pos(name, hoehe, [v.pos.0, v.pos.1, v.pos.2], v.yaw_deg, seat, passenger_type);
        Some((p[0], p[1], p[2]))
    }

    // Das Fahrzeug, das der eigene Client lenkt (None = keins), s. Entity::driven.
    pub fn set_driven(&mut self, id: Option<i32>) {
        for (eid, e) in &mut self.by_id {
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
    #[must_use] 
    pub fn is_riding(&self, id: i32) -> bool {
        self.vehicle_of.contains_key(&id)
    }

    // Ruderstand eines selbst gelenkten Boots: Vanilla setzt ihn auf dem Client selbst
    // (AbstractBoat.setPaddleState im Tick des Fahrers), das Echo des Servers kaeme einen Ping
    // zu spaet.
    pub fn set_paddles(&mut self, id: i32, paddles: [bool; 2]) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.boat_paddles = paddles;
        }
    }

    // Worauf dieses Entity sitzt, und auf welchem Platz.
    #[must_use] 
    pub fn vehicle_seat(&self, id: i32) -> Option<(i32, crate::riding::Seat)> {
        self.vehicle_of.get(&id).copied()
    }

    #[must_use] 
    pub fn has_passengers(&self, vehicle: i32) -> bool {
        self.vehicle_of.values().any(|(v, _)| *v == vehicle)
    }

    pub fn passengers_of(&self, vehicle: i32) -> impl Iterator<Item = i32> + '_ {
        self.vehicle_of.iter().filter(move |(_, (v, _))| *v == vehicle).map(|(&p, _)| p)
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
            if anim_debug::anim_debug_an() {
                let d = ((pos.0 - e.pos.0).powi(2) + (pos.1 - e.pos.1).powi(2) + (pos.2 - e.pos.2).powi(2)).sqrt();
                if d > 0.5 {
                    anim_log(format_args!("passagier_sprung id={passagier} fahrzeug={} weite={d:.3} von={} nach={}", self.vehicle_of[&passagier].0, anim_debug::fmt_pos(e.pos), anim_debug::fmt_pos(pos)));
                }
            }
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
    #[must_use] 
    pub const fn duplicate_uuid_count(&self) -> u32 {
        self.duplicate_uuids
    }

    #[must_use] 
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&i32, &Entity)> {
        self.by_id.iter()
    }

    #[must_use] 
    pub fn get(&self, id: i32) -> Option<&Entity> {
        self.by_id.get(&id)
    }

    pub fn get_mut(&mut self, id: i32) -> Option<&mut Entity> {
        self.by_id.get_mut(&id)
    }

    // coral-record: alle Entities als die Pakete, die sie so erzeugt haetten, fuer den Snapshot
    // einer Aufnahme ab der Mitte. Dazu, wer auf wem sitzt (Fahrzeug, Passagiere nach Sitz).
    #[must_use] 
    pub fn snapshot(&self) -> (Vec<EntitySnapshot>, Vec<(i32, Vec<i32>)>) {
        let entities = self.by_id.iter().map(|(&id, e)| e.snapshot(id)).collect();
        let mut sitze: HashMap<i32, Vec<(usize, i32)>> = HashMap::new();
        for (&p, &(v, seat)) in &self.vehicle_of {
            sitze.entry(v).or_default().push((seat.index, p));
        }
        let passengers = sitze
            .into_iter()
            .map(|(v, mut ps)| {
                ps.sort_unstable();
                (v, ps.into_iter().map(|(_, p)| p).collect())
            })
            .collect();
        (entities, passengers)
    }
}

#[cfg(test)]
mod tests;
