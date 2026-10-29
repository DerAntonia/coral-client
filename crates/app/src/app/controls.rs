// Steuerung: Tastenbelegungen, Maus, Hotbar, Klicks im Inventar und im Rezeptbuch, Scrollen.

use crate::*;

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
    pub(crate) fn place_cursor(&mut self, pos: (f32, f32)) {
        self.cursor_pos = pos;
        if self.focused && !self.owns_mouse() {
            if let Some(gpu) = self.gpu.as_ref() {
                let _ = gpu.window.set_cursor_position(winit::dpi::PhysicalPosition::new(pos.0 as f64, pos.1 as f64));
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

    // Escape: kontextabhaengig. In der Welt -> Pausemenue (Main). Im Menue -> je nach
    // Screen eine Ebene zurueck; auf Main entweder ins Spiel zurueck (verbunden) oder
    // nichts (Startbildschirm, es gibt kein "dahinter").
    pub(crate) fn on_escape(&mut self) {
        // Dialoge des Servers liegen ueber allem (s. dialog_escape).
        if self.dialog_escape() {
            return;
        }
        // Beim Verbinden tut Esc nichts -- abgebrochen wird mit dem Knopf im Overlay (s.
        // connect_cancel_rect). Ein Dialog des Servers bekommt Esc oben trotzdem.
        if self.connecting_view() {
            return;
        }
        if self.disconnect_popup.take().is_some() || self.theme_delete.take().is_some() || self.theme_name_dialog.take().is_some() {
            self.play_ui_sound(ui_sound::UiSound::Close);
            return;
        }
        if self.quick_connect_open {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.quick_connect_open = false;
            return;
        }
        if self.server_drag.take().is_some() {
            return;
        }
        if self.skin_files.take().is_some() {
            self.play_ui_sound(ui_sound::UiSound::Close);
            return;
        }
        if self.skin_drag.take().is_some() {
            return;
        }
        if self.account_menu_open {
            self.account_menu_open = false;
            return;
        }
        if self.account_context.take().is_some() {
            return;
        }
        if self.account_manager_open {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.account_manager_open = false;
            return;
        }
        if self.keybind_advanced.take().is_some() {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.awaiting_keybind = None;
            return;
        }
        if self.dropdown.take().is_some() {
            self.play_ui_sound(ui_sound::UiSound::Close);
            return;
        }
        // Offenes Farbfenster oder Benachrichtigungsliste: Escape schliesst nur sie.
        if self.color_popup.take().is_some() {
            self.persist_settings();
            return;
        }
        if self.notification_panel_open {
            self.play_ui_sound(ui_sound::UiSound::Close);
            self.set_notification_panel(false);
            return;
        }
        // Schild-Editor: Escape heisst wie in Vanilla "fertig", nicht "verwerfen".
        if self.sign_edit.is_some() {
            self.finish_sign_edit();
            return;
        }
        if self.book_view.is_some() {
            self.close_book();
            return;
        }
        // Buch mit Feder: Escape verwirft wie in Vanilla (nur "Fertig" speichert).
        if self.book_edit.is_some() {
            self.close_book_editor(false);
            return;
        }
        // Offene Chat-Eingabe: Escape schliesst nur sie (wie Vanilla), oeffnet NICHT
        // zusaetzlich das Pausemenue.
        if self.chat_input.is_some() {
            self.close_chat();
            return;
        }
        // Offenes Modul-Menue: Escape schliesst nur das Overlay, wie bei jedem Screen --
        // und vor allem NICHT zusaetzlich das Pausemenue, sonst landet man mit einem Tastendruck
        // zwei Ebenen weiter weg vom Spiel.
        if self.module_menu_open {
            self.module_menu_open = false;
            return;
        }
        // Offener Inventar-/Containerbildschirm: Escape schliesst ihn (wie Vanilla), statt
        // ins Pausemenue zu springen.
        if self.inventory.screen_open {
            self.close_inventory();
            return;
        }
        match self.mode {
            AppMode::World => {
                self.mode = AppMode::Menu;
                self.screen = MenuScreen::Main;
                self.clear_controls();
                // Der Zeiger erscheint immer in der Bildschirmmitte, nicht dort, wo er beim
                // letzten Menue stand.
                if let Some((w, h)) = self.gpu.as_ref().map(|g| g.size()) {
                    self.place_cursor((w / 2.0, h / 2.0));
                }
            }
            AppMode::Menu => match self.screen {
                // Pixel-Editor: Escape schliesst nur ihn.
                MenuScreen::HudEditor if self.crosshair_painter.is_some() => {
                    self.crosshair_painter = None;
                    self.persist_settings();
                }
                // Offene Element-Einstellungen: Escape schliesst nur sie.
                MenuScreen::HudEditor if self.hud_selected.is_some() => {
                    self.hud_selected = None;
                    self.persist_settings();
                }
                // Dasselbe Prinzip beim offenen Server-Formular: Escape bricht NUR das
                // Bearbeiten ab (wie "Abbrechen"), statt gleich den Connect-Screen zu
                // verlassen und die halbfertige Eingabe mitzunehmen.
                MenuScreen::Connect if self.server_edit.is_some() => {
                    self.play_ui_sound(ui_sound::UiSound::Close);
                    self.server_edit = None;
                }
                MenuScreen::Connect if self.server_context.is_some() => self.server_context = None,
                // Offene Suche: Escape leert sie, die volle Liste kommt zurueck.
                MenuScreen::Connect if self.server_search.is_some() => self.server_search = None,
                // Offenes Namensfeld: Escape bricht nur die Eingabe ab.
                MenuScreen::SettingsConfigs if self.input_field.is_some() => self.input_field = None,
                MenuScreen::Settings
                | MenuScreen::SettingsConfigs
                | MenuScreen::SettingsLightLevels
                | MenuScreen::SettingsParticles
                | MenuScreen::SettingsGeneral
                | MenuScreen::SettingsControls
                | MenuScreen::SettingsVideo
                | MenuScreen::SettingsAudio
                | MenuScreen::SettingsSkin
                | MenuScreen::SettingsInventory
                | MenuScreen::SettingsMisc
                | MenuScreen::SettingsFont
                | MenuScreen::SettingsDesign
                | MenuScreen::SettingsLanguage
                | MenuScreen::SettingsModules
                | MenuScreen::SettingsSensCalc
                | MenuScreen::SettingsShader
                | MenuScreen::SettingsSetup
                | MenuScreen::HudEditor
                | MenuScreen::Account
                | MenuScreen::Connect => self.navigate_back(),
                // Tot: Esc tut nichts -- raus kommt man nur ueber Respawn oder Disconnect.
                MenuScreen::Death => {}
                MenuScreen::Main => {
                    if self.conn_status != ConnStatus::Disconnected {
                        self.mode = AppMode::World;
                        self.restore_held_controls();
                    }
                    // Getrennt: Esc auf dem Startbildschirm tut nichts (Alt+F4 beendet).
                }
            },
        }
    }

    // Eine Menue-Ebene zurueck: Tastenbelegung -> Einstellungen, sonst -> Main. Beendet
    // dabei einen etwaigen HUD-Drag.
    pub(crate) fn navigate_back(&mut self) {
        self.hud_drag_offset = None;
        // Verlassen des HUD-Editors raeumt seinen Zustand auf: naechstes Mal ohne offene
        // Element-Einstellungen starten. Positionen sind schon beim Loslassen gespeichert.
        if self.screen == MenuScreen::HudEditor {
            self.persist_settings();
        }
        self.hud_selected = None;
        // Ebenso Server-Fenster, Rechtsklick-Menue und Ziehen: naechstes Betreten des
        // Connect-Screens soll wieder mit der normalen Liste beginnen, nicht mit einer alten,
        // nie uebernommenen Eingabe.
        self.server_edit = None;
        self.server_context = None;
        self.server_drag = None;
        self.server_search = None;
        // Ebenso das Namensfeld und die Loesch-Nachfrage der Configs-Seite.
        if matches!(self.input_field, Some((InputKind::ConfigName(_), _))) {
            self.input_field = None;
        }
        self.config_delete_armed = false;
        self.screen = match self.screen {
            MenuScreen::SettingsGeneral
            | MenuScreen::SettingsControls
            | MenuScreen::SettingsVideo
            | MenuScreen::SettingsAudio
            | MenuScreen::SettingsSkin
            | MenuScreen::SettingsInventory
            | MenuScreen::SettingsMisc
            | MenuScreen::SettingsFont
            | MenuScreen::SettingsDesign
            | MenuScreen::SettingsLanguage
            | MenuScreen::SettingsModules
            | MenuScreen::SettingsConfigs
            | MenuScreen::SettingsSetup
            | MenuScreen::Settings => self.settings_return,
            MenuScreen::SettingsSensCalc => MenuScreen::SettingsControls,
            MenuScreen::SettingsShader => MenuScreen::SettingsGeneral,
            MenuScreen::SettingsLightLevels => MenuScreen::SettingsShader,
            MenuScreen::SettingsParticles => MenuScreen::SettingsVideo,
            MenuScreen::HudEditor => self.hud_editor_return,
            // Getrennt ist die Server-Liste der Startpunkt, es gibt nichts dahinter.
            MenuScreen::Account | MenuScreen::Connect if self.conn_status == ConnStatus::Disconnected => MenuScreen::Connect,
            _ => MenuScreen::Main,
        };
    }

    // Alle Halte-Zustaende auf "nicht gedrueckt". Noetig bei jedem Wechsel, nach dem ein
    // Loslassen-Event nicht mehr ankommt (Menu/World-Wechsel, Fokusverlust) -- sonst liefe
    // der Spieler weiter, weil das release der gehaltenen Taste woanders hinging.
    pub(crate) fn clear_controls(&mut self) {
        // NUR Halten-Aktionen auf "nicht aktiv" zuruecksetzen: fuer die gilt der
        // physische Tastenzustand direkt, und den kennen wir nach einem Fokusverlust nicht
        // mehr (s. Funktionskommentar) -- ohne Reset liefe der Spieler sonst weiter.
        // Umschalten-Aktionen (z.B. Sprint-Lock) sind dagegen ein bewusst gesetzter Zustand,
        // kein "gerade gedrueckt" -- Escape/Pausemenue oeffnen darf den nicht aufheben,
        // sonst muesste man nach jedem Menuebesuch Sprint neu togglen. Nur ein echter
        // Neustart der App soll den Umschalten-Zustand zuruecksetzen (Felder starten dann
        // ohnehin frisch mit false).
        for action in KeyBindAction::ALL {
            if self.settings.keybind_modes.get(action) == KeybindMode::Hold {
                self.set_action_active(action, false);
            }
        }
        // Ein laufender Quicklook endet wie jede gehaltene Taste.
        if self.quicklook_hold() && self.keybind_physical_down[KeyBindAction::TogglePerspective.index()] {
            self.perspective = Perspective::FirstPerson;
        }
        // Auch die rohe Tastenverfolgung leeren: sonst wuerde nach einem Fokuswechsel eine
        // eigentlich losgelassene Taste im Umschalten-Modus als "noch unten" gelten und der
        // naechste Druck loest keine steigende Flanke aus (s. apply_key_action).
        self.keybind_physical_down = [false; KeyBindAction::ALL.len()];
    }

    pub(crate) fn track_bind(&mut self, bind: Bind, pressed: bool) {
        self.binds_down.retain(|b| *b != bind);
        if pressed {
            self.binds_down.push(bind);
        }
        // Feststelltaste im Spiel: die Sperre, die das System beim Druecken setzt, sofort wieder
        // loesen -- und kurz darauf noch einmal, falls der X-Server den Druck erst nach uns
        // verarbeitet (evdev liest parallel zu ihm). So bleibt auch beim Halten die Lampe aus.
        // Wo getippt wird (Chat, Schild, Textfelder), bleibt sie wie gewohnt.
        if bind == Bind::Key(KeyCode::CapsLock)
            && self.settings.caps_lock_blocked
            && self.focused
            && self.mode == AppMode::World
            && self.chat_input.is_none()
            && self.sign_edit.is_none()
            && !self.text_field_focused()
        {
            self.caps_lock.on_key(pressed);
            if pressed {
                self.caps_lock_recheck = Some(Instant::now() + CAPS_LOCK_RECHECK);
            }
        }
    }

    // Zurueck in die Welt (Verbindung steht, Respawn, Pausemenue/Chat zu): erst alles
    // loslassen wie clear_controls, dann die Halte-Aktionen wieder setzen, deren Taste noch
    // gedrueckt ist. Das ist Vanillas KeyBinding.updatePressedStates(), das setScreen(null)
    // beim Schliessen jedes Bildschirms aufruft. Sonst wird ein gehaltenes W beim Beitreten
    // einer neuen Welt verschluckt -- vorher wurde nur losgelassen,
    // und W wirkte erst nach erneutem Druecken.
    //
    // Angriff und Benutzen bleiben aussen vor: der Klick, der den Bildschirm schliesst (etwa
    // "Wiederbeleben"), ist in dem Moment noch unten und darf nicht als Schlag weiterlaufen.
    pub(crate) fn restore_held_controls(&mut self) {
        self.clear_controls();
        if self.mode != AppMode::World || self.chat_input.is_some() || self.screen_overlay_open() || !self.focused {
            return;
        }
        let kb = self.settings.keybinds;
        for action in KeyBindAction::ALL {
            if matches!(action, KeyBindAction::Attack | KeyBindAction::Use) {
                continue;
            }
            if !kb.binds(action).any(|b| self.binds_down.contains(&b)) {
                continue;
            }
            // Auch Umschalt-Aktionen als "unten" merken, damit ihr Loslassen keine falsche
            // Flanke ergibt -- umgeschaltet wird dabei aber nichts.
            self.keybind_physical_down[action.index()] = true;
            if self.settings.keybind_modes.get(action) == KeybindMode::Hold {
                self.set_action_active(action, true);
            }
        }
    }

    pub(crate) fn action_active(&self, action: KeyBindAction) -> bool {
        match action {
            KeyBindAction::Forward => self.move_forward,
            KeyBindAction::Backward => self.move_backward,
            KeyBindAction::Left => self.move_left,
            KeyBindAction::Right => self.move_right,
            KeyBindAction::Jump => self.jump_held,
            KeyBindAction::Sneak => self.sneak_held,
            KeyBindAction::Sprint => self.sprint_held,
            KeyBindAction::InventoryMoveStack => self.inventory_move_stack_held,
            KeyBindAction::Use => self.use_held,
            KeyBindAction::PlayerList => self.player_list_held,
            KeyBindAction::Zoom => self.zoom_held,
            KeyBindAction::PushToTalk => self.push_to_talk_held,
            KeyBindAction::InspectItem => self.inspect_held,
            KeyBindAction::VoiceWhisper => self.whisper_held,
            KeyBindAction::GameModeWheel => self.gamemode_wheel.is_some(),
            KeyBindAction::Reconnect => self.reconnect_hold.is_some(),
            // Einmal-Aktionen ohne Dauerzustand -- sie loesen beim Druck aus (s.
            // apply_key_action). BEWUSST einzeln aufgezaehlt statt `_ => false`: mit dem
            // Auffangzweig fiel eine neu hinzugefuegte HALTE-Aktion still hier hinein und war
            // dauerhaft "nicht aktiv" -- genau so ging der Stapel-Verschieben-Modifikator
            // beim Einbauen verloren, ohne dass irgendetwas gemeckert haette. So erzwingt der
            // Compiler bei jeder neuen Aktion eine Entscheidung.
            KeyBindAction::TogglePerspective
            | KeyBindAction::OpenInventory
            | KeyBindAction::Attack
            | KeyBindAction::DropItem
            | KeyBindAction::SwapOffhand
            | KeyBindAction::OpenChat
            | KeyBindAction::OpenCommand
            | KeyBindAction::ToggleDebugOverlay
            | KeyBindAction::HideHud
            | KeyBindAction::ToggleProfiler
            | KeyBindAction::OpenModuleMenu
            | KeyBindAction::InventoryDropItem
            | KeyBindAction::Hotbar(_)
            | KeyBindAction::HotbarPrev
            | KeyBindAction::HotbarNext
            | KeyBindAction::VolumeUp
            | KeyBindAction::VolumeDown
            | KeyBindAction::VoiceMute
            | KeyBindAction::VoiceDeafen
            | KeyBindAction::VoiceConfig
            | KeyBindAction::PickBlock
            | KeyBindAction::SpectatorSlower
            | KeyBindAction::SpectatorFaster => false,
        }
    }

    pub(crate) fn set_action_active(&mut self, action: KeyBindAction, active: bool) {
        match action {
            KeyBindAction::Forward => self.move_forward = active,
            KeyBindAction::Backward => self.move_backward = active,
            KeyBindAction::Left => self.move_left = active,
            KeyBindAction::Right => self.move_right = active,
            KeyBindAction::Jump => self.jump_held = active,
            KeyBindAction::Sneak => self.sneak_held = active,
            KeyBindAction::Sprint => self.sprint_held = active,
            KeyBindAction::InventoryMoveStack => self.inventory_move_stack_held = active,
            KeyBindAction::Use => self.use_held = active,
            KeyBindAction::PlayerList => self.player_list_held = active,
            KeyBindAction::Zoom => self.zoom_held = active,
            KeyBindAction::PushToTalk => self.push_to_talk_held = active,
            KeyBindAction::InspectItem => self.inspect_held = active,
            KeyBindAction::VoiceWhisper => self.whisper_held = active,
            KeyBindAction::GameModeWheel => self.set_gamemode_wheel(active),
            KeyBindAction::Reconnect => {
                self.reconnect_hold = (active && !self.reconnect_needs_release).then(|| self.reconnect_hold.unwrap_or_else(Instant::now));
            }
            // Ebenfalls vollstaendig aufgezaehlt, aus demselben Grund wie in action_active.
            KeyBindAction::TogglePerspective
            | KeyBindAction::OpenInventory
            | KeyBindAction::Attack
            | KeyBindAction::DropItem
            | KeyBindAction::SwapOffhand
            | KeyBindAction::OpenChat
            | KeyBindAction::OpenCommand
            | KeyBindAction::ToggleDebugOverlay
            | KeyBindAction::HideHud
            | KeyBindAction::ToggleProfiler
            | KeyBindAction::OpenModuleMenu
            | KeyBindAction::InventoryDropItem
            | KeyBindAction::Hotbar(_)
            | KeyBindAction::HotbarPrev
            | KeyBindAction::HotbarNext
            | KeyBindAction::VolumeUp
            | KeyBindAction::VolumeDown
            | KeyBindAction::VoiceMute
            | KeyBindAction::VoiceDeafen
            | KeyBindAction::VoiceConfig
            | KeyBindAction::PickBlock
            | KeyBindAction::SpectatorSlower
            | KeyBindAction::SpectatorFaster => {}
        }
    }

    // Wendet eine einzelne physische Tasten-Transition (Druck/Loslassen) auf den EFFEKTIVEN
    // Aktions-Zustand an (move_forward/.../sprint_held) -- gemeinsamer Kern fuer den
    // winit-Pfad (s. window_event) und den Raw-Keyboard-Pfad (evdev, s. about_to_wait), damit
    // Halten/Umschalten (s. KeybindMode) nur an EINER Stelle beruecksichtigt werden muss.
    // Halten: folgt 1:1 dem physischen Zustand, wie bisher. Umschalten: nur die STEIGENDE
    // Flanke (Druck, nicht Loslassen, nicht Auto-Repeat-Wiederholungen desselben Zustands)
    // kippt den Zustand um -- dafuer wird der vorherige physische Zustand mitgefuehrt
    // (keybind_physical_down), da move_forward selbst im Umschalten-Modus NICHT mehr dem
    // physischen Zustand entspricht.
    // Naechste Perspektive fuer die Taste, uebersprungene 3.-Person-Ansichten ausgelassen.
    pub(crate) fn next_perspective(&self) -> Perspective {
        let mut p = self.perspective.next();
        while self.settings.perspective_skipped(p) {
            p = p.next();
        }
        p
    }

    pub(crate) fn quicklook_hold(&self) -> bool {
        self.settings.quicklook_target().is_some() && self.settings.keybind_modes.perspective == KeybindMode::Hold
    }

    // Der Autoklicker wirkt nur, wo er nichts anrichtet: auf anticheat-test.com (dafuer ist er
    // da) und auf Servern auf diesem Rechner, auch dem pktproxy davor.
    pub(crate) fn autoclicker_allowed(&self) -> bool {
        let adresse = self.server_address.trim();
        let host = adresse.rsplit_once(':').map_or(adresse, |(h, _)| h).trim_end_matches('.').to_ascii_lowercase();
        net::is_local_address(adresse) || host == "anticheat-test.com" || host.ends_with(".anticheat-test.com")
    }

    pub(crate) fn autoclicker_active(&self) -> bool {
        self.settings.module_autoclicker && self.mode == AppMode::World && self.autoclicker_allowed()
    }

    // Einmal je Bild, fuer Angreifen und Benutzen getrennt: solange die Taste gehalten ist,
    // druecken und wieder loslassen im Takt von autoclicker_cps, jeder Klick eine halbe
    // Periode unten. Faellt ein Bild zu spaet, wird nicht nachgeholt -- sonst kaemen nach
    // einem Ruckler mehrere Klicks auf einmal.
    pub(crate) fn tick_autoclicker(&mut self) {
        let jetzt = Instant::now();
        let erlaubt = self.autoclicker_active();
        let bereit = erlaubt && self.focused && self.chat_input.is_none() && !self.screen_overlay_open();
        let periode = Duration::from_secs_f64(1.0 / self.settings.autoclicker_cps.max(1) as f64);
        for (i, action) in AUTOCLICK_ACTIONS.into_iter().enumerate() {
            let k = &self.autoclick.channels[i];
            let aktiv = k.held && bereit;
            if k.down && (!aktiv || k.release_at.is_none_or(|t| jetzt >= t)) {
                self.synthetic_click(i, action, false);
            }
            let k = &mut self.autoclick.channels[i];
            if !aktiv {
                k.held &= erlaubt;
                k.next = None;
                continue;
            }
            let faellig = *k.next.get_or_insert(jetzt);
            if !k.down && jetzt >= faellig {
                let naechster = faellig + periode;
                k.next = Some(if naechster < jetzt { jetzt + periode } else { naechster });
                k.release_at = Some(jetzt + periode / 2);
                self.synthetic_click(i, action, true);
            }
        }
    }

    pub(crate) fn synthetic_click(&mut self, kanal: usize, action: KeyBindAction, pressed: bool) {
        self.autoclick.synthetic = true;
        self.apply_key_action(action, pressed);
        self.autoclick.synthetic = false;
        self.autoclick.channels[kanal].down = pressed;
    }

    pub(crate) fn apply_key_action(&mut self, action: KeyBindAction, pressed: bool) {
        // Autoklicker: gehaltenes Angreifen/Benutzen klickt nicht selbst, es gibt nur den Takt
        // frei (s. tick_autoclicker). Die erzeugten Klicks kommen mit synthetic hier durch.
        if let Some(kanal) = AUTOCLICK_ACTIONS.iter().position(|a| *a == action).filter(|_| !self.autoclick.synthetic && self.autoclicker_active()) {
            let k = &mut self.autoclick.channels[kanal];
            k.held = pressed;
            if pressed {
                k.next = Some(Instant::now());
            }
            return;
        }
        let idx = action.index();
        let was_down = self.keybind_physical_down[idx];
        self.keybind_physical_down[idx] = pressed;
        if pressed && !was_down {
            self.keybind_press_counter += 1;
            self.keybind_press_order[idx] = self.keybind_press_counter;
            // Stapel-Wurf: "Stapel verschieben" neu gedrueckt, waehrend Wegwerfen gehalten ist.
            if action == KeyBindAction::InventoryMoveStack {
                self.inventory_throw_sweep(true);
            }
            // Rechtsklick zaehlt JEDEN Druck, obwohl er zugleich einen Haltezustand fuehrt:
            // sonst verschmelzen zwei Klicks in einem Tick zu einem (s. use_queue).
            if action == KeyBindAction::Use {
                self.queue_use_click();
            }
        }
        // Quicklook: Perspektive-Taste auf Halten und eine 3.-Person-Ansicht uebersprungen --
        // solange sie unten ist, schaut man in die andere, beim Loslassen zurueck ins Auge.
        if action == KeyBindAction::TogglePerspective && self.quicklook_hold() {
            if let Some(ziel) = self.settings.quicklook_target() {
                if pressed && !was_down {
                    self.perspective = ziel;
                } else if !pressed && was_down {
                    self.perspective = Perspective::FirstPerson;
                }
            }
            return;
        }
        // Einmal-Aktionen: nur die steigende Flanke zaehlt (kein Auto-Repeat, kein
        // Loslassen), danach ist nichts weiter zu setzen.
        if action.is_press_action() {
            if pressed && !was_down {
                self.trigger_press_action(action);
            }
            // Mittelklick-Verteilen im Kreativinventar endet beim Loslassen.
            if !pressed && was_down && action == KeyBindAction::PickBlock && self.creative_shown() {
                self.creative_release(2);
            }
            return;
        }
        let active = match self.settings.keybind_modes.get(action) {
            KeybindMode::Hold => pressed,
            KeybindMode::Toggle => {
                if pressed && !was_down {
                    !self.action_active(action)
                } else {
                    self.action_active(action)
                }
            }
        };
        // Spielmodus-Rad: nur ein echtes Loslassen waehlt (clear_controls schliesst bloss).
        if action == KeyBindAction::GameModeWheel && !active && self.action_active(action) {
            self.gamemode_wheel_choose();
        }
        self.set_action_active(action, active);
    }

    // --- Inventar-Bedienung -------------------------------------------------------------

    // E: Inventar oeffnen bzw. den offenen Bildschirm schliessen. Nur im World-Modus mit
    // stehender Verbindung -- ohne Server gaebe es nichts anzuzeigen.
    pub(crate) fn toggle_inventory(&mut self) {
        if self.mode != AppMode::World || self.conn_status != ConnStatus::Connected {
            return;
        }
        // Ohne Oeffnen/Schliessen-Klang (Nutzerwunsch): das Inventar ist kein Fenster der Oberflaeche.
        if self.inventory.screen_open {
            self.close_inventory();
        } else {
            if self.game_mode == GAME_MODE_CREATIVE {
                self.creative_prepare();
            }
            self.inventory.screen_open = true;
            // Zeiger in die Mitte des mittleren Slots der obersten Inventarreihe (Nutzerwunsch;
            // Vanilla nimmt die Bildschirmmitte). Aus dem echten Layout, also mit der
            // eingestellten Inventar-Skalierung.
            if let Some(screen) = self.gpu.as_ref().map(|g| g.size()) {
                let layout = inventory::screen_layout(&self.inventory, screen, self.inventory_scale(screen));
                let pos = inventory::inventory_cursor_spawn(&layout).unwrap_or((screen.0 / 2.0, screen.1 / 2.0));
                self.place_cursor(pos);
            }
            // Und gehaltene Tasten loslassen -- derselbe Grund wie beim vom Server geoeffneten
            // Container (s. ClientEvent::OpenScreen). Hier faellt es weniger auf, weil man das
            // Inventar mit einer TASTE oeffnet und nicht mit der Maustaste, die danach haengt.
            self.clear_controls();
        }
    }

    // Schliessen meldet dem Server das Schliessen des Containers (sonst haelt er ihn offen
    // und wirft z.B. den Cursor-Stack nicht zurueck ins Inventar).
    pub(crate) fn close_inventory(&mut self) {
        self.stash_cursor_in_offhand();
        let sync_id = self.inventory.open.as_ref().map(|c| c.sync_id).unwrap_or(0);
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::close_container_payload(sync_id));
        }
        // Ein noch laufendes Verteilen-Ziehen zuerst zuruecknehmen: seine Vorschau wurde nie
        // abgeschickt, sie darf also auch nicht stehen bleiben. Erst danach schliessen --
        // sonst zeigte das Inventar beim naechsten Oeffnen eine Verteilung, die es auf dem
        // Server nie gab.
        if let Some(drag) = self.inventory_drag.take() {
            self.inventory_drag_restore(&drag);
        }
        self.quick_craft_pending = None;
        self.creative.drag = None;
        self.inventory.close();
        self.last_inventory_click = None;
        // Sonst verschluckt auch das Schliessen des Inventars ein gehaltenes W: beim Oeffnen wurde
        // losgelassen, beim Schliessen nie wieder uebernommen (s. restore_held_controls).
        self.restore_held_controls();
    }

    // Schliessen mit einem Stapel am Zeiger, fuer den das Inventar keinen Platz mehr hat: der
    // Server wuerde ihn wegwerfen (AbstractContainerMenu.removed legt zurueck, was passt, und
    // wirft den Rest). Stattdessen kommt er in die leere Zweithand. Im eigenen Inventar ist das
    // ein Klick auf sie; in einem Container hat sie keinen Slot, dort geht es ueber einen Platz
    // des Spielerteils: Stapel dort ablegen (das Item von dort kommt an den Zeiger), den Platz
    // mit der Zweithand tauschen, das Item zurueck auf seinen Platz.
    pub(crate) fn stash_cursor_in_offhand(&mut self) {
        if !self.inventory.screen_open || self.creative_shown() {
            return;
        }
        let Some(stapel) = self.inventory.cursor.clone() else { return };
        let zweithand_frei = self.inventory.player.get(inventory::PLAYER_OFFHAND).is_some_and(|s| s.is_none());
        if !zweithand_frei || self.inventory_room_for(&stapel) {
            return;
        }
        use protocol::packets::{CLICK_MODE_PICKUP, CLICK_MODE_SWAP};
        let Some(anzahl) = self.inventory.open.as_ref().map(|c| c.slots.len()) else {
            self.send_container_click(inventory::PLAYER_OFFHAND as i16, 0, CLICK_MODE_PICKUP);
            return;
        };
        // Ein Platz mit einem ANDEREN Item -- mit gleichem wuerde abgelegt statt getauscht.
        let Some(platz) = (anzahl.saturating_sub(36)..anzahl).find(|&i| self.inventory.slot_at_index(i).is_some_and(|s| !s.stackable_with(&stapel))) else {
            return;
        };
        self.send_container_click(platz as i16, 0, CLICK_MODE_PICKUP);
        // Den Tausch mit der Zweithand sagt predict_click im Container nicht voraus (sie hat
        // dort keinen Slot) -- das Ergebnis steht aber fest: der Platz wird leer.
        let vorhersage = inventory::Prediction { modified: vec![(platz as i16, None)], cursor: self.inventory.cursor.clone(), hashable: true };
        let (sync_id, revision, _) = self.inventory.active();
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(
                sync_id,
                revision,
                platz as i16,
                inventory::OFFHAND_SWAP_BUTTON,
                CLICK_MODE_SWAP,
                &vorhersage.modified,
                vorhersage.cursor.as_ref(),
            ));
        }
        self.inventory.apply_prediction(&vorhersage);
        if let Some(zweithand) = self.inventory.player.get_mut(inventory::PLAYER_OFFHAND) {
            *zweithand = Some(stapel);
        }
        self.send_container_click(platz as i16, 0, CLICK_MODE_PICKUP);
    }

    // Passt `stapel` beim Schliessen ganz ins Spielerinventar (Hauptteil und Hotbar, auf freie
    // Plaetze und angebrochene gleiche Stapel)?
    pub(crate) fn inventory_room_for(&self, stapel: &protocol::item::ItemStack) -> bool {
        let (_, _, slots) = self.inventory.active();
        let spielerteil = match &self.inventory.open {
            Some(_) => &slots[slots.len().saturating_sub(36)..],
            None => slots.get(inventory::PLAYER_MAIN_START..inventory::PLAYER_OFFHAND).unwrap_or(&[]),
        };
        let max = world::item_components::max_stack(stapel);
        let platz: i32 = spielerteil
            .iter()
            .map(|s| match s {
                None => max,
                Some(s) if s.stackable_with(stapel) => (max - s.count).max(0),
                Some(_) => 0,
            })
            .sum();
        platz >= stapel.count
    }

    // Das gehaltene Item wegwerfen (im SPIEL, nicht im Inventar -- dort ist es
    // inventory_throw). Ein einzelnes Stueck; zusammen mit der Taste "Stapel verschieben"
    // (auf Wunsch statt fest Strg, s. drop_whole_stack_held) der ganze Stapel.
    pub(crate) fn drop_held_item(&mut self) {
        if self.inventory.screen_open {
            return; // im offenen Inventar wirft der Klick neben das Fenster, nicht Q
        }
        let ganzer_stapel = self.drop_whole_stack_held();
        self.queue_input_action(PendingInputAction::DropItem(ganzer_stapel));
    }

    // Wirft ein Druck auf "wegwerfen" gerade den GANZEN Stapel? Ja, solange die Taste
    // "Stapel verschieben" gehalten wird (ohne eigene Belegung ist das die Schleichtaste, s.
    // KeyBindAction::inventory_fallback) -- so gewuenscht, statt Vanillas festem Strg. Strg
    // gilt zusaetzlich weiter, damit Vanilla-Gewohnheit nicht ins Leere laeuft.
    pub(crate) fn drop_whole_stack_held(&self) -> bool {
        self.keybind_physical_down[KeyBindAction::InventoryMoveStack.index()] || self.modifiers.control_key()
    }

    // ClientPlayerEntity.dropSelectedItem + MinecraftClient.handleInputEvents: das Item
    // verschwindet sofort lokal, das Drop-Paket geht raus, und war etwas in der Hand, schwingt
    // die Haupthand.
    //
    // Frueher ging nur das Paket raus -- die Hotbar aenderte sich erst mit der Serverantwort,
    // und die Hand schwang nicht.
    pub(crate) fn drop_selected_item(&mut self, ganzer_stapel: bool) {
        let slot = inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8);
        let mut geworfen = false;
        if let Some(eintrag) = self.inventory.player.get_mut(slot) {
            if let Some(stack) = eintrag.as_mut() {
                geworfen = true;
                if ganzer_stapel || stack.count <= 1 {
                    *eintrag = None;
                } else {
                    stack.count -= 1;
                }
            }
        }
        let aktion = if ganzer_stapel { protocol::packets::PLAYER_ACTION_DROP_ALL_ITEMS } else { protocol::packets::PLAYER_ACTION_DROP_ITEM };
        self.send_player_action(aktion);
        if geworfen {
            self.swing_arm();
        }
    }

    // F: Haupt- und Zweithand tauschen.
    pub(crate) fn swap_offhand(&mut self) {
        if self.inventory.screen_open {
            return;
        }
        self.queue_input_action(PendingInputAction::SwapOffhand);
    }

    // Aktiven Hotbar-Slot setzen (Zifferntasten/Mausrad). Rein lokal -- gesendet wird der
    // Slot erst zum Tickbeginn (s. sync_selected_slot), genau wie Vanillas
    // PlayerInventory.setSelectedSlot() nur das Feld setzt und
    // ClientPlayerInteractionManager.tick() daraus ein Paket macht. Deshalb erzeugt auch
    // schnelles Durchscrollen ueber mehrere Slots hoechstens EIN Paket pro Tick, mit dem
    // Endwert -- vorher ging pro Rasterschritt eins raus.
    pub(crate) fn select_hotbar_slot(&mut self, slot: usize) {
        if slot > 8 {
            return;
        }
        // Anderer Slot = anderes Item in der Hand: es kommt von ganz unten und steigt mit der
        // Aufladung, die der Wechsel ebenfalls zuruecksetzt (s. equip_target). Kein UI-Klang:
        // die Hotbar ist Spiel, keine Oberflaeche (Wunsch des Nutzers). Vanilla senkt
        // vorher noch das alte Item; da unser Slot sofort wechselt, waere das nur eine
        // Verzoegerung ohne Bild. Beide Werte setzen, sonst glitte die Hand ab der Tickmitte
        // los und sprange beim naechsten Tick. Nur von leerer zu leerer Hand bleibt die Faust
        // stehen. Vanilla tauscht auch gleiche Items in gleicher Anzahl ohne Senken aus
        // (ItemInHandRenderer.shouldInstantlyReplaceVisibleItem -> ItemStack.
        // matchesIgnoringComponents) -- zwei verschiedene Traenke sind dort dasselbe Item, der
        // Wechsel saehe nach nichts aus (Wunsch des Nutzers: immer animieren). Die Aufladung
        // selbst setzt nur ein anderes Item zurueck (Player.tick: !isSameItem, s.
        // attack_hand_changed).
        let leer = |i: usize| self.inventory.player.get(inventory::PLAYER_HOTBAR_START + i).is_none_or(|s| s.is_none());
        if self.inventory.held_slot != slot && !(leer(self.inventory.held_slot) && leer(slot)) {
            self.own_equip_progress = 1.0;
            self.own_last_equip_progress = 1.0;
        }
        self.inventory.held_slot = slot;
    }

    // Mausrad im World-Modus: durch die Hotbar blaettern (Vanilla: hoch = nach links).
    pub(crate) fn scroll_hotbar(&mut self, up: bool) {
        // Das Rad blaettert vom SICHTBAREN Slot aus weiter; wartende Zifferntasten waeren
        // danach nur noch Spruenge zurueck.
        if self.server_still_eating() {
            self.slot_queue.scroll(self.inventory.held_slot, up);
            return;
        }
        self.slot_queue.clear();
        let current = self.inventory.held_slot as i32;
        let next = (current + if up { -1 } else { 1 }).rem_euclid(9) as usize;
        self.select_hotbar_slot(next);
    }

    // Klick auf einen Slot des offenen Bildschirms. Es wird NICHTS lokal vorweggenommen --
    // das Paket geht raus, der Server antwortet mit dem neuen Slotinhalt (s. inventory.rs).
    pub(crate) fn inventory_click(&mut self, button: i8) {
        if self.creative_shown() {
            self.creative_click(button);
            return;
        }
        // Ein neuer Klick baut auf dem aktuellen Stand auf; eine alte Vorschau festzuhalten
        // hiesse, ihn gegen den Server zu verteidigen.
        self.quick_craft_pending = None;
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        if self.recipe_book_click(&layout, button) {
            return;
        }
        let (cx, cy) = self.cursor_pos;
        let (sync_id, revision, _) = self.inventory.active();
        // Handel: Schieber packen oder ein Angebot waehlen (MerchantScreen.mouseClicked und
        // die Knoepfe der Liste).
        if self.open_menu_type() == Some(inventory::MENU_MERCHANT) && button == 0 {
            let s = layout.scale;
            let (bx, by) = (layout.bg_x, layout.bg_y);
            let (anzahl, scroll) = self.inventory.open.as_ref().map_or((0, 0), |c| (c.merchant.offers.len(), c.merchant.scroll));
            // Vanillas Grenzen: rechts und oben offen, unten ein Pixel weiter als die Bahn.
            let gx = (cx - bx) / s;
            let gy = (cy - by) / s;
            if anzahl > inventory::MERCHANT_VISIBLE_OFFERS && gx > 94.0 && gx < 100.0 && gy > 18.0 && gy <= 18.0 + inventory::MERCHANT_SCROLL_TRACK + 1.0 {
                if let Some(c) = self.inventory.open.as_mut() {
                    c.merchant.dragging = true;
                }
                return;
            }
            for i in 0..inventory::MERCHANT_VISIBLE_OFFERS.min(anzahl) {
                let (x, y, w, h) = inventory::merchant_button_rect(&layout, i);
                if cx >= x && cy >= y && cx < x + w && cy < y + h {
                    if let Some(c) = self.inventory.open.as_mut() {
                        c.merchant.focused = Some(i);
                    }
                    self.select_merchant_offer(i + scroll);
                    return;
                }
            }
        }
        // Zaubertisch: Klick auf eine der drei Optionen (EnchantmentScreen.mouseClicked).
        if self.open_menu_type() == Some(inventory::MENU_ENCHANTMENT) && button == 0 {
            for i in 0..3 {
                let (x, y, w, h) = inventory::enchant_option_rect(&layout, i);
                if cx >= x && cy >= y && cx < x + w && cy < y + h {
                    if self.enchant_option_available(i) {
                        if let Some(tx) = &self.outgoing_tx {
                            let _ = tx.send(protocol::packets::container_button_click_payload(sync_id, i as i32));
                        }
                        self.play_ui_sound(ui_sound::UiSound::Click);
                    }
                    return;
                }
            }
        }
        let move_stack = self.action_active(KeyBindAction::InventoryMoveStack);
        // Doppelklick: zweimal dieselbe Taste auf denselben Slot innerhalb von 250ms --
        // Vanillas Fenster (Mouse.field_61505) und Vanillas Bedingung (HandledScreen:
        // `lastClickedSlot == slot && doubled`). Nur die linke Taste, wie dort.
        let now = Instant::now();
        let hovered = layout.slot_at(cx, cy);
        let doppelklick = button == 0
            && hovered.is_some()
            && self.last_inventory_click == hovered.map(|slot| (slot, button))
            && now.duration_since(self.last_inventory_click_at) < INVENTORY_DOUBLE_CLICK;
        self.last_inventory_click = hovered.map(|slot| (slot, button));
        self.last_inventory_click_at = now;
        // ...aber nur, wenn dabei ueberhaupt etwas eingesammelt WIRD.
        //
        // ECHTER BUG gewesen (Items liessen sich nicht schnell nehmen und wieder ablegen): Vanillas PICKUP_ALL tut nichts, wenn nichts Gleichartiges mehr im
        // Inventar liegt (ScreenHandler.internalOnSlotClick prueft `!slot.hasStack()` und
        // sammelt dann nur passende Stapel ein). Bei uns hat aber JEDER zweite Klick auf
        // denselben Slot innerhalb von 250ms diesen Zweig genommen -- also ging ein
        // Leerlauf-Paket raus statt des Ablegens, und wer schnell klickte, bekam den Stapel
        // nie wieder aus der Hand. Gibt es nichts zu sammeln, ist der Doppelklick hier
        // wirkungslos und der Klick zaehlt als gewoehnlicher Klick.
        //
        // Mit dem Stapel-Modifikator bleibt es beim Doppelklick: quick_move_all raeumt auch
        // dann etwas weg, wenn PICKUP_ALL nichts zu holen haette.
        let doppelklick = doppelklick
            && (move_stack
                || hovered
                    .is_some_and(|slot| self.inventory.predict_click(slot as i16, 0, protocol::packets::CLICK_MODE_PICKUP_ALL).is_some()));

        let (slot, mode) = match hovered {
            // Doppelklick mit Stapel-Verschieben: alle gleichen Stapel im selben Bereich
            // wandern hinueber (Vanilla loest dafuer eine QUICK_MOVE-Kette aus, s.
            // quick_move_all). Ohne den Modifikator sammelt der Doppelklick alles Gleiche
            // auf den Zeiger -- PICKUP_ALL.
            Some(slot) if doppelklick => {
                if move_stack {
                    self.quick_move_all(slot);
                    return;
                }
                (slot as i16, protocol::packets::CLICK_MODE_PICKUP_ALL)
            }
            // Stapel verschieben mit einem Item am Zeiger auf dasselbe Item: alle Stapel dieser
            // Sorte wandern hinueber, schon beim ersten Klick -- Vanilla braucht dafuer einen
            // Doppelklick. Der Stapel am Zeiger bleibt dort.
            Some(slot)
                if move_stack
                    && button == 0
                    && self.inventory.cursor.as_ref().is_some_and(|c| self.inventory.slot_at_index(slot).is_some_and(|s| s.stackable_with(c))) =>
            {
                self.quick_move_all(slot);
                return;
            }
            // Voller Zeiger: Vanilla legt hier NICHT sofort ab, sondern beginnt ein
            // moegliches Verteilen-Ziehen (HandledScreen.mouseClicked, Zweig `else {
            // cursorDragging = true }`) und entscheidet erst beim Loslassen, was daraus wurde
            // (s. inventory_release). Das ist die Voraussetzung fuers Ziehen -- der Stapel
            // muss bis dahin am Zeiger bleiben -- fuehlt sich aber traeger an.
            //
            // Mit `inventory_place_on_press` (Standard) wird stattdessen sofort abgelegt und
            // auf das Ziehen verzichtet. Beides zugleich geht nicht: was schon im Slot liegt,
            // laesst sich nicht mehr verteilen.
            // Voller Zeiger: das kann ein einfacher Klick ODER der Anfang eines
            // Verteilen-Ziehens sein -- entschieden wird beim Loslassen (Vanilla:
            // HandledScreen.mouseClicked setzt hier nur cursorDragging).
            //
            // Anders als Vanilla wird die Wirkung aber SOFORT lokal angezeigt (s.
            // inventory_drag_preview): das Item liegt im Slot, sobald man drueckt, und
            // verteilt sich beim Ziehen live weiter. Nur das PAKET geht erst beim Loslassen
            // raus -- vorher steht ja nicht fest, ob es ein Klick oder ein Ziehen wird.
            // Sonst wartet das Ablegen sichtbar, bis man die Maus loslaesst.
            Some(_) | None if self.inventory.cursor.is_some() && (button == 0 || button == 1) => {
                let cursor_before = self.inventory.cursor.clone();
                let mut drag = InventoryDrag { button, slots: Vec::new(), cursor_before, move_stack };
                if let Some(slot) = hovered {
                    drag.slots.push((slot, self.inventory.slot_at_index(slot).cloned()));
                }
                self.inventory_drag = Some(drag);
                self.inventory_drag_preview();
                // Das Item liegt ab jetzt sichtbar im Slot (Vorschau), also springt der Zeiger
                // JETZT in dessen Mitte -- beim Loslassen kaeme der Sprung zu spaet, man saehe
                // bis dahin den Zeiger noch an der alten Stelle. Nicht beim Stapel-Verschieben:
                // dort landet das Item woanders. Und nur, wenn der Zeiger danach LEER ist:
                // haengt noch etwas dran (Rechtsklick legt ein Stueck ab, oder der Slot war
                // fast voll), geht es mit dem Rest weiter -- sonst bleibt der Stapel im
                // Inventar, der Zeiger springt aber trotzdem in die Mitte, und beim
                // Verteilen mit rechts sprang der Zeiger mitten im Ziehen.
                let abgelegt = self.inventory.cursor.is_none();
                if let Some(slot) = hovered.filter(|_| self.settings.inventory_cursor_to_slot && !move_stack && abgelegt) {
                    if let Some(mitte) = layout.slot_center(slot) {
                        self.place_cursor(mitte);
                    }
                }
                return;
            }
            Some(slot) => {
                let mode = if move_stack { protocol::packets::CLICK_MODE_QUICK_MOVE } else { protocol::packets::CLICK_MODE_PICKUP };
                (slot as i16, mode)
            }
            // Klick NEBEN das Fenster wirft den Stapel an der Maus weg -- Vanillas
            // Sonder-Slot -999 mit Modus PICKUP (Button 0 = ganzer Stapel, 1 = ein Stueck).
            // Innerhalb des Fensters (Rand, Ritze zwischen zwei Slots) passiert dagegen
            // nichts: dort etwas wegzuwerfen war ein Bug, kein Vanilla-Verhalten.
            None if layout.is_outside(cx, cy) && self.inventory.cursor.is_some() => {
                (inventory::DROP_SLOT, protocol::packets::CLICK_MODE_PICKUP)
            }
            None => return,
        };
        // Vorhersagen, was der Klick bewirkt -- daraus werden die Hash-Felder des Pakets
        // (s. inventory::predict_click). Klappt das nicht, gehen leere Hashes raus und der
        // Server schickt den neuen Zustand nach.
        let prediction = self.predict_inventory_click(slot, button, mode);
        let (modified, cursor) = match &prediction {
            // Auch nicht hashbare Stapel (eigener Name, Verzauberung ...) gehen mit: der Hash
            // weicht dann absichtlich ab (s. packets::write_item_stack_hash), der Server sieht
            // den Unterschied und schickt seinen Stand. Frueher gingen hier leere Felder und
            // der alte Cursor raus, waehrend lokal schon die Vorhersage galt -- der Server
            // hielt den Client fuer unveraendert und korrigierte nichts: eine Glasscheibe aus
            // einem Server-Menue blieb am Zeiger haengen, statt sofort zurueckzuspringen.
            Some(p) => (p.modified.clone(), p.cursor.clone()),
            // Ohne Vorhersage wird der Cursor gesendet, wie er JETZT ist -- nicht "leer".
            // Das ist immer die sichere Angabe: aendert der Klick den Cursor nicht (Quick-
            // Move, Tauschen), stimmt sie; aendert er ihn doch, weicht sie ab und der Server
            // schickt den neuen Cursor nach. Ein pauschales "leer" war hier ein Bug: legt man
            // etwas in ein Feld, dessen Regeln wir nicht vorhersagen (Werkbank, Ruestung), ist
            // der Server-Cursor danach TATSAECHLICH leer -- die Falschaussage stimmte also
            // zufaellig, die Korrektur blieb aus und das Item klebte lokal am Zeiger fest.
            None => (Vec::new(), self.inventory.cursor.clone()),
        };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(sync_id, revision, slot, button, mode, &modified, cursor.as_ref()));
        }
        if let Some(prediction) = prediction {
            self.inventory.apply_prediction(&prediction);
        }
    }

    // Maustaste im Inventar losgelassen. Zwei Faelle, genau wie in Vanillas mouseReleased:
    // wurden waehrend des Ziehens Slots eingesammelt, geht die Verteil-Kette raus; sonst war
    // es doch nur ein gewoehnlicher Klick und wird JETZT ausgefuehrt.
    pub(crate) fn inventory_release(&mut self, button: i8) {
        if self.creative_shown() {
            self.creative_release(button.clamp(0, 1) as u8);
            return;
        }
        if button == 0 {
            if let Some(c) = self.inventory.open.as_mut() {
                c.merchant.dragging = false;
            }
        }
        let Some(drag) = self.inventory_drag.take() else { return };
        if drag.button != button {
            return;
        }
        // Ueber MEHRERE Slots gezogen: die Verteil-Kette. Die Vorschau steht lokal schon,
        // sie bleibt bis zur Serverantwort stehen (fuer QUICK_CRAFT gibt es keine
        // Klick-Vorhersage, die sie ersetzen koennte).
        if drag.slots.len() > 1 {
            // ScreenHandler.packQuickCraftData(stage, button) = stage & 3 | (button & 3) << 2.
            // Stufe 0 = Anfang, 1 = ein Slot, 2 = Ende; Anfang und Ende tragen den
            // Sonder-Slot -999. Die Kette geht am Stueck raus, der Server verteilt dann.
            //
            // Der gesendete Cursor ist bis einschliesslich Stufe 1 der Stand VOR dem Ziehen:
            // Vanilla verteilt nichts, solange die Kette laeuft -- Stufe 1 sammelt nur Slots
            // ein (ScreenHandler.internalOnSlotClick, `quickCraftSlots.add(slot)`), erst
            // Stufe 2 raeumt den Zeiger leer. Unsere lokale Vorschau greift dem aber schon
            // beim Ziehen vor, und wer den bereits geleerten Zeiger meldet, widerspricht dem
            // Server: der schickt daraufhin mitten in der Kette den vollen Stapel zurueck an
            // die Maus.
            let vorher = drag.cursor_before.clone();
            self.send_quick_craft_click(inventory::DROP_SLOT, quick_craft_button(0, drag.button), vorher.as_ref());
            for (slot, _) in &drag.slots {
                self.send_quick_craft_click(*slot as i16, quick_craft_button(1, drag.button), vorher.as_ref());
            }
            // Stufe 2 traegt das Ergebnis -- hier stimmt unsere Vorschau mit dem ueberein,
            // was der Server gleich selbst ausrechnet.
            let danach = self.inventory.cursor.clone();
            self.send_quick_craft_click(inventory::DROP_SLOT, quick_craft_button(2, drag.button), danach.as_ref());
            self.quick_craft_pending = Some(QuickCraftPending {
                slots: drag.slots.iter().map(|(slot, _)| (*slot, self.inventory.slot_at_index(*slot).cloned())).collect(),
                cursor: danach,
                remaining: drag.slots.len() + 2,
                since: Instant::now(),
            });
            return;
        }

        // Nur ein Slot (oder keiner) -- es war doch ein gewoehnlicher Klick. Die Vorschau
        // wird dafuer ZURUECKGENOMMEN und der normale Weg gegangen: der rechnet dieselbe
        // Wirkung noch einmal, aber mitsamt den Hash-Feldern fuers Paket (s.
        // send_container_click). Ohne das Zuruecknehmen kaeme die Wirkung doppelt.
        self.inventory_drag_restore(&drag);
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let move_stack = drag.move_stack;
        match layout.slot_at(cx, cy) {
            Some(slot) => {
                let mode = if move_stack { protocol::packets::CLICK_MODE_QUICK_MOVE } else { protocol::packets::CLICK_MODE_PICKUP };
                self.send_container_click(slot as i16, button, mode);
            }
            // Neben das Fenster losgelassen: wegwerfen (s. inventory_click).
            None if layout.is_outside(cx, cy) => {
                self.send_container_click(inventory::DROP_SLOT, button, protocol::packets::CLICK_MODE_PICKUP);
            }
            None => {}
        }
    }

    // Nach einem vollen Container-Abgleich: laeuft noch eine Verteil-Kette und zeigt der
    // Abgleich einen Zwischenstand, die Vorschau wieder darueberlegen (s. QuickCraftPending).
    // Stimmt der Abgleich mit der Vorschau ueberein, ist die Kette bestaetigt; der letzte
    // erwartete Abgleich und die Zeitgrenze beenden sie ebenso -- dann gilt der Server.
    pub(crate) fn hold_quick_craft_preview(&mut self) {
        let Some(p) = self.quick_craft_pending.as_mut() else { return };
        p.remaining = p.remaining.saturating_sub(1);
        let bestaetigt = p.slots.iter().all(|(slot, stack)| self.inventory.slot_at_index(*slot) == stack.as_ref());
        if bestaetigt || p.remaining == 0 || p.since.elapsed() > QUICK_CRAFT_PENDING_MAX {
            self.quick_craft_pending = None;
            return;
        }
        let prediction = inventory::Prediction {
            modified: p.slots.iter().map(|(slot, stack)| (*slot as i16, stack.clone())).collect(),
            cursor: p.cursor.clone(),
            hashable: false,
        };
        self.inventory.apply_prediction(&prediction);
    }

    // Ein Server-Update WAEHREND eines Verteilen-Ziehens: die Vorschau zuruecknehmen, das
    // Update auf den echten Stand anwenden, und die Verteilung darauf neu rechnen. Ohne das
    // ueberschrieb jeder Abgleich (Plugins schicken in Menues laufend welche) die Vorschau --
    // die schon verteilten Items verschwanden aus den Slots und hingen wieder am Zeiger.
    pub(crate) fn with_drag_suspended(&mut self, update: impl FnOnce(&mut Self)) {
        let Some(mut drag) = self.inventory_drag.take() else {
            update(self);
            return;
        };
        self.inventory_drag_restore(&drag);
        update(self);
        for (slot, vorher) in drag.slots.iter_mut() {
            *vorher = self.inventory.slot_at_index(*slot).cloned();
        }
        drag.cursor_before = self.inventory.cursor.clone();
        self.inventory_drag = Some(drag);
        self.inventory_drag_preview();
    }

    // Den Stand von VOR dem Ziehen wiederherstellen (s. inventory_drag_preview).
    pub(crate) fn inventory_drag_restore(&mut self, drag: &InventoryDrag) {
        self.inventory.apply_prediction(&inventory::Prediction {
            modified: drag.slots.iter().map(|(slot, inhalt)| (*slot as i16, inhalt.clone())).collect(),
            cursor: drag.cursor_before.clone(),
            hashable: false,
        });
    }

    // Waehrend des Ziehens jeden ueberfahrenen Slot einsammeln. Vanillas Grenze
    // (HandledScreen.mouseDragged): nur solange noch mehr Stuecke am Zeiger haengen als
    // Slots gesammelt sind -- auf mehr Slots als Items laesst sich nichts verteilen.
    pub(crate) fn inventory_drag_collect(&mut self) {
        if self.inventory_drag.is_none() {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let Some(slot) = layout.slot_at(cx, cy) else { return };
        // Was JETZT im Slot liegt, ist bereits die Vorschau -- gemerkt wird der Zustand von
        // VOR dem Ziehen. Fuer einen noch nicht besuchten Slot ist das schlicht sein Inhalt.
        let inhalt = self.inventory.slot_at_index(slot).cloned();
        let am_zeiger = self.inventory.cursor.as_ref().map_or(0, |stack| stack.count);
        let Some(drag) = self.inventory_drag.as_mut() else { return };
        if drag.slots.iter().any(|(besucht, _)| *besucht == slot) {
            return;
        }
        // Vanillas Grenze (HandledScreen.mouseDragged): nur solange noch mehr Stuecke am
        // Zeiger haengen, als Slots gesammelt sind -- auf mehr Slots als Items laesst sich
        // nichts verteilen. Gemessen am Stand VOR dem Ziehen, denn der Zeiger ist in der
        // Vorschau schon (teilweise) geleert.
        let gesamt = drag.cursor_before.as_ref().map_or(am_zeiger, |stack| stack.count);
        if (gesamt as usize) <= drag.slots.len() {
            return;
        }
        drag.slots.push((slot, inhalt));
        self.inventory_drag_preview();
    }

    // Die Vorschau des laufenden Ziehens anwenden: erst den Stand von VOR dem Ziehen
    // wiederherstellen, dann neu verteilen. Ohne das Zuruecksetzen wuerde sich beim vierten
    // Slot die Verteilung des dritten mit einrechnen.
    pub(crate) fn inventory_drag_preview(&mut self) {
        let Some(drag) = self.inventory_drag.as_ref() else { return };
        let Some(cursor) = drag.cursor_before.clone() else { return };
        let vorher: Vec<(usize, Option<protocol::item::ItemStack>)> = drag.slots.clone();
        let (geaendert, rest) = inventory::drag_preview(drag.button, &cursor, &vorher);

        // Zuruecksetzen und dann die neue Verteilung darueberlegen -- beides ueber denselben
        // Weg wie eine Klick-Vorhersage, damit der Container-/Spielerteil-Abgleich (s.
        // inventory::apply_prediction) nicht zweimal existiert.
        let zuruecksetzen = inventory::Prediction {
            modified: vorher.iter().map(|(slot, inhalt)| (*slot as i16, inhalt.clone())).collect(),
            cursor: Some(cursor),
            hashable: false,
        };
        self.inventory.apply_prediction(&zuruecksetzen);
        self.inventory.apply_prediction(&inventory::Prediction { modified: geaendert, cursor: rest, hashable: false });
    }

    // Ein Glied der Verteil-Kette. Eigener Weg statt send_container_click, weil fuer
    // QUICK_CRAFT keine Klick-Vorhersage existiert (der gesendete Cursor haengt an der Stufe,
    // s. inventory_release) und geaenderte Slots dort nie mitgeschickt werden -- Vanilla
    // veraendert waehrend der Kette ja nichts, was es melden koennte.
    pub(crate) fn send_quick_craft_click(&mut self, slot: i16, button: i8, cursor: Option<&protocol::item::ItemStack>) {
        let (sync_id, revision, _) = self.inventory.active();
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(
                sync_id,
                revision,
                slot,
                button,
                protocol::packets::CLICK_MODE_QUICK_CRAFT,
                &[],
                cursor,
            ));
        }
    }

    // inventory::predict_click, davor Vanillas Sperre fuer Ruestung mit Fluch der Bindung
    // (ArmorSlot.canTakeItems: nicht im Kreativmodus, und nur mit PREVENT_ARMOR_CHANGE). Ein
    // gesperrter Klick aendert nichts -- weder Aufnehmen noch Tauschen, Shift-Klick oder
    // Wegwerfen -- und laesst den Slot zittern (s. inventory::shake_offset).
    pub(crate) fn predict_inventory_click(&mut self, slot: i16, button: i8, mode: i32) -> Option<inventory::Prediction> {
        let gesperrt = self.inventory.open.is_none()
            && self.game_mode != GAME_MODE_CREATIVE
            && usize::try_from(slot).ok().filter(|s| (inventory::PLAYER_ARMOR_START..inventory::PLAYER_ARMOR_START + 4).contains(s)).is_some_and(|s| {
                self.inventory.slot_at_index(s).is_some_and(|stack| self.enchantments.prevents_armor_change(stack))
            });
        if gesperrt {
            self.armor_shake = Some((slot as usize, Instant::now()));
            return Some(inventory::locked_prediction(self.inventory.cursor.as_ref()));
        }
        self.inventory.predict_click(slot, button, mode)
    }

    // Ein Klick-Paket bauen und abschicken, samt Vorhersage. Gemeinsamer Unterbau aller
    // Inventar-Aktionen -- die Hash-Felder und die Revision hier zweimal zu schreiben waere
    // die Sorte Verdopplung, an der spaeter genau eine der beiden Stellen vergessen wird.
    pub(crate) fn send_container_click(&mut self, slot: i16, button: i8, mode: i32) {
        let (sync_id, revision, _) = self.inventory.active();
        // BundleItem.playInsertSound & Co.: schon im Client, wie in Vanilla.
        if let Some(klang) = self.inventory.bundle_click_sound(slot, button, mode) {
            self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
            let (laut, hoehe) = if klang.ends_with("_fail") { (1.0, 1.0) } else { (0.8, 0.8 + zufallswert(self.type_pitch_seed) * 0.4) };
            let ort = self.physics.eye_pos();
            self.play_event_at_scaled(klang, ort, laut, hoehe, protocol::packets::SoundCategory::Players);
        }
        let prediction = self.predict_inventory_click(slot, button, mode);
        let (modified, cursor) = match &prediction {
            // Nicht hashbar: trotzdem mitschicken, s. inventory_click.
            Some(p) => (p.modified.clone(), p.cursor.clone()),
            None => (Vec::new(), self.inventory.cursor.clone()),
        };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(sync_id, revision, slot, button, mode, &modified, cursor.as_ref()));
        }
        if let Some(prediction) = prediction {
            self.inventory.apply_prediction(&prediction);
        }
    }

    // Kantenlaenge des Craftingfelds, wenn der offene Bildschirm ein Rezeptbuch hat: 2 = das
    // eigene Inventar, 3 = die Werkbank.
    pub(crate) fn recipe_grid(&self) -> Option<i32> {
        if !self.inventory.screen_open || self.creative_shown() {
            return None;
        }
        match &self.inventory.open {
            None => Some(2),
            Some(c) if c.screen_type == inventory::MENU_CRAFTING => Some(3),
            Some(_) => None,
        }
    }

    // Knopf zum Oeffnen des Buchs im Fenster (Bildschirmpixel).
    pub(crate) fn recipe_button_rect(&self, layout: &inventory::ScreenLayout) -> Option<Rect> {
        let kante = self.recipe_grid()?;
        let (gx, gy) = recipe_book::button_offset(kante);
        let s = layout.scale;
        Some(Rect { x: layout.bg_x + gx * s, y: layout.bg_y + gy * s, w: recipe_book::BUTTON_W * s, h: recipe_book::BUTTON_H * s })
    }

    // Die Truhe, wenn das Buch offen ist.
    pub(crate) fn recipe_panel(&self, layout: &inventory::ScreenLayout) -> Option<recipe_book::Panel> {
        self.recipe_grid()?;
        self.recipe_book.open.then(|| recipe_book::Panel::beside(layout.bg_x, layout.bg_y, layout.scale))
    }

    // Was zum Craften da ist: Inventar und Hotbar plus das Craftingfeld (das gibt ein neues
    // Rezept zuerst zurueck). Nicht Ruestung, Zweithand und das Ergebnisfeld.
    pub(crate) fn recipe_stock(&self) -> std::collections::HashMap<u32, i32> {
        let (_, _, slots) = self.inventory.active();
        let haupt = slots.len().saturating_sub(36);
        let feld = match self.recipe_grid() {
            Some(2) => 1..5,
            Some(_) => 1..10,
            None => 0..0,
        };
        recipe_book::stock(slots.iter().enumerate().filter(|(i, _)| feld.contains(i) || (*i >= haupt && *i < haupt + 36)).filter_map(|(_, s)| s.as_ref()))
    }

    pub(crate) fn recipe_shown(&self) -> Vec<recipe_book::Shown> {
        let Some(kante) = self.recipe_grid() else { return Vec::new() };
        self.recipe_book.shown(kante, &self.recipe_stock())
    }

    // Klick in die Truhe. Links: einmal craften, mit Stapel-Modifikator einen Stapel des
    // Ergebnisses; rechts: nur ins Feld legen (so viel wie geht). Das Ergebnis holt sich der
    // Client ueber Klicks auf das Ergebnisfeld; die gehen ohne Vorhersage raus, weil das Feld
    // erst auf dem Server gefuellt wird -- der Server gleicht den Stand danach ab.
    pub(crate) fn recipe_click(&mut self, shown: recipe_book::Shown, button: i8) {
        use protocol::packets::{click_container_payload, CLICK_MODE_PICKUP, CLICK_MODE_QUICK_MOVE};
        use protocol::recipe::place_recipe_payload;
        let Some(entry) = self.recipe_book.entries.get(shown.index).cloned() else { return };
        let Some(tx) = self.outgoing_tx.clone() else { return };
        let (sync_id, revision, slots) = self.inventory.active();
        let klick = |slot: i16, mode: i32| click_container_payload(sync_id, revision, slot, 0, mode, &[], None);
        if button == 1 {
            let _ = tx.send(place_recipe_payload(sync_id, entry.id, true));
            self.play_ui_sound(ui_sound::UiSound::Click);
            return;
        }
        // Craften braucht eine freie Hand (das Ergebnis kommt ueber den Zeiger) und Zutaten.
        if button != 0 || self.inventory.cursor.is_some() || shown.sets == 0 {
            return;
        }
        let Some(ergebnis) = entry.display.result().first_stack() else { return };
        let je_satz = ergebnis.count.max(1);
        let stapel = world::item_components::max_stack(&ergebnis);
        let saetze = if self.action_active(KeyBindAction::InventoryMoveStack) { (stapel / je_satz).max(1) } else { 1 };
        // Ein Stueck je Satz und nie mehr Saetze im Feld als gewollt: dann raeumt ein
        // Schnell-Verschieben des Ergebnisses genau einen Stapel ins Inventar.
        if saetze > 1 && je_satz == 1 && self.recipe_book.sets_from(&entry, &self.recipe_stock(), 64) <= saetze {
            let _ = tx.send(place_recipe_payload(sync_id, entry.id, true));
            let _ = tx.send(klick(0, CLICK_MODE_QUICK_MOVE));
            self.play_ui_sound(ui_sound::UiSound::Click);
            return;
        }
        // Sonst Satz fuer Satz auf den Zeiger und von dort in einen freien Platz: der letzte
        // freie im Inventar (das Feld gibt beim Neubelegen Reste vorn in die Hotbar zurueck).
        let haupt = slots.len().saturating_sub(36);
        let ziel = (haupt..haupt + 27).rev().chain((haupt + 27..haupt + 36).rev()).find(|&i| slots.get(i).is_some_and(|s| s.is_none()));
        let _ = tx.send(place_recipe_payload(sync_id, entry.id, saetze > 1));
        for _ in 0..saetze {
            let _ = tx.send(klick(0, CLICK_MODE_PICKUP));
        }
        // Kein Platz: das Ergebnis bleibt am Zeiger, statt irgendwo getauscht zu werden.
        if let Some(ziel) = ziel {
            let _ = tx.send(klick(ziel as i16, CLICK_MODE_PICKUP));
        }
        self.play_ui_sound(ui_sound::UiSound::Click);
    }

    // Klick auf Knopf oder Truhe des Rezeptbuchs; true = verbraucht.
    pub(crate) fn recipe_book_click(&mut self, layout: &inventory::ScreenLayout, button: i8) -> bool {
        let (cx, cy) = self.cursor_pos;
        if self.recipe_button_rect(layout).is_some_and(|r| r.contains(cx, cy)) {
            if button == 0 {
                self.recipe_book.open = !self.recipe_book.open;
                self.play_ui_sound(ui_sound::UiSound::Click);
            }
            return true;
        }
        let Some(panel) = self.recipe_panel(layout).filter(|p| p.contains(cx, cy)) else { return false };
        let (fx, fy, fw, fh) = panel.filter();
        if button == 0 && (Rect { x: fx, y: fy, w: fw, h: fh }).contains(cx, cy) {
            self.recipe_book.craftable_first = !self.recipe_book.craftable_first;
            self.recipe_book.scroll = 0;
            self.play_ui_sound(ui_sound::UiSound::Click);
            return true;
        }
        if let Some(i) = panel.slot_at(cx, cy) {
            let shown = self.recipe_shown();
            if let Some(&eintrag) = shown.get(self.recipe_book.row(shown.len()) * recipe_book::COLS + i) {
                self.recipe_click(eintrag, button);
            }
        }
        true
    }

    // Den Stapel unter dem Zeiger wegwerfen (SlotActionType.THROW). Vanilla:
    //     onMouseClick(slot, slot.id, hasControlDown() ? 1 : 0, SlotActionType.THROW)
    // Button 0 wirft EIN Stueck, Button 1 den ganzen Stapel -- deshalb liegt der ganze
    // Stapel auf Strg. Nur mit LEEREM Zeiger: haelt man etwas, ist Vanillas Wurf der Klick
    // ins Leere (s. inventory_click), nicht diese Aktion.
    //
    // Anders als Vanilla zaehlt ein Stapel am Zeiger als der Stapel unter dem Zeiger: er wird
    // geworfen (ein Stueck, mit "Stapel verschieben" ganz) -- ueber den Wurf-Slot -999.
    pub(crate) fn inventory_throw(&mut self) {
        if self.creative_shown() {
            self.creative_throw();
            return;
        }
        if !self.inventory.screen_open {
            return;
        }
        if self.drop_whole_stack_held() {
            self.inventory_throw_sweep(true);
            return;
        }
        if self.inventory.cursor.is_some() {
            self.send_container_click(inventory::DROP_SLOT, 1, protocol::packets::CLICK_MODE_PICKUP);
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let Some(slot) = layout.slot_at(cx, cy) else { return };
        self.send_container_click(slot as i16, 0, protocol::packets::CLICK_MODE_THROW);
    }

    // Stapel-Wurf: solange Wegwerfen und "Stapel verschieben" beide gehalten sind, fliegt jeder
    // Stapel, ueber den der Zeiger faehrt, sofort ganz raus -- je Betreten eines Slots einmal.
    // Fuellt sich der Slot unter dem Zeiger neu, bleibt das liegen, bis der Zeiger ihn verlaesst
    // und wieder betritt oder eine der beiden Tasten neu gedrueckt wird (`neu`). Der Stapel am
    // Zeiger zaehlt als immer ueberfahren: er fliegt beim Druck mit.
    pub(crate) fn inventory_throw_sweep(&mut self, neu: bool) {
        if !self.inventory.screen_open || self.creative_shown() {
            return;
        }
        let gehalten = self.keybind_physical_down[KeyBindAction::InventoryDropItem.index()] && self.drop_whole_stack_held();
        if !gehalten {
            self.throw_sweep_slot = None;
            return;
        }
        if neu {
            self.throw_sweep_slot = None;
            if self.inventory.cursor.is_some() {
                self.send_container_click(inventory::DROP_SLOT, 0, protocol::packets::CLICK_MODE_PICKUP);
            }
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let slot = layout.slot_at(cx, cy);
        if slot == self.throw_sweep_slot {
            return;
        }
        self.throw_sweep_slot = slot;
        if let Some(slot) = slot.filter(|&s| self.inventory.slot_at_index(s).is_some()) {
            self.send_container_click(slot as i16, 1, protocol::packets::CLICK_MODE_THROW);
        }
    }

    // Stapel-Verschieben per Doppelklick: ALLE Stapel derselben Sorte wandern hinueber.
    // Vanilla schickt dafuer keine eigene Aktion, sondern eine Kette einzelner QUICK_MOVEs
    // (HandledScreen.mouseClicked, Zweig `doubleClicking && hasShift`) -- genau das hier.
    //
    // Vanilla beschraenkt die Kette auf Slots DESSELBEN Inventars wie der angeklickte
    // (`slot2.inventory == slot.inventory`): ein Doppelklick in der Kiste raeumt die Kiste,
    // nicht zusaetzlich den eigenen Rucksack. Bei uns ist die Grenze dieselbe -- der
    // Spielerteil eines Containers sind die letzten 36 Slots (s. inventory::set_slot).
    pub(crate) fn quick_move_all(&mut self, slot: usize) {
        let Some(vorlage) = self.inventory.slot_at_index(slot).cloned() else { return };
        let (_, _, slots) = self.inventory.active();
        let anzahl = slots.len();
        let spieler_start = if self.inventory.screen_open { anzahl.saturating_sub(36) } else { 0 };
        let im_spielerteil = |index: usize| index >= spieler_start;
        let ziel_ist_spieler = im_spielerteil(slot);

        let betroffen: Vec<usize> = (0..anzahl)
            .filter(|&index| im_spielerteil(index) == ziel_ist_spieler)
            .filter(|&index| slots[index].as_ref().is_some_and(|stack| stack.stackable_with(&vorlage)))
            .collect();
        for index in betroffen {
            self.send_container_click(index as i16, 0, protocol::packets::CLICK_MODE_QUICK_MOVE);
        }
    }

    // Zifferntaste ueber einem Slot: Vanillas Tauschen mit dem entsprechenden Hotbar-Platz
    // (Modus SWAP, Button = Hotbar-Index).
    pub(crate) fn inventory_swap_with_hotbar(&mut self, hotbar_index: usize) {
        if self.creative_shown() {
            self.creative_swap(hotbar_index);
            return;
        }
        // Mit einem Stapel am Zeiger tut Vanilla hier nichts. Bei uns kommt er auf den
        // Hotbar-Platz, und was dort lag, an den Zeiger -- ein gewoehnlicher Klick auf den Platz.
        if self.inventory.cursor.is_some() {
            if let Some(ziel) = self.inventory.hotbar_screen_slot(hotbar_index) {
                self.send_container_click(ziel as i16, 0, protocol::packets::CLICK_MODE_PICKUP);
            }
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let layout = inventory::screen_layout(&self.inventory, gpu.size(), self.inventory_scale(gpu.size()));
        let (cx, cy) = self.cursor_pos;
        let Some(slot) = layout.slot_at(cx, cy) else { return };
        // Craft-Ergebnis auf einen belegten Platz: Vanillas Tausch scheitert, weil ins
        // Ergebnisfeld nichts hinein darf. Stattdessen erst das Item vom Platz an den Zeiger
        // nehmen, dann das Ergebnis per Tausch auf den nun leeren Platz.
        let ergebnisfeld = slot == 0 && (self.inventory.open.is_none() || self.open_menu_type() == Some(inventory::MENU_CRAFTING));
        if ergebnisfeld && self.inventory.slot_at_index(0).is_some() {
            if let Some(ziel) = self.inventory.hotbar_screen_slot(hotbar_index).filter(|&z| self.inventory.slot_at_index(z).is_some()) {
                self.send_container_click(ziel as i16, 0, protocol::packets::CLICK_MODE_PICKUP);
                self.send_container_click(0, hotbar_index as i8, protocol::packets::CLICK_MODE_SWAP);
                return;
            }
        }
        let (sync_id, revision, _) = self.inventory.active();
        // Wie bei jedem anderen Klick erst vorhersagen, dann senden, dann lokal anwenden --
        // vorher ging hier NUR das Paket raus und die Anzeige stand still, bis der Server
        // antwortete, und Hotbar-Swaps mit der Taste wirkten langsam.
        let prediction = self.predict_inventory_click(slot as i16, hotbar_index as i8, protocol::packets::CLICK_MODE_SWAP);
        let (modified, cursor) = match &prediction {
            // Nicht hashbar: trotzdem mitschicken, s. inventory_click.
            Some(p) => (p.modified.clone(), p.cursor.clone()),
            // Tauschen laesst den Cursor unveraendert -- den koennen wir also immer exakt
            // melden, die getauschten Slots ohne Vorhersage nicht.
            None => (Vec::new(), self.inventory.cursor.clone()),
        };
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::click_container_payload(
                sync_id,
                revision,
                slot as i16,
                hotbar_index as i8,
                protocol::packets::CLICK_MODE_SWAP,
                &modified,
                cursor.as_ref(),
            ));
        }
        if let Some(prediction) = prediction {
            self.inventory.apply_prediction(&prediction);
        }
    }

    // Was eine Einmal-Aktion (s. KeyBindAction::is_press_action) beim Druck tut.
    pub(crate) fn trigger_press_action(&mut self, action: KeyBindAction) {
        // Offenes Modul-Menue: es ist ein Bildschirm wie das Inventar. Ausser seiner eigenen
        // Taste (schliessen) wirkt keine Spielaktion hindurch -- sonst liesse sich darin
        // das Inventar oeffnen.
        if self.module_menu_open
            && !matches!(
                action,
                KeyBindAction::OpenModuleMenu
                    | KeyBindAction::VoiceConfig
                    | KeyBindAction::VolumeUp
                    | KeyBindAction::VolumeDown
                    | KeyBindAction::ToggleDebugOverlay
                    | KeyBindAction::ToggleProfiler
            )
        {
            return;
        }
        match action {
            KeyBindAction::TogglePerspective => self.perspective = self.next_perspective(),
            KeyBindAction::OpenInventory => self.toggle_inventory(),
            KeyBindAction::Attack => {
                // Vanillas Schwungdauer fuer Spieler: 6 Ticks. Solange richtet sich der
                // Koerper zur Blickrichtung aus (s. own_swing_ticks). +1 = angestossen, der
                // Schwung selbst beginnt erst mit dem naechsten Tick.
                // Nicht, wenn der Klick dort mangels Aufladung verpufft (Speere): sonst
                // stach der Speer sichtbar bei jedem Klick. Der Tick zaehlt vor dem Angriff
                // noch einen weiter, daher +1.
                // Auch nicht waehrend einer Benutzung (Schild oben, Essen, Bogen): dort verwirft
                // tick_item_use den Klick wie Vanilla, ein Schwung kaeme ohne Angriff.
                if !self.attack_refused(1) && !self.using_item {
                    self.own_swing_ticks = restart_swing(self.own_swing_ticks, self.own_swing_duration());
                }
                self.attack_click();
            }
            // Nur im Spiel: bei offenem Inventar gehoert dieselbe Taste der
            // Inventar-Variante (s. InventoryDropItem), sonst wuerfe ein Druck beides weg.
            KeyBindAction::DropItem if !self.screen_overlay_open() => self.drop_held_item(),
            KeyBindAction::InventoryDropItem => self.inventory_throw(),
            // Bei offenem Inventar tauscht die Taste wie in Vanilla den Slot unter dem Zeiger mit
            // der Zweithand (Totem nachlegen!), statt die Haende zu tauschen -- sonst liessen
            // sich Totems so nicht in die Offhand legen.
            KeyBindAction::SwapOffhand => {
                if self.inventory.screen_open {
                    self.inventory_swap_with_hotbar(inventory::OFFHAND_SWAP_BUTTON as usize);
                } else {
                    self.swap_offhand();
                }
            }
            KeyBindAction::OpenChat => {
                if self.chat_input.is_none() && !self.inventory.screen_open {
                    self.open_chat();
                } else if self.creative_shown() && self.creative.kind() != creative::TabKind::Search {
                    // Im Kreativinventar springt die Chat-Taste in die Suche.
                    if let Some(i) = self.creative.tabs.iter().position(|t| t.kind == creative::TabKind::Search) {
                        self.creative.select(i);
                    }
                }
            }
            KeyBindAction::PickBlock => {
                // Zuschauer: kein Menue mehr (zum Teleportieren gibt es /tp), also nichts.
                if self.game_mode == GAME_MODE_SPECTATOR && !self.screen_overlay_open() {
                } else if self.creative_shown() {
                    self.creative_clone();
                } else if !self.screen_overlay_open() && self.chat_input.is_none() {
                    self.pick_block_or_entity();
                }
            }
            KeyBindAction::OpenCommand if self.chat_input.is_none() && !self.inventory.screen_open => {
                self.open_chat();
                if let Some(feld) = self.chat_input.as_mut() {
                    feld.push_str("/");
                }
                self.request_command_suggestions();
            }
            KeyBindAction::ToggleDebugOverlay => self.debug_overlay = !self.debug_overlay,
            KeyBindAction::HideHud => self.hud_hidden = !self.hud_hidden,
            KeyBindAction::ToggleProfiler => self.toggle_profiler(),
            KeyBindAction::OpenModuleMenu => self.toggle_module_menu(),
            KeyBindAction::VoiceConfig if self.chat_input.is_none() && !self.inventory.screen_open => self.open_voice_config(),
            KeyBindAction::VoiceMute if self.settings.module_voice_chat && self.chat_input.is_none() => self.voice_flag(VoiceFlag::Muted),
            KeyBindAction::VoiceDeafen if self.settings.module_voice_chat && self.chat_input.is_none() => self.voice_flag(VoiceFlag::Disabled),
            // Tasten laufen ueber tick_volume_keys (Menue und Halten), das Rad ueber
            // wheel_notch -- hier nichts, sonst zaehlte ein Druck im Spiel doppelt.
            KeyBindAction::VolumeUp | KeyBindAction::VolumeDown => {}
            // Bei offenem Bildschirm blaettert Vanilla die Hotbar nicht.
            // Zuschauer: das Rad stellt die Fluggeschwindigkeit wie die beiden Tasten (hoch =
            // schneller). Bleibt lokal, der Server erfaehrt davon nichts.
            // Die Bedingungen bleiben im Arm: als Waechter fiele die Taste bei offenem
            // Bildschirm in die folgenden Arme durch.
            #[allow(clippy::collapsible_match)]
            KeyBindAction::HotbarPrev | KeyBindAction::HotbarNext if self.game_mode == GAME_MODE_SPECTATOR => {
                if !self.screen_overlay_open() {
                    self.spectator_speed_step(action == KeyBindAction::HotbarPrev);
                }
            }
            #[allow(clippy::collapsible_match)]
            KeyBindAction::SpectatorSlower | KeyBindAction::SpectatorFaster => {
                if self.game_mode == GAME_MODE_SPECTATOR && !self.screen_overlay_open() && self.chat_input.is_none() {
                    self.spectator_speed_step(action == KeyBindAction::SpectatorFaster);
                }
            }
            KeyBindAction::HotbarPrev | KeyBindAction::HotbarNext if !self.screen_overlay_open() => {
                self.scroll_hotbar(action == KeyBindAction::HotbarPrev);
            }
            // Ueber einem Slot tauscht die Ziffer mit dem Hotbar-Platz (wie in Vanilla),
            // sonst waehlt sie ihn aus.
            KeyBindAction::Hotbar(slot) => {
                let index = slot.clamp(1, 9) as usize - 1;
                if self.inventory.screen_open {
                    self.inventory_swap_with_hotbar(index);
                } else if self.game_mode == GAME_MODE_SPECTATOR {
                    // Zuschauer haben keine Hotbar und kein Menue.
                } else {
                    // Sofort nur, wenn der aktuelle Slot schon beim Server ist und keine
                    // Klicks mehr auf ihn warten (s. SlotQueue).
                    let sofort = self.inventory.held_slot == self.last_synced_slot && self.use_queue == 0 && !self.server_still_eating();
                    if let Some(slot) = self.slot_queue.press(index, sofort) {
                        self.select_hotbar_slot(slot);
                    }
                }
            }
            // Alle anderen sind Halte-/Umschalt-Aktionen und landen hier nie.
            _ => {}
        }
    }

    // Wendet eine einzelne Tasten-Transition aus dem Raw-Keyboard auf die Steuerung an --
    // dieselbe Keybind-Zuordnung wie der winit-Pfad (s. window_event), nur eben aus evdev.
    pub(crate) fn apply_raw_key(&mut self, code: KeyCode, pressed: bool) {
        let chat_war_zu = self.chat_input.is_none();
        self.apply_bind(Bind::Key(code), pressed);
        // Hat gerade DIESE Taste den Chat geoeffnet, muss ihr winit-Gegenstueck verworfen
        // werden: der evdev-Leser grabbt die Tastatur NICHT exklusiv (s. raw_input.rs, kein
        // EVIOCGRAB), also meldet winit denselben Druck ein zweites Mal. Weil der Chat dann
        // schon offen ist, greift die Weiche in window_event nicht mehr und die Taste landet
        // als ZEICHEN im Eingabefeld -- man oeffnet den Chat und hat schon ein "t" darin.
        if chat_war_zu && self.chat_input.is_some() {
            self.swallow_text_key = Some((code, Instant::now()));
        }
    }

    // Eine Eingabe (Taste ODER Maustaste) auf die ihr zugeordnete Aktion anwenden -- eine
    // Stelle fuer alle vier Quellen: winit-Tastatur, evdev-Tastatur, winit-Maus, evdev-Maus.
    // Eine Eingabe auf ihre Aktion(en) anwenden. Kein `return` nach dem ersten Treffer: eine
    // Taste darf mehreren Aktionen gehoeren (wie in Vanilla), dann loesen alle aus.
    // Eine MAUStaste bei offenem Inventar: nur die Aktionen, die dort etwas bedeuten -- Tausch
    // mit Zweithand und Hotbar-Platz (Vanilla nimmt dafuer in HandledScreen.mouseClicked auch
    // Maustasten). Alles andere (Perspektive, Sprint, ...) bleibt im Inventar aus.
    // Frueher liess sich mit "Zweithand tauschen" auf der mittleren Maustaste im Inventar
    // nichts tauschen -- der Roh-Pfad warf ausser Links/Rechts jede Maustaste weg.
    pub(crate) fn apply_inventory_mouse_bind(&mut self, bind: Bind, pressed: bool) {
        let kb = self.settings.keybinds;
        for action in KeyBindAction::ALL {
            let zaehlt = matches!(action, KeyBindAction::SwapOffhand | KeyBindAction::Hotbar(_) | KeyBindAction::PickBlock);
            if zaehlt && kb.binds(action).any(|b| b == bind) {
                self.apply_key_action(action, pressed);
            }
        }
    }

    pub(crate) fn apply_bind(&mut self, bind: Bind, pressed: bool) {
        // Beim Verbinden gibt es noch nichts zu steuern; Loslassen kommt durch, damit nichts
        // haengen bleibt.
        if pressed && self.connecting_view() {
            return;
        }
        // Addons sehen jede Steuertaste zuerst und duerfen sie schlucken (Hook::Key/Mouse/Scroll).
        let ereignis = match bind {
            Bind::Key(code) => coral_addon::Event::Key { key: keycode_name(code), pressed },
            Bind::Mouse(BindMouse::WheelUp) => coral_addon::Event::Scroll { delta: 1.0 },
            Bind::Mouse(BindMouse::WheelDown) => coral_addon::Event::Scroll { delta: -1.0 },
            Bind::Mouse(m) => coral_addon::Event::Mouse { button: m.addon_index(), pressed },
        };
        // Das Rad kennt kein Loslassen (s. BindMouse::WheelUp): nur der Druck zaehlt.
        let rad = matches!(ereignis, coral_addon::Event::Scroll { .. });
        if (pressed || !rad) && self.addon_hook(ereignis) == coral_addon::Flow::Cancel {
            return;
        }
        let kb = self.settings.keybinds;
        for action in KeyBindAction::ALL {
            if kb.binds(action).any(|b| b == bind) {
                self.apply_key_action(action, pressed);
            }
        }
        // Inventar-Aktionen ohne eigene Taste erben die ihres Welt-Gegenstuecks (s.
        // KeyBindAction::inventory_fallback). Getrennt vom Durchgang oben, damit eine eigene
        // Belegung IMMER Vorrang hat und nicht zusaetzlich zur geerbten feuert.
        for action in KeyBindAction::ALL.into_iter().filter(|a| a.is_inventory_action()) {
            if kb.count(action) > 0 {
                continue;
            }
            let Some(fallback) = action.inventory_fallback() else { continue };
            if kb.binds(fallback).any(|b| b == bind) {
                self.apply_key_action(action, pressed);
            }
        }
    }

    // Welche Belegung (Aktion + Slot) unter (px,py, Inhaltskoordinaten) liegt -- nur auf dem
    // Steuerung-Screen und nur ueber einer tatsaechlich belegten. Fuer den Rechtsklick zum
    // Loeschen; die Linksklick-Seite laeuft ueber die normalen Buttons. Auf der Seite steht je
    // Aktion die erste Belegung, im Fenster "Erweitert" alle.
    pub(crate) fn keybind_slot_at(&self, px: f32, py: f32) -> Option<(KeyBindAction, usize)> {
        if self.screen != MenuScreen::SettingsControls {
            return None;
        }
        if let Some(aktion) = self.keybind_advanced {
            let (dx, dy) = self.content_offset();
            let (_, felder, _, _) = self.keybind_advanced_layout(aktion);
            return felder.iter().position(|r| r.contains(px + dx, py + dy)).map(|slot| (aktion, slot));
        }
        controls_rows().iter().enumerate().find_map(|(row, &zeile)| match zeile {
            ControlsRow::Bind(aktion) if self.settings.keybinds.count(aktion) > 0 && keybind_rect(row).contains(px, py) => Some((aktion, 0)),
            _ => None,
        })
    }

    // Wie weit die aktuelle Einstellungsseite gescrollt werden kann: so weit, dass die letzte
    // Zeile ganz unten steht. 0, wenn alles ins Fenster passt oder die Seite nicht scrollt.
    pub(crate) fn page_max_scroll(&self) -> usize {
        let Some(page) = self.screen.settings_page() else { return 0 };
        let h = self.gpu.as_ref().map_or(1080.0, |g| g.size().1);
        settings_max_first(page.rows, page.section_starts(), settings_rows_bottom(h))
    }

    // Scrollstand der aktuellen Seite (in Zeilen, ungeklemmt).
    pub(crate) fn page_scroll(&self) -> smooth_scroll::SmoothScroll {
        match self.screen {
            MenuScreen::SettingsGeneral => self.settings_scroll,
            MenuScreen::SettingsControls => self.keybind_scroll,
            s => self.page_scrolls.get(s.settings_key()).copied().unwrap_or_default(),
        }
    }

    pub(crate) fn page_scroll_mut(&mut self) -> &mut smooth_scroll::SmoothScroll {
        match self.screen {
            MenuScreen::SettingsGeneral => &mut self.settings_scroll,
            MenuScreen::SettingsControls => &mut self.keybind_scroll,
            s => self.page_scrolls.entry(s.settings_key()).or_default(),
        }
    }

    // Die Zeile, auf die die Seite zulaeuft (in die Seite geklemmt, gerundet).
    pub(crate) fn page_target_row(&self) -> usize {
        let max = self.page_max_scroll();
        (self.page_scroll().target(max as f32).round() as usize).min(max)
    }

    // Den Seiten-Scroll fuer settings_row_y veroeffentlichen -- nur Seiten mit settings_page
    // scrollen, alle anderen Screens bekommen die feste Anordnung.
    pub(crate) fn sync_settings_scroll(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        if let Some(gpu) = self.gpu.as_ref() {
            let (w, h) = gpu.size();
            UI_SCREEN[0].store(w.to_bits(), Relaxed);
            UI_SCREEN[1].store(h.to_bits(), Relaxed);
        }
        match self.screen.settings_page().filter(|_| self.mode == AppMode::Menu) {
            Some(page) => {
                let h = self.gpu.as_ref().map_or(1080.0, |g| g.size().1);
                THEME_TILE_COUNT.store(theme::PRESETS.len() + self.settings.custom_themes.len() + 1, Relaxed);
                SETTINGS_SCROLL_FIRST.store(self.page_scroll().stand.to_bits(), Relaxed);
                SETTINGS_SCROLL_BOTTOM.store(settings_rows_bottom(h).to_bits(), Relaxed);
                store_section_starts(page.section_starts());
            }
            None => {
                SETTINGS_SCROLL_FIRST.store(0f32.to_bits(), Relaxed);
                SETTINGS_SCROLL_BOTTOM.store(f32::INFINITY.to_bits(), Relaxed);
                store_section_starts(0);
            }
        }
    }

    // `weg` Zeilen Richtung Seitenende (s. App::wheel_units).
    pub(crate) fn scroll_settings(&mut self, weg: f32) {
        let max = self.page_max_scroll() as f32;
        self.page_scroll_mut().push(weg, SETTINGS_SCROLL_ROWS_PER_NOTCH, max);
    }

    // Reiter angeklickt: Abschnitt `i` nach oben gleiten (so weit die Seite reicht).
    pub(crate) fn jump_to_section(&mut self, i: usize) {
        let Some(&(_, row)) = self.screen.settings_page().and_then(|p| p.sections.get(i)) else { return };
        let max = self.page_max_scroll();
        let erste = row.min(max);
        self.page_scroll_mut().glide_to(erste as f32, max as f32);
        self.section_click = Some((self.screen, i, erste));
    }

    // Linksziehen in den Listen wie in osu! (s. smooth_scroll::DragGrip): die Liste unter dem
    // Zeiger, wenn sie gerade scrollt. Nicht auf Reglern ueber Pfeil-Aktionen (die ziehen
    // selbst); Schieberegler, Wertfelder, Skin- und Schrift-Kaestchen haben ihren Druck schon
    // vorher bekommen. Nicht in Listen, deren Klick sofort wirken soll (Serverliste, Inventar)
    // oder in denen Ziehen schon etwas anderes heisst (Skin-Spalten, Chat).
    pub(crate) fn list_drag_list(&self) -> Option<ScrollList> {
        if self.step_slider_at_pointer().is_some() {
            return None;
        }
        let (qx, qy) = self.pointer();
        let liste = if self.module_menu_shown() {
            let gpu = self.gpu.as_ref()?;
            let panel = self.module_menu_rect(gpu);
            // In der Uebersicht scrollen nur die Kacheln, nicht die Kategorien links daneben.
            let links = if self.module_menu_detail { panel.x } else { panel.x + (Self::MODULE_CAT_W + Self::MODULE_PAD) * self.module_scale(gpu) };
            (panel.contains(qx, qy) && qx >= links).then_some(ScrollList::ModuleMenu)?
        } else if self.mode != AppMode::Menu {
            return None;
        } else if matches!(self.screen, MenuScreen::SettingsParticles | MenuScreen::SettingsLightLevels) {
            // Die Eintraege unter der festen Kopfzeile (s. header_list_rows).
            let sichtbar = self.light_visible_rows() as f32;
            let oben = settings_row_y(0) + SETTINGS_ROW_H;
            if !(qx >= 0.0 && qy >= oben && qy < oben + sichtbar * SETTINGS_ROW_H) {
                return None;
            }
            if self.screen == MenuScreen::SettingsParticles { ScrollList::Particles } else { ScrollList::LightLevels }
        } else if self.screen.settings_page().is_some() {
            let h = self.gpu.as_ref().map_or(1080.0, |g| g.size().1);
            if !(qx >= 0.0 && qy >= SETTINGS_FIRST_Y && qy < settings_rows_bottom(h)) {
                return None;
            }
            ScrollList::SettingsPage(self.screen)
        } else {
            return None;
        };
        self.drag_scroll_geometry(liste).filter(|&(_, _, max)| max > 0.0).map(|_| liste)
    }

    // Aufgeklappte Liste: gezogen wird im Kasten, wenn nicht alle Eintraege hineinpassen.
    pub(crate) fn dropdown_drag_list(&self) -> Option<ScrollList> {
        let d = self.dropdown_active()?;
        let (kasten, _) = self.dropdown_layout(d);
        let (cx, cy) = self.cursor_pos;
        let passt = self.choice_options(d.choice).0.len() <= DROPDOWN_MAX_ITEMS;
        (!passt && kasten.contains(cx, cy)).then_some(ScrollList::Dropdown)
    }

    // Pixel je Einheit, Einheiten je Mausrad-Raste (fuer den Widerstand am Rand) und Ende der
    // Liste; None, wenn es sie gerade nicht (mehr) gibt.
    pub(crate) fn drag_scroll_geometry(&self, liste: ScrollList) -> Option<(f32, f32, f32)> {
        let menue = |screen: MenuScreen| self.mode == AppMode::Menu && self.screen == screen;
        match liste {
            ScrollList::SettingsPage(screen) => {
                let page = self.screen.settings_page().filter(|_| menue(screen))?;
                // Zeilen liegen nicht gleich weit auseinander (Abschnittsluecken): die Hoehe der
                // Zeile, in der der Stand gerade steht.
                let f = self.page_scroll().stand.floor();
                let px = settings_row_pos(f + 1.0, page.section_starts()) - settings_row_pos(f, page.section_starts());
                Some((px, SETTINGS_SCROLL_ROWS_PER_NOTCH, self.page_max_scroll() as f32))
            }
            ScrollList::Particles => menue(MenuScreen::SettingsParticles).then(|| (SETTINGS_ROW_H, SETTINGS_SCROLL_ROWS_PER_NOTCH, self.particle_max_scroll())),
            ScrollList::LightLevels => menue(MenuScreen::SettingsLightLevels).then(|| (SETTINGS_ROW_H, SETTINGS_SCROLL_ROWS_PER_NOTCH, self.light_max_scroll())),
            ScrollList::ModuleMenu => {
                let gpu = self.gpu.as_ref().filter(|_| self.module_menu_shown())?;
                let px = if self.module_menu_detail { SETTINGS_ROW_H } else { (Self::MODULE_TILE_H + Self::MODULE_TILE_GAP) * self.module_scale(gpu) };
                Some((px, SETTINGS_SCROLL_ROWS_PER_NOTCH, self.module_max_scroll(gpu) as f32))
            }
            ScrollList::Dropdown => {
                let d = self.dropdown_active()?;
                Some((DROPDOWN_ITEM_H, 1.0, self.choice_options(d.choice).0.len().saturating_sub(DROPDOWN_MAX_ITEMS) as f32))
            }
            ScrollList::Dialog | ScrollList::CodeOfConduct => None,
        }
    }

    // Linke Taste in `liste` gedrueckt: der Klick wartet bis zum Loslassen (s.
    // drag_scroll_release). Ohne Liste false, dann geht der Druck seinen gewohnten Weg.
    pub(crate) fn drag_scroll_grab(&mut self, liste: Option<ScrollList>) -> bool {
        let Some(liste) = liste else { return false };
        self.drag_scroll = Some((liste, smooth_scroll::DragGrip::new(self.cursor_pos)));
        true
    }

    // Den Scrollstand der Liste und ihre Masse (Pixel je Einheit, Raste, Ende) zusammen.
    fn with_drag_scroll(&mut self, liste: ScrollList, f: impl FnOnce(&mut smooth_scroll::SmoothScroll, f32, f32, f32)) {
        if let ScrollList::Dialog | ScrollList::CodeOfConduct = liste {
            if let Some((s, raste, max)) = self.dialog_drag_scroll(liste == ScrollList::CodeOfConduct) {
                f(s, 1.0, raste, max);
            }
            return;
        }
        let Some((px, raste, max)) = self.drag_scroll_geometry(liste) else { return };
        let s = match liste {
            ScrollList::SettingsPage(_) => self.page_scroll_mut(),
            ScrollList::Particles => &mut self.particle_scroll,
            ScrollList::LightLevels => &mut self.light_scroll,
            ScrollList::ModuleMenu => &mut self.module_menu_scroll,
            ScrollList::Dropdown => match self.dropdown.as_mut() {
                Some(d) => &mut d.scroll,
                None => return,
            },
            ScrollList::Dialog | ScrollList::CodeOfConduct => return,
        };
        f(s, px, raste, max);
    }

    // Zeiger bewegt bei gedrueckter Taste.
    pub(crate) fn drag_scroll_motion(&mut self) {
        let Some((liste, mut griff)) = self.drag_scroll.take() else { return };
        let (cursor, jetzt) = (self.cursor_pos, Instant::now());
        self.with_drag_scroll(liste, |s, px, raste, max| griff.motion(cursor, jetzt, s, px, raste, max));
        self.drag_scroll = Some((liste, griff));
    }

    // Je Bild nach dem Gleiten (s. tick_ui_anim): die gezogene Liste bleibt am Zeiger.
    pub(crate) fn drag_scroll_hold(&mut self) {
        let Some((liste, griff)) = self.drag_scroll.take() else { return };
        self.with_drag_scroll(liste, |s, _, _, _| griff.hold(s));
        self.drag_scroll = Some((liste, griff));
    }

    // Linke Taste losgelassen: gezogen -> mit Schwung weiter; nicht gezogen -> jetzt erst der
    // Klick, den der Druck ausgeloest haette.
    pub(crate) fn drag_scroll_release(&mut self) {
        let Some((liste, griff)) = self.drag_scroll.take() else { return };
        if griff.moved {
            let (jetzt, rate) = (Instant::now(), scroll_inertia_rate(self.settings.scroll_inertia));
            self.with_drag_scroll(liste, |s, _, _, max| griff.release(jetzt, s, rate, max));
            return;
        }
        match liste {
            ScrollList::Dialog | ScrollList::CodeOfConduct if self.dialog_shown() || self.code_of_conduct.is_some() => self.dialog_press(false),
            ScrollList::Dropdown if self.dropdown.is_some() => {
                self.dropdown_press();
            }
            ScrollList::SettingsPage(_) | ScrollList::Particles | ScrollList::LightLevels | ScrollList::ModuleMenu if self.drag_scroll_geometry(liste).is_some() => {
                let (px, py) = self.cursor_pos;
                self.handle_click(px, py);
            }
            // Die Liste ist inzwischen weg (Seite gewechselt, Fenster zu): kein Klick ins Leere.
            _ => {}
        }
    }

    // Hervorgehobener Reiter: der zuletzt angeklickte, solange die Seite noch dorthin laeuft,
    // sonst der Abschnitt der obersten sichtbaren Zeile.
    pub(crate) fn active_section(&self, page: SettingsPage) -> usize {
        let max = self.page_max_scroll() as f32;
        let erste = self.page_scroll().stand.clamp(0.0, max).round() as usize;
        match self.section_click {
            Some((screen, i, at)) if screen == self.screen && at == self.page_target_row() => i,
            _ => page.sections.iter().rposition(|&(_, row)| row <= erste).unwrap_or(0),
        }
    }

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
            // Offenes Spielmodus-Rad: die Maus waehlt darin, der Kopf steht.
            AppMode::World if self.gamemode_wheel.is_some() => self.gamemode_wheel_mouse(delta),
            AppMode::World => {
                // mouse_sensitivity ist auf ein Radiant-Delta pro Pixel kalibriert (unveraendert
                // seit vor der Grad-Umstellung der Kamera, s. camera.rs) -- deshalb hier erst in
                // Radiant rechnen und dann fuers Grad-Feld umrechnen, statt die Kalibrierung
                // aller Settings-Presets anzufassen.
                let zoom = zoom_sens_factor(self.settings.zoom_sens_percent, self.zoom_amount);
                let (sens_x, sens_y) = camera_sensitivities(&self.settings);
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
                    || self.page_max_scroll() > 0
                    || self.screen == MenuScreen::SettingsModules
                    // Einstellungen mit eigenen Listen: ueber ihnen scrollt das Rad die Liste,
                    // nie die Lautstaerke -- auch wenn die Liste gerade ganz hineinpasst.
                    || (self.screen == MenuScreen::SettingsSkin
                        && (self.skin_files.is_some()
                            || [skin_tab::Column::Available, skin_tab::Column::Selected].into_iter().any(|c| skin_column_rect(c).contains(px, py))))
                    || matches!(self.screen, MenuScreen::SettingsParticles | MenuScreen::SettingsLightLevels)
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
            || self.server_edit.is_some()
            || (self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsLightLevels);
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
        let eingefahren = self.volume_entered_at.map(|t| t.elapsed().as_secs_f32()).unwrap_or(f32::INFINITY);
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
        let mut g = UiOverlayGroup { rects: Vec::new(), texts: Vec::new(), sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 };
        g.rects.push(UiRect::framed(Rect { x, y, w, h }, a(theme::with_alpha(tg(Slot::BgPanel), 0.94)), 6.0, 1.0, a(tg(Slot::Border1))));
        g.texts.push(UiText { text: "Lautstärke".to_string(), x: x + pad, y: y + pad, color: a(tg(Slot::AccentText)), scale: UI_SCALE_SMALL });
        let wert = self.settings.master_volume;
        g.texts.push(UiText { text: volume_text(wert), x: x + pad, y: y + pad + klein + 6.0, color: a(theme::text()), scale: UI_SCALE_NORMAL });
        let balken = Rect { x: x + pad, y: y + h - pad - VOLUME_BAR_H, w: w - 2.0 * pad, h: VOLUME_BAR_H };
        g.rects.push(UiRect { rect: balken, color: a(tg(Slot::SurfaceSunken)), ..UiRect::FLAT });
        g.rects.push(UiRect { rect: Rect { w: balken.w * wert / 100.0, ..balken }, color: a(theme::accent()), ..UiRect::FLAT });
        overlay.groups.push(g);
    }

    // Eine Benachrichtigung oben rechts zeigen (s. notifications). Der Weg fuer alles im
    // Client, was dem Spieler etwas mitteilen will, ohne dafuer den Chat zu benutzen.
    // Raketen am eigenen Spieler (Elytra-Schub). In Vanilla tickt die Rakete als eigenes
    // Entity NACH dem Spieler (der steht vorn in der Entity-Liste) und schiebt dabei dessen
    // Geschwindigkeit an -- das wirkt also erst auf die Bewegung des naechsten Ticks. Solange
    // der Server die Rakete nicht entfernt, schiebt sie weiter; mehrere schieben nacheinander.
    pub(crate) fn tick_firework_boost(&mut self) {
        let Some(id) = self.own_entity_id else { return };
        if !self.physics.gliding {
            return;
        }
        let raketen = self
            .entities
            .iter()
            .filter(|(_, e)| e.firework_shooter == Some(id) && world::entity::entity_type_name(e.entity_type) == Some("firework_rocket"))
            .count();
        let rotation = physics::rotation_vector(self.look_pitch_deg(), self.camera.yaw);
        for _ in 0..raketen {
            self.physics.vel = physics::firework_boost(self.physics.vel, rotation);
        }
    }

    // Die Zeiger-Effekte, wie sie in diesem Augenblick aussehen.
    pub(crate) fn cursor_fx_frame(&self) -> CursorFxFrame {
        let jetzt = Instant::now();
        CursorFxFrame {
            // Nur die einstellbare Groesse (Settings::cursor_size). Das Zusammenziehen beim
            // Druecken (cursor_fx.scale) ist bewusst NICHT mehr drin -- so gewuenscht.
            // cursor_polygons skaliert um die Spitze, der Klickpunkt bleibt also stehen.
            scale: self.settings.cursor_size as f32 / 100.0,
            rotation_deg: self.cursor_fx.rotation_deg(jetzt),
            additive: self.cursor_fx.additive_alpha(jetzt),
            right: self.cursor_fx.right_tint(),
            circle: self.settings.cursor_circle,
            width: self.settings.cursor_width as f32 / 100.0,
        }
    }

    // Mauszeiger bewegen -- MIT der Zeiger-Empfindlichkeit, die von der Kamera-Empfindlichkeit
    // getrennt ist (s. DEFAULT_CURSOR_SENSITIVITY). Eine Stelle fuer beide Zeiger-Faelle
    // (Menue und offenes Overlay), damit sie nicht auseinanderlaufen koennen.
    // Zoom-Modul: den Anteil weich zum Ziel fuehren -- voll, solange Modul an, Taste aktiv und
    // nichts ueber der Welt offen ist (im Inventar oder Chat braucht niemand Zoom).
    pub(crate) fn update_zoom(&mut self) {
        let jetzt = Instant::now();
        let dt = self.zoom_updated.map_or(0.0, |t| jetzt.duration_since(t).as_secs_f32());
        self.zoom_updated = Some(jetzt);
        let aktiv = self.settings.module_zoom && self.zoom_held && !self.screen_overlay_open() && self.chat_input.is_none();
        let ziel = if aktiv { 1.0 } else { 0.0 };
        let k = 1.0 - (-dt / ZOOM_SMOOTH_SECS).exp();
        self.zoom_amount += (ziel - self.zoom_amount) * k;
        if (ziel - self.zoom_amount).abs() < 1e-3 {
            self.zoom_amount = ziel;
        }
    }

    pub(crate) fn move_cursor(&mut self, delta: (f64, f64)) {
        let (w, h) = self.gpu.as_ref().map(|g| g.size()).unwrap_or((1920.0, 1080.0));
        let faktor = self.settings.cursor_sensitivity;
        self.cursor_pos.0 = (self.cursor_pos.0 + delta.0 as f32 * faktor).clamp(0.0, w);
        self.cursor_pos.1 = (self.cursor_pos.1 + delta.1 as f32 * faktor).clamp(0.0, h);
        // Die Drehung haengt an der Zeigerbewegung, nicht an den Maus-Deltas: geklemmt am
        // Bildschirmrand bewegt sich der Zeiger nicht mehr, und dann soll er auch nicht
        // weiterdrehen.
        self.cursor_fx.mouse_move(Instant::now(), self.cursor_pos);
        self.selection_drag();
        self.server_field_drag();
        self.server_bar_drag();
        self.slider_drag_update();
        self.step_slider_drag();
        self.color_popup_drag();
        self.dialog_drag();
        self.drag_scroll_motion();
    }

    // Fester Zeitschritt (physics::TICK_DT = 1/20s) statt "Geschwindigkeit * echte
    // Frame-Zeit" wie frueher bei der reinen Fly-Kamera: Vanillas Formeln (Reibung,
    // Beschleunigung, Sprung) sind PRO TICK definiert, nicht pro Sekunde -- bei 240 FPS
    // wuerden 240 Mini-Ticks pro Sekunde etwas anderes ergeben als Vanillas 20, selbst mit
    // korrekt skalierter Geschwindigkeit (die Reibungs-Rueckkopplung ist nicht linear in
    // der Schrittweite). Der Akkumulator haengt echte Zeit an ganze Ticks; bei starkem
    // Rueckstand (z.B. nach Alt-Tab) wird auf hoechstens ein paar Nachhol-Ticks gedeckelt,
    // damit ein langer Haenger nicht Sekunden an Physik in einem Frame nachrechnet.
    // Ein Paar gegensaetzlicher Richtungen aufloesen. Ohne SnapTap unveraendert
    // durchgereicht (beide gedrueckt = beide aktiv = sie heben sich in der Summe auf, wie
    // Vanilla); mit SnapTap gewinnt die zuletzt gedrueckte Taste, die andere zaehlt so
    // lange als losgelassen.
    pub(crate) fn snap_tap_pair(&self, first: KeyBindAction, second: KeyBindAction) -> (bool, bool) {
        resolve_snap_tap(
            (self.action_active(first), self.keybind_press_order[first.index()]),
            (self.action_active(second), self.keybind_press_order[second.index()]),
            self.settings.snap_tap_enabled,
        )
    }

    // Der Tastenzustand, wie er GILT -- nach SnapTap und nach dem Bildschirm-Gate. Genau
    // diese sieben Flags gehen sowohl in die Physik als auch ins PlayerInput-Paket; getrennt
    // berechnet waren sie ein Anticheat-Flag: Vanillas KeyboardInput.tick() baut den
    // Bewegungsvektor aus DENSELBEN Booleans, die es ins Paket schreibt (im 1.21.11-Jar
    // verifiziert), also muss auch hier gelten "was ich sende, ist was ich laufe".
    //   * SnapTap: das Paket meldete bisher A UND D gedrueckt, waehrend der Client dank
    //     SnapTap tatsaechlich strafte. Fuer Grim ist das Stillstand-laut-Eingabe gegen
    //     Bewegung-laut-Position -- also grim.simulation.
    //   * Offener Bildschirm: MinecraftClient.setScreen ruft KeyBinding.unpressAll(), Vanilla
    //     meldet bei offenem Inventar also durchweg false. Wir standen still, meldeten aber
    //     weiter gedrueckte Tasten -- dieselbe Art Widerspruch.
    //
    // "Offener Bildschirm" heisst dabei JEDER Bildschirm, also auch das Pausemenue und die
    // Einstellungen (AppMode::Menu), nicht nur Inventar/Modul-Menue: seit der Tick auf einem
    // Server auch mit offenem Menue weiterlaeuft (s. update_movement, BadPacketsR), waere ein
    // fehlendes Gate hier ein Spieler, der im Pausemenue weiterlaeuft -- in Vanilla
    // unmoeglich, weil setScreen alle Tasten loest.
    pub(crate) fn movement_input_flags(&self) -> (bool, bool, bool, bool, bool, bool, bool) {
        let (forward, backward) = self.snap_tap_pair(KeyBindAction::Forward, KeyBindAction::Backward);
        let (left, right) = self.snap_tap_pair(KeyBindAction::Left, KeyBindAction::Right);
        let open = self.screen_overlay_open() || self.mode != AppMode::World;
        let gate = |pressed: bool| pressed && !open;
        // Was Addons dazu druecken (s. Call::SetInput) -- auch bei offenem Bildschirm.
        let a = self.addon_input();
        (
            gate(forward) || a.forward,
            gate(backward) || a.back,
            gate(left) || a.left,
            gate(right) || a.right,
            gate(self.jump_held) || a.jump,
            gate(self.sneak_held) || a.sneak,
            gate(self.sprint_held) || a.sprint,
        )
    }
}
