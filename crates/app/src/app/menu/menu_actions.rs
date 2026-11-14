// Klicks im Menue und die Ausfuehrung der Button-Aktionen.

use super::*;

impl App {
    pub(crate) fn handle_click(&mut self, px: f32, py: f32) {
        // Das Modul-Menue ist ein In-Welt-Overlay, seine Knoepfe liegen also NICHT in
        // buttons() (das gilt nur fuer Menue-Screens). Eigene Trefferpruefung, danach
        // derselbe Weg durch apply_button_action wie jeder Menue-Klick.
        if self.module_menu_shown() {
            let buttons = self.gpu.as_ref().map(|gpu| self.module_menu_buttons(gpu)).unwrap_or_default();
            // In den Einstellungen liegt die Tafel im verschobenen Inhalt (s. content_offset).
            let (dx, dy) = self.content_offset();
            let (qx, qy) = (px - dx, py - dy);
            let hit = buttons.iter().find(|b| b.enabled && self.module_hit(b, qx, qy)).map(|b| b.action);
            if let Some(t) = hit.and_then(Self::color_target_of) {
                let r = buttons.iter().find(|b| b.enabled && self.module_hit(b, qx, qy)).map_or(Rect { x: qx, y: qy, w: 0.0, h: 0.0 }, |b| b.rect);
                self.play_ui_sound(ui_sound::UiSound::Click);
                self.open_color_popup(t, Rect { x: r.x + dx, y: r.y + dy, ..r });
                return;
            }
            // In den Einstellungen liegt daneben noch der Zurueck-Knopf (buttons()).
            if hit.is_none() && self.mode == AppMode::Menu && self.buttons().iter().any(|b| b.hits(px, py)) {
                return self.handle_menu_click(px, py);
            }
            let Some(action) = hit else {
                // Genau wie in den Menue-Screens (s. unten): danebengeklickt ist stumm, auf
                // einen ABGEBLENDETEN Knopf geklickt nicht. Ohne das gab es hier gar keine
                // Rueckmeldung -- die Spalte eines abgeschalteten Moduls wirkte dadurch
                // kaputt statt gesperrt.
                if buttons.iter().any(|b| !b.enabled && self.module_hit(b, qx, qy)) {
                    self.play_ui_sound(ui_sound::UiSound::Denied);
                }
                return;
            };
            if action == ButtonAction::Nothing {
                return;
            }
            // Auswahl ab drei Werten: Liste aufklappen statt weiterschalten.
            if let Some(c) = Self::choice_of(action) {
                let r = buttons.iter().find(|b| b.enabled && self.module_hit(b, qx, qy)).map_or(Rect { x: qx, y: qy, w: 0.0, h: 0.0 }, |b| b.rect);
                self.toggle_dropdown(c, Rect { x: r.x + dx, y: r.y + dy, ..r });
                return;
            }
            // Schieberegler: Klick setzt den Wert unter den Zeiger, Ziehen fuehrt weiter (wie
            // in handle_menu_click).
            if let Some(sl) = buttons.iter().find(|b| b.enabled && self.module_hit(b, qx, qy)).and_then(|b| b.slider) {
                self.play_ui_sound(ui_sound::UiSound::Click);
                if sl.value_box().contains(qx, qy) {
                    self.begin_step_edit(sl.prev);
                    return;
                }
                self.begin_step_drag(sl, false);
                return;
            }
            self.play_ui_sound(ui_sound_for_action(action, &self.settings));
            // "Zu" in den Einstellungen: eine Ebene zurueck statt ein Overlay schliessen.
            if action == ButtonAction::CloseModuleMenu && self.mode == AppMode::Menu {
                self.navigate_back();
                return;
            }
            self.apply_module_action(action);
            self.persist_settings();
            // Einfarbig gewaehlt: gleich das Farbfenster neben dem Farbfeld dazu.
            if action == ButtonAction::SetBlockOutlineComplementary(false) && self.color_popup.is_none() {
                if let Some(feld) = buttons.iter().find(|b| b.action == ButtonAction::BlockOutlineEditor) {
                    self.open_color_popup(ColorTarget::BlockOutline, Rect { x: feld.rect.x + dx, y: feld.rect.y + dy, ..feld.rect });
                }
            }
            return;
        }
        self.handle_menu_click(px, py);
    }

    // Klick auf die Knoepfe eines Menue-Screens (buttons()).
    pub(crate) fn handle_menu_click(&mut self, px: f32, py: f32) {
        // Buttons nur im Menu-Modus.
        if self.mode != AppMode::Menu {
            return;
        }
        // buttons() borgt self immutabel; Aktion erst danach ausfuehren.
        let treffer = self.buttons().into_iter().find(|b| b.enabled && b.hits(px, py));
        // Farbfelder oeffnen das Farbfenster unter sich -- dafuer zaehlt ihr Rechteck.
        if let Some(t) = treffer.as_ref().and_then(|b| Self::color_target_of(b.action)) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            // Die Kreuz-Farbe wirkt nur im Modus Einfarbig: wer sie anklickt, will den.
            if t == ColorTarget::Crosshair {
                self.settings.crosshair_color_mode = 1;
            }
            let anker = treffer.as_ref().map_or(Rect { x: px, y: py, w: 0.0, h: 0.0 }, |b| b.rect);
            self.open_color_popup(t, anker);
            return;
        }
        // Auswahl ab drei Werten: Liste unter dem Knopf aufklappen statt weiterschalten.
        if let Some((c, anker)) = treffer.as_ref().and_then(|b| Self::choice_of(b.action).map(|c| (c, b.rect))) {
            self.toggle_dropdown(c, anker);
            return;
        }
        let hit = treffer.map(|b| b.action);

        let Some(action) = hit else {
            // Danebengeklickt ist stumm; auf einen ABGEBLENDETEN Knopf geklickt nicht -- dort
            // erwartet man eine Reaktion und bekommt sonst gar keine Rueckmeldung.
            if self.buttons().iter().any(|b| !b.enabled && b.hits(px, py)) {
                self.play_ui_sound(ui_sound::UiSound::Denied);
            }
            return;
        };
        // Der schon gewaehlte Knopf einer Auswahl: stumm, es aendert sich nichts.
        if action == ButtonAction::Nothing {
            return;
        }
        // Schieberegler: Klick setzt den Wert unter den Zeiger, Ziehen fuehrt weiter.
        if let Some(sl) = self.buttons().into_iter().find(|b| b.enabled && b.hits(px, py)).and_then(|b| b.slider) {
            self.play_ui_sound(ui_sound::UiSound::Click);
            // Die Zahl angeklickt: eintippen statt ziehen (der Regler liegt in
            // Inhaltskoordinaten, s. pointer).
            let (qx, qy) = self.pointer();
            if sl.value_box().contains(qx, qy) {
                self.begin_step_edit(sl.prev);
                return;
            }
            self.begin_step_drag(sl, false);
            return;
        }
        // Bei den Reglern der einzelnen UI-Klaenge spielt statt des Klicks der Klang selbst,
        // und zwar NACH der Aenderung -- so hoert man die neue Lautstaerke.
        let vorhoeren = match action {
            ButtonAction::UiSoundVolumeDown(i) | ButtonAction::UiSoundVolumeUp(i) => ui_sound::UiSound::ALL.get(i).copied(),
            _ => None,
        };
        if vorhoeren.is_none() {
            let in_einstellungen = self.screen == MenuScreen::Settings || self.screen.settings_category().is_some();
            let klang = match action {
                ButtonAction::ToggleNotificationPanel if self.notification_panel_open => ui_sound::UiSound::Close,
                ButtonAction::ToggleNotificationPanel => ui_sound::UiSound::Open,
                // Das Zahnrad oeffnet die Einstellungen, in ihnen fuehrt es hinaus.
                ButtonAction::ToggleSettings if in_einstellungen => ui_sound::UiSound::Close,
                ButtonAction::ToggleSettings => ui_sound::UiSound::Open,
                // Die Einstellungen von aussen betreten (Optionen im Pausemenue usw.).
                ButtonAction::OpenScreen(ziel) if !in_einstellungen && (ziel == MenuScreen::Settings || ziel.settings_category().is_some()) => ui_sound::UiSound::Open,
                // Offene Kontenliste: derselbe Knopf oeffnet den Acc-Manager, ein Fenster.
                ButtonAction::ToggleAccountMenu if self.account_menu_open => ui_sound::UiSound::Open,
                _ => ui_sound_for_action(action, &self.settings),
            };
            self.play_ui_sound(klang);
        }
        self.run_menu_action(action);
        if let Some(klang) = vorhoeren {
            self.play_ui_sound(klang);
        }
        // Einfarbig gewaehlt: gleich das Farbfenster neben dem Farbfeld dazu.
        if action == ButtonAction::CrosshairColorNext && self.color_popup.is_none() {
            if let Some(feld) = self.buttons().into_iter().find(|b| b.action == ButtonAction::OpenColorPicker(ColorTarget::Crosshair)) {
                self.open_color_popup(ColorTarget::Crosshair, feld.rect);
            }
        }
    }

    // Eine Menue-Aktion ausfuehren (Klick oder ein Schritt eines Schiebereglers).
    pub(crate) fn run_menu_action(&mut self, action: ButtonAction) {
        // Das Fenster "Erweitert" schliesst sich bei allem, was nicht zu ihm gehoert (z.B. ein
        // Klick in die Kategorienleiste).
        if !matches!(action, ButtonAction::Rebind(..) | ButtonAction::RebindModifier(..) | ButtonAction::OpenKeybindAdvanced(_) | ButtonAction::CloseKeybindAdvanced | ButtonAction::Nothing) {
            self.keybind_advanced = None;
        }
        // Alles ausser Tab-Wechsel und Verbindungs-Aktionen veraendert Einstellungen und
        // wird danach sofort geschrieben.
        // TearFree ist ausgenommen: das ist externer Systemzustand, kein Teil unserer
        // Settings-Datei -- ihn "speichern" waere falsch, er lebt schon in xrandr.
        // Rebind ebenfalls: der Klick startet nur das Warten auf die naechste Taste, die
        // eigentliche Aenderung (und ihr persist_settings()) passiert erst in
        // window_event(), wenn diese Taste kommt.
        let changes_settings = !matches!(
            action,
            ButtonAction::OpenScreen(_)
                | ButtonAction::OpenLink(_)
                | ButtonAction::OpenQuickConnect
                | ButtonAction::ToggleAccountMenu
                | ButtonAction::SettingsSection(_)
                | ButtonAction::SearchHit(_)
                | ButtonAction::ToggleSettings
                | ButtonAction::ToggleNotificationPanel
                | ButtonAction::Back
                | ButtonAction::Connect
                | ButtonAction::Disconnect
                | ButtonAction::ToggleTearFree
                | ButtonAction::Rebind(..)
                | ButtonAction::RebindModifier(..)
                | ButtonAction::OpenKeybindAdvanced(_)
                | ButtonAction::CloseKeybindAdvanced
                | ButtonAction::Respawn
                | ButtonAction::HudDeselect
                | ButtonAction::AudioTestTone
                | ButtonAction::PipewireLimit(_)
                | ButtonAction::AudioSpatialTest
                | ButtonAction::OpenSkinFolder
                | ButtonAction::ReloadSkins
                | ButtonAction::OpenConfigFolder
                | ButtonAction::ServerLinks
                | ButtonAction::PauseDialog
                // Configs speichern selbst (s. switch_config); ein Speichern danach schriebe
                // nur dieselben Werte noch einmal.
                | ButtonAction::SelectConfig(_)
                | ButtonAction::EditConfig(_)
                | ButtonAction::DeleteConfig
                | ButtonAction::OpenInputField(_)
                | ButtonAction::SubmitInputField
                // Der Dialog schreibt nur die Bilddatei; die Einstellung folgt, wenn er fertig
                // ist (s. poll_wallpaper).
                | ButtonAction::PickWallpaper
                // Die Server-Liste lebt in servers.conf, nicht in den Einstellungen -- sie
                // schreibt sich selbst (persist_servers), settings.conf bleibt unberuehrt.
                | ButtonAction::ConnectToServer(_)
                | ButtonAction::NewServer
                | ButtonAction::CloseModuleMenu
        );

        let max_scale = self.max_gui_scale_setting();
        let set = &mut self.settings;
        match action {
            ButtonAction::Nothing => {}
            // Klappt nur die Liste auf (s. toggle_dropdown), aendert selbst nichts.
            ButtonAction::OpenChoice(_) => {}
            // Die Modul-Aktionen koennen hier nicht auftreten: das Modul-Menue ist ein
            // In-Welt-Overlay, seine Knoepfe stehen nicht in buttons() und laufen ueber
            // apply_module_action (s. handle_click). Der Arm ist nur da, damit das match
            // vollstaendig bleibt und ein neues Overlay-Element nicht still hier landet.
            ButtonAction::ToggleModuleTotem
            | ButtonAction::ToggleAddonModule(_)
            | ButtonAction::AddonToggle(..)
            | ButtonAction::AddonStep(..)
            | ButtonAction::AddonButton(..)
            | ButtonAction::ToggleModuleTotemSound
            | ButtonAction::Toteming(_)
            | ButtonAction::SelectModule(_)
            | ButtonAction::SelectModuleCategory(_)
            | ButtonAction::ModuleBack
            | ButtonAction::ToggleModuleHitbox
            | ButtonAction::HitboxPlayerRaw(_)
            | ButtonAction::ToggleModuleHitboxDotted
            | ButtonAction::HitboxLines(_)
            | ButtonAction::ToggleModuleHitboxSelf
            | ButtonAction::HitboxDensityPrev
            | ButtonAction::HitboxDensityNext
            | ButtonAction::HitboxOpacityPrev
            | ButtonAction::HitboxOpacityNext
            | ButtonAction::HitboxColorEditor(_)
            | ButtonAction::ToggleHitboxKind(_)
            | ButtonAction::ToggleHitboxKindOverride(_)
            | ButtonAction::ToggleModuleSkyColor
            | ButtonAction::ToggleModuleGhostTolerance
            | ButtonAction::ToggleModuleCrystalOptimizer
            | ButtonAction::ToggleModuleConsumableOptimizer
            | ButtonAction::EatHoldThroughPrev
            | ButtonAction::EatHoldThroughNext
            | ButtonAction::ToggleEatSlotLock
            | ButtonAction::ToggleNoShieldLock
            | ButtonAction::ToggleModuleUseTimer
            | ButtonAction::ToggleModuleNoToolBreak
            | ButtonAction::ToggleModuleBotcheckPass
            | ButtonAction::ToggleModuleDynamicFire
            | ButtonAction::ToggleModuleZoom
            | ButtonAction::ToggleModuleFreelook
            | ButtonAction::FreelookView(_)
            | ButtonAction::ToggleModuleVoiceChat
            | ButtonAction::VoiceStep(..)
            | ButtonAction::VoiceFlag(_)
            | ButtonAction::ZoomFactorPrev
            | ButtonAction::ZoomFactorNext
            | ButtonAction::ZoomSensPrev
            | ButtonAction::ZoomSensNext
            | ButtonAction::NoToolBreakReservePrev
            | ButtonAction::NoToolBreakReserveNext
            | ButtonAction::GhostTolerancePrev
            | ButtonAction::LagDelayPrev
            | ButtonAction::LagDelayNext
            | ButtonAction::GhostToleranceNext
            | ButtonAction::ToggleModuleAutoclicker
            | ButtonAction::ToggleModuleSaturationHud
            | ButtonAction::SaturationHeightPrev
            | ButtonAction::SaturationHeightNext
            | ButtonAction::SaturationAlpha(..)
            | ButtonAction::ToggleModuleEgoName
            | ButtonAction::EditEgoAlias
            | ButtonAction::CycleEgoDeco
            | ButtonAction::CycleEgoShadow
            | ButtonAction::EgoLight(..)
            | ButtonAction::EgoSpeed(_)
            | ButtonAction::CycleEgoSound
            | ButtonAction::EgoVolume(_)
            | ButtonAction::AutoclickerCpsPrev
            | ButtonAction::AutoclickerCpsNext
            | ButtonAction::CycleSkyMode
            | ButtonAction::ToggleSkySun
            | ButtonAction::ToggleSkyMoon
            | ButtonAction::ToggleSkyStars
            | ButtonAction::ToggleSkyFixedTime
            | ButtonAction::SkyTimePrev
            | ButtonAction::SkyTimeNext
            | ButtonAction::ToggleModuleWeather
            | ButtonAction::CycleWeatherMode
            | ButtonAction::WeatherRadiusPrev
            | ButtonAction::WeatherRadiusNext
            | ButtonAction::WeatherSnowRadiusPrev
            | ButtonAction::WeatherSnowRadiusNext
            | ButtonAction::WeatherSnowRadiusMaxPrev
            | ButtonAction::WeatherSnowRadiusMaxNext
            | ButtonAction::WeatherDensityPrev
            | ButtonAction::WeatherDensityNext
            | ButtonAction::WeatherSnowSpeedPrev
            | ButtonAction::WeatherSnowSpeedNext
            | ButtonAction::ToggleModuleBlockOutline
            | ButtonAction::BlockOutlineEditor
            | ButtonAction::BlockOutlineWidthPrev
            | ButtonAction::BlockOutlineWidthNext
            | ButtonAction::ToggleBlockOutlineHideMining
            | ButtonAction::SetBlockOutlineComplementary(_)
            | ButtonAction::ToggleModuleShields
            | ButtonAction::ToggleModuleArrows
            | ButtonAction::ToggleModuleArmorHud
            | ButtonAction::ToggleModuleTeams
            | ButtonAction::TeamToggle(_)
            | ButtonAction::TeamMatch(_)
            | ButtonAction::TeamOwnColor(_)
            | ButtonAction::TeamNametag(..)
            | ButtonAction::TeamOpacity(..)
            | ButtonAction::TeamDensity(..)
            | ButtonAction::TeamDotted(_)
            | ButtonAction::ToggleModuleKeystrokes
            | ButtonAction::ToggleModuleSessionInfo
            | ButtonAction::ToggleModuleClock
            | ButtonAction::ToggleSessionLine(_)
            | ButtonAction::ResetSession
            | ButtonAction::ToggleArrowIndicator
            | ButtonAction::ToggleArrowWhite
            | ButtonAction::ToggleArrowCritTrail
            | ButtonAction::ToggleArrowEmptySound
            | ButtonAction::ToggleArrowTrail
            | ButtonAction::TrailHoldPrev
            | ButtonAction::TrailHoldNext
            | ButtonAction::TrailFadePrev
            | ButtonAction::TrailFadeNext
            | ButtonAction::ToggleShieldCdSelf
            | ButtonAction::ToggleShieldCdPlayers
            | ButtonAction::ToggleShieldHoldPlayers
            | ButtonAction::ShieldColorEditor(_)
            | ButtonAction::CycleShieldMark(_)
            | ButtonAction::CycleArrowEmptyFeedback
            | ButtonAction::ToggleModuleShowMarkers
            | ButtonAction::ToggleModuleSoundDebug
            | ButtonAction::ToggleModuleHandDebug
            | ButtonAction::ToggleModuleConnectionDebug
            | ButtonAction::ToggleModuleDelay
            | ButtonAction::ToggleModuleDiscordStatus
            | ButtonAction::ToggleModuleInputLog
            | ButtonAction::ToggleModuleCoralRecord
            | ButtonAction::ToggleModuleCoralClip
            | ButtonAction::ClipSecondsPrev
            | ButtonAction::ClipSecondsNext
            | ButtonAction::ClipCheckpointsPrev
            | ButtonAction::ClipCheckpointsNext
            | ButtonAction::ToggleModuleColor
            | ButtonAction::ToggleModuleSustainability
            | ButtonAction::ToggleSustainBackground
            | ButtonAction::ToggleSustainPause
            | ButtonAction::ToggleModuleChunkSaver
            | ButtonAction::ToggleChunkSaverRenderSaved
            | ButtonAction::ChunkSaverThreadsPrev
            | ButtonAction::ChunkSaverThreadsNext
            | ButtonAction::ColorSaturationPrev
            | ButtonAction::ColorSaturationNext
            | ButtonAction::DiscordPasteClientId
            | ButtonAction::ShieldColorPrev(_, _)
            | ButtonAction::ShieldColorNext(_, _)
            | ButtonAction::CloseModuleMenu => {}
            ButtonAction::ToggleSettings => {
                if self.screen.settings_category().is_some() {
                    self.screen = self.settings_return;
                } else {
                    self.open_settings(MenuScreen::Settings);
                }
            }
            // Abgefangen in handle_menu_click/handle_click (braucht das Rechteck des Knopfs).
            ButtonAction::OpenColorPicker(_) => {}
            ButtonAction::ApplyThemePreset(i) => {
                if i < theme::PRESETS.len() {
                    set.select_theme(ThemeChoice::Preset(i));
                }
            }
            ButtonAction::SelectCustomTheme(i) => {
                if i < set.custom_themes.len() {
                    set.select_theme(ThemeChoice::Custom(i));
                }
            }
            ButtonAction::NewCustomTheme => {
                self.theme_name_dialog = Some(ThemeNameDialog {
                    field: text_field::TextField::new("", THEME_NAME_MAX),
                    dragging: false,
                    last_press: None,
                    purpose: NameDialogPurpose::Theme,
                });
            }
            ButtonAction::SettingsSection(i) => self.jump_to_section(i as usize),
            ButtonAction::SearchHit(i) => self.open_search_hit(i as usize),
            ButtonAction::ToggleUptime => self.settings.topbar_uptime = !self.settings.topbar_uptime,
            ButtonAction::SelectTab(i) => self.switch_tab(i),
            ButtonAction::CloseTab(i) => self.close_tab(i),
            ButtonAction::NewTab => self.new_tab(),
            ButtonAction::OpenLink(i) => {
                if let Some((_, url, _)) = SIDEBAR_LINKS.get(i) {
                    match open_external_command(url).spawn() {
                        Ok(_) => self.push_log(format!("Link geöffnet: {url}")),
                        Err(e) => self.push_log(format!("Link konnte nicht geöffnet werden ({e}): {url}")),
                    }
                }
            }
            ButtonAction::OpenQuickConnect => {
                self.quick_connect_open = true;
                self.quick_field = text_field::TextField::new("", MAX_ADDRESS_LEN);
                self.quick_dragging = false;
                self.quick_last_press = None;
            }
            ButtonAction::ToggleAccountMenu => {
                if self.account_menu_open {
                    // Offen ist der Knopf der Acc-Manager: er oeffnet das Konten-Fenster.
                    self.account_menu_open = false;
                    self.account_manager_open = true;
                    self.account_context = None;
                } else {
                    self.account_menu_open = true;
                }
                // Koepfe laden und alle Tokens pruefen, sobald die Liste aufgeht.
                self.refresh_account_list();
            }
            ButtonAction::ToggleNotificationPanel => {
                self.set_notification_panel(!self.notification_panel_open);
                self.notifications_seen = self.notification_history.len();
            }
            ButtonAction::OpenScreen(target) if target.settings_category().is_some() || target == MenuScreen::Settings => {
                self.open_settings(target);
            }
            ButtonAction::OpenScreen(target) => {
                if target == MenuScreen::HudEditor {
                    self.hud_editor_return = self.screen;
                }
                self.screen = target;
                // Beim Betreten von Connect/Account ggf. eine gespeicherte Anmeldung lautlos
                // erneuern (Refresh-Token), damit man ohne erneuten Login verbinden kann.
                if target == MenuScreen::Connect || target == MenuScreen::Account {
                    self.try_refresh_login();
                }
            }
            ButtonAction::Back => self.navigate_back(),
            ButtonAction::Connect => self.go_connect(),
            ButtonAction::Disconnect => self.disconnect(),
            ButtonAction::Quit => self.quit_requested = true,
            ButtonAction::Reconnect => self.reconnect(),
            ButtonAction::MicrosoftLogin => self.start_microsoft_login(),
            ButtonAction::OpenInputField(kind) => {
                // Toggle: nochmal auf denselben Button schliesst das Feld; ein anderer wechselt.
                self.input_field = match &self.input_field {
                    Some((k, _)) if *k == kind => None,
                    _ => {
                        let prefill = match kind {
                            InputKind::Token | InputKind::RefreshToken => String::new(),
                            InputKind::ConfigName(ConfigEdit::Rename) => active_config(),
                            InputKind::ConfigName(_) => String::new(),
                        };
                        Some((kind, prefill))
                    }
                };
            }
            ButtonAction::SubmitInputField => self.submit_input_field(),
            ButtonAction::SelectConfig(i) => {
                self.config_delete_armed = false;
                if let Some(name) = assets::list_configs().get(i).cloned() {
                    self.switch_config(&name);
                }
            }
            ButtonAction::EditConfig(edit) => {
                self.config_delete_armed = false;
                let kind = InputKind::ConfigName(edit);
                self.input_field = match &self.input_field {
                    Some((k, _)) if *k == kind => None,
                    _ => Some((kind, if edit == ConfigEdit::Rename { active_config() } else { String::new() })),
                };
            }
            ButtonAction::DeleteConfig => {
                if !std::mem::replace(&mut self.config_delete_armed, true) {
                    return;
                }
                self.config_delete_armed = false;
                let name = active_config();
                match assets::delete_config(&name) {
                    Ok(()) => {
                        let naechste = assets::list_configs().into_iter().next().unwrap_or_else(|| assets::DEFAULT_CONFIG.to_string());
                        // Die geloeschte darf beim Wechsel nicht wieder gespeichert werden.
                        self.switch_config_unsaved(&naechste);
                        self.notify(notifications::Notification::info("Config gelöscht", format!("\"{name}\" -- jetzt aktiv: {naechste}")));
                    }
                    Err(e) => {
                        self.notify(notifications::Notification::error("Config nicht gelöscht", e.to_string()));
                    }
                }
            }
            ButtonAction::Logout => self.logout(),
            ButtonAction::UseAccount(i) => self.use_account(i),
            // --- Server-Liste (Connect-Screen) ---
            // Klick auf eine Karte verbindet sofort.
            ButtonAction::ConnectToServer(i) => {
                if let Some(entry) = self.servers.get(i) {
                    // Adresse ins Eingabefeld links uebernehmen, damit sichtbar ist, womit
                    // gerade verbunden wird (und ein Reconnect ohne Umweg funktioniert).
                    self.server_address = entry.address.clone();
                    self.go_connect();
                }
            }
            ButtonAction::NewServer => self.open_server_dialog(None),
            ButtonAction::ToggleRecordings => self.toggle_recordings(),
            ButtonAction::PlayRecording(i) => self.play_recording(i),
            ButtonAction::ToggleTearing => set.show_tearing_test = !set.show_tearing_test,
            ButtonAction::ToggleHud => set.show_hud = !set.show_hud,
            ButtonAction::ToggleCmaa2 => set.cmaa2_enabled = !set.cmaa2_enabled,
            ButtonAction::FrameLatencyDown | ButtonAction::FrameLatencyUp => {
                let delta: i32 = if action == ButtonAction::FrameLatencyDown { -1 } else { 1 };
                set.frame_latency = (set.frame_latency as i32 + delta).clamp(FRAME_LATENCY_MIN as i32, FRAME_LATENCY_MAX as i32) as u32;
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.set_frame_latency(set.frame_latency);
                }
            }
            ButtonAction::FpsLimitPrev => set.fps_limit = fps_limit_step(set.fps_limit, false).unwrap_or(set.fps_limit),
            ButtonAction::FpsLimitNext => set.fps_limit = fps_limit_step(set.fps_limit, true).unwrap_or(set.fps_limit),
            // Beide Regler stehen immer da; ohne "X/Y getrennt" zieht der eine den anderen mit.
            ButtonAction::SensitivityDown | ButtonAction::SensitivityUp => {
                set.mouse_sensitivity = step_sensitivity(set.mouse_sensitivity, action == ButtonAction::SensitivityUp);
                set.couple_sensitivity(false);
                self.sens_last_changed_y = false;
            }
            ButtonAction::SensitivityYDown | ButtonAction::SensitivityYUp => {
                set.mouse_sensitivity_y = step_sensitivity(set.mouse_sensitivity_y, action == ButtonAction::SensitivityYUp);
                set.couple_sensitivity(true);
                self.sens_last_changed_y = true;
            }
            ButtonAction::ToggleSensSplit => {
                set.mouse_sens_split = !set.mouse_sens_split;
                // Wieder gleich: der Wert, den man NICHT zuletzt verstellt hat, gilt fuer beide.
                if !set.mouse_sens_split {
                    set.couple_sensitivity(!self.sens_last_changed_y);
                }
            }
            ButtonAction::OpenSensCalc => self.screen = MenuScreen::SettingsSensCalc,
            ButtonAction::ApplySensCalc => {
                set.mouse_sensitivity = minecraft_sens_to_client(set.sens_calc_mc_percent).clamp(SENSITIVITY_MIN, SENSITIVITY_MAX);
                set.couple_sensitivity(false);
            }
            ButtonAction::CursorSensitivityDown => set.cursor_sensitivity = step_cursor_sensitivity(set.cursor_sensitivity, false),
            ButtonAction::CursorSensitivityUp => set.cursor_sensitivity = step_cursor_sensitivity(set.cursor_sensitivity, true),
            ButtonAction::CursorSizeDown => set.cursor_size = set.cursor_size.saturating_sub(CURSOR_SIZE_STEP).max(CURSOR_SIZE_MIN),
            ButtonAction::CursorSizeUp => set.cursor_size = (set.cursor_size + CURSOR_SIZE_STEP).min(CURSOR_SIZE_MAX),
            ButtonAction::CursorWidthDown => set.cursor_width = set.cursor_width.saturating_sub(CURSOR_WIDTH_STEP).max(CURSOR_WIDTH_MIN),
            ButtonAction::CursorOutlineDown => set.cursor_outline = set.cursor_outline.saturating_sub(CURSOR_OUTLINE_STEP),
            ButtonAction::CursorOutlineUp => set.cursor_outline = (set.cursor_outline + CURSOR_OUTLINE_STEP).min(CURSOR_OUTLINE_MAX),
            ButtonAction::CursorFillDown => set.cursor_fill_transparency = set.cursor_fill_transparency.saturating_sub(CURSOR_FILL_STEP),
            ButtonAction::CursorFillUp => set.cursor_fill_transparency = (set.cursor_fill_transparency + CURSOR_FILL_STEP).min(100),
            ButtonAction::CursorWidthUp => set.cursor_width = (set.cursor_width + CURSOR_WIDTH_STEP).min(CURSOR_WIDTH_MAX),
            ButtonAction::HoldConfirmDown => set.hold_confirm_ms = set.hold_confirm_ms.saturating_sub(HOLD_CONFIRM_STEP),
            ButtonAction::HoldConfirmUp => set.hold_confirm_ms = (set.hold_confirm_ms + HOLD_CONFIRM_STEP).min(HOLD_CONFIRM_MAX),
            ButtonAction::ReconnectHoldDown => set.reconnect_hold_ms = (set.reconnect_hold_ms - HOLD_CONFIRM_STEP as i32).max(RECONNECT_HOLD_GENERAL),
            ButtonAction::ReconnectHoldUp => set.reconnect_hold_ms = (set.reconnect_hold_ms + HOLD_CONFIRM_STEP as i32).min(HOLD_CONFIRM_MAX as i32),
            ButtonAction::ToggleCursorCircle => set.cursor_circle = !set.cursor_circle,
            ButtonAction::UiPitchDown => set.ui_pitch_semitones -= 1,
            ButtonAction::UiPitchUp => set.ui_pitch_semitones += 1,
            ButtonAction::FovDown(p) => set.set_fov_of(p, set.fov_of(p) - FOV_STEP),
            ButtonAction::SkipPerspective(p) => {
                match p {
                    Perspective::FirstPerson => {}
                    Perspective::ThirdPersonBack => set.skip_third_back = !set.skip_third_back,
                    Perspective::ThirdPersonFront => set.skip_third_front = !set.skip_third_front,
                }
                // Steht man gerade in der jetzt uebersprungenen Ansicht, zurueck ins Auge.
                if set.perspective_skipped(self.perspective) {
                    self.perspective = Perspective::FirstPerson;
                }
            }
            ButtonAction::FovUp(p) => set.set_fov_of(p, set.fov_of(p) + FOV_STEP),
            ButtonAction::CameraDistanceDown(p) => set.set_camera_distance_of(p, set.camera_distance_of(p) - CAMERA_DISTANCE_STEP),
            ButtonAction::CameraDistanceUp(p) => set.set_camera_distance_of(p, set.camera_distance_of(p) + CAMERA_DISTANCE_STEP),
            ButtonAction::CameraYOffsetDown(p) => set.set_camera_y_offset_of(p, set.camera_y_offset_of(p) - CAMERA_Y_OFFSET_STEP),
            ButtonAction::CameraYOffsetUp(p) => set.set_camera_y_offset_of(p, set.camera_y_offset_of(p) + CAMERA_Y_OFFSET_STEP),
            ButtonAction::CycleProjection => set.projection_mode = (set.projection_mode + 1) % PROJECTION_NAMES.len() as u8,
            ButtonAction::CycleTearPosition => set.tear_position = u8::from(set.tear_position == 0),
            ButtonAction::TearTimingDown | ButtonAction::TearTimingUp => {
                let delta = if action == ButtonAction::TearTimingDown { -TEAR_TIMING_STEP_US } else { TEAR_TIMING_STEP_US };
                set.tear_timing_us = (set.tear_timing_us + delta).clamp(TEAR_TIMING_MIN_US, TEAR_TIMING_MAX_US);
            }
            ButtonAction::TearReserveDown | ButtonAction::TearReserveUp => {
                let delta = if action == ButtonAction::TearReserveDown { -TEAR_RESERVE_STEP_US } else { TEAR_RESERVE_STEP_US };
                set.tear_reserve_us = (set.tear_reserve_us + delta).clamp(TEAR_RESERVE_MIN_US, TEAR_RESERVE_MAX_US);
            }
            ButtonAction::CycleParticles(own) => {
                let m = set.particles_mode_of(own);
                *m = (*m + 1) % PARTICLE_MODE_NAMES.len() as u8;
            }
            ButtonAction::CycleDeathAnimation => set.death_animation = (set.death_animation + 1) % DEATH_ANIM_NAMES.len() as u8,
            ButtonAction::ToggleStableNumbers => {
                set.stable_numbers = !set.stable_numbers;
                if let Some(gpu) = self.gpu.as_ref() {
                    let mut zahlen = gpu.stable_numbers.borrow_mut();
                    zahlen.clear();
                    zahlen.set_enabled(set.stable_numbers);
                }
            }
            ButtonAction::ToggleShadowHalf(i) => {
                set.shadow_half[i] = !set.shadow_half[i];
                self.sync_text_shadow();
            }
            ButtonAction::ToggleCapitals => {
                set.font_capitals = !set.font_capitals;
                crate::font_atlas::set_capitals(set.font_capitals);
                // Gemerkte Breiten passen zur alten Schreibung.
                self.hud_latched_widths.borrow_mut().clear();
                if let Some(gpu) = self.gpu.as_ref() {
                    gpu.stable_numbers.borrow_mut().clear();
                }
            }
            // Das Laden danach, unten -- es braucht ganz self.
            ButtonAction::CycleWallpaper => set.wallpaper = (set.wallpaper + 1) % wallpaper::WallpaperMode::ALL.len() as u8,
            ButtonAction::PanoramaSpeedPrev => set.panorama_speed = (set.panorama_speed - crate::panorama::SPEED_STEP).max(crate::panorama::SPEED_MIN),
            ButtonAction::PanoramaSpeedNext => set.panorama_speed = (set.panorama_speed + crate::panorama::SPEED_STEP).min(crate::panorama::SPEED_MAX),
            ButtonAction::CycleLanguage(row) => {
                let bereiche = [&mut set.lang_ui, &mut set.lang_items, &mut set.lang_effects, &mut set.lang_chat];
                if row == 0 {
                    // Gemeinsam weiter: von der Sprache der Oberflaeche aus.
                    let neu = i18n::Language::from_index(*bereiche[0]).next().index();
                    for b in bereiche {
                        *b = neu;
                    }
                } else if let Some(b) = bereiche.into_iter().nth(row - 1) {
                    *b = i18n::Language::from_index(*b).next().index();
                }
            }
            ButtonAction::PickWallpaper => {
                if let Some(ziel) = shared_config_dir().map(|d| wallpaper::stored_path(&d)) {
                    self.wallpaper_loader.pick(ziel);
                }
            }
            ButtonAction::ToggleModuleCritColor => set.module_crit_color_enabled = !set.module_crit_color_enabled,
            ButtonAction::ToggleDamageTintSkinOnly => set.damage_tint_skin_only = !set.damage_tint_skin_only,
            ButtonAction::DamageTintTicksPrev => set.damage_tint_ticks = set.damage_tint_ticks.saturating_sub(1).max(DAMAGE_TINT_TICKS_MIN),
            ButtonAction::DamageTintTicksNext => set.damage_tint_ticks = (set.damage_tint_ticks + 1).min(DAMAGE_TINT_TICKS_MAX),
            ButtonAction::MeshBudgetDown => set.mesh_columns_per_frame = set.mesh_columns_per_frame.saturating_sub(1).max(1),
            ButtonAction::MeshBudgetUp => set.mesh_columns_per_frame = (set.mesh_columns_per_frame + 1).min(MESH_COLUMNS_MAX),
            // Fernsicht im Video-Tab (s. lod.rs).
            ButtonAction::ChunkSaverThresholdPrev
            | ButtonAction::ChunkSaverThresholdNext
            | ButtonAction::ChunkSaverDistancePrev
            | ButtonAction::ChunkSaverDistanceNext => lod::apply_action(set, action),
            ButtonAction::MaxRenderDistanceDown => {
                set.max_render_distance = set.max_render_distance.saturating_sub(1).max(MAX_RENDER_DISTANCE_MIN);
            }
            ButtonAction::MaxRenderDistanceUp => set.max_render_distance = (set.max_render_distance + 1).min(MAX_RENDER_DISTANCE_MAX),
            ButtonAction::ChunkUpdateThreadsDown | ButtonAction::ChunkUpdateThreadsUp => {
                set.chunk_update_threads = if action == ButtonAction::ChunkUpdateThreadsDown {
                    set.chunk_update_threads.saturating_sub(1).max(CHUNK_UPDATE_THREADS_MIN)
                } else {
                    (set.chunk_update_threads + 1).min(CHUNK_UPDATE_THREADS_MAX)
                };
                self.mesh_pool.resize(set.chunk_update_threads);
            }
            // In 5er-Schritten -- feiner braucht es niemand, gezielt tippen geht ueber das
            // Wertfeld (s. EditableValue::InventoryTransparency).
            ButtonAction::MasterVolumeDown => set.master_volume = (set.master_volume - 5.0).max(0.0),
            ButtonAction::MasterVolumeUp => set.master_volume = (set.master_volume + 5.0).min(100.0),
            ButtonAction::CategoryVolumeDown(i) => set.category_volume[i] = (set.category_volume[i] - 5.0).max(0.0),
            ButtonAction::CategoryVolumeUp(i) => set.category_volume[i] = (set.category_volume[i] + 5.0).min(100.0),
            ButtonAction::UiSoundVolumeDown(i) => set.ui_sound_volume[i] = (set.ui_sound_volume[i] - 5.0).max(0.0),
            ButtonAction::UiSoundVolumeUp(i) => set.ui_sound_volume[i] = (set.ui_sound_volume[i] + 5.0).min(100.0),
            ButtonAction::InventoryTransparencyPrev => set.inventory_transparency = set.inventory_transparency.saturating_sub(5),
            ButtonAction::InventoryTransparencyNext => set.inventory_transparency = (set.inventory_transparency + 5).min(100),
            ButtonAction::GlintSpeedPrev => set.glint_speed = set.glint_speed.saturating_sub(5),
            ButtonAction::GlintSpeedNext => set.glint_speed = (set.glint_speed + 5).min(100),
            ButtonAction::GlintStrengthPrev => set.glint_strength = set.glint_strength.saturating_sub(5),
            ButtonAction::GlintStrengthNext => set.glint_strength = (set.glint_strength + 5).min(100),
            ButtonAction::PlayerListScalePrev => set.player_list_scale = set.player_list_scale.saturating_sub(HUD_SCALE_PCT_STEP).max(HUD_SCALE_PCT_MIN),
            ButtonAction::PlayerListScaleNext => set.player_list_scale = (set.player_list_scale + HUD_SCALE_PCT_STEP).min(HUD_SCALE_PCT_MAX),
            ButtonAction::MenuBackdropPrev => set.menu_backdrop_opacity = set.menu_backdrop_opacity.saturating_sub(5),
            ButtonAction::MenuBackdropNext => set.menu_backdrop_opacity = (set.menu_backdrop_opacity + 5).min(100),
            ButtonAction::ScrollInertiaPrev => set.scroll_inertia = set.scroll_inertia.saturating_sub(5),
            ButtonAction::ScrollInertiaNext => set.scroll_inertia = (set.scroll_inertia + 5).min(100),
            ButtonAction::ScrollDistancePrev => set.scroll_distance = set.scroll_distance.saturating_sub(SCROLL_DISTANCE_STEP).max(SCROLL_DISTANCE_MIN),
            ButtonAction::ScrollDistanceNext => set.scroll_distance = (set.scroll_distance + SCROLL_DISTANCE_STEP).min(SCROLL_DISTANCE_MAX),
            // Auf das Raster einrasten: ein eingetippter Wert wie 100 geht zu 96 bzw. 112.
            ButtonAction::EntityDistancePrev => {
                set.entity_render_distance =
                    ((set.entity_render_distance - 1) / ENTITY_DISTANCE_STEP * ENTITY_DISTANCE_STEP).max(ENTITY_DISTANCE_MIN);
            }
            ButtonAction::EntityDistanceNext => {
                set.entity_render_distance =
                    ((set.entity_render_distance / ENTITY_DISTANCE_STEP + 1) * ENTITY_DISTANCE_STEP).min(ENTITY_DISTANCE_MAX);
            }
            ButtonAction::BlockEntityDistancePrev => {
                set.block_entity_render_distance = ((set.block_entity_render_distance.max(1) - 1) / BLOCK_ENTITY_DISTANCE_STEP * BLOCK_ENTITY_DISTANCE_STEP)
                    .max(BLOCK_ENTITY_DISTANCE_MIN);
            }
            ButtonAction::BlockEntityDistanceNext => {
                set.block_entity_render_distance = ((set.block_entity_render_distance / BLOCK_ENTITY_DISTANCE_STEP + 1) * BLOCK_ENTITY_DISTANCE_STEP)
                    .min(BLOCK_ENTITY_DISTANCE_MAX);
            }
            ButtonAction::InventoryScalePrev => set.inventory_scale = set.inventory_scale.saturating_sub(1),
            ButtonAction::InventoryScaleNext => set.inventory_scale = (set.inventory_scale + 1).min(max_scale),
            ButtonAction::ToggleInventoryCursorToSlot => set.inventory_cursor_to_slot = !set.inventory_cursor_to_slot,
            ButtonAction::ToggleCreativeOpTab => set.creative_op_tab = !set.creative_op_tab,
            ButtonAction::ToggleServerClose(zeile) => set.server_close_blocked ^= 1 << zeile,
            ButtonAction::ServerOpen(i, m) => set.server_open[i] = m,
            ButtonAction::ToggleNoChatClear => set.no_chat_clear = !set.no_chat_clear,
            ButtonAction::ToggleAudioAutoOptimize => set.audio_auto_optimize = !set.audio_auto_optimize,
            ButtonAction::ToggleSignEditInWorld => set.sign_edit_in_world = !set.sign_edit_in_world,
            ButtonAction::NoSlotSet(m) => set.no_slot_set = m,
            ButtonAction::NoRotateSet(m) => set.no_rotate_set = m,
            ButtonAction::TogglePotionMarkStrong => set.potion_mark_strong = !set.potion_mark_strong,
            ButtonAction::TogglePotionMarkLong => set.potion_mark_long = !set.potion_mark_long,
            ButtonAction::TogglePotionMarkWeak => set.potion_mark_weak = !set.potion_mark_weak,
            ButtonAction::ToggleSwapAnimSkipTotems => set.swap_anim_skip_totems = !set.swap_anim_skip_totems,
            ButtonAction::ToggleSwapAnimSkipHealing => set.swap_anim_skip_healing = !set.swap_anim_skip_healing,
            ButtonAction::ToggleDollBackdropClear => set.doll_backdrop_clear = !set.doll_backdrop_clear,
            ButtonAction::HotbarScalePrev => set.hud_hotbar_scale = set.hud_hotbar_scale.saturating_sub(1),
            ButtonAction::HotbarScaleNext => set.hud_hotbar_scale = (set.hud_hotbar_scale + 1).min(max_scale),
            ButtonAction::HotbarDurabilityPrev if set.hotbar_durability_absolute => {
                let i = HOTBAR_DURABILITY_POINTS.iter().position(|&p| p == set.hotbar_durability_points).unwrap_or(0);
                set.hotbar_durability_points = HOTBAR_DURABILITY_POINTS[i.saturating_sub(1)];
            }
            ButtonAction::HotbarDurabilityNext if set.hotbar_durability_absolute => {
                let i = HOTBAR_DURABILITY_POINTS.iter().position(|&p| p == set.hotbar_durability_points).unwrap_or(0);
                set.hotbar_durability_points = HOTBAR_DURABILITY_POINTS[(i + 1).min(HOTBAR_DURABILITY_POINTS.len() - 1)];
            }
            ButtonAction::HotbarDurabilityPrev => {
                set.hotbar_durability_percent = set.hotbar_durability_percent.saturating_sub(HOTBAR_DURABILITY_STEP).max(HOTBAR_DURABILITY_STEP);
            }
            ButtonAction::HotbarDurabilityNext => set.hotbar_durability_percent = (set.hotbar_durability_percent + HOTBAR_DURABILITY_STEP).min(100),
            ButtonAction::ToggleHotbarDurabilityMode => set.hotbar_durability_absolute = !set.hotbar_durability_absolute,
            ButtonAction::ToggleHotbarAirBar => set.hotbar_air_bar = !set.hotbar_air_bar,
            ButtonAction::EffectsDetailed(an) => set.effects_detailed = an,
            ButtonAction::ToggleEffectsIcon => set.effects_icon = !set.effects_icon,
            ButtonAction::ToggleEffectsHideAmbient => set.effects_hide_ambient = !set.effects_hide_ambient,
            ButtonAction::ToggleEffectsName => set.effects_name = !set.effects_name,
            ButtonAction::EffectsTimerBlink(m) => set.effects_timer_blink = m,
            ButtonAction::EffectsNameColor(an) => set.effects_name_color = an,
            ButtonAction::EffectsTimerStyle(i) => set.effects_timer_style = i,
            ButtonAction::EffectsTimerColor(an) => set.effects_timer_color = an,
            ButtonAction::ChatScalePrev => set.chat_scale_percent = set.chat_scale_percent.saturating_sub(CHAT_SCALE_STEP).max(CHAT_SCALE_MIN),
            ButtonAction::ChatScaleNext => set.chat_scale_percent = (set.chat_scale_percent + CHAT_SCALE_STEP).min(CHAT_SCALE_MAX),
            ButtonAction::ChatWidthPrev => set.chat_width = set.chat_width.saturating_sub(CHAT_WIDTH_STEP).max(CHAT_WIDTH_MIN),
            ButtonAction::ChatWidthNext => set.chat_width = (set.chat_width + CHAT_WIDTH_STEP).min(CHAT_WIDTH_MAX),
            ButtonAction::ChatLinesPrev => set.chat_lines = set.chat_lines.saturating_sub(1).max(1),
            ButtonAction::ChatLinesNext => set.chat_lines = (set.chat_lines + 1).min(CHAT_LINES_MAX),
            ButtonAction::ChatLinesOpenPrev => set.chat_lines_open = set.chat_lines_open.saturating_sub(1).max(1),
            ButtonAction::ChatLinesOpenNext => set.chat_lines_open = (set.chat_lines_open + 1).min(CHAT_LINES_OPEN_MAX),
            ButtonAction::ChatBackgroundPrev => set.chat_background = set.chat_background.saturating_sub(10),
            ButtonAction::ChatBackgroundNext => set.chat_background = (set.chat_background + 10).min(100),
            ButtonAction::ChatOpenDimPrev => set.chat_open_dim = set.chat_open_dim.saturating_sub(10),
            ButtonAction::ChatOpenDimNext => set.chat_open_dim = (set.chat_open_dim + 10).min(100),
            ButtonAction::ChatFadePrev => set.chat_fade_secs = chat_fade_step(set.chat_fade_secs, -1),
            ButtonAction::ChatFadeNext => set.chat_fade_secs = chat_fade_step(set.chat_fade_secs, 1),
            ButtonAction::ChatLineSpacingPrev => set.chat_line_spacing = set.chat_line_spacing.saturating_sub(CHAT_LINE_SPACING_STEP),
            ButtonAction::ChatLineSpacingNext => set.chat_line_spacing = (set.chat_line_spacing + CHAT_LINE_SPACING_STEP).min(CHAT_LINE_SPACING_MAX),
            ButtonAction::ChatStyle(b) => set.chat_bedrock = b,
            ButtonAction::ToggleCrosshairOutline => set.crosshair_outline = !set.crosshair_outline,
            ButtonAction::ToggleCrosshairRingOutline => set.crosshair_ring_outline = !set.crosshair_ring_outline,
            ButtonAction::CrosshairRingRadiusPrev => set.crosshair_ring_radius = set.crosshair_ring_radius.saturating_sub(1).max(SHIELD_RING_RADIUS_MIN_PX),
            ButtonAction::CrosshairRingRadiusNext => set.crosshair_ring_radius = (set.crosshair_ring_radius + 1).min(SHIELD_RING_RADIUS_MAX_PX),
            ButtonAction::CrosshairSizePrev => set.step_crosshair_size(-1),
            ButtonAction::CrosshairSizeNext => set.step_crosshair_size(1),
            ButtonAction::OpenCrosshairPainter => {
                set.use_custom_crosshair();
                self.crosshair_painter = Some(CrosshairPainter::default());
            }
            ButtonAction::CrosshairPainterDone => self.crosshair_painter = None,
            ButtonAction::CrosshairPainterClear => set.crosshair_pattern = crosshair_editor::Pattern::empty(),
            ButtonAction::CrosshairPainterStandard => set.crosshair_pattern = crosshair_editor::Pattern::standard(set.crosshair_outline),
            ButtonAction::CrosshairPainterSymmetry => {
                if let Some(p) = self.crosshair_painter.as_mut() {
                    p.symmetry = p.symmetry.next();
                }
            }
            ButtonAction::ToggleEffectsHud => set.show_effects_hud = !set.show_effects_hud,
            ButtonAction::OpenParticleSettings(own) => {
                self.screen = MenuScreen::SettingsParticles;
                self.particle_screen_own = own;
                crate::PARTICLE_SCREEN_OWN.store(own, std::sync::atomic::Ordering::Relaxed);
                self.particle_scroll = Default::default();
            }
            ButtonAction::OpenAnimSettings => {
                self.screen = MenuScreen::SettingsAnimations;
                *self.page_scroll_mut() = Default::default();
            }
            ButtonAction::OpenMotionBlurSettings => {
                self.screen = MenuScreen::SettingsMotionBlur;
                *self.page_scroll_mut() = Default::default();
            }
            ButtonAction::OpenServerScreens => {
                self.screen = MenuScreen::SettingsServerScreens;
                *self.page_scroll_mut() = Default::default();
            }
            ButtonAction::ParticleToggle(own, i) => {
                if let Some(t) = set.particle_tuning_of_mut(own).get_mut(i) {
                    t.enabled = !t.enabled;
                }
            }
            ButtonAction::ParticleAllAtOnce(own, i) => {
                if let Some(t) = set.particle_tuning_of_mut(own).get_mut(i) {
                    t.all_at_once = !t.all_at_once;
                }
            }
            ButtonAction::ParticleStep(own, i, col, hoch) => {
                if let Some(t) = set.particle_tuning_of_mut(own).get_mut(i) {
                    match col {
                        0 => t.lifetime_pct = step_in(&PARTICLE_LIFETIME_STEPS, t.lifetime_pct, hoch),
                        1 => {
                            t.opacity_pct =
                                if hoch { (t.opacity_pct + PARTICLE_OPACITY_STEP).min(100) } else { t.opacity_pct.saturating_sub(PARTICLE_OPACITY_STEP).max(PARTICLE_OPACITY_STEP) }
                        }
                        2 => t.speed_pct = step_in(&PARTICLE_SPEED_STEPS, t.speed_pct, hoch),
                        3 => t.size_pct = step_in(&PARTICLE_SIZE_STEPS, t.size_pct, hoch),
                        4 => t.spread_pct = step_in(&PARTICLE_SPREAD_STEPS, t.spread_pct, hoch),
                        // Der letzte Regler ist Menge oder Intervall, je nach Art (s. particle_slider_rows).
                        _ if particles::KINDS[i].spawn.has_interval() => t.interval_pct = step_in(&PARTICLE_INTERVAL_STEPS, t.interval_pct, hoch),
                        _ => t.amount_pct = step_in(&PARTICLE_AMOUNT_STEPS, t.amount_pct, hoch),
                    }
                }
            }
            ButtonAction::ToggleCaveCulling => set.cave_culling = !set.cave_culling,
            ButtonAction::SetEntityCulling(m) => (set.entity_culling, set.entity_ray_culling) = (m >= 1, m >= 2),
            ButtonAction::ToggleMipmapping => set.mipmapping = !set.mipmapping,
            ButtonAction::ToggleLatinMacrons => set.latin_macrons = !set.latin_macrons,
            ButtonAction::ToggleLoanwords => set.english_loanwords = !set.english_loanwords,
            ButtonAction::ToggleEffectsInventory => set.show_effects_inventory = !set.show_effects_inventory,
            ButtonAction::CrosshairBoostPrev => set.crosshair_brightness_boost = set.crosshair_brightness_boost.saturating_sub(5),
            ButtonAction::CrosshairBoostNext => set.crosshair_brightness_boost = (set.crosshair_brightness_boost + 5).min(100),
            ButtonAction::CrosshairColorPrev => set.crosshair_color_mode = set.crosshair_color_mode.saturating_sub(1),
            ButtonAction::CrosshairColorNext => {
                set.crosshair_color_mode = (set.crosshair_color_mode + 1).min(CROSSHAIR_COLOR_MODES.len() - 1);
            }
            ButtonAction::AudioTestTone => self.audio.play_test_tone(),
            ButtonAction::PipewireLimit(frames) => {
                let rate = self.pipewire.messung().map_or(48_000, |m| m.rate);
                self.pipewire.grenze_setzen(frames, rate);
            }
            ButtonAction::AudioSpatialTest => {
                // Uhr mit zuruecksetzen, sonst zaehlt die Zeit seit dem letzten Frame als
                // schon vergangene Testzeit und der erste Klick kaeme verschoben.
                self.last_orbit_update = Instant::now();
                self.orbit_test = Some(spatial_sound::OrbitTest::start());
            }
            ButtonAction::HudAlign(element, align) => {
                // Die Ausrichtung wechselt, das Element soll aber stehen bleiben: der Anker
                // wird aus der bisherigen linken Kante neu bestimmt.
                if let Some(gpu) = self.gpu.as_ref() {
                    let r = self.hud_element_rect(gpu, element);
                    let sw = gpu.size().0;
                    let anker = (align_anchor(r.x, r.w, align) / sw).clamp(0.0, 1.0);
                    match element {
                        HudElement::Title => {
                            self.settings.hud_title_align = align;
                            self.settings.hud_title_x = anker;
                        }
                        HudElement::Subtitle => {
                            self.settings.hud_subtitle_align = align;
                            self.settings.hud_subtitle_x = anker;
                        }
                        HudElement::ActionBar => {
                            self.settings.hud_actionbar_align = align;
                            self.settings.hud_actionbar_x = anker;
                        }
                        _ => {}
                    }
                }
            }
            ButtonAction::EffectsAlign(align) => {
                // Wie bei HudAlign bleibt die Anzeige stehen: die Anker kommen aus dem
                // bisherigen Rechteck, nur der feste Rand wechselt.
                if let Some(gpu) = self.gpu.as_ref() {
                    let r = self.hud_effects_rect(gpu, true);
                    let (sw, sh) = gpu.size();
                    let (x, y) = effects_detail_anchor(r, align);
                    self.settings.effects_align = align;
                    self.settings.hud_effects_detail_x = (x / sw).clamp(0.0, 1.0);
                    self.settings.hud_effects_detail_y = (y / sh).clamp(0.0, 1.0);
                }
            }
            ButtonAction::ScoreboardAlign(align) => {
                // Die Leiste bleibt stehen, nur die feste Ecke wechselt (wie bei EffectsAlign).
                if let Some(gpu) = self.gpu.as_ref() {
                    let r = self.hud_scoreboard_rect(gpu);
                    let (sw, sh) = gpu.size();
                    let (x, y) = scoreboard_anchor(r, align);
                    self.settings.scoreboard_align = align;
                    self.settings.hud_scoreboard_x = (x / sw).clamp(0.0, 1.0);
                    self.settings.hud_scoreboard_y = (y / sh).clamp(0.0, 1.0);
                }
            }
            ButtonAction::HudDeselect => self.hud_selected = None,
            ButtonAction::ArmorHudLayout(l) => self.settings.armor_hud_layout = l,
            ButtonAction::ArmorHudGapPrev => self.settings.armor_hud_gap = (self.settings.armor_hud_gap - 1).max(armor_hud::GAP_MIN),
            ButtonAction::ArmorHudGapNext => self.settings.armor_hud_gap = (self.settings.armor_hud_gap + 1).min(armor_hud::GAP_MAX),
            ButtonAction::ArmorHudNumberScalePrev | ButtonAction::ArmorHudNumberScaleNext => {
                let wert = &mut self.settings.armor_hud_number_scale;
                *wert = if action == ButtonAction::ArmorHudNumberScaleNext { *wert + armor_hud::NUMBER_SCALE_STEP } else { wert.saturating_sub(armor_hud::NUMBER_SCALE_STEP) }
                    .clamp(armor_hud::NUMBER_SCALE_MIN, armor_hud::NUMBER_SCALE_MAX);
            }
            ButtonAction::ArmorHudNumberXPrev | ButtonAction::ArmorHudNumberXNext | ButtonAction::ArmorHudNumberYPrev | ButtonAction::ArmorHudNumberYNext => {
                let set = &mut self.settings;
                let (wert, schritt) = match action {
                    ButtonAction::ArmorHudNumberXPrev => (&mut set.armor_hud_number_x, -1),
                    ButtonAction::ArmorHudNumberXNext => (&mut set.armor_hud_number_x, 1),
                    ButtonAction::ArmorHudNumberYPrev => (&mut set.armor_hud_number_y, -1),
                    _ => (&mut set.armor_hud_number_y, 1),
                };
                *wert = (*wert + schritt).clamp(armor_hud::NUMBER_OFFSET_MIN, armor_hud::NUMBER_OFFSET_MAX);
            }
            ButtonAction::ToggleHurtCam => self.settings.hurt_cam = !self.settings.hurt_cam,
            ButtonAction::ToggleShoulderCam => self.settings.shoulder_cam = !self.settings.shoulder_cam,
            ButtonAction::ToggleShoulderCrosshair => self.settings.shoulder_crosshair = !self.settings.shoulder_crosshair,
            ButtonAction::ToggleShoulderFade => self.settings.shoulder_fade = !self.settings.shoulder_fade,
            ButtonAction::ToggleShoulderDecoupled => self.settings.shoulder_decoupled = !self.settings.shoulder_decoupled,
            ButtonAction::ShoulderOffsetPrev => self.settings.shoulder_offset_cm = (self.settings.shoulder_offset_cm - SHOULDER_OFFSET_STEP).max(SHOULDER_OFFSET_MIN),
            ButtonAction::ShoulderOffsetNext => self.settings.shoulder_offset_cm = (self.settings.shoulder_offset_cm + SHOULDER_OFFSET_STEP).min(SHOULDER_OFFSET_MAX),
            ButtonAction::ToggleArmorTrims => self.settings.armor_trims = !self.settings.armor_trims,
            ButtonAction::ToggleRenderOrder(zeile, spalte) => self.settings.render_order.toggle(usize::from(zeile), usize::from(spalte)),
            ButtonAction::ToggleViewModel => self.settings.modified_view_model = !self.settings.modified_view_model,
            ButtonAction::ToggleViewModelBobbing => self.settings.view_model_bobbing = !self.settings.view_model_bobbing,
            ButtonAction::ToggleAnimFlag(bit) => self.settings.anim_flags ^= bit,
            ButtonAction::ToggleAnims => self.settings.anims_enabled = !self.settings.anims_enabled,
            ButtonAction::HurtCamStrengthPrev => self.settings.hurt_cam_strength = self.settings.hurt_cam_strength.saturating_sub(5),
            ButtonAction::HurtCamStrengthNext => self.settings.hurt_cam_strength = (self.settings.hurt_cam_strength + 5).min(100),
            ButtonAction::FireMode(m) => self.settings.fire_mode = m,
            ButtonAction::FireHeightPrev => self.settings.fire_height = self.settings.fire_height.saturating_sub(5),
            ButtonAction::FireHeightNext => self.settings.fire_height = (self.settings.fire_height + 5).min(100),
            ButtonAction::KeysOpacityPrev => self.settings.keys_opacity = self.settings.keys_opacity.saturating_sub(10),
            ButtonAction::KeysOpacityNext => self.settings.keys_opacity = (self.settings.keys_opacity + 10).min(100),
            ButtonAction::SessionOpacity(mehr) => {
                let o = &mut self.settings.session_opacity;
                *o = if mehr { (*o + 10).min(100) } else { o.saturating_sub(10) };
            }
            ButtonAction::ArmorHudSort(r) => {
                if armor_hud::is_vertical(self.settings.armor_hud_layout) {
                    self.settings.armor_hud_sort_vertical = r;
                } else {
                    self.settings.armor_hud_sort_flat = r;
                }
            }
            ButtonAction::ToggleArmorHudGreen => self.settings.armor_hud_green = !self.settings.armor_hud_green,
            ButtonAction::ToggleArmorHudRing => self.settings.armor_hud_ring = !self.settings.armor_hud_ring,
            // Aussehen von Schild-Cooldown und Pfeil-Anzeige: im Fenster des HUD-Editors, die
            // Werte gehoeren aber zu den Modulen.
            ButtonAction::ToggleArrowNumberOnly
            | ButtonAction::ArrowOpacityPrev
            | ButtonAction::ArrowOpacityNext
            | ButtonAction::CycleArrowTextStyle
            | ButtonAction::CycleShieldCdTextStyle
            | ButtonAction::CycleShieldSelfMode
            | ButtonAction::OpenHudConfig(_) => self.apply_module_action(action),
            ButtonAction::HudScaleStep(element, groesser) => {
                let wert = match element {
                    HudElement::Title => &mut self.settings.hud_title_scale,
                    HudElement::Subtitle => &mut self.settings.hud_subtitle_scale,
                    HudElement::Effects => &mut self.settings.hud_effects_scale,
                    HudElement::BossBar => &mut self.settings.hud_bossbar_scale,
                    HudElement::Scoreboard => &mut self.settings.hud_scoreboard_scale,
                    HudElement::Arrows => &mut self.settings.hud_arrows_scale,
                    HudElement::HeldName => &mut self.settings.hud_held_name_scale,
                    HudElement::Armor => &mut self.settings.hud_armor_scale,
                    HudElement::Keystrokes => &mut self.settings.hud_keys_scale,
                    HudElement::Session => &mut self.settings.hud_session_scale,
                    HudElement::Clock => &mut self.settings.hud_clock_scale,
                    _ => return,
                };
                *wert = if groesser { *wert + HUD_SCALE_PCT_STEP } else { wert.saturating_sub(HUD_SCALE_PCT_STEP) }
                    .clamp(HUD_SCALE_PCT_MIN, HUD_SCALE_PCT_MAX);
            }
            ButtonAction::ResetHudElement => {
                if let Some(element) = self.hud_selected {
                    self.reset_hud_element(element);
                }
            }
            // Expliziter, vom Nutzer gedrueckter Schalter -- keine Automatik, die im
            // Hintergrund an einer globalen Anzeigeeinstellung dreht (s. latency_tuning.rs).
            ButtonAction::ToggleTearFree => {
                let want_on = !self.tearfree_state.unwrap_or(false);
                match latency_tuning::set_tearfree(want_on) {
                    // Real abfragen statt den gewuenschten Wert einfach zu uebernehmen --
                    // xrandr kann trotz Erfolgsmeldung etwas anderes gesetzt haben, als wir
                    // dachten (s. die ganze TearFree-Untersuchung diese Sitzung).
                    Ok(()) => {
                        self.tearfree_state = latency_tuning::tearfree_enabled();
                        // Merken, was der Nutzer will, solange der Client laeuft -- das ist
                        // getrennt vom Systemwert, der bei Fokusverlust/Beenden wieder auf
                        // tearfree_original_value zurueckspringt.
                        self.tearfree_user_choice = self.tearfree_state;
                    }
                    Err(e) => self.push_log(format!("TearFree konnte nicht gesetzt werden: {e}")),
                }
            }
            ButtonAction::ToggleBlockRngRotation => set.block_rng_rotation = !set.block_rng_rotation,
            ButtonAction::SmoothLightingDown => set.smooth_lighting_percent = set.smooth_lighting_percent.saturating_sub(SMOOTH_LIGHTING_STEP),
            ButtonAction::SmoothLightingUp => {
                set.smooth_lighting_percent = (set.smooth_lighting_percent + SMOOTH_LIGHTING_STEP).min(SMOOTH_LIGHTING_MAX);
            }
            ButtonAction::ParticleDistanceDown => set.particle_distance = set.particle_distance.saturating_sub(PARTICLE_DISTANCE_STEP).max(PARTICLE_DISTANCE_MIN),
            ButtonAction::ParticleDistanceUp => set.particle_distance = (set.particle_distance + PARTICLE_DISTANCE_STEP).min(PARTICLE_DISTANCE_MAX),
            ButtonAction::ParticleFadeDown => set.particle_fade_out_ms = set.particle_fade_out_ms.saturating_sub(PARTICLE_FADE_STEP_MS),
            ButtonAction::ParticleFadeUp => set.particle_fade_out_ms = (set.particle_fade_out_ms + PARTICLE_FADE_STEP_MS).min(PARTICLE_FADE_MAX_MS),
            ButtonAction::EntityNearDown => set.entity_near_dither_cm = set.entity_near_dither_cm.saturating_sub(ENTITY_NEAR_STEP_CM),
            ButtonAction::MipSharpnessDown => set.mip_sharpness_pct = set.mip_sharpness_pct.saturating_sub(MIP_SHARPNESS_STEP),
            ButtonAction::MipSharpnessUp => set.mip_sharpness_pct = (set.mip_sharpness_pct + MIP_SHARPNESS_STEP).min(MIP_SHARPNESS_MAX),
            ButtonAction::EntityNearUp => set.entity_near_dither_cm = (set.entity_near_dither_cm + ENTITY_NEAR_STEP_CM).min(ENTITY_NEAR_MAX_CM),
            ButtonAction::ChunkFadeDown => set.chunk_fade_ms = set.chunk_fade_ms.saturating_sub(CHUNK_FADE_STEP_MS),
            ButtonAction::ChunkFadeUp => set.chunk_fade_ms = (set.chunk_fade_ms + CHUNK_FADE_STEP_MS).min(CHUNK_FADE_MAX_MS),
            ButtonAction::SlotHoverFadeDown => set.slot_hover_fade_ms = set.slot_hover_fade_ms.saturating_sub(SLOT_HOVER_FADE_STEP),
            ButtonAction::SlotHoverFadeUp => set.slot_hover_fade_ms = (set.slot_hover_fade_ms + SLOT_HOVER_FADE_STEP).min(SLOT_HOVER_FADE_MAX),
            ButtonAction::SwapAnimDown => set.swap_anim_ms = set.swap_anim_ms.saturating_sub(SWAP_ANIM_STEP),
            ButtonAction::SwapAnimUp => set.swap_anim_ms = (set.swap_anim_ms + SWAP_ANIM_STEP).min(SWAP_ANIM_MAX),
            ButtonAction::HotbarGlideDown => set.hotbar_glide_ms = set.hotbar_glide_ms.saturating_sub(HOTBAR_GLIDE_STEP),
            ButtonAction::HotbarGlideUp => set.hotbar_glide_ms = (set.hotbar_glide_ms + HOTBAR_GLIDE_STEP).min(HOTBAR_GLIDE_MAX),
            ButtonAction::MotionBlurDown => set.motion_blur_percent = set.motion_blur_percent.saturating_sub(MOTION_BLUR_STEP),
            ButtonAction::MotionBlurUp => set.motion_blur_percent = (set.motion_blur_percent + MOTION_BLUR_STEP).min(MOTION_BLUR_MAX),
            ButtonAction::EntityBlurDown => set.motion_blur_entities_percent = set.motion_blur_entities_percent.saturating_sub(MOTION_BLUR_STEP),
            ButtonAction::EntityBlurUp => set.motion_blur_entities_percent = (set.motion_blur_entities_percent + MOTION_BLUR_STEP).min(MOTION_BLUR_MAX),
            ButtonAction::HandBlurDown => set.hand_blur_percent = set.hand_blur_percent.saturating_sub(HAND_BLUR_STEP),
            ButtonAction::HandBlurUp => set.hand_blur_percent = (set.hand_blur_percent + HAND_BLUR_STEP).min(HAND_BLUR_MAX),
            ButtonAction::HandBlurCurvePrev => set.hand_blur_curve = set.hand_blur_curve.saturating_sub(1),
            ButtonAction::HandBlurCurveNext => set.hand_blur_curve = (set.hand_blur_curve + 1).min(HAND_BLUR_CURVES.len() as u8 - 1),
            ButtonAction::ToggleJumpCooldown => set.jump_cooldown_enabled = !set.jump_cooldown_enabled,
            ButtonAction::ToggleSprintOnStart => set.sprint_on_start = !set.sprint_on_start,
            ButtonAction::ToggleCapsLockBlocked => set.caps_lock_blocked = !set.caps_lock_blocked,
            ButtonAction::ToggleSnapTap => set.snap_tap_enabled = !set.snap_tap_enabled,
            ButtonAction::ToggleUseWhileMining => set.use_while_mining = !set.use_while_mining,
            ButtonAction::ToggleRocketRepeat => set.hold_use_repeat_rockets = !set.hold_use_repeat_rockets,
            ButtonAction::ToggleExtinguishDcGhost => set.no_extinguish_dc_ghost = !set.no_extinguish_dc_ghost,
            ButtonAction::CycleHoldUseRepeat => set.hold_use_repeat = set.hold_use_repeat.next(),
            ButtonAction::ToggleSneakEdgeProtection => set.sneak_edge_protection_enabled = !set.sneak_edge_protection_enabled,
            // Wartet auf den naechsten Tastendruck -- ausgewertet in window_event(), nicht
            // hier: ein Mausklick ist kein Tastendruck, wir muessen den ERST NOCH abwarten.
            ButtonAction::Rebind(target, slot) => {
                self.awaiting_keybind = Some((target, slot));
                self.awaiting_modifier = false;
            }
            ButtonAction::RebindModifier(target, slot) => {
                self.awaiting_keybind = Some((target, slot));
                self.awaiting_modifier = true;
            }
            ButtonAction::OpenKeybindAdvanced(aktion) => {
                self.awaiting_keybind = None;
                self.keybind_advanced = Some(aktion);
            }
            ButtonAction::CloseKeybindAdvanced => {
                self.awaiting_keybind = None;
                self.keybind_advanced = None;
            }
            ButtonAction::ToggleKeybindMode(action) => {
                let mode = set.keybind_modes.get(action).toggled();
                set.keybind_modes.set(action, mode);
            }
            // Respawn anfordern. Optimistisch zurueck in die Welt -- der Server bestaetigt
            // mit neuem Leben (Set Health) + Position; kommt es wider Erwarten nicht,
            // bliebe man tot und koennte erneut klicken.
            ButtonAction::Respawn => {
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::perform_respawn_payload());
                }
                self.is_dead = false;
                self.mode = AppMode::World;
                self.screen = MenuScreen::Main;
                self.restore_held_controls();
            }
            // Tatsaechlich behandelt NACH dem match (braucht &mut self).
            ButtonAction::OpenSkinFolder | ButtonAction::ReloadSkins | ButtonAction::OpenConfigFolder | ButtonAction::ServerLinks | ButtonAction::PauseDialog => {}
            ButtonAction::ToggleSkinAutoUnzip => set.skin_auto_unzip = !set.skin_auto_unzip,
        }

        if action == ButtonAction::OpenSkinFolder {
            self.open_skins_dir();
        }
        if action == ButtonAction::ReloadSkins {
            self.reload_resource_pack(self.selected_skins.clone());
        }
        if action == ButtonAction::OpenConfigFolder {
            self.open_configs_dir();
        }
        if action == ButtonAction::ServerLinks {
            self.open_server_links();
        }
        if action == ButtonAction::PauseDialog {
            if let Some((id, _)) = self.pause_dialog() {
                self.show_dialog_ref(protocol::packets::DialogRef::Registry(id as i32));
            }
        }
        if action == ButtonAction::CycleWallpaper {
            self.request_wallpaper(true);
        }
        if matches!(action, ButtonAction::CycleLanguage(_) | ButtonAction::ToggleLatinMacrons | ButtonAction::ToggleLoanwords) {
            apply_language_settings(&mut self.installation, &self.settings);
        }

        if changes_settings {
            self.persist_settings();
        }

        // Block-Rotation wirkt sich nur beim (Neu-)Meshen aus (s. world::mesher) -- ohne
        // erzwungenen Neubau blieben schon geladene Spalten bis zum naechsten echten
        // Blockupdate/Neuladen bei der alten Ausrichtung.
        if action == ButtonAction::ToggleBlockRngRotation {
            self.block_uvs.set_rng_rotation_enabled(self.settings.block_rng_rotation);
            for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                self.world.mark_dirty(cx, cz);
            }
        }
        // Weiche Beleuchtung steckt wie die Block-Rotation in den Vertexfarben -- ohne
        // Neubau bliebe die schon geladene Welt bis zum naechsten Blockupdate beim alten Stand.
        if matches!(action, ButtonAction::SmoothLightingDown | ButtonAction::SmoothLightingUp) {
            for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                self.world.mark_dirty(cx, cz);
            }
        }
    }
}
