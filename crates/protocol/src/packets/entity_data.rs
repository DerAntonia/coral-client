// Ausruestung und Entity-Daten.

use super::*;

// Ein Eintrag des Pakets: Slot und (falls belegt) der Stapel darin. None = Slot leer, das
// ist die normale Art, ein Item aus der Hand verschwinden zu lassen.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct EquipmentEntry {
    pub slot: u8,
    pub item: Option<ItemStack>,
}

// Sicherheitsdeckel gegen ein absichtlich endloses Fortsetzungsbit: mehr als alle acht
// Slots kann ein ehrlicher Server nicht schicken, ein paar Reserve schadet aber nicht.
pub(crate) const MAX_EQUIPMENT_ENTRIES: usize = 16;

pub fn read_set_equipment(payload: &[u8]) -> Result<(i32, Vec<EquipmentEntry>), SlotError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let mut entries = Vec::new();
    loop {
        let raw = r.read_u8()?;
        entries.push(EquipmentEntry { slot: raw & 0x7F, item: read_optional_slot(&mut r)? });
        if raw & 0x80 == 0 {
            return Ok((entity_id, entries));
        }
        if entries.len() >= MAX_EQUIPMENT_ENTRIES {
            return Err(SlotError::Read);
        }
    }
}

// Set Entity Data (0x61): der Metadaten-Strom einer Entity. Aufbau (EntityTrackerUpdate-
// S2CPacket / DataTracker.SerializedEntry): VarInt entityId, dann beliebig viele Eintraege
// aus Index (u8), Serializer-ID (VarInt) und Wert, beendet durch den Index 0xFF. Der Server
// schickt nur GEAENDERTE Felder, nicht den ganzen Satz.
//
// ID aus der vollstaendig gegen PlayStateFactories abgezaehlten Clientbound-Tabelle -- die
// direkten Nachbarn Set Entity Link 0x62, Set Entity Motion 0x63, Set Equipment 0x64 und Set
// Health 0x66 waren bereits verifiziert und werden exakt getroffen.
pub const CB_PLAY_SET_ENTITY_DATA: i32 = 0x61;

// Feld-Indizes aus Entity.class (1.21.11 yarn, Reihenfolge der registerData-Aufrufe):
// 0 FLAGS (Byte), 1 AIR (Int), 2 CUSTOM_NAME (Optional<Text>), 3 NAME_VISIBLE (Bool),
// 4 SILENT (Bool), 5 NO_GRAVITY (Bool), 6 POSE (EntityPose), 7 FROZEN_TICKS (Int).
pub(crate) const ENTITY_DATA_INDEX_FLAGS: u8 = 0;
// Entity.AIR (Int): Restluft in Ticks, 300 = voll (Entity.getMaxAir). Fuer die Blasenleiste
// im HUD -- der Server schickt den Wert nur, wenn er sich aendert.
pub(crate) const ENTITY_DATA_INDEX_AIR: u8 = 1;
pub(crate) const ENTITY_DATA_INDEX_POSE: u8 = 6;
// Entity.FROZEN_TICKS (Int): Ticks im Pulverschnee, ab 140 komplett eingefroren.
pub(crate) const ENTITY_DATA_INDEX_FROZEN_TICKS: u8 = 7;
// LivingEntity.SLEEPING_POSITION. Der Index folgt der REIHENFOLGE DER STATISCHEN
// registerData-Felder (nicht der von initDataTracker!): Entity belegt 0..7, LivingEntity
// deklariert dann LIVING_FLAGS(8), HEALTH(9), POTION_SWIRLS(10), POTION_SWIRLS_AMBIENT(11),
// STUCK_ARROW_COUNT(12), STINGER_COUNT(13), SLEEPING_POSITION(14).
// Genau dieses Feld IST der Schlafzustand: LivingEntity.isSleeping() ist nichts anderes als
// `getSleepingPosition().isPresent()` -- ein eigenes Schlaf-Paket gibt es nicht mehr.
pub(crate) const ENTITY_DATA_INDEX_SLEEPING_POS: u8 = 14;
// LivingEntity.POTION_SWIRLS / POTION_SWIRLS_AMBIENT, s. EntityDataUpdate::potion_swirls.
pub(crate) const ENTITY_DATA_INDEX_POTION_SWIRLS: u8 = 10;
pub(crate) const ENTITY_DATA_INDEX_POTION_SWIRLS_AMBIENT: u8 = 11;
// PassiveEntity.CHILD und ZombieEntity.BABY: das erste eigene Feld nach MobEntity (Index 15),
// beide ein Boolean. Andere Typen haben an derselben Stelle andere Booleans (Guardian, Raider,
// Ghast, ...) -- welcher Typ gemeint ist, entscheidet der Empfaenger (s. world::can_be_baby).
pub(crate) const AGEABLE_INDEX_BABY: u8 = 16;
// Entity.CUSTOM_NAME (Optional<Text>) und Entity.NAME_VISIBLE (Bool). Zusammen sind das die
// klassischen "Hologramme": ein unsichtbarer Armorstand, dessen Namensschild dauerhaft
// eingeblendet ist (Entity.shouldRenderName() == isCustomNameVisible()). Dieselben zwei
// Felder tragen auch den Namen eines umbenannten Mobs.
pub(crate) const ENTITY_DATA_INDEX_CUSTOM_NAME: u8 = 2;
pub(crate) const ENTITY_DATA_INDEX_NAME_VISIBLE: u8 = 3;

// ArmorStandEntity.ARMOR_STAND_FLAGS -- erstes eigenes Feld nach LivingEntity (0..14), also
// Index 15. Nur zwei seiner Bits interessieren, aber die entscheiden ueber die HOEHE des
// Standes und damit darueber, wo sein Namensschild sitzt: MARKER ist 0x0 gross, SMALL die
// halbe Groesse, sonst 0.5 x 1.975 (ArmorStandEntity.MARKER_DIMENSIONS/SMALL_DIMENSIONS).
pub(crate) const ARMOR_STAND_INDEX_FLAGS: u8 = 15;
pub const ARMOR_STAND_FLAG_SMALL: u8 = 1;
pub const ARMOR_STAND_FLAG_MARKER: u8 = 16;
// Beide Namen meinen denselben Index (s. den Byte-Zweig in read_set_entity_data). Hier
// festgeschrieben, damit ein Versionswechsel, der einen der beiden verschiebt, den Bau
// abbricht -- statt still das falsche Feld zu fuellen.
const _: () = assert!(ARMOR_STAND_INDEX_FLAGS == DISPLAY_INDEX_BILLBOARD);
pub const ARMOR_STAND_FLAG_SHOW_ARMS: u8 = 4;
pub const ARMOR_STAND_FLAG_NO_BASEPLATE: u8 = 8;
// Die sechs Gelenkwinkel (Rotations, Grad) danach: Kopf, Rumpf, linker Arm, rechter Arm, linkes
// Bein, rechtes Bein (ArmorStand.DATA_HEAD_POSE .. DATA_RIGHT_LEG_POSE). Andere Typen fuehren an
// diesen Indizes andere Serializer -- ROTATION gibt es nur beim Armorstand.
pub(crate) const ARMOR_STAND_INDEX_HEAD_POSE: u8 = 16;
pub(crate) const ARMOR_STAND_POSE_COUNT: u8 = 6;

// --- DisplayEntity (1.21.11 yarn, net/minecraft/entity/decoration/DisplayEntity.class) -----
//
// Dieselbe Zaehlregel wie oben: Entity belegt 0..7, DisplayEntity deklariert danach in
// STATISCHER Feldreihenfolge START_INTERPOLATION(8), INTERPOLATION_DURATION(9),
// TELEPORT_DURATION(10), TRANSLATION(11), SCALE(12), LEFT_ROTATION(13), RIGHT_ROTATION(14),
// BILLBOARD(15), BRIGHTNESS(16), VIEW_RANGE(17), SHADOW_RADIUS(18), SHADOW_STRENGTH(19),
// WIDTH(20), HEIGHT(21), GLOW_COLOR_OVERRIDE(22).
//
// ACHTUNG, haeufige Fehlerquelle: `initDataTracker` listet die Felder in einer ANDEREN
// Reihenfolge (TELEPORT_DURATION zuerst). Massgeblich ist die Reihenfolge der
// registerData-Aufrufe, nicht die des Builders.
pub(crate) const DISPLAY_INDEX_START_INTERPOLATION: u8 = 8;
pub(crate) const DISPLAY_INDEX_INTERPOLATION_DURATION: u8 = 9;
pub(crate) const DISPLAY_INDEX_TELEPORT_DURATION: u8 = 10;
const _: () = assert!(ITEM_FRAME_INDEX_ROTATION == DISPLAY_INDEX_TELEPORT_DURATION);
pub(crate) const DISPLAY_INDEX_TRANSLATION: u8 = 11;
pub(crate) const DISPLAY_INDEX_SCALE: u8 = 12;
pub(crate) const DISPLAY_INDEX_LEFT_ROTATION: u8 = 13;
pub(crate) const DISPLAY_INDEX_RIGHT_ROTATION: u8 = 14;
pub(crate) const DISPLAY_INDEX_BILLBOARD: u8 = 15;
pub(crate) const DISPLAY_INDEX_VIEW_RANGE: u8 = 17;
// PlayerEntity.ABSORPTION_AMOUNT (Float). LivingEntity belegt bis 14, PlayerLikeEntity 15
// (MAIN_ARM_ID) und 16 (PLAYER_MODE_CUSTOMIZATION_ID), PlayerEntity beginnt mit diesem Feld
// (Reihenfolge der statischen registerData-Aufrufe im Jar). Derselbe Index ist bei einem
// Display die Sichtweite -- welches Feld gemeint ist, weiss erst der Empfaenger.
pub(crate) const PLAYER_INDEX_ABSORPTION: u8 = 17;

// Die drei Unterklassen setzen alle bei 23 auf. Welches Feld dort steht, haengt am
// Entity-Typ -- die drei sind aber an ihrem SERIALIZER unterscheidbar (Text vs. ItemStack
// vs. BlockState), deshalb kommt dieser Parser ohne den Typ aus und der Aufrufer entscheidet
// anhand von `world::entity_type_name`, was er davon glaubt.
//   TextDisplayEntity:  TEXT(23), LINE_WIDTH(24), BACKGROUND(25), TEXT_OPACITY(26), FLAGS(27)
//   ItemDisplayEntity:  ITEM(23), ITEM_DISPLAY(24)
//   BlockDisplayEntity: BLOCK_STATE(23)
pub(crate) const DISPLAY_INDEX_CONTENT: u8 = 23;
pub(crate) const TNT_INDEX_BLOCK_STATE: u8 = 9;
// ItemDisplayEntity.ITEM_DISPLAY: der ItemDisplayContext als Byte (0 NONE, 1/2 dritte Person
// links/rechts, 3/4 Ego links/rechts, 5 HEAD, 6 GUI, 7 GROUND, 8 FIXED, 9 ON_SHELF). An
// Index 24 steht beim Text-Display die Zeilenbreite -- ein INT, also unverwechselbar.
pub(crate) const ITEM_DISPLAY_INDEX_MODE: u8 = 24;
// Der getragene Stack an Index 8: ItemEntity.STACK, ThrownItemEntity.ITEM (Ei, Schneeball,
// Enderperle, Traenke, Erfahrungsflasche), EyeOfEnderEntity.ITEM, AbstractFireballEntity.ITEM
// und FireworkRocketEntity.ITEM.
pub(crate) const ENTITY_INDEX_ITEM: u8 = 8;
// Gegenstandsrahmen: HangingEntity belegt 8 mit DATA_DIRECTION (seit 1.21.2), ItemFrame setzt
// mit DATA_ITEM (9) und DATA_ROTATION (10) dahinter auf. Frueher hier als 8 und 9 gelesen --
// dann kam bei keinem Rahmen je ein Inhalt an.
pub(crate) const HANGING_INDEX_DIRECTION: u8 = 8;
pub(crate) const ITEM_FRAME_INDEX_ITEM: u8 = 9;
pub(crate) const ITEM_FRAME_INDEX_ROTATION: u8 = 10;
pub(crate) const TEXT_DISPLAY_INDEX_LINE_WIDTH: u8 = 24;
pub(crate) const TEXT_DISPLAY_INDEX_BACKGROUND: u8 = 25;
pub(crate) const TEXT_DISPLAY_INDEX_TEXT_OPACITY: u8 = 26;
pub(crate) const TEXT_DISPLAY_INDEX_FLAGS: u8 = 27;

// TextDisplayEntity-Flags (dieselbe Klasse): Schatten, Durchsehbarkeit, Standard-Hintergrund
// und die beiden Ausrichtungs-Bits.
pub const TEXT_DISPLAY_FLAG_SHADOW: u8 = 1;
pub const TEXT_DISPLAY_FLAG_SEE_THROUGH: u8 = 2;
pub const TEXT_DISPLAY_FLAG_DEFAULT_BACKGROUND: u8 = 4;
pub const TEXT_DISPLAY_FLAG_ALIGN_LEFT: u8 = 8;
pub const TEXT_DISPLAY_FLAG_ALIGN_RIGHT: u8 = 16;
// TextDisplayEntity.INITIAL_BACKGROUND = 0x40000000: 25 % Schwarz, dasselbe wie hinter einem
// gewoehnlichen Namensschild.
pub const TEXT_DISPLAY_INITIAL_BACKGROUND: i32 = 0x4000_0000;

// Serializer-IDs = Registrierungsreihenfolge in TrackedDataHandlerRegistry.class:
// 0 BYTE, 1 INTEGER, 2 LONG, 3 FLOAT, 4 STRING, 5 TEXT_COMPONENT, 6 OPTIONAL_TEXT_COMPONENT,
// 7 ITEM_STACK, 8 BOOLEAN, ... 20 ENTITY_POSE.
pub(crate) const DATA_TYPE_BYTE: i32 = 0;
pub(crate) const DATA_TYPE_INT: i32 = 1;
pub(crate) const DATA_TYPE_LONG: i32 = 2;
pub(crate) const DATA_TYPE_FLOAT: i32 = 3;
pub(crate) const DATA_TYPE_STRING: i32 = 4;
pub(crate) const DATA_TYPE_TEXT: i32 = 5;
pub(crate) const DATA_TYPE_OPTIONAL_TEXT: i32 = 6;
pub(crate) const DATA_TYPE_ITEM_STACK: i32 = 7;
pub(crate) const DATA_TYPE_BOOLEAN: i32 = 8;
// EndCrystalEntity: Entity belegt 0..7, danach BEAM_TARGET (8) und SHOW_BOTTOM (9) in der
// Reihenfolge der registerData-Aufrufe. Ein von Spielern gesetzter Kristall hat KEINEN
// Boden (EndCrystalItem: setShowBottom(false)).
pub(crate) const END_CRYSTAL_INDEX_SHOW_BOTTOM: u8 = 9;
// AbstractArrow: ID_FLAGS (8), PIERCE_LEVEL (9), IN_GROUND (10, Boolean) -- steckt der Pfeil?
pub(crate) const ARROW_INDEX_IN_GROUND: u8 = 10;
// Interaction: WIDTH (8) und HEIGHT (9) als Float, RESPONSE (10) -- die unsichtbare Klickflaeche,
// mit der Server Display-Entities anklickbar machen.
pub(crate) const INTERACTION_INDEX_WIDTH: u8 = 8;
pub(crate) const INTERACTION_INDEX_HEIGHT: u8 = 9;
// 9 ROTATION, 10 BLOCK_POS, 11 OPTIONAL_BLOCK_POS -- gegengeprueft daran, dass mit derselben
// Zaehlung ENTITY_POSE auf 20 faellt, was schon verifiziert war.
pub(crate) const DATA_TYPE_ROTATION: i32 = 9;
pub(crate) const DATA_TYPE_BLOCK_POS: i32 = 10;
pub(crate) const DATA_TYPE_OPTIONAL_BLOCK_POS: i32 = 11;
pub(crate) const DATA_TYPE_FACING: i32 = 12;
pub(crate) const DATA_TYPE_BLOCK_STATE: i32 = 14;
pub(crate) const DATA_TYPE_OPTIONAL_BLOCK_STATE: i32 = 15;
pub(crate) const DATA_TYPE_OPTIONAL_INT: i32 = 19;
pub(crate) const DATA_TYPE_ENTITY_POSE: i32 = 20;
// TamableAnimal.DATA_OWNERUUID_ID: optional(UUID) -- ein Bool, dann 16 Bytes. Steht vor der
// Variante einer Katze; ohne ihn brach der Strom dort ab.
pub(crate) const DATA_TYPE_OPTIONAL_LIVING_ENTITY_REFERENCE: i32 = 13;
// Die Tier-Varianten (Katze, Kuh, Wolf, Wolfsklang, Frosch, Schwein, Huhn): alle
// ByteBufCodecs.holderRegistry, also eine VarInt-Registry-ID.
pub(crate) const DATA_TYPE_CAT_VARIANT: i32 = 21;
pub(crate) const DATA_TYPE_CHICKEN_VARIANT: i32 = 27;
// Die uebrigen Serializer bis 34, in Registry-Reihenfolge (TrackedDataHandlerRegistry, per
// Bootstrap des 1.21.11-Clients ausgelesen): Dorfbewohner-Daten, Zombie-Nautilus-Variante,
// optionale GlobalPos, Gemaelde-Variante und vier Zustands-Enums.
pub(crate) const DATA_TYPE_VILLAGER_DATA: i32 = 18;
pub(crate) const DATA_TYPE_ZOMBIE_NAUTILUS_VARIANT: i32 = 28;
pub(crate) const DATA_TYPE_OPTIONAL_GLOBAL_POS: i32 = 29;
pub(crate) const DATA_TYPE_PAINTING_VARIANT: i32 = 30;
pub(crate) const DATA_TYPE_SNIFFER_STATE: i32 = 31;
pub(crate) const DATA_TYPE_OXIDATION_LEVEL: i32 = 34;
// TamableAnimal.DATA_FLAGS_ID (Bit 1 = sitzt). AgeableMob belegt 16, Animal nichts.
pub(crate) const TAMABLE_INDEX_FLAGS: u8 = 17;
pub const TAMABLE_FLAG_SITTING: u8 = 1;
// ParticleTypes.PACKET_CODEC bzw. dessen Liste (VarInt-Anzahl, dann die Partikel). Die Liste
// ist LivingEntity.POTION_SWIRLS (Index 10) -- sie steht VOR der Absorption (17) und kommt
// beim Goldapfel im selben Paket mit. Solange sie unlesbar war, brach der Strom dort ab und
// die goldenen Herzen kamen nie an.
pub(crate) const DATA_TYPE_PARTICLE: i32 = 16;
// AreaEffectCloud (Trankwolke am Boden): Entity belegt 0..7, danach RADIUS (8, Float),
// WAITING (9, Boolean) und PARTICLE (10, ein einzelner Partikel).
pub(crate) const CLOUD_INDEX_RADIUS: u8 = 8;
pub(crate) const CLOUD_INDEX_WAITING: u8 = 9;
// AbstractBoat: VehicleEntity belegt 8..10 (Wackeln, Richtung, Schaden), danach
// DATA_ID_PADDLE_LEFT (11) und DATA_ID_PADDLE_RIGHT (12), beide Boolean -- welches Ruder sich
// gerade bewegt.
pub(crate) const BOAT_INDEX_PADDLE_LEFT: u8 = 11;
pub(crate) const BOAT_INDEX_PADDLE_RIGHT: u8 = 12;
// AbstractMinecart: ebenfalls nach VehicleEntity DATA_ID_CUSTOM_DISPLAY_BLOCK (11, optionaler
// BlockState) und DATA_ID_DISPLAY_OFFSET (12, Int); dahinter MinecartFurnace.DATA_ID_FUEL (13,
// Boolean) -- ob der Ofen brennt.
pub(crate) const MINECART_INDEX_DISPLAY_BLOCK: u8 = 11;
pub(crate) const MINECART_INDEX_DISPLAY_OFFSET: u8 = 12;
pub(crate) const FURNACE_MINECART_INDEX_FUEL: u8 = 13;
pub(crate) const CLOUD_INDEX_PARTICLE: u8 = 10;
pub(crate) const DATA_TYPE_PARTICLE_LIST: i32 = 17;
// PlayerLikeEntity.MAIN_ARM (Index 15), PacketCodecs.indexed = VarInt: 0 links, 1 rechts.
pub(crate) const DATA_TYPE_ARM: i32 = 38;
pub(crate) const PLAYER_INDEX_MAIN_ARM: u8 = 15;
// VECTOR_3F (3 Floats) und QUATERNION_F (4 Floats) stehen ganz hinten in derselben Liste.
// Sie sind fuer Display-Entities NOTWENDIG, auch wenn uns ihr Inhalt teils gar nicht
// interessiert: TRANSLATION/SCALE/LEFT_ROTATION/RIGHT_ROTATION liegen VOR dem eigentlichen
// Inhalt (Index 23). Ohne ihre Laenge bricht der Parser vorher ab und der Text kommt nie an.
pub(crate) const DATA_TYPE_VECTOR_3F: i32 = 35;
pub(crate) const DATA_TYPE_QUATERNION_F: i32 = 36;

// Entity.FLAGS, Bit 1 (0x02) -- isSneaking(). NICHT dasselbe wie die Pose: gedrueckte
// Schleichtaste setzt das Flag sofort, die Pose CROUCHING kommt nur, wenn der Spieler auch
// wirklich in die geduckte Box wechselt. Fuer die Hitbox zaehlt die POSE, das Flag ist nur
// fuer die Darstellung interessant.
pub const ENTITY_FLAG_SNEAKING: u8 = 0x02;
// Entity.FLAGS Bit 3: sprintet.
pub const ENTITY_FLAG_SPRINTING: u8 = 0x08;
// Entity.FLAGS, Bit 0 (0x01) -- brennt (Entity.isOnFire auf dem Client).
pub const ENTITY_FLAG_ON_FIRE: u8 = 0x01;
// Entity.FLAGS, Bit 5 (0x20) -- isInvisible(). Ein unsichtbarer Armorstand ist der Traeger
// fast jedes Hologramms: sein Modell darf nicht gezeichnet werden, sein Namensschild schon.
pub const ENTITY_FLAG_INVISIBLE: u8 = 0x20;
// Entity.FLAGS, Bit 6 (0x40) -- leuchtet (Effekt Leuchten, Spektralpfeil, /effect, Teams mit
// Plugins): der Client zeichnet einen Umriss in Teamfarbe, sichtbar durch Waende.
pub const ENTITY_FLAG_GLOWING: u8 = 0x40;
// Entity.FLAGS, Bit 7 (0x80) -- LivingEntity.isGliding(): Elytra-Flug.
pub const ENTITY_FLAG_GLIDING: u8 = 0x80;
// FireworkRocketEntity: Entity belegt 0..7, danach ITEM (8), SHOOTER_ENTITY_ID (9, OptionalInt)
// und SHOT_AT_ANGLE (10). SHOOTER_ENTITY_ID ist nur gesetzt, wenn die Rakete an einem
// Schuetzen HAENGT (Elytra-Schub), nicht bei einer frei abgeschossenen.
pub(crate) const FIREWORK_INDEX_SHOOTER: u8 = 9;

// LivingEntity.LIVING_FLAGS -- das ERSTE von LivingEntity registrierte Feld, also Index 8
// (Entity belegt 0..7). Gegengeprueft an der schon verifizierten SLEEPING_POSITION auf 14:
// 8 LIVING_FLAGS, 9 HEALTH, 10 POTION_SWIRLS, 11 POTION_SWIRLS_AMBIENT, 12 STUCK_ARROW_COUNT,
// 13 STINGER_COUNT, 14 SLEEPING_POSITION -- die Zaehlung geht exakt auf.
//
// Daran haengt, ob jemand gerade ein Item BENUTZT: das Schild oben, den Bogen gespannt, am
// Essen. Fuer einen PvP-Client ist das keine Feinheit -- ob der Gegner blockt, entscheidet,
// ob man zuschlaegt oder wartet.
pub(crate) const ENTITY_DATA_INDEX_LIVING_FLAGS: u8 = 8;
// LivingEntity.USING_ITEM_FLAG / OFF_HAND_ACTIVE_FLAG.
pub const LIVING_FLAG_USING_ITEM: u8 = 0x01;
pub const LIVING_FLAG_OFFHAND_ACTIVE: u8 = 0x02;
// LivingEntity.isUsingRiptide(): Sog-Angriff laeuft (setzt nur der Server, s. physics).
pub const LIVING_FLAG_USING_RIPTIDE: u8 = 0x04;

// AbstractArrow.ID_FLAGS (Index 8, Byte): Bit 0x01 = kritisch (voll gespannt), 0x02 = ohne
// Kollision (Loyalitaets-Dreizack auf dem Rueckweg).
pub const ARROW_FLAG_CRIT: u8 = 0x01;

// Was uns aus dem Metadaten-Strom interessiert. Alles andere wird uebersprungen; None heisst
// "stand nicht in diesem Paket", nicht "auf Standard zurueckgesetzt".
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct EntityDataUpdate {
    pub entity_id: i32,
    // Spieler (und Puppen): Haupthand links (Vanillas Option "Haupthand")? Dann haelt der
    // linke Arm das Haupthand-Item.
    pub main_arm_left: Option<bool>,
    // Boolean an Index 16 (Baby bei Tieren, Dorfbewohnern, Zombies; s. AGEABLE_INDEX_BABY).
    pub baby: Option<bool>,
    // VillagerData (Dorfbewohner, Zombie-Dorfbewohner): (Biom-Typ, Beruf, Stufe) als
    // Registry-IDs bzw. 1..5.
    pub villager_data: Option<(i32, i32, i32)>,
    pub pose: Option<u8>,
    pub flags: Option<u8>,
    // Restluft in Ticks (s. ENTITY_DATA_INDEX_AIR).
    pub air: Option<i32>,
    // Frost-Ticks (s. ENTITY_DATA_INDEX_FROZEN_TICKS).
    pub frozen_ticks: Option<i32>,
    // Doppeltes Option mit Absicht: das AEUSSERE None heisst "stand nicht im Paket", das
    // innere None heisst "der Server hat das Feld geleert" -- also aufgewacht.
    pub sleeping_pos: Option<Option<(i32, i32, i32)>>,
    // Entity.CUSTOM_NAME, ebenfalls doppelt: aussen "stand nicht im Paket", innen "Name
    // geloescht". Die Abschnitte tragen die Farben mit (s. nbt::text_to_spans).
    pub custom_name: Option<Option<Vec<crate::nbt::TextSpan>>>,
    pub name_visible: Option<bool>,
    // LivingEntity.LIVING_FLAGS, s. LIVING_FLAG_USING_ITEM.
    pub living_flags: Option<u8>,
    // Absorptionspunkte eines Spielers (s. PLAYER_INDEX_ABSORPTION).
    pub absorption: Option<f32>,
    // Dasselbe Byte wie `display_billboard` (s. ARMOR_STAND_INDEX_FLAGS).
    pub armor_stand_flags: Option<u8>,
    // Gelenkwinkel eines Armorstands (s. ARMOR_STAND_INDEX_HEAD_POSE), je Gelenk nur, wenn er im
    // Paket stand.
    pub armor_stand_poses: [Option<[f32; 3]>; 6],
    // --- DisplayEntity ---------------------------------------------------------------
    pub display_translation: Option<[f32; 3]>,
    pub display_scale: Option<[f32; 3]>,
    // Quaternionen (x, y, z, w) der AffineTransformation -- Reihenfolge wie
    // PacketByteBuf.writeQuaternionf.
    pub display_left_rotation: Option<[f32; 4]>,
    // EndCrystalEntity.SHOW_BOTTOM (Index 9, Boolean) -- nur bei einem End-Kristall gemeint.
    pub end_crystal_show_bottom: Option<bool>,
    // AbstractArrow.IN_GROUND (Index 10, Boolean) -- nur bei Pfeil, Spektralpfeil und
    // Dreizack gemeint.
    pub arrow_in_ground: Option<bool>,
    // Interaction.WIDTH/HEIGHT (Index 8/9, Float) -- nur bei einem interaction-Entity gemeint
    // (Index 9 ist bei Lebewesen die Gesundheit).
    pub interaction_width: Option<f32>,
    pub interaction_height: Option<f32>,
    // LivingEntity.POTION_SWIRLS (Index 10): die Farben (ARGB) der entity_effect-Wirbel, die
    // das Entity wegen seiner Trankeffekte verstreut. Leer = keine. Andere Partikelarten in
    // der Liste werden hier weggelassen -- Vanilla legt dort nur entity_effect ab.
    pub potion_swirls: Option<Vec<u32>>,
    // LivingEntity.POTION_SWIRLS_AMBIENT (Index 11): nur Umgebungs-Effekte (Leuchtfeuer) --
    // dann streut das Entity fuenfmal seltener.
    pub potion_swirls_ambient: Option<bool>,
    pub display_right_rotation: Option<[f32; 4]>,
    pub display_billboard: Option<u8>,
    // Interpolation einer Transformations-Aenderung: Verzoegerung und Dauer in Ticks.
    pub display_interp_start: Option<i32>,
    // ExperienceOrb.DATA_VALUE (ebenfalls Index 8, Int) -- nur bei einer XP-Kugel gemeint.
    // Bei gezuendetem TNT steht hier die Zuendzeit (PrimedTnt.DATA_FUSE_ID).
    pub orb_value: Option<i32>,
    pub display_interp_duration: Option<i32>,
    // Ueber wie viele Ticks ein Display einer neuen Position folgt (teleport_duration).
    pub display_teleport_duration: Option<i32>,
    pub item_display_mode: Option<u8>,
    pub display_view_range: Option<f32>,
    // Der Inhalt eines Display-Entities (Index 23), unterschieden am Serializer.
    pub display_text: Option<Vec<crate::nbt::TextSpan>>,
    pub display_item: Option<Option<ItemStack>>,
    // Der Stack an Index 8 (s. ENTITY_INDEX_ITEM): aussen "stand nicht im Paket", innen leer.
    pub item_stack: Option<Option<ItemStack>>,
    // Gegenstandsrahmen: Inhalt (Index 9, wie item_stack doppelt), Drehung des Inhalts (Index 10,
    // 0..7) und die Wand, an der er haengt (Index 8, Direction-Ordinal wie im Spawn-Paket).
    pub frame_item: Option<Option<ItemStack>>,
    pub frame_rotation: Option<i32>,
    pub hanging_direction: Option<u8>,
    pub display_block_state: Option<u32>,
    // PrimedTnt.DATA_BLOCK_STATE_ID (Index 9): welcher Block da brennt (Vorgabe: TNT).
    pub tnt_block_state: Option<u32>,
    // FireworkRocketEntity.SHOOTER_ENTITY_ID, doppelt wie custom_name: aussen "stand nicht im
    // Paket", innen "kein Schuetze". Welcher Typ gemeint ist, weiss erst der Empfaenger.
    pub firework_shooter: Option<Option<i32>>,
    // Nur bei text_display belegt.
    pub text_line_width: Option<i32>,
    pub text_background: Option<i32>,
    pub text_opacity: Option<u8>,
    pub text_flags: Option<u8>,
    // TamableAnimal-Flags (Index 17, Byte) -- nur bei zaehmbaren Tieren gemeint.
    pub tamable_flags: Option<u8>,
    // Registry-ID der Katzenvariante (Serializer CAT_VARIANT).
    pub cat_variant: Option<i32>,
    // Nur bei einer Trankwolke gemeint (s. CLOUD_INDEX_RADIUS).
    pub cloud_radius: Option<f32>,
    pub cloud_waiting: Option<bool>,
    pub cloud_particle: Option<crate::particle::ParticleEffect>,
    // Nur bei Booten und Floessen gemeint: linkes, rechtes Ruder in Bewegung (s.
    // BOAT_INDEX_PADDLE_LEFT).
    pub boat_paddles: [Option<bool>; 2],
    // Nur bei Loren gemeint (s. MINECART_INDEX_DISPLAY_BLOCK): der eigene Anzeigeblock, doppelt
    // wie custom_name (aussen "stand nicht im Paket", innen "keiner, die Vorgabe der Lore"),
    // sein Versatz in Pixeln und bei der Ofenlore, ob sie brennt.
    pub minecart_block: Option<Option<u32>>,
    pub minecart_offset: Option<i32>,
    pub minecart_fuel: Option<bool>,
}

// Sicherheitsdeckel gegen einen Strom ohne Endmarke.
pub(crate) const MAX_ENTITY_DATA_FIELDS: usize = 64;

// Liest den Metadaten-Strom, bis die Endmarke kommt ODER ein Serializer auftaucht, dessen
// Laenge wir nicht kennen. Der zweite Fall ist bewusst KEIN Fehler: ab da wissen wir nicht
// mehr, wo das naechste Feld anfaengt, also hoeren wir auf und liefern, was bis dahin
// dastand. Die uns wichtigen Felder (0 und 6) stehen am Anfang und benutzen alle
// Serializer, die wir kennen -- ein unbekannter Typ dahinter kostet uns nichts.
pub fn read_set_entity_data(payload: &[u8]) -> Result<EntityDataUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let mut out = EntityDataUpdate { entity_id: r.read_varint()?, ..Default::default() };
    for _ in 0..MAX_ENTITY_DATA_FIELDS {
        let index = r.read_u8()?;
        if index == 0xFF {
            break;
        }
        let data_type = r.read_varint()?;
        match data_type {
            DATA_TYPE_BYTE => {
                let v = r.read_u8()?;
                match index {
                    ENTITY_DATA_INDEX_FLAGS => out.flags = Some(v),
                    ENTITY_DATA_INDEX_LIVING_FLAGS => out.living_flags = Some(v),
                    // Index 15 ist doppelt belegt: bei einem DisplayEntity der
                    // Billboard-Modus, bei einem ArmorStandEntity dessen Flags. Beide sind
                    // Bytes und im Strom nicht zu unterscheiden -- welches Feld gemeint ist,
                    // entscheidet erst der Entity-TYP, und den kennt dieser Parser nicht
                    // (s. Entities::apply_metadata, das genau eines der beiden uebernimmt).
                    DISPLAY_INDEX_BILLBOARD => {
                        out.display_billboard = Some(v);
                        out.armor_stand_flags = Some(v);
                    }
                    TEXT_DISPLAY_INDEX_TEXT_OPACITY => out.text_opacity = Some(v),
                    TEXT_DISPLAY_INDEX_FLAGS => out.text_flags = Some(v),
                    ITEM_DISPLAY_INDEX_MODE => out.item_display_mode = Some(v),
                    _ => {}
                }
                if index == TAMABLE_INDEX_FLAGS {
                    out.tamable_flags = Some(v);
                }
            }
            DATA_TYPE_ENTITY_POSE => {
                let v = r.read_varint()?;
                if index == ENTITY_DATA_INDEX_POSE {
                    // OutOfBoundsHandling.ZERO: ein unbekannter Index ist in Vanilla
                    // STANDING, kein Fehler.
                    out.pose = Some(u8::try_from(v).unwrap_or(0));
                }
            }
            DATA_TYPE_INT => {
                let v = r.read_varint()?;
                match index {
                    ENTITY_DATA_INDEX_AIR => out.air = Some(v),
                    ENTITY_DATA_INDEX_FROZEN_TICKS => out.frozen_ticks = Some(v),
                    // Bei anderen Typen stehen hier andere Ints (Boot: Wackel-Ticks) -- der
                    // Empfaenger nimmt sie nur bei einem Display.
                    DISPLAY_INDEX_START_INTERPOLATION => {
                        out.display_interp_start = Some(v);
                        out.orb_value = Some(v);
                    }
                    DISPLAY_INDEX_INTERPOLATION_DURATION => out.display_interp_duration = Some(v),
                    // Beim Bilderrahmen steht an derselben Stelle seine Drehung.
                    DISPLAY_INDEX_TELEPORT_DURATION => {
                        out.display_teleport_duration = Some(v);
                        out.frame_rotation = Some(v);
                    }
                    TEXT_DISPLAY_INDEX_LINE_WIDTH => out.text_line_width = Some(v),
                    TEXT_DISPLAY_INDEX_BACKGROUND => out.text_background = Some(v),
                    MINECART_INDEX_DISPLAY_OFFSET => out.minecart_offset = Some(v),
                    _ => {}
                }
            }
            DATA_TYPE_LONG => {
                r.read_varlong()?;
            }
            DATA_TYPE_FLOAT => {
                let v = r.read_f32()?;
                if index == DISPLAY_INDEX_VIEW_RANGE {
                    out.display_view_range = Some(v);
                }
                if index == PLAYER_INDEX_ABSORPTION {
                    out.absorption = Some(v);
                }
                if index == INTERACTION_INDEX_WIDTH {
                    out.interaction_width = Some(v);
                }
                if index == INTERACTION_INDEX_HEIGHT {
                    out.interaction_height = Some(v);
                }
                if index == CLOUD_INDEX_RADIUS {
                    out.cloud_radius = Some(v);
                }
            }
            DATA_TYPE_BOOLEAN => {
                let v = r.read_bool()?;
                if index == ENTITY_DATA_INDEX_NAME_VISIBLE {
                    out.name_visible = Some(v);
                }
                // Welcher Typ das ist, weiss erst der Empfaenger -- s. world::Entity.
                if index == END_CRYSTAL_INDEX_SHOW_BOTTOM {
                    out.end_crystal_show_bottom = Some(v);
                }
                if index == ARROW_INDEX_IN_GROUND {
                    out.arrow_in_ground = Some(v);
                }
                if index == ENTITY_DATA_INDEX_POTION_SWIRLS_AMBIENT {
                    out.potion_swirls_ambient = Some(v);
                }
                if index == AGEABLE_INDEX_BABY {
                    out.baby = Some(v);
                }
                if index == CLOUD_INDEX_WAITING {
                    out.cloud_waiting = Some(v);
                }
                if index == BOAT_INDEX_PADDLE_LEFT {
                    out.boat_paddles[0] = Some(v);
                }
                if index == BOAT_INDEX_PADDLE_RIGHT {
                    out.boat_paddles[1] = Some(v);
                }
                if index == FURNACE_MINECART_INDEX_FUEL {
                    out.minecart_fuel = Some(v);
                }
            }
            DATA_TYPE_VECTOR_3F => {
                let v = [r.read_f32()?, r.read_f32()?, r.read_f32()?];
                match index {
                    DISPLAY_INDEX_TRANSLATION => out.display_translation = Some(v),
                    DISPLAY_INDEX_SCALE => out.display_scale = Some(v),
                    _ => {}
                }
            }
            // Quaternion: vier Floats, x y z w (PacketByteBuf.writeQuaternionf).
            DATA_TYPE_QUATERNION_F => {
                let v = [r.read_f32()?, r.read_f32()?, r.read_f32()?, r.read_f32()?];
                match index {
                    DISPLAY_INDEX_LEFT_ROTATION => out.display_left_rotation = Some(v),
                    DISPLAY_INDEX_RIGHT_ROTATION => out.display_right_rotation = Some(v),
                    _ => {}
                }
            }
            DATA_TYPE_BLOCK_STATE => {
                let v = r.read_varint()?;
                if index == DISPLAY_INDEX_CONTENT {
                    out.display_block_state = Some(v.max(0) as u32);
                }
                if index == TNT_INDEX_BLOCK_STATE {
                    out.tnt_block_state = Some(v.max(0) as u32);
                }
            }
            // PacketCodecs.optional(BLOCK_STATE): 0 = leer, sonst StateId + 1.
            DATA_TYPE_OPTIONAL_BLOCK_STATE => {
                let v = r.read_varint()?;
                if index == MINECART_INDEX_DISPLAY_BLOCK {
                    out.minecart_block = Some((v > 0).then(|| (v - 1) as u32));
                }
            }
            // PacketCodecs.OPTIONAL_INT: 0 = leer, sonst Wert + 1.
            DATA_TYPE_OPTIONAL_INT => {
                let v = r.read_varint()?;
                if index == FIREWORK_INDEX_SHOOTER {
                    out.firework_shooter = Some((v != 0).then(|| v - 1));
                }
            }
            DATA_TYPE_FACING => {
                let v = r.read_varint()?;
                if index == HANGING_INDEX_DIRECTION {
                    out.hanging_direction = u8::try_from(v).ok();
                }
            }
            DATA_TYPE_ARM => {
                let v = r.read_varint()?;
                if index == PLAYER_INDEX_MAIN_ARM {
                    out.main_arm_left = Some(v == 0);
                }
            }
            DATA_TYPE_OPTIONAL_LIVING_ENTITY_REFERENCE => {
                if r.read_bool()? {
                    r.read_i64()?;
                    r.read_i64()?;
                }
            }
            // VarInt Biom-Typ, VarInt Beruf (beides Registry-IDs), VarInt Stufe.
            DATA_TYPE_VILLAGER_DATA => {
                let v = (r.read_varint()?, r.read_varint()?, r.read_varint()?);
                out.villager_data = Some(v);
            }
            // Registry-ID bzw. Enum-Index, je ein VarInt.
            DATA_TYPE_ZOMBIE_NAUTILUS_VARIANT | DATA_TYPE_SNIFFER_STATE..=DATA_TYPE_OXIDATION_LEVEL => {
                r.read_varint()?;
            }
            // Bool, dann Dimension (Identifier als String) und Blockposition (Long).
            DATA_TYPE_OPTIONAL_GLOBAL_POS => {
                if r.read_bool()? {
                    r.read_string()?;
                    r.read_i64()?;
                }
            }
            // RegistryEntry: VarInt id + 1, 0 hiesse "direkt mitgeschickt" -- dessen Inhalt
            // laesst sich nicht ueberspringen, also dann abbrechen.
            DATA_TYPE_PAINTING_VARIANT => {
                if r.read_varint()? == 0 {
                    break;
                }
            }
            DATA_TYPE_CAT_VARIANT..=DATA_TYPE_CHICKEN_VARIANT => {
                let v = r.read_varint()?;
                if data_type == DATA_TYPE_CAT_VARIANT {
                    out.cat_variant = Some(v);
                }
            }
            // Ein unlesbarer Partikel (neuere Serverversion) macht die Position unbrauchbar --
            // dann abbrechen wie bei einem unbekannten Serializer.
            DATA_TYPE_PARTICLE => {
                let Ok(p) = crate::particle::read_particle(&mut r) else { break };
                if index == CLOUD_INDEX_PARTICLE {
                    out.cloud_particle = Some(p);
                }
            }
            DATA_TYPE_PARTICLE_LIST => {
                let n = r.read_varint()?;
                if !(0..=256).contains(&n) {
                    break;
                }
                let mut farben = Vec::new();
                let mut kaputt = false;
                for _ in 0..n {
                    match crate::particle::read_particle(&mut r) {
                        Ok(crate::particle::ParticleEffect { type_id: crate::particle::PARTICLE_ENTITY_EFFECT, options: crate::particle::ParticleOptions::Tinted(argb) }) => farben.push(argb),
                        Ok(_) => {}
                        Err(_) => kaputt = true,
                    }
                    if kaputt {
                        break;
                    }
                }
                if kaputt {
                    break;
                }
                if index == ENTITY_DATA_INDEX_POTION_SWIRLS {
                    out.potion_swirls = Some(farben);
                }
            }
            DATA_TYPE_ITEM_STACK => {
                // Ein Component, den dieser Client nicht kennt, macht die Leseposition
                // unbrauchbar -- dann bricht der Strom hier ab wie bei einem unbekannten
                // Serializer, statt danach Muell zu lesen.
                let Ok(stack) = crate::item::read_optional_slot(&mut r) else { break };
                if index == DISPLAY_INDEX_CONTENT {
                    out.display_item = Some(stack);
                } else if index == ENTITY_INDEX_ITEM {
                    out.item_stack = Some(stack);
                } else if index == ITEM_FRAME_INDEX_ITEM {
                    out.frame_item = Some(stack);
                }
            }
            DATA_TYPE_OPTIONAL_BLOCK_POS => {
                let vorhanden = r.read_bool()?;
                let pos = if vorhanden { Some(decode_block_pos(r.read_i64()?)) } else { None };
                if index == ENTITY_DATA_INDEX_SLEEPING_POS {
                    out.sleeping_pos = Some(pos);
                }
            }
            DATA_TYPE_BLOCK_POS => {
                r.read_i64()?;
            }
            DATA_TYPE_ROTATION => {
                let v = [r.read_f32()?, r.read_f32()?, r.read_f32()?];
                if let Some(i) = index.checked_sub(ARMOR_STAND_INDEX_HEAD_POSE).filter(|&i| i < ARMOR_STAND_POSE_COUNT) {
                    out.armor_stand_poses[i as usize] = Some(v);
                }
            }
            DATA_TYPE_STRING => {
                r.read_string()?;
            }
            DATA_TYPE_TEXT => {
                let value = crate::nbt::read_root(&mut r)?;
                if index == DISPLAY_INDEX_CONTENT {
                    out.display_text = Some(value.as_ref().map(crate::nbt::text_to_spans).unwrap_or_default());
                }
            }
            DATA_TYPE_OPTIONAL_TEXT => {
                let value = if r.read_bool()? { crate::nbt::read_root(&mut r)? } else { None };
                if index == ENTITY_DATA_INDEX_CUSTOM_NAME {
                    out.custom_name = Some(value.as_ref().map(crate::nbt::text_to_spans));
                }
            }
            // Unbekannte Laenge -- ab hier ist der Strom fuer uns nicht mehr zerlegbar.
            _ => break,
        }
    }
    Ok(out)
}

// Serverbound Client Command mit Aktion PERFORM_RESPAWN (0) -- nach dem Tod abschicken,
// damit der Server den Spieler neu setzt (Position + Chunks folgen als eigene Pakete).
#[must_use] 
pub fn perform_respawn_payload() -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_CLIENT_COMMAND).write_varint(0);
    w.into_bytes()
}

// Remove Entities (0x4B): Anzahl (VarInt), dann EntityIds (je VarInt) -- readIntList.
// EntityPassengersSetS2CPacket: VarInt Fahrzeug-Id, dann ein VarInt-Array der Passagier-Ids
// (buf.readIntArray = Laenge als VarInt, danach je ein VarInt). ID aus PlayStateFactories.
//
// Frueherer Fehler: beim Spawn eines Spielers kamen 2 Nametags, einer blieb stehen.
// Namensschild-Plugins setzen dem Spieler ein text_display auf den Kopf. Ein Passagier
// bekommt vom Server KEINE eigenen Bewegungspakete -- er wird vom Fahrzeug mitgezogen. Wer
// dieses Paket nicht auswertet, laesst das Display fuer immer an seinem Spawnpunkt stehen,
// waehrend der Spieler weggeht: genau das "statische, in der Luft schwebende" Schild.
pub const CB_PLAY_SET_PASSENGERS: i32 = 0x69;
