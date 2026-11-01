// Rahmen zeichnen, fallende Zeichen, Benachrichtigungen, Hover und Bildschirminhalt.

use super::*;

impl App {
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
        eintrag(rects, texts, Self::sidebar_search_rect(sh), MenuScreen::SettingsSearch, "Suche");
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
        Some((panel, UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None }))
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
    pub(crate) fn drawn_hover(&self) -> Option<(i32, i32, i32, i32)> {
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
            MenuScreen::Connect => self.build_connect_screen(rects, texts),
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
            MenuScreen::SettingsSearch => self.build_settings_search(gpu, rects, texts),
            MenuScreen::HudEditor => match (self.crosshair_painter.is_some(), self.hud_selected) {
                (true, _) => self.build_crosshair_painter(gpu, rects, texts),
                (false, Some(element)) => self.build_hud_config(gpu, element, rects, texts),
                (false, None) => {}
            },
            MenuScreen::Death => self.build_death_screen(gpu, rects, texts),
        }
    }
}
