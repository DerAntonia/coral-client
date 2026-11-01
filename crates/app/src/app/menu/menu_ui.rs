// Das ganze Menue-Bild und die Tooltips.

use super::*;

impl App {
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
        // Seiten mit Seiten-Scroll: Zeilen werden am Zeilenband beschnitten (s. content_clip),
        // die Reiter darueber nicht.
        let band = self.screen.settings_page().is_some() && self.mode == AppMode::Menu;
        let seiten_fade = ScrollFade::new(SETTINGS_CLIP_TOP, settings_rows_bottom(screen_h), SETTINGS_ROW_H, self.page_scroll().stand, self.page_max_scroll() as f32);
        if band {
            push_clip_marker(&mut rects, &mut texts, true);
            push_fade_marker(&mut rects, &mut texts, Some(seiten_fade));
        }

        // Nach einem Klick auf einen Suchtreffer leuchtet seine Zeile kurz auf.
        if let Some((SearchPlace::Page(seite, row), seit)) = self.search_flash {
            let k = 1.0 - seit.elapsed().as_secs_f32() / SEARCH_FLASH_SECS;
            if seite == self.screen && k > 0.0 {
                let r = Rect { x: SETTINGS_LABEL_X - 12.0, y: settings_row_y(row) - 4.0, w: SETTINGS_CTRL_X - SETTINGS_LABEL_X + SLIDER_SPAN_W + 24.0, h: SETTINGS_CTRL_H + 8.0 };
                rects.push(UiRect::rounded(r, theme::with_alpha(theme::get(theme::Slot::Accent), 0.35 * k.min(1.0)), 6.0));
            }
        }

        // Inhalt des aktuellen Screens. Die Bedienelemente selbst kommen aus buttons()
        // (weiter unten) -- hier nur Beschriftungen, Felder, Panels, Editor-Flaeche.
        self.build_screen_content(gpu, &mut rects, &mut texts, &mut sprites);

        // Buttons (Hintergrund + zentriertes Label) -- was klickbar ist, kommt aus einer
        // Quelle (buttons()), damit Zeichnen und Klick-Trefferpruefung nie auseinanderlaufen.
        for button in self.content_buttons() {
            let hover = !self.pointer_blocked() && button.hits(self.cursor_pos.0, self.cursor_pos.1);
            // Die Reiter liegen ueber dem Zeilenband und werden nicht beschnitten (sie haben
            // weder Regler noch Aufklappliste, kommen also unten an).
            let reiter = band && matches!(button.action, ButtonAction::SettingsSection(_));
            if reiter {
                push_fade_marker(&mut rects, &mut texts, None);
                push_clip_marker(&mut rects, &mut texts, false);
            }
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
                let ende = self.step_slider_end(&sl);
                let t = self.glide_slider(SliderKey::Step(sl.prev), ((sl.value - sl.min) / (ende - sl.min).max(1e-9)) as f32);
                let aktiv = self.step_drag == Some(sl.prev);
                Self::push_slider_visual(&mut rects, sl.track(), t, hover || aktiv, aktiv, button.enabled);
                continue;
            }
            let (flaeche, color) = styled_button(&button, self.hover_level(button.rect, hover));
            let (dx, dy) = self.content_offset();
            // Zurueck in Inhaltskoordinaten, die Verschiebung folgt unten fuer alles zusammen.
            let mut rect = Rect { x: button.rect.x - dx, y: button.rect.y - dy, ..button.rect };
            // Der aktive Reiter zeichnet seine Hinterlegung dort, wo sie gerade hingleitet.
            if reiter && button.color == theme::get(theme::Slot::AccentSurface) {
                if let Some((_, r)) = self.section_slide.filter(|&(screen, _)| screen == self.screen) {
                    rect = Rect { x: r.x, w: r.w, ..rect };
                }
            }
            rects.push(UiRect { rect, ..flaeche });
            if let Some(c) = Self::choice_of(button.action) {
                let offen = self.dropdown_active().is_some_and(|d| d.choice == c);
                self.push_dropdown_face(gpu, rect, &button.label, color, button.scale, offen, &mut rects, &mut texts);
                continue;
            }
            texts.push(gpu.centered_text(rect, button.label, color, button.scale));
            if reiter {
                push_clip_marker(&mut rects, &mut texts, true);
                push_fade_marker(&mut rects, &mut texts, Some(seiten_fade));
            }
        }
        if band {
            push_fade_marker(&mut rects, &mut texts, None);
            push_clip_marker(&mut rects, &mut texts, false);
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
            push_clip_marker(&mut rects, &mut texts, true);
            push_fade_marker(&mut rects, &mut texts, Some(seiten_fade));
            self.build_font_slots(gpu, &mut rects, &mut texts);
            push_fade_marker(&mut rects, &mut texts, None);
            push_clip_marker(&mut rects, &mut texts, false);
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
            UiOverlayGroup { rects: fenster, texts: texts.drain(inhalt_t..).collect(), sprites: sprites.drain(inhalt_s..).collect(), pivot: (0.0, 0.0), angle: 0.0, clip: None }
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
                gruppen.insert(0, UiOverlayGroup { rects: r, texts: t, sprites: s, pivot: (0.0, 0.0), angle: 0.0, clip: None });
                gruppen.insert(0, self.build_server_list(gpu));
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
                } else if settings_row_hover_rect(VIEW_MODEL_ROW).contains(cx, cy) {
                    Some(VIEW_MODEL_TOOLTIP)
                } else if settings_row_hover_rect(DEATH_ANIM_ROW).contains(cx, cy) {
                    Some(DEATH_ANIM_TOOLTIP)
                } else if (0..world::more_anims::FEATURES.len()).any(|i| settings_row_hover_rect(ANIM_FIRST_ROW + i).contains(cx, cy)) {
                    Some(ANIMS_TOOLTIP)
                } else if settings_row_hover_rect(RENDER_DISTANCE_ROW).contains(cx, cy) {
                    Some(RENDER_DISTANCE_TOOLTIP)
                } else if settings_row_hover_rect(ENTITY_DISTANCE_ROW).contains(cx, cy) {
                    Some(ENTITY_DISTANCE_TOOLTIP)
                } else if settings_row_hover_rect(BLOCK_ENTITY_DISTANCE_ROW).contains(cx, cy) {
                    Some(BLOCK_ENTITY_DISTANCE_TOOLTIP)
                } else if settings_row_hover_rect(LOD_DETAIL_ROW).contains(cx, cy) {
                    Some(LOD_DETAIL_TOOLTIP)
                } else if settings_row_hover_rect(SHOULDER_ROW).contains(cx, cy) || settings_row_hover_rect(SHOULDER_OFFSET_ROW).contains(cx, cy) {
                    Some(SHOULDER_TOOLTIP)
                } else if settings_row_hover_rect(SHOULDER_CROSSHAIR_ROW).contains(cx, cy) {
                    Some(SHOULDER_CROSSHAIR_TOOLTIP)
                } else if settings_row_hover_rect(SHOULDER_FADE_ROW).contains(cx, cy) {
                    Some(SHOULDER_FADE_TOOLTIP)
                } else if settings_row_hover_rect(SHOULDER_DECOUPLED_ROW).contains(cx, cy) {
                    Some(SHOULDER_DECOUPLED_TOOLTIP)

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
            MenuScreen::SettingsFont => Some(if (0..=TEXT_AREA_COUNT).any(|i| self.settings.shadow_slot_style(i) == Some(TextShadow::Vanilla) && shadow_half_rect(i).contains(cx, cy)) {
                VANILLA_SHADOW_TOOLTIP
            } else if settings_row_hover_rect(0).contains(cx, cy) {
                if cx < FONT_SHADOW_X - 5.0 { FONT_TOOLTIP } else { TEXT_SHADOW_TOOLTIP }
            } else if settings_row_hover_rect(STABLE_NUMBERS_ROW).contains(cx, cy) {
                STABLE_NUMBERS_TOOLTIP
            } else if settings_row_hover_rect(CAPITALS_ROW).contains(cx, cy) {
                CAPITALS_TOOLTIP
            } else if (FONT_EXCEPTIONS_ROW..=TEXT_AREA_COUNT).any(|row| settings_row_hover_rect(row).contains(cx, cy)) {
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
                    Some("Farbe der Stückzahl in den Slots von Inventar und Kisten. Die Hotbar hat ihre eigene (HUD-Editor, Hotbar).")
                } else if settings_row_hover_rect(6).contains(cx, cy) {
                    Some("Kleines S neben dem Flaschenhals verstärkter Tränke (Stufe II), z. B. Stärke II oder Schnelligkeit II. Gilt im Inventar, nicht in der Hotbar.")
                } else if settings_row_hover_rect(7).contains(cx, cy) {
                    Some("Kleines L neben dem Flaschenhals verlängerter Tränke (mit Redstone gebraut). Gilt im Inventar, nicht in der Hotbar.")
                } else if settings_row_hover_rect(8).contains(cx, cy) {
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
}
