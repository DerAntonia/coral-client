// Inventar oeffnen, schliessen, klicken, ziehen und Klicks an den Server.

use super::*;

impl App {
    // E: Inventar oeffnen bzw. den offenen Bildschirm schliessen. Nur im World-Modus mit
    // stehender Verbindung -- ohne Server gaebe es nichts anzuzeigen.
    pub(crate) fn toggle_inventory(&mut self) {
        if self.mode != AppMode::World || self.conn_status != ConnStatus::Connected {
            return;
        }
        // Ohne Oeffnen/Schliessen-Klang (Nutzerwunsch): das Inventar ist kein Fenster der Oberflaeche.
        if self.inventory.screen_open {
            self.close_inventory();
        } else {
            if self.game_mode == GAME_MODE_CREATIVE {
                self.creative_prepare();
            }
            self.inventory.screen_open = true;
            // Zeiger in die Mitte des mittleren Slots der obersten Inventarreihe (Nutzerwunsch;
            // Vanilla nimmt die Bildschirmmitte). Aus dem echten Layout, also mit der
            // eingestellten Inventar-Skalierung.
            if let Some(screen) = self.gpu.as_ref().map(super::super::super::GpuState::size) {
                let layout = inventory::screen_layout(&self.inventory, screen, self.inventory_scale(screen));
                let pos = inventory::inventory_cursor_spawn(&layout).unwrap_or((screen.0 / 2.0, screen.1 / 2.0));
                self.place_cursor(pos);
            }
            // Und gehaltene Tasten loslassen -- derselbe Grund wie beim vom Server geoeffneten
            // Container (s. ClientEvent::OpenScreen). Hier faellt es weniger auf, weil man das
            // Inventar mit einer TASTE oeffnet und nicht mit der Maustaste, die danach haengt.
            self.clear_controls();
        }
    }

    // Schliessen meldet dem Server das Schliessen des Containers (sonst haelt er ihn offen
    // und wirft z.B. den Cursor-Stack nicht zurueck ins Inventar).
    pub(crate) fn close_inventory(&mut self) {
        self.stash_cursor_in_offhand();
        let sync_id = self.inventory.open.as_ref().map_or(0, |c| c.sync_id);
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::close_container_payload(sync_id));
        }
        // Ein noch laufendes Verteilen-Ziehen zuerst zuruecknehmen: seine Vorschau wurde nie
        // abgeschickt, sie darf also auch nicht stehen bleiben. Erst danach schliessen --
        // sonst zeigte das Inventar beim naechsten Oeffnen eine Verteilung, die es auf dem
        // Server nie gab.
        if let Some(drag) = self.inventory_drag.take() {
            self.inventory_drag_restore(&drag);
        }
        self.quick_craft_pending = None;
        self.creative.drag = None;
        self.inventory.close();
        self.last_inventory_click = None;
        // Sonst verschluckt auch das Schliessen des Inventars ein gehaltenes W: beim Oeffnen wurde
        // losgelassen, beim Schliessen nie wieder uebernommen (s. restore_held_controls).
        self.restore_held_controls();
    }

    // Schliessen mit einem Stapel am Zeiger, fuer den das Inventar keinen Platz mehr hat: der
    // Server wuerde ihn wegwerfen (AbstractContainerMenu.removed legt zurueck, was passt, und
    // wirft den Rest). Stattdessen kommt er in die leere Zweithand. Im eigenen Inventar ist das
    // ein Klick auf sie; in einem Container hat sie keinen Slot, dort geht es ueber einen Platz
    // des Spielerteils: Stapel dort ablegen (das Item von dort kommt an den Zeiger), den Platz
    // mit der Zweithand tauschen, das Item zurueck auf seinen Platz.
    pub(crate) fn stash_cursor_in_offhand(&mut self) {
        if !self.inventory.screen_open || self.creative_shown() {
            return;
        }
        let Some(stapel) = self.inventory.cursor.clone() else { return };
        let zweithand_frei = self.inventory.player.get(inventory::PLAYER_OFFHAND).is_some_and(std::option::Option::is_none);
        if !zweithand_frei || self.inventory_room_for(&stapel) {
            return;
        }
        // Per Taste in die Zweithand: das fliegt (s. inventory_anim).
        self.inventory_anim.get_mut().flights.click_sent(Instant::now());
        use protocol::packets::{CLICK_MODE_PICKUP, CLICK_MODE_SWAP};
        let Some(anzahl) = self.inventory.open.as_ref().map(|c| c.slots.len()) else {
            self.send_container_click(inventory::PLAYER_OFFHAND as i16, 0, CLICK_MODE_PICKUP);
            return;
        };
        // Ein Platz mit einem ANDEREN Item -- mit gleichem wuerde abgelegt statt getauscht.
        let Some(platz) = (anzahl.saturating_sub(36)..anzahl).find(|&i| self.inventory.slot_at_index(i).is_some_and(|s| !s.stackable_with(&stapel))) else {
            return;
        };
        self.send_container_click(platz as i16, 0, CLICK_MODE_PICKUP);
        // Den Tausch mit der Zweithand sagt predict_click im Container nicht voraus (sie hat
        // dort keinen Slot) -- das Ergebnis steht aber fest: der Platz wird leer.
        let vorhersage = inventory::Prediction { modified: vec![(platz as i16, None)], cursor: self.inventory.cursor.clone(), hashable: true };
        let (sync_id, revision, _) = self.inventory.active();
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(
                sync_id,
                revision,
                platz as i16,
                inventory::OFFHAND_SWAP_BUTTON,
                CLICK_MODE_SWAP,
                &vorhersage.modified,
                vorhersage.cursor.as_ref(),
            ));
        }
        self.inventory.apply_prediction(&vorhersage);
        if let Some(zweithand) = self.inventory.player.get_mut(inventory::PLAYER_OFFHAND) {
            *zweithand = Some(stapel);
        }
        self.send_container_click(platz as i16, 0, CLICK_MODE_PICKUP);
    }

    // Passt `stapel` beim Schliessen ganz ins Spielerinventar (Hauptteil und Hotbar, auf freie
    // Plaetze und angebrochene gleiche Stapel)?
    pub(crate) fn inventory_room_for(&self, stapel: &protocol::item::ItemStack) -> bool {
        let (_, _, slots) = self.inventory.active();
        let spielerteil = match &self.inventory.open {
            Some(_) => &slots[slots.len().saturating_sub(36)..],
            None => slots.get(inventory::PLAYER_MAIN_START..inventory::PLAYER_OFFHAND).unwrap_or(&[]),
        };
        let max = world::item_components::max_stack(stapel);
        let platz: i32 = spielerteil
            .iter()
            .map(|s| match s {
                None => max,
                Some(s) if s.stackable_with(stapel) => (max - s.count).max(0),
                Some(_) => 0,
            })
            .sum();
        platz >= stapel.count
    }

    // Das gehaltene Item wegwerfen (im SPIEL, nicht im Inventar -- dort ist es
    // inventory_throw). Ein einzelnes Stueck; zusammen mit der Taste "Stapel verschieben"
    // (auf Wunsch statt fest Strg, s. drop_whole_stack_held) der ganze Stapel.
    pub(crate) fn drop_held_item(&mut self) {
        if self.inventory.screen_open {
            return; // im offenen Inventar wirft der Klick neben das Fenster, nicht Q
        }
        let ganzer_stapel = self.drop_whole_stack_held();
        self.queue_input_action(PendingInputAction::DropItem(ganzer_stapel));
    }

    // Wirft ein Druck auf "wegwerfen" gerade den GANZEN Stapel? Ja, solange die Taste
    // "Stapel verschieben" gehalten wird (ohne eigene Belegung ist das die Schleichtaste, s.
    // KeyBindAction::inventory_fallback) -- so gewuenscht, statt Vanillas festem Strg. Strg
    // gilt zusaetzlich weiter, damit Vanilla-Gewohnheit nicht ins Leere laeuft.
    pub(crate) fn drop_whole_stack_held(&self) -> bool {
        self.keybind_physical_down[KeyBindAction::InventoryMoveStack.index()] || self.modifiers.control_key()
    }

    // ClientPlayerEntity.dropSelectedItem + MinecraftClient.handleInputEvents: das Item
    // verschwindet sofort lokal, das Drop-Paket geht raus, und war etwas in der Hand, schwingt
    // die Haupthand.
    //
    // Frueher ging nur das Paket raus -- die Hotbar aenderte sich erst mit der Serverantwort,
    // und die Hand schwang nicht.
    pub(crate) fn drop_selected_item(&mut self, ganzer_stapel: bool) {
        let slot = inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8);
        // Haupthand leer, Zweithand nicht (kein Vanilla, Wunsch des Nutzers): das Item der
        // Zweithand fliegt, als waere im Inventar Q ueber ihrem Slot gedrueckt worden.
        let belegt = |s: usize| self.inventory.player.get(s).is_some_and(std::option::Option::is_some);
        // Nur im eigenen Inventar (Fenster 0): dort ist Slot 45 die Zweithand.
        if self.inventory.open.is_none() && !belegt(slot) && belegt(inventory::PLAYER_OFFHAND) {
            self.send_container_click(inventory::PLAYER_OFFHAND as i16, i8::from(ganzer_stapel), protocol::packets::CLICK_MODE_THROW);
            return;
        }
        let mut geworfen = false;
        if let Some(eintrag) = self.inventory.player.get_mut(slot) {
            if let Some(stack) = eintrag.as_mut() {
                geworfen = true;
                if ganzer_stapel || stack.count <= 1 {
                    *eintrag = None;
                } else {
                    stack.count -= 1;
                }
            }
        }
        let aktion = if ganzer_stapel { protocol::packets::PLAYER_ACTION_DROP_ALL_ITEMS } else { protocol::packets::PLAYER_ACTION_DROP_ITEM };
        self.send_player_action(aktion);
        if geworfen {
            self.swing_arm();
        }
    }

    // F: Haupt- und Zweithand tauschen.
    pub(crate) fn swap_offhand(&mut self) {
        if self.inventory.screen_open {
            return;
        }
        self.queue_input_action(PendingInputAction::SwapOffhand);
    }

    // Aktiven Hotbar-Slot setzen (Zifferntasten/Mausrad). Rein lokal -- gesendet wird der
    // Slot erst zum Tickbeginn (s. sync_selected_slot), genau wie Vanillas
    // PlayerInventory.setSelectedSlot() nur das Feld setzt und
    // ClientPlayerInteractionManager.tick() daraus ein Paket macht. Deshalb erzeugt auch
    // schnelles Durchscrollen ueber mehrere Slots hoechstens EIN Paket pro Tick, mit dem
    // Endwert -- vorher ging pro Rasterschritt eins raus.
    pub(crate) fn select_hotbar_slot(&mut self, slot: usize) {
        if slot > 8 {
            return;
        }
        // Anderer Slot = anderes Item in der Hand: es kommt von ganz unten und steigt mit der
        // Aufladung, die der Wechsel ebenfalls zuruecksetzt (s. equip_target). Kein UI-Klang:
        // die Hotbar ist Spiel, keine Oberflaeche (Wunsch des Nutzers). Vanilla senkt
        // vorher noch das alte Item; da unser Slot sofort wechselt, waere das nur eine
        // Verzoegerung ohne Bild. Beide Werte setzen, sonst glitte die Hand ab der Tickmitte
        // los und sprange beim naechsten Tick. Nur von leerer zu leerer Hand bleibt die Faust
        // stehen. Vanilla tauscht auch gleiche Items in gleicher Anzahl ohne Senken aus
        // (ItemInHandRenderer.shouldInstantlyReplaceVisibleItem -> ItemStack.
        // matchesIgnoringComponents) -- zwei verschiedene Traenke sind dort dasselbe Item, der
        // Wechsel saehe nach nichts aus (Wunsch des Nutzers: immer animieren). Die Aufladung
        // selbst setzt nur ein anderes Item zurueck (Player.tick: !isSameItem, s.
        // attack_hand_changed).
        let leer = |i: usize| self.inventory.player.get(inventory::PLAYER_HOTBAR_START + i).is_none_or(std::option::Option::is_none);
        if self.inventory.held_slot != slot && !(leer(self.inventory.held_slot) && leer(slot)) {
            self.own_equip_progress = 1.0;
            self.own_last_equip_progress = 1.0;
        }
        self.inventory.held_slot = slot;
    }

    // Mausrad im World-Modus: durch die Hotbar blaettern (Vanilla: hoch = nach links).
    pub(crate) fn scroll_hotbar(&mut self, up: bool) {
        // Das Rad blaettert vom SICHTBAREN Slot aus weiter; wartende Zifferntasten waeren
        // danach nur noch Spruenge zurueck.
        if self.server_still_eating() {
            self.slot_queue.scroll(self.inventory.held_slot, up);
            return;
        }
        self.slot_queue.clear();
        let current = self.inventory.held_slot as i32;
        let schritt = if up { -1 } else { 1 };
        let next = (current + schritt).rem_euclid(9) as usize;
        self.hotbar_glide.get_mut().scrolled(schritt);
        self.select_hotbar_slot(next);
    }

    // Klick auf einen Slot des offenen Bildschirms. Es wird NICHTS lokal vorweggenommen --
    // das Paket geht raus, der Server antwortet mit dem neuen Slotinhalt (s. inventory.rs).
    pub(crate) fn inventory_click(&mut self, button: i8) {
        if self.creative_shown() {
            self.creative_click(button);
            return;
        }
        // Ein neuer Klick baut auf dem aktuellen Stand auf; eine alte Vorschau festzuhalten
        // hiesse, ihn gegen den Server zu verteidigen.
        self.quick_craft_pending = None;
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        if self.recipe_book_click(&layout, button) {
            return;
        }
        let (cx, cy) = self.cursor_pos;
        let (sync_id, revision, _) = self.inventory.active();
        // Handel: Schieber packen oder ein Angebot waehlen (MerchantScreen.mouseClicked und
        // die Knoepfe der Liste).
        if self.open_menu_type() == Some(inventory::MENU_MERCHANT) && button == 0 {
            let s = layout.scale;
            let (bx, by) = (layout.bg_x, layout.bg_y);
            let (anzahl, scroll) = self.inventory.open.as_ref().map_or((0, 0), |c| (c.merchant.offers.len(), c.merchant.scroll));
            // Vanillas Grenzen: rechts und oben offen, unten ein Pixel weiter als die Bahn.
            let gx = (cx - bx) / s;
            let gy = (cy - by) / s;
            if anzahl > inventory::MERCHANT_VISIBLE_OFFERS && gx > 94.0 && gx < 100.0 && gy > 18.0 && gy <= 18.0 + inventory::MERCHANT_SCROLL_TRACK + 1.0 {
                if let Some(c) = self.inventory.open.as_mut() {
                    c.merchant.dragging = true;
                }
                return;
            }
            for i in 0..inventory::MERCHANT_VISIBLE_OFFERS.min(anzahl) {
                let (x, y, w, h) = inventory::merchant_button_rect(&layout, i);
                if cx >= x && cy >= y && cx < x + w && cy < y + h {
                    if let Some(c) = self.inventory.open.as_mut() {
                        c.merchant.focused = Some(i);
                    }
                    self.select_merchant_offer(i + scroll);
                    return;
                }
            }
        }
        // Zaubertisch: Klick auf eine der drei Optionen (EnchantmentScreen.mouseClicked).
        if self.open_menu_type() == Some(inventory::MENU_ENCHANTMENT) && button == 0 {
            for i in 0..3 {
                let (x, y, w, h) = inventory::enchant_option_rect(&layout, i);
                if cx >= x && cy >= y && cx < x + w && cy < y + h {
                    if self.enchant_option_available(i) {
                        if let Some(tx) = &self.outgoing_tx {
                            let _ = tx.send(protocol::packets::container_button_click_payload(sync_id, i as i32));
                        }
                        self.play_ui_sound(ui_sound::UiSound::Click);
                    }
                    return;
                }
            }
        }
        let move_stack = self.action_active(KeyBindAction::InventoryMoveStack);
        // Doppelklick: zweimal dieselbe Taste auf denselben Slot innerhalb von 250ms --
        // Vanillas Fenster (Mouse.field_61505) und Vanillas Bedingung (HandledScreen:
        // `lastClickedSlot == slot && doubled`). Nur die linke Taste, wie dort.
        let now = Instant::now();
        let hovered = layout.slot_at(cx, cy);
        let doppelklick = button == 0
            && hovered.is_some()
            && self.last_inventory_click == hovered.map(|slot| (slot, button))
            && now.duration_since(self.last_inventory_click_at) < INVENTORY_DOUBLE_CLICK;
        self.last_inventory_click = hovered.map(|slot| (slot, button));
        self.last_inventory_click_at = now;
        // ...aber nur, wenn dabei ueberhaupt etwas eingesammelt WIRD.
        //
        // ECHTER BUG gewesen (Items liessen sich nicht schnell nehmen und wieder ablegen): Vanillas PICKUP_ALL tut nichts, wenn nichts Gleichartiges mehr im
        // Inventar liegt (ScreenHandler.internalOnSlotClick prueft `!slot.hasStack()` und
        // sammelt dann nur passende Stapel ein). Bei uns hat aber JEDER zweite Klick auf
        // denselben Slot innerhalb von 250ms diesen Zweig genommen -- also ging ein
        // Leerlauf-Paket raus statt des Ablegens, und wer schnell klickte, bekam den Stapel
        // nie wieder aus der Hand. Gibt es nichts zu sammeln, ist der Doppelklick hier
        // wirkungslos und der Klick zaehlt als gewoehnlicher Klick.
        //
        // Mit dem Stapel-Modifikator bleibt es beim Doppelklick: quick_move_all raeumt auch
        // dann etwas weg, wenn PICKUP_ALL nichts zu holen haette.
        let doppelklick = doppelklick && (move_stack || hovered.is_some_and(|slot| self.inventory.pickup_all_collects(slot)));

        let (slot, mode) = match hovered {
            // Doppelklick mit Stapel-Verschieben: alle gleichen Stapel im selben Bereich
            // wandern hinueber (Vanilla loest dafuer eine QUICK_MOVE-Kette aus, s.
            // quick_move_all). Ohne den Modifikator sammelt der Doppelklick alles Gleiche
            // auf den Zeiger -- PICKUP_ALL.
            Some(slot) if doppelklick => {
                if move_stack {
                    self.quick_move_all(slot);
                    return;
                }
                (slot as i16, protocol::packets::CLICK_MODE_PICKUP_ALL)
            }
            // Stapel verschieben mit einem Item am Zeiger auf dasselbe Item: alle Stapel dieser
            // Sorte wandern hinueber, schon beim ersten Klick -- Vanilla braucht dafuer einen
            // Doppelklick. Der Stapel am Zeiger bleibt dort.
            Some(slot)
                if move_stack
                    && button == 0
                    && self.inventory.cursor.as_ref().is_some_and(|c| self.inventory.slot_at_index(slot).is_some_and(|s| s.stackable_with(c))) =>
            {
                self.quick_move_all(slot);
                return;
            }
            // Voller Zeiger: Vanilla legt hier NICHT sofort ab, sondern beginnt ein
            // moegliches Verteilen-Ziehen (HandledScreen.mouseClicked, Zweig `else {
            // cursorDragging = true }`) und entscheidet erst beim Loslassen, was daraus wurde
            // (s. inventory_release). Das ist die Voraussetzung fuers Ziehen -- der Stapel
            // muss bis dahin am Zeiger bleiben -- fuehlt sich aber traeger an.
            //
            // Mit `inventory_place_on_press` (Standard) wird stattdessen sofort abgelegt und
            // auf das Ziehen verzichtet. Beides zugleich geht nicht: was schon im Slot liegt,
            // laesst sich nicht mehr verteilen.
            // Voller Zeiger: das kann ein einfacher Klick ODER der Anfang eines
            // Verteilen-Ziehens sein -- entschieden wird beim Loslassen (Vanilla:
            // HandledScreen.mouseClicked setzt hier nur cursorDragging).
            //
            // Anders als Vanilla wird die Wirkung aber SOFORT lokal angezeigt (s.
            // inventory_drag_preview): das Item liegt im Slot, sobald man drueckt, und
            // verteilt sich beim Ziehen live weiter. Nur das PAKET geht erst beim Loslassen
            // raus -- vorher steht ja nicht fest, ob es ein Klick oder ein Ziehen wird.
            // Sonst wartet das Ablegen sichtbar, bis man die Maus loslaesst.
            Some(_) | None if self.inventory.cursor.is_some() && (button == 0 || button == 1) => {
                let cursor_before = self.inventory.cursor.clone();
                let mut drag = InventoryDrag { button, slots: Vec::new(), cursor_before, move_stack };
                if let Some(slot) = hovered {
                    drag.slots.push((slot, self.inventory.slot_at_index(slot).cloned()));
                }
                self.inventory_drag = Some(drag);
                self.inventory_drag_preview();
                // Das Item liegt ab jetzt sichtbar im Slot (Vorschau), also springt der Zeiger
                // JETZT in dessen Mitte -- beim Loslassen kaeme der Sprung zu spaet, man saehe
                // bis dahin den Zeiger noch an der alten Stelle. Nicht beim Stapel-Verschieben:
                // dort landet das Item woanders. Und nur, wenn der Zeiger danach LEER ist:
                // haengt noch etwas dran (Rechtsklick legt ein Stueck ab, oder der Slot war
                // fast voll), geht es mit dem Rest weiter -- sonst bleibt der Stapel im
                // Inventar, der Zeiger springt aber trotzdem in die Mitte, und beim
                // Verteilen mit rechts sprang der Zeiger mitten im Ziehen.
                let abgelegt = self.inventory.cursor.is_none();
                if let Some(slot) = hovered.filter(|_| self.settings.inventory_cursor_to_slot && !move_stack && abgelegt) {
                    if let Some(mitte) = layout.slot_center(slot) {
                        self.place_cursor(mitte);
                    }
                }
                return;
            }
            Some(slot) => {
                let mode = if move_stack { protocol::packets::CLICK_MODE_QUICK_MOVE } else { protocol::packets::CLICK_MODE_PICKUP };
                (slot as i16, mode)
            }
            // Klick NEBEN das Fenster wirft den Stapel an der Maus weg -- Vanillas
            // Sonder-Slot -999 mit Modus PICKUP (Button 0 = ganzer Stapel, 1 = ein Stueck).
            // Innerhalb des Fensters (Rand, Ritze zwischen zwei Slots) passiert dagegen
            // nichts: dort etwas wegzuwerfen war ein Bug, kein Vanilla-Verhalten.
            None if layout.is_outside(cx, cy) && self.inventory.cursor.is_some() => {
                (inventory::DROP_SLOT, protocol::packets::CLICK_MODE_PICKUP)
            }
            None => return,
        };
        // Vorhersagen, was der Klick bewirkt -- daraus werden die Hash-Felder des Pakets
        // (s. inventory::predict_click). Klappt das nicht, gehen leere Hashes raus und der
        // Server schickt den neuen Zustand nach.
        let prediction = self.predict_inventory_click(slot, button, mode);
        let (modified, cursor) = match &prediction {
            // Auch nicht hashbare Stapel (eigener Name, Verzauberung ...) gehen mit: der Hash
            // weicht dann absichtlich ab (s. packets::write_item_stack_hash), der Server sieht
            // den Unterschied und schickt seinen Stand. Frueher gingen hier leere Felder und
            // der alte Cursor raus, waehrend lokal schon die Vorhersage galt -- der Server
            // hielt den Client fuer unveraendert und korrigierte nichts: eine Glasscheibe aus
            // einem Server-Menue blieb am Zeiger haengen, statt sofort zurueckzuspringen.
            Some(p) => (p.modified.clone(), p.cursor.clone()),
            // Ohne Vorhersage wird der Cursor gesendet, wie er JETZT ist -- nicht "leer".
            // Das ist immer die sichere Angabe: aendert der Klick den Cursor nicht (Quick-
            // Move, Tauschen), stimmt sie; aendert er ihn doch, weicht sie ab und der Server
            // schickt den neuen Cursor nach. Ein pauschales "leer" war hier ein Bug: legt man
            // etwas in ein Feld, dessen Regeln wir nicht vorhersagen (Werkbank, Ruestung), ist
            // der Server-Cursor danach TATSAECHLICH leer -- die Falschaussage stimmte also
            // zufaellig, die Korrektur blieb aus und das Item klebte lokal am Zeiger fest.
            None => (Vec::new(), self.inventory.cursor.clone()),
        };
        // Shift-Klick: verschoben per Tastatur, das fliegt (s. inventory_anim).
        if mode == protocol::packets::CLICK_MODE_QUICK_MOVE {
            self.inventory_anim.get_mut().flights.click_sent(Instant::now());
        }
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(sync_id, revision, slot, button, mode, &modified, cursor.as_ref()));
        }
        if let Some(prediction) = prediction {
            self.inventory.apply_prediction(&prediction);
        }
    }

    // Maustaste im Inventar losgelassen. Zwei Faelle, genau wie in Vanillas mouseReleased:
    // wurden waehrend des Ziehens Slots eingesammelt, geht die Verteil-Kette raus; sonst war
    // es doch nur ein gewoehnlicher Klick und wird JETZT ausgefuehrt.
    pub(crate) fn inventory_release(&mut self, button: i8) {
        if self.creative_shown() {
            self.creative_release(button.clamp(0, 1) as u8);
            return;
        }
        if button == 0 {
            if let Some(c) = self.inventory.open.as_mut() {
                c.merchant.dragging = false;
            }
        }
        let Some(drag) = self.inventory_drag.take() else { return };
        if drag.button != button {
            return;
        }
        // Ueber MEHRERE Slots gezogen: die Verteil-Kette. Die Vorschau steht lokal schon,
        // sie bleibt bis zur Serverantwort stehen (fuer QUICK_CRAFT gibt es keine
        // Klick-Vorhersage, die sie ersetzen koennte).
        if drag.slots.len() > 1 {
            // ScreenHandler.packQuickCraftData(stage, button) = stage & 3 | (button & 3) << 2.
            // Stufe 0 = Anfang, 1 = ein Slot, 2 = Ende; Anfang und Ende tragen den
            // Sonder-Slot -999. Die Kette geht am Stueck raus, der Server verteilt dann.
            //
            // Der gesendete Cursor ist bis einschliesslich Stufe 1 der Stand VOR dem Ziehen:
            // Vanilla verteilt nichts, solange die Kette laeuft -- Stufe 1 sammelt nur Slots
            // ein (ScreenHandler.internalOnSlotClick, `quickCraftSlots.add(slot)`), erst
            // Stufe 2 raeumt den Zeiger leer. Unsere lokale Vorschau greift dem aber schon
            // beim Ziehen vor, und wer den bereits geleerten Zeiger meldet, widerspricht dem
            // Server: der schickt daraufhin mitten in der Kette den vollen Stapel zurueck an
            // die Maus.
            let vorher = drag.cursor_before.clone();
            self.send_quick_craft_click(inventory::DROP_SLOT, quick_craft_button(0, drag.button), vorher.as_ref());
            for (slot, _) in &drag.slots {
                self.send_quick_craft_click(*slot as i16, quick_craft_button(1, drag.button), vorher.as_ref());
            }
            // Stufe 2 traegt das Ergebnis -- hier stimmt unsere Vorschau mit dem ueberein,
            // was der Server gleich selbst ausrechnet.
            let danach = self.inventory.cursor.clone();
            self.send_quick_craft_click(inventory::DROP_SLOT, quick_craft_button(2, drag.button), danach.as_ref());
            self.quick_craft_pending = Some(QuickCraftPending {
                slots: drag.slots.iter().map(|(slot, _)| (*slot, self.inventory.slot_at_index(*slot).cloned())).collect(),
                cursor: danach,
                remaining: drag.slots.len() + 2,
                since: Instant::now(),
            });
            return;
        }

        // Nur ein Slot (oder keiner) -- es war doch ein gewoehnlicher Klick. Die Vorschau
        // wird dafuer ZURUECKGENOMMEN und der normale Weg gegangen: der rechnet dieselbe
        // Wirkung noch einmal, aber mitsamt den Hash-Feldern fuers Paket (s.
        // send_container_click). Ohne das Zuruecknehmen kaeme die Wirkung doppelt.
        self.inventory_drag_restore(&drag);
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let move_stack = drag.move_stack;
        match layout.slot_at(cx, cy) {
            Some(slot) => {
                let mode = if move_stack { protocol::packets::CLICK_MODE_QUICK_MOVE } else { protocol::packets::CLICK_MODE_PICKUP };
                self.send_container_click(slot as i16, button, mode);
            }
            // Neben das Fenster losgelassen: wegwerfen (s. inventory_click).
            None if layout.is_outside(cx, cy) => {
                self.send_container_click(inventory::DROP_SLOT, button, protocol::packets::CLICK_MODE_PICKUP);
            }
            None => {}
        }
    }

    // Nach einem vollen Container-Abgleich: laeuft noch eine Verteil-Kette und zeigt der
    // Abgleich einen Zwischenstand, die Vorschau wieder darueberlegen (s. QuickCraftPending).
    // Stimmt der Abgleich mit der Vorschau ueberein, ist die Kette bestaetigt; der letzte
    // erwartete Abgleich und die Zeitgrenze beenden sie ebenso -- dann gilt der Server.
    pub(crate) fn hold_quick_craft_preview(&mut self) {
        let Some(p) = self.quick_craft_pending.as_mut() else { return };
        p.remaining = p.remaining.saturating_sub(1);
        let bestaetigt = p.slots.iter().all(|(slot, stack)| self.inventory.slot_at_index(*slot) == stack.as_ref());
        if bestaetigt || p.remaining == 0 || p.since.elapsed() > QUICK_CRAFT_PENDING_MAX {
            self.quick_craft_pending = None;
            return;
        }
        let prediction = inventory::Prediction {
            modified: p.slots.iter().map(|(slot, stack)| (*slot as i16, stack.clone())).collect(),
            cursor: p.cursor.clone(),
            hashable: false,
        };
        self.inventory.apply_prediction(&prediction);
    }

    // Ein Server-Update WAEHREND eines Verteilen-Ziehens: die Vorschau zuruecknehmen, das
    // Update auf den echten Stand anwenden, und die Verteilung darauf neu rechnen. Ohne das
    // ueberschrieb jeder Abgleich (Plugins schicken in Menues laufend welche) die Vorschau --
    // die schon verteilten Items verschwanden aus den Slots und hingen wieder am Zeiger.
    pub(crate) fn with_drag_suspended(&mut self, update: impl FnOnce(&mut Self)) {
        let Some(mut drag) = self.inventory_drag.take() else {
            update(self);
            return;
        };
        self.inventory_drag_restore(&drag);
        update(self);
        for (slot, vorher) in &mut drag.slots {
            *vorher = self.inventory.slot_at_index(*slot).cloned();
        }
        drag.cursor_before = self.inventory.cursor.clone();
        self.inventory_drag = Some(drag);
        self.inventory_drag_preview();
    }

    // Den Stand von VOR dem Ziehen wiederherstellen (s. inventory_drag_preview).
    pub(crate) fn inventory_drag_restore(&mut self, drag: &InventoryDrag) {
        self.inventory.apply_prediction(&inventory::Prediction {
            modified: drag.slots.iter().map(|(slot, inhalt)| (*slot as i16, inhalt.clone())).collect(),
            cursor: drag.cursor_before.clone(),
            hashable: false,
        });
    }

    // Waehrend des Ziehens jeden ueberfahrenen Slot einsammeln. Vanillas Grenze
    // (HandledScreen.mouseDragged): nur solange noch mehr Stuecke am Zeiger haengen als
    // Slots gesammelt sind -- auf mehr Slots als Items laesst sich nichts verteilen.
    pub(crate) fn inventory_drag_collect(&mut self) {
        if self.inventory_drag.is_none() {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let Some(slot) = layout.slot_at(cx, cy) else { return };
        // Was JETZT im Slot liegt, ist bereits die Vorschau -- gemerkt wird der Zustand von
        // VOR dem Ziehen. Fuer einen noch nicht besuchten Slot ist das schlicht sein Inhalt.
        let inhalt = self.inventory.slot_at_index(slot).cloned();
        let am_zeiger = self.inventory.cursor.as_ref().map_or(0, |stack| stack.count);
        let Some(drag) = self.inventory_drag.as_mut() else { return };
        if drag.slots.iter().any(|(besucht, _)| *besucht == slot) {
            return;
        }
        // Vanillas Grenze (HandledScreen.mouseDragged): nur solange noch mehr Stuecke am
        // Zeiger haengen, als Slots gesammelt sind -- auf mehr Slots als Items laesst sich
        // nichts verteilen. Gemessen am Stand VOR dem Ziehen, denn der Zeiger ist in der
        // Vorschau schon (teilweise) geleert.
        let gesamt = drag.cursor_before.as_ref().map_or(am_zeiger, |stack| stack.count);
        if (gesamt as usize) <= drag.slots.len() {
            return;
        }
        drag.slots.push((slot, inhalt));
        self.inventory_drag_preview();
    }

    // Die Vorschau des laufenden Ziehens anwenden: erst den Stand von VOR dem Ziehen
    // wiederherstellen, dann neu verteilen. Ohne das Zuruecksetzen wuerde sich beim vierten
    // Slot die Verteilung des dritten mit einrechnen.
    pub(crate) fn inventory_drag_preview(&mut self) {
        let Some(drag) = self.inventory_drag.as_ref() else { return };
        let Some(cursor) = drag.cursor_before.clone() else { return };
        let vorher: Vec<(usize, Option<protocol::item::ItemStack>)> = drag.slots.clone();
        let (geaendert, rest) = inventory::drag_preview(drag.button, &cursor, &vorher);

        // Zuruecksetzen und dann die neue Verteilung darueberlegen -- beides ueber denselben
        // Weg wie eine Klick-Vorhersage, damit der Container-/Spielerteil-Abgleich (s.
        // inventory::apply_prediction) nicht zweimal existiert.
        let zuruecksetzen = inventory::Prediction {
            modified: vorher.iter().map(|(slot, inhalt)| (*slot as i16, inhalt.clone())).collect(),
            cursor: Some(cursor),
            hashable: false,
        };
        self.inventory.apply_prediction(&zuruecksetzen);
        self.inventory.apply_prediction(&inventory::Prediction { modified: geaendert, cursor: rest, hashable: false });
    }

    // Ein Glied der Verteil-Kette. Eigener Weg statt send_container_click, weil fuer
    // QUICK_CRAFT keine Klick-Vorhersage existiert (der gesendete Cursor haengt an der Stufe,
    // s. inventory_release) und geaenderte Slots dort nie mitgeschickt werden -- Vanilla
    // veraendert waehrend der Kette ja nichts, was es melden koennte.
    pub(crate) fn send_quick_craft_click(&mut self, slot: i16, button: i8, cursor: Option<&protocol::item::ItemStack>) {
        let (sync_id, revision, _) = self.inventory.active();
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(
                sync_id,
                revision,
                slot,
                button,
                protocol::packets::CLICK_MODE_QUICK_CRAFT,
                &[],
                cursor,
            ));
        }
    }

    // inventory::predict_click, davor Vanillas Sperre fuer Ruestung mit Fluch der Bindung
    // (ArmorSlot.canTakeItems: nicht im Kreativmodus, und nur mit PREVENT_ARMOR_CHANGE). Ein
    // gesperrter Klick aendert nichts -- weder Aufnehmen noch Tauschen, Shift-Klick oder
    // Wegwerfen -- und laesst den Slot zittern (s. inventory::shake_offset).
    pub(crate) fn predict_inventory_click(&mut self, slot: i16, button: i8, mode: i32) -> Option<inventory::Prediction> {
        let gesperrt = self.inventory.open.is_none()
            && self.game_mode != GAME_MODE_CREATIVE
            && usize::try_from(slot).ok().filter(|s| (inventory::PLAYER_ARMOR_START..inventory::PLAYER_ARMOR_START + 4).contains(s)).is_some_and(|s| {
                self.inventory.slot_at_index(s).is_some_and(|stack| self.enchantments.prevents_armor_change(stack))
            });
        if gesperrt {
            self.armor_shake = Some((slot as usize, Instant::now()));
            return Some(inventory::locked_prediction(self.inventory.cursor.as_ref()));
        }
        self.inventory.predict_click(slot, button, mode)
    }

    // Ein Klick-Paket bauen und abschicken, samt Vorhersage. Gemeinsamer Unterbau aller
    // Inventar-Aktionen -- die Hash-Felder und die Revision hier zweimal zu schreiben waere
    // die Sorte Verdopplung, an der spaeter genau eine der beiden Stellen vergessen wird.
    pub(crate) fn send_container_click(&mut self, slot: i16, button: i8, mode: i32) {
        let (sync_id, revision, _) = self.inventory.active();
        // BundleItem.playInsertSound & Co.: schon im Client, wie in Vanilla.
        if let Some(klang) = self.inventory.bundle_click_sound(slot, button, mode) {
            self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
            let (laut, hoehe) = if klang.ends_with("_fail") { (1.0, 1.0) } else { (0.8, 0.8 + zufallswert(self.type_pitch_seed) * 0.4) };
            let ort = self.physics.eye_pos();
            self.play_event_at_scaled(klang, ort, laut, hoehe, protocol::packets::SoundCategory::Players);
        }
        let prediction = self.predict_inventory_click(slot, button, mode);
        let (modified, cursor) = match &prediction {
            // Nicht hashbar: trotzdem mitschicken, s. inventory_click.
            Some(p) => (p.modified.clone(), p.cursor.clone()),
            None => (Vec::new(), self.inventory.cursor.clone()),
        };
        // Shift-Klick: verschoben per Tastatur, das fliegt (s. inventory_anim).
        if mode == protocol::packets::CLICK_MODE_QUICK_MOVE {
            self.inventory_anim.get_mut().flights.click_sent(Instant::now());
        }
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(sync_id, revision, slot, button, mode, &modified, cursor.as_ref()));
        }
        if let Some(prediction) = prediction {
            self.inventory.apply_prediction(&prediction);
        }
    }
}
