// Abbau-Daten und Vanillas Abbauformel.
//
// mining_data.txt ist ein maschineller Dump aus dem ECHTEN 1.21.11-Jar (Bootstrap +
// Registries, nicht aus dem Quelltext abgeschrieben):
//   B <block> <haerte> <braucht_werkzeug>     -- in Registry-Reihenfolge, also Zeile = Roh-ID
//   I <item> <default_speed> <regeln> <creative_abbau>
//   R <#tag | block,block,...> <speed|-> <correct_for_drops 1|0|->   -- Regeln des Items davor
//   T <tag> <block,block,...>                  -- Block-Tags aus den Datendateien, aufgeloest
//
// Die Formel (AbstractBlock.calcBlockBreakingDelta / PlayerEntity.getBlockBreakingSpeed):
//
//   haerte == -1                 -> 0 (unzerstoerbar)
//   f = Werkzeug.getSpeed(block)   (erste Regel mit speed, deren Blockmenge passt; sonst default)
//   f > 1: f += mining_efficiency (Attribut, Effizienz-Verzauberung)
//   Eile/Conduit:  f *= 1 + (stufe + 1) * 0.2
//   Abbaulaehmung: f *= 0.3 / 0.09 / 0.0027 / 0.00081
//   f *= block_break_speed (Attribut)
//   Kopf unter Wasser: f *= submerged_mining_speed (Attribut, 0.2)
//   nicht am Boden:    f /= 5
//   delta = f / haerte / (kannErnten ? 30 : 100)
//   kannErnten = !braucht_werkzeug || Werkzeug.isCorrectForDrops(block)

use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

use crate::block_registry::block_name;

// Die Blockmenge einer Werkzeugregel -- ein Tag oder eine feste Liste (Block-Roh-IDs).
#[derive(Clone, Debug, PartialEq)]
pub enum BlockSet {
    Tag(String),
    Ids(Vec<u32>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolRule {
    pub blocks: BlockSet,
    pub speed: Option<f32>,
    pub correct_for_drops: Option<bool>,
}

// ToolComponent (ohne damage_per_block, das braucht nur der Server).
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub rules: Vec<ToolRule>,
    pub default_speed: f32,
    pub can_destroy_blocks_in_creative: bool,
}

struct Data {
    hardness: Vec<f32>,
    requires_tool: Vec<bool>,
    block_ids: HashMap<String, u32>,
    tags: HashMap<String, HashSet<u32>>,
    tools: HashMap<String, Tool>,
}

fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| parse(include_str!("mining_data.txt")))
}

fn parse(text: &str) -> Data {
    let mut d = Data { hardness: Vec::new(), requires_tool: Vec::new(), block_ids: HashMap::new(), tags: HashMap::new(), tools: HashMap::new() };
    let mut tag_lines: Vec<(&str, &str)> = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let mut parts = line.split(' ');
        match parts.next() {
            Some("B") => {
                let name = parts.next().unwrap_or("");
                let hardness: f32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let requires = parts.next() == Some("1");
                d.block_ids.insert(name.to_string(), d.hardness.len() as u32);
                d.hardness.push(hardness);
                d.requires_tool.push(requires);
            }
            Some("I") => {
                let name = parts.next().unwrap_or("").to_string();
                let default_speed = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1.0);
                let _count = parts.next();
                let creative = parts.next() != Some("false");
                d.tools.insert(name.clone(), Tool { rules: Vec::new(), default_speed, can_destroy_blocks_in_creative: creative });
                current = Some(name);
            }
            Some("R") => {
                let blocks = parts.next().unwrap_or("");
                let speed = parts.next().and_then(|s| s.parse().ok());
                let correct = match parts.next() {
                    Some("1") => Some(true),
                    Some("0") => Some(false),
                    _ => None,
                };
                // Listen-Regeln brauchen die Block-IDs -- die stehen vollstaendig VOR den
                // Items in der Datei.
                let blocks = match blocks.strip_prefix('#') {
                    Some(tag) => BlockSet::Tag(tag.to_string()),
                    None => BlockSet::Ids(blocks.split(',').filter_map(|b| d.block_ids.get(b).copied()).collect()),
                };
                if let Some(tool) = current.as_ref().and_then(|c| d.tools.get_mut(c)) {
                    tool.rules.push(ToolRule { blocks, speed, correct_for_drops: correct });
                }
            }
            Some("T") => {
                let name = parts.next().unwrap_or("");
                tag_lines.push((name, parts.next().unwrap_or("")));
            }
            _ => {}
        }
    }
    for (name, list) in tag_lines {
        let ids = list.split(',').filter_map(|b| d.block_ids.get(b).copied()).collect();
        d.tags.insert(name.to_string(), ids);
    }
    d
}

// Block-Roh-ID eines Zustands (Registry-Reihenfolge).
pub fn block_id(state: u32) -> Option<u32> {
    data().block_ids.get(block_name(state)?).copied()
}

// Haerte des Blocks; -1 = unzerstoerbar.
pub fn hardness(state: u32) -> f32 {
    block_id(state).map_or(0.0, |id| data().hardness[id as usize])
}

pub fn requires_tool(state: u32) -> bool {
    block_id(state).is_some_and(|id| data().requires_tool[id as usize])
}

// Das Werkzeug-Component eines Items ab Werk.
pub fn default_tool(item: &str) -> Option<&'static Tool> {
    data().tools.get(item)
}

// Die Block-Tags des Servers (Update Tags). Sie gelten statt der eingebauten, sobald einer
// da ist -- Datapacks aendern Tags, und can_place_on/can_break nennen beliebige.
static SERVER_TAGS: RwLock<Option<HashMap<String, HashSet<u32>>>> = RwLock::new(None);

// Schluessel ohne "minecraft:" wie in mining_data.txt.
pub fn set_server_block_tags(tags: HashMap<String, Vec<u32>>) {
    let tags = tags
        .into_iter()
        .map(|(name, ids)| (name.strip_prefix("minecraft:").map(str::to_string).unwrap_or(name), ids.into_iter().collect()))
        .collect();
    if let Ok(mut g) = SERVER_TAGS.write() {
        *g = Some(tags);
    }
}

fn set_contains(set: &BlockSet, block: u32) -> bool {
    match set {
        // Tags kommen mit oder ohne "minecraft:"-Praefix (vom Server immer mit).
        BlockSet::Tag(tag) => {
            let tag = tag.strip_prefix('#').unwrap_or(tag);
            let tag = tag.strip_prefix("minecraft:").unwrap_or(tag);
            if let Ok(g) = SERVER_TAGS.read() {
                if let Some(server) = g.as_ref() {
                    return server.get(tag).is_some_and(|s| s.contains(&block));
                }
            }
            data().tags.get(tag).is_some_and(|s| s.contains(&block))
        }
        BlockSet::Ids(ids) => ids.contains(&block),
    }
}

impl BlockSet {
    // Liegt der Block dieses Zustands in der Menge?
    pub fn contains_state(&self, state: u32) -> bool {
        block_id(state).is_some_and(|b| set_contains(self, b))
    }
}

impl Tool {
    // ToolComponent.getSpeed.
    pub fn speed(&self, state: u32) -> f32 {
        let Some(block) = block_id(state) else { return self.default_speed };
        self.rules
            .iter()
            .find(|r| r.speed.is_some() && set_contains(&r.blocks, block))
            .and_then(|r| r.speed)
            .unwrap_or(self.default_speed)
    }

    // ToolComponent.isCorrectForDrops.
    pub fn correct_for_drops(&self, state: u32) -> bool {
        let Some(block) = block_id(state) else { return false };
        self.rules
            .iter()
            .find(|r| r.correct_for_drops.is_some() && set_contains(&r.blocks, block))
            .and_then(|r| r.correct_for_drops)
            .unwrap_or(false)
    }
}

// Alles am Spieler, was in die Abbaugeschwindigkeit eingeht.
#[derive(Clone, Copy, Debug)]
pub struct BreakSpeedInput {
    pub mining_efficiency: f32,
    pub block_break_speed: f32,
    pub submerged_mining_speed: f32,
    // Staerke (amplifier) von Eile bzw. Conduit-Kraft -- die hoehere zaehlt
    // (StatusEffectUtil.getHasteAmplifier).
    pub haste: Option<i32>,
    pub mining_fatigue: Option<i32>,
    pub submerged_in_water: bool,
    pub on_ground: bool,
}

impl Default for BreakSpeedInput {
    fn default() -> Self {
        Self {
            mining_efficiency: 0.0,
            block_break_speed: 1.0,
            submerged_mining_speed: 0.2,
            haste: None,
            mining_fatigue: None,
            submerged_in_water: false,
            on_ground: true,
        }
    }
}

// AbstractBlock.calcBlockBreakingDelta: Fortschritt pro Tick, >= 1 heisst sofort abgebaut.
pub fn breaking_delta(state: u32, tool: Option<&Tool>, input: &BreakSpeedInput) -> f32 {
    let h = hardness(state);
    if h == -1.0 {
        return 0.0;
    }
    let can_harvest = !requires_tool(state) || tool.is_some_and(|t| t.correct_for_drops(state));
    let mut f = tool.map_or(1.0, |t| t.speed(state));
    if f > 1.0 {
        f += input.mining_efficiency;
    }
    if let Some(amp) = input.haste {
        f *= 1.0 + (amp + 1) as f32 * 0.2;
    }
    if let Some(amp) = input.mining_fatigue {
        f *= match amp {
            0 => 0.3,
            1 => 0.09,
            2 => 0.0027,
            _ => 8.1e-4,
        };
    }
    f *= input.block_break_speed;
    if input.submerged_in_water {
        f *= input.submerged_mining_speed;
    }
    if !input.on_ground {
        f /= 5.0;
    }
    f / h / if can_harvest { 30.0 } else { 100.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str) -> u32 {
        (0..crate::block_registry::state_count() as u32).find(|&s| block_name(s) == Some(name)).unwrap()
    }

    #[test]
    fn stein_mit_und_ohne_spitzhacke() {
        let input = BreakSpeedInput::default();
        // Hand: 1 / 1.5 / 100 pro Tick -> 150 Ticks = 7.5 s (Wiki: 7.5 s).
        let hand = breaking_delta(state("stone"), None, &input);
        assert!((hand - 1.0 / 150.0).abs() < 1e-6, "{hand}");
        // Holzspitzhacke: 2 / 1.5 / 30 -> 22.5 Ticks = 1.15 s (Wiki: 1.15 s).
        let holz = breaking_delta(state("stone"), default_tool("wooden_pickaxe"), &input);
        assert!((holz - 2.0 / 1.5 / 30.0).abs() < 1e-6, "{holz}");
        // Diamant-Spitzhacke + Effizienz V (25 + 1 = 26 extra): (8 + 26) / 1.5 / 30 = 0.76 --
        // noch nicht sofort. Erst mit Eile II (* 1.6) ist Stein ein Instamine.
        let eff5 = BreakSpeedInput { mining_efficiency: 26.0, ..input };
        assert!(breaking_delta(state("stone"), default_tool("diamond_pickaxe"), &eff5) < 1.0);
        let eile2 = BreakSpeedInput { haste: Some(1), ..eff5 };
        assert!(breaking_delta(state("stone"), default_tool("diamond_pickaxe"), &eile2) >= 1.0);
        // Obsidian mit Eisen: falsches Werkzeug -> /100.
        let eisen = breaking_delta(state("obsidian"), default_tool("iron_pickaxe"), &input);
        assert!((eisen - 6.0 / 50.0 / 100.0).abs() < 1e-6, "{eisen}");
        assert_eq!(breaking_delta(state("bedrock"), default_tool("netherite_pickaxe"), &input), 0.0);
    }

    #[test]
    fn schwert_und_cobweb() {
        let input = BreakSpeedInput::default();
        let schwert = default_tool("diamond_sword").unwrap();
        // Cobweb: Regel mit speed 15 und correct_for_drops -> 15 / 4 / 30.
        assert!((breaking_delta(state("cobweb"), Some(schwert), &input) - 15.0 / 4.0 / 30.0).abs() < 1e-6);
        // Gras hat Haerte 0 -> sofort (Division durch 0 = unendlich).
        assert!(breaking_delta(state("short_grass"), None, &input) >= 1.0);
        // In der Luft ein Fuenftel.
        let luft = BreakSpeedInput { on_ground: false, ..input };
        assert!((breaking_delta(state("dirt"), None, &luft) - 1.0 / 0.5 / 30.0 / 5.0).abs() < 1e-6);
    }
}
