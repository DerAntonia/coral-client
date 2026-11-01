// Die Buttons jedes Menue-Bildschirms.

use super::*;

impl App {
    pub(crate) fn screen_buttons(&self) -> Vec<Button> {
        self.sync_settings_scroll();
        let mut out: Vec<Button> = Vec::new();
        // Fuer alles, was mittig steht (Start, Pause, Tod, Einstellungs-Kacheln).
        let screen_w = self.gpu.as_ref().map_or(1280.0, |g| g.size().0);

        match self.screen {
            MenuScreen::Main => self.buttons_main(&mut out, screen_w),
            MenuScreen::Connect => {
                // Nur die Server-Karten bzw. das Formular; Quick Connect und Bearbeiten
                // liegen in der Leiste unten, das Konto in der Leiste oben.
                self.server_panel_buttons(&mut out);
                self.recordings_buttons(&mut out);
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
            MenuScreen::SettingsSearch => out.extend(self.search_hit_buttons()),
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
            MenuScreen::SettingsGeneral => self.buttons_settings_general(&mut out),
            MenuScreen::SettingsVideo => self.buttons_settings_video(&mut out),
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
            MenuScreen::SettingsDesign => self.buttons_settings_design(&mut out),
            MenuScreen::SettingsFont => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                // Schrift und Schatten sind Kaestchen zum Hineinziehen (s. FontSlot).
                let mut stabil = Self::toggle(ButtonAction::ToggleStableNumbers, STABLE_NUMBERS_ROW, self.settings.stable_numbers);
                stabil.rect.x = FONT_CTRL_X;
                out.push(stabil);
                let mut gross = Self::toggle(ButtonAction::ToggleCapitals, CAPITALS_ROW, self.settings.font_capitals);
                gross.rect.x = FONT_CTRL_X;
                out.push(gross);
                // Neben jedem Vanilla-Schatten: 1 oder 1/2 Pixel Versatz.
                for i in 0..=TEXT_AREA_COUNT {
                    if self.settings.shadow_slot_style(i) == Some(TextShadow::Vanilla) {
                        let halb = self.settings.shadow_half[i];
                        let a = ButtonAction::ToggleShadowHalf(i);
                        out.extend(pick_buttons(shadow_half_rect(i), &[("1 px", a, !halb), ("½ px", a, halb)], true));
                    }
                }
            }
            MenuScreen::SettingsShader => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let p = self.settings.shader_scattering_percent;
                out.push(Self::step_slider(ButtonAction::ScatteringPrev, ButtonAction::ScatteringNext, stepper_value_rect(0), f64::from(p), 0.0, f64::from(SCATTERING_MAX), true));
                let g = self.settings.shader_sun_size_tenths;
                out.push(Self::step_slider(ButtonAction::SunSizePrev, ButtonAction::SunSizeNext, stepper_value_rect(1), f64::from(g), 0.0, f64::from(SUN_SIZE_MAX_TENTHS), true));
                out.push(Self::toggle(ButtonAction::ToggleBlockShadows, 2, self.settings.shader_block_shadows));
                let b = self.settings.shader_block_softness_deg;
                let an = self.settings.shader_block_shadows;
                out.push(Self::step_slider(ButtonAction::BlockSoftnessPrev, ButtonAction::BlockSoftnessNext, stepper_value_rect(3), f64::from(b), 0.0, f64::from(BLOCK_SOFTNESS_MAX_DEG), an));
                out.push(Self::nav_button(
                    ButtonAction::OpenLightLevels,
                    Rect { x: SETTINGS_CTRL_X, y: settings_row_y(4), w: STEPPER_VALUE_W + 2.0 * (STEPPER_BTN_W + STEPPER_GAP), h: SETTINGS_CTRL_H },
                    "Bearbeiten",
                    UI_SCALE_SMALL,
                ));
                let d = self.settings.shader_lightmap_debug;
                out.extend(Self::stepper(ButtonAction::LightmapDebugPrev, ButtonAction::LightmapDebugNext, 5, d > 0, (d as usize) + 1 < LIGHTMAP_DEBUG_NAMES.len()));
            }
            MenuScreen::SettingsParticles => self.buttons_settings_particles(&mut out),
            MenuScreen::SettingsLightLevels => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
                let treffer = self.light_search_hits();
                for (k, y) in header_list_rows(treffer.len(), self.light_scroll.stand, self.light_visible_rows()) {
                    let i = treffer[k];
                    let p = self.settings.light_factor_of(light_source_blocks()[i]);
                    out.push(Self::soft_end(Self::step_slider(ButtonAction::LightFactorPrev(i), ButtonAction::LightFactorNext(i), Rect { y, ..stepper_value_rect(0) }, f64::from(p), 0.0, f64::from(LIGHT_FACTOR_MAX), true), 200.0));
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
                out.push(Self::swatch(ColorTarget::StackCount(false), settings_ctrl_rect(5), self.settings.stack_count_swatch(false)));
                out.push(Self::toggle(ButtonAction::TogglePotionMarkStrong, 6, self.settings.potion_mark_strong));
                out.push(Self::toggle(ButtonAction::TogglePotionMarkLong, 7, self.settings.potion_mark_long));
                out.push(Self::toggle(ButtonAction::TogglePotionMarkWeak, 8, self.settings.potion_mark_weak));
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
            MenuScreen::SettingsAudio => self.buttons_settings_audio(&mut out),
            MenuScreen::SettingsConfigs => self.buttons_settings_configs(&mut out),
            // Skin-Tab: die Kacheln zeichnen und behandeln sich selbst (s. skin_tile_press),
            // die Knoepfe im Rand rechts stehen bei chrome_buttons.
            MenuScreen::SettingsSkin => {
                out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
            }
            MenuScreen::SettingsControls => self.buttons_settings_controls(&mut out),
            MenuScreen::HudEditor => self.buttons_hud_editor(&mut out),
            MenuScreen::Death => {
                out.push(Self::nav_button(ButtonAction::Respawn, pause_button(0, screen_w), "Respawn", UI_SCALE_NORMAL));
                out.push(Self::nav_button(ButtonAction::Disconnect, pause_button(1, screen_w), "Disconnect", UI_SCALE_NORMAL));
            }
        }
        out
    }

    fn buttons_main(&self, out: &mut Vec<Button>, screen_w: f32) {
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

    fn buttons_settings_general(&self, out: &mut Vec<Button>) {
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
        let fps_limit_wirksam = !self.gpu.as_ref().is_some_and(super::super::super::GpuState::present_mode_self_paces);
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
        out.push(Self::step_slider(ButtonAction::TearTimingDown, ButtonAction::TearTimingUp, stepper_value_rect(TEAR_TIMING_ROW), f64::from(set.tear_timing_us), f64::from(TEAR_TIMING_MIN_US), f64::from(TEAR_TIMING_MAX_US), true));
        out.push(Self::step_slider(ButtonAction::TearReserveDown, ButtonAction::TearReserveUp, stepper_value_rect(TEAR_RESERVE_ROW), f64::from(set.tear_reserve_us), f64::from(TEAR_RESERVE_MIN_US), f64::from(TEAR_RESERVE_MAX_US), true));
        // CMAA2-Umschalter deaktiviert, s. CMAA2_ROW-Kommentar.
        // out.push(Self::toggle(ButtonAction::ToggleCmaa2, CMAA2_ROW, set.cmaa2_enabled));
    }

    fn buttons_settings_video(&self, out: &mut Vec<Button>) {
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
        out.push(Self::step_slider(ButtonAction::SmoothLightingDown, ButtonAction::SmoothLightingUp, stepper_value_rect(SMOOTH_LIGHTING_ROW), f64::from(set.smooth_lighting_percent), 0.0, f64::from(SMOOTH_LIGHTING_MAX), true));
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
        out.push(Self::step_slider(ButtonAction::DamageTintTicksPrev, ButtonAction::DamageTintTicksNext, stepper_value_rect(DAMAGE_TINT_TICKS_ROW), f64::from(set.damage_tint_ticks), f64::from(DAMAGE_TINT_TICKS_MIN), f64::from(DAMAGE_TINT_TICKS_MAX), true));
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
        out.push(Self::toggle(ButtonAction::ToggleViewModel, VIEW_MODEL_ROW, set.modified_view_model));
        for (i, (bit, _)) in world::more_anims::FEATURES.into_iter().enumerate() {
            out.push(Self::toggle(ButtonAction::ToggleAnimFlag(bit), ANIM_FIRST_ROW + i, set.anim_flags & bit != 0));
        }
        out.extend(Self::stepper(
            ButtonAction::MaxRenderDistanceDown,
            ButtonAction::MaxRenderDistanceUp,
            RENDER_DISTANCE_ROW,
            set.max_render_distance > MAX_RENDER_DISTANCE_MIN,
            set.max_render_distance < MAX_RENDER_DISTANCE_MAX,
        ));
        out.push(Self::step_slider(ButtonAction::ChunkSaverThresholdPrev, ButtonAction::ChunkSaverThresholdNext, stepper_value_rect(LOD_DETAIL_ROW), f64::from(set.chunk_saver_threshold), f64::from(lod::THRESHOLD_MIN), f64::from(lod::THRESHOLD_MAX), true));
        // Crit-Farbe: Schalter, rechts daneben das Farbfeld (dieselben vier Achsen wie
        // die Schadens-Faerbung).
        out.push(Self::toggle(ButtonAction::ToggleHurtCam, HURT_CAM_ROW, set.hurt_cam));
        out.push(Self::toggle(ButtonAction::ToggleShoulderCam, SHOULDER_ROW, set.shoulder_cam));
        out.push(Self::step_slider(
            ButtonAction::ShoulderOffsetPrev,
            ButtonAction::ShoulderOffsetNext,
            stepper_value_rect(SHOULDER_OFFSET_ROW),
            f64::from(set.shoulder_offset_cm),
            f64::from(SHOULDER_OFFSET_MIN),
            f64::from(SHOULDER_OFFSET_MAX),
            set.shoulder_cam,
        ));
        out.push(Self::toggle(ButtonAction::ToggleShoulderCrosshair, SHOULDER_CROSSHAIR_ROW, set.shoulder_crosshair));
        out.push(Self::toggle(ButtonAction::ToggleShoulderFade, SHOULDER_FADE_ROW, set.shoulder_fade));
        out.push(Self::toggle(ButtonAction::ToggleShoulderDecoupled, SHOULDER_DECOUPLED_ROW, set.shoulder_decoupled));
        out.push(Self::step_slider(
            ButtonAction::HurtCamStrengthPrev,
            ButtonAction::HurtCamStrengthNext,
            stepper_value_rect(HURT_CAM_STRENGTH_ROW),
            f64::from(set.hurt_cam_strength),
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
                f64::from(set.fire_height),
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

    fn buttons_settings_design(&self, out: &mut Vec<Button>) {
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

    fn buttons_settings_particles(&self, out: &mut Vec<Button>) {
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
                    f64::from(wert),
                    f64::from(min),
                    f64::from(max),
                    true,
                );
                out.push(Self::slider_with_track(b, PARTICLE_TRACK_W));
            }
            if art.has_all_at_once() {
                out.push(schalter(ButtonAction::ParticleAllAtOnce(i), particle_column_x(PARTICLE_ALL_AT_ONCE_COL), y, t.all_at_once));
            }
        }
    }

    fn buttons_settings_audio(&self, out: &mut Vec<Button>) {
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
        out.push(Self::step_slider(ButtonAction::UiPitchDown, ButtonAction::UiPitchUp, stepper_value_rect(AUDIO_PITCH_ROW), f64::from(self.settings.ui_pitch_semitones), -24.0, 24.0, true));
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

    fn buttons_settings_configs(&self, out: &mut Vec<Button>) {
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

    fn buttons_settings_controls(&self, out: &mut Vec<Button>) {
        out.push(Self::nav_button(ButtonAction::Back, BACK_BUTTON, "← Zurück", UI_SCALE_SMALL));
        // Fenster "Erweitert" offen: nur seine Knoepfe (s. keybind_advanced_buttons).
        if let Some(aktion) = self.keybind_advanced {
            let (dx, dy) = self.content_offset();
            out.extend(self.keybind_advanced_buttons(aktion).into_iter().map(|mut b| {
                b.rect.x -= dx;
                b.rect.y -= dy;
                b
            }));
            return;
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
                ControlsRow::CursorSize => out.push(Self::soft_end(Self::step_slider(
                    ButtonAction::CursorSizeDown,
                    ButtonAction::CursorSizeUp,
                    stepper_value_rect(row),
                    f64::from(set.cursor_size),
                    f64::from(CURSOR_SIZE_MIN),
                    f64::from(CURSOR_SIZE_MAX),
                    true,
                ), 200.0)),
                ControlsRow::HoldConfirm => out.push(Self::soft_end(Self::step_slider(
                    ButtonAction::HoldConfirmDown,
                    ButtonAction::HoldConfirmUp,
                    stepper_value_rect(row),
                    f64::from(set.hold_confirm_ms),
                    0.0,
                    f64::from(HOLD_CONFIRM_MAX),
                    true,
                ), 1000.0)),
                ControlsRow::ReconnectHold => out.push(Self::soft_end(Self::step_slider(
                    ButtonAction::ReconnectHoldDown,
                    ButtonAction::ReconnectHoldUp,
                    stepper_value_rect(row),
                    f64::from(set.reconnect_hold_ms),
                    f64::from(RECONNECT_HOLD_GENERAL),
                    f64::from(HOLD_CONFIRM_MAX),
                    true,
                ), 1000.0)),
                ControlsRow::CursorWidth => out.push(Self::step_slider(
                    ButtonAction::CursorWidthDown,
                    ButtonAction::CursorWidthUp,
                    stepper_value_rect(row),
                    f64::from(set.cursor_width),
                    f64::from(CURSOR_WIDTH_MIN),
                    f64::from(CURSOR_WIDTH_MAX),
                    true,
                )),
                ControlsRow::CursorCircle => out.push(Self::toggle(ButtonAction::ToggleCursorCircle, row, set.cursor_circle)),
                ControlsRow::SneakProtection => out.push(Self::toggle(ButtonAction::ToggleSneakEdgeProtection, row, set.sneak_edge_protection_enabled)),
                ControlsRow::SnapTap => out.push(Self::toggle(ButtonAction::ToggleSnapTap, row, set.snap_tap_enabled)),
                ControlsRow::JumpCooldown => out.push(Self::toggle(ButtonAction::ToggleJumpCooldown, row, set.jump_cooldown_enabled)),
                ControlsRow::SprintOnStart => out.push(Self::toggle(ButtonAction::ToggleSprintOnStart, row, set.sprint_on_start)),
                ControlsRow::HoldUseRepeat => {
                    out.push(Self::nav_button(ButtonAction::CycleHoldUseRepeat, settings_wide_rect(row), set.hold_use_repeat.label(), UI_SCALE_SMALL));
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

    fn buttons_hud_editor(&self, out: &mut Vec<Button>) {
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
            return;
        }
        // Ohne offenes Einstellungsfenster keine Knoepfe -- nur die Elemente zum Ziehen.
        let Some(element) = self.hud_selected else { return };
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
                            out.push(Self::step_slider(ButtonAction::CrosshairBoostPrev, ButtonAction::CrosshairBoostNext, stepper_value_rect(3), f64::from(boost), 0.0, 100.0, true));
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
                                f64::from(self.settings.crosshair_size()),
                                f64::from(min),
                                f64::from(max),
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
                        out.push(Self::swatch(ColorTarget::StackCount(true), settings_ctrl_rect(4), self.settings.stack_count_swatch(true)));
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
                            out.push(Self::step_slider(prev, next, stepper_value_rect(row), f64::from(wert), f64::from(min), f64::from(max), true));
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
                        out.push(Self::soft_end(Self::step_slider(
                            ButtonAction::HudScaleStep(element, false),
                            ButtonAction::HudScaleStep(element, true),
                            stepper_value_rect(0),
                            f64::from(set.hud_keys_scale),
                            f64::from(HUD_SCALE_PCT_MIN),
                            f64::from(HUD_SCALE_PCT_MAX),
                            true,
                        ), 200.0));
                        out.push(Self::step_slider(
                            ButtonAction::KeysOpacityPrev,
                            ButtonAction::KeysOpacityNext,
                            stepper_value_rect(1),
                            f64::from(set.keys_opacity),
                            0.0,
                            100.0,
                            true,
                        ));
                    }
                    HudElement::Session => {
                        let set = &self.settings;
                        out.push(Self::soft_end(Self::step_slider(
                            ButtonAction::HudScaleStep(element, false),
                            ButtonAction::HudScaleStep(element, true),
                            stepper_value_rect(0),
                            f64::from(set.hud_session_scale),
                            f64::from(HUD_SCALE_PCT_MIN),
                            f64::from(HUD_SCALE_PCT_MAX),
                            true,
                        ), 200.0));
                        out.push(Self::step_slider(
                            ButtonAction::SessionOpacity(false),
                            ButtonAction::SessionOpacity(true),
                            stepper_value_rect(1),
                            f64::from(set.session_opacity),
                            0.0,
                            100.0,
                            true,
                        ));
                    }
                    HudElement::Armor => {
                        let set = &self.settings;
                        out.push(Self::soft_end(Self::step_slider(
                            ButtonAction::HudScaleStep(element, false),
                            ButtonAction::HudScaleStep(element, true),
                            stepper_value_rect(0),
                            f64::from(set.hud_armor_scale),
                            f64::from(HUD_SCALE_PCT_MIN),
                            f64::from(HUD_SCALE_PCT_MAX),
                            true,
                        ), 200.0));
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
                            f64::from(set.armor_hud_gap),
                            f64::from(armor_hud::GAP_MIN),
                            f64::from(armor_hud::GAP_MAX),
                            true,
                        ));
                        out.push(Self::soft_end(Self::step_slider(
                            ButtonAction::ArmorHudNumberScalePrev,
                            ButtonAction::ArmorHudNumberScaleNext,
                            stepper_value_rect(4),
                            f64::from(set.armor_hud_number_scale),
                            f64::from(armor_hud::NUMBER_SCALE_MIN),
                            f64::from(armor_hud::NUMBER_SCALE_MAX),
                            true,
                        ), 200.0));
                        for (row, wert, (weniger, mehr)) in [
                            (5, set.armor_hud_number_x, (ButtonAction::ArmorHudNumberXPrev, ButtonAction::ArmorHudNumberXNext)),
                            (6, set.armor_hud_number_y, (ButtonAction::ArmorHudNumberYPrev, ButtonAction::ArmorHudNumberYNext)),
                        ] {
                            out.push(Self::step_slider(
                                weniger,
                                mehr,
                                stepper_value_rect(row),
                                f64::from(wert),
                                f64::from(armor_hud::NUMBER_OFFSET_MIN),
                                f64::from(armor_hud::NUMBER_OFFSET_MAX),
                                true,
                            ));
                        }
                        out.push(Self::toggle(ButtonAction::ToggleArmorHudGreen, 7, set.armor_hud_green));
                        out.push(Self::toggle(ButtonAction::ToggleArmorHudRing, 8, set.armor_hud_ring));
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
                        out.push(Self::soft_end(Self::step_slider(
                            ButtonAction::HudScaleStep(element, false),
                            ButtonAction::HudScaleStep(element, true),
                            stepper_value_rect(0),
                            f64::from(pct),
                            f64::from(HUD_SCALE_PCT_MIN),
                            f64::from(HUD_SCALE_PCT_MAX),
                            true,
                        ), 200.0));
                        out.push(Self::toggle(ButtonAction::ToggleArrowNumberOnly, 1, set.arrow_number_only));
                        out.push(Self::step_slider(
                            ButtonAction::ArrowOpacityPrev,
                            ButtonAction::ArrowOpacityNext,
                            stepper_value_rect(2),
                            f64::from(set.arrow_opacity),
                            f64::from(ARROW_OPACITY_MIN),
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
}
