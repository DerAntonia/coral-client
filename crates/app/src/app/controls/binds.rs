// Druck-Aktionen, rohe Tasten, Belegungen und Belegungs-Slots.

use super::*;

impl App {
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
            KeyBindAction::InspectHeld if self.chat_input.is_none() && !self.inventory.screen_open => self.start_inspect(),
            KeyBindAction::SwapShoulder if self.chat_input.is_none() => self.shoulder_side = -self.shoulder_side,
            KeyBindAction::ToggleHandDebug if self.chat_input.is_none() => {
                self.settings.module_hand_debug = !self.settings.module_hand_debug;
                self.persist_settings();
            }
            KeyBindAction::SaveClip if self.chat_input.is_none() => self.save_clip(),
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
        let unten = self.binds_down.clone();
        for action in KeyBindAction::ALL {
            let zaehlt = matches!(action, KeyBindAction::SwapOffhand | KeyBindAction::Hotbar(_) | KeyBindAction::PickBlock);
            if zaehlt && Self::bind_hits(&kb, action, bind, pressed, &unten) {
                self.apply_key_action(action, pressed);
            }
        }
    }

    // Wiedergabe: nur die Aktionen fuers Zuschauen (s. App::replay_view_action).
    pub(crate) fn apply_replay_bind(&mut self, bind: Bind, pressed: bool) {
        let kb = self.settings.keybinds;
        let unten = self.binds_down.clone();
        for action in [KeyBindAction::TogglePerspective, KeyBindAction::Freelook, KeyBindAction::OpenInventory] {
            if Self::bind_hits(&kb, action, bind, pressed, &unten) {
                self.replay_view_action(action, pressed);
            }
        }
    }

    // Trifft dieser Druck bzw. dieses Loslassen die Aktion? Ein Druck nur mit dem Modifier der
    // Belegung (s. Keybinds::matches), ein Loslassen der Haupttaste immer -- sonst bliebe eine
    // Aktion haengen, deren Modifier vor ihr losgelassen wurde.
    fn bind_hits(kb: &Keybinds, action: KeyBindAction, bind: Bind, pressed: bool, unten: &[Bind]) -> bool {
        if pressed {
            kb.matches(action, bind, |b| unten.contains(&b))
        } else {
            kb.binds(action).any(|b| b == bind)
        }
    }

    pub(crate) fn apply_bind(&mut self, bind: Bind, pressed: bool) {
        // Beim Verbinden gibt es noch nichts zu steuern; Loslassen kommt durch, damit nichts
        // haengen bleibt.
        if pressed && self.connecting_view() {
            return;
        }
        if pressed && self.hand_debug_key(bind) {
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
        // Beim Druck ist `bind` schon dabei, beim Loslassen schon weg (s. track_bind).
        let unten = self.binds_down.clone();
        for action in KeyBindAction::ALL {
            if Self::bind_hits(&kb, action, bind, pressed, &unten) {
                self.apply_key_action(action, pressed);
            } else if !pressed && kb.modifier_released(action, bind, |b| unten.contains(&b)) {
                // Der Modifier ging vor der Haupttaste hoch: die Belegung gilt nur mit beiden.
                self.apply_key_action(action, false);
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
            if Self::bind_hits(&kb, fallback, bind, pressed, &unten) {
                self.apply_key_action(action, pressed);
            } else if !pressed && kb.modifier_released(fallback, bind, |b| unten.contains(&b)) {
                self.apply_key_action(action, false);
            }
        }
    }

    // Welches Modifier-Feld im Fenster "Erweitert" unter (px,py, Inhaltskoordinaten) liegt, wenn
    // es einen Modifier traegt -- fuer den Rechtsklick zum Loeschen.
    pub(crate) fn keybind_modifier_at(&self, px: f32, py: f32) -> Option<(KeyBindAction, usize)> {
        let aktion = self.keybind_advanced.filter(|_| self.screen == MenuScreen::SettingsControls)?;
        let (dx, dy) = self.content_offset();
        let slot = self.keybind_advanced_layout(aktion).modifiers.iter().position(|r| r.contains(px + dx, py + dy))?;
        self.settings.keybinds.modifier_at(aktion, slot).map(|_| (aktion, slot))
    }

    // Die Eingabe, auf die das Warten (s. awaiting_keybind) gewartet hat: als Taste oder als
    // Modifier des Slots. Das Mausrad kennt kein Halten und taugt nicht als Modifier.
    pub(crate) fn assign_awaited_bind(&mut self, target: KeyBindAction, slot: usize, bind: Bind) {
        let modifier = std::mem::take(&mut self.awaiting_modifier);
        if modifier {
            if matches!(bind, Bind::Mouse(BindMouse::WheelUp | BindMouse::WheelDown)) {
                self.push_log("Tastenbelegung: das Mausrad geht nicht als Modifier");
                return;
            }
            self.settings.keybinds.set_modifier_at(target, slot, Some(bind));
            self.push_log(format!("Tastenbelegung: {} → Modifier {}", target.label(), bind_name(bind)));
        } else {
            self.settings.keybinds.set_at(target, slot, bind);
            self.push_log(format!("Tastenbelegung: {} → {}", target.label(), bind_name(bind)));
        }
        self.persist_settings();
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
            let felder = self.keybind_advanced_layout(aktion).binds;
            return felder.iter().position(|r| r.contains(px + dx, py + dy)).map(|slot| (aktion, slot));
        }
        controls_rows().iter().enumerate().find_map(|(row, &zeile)| match zeile {
            ControlsRow::Bind(aktion) if self.settings.keybinds.count(aktion) > 0 && keybind_rect(row).contains(px, py) => Some((aktion, 0)),
            _ => None,
        })
    }
}
