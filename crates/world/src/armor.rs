// Getragene Ruestung: welches Item welche Modellebene und welche Textur bekommt.
//
// Vanilla loest das ueber die EQUIPPABLE-Datenkomponente des Items. Sie nennt einen
// `assetId`, der auf assets/minecraft/equipment/<asset>.json zeigt, und das listet je
// "Layer-Typ" (humanoid, humanoid_leggings, horse_body, ...) die Texturen auf.
// ArmorFeatureRenderer.renderArmor nimmt fuer die Beine (usesInnerModel) den Typ
// HUMANOID_LEGGINGS, sonst HUMANOID, und zeichnet damit das Asset der Komponente.
//
// Die Komponente schickt der Server nur, wenn sie vom Item-Standard ABWEICHT (s. Equip). Den
// Standard kennt dieser Client nicht aus den Item-Daten -- dafuer die feste Tabelle unten:
// welches Vanilla-Item an welchem Slot welches Equipment-Asset traegt. Welche Texturen ein
// Asset zeigt, steht dann in dessen Equipment-Datei (assets::EquipmentModel), wie in Vanilla
// auch fuer die Standard-Ruestung -- ein Pack kann sie damit umfaerben oder ausblenden.

// Vanillas EquipmentSlot, beschraenkt auf die vier, die ein Biped am Koerper traegt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmorSlot {
    Feet,
    Legs,
    Chest,
    Head,
}

impl ArmorSlot {
    // Set Equipment nennt EquipmentSlot.ordinal() (s. packets::EQUIPMENT_SLOT_MAINHAND fuer
    // die vollstaendige Reihenfolge): MAINHAND, OFFHAND, FEET, LEGS, CHEST, HEAD, BODY,
    // SADDLE. BODY/SADDLE sind Tier-Slots und haben am Biped nichts zu suchen.
    pub fn from_equipment_slot(slot: u8) -> Option<Self> {
        match slot {
            2 => Some(Self::Feet),
            3 => Some(Self::Legs),
            4 => Some(Self::Chest),
            5 => Some(Self::Head),
            _ => None,
        }
    }

    // ArmorFeatureRenderer.usesInnerModel: NUR die Hose nimmt das innere Modell. Der
    // Unterschied ist die Aufblaehung (0.5 statt 1.0 Pixel) -- ohne sie steckten Brustpanzer
    // und Hose an der Huefte ineinander und flimmerten gegeneinander.
    pub fn inner(self) -> bool {
        matches!(self, Self::Legs)
    }

    // Zeichenreihenfolge aus ArmorFeatureRenderer.render -- Brust, Hose, Schuhe, Kopf.
    pub const ALL: [ArmorSlot; 4] = [Self::Chest, Self::Legs, Self::Feet, Self::Head];

    // Nur fuer Tabellen/Tests: ein fortlaufender Index.
    pub fn index(self) -> usize {
        match self {
            Self::Feet => 0,
            Self::Legs => 1,
            Self::Chest => 2,
            Self::Head => 3,
        }
    }
}

// Eine Ruestungsebene: Grundtextur plus die optionale zweite, NIE gefaerbte darueber.
// Letztere hat nur Leder -- leather.json listet unter jedem Layer-Typ zwei Eintraege, den
// gefaerbten Grund und "leather_overlay" ohne `dyeable`. Das ist der Grund, warum gefaerbte
// Lederruestung ihre Beschlaege in Naturfarbe behaelt.
pub struct Layer {
    pub base: &'static str,
    pub overlay: Option<&'static str>,
}

// Ein Ruestungsmaterial mit seinen beiden Modellebenen.
pub struct Material {
    // Der Itemname OHNE das Teil ("iron_helmet" -> "iron"). Nicht immer gleich dem
    // Asset-Namen: "golden_*" traegt das Asset "gold", "turtle_helmet" das Asset
    // "turtle_scute".
    pub prefix: &'static str,
    // Das Equipment-Asset (assets/minecraft/equipment/<asset>.json).
    pub asset: &'static str,
    // Aeussere Ebene: Helm, Brustpanzer, Schuhe.
    pub humanoid: Layer,
    // Innere Ebene: die Hose. None fuer Materialien, die es NUR als Helm gibt -- fuer die
    // existiert unter textures/entity/equipment/humanoid_leggings/ gar keine Datei, und ein
    // erfundener Pfad landete als fehlende Kachel im Atlas.
    pub leggings: Option<Layer>,
    // Traegt die GRUNDebene eine Faerbung? Nur Leder (`dyeable` in leather.json). Ohne
    // Faerbung waere die Grundtextur fast weiss -- sie ist eine Graustufenvorlage.
    pub dyeable: bool,
}

// Vanillas Farbe fuer UNGEFAERBTES Leder: leather.json nennt "color_when_undyed": -6265536,
// also 0xFFA06540.
pub const UNDYED_LEATHER: [f32; 3] = [0xA0 as f32 / 255.0, 0x65 as f32 / 255.0, 0x40 as f32 / 255.0];

macro_rules! einfach {
    ($prefix:literal, $asset:literal) => {
        Material {
            prefix: $prefix,
            asset: $asset,
            humanoid: Layer { base: concat!("entity/equipment/humanoid/", $asset), overlay: None },
            leggings: Some(Layer { base: concat!("entity/equipment/humanoid_leggings/", $asset), overlay: None }),
            dyeable: false,
        }
    };
}

pub const MATERIALS: [Material; 8] = [
    Material {
        prefix: "leather",
        asset: "leather",
        humanoid: Layer { base: "entity/equipment/humanoid/leather", overlay: Some("entity/equipment/humanoid/leather_overlay") },
        leggings: Some(Layer {
            base: "entity/equipment/humanoid_leggings/leather",
            overlay: Some("entity/equipment/humanoid_leggings/leather_overlay"),
        }),
        dyeable: true,
    },
    einfach!("chainmail", "chainmail"),
    einfach!("iron", "iron"),
    einfach!("golden", "gold"),
    einfach!("diamond", "diamond"),
    einfach!("netherite", "netherite"),
    einfach!("copper", "copper"),
    // Der Schildkroetenpanzer ist die einzige Ruestung ohne vollen Satz: es gibt nur den Helm,
    // und entsprechend auch nur die aeussere Texturebene. Sein Asset heisst nach dem Material,
    // nicht nach dem Item.
    Material {
        prefix: "turtle",
        asset: "turtle_scute",
        humanoid: Layer { base: "entity/equipment/humanoid/turtle_scute", overlay: None },
        leggings: None,
        dyeable: false,
    },
];

// Alle Texturpfade, die in den Entity-Atlas muessen. Anders als Skins werden sie NICHT
// nachgeladen: es sind siebzehn kleine 64x32-Bilder, die beim Atlasbau feststehen, und ein
// Spieler in voller Ruestung wuerde sonst vier Nachladeplaetze auf einmal belegen.
pub fn all_textures() -> Vec<&'static str> {
    let mut out = Vec::with_capacity(MATERIALS.len() * 3);
    for m in &MATERIALS {
        for layer in [Some(&m.humanoid), m.leggings.as_ref()].into_iter().flatten() {
            out.push(layer.base);
            out.extend(layer.overlay);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

// Item -> (welcher Slot, welches Material). None fuer alles, was keine Vanilla-Ruestung ist.
pub fn armor_of(item: &str) -> Option<(ArmorSlot, &'static Material)> {
    let (slot, prefix) = if let Some(p) = item.strip_suffix("_helmet") {
        (ArmorSlot::Head, p)
    } else if let Some(p) = item.strip_suffix("_chestplate") {
        (ArmorSlot::Chest, p)
    } else if let Some(p) = item.strip_suffix("_leggings") {
        (ArmorSlot::Legs, p)
    } else if let Some(p) = item.strip_suffix("_boots") {
        (ArmorSlot::Feet, p)
    } else {
        return None;
    };
    let material = MATERIALS.iter().find(|m| m.prefix == prefix)?;
    // Schildkroetenpanzer gibt es NUR als Helm -- "turtle_boots" existiert nicht, aber ein
    // Server koennte den Namen erfinden, und dann saehe man Panzer-Schuhe.
    if material.prefix == "turtle" && slot != ArmorSlot::Head {
        return None;
    }
    Some((slot, material))
}

// Was ein Item getragen zeigt: die wirksame equippable-Component (s. item_components).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Equip {
    // Unbekannt, weil die Standard-Components nicht extrahiert sind und der Server nichts
    // gesetzt hat: die alte Tabelle (s. MATERIALS) entscheidet.
    Default,
    // Keine equippable-Component (entfernt, oder das Item hat keine): kein Ruestungsbild.
    Removed,
    // An welchem Slot (EquipmentSlot-Ordinal) und welches Asset (Nummer aus asset_index; None =
    // getragen, aber unsichtbar).
    Set { slot: u8, asset: Option<u32> },
}

impl Equip {
    pub fn from_stack(s: &protocol::item::ItemStack) -> Self {
        match crate::item_components::Components::of(s).equippable() {
            Some(e) => Equip::Set { slot: e.slot, asset: e.asset_id.as_deref().map(asset_index) },
            None if s.equippable_removed() || crate::item_components::loaded() => Equip::Removed,
            None => Equip::Default,
        }
    }
}

static ASSETS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

// Die Nummer eines Equipment-Assets ("ns:pfad"), beim ersten Auftauchen vergeben -- damit
// HeldItem Copy bleibt (wie entity::custom_model_id).
pub fn asset_index(id: &str) -> u32 {
    let id = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    let mut liste = ASSETS.lock().unwrap_or_else(|e| e.into_inner());
    match liste.iter().position(|a| *a == id) {
        Some(i) => i as u32,
        None => {
            liste.push(id);
            liste.len() as u32 - 1
        }
    }
}

pub fn asset_name(index: u32) -> Option<String> {
    ASSETS.lock().unwrap_or_else(|e| e.into_inner()).get(index as usize).cloned()
}

// Welches Equipment-Asset ein Item an diesem Slot zeigt (Vanillas
// HumanoidArmorLayer.shouldRender: die Component nennt genau diesen Slot und ein Asset).
// None = an diesem Slot nichts.
pub fn worn_asset(item: &str, equip: Equip, slot: ArmorSlot) -> Option<String> {
    match equip {
        Equip::Default => {
            let (item_slot, material) = armor_of(item)?;
            (item_slot == slot).then(|| format!("minecraft:{}", material.asset))
        }
        Equip::Removed => None,
        Equip::Set { slot: s, asset } => {
            if ArmorSlot::from_equipment_slot(s) != Some(slot) {
                return None;
            }
            asset_name(asset?)
        }
    }
}

// Das Equipment-Asset fuer die Fluegel am Ruecken (WingsLayer): das der equippable-Component
// des Brust-Items, ohne Slot-Pruefung. Von Haus aus hat nur die Elytra eins.
pub fn wings_asset(item: &str, equip: Equip) -> Option<String> {
    match equip {
        Equip::Default => (item == "elytra").then(|| "minecraft:elytra".to_string()),
        Equip::Removed => None,
        Equip::Set { asset, .. } => asset_name(asset?),
    }
}

// Die Ebene dieses Materials fuer diesen Slot. Welche der beiden gespeicherten gilt, haengt
// allein daran, ob der Slot die INNERE Modellebene nutzt (s. ArmorSlot::inner). None, wenn es
// das Material fuer diese Ebene gar nicht gibt (Schildkroetenpanzer als Hose).
pub fn layer_for(material: &'static Material, slot: ArmorSlot) -> Option<&'static Layer> {
    if slot.inner() {
        material.leggings.as_ref()
    } else {
        Some(&material.humanoid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item_registry::all_item_names;

    // Jeder Tabelleneintrag muss zu ECHTEN Items passen -- ein Tippfehler im Praefix waere
    // sonst unsichtbar (die Ruestung bliebe einfach aus).
    #[test]
    fn jede_vanilla_ruestung_wird_erkannt() {
        let mut gefunden = 0;
        for name in all_item_names() {
            let name = name.as_str();
            let ist_ruestung = ["_helmet", "_chestplate", "_leggings", "_boots"].iter().any(|e| name.ends_with(e))
                // Pferde-/Wolfsruestung traegt kein Biped.
                && !name.contains("horse_armor")
                && name != "wolf_armor";
            if ist_ruestung {
                assert!(armor_of(name).is_some(), "{name} muss als Ruestung erkannt werden");
                gefunden += 1;
            }
        }
        // 7 volle Saetze a 4 Teile + der Schildkroetenhelm.
        assert_eq!(gefunden, 29, "die Registry hat andere Ruestungen als erwartet");
    }

    #[test]
    fn slots_und_materialien_stimmen() {
        let (slot, m) = armor_of("netherite_chestplate").unwrap();
        assert_eq!(slot, ArmorSlot::Chest);
        assert_eq!(m.humanoid.base, "entity/equipment/humanoid/netherite");
        // "golden_*" traegt das Asset "gold" -- die eine Stelle, an der Itemname und
        // Assetname auseinandergehen.
        let (_, gold) = armor_of("golden_boots").unwrap();
        assert_eq!(gold.humanoid.base, "entity/equipment/humanoid/gold");
        let (slot, turtle) = armor_of("turtle_helmet").unwrap();
        assert_eq!(slot, ArmorSlot::Head);
        assert_eq!(turtle.humanoid.base, "entity/equipment/humanoid/turtle_scute");
        assert!(turtle.leggings.is_none(), "den Schildkroetenpanzer gibt es nur als Helm");
        assert!(armor_of("turtle_boots").is_none(), "Panzerschuhe gibt es nicht");
    }

    // Die Hose ist die einzige innere Ebene -- daran haengt Dilation UND Texturwahl.
    #[test]
    fn nur_die_hose_ist_die_innere_ebene() {
        assert!(ArmorSlot::Legs.inner());
        for slot in [ArmorSlot::Head, ArmorSlot::Chest, ArmorSlot::Feet] {
            assert!(!slot.inner(), "{slot:?} darf nicht die innere Ebene sein");
        }
        let (_, leder) = armor_of("leather_leggings").unwrap();
        let hose = layer_for(leder, ArmorSlot::Legs).unwrap();
        assert_eq!(hose.base, "entity/equipment/humanoid_leggings/leather");
        assert_eq!(hose.overlay, Some("entity/equipment/humanoid_leggings/leather_overlay"));
        let panzer = layer_for(leder, ArmorSlot::Chest).unwrap();
        assert_eq!(panzer.base, "entity/equipment/humanoid/leather");
        // Und der Schildkroetenpanzer hat als Hose ueberhaupt keine Ebene -- fuer ihn
        // existiert unter humanoid_leggings/ gar keine Datei.
        let (_, turtle) = armor_of("turtle_helmet").unwrap();
        assert!(layer_for(turtle, ArmorSlot::Legs).is_none());
    }

    // Die Slot-Zuordnung kommt aus EquipmentSlot.ordinal() -- verwechselt man sie, landet
    // der Helm an den Fuessen.
    #[test]
    fn equipment_slots_treffen_die_ordinalwerte() {
        assert_eq!(ArmorSlot::from_equipment_slot(2), Some(ArmorSlot::Feet));
        assert_eq!(ArmorSlot::from_equipment_slot(3), Some(ArmorSlot::Legs));
        assert_eq!(ArmorSlot::from_equipment_slot(4), Some(ArmorSlot::Chest));
        assert_eq!(ArmorSlot::from_equipment_slot(5), Some(ArmorSlot::Head));
        // Haupthand/Nebenhand und die Tier-Slots gehoeren nicht dazu.
        for slot in [0, 1, 6, 7] {
            assert_eq!(ArmorSlot::from_equipment_slot(slot), None);
        }
    }

    // Der Standard aus der Tabelle, vom Server entfernt, auf ein eigenes Asset gesetzt oder
    // unsichtbar gemacht -- und ein Slot, der nicht passt.
    #[test]
    fn getragenes_asset_folgt_der_component() {
        assert_eq!(worn_asset("diamond_helmet", Equip::Default, ArmorSlot::Head).as_deref(), Some("minecraft:diamond"));
        assert_eq!(worn_asset("golden_boots", Equip::Default, ArmorSlot::Feet).as_deref(), Some("minecraft:gold"));
        assert_eq!(worn_asset("diamond_helmet", Equip::Default, ArmorSlot::Feet), None);
        assert_eq!(worn_asset("diamond_helmet", Equip::Removed, ArmorSlot::Head), None);
        let uhc = Equip::Set { slot: 5, asset: Some(asset_index("uhc:invisible_helmet")) };
        assert_eq!(worn_asset("diamond_helmet", uhc, ArmorSlot::Head).as_deref(), Some("uhc:invisible_helmet"));
        assert_eq!(worn_asset("diamond_helmet", Equip::Set { slot: 5, asset: None }, ArmorSlot::Head), None);
        // Ein Stock mit equippable am Kopf wird getragen gezeichnet.
        let hut = Equip::Set { slot: 5, asset: Some(asset_index("iron")) };
        assert_eq!(worn_asset("stick", hut, ArmorSlot::Head).as_deref(), Some("minecraft:iron"));
        assert_eq!(worn_asset("stick", hut, ArmorSlot::Chest), None);
    }

    #[test]
    fn alle_texturen_sind_eindeutig_und_vollstaendig() {
        let texturen = all_textures();
        // 7 volle Materialien a 2 Ebenen + Leder-Overlay 2x + der Schildkroetenhelm mit nur
        // einer Ebene = 17.
        assert_eq!(texturen.len(), 17, "{texturen:?}");
        let mut sortiert = texturen.clone();
        sortiert.dedup();
        assert_eq!(sortiert.len(), texturen.len(), "keine Dubletten");
    }
}
