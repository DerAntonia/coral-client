// Menues: Bedienelemente, Serverliste, Knoepfe und ihre Aktionen, Farbwaehler, Aufklapplisten.

use crate::*;

impl App {
    // Schieberegler statt der zwei Pfeile eines Steppers (s. StepSlider). `value_rect` ist
    // das Wertfeld, das der Stepper gehabt haette.
    pub(crate) fn step_slider(prev: ButtonAction, next: ButtonAction, value_rect: Rect, value: f64, min: f64, max: f64, enabled: bool) -> Button {
        Button {
            slider: Some(StepSlider { prev, next, value, min, max, value_rect, digits: 3, track_w: SLIDER_TRACK_W }),
            action: next,
            rect: slider_rect_of(value_rect, 3),
            label: String::new(),
            enabled,
            color: SERVER_CARD_MARKER,
            scale: UI_SCALE_SMALL,
        }
    }

    // Derselbe Regler mit Platz fuer vier Ziffern.
    #[cfg(test)]
    pub(crate) fn wide_slider(mut b: Button) -> Button {
        if let Some(s) = b.slider.as_mut() {
            s.digits = 4;
            b.rect = s.rect();
        }
        b
    }

    // Derselbe Regler mit eigener Bahnlaenge.
    pub(crate) fn slider_with_track(mut b: Button, track_w: f32) -> Button {
        if let Some(s) = b.slider.as_mut() {
            s.track_w = track_w;
            b.rect = s.rect();
        }
        b
    }

    // Farbfeld (54x36, schwarzer Rand) am Anfang von `at`; Klick oeffnet das Farbfenster.
    pub(crate) fn swatch(target: ColorTarget, at: Rect, rgba: [f32; 4]) -> Button {
        Button {
            action: ButtonAction::OpenColorPicker(target),
            rect: Rect { w: SWATCH_W, ..at },
            label: String::new(),
            enabled: true,
            color: [rgba[0], rgba[1], rgba[2], 1.0],
            scale: UI_SCALE_SMALL,
            slider: None,
        }
    }

    // Hilfs-Konstruktoren fuer die Bedienelemente im Settings-Tab.
    pub(crate) fn toggle(action: ButtonAction, row: usize, on: bool) -> Button {
        Button {
            action,
            rect: Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: TOGGLE_W, h: SETTINGS_CTRL_H },
            label: if on { "AN" } else { "AUS" }.to_string(),
            enabled: true,
            color: if on { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
            scale: UI_SCALE_SMALL,
            slider: None,
        }
    }

    // TearFree-Schalter: nur bedienbar, wenn der Grafiktreiber die Eigenschaft hat, sonst
    // abgeblendet mit "n/v" (s. latency_tuning.rs -- das haengt am Treiber, nicht am
    // GPU-Hersteller).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn tearfree_button(tearfree_state: Option<bool>, row: usize) -> Button {
        if tearfree_state.is_none() {
            return Button {
                action: ButtonAction::ToggleTearFree,
                rect: Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: TOGGLE_W, h: SETTINGS_CTRL_H },
                label: "n/v".to_string(),
                enabled: false,
                color: theme::get(theme::Slot::Surface1),
                scale: UI_SCALE_SMALL,
                slider: None,
            };
        }
        let label = if tearfree_state == Some(true) { "AN" } else { "AUS" };
        Button {
            action: ButtonAction::ToggleTearFree,
            rect: Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: TOGGLE_W, h: SETTINGS_CTRL_H },
            label: label.to_string(),
            enabled: true,
            color: if tearfree_state == Some(true) { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
            scale: UI_SCALE_SMALL,
            slider: None,
        }
    }

    // Zwei Pfeile links und rechts des Wertfelds. Am Anschlag wird der jeweilige Pfeil
    // abgeblendet -- so sieht man, dass es nicht weitergeht, statt ins Leere zu klicken.
    pub(crate) fn stepper(prev: ButtonAction, next: ButtonAction, row: usize, can_prev: bool, can_next: bool) -> [Button; 2] {
        Self::stepper_at(prev, next, stepper_value_rect(row), can_prev, can_next)
    }

    // Kern von stepper(): zwei Pfeil-Buttons links/rechts eines
    // Werte-Rechtecks, das der Aufrufer schon fertig positioniert hat.
    pub(crate) fn stepper_at(prev: ButtonAction, next: ButtonAction, value: Rect, can_prev: bool, can_next: bool) -> [Button; 2] {
        let y = value.y;
        let arrow = |action, x, label: &str, enabled| Button {
            action,
            rect: Rect { x, y, w: STEPPER_BTN_W, h: SETTINGS_CTRL_H },
            label: label.to_string(),
            enabled,
            color: if enabled { theme::get(theme::Slot::Surface2) } else { theme::get(theme::Slot::Surface1) },
            scale: UI_SCALE_SMALL,
            slider: None,
        };
        [arrow(prev, value.x - STEPPER_BTN_W - STEPPER_GAP, "←", can_prev), arrow(next, value.x + value.w + STEPPER_GAP, "→", can_next)]
    }

    // Ein einfacher Button-Bauer, spart die wiederkehrende Struct-Literal-Fuellung.
    pub(crate) fn nav_button(action: ButtonAction, rect: Rect, label: &str, scale: f32) -> Button {
        Button { action, rect, label: label.to_string(), enabled: true, color: theme::get(theme::Slot::Surface2), scale, slider: None }
    }

    // Zeile 0 im Audio-Tab: "System-Standard" plus alle von cpal gelieferten Geraete -- die
    // Liste kommt vom System, nicht von uns, und kann sich zwischen zwei Starts aendern.
    pub(crate) fn audio_device_options(&self) -> Vec<String> {
        let mut out = vec!["System-Standard".to_string()];
        out.extend(self.audio_devices.iter().cloned());
        out
    }

    pub(crate) fn audio_device_index(&self) -> usize {
        if self.settings.audio_output_device.is_empty() {
            0
        } else {
            self.audio_devices.iter().position(|d| *d == self.settings.audio_output_device).map(|i| i + 1).unwrap_or(0)
        }
    }

    // Wie viele Kacheln der Liste tatsaechlich gezeichnet/anklickbar sind.
    // Gleitende Werte der Oberflaeche einmal je Bild nachziehen; auf der Server-Seite ausserdem
    // veraltete Status-Pings neu anstossen.
    pub(crate) fn tick_ui_anim(&mut self) {
        let jetzt = Instant::now();
        let dt = jetzt.duration_since(self.ui_anim_at).as_secs_f32().min(0.1);
        self.ui_anim_at = jetzt;
        // Alle Listen gleiten gleich (s. smooth_scroll): jede mit ihrem aktuellen Ende.
        let rate = scroll_inertia_rate(self.settings.scroll_inertia);
        if let Some(n) = self.dropdown.as_ref().map(|d| self.choice_options(d.choice).0.len()) {
            if let Some(d) = self.dropdown.as_mut() {
                d.scroll.tick(dt, rate, n.saturating_sub(DROPDOWN_MAX_ITEMS) as f32);
            }
        }
        // Die Suche gilt nur auf dem Connect-Screen; leergeloescht verschwindet das Feld. Jeder
        // neue Suchtext faengt oben an.
        if self.mode != AppMode::Menu || self.screen != MenuScreen::Connect || self.server_search.as_ref().is_some_and(|f| f.text().is_empty()) {
            self.server_search = None;
        }
        let suche = self.server_search.as_ref().map_or("", |f| f.text());
        if suche != self.server_search_applied {
            self.server_search_applied = suche.to_string();
            self.server_scroll.jump(0.0);
        }
        let max = self.server_max_scroll();
        self.server_scroll.tick(dt, rate, max);
        if self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsSkin {
            for (i, c) in [skin_tab::Column::Available, skin_tab::Column::Selected].into_iter().enumerate() {
                let max = self.skin_max_scroll(c);
                self.skin_scroll[i].tick(dt, rate, max);
            }
            if let Some(max) = self.skin_files_max_scroll() {
                if let Some(w) = self.skin_files.as_mut() {
                    w.scroll.tick(dt, rate, max);
                }
            }
        }
        if self.mode == AppMode::Menu && self.screen.settings_page().is_some() {
            let max = self.page_max_scroll() as f32;
            self.page_scroll_mut().tick(dt, rate, max);
        }
        if self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsParticles {
            let max = self.particle_max_scroll();
            self.particle_scroll.tick(dt, rate, max);
        }
        if self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsLightLevels {
            let max = self.light_max_scroll();
            self.light_scroll.tick(dt, rate, max);
        }
        if let Some(max) = self.gpu.as_ref().filter(|_| self.module_menu_shown()).map(|gpu| self.module_max_scroll(gpu) as f32) {
            self.module_menu_scroll.tick(dt, rate, max);
        }
        self.tick_dialog_scroll(dt, rate);
        self.drag_scroll_hold();
        // Der Chat blaettert weiter in ganzen Zeilen (Wachsen, Auswahl und neue Nachrichten
        // haengen daran); gezeichnet wird mit dem gleitenden Stand dahinter.
        self.chat_scroll_glide.ziel = self.chat_scroll as f32;
        self.chat_scroll_glide.tick(dt, rate, f32::INFINITY);
        self.chat_history_glide.ziel = self.chat_history_scroll as f32;
        self.chat_history_glide.tick(dt, rate, f32::INFINITY);
        for rise in [&mut self.chat_rise, &mut self.chat_rise_open] {
            *rise = if rate.is_finite() { *rise * (-dt * rate).exp() } else { 0.0 };
            if *rise < 0.002 {
                *rise = 0.0;
            }
        }
        let ziel = self.chat_bedrock_target.get();
        // Bedrock: neue Nachrichten sind sofort da -- das Ende springt, und was sie den vollen
        // Chat weiterschieben, springt am Anfang mit. Nur das Nachruecken, wenn oben eine
        // Nachricht ausgeblendet ist, gleitet.
        self.chat_bedrock_anim = Some(match self.chat_bedrock_anim {
            Some((s, e)) if rate.is_finite() => {
                let geschoben = (ziel.0 - self.chat_bedrock_last_start).clamp(0.0, (ziel.1 - e).max(0.0));
                let s = s + geschoben;
                let k = 1.0 - (-dt * rate).exp();
                (if (ziel.0 - s).abs() < 0.002 { ziel.0 } else { s + (ziel.0 - s) * k }, ziel.1)
            }
            _ => ziel,
        });
        self.chat_bedrock_last_start = ziel.0;
        if self.mode == AppMode::Menu && self.screen == MenuScreen::Connect {
            self.refresh_server_status();
        }
    }

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
                let _ = tx.send((adresse, ergebnis.ok().and_then(|r| r.ok())));
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
        (self.server_search_hits().len() + usize::from(self.server_search.is_some())).saturating_sub(server_rows_visible()) as f32
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
        let sichtbar = server_rows_visible() as f32;
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
        let mut reihe: Vec<usize> = (0..self.servers.len()).collect();
        reihe.remove(i);
        reihe.insert(ziel, i);
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

    pub(crate) fn screen_buttons(&self) -> Vec<Button> {
        self.sync_settings_scroll();
        let mut out: Vec<Button> = Vec::new();
        // Fuer alles, was mittig steht (Start, Pause, Tod, Einstellungs-Kacheln).
        let screen_w = self.gpu.as_ref().map_or(1280.0, |g| g.size().0);

        match self.screen {
            MenuScreen::Main => {
                if self.conn_status == ConnStatus::Disconnected {
                    // Startbildschirm: Verbinden (oeffnet den Connect-Screen mit der
                    // Adress-Eingabe) + Einstellungen. Die Adresse wird bewusst erst dort
                    // gezeigt, nicht schon hier.
                    out.push(Self::nav_button(
                        ButtonAction::OpenScreen(MenuScreen::Connect),
                        pause_button(0, screen_w),
                        "Verbinden",
                        UI_SCALE_NORMAL,
                    ));
                    out.push(Self::nav_button(
                        ButtonAction::OpenScreen(MenuScreen::Settings),
                        pause_button(1, screen_w),
                        "Einstellungen",
                        UI_SCALE_NORMAL,
                    ));
                } else {
                    // Esc-Pausemenue (verbunden/verbindend): HUD-Editor, Disconnect,
                    // Einstellungen. Reconnect steckt weiterhin im Connect-Pfad, ist hier
                    // aber selten noetig -- ueber Disconnect -> Startbildschirm erreichbar.
                    // Disconnect genau in der Bildschirmmitte: dort erscheint der Zeiger beim
                    // Pausieren (s. on_escape), er liegt also gleich darauf.
                    let screen_h = self.gpu.as_ref().map_or(720.0, |g| g.size().1);
                    // HUD-Editor und Einstellungen gehoeren zusammen, darum beide ueber Disconnect.
                    let zeile = |row: usize| pause_button_around_center(row, 2, screen_w, screen_h);
                    out.push(Self::nav_button(ButtonAction::OpenScreen(MenuScreen::HudEditor), zeile(0), "HUD-Editor", UI_SCALE_NORMAL));
                    out.push(Self::nav_button(ButtonAction::OpenScreen(MenuScreen::Settings), zeile(1), "Einstellungen", UI_SCALE_NORMAL));
                    out.push(Self::nav_button(ButtonAction::Disconnect, zeile(2), "Disconnect", UI_SCALE_NORMAL));
                    out.push(Self::nav_button(ButtonAction::Reconnect, zeile(3), "Reconnect", UI_SCALE_NORMAL));
                    // Links des Servers (Vanillas "Server-Links..."), nur wenn er welche schickt.
                    if !self.server_links.is_empty() {
                        out.push(Self::nav_button(ButtonAction::ServerLinks, zeile(4), "Server-Links", UI_SCALE_NORMAL));
                    }
                }
            }
            MenuScreen::Connect => {
                // Nur die Server-Karten bzw. das Formular; Quick Connect und Bearbeiten
                // liegen in der Leiste unten, das Konto in der Leiste oben.
                self.server_panel_buttons(&mut out);
            }
            MenuScreen::Account => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                if self.credentials.is_some() {
                    // Angemeldet: nur abmelden.
                    out.push(Self::nav_button(ButtonAction::Logout, account_row_rect(0), "Abmelden", UI_SCALE_SMALL));
                } else {
                    // Zeile 0: Microsoft-Login + Refresh-Token. Zeile 1: Token-Feld oeffnen.
                    out.push(Self::nav_button(ButtonAction::MicrosoftLogin, account_row_rect(0), "Mit Microsoft anmelden", UI_SCALE_SMALL));
                    out.push(Self::nav_button(
                        ButtonAction::OpenInputField(InputKind::RefreshToken),
                        account_row_rect_col2(0),
                        "Refresh-Token",
                        UI_SCALE_SMALL,
                    ));
                    out.push(Self::nav_button(
                        ButtonAction::OpenInputField(InputKind::Token),
                        account_row_rect(1),
                        "Token eingeben",
                        UI_SCALE_SMALL,
                    ));
                    // Zeile 2: OK, wenn ein Eingabefeld offen ist.
                    if self.input_field.is_some() {
                        out.push(Self::nav_button(ButtonAction::SubmitInputField, ACCOUNT_INPUT_OK, "OK", UI_SCALE_SMALL));
                    }
                }
            }
            // Kategorie-Auswahl: nur Kacheln, keine einzelnen Einstellungen (s.
            // MenuScreen-Kommentar) -- ButtonAction::OpenScreen(_) reicht, kein eigener
            // ButtonAction-Fall noetig.
            MenuScreen::SettingsSetup => {}
            MenuScreen::Settings => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                // Alphabetisch -- neue Kategorien sortieren sich von selbst ein.
                let mut kategorien = SETTINGS_CATEGORIES;
                kategorien.sort_by_key(|&(_, label)| i18n::tr(label).to_lowercase());
                for (i, (screen, label)) in kategorien.into_iter().enumerate() {
                    out.push(Self::nav_button(
                        ButtonAction::OpenScreen(screen),
                        settings_hub_button(i, kategorien.len(), screen_w),
                        label,
                        UI_SCALE_NORMAL,
                    ));
                }
            }
            MenuScreen::SettingsGeneral => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let set = &self.settings;
                out.push(Self::toggle(ButtonAction::ToggleTearing, TEARING_TEST_ROW, set.show_tearing_test));
                // Die FPS-Anzeige schaltet man im HUD-Editor (Rechtsklick auf sie).
                // Render-Ahead: feste Grenzen, s. FRAME_LATENCY_MIN/MAX.
                out.extend(Self::stepper(
                    ButtonAction::FrameLatencyDown,
                    ButtonAction::FrameLatencyUp,
                    FRAME_LATENCY_ROW,
                    set.frame_latency > FRAME_LATENCY_MIN,
                    set.frame_latency < FRAME_LATENCY_MAX,
                ));
                // Bei selbsttaktenden Modi hat der Deckel keine Wirkung -- der VBlank
                // bestimmt die Rate. Also abblenden statt so tun, als wuerde er etwas tun.
                let fps_limit_wirksam = !self.gpu.as_ref().is_some_and(|g| g.present_mode_self_paces());
                out.extend(Self::stepper(
                    ButtonAction::FpsLimitPrev,
                    ButtonAction::FpsLimitNext,
                    FPS_LIMIT_ROW,
                    fps_limit_wirksam && fps_limit_step(set.fps_limit, false).is_some(),
                    fps_limit_wirksam && fps_limit_step(set.fps_limit, true).is_some(),
                ));
                // Maus-Empfindlichkeit ist jetzt auf dem Steuerung-Screen (echte Steuerung,
                // keine Performance-Einstellung), s. MenuScreen::SettingsControls.
                out.extend(Self::stepper(
                    ButtonAction::MeshBudgetDown,
                    ButtonAction::MeshBudgetUp,
                    MESH_BUDGET_ROW,
                    set.mesh_columns_per_frame > 1,
                    set.mesh_columns_per_frame < MESH_COLUMNS_MAX,
                ));
                out.push(Self::step_slider(ButtonAction::ChunkUpdateThreadsDown, ButtonAction::ChunkUpdateThreadsUp, stepper_value_rect(CHUNK_THREADS_ROW), set.chunk_update_threads as f64, CHUNK_UPDATE_THREADS_MIN as f64, CHUNK_UPDATE_THREADS_MAX as f64, true));
                // Die GPU-Zeile ist reine Information ohne Bedienelement, s.
                // build_settings_general_tab.
                // TearFree ist eine X11-Eigenschaft des AMD-Treibers; anderswo steht statt des
                // Schalters ein Hinweis (s. build_settings_general_tab).
                #[cfg(target_os = "linux")]
                out.push(Self::tearfree_button(self.tearfree_state, TEARFREE_ROW));
                out.push(Self::toggle(ButtonAction::ToggleCaveCulling, CAVE_CULLING_ROW, set.cave_culling));
                out.push(Self::toggle(ButtonAction::ToggleMipmapping, MIPMAPPING_ROW, set.mipmapping));
                // Renderpfad samt Shadereinstellungen vorerst ausgeblendet (der Shader-Pfad ruht).
                // out.push(Self::nav_button(
                //     ButtonAction::CycleRenderPath,
                //     Rect { x: SETTINGS_CTRL_X, y: settings_row_y(RENDER_PATH_ROW), w: 220.0, h: SETTINGS_CTRL_H },
                //     RENDER_PATH_NAMES[set.render_path as usize],
                //     UI_SCALE_SMALL,
                // ));
                // out.push(Self::nav_button(
                //     ButtonAction::OpenShaderSettings,
                //     Rect { x: SETTINGS_CTRL_X + 230.0, y: settings_row_y(RENDER_PATH_ROW), w: 220.0, h: SETTINGS_CTRL_H },
                //     "Shadereinstellungen",
                //     UI_SCALE_SMALL,
                // ));
                out.push(Self::toggle(ButtonAction::CycleTearPosition, TEAR_POSITION_ROW, set.tear_position != 0));
                out.push(Self::step_slider(ButtonAction::TearTimingDown, ButtonAction::TearTimingUp, stepper_value_rect(TEAR_TIMING_ROW), set.tear_timing_us as f64, TEAR_TIMING_MIN_US as f64, TEAR_TIMING_MAX_US as f64, true));
                out.push(Self::step_slider(ButtonAction::TearReserveDown, ButtonAction::TearReserveUp, stepper_value_rect(TEAR_RESERVE_ROW), set.tear_reserve_us as f64, TEAR_RESERVE_MIN_US as f64, TEAR_RESERVE_MAX_US as f64, true));
                // CMAA2-Umschalter deaktiviert, s. CMAA2_ROW-Kommentar.
                // out.push(Self::toggle(ButtonAction::ToggleCmaa2, CMAA2_ROW, set.cmaa2_enabled));
            }
            MenuScreen::SettingsVideo => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let set = &self.settings;
                // Zeilen 0..2: je ein FOV-Stepper (< 70 >) pro Perspektive, Zeile 3: der
                // Projektions-Umschalter (ein Button, der durch PROJECTION_NAMES zyklt).
                // Alle echt, Platzhalter erst ab Zeile 4.
                for p in Perspective::ALL {
                    let fov = set.fov_of(p);
                    out.extend(Self::stepper(
                        ButtonAction::FovDown(p),
                        ButtonAction::FovUp(p),
                        p.fov_row(),
                        fov > FOV_MIN + f32::EPSILON,
                        fov < FOV_MAX - f32::EPSILON,
                    ));
                    // Kameraabstand und -hoehe -- nur die 3.-Person-Ansichten haben beides,
                    // und beides sitzt in der rechten Spalte (s. Perspective::distance_row).
                    if let Some(row) = p.distance_row() {
                        let d = set.camera_distance_of(p);
                        out.extend(Self::stepper_at(
                            ButtonAction::CameraDistanceDown(p),
                            ButtonAction::CameraDistanceUp(p),
                            stepper_value_rect(row),
                            d > CAMERA_DISTANCE_MIN + f32::EPSILON,
                            d < CAMERA_DISTANCE_MAX - f32::EPSILON,
                        ));
                    }
                    // Ueberspringen: hoechstens eine der beiden 3.-Person-Ansichten.
                    if p != Perspective::FirstPerson {
                        let an = set.perspective_skipped(p);
                        let andere = Perspective::THIRD_PERSON.into_iter().any(|q| q != p && set.perspective_skipped(q));
                        out.push(Button {
                            action: ButtonAction::SkipPerspective(p),
                            rect: skip_perspective_rect(p.fov_row()),
                            label: "Überspringen".to_string(),
                            enabled: an || !andere,
                            color: if an { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                            scale: UI_SCALE_SMALL,
                            slider: None,
                        });
                    }
                    if let Some(row) = p.y_offset_row() {
                        let y = set.camera_y_offset_of(p);
                        out.extend(Self::stepper_at(
                            ButtonAction::CameraYOffsetDown(p),
                            ButtonAction::CameraYOffsetUp(p),
                            stepper_value_rect(row),
                            y > CAMERA_Y_OFFSET_MIN + f32::EPSILON,
                            y < CAMERA_Y_OFFSET_MAX - f32::EPSILON,
                        ));
                    }
                }
                out.push(Self::step_slider(ButtonAction::SmoothLightingDown, ButtonAction::SmoothLightingUp, stepper_value_rect(SMOOTH_LIGHTING_ROW), set.smooth_lighting_percent as f64, 0.0, SMOOTH_LIGHTING_MAX as f64, true));
                out.push(Self::nav_button(
                    ButtonAction::CycleParticles,
                    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(PARTICLES_ROW), w: 220.0, h: SETTINGS_CTRL_H },
                    PARTICLE_MODE_NAMES[set.particles_mode as usize],
                    UI_SCALE_SMALL,
                ));
                // Hervorgehoben, sobald eine Art von Vanilla abweicht.
                let angepasst = set.particle_tuning.iter().any(|t| *t != particles::ParticleTuning::default());
                out.push(Button {
                    action: ButtonAction::OpenParticleSettings,
                    rect: Rect { x: SETTINGS_CTRL_X + 230.0, y: settings_row_y(PARTICLES_ROW), w: 160.0, h: SETTINGS_CTRL_H },
                    label: "Anpassen".to_string(),
                    enabled: true,
                    color: if angepasst { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                    scale: UI_SCALE_SMALL,
                    slider: None,
                });
                out.push(Self::nav_button(
                    ButtonAction::CycleProjection,
                    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(PROJECTION_ROW), w: 220.0, h: SETTINGS_CTRL_H },
                    PROJECTION_NAMES[set.projection_mode as usize],
                    UI_SCALE_SMALL,
                ));
                // Schadens-Faerbung: ein Farbfeld, die Regler im Farbfenster.
                out.push(Self::swatch(ColorTarget::DamageTint, settings_ctrl_rect(DAMAGE_TINT_FIRST_ROW), set.damage_tint_rgba()));
                out.push(Self::toggle(ButtonAction::ToggleDamageTintSkinOnly, DAMAGE_TINT_SKIN_ONLY_ROW, set.damage_tint_skin_only));
                out.push(Self::step_slider(ButtonAction::DamageTintTicksPrev, ButtonAction::DamageTintTicksNext, stepper_value_rect(DAMAGE_TINT_TICKS_ROW), set.damage_tint_ticks as f64, DAMAGE_TINT_TICKS_MIN as f64, DAMAGE_TINT_TICKS_MAX as f64, true));
                let speed = set.glint_speed;
                out.extend(Self::stepper(ButtonAction::GlintSpeedPrev, ButtonAction::GlintSpeedNext, GLINT_SPEED_ROW, speed > 0, speed < 100));
                let strength = set.glint_strength;
                out.extend(Self::stepper(
                    ButtonAction::GlintStrengthPrev,
                    ButtonAction::GlintStrengthNext,
                    GLINT_STRENGTH_ROW,
                    strength > 0,
                    strength < 100,
                ));
                let liste = set.player_list_scale;
                out.extend(Self::stepper(
                    ButtonAction::PlayerListScalePrev,
                    ButtonAction::PlayerListScaleNext,
                    PLAYER_LIST_SCALE_ROW,
                    liste > HUD_SCALE_PCT_MIN,
                    liste < HUD_SCALE_PCT_MAX,
                ));
                out.extend(Self::stepper(
                    ButtonAction::EntityDistancePrev,
                    ButtonAction::EntityDistanceNext,
                    ENTITY_DISTANCE_ROW,
                    set.entity_render_distance > ENTITY_DISTANCE_MIN,
                    set.entity_render_distance < ENTITY_DISTANCE_MAX,
                ));
                out.extend(Self::stepper(
                    ButtonAction::BlockEntityDistancePrev,
                    ButtonAction::BlockEntityDistanceNext,
                    BLOCK_ENTITY_DISTANCE_ROW,
                    set.block_entity_render_distance > BLOCK_ENTITY_DISTANCE_MIN,
                    set.block_entity_render_distance < BLOCK_ENTITY_DISTANCE_MAX,
                ));
                out.push(Self::toggle(ButtonAction::ToggleBlockRngRotation, BLOCK_ROTATION_ROW, set.block_rng_rotation));
                out.push(Self::toggle(ButtonAction::ToggleArmorTrims, ARMOR_TRIMS_ROW, set.armor_trims));
                out.extend(Self::stepper(
                    ButtonAction::MaxRenderDistanceDown,
                    ButtonAction::MaxRenderDistanceUp,
                    RENDER_DISTANCE_ROW,
                    set.max_render_distance > MAX_RENDER_DISTANCE_MIN,
                    set.max_render_distance < MAX_RENDER_DISTANCE_MAX,
                ));
                out.push(Self::step_slider(ButtonAction::ChunkSaverThresholdPrev, ButtonAction::ChunkSaverThresholdNext, stepper_value_rect(LOD_DETAIL_ROW), set.chunk_saver_threshold as f64, lod::THRESHOLD_MIN as f64, lod::THRESHOLD_MAX as f64, true));
                // Crit-Farbe: Schalter, rechts daneben das Farbfeld (dieselben vier Achsen wie
                // die Schadens-Faerbung).
                out.push(Self::toggle(ButtonAction::ToggleHurtCam, HURT_CAM_ROW, set.hurt_cam));
                out.push(Self::step_slider(
                    ButtonAction::HurtCamStrengthPrev,
                    ButtonAction::HurtCamStrengthNext,
                    stepper_value_rect(HURT_CAM_STRENGTH_ROW),
                    set.hurt_cam_strength as f64,
                    0.0,
                    100.0,
                    set.hurt_cam,
                ));
                out.push(Self::nav_button(
                    ButtonAction::CycleDeathAnimation,
                    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(DEATH_ANIM_ROW), w: 220.0, h: SETTINGS_CTRL_H },
                    DEATH_ANIM_NAMES[set.death_animation as usize],
                    UI_SCALE_SMALL,
                ));
                let modi: Vec<_> = FIRE_MODE_NAMES.iter().enumerate().map(|(i, &name)| (name, ButtonAction::FireMode(i as u8), set.fire_mode == i as u8)).collect();
                out.extend(pick_buttons(settings_wide_rect(FIRE_MODE_ROW), &modi, true));
                if set.fire_mode == FIRE_MODE_CROSSHAIR {
                    out.push(Self::swatch(ColorTarget::FireCrosshair, settings_ctrl_rect(FIRE_DETAIL_ROW), set.fire_crosshair_rgba()));
                } else if set.fire_mode == FIRE_MODE_VANILLA {
                    out.push(Self::step_slider(
                        ButtonAction::FireHeightPrev,
                        ButtonAction::FireHeightNext,
                        stepper_value_rect(FIRE_DETAIL_ROW),
                        set.fire_height as f64,
                        0.0,
                        100.0,
                        true,
                    ));
                }
                out.push(Self::toggle(ButtonAction::ToggleModuleCritColor, CRIT_COLOR_ROW, set.module_crit_color_enabled));
                if set.module_crit_color_enabled {
                    out.push(Self::swatch(ColorTarget::Crit, swatch_after_toggle_rect(CRIT_COLOR_ROW), set.crit_color_rgba()));
                }
            }
            MenuScreen::SettingsSensCalc => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let wert = stepper_value_rect(SENS_CALC_RESULT_ROW);
                out.push(Self::nav_button(
                    ButtonAction::ApplySensCalc,
                    Rect { x: wert.x + wert.w + STEPPER_GAP, y: wert.y, w: 160.0, h: SETTINGS_CTRL_H },
                    "Übernehmen",
                    UI_SCALE_SMALL,
                ));
            }
            // Die Modul-Knoepfe selbst kommen aus module_menu_buttons (s. handle_click).
            MenuScreen::SettingsModules => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
            }
            MenuScreen::SettingsLanguage => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                for row in 0..LANGUAGE_ROWS.len() {
                    // Die Sprachnamen stehen in ihrer eigenen Sprache (s. Language::label);
                    // "Alles" zeigt die gemeinsame Sprache, sonst einen Strich.
                    let label = self.language_of_row(row).map_or("--", i18n::Language::label);
                    out.push(Self::nav_button(
                        ButtonAction::CycleLanguage(row),
                        Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: 220.0, h: SETTINGS_CTRL_H },
                        label,
                        UI_SCALE_SMALL,
                    ));
                }
                out.push(Self::toggle(ButtonAction::ToggleLatinMacrons, LATIN_MACRONS_ROW, self.settings.latin_macrons));
                out.push(Self::toggle(ButtonAction::ToggleLoanwords, LOANWORDS_ROW, self.settings.english_loanwords));
            }
            MenuScreen::SettingsDesign => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let modus = wallpaper::WallpaperMode::from_index(self.settings.wallpaper);
                out.push(Self::nav_button(
                    ButtonAction::CycleWallpaper,
                    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(WALLPAPER_ROW), w: 220.0, h: SETTINGS_CTRL_H },
                    modus.label(),
                    UI_SCALE_SMALL,
                ));
                if modus == wallpaper::WallpaperMode::File {
                    let text = if self.wallpaper_loader.picking() { "Dialog offen..." } else { "Bild wählen..." };
                    out.push(Self::nav_button(
                        ButtonAction::PickWallpaper,
                        Rect { x: WALLPAPER_PICK_X, y: settings_row_y(WALLPAPER_ROW), w: 220.0, h: SETTINGS_CTRL_H },
                        text,
                        UI_SCALE_SMALL,
                    ));
                }
                let abdunkeln = self.settings.menu_backdrop_opacity;
                out.extend(Self::stepper(ButtonAction::MenuBackdropPrev, ButtonAction::MenuBackdropNext, MENU_BACKDROP_ROW, abdunkeln > 0, abdunkeln < 100));
                let traegheit = self.settings.scroll_inertia;
                out.extend(Self::stepper(ButtonAction::ScrollInertiaPrev, ButtonAction::ScrollInertiaNext, SCROLL_INERTIA_ROW, traegheit > 0, traegheit < 100));
                let weite = self.settings.scroll_distance;
                out.extend(Self::stepper(ButtonAction::ScrollDistancePrev, ButtonAction::ScrollDistanceNext, SCROLL_DISTANCE_ROW, weite > SCROLL_DISTANCE_MIN, weite < SCROLL_DISTANCE_MAX));
                // Kacheln: zeichnen sich selbst (build_settings_design_tab), hier nur die
                // Trefferflaechen.
                let eigene = self.settings.custom_themes.len();
                let aktionen = (0..theme::PRESETS.len())
                    .map(ButtonAction::ApplyThemePreset)
                    .chain((0..eigene).map(ButtonAction::SelectCustomTheme))
                    .chain([ButtonAction::NewCustomTheme]);
                for (i, action) in aktionen.enumerate() {
                    out.push(Button { action, rect: theme_tile_rect(i), label: String::new(), enabled: true, color: SERVER_CARD_MARKER, scale: UI_SCALE_SMALL, slider: None });
                }
                // Farbfelder nur bei eigenen Themes -- ein Preset ist fest (seine Farben zeichnet
                // build_settings_design_tab ohne Knopf).
                if matches!(self.settings.theme_choice, ThemeChoice::Custom(_)) {
                    for (i, slot) in theme::EDITABLE.into_iter().enumerate() {
                        let (row, x) = theme_slot_cell(i);
                        out.push(Self::swatch(ColorTarget::Theme(slot), Rect { x: x + THEME_COLOR_SWATCH_DX, y: settings_row_y(row), w: SWATCH_W, h: SETTINGS_CTRL_H }, theme::get(slot)));
                    }
                }
            }
            MenuScreen::SettingsFont => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                // Schrift und Schatten sind Kaestchen zum Hineinziehen (s. FontSlot).
                let mut stabil = Self::toggle(ButtonAction::ToggleStableNumbers, STABLE_NUMBERS_ROW, self.settings.stable_numbers);
                stabil.rect.x = FONT_CTRL_X;
                out.push(stabil);
            }
            MenuScreen::SettingsShader => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let p = self.settings.shader_scattering_percent;
                out.push(Self::step_slider(ButtonAction::ScatteringPrev, ButtonAction::ScatteringNext, stepper_value_rect(0), p as f64, 0.0, SCATTERING_MAX as f64, true));
                let g = self.settings.shader_sun_size_tenths;
                out.push(Self::step_slider(ButtonAction::SunSizePrev, ButtonAction::SunSizeNext, stepper_value_rect(1), g as f64, 0.0, SUN_SIZE_MAX_TENTHS as f64, true));
                out.push(Self::toggle(ButtonAction::ToggleBlockShadows, 2, self.settings.shader_block_shadows));
                let b = self.settings.shader_block_softness_deg;
                let an = self.settings.shader_block_shadows;
                out.push(Self::step_slider(ButtonAction::BlockSoftnessPrev, ButtonAction::BlockSoftnessNext, stepper_value_rect(3), b as f64, 0.0, BLOCK_SOFTNESS_MAX_DEG as f64, an));
                out.push(Self::nav_button(
                    ButtonAction::OpenLightLevels,
                    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(4), w: STEPPER_VALUE_W + 2.0 * (STEPPER_BTN_W + STEPPER_GAP), h: SETTINGS_CTRL_H },
                    "Bearbeiten",
                    UI_SCALE_SMALL,
                ));
                let d = self.settings.shader_lightmap_debug;
                out.extend(Self::stepper(ButtonAction::LightmapDebugPrev, ButtonAction::LightmapDebugNext, 5, d > 0, (d as usize) + 1 < LIGHTMAP_DEBUG_NAMES.len()));
            }
            MenuScreen::SettingsParticles => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let schalter = |action: ButtonAction, x: f32, y: f32, an: bool| Button {
                    action,
                    rect: Rect { x, y, w: PARTICLE_TOGGLE_W, h: SETTINGS_CTRL_H },
                    label: if an { "AN" } else { "AUS" }.to_string(),
                    enabled: true,
                    color: if an { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                    scale: UI_SCALE_SMALL,
                    slider: None,
                };
                for (i, y) in header_list_rows(particles::KINDS.len(), self.particle_scroll.stand, self.particle_visible_rows()) {
                    let t = self.settings.particle_tuning[i];
                    let art = particles::KINDS[i].spawn;
                    out.push(schalter(ButtonAction::ParticleToggle(i), PARTICLE_TOGGLE_X, y, t.enabled));
                    let mut regler = vec![
                        (t.lifetime_pct, PARTICLE_LIFETIME_STEPS[0], *PARTICLE_LIFETIME_STEPS.last().unwrap()),
                        (t.opacity_pct, PARTICLE_OPACITY_STEP, 100),
                        (t.speed_pct, PARTICLE_SPEED_STEPS[0], *PARTICLE_SPEED_STEPS.last().unwrap()),
                    ];
                    if art.has_amount() {
                        regler.push((t.amount_pct, PARTICLE_AMOUNT_STEPS[0], *PARTICLE_AMOUNT_STEPS.last().unwrap()));
                    } else if art.has_interval() {
                        regler.push((t.interval_pct, PARTICLE_INTERVAL_STEPS[0], *PARTICLE_INTERVAL_STEPS.last().unwrap()));
                    }
                    for (col, (wert, min, max)) in regler.into_iter().enumerate() {
                        let b = Self::step_slider(
                            ButtonAction::ParticleStep(i, col as u8, false),
                            ButtonAction::ParticleStep(i, col as u8, true),
                            Rect { y, ..particle_value_rect(col, 0) },
                            wert as f64,
                            min as f64,
                            max as f64,
                            true,
                        );
                        out.push(Self::slider_with_track(b, PARTICLE_TRACK_W));
                    }
                    if art.has_all_at_once() {
                        out.push(schalter(ButtonAction::ParticleAllAtOnce(i), particle_column_x(PARTICLE_ALL_AT_ONCE_COL), y, t.all_at_once));
                    }
                }
            }
            MenuScreen::SettingsLightLevels => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let treffer = self.light_search_hits();
                for (k, y) in header_list_rows(treffer.len(), self.light_scroll.stand, self.light_visible_rows()) {
                    let i = treffer[k];
                    let p = self.settings.light_factor_of(light_source_blocks()[i]);
                    out.push(Self::step_slider(ButtonAction::LightFactorPrev(i), ButtonAction::LightFactorNext(i), Rect { y, ..stepper_value_rect(0) }, p as f64, 0.0, LIGHT_FACTOR_MAX as f64, true));
                }
            }
            MenuScreen::SettingsInventory => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let value = self.settings.inventory_transparency;
                out.extend(Self::stepper(
                    ButtonAction::InventoryTransparencyPrev,
                    ButtonAction::InventoryTransparencyNext,
                    0,
                    value > 0,
                    value < 100,
                ));
                let scale = self.settings.inventory_scale;
                out.extend(Self::stepper(ButtonAction::InventoryScalePrev, ButtonAction::InventoryScaleNext, 1, scale > 0, scale < self.max_gui_scale_setting()));
                out.push(Self::toggle(ButtonAction::ToggleInventoryCursorToSlot, 2, self.settings.inventory_cursor_to_slot));
                out.push(Self::toggle(ButtonAction::ToggleCreativeOpTab, 3, self.settings.creative_op_tab));
                out.push(Self::toggle(ButtonAction::ToggleInventoryMergeHover, 4, self.settings.inventory_merge_hover));
                out.push(Self::toggle(ButtonAction::TogglePotionMarkStrong, 5, self.settings.potion_mark_strong));
                out.push(Self::toggle(ButtonAction::TogglePotionMarkLong, 6, self.settings.potion_mark_long));
                out.push(Self::toggle(ButtonAction::TogglePotionMarkWeak, 7, self.settings.potion_mark_weak));
            }
            MenuScreen::SettingsMisc => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let modi: Vec<_> = NO_UI_CLOSE_NAMES
                    .iter()
                    .map(|&(name, wert)| (name, ButtonAction::NoUiClose(wert), self.settings.no_ui_close == wert))
                    .collect();
                out.extend(pick_buttons(settings_wide_rect(0), &modi, true));
                out.push(Self::toggle(ButtonAction::ToggleNoChatClear, 1, self.settings.no_chat_clear));
                out.push(Self::toggle(ButtonAction::ToggleSignEditInWorld, 2, self.settings.sign_edit_in_world));
                for (row, wert, aktion) in [
                    (3, self.settings.no_slot_set, ButtonAction::NoSlotSet as fn(u8) -> ButtonAction),
                    (4, self.settings.no_rotate_set, ButtonAction::NoRotateSet),
                ] {
                    let modi: Vec<_> = SERVER_SET_NAMES.iter().map(|&(name, m)| (name, aktion(m), wert == m)).collect();
                    out.extend(pick_buttons(settings_wide_rect(row), &modi, true));
                }
            }
            MenuScreen::SettingsAudio => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let geraet = self.audio_device_options().get(self.audio_device_index()).cloned().unwrap_or_else(|| "?".to_string());
                out.push(Self::nav_button(ButtonAction::OpenChoice(Choice::AudioDevice), settings_wide_rect(AUDIO_DEVICE_ROW), &geraet, UI_SCALE_SMALL));
                let puffer = audio_buffer_label(self.settings.audio_buffer_frames);
                out.push(Self::nav_button(ButtonAction::OpenChoice(Choice::AudioBuffer), settings_wide_rect(AUDIO_BUFFER_ROW), &puffer, UI_SCALE_SMALL));
                if let PipewireKnopf::Knopf(text, ziel) = self.pipewire_knopf() {
                    out.push(Self::nav_button(ButtonAction::PipewireLimit(ziel), settings_wide_rect(AUDIO_PIPEWIRE_SET_ROW), &text, UI_SCALE_SMALL));
                }
                out.push(Self::toggle(ButtonAction::ToggleAudioAutoOptimize, AUDIO_AUTO_ROW, self.settings.audio_auto_optimize));
                out.push(Self::nav_button(ButtonAction::AudioTestTone, stepper_value_rect(AUDIO_TEST_TONE_ROW), "Testton abspielen", UI_SCALE_SMALL));
                // Ohne Grenze -- beide Pfeile bleiben immer anklickbar, s. Kommentar an
                // Settings::ui_pitch_semitones.
                out.push(Self::step_slider(ButtonAction::UiPitchDown, ButtonAction::UiPitchUp, stepper_value_rect(AUDIO_PITCH_ROW), self.settings.ui_pitch_semitones as f64, -24.0, 24.0, true));
                // Waehrend die Runde laeuft abgeblendet: ein zweiter Start mittendrin wuerde
                // die Runde von vorn beginnen und die gehoerte Richtung zerreissen.
                let laeuft = self.orbit_test.is_some();
                out.extend(Self::stepper(
                    ButtonAction::MasterVolumeDown,
                    ButtonAction::MasterVolumeUp,
                    AUDIO_MASTER_ROW,
                    self.settings.master_volume > 0.0,
                    self.settings.master_volume < 100.0,
                ));
                for (i, &v) in self.settings.category_volume.iter().enumerate() {
                    out.extend(Self::stepper(ButtonAction::CategoryVolumeDown(i), ButtonAction::CategoryVolumeUp(i), AUDIO_CATEGORY_FIRST_ROW + i, v > 0.0, v < 100.0));
                }
                for (i, &v) in self.settings.ui_sound_volume.iter().enumerate() {
                    out.extend(Self::stepper(ButtonAction::UiSoundVolumeDown(i), ButtonAction::UiSoundVolumeUp(i), AUDIO_UI_SOUND_FIRST_ROW + i, v > 0.0, v < 100.0));
                }
                out.push(Button {
                    action: ButtonAction::AudioSpatialTest,
                    rect: stepper_value_rect(AUDIO_SPATIAL_ROW),
                    label: if laeuft { "laeuft ...".to_string() } else { "Runde starten".to_string() },
                    enabled: !laeuft,
                    color: if laeuft { theme::get(theme::Slot::Surface1) } else { theme::get(theme::Slot::Surface2) },
                    scale: UI_SCALE_SMALL,
                    slider: None,
                });
            }
            MenuScreen::SettingsConfigs => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let configs = assets::list_configs();
                let offen = |edit| matches!(self.input_field, Some((InputKind::ConfigName(e), _)) if e == edit);
                for (i, (edit, label)) in [(ConfigEdit::New, "Neu"), (ConfigEdit::Duplicate, "Duplizieren"), (ConfigEdit::Rename, "Umbenennen")]
                    .into_iter()
                    .enumerate()
                {
                    out.push(Button {
                        action: ButtonAction::EditConfig(edit),
                        rect: config_button_rect(i),
                        label: label.to_string(),
                        enabled: true,
                        color: if offen(edit) { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                        scale: UI_SCALE_SMALL,
                        slider: None,
                    });
                }
                out.push(Self::nav_button(ButtonAction::OpenConfigFolder, config_button_rect(4), "Ordner öffnen", UI_SCALE_SMALL));
                out.push(Button {
                    action: ButtonAction::DeleteConfig,
                    rect: config_button_rect(3),
                    label: if self.config_delete_armed { "Wirklich?" } else { "Löschen" }.to_string(),
                    // Die letzte Config bleibt (s. assets::delete_config).
                    enabled: configs.len() > 1,
                    color: if self.config_delete_armed { [0.55, 0.18, 0.18, 1.0] } else { theme::get(theme::Slot::Surface2) },
                    scale: UI_SCALE_SMALL,
                    slider: None,
                });
                if matches!(self.input_field, Some((InputKind::ConfigName(_), _))) {
                    let feld = config_name_field_rect();
                    out.push(Self::nav_button(
                        ButtonAction::SubmitInputField,
                        Rect { x: feld.x + feld.w + CONFIG_BUTTON_GAP, y: feld.y, w: CONFIG_BUTTON_W, h: feld.h },
                        "OK",
                        UI_SCALE_SMALL,
                    ));
                }
                let aktiv = active_config();
                for (i, name) in configs.iter().take(MAX_VISIBLE_CONFIGS).enumerate() {
                    let ist_aktiv = *name == aktiv;
                    out.push(Button {
                        action: ButtonAction::SelectConfig(i),
                        rect: config_row_rect(i),
                        label: name.clone(),
                        enabled: true,
                        color: if ist_aktiv { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                        scale: UI_SCALE_SMALL,
                        slider: None,
                    });
                }
            }
            // Skin-Tab: die Kacheln zeichnen und behandeln sich selbst (s. skin_tile_press),
            // die Knoepfe im Rand rechts stehen bei chrome_buttons.
            MenuScreen::SettingsSkin => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
            }
            MenuScreen::SettingsControls => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                // Fenster "Erweitert" offen: nur seine Knoepfe (s. keybind_advanced_buttons).
                if let Some(aktion) = self.keybind_advanced {
                    let (dx, dy) = self.content_offset();
                    out.extend(self.keybind_advanced_buttons(aktion).into_iter().map(|mut b| {
                        b.rect.x -= dx;
                        b.rect.y -= dy;
                        b
                    }));
                    return out;
                }
                let set = &self.settings;
                for (row, &zeile) in controls_rows().iter().enumerate() {
                    match zeile {
                        // Beide Kamera-Regler stehen immer da; ohne "X/Y getrennt" ziehen sie
                        // einander mit (s. Settings::couple_sensitivity).
                        ControlsRow::SensX => out.extend(Self::stepper(
                            ButtonAction::SensitivityDown,
                            ButtonAction::SensitivityUp,
                            row,
                            set.mouse_sensitivity > SENSITIVITY_MIN + f32::EPSILON,
                            set.mouse_sensitivity < SENSITIVITY_MAX - f32::EPSILON,
                        )),
                        ControlsRow::SensY => out.extend(Self::stepper(
                            ButtonAction::SensitivityYDown,
                            ButtonAction::SensitivityYUp,
                            row,
                            set.mouse_sensitivity_y > SENSITIVITY_MIN + f32::EPSILON,
                            set.mouse_sensitivity_y < SENSITIVITY_MAX - f32::EPSILON,
                        )),
                        ControlsRow::SensSplit => out.push(Self::toggle(ButtonAction::ToggleSensSplit, row, set.mouse_sens_split)),
                        ControlsRow::SensCalc => out.push(Self::nav_button(ButtonAction::OpenSensCalc, settings_ctrl_rect(row), "Öffnen", UI_SCALE_SMALL)),
                        ControlsRow::CursorSens => out.extend(Self::stepper(
                            ButtonAction::CursorSensitivityDown,
                            ButtonAction::CursorSensitivityUp,
                            row,
                            set.cursor_sensitivity > CURSOR_SENSITIVITY_MIN + f32::EPSILON,
                            set.cursor_sensitivity < CURSOR_SENSITIVITY_MAX - f32::EPSILON,
                        )),
                        ControlsRow::CursorSize => out.push(Self::step_slider(
                            ButtonAction::CursorSizeDown,
                            ButtonAction::CursorSizeUp,
                            stepper_value_rect(row),
                            set.cursor_size as f64,
                            CURSOR_SIZE_MIN as f64,
                            CURSOR_SIZE_MAX as f64,
                            true,
                        )),
                        ControlsRow::HoldConfirm => out.push(Self::step_slider(
                            ButtonAction::HoldConfirmDown,
                            ButtonAction::HoldConfirmUp,
                            stepper_value_rect(row),
                            set.hold_confirm_ms as f64,
                            0.0,
                            HOLD_CONFIRM_MAX as f64,
                            true,
                        )),
                        ControlsRow::ReconnectHold => out.push(Self::step_slider(
                            ButtonAction::ReconnectHoldDown,
                            ButtonAction::ReconnectHoldUp,
                            stepper_value_rect(row),
                            set.reconnect_hold_ms as f64,
                            RECONNECT_HOLD_GENERAL as f64,
                            HOLD_CONFIRM_MAX as f64,
                            true,
                        )),
                        ControlsRow::CursorWidth => out.push(Self::step_slider(
                            ButtonAction::CursorWidthDown,
                            ButtonAction::CursorWidthUp,
                            stepper_value_rect(row),
                            set.cursor_width as f64,
                            CURSOR_WIDTH_MIN as f64,
                            CURSOR_WIDTH_MAX as f64,
                            true,
                        )),
                        ControlsRow::CursorCircle => out.push(Self::toggle(ButtonAction::ToggleCursorCircle, row, set.cursor_circle)),
                        ControlsRow::SneakProtection => out.push(Self::toggle(ButtonAction::ToggleSneakEdgeProtection, row, set.sneak_edge_protection_enabled)),
                        ControlsRow::SnapTap => out.push(Self::toggle(ButtonAction::ToggleSnapTap, row, set.snap_tap_enabled)),
                        ControlsRow::JumpCooldown => out.push(Self::toggle(ButtonAction::ToggleJumpCooldown, row, set.jump_cooldown_enabled)),
                        ControlsRow::SprintOnStart => out.push(Self::toggle(ButtonAction::ToggleSprintOnStart, row, set.sprint_on_start)),
                        ControlsRow::HoldUseRepeat => {
                            out.push(Self::nav_button(ButtonAction::CycleHoldUseRepeat, settings_wide_rect(row), set.hold_use_repeat.label(), UI_SCALE_SMALL))
                        }
                        ControlsRow::RocketRepeat => out.push(Self::toggle(ButtonAction::ToggleRocketRepeat, row, set.hold_use_repeat_rockets)),
                        ControlsRow::ExtinguishDcGhost => out.push(Self::toggle(ButtonAction::ToggleExtinguishDcGhost, row, set.no_extinguish_dc_ghost)),
                        ControlsRow::UseWhileMining => out.push(Self::toggle(ButtonAction::ToggleUseWhileMining, row, set.use_while_mining)),
                        ControlsRow::CapsLock => out.push(Self::toggle(ButtonAction::ToggleCapsLockBlocked, row, set.caps_lock_blocked)),
                        // Eine Belegung je Zeile (die erste der Aktion): Linksklick belegt sie
                        // neu, Rechtsklick entfernt sie (s. window_event). Daneben H/T, beim
                        // Warten stattdessen "Erweitert" fuer weitere Belegungen.
                        ControlsRow::Bind(aktion) => {
                            let wartet = self.awaiting_keybind == Some((aktion, 0));
                            let kb = &set.keybinds;
                            let text = match kb.bind_at(aktion, 0) {
                                _ if wartet => "> Taste/Maus <".to_string(),
                                Some(b) if kb.count(aktion) > 1 => format!("{}  +{}", bind_display(b), kb.count(aktion) - 1),
                                Some(b) => bind_display(b),
                                None => String::new(),
                            };
                            let mut knopf = Self::nav_button(ButtonAction::Rebind(aktion, 0), keybind_rect(row), &text, UI_SCALE_SMALL);
                            knopf.color = if wartet {
                                theme::get(theme::Slot::AccentSurface)
                            } else if kb.count(aktion) == 0 {
                                theme::get(theme::Slot::Surface1)
                            } else {
                                theme::get(theme::Slot::Surface2)
                            };
                            out.push(knopf);
                            if wartet {
                                out.push(Self::nav_button(ButtonAction::OpenKeybindAdvanced(aktion), keybind_mode_rect(row, true), "Erweitert", UI_SCALE_SMALL));
                            } else if !aktion.is_press_action()
                                || (aktion == KeyBindAction::TogglePerspective && set.quicklook_target().is_some())
                            {
                                out.push(Self::nav_button(
                                    ButtonAction::ToggleKeybindMode(aktion),
                                    keybind_mode_rect(row, false),
                                    set.keybind_modes.get(aktion).short(),
                                    UI_SCALE_SMALL,
                                ));
                            }
                        }
                    }
                }
            }
            MenuScreen::HudEditor => {
                // Pixel-Editor offen: nur seine eigenen Knoepfe.
                if let Some(painter) = &self.crosshair_painter {
                    out.push(Self::nav_button(ButtonAction::CrosshairPainterDone, hud_done_button(), "Fertig", UI_SCALE_NORMAL));
                    let sym = format!("Symmetrie: {}", painter.symmetry.label());
                    for (row, (action, label)) in [
                        (ButtonAction::CrosshairPainterSymmetry, sym.as_str()),
                        (ButtonAction::CrosshairPainterStandard, "Standardkreuz"),
                        (ButtonAction::CrosshairPainterClear, "Leeren"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        out.push(Button {
                            action,
                            rect: hud_list_item_rect(row),
                            label: label.to_string(),
                            enabled: true,
                            color: theme::get(theme::Slot::Surface2),
                            scale: UI_SCALE_SMALL,
                            slider: None,
                        });
                    }
                    return out;
                }
                // Ohne offenes Einstellungsfenster keine Knoepfe -- nur die Elemente zum Ziehen.
                let Some(element) = self.hud_selected else { return out };
                {
                    {
                        let panel = hud_config_panel(element, &self.settings);
                        out.push(Self::nav_button(ButtonAction::HudDeselect, hud_config_close_button(panel), "Schließen", UI_SCALE_SMALL));
                        match element {
                            HudElement::Crosshair => {
                                // Komplementaer oder Einfarbig, rechts daneben die Farbe dazu.
                                let mode = self.settings.crosshair_color_mode;
                                out.extend(pick_buttons(
                                    settings_wide_rect(0),
                                    &[
                                        (CROSSHAIR_COLOR_MODES[0].0, ButtonAction::CrosshairColorPrev, mode == 0),
                                        (CROSSHAIR_COLOR_MODES[1].0, ButtonAction::CrosshairColorNext, mode == 1),
                                    ],
                                    true,
                                ));
                                let [r, g, b] = self.settings.crosshair_rgb();
                                out.push(Self::swatch(ColorTarget::Crosshair, crosshair_swatch_rect(), [r, g, b, 1.0]));
                                // Kontur gilt fuer jeden Farbmodus, steht deshalb fest auf
                                // Zeile 1.
                                out.push(Self::toggle(ButtonAction::ToggleCrosshairOutline, 1, self.settings.crosshair_outline));
                                // Der Ring (Schild-Cooldown, Timer) hat seinen eigenen Schalter.
                                out.push(Self::toggle(ButtonAction::ToggleCrosshairRingOutline, 2, self.settings.crosshair_ring_outline));
                                // Aufhellung NUR im Komplementaer-Modus: bei einer fest
                                // gewaehlten Farbe gaebe es nichts aufzuhellen, ein
                                // wirkungsloser Regler waere nur eine Attrappe.
                                if mode == CROSSHAIR_MODE_COMPLEMENTARY {
                                    let boost = self.settings.crosshair_brightness_boost;
                                    out.push(Self::step_slider(ButtonAction::CrosshairBoostPrev, ButtonAction::CrosshairBoostNext, stepper_value_rect(3), boost as f64, 0.0, 100.0, true));
                                }
                                // Form unter den Farbzeilen, darunter ihre Groesse -- bei der
                                // eigenen Form stattdessen der Pixel-Editor.
                                let zeile = crosshair_shape_row(mode);
                                let wahl = self.settings.crosshair_shape_choice();
                                out.push(Self::nav_button(ButtonAction::OpenChoice(Choice::CrosshairShape), settings_wide_rect(zeile), crosshair_shape_label(wahl), UI_SCALE_SMALL));
                                if self.settings.crosshair_custom {
                                    out.push(Self::nav_button(ButtonAction::OpenCrosshairPainter, settings_wide_rect(zeile + 1), "Pixel bearbeiten", UI_SCALE_SMALL));
                                } else {
                                    let (_, min, max, _) = CROSSHAIR_SHAPES[self.settings.crosshair_shape];
                                    out.push(Self::step_slider(
                                        ButtonAction::CrosshairSizePrev,
                                        ButtonAction::CrosshairSizeNext,
                                        stepper_value_rect(zeile + 1),
                                        self.settings.crosshair_size() as f64,
                                        min as f64,
                                        max as f64,
                                        true,
                                    ));
                                }
                            }
                            HudElement::Hotbar => {
                                let scale = self.settings.hud_hotbar_scale;
                                out.extend(Self::stepper(
                                    ButtonAction::HotbarScalePrev,
                                    ButtonAction::HotbarScaleNext,
                                    0,
                                    scale > 0,
                                    scale < self.max_gui_scale_setting(),
                                ));
                                let (weniger, mehr) = if self.settings.hotbar_durability_absolute {
                                    let p = self.settings.hotbar_durability_points;
                                    (p > HOTBAR_DURABILITY_POINTS[0], p < *HOTBAR_DURABILITY_POINTS.last().unwrap())
                                } else {
                                    let pct = self.settings.hotbar_durability_percent;
                                    (pct > HOTBAR_DURABILITY_STEP, pct < 100)
                                };
                                out.extend(Self::stepper(ButtonAction::HotbarDurabilityPrev, ButtonAction::HotbarDurabilityNext, 1, weniger, mehr));
                                let pauschal = self.settings.hotbar_durability_absolute;
                                let modus = ButtonAction::ToggleHotbarDurabilityMode;
                                out.extend(pick_buttons(settings_wide_rect(2), &[("Prozentual", modus, !pauschal), ("Pauschal", modus, pauschal)], true));
                                let balken = self.settings.hotbar_air_bar;
                                let luft = ButtonAction::ToggleHotbarAirBar;
                                out.extend(pick_buttons(settings_wide_rect(3), &[("Blasen", luft, !balken), ("Balken", luft, balken)], true));
                            }
                            HudElement::Chat => {
                                let set = &self.settings;
                                // Je Zeile: Pfeil-Aktionen, Wert und Bereich -- als Schieberegler.
                                let zeilen = [
                                    (ButtonAction::ChatScalePrev, ButtonAction::ChatScaleNext, set.chat_scale_percent, CHAT_SCALE_MIN, CHAT_SCALE_MAX),
                                    (ButtonAction::ChatWidthPrev, ButtonAction::ChatWidthNext, set.chat_width, CHAT_WIDTH_MIN, CHAT_WIDTH_MAX),
                                    (ButtonAction::ChatBackgroundPrev, ButtonAction::ChatBackgroundNext, set.chat_background, 0, 100),
                                    (ButtonAction::ChatOpenDimPrev, ButtonAction::ChatOpenDimNext, set.chat_open_dim, 0, 100),
                                    (ButtonAction::ChatLinesPrev, ButtonAction::ChatLinesNext, set.chat_lines, 1, CHAT_LINES_MAX),
                                    (ButtonAction::ChatLinesOpenPrev, ButtonAction::ChatLinesOpenNext, set.chat_lines_open, 1, CHAT_LINES_OPEN_MAX),
                                    (ButtonAction::ChatFadePrev, ButtonAction::ChatFadeNext, set.chat_fade_secs, 0, CHAT_FADE_STEPS[CHAT_FADE_STEPS.len() - 1]),
                                    (ButtonAction::ChatLineSpacingPrev, ButtonAction::ChatLineSpacingNext, set.chat_line_spacing, 0, CHAT_LINE_SPACING_MAX),
                                ];
                                for (row, (prev, next, wert, min, max)) in zeilen.into_iter().enumerate() {
                                    out.push(Self::step_slider(prev, next, stepper_value_rect(row), wert as f64, min as f64, max as f64, true));
                                }
                                let b = set.chat_bedrock;
                                out.extend(pick_buttons(settings_wide_rect(CHAT_STYLE_ROW), &[("Java", ButtonAction::ChatStyle(false), !b), ("Bedrock", ButtonAction::ChatStyle(true), b)], true));
                            }
                            HudElement::BossBar => {
                                let pct = self.settings.hud_bossbar_scale;
                                out.extend(Self::stepper(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    0,
                                    pct > HUD_SCALE_PCT_MIN,
                                    pct < HUD_SCALE_PCT_MAX,
                                ));
                            }
                            HudElement::Effects => {
                                let pct = self.settings.hud_effects_scale;
                                out.extend(Self::stepper(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    0,
                                    pct > HUD_SCALE_PCT_MIN,
                                    pct < HUD_SCALE_PCT_MAX,
                                ));
                                out.push(Self::toggle(ButtonAction::ToggleEffectsHud, 1, self.settings.show_effects_hud));
                                out.push(Self::toggle(ButtonAction::ToggleEffectsInventory, 2, self.settings.show_effects_inventory));
                                let set = &self.settings;
                                out.push(Self::toggle(ButtonAction::ToggleEffectsHideAmbient, EFFECTS_HIDE_AMBIENT_ROW, set.effects_hide_ambient));
                                let detailliert = set.effects_detailed;
                                out.extend(pick_buttons(
                                    settings_wide_rect(EFFECTS_STYLE_ROW),
                                    &[("Vanilla", ButtonAction::EffectsDetailed(false), !detailliert), ("Detailliert", ButtonAction::EffectsDetailed(true), detailliert)],
                                    true,
                                ));
                                if detailliert {
                                    let wahl: Vec<_> =
                                        EFFECTS_ALIGNS.iter().enumerate().map(|(i, &name)| (name, ButtonAction::EffectsAlign(i as u8), set.effects_align == i as u8)).collect();
                                    out.extend(pick_buttons(hud_config_pick_rect(EFFECTS_ALIGN_ROW), &wahl, true));
                                    out.push(Self::toggle(ButtonAction::ToggleEffectsIcon, EFFECTS_ICON_ROW, set.effects_icon));
                                    let farbe = |r: Rect, aktion: fn(bool) -> ButtonAction, effekt: bool, an: bool| {
                                        pick_buttons(r, &[("Weiß", aktion(false), !effekt), ("Effektfarbe", aktion(true), effekt)], an)
                                    };
                                    // Name: Schalter, rechts daneben seine Farbe.
                                    out.push(Self::toggle(ButtonAction::ToggleEffectsName, EFFECTS_NAME_ROW, set.effects_name));
                                    let neben = Rect { x: SETTINGS_CTRL_X + TOGGLE_W + STEPPER_GAP, ..settings_wide_rect(EFFECTS_NAME_ROW) };
                                    out.extend(farbe(neben, ButtonAction::EffectsNameColor, set.effects_name_color, set.effects_name));
                                    let wahl: Vec<_> = EFFECTS_TIMER_STYLES
                                        .iter()
                                        .enumerate()
                                        .map(|(i, &name)| (name, ButtonAction::EffectsTimerStyle(i as u8), set.effects_timer_style == i as u8))
                                        .collect();
                                    out.extend(pick_buttons(hud_config_pick_rect(EFFECTS_TIMER_ROW), &wahl, true));
                                    out.extend(farbe(settings_wide_rect(EFFECTS_TIMER_COLOR_ROW), ButtonAction::EffectsTimerColor, set.effects_timer_color, true));
                                    let wahl: Vec<_> = EFFECTS_BLINK_MODES
                                        .iter()
                                        .enumerate()
                                        .map(|(i, &name)| (name, ButtonAction::EffectsTimerBlink(i as u8), set.effects_timer_blink == i as u8))
                                        .collect();
                                    out.extend(pick_buttons(hud_config_pick_rect(EFFECTS_BLINK_ROW), &wahl, true));
                                }
                            }
                            HudElement::Title | HudElement::Subtitle | HudElement::ActionBar => {
                                let aktuell = match element {
                                    HudElement::Title => self.settings.hud_title_align,
                                    HudElement::Subtitle => self.settings.hud_subtitle_align,
                                    _ => self.settings.hud_actionbar_align,
                                };
                                if element != HudElement::ActionBar {
                                    let pct = if element == HudElement::Title { self.settings.hud_title_scale } else { self.settings.hud_subtitle_scale };
                                    out.extend(Self::stepper(
                                        ButtonAction::HudScaleStep(element, false),
                                        ButtonAction::HudScaleStep(element, true),
                                        1,
                                        pct > HUD_SCALE_PCT_MIN,
                                        pct < HUD_SCALE_PCT_MAX,
                                    ));
                                }
                                for (i, name) in ["Links", "Mittig", "Rechts"].into_iter().enumerate() {
                                    out.push(Button {
                                        action: ButtonAction::HudAlign(element, i as u8),
                                        rect: hud_align_button(i),
                                        label: name.to_string(),
                                        enabled: true,
                                        color: if aktuell == i as u8 { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                                        scale: UI_SCALE_SMALL,
                                        slider: None,
                                    });
                                }
                            }
                            HudElement::Keystrokes => {
                                let set = &self.settings;
                                out.push(Self::step_slider(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    stepper_value_rect(0),
                                    set.hud_keys_scale as f64,
                                    HUD_SCALE_PCT_MIN as f64,
                                    HUD_SCALE_PCT_MAX as f64,
                                    true,
                                ));
                                out.push(Self::step_slider(
                                    ButtonAction::KeysOpacityPrev,
                                    ButtonAction::KeysOpacityNext,
                                    stepper_value_rect(1),
                                    set.keys_opacity as f64,
                                    0.0,
                                    100.0,
                                    true,
                                ));
                            }
                            HudElement::Session => {
                                let set = &self.settings;
                                out.push(Self::step_slider(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    stepper_value_rect(0),
                                    set.hud_session_scale as f64,
                                    HUD_SCALE_PCT_MIN as f64,
                                    HUD_SCALE_PCT_MAX as f64,
                                    true,
                                ));
                                out.push(Self::step_slider(
                                    ButtonAction::SessionOpacity(false),
                                    ButtonAction::SessionOpacity(true),
                                    stepper_value_rect(1),
                                    set.session_opacity as f64,
                                    0.0,
                                    100.0,
                                    true,
                                ));
                            }
                            HudElement::Armor => {
                                let set = &self.settings;
                                out.push(Self::step_slider(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    stepper_value_rect(0),
                                    set.hud_armor_scale as f64,
                                    HUD_SCALE_PCT_MIN as f64,
                                    HUD_SCALE_PCT_MAX as f64,
                                    true,
                                ));
                                let lagen: Vec<_> = armor_hud::LAYOUT_NAMES
                                    .iter()
                                    .enumerate()
                                    .map(|(i, &name)| (name, ButtonAction::ArmorHudLayout(i as u8), set.armor_hud_layout == i as u8))
                                    .collect();
                                out.extend(pick_buttons(hud_config_pick_rect(1), &lagen, true));
                                let (namen, gewaehlt) = if armor_hud::is_vertical(set.armor_hud_layout) {
                                    (armor_hud::SORT_VERTICAL_NAMES, set.armor_hud_sort_vertical)
                                } else {
                                    (armor_hud::SORT_FLAT_NAMES, set.armor_hud_sort_flat)
                                };
                                let wahl: Vec<_> = namen.iter().enumerate().map(|(i, &name)| (name, ButtonAction::ArmorHudSort(i as u8), gewaehlt == i as u8)).collect();
                                out.extend(pick_buttons(hud_config_pick_rect(2), &wahl, true));
                                out.push(Self::step_slider(
                                    ButtonAction::ArmorHudGapPrev,
                                    ButtonAction::ArmorHudGapNext,
                                    stepper_value_rect(3),
                                    set.armor_hud_gap as f64,
                                    armor_hud::GAP_MIN as f64,
                                    armor_hud::GAP_MAX as f64,
                                    true,
                                ));
                                out.push(Self::step_slider(
                                    ButtonAction::ArmorHudNumberScalePrev,
                                    ButtonAction::ArmorHudNumberScaleNext,
                                    stepper_value_rect(4),
                                    set.armor_hud_number_scale as f64,
                                    armor_hud::NUMBER_SCALE_MIN as f64,
                                    armor_hud::NUMBER_SCALE_MAX as f64,
                                    true,
                                ));
                                for (row, wert, (weniger, mehr)) in [
                                    (5, set.armor_hud_number_x, (ButtonAction::ArmorHudNumberXPrev, ButtonAction::ArmorHudNumberXNext)),
                                    (6, set.armor_hud_number_y, (ButtonAction::ArmorHudNumberYPrev, ButtonAction::ArmorHudNumberYNext)),
                                ] {
                                    out.push(Self::step_slider(
                                        weniger,
                                        mehr,
                                        stepper_value_rect(row),
                                        wert as f64,
                                        armor_hud::NUMBER_OFFSET_MIN as f64,
                                        armor_hud::NUMBER_OFFSET_MAX as f64,
                                        true,
                                    ));
                                }
                                out.push(Self::toggle(ButtonAction::ToggleArmorHudGreen, 7, set.armor_hud_green));
                            }
                            HudElement::HeldName => {
                                let pct = self.settings.hud_held_name_scale;
                                out.extend(Self::stepper(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    0,
                                    pct > HUD_SCALE_PCT_MIN,
                                    pct < HUD_SCALE_PCT_MAX,
                                ));
                            }
                            HudElement::Scoreboard => {
                                let pct = self.settings.hud_scoreboard_scale;
                                out.extend(Self::stepper(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    0,
                                    pct > HUD_SCALE_PCT_MIN,
                                    pct < HUD_SCALE_PCT_MAX,
                                ));
                            }
                            HudElement::ShieldCooldown => {
                                let stil = self.settings.shield_cd_text_style;
                                let aktion = ButtonAction::CycleShieldCdTextStyle;
                                out.extend(pick_buttons(
                                    settings_wide_rect(0),
                                    &[(HUD_TEXT_STYLE_NAMES[0], aktion, stil == 0), (HUD_TEXT_STYLE_NAMES[1], aktion, stil == 1)],
                                    true,
                                ));
                            }
                            HudElement::Arrows => {
                                let set = &self.settings;
                                let pct = set.hud_arrows_scale;
                                out.push(Self::step_slider(
                                    ButtonAction::HudScaleStep(element, false),
                                    ButtonAction::HudScaleStep(element, true),
                                    stepper_value_rect(0),
                                    pct as f64,
                                    HUD_SCALE_PCT_MIN as f64,
                                    HUD_SCALE_PCT_MAX as f64,
                                    true,
                                ));
                                out.push(Self::toggle(ButtonAction::ToggleArrowNumberOnly, 1, set.arrow_number_only));
                                out.push(Self::step_slider(
                                    ButtonAction::ArrowOpacityPrev,
                                    ButtonAction::ArrowOpacityNext,
                                    stepper_value_rect(2),
                                    set.arrow_opacity as f64,
                                    ARROW_OPACITY_MIN as f64,
                                    100.0,
                                    true,
                                ));
                                let stil = set.arrow_text_style;
                                let aktion = ButtonAction::CycleArrowTextStyle;
                                out.extend(pick_buttons(
                                    settings_wide_rect(3),
                                    &[(HUD_TEXT_STYLE_NAMES[0], aktion, stil == 0), (HUD_TEXT_STYLE_NAMES[1], aktion, stil == 1)],
                                    true,
                                ));
                            }
                            // Nur Position, keine weiteren Bedienelemente (nur Reset unten).
                            HudElement::Perf => {}
                        }
                        out.push(Self::nav_button(
                            ButtonAction::ResetHudElement,
                            hud_reset_button(hud_config_rows(element, &self.settings)),
                            "Zurücksetzen",
                            UI_SCALE_SMALL,
                        ));
                    }
                }
            }
            MenuScreen::Death => {
                out.push(Self::nav_button(ButtonAction::Respawn, pause_button(0, screen_w), "Respawn", UI_SCALE_NORMAL));
                out.push(Self::nav_button(ButtonAction::Disconnect, pause_button(1, screen_w), "Disconnect", UI_SCALE_NORMAL));
            }
        }
        out
    }

    pub(crate) fn handle_click(&mut self, px: f32, py: f32) {
        // Das Modul-Menue ist ein In-Welt-Overlay, seine Knoepfe liegen also NICHT in
        // buttons() (das gilt nur fuer Menue-Screens). Eigene Trefferpruefung, danach
        // derselbe Weg durch apply_button_action wie jeder Menue-Klick.
        if self.module_menu_shown() {
            let buttons = self.gpu.as_ref().map(|gpu| self.module_menu_buttons(gpu)).unwrap_or_default();
            // In den Einstellungen liegt die Tafel im verschobenen Inhalt (s. content_offset).
            let (dx, dy) = self.content_offset();
            let (qx, qy) = (px - dx, py - dy);
            let hit = buttons.iter().find(|b| b.enabled && b.hits(qx, qy)).map(|b| b.action);
            if let Some(t) = hit.and_then(Self::color_target_of) {
                let r = buttons.iter().find(|b| b.enabled && b.hits(qx, qy)).map(|b| b.rect).unwrap_or(Rect { x: qx, y: qy, w: 0.0, h: 0.0 });
                self.play_ui_sound(ui_sound::UiSound::Click);
                self.open_color_popup(t, Rect { x: r.x + dx, y: r.y + dy, ..r });
                return;
            }
            // In den Einstellungen liegt daneben noch der Zurueck-Knopf (buttons()).
            if hit.is_none() && self.mode == AppMode::Menu && self.buttons().iter().any(|b| b.hits(px, py)) {
                return self.handle_menu_click(px, py);
            }
            let Some(action) = hit else {
                // Genau wie in den Menue-Screens (s. unten): danebengeklickt ist stumm, auf
                // einen ABGEBLENDETEN Knopf geklickt nicht. Ohne das gab es hier gar keine
                // Rueckmeldung -- die Spalte eines abgeschalteten Moduls wirkte dadurch
                // kaputt statt gesperrt.
                if buttons.iter().any(|b| !b.enabled && b.hits(qx, qy)) {
                    self.play_ui_sound(ui_sound::UiSound::Denied);
                }
                return;
            };
            if action == ButtonAction::Nothing {
                return;
            }
            // Auswahl ab drei Werten: Liste aufklappen statt weiterschalten.
            if let Some(c) = Self::choice_of(action) {
                let r = buttons.iter().find(|b| b.enabled && b.hits(qx, qy)).map_or(Rect { x: qx, y: qy, w: 0.0, h: 0.0 }, |b| b.rect);
                self.toggle_dropdown(c, Rect { x: r.x + dx, y: r.y + dy, ..r });
                return;
            }
            // Schieberegler: Klick setzt den Wert unter den Zeiger, Ziehen fuehrt weiter (wie
            // in handle_menu_click).
            if let Some(sl) = buttons.iter().find(|b| b.enabled && b.hits(qx, qy)).and_then(|b| b.slider) {
                self.play_ui_sound(ui_sound::UiSound::Click);
                if sl.value_box().contains(qx, qy) {
                    self.begin_step_edit(sl.prev);
                    return;
                }
                self.step_drag = Some(sl.prev);
                self.step_slider_drag();
                return;
            }
            self.play_ui_sound(ui_sound_for_action(action, &self.settings));
            // "Zu" in den Einstellungen: eine Ebene zurueck statt ein Overlay schliessen.
            if action == ButtonAction::CloseModuleMenu && self.mode == AppMode::Menu {
                self.navigate_back();
                return;
            }
            self.apply_module_action(action);
            self.persist_settings();
            // Einfarbig gewaehlt: gleich das Farbfenster neben dem Farbfeld dazu.
            if action == ButtonAction::SetBlockOutlineComplementary(false) && self.color_popup.is_none() {
                if let Some(feld) = buttons.iter().find(|b| b.action == ButtonAction::BlockOutlineEditor) {
                    self.open_color_popup(ColorTarget::BlockOutline, Rect { x: feld.rect.x + dx, y: feld.rect.y + dy, ..feld.rect });
                }
            }
            return;
        }
        self.handle_menu_click(px, py);
    }

    // Klick auf die Knoepfe eines Menue-Screens (buttons()).
    pub(crate) fn handle_menu_click(&mut self, px: f32, py: f32) {
        // Buttons nur im Menu-Modus.
        if self.mode != AppMode::Menu {
            return;
        }
        // buttons() borgt self immutabel; Aktion erst danach ausfuehren.
        let treffer = self.buttons().into_iter().find(|b| b.enabled && b.hits(px, py));
        // Farbfelder oeffnen das Farbfenster unter sich -- dafuer zaehlt ihr Rechteck.
        if let Some(t) = treffer.as_ref().and_then(|b| Self::color_target_of(b.action)) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            // Die Kreuz-Farbe wirkt nur im Modus Einfarbig: wer sie anklickt, will den.
            if t == ColorTarget::Crosshair {
                self.settings.crosshair_color_mode = 1;
            }
            let anker = treffer.as_ref().map_or(Rect { x: px, y: py, w: 0.0, h: 0.0 }, |b| b.rect);
            self.open_color_popup(t, anker);
            return;
        }
        // Auswahl ab drei Werten: Liste unter dem Knopf aufklappen statt weiterschalten.
        if let Some((c, anker)) = treffer.as_ref().and_then(|b| Self::choice_of(b.action).map(|c| (c, b.rect))) {
            self.toggle_dropdown(c, anker);
            return;
        }
        let hit = treffer.map(|b| b.action);

        let Some(action) = hit else {
            // Danebengeklickt ist stumm; auf einen ABGEBLENDETEN Knopf geklickt nicht -- dort
            // erwartet man eine Reaktion und bekommt sonst gar keine Rueckmeldung.
            if self.buttons().iter().any(|b| !b.enabled && b.hits(px, py)) {
                self.play_ui_sound(ui_sound::UiSound::Denied);
            }
            return;
        };
        // Der schon gewaehlte Knopf einer Auswahl: stumm, es aendert sich nichts.
        if action == ButtonAction::Nothing {
            return;
        }
        // Schieberegler: Klick setzt den Wert unter den Zeiger, Ziehen fuehrt weiter.
        if let Some(sl) = self.buttons().into_iter().find(|b| b.enabled && b.hits(px, py)).and_then(|b| b.slider) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            // Die Zahl angeklickt: eintippen statt ziehen (der Regler liegt in
            // Inhaltskoordinaten, s. pointer).
            let (qx, qy) = self.pointer();
            if sl.value_box().contains(qx, qy) {
                self.begin_step_edit(sl.prev);
                return;
            }
            self.step_drag = Some(sl.prev);
            self.step_slider_drag();
            return;
        }
        // Bei den Reglern der einzelnen UI-Klaenge spielt statt des Klicks der Klang selbst,
        // und zwar NACH der Aenderung -- so hoert man die neue Lautstaerke.
        let vorhoeren = match action {
            ButtonAction::UiSoundVolumeDown(i) | ButtonAction::UiSoundVolumeUp(i) => ui_sound::UiSound::ALL.get(i).copied(),
            _ => None,
        };
        if vorhoeren.is_none() {
            let in_einstellungen = self.screen == MenuScreen::Settings || self.screen.settings_category().is_some();
            let klang = match action {
                ButtonAction::ToggleNotificationPanel if self.notification_panel_open => ui_sound::UiSound::Close,
                ButtonAction::ToggleNotificationPanel => ui_sound::UiSound::Open,
                // Das Zahnrad oeffnet die Einstellungen, in ihnen fuehrt es hinaus.
                ButtonAction::ToggleSettings if in_einstellungen => ui_sound::UiSound::Close,
                ButtonAction::ToggleSettings => ui_sound::UiSound::Open,
                // Die Einstellungen von aussen betreten (Optionen im Pausemenue usw.).
                ButtonAction::OpenScreen(ziel) if !in_einstellungen && (ziel == MenuScreen::Settings || ziel.settings_category().is_some()) => ui_sound::UiSound::Open,
                // Offene Kontenliste: derselbe Knopf oeffnet den Acc-Manager, ein Fenster.
                ButtonAction::ToggleAccountMenu if self.account_menu_open => ui_sound::UiSound::Open,
                _ => ui_sound_for_action(action, &self.settings),
            };
            self.play_ui_sound(klang);
        }
        self.run_menu_action(action);
        if let Some(klang) = vorhoeren {
            self.play_ui_sound(klang);
        }
        // Einfarbig gewaehlt: gleich das Farbfenster neben dem Farbfeld dazu.
        if action == ButtonAction::CrosshairColorNext && self.color_popup.is_none() {
            if let Some(feld) = self.buttons().into_iter().find(|b| b.action == ButtonAction::OpenColorPicker(ColorTarget::Crosshair)) {
                self.open_color_popup(ColorTarget::Crosshair, feld.rect);
            }
        }
    }

    // Eine Menue-Aktion ausfuehren (Klick oder ein Schritt eines Schiebereglers).
    pub(crate) fn run_menu_action(&mut self, action: ButtonAction) {
        // Das Fenster "Erweitert" schliesst sich bei allem, was nicht zu ihm gehoert (z.B. ein
        // Klick in die Kategorienleiste).
        if !matches!(action, ButtonAction::Rebind(..) | ButtonAction::OpenKeybindAdvanced(_) | ButtonAction::CloseKeybindAdvanced | ButtonAction::Nothing) {
            self.keybind_advanced = None;
        }
        // Alles ausser Tab-Wechsel und Verbindungs-Aktionen veraendert Einstellungen und
        // wird danach sofort geschrieben.
        // TearFree ist ausgenommen: das ist externer Systemzustand, kein Teil unserer
        // Settings-Datei -- ihn "speichern" waere falsch, er lebt schon in xrandr.
        // Rebind ebenfalls: der Klick startet nur das Warten auf die naechste Taste, die
        // eigentliche Aenderung (und ihr persist_settings()) passiert erst in
        // window_event(), wenn diese Taste kommt.
        let changes_settings = !matches!(
            action,
            ButtonAction::OpenScreen(_)
                | ButtonAction::OpenLink(_)
                | ButtonAction::OpenQuickConnect
                | ButtonAction::ToggleAccountMenu
                | ButtonAction::SettingsSection(_)
                | ButtonAction::ToggleSettings
                | ButtonAction::ToggleNotificationPanel
                | ButtonAction::Back
                | ButtonAction::Connect
                | ButtonAction::Disconnect
                | ButtonAction::ToggleTearFree
                | ButtonAction::Rebind(..)
                | ButtonAction::OpenKeybindAdvanced(_)
                | ButtonAction::CloseKeybindAdvanced
                | ButtonAction::Respawn
                | ButtonAction::HudDeselect
                | ButtonAction::AudioTestTone
                | ButtonAction::PipewireLimit(_)
                | ButtonAction::AudioSpatialTest
                | ButtonAction::OpenSkinFolder
                | ButtonAction::ReloadSkins
                | ButtonAction::OpenConfigFolder
                | ButtonAction::ServerLinks
                // Configs speichern selbst (s. switch_config); ein Speichern danach schriebe
                // nur dieselben Werte noch einmal.
                | ButtonAction::SelectConfig(_)
                | ButtonAction::EditConfig(_)
                | ButtonAction::DeleteConfig
                | ButtonAction::OpenInputField(_)
                | ButtonAction::SubmitInputField
                // Der Dialog schreibt nur die Bilddatei; die Einstellung folgt, wenn er fertig
                // ist (s. poll_wallpaper).
                | ButtonAction::PickWallpaper
                // Die Server-Liste lebt in servers.conf, nicht in den Einstellungen -- sie
                // schreibt sich selbst (persist_servers), settings.conf bleibt unberuehrt.
                | ButtonAction::ConnectToServer(_)
                | ButtonAction::NewServer
                | ButtonAction::CloseModuleMenu
        );

        let max_scale = self.max_gui_scale_setting();
        let set = &mut self.settings;
        match action {
            ButtonAction::Nothing => {}
            // Klappt nur die Liste auf (s. toggle_dropdown), aendert selbst nichts.
            ButtonAction::OpenChoice(_) => {}
            // Die Modul-Aktionen koennen hier nicht auftreten: das Modul-Menue ist ein
            // In-Welt-Overlay, seine Knoepfe stehen nicht in buttons() und laufen ueber
            // apply_module_action (s. handle_click). Der Arm ist nur da, damit das match
            // vollstaendig bleibt und ein neues Overlay-Element nicht still hier landet.
            ButtonAction::ToggleModuleTotem
            | ButtonAction::ToggleAddonModule(_)
            | ButtonAction::AddonToggle(..)
            | ButtonAction::AddonStep(..)
            | ButtonAction::AddonButton(..)
            | ButtonAction::ToggleModuleTotemSound
            | ButtonAction::Toteming(_)
            | ButtonAction::SelectModule(_)
            | ButtonAction::SelectModuleCategory(_)
            | ButtonAction::ModuleBack
            | ButtonAction::ToggleModuleHitbox
            | ButtonAction::ToggleModuleHitboxSmoothed
            | ButtonAction::ToggleModuleHitboxRaw
            | ButtonAction::ToggleModuleHitboxDotted
            | ButtonAction::ToggleModuleHitboxItems
            | ButtonAction::ToggleModuleHitboxSelf
            | ButtonAction::HitboxDensityPrev
            | ButtonAction::HitboxDensityNext
            | ButtonAction::HitboxOpacityPrev
            | ButtonAction::HitboxOpacityNext
            | ButtonAction::HitboxColorEditor(_)
            | ButtonAction::ToggleHitboxKind(_)
            | ButtonAction::ToggleHitboxKindOverride(_)
            | ButtonAction::ToggleModuleSkyColor
            | ButtonAction::ToggleModuleGhostTolerance
            | ButtonAction::ToggleModuleCrystalOptimizer
            | ButtonAction::ToggleModuleConsumableOptimizer
            | ButtonAction::EatHoldThroughPrev
            | ButtonAction::EatHoldThroughNext
            | ButtonAction::ToggleEatSlotLock
            | ButtonAction::ToggleEatAggressivePrediction
            | ButtonAction::ToggleModuleUseTimer
            | ButtonAction::ToggleModuleNoToolBreak
            | ButtonAction::ToggleModuleBotcheckPass
            | ButtonAction::ToggleModuleDynamicFire
            | ButtonAction::ToggleModuleZoom
            | ButtonAction::ToggleModuleVoiceChat
            | ButtonAction::VoiceStep(..)
            | ButtonAction::VoiceFlag(_)
            | ButtonAction::ZoomFactorPrev
            | ButtonAction::ZoomFactorNext
            | ButtonAction::ZoomSensPrev
            | ButtonAction::ZoomSensNext
            | ButtonAction::NoToolBreakReservePrev
            | ButtonAction::NoToolBreakReserveNext
            | ButtonAction::GhostTolerancePrev
            | ButtonAction::LagDelayPrev
            | ButtonAction::LagDelayNext
            | ButtonAction::GhostToleranceNext
            | ButtonAction::ToggleModuleAutoclicker
            | ButtonAction::ToggleModuleSaturationHud
            | ButtonAction::SaturationHeightPrev
            | ButtonAction::SaturationHeightNext
            | ButtonAction::SaturationAlpha(..)
            | ButtonAction::ToggleModuleEgoName
            | ButtonAction::EditEgoAlias
            | ButtonAction::CycleEgoDeco
            | ButtonAction::CycleEgoShadow
            | ButtonAction::EgoLight(..)
            | ButtonAction::EgoSpeed(_)
            | ButtonAction::CycleEgoSound
            | ButtonAction::EgoVolume(_)
            | ButtonAction::AutoclickerCpsPrev
            | ButtonAction::AutoclickerCpsNext
            | ButtonAction::CycleSkyMode
            | ButtonAction::ToggleSkySun
            | ButtonAction::ToggleSkyMoon
            | ButtonAction::ToggleSkyStars
            | ButtonAction::ToggleSkyFixedTime
            | ButtonAction::SkyTimePrev
            | ButtonAction::SkyTimeNext
            | ButtonAction::ToggleModuleWeather
            | ButtonAction::CycleWeatherMode
            | ButtonAction::WeatherRadiusPrev
            | ButtonAction::WeatherRadiusNext
            | ButtonAction::WeatherRadiusInnerPrev
            | ButtonAction::WeatherRadiusInnerNext
            | ButtonAction::ToggleModuleBlockOutline
            | ButtonAction::BlockOutlineEditor
            | ButtonAction::BlockOutlineWidthPrev
            | ButtonAction::BlockOutlineWidthNext
            | ButtonAction::ToggleBlockOutlineHideMining
            | ButtonAction::SetBlockOutlineComplementary(_)
            | ButtonAction::ToggleModuleShields
            | ButtonAction::ToggleModuleArrows
            | ButtonAction::ToggleModuleArmorHud
            | ButtonAction::ToggleModuleTeams
            | ButtonAction::TeamToggle(_)
            | ButtonAction::TeamMatch(_)
            | ButtonAction::TeamOwnColor(_)
            | ButtonAction::TeamNametag(..)
            | ButtonAction::TeamOpacity(..)
            | ButtonAction::TeamDensity(..)
            | ButtonAction::TeamDotted(_)
            | ButtonAction::ToggleModuleKeystrokes
            | ButtonAction::ToggleModuleSessionInfo
            | ButtonAction::ToggleSessionLine(_)
            | ButtonAction::ResetSession
            | ButtonAction::ToggleArrowIndicator
            | ButtonAction::ToggleArrowWhite
            | ButtonAction::ToggleArrowTrail
            | ButtonAction::TrailHoldPrev
            | ButtonAction::TrailHoldNext
            | ButtonAction::TrailFadePrev
            | ButtonAction::TrailFadeNext
            | ButtonAction::ToggleShieldCdSelf
            | ButtonAction::ToggleShieldCdPlayers
            | ButtonAction::ToggleShieldHoldPlayers
            | ButtonAction::ShieldColorEditor(_)
            | ButtonAction::CycleShieldMark(_)
            | ButtonAction::CycleArrowEmptyFeedback
            | ButtonAction::ToggleModuleShowMarkers
            | ButtonAction::ToggleModuleSoundDebug
            | ButtonAction::ToggleModuleConnectionDebug
            | ButtonAction::ToggleModuleDelay
            | ButtonAction::ToggleModuleDiscordStatus
            | ButtonAction::ToggleModuleInputLog
            | ButtonAction::ToggleModuleColor
            | ButtonAction::ToggleModuleSustainability
            | ButtonAction::ToggleSustainBackground
            | ButtonAction::ToggleSustainPause
            | ButtonAction::ToggleModuleChunkSaver
            | ButtonAction::ToggleChunkSaverRenderSaved
            | ButtonAction::ChunkSaverThreadsPrev
            | ButtonAction::ChunkSaverThreadsNext
            | ButtonAction::ColorSaturationPrev
            | ButtonAction::ColorSaturationNext
            | ButtonAction::DiscordPasteClientId
            | ButtonAction::ShieldColorPrev(_, _)
            | ButtonAction::ShieldColorNext(_, _)
            | ButtonAction::CloseModuleMenu => {}
            ButtonAction::ToggleSettings => {
                if self.screen.settings_category().is_some() {
                    self.screen = self.settings_return;
                } else {
                    self.open_settings(MenuScreen::Settings);
                }
            }
            // Abgefangen in handle_menu_click/handle_click (braucht das Rechteck des Knopfs).
            ButtonAction::OpenColorPicker(_) => {}
            ButtonAction::ApplyThemePreset(i) => {
                if i < theme::PRESETS.len() {
                    set.select_theme(ThemeChoice::Preset(i));
                }
            }
            ButtonAction::SelectCustomTheme(i) => {
                if i < set.custom_themes.len() {
                    set.select_theme(ThemeChoice::Custom(i));
                }
            }
            ButtonAction::NewCustomTheme => {
                self.theme_name_dialog = Some(ThemeNameDialog {
                    field: text_field::TextField::new("", THEME_NAME_MAX),
                    dragging: false,
                    last_press: None,
                    purpose: NameDialogPurpose::Theme,
                });
            }
            ButtonAction::SettingsSection(i) => self.jump_to_section(i as usize),
            ButtonAction::ToggleUptime => self.settings.topbar_uptime = !self.settings.topbar_uptime,
            ButtonAction::SelectTab(i) => self.switch_tab(i),
            ButtonAction::CloseTab(i) => self.close_tab(i),
            ButtonAction::NewTab => self.new_tab(),
            ButtonAction::OpenLink(i) => {
                if let Some((_, url, _)) = SIDEBAR_LINKS.get(i) {
                    match open_external_command(url).spawn() {
                        Ok(_) => self.push_log(format!("Link geöffnet: {url}")),
                        Err(e) => self.push_log(format!("Link konnte nicht geöffnet werden ({e}): {url}")),
                    }
                }
            }
            ButtonAction::OpenQuickConnect => {
                self.quick_connect_open = true;
                self.quick_field = text_field::TextField::new("", MAX_ADDRESS_LEN);
                self.quick_dragging = false;
                self.quick_last_press = None;
            }
            ButtonAction::ToggleAccountMenu => {
                if self.account_menu_open {
                    // Offen ist der Knopf der Acc-Manager: er oeffnet das Konten-Fenster.
                    self.account_menu_open = false;
                    self.account_manager_open = true;
                    self.account_context = None;
                } else {
                    self.account_menu_open = true;
                }
                // Koepfe laden und alle Tokens pruefen, sobald die Liste aufgeht.
                self.refresh_account_list();
            }
            ButtonAction::ToggleNotificationPanel => {
                self.set_notification_panel(!self.notification_panel_open);
                self.notifications_seen = self.notification_history.len();
            }
            ButtonAction::OpenScreen(target) if target.settings_category().is_some() || target == MenuScreen::Settings => {
                self.open_settings(target);
            }
            ButtonAction::OpenScreen(target) => {
                if target == MenuScreen::HudEditor {
                    self.hud_editor_return = self.screen;
                }
                self.screen = target;
                // Beim Betreten von Connect/Account ggf. eine gespeicherte Anmeldung lautlos
                // erneuern (Refresh-Token), damit man ohne erneuten Login verbinden kann.
                if target == MenuScreen::Connect || target == MenuScreen::Account {
                    self.try_refresh_login();
                }
            }
            ButtonAction::Back => self.navigate_back(),
            ButtonAction::Connect => self.go_connect(),
            ButtonAction::Disconnect => self.disconnect(),
            ButtonAction::Reconnect => self.reconnect(),
            ButtonAction::MicrosoftLogin => self.start_microsoft_login(),
            ButtonAction::OpenInputField(kind) => {
                // Toggle: nochmal auf denselben Button schliesst das Feld; ein anderer wechselt.
                self.input_field = match &self.input_field {
                    Some((k, _)) if *k == kind => None,
                    _ => {
                        let prefill = match kind {
                            InputKind::Token | InputKind::RefreshToken => String::new(),
                            InputKind::ConfigName(ConfigEdit::Rename) => active_config(),
                            InputKind::ConfigName(_) => String::new(),
                        };
                        Some((kind, prefill))
                    }
                };
            }
            ButtonAction::SubmitInputField => self.submit_input_field(),
            ButtonAction::SelectConfig(i) => {
                self.config_delete_armed = false;
                if let Some(name) = assets::list_configs().get(i).cloned() {
                    self.switch_config(&name);
                }
            }
            ButtonAction::EditConfig(edit) => {
                self.config_delete_armed = false;
                let kind = InputKind::ConfigName(edit);
                self.input_field = match &self.input_field {
                    Some((k, _)) if *k == kind => None,
                    _ => Some((kind, if edit == ConfigEdit::Rename { active_config() } else { String::new() })),
                };
            }
            ButtonAction::DeleteConfig => {
                if !std::mem::replace(&mut self.config_delete_armed, true) {
                    return;
                }
                self.config_delete_armed = false;
                let name = active_config();
                match assets::delete_config(&name) {
                    Ok(()) => {
                        let naechste = assets::list_configs().into_iter().next().unwrap_or_else(|| assets::DEFAULT_CONFIG.to_string());
                        // Die geloeschte darf beim Wechsel nicht wieder gespeichert werden.
                        self.switch_config_unsaved(&naechste);
                        self.notify(notifications::Notification::info("Config gelöscht", format!("\"{name}\" -- jetzt aktiv: {naechste}")));
                    }
                    Err(e) => {
                        self.notify(notifications::Notification::error("Config nicht gelöscht", e.to_string()));
                    }
                }
            }
            ButtonAction::Logout => self.logout(),
            ButtonAction::UseAccount(i) => self.use_account(i),
            // --- Server-Liste (Connect-Screen) ---
            // Klick auf eine Karte verbindet sofort.
            ButtonAction::ConnectToServer(i) => {
                if let Some(entry) = self.servers.get(i) {
                    // Adresse ins Eingabefeld links uebernehmen, damit sichtbar ist, womit
                    // gerade verbunden wird (und ein Reconnect ohne Umweg funktioniert).
                    self.server_address = entry.address.clone();
                    self.go_connect();
                }
            }
            ButtonAction::NewServer => self.open_server_dialog(None),
            ButtonAction::ToggleTearing => set.show_tearing_test = !set.show_tearing_test,
            ButtonAction::ToggleHud => set.show_hud = !set.show_hud,
            ButtonAction::ToggleCmaa2 => set.cmaa2_enabled = !set.cmaa2_enabled,
            ButtonAction::FrameLatencyDown | ButtonAction::FrameLatencyUp => {
                let delta: i32 = if action == ButtonAction::FrameLatencyDown { -1 } else { 1 };
                set.frame_latency = (set.frame_latency as i32 + delta).clamp(FRAME_LATENCY_MIN as i32, FRAME_LATENCY_MAX as i32) as u32;
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.set_frame_latency(set.frame_latency);
                }
            }
            ButtonAction::FpsLimitPrev => set.fps_limit = fps_limit_step(set.fps_limit, false).unwrap_or(set.fps_limit),
            ButtonAction::FpsLimitNext => set.fps_limit = fps_limit_step(set.fps_limit, true).unwrap_or(set.fps_limit),
            // Beide Regler stehen immer da; ohne "X/Y getrennt" zieht der eine den anderen mit.
            ButtonAction::SensitivityDown | ButtonAction::SensitivityUp => {
                set.mouse_sensitivity = step_sensitivity(set.mouse_sensitivity, action == ButtonAction::SensitivityUp);
                set.couple_sensitivity(false);
                self.sens_last_changed_y = false;
            }
            ButtonAction::SensitivityYDown | ButtonAction::SensitivityYUp => {
                set.mouse_sensitivity_y = step_sensitivity(set.mouse_sensitivity_y, action == ButtonAction::SensitivityYUp);
                set.couple_sensitivity(true);
                self.sens_last_changed_y = true;
            }
            ButtonAction::ToggleSensSplit => {
                set.mouse_sens_split = !set.mouse_sens_split;
                // Wieder gleich: der Wert, den man NICHT zuletzt verstellt hat, gilt fuer beide.
                if !set.mouse_sens_split {
                    set.couple_sensitivity(!self.sens_last_changed_y);
                }
            }
            ButtonAction::OpenSensCalc => self.screen = MenuScreen::SettingsSensCalc,
            ButtonAction::OpenShaderSettings => self.screen = MenuScreen::SettingsShader,
            ButtonAction::ApplySensCalc => {
                set.mouse_sensitivity = minecraft_sens_to_client(set.sens_calc_mc_percent).clamp(SENSITIVITY_MIN, SENSITIVITY_MAX);
                set.couple_sensitivity(false);
            }
            ButtonAction::CursorSensitivityDown => set.cursor_sensitivity = step_cursor_sensitivity(set.cursor_sensitivity, false),
            ButtonAction::CursorSensitivityUp => set.cursor_sensitivity = step_cursor_sensitivity(set.cursor_sensitivity, true),
            ButtonAction::CursorSizeDown => set.cursor_size = set.cursor_size.saturating_sub(CURSOR_SIZE_STEP).max(CURSOR_SIZE_MIN),
            ButtonAction::CursorSizeUp => set.cursor_size = (set.cursor_size + CURSOR_SIZE_STEP).min(CURSOR_SIZE_MAX),
            ButtonAction::CursorWidthDown => set.cursor_width = set.cursor_width.saturating_sub(CURSOR_WIDTH_STEP).max(CURSOR_WIDTH_MIN),
            ButtonAction::CursorWidthUp => set.cursor_width = (set.cursor_width + CURSOR_WIDTH_STEP).min(CURSOR_WIDTH_MAX),
            ButtonAction::HoldConfirmDown => set.hold_confirm_ms = set.hold_confirm_ms.saturating_sub(HOLD_CONFIRM_STEP),
            ButtonAction::HoldConfirmUp => set.hold_confirm_ms = (set.hold_confirm_ms + HOLD_CONFIRM_STEP).min(HOLD_CONFIRM_MAX),
            ButtonAction::ReconnectHoldDown => set.reconnect_hold_ms = (set.reconnect_hold_ms - HOLD_CONFIRM_STEP as i32).max(RECONNECT_HOLD_GENERAL),
            ButtonAction::ReconnectHoldUp => set.reconnect_hold_ms = (set.reconnect_hold_ms + HOLD_CONFIRM_STEP as i32).min(HOLD_CONFIRM_MAX as i32),
            ButtonAction::ToggleCursorCircle => set.cursor_circle = !set.cursor_circle,
            ButtonAction::UiPitchDown => set.ui_pitch_semitones -= 1,
            ButtonAction::UiPitchUp => set.ui_pitch_semitones += 1,
            ButtonAction::FovDown(p) => set.set_fov_of(p, set.fov_of(p) - FOV_STEP),
            ButtonAction::SkipPerspective(p) => {
                match p {
                    Perspective::FirstPerson => {}
                    Perspective::ThirdPersonBack => set.skip_third_back = !set.skip_third_back,
                    Perspective::ThirdPersonFront => set.skip_third_front = !set.skip_third_front,
                }
                // Steht man gerade in der jetzt uebersprungenen Ansicht, zurueck ins Auge.
                if set.perspective_skipped(self.perspective) {
                    self.perspective = Perspective::FirstPerson;
                }
            }
            ButtonAction::FovUp(p) => set.set_fov_of(p, set.fov_of(p) + FOV_STEP),
            ButtonAction::CameraDistanceDown(p) => set.set_camera_distance_of(p, set.camera_distance_of(p) - CAMERA_DISTANCE_STEP),
            ButtonAction::CameraDistanceUp(p) => set.set_camera_distance_of(p, set.camera_distance_of(p) + CAMERA_DISTANCE_STEP),
            ButtonAction::CameraYOffsetDown(p) => set.set_camera_y_offset_of(p, set.camera_y_offset_of(p) - CAMERA_Y_OFFSET_STEP),
            ButtonAction::CameraYOffsetUp(p) => set.set_camera_y_offset_of(p, set.camera_y_offset_of(p) + CAMERA_Y_OFFSET_STEP),
            ButtonAction::CycleProjection => set.projection_mode = (set.projection_mode + 1) % PROJECTION_NAMES.len() as u8,
            ButtonAction::CycleTearPosition => set.tear_position = u8::from(set.tear_position == 0),
            ButtonAction::CycleRenderPath => set.render_path = (set.render_path + 1) % RENDER_PATH_NAMES.len() as u8,
            ButtonAction::TearTimingDown | ButtonAction::TearTimingUp => {
                let delta = if action == ButtonAction::TearTimingDown { -TEAR_TIMING_STEP_US } else { TEAR_TIMING_STEP_US };
                set.tear_timing_us = (set.tear_timing_us + delta).clamp(TEAR_TIMING_MIN_US, TEAR_TIMING_MAX_US);
            }
            ButtonAction::TearReserveDown | ButtonAction::TearReserveUp => {
                let delta = if action == ButtonAction::TearReserveDown { -TEAR_RESERVE_STEP_US } else { TEAR_RESERVE_STEP_US };
                set.tear_reserve_us = (set.tear_reserve_us + delta).clamp(TEAR_RESERVE_MIN_US, TEAR_RESERVE_MAX_US);
            }
            ButtonAction::CycleParticles => set.particles_mode = (set.particles_mode + 1) % PARTICLE_MODE_NAMES.len() as u8,
            ButtonAction::CycleDeathAnimation => set.death_animation = (set.death_animation + 1) % DEATH_ANIM_NAMES.len() as u8,
            ButtonAction::ToggleStableNumbers => {
                set.stable_numbers = !set.stable_numbers;
                if let Some(gpu) = self.gpu.as_ref() {
                    let mut zahlen = gpu.stable_numbers.borrow_mut();
                    zahlen.clear();
                    zahlen.set_enabled(set.stable_numbers);
                }
            }
            // Das Laden danach, unten -- es braucht ganz self.
            ButtonAction::CycleWallpaper => set.wallpaper = (set.wallpaper + 1) % wallpaper::WallpaperMode::ALL.len() as u8,
            ButtonAction::CycleLanguage(row) => {
                let bereiche = [&mut set.lang_ui, &mut set.lang_items, &mut set.lang_effects, &mut set.lang_chat];
                if row == 0 {
                    // Gemeinsam weiter: von der Sprache der Oberflaeche aus.
                    let neu = i18n::Language::from_index(*bereiche[0]).next().index();
                    for b in bereiche {
                        *b = neu;
                    }
                } else if let Some(b) = bereiche.into_iter().nth(row - 1) {
                    *b = i18n::Language::from_index(*b).next().index();
                }
            }
            ButtonAction::PickWallpaper => {
                if let Some(ziel) = shared_config_dir().map(|d| wallpaper::stored_path(&d)) {
                    self.wallpaper_loader.pick(ziel);
                }
            }
            ButtonAction::ToggleModuleCritColor => set.module_crit_color_enabled = !set.module_crit_color_enabled,
            ButtonAction::ToggleDamageTintSkinOnly => set.damage_tint_skin_only = !set.damage_tint_skin_only,
            ButtonAction::DamageTintTicksPrev => set.damage_tint_ticks = set.damage_tint_ticks.saturating_sub(1).max(DAMAGE_TINT_TICKS_MIN),
            ButtonAction::DamageTintTicksNext => set.damage_tint_ticks = (set.damage_tint_ticks + 1).min(DAMAGE_TINT_TICKS_MAX),
            ButtonAction::MeshBudgetDown => set.mesh_columns_per_frame = set.mesh_columns_per_frame.saturating_sub(1).max(1),
            ButtonAction::MeshBudgetUp => set.mesh_columns_per_frame = (set.mesh_columns_per_frame + 1).min(MESH_COLUMNS_MAX),
            // Fernsicht im Video-Tab (s. lod.rs).
            ButtonAction::ChunkSaverThresholdPrev
            | ButtonAction::ChunkSaverThresholdNext
            | ButtonAction::ChunkSaverDistancePrev
            | ButtonAction::ChunkSaverDistanceNext => lod::apply_action(set, action),
            ButtonAction::MaxRenderDistanceDown => {
                set.max_render_distance = set.max_render_distance.saturating_sub(1).max(MAX_RENDER_DISTANCE_MIN)
            }
            ButtonAction::MaxRenderDistanceUp => set.max_render_distance = (set.max_render_distance + 1).min(MAX_RENDER_DISTANCE_MAX),
            ButtonAction::ChunkUpdateThreadsDown | ButtonAction::ChunkUpdateThreadsUp => {
                set.chunk_update_threads = if action == ButtonAction::ChunkUpdateThreadsDown {
                    set.chunk_update_threads.saturating_sub(1).max(CHUNK_UPDATE_THREADS_MIN)
                } else {
                    (set.chunk_update_threads + 1).min(CHUNK_UPDATE_THREADS_MAX)
                };
                self.mesh_pool.resize(set.chunk_update_threads);
            }
            // In 5er-Schritten -- feiner braucht es niemand, gezielt tippen geht ueber das
            // Wertfeld (s. EditableValue::InventoryTransparency).
            ButtonAction::MasterVolumeDown => set.master_volume = (set.master_volume - 5.0).max(0.0),
            ButtonAction::MasterVolumeUp => set.master_volume = (set.master_volume + 5.0).min(100.0),
            ButtonAction::CategoryVolumeDown(i) => set.category_volume[i] = (set.category_volume[i] - 5.0).max(0.0),
            ButtonAction::CategoryVolumeUp(i) => set.category_volume[i] = (set.category_volume[i] + 5.0).min(100.0),
            ButtonAction::UiSoundVolumeDown(i) => set.ui_sound_volume[i] = (set.ui_sound_volume[i] - 5.0).max(0.0),
            ButtonAction::UiSoundVolumeUp(i) => set.ui_sound_volume[i] = (set.ui_sound_volume[i] + 5.0).min(100.0),
            ButtonAction::InventoryTransparencyPrev => set.inventory_transparency = set.inventory_transparency.saturating_sub(5),
            ButtonAction::InventoryTransparencyNext => set.inventory_transparency = (set.inventory_transparency + 5).min(100),
            ButtonAction::GlintSpeedPrev => set.glint_speed = set.glint_speed.saturating_sub(5),
            ButtonAction::GlintSpeedNext => set.glint_speed = (set.glint_speed + 5).min(100),
            ButtonAction::GlintStrengthPrev => set.glint_strength = set.glint_strength.saturating_sub(5),
            ButtonAction::GlintStrengthNext => set.glint_strength = (set.glint_strength + 5).min(100),
            ButtonAction::PlayerListScalePrev => set.player_list_scale = set.player_list_scale.saturating_sub(HUD_SCALE_PCT_STEP).max(HUD_SCALE_PCT_MIN),
            ButtonAction::PlayerListScaleNext => set.player_list_scale = (set.player_list_scale + HUD_SCALE_PCT_STEP).min(HUD_SCALE_PCT_MAX),
            ButtonAction::MenuBackdropPrev => set.menu_backdrop_opacity = set.menu_backdrop_opacity.saturating_sub(5),
            ButtonAction::MenuBackdropNext => set.menu_backdrop_opacity = (set.menu_backdrop_opacity + 5).min(100),
            ButtonAction::ScrollInertiaPrev => set.scroll_inertia = set.scroll_inertia.saturating_sub(5),
            ButtonAction::ScrollInertiaNext => set.scroll_inertia = (set.scroll_inertia + 5).min(100),
            ButtonAction::ScrollDistancePrev => set.scroll_distance = set.scroll_distance.saturating_sub(SCROLL_DISTANCE_STEP).max(SCROLL_DISTANCE_MIN),
            ButtonAction::ScrollDistanceNext => set.scroll_distance = (set.scroll_distance + SCROLL_DISTANCE_STEP).min(SCROLL_DISTANCE_MAX),
            // Auf das Raster einrasten: ein eingetippter Wert wie 100 geht zu 96 bzw. 112.
            ButtonAction::EntityDistancePrev => {
                set.entity_render_distance =
                    ((set.entity_render_distance - 1) / ENTITY_DISTANCE_STEP * ENTITY_DISTANCE_STEP).max(ENTITY_DISTANCE_MIN)
            }
            ButtonAction::EntityDistanceNext => {
                set.entity_render_distance =
                    ((set.entity_render_distance / ENTITY_DISTANCE_STEP + 1) * ENTITY_DISTANCE_STEP).min(ENTITY_DISTANCE_MAX)
            }
            ButtonAction::BlockEntityDistancePrev => {
                set.block_entity_render_distance = ((set.block_entity_render_distance.max(1) - 1) / BLOCK_ENTITY_DISTANCE_STEP * BLOCK_ENTITY_DISTANCE_STEP)
                    .max(BLOCK_ENTITY_DISTANCE_MIN)
            }
            ButtonAction::BlockEntityDistanceNext => {
                set.block_entity_render_distance = ((set.block_entity_render_distance / BLOCK_ENTITY_DISTANCE_STEP + 1) * BLOCK_ENTITY_DISTANCE_STEP)
                    .min(BLOCK_ENTITY_DISTANCE_MAX)
            }
            ButtonAction::InventoryScalePrev => set.inventory_scale = set.inventory_scale.saturating_sub(1),
            ButtonAction::InventoryScaleNext => set.inventory_scale = (set.inventory_scale + 1).min(max_scale),
            ButtonAction::ToggleInventoryCursorToSlot => set.inventory_cursor_to_slot = !set.inventory_cursor_to_slot,
            ButtonAction::ToggleCreativeOpTab => set.creative_op_tab = !set.creative_op_tab,
            ButtonAction::NoUiClose(m) => set.no_ui_close = m,
            ButtonAction::ToggleNoChatClear => set.no_chat_clear = !set.no_chat_clear,
            ButtonAction::ToggleAudioAutoOptimize => set.audio_auto_optimize = !set.audio_auto_optimize,
            ButtonAction::ToggleSignEditInWorld => set.sign_edit_in_world = !set.sign_edit_in_world,
            ButtonAction::NoSlotSet(m) => set.no_slot_set = m,
            ButtonAction::NoRotateSet(m) => set.no_rotate_set = m,
            ButtonAction::ToggleInventoryMergeHover => set.inventory_merge_hover = !set.inventory_merge_hover,
            ButtonAction::TogglePotionMarkStrong => set.potion_mark_strong = !set.potion_mark_strong,
            ButtonAction::TogglePotionMarkLong => set.potion_mark_long = !set.potion_mark_long,
            ButtonAction::TogglePotionMarkWeak => set.potion_mark_weak = !set.potion_mark_weak,
            ButtonAction::ScatteringPrev => set.shader_scattering_percent = set.shader_scattering_percent.saturating_sub(SCATTERING_STEP),
            ButtonAction::ScatteringNext => set.shader_scattering_percent = (set.shader_scattering_percent + SCATTERING_STEP).min(SCATTERING_MAX),
            ButtonAction::SunSizePrev => set.shader_sun_size_tenths = set.shader_sun_size_tenths.saturating_sub(SUN_SIZE_STEP_TENTHS),
            ButtonAction::SunSizeNext => set.shader_sun_size_tenths = (set.shader_sun_size_tenths + SUN_SIZE_STEP_TENTHS).min(SUN_SIZE_MAX_TENTHS),
            ButtonAction::ToggleBlockShadows => set.shader_block_shadows = !set.shader_block_shadows,
            ButtonAction::LightmapDebugPrev => set.shader_lightmap_debug = set.shader_lightmap_debug.saturating_sub(1),
            ButtonAction::LightmapDebugNext => set.shader_lightmap_debug = (set.shader_lightmap_debug + 1).min(LIGHTMAP_DEBUG_NAMES.len() as u8 - 1),
            ButtonAction::OpenLightLevels => {
                self.screen = MenuScreen::SettingsLightLevels;
                self.light_search.clear();
                self.light_scroll = Default::default();
            }
            ButtonAction::LightFactorPrev(i) | ButtonAction::LightFactorNext(i) => {
                if let Some(name) = light_source_blocks().get(i) {
                    let jetzt = set.light_factor_of(name);
                    let neu = if matches!(action, ButtonAction::LightFactorNext(_)) {
                        (jetzt + light_factor_step(jetzt, true)).min(LIGHT_FACTOR_MAX)
                    } else {
                        jetzt.saturating_sub(light_factor_step(jetzt, false))
                    };
                    set.set_light_factor(name, neu);
                }
            }
            ButtonAction::BlockSoftnessPrev => set.shader_block_softness_deg = set.shader_block_softness_deg.saturating_sub(BLOCK_SOFTNESS_STEP_DEG),
            ButtonAction::BlockSoftnessNext => {
                set.shader_block_softness_deg = (set.shader_block_softness_deg + BLOCK_SOFTNESS_STEP_DEG).min(BLOCK_SOFTNESS_MAX_DEG)
            }
            ButtonAction::HotbarScalePrev => set.hud_hotbar_scale = set.hud_hotbar_scale.saturating_sub(1),
            ButtonAction::HotbarScaleNext => set.hud_hotbar_scale = (set.hud_hotbar_scale + 1).min(max_scale),
            ButtonAction::HotbarDurabilityPrev if set.hotbar_durability_absolute => {
                let i = HOTBAR_DURABILITY_POINTS.iter().position(|&p| p == set.hotbar_durability_points).unwrap_or(0);
                set.hotbar_durability_points = HOTBAR_DURABILITY_POINTS[i.saturating_sub(1)];
            }
            ButtonAction::HotbarDurabilityNext if set.hotbar_durability_absolute => {
                let i = HOTBAR_DURABILITY_POINTS.iter().position(|&p| p == set.hotbar_durability_points).unwrap_or(0);
                set.hotbar_durability_points = HOTBAR_DURABILITY_POINTS[(i + 1).min(HOTBAR_DURABILITY_POINTS.len() - 1)];
            }
            ButtonAction::HotbarDurabilityPrev => {
                set.hotbar_durability_percent = set.hotbar_durability_percent.saturating_sub(HOTBAR_DURABILITY_STEP).max(HOTBAR_DURABILITY_STEP)
            }
            ButtonAction::HotbarDurabilityNext => set.hotbar_durability_percent = (set.hotbar_durability_percent + HOTBAR_DURABILITY_STEP).min(100),
            ButtonAction::ToggleHotbarDurabilityMode => set.hotbar_durability_absolute = !set.hotbar_durability_absolute,
            ButtonAction::ToggleHotbarAirBar => set.hotbar_air_bar = !set.hotbar_air_bar,
            ButtonAction::EffectsDetailed(an) => set.effects_detailed = an,
            ButtonAction::ToggleEffectsIcon => set.effects_icon = !set.effects_icon,
            ButtonAction::ToggleEffectsHideAmbient => set.effects_hide_ambient = !set.effects_hide_ambient,
            ButtonAction::ToggleEffectsName => set.effects_name = !set.effects_name,
            ButtonAction::EffectsTimerBlink(m) => set.effects_timer_blink = m,
            ButtonAction::EffectsNameColor(an) => set.effects_name_color = an,
            ButtonAction::EffectsTimerStyle(i) => set.effects_timer_style = i,
            ButtonAction::EffectsTimerColor(an) => set.effects_timer_color = an,
            ButtonAction::ChatScalePrev => set.chat_scale_percent = set.chat_scale_percent.saturating_sub(CHAT_SCALE_STEP).max(CHAT_SCALE_MIN),
            ButtonAction::ChatScaleNext => set.chat_scale_percent = (set.chat_scale_percent + CHAT_SCALE_STEP).min(CHAT_SCALE_MAX),
            ButtonAction::ChatWidthPrev => set.chat_width = set.chat_width.saturating_sub(CHAT_WIDTH_STEP).max(CHAT_WIDTH_MIN),
            ButtonAction::ChatWidthNext => set.chat_width = (set.chat_width + CHAT_WIDTH_STEP).min(CHAT_WIDTH_MAX),
            ButtonAction::ChatLinesPrev => set.chat_lines = set.chat_lines.saturating_sub(1).max(1),
            ButtonAction::ChatLinesNext => set.chat_lines = (set.chat_lines + 1).min(CHAT_LINES_MAX),
            ButtonAction::ChatLinesOpenPrev => set.chat_lines_open = set.chat_lines_open.saturating_sub(1).max(1),
            ButtonAction::ChatLinesOpenNext => set.chat_lines_open = (set.chat_lines_open + 1).min(CHAT_LINES_OPEN_MAX),
            ButtonAction::ChatBackgroundPrev => set.chat_background = set.chat_background.saturating_sub(10),
            ButtonAction::ChatBackgroundNext => set.chat_background = (set.chat_background + 10).min(100),
            ButtonAction::ChatOpenDimPrev => set.chat_open_dim = set.chat_open_dim.saturating_sub(10),
            ButtonAction::ChatOpenDimNext => set.chat_open_dim = (set.chat_open_dim + 10).min(100),
            ButtonAction::ChatFadePrev => set.chat_fade_secs = chat_fade_step(set.chat_fade_secs, -1),
            ButtonAction::ChatFadeNext => set.chat_fade_secs = chat_fade_step(set.chat_fade_secs, 1),
            ButtonAction::ChatLineSpacingPrev => set.chat_line_spacing = set.chat_line_spacing.saturating_sub(CHAT_LINE_SPACING_STEP),
            ButtonAction::ChatLineSpacingNext => set.chat_line_spacing = (set.chat_line_spacing + CHAT_LINE_SPACING_STEP).min(CHAT_LINE_SPACING_MAX),
            ButtonAction::ChatStyle(b) => set.chat_bedrock = b,
            ButtonAction::ToggleCrosshairOutline => set.crosshair_outline = !set.crosshair_outline,
            ButtonAction::ToggleCrosshairRingOutline => set.crosshair_ring_outline = !set.crosshair_ring_outline,
            ButtonAction::CrosshairSizePrev => set.step_crosshair_size(-1),
            ButtonAction::CrosshairSizeNext => set.step_crosshair_size(1),
            ButtonAction::OpenCrosshairPainter => {
                set.use_custom_crosshair();
                self.crosshair_painter = Some(CrosshairPainter::default());
            }
            ButtonAction::CrosshairPainterDone => self.crosshair_painter = None,
            ButtonAction::CrosshairPainterClear => set.crosshair_pattern = crosshair_editor::Pattern::empty(),
            ButtonAction::CrosshairPainterStandard => set.crosshair_pattern = crosshair_editor::Pattern::standard(set.crosshair_outline),
            ButtonAction::CrosshairPainterSymmetry => {
                if let Some(p) = self.crosshair_painter.as_mut() {
                    p.symmetry = p.symmetry.next();
                }
            }
            ButtonAction::ToggleEffectsHud => set.show_effects_hud = !set.show_effects_hud,
            ButtonAction::OpenParticleSettings => {
                self.screen = MenuScreen::SettingsParticles;
                self.particle_scroll = Default::default();
            }
            ButtonAction::ParticleToggle(i) => {
                if let Some(t) = set.particle_tuning.get_mut(i) {
                    t.enabled = !t.enabled;
                }
            }
            ButtonAction::ParticleAllAtOnce(i) => {
                if let Some(t) = set.particle_tuning.get_mut(i) {
                    t.all_at_once = !t.all_at_once;
                }
            }
            ButtonAction::ParticleStep(i, col, hoch) => {
                if let Some(t) = set.particle_tuning.get_mut(i) {
                    match col {
                        0 => t.lifetime_pct = step_in(&PARTICLE_LIFETIME_STEPS, t.lifetime_pct, hoch),
                        1 => {
                            t.opacity_pct =
                                if hoch { (t.opacity_pct + PARTICLE_OPACITY_STEP).min(100) } else { t.opacity_pct.saturating_sub(PARTICLE_OPACITY_STEP).max(PARTICLE_OPACITY_STEP) }
                        }
                        2 => t.speed_pct = step_in(&PARTICLE_SPEED_STEPS, t.speed_pct, hoch),
                        // Die vierte Spalte ist Menge oder Intervall, je nach Art.
                        _ if particles::KINDS[i].spawn.has_interval() => t.interval_pct = step_in(&PARTICLE_INTERVAL_STEPS, t.interval_pct, hoch),
                        _ => t.amount_pct = step_in(&PARTICLE_AMOUNT_STEPS, t.amount_pct, hoch),
                    }
                }
            }
            ButtonAction::ToggleCaveCulling => set.cave_culling = !set.cave_culling,
            ButtonAction::ToggleMipmapping => set.mipmapping = !set.mipmapping,
            ButtonAction::ToggleLatinMacrons => set.latin_macrons = !set.latin_macrons,
            ButtonAction::ToggleLoanwords => set.english_loanwords = !set.english_loanwords,
            ButtonAction::ToggleEffectsInventory => set.show_effects_inventory = !set.show_effects_inventory,
            ButtonAction::CrosshairBoostPrev => set.crosshair_brightness_boost = set.crosshair_brightness_boost.saturating_sub(5),
            ButtonAction::CrosshairBoostNext => set.crosshair_brightness_boost = (set.crosshair_brightness_boost + 5).min(100),
            ButtonAction::CrosshairColorPrev => set.crosshair_color_mode = set.crosshair_color_mode.saturating_sub(1),
            ButtonAction::CrosshairColorNext => {
                set.crosshair_color_mode = (set.crosshair_color_mode + 1).min(CROSSHAIR_COLOR_MODES.len() - 1)
            }
            ButtonAction::AudioTestTone => self.audio.play_test_tone(),
            ButtonAction::PipewireLimit(frames) => {
                let rate = self.pipewire.messung().map_or(48_000, |m| m.rate);
                self.pipewire.grenze_setzen(frames, rate);
            }
            ButtonAction::AudioSpatialTest => {
                // Uhr mit zuruecksetzen, sonst zaehlt die Zeit seit dem letzten Frame als
                // schon vergangene Testzeit und der erste Klick kaeme verschoben.
                self.last_orbit_update = Instant::now();
                self.orbit_test = Some(spatial_sound::OrbitTest::start());
            }
            ButtonAction::HudAlign(element, align) => {
                // Die Ausrichtung wechselt, das Element soll aber stehen bleiben: der Anker
                // wird aus der bisherigen linken Kante neu bestimmt.
                if let Some(gpu) = self.gpu.as_ref() {
                    let r = self.hud_element_rect(gpu, element);
                    let sw = gpu.size().0;
                    let anker = (align_anchor(r.x, r.w, align) / sw).clamp(0.0, 1.0);
                    match element {
                        HudElement::Title => {
                            self.settings.hud_title_align = align;
                            self.settings.hud_title_x = anker;
                        }
                        HudElement::Subtitle => {
                            self.settings.hud_subtitle_align = align;
                            self.settings.hud_subtitle_x = anker;
                        }
                        HudElement::ActionBar => {
                            self.settings.hud_actionbar_align = align;
                            self.settings.hud_actionbar_x = anker;
                        }
                        _ => {}
                    }
                }
            }
            ButtonAction::EffectsAlign(align) => {
                // Wie bei HudAlign bleibt die Anzeige stehen: die Anker kommen aus dem
                // bisherigen Rechteck, nur der feste Rand wechselt.
                if let Some(gpu) = self.gpu.as_ref() {
                    let r = self.hud_effects_rect(gpu, true);
                    let (sw, sh) = gpu.size();
                    let (x, y) = effects_detail_anchor(r, align);
                    self.settings.effects_align = align;
                    self.settings.hud_effects_detail_x = (x / sw).clamp(0.0, 1.0);
                    self.settings.hud_effects_detail_y = (y / sh).clamp(0.0, 1.0);
                }
            }
            ButtonAction::HudDeselect => self.hud_selected = None,
            ButtonAction::ArmorHudLayout(l) => self.settings.armor_hud_layout = l,
            ButtonAction::ArmorHudGapPrev => self.settings.armor_hud_gap = (self.settings.armor_hud_gap - 1).max(armor_hud::GAP_MIN),
            ButtonAction::ArmorHudGapNext => self.settings.armor_hud_gap = (self.settings.armor_hud_gap + 1).min(armor_hud::GAP_MAX),
            ButtonAction::ArmorHudNumberScalePrev | ButtonAction::ArmorHudNumberScaleNext => {
                let wert = &mut self.settings.armor_hud_number_scale;
                *wert = if action == ButtonAction::ArmorHudNumberScaleNext { *wert + armor_hud::NUMBER_SCALE_STEP } else { wert.saturating_sub(armor_hud::NUMBER_SCALE_STEP) }
                    .clamp(armor_hud::NUMBER_SCALE_MIN, armor_hud::NUMBER_SCALE_MAX);
            }
            ButtonAction::ArmorHudNumberXPrev | ButtonAction::ArmorHudNumberXNext | ButtonAction::ArmorHudNumberYPrev | ButtonAction::ArmorHudNumberYNext => {
                let set = &mut self.settings;
                let (wert, schritt) = match action {
                    ButtonAction::ArmorHudNumberXPrev => (&mut set.armor_hud_number_x, -1),
                    ButtonAction::ArmorHudNumberXNext => (&mut set.armor_hud_number_x, 1),
                    ButtonAction::ArmorHudNumberYPrev => (&mut set.armor_hud_number_y, -1),
                    _ => (&mut set.armor_hud_number_y, 1),
                };
                *wert = (*wert + schritt).clamp(armor_hud::NUMBER_OFFSET_MIN, armor_hud::NUMBER_OFFSET_MAX);
            }
            ButtonAction::ToggleHurtCam => self.settings.hurt_cam = !self.settings.hurt_cam,
            ButtonAction::ToggleArmorTrims => self.settings.armor_trims = !self.settings.armor_trims,
            ButtonAction::HurtCamStrengthPrev => self.settings.hurt_cam_strength = self.settings.hurt_cam_strength.saturating_sub(5),
            ButtonAction::HurtCamStrengthNext => self.settings.hurt_cam_strength = (self.settings.hurt_cam_strength + 5).min(100),
            ButtonAction::FireMode(m) => self.settings.fire_mode = m,
            ButtonAction::FireHeightPrev => self.settings.fire_height = self.settings.fire_height.saturating_sub(5),
            ButtonAction::FireHeightNext => self.settings.fire_height = (self.settings.fire_height + 5).min(100),
            ButtonAction::KeysOpacityPrev => self.settings.keys_opacity = self.settings.keys_opacity.saturating_sub(10),
            ButtonAction::KeysOpacityNext => self.settings.keys_opacity = (self.settings.keys_opacity + 10).min(100),
            ButtonAction::SessionOpacity(mehr) => {
                let o = &mut self.settings.session_opacity;
                *o = if mehr { (*o + 10).min(100) } else { o.saturating_sub(10) };
            }
            ButtonAction::ArmorHudSort(r) => {
                if armor_hud::is_vertical(self.settings.armor_hud_layout) {
                    self.settings.armor_hud_sort_vertical = r;
                } else {
                    self.settings.armor_hud_sort_flat = r;
                }
            }
            ButtonAction::ToggleArmorHudGreen => self.settings.armor_hud_green = !self.settings.armor_hud_green,
            // Aussehen von Schild-Cooldown und Pfeil-Anzeige: im Fenster des HUD-Editors, die
            // Werte gehoeren aber zu den Modulen.
            ButtonAction::ToggleArrowNumberOnly
            | ButtonAction::ArrowOpacityPrev
            | ButtonAction::ArrowOpacityNext
            | ButtonAction::CycleArrowTextStyle
            | ButtonAction::CycleShieldCdTextStyle
            | ButtonAction::CycleShieldSelfMode
            | ButtonAction::OpenHudConfig(_) => self.apply_module_action(action),
            ButtonAction::HudScaleStep(element, groesser) => {
                let wert = match element {
                    HudElement::Title => &mut self.settings.hud_title_scale,
                    HudElement::Subtitle => &mut self.settings.hud_subtitle_scale,
                    HudElement::Effects => &mut self.settings.hud_effects_scale,
                    HudElement::BossBar => &mut self.settings.hud_bossbar_scale,
                    HudElement::Scoreboard => &mut self.settings.hud_scoreboard_scale,
                    HudElement::Arrows => &mut self.settings.hud_arrows_scale,
                    HudElement::HeldName => &mut self.settings.hud_held_name_scale,
                    HudElement::Armor => &mut self.settings.hud_armor_scale,
                    HudElement::Keystrokes => &mut self.settings.hud_keys_scale,
                    HudElement::Session => &mut self.settings.hud_session_scale,
                    _ => return,
                };
                *wert = if groesser { *wert + HUD_SCALE_PCT_STEP } else { wert.saturating_sub(HUD_SCALE_PCT_STEP) }
                    .clamp(HUD_SCALE_PCT_MIN, HUD_SCALE_PCT_MAX);
            }
            ButtonAction::ResetHudElement => {
                if let Some(element) = self.hud_selected {
                    self.reset_hud_element(element);
                }
            }
            // Expliziter, vom Nutzer gedrueckter Schalter -- keine Automatik, die im
            // Hintergrund an einer globalen Anzeigeeinstellung dreht (s. latency_tuning.rs).
            ButtonAction::ToggleTearFree => {
                let want_on = !self.tearfree_state.unwrap_or(false);
                match latency_tuning::set_tearfree(want_on) {
                    // Real abfragen statt den gewuenschten Wert einfach zu uebernehmen --
                    // xrandr kann trotz Erfolgsmeldung etwas anderes gesetzt haben, als wir
                    // dachten (s. die ganze TearFree-Untersuchung diese Sitzung).
                    Ok(()) => {
                        self.tearfree_state = latency_tuning::tearfree_enabled();
                        // Merken, was der Nutzer will, solange der Client laeuft -- das ist
                        // getrennt vom Systemwert, der bei Fokusverlust/Beenden wieder auf
                        // tearfree_original_value zurueckspringt.
                        self.tearfree_user_choice = self.tearfree_state;
                    }
                    Err(e) => self.push_log(format!("TearFree konnte nicht gesetzt werden: {e}")),
                }
            }
            ButtonAction::ToggleBlockRngRotation => set.block_rng_rotation = !set.block_rng_rotation,
            ButtonAction::SmoothLightingDown => set.smooth_lighting_percent = set.smooth_lighting_percent.saturating_sub(SMOOTH_LIGHTING_STEP),
            ButtonAction::SmoothLightingUp => {
                set.smooth_lighting_percent = (set.smooth_lighting_percent + SMOOTH_LIGHTING_STEP).min(SMOOTH_LIGHTING_MAX)
            }
            ButtonAction::ToggleJumpCooldown => set.jump_cooldown_enabled = !set.jump_cooldown_enabled,
            ButtonAction::ToggleSprintOnStart => set.sprint_on_start = !set.sprint_on_start,
            ButtonAction::ToggleCapsLockBlocked => set.caps_lock_blocked = !set.caps_lock_blocked,
            ButtonAction::ToggleSnapTap => set.snap_tap_enabled = !set.snap_tap_enabled,
            ButtonAction::ToggleUseWhileMining => set.use_while_mining = !set.use_while_mining,
            ButtonAction::ToggleRocketRepeat => set.hold_use_repeat_rockets = !set.hold_use_repeat_rockets,
            ButtonAction::ToggleExtinguishDcGhost => set.no_extinguish_dc_ghost = !set.no_extinguish_dc_ghost,
            ButtonAction::CycleHoldUseRepeat => set.hold_use_repeat = set.hold_use_repeat.next(),
            ButtonAction::ToggleSneakEdgeProtection => set.sneak_edge_protection_enabled = !set.sneak_edge_protection_enabled,
            // Wartet auf den naechsten Tastendruck -- ausgewertet in window_event(), nicht
            // hier: ein Mausklick ist kein Tastendruck, wir muessen den ERST NOCH abwarten.
            ButtonAction::Rebind(target, slot) => self.awaiting_keybind = Some((target, slot)),
            ButtonAction::OpenKeybindAdvanced(aktion) => {
                self.awaiting_keybind = None;
                self.keybind_advanced = Some(aktion);
            }
            ButtonAction::CloseKeybindAdvanced => {
                self.awaiting_keybind = None;
                self.keybind_advanced = None;
            }
            ButtonAction::ToggleKeybindMode(action) => {
                let mode = set.keybind_modes.get(action).toggled();
                set.keybind_modes.set(action, mode);
            }
            // Respawn anfordern. Optimistisch zurueck in die Welt -- der Server bestaetigt
            // mit neuem Leben (Set Health) + Position; kommt es wider Erwarten nicht,
            // bliebe man tot und koennte erneut klicken.
            ButtonAction::Respawn => {
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::perform_respawn_payload());
                }
                self.is_dead = false;
                self.mode = AppMode::World;
                self.screen = MenuScreen::Main;
                self.restore_held_controls();
            }
            // Tatsaechlich behandelt NACH dem match (braucht &mut self).
            ButtonAction::OpenSkinFolder | ButtonAction::ReloadSkins | ButtonAction::OpenConfigFolder | ButtonAction::ServerLinks => {}
            ButtonAction::ToggleSkinAutoUnzip => set.skin_auto_unzip = !set.skin_auto_unzip,
        }

        if action == ButtonAction::OpenSkinFolder {
            self.open_skins_dir();
        }
        if action == ButtonAction::ReloadSkins {
            self.reload_resource_pack(self.selected_skins.clone());
        }
        if action == ButtonAction::OpenConfigFolder {
            self.open_configs_dir();
        }
        if action == ButtonAction::ServerLinks {
            self.open_server_links();
        }
        if action == ButtonAction::CycleWallpaper {
            self.request_wallpaper(true);
        }
        // Der Shader-Pfad meshet mit Licht, der PvP-Pfad ohne: alles neu.
        if action == ButtonAction::CycleRenderPath {
            self.world.light_remesh = self.settings.render_path == 1;
            world::mesher::set_flat_face_shade(self.settings.render_path == 1);
            for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                self.world.mark_dirty(cx, cz);
            }
        }
        if let ButtonAction::LightFactorPrev(i) | ButtonAction::LightFactorNext(i) = action {
            // Sonnenlicht liest der Shader-Pfad je Frame, dafuer muss nichts neu entstehen.
            if light_source_blocks().get(i) == Some(&world::light::SUNLIGHT) {
                world::light::set_light_factors(self.settings.light_factor_table());
            } else {
                self.apply_light_factors();
            }
        }
        if matches!(action, ButtonAction::CycleLanguage(_) | ButtonAction::ToggleLatinMacrons | ButtonAction::ToggleLoanwords) {
            apply_language_settings(&mut self.installation, &self.settings);
        }

        if changes_settings {
            self.persist_settings();
        }

        // Block-Rotation wirkt sich nur beim (Neu-)Meshen aus (s. world::mesher) -- ohne
        // erzwungenen Neubau blieben schon geladene Spalten bis zum naechsten echten
        // Blockupdate/Neuladen bei der alten Ausrichtung.
        if action == ButtonAction::ToggleBlockRngRotation {
            self.block_uvs.set_rng_rotation_enabled(self.settings.block_rng_rotation);
            for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                self.world.mark_dirty(cx, cz);
            }
        }
        // Weiche Beleuchtung steckt wie die Block-Rotation in den Vertexfarben -- ohne
        // Neubau bliebe die schon geladene Welt bis zum naechsten Blockupdate beim alten Stand.
        if matches!(action, ButtonAction::SmoothLightingDown | ButtonAction::SmoothLightingUp) {
            for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                self.world.mark_dirty(cx, cz);
            }
        }
    }

    // Welches numerische Wertfeld (falls ueberhaupt) unter (px,py) liegt -- s.
    // EditableValue. Ein Screen kann mehrere haben (SettingsGeneral hat vier), deshalb wird
    // ueber alle gesucht statt pro Screen genau eines zurueckzugeben.
    pub(crate) fn editable_value_at(&self, px: f32, py: f32) -> Option<EditableValue> {
        EditableValue::ALL.into_iter().find(|ev| {
            ev.screen() == self.screen && self.editable_shown(*ev) && if ev.slider().is_some() { ev.slider_value_rect() } else { ev.rect() }.contains(px, py)
        })
    }

    // Steht das Wertfeld gerade da? Die Staerke eines Schatten-Kaestchens nur, wenn es gefuellt
    // ist, und je nach Art die Dicke (Kontur) oder die Deckkraft (alle ausser "Aus").
    pub(crate) fn editable_shown(&self, ev: EditableValue) -> bool {
        match ev {
            EditableValue::ShadowThickness(i) => self.settings.shadow_slot_style(i) == Some(TextShadow::Outline),
            EditableValue::ShadowOpacity(i) => {
                matches!(self.settings.shadow_slot_style(i), Some(TextShadow::Vanilla | TextShadow::Below | TextShadow::Around))
            }
            _ => true,
        }
    }

    // Freitext-Bearbeitung eines Zahlenfelds beginnen -- Startwert ist der aktuell
    // angezeigte (formatierte) Wert, damit man ihn bequem nachbearbeiten statt komplett neu
    // eintippen kann.
    pub(crate) fn begin_edit_value(&mut self, ev: EditableValue) {
        // Puffer startet LEER, nicht mit dem bisherigen Wert: fast immer will man eine neue
        // Zahl tippen, nicht die alte nachbearbeiten -- sonst muesste man sie erst
        // wegloeschen. Der bisherige Wert geht dabei nicht verloren: er steht weiter
        // abgeblendet im Feld (s. draw_editable_value), und ein leer bestaetigter Puffer
        // laesst ihn unveraendert stehen (commit_edit_value verwirft Unparsbares).
        self.editing_value = Some((ev, String::new()));
    }

    // Freitext-Eingabe in einem Regler ueber Pfeil-Aktionen beginnen -- leer wie bei
    // begin_edit_value; ein zweiter Klick mitten in der Eingabe setzt sie nicht zurueck.
    pub(crate) fn begin_step_edit(&mut self, prev: ButtonAction) {
        if self.editing_step.as_ref().map(|(p, _)| *p) != Some(prev) {
            self.editing_step = Some((prev, String::new()));
        }
    }

    // Puffer parsen, klemmen (dieselben Grenzen wie der jeweilige Stepper) und uebernehmen
    // -- ein nicht parsbarer Puffer (leer, nur ein Punkt, ...) wird stillschweigend
    // verworfen, der alte Wert bleibt stehen, statt auf 0 zu springen.
    pub(crate) fn commit_edit_value(&mut self) {
        let Some((ev, buf)) = self.editing_value.take() else {
            return;
        };
        self.apply_edit_buffer(ev, &buf);
        self.persist_settings();
    }

    // Eine Zahl (als Text in Anzeige-Einheiten) uebernehmen, mit denselben Grenzen wie der
    // jeweilige Stepper. Ohne Speichern -- das macht der Aufrufer (beim Ziehen am Regler erst
    // beim Loslassen).
    pub(crate) fn apply_edit_buffer(&mut self, ev: EditableValue, buf: &str) {
        // "-1" heisst bei jeder Obergrenze "unbegrenzt" (s. EditableValue::allows_unlimited).
        // Fuer Renderdistanz/Chunk-Anzahl ist das schlicht ihr jeweiliges Maximum: dort
        // greift der Limiter ohnehin nie, weil der Server nicht mehr schickt (s.
        // MAX_RENDER_DISTANCE_MAX) -- ein zusaetzlicher Sonderwert waere eine Unterscheidung
        // ohne Unterschied.
        let unlimited = ev.allows_unlimited() && buf.trim() == UNLIMITED_INPUT;
        match ev {
            EditableValue::MouseSensitivity => {
                // Puffer zeigt den *1000-skalierten Anzeigewert (s. begin_edit_value) --
                // zurueck in Radiant/Pixel umrechnen.
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.mouse_sensitivity = v / 1000.0;
                    self.settings.couple_sensitivity(false);
                    self.sens_last_changed_y = false;
                }
            }
            EditableValue::MouseSensitivityY => {
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.mouse_sensitivity_y = v / 1000.0;
                    self.settings.couple_sensitivity(true);
                    self.sens_last_changed_y = true;
                }
            }
            EditableValue::CursorSensitivity => {
                // Der Puffer zeigt den Faktor unskaliert (s. begin_edit_value) -- direkt
                // uebernehmen, kein 1000er-Faktor wie bei der Kamera.
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.cursor_sensitivity = v;
                }
            }
            EditableValue::MeshBudget => {
                if let Ok(v) = buf.parse::<usize>() {
                    self.settings.mesh_columns_per_frame = v;
                }
            }
            EditableValue::Fov(p) => {
                if let Ok(v) = buf.parse::<f32>() {
                    match p {
                        Perspective::FirstPerson => self.settings.fov = v,
                        Perspective::ThirdPersonBack => self.settings.fov_third_back = v,
                        Perspective::ThirdPersonFront => self.settings.fov_third_front = v,
                    }
                }
            }
            EditableValue::CameraDistance(p) => {
                if let Ok(v) = buf.parse::<f32>() {
                    match p {
                        Perspective::FirstPerson => {}
                        Perspective::ThirdPersonBack => self.settings.camera_distance_back = v,
                        Perspective::ThirdPersonFront => self.settings.camera_distance_front = v,
                    }
                }
            }
            EditableValue::CameraYOffset(p) => {
                if let Ok(v) = buf.parse::<f32>() {
                    match p {
                        Perspective::FirstPerson => {}
                        Perspective::ThirdPersonBack => self.settings.camera_y_offset_back = v,
                        Perspective::ThirdPersonFront => self.settings.camera_y_offset_front = v,
                    }
                }
            }
            EditableValue::FpsLimit => {
                if unlimited {
                    self.settings.fps_limit = FPS_LIMIT_UNLIMITED;
                } else if let Ok(v) = buf.parse::<u32>() {
                    self.settings.fps_limit = v;
                }
            }
            EditableValue::MaxRenderDistance => {
                if unlimited {
                    self.settings.max_render_distance = MAX_RENDER_DISTANCE_MAX;
                } else if let Ok(v) = buf.parse::<u32>() {
                    self.settings.max_render_distance = v;
                }
            }
            EditableValue::InventoryTransparency => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.inventory_transparency = v;
                }
            }
            EditableValue::GlintSpeed => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.glint_speed = v;
                }
            }
            EditableValue::GlintStrength => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.glint_strength = v;
                }
            }
            EditableValue::PlayerListScale => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.player_list_scale = v;
                }
            }
            EditableValue::MenuBackdropOpacity => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.menu_backdrop_opacity = v;
                }
            }
            EditableValue::ScrollInertia => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.scroll_inertia = v.min(100);
                }
            }
            EditableValue::ScrollDistance => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.scroll_distance = v.clamp(SCROLL_DISTANCE_MIN, SCROLL_DISTANCE_MAX);
                }
            }
            EditableValue::EntityRenderDistance => {
                if unlimited {
                    self.settings.entity_render_distance = ENTITY_DISTANCE_MAX;
                } else if let Ok(v) = buf.parse::<u32>() {
                    self.settings.entity_render_distance = v;
                }
            }
            EditableValue::BlockEntityRenderDistance => {
                if unlimited {
                    self.settings.block_entity_render_distance = BLOCK_ENTITY_DISTANCE_MAX;
                } else if let Ok(v) = buf.parse::<u32>() {
                    self.settings.block_entity_render_distance = v;
                }
            }
            EditableValue::CalcMinecraftSens => {
                if let Ok(v) = buf.parse::<f32>() {
                    self.settings.sens_calc_mc_percent = v;
                }
            }
            EditableValue::CalcDpi => {
                if let Ok(v) = buf.parse::<u32>() {
                    self.settings.sens_calc_dpi = v;
                }
            }
            EditableValue::MasterVolume => {
                if let Some(v) = parse_volume(buf) {
                    self.settings.master_volume = v;
                }
            }
            EditableValue::CategoryVolume(i) => {
                if let (Some(v), Some(feld)) = (parse_volume(buf), self.settings.category_volume.get_mut(i)) {
                    *feld = v;
                }
            }
            EditableValue::UiSoundVolume(i) => {
                if let (Some(v), Some(feld)) = (parse_volume(buf), self.settings.ui_sound_volume.get_mut(i)) {
                    *feld = v;
                }
            }
            EditableValue::ShadowThickness(i) => {
                if let (Ok(v), Some(feld)) = (buf.replace(',', ".").parse::<f32>(), self.settings.shadow_thickness.get_mut(i)) {
                    // Nur was ein u8 fassen kann.
                    *feld = (v * 10.0).round().clamp(0.0, u8::MAX as f32) as u8;
                }
                self.sync_text_shadow();
            }
            EditableValue::ShadowOpacity(i) => {
                if let (Ok(v), Some(feld)) = (buf.parse::<u32>(), self.settings.shadow_opacity.get_mut(i)) {
                    *feld = v.min(u8::MAX as u32) as u8;
                }
                self.sync_text_shadow();
            }
        }
    }

    // Aktueller Wert eines Zahlenfelds in Anzeige-Einheiten (wie apply_edit_buffer ihn liest).
    pub(crate) fn editable_number(&self, ev: EditableValue) -> f64 {
        let s = &self.settings;
        match ev {
            EditableValue::MouseSensitivity => s.mouse_sensitivity as f64 * 1000.0,
            EditableValue::MouseSensitivityY => s.mouse_sensitivity_y as f64 * 1000.0,
            EditableValue::CursorSensitivity => s.cursor_sensitivity as f64,
            EditableValue::MeshBudget => s.mesh_columns_per_frame as f64,
            EditableValue::Fov(p) => s.fov_of(p) as f64,
            EditableValue::CameraDistance(p) => s.camera_distance_of(p) as f64,
            EditableValue::CameraYOffset(p) => s.camera_y_offset_of(p) as f64,
            EditableValue::FpsLimit => if s.fps_limit == FPS_LIMIT_UNLIMITED { FPS_LIMIT_TYPED_MAX as f64 } else { s.fps_limit as f64 },
            EditableValue::MaxRenderDistance => s.max_render_distance as f64,
            EditableValue::InventoryTransparency => s.inventory_transparency as f64,
            EditableValue::GlintSpeed => s.glint_speed as f64,
            EditableValue::GlintStrength => s.glint_strength as f64,
            EditableValue::PlayerListScale => s.player_list_scale as f64,
            EditableValue::MenuBackdropOpacity => s.menu_backdrop_opacity as f64,
            EditableValue::ScrollInertia => s.scroll_inertia as f64,
            EditableValue::ScrollDistance => s.scroll_distance as f64,
            EditableValue::EntityRenderDistance => s.entity_render_distance as f64,
            EditableValue::BlockEntityRenderDistance => s.block_entity_render_distance as f64,
            EditableValue::CalcMinecraftSens => s.sens_calc_mc_percent as f64,
            EditableValue::CalcDpi => s.sens_calc_dpi as f64,
            EditableValue::MasterVolume => s.master_volume as f64,
            EditableValue::CategoryVolume(i) => s.category_volume.get(i).copied().unwrap_or(0.0) as f64,
            EditableValue::UiSoundVolume(i) => s.ui_sound_volume.get(i).copied().unwrap_or(0.0) as f64,
            EditableValue::ShadowThickness(i) => s.shadow_thickness.get(i).copied().unwrap_or(SHADOW_THICKNESS_DEFAULT) as f64 / 10.0,
            EditableValue::ShadowOpacity(i) => s.shadow_opacity.get(i).copied().unwrap_or(100) as f64,
        }
    }

    // Rechtes Ende eines Reglers: mit gehaltener rechter Taste das hoechste erlaubte, sonst das
    // normale -- aber nie unter dem aktuellen Wert (ein grosser Wert soll nicht am Rand kleben).
    pub(crate) fn slider_max(&self, ev: EditableValue) -> f64 {
        let Some(r) = ev.slider() else { return 1.0 };
        if self.slider_extended && self.slider_drag == Some(ev) {
            return r.hard;
        }
        r.soft.max(self.editable_number(ev).min(r.hard))
    }

    // Wert aus der Zeigerposition auf der Bahn setzen (Klick oder Ziehen).
    pub(crate) fn slider_set_from_pointer(&mut self, ev: EditableValue) {
        let Some(r) = ev.slider() else { return };
        let bahn = ev.slider_track();
        let t = ((self.pointer().0 - bahn.x) / bahn.w).clamp(0.0, 1.0) as f64;
        let max = self.slider_max(ev);
        let wert = r.min + t * (max - r.min);
        let text = format!("{wert:.*}", r.decimals);
        let vorher = self.editable_number(ev);
        self.apply_edit_buffer(ev, &text);
        let nachher = self.editable_number(ev);
        if nachher != vorher {
            self.play_slider_tick((nachher - r.min) / (max - r.min).max(1e-9));
        }
    }

    // Regler unter dem Zeiger (ganze Flaeche ausser der Wertanzeige).
    pub(crate) fn slider_at(&self, px: f32, py: f32) -> Option<EditableValue> {
        if self.mode != AppMode::Menu {
            return None;
        }
        EditableValue::ALL
            .into_iter()
            .filter(|ev| ev.screen() == self.screen && ev.slider().is_some() && self.editable_shown(*ev))
            .find(|ev| ev.slider_rect().contains(px, py) && !ev.slider_value_rect().contains(px, py))
    }

    // --- Schieberegler ueber Pfeil-Aktionen (s. StepSlider) ---

    pub(crate) fn step_slider_of(&self, prev: ButtonAction) -> Option<StepSlider> {
        let knoepfe = if self.module_menu_shown() {
            self.gpu.as_ref().map(|gpu| self.module_menu_buttons(gpu)).unwrap_or_default()
        } else {
            self.screen_buttons()
        };
        knoepfe.into_iter().filter_map(|b| b.slider).find(|s| s.prev == prev)
    }

    // Ein Schritt eines Schiebereglers: im Modul-Menue ueber dessen eigene Aktionen.
    pub(crate) fn run_step_action(&mut self, action: ButtonAction) {
        if self.module_menu_shown() {
            self.apply_module_action(action);
        } else {
            self.run_menu_action(action);
        }
    }

    // Den Wert unter den Zeiger bringen: Schritt fuer Schritt ueber die Pfeil-Aktionen, bis der
    // naechste Schritt weiter weg vom Ziel fuehren wuerde. Gespeichert wird erst beim
    // Loslassen (s. persist_paused).
    pub(crate) fn step_slider_drag(&mut self) {
        let Some(prev) = self.step_drag else { return };
        let Some(sl) = self.step_slider_of(prev) else { return };
        let bahn = sl.track();
        let t = ((self.pointer().0 - bahn.x) / bahn.w).clamp(0.0, 1.0) as f64;
        let ziel = sl.min + t * (sl.max - sl.min);
        self.persist_paused = true;
        let start = sl.value;
        // Ohne Schritte: der Wert unter dem Zeiger, ganzzahlige Felder gerundet.
        if self.set_step_value(prev, ziel) {
            self.persist_paused = false;
            if let Some(ende) = self.step_slider_of(prev).map(|s| s.value).filter(|&v| v != start) {
                self.play_slider_tick((ende - sl.min) / (sl.max - sl.min).max(1e-9));
            }
            return;
        }
        let mut wert = sl.value;
        for _ in 0..400 {
            let hoch = ziel > wert;
            if (ziel - wert).abs() < 1e-9 {
                break;
            }
            self.run_step_action(if hoch { sl.next } else { sl.prev });
            let Some(neu) = self.step_slider_of(prev).map(|s| s.value) else { break };
            let weiter = if hoch { neu > wert } else { neu < wert };
            if !weiter {
                break;
            }
            // Ueber das Ziel hinaus: zuruecknehmen, wenn der alte Wert naeher lag.
            if (hoch && neu > ziel) || (!hoch && neu < ziel) {
                if (neu - ziel).abs() > (wert - ziel).abs() {
                    self.run_step_action(if hoch { sl.prev } else { sl.next });
                }
                break;
            }
            wert = neu;
        }
        self.persist_paused = false;
        if let Some(ende) = self.step_slider_of(prev).map(|s| s.value).filter(|&v| v != start) {
            self.play_slider_tick((ende - sl.min) / (sl.max - sl.min).max(1e-9));
        }
    }

    // Regler ueber Pfeil-Aktionen unter dem Zeiger, sofern bedienbar.
    pub(crate) fn step_slider_at_pointer(&self) -> Option<StepSlider> {
        let knoepfe = if self.module_menu_shown() {
            self.gpu.as_ref().map(|gpu| self.module_menu_buttons(gpu)).unwrap_or_default()
        } else if self.mode == AppMode::Menu {
            self.screen_buttons()
        } else {
            return None;
        };
        let (px, py) = self.pointer();
        knoepfe.into_iter().filter(|b| b.enabled).filter_map(|b| b.slider).find(|s| s.rect().contains(px, py))
    }

    // Was im Feld eines Reglers ueber Pfeil-Aktionen steht: im Modul-Menue aus seinen Zeilen,
    // sonst aus dem Inhalt des Tabs, der den Wert ins alte Wertfeld schreibt (s. build_menu_ui).
    pub(crate) fn step_slider_text(&self, sl: &StepSlider) -> Option<String> {
        if self.module_menu_shown() {
            return self.module_detail_rows().into_iter().find_map(|r| match r {
                ModuleRow::Stepper { prev, text, .. } if prev == sl.prev => Some(text),
                _ => None,
            });
        }
        let gpu = self.gpu.as_ref()?;
        let (mut rects, mut texts, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
        self.build_screen_content(gpu, &mut rects, &mut texts, &mut sprites);
        let feld = sl.value_rect;
        texts.into_iter().find(|t| feld.contains(t.x + 1.0, t.y + 1.0)).map(|t| t.text)
    }

    // Stand eines Reglers ueber Pfeil-Aktionen: innerer Wert und die Zahl im Feld. Ein Wort
    // im Feld ("Aus", "unbegrenzt") liegt vor bzw. hinter jeder Zahl.
    pub(crate) fn step_slider_reading(&self, prev: ButtonAction, uhr: bool) -> Option<(f64, f64)> {
        let sl = self.step_slider_of(prev)?;
        let zahl = match self.step_slider_text(&sl).as_deref().and_then(slider_number) {
            Some((n, true)) if uhr => clock_from_day_start(n),
            Some((n, _)) => n,
            None if sl.value <= sl.min => f64::NEG_INFINITY,
            None => f64::INFINITY,
        };
        Some((sl.value, zahl))
    }

    // Getippte Zahl in einen Regler ueber Pfeil-Aktionen uebernehmen: Schritt fuer Schritt, bis
    // das Feld sie zeigt -- gezaehlt in dem, was dort steht (Zehntel, Prozent, Uhrzeit), nicht
    // im inneren Wert. Grenzen und Schrittweite bleiben so die der Pfeile.
    pub(crate) fn commit_step_edit(&mut self) {
        let Some((prev, buf)) = self.editing_step.take() else { return };
        let Some((getippt, _)) = slider_number(&buf) else { return };
        let Some(sl) = self.step_slider_of(prev) else { return };
        let uhr = self.step_slider_text(&sl).as_deref().and_then(slider_number).is_some_and(|(_, u)| u);
        let ziel = if uhr { clock_from_day_start(getippt) } else { getippt };
        if let Some(innen) = self.display_to_step_value(prev, &sl, uhr, ziel) {
            if self.set_step_value(prev, innen) {
                self.persist_settings();
                return;
            }
        }
        let Some((mut wert, mut zahl)) = self.step_slider_reading(prev, uhr) else { return };
        self.persist_paused = true;
        for _ in 0..2000 {
            if (zahl - ziel).abs() < 1e-9 {
                break;
            }
            let hoch = ziel > zahl;
            let (hin, zurueck) = if hoch { (sl.next, sl.prev) } else { (sl.prev, sl.next) };
            self.run_step_action(hin);
            let Some((neu_wert, neu_zahl)) = self.step_slider_reading(prev, uhr) else { break };
            // Am Rand angekommen.
            if neu_wert == wert {
                break;
            }
            // Falsche Richtung (ein Pfeil, der am Ende umlaeuft): zuruecknehmen.
            if (hoch && neu_zahl < zahl) || (!hoch && neu_zahl > zahl) {
                self.run_step_action(zurueck);
                break;
            }
            // Ueber das Ziel hinaus: zuruecknehmen, wenn der alte Wert naeher lag.
            if (hoch && neu_zahl > ziel) || (!hoch && neu_zahl < ziel) {
                if (neu_zahl - ziel).abs() > (zahl - ziel).abs() {
                    self.run_step_action(zurueck);
                }
                break;
            }
            (wert, zahl) = (neu_wert, neu_zahl);
        }
        self.persist_paused = false;
        self.persist_settings();
    }

    // Regler, deren getippte Zahl genau so uebernommen wird statt ueber die Pfeilschritte
    // angefahren -- dort sind Werte zwischen den Schritten (Nachkommastellen) erlaubt.
    // Setzt den Wert eines Reglers ueber Pfeil-Aktionen direkt, in inneren Einheiten (dem, was
    // der Regler als Wert fuehrt) -- ohne Schritte und ohne Grenzen: was getippt oder gezogen
    // wird, bleibt so stehen. Ganzzahlige Felder runden auf die naechste ganze Zahl; was der Typ
    // nicht fassen kann (negativ in einem vorzeichenlosen Feld), wird verworfen. false = fuer
    // diesen Regler gibt es keinen direkten Zugriff.
    pub(crate) fn set_step_value(&mut self, prev: ButtonAction, v: f64) -> bool {
        if !self.set_step_quiet(prev, v) {
            return false;
        }
        self.after_step_value(prev);
        true
    }

    // Wie set_step_value, ohne die Nebenwirkungen der Pfeil-Aktion (s. after_step_value).
    fn set_step_quiet(&mut self, prev: ButtonAction, v: f64) -> bool {
        if !v.is_finite() {
            return false;
        }
        match prev {
            ButtonAction::EgoVolume(false) => self.settings.ego_volume = v as f32,
            ButtonAction::AddonStep(m, k, false) => {
                if !matches!(self.addon_setting(m, k).map(|d| d.kind), Some(addons::SettingKind::Slider { .. })) {
                    return false;
                }
                self.set_addon_value(m, k, addon_number_text(v).replace(',', "."));
            }
            ButtonAction::LightFactorPrev(i) => {
                let Some(name) = light_source_blocks().get(i) else { return false };
                let Some(pct) = num_u32(v) else { return false };
                self.settings.set_light_factor(name, pct);
            }
            ButtonAction::Toteming(toteming::Action::Volume(slot, _)) => {
                let Some(n) = num_u32(v) else { return false };
                self.settings.toteming.sound[slot.index()].volume = n;
            }
            ButtonAction::Toteming(toteming::Action::Pitch(slot, _)) => {
                let Some(n) = num_u32(v) else { return false };
                self.settings.toteming.sound[slot.index()].pitch = n;
            }
            _ => {
                let Some(feld) = self.step_field(prev) else { return false };
                if !feld.set(v) {
                    return false;
                }
            }
        }
        true
    }

    // Getippte Zahl (in dem, was im Feld steht: Zehntel, Prozent, Uhrzeit ...) in den inneren
    // Wert umrechnen. Die Anzeige ist je Regler linear im inneren Wert; zwei Probewerte ergeben
    // die Gerade, danach steht der alte Wert wieder. None = kein direkter Zugriff oder keine
    // zwei Zahlen im Feld.
    fn display_to_step_value(&mut self, prev: ButtonAction, sl: &StepSlider, uhr: bool, ziel: f64) -> Option<f64> {
        let vorher = sl.value;
        let mut punkte: Vec<(f64, f64)> = Vec::new();
        for t in [0.5, 1.0, 0.25, 0.0, 0.75] {
            let probe = sl.min + t * (sl.max - sl.min);
            if !self.set_step_quiet(prev, probe) {
                break;
            }
            let Some((w, z)) = self.step_slider_reading(prev, uhr) else { continue };
            if z.is_finite() && punkte.iter().all(|&(pw, pz)| pw != w && pz != z) {
                punkte.push((w, z));
            }
            if punkte.len() == 2 {
                break;
            }
        }
        self.set_step_quiet(prev, vorher);
        let [(w1, z1), (w2, z2)] = punkte[..] else { return None };
        Some(w1 + (ziel - z1) * (w2 - w1) / (z2 - z1))
    }

    // Das Feld hinter einem Regler ueber Pfeil-Aktionen (s. set_step_value).
    fn step_field(&mut self, prev: ButtonAction) -> Option<NumMut<'_>> {
        let set = &mut self.settings;
        Some(match prev {
            ButtonAction::EgoLight(schatten, false) => if schatten { (&mut set.ego_shadow_light).into() } else { (&mut set.ego_light).into() },
            ButtonAction::ShieldColorPrev(farbe, achse) => set.shield_color_mut(farbe, achse)?.into(),
            ButtonAction::HitboxDensityPrev => (&mut set.module_hitbox_density_px).into(),
            ButtonAction::HitboxOpacityPrev => (&mut set.module_hitbox_opacity).into(),
            ButtonAction::TeamOpacity(seite, false) => (&mut set.team_hitboxes[seite.index()].opacity).into(),
            ButtonAction::TeamDensity(seite, false) => (&mut set.team_hitboxes[seite.index()].density_px).into(),
            ButtonAction::TrailHoldPrev => (&mut set.trail_hold_tenths).into(),
            ButtonAction::TrailFadePrev => (&mut set.trail_fade_tenths).into(),
            ButtonAction::SkyTimePrev => (&mut set.module_sky_time).into(),
            ButtonAction::WeatherRadiusInnerPrev => (&mut set.weather_radius_inner).into(),
            ButtonAction::WeatherRadiusPrev => (&mut set.weather_radius).into(),
            ButtonAction::ColorSaturationPrev => (&mut set.color_saturation).into(),
            ButtonAction::BlockOutlineWidthPrev => (&mut set.block_outline_width_px).into(),
            ButtonAction::VoiceStep(knob, false) => {
                let v = &mut set.voice;
                match knob {
                    VoiceKnob::Volume => (&mut v.volume_percent).into(),
                    VoiceKnob::MicGain => (&mut v.mic_gain_db).into(),
                    VoiceKnob::Threshold => (&mut v.threshold_db).into(),
                    VoiceKnob::Delay => if v.voice_activation { (&mut v.voice_delay_frames).into() } else { (&mut v.ptt_delay_frames).into() },
                    VoiceKnob::OutputBuffer => (&mut v.output_buffer_frames).into(),
                    VoiceKnob::PacketThreshold => (&mut v.packet_threshold).into(),
                }
            }
            ButtonAction::ZoomFactorPrev => (&mut set.zoom_factor_tenths).into(),
            ButtonAction::ZoomSensPrev => (&mut set.zoom_sens_percent).into(),
            ButtonAction::EatHoldThroughPrev => (&mut set.eat_hold_through_ms).into(),
            ButtonAction::NoToolBreakReservePrev => (&mut set.no_tool_break_reserve).into(),
            ButtonAction::SaturationHeightPrev => (&mut set.saturation_hud_height).into(),
            ButtonAction::SaturationAlpha(raster, false) => if raster { (&mut set.saturation_hud_grid_alpha).into() } else { (&mut set.saturation_hud_bg_alpha).into() },
            ButtonAction::EgoSpeed(false) => (&mut set.ego_speed).into(),
            ButtonAction::AutoclickerCpsPrev => (&mut set.autoclicker_cps).into(),
            ButtonAction::LagDelayPrev => (&mut set.lag_delay_ms).into(),
            ButtonAction::GhostTolerancePrev => (&mut set.ghost_tolerance_ms).into(),
            ButtonAction::Toteming(toteming::Action::Offset(_)) => (&mut set.toteming.offset).into(),
            ButtonAction::Toteming(toteming::Action::Size(_)) => (&mut set.toteming.size).into(),
            ButtonAction::ChunkUpdateThreadsDown => (&mut set.chunk_update_threads).into(),
            ButtonAction::TearTimingDown => (&mut set.tear_timing_us).into(),
            ButtonAction::TearReserveDown => (&mut set.tear_reserve_us).into(),
            ButtonAction::SmoothLightingDown => (&mut set.smooth_lighting_percent).into(),
            ButtonAction::DamageTintTicksPrev => (&mut set.damage_tint_ticks).into(),
            ButtonAction::ChunkSaverThresholdPrev => (&mut set.chunk_saver_threshold).into(),
            ButtonAction::HurtCamStrengthPrev => (&mut set.hurt_cam_strength).into(),
            ButtonAction::FireHeightPrev => (&mut set.fire_height).into(),
            ButtonAction::ScatteringPrev => (&mut set.shader_scattering_percent).into(),
            ButtonAction::SunSizePrev => (&mut set.shader_sun_size_tenths).into(),
            ButtonAction::BlockSoftnessPrev => (&mut set.shader_block_softness_deg).into(),
            ButtonAction::ParticleStep(i, col, false) => {
                let t = set.particle_tuning.get_mut(i)?;
                match col {
                    0 => (&mut t.lifetime_pct).into(),
                    1 => (&mut t.opacity_pct).into(),
                    2 => (&mut t.speed_pct).into(),
                    _ if particles::KINDS[i].spawn.has_interval() => (&mut t.interval_pct).into(),
                    _ => (&mut t.amount_pct).into(),
                }
            }
            ButtonAction::UiPitchDown => (&mut set.ui_pitch_semitones).into(),
            ButtonAction::CursorSizeDown => (&mut set.cursor_size).into(),
            ButtonAction::HoldConfirmDown => (&mut set.hold_confirm_ms).into(),
            ButtonAction::ReconnectHoldDown => (&mut set.reconnect_hold_ms).into(),
            ButtonAction::CursorWidthDown => (&mut set.cursor_width).into(),
            ButtonAction::CrosshairBoostPrev => (&mut set.crosshair_brightness_boost).into(),
            ButtonAction::CrosshairSizePrev => {
                let i = set.crosshair_shape.min(CROSSHAIR_SHAPES.len() - 1);
                (&mut set.crosshair_sizes[i]).into()
            }
            ButtonAction::HudScaleStep(element, false) => match element {
                HudElement::Title => (&mut set.hud_title_scale).into(),
                HudElement::Subtitle => (&mut set.hud_subtitle_scale).into(),
                HudElement::Effects => (&mut set.hud_effects_scale).into(),
                HudElement::BossBar => (&mut set.hud_bossbar_scale).into(),
                HudElement::Scoreboard => (&mut set.hud_scoreboard_scale).into(),
                HudElement::Arrows => (&mut set.hud_arrows_scale).into(),
                HudElement::HeldName => (&mut set.hud_held_name_scale).into(),
                HudElement::Armor => (&mut set.hud_armor_scale).into(),
                HudElement::Keystrokes => (&mut set.hud_keys_scale).into(),
                HudElement::Session => (&mut set.hud_session_scale).into(),
                _ => return None,
            },
            ButtonAction::KeysOpacityPrev => (&mut set.keys_opacity).into(),
            ButtonAction::SessionOpacity(false) => (&mut set.session_opacity).into(),
            ButtonAction::ArmorHudGapPrev => (&mut set.armor_hud_gap).into(),
            ButtonAction::ArmorHudNumberScalePrev => (&mut set.armor_hud_number_scale).into(),
            ButtonAction::ArmorHudNumberXPrev => (&mut set.armor_hud_number_x).into(),
            ButtonAction::ArmorHudNumberYPrev => (&mut set.armor_hud_number_y).into(),
            ButtonAction::ArrowOpacityPrev => (&mut set.arrow_opacity).into(),
            ButtonAction::ChatScalePrev => (&mut set.chat_scale_percent).into(),
            ButtonAction::ChatWidthPrev => (&mut set.chat_width).into(),
            ButtonAction::ChatBackgroundPrev => (&mut set.chat_background).into(),
            ButtonAction::ChatOpenDimPrev => (&mut set.chat_open_dim).into(),
            ButtonAction::ChatLinesPrev => (&mut set.chat_lines).into(),
            ButtonAction::ChatLinesOpenPrev => (&mut set.chat_lines_open).into(),
            ButtonAction::ChatFadePrev => (&mut set.chat_fade_secs).into(),
            ButtonAction::ChatLineSpacingPrev => (&mut set.chat_line_spacing).into(),
            _ => return None,
        })
    }

    // Was die Pfeil-Aktion eines Reglers ausser dem Wert noch anstoesst, nach dem direkten Setzen.
    fn after_step_value(&mut self, prev: ButtonAction) {
        match prev {
            ButtonAction::ChunkUpdateThreadsDown => {
                self.mesh_pool.resize(self.settings.chunk_update_threads);
                self.light_scroll = Default::default();
            }
            ButtonAction::VoiceStep(VoiceKnob::OutputBuffer, _) => self.audio.sink().set_stream_buffer(self.settings.voice.output_buffer_frames as usize),
            ButtonAction::VoiceStep(VoiceKnob::PacketThreshold, _) => {
                if let Some(c) = &self.voice.client {
                    c.set_packet_threshold(self.settings.voice.packet_threshold as usize);
                }
            }
            ButtonAction::LagDelayPrev => self.sync_lag_delay(),
            // Gezogen oder getippt: wie bei den Pfeilen steckt sie im Mesh (s. run_menu_action).
            ButtonAction::SmoothLightingDown => {
                for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                    self.world.mark_dirty(cx, cz);
                }
            }
            _ => {}
        }
    }

    // Laufende Freitext-Eingabe in einem Wertfeld: der Puffer linksbuendig mit blinkendem
    // Caret (wie das Adressfeld auf dem Connect-Screen). Solange noch nichts getippt ist, steht
    // der bisherige Wert abgeblendet da (wie ein Platzhalter in einem Web-Formular): das Feld
    // startet leer, damit man sofort lostippen kann, aber man sieht weiterhin, was gerade
    // gilt -- und dass Enter ohne Eingabe genau diesen Wert stehen laesst.
    pub(crate) fn push_value_input(&self, gpu: &GpuState, rect: Rect, bisher: String, buf: &str, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let x = rect.x + 8.0;
        let y = (rect.y + (rect.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let (shown, color) = if buf.is_empty() { (bisher, theme::text_disabled()) } else { (buf.to_string(), theme::text()) };
        // Bei leerem Puffer sitzt der Caret VOR dem Platzhalter, sonst hinter dem Getippten --
        // in beiden Faellen dort, wo das naechste Zeichen landet.
        let caret_x = if buf.is_empty() { x } else { x + gpu.measure_raw(buf, UI_SCALE_SMALL) };
        let text_x = if buf.is_empty() { x + CARET_WIDTH_PX + 3.0 } else { x };
        push_typed_marker(texts);
        texts.push(UiText { text: shown, x: text_x, y, color, scale: UI_SCALE_SMALL });
        self.push_caret(rects, gpu.caret_rect(caret_x, y, UI_SCALE_SMALL));
    }

    // Schieberegler zeichnen: Bahn, gefuellter Teil, Daumen (mit Ring beim Zeigen/Ziehen).
    pub(crate) fn push_slider_visual(rects: &mut Vec<UiRect>, bahn: Rect, t: f32, hover: bool, aktiv: bool, enabled: bool) {
        use theme::Slot;
        let g = theme::get;
        let t = t.clamp(0.0, 1.0);
        rects.push(UiRect::rounded(bahn, g(Slot::Surface3), 2.0));
        let fuell = if enabled { g(Slot::Accent) } else { g(Slot::Border2) };
        rects.push(UiRect::rounded(Rect { w: bahn.w * t, ..bahn }, fuell, 2.0));
        let daumen = Rect { x: (bahn.x + bahn.w * t - 5.0).round(), y: bahn.y + 2.0 - 10.0, w: 10.0, h: 20.0 };
        if hover && enabled {
            let ring = if aktiv { 6.0 } else { 4.0 };
            let a = if aktiv { 0.25 } else { 0.18 };
            rects.push(UiRect::rounded(
                Rect { x: daumen.x - ring, y: daumen.y - ring, w: daumen.w + 2.0 * ring, h: daumen.h + 2.0 * ring },
                theme::with_alpha(g(Slot::Accent), a),
                3.0 + ring,
            ));
        }
        rects.push(UiRect::shadow(daumen, 3.0, 1.0, 3.0, 0.6));
        rects.push(UiRect::rounded(daumen, if enabled { theme::rgba(0xF4F7FA) } else { g(Slot::TextDisabled) }, 3.0));
    }

    // --- Farbfenster ---

    pub(crate) fn color_axes(&self, t: ColorTarget) -> usize {
        match t {
            ColorTarget::Theme(_)
            | ColorTarget::Hitbox(_)
            | ColorTarget::Team(_)
            | ColorTarget::Sky
            | ColorTarget::Crosshair
            | ColorTarget::FireCrosshair
            | ColorTarget::SaturationHud => 3,
            ColorTarget::EgoName(i) => if i == 2 { 4 } else { 3 },
            ColorTarget::Addon(..) => 3,
            ColorTarget::Shield(farbe) => ShieldColor::axes(self.settings.shield_mark(farbe)).len(),
            ColorTarget::BlockOutline | ColorTarget::DamageTint | ColorTarget::Crit | ColorTarget::HitboxKind(_) => 4,
        }
    }

    pub(crate) fn color_title(t: ColorTarget) -> String {
        match t {
            ColorTarget::Theme(slot) => slot.label().to_string(),
            ColorTarget::Hitbox(HitboxVariant::Smoothed) => "Hitbox geglättet".to_string(),
            ColorTarget::Hitbox(HitboxVariant::Raw) => "Hitbox roh".to_string(),
            ColorTarget::HitboxKind(art) => format!("Hitbox {}", art.label()),
            ColorTarget::Team(seite) => format!("Hitbox {}", seite.label()),
            ColorTarget::Shield(ShieldColor::Cooldown) => "Schild-Cooldown".to_string(),
            ColorTarget::Shield(ShieldColor::Hold) => "Schild gehalten".to_string(),
            ColorTarget::Sky => "Himmelsfarbe".to_string(),
            ColorTarget::BlockOutline => "Blockumriss".to_string(),
            ColorTarget::DamageTint => "Schadens-Färbung".to_string(),
            ColorTarget::Crit => "Crit-Farbe".to_string(),
            ColorTarget::Crosshair => "Kreuz-Farbe".to_string(),
            ColorTarget::FireCrosshair => "Kreuz beim Brennen".to_string(),
            ColorTarget::SaturationHud => "Sättigung".to_string(),
            ColorTarget::EgoName(2) => "Namensschatten".to_string(),
            ColorTarget::EgoName(3) => "Namensschatten 2".to_string(),
            ColorTarget::EgoName(i) => format!("Namensfarbe {}", i + 1),
            ColorTarget::Addon(m, _) => ModuleId::Addon(m).label().to_string(),
        }
    }

    // Stand einer Farbe: Ton 0..360, Saettigung/Helligkeit/Deckkraft 0..100 (ohne Deckkraft 100).
    pub(crate) fn color_hsva(&self, t: ColorTarget) -> [f32; 4] {
        let s = &self.settings;
        let a4 = |f: &dyn Fn(DamageTintAxis) -> u32| {
            [f(DamageTintAxis::Hue) as f32, f(DamageTintAxis::Saturation) as f32, f(DamageTintAxis::Value) as f32, f(DamageTintAxis::Alpha) as f32]
        };
        match t {
            ColorTarget::Theme(slot) => {
                let (h, sa, v) = theme::to_hsv(theme::srgb(slot));
                [h, sa * 100.0, v * 100.0, 100.0]
            }
            ColorTarget::Hitbox(variant) => {
                let (h, sa, v) = self.hitbox_color_of(variant);
                [h as f32, sa as f32, v as f32, 100.0]
            }
            ColorTarget::HitboxKind(art) => {
                let st = s.module_hitbox_styles[art.index()];
                [st.hue as f32, st.saturation as f32, st.value as f32, st.alpha as f32]
            }
            ColorTarget::Team(seite) => {
                let st = s.team_hitboxes[seite.index()];
                [st.hue as f32, st.saturation as f32, st.value as f32, 100.0]
            }
            ColorTarget::Crosshair => [s.crosshair_hue as f32, s.crosshair_saturation as f32, s.crosshair_value as f32, 100.0],
            ColorTarget::FireCrosshair => [s.fire_crosshair_hue as f32, s.fire_crosshair_saturation as f32, s.fire_crosshair_value as f32, 100.0],
            ColorTarget::SaturationHud => [s.saturation_hud_hue as f32, s.saturation_hud_saturation as f32, s.saturation_hud_value as f32, 100.0],
            ColorTarget::EgoName(i) => s.ego_colors[i as usize].map(|v| v as f32),
            ColorTarget::Addon(m, k) => {
                let (h, sa, v) = theme::to_hsv(addons::parse_color(&self.addon_value(m, k)).unwrap_or(0xFF_FFFF));
                [h, sa * 100.0, v * 100.0, 100.0]
            }
            ColorTarget::Sky => [
                s.sky_color_of(SkyColorAxis::Hue) as f32,
                s.sky_color_of(SkyColorAxis::Saturation) as f32,
                s.sky_color_of(SkyColorAxis::Value) as f32,
                100.0,
            ],
            ColorTarget::Shield(farbe) => {
                let mut c = a4(&|a| s.shield_color_of(farbe, a));
                if self.color_axes(t) < 4 {
                    c[3] = 100.0;
                }
                c
            }
            ColorTarget::BlockOutline => a4(&|a| s.block_outline_of(a)),
            ColorTarget::DamageTint => a4(&|a| s.damage_tint_of(a)),
            ColorTarget::Crit => a4(&|a| s.crit_color_of(a)),
        }
    }

    pub(crate) fn set_color_hsva(&mut self, t: ColorTarget, c: [f32; 4]) {
        if let ColorTarget::Addon(m, k) = t {
            let [r, g, b] = hsv_to_rgb(c[0], c[1] / 100.0, c[2] / 100.0);
            let k8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
            self.set_addon_value(m, k, format!("#{:06X}", (k8(r) << 16) | (k8(g) << 8) | k8(b)));
            return;
        }
        let u = |i: usize| c[i].round().max(0.0) as u32;
        let h = u(0) % 360;
        let set = &mut self.settings;
        match t {
            // Oben schon erledigt.
            ColorTarget::Addon(..) => {}
            // Nur eigene Themes: bei einem Preset gibt es gar keine Farbfelder zum Anklicken.
            ColorTarget::Theme(slot) => {
                if let ThemeChoice::Custom(i) = set.theme_choice {
                    set.theme[slot.index()] = theme::from_hsv(c[0], c[1] / 100.0, c[2] / 100.0);
                    theme::derive(&mut set.theme, slot);
                    if let Some(t) = set.custom_themes.get_mut(i) {
                        t.colors = set.theme;
                    }
                    theme::apply(&set.theme);
                }
            }
            ColorTarget::Hitbox(HitboxVariant::Smoothed) => {
                (set.module_hitbox_smoothed_hue, set.module_hitbox_smoothed_saturation, set.module_hitbox_smoothed_value) = (h, u(1), u(2));
            }
            ColorTarget::Hitbox(HitboxVariant::Raw) => {
                (set.module_hitbox_raw_hue, set.module_hitbox_raw_saturation, set.module_hitbox_raw_value) = (h, u(1), u(2));
            }
            ColorTarget::HitboxKind(art) => {
                let st = &mut set.module_hitbox_styles[art.index()];
                (st.hue, st.saturation, st.value, st.alpha) = (h, u(1).min(100), u(2).min(100), u(3).min(100));
            }
            ColorTarget::Team(seite) => {
                let st = &mut set.team_hitboxes[seite.index()];
                (st.hue, st.saturation, st.value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::Crosshair => {
                (set.crosshair_hue, set.crosshair_saturation, set.crosshair_value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::FireCrosshair => {
                (set.fire_crosshair_hue, set.fire_crosshair_saturation, set.fire_crosshair_value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::EgoName(i) => set.ego_colors[i as usize] = [h, u(1).min(100), u(2).min(100), if i == 2 { u(3).min(100) } else { 100 }],
            ColorTarget::SaturationHud => {
                (set.saturation_hud_hue, set.saturation_hud_saturation, set.saturation_hud_value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::Sky => {
                *set.sky_color_mut(SkyColorAxis::Hue) = h;
                *set.sky_color_mut(SkyColorAxis::Saturation) = u(1);
                *set.sky_color_mut(SkyColorAxis::Value) = u(2);
            }
            ColorTarget::Shield(farbe) => {
                for (i, a) in DamageTintAxis::ALL.into_iter().enumerate() {
                    if let Some(feld) = set.shield_color_mut(farbe, a) {
                        *feld = if i == 0 { h } else { u(i) };
                    }
                }
            }
            ColorTarget::BlockOutline | ColorTarget::DamageTint | ColorTarget::Crit => {
                for (i, a) in DamageTintAxis::ALL.into_iter().enumerate() {
                    let feld = match t {
                        ColorTarget::BlockOutline => set.block_outline_mut(a),
                        ColorTarget::DamageTint => set.damage_tint_mut(a),
                        _ => set.crit_color_mut(a),
                    };
                    *feld = if i == 0 { h } else { u(i) };
                }
            }
        }
    }

    // Farbe als Flaechenfarbe fuer Farbfeld und Vorschau.
    pub(crate) fn color_preview(&self, t: ColorTarget, c: [f32; 4]) -> [f32; 4] {
        match t {
            ColorTarget::Theme(_) => theme::rgba(theme::from_hsv(c[0], c[1] / 100.0, c[2] / 100.0)),
            _ => {
                let rgb = hsv_to_rgb(c[0], c[1] / 100.0, c[2] / 100.0);
                [rgb[0], rgb[1], rgb[2], 1.0]
            }
        }
    }

    // Welche Farbe ein Knopf oeffnet: das neue OpenColorPicker und die alten Aufklapp-Knoepfe
    // der Module, die jetzt ebenfalls das Fenster oeffnen.
    pub(crate) fn color_target_of(action: ButtonAction) -> Option<ColorTarget> {
        match action {
            ButtonAction::OpenColorPicker(t) => Some(t),
            ButtonAction::HitboxColorEditor(v) => Some(ColorTarget::Hitbox(v)),
            ButtonAction::ShieldColorEditor(c) => Some(ColorTarget::Shield(c)),
            ButtonAction::BlockOutlineEditor => Some(ColorTarget::BlockOutline),
            _ => None,
        }
    }

    pub(crate) fn open_color_popup(&mut self, target: ColorTarget, anchor: Rect) {
        if self.color_popup.as_ref().is_some_and(|p| p.target == target) {
            self.color_popup = None;
            return;
        }
        self.color_popup = Some(ColorPopup { target, anchor, hsva: self.color_hsva(target), drag: None });
    }

    // Fenster und seine Teile in Bildschirmkoordinaten: (Fenster, Vorschau, Spur je Achse).
    // Farbfenster, die neben statt unter ihrem Farbfeld aufgehen: Kreuz und Blockumriss.
    pub(crate) fn color_popup_beside(t: ColorTarget) -> bool {
        matches!(t, ColorTarget::Crosshair | ColorTarget::BlockOutline)
    }

    pub(crate) fn color_popup_layout(&self, popup: &ColorPopup) -> (Rect, Rect, Vec<Rect>) {
        let (sw, sh) = ui_screen();
        let achsen = self.color_axes(popup.target);
        let h = COLOR_POPUP_PAD + 24.0 + COLOR_POPUP_PREVIEW_H + 8.0 + achsen as f32 * COLOR_POPUP_ROW_H + COLOR_POPUP_PAD;
        let a = popup.anchor;
        let (x, y) = if Self::color_popup_beside(popup.target) {
            // Rechts neben dem Farbfeld, oben buendig; passt es dort nicht hin, links davon.
            let rechts = a.x + a.w + 8.0;
            let x = if rechts + COLOR_POPUP_W <= sw - 8.0 { rechts } else { (a.x - 8.0 - COLOR_POPUP_W).max(8.0) };
            (x, a.y.min(sh - 8.0 - h).max(8.0))
        } else {
            let x = (a.x + a.w - COLOR_POPUP_W).clamp(8.0, (sw - COLOR_POPUP_W - 8.0).max(8.0));
            let unten = a.y + a.h + 8.0;
            (x, if unten + h <= sh - 8.0 { unten } else { (a.y - 8.0 - h).max(TOPBAR_H + 8.0) })
        };
        let fenster = Rect { x, y, w: COLOR_POPUP_W, h };
        let vorschau = Rect { x: x + COLOR_POPUP_PAD, y: y + COLOR_POPUP_PAD + 24.0, w: COLOR_POPUP_W - 2.0 * COLOR_POPUP_PAD, h: COLOR_POPUP_PREVIEW_H };
        let spuren = (0..achsen)
            .map(|i| Rect {
                x: vorschau.x,
                y: vorschau.y + vorschau.h + 8.0 + i as f32 * COLOR_POPUP_ROW_H + 22.0,
                w: vorschau.w,
                h: COLOR_POPUP_TRACK_H,
            })
            .collect();
        (fenster, vorschau, spuren)
    }

    // Reihenfolge der Regler wie im Design: Ton, Helligkeit, Saettigung, Deckkraft -- als
    // Index in hsva.
    pub(crate) const COLOR_POPUP_ORDER: [usize; 4] = [0, 2, 1, 3];

    // Linke Maustaste gedrueckt, waehrend das Fenster offen ist. true = Klick verbraucht.
    pub(crate) fn color_popup_press(&mut self) -> bool {
        let Some(popup) = self.color_popup.as_ref() else { return false };
        let (fenster, _, spuren) = self.color_popup_layout(popup);
        let (cx, cy) = self.cursor_pos;
        if !fenster.contains(cx, cy) {
            // Daneben: schliessen. Der Klick tut sonst nichts -- sonst loeste man beim
            // Wegklicken versehentlich einen Knopf darunter aus.
            self.color_popup = None;
            self.persist_settings();
            return true;
        }
        if let Some(i) = spuren.iter().position(|r| cx >= r.x - 6.0 && cx <= r.x + r.w + 6.0 && cy >= r.y - 10.0 && cy <= r.y + r.h + 10.0) {
            if let Some(p) = self.color_popup.as_mut() {
                p.drag = Some(i);
            }
            self.color_popup_drag();
        }
        true
    }

    pub(crate) fn color_popup_drag(&mut self) {
        let Some(popup) = self.color_popup.as_ref() else { return };
        let Some(i) = popup.drag else { return };
        let (_, _, spuren) = self.color_popup_layout(popup);
        let r = spuren[i];
        let t = ((self.cursor_pos.0 - r.x) / r.w).clamp(0.0, 1.0);
        let achse = Self::COLOR_POPUP_ORDER[i];
        let max = if achse == 0 { 359.0 } else { 100.0 };
        let target = popup.target;
        let mut hsva = popup.hsva;
        hsva[achse] = t * max;
        if let Some(p) = self.color_popup.as_mut() {
            p.hsva = hsva;
        }
        self.set_color_hsva(target, hsva);
    }

    pub(crate) fn color_popup_release(&mut self) {
        if let Some(p) = self.color_popup.as_mut() {
            if p.drag.take().is_some() {
                self.persist_settings();
            }
        }
    }

    // Das Fenster in der Overlay-Ebene (ueber allem Menuetext).
    // Fenster "Erweitert" in Bildschirmkoordinaten, mittig: (Fenster, je Belegung ein Feld,
    // "+" fuer eine weitere (None, wenn alle Plaetze voll sind), "Fertig").
    pub(crate) fn keybind_advanced_layout(&self, aktion: KeyBindAction) -> (Rect, Vec<Rect>, Option<Rect>, Rect) {
        let (sw, sh) = ui_screen();
        let (w, pad, kopf, zeile) = (400.0, 20.0, 44.0, SETTINGS_CTRL_H + 8.0);
        let belegt = self.settings.keybinds.count(aktion);
        let plus = belegt < MAX_BINDS_PER_ACTION;
        let h = pad + kopf + (belegt + plus as usize) as f32 * zeile + 12.0 + SETTINGS_CTRL_H + pad;
        let fenster = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let feld = |i: usize| Rect { x: fenster.x + pad, y: fenster.y + pad + kopf + i as f32 * zeile, w: w - 2.0 * pad, h: SETTINGS_CTRL_H };
        let felder = (0..belegt).map(feld).collect();
        let fertig = Rect { x: fenster.x + w - pad - 140.0, y: fenster.y + h - pad - SETTINGS_CTRL_H, w: 140.0, h: SETTINGS_CTRL_H };
        (fenster, felder, plus.then(|| feld(belegt)), fertig)
    }

    // Seine Knoepfe als unsichtbare Trefferflaechen -- gezeichnet wird das Fenster in der
    // Overlay-Ebene (s. build_keybind_advanced), sonst laege die Seite darueber.
    pub(crate) fn keybind_advanced_buttons(&self, aktion: KeyBindAction) -> Vec<Button> {
        let (_, felder, plus, fertig) = self.keybind_advanced_layout(aktion);
        let flaeche = |action, rect| Button { action, rect, label: String::new(), enabled: true, color: SERVER_CARD_MARKER, scale: UI_SCALE_SMALL, slider: None };
        let mut out: Vec<Button> = felder.into_iter().enumerate().map(|(i, r)| flaeche(ButtonAction::Rebind(aktion, i), r)).collect();
        if let Some(r) = plus {
            out.push(flaeche(ButtonAction::Rebind(aktion, self.settings.keybinds.count(aktion)), r));
        }
        out.push(flaeche(ButtonAction::CloseKeybindAdvanced, fertig));
        out
    }

    pub(crate) fn build_keybind_advanced(&self, overlay: &mut UiOverlay) {
        let Some(aktion) = self.keybind_advanced.filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsControls) else { return };
        let Some(gpu) = self.gpu.as_ref() else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (fenster, felder, plus, fertig) = self.keybind_advanced_layout(aktion);
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.0, 0.0, 0.0, 0.45], ..UiRect::FLAT },
            UiRect::shadow(fenster, 8.0, 8.0, 24.0, 0.55),
            UiRect::framed(fenster, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let mut texts = vec![UiText { text: format!("Tasten: {}", aktion.short_label()), x: fenster.x + 20.0, y: fenster.y + 22.0, color: theme::text(), scale: UI_SCALE_SMALL }];
        let knopf = |r: Rect, text: String, wartet: bool, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>| {
            let rand = if r.contains(cx, cy) { g(Slot::Border2) } else { g(Slot::Border1) };
            let flaeche = if wartet { g(Slot::AccentSurface) } else { g(Slot::Surface2) };
            rects.push(UiRect::framed(r, flaeche, 6.0, 1.0, rand));
            texts.push(gpu.centered_text(r, text, theme::text(), UI_SCALE_SMALL));
        };
        let kb = &self.settings.keybinds;
        for (i, r) in felder.into_iter().enumerate() {
            let wartet = self.awaiting_keybind == Some((aktion, i));
            let text = if wartet { "> Taste/Maus <".to_string() } else { kb.bind_at(aktion, i).map(bind_display).unwrap_or_default() };
            knopf(r, text, wartet, &mut rects, &mut texts);
        }
        if let Some(r) = plus {
            let wartet = self.awaiting_keybind == Some((aktion, kb.count(aktion)));
            knopf(r, if wartet { "> Taste/Maus <" } else { "+" }.to_string(), wartet, &mut rects, &mut texts);
        }
        knopf(fertig, "Fertig".to_string(), false, &mut rects, &mut texts);
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    pub(crate) fn build_color_popup(&self, overlay: &mut UiOverlay) {
        let (Some(gpu), Some(popup)) = (self.gpu.as_ref(), self.color_popup.as_ref()) else { return };
        use theme::Slot;
        let g = theme::get;
        let (fenster, vorschau, spuren) = self.color_popup_layout(popup);
        let mut rects = vec![UiRect::shadow(fenster, 8.0, 8.0, 24.0, 0.55), UiRect::framed(fenster, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1))];
        let mut texts = Vec::new();
        texts.push(UiText {
            text: Self::color_title(popup.target),
            x: fenster.x + COLOR_POPUP_PAD,
            y: fenster.y + COLOR_POPUP_PAD,
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
        let c = popup.hsva;
        let farbe = self.color_preview(popup.target, c);
        let mit_deckkraft = self.color_axes(popup.target) == 4;
        // Vorschau; mit Deckkraft ueber einem Schachbrett, damit man sie sieht.
        if mit_deckkraft {
            let feld = 12.0;
            let (nx, ny) = ((vorschau.w / feld).ceil() as usize, (vorschau.h / feld).ceil() as usize);
            rects.push(UiRect::rounded(vorschau, theme::rgba(0x2A2F36), 6.0));
            for yy in 0..ny {
                for xx in 0..nx {
                    if (xx + yy) % 2 == 0 {
                        let r = Rect { x: vorschau.x + xx as f32 * feld, y: vorschau.y + yy as f32 * feld, w: feld, h: feld };
                        let r = Rect { w: r.w.min(vorschau.x + vorschau.w - r.x), h: r.h.min(vorschau.y + vorschau.h - r.y), ..r };
                        rects.push(UiRect { rect: r, color: theme::rgba(0x3A414B), ..UiRect::FLAT });
                    }
                }
            }
        }
        rects.push(UiRect::framed(vorschau, theme::with_alpha(farbe, if mit_deckkraft { c[3] / 100.0 } else { 1.0 }), 6.0, 2.0, [0.0, 0.0, 0.0, 1.0]));
        let namen = ["Farbton", "Helligkeit", "Sättigung", "Deckkraft"];
        for (i, spur) in spuren.iter().enumerate() {
            let achse = Self::COLOR_POPUP_ORDER[i];
            let wert = c[achse];
            let (text, max) = if achse == 0 { (format!("{}°", wert.round() as u32), 360.0) } else { (format!("{} %", wert.round() as u32), 100.0) };
            let ly = spur.y - 20.0;
            texts.push(UiText { text: namen[i].to_string(), x: spur.x, y: ly, color: theme::text_2(), scale: TOPBAR_SUB_SCALE });
            let tw = gpu.measure_text(&text, TOPBAR_SUB_SCALE);
            texts.push(UiText { text, x: (spur.x + spur.w - tw).round(), y: ly, color: theme::text(), scale: TOPBAR_SUB_SCALE });
            // Verlauf der Spur: so viele Streifen, dass er glatt wirkt.
            const STUECKE: usize = 48;
            for k in 0..STUECKE {
                let t = (k as f32 + 0.5) / STUECKE as f32;
                let mut probe = c;
                probe[3] = 100.0;
                match achse {
                    0 => (probe[0], probe[1], probe[2]) = (t * 360.0, 100.0, 100.0),
                    3 => {}
                    _ => probe[achse] = t * 100.0,
                }
                let mut f = self.color_preview(popup.target, probe);
                if achse == 3 {
                    f[3] = t;
                }
                let x0 = spur.x + spur.w * k as f32 / STUECKE as f32;
                let x1 = spur.x + spur.w * (k + 1) as f32 / STUECKE as f32;
                rects.push(UiRect { rect: Rect { x: x0, y: spur.y, w: x1 - x0 + 0.5, h: spur.h }, color: f, ..UiRect::FLAT });
            }
            rects.push(UiRect::framed(*spur, [0.0; 4], 3.0, 1.0, g(Slot::Border2)));
            let tx = spur.x + spur.w * (wert / max).clamp(0.0, 1.0);
            let daumen = Rect { x: (tx - 5.0).round(), y: spur.y + spur.h / 2.0 - 10.0, w: 10.0, h: 20.0 };
            rects.push(UiRect::shadow(daumen, 3.0, 1.0, 3.0, 0.6));
            rects.push(UiRect::framed(daumen, [1.0; 4], 3.0, 2.0, [0.0, 0.0, 0.0, 1.0]));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // --- Addons (s. addons.rs) ---

    // Welche Auswahl ein Knopf aufklappt, statt weiterzuschalten.
    pub(crate) fn choice_of(action: ButtonAction) -> Option<Choice> {
        Some(match action {
            ButtonAction::CycleProjection => Choice::Projection,
            ButtonAction::CycleParticles => Choice::Particles,
            ButtonAction::CycleDeathAnimation => Choice::DeathAnimation,
            ButtonAction::CycleWallpaper => Choice::Wallpaper,
            ButtonAction::CycleLanguage(row) => Choice::Language(row),
            ButtonAction::CycleHoldUseRepeat => Choice::HoldUseRepeat,
            ButtonAction::CycleArrowEmptyFeedback => Choice::ArrowEmptyFeedback,
            ButtonAction::CycleSkyMode => Choice::SkyMode,
            ButtonAction::CycleWeatherMode => Choice::WeatherMode,
            ButtonAction::CycleEgoDeco => Choice::EgoDeco,
            ButtonAction::CycleEgoShadow => Choice::EgoShadow,
            ButtonAction::CycleEgoSound => Choice::EgoSound,
            ButtonAction::OpenChoice(c) => c,
            _ => return None,
        })
    }

    // Die Werte einer Auswahl und welcher gerade gilt (None: keiner, z.B. gemischte Sprachen).
    pub(crate) fn choice_options(&self, c: Choice) -> (Vec<String>, Option<usize>) {
        let s = &self.settings;
        let namen = |l: &[&str]| l.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        match c {
            Choice::Projection => (namen(&PROJECTION_NAMES), Some(s.projection_mode as usize)),
            Choice::Particles => (namen(&PARTICLE_MODE_NAMES), Some(s.particles_mode as usize)),
            Choice::DeathAnimation => (namen(&DEATH_ANIM_NAMES), Some(s.death_animation as usize)),
            Choice::Wallpaper => (wallpaper::WallpaperMode::ALL.iter().map(|m| m.label().to_string()).collect(), Some(s.wallpaper as usize)),
            Choice::Language(row) => (i18n::Language::ALL.iter().map(|l| l.label().to_string()).collect(), self.language_of_row(row).map(|l| l.index() as usize)),
            Choice::HoldUseRepeat => (HoldUseRepeat::ALL.iter().map(|h| h.label().to_string()).collect(), HoldUseRepeat::ALL.iter().position(|h| *h == s.hold_use_repeat)),
            Choice::ArrowEmptyFeedback => (namen(&ARROW_EMPTY_FEEDBACK_NAMES), Some(s.arrow_empty_feedback as usize)),
            Choice::SkyMode => (SkyMode::ALL.iter().map(|m| m.label().to_string()).collect(), SkyMode::ALL.iter().position(|m| *m == s.sky_mode)),
            Choice::WeatherMode => (WeatherMode::ALL.iter().map(|m| m.label().to_string()).collect(), WeatherMode::ALL.iter().position(|m| *m == s.weather_mode)),
            Choice::EgoDeco => (ego_name::Deco::ALL.iter().map(|d| d.label().to_string()).collect(), Some(s.ego_deco as usize)),
            Choice::EgoShadow => (ego_name::Deco::ALL.iter().map(|d| d.label_or("Standard").to_string()).collect(), Some(s.ego_shadow as usize)),
            Choice::EgoSound => (ego_name::SOUNDS.iter().map(|(_, l)| l.to_string()).collect(), ego_name::SOUNDS.iter().position(|(e, _)| *e == s.ego_sound)),
            Choice::AudioDevice => (self.audio_device_options(), Some(self.audio_device_index())),
            Choice::AudioBuffer => (
                AUDIO_BUFFER_CHOICES.iter().map(|&f| audio_buffer_label(f)).collect(),
                AUDIO_BUFFER_CHOICES.iter().position(|&f| f == s.audio_buffer_frames),
            ),
            Choice::CrosshairShape => ((0..=CROSSHAIR_SHAPE_CUSTOM).map(|i| crosshair_shape_label(i).to_string()).collect(), Some(s.crosshair_shape_choice())),
            Choice::Addon(m, k) => match self.addon_setting(m, k).map(|d| d.kind) {
                Some(addons::SettingKind::Choice { options }) => {
                    let wert = self.addon_value(m, k);
                    let jetzt = options.iter().position(|o| *o == wert);
                    (options, jetzt)
                }
                _ => (Vec::new(), None),
            },
        }
    }

    // Wert i einer Auswahl setzen. Die Durchschalt-Auswahlen setzen den Wert davor und schalten
    // einmal weiter -- so laeuft genau einmal, was am Weiterschalten haengt (Hintergrund laden,
    // Sprache anwenden, Ton vorspielen).
    pub(crate) fn pick_choice(&mut self, c: Choice, i: usize) {
        let (optionen, jetzt) = self.choice_options(c);
        let n = optionen.len();
        if i >= n || jetzt == Some(i) {
            return;
        }
        match c {
            Choice::AudioDevice => {
                let name = if i == 0 { String::new() } else { optionen[i].clone() };
                let opt = (!name.is_empty()).then_some(name.as_str());
                match self.audio.set_output_device(opt) {
                    Ok(()) => self.settings.audio_output_device = name,
                    Err(e) => self.push_log(format!("Ausgabegerät konnte nicht gesetzt werden: {e}")),
                }
                self.persist_settings();
                return;
            }
            Choice::AudioBuffer => {
                let frames = AUDIO_BUFFER_CHOICES[i];
                match self.audio.set_buffer_frames(audio_buffer_request(frames)) {
                    Ok(()) => self.settings.audio_buffer_frames = frames,
                    Err(e) => self.push_log(format!("Puffer {} geht mit diesem Gerät nicht: {e}", audio_buffer_label(frames))),
                }
                self.persist_settings();
                return;
            }
            Choice::CrosshairShape => {
                self.settings.set_crosshair_shape_choice(i);
                self.persist_settings();
                return;
            }
            Choice::Addon(m, k) => {
                self.set_addon_value(m, k, optionen[i].clone());
                self.persist_settings();
                return;
            }
            _ => {}
        }
        let vorher = (i + n - 1) % n;
        let s = &mut self.settings;
        let weiter = match c {
            Choice::Projection => {
                s.projection_mode = vorher as u8;
                ButtonAction::CycleProjection
            }
            Choice::Particles => {
                s.particles_mode = vorher as u8;
                ButtonAction::CycleParticles
            }
            Choice::DeathAnimation => {
                s.death_animation = vorher as u8;
                ButtonAction::CycleDeathAnimation
            }
            Choice::Wallpaper => {
                s.wallpaper = vorher as u8;
                ButtonAction::CycleWallpaper
            }
            Choice::Language(row) => {
                // Zeile 0 schaltet von der Sprache der Oberflaeche aus alle zusammen weiter.
                let v = i18n::Language::ALL[vorher].index();
                match row {
                    0 | 1 => s.lang_ui = v,
                    2 => s.lang_items = v,
                    3 => s.lang_effects = v,
                    _ => s.lang_chat = v,
                }
                ButtonAction::CycleLanguage(row)
            }
            Choice::HoldUseRepeat => {
                s.hold_use_repeat = HoldUseRepeat::ALL[vorher];
                ButtonAction::CycleHoldUseRepeat
            }
            Choice::ArrowEmptyFeedback => {
                s.arrow_empty_feedback = vorher as u8;
                ButtonAction::CycleArrowEmptyFeedback
            }
            Choice::SkyMode => {
                s.sky_mode = SkyMode::ALL[vorher];
                ButtonAction::CycleSkyMode
            }
            Choice::WeatherMode => {
                s.weather_mode = WeatherMode::ALL[vorher];
                ButtonAction::CycleWeatherMode
            }
            Choice::EgoDeco => {
                s.ego_deco = ego_name::Deco::ALL[vorher];
                ButtonAction::CycleEgoDeco
            }
            Choice::EgoShadow => {
                s.ego_shadow = ego_name::Deco::ALL[vorher];
                ButtonAction::CycleEgoShadow
            }
            Choice::EgoSound => {
                s.ego_sound = ego_name::SOUNDS[vorher].0.to_string();
                ButtonAction::CycleEgoSound
            }
            Choice::AudioDevice | Choice::AudioBuffer | Choice::CrosshairShape | Choice::Addon(..) => return,
        };
        if matches!(c, Choice::ArrowEmptyFeedback | Choice::SkyMode | Choice::WeatherMode | Choice::EgoDeco | Choice::EgoShadow | Choice::EgoSound) {
            self.apply_module_action(weiter);
            self.persist_settings();
        } else {
            self.run_menu_action(weiter);
        }
    }

    // Die offene Liste, sofern sie noch zur Ansicht passt, in der sie aufging.
    pub(crate) fn dropdown_active(&self) -> Option<&Dropdown> {
        self.dropdown.as_ref().filter(|d| d.mode == self.mode && d.screen == self.screen && d.module_menu == self.module_menu_shown())
    }

    // Knopf einer Auswahl angeklickt: Liste darunter auf, ein zweiter Klick schliesst sie.
    pub(crate) fn toggle_dropdown(&mut self, choice: Choice, anchor: Rect) {
        if self.dropdown_active().is_some_and(|d| d.choice == choice) {
            self.dropdown = None;
            self.play_ui_sound(ui_sound::UiSound::Close);
            return;
        }
        let (optionen, jetzt) = self.choice_options(choice);
        let sichtbar = optionen.len().min(DROPDOWN_MAX_ITEMS);
        // Der gewaehlte Eintrag steht moeglichst in der Mitte.
        let scroll = jetzt.unwrap_or(0).saturating_sub(sichtbar / 2).min(optionen.len() - sichtbar);
        self.dropdown = Some(Dropdown { choice, anchor, scroll: smooth_scroll::SmoothScroll::at(scroll as f32), mode: self.mode, screen: self.screen, module_menu: self.module_menu_shown() });
        self.play_ui_sound(ui_sound::UiSound::Open);
    }

    // Kasten der Liste und die sichtbaren Eintraege (Index, Flaeche), in Bildschirmkoordinaten:
    // unter dem Knopf, passt sie dort nicht hin, darueber. Beim gleitenden Scrollen ragen die
    // Eintraege am Rand nur halb herein -- ihre Flaeche ist dann auf den Innenraum gekuerzt
    // (s. dropdown_clip), zum Treffen wie zum Zeichnen.
    pub(crate) fn dropdown_layout(&self, d: &Dropdown) -> (Rect, Vec<(usize, Rect)>) {
        let Some(gpu) = self.gpu.as_ref() else { return (Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, Vec::new()) };
        let (sw, sh) = gpu.size();
        let optionen = self.choice_options(d.choice).0;
        let sichtbar = optionen.len().min(DROPDOWN_MAX_ITEMS);
        let breiteste = optionen.iter().map(|o| gpu.measure_text(o, UI_SCALE_SMALL)).fold(0.0, f32::max);
        let w = d.anchor.w.max(breiteste + 2.0 * DROPDOWN_PAD + 40.0).min(sw - 16.0);
        let h = sichtbar as f32 * DROPDOWN_ITEM_H + 2.0 * DROPDOWN_PAD;
        let unten = d.anchor.y + d.anchor.h + 4.0;
        let y = if unten + h <= sh - 8.0 { unten } else { (d.anchor.y - 4.0 - h).max(8.0) };
        let x = d.anchor.x.min(sw - w - 8.0).max(8.0);
        let kasten = Rect { x, y, w, h };
        let balken = if optionen.len() > sichtbar { DROPDOWN_BAR_W + DROPDOWN_PAD } else { 0.0 };
        let stand = d.scroll.stand;
        let erster = d.scroll.first();
        let eintraege = (erster..(erster + sichtbar + 1).min(optionen.len()))
            .map(|i| {
                let r = Rect { x: x + DROPDOWN_PAD, y: y + DROPDOWN_PAD + (i as f32 - stand) * DROPDOWN_ITEM_H, w: w - 2.0 * DROPDOWN_PAD - balken, h: DROPDOWN_ITEM_H };
                (i, r)
            })
            .filter(|(_, r)| dropdown_clip(kasten, *r).is_some())
            .collect();
        (kasten, eintraege)
    }

    // Linke Taste bei offener Liste: Eintrag waehlen oder daneben schliessen. Die Liste liegt ueber
    // allem; der Klick daneben tut sonst nichts (wie beim Farbfenster). true = verbraucht.
    pub(crate) fn dropdown_press(&mut self) -> bool {
        let Some(d) = self.dropdown_active() else {
            self.dropdown = None;
            return false;
        };
        let choice = d.choice;
        let (kasten, eintraege) = self.dropdown_layout(d);
        let (cx, cy) = self.cursor_pos;
        if let Some(&(i, _)) = eintraege.iter().find(|(_, r)| dropdown_clip(kasten, *r).is_some_and(|r| r.contains(cx, cy))) {
            self.dropdown = None;
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.pick_choice(choice, i);
        } else if !kasten.contains(cx, cy) {
            self.dropdown = None;
            self.play_ui_sound(ui_sound::UiSound::Close);
        }
        true
    }

    // Mausrad ueber offener Liste: eine Zeile je Raste.
    pub(crate) fn dropdown_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        let Some(choice) = self.dropdown_active().map(|d| d.choice) else { return };
        let n = self.choice_options(choice).0.len();
        let max = n.saturating_sub(DROPDOWN_MAX_ITEMS);
        let weg = self.wheel_units(delta, 1.0, DROPDOWN_ITEM_H);
        if let Some(d) = self.dropdown.as_mut() {
            d.scroll.push(weg, 1.0, max as f32);
        }
    }

    // Geschlossener Knopf einer Auswahl: Wert linksbuendig, rechts ein Pfeil (offen nach oben),
    // offen mit Akzentrand -- wie die Aufklapplisten in osu!lazer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_dropdown_face(&self, gpu: &GpuState, rect: Rect, label: &str, color: [f32; 4], scale: f32, offen: bool, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if offen {
            rects.push(UiRect::framed(rect, [0.0; 4], 6.0, 1.5, theme::get(theme::Slot::Accent)));
        }
        let text = Self::fit_text(gpu, label, rect.w - 12.0 - 32.0, scale);
        let y = (rect.y + (rect.h - gpu.text_height(scale)) / 2.0).round();
        texts.push(UiText { text, x: rect.x + 12.0, y, color, scale });
        // Pfeil aus kleinen Quadraten: V, offen ^.
        let (cx, cy) = ((rect.x + rect.w - 20.0).round(), (rect.y + rect.h / 2.0).round());
        for k in 0..5 {
            let dy = if offen { 2.0 - k as f32 } else { k as f32 - 2.0 };
            for dx in [-4.0 + k as f32, 4.0 - k as f32] {
                rects.push(UiRect { rect: Rect { x: cx + dx - 1.0, y: cy + dy - 1.0, w: 2.0, h: 2.0 }, color, ..UiRect::FLAT });
            }
        }
    }

    // Offene Liste zeichnen: Kasten mit Akzentrand, der gewaehlte Eintrag hinterlegt, der unter
    // dem Zeiger hell (mit demselben Nachleuchten wie die Knoepfe), bei Ueberlaenge ein Balken.
    pub(crate) fn build_dropdown(&self, overlay: &mut UiOverlay) {
        let (Some(gpu), Some(d)) = (self.gpu.as_ref(), self.dropdown_active()) else { return };
        use theme::Slot;
        let g = theme::get;
        let (optionen, jetzt) = self.choice_options(d.choice);
        let (kasten, eintraege) = self.dropdown_layout(d);
        let mut rects = vec![UiRect::shadow(kasten, 8.0, 8.0, 24.0, 0.55), UiRect::framed(kasten, g(Slot::BgPanel), 8.0, 1.5, g(Slot::Accent))];
        let mut texts = Vec::new();
        let (cx, cy) = self.cursor_pos;
        let innen = (kasten.y + DROPDOWN_PAD, kasten.y + kasten.h - DROPDOWN_PAD);
        for (i, r) in eintraege {
            let Some(sichtbar) = dropdown_clip(kasten, r) else { continue };
            // Nachleuchten am Eintrag, nicht an seiner Lage -- die wandert beim Gleiten.
            let schluessel = Rect { y: -100_000.0 - i as f32 * DROPDOWN_ITEM_H, ..r };
            let k = self.hover_level(schluessel, sichtbar.contains(cx, cy)).min(1.0);
            if jetzt == Some(i) {
                rects.push(UiRect::rounded(sichtbar, mix_color(g(Slot::AccentSurface), g(Slot::AccentSurfaceHover), k), 6.0));
            } else if k > 0.0 {
                rects.push(UiRect::rounded(sichtbar, theme::with_alpha(g(Slot::Surface3), k), 6.0));
            }
            let Some(name) = optionen.get(i) else { continue };
            let text = Self::fit_text(gpu, name, r.w - 24.0, UI_SCALE_SMALL);
            let y = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            // Text nur, wo er ganz hineinpasst (wie in der Serverliste).
            if y < innen.0 || y + gpu.text_height(UI_SCALE_SMALL) > innen.1 {
                continue;
            }
            let farbe = if jetzt == Some(i) { g(Slot::AccentText) } else { theme::text() };
            texts.push(UiText { text, x: r.x + 12.0, y, color: farbe, scale: UI_SCALE_SMALL });
        }
        let n = optionen.len();
        if n > DROPDOWN_MAX_ITEMS {
            let max = (n - DROPDOWN_MAX_ITEMS) as f32;
            let bahn = Rect { x: kasten.x + kasten.w - DROPDOWN_PAD - DROPDOWN_BAR_W, y: innen.0, w: DROPDOWN_BAR_W, h: innen.1 - innen.0 };
            rects.push(UiRect::rounded(bahn, g(Slot::Surface1), DROPDOWN_BAR_W / 2.0));
            let anteil = DROPDOWN_MAX_ITEMS as f32 / n as f32;
            let daumen_h = (bahn.h * anteil).max(24.0);
            let daumen_y = bahn.y + (bahn.h - daumen_h) * (d.scroll.stand / max).clamp(0.0, 1.0);
            let ueber = bahn.contains(cx, cy);
            rects.push(UiRect::rounded(Rect { y: daumen_y, h: daumen_h, ..bahn }, g(if ueber { Slot::Accent } else { Slot::Border2 }), DROPDOWN_BAR_W / 2.0));
            // Am Ende angekommen: eine Linie im Abstand zwischen Rahmen und erstem bzw. letztem
            // Eintrag, eingeblendet auf dem letzten Viertel Eintrag davor.
            let breite = kasten.w - 2.0 * DROPDOWN_PAD - DROPDOWN_BAR_W - DROPDOWN_PAD;
            for (deckung, y) in [((1.0 - d.scroll.stand * 4.0).clamp(0.0, 1.0), kasten.y + DROPDOWN_PAD / 2.0 - 1.0), ((1.0 - (max - d.scroll.stand) * 4.0).clamp(0.0, 1.0), kasten.y + kasten.h - DROPDOWN_PAD / 2.0 - 1.0)] {
                if deckung > 0.0 {
                    let linie = Rect { x: kasten.x + DROPDOWN_PAD + 6.0, y: y.round(), w: breite - 12.0, h: 2.0 };
                    rects.push(UiRect::rounded(linie, theme::with_alpha(g(Slot::Accent), deckung), 1.0));
                }
            }
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Laufendes Ziehen am Regler (s. slider_drag): bei jeder Zeigerbewegung.
    pub(crate) fn slider_drag_update(&mut self) {
        if let Some(ev) = self.slider_drag {
            self.slider_set_from_pointer(ev);
        }
    }

    // Zeichnet ein Zahlen-Wertfeld: waehrend es bearbeitet wird (editing_value) den
    // Live-Puffer linksbuendig mit blinkendem Caret (wie das Adressfeld auf dem
    // Connect-Screen), sonst den normal formatierten Wert zentriert.
    pub(crate) fn draw_editable_value(
        &self,
        gpu: &GpuState,
        rect: Rect,
        ev: EditableValue,
        mut formatted: String,
        rects: &mut Vec<UiRect>,
        texts: &mut Vec<UiText>,
    ) {
        // Als Regler (Design-System): Bahn mit gefuelltem Teil, Daumen, rechts der Wert. Das
        // Wertfeld selbst ist dann nur die Anzeige rechts (dort wird getippt).
        let rect = if let Some(r) = ev.slider() {
            let bahn = ev.slider_track();
            let max = self.slider_max(ev);
            let t = ((self.editable_number(ev) - r.min) / (max - r.min).max(1e-9)) as f32;
            let aktiv = self.slider_drag == Some(ev);
            let (qx, qy) = self.pointer();
            Self::push_slider_visual(rects, bahn, t, aktiv || ev.slider_rect().contains(qx, qy), aktiv, true);
            // Mit gehaltener rechter Taste: das rechte Ende klein unter der Bahn.
            if aktiv && self.slider_extended {
                let text = format!("max {max:.*}", r.decimals);
                let w = gpu.measure_text(&text, TOPBAR_SUB_SCALE);
                texts.push(UiText { text, x: (bahn.x + bahn.w - w).round(), y: (bahn.y + 9.0).round(), color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
            }
            let (kasten, zahl) = push_slider_unit(gpu, ev.slider_value_rect(), &formatted, texts);
            formatted = zahl;
            kasten
        } else {
            rect
        };
        rects.push(UiRect::framed(rect, theme::get(theme::Slot::SurfaceSunken), 6.0, 1.0, theme::get(theme::Slot::Border1)));
        if let Some((editing_ev, buf)) = &self.editing_value {
            if *editing_ev == ev {
                self.push_value_input(gpu, rect, formatted, buf, rects, texts);
                return;
            }
        }
        texts.push(gpu.centered_text(rect, formatted, theme::text(), UI_SCALE_SMALL));
    }

    // Das offene Eingabefeld uebernehmen (OK-Knopf oder Enter).
    pub(crate) fn submit_input_field(&mut self) {
        let Some((kind, text)) = self.input_field.take() else { return };
        match kind {
            InputKind::Token => self.submit_token_login(text, accounts::Source::Token),
            InputKind::RefreshToken => self.submit_refresh_token_login(text),
            InputKind::ConfigName(edit) => {
                // Vorher speichern: Duplizieren kopiert die Datei, und die soll den Stand von
                // jetzt tragen.
                self.persist_settings();
                let aktiv = active_config();
                let ergebnis = match edit {
                    ConfigEdit::New => assets::create_config(&text, None),
                    ConfigEdit::Duplicate => assets::create_config(&text, Some(&aktiv)),
                    ConfigEdit::Rename => assets::rename_config(&aktiv, &text),
                };
                match ergebnis {
                    Ok(name) if edit == ConfigEdit::Rename => {
                        set_active_config(&name);
                        assets::set_last_used(&name);
                    }
                    Ok(name) => self.switch_config(&name),
                    Err(e) => {
                        // Feld mit dem Getippten wieder auf, damit man nur korrigieren muss.
                        self.input_field = Some((kind, text));
                        self.notify(notifications::Notification::error("Config", e.to_string()));
                    }
                }
            }
        }
    }

    // Im laufenden Spiel zu einer anderen Config wechseln: die aktuelle speichern, die andere
    // laden und alles nachziehen, was sonst nur beim Start oder beim Klick auf die jeweilige
    // Einstellung passiert.
    pub(crate) fn switch_config(&mut self, name: &str) {
        if name == active_config() {
            return;
        }
        self.persist_settings();
        self.switch_config_unsaved(name);
    }

    pub(crate) fn switch_config_unsaved(&mut self, name: &str) {
        set_active_config(name);
        assets::set_last_used(name);
        let mut neu = Settings::load();
        let alt = std::mem::take(&mut self.settings);
        // Eine Config ohne Anmeldung uebernimmt die laufende: man bleibt eingeloggt, statt
        // beim naechsten Verbinden ploetzlich ohne Konto dazustehen.
        if neu.mc_access_token.is_empty() {
            neu.account_name = alt.account_name.clone();
            neu.account_uuid = alt.account_uuid.clone();
            neu.mc_access_token = alt.mc_access_token.clone();
            neu.msa_refresh_token = alt.msa_refresh_token.clone();
        } else {
            self.credentials = Some(auth::Credentials {
                access_token: neu.mc_access_token.clone(),
                profile_id: neu.account_uuid.clone(),
                username: neu.account_name.clone(),
            });
        }
        self.settings = neu;
        self.settings_scroll = Default::default();
        self.page_scrolls.clear();
        self.apply_switched_settings(&alt);
        self.persist_settings();
        self.notify(notifications::Notification::success("Config gewechselt", format!("Aktiv: {name}")));
    }

    // Was eine geaenderte Einstellung ausserhalb von self.settings nach sich zieht -- fuer
    // den Config-Wechsel, bei dem sich alles auf einmal aendern kann.
    pub(crate) fn apply_switched_settings(&mut self, alt: &Settings) {
        self.apply_frame_latency();
        self.mesh_pool.resize(self.settings.chunk_update_threads);
        if self.settings.audio_output_device != alt.audio_output_device {
            let geraet = (!self.settings.audio_output_device.is_empty()).then_some(self.settings.audio_output_device.as_str());
            if let Err(e) = self.audio.set_output_device(geraet) {
                eprintln!("[audio] Ausgabegerät der Config nicht verfügbar: {e}");
                let _ = self.audio.set_output_device(None);
            }
        }
        if self.settings.font != alt.font {
            self.hud_latched_widths.borrow_mut().clear();
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.stable_numbers.borrow_mut().clear();
                if let Err(e) = gpu.font_atlas.switch(&gpu.device, &gpu.queue, &mut self.installation, FontKind::from_index(self.settings.font)) {
                    eprintln!("Schrift konnte nicht gewechselt werden: {e}");
                }
            }
        }
        self.sync_text_shadow();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.stable_numbers.borrow_mut().set_enabled(self.settings.stable_numbers);
        }
        apply_language_settings(&mut self.installation, &self.settings);
        self.input_log.set_enabled(self.settings.module_input_log_enabled);
        self.publish_totem_sound_enabled();
        self.apply_light_factors();
        // Renderpfad, Block-Rotation, weiche Beleuchtung, Markierungen: alles steckt im Mesh.
        self.block_uvs.set_rng_rotation_enabled(self.settings.block_rng_rotation);
        self.world.light_remesh = self.settings.render_path == 1;
        world::mesher::set_flat_face_shade(self.settings.render_path == 1);
        for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
            self.world.mark_dirty(cx, cz);
        }
        self.menu_world_frozen = false;
        if self.settings.active_skins != alt.active_skins || self.skin_exclusions_of(&self.settings.active_skins) != self.loaded_skin_exclusions {
            self.selected_skins = self.settings.active_skins.clone();
            self.reload_resource_pack(self.settings.active_skins.clone());
        }
    }

    // Lichtstaerken an die Welt geben: das Blocklicht steckt im Mesh, die Quellen im
    // Voxel-Abbild -- beides muss neu entstehen.
    pub(crate) fn apply_light_factors(&mut self) {
        world::light::set_light_factors(self.settings.light_factor_table());
        for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
            self.world.mark_dirty(cx, cz);
        }
        if let Some(sp) = self.gpu.as_mut().and_then(|g| g.shader_path.as_mut()) {
            sp.voxels_mut().mark_all_stale();
        }
    }

    // Treffer der Lichtstaerken-Suche: Indizes in light_source_blocks().
    pub(crate) fn light_search_hits(&self) -> Vec<usize> {
        let suche = self.light_search.trim().to_lowercase();
        light_source_blocks()
            .iter()
            .enumerate()
            .filter(|(_, id)| suche.is_empty() || id.contains(&suche) || block_display_name(id).to_lowercase().contains(&suche))
            .map(|(i, _)| i)
            .collect()
    }


    pub(crate) fn particle_max_scroll(&self) -> f32 {
        particles::KINDS.len().saturating_sub(self.particle_visible_rows()) as f32
    }

    pub(crate) fn light_max_scroll(&self) -> f32 {
        self.light_search_hits().len().saturating_sub(self.light_visible_rows()) as f32
    }

    pub(crate) fn particle_visible_rows(&self) -> usize {
        self.light_visible_rows()
    }

    pub(crate) fn light_visible_rows(&self) -> usize {
        let h = self.gpu.as_ref().map_or(1080.0, |g| g.size().1);
        settings_visible_rows(h).saturating_sub(1).max(1)
    }

    // Nach jeder Aenderung sofort schreiben. Das sind ein paar Dutzend Byte bei einem
    // Klick -- billiger als jede Loesung, die sich merken muesste, wann etwas "offen" ist.
    // Den allgemeinen Rand samt Staerke an die GPU geben (s. GpuState::text_shadow_style).
    pub(crate) fn sync_text_shadow(&mut self) {
        let stil = self.settings.global_text_shadow();
        let staerke = self.settings.shadow_strength(0, stil);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.text_shadow_style = stil;
            gpu.text_shadow_strength = staerke;
        }
    }

    pub(crate) fn persist_settings(&mut self) {
        // Waehrend ein Regler Schritte ausfuehrt, nicht bei jedem Schritt schreiben.
        if self.persist_paused {
            return;
        }
        // Scrollstaende mitschreiben -- sie aendern sich ohne eigenes Speichern (Mausrad).
        if self.scrolls_loaded {
            let staende: Vec<(&str, usize)> = self.scroll_slots().into_iter().map(|(n, w)| (n, w.ziel.max(0.0).round() as usize)).collect();
            for (name, wert) in staende {
                self.settings.settings_scrolls.insert(name.to_string(), wert);
            }
        }
        // Gespeichert wird still; nur ein Fehlschlag meldet sich (einmal, nicht bei jedem
        // weiteren Versuch mit demselben Fehler).
        let status = match self.settings.save() {
            Ok(_) => SaveStatus::Saved,
            Err(e) => SaveStatus::Failed(e.to_string()),
        };
        if let SaveStatus::Failed(e) = &status {
            if !matches!(&self.settings_status, Some(SaveStatus::Failed(alt)) if alt == e) {
                self.notify(notifications::Notification::error("Speichern fehlgeschlagen", e.clone()));
            }
        }
        self.settings_status = Some(status);
    }

    // Traegt das gespeicherte Render-Ahead-Limit in die Surface-Konfiguration ein. Anders
    // ohne Treiber-Rueckfrage moeglich (s. GpuState::set_frame_latency).
    pub(crate) fn apply_frame_latency(&mut self) {
        let wanted = self.settings.frame_latency;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_frame_latency(wanted);
        }
    }

    // Die Schriften, die HUD-Zahlen (Schild-Countdown, Pfeile) oder Bereiche (Chat, Inventar,
    // ...) statt der allgemeinen wollen -- dafuer haelt GpuState weitere Atlanten bereit (s.
    // GpuState::ensure_fonts).
    pub(crate) fn wanted_fonts(&self) -> Vec<FontKind> {
        let set = &self.settings;
        let allgemein = FontKind::from_index(set.font);
        let mut fonts: Vec<FontKind> = Vec::new();
        // Der Schrift-Tab zeigt jede Schrift zur Probe.
        let probe = (self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsFont).then_some(FontKind::ALL).into_iter().flatten();
        // Kleine Beschriftungen stehen nie in der Minecraft-Schrift (s. GpuState::small_label_font).
        let klein = (allgemein == FontKind::Minecraft).then_some(FontKind::Outfit);
        for f in ShadowArea::ALL.into_iter().filter_map(|a| set.font_of(a)).chain(probe).chain(klein) {
            if f != allgemein && !fonts.contains(&f) {
                fonts.push(f);
            }
        }
        fonts
    }

    // Anfang und Ende eines Textbereichs mit eigenem Rand und eigener Schrift (s.
    // Settings::area_shadow/area_font): Marker fuer shape_text, und bis end_text_area misst
    // GpuState mit der Schrift des Bereichs.
    pub(crate) fn begin_text_area(&self, gpu: &GpuState, texts: &mut Vec<UiText>, area: ShadowArea) {
        push_shadow_marker(texts, self.settings.shadow_marker_of(area));
        let font = self.settings.font_of(area);
        push_font_marker(texts, font);
        gpu.set_layout_font(font);
        if area.plain_numbers() {
            push_plain_numbers_marker(texts, true);
            gpu.plain_numbers.set(true);
        }
    }

    pub(crate) fn end_text_area(gpu: &GpuState, texts: &mut Vec<UiText>) {
        push_shadow_marker(texts, None);
        push_font_marker(texts, None);
        gpu.set_layout_font(None);
        push_plain_numbers_marker(texts, false);
        gpu.plain_numbers.set(false);
    }

    // Fuer Rechnungen ausserhalb des Aufbaus (Klicks, Auswahl, Blaettern): bis der Waechter
    // faellt, misst GpuState mit der Schrift des Bereichs -- dieselben Breiten wie beim Zeichnen.
    pub(crate) fn area_font_scope<'a>(&self, gpu: &'a GpuState, area: ShadowArea) -> LayoutFontScope<'a> {
        let (vorher, ruhig_vorher) = (gpu.layout_font.get(), gpu.plain_numbers.get());
        gpu.set_layout_font(self.settings.font_of(area));
        gpu.plain_numbers.set(area.plain_numbers());
        LayoutFontScope { gpu, vorher, ruhig_vorher }
    }

    // Welcher klickbare Knopf liegt gerade unter dem Zeiger? Nur dort, wo es auch einen
    // gezeichneten Zeiger gibt -- im Weltmodus ohne Overlay steuert die Maus den Blick, ein
    // Hover-Klang waere dort sinnlos.
    // Zeiger im Koordinatenraum des Inhalts: auf den Einstellungsseiten ist der Inhalt um
    // content_offset verschoben (Leiste oben, Kategorien links), seine Rechtecke stehen aber
    // weiter in den alten Koordinaten. Ueberall sonst gleich cursor_pos.
    pub(crate) fn pointer(&self) -> (f32, f32) {
        // Offene Kontenliste liegt ueber dem Inhalt: darunter zeigt nichts auf etwas.
        if self.pointer_blocked() {
            return (f32::NEG_INFINITY, f32::NEG_INFINITY);
        }
        let (dx, dy) = self.content_offset();
        (self.cursor_pos.0 - dx, self.cursor_pos.1 - dy)
    }

    // Liegt etwas ueber dem Inhalt, das den Zeiger fuer sich hat: die offene Kontenliste, oder
    // der Zeiger steht auf der offenen Benachrichtigungsliste.
    pub(crate) fn pointer_blocked(&self) -> bool {
        self.account_menu_open || self.notification_panel_rect().is_some_and(|r| self.notification_panel_open && r.contains(self.cursor_pos.0, self.cursor_pos.1))
    }

    pub(crate) fn content_offset(&self) -> (f32, f32) {
        if self.mode != AppMode::Menu {
            (0.0, 0.0)
        } else if self.screen.settings_category().is_some() {
            (SETTINGS_CONTENT_DX, SETTINGS_CONTENT_DY)
        } else if self.screen == MenuScreen::Account {
            // Der Zurueck-Knopf sass frueher oben links, wo jetzt die Leiste liegt.
            (0.0, SETTINGS_CONTENT_DY)
        } else if let Some(panel) = self.hud_config_panel() {
            // Einstellungsfenster im HUD-Editor: in die Bildmitte.
            let (sw, sh) = self.gpu.as_ref().map_or((1920.0, 1080.0), |g| g.size());
            (((sw - panel.w) / 2.0 - panel.x).round(), ((sh - panel.h) / 2.0 - panel.y).round())
        } else {
            (0.0, 0.0)
        }
    }

    // Das offene Einstellungsfenster des HUD-Editors in Inhaltskoordinaten, None = keins.
    pub(crate) fn hud_config_panel(&self) -> Option<Rect> {
        let element = self.hud_selected.filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::HudEditor && self.crosshair_painter.is_none())?;
        Some(hud_config_panel(element, &self.settings))
    }

    // Liegt der Bildschirmpunkt im offenen Einstellungsfenster?
    pub(crate) fn hud_config_panel_contains(&self, (x, y): (f32, f32)) -> bool {
        let Some(panel) = self.hud_config_panel() else { return false };
        let (dx, dy) = self.content_offset();
        Rect { x: panel.x + dx, y: panel.y + dy, ..panel }.contains(x, y)
    }

    // Leiste oben: auf allen Menue-Screens ausser dort, wo der ganze Bildschirm gebraucht wird.
    pub(crate) fn chrome_visible(&self) -> bool {
        self.mode == AppMode::Menu && !matches!(self.screen, MenuScreen::HudEditor | MenuScreen::Death)
    }

    // Scrollstaende, die beim Verlassen erhalten bleiben (s. Settings::settings_scrolls).
    pub(crate) fn scroll_slots(&mut self) -> [(&'static str, &mut smooth_scroll::SmoothScroll); 7] {
        let [links, rechts] = &mut self.skin_scroll;
        [
            ("render", &mut self.settings_scroll),
            ("controls", &mut self.keybind_scroll),
            ("skin", links),
            ("skin_right", rechts),
            ("modules", &mut self.module_menu_scroll),
            ("light_levels", &mut self.light_scroll),
            ("particles", &mut self.particle_scroll),
        ]
    }

    // Einstellungen oeffnen: MenuScreen::Settings heisst "die zuletzt offene Kategorie",
    // beim allerersten Mal die Einrichtung. Gemerkt wird die Kategorie sofort.
    pub(crate) fn open_settings(&mut self, target: MenuScreen) {
        if !self.scrolls_loaded {
            let gespeichert = self.settings.settings_scrolls.clone();
            for (name, feld) in self.scroll_slots() {
                if let Some(&v) = gespeichert.get(name) {
                    feld.jump(v as f32);
                }
            }
            self.scrolls_loaded = true;
        }
        let target = if target == MenuScreen::Settings {
            MenuScreen::from_settings_key(&self.settings.settings_category).unwrap_or(MenuScreen::SettingsSetup)
        } else {
            target
        };
        if self.screen.settings_category().is_none() && self.screen != MenuScreen::Settings {
            self.settings_return = self.screen;
        }
        self.screen = target;
        if let Some(kategorie) = target.settings_category() {
            if self.settings.settings_category != kategorie.settings_key() {
                self.settings.settings_category = kategorie.settings_key().to_string();
            }
        }
        self.persist_settings();
    }

    // Die Kategorien in der Leiste: alphabetisch nach angezeigtem Namen, die Einrichtung
    // ausserhalb davon ganz unten (s. sidebar_item_rect).
    pub(crate) fn sidebar_categories() -> Vec<(MenuScreen, &'static str)> {
        // Dazu der HUD-Editor: keine Einstellungsseite (er braucht den ganzen Bildschirm),
        // aber von hier aus zu oeffnen; Zurueck fuehrt wieder hierher.
        let mut kategorien = SETTINGS_CATEGORIES.to_vec();
        kategorien.push((MenuScreen::HudEditor, "HUD"));
        kategorien.sort_by_key(|&(_, label)| i18n::tr(label).to_lowercase());
        kategorien
    }

    pub(crate) fn sidebar_item_rect(i: usize, screen_h: f32) -> Rect {
        Rect { x: SIDEBAR_PAD, y: TOPBAR_H + 16.0 + i as f32 * (SIDEBAR_ITEM_H + 2.0), w: SIDEBAR_W - 2.0 * SIDEBAR_PAD, h: SIDEBAR_ITEM_H }
            .clamp_bottom(screen_h)
    }

    // Trefferflaeche eines Eintrags: die ganze Breite der Leiste und je die halbe Luecke zu den
    // Nachbarn, damit ein Klick zwischen zwei Eintraegen nicht ins Leere geht.
    pub(crate) fn sidebar_hit_rect(r: Rect) -> Rect {
        Rect { x: 0.0, y: r.y - 1.0, w: SIDEBAR_W, h: r.h + 2.0 }
    }

    pub(crate) fn sidebar_setup_rect(screen_h: f32) -> Rect {
        Rect { x: SIDEBAR_PAD, y: screen_h - SIDEBAR_PAD - SIDEBAR_ITEM_H, w: SIDEBAR_W - 2.0 * SIDEBAR_PAD, h: SIDEBAR_ITEM_H }
    }

    // Logo `i` aus SIDEBAR_LINKS: quadratisch, nebeneinander direkt ueber der Linie vor
    // "Setup"; die Version steht rechts daneben.
    pub(crate) fn sidebar_link_rect(i: usize, screen_h: f32) -> Rect {
        let setup = Self::sidebar_setup_rect(screen_h);
        Rect { x: SIDEBAR_PAD + i as f32 * (SIDEBAR_ITEM_H + 4.0), y: setup.y - 18.0 - SIDEBAR_ITEM_H, w: SIDEBAR_ITEM_H, h: SIDEBAR_ITEM_H }
    }

    // Rechte Haelfte der Leiste oben, von rechts nach links: Glocke, Uhr, Kopf, Name.
    pub(crate) fn topbar_layout(&self, gpu: &GpuState) -> TopbarLayout {
        let (sw, _) = gpu.size();
        let y = ((TOPBAR_H - TOPBAR_ICON) / 2.0).round();
        let bell = Rect { x: sw - 12.0 - TOPBAR_ICON, y, w: TOPBAR_ICON, h: TOPBAR_ICON };
        let mut uhr_w = gpu.measure_text("00:00:00", UI_SCALE_SMALL);
        if self.settings.topbar_uptime {
            uhr_w = uhr_w.max(gpu.measure_text("läuft seit 00:00", TOPBAR_SUB_SCALE));
        }
        let uhr = Rect { x: bell.x - 20.0 - uhr_w, y, w: uhr_w, h: TOPBAR_ICON };
        let kopf = Rect { x: uhr.x - 24.0 - 32.0, y: ((TOPBAR_H - 32.0) / 2.0).round(), w: 32.0, h: 32.0 };
        let name = match self.switching_account() {
            Some(a) => a.name.clone(),
            None => self.credentials.as_ref().map_or_else(|| "Nicht angemeldet".to_string(), |c| c.username.clone()),
        };
        let name_w = gpu.measure_text(&name, UI_SCALE_SMALL);
        let mut konto = Rect { x: kopf.x - 10.0 - name_w - 10.0, y, w: name_w + 10.0 + kopf.w + 20.0, h: TOPBAR_ICON };
        let konto_links = konto.x;
        // Bei offener Kontenliste steht "Acc-Manager" im Knopf (in Englisch laenger als ein
        // kurzer Name): dann nach links verbreitern, rechts bleibt er stehen.
        if self.account_menu_open {
            let noetig = gpu.measure_text("Acc-Manager", UI_SCALE_SMALL) + 32.0;
            if noetig > konto.w {
                konto = Rect { x: konto.x + konto.w - noetig, w: noetig, ..konto };
            }
        }
        let gear = Rect { x: 12.0, y, w: TOPBAR_ICON, h: TOPBAR_ICON };
        let gear_hit = Rect { x: 0.0, y: 0.0, w: gear.x + gear.w, h: TOPBAR_H };
        let bell_hit = Rect { x: bell.x, y: 0.0, w: sw - bell.x, h: TOPBAR_H };
        TopbarLayout { gear, bell, gear_hit, bell_hit, uhr, kopf, konto, konto_links, name }
    }

    // Klickbares der Leiste oben und der Kategorienleiste -- in Bildschirmkoordinaten.
    pub(crate) fn chrome_buttons(&self) -> Vec<Button> {
        let mut out = Vec::new();
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.chrome_visible()) else { return out };
        let (_, sh) = gpu.size();
        let knopf = |action, rect| Button { action, rect, label: String::new(), enabled: true, color: [0.0; 4], scale: UI_SCALE_SMALL, slider: None };
        let l = self.topbar_layout(gpu);
        out.push(knopf(ButtonAction::ToggleSettings, l.gear_hit));
        out.push(knopf(ButtonAction::ToggleAccountMenu, l.konto));
        out.push(knopf(ButtonAction::ToggleNotificationPanel, l.bell_hit));
        out.push(knopf(ButtonAction::ToggleUptime, l.uhr));
        // Das Kreuz vor seinem Tab, damit es zuerst getroffen wird.
        let (tabs, plus) = self.tab_rects(&l);
        for (i, r) in tabs.iter().enumerate() {
            out.push(knopf(ButtonAction::CloseTab(i), r.close));
            out.push(knopf(ButtonAction::SelectTab(i), r.tab));
        }
        out.push(knopf(ButtonAction::NewTab, plus));
        for (i, (action, _, _)) in self.bottom_tiles().into_iter().enumerate() {
            out.push(knopf(action, bottom_tile_rect(i)));
        }
        if self.screen.settings_category().is_some() {
            for (i, (kategorie, _)) in Self::sidebar_categories().into_iter().enumerate() {
                out.push(knopf(ButtonAction::OpenScreen(kategorie), Self::sidebar_hit_rect(Self::sidebar_item_rect(i, sh))));
            }
            out.push(knopf(ButtonAction::OpenScreen(MenuScreen::SettingsSetup), Self::sidebar_hit_rect(Self::sidebar_setup_rect(sh))));
            for i in 0..SIDEBAR_LINKS.len() {
                out.push(knopf(ButtonAction::OpenLink(i), Self::sidebar_link_rect(i, sh)));
            }
        }
        if self.screen == MenuScreen::SettingsSkin && self.skin_files.is_none() && self.skin_drag_active().is_none() {
            out.extend(self.skin_strip(gpu).0);
        }
        out
    }

    // Die Knoepfe des aktuellen Screens in Bildschirmkoordinaten (s. content_offset). Auf
    // einer Kategorie der Einstellungen faellt der Zurueck-Knopf weg -- die Leiste links
    // ersetzt ihn.
    pub(crate) fn content_buttons(&self) -> Vec<Button> {
        let mut out = self.screen_buttons();
        // Hinter dem Fenster "Erweitert" ist die Seite gesperrt, auch ihre Reiter.
        if self.keybind_advanced.is_none() {
            out.extend(self.section_tabs());
        }
        // Wo ein Schieberegler steht, ersetzt er die Pfeile des alten Steppers.
        if self.mode == AppMode::Menu {
            let regler: Vec<Rect> = EditableValue::ALL
                .into_iter()
                .filter(|ev| ev.screen() == self.screen && ev.slider().is_some())
                .map(|ev| ev.slider_rect())
                .collect();
            out.retain(|b| {
                let (mx, my) = (b.rect.x + b.rect.w / 2.0, b.rect.y + b.rect.h / 2.0);
                !regler.iter().any(|r| r.contains(mx, my))
            });
        }
        if self.mode == AppMode::Menu && self.screen.is_settings_category() {
            out.retain(|b| !(b.action == ButtonAction::Back && b.rect.x == BACK_BUTTON.x && b.rect.y == BACK_BUTTON.y));
            // Was gerade nicht geht, steht in den Einstellungen gar nicht erst da.
            out.retain(|b| b.enabled);
        }
        let (dx, dy) = self.content_offset();
        for b in &mut out {
            b.rect.x += dx;
            b.rect.y += dy;
        }
        out
    }

    // Reiter oben auf Einstellungsseiten mit mehreren Abschnitten (s. SettingsPage), der
    // aktive mit Akzentlinie (s. styled_button).
    pub(crate) fn section_tabs(&self) -> Vec<Button> {
        let page = match self.screen.settings_page() {
            Some(p) if self.mode == AppMode::Menu && p.sections.len() >= 2 => p,
            _ => return Vec::new(),
        };
        let aktiv = self.active_section(page);
        let mut x = SETTINGS_LABEL_X;
        let mut out = Vec::new();
        for (i, &(label, _)) in page.sections.iter().enumerate() {
            let w = self.gpu.as_ref().map_or(100.0, |g| g.measure_text(label, UI_SCALE_SMALL)) + 24.0;
            out.push(Button {
                action: ButtonAction::SettingsSection(i as u8),
                rect: Rect { x, y: SETTINGS_FIRST_Y - 44.0, w, h: 40.0 },
                label: label.to_string(),
                enabled: true,
                color: if i == aktiv { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                scale: UI_SCALE_SMALL,
                slider: None,
            });
            x += w + SECTION_TAB_GAP;
        }
        out
    }

    pub(crate) fn buttons(&self) -> Vec<Button> {
        let mut out = self.content_buttons();
        out.extend(self.chrome_buttons());
        out
    }

    // Kacheln der Leiste unten (nur Server-Seite): Aktion, Beschriftung, Farbe des Streifens.
    pub(crate) fn bottom_tiles(&self) -> Vec<(ButtonAction, &'static str, [f32; 4])> {
        use theme::Slot;
        if self.screen != MenuScreen::Connect {
            return Vec::new();
        }
        vec![
            (ButtonAction::OpenQuickConnect, "Quick Connect", theme::get(Slot::Accent)),
            (ButtonAction::NewServer, "Neuer Eintrag", theme::get(Slot::Success)),
        ]
    }

    // Leiste unten wie in osu!: Kacheln mit runder Oberkante und farbigem Streifen unten.
    pub(crate) fn build_bottom_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        rects.push(UiRect { rect: Rect { x: 0.0, y: sh - BOTTOMBAR_H, w: sw, h: BOTTOMBAR_H }, color: g(Slot::BgPanel), ..UiRect::FLAT });
        rects.push(UiRect { rect: Rect { x: 0.0, y: sh - BOTTOMBAR_H, w: sw, h: 1.0 }, color: g(Slot::Border1), ..UiRect::FLAT });
        for (i, (_, label, farbe)) in self.bottom_tiles().into_iter().enumerate() {
            let r = bottom_tile_rect(i);
            let hover = r.contains(cx, cy);
            // Unten ueber den Rand hinaus, damit nur die oberen Ecken rund sind.
            let flaeche = Rect { h: r.h + 12.0, ..r };
            rects.push(UiRect::framed(flaeche, if hover { g(Slot::Surface3) } else { g(Slot::Surface2) }, 8.0, 1.0, g(Slot::Border1)));
            rects.push(UiRect { rect: Rect { x: r.x + 1.0, y: r.y + r.h - 5.0, w: r.w - 2.0, h: 5.0 }, color: farbe, ..UiRect::FLAT });
            texts.push(gpu.centered_text(Rect { h: r.h - 5.0, ..r }, label.to_string(), theme::text(), UI_SCALE_SMALL));
        }
    }

    // Reihenfolge der gespeicherten Konten in Liste und Acc-Manager (s. accounts::display_order).
    pub(crate) fn account_order(&self) -> Vec<usize> {
        accounts::display_order(&self.accounts.list, |a| self.account_check(a), self.credentials.as_ref().map(|c| c.profile_id.as_str()))
    }

    // Kontenliste unter dem Konto-Knopf: zuerst das angemeldete Konto, dann die uebrigen,
    // ungueltige zuletzt. Some = Index in accounts.list; None = ein angemeldetes Konto, das nicht
    // in der Liste steht. Ohne Anmeldung gibt es keine eigene Zeile, es ist nur keins markiert.
    pub(crate) fn account_menu_rows(&self, gpu: &GpuState) -> Vec<(Rect, Option<usize>)> {
        let l = self.topbar_layout(gpu);
        let w = l.konto.w.max(340.0);
        let x = l.konto.x + l.konto.w - w;
        let mut zeilen = Vec::new();
        if self.credentials.as_ref().is_some_and(|c| self.accounts.find(&c.profile_id).is_none()) {
            zeilen.push(None);
        }
        zeilen.extend(self.account_order().into_iter().map(Some));
        zeilen.into_iter().enumerate().map(|(n, z)| (Rect { x, y: TOPBAR_H + 4.0 + n as f32 * 48.0, w, h: 44.0 }, z)).collect()
    }

    // Kopf eines Kontos in `bild`: Gesicht + Hutlage aus dem Skin; waehrend der Pruefung der
    // Ladekreis an seiner Stelle, ein ungueltiges Konto blass.
    pub(crate) fn push_account_head(&self, a: Option<&accounts::Account>, uuid: Option<[u8; 16]>, bild: Rect, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>) {
        use theme::Slot;
        let g = theme::get;
        rects.push(UiRect::rounded(Rect { x: bild.x - 2.0, y: bild.y - 2.0, w: bild.w + 4.0, h: bild.h + 4.0 }, g(Slot::Border2), 4.0));
        // Ladepunkte nur, solange es noch keinen Kopf gibt -- eine erneute Pruefung laesst ihn stehen.
        let kopf_da = uuid.is_some_and(|k| self.skins.get(&k).is_some_and(|s| s.slot.is_some()));
        let pruefung = a.and_then(|a| self.account_checks.get(&accounts::key(&a.uuid))).filter(|(c, _)| *c == accounts::Check::Checking && !kopf_da).map(|(_, seit)| seit);
        // Auf dieses Konto wird gerade gewechselt: Ladepunkte, bis die Anmeldung durch ist.
        let wechsel = self.account_switching.as_ref().filter(|(k, _)| a.is_some_and(|a| accounts::key(&a.uuid) == *k)).map(|(_, seit)| seit);
        if let Some(seit) = wechsel.or(pruefung) {
            rects.push(UiRect::rounded(bild, g(Slot::SurfaceSunken), 3.0));
            rects.extend(loading_dots(bild, seit.elapsed().as_secs_f32(), g(Slot::Accent)));
            return;
        }
        let blass = a.is_some_and(|a| matches!(self.account_check(a), accounts::Check::Invalid(_)));
        let tint = if blass { [1.0, 1.0, 1.0, 0.35] } else { [1.0; 4] };
        let uuid = uuid.unwrap_or([0; 16]);
        for (px, py) in [(8.0, 8.0), (40.0, 8.0)] {
            sprites.push(UiSprite::quad(bild.x, bild.y, bild.w, bild.h, self.skin_face_uv(&uuid, px, py), tint, SpriteAtlas::Entity));
        }
    }

    pub(crate) fn build_account_menu(&self, overlay: &mut UiOverlay) {
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.account_menu_open && self.chrome_visible()) else { return };
        use theme::Slot;
        let g = theme::get;
        let zeilen = self.account_menu_rows(gpu);
        let (Some(erste), Some(letzte)) = (zeilen.first(), zeilen.last()) else { return };
        let panel = Rect { x: erste.0.x - 6.0, y: erste.0.y - 6.0, w: erste.0.w + 12.0, h: letzte.0.y + letzte.0.h - erste.0.y + 12.0 };
        let mut rects = vec![UiRect::shadow(panel, 8.0, 8.0, 24.0, 0.5), UiRect::framed(panel, g(Slot::Surface1), 8.0, 1.0, g(Slot::Border1))];
        let mut texts = Vec::new();
        let mut sprites = Vec::new();
        let (cx, cy) = self.cursor_pos;
        for (r, z) in zeilen {
            let konto = z.and_then(|i| self.accounts.list.get(i));
            let (name, aktiv, uuid) = match konto {
                None => (
                    self.credentials.as_ref().map_or_else(String::new, |c| c.username.clone()),
                    true,
                    self.credentials.as_ref().and_then(|c| uuid_bytes_from_hex(&c.profile_id)),
                ),
                Some(a) => (a.name.clone(), self.shown_as_active(a), uuid_bytes_from_hex(&a.uuid)),
            };
            let ungueltig = konto.is_some_and(|a| matches!(self.account_check(a), accounts::Check::Invalid(_)));
            // Weich ein- und ausgeblendet wie in den Aufklapplisten; hover_level meldet den
            // Hover auch fuer den Klang an.
            let k = self.hover_level(r, r.contains(cx, cy)).min(1.0);
            if aktiv {
                rects.push(UiRect::rounded(r, mix_color(g(Slot::AccentSurface), g(Slot::AccentSurfaceHover), k), 6.0));
            } else if k > 0.0 {
                rects.push(UiRect::rounded(r, theme::with_alpha(g(Slot::Surface2), k), 6.0));
            }
            // Kopf aus dem Skin wie oben in der Leiste; solange der Skin eines Kontos noch
            // laedt, schwarz-lila (s. request_account_heads).
            let bild = Rect { x: r.x + 8.0, y: r.y + 6.0, w: 32.0, h: 32.0 };
            self.push_account_head(konto, uuid, bild, &mut rects, &mut sprites);
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            let farbe = if aktiv && !ungueltig {
                g(Slot::AccentText)
            } else if ungueltig {
                theme::text_3()
            } else {
                theme::text()
            };
            texts.push(UiText { text: name, x: bild.x + bild.w + 12.0, y: ty, color: farbe, scale: UI_SCALE_SMALL });
            if let Some(a) = konto {
                self.push_account_info(gpu, a, r, &mut texts);
            }
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites, pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Linke Taste bei offener Kontenliste. true = verbraucht (Klick in der Liste oder daneben).
    pub(crate) fn account_menu_press(&mut self) -> bool {
        let Some(gpu) = self.gpu.as_ref() else { return false };
        let (cx, cy) = self.cursor_pos;
        // Der Knopf selbst (jetzt Acc-Manager) laeuft normal ueber buttons().
        if self.topbar_layout(gpu).konto.contains(cx, cy) {
            return false;
        }
        let treffer = self.account_menu_rows(gpu).into_iter().find(|(r, _)| r.contains(cx, cy));
        self.account_menu_open = false;
        if let Some((_, Some(i))) = treffer {
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.use_account(i);
        }
        true
    }

    // Acc-Manager: alle Teile des Fensters, fuer Zeichnen und Klicks aus EINER Quelle.
    pub(crate) fn account_manager_layout(&self, gpu: &GpuState) -> (Rect, Vec<AccElem>) {
        let (sw, sh) = gpu.size();
        let w = 600.0;
        let pad = 24.0;
        let innen = w - 2.0 * pad;
        let mut el = Vec::new();
        let mut y = 20.0 + gpu.text_height(UI_SCALE_NORMAL) + 12.0 + gpu.text_height(UI_SCALE_SMALL) + 24.0;
        // Zwei Gruppen mit eigener Ueberschrift, leere fallen weg: echte Konten (erneuern sich
        // selbst) und eingefuegte Tokens (laufen ab). Ein Klick meldet an, ein Rechtsklick
        // oeffnet "Ausloggen"/"Account vergessen".
        let ordnung = self.account_order();
        let fremd = self.credentials.as_ref().filter(|c| self.accounts.find(&c.profile_id).is_none());
        let mut erste = true;
        for (quelle, titel) in [(accounts::Source::Microsoft, "KONTEN"), (accounts::Source::Token, "TOKENS")] {
            let zeilen: Vec<usize> = ordnung.iter().copied().filter(|&i| self.accounts.list[i].source == quelle).collect();
            let konten = quelle == accounts::Source::Microsoft;
            // Ohne jedes Konto steht "Konten" mit dem Hinweis da.
            let leer = ordnung.is_empty() && fremd.is_none();
            if zeilen.is_empty() && !(konten && (fremd.is_some() || leer)) {
                continue;
            }
            if !erste {
                y += 12.0;
            }
            erste = false;
            el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: gpu.text_height(TOPBAR_SUB_SCALE) }, action: None, label: titel.into(), kind: AccKind::Header });
            y += gpu.text_height(TOPBAR_SUB_SCALE) + 8.0;
            // Angemeldet, aber (noch) nicht in der Liste: als eigene Zeile ohne Aktion.
            if let Some(c) = fremd.filter(|_| konten) {
                el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: 48.0 }, action: None, label: c.username.clone(), kind: AccKind::Current });
                y += 52.0;
            }
            for i in zeilen {
                let a = &self.accounts.list[i];
                let kind = if self.shown_as_active(a) { AccKind::Current } else { AccKind::Other };
                el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: 48.0 }, action: Some(ButtonAction::UseAccount(i)), label: a.name.clone(), kind });
                y += 52.0;
            }
            if konten && leer {
                el.push(AccElem { rect: Rect { x: pad, y, w: innen, h: 40.0 }, action: None, label: "Noch kein Konto. Mit Microsoft anmelden.".into(), kind: AccKind::Hint });
                y += 44.0;
            }
        }
        y += 16.0;
        // Anmelden: Microsoft ueber die ganze Breite, darunter die beiden Token-Wege nebeneinander.
        let halb = (innen - 12.0) / 2.0;
        let knopf = |x: f32, y: f32, w: f32, action, label: &str, kind| AccElem { rect: Rect { x, y, w, h: 40.0 }, action: Some(action), label: label.into(), kind };
        el.push(knopf(pad, y, innen, ButtonAction::MicrosoftLogin, "Mit Microsoft anmelden", AccKind::Primary));
        y += 48.0;
        el.push(knopf(pad, y, halb, ButtonAction::OpenInputField(InputKind::RefreshToken), "Refresh-Token", AccKind::Button));
        el.push(knopf(pad + halb + 12.0, y, halb, ButtonAction::OpenInputField(InputKind::Token), "Token eingeben", AccKind::Button));
        y += 48.0;
        // Offenes Eingabefeld (Token oder Refresh-Token) mit OK.
        if matches!(self.input_field, Some((InputKind::Token | InputKind::RefreshToken, _))) {
            y += gpu.text_height(TOPBAR_SUB_SCALE) + 8.0;
            el.push(AccElem { rect: Rect { x: pad, y, w: innen - 100.0, h: 40.0 }, action: None, label: String::new(), kind: AccKind::Field });
            el.push(AccElem { rect: Rect { x: pad + innen - 88.0, y, w: 88.0, h: 40.0 }, action: Some(ButtonAction::SubmitInputField), label: "OK".into(), kind: AccKind::Primary });
            y += 48.0;
        }
        y += 8.0;
        el.push(AccElem { rect: Rect { x: pad + innen - 140.0, y, w: 140.0, h: 40.0 }, action: None, label: "Schließen".into(), kind: AccKind::Close });
        let h = y + 40.0 + pad;
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round().max(TOPBAR_H + 8.0), w, h };
        for e in &mut el {
            e.rect.x += p.x;
            e.rect.y += p.y;
        }
        (p, el)
    }

    pub(crate) fn build_account_manager(&self, overlay: &mut UiOverlay) {
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.account_manager_open && self.mode == AppMode::Menu) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, el) = self.account_manager_layout(gpu);
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let mut sprites = Vec::new();
        let mut texts = vec![UiText { text: "Acc-Manager".into(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL }];
        let (status, farbe) = self.account_status_line();
        let status_y = p.y + 20.0 + gpu.text_height(UI_SCALE_NORMAL) + 12.0;
        texts.push(UiText { text: status, x: p.x + 24.0, y: status_y, color: farbe, scale: UI_SCALE_SMALL });
        let mitte_y = |r: Rect, scale: f32| (r.y + (r.h - gpu.text_height(scale)) / 2.0).round();
        for e in &el {
            let hover = e.rect.contains(cx, cy);
            match e.kind {
                AccKind::Current | AccKind::Other => {
                    let an = e.kind == AccKind::Current;
                    let konto = match e.action {
                        Some(ButtonAction::UseAccount(i)) => self.accounts.list.get(i),
                        _ => None,
                    };
                    let ungueltig = konto.is_some_and(|a| matches!(self.account_check(a), accounts::Check::Invalid(_)));
                    let fill = if an { g(Slot::AccentSurface) } else if hover || self.account_context.is_some_and(|(i, _)| konto.is_some_and(|a| self.accounts.list.get(i) == Some(a))) { g(Slot::Surface2) } else { g(Slot::Surface1) };
                    rects.push(UiRect::framed(e.rect, fill, 6.0, 1.0, if an { g(Slot::Accent) } else { g(Slot::Border1) }));
                    let bild = Rect { x: e.rect.x + 8.0, y: e.rect.y + 8.0, w: 32.0, h: 32.0 };
                    let uuid = konto.map(|a| a.uuid.as_str()).or(self.credentials.as_ref().map(|c| c.profile_id.as_str())).and_then(uuid_bytes_from_hex);
                    self.push_account_head(konto, uuid, bild, &mut rects, &mut sprites);
                    let farbe = if ungueltig {
                        theme::text_3()
                    } else if an {
                        g(Slot::AccentText)
                    } else {
                        theme::text()
                    };
                    texts.push(UiText { text: e.label.clone(), x: bild.x + bild.w + 12.0, y: mitte_y(e.rect, UI_SCALE_SMALL), color: farbe, scale: UI_SCALE_SMALL });
                    if let Some(a) = konto {
                        self.push_account_info(gpu, a, e.rect, &mut texts);
                    }
                }
                AccKind::Hint => texts.push(UiText { text: e.label.clone(), x: e.rect.x, y: mitte_y(e.rect, UI_SCALE_SMALL), color: theme::text_3(), scale: UI_SCALE_SMALL }),
                AccKind::Header => texts.push(UiText { text: e.label.clone(), x: e.rect.x, y: e.rect.y, color: theme::text_3(), scale: TOPBAR_SUB_SCALE }),
                AccKind::Field => {
                    let Some((kind, wert)) = &self.input_field else { continue };
                    let label = match kind {
                        InputKind::Token => "MINECRAFT-TOKEN (STRG+V)",
                        _ => "MICROSOFT-REFRESH-TOKEN (STRG+V)",
                    };
                    texts.push(UiText { text: label.into(), x: e.rect.x, y: e.rect.y - gpu.text_height(TOPBAR_SUB_SCALE) - 8.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
                    rects.push(UiRect::framed(e.rect, g(Slot::SurfaceSunken), 6.0, 1.0, g(Slot::Accent)));
                    let gezeigt = token_display(gpu, wert, e.rect.w - 24.0);
                    let ty = mitte_y(e.rect, UI_SCALE_SMALL);
                    let x = e.rect.x + 12.0 + gpu.measure_raw(&gezeigt, UI_SCALE_SMALL);
                    self.push_caret(&mut rects, gpu.caret_rect(x, ty, UI_SCALE_SMALL));
                    push_typed_marker(&mut texts);
                    texts.push(UiText { text: gezeigt, x: e.rect.x + 12.0, y: ty, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                AccKind::Button | AccKind::Primary | AccKind::Close => {
                    let farbe = match e.kind {
                        AccKind::Primary => g(Slot::Accent),
                        _ => g(Slot::Surface2),
                    };
                    let b = Button { slider: None, action: ButtonAction::Back, rect: e.rect, label: String::new(), enabled: true, color: farbe, scale: UI_SCALE_SMALL };
                    let (flaeche, textfarbe) = styled_button(&b, self.hover_level(b.rect, hover));
                    rects.push(flaeche);
                    texts.push(gpu.centered_text(e.rect, e.label.clone(), textfarbe, UI_SCALE_SMALL));
                }
            }
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites, pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Rechts in einer Kontozeile: bei Tokens, die sich nicht selbst erneuern, wie lange sie
    // noch gelten; ein ungueltiges Konto sagt das statt dessen. Woher ein Konto stammt, steht
    // im Acc-Manager schon in der Ueberschrift seiner Gruppe.
    pub(crate) fn push_account_info(&self, gpu: &GpuState, a: &accounts::Account, zeile: Rect, texts: &mut Vec<UiText>) {
        let (text, farbe) = match self.account_check(a) {
            accounts::Check::Invalid(_) => ("ungültig".to_string(), theme::get(theme::Slot::Danger)),
            _ if a.source != accounts::Source::Microsoft => match accounts::expiry(&a.access_token) {
                Some(exp) => (accounts::remaining_text(exp, accounts::now_unix()), theme::text_2()),
                None => return,
            },
            _ => return,
        };
        let ty = (zeile.y + (zeile.h - gpu.text_height(TOPBAR_SUB_SCALE)) / 2.0).round();
        let x = zeile.x + zeile.w - 12.0 - gpu.measure_text(&text, TOPBAR_SUB_SCALE);
        texts.push(UiText { text, x: x.round(), y: ty, color: farbe, scale: TOPBAR_SUB_SCALE });
    }

    // Rechtsklick-Menue eines Kontos im Acc-Manager: am Zeiger, in den Bildschirm geschoben.
    // "Ausloggen" nur beim angemeldeten Konto.
    pub(crate) fn account_context_layout(&self) -> Option<(Rect, Vec<(Rect, AccountContextItem)>)> {
        let (i, at) = self.account_context?;
        let a = self.accounts.list.get(i)?;
        let mut items = Vec::new();
        if self.is_active_account(a) {
            items.push(AccountContextItem::Logout);
        }
        items.push(AccountContextItem::Forget);
        let (sw, sh) = ui_screen();
        let h = items.len() as f32 * SERVER_CONTEXT_ROW_H + 12.0;
        let x = at.0.min(sw - SERVER_CONTEXT_W - 8.0);
        let y = at.1.min(sh - h - 8.0);
        let p = Rect { x, y, w: SERVER_CONTEXT_W, h };
        let zeilen = items.into_iter().enumerate().map(|(n, item)| (Rect { x: p.x + 6.0, y: p.y + 6.0 + n as f32 * SERVER_CONTEXT_ROW_H, w: p.w - 12.0, h: SERVER_CONTEXT_ROW_H }, item)).collect();
        Some((p, zeilen))
    }

    pub(crate) fn account_context_open(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (cx, cy) = self.cursor_pos;
        let (_, el) = self.account_manager_layout(gpu);
        self.account_context = el.iter().find_map(|e| match e.action {
            Some(ButtonAction::UseAccount(i)) if e.rect.contains(cx, cy) => Some((i, (cx, cy))),
            _ => None,
        });
        if self.account_context.is_some() {
            self.play_ui_sound(ui_sound::UiSound::Open);
        }
    }

    // Linke Taste bei offenem Konto-Menue: Zeile ausfuehren, daneben schliesst es nur.
    pub(crate) fn account_context_press(&mut self) {
        let Some((_, zeilen)) = self.account_context_layout() else { return };
        let Some((i, _)) = self.account_context.take() else { return };
        let (cx, cy) = self.cursor_pos;
        match zeilen.into_iter().find(|(r, _)| r.contains(cx, cy)).map(|(_, item)| item) {
            Some(AccountContextItem::Logout) => {
                self.play_ui_sound(ui_sound::UiSound::Click);
                self.logout();
            }
            Some(AccountContextItem::Forget) => {
                self.play_ui_sound(ui_sound::UiSound::Close);
                self.forget_account(i);
            }
            None => {}
        }
    }

    pub(crate) fn build_account_context(&self, overlay: &mut UiOverlay) {
        let Some((gpu, (p, zeilen))) = self.gpu.as_ref().zip(self.account_context_layout()).filter(|_| self.account_manager_open && self.mode == AppMode::Menu) else { return };
        use theme::Slot;
        let g = theme::get;
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![UiRect::shadow(p, 8.0, 8.0, 24.0, 0.5), UiRect::framed(p, g(Slot::Surface1), 8.0, 1.0, g(Slot::Border1))];
        let mut texts = Vec::new();
        for (r, item) in zeilen {
            let (label, farbe) = match item {
                AccountContextItem::Logout => ("Ausloggen", theme::text()),
                AccountContextItem::Forget => ("Account vergessen", g(Slot::Danger)),
            };
            if r.contains(cx, cy) {
                rects.push(UiRect::rounded(r, g(Slot::Surface2), 6.0));
            }
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: r.x + 12.0, y: ty, color: farbe, scale: UI_SCALE_SMALL });
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Linke Taste bei offenem Acc-Manager. Immer verbraucht; daneben schliesst er.
    pub(crate) fn account_manager_press(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (p, el) = self.account_manager_layout(gpu);
        let (cx, cy) = self.cursor_pos;
        if !p.contains(cx, cy) {
            self.account_manager_open = false;
            return;
        }
        // Spaetere Elemente liegen oben.
        let Some(e) = el.into_iter().rev().find(|e| e.rect.contains(cx, cy)) else { return };
        self.play_ui_sound(ui_sound::UiSound::Click);
        match (e.kind, e.action) {
            (AccKind::Close, _) => self.account_manager_open = false,
            (_, Some(action)) => self.run_menu_action(action),
            _ => {}
        }
    }

    // Quick Connect: Fenster mitten auf dem abgedunkelten Bildschirm.
    // (Fenster, Adressfeld, Los, Abbrechen)
    // Fenster "Neuer Eintrag"/"Eintrag bearbeiten": Adresse, darunter der freiwillige Name.
    pub(crate) fn build_server_dialog(&self, overlay: &mut UiOverlay) {
        let Some((gpu, edit)) = self.gpu.as_ref().zip(self.server_edit.as_ref()).filter(|_| self.mode == AppMode::Menu) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, adresse, name, speichern, abbrechen) = server_dialog_layout();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let titel = if edit.index.is_some() { "Eintrag bearbeiten" } else { "Neuer Eintrag" };
        let mut texts = vec![UiText { text: titel.to_string(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL }];
        for (feld, field, label) in [(adresse, ServerField::Address, "ADRESSE"), (name, ServerField::Name, "NAME (OPTIONAL)")] {
            let fokus = edit.focus == field;
            rects.push(UiRect::framed(feld, g(Slot::SurfaceSunken), 6.0, 1.0, if fokus { g(Slot::Accent) } else { g(Slot::Border1) }));
            texts.push(UiText { text: label.to_string(), x: feld.x, y: feld.y - gpu.text_height(TOPBAR_SUB_SCALE) - 8.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
            self.push_text_field(gpu, edit.field(field), feld, fokus, &mut rects, &mut texts);
        }
        // Verbindungsbild: zwei Werte, zwei Knoepfe nebeneinander; der gewaehlte in Akzent.
        let knoepfe = server_dialog_view_buttons();
        texts.push(UiText {
            text: "VERBINDUNGSBILD".to_string(),
            x: knoepfe[0].x,
            y: knoepfe[0].y - gpu.text_height(TOPBAR_SUB_SCALE) - 8.0,
            color: theme::text_3(),
            scale: TOPBAR_SUB_SCALE,
        });
        for (i, (r, label)) in knoepfe.into_iter().zip(["Erster Moment", "Letzter Moment"]).enumerate() {
            let an = edit.view_last() == (i == 1);
            let b = Button {
                slider: None,
                action: ButtonAction::NewServer,
                rect: r,
                label: String::new(),
                enabled: true,
                color: if an { g(Slot::Accent) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        // Namen anzeigen: ein Schalter wie in den Einstellungen, die Beschriftung links daneben.
        let schalter = Self::toggle(ButtonAction::NewServer, 0, edit.show_name);
        let schalter = Button { rect: server_dialog_name_toggle(), ..schalter };
        let (flaeche, farbe) = styled_button(&schalter, self.hover_level(schalter.rect, schalter.rect.contains(cx, cy)));
        rects.push(flaeche);
        texts.push(gpu.centered_text(schalter.rect, schalter.label.clone(), farbe, UI_SCALE_SMALL));
        let label_y = (schalter.rect.y + (schalter.rect.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Namen anzeigen".to_string(), x: knoepfe[0].x, y: label_y, color: theme::text(), scale: UI_SCALE_SMALL });
        for (r, label, primaer) in [(abbrechen, "Abbrechen", false), (speichern, "Speichern", true)] {
            let b = Button {
                slider: None,
                action: ButtonAction::NewServer,
                rect: r,
                label: String::new(),
                enabled: !primaer || !edit.address.text().trim().is_empty(),
                color: if primaer { g(Slot::Accent) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Rechtsklick-Menue einer Server-Karte (s. server_context_layout).
    pub(crate) fn build_server_context(&self, overlay: &mut UiOverlay) {
        let Some((gpu, (p, zeilen))) = self.gpu.as_ref().zip(self.server_context_layout()).filter(|_| self.mode == AppMode::Menu) else { return };
        use theme::Slot;
        let g = theme::get;
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![UiRect::shadow(p, 8.0, 8.0, 24.0, 0.5), UiRect::framed(p, g(Slot::Surface1), 8.0, 1.0, g(Slot::Border1))];
        let mut texts = Vec::new();
        for (r, label, farbe) in [(zeilen[0], "Bearbeiten", theme::text()), (zeilen[1], "Löschen", g(Slot::Danger))] {
            if r.contains(cx, cy) {
                rects.push(UiRect::rounded(r, g(Slot::Surface2), 6.0));
            }
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: r.x + 12.0, y: ty, color: farbe, scale: UI_SCALE_SMALL });
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Gezogene Server-Karte: nur ihr Icon, mittig am Zeiger.
    pub(crate) fn build_server_drag(&self, overlay: &mut UiOverlay) {
        let Some(i) = self.server_drag_active().filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::Connect) else { return };
        let Some(entry) = self.servers.get(i) else { return };
        let icon = server_icon_px();
        let (cx, cy) = self.cursor_pos;
        let feld = Rect { x: (cx - icon / 2.0).round(), y: (cy - icon / 2.0).round(), w: icon, h: icon };
        let mut rects = vec![UiRect::shadow(feld, 6.0, 4.0, 16.0, 0.5), UiRect::rounded(feld, theme::get(theme::Slot::SurfaceSunken), 6.0)];
        let mut sprites = Vec::new();
        match self.server_list_icon(&entry.address) {
            Some(slot) => sprites.push(UiSprite::quad(feld.x, feld.y, feld.w, feld.h, server_icon_uv(slot), [1.0; 4], SpriteAtlas::ServerIcons)),
            None => rects.push(UiRect::framed(feld, theme::get(theme::Slot::SurfaceSunken), 6.0, 1.0, theme::get(theme::Slot::Border2))),
        }
        rects.push(drag_icon_rim(feld));
        overlay.groups.push(UiOverlayGroup { rects, texts: Vec::new(), sprites, pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Zahnraeder auf den Pack-Bildern des Skin-Tabs: gross mit Abdunklung unter dem Zeiger,
    // klein oben rechts, wenn im Pack etwas ausgeschaltet ist. Eigene Overlay-Gruppe, weil
    // das Bild ein Sprite ist und Sprites nach den Flaechen gezeichnet werden -- im Tab selbst
    // laege das Bild ueber dem Zahnrad. Bildschirmkoordinaten.
    pub(crate) fn build_skin_gears(&self) -> Option<UiOverlayGroup> {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsSkin {
            return None;
        }
        let (dx, dy) = self.content_offset();
        let (px, py) = self.pointer();
        let frei = self.skin_drag_active().is_none() && self.skin_files.is_none();
        let (oben, unten) = (skin_tiles_top(), skin_tiles_bottom());
        let mut rects = Vec::new();
        for (_, _, name, karte) in self.skin_tiles() {
            let bild = skin_icon_rect(karte);
            if !self.skin_has_settings(&name) || bild.y < oben || bild.y + bild.h > unten || skin_clip(karte).is_none() {
                continue;
            }
            let b = Rect { x: bild.x + dx, y: bild.y + dy, ..bild };
            if frei && bild.contains(px, py) {
                rects.push(UiRect::rounded(b, dialog_dim(), 6.0));
                push_gear(&mut rects, (b.x + b.w / 2.0, b.y + b.h / 2.0), (b.w * 0.2).round(), theme::text());
            } else if self.skin_has_exclusions(&name) {
                let mitte = (b.x + b.w - 2.0, b.y + 2.0);
                rects.push(UiRect::rounded(Rect { x: mitte.0 - 12.0, y: mitte.1 - 12.0, w: 24.0, h: 24.0 }, theme::get(theme::Slot::BgPanel), 12.0));
                push_gear(&mut rects, mitte, 6.0, theme::text());
            }
        }
        (!rects.is_empty()).then_some(UiOverlayGroup { rects, texts: Vec::new(), sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 })
    }

    // Gezogene Pack-Kachel: nur ihr Bild, mittig am Zeiger (der Zeiger selbst ist dann weg).
    pub(crate) fn build_skin_drag(&self, overlay: &mut UiOverlay) {
        let Some(d) = self.skin_drag_active().filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsSkin) else { return };
        let (links, rechts) = self.skin_columns();
        let liste = if d.column == skin_tab::Column::Available { links } else { rechts };
        let Some(name) = liste.get(d.index) else { return };
        let s = skin_icon_rect(skin_tile_rect(d.column, 0.0)).w;
        let (cx, cy) = self.cursor_pos;
        let feld = Rect { x: (cx - s / 2.0).round(), y: (cy - s / 2.0).round(), w: s, h: s };
        let mut rects = vec![UiRect::shadow(feld, 6.0, 4.0, 16.0, 0.5), UiRect::rounded(feld, theme::get(theme::Slot::SurfaceSunken), 6.0)];
        let mut sprites = Vec::new();
        match self.skin_icon_slot(name) {
            Some(platz) => sprites.push(UiSprite::quad(feld.x, feld.y, feld.w, feld.h, pack_icon_uv(platz), [1.0; 4], SpriteAtlas::PackIcons)),
            None => rects.push(UiRect::framed(feld, theme::get(theme::Slot::SurfaceSunken), 6.0, 1.0, theme::get(theme::Slot::Border2))),
        }
        rects.push(drag_icon_rim(feld));
        overlay.groups.push(UiOverlayGroup { rects, texts: Vec::new(), sprites, pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Fenster mit dem Dateibaum eines Packs: je Zeile Pfeil (Ordner), Kaestchen, Name.
    pub(crate) fn build_skin_files(&self, overlay: &mut UiOverlay) {
        use theme::Slot;
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsSkin) else { return };
        let Some(w) = &self.skin_files else { return };
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, liste, alles, fertig) = skin_files_layout();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
            UiRect::framed(liste, g(Slot::SurfaceSunken), 6.0, 1.0, g(Slot::Border1)),
        ];
        let mut texts = vec![
            UiText { text: ellipsize(gpu, &w.title, UI_SCALE_NORMAL, p.w - 48.0), x: p.x + 24.0, y: p.y + 18.0, color: theme::text(), scale: UI_SCALE_NORMAL },
            UiText {
                text: "assets/minecraft -- Ausgeschaltetes kommt aus den Packs darunter bzw. von Vanilla".to_string(),
                x: p.x + 24.0,
                y: p.y + 18.0 + gpu.text_height(UI_SCALE_NORMAL) + 8.0,
                color: theme::text_2(),
                scale: UI_SCALE_SMALL,
            },
        ];
        let aus = self.settings.skin_exclusions.get(&w.skin).map(Vec::as_slice).unwrap_or(&[]);
        if w.entries.is_empty() {
            texts.push(UiText { text: "Das Pack hat nichts unter assets/minecraft.".to_string(), x: liste.x + 12.0, y: liste.y + 10.0, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        for (i, zeile) in self.skin_files_rows() {
            let e = &w.entries[i];
            if zeile.contains(cx, cy) {
                rects.push(UiRect::rounded(Rect { x: zeile.x + 2.0, w: zeile.w - 4.0, ..zeile }, g(Slot::Surface1), 4.0));
            }
            let kasten = Self::skin_files_check_rect(zeile, e.depth);
            if e.is_dir {
                push_chevron(&mut rects, (kasten.x - 13.0, kasten.y + kasten.h / 2.0), w.expanded.contains(&e.path), theme::text());
            }
            push_check_box(&mut rects, kasten, !skin_tab::is_off(aus, &e.path));
            let tx = kasten.x + kasten.w + 10.0;
            let name = if e.is_dir { format!("{}/", e.name) } else { e.name.clone() };
            texts.push(UiText {
                text: ellipsize(gpu, &name, UI_SCALE_SMALL, liste.x + liste.w - 12.0 - tx),
                x: tx,
                y: (zeile.y + (zeile.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round(),
                color: theme::text(),
                scale: UI_SCALE_SMALL,
            });
        }
        // Bildlaufleiste, sobald nicht alle Zeilen passen.
        let n = skin_tab::visible_rows(&w.entries, &w.expanded).len();
        let sichtbar = skin_files_rows_visible();
        if n > sichtbar {
            let spur = Rect { x: liste.x + liste.w - 8.0, y: liste.y + 4.0, w: 4.0, h: liste.h - 8.0 };
            rects.push(UiRect::rounded(spur, g(Slot::Surface1), 2.0));
            let griff_h = (spur.h * sichtbar as f32 / n as f32).max(24.0);
            let max = n - sichtbar;
            let griff = Rect { y: spur.y + (spur.h - griff_h) * (w.scroll.stand / max as f32).clamp(0.0, 1.0), h: griff_h, ..spur };
            rects.push(UiRect::rounded(griff, g(Slot::Border2), 2.0));
        }
        for (r, label, primaer) in [(alles, "Alles an", false), (fertig, "Fertig", true)] {
            let b = Button {
                slider: None,
                action: ButtonAction::Nothing,
                rect: r,
                label: String::new(),
                enabled: primaer || !aus.is_empty(),
                color: if primaer { g(Slot::Accent) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    pub(crate) fn quick_connect_layout() -> (Rect, Rect, Rect, Rect) {
        let (sw, sh) = ui_screen();
        let (w, h) = (560.0, 220.0);
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let feld = Rect { x: p.x + 24.0, y: p.y + 92.0, w: p.w - 48.0, h: 44.0 };
        let los = Rect { x: p.x + p.w - 24.0 - 140.0, y: p.y + p.h - 24.0 - 40.0, w: 140.0, h: 40.0 };
        let abbrechen = Rect { x: los.x - 12.0 - 140.0, ..los };
        (p, feld, los, abbrechen)
    }

    pub(crate) fn build_quick_connect(&self, overlay: &mut UiOverlay) {
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.quick_connect_open && self.mode == AppMode::Menu) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, feld, los, abbrechen) = Self::quick_connect_layout();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
            UiRect::framed(feld, g(Slot::SurfaceSunken), 6.0, 1.0, g(Slot::Accent)),
        ];
        let mut texts = vec![
            UiText { text: "Quick Connect".to_string(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL },
            UiText { text: "SERVER-ADRESSE".to_string(), x: feld.x, y: feld.y - gpu.text_height(TOPBAR_SUB_SCALE) - 8.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE },
        ];
        self.push_text_field(gpu, &self.quick_field, feld, true, &mut rects, &mut texts);
        let leer = self.quick_field.text().trim().is_empty();
        for (r, label, primaer) in [(abbrechen, "Abbrechen", false), (los, "Los", true)] {
            let hover = r.contains(cx, cy);
            let b = Button {
                slider: None,
                action: ButtonAction::Connect,
                rect: r,
                label: String::new(),
                enabled: !primaer || !leer,
                color: if primaer { g(Slot::Accent) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, hover));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Zeichengrenze im Quick-Connect-Feld unter dem Zeiger.
    pub(crate) fn quick_field_index(&self, x: f32) -> usize {
        let Some(gpu) = self.gpu.as_ref() else { return 0 };
        let (_, feld, _, _) = Self::quick_connect_layout();
        text_field::index_at(self.quick_field.text(), x - feld.x - 12.0, |t| gpu.measure_raw(t, UI_SCALE_NORMAL))
    }

    // Linke Maustaste, waehrend Quick Connect offen ist. Immer verbraucht.
    pub(crate) fn quick_connect_press(&mut self) {
        let (p, feld, los, abbrechen) = Self::quick_connect_layout();
        let (cx, cy) = self.cursor_pos;
        if feld.contains(cx, cy) {
            let i = self.quick_field_index(cx);
            let shift = self.modifiers.shift_key();
            if field_press(&mut self.quick_field, &mut self.quick_last_press, &mut self.quick_dragging, i, shift, (cx, cy)) {
                self.selection_sound_hint = Some(ui_sound::UiSound::Step);
            }
            return;
        }
        if los.contains(cx, cy) {
            self.quick_connect_submit();
        } else if abbrechen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.quick_connect_open = false;
        }
    }

    pub(crate) fn quick_connect_submit(&mut self) {
        let adresse = self.quick_field.text().trim().to_string();
        if adresse.is_empty() {
            self.play_ui_sound(ui_sound::UiSound::Denied);
            return;
        }
        self.play_ui_sound(ui_sound::UiSound::Click);
        self.quick_connect_open = false;
        self.server_address = adresse;
        self.go_connect();
    }

    // Wird gerade ins Quick-Connect-Feld getippt?
    pub(crate) fn quick_typing(&self) -> bool {
        self.quick_connect_open && self.mode == AppMode::Menu && self.screen == MenuScreen::Connect
    }

    // Das einzeilige Feld, in das gerade getippt wird: das fokussierte des Server-Fensters (hat
    // Vorrang), das Namensfeld eines neuen Themes, sonst das von Quick Connect.
    pub(crate) fn active_text_field(&mut self) -> Option<&mut text_field::TextField> {
        if self.dialog.is_some() {
            return self.dialog_text_field();
        }
        if self.server_edit.is_some() {
            return self.server_edit.as_mut().map(ServerEdit::focused);
        }
        if self.theme_name_dialog.is_some() {
            return self.theme_name_dialog.as_mut().map(|d| &mut d.field);
        }
        if self.quick_typing() {
            return Some(&mut self.quick_field);
        }
        if self.server_search_typing() {
            return self.server_search.as_mut();
        }
        None
    }

    // Leiste oben und (in den Einstellungen) die Kategorienleiste links.
    pub(crate) fn build_chrome(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        let l = self.topbar_layout(gpu);
        let einstellungen = self.screen.settings_category().is_some();

        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: TOPBAR_H }, color: g(Slot::BgPanel), ..UiRect::FLAT });
        rects.push(UiRect { rect: Rect { x: 0.0, y: TOPBAR_H - 1.0, w: sw, h: 1.0 }, color: g(Slot::Border1), ..UiRect::FLAT });

        // `r` ist die Trefferflaeche (bei Zahnrad und Glocke groesser als das Symbol).
        let icon_farbe = |r: Rect, aktiv: bool| {
            if aktiv {
                g(Slot::Accent)
            } else if r.contains(cx, cy) {
                theme::text()
            } else {
                theme::text_2()
            }
        };
        // Gezeichnet wird die Flaeche um das Symbol, getroffen wird `hit`.
        let hover_flaeche = |rects: &mut Vec<UiRect>, r: Rect, hit: Rect, aktiv: bool| {
            if aktiv {
                rects.push(UiRect::rounded(r, g(Slot::AccentSurface), 6.0));
            } else if hit.contains(cx, cy) {
                rects.push(UiRect::rounded(r, g(Slot::Surface2), 6.0));
            }
        };
        hover_flaeche(rects, l.gear, l.gear_hit, false);
        // In den Einstellungen fuehrt derselbe Knopf zurueck: Pfeil nach links statt Zahnrad.
        if einstellungen {
            push_back_arrow_icon(rects, l.gear, icon_farbe(l.gear_hit, false));
        } else {
            push_gear_icon(rects, l.gear, icon_farbe(l.gear_hit, false));
        }
        self.build_tab_bar(gpu, &l, rects, texts);
        hover_flaeche(rects, l.bell, l.bell_hit, self.notification_panel_open);
        push_bell_icon(rects, l.bell, icon_farbe(l.bell_hit, self.notification_panel_open));
        if self.notification_history.len() > self.notifications_seen {
            rects.push(UiRect::rounded(Rect { x: l.bell.x + l.bell.w - 13.0, y: l.bell.y + 7.0, w: 8.0, h: 8.0 }, g(Slot::Accent), 4.0));
        }

        // Konto: Name, daneben der Kopf aus dem Skin (Gesicht + Hutlage). Bei offener
        // Kontenliste wird der Knopf zum Acc-Manager.
        hover_flaeche(rects, l.konto, l.konto, self.screen == MenuScreen::Account || self.account_menu_open);
        if self.account_menu_open {
            texts.push(gpu.centered_text(l.konto, "Acc-Manager".to_string(), g(Slot::AccentText), UI_SCALE_SMALL));
        } else {
            let text_y = ((TOPBAR_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: l.name.clone(), x: l.konto.x + 10.0, y: text_y, color: theme::text(), scale: UI_SCALE_SMALL });
            let uuid = self.credentials.as_ref().and_then(|c| uuid_bytes_from_hex(&c.profile_id)).unwrap_or([0; 16]);
            let k = l.kopf;
            rects.push(UiRect::rounded(Rect { x: k.x - 2.0, y: k.y - 2.0, w: k.w + 4.0, h: k.h + 4.0 }, g(Slot::Border2), 4.0));
            // Waehrend des Wechsels Ladepunkte statt des Kopfs: die Anmeldung laeuft noch.
            if let Some((_, seit)) = &self.account_switching {
                rects.push(UiRect::rounded(k, g(Slot::SurfaceSunken), 3.0));
                rects.extend(loading_dots(k, seit.elapsed().as_secs_f32(), g(Slot::Accent)));
            } else {
                sprites.push(UiSprite::quad(k.x, k.y, k.w, k.h, self.skin_face_uv(&uuid, 8.0, 8.0), [1.0; 4], SpriteAtlas::Entity));
                sprites.push(UiSprite::quad(k.x, k.y, k.w, k.h, self.skin_face_uv(&uuid, 40.0, 8.0), [1.0; 4], SpriteAtlas::Entity));
            }
        }

        // Uhr und darunter, wie lange der Client schon laeuft -- per Klick auf die Uhr nur die
        // Uhrzeit (wie in osu!).
        let jetzt = chrono::Local::now().format("%H:%M:%S").to_string();
        if self.settings.topbar_uptime {
            let lauf = self.app_started.elapsed().as_secs();
            // Nur Stunden und Minuten -- Sekunden sind hier Unruhe ohne Nutzen.
            let lauf = format!("{:02}:{:02}", lauf / 3600, lauf / 60 % 60);
            let oben = ((TOPBAR_H - gpu.text_height(UI_SCALE_SMALL) - gpu.text_height(TOPBAR_SUB_SCALE) - 2.0) / 2.0).round();
            texts.push(UiText { text: jetzt, x: l.uhr.x, y: oben, color: theme::text(), scale: UI_SCALE_SMALL });
            texts.push(UiText {
                text: format!("läuft seit {lauf}"),
                x: l.uhr.x,
                y: oben + gpu.text_height(UI_SCALE_SMALL) + 2.0,
                color: g(Slot::AccentText),
                scale: TOPBAR_SUB_SCALE,
            });
        } else {
            let oben = ((TOPBAR_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: jetzt, x: l.uhr.x, y: oben, color: theme::text(), scale: UI_SCALE_SMALL });
        }

        if !einstellungen {
            return;
        }
        // Kategorienleiste.
        rects.push(UiRect { rect: Rect { x: 0.0, y: TOPBAR_H, w: SIDEBAR_W, h: sh - TOPBAR_H }, color: g(Slot::BgPanel), ..UiRect::FLAT });
        rects.push(UiRect { rect: Rect { x: SIDEBAR_W - 1.0, y: TOPBAR_H, w: 1.0, h: sh - TOPBAR_H }, color: g(Slot::Border1), ..UiRect::FLAT });
        // Rand rechts fuer die Erklaerungen (s. tooltip_strip_w), wie die Leiste links.
        let rand = self.side_strip_w(sw);
        rects.push(UiRect { rect: Rect { x: sw - rand, y: TOPBAR_H, w: rand, h: sh - TOPBAR_H }, color: g(Slot::BgPanel), ..UiRect::FLAT });
        rects.push(UiRect { rect: Rect { x: sw - rand, y: TOPBAR_H, w: 1.0, h: sh - TOPBAR_H }, color: g(Slot::Border1), ..UiRect::FLAT });
        let aktiv = self.screen.settings_category();
        let eintrag = |rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, r: Rect, kategorie: MenuScreen, label: &str| {
            let an = aktiv == Some(kategorie);
            let k = self.hover_level(r, !an && Self::sidebar_hit_rect(r).contains(cx, cy));
            if an {
                rects.push(UiRect::rounded(r, g(Slot::AccentSurface), 6.0));
                rects.push(UiRect::rounded(Rect { x: r.x, y: r.y + 8.0, w: 3.0, h: r.h - 16.0 }, g(Slot::Accent), 1.5));
            } else if k > 0.0 {
                rects.push(UiRect::rounded(r, sidebar_hover_fill(k), 6.0));
            }
            let farbe = if an { g(Slot::AccentText) } else { mix_color(theme::text_2(), theme::text(), k.min(1.0)) };
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: r.x + 14.0, y: ty, color: farbe, scale: UI_SCALE_SMALL });
        };
        for (i, (kategorie, label)) in Self::sidebar_categories().into_iter().enumerate() {
            eintrag(rects, texts, Self::sidebar_item_rect(i, sh), kategorie, label);
        }
        let setup = Self::sidebar_setup_rect(sh);
        rects.push(UiRect { rect: Rect { x: SIDEBAR_PAD, y: setup.y - 9.0, w: SIDEBAR_W - 2.0 * SIDEBAR_PAD, h: 1.0 }, color: g(Slot::Border1), ..UiRect::FLAT });
        eintrag(rects, texts, setup, MenuScreen::SettingsSetup, "Setup");
        for i in 0..SIDEBAR_LINKS.len() {
            let r = Self::sidebar_link_rect(i, sh);
            let k = self.hover_level(r, r.contains(cx, cy));
            if k > 0.0 {
                rects.push(UiRect::rounded(r, sidebar_hover_fill(k), 6.0));
            }
            let farbe = mix_color(theme::text_2(), theme::text(), k.min(1.0));
            let (x, y) = ((r.x + (r.w - LOGO_DRAW) / 2.0).round(), (r.y + (r.h - LOGO_DRAW) / 2.0).round());
            sprites.push(UiSprite::quad(x, y, LOGO_DRAW, LOGO_DRAW, logo_uv(i), farbe, SpriteAtlas::Logos));
        }
        // Version rechts neben den Logos, mittig zu ihnen.
        let letztes = Self::sidebar_link_rect(SIDEBAR_LINKS.len() - 1, sh);
        let vy = (letztes.y + (letztes.h - gpu.text_height(TOPBAR_SUB_SCALE)) / 2.0).round();
        push_typed_marker(texts);
        texts.push(UiText { text: concat!("v", env!("CARGO_PKG_VERSION")).to_string(), x: letztes.x + letztes.w + 8.0, y: vy, color: theme::text_2(), scale: TOPBAR_SUB_SCALE });
        // Skin-Tab: Knoepfe im Rand rechts (s. skin_strip).
        if self.screen == MenuScreen::SettingsSkin {
            let (knoepfe, (lx, ly), _) = self.skin_strip(gpu);
            texts.push(UiText { text: "Zips automatisch entpacken".to_string(), x: lx, y: ly, color: theme::text(), scale: UI_SCALE_SMALL });
            for b in knoepfe {
                let hover = self.skin_files.is_none() && b.hits(cx, cy);
                let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, hover));
                rects.push(flaeche);
                texts.push(gpu.centered_text(b.rect, b.label, farbe, b.scale));
            }
        }

    }

    // Geloeschte Zeichen herausfallen lassen (s. ERASE_FALL): die Eingabe-Texte dieses Frames
    // mit denen des letzten vergleichen, wenn dazwischen geloescht wurde, und alles, was gerade
    // faellt, als oberste Ebene dazulegen. Nach dem Aufbau der UI aufrufen.
    pub(crate) fn update_falling_glyphs(&mut self, texts: &[UiText], overlay: &mut UiOverlay) {
        let jetzt = Instant::now();
        // Der Cursor des alten Frames gehoert zu dessen Text (typed_prev).
        let caret = std::mem::replace(&mut self.caret_prev, self.caret_drawn.take());
        let runs = typed_runs(texts.iter().chain(overlay.groups.iter().flat_map(|g| g.texts.iter())));
        // Was der Chat selbst als geloescht meldet -- jedes Bild abholen, sonst gaelte eine alte
        // Meldung fuer ein spaeteres Loeschen. Seine Stelle im alten Frame: der alte Chat-Text
        // (der jetzige mit dem Geloeschten wieder drin) unter den getippten Texten.
        let chat_geloescht = self.chat_input.as_mut().and_then(chat_input::ChatInput::take_erased);
        let genau = chat_geloescht.and_then(|e| {
            let jetzt_text = self.chat_input.as_ref()?.text();
            let alt_text = format!("{}{}{}", jetzt_text.get(..e.start)?, e.text, jetzt_text.get(e.start..)?);
            let a: String = self.typed_prev.iter().map(|r| r.text.as_str()).collect();
            Some((a.find(&alt_text)? + e.start, e.text.len(), e.at_cursor))
        });
        if std::mem::take(&mut self.erase_pending) {
            if let Some(gpu) = self.gpu.as_ref() {
                let vorher = gpu.layout_font.get();
                // In der Schrift des Stuecks messen (s. TypedRun::font); der Text eines Frames
                // hat in allen Stuecken eines Felds dieselbe.
                let mass = |r: &TypedRun, t: &str| {
                    gpu.set_layout_font(r.font);
                    gpu.measure_text(t, r.scale)
                };
                for stueck in erased_pieces(&self.typed_prev, &runs, mass, caret, genau) {
                    let h = gpu.text_height(stueck.scale);
                    for (i, c) in stueck.text.char_indices().filter(|(_, c)| !c.is_whitespace()) {
                        let x = stueck.x + mass(&stueck, &stueck.text[..i]);
                        let zeichen = c.to_string();
                        let w = mass(&stueck, &zeichen);
                        let mut r = [0.0; 6];
                        for v in &mut r {
                            self.erase_seed = self.erase_seed.wrapping_add(1);
                            *v = zufallswert(self.erase_seed ^ 0x5eed_fa11);
                        }
                        self.falling_glyphs.push(FallingGlyph::new(TypedRun { text: zeichen, x, ..stueck.clone() }, w, h, jetzt, r));
                    }
                }
                gpu.set_layout_font(vorher);
            }
        }
        self.typed_prev = runs;
        self.falling_glyphs.retain(|g| jetzt.duration_since(g.start) < g.dur);
        overlay.groups.extend(self.falling_glyphs.iter().filter_map(|g| g.group(jetzt.duration_since(g.start).as_secs_f32())));
    }

    // Aufklappliste der Glocke -- in der Overlay-Ebene, damit kein Menuetext durchscheint.
    pub(crate) fn build_notification_panel(&self, overlay: &mut UiOverlay) {
        if let Some((_, gruppe)) = self.notification_panel_drawn() {
            overlay.groups.push(gruppe);
        }
    }

    pub(crate) fn set_notification_panel(&mut self, open: bool) {
        if open != self.notification_panel_open {
            self.notification_panel_open = open;
            self.notification_panel_since = Instant::now();
        }
    }

    // Fenster der Glocke wie in osu!: ein Viertel der Breite rechts, unter der Leiste bis
    // ganz unten, faehrt von rechts herein und beim Schliessen wieder hinaus. Das Rechteck
    // ist das ganze Fenster an seiner jetzigen Stelle; waehrend es hinausfaehrt, gibt es
    // keins zurueck (s. notification_panel), nur die Zeichnung (s. build_notification_panel).
    pub(crate) fn notification_panel_drawn(&self) -> Option<(Rect, UiOverlayGroup)> {
        let gpu = self.gpu.as_ref()?;
        let panel = self.notification_panel_rect()?;
        use theme::Slot;
        let g = theme::get;
        let x = panel.x;
        let (w, sh) = (panel.w, panel.y + panel.h);
        let pad = 16.0;
        let zeile = gpu.text_height(UI_SCALE_SMALL) + 4.0;
        let klein = gpu.text_height(TOPBAR_SUB_SCALE) + 3.0;
        let mut rects = vec![
            UiRect::shadow(panel, 0.0, 0.0, 24.0, 0.45),
            UiRect { rect: panel, color: g(Slot::BgPanel), ..UiRect::FLAT },
            UiRect { rect: Rect { x, y: TOPBAR_H, w: 1.0, h: panel.h }, color: g(Slot::Border1), ..UiRect::FLAT },
        ];
        let mut texts = Vec::new();
        let mut ty = TOPBAR_H + pad;
        texts.push(UiText { text: "Benachrichtigungen".to_string(), x: x + pad, y: ty, color: theme::text(), scale: UI_SCALE_SMALL });
        if !self.notification_history.is_empty() {
            let anzahl = self.notification_history.len().to_string();
            let aw = gpu.measure_text(&anzahl, TOPBAR_SUB_SCALE);
            texts.push(UiText { text: anzahl, x: x + w - pad - aw, y: ty + 2.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
        }
        ty += zeile + 12.0;
        if self.notification_history.is_empty() {
            texts.push(UiText { text: "Noch keine.".to_string(), x: x + pad, y: ty, color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
        }
        // Je Benachrichtigung eine Karte, die neueste oben; was unten nicht mehr passt, faellt weg.
        let karte_x = x + pad;
        let karte_w = w - 2.0 * pad;
        let innen = 12.0;
        let text_x = karte_x + innen + 6.0;
        let text_w = karte_w - 2.0 * innen - 6.0;
        for (titel, text, art, wann) in self.notification_history.iter().rev() {
            let linien: Vec<String> = wrap_text(gpu, text, TOPBAR_SUB_SCALE, text_w).into_iter().take(6).collect();
            let h = 2.0 * innen + zeile + linien.len() as f32 * klein + if linien.is_empty() { -4.0 } else { 2.0 };
            if ty + h > sh - pad {
                break;
            }
            let karte = Rect { x: karte_x, y: ty, w: karte_w, h };
            let akzent = notification_accent(*art);
            rects.push(UiRect::framed(karte, g(Slot::Surface1), 6.0, 1.0, g(Slot::Border1)));
            rects.push(UiRect::rounded(Rect { x: karte_x + innen - 4.0, y: ty + innen, w: 3.0, h: h - 2.0 * innen }, [akzent[0], akzent[1], akzent[2], 1.0], 1.5));
            let mut zy = ty + innen;
            texts.push(UiText { text: titel.clone(), x: text_x, y: zy, color: theme::text(), scale: UI_SCALE_SMALL });
            let alter = wann.elapsed().as_secs();
            let alter = if alter < 60 { format!("{alter} s") } else if alter < 3600 { format!("{} min", alter / 60) } else { format!("{} h", alter / 3600) };
            let aw = gpu.measure_text(&alter, TOPBAR_SUB_SCALE);
            texts.push(UiText { text: alter, x: karte_x + karte_w - innen - aw, y: zy + 2.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE });
            zy += zeile + 2.0;
            for linie in linien {
                texts.push(UiText { text: linie, x: text_x, y: zy, color: theme::text_2(), scale: TOPBAR_SUB_SCALE });
                zy += klein;
            }
            ty += h + 8.0;
        }
        Some((panel, UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 }))
    }

    // Nur das Rechteck des Fensters an seiner jetzigen Stelle (s. notification_panel_drawn).
    pub(crate) fn notification_panel_rect(&self) -> Option<Rect> {
        let gpu = self.gpu.as_ref().filter(|_| self.chrome_visible())?;
        let t = (self.notification_panel_since.elapsed().as_secs_f32() / NOTIFICATION_PANEL_SLIDE).min(1.0);
        let raus = 1.0 - (1.0 - t).powi(3);
        let sichtbar = if self.notification_panel_open { raus } else { 1.0 - raus };
        if sichtbar <= 0.0 {
            return None;
        }
        let (sw, sh) = gpu.size();
        let w = notification_panel_w(sw);
        let x = (sw - w * sichtbar).round();
        Some(Rect { x, y: TOPBAR_H, w, h: sh - TOPBAR_H })
    }

    // Das Fenster, solange es offen ist (Klicks darauf gehoeren ihm) -- beim Hinausfahren None.
    pub(crate) fn notification_panel(&self) -> Option<(Rect, UiOverlayGroup)> {
        self.notification_panel_drawn().filter(|_| self.notification_panel_open)
    }

    // Einrichtung: Platz fuer den Tastenbelegungs-Assistenten.
    pub(crate) fn build_settings_setup(&self, gpu: &GpuState, texts: &mut Vec<UiText>) {
        let zeilen = [
            "Willkommen. Hier richtest du den Client in wenigen Schritten ein.",
            "Der Assistent für die Tastenbelegung kommt hierhin.",
            "Bis dahin findest du alle Einstellungen links in der Leiste.",
        ];
        for (i, z) in zeilen.iter().enumerate() {
            texts.push(UiText {
                text: z.to_string(),
                x: SETTINGS_LABEL_X,
                y: SETTINGS_FIRST_Y + i as f32 * (gpu.text_height(UI_SCALE_SMALL) + 10.0),
                color: theme::text(),
                scale: UI_SCALE_SMALL,
            });
        }
    }

    pub(crate) fn button_under_cursor(&self) -> Option<ButtonAction> {
        let (px, py) = self.cursor_pos;
        let treffer = |knoepfe: Vec<Button>| knoepfe.into_iter().find(|b| b.enabled && b.hits(px, py)).map(|b| b.action);
        if self.pointer_blocked() {
            return None;
        }
        match self.mode {
            AppMode::Menu if self.module_menu_shown() => {
                // Die Modul-Tafel liegt im Inhalt der Einstellungen, also im Inhaltsraum.
                let (qx, qy) = self.pointer();
                let modul = self.gpu.as_ref().map(|gpu| self.module_menu_buttons(gpu)).unwrap_or_default();
                modul.into_iter().find(|b| b.enabled && b.hits(qx, qy)).map(|b| b.action).or_else(|| treffer(self.buttons()))
            }
            AppMode::Menu => treffer(self.buttons()),
            AppMode::World if self.module_menu_open => self.gpu.as_ref().and_then(|gpu| treffer(self.module_menu_buttons(gpu))),
            AppMode::World => None,
        }
    }

    // Einmal pro Frame: hat sich das Element unter dem Zeiger geaendert, klingt es.
    pub(crate) fn update_hover_sound(&mut self) {
        let jetzt = match self.skin_hover_target() {
            Some(ziel) => ziel,
            None => self.button_under_cursor().map(HoverTarget::Button).or_else(|| self.drawn_hover().map(HoverTarget::Drawn)),
        };
        if jetzt != self.hovered_button {
            self.hovered_button = jetzt;
            if jetzt.is_some() {
                self.play_ui_sound(ui_sound::UiSound::Hover);
            }
        }
    }

    // Was zuletzt mit Hover gezeichnet wurde, solange es frisch ist und der Zeiger noch darauf
    // steht (sonst klaenge das Verlassen eines Knopfs wie ein neues Ziel, bis er ohne Hover
    // gezeichnet ist). Der Zeiger zaehlt auf dem Bildschirm und im Inhaltsraum (s. pointer).
    fn drawn_hover(&self) -> Option<(i32, i32, i32, i32)> {
        let (key, seen) = self.hover_drawn.get()?;
        if seen.elapsed() > Duration::from_millis(200) {
            return None;
        }
        let r = Rect { x: key.0 as f32, y: key.1 as f32, w: key.2 as f32, h: key.3 as f32 };
        let ((ax, ay), (bx, by)) = (self.cursor_pos, self.pointer());
        (r.contains(ax, ay) || r.contains(bx, by)).then_some(key)
    }

    // --- Modul-Menue (In-Welt-Overlay, s. modules.rs) ---
    //
    // Zweispaltig: links die Modulliste (An/Aus je Modul), rechts die Einstellungen des
    // GERADE AUSGEWAEHLTEN Moduls (s. module_menu_selected). Damit waechst die Tafel nicht
    // mit jedem neuen Modul in die Breite.
    //
    // EINE Quelle fuer die Detailzeilen: module_detail_rows() beschreibt sie als Daten,
    // module_menu_buttons() (Trefferpruefung) und build_module_menu() (Zeichnen) lesen
    // beide daraus. Frueher waren das zwei spiegelbildliche Funktionen, die man von Hand
    // synchron halten musste -- eine neue Zeile an nur einer der beiden Stellen ergab einen
    // Knopf ohne Beschriftung oder eine Beschriftung ohne Knopf.
    //
    // Kein Drag-Regler wie im Mockup (Downloads/Mods Menu.dc.html): dieses Menuesystem
    // kennt bisher nur Klick-Ziele (Knopf, Pfeil-Stepper, s. Self::stepper_at) und keine
    // Maus-Drag-Interaktion. Zahlenwerte laufen deshalb ueber "<"/">"-Stepper wie ueberall
    // sonst im Menue -- ein zweites Eingabemodell nur fuer dieses eine Menue waere mehr Code
    // fuer eine Inkonsistenz, nicht fuer eine Verbesserung.

    // Kategorienspalte links, Kachelraster rechts (s. module_menu_rect).
    pub(crate) const MODULE_CAT_W: f32 = 45.0;
    pub(crate) const MODULE_PAD: f32 = 8.0;
    pub(crate) const MODULE_TILE_COLS: usize = 4;
    pub(crate) const MODULE_TILE_W: f32 = 110.0;
    pub(crate) const MODULE_TILE_H: f32 = 26.0;
    pub(crate) const MODULE_TILE_GAP: f32 = 6.0;
    // Hoehe der Uebersicht; die Detailansicht waechst darueber hinaus (s. module_menu_rect).
    pub(crate) const MODULE_PANEL_H: f32 = 280.0;
    // Zeilenhoehe der Kategorienspalte.
    pub(crate) const MODULE_ROW_H: f32 = 20.0;
    pub(crate) const MODULE_TOP_H: f32 = 24.0;
    pub(crate) const MODULE_FOOT_H: f32 = 14.0;
    // Rand, den die Tafel zum Bildschirmrand mindestens frei laesst (s. module_menu_rect).
    pub(crate) const MODULE_SCREEN_MARGIN: f32 = 24.0;

    // Baut die Menu-UI (farbige Flaechen + Text-Runs) fuer diesen Frame. Der Cursor kommt
    // als eigener Rueckgabewert zurueck, nicht als UiRect: er muss als letzter, dritter
    // Zeichendurchgang NACH dem Text gezeichnet werden (s. render_menu), sonst wuerde ihn
    // ueberlappender Text zudecken -- render_menu zeichnet erst alle Flaechen, dann allen
    // Text, in dieser festen Reihenfolge, unabhaengig davon, wann wir ihn hier aufbauen.
    // Inhalt des aktuellen Screens ohne seine Knoepfe: Beschriftungen, Felder, Panels und die
    // Werte, die die Tabs in die Felder ihrer Regler schreiben (s. step_slider_text).
    pub(crate) fn build_screen_content(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        match self.screen {
            MenuScreen::Main => self.build_main_screen(gpu, rects, texts),
            MenuScreen::Connect => self.build_connect_screen(gpu, rects, texts, sprites),
            MenuScreen::Account => self.build_account_screen(gpu, rects, texts),
            MenuScreen::Settings => self.build_settings_hub(gpu, texts),
            MenuScreen::SettingsGeneral => self.build_settings_general_tab(gpu, rects, texts),
            MenuScreen::SettingsVideo => self.build_settings_video_tab(gpu, rects, texts),
            MenuScreen::SettingsAudio => self.build_settings_audio_tab(gpu, rects, texts),
            MenuScreen::SettingsSkin => self.build_settings_skin_tab(gpu, rects, texts, sprites),
            MenuScreen::SettingsControls => self.build_settings_controls_tab(gpu, rects, texts),
            MenuScreen::SettingsInventory => self.build_settings_inventory_tab(gpu, rects, texts),
            MenuScreen::SettingsMisc => {
                let y = (settings_row_y(0) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: "No UI close".to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                let y = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: "No chat clear".to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                let y = (settings_row_y(2) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: "Schild direkt beschreiben".to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                for (row, label) in [(3, "No slot set"), (4, "No rotate set")] {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
            }
            MenuScreen::SettingsFont => self.build_settings_font_tab(gpu, rects, texts),
            MenuScreen::SettingsDesign => self.build_settings_design_tab(gpu, rects, texts),
            MenuScreen::SettingsLanguage => self.build_settings_language_tab(gpu, texts),
            MenuScreen::SettingsModules => self.build_module_menu(gpu, rects, texts),
            MenuScreen::SettingsSensCalc => self.build_sens_calc(gpu, rects, texts),
            MenuScreen::SettingsShader => self.build_settings_shader(gpu, rects, texts),
            MenuScreen::SettingsConfigs => self.build_settings_configs(gpu, rects, texts),
            MenuScreen::SettingsLightLevels => self.build_settings_light_levels(gpu, rects, texts),
            MenuScreen::SettingsParticles => self.build_settings_particles(gpu, rects, texts),
            MenuScreen::SettingsSetup => self.build_settings_setup(gpu, texts),
            MenuScreen::HudEditor => match (self.crosshair_painter.is_some(), self.hud_selected) {
                (true, _) => self.build_crosshair_painter(gpu, rects, texts),
                (false, Some(element)) => self.build_hud_config(gpu, element, rects, texts),
                (false, None) => {}
            },
            MenuScreen::Death => self.build_death_screen(gpu, rects, texts),
        }
    }

    pub(crate) fn build_menu_ui(&self) -> (Vec<UiRect>, Vec<UiText>, Vec<UiSprite>, Option<(f32, f32)>, Vec<UiOverlayGroup>) {
        self.sync_settings_scroll();
        let gpu = self.gpu.as_ref().expect("build_menu_ui ohne GpuState");
        SLIDER_DIGIT_W.store(gpu.measure_raw("8", UI_SCALE_SMALL).to_bits(), std::sync::atomic::Ordering::Relaxed);
        let (screen_w, screen_h) = gpu.size();
        let mut rects: Vec<UiRect> = Vec::new();
        let mut texts: Vec<UiText> = Vec::new();
        let mut sprites: Vec<UiSprite> = Vec::new();

        // Einstellungen fuellen den ganzen Bildschirm (wie in osu!), ohne Welt oder Bild dahinter.
        let einstellungen = self.screen.settings_category().is_some();
        if einstellungen {
            rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: screen_w, h: screen_h }, color: theme::get(theme::Slot::BgApp), ..UiRect::FLAT });
        }
        // HUD-Editor: Raster und Elemente liegen in Bildschirmkoordinaten und bleiben stehen,
        // wenn das Einstellungsfenster (Inhalt) in die Mitte geschoben wird.
        let hud_editor = self.screen == MenuScreen::HudEditor && self.crosshair_painter.is_none();
        if hud_editor {
            self.build_hud_editor(gpu, &mut rects, &mut texts, &mut sprites);
        }
        // Ab hier der Inhalt des Screens in seinen eigenen Koordinaten -- auf den
        // Einstellungsseiten wird er unten um content_offset verschoben.
        let (inhalt_r, inhalt_t, inhalt_s) = (rects.len(), texts.len(), sprites.len());

        // Inhalt des aktuellen Screens. Die Bedienelemente selbst kommen aus buttons()
        // (weiter unten) -- hier nur Beschriftungen, Felder, Panels, Editor-Flaeche.
        self.build_screen_content(gpu, &mut rects, &mut texts, &mut sprites);

        // Buttons (Hintergrund + zentriertes Label) -- was klickbar ist, kommt aus einer
        // Quelle (buttons()), damit Zeichnen und Klick-Trefferpruefung nie auseinanderlaufen.
        for button in self.content_buttons() {
            let hover = !self.pointer_blocked() && button.hits(self.cursor_pos.0, self.cursor_pos.1);
            if let Some(sl) = button.slider {
                // Regler statt Pfeilen. Den Wert hat der Tab schon ins alte Wertfeld
                // geschrieben (Flaeche + zentrierter Text): Flaeche weg, Text in die Anzeige
                // rechts im Regler.
                let feld = sl.value_rect;
                rects.retain(|r| !(r.rect.x == feld.x && r.rect.y == feld.y && r.rect.w == feld.w && r.rect.h == feld.h));
                let mut kasten = sl.value_box();
                let wert_text = (inhalt_t..texts.len()).find(|&i| feld.contains(texts[i].x + 1.0, texts[i].y + 1.0));
                if let Some(i) = wert_text {
                    let alt = std::mem::take(&mut texts[i].text);
                    let mut einheit = Vec::new();
                    let (k, zahl) = push_slider_unit(gpu, kasten, &alt, &mut einheit);
                    kasten = k;
                    let w = gpu.measure_text(&zahl, texts[i].scale);
                    texts[i].text = zahl;
                    texts[i].x = (kasten.x + (kasten.w - w) / 2.0).round();
                    texts.extend(einheit);
                }
                rects.push(UiRect::framed(kasten, theme::get(theme::Slot::SurfaceSunken), 6.0, 1.0, theme::get(theme::Slot::Border1)));
                // Laufende Eingabe: der Puffer statt der Zahl (s. commit_step_edit).
                if let Some((_, buf)) = self.editing_step.as_ref().filter(|(p, _)| *p == sl.prev) {
                    let bisher = wert_text.map(|i| std::mem::take(&mut texts[i].text)).unwrap_or_default();
                    self.push_value_input(gpu, kasten, bisher, buf, &mut rects, &mut texts);
                }
                let t = ((sl.value - sl.min) / (sl.max - sl.min).max(1e-9)) as f32;
                let aktiv = self.step_drag == Some(sl.prev);
                Self::push_slider_visual(&mut rects, sl.track(), t, hover || aktiv, aktiv, button.enabled);
                continue;
            }
            let (flaeche, color) = styled_button(&button, self.hover_level(button.rect, hover));
            let (dx, dy) = self.content_offset();
            // Zurueck in Inhaltskoordinaten, die Verschiebung folgt unten fuer alles zusammen.
            let rect = Rect { x: button.rect.x - dx, y: button.rect.y - dy, ..button.rect };
            rects.push(UiRect { rect, ..flaeche });
            if let Some(c) = Self::choice_of(button.action) {
                let offen = self.dropdown_active().is_some_and(|d| d.choice == c);
                self.push_dropdown_face(gpu, rect, &button.label, color, button.scale, offen, &mut rects, &mut texts);
                continue;
            }
            texts.push(gpu.centered_text(rect, button.label, color, button.scale));
        }
        let (dx, dy) = self.content_offset();
        if einstellungen {
            // Graue Hinweise, Ueberschriften und abgeblendete Werte gibt es auf den
            // Einstellungsseiten nicht (abgeblendete Knoepfe fallen in content_buttons weg).
            let grau = [theme::text_disabled(), theme::text_3()];
            let rest: Vec<UiText> = texts.drain(inhalt_t..).filter(|t| !grau.contains(&t.color)).collect();
            texts.extend(rest);
            let rechts = screen_w - self.side_strip_w(screen_w) - dx - 16.0;
            Self::fit_texts(gpu, &rects[inhalt_r..], &mut texts[inhalt_t..], rechts);
        }
        // Nach dem Einpassen: die Namen der Abschnitte stehen buendig an der rechten Kante, die
        // Kaestchen des Schrift-Tabs zeigen Text in ihrer eigenen Schrift.
        self.build_settings_page_frame(gpu, &mut rects, &mut texts);
        if einstellungen && self.screen == MenuScreen::SettingsFont {
            self.build_font_slots(gpu, &mut rects, &mut texts);
        }
        if dx != 0.0 || dy != 0.0 {
            for r in &mut rects[inhalt_r..] {
                r.rect.x += dx;
                r.rect.y += dy;
            }
            for t in &mut texts[inhalt_t..] {
                t.x += dx;
                t.y += dy;
            }
            for sp in &mut sprites[inhalt_s..] {
                for c in &mut sp.corners {
                    c[0] += dx;
                    c[1] += dy;
                }
            }
        }
        // Das Einstellungsfenster eines HUD-Elements als eigene Overlay-Gruppe ueber dem Editor
        // (sonst laegen die Sprites der Elemente, z.B. die Hotbar, ueber dem Fenster), mit
        // derselben Abdunklung wie die anderen Fenster in der Mitte.
        let hud_fenster = self.hud_config_panel().map(|panel| {
            let (dx, dy) = self.content_offset();
            let p = Rect { x: panel.x + dx, y: panel.y + dy, ..panel };
            let mut fenster = vec![
                UiRect { rect: Rect { x: 0.0, y: 0.0, w: screen_w, h: screen_h }, color: dialog_dim(), ..UiRect::FLAT },
                UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
                UiRect::framed(p, theme::get(theme::Slot::BgPanel), 8.0, 1.0, theme::get(theme::Slot::Border1)),
            ];
            fenster.extend(rects.drain(inhalt_r..));
            UiOverlayGroup { rects: fenster, texts: texts.drain(inhalt_t..).collect(), sprites: sprites.drain(inhalt_s..).collect(), pivot: (0.0, 0.0), angle: 0.0 }
        });
        let mut gruppen: Vec<UiOverlayGroup> = hud_fenster.into_iter().collect();
        if self.chrome_visible() {
            if self.screen == MenuScreen::Connect {
                // Die Leisten als eigene Ebene ueber der Serverliste: eine Karte, die halb unter
                // einer Leiste steckt, wird ganz gezeichnet und von ihr verdeckt -- auch Text und
                // Icon, die im selben Durchgang ueber allen Flaechen laegen.
                let (mut r, mut t, mut s) = (Vec::new(), Vec::new(), Vec::new());
                self.build_bottom_bar(gpu, &mut r, &mut t);
                self.build_chrome(gpu, &mut r, &mut t, &mut s);
                gruppen.insert(0, UiOverlayGroup { rects: r, texts: t, sprites: s, pivot: (0.0, 0.0), angle: 0.0 });
            } else {
                self.build_chrome(gpu, &mut rects, &mut texts, &mut sprites);
            }
        }

        // Kontextabhaengige Hinweiszeile unten. Verschiebemodus zuerst geprueft: raeumt
        // wirklich ALLES an Text weg (auch diese Zeile), Enter/Escape bestaetigen stumm.
        let hint = if self.screen == MenuScreen::HudEditor {
            ""
        } else {
            match (self.screen, self.conn_status) {
                (MenuScreen::Main, ConnStatus::Disconnected) => "Alt+F4 = Beenden",
                (MenuScreen::Main, _) => "Esc = zurück ins Spiel  |  Alt+F4 = Beenden",
                (MenuScreen::Connect, _) if self.server_edit.is_some() => {
                    "Feld anklicken oder Tab zum Wechseln  |  Enter = speichern  |  Esc = abbrechen"
                }
                (MenuScreen::Connect, _) => {
                    "Server anklicken = verbinden  |  ziehen = verschieben  |  Rechtsklick = bearbeiten/löschen"
                }
                (MenuScreen::Death, _) => "Respawn zum Wiederbeleben  |  Alt+F4 = Beenden",
                _ => "Esc = zurück  |  Alt+F4 = Beenden",
            }
        };
        // Einstellungen und Server-Liste kommen ohne Hinweiszeile aus.
        if !einstellungen && self.screen != MenuScreen::Connect {
            texts.push(UiText { text: hint.to_string(), x: UI_MARGIN, y: screen_h - 36.0, color: theme::text_3(), scale: UI_SCALE_SMALL });
        }
        // Perf-Overlay in normalen Menue-Screens mitzeichnen; im HUD-Editor uebernimmt das
        // build_hud_editor (verschiebbare, hervorgehobene Variante).
        if self.screen != MenuScreen::HudEditor {
            // hud_lines (F3-Graphen) faellt hier bewusst weg: debug_overlay fuellt sie nur
            // im World-Modus (s. build_hud), im Menu ist die Liste also immer leer -- und
            // render_menu kennt (anders als render_world) noch keine Linien-Primitive.
            let (hud_rects, _hud_lines, hud_texts, _hud_sprites, _doll) = self.build_hud();
            rects.extend(hud_rects);
            texts.extend(hud_texts);
        }

        // Hover-Tooltip einer Einstellungszeile -- ganz zuletzt gezeichnet, damit er ueber
        // allem anderen liegt (s. hovered_tooltip: nur sichtbar, waehrend der Cursor
        // tatsaechlich ueber der Zeile steht, ersetzt die frueher staendig sichtbaren
        // Hinweiszeilen). Feste Position rechts im Bildschirmbereich statt am Mauszeiger --
        // der Zeiger verdeckt sonst genau die Zeile, die man gerade lesen will, und die Box
        // springt bei jeder Mausbewegung mit.
        if einstellungen && self.screen == MenuScreen::SettingsFont {
            self.build_font_palette(gpu, &mut rects, &mut texts);
        }
        if let Some(text) = self.hovered_tooltip() {
            const PADDING: f32 = 12.0;
            // Auf den Einstellungsseiten im Rand rechts (s. build_chrome), sonst als Kasten.
            let (px, py, pw) = if einstellungen {
                let rand = self.side_strip_w(screen_w);
                // Im Schrift-Tab unter der Auswahl.
                let oben = match self.screen {
                    MenuScreen::SettingsFont => self.font_palette(gpu).bottom,
                    MenuScreen::SettingsSkin => self.skin_strip(gpu).2,
                    _ => SETTINGS_FIRST_Y + SETTINGS_CONTENT_DY,
                };
                (screen_w - rand + 8.0, oben - PADDING, rand - 16.0)
            } else {
                tooltip_panel_origin(screen_w)
            };
            let line_h = gpu.text_height(UI_SCALE_SMALL) + 4.0;
            let lines = wrap_text(gpu, text, UI_SCALE_SMALL, pw - 2.0 * PADDING);
            let box_h = lines.len() as f32 * line_h + 2.0 * PADDING - 4.0;
            if !einstellungen {
                rects.push(UiRect { rect: Rect { x: px, y: py, w: pw, h: box_h }, color: theme::get(theme::Slot::BgPanel), ..UiRect::FLAT });
            }
            for (i, line) in lines.iter().enumerate() {
                texts.push(UiText {
                    text: line.clone(),
                    x: px + PADDING,
                    y: py + PADDING + i as f32 * line_h,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
            }
        }

        // Eigenen Cursor nur, wenn wir die Maus besitzen -- sonst waere er ein zweiter,
        // eingefrorener Zeiger neben dem echten System-Cursor. Position statt Rect: er
        // wird als eigener Overlay-Durchgang gezeichnet, nicht als UiRect (s. oben).
        let cursor = (self.owns_mouse() && !self.drag_hides_cursor()).then_some(self.cursor_pos);

        // Das gezogene Kaestchen des Schrift-Tabs haengt am Zeiger, wo er es gepackt hat.
        if let Some(d) = self.font_drag.filter(|_| einstellungen && self.screen == MenuScreen::SettingsFont) {
            let r = Rect { x: self.cursor_pos.0 - d.grab.0, y: self.cursor_pos.1 - d.grab.1, w: d.tile.box_w(), h: FONT_SLOT_H };
            Self::push_font_slot_tile(gpu, &mut rects, &mut texts, r, Some(d.tile), theme::get(theme::Slot::Accent));
        }

        // Tearing-Test-Balken (nur wenn aktiviert) ueber allen Flaechen: ganz hinten lagen sie
        // unter dem Vollbild-Hintergrund der Einstellungen und den Panels und waren unsichtbar.
        self.push_tearing_bars(&mut rects, screen_w, screen_h);
        self.push_profiler_overlay(gpu, &mut rects, &mut texts);
        self.push_selection_rects(gpu, &mut rects);
        (rects, texts, sprites, cursor, gruppen)
    }

    // Welcher Tooltip (falls ueberhaupt) gerade angezeigt werden soll -- ausschliesslich
    // im Menue und nur, waehrend der Cursor ueber der jeweiligen Einstellungszeile steht (s.
    // settings_row_hover_rect). Jede Einstellungen-Kategorie hat ihre eigene Zeilenliste,
    // gemeinsame Zeilenhoehe/-position mit dem jeweiligen build_*-Renderer.
    pub(crate) fn hovered_tooltip(&self) -> Option<&'static str> {
        self.sync_settings_scroll();
        if self.mode != AppMode::Menu {
            return None;
        }
        let (cx, cy) = self.pointer();
        let hit_row = |count: usize| (0..count).find(|&row| settings_row_hover_rect(row).contains(cx, cy));
        match self.screen {
            MenuScreen::SettingsGeneral => hit_row(SETTINGS_TOOLTIPS.len()).map(|row| SETTINGS_TOOLTIPS[row]),
            // Zeilen 0..2 = Sichtfeld je Perspektive (echt), Zeile 3 = Projektion (echt),
            // Platzhalter ab Zeile 4.
            MenuScreen::SettingsVideo => {
                if let Some(p) = Perspective::ALL.into_iter().find(|p| settings_row_hover_rect(p.fov_row()).contains(cx, cy)) {
                    Some(match p {
                        Perspective::FirstPerson => FOV_TOOLTIP,
                        Perspective::ThirdPersonBack => FOV_THIRD_BACK_TOOLTIP,
                        Perspective::ThirdPersonFront => FOV_THIRD_FRONT_TOOLTIP,
                    })
                // Rechte Spalte: eigene Hover-Rechtecke (right_row_hover_rect), sonst wuerde
                // ein Hover ueber der linken Spalte auf derselben Zeilenhoehe faelschlich
                // diesen Tooltip zeigen -- derselbe Fall wie im Steuerung-Tab.
                } else if Perspective::THIRD_PERSON
                    .into_iter()
                    .any(|p| p.distance_row().is_some_and(|row| settings_row_hover_rect(row).contains(cx, cy)))
                {
                    Some(CAMERA_DISTANCE_TOOLTIP)
                } else if Perspective::THIRD_PERSON
                    .into_iter()
                    .any(|p| p.y_offset_row().is_some_and(|row| settings_row_hover_rect(row).contains(cx, cy)))
                {
                    Some(CAMERA_Y_OFFSET_TOOLTIP)
                } else if DamageTintAxis::ALL.into_iter().any(|a| settings_row_hover_rect(a.row()).contains(cx, cy)) {
                    Some(DAMAGE_TINT_TOOLTIP)
                } else if settings_row_hover_rect(DAMAGE_TINT_SKIN_ONLY_ROW).contains(cx, cy) {
                    Some(DAMAGE_TINT_SKIN_ONLY_TOOLTIP)
                } else if settings_row_hover_rect(DAMAGE_TINT_TICKS_ROW).contains(cx, cy) {
                    Some(DAMAGE_TINT_TICKS_TOOLTIP)
                } else if settings_row_hover_rect(CRIT_COLOR_ROW).contains(cx, cy) {
                    Some(CRIT_COLOR_TOOLTIP)
                } else if settings_row_hover_rect(PROJECTION_ROW).contains(cx, cy) {
                    Some(PROJECTION_TOOLTIP)
                } else if settings_row_hover_rect(SMOOTH_LIGHTING_ROW).contains(cx, cy) {
                    Some(SMOOTH_LIGHTING_TOOLTIP)
                } else if settings_row_hover_rect(PARTICLES_ROW).contains(cx, cy) {
                    Some(PARTICLES_TOOLTIP)
                } else if settings_row_hover_rect(GLINT_SPEED_ROW).contains(cx, cy) {
                    Some(GLINT_SPEED_TOOLTIP)
                } else if settings_row_hover_rect(GLINT_STRENGTH_ROW).contains(cx, cy) {
                    Some(GLINT_STRENGTH_TOOLTIP)
                } else if settings_row_hover_rect(PLAYER_LIST_SCALE_ROW).contains(cx, cy) {
                    Some(PLAYER_LIST_SCALE_TOOLTIP)
                } else if settings_row_hover_rect(BLOCK_ROTATION_ROW).contains(cx, cy) {
                    Some(BLOCK_ROTATION_TOOLTIP)
                } else if settings_row_hover_rect(ARMOR_TRIMS_ROW).contains(cx, cy) {
                    Some(ARMOR_TRIMS_TOOLTIP)
                } else if settings_row_hover_rect(DEATH_ANIM_ROW).contains(cx, cy) {
                    Some(DEATH_ANIM_TOOLTIP)
                } else if settings_row_hover_rect(RENDER_DISTANCE_ROW).contains(cx, cy) {
                    Some(RENDER_DISTANCE_TOOLTIP)
                } else if settings_row_hover_rect(ENTITY_DISTANCE_ROW).contains(cx, cy) {
                    Some(ENTITY_DISTANCE_TOOLTIP)
                } else if settings_row_hover_rect(BLOCK_ENTITY_DISTANCE_ROW).contains(cx, cy) {
                    Some(BLOCK_ENTITY_DISTANCE_TOOLTIP)
                } else if settings_row_hover_rect(LOD_DETAIL_ROW).contains(cx, cy) {
                    Some(LOD_DETAIL_TOOLTIP)

                } else {
                    None
                }
            }
            MenuScreen::SettingsAudio => {
                if settings_row_hover_rect(AUDIO_DEVICE_ROW).contains(cx, cy) {
                    Some("Wählt das Ausgabegerät für den Testton (und später echte Spielgeräusche). \"System-Standard\" folgt automatisch der Systemeinstellung.")
                } else if settings_row_hover_rect(AUDIO_TEST_TONE_ROW).contains(cx, cy) {
                    Some("Spielt einen kurzen Testton auf dem gewählten Ausgabegerät ab, um Gerät + Stream zu prüfen.")
                } else if settings_row_hover_rect(AUDIO_PITCH_ROW).contains(cx, cy) {
                    Some(UI_PITCH_TOOLTIP)
                } else if settings_row_hover_rect(AUDIO_BUFFER_ROW).contains(cx, cy) {
                    Some("Größe des Ausgabepuffers. Kleiner = weniger Verzögerung, aber anfälliger für Knackser und Aussetzer. Bei Bluetooth-Kopfhörern 1024 oder System-Vorgabe nehmen: die Funkstrecke kostet ohnehin 100 ms und mehr.")
                } else if settings_row_hover_rect(AUDIO_BUFFER_INFO_ROW).contains(cx, cy) {
                    Some("Womit der Stream gerade läuft: angefragte Größe, in Klammern was das System tatsächlich je Durchlauf anfordert, falls es abweicht.")
                } else if settings_row_hover_rect(AUDIO_PIPEWIRE_ROW).contains(cx, cy) {
                    Some("In welchen Blöcken PipeWire die Soundkarte gerade bedient, gemessen mit pw-top, solange dieser Tab offen ist. PipeWire rundet auf eine Zweierpotenz ab: ein Puffer von 48 läuft hier als 32 oder 64.")
                } else if settings_row_hover_rect(AUDIO_AUTO_ROW).contains(cx, cy) {
                    Some("Nur unter Linux mit PipeWire. Senkt die PipeWire-Grenze passend zum Puffer und startet pipewire-pulse dafür nur neu, wenn die Grenze noch nicht stimmt. Fehlen pipewire-pulse Echtzeit oder Nice -11, holt der Client das alle 10 Sekunden über RTKit nach, ohne Neustart. Der eigene Ton-Faden des Clients bekommt ebenfalls Echtzeit, damit es beim Laden nicht knackt.")
                } else if settings_row_hover_rect(AUDIO_PIPEWIRE_PRIO_ROW).contains(cx, cy) {
                    Some("Wie pipewire-pulse eingeplant ist: Echtzeit-Priorität seiner Ton-Fäden und Nice des Haupt-Threads. Normal ist Echtzeit 20 · Nice -11. Ohne Echtzeit kommt der Ton bei kleinen Blöcken ab und zu zu spät und rauscht; nach dem Setzen der Grenze holt der Client die Priorität über RTKit nach.")
                } else if settings_row_hover_rect(AUDIO_PIPEWIRE_SET_ROW).contains(cx, cy) {
                    Some("Unter PipeWire läuft der Ton über pipewire-pulse, und das mischt nie in kleineren Blöcken als 128 Frames (2,7 ms) -- ein kleinerer Puffer bringt dann nichts. Der Knopf legt ~/.config/pipewire/pipewire-pulse.conf.d/90-coral-client.conf an und startet pipewire-pulse neu: alle Programme verlieren kurz den Ton, und die Grenze gilt danach für alle. Entfernen stellt die Vorgabe wieder her.")
                } else if settings_row_hover_rect(AUDIO_SPATIAL_ROW).contains(cx, cy) {
                    Some(SPATIAL_TEST_TOOLTIP)
                } else if settings_row_hover_rect(AUDIO_MASTER_ROW).contains(cx, cy) {
                    Some(MASTER_VOLUME_TOOLTIP)
                } else {
                    VOLUME_CATEGORIES
                        .iter()
                        .enumerate()
                        .find(|&(i, _)| settings_row_hover_rect(AUDIO_CATEGORY_FIRST_ROW + i).contains(cx, cy))
                        .map(|(_, c)| c.tooltip)
                        .or_else(|| {
                            ui_sound::UiSound::ALL.iter().enumerate().find(|&(i, _)| settings_row_hover_rect(AUDIO_UI_SOUND_FIRST_ROW + i).contains(cx, cy)).map(|(_, s)| s.tooltip())
                        })
                }
            }
            // Ohne Erklaerungsrand (s. side_strip_w).
            MenuScreen::SettingsDesign => None,
            MenuScreen::SettingsMisc => {
                if settings_row_hover_rect(0).contains(cx, cy) {
                    Some("Schließt der Server ein offenes Fenster, bleibt es trotzdem offen. Immer: jedes Fenster (Truhe, Inventar, Schild-Editor ...). Nur Inventar: nur das eigene Inventar, Truhen & Co. schließt der Server weiter. Kein Vanilla-Verhalten: Klicks in einen Container, den der Server schon geschlossen hat, nimmt er nicht mehr an.")
                } else if settings_row_hover_rect(2).contains(cx, cy) {
                    Some("An: man schreibt direkt auf das Schild in der Welt, solange man die Seite sieht. Aus: immer das Editor-Fenster wie in Vanilla.")
                } else if settings_row_hover_rect(3).contains(cx, cy) {
                    Some("Wenn der Server deinen Hotbar-Slot wechselt. Aus: wie Vanilla, der Slot wechselt. Ignore: dein Slot bleibt, dem Server wird nichts gemeldet -- für ihn gilt seiner, bis du selbst wechselst. Reset: dein Slot bleibt und geht im nächsten Tick an den Server zurück.")
                } else if settings_row_hover_rect(4).contains(cx, cy) {
                    Some("Wenn der Server deine Blickrichtung dreht (Teleport, Kamera-Effekte). Aus: wie Vanilla. Ignore: deine Kamera bleibt, dem Server wird nichts gemeldet -- für ihn gilt seine Richtung, bis du dich bewegst. Reset: deine Kamera bleibt und deine Richtung geht im nächsten Tick an den Server. Die Bestätigung des Teleports trägt immer die Richtung des Servers, sonst flaggen Anticheats sofort.")
                } else {
                    None
                }
            }
            MenuScreen::SettingsLanguage => {
                if settings_row_hover_rect(LATIN_MACRONS_ROW).contains(cx, cy) {
                    Some(LATIN_MACRONS_TOOLTIP)
                } else if settings_row_hover_rect(LOANWORDS_ROW).contains(cx, cy) {
                    Some(i18n::LOANWORDS_TOOLTIP)
                } else {
                    LANGUAGE_ROWS.iter().enumerate().find(|(row, _)| settings_row_hover_rect(*row).contains(cx, cy)).map(|(_, z)| z.1)
                }
            }
            // Ohne Zeile unter dem Zeiger steht da, wie man etwas aendert.
            // Zeile "Standard": links die Schrift, ab dem Schatten-Kaestchen der Schatten.
            MenuScreen::SettingsFont => Some(if settings_row_hover_rect(0).contains(cx, cy) {
                if cx < FONT_SHADOW_X - 5.0 { FONT_TOOLTIP } else { TEXT_SHADOW_TOOLTIP }
            } else if settings_row_hover_rect(STABLE_NUMBERS_ROW).contains(cx, cy) {
                STABLE_NUMBERS_TOOLTIP
            } else if (FONT_EXCEPTIONS_ROW..FONT_EXCEPTIONS_ROW + TEXT_AREA_COUNT).any(|row| settings_row_hover_rect(row).contains(cx, cy)) {
                AREA_SHADOW_TOOLTIP
            } else {
                FONT_DRAG_HINT
            }),
            MenuScreen::SettingsShader if settings_row_hover_rect(5).contains(cx, cy) => Some(
                "Zur Fehlersuche, statt des Bildes. Licht: wie viel Licht der Lampen ankommt -- gelb am meisten, blau am wenigsten, schwarz keins, rot die Grenze, wo es aufhört. Abweichung: wie weit die Lichtkarten von echten Strahlen abweichen -- schwarz gleich, rot zu hell, blau zu dunkel (voll bei 25 %).",
            ),
            MenuScreen::SettingsShader if settings_row_hover_rect(4).contains(cx, cy) => Some(
                "Für jede Lichtquelle ein eigener Faktor auf ihre Leuchtstärke (Shader-Pfad): 50 % = halb so hell und halb so weit, 0 % = leuchtet gar nicht. Über 100 % wird es heller und reicht weiter (bis 1000 %, dann bis zu 150 Blöcke).",
            ),
            MenuScreen::SettingsShader if settings_row_hover_rect(3).contains(cx, cy) => Some(
                "Wie weich die Schatten von Fackeln, Laternen und anderen Lichtquellen werden, als scheinbare Größe der Quelle in Grad. Wie bei der Sonne: direkt am Hindernis scharf, mit Abstand immer weicher. 0 = überall scharf.",
            ),
            MenuScreen::SettingsShader if settings_row_hover_rect(2).contains(cx, cy) => Some(
                "Fackeln, Laternen, Glowstone und andere Lichtquellen werfen Schatten: Was zwischen Quelle und Fläche steht, hält ihr Licht ab, Buntglas färbt es. Ein Rest bleibt als zurückgeworfenes Licht. Kostet GPU-Zeit, wo viele Quellen nah beieinander sind.",
            ),
            MenuScreen::SettingsShader if settings_row_hover_rect(1).contains(cx, cy) => Some(
                "Wie weich Schattenkanten werden, als scheinbare Größe der Sonne in Grad. Direkt an einer Wand bleibt der Schatten scharf, mit wachsendem Abstand zum Hindernis wird der Übergang breiter (Halbschatten). Die echte Sonne hat 0,5°, 0 = überall scharf.",
            ),
            MenuScreen::SettingsShader => settings_row_hover_rect(0).contains(cx, cy).then_some(
                "Wie stark die Luft Licht streut (Shader-Pfad, Oberwelt). Physikalisch die Dichte der streuenden Teilchen: 100 % = Erdatmosphäre, 0 % = keine Luft und ein schwarzer Himmel wie im Weltraum, mehr = dichtere, dunstigere Luft mit blasserem Himmel und röteren Sonnenuntergängen. Bestimmt auch das Umgebungslicht vom Himmel: ohne Luft sind Schatten tiefschwarz, mit mehr Luft heller.",
            ),
            MenuScreen::SettingsInventory => {
                if settings_row_hover_rect(2).contains(cx, cy) {
                    Some("Legt man ein Item in einen Slot, springt der Mauszeiger in die Mitte dieses Slots. Nicht beim Stapel-Verschieben (Shift). Kein Vanilla-Verhalten.")
                } else if settings_row_hover_rect(4).contains(cx, cy) {
                    Some("Hält man einen Stapel über einen Slot mit demselben Item, steht am Zeiger die Summe beider Stapel und die Zahl im Slot wird ausgeblendet, statt dass sich zwei Zahlen überlagern.")
                } else if settings_row_hover_rect(5).contains(cx, cy) {
                    Some("Kleines S neben dem Flaschenhals verstärkter Tränke (Stufe II), z. B. Stärke II oder Schnelligkeit II. Gilt im Inventar, nicht in der Hotbar.")
                } else if settings_row_hover_rect(6).contains(cx, cy) {
                    Some("Kleines L neben dem Flaschenhals verlängerter Tränke (mit Redstone gebraut). Gilt im Inventar, nicht in der Hotbar.")
                } else if settings_row_hover_rect(7).contains(cx, cy) {
                    Some("Kleines W auf Heil- und Schadenstränken der schwachen Stufe I. Stufe II bekommt keinen Buchstaben, weil sie im PvP der Normalfall ist. Gilt im Inventar, nicht in der Hotbar.")
                } else {
                    settings_row_hover_rect(0).contains(cx, cy).then_some(
                        "Wie durchsichtig der abgedunkelte Hintergrund hinter dem offenen Inventar ist. 0 % = deckend schwarz, 100 % = die Welt bleibt unverändert sichtbar. Zahl anklicken für einen eigenen Wert.",
                    )
                }
            }
            MenuScreen::SettingsControls => {
                let row = (0..controls_rows().len()).find(|&row| settings_row_hover_rect(row).contains(cx, cy))?;
                match controls_rows()[row] {
                    ControlsRow::SensX | ControlsRow::SensY | ControlsRow::SensSplit => Some(SENSITIVITY_TOOLTIP),
                    ControlsRow::SensCalc => None,
                    ControlsRow::CursorSens => Some(CURSOR_SENSITIVITY_TOOLTIP),
                    ControlsRow::CursorSize => Some(CURSOR_SIZE_TOOLTIP),
                    ControlsRow::CursorWidth => Some(CURSOR_WIDTH_TOOLTIP),
                    ControlsRow::HoldConfirm => Some(HOLD_CONFIRM_TOOLTIP),
                    ControlsRow::ReconnectHold => Some(RECONNECT_HOLD_TOOLTIP),
                    ControlsRow::CursorCircle => Some(CURSOR_CIRCLE_TOOLTIP),
                    ControlsRow::SneakProtection => Some(SNEAK_PROTECTION_TOOLTIP),
                    ControlsRow::SnapTap => Some(SNAP_TAP_TOOLTIP),
                    ControlsRow::JumpCooldown => Some(JUMP_COOLDOWN_TOOLTIP),
                    ControlsRow::SprintOnStart => Some(SPRINT_ON_START_TOOLTIP),
                    ControlsRow::HoldUseRepeat => Some(HOLD_USE_REPEAT_TOOLTIP),
                    ControlsRow::RocketRepeat => Some(ROCKET_REPEAT_TOOLTIP),
                    ControlsRow::ExtinguishDcGhost => Some(EXTINGUISH_DC_GHOST_TOOLTIP),
                    ControlsRow::UseWhileMining => Some(USE_WHILE_MINING_TOOLTIP),
                    ControlsRow::CapsLock => Some(CAPS_LOCK_TOOLTIP),
                    ControlsRow::Bind(_) => Some(KEYBIND_TOOLTIP),
                }
            }
            MenuScreen::SettingsSkin => self.skin_files.is_none().then_some(SKIN_TAB_HINT),
            _ => None,
        }
    }

    // Main-Screen: getrennt -> Startbildschirm (nur Titel + Buttons Verbinden/Einstellungen;
    // die Adress-Eingabe steckt jetzt im Connect-Screen), verbunden -> Esc-Pausemenue
    // (nur Titel). Die Buttons kommen aus buttons().
    pub(crate) fn build_main_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        // Titel wie die Buttons mittig.
        let (sw, _) = gpu.size();
        let titel = |text: &str| {
            let w = gpu.measure_text(text, UI_SCALE_TITLE);
            UiText { text: text.to_string(), x: ((sw - w) / 2.0).round().max(UI_MARGIN), y: 90.0, color: theme::text(), scale: UI_SCALE_TITLE }
        };
        if self.conn_status == ConnStatus::Disconnected {
            if gpu.wallpaper.is_some() {
                let (w, h) = gpu.size();
                rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w, h }, color: WALLPAPER_DIM, ..UiRect::FLAT });
            }
            texts.push(titel("Coral Client"));
        } else {
            // Nur der Titel; die Chunk-Zahlen stehen im F3-Overlay.
            texts.push(titel("Pause"));
        }
    }

    // HUD-Editor: Raster zum Ausrichten + das verschiebbare Perf-Element (hervorgehoben).
    // Fertig-Button und Sichtbarkeits-Schalter kommen aus buttons().
    // Lage des Malrasters: (x, y der linken oberen Ecke, Kantenlaenge einer Zelle).
    pub(crate) fn crosshair_painter_grid(&self, gpu: &GpuState) -> (f32, f32, f32) {
        let (sw, sh) = gpu.size();
        let links = 40.0 + HUD_LIST_ITEM_W + 40.0;
        let vorschau = 200.0;
        let n = crosshair_editor::GRID as f32;
        let zelle = ((sh - 200.0).min(sw - links - vorschau - 40.0) / n).floor().clamp(4.0, 20.0);
        let g = zelle * n;
        let x0 = (links + ((sw - links - vorschau - g) / 2.0).max(0.0)).round();
        let y0 = ((sh - g) / 2.0).round();
        (x0, y0, zelle)
    }

    pub(crate) fn crosshair_painter_cell(&self, p: (f32, f32)) -> Option<(i32, i32)> {
        let gpu = self.gpu.as_ref()?;
        let (x0, y0, zelle) = self.crosshair_painter_grid(gpu);
        let (x, y) = (((p.0 - x0) / zelle).floor() as i32, ((p.1 - y0) / zelle).floor() as i32);
        let n = crosshair_editor::GRID as i32;
        ((0..n).contains(&x) && (0..n).contains(&y)).then_some((x, y))
    }

    // Maustaste im Pixel-Editor. Links malt Fuellung, rechts Kontur; beginnt der Strich auf
    // einem Pixel, das schon so aussieht, loescht er stattdessen. true = Klick verbraucht.
    pub(crate) fn crosshair_painter_mouse(&mut self, rechts: bool, gedrueckt: bool) -> bool {
        let Some(painter) = self.crosshair_painter.as_ref() else { return false };
        if !gedrueckt {
            let hatte = self.crosshair_painter.as_mut().and_then(|p| p.stroke.take()).is_some();
            if hatte {
                self.persist_settings();
            }
            return hatte;
        }
        let Some(zelle) = self.crosshair_painter_cell(self.cursor_pos) else { return false };
        let werkzeug = if rechts { crosshair_editor::Pixel::Outline } else { crosshair_editor::Pixel::Fill };
        let jetzt = self.settings.crosshair_pattern.get(zelle.0, zelle.1);
        let wert = if jetzt == werkzeug { crosshair_editor::Pixel::Empty } else { werkzeug };
        let sym = painter.symmetry;
        self.settings.crosshair_pattern.paint(zelle.0, zelle.1, wert, sym);
        if let Some(p) = self.crosshair_painter.as_mut() {
            p.stroke = Some((wert, zelle));
        }
        true
    }

    // Zeiger bewegt: laufenden Strich bis zur neuen Zelle ziehen, ohne Luecken bei schnellen
    // Bewegungen.
    pub(crate) fn crosshair_painter_drag(&mut self) {
        let Some(painter) = self.crosshair_painter.as_ref() else { return };
        let Some((wert, (mut x, mut y))) = painter.stroke else { return };
        let sym = painter.symmetry;
        let Some((zx, zy)) = self.crosshair_painter_cell(self.cursor_pos) else { return };
        // Bresenham von der letzten zur neuen Zelle.
        let (dx, dy) = ((zx - x).abs(), -(zy - y).abs());
        let (sx, sy) = ((zx - x).signum(), (zy - y).signum());
        let mut err = dx + dy;
        loop {
            self.settings.crosshair_pattern.paint(x, y, wert, sym);
            if x == zx && y == zy {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        if let Some(p) = self.crosshair_painter.as_mut() {
            p.stroke = Some((wert, (zx, zy)));
        }
    }

    // Der Pixel-Editor: Raster in Vergroesserung, Mittellinien, Vorschau in Originalgroesse.
    pub(crate) fn build_crosshair_painter(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.05, 0.05, 0.07, 0.92], ..UiRect::FLAT });
        let (x0, y0, zelle) = self.crosshair_painter_grid(gpu);
        let n = crosshair_editor::GRID as i32;
        let p = &self.settings.crosshair_pattern;
        // Eine Flaeche plus Rasterlinien plus ein Viereck je Lauf gemalter Pixel -- ein Viereck
        // je Zelle waeren 48x48, weit mehr als der UI-Puffer fasst.
        let g = zelle * n as f32;
        let mitte = (x0 + g / 2.0, y0 + g / 2.0);
        rects.push(UiRect { rect: Rect { x: x0, y: y0, w: g, h: g }, color: [0.0, 0.0, 0.0, 0.55], ..UiRect::FLAT });
        let raster = [1.0, 1.0, 1.0, 0.06];
        for i in 1..n {
            let o = (i as f32 * zelle).round();
            rects.push(UiRect { rect: Rect { x: x0 + o, y: y0, w: 1.0, h: g }, color: raster, ..UiRect::FLAT });
            rects.push(UiRect { rect: Rect { x: x0, y: y0 + o, w: g, h: 1.0 }, color: raster, ..UiRect::FLAT });
        }
        push_pattern_rects(p, mitte, zelle, rects);
        // Die Bildmitte (Blickstrahl) liegt zwischen den vier mittleren Pixeln.
        let linie = [1.0, 0.3, 0.3, 0.7];
        rects.push(UiRect { rect: Rect { x: mitte.0 - 0.5, y: y0, w: 1.0, h: g }, color: linie, ..UiRect::FLAT });
        rects.push(UiRect { rect: Rect { x: x0, y: mitte.1 - 0.5, w: g, h: 1.0 }, color: linie, ..UiRect::FLAT });

        // Vorschau 1:1 -- so gross erscheint es im Spiel bei dieser Aufloesung.
        let kasten = Rect { x: x0 + g + 40.0, y: mitte.1 - 50.0, w: 100.0, h: 100.0 };
        rects.push(UiRect { rect: kasten, color: [0.45, 0.55, 0.62, 1.0], ..UiRect::FLAT });
        push_pattern_rects(p, ((kasten.x + kasten.w / 2.0).round(), (kasten.y + kasten.h / 2.0).round()), 1.0, rects);
        texts.push(UiText { text: "1:1".to_string(), x: kasten.x, y: kasten.y + kasten.h + 8.0, color: theme::text(), scale: UI_SCALE_SMALL });

        texts.push(UiText {
            text: "Links: Füllung   Rechts: Kontur   Auf gleichem Pixel: löschen   Esc: fertig".to_string(),
            x: x0,
            y: (y0 + g + 12.0).min(sh - 30.0),
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
    }

    // Einstellungen eines HUD-Elements (Rechtsklick im Editor): Titel, Zeilen wie auf einer
    // Einstellungsseite, darunter Schrift und Zuruecksetzen. In Inhaltskoordinaten --
    // build_menu_ui schiebt es in die Bildmitte und legt es als Fenster ueber den Editor.
    pub(crate) fn build_hud_config(&self, gpu: &GpuState, element: HudElement, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let panel = hud_config_panel(element, &self.settings);
        texts.push(UiText { text: element.label().to_string(), x: HUD_CONFIG_LABEL_X, y: panel.y + 22.0, color: theme::text(), scale: UI_SCALE_NORMAL });
        let row0_y = (settings_row_y(0) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        match element {
            HudElement::Crosshair => {
                texts.push(UiText {
                    text: "Kreuz-Farbe".to_string(),
                    x: HUD_CONFIG_LABEL_X,
                    y: row0_y,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
                // Kontur gilt fuer jeden Farbmodus (Zeile 1), s. buttons().
                let row1_y = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText {
                    text: "Kontur".to_string(),
                    x: HUD_CONFIG_LABEL_X,
                    y: row1_y,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
                let zeile = crosshair_shape_row(self.settings.crosshair_color_mode);
                let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: "Form".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(zeile), color: theme::text(), scale: UI_SCALE_SMALL });
                // Die Form selbst ist eine Aufklappliste aus buttons().
                if !self.settings.crosshair_custom {
                    texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(zeile + 1), color: theme::text(), scale: UI_SCALE_SMALL });
                    let groesse_rect = stepper_value_rect(zeile + 1);
                    rects.push(UiRect { rect: groesse_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(groesse_rect, format!("{} px", self.settings.crosshair_size()), theme::text(), UI_SCALE_SMALL));
                }
                texts.push(UiText { text: "Ring hat Kontur".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(2), color: theme::text(), scale: UI_SCALE_SMALL });
                // Aufhellung nur beim Komplementaer-Modus (Zeile 3).
                if self.settings.crosshair_color_mode == CROSSHAIR_MODE_COMPLEMENTARY {
                    texts.push(UiText { text: "Aufhellung".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(3), color: theme::text(), scale: UI_SCALE_SMALL });
                    let boost_rect = stepper_value_rect(3);
                    rects.push(UiRect { rect: boost_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(
                        boost_rect,
                        format!("{} %", self.settings.crosshair_brightness_boost),
                        theme::text(),
                        UI_SCALE_SMALL,
                    ));
                }
            }
            HudElement::Chat => {
                for (row, (label, wert)) in self.chat_setting_rows().into_iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: label.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                    // Die Wahl Java/Bedrock sind zwei Knoepfe (s. buttons), kein Wertfeld.
                    if row == CHAT_STYLE_ROW {
                        continue;
                    }
                    let rect = stepper_value_rect(row);
                    rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(rect, wert, theme::text(), UI_SCALE_SMALL));
                }
                // Erklaerung der Zeile unter dem Zeiger in einem Kasten direkt unter dem Fenster
                // (die Einstellungsseiten haben dafuer den Rand rechts, der HUD-Editor nicht).
                let (px, py) = self.pointer();
                let zeile = (0..CHAT_TOOLTIPS.len()).find(|&row| Rect { x: panel.x, y: settings_row_y(row), w: panel.w, h: SETTINGS_ROW_H }.contains(px, py));
                if let Some(row) = zeile {
                    const PADDING: f32 = 12.0;
                    let line_h = gpu.text_height(UI_SCALE_SMALL) + 4.0;
                    let lines = wrap_text(gpu, CHAT_TOOLTIPS[row], UI_SCALE_SMALL, panel.w - 2.0 * PADDING);
                    let kasten = Rect { x: panel.x, y: panel.y + panel.h + 8.0, w: panel.w, h: lines.len() as f32 * line_h + 2.0 * PADDING - 4.0 };
                    rects.push(UiRect::framed(kasten, theme::get(theme::Slot::BgPanel), 8.0, 1.0, theme::get(theme::Slot::Border1)));
                    for (i, line) in lines.into_iter().enumerate() {
                        texts.push(UiText { text: line, x: kasten.x + PADDING, y: kasten.y + PADDING + i as f32 * line_h, color: theme::text(), scale: UI_SCALE_SMALL });
                    }
                }
            }
            HudElement::Hotbar => {
                texts.push(UiText {
                    text: "Skalierung".to_string(),
                    x: HUD_CONFIG_LABEL_X,
                    y: row0_y,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
                let label = Self::scale_label(self.settings.hud_hotbar_scale, gpu.size());
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                texts.push(gpu.centered_text(value_rect, label, theme::text(), UI_SCALE_SMALL));
                {
                    let row1_y = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Haltbarkeit ab".to_string(), x: HUD_CONFIG_LABEL_X, y: row1_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let pct = self.settings.hotbar_durability_percent;
                    let text = if self.settings.hotbar_durability_absolute {
                        format!("\u{2264} {} Punkte", self.settings.hotbar_durability_points)
                    } else if pct >= 100 {
                        "immer".to_string()
                    } else {
                        format!("\u{2264} {pct} %")
                    };
                    let value_rect = stepper_value_rect(1);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, text, theme::text(), UI_SCALE_SMALL));
                    let row2_y = (settings_row_y(2) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Grenze".to_string(), x: HUD_CONFIG_LABEL_X, y: row2_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let row3_y = (settings_row_y(3) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Sauerstoff".to_string(), x: HUD_CONFIG_LABEL_X, y: row3_y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
            }
            HudElement::Title | HudElement::Subtitle | HudElement::ActionBar => {
                texts.push(UiText { text: "Ausrichtung".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                if element != HudElement::ActionBar {
                    let pct = if element == HudElement::Title { self.settings.hud_title_scale } else { self.settings.hud_subtitle_scale };
                    let row1_y = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row1_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let value_rect = stepper_value_rect(1);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, format!("{pct} %"), theme::text(), UI_SCALE_SMALL));
                }
            }
            HudElement::BossBar => {
                texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                texts.push(gpu.centered_text(value_rect, format!("{} %", self.settings.hud_bossbar_scale), theme::text(), UI_SCALE_SMALL));
            }
            HudElement::Effects => {
                texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                texts.push(gpu.centered_text(value_rect, format!("{} %", self.settings.hud_effects_scale), theme::text(), UI_SCALE_SMALL));
                let mut zeilen = vec![(1, "Im HUD"), (2, "Im Inventar"), (EFFECTS_HIDE_AMBIENT_ROW, "Ambiente verstecken"), (EFFECTS_STYLE_ROW, "Stil")];
                if self.settings.effects_detailed {
                    zeilen.extend([
                        (EFFECTS_ALIGN_ROW, "Ausrichtung"),
                        (EFFECTS_ICON_ROW, "Icon"),
                        (EFFECTS_NAME_ROW, "Name"),
                        (EFFECTS_TIMER_ROW, "Timer"),
                        (EFFECTS_TIMER_COLOR_ROW, "Timer-Farbe"),
                        (EFFECTS_BLINK_ROW, "Blinken ab 10 s"),
                    ]);
                }
                for (row, text) in zeilen {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
            }
            HudElement::Scoreboard | HudElement::HeldName => {
                texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                let pct = if element == HudElement::Scoreboard { self.settings.hud_scoreboard_scale } else { self.settings.hud_held_name_scale };
                texts.push(gpu.centered_text(value_rect, format!("{pct} %"), theme::text(), UI_SCALE_SMALL));
            }
            HudElement::ShieldCooldown | HudElement::Arrows => {
                let zeilen: &[&str] = if element == HudElement::Arrows {
                    &["Größe", "Nur Zahl", "Deckkraft", "Zahl-Farbe"]
                } else {
                    &["Farbe"]
                };
                for (row, text) in zeilen.iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                if element == HudElement::Arrows {
                    for (row, wert) in [(0, self.settings.hud_arrows_scale), (2, self.settings.arrow_opacity)] {
                        let value_rect = stepper_value_rect(row);
                        rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                        texts.push(gpu.centered_text(value_rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
                    }
                }
            }
            HudElement::Keystrokes | HudElement::Session => {
                let werte = if element == HudElement::Session {
                    [("Größe", self.settings.hud_session_scale), ("Deckkraft", self.settings.session_opacity)]
                } else {
                    [("Größe", self.settings.hud_keys_scale), ("Deckkraft", self.settings.keys_opacity)]
                };
                for (row, (text, wert)) in werte.into_iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let value_rect = stepper_value_rect(row);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
                }
            }
            HudElement::Armor => {
                for (row, text) in ["Größe", "Ausrichtung", "Rücken nach", "Abstand", "Zahl-Größe", "Zahl X", "Zahl Y", "Grün bei >87 %"].into_iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                let set = &self.settings;
                for (row, wert) in [(0, format!("{} %", set.hud_armor_scale)), (3, format!("{} px", set.armor_hud_gap)), (4, format!("{} %", set.armor_hud_number_scale)), (5, format!("{} px", set.armor_hud_number_x)), (6, format!("{} px", set.armor_hud_number_y))] {
                    let value_rect = stepper_value_rect(row);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, wert, theme::text(), UI_SCALE_SMALL));
                }
            }
            // Die Performance-Anzeige hat keine eigenen Werte.
            HudElement::Perf => {}
        }
    }

    // Raster und alle Elemente des HUD-Editors, in Bildschirmkoordinaten (s. build_menu_ui).
    pub(crate) fn build_hud_editor(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        let (sw, sh) = gpu.size();
        // Duenne Rasterlinien (als schmale Rechtecke) -- Orientierung beim Ausrichten.
        let mut x = HUD_EDITOR_GRID_PX;
        while x < sw {
            rects.push(UiRect { rect: Rect { x, y: 0.0, w: 1.0, h: sh }, color: COLOR_GRID_LINE, ..UiRect::FLAT });
            x += HUD_EDITOR_GRID_PX;
        }
        let mut y = HUD_EDITOR_GRID_PX;
        while y < sh {
            rects.push(UiRect { rect: Rect { x: 0.0, y, w: sw, h: 1.0 }, color: COLOR_GRID_LINE, ..UiRect::FLAT });
            y += HUD_EDITOR_GRID_PX;
        }

        // Ist gerade ein Element im Zug -- fuer die aktive Griff-Farbe (s. hud_drag_offset).
        let dragging = self.hud_drag_offset.map(|(e, _, _)| e);
        let pad = 6.0;

        // Alle eingeschalteten Elemente, jedes mit Griff-Rahmen zum Anfassen.
        if self.hud_element_shown(HudElement::Perf) {
            let rect = self.hud_perf_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Perf) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            self.push_hud_perf_texts(gpu, COLOR_PERF, texts);
        }

        {
            // Griff-Rahmen um die tatsaechliche Position, plus ein "+" als Vorschau (immer
            // in COLOR_HUD -- der echte Kreuz-Pass laeuft nur in AppMode::World, hier reicht
            // eine einfache statische Andeutung).
            let ch_rect = self.hud_crosshair_rect(gpu);
            let farbe = if dragging == Some(HudElement::Crosshair) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE };
            rects.push(UiRect::rounded(ch_rect, farbe, ch_rect.w / 2.0));
            let mitte = ((sw / 2.0).round(), (sh / 2.0).round());
            if self.settings.crosshair_custom {
                push_pattern_rects(&self.settings.crosshair_pattern, mitte, 1.0, rects);
            } else {
                for (teil, kontur) in crosshair_parts(self.settings.crosshair_shape, self.settings.crosshair_size(), mitte, self.settings.crosshair_outline) {
                    rects.push(crosshair_part_rect(teil, if kontur { [0.0, 0.0, 0.0, 1.0] } else { COLOR_HUD }));
                }
            }
        }

        // Scoreboard: Griff-Rahmen + Platzhalter-Panel (echter Inhalt nur bei Verbindung, hier
        // reicht ein Kasten mit "Scoreboard" zum Positionieren).
        {
            let rect = self.hud_scoreboard_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Scoreboard) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            rects.push(UiRect { rect, color: COLOR_SCOREBOARD_BG, ..UiRect::FLAT });
            // Bei Verbindung der echte Inhalt, sonst ein Platzhalter-Titel. Im HUD-Editor
            // geht es um die LAGE -- deshalb einfarbig in der Griff-Farbe.
            let title: String = self
                .scoreboard_lines()
                .map(|(t, _)| t.iter().map(|s| s.text.as_str()).collect::<String>())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Scoreboard".to_string());
            let (scale, _, title_h, _) = self.scoreboard_metrics(gpu);
            let tw = gpu.measure_text(&title, scale);
            texts.push(UiText {
                text: title,
                x: (rect.x + (rect.w - tw) / 2.0).round(),
                y: (rect.y + (title_h - gpu.text_height(scale)) / 2.0).round(),
                color: COLOR_HUD,
                scale,
            });
        }

        // Schild-Cooldown: Griff-Rahmen + Beispielzahl. Im Spiel steht sie in Fadenkreuzfarbe
        // (Fadenkreuz-Pass), hier in der Griff-Farbe -- es geht um die Lage.
        if self.hud_element_shown(HudElement::ShieldCooldown) {
            let rect = self.hud_shield_cd_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::ShieldCooldown) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            // Beispielzahl in der allgemeinen Schrift -- es geht um die Lage, nicht um Schrift
            // und Farbe (die setzt im Spiel der Fadenkreuz-Pass).
            texts.push(UiText { text: "5.0".to_string(), x: rect.x, y: rect.y, color: COLOR_HUD, scale: SHIELD_CD_TEXT_SCALE });
        }

        // Pfeil-Anzeige: Griff-Rahmen + Beispiel (64 Pfeile) mit den eingestellten Optionen.
        if self.hud_element_shown(HudElement::Arrows) {
            let rect = self.hud_arrows_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Arrows) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            if let Some(pfeil) = world::item_registry::item_id_by_name("arrow") {
                self.push_arrow_indicator(gpu, &protocol::item::ItemStack::new(pfeil, 64), sprites, texts, true);
            }
        }

        // Ruestung: Griff-Rahmen um alle vier Zellen und die getragene Ruestung, ohne welche ein
        // Beispielsatz (einmal mit wenig Haltbarkeit, damit man die orange Zahl sieht).
        if self.hud_element_shown(HudElement::Armor) {
            let rect = self.hud_armor_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Armor) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let getragen = self.worn_armor();
            let teile = if getragen.iter().any(Option::is_some) {
                getragen.map(|t| t.cloned())
            } else {
                ["diamond_helmet", "diamond_chestplate", "diamond_leggings", "diamond_boots"].map(|name| {
                    let mut stack = protocol::item::ItemStack::new(world::item_registry::item_id_by_name(name)?, 1);
                    if name == "diamond_chestplate" {
                        stack.damage = Some(490);
                    }
                    Some(stack)
                })
            };
            self.begin_text_area(gpu, texts, ShadowArea::Hotbar);
            self.push_armor_hud(gpu, [teile[0].as_ref(), teile[1].as_ref(), teile[2].as_ref(), teile[3].as_ref()], [false; 4], rects, sprites, texts);
            Self::end_text_area(gpu, texts);
        }

        // Keystrokes: Griff-Rahmen und die Tasten selbst, wie im Spiel.
        if self.hud_element_shown(HudElement::Keystrokes) {
            let rect = self.hud_keys_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Keystrokes) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            self.push_keystrokes(gpu, rects, texts);
        }

        // Session-Info: Griff-Rahmen und die Tafel selbst, wie im Spiel.
        if self.hud_element_shown(HudElement::Session) {
            let rect = self.hud_session_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect { rect: frame, color: if dragging == Some(HudElement::Session) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE }, ..UiRect::FLAT });
            self.push_session_info(gpu, rects, texts);
        }

        // Actionbar: Griff-Rahmen + Text-Vorschau (echter Text nur bei Verbindung).
        {
            let rect = self.hud_actionbar_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::ActionBar) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            // Im HUD-Editor geht es um die LAGE, nicht um die Farben -- deshalb einfarbig.
            let text: String = match self.action_bar.as_ref() {
                Some((spans, _)) => spans.iter().map(|s| s.text.as_str()).collect(),
                None => "Actionbar".to_string(),
            };
            let tw = gpu.measure_text(&text, UI_SCALE_NORMAL);
            let tx = align_left(align_anchor(rect.x, rect.w, self.settings.hud_actionbar_align), tw, self.settings.hud_actionbar_align);
            texts.push(UiText { text, x: tx.round(), y: rect.y, color: COLOR_HUD, scale: UI_SCALE_NORMAL });
        }

        // Titel und Untertitel: je ein Griff-Rahmen + Beispieltext in eigener Ausrichtung und
        // Groesse.
        for (element, subtitle) in [(HudElement::Title, false), (HudElement::Subtitle, true)] {
            let rect = self.hud_title_line_rect(gpu, subtitle);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect { rect: frame, color: if dragging == Some(element) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE }, ..UiRect::FLAT });
            let aktiv = self.title_text.is_some() || self.subtitle_text.is_some();
            let spans = if subtitle { &self.subtitle_text } else { &self.title_text };
            let text: String = match spans {
                Some(sp) => sp.iter().map(|t| t.text.as_str()).collect(),
                None if aktiv => String::new(),
                None => if subtitle { "Untertitel" } else { "Titel" }.to_string(),
            };
            let a = if subtitle { self.settings.hud_subtitle_align } else { self.settings.hud_title_align };
            let scale = self.title_scale(subtitle);
            let tw = gpu.measure_text(&text, scale);
            let x = align_left(align_anchor(rect.x, rect.w, a), tw, a).round();
            texts.push(UiText { text, x, y: rect.y, color: COLOR_HUD, scale });
        }

        // Itemname: Griff-Rahmen und der Name des gehaltenen Items bzw. ein Platzhalter.
        {
            let rect = self.hud_held_name_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::HeldName) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let g = self.held_name_scale(gpu);
            let text: String = match self.own_hand_stack(protocol::packets::HAND_MAIN) {
                Some(stack) => self.held_name_spans(stack).iter().map(|s| s.text.as_str()).collect(),
                None => "Itemname".to_string(),
            };
            let text = if text.is_empty() { "Itemname".to_string() } else { text };
            let tw = gpu.measure_text(&text, g);
            texts.push(UiText { text, x: (rect.x + (rect.w - tw) / 2.0).round(), y: rect.y, color: COLOR_HUD, scale: g });
        }

        // Bossbars: Griff-Rahmen und die echten Bars, ohne Verbindung eine Beispiel-Bar.
        {
            let rect = self.hud_bossbar_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::BossBar) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            if self.boss_bars.is_empty() {
                let beispiel = BossBarState {
                    uuid: [0; 16],
                    name: vec![protocol::nbt::TextSpan { text: "Bossbar".to_string(), ..Default::default() }],
                    shown: 0.6,
                    target: 0.6,
                    set_at: Instant::now(),
                    color: 0,
                    style: 0,
                };
                self.build_boss_bars(gpu, std::slice::from_ref(&beispiel), sprites, texts);
            } else {
                self.build_boss_bars(gpu, &self.boss_bars, sprites, texts);
            }
        }

        // Effekte: Griff-Rahmen, die echten Effekte oder Beispiele. Ist die HUD-Anzeige aus,
        // bleibt sie im Editor sichtbar (sonst liesse sie sich nie mehr setzen), dann blasser.
        {
            let rect = self.hud_effects_rect(gpu, true);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Effects) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let alpha = if self.settings.show_effects_hud { 1.0 } else { 0.4 };
            self.build_effect_hud(gpu, sprites, texts, true, alpha);
        }

        // Die Mittellinie, an der gezogene Elemente einrasten (s. update_hud_drag) -- heller,
        // solange das gezogene Element genau darauf sitzt.
        {
            let eingerastet = dragging.is_some_and(|e| {
                let r = self.hud_element_rect(gpu, e);
                (r.x + r.w / 2.0 - sw / 2.0).abs() < 1.0
            });
            let farbe = if eingerastet { [1.0, 0.85, 0.2, 0.9] } else { [1.0, 1.0, 1.0, 0.25] };
            rects.push(UiRect { rect: Rect { x: (sw / 2.0 - 1.0).round(), y: 0.0, w: 2.0, h: sh }, color: farbe, ..UiRect::FLAT });
        }

        // Chat: Griff-Rahmen um den ganzen Nachrichtenblock plus die letzten Zeilen als
        // Vorschau. Ohne Verbindung ist der Chat leer -- dann Platzhalterzeilen, sonst
        // haette man nichts zum Abschaetzen der Groesse.
        {
            let rect = self.hud_chat_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Chat) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let scale = self.chat_scale();
            let line_h = self.chat_line_height(gpu);
            // Im HUD-Editor geht es um die LAGE des Blocks, nicht um den Inhalt -- deshalb hier
            // bewusst einfarbig in der Griff-Farbe, aber genauso gesetzt wie im Spiel.
            let lines: Vec<ChatLine> = if self.chat_messages.is_empty() {
                (1..=self.settings.chat_lines)
                    .map(|i| ChatLine { indent: 0.0, spans: vec![protocol::nbt::TextSpan { text: format!("Chat-Zeile {i}"), ..Default::default() }] })
                    .collect()
            } else {
                self.chat_wrapped_tail(gpu, rect.w - 4.0, scale, self.settings.chat_lines as usize)
            };
            let top = rect.y + rect.h - lines.len() as f32 * line_h;
            for (i, line) in lines.into_iter().enumerate() {
                let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                texts.push(UiText { text, x: rect.x + 2.0 + line.indent, y: top + i as f32 * line_h, color: COLOR_HUD, scale });
            }
        }

        // Hotbar: Griff-Rahmen plus das echte Vanilla-Sheet als Vorschau (samt
        // Auswahlrahmen), damit man beim Verschieben sieht, was man tatsaechlich verschiebt.
        {
            let rect = self.hud_hotbar_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Hotbar) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            if let Some(sheet) = self.ui_atlas.rect("hud/hotbar") {
                sprites.push(UiSprite::quad(rect.x, rect.y, rect.w, rect.h, sheet.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
            }
            if let Some(selection) = self.ui_atlas.rect("hud/hotbar_selection") {
                let scale = self.hotbar_scale(gpu.size());
                let size = inventory::HOTBAR_SELECTION * scale;
                sprites.push(UiSprite::quad(
                    rect.x + (self.inventory.held_slot as f32 * 20.0 - 1.0) * scale,
                    rect.y - scale,
                    size,
                    size * (23.0 / 24.0),
                    selection.uv,
                    [1.0, 1.0, 1.0, 1.0],
                    SpriteAtlas::Ui,
                ));
            }
        }
    }

    // Gestorben-Screen: roter Vollbild-Schleier + Ueberschrift. Respawn/Disconnect kommen
    // aus buttons(). Vanilla-Anmutung (roter Ton), ohne die Todesnachricht zu parsen (die
    // steckt als Text-Component im Combat-Kill-Paket -- ein eigener NBT-Parser, kein
    // MVP-Ziel; das Set-Health-Paket, aus dem wir den Tod erkennen, traegt keinen Text).
    pub(crate) fn build_death_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: COLOR_DEATH_OVERLAY, ..UiRect::FLAT });
        // Ueberschrift, ueber den Buttons zentriert.
        let title = "Du bist gestorben";
        let tw = gpu.measure_text(title, UI_SCALE_TITLE);
        texts.push(UiText {
            text: title.to_string(),
            x: ((sw - tw) / 2.0).round().max(UI_MARGIN),
            y: 120.0,
            color: COLOR_DEATH_TITLE,
            scale: UI_SCALE_TITLE,
        });
    }

    // Connect-Screen-Inhalt: die Serverliste. Das Log steht im Launcher, Adressen gehen ueber
    // Quick Connect (Leiste unten), das Konto liegt in der Leiste oben.
    pub(crate) fn build_connect_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        self.build_server_panel(gpu, rects, texts, sprites);
    }

    // Server-Liste bzw. Editier-Formular rechts auf dem Connect-Screen. Die Klickflaechen
    // selbst kommen aus server_panel_buttons() -- hier nur Panel, Ueberschrift und die
    // Beschriftungen, die gpu.centered_text nicht hinbekommt (zweizeilige Kacheln,
    // linksbuendige Felder mit Caret).
    pub(crate) fn build_server_panel(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        use theme::Slot;
        let g = theme::get;
        let panel = server_panel();
        {
            if self.servers.is_empty() {
                let hint = "Noch keine Server gespeichert. Unten auf \"Neuer Eintrag\".";
                texts.push(UiText { text: hint.to_string(), x: panel.x, y: server_list_top() + 8.0, color: theme::text_3(), scale: UI_SCALE_SMALL });
            }
            let (cx, cy) = self.pointer();
            let (oben, unten) = (server_list_top(), server_list_bottom());
            // Suchfeld an der Stelle der ersten Karte; ohne Treffer darunter ein Hinweis.
            if let Some(suche) = self.server_search.as_ref() {
                let r = server_card_rect(0, self.server_scroll.stand);
                if r.y + r.h > oben && r.y < unten {
                    rects.push(UiRect::shadow(r, 8.0, 2.0, 8.0, 0.35));
                    rects.push(UiRect::framed(r, g(Slot::SurfaceSunken), 8.0, 1.0, g(Slot::Accent)));
                    let feld = Rect { x: r.x + 16.0, y: r.y, w: r.w - 32.0, h: r.h };
                    self.push_text_field(gpu, suche, feld, self.server_search_typing(), rects, texts);
                }
                if self.server_search_hits().is_empty() {
                    let y = server_card_rect(1, self.server_scroll.stand).y + 8.0;
                    texts.push(UiText { text: "Keine Treffer".to_string(), x: panel.x + 16.0, y, color: theme::text_3(), scale: UI_SCALE_SMALL });
                }
            }
            // Was aus der Liste ragt, verdecken die Leisten: sie liegen auf dem Server-Screen als
            // eigene Ebene darueber (s. build_menu_ui).
            let icon = server_icon_px();
            for (i, karte) in self.server_cards() {
                let entry = &self.servers[i];
                let hover = karte.contains(cx, cy) && cy >= oben && cy <= unten;
                // Karte wie in osu!: Flaeche mit Rahmen, beim Draufzeigen heller und mit
                // Akzentstreifen links.
                let (fill, rand) = if hover { (g(Slot::Surface2), g(Slot::Accent)) } else { (g(Slot::Surface1), g(Slot::Border1)) };
                rects.push(UiRect::shadow(karte, 8.0, 2.0, 8.0, 0.35));
                rects.push(UiRect::framed(karte, fill, 8.0, 1.0, rand));
                let inhalt_x = karte.x;
                if hover {
                    rects.push(UiRect::rounded(Rect { x: karte.x + 6.0, y: karte.y + 14.0, w: 4.0, h: karte.h - 28.0 }, g(Slot::Accent), 2.0));
                }
                // Icon des Servers (aus dem Status-Ping), ohne eins das Vanilla-Bild fuer
                // unbekannte Server (s. server_list_icon).
                let feld = Rect { x: inhalt_x + 16.0, y: karte.y + 12.0, w: icon, h: icon };
                let status = self.server_status.get(&entry.address);
                rects.push(UiRect::rounded(feld, g(Slot::SurfaceSunken), 6.0));
                if let Some(slot) = self.server_list_icon(&entry.address) {
                    sprites.push(UiSprite::quad(feld.x, feld.y, feld.w, feld.h, server_icon_uv(slot), [1.0; 4], SpriteAtlas::ServerIcons));
                } else if let Some(e) = status.filter(|e| matches!(e.state, server_status::State::Pending)) {
                    rects.extend(loading_dots(feld, e.since.elapsed().as_secs_f32(), g(Slot::Accent)));
                }
                // Mit Namen: der Name gross, darunter die MOTD in einer Zeile. Ohne: die MOTD auf
                // hoechstens zwei Zeilen umbrochen (wie Vanilla, das sie auf die Breite der Liste
                // setzt), mittig in der Karte. Farbig wie vom Server geschickt. Die Adresse steht
                // nur da, solange es weder Namen noch MOTD gibt (Ping laeuft noch oder der Server
                // ist offline).
                let rechts = karte.x + karte.w - 16.0;
                let zeile1 = (karte.y + 14.0).round();
                let zeile2 = (karte.y + 14.0 + gpu.text_height(UI_SCALE_NORMAL) + 6.0).round();
                let tx = feld.x + feld.w + 16.0;
                let breite = (rechts - tx).max(60.0);
                let mut motd: Vec<protocol::nbt::TextSpan> = Vec::new();
                if let Some(server_status::State::Ok(st)) = status.map(|e| &e.state) {
                    for (n, z) in st.motd.iter().enumerate() {
                        if n > 0 {
                            motd.push(protocol::nbt::TextSpan { text: "\n".to_string(), ..Default::default() });
                        }
                        motd.extend(z.iter().cloned());
                    }
                }
                // "Namen anzeigen" aus: mit MOTD wie ohne Namen, ohne MOTD bleibt der Name.
                let name = if entry.show_name || motd.is_empty() { entry.name.as_str() } else { "" };
                // Ohne Namen ist die MOTD das Einzige in der Karte: umbrochen wie in Vanilla (an
                // derselben Stelle, gemessen in Vanillas Breiten -- Server richten ihre MOTD
                // darauf aus), hoechstens zwei Zeilen, so gross wie sonst der Name, wenn beide
                // so passen, sonst klein. Unter einem Namen bleibt sie klein.
                let mut vanilla: Vec<Vec<protocol::nbt::TextSpan>> =
                    wrap_spans_styled(&motd, MOTD_VANILLA_WIDTH, |t, stil| gpu.vanilla_width(t, stil), 0.0).into_iter().map(|l| l.spans).filter(|z| !z.is_empty()).take(2).collect();
                trim_motd_padding(&mut vanilla);
                vanilla.retain(|z| !z.is_empty());
                let passt_bei = |scale: f32| vanilla.iter().all(|z| span_row_width(gpu, z, scale) <= breite);
                let motd_scale = if name.is_empty() && passt_bei(UI_SCALE_NORMAL) { UI_SCALE_NORMAL } else { UI_SCALE_SMALL };
                let klein = gpu.text_height(motd_scale);
                let zeilen: Vec<Vec<protocol::nbt::TextSpan>> = if name.is_empty() && passt_bei(motd_scale) {
                    vanilla
                } else {
                    wrap_spans(&motd, breite, |t| gpu.measure_text(t, motd_scale), 0.0).into_iter().map(|l| l.spans).filter(|z| !z.is_empty()).collect()
                };
                let mut setzen: Vec<(f32, Vec<protocol::nbt::TextSpan>)> = Vec::new();
                if !name.is_empty() {
                    texts.push(UiText { text: ellipsize(gpu, name, UI_SCALE_NORMAL, breite), x: tx, y: zeile1, color: theme::text(), scale: UI_SCALE_NORMAL });
                    if let Some(erste) = zeilen.first() {
                        let mut z = erste.clone();
                        if zeilen.len() > 1 {
                            z.push(protocol::nbt::TextSpan { text: " ".to_string(), ..Default::default() });
                            z.extend(zeilen[1].iter().cloned());
                        }
                        setzen.push((zeile2, z));
                    }
                } else if zeilen.is_empty() {
                    texts.push(UiText { text: ellipsize(gpu, &entry.address, UI_SCALE_NORMAL, breite), x: tx, y: zeile1, color: theme::text(), scale: UI_SCALE_NORMAL });
                } else {
                    let n = zeilen.len().min(2);
                    let abstand = 6.0;
                    let y0 = karte.y + (karte.h - n as f32 * klein - (n - 1) as f32 * abstand) / 2.0;
                    // Wie Vanilla: was nach der zweiten Zeile kommt, faellt weg.
                    for (k, z) in zeilen.iter().take(n).enumerate() {
                        setzen.push(((y0 + k as f32 * (klein + abstand)).round(), z.clone()));
                    }
                }
                for (y, z) in setzen {
                    let z = ellipsize_spans(gpu, &z, motd_scale, breite);
                    push_styled_row(gpu, &z, tx, y, theme::text_2(), motd_scale, texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
                }
            }
            // Gezogene Karte: weisse Linie in der Luecke, in der sie landen wuerde.
            if self.server_drag_active().is_some() {
                if let Some(platz) = self.server_drop_slot() {
                    let y = (server_card_rect(platz, self.server_scroll.stand).y - SERVER_ROW_GAP / 2.0 - 1.0).round();
                    if y >= oben - SERVER_ROW_GAP && y <= unten {
                        rects.push(UiRect::rounded(Rect { x: panel.x, y, w: panel.w, h: 2.0 }, theme::text(), 1.0));
                    }
                }
            }
            // Bildlaufleiste links neben den Karten, sobald nicht alle passen. Zum Anfassen: unter
            // dem Zeiger und gefasst in der Akzentfarbe.
            if let Some((spur, griff)) = self.server_scrollbar() {
                rects.push(UiRect::rounded(spur, g(Slot::Surface1), SERVER_BAR_W / 2.0));
                let aktiv = self.server_bar_grab.is_some() || self.server_bar_hit().is_some_and(|r| r.contains(cx, cy));
                rects.push(UiRect::rounded(griff, g(if aktiv { Slot::Accent } else { Slot::Border2 }), SERVER_BAR_W / 2.0));
            }
        }
    }

    // Einheitliche Konto-Statuszeile (Text + Farbe) fuer Connect- und Account-Screen.
    pub(crate) fn account_status_line(&self) -> (String, [f32; 4]) {
        match &self.credentials {
            Some(c) => (format!("Angemeldet als {}", c.username), [0.4, 1.0, 0.5, 1.0]),
            None if self.login_busy => ("Anmeldung läuft ...".to_string(), [1.0, 0.85, 0.3, 1.0]),
            None => ("Nicht angemeldet (Offline-Mode)".to_string(), [0.7, 0.7, 0.7, 1.0]),
        }
    }

    // Account-Screen: Konto-Status + Verwaltungs-Buttons (aus buttons()) + Beschriftungen,
    // das Eingabefeld (Token/Client-ID) und die Liste der Launcher-Konten. Aufgeraeumte
    // vertikale Spalte, nichts ueberlappt mehr.
    pub(crate) fn build_account_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        // Ueberschrift oben (rechts neben dem Zurueck-Button).
        texts.push(UiText {
            text: "Konto".to_string(),
            x: BACK_BUTTON.x + BACK_BUTTON.w + 24.0,
            y: (BACK_BUTTON.y + 10.0).round(),
            color: theme::text(),
            scale: UI_SCALE_NORMAL,
        });
        // Statuszeile.
        let (status, color) = self.account_status_line();
        texts.push(UiText { text: status, x: 40.0, y: 88.0, color, scale: UI_SCALE_NORMAL });

        if self.credentials.is_some() {
            // Angemeldet: nur ein Hinweis (Abmelden-Button kommt aus buttons()).
            texts.push(UiText {
                text: "Zum Kontowechsel erst abmelden.".to_string(),
                x: 40.0,
                y: (account_row_rect(0).y + account_row_rect(0).h + 8.0).round(),
                color: [0.6, 0.6, 0.6, 1.0],
                scale: UI_SCALE_SMALL,
            });
            return;
        }

        // Eingabefeld (Token bzw. Refresh-Token), nur wenn aufgeklappt.
        if let Some((kind, value)) = &self.input_field {
            let label = match kind {
                InputKind::Token => "Minecraft-Token einfügen (Strg+V), dann OK/Enter:",
                InputKind::RefreshToken => "Microsoft-Refresh-Token einfügen (Strg+V), dann OK/Enter:",
                // Gehoert zur Configs-Seite (s. build_settings_configs).
                InputKind::ConfigName(_) => return,
            };
            texts.push(UiText {
                text: label.to_string(),
                x: ACCOUNT_INPUT_FIELD.x,
                y: (ACCOUNT_INPUT_FIELD.y - gpu.text_height(UI_SCALE_SMALL) - 4.0).round(),
                color: [0.7, 0.7, 0.7, 1.0],
                scale: UI_SCALE_SMALL,
            });
            rects.push(UiRect { rect: ACCOUNT_INPUT_FIELD, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            let tx = ACCOUNT_INPUT_FIELD.x + 10.0;
            let ty = (ACCOUNT_INPUT_FIELD.y + (ACCOUNT_INPUT_FIELD.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            let shown = token_display(gpu, value, ACCOUNT_INPUT_FIELD.w - 20.0);
            push_typed_marker(texts);
            texts.push(UiText { text: shown.clone(), x: tx, y: ty, color: theme::text(), scale: UI_SCALE_SMALL });
            let caret_x = tx + gpu.measure_raw(&shown, UI_SCALE_SMALL);
            self.push_caret(rects, gpu.caret_rect(caret_x, ty, UI_SCALE_SMALL));
        }
    }
}

// Ein Zahlenfeld der Einstellungen, egal welchen Typs (s. App::step_field).
pub(crate) enum NumMut<'a> {
    U32(&'a mut u32),
    I32(&'a mut i32),
    U8(&'a mut u8),
    Usize(&'a mut usize),
    F32(&'a mut f32),
}

impl NumMut<'_> {
    // Ganzzahlen gerundet; was der Typ nicht fassen kann, wird verworfen (false).
    fn set(self, v: f64) -> bool {
        let n = v.round();
        match self {
            NumMut::U32(f) => num_u32(v).map(|x| *f = x).is_some(),
            NumMut::I32(f) => (n >= i32::MIN as f64 && n <= i32::MAX as f64).then(|| *f = n as i32).is_some(),
            NumMut::U8(f) => (0.0..=u8::MAX as f64).contains(&n).then(|| *f = n as u8).is_some(),
            NumMut::Usize(f) => (n >= 0.0 && n <= usize::MAX as f64).then(|| *f = n as usize).is_some(),
            NumMut::F32(f) => {
                *f = v as f32;
                true
            }
        }
    }
}

fn num_u32(v: f64) -> Option<u32> {
    let n = v.round();
    (0.0..=u32::MAX as f64).contains(&n).then_some(n as u32)
}

impl<'a> From<&'a mut u32> for NumMut<'a> {
    fn from(f: &'a mut u32) -> Self {
        NumMut::U32(f)
    }
}
impl<'a> From<&'a mut i32> for NumMut<'a> {
    fn from(f: &'a mut i32) -> Self {
        NumMut::I32(f)
    }
}
impl<'a> From<&'a mut u8> for NumMut<'a> {
    fn from(f: &'a mut u8) -> Self {
        NumMut::U8(f)
    }
}
impl<'a> From<&'a mut usize> for NumMut<'a> {
    fn from(f: &'a mut usize) -> Self {
        NumMut::Usize(f)
    }
}
impl<'a> From<&'a mut f32> for NumMut<'a> {
    fn from(f: &'a mut f32) -> Self {
        NumMut::F32(f)
    }
}

// Eintrag `r` einer offenen Liste, gekuerzt auf ihren Innenraum (ohne Abstand zum Rahmen).
fn dropdown_clip(kasten: Rect, r: Rect) -> Option<Rect> {
    let oben = r.y.max(kasten.y + DROPDOWN_PAD);
    let unten = (r.y + r.h).min(kasten.y + kasten.h - DROPDOWN_PAD);
    (unten - oben >= 1.0).then_some(Rect { y: oben, h: unten - oben, ..r })
}
