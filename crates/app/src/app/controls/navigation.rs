// Escape, Zurueck und Loslassen aller Steuerungen.

use super::*;

impl App {
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
        // Token-Export auskommentiert, vorerst abgeschaltet. Dann wieder: || self.token_export.take().is_some()
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
                self.pause_bar_slide = Some(Instant::now());
                self.clear_controls();
                // Der Zeiger erscheint immer in der Bildschirmmitte, nicht dort, wo er beim
                // letzten Menue stand.
                if let Some((w, h)) = self.gpu.as_ref().map(super::super::super::GpuState::size) {
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
                | MenuScreen::SettingsParticles
                | MenuScreen::SettingsAnimations
                | MenuScreen::SettingsMotionBlur
                | MenuScreen::SettingsServerScreens
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
                | MenuScreen::SettingsSetup
                | MenuScreen::SettingsSearch
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
            | MenuScreen::SettingsSearch
            | MenuScreen::Settings => self.settings_return,
            MenuScreen::SettingsSensCalc => MenuScreen::SettingsControls,
            MenuScreen::SettingsMotionBlur => MenuScreen::SettingsGeneral,
            MenuScreen::SettingsParticles | MenuScreen::SettingsAnimations => MenuScreen::SettingsVideo,
            MenuScreen::SettingsServerScreens => MenuScreen::SettingsMisc,
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
}
