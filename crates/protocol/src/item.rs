// ItemStacks ("Slots") aus dem Protokoll, inklusive der Data-Components.
//
// Wire-Format seit 1.20.5 (verifiziert am 1.21.11-Jar, ItemStack$1 /
// ComponentChanges.PACKET_CODEC):
//
//   varint count           -- 0 = leerer Slot, dann ist der Slot zu Ende
//   varint item_id         -- Registry-ID (s. world::item_registry)
//   varint n_add           -- Anzahl gesetzter Components
//   varint n_remove        -- Anzahl entfernter (auf Default zurueckgesetzter) Components
//   n_add  * (varint type_id + typspezifische Payload)
//   n_remove * varint type_id
//
// Der Haken: die Payloads haben KEIN Laengenpraefix (die laengenpraefixte Variante
// LENGTH_PREPENDED_PACKET_CODEC nutzt Vanilla nur woanders). Wer einen Component-Typ nicht
// byteweise kennt, kann ihn also nicht ueberspringen und verliert den Rest der Slot-Liste.
// Deshalb steht hier fuer JEDEN der 104 Typen von 1.21.11 ein exakter Leser (s.
// read_component_payload). Uebertragen wird ohnehin nur, was vom Item-Default ABWEICHT --
// die exotischen Typen tauchen real also selten auf, aber ein einziger unbekannter wuerde
// die ganze Kiste leer erscheinen lassen.
//
// Fehlerstrategie: ein unbekannter/unlesbarer Component bricht NUR den aktuellen Stack ab
// (ComponentError::Unsupported mit Typ-ID, damit man es im Log sieht) -- der Aufrufer
// behaelt die bereits gelesenen Slots und wirft nicht das ganze Packet weg.

use crate::buffer::{PacketReader, PacketWriter, ReadError};
use crate::nbt;

// Ein Stack, reduziert auf das, was Client-Anzeige und Interaktion brauchen. Die uebrigen
// Components werden korrekt ueberlesen, aber nicht aufgehoben -- wer spaeter z.B. Trims
// rendern will, faengt hier an.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemStack {
    pub item_id: u32,
    pub count: i32,
    // Schadenswert (damage-Component). Zusammen mit max_damage ergibt das den
    // Haltbarkeitsbalken; max_damage kommt nur mit, wenn das Item vom Default abweicht --
    // sonst kennt die App den Default aus der Item-Tabelle nicht und zeigt keinen Balken.
    pub damage: Option<i32>,
    pub max_damage: Option<i32>,
    // custom_name (vom Spieler/Server umbenannt) bzw. item_name (Server-Anzeigename).
    pub custom_name: Option<String>,
    // enchantments-Component: (Registry-ID der Verzauberung, Stufe), in Empfangsreihenfolge.
    // Die ID zeigt in die Verzauberungs-Registry der Konfigurationsphase (s.
    // packets::read_enchantments) -- die ist datengetrieben, feste Nummern gibt es nicht.
    pub enchantments: Vec<Enchantment>,
    // stored_enchantments: dasselbe fuer ein Verzauberungsbuch. Wirkt NICHT, wird nur
    // angezeigt (und traegt keinen eigenen Glint -- den bekommt das Buch ueber seinen Default,
    // s. world::enchantment::has_glint).
    pub stored_enchantments: Vec<Enchantment>,
    // enchantment_glint_override, falls GESETZT (vom Item-Default abweichend).
    pub glint_override: Option<bool>,
    // Hat der Stack GESETZTE Components (vom Item-Default abweichend)? Nur solche Stacks
    // lassen sich nicht exakt hashen: der Klick-Hash (s. packets::ItemStackHash) verlangt
    // je gesetztem Component einen Prüfwert ueber dessen Inhalt, den wir hier gar nicht
    // aufheben. Entfernte Components (removed) brauchen dagegen keinen Wert, nur ihre ID --
    // die sind also weiterhin exakt hashbar.
    pub has_added_components: bool,
    // IDs der auf ihren Default zurueckgesetzten Components, in Empfangsreihenfolge.
    pub removed_components: Vec<i32>,
    // Die gesetzten Components so, wie sie auf der Leitung standen (leer ohne welche). Damit
    // laesst sich "identische Components" pruefen, ohne jeden Component-Typ auszuwerten --
    // gebraucht fuers Zusammenlegen, z.B. Pfeile mit Trank (s. stackable_with).
    pub added_component_bytes: Vec<u8>,
    // Gesetzte use_cooldown-Component (UseCooldownComponent: Sekunden + optionale
    // Cooldown-Gruppe). Nur der GESETZTE Wert -- den Item-Default kennt die App (s.
    // item_cooldowns::use_cooldown).
    pub use_cooldown: Option<UseCooldown>,
    // Gesetzte food-Component: Some(can_always_eat). Ein Server kann Essen damit jederzeit
    // essbar machen (Golden Heads) -- dann darf der Client es bei vollem Hunger nicht sperren.
    pub food_always_edible: Option<bool>,
    // Gesetzte consumable-Component: Essdauer in Sekunden (consume_seconds). Server bauen damit
    // eigene Esswaren (Golden Heads) mit eigener Dauer.
    pub consume_seconds: Option<f32>,
    // Gesetztes tool-Component (vom Server geaendertes Werkzeug).
    pub tool: Option<ToolData>,
    // charged_projectiles nicht leer: eine GELADENE Armbrust. Daran haengt, ob ein Rechtsklick
    // schiesst (einmalig) oder das Laden beginnt (Dauerbenutzung), s. CrossbowItem.use.
    // Item-IDs der geladenen Geschosse -- leer = ungeladen. Vanilla waehlt daran auch das
    // Bild (items/crossbow.json: charge_type "rocket", sobald eine Rakete dabei ist).
    pub charged_projectiles: Vec<u32>,
    // potion_contents (Trank, Wurf-/Verweiltrank, getippter Pfeil): Name und Flaschenfarbe
    // haengen daran, s. world::potion.
    pub potion: Option<PotionContents>,
    // potion_duration_scale: Faktor auf die Wirkungsdauern im Tooltip (Verweiltrank 0.25).
    pub potion_duration_scale: Option<f32>,
    // item_model-Component: das Modell, mit dem der Server das Item zeigen will, statt des
    // Item-Standards (z.B. "minecraft:golden_apple").
    pub item_model: Option<String>,
    // profile-Component eines Spielerkopfes: wessen Skin er zeigt.
    pub profile: Option<HeadProfile>,
    // custom_name und item_name MIT Farben (fuer den Tooltip; custom_name oben ist der
    // Klartext).
    pub custom_name_spans: Option<Vec<nbt::TextSpan>>,
    pub item_name_spans: Option<Vec<nbt::TextSpan>>,
    // lore: je Zeile ihre Abschnitte.
    pub lore: Vec<Vec<nbt::TextSpan>>,
    // rarity, falls gesetzt (0 common, 1 uncommon, 2 rare, 3 epic).
    pub rarity: Option<u8>,
    // dyed_color (0xRRGGBB): gefaerbte Lederruestung.
    pub dyed_color: Option<u32>,
    // trim: (Material-ID, Muster-ID) in den Registries trim_material/trim_pattern der
    // Konfigurationsphase; -1 fuer inline geschickte (Datapack-)Eintraege.
    pub trim: Option<(i32, i32)>,
    // banner_patterns: (ID in der banner_pattern-Registry oder -1, Farbe), unten zuerst.
    pub banner_patterns: Vec<(i32, u8)>,
    // base_color (DyeColor): Grundfarbe eines Schildes mit Banner.
    pub base_color: Option<u8>,
    // equippable, falls GESETZT (vom Item-Default abweichend). Ob sie ENTFERNT wurde, steht in
    // removed_components (s. equippable_removed).
    pub equippable: Option<Equippable>,
    // IDs der GESETZTEN Components in Empfangsreihenfolge (auch die, deren Wert hier nicht
    // aufgehoben wird) -- fuer "hat die Component" (Item-Modelle: has_component).
    pub added_ids: Vec<i32>,
    // custom_model_data: die vier Listen, mit denen Packs Modelle waehlen (s. assets
    // item_model) -- Zahlen, Schalter, Texte, Farben.
    pub custom_model_data: Option<CustomModelData>,
    // max_stack_size, falls gesetzt.
    pub max_stack_size: Option<i32>,
    // Verhalten, das Items ueber Components bekommen (je falls gesetzt) -- s.
    // world::item_components fuer die wirksamen Werte samt Item-Standard.
    pub use_effects: Option<UseEffects>,
    pub consume_animation: Option<i32>,
    // consumable.has_consume_particles: Kruemel beim Essen (Getraenke haben keine).
    pub consume_particles: Option<bool>,
    pub food_nutrition: Option<i32>,
    pub weapon: Option<Weapon>,
    pub attack_range: Option<AttackRange>,
    pub blocks_attacks: Option<BlocksAttacks>,
    pub piercing_weapon: Option<PiercingWeapon>,
    pub kinetic_weapon: Option<KineticWeapon>,
    pub swing_animation: Option<SwingAnimation>,
    pub minimum_attack_charge: Option<f32>,
    // can_place_on / can_break (Abenteuermodus): s. BlockPredicate.
    pub can_place_on: Option<Vec<BlockPredicate>>,
    pub can_break: Option<Vec<BlockPredicate>>,
    // bundle_contents: was im Buendel steckt, das zuletzt Eingelegte zuerst.
    pub bundle_contents: Option<Vec<ItemStack>>,
    // Im Buendel gewaehltes Item (Mausrad ueber dem Buendel), -1 = keins. Nur im Client, wie
    // Vanillas BundleContents.selectedItem -- nicht Teil der Netzform.
    pub bundle_selected: i32,
    // bees: Anzahl Bienen im Nest/Stock (fuer das Buendel-Gewicht).
    pub bees: Option<usize>,
    // writable_book_content: die Seiten des Buchs mit Feder als Klartext.
    pub writable_pages: Option<Vec<String>>,
    // written_book_content: Titel, Autor, Generation und die Seiten als Text-Components.
    pub written_book: Option<WrittenBook>,
    // map_id: welche Karte (Kartendaten kommen per eigenem Paket).
    pub map_id: Option<i32>,
    // attribute_modifiers, falls gesetzt -- die Standards (Schaden eines Schwerts, Ruestung
    // eines Helms) kommen ueber world::item_components.
    pub attribute_modifiers: Option<Vec<ItemAttributeModifier>>,
    // tooltip_display: der ganze Tooltip aus, bzw. diese Component-IDs nicht im Tooltip.
    pub tooltip_hidden: bool,
    pub tooltip_hidden_components: Vec<i32>,
}

// Ein Eintrag der attribute_modifiers-Component. `attribute` ist die Roh-ID der eingebauten
// Attribut-Registry (s. ATTRIBUTE_*), `slot` die EquipmentSlotGroup.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemAttributeModifier {
    pub attribute: i32,
    pub id: String,
    pub amount: f64,
    // 0 add_value, 1 add_multiplied_base, 2 add_multiplied_total.
    pub operation: i32,
    pub slot: i32,
    // Display: true = im Tooltip versteckt.
    pub hidden: bool,
}

// Attribut-IDs in Registrierungsreihenfolge (am 1.21.11-Jar abgelesen).
pub const ATTRIBUTE_ARMOR: i32 = 0;
pub const ATTRIBUTE_ARMOR_TOUGHNESS: i32 = 1;
pub const ATTRIBUTE_ATTACK_DAMAGE: i32 = 2;
pub const ATTRIBUTE_ATTACK_SPEED: i32 = 4;
// EquipmentSlotGroup in Enum-Reihenfolge: any, mainhand, offhand, hand, feet, legs, chest,
// head, armor, body, saddle.
pub const SLOT_GROUP_ANY: i32 = 0;
pub const SLOT_GROUP_MAINHAND: i32 = 1;
pub const SLOT_GROUP_HAND: i32 = 3;
pub const SLOT_GROUP_FEET: i32 = 4;
pub const SLOT_GROUP_ARMOR: i32 = 8;
pub const COMPONENT_ATTRIBUTE_MODIFIERS: i32 = 16;
pub const COMPONENT_ATTACK_RANGE: i32 = 30;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WrittenBook {
    pub title: String,
    pub author: String,
    // 0 Original, 1 Kopie des Originals, 2 Kopie einer Kopie, 3 zerfleddert.
    pub generation: i32,
    pub pages: Vec<Vec<nbt::TextSpan>>,
    // Hat der Server Auswahl-, Punkte- und NBT-Texte schon aufgeloest?
    pub resolved: bool,
}

// UseEffects: waehrend der Benutzung sprinten erlaubt, Vibrationen, Tempo-Faktor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UseEffects {
    pub can_sprint: bool,
    pub interact_vibrations: bool,
    pub speed_multiplier: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weapon {
    pub item_damage_per_attack: i32,
    pub disable_blocking_for_seconds: f32,
}

// AttackRange: Reichweite eines Angriffs (Ueberleben und Kreativ), Zuschlag auf die Hitbox und
// Faktor fuer Mobs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackRange {
    pub min_reach: f32,
    pub max_reach: f32,
    pub min_creative_reach: f32,
    pub max_creative_reach: f32,
    pub hitbox_margin: f32,
    pub mob_factor: f32,
}

// BlocksAttacks (Schild): Verzoegerung bis zum Blocken und wie stark Axthiebe die Abklingzeit
// verlaengern -- die Schadenstabellen bleiben Sache des Servers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlocksAttacks {
    pub block_delay_seconds: f32,
    pub disable_cooldown_scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PiercingWeapon {
    pub deals_knockback: bool,
    pub dismounts: bool,
}

// KineticWeapon (Speer-Ansturm): Abklingzeit je Treffer, Verzoegerung und die Hoechstdauern der
// drei Bedingungen (Absitzen, Rueckstoss, Schaden), alles in Ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KineticWeapon {
    pub contact_cooldown_ticks: i32,
    pub delay_ticks: i32,
    pub dismount: Option<KineticCondition>,
    pub knockback: Option<KineticCondition>,
    pub damage: Option<KineticCondition>,
    pub forward_movement: f32,
    pub damage_multiplier: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KineticCondition {
    pub max_duration_ticks: i32,
    pub min_speed: f32,
    pub min_relative_speed: f32,
}

// SwingAnimation: Art (0 keine, 1 Schlag, 2 Stich) und Dauer in Ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingAnimation {
    pub kind: i32,
    pub duration: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomModelData {
    pub floats: Vec<f32>,
    pub flags: Vec<bool>,
    pub strings: Vec<String>,
    pub colors: Vec<i32>,
}

// equippable-Component (EquippableComponent): wo das Item getragen wird und wie es dort
// aussieht. Server bauen damit eigene Ruestung oder machen sie unsichtbar -- UHC Royale gibt
// dem Helm das Modell "uhc:invisible_helmet", dessen Equipment-Datei keine Ebenen hat.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Equippable {
    // EquipmentSlot-ORDINAL (wie im Equipment-Paket): 0 Haupthand, 1 Zweithand, 2 Fuesse,
    // 3 Beine, 4 Brust, 5 Kopf, 6 Koerper (Tiere), 7 Sattel. Auf der Leitung steht die Netz-ID,
    // s. read_component_payload.
    pub slot: u8,
    // Equipment-Asset ("ns:pfad" -> assets/ns/equipment/pfad.json). None = getragen, aber
    // ohne Bild.
    pub asset_id: Option<String>,
    // Bild ueber dem Bildschirm, solange man das Item traegt (Kuerbis: "misc/pumpkinblur").
    pub camera_overlay: Option<String>,
    // Rechtsklick zieht es an (bzw. tauscht mit dem Getragenen).
    pub swappable: bool,
    // Nur fuer bestimmte Entity-Typen (Pferde-, Wolfsruestung) -- ein Spieler kann es dann nicht
    // anziehen.
    pub restricted: bool,
}

// Das Profil eines PLATZIERTEN Kopfes aus dem Block-Entity-NBT (SkullBlockEntity, Schluessel
// "profile", ProfileComponent-Codec): entweder nur ein Name als String oder ein Compound mit
// name, id (UUID als vier Ints) und properties [{name: "textures", value, ...}].
pub fn head_profile_from_nbt(v: &crate::nbt::NbtValue) -> Option<HeadProfile> {
    use crate::nbt::NbtValue;
    let profil = v.get("profile")?;
    if let Some(name) = profil.as_str() {
        return Some(HeadProfile { name: Some(name.to_string()), ..Default::default() });
    }
    let mut out = HeadProfile { name: profil.get("name").and_then(NbtValue::as_str).map(str::to_string), ..Default::default() };
    if let Some(NbtValue::IntArray(teile)) = profil.get("id") {
        if teile.len() == 4 {
            let mut uuid = [0u8; 16];
            for (i, t) in teile.iter().enumerate() {
                uuid[i * 4..i * 4 + 4].copy_from_slice(&t.to_be_bytes());
            }
            out.uuid = Some(uuid);
        }
    }
    if let Some(NbtValue::List(props)) = profil.get("properties") {
        out.textures = props
            .iter()
            .find(|p| p.get("name").and_then(NbtValue::as_str) == Some("textures"))
            .and_then(|p| p.get("value"))
            .and_then(NbtValue::as_str)
            .map(str::to_string);
    }
    (out.uuid.is_some() || out.name.is_some() || out.textures.is_some()).then_some(out)
}

// ResolvableProfile, reduziert auf das, was ein Kopf zum Zeichnen braucht.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadProfile {
    pub uuid: Option<[u8; 16]>,
    pub name: Option<String>,
    // Wert der "textures"-Property (Base64-JSON mit der Skin-URL), falls mitgeschickt.
    pub textures: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enchantment {
    pub id: u32,
    pub level: i32,
}

// Registry-ID der enchantment_glint_override-Component (s. read_component_payload).
pub const COMPONENT_ENCHANTMENT_GLINT_OVERRIDE: i32 = 21;

// PotionContents.STREAM_CODEC: optionaler Basistrank (Registry-ID ohne +1), optionale feste
// Farbe, zusaetzliche Effekte und ein optionaler Namens-Schluessel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PotionContents {
    pub potion: Option<u32>,
    pub custom_color: Option<i32>,
    pub custom_effects: Vec<PotionEffect>,
    pub custom_name: Option<String>,
}

// Das, was von einer MobEffectInstance fuer die Farbe zaehlt (PotionContents.getColorOptional).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PotionEffect {
    pub effect: u32,
    pub amplifier: i32,
    // In Ticks, -1 = unendlich.
    pub duration: i32,
    // showParticles -- MobEffectInstance.isVisible(); unsichtbare Effekte faerben nicht mit.
    pub visible: bool,
}

// ToolComponent wie auf der Leitung. Die Blockmenge einer Regel ist entweder ein Tag oder
// eine Liste von Block-Roh-IDs (PacketCodecs.registryEntryList).
#[derive(Clone, Debug, PartialEq)]
pub struct ToolData {
    pub rules: Vec<ToolRuleData>,
    pub default_speed: f32,
    pub can_destroy_blocks_in_creative: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolRuleData {
    pub tag: Option<String>,
    pub ids: Vec<i32>,
    pub speed: Option<f32>,
    pub correct_for_drops: Option<bool>,
}

// Registry-ID des tool-Components.
pub const COMPONENT_TOOL: i32 = 28;

// AdventureModePredicate (can_place_on 14 / can_break 15): ein Block passt, wenn EINES der
// Praedikate passt (BlockPredicate.test, ausgewertet in world::item_behavior).
pub const COMPONENT_CAN_PLACE_ON: i32 = 14;
pub const COMPONENT_CAN_BREAK: i32 = 15;

#[derive(Clone, Debug, PartialEq)]
pub struct BlockPredicate {
    // None = jeder Block. Sonst wie bei ToolRuleData: Tag ODER Block-Roh-IDs.
    pub blocks: Option<(Option<String>, Vec<i32>)>,
    // StatePropertiesPredicate: Property-Name und Bedingung, alle muessen passen.
    pub properties: Vec<(String, StateValueMatcher)>,
    // Bedingungen an NBT oder Components des Blockobjekts. Die Daten dafuer fuehren wir
    // nicht -- ein solches Praedikat passt hier nie, entscheiden tut ohnehin der Server.
    pub needs_block_entity: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StateValueMatcher {
    Exact(String),
    Range(Option<String>, Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UseCooldown {
    pub seconds: f32,
    pub group: Option<String>,
}

// Registry-ID der food-Component (FoodComponent: nutrition, saturation, can_always_eat).
pub const COMPONENT_CONSUMABLE: i32 = 24;
pub const COMPONENT_FOOD: i32 = 23;
// Registry-ID der use_cooldown-Component (s. read_component_payload).
pub const COMPONENT_USE_COOLDOWN: i32 = 26;
pub const COMPONENT_EQUIPPABLE: i32 = 32;

impl ItemStack {
    pub fn new(item_id: u32, count: i32) -> Self {
        Self {
            item_id,
            count,
            damage: None,
            max_damage: None,
            custom_name: None,
            enchantments: Vec::new(),
            stored_enchantments: Vec::new(),
            glint_override: None,
            has_added_components: false,
            removed_components: Vec::new(),
            added_component_bytes: Vec::new(),
            use_cooldown: None,
            food_always_edible: None,
            consume_seconds: None,
            tool: None,
            charged_projectiles: Vec::new(),
            potion: None,
            potion_duration_scale: None,
            item_model: None,
            profile: None,
            custom_name_spans: None,
            item_name_spans: None,
            lore: Vec::new(),
            rarity: None,
            dyed_color: None,
            trim: None,
            banner_patterns: Vec::new(),
            base_color: None,
            equippable: None,
            added_ids: Vec::new(),
            custom_model_data: None,
            max_stack_size: None,
            use_effects: None,
            consume_animation: None,
            consume_particles: None,
            food_nutrition: None,
            weapon: None,
            attack_range: None,
            blocks_attacks: None,
            piercing_weapon: None,
            kinetic_weapon: None,
            swing_animation: None,
            minimum_attack_charge: None,
            can_place_on: None,
            can_break: None,
            bundle_contents: None,
            bundle_selected: -1,
            bees: None,
            writable_pages: None,
            written_book: None,
            map_id: None,
            attribute_modifiers: None,
            tooltip_hidden: false,
            tooltip_hidden_components: Vec::new(),
        }
    }

    // Hat der Server die equippable-Component dieses Stacks entfernt? Dann traegt Vanilla das
    // Item zwar nicht mehr ueber Rechtsklick, und getragen zeigt es kein Ruestungsbild.
    pub fn equippable_removed(&self) -> bool {
        self.removed_components.contains(&COMPONENT_EQUIPPABLE)
    }

    pub fn is_charged(&self) -> bool {
        !self.charged_projectiles.is_empty()
    }

    // Laesst sich dieser Stack byte-genau als ItemStackHash beschreiben? (s.
    // has_added_components)
    pub fn hashable(&self) -> bool {
        !self.has_added_components
    }

    // Duerfen zwei Stapel zusammengelegt werden? Vanilla verlangt gleiches Item UND
    // identische Components (ItemStack.areItemsAndComponentsEqual). Verglichen werden die
    // gesetzten Components byteweise, wie sie auf der Leitung standen: gleiche Bytes heissen
    // gleiche Components. Umgekehrt koennte der Server dieselben Components in anderer
    // Reihenfolge schicken -- dann liefert das hier false, also "nicht vorhersagen", was
    // der sichere Ausgang ist.
    //
    // ECHTER BUG gewesen: vorher galt jeder Stapel MIT gesetzten Components als nie
    // zusammenlegbar. Pfeile mit Trank (sanfter Fall ...) tragen immer welche -- der
    // Doppelklick zum Einsammeln tat bei ihnen deshalb gar nichts.
    pub fn stackable_with(&self, other: &ItemStack) -> bool {
        self.item_id == other.item_id
            && self.has_added_components == other.has_added_components
            && self.added_component_bytes == other.added_component_bytes
            && self.removed_components == other.removed_components
    }
}

#[derive(Debug, PartialEq)]
pub enum SlotError {
    // Buffer zu Ende / kaputtes Packet.
    Read,
    // Component-Typ, den dieser Client (noch) nicht byteweise kennt. Ab hier ist die
    // Leseposition unbrauchbar; der Aufrufer bricht die Liste ab und loggt die ID.
    UnsupportedComponent(i32),
}

impl From<ReadError> for SlotError {
    fn from(_: ReadError) -> Self {
        SlotError::Read
    }
}

// Optionaler Slot (ItemStack.OPTIONAL_PACKET_CODEC): count <= 0 bedeutet "leer".
pub fn read_optional_slot(r: &mut PacketReader) -> Result<Option<ItemStack>, SlotError> {
    let count = r.read_varint()?;
    if count <= 0 {
        return Ok(None);
    }
    let item_id = r.read_varint()?.max(0) as u32;
    let mut stack = ItemStack::new(item_id, count);
    read_component_changes(r, &mut stack)?;
    Ok(Some(stack))
}

// Die gesetzten Components eines Stapels einzeln: (Typ-ID, Netzform ohne Typ und Laenge).
// Der Stapel haelt sie nur hintereinander (added_component_bytes); die Grenzen ergeben sich
// erst beim erneuten Lesen.
pub fn added_components(stack: &ItemStack) -> Result<Vec<(i32, Vec<u8>)>, SlotError> {
    let mut r = PacketReader::new(&stack.added_component_bytes);
    let mut leer = ItemStack::new(stack.item_id, 1);
    let mut out = Vec::new();
    while !r.is_exhausted() {
        let typ = r.read_varint()?;
        let start = r.position();
        read_component_payload(&mut r, typ, &mut leer)?;
        out.push((typ, r.read_since(start).to_vec()));
    }
    Ok(out)
}

// ItemStack.OPTIONAL_UNTRUSTED_STREAM_CODEC, die Form im Kreativ-Slot-Paket: wie der normale
// Slot, nur steht vor jeder gesetzten Component ihre Laenge in Bytes
// (DataComponentPatch.DELIMITED_STREAM_CODEC) -- der Server kann so jede einzeln pruefen.
pub fn write_untrusted_slot(w: &mut PacketWriter, stack: Option<&ItemStack>) -> Result<(), SlotError> {
    let Some(stack) = stack.filter(|s| s.count > 0) else {
        w.write_varint(0);
        return Ok(());
    };
    let teile = added_components(stack)?;
    w.write_varint(stack.count).write_varint(stack.item_id as i32);
    w.write_varint(teile.len() as i32).write_varint(stack.removed_components.len() as i32);
    for (typ, bytes) in &teile {
        w.write_varint(*typ).write_bytes(bytes);
    }
    for typ in &stack.removed_components {
        w.write_varint(*typ);
    }
    Ok(())
}

// Ein Stapel aus seinen Teilen, gelesen wie vom Server: so fuellen sich alle Anzeige-Felder
// (Trankfarbe, Verzauberungen, Name ...) auf demselben Weg wie bei jedem empfangenen Slot.
pub fn stack_from_parts(item_id: u32, count: i32, added: &[(i32, Vec<u8>)], removed: &[i32]) -> Result<ItemStack, SlotError> {
    let mut w = PacketWriter::new();
    w.write_varint(count).write_varint(item_id as i32);
    w.write_varint(added.len() as i32).write_varint(removed.len() as i32);
    for (typ, bytes) in added {
        w.write_varint(*typ).write_raw_bytes(bytes);
    }
    for typ in removed {
        w.write_varint(*typ);
    }
    let bytes = w.into_bytes();
    let mut r = PacketReader::new(&bytes);
    read_optional_slot(&mut r)?.ok_or(SlotError::Read)
}

// Preis eines Handels (TradedItem.PACKET_CODEC): VarInt Item, VarInt Anzahl, dann die
// exakt geforderten Components als Liste (VarInt Laenge, je VarInt Typ + Payload -- dasselbe
// Payload-Format wie im Slot). Als Stack zurueck, damit Anzeige und Tooltip wie bei jedem
// Slot funktionieren; die geforderten Components landen dabei als "gesetzt" im Stack.
pub fn read_item_cost(r: &mut PacketReader) -> Result<ItemStack, SlotError> {
    let item_id = r.read_varint()?.max(0) as u32;
    let count = r.read_varint()?;
    let mut stack = ItemStack::new(item_id, count);
    let n = r.read_varint()?;
    if n < 0 {
        return Err(SlotError::Read);
    }
    stack.has_added_components = n > 0;
    let start = r.position();
    for _ in 0..n {
        let type_id = r.read_varint()?;
        read_component_payload(r, type_id, &mut stack)?;
    }
    stack.added_component_bytes = r.read_since(start).to_vec();
    Ok(stack)
}

// Nicht-optionaler Slot (ItemStack.PACKET_CODEC): identisches Format, aber count == 0 ist
// serverseitig verboten. Kommt in Components vor, die selbst Stacks enthalten
// (charged_projectiles, bundle_contents, use_remainder).
fn read_slot(r: &mut PacketReader) -> Result<(), SlotError> {
    read_optional_slot(r).map(|_| ())
}

// Wo das Lesen des letzten fehlgeschlagenen Stacks haengen blieb: Item, Component und die
// Components davor. Nur fuer die Diagnose (s. last_slot_failure) -- ein Lesefehler
// verwirft den Rest einer ganzen Slot-Liste, und ohne diese Angabe weiss niemand, woran.
thread_local! {
    static LETZTER_FEHLER: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

// Die Beschreibung des letzten Lesefehlers dieses Threads (und zuruecksetzen).
pub fn last_slot_failure() -> Option<String> {
    LETZTER_FEHLER.with(|f| f.borrow_mut().take())
}

// Eine einzelne Component in Netzform (ohne Typ-ID und Laenge) auf `stack` anwenden -- wie
// beim Lesen eines Slots. Gebraucht fuer die Standard-Components der Items, die der Launcher in
// derselben Netzform extrahiert (s. assets ItemComponentsDump.java). Ein Rest hinter dem Wert
// gilt als Fehler: dann passte der Parser nicht zur Netzform.
pub fn apply_component(stack: &mut ItemStack, type_id: i32, bytes: &[u8]) -> Result<(), SlotError> {
    let mut r = PacketReader::new(bytes);
    read_component_payload(&mut r, type_id, stack)?;
    if r.position() != bytes.len() {
        return Err(SlotError::UnsupportedComponent(type_id));
    }
    if !stack.added_ids.contains(&type_id) {
        stack.added_ids.push(type_id);
    }
    Ok(())
}

fn read_component_changes(r: &mut PacketReader, stack: &mut ItemStack) -> Result<(), SlotError> {
    let n_add = r.read_varint()?;
    let n_remove = r.read_varint()?;
    if n_add < 0 || n_remove < 0 {
        return Err(SlotError::Read);
    }
    stack.has_added_components = n_add > 0;
    let start = r.position();
    let mut gelesen: Vec<i32> = Vec::new();
    for _ in 0..n_add {
        let type_id = r.read_varint()?;
        if let Err(e) = read_component_payload(r, type_id, stack) {
            let text = format!("Item-ID {} ({n_add} Components): Component {type_id} fehlgeschlagen ({e:?}), davor gelesen {gelesen:?}", stack.item_id);
            // Verschachtelte Stacks (Container, Buendel) melden zuerst den INNEREN Fehler --
            // der aeussere haengt sich nur an.
            LETZTER_FEHLER.with(|f| {
                let mut f = f.borrow_mut();
                *f = Some(match f.take() {
                    Some(innen) => format!("{text} <- innen: {innen}"),
                    None => text,
                });
            });
            return Err(e);
        }
        gelesen.push(type_id);
    }
    stack.added_ids = gelesen;
    stack.added_component_bytes = r.read_since(start).to_vec();
    for _ in 0..n_remove {
        stack.removed_components.push(r.read_varint()?);
    }
    Ok(())
}

// --- Bausteine der Vanilla-PacketCodecs (s. PacketCodecs.java) -------------------------

// PacketCodecs.optional: bool-Praefix, dann ggf. der Wert.
fn optional<F>(r: &mut PacketReader, mut f: F) -> Result<(), SlotError>
where
    F: FnMut(&mut PacketReader) -> Result<(), SlotError>,
{
    if r.read_bool()? {
        f(r)?;
    }
    Ok(())
}

// PacketCodecs.toList/toCollection/map: VarInt-Anzahl, dann die Elemente.
fn list<F>(r: &mut PacketReader, mut f: F) -> Result<(), SlotError>
where
    F: FnMut(&mut PacketReader) -> Result<(), SlotError>,
{
    let len = r.read_varint()?;
    if len < 0 {
        return Err(SlotError::Read);
    }
    for _ in 0..len {
        f(r)?;
    }
    Ok(())
}

fn skip_varint(r: &mut PacketReader) -> Result<(), SlotError> {
    r.read_varint()?;
    Ok(())
}

fn skip_string(r: &mut PacketReader) -> Result<(), SlotError> {
    r.read_string()?;
    Ok(())
}

fn skip_f32(r: &mut PacketReader) -> Result<(), SlotError> {
    r.read_f32()?;
    Ok(())
}

fn skip_bool(r: &mut PacketReader) -> Result<(), SlotError> {
    r.read_bool()?;
    Ok(())
}

// PacketCodecs.INTEGER: 4 Byte, NICHT VarInt.
fn skip_i32(r: &mut PacketReader) -> Result<(), SlotError> {
    r.read_i32()?;
    Ok(())
}

// PacketCodecs.NBT_COMPOUND bzw. jeder Component ohne eigenen packetCodec (dann greift
// PacketCodecs.registryCodec(codec) -> NBT, s. ComponentType.Builder::build).
fn skip_nbt(r: &mut PacketReader) -> Result<(), SlotError> {
    nbt::read_root(r).map_err(|_| SlotError::Read)?;
    Ok(())
}

// Text-Components liegen im Netz als NBT (TextCodecs.REGISTRY_PACKET_CODEC).
fn read_text(r: &mut PacketReader) -> Result<Option<String>, SlotError> {
    let value = nbt::read_root(r).map_err(|_| SlotError::Read)?;
    Ok(value.map(|v| nbt::text_to_plain_string(&v)))
}

fn read_text_spans(r: &mut PacketReader) -> Result<Option<(String, Vec<nbt::TextSpan>)>, SlotError> {
    let value = nbt::read_root(r).map_err(|_| SlotError::Read)?;
    Ok(value.map(|v| (nbt::text_to_plain_string(&v), nbt::text_to_spans(&v))))
}

fn skip_text(r: &mut PacketReader) -> Result<(), SlotError> {
    read_text(r).map(|_| ())
}

// PacketCodecs.registryEntry OHNE Direct-Codec: schlichte VarInt-Registry-ID.
fn skip_registry_id(r: &mut PacketReader) -> Result<(), SlotError> {
    skip_varint(r)
}

// PacketCodecs.registryEntry MIT Direct-Codec: VarInt id+1, oder 0 und dann der Wert inline.
fn registry_entry<F>(r: &mut PacketReader, direct: F) -> Result<(), SlotError>
where
    F: FnMut(&mut PacketReader) -> Result<(), SlotError>,
{
    registry_entry_id(r, direct).map(|_| ())
}

// Dasselbe, liefert aber die Registry-ID (-1 fuer einen inline geschickten Wert).
fn registry_entry_id<F>(r: &mut PacketReader, mut direct: F) -> Result<i32, SlotError>
where
    F: FnMut(&mut PacketReader) -> Result<(), SlotError>,
{
    let id = r.read_varint()?;
    if id == 0 {
        direct(r)?;
    }
    Ok(id - 1)
}

// SoundEvent.PACKET_CODEC: Identifier + optionaler Fixed-Range-Float.
fn skip_sound_direct(r: &mut PacketReader) -> Result<(), SlotError> {
    skip_string(r)?;
    optional(r, skip_f32)
}

fn skip_sound_entry(r: &mut PacketReader) -> Result<(), SlotError> {
    registry_entry(r, skip_sound_direct)
}

// PacketCodecs.registryEntryList: VarInt n; n == 0 heisst "Tag" (dann folgt ein
// Identifier), sonst folgen n-1 Registry-Entries.
fn skip_registry_entry_list(r: &mut PacketReader) -> Result<(), SlotError> {
    let n = r.read_varint()?;
    if n == 0 {
        return skip_string(r);
    }
    for _ in 0..(n - 1) {
        skip_registry_id(r)?;
    }
    Ok(())
}

// LazyRegistryEntryReference: either(Entry, RegistryKey) -- bool, dann Entry oder Identifier.
fn skip_lazy_entry<F>(r: &mut PacketReader, direct: F) -> Result<(), SlotError>
where
    F: FnMut(&mut PacketReader) -> Result<(), SlotError>,
{
    if r.read_bool()? {
        registry_entry(r, direct)
    } else {
        skip_string(r)
    }
}

// StatusEffectInstance: Effekt-ID + Parameters (rekursiv ueber hiddenEffect).
fn skip_status_effect(r: &mut PacketReader) -> Result<(), SlotError> {
    skip_registry_id(r)?;
    skip_status_effect_parameters(r)
}

fn skip_status_effect_parameters(r: &mut PacketReader) -> Result<(), SlotError> {
    skip_varint(r)?; // amplifier
    skip_varint(r)?; // duration
    skip_bool(r)?; // ambient
    skip_bool(r)?; // showParticles
    skip_bool(r)?; // showIcon
    optional(r, skip_status_effect_parameters)
}

// BlockPredicate (can_place_on/can_break).
fn read_block_predicate(r: &mut PacketReader) -> Result<BlockPredicate, SlotError> {
    // blocks: registryEntryList, Groesse + 1, 0 heisst "Tag folgt".
    let blocks = if r.read_bool()? {
        let size = r.read_varint()? - 1;
        Some(if size == -1 {
            (Some(r.read_string()?), Vec::new())
        } else {
            if !(0..=65536).contains(&size) {
                return Err(SlotError::Read);
            }
            let mut ids = Vec::new();
            for _ in 0..size {
                ids.push(r.read_varint()?);
            }
            (None, ids)
        })
    } else {
        None
    };
    let mut properties = Vec::new();
    if r.read_bool()? {
        let n = r.read_varint()?;
        if !(0..=256).contains(&n) {
            return Err(SlotError::Read);
        }
        for _ in 0..n {
            let name = r.read_string()?;
            properties.push((name, read_state_value_matcher(r)?));
        }
    }
    let mut needs_block_entity = false;
    if r.read_bool()? {
        skip_nbt(r)?; // NbtPredicate
        needs_block_entity = true;
    }
    needs_block_entity |= read_components_predicate(r)?;
    Ok(BlockPredicate { blocks, properties, needs_block_entity })
}

// StatePredicate.ValueMatcher: Exact (0) = ein String, Ranged (1) = zwei optionale Strings.
fn read_state_value_matcher(r: &mut PacketReader) -> Result<StateValueMatcher, SlotError> {
    match r.read_varint()? {
        0 => Ok(StateValueMatcher::Exact(r.read_string()?)),
        1 => {
            let min = if r.read_bool()? { Some(r.read_string()?) } else { None };
            let max = if r.read_bool()? { Some(r.read_string()?) } else { None };
            Ok(StateValueMatcher::Range(min, max))
        }
        other => Err(SlotError::UnsupportedComponent(other)),
    }
}

// DataComponentMatchers: exakte Component-Map + partielle Praedikate. Die exakten Werte sind
// gewoehnliche Component-Payloads und lassen sich mit der Tabelle unten ueberlesen. Die
// partiellen haben je Praedikat-Typ ein eigenes Format, das wir nicht kennen -- dort bricht es
// bewusst ab statt sich zu verlesen. Zurueck: ob ueberhaupt eine Bedingung dastand.
fn read_components_predicate(r: &mut PacketReader) -> Result<bool, SlotError> {
    let exact = r.read_varint()?;
    if !(0..=256).contains(&exact) {
        return Err(SlotError::Read);
    }
    let mut ablage = ItemStack::new(0, 1);
    for _ in 0..exact {
        let type_id = r.read_varint()?;
        read_component_payload(r, type_id, &mut ablage)?;
    }
    let partial = r.read_varint()?;
    if partial != 0 {
        return Err(SlotError::UnsupportedComponent(-2));
    }
    Ok(exact != 0)
}

// --- Die Component-Tabelle ------------------------------------------------------------

// IDs = Reihenfolge der Registry "minecraft:data_component_type" in 1.21.11 (verifiziert
// gegen registries.json des Datengenerators). Bei einem Versionswechsel wandern die IDs --
// dann diese Tabelle neu gegen den Report pruefen.
fn read_component_payload(r: &mut PacketReader, type_id: i32, stack: &mut ItemStack) -> Result<(), SlotError> {
    match type_id {
        0 => skip_nbt(r),    // custom_data
        1 => {
            stack.max_stack_size = Some(r.read_varint()?);
            Ok(())
        }
        2 => {
            stack.max_damage = Some(r.read_varint()?);
            Ok(())
        }
        3 => {
            stack.damage = Some(r.read_varint()?);
            Ok(())
        }
        4 => Ok(()), // unbreakable (Unit -- 0 Byte)
        5 => {
            stack.use_effects = Some(UseEffects { can_sprint: r.read_bool()?, interact_vibrations: r.read_bool()?, speed_multiplier: r.read_f32()? });
            Ok(())
        }
        6 => {
            if let Some((text, spans)) = read_text_spans(r)? {
                stack.custom_name = Some(text);
                stack.custom_name_spans = Some(spans);
            }
            Ok(())
        }
        7 => {
            stack.minimum_attack_charge = Some(r.read_f32()?);
            Ok(())
        }
        8 => skip_lazy_entry(r, |_| Err(SlotError::UnsupportedComponent(8))), // damage_type
        9 => {
            // item_name -- nur nutzen, wenn kein custom_name gesetzt ist
            if let Some((text, spans)) = read_text_spans(r)? {
                if stack.custom_name.is_none() {
                    stack.custom_name = Some(text);
                }
                stack.item_name_spans = Some(spans);
            }
            Ok(())
        }
        10 => {
            stack.item_model = Some(r.read_string()?);
            Ok(())
        }
        11 => {
            // lore
            let mut zeilen = Vec::new();
            list(r, |r| {
                zeilen.push(read_text_spans(r)?.map(|(_, s)| s).unwrap_or_default());
                Ok(())
            })?;
            stack.lore = zeilen;
            Ok(())
        }
        12 => {
            stack.rarity = Some(r.read_varint()?.clamp(0, 3) as u8);
            Ok(())
        }
        13 | 41 => {
            // enchantments / stored_enchantments: Map Enchantment-ID -> Level
            // (ItemEnchantmentsComponent.PACKET_CODEC: VarInt-Anzahl, je Eintrag die nackte
            // Registry-ID und die Stufe als VarInt).
            let mut liste = Vec::new();
            list(r, |r| {
                let id = r.read_varint()?;
                let level = r.read_varint()?;
                liste.push(Enchantment { id: id.max(0) as u32, level });
                Ok(())
            })?;
            if type_id == 13 {
                stack.enchantments = liste;
            } else {
                stack.stored_enchantments = liste;
            }
            Ok(())
        }
        COMPONENT_CAN_PLACE_ON | COMPONENT_CAN_BREAK => {
            let n = r.read_varint()?;
            if !(0..=256).contains(&n) {
                return Err(SlotError::Read);
            }
            let mut preds = Vec::new();
            for _ in 0..n {
                preds.push(read_block_predicate(r)?);
            }
            if type_id == COMPONENT_CAN_PLACE_ON {
                stack.can_place_on = Some(preds);
            } else {
                stack.can_break = Some(preds);
            }
            Ok(())
        }
        16 => {
            // attribute_modifiers
            let mut liste = Vec::new();
            list(r, |r| {
                let attribute = r.read_varint()?;
                let id = r.read_string()?;
                let amount = r.read_f64()?;
                let operation = r.read_varint()?;
                let slot = r.read_varint()?;
                // Display: 0 = default, 1 = hidden, 2 = Override mit Text
                let hidden = match r.read_varint()? {
                    0 => false,
                    1 => true,
                    2 => {
                        skip_text(r)?;
                        false
                    }
                    other => return Err(SlotError::UnsupportedComponent(other)),
                };
                liste.push(ItemAttributeModifier { attribute, id, amount, operation, slot, hidden });
                Ok(())
            })?;
            stack.attribute_modifiers = Some(liste);
            Ok(())
        }
        17 => {
            // custom_model_data
            let mut cmd = CustomModelData::default();
            list(r, |r| {
                cmd.floats.push(r.read_f32()?);
                Ok(())
            })?;
            list(r, |r| {
                cmd.flags.push(r.read_bool()?);
                Ok(())
            })?;
            list(r, |r| {
                cmd.strings.push(r.read_string()?);
                Ok(())
            })?;
            list(r, |r| {
                cmd.colors.push(r.read_i32()?);
                Ok(())
            })?;
            stack.custom_model_data = Some(cmd);
            Ok(())
        }
        18 => {
            // tooltip_display
            stack.tooltip_hidden = r.read_bool()?;
            let mut ids = Vec::new();
            list(r, |r| {
                ids.push(r.read_varint()?);
                Ok(())
            })?;
            stack.tooltip_hidden_components = ids;
            Ok(())
        }
        19 => skip_varint(r), // repair_cost
        20 => Ok(()),         // creative_slot_lock (Unit)
        21 => {
            // enchantment_glint_override
            stack.glint_override = Some(r.read_bool()?);
            Ok(())
        }
        22 => skip_nbt(r),    // intangible_projectile (Unit ohne packetCodec -> NBT)
        COMPONENT_FOOD => {
            stack.food_nutrition = Some(r.read_varint()?);
            skip_f32(r)?; // saturation
            stack.food_always_edible = Some(r.read_bool()?);
            Ok(())
        }
        COMPONENT_CONSUMABLE => {
            stack.consume_seconds = Some(r.read_f32()?);
            stack.consume_animation = Some(r.read_varint()?); // ItemUseAnimation
            skip_sound_entry(r)?;
            stack.consume_particles = Some(r.read_bool()?);
            list(r, skip_consume_effect)
        }
        25 => read_slot(r), // use_remainder
        COMPONENT_USE_COOLDOWN => {
            let seconds = r.read_f32()?;
            let group = if r.read_bool()? { Some(r.read_string()?) } else { None };
            stack.use_cooldown = Some(UseCooldown { seconds, group });
            Ok(())
        }
        27 => skip_string(r), // damage_resistant (TagKey)
        COMPONENT_TOOL => {
            let n = r.read_varint()?;
            if !(0..=1024).contains(&n) {
                return Err(SlotError::Read);
            }
            let mut rules = Vec::new();
            for _ in 0..n {
                // registryEntryList: Groesse + 1, 0 heisst "Tag folgt".
                let size = r.read_varint()? - 1;
                let (tag, ids) = if size == -1 {
                    (Some(r.read_string()?), Vec::new())
                } else {
                    if !(0..=65536).contains(&size) {
                        return Err(SlotError::Read);
                    }
                    let mut ids = Vec::new();
                    for _ in 0..size {
                        ids.push(r.read_varint()?);
                    }
                    (None, ids)
                };
                let speed = if r.read_bool()? { Some(r.read_f32()?) } else { None };
                let correct_for_drops = if r.read_bool()? { Some(r.read_bool()?) } else { None };
                rules.push(ToolRuleData { tag, ids, speed, correct_for_drops });
            }
            let default_speed = r.read_f32()?;
            let _damage_per_block = r.read_varint()?;
            let can_destroy_blocks_in_creative = r.read_bool()?;
            stack.tool = Some(ToolData { rules, default_speed, can_destroy_blocks_in_creative });
            Ok(())
        }
        29 => {
            stack.weapon = Some(Weapon { item_damage_per_attack: r.read_varint()?, disable_blocking_for_seconds: r.read_f32()? });
            Ok(())
        }
        COMPONENT_ATTACK_RANGE => {
            stack.attack_range = Some(AttackRange {
                min_reach: r.read_f32()?,
                max_reach: r.read_f32()?,
                min_creative_reach: r.read_f32()?,
                max_creative_reach: r.read_f32()?,
                hitbox_margin: r.read_f32()?,
                mob_factor: r.read_f32()?,
            });
            Ok(())
        }
        31 => skip_varint(r), // enchantable
        COMPONENT_EQUIPPABLE => {
            // Reihenfolge wie EquippableComponent.PACKET_CODEC. Der Slot steht als Netz-ID von
            // EquipmentSlot da (Haupthand 0, Fuesse 1, Beine 2, Brust 3, Kopf 4, Zweithand 5,
            // Koerper 6, Sattel 7) -- NICHT als Ordinalzahl wie im Equipment-Paket. Gespeichert
            // wird die Ordinalzahl, damit beide Quellen gleich zaehlen.
            let slot = match r.read_varint()? {
                0 => 0,
                5 => 1,
                id @ 1..=4 => id as u8 + 1,
                id => id.clamp(6, 7) as u8,
            };
            skip_sound_entry(r)?; // equipSound
            let mut asset_id = None;
            optional(r, |r| {
                asset_id = Some(r.read_string()?);
                Ok(())
            })?;
            let mut camera_overlay = None;
            optional(r, |r| {
                camera_overlay = Some(r.read_string()?);
                Ok(())
            })?;
            let mut restricted = false;
            optional(r, |r| {
                restricted = true;
                skip_registry_entry_list(r)
            })?; // allowedEntities
            skip_bool(r)?; // dispensable
            let swappable = r.read_bool()?;
            for _ in 0..3 {
                skip_bool(r)?; // damageOnHurt, equipOnInteract, canBeSheared
            }
            skip_sound_entry(r)?; // shearingSound
            stack.equippable = Some(Equippable { slot, asset_id, camera_overlay, swappable, restricted });
            Ok(())
        }
        33 => skip_registry_entry_list(r),  // repairable
        34 => Ok(()),                       // glider (Unit)
        35 => skip_string(r),               // tooltip_style
        36 => list(r, skip_consume_effect), // death_protection
        37 => {
            // blocks_attacks
            stack.blocks_attacks = Some(BlocksAttacks { block_delay_seconds: r.read_f32()?, disable_cooldown_scale: r.read_f32()? });
            list(r, |r| {
                skip_f32(r)?;
                optional(r, skip_registry_entry_list)?;
                skip_f32(r)?;
                skip_f32(r)
            })?;
            // ItemDamage
            skip_f32(r)?;
            skip_f32(r)?;
            skip_f32(r)?;
            optional(r, skip_string)?; // bypassedBy (TagKey)
            optional(r, skip_sound_entry)?;
            optional(r, skip_sound_entry)
        }
        38 => {
            stack.piercing_weapon = Some(PiercingWeapon { deals_knockback: r.read_bool()?, dismounts: r.read_bool()? });
            optional(r, skip_sound_entry)?;
            optional(r, skip_sound_entry)
        }
        39 => {
            let contact_cooldown_ticks = r.read_varint()?;
            let delay_ticks = r.read_varint()?;
            let mut bedingungen = [None; 3];
            for b in &mut bedingungen {
                optional(r, |r| {
                    *b = Some(KineticCondition { max_duration_ticks: r.read_varint()?, min_speed: r.read_f32()?, min_relative_speed: r.read_f32()? });
                    Ok(())
                })?;
            }
            let forward_movement = r.read_f32()?;
            let damage_multiplier = r.read_f32()?;
            optional(r, skip_sound_entry)?;
            optional(r, skip_sound_entry)?;
            let [dismount, knockback, damage] = bedingungen;
            stack.kinetic_weapon = Some(KineticWeapon { contact_cooldown_ticks, delay_ticks, dismount, knockback, damage, forward_movement, damage_multiplier });
            Ok(())
        }
        40 => {
            stack.swing_animation = Some(SwingAnimation { kind: r.read_varint()?, duration: r.read_varint()? });
            Ok(())
        }
        42 => {
            stack.dyed_color = Some(r.read_i32()? as u32 & 0xff_ffff);
            Ok(())
        }
        43 => skip_i32(r),                    // map_color
        44 => {
            stack.map_id = Some(r.read_varint()?);
            Ok(())
        }
        45 => skip_nbt(r),                    // map_decorations
        46 => skip_varint(r),                 // map_post_processing
        47 => {
            // charged_projectiles
            let len = r.read_varint()?;
            if len < 0 {
                return Err(SlotError::Read);
            }
            stack.charged_projectiles.clear();
            for _ in 0..len {
                if let Some(geschoss) = read_optional_slot(r)? {
                    stack.charged_projectiles.push(geschoss.item_id);
                }
            }
            Ok(())
        }
        48 => {
            let mut inhalt = Vec::new();
            list(r, |r| {
                inhalt.extend(read_optional_slot(r)?);
                Ok(())
            })?;
            stack.bundle_contents = Some(inhalt);
            Ok(())
        }
        49 => {
            // potion_contents
            let potion = if r.read_bool()? { Some(r.read_varint()? as u32) } else { None };
            let custom_color = if r.read_bool()? { Some(r.read_i32()?) } else { None };
            let len = r.read_varint()?;
            if len < 0 {
                return Err(SlotError::Read);
            }
            let mut custom_effects = Vec::new();
            for _ in 0..len {
                let effect = r.read_varint()? as u32;
                let amplifier = r.read_varint()?;
                let duration = r.read_varint()?;
                skip_bool(r)?; // ambient
                let visible = r.read_bool()?;
                skip_bool(r)?; // showIcon
                optional(r, skip_status_effect_parameters)?; // hiddenEffect
                custom_effects.push(PotionEffect { effect, amplifier, duration, visible });
            }
            let custom_name = if r.read_bool()? { Some(r.read_string()?) } else { None };
            stack.potion = Some(PotionContents { potion, custom_color, custom_effects, custom_name });
            Ok(())
        }
        50 => {
            stack.potion_duration_scale = Some(r.read_f32()?);
            Ok(())
        }
        51 => list(r, |r| {
            // suspicious_stew_effects
            skip_registry_id(r)?;
            skip_varint(r)
        }),
        52 => {
            // writable_book_content: je Seite der Rohtext (und optional eine gefilterte Fassung).
            let mut seiten = Vec::new();
            list(r, |r| {
                seiten.push(r.read_string()?);
                optional(r, skip_string)
            })?;
            stack.writable_pages = Some(seiten);
            Ok(())
        }
        53 => {
            // written_book_content
            let title = r.read_string()?;
            optional(r, skip_string)?;
            let author = r.read_string()?;
            let generation = r.read_varint()?;
            let mut pages = Vec::new();
            list(r, |r| {
                pages.push(read_text_spans(r)?.map(|(_, s)| s).unwrap_or_default());
                optional(r, skip_text)
            })?;
            let resolved = r.read_bool()?;
            stack.written_book = Some(WrittenBook { title, author, generation, pages, resolved });
            Ok(())
        }
        54 => {
            // trim: Material-Entry + Pattern-Entry
            let material = registry_entry_id(r, skip_trim_material_direct)?;
            let muster = registry_entry_id(r, |r| {
                skip_string(r)?; // assetId
                skip_text(r)?; // description
                skip_bool(r) // decal
            })?;
            stack.trim = Some((material, muster));
            Ok(())
        }
        55 => skip_nbt(r), // debug_stick_state
        56 | 58 => {
            // entity_data / block_entity_data: Typ-ID + NBT
            skip_varint(r)?;
            skip_nbt(r)
        }
        57 => skip_nbt(r),                                                      // bucket_entity_data
        59 => skip_lazy_entry(r, |_| Err(SlotError::UnsupportedComponent(59))), // instrument
        60 => skip_lazy_entry(r, |_| Err(SlotError::UnsupportedComponent(60))), // provides_trim_material
        61 => skip_varint(r),                                                   // ominous_bottle_amplifier
        62 => skip_lazy_entry(r, |_| Err(SlotError::UnsupportedComponent(62))), // jukebox_playable
        63 => skip_string(r),                                                   // provides_banner_patterns (TagKey)
        64 => skip_nbt(r),                                                      // recipes
        65 => {
            // lodestone_tracker: optionale GlobalPos (Dimension + gepackte BlockPos), Flag
            optional(r, |r| {
                skip_string(r)?;
                r.read_i64()?;
                Ok(())
            })?;
            skip_bool(r)
        }
        66 => skip_firework_explosion(r),
        67 => {
            // fireworks
            skip_varint(r)?;
            list(r, skip_firework_explosion)
        }
        68 => {
            // profile: either(GameProfile, Data) + SkinOverride
            let mut profil = HeadProfile::default();
            if r.read_bool()? {
                profil.uuid = Some(r.read_uuid()?.0);
                profil.name = Some(r.read_string()?);
            } else {
                optional(r, |r| {
                    profil.name = Some(r.read_string()?);
                    Ok(())
                })?;
                optional(r, |r| {
                    profil.uuid = Some(r.read_uuid()?.0);
                    Ok(())
                })?;
            }
            list(r, |r| {
                let name = r.read_string()?;
                let wert = r.read_string()?;
                optional(r, skip_string)?;
                if name == "textures" {
                    profil.textures = Some(wert);
                }
                Ok(())
            })?;
            stack.profile = Some(profil);
            // SkinOverride: drei optionale Texture-Assets + optionales Modell
            for _ in 0..3 {
                optional(r, skip_string)?;
            }
            optional(r, skip_varint)
        }
        69 => skip_string(r), // note_block_sound
        70 => {
            // banner_patterns
            let mut lagen = Vec::new();
            list(r, |r| {
                let id = registry_entry_id(r, |r| {
                    skip_string(r)?; // assetId
                    skip_string(r) // translationKey
                })?;
                let farbe = r.read_varint()?.clamp(0, 15) as u8;
                lagen.push((id, farbe));
                Ok(())
            })?;
            stack.banner_patterns = lagen;
            Ok(())
        }
        71 => {
            stack.base_color = Some(r.read_varint()?.clamp(0, 15) as u8);
            Ok(())
        }
        72 => list(r, skip_registry_id), // pot_decorations
        73 => list(r, |r| {
            // container: Liste OPTIONALER Stacks
            read_optional_slot(r).map(|_| ())
        }),
        74 => list(r, |r| {
            // block_state: Map Property -> Wert
            skip_string(r)?;
            skip_string(r)
        }),
        75 => {
            // bees
            let mut n = 0;
            list(r, |r| {
                skip_varint(r)?; // EntityType
                skip_nbt(r)?;
                skip_varint(r)?;
                n += 1;
                skip_varint(r)
            })?;
            stack.bees = Some(n);
            Ok(())
        }
        76 => skip_nbt(r),         // lock
        77 => skip_nbt(r),         // container_loot
        78 => skip_sound_entry(r), // break_sound
        // 79..=103: Mob-/Farb-Varianten (Reihenfolge und Codecs aus der 1.21.11 nachgeprueft):
        //   - die meisten: ein Enum-Index oder eine reine Registry-ID (holderRegistry) -- ein
        //     VarInt, 0 ist dabei ein gueltiger Wert (die erste Variante);
        //   - painting/variant (97): Holder mit Inline-Moeglichkeit, 0 = Inline-Wert folgt (nur
        //     mit Datapack-eigenen Gemaelden; dann sauber abbrechen statt verlesen);
        //   - chicken/variant (93), zombie_nautilus/variant (94): EitherHolder -- ein Flag, dann
        //     die Registry-ID oder der Registry-Schluessel als Text (so stehen die Eier im Jar).
        79..=103 => match type_id {
            93 | 94 => {
                if r.read_bool()? {
                    skip_varint(r)
                } else {
                    skip_string(r)
                }
            }
            97 => {
                if r.read_varint()? == 0 {
                    return Err(SlotError::UnsupportedComponent(type_id));
                }
                Ok(())
            }
            _ => skip_varint(r),
        },
        other => Err(SlotError::UnsupportedComponent(other)),
    }
}

// ArmorTrimMaterial direkt (nur bei Datapack-Materialien im Stream): Assets + Beschreibung.
fn skip_trim_material_direct(r: &mut PacketReader) -> Result<(), SlotError> {
    skip_string(r)?; // base asset suffix
    list(r, |r| {
        skip_string(r)?; // EquipmentAsset-Key
        skip_string(r) // Suffix
    })?;
    skip_text(r)
}

fn skip_firework_explosion(r: &mut PacketReader) -> Result<(), SlotError> {
    skip_varint(r)?; // Form
    list(r, skip_i32)?; // Farben
    list(r, skip_i32)?; // Verlaufsfarben
    skip_bool(r)?;
    skip_bool(r)
}

// GameProfile-PropertyMap: Anzahl, dann je Name/Wert und optionale Signatur.
// ConsumeEffect: dispatch ueber die Registry CONSUME_EFFECT_TYPE (Reihenfolge der
// register()-Aufrufe in ConsumeEffect.Type, Codecs aus dem 1.21.11-Jar):
//   0 apply_effects      Liste StatusEffectInstance + float probability
//   1 remove_effects     registryEntryList(STATUS_EFFECT)
//   2 clear_all_effects  nichts (unit)
//   3 teleport_randomly  float diameter
//   4 play_sound         SoundEvent-Entry
// ECHTER BUG gewesen: hier stand nur ein Abbruch ("bei Vanilla-Items ist die Liste leer").
// Server-eigene Essen haben aber genau solche Effekte -- ein Golden Head (Kopf mit
// Regeneration/Absorption) brach die ganze Slot-Liste ab, der Container zeigte die falsche
// Reihenzahl und alles dahinter fehlte (etwa im Queue-Menue von mcpvp.club).
fn skip_consume_effect(r: &mut PacketReader) -> Result<(), SlotError> {
    match r.read_varint()? {
        0 => {
            list(r, skip_status_effect)?;
            skip_f32(r)
        }
        1 => skip_registry_entry_list(r),
        2 => Ok(()),
        3 => skip_f32(r),
        4 => skip_sound_entry(r),
        ty => Err(SlotError::UnsupportedComponent(1000 + ty)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::PacketWriter;

    fn reader(bytes: &[u8]) -> PacketReader<'_> {
        PacketReader::new(bytes)
    }

    // can_break: Stein (Roh-ID 1) und ein Tag mit Property-Bereich; ein exakter Component-
    // Wert am Blockobjekt wird ueberlesen und macht das Praedikat unauswertbar.
    #[test]
    fn can_break_wird_gelesen() {
        let mut w = PacketWriter::new();
        w.write_varint(1).write_varint(5).write_varint(1).write_varint(0);
        w.write_varint(COMPONENT_CAN_BREAK).write_varint(3);
        // 1: blocks = [1], keine Properties, kein NBT, keine Components.
        w.write_bool(true).write_varint(2).write_varint(1);
        w.write_bool(false).write_bool(false).write_varint(0).write_varint(0);
        // 2: Tag, age >= 7 als Bereich.
        w.write_bool(true).write_varint(0).write_string("minecraft:crops");
        w.write_bool(true).write_varint(1).write_string("age").write_varint(1).write_bool(true).write_string("7").write_bool(false);
        w.write_bool(false).write_varint(0).write_varint(0);
        // 3: jeder Block, aber mit exakter Component (max_stack_size 3).
        w.write_bool(false).write_bool(false).write_bool(false);
        w.write_varint(1).write_varint(1).write_varint(3).write_varint(0);
        let stack = read_optional_slot(&mut PacketReader::new(&w.into_bytes())).unwrap().unwrap();
        let p = stack.can_break.expect("can_break gelesen");
        assert_eq!(p.len(), 3);
        assert_eq!(p[0], BlockPredicate { blocks: Some((None, vec![1])), properties: vec![], needs_block_entity: false });
        assert_eq!(p[1].blocks, Some((Some("minecraft:crops".into()), vec![])));
        assert_eq!(p[1].properties, vec![("age".into(), StateValueMatcher::Range(Some("7".into()), None))]);
        assert!(p[2].needs_block_entity);
    }

    // Der unsichtbare Helm aus UHC Royale: equippable mit eigenem Equipment-Asset -- und der
    // naechste Slot muss danach noch lesbar sein.
    #[test]
    fn equippable_wird_gelesen() {
        let mut c = PacketWriter::new();
        c.write_varint(4); // Kopf (Netz-ID; gespeichert als Ordinalzahl 5)
        c.write_varint(1); // equipSound: Registry-ID 0
        c.write_bool(true).write_string("uhc:invisible_helmet");
        c.write_bool(true).write_string("minecraft:misc/pumpkinblur");
        c.write_bool(false); // allowedEntities
        c.write_bool(true).write_bool(true).write_bool(false).write_bool(true).write_bool(false);
        c.write_varint(1); // shearingSound
        let stack = stack_from_parts(800, 1, &[(COMPONENT_EQUIPPABLE, c.into_bytes())], &[]).unwrap();
        let e = stack.equippable.clone().expect("equippable");
        assert_eq!((e.slot, e.asset_id.as_deref(), e.camera_overlay.as_deref(), e.swappable), (5, Some("uhc:invisible_helmet"), Some("minecraft:misc/pumpkinblur"), true));
        assert!(!stack.equippable_removed());
        let ohne = stack_from_parts(800, 1, &[], &[COMPONENT_EQUIPPABLE]).unwrap();
        assert!(ohne.equippable.is_none() && ohne.equippable_removed());
    }

    // Ein Golden Head: essbar (consumable) mit apply_effects (Regeneration II, Absorption)
    // und einem Klang -- danach muss der NAECHSTE Slot noch lesbar sein.
    #[test]
    fn consumable_mit_effekten_wird_ganz_gelesen() {
        let mut w = PacketWriter::new();
        w.write_varint(1).write_varint(5); // count 1, Item-ID beliebig
        w.write_varint(1).write_varint(0); // 1 hinzugefuegt, 0 entfernt
        w.write_varint(24); // consumable
        w.write_f32(1.6).write_varint(1); // Dauer, UseAction eat
        w.write_varint(1); // Klang: Registry-ID 0 (+1)
        w.write_bool(true); // Partikel
        w.write_varint(2); // zwei Effekte
        w.write_varint(0).write_varint(1).write_varint(9).write_varint(100); // apply_effects, 1 Effekt, regeneration, Stufe 1 ...
        w.write_varint(100); // ... Dauer
        w.write_bool(false).write_bool(true).write_bool(true).write_bool(false); // ambient, particles, icon, kein hidden
        w.write_f32(1.0); // probability
        w.write_varint(4).write_varint(1); // play_sound, Registry-ID 0 (+1)
        w.write_varint(0); // naechster Slot: leer
        let bytes = w.into_bytes();
        let mut r = reader(&bytes);
        let stack = read_optional_slot(&mut r).unwrap().expect("Stapel");
        assert_eq!(stack.consume_seconds, Some(1.6));
        assert_eq!(read_optional_slot(&mut r).unwrap(), None, "danach muss die Liste weiterlesbar sein");
    }

    #[test]
    fn leerer_slot_ist_none() {
        let mut w = PacketWriter::new();
        w.write_varint(0);
        let bytes = w.into_bytes();
        assert_eq!(read_optional_slot(&mut reader(&bytes)).unwrap(), None);
    }

    #[test]
    fn stack_ohne_components() {
        let mut w = PacketWriter::new();
        w.write_varint(64).write_varint(1).write_varint(0).write_varint(0);
        let bytes = w.into_bytes();
        let stack = read_optional_slot(&mut reader(&bytes)).unwrap().unwrap();
        assert_eq!(stack, ItemStack::new(1, 64));
    }

    #[test]
    fn trankinhalt_wird_gelesen() {
        let mut w = PacketWriter::new();
        w.write_varint(1).write_varint(1).write_varint(1).write_varint(0);
        w.write_varint(49); // potion_contents
        w.write_bool(true).write_varint(15); // strong_swiftness (ohne +1)
        w.write_bool(true).write_i32(0x123456); // custom_color
        w.write_varint(1); // ein Zusatzeffekt: Staerke II, ohne Partikel ...
        w.write_varint(4).write_varint(1).write_varint(200);
        w.write_bool(false).write_bool(false).write_bool(true);
        w.write_bool(true); // ... mit verstecktem Effekt darunter
        w.write_varint(0).write_varint(100).write_bool(false).write_bool(true).write_bool(true).write_bool(false);
        w.write_bool(true).write_string("healing"); // custom_name
        let bytes = w.into_bytes();
        let mut r = reader(&bytes);
        let stack = read_optional_slot(&mut r).unwrap().unwrap();
        let p = stack.potion.expect("potion_contents");
        assert_eq!(p.potion, Some(15));
        assert_eq!(p.custom_color, Some(0x123456));
        assert_eq!(p.custom_effects, vec![PotionEffect { effect: 4, amplifier: 1, duration: 200, visible: false }]);
        assert_eq!(p.custom_name.as_deref(), Some("healing"));
        assert!(r.is_exhausted());
    }

    #[test]
    fn damage_und_verzauberung_werden_gelesen() {
        let mut w = PacketWriter::new();
        // Diamantschwert, 1x, mit damage=42 (Typ 3) und einer Verzauberung (Typ 13)
        w.write_varint(1).write_varint(936).write_varint(2).write_varint(0);
        w.write_varint(3).write_varint(42);
        w.write_varint(13).write_varint(1).write_varint(7).write_varint(5);
        let bytes = w.into_bytes();
        let mut r = reader(&bytes);
        let stack = read_optional_slot(&mut r).unwrap().unwrap();
        assert_eq!(stack.damage, Some(42));
        assert_eq!(stack.enchantments, vec![Enchantment { id: 7, level: 5 }]);
        assert!(stack.stored_enchantments.is_empty());
        // Alles verbraucht -- sonst wuerde sich der naechste Slot der Liste verlesen.
        assert!(r.is_exhausted());
    }

    #[test]
    fn unbekannter_component_meldet_seine_id() {
        let mut w = PacketWriter::new();
        w.write_varint(1).write_varint(1).write_varint(1).write_varint(0);
        w.write_varint(9999);
        let bytes = w.into_bytes();
        assert_eq!(read_optional_slot(&mut reader(&bytes)), Err(SlotError::UnsupportedComponent(9999)));
    }

    #[test]
    fn geschachtelte_stacks_im_container_component() {
        let mut w = PacketWriter::new();
        // Shulkerbox mit container-Component (Typ 73): zwei Slots, einer leer
        w.write_varint(1).write_varint(1).write_varint(1).write_varint(0);
        w.write_varint(73).write_varint(2);
        w.write_varint(5).write_varint(1).write_varint(0).write_varint(0);
        w.write_varint(0);
        // danach ein weiterer Stack in derselben Liste -- muss sauber ankommen
        w.write_varint(3).write_varint(4).write_varint(0).write_varint(0);
        let bytes = w.into_bytes();
        let mut r = reader(&bytes);
        let outer = read_optional_slot(&mut r).unwrap().unwrap();
        assert_eq!(outer.item_id, 1);
        let next = read_optional_slot(&mut r).unwrap().unwrap();
        assert_eq!(next, ItemStack::new(4, 3));
    }
}
