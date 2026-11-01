// Sprache und Schriften.

use super::*;

impl App {
    pub(crate) fn build_settings_language_tab(&self, gpu: &GpuState, texts: &mut Vec<UiText>) {
        let beschriftungen = LANGUAGE_ROWS.iter().map(|(label, _)| *label).chain(["Längezeichen (Latein)", "Lehnwörter auf Englisch"]);
        for (row, label) in beschriftungen.enumerate() {
            let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            texts.push(UiText { text: label.to_string(), x: SETTINGS_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        let y = settings_row_y(LOANWORDS_ROW + 1) + 10.0;
        for (i, zeile) in wrap_text(gpu, LANGUAGE_HINT, UI_SCALE_SMALL, gpu.size().0 - SETTINGS_LABEL_X - 40.0).into_iter().enumerate() {
            texts.push(UiText {
                text: zeile,
                x: SETTINGS_LABEL_X,
                y: y + i as f32 * (gpu.text_height(UI_SCALE_SMALL) + 4.0),
                color: theme::text_disabled(),
                scale: UI_SCALE_SMALL,
            });
        }
    }

    // Die Sprache einer Zeile im Sprach-Tab; "Alles" nur, wenn alle Bereiche gleich sind.
    pub(crate) fn language_of_row(&self, row: usize) -> Option<i18n::Language> {
        let set = &self.settings;
        let bereiche = [set.lang_ui, set.lang_items, set.lang_effects, set.lang_chat];
        let index = match row {
            0 => bereiche.iter().all(|&b| b == bereiche[0]).then_some(bereiche[0])?,
            r => *bereiche.get(r - 1)?,
        };
        Some(i18n::Language::from_index(index))
    }

    pub(crate) fn build_settings_font_tab(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        let allgemein = [(0, "Standard"), (STABLE_NUMBERS_ROW, "Stabile Zahlen"), (CAPITALS_ROW, "Großbuchstaben")];
        let ausnahmen = ShadowArea::ALL.into_iter().map(|a| (FONT_EXCEPTIONS_ROW + a.index(), a.label()));
        for (row, text) in allgemein.into_iter().chain(ausnahmen) {
            texts.push(UiText { text: text.to_string(), x: SETTINGS_LABEL_X, y: zeile_y(row), color: theme::text(), scale: UI_SCALE_SMALL });
        }
        // Schattenstaerke neben jedem gefuellten Schatten-Kaestchen (s. editable_shown).
        for ev in EditableValue::ALL.into_iter().filter(|ev| ev.screen() == MenuScreen::SettingsFont && self.editable_shown(*ev)) {
            if ev.rect().y < 0.0 {
                continue;
            }
            let wert = self.editable_number(ev);
            let text = match ev {
                EditableValue::ShadowThickness(_) => format!("{wert:.1} px"),
                // Ohne "%": rechts daneben stehen bei Vanilla noch die Knoepfe fuer den Versatz.
                EditableValue::ShadowOpacity(_) => format!("{wert:.0}"),
                _ => format!("{wert:.0} %"),
            };
            self.draw_editable_value(gpu, ev.rect(), ev, text, rects, texts);
        }
    }

    // Die Kaestchen des Schrift-Tabs, in Inhaltskoordinaten. Erst nach fit_texts gezeichnet,
    // damit der Name in seiner eigenen Schrift nicht mit der allgemeinen eingepasst wird.
    pub(crate) fn build_font_slots(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let ziel = self.font_drop_target();
        for slot in FontSlot::all() {
            let Some(r) = slot.rect() else { continue };
            // Was gerade herausgezogen wird, fehlt schon im Kaestchen.
            let heraus = !slot.is_default() && self.font_drag.is_some_and(|d| d.from == Some(slot));
            let inhalt = if heraus { None } else { self.settings.font_slot(slot) };
            let rahmen = if ziel == Some(slot) { theme::get(theme::Slot::Accent) } else { theme::get(theme::Slot::Border1) };
            Self::push_font_slot_tile(gpu, rects, texts, r, inhalt, rahmen);
        }
    }

    // Ein Kaestchen links im Schrift-Tab: leer, oder der Name der Schrift (in ihr gesetzt)
    // bzw. des Schattens.
    pub(crate) fn push_font_slot_tile(gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, r: Rect, tile: Option<FontTile>, rahmen: [f32; 4]) {
        rects.push(UiRect::framed(r, theme::get(theme::Slot::SurfaceSunken), 4.0, 1.0, rahmen));
        let Some(tile) = tile else { return };
        let font = match tile {
            FontTile::Font(kind) => Some(kind),
            FontTile::Shadow(_) => None,
        };
        gpu.set_layout_font(font);
        let name = ellipsize(gpu, tile.label(), UI_SCALE_SMALL, r.w - 12.0);
        let w = gpu.measure_text(&name, UI_SCALE_SMALL);
        let y = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        push_font_marker(texts, font);
        texts.push(UiText { text: name, x: (r.x + (r.w - w) / 2.0).round(), y, color: theme::text(), scale: UI_SCALE_SMALL });
        push_font_marker(texts, None);
        gpu.set_layout_font(None);
    }

    // Eine Kachel der Auswahl rechts: oben die Probe (Schrift: "Gabriel" und darunter Ziffern
    // in ihr; Schatten: ein grosses A damit auf grau-gelben Karos), darunter ein Strich und der
    // Name. Schriften ohne Rahmen, die Schatten stehen in einem Raster (s. font_palette).
    pub(crate) fn push_font_palette_tile(gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, r: Rect, tile: FontTile) {
        let name_h = font_palette_name_h(gpu);
        let probe = Rect { h: r.h - name_h, ..r };
        match tile {
            FontTile::Shadow(stil) => {
                let mut y = 0.0;
                while y < probe.h {
                    let mut x = 0.0;
                    while x < probe.w {
                        let gelb = ((x / SHADOW_CHECKER_PX) as usize + (y / SHADOW_CHECKER_PX) as usize) % 2 == 1;
                        let feld = Rect { x: probe.x + x, y: probe.y + y, w: SHADOW_CHECKER_PX.min(probe.w - x), h: SHADOW_CHECKER_PX.min(probe.h - y) };
                        rects.push(UiRect { rect: feld, color: if gelb { SHADOW_CHECKER_YELLOW } else { SHADOW_CHECKER_GREY }, ..UiRect::FLAT });
                        x += SHADOW_CHECKER_PX;
                    }
                    y += SHADOW_CHECKER_PX;
                }
                let scale = 2.0 * UI_SCALE_SMALL;
                let w = gpu.measure_text("A", scale);
                let y = (probe.y + (probe.h - gpu.text_height(scale)) / 2.0).round();
                push_shadow_marker(texts, Some((stil, 1.0, false)));
                texts.push(UiText { text: "A".to_string(), x: (probe.x + (probe.w - w) / 2.0).round(), y, color: SHADOW_PREVIEW_TEXT, scale });
                push_shadow_marker(texts, None);
            }
            FontTile::Font(kind) => {
                gpu.set_layout_font(Some(kind));
                let zeile = gpu.text_height(UI_SCALE_SMALL) + 2.0;
                let oben = (probe.y + f32::midpoint(probe.h - 2.0 * zeile, 2.0)).round();
                push_font_marker(texts, Some(kind));
                for (i, wort) in ["Gabriel", "1247"].into_iter().enumerate() {
                    let w = gpu.measure_text(wort, UI_SCALE_SMALL);
                    texts.push(UiText { text: wort.to_string(), x: (probe.x + (probe.w - w) / 2.0).round(), y: oben + i as f32 * zeile, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                push_font_marker(texts, None);
                gpu.set_layout_font(None);
            }
        }
        rects.push(UiRect { rect: Rect { x: r.x, y: probe.y + probe.h, w: r.w, h: 1.0 }, color: theme::get(theme::Slot::Border1), ..UiRect::FLAT });
        let name = ellipsize(gpu, tile.label(), UI_SCALE_SMALL, r.w - 8.0);
        let w = gpu.measure_text(&name, UI_SCALE_SMALL);
        let y = (probe.y + probe.h + (name_h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        texts.push(UiText { text: name, x: (r.x + (r.w - w) / 2.0).round(), y, color: theme::text(), scale: UI_SCALE_SMALL });
    }

    // Die Auswahl im Rand rechts des Schrift-Tabs, in Bildschirmkoordinaten: oben die
    // Schriften, vier je Reihe, dann ein Strich quer durch den Rand und darunter die Schatten
    // in einem Raster, ebenfalls vier je Reihe.
    pub(crate) fn font_palette(&self, gpu: &GpuState) -> FontPalette {
        const PADDING: f32 = 12.0;
        const ABSTAND: f32 = 16.0;
        const JE_REIHE: usize = 4;
        let (sw, _) = gpu.size();
        let rand = self.side_strip_w(sw);
        let links = sw - rand + 8.0 + PADDING;
        let rechts = sw - 8.0 - PADDING;
        let th = gpu.text_height(UI_SCALE_SMALL);
        let name_h = font_palette_name_h(gpu);
        let mut y = SETTINGS_FIRST_Y + SETTINGS_CONTENT_DY;
        let mut headings = Vec::new();
        let mut tiles = Vec::new();
        let mut lines = Vec::new();
        // Schriften: Kacheln mit Abstand.
        headings.push(("Schriften", y));
        y += th + 10.0;
        let w = ((rechts - links - (JE_REIHE - 1) as f32 * ABSTAND) / JE_REIHE as f32).floor();
        let h = (2.0 * (th + 2.0) + 20.0 + name_h).round();
        let schriften: Vec<FontTile> = FontTile::palette().filter(|t| matches!(t, FontTile::Font(_))).collect();
        for (i, tile) in schriften.iter().enumerate() {
            let (spalte, reihe) = (i % JE_REIHE, i / JE_REIHE);
            tiles.push((*tile, Rect { x: links + spalte as f32 * (w + ABSTAND), y: y + reihe as f32 * (h + ABSTAND), w, h }));
        }
        y += schriften.len().div_ceil(JE_REIHE) as f32 * (h + ABSTAND) + 4.0;
        // Strich quer durch den Rand.
        lines.push(Rect { x: sw - rand, y: y.round(), w: rand, h: 1.0 });
        y += 20.0;
        headings.push(("Schatten", y));
        y += th + 10.0;
        // Schatten: Zellen ohne Luecke, die Linien dazwischen sind das Raster.
        let w = ((rechts - links) / JE_REIHE as f32).floor();
        let h = (2.0 * gpu.text_height(UI_SCALE_SMALL) + 24.0 + name_h).round();
        let schatten: Vec<FontTile> = FontTile::palette().filter(|t| matches!(t, FontTile::Shadow(_))).collect();
        let reihen = schatten.len().div_ceil(JE_REIHE);
        for (i, tile) in schatten.iter().enumerate() {
            let (spalte, reihe) = (i % JE_REIHE, i / JE_REIHE);
            tiles.push((*tile, Rect { x: links + spalte as f32 * w, y: y + reihe as f32 * h, w, h }));
        }
        let raster = |lines: &mut Vec<Rect>| {
            for reihe in 0..reihen {
                let in_reihe = (schatten.len() - reihe * JE_REIHE).min(JE_REIHE);
                let oben = y + reihe as f32 * h;
                let breite = in_reihe as f32 * w;
                lines.push(Rect { x: links, y: oben, w: breite + 1.0, h: 1.0 });
                lines.push(Rect { x: links, y: oben + h, w: breite + 1.0, h: 1.0 });
                for k in 0..=in_reihe {
                    lines.push(Rect { x: links + k as f32 * w, y: oben, w: 1.0, h: h + 1.0 });
                }
            }
        };
        raster(&mut lines);
        y += reihen as f32 * h + 2.0 * ABSTAND;
        FontPalette { x: links, headings, tiles, lines, bottom: y }
    }

    pub(crate) fn build_font_palette(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let palette = self.font_palette(gpu);
        for (titel, y) in palette.headings {
            texts.push(UiText { text: titel.to_string(), x: palette.x, y, color: theme::text(), scale: UI_SCALE_SMALL });
        }
        for (tile, r) in palette.tiles {
            Self::push_font_palette_tile(gpu, rects, texts, r, tile);
        }
        for l in palette.lines {
            rects.push(UiRect { rect: l, color: theme::get(theme::Slot::Border1), ..UiRect::FLAT });
        }
    }

    // Breite des Rands rechts auf den Einstellungsseiten (s. tooltip_strip_w). Im Schrift-Tab
    // doppelt so breit: dort steht ueber den Erklaerungen die Auswahl zum Hineinziehen.
    pub(crate) fn side_strip_w(&self, screen_w: f32) -> f32 {
        let w = tooltip_strip_w(screen_w);
        match self.screen {
            MenuScreen::SettingsFont => 2.0 * w,
            // Die Partikel-Tabelle braucht die ganze Breite und hat keine Erklaerungen, ebenso
            // Aussehen (Kacheln und Farben nebeneinander) und die Module (Kacheln, ohne Tooltips).
            MenuScreen::SettingsParticles | MenuScreen::SettingsDesign | MenuScreen::SettingsModules => 0.0,
            _ => w,
        }
    }

    // Kaestchen des Schrift-Tabs unter dem Zeiger (Bildschirmkoordinaten).
    pub(crate) fn font_slot_at(&self, x: f32, y: f32) -> Option<FontSlot> {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsFont {
            return None;
        }
        let (dx, dy) = self.content_offset();
        FontSlot::all().find(|s| s.rect().is_some_and(|r| r.contains(x - dx, y - dy)))
    }

    // Wohin das gezogene Kaestchen fiele: das unter dem Zeiger, wenn es dazu passt.
    pub(crate) fn font_drop_target(&self) -> Option<FontSlot> {
        let d = self.font_drag?;
        self.font_slot_at(self.cursor_pos.0, self.cursor_pos.1).filter(|s| s.accepts(d.tile))
    }

    // Druck im Schrift-Tab: ein Kaestchen der Auswahl oder ein gefuelltes links wird
    // aufgenommen. false, wenn dort keins ist.
    pub(crate) fn font_drag_press(&mut self) -> bool {
        let (cx, cy) = self.cursor_pos;
        let aus_auswahl = self
            .gpu
            .as_ref()
            .filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsFont)
            .and_then(|gpu| self.font_palette(gpu).tiles.into_iter().find(|(_, r)| r.contains(cx, cy)))
            .map(|(tile, r)| (tile, None, r));
        let gefunden = aus_auswahl.or_else(|| {
            let slot = self.font_slot_at(cx, cy)?;
            let (r, (dx, dy)) = (slot.rect()?, self.content_offset());
            Some((self.settings.font_slot(slot)?, Some(slot), Rect { x: r.x + dx, y: r.y + dy, ..r }))
        });
        let Some((tile, from, r)) = gefunden else { return false };
        // Am Zeiger haengt es in der Groesse der Kaestchen links: aus der groesseren Auswahl
        // gegriffen, haelt er es in der Mitte.
        let grab = if from.is_some() { (cx - r.x, cy - r.y) } else { (tile.box_w() / 2.0, FONT_SLOT_H / 2.0) };
        self.font_drag = Some(FontDrag { tile, from, grab });
        true
    }

    // Loslassen: ins passende Kaestchen unter dem Zeiger. Was aus einem Kaestchen kam und
    // woanders landet, ist dort weg -- auch, wenn es neben jedes Kaestchen faellt.
    pub(crate) fn font_drag_release(&mut self) {
        let Some(d) = self.font_drag.take() else { return };
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsFont {
            return;
        }
        let ziel = self.font_slot_at(self.cursor_pos.0, self.cursor_pos.1).filter(|s| s.accepts(d.tile));
        if ziel.is_some() && ziel == d.from {
            return;
        }
        if let Some(ziel) = ziel {
            self.set_font_slot(ziel, Some(d.tile));
            self.play_ui_sound(ui_sound::UiSound::Click);
        }
        if let Some(von) = d.from {
            self.set_font_slot(von, None);
        }
        self.persist_settings();
    }

    // Ein Kaestchen des Schrift-Tabs fuellen oder leeren (None). Standard-Schrift und
    // -Schatten lassen sich nicht leeren.
    pub(crate) fn set_font_slot(&mut self, slot: FontSlot, tile: Option<FontTile>) {
        let schrift = match (slot, tile) {
            (FontSlot::DefaultFont, Some(FontTile::Font(kind))) => {
                let i = area_font_code(Some(kind)) - 1;
                if self.settings.font == i {
                    return;
                }
                self.settings.font = i;
                if let Some(gpu) = self.gpu.as_mut() {
                    if let Err(e) = gpu.font_atlas.switch(&gpu.device, &gpu.queue, &mut self.installation, kind) {
                        eprintln!("Schrift konnte nicht gewechselt werden: {e}");
                    }
                }
                true
            }
            (FontSlot::DefaultShadow, Some(FontTile::Shadow(stil))) => {
                self.settings.set_global_text_shadow(stil);
                self.sync_text_shadow();
                false
            }
            (FontSlot::AreaFont(a), t) => {
                let font = match t {
                    Some(FontTile::Font(kind)) => Some(kind),
                    _ => None,
                };
                self.settings.area_font[a.index()] = area_font_code(font);
                true
            }
            (FontSlot::AreaShadow(a), t) => {
                self.settings.area_shadow[a.index()] = match t {
                    Some(FontTile::Shadow(stil)) => area_shadow_code(stil),
                    _ => 0,
                };
                false
            }
            _ => return,
        };
        // Andere Schrift, andere Breiten -- die eingerasteten gelten nicht mehr.
        if schrift {
            self.hud_latched_widths.borrow_mut().clear();
            if let Some(gpu) = self.gpu.as_ref() {
                gpu.stable_numbers.borrow_mut().clear();
            }
        }
    }
}
