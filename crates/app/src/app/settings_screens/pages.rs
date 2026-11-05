// Empfindlichkeitsrechner, Glint und Krit, Konfigurationen, Suche, Licht, Partikel, Shader,
// Inventar und Audio.

use super::*;

impl App {
    // Sens-Rechner: Eingaben (anklickbar, s. EditableValue::CalcMinecraftSens/CalcDpi) und
    // die Ergebnisse als reine Anzeige. "Uebernehmen" kommt aus buttons().
    pub(crate) fn build_sens_calc(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let set = &self.settings;
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let label = |row: usize, text: &str, texts: &mut Vec<UiText>| {
            texts.push(UiText { text: text.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
        };
        let anzeige = |row: usize, text: String, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>| {
            let rect = stepper_value_rect(row);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, text, theme::text(), UI_SCALE_SMALL));
        };
        let umgerechnet = minecraft_sens_to_client(set.sens_calc_mc_percent);
        let geklemmt = umgerechnet.clamp(SENSITIVITY_MIN, SENSITIVITY_MAX);

        label(SENS_CALC_MC_ROW, "Minecraft-Sens", texts);
        self.draw_editable_value(
            gpu,
            stepper_value_rect(SENS_CALC_MC_ROW),
            EditableValue::CalcMinecraftSens,
            format!("{} %", trimme_nullen(format!("{:.2}", set.sens_calc_mc_percent))),
            rects,
            texts,
        );
        label(SENS_CALC_RESULT_ROW, "= Coral-Client-Sens", texts);
        anzeige(SENS_CALC_RESULT_ROW, format_sensitivity(umgerechnet), rects, texts);
        if (geklemmt - umgerechnet).abs() > f32::EPSILON {
            texts.push(UiText {
                text: format!("außerhalb des Bereichs -- wird auf {} begrenzt", format_sensitivity(geklemmt)),
                x: SETTINGS_LABEL_X,
                y: zeile_y(SENS_CALC_RESULT_ROW + 1),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }

        label(SENS_CALC_DPI_ROW, "DPI der Maus", texts);
        self.draw_editable_value(gpu, stepper_value_rect(SENS_CALC_DPI_ROW), EditableValue::CalcDpi, set.sens_calc_dpi.to_string(), rects, texts);
        let cm = |sens: f32| format!("{:.1} cm", cm_per_360(sens, set.sens_calc_dpi));
        label(SENS_CALC_CM_MC_ROW, "cm/360 mit der Minecraft-Sens", texts);
        anzeige(SENS_CALC_CM_MC_ROW, cm(umgerechnet), rects, texts);
        label(SENS_CALC_CM_OWN_ROW, &format!("cm/360 mit Coral-Client-Sens {}", format_sensitivity(set.mouse_sensitivity)), texts);
        anzeige(SENS_CALC_CM_OWN_ROW, cm(set.mouse_sensitivity), rects, texts);
        texts.push(UiText {
            text: "Werte anklicken zum Eintippen. Gilt für die Maus ohne Beschleunigung (Rohdaten), wie in Minecraft.".to_string(),
            x: SETTINGS_LABEL_X,
            y: zeile_y(SENS_CALC_CM_OWN_ROW + 2),
            color: theme::text_disabled(),
            scale: UI_SCALE_SMALL,
        });
    }

    // Video-Tab: Glint-Regler links unter den Partikeln, Crit-Farbe rechts unter der
    // Schadens-Faerbung.
    pub(crate) fn build_video_glint_and_crit(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        for (row, label, feld, wert) in [
            (GLINT_SPEED_ROW, "Glint-Geschwindigkeit", EditableValue::GlintSpeed, self.settings.glint_speed),
            (GLINT_STRENGTH_ROW, "Glint-Stärke", EditableValue::GlintStrength, self.settings.glint_strength),
            (PLAYER_LIST_SCALE_ROW, "Größe", EditableValue::PlayerListScale, self.settings.player_list_scale),
        ] {
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
            self.draw_editable_value(gpu, stepper_value_rect(row), feld, format!("{wert} %"), rects, texts);
        }

        texts.push(UiText { text: "Eigene Crit-Farbe".to_string(), x: SETTINGS_LABEL_X, y: zeile_y(CRIT_COLOR_ROW), color: theme::text(), scale: UI_SCALE_SMALL });

        let set = &self.settings;
        // Beim Modus Aus gibt es nichts einzustellen: die Zeile darunter bleibt leer.
        let detail = match set.fire_mode {
            FIRE_MODE_CROSSHAIR => "Farbe",
            FIRE_MODE_OFF => "",
            _ => "Höhe",
        };
        for (row, label) in [
            (HURT_CAM_ROW, "Hurt-Cam"),
            (HURT_CAM_STRENGTH_ROW, "Stärke"),
            (SHOULDER_ROW, "Schulterkamera"),
            (SHOULDER_OFFSET_ROW, "Seitlicher Versatz"),
            (SHOULDER_CROSSHAIR_ROW, "Zielendes Fadenkreuz"),
            (SHOULDER_FADE_ROW, "Spieler durchsichtig nah"),
            (SHOULDER_DECOUPLED_ROW, "Kamera frei vom Körper"),
            (FIRE_MODE_ROW, "Modus"),
            (FIRE_DETAIL_ROW, detail),
            (DEATH_ANIM_ROW, "Todesanimation"),
            (ANIM_ROW, "Animationen"),
            (VIEW_MODEL_ROW, "Standardmäßig an"),
        ] {
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
        }
        let mut werte = vec![(HURT_CAM_STRENGTH_ROW, set.hurt_cam_strength)];
        if set.fire_mode == FIRE_MODE_VANILLA {
            werte.push((FIRE_DETAIL_ROW, set.fire_height));
        }
        for (row, wert) in werte {
            let value_rect = stepper_value_rect(row);
            rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(value_rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
        }
        // Seitlicher Versatz in Bloecken, positiv = rechte Schulter.
        let value_rect = stepper_value_rect(SHOULDER_OFFSET_ROW);
        rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(value_rect, format!("{:+.2} Blöcke", set.shoulder_offset_cm as f32 / 100.0), theme::text(), UI_SCALE_SMALL));
    }

    pub(crate) fn build_settings_configs(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: format!("Aktiv: {}", active_config()), x: SETTINGS_LABEL_X, y: zeile(0), color: theme::text(), scale: UI_SCALE_SMALL });
        if let Some((InputKind::ConfigName(edit), wert)) = &self.input_field {
            let label = match edit {
                ConfigEdit::New => "Name der neuen Config",
                ConfigEdit::Duplicate => "Name der Kopie",
                ConfigEdit::Rename => "Neuer Name",
            };
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile(1), color: theme::text(), scale: UI_SCALE_SMALL });
            let feld = config_name_field_rect();
            rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            let tx = feld.x + 10.0;
            push_typed_marker(texts);
            texts.push(UiText { text: wert.clone(), x: tx, y: zeile(1), color: theme::text(), scale: UI_SCALE_SMALL });
            self.push_caret(rects, gpu.caret_rect(tx + gpu.measure_raw(wert, UI_SCALE_SMALL), zeile(1), UI_SCALE_SMALL));
        } else {
            texts.push(UiText {
                text: "Klick auf eine Config wechselt sofort -- auch mitten im Spiel.".to_string(),
                x: SETTINGS_LABEL_X,
                y: zeile(1),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }
        texts.push(UiText { text: "Configs".to_string(), x: SETTINGS_LABEL_X, y: zeile(CONFIG_LIST_FIRST_ROW), color: theme::text(), scale: UI_SCALE_SMALL });
        let anzahl = assets::list_configs().len();
        if anzahl > MAX_VISIBLE_CONFIGS {
            texts.push(UiText {
                text: format!("+ {} weitere (im Launcher)", anzahl - MAX_VISIBLE_CONFIGS),
                x: SETTINGS_CTRL_X,
                y: zeile(CONFIG_LIST_FIRST_ROW + MAX_VISIBLE_CONFIGS),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }
    }

    // Suchseite: oben das Feld (Tippen geht immer hierhin), nach Enter je Treffer eine Zeile
    // mit der Beschriftung links und dem Ort rechts. Die Zeilen sind Knoepfe (s.
    // search_hit_buttons), ein Klick oeffnet die Stelle.
    pub(crate) fn build_settings_search(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Suche".to_string(), x: SETTINGS_LABEL_X, y: zeile(0), color: theme::text(), scale: UI_SCALE_SMALL });
        let feld = Self::search_field_rect();
        rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let tx = feld.x + 10.0;
        let leer = self.settings_search.is_empty();
        let (shown, farbe) = if leer { ("Einstellung oder Modul, Enter sucht".to_string(), theme::text_disabled()) } else { (self.settings_search.clone(), theme::text()) };
        push_typed_marker(texts);
        texts.push(UiText { text: shown, x: if leer { tx + CARET_WIDTH_PX + 3.0 } else { tx }, y: zeile(0), color: farbe, scale: UI_SCALE_SMALL });
        let caret_x = tx + if leer { 0.0 } else { gpu.measure_raw(&self.settings_search, UI_SCALE_SMALL) };
        self.push_caret(rects, gpu.caret_rect(caret_x, zeile(0), UI_SCALE_SMALL));
        if self.search_done.is_some() && self.search_hits.is_empty() {
            texts.push(UiText { text: "Nichts gefunden.".to_string(), x: SETTINGS_LABEL_X, y: zeile(1), color: theme::text_2(), scale: UI_SCALE_SMALL });
        }
        for (i, treffer) in self.search_hits.iter().enumerate() {
            let y = zeile(i + 1);
            texts.push(UiText { text: treffer.label.clone(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            texts.push(UiText { text: treffer.path.clone(), x: SETTINGS_CTRL_X, y, color: theme::text_2(), scale: UI_SCALE_SMALL });
        }
    }

    pub(crate) fn search_field_rect() -> Rect {
        Rect { x: SETTINGS_CTRL_X, y: settings_row_y(0), w: SLIDER_SPAN_W, h: SETTINGS_CTRL_H }
    }

    // Je Treffer ein Knopf ueber die ganze Zeile (ohne Text, den zeichnet build_settings_search).
    pub(crate) fn search_hit_buttons(&self) -> Vec<Button> {
        (0..self.search_hits.len())
            .map(|i| Button {
                action: ButtonAction::SearchHit(i as u16),
                rect: Rect { x: SETTINGS_LABEL_X - 12.0, y: settings_row_y(i + 1) - 2.0, w: SETTINGS_CTRL_X - SETTINGS_LABEL_X + SLIDER_SPAN_W + 24.0, h: SETTINGS_CTRL_H + 4.0 },
                label: String::new(),
                enabled: true,
                color: theme::get(theme::Slot::Surface2),
                scale: UI_SCALE_SMALL,
                slider: None,
            })
            .collect()
    }

    // Enter im Suchfeld: alle Einstellungszeilen und Module durchsuchen. Jedes Wort der Eingabe
    // muss in der Beschriftung oder ihrem Ort vorkommen (auch in der eingestellten Sprache).
    pub(crate) fn run_settings_search(&mut self) {
        let worte: Vec<String> = self.settings_search.to_lowercase().split_whitespace().map(str::to_string).collect();
        self.search_hits = if worte.is_empty() {
            Vec::new()
        } else {
            self.search_index()
                .into_iter()
                .filter(|t| {
                    let heu = format!("{} {} {}", t.label, i18n::tr(&t.label), t.path).to_lowercase();
                    worte.iter().all(|w| heu.contains(w.as_str()))
                })
                .collect()
        };
        SEARCH_HIT_COUNT.store(self.search_hits.len(), std::sync::atomic::Ordering::Relaxed);
        self.search_done = Some(self.settings_search.clone());
        self.page_scroll_mut().jump(0.0);
    }

    // Alle Zeilen, die die Suche kennt. Die Einstellungsseiten werden dafuer einmal ohne
    // Scroll-Grenzen aufgebaut und ihre Beschriftungen (Spalte SETTINGS_LABEL_X) ueber ihre
    // Hoehe den Zeilen zugeordnet -- so findet die Suche jede Zeile, auch kuenftige, ohne
    // eigene Liste. Danach die Detailzeilen jedes Moduls.
    pub(crate) fn search_index(&mut self) -> Vec<SearchHit> {
        use std::sync::atomic::Ordering::Relaxed;
        let mut out: Vec<SearchHit> = Vec::new();
        let vorher = (self.screen, self.module_menu_selected);
        for (seite, name) in SETTINGS_CATEGORIES {
            let Some(page) = seite.settings_page() else { continue };
            self.screen = seite;
            let starts = page.section_starts();
            SETTINGS_SCROLL_FIRST.store(0f32.to_bits(), Relaxed);
            SETTINGS_SCROLL_BOTTOM.store(f32::INFINITY.to_bits(), Relaxed);
            store_section_starts(starts);
            let Some(gpu) = self.gpu.as_ref() else { break };
            let (mut rects, mut texts, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
            self.build_screen_content(gpu, &mut rects, &mut texts, &mut sprites);
            let dy = (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0;
            for t in texts {
                if t.x != SETTINGS_LABEL_X || t.text.trim().is_empty() {
                    continue;
                }
                let Some(row) = (0..page.rows).find(|&r| ((SETTINGS_FIRST_Y + settings_row_pos(r as f32, starts) + dy).round() - t.y).abs() < 1.0) else { continue };
                let abschnitt = page.sections.iter().rev().find(|&&(_, start)| start <= row).map(|&(n, _)| n);
                let path = match abschnitt {
                    Some(a) if page.sections.len() >= 2 => format!("{} › {}", i18n::tr(name), i18n::tr(a)),
                    _ => i18n::tr(name).into_owned(),
                };
                let place = SearchPlace::Page(seite, row);
                if !out.iter().any(|h| h.place == place && h.label == t.text) {
                    out.push(SearchHit { label: t.text, path, place });
                }
            }
        }
        self.screen = vorher.0;
        self.sync_settings_scroll();
        for kategorie in ModuleCategory::all() {
            for id in kategorie.modules() {
                self.module_menu_selected = id;
                let ort = format!("{} › {}", i18n::tr("Module"), i18n::tr(kategorie.label()));
                out.push(SearchHit { label: id.label().to_string(), path: ort.clone(), place: SearchPlace::Module(id, 0) });
                let pfad = format!("{ort} › {}", i18n::tr(id.label()));
                for (i, zeile) in self.module_detail_rows().iter().enumerate().skip(1) {
                    if matches!(zeile, ModuleRow::Heading { .. }) || zeile.label().trim().is_empty() {
                        continue;
                    }
                    out.push(SearchHit { label: zeile.label().to_string(), path: pfad.clone(), place: SearchPlace::Module(id, i) });
                }
            }
        }
        self.module_menu_selected = vorher.1;
        out
    }

    // Klick auf einen Treffer: Seite bzw. Modul oeffnen und die Zeile in den Blick holen.
    pub(crate) fn open_search_hit(&mut self, i: usize) {
        let Some(place) = self.search_hits.get(i).map(|t| t.place) else { return };
        match place {
            SearchPlace::Page(seite, row) => {
                self.open_settings(seite);
                self.sync_settings_scroll();
                let max = self.page_max_scroll();
                // Zwei Zeilen Luft darueber, damit man sieht, wo man gelandet ist.
                self.page_scroll_mut().glide_to(row.saturating_sub(2).min(max) as f32, max as f32);
            }
            SearchPlace::Module(id, row) => {
                self.open_settings(MenuScreen::SettingsModules);
                self.module_menu_category = id.category();
                self.apply_module_action(ButtonAction::SelectModule(id));
                if let Some(max) = self.gpu.as_ref().map(|gpu| self.module_max_scroll(gpu)) {
                    self.module_menu_scroll.glide_to(row.saturating_sub(1).min(max) as f32, max as f32);
                }
            }
        }
        self.search_flash = Some((place, Instant::now()));
    }

    pub(crate) fn build_settings_particles(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        // Spaltenkoepfe ueber den Knoepfen. Die vierte Spalte ist je nach Art Menge (Schwall)
        // oder Intervall (verweilend), s. particles::SpawnKind.
        let regler = |col: usize| (particle_column_x(col), PARTICLE_SLIDER_W);
        let koepfe = [
            ((PARTICLE_TOGGLE_X, PARTICLE_TOGGLE_W), "Zeigen"),
            (regler(0), "Dauer"),
            (regler(1), "Deckkraft"),
            (regler(2), "Tempo"),
            (regler(3), "Menge / Intervall"),
            ((particle_column_x(PARTICLE_ALL_AT_ONCE_COL), PARTICLE_TOGGLE_W), "Auf einmal"),
        ];
        let sichtbar = self.particle_visible_rows();
        // Welche der beiden Anpassungen (s. ButtonAction::OpenParticleSettings) hier offen ist.
        let own = self.particle_screen_own;
        let kopf = if particles::KINDS.len() > sichtbar {
            let erste = header_list_first(particles::KINDS.len(), self.particle_scroll.stand, sichtbar);
            let bis = (erste + sichtbar).min(particles::KINDS.len());
            if own {
                format!("Eigene Partikel ({}–{} von {})", erste + 1, bis, particles::KINDS.len())
            } else {
                format!("Andere Partikel ({}–{} von {})", erste + 1, bis, particles::KINDS.len())
            }
        } else if own {
            "Eigene Partikel".to_string()
        } else {
            "Andere Partikel".to_string()
        };
        // Die Koepfe stehen ueber dem Zeilenband: weder beschnitten noch ausgeblendet.
        push_fade_marker(rects, texts, None);
        crate::push_clip_marker(rects, texts, false);
        texts.push(UiText { text: kopf, x: SETTINGS_LABEL_X, y: zeile(0), color: theme::text_2(), scale: UI_SCALE_SMALL });
        for ((x, w), text) in koepfe {
            texts.push(gpu.centered_text(Rect { x, y: settings_row_y(0), w, h: SETTINGS_CTRL_H }, text.to_string(), theme::text_2(), UI_SCALE_SMALL));
        }
        crate::push_clip_marker(rects, texts, true);
        push_fade_marker(rects, texts, Some(self.particle_list_band().1));
        let text_dy = (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0;
        for (i, y) in header_list_rows_smooth(particles::KINDS.len(), self.particle_scroll.stand, sichtbar) {
            let t = self.settings.particle_tuning_of(own)[i];
            let art = particles::KINDS[i].spawn;
            let farbe = if t == particles::ParticleTuning::default() { theme::text() } else { COLOR_HUD };
            texts.push(UiText { text: particles::KINDS[i].label.to_string(), x: SETTINGS_LABEL_X, y: (y + text_dy).round(), color: farbe, scale: UI_SCALE_SMALL });
            let mut werte = vec![t.lifetime_pct, t.opacity_pct, t.speed_pct];
            if art.has_amount() {
                werte.push(t.amount_pct);
            } else if art.has_interval() {
                werte.push(t.interval_pct);
            }
            // Wertfelder wie bei einem Stepper -- der Regler holt sich Flaeche und Zahl daraus.
            for (col, wert) in werte.into_iter().enumerate() {
                let rect = Rect { y, ..particle_value_rect(col, 0) };
                rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                let wert_farbe = if wert == 100 { theme::text() } else { COLOR_HUD };
                texts.push(gpu.centered_text(rect, format!("{wert} %"), wert_farbe, UI_SCALE_SMALL));
            }
        }
    }

    pub(crate) fn build_settings_inventory_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let y = (settings_row_y(0) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText {
            // "Opakheit", nicht "Transparenz": der Prozentwert IST die Abdunklung, er laeuft
            // also andersherum als der alte Name sagte (0 % = Welt voll sichtbar, 100 % =
            // deckend schwarz, s. Settings::inventory_transparency). Genau diese Umkehrung war
            // hier schon einmal ein echter Bug im Code -- der Name hat ihn nahegelegt.
            text: "Hintergrund-Opakheit".to_string(),
            x: SETTINGS_LABEL_X,
            y,
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
        let value = format!("{} %", self.settings.inventory_transparency);
        self.draw_editable_value(gpu, stepper_value_rect(0), EditableValue::InventoryTransparency, value, rects, texts);

        let y1 = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Skalierung".to_string(), x: SETTINGS_LABEL_X, y: y1, color: theme::text(), scale: UI_SCALE_SMALL });
        let scale_rect = stepper_value_rect(1);
        rects.push(UiRect { rect: scale_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(scale_rect, Self::scale_label(self.settings.inventory_scale, gpu.size()), theme::text(), UI_SCALE_SMALL));

        let y2 = (settings_row_y(2) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Zeiger in Slotmitte".to_string(), x: SETTINGS_LABEL_X, y: y2, color: theme::text(), scale: UI_SCALE_SMALL });

        let y3 = (settings_row_y(3) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Operator-Items (Kreativ)".to_string(), x: SETTINGS_LABEL_X, y: y3, color: theme::text(), scale: UI_SCALE_SMALL });

        for (zeile, text) in [
            (4, "Farbe der Anzahl"),
            (5, "Trank-Buchstabe S (verstärkt)"),
            (6, "Trank-Buchstabe L (verlängert)"),
            (7, "Trank-Buchstabe W (schwach)"),
            (SLOT_HOVER_FADE_ROW, "Hover-Übergang"),
            (SLOT_HOVER_COLOR_ROW, "Hover-Farbe"),
            (SWAP_ANIM_ROW, "Verschieben animieren"),
            (SWAP_ANIM_TOTEM_ROW, "Außer Totems"),
            (SWAP_ANIM_HEALING_ROW, "Außer Heiltränke"),
            (DOLL_BACKDROP_ROW, "Puppen-Hintergrund durchsichtig"),
        ] {
            let y = (settings_row_y(zeile) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: text.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        // Dauern in Millisekunden, 0 heisst aus.
        for (zeile, ms) in [(SLOT_HOVER_FADE_ROW, self.settings.slot_hover_fade_ms), (SWAP_ANIM_ROW, self.settings.swap_anim_ms)] {
            let rect = stepper_value_rect(zeile);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            let text = if ms == 0 { "Aus".to_string() } else { format!("{ms} ms") };
            texts.push(gpu.centered_text(rect, text, theme::text(), UI_SCALE_SMALL));
        }
    }

    pub(crate) fn build_settings_audio_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let y0 = (settings_row_y(AUDIO_DEVICE_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Ausgabegerät".to_string(), x: SETTINGS_LABEL_X, y: y0, color: theme::text(), scale: UI_SCALE_SMALL });
        // Das Geraet selbst ist eine Aufklappliste aus buttons().

        let y1 = (settings_row_y(AUDIO_TEST_TONE_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Testton".to_string(), x: SETTINGS_LABEL_X, y: y1, color: theme::text(), scale: UI_SCALE_SMALL });

        // Master-Tonhoehe (echt, s. Settings::ui_pitch_semitones) -- Pfeile kommen aus
        // buttons(), hier nur Beschriftung und Wert.
        let y2 = (settings_row_y(AUDIO_PITCH_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText {
            text: "Tonhöhe (Aufnahmen)".to_string(),
            x: SETTINGS_LABEL_X,
            y: y2,
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
        let pitch_rect = stepper_value_rect(AUDIO_PITCH_ROW);
        rects.push(UiRect { rect: pitch_rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        texts.push(gpu.centered_text(pitch_rect, format_ui_pitch(self.settings.ui_pitch_semitones), theme::text(), UI_SCALE_SMALL));

        // Puffer: die Auswahl ist eine Aufklappliste aus buttons(), darunter nur Anzeige, womit
        // der Stream gerade laeuft (s. format_audio_buffer).
        let y5 = (settings_row_y(AUDIO_BUFFER_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Puffer".to_string(), x: SETTINGS_LABEL_X, y: y5, color: theme::text(), scale: UI_SCALE_SMALL });
        let y6 = (settings_row_y(AUDIO_BUFFER_INFO_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Puffer aktiv".to_string(), x: SETTINGS_LABEL_X, y: y6, color: theme::text(), scale: UI_SCALE_SMALL });
        let buffer_rect = settings_wide_rect(AUDIO_BUFFER_INFO_ROW);
        rects.push(UiRect { rect: buffer_rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        texts.push(gpu.centered_text(buffer_rect, format_audio_buffer(self.audio.buffer_info()), theme::text(), UI_SCALE_SMALL));

        // PipeWire: gemessene Blockgroesse, darunter die Grenze. Deren Knopf kommt aus buttons(),
        // wenn es etwas zu tun gibt; sonst steht hier, warum nicht.
        let y7 = (settings_row_y(AUDIO_PIPEWIRE_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "PipeWire aktiv".to_string(), x: SETTINGS_LABEL_X, y: y7, color: theme::text(), scale: UI_SCALE_SMALL });
        let pw_rect = settings_wide_rect(AUDIO_PIPEWIRE_ROW);
        rects.push(UiRect { rect: pw_rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        texts.push(gpu.centered_text(pw_rect, self.pipewire_text(), theme::text(), UI_SCALE_SMALL));
        let y_prio = (settings_row_y(AUDIO_PIPEWIRE_PRIO_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "pipewire-pulse".to_string(), x: SETTINGS_LABEL_X, y: y_prio, color: theme::text(), scale: UI_SCALE_SMALL });
        let prio_rect = settings_wide_rect(AUDIO_PIPEWIRE_PRIO_ROW);
        rects.push(UiRect { rect: prio_rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        texts.push(gpu.centered_text(prio_rect, self.pipewire_prio_text(), theme::text(), UI_SCALE_SMALL));
        let y8 = (settings_row_y(AUDIO_PIPEWIRE_SET_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "PipeWire-Grenze".to_string(), x: SETTINGS_LABEL_X, y: y8, color: theme::text(), scale: UI_SCALE_SMALL });
        if let PipewireKnopf::Text(text) = self.pipewire_knopf() {
            let rect = settings_wide_rect(AUDIO_PIPEWIRE_SET_ROW);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, text, theme::text(), UI_SCALE_SMALL));
        }
        let y_auto = (settings_row_y(AUDIO_AUTO_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Automatisch optimieren".to_string(), x: SETTINGS_LABEL_X, y: y_auto, color: theme::text(), scale: UI_SCALE_SMALL });

        // Raumklang-Hoertest (echt, s. spatial_sound::OrbitTest) -- der Knopf selbst kommt
        // aus buttons(), hier nur die Beschriftung davor.
        let y3 = (settings_row_y(AUDIO_SPATIAL_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Raumklang-Test".to_string(), x: SETTINGS_LABEL_X, y: y3, color: theme::text(), scale: UI_SCALE_SMALL });

        // Hauptlautstaerke (echt, s. Settings::master_volume) -- Pfeile aus buttons().
        let y4 = (settings_row_y(AUDIO_MASTER_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Hauptlautstärke".to_string(), x: SETTINGS_LABEL_X, y: y4, color: theme::text(), scale: UI_SCALE_SMALL });
        let volume_label = volume_text(self.settings.master_volume);
        self.draw_editable_value(gpu, stepper_value_rect(AUDIO_MASTER_ROW), EditableValue::MasterVolume, volume_label, rects, texts);

        // Je Kategorie ein Regler (s. VOLUME_CATEGORIES), Pfeile aus buttons().
        for (i, (kategorie, wert)) in VOLUME_CATEGORIES.iter().zip(self.settings.category_volume).enumerate() {
            let row = AUDIO_CATEGORY_FIRST_ROW + i;
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: kategorie.label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::CategoryVolume(i), volume_text(wert), rects, texts);
        }
        // Je UI-Klang ein Regler (s. ui_sound::UiSound::ALL), Pfeile aus buttons().
        for (i, (klang, wert)) in ui_sound::UiSound::ALL.iter().zip(self.settings.ui_sound_volume).enumerate() {
            let row = AUDIO_UI_SOUND_FIRST_ROW + i;
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: klang.label().to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::UiSoundVolume(i), volume_text(wert), rects, texts);
        }
    }
}
