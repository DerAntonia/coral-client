// Das Kreativinventar in der App: Zeichnen, Klicks, Suchfeld, gespeicherte Hotbars und der
// Mittelklick. Die reine Logik (Tabs, Scrollen, Klickfolgen) steht in creative.rs.

use std::sync::OnceLock;

use winit::keyboard::{Key, NamedKey};

use crate::creative::{self, ClickKind, Target, TabKind};
use crate::item_icons::{SpriteAtlas, UiSprite};
use crate::{inventory, rgb_to_rgba, App, GpuState, Rect, UiRect, UiText, GAME_MODE_CREATIVE, GAME_MODE_SPECTATOR, UI_SCALE_SMALL};

// Der Text des Kreativ-Dumps aus dem Speicher des Launchers (s. assets::stored_creative_tabs).
static CREATIVE_DUMP: OnceLock<String> = OnceLock::new();

pub(crate) fn set_creative_dump(text: String) {
    let _ = CREATIVE_DUMP.set(text);
}

const WHITE: [f32; 4] = [1.0; 4];

impl App {
    // Zeigt das offene Inventar gerade das Kreativinventar? Nur im Kreativmodus, ohne
    // Container und sobald die Tabs da sind.
    pub(crate) fn creative_shown(&self) -> bool {
        self.inventory.screen_open && self.inventory.open.is_none() && self.game_mode == GAME_MODE_CREATIVE && self.creative.ready()
    }

    // Tabs, gespeicherte Hotbars und Server-IDs bereitstellen -- vor dem Oeffnen.
    pub(crate) fn creative_prepare(&mut self) {
        if !self.creative.ready() {
            if let Some(text) = CREATIVE_DUMP.get() {
                self.creative.set_tabs(creative::parse(text));
            }
        }
        self.creative.rebuild(&self.creative_registries);
        // Befehlsbloecke & Co. nur mit Option und Operator-Stufe ab 2 (Vanillas hasPermissions).
        self.creative.set_op_allowed(self.settings.creative_op_tab && self.op_level >= 2);
    }

    pub(crate) fn creative_layout(&self, screen: (f32, f32)) -> Option<inventory::ScreenLayout> {
        let tab = self.creative.tab()?;
        Some(creative::layout(screen, self.inventory_scale(screen), tab.kind, tab.background))
    }

    // Was in diesem Slot des Kreativfensters liegt (Rasterzelle, Spielerslot, Muelleimer).
    fn creative_slot_stack(&self, index: usize) -> Option<protocol::item::ItemStack> {
        match index {
            creative::SLOT_DESTROY => None,
            i if i >= creative::SLOT_GRID => self.creative.cell_item(i - creative::SLOT_GRID),
            i => self.inventory.player.get(i).cloned().flatten(),
        }
    }

    fn push_creative_tab(&self, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>, tab: &creative::Tab, selected: bool, o: (f32, f32), scale: f32) {
        let name = format!(
            "container/creative_inventory/tab_{}_{}_{}",
            if tab.top { "top" } else { "bottom" },
            if selected { "selected" } else { "unselected" },
            tab.column.min(6) + 1
        );
        let (x, y) = creative::tab_draw_pos(tab, o, scale);
        if let Some(r) = self.ui_atlas.rect(&name) {
            sprites.push(UiSprite::quad(x, y, creative::TAB_W * scale, creative::TAB_H * scale, r.uv, WHITE, SpriteAtlas::Ui));
        }
        if let Some(icon) = self.creative.icon(tab) {
            let (ix, iy) = creative::tab_icon_pos(tab, o, scale);
            self.push_item(sprites, texts, &icon, ix, iy, inventory::ICON_SIZE * scale);
        }
    }

    fn creative_tab_name(tab: &creative::Tab) -> String {
        protocol::nbt::translate(&tab.name_key).unwrap_or_else(|| tab.key.rsplit(':').next().unwrap_or(&tab.key).replace('_', " "))
    }

    // CreativeModeInventoryScreen.render: Reiter, Fenster, Schieber, Suchfeld, Slots, der Stapel
    // an der Maus und die Tooltips. Rueckgabe wie build_inventory_screen: wo die Spielerfigur
    // in die Sprite-Liste gehoert (nur im Inventar-Reiter).
    pub(crate) fn build_creative_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) -> Option<usize> {
        let screen = gpu.size();
        let tab = self.creative.tab()?;
        let layout = self.creative_layout(screen)?;
        let scale = layout.scale;
        let o = (layout.bg_x, layout.bg_y);
        let dim = self.settings.inventory_transparency as f32 / 100.0;
        if dim > 0.0 {
            rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: screen.0, h: screen.1 }, color: [0.0, 0.0, 0.0, dim], ..UiRect::FLAT });
        }
        // Erst die anderen Reiter, dann das Fenster darueber, zuletzt der gewaehlte Reiter --
        // so verbindet er sich mit dem Fenster, die anderen liegen dahinter.
        for (i, t) in self.creative.tabs.iter().enumerate() {
            if self.creative.shows(t) && i != self.creative.selected {
                self.push_creative_tab(sprites, texts, t, false, o, scale);
            }
        }
        if let Some(r) = self.ui_atlas.rect(tab.background) {
            sprites.push(UiSprite::quad(o.0, o.1, layout.bg_w, layout.bg_h, r.sub(0, 0, creative::SHEET_W as u32, creative::SHEET_H as u32), WHITE, SpriteAtlas::Ui));
        }
        if tab.can_scroll {
            let name = if self.creative.can_scroll() { "container/creative_inventory/scroller" } else { "container/creative_inventory/scroller_disabled" };
            if let Some(r) = self.ui_atlas.rect(name) {
                let y = creative::SCROLLER_Y + self.creative.scroller_offset();
                sprites.push(UiSprite::quad(o.0 + creative::SCROLLER_X * scale, o.1 + y * scale, creative::SCROLLER_W * scale, creative::SCROLLER_H * scale, r.uv, WHITE, SpriteAtlas::Ui));
            }
        }
        self.push_creative_tab(sprites, texts, tab, true, o, scale);
        if tab.show_title {
            texts.push(UiText { text: Self::creative_tab_name(tab), x: (o.0 + 8.0 * scale).round(), y: (o.1 + 6.0 * scale).round(), color: rgb_to_rgba(0x404040), scale });
        }
        // Das Suchfeld hat im Suchreiter immer den Fokus: Text plus blinkender Strich.
        if tab.kind == TabKind::Search {
            let blink = (self.start_time.elapsed().as_millis() / 300).is_multiple_of(2);
            let mut text = self.creative.search.clone();
            while gpu.measure_text(&text, scale) > creative::SEARCH_W * scale && !text.is_empty() {
                text.remove(0);
            }
            if blink {
                text.push('_');
            }
            crate::push_typed_marker(texts);
            texts.push(UiText { text, x: (o.0 + creative::SEARCH_X * scale).round(), y: (o.1 + creative::SEARCH_Y * scale).round(), color: WHITE, scale });
        }
        let doll_split = layout.doll.map(|_| sprites.len());

        let (cx, cy) = self.cursor_pos;
        // Vorschau eines laufenden Verteilens: so saehe es nach dem Loslassen aus.
        let vorschau = self.creative.drag.as_ref().filter(|d| d.distributes()).map(|d| d.result(&self.inventory.player));
        let mut hovered = None;
        for slot in &layout.slots {
            let ersatz = vorschau.as_ref().and_then(|(neu, _)| neu.iter().find(|(i, _)| *i == slot.index)).map(|(_, s)| s.clone());
            if ersatz.is_some() {
                rects.push(UiRect { rect: Rect { x: slot.x, y: slot.y, w: slot.size, h: slot.size }, color: [1.0, 1.0, 1.0, 0.25], ..UiRect::FLAT });
            }
            let Some(stack) = ersatz.or_else(|| self.creative_slot_stack(slot.index)) else { continue };
            self.push_item(sprites, texts, &stack, slot.x, slot.y, slot.size);
            if slot.contains(cx, cy) {
                hovered = self.item_hover_tooltip(&stack).map(|zeilen| (zeilen, *slot));
            }
        }
        if let Some(slot) = layout.slots.iter().find(|s| s.contains(cx, cy)) {
            rects.push(UiRect { rect: Rect { x: slot.x, y: slot.y, w: slot.size, h: slot.size }, color: [1.0, 1.0, 1.0, 0.25], ..UiRect::FLAT });
        }
        // Der Name eines Reiters unter dem Zeiger (checkTabHovering).
        let reiter = self.creative.tabs.iter().filter(|t| self.creative.shows(t)).find(|t| creative::tab_hit(t, o, scale, cx, cy)).map(Self::creative_tab_name);

        let maus = match &vorschau {
            Some((_, rest)) => rest.clone(),
            None => self.inventory.cursor.clone(),
        };
        if let Some(stack) = &maus {
            let size = inventory::ICON_SIZE * scale;
            self.push_item(sprites, texts, stack, cx - size / 2.0, cy - size / 2.0, size);
        } else if let Some(text) = reiter {
            let breite = gpu.measure_text(&text, UI_SCALE_SMALL);
            let h = gpu.text_height(UI_SCALE_SMALL);
            let (x0, y0) = ((cx + 16.0).round(), (cy - 4.0).round());
            rects.push(UiRect { rect: Rect { x: x0 - 4.0, y: y0 - 4.0, w: breite + 8.0, h: h + 8.0 }, color: [0.06, 0.0, 0.06, 0.94], ..UiRect::FLAT });
            texts.push(UiText { text, x: x0, y: y0, color: WHITE, scale: UI_SCALE_SMALL });
        } else if let Some((zeilen, slot)) = hovered {
            let (x, center_y, right_aligned) = layout.tooltip_anchor(&slot);
            let zeilen_h = gpu.text_height(UI_SCALE_SMALL);
            let mut y = center_y - zeilen_h * zeilen.len() as f32 / 2.0;
            for zeile in zeilen {
                let breite: f32 = zeile.iter().map(|span| gpu.measure_text(&span.text, UI_SCALE_SMALL)).sum();
                let mut sx = if right_aligned { x - breite } else { x };
                for span in zeile {
                    let w = gpu.measure_text(&span.text, UI_SCALE_SMALL);
                    let color = span.color.map(rgb_to_rgba).unwrap_or(WHITE);
                    crate::push_span_shadow(texts, span.shadow);
                    texts.push(UiText { text: span.text, x: sx.round(), y: y.round(), color, scale: UI_SCALE_SMALL });
                    sx += w;
                }
                y += zeilen_h;
            }
        }
        doll_split
    }

    // Wohin der Zeiger zeigt: Rasterzelle, Spielerslot, Muelleimer oder daneben. None = auf
    // dem Fenster, aber auf keinem Slot (oder auf einem Reiter).
    fn creative_target_at(&self, px: f32, py: f32) -> Option<Target> {
        let gpu = self.gpu.as_ref()?;
        let layout = self.creative_layout(gpu.size())?;
        let o = (layout.bg_x, layout.bg_y);
        if self.creative.tabs.iter().any(|t| self.creative.shows(t) && creative::tab_hit(t, o, layout.scale, px, py)) {
            return None;
        }
        match layout.slot_at(px, py) {
            Some(creative::SLOT_DESTROY) => Some(Target::Destroy),
            Some(i) if i >= creative::SLOT_GRID => Some(Target::Grid(i - creative::SLOT_GRID)),
            Some(i) => Some(Target::Player(i)),
            None if layout.is_outside(px, py) => Some(Target::Outside),
            None => None,
        }
    }

    // Einen Klick ausfuehren und jeden geaenderten Slot an den Server melden.
    fn creative_apply(&mut self, target: Target, kind: ClickKind) {
        let cell = match target {
            Target::Grid(c) => self.creative.cell_item(c),
            _ => None,
        };
        let tab = self.creative.kind();
        let mut player = std::mem::take(&mut self.inventory.player);
        let mut cursor = self.inventory.cursor.take();
        let out = creative::click(tab, target, cell.as_ref(), kind, &mut player, &mut cursor);
        self.inventory.player = player;
        self.inventory.cursor = cursor;
        let mut gesendet = Vec::new();
        for i in out.changed {
            if gesendet.contains(&i) {
                continue;
            }
            gesendet.push(i);
            let stack = self.inventory.player.get(i).cloned().flatten();
            self.send_creative_slot(i as i16, stack.as_ref());
        }
        for stack in &out.drops {
            self.send_creative_slot(-1, Some(stack));
        }
    }

    // MultiPlayerGameMode.handleCreativeModeItemAdd/-Drop: -1 wirft den Stapel aus.
    pub(crate) fn send_creative_slot(&mut self, slot: i16, stack: Option<&protocol::item::ItemStack>) {
        let Some(tx) = &self.outgoing_tx else { return };
        match protocol::packets::set_creative_mode_slot_payload(slot, stack) {
            Some(bytes) => {
                let _ = tx.send(bytes);
            }
            None => eprintln!("[kreativ] Stapel in Slot {slot} laesst sich nicht schicken (unbekannte Component)"),
        }
    }

    // Maustaste gedrueckt im Kreativfenster.
    pub(crate) fn creative_click(&mut self, button: i8) {
        let (px, py) = self.cursor_pos;
        let Some(gpu) = self.gpu.as_ref() else { return };
        let Some(layout) = self.creative_layout(gpu.size()) else { return };
        let o = (layout.bg_x, layout.bg_y);
        if button == 0 {
            if let Some(i) = self.creative.tabs.iter().position(|t| self.creative.shows(t) && creative::tab_hit(t, o, layout.scale, px, py)) {
                if i != self.creative.selected {
                    self.creative.select(i);
                    self.play_ui_sound(crate::ui_sound::UiSound::Click);
                }
                return;
            }
            if self.creative.can_scroll() && creative::on_scrollbar(o, layout.scale, px, py) {
                self.creative.scrolling = true;
                self.creative_drag();
                return;
            }
        }
        let Some(target) = self.creative_target_at(px, py) else { return };
        let schnell = self.action_active(crate::KeyBindAction::InventoryMoveStack);
        // Mit einem Stapel an der Maus entscheidet erst das Loslassen: Verteilen oder Klick.
        if let (Some(cursor), false) = (self.inventory.cursor.clone(), schnell) {
            self.creative.drag = Some(creative::Drag { button: button.clamp(0, 1) as u8, slots: Vec::new(), cursor });
            self.creative_drag_collect();
            return;
        }
        let kind = if schnell { ClickKind::QuickMove } else { ClickKind::Pickup(button.clamp(0, 1) as u8) };
        self.creative_apply(target, kind);
    }

    // Maustaste los (0 links, 1 rechts, 2 Mitte).
    pub(crate) fn creative_release(&mut self, button: u8) {
        self.creative.scrolling = false;
        let Some(drag) = self.creative.drag.take_if(|d| d.button == button) else { return };
        if drag.distributes() {
            let (neu, rest) = drag.result(&self.inventory.player);
            for (slot, stack) in neu {
                self.inventory.player[slot] = Some(stack.clone());
                self.send_creative_slot(slot as i16, Some(&stack));
            }
            self.inventory.cursor = rest;
        } else if button < 2 {
            let (px, py) = self.cursor_pos;
            if let Some(target) = self.creative_target_at(px, py) {
                self.creative_apply(target, ClickKind::Pickup(button));
            }
        }
    }

    // Beim Ziehen jeden ueberfahrenen Spielerslot mitnehmen, der passt.
    fn creative_drag_collect(&mut self) {
        let (px, py) = self.cursor_pos;
        let Some(Target::Player(slot)) = self.creative_target_at(px, py) else { return };
        let inhalt = self.inventory.player.get(slot).cloned().flatten();
        if let Some(drag) = self.creative.drag.as_mut().filter(|d| d.accepts(slot, inhalt.as_ref())) {
            drag.slots.push(slot);
        }
    }

    // Zeiger bewegt: Schieber ziehen bzw. Slots fuers Verteilen sammeln.
    pub(crate) fn creative_drag(&mut self) {
        if !self.creative_shown() {
            return;
        }
        if self.creative.drag.is_some() {
            self.creative_drag_collect();
        }
        if !self.creative.scrolling {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let Some(layout) = self.creative_layout(gpu.size()) else { return };
        let rel = (self.cursor_pos.1 - layout.bg_y) / layout.scale - creative::SCROLLER_Y;
        self.creative.drag_to(rel);
    }

    // Zifferntaste (oder Zweithand-Taste) ueber einem Slot.
    pub(crate) fn creative_swap(&mut self, taste: usize) {
        let (px, py) = self.cursor_pos;
        if let Some(target @ (Target::Grid(_) | Target::Player(_))) = self.creative_target_at(px, py) {
            self.creative_apply(target, ClickKind::Swap(taste as u8));
        }
    }

    // Wegwerf-Taste ueber einem Slot.
    pub(crate) fn creative_throw(&mut self) {
        let (px, py) = self.cursor_pos;
        let whole = self.drop_whole_stack_held();
        if let Some(target @ (Target::Grid(_) | Target::Player(_))) = self.creative_target_at(px, py) {
            self.creative_apply(target, ClickKind::Throw { whole });
        }
    }

    // Mittelklick ueber einem Slot: einen vollen Stapel an die Maus -- oder, mit einem Stapel
    // an der Maus, volle Stapel durch Ziehen verteilen.
    pub(crate) fn creative_clone(&mut self) {
        if let Some(cursor) = self.inventory.cursor.clone() {
            self.creative.drag = Some(creative::Drag { button: 2, slots: Vec::new(), cursor });
            self.creative_drag_collect();
            return;
        }
        let (px, py) = self.cursor_pos;
        if let Some(target @ (Target::Grid(_) | Target::Player(_))) = self.creative_target_at(px, py) {
            self.creative_apply(target, ClickKind::Clone);
        }
    }

    // Tippt man gerade ins Suchfeld? (Dann gehoeren die Tasten dem Feld, nicht dem Spiel.)
    pub(crate) fn creative_search_focused(&self) -> bool {
        self.creative_shown() && self.creative.kind() == TabKind::Search
    }

    pub(crate) fn creative_search_key(&mut self, key: &Key, text: Option<&str>, control: bool, paste: bool) {
        match key {
            Key::Named(NamedKey::Escape) => self.close_inventory(),
            Key::Named(NamedKey::Backspace) => self.creative.backspace(control),
            _ => {
                let einfuegen = if paste {
                    arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok())
                } else if control {
                    None
                } else {
                    text.map(str::to_string)
                };
                for ch in einfuegen.unwrap_or_default().chars() {
                    self.creative.type_char(ch);
                }
            }
        }
    }

    // Vanillas Mittelklick im Spiel (Minecraft.pickBlock): seit 1.21.4 sucht der Server das
    // Item -- im Kreativmodus legt er es neu an, sonst holt er es aus dem Inventar. Mit Strg
    // im Kreativmodus samt Daten (Kisteninhalt ...).
    pub(crate) fn pick_block_or_entity(&mut self) {
        if self.game_mode == GAME_MODE_SPECTATOR {
            return;
        }
        let mit_daten = self.modifiers.control_key() && self.game_mode == GAME_MODE_CREATIVE;
        let paket = match self.crosshair_target() {
            crate::CrosshairTarget::Block(pos, _) => protocol::packets::pick_item_from_block_payload(pos, mit_daten),
            crate::CrosshairTarget::Entity(id) => protocol::packets::pick_item_from_entity_payload(id, mit_daten),
            crate::CrosshairTarget::Miss => return,
        };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(paket);
        }
    }
}
