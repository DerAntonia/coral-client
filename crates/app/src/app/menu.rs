// Menues: Bedienelemente, Serverliste, Knoepfe und ihre Aktionen, Farbwaehler, Aufklapplisten.

use crate::{theme, App, ButtonAction, Rect, Button, StepSlider, SLIDER_TRACK_W, slider_rect_of, SERVER_CARD_MARKER, UI_SCALE_SMALL, ColorTarget, SWATCH_W, SETTINGS_CTRL_X, settings_row_y, TOGGLE_W, SETTINGS_CTRL_H, stepper_value_rect, STEPPER_BTN_W, STEPPER_GAP, Instant, scroll_inertia_rate, DROPDOWN_MAX_ITEMS, AppMode, MenuScreen, skin_tab, SECTION_SLIDE_RATE, server_status, UNKNOWN_SERVER_ICON_SLOT, server_rows_visible, server_card_rect, server_list_top, server_list_bottom, server_rows_fit, text_field, SERVER_SEARCH_MAX_CHARS, ui_sound, ui_sound_for_action, server_panel, SERVER_BAR_GAP, SERVER_BAR_W, SERVER_BAR_HIT, SERVER_DRAG_START_PX, server_row_h, SERVER_ROW_GAP, ServerDrag, ServerContextMenu, ServerEdit, MAX_SERVER_NAME_LEN, MAX_ADDRESS_LEN, ServerField, server_dialog_layout, UI_SCALE_NORMAL, MULTI_CLICK_MS, MULTI_CLICK_PX, server_dialog_view_buttons, server_dialog_name_toggle, ui_screen, SERVER_CONTEXT_ROW_H, SERVER_CONTEXT_W, ServerEntry, save_servers, ConnStatus, pause_button, pause_button_around_center, BACK_BUTTON, account_row_rect, InputKind, account_row_rect_col2, ACCOUNT_INPUT_OK, SETTINGS_CATEGORIES, i18n, settings_hub_button, TEARING_TEST_ROW, FRAME_LATENCY_ROW, FRAME_LATENCY_MIN, FRAME_LATENCY_MAX, FPS_LIMIT_ROW, fps_limit_step, MESH_BUDGET_ROW, MESH_COLUMNS_MAX, CHUNK_THREADS_ROW, CHUNK_UPDATE_THREADS_MIN, CHUNK_UPDATE_THREADS_MAX, TEARFREE_ROW, CAVE_CULLING_ROW, ENTITY_CULLING_ROW, ENTITY_CULLING_NAMES, MIPMAPPING_ROW, MOTION_BLUR_ROW, MOTION_BLUR_MAX, MOTION_BLUR_STEP, MB_WORLD_ROW, MB_ENTITIES_ROW, MB_HAND_ROW, MB_HAND_CURVE_ROW, HAND_BLUR_MAX, HAND_BLUR_STEP, HAND_BLUR_CURVES, MB_WORLD_TOOLTIP, MB_ENTITIES_TOOLTIP, MB_HAND_TOOLTIP, MB_HAND_CURVE_TOOLTIP, TEAR_POSITION_ROW, TEAR_TIMING_ROW, TEAR_TIMING_MIN_US, TEAR_TIMING_MAX_US, TEAR_RESERVE_ROW, TEAR_RESERVE_MIN_US, TEAR_RESERVE_MAX_US, Perspective, FOV_MIN, FOV_MAX, CAMERA_DISTANCE_MIN, CAMERA_DISTANCE_MAX, skip_perspective_rect, CAMERA_Y_OFFSET_MIN, CAMERA_Y_OFFSET_MAX, SMOOTH_LIGHTING_ROW, SMOOTH_LIGHTING_MAX, PARTICLES_ROW, PARTICLES_OTHER_ROW, PARTICLE_MODE_NAMES, particles, PROJECTION_ROW, PROJECTION_NAMES, settings_ctrl_rect, DAMAGE_TINT_FIRST_ROW, DAMAGE_TINT_SKIN_ONLY_ROW, DAMAGE_TINT_TICKS_ROW, DAMAGE_TINT_TICKS_MIN, DAMAGE_TINT_TICKS_MAX, GLINT_SPEED_ROW, GLINT_STRENGTH_ROW, PLAYER_LIST_SCALE_ROW, HUD_SCALE_PCT_MIN, HUD_SCALE_PCT_MAX, ENTITY_DISTANCE_ROW, ENTITY_DISTANCE_MIN, ENTITY_DISTANCE_MAX, BLOCK_ENTITY_DISTANCE_ROW, BLOCK_ENTITY_DISTANCE_MIN, BLOCK_ENTITY_DISTANCE_MAX, BLOCK_ROTATION_ROW, ARMOR_TRIMS_ROW, VIEW_MODEL_ROW, ANIM_ROW, RENDER_DISTANCE_ROW, MAX_RENDER_DISTANCE_MIN, MAX_RENDER_DISTANCE_MAX, LOD_DETAIL_ROW, lod, HURT_CAM_ROW, SHOULDER_ROW, SHOULDER_OFFSET_ROW, SHOULDER_OFFSET_MIN, SHOULDER_OFFSET_MAX, SHOULDER_CROSSHAIR_ROW, SHOULDER_FADE_ROW, SHOULDER_DECOUPLED_ROW, HURT_CAM_STRENGTH_ROW, DEATH_ANIM_ROW, DEATH_ANIM_NAMES, FIRE_MODE_NAMES, pick_buttons, settings_wide_rect, FIRE_MODE_ROW, FIRE_MODE_CROSSHAIR, FIRE_DETAIL_ROW, FIRE_MODE_VANILLA, CRIT_COLOR_ROW, swatch_after_toggle_rect, SENS_CALC_RESULT_ROW, LANGUAGE_ROWS, LATIN_MACRONS_ROW, LOANWORDS_ROW, wallpaper, WALLPAPER_ROW, WALLPAPER_PICK_X, MENU_BACKDROP_ROW, SCROLL_INERTIA_ROW, SCROLL_DISTANCE_ROW, SCROLL_DISTANCE_MIN, SCROLL_DISTANCE_MAX, theme_tile_rect, ThemeChoice, theme_slot_cell, THEME_COLOR_SWATCH_DX, STABLE_NUMBERS_ROW, FONT_CTRL_X, CAPITALS_ROW, TEXT_AREA_COUNT, TextShadow, shadow_half_rect, STEPPER_VALUE_W, PARTICLE_TOGGLE_W, header_list_rows_smooth, PARTICLE_TOGGLE_X, PARTICLE_LIFETIME_STEPS, PARTICLE_OPACITY_STEP, PARTICLE_SPEED_STEPS, PARTICLE_AMOUNT_STEPS, PARTICLE_INTERVAL_STEPS, particle_value_rect, PARTICLE_TRACK_W, particle_column_x, PARTICLE_ALL_AT_ONCE_COL, SERVER_OPEN_NAMES, SERVER_OPEN_ROW, SERVER_OPEN_BOOK, SERVER_OPEN_SIGN, SERVER_OPEN_ALLOW, SERVER_OPEN_IGNORE, CONTAINER_GROUPS, SERVER_CLOSE_GROUP_ROW, SERVER_CLOSE_SIGN_ROW, SERVER_SCREENS_ROWS, SLOT_HOVER_FADE_ROW, SLOT_HOVER_COLOR_ROW, SWAP_ANIM_ROW, SLOT_HOVER_FADE_MAX, SLOT_HOVER_FADE_STEP, SWAP_ANIM_MAX, SWAP_ANIM_STEP, HOTBAR_GLIDE_ROW, HOTBAR_GLIDE_MAX, HOTBAR_GLIDE_STEP, SLOT_HOVER_FADE_TOOLTIP, SLOT_HOVER_COLOR_TOOLTIP, SWAP_ANIM_TOOLTIP, SWAP_ANIM_TOTEM_TOOLTIP, DOLL_BACKDROP_TOOLTIP, SWAP_ANIM_TOTEM_ROW, SWAP_ANIM_HEALING_ROW, SWAP_ANIM_HEALING_TOOLTIP, DOLL_BACKDROP_ROW, SERVER_SCREENS_TOOLTIP, SERVER_CLOSE_INVENTORY_TOOLTIP, SERVER_CLOSE_SIGN_TOOLTIP, SERVER_OPEN_SIGN_TOOLTIP, SERVER_OPEN_BOOK_TOOLTIP, SERVER_OPEN_DIALOG_TOOLTIP, SERVER_SET_NAMES, Choice, AUDIO_DEVICE_ROW, audio_buffer_label, AUDIO_BUFFER_ROW, PipewireKnopf, AUDIO_PIPEWIRE_SET_ROW, AUDIO_AUTO_ROW, AUDIO_TEST_TONE_ROW, AUDIO_PITCH_ROW, AUDIO_MASTER_ROW, AUDIO_CATEGORY_FIRST_ROW, AUDIO_UI_SOUND_FIRST_ROW, AUDIO_SPATIAL_ROW, ConfigEdit, config_button_rect, config_name_field_rect, CONFIG_BUTTON_GAP, CONFIG_BUTTON_W, active_config, MAX_VISIBLE_CONFIGS, config_row_rect, controls_rows, ControlsRow, SENSITIVITY_MIN, SENSITIVITY_MAX, CURSOR_SENSITIVITY_MIN, CURSOR_SENSITIVITY_MAX, CURSOR_SIZE_MIN, CURSOR_SIZE_MAX, HOLD_CONFIRM_MAX, RECONNECT_HOLD_GENERAL, CURSOR_WIDTH_MIN, CURSOR_WIDTH_MAX, bind_display, keybind_rect, keybind_mode_rect, KeyBindAction, hud_done_button, hud_list_item_rect, hud_config_panel, hud_config_close_button, HudElement, CROSSHAIR_COLOR_MODES, crosshair_swatch_rect, CROSSHAIR_MODE_COMPLEMENTARY, crosshair_shape_row, crosshair_shape_label, CROSSHAIR_SHAPES, HOTBAR_DURABILITY_POINTS, HOTBAR_DURABILITY_STEP, CHAT_SCALE_MIN, CHAT_SCALE_MAX, CHAT_WIDTH_MIN, CHAT_WIDTH_MAX, CHAT_LINES_MAX, CHAT_LINES_OPEN_MAX, CHAT_FADE_STEPS, CHAT_LINE_SPACING_MAX, CHAT_STYLE_ROW, EFFECTS_HIDE_AMBIENT_ROW, EFFECTS_STYLE_ROW, EFFECTS_ALIGNS, hud_config_pick_rect, EFFECTS_ALIGN_ROW, EFFECTS_ICON_ROW, EFFECTS_NAME_ROW, EFFECTS_TIMER_STYLES, EFFECTS_TIMER_ROW, EFFECTS_TIMER_COLOR_ROW, EFFECTS_BLINK_MODES, EFFECTS_BLINK_ROW, hud_align_button, armor_hud, HUD_TEXT_STYLE_NAMES, ARROW_OPACITY_MIN, hud_reset_button, hud_config_rows, ThemeNameDialog, THEME_NAME_MAX, NameDialogPurpose, SIDEBAR_LINKS, open_external_command, notifications, step_sensitivity, minecraft_sens_to_client, step_cursor_sensitivity, CURSOR_SIZE_STEP, CURSOR_WIDTH_STEP, HOLD_CONFIRM_STEP, FOV_STEP, CAMERA_DISTANCE_STEP, CAMERA_Y_OFFSET_STEP, TEAR_TIMING_STEP_US, TEAR_RESERVE_STEP_US, shared_config_dir, HUD_SCALE_PCT_STEP, SCROLL_DISTANCE_STEP, ENTITY_DISTANCE_STEP, BLOCK_ENTITY_DISTANCE_STEP, CHAT_SCALE_STEP, CHAT_WIDTH_STEP, chat_fade_step, CHAT_LINE_SPACING_STEP, CrosshairPainter, crosshair_editor, step_in, spatial_sound, align_anchor, effects_detail_anchor, SHOULDER_OFFSET_STEP, latency_tuning, SMOOTH_LIGHTING_STEP, apply_language_settings, EditableValue, UNLIMITED_INPUT, FPS_LIMIT_UNLIMITED, parse_volume, FPS_LIMIT_TYPED_MAX, SHADOW_THICKNESS_DEFAULT, SliderKey, ModuleRow, slider_number, clock_from_day_start, addons, addon_number_text, toteming, VoiceKnob, GpuState, UiRect, UiText, CARET_WIDTH_PX, push_typed_marker, TOPBAR_SUB_SCALE, ShieldColor, HitboxVariant, ModuleId, DamageTintAxis, SkyColorAxis, hsv_to_rgb, ColorPopup, COLOR_POPUP_PAD, COLOR_POPUP_PREVIEW_H, COLOR_POPUP_ROW_H, COLOR_POPUP_W, TOPBAR_H, COLOR_POPUP_TRACK_H, MAX_BINDS_PER_ACTION, UiOverlay, UiOverlayGroup, HoldUseRepeat, ARROW_EMPTY_FEEDBACK_NAMES, SkyMode, WeatherMode, ego_name, AUDIO_BUFFER_CHOICES, CROSSHAIR_SHAPE_CUSTOM, audio_buffer_request, Dropdown, smooth_scroll, DROPDOWN_PAD, DROPDOWN_ITEM_H, DROPDOWN_BAR_W, push_fade_marker, ScrollFade, mix_color, push_slider_unit, accounts, set_active_config, Settings, config_stamp, CONFIG_POLL_INTERVAL, FontKind, settings_visible_rows, SaveStatus, ShadowArea, push_shadow_marker, push_font_marker, push_plain_numbers_marker, LayoutFontScope, SETTINGS_CONTENT_DX, SETTINGS_CONTENT_DY, SIDEBAR_PAD, SIDEBAR_ITEM_H, SIDEBAR_W, TopbarLayout, TOPBAR_ICON, bottom_tile_rect, SETTINGS_LABEL_X, SETTINGS_FIRST_Y, SECTION_TAB_GAP, BOTTOMBAR_H, UiSprite, loading_dots, SpriteAtlas, uuid_bytes_from_hex, AccElem, AccKind, dialog_dim, token_display, styled_button, AccountContextItem, wrap_text, server_icon_px, server_icon_uv, drag_icon_rim, skin_tiles_top, skin_tiles_bottom, skin_icon_rect, skin_clip, push_gear, skin_tile_rect, pack_icon_uv, skin_files_layout, ellipsize, push_chevron, push_check_box, skin_files_rows_visible, field_press, push_back_arrow_icon, push_gear_icon, push_bell_icon, sidebar_hover_fill, LOGO_DRAW, logo_uv, typed_runs, chat_input, TypedRun, erased_pieces, zufallswert, FallingGlyph, notification_accent, NOTIFICATION_PANEL_SLIDE, notification_panel_w, HoverTarget, Duration, SLIDER_DIGIT_W, SETTINGS_CLIP_TOP, settings_rows_bottom, SETTINGS_ROW_H, push_clip_marker, SearchPlace, SEARCH_FLASH_SECS, SLIDER_SPAN_W, UI_MARGIN, tooltip_panel_origin, FONT_SLOT_H, settings_row_hover_rect, SETTINGS_TOOLTIPS, FOV_TOOLTIP, FOV_THIRD_BACK_TOOLTIP, FOV_THIRD_FRONT_TOOLTIP, CAMERA_DISTANCE_TOOLTIP, CAMERA_Y_OFFSET_TOOLTIP, DAMAGE_TINT_TOOLTIP, DAMAGE_TINT_SKIN_ONLY_TOOLTIP, DAMAGE_TINT_TICKS_TOOLTIP, CRIT_COLOR_TOOLTIP, PROJECTION_TOOLTIP, SMOOTH_LIGHTING_TOOLTIP, PARTICLES_TOOLTIP, GLINT_SPEED_TOOLTIP, GLINT_STRENGTH_TOOLTIP, PLAYER_LIST_SCALE_TOOLTIP, BLOCK_ROTATION_TOOLTIP, ARMOR_TRIMS_TOOLTIP, VIEW_MODEL_TOOLTIP, DEATH_ANIM_TOOLTIP, ANIMS_TOOLTIP, RENDER_DISTANCE_TOOLTIP, ENTITY_DISTANCE_TOOLTIP, BLOCK_ENTITY_DISTANCE_TOOLTIP, LOD_DETAIL_TOOLTIP, SHOULDER_TOOLTIP, SHOULDER_CROSSHAIR_TOOLTIP, SHOULDER_FADE_TOOLTIP, SHOULDER_DECOUPLED_TOOLTIP, UI_PITCH_TOOLTIP, AUDIO_BUFFER_INFO_ROW, AUDIO_PIPEWIRE_ROW, AUDIO_PIPEWIRE_PRIO_ROW, SPATIAL_TEST_TOOLTIP, MASTER_VOLUME_TOOLTIP, VOLUME_CATEGORIES, LATIN_MACRONS_TOOLTIP, VANILLA_SHADOW_TOOLTIP, FONT_SHADOW_X, FONT_TOOLTIP, TEXT_SHADOW_TOOLTIP, STABLE_NUMBERS_TOOLTIP, CAPITALS_TOOLTIP, FONT_EXCEPTIONS_ROW, AREA_SHADOW_TOOLTIP, FONT_DRAG_HINT, SENSITIVITY_TOOLTIP, CURSOR_SENSITIVITY_TOOLTIP, CURSOR_SIZE_TOOLTIP, CURSOR_WIDTH_TOOLTIP, HOLD_CONFIRM_TOOLTIP, RECONNECT_HOLD_TOOLTIP, CURSOR_CIRCLE_TOOLTIP, SNEAK_PROTECTION_TOOLTIP, SNAP_TAP_TOOLTIP, JUMP_COOLDOWN_TOOLTIP, SPRINT_ON_START_TOOLTIP, HOLD_USE_REPEAT_TOOLTIP, ROCKET_REPEAT_TOOLTIP, EXTINGUISH_DC_GHOST_TOOLTIP, USE_WHILE_MINING_TOOLTIP, CAPS_LOCK_TOOLTIP, KEYBIND_TOOLTIP, SKIN_TAB_HINT, UI_SCALE_TITLE, WALLPAPER_DIM, HUD_LIST_ITEM_W, push_pattern_rects, HUD_CONFIG_LABEL_X, CHAT_TOOLTIPS, HUD_EDITOR_GRID_PX, COLOR_GRID_LINE, COLOR_HUD_HANDLE_ACTIVE, COLOR_HUD_HANDLE, COLOR_PERF, crosshair_parts, crosshair_part_rect, COLOR_HUD, COLOR_SCOREBOARD_BG, SHIELD_CD_TEXT_SCALE, align_left, BossBarState, ChatLine, inventory, COLOR_DEATH_OVERLAY, COLOR_DEATH_TITLE, wrap_spans_styled, MOTD_VANILLA_WIDTH, trim_motd_padding, span_row_width, wrap_spans, ellipsize_spans, push_styled_row, fade_sprite, ACCOUNT_INPUT_FIELD};
// Token-Export auskommentiert, vorerst abgeschaltet. Dann im use oben wieder: ACCOUNT_CONTEXT_W, TokenExport, AuthEvent, TokenExportButton

mod servers;
mod screen_buttons;
mod menu_actions;
mod sliders;
mod color_popup;
mod dropdown;
mod config;
mod chrome;
mod account_menu;
mod dialogs;
mod chrome_draw;
mod menu_ui;
mod hud_editor;
mod press;

impl App {
    // Schieberegler statt der zwei Pfeile eines Steppers (s. StepSlider). `value_rect` ist
    // das Wertfeld, das der Stepper gehabt haette.
    pub(crate) fn step_slider(prev: ButtonAction, next: ButtonAction, value_rect: Rect, value: f64, min: f64, max: f64, enabled: bool) -> Button {
        Button {
            slider: Some(StepSlider { prev, next, value, min, max, value_rect, digits: 3, track_w: SLIDER_TRACK_W, soft: max }),
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

    // Derselbe Regler mit kuerzerer Bahn: sie endet bei `soft`, mit gehaltener rechter Taste
    // reicht sie bis zur harten Grenze (s. StepSlider::end).
    pub(crate) const fn soft_end(mut b: Button, soft: f64) -> Button {
        if let Some(s) = b.slider.as_mut() {
            s.soft = soft.min(s.max);
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
    pub(crate) const fn swatch(target: ColorTarget, at: Rect, rgba: [f32; 4]) -> Button {
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
            self.audio_devices.iter().position(|d| *d == self.settings.audio_output_device).map_or(0, |i| i + 1)
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
        if let Some(max) = self.gpu.as_ref().filter(|_| self.module_menu_shown()).map(|gpu| self.module_max_scroll(gpu) as f32) {
            self.module_menu_scroll.tick(dt, rate, max);
        }
        self.tick_dialog_scroll(dt, rate);
        self.drag_scroll_hold();
        self.tick_press_anim();
        // Der aktive Reiter: die Hinterlegung gleitet hin, auch ohne Scroll-Traegheit. Auf
        // einer anderen Seite steht sie gleich am Ziel.
        let ziel = self.section_tabs().into_iter().find(|b| b.color == theme::get(theme::Slot::AccentSurface)).map(|b| b.rect);
        self.section_slide = match (self.section_slide, ziel) {
            (Some((screen, r)), Some(z)) if screen == self.screen => {
                let k = 1.0 - (-dt * SECTION_SLIDE_RATE).exp();
                let naeher = |a: f32, b: f32| if (b - a).abs() < 0.5 { b } else { a + (b - a) * k };
                Some((screen, Rect { x: naeher(r.x, z.x), w: naeher(r.w, z.w), ..z }))
            }
            _ => ziel.map(|z| (self.screen, z)),
        };
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
            NumMut::I32(f) => (n >= f64::from(i32::MIN) && n <= f64::from(i32::MAX)).then(|| *f = n as i32).is_some(),
            NumMut::U8(f) => (0.0..=f64::from(u8::MAX)).contains(&n).then(|| *f = n as u8).is_some(),
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
    (0.0..=f64::from(u32::MAX)).contains(&n).then_some(n as u32)
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
