// Serverliste: Status, Suche, Scrollen, Ziehen, Serverdialog und Kontextmenue.

use super::*;

impl App {
    // Jeden gespeicherten Server anpingen, dessen Ergebnis fehlt oder veraltet ist. Im
    // Hintergrund (tokio), mit Zeitlimit; das Ergebnis holt drain_server_status ab.
    pub(crate) fn refresh_server_status(&mut self) {
        let adressen: Vec<String> = self.servers.iter().map(|s| s.address.clone()).collect();
        for adresse in adressen {
            let faellig = match self.server_status.get(&adresse) {
                None => true,
                Some(e) => !matches!(e.state, server_status::State::Pending) && e.since.elapsed() > server_status::REFRESH,
            };
            if !faellig {
                continue;
            }
            let slot = self.server_status.get(&adresse).and_then(|e| e.icon_slot);
            self.server_status.insert(adresse.clone(), server_status::Entry { state: server_status::State::Pending, since: Instant::now(), icon_slot: slot });
            let tx = self.server_status_tx.clone();
            self.tokio_rt.spawn(async move {
                let ergebnis = tokio::time::timeout(server_status::TIMEOUT, net::ping(&adresse)).await;
                let _ = tx.send((adresse, ergebnis.ok().and_then(std::result::Result::ok)));
            });
        }
    }

    pub(crate) fn drain_server_status(&mut self) {
        while let Ok((adresse, ergebnis)) = self.server_status_rx.try_recv() {
            let status = ergebnis.and_then(|json| server_status::parse(&json));
            let alt_slot = self.server_status.get(&adresse).and_then(|e| e.icon_slot);
            let mut slot = alt_slot;
            if let Some(icon) = status.as_ref().and_then(|s| s.icon.as_ref()) {
                let neu = slot.unwrap_or_else(|| {
                    let n = self.next_icon_slot;
                    self.next_icon_slot = (n + 1) % UNKNOWN_SERVER_ICON_SLOT;
                    n
                });
                // Ein Platz, der reihum wieder vergeben wird, gehoert nur noch dem neuen.
                for e in self.server_status.values_mut() {
                    if e.icon_slot == Some(neu) {
                        e.icon_slot = None;
                    }
                }
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.upload_server_icon(neu, icon);
                }
                slot = Some(neu);
            }
            if slot.is_none() {
                self.upload_unknown_server_icon();
            }
            let state = match status {
                Some(mut st) => {
                    st.icon = None;
                    server_status::State::Ok(st)
                }
                None => server_status::State::Failed,
            };
            self.server_status.insert(adresse, server_status::Entry { state, since: Instant::now(), icon_slot: slot });
        }
    }

    // Das Vanilla-Bild fuer Server ohne Icon einmal in den Atlas legen -- erst, wenn ein Server
    // es braucht.
    pub(crate) fn upload_unknown_server_icon(&mut self) {
        if self.unknown_server_icon {
            return;
        }
        let Ok(tex) = self.installation.load_texture("minecraft/textures/misc/unknown_server.png") else { return };
        let Some(rgba) = server_status::fit_icon(tex.width, tex.height, tex.rgba) else { return };
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.upload_server_icon(UNKNOWN_SERVER_ICON_SLOT, &rgba);
            self.unknown_server_icon = true;
        }
    }

    // Welches Bild die Karte eines Servers zeigt: sein eigenes Icon, sonst -- sobald der Ping
    // fertig ist, also auch ohne Antwort -- das Vanilla-Bild fuer unbekannte Server. Waehrend
    // des Pings ohne Icon steht dort der Ladekreis (s. loading_dots).
    pub(crate) fn server_list_icon(&self, address: &str) -> Option<usize> {
        let eintrag = self.server_status.get(address)?;
        let fertig = !matches!(eintrag.state, server_status::State::Pending);
        eintrag.icon_slot.or((fertig && self.unknown_server_icon).then_some(UNKNOWN_SERVER_ICON_SLOT))
    }

    // Karten, die gerade (ganz oder teilweise) in der Liste stehen: Index und Rechteck beim
    // gleitenden Scrollstand. Die ausgewaehlte ragt nach links heraus.
    // Waehrend eine Karte gezogen wird, fehlt sie, und die uebrigen ruecken zusammen.
    // Bei offener Suche nur die Treffer, und die ruecken um einen Platz nach unten: oben steht
    // das Suchfeld.
    pub(crate) fn server_cards(&self) -> Vec<(usize, Rect)> {
        let s = self.server_scroll.stand;
        let gezogen = self.server_drag_active();
        let versatz = usize::from(self.server_search.is_some());
        let reihe: Vec<usize> = self.server_search_hits().into_iter().filter(|&i| Some(i) != gezogen).collect();
        let von = (s.floor() as isize - 1 - versatz as isize).max(0) as usize;
        let bis = (s.ceil() as usize + server_rows_visible() + 1).min(reihe.len());
        (von..bis)
            .map(|platz| {
                (reihe[platz], server_card_rect(platz + versatz, s))
            })
            .filter(|(_, r)| r.y + r.h > server_list_top() && r.y < server_list_bottom())
            .collect()
    }

    // So weit laesst sich die Serverliste scrollen, in Karten.
    pub(crate) fn server_max_scroll(&self) -> f32 {
        // Bruchteil erlaubt: ganz unten schliesst die letzte Karte mit der Unterkante ab.
        let n = (self.server_search_hits().len() + usize::from(self.server_search.is_some())) as f32;
        (n - server_rows_fit()).max(0.0)
    }

    // Eintraege, die zur Suche passen, in Listenreihenfolge; ohne Suche alle. Gesucht wird
    // ohne Gross/klein in Name, Adresse und MOTD.
    pub(crate) fn server_search_hits(&self) -> Vec<usize> {
        let Some(suche) = self.server_search.as_ref().map(|f| f.text().to_lowercase()) else {
            return (0..self.servers.len()).collect();
        };
        (0..self.servers.len())
            .filter(|&i| {
                let entry = &self.servers[i];
                let mut text = format!("{}\n{}", entry.name, entry.address);
                if let Some(server_status::State::Ok(st)) = self.server_status.get(&entry.address).map(|e| &e.state) {
                    for zeile in &st.motd {
                        text.push('\n');
                        text.extend(zeile.iter().map(|span| span.text.as_str()));
                    }
                }
                text.to_lowercase().contains(&suche)
            })
            .collect()
    }

    // Wird gerade in die Serversuche getippt? Fenster darueber haben Vorrang.
    pub(crate) fn server_search_typing(&self) -> bool {
        self.server_search.is_some()
            && self.mode == AppMode::Menu
            && self.screen == MenuScreen::Connect
            && !self.quick_connect_open
            && self.server_edit.is_none()
            && self.theme_name_dialog.is_none()
            && self.dialog.is_none()
    }

    // Erstes getipptes Zeichen auf dem Connect-Screen, solange sonst nichts offen ist: oeffnet
    // die Suche damit. Leerzeichen und Steuerzeichen oeffnen nichts.
    pub(crate) fn server_search_start(&mut self, text: &str) -> bool {
        let frei = self.mode == AppMode::Menu
            && self.screen == MenuScreen::Connect
            && self.server_search.is_none()
            && !self.quick_connect_open
            && self.server_edit.is_none()
            && self.server_context.is_none()
            && self.dialog.is_none()
            && self.disconnect_popup.is_none()
            && !self.account_menu_open
            && !self.account_manager_open
            && !self.notification_panel_open
            && self.dropdown.is_none();
        if !frei || !text.chars().next().is_some_and(|c| !c.is_control() && !c.is_whitespace()) {
            return false;
        }
        let mut feld = text_field::TextField::new("", SERVER_SEARCH_MAX_CHARS);
        feld.insert(text);
        self.server_search = Some(feld);
        true
    }

    // Enter in der Suche: wie in osu! mit dem ersten Treffer verbinden. Ohne Treffer nur der
    // Ablehnungsklang.
    pub(crate) fn server_search_submit(&mut self) {
        let Some(&i) = self.server_search_hits().first() else {
            self.play_ui_sound(ui_sound::UiSound::Denied);
            return;
        };
        let action = ButtonAction::ConnectToServer(i);
        self.play_ui_sound(ui_sound_for_action(action, &self.settings));
        self.run_menu_action(action);
    }

    // Bildlaufleiste links neben den Karten: Spur und Griff beim gezeichneten Stand. None,
    // solange alle Karten (und das Suchfeld) passen.
    pub(crate) fn server_scrollbar(&self) -> Option<(Rect, Rect)> {
        let max = self.server_max_scroll();
        if max <= 0.0 {
            return None;
        }
        let p = server_panel();
        let (oben, unten) = (server_list_top(), server_list_bottom());
        let spur = Rect { x: p.x - SERVER_BAR_GAP - SERVER_BAR_W, y: oben, w: SERVER_BAR_W, h: unten - oben };
        let sichtbar = server_rows_fit();
        let griff_h = (spur.h * sichtbar / (max + sichtbar)).max(32.0);
        let griff = Rect { y: spur.y + (spur.h - griff_h) * (self.server_scroll.stand / max).clamp(0.0, 1.0), h: griff_h, ..spur };
        Some((spur, griff))
    }

    // Flaeche, in der ein Druck die Leiste trifft: etwas breiter als sie selbst.
    pub(crate) fn server_bar_hit(&self) -> Option<Rect> {
        self.server_scrollbar().map(|(spur, _)| Rect { x: spur.x - SERVER_BAR_HIT, w: spur.w + 2.0 * SERVER_BAR_HIT, ..spur })
    }

    // Linke Taste auf der Leiste: am Griff fasst sie ihn, wo man ihn gepackt hat; daneben
    // springt seine Mitte unter den Zeiger. Ziehen fuehrt weiter (s. server_bar_drag).
    pub(crate) fn server_bar_press(&mut self) -> bool {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::Connect || self.server_edit.is_some() {
            return false;
        }
        let Some((_, griff)) = self.server_scrollbar() else { return false };
        let (cx, cy) = self.cursor_pos;
        if !self.server_bar_hit().is_some_and(|r| r.contains(cx, cy)) {
            return false;
        }
        let fass = if cy >= griff.y && cy <= griff.y + griff.h { cy - griff.y } else { griff.h / 2.0 };
        self.server_bar_grab = Some(fass);
        self.server_bar_drag();
        true
    }

    // Der gefasste Griff folgt dem Zeiger, die Liste sofort mit (ohne Gleiten).
    pub(crate) fn server_bar_drag(&mut self) {
        let Some(fass) = self.server_bar_grab else { return };
        let Some((spur, griff)) = self.server_scrollbar() else {
            self.server_bar_grab = None;
            return;
        };
        let t = ((self.cursor_pos.1 - fass - spur.y) / (spur.h - griff.h).max(1.0)).clamp(0.0, 1.0);
        self.server_scroll.jump(t * self.server_max_scroll());
    }

    // Mausrad in Einheiten einer Liste, Richtung Listenende positiv: eine Raste sind `je_raste`
    // Einheiten mal Scrollweite; ein Touchpad (Pixel) folgt dem Finger, `px` Pixel je Einheit.
    pub(crate) fn wheel_units(&self, delta: winit::event::MouseScrollDelta, je_raste: f32, px: f32) -> f32 {
        match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => -y * je_raste * self.settings.scroll_distance as f32 / 100.0,
            winit::event::MouseScrollDelta::PixelDelta(p) => -p.y as f32 / px,
        }
    }

    // Trefferflaechen der Karten rechts auf dem Connect-Screen. Die Karten zeichnet
    // build_server_panel selbst (Karte statt Knopf): hier nur die Flaechen, ohne Farbe und
    // Label, auf den sichtbaren Teil der Liste beschnitten. Gedrueckt wird ueber
    // server_card_press (Klick erst beim Loslassen, sonst koennte man nicht ziehen); die
    // Knoepfe hier liefern Hover und Klang. Quick Connect und Neuer Eintrag liegen in der
    // Leiste unten (s. chrome_buttons).
    pub(crate) fn server_panel_buttons(&self, out: &mut Vec<Button>) {
        if self.server_drag_active().is_some() || self.server_edit.is_some() {
            return;
        }
        for (i, karte) in self.server_cards() {
            let y0 = karte.y.max(server_list_top());
            let y1 = (karte.y + karte.h).min(server_list_bottom());
            if y1 - y0 < 8.0 {
                continue;
            }
            out.push(Button {
                action: ButtonAction::ConnectToServer(i),
                rect: Rect { y: y0, h: y1 - y0, ..karte },
                label: String::new(),
                enabled: true,
                color: SERVER_CARD_MARKER,
                scale: UI_SCALE_SMALL,
                slider: None,
            });
        }
    }

    // Karte unter dem Zeiger (nur der sichtbare Teil der Liste zaehlt).
    pub(crate) fn server_card_at(&self, x: f32, y: f32) -> Option<usize> {
        if y < server_list_top() || y > server_list_bottom() {
            return None;
        }
        self.server_cards().into_iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| i)
    }

    // Welche Karte gerade gezogen wird -- erst ab SERVER_DRAG_START_PX Bewegung, danach bis
    // zum Loslassen.
    pub(crate) fn server_drag_active(&self) -> Option<usize> {
        let d = self.server_drag.as_ref()?;
        if !d.moved.get() {
            let (x, y) = self.cursor_pos;
            if (x - d.press.0).abs() + (y - d.press.1).abs() > SERVER_DRAG_START_PX {
                d.moved.set(true);
            }
        }
        // In der gefilterten Liste gibt es keine sinnvolle Luecke zum Ablegen.
        d.moved.get().then_some(d.index).filter(|_| self.server_search.is_none())
    }

    // Wohin die gezogene Karte fiele: Platz 0..=n unter den uebrigen n Karten, die Luecke
    // dem Zeiger am naechsten. None ausserhalb der Liste (Loslassen dort bricht ab).
    pub(crate) fn server_drop_slot(&self) -> Option<usize> {
        let (x, y) = self.cursor_pos;
        let p = server_panel();
        if x < p.x || x > p.x + p.w || y < p.y || y > p.y + p.h {
            return None;
        }
        let n = self.servers.len().saturating_sub(1);
        let schritt = server_row_h() + SERVER_ROW_GAP;
        let platz = ((y - server_list_top() + SERVER_ROW_GAP / 2.0) / schritt + self.server_scroll.stand + 0.5).floor();
        Some((platz.max(0.0) as usize).min(n))
    }

    // Linke Taste auf einer Karte: sofort verbinden, schon beim Druecken.
    pub(crate) fn server_card_connect(&mut self) -> bool {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::Connect || self.server_edit.is_some() {
            return false;
        }
        let (x, y) = self.cursor_pos;
        let Some(i) = self.server_card_at(x, y) else { return false };
        let action = ButtonAction::ConnectToServer(i);
        self.play_ui_sound(ui_sound_for_action(action, &self.settings));
        self.run_menu_action(action);
        true
    }

    // Rechte Taste auf einer Karte: nur vormerken. Ob es ein Ziehen (verschieben) oder ein
    // Klick (Kontextmenue) wird, entscheidet die Bewegung bis zum Loslassen (s.
    // server_drag_release).
    pub(crate) fn server_card_press(&mut self) {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::Connect || self.server_edit.is_some() {
            return;
        }
        let (x, y) = self.cursor_pos;
        let Some(i) = self.server_card_at(x, y) else { return };
        self.server_drag = Some(ServerDrag { index: i, press: (x, y), moved: std::cell::Cell::new(false) });
    }

    pub(crate) fn server_drag_release(&mut self) {
        let gezogen = self.server_drag_active();
        let ziel = self.server_drop_slot();
        let Some(d) = self.server_drag.take() else { return };
        let Some(i) = gezogen else {
            // Nicht bewegt: ein Rechtsklick auf die Karte, also ihr Kontextmenue.
            if !d.moved.get() {
                self.server_context = Some(ServerContextMenu { index: d.index, at: d.press });
            }
            return;
        };
        let Some(ziel) = ziel.filter(|_| i < self.servers.len()) else { return };
        let eintrag = self.servers.remove(i);
        self.servers.insert(ziel, eintrag);
        self.play_ui_sound(ui_sound::UiSound::Click);
        self.persist_servers();
    }

    pub(crate) fn open_server_dialog(&mut self, index: Option<usize>) {
        let (name, address, view_last, show_name) = match index.and_then(|i| self.servers.get(i)) {
            Some(e) => (e.name.as_str(), e.address.as_str(), e.view_last, e.show_name),
            None => ("", "", None, true),
        };
        self.server_edit = Some(ServerEdit {
            index,
            name: text_field::TextField::new(name, MAX_SERVER_NAME_LEN),
            address: text_field::TextField::new(address, MAX_ADDRESS_LEN),
            focus: ServerField::Address,
            dragging: false,
            last_press: None,
            view_last,
            show_name,
        });
        self.server_context = None;
    }

    // Zeichengrenze im Feld `f` unter dem Zeiger (Text beginnt 12 px nach dem Feldrand).
    pub(crate) fn server_field_index(&self, f: ServerField, x: f32) -> usize {
        let (Some(gpu), Some(edit)) = (self.gpu.as_ref(), self.server_edit.as_ref()) else { return 0 };
        let (_, adresse, name, _, _) = server_dialog_layout();
        let feld = if f == ServerField::Address { adresse } else { name };
        text_field::index_at(edit.field(f).text(), x - feld.x - 12.0, |t| gpu.measure_raw(t, UI_SCALE_NORMAL))
    }

    // Zeiger bewegt, waehrend im Feld gedrueckt ist: Markierung bis hierher (Server-Fenster
    // oder Quick Connect).
    pub(crate) fn server_field_drag(&mut self) {
        if self.theme_name_dialog.as_ref().is_some_and(|d| d.dragging) {
            let i = self.theme_name_index(self.cursor_pos.0);
            if let Some(d) = self.theme_name_dialog.as_mut() {
                d.field.drag_to(i);
            }
        }
        if self.quick_dragging && self.quick_connect_open {
            let i = self.quick_field_index(self.cursor_pos.0);
            self.quick_field.drag_to(i);
        }
        let Some(f) = self.server_edit.as_ref().filter(|e| e.dragging).map(|e| e.focus) else { return };
        let i = self.server_field_index(f, self.cursor_pos.0);
        if let Some(edit) = self.server_edit.as_mut() {
            edit.focused().drag_to(i);
        }
    }

    // Linke Taste bei offenem Server-Fenster. Immer verbraucht.
    pub(crate) fn server_dialog_press(&mut self) {
        let (p, adresse, name, speichern, abbrechen) = server_dialog_layout();
        let (cx, cy) = self.cursor_pos;
        let feld = [(adresse, ServerField::Address), (name, ServerField::Name)].into_iter().find(|(r, _)| r.contains(cx, cy)).map(|(_, f)| f);
        if let Some(f) = feld {
            let i = self.server_field_index(f, cx);
            let shift = self.modifiers.shift_key();
            let jetzt = Instant::now();
            let Some(edit) = self.server_edit.as_mut() else { return };
            let doppelt = edit.focus == f
                && edit.last_press.is_some_and(|(t, p)| jetzt.duration_since(t).as_millis() <= MULTI_CLICK_MS && (cx - p.0).abs() + (cy - p.1).abs() <= MULTI_CLICK_PX);
            if edit.focus != f {
                edit.focus = f;
            }
            if doppelt {
                edit.focused().select_word_at(i);
                edit.last_press = None;
                self.selection_sound_hint = Some(ui_sound::UiSound::Step);
            } else {
                edit.focused().press(i, shift);
                edit.dragging = true;
                edit.last_press = Some((jetzt, (cx, cy)));
            }
            return;
        }
        let Some(edit) = self.server_edit.as_mut() else { return };
        if let Some(i) = server_dialog_view_buttons().iter().position(|r| r.contains(cx, cy)) {
            edit.view_last = Some(i == 1);
            self.play_ui_sound(ui_sound::UiSound::Click);
            return;
        }
        if server_dialog_name_toggle().contains(cx, cy) {
            edit.show_name = !edit.show_name;
            let ton = if edit.show_name { ui_sound::UiSound::ToggleOn } else { ui_sound::UiSound::ToggleOff };
            self.play_ui_sound(ton);
            return;
        }
        if speichern.contains(cx, cy) {
            if edit.address.text().trim().is_empty() {
                self.play_ui_sound(ui_sound::UiSound::Denied);
            } else {
                self.play_ui_sound(ui_sound::UiSound::Click);
                self.commit_server_edit();
            }
        } else if abbrechen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.server_edit = None;
        }
    }

    // Rechtsklick-Menue: Fenster und die Zeilen Bearbeiten/Loeschen, am Zeiger aufgeklappt und
    // in den Bildschirm geschoben.
    pub(crate) fn server_context_layout(&self) -> Option<(Rect, [Rect; 2])> {
        let m = self.server_context.as_ref()?;
        let (sw, sh) = ui_screen();
        let h = 2.0 * SERVER_CONTEXT_ROW_H + 12.0;
        let x = m.at.0.min(sw - SERVER_CONTEXT_W - 8.0);
        let y = m.at.1.min(sh - h - 8.0);
        let p = Rect { x, y, w: SERVER_CONTEXT_W, h };
        let zeile = |n: usize| Rect { x: p.x + 6.0, y: p.y + 6.0 + n as f32 * SERVER_CONTEXT_ROW_H, w: p.w - 12.0, h: SERVER_CONTEXT_ROW_H };
        Some((p, [zeile(0), zeile(1)]))
    }

    // Linke Taste bei offenem Rechtsklick-Menue: Zeile ausfuehren, daneben schliesst es nur.
    pub(crate) fn server_context_press(&mut self) {
        let Some((_, [bearbeiten, loeschen])) = self.server_context_layout() else { return };
        let Some(i) = self.server_context.take().map(|m| m.index).filter(|&i| i < self.servers.len()) else { return };
        let (cx, cy) = self.cursor_pos;
        if bearbeiten.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Open);
            self.open_server_dialog(Some(i));
        } else if loeschen.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.servers.remove(i);
            self.persist_servers();
        }
    }

    // Formular aus dem Formularzustand in die Liste uebernehmen und sofort schreiben.
    // Ohne Adresse bleibt das Fenster offen (Enter wie der abgeblendete Speichern-Knopf).
    pub(crate) fn commit_server_edit(&mut self) {
        if self.server_edit.as_ref().is_none_or(|e| e.address.text().trim().is_empty()) {
            self.play_ui_sound(ui_sound::UiSound::Denied);
            return;
        }
        let Some(edit) = self.server_edit.take() else {
            return;
        };
        let address = edit.address.text().trim().to_string();
        // Leerer Name bleibt leer: die Karte zeigt dann die MOTD (s. build_server_panel).
        let name = edit.name.text().trim().to_string();
        let (view_last, show_name) = (edit.view_last, edit.show_name);
        match edit.index {
            Some(i) if i < self.servers.len() => self.servers[i] = ServerEntry { name, address, view_last, show_name },
            _ => self.servers.push(ServerEntry { name, address, view_last, show_name }),
        }
        self.persist_servers();
    }

    pub(crate) fn persist_servers(&mut self) {
        if let Err(e) = save_servers(&self.servers) {
            self.push_log(format!("Server-Liste konnte nicht gespeichert werden: {e}"));
        }
    }

    // Text ins aktive Feld (Tippen, Einfuegen); ersetzt eine Markierung.
    pub(crate) fn server_edit_insert(&mut self, text: &str) {
        if let Some(feld) = self.active_text_field() {
            feld.insert(text);
        }
    }
}
