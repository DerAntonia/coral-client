// Steuerung: Tastenbelegungen, Maus, Hotbar, Klicks im Inventar und im Rezeptbuch, Scrollen.

use crate::{theme, App, AppMode, latency_tuning, ui_sound, MenuScreen, ConnStatus, InputKind, KeyBindAction, KeybindMode, Perspective, Bind, BindMouse, KeyCode, Instant, INVENTORY_DOUBLE_CLICK, CAPS_LOCK_RECHECK, Duration, AUTOCLICK_ACTIONS, SHOULDER_AIM_HOLD, GAME_MODE_CREATIVE, inventory, PendingInputAction, InventoryDrag, quick_craft_button, QuickCraftPending, QUICK_CRAFT_PENDING_MAX, zufallswert, Rect, recipe_book, restart_swing, creative, GAME_MODE_SPECTATOR, VoiceFlag, keycode_name, controls_rows, ControlsRow, keybind_rect, settings_max_first, settings_rows_bottom, smooth_scroll, UI_SCREEN, THEME_TILE_COUNT, SETTINGS_SCROLL_FIRST, SETTINGS_SCROLL_BOTTOM, store_section_starts, SETTINGS_CLIP_TOP, store_content_clip, SETTINGS_SCROLL_ROWS_PER_NOTCH, ScrollList, SETTINGS_ROW_H, SETTINGS_FIRST_Y, DROPDOWN_MAX_ITEMS, settings_row_pos, DROPDOWN_ITEM_H, scroll_inertia_rate, SettingsPage, zoom_sens_factor, camera_sensitivities, MAX_PITCH, server_panel, skin_tab, skin_column_rect, VOLUME_WHEEL_STEP, VOLUME_DISPLAY_HOLD, VOLUME_DISPLAY_FADE, VOLUME_KEY_REPEAT, VOLUME_KEY_REPEAT_DELAY, UiOverlay, cursor_fx, VOLUME_DISPLAY_ENTER, VOLUME_DISPLAY_W, NOTIFICATION_PADDING, UI_SCALE_SMALL, UI_SCALE_NORMAL, VOLUME_BAR_H, VOLUME_DISPLAY_MARGIN, UiOverlayGroup, UiRect, UiText, volume_text, physics, CursorFxFrame, ZOOM_SMOOTH_SECS, resolve_snap_tap, Keybinds, bind_name};

mod navigation;
mod key_actions;
mod inventory_input;
mod recipe_input;
mod binds;
mod scrolling;
mod mouse_wheel;
mod movement_input;

impl App {
    // Wir uebernehmen die Maus nur, wenn wir den Bildschirm wirklich besitzen: fokussiert
    // UND im Fullscreen. Sonst (Alt-Tab, oder Fenstermodus weil Exclusive Fullscreen
    // fehlschlug) bleibt der System-Cursor sichtbar und die Maus gehoert dem Desktop.
    pub(crate) fn owns_mouse(&self) -> bool {
        match &self.gpu {
            Some(gpu) => self.focused && gpu.window.fullscreen().is_some(),
            None => false,
        }
    }

    // Den Zeiger an eine Stelle setzen. Zeichnen wir ihn selbst (Vollbild), genuegt die
    // Position; sonst gehoert er dem Fenstersystem und wird dorthin versetzt -- das naechste
    // CursorMoved schriebe sonst die alte Stelle zurueck. Ohne Fokus nie: dann gehoert der
    // Zeiger dem Desktop, und ein Server, der ein Fenster oeffnet, riss ihn sonst mitten aus
    // einem anderen Programm.
    // Massstab der Menues an Modus und Fensterhoehe angleichen (s. GpuState::ui_scale), vor
    // jedem Ereignis: Menues sehen bei jeder Aufloesung gleich aus, auch das Modul-Menue im
    // Spiel. Das Spiel selbst (HUD, Inventar) und der HUD-Editor, der das HUD in echter Groesse
    // zeigt, rechnen in echten Pixeln. Der Zeiger bleibt dabei an seiner Stelle auf dem Schirm.
    pub(crate) fn sync_ui_scale(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let menue = match self.mode {
            AppMode::Menu => self.screen != MenuScreen::HudEditor,
            AppMode::World => self.module_menu_open,
        };
        let neu = if menue { Self::world_menu_scale(gpu) } else { 1.0 };
        let alt = gpu.ui_scale.replace(neu);
        gpu.cursor_scale.set(neu);
        if alt != neu && self.cursor_pos.0.is_finite() && self.cursor_pos.1.is_finite() {
            self.cursor_pos = (self.cursor_pos.0 * alt / neu, self.cursor_pos.1 * alt / neu);
        }
    }

    // Massstab der Menue-Teile, die im Spiel zwischen das HUD gezeichnet werden (Modul-Menue,
    // Benachrichtigungen): derselbe wie im Menue.
    //
    // Zum Pruefen ohne zweiten Bildschirm: CORAL_MENU_SCALE=1.25 erzwingt einen Massstab, das
    // Menue rechnet dann wie auf einem entsprechend kleineren Schirm und wird vergroessert
    // gezeichnet -- alle Umrechnungen (Zeiger, Zuschnitt, Schrift) laufen wie bei QHD.
    pub(crate) fn world_menu_scale(gpu: &crate::GpuState) -> f32 {
        static ERZWUNGEN: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
        let erzwungen = *ERZWUNGEN.get_or_init(|| std::env::var("CORAL_MENU_SCALE").ok()?.parse::<f32>().ok().filter(|s| (0.25..=4.0).contains(s)));
        erzwungen.unwrap_or_else(|| crate::menu_scale_for(gpu.real_size().1))
    }

    pub(crate) fn place_cursor(&mut self, pos: (f32, f32)) {
        self.cursor_pos = pos;
        if self.focused && !self.owns_mouse() {
            if let Some(gpu) = self.gpu.as_ref() {
                // Das Fenstersystem zaehlt echte Pixel (s. GpuState::ui_scale).
                let s = gpu.ui_scale.get();
                let _ = gpu.window.set_cursor_position(winit::dpi::PhysicalPosition::new(f64::from(pos.0 * s), f64::from(pos.1 * s)));
            }
        }
    }

    // System-Cursor verstecken, wenn wir ihn selbst zeichnen -- und waehrend eine Server-
    // oder Pack-Kachel gezogen wird: dann haengt ihr Bild am Zeiger und ersetzt ihn. Nur bei
    // einer Aenderung ans Fenster weitergeben (laeuft jedes Bild).
    pub(crate) fn refresh_cursor_visibility(&self) {
        let sichtbar = !self.owns_mouse() && !self.drag_hides_cursor();
        if self.system_cursor_visible.replace(Some(sichtbar)) == Some(sichtbar) {
            return;
        }
        if let Some(gpu) = &self.gpu {
            gpu.window.set_cursor_visible(sichtbar);
        }
    }

    pub(crate) fn drag_hides_cursor(&self) -> bool {
        self.mode == AppMode::Menu && (self.server_drag_active().is_some() || self.skin_drag_active().is_some())
    }

    // Bei Fokusverlust (Alt-Tab) und beim Beenden auf den Systemwert von vor dem Start
    // zurueck -- der Schalter in den Einstellungen soll nur wirken, waehrend der Client
    // tatsaechlich laeuft und im Vordergrund/Vollbild ist, nicht dauerhaft den Desktop
    // umstellen.
    pub(crate) fn restore_original_tearfree(&mut self) {
        // Genau den Rohwert von vorher zurueck ("off" bleibt "off", "auto" bleibt "auto") --
        // nicht bloss an/aus.
        if let Some(original) = self.tearfree_original_value.clone() {
            if latency_tuning::tearfree_value().as_deref() != Some(original.as_str()) {
                match latency_tuning::set_tearfree_value(&original) {
                    Ok(()) => self.tearfree_state = latency_tuning::tearfree_enabled(),
                    Err(e) => self.push_log(format!("TearFree konnte nicht zurückgesetzt werden: {e}")),
                }
            }
        }
    }

    // Gegenstueck bei Fokusrueckgewinn: den vom Nutzer gewaehlten Wert (falls er den
    // Schalter je bedient hat) wieder anwenden, statt beim zwischenzeitlich
    // wiederhergestellten Originalwert zu bleiben.
    pub(crate) fn reapply_user_tearfree_choice(&mut self) {
        if self.tearfree_state.is_none() {
            return;
        }
        if let Some(choice) = self.tearfree_user_choice {
            if self.tearfree_state != Some(choice) {
                match latency_tuning::set_tearfree(choice) {
                    Ok(()) => self.tearfree_state = latency_tuning::tearfree_enabled(),
                    Err(e) => self.push_log(format!("TearFree konnte nicht gesetzt werden: {e}")),
                }
            }
        }
    }
}
