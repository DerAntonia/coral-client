// Eigene Bildschirme: Buch, Schild, Inventar, Haendler, Amboss, Verzauberung, Tooltips.

use crate::*;

impl App {
    // Liegt ein Bildschirm-Overlay ueber der Welt? Inventar und Modul-Menue verhalten sich
    // gleich: Maus bewegt den Zeiger statt den Kopf, Bewegungseingabe ist gesperrt, kein
    // Fadenkreuz. Eine Quelle dafuer, damit ein neues Overlay nicht die Haelfte der Stellen
    // vergisst -- ein Overlay, das die Eingabe nur halb sperrt, laeuft Grim sofort auf.
    pub(crate) fn screen_overlay_open(&self) -> bool {
        self.inventory.screen_open
            || self.module_menu_open
            || self.sign_edit.is_some()
            || self.book_view.is_some()
            || self.book_edit.is_some()
            || self.dialog_shown()
            || self.code_of_conduct.is_some()
    }

    // --- Buecher (Vanillas BookViewScreen, s. book.rs) ------------------------------------

    // open_book: das Buch in dieser Hand zum Lesen oeffnen.
    pub(crate) fn open_book(&mut self, hand: i32) {
        let hand = if hand == 1 { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
        let Some(stack) = self.own_hand_stack(hand) else { return };
        let ansicht = book::BookView::new(stack);
        self.close_chat();
        if self.inventory.screen_open {
            self.close_inventory();
        }
        self.book_view = Some(ansicht);
        if let Some(gpu) = self.gpu.as_ref() {
            self.cursor_pos = (gpu.size().0 / 2.0, gpu.size().1 / 2.0);
        }
        self.clear_controls();
    }

    pub(crate) fn close_book(&mut self) {
        if self.book_view.take().is_some() && self.mode == AppMode::World {
            self.restore_held_controls();
        }
    }

    // Das Buch auf dem Lesepult (Menue "lectern"): Seiten aus dem Slot, die Seite aus den
    // Menue-Daten.
    pub(crate) fn lectern_book(&self) -> Option<(Vec<Vec<protocol::nbt::TextSpan>>, usize)> {
        if self.open_menu_type() != Some(inventory::MENU_LECTERN) {
            return None;
        }
        let c = self.inventory.open.as_ref()?;
        let seiten = c.slots.first().and_then(|s| s.as_ref()).map(book::pages_of).unwrap_or_else(|| vec![Vec::new()]);
        let seite = c.data.first().copied().unwrap_or(0).max(0) as usize;
        let seite = seite.min(seiten.len() - 1);
        Some((seiten, seite))
    }

    // Die Knoepfe in Bildschirmpixeln: zurueck, weiter, Fertig, (Lesepult) Buch nehmen.
    pub(crate) fn book_buttons(&self, gpu: &GpuState, lectern: bool) -> [Rect; 4] {
        let (sw, _) = gpu.size();
        let s = self.inventory_scale(gpu.size());
        let (ox, oy) = book::origin(sw, s);
        let knopf = |gx: f32| Rect { x: ox + gx * s, y: oy + book::BUTTON_Y * s, w: book::PAGE_BUTTON_W * s, h: book::PAGE_BUTTON_H * s };
        let unten = oy + (book::IMAGE_H + 2.0) * s;
        let (fertig, nehmen) = if lectern {
            // LecternScreen: "Fertig" links, "Buch nehmen" rechts, je 98 breit.
            (Rect { x: sw / 2.0 - 100.0 * s, y: unten, w: 98.0 * s, h: 20.0 * s }, Rect { x: sw / 2.0 + 2.0 * s, y: unten, w: 98.0 * s, h: 20.0 * s })
        } else {
            (Rect { x: sw / 2.0 - 100.0 * s, y: unten, w: 200.0 * s, h: 20.0 * s }, Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 })
        };
        [knopf(book::BACK_X), knopf(book::FORWARD_X), fertig, nehmen]
    }

    pub(crate) fn build_book_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let (seiten, seite, lectern) = match (&self.book_view, self.lectern_book()) {
            (Some(b), _) => (b.pages.clone(), b.page, false),
            (None, Some((s, p))) => (s, p, true),
            (None, None) => return,
        };
        let (sw, sh) = gpu.size();
        let s = self.inventory_scale(gpu.size());
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT });
        let (ox, oy) = book::origin(sw, s);
        if let Some(r) = self.ui_atlas.rect("book") {
            sprites.push(UiSprite::quad(ox, oy, book::IMAGE_W * s, book::IMAGE_H * s, r.sub(0, 0, 192, 192), [1.0; 4], SpriteAtlas::Ui));
        }
        // Seitenzahl rechtsbuendig, schwarz ohne Schatten.
        let schwarz = [0.0, 0.0, 0.0, 1.0];
        let anzeige = book::page_indicator(seite, seiten.len());
        let w = gpu.measure_text(&anzeige, s);
        push_span_shadow(texts, Some(0));
        texts.push(UiText { text: anzeige, x: (ox + book::INDICATOR_RIGHT * s - w).round(), y: (oy + book::INDICATOR_Y * s).round(), color: schwarz, scale: s });
        // Der Seitentext, auf 114 GUI-Pixel umbrochen, hoechstens 14 Zeilen.
        let zeilen = wrap_spans(&seiten[seite], book::TEXT_W * s, |t| gpu.measure_text(t, s), 0.0);
        for (i, zeile) in zeilen.into_iter().take(book::MAX_LINES).enumerate() {
            let mut x = ox + book::TEXT_X * s;
            let y = (oy + (book::TEXT_Y + i as f32 * book::LINE_H) * s).round();
            for span in zeile.spans {
                let w = gpu.measure_text(&span.text, s);
                push_span_shadow(texts, Some(0));
                texts.push(UiText { text: span.text, x: x.round(), y, color: span.color.map(rgb_to_rgba).unwrap_or(schwarz), scale: s });
                x += w;
            }
        }
        // Blaettern nur, wo es weitergeht; unter dem Zeiger hell.
        let [zurueck, weiter, fertig, nehmen] = self.book_buttons(gpu, lectern);
        let (cx, cy) = self.cursor_pos;
        let mut knopf = |r: Rect, name: &str, sichtbar: bool| {
            if !sichtbar {
                return;
            }
            let name = if r.contains(cx, cy) { format!("{name}_highlighted") } else { name.to_string() };
            if let Some(a) = self.ui_atlas.rect(&name) {
                sprites.push(UiSprite::quad(r.x, r.y, r.w, r.h, a.uv, [1.0; 4], SpriteAtlas::Ui));
            }
        };
        knopf(zurueck, "widget/page_backward", seite > 0);
        knopf(weiter, "widget/page_forward", seite + 1 < seiten.len());
        self.push_book_button(gpu, rects, sprites, texts, fertig, &vanilla_text("gui.done", "Done"), true);
        if lectern {
            self.push_book_button(gpu, rects, sprites, texts, nehmen, &vanilla_text("lectern.take_book", "Take Book"), true);
        }
    }

    // Ein beschrifteter Knopf der Buch-Bildschirme (Vanillas Button: hell unter dem Zeiger,
    // ausgegraut, solange er nicht geht).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_book_button(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>, r: Rect, text: &str, aktiv: bool) {
        let s = self.inventory_scale(gpu.size());
        let drauf = aktiv && r.contains(self.cursor_pos.0, self.cursor_pos.1);
        let sprite = match (aktiv, drauf) {
            (false, _) => "widget/button_disabled",
            (true, true) => "widget/button_highlighted",
            (true, false) => "widget/button",
        };
        if self.ui_atlas.rect(sprite).is_some() {
            self.push_nine_slice(sprites, sprite, r.x, r.y, r.w, r.h, 3, s);
        } else {
            rects.push(UiRect { rect: r, color: if drauf { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) }, ..UiRect::FLAT });
        }
        let lw = gpu.measure_text(text, s);
        let farbe = if aktiv { COLOR_WHITE } else { rgb_to_rgba(0xA0A0A0) };
        texts.push(UiText { text: text.to_string(), x: (r.x + (r.w - lw) / 2.0).round(), y: (r.y + (r.h - gpu.text_height(s)) / 2.0).round(), color: farbe, scale: s });
    }

    // Klick im Buch: blaettern, Fertig, (Lesepult) Buch nehmen.
    pub(crate) fn book_click(&mut self) {
        let lectern = self.book_view.is_none();
        let Some(gpu) = self.gpu.as_ref() else { return };
        let [zurueck, weiter, fertig, nehmen] = self.book_buttons(gpu, lectern);
        let (cx, cy) = self.cursor_pos;
        let treffer = |r: Rect| r.contains(cx, cy);
        if treffer(zurueck) {
            self.book_turn(false);
        } else if treffer(weiter) {
            self.book_turn(true);
        } else if treffer(fertig) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            if lectern {
                self.close_inventory();
            } else {
                self.close_book();
            }
        } else if lectern && treffer(nehmen) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.lectern_button(3);
        }
    }

    // Eine Seite weiter bzw. zurueck. Beim Lesepult blaettert der Server (Knopf 1 zurueck, 2
    // weiter), damit alle Leser dieselbe Seite sehen.
    pub(crate) fn book_turn(&mut self, vor: bool) {
        if let Some(b) = self.book_view.as_mut() {
            let vorher = b.page;
            if vor {
                b.forward();
            } else {
                b.back();
            }
            if b.page != vorher {
                self.play_event_at_scaled("item.book.page_turn", self.physics.eye_pos(), 1.0, 1.0, protocol::packets::SoundCategory::Ui);
            }
            return;
        }
        if let Some((seiten, seite)) = self.lectern_book() {
            let geht = if vor { seite + 1 < seiten.len() } else { seite > 0 };
            if geht {
                self.lectern_button(if vor { 2 } else { 1 });
            }
        }
    }

    // --- Buch mit Feder (Vanillas BookEditScreen/BookSignScreen, s. book::BookEdit) ----------

    // WritableBookItem.use: der Editor fuer das Buch in dieser Hand, mit dessen Seiten.
    pub(crate) fn open_book_editor(&mut self, hand: i32) {
        let Some(stack) = self.own_hand_stack(hand) else { return };
        let seiten = stack.writable_pages.clone().unwrap_or_default();
        let slot = if hand == protocol::packets::HAND_OFF { 40 } else { self.inventory.held_slot.min(8) as i32 };
        self.close_chat();
        if self.inventory.screen_open {
            self.close_inventory();
        }
        self.book_edit = Some(book::BookEdit::new(slot, seiten));
        self.book_edit_since = Instant::now();
        self.book_edit_dragging = false;
        if let Some(gpu) = self.gpu.as_ref() {
            self.cursor_pos = (gpu.size().0 / 2.0, gpu.size().1 / 2.0);
        }
        self.clear_controls();
    }

    // Schliessen; `speichern` heisst "Fertig" bzw. "Signieren und schliessen": die Seiten gehen
    // per edit_book an den Server (beim Signieren mit Titel). Fertig laesst wie Vanilla die
    // leeren Seiten am Ende weg und traegt die Seiten gleich ins eigene Buch ein
    // (updateLocalCopy); signiert ersetzt der Server das Buch.
    pub(crate) fn close_book_editor(&mut self, speichern: bool) {
        let Some(mut edit) = self.book_edit.take() else { return };
        self.book_edit_dragging = false;
        if speichern {
            let titel = edit.signing.then(|| edit.title.trim().to_string());
            let seiten = if titel.is_some() { edit.pages.clone() } else { edit.finished_pages() };
            if let Some(tx) = &self.outgoing_tx {
                let _ = tx.send(protocol::packets::edit_book_payload(edit.slot, &seiten, titel.as_deref()));
            }
            if titel.is_none() {
                let platz = if edit.slot == 40 { inventory::PLAYER_OFFHAND } else { inventory::PLAYER_HOTBAR_START + edit.slot as usize };
                if let Some(Some(st)) = self.inventory.player.get_mut(platz) {
                    st.writable_pages = Some(seiten);
                }
            }
        }
        if self.mode == AppMode::World {
            self.restore_held_controls();
        }
    }

    // Textanfang des Felds in Bildschirmpixeln und der GUI-Massstab.
    pub(crate) fn book_edit_text_origin(&self, gpu: &GpuState) -> (f32, f32, f32) {
        let s = self.inventory_scale(gpu.size());
        let (ox, oy) = book::origin(gpu.size().0, s);
        (ox + book::EDIT_TEXT_X * s, oy + book::EDIT_TEXT_Y * s, s)
    }

    pub(crate) fn book_edit_click(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let Some(edit) = self.book_edit.as_ref() else { return };
        // Dieselben Plaetze wie beim Lesepult: Blaettern, links "Signieren" bzw. "Signieren und
        // schliessen", rechts "Fertig" bzw. "Abbrechen".
        let [zurueck, weiter, links, rechts] = self.book_buttons(gpu, true);
        let (cx, cy) = self.cursor_pos;
        let (tx, ty, s) = self.book_edit_text_origin(gpu);
        // Das Textfeld selbst ist 122x134 und beginnt 4 Pixel vor dem Text.
        let feld = Rect { x: tx - 4.0 * s, y: ty - 4.0 * s, w: 122.0 * s, h: 134.0 * s };
        let doppelt = self.last_left_click.is_some_and(|(_, _, n)| n >= 2);
        let shift = self.modifiers.shift_key();
        let breite = |c: char| gpu.advance(c);
        if edit.signing {
            if links.contains(cx, cy) && edit.can_sign() {
                self.play_ui_sound(ui_sound::UiSound::Click);
                self.close_book_editor(true);
            } else if rechts.contains(cx, cy) {
                self.play_ui_sound(ui_sound::UiSound::Click);
                if let Some(e) = self.book_edit.as_mut() {
                    e.signing = false;
                }
            }
            return;
        }
        if edit.page > 0 && zurueck.contains(cx, cy) {
            self.book_edit_turn(false);
        } else if weiter.contains(cx, cy) {
            self.book_edit_turn(true);
        } else if links.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            if let Some(e) = self.book_edit.as_mut() {
                e.signing = true;
            }
            self.book_edit_since = Instant::now();
        } else if rechts.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.close_book_editor(true);
        } else if feld.contains(cx, cy) {
            let (x, y) = ((cx - tx) / s, (cy - ty) / s);
            if let Some(e) = self.book_edit.as_mut() {
                e.click(x, y, shift, doppelt, &breite);
            }
            self.book_edit_dragging = !doppelt;
            self.book_edit_since = Instant::now();
        }
    }

    // Mit gedrueckter Taste ueber das Feld gezogen: markieren (MultiLineEditBox.onDrag).
    pub(crate) fn book_edit_drag(&mut self) {
        if !self.book_edit_dragging {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (tx, ty, s) = self.book_edit_text_origin(gpu);
        let (x, y) = ((self.cursor_pos.0 - tx) / s, (self.cursor_pos.1 - ty) / s);
        let breite = |c: char| gpu.advance(c);
        if let Some(e) = self.book_edit.as_mut().filter(|e| !e.signing) {
            e.drag(x, y, &breite);
        }
    }

    pub(crate) fn book_edit_turn(&mut self, vor: bool) {
        let Some(e) = self.book_edit.as_mut() else { return };
        let geblaettert = if vor { e.forward() } else { e.back() };
        if geblaettert {
            self.book_edit_since = Instant::now();
            self.play_event_at_scaled("item.book.page_turn", self.physics.eye_pos(), 1.0, 1.0, protocol::packets::SoundCategory::Ui);
        }
    }

    // Taste im Editor. Schreiben: MultilineTextField.keyPressed samt Strg+A/C/X/V, Bild auf/ab
    // blaettert. Signieren: das Titelfeld, Enter signiert (sobald etwas drinsteht).
    pub(crate) fn book_edit_key(&mut self, key: &Key, code: PhysicalKey, text: Option<&str>) {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        let strg = |k: KeyCode| ctrl && code == PhysicalKey::Code(k);
        let Some(gpu) = self.gpu.as_ref() else { return };
        let breite = |c: char| gpu.advance(c);
        let Some(edit) = self.book_edit.as_mut() else { return };
        self.book_edit_since = Instant::now();
        if edit.signing {
            if strg(KeyCode::KeyV) {
                if let Some(t) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()) {
                    edit.title_insert(&t);
                }
                return;
            }
            match key {
                Key::Named(NamedKey::Enter) if !edit.title.is_empty() => self.close_book_editor(true),
                Key::Named(NamedKey::Backspace) => edit.title_backspace(),
                _ => {
                    if let Some(t) = text {
                        edit.title_insert(t);
                    }
                }
            }
            return;
        }
        if strg(KeyCode::KeyA) {
            edit.select_all();
            return;
        }
        if strg(KeyCode::KeyC) || strg(KeyCode::KeyX) {
            let markiert = if strg(KeyCode::KeyX) { edit.cut(&breite) } else { edit.selected_text().to_string() };
            if let Ok(mut c) = arboard::Clipboard::new() {
                let _ = c.set_text(markiert);
            }
            return;
        }
        if strg(KeyCode::KeyV) {
            if let Some(t) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()) {
                edit.insert(&t, &breite);
            }
            return;
        }
        let taste = match key {
            Key::Named(NamedKey::PageUp) => return self.book_edit_turn(false),
            Key::Named(NamedKey::PageDown) => return self.book_edit_turn(true),
            Key::Named(NamedKey::ArrowLeft) => book::EditKey::Left,
            Key::Named(NamedKey::ArrowRight) => book::EditKey::Right,
            Key::Named(NamedKey::ArrowUp) => book::EditKey::Up,
            Key::Named(NamedKey::ArrowDown) => book::EditKey::Down,
            Key::Named(NamedKey::Home) => book::EditKey::Home,
            Key::Named(NamedKey::End) => book::EditKey::End,
            Key::Named(NamedKey::Backspace) => book::EditKey::Backspace,
            Key::Named(NamedKey::Delete) => book::EditKey::Delete,
            Key::Named(NamedKey::Enter) => book::EditKey::Enter,
            _ => {
                if let Some(t) = text {
                    edit.insert(t, &breite);
                }
                return;
            }
        };
        edit.key(taste, shift, ctrl, &breite);
    }

    pub(crate) fn build_book_edit_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let Some(edit) = self.book_edit.as_ref() else { return };
        let (sw, sh) = gpu.size();
        let s = self.inventory_scale(gpu.size());
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT });
        let (ox, oy) = book::origin(sw, s);
        if let Some(r) = self.ui_atlas.rect("book") {
            sprites.push(UiSprite::quad(ox, oy, book::IMAGE_W * s, book::IMAGE_H * s, r.sub(0, 0, 192, 192), [1.0; 4], SpriteAtlas::Ui));
        }
        let schwarz = [0.0, 0.0, 0.0, 1.0];
        let blinkt = (self.book_edit_since.elapsed().as_millis() / 300).is_multiple_of(2);
        let text = |texts: &mut Vec<UiText>, t: &str, x: f32, y: f32, farbe: [f32; 4]| {
            push_span_shadow(texts, Some(0));
            push_typed_marker(texts);
            texts.push(UiText { text: t.to_string(), x: x.round(), y: y.round(), color: farbe, scale: s });
        };
        let [zurueck, weiter, links, rechts] = self.book_buttons(gpu, true);
        if edit.signing {
            // Alles mittig ueber dem Textbereich (36 + 114/2).
            let mitte = ox + book::SIGN_CENTER_X * s;
            let label = vanilla_text("book.editTitle", "Enter Book Title:");
            text(texts, &label, mitte - gpu.measure_text(&label, s) / 2.0, oy + book::SIGN_LABEL_Y * s, schwarz);
            let tw = gpu.measure_raw(&edit.title, s);
            let tx = (mitte - tw / 2.0).round();
            text(texts, &edit.title, tx, oy + book::SIGN_TITLE_Y * s, schwarz);
            if blinkt {
                text(texts, "_", tx + tw, oy + book::SIGN_TITLE_Y * s, schwarz);
            }
            let name = self.own_uuid.and_then(|u| self.player_names.get(&u)).cloned().or_else(|| self.credentials.as_ref().map(|c| c.username.clone())).unwrap_or_else(|| "Player".to_string());
            let von = vanilla_text("book.byAuthor", "by %1$s").replace("%1$s", &name).replacen("%s", &name, 1);
            text(texts, &von, mitte - gpu.measure_text(&von, s) / 2.0, oy + book::SIGN_OWNER_Y * s, rgb_to_rgba(0x555555));
            let hinweis = vanilla_text("book.finalizeWarning", "Note! When you sign the book, it will no longer be editable.");
            let spans = [protocol::nbt::TextSpan { text: hinweis, ..Default::default() }];
            for (i, zeile) in wrap_spans(&spans, book::TEXT_W * s, |t| gpu.measure_text(t, s), 0.0).into_iter().enumerate() {
                let t: String = zeile.spans.iter().map(|sp| sp.text.as_str()).collect();
                text(texts, &t, ox + book::SIGN_WARNING_X * s, oy + (book::SIGN_WARNING_Y + i as f32 * book::LINE_H) * s, schwarz);
            }
            self.push_book_button(gpu, rects, sprites, texts, links, &vanilla_text("book.finalizeButton", "Sign and Close"), edit.can_sign());
            self.push_book_button(gpu, rects, sprites, texts, rechts, &vanilla_text("gui.cancel", "Cancel"), true);
            return;
        }
        let anzeige = book::page_indicator(edit.page, edit.pages.len());
        let w = gpu.measure_text(&anzeige, s);
        text(texts, &anzeige, ox + book::INDICATOR_RIGHT * s - w, oy + book::INDICATOR_Y * s, schwarz);
        // Die Zeilen, markierter Teil weiss auf Blau (Vanillas invertierte Markierung).
        let (tx, ty, _) = self.book_edit_text_origin(gpu);
        let seite = edit.text();
        let breite = |c: char| gpu.advance(c);
        let zeilen = book::display_lines(seite, &breite);
        let markiert = edit.selection();
        let mut cursor_gezeichnet = false;
        for (i, &(a, b)) in zeilen.iter().enumerate() {
            let y = ty + i as f32 * book::LINE_H * s;
            let x_bei = |pos: usize| tx + gpu.measure_raw(&seite[a..pos], s);
            match markiert.filter(|&(sa, sb)| sa <= b && sb >= a) {
                Some((sa, sb)) => {
                    let (m0, m1) = (sa.max(a), sb.min(b));
                    let rechts_x = if sb > b { tx + book::TEXT_W * s } else { x_bei(m1) };
                    rects.push(UiRect { rect: Rect { x: x_bei(m0).round(), y: y.round(), w: (rechts_x - x_bei(m0)).round(), h: (book::LINE_H * s).round() }, color: [0.0, 0.0, 0.8, 1.0], ..UiRect::FLAT });
                    text(texts, &seite[a..m0], tx, y, schwarz);
                    text(texts, &seite[m0..m1], x_bei(m0), y, [1.0; 4]);
                    text(texts, &seite[m1..b], x_bei(m1), y, schwarz);
                }
                None => text(texts, &seite[a..b], tx, y, schwarz),
            }
            // Der Cursor in der ersten Zeile, die ihn enthaelt: am Ende des Texts ein "_",
            // mitten drin ein senkrechter Strich.
            let c = edit.cursor();
            if blinkt && !cursor_gezeichnet && a <= c && c <= b {
                cursor_gezeichnet = true;
                if c == seite.len() {
                    text(texts, "_", x_bei(c), y, schwarz);
                } else {
                    rects.push(UiRect { rect: Rect { x: x_bei(c).round(), y: (y - s).round(), w: s.max(1.0).round(), h: (11.0 * s).round() }, color: schwarz, ..UiRect::FLAT });
                }
            }
        }
        let (cx, cy) = self.cursor_pos;
        let mut knopf = |r: Rect, name: &str, sichtbar: bool| {
            if !sichtbar {
                return;
            }
            let name = if r.contains(cx, cy) { format!("{name}_highlighted") } else { name.to_string() };
            if let Some(a) = self.ui_atlas.rect(&name) {
                sprites.push(UiSprite::quad(r.x, r.y, r.w, r.h, a.uv, [1.0; 4], SpriteAtlas::Ui));
            }
        };
        knopf(zurueck, "widget/page_backward", edit.page > 0);
        knopf(weiter, "widget/page_forward", true);
        self.push_book_button(gpu, rects, sprites, texts, links, &vanilla_text("book.signButton", "Sign"), true);
        self.push_book_button(gpu, rects, sprites, texts, rechts, &vanilla_text("gui.done", "Done"), true);
    }

    pub(crate) fn lectern_button(&mut self, knopf: i32) {
        let Some(c) = self.inventory.open.as_ref() else { return };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::container_button_click_payload(c.sync_id, knopf));
        }
    }

    // --- Schild-Editor (Vanillas SignEditScreen / HangingSignEditScreen) -----------------

    // Der Server hat den Editor geoeffnet: vorbefuellt mit dem aktuellen Text dieser Seite,
    // Zeiger frei, alle Tasten los (wie beim Chat, s. open_chat).
    pub(crate) fn open_sign_editor(&mut self, pos: (i32, i32, i32), front: bool) {
        let seite = self.signs.get(&pos).map(|t| if front { t.front.clone() } else { t.back.clone() });
        let lines: [String; 4] = std::array::from_fn(|i| seite.as_ref().map(|s| s.lines[i].iter().map(|sp| sp.text.as_str()).collect()).unwrap_or_default());
        let (wood, hanging) = match self.sign_at(pos) {
            Some((holz, art, _)) => (holz, matches!(art, world::entity_model::SignKind::Hanging(_))),
            None => (0, false),
        };
        let color = seite.as_ref().map_or(15, |s| s.color);
        self.close_chat();
        if self.inventory.screen_open {
            self.close_inventory();
        }
        self.sign_edit = Some(SignEdit { pos, front, lines, row: 0, hanging, wood, color, opened: Instant::now(), caret: Default::default() });
        if let Some(gpu) = self.gpu.as_ref() {
            self.cursor_pos = (gpu.size().0 / 2.0, gpu.size().1 / 2.0);
        }
        self.clear_controls();
    }

    // Fertig (Knopf, Escape): Sign Update senden, lokal schon uebernehmen, schliessen.
    pub(crate) fn finish_sign_edit(&mut self) {
        let Some(edit) = self.sign_edit.take() else { return };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::sign::sign_update_payload(edit.pos, edit.front, &edit.lines));
        }
        let eintrag = self.signs.entry(edit.pos).or_default();
        let seite = if edit.front { &mut eintrag.front } else { &mut eintrag.back };
        for (zeile, text) in seite.lines.iter_mut().zip(&edit.lines) {
            *zeile = if text.is_empty() { Vec::new() } else { vec![protocol::nbt::TextSpan { text: text.clone(), ..Default::default() }] };
        }
        if self.mode == AppMode::World {
            self.restore_held_controls();
        }
    }

    // Tastendruck im Editor. Shift+Enter/Tab/Pfeil runter: naechste Zeile, Pfeil hoch: vorige,
    // Ruecktaste loescht, Strg+V fuegt ein. Eine Zeile nimmt nur so viel auf, wie aufs Brett
    // passt (SignEditScreen: Breite <= maxTextWidth, 90 bzw. 60 Schriftpixel).
    pub(crate) fn sign_edit_key(&mut self, key: &Key, text: Option<&str>, control: bool, paste: bool) {
        let einfuegen: Option<String> = if paste {
            arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok())
        } else if control {
            None
        } else {
            text.map(str::to_string)
        };
        let breite_max = if self.sign_edit.as_ref().is_some_and(|e| e.hanging) { 60.0 } else { 90.0 };
        // In der Schrift der Schilder: so breit, wie die Zeile auf dem Brett steht.
        let _schrift = self.gpu.as_ref().map(|g| self.area_font_scope(g, ShadowArea::Signs));
        let messen = |t: &str| self.gpu.as_ref().map_or(t.chars().count() as f32 * 6.0, |g| g.measure_text(t, 1.0));
        let Some(edit) = self.sign_edit.as_mut() else { return };
        match key {
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Tab) | Key::Named(NamedKey::ArrowDown) => edit.row = (edit.row + 1) % 4,
            Key::Named(NamedKey::ArrowUp) => edit.row = (edit.row + 3) % 4,
            Key::Named(NamedKey::Backspace) => {
                edit.lines[edit.row].pop();
            }
            _ => {
                if let Some(t) = einfuegen {
                    for ch in t.chars().filter(|c| !c.is_control() && *c != '\u{a7}') {
                        let mut neu = edit.lines[edit.row].clone();
                        neu.push(ch);
                        if messen(&neu) > breite_max || neu.len() > 384 {
                            break;
                        }
                        edit.lines[edit.row] = neu;
                    }
                }
            }
        }
    }

    // Ohne Fertig-Knopf: Enter schliesst ab, Shift+Enter ist die naechste Zeile. Das Fenster nur,
    // wenn man die Seite in der Welt nicht sieht (s. sign_edit_in_world).
    pub(crate) fn build_sign_editor(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let Some(edit) = self.sign_edit.as_ref() else { return };
        if self.sign_edit_in_world(gpu).is_none() {
            self.build_sign_editor_window(gpu, edit, rects, sprites, texts);
        }
    }

    // Das Editor-Fenster wie Vanillas SignEditScreen: abgedunkelt, das Brett gross in der Mitte.
    pub(crate) fn build_sign_editor_window(&self, gpu: &GpuState, edit: &SignEdit, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        let g = self.inventory_scale((sw, sh));
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT });
        let titel = if edit.hanging { "Hängeschild bearbeiten" } else { "Schild bearbeiten" };
        let tw = gpu.measure_text(titel, g);
        texts.push(UiText { text: titel.to_string(), x: ((sw - tw) / 2.0).round(), y: (40.0 * g).round(), color: COLOR_WHITE, scale: g });

        // Das Brett von vorn, aus der Entity-Textur: stehend 24x12 Modellpixel ab (2, 2),
        // haengend 14x10 ab (2, 14). Der Text steht darauf im Massstab der Welt (stehend 96x48
        // Schriftpixel je Brett, haengend 56x40).
        let (px, py, bw, bh, schrift_w, zeile_h) = if edit.hanging { (2.0, 14.0, 14.0, 10.0, 56.0, 9.0) } else { (2.0, 2.0, 24.0, 12.0, 96.0, 10.0) };
        let textur = if edit.hanging { world::entity_model::HANGING_SIGN_TEXTURES[edit.wood] } else { world::entity_model::SIGN_TEXTURES[edit.wood] };
        let brett_w = (sw * 0.45).min(schrift_w * 3.0 * g).round();
        let s = brett_w / schrift_w;
        let brett_h = (brett_w * bh / bw).round();
        let (bx, by) = (((sw - brett_w) / 2.0).round(), (sh / 4.0 + 40.0 * g).round());
        if let Some([u0, v0, u1, v1]) = self.entity_atlas.rect(textur) {
            let (du, dv) = ((u1 - u0) / 64.0, (v1 - v0) / 32.0);
            let uv = [u0 + px * du, v0 + py * dv, u0 + (px + bw) * du, v0 + (py + bh) * dv];
            sprites.push(UiSprite::quad(bx, by, brett_w, brett_h, uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Entity));
        }
        let farbe = rgb_to_rgba(sign_text_color(edit.color, false));
        let blink = (edit.opened.elapsed().as_millis() / 300).is_multiple_of(2);
        let mitte_y = by + brett_h / 2.0;
        self.begin_text_area(gpu, texts, ShadowArea::Signs);
        for (i, zeile) in edit.lines.iter().enumerate() {
            let w = gpu.measure_text(zeile, s);
            let x = (bx + (brett_w - w) / 2.0).round();
            let y = (mitte_y + (i as f32 * zeile_h - 2.0 * zeile_h) * s).round();
            push_typed_marker(texts);
            texts.push(UiText { text: zeile.clone(), x, y, color: farbe, scale: s });
            if i == edit.row && blink {
                texts.push(UiText { text: "_".to_string(), x: (x + w).round(), y, color: farbe, scale: s });
            }
        }
        Self::end_text_area(gpu, texts);
    }

    // Inventar-/Container-Bildschirm: Vanilla-Sheet, alle Slots, der Stack an der Maus und
    // der Name des Items unter dem Zeiger.
    // Rueckgabe: der Index in `sprites`, VOR dem die Spielerfigur gezeichnet werden muss --
    // also direkt hinter dem Fensterhintergrund und vor den Item-Symbolen. Die Figur ist echte
    // 3D-Geometrie in einem eigenen Durchgang (s. GpuState::render_world), sie kann deshalb
    // nicht einfach zwischen die Sprites gereiht werden. None = dieser Bildschirm hat keine
    // Figur (Kiste, Ofen, ...).
    pub(crate) fn build_inventory_screen(
        &self,
        gpu: &GpuState,
        rects: &mut Vec<UiRect>,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
    ) -> Option<usize> {
        if self.creative_shown() {
            return self.build_creative_screen(gpu, rects, sprites, texts);
        }
        let screen = gpu.size();
        let layout = inventory::screen_layout(&self.inventory, screen, self.inventory_scale(screen));
        // Abdunklung der WELT hinter dem Fenster. Das Fenster selbst bleibt immer deckend --
        // ein durchsichtiges Inventar waere nur unbedienbar.
        //
        // ECHTER BUG gewesen (die Inventar-Transparenz wirkte nicht): der Regler
        // lief genau verkehrt herum. 0 % bedeutete "ganz schwarz" und 100 % "gar keine
        // Abdunklung" -- gemeint ist das Gegenteil, der Prozentwert IST die Abdunklung:
        //   0 %   = keine Abdunklung, das Kamerabild bleibt voll sichtbar
        //   100 % = deckend schwarz hinter dem Fenster
        let dim = self.settings.inventory_transparency as f32 / 100.0;
        if dim > 0.0 {
            rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: screen.0, h: screen.1 }, color: [0.0, 0.0, 0.0, dim], ..UiRect::FLAT });
        }

        if let Some(sheet) = self.ui_atlas.rect(layout.sheet) {
            for ((sx, sy, sw, sh), dy) in &layout.parts {
                sprites.push(UiSprite::quad(
                    layout.bg_x,
                    layout.bg_y + dy * layout.scale,
                    *sw as f32 * layout.scale,
                    *sh as f32 * layout.scale,
                    sheet.sub(*sx, *sy, *sw, *sh),
                    [1.0, 1.0, 1.0, 1.0],
                    SpriteAtlas::Ui,
                ));
            }
        }
        // Containertitel oben links (HandledScreen.drawForeground: titleX/Y = 8/6) und darueber
        // dem Spielerteil "Inventar" (playerInventoryTitleY = backgroundHeight - 94).
        // Vanillas Grundfarbe 0x404040; die Farben des Servers bleiben erhalten.
        // Ohne das fehlt der Titel ganz (z.B. "Queue Duels" auf mcpvp.club).
        if let Some(container) = self.inventory.open.as_ref().filter(|c| !c.slots.is_empty()) {
            let grau = rgb_to_rgba(0x404040);
            let mut zeile = |spans: Vec<protocol::nbt::TextSpan>, gx: f32, gy: f32| {
                let mut x = layout.bg_x + gx * layout.scale;
                for span in spans {
                    let w = gpu.measure_text(&span.text, layout.scale);
                    texts.push(UiText {
                        text: span.text,
                        x: x.round(),
                        y: (layout.bg_y + gy * layout.scale).round(),
                        color: span.color.map(rgb_to_rgba).unwrap_or(grau),
                        scale: layout.scale,
                    });
                    x += w;
                }
            };
            if container.screen_type == inventory::MENU_MERCHANT {
                // MerchantScreen.drawForeground: der Titel mittig ueber dem rechten Teil, mit
                // der Berufsstufe dahinter ("Bauer - Lehrling"), solange der Dorfbewohner sie
                // zeigt; "Angebote" mittig ueber der Liste; "Inventar" bei x 107.
                let m = &container.merchant;
                let mut titel = protocol::nbt::legacy_to_spans(&container.title, None);
                if (1..=5).contains(&m.level) && m.show_progress {
                    let stufe = protocol::nbt::translate(&format!("merchant.level.{}", m.level))
                        .unwrap_or_else(|| ["Neuling", "Lehrling", "Geselle", "Experte", "Meister"][m.level as usize - 1].to_string());
                    titel.push(protocol::nbt::TextSpan { text: format!(" - {stufe}"), ..Default::default() });
                }
                let breite = |spans: &[protocol::nbt::TextSpan]| spans.iter().map(|sp| gpu.measure_text(&sp.text, 1.0)).sum::<f32>();
                let tw = breite(&titel);
                zeile(titel, (49.0 + inventory::MERCHANT_SHEET_W / 2.0 - tw / 2.0).floor(), 6.0);
                let angebote = protocol::nbt::legacy_to_spans(&protocol::nbt::translate("merchant.trades").unwrap_or_else(|| "Angebote".into()), None);
                let aw = breite(&angebote);
                zeile(angebote, (5.0 - aw / 2.0 + 48.0).floor(), 6.0);
                zeile(protocol::nbt::legacy_to_spans("Inventar", None), 107.0, layout.bg_h / layout.scale - 94.0);
            } else {
                // Der Amboss setzt seinen Titel rechts neben den Hammer (AnvilScreen.titleLabelX = 60).
                let titel_x = if container.screen_type == inventory::MENU_ANVIL { 60.0 } else { 8.0 };
                zeile(protocol::nbt::legacy_to_spans(&container.title, None), titel_x, 6.0);
                zeile(protocol::nbt::legacy_to_spans("Inventar", None), 8.0, layout.bg_h / layout.scale - 94.0);
            }
        }
        let enchant_tooltip = self.build_special_screen(gpu, &layout, rects, sprites, texts);
        let rezept_tooltip = self.build_recipe_book(gpu, &layout, rects, sprites, texts);
        // Nur neben dem eigenen Inventar, wie in Vanilla (InventoryScreen.showsActiveEffects).
        if self.settings.show_effects_inventory && self.inventory.open.is_none() {
            self.build_inventory_effects(gpu, &layout, sprites, texts);
        }
        // GENAU HIER gehoert die Figur hin: der Hintergrund steht, die Symbole noch nicht.
        // Danach, und sie verdeckte den Stapel an der Maus; davor, und der Hintergrund
        // uebermalte sie.
        let doll_split = layout.doll.map(|_| sprites.len());

        let (_, _, slots) = self.inventory.active();
        let (cx, cy) = self.cursor_pos;
        // Zusammenfassen bei Hover: der gleichartige Stapel unter dem Zeiger zeigt keine Zahl,
        // der am Zeiger die Summe.
        let zusammen = self
            .inventory
            .cursor
            .as_ref()
            .filter(|_| self.settings.inventory_merge_hover)
            .and_then(|c| {
                let slot = layout.slots.iter().find(|s| s.contains(cx, cy))?;
                let unten = slots.get(slot.index)?.as_ref().filter(|s| s.stackable_with(c))?;
                Some((slot.index, unten.count + c.count))
            });
        // Tooltip des Slots unter dem Zeiger: je Zeile ihre Abschnitte (Text, Farbe). Erste
        // Zeile der Name, darunter die Verzauberungen (world::enchantment::tooltip_lines).
        let mut hovered: Option<(Vec<Vec<protocol::nbt::TextSpan>>, inventory::SlotRect, Option<&protocol::item::ItemStack>)> = None;
        for slot in &layout.slots {
            let Some(Some(stack)) = slots.get(slot.index) else { continue };
            // Gesperrter Ruestungsslot zittert kurz (s. predict_inventory_click).
            let zittern = match self.armor_shake {
                Some((index, seit)) if index == slot.index && self.inventory.open.is_none() => {
                    inventory::shake_offset(seit.elapsed().as_secs_f32()) * layout.scale
                }
                _ => 0.0,
            };
            if zusammen.is_some_and(|(index, _)| index == slot.index) {
                self.push_item(sprites, texts, &protocol::item::ItemStack { count: 1, ..stack.clone() }, slot.x + zittern, slot.y, slot.size);
            } else {
                self.push_item(sprites, texts, stack, slot.x + zittern, slot.y, slot.size);
            }
            if slot.contains(cx, cy) {
                hovered = self.item_hover_tooltip(stack).map(|zeilen| (zeilen, *slot, world::bundle::is_bundle(stack).then_some(stack)));
            }
        }
        // Slot unter dem Zeiger hervorheben (Vanillas heller Schleier).
        if let Some(slot) = layout.slots.iter().find(|s| s.contains(cx, cy)) {
            rects.push(UiRect { rect: Rect { x: slot.x, y: slot.y, w: slot.size, h: slot.size }, color: [1.0, 1.0, 1.0, 0.25], ..UiRect::FLAT });
        }

        // Der Stack an der Maus haengt am Zeiger.
        if let Some(stack) = &self.inventory.cursor {
            let size = inventory::ICON_SIZE * layout.scale;
            match zusammen {
                Some((_, summe)) => self.push_item(sprites, texts, &protocol::item::ItemStack { count: summe, ..stack.clone() }, cx - size / 2.0, cy - size / 2.0, size),
                None => self.push_item(sprites, texts, stack, cx - size / 2.0, cy - size / 2.0, size),
            }
        } else if let Some(zeilen) = enchant_tooltip {
            // Hinweis einer Zauber-Option: rechts neben dem Zeiger, mit dunklem Grund.
            let zeilen_h = gpu.text_height(UI_SCALE_SMALL);
            let breite = zeilen.iter().map(|z| z.iter().map(|s| gpu.measure_text(&s.text, UI_SCALE_SMALL)).sum::<f32>()).fold(0.0, f32::max);
            let (x0, y0) = ((cx + 16.0).round(), (cy - 4.0).round());
            rects.push(UiRect { rect: Rect { x: x0 - 4.0, y: y0 - 4.0, w: breite + 8.0, h: zeilen_h * zeilen.len() as f32 + 8.0 }, color: [0.06, 0.0, 0.06, 0.94], ..UiRect::FLAT });
            let mut y = y0;
            for zeile in zeilen {
                let mut sx = x0;
                for span in zeile {
                    let w = gpu.measure_text(&span.text, UI_SCALE_SMALL);
                    let color = span.color.map(rgb_to_rgba).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                    push_span_shadow(texts, span.shadow);
                    texts.push(UiText { text: span.text, x: sx.round(), y: y.round(), color, scale: UI_SCALE_SMALL });
                    sx += w;
                }
                y += zeilen_h;
            }
        } else if let Some((zeilen, center_y)) = rezept_tooltip {
            // Rezept unter dem Zeiger: links neben der Truhe, rechtsbuendig.
            let x = self.recipe_panel(&layout).map_or(layout.bg_x, |p| p.x) - 6.0 * layout.scale;
            let zeilen_h = gpu.text_height(UI_SCALE_SMALL);
            let mut y = center_y - zeilen_h * zeilen.len() as f32 / 2.0;
            for zeile in zeilen {
                let breite: f32 = zeile.iter().map(|span| gpu.measure_text(&span.text, UI_SCALE_SMALL)).sum();
                let mut sx = x - breite;
                for span in zeile {
                    let w = gpu.measure_text(&span.text, UI_SCALE_SMALL);
                    let color = span.color.map(rgb_to_rgba).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                    push_span_shadow(texts, span.shadow);
                    texts.push(UiText { text: span.text, x: sx.round(), y: y.round(), color, scale: UI_SCALE_SMALL });
                    sx += w;
                }
                y += zeilen_h;
            }
        } else if let Some((zeilen, slot, buendel)) = hovered {
            // Fest am Slot statt am Zeiger, und immer nach aussen zeigend (s.
            // ScreenLayout::tooltip_anchor). Mehrere Zeilen stehen als Block mittig auf Hoehe
            // des Slots; rechtsbuendig heisst jede Zeile fuer sich rechtsbuendig.
            let (x, center_y, right_aligned) = layout.tooltip_anchor(&slot);
            // Mit offenem Rezeptbuch links neben die Truhe statt darauf.
            let x = match self.recipe_panel(&layout) {
                Some(p) if right_aligned => p.x - 6.0 * layout.scale,
                _ => x,
            };
            let zeilen_h = gpu.text_height(UI_SCALE_SMALL);
            let mut y = center_y - zeilen_h * zeilen.len() as f32 / 2.0;
            // Ein Buendel zeigt darunter seinen Inhalt (ClientBundleTooltip).
            if let Some(b) = buendel {
                let s = layout.scale;
                let px = if right_aligned { x - BUNDLE_PANEL_W * s } else { x };
                self.push_bundle_panel(gpu, sprites, texts, b, px, y + zeilen_h * zeilen.len() as f32 + 2.0 * s, s);
            }
            for zeile in zeilen {
                let breite: f32 = zeile.iter().map(|span| gpu.measure_text(&span.text, UI_SCALE_SMALL)).sum();
                let mut sx = if right_aligned { x - breite } else { x };
                for span in zeile {
                    let w = gpu.measure_text(&span.text, UI_SCALE_SMALL);
                    let color = span.color.map(rgb_to_rgba).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                    push_span_shadow(texts, span.shadow);
                    texts.push(UiText { text: span.text, x: sx.round(), y: y.round(), color, scale: UI_SCALE_SMALL });
                    sx += w;
                }
                y += zeilen_h;
            }
        }
        doll_split
    }

    // Rezeptbuch: der Knopf im Fenster und, wenn offen, die Truhe links daneben. Zurueck kommt
    // der Tooltip des Rezepts unter dem Zeiger (Zeilen und Mitte-y), gezeichnet vom Aufrufer.
    #[allow(clippy::type_complexity)]
    pub(crate) fn build_recipe_book(
        &self,
        gpu: &GpuState,
        layout: &inventory::ScreenLayout,
        rects: &mut Vec<UiRect>,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
    ) -> Option<(Vec<Vec<protocol::nbt::TextSpan>>, f32)> {
        let (cx, cy) = self.cursor_pos;
        let weiss = [1.0, 1.0, 1.0, 1.0];
        let knopf = self.recipe_button_rect(layout)?;
        let name = if knopf.contains(cx, cy) { "recipe_book/button_highlighted" } else { "recipe_book/button" };
        if let Some(r) = self.ui_atlas.rect(name) {
            sprites.push(UiSprite::quad(knopf.x, knopf.y, knopf.w, knopf.h, r.uv, weiss, SpriteAtlas::Ui));
        }
        let panel = self.recipe_panel(layout)?;
        let s = panel.scale;
        // Truhe aus generic_54: Kopf, so viele Reihen wie noetig, Unterkante.
        let sheet = self.ui_atlas.rect("container/generic_54")?;
        let w = recipe_book::PANEL_W * s;
        sprites.push(UiSprite::quad(panel.x, panel.y, w, recipe_book::HEAD_H * s, sheet.sub(0, 0, 176, 17), weiss, SpriteAtlas::Ui));
        for r in 0..recipe_book::ROWS {
            let y = panel.y + (recipe_book::HEAD_H + r as f32 * 18.0) * s;
            sprites.push(UiSprite::quad(panel.x, y, w, 18.0 * s, sheet.sub(0, 17, 176, 18), weiss, SpriteAtlas::Ui));
        }
        let unten = panel.y + (recipe_book::PANEL_H - recipe_book::FOOT_H) * s;
        sprites.push(UiSprite::quad(panel.x, unten, w, recipe_book::FOOT_H * s, sheet.sub(0, 215, 176, 7), weiss, SpriteAtlas::Ui));
        texts.push(UiText {
            text: "Rezeptbuch".to_string(),
            x: (panel.x + 8.0 * s).round(),
            y: (panel.y + 6.0 * s).round(),
            color: rgb_to_rgba(0x404040),
            scale: s,
        });
        // Schalter "Craftbares zuerst".
        let (fx, fy, fw, fh) = panel.filter();
        let ueber_filter = (Rect { x: fx, y: fy, w: fw, h: fh }).contains(cx, cy);
        let filter = match (self.recipe_book.craftable_first, ueber_filter) {
            (true, false) => "recipe_book/filter_enabled",
            (true, true) => "recipe_book/filter_enabled_highlighted",
            (false, false) => "recipe_book/filter_disabled",
            (false, true) => "recipe_book/filter_disabled_highlighted",
        };
        if let Some(r) = self.ui_atlas.rect(filter) {
            sprites.push(UiSprite::quad(fx, fy, fw, fh, r.uv, weiss, SpriteAtlas::Ui));
        }
        let mut tooltip = if ueber_filter {
            let text = if self.recipe_book.craftable_first { "Craftbares zuerst" } else { "Alle Rezepte" };
            Some((vec![vec![protocol::nbt::TextSpan { text: text.to_string(), ..Default::default() }]], fy + fh / 2.0))
        } else {
            None
        };
        let shown = self.recipe_shown();
        let erster = self.recipe_book.row(shown.len()) * recipe_book::COLS;
        let size = inventory::ICON_SIZE * s;
        for (i, eintrag) in shown.iter().skip(erster).take(recipe_book::COLS * recipe_book::ROWS).enumerate() {
            let (x, y) = panel.slot(i);
            let entry = &self.recipe_book.entries[eintrag.index];
            // Nicht craftbar: der Platz rot getoent (wie in Bedrock).
            if eintrag.sets == 0 {
                sprites.push(UiSprite::quad(x, y, size, size, sheet.sub(8, 18, 16, 16), [1.0, 0.4, 0.4, 1.0], SpriteAtlas::Ui));
            }
            let Some(mut stack) = entry.display.result().first_stack() else { continue };
            // Die Zahl am Platz: wie viel insgesamt aus dem Vorrat geht, nicht je Durchgang; ab
            // hundert "99+", damit sie im Platz bleibt.
            let gesamt = eintrag.sets.saturating_mul(stack.count.max(1));
            stack.count = 1;
            self.push_item(sprites, texts, &stack, x, y, size);
            if gesamt > 1 {
                let text = if gesamt > 99 { "99+".to_string() } else { gesamt.to_string() };
                self.push_count_label(texts, &text, x, y, size);
            }
            if panel.slot_at(cx, cy) == Some(i) {
                rects.push(UiRect { rect: Rect { x, y, w: size, h: size }, color: [1.0, 1.0, 1.0, 0.25], ..UiRect::FLAT });
                let mut zeilen = self.item_hover_tooltip(&stack).unwrap_or_default();
                if eintrag.sets == 0 {
                    zeilen.push(vec![protocol::nbt::TextSpan { text: "Zutaten fehlen".to_string(), color: Some(0xFF5555), ..Default::default() }]);
                }
                tooltip = Some((zeilen, y + size / 2.0));
            }
        }
        // Mehr als in die Truhe passt: unten rechts, auf welcher Seite man ist.
        let seiten = shown.len().div_ceil(recipe_book::COLS).saturating_sub(recipe_book::ROWS) + 1;
        if seiten > 1 {
            let text = format!("{}/{}", self.recipe_book.row(shown.len()) + 1, seiten);
            let tw = gpu.measure_text(&text, s);
            texts.push(UiText { text, x: (fx - 4.0 * s - tw).round(), y: (panel.y + 6.0 * s).round(), color: rgb_to_rgba(0x404040), scale: s });
        }
        tooltip
    }

    // Der Inhalt eines Buendels wie ClientBundleTooltip, oben links bei (px, py), in GUI-Pixeln
    // mal `s`: bis zu zwoelf Plaetze zu 24 px in Viererreihen, von rechts unten gefuellt (das
    // zuletzt Eingelegte oben links), bei mehr Eintraegen rechts unten "+N"; der gewaehlte Platz
    // hervorgehoben; darunter der Fuellbalken (96x13) mit "Voll"/"Leer". Leer steht statt des
    // Rasters die Beschreibung.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_bundle_panel(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>, b: &protocol::item::ItemStack, px: f32, py: f32, s: f32) {
        let sprite = |sprites: &mut Vec<UiSprite>, name: &str, x: f32, y: f32, w: f32, h: f32| {
            if let Some(r) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(x, y, w * s, h * s, r.uv, [1.0; 4], SpriteAtlas::Ui));
            }
        };
        let tr = |key: &str, standard: &str| protocol::nbt::translate(key).unwrap_or_else(|| standard.to_string());
        let inhalt = world::bundle::contents(b);
        let gewicht = world::bundle::weight(b);
        let balken_y = if inhalt.is_empty() {
            let mut y = py;
            for zeile in wrap_measured(|t| gpu.measure_raw(t, UI_SCALE_SMALL), &tr("item.minecraft.bundle.empty.description", "Can hold a mixed stack of items"), BUNDLE_PANEL_W * s) {
                texts.push(UiText { text: zeile, x: px.round(), y: y.round(), color: rgb_to_rgba(0xAAAAAA), scale: UI_SCALE_SMALL });
                y += gpu.text_height(UI_SCALE_SMALL);
            }
            y + 4.0 * s
        } else {
            let ueberzaehlig = inhalt.len() > 12;
            let gezeigt = &inhalt[..world::bundle::items_to_show(inhalt.len())];
            let reihen = inhalt.len().min(12).div_ceil(4);
            let (rechts, unten) = (px + BUNDLE_PANEL_W * s, py + reihen as f32 * 24.0 * s);
            let mut i = 1;
            for r in 1..=reihen {
                for c in 1..=4 {
                    let (sx, sy) = (rechts - (c * 24) as f32 * s, unten - (r * 24) as f32 * s);
                    if ueberzaehlig && r * c == 1 {
                        let rest: i32 = inhalt[gezeigt.len()..].iter().map(|x| x.count).sum();
                        let text = format!("+{rest}");
                        let w = gpu.measure_text(&text, UI_SCALE_SMALL);
                        texts.push(UiText { text, x: (sx + 12.0 * s - w / 2.0).round(), y: (sy + 10.0 * s).round(), color: [1.0; 4], scale: UI_SCALE_SMALL });
                    } else if gezeigt.len() >= i {
                        let index = gezeigt.len() - i;
                        let gewaehlt = index as i32 == b.bundle_selected;
                        sprite(sprites, if gewaehlt { "container/bundle/slot_highlight_back" } else { "container/bundle/slot_background" }, sx, sy, 24.0, 24.0);
                        self.push_item(sprites, texts, &gezeigt[index], sx + 4.0 * s, sy + 4.0 * s, 16.0 * s);
                        if gewaehlt {
                            sprite(sprites, "container/bundle/slot_highlight_front", sx, sy, 24.0, 24.0);
                        }
                        i += 1;
                    }
                }
            }
            // Der Name des gewaehlten Items ueber dem Raster.
            if let Some(g) = usize::try_from(b.bundle_selected).ok().and_then(|i| inhalt.get(i)) {
                if let Some(name) = self.item_tooltip_lines(g).into_iter().next() {
                    let text: String = name.iter().map(|sp| sp.text.as_str()).collect();
                    let w = gpu.measure_text(&text, UI_SCALE_SMALL);
                    let farbe = name.first().and_then(|sp| sp.color).map(rgb_to_rgba).unwrap_or([1.0; 4]);
                    texts.push(UiText { text, x: (px + BUNDLE_PANEL_W * s / 2.0 - w / 2.0).round(), y: (py - gpu.text_height(UI_SCALE_SMALL) - 2.0 * s).round(), color: farbe, scale: UI_SCALE_SMALL });
                }
            }
            unten + 4.0 * s
        };
        let voll = gewicht == world::bundle::Fraction::ONE;
        let fuellung = gewicht.mul_trunc(94).min(94) as f32;
        sprite(sprites, if voll { "container/bundle/bundle_progressbar_full" } else { "container/bundle/bundle_progressbar_fill" }, px + s, balken_y, fuellung, 13.0);
        sprite(sprites, "container/bundle/bundle_progressbar_border", px, balken_y, BUNDLE_PANEL_W, 13.0);
        let beschriftung = if voll {
            Some(tr("item.minecraft.bundle.full", "Full"))
        } else if gewicht == world::bundle::Fraction::ZERO {
            Some(tr("item.minecraft.bundle.empty", "Empty"))
        } else {
            None
        };
        if let Some(text) = beschriftung {
            let w = gpu.measure_text(&text, UI_SCALE_SMALL);
            texts.push(UiText { text, x: (px + 48.0 * s - w / 2.0).round(), y: (balken_y + 3.0 * s).round(), color: [1.0; 4], scale: UI_SCALE_SMALL });
        }
    }

    // Zeilen des Item-Tooltips wie ItemStack.getTooltip, soweit dieser Client die Daten hat:
    // der Name in der Farbe seiner Seltenheit (eigene Farben im Namen gehen vor), darunter
    // die Verzauberungen und die Lore (Vorgabe dunkelviolett). Vorher stand der Name als
    // Klartext in Weiss da und die Lore fehlte ganz.
    // Was Zaubertisch, Amboss und Schleifstein ueber ihr Sheet zeichnen (EnchantmentScreen,
    // AnvilScreen, GrindstoneScreen). Rueckgabe: der Hinweis-Tooltip der Zauber-Option unter
    // dem Zeiger, falls es einen gibt.
    pub(crate) fn build_special_screen(
        &self,
        gpu: &GpuState,
        layout: &inventory::ScreenLayout,
        rects: &mut Vec<UiRect>,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
    ) -> Option<Vec<Vec<protocol::nbt::TextSpan>>> {
        let typ = self.open_menu_type()?;
        let s = layout.scale;
        let (bx, by) = (layout.bg_x, layout.bg_y);
        let sprite = |sprites: &mut Vec<UiSprite>, name: &str, gx: f32, gy: f32, w: f32, h: f32| {
            if let Some(r) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(bx + gx * s, by + gy * s, w * s, h * s, r.uv, [1.0; 4], SpriteAtlas::Ui));
            }
        };
        let hat = |slot: usize| self.inventory.slot_at_index(slot).is_some();
        let (cx, cy) = self.cursor_pos;
        match typ {
            inventory::MENU_ENCHANTMENT => {
                let data = self.inventory.open.as_ref().map(|c| c.data.clone()).unwrap_or_default();
                let wert = |i: usize| data.get(i).copied().unwrap_or(if i >= 4 { -1 } else { 0 }) as i32;
                let mut tooltip = None;
                for i in 0..3 {
                    let gy = 14.0 + 19.0 * i as f32;
                    let kosten = wert(i);
                    if kosten <= 0 {
                        sprite(sprites, "container/enchanting_table/enchantment_slot_disabled", 60.0, gy, 108.0, 19.0);
                        continue;
                    }
                    let (x, y, w, h) = inventory::enchant_option_rect(layout, i);
                    let ueber = cx >= x && cy >= y && cx < x + w && cy < y + h;
                    let frei = self.enchant_option_available(i);
                    let (hinter, stufe, farbe) = if !frei {
                        ("container/enchanting_table/enchantment_slot_disabled", format!("container/enchanting_table/level_{}_disabled", i + 1), 0x407F10)
                    } else if ueber {
                        ("container/enchanting_table/enchantment_slot_highlighted", format!("container/enchanting_table/level_{}", i + 1), 0x80FF20)
                    } else {
                        ("container/enchanting_table/enchantment_slot", format!("container/enchanting_table/level_{}", i + 1), 0x80FF20)
                    };
                    sprite(sprites, hinter, 60.0, gy, 108.0, 19.0);
                    sprite(sprites, &stufe, 61.0, gy + 1.0, 16.0, 16.0);
                    // Die Kosten rechtsbuendig unten in der Option, davor die Zierzeile in der
                    // Zauberschrift: auf den Platz links der Kosten gekuerzt, je nach Zustand
                    // braun, hellgelb (unter dem Zeiger) oder abgedunkelt.
                    let text = kosten.to_string();
                    let platz = 86.0 - gpu.measure_text(&text, 1.0);
                    let mut zeile = enchant_phrase(wert(3) as u32, i);
                    while self.alt_font.width(&zeile) > platz && zeile.pop().is_some() {}
                    let zier = if !frei {
                        (0x685E4A & 0xFEFEFE) >> 1
                    } else if ueber {
                        0xFFFF80
                    } else {
                        0x685E4A
                    };
                    self.push_alt_text(sprites, &zeile, bx + 80.0 * s, by + (gy + 2.0) * s, s, zier);
                    let tw = gpu.measure_text(&text, s);
                    texts.push(UiText {
                        text,
                        x: (bx + (60.0 + 20.0 + 86.0) * s - tw).round(),
                        y: (by + (gy + 2.0 + 7.0) * s).round(),
                        color: rgb_to_rgba(farbe),
                        scale: s,
                    });
                    // EnchantmentScreen.render: Hinweis "<Verzauberung> . . . ?" und was fehlt.
                    let (clue, level) = (wert(4 + i), wert(7 + i));
                    if ueber && clue >= 0 && level >= 0 {
                        let mut zeilen = Vec::new();
                        if let Some(mut name) = self.enchantments.name_by_id(clue, level) {
                            for span in &mut name {
                                span.color = Some(0xFFFFFF);
                            }
                            name.push(protocol::nbt::TextSpan { text: " . . . ?".into(), color: Some(0xFFFFFF), ..Default::default() });
                            zeilen.push(name);
                        }
                        if self.game_mode != GAME_MODE_CREATIVE {
                            let zeile = |text: String, farbe: u32| vec![protocol::nbt::TextSpan { text, color: Some(farbe), ..Default::default() }];
                            zeilen.push(zeile(String::new(), 0xAAAAAA));
                            if self.xp_level < kosten {
                                zeilen.push(zeile(format!("Benötigt Erfahrungsstufe {kosten}"), 0xFF5555));
                            } else {
                                let lapis = self.inventory.slot_at_index(1).map_or(0, |st| st.count);
                                let n = i as i32 + 1;
                                let lapis_text = if n == 1 { "1 Lapislazuli".to_string() } else { format!("{n} Lapislazuli") };
                                zeilen.push(zeile(lapis_text, if lapis >= n { 0xAAAAAA } else { 0xFF5555 }));
                                let stufen = if n == 1 { "1 Erfahrungsstufe".to_string() } else { format!("{n} Erfahrungsstufen") };
                                zeilen.push(zeile(stufen, 0xAAAAAA));
                            }
                        }
                        tooltip = Some(zeilen);
                    }
                }
                tooltip
            }
            inventory::MENU_ANVIL => {
                let feld = if hat(0) { "container/anvil/text_field" } else { "container/anvil/text_field_disabled" };
                sprite(sprites, feld, 59.0, 20.0, 110.0, 16.0);
                if (hat(0) || hat(1)) && !hat(2) {
                    sprite(sprites, "container/anvil/error", 99.0, 45.0, 28.0, 21.0);
                }
                if hat(0) {
                    // Das Feld hat immer den Fokus (setCanLoseFocus(false)): Text plus
                    // blinkender Strich am Ende.
                    let blink = (self.start_time.elapsed().as_millis() / 300).is_multiple_of(2);
                    let text = if blink { format!("{}_", self.anvil_name) } else { self.anvil_name.clone() };
                    push_typed_marker(texts);
                    texts.push(UiText { text, x: (bx + 62.0 * s).round(), y: (by + 24.0 * s).round(), color: [1.0; 4], scale: s });
                }
                let kosten = self.inventory.open.as_ref().and_then(|c| c.data.first().copied()).unwrap_or(0) as i32;
                if kosten > 0 {
                    let kreativ = self.game_mode == GAME_MODE_CREATIVE;
                    let text = if kosten >= 40 && !kreativ {
                        Some(("Zu teuer!".to_string(), 0xFF6060))
                    } else if !hat(2) {
                        None
                    } else {
                        let nehmbar = kreativ || self.xp_level >= kosten;
                        Some((format!("Erfahrungskosten: {kosten}"), if nehmbar { 0x80FF20 } else { 0xFF6060 }))
                    };
                    if let Some((text, farbe)) = text {
                        let tw = gpu.measure_text(&text, s) / s;
                        let gx = INVENTORY_GUI_W - 8.0 - tw - 2.0;
                        rects.push(UiRect {
                            rect: Rect { x: bx + (gx - 2.0) * s, y: by + 67.0 * s, w: (INVENTORY_GUI_W - 8.0 - (gx - 2.0)) * s, h: 12.0 * s },
                            color: [0.0, 0.0, 0.0, 0.31],
                            ..UiRect::FLAT
                        });
                        texts.push(UiText { text, x: (bx + gx * s).round(), y: (by + 69.0 * s).round(), color: rgb_to_rgba(farbe), scale: s });
                    }
                }
                None
            }
            inventory::MENU_GRINDSTONE => {
                if (hat(0) || hat(1)) && !hat(2) {
                    sprite(sprites, "container/grindstone/error", 92.0, 31.0, 28.0, 21.0);
                }
                None
            }
            // AbstractFurnaceScreen: Flamme (14x14, von unten gefuellt) solange Brennstoff
            // brennt, Pfeil (24x16, von links) nach dem Garfortschritt. Die Werte sind die
            // Container-Daten: 0 Restbrenndauer, 1 Brenndauer des Brennstoffs, 2 Garzeit
            // bisher, 3 Garzeit gesamt.
            inventory::MENU_FURNACE | inventory::MENU_BLAST_FURNACE | inventory::MENU_SMOKER => {
                let art = match typ {
                    inventory::MENU_BLAST_FURNACE => "blast_furnace",
                    inventory::MENU_SMOKER => "smoker",
                    _ => "furnace",
                };
                let data = self.inventory.open.as_ref().map(|c| c.data.clone()).unwrap_or_default();
                let wert = |i: usize| data.get(i).copied().unwrap_or(0) as f32;
                // Ausschnitt eines Sprites: Anteil (u0, v0, u1, v1) des ganzen Rechtecks.
                let teil = |sprites: &mut Vec<UiSprite>, name: &str, gx: f32, gy: f32, w: f32, h: f32, a: [f32; 4]| {
                    if let Some(r) = self.ui_atlas.rect(name) {
                        let (du, dv) = (r.uv[2] - r.uv[0], r.uv[3] - r.uv[1]);
                        let uv = [r.uv[0] + du * a[0], r.uv[1] + dv * a[1], r.uv[0] + du * a[2], r.uv[1] + dv * a[3]];
                        sprites.push(UiSprite::quad(bx + gx * s, by + gy * s, w * s, h * s, uv, [1.0; 4], SpriteAtlas::Ui));
                    }
                };
                if wert(0) > 0.0 {
                    let gesamt = if wert(1) == 0.0 { 200.0 } else { wert(1) };
                    let l = ((wert(0) / gesamt).clamp(0.0, 1.0) * 13.0).ceil() + 1.0;
                    teil(sprites, &format!("container/{art}/lit_progress"), 56.0, 36.0 + 14.0 - l, 14.0, l, [0.0, (14.0 - l) / 14.0, 1.0, 1.0]);
                }
                let gar = if wert(3) != 0.0 && wert(2) != 0.0 { (wert(2) / wert(3)).clamp(0.0, 1.0) } else { 0.0 };
                let w = (gar * 24.0).ceil();
                if w > 0.0 {
                    teil(sprites, &format!("container/{art}/burn_progress"), 79.0, 34.0, w, 16.0, [0.0, 0.0, w / 24.0, 1.0]);
                }
                None
            }
            inventory::MENU_MERCHANT => self.build_merchant_screen(layout, sprites, texts),
            _ => None,
        }
    }

    // MerchantScreen: Angebotsliste (Knoepfe mit Preis -> Ware), Schieber, Ausverkauft-Marke
    // und Erfahrungsleiste. Rueckgabe: der Tooltip fuer das, was unter dem Zeiger liegt.
    pub(crate) fn build_merchant_screen(&self, layout: &inventory::ScreenLayout, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) -> Option<Vec<Vec<protocol::nbt::TextSpan>>> {
        let container = self.inventory.open.as_ref()?;
        let st = &container.merchant;
        if st.offers.is_empty() {
            return None;
        }
        let s = layout.scale;
        let (bx, by) = (layout.bg_x, layout.bg_y);
        let (cx, cy) = self.cursor_pos;
        let sprite = |sprites: &mut Vec<UiSprite>, name: &str, gx: f32, gy: f32, w: f32, h: f32| {
            if let Some(r) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(bx + gx * s, by + gy * s, w * s, h * s, r.uv, [1.0; 4], SpriteAtlas::Ui));
            }
        };
        let in_gui = |gx: f32, gy: f32, w: f32, h: f32| cx >= bx + gx * s && cy >= by + gy * s && cx < bx + (gx + w) * s && cy < by + (gy + h) * s;
        let mut tooltip = None;

        // Ausverkauft: rotes Kreuz ueber dem Pfeil zwischen Bezahlung und Ergebnis.
        let gewaehlt = st.offers.get(st.selected);
        if gewaehlt.is_some_and(|o| o.disabled()) {
            sprite(sprites, "container/villager/out_of_stock", 182.0, 35.0, 28.0, 21.0);
            if st.can_restock && in_gui(186.0, 35.0, 22.0, 21.0) {
                let text = protocol::nbt::translate("merchant.deprecated").unwrap_or_else(|| "Dorfbewohner füllen ihre Vorräte bis zu zweimal am Tag auf.".into());
                tooltip = Some(vec![vec![protocol::nbt::TextSpan { text, ..Default::default() }]]);
            }
        }

        // Die Knoepfe (88x20, Neun-Teile-Sprite mit Rand 3 und gekachelter Mitte: bei 88
        // Breite sind das die linken 85 Spalten und die rechten 3).
        for i in 0..inventory::MERCHANT_VISIBLE_OFFERS.min(st.offers.len()) {
            let (x, y, _, _) = inventory::merchant_button_rect(layout, i);
            let ueber = cx >= x && cy >= y && cx < x + 88.0 * s && cy < y + 20.0 * s;
            let name = if ueber || st.focused == Some(i) { "widget/button_highlighted" } else { "widget/button" };
            if let Some(r) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(x, y, 85.0 * s, 20.0 * s, r.sub(0, 0, 85, 20), [1.0; 4], SpriteAtlas::Ui));
                sprites.push(UiSprite::quad(x + 85.0 * s, y, 3.0 * s, 20.0 * s, r.sub(197, 0, 3, 20), [1.0; 4], SpriteAtlas::Ui));
            }
        }

        // Schieber: grau oben, wenn alles hineinpasst.
        match st.scroller_offset() {
            Some(n) => sprite(sprites, "container/villager/scroller", 94.0, 18.0 + n, 6.0, 27.0),
            None => sprite(sprites, "container/villager/scroller_disabled", 94.0, 18.0, 6.0, 27.0),
        }

        // Die sichtbaren Angebote: erster Preis (bei Auf-/Abschlag mit durchgestrichenem
        // Grundpreis), zweiter Preis, Pfeil, Ware.
        let size = inventory::ICON_SIZE * s;
        let at = |gx: f32, gy: f32| (bx + gx * s, by + gy * s);
        let viele = st.offers.len() > inventory::MERCHANT_VISIBLE_OFFERS;
        let mut zeile = 0;
        for (idx, o) in st.offers.iter().enumerate() {
            if viele && (idx < st.scroll || idx >= st.scroll + inventory::MERCHANT_VISIBLE_OFFERS) {
                continue;
            }
            let gy = 19.0 + 20.0 * zeile as f32;
            zeile += 1;
            let preis = o.displayed_cost_a(world::item_components::max_stack(&o.cost_a));
            let (x, y) = at(10.0, gy);
            // Am Item steht immer der Preis, der wirklich gilt. Weicht er vom Grundpreis ab,
            // steht rechts daneben (wo Vanilla den neuen Preis zeigt) nur die Abweichung:
            // gruen, wenn es billiger ist, orange, wenn teurer. Kein Durchstreichen.
            let mut a = o.cost_a.clone();
            a.count = preis;
            self.push_item(sprites, texts, &a, x, y, size);
            let abweichung = preis - o.cost_a.count;
            if abweichung != 0 {
                let (x2, _) = at(24.0, gy);
                let (text, farbe) = if abweichung < 0 { (abweichung.to_string(), 0x55FF55) } else { (format!("+{abweichung}"), 0xFFAA00) };
                self.push_count_label(texts, &text, x2, y, size);
                if let Some(t) = texts.last_mut() {
                    t.color = rgb_to_rgba(farbe);
                }
            }
            if let Some(b) = &o.cost_b {
                let (x, y) = at(40.0, gy);
                self.push_item(sprites, texts, b, x, y, size);
            }
            let pfeil = if o.disabled() { "container/villager/trade_arrow_out_of_stock" } else { "container/villager/trade_arrow" };
            sprite(sprites, pfeil, 60.0, gy + 3.0, 10.0, 9.0);
            if let Some(r) = &o.result {
                let (x, y) = at(73.0, gy);
                self.push_item(sprites, texts, r, x, y, size);
            }
        }

        // Tooltip des Knopfs unter dem Zeiger: links der Preis, in der Mitte der zweite
        // Preis, rechts die Ware (MerchantScreen$WidgetButtonPage.renderTooltip).
        if tooltip.is_none() && self.inventory.cursor.is_none() {
            for i in 0..inventory::MERCHANT_VISIBLE_OFFERS.min(st.offers.len()) {
                let (x, y, w, h) = inventory::merchant_button_rect(layout, i);
                if !(cx >= x && cy >= y && cx < x + w && cy < y + h) {
                    continue;
                }
                let Some(o) = st.offers.get(i + st.scroll) else { break };
                let rel = (cx - x) / s;
                let stack = if rel < 20.0 {
                    let mut a = o.cost_a.clone();
                    a.count = o.displayed_cost_a(world::item_components::max_stack(&a));
                    Some(a)
                } else if rel > 30.0 && rel < 50.0 {
                    o.cost_b.clone()
                } else if rel > 65.0 {
                    o.result.clone()
                } else {
                    None
                };
                tooltip = stack.and_then(|st| self.item_hover_tooltip(&st));
                break;
            }
        }

        // Erfahrungsleiste des Dorfbewohners (nicht beim fahrenden Haendler und nicht auf
        // Meisterstufe): bisheriger Fortschritt in Gruen, was der Handel im Ergebnisslot
        // dazugaebe, heller dahinter.
        if st.show_progress && st.level < 5 {
            sprite(sprites, "container/villager/experience_bar_background", 136.0, 16.0, 102.0, 5.0);
            // Erfahrung, ab der eine Stufe beginnt (VillagerData: Novize..Meister).
            const STUFEN_XP: [i32; 5] = [0, 10, 70, 150, 250];
            if (1..5).contains(&st.level) {
                let (unten, oben) = (STUFEN_XP[st.level as usize - 1], STUFEN_XP[st.level as usize]);
                if st.xp >= unten {
                    let skala = 102.0 / (oben - unten) as f32;
                    let w = ((skala * (st.xp - unten) as f32).floor() as i32).min(102);
                    let teil = |sprites: &mut Vec<UiSprite>, name: &str, u: i32, breite: i32| {
                        if let (Some(r), true) = (self.ui_atlas.rect(name), breite > 0) {
                            sprites.push(UiSprite::quad(bx + (136.0 + u as f32) * s, by + 16.0 * s, breite as f32 * s, 5.0 * s, r.sub(u as u32, 0, breite as u32, 5), [1.0; 4], SpriteAtlas::Ui));
                        }
                    };
                    teil(sprites, "container/villager/experience_bar_current", 0, w);
                    let belohnung = self.merchant_reward_xp();
                    if belohnung > 0 {
                        let r = ((belohnung as f32 * skala).floor() as i32).min(102 - w);
                        teil(sprites, "container/villager/experience_bar_result", w, r);
                    }
                }
            }
        }
        tooltip
    }

    // Erfahrung, die der Handel im Ergebnisslot dem Dorfbewohner braechte: die des passenden
    // Angebots -- bevorzugt das ausgewaehlte, wie Vanillas Angebotssuche.
    pub(crate) fn merchant_reward_xp(&self) -> i32 {
        let Some(c) = self.inventory.open.as_ref() else { return 0 };
        let Some(Some(ergebnis)) = c.slots.get(2) else { return 0 };
        let passt = |o: &&protocol::packets::TradeOffer| o.result.as_ref().is_some_and(|r| r.item_id == ergebnis.item_id);
        c.merchant.offers.get(c.merchant.selected).filter(passt).or_else(|| c.merchant.offers.iter().find(passt)).map_or(0, |o| o.xp)
    }

    // Der Tooltip beim Ueberfahren eines Items: nur, wenn das Bild allein nicht verraet,
    // was es ist (s. item_needs_tooltip) -- ein Schneeball ist am Symbol erkennbar, ein
    // Schwert mit Schaerfe V nicht. Und dann nur das, was das Bild nicht zeigt (s.
    // item_tooltip_short). Mit gehaltener Untersuchen-Taste immer, und ausfuehrlich.
    pub(crate) fn item_hover_tooltip(&self, stack: &protocol::item::ItemStack) -> Option<Vec<Vec<protocol::nbt::TextSpan>>> {
        if self.inspect_held {
            return Some(self.item_tooltip_detailed(stack));
        }
        // tooltip_display: der Server will gar keinen Tooltip. Traenke (auch Wurf-/Verweil-
        // traenke und getippte Pfeile) erkennt man an der Farbe und dem Buchstaben in der
        // Ecke; was sie bewirken, zeigt die Untersuchen-Taste. Einen eigenen Namen oder Lore
        // zeigen Farbe und Buchstabe aber nicht -- dann gibt es den Tooltip doch.
        if stack.tooltip_hidden || !item_needs_tooltip(stack) || (stack.potion.is_some() && !item_shows_held_name(stack)) {
            return None;
        }
        let zeilen = protocol::nbt::with_lang_area(protocol::nbt::LangArea::Items, || self.item_tooltip_short(stack));
        // Ein Buendel zeigt seinen Inhalt auch ohne Textzeilen.
        (!zeilen.is_empty() || world::bundle::is_bundle(stack)).then_some(zeilen)
    }

    // Der kurze Tooltip: keinen Itemnamen -- welches Item es ist, zeigt das Bild. Ein
    // verzaubertes Schwert zeigt also nur seine Verzauberungen. Einen Namen gibt es nur, wo
    // er etwas sagt, das Bild und Verzauberungen nicht zeigen: ein vom Server oder Spieler
    // gesetzter Name an einem unverzauberten Item (Menue-Knoepfe auf Servern), der Titel eines
    // Buchs. Was der Server per tooltip_display versteckt, fehlt.
    pub(crate) fn item_tooltip_short(&self, stack: &protocol::item::ItemStack) -> Vec<Vec<protocol::nbt::TextSpan>> {
        let versteckt = |id: i32| stack.tooltip_hidden_components.contains(&id);
        let verzauberungen = if versteckt(if stack.enchantments.is_empty() { 41 } else { 13 }) { Vec::new() } else { self.enchantments.tooltip_lines(stack) };
        let buch = stack.written_book.as_ref().is_some_and(|b| !b.title.trim().is_empty());
        let eigener_name = stack.custom_name_spans.is_some() || stack.item_name_spans.is_some() || buch;
        let mut zeilen = Vec::new();
        if eigener_name && verzauberungen.is_empty() {
            // Der Name, bei einem Buch dazu Autor und Generation (die direkt darauf folgen).
            zeilen.extend(self.item_tooltip_lines_in_area(stack).into_iter().take(if buch { 3 } else { 1 }));
        }
        zeilen.extend(verzauberungen);
        if !versteckt(11) {
            zeilen.extend(stack.lore.iter().map(|z| z.iter().map(|s| protocol::nbt::TextSpan { color: s.color.or(Some(0xAA_00AA)), ..s.clone() }).collect()));
        }
        zeilen
    }

    // Was ein Item im Kampf bringt (Untersuchen-Taste). Waffen: Schaden mit den
    // Verzauberungen, die jeden Treffer verstaerken (Schaerfe), dazu der Krit (Grundschaden
    // mal 1,5, der Bonus kommt unverstaerkt dazu) und Schlaege je Sekunde. Ruestung: der Schutz
    // dieses Teils in Prozent -- Ruestungspunkte nehmen bei normalen Treffern je 4 % (bis 80 %),
    // Schutz-Verzauberungen danach je Punkt 4 % vom Rest (bis 80 %). Haerte zaehlt erst bei
    // grossen Treffern und steht deshalb nur daneben. Grundwerte des Spielers: 1 Schaden,
    // 4 Schlaege/s; Effekte wie Staerke zaehlen nicht.
    pub(crate) fn item_combat_lines(&self, stack: &protocol::item::ItemStack) -> Vec<Vec<protocol::nbt::TextSpan>> {
        use protocol::item as pi;
        let mods = world::item_components::Components::of(stack).attribute_modifiers();
        let hand = |slot: i32| matches!(slot, pi::SLOT_GROUP_ANY | pi::SLOT_GROUP_MAINHAND | pi::SLOT_GROUP_HAND);
        let koerper = |slot: i32| slot == pi::SLOT_GROUP_ANY || (pi::SLOT_GROUP_FEET..=pi::SLOT_GROUP_ARMOR + 1).contains(&slot);
        let wert = |attribute: i32, basis: f64, slot: &dyn Fn(i32) -> bool| item_attribute_value(mods, attribute, basis, slot);
        let zeile = |text: String, farbe: u32| vec![protocol::nbt::TextSpan { text, color: Some(farbe), ..Default::default() }];
        let mut zeilen = Vec::new();
        if let Some(schaden) = wert(pi::ATTRIBUTE_ATTACK_DAMAGE, 1.0, &hand) {
            let bonus = self.enchantments.damage_bonus(stack, schaden as f32) as f64;
            let krit = schaden * 1.5 + bonus;
            zeilen.push(zeile(format!("Schaden: {} (Krit {})", decimal_de(schaden + bonus), decimal_de(krit)), 0x00_AA00));
            if let Some(tempo) = wert(pi::ATTRIBUTE_ATTACK_SPEED, 4.0, &hand) {
                zeilen.push(zeile(format!("Tempo: {} Schläge/s", decimal_de(tempo)), 0x00_AA00));
            }
        }
        // Eigene Reichweite (Speere): von bis, in Bloecken ab dem Auge.
        if let Some(r) = world::item_components::Components::of(stack).attack_range() {
            let (min, max) = if self.game_mode == GAME_MODE_CREATIVE { (r.min_creative_reach, r.max_creative_reach) } else { (r.min_reach, r.max_reach) };
            let von = if min > 0.0 { format!("{}–", decimal_de(min as f64)) } else { String::new() };
            zeilen.push(zeile(format!("Reichweite: {von}{} Blöcke", decimal_de(max as f64)), 0x00_AA00));
        }
        let ruestung = wert(pi::ATTRIBUTE_ARMOR, 0.0, &koerper).unwrap_or(0.0);
        let haerte = wert(pi::ATTRIBUTE_ARMOR_TOUGHNESS, 0.0, &koerper).unwrap_or(0.0);
        use world::enchantment::ProtectionKind as Art;
        let allgemein = self.enchantments.protection(stack, Art::General) as f64;
        if ruestung > 0.0 || allgemein > 0.0 {
            let von_ruestung = ruestung.clamp(0.0, 20.0) / 25.0;
            let schutz = |punkte: f64| 1.0 - (1.0 - von_ruestung) * (1.0 - punkte.clamp(0.0, 20.0) / 25.0);
            zeilen.push(zeile(format!("Schutz: {} %", decimal_de((schutz(allgemein) * 100.0 * 10.0).round() / 10.0)), 0x00_AA00));
            let mut teile = vec![format!("{} Rüstung", decimal_de(ruestung))];
            if haerte > 0.0 {
                teile.push(format!("{} Härte", decimal_de(haerte)));
            }
            if allgemein > 0.0 {
                teile.push(format!("Verzauberung {} %", decimal_de(allgemein.min(20.0) * 4.0)));
            }
            zeilen.push(zeile(teile.join(" · "), 0xAA_AAAA));
            for (art, name) in [(Art::Projectile, "Geschosse"), (Art::Explosion, "Explosionen"), (Art::Fire, "Feuer"), (Art::Fall, "Fallschaden")] {
                let extra = self.enchantments.protection(stack, art) as f64;
                if extra > 0.0 {
                    zeilen.push(zeile(format!("gegen {name}: {} %", decimal_de((schutz(allgemein + extra) * 100.0 * 10.0).round() / 10.0)), 0x00_AA00));
                }
            }
        }
        zeilen
    }

    // Ausfuehrlicher Tooltip (Untersuchen-Taste): der normale plus Kampfwerte, Haltbarkeit,
    // Anzahl gesetzter Komponenten und die ID.
    pub(crate) fn item_tooltip_detailed(&self, stack: &protocol::item::ItemStack) -> Vec<Vec<protocol::nbt::TextSpan>> {
        let mut zeilen = self.item_tooltip_lines(stack);
        // Die Wirkung eines Tranks direkt unter dem Namen, wie in Vanilla.
        let wirkung = self.potion_effect_lines(stack);
        zeilen.splice(1.min(zeilen.len())..1.min(zeilen.len()), wirkung);
        zeilen.extend(self.item_combat_lines(stack));
        let item = world::item_registry::item_name(stack.item_id).unwrap_or("?");
        let zeile = |text: String, farbe: u32| vec![protocol::nbt::TextSpan { text, color: Some(farbe), ..Default::default() }];
        if let Some(max) = max_damage_of(stack, item).filter(|m| *m > 0) {
            let rest = max - stack.damage.unwrap_or(0);
            zeilen.push(zeile(format!("Haltbarkeit: {rest} / {max}"), 0xFFFFFF));
        }
        let anzahl = world::item_components::max_stack(stack);
        if anzahl > 1 {
            zeilen.push(zeile(format!("Stapel: {} / {anzahl}", stack.count), 0xAAAAAA));
        }
        if stack.has_added_components {
            zeilen.push(zeile("+ eigene Komponenten".to_string(), 0x555555));
        }
        zeilen.push(zeile(format!("minecraft:{item}"), 0x555555));
        zeilen
    }

    // PotionContents.addPotionTooltip: je Effekt Name und Stufe, dazu die Dauer (mal dem
    // Dauerfaktor des Items, s. world::potion::duration_scale), sofern sie laenger als eine
    // Sekunde ist -- Sofortwirkungen haben keine. Nuetzliche blau, schaedliche rot; ohne
    // Effekte "Keine Wirkung".
    pub(crate) fn potion_effect_lines(&self, stack: &protocol::item::ItemStack) -> Vec<Vec<protocol::nbt::TextSpan>> {
        let Some(inhalt) = stack.potion.as_ref() else { return Vec::new() };
        let item = world::item_registry::item_name(stack.item_id).unwrap_or("potion");
        let faktor = world::potion::duration_scale(item, stack);
        let effekte = world::potion::all_effects(inhalt);
        if effekte.is_empty() {
            let text = protocol::nbt::with_lang_area(protocol::nbt::LangArea::Effects, || protocol::nbt::translate("effect.none")).unwrap_or_else(|| "No Effects".to_string());
            return vec![vec![protocol::nbt::TextSpan { text, color: Some(0xAA_AAAA), ..Default::default() }]];
        }
        effekte
            .into_iter()
            .map(|(effect, amplifier, dauer)| {
                let effect = effect as i32;
                let dauer = if dauer == protocol::packets::MOB_EFFECT_INFINITE { dauer } else { (dauer as f32 * faktor).floor() as i32 };
                let mut text = effect_display_name(&EffectEntry { effect, ambient: false, amplifier, remaining: dauer, exact: dauer as f64, received: dauer });
                if dauer == protocol::packets::MOB_EFFECT_INFINITE || dauer > 20 {
                    text = format!("{text} ({})", format_effect_duration(dauer));
                }
                let farbe = if world::potion::effect_beneficial(effect) { 0x55_55FF } else { 0xFF_5555 };
                vec![protocol::nbt::TextSpan { text, color: Some(farbe), ..Default::default() }]
            })
            .collect()
    }

    pub(crate) fn item_tooltip_lines(&self, stack: &protocol::item::ItemStack) -> Vec<Vec<protocol::nbt::TextSpan>> {
        protocol::nbt::with_lang_area(protocol::nbt::LangArea::Items, || self.item_tooltip_lines_in_area(stack))
    }

    pub(crate) fn item_tooltip_lines_in_area(&self, stack: &protocol::item::ItemStack) -> Vec<Vec<protocol::nbt::TextSpan>> {
        let item = world::item_registry::item_name(stack.item_id).unwrap_or("?");
        let seltenheit = item_rarity(item, stack, !stack.enchantments.is_empty());
        let farbe = [None, Some(0xFF_FF55), Some(0x55_FFFF), Some(0xFF_55FF)][seltenheit as usize];
        let einfaerben = |spans: &[protocol::nbt::TextSpan], vorgabe: Option<u32>| -> Vec<protocol::nbt::TextSpan> {
            spans.iter().map(|s| protocol::nbt::TextSpan { color: s.color.or(vorgabe), ..s.clone() }).collect()
        };
        let buch = stack.written_book.as_ref().filter(|b| !b.title.trim().is_empty());
        let name = match stack.custom_name_spans.as_ref().or(stack.item_name_spans.as_ref()) {
            Some(spans) if !spans.is_empty() => einfaerben(spans, farbe),
            // Ein geschriebenes Buch heisst wie sein Titel (WrittenBookContent).
            _ if buch.is_some() => vec![protocol::nbt::TextSpan { text: buch.map(|b| b.title.clone()).unwrap_or_default(), color: farbe, ..Default::default() }],
            _ => {
                // Item.getName: item.minecraft.<id>, bei Blockitems block.minecraft.<id>.
                let text = world::potion::display_name(item, stack.potion.as_ref())
                    .or_else(|| protocol::nbt::translate(&format!("item.minecraft.{item}")))
                    .or_else(|| protocol::nbt::translate(&format!("block.minecraft.{item}")))
                    .unwrap_or_else(|| item.replace('_', " "));
                vec![protocol::nbt::TextSpan { text, color: farbe, ..Default::default() }]
            }
        };
        let mut zeilen = vec![name];
        // Darunter grau Autor und Generation (WrittenBookContent.addToTooltip).
        if let Some(b) = buch {
            let grau = |text: String| vec![protocol::nbt::TextSpan { text, color: Some(0xAA_AAAA), ..Default::default() }];
            let von = protocol::nbt::translate("book.byAuthor").unwrap_or_else(|| "by %1$s".to_string());
            zeilen.push(grau(von.replace("%1$s", &b.author).replace("%s", &b.author)));
            let generation = b.generation.clamp(0, 3);
            let standard = ["Original", "Copy of original", "Copy of a copy", "Tattered"][generation as usize];
            zeilen.push(grau(protocol::nbt::translate(&format!("book.generation.{generation}")).unwrap_or_else(|| standard.to_string())));
        }
        zeilen.extend(self.enchantments.tooltip_lines(stack));
        zeilen.extend(stack.lore.iter().map(|z| einfaerben(z, Some(0xAA_00AA))));
        zeilen
    }

    // --- Tab-Liste ----------------------------------------------------------------------

    // Das Hintergrundbild gehoert nur auf den Startbildschirm (getrennt, Hauptmenue).
    // Den Discord-Status an den aktuellen Zustand anpassen -- DiscordPresence schickt nur bei
    // einer Aenderung etwas.
    pub(crate) fn update_discord_status(&mut self) {
        let activity = self.settings.module_discord_status.then(|| discord_rpc::Activity {
            details: format!("Coral Client {}", env!("CARGO_PKG_VERSION")),
            state: match self.conn_status {
                // Discord zeigt den Text selbst, er laeuft also nicht durch shape_text. "auf {}"
                // steht mit Absicht nicht in der Tabelle: dort passte es auf jeden Text mit "auf ".
                ConnStatus::Connected => match crate::i18n::ui_language() {
                    crate::i18n::Language::German => format!("auf {}", self.server_address),
                    crate::i18n::Language::English => format!("on {}", self.server_address),
                    crate::i18n::Language::Latin => format!("in {}", self.server_address),
                },
                _ => crate::i18n::tr("im Menü").into_owned(),
            },
            start: self.discord_start,
        });
        let client_id = match self.settings.discord_client_id.as_str() {
            "" => discord_rpc::DEFAULT_CLIENT_ID,
            id => id,
        };
        self.discord.update(client_id, activity);
    }

    // Typ des gerade gezeigten Containerbildschirms, falls einer offen ist.
    pub(crate) fn open_menu_type(&self) -> Option<i32> {
        if !self.inventory.screen_open {
            return None;
        }
        self.inventory.open.as_ref().filter(|c| !c.slots.is_empty()).map(|c| c.screen_type)
    }

    // Tippt man gerade in das Namensfeld des Ambosses? Nur mit einem Gegenstand in Slot 0 --
    // wie in Vanilla, wo das Feld sonst nicht bearbeitbar ist. Dann gehen Tasten ins Feld
    // statt an die Belegungen (sonst schloesse ein "e" im Namen das Fenster).
    // Hat ein Textfeld den Fokus (Amboss-Name, Kreativ-Suche)? Dann gehoeren die Tasten ihm
    // und nicht der Steuerung.
    pub(crate) fn text_field_focused(&self) -> bool {
        self.anvil_typing()
            || self.creative_search_focused()
            || self.book_edit.is_some()
            || self.dialog_shown()
            || self.code_of_conduct.is_some()
            || self.theme_name_dialog.is_some()
    }

    pub(crate) fn anvil_typing(&self) -> bool {
        self.open_menu_type() == Some(inventory::MENU_ANVIL) && self.inventory.slot_at_index(0).is_some()
    }

    // Klartext-Name eines Stapels (Vanillas getHoverName().getString()).
    pub(crate) fn stack_plain_name(&self, stack: &protocol::item::ItemStack) -> String {
        self.item_tooltip_lines(stack).first().map(|z| z.iter().map(|s| s.text.as_str()).collect()).unwrap_or_default()
    }

    // AnvilScreen.slotChanged: wechselt der Gegenstand in Slot 0, steht sein Name im Feld.
    pub(crate) fn sync_anvil_name(&mut self) {
        if self.open_menu_type() != Some(inventory::MENU_ANVIL) {
            self.anvil_name_for = None;
            return;
        }
        let stack = self.inventory.slot_at_index(0).cloned();
        let schluessel = stack.as_ref().map(|st| format!("{}:{:?}", st.item_id, st.custom_name_spans));
        if schluessel != self.anvil_name_for {
            self.anvil_name = stack.as_ref().map(|st| self.stack_plain_name(st)).unwrap_or_default();
            self.anvil_name_for = schluessel;
        }
    }

    // Taste im Namensfeld. Escape schliesst, Ruecktaste loescht, Strg+V fuegt ein, sonst Text
    // bis 50 Zeichen (EditBox.setMaxLength). Jede Aenderung geht als Rename-Paket raus --
    // gleich dem Standardnamen als leerer Name (AnvilScreen.onNameChanged).
    pub(crate) fn anvil_name_key(&mut self, key: &Key, text: Option<&str>, control: bool, paste: bool) {
        let vorher = self.anvil_name.clone();
        match key {
            Key::Named(NamedKey::Escape) => {
                self.close_inventory();
                return;
            }
            Key::Named(NamedKey::Backspace) => {
                self.anvil_name.pop();
            }
            _ => {
                let einfuegen = if paste {
                    arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok())
                } else if control {
                    None
                } else {
                    text.map(str::to_string)
                };
                for ch in einfuegen.unwrap_or_default().chars().filter(|c| !c.is_control() && *c != '\u{a7}') {
                    if self.anvil_name.chars().count() >= 50 {
                        break;
                    }
                    self.anvil_name.push(ch);
                }
            }
        }
        if self.anvil_name == vorher {
            return;
        }
        let Some(stack) = self.inventory.slot_at_index(0).cloned() else { return };
        let name = if stack.custom_name_spans.is_none() && self.anvil_name == self.stack_plain_name(&stack) { String::new() } else { self.anvil_name.clone() };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::rename_item_payload(&name));
        }
    }

    // Zaubertisch: Option `i` waehlbar? (EnchantmentMenu.clickMenuButton: Kosten gesetzt,
    // genug Lapis und Stufen -- im Kreativmodus reicht die Kosten-Bedingung.)
    pub(crate) fn enchant_option_available(&self, i: usize) -> bool {
        let Some(c) = self.inventory.open.as_ref() else { return false };
        let kosten = c.data.get(i).copied().unwrap_or(0) as i32;
        if kosten <= 0 {
            return false;
        }
        let lapis = self.inventory.slot_at_index(1).map_or(0, |s| s.count);
        self.game_mode == GAME_MODE_CREATIVE || (lapis > i as i32 && self.xp_level >= kosten)
    }

    // Buch-Animation des Zaubertischs: nur solange sein Bildschirm offen ist. Beim Oeffnen
    // startet sie zugeklappt (Vanilla baut den Bildschirm jedes Mal neu).
    pub(crate) fn tick_enchant_book(&mut self) {
        if self.open_menu_type() != Some(inventory::MENU_ENCHANTMENT) {
            if self.book_anim.last_item.is_some() || self.book_anim.open > 0.0 || self.book_anim.prev_open > 0.0 {
                let rng = self.book_anim.rng;
                self.book_anim = BookAnim { rng, ..Default::default() };
            }
            return;
        }
        let item = self.inventory.slot_at_index(0).map(|s| (s.item_id, s.count));
        let data = self.inventory.open.as_ref().map(|c| c.data.clone()).unwrap_or_default();
        let any_option = (0..3).any(|i| data.get(i).copied().unwrap_or(0) != 0);
        self.book_anim.tick(item, any_option);
    }

    // Das aufgeschlagene Buch links oben im Zaubertisch (EnchantmentScreen.drawBook +
    // BookModelGuiElementRenderer), als 3D-Geometrie fuer den Figuren-Durchgang. Gerechnet in
    // Bildschirmpixeln mit Y nach OBEN (s. book_camera): Vanillas Kette laeuft im GUI-Raum
    // mit Y nach unten; die eine Spiegelung vorneweg macht daraus wieder ein Rechtssystem,
    // dann bleiben Umlaufsinn und Licht wie bei jedem anderen Modell.
    pub(crate) fn build_enchant_book(&self, rect: inventory::DollRect, screen: (f32, f32)) -> Option<(Vec<world::BlockVertex>, glam::Mat4, glam::Mat4)> {
        let tex = self.entity_atlas.rect(world::entity_model::ENCHANTING_BOOK_TEXTURE)?;
        let (open, flip) = self.book_anim.at(self.tick_progress());
        let out = enchant_book_vertices(&self.book_model, tex, rect, open, flip);
        let (view, proj) = book_camera(screen);
        Some((out, view, proj))
    }

    // Text in der Zauberschrift (font/alt.json) als Sprites aus dem ascii_sga-Sheet. `x`/`y`
    // in Bildschirmpixeln (Oberkante der Zeile), `s` die GUI-Skala. Ohne Schatten, wie
    // Vanillas Zierzeilen.
    pub(crate) fn push_alt_text(&self, sprites: &mut Vec<UiSprite>, text: &str, x: f32, y: f32, s: f32, farbe: u32) {
        let Some(sheet) = self.ui_atlas.rect("font/ascii_sga") else { return };
        let color = rgb_to_rgba(farbe);
        let mut gx = x;
        for c in text.chars() {
            if c == ' ' {
                gx += self.alt_font.space_advance * s;
                continue;
            }
            let Some(g) = self.alt_font.glyphs.get(&c) else { continue };
            let [px, py, pw, ph] = g.cell;
            if pw > 0 {
                sprites.push(UiSprite::quad(gx, y, pw as f32 * s, ph as f32 * s, sheet.sub(px, py, pw, ph), color, SpriteAtlas::Ui));
            }
            gx += g.advance * s;
        }
    }

    // Handelsbildschirm: Angebot `i` auswaehlen (MerchantScreen.syncRecipeIndex). Der Server
    // legt daraufhin die passenden Items aus dem Inventar in die Bezahlslots.
    pub(crate) fn select_merchant_offer(&mut self, i: usize) {
        let Some(c) = self.inventory.open.as_mut() else { return };
        if i >= c.merchant.offers.len() {
            return;
        }
        c.merchant.selected = i;
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::select_trade_payload(i as i32));
        }
        self.play_ui_sound(ui_sound::UiSound::Click);
    }

    // Schieber der Angebotsliste ziehen: folgt dem Zeiger, solange die Taste unten ist.
    pub(crate) fn merchant_drag(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (_, bahn_y, _, _) = inventory::merchant_scroll_rect(&layout);
        let rel = (self.cursor_pos.1 - bahn_y) / layout.scale;
        if let Some(c) = self.inventory.open.as_mut().filter(|c| c.merchant.dragging) {
            c.merchant.drag_to(rel);
        }
    }
}
