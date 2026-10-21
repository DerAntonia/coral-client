// Rezeptbuch (Protokoll 774): was der Server ueber freigeschaltete Rezepte schickt, und das
// eine Paket, mit dem der Client ein Rezept ins Craftingfeld legen laesst.
//
// Der Server schickt keine Rezepte an sich, sondern ANZEIGEN davon (RecipeDisplay): welche
// Zutaten wo liegen, was herauskommt -- plus, was zum Craften gebraucht wird
// (crafting_requirements), damit der Client "craftbar?" selbst pruefen kann. Gecraftet wird
// immer auf dem Server: der Client nennt nur die ID der Anzeige (place_recipe).
//
// Reihenfolgen der Typ-IDs (eingebaute Registries, nicht synchronisiert) aus dem 1.21.11-
// Client: SlotDisplay empty, any_fuel, item, item_stack, tag, smithing_trim, with_remainder,
// composite; RecipeDisplay crafting_shapeless, crafting_shaped, furnace, stonecutter,
// smithing; Kategorien s. CATEGORY_*.

use std::collections::HashMap;

use crate::buffer::{PacketReader, PacketWriter, ReadError};
use crate::item::{read_optional_slot, ItemStack};

pub const CB_PLAY_PLACE_GHOST_RECIPE: i32 = 0x3D;
pub const CB_PLAY_RECIPE_BOOK_ADD: i32 = 0x48;
pub const CB_PLAY_RECIPE_BOOK_REMOVE: i32 = 0x49;
pub const CB_PLAY_RECIPE_BOOK_SETTINGS: i32 = 0x4A;
pub const CB_PLAY_UPDATE_TAGS: i32 = 0x84;
pub const CB_CONFIG_UPDATE_TAGS: i32 = 0x0D;
pub const SB_PLAY_PLACE_RECIPE: i32 = 0x26;

// Rezeptbuch-Kategorien (Registry recipe_book_category): die vier der Werkbank zuerst.
pub const CATEGORY_CRAFTING_BUILDING_BLOCKS: i32 = 0;
pub const CATEGORY_CRAFTING_REDSTONE: i32 = 1;
pub const CATEGORY_CRAFTING_EQUIPMENT: i32 = 2;
pub const CATEGORY_CRAFTING_MISC: i32 = 3;

// Was in einem Feld einer Anzeige steht.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotDisplay {
    Empty,
    AnyFuel,
    Item(u32),
    Stack(Box<ItemStack>),
    Tag(String),
    SmithingTrim { base: Box<SlotDisplay>, material: Box<SlotDisplay> },
    WithRemainder { input: Box<SlotDisplay>, remainder: Box<SlotDisplay> },
    Composite(Vec<SlotDisplay>),
}

impl SlotDisplay {
    // Ein Stapel, der fuer dieses Feld stehen kann (zum Zeichnen), falls es einen gibt.
    pub fn first_stack(&self) -> Option<ItemStack> {
        match self {
            SlotDisplay::Item(id) => Some(ItemStack::new(*id, 1)),
            SlotDisplay::Stack(s) => Some((**s).clone()),
            SlotDisplay::WithRemainder { input, .. } => input.first_stack(),
            SlotDisplay::SmithingTrim { base, .. } => base.first_stack(),
            SlotDisplay::Composite(alle) => alle.iter().find_map(|d| d.first_stack()),
            SlotDisplay::Empty | SlotDisplay::AnyFuel | SlotDisplay::Tag(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecipeDisplay {
    Shapeless { ingredients: Vec<SlotDisplay>, result: SlotDisplay, station: SlotDisplay },
    Shaped { width: i32, height: i32, ingredients: Vec<SlotDisplay>, result: SlotDisplay, station: SlotDisplay },
    Furnace { ingredient: SlotDisplay, fuel: SlotDisplay, result: SlotDisplay, station: SlotDisplay, duration: i32, experience: f32 },
    Stonecutter { input: SlotDisplay, result: SlotDisplay, station: SlotDisplay },
    Smithing { template: SlotDisplay, base: SlotDisplay, addition: SlotDisplay, result: SlotDisplay, station: SlotDisplay },
}

impl RecipeDisplay {
    pub fn result(&self) -> &SlotDisplay {
        match self {
            RecipeDisplay::Shapeless { result, .. }
            | RecipeDisplay::Shaped { result, .. }
            | RecipeDisplay::Furnace { result, .. }
            | RecipeDisplay::Stonecutter { result, .. }
            | RecipeDisplay::Smithing { result, .. } => result,
        }
    }

    // Passt das Rezept in ein Craftingfeld dieser Kantenlaenge (2 = Inventar, 3 = Werkbank)?
    pub fn fits_grid(&self, kante: i32) -> bool {
        match self {
            RecipeDisplay::Shaped { width, height, .. } => *width <= kante && *height <= kante,
            RecipeDisplay::Shapeless { ingredients, .. } => ingredients.len() as i32 <= kante * kante,
            _ => false,
        }
    }
}

// Eine Zutat: eine Liste von Items oder ein Item-Tag (HolderSet).
#[derive(Clone, Debug, PartialEq)]
pub enum Ingredient {
    Items(Vec<u32>),
    Tag(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecipeEntry {
    // Die ID, mit der place_recipe das Rezept nennt.
    pub id: i32,
    pub display: RecipeDisplay,
    pub group: Option<i32>,
    pub category: i32,
    // Je benoetigtem Stueck eine Zutat; None = nicht ueber das Buch craftbar.
    pub requirements: Option<Vec<Ingredient>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecipeBookAdd {
    // Eintrag und Flags (Bit 0 = Hinweis zeigen, Bit 1 = hervorheben).
    pub entries: Vec<(RecipeEntry, u8)>,
    // true = alles Bisherige ersetzen (beim Betreten), false = dazu.
    pub replace: bool,
}

fn read_count(r: &mut PacketReader) -> Result<usize, ReadError> {
    let n = r.read_varint()?;
    if !(0..=1 << 20).contains(&n) {
        return Err(ReadError);
    }
    Ok(n as usize)
}

pub fn read_slot_display(r: &mut PacketReader) -> Result<SlotDisplay, ReadError> {
    read_slot_display_depth(r, 0)
}

fn read_slot_display_depth(r: &mut PacketReader, tiefe: u32) -> Result<SlotDisplay, ReadError> {
    if tiefe > 16 {
        return Err(ReadError);
    }
    let weiter = |r: &mut PacketReader| read_slot_display_depth(r, tiefe + 1).map(Box::new);
    Ok(match r.read_varint()? {
        0 => SlotDisplay::Empty,
        1 => SlotDisplay::AnyFuel,
        2 => SlotDisplay::Item(r.read_varint()?.max(0) as u32),
        3 => SlotDisplay::Stack(Box::new(read_optional_slot(r).map_err(|_| ReadError)?.ok_or(ReadError)?)),
        4 => SlotDisplay::Tag(r.read_string()?),
        5 => {
            let base = weiter(r)?;
            let material = weiter(r)?;
            // Muster: Registry-Verweis (ID + 1) oder 0 und das Muster selbst.
            if r.read_varint()? == 0 {
                r.read_string()?;
                crate::nbt::read_root(r)?;
                r.read_bool()?;
            }
            SlotDisplay::SmithingTrim { base, material }
        }
        6 => SlotDisplay::WithRemainder { input: weiter(r)?, remainder: weiter(r)? },
        7 => {
            let n = read_count(r)?;
            let mut alle = Vec::with_capacity(n.min(64));
            for _ in 0..n {
                alle.push(read_slot_display_depth(r, tiefe + 1)?);
            }
            SlotDisplay::Composite(alle)
        }
        _ => return Err(ReadError),
    })
}

fn read_slot_list(r: &mut PacketReader) -> Result<Vec<SlotDisplay>, ReadError> {
    let n = read_count(r)?;
    let mut out = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        out.push(read_slot_display(r)?);
    }
    Ok(out)
}

pub fn read_recipe_display(r: &mut PacketReader) -> Result<RecipeDisplay, ReadError> {
    Ok(match r.read_varint()? {
        0 => RecipeDisplay::Shapeless { ingredients: read_slot_list(r)?, result: read_slot_display(r)?, station: read_slot_display(r)? },
        1 => RecipeDisplay::Shaped {
            width: r.read_varint()?,
            height: r.read_varint()?,
            ingredients: read_slot_list(r)?,
            result: read_slot_display(r)?,
            station: read_slot_display(r)?,
        },
        2 => RecipeDisplay::Furnace {
            ingredient: read_slot_display(r)?,
            fuel: read_slot_display(r)?,
            result: read_slot_display(r)?,
            station: read_slot_display(r)?,
            duration: r.read_varint()?,
            experience: r.read_f32()?,
        },
        3 => RecipeDisplay::Stonecutter { input: read_slot_display(r)?, result: read_slot_display(r)?, station: read_slot_display(r)? },
        4 => RecipeDisplay::Smithing {
            template: read_slot_display(r)?,
            base: read_slot_display(r)?,
            addition: read_slot_display(r)?,
            result: read_slot_display(r)?,
            station: read_slot_display(r)?,
        },
        _ => return Err(ReadError),
    })
}

// HolderSet<Item>: 0 = Tag-Name folgt, sonst Anzahl + 1 Item-IDs.
fn read_ingredient(r: &mut PacketReader) -> Result<Ingredient, ReadError> {
    let n = r.read_varint()?;
    if n == 0 {
        return Ok(Ingredient::Tag(r.read_string()?));
    }
    if !(1..=1 << 16).contains(&n) {
        return Err(ReadError);
    }
    let mut items = Vec::with_capacity((n - 1).min(64) as usize);
    for _ in 0..n - 1 {
        items.push(r.read_varint()?.max(0) as u32);
    }
    Ok(Ingredient::Items(items))
}

fn read_entry(r: &mut PacketReader) -> Result<RecipeEntry, ReadError> {
    let id = r.read_varint()?;
    let display = read_recipe_display(r)?;
    // OPTIONAL_VAR_INT: 0 = keine Gruppe, sonst Gruppe + 1.
    let group = match r.read_varint()? {
        0 => None,
        g => Some(g - 1),
    };
    let category = r.read_varint()?;
    let requirements = if r.read_bool()? {
        let n = read_count(r)?;
        let mut zutaten = Vec::with_capacity(n.min(16));
        for _ in 0..n {
            zutaten.push(read_ingredient(r)?);
        }
        Some(zutaten)
    } else {
        None
    };
    Ok(RecipeEntry { id, display, group, category, requirements })
}

pub fn read_recipe_book_add(payload: &[u8]) -> Result<RecipeBookAdd, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let n = read_count(&mut r)?;
    let mut entries = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        let entry = read_entry(&mut r)?;
        let flags = r.read_u8()?;
        entries.push((entry, flags));
    }
    let replace = r.read_bool()?;
    Ok(RecipeBookAdd { entries, replace })
}

pub fn read_recipe_book_remove(payload: &[u8]) -> Result<Vec<i32>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let n = read_count(&mut r)?;
    let mut ids = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        ids.push(r.read_varint()?);
    }
    Ok(ids)
}

// "Ins Craftingfeld legen": `max` = so viele Saetze wie moeglich statt einem.
pub fn place_recipe_payload(container_id: i32, recipe_id: i32, max: bool) -> Vec<u8> {
    let mut w = PacketWriter::new();
    w.write_varint(SB_PLAY_PLACE_RECIPE).write_varint(container_id).write_varint(recipe_id).write_bool(max);
    w.into_bytes()
}

// Update Tags (Konfiguration und Spiel gleich aufgebaut): je Registry ihre Tags mit den
// Eintrags-IDs. Zurueck kommen nur die Item-Tags, Schluessel ohne "#".
pub fn read_item_tags(payload: &[u8]) -> Result<HashMap<String, Vec<u32>>, ReadError> {
    read_tags_of(payload, "item")
}

// Dasselbe Paket, die Block-Tags (Block-Roh-IDs) -- fuer Werkzeugregeln und
// can_place_on/can_break mit Tag.
pub fn read_block_tags(payload: &[u8]) -> Result<HashMap<String, Vec<u32>>, ReadError> {
    read_tags_of(payload, "block")
}

fn read_tags_of(payload: &[u8], wanted: &str) -> Result<HashMap<String, Vec<u32>>, ReadError> {
    let mut r = PacketReader::new(payload);
    let _id = r.read_varint()?;
    let mut out = HashMap::new();
    for _ in 0..read_count(&mut r)? {
        let registry = r.read_string()?;
        let items = registry.strip_prefix("minecraft:").unwrap_or(&registry) == wanted;
        for _ in 0..read_count(&mut r)? {
            let name = r.read_string()?;
            let n = read_count(&mut r)?;
            let mut ids = Vec::with_capacity(if items { n.min(4096) } else { 0 });
            for _ in 0..n {
                let id = r.read_varint()?;
                if items {
                    ids.push(id.max(0) as u32);
                }
            }
            if items {
                out.insert(name, ids);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liest_einen_geformten_eintrag_mit_tag_zutat() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_RECIPE_BOOK_ADD).write_varint(1);
        // Eintrag: ID 7, geformt 1x2 (zwei Bretter-Tags -> 4 Stoecke), Gruppe 3, Kategorie misc.
        w.write_varint(7).write_varint(1).write_varint(1).write_varint(2);
        w.write_varint(2).write_varint(4).write_string("minecraft:planks").write_varint(4).write_string("minecraft:planks");
        w.write_varint(3).write_varint(4).write_varint(900).write_varint(0).write_varint(0); // Ergebnis: 4x Item 900, ohne Components
        w.write_varint(0); // Station: leer
        w.write_varint(4).write_varint(CATEGORY_CRAFTING_MISC);
        w.write_bool(true).write_varint(2).write_varint(0).write_string("minecraft:planks").write_varint(3).write_varint(11).write_varint(12);
        w.write_u8(1).write_bool(true);
        let add = read_recipe_book_add(&w.into_bytes()).unwrap();
        assert!(add.replace);
        let (e, flags) = &add.entries[0];
        assert_eq!((e.id, e.group, e.category, *flags), (7, Some(3), CATEGORY_CRAFTING_MISC, 1));
        assert!(e.display.fits_grid(2));
        assert_eq!(e.display.result().first_stack().map(|s| (s.item_id, s.count)), Some((900, 4)));
        assert_eq!(e.requirements, Some(vec![Ingredient::Tag("minecraft:planks".into()), Ingredient::Items(vec![11, 12])]));
    }

    #[test]
    fn liest_nur_item_tags() {
        let mut w = PacketWriter::new();
        w.write_varint(CB_PLAY_UPDATE_TAGS).write_varint(2);
        w.write_string("minecraft:block").write_varint(1).write_string("minecraft:logs").write_varint(1).write_varint(5);
        w.write_string("minecraft:item").write_varint(1).write_string("minecraft:planks").write_varint(2).write_varint(11).write_varint(12);
        let tags = read_item_tags(&w.into_bytes()).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags["minecraft:planks"], vec![11, 12]);
    }
}
