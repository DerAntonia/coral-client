// Fensterereignisse: Tastatur, Maus, Fokus.

use crate::*;

impl App {
    pub(crate) fn handle_window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.gpu.is_none() {
            return;
        }

        if matches!(event, WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. }) {
            self.selection_sound_deselect = true;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(focused) => {
                // Fokus verloren (Alt-Tab): System-Cursor zurueckgeben, eigenen Cursor
                // nicht mehr zeichnen und keine Maus-Deltas mehr verarbeiten.
                self.focused = focused;
                self.refresh_cursor_visibility();
                // Fokus verloren: gehaltene Steuertasten loslassen. Beim Raw-Keyboard kommt
                // das release der noch gedrueckten Taste sonst nie an (evdev laeuft weiter,
                // aber wir wenden es nicht mehr an) -- der Spieler wuerde ewig weiterlaufen.
                if !focused {
                    self.cursor_fx.release_all(Instant::now());
                    self.clear_controls();
                    // Loslassen ausserhalb des Fensters bekommen wir nicht mit.
                    self.binds_down.clear();
                    // DXGI-Exclusive-Fullscreen gibt den Bildschirm sonst nicht her: der
                    // Fokusverlust (Alt-Tab, Win+D) kommt als Event hier an, der eigentliche
                    // Anzeigewechsel aber nicht von selbst -- erst das explizite Minimieren
                    // laesst Windows aus dem Exclusive-Modus raus. Unter Linux/X11 macht der
                    // Fenstermanager das schon selbst (s. owns_mouse), deshalb nur hier.
                    #[cfg(target_os = "windows")]
                    if let Some(gpu) = self.gpu.as_ref() {
                        gpu.window.set_minimized(true);
                    }
                }
                // TearFree soll nur greifen, waehrend der Client laeuft und im Vordergrund
                // ist -- bei Alt-Tab weg den Desktop-Wert zurueck, bei Rueckkehr den vom
                // Nutzer gewaehlten Wert wieder anwenden.
                if focused {
                    self.reapply_user_tearfree_choice();
                } else {
                    self.restore_original_tearfree();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            // Das Mausrad. Es kommt ueber winit, nicht ueber den evdev-Bypass: der liest nur
            // REL_X/REL_Y, und fuer eine Scroll-Geste zaehlt Latenz nicht.
            //
            // Wohin eine Raste geht, der Reihe nach: eine wartende Tastenbelegung; eine Liste
            // unter dem Zeiger (Log, Tastenbelegungen, Chat, Modul-Menue); im Spiel die
            // Aktionen, die auf dem Rad liegen (Standard: Hotbar blaettern). Was dann noch
            // uebrig ist, stellt die Lautstaerke (wie in osu!).
            WindowEvent::MouseWheel { delta, .. } => {
                // Rasten: bei LineDelta ist 1.0 eine Raste, ein Touchpad liefert Pixel.
                let rasten = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / WHEEL_PIXELS_PER_NOTCH,
                };
                if self.dialog_shown() || self.code_of_conduct.is_some() {
                    self.dialog_scroll(rasten * 40.0);
                    return;
                }
                if self.dropdown_active().is_some() {
                    self.dropdown_scroll(rasten);
                    return;
                }
                // Offene Kontenliste oder Zeiger auf der Benachrichtigungsliste: die Seite darunter
                // bleibt stehen.
                if self.pointer_blocked() && self.mode == AppMode::Menu {
                    return;
                }
                if self.awaiting_keybind.is_some() {
                    if rasten.abs() > 0.01 {
                        let Some((target, slot)) = self.awaiting_keybind.take() else { return };
                        let rad = if rasten > 0.0 { BindMouse::WheelUp } else { BindMouse::WheelDown };
                        self.settings.keybinds.set_at(target, slot, Bind::Mouse(rad));
                        self.persist_settings();
                        self.push_log(format!("Tastenbelegung: {} → {}", target.label(), rad.name()));
                    }
                    return;
                }
                // Server-Karten: eine Karte je Raste.
                if self.mode == AppMode::Menu && self.screen == MenuScreen::Connect && self.server_edit.is_none() && server_panel().contains(self.cursor_pos.0, self.cursor_pos.1) {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / server_row_h(),
                    };
                    // Eine Karte je Raste; gleitend nachgezogen in tick_ui_anim.
                    let schritt = if lines.abs() < 1.0 && lines != 0.0 { lines.signum() } else { lines.round() };
                    let max = self.servers.len().saturating_sub(server_rows_visible());
                    self.server_scroll = (self.server_first() as f32 - schritt).clamp(0.0, max as f32) as usize;
                }
                // Einstellungsseiten im Zeilenraster: die ganze Seite, sobald sie nicht ins
                // Fenster passt.
                if self.mode == AppMode::Menu && self.screen.settings_page().is_some() {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / SETTINGS_ROW_H,
                    };
                    self.scroll_settings(lines * SETTINGS_SCROLL_ROWS_PER_NOTCH);
                }
                // Skin-Tab: das Dateifenster, sonst die Spalte unter dem Zeiger.
                if self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsSkin {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / SETTINGS_ROW_H,
                    };
                    if self.skin_files.is_some() {
                        self.scroll_skin_files(lines);
                    } else {
                        self.scroll_skin_column(lines);
                    }
                }
                // Partikel-Anpassung: durch die Arten.
                if self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsParticles {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / SETTINGS_ROW_H,
                    };
                    let max = particles::KINDS.len().saturating_sub(self.particle_visible_rows());
                    let neu = self.particle_scroll as f32 - (lines * SETTINGS_SCROLL_ROWS_PER_NOTCH).round();
                    self.particle_scroll = (neu.max(0.0) as usize).min(max);
                }
                // Lichtstaerken: durch die Trefferliste.
                if self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsLightLevels {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / SETTINGS_ROW_H,
                    };
                    let max = self.light_search_hits().len().saturating_sub(self.light_visible_rows());
                    let neu = self.light_scroll as f32 - (lines * SETTINGS_SCROLL_ROWS_PER_NOTCH).round();
                    self.light_scroll = (neu.max(0.0) as usize).min(max);
                }
                // Offener Chat: durch den Verlauf blaettern.
                if self.mode == AppMode::World && self.chat_input.is_some() {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 16.0,
                    };
                    let ueber_verlauf = self.gpu.as_ref().is_some_and(|gpu| self.chat_history_rect(gpu).contains(self.cursor_pos.0, self.cursor_pos.1));
                    if ueber_verlauf {
                        self.scroll_chat_history(lines * CHAT_SCROLL_LINES_PER_NOTCH);
                    } else {
                        self.scroll_chat(lines * CHAT_SCROLL_LINES_PER_NOTCH);
                    }
                }
                // Ueber einem Buendel waehlt das Rad ein Item darin (BundleMouseActions) -- und
                // scrollt sonst nichts.
                if self.mode == AppMode::World && self.screen_overlay_open() && self.bundle_scroll(rasten) {
                    return;
                }
                // Rezeptbuch: eine Reihe je Raste, solange der Zeiger ueber der Truhe steht.
                if self.mode == AppMode::World {
                    let ueber_buch = self.gpu.as_ref().is_some_and(|gpu| {
                        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
                        self.recipe_panel(&layout).is_some_and(|p| p.contains(self.cursor_pos.0, self.cursor_pos.1))
                    });
                    if ueber_buch {
                        let anzahl = self.recipe_shown().len();
                        self.recipe_book.scroll_by(rasten as f64, anzahl);
                        return;
                    }
                }
                // Kreativinventar: das Raster, eine Zeile je Raste.
                if self.mode == AppMode::World && self.creative_shown() {
                    self.creative.scroll_by(rasten as f64);
                }
                // Handelsbildschirm: die Angebotsliste, eine Zeile je Raste.
                if self.mode == AppMode::World && self.open_menu_type() == Some(inventory::MENU_MERCHANT) {
                    if let Some(c) = self.inventory.open.as_mut() {
                        c.merchant.scroll_by(rasten as f64);
                    }
                }
                // Modul-Menue: die Detailspalte scrollen, wenn sie nicht ganz hineinpasst.
                if self.module_menu_shown() {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / SETTINGS_ROW_H,
                    };
                    self.scroll_module_menu(lines * SETTINGS_SCROLL_ROWS_PER_NOTCH);
                }
                if self.wheel_scrolls_list() {
                    return;
                }
                // Was jetzt noch uebrig ist, ist Steuerung (Hotbar blaettern, Lautstaerke, was
                // sonst auf dem Rad liegt) -- und die kommt mit laufendem Maus-Bypass von dort,
                // nicht ueber X11 (s. RawMouseEvent::Wheel). Sonst zaehlte jede Raste doppelt.
                if self.raw_mouse.is_some() {
                    return;
                }
                self.wheel_rasten(rasten);
            }
            // Wartet die Tastenbelegung auf eine Eingabe, faengt JEDE Maustaste sie hier ab --
            // vor dem Linksklick-Arm darunter, sonst wuerde ein Rebind auf die linke Taste
            // sofort als normaler Menue-Klick verarbeitet statt als gewaehlte Belegung.
            WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } if self.awaiting_keybind.is_some() => {
                // Ausser "Erweitert" neben der wartenden Zeile: das oeffnet das Fenster mit allen
                // Belegungen, statt die linke Maustaste zu belegen.
                if button == MouseButton::Left {
                    let (cx, cy) = self.cursor_pos;
                    let erweitert = self.buttons().into_iter().find(|b| matches!(b.action, ButtonAction::OpenKeybindAdvanced(_)) && b.hits(cx, cy));
                    if let Some(b) = erweitert {
                        self.play_ui_sound(ui_sound_for_action(b.action, &self.settings));
                        self.run_menu_action(b.action);
                        return;
                    }
                }
                let Some((target, slot)) = self.awaiting_keybind.take() else { return };
                match BindMouse::from_winit(button) {
                    Some(mouse) => {
                        self.settings.keybinds.set_at(target, slot, Bind::Mouse(mouse));
                        self.persist_settings();
                        self.push_log(format!("Tastenbelegung: {} → {}", target.label(), mouse.name()));
                    }
                    None => self.push_log("Tastenbelegung: diese Maustaste wird nicht unterstützt"),
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                // Die Zeiger-Effekte hoeren auf JEDE Taste, unabhaengig davon, was der Klick
                // sonst noch ausloest -- so wie in osu!lazer, wo derselbe Zeiger ueber allem
                // liegt. Bewusst hier im winit-Pfad und nicht im Roh-Pfad: gezeichnet wird der
                // Zeiger nur, wenn es ihn gibt (Menue/Overlay), und dort kommen die Klicks
                // ueber winit.
                // Wie bei der Tastatur: mit Maus-Bypass kommt der Tastenzustand von dort. Nur
                // Tasten, die evdev hier gar nicht liest (Other), bleiben beim Fenstersystem.
                if let Some(mouse) = BindMouse::from_winit(button) {
                    if self.raw_mouse.is_none() || matches!(mouse, BindMouse::Other(_)) {
                        self.track_bind(Bind::Mouse(mouse), state == ElementState::Pressed);
                    }
                }
                let taste = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(n) => 5 + u32::from(n),
                };
                let jetzt = Instant::now();
                self.cursor_fx.button(jetzt, self.cursor_pos, taste, state == ElementState::Pressed);
                // Dialoge des Servers liegen ueber allem und schlucken jeden Druck, in Menue
                // und Welt.
                if self.dialog_shown() || self.code_of_conduct.is_some() {
                    match state {
                        ElementState::Pressed if matches!(button, MouseButton::Left | MouseButton::Right) => self.dialog_press(button == MouseButton::Right),
                        ElementState::Released => self.dialog_release(),
                        _ => {}
                    }
                    return;
                }
                // Verbindungs-Overlay: nur der Abbrechen-Knopf, sonst nichts.
                if state == ElementState::Pressed && self.connecting_view() {
                    if button == MouseButton::Left && connect_cancel_rect().contains(self.cursor_pos.0, self.cursor_pos.1) {
                        self.cancel_connect();
                    }
                    return;
                }
                // Offene Kontenliste: Klick darin waehlt ein Konto, daneben schliesst sie.
                if button == MouseButton::Left && state == ElementState::Pressed && self.account_menu_open && self.account_menu_press() {
                    return;
                }
                // Acc-Manager liegt ueber der ganzen Seite und schluckt jeden Klick. Rechts auf ein
                // Konto oeffnet dessen Menue; ein offenes Menue schluckt den naechsten Druck.
                if matches!(button, MouseButton::Left | MouseButton::Right) && state == ElementState::Pressed && self.account_manager_open && self.mode == AppMode::Menu {
                    if self.account_context.is_some() {
                        if button == MouseButton::Left {
                            self.account_context_press();
                        } else {
                            self.account_context = None;
                            self.account_context_open();
                        }
                    } else if button == MouseButton::Left {
                        self.account_manager_press();
                    } else {
                        self.account_context_open();
                    }
                    return;
                }
                // Das Disconnect-Fenster liegt ueber allem und schluckt jeden Druck.
                if state == ElementState::Pressed && self.mode == AppMode::Menu && self.disconnect_popup.is_some() {
                    if button == MouseButton::Left {
                        self.disconnect_popup_press();
                    }
                    return;
                }
                // Die Fenster der Seite Aussehen (neues Theme, Loeschen?) liegen ueber allem und
                // schlucken jeden Druck.
                if state == ElementState::Pressed && self.mode == AppMode::Menu && self.theme_delete.is_some() {
                    if button == MouseButton::Left {
                        self.theme_delete_press();
                    }
                    return;
                }
                if state == ElementState::Pressed && self.theme_name_dialog.is_some() {
                    if button == MouseButton::Left {
                        self.theme_name_press();
                    }
                    return;
                }
                // Quick Connect liegt ueber der ganzen Seite und schluckt jeden Klick.
                if button == MouseButton::Left && state == ElementState::Pressed && self.quick_connect_open && self.mode == AppMode::Menu {
                    self.quick_connect_press();
                    return;
                }
                // Ebenso das Server-Fenster; ein offenes Rechtsklick-Menue schluckt den naechsten
                // Druck (auf einer Zeile fuehrt er sie aus, daneben schliesst er es nur).
                if state == ElementState::Pressed && self.mode == AppMode::Menu && self.server_edit.is_some() {
                    if button == MouseButton::Left {
                        self.server_dialog_press();
                    }
                    return;
                }
                if state == ElementState::Pressed && self.mode == AppMode::Menu && self.server_context.is_some() {
                    if button == MouseButton::Left {
                        self.server_context_press();
                    } else {
                        self.server_context = None;
                        if button == MouseButton::Right {
                            self.server_card_press();
                        }
                    }
                    return;
                }
                // Farbfenster: es liegt ueber allem und bekommt jeden Druck der linken Taste,
                // auch den daneben (der schliesst es nur).
                if button == MouseButton::Left && self.color_popup.is_some() {
                    match state {
                        ElementState::Pressed => {
                            if self.color_popup_press() {
                                return;
                            }
                        }
                        ElementState::Released => self.color_popup_release(),
                    }
                }
                // Aufklappliste: wie das Farbfenster ueber allem, ein Druck daneben schliesst sie nur.
                if button == MouseButton::Left && state == ElementState::Pressed && self.dropdown.is_some() && self.dropdown_press() {
                    return;
                }
                // Offene Benachrichtigungsliste: ein Klick daneben (nicht auf die Glocke) schliesst sie.
                if button == MouseButton::Left && state == ElementState::Pressed && self.notification_panel_open {
                    let (cx, cy) = self.cursor_pos;
                    let auf_glocke = self.gpu.as_ref().is_some_and(|gpu| self.topbar_layout(gpu).bell_hit.contains(cx, cy));
                    match self.notification_panel() {
                        Some((r, _)) if r.contains(cx, cy) => return,
                        _ if auf_glocke => {}
                        _ => {
                            self.play_ui_sound(ui_sound::UiSound::Close);
                            self.set_notification_panel(false);
                            return;
                        }
                    }
                }
                // Benachrichtigungen liegen ueber allem: ein Druck auf eine Karte gehoert ihr
                // (Ziehen/Schliessen) und erreicht nichts, was darunter liegt.
                if button == MouseButton::Left {
                    let geschluckt = match state {
                        ElementState::Pressed => self.notifications.press(jetzt),
                        ElementState::Released => self.notifications.release(jetzt),
                    };
                    if geschluckt {
                        return;
                    }
                }
                // Pixel-Editor des Fadenkreuzes: Klicks ins Raster malen (links/rechts).
                if self.mode == AppMode::Menu
                    && self.screen == MenuScreen::HudEditor
                    && matches!(button, MouseButton::Left | MouseButton::Right)
                    && self.crosshair_painter_mouse(button == MouseButton::Right, state == ElementState::Pressed)
                {
                    return;
                }
                // Textauswahl: linke Taste markiert (zusaetzlich zu dem, was der Klick sonst
                // ausloest), die mittlere fuegt die primaere Auswahl ein -- beides nur, wo es
                // einen freien Zeiger gibt.
                if button == MouseButton::Left {
                    match state {
                        ElementState::Pressed => self.selection_press(),
                        ElementState::Released => self.selection_release(),
                    }
                }
                if button == MouseButton::Middle && state == ElementState::Pressed && self.selection_allowed() {
                    self.paste_primary();
                }
                // Gebundene Maustasten (s. Bind) im World-Modus auf ihre Aktion anwenden --
                // wie beim Angriff nur als Fallback ohne Raw-Maus, sonst kaeme derselbe
                // Klick doppelt an (einmal roh in about_to_wait, einmal hier).
                if self.mode == AppMode::World && self.raw_mouse.is_none() && self.chat_input.is_none() && self.sign_edit.is_none() {
                    if let Some(mouse) = BindMouse::from_winit(button) {
                        if !self.inventory.screen_open {
                            self.apply_bind(Bind::Mouse(mouse), state == ElementState::Pressed);
                        } else if !matches!(mouse, BindMouse::Left | BindMouse::Right) {
                            self.apply_inventory_mouse_bind(Bind::Mouse(mouse), state == ElementState::Pressed);
                        }
                    }
                }
                // Rechtsklick auf eine Belegung loescht sie. Bewusst NUR in der
                // Tastenbelegungs-Tabelle -- es ist der einzige Ort mit mehreren gleichartigen
                // Eintraegen, bei denen ein eigener Loeschen-Knopf je Zeile die Spalte
                // sprengen wuerde.
                // Rechte Taste auf einem Schieberegler gedrueckt halten: er reicht dann bis zur
                // hoechsten erlaubten Grenze und folgt dem Zeiger; Loslassen beendet das.
                // Bei einem Regler ueber Pfeil-Aktionen (s. StepSlider) ist das rechte Ende schon
                // die harte Grenze -- rechts gehalten zieht er deshalb wie mit der linken Taste.
                if button == MouseButton::Right && (self.module_menu_shown() || self.mode == AppMode::Menu) {
                    if state == ElementState::Released && self.step_drag.take().is_some() {
                        self.persist_settings();
                        return;
                    }
                    if state == ElementState::Pressed {
                        if let Some(sl) = self.step_slider_at_pointer() {
                            self.play_ui_sound(ui_sound::UiSound::Click);
                            self.step_drag = Some(sl.prev);
                            self.step_slider_drag();
                            return;
                        }
                    }
                }
                if button == MouseButton::Right && self.mode == AppMode::Menu {
                    if state == ElementState::Released && self.slider_extended {
                        self.slider_extended = false;
                        if self.slider_drag.take().is_some() {
                            self.persist_settings();
                        }
                        return;
                    }
                    let (qx, qy) = self.pointer();
                    if state == ElementState::Pressed {
                        if let Some(ev) = self.slider_at(qx, qy).or_else(|| self.editable_value_at(qx, qy).filter(|ev| ev.slider().is_some())) {
                            self.slider_extended = true;
                            self.slider_drag = Some(ev);
                            self.slider_set_from_pointer(ev);
                            return;
                        }
                    }
                }
                if button == MouseButton::Right && state == ElementState::Pressed && self.mode == AppMode::Menu {
                    // Rechte Taste auf einer Karte: ziehen zum Verschieben, ohne Bewegung das
                    // Kontextmenue (s. server_drag_release).
                    if self.screen == MenuScreen::Connect {
                        self.server_card_press();
                        return;
                    }
                    // Aussehen: Rechtsklick auf ein eigenes Theme fragt, ob es weg soll.
                    if let Some(i) = self.custom_theme_at_pointer() {
                        self.play_ui_sound(ui_sound::UiSound::Open);
                        self.theme_delete = Some(i);
                        return;
                    }
                    // HUD-Editor: Rechtsklick auf ein Element oeffnet seine Einstellungen.
                    if self.screen == MenuScreen::HudEditor && self.crosshair_painter.is_none() {
                        if let Some(element) = self.hud_element_at_cursor() {
                            self.hud_selected = Some(element);
                            self.hud_drag_offset = None;
                        }
                        return;
                    }
                    // Schrift-Tab: Rechtsklick leert eine Ausnahme.
                    if let Some(slot) = self.font_slot_at(self.cursor_pos.0, self.cursor_pos.1).filter(|s| !s.is_default()) {
                        self.set_font_slot(slot, None);
                        self.persist_settings();
                        return;
                    }
                    let (qx, qy) = self.pointer();
                    if let Some((action, slot)) = self.keybind_slot_at(qx, qy) {
                        if let Some(removed) = self.settings.keybinds.bind_at(action, slot) {
                            self.settings.keybinds.remove_at(action, slot);
                            self.persist_settings();
                            self.push_log(format!("Tastenbelegung: {} -- {} entfernt", action.label(), bind_name(removed)));
                        }
                        // Ein laufendes Warten auf diese Zeile ergibt jetzt keinen Sinn mehr.
                        self.awaiting_keybind = None;
                    }
                    return;
                }
                if button == MouseButton::Right && state == ElementState::Released {
                    self.server_drag_release();
                }
                // Alles Weitere haengt an der LINKEN Taste (Menue-Klicks, Angriff, Drag).
                if button != MouseButton::Left {
                    return;
                }
                if state == ElementState::Released {
                    self.skin_drag_release();
                    if let Some(edit) = self.server_edit.as_mut() {
                        edit.dragging = false;
                    }
                    self.quick_dragging = false;
                    if let Some(d) = self.theme_name_dialog.as_mut() {
                        d.dragging = false;
                    }
                    self.book_edit_dragging = false;
                    if !self.slider_extended && self.slider_drag.take().is_some() {
                        self.persist_settings();
                    }
                    if self.step_drag.take().is_some() {
                        self.persist_settings();
                    }
                    // Drag beenden und die neue Position speichern.
                    if self.hud_drag_offset.take().is_some() {
                        self.persist_settings();
                    }
                    self.font_drag_release();
                    // Ohne Raw-Maus laeuft auch der Inventar-Klick hier -- dann muss das
                    // Loslassen ebenso hier ankommen, sonst bliebe ein Klick mit vollem
                    // Zeiger unbeantwortet haengen (s. inventory_release).
                    if self.raw_mouse.is_none() && self.inventory.screen_open {
                        self.inventory_release(0);
                    }
                    return;
                }
                // Laeuft gerade eine Freitext-Zahleneingabe (s. editing_value) und der Klick
                // liegt NICHT auf deren eigenem Feld, gilt er als Bestaetigung -- irgendwo
                // anders hinklicken uebernimmt die Eingabe, statt sie stillschweigend zu
                // verwerfen. Muss vor allem andere hier, sonst wuerde z.B. ein Klick auf
                // einen Stepper-Pfeil den Puffer verlieren, statt ihn erst zu commiten.
                if let Some((ev, _)) = self.editing_value {
                    let (qx, qy) = self.pointer();
                    if !if ev.slider().is_some() { ev.slider_value_rect() } else { ev.rect() }.contains(qx, qy) {
                        self.commit_edit_value();
                    }
                }
                if let Some((prev, _)) = self.editing_step {
                    let (qx, qy) = self.pointer();
                    if !self.step_slider_of(prev).is_some_and(|sl| sl.value_box().contains(qx, qy)) {
                        self.commit_step_edit();
                    }
                }
                // World-Modus: Angriff -- aber nur als Fallback ohne Raw-Maus, sonst kaeme
                // derselbe Klick doppelt an (einmal roh in about_to_wait, einmal hier).
                if self.mode == AppMode::World {
                    // Angriff laeuft ueber die Tastenbelegung (Standard: linke Maustaste, s.
                    // KeyBindAction::Attack) -- hier nur noch Oberflaechen-Klicks, die keine
                    // belegbare Aktion sind: Inventar-Slots und das Modul-Menue.
                    //
                    // Das Modul-Menue geht ueber winit statt ueber den Raw-Pfad (wie die
                    // Menue-Klicks): dort zaehlt der gezeichnete Zeiger, nicht die Latenz.
                    if self.sign_edit.is_some() {
                        // Der Schild-Editor hat keinen Knopf: Enter schliesst ab.
                    } else if self.book_edit.is_some() {
                        self.book_edit_click();
                    } else if self.book_view.is_some() || self.lectern_book().is_some() {
                        self.book_click();
                    } else if self.module_menu_open {
                        let (px, py) = self.cursor_pos;
                        self.handle_click(px, py);
                    } else if self.chat_input.is_some() {
                        let (px, py) = self.cursor_pos;
                        self.chat_click(px, py);
                    } else if self.raw_mouse.is_none() && self.inventory.screen_open {
                        self.inventory_click(0);
                    }
                } else if self.screen == MenuScreen::HudEditor && self.crosshair_painter.is_none() && self.hud_selected.is_none() {
                    // Ohne offenes Fenster gibt es keine Knoepfe (s. buttons()), ein Klick kann
                    // nur ein Element treffen (oder daneben -- dann passiert nichts).
                    if let Some(element) = self.hud_element_at_cursor() {
                        self.begin_hud_drag(element);
                    }
                } else if self.screen == MenuScreen::HudEditor && self.crosshair_painter.is_none() && !self.hud_config_panel_contains(self.cursor_pos) {
                    // Klick neben das Einstellungsfenster schliesst es.
                    self.hud_selected = None;
                    self.persist_settings();
                } else if self.skin_files_press() || self.skin_tile_press() {
                    // Skin-Tab: Dateifenster bzw. eine Kachel (Klick oder Ziehen).
                } else if self.font_drag_press() {
                    // Schrift-Tab: ein Kaestchen haengt jetzt am Zeiger.
                } else if let Some(ev) = self.slider_at(self.pointer().0, self.pointer().1) {
                    // Auf die Bahn geklickt: der Wert springt dorthin, Ziehen fuehrt weiter.
                    self.slider_drag = Some(ev);
                    self.slider_set_from_pointer(ev);
                    self.play_ui_sound(ui_sound::UiSound::Click);
                } else if let Some(ev) = self.editable_value_at(self.pointer().0, self.pointer().1) {
                    // Zahl direkt angeklickt -> Freitexteingabe starten (ausser sie laeuft
                    // fuer genau dieses Feld schon, sonst wuerde ein zweiter Klick mitten in
                    // der Eingabe den Puffer auf den alten Wert zuruecksetzen).
                    if self.editing_value.as_ref().map(|(e, _)| *e) != Some(ev) {
                        self.begin_edit_value(ev);
                    }
                } else if !self.server_card_connect() {
                    let (px, py) = self.cursor_pos;
                    self.handle_click(px, py);
                }
            }
            WindowEvent::KeyboardInput { event: key_event, .. } => {
                // Loeschen in einem Eingabefeld: der naechste Frame laesst das Geloeschte fallen.
                if key_event.state == ElementState::Pressed
                    && (matches!(key_event.logical_key, Key::Named(NamedKey::Backspace | NamedKey::Delete))
                        || (self.modifiers.control_key() && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyX)))
                {
                    self.erase_pending = true;
                }
                // Welche Tasten unten sind (binds_down), kommt mit laufendem Tastatur-Bypass NUR
                // von dort -- einstellbare Eingaben sind raw (s. raw_input, Grundregel).
                if let PhysicalKey::Code(code) = key_event.physical_key {
                    if !key_event.repeat && self.raw_keyboard.is_none() {
                        self.track_bind(Bind::Key(code), key_event.state == ElementState::Pressed);
                    }
                }
                // Strg+C mit markiertem Text: kopieren. Nur wenn es wirklich etwas zu kopieren
                // gibt -- sonst laeuft die Taste normal weiter (C kann belegt sein).
                if self.awaiting_keybind.is_none()
                    && key_event.state == ElementState::Pressed
                    && self.modifiers.control_key()
                    && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyC)
                    && self.copy_selection()
                {
                    return;
                }
                // Debug-Flag CORAL_SCREENSHOT: F2 legt das naechste Bild in die Zwischenablage.
                if self.awaiting_keybind.is_none()
                    && key_event.state == ElementState::Pressed
                    && !key_event.repeat
                    && key_event.physical_key == PhysicalKey::Code(KeyCode::F2)
                {
                    if let Some(gpu) = self.gpu.as_mut().filter(|g| g.screenshot_supported) {
                        gpu.screenshot_requested = true;
                        self.push_log("Screenshot in die Zwischenablage kopiert");
                        return;
                    }
                }
                // Strg+D mit Markierung: aufheben.
                if self.awaiting_keybind.is_none()
                    && key_event.state == ElementState::Pressed
                    && self.modifiers.control_key()
                    && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyD)
                    && self.deselect_all()
                {
                    return;
                }
                // Wartet der Keybinds-Tab auf eine neue Taste, faengt sie hier ab, bevor
                // irgendetwas anderes (Alt+F4, Escape-Toggle, Bewegung) sie sieht -- sonst
                // wuerde z.B. ein Rebind auf Escape sofort als Abbruch missverstanden statt
                // als gewaehlte Taste.
                if let Some((target, slot)) = self.awaiting_keybind {
                    if key_event.state == ElementState::Pressed && !key_event.repeat {
                        if let PhysicalKey::Code(code) = key_event.physical_key {
                            if code == KeyCode::Escape {
                                self.push_log("Tastenbelegung: abgebrochen");
                            } else {
                                self.settings.keybinds.set_at(target, slot, Bind::Key(code));
                                self.persist_settings();
                                self.push_log(format!("Tastenbelegung: {} → {}", target.label(), keycode_name(code)));
                            }
                        }
                        self.awaiting_keybind = None;
                    }
                    return;
                }

                // Dasselbe fuer einen Regler ueber Pfeil-Aktionen (s. editing_step). Erlaubt ist,
                // was in dessen Feld stehen kann: Dezimalzahl mit Punkt oder Komma, Uhrzeit,
                // Minus vorn.
                if self.editing_step.is_some() {
                    if key_event.state == ElementState::Pressed {
                        if key_event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                            self.editing_step = None;
                        } else {
                            match &key_event.logical_key {
                                Key::Named(NamedKey::Enter) => self.commit_step_edit(),
                                Key::Named(NamedKey::Backspace) => {
                                    if let Some((_, buf)) = self.editing_step.as_mut() {
                                        buf.pop();
                                    }
                                }
                                _ => {
                                    if let (false, Some(text), Some((_, buf))) = (self.modifiers.control_key(), key_event.text.clone(), self.editing_step.as_mut()) {
                                        for ch in text.chars() {
                                            let ok = ch.is_ascii_digit()
                                                || (matches!(ch, '.' | ',') && !buf.contains(['.', ',', ':']))
                                                || (ch == ':' && !buf.contains(['.', ',', ':']) && !buf.is_empty())
                                                || (ch == '-' && buf.is_empty());
                                            if ok && buf.chars().count() < 8 {
                                                buf.push(ch);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                // Laeuft eine Freitext-Zahleneingabe (s. editing_value), faengt das hier ab
                // -- genau wie beim Tastenbelegung-Warten oben, bevor Alt+F4/Escape-Toggle/
                // Bewegung zum Zug kommen. Escape verwirft, Enter uebernimmt.
                if self.editing_value.is_some() {
                    if key_event.state == ElementState::Pressed {
                        if key_event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                            self.editing_value = None;
                        } else {
                            match &key_event.logical_key {
                                Key::Named(NamedKey::Enter) => self.commit_edit_value(),
                                Key::Named(NamedKey::Backspace) => {
                                    if let Some((_, buf)) = self.editing_value.as_mut() {
                                        buf.pop();
                                    }
                                }
                                _ => {
                                    if !self.modifiers.control_key() {
                                        if let Some(text) = key_event.text.clone() {
                                            if let Some((ev, buf)) = self.editing_value.as_mut() {
                                                let allow_dot = ev.allows_decimal();
                                                let allow_minus = ev.allows_negative();
                                                for ch in text.chars() {
                                                    // Komma ist der deutsche Dezimalpunkt.
                                                    let ch = if ch == ',' { '.' } else { ch };
                                                    let is_dot = ch == '.' && allow_dot && !buf.contains('.');
                                                    // Minus nur ganz vorn und nur, wo "-1" ueberhaupt eine
                                                    // Bedeutung hat -- sonst koennte man eine negative
                                                    // Renderdistanz eintippen, die beim Klemmen still auf
                                                    // das Minimum faellt.
                                                    let is_minus = ch == '-' && allow_minus && buf.is_empty();
                                                    if (ch.is_ascii_digit() || is_dot || is_minus) && buf.chars().count() < 7 {
                                                        buf.push(ch);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                // Alt+F4: beenden. Explizit behandelt, weil im Exclusive Fullscreen der
                // Fenstermanager oft gar nicht erst zum Zug kommt (kein CloseRequested).
                if self.modifiers.alt_key()
                    && key_event.physical_key == PhysicalKey::Code(KeyCode::F4)
                    && key_event.state == ElementState::Pressed
                {
                    event_loop.exit();
                    return;
                }

                // Escape: zwischen Menu und 3D-Welt wechseln (nur echter Press, kein
                // Auto-Repeat -- sonst wuerde es beim Halten durchtoggeln).
                if key_event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                    && key_event.state == ElementState::Pressed
                    && !key_event.repeat
                {
                    self.on_escape();
                    return;
                }

                // Debug-Overlay und Chat sind normale, belegbare Aktionen (s.
                // KeyBindAction::ToggleDebugOverlay/OpenChat) und laufen ueber apply_bind --
                // hier steht deshalb nichts mehr fest verdrahtet. Das Debug-Overlay soll
                // aber auch OHNE Welt-Modus umschaltbar bleiben, deshalb kommt es vor dem
                // World-Zweig darunter noch durch den winit-Pfad.
                if key_event.state == ElementState::Pressed && !key_event.repeat && self.mode == AppMode::Menu {
                    if let PhysicalKey::Code(code) = key_event.physical_key {
                        if self.settings.keybinds.binds(KeyBindAction::ToggleDebugOverlay).any(|b| b == Bind::Key(code)) {
                            self.debug_overlay = !self.debug_overlay;
                            return;
                        }
                        if self.settings.keybinds.binds(KeyBindAction::ToggleProfiler).any(|b| b == Bind::Key(code)) {
                            self.toggle_profiler();
                            return;
                        }
                    }
                }

                // Bewegungstasten: physische Position, nur im World-Modus abgefangen --
                // sonst waeren Hostnamen mit 'w'/'a'/'s'/'d' im Menu nicht mehr tippbar.
                // Gegen die frei belegbaren Tasten aus den Einstellungen geprueft (s.
                // Keybinds-Tab), nicht mehr fest verdrahtet. Bei offener Chat-Eingabe
                // ebenfalls uebersprungen (chat_input.is_none()) -- sonst wuerde z.B. "w"
                // gleichzeitig ein Zeichen tippen UND vorwaerts laufen, und faellt stattdessen
                // unten bis zur Texteingabe durch (auch wenn der Raw-Bypass laeuft: der
                // steuert waehrend des Tippens nichts mehr, s. about_to_wait).
                if self.mode == AppMode::World && self.chat_input.is_none() && self.sign_edit.is_none() && !self.text_field_focused() {
                    // Laeuft der evdev-Raw-Keyboard-Bypass, ist DER die alleinige Quelle fuer
                    // Spieltasten (s. about_to_wait) -- winit hier ganz ueberspringen, sonst
                    // kaeme dieselbe Taste doppelt an und der laggy X11-Weg wuerde die
                    // Latenz-Ersparnis aushebeln (analog zum Maus-Bypass in device_event).
                    if self.raw_keyboard.is_some() {
                        return;
                    }
                    if let PhysicalKey::Code(code) = key_event.physical_key {
                        let pressed = key_event.state == ElementState::Pressed;
                        self.apply_bind(Bind::Key(code), pressed);
                        if self.settings.keybinds.is_bound(Bind::Key(code)) {
                            return;
                        }
                    }
                }

                if key_event.state != ElementState::Pressed {
                    return;
                }

                // Im Buch blaettern Bild-auf/-ab (BookViewScreen.keyPressed).
                if self.book_view.is_some() || self.lectern_book().is_some() {
                    match key_event.logical_key {
                        Key::Named(NamedKey::PageUp) => {
                            self.book_turn(false);
                            return;
                        }
                        Key::Named(NamedKey::PageDown) => {
                            self.book_turn(true);
                            return;
                        }
                        _ => {}
                    }
                }
                if self.book_edit.is_some() && self.mode == AppMode::World {
                    let text = key_event.text.as_ref().map(|t| t.to_string());
                    self.book_edit_key(&key_event.logical_key, key_event.physical_key, text.as_deref());
                    return;
                }
                if self.sign_edit.is_some() {
                    // Enter ist Fertig, Shift+Enter die naechste Zeile (Tab und Pfeile wie gehabt).
                    if key_event.logical_key == Key::Named(NamedKey::Enter) && !self.modifiers.shift_key() {
                        self.finish_sign_edit();
                        return;
                    }
                    let paste = self.modifiers.control_key() && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyV);
                    let text = key_event.text.as_ref().map(|t| t.to_string());
                    self.sign_edit_key(&key_event.logical_key, text.as_deref(), self.modifiers.control_key(), paste);
                    return;
                }
                if self.anvil_typing() && self.mode == AppMode::World {
                    let paste = self.modifiers.control_key() && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyV);
                    let text = key_event.text.as_ref().map(|t| t.to_string());
                    self.anvil_name_key(&key_event.logical_key, text.as_deref(), self.modifiers.control_key(), paste);
                    return;
                }
                if self.creative_search_focused() && self.mode == AppMode::World {
                    let paste = self.modifiers.control_key() && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyV);
                    let text = key_event.text.as_ref().map(|t| t.to_string());
                    self.creative_search_key(&key_event.logical_key, text.as_deref(), self.modifiers.control_key(), paste);
                    return;
                }

                // Chat-Eingabe: eigener, kompletter Zweig statt in den Adress-Eingabe-Match
                // unten verschachtelt -- beide Felder gleichzeitig aktiv gibt es nie (Chat
                // nur im World-Modus, Adresse nur auf dem Connect-Screen), aber getrennt
                // bleibt klarer, welches Feld welche Logik hat (Chat sendet bei Enter statt
                // zu verbinden, kein Adresslaengen-Limit).
                if self.chat_input.is_some() {
                    // Das winit-Duplikat der Taste, die den Chat gerade geoeffnet hat (s.
                    // apply_raw_key) -- einmal verwerfen, damit es nicht als Zeichen im Feld
                    // landet.
                    if let Some((code, wann)) = self.swallow_text_key {
                        let ist_dieselbe = key_event.physical_key == PhysicalKey::Code(code);
                        if wann.elapsed() > SWALLOW_TEXT_KEY_WINDOW {
                            self.swallow_text_key = None;
                        } else if ist_dieselbe {
                            self.swallow_text_key = None;
                            return;
                        }
                    }
                    // Einfuegen: derselbe Weg wie getippter Text, damit auch hier die
                    // Laengengrenze und die Vorschlagslogik greifen.
                    if self.modifiers.control_key() && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyV) {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            if let Ok(text) = clipboard.get_text() {
                                self.chat_input_edit(&text, false);
                            }
                        }
                        return;
                    }
                    if self.modifiers.control_key() && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyA) {
                        self.chat_select_all();
                        return;
                    }
                    match &key_event.logical_key {
                        // Mit Markierung loeschen beide Tasten die Markierung; ohne loescht
                        // die Ruecktaste das letzte Zeichen (der Cursor steht immer am Ende,
                        // Entf hat dann nichts rechts von sich).
                        Key::Named(NamedKey::Backspace) => {
                            self.chat_input_edit("", true);
                        }
                        Key::Named(NamedKey::Delete) => {
                            let markiert = self.chat_input.as_ref().and_then(chat_input::ChatInput::keyboard_selection).or_else(|| self.chat_input_selection());
                            if markiert.is_some() {
                                self.chat_input_edit("", true);
                            } else if self.chat_input.as_mut().is_some_and(chat_input::ChatInput::delete_forward) {
                                self.play_ui_sound(ui_sound::UiSound::Erase);
                                self.request_command_suggestions();
                            }
                        }
                        // Cursor: Pfeile zeichenweise, mit Strg wortweise, Pos1/Ende; Shift
                        // markiert dabei.
                        Key::Named(taste @ (NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::Home | NamedKey::End)) => {
                            let strg = self.modifiers.control_key();
                            let m = match taste {
                                NamedKey::ArrowLeft if strg => chat_input::Move::WordLeft,
                                NamedKey::ArrowLeft => chat_input::Move::Left,
                                NamedKey::ArrowRight if strg => chat_input::Move::WordRight,
                                NamedKey::ArrowRight => chat_input::Move::Right,
                                NamedKey::Home => chat_input::Move::Home,
                                _ => chat_input::Move::End,
                            };
                            self.chat_move_cursor(m, self.modifiers.shift_key());
                        }
                        Key::Named(NamedKey::Enter) => self.send_chat_message(),
                        // Tab setzt den gewaehlten Vorschlag ein. Ist keiner offen, wird
                        // einer angefragt -- so kommt man mit Tab an die Liste, auch wenn
                        // der Server auf das bisher Getippte noch nichts geschickt hat.
                        // Tab setzt den gewaehlten Vorschlag ein, falls einer offen ist, und
                        // fragt danach in JEDEM Fall neu an: nach dem Einsetzen fuer die
                        // naechste Ebene (Befehl gewaehlt -> jetzt dessen Argumente), und ohne
                        // offene Liste, um ueberhaupt an eine zu kommen.
                        Key::Named(NamedKey::Tab) => {
                            if self.chat_input.as_mut().map(chat_input::ChatInput::apply_selected).unwrap_or(false) {
                                self.play_ui_sound(ui_sound::UiSound::Click);
                            }
                            self.request_command_suggestions();
                        }
                        // Pfeile: bei offener Vorschlagsliste blaettern sie durch DIESE, sonst
                        // durch den Nachrichtenverlauf (s. chat_input::ChatInput::step).
                        Key::Named(NamedKey::ArrowUp) => self.step_chat(chat_input::Step::Up),
                        Key::Named(NamedKey::ArrowDown) => self.step_chat(chat_input::Step::Down),
                        _ => {
                            if !self.modifiers.control_key() {
                                if let Some(text) = key_event.text.clone() {
                                    // Nur echte Zeichen -- ein Steuerzeichen (Escape o.ae.)
                                    // soll keine Markierung wegloeschen.
                                    if text.chars().any(|c| !c.is_control()) {
                                        self.chat_input_edit(&text, false);
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                // Disconnect-Fenster: Enter schliesst es wie der Knopf (Esc ueber on_escape).
                if self.disconnect_popup.is_some() && self.mode == AppMode::Menu {
                    if key_event.state == ElementState::Pressed && matches!(key_event.logical_key, Key::Named(NamedKey::Enter | NamedKey::Space)) {
                        self.play_ui_sound(ui_sound::UiSound::Close);
                        self.disconnect_popup = None;
                    }
                    return;
                }
                // Server-Formular bzw. Quick Connect: dieselben Tasten fuer beide Felder (s.
                // active_text_field), das Formular hat Vorrang.
                // Ohne Textfeld im Dialog gehen Tasten nirgends hin (Escape kam schon oben an).
                if (self.dialog_shown() || self.code_of_conduct.is_some()) && self.dialog_text_field().is_none() {
                    if key_event.logical_key == Key::Named(NamedKey::Tab) {
                        self.dialog_next_field();
                    }
                    return;
                }
                if self.dialog.is_some() || self.server_edit.is_some() || self.theme_name_dialog.is_some() || self.quick_typing() {
                    if key_event.state != ElementState::Pressed {
                        return;
                    }
                    let strg = self.modifiers.control_key();
                    let shift = self.modifiers.shift_key();
                    let taste = |c: KeyCode| key_event.physical_key == PhysicalKey::Code(c);
                    if strg && (taste(KeyCode::KeyC) || taste(KeyCode::KeyX)) {
                        let markiert = match self.active_text_field() {
                            Some(feld) if taste(KeyCode::KeyX) => feld.cut(),
                            Some(feld) => feld.selected_text().to_string(),
                            None => String::new(),
                        };
                        if !markiert.is_empty() {
                            if let Some(cb) = self.clipboard() {
                                let _ = cb.set_text(markiert);
                            }
                        }
                        return;
                    }
                    if strg && taste(KeyCode::KeyV) {
                        if let Some(text) = self.clipboard().and_then(|cb| cb.get_text().ok()) {
                            self.server_edit_insert(&text);
                        }
                        return;
                    }
                    if strg && taste(KeyCode::KeyA) {
                        if let Some(feld) = self.active_text_field() {
                            feld.select_all();
                        }
                        return;
                    }
                    let bewegung = match &key_event.logical_key {
                        Key::Named(NamedKey::ArrowLeft) => Some(if strg { chat_input::Move::WordLeft } else { chat_input::Move::Left }),
                        Key::Named(NamedKey::ArrowRight) => Some(if strg { chat_input::Move::WordRight } else { chat_input::Move::Right }),
                        Key::Named(NamedKey::Home) => Some(chat_input::Move::Home),
                        Key::Named(NamedKey::End) => Some(chat_input::Move::End),
                        _ => None,
                    };
                    if let Some(m) = bewegung {
                        if let Some(feld) = self.active_text_field() {
                            feld.move_cursor(m, shift);
                        }
                        return;
                    }
                    match &key_event.logical_key {
                        Key::Named(NamedKey::Backspace) => {
                            if let Some(feld) = self.active_text_field() {
                                feld.backspace();
                            }
                        }
                        Key::Named(NamedKey::Delete) => {
                            if let Some(feld) = self.active_text_field() {
                                feld.delete_forward();
                            }
                        }
                        Key::Named(NamedKey::Tab) if self.dialog.is_some() => self.dialog_next_field(),
                        Key::Named(NamedKey::Enter) if self.dialog.is_some() => self.dialog_enter(),
                        Key::Named(NamedKey::Tab) => {
                            if let Some(edit) = &mut self.server_edit {
                                edit.focus = match edit.focus {
                                    ServerField::Name => ServerField::Address,
                                    ServerField::Address => ServerField::Name,
                                };
                            }
                        }
                        // Escape kommt hier nie an -- es wird weiter oben global abgefangen
                        // und landet in on_escape(), das das Fenster schliesst.
                        Key::Named(NamedKey::Enter) if self.server_edit.is_some() => self.commit_server_edit(),
                        Key::Named(NamedKey::Enter) if self.theme_name_dialog.is_some() => self.create_custom_theme(),
                        Key::Named(NamedKey::Enter) => self.quick_connect_submit(),
                        _ => {
                            if !strg {
                                if let Some(text) = key_event.text.clone() {
                                    self.server_edit_insert(&text);
                                }
                            }
                        }
                    }
                    return;
                }

                // Eingabefeld (Token/Client-ID) hat Vorrang vor der Adresse, solange es offen
                // ist: Text, Einfuegen (Strg+V) und Enter (uebernehmen) gehen dorthin.
                if self.input_field.is_some() {
                    if self.modifiers.control_key() && key_event.physical_key == PhysicalKey::Code(KeyCode::KeyV) {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            if let Ok(text) = cb.get_text() {
                                if let Some((_, buf)) = &mut self.input_field {
                                    for ch in text.chars() {
                                        if !ch.is_control() {
                                            buf.push(ch);
                                        }
                                    }
                                }
                            }
                        }
                        return;
                    }
                    match &key_event.logical_key {
                        Key::Named(NamedKey::Backspace) => {
                            if let Some((_, buf)) = &mut self.input_field {
                                buf.pop();
                            }
                        }
                        Key::Named(NamedKey::Escape) => self.input_field = None,
                        Key::Named(NamedKey::Enter) => self.submit_input_field(),
                        _ => {
                            if !self.modifiers.control_key() {
                                if let Some(text) = &key_event.text {
                                    if let Some((_, buf)) = &mut self.input_field {
                                        for ch in text.chars() {
                                            if !ch.is_control() {
                                                buf.push(ch);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                // Lichtstaerken-Seite: Tippen geht ins Suchfeld.
                if self.screen == MenuScreen::SettingsLightLevels {
                    match &key_event.logical_key {
                        Key::Named(NamedKey::Backspace) => {
                            self.light_search.pop();
                        }
                        _ => {
                            if !self.modifiers.control_key() {
                                if let Some(text) = &key_event.text {
                                    self.light_search.extend(text.chars().filter(|c| !c.is_control()));
                                }
                            }
                        }
                    }
                    self.light_scroll = 0;
                    return;
                }

                // Enter schliesst im HUD-Editor die offenen Element-Einstellungen (wie Escape).
                // Die Adresse von Quick Connect tippt sich weiter oben (s. active_text_field).
                if key_event.logical_key == Key::Named(NamedKey::Enter)
                    && self.screen == MenuScreen::HudEditor
                    && self.crosshair_painter.is_none()
                    && self.hud_selected.take().is_some()
                {
                    self.persist_settings();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                // Das stehende Weltbild hinter dem Pausemenue ist mit der Groesse weg.
                self.menu_world_frozen = false;
            }
            WindowEvent::RedrawRequested => {
                // Erst aufs Swapchain-Bild warten, DANN die Zeit lesen und den Frame bauen (s.
                // GpuState::acquire_frame). update_movement ein zweites Mal: rechnet das
                // Zeitguthaben bis JETZT fort (samt faelliger Ticks) und setzt die Kamera auf
                // diesen Zeitpunkt -- der Aufruf in about_to_wait lag vor dem Warten.
                use profiler::Phase;
                self.update_system_watch();
                self.selection_sounds();
                let messen = self.profiler.active() || self.tear_control_active();
                let presented_at = if self.tear_control_active() { self.tear_present_at.take() } else { None };
                let riss = self.tear_control_active();
                if let Some(gpu) = self.gpu.as_mut() {
                    // Mit Zeitstempeln braucht der Profiler kein hartes Warten auf die GPU mehr
                    // (das zwang CPU und GPU in den Gleichschritt, s. gpu_timer) -- nur noch die
                    // Riss-Steuerung, oder ein Adapter ohne Zeitstempel.
                    let messen = riss || (messen && gpu.gpu_timer.is_none());
                    gpu.measure_gpu_done = messen;
                    gpu.present_at_ns = presented_at;
                }
                let Some(acquired) = self.gpu.as_mut().and_then(|gpu| gpu.acquire_frame()) else { return };
                self.profiler.add_ms(Phase::Acquire, acquired.1);
                let t = Instant::now();
                self.update_movement();
                self.profiler.add(Phase::MovementFrame, t);
                let fonts = self.wanted_fonts();
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.ensure_fonts(&mut self.installation, &fonts);
                }
                // Arbeitszeit und Wartezeit getrennt erfasst (s. FrameTiming): render_*
                // liefert bereits beides fuer den eigentlichen Zeichenteil, hier kommt nur
                // noch die UI-Aufbauzeit (build_hud/build_menu_ui) als Arbeit dazu -- die
                // laeuft vor dem Acquire und darf die Wartezeit nicht verfaelschen.
                self.update_discord_status();
                self.sync_anvil_name();
                // Kontokoepfe jedes Bild pruefen: Trennen, Tab- und Packwechsel raeumen die
                // Skin-Plaetze ab, und vorher kamen die Koepfe erst beim Oeffnen der Kontenliste
                // zurueck. Kostet nichts, solange sie im Atlas stehen (s. request_account_heads).
                self.request_account_heads();
                let timing = match self.mode {
                    // Verbindungsbildschirm mit Bild: wie ein Menue zeichnen, das Bild dahinter
                    // statt der (noch leeren) Welt.
                    AppMode::World if self.connect_view_shown() => {
                        let ui_started = Instant::now();
                        self.tick_notifications();
                        // Server-Resourcepacks kommen oft gerade in dieser Phase (Konfiguration).
                        self.poll_resource_pack_job();
                        let overlay = self.build_notifications();
                        let (rects, _, texts, sprites, _) = self.build_hud();
                        let cursor_fx = self.cursor_fx_frame();
                        let cursor = Some(self.cursor_pos);
                        let hell = 1.0 - self.settings.menu_backdrop_opacity.min(100) as f32 / 100.0;
                        self.profiler.add_ms(Phase::Menu, ui_started.elapsed().as_secs_f32() * 1000.0);
                        self.gpu.as_mut().and_then(|gpu| {
                            gpu.render_menu(&rects, &texts, &sprites, &overlay, cursor, cursor_fx, false, None, false, Some(hell), acquired)
                        })
                    }
                    AppMode::World => {
                        self.menu_world_frozen = false;
                        self.draw_world_frame(Some(acquired), true)
                    }
                    AppMode::Menu => {
                        // Verbunden: die Welt laeuft hinter dem Menue weiter (wie in Vanilla im
                        // Mehrspieler) -- erst ins offscreen_view zeichnen, render_menu zeigt es.
                        // Modul Nachhaltigkeit: die Welt nur einmal zeichnen, danach steht das Bild.
                        let stehend = self.settings.module_sustainability && self.settings.sustain_pause && self.menu_world_frozen;
                        if self.world_backdrop_visible() && !stehend {
                            let _ = self.draw_world_frame(None, false);
                            self.menu_world_frozen = true;
                        }
                        let ui_started = Instant::now();
                        self.tick_notifications();
                        self.poll_resource_pack_job();
                        let mut overlay = self.build_notifications();
                        self.build_notification_panel(&mut overlay);
                        self.build_keybind_advanced(&mut overlay);
                        self.build_color_popup(&mut overlay);
                        self.build_dropdown(&mut overlay);
                        self.build_quick_connect(&mut overlay);
                        self.build_theme_name_dialog(&mut overlay);
                        self.build_theme_delete(&mut overlay);
                        self.build_server_dialog(&mut overlay);
                        self.build_server_context(&mut overlay);
                        self.build_server_drag(&mut overlay);
                        self.build_skin_drag(&mut overlay);
                        self.build_skin_files(&mut overlay);
                        self.build_account_menu(&mut overlay);
                        self.build_account_manager(&mut overlay);
                        self.build_account_context(&mut overlay);
                        self.build_disconnect_popup(&mut overlay);
                        self.build_dialog_overlay(&mut overlay);
                        // Liegt der Zeiger auf etwas, das UEBER dem Inhalt schwebt (Kontextmenue,
                        // Popup, Panel, Benachrichtigung), zeigt er fuer den Inhalt darunter auf
                        // nichts: kein Hover-Rand, keine Hover-Flaeche, kein Tooltip. Eine
                        // Stelle fuer alle Ebenen, statt jede Liste einzeln zu pruefen.
                        let echt = self.cursor_pos;
                        let verdeckt = overlay.groups.iter().flat_map(|g| g.rects.iter()).any(|r| {
                            (r.color[3] > 0.0 || r.border > 0.0) && r.soft == 0.0 && r.rect.contains(echt.0, echt.1)
                        });
                        if verdeckt {
                            self.cursor_pos = (f32::NEG_INFINITY, f32::NEG_INFINITY);
                        }
                        let (rects, texts, sprites, cursor, gruppen) = self.build_menu_ui();
                        self.cursor_pos = echt;
                        let cursor = cursor.map(|_| echt);
                        // Unter Benachrichtigungen, Farbfenster & Co., aber ueber dem Editor bzw.
                        // der Serverliste.
                        for g in gruppen.into_iter().rev() {
                            overlay.groups.insert(0, g);
                        }
                        self.update_falling_glyphs(&texts, &mut overlay);
                        // Ebenso ganz unten: ueber dem Pack-Bild, unter jedem Fenster.
                        if let Some(g) = self.build_skin_gears() {
                            overlay.groups.insert(0, g);
                        }
                        let cursor_fx = self.cursor_fx_frame();
                        let show_wallpaper = self.wallpaper_visible();
                        let show_world =
                            self.world_backdrop_visible().then(|| 1.0 - self.settings.menu_backdrop_opacity.min(100) as f32 / 100.0);
                        let show_last_view = self.last_view_visible();
                        let ui_ms = ui_started.elapsed().as_secs_f32() * 1000.0;
                        self.profiler.add_ms(Phase::Menu, ui_ms);
                        self.gpu.as_mut().and_then(|gpu| {
                            gpu.render_menu(&rects, &texts, &sprites, &overlay, cursor, cursor_fx, show_wallpaper, show_world, show_last_view, None, acquired)
                        })
                    }
                };
                if let Some(timing) = timing {
                    self.block_animations.presented(timing.present_ns);
                    let now = Instant::now();
                    self.frame_stats.record(now, timing.idle_ms);
                    let sort_ms = match self.mode {
                        AppMode::World => self.gpu.as_ref().map(|g| g.last_sort_ms).unwrap_or(0.0),
                        AppMode::Menu => 0.0,
                    };
                    self.profiler.add_ms(Phase::Sort, sort_ms);
                    self.profiler.add_ms(Phase::Encode, timing.encode_ms - sort_ms);
                    self.profiler.add_ms(Phase::Submit, timing.submit_ms);
                    self.profiler.add_ms(Phase::Present, timing.present_ms);
                    let ziel = if presented_at.is_some() {
                        self.vblank.as_ref().and_then(|v| v.latest()).and_then(|(_, period)| self.tear_target_phase(period))
                    } else {
                        None
                    };
                    self.profiler.set_display_timing(timing.present_ns, timing.gpu_done_ns, ziel);
                    if let Some(gpu) = self.gpu.as_mut() {
                        if let Some((passes, total)) = gpu.gpu_timer.as_mut().and_then(|t| t.collect(&gpu.device)) {
                            self.frame_stats.record_gpu(now, total);
                            self.profiler.set_gpu_passes(&passes, total);
                        }
                    }
                    if let (Some(ready), Some(at)) = (timing.ready_ns, presented_at) {
                        self.adapt_tear_lead(ready, at);
                    }
                }
                let target = self.profiler_target_interval();
                self.profiler.end_frame(target);
            }
            _ => {}
        }
    }

    // Der Text des Feldes, in das eine Taste gerade schreiben wuerde -- dieselbe Reihenfolge
    // wie die Tastenbehandlung in handle_window_event. Die Kennung trennt die Felder, damit
    // ein Feldwechsel (Tab) nicht als Tippen zaehlt. Der Chat fehlt: er spielt seine Klaenge
    // selbst (s. chat_input_edit).
    pub(crate) fn typing_snapshot(&self) -> Option<(u8, String)> {
        if let Some((i, text)) = self.dialog_field_text() {
            return Some((100u8.saturating_add(i.min(150) as u8), text));
        }
        if let Some((_, buf)) = &self.editing_step {
            return Some((1, buf.clone()));
        }
        if let Some((_, buf)) = &self.editing_value {
            return Some((2, buf.clone()));
        }
        if self.chat_input.is_some() {
            return None;
        }
        if self.mode == AppMode::World {
            if let Some(b) = &self.book_edit {
                let seite = b.pages.get(b.page).map_or("", String::as_str);
                return Some((3, format!("{}\u{0}{}", b.title, seite)));
            }
        }
        if let Some(sign) = &self.sign_edit {
            return Some((4 + sign.row as u8, sign.lines.join("\n")));
        }
        if self.mode == AppMode::World && self.anvil_typing() {
            return Some((10, self.anvil_name.clone()));
        }
        if self.mode == AppMode::World && self.creative_search_focused() {
            return Some((11, self.creative.search.clone()));
        }
        if let Some(edit) = &self.server_edit {
            return Some(match edit.focus {
                ServerField::Name => (12, edit.name.text().to_string()),
                ServerField::Address => (13, edit.address.text().to_string()),
            });
        }
        if let Some(d) = &self.theme_name_dialog {
            return Some((14, d.field.text().to_string()));
        }
        if self.quick_typing() {
            return Some((15, self.quick_field.text().to_string()));
        }
        if let Some((_, buf)) = &self.input_field {
            return Some((16, buf.clone()));
        }
        if self.screen == MenuScreen::SettingsLightLevels {
            return Some((17, self.light_search.clone()));
        }
        None
    }

    // Nach einer Taste: Text dazugekommen -> Tippen, weniger geworden -> Loeschen.
    pub(crate) fn typing_feedback(&mut self, vorher: Option<(u8, String)>) {
        let (Some((feld, alt)), Some((feld_neu, neu))) = (vorher, self.typing_snapshot()) else { return };
        if feld != feld_neu || alt == neu {
            return;
        }
        let laenge = |t: &str| t.chars().count();
        let klang = if laenge(&neu) < laenge(&alt) { ui_sound::UiSound::Erase } else { ui_sound::UiSound::Type };
        self.play_ui_sound(klang);
    }
}
