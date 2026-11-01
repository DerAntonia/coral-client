// Teleport zu Entity, Kreativ-Slot, Item auswaehlen, Kamera, Faehigkeiten und Attribute.

use super::*;

// ServerboundTeleportToEntityPacket (Index 61, direkt nach SWING 0x3C): Zuschauer springen zu
// einem Spieler (Zuschauer-Menue). Nur die UUID.
pub const SB_PLAY_TELEPORT_TO_ENTITY: i32 = 0x3D;

#[must_use] 
pub fn teleport_to_entity_payload(uuid: [u8; 16]) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_TELEPORT_TO_ENTITY).write_raw_bytes(&uuid);
    w.into_bytes()
}

// ServerboundSetCreativeModeSlotPacket (Index 55, zwischen SET_COMMAND_MINECART und
// SET_JIGSAW_BLOCK; Nachbar der verifizierten SET_CARRIED_ITEM 0x34 und SWING 0x3C): Slot als
// short im Spielerinventar (ScreenHandler 0, -1 = aus dem Inventar werfen) und der Stapel in
// der Kreativ-Form (s. item::write_untrusted_slot). Nur im Kreativmodus; der Server
// uebernimmt den Stapel ohne Rueckmeldung.
pub const SB_PLAY_SET_CREATIVE_MODE_SLOT: i32 = 0x37;

#[must_use] 
pub fn set_creative_mode_slot_payload(slot: i16, stack: Option<&crate::item::ItemStack>) -> Option<Vec<u8>> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_SET_CREATIVE_MODE_SLOT).write_u16(slot as u16);
    crate::item::write_untrusted_slot(&mut w, stack).ok()?;
    Some(w.into_bytes())
}

// ServerboundPickItemFromBlockPacket / ...FromEntityPacket (Index 35/36, direkt nach
// PADDLE_BOAT 0x22): der Mittelklick. Seit 1.21.4 sucht der SERVER das passende Item (im
// Kreativmodus legt er es neu an), der Client nennt nur den Block bzw. das Entity und ob
// die Daten mitkommen sollen (Kreativ mit Strg).
pub const SB_PLAY_PICK_ITEM_FROM_BLOCK: i32 = 0x23;
pub const SB_PLAY_PICK_ITEM_FROM_ENTITY: i32 = 0x24;

#[must_use] 
pub fn pick_item_from_block_payload(pos: (i32, i32, i32), include_data: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PICK_ITEM_FROM_BLOCK).write_i64(encode_block_pos(pos.0, pos.1, pos.2)).write_bool(include_data);
    w.into_bytes()
}

#[must_use] 
pub fn pick_item_from_entity_payload(entity_id: i32, include_data: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PICK_ITEM_FROM_ENTITY).write_varint(entity_id).write_bool(include_data);
    w.into_bytes()
}

// ClientboundSetCameraPacket (Index 91, zwischen SET_BORDER_WARNING_DISTANCE und
// SET_CHUNK_CACHE_CENTER; Nachbar der verifizierten SET_CURSOR_ITEM 0x5E): nur die
// Entity-ID, aus deren Augen der Client sieht. Zuschauer bekommen sie per Klick auf ein
// Entity, die eigene ID heisst "zurueck zu sich selbst".
pub const CB_PLAY_SET_CAMERA: i32 = 0x5B;

pub fn read_set_camera(payload: &[u8]) -> Result<i32, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    r.read_varint()
}

// ClientboundPlayerAbilitiesPacket (Index 62, zwischen PLACE_GHOST_RECIPE und PLAYER_CHAT;
// Nachbar der verifizierten PING 0x3B und PLAYER_INFO_REMOVE 0x43). Auf der Leitung ein
// Flag-Byte (unverwundbar 1, fliegt 2, darf fliegen 4, sofort bauen 8) und die Flug- und
// Laufgeschwindigkeit als float.
pub const CB_PLAY_PLAYER_ABILITIES: i32 = 0x3E;
pub const ABILITY_INVULNERABLE: u8 = 1;
pub const ABILITY_FLYING: u8 = 2;
pub const ABILITY_MAY_FLY: u8 = 4;
pub const ABILITY_INSTABUILD: u8 = 8;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct PlayerAbilities {
    pub flags: u8,
    pub fly_speed: f32,
    pub walk_speed: f32,
}

pub fn read_player_abilities(payload: &[u8]) -> Result<PlayerAbilities, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let flags = r.read_u8()?;
    let fly_speed = r.read_f32()?;
    let walk_speed = r.read_f32()?;
    Ok(PlayerAbilities { flags, fly_speed, walk_speed })
}

// ServerboundPlayerAbilitiesPacket (Index 39, direkt vor PLAYER_ACTION 0x28): nur das
// Flug-Bit, alles andere entscheidet der Server. Vanilla schickt es, wenn der Client den
// Flug selbst umschaltet (LocalPlayer.onUpdateAbilities).
pub const SB_PLAY_PLAYER_ABILITIES: i32 = 0x27;

#[must_use] 
pub fn player_abilities_payload(flying: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLAYER_ABILITIES).write_u8(if flying { ABILITY_FLYING } else { 0 });
    w.into_bytes()
}

// EntityAttributesS2CPacket (Index 129 in derselben Zaehlung).
pub const CB_PLAY_UPDATE_ATTRIBUTES: i32 = 0x81;

// Registry-Index von `minecraft:armor`. Attribute liegen in einer EINGEBAUTEN Registry (nicht
// datapack-erweiterbar), ihre IDs sind also die Registrierungsreihenfolge in
// EntityAttributes.class -- und dort steht `armor` als allererster register()-Aufruf.
// Uebertragen wird die nackte Roh-ID als VarInt: EntityAttribute.PACKET_CODEC ist
// PacketCodecs.registryEntry(RegistryKeys.ATTRIBUTE) und das ist die EINARGUMENTIGE Fassung
// (registry(..., Registry::getIndexedEntries)) ohne das "0 = folgt inline"-Schema der
// zweiargumentigen Variante -- hier gibt es also KEIN +1.
pub(crate) const ATTRIBUTE_ID_ARMOR: i32 = 0;
// `minecraft:max_health` -- Index 19 in derselben (alphabetisch registrierten) Reihenfolge.
// Bestimmt, WIE VIELE Herzen ueberhaupt gezeichnet werden; Server mit angehobener Maximal-
// gesundheit zeigen sonst zu wenige.
pub(crate) const ATTRIBUTE_ID_MAX_HEALTH: i32 = 19;
// Die drei Attribute der Abbaugeschwindigkeit (PlayerEntity.getBlockBreakingSpeed), in
// derselben Zaehlung: block_break_speed 5, mining_efficiency 20, submerged_mining_speed 29.
pub(crate) const ATTRIBUTE_ID_BLOCK_BREAK_SPEED: i32 = 5;
pub(crate) const ATTRIBUTE_ID_MINING_EFFICIENCY: i32 = 20;
pub(crate) const ATTRIBUTE_ID_SUBMERGED_MINING_SPEED: i32 = 29;
// Die drei BEWEGUNGS-Attribute in derselben Zaehlung. GrimAC liest bei jedem
// Bewegungs-Tick genau diese drei aus (MovementCheckRunner: MOVEMENT_SPEED,
// WATER_MOVEMENT_EFFICIENCY, SNEAKING_SPEED) -- wer sie ignoriert, rechnet ab der ersten
// Verzauberung, die sie setzt, eine andere Bewegung als der Server.
//   movement_speed             22  Grundtempo (Plugin-Walkspeed, Trank-Modifikatoren)
//   sneaking_speed             26  Schleichtempo, Grundwert 0.3 -- Schleichender Schritt hebt es an
//   water_movement_efficiency  32  Wassertempo, Grundwert 0.0 -- Wasserlaeufer hebt es an
pub(crate) const ATTRIBUTE_ID_MOVEMENT_SPEED: i32 = 22;
pub(crate) const ATTRIBUTE_ID_SNEAKING_SPEED: i32 = 26;
pub(crate) const ATTRIBUTE_ID_WATER_MOVEMENT_EFFICIENCY: i32 = 32;
// Die Reichweiten in derselben Zaehlung (am 1.21.11-Jar abgelesen: camera_distance und
// explosion_knockback_resistance stehen dazwischen). Der Server legt im Kreativmodus
// +0.5 bzw. +2.0 an (ServerPlayer.updatePlayerAttributes) und schickt sie hier mit.
pub(crate) const ATTRIBUTE_ID_BLOCK_INTERACTION_RANGE: i32 = 6;
// Angriffstempo (Grundwert 4.0). Server mit 1.8-Kampf setzen den Grundwert hoch (anticheat-test:
// 20.0) -- dann ist jeder Schlag sofort aufgeladen, und Grim erwartet bei jedem Sprint-Treffer
// den Slowdown. Den Item-Modifikator rechnet der Client selbst (er wechselt erst im
// Spieler-Tick, s. App::attack_speed_attribute), deshalb kommt er hier nicht mit.
pub(crate) const ATTRIBUTE_ID_ATTACK_SPEED: i32 = 4;
pub(crate) const ITEM_ATTACK_SPEED_MODIFIER: &str = "minecraft:base_attack_speed";
pub(crate) const ATTRIBUTE_ID_ENTITY_INTERACTION_RANGE: i32 = 10;
// Die Koerper-Attribute in derselben Zaehlung. Vanilla setzt sie nur ueber Verzauberungen
// (movement_efficiency: Seelenlaeufigkeit auf Seelensand), sonst Plugins und /attribute --
// GrimAC rechnet mit allen fuenf, wer sie ignoriert, laeuft dann in Simulation-Flags.
//   gravity               14  Schwerkraft je Tick, Vorgabe 0.08
//   jump_strength         15  Sprungkraft, Vorgabe 0.42
//   movement_efficiency   21  hebt die Blockbremse (Seelensand, Honig) anteilig auf, Vorgabe 0
//   scale                 25  Groesse: Box, Augenhoehe und Modell, Vorgabe 1
//   step_height           28  Stufenhoehe, Vorgabe 0.6
pub(crate) const ATTRIBUTE_ID_GRAVITY: i32 = 14;
pub(crate) const ATTRIBUTE_ID_JUMP_STRENGTH: i32 = 15;
pub(crate) const ATTRIBUTE_ID_MOVEMENT_EFFICIENCY: i32 = 21;
pub(crate) const ATTRIBUTE_ID_SCALE: i32 = 25;
pub(crate) const ATTRIBUTE_ID_STEP_HEIGHT: i32 = 28;

// Modifikatoren auf MOVEMENT_SPEED, die der CLIENT selbst anlegt (LivingEntity/MobEffects,
// IDs am Jar geprueft) -- der Server schickt sie zusaetzlich im Attribut-Paket mit. Wir
// rechnen sie in der Physik ohnehin selbst (Sprint x1.3, Schnelligkeit/Langsamkeit, s.
// physics::effect_speed_factor), deshalb muessen sie hier UEBERSPRUNGEN werden, sonst
// zaehlen sie doppelt. Vanilla hat dasselbe Problem nicht, weil seine Modifikatoren nach ID
// in einer Map liegen und der doppelte Eintrag den ersten schlicht ersetzt.
pub(crate) const LOCAL_SPEED_MODIFIERS: [&str; 3] = ["minecraft:sprinting", "minecraft:effect.speed", "minecraft:effect.slowness"];

// EntityAttributeModifier.Operation, Reihenfolge der Enum-Konstanten.
pub(crate) const ATTRIBUTE_OP_ADD_VALUE: i32 = 0;
pub(crate) const ATTRIBUTE_OP_ADD_MULTIPLIED_BASE: i32 = 1;
pub(crate) const ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL: i32 = 2;

// Sicherheitsdeckel: ein kaputter Zaehler soll nicht in eine Endlosschleife laufen.
pub(crate) const MAX_ATTRIBUTE_ENTRIES: i32 = 128;
pub(crate) const MAX_ATTRIBUTE_MODIFIERS: i32 = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AttributeUpdate {
    pub entity_id: i32,
    // None = stand nicht in diesem Paket (der Server schickt nur geaenderte Attribute).
    pub armor: Option<f64>,
    pub max_health: Option<f64>,
    pub block_break_speed: Option<f64>,
    pub mining_efficiency: Option<f64>,
    pub submerged_mining_speed: Option<f64>,
    // OHNE die Modifikatoren aus LOCAL_SPEED_MODIFIERS -- die Physik legt sie selbst an.
    pub movement_speed: Option<f64>,
    pub sneaking_speed: Option<f64>,
    pub water_movement_efficiency: Option<f64>,
    pub block_interaction_range: Option<f64>,
    pub entity_interaction_range: Option<f64>,
    // Angriffstempo OHNE den Item-Modifikator, zerlegt wie computeValue: (Grundwert + alle
    // ADD_VALUE, Summe der ADD_MULTIPLIED_BASE, Produkt der ADD_MULTIPLIED_TOTAL-Faktoren).
    // Der Wert mit Item ist dann (a + item) * (1 + b) * c.
    pub attack_speed: Option<(f64, f64, f64)>,
    // Die Koerper-Attribute, schon auf Vanillas Grenzen geklemmt (RangedAttribute.sanitizeValue).
    pub gravity: Option<f64>,
    pub jump_strength: Option<f64>,
    pub movement_efficiency: Option<f64>,
    pub scale: Option<f64>,
    pub step_height: Option<f64>,
}

// Ruestung und Maximalgesundheit aus dem Attribut-Paket; alles andere wird ueberlesen.
//
// Warum ueber das Attribut und nicht ueber die getragenen Teile: `PlayerEntity.getArmor()`
// IST in Vanilla nichts anderes als der Attributwert. Eine eigene Tabelle "Eisenharnisch = 6"
// waere eine zweite Wahrheit, die bei jedem Verzauberungs-/Plugin-Modifikator falsch liegt.
pub fn read_attributes(payload: &[u8]) -> Result<AttributeUpdate, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let entity_id = r.read_varint()?;
    let count = r.read_varint()?;
    if !(0..=MAX_ATTRIBUTE_ENTRIES).contains(&count) {
        return Err(ReadError);
    }
    let mut out = AttributeUpdate { entity_id, ..Default::default() };
    for _ in 0..count {
        let attribute_id = r.read_varint()?;
        let base = r.read_f64()?;
        let modifier_count = r.read_varint()?;
        if !(0..=MAX_ATTRIBUTE_MODIFIERS).contains(&modifier_count) {
            return Err(ReadError);
        }
        // EntityAttributeInstance.computeValue(): erst alle ADD_VALUE auf den Grundwert,
        // dann ADD_MULTIPLIED_BASE auf DIESE Zwischensumme (jeweils mit dem Grundwert nach
        // Stufe 1 multipliziert), zuletzt ADD_MULTIPLIED_TOTAL multiplikativ.
        let mut sum = base;
        let mut multiplied_base = 0.0;
        let mut total_factor = 1.0;
        for _ in 0..modifier_count {
            let id = r.read_string()?;
            let amount = r.read_f64()?;
            let operation = r.read_varint()?;
            // s. LOCAL_SPEED_MODIFIERS: nur beim Tempo-Attribut, und nur fuer die IDs, die
            // die Physik selbst anlegt.
            // Wie Identifier.parse: ohne Namensraum gilt "minecraft:". anticheat-test.com
            // schickt den Sprint-Modifikator als blosses "sprinting" -- ungefiltert zaehlte der
            // Sprint dann doppelt (Tempo x1.69, Grim Simulation 0.038 je Tick).
            let voll = if id.contains(':') { id } else { format!("minecraft:{id}") };
            if attribute_id == ATTRIBUTE_ID_MOVEMENT_SPEED && LOCAL_SPEED_MODIFIERS.contains(&voll.as_str()) {
                continue;
            }
            if attribute_id == ATTRIBUTE_ID_ATTACK_SPEED && voll == ITEM_ATTACK_SPEED_MODIFIER {
                continue;
            }
            match operation {
                ATTRIBUTE_OP_ADD_VALUE => sum += amount,
                ATTRIBUTE_OP_ADD_MULTIPLIED_BASE => multiplied_base += amount,
                ATTRIBUTE_OP_ADD_MULTIPLIED_TOTAL => total_factor *= 1.0 + amount,
                _ => {}
            }
        }
        let value = (sum + sum * multiplied_base) * total_factor;
        match attribute_id {
            ATTRIBUTE_ID_ARMOR => out.armor = Some(value),
            ATTRIBUTE_ID_MAX_HEALTH => out.max_health = Some(value),
            ATTRIBUTE_ID_BLOCK_BREAK_SPEED => out.block_break_speed = Some(value),
            ATTRIBUTE_ID_MINING_EFFICIENCY => out.mining_efficiency = Some(value),
            ATTRIBUTE_ID_SUBMERGED_MINING_SPEED => out.submerged_mining_speed = Some(value),
            ATTRIBUTE_ID_MOVEMENT_SPEED => out.movement_speed = Some(value),
            ATTRIBUTE_ID_SNEAKING_SPEED => out.sneaking_speed = Some(value),
            ATTRIBUTE_ID_WATER_MOVEMENT_EFFICIENCY => out.water_movement_efficiency = Some(value),
            ATTRIBUTE_ID_BLOCK_INTERACTION_RANGE => out.block_interaction_range = Some(value),
            ATTRIBUTE_ID_ENTITY_INTERACTION_RANGE => out.entity_interaction_range = Some(value),
            ATTRIBUTE_ID_ATTACK_SPEED => out.attack_speed = Some((sum, multiplied_base, total_factor)),
            // Grenzen aus Attributes (RangedAttribute min/max).
            ATTRIBUTE_ID_GRAVITY => out.gravity = Some(value.clamp(-1.0, 1.0)),
            ATTRIBUTE_ID_JUMP_STRENGTH => out.jump_strength = Some(value.clamp(0.0, 32.0)),
            ATTRIBUTE_ID_MOVEMENT_EFFICIENCY => out.movement_efficiency = Some(value.clamp(0.0, 1.0)),
            ATTRIBUTE_ID_SCALE => out.scale = Some(value.clamp(0.0625, 16.0)),
            ATTRIBUTE_ID_STEP_HEIGHT => out.step_height = Some(value.clamp(0.0, 10.0)),
            _ => {}
        }
    }
    Ok(out)
}

// --- Scoreboard ---------------------------------------------------------------------------
