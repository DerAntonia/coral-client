// Das Kreativinventar (Vanillas CreativeModeInventoryScreen).
//
// Welche Items in welchem Tab stehen, kommt aus dem Client-Jar der Installation (s.
// assets::creative_tabs_data und CreativeTabsDump.java). Der Dump nennt jeden Stapel mit
// seinen Components; die meisten stehen schon in Netzform darin, nur Verweise auf Registries,
// die erst der Server festlegt (Verzauberungen, Gemaelde, Bannermuster, Ziegenhoerner), werden
// hier mit dessen IDs nachgerechnet (s. resolve).
//
// Im Kreativmodus verwaltet der Client das Inventar selbst: jeder geaenderte Slot geht als
// SetCreativeModeSlot an den Server, der ihn ohne Rueckmeldung uebernimmt. Die Klicklogik
// unten (s. click) bildet nach, was Vanilla dabei lokal rechnet.

use protocol::item::ItemStack;

use crate::inventory::{DollRect, ScreenLayout, SlotRect, ICON_SIZE, PLAYER_HOTBAR_START, PLAYER_MAIN_START, PLAYER_OFFHAND, PLAYER_SLOT_COUNT, SLOT_PITCH};

// Fenster in GUI-Pixeln (imageWidth/imageHeight).
pub const SHEET_W: f32 = 195.0;
pub const SHEET_H: f32 = 136.0;
// Reiter: 26x32, je Spalte 27 weiter; oben ragen sie 28 ueber das Fenster, unten haengen sie
// 4 in dessen Unterkante. Treffer- und Hinweisflaeche liegen etwas anders (s. tab_hit).
pub const TAB_W: f32 = 26.0;
pub const TAB_H: f32 = 32.0;
const TAB_PITCH: f32 = 27.0;
// Raster der Items: 9 Spalten, 5 Zeilen ab (9, 18); darunter die Hotbar bei y 112.
pub const GRID_COLUMNS: usize = 9;
pub const GRID_ROWS: usize = 5;
const GRID_X: f32 = 9.0;
const GRID_Y: f32 = 18.0;
const HOTBAR_Y: f32 = 112.0;
// Schieber: Bahn ab (175, 18), 112 hoch, der Griff 12x15.
pub const SCROLLER_X: f32 = 175.0;
pub const SCROLLER_Y: f32 = 18.0;
pub const SCROLL_TRACK: f32 = 112.0;
pub const SCROLLER_W: f32 = 12.0;
pub const SCROLLER_H: f32 = 15.0;
// Suchfeld (nur im Suchreiter).
pub const SEARCH_X: f32 = 82.0;
pub const SEARCH_Y: f32 = 6.0;
pub const SEARCH_W: f32 = 80.0;
pub const SEARCH_MAX_CHARS: usize = 50;
// Muelleimer-Slot im Inventar-Reiter.
const DESTROY_X: f32 = 173.0;
const DESTROY_Y: f32 = 112.0;

// SlotRect::index fuer Plaetze, die keine Spielerslots sind: das Raster (plus Zelle) und der
// Muelleimer. Spielerslots behalten ihre Nummer im Spielerinventar (ScreenHandler 0).
pub const SLOT_GRID: usize = 1000;
pub const SLOT_DESTROY: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabKind {
    Category,
    Hotbar,
    Search,
    Inventory,
}

pub struct Tab {
    pub key: String,
    pub kind: TabKind,
    pub top: bool,
    pub column: u32,
    pub aligned_right: bool,
    pub show_title: bool,
    pub can_scroll: bool,
    // Name im UI-Atlas (s. assets::ui_atlas::GUI_SHEETS).
    pub background: &'static str,
    pub icon: String,
    pub name_key: String,
    pub visible: bool,
    pub items: Vec<DumpStack>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DumpStack {
    pub item: String,
    pub count: i32,
    pub added: Vec<DumpComponent>,
    pub removed: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DumpComponent {
    pub name: String,
    pub id: i32,
    // Netzform mit den eingebauten Registries; None = braucht die IDs des Servers.
    pub bytes: Option<Vec<u8>>,
    pub json: String,
}

fn background_sheet(texture: &str) -> &'static str {
    if texture.ends_with("tab_item_search.png") {
        "container/creative_inventory/tab_item_search"
    } else if texture.ends_with("tab_inventory.png") {
        "container/creative_inventory/tab_inventory"
    } else {
        "container/creative_inventory/tab_items"
    }
}

fn hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

// Den Text des Dumpers lesen (Format s. CreativeTabsDump.java). Unlesbare Zeilen fallen weg.
pub fn parse(text: &str) -> Vec<Tab> {
    let mut tabs: Vec<Tab> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("T ") {
            let f: Vec<&str> = rest.split(' ').collect();
            if f.len() < 11 {
                continue;
            }
            let kind = match f[1] {
                "HOTBAR" => TabKind::Hotbar,
                "SEARCH" => TabKind::Search,
                "INVENTORY" => TabKind::Inventory,
                _ => TabKind::Category,
            };
            tabs.push(Tab {
                key: f[0].to_string(),
                kind,
                top: f[2] == "TOP",
                column: f[3].parse().unwrap_or(0),
                aligned_right: f[4] == "1",
                show_title: f[5] == "1",
                can_scroll: f[6] == "1",
                background: background_sheet(f[7]),
                icon: f[8].to_string(),
                name_key: f[9].to_string(),
                // Gespeicherte Hotbars gibt es (noch) nicht -- Vanillas Fassung ist umstaendlich,
                // eine bessere kommt spaeter. Bis dahin bleibt ihr Reiter weg.
                visible: f[10] == "1" && kind != TabKind::Hotbar,
                items: Vec::new(),
            });
        } else if let Some(rest) = line.strip_prefix("I ") {
            let Some(tab) = tabs.last_mut() else { continue };
            let mut f = rest.split(' ');
            let (Some(item), Some(count)) = (f.next(), f.next().and_then(|c| c.parse().ok())) else { continue };
            tab.items.push(DumpStack { item: item.to_string(), count, added: Vec::new(), removed: Vec::new() });
        } else if let Some(rest) = line.strip_prefix("C ") {
            let Some(stack) = tabs.last_mut().and_then(|t| t.items.last_mut()) else { continue };
            let mut f = rest.splitn(3, '\t');
            let (Some(kopf), Some(netz), Some(json)) = (f.next(), f.next(), f.next()) else { continue };
            let Some((name, id)) = kopf.split_once(' ') else { continue };
            let Ok(id) = id.parse() else { continue };
            stack.added.push(DumpComponent { name: name.to_string(), id, bytes: if netz == "-" { None } else { hex(netz) }, json: json.to_string() });
        } else if let Some(rest) = line.strip_prefix("R ") {
            let Some(stack) = tabs.last_mut().and_then(|t| t.items.last_mut()) else { continue };
            if let Some(id) = rest.split(' ').nth(1).and_then(|i| i.parse().ok()) {
                stack.removed.push(id);
            }
        }
    }
    tabs
}

// Die Namen der Eintraege der Server-Registries, in ID-Reihenfolge (aus den
// Konfigurationsdaten, s. packets::read_enchantments/read_named_registry).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registries {
    pub enchantment: Vec<String>,
    pub painting_variant: Vec<String>,
    pub banner_pattern: Vec<String>,
}

fn position(liste: &[String], name: &str) -> Option<i32> {
    liste.iter().position(|n| n == name).map(|i| i as i32)
}

fn varint(out: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        if v & !0x7F == 0 {
            out.push(v as u8);
            return;
        }
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
}

const DYE_COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
];

// Netzform einer Component, deren Verweise erst die Registries des Servers festlegen. Aus dem
// JSON des Dumps, in dem diese Verweise als Namen stehen. None = kennt der Server nicht (dann
// zeigt auch Vanilla den Stapel nicht) oder unbekannte Component.
fn encode_with_registries(c: &DumpComponent, reg: &Registries) -> Option<Vec<u8>> {
    let json: serde_json::Value = serde_json::from_str(&c.json).ok()?;
    let mut out = Vec::new();
    match c.name.as_str() {
        // ItemEnchantments: Anzahl, dann je Eintrag die nackte Registry-ID und die Stufe.
        "minecraft:stored_enchantments" | "minecraft:enchantments" => {
            let map = json.as_object()?;
            varint(&mut out, map.len() as i32);
            for (name, stufe) in map {
                varint(&mut out, position(&reg.enchantment, name)?);
                varint(&mut out, stufe.as_i64()? as i32);
            }
        }
        // Instrument als EitherHolder: falsch = Verweis per Namen, so braucht es keine ID.
        "minecraft:instrument" => {
            let name = json.as_str()?;
            out.push(0);
            varint(&mut out, name.len() as i32);
            out.extend_from_slice(name.as_bytes());
        }
        // Holder: Registry-ID + 1 (0 hiesse "folgt direkt").
        "minecraft:painting/variant" => varint(&mut out, position(&reg.painting_variant, json.as_str()?)? + 1),
        // BannerPatternLayers: Liste aus (Holder des Musters, Farbe).
        "minecraft:banner_patterns" => {
            let lagen = json.as_array()?;
            varint(&mut out, lagen.len() as i32);
            for lage in lagen {
                varint(&mut out, position(&reg.banner_pattern, lage.get("pattern")?.as_str()?)? + 1);
                let farbe = lage.get("color")?.as_str()?;
                varint(&mut out, DYE_COLORS.iter().position(|f| *f == farbe)? as i32);
            }
        }
        _ => return None,
    }
    Some(out)
}

// Ein Stapel des Dumps als echter Stapel mit den IDs dieses Servers.
pub fn resolve(stack: &DumpStack, reg: &Registries) -> Option<ItemStack> {
    let item = world::item_registry::item_id_by_name(stack.item.strip_prefix("minecraft:").unwrap_or(&stack.item))?;
    let mut teile = Vec::with_capacity(stack.added.len());
    for c in &stack.added {
        let bytes = match &c.bytes {
            Some(b) => b.clone(),
            None => encode_with_registries(c, reg)?,
        };
        teile.push((c.id, bytes));
    }
    protocol::item::stack_from_parts(item, stack.count, &teile, &stack.removed).ok()
}

// --- Zustand des Bildschirms --------------------------------------------------------------

#[derive(Default)]
pub struct Creative {
    pub tabs: Vec<Tab>,
    // Je Tab die aufgeloesten Stapel (s. rebuild).
    items: Vec<Vec<ItemStack>>,
    // Fuer welche Registries `items` gerechnet ist.
    built_for: Option<Registries>,
    // Gewaehlter Reiter (Index in `tabs`) -- bleibt wie in Vanilla ueber das Schliessen hinweg.
    pub selected: usize,
    // Scrollstand 0..1 (scrollOffs) und ob gerade am Schieber gezogen wird.
    pub scroll: f32,
    pub scrolling: bool,
    pub search: String,
    search_hits: Vec<ItemStack>,
    // Darf der Reiter mit Befehlsbloecken erscheinen (s. set_op_allowed)?
    op_allowed: bool,
    // Laufendes Verteilen-Ziehen (s. Drag).
    pub drag: Option<Drag>,
}

impl Creative {
    pub fn set_tabs(&mut self, tabs: Vec<Tab>) {
        self.selected = tabs.iter().position(|t| t.key == "minecraft:building_blocks").unwrap_or(0);
        self.tabs = tabs;
        self.built_for = None;
        self.items.clear();
    }

    // Der Reiter, den Vanilla nur Operatoren mit eingeschalteter Option zeigt.
    fn is_op_tab(tab: &Tab) -> bool {
        tab.key == "minecraft:op_blocks"
    }

    // Wird dieser Reiter gezeigt?
    pub fn shows(&self, tab: &Tab) -> bool {
        tab.visible && (self.op_allowed || !Self::is_op_tab(tab))
    }

    pub fn set_op_allowed(&mut self, erlaubt: bool) {
        if erlaubt == self.op_allowed {
            return;
        }
        self.op_allowed = erlaubt;
        if !erlaubt && self.tab().is_some_and(Self::is_op_tab) {
            self.selected = self.tabs.iter().position(|t| t.key == "minecraft:building_blocks").unwrap_or(0);
        }
        self.refresh_search();
    }

    pub fn ready(&self) -> bool {
        !self.tabs.is_empty()
    }

    // Die Stapel gegen die Registries des Servers aufloesen -- einmal je Registry-Stand.
    pub fn rebuild(&mut self, reg: &Registries) {
        if self.built_for.as_ref() == Some(reg) && self.items.len() == self.tabs.len() {
            return;
        }
        self.items = self.tabs.iter().map(|t| t.items.iter().filter_map(|s| resolve(s, reg)).collect()).collect();
        self.built_for = Some(reg.clone());
        self.refresh_search();
    }

    pub fn tab(&self) -> Option<&Tab> {
        self.tabs.get(self.selected)
    }

    pub fn kind(&self) -> TabKind {
        self.tab().map_or(TabKind::Category, |t| t.kind)
    }

    pub fn select(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.selected = index;
        self.scroll = 0.0;
        self.scrolling = false;
        // Vanilla leert das Suchfeld beim Wechsel in den Suchreiter.
        if self.kind() == TabKind::Search {
            self.search.clear();
            self.refresh_search();
        }
    }

    pub fn icon(&self, tab: &Tab) -> Option<ItemStack> {
        let id = world::item_registry::item_id_by_name(tab.icon.strip_prefix("minecraft:").unwrap_or(&tab.icon))?;
        Some(ItemStack::new(id, 1))
    }

    // Was im Raster steht, Zelle fuer Zelle (ohne Scrollen). Geliehen, nicht kopiert: das
    // Raster fragt je Bild fuer jede seiner 45 Zellen nach, und im Suchreiter stehen hier alle
    // Items -- als Kopie waren das zehntausende Stapel je Bild.
    fn grid_items(&self) -> &[ItemStack] {
        match self.kind() {
            TabKind::Inventory | TabKind::Hotbar => &[],
            TabKind::Search => &self.search_hits,
            TabKind::Category => self.items.get(self.selected).map_or(&[], Vec::as_slice),
        }
    }

    // Wie viele Zeilen ueber die fuenf sichtbaren hinaus es gibt (calculateRowCount).
    fn extra_rows(&self) -> usize {
        self.grid_items().len().div_ceil(GRID_COLUMNS).saturating_sub(GRID_ROWS)
    }

    pub fn can_scroll(&self) -> bool {
        self.tab().is_some_and(|t| t.can_scroll) && self.grid_items().len() > GRID_COLUMNS * GRID_ROWS
    }

    // Erste sichtbare Zeile (getRowIndexForScroll).
    pub fn first_row(&self) -> usize {
        (self.scroll as f64 * self.extra_rows() as f64 + 0.5) as usize
    }

    // Mausrad: eine Zeile je Raste (subtractInputFromScroll).
    pub fn scroll_by(&mut self, rasten: f64) {
        if !self.can_scroll() {
            return;
        }
        let n = self.extra_rows() as f64;
        self.scroll = (self.scroll - (rasten / n) as f32).clamp(0.0, 1.0);
    }

    // Schieber ziehen: `rel_y` ist die Zeigerhoehe ueber der Bahnoberkante in GUI-Pixeln.
    pub fn drag_to(&mut self, rel_y: f32) {
        self.scroll = ((rel_y - 7.5) / (SCROLL_TRACK - SCROLLER_H)).clamp(0.0, 1.0);
    }

    // Oberkante des Griffs ueber der Bahn in GUI-Pixeln.
    pub fn scroller_offset(&self) -> f32 {
        ((SCROLL_TRACK - 17.0) * self.scroll) as i32 as f32
    }

    // Das Item in Rasterzelle `cell` (0..45) beim aktuellen Scrollstand.
    pub fn cell_item(&self, cell: usize) -> Option<ItemStack> {
        let index = self.first_row() * GRID_COLUMNS + cell;
        self.grid_items().get(index).cloned()
    }

    // Suchtreffer neu rechnen: Name (wie angezeigt) oder Item-ID enthaelt den Suchtext, ohne
    // Gross-/Kleinschreibung. Leer = alles aus dem Suchreiter.
    pub fn refresh_search(&mut self) {
        let Some(suche) = self.tabs.iter().position(|t| t.kind == TabKind::Search) else { return };
        let mut alle = self.items.get(suche).cloned().unwrap_or_default();
        // Ohne den Operator-Reiter fehlen seine Items auch in der Suche (Vanilla baut die Suche
        // dann ohne sie).
        if !self.op_allowed {
            if let Some(op) = self.tabs.iter().position(Self::is_op_tab).and_then(|i| self.items.get(i)) {
                alle.retain(|s| !op.iter().any(|o| o.stackable_with(s)));
            }
        }
        let text = self.search.trim().to_lowercase();
        self.search_hits = if text.is_empty() { alle } else { alle.into_iter().filter(|s| search_text(s).contains(&text)).collect() };
        self.scroll = 0.0;
    }

    pub fn type_char(&mut self, ch: char) {
        if self.search.chars().count() < SEARCH_MAX_CHARS && !ch.is_control() {
            self.search.push(ch);
            self.refresh_search();
        }
    }

    pub fn backspace(&mut self, ganzes_wort: bool) {
        if ganzes_wort {
            while self.search.ends_with(' ') {
                self.search.pop();
            }
            while self.search.chars().last().is_some_and(|c| c != ' ') {
                self.search.pop();
            }
        } else {
            self.search.pop();
        }
        self.refresh_search();
    }
}

// Wonach ein Stapel gefunden wird: sein angezeigter Name und seine Item-ID.
fn search_text(stack: &ItemStack) -> String {
    let id = world::item_registry::item_name(stack.item_id).unwrap_or("");
    let mut text = stack.custom_name.clone().unwrap_or_default();
    if text.is_empty() {
        text = protocol::nbt::translate(&format!("item.minecraft.{id}")).or_else(|| protocol::nbt::translate(&format!("block.minecraft.{id}"))).unwrap_or_default();
    }
    format!("{} {}", text.to_lowercase(), id.replace('_', " "))
}

// --- Bildschirmaufteilung -----------------------------------------------------------------

// Linke obere Ecke des Fensters in Bildschirmpixeln (leftPos/topPos).
pub fn origin(screen: (f32, f32), scale: f32) -> (f32, f32) {
    (((screen.0 - SHEET_W * scale) / 2.0).floor(), ((screen.1 - SHEET_H * scale) / 2.0).floor())
}

// Spalte eines Reiters in GUI-Pixeln relativ zum Fenster (getTabX).
fn tab_x(tab: &Tab) -> f32 {
    if tab.aligned_right {
        SHEET_W - TAB_PITCH * (7 - tab.column as i32) as f32 + 1.0
    } else {
        TAB_PITCH * tab.column as f32
    }
}

// Wo der Reiter gezeichnet wird (Bildschirmpixel, linke obere Ecke).
pub fn tab_draw_pos(tab: &Tab, o: (f32, f32), scale: f32) -> (f32, f32) {
    let y = if tab.top { -28.0 } else { SHEET_H - 4.0 };
    (o.0 + tab_x(tab) * scale, o.1 + y * scale)
}

// Wo sein Symbol sitzt: mittig, oben einen Pixel tiefer, unten einen hoeher.
pub fn tab_icon_pos(tab: &Tab, o: (f32, f32), scale: f32) -> (f32, f32) {
    let (x, y) = tab_draw_pos(tab, o, scale);
    (x + 5.0 * scale, y + if tab.top { 9.0 } else { 7.0 } * scale)
}

// Trifft der Punkt diesen Reiter? (checkTabClicked: 26x32 ab getTabX/getTabY.)
pub fn tab_hit(tab: &Tab, o: (f32, f32), scale: f32, px: f32, py: f32) -> bool {
    let (gx, gy) = ((px - o.0) / scale, (py - o.1) / scale);
    let ty = if tab.top { -TAB_H } else { SHEET_H };
    let tx = tab_x(tab);
    gx >= tx && gx <= tx + TAB_W && gy >= ty && gy <= ty + TAB_H
}

// Liegt der Punkt auf der Schieberbahn (insideScrollbar)?
pub fn on_scrollbar(o: (f32, f32), scale: f32, px: f32, py: f32) -> bool {
    let (gx, gy) = ((px - o.0) / scale, (py - o.1) / scale);
    gx >= SCROLLER_X && gy >= SCROLLER_Y && gx < SCROLLER_X + 14.0 && gy < SCROLLER_Y + SCROLL_TRACK
}

// Die Slots des Fensters fuer den Reiter `kind`.
pub fn layout(screen: (f32, f32), scale: f32, kind: TabKind, background: &'static str) -> ScreenLayout {
    let (bg_x, bg_y) = origin(screen, scale);
    let at = |index: usize, gx: f32, gy: f32| SlotRect {
        index,
        x: bg_x + gx * scale,
        y: bg_y + gy * scale,
        size: ICON_SIZE * scale,
        pad: (SLOT_PITCH - ICON_SIZE) / 2.0 * scale,
    };
    let mut slots = Vec::new();
    let mut doll = None;
    if kind == TabKind::Inventory {
        // Ruestung zwei Spalten zu zwei (Helm, Brust links; Hose, Schuhe rechts), Zweithand
        // daneben, das Hauptinventar darunter, ganz unten die Hotbar und der Muelleimer.
        for i in 0..4 {
            slots.push(at(crate::inventory::PLAYER_ARMOR_START + i, 54.0 + (i / 2) as f32 * 54.0, 6.0 + (i % 2) as f32 * 27.0));
        }
        slots.push(at(PLAYER_OFFHAND, 35.0, 20.0));
        for i in 0..27 {
            slots.push(at(PLAYER_MAIN_START + i, GRID_X + (i % 9) as f32 * SLOT_PITCH, 54.0 + (i / 9) as f32 * SLOT_PITCH));
        }
        slots.push(at(SLOT_DESTROY, DESTROY_X, DESTROY_Y));
        doll = Some(DollRect { x0: bg_x + 73.0 * scale, y0: bg_y + 6.0 * scale, x1: bg_x + 105.0 * scale, y1: bg_y + 49.0 * scale, pixels_per_meter: 20.0 * scale });
    } else {
        for cell in 0..GRID_COLUMNS * GRID_ROWS {
            slots.push(at(SLOT_GRID + cell, GRID_X + (cell % 9) as f32 * SLOT_PITCH, GRID_Y + (cell / 9) as f32 * SLOT_PITCH));
        }
    }
    for i in 0..9 {
        slots.push(at(PLAYER_HOTBAR_START + i, GRID_X + i as f32 * SLOT_PITCH, HOTBAR_Y));
    }
    ScreenLayout {
        bg_x,
        bg_y,
        bg_w: SHEET_W * scale,
        bg_h: SHEET_H * scale,
        sheet: background,
        parts: vec![((0, 0, SHEET_W as u32, SHEET_H as u32), 0.0)],
        slots,
        scale,
        tooltip_divider_x: bg_x + SHEET_W / 2.0 * scale,
        doll,
    }
}

// --- Verteilen durch Ziehen ---------------------------------------------------------------

// Mit einem Stapel an der Maus ueber mehrere Spielerslots ziehen (Vanillas Quick-Craft):
// links gleichmaessig aufteilen, rechts je ein Stueck, Mitte (nur Kreativ) je ein voller Stapel.
// Ins Raster laesst sich nicht ziehen (ItemPickerMenu.canDragTo).
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub button: u8,
    pub slots: Vec<usize>,
    // Der Stapel an der Maus beim Druecken.
    pub cursor: ItemStack,
}

impl Drag {
    // Darf `slot` mit diesem Inhalt noch dazu? Leer oder dasselbe Item, und bei links/rechts
    // nicht mehr Slots als Stuecke.
    pub fn accepts(&self, slot: usize, inhalt: Option<&ItemStack>) -> bool {
        !self.slots.contains(&slot)
            && inhalt.is_none_or(|s| s.stackable_with(&self.cursor))
            && (self.button == 2 || self.cursor.count > self.slots.len() as i32)
    }

    // Zaehlt das Ziehen als Verteilen? Sonst ist es beim Loslassen ein gewoehnlicher Klick.
    pub fn distributes(&self) -> bool {
        self.slots.len() > 1 || (self.button == 2 && !self.slots.is_empty())
    }

    // Ergebnis: die neuen Slot-Inhalte und was an der Maus bleibt.
    pub fn result(&self, player: &[Option<ItemStack>]) -> (Vec<(usize, ItemStack)>, Option<ItemStack>) {
        let ziele: Vec<(usize, Option<ItemStack>)> = self.slots.iter().map(|&i| (i, player.get(i).cloned().flatten())).collect();
        if self.button == 2 {
            let voll = with_count(&self.cursor, max_stack(&self.cursor));
            return (ziele.iter().map(|(i, _)| (*i, voll.clone())).collect(), Some(self.cursor.clone()));
        }
        let (geaendert, rest) = crate::inventory::drag_preview(self.button as i8, &self.cursor, &ziele);
        (geaendert.into_iter().filter_map(|(i, s)| Some((i as usize, s?))).collect(), rest)
    }
}

// --- Klicks -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    // Rasterzelle 0..45 (ohne Scrollen gezaehlt).
    Grid(usize),
    // Spielerslot (ScreenHandler-0-Nummer).
    Player(usize),
    Destroy,
    // Neben das Fenster (nicht auf einen Reiter).
    Outside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    // Links (0) oder rechts (1).
    Pickup(u8),
    QuickMove,
    // Zifferntaste 0..8, 40 = Zweithand.
    Swap(u8),
    // Mittelklick.
    Clone,
    // Wegwerf-Taste; `whole` mit gehaltener Stapel-Taste.
    Throw { whole: bool },
}

// Was ein Klick ausgeloest hat: diese Spielerslots haben sich geaendert (gehen als
// SetCreativeModeSlot raus), diese Stapel werden geworfen (SetCreativeModeSlot mit -1).
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub changed: Vec<usize>,
    pub drops: Vec<ItemStack>,
}

fn max_stack(s: &ItemStack) -> i32 {
    world::item_components::max_stack(s).max(1)
}

fn with_count(s: &ItemStack, count: i32) -> ItemStack {
    ItemStack { count, ..s.clone() }
}

// Gewoehnlicher Container-Klick auf einen Spielerslot (AbstractContainerMenu.doClick), wie
// Vanilla ihn im Kreativmodus lokal rechnet.
fn click_player(slot: usize, kind: ClickKind, item_tab: bool, player: &mut [Option<ItemStack>], cursor: &mut Option<ItemStack>, out: &mut Outcome) {
    if slot >= player.len() {
        return;
    }
    match kind {
        ClickKind::Pickup(button) => {
            let vorher = player[slot].clone();
            match (player[slot].take(), cursor.take()) {
                (None, None) => {}
                (Some(s), None) => {
                    if button == 0 {
                        *cursor = Some(s);
                    } else {
                        let nimm = (s.count + 1) / 2;
                        *cursor = Some(with_count(&s, nimm));
                        player[slot] = (s.count > nimm).then(|| with_count(&s, s.count - nimm));
                    }
                }
                (None, Some(c)) => {
                    let lege = if button == 0 { c.count.min(max_stack(&c)) } else { 1 };
                    player[slot] = Some(with_count(&c, lege));
                    *cursor = (c.count > lege).then(|| with_count(&c, c.count - lege));
                }
                (Some(s), Some(c)) if s.stackable_with(&c) => {
                    let platz = (max_stack(&s) - s.count).max(0);
                    let lege = if button == 0 { c.count.min(platz) } else { 1.min(platz) };
                    player[slot] = Some(with_count(&s, s.count + lege));
                    *cursor = (c.count > lege).then(|| with_count(&c, c.count - lege));
                }
                (Some(s), Some(c)) => {
                    player[slot] = Some(c);
                    *cursor = Some(s);
                }
            }
            if player[slot] != vorher {
                out.changed.push(slot);
            }
        }
        ClickKind::QuickMove => {
            let Some(s) = player[slot].take() else { return };
            out.changed.push(slot);
            // Im Item-Reiter loescht Shift-Klick auf die Hotbar (ItemPickerMenu.quickMoveStack).
            if item_tab {
                return;
            }
            // InventoryMenu.quickMoveStack: Hauptteil <-> Hotbar, alles andere ins Inventar.
            let ziel = if (PLAYER_MAIN_START..PLAYER_HOTBAR_START).contains(&slot) {
                PLAYER_HOTBAR_START..PLAYER_OFFHAND
            } else if (PLAYER_HOTBAR_START..PLAYER_OFFHAND).contains(&slot) {
                PLAYER_MAIN_START..PLAYER_HOTBAR_START
            } else {
                PLAYER_MAIN_START..PLAYER_OFFHAND
            };
            let mut rest = s.count;
            for i in ziel.clone() {
                if rest == 0 {
                    break;
                }
                if let Some(t) = &player[i] {
                    if t.stackable_with(&s) && t.count < max_stack(t) {
                        let n = rest.min(max_stack(t) - t.count);
                        player[i] = Some(with_count(t, t.count + n));
                        rest -= n;
                        out.changed.push(i);
                    }
                }
            }
            for i in ziel {
                if rest == 0 {
                    break;
                }
                if player[i].is_none() {
                    let n = rest.min(max_stack(&s));
                    player[i] = Some(with_count(&s, n));
                    rest -= n;
                    out.changed.push(i);
                }
            }
            if rest > 0 {
                player[slot] = Some(with_count(&s, rest));
            }
        }
        ClickKind::Swap(taste) => {
            let andere = if taste == 40 { PLAYER_OFFHAND } else if taste < 9 { PLAYER_HOTBAR_START + taste as usize } else { return };
            if andere == slot {
                return;
            }
            player.swap(slot, andere);
            out.changed.extend([slot, andere]);
        }
        ClickKind::Clone => {
            if cursor.is_none() {
                *cursor = player[slot].as_ref().map(|s| with_count(s, max_stack(s)));
            }
        }
        // Wegwerfen aus einem Slot nur mit leerer Maus (AbstractContainerMenu.doClick).
        ClickKind::Throw { whole } => {
            if cursor.is_some() {
                return;
            }
            let Some(s) = player[slot].take() else { return };
            let n = if whole { s.count } else { 1 };
            out.drops.push(with_count(&s, n));
            player[slot] = (s.count > n).then(|| with_count(&s, s.count - n));
            out.changed.push(slot);
        }
    }
}

// Ein Klick im Kreativinventar (CreativeModeInventoryScreen.slotClicked). `cell_item` ist das
// Item der angeklickten Rasterzelle, falls es eine ist.
pub fn click(tab: TabKind, target: Target, cell_item: Option<&ItemStack>, kind: ClickKind, player: &mut [Option<ItemStack>], cursor: &mut Option<ItemStack>) -> Outcome {
    let mut out = Outcome::default();
    match target {
        // Shift-Klick auf den Muelleimer leert das ganze Inventar, sonst verschwindet nur der
        // Stapel an der Maus.
        Target::Destroy => {
            if kind == ClickKind::QuickMove {
                for (i, s) in player.iter_mut().enumerate().take(PLAYER_SLOT_COUNT) {
                    *s = None;
                    out.changed.push(i);
                }
            } else {
                *cursor = None;
            }
        }
        Target::Outside => {
            let Some(c) = cursor.take() else { return out };
            let werfen = match kind {
                ClickKind::Pickup(1) => 1,
                _ => c.count,
            };
            out.drops.push(with_count(&c, werfen));
            *cursor = (c.count > werfen).then(|| with_count(&c, c.count - werfen));
        }
        Target::Player(slot) => click_player(slot, kind, tab != TabKind::Inventory, player, cursor, &mut out),
        Target::Grid(_) => {
            let item = cell_item.cloned();
            match kind {
                ClickKind::Swap(taste) => {
                    let ziel = if taste == 40 { PLAYER_OFFHAND } else if taste < 9 { PLAYER_HOTBAR_START + taste as usize } else { return out };
                    if let Some(item) = item {
                        player[ziel] = Some(with_count(&item, max_stack(&item)));
                        out.changed.push(ziel);
                    }
                }
                ClickKind::Clone => {
                    if cursor.is_none() {
                        *cursor = item.map(|i| with_count(&i, max_stack(&i)));
                    }
                }
                ClickKind::Throw { whole } => {
                    if let Some(item) = item {
                        out.drops.push(with_count(&item, if whole { max_stack(&item) } else { 1 }));
                    }
                }
                ClickKind::Pickup(_) | ClickKind::QuickMove => {
                    let schnell = kind == ClickKind::QuickMove;
                    let button = if let ClickKind::Pickup(b) = kind { b } else { 0 };
                    match (cursor.take(), item) {
                        // Gleiches Item an der Maus: links einer mehr (mit Shift voll), rechts einer weniger.
                        (Some(c), Some(i)) if c.stackable_with(&i) => {
                            *cursor = Some(if button == 0 {
                                if schnell {
                                    with_count(&c, max_stack(&c))
                                } else {
                                    with_count(&c, (c.count + 1).min(max_stack(&c)).max(c.count))
                                }
                            } else {
                                with_count(&c, c.count - 1)
                            })
                            .filter(|s| s.count > 0);
                        }
                        (None, Some(i)) => *cursor = Some(with_count(&i, if schnell { max_stack(&i) } else { i.count })),
                        // Sonst: links verschwindet der Stapel an der Maus ins Raster, rechts einer.
                        (Some(c), _) => {
                            if button != 0 && c.count > 1 {
                                *cursor = Some(with_count(&c, c.count - 1));
                            }
                        }
                        (None, None) => {}
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stein(n: i32) -> ItemStack {
        ItemStack::new(world::item_registry::item_id_by_name("stone").unwrap(), n)
    }

    fn leer() -> Vec<Option<ItemStack>> {
        vec![None; PLAYER_SLOT_COUNT]
    }

    #[test]
    fn dump_wird_gelesen() {
        let text = "T minecraft:combat CATEGORY BOTTOM 1 0 1 1 minecraft:textures/gui/container/creative_inventory/tab_items.png minecraft:netherite_sword itemGroup.combat 1\n\
                    I minecraft:enchanted_book 1\n\
                    C minecraft:stored_enchantments 41\t-\t{\"minecraft:sharpness\":5}\n\
                    I minecraft:potion 1\n\
                    C minecraft:potion_contents 49\t0105000000\t{\"potion\":\"minecraft:x\"}\n\
                    R minecraft:lore 11\n";
        let tabs = parse(text);
        assert_eq!(tabs.len(), 1);
        let t = &tabs[0];
        assert_eq!((t.kind, t.top, t.column, t.background), (TabKind::Category, false, 1, "container/creative_inventory/tab_items"));
        assert_eq!(t.items.len(), 2);
        assert_eq!(t.items[0].added[0].bytes, None);
        assert_eq!(t.items[1].added[0].bytes, Some(vec![1, 5, 0, 0, 0]));
        assert_eq!(t.items[1].removed, vec![11]);
        // Mit den Server-IDs aufgeloest: Schaerfe steht dort an Position 2.
        let reg = Registries { enchantment: vec!["minecraft:a".into(), "minecraft:b".into(), "minecraft:sharpness".into()], ..Default::default() };
        let buch = resolve(&t.items[0], &reg).unwrap();
        assert_eq!(buch.stored_enchantments, vec![protocol::item::Enchantment { id: 2, level: 5 }]);
        // Kennt der Server die Verzauberung nicht, fehlt das Buch -- wie in Vanilla.
        assert!(resolve(&t.items[0], &Registries::default()).is_none());
    }

    #[test]
    fn raster_nehmen_ablegen_und_loeschen() {
        let mut p = leer();
        let mut maus = None;
        let s = stein(1);
        // Linksklick nimmt einen, Shift-Klick einen vollen Stapel.
        click(TabKind::Category, Target::Grid(0), Some(&s), ClickKind::Pickup(0), &mut p, &mut maus);
        assert_eq!(maus.as_ref().map(|m| m.count), Some(1));
        click(TabKind::Category, Target::Grid(0), Some(&s), ClickKind::Pickup(0), &mut p, &mut maus);
        assert_eq!(maus.as_ref().map(|m| m.count), Some(2));
        click(TabKind::Category, Target::Grid(0), Some(&s), ClickKind::QuickMove, &mut p, &mut maus);
        assert_eq!(maus.as_ref().map(|m| m.count), Some(64));
        // Ablegen in die Hotbar meldet genau diesen Slot.
        let o = click(TabKind::Category, Target::Player(PLAYER_HOTBAR_START), None, ClickKind::Pickup(0), &mut p, &mut maus);
        assert_eq!(o.changed, vec![PLAYER_HOTBAR_START]);
        assert!(maus.is_none());
        // Zurueck ins Raster geworfen ist er weg.
        click(TabKind::Category, Target::Player(PLAYER_HOTBAR_START), None, ClickKind::Pickup(0), &mut p, &mut maus);
        click(TabKind::Category, Target::Grid(3), None, ClickKind::Pickup(0), &mut p, &mut maus);
        assert!(maus.is_none() && p[PLAYER_HOTBAR_START].is_none());
    }

    #[test]
    fn zifferntaste_legt_einen_vollen_stapel_in_die_hotbar() {
        let mut p = leer();
        let mut maus = None;
        let o = click(TabKind::Category, Target::Grid(5), Some(&stein(1)), ClickKind::Swap(2), &mut p, &mut maus);
        assert_eq!(o.changed, vec![PLAYER_HOTBAR_START + 2]);
        assert_eq!(p[PLAYER_HOTBAR_START + 2].as_ref().map(|s| s.count), Some(64));
    }

    #[test]
    fn inventar_reiter_shift_und_muelleimer() {
        let mut p = leer();
        let mut maus = None;
        p[PLAYER_HOTBAR_START] = Some(stein(10));
        p[PLAYER_MAIN_START + 4] = Some(stein(60));
        // Shift aus der Hotbar fuellt erst den angefangenen Stapel, dann den ersten freien.
        let o = click(TabKind::Inventory, Target::Player(PLAYER_HOTBAR_START), None, ClickKind::QuickMove, &mut p, &mut maus);
        assert_eq!(p[PLAYER_MAIN_START + 4].as_ref().map(|s| s.count), Some(64));
        assert_eq!(p[PLAYER_MAIN_START].as_ref().map(|s| s.count), Some(6));
        assert!(p[PLAYER_HOTBAR_START].is_none());
        assert_eq!(o.changed, vec![PLAYER_HOTBAR_START, PLAYER_MAIN_START + 4, PLAYER_MAIN_START]);
        // Shift auf den Muelleimer leert alles.
        let o = click(TabKind::Inventory, Target::Destroy, None, ClickKind::QuickMove, &mut p, &mut maus);
        assert_eq!(o.changed.len(), PLAYER_SLOT_COUNT);
        assert!(p.iter().all(Option::is_none));
    }

    #[test]
    fn ziehen_verteilt_links_rechts_und_mitte() {
        let mut p = leer();
        p[PLAYER_HOTBAR_START + 1] = Some(stein(3));
        let d = Drag { button: 0, slots: vec![PLAYER_HOTBAR_START, PLAYER_HOTBAR_START + 1, PLAYER_HOTBAR_START + 2], cursor: stein(10) };
        let (neu, rest) = d.result(&p);
        assert_eq!(neu.iter().map(|(_, s)| s.count).collect::<Vec<_>>(), vec![3, 6, 3]);
        assert_eq!(rest.map(|r| r.count), Some(1));
        let rechts = Drag { button: 1, ..d.clone() };
        assert_eq!(rechts.result(&p).1.map(|r| r.count), Some(7));
        // Mitte: volle Stapel, die Maus behaelt ihren.
        let mitte = Drag { button: 2, slots: vec![PLAYER_HOTBAR_START], cursor: stein(1) };
        assert!(mitte.distributes());
        assert_eq!(mitte.result(&p).0[0].1.count, 64);
        assert_eq!(mitte.result(&p).1.map(|r| r.count), Some(1));
        // Nicht mehr Slots als Stuecke, nichts Fremdes.
        let zwei = Drag { button: 0, slots: vec![1, 2], cursor: stein(2) };
        assert!(!zwei.accepts(3, None));
        let fremd = ItemStack::new(world::item_registry::item_id_by_name("dirt").unwrap(), 1);
        assert!(!d.accepts(9, Some(&fremd)));
    }

    #[test]
    fn daneben_klicken_wirft() {
        let mut p = leer();
        let mut maus = Some(stein(5));
        let o = click(TabKind::Category, Target::Outside, None, ClickKind::Pickup(1), &mut p, &mut maus);
        assert_eq!(o.drops.iter().map(|d| d.count).collect::<Vec<_>>(), vec![1]);
        assert_eq!(maus.as_ref().map(|m| m.count), Some(4));
        let o = click(TabKind::Category, Target::Outside, None, ClickKind::Pickup(0), &mut p, &mut maus);
        assert_eq!(o.drops[0].count, 4);
        assert!(maus.is_none());
    }

    #[test]
    fn scrollen_wie_vanilla() {
        let mut c = Creative::default();
        let tab = Tab {
            key: "k".into(),
            kind: TabKind::Category,
            top: true,
            column: 0,
            aligned_right: false,
            show_title: true,
            can_scroll: true,
            background: "x",
            icon: "minecraft:stone".into(),
            name_key: "n".into(),
            visible: true,
            items: (0..100).map(|_| DumpStack { item: "minecraft:stone".into(), count: 1, added: Vec::new(), removed: Vec::new() }).collect(),
        };
        c.set_tabs(vec![tab]);
        c.rebuild(&Registries::default());
        // 100 Items = 12 Zeilen, 7 mehr als sichtbar.
        assert!(c.can_scroll());
        c.scroll_by(-1.0);
        assert_eq!(c.first_row(), 1);
        c.drag_to(1000.0);
        assert_eq!(c.first_row(), 7);
        assert_eq!(c.scroller_offset(), 95.0);
    }
}
