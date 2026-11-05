// Rezeptbuch, Werfen aus dem Inventar, Schnellverschieben und Hotbar-Tausch.

use super::*;

impl App {
    // Kantenlaenge des Craftingfelds, wenn der offene Bildschirm ein Rezeptbuch hat: 2 = das
    // eigene Inventar, 3 = die Werkbank.
    pub(crate) const fn recipe_grid(&self) -> Option<i32> {
        if !self.inventory.screen_open || self.creative_shown() {
            return None;
        }
        match &self.inventory.open {
            None => Some(2),
            Some(c) if c.screen_type == inventory::MENU_CRAFTING => Some(3),
            Some(_) => None,
        }
    }

    // Knopf zum Oeffnen des Buchs im Fenster (Bildschirmpixel).
    pub(crate) fn recipe_button_rect(&self, layout: &inventory::ScreenLayout) -> Option<Rect> {
        let kante = self.recipe_grid()?;
        let (gx, gy) = recipe_book::button_offset(kante);
        let s = layout.scale;
        Some(Rect { x: layout.bg_x + gx * s, y: layout.bg_y + gy * s, w: recipe_book::BUTTON_W * s, h: recipe_book::BUTTON_H * s })
    }

    // Die Truhe, wenn das Buch offen ist.
    pub(crate) fn recipe_panel(&self, layout: &inventory::ScreenLayout) -> Option<recipe_book::Panel> {
        self.recipe_grid()?;
        self.recipe_book.open.then(|| recipe_book::Panel::beside(layout.bg_x, layout.bg_y, layout.scale))
    }

    // Was zum Craften da ist: Inventar und Hotbar plus das Craftingfeld (das gibt ein neues
    // Rezept zuerst zurueck). Nicht Ruestung, Zweithand und das Ergebnisfeld.
    pub(crate) fn recipe_stock(&self) -> std::collections::HashMap<u32, i32> {
        let (_, _, slots) = self.inventory.active();
        let haupt = slots.len().saturating_sub(36);
        let feld = match self.recipe_grid() {
            Some(2) => 1..5,
            Some(_) => 1..10,
            None => 0..0,
        };
        recipe_book::stock(slots.iter().enumerate().filter(|(i, _)| feld.contains(i) || (*i >= haupt && *i < haupt + 36)).filter_map(|(_, s)| s.as_ref()))
    }

    pub(crate) fn recipe_shown(&self) -> Vec<recipe_book::Shown> {
        let Some(kante) = self.recipe_grid() else { return Vec::new() };
        self.recipe_book.shown(kante, &self.recipe_stock())
    }

    // Klick in die Truhe. Links: einmal craften, mit Stapel-Modifikator einen Stapel des
    // Ergebnisses; rechts: nur ins Feld legen (so viel wie geht). Das Ergebnis holt sich der
    // Client ueber Klicks auf das Ergebnisfeld; die gehen ohne Vorhersage raus, weil das Feld
    // erst auf dem Server gefuellt wird -- der Server gleicht den Stand danach ab.
    pub(crate) fn recipe_click(&mut self, shown: recipe_book::Shown, button: i8) {
        use protocol::packets::{click_container_payload, CLICK_MODE_PICKUP, CLICK_MODE_QUICK_MOVE};
        use protocol::recipe::place_recipe_payload;
        let Some(entry) = self.recipe_book.entries.get(shown.index).cloned() else { return };
        let Some(tx) = self.outgoing_tx.clone() else { return };
        let (sync_id, revision, slots) = self.inventory.active();
        let klick = |slot: i16, mode: i32| click_container_payload(sync_id, revision, slot, 0, mode, &[], None);
        if button == 1 {
            let _ = tx.send(place_recipe_payload(sync_id, entry.id, true));
            self.play_ui_sound(ui_sound::UiSound::Click);
            return;
        }
        // Craften braucht eine freie Hand (das Ergebnis kommt ueber den Zeiger) und Zutaten.
        if button != 0 || self.inventory.cursor.is_some() || shown.sets == 0 {
            return;
        }
        let Some(ergebnis) = entry.display.result().first_stack() else { return };
        let je_satz = ergebnis.count.max(1);
        let stapel = world::item_components::max_stack(&ergebnis);
        let saetze = if self.action_active(KeyBindAction::InventoryMoveStack) { (stapel / je_satz).max(1) } else { 1 };
        // Ein Stueck je Satz und nie mehr Saetze im Feld als gewollt: dann raeumt ein
        // Schnell-Verschieben des Ergebnisses genau einen Stapel ins Inventar.
        if saetze > 1 && je_satz == 1 && self.recipe_book.sets_from(&entry, &self.recipe_stock(), 64) <= saetze {
            let _ = tx.send(place_recipe_payload(sync_id, entry.id, true));
            let _ = tx.send(klick(0, CLICK_MODE_QUICK_MOVE));
            self.play_ui_sound(ui_sound::UiSound::Click);
            return;
        }
        // Sonst Satz fuer Satz auf den Zeiger und von dort in einen freien Platz: der letzte
        // freie im Inventar (das Feld gibt beim Neubelegen Reste vorn in die Hotbar zurueck).
        let haupt = slots.len().saturating_sub(36);
        let ziel = (haupt..haupt + 27).rev().chain((haupt + 27..haupt + 36).rev()).find(|&i| slots.get(i).is_some_and(std::option::Option::is_none));
        let _ = tx.send(place_recipe_payload(sync_id, entry.id, saetze > 1));
        for _ in 0..saetze {
            let _ = tx.send(klick(0, CLICK_MODE_PICKUP));
        }
        // Kein Platz: das Ergebnis bleibt am Zeiger, statt irgendwo getauscht zu werden.
        if let Some(ziel) = ziel {
            let _ = tx.send(klick(ziel as i16, CLICK_MODE_PICKUP));
        }
        self.play_ui_sound(ui_sound::UiSound::Click);
    }

    // Klick auf Knopf oder Truhe des Rezeptbuchs; true = verbraucht.
    pub(crate) fn recipe_book_click(&mut self, layout: &inventory::ScreenLayout, button: i8) -> bool {
        let (cx, cy) = self.cursor_pos;
        if self.recipe_button_rect(layout).is_some_and(|r| r.contains(cx, cy)) {
            if button == 0 {
                self.recipe_book.open = !self.recipe_book.open;
                self.play_ui_sound(ui_sound::UiSound::Click);
            }
            return true;
        }
        let Some(panel) = self.recipe_panel(layout).filter(|p| p.contains(cx, cy)) else { return false };
        let (fx, fy, fw, fh) = panel.filter();
        if button == 0 && (Rect { x: fx, y: fy, w: fw, h: fh }).contains(cx, cy) {
            self.recipe_book.craftable_first = !self.recipe_book.craftable_first;
            self.recipe_book.scroll = 0;
            self.play_ui_sound(ui_sound::UiSound::Click);
            return true;
        }
        if let Some(i) = panel.slot_at(cx, cy) {
            let shown = self.recipe_shown();
            if let Some(&eintrag) = shown.get(self.recipe_book.row(shown.len()) * recipe_book::COLS + i) {
                self.recipe_click(eintrag, button);
            }
        }
        true
    }

    // Den Stapel unter dem Zeiger wegwerfen (SlotActionType.THROW). Vanilla:
    //     onMouseClick(slot, slot.id, hasControlDown() ? 1 : 0, SlotActionType.THROW)
    // Button 0 wirft EIN Stueck, Button 1 den ganzen Stapel -- deshalb liegt der ganze
    // Stapel auf Strg. Nur mit LEEREM Zeiger: haelt man etwas, ist Vanillas Wurf der Klick
    // ins Leere (s. inventory_click), nicht diese Aktion.
    //
    // Anders als Vanilla zaehlt ein Stapel am Zeiger als der Stapel unter dem Zeiger: er wird
    // geworfen (ein Stueck, mit "Stapel verschieben" ganz) -- ueber den Wurf-Slot -999.
    pub(crate) fn inventory_throw(&mut self) {
        if self.creative_shown() {
            self.creative_throw();
            return;
        }
        if !self.inventory.screen_open {
            return;
        }
        if self.drop_whole_stack_held() {
            self.inventory_throw_sweep(true);
            return;
        }
        if self.inventory.cursor.is_some() {
            self.send_container_click(inventory::DROP_SLOT, 1, protocol::packets::CLICK_MODE_PICKUP);
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let Some(slot) = layout.slot_at(cx, cy) else { return };
        self.send_container_click(slot as i16, 0, protocol::packets::CLICK_MODE_THROW);
    }

    // Stapel-Wurf: solange Wegwerfen und "Stapel verschieben" beide gehalten sind, fliegt jeder
    // Stapel, ueber den der Zeiger faehrt, sofort ganz raus -- je Betreten eines Slots einmal.
    // Fuellt sich der Slot unter dem Zeiger neu, bleibt das liegen, bis der Zeiger ihn verlaesst
    // und wieder betritt oder eine der beiden Tasten neu gedrueckt wird (`neu`). Der Stapel am
    // Zeiger zaehlt als immer ueberfahren: er fliegt beim Druck mit.
    pub(crate) fn inventory_throw_sweep(&mut self, neu: bool) {
        if !self.inventory.screen_open || self.creative_shown() {
            return;
        }
        let gehalten = self.keybind_physical_down[KeyBindAction::InventoryDropItem.index()] && self.drop_whole_stack_held();
        if !gehalten {
            self.throw_sweep_slot = None;
            return;
        }
        if neu {
            self.throw_sweep_slot = None;
            if self.inventory.cursor.is_some() {
                self.send_container_click(inventory::DROP_SLOT, 0, protocol::packets::CLICK_MODE_PICKUP);
            }
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let slot = layout.slot_at(cx, cy);
        if slot == self.throw_sweep_slot {
            return;
        }
        self.throw_sweep_slot = slot;
        if let Some(slot) = slot.filter(|&s| self.inventory.slot_at_index(s).is_some()) {
            self.send_container_click(slot as i16, 1, protocol::packets::CLICK_MODE_THROW);
        }
    }

    // Stapel-Verschieben per Doppelklick: ALLE Stapel derselben Sorte wandern hinueber.
    // Vanilla schickt dafuer keine eigene Aktion, sondern eine Kette einzelner QUICK_MOVEs
    // (HandledScreen.mouseClicked, Zweig `doubleClicking && hasShift`) -- genau das hier.
    //
    // Vanilla beschraenkt die Kette auf Slots DESSELBEN Inventars wie der angeklickte
    // (`slot2.inventory == slot.inventory`): ein Doppelklick in der Kiste raeumt die Kiste,
    // nicht zusaetzlich den eigenen Rucksack. Bei uns ist die Grenze dieselbe -- der
    // Spielerteil eines Containers sind die letzten 36 Slots (s. inventory::set_slot).
    pub(crate) fn quick_move_all(&mut self, slot: usize) {
        let Some(vorlage) = self.inventory.slot_at_index(slot).cloned() else { return };
        let (_, _, slots) = self.inventory.active();
        let anzahl = slots.len();
        let spieler_start = if self.inventory.screen_open { anzahl.saturating_sub(36) } else { 0 };
        let im_spielerteil = |index: usize| index >= spieler_start;
        let ziel_ist_spieler = im_spielerteil(slot);

        let betroffen: Vec<usize> = (0..anzahl)
            .filter(|&index| im_spielerteil(index) == ziel_ist_spieler)
            .filter(|&index| slots[index].as_ref().is_some_and(|stack| stack.stackable_with(&vorlage)))
            .collect();
        for index in betroffen {
            self.send_container_click(index as i16, 0, protocol::packets::CLICK_MODE_QUICK_MOVE);
        }
    }

    // Zifferntaste ueber einem Slot: Vanillas Tauschen mit dem entsprechenden Hotbar-Platz
    // (Modus SWAP, Button = Hotbar-Index).
    pub(crate) fn inventory_swap_with_hotbar(&mut self, hotbar_index: usize) {
        if self.creative_shown() {
            self.creative_swap(hotbar_index);
            return;
        }
        // Zifferntaste oder Zweithand-Taste: verschoben per Tastatur, das fliegt (s. inventory_anim).
        self.inventory_anim.get_mut().flights.click_sent(Instant::now());
        // Mit einem Stapel am Zeiger tut Vanilla hier nichts. Bei uns kommt er auf den
        // Hotbar-Platz, und was dort lag, an den Zeiger -- ein gewoehnlicher Klick auf den Platz.
        if self.inventory.cursor.is_some() {
            if let Some(ziel) = self.inventory.hotbar_screen_slot(hotbar_index) {
                self.send_container_click(ziel as i16, 0, protocol::packets::CLICK_MODE_PICKUP);
            }
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let Some(slot) = layout.slot_at(cx, cy) else { return };
        // Craft-Ergebnis auf einen belegten Platz: Vanillas Tausch scheitert, weil ins
        // Ergebnisfeld nichts hinein darf. Stattdessen erst das Item vom Platz an den Zeiger
        // nehmen, dann das Ergebnis per Tausch auf den nun leeren Platz.
        let ergebnisfeld = slot == 0 && (self.inventory.open.is_none() || self.open_menu_type() == Some(inventory::MENU_CRAFTING));
        if ergebnisfeld && self.inventory.slot_at_index(0).is_some() {
            if let Some(ziel) = self.inventory.hotbar_screen_slot(hotbar_index).filter(|&z| self.inventory.slot_at_index(z).is_some()) {
                self.send_container_click(ziel as i16, 0, protocol::packets::CLICK_MODE_PICKUP);
                self.send_container_click(0, hotbar_index as i8, protocol::packets::CLICK_MODE_SWAP);
                return;
            }
        }
        let (sync_id, revision, _) = self.inventory.active();
        // Wie bei jedem anderen Klick erst vorhersagen, dann senden, dann lokal anwenden --
        // vorher ging hier NUR das Paket raus und die Anzeige stand still, bis der Server
        // antwortete, und Hotbar-Swaps mit der Taste wirkten langsam.
        let prediction = self.predict_inventory_click(slot as i16, hotbar_index as i8, protocol::packets::CLICK_MODE_SWAP);
        let (modified, cursor) = match &prediction {
            // Nicht hashbar: trotzdem mitschicken, s. inventory_click.
            Some(p) => (p.modified.clone(), p.cursor.clone()),
            // Tauschen laesst den Cursor unveraendert -- den koennen wir also immer exakt
            // melden, die getauschten Slots ohne Vorhersage nicht.
            None => (Vec::new(), self.inventory.cursor.clone()),
        };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(
                sync_id,
                revision,
                slot as i16,
                hotbar_index as i8,
                protocol::packets::CLICK_MODE_SWAP,
                &modified,
                cursor.as_ref(),
            ));
        }
        if let Some(prediction) = prediction {
            self.inventory.apply_prediction(&prediction);
        }
    }
}
