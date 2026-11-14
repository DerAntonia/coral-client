// Textbereiche, Zeiger, Seitenleiste, Leiste oben, Inhalts- und Abschnitts-Buttons.

use super::*;

impl App {
    // Anfang und Ende eines Textbereichs mit eigenem Rand und eigener Schrift (s.
    // Settings::area_shadow/area_font): Marker fuer shape_text, und bis end_text_area misst
    // GpuState mit der Schrift des Bereichs.
    pub(crate) fn begin_text_area(&self, gpu: &GpuState, texts: &mut Vec<UiText>, area: ShadowArea) {
        push_shadow_marker(texts, self.settings.shadow_marker_of(area));
        let font = self.settings.font_of(area);
        push_font_marker(texts, font);
        gpu.set_layout_font(font);
        if area.plain_numbers() {
            push_plain_numbers_marker(texts, true);
            gpu.plain_numbers.set(true);
        }
    }

    pub(crate) fn end_text_area(gpu: &GpuState, texts: &mut Vec<UiText>) {
        push_shadow_marker(texts, None);
        push_font_marker(texts, None);
        gpu.set_layout_font(None);
        push_plain_numbers_marker(texts, false);
        gpu.plain_numbers.set(false);
    }

    // Fuer Rechnungen ausserhalb des Aufbaus (Klicks, Auswahl, Blaettern): bis der Waechter
    // faellt, misst GpuState mit der Schrift des Bereichs -- dieselben Breiten wie beim Zeichnen.
    pub(crate) fn area_font_scope<'a>(&self, gpu: &'a GpuState, area: ShadowArea) -> LayoutFontScope<'a> {
        let (vorher, ruhig_vorher) = (gpu.layout_font.get(), gpu.plain_numbers.get());
        gpu.set_layout_font(self.settings.font_of(area));
        gpu.plain_numbers.set(area.plain_numbers());
        LayoutFontScope { gpu, vorher, ruhig_vorher }
    }

    // Welcher klickbare Knopf liegt gerade unter dem Zeiger? Nur dort, wo es auch einen
    // gezeichneten Zeiger gibt -- im Weltmodus ohne Overlay steuert die Maus den Blick, ein
    // Hover-Klang waere dort sinnlos.
    // Zeiger im Koordinatenraum des Inhalts: auf den Einstellungsseiten ist der Inhalt um
    // content_offset verschoben (Leiste oben, Kategorien links), seine Rechtecke stehen aber
    // weiter in den alten Koordinaten. Ueberall sonst gleich cursor_pos.
    pub(crate) fn pointer(&self) -> (f32, f32) {
        // Offene Kontenliste liegt ueber dem Inhalt: darunter zeigt nichts auf etwas.
        if self.pointer_blocked() {
            return (f32::NEG_INFINITY, f32::NEG_INFINITY);
        }
        let (dx, dy) = self.content_offset();
        (self.cursor_pos.0 - dx, self.cursor_pos.1 - dy)
    }

    // Liegt etwas ueber dem Inhalt, das den Zeiger fuer sich hat: die offene Kontenliste, oder
    // der Zeiger steht auf der offenen Benachrichtigungsliste.
    pub(crate) fn pointer_blocked(&self) -> bool {
        self.account_menu_open || self.notification_panel_rect().is_some_and(|r| self.notification_panel_open && r.contains(self.cursor_pos.0, self.cursor_pos.1))
    }

    pub(crate) fn content_offset(&self) -> (f32, f32) {
        if self.mode != AppMode::Menu {
            (0.0, 0.0)
        } else if self.screen.settings_category().is_some() {
            (SETTINGS_CONTENT_DX, SETTINGS_CONTENT_DY)
        } else if self.screen == MenuScreen::Account {
            // Der Zurueck-Knopf sass frueher oben links, wo jetzt die Leiste liegt.
            (0.0, SETTINGS_CONTENT_DY)
        } else if let Some(panel) = self.hud_config_panel() {
            // Einstellungsfenster im HUD-Editor: in die Bildmitte.
            let (sw, sh) = self.gpu.as_ref().map_or((1920.0, 1080.0), super::super::super::GpuState::size);
            (((sw - panel.w) / 2.0 - panel.x).round(), ((sh - panel.h) / 2.0 - panel.y).round())
        } else {
            (0.0, 0.0)
        }
    }

    // Das offene Einstellungsfenster des HUD-Editors in Inhaltskoordinaten, None = keins.
    pub(crate) fn hud_config_panel(&self) -> Option<Rect> {
        let element = self.hud_selected.filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::HudEditor && self.crosshair_painter.is_none())?;
        Some(hud_config_panel(element, &self.settings))
    }

    // Liegt der Bildschirmpunkt im offenen Einstellungsfenster?
    pub(crate) fn hud_config_panel_contains(&self, (x, y): (f32, f32)) -> bool {
        let Some(panel) = self.hud_config_panel() else { return false };
        let (dx, dy) = self.content_offset();
        Rect { x: panel.x + dx, y: panel.y + dy, ..panel }.contains(x, y)
    }

    // Leiste oben: auf allen Menue-Screens ausser dort, wo der ganze Bildschirm gebraucht wird.
    pub(crate) fn chrome_visible(&self) -> bool {
        self.mode == AppMode::Menu && !matches!(self.screen, MenuScreen::HudEditor | MenuScreen::Death)
    }

    // Scrollstaende, die beim Verlassen erhalten bleiben (s. Settings::settings_scrolls).
    pub(crate) const fn scroll_slots(&mut self) -> [(&'static str, &mut smooth_scroll::SmoothScroll); 6] {
        let [links, rechts] = &mut self.skin_scroll;
        [
            ("render", &mut self.settings_scroll),
            ("controls", &mut self.keybind_scroll),
            ("skin", links),
            ("skin_right", rechts),
            ("modules", &mut self.module_menu_scroll),
            ("particles", &mut self.particle_scroll),
        ]
    }

    // Einstellungen oeffnen: MenuScreen::Settings heisst "die zuletzt offene Kategorie",
    // beim allerersten Mal die Einrichtung. Gemerkt wird die Kategorie sofort.
    pub(crate) fn open_settings(&mut self, target: MenuScreen) {
        if !self.scrolls_loaded {
            let gespeichert = self.settings.settings_scrolls.clone();
            for (name, feld) in self.scroll_slots() {
                if let Some(&v) = gespeichert.get(name) {
                    feld.jump(v as f32);
                }
            }
            self.scrolls_loaded = true;
        }
        let target = if target == MenuScreen::Settings {
            MenuScreen::from_settings_key(&self.settings.settings_category).unwrap_or(MenuScreen::SettingsSetup)
        } else {
            target
        };
        if self.screen.settings_category().is_none() && self.screen != MenuScreen::Settings {
            self.settings_return = self.screen;
        }
        self.screen = target;
        if let Some(kategorie) = target.settings_category() {
            if self.settings.settings_category != kategorie.settings_key() {
                self.settings.settings_category = kategorie.settings_key().to_string();
            }
        }
        self.persist_settings();
    }

    // Die Kategorien in der Leiste: alphabetisch nach angezeigtem Namen, die Einrichtung
    // ausserhalb davon ganz unten (s. sidebar_item_rect).
    pub(crate) fn sidebar_categories() -> Vec<(MenuScreen, &'static str)> {
        // Dazu der HUD-Editor: keine Einstellungsseite (er braucht den ganzen Bildschirm),
        // aber von hier aus zu oeffnen; Zurueck fuehrt wieder hierher.
        let mut kategorien = SETTINGS_CATEGORIES.to_vec();
        kategorien.push((MenuScreen::HudEditor, "HUD"));
        kategorien.sort_by_key(|&(_, label)| i18n::tr(label).to_lowercase());
        kategorien
    }

    pub(crate) fn sidebar_item_rect(i: usize, screen_h: f32) -> Rect {
        Rect { x: SIDEBAR_PAD, y: TOPBAR_H + 16.0 + i as f32 * (SIDEBAR_ITEM_H + 2.0), w: SIDEBAR_W - 2.0 * SIDEBAR_PAD, h: SIDEBAR_ITEM_H }
            .clamp_bottom(screen_h)
    }

    // Trefferflaeche eines Eintrags: die ganze Breite der Leiste und je die halbe Luecke zu den
    // Nachbarn, damit ein Klick zwischen zwei Eintraegen nicht ins Leere geht.
    pub(crate) fn sidebar_hit_rect(r: Rect) -> Rect {
        Rect { x: 0.0, y: r.y - 1.0, w: SIDEBAR_W, h: r.h + 2.0 }
    }

    pub(crate) fn sidebar_setup_rect(screen_h: f32) -> Rect {
        let suche = Self::sidebar_search_rect(screen_h);
        Rect { y: suche.y - SIDEBAR_ITEM_H - 2.0, ..suche }
    }

    // Suche: ganz unten, unter Setup.
    pub(crate) fn sidebar_search_rect(screen_h: f32) -> Rect {
        Rect { x: SIDEBAR_PAD, y: screen_h - SIDEBAR_PAD - SIDEBAR_ITEM_H, w: SIDEBAR_W - 2.0 * SIDEBAR_PAD, h: SIDEBAR_ITEM_H }
    }

    // Logo `i` aus SIDEBAR_LINKS: quadratisch, nebeneinander direkt ueber der Linie vor
    // "Setup"; die Version steht rechts daneben.
    pub(crate) fn sidebar_link_rect(i: usize, screen_h: f32) -> Rect {
        let setup = Self::sidebar_setup_rect(screen_h);
        Rect { x: SIDEBAR_PAD + i as f32 * (SIDEBAR_ITEM_H + 4.0), y: setup.y - 18.0 - SIDEBAR_ITEM_H, w: SIDEBAR_ITEM_H, h: SIDEBAR_ITEM_H }
    }

    // Rechte Haelfte der Leiste oben, von rechts nach links: Glocke, Uhr, Kopf, Name.
    pub(crate) fn topbar_layout(&self, gpu: &GpuState) -> TopbarLayout {
        let (sw, _) = gpu.size();
        let y = ((TOPBAR_H - TOPBAR_ICON) / 2.0).round();
        let bell = Rect { x: sw - 12.0 - TOPBAR_ICON, y, w: TOPBAR_ICON, h: TOPBAR_ICON };
        let mut uhr_w = gpu.measure_text("00:00:00", UI_SCALE_SMALL);
        if self.settings.topbar_uptime {
            uhr_w = uhr_w.max(gpu.measure_text("läuft seit 00:00", TOPBAR_SUB_SCALE));
        }
        let uhr = Rect { x: bell.x - 20.0 - uhr_w, y, w: uhr_w, h: TOPBAR_ICON };
        let kopf = Rect { x: uhr.x - 24.0 - 32.0, y: ((TOPBAR_H - 32.0) / 2.0).round(), w: 32.0, h: 32.0 };
        let name = match self.switching_account() {
            Some(a) => a.name.clone(),
            None => self.credentials.as_ref().map_or_else(|| "Nicht angemeldet".to_string(), |c| c.username.clone()),
        };
        let name_w = gpu.measure_text(&name, UI_SCALE_SMALL);
        let mut konto = Rect { x: kopf.x - 10.0 - name_w - 10.0, y, w: name_w + 10.0 + kopf.w + 20.0, h: TOPBAR_ICON };
        let konto_links = konto.x;
        // Bei offener Kontenliste steht "Acc-Manager" im Knopf (in Englisch laenger als ein
        // kurzer Name): dann nach links verbreitern, rechts bleibt er stehen.
        if self.account_menu_open {
            let noetig = gpu.measure_text("Acc-Manager", UI_SCALE_SMALL) + 32.0;
            if noetig > konto.w {
                konto = Rect { x: konto.x + konto.w - noetig, w: noetig, ..konto };
            }
        }
        let gear = Rect { x: 12.0, y, w: TOPBAR_ICON, h: TOPBAR_ICON };
        let gear_hit = Rect { x: 0.0, y: 0.0, w: gear.x + gear.w, h: TOPBAR_H };
        let bell_hit = Rect { x: bell.x, y: 0.0, w: sw - bell.x, h: TOPBAR_H };
        TopbarLayout { gear, bell, gear_hit, bell_hit, uhr, kopf, konto, konto_links, name }
    }

    // Klickbares der Leiste oben und der Kategorienleiste -- in Bildschirmkoordinaten.
    pub(crate) fn chrome_buttons(&self) -> Vec<Button> {
        let mut out = Vec::new();
        let Some(gpu) = self.gpu.as_ref().filter(|_| self.chrome_visible()) else { return out };
        let (_, sh) = gpu.size();
        let knopf = |action, rect| Button { action, rect, label: String::new(), enabled: true, color: [0.0; 4], scale: UI_SCALE_SMALL, slider: None };
        let l = self.topbar_layout(gpu);
        out.push(knopf(ButtonAction::ToggleSettings, l.gear_hit));
        out.push(knopf(ButtonAction::ToggleAccountMenu, l.konto));
        out.push(knopf(ButtonAction::ToggleNotificationPanel, l.bell_hit));
        out.push(knopf(ButtonAction::ToggleUptime, l.uhr));
        // Das Kreuz vor seinem Tab, damit es zuerst getroffen wird.
        let (tabs, plus) = self.tab_rects(&l);
        for (i, r) in tabs.iter().enumerate() {
            out.push(knopf(ButtonAction::CloseTab(i), r.close));
            out.push(knopf(ButtonAction::SelectTab(i), r.tab));
        }
        out.push(knopf(ButtonAction::NewTab, plus));
        for (i, (action, _, _)) in self.bottom_tiles().into_iter().enumerate() {
            out.push(knopf(action, bottom_tile_rect(i)));
        }
        if self.screen.settings_category().is_some() {
            for (i, (kategorie, _)) in Self::sidebar_categories().into_iter().enumerate() {
                out.push(knopf(ButtonAction::OpenScreen(kategorie), Self::sidebar_hit_rect(Self::sidebar_item_rect(i, sh))));
            }
            out.push(knopf(ButtonAction::OpenScreen(MenuScreen::SettingsSetup), Self::sidebar_hit_rect(Self::sidebar_setup_rect(sh))));
            out.push(knopf(ButtonAction::OpenScreen(MenuScreen::SettingsSearch), Self::sidebar_hit_rect(Self::sidebar_search_rect(sh))));
            for i in 0..SIDEBAR_LINKS.len() {
                out.push(knopf(ButtonAction::OpenLink(i), Self::sidebar_link_rect(i, sh)));
            }
        }
        if self.screen == MenuScreen::SettingsSkin && self.skin_files.is_none() && self.skin_drag_active().is_none() {
            out.extend(self.skin_strip(gpu).0);
        }
        out
    }

    // Die Knoepfe des aktuellen Screens in Bildschirmkoordinaten (s. content_offset). Auf
    // einer Kategorie der Einstellungen faellt der Zurueck-Knopf weg -- die Leiste links
    // ersetzt ihn.
    pub(crate) fn content_buttons(&self) -> Vec<Button> {
        let mut out = self.screen_buttons();
        // Hinter dem Fenster "Erweitert" ist die Seite gesperrt, auch ihre Reiter.
        if self.keybind_advanced.is_none() {
            out.extend(self.section_tabs());
        }
        // Wo ein Schieberegler steht, ersetzt er die Pfeile des alten Steppers. Die Reiter
        // bleiben: unter ihnen steckt beim Scrollen der Regler einer angeschnittenen Zeile.
        if self.mode == AppMode::Menu {
            let regler: Vec<Rect> = EditableValue::ALL
                .into_iter()
                .filter(|ev| ev.screen() == self.screen && ev.slider().is_some())
                .map(super::super::super::EditableValue::slider_rect)
                .collect();
            out.retain(|b| {
                let (mx, my) = (b.rect.x + b.rect.w / 2.0, b.rect.y + b.rect.h / 2.0);
                matches!(b.action, ButtonAction::SettingsSection(_)) || !regler.iter().any(|r| r.contains(mx, my))
            });
        }
        if self.mode == AppMode::Menu && self.screen.is_settings_category() {
            out.retain(|b| !(b.action == ButtonAction::Back && b.rect.x == BACK_BUTTON.x && b.rect.y == BACK_BUTTON.y));
            // Was gerade nicht geht, steht in den Einstellungen gar nicht erst da.
            out.retain(|b| b.enabled);
        }
        let (dx, dy) = self.content_offset();
        for b in &mut out {
            b.rect.x += dx;
            b.rect.y += dy;
        }
        // Ganz aus dem Zeilenband gescrollte Knoepfe sind nicht zu sehen (s. build_menu_ui) und
        // treffen auch nichts. Button::hits schneidet nur angeschnittene ab -- ein Knopf ganz
        // hinter den Reitern (Anpassen der anderen Partikel ueber Glint) fing sonst deren Klick ab.
        if self.mode == AppMode::Menu && self.screen.settings_page().is_some() {
            if let Some(band) = crate::content_clip() {
                out.retain(|b| {
                    let r = b.rect;
                    matches!(b.action, ButtonAction::SettingsSection(_) | ButtonAction::Back)
                        || r.x + r.w <= SIDEBAR_W
                        || (r.x < band.x + band.w && r.x + r.w > band.x && r.y < band.y + band.h && r.y + r.h > band.y)
                });
            }
        }
        out
    }

    // Reiter oben auf Einstellungsseiten mit mehreren Abschnitten (s. SettingsPage), der
    // aktive mit Akzentlinie (s. styled_button).
    pub(crate) fn section_tabs(&self) -> Vec<Button> {
        let page = match self.screen.settings_page() {
            Some(p) if self.mode == AppMode::Menu && p.tabs && p.sections.len() >= 2 => p,
            _ => return Vec::new(),
        };
        let aktiv = self.active_section(page);
        let mut x = SETTINGS_LABEL_X;
        let mut out = Vec::new();
        for (i, &(label, _)) in page.sections.iter().enumerate() {
            let w = self.gpu.as_ref().map_or(100.0, |g| g.measure_text(label, UI_SCALE_SMALL)) + 24.0;
            out.push(Button {
                action: ButtonAction::SettingsSection(i as u8),
                rect: Rect { x, y: SETTINGS_FIRST_Y - 44.0, w, h: 40.0 },
                label: label.to_string(),
                enabled: true,
                color: if i == aktiv { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                scale: UI_SCALE_SMALL,
                slider: None,
            });
            x += w + SECTION_TAB_GAP;
        }
        out
    }

    pub(crate) fn buttons(&self) -> Vec<Button> {
        let mut out = self.content_buttons();
        out.extend(self.chrome_buttons());
        out
    }

    // Kacheln der Leiste unten (nur Server-Seite): Aktion, Beschriftung, Farbe des Streifens.
    pub(crate) fn bottom_tiles(&self) -> Vec<(ButtonAction, &'static str, [f32; 4])> {
        use theme::Slot;
        if self.screen != MenuScreen::Connect {
            return Vec::new();
        }
        vec![
            (ButtonAction::ToggleRecordings, "Aufnahmen", theme::get(Slot::Accent2)),
            (ButtonAction::OpenQuickConnect, "Quick Connect", theme::get(Slot::Accent)),
            (ButtonAction::NewServer, "Neuer Eintrag", theme::get(Slot::Success)),
        ]
    }

    // Leiste unten wie in osu!: Kacheln mit runder Oberkante und farbigem Streifen unten.
    pub(crate) fn build_bottom_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        rects.push(UiRect { rect: Rect { x: 0.0, y: sh - BOTTOMBAR_H, w: sw, h: BOTTOMBAR_H }, color: g(Slot::BgPanel), ..UiRect::FLAT });
        rects.push(UiRect { rect: Rect { x: 0.0, y: sh - BOTTOMBAR_H, w: sw, h: 1.0 }, color: g(Slot::Border1), ..UiRect::FLAT });
        for (i, (_, label, farbe)) in self.bottom_tiles().into_iter().enumerate() {
            let r = bottom_tile_rect(i);
            let k = self.hover_level(r, r.contains(cx, cy));
            // Unten ueber den Rand hinaus, damit nur die oberen Ecken rund sind.
            let flaeche = Rect { h: r.h + 12.0, ..r };
            rects.push(UiRect::framed(flaeche, mix_color(g(Slot::Surface2), g(Slot::Surface3), k), 8.0, 1.0, g(Slot::Border1)));
            rects.push(UiRect { rect: Rect { x: r.x + 1.0, y: r.y + r.h - 5.0, w: r.w - 2.0, h: 5.0 }, color: farbe, ..UiRect::FLAT });
            texts.push(gpu.centered_text(Rect { h: r.h - 5.0, ..r }, label.to_string(), theme::text(), UI_SCALE_SMALL));
        }
    }
}
