// Design-Seite, Themes, Theme-Dialoge und Trennen-Popup.

use super::*;

impl App {
    // Aussehen: Hintergrundbild, darunter das Theme. Die Knoepfe kommen aus buttons().
    pub(crate) fn build_settings_design_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        self.build_theme_section(gpu, rects, texts);
        texts.push(UiText { text: "Spielbild abdunkeln".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(MENU_BACKDROP_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        let abdunkeln = format!("{} %", self.settings.menu_backdrop_opacity);
        self.draw_editable_value(gpu, stepper_value_rect(MENU_BACKDROP_ROW), EditableValue::MenuBackdropOpacity, abdunkeln, rects, texts);
        texts.push(UiText { text: "Trägheit beim Scrollen".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(SCROLL_INERTIA_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        let traegheit = format!("{} %", self.settings.scroll_inertia);
        self.draw_editable_value(gpu, stepper_value_rect(SCROLL_INERTIA_ROW), EditableValue::ScrollInertia, traegheit, rects, texts);
        texts.push(UiText { text: "Scrollweite".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(SCROLL_DISTANCE_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        let weite = format!("{} %", self.settings.scroll_distance);
        self.draw_editable_value(gpu, stepper_value_rect(SCROLL_DISTANCE_ROW), EditableValue::ScrollDistance, weite, rects, texts);
        texts.push(UiText { text: "Hintergrundbild".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(WALLPAPER_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        let panorama_an = wallpaper::WallpaperMode::from_index(self.settings.wallpaper) == wallpaper::WallpaperMode::Panorama;
        let farbe = if panorama_an { theme::text() } else { theme::text_disabled() };
        texts.push(UiText { text: "Panorama-Drehung".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(PANORAMA_SPEED_ROW), color: farbe, scale: UI_SCALE_SMALL });
        let value_rect = stepper_value_rect(PANORAMA_SPEED_ROW);
        rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(value_rect, format!("{} %", self.settings.panorama_speed), farbe, UI_SCALE_SMALL));
        // Rechts daneben, was gerade gilt. Beim eigenen Bild steht dort der Knopf (s.
        // buttons()), der Hinweis rechts davon.
        let hinweis_x = WALLPAPER_PICK_X + 240.0;
        match wallpaper::WallpaperMode::from_index(self.settings.wallpaper) {
            wallpaper::WallpaperMode::None => {}
            wallpaper::WallpaperMode::File => {
                let da = shared_config_dir().map(|d| wallpaper::stored_path(&d)).is_some_and(|p| p.is_file());
                let text = if da { "Bild gespeichert" } else { "Noch kein Bild gewählt" };
                texts.push(UiText { text: text.to_string(), x: hinweis_x, y: zeile_y(WALLPAPER_ROW), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
            }
            wallpaper::WallpaperMode::Desktop => {
                let text = match self.wallpaper_loader.current_path() {
                    Some(p) => p.display().to_string(),
                    None => "Kein Desktop-Hintergrund gefunden".to_string(),
                };
                texts.push(UiText { text, x: WALLPAPER_PICK_X, y: zeile_y(WALLPAPER_ROW), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
            }
            wallpaper::WallpaperMode::Coral => {
                if !hero_scene::path().is_some_and(|p| p.is_file()) {
                    let text = "Wird beim nächsten Start gerendert";
                    texts.push(UiText { text: text.to_string(), x: WALLPAPER_PICK_X, y: zeile_y(WALLPAPER_ROW), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
                }
            }
            // Der Skin steht im Knopf (s. buttons()); ohne Skin mit Panorama nur der Hinweis.
            wallpaper::WallpaperMode::Panorama => {
                if crate::panorama::chosen_skin(&self.settings.panorama_skin).is_none() {
                    let text = "Kein Skin mit Panorama";
                    texts.push(UiText { text: text.to_string(), x: WALLPAPER_PICK_X, y: zeile_y(WALLPAPER_ROW), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
                }
            }
        }
    }

    // Theme-Abschnitt der Seite Aussehen: Kacheln links, Trennlinie, Farben rechts.
    pub(crate) fn build_theme_section(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        use theme::Slot;
        let g = theme::get;
        let set = &self.settings;
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let sichtbar = |y: f32| y > -1000.0;
        // Hinter einem offenen Fenster leuchtet nichts auf.
        let (qx, qy) = if self.theme_name_dialog.is_some() || self.theme_delete.is_some() { (-1.0, -1.0) } else { self.pointer() };
        let presets = theme::PRESETS.len();
        let anzahl = presets + set.custom_themes.len() + 1;
        for i in 0..anzahl {
            let r = theme_tile_rect(i);
            if !sichtbar(r.y) {
                continue;
            }
            let k = self.hover_level(r, r.contains(qx, qy)).min(1.0);
            if i == anzahl - 1 {
                // "+": ein neues eigenes Theme.
                rects.push(UiRect::framed(r, mix_color(g(Slot::Surface1), g(Slot::Surface2), k), 8.0, 1.0, mix_color(g(Slot::Border2), g(Slot::BorderStrong), k)));
                let farbe = mix_color(theme::text_2(), theme::text(), k);
                let (mx, my) = ((r.x + r.w / 2.0).round(), (r.y + r.h / 2.0).round());
                push_bar(rects, (mx - 10.0, my), (mx + 10.0, my), 3.0, farbe);
                push_bar(rects, (mx, my - 10.0), (mx, my + 10.0), 3.0, farbe);
                continue;
            }
            let (name, farben, wahl) = match i.checked_sub(presets) {
                None => (theme::PRESETS[i].name.to_string(), theme::PRESETS[i].colors, ThemeChoice::Preset(i)),
                Some(j) => (set.custom_themes[j].name.clone(), set.custom_themes[j].colors, ThemeChoice::Custom(j)),
            };
            // Die Kachel zeigt das Theme selbst: seine Flaeche, sein Akzent, seine Textfarbe.
            let c = |slot: Slot| theme::rgba(farben[slot.index()]);
            let gewaehlt = set.theme_choice == wahl;
            let rand = if gewaehlt { g(Slot::Accent) } else { mix_color(g(Slot::Border1), g(Slot::BorderStrong), k) };
            rects.push(UiRect::framed(r, c(Slot::BgPanel), 8.0, if gewaehlt { 2.0 } else { 1.0 }, rand));
            rects.push(UiRect::rounded(Rect { x: r.x + 10.0, y: r.y + 9.0, w: 6.0, h: r.h - 18.0 }, c(Slot::Accent), 3.0));
            for (n, slot) in [Slot::Surface2, Slot::AccentSurface, Slot::Text2].into_iter().enumerate() {
                let d = Rect { x: r.x + r.w - 14.0 - (3 - n) as f32 * 16.0, y: r.y + r.h / 2.0 - 6.0, w: 12.0, h: 12.0 };
                rects.push(UiRect::framed(d, c(slot), 6.0, 1.0, c(Slot::Border2)));
            }
            let ty = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            // Eigene Namen werden nicht uebersetzt (s. push_typed_marker), Presets schon.
            let name = if matches!(wahl, ThemeChoice::Custom(_)) {
                push_typed_marker(texts);
                name
            } else {
                Self::fit_text(gpu, &name, r.w - 26.0 - 64.0, UI_SCALE_SMALL)
            };
            texts.push(UiText { text: name, x: r.x + 26.0, y: ty, color: c(Slot::Text1), scale: UI_SCALE_SMALL });
        }
        // Trennlinie ueber alle sichtbaren Zeilen des Abschnitts.
        let ys: Vec<f32> = (DESIGN_THEME_ROW..design_row_count()).map(settings_row_y).filter(|&y| sichtbar(y)).collect();
        if let (Some(&oben), Some(&unten)) = (ys.first(), ys.last()) {
            rects.push(UiRect { rect: Rect { x: THEME_DIVIDER_X, y: oben - 3.0, w: 1.0, h: unten + SETTINGS_CTRL_H + 3.0 - (oben - 3.0) }, color: g(Slot::Border1), ..UiRect::FLAT });
        }
        // Rechts: welches Theme, dann seine Farben -- bei einem Preset nur zum Ansehen.
        let fest = matches!(set.theme_choice, ThemeChoice::Preset(_));
        if sichtbar(settings_row_y(DESIGN_THEME_ROW)) {
            let name = match set.theme_choice {
                ThemeChoice::Preset(i) => theme::PRESETS.get(i).map_or("", |p| p.name).to_string(),
                ThemeChoice::Custom(i) => set.custom_themes.get(i).map_or_else(String::new, |t| t.name.clone()),
            };
            let y = zeile_y(DESIGN_THEME_ROW);
            let breite = if fest {
                gpu.measure_text(&name, UI_SCALE_SMALL)
            } else {
                push_typed_marker(texts);
                gpu.measure_raw(&name, UI_SCALE_SMALL)
            };
            texts.push(UiText { text: name, x: THEME_COLORS_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            let hinweis = if fest { "Preset: Farben fest. Eigene Farben mit +" } else { "Farbfeld anklicken zum Ändern, Rechtsklick auf die Kachel löscht" };
            texts.push(UiText { text: hinweis.to_string(), x: THEME_COLORS_X + breite + 16.0, y, color: theme::text_2(), scale: UI_SCALE_SMALL });
        }
        for (i, slot) in theme::EDITABLE.into_iter().enumerate() {
            let (row, x) = theme_slot_cell(i);
            if !sichtbar(settings_row_y(row)) {
                continue;
            }
            texts.push(UiText { text: slot.label().to_string(), x, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
            // Bei einem Preset gibt es keine Farbfeld-Knoepfe (s. screen_buttons): die Farbe
            // steht hier ohne Hover und ohne Klick.
            if fest {
                let feld = Rect { x: x + THEME_COLOR_SWATCH_DX, y: settings_row_y(row), w: SWATCH_W, h: SETTINGS_CTRL_H };
                rects.push(UiRect::framed(feld, theme::get(slot), 6.0, 2.0, [0.0, 0.0, 0.0, 1.0]));
            }
        }
    }

    // Namensfenster fuer ein neues Theme (Geometrie wie Quick Connect).
    pub(crate) fn build_theme_name_dialog(&self, overlay: &mut UiOverlay) {
        let Some((gpu, d)) = self.gpu.as_ref().zip(self.theme_name_dialog.as_ref()) else { return };
        let alias = d.purpose == NameDialogPurpose::EgoAlias;
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, feld, ok, abbrechen) = Self::quick_connect_layout();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
            UiRect::framed(feld, g(Slot::SurfaceSunken), 6.0, 1.0, g(Slot::Accent)),
        ];
        let mut texts = vec![
            UiText { text: if alias { "Eigener Name" } else { "Neues Theme" }.to_string(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL },
            UiText { text: "NAME".to_string(), x: feld.x, y: feld.y - gpu.text_height(TOPBAR_SUB_SCALE) - 8.0, color: theme::text_3(), scale: TOPBAR_SUB_SCALE },
        ];
        self.push_text_field(gpu, &d.field, feld, true, &mut rects, &mut texts);
        // Ein leerer Alias ist erlaubt: dann bleibt der echte Name.
        let leer = !alias && d.field.text().trim().is_empty();
        for (r, label, primaer) in [(abbrechen, "Abbrechen", false), (ok, if alias { "Übernehmen" } else { "Erstellen" }, true)] {
            let b = Button {
                slider: None,
                action: ButtonAction::NewCustomTheme,
                rect: r,
                label: String::new(),
                enabled: !primaer || !leer,
                color: if primaer { g(Slot::Accent) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, b.enabled && r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    pub(crate) fn theme_name_index(&self, x: f32) -> usize {
        let (Some(gpu), Some(d)) = (self.gpu.as_ref(), self.theme_name_dialog.as_ref()) else { return 0 };
        let (_, feld, _, _) = Self::quick_connect_layout();
        text_field::index_at(d.field.text(), x - feld.x - 12.0, |t| gpu.measure_raw(t, UI_SCALE_NORMAL))
    }

    // Linke Taste bei offenem Namensfenster. Immer verbraucht.
    pub(crate) fn theme_name_press(&mut self) {
        let (p, feld, ok, abbrechen) = Self::quick_connect_layout();
        let (cx, cy) = self.cursor_pos;
        if feld.contains(cx, cy) {
            let i = self.theme_name_index(cx);
            let shift = self.modifiers.shift_key();
            if let Some(d) = self.theme_name_dialog.as_mut() {
                if field_press(&mut d.field, &mut d.last_press, &mut d.dragging, i, shift, (cx, cy)) {
                    self.selection_sound_hint = Some(ui_sound::UiSound::Step);
                }
            }
        } else if ok.contains(cx, cy) {
            self.create_custom_theme();
        } else if abbrechen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.theme_name_dialog = None;
        }
    }

    // Neues eigenes Theme mit dem Namen aus dem Fenster, anfangs mit den Farben des gerade
    // gewaehlten -- und gleich gewaehlt, damit man rechts die Farben setzen kann.
    pub(crate) fn create_custom_theme(&mut self) {
        if self.theme_name_dialog.as_ref().is_some_and(|d| d.purpose == NameDialogPurpose::EgoAlias) {
            let alias = self.theme_name_dialog.take().map(|d| d.field.text().trim().to_string()).unwrap_or_default();
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.settings.ego_alias = alias;
            self.persist_settings();
            return;
        }
        let name = self.theme_name_dialog.as_ref().map(|d| d.field.text().trim().to_string()).unwrap_or_default();
        if name.is_empty() {
            self.play_ui_sound(ui_sound::UiSound::Denied);
            return;
        }
        self.play_ui_sound(ui_sound::UiSound::Click);
        self.theme_name_dialog = None;
        let colors = self.settings.theme;
        self.settings.custom_themes.push(CustomTheme { name, colors });
        let i = self.settings.custom_themes.len() - 1;
        self.settings.select_theme(ThemeChoice::Custom(i));
        self.persist_settings();
    }

    // Eigenes Theme unter dem Zeiger (Seite Aussehen), Index in custom_themes.
    pub(crate) fn custom_theme_at_pointer(&self) -> Option<usize> {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsDesign {
            return None;
        }
        let (qx, qy) = self.pointer();
        (0..self.settings.custom_themes.len()).find(|&j| theme_tile_rect(theme::PRESETS.len() + j).contains(qx, qy))
    }

    // Disconnect-Fenster: Ueberschrift mittig, darunter die Meldung (umbrochen, jede Zeile
    // mittig, in ihren Farben), ganz unten mittig "Schliessen". Die Hoehe waechst mit der
    // Meldung. Alles um DISCONNECT_POPUP_SCALE vergroessert.
    // (Fenster, Schliessen, Zeilen)
    // Die Schrift dabei auf ganze Faktoren gerundet (Nutzer 2026-11-11): ein Schriftpixel wird
    // 3x3 statt 2,83x2,83 Bildschirmpixel -- dazwischen sieht die Minecraft-Schrift zerhackt aus.
    pub(crate) fn disconnect_text_scales() -> (f32, f32) {
        let k = DISCONNECT_POPUP_SCALE;
        ((UI_SCALE_SMALL * k).round(), (UI_SCALE_NORMAL * k).round())
    }

    pub(crate) fn disconnect_popup_layout(&self) -> Option<(Rect, Rect, Vec<Vec<protocol::nbt::TextSpan>>)> {
        let (gpu, meldung) = self.gpu.as_ref().zip(self.disconnect_popup.as_ref())?;
        let k = DISCONNECT_POPUP_SCALE;
        let (sw, sh) = ui_screen();
        let w = (560.0 * k).min(sw - 32.0);
        let (klein, gross) = Self::disconnect_text_scales();
        let zeile_h = gpu.text_height(klein) + 6.0 * k;
        let titel_h = gpu.text_height(gross);
        let (rand, abstand, knopf_h) = (24.0 * k, 20.0 * k, 40.0 * k);
        // Hoechstens so viele Zeilen, wie aufs Fenster passen; der Rest steht im Log.
        let max_zeilen = (((sh - 32.0 - rand - titel_h - abstand - rand - knopf_h - rand) / zeile_h).floor() as usize).max(1);
        // Die Meldung traegt die Farben des Servers als Paragraphen-Codes (s. net::disconnect_message).
        let spans = protocol::nbt::legacy_to_spans(meldung, None);
        let mut zeilen: Vec<Vec<protocol::nbt::TextSpan>> = wrap_spans(&spans, w - 2.0 * rand, |t| gpu.measure_raw(t, klein), 0.0).into_iter().map(|l| l.spans).collect();
        if zeilen.len() > max_zeilen {
            zeilen.truncate(max_zeilen);
            if let Some(letzte) = zeilen.last_mut() {
                letzte.push(protocol::nbt::TextSpan { text: " …".to_string(), ..Default::default() });
            }
        }
        let h = rand + titel_h + abstand + zeilen.len() as f32 * zeile_h + rand + knopf_h + rand;
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h: h.round() };
        let knopf_w = 140.0 * k;
        let schliessen = Rect { x: (p.x + (p.w - knopf_w) / 2.0).round(), y: (p.y + p.h - rand - knopf_h).round(), w: knopf_w.round(), h: knopf_h.round() };
        Some((p, schliessen, zeilen))
    }

    pub(crate) fn build_disconnect_popup(&self, overlay: &mut UiOverlay) {
        if self.mode != AppMode::Menu {
            return;
        }
        let (Some(gpu), Some((p, schliessen, zeilen))) = (self.gpu.as_ref(), self.disconnect_popup_layout()) else { return };
        use theme::Slot;
        let g = theme::get;
        let k = DISCONNECT_POPUP_SCALE;
        let (sw, sh) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0 * k, 12.0 * k, 32.0 * k, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0 * k, 1.0, g(Slot::Border1)),
        ];
        let (klein, gross) = Self::disconnect_text_scales();
        let titel_h = gpu.text_height(gross);
        let titel = Rect { x: p.x, y: p.y + 24.0 * k, w: p.w, h: titel_h };
        let mut texts = vec![gpu.centered_text(titel, "Disconnected".to_string(), theme::text(), gross)];
        // Die Meldung kommt vom Server: nicht uebersetzen, keine Zahlen "lernen".
        push_typed_marker(&mut texts);
        let zeile_h = gpu.text_height(klein) + 6.0 * k;
        let mut y = titel.y + titel_h + 20.0 * k;
        for z in zeilen {
            let breite = span_row_width(gpu, &z, klein);
            let x = (p.x + (p.w - breite) / 2.0).round();
            push_styled_row(gpu, &z, x, y.round(), theme::text_2(), klein, &mut texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
            y += zeile_h;
        }
        let b = Button {
            slider: None,
            action: ButtonAction::Nothing,
            rect: schliessen,
            label: String::new(),
            enabled: true,
            color: g(Slot::Accent),
            scale: klein,
        };
        let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, schliessen.contains(cx, cy)));
        rects.push(flaeche);
        texts.push(gpu.centered_text(schliessen, "Schließen".to_string(), farbe, klein));
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    // Linke Taste bei offenem Disconnect-Fenster: der Knopf oder ein Klick daneben schliesst es.
    pub(crate) fn disconnect_popup_press(&mut self) {
        let Some((p, schliessen, _)) = self.disconnect_popup_layout() else { return };
        let (cx, cy) = self.cursor_pos;
        if schliessen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.disconnect_popup = None;
        }
    }

    // Rueckfrage vor dem Loeschen: (Fenster, Loeschen, Abbrechen).
    pub(crate) fn theme_delete_layout() -> (Rect, Rect, Rect) {
        let (sw, sh) = ui_screen();
        let (w, h) = (440.0, 176.0);
        let p = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let loeschen = Rect { x: p.x + p.w - 24.0 - 140.0, y: p.y + p.h - 24.0 - 40.0, w: 140.0, h: 40.0 };
        let abbrechen = Rect { x: loeschen.x - 12.0 - 140.0, ..loeschen };
        (p, loeschen, abbrechen)
    }

    pub(crate) fn build_theme_delete(&self, overlay: &mut UiOverlay) {
        let Some((gpu, i)) = self.gpu.as_ref().zip(self.theme_delete).filter(|_| self.mode == AppMode::Menu) else { return };
        let Some(t) = self.settings.custom_themes.get(i) else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (p, loeschen, abbrechen) = Self::theme_delete_layout();
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: dialog_dim(), ..UiRect::FLAT },
            UiRect::shadow(p, 8.0, 12.0, 32.0, 0.6),
            UiRect::framed(p, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let mut texts = vec![UiText { text: "Löschen?".to_string(), x: p.x + 24.0, y: p.y + 20.0, color: theme::text(), scale: UI_SCALE_NORMAL }];
        push_typed_marker(&mut texts);
        texts.push(UiText { text: t.name.clone(), x: p.x + 24.0, y: p.y + 24.0 + gpu.text_height(UI_SCALE_NORMAL) + 10.0, color: theme::text_2(), scale: UI_SCALE_SMALL });
        for (r, label, gefahr) in [(abbrechen, "Abbrechen", false), (loeschen, "Löschen", true)] {
            let b = Button {
                slider: None,
                action: ButtonAction::Nothing,
                rect: r,
                label: String::new(),
                enabled: true,
                color: if gefahr { g(Slot::DangerSurface) } else { g(Slot::Surface2) },
                scale: UI_SCALE_SMALL,
            };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(b.rect, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, label.to_string(), farbe, UI_SCALE_SMALL));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    // Linke Taste bei offener Rueckfrage. Immer verbraucht.
    pub(crate) fn theme_delete_press(&mut self) {
        let (p, loeschen, abbrechen) = Self::theme_delete_layout();
        let (cx, cy) = self.cursor_pos;
        if loeschen.contains(cx, cy) {
            if let Some(i) = self.theme_delete.take() {
                self.play_ui_sound(ui_sound::UiSound::Close);
                self.delete_custom_theme(i);
            }
        } else if abbrechen.contains(cx, cy) || !p.contains(cx, cy) {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.theme_delete = None;
        }
    }

    // Eigenes Theme `i` entfernen; war es gewaehlt, gilt danach der Standard.
    pub(crate) fn delete_custom_theme(&mut self, i: usize) {
        let set = &mut self.settings;
        if i >= set.custom_themes.len() {
            return;
        }
        set.custom_themes.remove(i);
        match set.theme_choice {
            ThemeChoice::Custom(j) if j == i => set.select_theme(ThemeChoice::Preset(0)),
            ThemeChoice::Custom(j) if j > i => set.theme_choice = ThemeChoice::Custom(j - 1),
            _ => {}
        }
        self.persist_settings();
    }
}
