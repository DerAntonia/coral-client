// Seiten scrollen, Listen ziehen und Abschnitte.

use super::*;

impl App {
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
            MenuScreen::SettingsParticles => self.particle_scroll,
            s => self.page_scrolls.get(s.settings_key()).copied().unwrap_or_default(),
        }
    }

    pub(crate) fn page_scroll_mut(&mut self) -> &mut smooth_scroll::SmoothScroll {
        match self.screen {
            MenuScreen::SettingsGeneral => &mut self.settings_scroll,
            MenuScreen::SettingsControls => &mut self.keybind_scroll,
            MenuScreen::SettingsParticles => &mut self.particle_scroll,
            s => self.page_scrolls.entry(s.settings_key()).or_default(),
        }
    }

    // Verlauf am Rand des Zeilenbands der aktuellen Seite. Steht ein Abschnitt genau oben, ist
    // darueber nichts von ihm abgeschnitten: oben kein Verlauf. Er blendet ueber eine Zeile
    // Abstand zum naechsten Abschnittsanfang ein.
    pub(crate) fn page_fade(&self) -> crate::ScrollFade {
        let h = self.gpu.as_ref().map_or(1080.0, |g| g.size().1);
        let stand = self.page_scroll().stand;
        let mut fade = crate::ScrollFade::new(SETTINGS_CLIP_TOP, settings_rows_bottom(h), SETTINGS_ROW_H, stand, self.page_max_scroll() as f32);
        if let Some(page) = self.screen.settings_page() {
            let abstand = page.sections.iter().map(|&(_, row)| (stand - page.section_stand(row)).abs()).fold(stand.abs(), f32::min);
            fade.edge_top = 1.0 - (1.0 - crate::SCROLL_FADE_EDGE) * abstand.clamp(0.0, 1.0);
        }
        fade
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
        if let Some(page) = self.screen.settings_page().filter(|_| self.mode == AppMode::Menu) {
            let h = self.gpu.as_ref().map_or(1080.0, |g| g.size().1);
            THEME_TILE_COUNT.store(theme::PRESETS.len() + self.settings.custom_themes.len() + 1, Relaxed);
            SETTINGS_SCROLL_FIRST.store(self.page_scroll().stand.to_bits(), Relaxed);
            SETTINGS_SCROLL_BOTTOM.store(settings_rows_bottom(h).to_bits(), Relaxed);
            store_section_starts(page.section_starts());
            // Das Zeilenband auf dem Bildschirm: von der Inhaltsspalte bis zum Rand rechts.
            let w = self.gpu.as_ref().map_or(1920.0, |g| g.size().0);
            let (dx, dy) = self.content_offset();
            let oben = SETTINGS_CLIP_TOP + dy;
            store_content_clip(Some(Rect { x: dx, y: oben, w: w - self.side_strip_w(w) - dx, h: settings_rows_bottom(h) + dy - oben }));
            crate::store_module_clip(None);
        } else {
            SETTINGS_SCROLL_FIRST.store(0f32.to_bits(), Relaxed);
            SETTINGS_SCROLL_BOTTOM.store(f32::INFINITY.to_bits(), Relaxed);
            store_section_starts(0);
            store_content_clip(None);
            // Die Detailansicht der Module schneidet am eigenen Zeilenband (s. module_detail_band),
            // im Inhaltsraum der Einstellungen, auf dem Bildschirm also um content_offset versetzt.
            let (dx, dy) = self.content_offset();
            let band = self.gpu.as_ref().filter(|_| self.module_detail_cut()).map(|g| {
                let b = self.module_detail_band(g);
                Rect { x: b.x + dx, y: b.y + dy, ..b }
            });
            crate::store_module_clip(band);
        }
    }

    // `weg` Zeilen Richtung Seitenende (s. App::wheel_units).
    pub(crate) fn scroll_settings(&mut self, weg: f32) {
        let max = self.page_max_scroll() as f32;
        self.page_scroll_mut().push(weg, SETTINGS_SCROLL_ROWS_PER_NOTCH, max);
    }

    // Reiter angeklickt: Abschnitt `i` nach oben gleiten (so weit die Seite reicht).
    pub(crate) fn jump_to_section(&mut self, i: usize) {
        let Some(page) = self.screen.settings_page() else { return };
        let Some(&(_, row)) = page.sections.get(i) else { return };
        let max = self.page_max_scroll();
        let erste = row.min(max);
        self.page_scroll_mut().glide_to(page.section_stand(row).min(max as f32), max as f32);
        self.section_click = Some((self.screen, i, erste));
        self.section_flash = Some((self.screen, i, Instant::now()));
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
        self.list_under_pointer()
    }

    // Die scrollbare Liste unter dem Zeiger, auch wenn dort ein Regler liegt.
    pub(crate) fn list_under_pointer(&self) -> Option<ScrollList> {
        let (qx, qy) = self.pointer();
        let liste = if self.module_menu_shown() {
            let gpu = self.gpu.as_ref()?;
            let panel = self.module_menu_rect(gpu);
            // In der Uebersicht scrollen nur die Kacheln, nicht die Kategorien links daneben.
            let links = if self.module_menu_detail { panel.x } else { panel.x + (Self::MODULE_CAT_W + Self::MODULE_PAD) * self.module_scale(gpu) };
            (panel.contains(qx, qy) && qx >= links).then_some(ScrollList::ModuleMenu)?
        } else if self.mode != AppMode::Menu {
            return None;
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
        self.drag_scroll_no_click = false;
        true
    }

    // Den Scrollstand der Liste und ihre Masse (Pixel je Einheit, Raste, Ende) zusammen.
    pub(crate) fn with_drag_scroll(&mut self, liste: ScrollList, f: impl FnOnce(&mut smooth_scroll::SmoothScroll, f32, f32, f32)) {
        if let ScrollList::Dialog | ScrollList::CodeOfConduct = liste {
            if let Some((s, raste, max)) = self.dialog_drag_scroll(liste == ScrollList::CodeOfConduct) {
                f(s, 1.0, raste, max);
            }
            return;
        }
        let Some((px, raste, max)) = self.drag_scroll_geometry(liste) else { return };
        let s = match liste {
            ScrollList::SettingsPage(_) => self.page_scroll_mut(),
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
        if std::mem::take(&mut self.drag_scroll_no_click) {
            return;
        }
        match liste {
            ScrollList::Dialog | ScrollList::CodeOfConduct if self.dialog_shown() || self.code_of_conduct.is_some() => self.dialog_press(false),
            ScrollList::Dropdown if self.dropdown.is_some() => {
                self.dropdown_press();
            }
            ScrollList::SettingsPage(_) | ScrollList::ModuleMenu if self.drag_scroll_geometry(liste).is_some() => {
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
}
