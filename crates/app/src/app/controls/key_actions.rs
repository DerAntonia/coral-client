// Hand-Debug-Tasten, gehaltene Aktionen, Autoclicker und Tastenaktionen.

use super::*;

impl App {
    // Hand-Debug: die Tasten stellen den Arm fuers gehaltene Item ein (Haupthand, sonst
    // Nebenhand; Settings::hand_fist_items). Pfeiltasten verschieben die Faust links/rechts und
    // hoch/runter, PgUp/PgDn in der Tiefe, je Druck 0.01 Bloecke; mit dem Knopf
    // "Pfeile: Item" (s. hand_debug_buttons) ebenso das Item selbst. Linksklick auf die Knoepfe. Mit
    // Alt drehen sie den Arm um die Faust (hoch/runter kippen, links/rechts schwenken, PgUp/PgDn
    // rollen), Komma/Punkt verdrehen ihn um seine eigene Laenge, je Druck 2 Grad. Mit Strg drehen
    // sie die Schulter, aus der der Arm kommt, um die Faust, je Druck 2 Grad (s.
    // emit_holding_arm). Im Item-Modus drehen sie mit Alt das Item um seinen Griff (s. item_turn),
    // Strg tut dort nichts. Die Ruecktaste setzt alles so, dass der Arm wie mit leerer Hand liegt,
    // nochmal auf den Standard. true = die Taste ist damit verbraucht.
    // Unter welchem Namen Hand-Debug gerade einstellt: das Item der Haupthand, sonst der Nebenhand
    // (s. tuning_name).
    // Mit "Sehnenhand" beim gespannten Bogen die Werte der Hand an der Sehne (s. string_hand).
    pub(crate) fn hand_debug_tuning_name(&self) -> Option<String> {
        if self.hand_debug_string {
            return self.string_hand().map(|(_, name)| name);
        }
        match self.own_hand_items() {
            (Some(i), _) => self.tuning_name(i, protocol::packets::HAND_MAIN),
            (None, Some(i)) => self.tuning_name(i, protocol::packets::HAND_OFF),
            _ => None,
        }
    }

    pub(crate) fn hand_debug_key(&mut self, bind: Bind) -> bool {
        if self.hand_debug_scale().is_none() || self.chat_input.is_some() || self.screen_overlay_open() {
            return false;
        }
        if bind == Bind::Mouse(BindMouse::Left) {
            let Some(gpu) = self.gpu.as_ref() else { return false };
            let (cx, cy) = self.cursor_pos;
            let Some(i) = self.hand_debug_buttons(gpu).iter().position(|(r, _)| r.contains(cx, cy)) else { return false };
            if i == 0 {
                self.hand_tuning_like_empty(false);
            } else if i == 1 {
                if let Some(name) = self.hand_debug_tuning_name() {
                    self.settings.hand_fist_items.remove(&name);
                    self.persist_settings();
                }
            } else if i == 2 {
                self.hand_debug_item_mode = !self.hand_debug_item_mode;
            } else if i == 7 || i == 8 {
                self.hand_debug_string = i == 8;
            } else {
                // Bogen in die Hand, oder (derselbe Knopf nochmal) wieder weg.
                let k = (i - 3) as u8;
                self.hand_debug_bow = match &self.hand_debug_bow {
                    Some((alt, _)) if *alt == k => None,
                    _ => world::item_registry::item_id_by_name("bow").map(|id| (k, protocol::item::ItemStack::new(id, 1))),
                };
            }
            return true;
        }
        let Bind::Key(code) = bind else { return false };
        if code == KeyCode::Backspace {
            // Die Sehnenhand ohne Werte liegt schon wie die leere Hand (s. string_hand_tuning).
            match self.hand_debug_string.then(|| self.string_hand()).flatten() {
                Some((_, name)) => {
                    self.settings.hand_fist_items.remove(&name);
                    self.persist_settings();
                }
                None => self.hand_tuning_like_empty(true),
            }
            return true;
        }
        let Some(name) = self.hand_debug_tuning_name() else { return false };
        let item_modus = self.hand_debug_item_mode;
        let (achse, schritt) = if item_modus && self.modifiers.control_key() {
            return true;
        } else if item_modus && self.modifiers.alt_key() {
            match code {
                KeyCode::ArrowUp => (13, -2),
                KeyCode::ArrowDown => (13, 2),
                KeyCode::ArrowLeft => (14, 2),
                KeyCode::ArrowRight => (14, -2),
                KeyCode::PageUp => (15, 2),
                KeyCode::PageDown => (15, -2),
                _ => return false,
            }
        } else if self.modifiers.control_key() {
            match code {
                KeyCode::ArrowUp => (7, -2),
                KeyCode::ArrowDown => (7, 2),
                KeyCode::ArrowLeft => (8, -2),
                KeyCode::ArrowRight => (8, 2),
                KeyCode::PageUp => (9, 2),
                KeyCode::PageDown => (9, -2),
                _ => return false,
            }
        } else if self.modifiers.alt_key() {
            let (achse, grad) = match code {
                KeyCode::ArrowUp => (0, -2),
                KeyCode::ArrowDown => (0, 2),
                KeyCode::ArrowLeft => (1, 2),
                KeyCode::ArrowRight => (1, -2),
                KeyCode::PageUp => (2, 2),
                KeyCode::PageDown => (2, -2),
                KeyCode::Comma => (3, -2),
                KeyCode::Period => (3, 2),
                _ => return false,
            };
            (3 + achse, grad)
        } else {
            // Faust oder (Knopf "Pfeile: Item") das Item selbst.
            let a = if item_modus { 10 } else { 0 };
            match code {
                KeyCode::ArrowLeft => (a, -10),
                KeyCode::ArrowRight => (a, 10),
                KeyCode::ArrowUp => (a + 1, 10),
                KeyCode::ArrowDown => (a + 1, -10),
                KeyCode::PageUp => (a + 2, -10),
                KeyCode::PageDown => (a + 2, 10),
                _ => return false,
            }
        };
        // Ein neuer Eintrag (z. B. eine Bogen-Spannstufe) beginnt bei den Werten, die gerade gelten.
        if !self.settings.hand_fist_items.contains_key(&name) {
            let (main, off) = self.own_hand_items();
            let jetzt = match (main, off) {
                _ if self.hand_debug_string => self.string_hand_tuning(&name),
                (Some(i), _) => self.hand_tuning(i, protocol::packets::HAND_MAIN),
                (None, Some(i)) => self.hand_tuning(i, protocol::packets::HAND_OFF),
                _ => [0; 16],
            };
            self.settings.hand_fist_items.insert(name.clone(), jetzt);
        }
        self.settings.hand_fist_items.entry(name).or_default()[achse] += schritt;
        self.persist_settings();
        true
    }

    pub(crate) fn track_bind(&mut self, bind: Bind, pressed: bool) {
        let war_unten = self.binds_down.contains(&bind);
        self.binds_down.retain(|b| *b != bind);
        if pressed {
            self.binds_down.push(bind);
        }
        if pressed && !war_unten {
            let jetzt = Instant::now();
            let doppelt = self.bind_presses.iter().any(|(b, t, _)| *b == bind && jetzt.duration_since(*t) < INVENTORY_DOUBLE_CLICK);
            self.bind_presses.retain(|(b, ..)| *b != bind);
            self.bind_presses.push((bind, jetzt, doppelt));
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
            if !kb.held(action, |b| self.binds_down.contains(&b)) {
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

    pub(crate) const fn action_active(&self, action: KeyBindAction) -> bool {
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
            KeyBindAction::ForcePlace => self.force_place_held,
            KeyBindAction::PlayerList => self.player_list_held,
            KeyBindAction::Zoom => self.zoom_held,
            KeyBindAction::Freelook => self.freelook_held,
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
            | KeyBindAction::SpectatorFaster
            | KeyBindAction::InspectHeld
            | KeyBindAction::SwapShoulder
            | KeyBindAction::SaveClip
            | KeyBindAction::ToggleHandDebug => false,
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
            KeyBindAction::ForcePlace => self.force_place_held = active,
            KeyBindAction::PlayerList => self.player_list_held = active,
            KeyBindAction::Zoom => self.zoom_held = active,
            KeyBindAction::Freelook => self.set_freelook(active),
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
            | KeyBindAction::SpectatorFaster
            | KeyBindAction::InspectHeld
            | KeyBindAction::SwapShoulder
            | KeyBindAction::SaveClip
            | KeyBindAction::ToggleHandDebug => {}
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
    pub(crate) const fn next_perspective(&self) -> Perspective {
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
        let periode = Duration::from_secs_f64(1.0 / f64::from(self.settings.autoclicker_cps.max(1)));
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
        // Kamera frei vom Koerper: vor Angriff, Benutzen und Block-Nehmen schaut der Spieler
        // dorthin, wo die Kamera zielt -- der Strahl dieser Aktion geht dann schon von dort aus.
        if pressed && matches!(action, KeyBindAction::Attack | KeyBindAction::Use | KeyBindAction::PickBlock) && self.camera_decoupled() {
            self.aim_player_at_camera_target();
            self.aim_until = Some(Instant::now() + SHOULDER_AIM_HOLD);
        }
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
        // Bei offenem Bildschirm, Chat oder Menue schaltet ein Druck nichts um: Vanillas Screen
        // verschluckt die Taste, sie erreicht die KeyBinding nie. Ohne das kippte etwa Shift beim
        // Umsortieren im Inventar den Schleichen-Umschalter (und Strg im Menue den Sprint-Umschalter).
        // Loslassen und Halten-Aktionen laufen weiter (s. restore_held_controls).
        if pressed
            && !action.is_press_action()
            && self.settings.keybind_modes.get(action) == KeybindMode::Toggle
            && (self.mode != AppMode::World || self.screen_overlay_open() || self.chat_input.is_some())
        {
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
}
