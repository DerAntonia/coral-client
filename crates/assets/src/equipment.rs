// Equipment-Modelle (assets/<ns>/equipment/<pfad>.json): welche Texturen ein getragenes Item
// je Ebene zeigt -- Vanillas EquipmentClientInfo. Die equippable-Component eines Items nennt
// das Asset; Server-Packs bringen eigene mit (UHC Royale: "uhc:invisible_helmet" ohne Ebenen,
// also ein unsichtbarer Helm).
//
// Aufbau:
//   { "layers": { "humanoid": [ { "texture": "ns:name", "dyeable": { "color_when_undyed": int },
//                                  "use_player_texture": bool }, ... ],
//                 "humanoid_leggings": [ ... ], "wings": [ ... ], ... } }
// Die Textur einer Ebene liegt unter assets/<ns>/textures/entity/equipment/<ebene>/<name>.png.
// Dieser Client zeichnet Ruestung nur am Menschenmodell und die Fluegel am Ruecken; die
// Ebenen fuer Tiere werden nicht gelesen.

use crate::MinecraftInstallation;

#[derive(Clone, Debug, PartialEq)]
pub struct EquipmentLayer {
    // Texturpfad ohne "textures/" und ".png", mit Namespace: "minecraft:entity/equipment/
    // humanoid/diamond".
    pub texture: String,
    // Faerbbar? Dann Some(Farbe ohne Faerbung, 0 = keine). Vanilla laesst eine faerbbare Ebene
    // ganz weg, wenn weder das Item gefaerbt ist noch eine Ersatzfarbe da ist.
    pub dyeable: Option<u32>,
    // Statt der eigenen Textur die des Spielers (Elytra-Bild aus dem Skin bzw. der Umhang),
    // sofern er eine hat -- so tragen Umhang-Besitzer ihn auf den Fluegeln.
    pub use_player_texture: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EquipmentModel {
    pub humanoid: Vec<EquipmentLayer>,
    pub humanoid_leggings: Vec<EquipmentLayer>,
    // Fluegel am Ruecken (WingsLayer): Elytra und alles, dessen Asset diese Ebene hat.
    pub wings: Vec<EquipmentLayer>,
}

impl EquipmentModel {
    // Die Ebenen fuer die innere (Hose) bzw. aeussere Modellebene.
    pub fn layers(&self, inner: bool) -> &[EquipmentLayer] {
        if inner {
            &self.humanoid_leggings
        } else {
            &self.humanoid
        }
    }
}

pub fn parse_equipment_model(json: &[u8]) -> Option<EquipmentModel> {
    let wurzel: serde_json::Value = serde_json::from_slice(json).ok()?;
    let ebenen = wurzel.get("layers")?;
    let lies = |typ: &str| -> Vec<EquipmentLayer> {
        let Some(liste) = ebenen.get(typ).and_then(|l| l.as_array()) else { return Vec::new() };
        liste
            .iter()
            .filter_map(|e| {
                let id = e.get("texture")?.as_str()?;
                let (ns, pfad) = id.split_once(':').unwrap_or(("minecraft", id));
                let dyeable = e.get("dyeable").map(|d| d.get("color_when_undyed").and_then(|c| c.as_i64()).map_or(0, |c| c as u32 & 0xff_ffff));
                let use_player_texture = e.get("use_player_texture").and_then(|v| v.as_bool()).unwrap_or(false);
                Some(EquipmentLayer { texture: format!("{ns}:entity/equipment/{typ}/{pfad}"), dyeable, use_player_texture })
            })
            .collect()
    };
    Some(EquipmentModel { humanoid: lies("humanoid"), humanoid_leggings: lies("humanoid_leggings"), wings: lies("wings") })
}

impl MinecraftInstallation {
    // Das Equipment-Modell zu einem Asset ("ns:pfad"), aus Packs und Vanilla. None, wenn es
    // die Datei nicht gibt oder sie kaputt ist -- Vanilla zeigt dann ebenfalls nichts.
    pub fn load_equipment_model(&mut self, asset_id: &str) -> Option<EquipmentModel> {
        let (ns, pfad) = asset_id.split_once(':').unwrap_or(("minecraft", asset_id));
        let bytes = self.load_bytes(&format!("{ns}/equipment/{pfad}.json")).ok()?;
        parse_equipment_model(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leder_und_leeres_modell() {
        let leder = br#"{"layers":{"humanoid":[{"texture":"minecraft:leather","dyeable":{"color_when_undyed":-6265536}},{"texture":"minecraft:leather_overlay"}],
            "humanoid_leggings":[{"texture":"minecraft:leather","dyeable":{}}],"wings":[{"texture":"x"}]}}"#;
        let m = parse_equipment_model(leder).unwrap();
        assert_eq!(m.humanoid.len(), 2);
        assert_eq!(m.humanoid[0].texture, "minecraft:entity/equipment/humanoid/leather");
        assert_eq!(m.humanoid[0].dyeable, Some(0xA06540));
        assert_eq!(m.humanoid[1].dyeable, None);
        assert_eq!(m.layers(true)[0].texture, "minecraft:entity/equipment/humanoid_leggings/leather");
        assert_eq!(m.layers(true)[0].dyeable, Some(0), "faerbbar ohne Ersatzfarbe");
        // UHC Royale: unsichtbarer Helm.
        let leer = parse_equipment_model(br#"{"layers":{}}"#).unwrap();
        assert!(leer.layers(false).is_empty() && leer.layers(true).is_empty());
        let eigen = parse_equipment_model(br#"{"layers":{"humanoid":[{"texture":"uhc:invisible"}]}}"#).unwrap();
        assert_eq!(eigen.humanoid[0].texture, "uhc:entity/equipment/humanoid/invisible");
    }

    // Vanillas elytra.json: eine Fluegel-Ebene, die das Spielerbild bevorzugt.
    #[test]
    fn fluegel_ebene() {
        let m = parse_equipment_model(br#"{"layers":{"wings":[{"texture":"minecraft:elytra","use_player_texture":true}]}}"#).unwrap();
        assert_eq!(m.wings.len(), 1);
        assert_eq!(m.wings[0].texture, "minecraft:entity/equipment/wings/elytra");
        assert!(m.wings[0].use_player_texture);
        assert!(m.humanoid.is_empty());
    }
}
