// Bloecke, die Vanilla NICHT aus einem Blockmodell zeichnet, sondern mit einem eigenen
// Renderer und einer ENTITY-Textur: die BlockEntities. Ihr Blockmodell ist absichtlich leer
// -- `models/block/bed.json` enthaelt nur ein "particle"-Feld und kein einziges Element.
//
// Fuer den Mesher heisst das: nichts zu zeichnen, der Block ist unsichtbar. Ohne Sonderweg
// fehlen Betten deshalb ganz. Beds bekommen deshalb denselben Weg wie
// Entities: ein Quadermodell (s. entity_model::bed_head_model) mit der Textur
// entity/bed/<farbe>.png, gezeichnet im Entity-Pass statt im Chunk-Mesh.
//
// Damit der Zeichenpfad die Betten findet, ohne jeden Frame die ganze Welt abzusuchen, fuehrt
// World eine kleine Liste ihrer Positionen (s. World::beds). Gepflegt wird sie dort, wo sich
// Bloecke ueberhaupt aendern koennen -- beim Laden einer Spalte und bei jedem Block-Update.
//
// Dieselbe Luecke haben Truhen, Schilder, Banner, Shulker-Kisten und Koepfe (272 Blocknamen
// ohne aufgeloeste Form, s. app::tests/block_shape_audit). Die Liste hier ist bewusst NUR das
// Bett: jede weitere Familie braucht ihr eigenes, aus dem Jar abgeschriebenes Modell.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::block_registry::{block_name, state_count};
use crate::state_props::property;

// Alle State-Ids, die zu einem Bett gehoeren -- einmal aufgebaut, damit der Scan beim Laden
// einer Spalte ein reiner u32-Vergleich bleibt und keine Namensaufloesung pro Block braucht.
pub fn bed_states() -> &'static HashSet<u32> {
    static STATES: OnceLock<HashSet<u32>> = OnceLock::new();
    STATES.get_or_init(|| (0..state_count() as u32).filter(|&s| block_name(s).is_some_and(|n| n.ends_with("_bed"))).collect())
}

pub fn is_bed(state: u32) -> bool {
    bed_states().contains(&state)
}

// Shulkerkisten: wie Betten ein leeres Blockmodell (models/block/shulker_box.json hat nur
// "particle"), gezeichnet von ShulkerBoxBlockEntityRenderer mit Entity-Textur. Ohne
// diesen Weg waeren sie in der Welt unsichtbar.
pub fn shulker_states() -> &'static HashSet<u32> {
    static STATES: OnceLock<HashSet<u32>> = OnceLock::new();
    STATES.get_or_init(|| (0..state_count() as u32).filter(|&s| block_name(s).is_some_and(|n| n.ends_with("shulker_box"))).collect())
}

pub fn is_shulker_box(state: u32) -> bool {
    shulker_states().contains(&state)
}

// Truhen aller Art (ChestBlockEntityRenderer): normale, Redstone-, Ender- und Kupfertruhen
// (auch gewachst). Leeres Blockmodell wie Betten und Shulker -- ohne das unsichtbar.
fn is_chest_name(name: &str) -> bool {
    name == "chest" || name == "trapped_chest" || name == "ender_chest" || name.ends_with("copper_chest")
}

pub fn chest_states() -> &'static HashSet<u32> {
    static STATES: OnceLock<HashSet<u32>> = OnceLock::new();
    STATES.get_or_init(|| (0..state_count() as u32).filter(|&s| block_name(s).is_some_and(is_chest_name)).collect())
}

pub fn is_chest(state: u32) -> bool {
    chest_states().contains(&state)
}

// Bloecke mit Deckel, den ein Block-Event (Typ 1, Zahl der Betrachter) oeffnet.
pub fn has_lid(state: u32) -> bool {
    is_shulker_box(state) || is_chest(state)
}

// Platzierte Koepfe mit dem 8x8x8-Kopfmodell (SkullBlockEntityRenderer), als Paar
// (Blockname, Itemname) -- der Itemname waehlt Textur und Skin wie beim gehaltenen Kopf.
// Drachen- und Piglinkopf haben eigene Modelle und fehlen noch (wie in der Hand).
const SKULL_BLOCKS: &[(&str, &str)] = &[
    ("player_head", "player_head"),
    ("player_wall_head", "player_head"),
    ("zombie_head", "zombie_head"),
    ("zombie_wall_head", "zombie_head"),
    ("creeper_head", "creeper_head"),
    ("creeper_wall_head", "creeper_head"),
    ("skeleton_skull", "skeleton_skull"),
    ("skeleton_wall_skull", "skeleton_skull"),
    ("wither_skeleton_skull", "wither_skeleton_skull"),
    ("wither_skeleton_wall_skull", "wither_skeleton_skull"),
];

pub fn skull_states() -> &'static HashSet<u32> {
    static STATES: OnceLock<HashSet<u32>> = OnceLock::new();
    STATES.get_or_init(|| (0..state_count() as u32).filter(|&s| block_name(s).is_some_and(|n| SKULL_BLOCKS.iter().any(|(b, _)| *b == n))).collect())
}

pub fn is_skull(state: u32) -> bool {
    skull_states().contains(&state)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkullInstance {
    pub pos: (i32, i32, i32),
    // Itemname, s. SKULL_BLOCKS.
    pub item: &'static str,
    // An der Wand: (x, z) der Blickrichtung (WallSkullBlock.FACING); None = auf dem Boden.
    pub wall: Option<(i32, i32)>,
    // RotationPropertyHelper.toDegrees: Boden ROTATION * 22.5, Wand aus der Gegenrichtung.
    pub yaw_deg: f32,
}

pub fn skull_instance(pos: (i32, i32, i32), state: u32) -> Option<SkullInstance> {
    let name = block_name(state)?;
    let item = SKULL_BLOCKS.iter().find(|(b, _)| *b == name)?.1;
    if name.contains("_wall_") {
        // fromDirection(facing.getOpposite()): Sued 0, West 4, Nord 8, Ost 12 -- also fuer
        // Nord die 0 (Gegenrichtung Sued), fuer Sued 8, fuer West 12, fuer Ost 4.
        let (wall, stufe) = match property(state, "facing") {
            Some("south") => ((0, 1), 8),
            Some("west") => ((-1, 0), 12),
            Some("east") => ((1, 0), 4),
            _ => ((0, -1), 0),
        };
        return Some(SkullInstance { pos, item, wall: Some(wall), yaw_deg: stufe as f32 * 22.5 });
    }
    let stufe: u32 = property(state, "rotation").and_then(|r| r.parse().ok()).unwrap_or(0);
    Some(SkullInstance { pos, item, wall: None, yaw_deg: stufe as f32 * 22.5 })
}

// Kupfergolem-Statuen (CopperGolemStatueBlockRenderer): je Oxidationsstufe die Textur des
// Golems (CopperGolemOxidationLevels), gewachst dieselbe. Gilt fuer Block- und Itemnamen.
pub fn copper_golem_statue_texture(name: &str) -> Option<&'static str> {
    let stufe = name.strip_prefix("waxed_").unwrap_or(name).strip_suffix("copper_golem_statue")?;
    Some(match stufe {
        "" => "entity/copper_golem/copper_golem",
        "exposed_" => "entity/copper_golem/exposed_copper_golem",
        "weathered_" => "entity/copper_golem/weathered_copper_golem",
        "oxidized_" => "entity/copper_golem/oxidized_copper_golem",
        _ => return None,
    })
}

// Alle Statuentexturen, fuer den Entity-Atlas.
pub const COPPER_GOLEM_STATUE_TEXTURES: [&str; 4] = [
    "entity/copper_golem/copper_golem",
    "entity/copper_golem/exposed_copper_golem",
    "entity/copper_golem/weathered_copper_golem",
    "entity/copper_golem/oxidized_copper_golem",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatueInstance {
    pub pos: (i32, i32, i32),
    // Modell-Layer der Haltung (copper_golem_pose): stehend, sitzend, laufend, Stern.
    pub layer: &'static str,
    pub texture: &'static str,
    // Drehung um Y wie Vanilla: MINUS Direction.getOpposite().toYRot() von FACING
    // (s. entity_model::copper_golem_statue_transform). Ohne das Minus -- es steht als fneg im
    // Jar -- schauten nach Osten und Westen gesetzte Statuen genau verkehrt herum.
    pub yaw_deg: f32,
}

pub fn statue_instance(pos: (i32, i32, i32), state: u32) -> Option<StatueInstance> {
    let texture = copper_golem_statue_texture(block_name(state)?)?;
    let layer = match property(state, "copper_golem_pose") {
        Some("sitting") => "copper_golem_sitting#main",
        Some("running") => "copper_golem_running#main",
        Some("star") => "copper_golem_star#main",
        _ => "copper_golem#main",
    };
    // toYRot der Gegenrichtung: Sued 0, West 90, Nord 180, Ost 270.
    let gegen = match property(state, "facing") {
        Some("north") => 0.0,
        Some("east") => 90.0,
        Some("south") => 180.0,
        _ => 270.0,
    };
    Some(StatueInstance { pos, layer, texture, yaw_deg: -gegen })
}

fn is_statue(state: u32) -> bool {
    block_name(state).is_some_and(|n| copper_golem_statue_texture(n).is_some())
}

// Alles, was World in ihrem Index fuer BlockEntity-Modelle fuehrt (s. World::model_blocks).
pub fn is_model_block(state: u32) -> bool {
    model_block_table().get(state as usize).copied().unwrap_or(false)
}

// is_model_block als Tabelle je State-ID -- ein Array-Zugriff statt vier HashSet-Nachschlaegen,
// weil das beim Laden jeder Spalte fuer jeden Block laeuft (s. World::reindex_beds).
pub fn model_block_table() -> &'static [bool] {
    static TABELLE: OnceLock<Vec<bool>> = OnceLock::new();
    TABELLE.get_or_init(|| (0..state_count() as u32).map(|s| is_bed(s) || is_shulker_box(s) || is_chest(s) || is_skull(s) || is_statue(s)).collect())
}

// ChestType: einzeln oder eine Haelfte einer Doppeltruhe (eigene Modelle und Texturen).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChestHalf {
    Single,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChestInstance {
    pub pos: (i32, i32, i32),
    pub texture: &'static str,
    // Direction.getPositiveHorizontalDegrees() von FACING: Sued 0, West 90, Nord 180, Ost 270.
    pub yaw_deg: f32,
    pub half: ChestHalf,
    // Die andere Haelfte einer Doppeltruhe (ChestBlock.getFacing): ihr Deckel geht mit auf.
    pub partner: Option<(i32, i32, i32)>,
}

// Alle Truhentexturen (TexturedRenderLayers.getChestTextureId), fuer den Entity-Atlas.
pub const CHEST_TEXTURES: &[&str] = &[
    "entity/chest/normal",
    "entity/chest/normal_left",
    "entity/chest/normal_right",
    "entity/chest/trapped",
    "entity/chest/trapped_left",
    "entity/chest/trapped_right",
    "entity/chest/christmas",
    "entity/chest/christmas_left",
    "entity/chest/christmas_right",
    "entity/chest/ender",
    "entity/chest/copper",
    "entity/chest/copper_left",
    "entity/chest/copper_right",
    "entity/chest/copper_exposed",
    "entity/chest/copper_exposed_left",
    "entity/chest/copper_exposed_right",
    "entity/chest/copper_weathered",
    "entity/chest/copper_weathered_left",
    "entity/chest/copper_weathered_right",
    "entity/chest/copper_oxidized",
    "entity/chest/copper_oxidized_left",
    "entity/chest/copper_oxidized_right",
];

// `weihnachten`: Vanillas Weihnachtstruhen vom 24. bis 26. Dezember (ChestBlockEntityRenderer.
// getVariant). Das gilt fuer alle Truhen ausser der Endertruhe -- auch Redstone- und
// Kupfertruhen, weil CHRISTMAS dort vor TRAPPED und COPPER geprueft wird.
pub fn chest_instance(pos: (i32, i32, i32), state: u32, weihnachten: bool) -> Option<ChestInstance> {
    let name = block_name(state)?;
    if !is_chest_name(name) {
        return None;
    }
    let half = match property(state, "type") {
        Some("left") => ChestHalf::Left,
        Some("right") => ChestHalf::Right,
        _ => ChestHalf::Single,
    };
    let basis = if name == "ender_chest" {
        "ender"
    } else if weihnachten {
        "christmas"
    } else if name == "trapped_chest" {
        "trapped"
    } else if name.contains("copper") {
        if name.contains("exposed") {
            "copper_exposed"
        } else if name.contains("weathered") {
            "copper_weathered"
        } else if name.contains("oxidized") {
            "copper_oxidized"
        } else {
            "copper"
        }
    } else {
        "normal"
    };
    // Die Endertruhe gibt es nur einzeln.
    let half = if basis == "ender" { ChestHalf::Single } else { half };
    let voll = match half {
        ChestHalf::Single => format!("entity/chest/{basis}"),
        ChestHalf::Left => format!("entity/chest/{basis}_left"),
        ChestHalf::Right => format!("entity/chest/{basis}_right"),
    };
    let texture = CHEST_TEXTURES.iter().copied().find(|t| *t == voll)?;
    let facing = property(state, "facing").unwrap_or("north");
    let yaw_deg = match facing {
        "west" => 90.0,
        "north" => 180.0,
        "east" => 270.0,
        _ => 0.0,
    };
    // ChestBlock.getFacing: LEFT -> facing.rotateYClockwise(), RIGHT -> gegen den Uhrzeigersinn.
    let im_uhrzeigersinn = |f: &str| match f {
        "north" => (1, 0),
        "east" => (0, 1),
        "south" => (-1, 0),
        _ => (0, -1),
    };
    let partner = match half {
        ChestHalf::Single => None,
        ChestHalf::Left => {
            let (dx, dz) = im_uhrzeigersinn(facing);
            Some((pos.0 + dx, pos.1, pos.2 + dz))
        }
        ChestHalf::Right => {
            let (dx, dz) = im_uhrzeigersinn(facing);
            Some((pos.0 - dx, pos.1, pos.2 - dz))
        }
    };
    Some(ChestInstance { pos, texture, yaw_deg, half, partner })
}

// TexturedRenderLayers.SHULKER_TEXTURE_ID (ungefaerbt) und getShulkerBoxTextureId(farbe).
pub const SHULKER_TEXTURES: &[&str] = &[
    "entity/shulker/shulker",
    "entity/shulker/shulker_white",
    "entity/shulker/shulker_orange",
    "entity/shulker/shulker_magenta",
    "entity/shulker/shulker_light_blue",
    "entity/shulker/shulker_yellow",
    "entity/shulker/shulker_lime",
    "entity/shulker/shulker_pink",
    "entity/shulker/shulker_gray",
    "entity/shulker/shulker_light_gray",
    "entity/shulker/shulker_cyan",
    "entity/shulker/shulker_purple",
    "entity/shulker/shulker_blue",
    "entity/shulker/shulker_brown",
    "entity/shulker/shulker_green",
    "entity/shulker/shulker_red",
    "entity/shulker/shulker_black",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShulkerInstance {
    pub pos: (i32, i32, i32),
    pub texture: &'static str,
    // ShulkerBoxBlock.FACING: in welche Richtung der Deckel aufgeht.
    pub facing: &'static str,
}

pub fn shulker_instance(pos: (i32, i32, i32), state: u32) -> Option<ShulkerInstance> {
    let name = block_name(state)?;
    let farbe = name.strip_suffix("shulker_box")?;
    let texture = match farbe.strip_suffix('_') {
        None if farbe.is_empty() => SHULKER_TEXTURES[0],
        Some(f) => SHULKER_TEXTURES.iter().copied().find(|t| t.strip_prefix("entity/shulker/shulker_") == Some(f))?,
        None => return None,
    };
    let facing = match property(state, "facing") {
        Some("down") => "down",
        Some("north") => "north",
        Some("south") => "south",
        Some("west") => "west",
        Some("east") => "east",
        _ => "up",
    };
    Some(ShulkerInstance { pos, texture, facing })
}

// Was der Zeichenpfad ueber ein Bett wissen muss.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BedInstance {
    pub pos: (i32, i32, i32),
    // Texturname im Entity-Atlas, z.B. "entity/bed/red".
    pub texture: &'static str,
    // Vanillas BedBlockEntityRenderState.facing (BedBlock.FACING) in Grad, wie
    // Direction.getPositiveHorizontalDegrees() sie liefert: Sued 0, West 90, Nord 180, Ost 270.
    pub facing_deg: f32,
    // BedPart.HEAD -> Kopfteil-Modell, sonst Fussteil.
    pub head: bool,
}

// Die 16 Farbtexturen, wie TexturedRenderLayers.getBedTextureId sie aufloest: der Blockname
// ist "<farbe>_bed", die Textur "entity/bed/<farbe>".
pub const BED_TEXTURES: &[&str] = &[
    "entity/bed/white",
    "entity/bed/orange",
    "entity/bed/magenta",
    "entity/bed/light_blue",
    "entity/bed/yellow",
    "entity/bed/lime",
    "entity/bed/pink",
    "entity/bed/gray",
    "entity/bed/light_gray",
    "entity/bed/cyan",
    "entity/bed/purple",
    "entity/bed/blue",
    "entity/bed/brown",
    "entity/bed/green",
    "entity/bed/red",
    "entity/bed/black",
];

pub fn bed_instance(pos: (i32, i32, i32), state: u32) -> Option<BedInstance> {
    let name = block_name(state)?;
    let farbe = name.strip_suffix("_bed")?;
    let texture = BED_TEXTURES.iter().copied().find(|t| t.strip_prefix("entity/bed/") == Some(farbe))?;
    let facing_deg = match property(state, "facing") {
        Some("west") => 90.0,
        Some("north") => 180.0,
        Some("east") => 270.0,
        _ => 0.0, // south
    };
    Some(BedInstance { pos, texture, facing_deg, head: property(state, "part") == Some("head") })
}

#[cfg(test)]
mod shulker_tests {
    use super::*;

    // Statue: Haltung waehlt den Layer, die Oxidationsstufe die Textur, FACING die Drehung.
    #[test]
    fn statue_nach_haltung_stufe_und_richtung() {
        let finde = |name: &str, pose: &str, facing: &str| {
            (0..state_count() as u32)
                .find(|&s| block_name(s) == Some(name) && property(s, "copper_golem_pose") == Some(pose) && property(s, "facing") == Some(facing))
                .unwrap_or_else(|| panic!("{name} {pose} {facing} fehlt"))
        };
        let s = statue_instance((0, 0, 0), finde("waxed_exposed_copper_golem_statue", "sitting", "south")).expect("Statue");
        assert_eq!((s.layer, s.texture, s.yaw_deg), ("copper_golem_sitting#main", "entity/copper_golem/exposed_copper_golem", -180.0));
        let n = statue_instance((0, 0, 0), finde("copper_golem_statue", "standing", "north")).expect("Statue");
        assert_eq!((n.layer, n.yaw_deg), ("copper_golem#main", 0.0));
        assert!(is_model_block(finde("copper_golem_statue", "star", "east")));
        // Das Gesicht (Modell -z, nach der Z-Halbdrehung unveraendert) zeigt in FACING-Richtung.
        for (facing, richtung) in [("north", [0.0, -1.0]), ("south", [0.0, 1.0]), ("east", [1.0, 0.0]), ("west", [-1.0, 0.0])] {
            let st = statue_instance((0, 0, 0), finde("copper_golem_statue", "standing", facing)).expect("Statue");
            let t = crate::entity_model::copper_golem_statue_transform([0.0; 3], st.yaw_deg);
            let (mitte, nase) = (t.apply([0.0, 0.0, 0.0]), t.apply([0.0, 0.0, -1.0]));
            let blick = [nase[0] - mitte[0], nase[2] - mitte[2]];
            assert!((blick[0] - richtung[0]).abs() < 1e-5 && (blick[1] - richtung[1]).abs() < 1e-5, "{facing}: schaut nach {blick:?}");
        }
    }
    use crate::block_registry::state_count;

    fn state_of(name: &str, facing: &str) -> u32 {
        (0..state_count() as u32).find(|&s| block_name(s) == Some(name) && property(s, "facing") == Some(facing)).unwrap()
    }

    #[test]
    fn farbe_und_richtung() {
        let s = shulker_instance((1, 2, 3), state_of("light_blue_shulker_box", "north")).unwrap();
        assert_eq!(s.texture, "entity/shulker/shulker_light_blue");
        assert_eq!(s.facing, "north");
        let s = shulker_instance((0, 0, 0), state_of("shulker_box", "down")).unwrap();
        assert_eq!(s.texture, "entity/shulker/shulker");
        assert_eq!(s.facing, "down");
        assert_eq!(shulker_states().len(), 17 * 6);
    }
}

#[cfg(test)]
mod chest_tests {
    use super::*;
    use crate::block_registry::state_count;

    fn state_of(name: &str, facing: &str, typ: &str) -> u32 {
        (0..state_count() as u32)
            .find(|&s| block_name(s) == Some(name) && property(s, "facing") == Some(facing) && property(s, "type").is_none_or(|t| t == typ))
            .unwrap()
    }

    #[test]
    fn varianten_und_haelften() {
        let c = chest_instance((0, 0, 0), state_of("chest", "north", "left"), false).unwrap();
        assert_eq!((c.texture, c.half, c.yaw_deg), ("entity/chest/normal_left", ChestHalf::Left, 180.0));
        // Nach Norden schauend liegt die rechte Haelfte der LINKEN im Osten (+X).
        assert_eq!(c.partner, Some((1, 0, 0)));
        let c = chest_instance((1, 0, 0), state_of("chest", "north", "right"), false).unwrap();
        assert_eq!(c.partner, Some((0, 0, 0)));
        let e = chest_instance((0, 0, 0), state_of("ender_chest", "south", "single"), true).unwrap();
        assert_eq!((e.texture, e.half), ("entity/chest/ender", ChestHalf::Single));
        let k = chest_instance((0, 0, 0), state_of("waxed_weathered_copper_chest", "east", "single"), false).unwrap();
        assert_eq!(k.texture, "entity/chest/copper_weathered");
        let t = chest_instance((0, 0, 0), state_of("trapped_chest", "east", "right"), true).unwrap();
        assert_eq!(t.texture, "entity/chest/christmas_right");
    }
}

#[cfg(test)]
mod skull_tests {
    use super::*;
    use crate::block_registry::state_count;

    #[test]
    fn boden_und_wand() {
        let boden = (0..state_count() as u32).find(|&s| block_name(s) == Some("player_head") && property(s, "rotation") == Some("4")).unwrap();
        let k = skull_instance((0, 0, 0), boden).unwrap();
        assert_eq!((k.item, k.wall, k.yaw_deg), ("player_head", None, 90.0));
        let wand = (0..state_count() as u32).find(|&s| block_name(s) == Some("skeleton_wall_skull") && property(s, "facing") == Some("east")).unwrap();
        let k = skull_instance((0, 0, 0), wand).unwrap();
        assert_eq!((k.item, k.wall, k.yaw_deg), ("skeleton_skull", Some((1, 0)), 90.0));
    }
}
