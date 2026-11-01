// Serverdialog, Skin-Dateien, Schnellverbinden und aktives Textfeld.

use super::*;

impl App {
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
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
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
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
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
        overlay.groups.push(UiOverlayGroup { rects, texts: Vec::new(), sprites, pivot: (0.0, 0.0), angle: 0.0, clip: None });
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
        (!rects.is_empty()).then_some(UiOverlayGroup { rects, texts: Vec::new(), sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None })
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
        overlay.groups.push(UiOverlayGroup { rects, texts: Vec::new(), sprites, pivot: (0.0, 0.0), angle: 0.0, clip: None });
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
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
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
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
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
}
