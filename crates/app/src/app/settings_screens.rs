// Die Einstellungsseiten: Allgemein, Video, Steuerung, Design, Schrift, Audio, Skins.

use crate::{theme, skin_tab, App, GpuState, UiText, UI_MARGIN, UI_SCALE_TITLE, UI_SCALE_SMALL, active_config, Instant, i18n, Duration, system_info, CPU_ROW, GPU_ROW, RAM_ROW, SWAP_ROW, COMPOSITING_ROW, UiRect, SETTINGS_LABELS, Rect, SETTINGS_LABEL_X, settings_row_y, SETTINGS_CTRL_X, SETTINGS_CTRL_H, stepper_value_rect, FRAME_LATENCY_ROW, FPS_LIMIT_ROW, fps_limit_label, EditableValue, MESH_BUDGET_ROW, CHUNK_THREADS_ROW, TEAR_TIMING_ROW, TEAR_RESERVE_ROW, SLOT_HOVER_FADE_ROW, SLOT_HOVER_COLOR_ROW, SWAP_ANIM_ROW, SWAP_ANIM_TOTEM_ROW, SWAP_ANIM_HEALING_ROW, DOLL_BACKDROP_ROW, Perspective, PROJECTION_ROW, SMOOTH_LIGHTING_ROW, BLOCK_ROTATION_ROW, PARTICLES_ROW, PARTICLES_OTHER_ROW, PARTICLE_DISTANCE_ROW, PARTICLE_FADE_ROW, ENTITY_NEAR_ROW, ARMOR_TRIMS_ROW, VIEW_MODEL_ROW, VIEW_MODEL_BOB_ROW, DAMAGE_TINT_FIRST_ROW, DAMAGE_TINT_SKIN_ONLY_ROW, DAMAGE_TINT_TICKS_ROW, damage_tint_duration_text, RENDER_DISTANCE_ROW, ENTITY_DISTANCE_ROW, BLOCK_ENTITY_DISTANCE_ROW, CHUNK_FADE_ROW, MAX_RENDER_DISTANCE_MAX, ENTITY_DISTANCE_MAX, BLOCK_ENTITY_DISTANCE_MAX, controls_rows, ControlsRow, format_sensitivity, format_cursor_sensitivity, AppMode, SETTINGS_CONTENT_DX, SETTINGS_FIRST_Y, MenuScreen, SETTINGS_ROW_H, SETTINGS_SECTION_GAP, settings_rows_bottom, MENU_BACKDROP_ROW, SCROLL_INERTIA_ROW, SCROLL_DISTANCE_ROW, WALLPAPER_ROW, PANORAMA_SPEED_ROW, WALLPAPER_PICK_X, wallpaper, shared_config_dir, hero_scene, theme_tile_rect, mix_color, push_bar, ThemeChoice, push_typed_marker, DESIGN_THEME_ROW, design_row_count, THEME_DIVIDER_X, THEME_COLORS_X, theme_slot_cell, THEME_COLOR_SWATCH_DX, SWATCH_W, UiOverlay, NameDialogPurpose, dialog_dim, UI_SCALE_NORMAL, TOPBAR_SUB_SCALE, Button, ButtonAction, styled_button, UiOverlayGroup, text_field, field_press, ui_sound, CustomTheme, ui_screen, LANGUAGE_ROWS, LOANWORDS_ROW, wrap_text, LANGUAGE_HINT, STABLE_NUMBERS_ROW, CAPITALS_ROW, ShadowArea, FONT_EXCEPTIONS_ROW, FontSlot, FontTile, ellipsize, push_font_marker, font_palette_name_h, SHADOW_CHECKER_PX, SHADOW_CHECKER_YELLOW, SHADOW_CHECKER_GREY, push_shadow_marker, SHADOW_PREVIEW_TEXT, FontPalette, SETTINGS_CONTENT_DY, tooltip_strip_w, FONT_SLOT_H, FontDrag, area_font_code, area_shadow_code, minecraft_sens_to_client, SENSITIVITY_MIN, SENSITIVITY_MAX, SENS_CALC_MC_ROW, trimme_nullen, SENS_CALC_RESULT_ROW, SENS_CALC_DPI_ROW, cm_per_360, SENS_CALC_CM_MC_ROW, SENS_CALC_CM_OWN_ROW, GLINT_SPEED_ROW, GLINT_STRENGTH_ROW, PLAYER_LIST_SCALE_ROW, CRIT_COLOR_ROW, FIRE_MODE_CROSSHAIR, FIRE_MODE_OFF, HURT_CAM_ROW, HURT_CAM_STRENGTH_ROW, SHOULDER_ROW, SHOULDER_OFFSET_ROW, SHOULDER_CROSSHAIR_ROW, SHOULDER_FADE_ROW, SHOULDER_DECOUPLED_ROW, FIRE_MODE_ROW, FIRE_DETAIL_ROW, DEATH_ANIM_ROW, ANIM_ROW, FIRE_MODE_VANILLA, InputKind, ConfigEdit, config_name_field_rect, CONFIG_LIST_FIRST_ROW, MAX_VISIBLE_CONFIGS, CARET_WIDTH_PX, SLIDER_SPAN_W, SEARCH_HIT_COUNT, SearchHit, SETTINGS_CATEGORIES, SETTINGS_SCROLL_FIRST, SETTINGS_SCROLL_BOTTOM, store_section_starts, settings_row_pos, SearchPlace, ModuleCategory, ModuleRow, COLOR_HUD, particle_column_x, PARTICLE_SLIDER_W, PARTICLE_TOGGLE_X, PARTICLE_TOGGLE_W, particle_value_rect, AUDIO_DEVICE_ROW, AUDIO_TEST_TONE_ROW, AUDIO_PITCH_ROW, format_ui_pitch, AUDIO_BUFFER_ROW, AUDIO_BUFFER_INFO_ROW, settings_wide_rect, format_audio_buffer, AUDIO_PIPEWIRE_ROW, AUDIO_PIPEWIRE_PRIO_ROW, AUDIO_PIPEWIRE_SET_ROW, PipewireKnopf, AUDIO_AUTO_ROW, AUDIO_SPATIAL_ROW, AUDIO_MASTER_ROW, volume_text, VOLUME_CATEGORIES, AUDIO_CATEGORY_FIRST_ROW, AUDIO_UI_SOUND_FIRST_ROW, UiSprite, skin_column_rect, SKIN_HEAD_H, skin_tiles_top, skin_tiles_bottom, ScrollFade, skin_tile_h, push_fade_marker, skin_clip, skin_icon_rect, pack_icon_uv, fade_sprite, SpriteAtlas, wrap_spans, ellipsize_spans, push_styled_row, skin_tile_rect, SKIN_TILE_GAP, skin_tiles_visible, SkinDrag, SERVER_DRAG_START_PX, HoverTarget, skin_files_layout, server_packs, skins_dir, list_available_skins, SkinFilesWindow, skin_files_rows_visible, SKIN_FILES_ROW_H, SKIN_FILES_INDENT, SKIN_FILES_ROWS_PER_NOTCH, TOGGLE_W, PARTICLE_HEADS, particle_slider_rows, DISCONNECT_POPUP_SCALE, span_row_width};

mod design;
mod fonts;
mod pages;
mod skin_page;

impl App {
    // Einstellungen-Tab: Beschriftung links, Bedienelement rechts. Die Bedienelemente
    // selbst kommen aus buttons(); hier nur Text und Wertfelder.
    // Kategorie-Auswahl: Titel + eine Kachel je SETTINGS_CATEGORIES-Eintrag (Buttons selbst
    // kommen aus buttons(), hier nur die Ueberschrift).
    pub(crate) fn build_settings_hub(&self, gpu: &GpuState, texts: &mut Vec<UiText>) {
        let title = "Einstellungen";
        texts.push(UiText { text: title.to_string(), x: UI_MARGIN, y: 90.0, color: theme::text(), scale: UI_SCALE_TITLE });
        // Welche Config gerade bearbeitet wird (gewaehlt im Launcher) -- rechts neben dem Titel
        // auf dessen Grundlinie, darunter beginnen schon die Kacheln.
        let x = UI_MARGIN + gpu.measure_text(title, UI_SCALE_TITLE) + 24.0;
        let y = 90.0 + gpu.text_height(UI_SCALE_TITLE) - gpu.text_height(UI_SCALE_SMALL);
        texts.push(UiText { text: format!("Config: {}", active_config()), x, y: y.round(), color: theme::text_disabled(), scale: UI_SCALE_SMALL });
    }

    // Werte des Abschnitts System (Zeile, Text). Speicher wird hoechstens einmal je Sekunde
    // gelesen; der Compositor alle 3 s in einem eigenen Thread (X-Verbindung, gsettings), das
    // Bild zeigt solange den letzten Stand.
    pub(crate) fn system_rows(&self, gpu: &GpuState) -> [(usize, String); 5] {
        static CPU: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let jetzt = Instant::now();
        let unbekannt = || i18n::tr("unbekannt").into_owned();
        let speicher = match self.memory_info.get() {
            Some((t, m)) if jetzt.duration_since(t) < Duration::from_secs(1) => m,
            _ => {
                let m = system_info::memory();
                self.memory_info.set(Some((jetzt, m)));
                m
            }
        };
        if self.compositing_checked.get().is_none_or(|t| jetzt.duration_since(t) >= Duration::from_secs(3)) {
            self.compositing_checked.set(Some(jetzt));
            let ziel = std::sync::Arc::clone(&self.compositing_info);
            std::thread::spawn(move || {
                let c = system_info::compositing();
                if let Ok(mut g) = ziel.lock() {
                    *g = c;
                }
            });
        }
        // Was mit unserem Fenster passiert: setzt der Compositor es zusammen, oder kommt es
        // direkt auf den Schirm -- und dann per Hardware-Flip oder als Kopie des X-Servers.
        let compositor = self.compositing_info.lock().ok().and_then(|g| g.clone());
        let fenster = self.window_path.as_ref().and_then(|w| w.lock().ok().map(|g| *g));
        let compositing = match (compositor, fenster) {
            (Some(system_info::Compositing::Always), _) => {
                let dwm = i18n::tr("DWM läuft, Umgehung nicht geprüft").into_owned();
                // Windows: wo die Presents landen (s. vblank::note_present).
                #[cfg(windows)]
                // In ms ab Zeile 0 wie Tear timing, mit der Lage der Luecke dahinter.
                let dwm = match crate::vblank::present_phase() {
                    Some(p) => format!(
                        "{dwm} · Present {:.2} ms ±{:.2} ({} {:.2}–{:.2})",
                        p.mitte_ms,
                        (p.oben_ms - p.unten_ms) / 2.0,
                        i18n::tr("Lücke"),
                        p.sichtbar_ms,
                        p.periode_ms
                    ),
                    None => dwm,
                };
                dwm
            }
            (c, Some(system_info::WindowPath { composited: Some(true), .. })) => {
                let name = match c {
                    Some(system_info::Compositing::On { name: Some(n) }) => n,
                    _ => "Compositor".to_string(),
                };
                format!("{} {name}", i18n::tr("zusammengesetzt von"))
            }
            (_, Some(f)) => {
                let weg = i18n::tr(match f.last_present {
                    Some(system_info::PresentPath::Flip) => "Hardware-Flip",
                    Some(system_info::PresentPath::Copy) => "Kopie (X-Server)",
                    Some(system_info::PresentPath::SuboptimalCopy) => "Kopie, Flip nicht möglich",
                    Some(system_info::PresentPath::Skip) => "übersprungen",
                    None => "direkt",
                });
                match self.tear_readout(&f) {
                    Some(riss) => format!("{weg} · {riss}"),
                    None => weg.into_owned(),
                }
            }
            (Some(system_info::Compositing::Off), None) => i18n::tr("aus").into_owned(),
            (Some(system_info::Compositing::On { name }), None) => name.unwrap_or_else(|| i18n::tr("an").into_owned()),
            (None, None) => unbekannt(),
        };
        let ram = speicher.map_or_else(unbekannt, |m| system_info::format_used(m.ram_used, m.ram_total));
        let swap = match speicher {
            Some(m) if m.swap_total == 0 => i18n::tr("keiner").into_owned(),
            Some(m) => system_info::format_used(m.swap_used, m.swap_total),
            None => unbekannt(),
        };
        let cpu = CPU.get_or_init(system_info::cpu_name).clone().unwrap_or_else(unbekannt);
        [(CPU_ROW, cpu), (GPU_ROW, gpu.gpu_label()), (RAM_ROW, ram), (SWAP_ROW, swap), (COMPOSITING_ROW, compositing)]
    }

    // Wo der Riss der letzten Bilder lag, aus den Meldungen des X-Servers und den VBlanks (s.
    // system_info::tear_position). Steht er bei jeder Einstellung an derselben Stelle, haelt der
    // X-Server die Bilder fest -- dann hilft nur ein Neustart von X.
    pub(crate) fn tear_readout(&self, f: &system_info::WindowPath) -> Option<String> {
        let (letzter, periode) = self.vblank.as_ref()?.latest()?;
        let sichtbar = if let Some((sichtbar, gesamt)) = self.vblank.as_ref().and_then(|v| v.lines) { periode * f64::from(sichtbar) / f64::from(gesamt) } else {
            let h = f64::from(self.gpu.as_ref()?.real_size().1);
            periode * h / (h + 45.0)
        };
        let (mitte, streuung) = system_info::tear_position(f, letzter.ts_ns, periode, sichtbar)?;
        Some(if streuung > 0.15 {
            i18n::tr("Riss wandert").into_owned()
        } else if mitte >= 1.0 {
            i18n::tr("Riss in der Austastlücke").into_owned()
        } else {
            format!("{} {:.0} %", i18n::tr("Riss bei"), mitte * 100.0)
        })
    }

    pub(crate) fn build_settings_general_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let set = &self.settings;
        for (row, label) in SETTINGS_LABELS.iter().enumerate() {
            let row_rect = Rect { x: SETTINGS_LABEL_X, y: settings_row_y(row), w: SETTINGS_CTRL_X, h: SETTINGS_CTRL_H };
            let y = (row_rect.y + (row_rect.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }

        // Render-Order: die Spaltennamen ueber den Schaltern der Tabelle (s. render_order).
        for (spalte, name) in crate::render_order::COL_NAMES.iter().enumerate() {
            let rect = crate::render_order_col_rect(crate::RENDER_ORDER_HEAD_ROW, spalte);
            texts.push(gpu.centered_text(rect, i18n::tr(name).into_owned(), theme::text(), UI_SCALE_SMALL));
        }

        // Wertfelder der Stepper (Mesh-Budget/Chunk-Limiter/Threads folgen unten).
        // Render-Ahead ist keine frei tippbare Zahl: mit 0..4 ist der Bereich so klein, dass die
        // Pfeile schneller sind (s. EditableValue::ALL-Kommentar).
        let frame_latency = if set.frame_latency == 0 { "0 (Fence)".to_string() } else { format!("{}", set.frame_latency) };
        let rect = stepper_value_rect(FRAME_LATENCY_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, frame_latency, theme::text(), UI_SCALE_SMALL));

        // FPS-Limit: bei selbsttaktenden Present-Modi ehrlicher als eine Zahl, die
        // gerade nichts bewirkt -- dann auch nicht bearbeitbar (der Stepper blendet dort
        // ebenfalls ab, s. buttons()).
        if gpu.present_mode_self_paces() {
            let rect = stepper_value_rect(FPS_LIMIT_ROW);
            rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(rect, "durch VBlank".to_string(), theme::text_disabled(), UI_SCALE_SMALL));
        } else {
            let fps = fps_limit_label(set.fps_limit);
            self.draw_editable_value(gpu, stepper_value_rect(FPS_LIMIT_ROW), EditableValue::FpsLimit, fps, rects, texts);
        }

        // Mesh-Budget: anklickbar fuer einen eigenen Wert.
        let mesh = format!("{}", set.mesh_columns_per_frame);
        self.draw_editable_value(gpu, stepper_value_rect(MESH_BUDGET_ROW), EditableValue::MeshBudget, mesh, rects, texts);

        // Worker-Anzahl: bewusst NICHT frei tippbar -- der sinnvolle Bereich haengt an der
        // Kernzahl dieser Maschine, nicht am Geschmack (s. EditableValue::ALL).
        let threads = format!("{}", set.chunk_update_threads);
        let rect = crate::mip_sharpness_value_rect();
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let bias = format!("{:.2}", -f64::from(self.settings.mip_sharpness_pct) / 100.0);
        texts.push(gpu.centered_text(rect, bias, theme::text(), UI_SCALE_SMALL));
        let rect = stepper_value_rect(CHUNK_THREADS_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, threads, theme::text(), UI_SCALE_SMALL));

        // Abschnitt System: reine Information, kein Bedienelement -- breitere Felder als die
        // Stepper-Werte, GPU- und CPU-Namen sind lang ("AMD: AMD Radeon Graphics (...)" ~425px).
        for (row, wert) in self.system_rows(gpu) {
            // Mindestens 500, laengere Werte (Compositing samt Present-Lage) bekommen ihre Breite,
            // statt mit "..." abgeschnitten zu werden.
            let breite = (gpu.measure_text(&wert, UI_SCALE_SMALL) + 32.0).max(500.0);
            let feld = Rect { x: SETTINGS_CTRL_X, y: settings_row_y(row), w: breite, h: SETTINGS_CTRL_H };
            rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(feld, wert, theme::text(), UI_SCALE_SMALL));
        }
        // Riss-Timing: Wert in Millisekunden, mit Vorzeichen.
        let rect = stepper_value_rect(TEAR_TIMING_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, format!("{:+.2} ms", set.tear_timing_us as f32 / 1000.0), theme::text(), UI_SCALE_SMALL));
        // Riss-Reserve: ebenfalls in Millisekunden.
        let rect = stepper_value_rect(TEAR_RESERVE_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, format!("{:.2} ms", set.tear_reserve_us as f32 / 1000.0), theme::text(), UI_SCALE_SMALL));

        // TearFree zeigt seinen Wert direkt im Schalter selbst (AN/AUS/n.v./?), s.
        // tearfree_button() -- kein zusaetzliches Wertfeld noetig. Block-Rotation ist ein
        // Toggle-Button, s. buttons()/ToggleBlockRngRotation.
        // Ausserhalb von Linux gibt es keinen Schalter, nur den Hinweis -- als Feld wie die
        // System-Zeilen, nicht grau: graue Texte filtert build_menu_ui auf Einstellungsseiten weg.
        #[cfg(not(target_os = "linux"))]
        {
            let feld = Rect { x: SETTINGS_CTRL_X, y: settings_row_y(crate::TEARFREE_ROW), w: crate::STEPPER_VALUE_W, h: SETTINGS_CTRL_H };
            rects.push(UiRect { rect: feld, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            texts.push(gpu.centered_text(feld, i18n::tr("nur Linux und AMD").into_owned(), theme::text(), UI_SCALE_SMALL));
        }

    }

    // Generischer Renderer fuer eine Platzhalter-Kategorie (Video/Audio/Chat/
    // Barrierefreiheit): nur Beschriftung + abgeblendeter, nicht klickbarer Wert je Zeile.
    // Die eigentliche Erklaerung kommt als Hover-Tooltip (s. hovered_tooltip), nicht als
    // Dauertext -- deshalb hier keine weitere Beschriftung noetig.
    // `row_offset` laesst Platz fuer echte (nicht-Platzhalter-)Zeilen davor -- s.
    // build_settings_controls_tab, wo Maus-Empfindlichkeit Zeile 0 belegt und die
    // Platzhalter erst ab Zeile 1 folgen.
    // Video: Zeile 0 = FOV (echter Stepper, Wertfeld hier + Pfeil-Buttons aus buttons()),
    // Zeile 1 = Projektion (Umschalt-Button aus buttons(), hier nur das Label davor).
    pub(crate) fn build_settings_video_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        // Linke Spalte: Sichtfeld je Perspektive + Projektion. CMAA2 war hier zwischenzeitlich
        // geplant, ist aber deaktiviert -- s. CMAA2_ROW-Kommentar bei PROJECTION_ROW.
        let mut left: Vec<(usize, &str)> = Perspective::ALL.iter().map(|p| (p.fov_row(), p.fov_label())).collect();
        left.push((PROJECTION_ROW, "Projektion"));
        left.push((SMOOTH_LIGHTING_ROW, "Weiche Beleuchtung"));
        left.push((BLOCK_ROTATION_ROW, "Block-Rotation"));
        left.push((PARTICLES_ROW, "Eigene Partikel"));
        left.push((PARTICLES_OTHER_ROW, "Andere Partikel"));
        left.push((PARTICLE_DISTANCE_ROW, "Partikel-Distanz"));
        left.push((PARTICLE_FADE_ROW, "Partikel ausblenden"));
        left.push((ENTITY_NEAR_ROW, "Nahe Entities rastern"));
        left.push((ARMOR_TRIMS_ROW, "Rüstungsbesatz"));
        for (row, label) in left {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        // Rechte Spalte: Kameraabstand + -hoehe der beiden 3.-Person-Ansichten.
        for p in Perspective::THIRD_PERSON {
            for (row, label) in [(p.distance_row(), p.distance_label()), (p.y_offset_row(), p.y_offset_label())] {
                let Some(row) = row else { continue };
                let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
            }
        }
        // Staerke der weichen Beleuchtung (Pfeile aus buttons()); 0 % heisst aus.
        let rect = stepper_value_rect(SMOOTH_LIGHTING_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let staerke = match self.settings.smooth_lighting_percent {
            0 => "Aus".to_string(),
            p => format!("{p} %"),
        };
        texts.push(gpu.centered_text(rect, staerke, theme::text(), UI_SCALE_SMALL));
        let rect = stepper_value_rect(PARTICLE_DISTANCE_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, format!("{} Blöcke", self.settings.particle_distance), theme::text(), UI_SCALE_SMALL));
        let rect = stepper_value_rect(PARTICLE_FADE_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let dauer = match self.settings.particle_fade_out_ms {
            0 => "Aus".to_string(),
            ms => format!("{:.2} s", f64::from(ms) / 1000.0),
        };
        texts.push(gpu.centered_text(rect, dauer, theme::text(), UI_SCALE_SMALL));
        let rect = stepper_value_rect(ENTITY_NEAR_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let abstand = match self.settings.entity_near_dither_cm {
            0 => "Aus".to_string(),
            cm => format!("{:.1} Blöcke", f64::from(cm) / 100.0),
        };
        texts.push(gpu.centered_text(rect, abstand, theme::text(), UI_SCALE_SMALL));
        // FOV-Wertfelder (Pfeile kommen aus buttons()), Zahl ohne Nachkommastelle -- jedes
        // anklickbar fuer einen eigenen Wert (s. EditableValue::Fov), weil die 5er-Schritte
        // des Steppers z.B. eine 103 nie treffen. Die aktive Perspektive wird hervorgehoben,
        // sonst raet man, welche der drei Zeilen gerade wirkt.
        for p in Perspective::ALL {
            let fov = format!("{} °", self.settings.fov_of(p) as i32);
            self.draw_editable_value(gpu, stepper_value_rect(p.fov_row()), EditableValue::Fov(p), fov, rects, texts);
            // Kameraabstand und -hoehe in der rechten Spalte, in Bloecken mit Nachkomma-
            // stellen (die 0.5er- bzw. 0.25er-Schritte waeren als ganze Zahl nicht ablesbar).
            if let Some(row) = p.distance_row() {
                let d = format!("{:.1} Blöcke", self.settings.camera_distance_of(p));
                self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::CameraDistance(p), d, rects, texts);
            }
            if let Some(row) = p.y_offset_row() {
                let y = format!("{:+.2} Blöcke", self.settings.camera_y_offset_of(p));
                self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::CameraYOffset(p), y, rects, texts);
            }
        }
        // Schadens-Faerbung, rechte Spalte unter der Kamera-Gruppe: Farbfeld, Nur-Skin-Schalter
        // und Dauer.
        let y = (settings_row_y(DAMAGE_TINT_FIRST_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: "Schadensfarbe".to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        for (row, label) in [(DAMAGE_TINT_SKIN_ONLY_ROW, "Nur Skin färben"), (DAMAGE_TINT_TICKS_ROW, "Färbedauer")] {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        let rect = stepper_value_rect(DAMAGE_TINT_TICKS_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        texts.push(gpu.centered_text(rect, damage_tint_duration_text(self.settings.damage_tint_ticks), theme::text(), UI_SCALE_SMALL));

        self.build_video_glint_and_crit(gpu, rects, texts);

        // Sichtweiten: Render-Distance (anklickbar fuer einen eigenen Wert, das Maximum heisst
        // "unbegrenzt"), Entity- und Block-Entity-Weite.
        let set = &self.settings;
        for (row, label) in [
            (RENDER_DISTANCE_ROW, "Render-Distance"),
            (ENTITY_DISTANCE_ROW, "Entity-Renderdistanz"),
            (BLOCK_ENTITY_DISTANCE_ROW, "Block-Entity-Renderdistanz"),
            (CHUNK_FADE_ROW, "Chunk-Einblendung"),
        ] {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        // Kein "unbegrenzt": das Maximum ist die angeforderte Sichtweite, also eine echte Zahl.
        let weite = format!("{} Chunks", set.max_render_distance.min(MAX_RENDER_DISTANCE_MAX));
        self.draw_editable_value(gpu, stepper_value_rect(RENDER_DISTANCE_ROW), EditableValue::MaxRenderDistance, weite, rects, texts);
        let weite = set.entity_render_distance;
        let weite = if weite >= ENTITY_DISTANCE_MAX { "unbegrenzt".to_string() } else { format!("{weite} Blöcke") };
        self.draw_editable_value(gpu, stepper_value_rect(ENTITY_DISTANCE_ROW), EditableValue::EntityRenderDistance, weite, rects, texts);
        let weite = set.block_entity_render_distance;
        let weite = if weite >= BLOCK_ENTITY_DISTANCE_MAX { "unbegrenzt".to_string() } else { format!("{weite} Blöcke") };
        self.draw_editable_value(gpu, stepper_value_rect(BLOCK_ENTITY_DISTANCE_ROW), EditableValue::BlockEntityRenderDistance, weite, rects, texts);

        let rect = stepper_value_rect(CHUNK_FADE_ROW);
        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
        let dauer = match set.chunk_fade_ms {
            0 => "Aus".to_string(),
            ms => format!("{:.2} s", f64::from(ms) / 1000.0),
        };
        texts.push(gpu.centered_text(rect, dauer, theme::text(), UI_SCALE_SMALL));
    }

    // Steuerung: zwei Spalten (s. Layout-Konstanten am Dateianfang). Links die
    // Tastenbelegungs-Tabelle (Kopfzeile + scrollbarer Zeilenbereich, Buttons selbst kommen
    // aus buttons() -- gemeinsamer Klick-Zustand, s. dortigen Kommentar). Rechts
    // Maus-Empfindlichkeit (Zeile 0) und das Verhalten darunter.
    pub(crate) fn build_settings_controls_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let set = &self.settings;
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        for (row, &zeile) in controls_rows().iter().enumerate() {
            let label = match zeile {
                ControlsRow::SensX => "Kamera-Empfindlichkeit X",
                ControlsRow::SensY => "Kamera-Empfindlichkeit Y",
                ControlsRow::SensSplit => "X/Y getrennt",
                ControlsRow::SensCalc => "Empfindlichkeits-Rechner",
                ControlsRow::CursorSens => "Zeiger-Empfindlichkeit",
                ControlsRow::CursorSize => "Zeigergröße",
                ControlsRow::CursorWidth => "Zeigerbreite",
                ControlsRow::CursorOutline => "Outline-Dicke",
                ControlsRow::CursorFill => "Transparenz innen",
                ControlsRow::CursorCircle => "Zeiger-Kreis",
                ControlsRow::SneakProtection => "Sneak-Kantenschutz",
                ControlsRow::SnapTap => "SnapTap (SOCD)",
                ControlsRow::JumpCooldown => "Sprung-Abklingzeit",
                ControlsRow::SprintOnStart => "Sprinten an bei Spielstart",
                ControlsRow::HoldUseRepeat => "Benutzen wiederholen",
                ControlsRow::RocketRepeat => "Raketen wiederholen",
                ControlsRow::ExtinguishDcGhost => "No Extinguish DC Ghost",
                ControlsRow::UseWhileMining => "Benutzen beim Abbauen",
                ControlsRow::CapsLock => "Feststelltaste sperren",
                ControlsRow::HoldConfirm => "Bestätigungsdauer (allgemein)",
                ControlsRow::ReconnectHold => "Reconnect-Verzögerung",
                ControlsRow::Bind(aktion) => aktion.short_label(),
            };
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
            // Wertfelder der Empfindlichkeiten (anklickbar fuer eine eigene Zahl, s.
            // EditableValue) und der beiden Zeiger-Regler.
            match zeile {
                ControlsRow::SensX => {
                    self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::MouseSensitivity, format_sensitivity(set.mouse_sensitivity), rects, texts);
                }
                ControlsRow::SensY => {
                    self.draw_editable_value(gpu, stepper_value_rect(row), EditableValue::MouseSensitivityY, format_sensitivity(set.mouse_sensitivity_y), rects, texts);
                }
                ControlsRow::CursorSens => self.draw_editable_value(
                    gpu,
                    stepper_value_rect(row),
                    EditableValue::CursorSensitivity,
                    format_cursor_sensitivity(set.cursor_sensitivity),
                    rects,
                    texts,
                ),
                ControlsRow::CursorSize | ControlsRow::CursorWidth | ControlsRow::CursorOutline | ControlsRow::CursorFill => {
                    let wert = match zeile {
                        ControlsRow::CursorSize => set.cursor_size,
                        ControlsRow::CursorWidth => set.cursor_width,
                        ControlsRow::CursorOutline => set.cursor_outline,
                        _ => set.cursor_fill_transparency,
                    };
                    let rect = stepper_value_rect(row);
                    rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
                }
                ControlsRow::HoldConfirm | ControlsRow::ReconnectHold => {
                    let wert = if zeile == ControlsRow::HoldConfirm { set.hold_confirm_ms as i32 } else { set.reconnect_hold_ms };
                    let text = if wert < 0 { "Allgemein".to_string() } else { format!("{wert} ms") };
                    let rect = stepper_value_rect(row);
                    rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(rect, text, theme::text(), UI_SCALE_SMALL));
                }
                _ => {}
            }
        }
    }

    // Inventar-Tab: bisher genau eine echte Einstellung.
    // Chat-Tab: Beschriftungen und Wertfelder der sieben echten Zeilen (Pfeile aus buttons()),
    // darunter die Platzhalter.
    // Beschriftung und Wert der Chat-Einstellungen, Reihenfolge wie CHAT_TOOLTIPS (im
    // Einstellungsfenster des Chats im HUD-Editor, s. build_hud_config).
    pub(crate) fn chat_setting_rows(&self) -> [(&'static str, String); 9] {
        let set = &self.settings;
        [
            ("Textgröße", format!("{} %", set.chat_scale_percent)),
            ("Breite", format!("{}", set.chat_width)),
            ("Hintergrund", format!("{} %", set.chat_background)),
            ("Abdunkeln beim Tippen", format!("{} %", set.chat_open_dim)),
            ("Zeilen (im Spiel)", format!("{}", set.chat_lines)),
            ("Zeilen (beim Tippen)", format!("{}", set.chat_lines_open)),
            ("Ausblenden nach", if set.chat_fade_secs == 0 { "Nie".to_string() } else { format!("{} s", set.chat_fade_secs) }),
            ("Zeilenabstand", format!("{} %", set.chat_line_spacing)),
            ("Darstellung", String::new()),
        ]
    }

    // Was alle Seiten mit settings_page gemeinsam haben: Linie unter den Reitern, Trennlinie
    // mit Namen vor jedem weiteren Abschnitt, Scrollleiste am rechten Rand, sobald nicht alle
    // Zeilen ins Fenster passen. Die Reiter selbst sind Knoepfe (s. section_tabs).
    pub(crate) fn build_settings_page_frame(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let Some(page) = self.screen.settings_page().filter(|_| self.mode == AppMode::Menu) else { return };
        let (screen_w, screen_h) = gpu.size();
        // Rechte Kante des Inhalts, wie fit_texts in build_menu_ui.
        let rechts = screen_w - self.side_strip_w(screen_w) - SETTINGS_CONTENT_DX - 16.0;
        let max_scroll = self.page_max_scroll();
        let linie_bis = if max_scroll > 0 { rechts - 18.0 } else { rechts };
        let linie = theme::get(theme::Slot::Border1);
        if page.sections.len() >= 2 {
            rects.push(UiRect { rect: Rect { x: SETTINGS_LABEL_X, y: SETTINGS_FIRST_Y - 5.0, w: linie_bis - SETTINGS_LABEL_X, h: 1.0 }, color: linie, ..UiRect::FLAT });
        }
        // Im Schrift-Tab erklaeren sich "Standard" und "Ausnahmen" schon aus den Reitern und
        // der Tabelle; rechts stuende der Name nur im Weg.
        let ohne_namen: &[&str] = if self.screen == MenuScreen::SettingsFont { &["Standard", "Ausnahmen"] } else { &[] };
        // Der Name steht rechts UNTER seiner Linie, im Abschnitt (Nutzer 2026-11-11).
        let name_ueber = |name: &str, linie_y: f32, texts: &mut Vec<UiText>| {
            if ohne_namen.contains(&name) {
                return;
            }
            let x = linie_bis - gpu.measure_text(name, UI_SCALE_SMALL);
            texts.push(UiText { text: name.to_string(), x: x.round(), y: (linie_y + 4.0).round(), color: theme::text(), scale: UI_SCALE_SMALL });
        };
        // Gleitend gescrollt liegt die Linie eines Abschnitts irgendwo zwischen den Zeilen: ist
        // sie ueber die Linie oben gewandert, steht sein Name oben, sonst an seiner eigenen Linie
        // -- dieselbe Grenze fuer beides, sonst stand er beim Scrollen doppelt da. Der erste
        // Abschnitt hat keine eigene Linie.
        // Eine Zeile ausserhalb des Fensters hat kein y (s. settings_row_y): dann entscheidet,
        // ob sie oben oder unten heraussteht.
        let oben = SETTINGS_FIRST_Y - 5.0;
        let erste = self.page_scroll().stand;
        let mut oben_name = page.sections.first().map(|&(name, _)| name);
        for &(name, row) in page.sections.iter().skip(1) {
            let y = settings_row_y(row);
            if y < 0.0 {
                if row as f32 <= erste {
                    oben_name = Some(name);
                }
                continue;
            }
            let linie_y = (y - f32::midpoint(SETTINGS_ROW_H - SETTINGS_CTRL_H, SETTINGS_SECTION_GAP)).round();
            if linie_y <= oben {
                oben_name = Some(name);
                continue;
            }
            rects.push(UiRect { rect: Rect { x: SETTINGS_LABEL_X, y: linie_y, w: linie_bis - SETTINGS_LABEL_X, h: 1.0 }, color: linie, ..UiRect::FLAT });
            name_ueber(name, linie_y, texts);
        }
        if page.sections.len() >= 2 {
            if let Some(name) = oben_name {
                name_ueber(name, oben, texts);
            }
        }
        if max_scroll > 0 {
            let hoehe = settings_rows_bottom(screen_h) - SETTINGS_FIRST_Y - (SETTINGS_ROW_H - SETTINGS_CTRL_H);
            let spur = Rect { x: rechts - 6.0, y: SETTINGS_FIRST_Y, w: 6.0, h: hoehe };
            rects.push(UiRect { rect: spur, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            let griff_h = (hoehe * (page.rows - max_scroll) as f32 / page.rows as f32).max(24.0);
            let anteil = (erste / max_scroll as f32).clamp(0.0, 1.0);
            let griff = Rect { x: spur.x, y: spur.y + (hoehe - griff_h) * anteil, w: spur.w, h: griff_h };
            rects.push(UiRect { rect: griff, color: theme::text_disabled(), ..UiRect::FLAT });
        }
    }
}
