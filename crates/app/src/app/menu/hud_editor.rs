// Hauptbildschirm, Fadenkreuz-Editor, HUD-Konfiguration und -Editor, Tod- und Verbinden-Bildschirm.

use super::*;

impl App {
    // Main-Screen: getrennt -> Startbildschirm (nur Titel + Buttons Verbinden/Einstellungen;
    // die Adress-Eingabe steckt jetzt im Connect-Screen), verbunden -> Esc-Pausemenue
    // (nur Titel). Die Buttons kommen aus buttons().
    pub(crate) fn build_main_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        // Titel wie die Buttons mittig.
        let (sw, _) = gpu.size();
        let titel = |text: &str| {
            let w = gpu.measure_text(text, UI_SCALE_TITLE);
            UiText { text: text.to_string(), x: ((sw - w) / 2.0).round().max(UI_MARGIN), y: 90.0, color: theme::text(), scale: UI_SCALE_TITLE }
        };
        if self.conn_status == ConnStatus::Disconnected {
            if gpu.wallpaper.is_some() {
                let (w, h) = gpu.size();
                rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w, h }, color: WALLPAPER_DIM, ..UiRect::FLAT });
            }
            texts.push(titel("Coral Client"));
        } else {
            // Nur der Titel; die Chunk-Zahlen stehen im F3-Overlay.
            texts.push(titel("Pause"));
        }
    }

    // HUD-Editor: Raster zum Ausrichten + das verschiebbare Perf-Element (hervorgehoben).
    // Fertig-Button und Sichtbarkeits-Schalter kommen aus buttons().
    // Lage des Malrasters: (x, y der linken oberen Ecke, Kantenlaenge einer Zelle).
    pub(crate) fn crosshair_painter_grid(&self, gpu: &GpuState) -> (f32, f32, f32) {
        let (sw, sh) = gpu.size();
        let links = 40.0 + HUD_LIST_ITEM_W + 40.0;
        let vorschau = 200.0;
        let n = crosshair_editor::GRID as f32;
        let zelle = ((sh - 200.0).min(sw - links - vorschau - 40.0) / n).floor().clamp(4.0, 20.0);
        let g = zelle * n;
        let x0 = (links + ((sw - links - vorschau - g) / 2.0).max(0.0)).round();
        let y0 = ((sh - g) / 2.0).round();
        (x0, y0, zelle)
    }

    pub(crate) fn crosshair_painter_cell(&self, p: (f32, f32)) -> Option<(i32, i32)> {
        let gpu = self.gpu.as_ref()?;
        let (x0, y0, zelle) = self.crosshair_painter_grid(gpu);
        let (x, y) = (((p.0 - x0) / zelle).floor() as i32, ((p.1 - y0) / zelle).floor() as i32);
        let n = crosshair_editor::GRID as i32;
        ((0..n).contains(&x) && (0..n).contains(&y)).then_some((x, y))
    }

    // Maustaste im Pixel-Editor. Links malt Fuellung, rechts Kontur; beginnt der Strich auf
    // einem Pixel, das schon so aussieht, loescht er stattdessen. true = Klick verbraucht.
    pub(crate) fn crosshair_painter_mouse(&mut self, rechts: bool, gedrueckt: bool) -> bool {
        let Some(painter) = self.crosshair_painter.as_ref() else { return false };
        if !gedrueckt {
            let hatte = self.crosshair_painter.as_mut().and_then(|p| p.stroke.take()).is_some();
            if hatte {
                self.persist_settings();
            }
            return hatte;
        }
        let Some(zelle) = self.crosshair_painter_cell(self.cursor_pos) else { return false };
        let werkzeug = if rechts { crosshair_editor::Pixel::Outline } else { crosshair_editor::Pixel::Fill };
        let jetzt = self.settings.crosshair_pattern.get(zelle.0, zelle.1);
        let wert = if jetzt == werkzeug { crosshair_editor::Pixel::Empty } else { werkzeug };
        let sym = painter.symmetry;
        self.settings.crosshair_pattern.paint(zelle.0, zelle.1, wert, sym);
        if let Some(p) = self.crosshair_painter.as_mut() {
            p.stroke = Some((wert, zelle));
        }
        true
    }

    // Zeiger bewegt: laufenden Strich bis zur neuen Zelle ziehen, ohne Luecken bei schnellen
    // Bewegungen.
    pub(crate) fn crosshair_painter_drag(&mut self) {
        let Some(painter) = self.crosshair_painter.as_ref() else { return };
        let Some((wert, (mut x, mut y))) = painter.stroke else { return };
        let sym = painter.symmetry;
        let Some((zx, zy)) = self.crosshair_painter_cell(self.cursor_pos) else { return };
        // Bresenham von der letzten zur neuen Zelle.
        let (dx, dy) = ((zx - x).abs(), -(zy - y).abs());
        let (sx, sy) = ((zx - x).signum(), (zy - y).signum());
        let mut err = dx + dy;
        loop {
            self.settings.crosshair_pattern.paint(x, y, wert, sym);
            if x == zx && y == zy {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        if let Some(p) = self.crosshair_painter.as_mut() {
            p.stroke = Some((wert, (zx, zy)));
        }
    }

    // Der Pixel-Editor: Raster in Vergroesserung, Mittellinien, Vorschau in Originalgroesse.
    pub(crate) fn build_crosshair_painter(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.05, 0.05, 0.07, 0.92], ..UiRect::FLAT });
        let (x0, y0, zelle) = self.crosshair_painter_grid(gpu);
        let n = crosshair_editor::GRID as i32;
        let p = &self.settings.crosshair_pattern;
        // Eine Flaeche plus Rasterlinien plus ein Viereck je Lauf gemalter Pixel -- ein Viereck
        // je Zelle waeren 48x48, weit mehr als der UI-Puffer fasst.
        let g = zelle * n as f32;
        let mitte = (x0 + g / 2.0, y0 + g / 2.0);
        rects.push(UiRect { rect: Rect { x: x0, y: y0, w: g, h: g }, color: [0.0, 0.0, 0.0, 0.55], ..UiRect::FLAT });
        let raster = [1.0, 1.0, 1.0, 0.06];
        for i in 1..n {
            let o = (i as f32 * zelle).round();
            rects.push(UiRect { rect: Rect { x: x0 + o, y: y0, w: 1.0, h: g }, color: raster, ..UiRect::FLAT });
            rects.push(UiRect { rect: Rect { x: x0, y: y0 + o, w: g, h: 1.0 }, color: raster, ..UiRect::FLAT });
        }
        push_pattern_rects(p, mitte, zelle, rects);
        // Die Bildmitte (Blickstrahl) liegt zwischen den vier mittleren Pixeln.
        let linie = [1.0, 0.3, 0.3, 0.7];
        rects.push(UiRect { rect: Rect { x: mitte.0 - 0.5, y: y0, w: 1.0, h: g }, color: linie, ..UiRect::FLAT });
        rects.push(UiRect { rect: Rect { x: x0, y: mitte.1 - 0.5, w: g, h: 1.0 }, color: linie, ..UiRect::FLAT });

        // Vorschau 1:1 -- so gross erscheint es im Spiel bei dieser Aufloesung.
        let kasten = Rect { x: x0 + g + 40.0, y: mitte.1 - 50.0, w: 100.0, h: 100.0 };
        rects.push(UiRect { rect: kasten, color: [0.45, 0.55, 0.62, 1.0], ..UiRect::FLAT });
        push_pattern_rects(p, ((kasten.x + kasten.w / 2.0).round(), (kasten.y + kasten.h / 2.0).round()), 1.0, rects);
        texts.push(UiText { text: "1:1".to_string(), x: kasten.x, y: kasten.y + kasten.h + 8.0, color: theme::text(), scale: UI_SCALE_SMALL });

        texts.push(UiText {
            text: "Links: Füllung   Rechts: Kontur   Auf gleichem Pixel: löschen   Esc: fertig".to_string(),
            x: x0,
            y: (y0 + g + 12.0).min(sh - 30.0),
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
    }

    // Einstellungen eines HUD-Elements (Rechtsklick im Editor): Titel, Zeilen wie auf einer
    // Einstellungsseite, darunter Schrift und Zuruecksetzen. In Inhaltskoordinaten --
    // build_menu_ui schiebt es in die Bildmitte und legt es als Fenster ueber den Editor.
    pub(crate) fn build_hud_config(&self, gpu: &GpuState, element: HudElement, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let panel = hud_config_panel(element, &self.settings);
        texts.push(UiText { text: element.label().to_string(), x: HUD_CONFIG_LABEL_X, y: panel.y + 22.0, color: theme::text(), scale: UI_SCALE_NORMAL });
        let row0_y = (settings_row_y(0) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
        match element {
            HudElement::Crosshair => {
                texts.push(UiText {
                    text: "Kreuz-Farbe".to_string(),
                    x: HUD_CONFIG_LABEL_X,
                    y: row0_y,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
                // Kontur gilt fuer jeden Farbmodus (Zeile 1), s. buttons().
                let row1_y = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText {
                    text: "Kontur".to_string(),
                    x: HUD_CONFIG_LABEL_X,
                    y: row1_y,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
                let zeile = crosshair_shape_row(self.settings.crosshair_color_mode);
                let zeile_y = |row: usize| (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: "Form".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(zeile), color: theme::text(), scale: UI_SCALE_SMALL });
                // Die Form selbst ist eine Aufklappliste aus buttons().
                if !self.settings.crosshair_custom {
                    texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(zeile + 1), color: theme::text(), scale: UI_SCALE_SMALL });
                    let groesse_rect = stepper_value_rect(zeile + 1);
                    rects.push(UiRect { rect: groesse_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(groesse_rect, format!("{} px", self.settings.crosshair_size()), theme::text(), UI_SCALE_SMALL));
                }
                texts.push(UiText { text: "Ring-Radius".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(zeile + 2), color: theme::text(), scale: UI_SCALE_SMALL });
                let radius_rect = stepper_value_rect(zeile + 2);
                rects.push(UiRect { rect: radius_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                texts.push(gpu.centered_text(radius_rect, format!("{} px", self.settings.crosshair_ring_radius), theme::text(), UI_SCALE_SMALL));
                texts.push(UiText { text: "Ring hat Kontur".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(2), color: theme::text(), scale: UI_SCALE_SMALL });
                // Aufhellung nur beim Komplementaer-Modus (Zeile 3).
                if self.settings.crosshair_color_mode == CROSSHAIR_MODE_COMPLEMENTARY {
                    texts.push(UiText { text: "Aufhellung".to_string(), x: HUD_CONFIG_LABEL_X, y: zeile_y(3), color: theme::text(), scale: UI_SCALE_SMALL });
                    let boost_rect = stepper_value_rect(3);
                    rects.push(UiRect { rect: boost_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(
                        boost_rect,
                        format!("{} %", self.settings.crosshair_brightness_boost),
                        theme::text(),
                        UI_SCALE_SMALL,
                    ));
                }
            }
            HudElement::Chat => {
                for (row, (label, wert)) in self.chat_setting_rows().into_iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: label.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                    // Die Wahl Java/Bedrock sind zwei Knoepfe (s. buttons), kein Wertfeld.
                    if row == CHAT_STYLE_ROW {
                        continue;
                    }
                    let rect = stepper_value_rect(row);
                    rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(rect, wert, theme::text(), UI_SCALE_SMALL));
                }
                // Erklaerung der Zeile unter dem Zeiger in einem Kasten direkt unter dem Fenster
                // (die Einstellungsseiten haben dafuer den Rand rechts, der HUD-Editor nicht).
                let (px, py) = self.pointer();
                let zeile = (0..CHAT_TOOLTIPS.len()).find(|&row| Rect { x: panel.x, y: settings_row_y(row), w: panel.w, h: SETTINGS_ROW_H }.contains(px, py));
                if let Some(row) = zeile {
                    const PADDING: f32 = 12.0;
                    let line_h = gpu.text_height(UI_SCALE_SMALL) + 4.0;
                    let lines = wrap_text(gpu, CHAT_TOOLTIPS[row], UI_SCALE_SMALL, panel.w - 2.0 * PADDING);
                    let kasten = Rect { x: panel.x, y: panel.y + panel.h + 8.0, w: panel.w, h: lines.len() as f32 * line_h + 2.0 * PADDING - 4.0 };
                    rects.push(UiRect::framed(kasten, theme::get(theme::Slot::BgPanel), 8.0, 1.0, theme::get(theme::Slot::Border1)));
                    for (i, line) in lines.into_iter().enumerate() {
                        texts.push(UiText { text: line, x: kasten.x + PADDING, y: kasten.y + PADDING + i as f32 * line_h, color: theme::text(), scale: UI_SCALE_SMALL });
                    }
                }
            }
            HudElement::Hotbar => {
                texts.push(UiText {
                    text: "Skalierung".to_string(),
                    x: HUD_CONFIG_LABEL_X,
                    y: row0_y,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
                let label = Self::scale_label(self.settings.hud_hotbar_scale, gpu.size());
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                texts.push(gpu.centered_text(value_rect, label, theme::text(), UI_SCALE_SMALL));
                {
                    let row1_y = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Haltbarkeit ab".to_string(), x: HUD_CONFIG_LABEL_X, y: row1_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let pct = self.settings.hotbar_durability_percent;
                    let text = if self.settings.hotbar_durability_absolute {
                        format!("\u{2264} {} Punkte", self.settings.hotbar_durability_points)
                    } else if pct >= 100 {
                        "immer".to_string()
                    } else {
                        format!("\u{2264} {pct} %")
                    };
                    let value_rect = stepper_value_rect(1);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, text, theme::text(), UI_SCALE_SMALL));
                    let row2_y = (settings_row_y(2) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Grenze".to_string(), x: HUD_CONFIG_LABEL_X, y: row2_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let row3_y = (settings_row_y(3) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Sauerstoff".to_string(), x: HUD_CONFIG_LABEL_X, y: row3_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let row4_y = (settings_row_y(4) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Farbe der Anzahl".to_string(), x: HUD_CONFIG_LABEL_X, y: row4_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let row5_y = (settings_row_y(HOTBAR_GLIDE_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Auswahl gleitet".to_string(), x: HUD_CONFIG_LABEL_X, y: row5_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    // Dauer in Millisekunden, 0 heisst aus (springt wie Vanilla).
                    let ms = self.settings.hotbar_glide_ms;
                    let value_rect = stepper_value_rect(HOTBAR_GLIDE_ROW);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, if ms == 0 { "Aus".to_string() } else { format!("{ms} ms") }, theme::text(), UI_SCALE_SMALL));
                }
            }
            HudElement::Title | HudElement::Subtitle | HudElement::ActionBar => {
                texts.push(UiText { text: "Ausrichtung".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                if element != HudElement::ActionBar {
                    let pct = if element == HudElement::Title { self.settings.hud_title_scale } else { self.settings.hud_subtitle_scale };
                    let row1_y = (settings_row_y(1) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row1_y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let value_rect = stepper_value_rect(1);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, format!("{pct} %"), theme::text(), UI_SCALE_SMALL));
                }
            }
            HudElement::BossBar => {
                texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                texts.push(gpu.centered_text(value_rect, format!("{} %", self.settings.hud_bossbar_scale), theme::text(), UI_SCALE_SMALL));
            }
            HudElement::Effects => {
                texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                texts.push(gpu.centered_text(value_rect, format!("{} %", self.settings.hud_effects_scale), theme::text(), UI_SCALE_SMALL));
                let mut zeilen = vec![(1, "Im Inventar"), (EFFECTS_HIDE_AMBIENT_ROW, "Ambiente verstecken"), (EFFECTS_STYLE_ROW, "Stil")];
                if self.settings.effects_detailed {
                    zeilen.extend([
                        (EFFECTS_ALIGN_ROW, "Ausrichtung"),
                        (EFFECTS_ICON_ROW, "Icon"),
                        (EFFECTS_NAME_ROW, "Name"),
                        (EFFECTS_TIMER_ROW, "Timer"),
                        (EFFECTS_TIMER_COLOR_ROW, "Timer-Farbe"),
                        (EFFECTS_BLINK_ROW, "Blinken ab 10 s"),
                    ]);
                }
                for (row, text) in zeilen {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
            }
            HudElement::Scoreboard | HudElement::HeldName | HudElement::Clock => {
                texts.push(UiText { text: "Größe".to_string(), x: HUD_CONFIG_LABEL_X, y: row0_y, color: theme::text(), scale: UI_SCALE_SMALL });
                let value_rect = stepper_value_rect(0);
                rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                let pct = match element {
                    HudElement::Scoreboard => self.settings.hud_scoreboard_scale,
                    HudElement::Clock => self.settings.hud_clock_scale,
                    _ => self.settings.hud_held_name_scale,
                };
                texts.push(gpu.centered_text(value_rect, format!("{pct} %"), theme::text(), UI_SCALE_SMALL));
                if element == HudElement::Scoreboard {
                    let y = (settings_row_y(SCOREBOARD_ALIGN_ROW) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: "Ausrichtung".to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
            }
            HudElement::ShieldCooldown | HudElement::Arrows => {
                let zeilen: &[&str] = if element == HudElement::Arrows {
                    &["Größe", "Nur Zahl", "Deckkraft", "Zahl-Farbe"]
                } else {
                    &["Farbe"]
                };
                for (row, text) in zeilen.iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                if element == HudElement::Arrows {
                    for (row, wert) in [(0, self.settings.hud_arrows_scale), (2, self.settings.arrow_opacity)] {
                        let value_rect = stepper_value_rect(row);
                        rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                        texts.push(gpu.centered_text(value_rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
                    }
                }
            }
            HudElement::Keystrokes | HudElement::Session => {
                let werte = if element == HudElement::Session {
                    [("Größe", self.settings.hud_session_scale), ("Deckkraft", self.settings.session_opacity)]
                } else {
                    [("Größe", self.settings.hud_keys_scale), ("Deckkraft", self.settings.keys_opacity)]
                };
                for (row, (text, wert)) in werte.into_iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                    let value_rect = stepper_value_rect(row);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, format!("{wert} %"), theme::text(), UI_SCALE_SMALL));
                }
            }
            HudElement::Armor => {
                for (row, text) in ["Größe", "Ausrichtung", "Rücken nach", "Abstand", "Zahl-Größe", "Zahl X", "Zahl Y", "Grün bei >87 %", "Mending-Ring"].into_iter().enumerate() {
                    let y = (settings_row_y(row) + (SETTINGS_CTRL_H - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                    texts.push(UiText { text: text.to_string(), x: HUD_CONFIG_LABEL_X, y, color: theme::text(), scale: UI_SCALE_SMALL });
                }
                let set = &self.settings;
                for (row, wert) in [(0, format!("{} %", set.hud_armor_scale)), (3, format!("{} px", set.armor_hud_gap)), (4, format!("{} %", set.armor_hud_number_scale)), (5, format!("{} px", set.armor_hud_number_x)), (6, format!("{} px", set.armor_hud_number_y))] {
                    let value_rect = stepper_value_rect(row);
                    rects.push(UiRect { rect: value_rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                    texts.push(gpu.centered_text(value_rect, wert, theme::text(), UI_SCALE_SMALL));
                }
            }
            // Die Performance-Anzeige hat keine eigenen Werte.
            HudElement::Perf => {}
        }
    }

    // Raster und alle Elemente des HUD-Editors, in Bildschirmkoordinaten (s. build_menu_ui).
    pub(crate) fn build_hud_editor(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        let (sw, sh) = gpu.size();
        // Duenne Rasterlinien (als schmale Rechtecke) -- Orientierung beim Ausrichten.
        let mut x = HUD_EDITOR_GRID_PX;
        while x < sw {
            rects.push(UiRect { rect: Rect { x, y: 0.0, w: 1.0, h: sh }, color: COLOR_GRID_LINE, ..UiRect::FLAT });
            x += HUD_EDITOR_GRID_PX;
        }
        let mut y = HUD_EDITOR_GRID_PX;
        while y < sh {
            rects.push(UiRect { rect: Rect { x: 0.0, y, w: sw, h: 1.0 }, color: COLOR_GRID_LINE, ..UiRect::FLAT });
            y += HUD_EDITOR_GRID_PX;
        }

        // Ist gerade ein Element im Zug -- fuer die aktive Griff-Farbe (s. hud_drag_offset).
        let dragging = self.hud_drag_offset.map(|(e, _, _)| e);
        let pad = 6.0;

        // Alle eingeschalteten Elemente, jedes mit Griff-Rahmen zum Anfassen.
        if self.hud_element_shown(HudElement::Perf) {
            let rect = self.hud_perf_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Perf) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            self.push_hud_perf_texts(gpu, COLOR_PERF, texts);
        }

        {
            // Griff-Rahmen um die tatsaechliche Position, plus ein "+" als Vorschau (immer
            // in COLOR_HUD -- der echte Kreuz-Pass laeuft nur in AppMode::World, hier reicht
            // eine einfache statische Andeutung).
            let ch_rect = self.hud_crosshair_rect(gpu);
            let farbe = if dragging == Some(HudElement::Crosshair) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE };
            rects.push(UiRect::rounded(ch_rect, farbe, ch_rect.w / 2.0));
            let mitte = ((sw / 2.0).round(), (sh / 2.0).round());
            if self.settings.crosshair_custom {
                push_pattern_rects(&self.settings.crosshair_pattern, mitte, 1.0, rects);
            } else {
                for (teil, kontur) in crosshair_parts(self.settings.crosshair_shape, self.settings.crosshair_size(), mitte, self.settings.crosshair_outline) {
                    rects.push(crosshair_part_rect(teil, if kontur { [0.0, 0.0, 0.0, 1.0] } else { COLOR_HUD }));
                }
            }
        }

        // Scoreboard: Griff-Rahmen + Platzhalter-Panel (echter Inhalt nur bei Verbindung, hier
        // reicht ein Kasten mit "Scoreboard" zum Positionieren).
        {
            let rect = self.hud_scoreboard_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Scoreboard) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            rects.push(UiRect { rect, color: COLOR_SCOREBOARD_BG, ..UiRect::FLAT });
            // Bei Verbindung der echte Inhalt, sonst ein Platzhalter-Titel. Im HUD-Editor
            // geht es um die LAGE -- deshalb einfarbig in der Griff-Farbe.
            let title: String = self
                .scoreboard_lines()
                .map(|(t, _)| t.iter().map(|s| s.text.as_str()).collect::<String>())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Scoreboard".to_string());
            let (scale, _, title_h, _) = self.scoreboard_metrics(gpu);
            let tw = gpu.measure_text(&title, scale);
            texts.push(UiText {
                text: title,
                x: (rect.x + (rect.w - tw) / 2.0).round(),
                y: (rect.y + (title_h - gpu.text_height(scale)) / 2.0).round(),
                color: COLOR_HUD,
                scale,
            });
        }

        // Schild-Cooldown: Griff-Rahmen + Beispielzahl. Im Spiel steht sie in Fadenkreuzfarbe
        // (Fadenkreuz-Pass), hier in der Griff-Farbe -- es geht um die Lage.
        if self.hud_element_shown(HudElement::ShieldCooldown) {
            let rect = self.hud_shield_cd_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::ShieldCooldown) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            // Beispielzahl in der allgemeinen Schrift -- es geht um die Lage, nicht um Schrift
            // und Farbe (die setzt im Spiel der Fadenkreuz-Pass).
            texts.push(UiText { text: "5.0".to_string(), x: rect.x, y: rect.y, color: COLOR_HUD, scale: SHIELD_CD_TEXT_SCALE });
        }

        // Pfeil-Anzeige: Griff-Rahmen + Beispiel (64 Pfeile) mit den eingestellten Optionen.
        if self.hud_element_shown(HudElement::Arrows) {
            let rect = self.hud_arrows_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Arrows) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            if let Some(pfeil) = world::item_registry::item_id_by_name("arrow") {
                self.push_arrow_indicator(gpu, &protocol::item::ItemStack::new(pfeil, 64), sprites, texts, true);
            }
        }

        // Ruestung: Griff-Rahmen um alle vier Zellen und die getragene Ruestung, ohne welche ein
        // Beispielsatz (einmal mit wenig Haltbarkeit, damit man die orange Zahl sieht).
        if self.hud_element_shown(HudElement::Armor) {
            let rect = self.hud_armor_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Armor) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let getragen = self.worn_armor();
            let teile = if getragen.iter().any(Option::is_some) {
                getragen.map(|t| t.cloned())
            } else {
                ["diamond_helmet", "diamond_chestplate", "diamond_leggings", "diamond_boots"].map(|name| {
                    let mut stack = protocol::item::ItemStack::new(world::item_registry::item_id_by_name(name)?, 1);
                    if name == "diamond_chestplate" {
                        stack.damage = Some(490);
                    }
                    Some(stack)
                })
            };
            self.begin_text_area(gpu, texts, ShadowArea::Hotbar);
            self.push_armor_hud(gpu, [teile[0].as_ref(), teile[1].as_ref(), teile[2].as_ref(), teile[3].as_ref()], [false; 4], rects, sprites, texts);
            Self::end_text_area(gpu, texts);
        }

        // Keystrokes: Griff-Rahmen und die Tasten selbst, wie im Spiel.
        if self.hud_element_shown(HudElement::Keystrokes) {
            let rect = self.hud_keys_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Keystrokes) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            self.push_keystrokes(gpu, rects, texts);
        }

        // Session-Info: Griff-Rahmen und die Tafel selbst, wie im Spiel.
        if self.hud_element_shown(HudElement::Session) {
            let rect = self.hud_session_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect { rect: frame, color: if dragging == Some(HudElement::Session) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE }, ..UiRect::FLAT });
            self.push_session_info(gpu, rects, texts);
        }

        // Uhr: Griff-Rahmen und die Uhr selbst, wie im Spiel.
        if self.hud_element_shown(HudElement::Clock) {
            let rect = self.hud_clock_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect { rect: frame, color: if dragging == Some(HudElement::Clock) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE }, ..UiRect::FLAT });
            self.push_clock(gpu, texts);
        }

        // Actionbar: Griff-Rahmen + Text-Vorschau (echter Text nur bei Verbindung).
        {
            let rect = self.hud_actionbar_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::ActionBar) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            // Im HUD-Editor geht es um die LAGE, nicht um die Farben -- deshalb einfarbig.
            let text: String = match self.action_bar.as_ref() {
                Some((spans, _)) => spans.iter().map(|s| s.text.as_str()).collect(),
                None => "Actionbar".to_string(),
            };
            let tw = gpu.measure_text(&text, UI_SCALE_NORMAL);
            let tx = align_left(align_anchor(rect.x, rect.w, self.settings.hud_actionbar_align), tw, self.settings.hud_actionbar_align);
            texts.push(UiText { text, x: tx.round(), y: rect.y, color: COLOR_HUD, scale: UI_SCALE_NORMAL });
        }

        // Titel und Untertitel: je ein Griff-Rahmen + Beispieltext in eigener Ausrichtung und
        // Groesse.
        for (element, subtitle) in [(HudElement::Title, false), (HudElement::Subtitle, true)] {
            let rect = self.hud_title_line_rect(gpu, subtitle);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect { rect: frame, color: if dragging == Some(element) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE }, ..UiRect::FLAT });
            let aktiv = self.title_text.is_some() || self.subtitle_text.is_some();
            let spans = if subtitle { &self.subtitle_text } else { &self.title_text };
            let text: String = match spans {
                Some(sp) => sp.iter().map(|t| t.text.as_str()).collect(),
                None if aktiv => String::new(),
                None => if subtitle { "Untertitel" } else { "Titel" }.to_string(),
            };
            let a = if subtitle { self.settings.hud_subtitle_align } else { self.settings.hud_title_align };
            let scale = self.title_scale(subtitle);
            let tw = gpu.measure_text(&text, scale);
            let x = align_left(align_anchor(rect.x, rect.w, a), tw, a).round();
            texts.push(UiText { text, x, y: rect.y, color: COLOR_HUD, scale });
        }

        // Itemname: Griff-Rahmen und der Name des gehaltenen Items bzw. ein Platzhalter.
        {
            let rect = self.hud_held_name_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::HeldName) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let g = self.held_name_scale(gpu);
            let text: String = match self.own_hand_stack(protocol::packets::HAND_MAIN) {
                Some(stack) => self.held_name_spans(stack).iter().map(|s| s.text.as_str()).collect(),
                None => "Itemname".to_string(),
            };
            let text = if text.is_empty() { "Itemname".to_string() } else { text };
            let tw = gpu.measure_text(&text, g);
            texts.push(UiText { text, x: (rect.x + (rect.w - tw) / 2.0).round(), y: rect.y, color: COLOR_HUD, scale: g });
        }

        // Bossbars: Griff-Rahmen und die echten Bars, ohne Verbindung eine Beispiel-Bar.
        {
            let rect = self.hud_bossbar_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::BossBar) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            if self.boss_bars.is_empty() {
                let beispiel = BossBarState {
                    uuid: [0; 16],
                    name: vec![protocol::nbt::TextSpan { text: "Bossbar".to_string(), ..Default::default() }],
                    shown: 0.6,
                    target: 0.6,
                    set_at: Instant::now(),
                    color: 0,
                    style: 0,
                };
                self.build_boss_bars(gpu, std::slice::from_ref(&beispiel), sprites, texts);
            } else {
                self.build_boss_bars(gpu, &self.boss_bars, sprites, texts);
            }
        }

        // Effekte: Griff-Rahmen, die echten Effekte oder Beispiele. Ist die HUD-Anzeige aus,
        // bleibt sie im Editor sichtbar (sonst liesse sie sich nie mehr setzen), dann blasser.
        {
            let rect = self.hud_effects_rect(gpu, true);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Effects) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let alpha = if self.settings.show_effects_hud { 1.0 } else { 0.4 };
            self.build_effect_hud(gpu, sprites, texts, true, alpha);
        }

        // Die Mittellinie, an der gezogene Elemente einrasten (s. update_hud_drag) -- heller,
        // solange das gezogene Element genau darauf sitzt.
        {
            let eingerastet = dragging.is_some_and(|e| {
                let r = self.hud_element_rect(gpu, e);
                (r.x + r.w / 2.0 - sw / 2.0).abs() < 1.0
            });
            let farbe = if eingerastet { [1.0, 0.85, 0.2, 0.9] } else { [1.0, 1.0, 1.0, 0.25] };
            rects.push(UiRect { rect: Rect { x: (sw / 2.0 - 1.0).round(), y: 0.0, w: 2.0, h: sh }, color: farbe, ..UiRect::FLAT });
        }

        // Chat: Griff-Rahmen um den ganzen Nachrichtenblock plus die letzten Zeilen als
        // Vorschau. Ohne Verbindung ist der Chat leer -- dann Platzhalterzeilen, sonst
        // haette man nichts zum Abschaetzen der Groesse.
        {
            let rect = self.hud_chat_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Chat) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            let scale = self.chat_scale();
            let line_h = self.chat_line_height(gpu);
            // Im HUD-Editor geht es um die LAGE des Blocks, nicht um den Inhalt -- deshalb hier
            // bewusst einfarbig in der Griff-Farbe, aber genauso gesetzt wie im Spiel.
            let lines: Vec<ChatLine> = if self.chat_messages.is_empty() {
                (1..=self.settings.chat_lines)
                    .map(|i| ChatLine { indent: 0.0, spans: vec![protocol::nbt::TextSpan { text: format!("Chat-Zeile {i}"), ..Default::default() }] })
                    .collect()
            } else {
                self.chat_wrapped_tail(gpu, rect.w - 4.0, scale, self.settings.chat_lines as usize)
            };
            let top = rect.y + rect.h - lines.len() as f32 * line_h;
            for (i, line) in lines.into_iter().enumerate() {
                let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                texts.push(UiText { text, x: rect.x + 2.0 + line.indent, y: top + i as f32 * line_h, color: COLOR_HUD, scale });
            }
        }

        // Hotbar: Griff-Rahmen plus das echte Vanilla-Sheet als Vorschau (samt
        // Auswahlrahmen), damit man beim Verschieben sieht, was man tatsaechlich verschiebt.
        {
            let rect = self.hud_hotbar_rect(gpu);
            let frame = Rect { x: rect.x - pad, y: rect.y - pad, w: rect.w + 2.0 * pad, h: rect.h + 2.0 * pad };
            rects.push(UiRect {
                rect: frame,
                color: if dragging == Some(HudElement::Hotbar) { COLOR_HUD_HANDLE_ACTIVE } else { COLOR_HUD_HANDLE },
                ..UiRect::FLAT
            });
            if let Some(sheet) = self.ui_atlas.rect("hud/hotbar") {
                sprites.push(UiSprite::quad(rect.x, rect.y, rect.w, rect.h, sheet.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
            }
            if let Some(selection) = self.ui_atlas.rect("hud/hotbar_selection") {
                let scale = self.hotbar_scale(gpu.size());
                let size = inventory::HOTBAR_SELECTION * scale;
                sprites.push(UiSprite::quad(
                    rect.x + (self.inventory.held_slot as f32 * 20.0 - 1.0) * scale,
                    rect.y - scale,
                    size,
                    size * (23.0 / 24.0),
                    selection.uv,
                    [1.0, 1.0, 1.0, 1.0],
                    SpriteAtlas::Ui,
                ));
            }
        }
    }

    // Gestorben-Screen: roter Vollbild-Schleier + Ueberschrift. Respawn/Disconnect kommen
    // aus buttons(). Vanilla-Anmutung (roter Ton), ohne die Todesnachricht zu parsen (die
    // steckt als Text-Component im Combat-Kill-Paket -- ein eigener NBT-Parser, kein
    // MVP-Ziel; das Set-Health-Paket, aus dem wir den Tod erkennen, traegt keinen Text).
    pub(crate) fn build_death_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: COLOR_DEATH_OVERLAY, ..UiRect::FLAT });
        // Ueberschrift, ueber den Buttons zentriert.
        let title = "Du bist gestorben";
        let tw = gpu.measure_text(title, UI_SCALE_TITLE);
        texts.push(UiText {
            text: title.to_string(),
            x: ((sw - tw) / 2.0).round().max(UI_MARGIN),
            y: 120.0,
            color: COLOR_DEATH_TITLE,
            scale: UI_SCALE_TITLE,
        });
    }

    // Connect-Screen-Inhalt: die Serverliste. Das Log steht im Launcher, Adressen gehen ueber
    // Quick Connect (Leiste unten), das Konto liegt in der Leiste oben.
    pub(crate) fn build_connect_screen(&self, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        self.build_server_panel(texts);
        self.build_recordings_panel(rects, texts);
    }

    // Server-Liste bzw. Editier-Formular rechts auf dem Connect-Screen. Die Klickflaechen
    // selbst kommen aus server_panel_buttons() -- hier nur Panel, Ueberschrift und die
    // Beschriftungen, die gpu.centered_text nicht hinbekommt (zweizeilige Kacheln,
    // linksbuendige Felder mit Caret).
    pub(crate) fn build_server_panel(&self, texts: &mut Vec<UiText>) {
        if self.servers.is_empty() {
            let hint = "Noch keine Server gespeichert. Unten auf \"Neuer Eintrag\".";
            texts.push(UiText { text: hint.to_string(), x: server_panel().x, y: server_list_top() + 8.0, color: theme::text_3(), scale: UI_SCALE_SMALL });
        }
    }

    // Die Karten der Serverliste (samt Suchfeld und Bildlaufleiste) als eigene Ebene: an der
    // Ober- und Unterkante der Liste hart abgeschnitten, darueber ist frei (wie in osu!), oben
    // mit dem Verlauf (s. ScrollFade). Unter den Leisten, ueber dem uebrigen Inhalt.
    pub(crate) fn build_server_list(&self, gpu: &GpuState) -> UiOverlayGroup {
        use theme::Slot;
        let g = theme::get;
        let panel = server_panel();
        let (mut rects, mut texts, mut sprites) = (Vec::new(), Vec::new(), Vec::new());
        let (cx, cy) = self.pointer();
        let (oben, unten) = (server_list_top(), server_list_bottom());
        let fade = ScrollFade::new(oben, unten, server_row_h(), self.server_scroll.stand, self.server_max_scroll());
        push_fade_marker(&mut rects, &mut texts, Some(fade));
        // Suchfeld an der Stelle der ersten Karte; ohne Treffer darunter ein Hinweis.
        if let Some(suche) = self.server_search.as_ref() {
            let r = server_card_rect(0, self.server_scroll.stand);
            if r.y + r.h > oben && r.y < unten {
                rects.push(UiRect::shadow(r, 8.0, 2.0, 8.0, 0.35));
                rects.push(UiRect::framed(r, g(Slot::SurfaceSunken), 8.0, 1.0, g(Slot::Accent)));
                let feld = Rect { x: r.x + 16.0, y: r.y, w: r.w - 32.0, h: r.h };
                self.push_text_field(gpu, suche, feld, self.server_search_typing(), &mut rects, &mut texts);
            }
            if self.server_search_hits().is_empty() {
                let y = server_card_rect(1, self.server_scroll.stand).y + 8.0;
                texts.push(UiText { text: "Keine Treffer".to_string(), x: panel.x + 16.0, y, color: theme::text_3(), scale: UI_SCALE_SMALL });
            }
        }
        // Was aus der Liste ragt, verdecken die Leisten: sie liegen auf dem Server-Screen als
        // eigene Ebene darueber (s. build_menu_ui).
        let icon = server_icon_px();
        for (i, karte) in self.server_cards() {
            let entry = &self.servers[i];
            let hover = karte.contains(cx, cy) && cy >= oben && cy <= unten;
            // Wie die Skin-Kacheln: die Flaeche hellt weich auf (s. hover_level, ohne Aufblitzen),
            // der Rand schaltet hart um.
            let k = self.hover_level(karte, hover).min(1.0);
            let fill = mix_color(g(Slot::Surface1), g(Slot::Surface2), k);
            let rand = if hover { g(Slot::Accent) } else { g(Slot::Border1) };
            rects.push(UiRect::shadow(karte, 8.0, 2.0, 8.0, 0.35));
            rects.push(UiRect::framed(karte, fill, 8.0, 1.0, rand));
            let inhalt_x = karte.x;
            // Icon des Servers (aus dem Status-Ping), ohne eins das Vanilla-Bild fuer
            // unbekannte Server (s. server_list_icon).
            let feld = crate::image_field(Rect { x: inhalt_x + 16.0, y: karte.y + 12.0, w: icon, h: icon });
            let status = self.server_status.get(&entry.address);
            // Das dunkle Feld nur ohne Bild (Ladepunkte): unter einem Icon schiene es durch dessen
            // durchsichtige Stellen, dort soll die Karte selbst zu sehen sein (Nutzer 2026-11-12).
            let bild = self.server_list_icon(&entry.address);
            if bild.is_none() {
                rects.push(UiRect::rounded(feld, g(Slot::SurfaceSunken), 6.0));
            }
            if let Some(slot) = bild {
                sprites.push(UiSprite::quad(feld.x, feld.y, feld.w, feld.h, server_icon_uv(slot), [1.0; 4], SpriteAtlas::ServerIcons));
            } else if let Some(e) = status.filter(|e| matches!(e.state, server_status::State::Pending)) {
                rects.extend(loading_dots(feld, e.since.elapsed().as_secs_f32(), g(Slot::Accent)));
            }
            // Mit Namen: der Name gross, darunter die MOTD in einer Zeile. Ohne: die MOTD auf
            // hoechstens zwei Zeilen umbrochen (wie Vanilla, das sie auf die Breite der Liste
            // setzt), mittig in der Karte. Farbig wie vom Server geschickt. Die Adresse steht
            // nur da, solange es weder Namen noch MOTD gibt (Ping laeuft noch oder der Server
            // ist offline).
            let rechts = karte.x + karte.w - 16.0;
            let zeile1 = (karte.y + 14.0).round();
            let zeile2 = (karte.y + 14.0 + gpu.text_height(UI_SCALE_NORMAL) + 6.0).round();
            let tx = feld.x + feld.w + 16.0;
            let breite = (rechts - tx).max(60.0);
            let mut motd: Vec<protocol::nbt::TextSpan> = Vec::new();
            if let Some(server_status::State::Ok(st)) = status.map(|e| &e.state) {
                for (n, z) in st.motd.iter().enumerate() {
                    if n > 0 {
                        motd.push(protocol::nbt::TextSpan { text: "\n".to_string(), ..Default::default() });
                    }
                    motd.extend(z.iter().cloned());
                }
            }
            // "Namen anzeigen" aus: mit MOTD wie ohne Namen, ohne MOTD bleibt der Name.
            let name = if entry.show_name || motd.is_empty() { entry.name.as_str() } else { "" };
            // Ohne Namen ist die MOTD das Einzige in der Karte: umbrochen wie in Vanilla (an
            // derselben Stelle, gemessen in Vanillas Breiten -- Server richten ihre MOTD
            // darauf aus), hoechstens zwei Zeilen, so gross wie sonst der Name, wenn beide
            // so passen, sonst klein. Unter einem Namen bleibt sie klein.
            let mut vanilla: Vec<Vec<protocol::nbt::TextSpan>> =
                wrap_spans_styled(&motd, MOTD_VANILLA_WIDTH, |t, stil| gpu.vanilla_width(t, stil), 0.0).into_iter().map(|l| l.spans).filter(|z| !z.is_empty()).take(2).collect();
            trim_motd_padding(&mut vanilla);
            vanilla.retain(|z| !z.is_empty());
            let passt_bei = |scale: f32| vanilla.iter().all(|z| span_row_width(gpu, z, scale) <= breite);
            let motd_scale = if name.is_empty() && passt_bei(UI_SCALE_NORMAL) { UI_SCALE_NORMAL } else { UI_SCALE_SMALL };
            let klein = gpu.text_height(motd_scale);
            let zeilen: Vec<Vec<protocol::nbt::TextSpan>> = if name.is_empty() && passt_bei(motd_scale) {
                vanilla
            } else {
                wrap_spans(&motd, breite, |t| gpu.measure_text(t, motd_scale), 0.0).into_iter().map(|l| l.spans).filter(|z| !z.is_empty()).collect()
            };
            let mut setzen: Vec<(f32, Vec<protocol::nbt::TextSpan>)> = Vec::new();
            if !name.is_empty() {
                texts.push(UiText { text: ellipsize(gpu, name, UI_SCALE_NORMAL, breite), x: tx, y: zeile1, color: theme::text(), scale: UI_SCALE_NORMAL });
                if let Some(erste) = zeilen.first() {
                    let mut z = erste.clone();
                    if zeilen.len() > 1 {
                        z.push(protocol::nbt::TextSpan { text: " ".to_string(), ..Default::default() });
                        z.extend(zeilen[1].iter().cloned());
                    }
                    setzen.push((zeile2, z));
                }
            } else if zeilen.is_empty() {
                texts.push(UiText { text: ellipsize(gpu, &entry.address, UI_SCALE_NORMAL, breite), x: tx, y: zeile1, color: theme::text(), scale: UI_SCALE_NORMAL });
            } else {
                let n = zeilen.len().min(2);
                let abstand = 6.0;
                let y0 = karte.y + (karte.h - n as f32 * klein - (n - 1) as f32 * abstand) / 2.0;
                // Wie Vanilla: was nach der zweiten Zeile kommt, faellt weg.
                for (k, z) in zeilen.iter().take(n).enumerate() {
                    setzen.push(((y0 + k as f32 * (klein + abstand)).round(), z.clone()));
                }
            }
            for (y, z) in setzen {
                let z = ellipsize_spans(gpu, &z, motd_scale, breite);
                push_styled_row(gpu, &z, tx, y, theme::text_2(), motd_scale, &mut texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
            }
        }
        // Gezogene Karte: weisse Linie in der Luecke, in der sie landen wuerde.
        if self.server_drag_active().is_some() {
            if let Some(platz) = self.server_drop_slot() {
                let y = (server_card_rect(platz, self.server_scroll.stand).y - SERVER_ROW_GAP / 2.0 - 1.0).round();
                if y >= oben - SERVER_ROW_GAP && y <= unten {
                    rects.push(UiRect::rounded(Rect { x: panel.x, y, w: panel.w, h: 2.0 }, theme::text(), 1.0));
                }
            }
        }
        // Bildlaufleiste links neben den Karten, sobald nicht alle passen. Zum Anfassen: unter
        // dem Zeiger und gefasst in der Akzentfarbe.
        // Die Leiste selbst blendet nicht aus.
        push_fade_marker(&mut rects, &mut texts, None);
        if let Some((spur, griff)) = self.server_scrollbar() {
            rects.push(UiRect::rounded(spur, g(Slot::Surface1), SERVER_BAR_W / 2.0));
            let aktiv = self.server_bar_grab.is_some() || self.server_bar_hit().is_some_and(|r| r.contains(cx, cy));
            rects.push(UiRect::rounded(griff, g(if aktiv { Slot::Accent } else { Slot::Border2 }), SERVER_BAR_W / 2.0));
        }
        let mut bilder = Vec::new();
        for s in sprites {
            fade_sprite(s, fade, &mut bilder);
        }
        let sw = gpu.size().0;
        UiOverlayGroup { rects, texts, sprites: bilder, pivot: (0.0, 0.0), angle: 0.0, clip: Some(Rect { x: 0.0, y: oben, w: sw, h: unten - oben }) }
    }

    // Einheitliche Konto-Statuszeile (Text + Farbe) fuer Connect- und Account-Screen.
    pub(crate) fn account_status_line(&self) -> (String, [f32; 4]) {
        match &self.credentials {
            Some(c) => (format!("Angemeldet als {}", c.username), [0.4, 1.0, 0.5, 1.0]),
            None if self.login_busy => ("Anmeldung läuft ...".to_string(), [1.0, 0.85, 0.3, 1.0]),
            None => ("Nicht angemeldet (Offline-Mode)".to_string(), [0.7, 0.7, 0.7, 1.0]),
        }
    }

    // Account-Screen: Konto-Status + Verwaltungs-Buttons (aus buttons()) + Beschriftungen,
    // das Eingabefeld (Token/Client-ID) und die Liste der Launcher-Konten. Aufgeraeumte
    // vertikale Spalte, nichts ueberlappt mehr.
    pub(crate) fn build_account_screen(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        // Ueberschrift oben (rechts neben dem Zurueck-Button).
        texts.push(UiText {
            text: "Konto".to_string(),
            x: BACK_BUTTON.x + BACK_BUTTON.w + 24.0,
            y: (BACK_BUTTON.y + 10.0).round(),
            color: theme::text(),
            scale: UI_SCALE_NORMAL,
        });
        // Statuszeile.
        let (status, color) = self.account_status_line();
        texts.push(UiText { text: status, x: 40.0, y: 88.0, color, scale: UI_SCALE_NORMAL });

        if self.credentials.is_some() {
            // Angemeldet: nur ein Hinweis (Abmelden-Button kommt aus buttons()).
            texts.push(UiText {
                text: "Zum Kontowechsel erst abmelden.".to_string(),
                x: 40.0,
                y: (account_row_rect(0).y + account_row_rect(0).h + 8.0).round(),
                color: [0.6, 0.6, 0.6, 1.0],
                scale: UI_SCALE_SMALL,
            });
            return;
        }

        // Eingabefeld (Token bzw. Refresh-Token), nur wenn aufgeklappt.
        if let Some((kind, value)) = &self.input_field {
            let label = match kind {
                InputKind::Token => "Minecraft-Token einfügen (Strg+V), dann OK/Enter:",
                InputKind::RefreshToken => "Microsoft-Refresh-Token einfügen (Strg+V), dann OK/Enter:",
                // Gehoert zur Configs-Seite (s. build_settings_configs).
                InputKind::ConfigName(_) => return,
            };
            texts.push(UiText {
                text: label.to_string(),
                x: ACCOUNT_INPUT_FIELD.x,
                y: (ACCOUNT_INPUT_FIELD.y - gpu.text_height(UI_SCALE_SMALL) - 4.0).round(),
                color: [0.7, 0.7, 0.7, 1.0],
                scale: UI_SCALE_SMALL,
            });
            rects.push(UiRect { rect: ACCOUNT_INPUT_FIELD, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
            let tx = ACCOUNT_INPUT_FIELD.x + 10.0;
            let ty = (ACCOUNT_INPUT_FIELD.y + (ACCOUNT_INPUT_FIELD.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            let shown = token_display(gpu, value, ACCOUNT_INPUT_FIELD.w - 20.0);
            push_typed_marker(texts);
            texts.push(UiText { text: shown.clone(), x: tx, y: ty, color: theme::text(), scale: UI_SCALE_SMALL });
            let caret_x = tx + gpu.measure_raw(&shown, UI_SCALE_SMALL);
            self.push_caret(rects, gpu.caret_rect(caret_x, ty, UI_SCALE_SMALL));
        }
    }
}
