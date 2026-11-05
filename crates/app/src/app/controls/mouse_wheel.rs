// Mausbewegung, Bundle, Mausrad und Lautstaerke.

use super::*;

impl App {
    // Rohe Maus-Deltas: im Menu-Modus 2D-Cursor, im World-Modus Kamera-Blickrichtung.
    pub(crate) fn handle_mouse_delta(&mut self, delta: (f64, f64)) {
        if !self.owns_mouse() {
            return; // Maus gehoert gerade dem Desktop (Alt-Tab / kein Fullscreen)
        }
        // Beim Verbinden ist die Welt ein Standbild; die Maus bewegt den Zeiger (Abbrechen-
        // Knopf, Dialoge des Servers).
        if self.connecting_view() {
            self.move_cursor(delta);
            return;
        }
        match self.mode {
            AppMode::Menu => {
                self.move_cursor(delta);
                // Laeuft ein HUD-Drag, folgt das Element dem Cursor.
                self.update_hud_drag();
                self.crosshair_painter_drag();
            }
            AppMode::World if self.screen_overlay_open() => {
                // Offenes Overlay (Inventar oder Modul-Menue): die Maus bewegt den Zeiger,
                // nicht den Kopf.
                self.move_cursor(delta);
                // Laeuft ein Verteilen-Ziehen, gehoert jeder ueberfahrene Slot dazu.
                self.inventory_drag_collect();
                self.inventory_throw_sweep(false);
                // Am Schieber der Handelsliste gezogen: die Liste folgt. Ebenso im Kreativinventar.
                self.merchant_drag();
                self.creative_drag();
                self.book_edit_drag();
            }
            // Offener Chat: wie in Vanilla (ChatScreen) gehoert die Maus dem Zeiger -- sonst
            // liesse sich kein Link anklicken, und der Kopf drehte sich beim Tippen mit.
            AppMode::World if self.chat_input.is_some() => self.move_cursor(delta),
            // Hand-Debug: die Maus fuehrt den Zeiger zu den Knoepfen im Schwarzen, der Kopf steht.
            AppMode::World if self.hand_debug_scale().is_some() => self.move_cursor(delta),
            // Offenes Spielmodus-Rad: die Maus waehlt darin, der Kopf steht.
            AppMode::World if self.gamemode_wheel.is_some() => self.gamemode_wheel_mouse(delta),
            AppMode::World => {
                // mouse_sensitivity ist auf ein Radiant-Delta pro Pixel kalibriert (unveraendert
                // seit vor der Grad-Umstellung der Kamera, s. camera.rs) -- deshalb hier erst in
                // Radiant rechnen und dann fuers Grad-Feld umrechnen, statt die Kalibrierung
                // aller Settings-Presets anzufassen.
                let zoom = zoom_sens_factor(self.settings.zoom_sens_percent, self.zoom_amount);
                let (sens_x, sens_y) = camera_sensitivities(&self.settings);
                // Geloeste Kamera (Freelook, Schulterkamera frei vom Koerper): nur sie dreht sich.
                if let Some((yaw, pitch)) = self.cam_look.as_mut() {
                    *yaw += (delta.0 as f32 * sens_x * zoom).to_degrees();
                    *pitch = (*pitch - (delta.1 as f32 * sens_y * zoom).to_degrees()).clamp(-MAX_PITCH, MAX_PITCH);
                    self.camera.view_look = self.cam_look;
                    return;
                }
                self.camera.yaw += (delta.0 as f32 * sens_x * zoom).to_degrees();
                self.camera.pitch -= (delta.1 as f32 * sens_y * zoom).to_degrees();
                self.camera.pitch = self.camera.pitch.clamp(-MAX_PITCH, MAX_PITCH);
                // Entity.changeLookDirection -> vehicle.onPassengerLookAround: im Boot bleibt
                // der Kopf innerhalb von 105 Grad zur Fahrtrichtung.
                self.clamp_yaw_to_boat();
            }
        }
    }

    // Gehoert das Rad gerade einer Liste unter dem Zeiger (oder einem Bildschirm, auf dem es
    // gar nichts tut)? Dann ist es Oberflaeche und laeuft ueber winit, weil dort der
    // gezeichnete Zeiger zaehlt. Sonst ist es Steuerung (s. wheel_rasten). Genau die Faelle,
    // die der winit-Zweig von WindowEvent::MouseWheel selbst scrollt -- EINE Stelle fuer beide
    // Eingabewege, damit keine Raste doppelt oder gar nicht ankommt.
    // BundleMouseActions.onMouseScrolled: ueber einem nicht leeren Buendel waehlt jede Raste das
    // naechste bzw. vorige der gezeigten Items, und der Server erfaehrt die Wahl (fuers
    // Herausnehmen). true, wenn das Rad damit verbraucht ist.
    pub(crate) fn bundle_scroll(&mut self, rasten: f32) -> bool {
        if rasten.abs() < 0.01 || self.inventory.cursor.is_some() {
            return false;
        }
        let Some(gpu) = self.gpu.as_ref() else { return false };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let Some(slot) = layout.slot_at(self.cursor_pos.0, self.cursor_pos.1) else { return false };
        let Some(b) = self.inventory.slot_at_index_mut(slot).filter(|b| world::bundle::is_bundle(b)) else { return false };
        let n = world::bundle::items_to_show(world::bundle::contents(b).len()) as i32;
        if n == 0 {
            return false;
        }
        // ScrollWheelHandler.getNextScrollWheelSelection: hoch geht zurueck, runter weiter; aus
        // "nichts gewaehlt" fuehrt hoch zum letzten, runter zum ersten.
        let schritt = if rasten > 0.0 { -1 } else { 1 };
        let neu = (b.bundle_selected + schritt).max(-1).rem_euclid(n);
        b.bundle_selected = neu;
        self.bundle_hover_slot = Some(slot);
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::select_bundle_item_payload(slot as i32, neu));
        }
        true
    }

    // BundleMouseActions.onStopHovering: verlaesst der Zeiger das Buendel (oder schliesst das
    // Inventar), ist nichts mehr gewaehlt.
    pub(crate) fn tick_bundle_hover(&mut self) {
        let Some(slot) = self.bundle_hover_slot else { return };
        let darueber = self.screen_overlay_open()
            && self.gpu.as_ref().is_some_and(|gpu| {
                inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size())).slot_at(self.cursor_pos.0, self.cursor_pos.1) == Some(slot)
            });
        if darueber {
            return;
        }
        self.bundle_hover_slot = None;
        if let Some(b) = self.inventory.slot_at_index_mut(slot).filter(|b| world::bundle::is_bundle(b) && b.bundle_selected >= 0) {
            b.bundle_selected = -1;
            if let Some(tx) = &self.outgoing_tx {
                let _ = tx.send(protocol::packets::select_bundle_item_payload(slot as i32, -1));
            }
        }
    }

    pub(crate) fn wheel_scrolls_list(&self) -> bool {
        if self.dropdown_active().is_some() {
            return true;
        }
        let (px, py) = self.pointer();
        match self.mode {
            AppMode::Menu => {
                (self.screen == MenuScreen::Connect && server_panel().contains(px, py))
                    // Einstellungsseiten im Zeilenraster: auch wenn alles hineinpasst, zieht das
                    // Rad die Seite gegen Widerstand ueber den Rand (s. scroll_settings).
                    || self.screen.settings_page().is_some()
                    || self.screen == MenuScreen::SettingsModules
                    // Einstellungen mit eigenen Listen: ueber ihnen scrollt das Rad die Liste,
                    // nie die Lautstaerke -- auch wenn die Liste gerade ganz hineinpasst.
                    || (self.screen == MenuScreen::SettingsSkin
                        && (self.skin_files.is_some()
                            || [skin_tab::Column::Available, skin_tab::Column::Selected].into_iter().any(|c| skin_column_rect(c).contains(px, py))))
                    || self.screen == MenuScreen::SettingsParticles
            }
            // Chat, Modul-Menue, Inventar. Im Inventar scrollt Vanilla gar nichts -- auch
            // keine Lautstaerke.
            AppMode::World => self.chat_input.is_some() || self.screen_overlay_open(),
        }
    }

    // Ganze Rasten abzaehlen; Bruchteile (hochaufloesende Raeder, Touchpads) sammeln sich, bis
    // eine volle Raste zusammen ist -- wie in Vanilla.
    pub(crate) fn wheel_rasten(&mut self, rasten: f32) {
        self.wheel_accum += rasten;
        while self.wheel_accum >= 1.0 {
            self.wheel_accum -= 1.0;
            self.wheel_notch(true);
        }
        while self.wheel_accum <= -1.0 {
            self.wheel_accum += 1.0;
            self.wheel_notch(false);
        }
    }

    // Eine volle Mausrad-Raste, die keine Liste verbraucht hat (s. WindowEvent::MouseWheel).
    // Im Spiel geht sie an die Aktionen auf dem Rad -- ein Druck mit sofortigem Loslassen,
    // denn ein Rad kennt kein Halten. Liegt nichts darauf (oder im Menue), stellt sie die
    // Lautstaerke.
    pub(crate) fn wheel_notch(&mut self, hoch: bool) {
        let bind = Bind::Mouse(if hoch { BindMouse::WheelUp } else { BindMouse::WheelDown });
        let kb = self.settings.keybinds;
        // Liegt die Lautstaerke-Aktion auf dem Rad, gilt sie ueberall, auch im Menue.
        if kb.binds(KeyBindAction::VolumeUp).any(|b| b == bind) {
            self.adjust_volume(1, true);
            return;
        }
        if kb.binds(KeyBindAction::VolumeDown).any(|b| b == bind) {
            self.adjust_volume(-1, true);
            return;
        }
        if self.mode == AppMode::World && kb.is_bound(bind) {
            self.apply_bind(bind, true);
            self.apply_bind(bind, false);
            return;
        }
        let schritt = VOLUME_WHEEL_STEP as i32;
        self.adjust_volume(if hoch { schritt } else { -schritt }, true);
    }

    // Hauptlautstaerke um `delta` Prozentpunkte verstellen und links anzeigen. `speichern`
    // schreibt settings.conf sofort; beim Halten einer Taste passiert das erst beim Loslassen
    // (s. tick_volume_keys), statt zwanzigmal je Sekunde.
    pub(crate) fn adjust_volume(&mut self, delta: i32, speichern: bool) {
        let vorher = self.settings.master_volume;
        self.settings.master_volume = (vorher + delta as f32).clamp(0.0, 100.0);
        // Auch am Anschlag anzeigen: sonst wirkt die Eingabe dort kaputt.
        let jetzt = Instant::now();
        // Einfahren nur, wenn die Anzeige gerade nicht zu sehen ist. Steht sie schon (oder
        // blendet gerade aus), bleibt sie stehen und zeigt nur den neuen Wert -- sonst
        // spraenge sie bei jedem Druck neu von links herein.
        let sichtbar = self.volume_shown_at.is_some_and(|t| jetzt.duration_since(t).as_secs_f32() < VOLUME_DISPLAY_HOLD + VOLUME_DISPLAY_FADE);
        if !sichtbar {
            self.volume_entered_at = Some(jetzt);
        }
        self.volume_shown_at = Some(jetzt);
        if self.settings.master_volume != vorher {
            if speichern {
                self.persist_settings();
            } else {
                self.volume_unsaved = true;
            }
        }
    }

    // Die Tasten auf "Lautstaerke hoch/runter": ein Schritt beim Druck, beim Halten nach einer
    // kurzen Pause weitere. Laeuft ueber binds_down statt ueber apply_key_action, weil das
    // nur im Spiel greift -- die Lautstaerke soll auch im Menue gehen. Waehrend einer
    // Texteingabe nicht: eine Buchstabentaste darauf verstellte sonst beim Tippen den Ton.
    pub(crate) fn tick_volume_keys(&mut self) {
        let texteingabe = self.chat_input.is_some()
            || self.editing_value.is_some()
            || self.editing_step.is_some()
            || self.awaiting_keybind.is_some()
            || self.input_field.is_some()
            || self.server_edit.is_some();
        let kb = self.settings.keybinds;
        let unten = |a: KeyBindAction| kb.binds(a).any(|b| self.binds_down.contains(&b));
        let richtung = match (unten(KeyBindAction::VolumeUp), unten(KeyBindAction::VolumeDown)) {
            (true, false) => Some(1),
            (false, true) => Some(-1),
            _ => None,
        };
        let Some(richtung) = richtung.filter(|_| !texteingabe) else {
            self.volume_key_repeat = None;
            if std::mem::take(&mut self.volume_unsaved) {
                self.persist_settings();
            }
            return;
        };
        let jetzt = Instant::now();
        match self.volume_key_repeat {
            Some((r, naechster)) if r == richtung => {
                if jetzt >= naechster {
                    self.adjust_volume(richtung, false);
                    self.volume_key_repeat = Some((richtung, jetzt + VOLUME_KEY_REPEAT));
                }
            }
            _ => {
                self.adjust_volume(richtung, false);
                self.volume_key_repeat = Some((richtung, jetzt + VOLUME_KEY_REPEAT_DELAY));
            }
        }
    }

    // Die Lautstaerke-Anzeige links (s. wheel_notch): gleitet herein, steht kurz und blendet
    // aus. Als Overlay-Gruppe, damit sie auch ueber Menuetext liegt (s. UiOverlay).
    pub(crate) fn build_volume_display(&self, overlay: &mut UiOverlay) {
        let Some(seit) = self.volume_shown_at.map(|t| t.elapsed().as_secs_f32()) else { return };
        let Some(gpu) = self.gpu.as_ref() else { return };
        let alpha = if seit < VOLUME_DISPLAY_HOLD { 1.0 } else { 1.0 - (seit - VOLUME_DISPLAY_HOLD) / VOLUME_DISPLAY_FADE };
        if alpha <= 0.0 {
            return;
        }
        let eingefahren = self.volume_entered_at.map_or(f32::INFINITY, |t| t.elapsed().as_secs_f32());
        let rein = cursor_fx::Easing::OutQuint.apply(eingefahren / VOLUME_DISPLAY_ENTER);
        let (_, screen_h) = gpu.size();
        let w = VOLUME_DISPLAY_W;
        let pad = NOTIFICATION_PADDING;
        let klein = gpu.text_height(UI_SCALE_SMALL);
        let gross = gpu.text_height(UI_SCALE_NORMAL);
        let h = pad + klein + 6.0 + gross + 10.0 + VOLUME_BAR_H + pad;
        let x = (VOLUME_DISPLAY_MARGIN - (1.0 - rein) * (w + VOLUME_DISPLAY_MARGIN)).round();
        let y = ((screen_h - h) / 2.0).round();
        let a = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * alpha];
        // Farben aus dem Theme: Flaeche und Rand wie ein Panel, Balken im Akzent.
        use theme::Slot;
        let tg = theme::get;
        let mut g = UiOverlayGroup { rects: Vec::new(), texts: Vec::new(), sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None };
        g.rects.push(UiRect::framed(Rect { x, y, w, h }, a(theme::with_alpha(tg(Slot::BgPanel), 0.94)), 6.0, 1.0, a(tg(Slot::Border1))));
        g.texts.push(UiText { text: "Lautstärke".to_string(), x: x + pad, y: y + pad, color: a(tg(Slot::AccentText)), scale: UI_SCALE_SMALL });
        let wert = self.settings.master_volume;
        g.texts.push(UiText { text: volume_text(wert), x: x + pad, y: y + pad + klein + 6.0, color: a(theme::text()), scale: UI_SCALE_NORMAL });
        let balken = Rect { x: x + pad, y: y + h - pad - VOLUME_BAR_H, w: w - 2.0 * pad, h: VOLUME_BAR_H };
        g.rects.push(UiRect { rect: balken, color: a(tg(Slot::SurfaceSunken)), ..UiRect::FLAT });
        g.rects.push(UiRect { rect: Rect { w: balken.w * wert / 100.0, ..balken }, color: a(theme::accent()), ..UiRect::FLAT });
        overlay.groups.push(g);
    }
}
