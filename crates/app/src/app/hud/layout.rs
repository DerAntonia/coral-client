// Lage der HUD-Elemente, Scoreboard, Verschieben und Zuruecksetzen, Leistungsanzeige.

use super::*;

impl App {
    // Rechteck der Schild-Cooldown-Zahl, bemessen an einem Beispielwert ("5.0").
    pub(crate) fn hud_shield_cd_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let (w, h) = (gpu.measure_text("5.0", SHIELD_CD_TEXT_SCALE), gpu.text_height(SHIELD_CD_TEXT_SCALE));
        let cx = self.settings.hud_shield_cd_x * sw;
        let cy = self.settings.hud_shield_cd_y * sh;
        Rect { x: (cx - w / 2.0).round(), y: (cy - h / 2.0).round(), w, h }
    }

    // Bildschirm-Quadrat des Kreuz-Elements im HUD-Editor, um den (normierten) Mittelpunkt
    // herum -- anders als hud_perf_rect (linke obere Ecke), weil der Crosshair selbst ueber
    // seinen Mittelpunkt positioniert wird (s. crosshair_vertices). Der Griff ist der Kreis
    // darin (s. hud_element_at_cursor), mit CROSSHAIR_HANDLE_MARGIN Luft um das Kreuz.
    pub(crate) fn hud_crosshair_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let (cx, cy) = ((sw / 2.0).round(), (sh / 2.0).round());
        let half = if self.settings.crosshair_custom {
            // Um die gesetzten Pixel, mindestens so gross wie das eingebaute Kreuz.
            let (x0, y0, x1, y1) = self.settings.crosshair_pattern.bounds().unwrap_or((-5, -5, 5, 5));
            x0.min(y0).min(-5).abs().max(x1.max(y1).max(5)) as f32
        } else {
            // Mindestens so gross wie das Standard-Kreuz, damit auch ein kleiner Punkt greifbar
            // bleibt.
            ((self.settings.crosshair_size() as f32 + 2.0 * CROSSHAIR_OUTLINE_PX).max(CROSSHAIR_SHAPES[0].3 as f32) / 2.0).ceil()
        } + CROSSHAIR_HANDLE_MARGIN;
        Rect { x: cx - half, y: cy - half, w: 2.0 * half, h: 2.0 * half }
    }

    // Titel + Zeilentexte der Seitenleiste (nach Score absteigend), oder None wenn kein
    // Objective in Slot 1 haengt bzw. keine Zeilen da sind. Von build_server_hud UND der
    // Groessenberechnung (hud_scoreboard_rect) genutzt, damit Anzeige und Griff-Rechteck exakt
    // uebereinstimmen.
    // Die Seitenleiste, wie sie gezeichnet wird (eigener Name umgeschrieben, s. ego_name).
    #[allow(clippy::type_complexity)]
    pub(crate) fn scoreboard_lines_shown(&self) -> Option<(Vec<protocol::nbt::TextSpan>, Vec<Vec<protocol::nbt::TextSpan>>)> {
        let (title, lines) = self.scoreboard_lines()?;
        let neu = |z: Vec<protocol::nbt::TextSpan>| self.ego_rewrite(&z).unwrap_or(z);
        Some((neu(title), lines.into_iter().map(neu).collect()))
    }

    pub(crate) fn scoreboard_lines(&self) -> Option<(Vec<protocol::nbt::TextSpan>, Vec<Vec<protocol::nbt::TextSpan>>)> {
        let obj = self.sb_sidebar_objective.as_ref()?;
        let scores = self.sb_scores.get(obj)?;
        let title = self.sb_objective_titles.get(obj).cloned().unwrap_or_default();
        // Je Zeile: Punktzahl, Text und die geerbte Farbe -- die Teamfarbe des Holders
        // (Team.decorateName faerbt die ganze Zeile damit, eigene Farben darin gehen vor).
        let mut lines: Vec<(i32, String, Option<u32>)> = scores
            .iter()
            .map(|(h, (s, d))| if let Some(d) = d { (*s, d.clone(), None) } else {
                let farbe = self.sb_entry_team.get(h).and_then(|t| self.sb_teams.get(t)).and_then(|t| protocol::nbt::formatting_color(t.color));
                (*s, self.decorate_team_name(h), farbe)
            })
            .collect();
        lines.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        lines.truncate(15);
        // Die Paragraphen-Codes AUFLOESEN statt sie hinzuschreiben: der Holder einer
        // Seitenleisten-Zeile ist auf vielen Servern ein reiner Platzhalter wie "§0" (er muss
        // je Zeile eindeutig sein, soll aber nichts anzeigen), und Prefix/Suffix tragen die
        // Farben. Ungeparst stand genau das im Bild: "mcpvp.club§9".
        Some((
            protocol::nbt::legacy_to_spans(&title, None),
            lines.into_iter().map(|(_, t, farbe)| protocol::nbt::legacy_to_spans(&t, farbe)).collect(),
        ))
    }

    // Vanilla Team.decorateName: gehoert der Holder einem Team an, ist der sichtbare Text
    // Prefix + Holder + Suffix. Ohne Team bleibt es der nackte Holder-Name.
    pub(crate) fn decorate_team_name(&self, holder: &str) -> String {
        match self.sb_entry_team.get(holder).and_then(|t| self.sb_teams.get(t)) {
            // Das Reset vor dem Namen: Prefix, Name und Suffix sind in Vanilla Geschwister-
            // Components -- die Farbe des Prefix faerbt den Namen NICHT mit, er traegt die
            // Teamfarbe (die geerbte Farbe, s. scoreboard_lines).
            Some(team) => format!("{}\u{a7}r{holder}{}", team.prefix, team.suffix),
            None => holder.to_string(),
        }
    }

    // Masse der Scoreboard-Seitenleiste aus dem Inhalt; ohne Inhalt (z.B. im HUD-Editor ohne
    // Serververbindung) ein fester Platzhalter, damit man das Element trotzdem positionieren kann.
    pub(crate) fn hud_scoreboard_size(&self, gpu: &GpuState) -> (f32, f32) {
        let (scale, line_h, title_h, pad) = self.scoreboard_metrics(gpu);
        match self.scoreboard_lines_shown() {
            Some((title, lines)) => {
                // Die ganze Zeile auf einmal messen: nur so zaehlt der Platz, den eine
                // wechselnde Zahl darin reserviert hat (s. build_server_hud).
                let breite = |spans: &[protocol::nbt::TextSpan], scale: f32| -> f32 { stable_row_width(gpu, spans, scale) };
                let mut max_w = breite(&title, scale);
                for t in &lines {
                    max_w = max_w.max(breite(t, scale));
                }
                (max_w + pad * 2.0, title_h + lines.len() as f32 * line_h + pad)
            }
            None => (gpu.measure_text("Scoreboard", scale) + pad * 2.0, title_h + 3.0 * line_h + pad),
        }
    }

    // Schriftfaktor der Seitenleiste (Prozent aus dem HUD-Editor, auf Viertel gerundet wie
    // title_scale), Zeilen- und Titelhoehe und Rand -- Abstaende wachsen mit der Schrift.
    pub(crate) fn scoreboard_metrics(&self, gpu: &GpuState) -> (f32, f32, f32, f32) {
        let scale = ((UI_SCALE_SMALL * self.settings.hud_scoreboard_scale as f32 / 100.0) * 4.0).round().max(1.0) / 4.0;
        let f = scale / UI_SCALE_SMALL;
        (scale, gpu.text_height(scale) + (3.0 * f).round(), gpu.text_height(scale) + (6.0 * f).round(), (8.0 * f).round())
    }

    // Griff-/Zeichen-Rechteck der Seitenleiste: die feste Ecke (Settings::scoreboard_align)
    // normiert in hud_scoreboard_x/y, die Leiste waechst von ihr weg; auf den Bildschirm
    // geklemmt wie das Perf-Element (Default oben links, x nahe 1.0 -> rechtsbuendig).
    pub(crate) fn hud_scoreboard_rect(&self, gpu: &GpuState) -> Rect {
        let (w, h) = self.hud_scoreboard_size(gpu);
        let (sw, sh) = gpu.size();
        let set = &self.settings;
        let (x, y) = scoreboard_origin((set.hud_scoreboard_x * sw, set.hud_scoreboard_y * sh), w, h, set.scoreboard_align);
        Rect { x: x.min(sw - w).max(0.0).round(), y: y.min(sh - h).max(0.0).round(), w, h }
    }

    // Actionbar-Rechteck: Breite aus dem aktuellen Text (oder Platzhalter), obere linke Ecke
    // normiert + geklemmt wie das Perf-Element.
    pub(crate) fn hud_actionbar_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        // Die Breite ergibt sich aus dem GESAMTEN Text, unabhaengig davon, in wie viele
        // farbige Abschnitte er zerfaellt.
        let text: String = match self.action_bar.as_ref() {
            Some((spans, _)) => spans.iter().map(|s| s.text.as_str()).collect(),
            None => "Actionbar".to_string(),
        };
        let w = gpu.measure_text(&text, UI_SCALE_NORMAL).max(80.0);
        let h = gpu.text_height(UI_SCALE_NORMAL);
        let x = align_left(self.settings.hud_actionbar_x * sw, w, self.settings.hud_actionbar_align).min(sw - w).max(0.0).round();
        let y = (self.settings.hud_actionbar_y * sh).min(sh - h).max(0.0).round();
        Rect { x, y, w, h }
    }

    // Titel und Untertitel als ein Block: so breit wie die breitere Zeile (ohne Titel ein
    // Platzhalter), Anker und Ausrichtung aus dem HUD-Editor.
    // Schriftgroesse von Titel bzw. Untertitel: Prozent der Vorgabe, auf Viertel gerundet
    // (DM Sans liegt in Viertel-Fassungen vor und bleibt nur dann pixelgenau).
    pub(crate) fn title_scale(&self, subtitle: bool) -> f32 {
        let (basis, pct) = if subtitle {
            (UI_SCALE_NORMAL, self.settings.hud_subtitle_scale)
        } else {
            (UI_SCALE_TITLE, self.settings.hud_title_scale)
        };
        ((basis * pct as f32 / 100.0) * 4.0).round().max(1.0) / 4.0
    }

    // Griff-/Zeichen-Rechteck von Titel oder Untertitel -- jeder mit eigener Lage und
    // Ausrichtung. Ohne laufenden Titel misst er einen Platzhalter, sonst den echten Text
    // (fehlt einer der beiden, bleibt ein schmaler Griff stehen).
    pub(crate) fn hud_title_line_rect(&self, gpu: &GpuState, subtitle: bool) -> Rect {
        let (sw, sh) = gpu.size();
        let scale = self.title_scale(subtitle);
        let aktiv = self.title_text.is_some() || self.subtitle_text.is_some();
        let (spans, platzhalter) = if subtitle { (self.subtitle_text.as_ref(), "Untertitel") } else { (self.title_text.as_ref(), "Titel") };
        let breite = match spans {
            Some(sp) => sp.iter().map(|s| gpu.measure_text(&s.text, scale)).sum(),
            None if aktiv => 0.0,
            None => gpu.measure_text(platzhalter, scale),
        };
        let w: f32 = breite.max(40.0);
        let h = gpu.text_height(scale);
        let (x_norm, y_norm, a) = if subtitle {
            (self.settings.hud_subtitle_x, self.settings.hud_subtitle_y, self.settings.hud_subtitle_align)
        } else {
            (self.settings.hud_title_x, self.settings.hud_title_y, self.settings.hud_title_align)
        };
        let x = align_left(x_norm * sw, w, a).min(sw - w).max(0.0).round();
        let y = (y_norm * sh).min(sh - h).max(0.0).round();
        Rect { x, y, w, h }
    }

    pub(crate) fn hud_title_rect(&self, gpu: &GpuState) -> Rect {
        self.hud_title_line_rect(gpu, false)
    }

    pub(crate) fn hud_subtitle_rect(&self, gpu: &GpuState) -> Rect {
        self.hud_title_line_rect(gpu, true)
    }

    // Schriftfaktor der Spielerliste: Vanillas GUI-Skala (wie die Hotbar) mal eigener
    // Prozentwert aus dem Video-Tab.
    pub(crate) fn player_list_scale(&self, screen: (f32, f32)) -> f32 {
        self.hud_scale(screen) * self.settings.player_list_scale as f32 / 100.0
    }

    // Faktor der Bossbars: GUI-Skala mal eigener Prozentwert (im Standard Vanillas Groesse).
    pub(crate) fn bossbar_scale(&self, screen: (f32, f32)) -> f32 {
        self.hud_scale(screen) * self.settings.hud_bossbar_scale as f32 / 100.0
    }

    // Faktor der Effektanzeige: Hotbar-Skala (Vanillas GUI-Skala) mal eigener Prozentwert.
    pub(crate) fn effects_scale(&self, screen: (f32, f32)) -> f32 {
        self.hud_scale(screen) * self.settings.hud_effects_scale as f32 / 100.0
    }

    // Rechteck der Effektanzeige: rechte Kante und Oberkante aus den Einstellungen, Groesse aus
    // den Effekten (im Editor ohne echte Effekte aus Beispielen), so geklemmt, dass sie im Bild
    // bleibt.
    pub(crate) fn hud_effects_rect(&self, gpu: &GpuState, beispiel: bool) -> Rect {
        let liste = self.hud_effect_list(beispiel);
        let (sw, sh) = gpu.size();
        let g = self.effects_scale((sw, sh));
        if self.settings.effects_detailed {
            // Der feste Rand bleibt, wo er gespeichert ist (s. effects_detail_anchor).
            let _schrift = self.area_font_scope(gpu, ShadowArea::Effects);
            let (_, w, h) = self.effect_detail_layout(gpu, &liste, g);
            let set = &self.settings;
            let x = set.hud_effects_detail_x * sw - if set.effects_align == EFFECTS_ALIGN_RIGHT { w } else { 0.0 };
            let y = set.hud_effects_detail_y * sh - if set.effects_align == EFFECTS_ALIGN_BOTTOM { h } else { 0.0 };
            return Rect { x: x.clamp(0.0, (sw - w).max(0.0)).round(), y: y.clamp(0.0, (sh - h).max(0.0)).round(), w, h };
        }
        let (_, w, h) = effect_hud_layout(&liste);
        let (w, h) = (w * g, h * g);
        let rechts = (self.settings.hud_effects_x * sw).clamp(w.min(sw), sw);
        let oben = (self.settings.hud_effects_y * sh).clamp(0.0, (sh - h).max(0.0));
        Rect { x: (rechts - w).round(), y: oben.round(), w, h }
    }

    pub(crate) fn hud_element_rect(&self, gpu: &GpuState, element: HudElement) -> Rect {
        match element {
            HudElement::Perf => self.hud_perf_rect(gpu),
            HudElement::Crosshair => self.hud_crosshair_rect(gpu),
            HudElement::ActionBar => self.hud_actionbar_rect(gpu),
            HudElement::Scoreboard => self.hud_scoreboard_rect(gpu),
            HudElement::Hotbar => self.hud_hotbar_rect(gpu),
            HudElement::Chat => self.hud_chat_rect(gpu),
            HudElement::ShieldCooldown => self.hud_shield_cd_rect(gpu),
            HudElement::Arrows => self.hud_arrows_rect(gpu),
            HudElement::Title => self.hud_title_rect(gpu),
            HudElement::Subtitle => self.hud_subtitle_rect(gpu),
            HudElement::Effects => self.hud_effects_rect(gpu, true),
            HudElement::BossBar => self.hud_bossbar_rect(gpu),
            HudElement::HeldName => self.hud_held_name_rect(gpu),
            HudElement::Armor => self.hud_armor_rect(gpu),
            HudElement::Keystrokes => self.hud_keys_rect(gpu),
            HudElement::Session => self.hud_session_rect(gpu),
            HudElement::Clock => self.hud_clock_rect(gpu),
        }
    }

    // Steht das Element im HUD-Editor? Nur, was im Spiel auch eingeschaltet ist -- die
    // Schalter dafuer sitzen in den Modulen (Schilde, Pfeile, Debug > FPS-Anzeige). Der
    // Schild-Cooldown als Kreis gehoert zum Fadenkreuz und steht nicht extra da.
    pub(crate) const fn hud_element_shown(&self, element: HudElement) -> bool {
        let set = &self.settings;
        match element {
            HudElement::Perf => set.show_hud,
            HudElement::ShieldCooldown => set.module_shields && set.shield_cd_self && set.shield_cd_self_mode == 0,
            HudElement::Arrows => set.module_arrows && set.arrow_indicator,
            HudElement::Armor => set.module_armor_hud,
            HudElement::Keystrokes => set.module_keystrokes,
            HudElement::Session => set.module_session_info,
            HudElement::Clock => set.module_clock,
            _ => true,
        }
    }

    // HUD-Editor: liegt der Cursor auf einem der verschiebbaren Elemente (inkl. Griff-Rand)?
    // Kreuz zuerst geprueft -- es ist meist das kleinere Ziel und liegt oft mittig, waehrend
    // das Perf-Element normalerweise an einem Rand sitzt (selten ueberlappend).
    pub(crate) fn hud_element_at_cursor(&self) -> Option<HudElement> {
        let gpu = self.gpu.as_ref()?;
        let pad = 6.0;
        let (cx, cy) = self.cursor_pos;
        let hits = |r: Rect| cx >= r.x - pad && cx <= r.x + r.w + pad && cy >= r.y - pad && cy <= r.y + r.h + pad;
        let kreuz = self.hud_crosshair_rect(gpu);
        let r = kreuz.w / 2.0;
        if (cx - kreuz.x - r).powi(2) + (cy - kreuz.y - r).powi(2) <= r * r {
            Some(HudElement::Crosshair)
        } else if self.hud_element_shown(HudElement::ShieldCooldown) && hits(self.hud_shield_cd_rect(gpu)) {
            Some(HudElement::ShieldCooldown)
        } else if self.hud_element_shown(HudElement::Arrows) && hits(self.hud_arrows_rect(gpu)) {
            Some(HudElement::Arrows)
        } else if self.hud_element_shown(HudElement::Armor) && hits(self.hud_armor_rect(gpu)) {
            Some(HudElement::Armor)
        } else if self.hud_element_shown(HudElement::Keystrokes) && hits(self.hud_keys_rect(gpu)) {
            Some(HudElement::Keystrokes)
        } else if self.hud_element_shown(HudElement::Session) && hits(self.hud_session_rect(gpu)) {
            Some(HudElement::Session)
        } else if self.hud_element_shown(HudElement::Clock) && hits(self.hud_clock_rect(gpu)) {
            Some(HudElement::Clock)
        } else if self.hud_element_shown(HudElement::Perf) && hits(self.hud_perf_rect(gpu)) {
            Some(HudElement::Perf)
        } else if hits(self.hud_scoreboard_rect(gpu)) {
            Some(HudElement::Scoreboard)
        } else if hits(self.hud_held_name_rect(gpu)) {
            Some(HudElement::HeldName)
        } else if hits(self.hud_bossbar_rect(gpu)) {
            Some(HudElement::BossBar)
        } else if hits(self.hud_title_rect(gpu)) {
            Some(HudElement::Title)
        } else if hits(self.hud_subtitle_rect(gpu)) {
            Some(HudElement::Subtitle)
        } else if hits(self.hud_actionbar_rect(gpu)) {
            Some(HudElement::ActionBar)
        } else if hits(self.hud_hotbar_rect(gpu)) {
            Some(HudElement::Hotbar)
        } else if hits(self.hud_effects_rect(gpu, true)) {
            Some(HudElement::Effects)
        } else if hits(self.hud_chat_rect(gpu)) {
            // Zuletzt geprueft: der Chatblock ist die groesste Flaeche und wuerde sonst
            // kleinere Elemente ueber sich verschlucken.
            Some(HudElement::Chat)
        } else {
            None
        }
    }

    // Drag beginnen: Versatz Cursor->Elementecke (bzw. -Mittelpunkt beim Kreuz) merken,
    // damit das Element unter dem Cursor bleibt und nicht mit der Ecke anspringt. Nur
    // ohne offene Element-Einstellungen aufgerufen (s. window_event).
    pub(crate) fn begin_hud_drag(&mut self, element: HudElement) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let r = self.hud_element_rect(gpu, element);
        let (cx, cy) = self.cursor_pos;
        self.hud_drag_offset = Some((element, cx - r.x, cy - r.y));
    }

    // Setzt Position (und bei Bedarf weitere Werte) des ausgewaehlten Elements auf den
    // Standard zurueck -- direkt aus Settings::default() gelesen, damit hier keine zweite
    // Kopie der Standardwerte gepflegt werden muss (s. dortigen Default-Impl-Kommentar).
    pub(crate) fn reset_hud_element(&mut self, element: HudElement) {
        let defaults = Settings::default();
        match element {
            HudElement::Perf => {
                self.settings.hud_perf_x = defaults.hud_perf_x;
                self.settings.hud_perf_y = defaults.hud_perf_y;
            }
            HudElement::Crosshair => {
                self.settings.crosshair_color_mode = defaults.crosshair_color_mode;
                (self.settings.crosshair_hue, self.settings.crosshair_saturation, self.settings.crosshair_value) =
                    (defaults.crosshair_hue, defaults.crosshair_saturation, defaults.crosshair_value);
                self.settings.crosshair_brightness_boost = defaults.crosshair_brightness_boost;
                self.settings.crosshair_outline = defaults.crosshair_outline;
                self.settings.crosshair_ring_radius = defaults.crosshair_ring_radius;
                self.settings.crosshair_shape = defaults.crosshair_shape;
                self.settings.crosshair_sizes = defaults.crosshair_sizes;
                self.settings.crosshair_custom = defaults.crosshair_custom;
            }
            HudElement::ActionBar => {
                self.settings.hud_actionbar_x = defaults.hud_actionbar_x;
                self.settings.hud_actionbar_y = defaults.hud_actionbar_y;
                self.settings.hud_actionbar_align = defaults.hud_actionbar_align;
            }
            HudElement::Title => {
                self.settings.hud_title_x = defaults.hud_title_x;
                self.settings.hud_title_y = defaults.hud_title_y;
                self.settings.hud_title_align = defaults.hud_title_align;
                self.settings.hud_title_scale = defaults.hud_title_scale;
            }
            HudElement::Subtitle => {
                self.settings.hud_subtitle_x = defaults.hud_subtitle_x;
                self.settings.hud_subtitle_y = defaults.hud_subtitle_y;
                self.settings.hud_subtitle_align = defaults.hud_subtitle_align;
                self.settings.hud_subtitle_scale = defaults.hud_subtitle_scale;
            }
            HudElement::Effects => {
                self.settings.hud_effects_x = defaults.hud_effects_x;
                self.settings.hud_effects_y = defaults.hud_effects_y;
                self.settings.hud_effects_scale = defaults.hud_effects_scale;
                self.settings.show_effects_hud = defaults.show_effects_hud;
                self.settings.show_effects_inventory = defaults.show_effects_inventory;
                self.settings.effects_detailed = defaults.effects_detailed;
                self.settings.effects_hide_ambient = defaults.effects_hide_ambient;
                self.settings.hud_effects_detail_x = defaults.hud_effects_detail_x;
                self.settings.hud_effects_detail_y = defaults.hud_effects_detail_y;
                self.settings.effects_align = defaults.effects_align;
                self.settings.effects_icon = defaults.effects_icon;
                self.settings.effects_name = defaults.effects_name;
                self.settings.effects_timer_blink = defaults.effects_timer_blink;
                self.settings.effects_name_color = defaults.effects_name_color;
                self.settings.effects_timer_style = defaults.effects_timer_style;
                self.settings.effects_timer_color = defaults.effects_timer_color;
            }
            HudElement::BossBar => {
                self.settings.hud_bossbar_x = defaults.hud_bossbar_x;
                self.settings.hud_bossbar_y = defaults.hud_bossbar_y;
                self.settings.hud_bossbar_scale = defaults.hud_bossbar_scale;
            }
            HudElement::HeldName => {
                self.settings.hud_held_name_x = defaults.hud_held_name_x;
                self.settings.hud_held_name_y = defaults.hud_held_name_y;
                self.settings.hud_held_name_scale = defaults.hud_held_name_scale;
            }
            HudElement::Scoreboard => {
                self.settings.hud_scoreboard_x = defaults.hud_scoreboard_x;
                self.settings.hud_scoreboard_y = defaults.hud_scoreboard_y;
                self.settings.scoreboard_align = defaults.scoreboard_align;
                self.settings.hud_scoreboard_scale = defaults.hud_scoreboard_scale;
            }
            HudElement::Hotbar => {
                self.settings.hud_hotbar_x = defaults.hud_hotbar_x;
                self.settings.hud_hotbar_y = defaults.hud_hotbar_y;
                self.settings.hud_hotbar_scale = defaults.hud_hotbar_scale;
                self.settings.hotbar_durability_percent = defaults.hotbar_durability_percent;
                self.settings.hotbar_durability_absolute = defaults.hotbar_durability_absolute;
                self.settings.hotbar_durability_points = defaults.hotbar_durability_points;
                self.settings.hotbar_air_bar = defaults.hotbar_air_bar;
                self.settings.hotbar_glide_ms = defaults.hotbar_glide_ms;
            }
            HudElement::Chat => {
                self.settings.hud_chat_x = defaults.hud_chat_x;
                self.settings.hud_chat_y = defaults.hud_chat_y;
                self.settings.chat_scale_percent = defaults.chat_scale_percent;
                self.settings.chat_width = defaults.chat_width;
                self.settings.chat_background = defaults.chat_background;
                self.settings.chat_open_dim = defaults.chat_open_dim;
                self.settings.chat_lines = defaults.chat_lines;
                self.settings.chat_lines_open = defaults.chat_lines_open;
                self.settings.chat_fade_secs = defaults.chat_fade_secs;
                self.settings.chat_line_spacing = defaults.chat_line_spacing;
                self.settings.chat_bedrock = defaults.chat_bedrock;
            }
            HudElement::ShieldCooldown => {
                self.settings.hud_shield_cd_x = defaults.hud_shield_cd_x;
                self.settings.hud_shield_cd_y = defaults.hud_shield_cd_y;
                self.settings.shield_cd_text_style = defaults.shield_cd_text_style;
            }
            HudElement::Keystrokes => {
                self.settings.hud_keys_x = defaults.hud_keys_x;
                self.settings.hud_keys_y = defaults.hud_keys_y;
                self.settings.hud_keys_scale = defaults.hud_keys_scale;
                self.settings.keys_opacity = defaults.keys_opacity;
            }
            HudElement::Session => {
                self.settings.hud_session_x = defaults.hud_session_x;
                self.settings.hud_session_y = defaults.hud_session_y;
                self.settings.hud_session_scale = defaults.hud_session_scale;
                self.settings.session_opacity = defaults.session_opacity;
            }
            HudElement::Clock => {
                self.settings.hud_clock_x = defaults.hud_clock_x;
                self.settings.hud_clock_y = defaults.hud_clock_y;
                self.settings.hud_clock_scale = defaults.hud_clock_scale;
            }
            HudElement::Armor => {
                self.settings.hud_armor_x = defaults.hud_armor_x;
                self.settings.hud_armor_y = defaults.hud_armor_y;
                self.settings.hud_armor_scale = defaults.hud_armor_scale;
                self.settings.armor_hud_layout = defaults.armor_hud_layout;
                self.settings.armor_hud_sort_vertical = defaults.armor_hud_sort_vertical;
                self.settings.armor_hud_sort_flat = defaults.armor_hud_sort_flat;
                self.settings.armor_hud_green = defaults.armor_hud_green;
                self.settings.armor_hud_ring = defaults.armor_hud_ring;
                self.settings.armor_hud_gap = defaults.armor_hud_gap;
                self.settings.armor_hud_number_scale = defaults.armor_hud_number_scale;
                self.settings.armor_hud_number_x = defaults.armor_hud_number_x;
                self.settings.armor_hud_number_y = defaults.armor_hud_number_y;
            }
            HudElement::Arrows => {
                self.settings.hud_arrows_x = defaults.hud_arrows_x;
                self.settings.hud_arrows_y = defaults.hud_arrows_y;
                self.settings.hud_arrows_scale = defaults.hud_arrows_scale;
                self.settings.arrow_number_only = defaults.arrow_number_only;
                self.settings.arrow_opacity = defaults.arrow_opacity;
                self.settings.arrow_text_style = defaults.arrow_text_style;
            }
        }
    }

    // Waehrend eines Drags die (normierte) Elementposition der Cursorbewegung nachfuehren.
    pub(crate) fn update_hud_drag(&mut self) {
        let Some((element, ox, oy)) = self.hud_drag_offset else {
            return;
        };
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (sw, sh) = gpu.size();
        let (mut cx, cy) = self.cursor_pos;
        // Einrasten an der senkrechten Mittellinie: liegt die Mitte des Elements nah genug
        // daran, wird der Zeiger so versetzt, dass sie genau darauf liegt. Alles Weitere
        // rechnet dann mit diesem Zeiger -- fuer jedes Element gleich.
        let w = self.hud_element_rect(gpu, element).w;
        let mitte = cx - ox + w / 2.0;
        if (mitte - sw / 2.0).abs() < HUD_SNAP_PX {
            cx += sw / 2.0 - mitte;
        }
        match element {
            HudElement::Title => {
                self.settings.hud_title_x = (align_anchor(cx - ox, w, self.settings.hud_title_align) / sw).clamp(0.0, 1.0);
                self.settings.hud_title_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Subtitle => {
                self.settings.hud_subtitle_x = (align_anchor(cx - ox, w, self.settings.hud_subtitle_align) / sw).clamp(0.0, 1.0);
                self.settings.hud_subtitle_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Effects if self.settings.effects_detailed => {
                let r = Rect { x: cx - ox, y: cy - oy, ..self.hud_effects_rect(gpu, true) };
                let (x, y) = effects_detail_anchor(r, self.settings.effects_align);
                self.settings.hud_effects_detail_x = (x / sw).clamp(0.0, 1.0);
                self.settings.hud_effects_detail_y = (y / sh).clamp(0.0, 1.0);
            }
            HudElement::Effects => {
                // Gespeichert werden RECHTE Kante und Oberkante (s. hud_effects_rect).
                self.settings.hud_effects_x = ((cx - ox + w) / sw).clamp(0.0, 1.0);
                self.settings.hud_effects_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Perf => {
                self.settings.hud_perf_x = ((cx - ox) / sw).clamp(0.0, 1.0);
                self.settings.hud_perf_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            // Das Kreuz bleibt, wo es ist -- greifen oeffnet nur seine Einstellungen.
            HudElement::Crosshair => {}
            HudElement::ActionBar => {
                self.settings.hud_actionbar_x = (align_anchor(cx - ox, w, self.settings.hud_actionbar_align) / sw).clamp(0.0, 1.0);
                self.settings.hud_actionbar_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::HeldName => {
                // Gespeichert werden Mitte und Oberkante -- ab dem ersten Ziehen fest, nicht
                // mehr an der Hotbar.
                self.settings.hud_held_name_x = ((cx - ox + w / 2.0) / sw).clamp(0.0, 1.0);
                self.settings.hud_held_name_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Scoreboard => {
                let h = self.hud_scoreboard_rect(gpu).h;
                let (x, y) = scoreboard_anchor(Rect { x: cx - ox, y: cy - oy, w, h }, self.settings.scoreboard_align);
                self.settings.hud_scoreboard_x = (x / sw).clamp(0.0, 1.0);
                self.settings.hud_scoreboard_y = (y / sh).clamp(0.0, 1.0);
            }
            HudElement::BossBar => {
                // Gespeichert werden MITTE und Oberkante (s. hud_bossbar_rect).
                self.settings.hud_bossbar_x = ((cx - ox + w / 2.0) / sw).clamp(0.0, 1.0);
                self.settings.hud_bossbar_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Hotbar => {
                // ox/oy sind relativ zur linken OBEREN Ecke; gespeichert werden Mitte und
                // Unterkante (s. Settings::hud_hotbar_x/y).
                let rect = self.hud_hotbar_rect(gpu);
                self.settings.hud_hotbar_x = ((cx - ox + rect.w / 2.0) / sw).clamp(0.0, 1.0);
                self.settings.hud_hotbar_y = ((cy - oy + rect.h) / sh).clamp(0.0, 1.0);
            }
            HudElement::Chat => {
                // Gespeichert werden linke Kante und UNTERKANTE (s. hud_chat_rect).
                let rect = self.hud_chat_rect(gpu);
                self.settings.hud_chat_x = ((cx - ox) / sw).clamp(0.0, 1.0);
                self.settings.hud_chat_y = ((cy - oy + rect.h) / sh).clamp(0.0, 1.0);
            }
            HudElement::ShieldCooldown => {
                // Gespeichert wird die MITTE (s. hud_shield_cd_rect).
                let rect = self.hud_shield_cd_rect(gpu);
                self.settings.hud_shield_cd_x = ((cx - ox + rect.w / 2.0) / sw).clamp(0.0, 1.0);
                self.settings.hud_shield_cd_y = ((cy - oy + rect.h / 2.0) / sh).clamp(0.0, 1.0);
            }
            HudElement::Keystrokes => {
                self.settings.hud_keys_x = ((cx - ox) / sw).clamp(0.0, 1.0);
                self.settings.hud_keys_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Session => {
                self.settings.hud_session_x = ((cx - ox) / sw).clamp(0.0, 1.0);
                self.settings.hud_session_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Clock => {
                self.settings.hud_clock_x = ((cx - ox) / sw).clamp(0.0, 1.0);
                self.settings.hud_clock_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Armor => {
                self.settings.hud_armor_x = ((cx - ox) / sw).clamp(0.0, 1.0);
                self.settings.hud_armor_y = ((cy - oy) / sh).clamp(0.0, 1.0);
            }
            HudElement::Arrows => {
                // Gespeichert wird die MITTE (s. hud_arrows_rect).
                let rect = self.hud_arrows_rect(gpu);
                self.settings.hud_arrows_x = ((cx - ox + rect.w / 2.0) / sw).clamp(0.0, 1.0);
                self.settings.hud_arrows_y = ((cy - oy + rect.h / 2.0) / sh).clamp(0.0, 1.0);
            }
        }
    }

    // Die Felder des Performance-HUD-Elements, je mit einer Vorlage fuer die Breite, die das
    // Feld mindestens bekommt (vier Stellen FPS): die Zahlen stehen rechtsbuendig und springen
    // nicht. "work": die laengste Arbeitszeit eines Frames im Fenster (s. HUD_PERF_WINDOW)
    // gegen die Zeit, die ein Frame hoechstens haben darf, ohne dass einer ausfaellt (bei 120
    // Hz 8,33 ms; ohne Deckel der Monitor).
    pub(crate) fn hud_perf_fields(&self) -> [(&'static str, String, &'static str); 2] {
        let frame_ms = self.profiler_target_interval().unwrap_or(self.monitor_frame_interval).as_secs_f32() * 1000.0;
        [
            ("perf.fps", format!("{} FPS", self.frame_stats.fps()), "8888 FPS"),
            ("perf.work", format!("work {:.1}/{:.2} ms", self.frame_stats.max_ms(), frame_ms), "work 88.8/88.88 ms"),
        ]
    }

    // EINRASTEN: die Breite eines wechselnden Textes waechst mit, schrumpft aber nie wieder.
    // Ausgerichteter Text (rechtsbuendig, mittig) sprang sonst bei jeder Zahl, die schmaler
    // oder breiter wurde -- etwa an der Performance-Anzeige (119 -> 120 FPS). Mit der
    // eingerasteten Breite steht die Anzeige, sobald sie einmal ihre groesste Form hatte.
    // Zurueckgesetzt wird nur beim Schriftwechsel (s. App::set_font_slot).
    pub(crate) fn latched_width(&self, key: &'static str, width: f32) -> f32 {
        latch_width(&mut self.hud_latched_widths.borrow_mut(), key, width)
    }

    // Rechteck und Textstuecke des Perf-Elements. Jedes Feld steht in einer eigenen,
    // eingerasteten Spalte (s. latched_width): so bleiben nicht nur die Gesamtbreite, sondern
    // auch die Trennstriche an ihrem Platz, wenn sich eine Zahl im Feld davor aendert.
    //
    // Die Position kommt aus der (normierten) HUD-Position und wird auf den Bildschirm
    // geklemmt, damit das Element nie halb hinausragt -- das macht die Default-Position
    // (x=0.98) automatisch rechtsbuendig. Auch die Drag-Trefferpruefung im Editor nutzt
    // genau dieses Rechteck.
    pub(crate) fn hud_perf_layout(&self, gpu: &GpuState) -> (Rect, Vec<(String, f32)>) {
        const SEPARATOR: &str = "  |  ";
        let sep_w = gpu.measure_text(SEPARATOR, UI_SCALE_SMALL);
        let fields = self.hud_perf_fields();
        let columns: Vec<f32> = fields
            .iter()
            .map(|(key, text, vorlage)| self.latched_width(key, gpu.measure_text(text, UI_SCALE_SMALL).max(gpu.measure_text(vorlage, UI_SCALE_SMALL))))
            .collect();
        let w = columns.iter().sum::<f32>() + sep_w * (fields.len() - 1) as f32;
        let h = gpu.text_height(UI_SCALE_SMALL);
        let (sw, sh) = gpu.size();
        let x = (self.settings.hud_perf_x * sw).min(sw - w).max(0.0).round();
        let y = (self.settings.hud_perf_y * sh).min(sh - h).max(0.0).round();
        let mut pieces = Vec::with_capacity(fields.len() * 2);
        let mut pen = x;
        for (i, ((_, text, _), col)) in fields.into_iter().zip(columns).enumerate() {
            if i > 0 {
                pieces.push((SEPARATOR.to_string(), pen));
                pen += sep_w;
            }
            // Das erste Feld rechtsbuendig in seiner Spalte (die Einheit bleibt stehen, nur die
            // Zahl waechst nach links), die folgenden linksbuendig direkt hinter dem Strich --
            // rechtsbuendig stand dort die Luecke der breiteren Vorlage.
            let text_w = gpu.measure_text(&text, UI_SCALE_SMALL);
            pieces.push((text, if i == 0 { pen + col - text_w } else { pen }));
            pen += col;
        }
        (Rect { x, y, w, h }, pieces)
    }

    pub(crate) fn hud_perf_rect(&self, gpu: &GpuState) -> Rect {
        let _schrift = self.area_font_scope(gpu, ShadowArea::Fps);
        self.hud_perf_layout(gpu).0
    }

    pub(crate) fn push_hud_perf_texts(&self, gpu: &GpuState, color: [f32; 4], texts: &mut Vec<UiText>) {
        self.begin_text_area(gpu, texts, ShadowArea::Fps);
        let (rect, pieces) = self.hud_perf_layout(gpu);
        let fps_farbe = if color == COLOR_PERF { self.perf_fps_color() } else { color };
        for (i, (text, x)) in pieces.into_iter().enumerate() {
            texts.push(UiText { text, x: x.round(), y: rect.y, color: if i == 0 { fps_farbe } else { color }, scale: UI_SCALE_SMALL });
        }
        Self::end_text_area(gpu, texts);
    }

    // Die FPS-Zahl: gruen, solange sie mindestens das FPS-Limit (bzw. die Bildrate des
    // Monitors) minus 1 erreicht (Nutzer 2026-11-11), darunter weiss und zunehmend orange --
    // ganz orange beim halben Limit. Ohne Limit immer weiss.
    pub(crate) fn perf_fps_color(&self) -> [f32; 4] {
        let selbst = self.gpu.as_ref().is_some_and(super::super::super::GpuState::present_mode_self_paces);
        if self.settings.fps_limit == FPS_LIMIT_UNLIMITED && !selbst {
            return COLOR_PERF;
        }
        let ziel = 1.0 / self.profiler_target_interval().unwrap_or(self.monitor_frame_interval).as_secs_f32().max(1e-6);
        if self.frame_stats.fps() as f32 >= ziel.round() - 1.0 {
            return theme::get(theme::Slot::Success);
        }
        let t = ((ziel - self.frame_stats.fps() as f32) / (ziel / 2.0)).clamp(0.0, 1.0);
        let orange = [1.0, 0.55, 0.1, 1.0];
        [0, 1, 2, 3].map(|k| COLOR_PERF[k] + (orange[k] - COLOR_PERF[k]) * t)
    }
}
