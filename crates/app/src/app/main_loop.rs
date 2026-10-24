// Ein Durchlauf der Hauptschleife.

use crate::*;

impl App {
    // Ein Durchlauf der Fensterschleife (s. about_to_wait).
    pub(crate) fn loop_step(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_none() {
            return;
        }

        use profiler::Phase;
        self.profiler.loop_call();
        let t = Instant::now();
        self.drain_events();
        self.poll_background_tabs();
        self.check_packet_gap();
        self.tick_reconnect_hold();
        self.profiler.add(Phase::DrainEvents, t);
        let t = Instant::now();
        self.drain_auth_events();
        self.drain_server_status();
        self.drain_addons();
        self.addon_frame();
        self.profiler.add(Phase::DrainAuth, t);
        let t = Instant::now();
        self.update_hover_sound();
        self.tick_ui_anim();
        self.advance_orbit_test();
        self.update_tracked_sounds();
        self.voice_tick();
        self.profiler.add(Phase::LoopMisc, t);
        let t = Instant::now();
        self.enforce_render_limits();
        self.profiler.add(Phase::RenderLimits, t);
        let t = Instant::now();
        self.mesh_pending_columns();
        self.profiler.add(Phase::Mesh, t);
        let t_misc = Instant::now();
        // Nur abtasten, waehrend das Debug-Overlay sichtbar ist -- sonst wuerden ungenutzte
        // Ringpuffer mit Loechern voll Stille laufen, sobald man F3 mal wieder einblendet.
        if self.debug_overlay {
            self.sys_stats.maybe_sample();
        }

        // Raw-Tastatur immer leeren (verhindert Stau), aber nur im World-Modus mit
        // Fokus anwenden: evdev liest global, ohne das duerfte Tippen in einem anderen
        // Fenster nach Alt-Tab weiter unseren Spieler steuern. Der Zustand wird bei
        // Fokusverlust und Modewechsel geleert (s. clear_controls), damit keine gehaltene
        // Taste "haengen" bleibt, deren Loslassen woanders hinging.
        // chat_input.is_none(): waehrend offener Chat-Eingabe (s. open_chat) duerfen Raw-
        // Tasten nicht mehr die Spielfigur steuern -- sonst wuerde z.B. "w" gleichzeitig
        // ein Zeichen tippen UND vorwaerts laufen.
        self.tick_volume_keys();
        self.poll_wallpaper();
        self.poll_last_view();
        self.tick_connect_view_capture();
        self.tick_hero_scene();
        // Nur bei offenem Skin-Tab (Einstellungen > Skin): dort sieht man das Ergebnis, und
        // sonst liest niemand jede Sekunde den Ordner.
        let skin_tab = matches!(self.mode, AppMode::Menu) && self.screen == MenuScreen::SettingsSkin;
        self.tick_skin_tab();
        self.refresh_cursor_visibility();
        if let Some(done) = self.skin_zips.poll(skins_dir().as_deref().filter(|_| skin_tab && self.settings.skin_auto_unzip)) {
            self.notify(done.notification());
        }
        self.profiler.add(Phase::LoopMisc, t_misc);
        let t_input = Instant::now();
        // Neu angesteckte Geraete: sind schon an die Kanaele gehaengt, hier nur die Meldung.
        let neue_geraete = self.input_watcher.as_ref().map(|w| w.drain()).unwrap_or_default();
        for geraet in neue_geraete {
            self.push_log(format!("Eingabegerät erfasst: {} ({})", geraet.name, geraet.kind_label()));
            self.notify(notifications::Notification::success(format!("{} erfasst!", geraet.name), geraet.kind_label()));
        }
        if self.caps_lock_recheck.is_some_and(|t| Instant::now() >= t) {
            self.caps_lock_recheck = None;
            self.caps_lock.recheck();
        }
        let raw_keys = self.raw_keyboard.as_ref().map(|kb| kb.drain()).unwrap_or_default();
        if self.focused {
            for &(code, pressed) in &raw_keys {
                self.track_bind(Bind::Key(code), pressed);
            }
        }
        if self.focused && self.mode == AppMode::World && self.chat_input.is_none() && self.sign_edit.is_none() && !self.text_field_focused() {
            for (code, pressed) in raw_keys {
                // input-log (aus, bis im Modul-Menue eingeschaltet): dieselbe Schranke wie
                // apply_raw_key -- nur echte Spieltasten, kein Chat/Textfeld, nur bei Fokus.
                self.input_log.key(code, pressed);
                self.apply_raw_key(code, pressed);
            }
        }

        // Maus aus dem Raw-Pfad: Bewegung UND Tasten in EINEM geordneten Durchgang. Getrennt
        // ging es nicht -- wurde erst die ganze Frame-Bewegung auf den Zeiger gerechnet und
        // danach geklickt, landete ein Klick mitten in einer schnellen Bewegung an der
        // Position vom Frame-Ende (s. raw_input::RawMouseEvent). Menue-Klicks bleiben bewusst
        // bei winit -- dort zaehlt der gezeichnete Cursor, nicht die Latenz.
        //
        // Immer LEEREN, auch ohne Fokus: sonst stauen sich die Events waehrend Alt-Tab und
        // die Kamera springt beim Zurueckwechseln. Angewendet wird nur mit Fokus; was die
        // Bewegung bewirkt (Kamera oder Zeiger), entscheidet handle_mouse_delta.
        let raw_events = self.raw_mouse.as_ref().map(|m| m.drain()).unwrap_or_default();
        let tasten_erlaubt = self.focused && self.mode == AppMode::World && self.chat_input.is_none() && self.sign_edit.is_none();
        for event in raw_events {
            match event {
                // Auch bei offenem Chat: dann bewegt die Maus den Zeiger statt der Kamera (die
                // Entscheidung faellt in handle_mouse_delta). Frueher wurde die Bewegung hier
                // verworfen -- der Zeiger im Chat stand still.
                raw_input::RawMouseEvent::Motion(dx, dy) => self.handle_mouse_delta((dx, dy)),
                // Das Rad als Steuerung -- im Spiel wie im Menue (dort stellt es die Lautstaerke).
                // Scrollt es gerade eine Liste, ist es Oberflaeche und laeuft ueber winit (s.
                // wheel_scrolls_list). Beim Neubelegen faengt winit die Raste als neue Belegung
                // ab; hier darf sie dann nicht zusaetzlich die Lautstaerke verstellen.
                raw_input::RawMouseEvent::Wheel(rasten) => {
                    if self.focused && self.awaiting_keybind.is_none() && !self.wheel_scrolls_list() && !self.dialog_shown() && self.code_of_conduct.is_none() {
                        self.input_log.wheel(rasten);
                        self.wheel_rasten(rasten);
                    }
                }
                raw_input::RawMouseEvent::Button(button, pressed) => {
                    if self.focused {
                        self.track_bind(Bind::Mouse(BindMouse::from_raw(button)), pressed);
                    }
                    if !tasten_erlaubt {
                        continue;
                    }
                    // input-log: nur echte Spiel-Klicks (fokussiert, im Spiel, kein Textfeld).
                    self.input_log.mouse_button(button, pressed);
                    // Offenes Modul-Menue: der Klick gehoert der Oberflaeche und wird im
                    // winit-Pfad behandelt -- hier nur sicherstellen, dass er nicht ZUSAETZLICH
                    // als Angriff durchgeht (sonst schlaegt man durch das offene Menue hindurch).
                    // Ebenso bei einem Dialog des Servers (s. dialog_press).
                    if self.module_menu_open || self.dialog_shown() || self.code_of_conduct.is_some() {
                        continue;
                    }
                    if self.inventory.screen_open {
                        // Offener Inventarbildschirm: die Maus bedient Slots statt zu schlagen.
                        // Loslassen zaehlt hier genauso wie Druecken -- ein Klick mit vollem
                        // Zeiger wird erst dort entschieden (s. inventory_release).
                        let taste = match button {
                            raw_input::MouseButton::Left => Some(0),
                            raw_input::MouseButton::Right => Some(1),
                            _ => None,
                        };
                        // Ein Druck auf eine Benachrichtigung ist kein Slot-Klick (s. den
                        // winit-Pfad; derselbe Klick kommt ueber beide Wege, notifications
                        // wertet ihn nur einmal aus).
                        if taste == Some(0) {
                            let jetzt = Instant::now();
                            let geschluckt = if pressed { self.notifications.press(jetzt) } else { self.notifications.release(jetzt) };
                            if geschluckt {
                                continue;
                            }
                        }
                        if let Some(taste) = taste {
                            if pressed {
                                self.inventory_click(taste);
                            } else {
                                self.inventory_release(taste);
                            }
                        } else {
                            self.apply_inventory_mouse_bind(Bind::Mouse(BindMouse::from_raw(button)), pressed);
                        }
                        continue;
                    }
                    // Gebundene Maustasten laufen ueber denselben evdev-Strom wie der Klick --
                    // damit nimmt z.B. Sprint auf der Daumentaste denselben Low-Latency-Pfad wie
                    // die Bewegung, statt ueber den laggy X11-Weg zu kommen.
                    self.apply_bind(Bind::Mouse(BindMouse::from_raw(button)), pressed);
                }
            }
        }

        self.tick_autoclicker();
        self.profiler.add(Phase::Input, t_input);
        let t = Instant::now();
        self.update_movement();
        self.profiler.add(Phase::Movement, t);

        // Modul Nachhaltigkeit: ohne Fokus keine Bilder. Spiel, Netz und Sprachchat laufen
        // weiter -- die Schleife wacht dafuer alle paar Millisekunden auf (und zu jedem Tick).
        if self.settings.module_sustainability && self.settings.sustain_background && !self.focused {
            let wach = Instant::now() + Duration::from_millis(10);
            self.next_frame_time = wach;
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_wake(wach, self.tick_deadline)));
            return;
        }

        // Taktet der Present-Modus selbst (Fifo), lassen wir ihn: wir zeichnen einfach
        // durch und schlafen im blockierenden get_current_texture() bis zum VBlank. Poll
        // ist hier also kein Busy-Loop -- gewartet wird im Treiber, nicht in der Schleife.
        if self.gpu.as_ref().is_some_and(|gpu| gpu.present_mode_self_paces()) {
            if let Some(gpu) = &self.gpu {
                gpu.window.request_redraw();
            }
            self.next_frame_time = Instant::now();
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }

        if self.tear_control_active() {
            self.schedule_tear_frame(event_loop);
            return;
        }
        self.tear_slot_ns = None;

        let now = Instant::now();
        if now >= self.next_frame_time {
            let interval = self.effective_frame_interval();
            self.next_frame_time += interval;
            // Nicht aufholen, sobald dieser Frame mehr als einen halben Takt zu spaet dran ist:
            // der naechste Termin laege sonst fast sofort danach, und auf einen langen Frame
            // folgten zwei viel zu kurze -- im Frame-Profiler als 16 ms -> 4 ms -> 5 ms
            // gemessen, sichtbar als zweiter Ruckler direkt nach dem ersten. Stattdessen das
            // Raster neu ab jetzt anlegen. Kleine Verspaetungen holt der alte Termin weiter
            // auf, damit die Bildrate im Mittel stimmt.
            if self.next_frame_time < now + interval / 2 {
                self.next_frame_time = now + interval;
            }
            if let Some(gpu) = &self.gpu {
                gpu.window.request_redraw();
            }
        }

        // Auch fuer Tick-Termine aufwachen, nicht nur fuer Frame-Termine (s. next_wake).
        event_loop.set_control_flow(ControlFlow::WaitUntil(next_wake(self.next_frame_time, self.tick_deadline)));
    }
}
