// HUD: Positionen und Verschieben der Elemente, Hotbar, Leisten, Effekte, Tab-Liste, Session-Info.

use crate::*;

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
            .map(|(h, (s, d))| match d {
                Some(d) => (*s, d.clone(), None),
                None => {
                    let farbe = self.sb_entry_team.get(h).and_then(|t| self.sb_teams.get(t)).and_then(|t| protocol::nbt::formatting_color(t.color));
                    (*s, self.decorate_team_name(h), farbe)
                }
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

    // Griff-/Zeichen-Rechteck der Seitenleiste: obere linke Ecke normiert (hud_scoreboard_x/y),
    // auf den Bildschirm geklemmt wie das Perf-Element (Default x nahe 1.0 -> rechtsbuendig).
    pub(crate) fn hud_scoreboard_rect(&self, gpu: &GpuState) -> Rect {
        let (w, h) = self.hud_scoreboard_size(gpu);
        let (sw, sh) = gpu.size();
        let x = (self.settings.hud_scoreboard_x * sw).min(sw - w).max(0.0).round();
        let y = (self.settings.hud_scoreboard_y * sh).min(sh - h).max(0.0).round();
        Rect { x, y, w, h }
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
        }
    }

    // Steht das Element im HUD-Editor? Nur, was im Spiel auch eingeschaltet ist -- die
    // Schalter dafuer sitzen in den Modulen (Schilde, Pfeile, Debug > FPS-Anzeige). Der
    // Schild-Cooldown als Kreis gehoert zum Fadenkreuz und steht nicht extra da.
    pub(crate) fn hud_element_shown(&self, element: HudElement) -> bool {
        let set = &self.settings;
        match element {
            HudElement::Perf => set.show_hud,
            HudElement::ShieldCooldown => set.module_shields && set.shield_cd_self && set.shield_cd_self_mode == 0,
            HudElement::Arrows => set.module_arrows && set.arrow_indicator,
            HudElement::Armor => set.module_armor_hud,
            HudElement::Keystrokes => set.module_keystrokes,
            HudElement::Session => set.module_session_info,
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
            HudElement::Armor => {
                self.settings.hud_armor_x = defaults.hud_armor_x;
                self.settings.hud_armor_y = defaults.hud_armor_y;
                self.settings.hud_armor_scale = defaults.hud_armor_scale;
                self.settings.armor_hud_layout = defaults.armor_hud_layout;
                self.settings.armor_hud_sort_vertical = defaults.armor_hud_sort_vertical;
                self.settings.armor_hud_sort_flat = defaults.armor_hud_sort_flat;
                self.settings.armor_hud_green = defaults.armor_hud_green;
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
                self.settings.hud_scoreboard_x = ((cx - ox) / sw).clamp(0.0, 1.0);
                self.settings.hud_scoreboard_y = ((cy - oy) / sh).clamp(0.0, 1.0);
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

    // Die FPS-Zahl: weiss, solange sie das FPS-Limit (bzw. die Bildrate des Monitors) haelt,
    // darunter zunehmend orange -- ganz orange beim halben Limit. Ohne Limit nie.
    pub(crate) fn perf_fps_color(&self) -> [f32; 4] {
        let selbst = self.gpu.as_ref().is_some_and(|g| g.present_mode_self_paces());
        if self.settings.fps_limit == FPS_LIMIT_UNLIMITED && !selbst {
            return COLOR_PERF;
        }
        let ziel = 1.0 / self.profiler_target_interval().unwrap_or(self.monitor_frame_interval).as_secs_f32().max(1e-6);
        let t = ((ziel - self.frame_stats.fps() as f32) / (ziel / 2.0)).clamp(0.0, 1.0);
        let orange = [1.0, 0.55, 0.1, 1.0];
        [0, 1, 2, 3].map(|k| COLOR_PERF[k] + (orange[k] - COLOR_PERF[k]) * t)
    }

    // Letzter Rueckgabewert: der Sprite-Index, vor dem die Spielerfigur im Inventar gezeichnet
    // werden muss (s. build_inventory_screen). None = keine Figur in diesem Frame.
    pub(crate) fn build_hud(&self) -> (Vec<UiRect>, Vec<UiLine>, Vec<UiText>, Vec<UiSprite>, Option<usize>) {
        let Some(gpu) = self.gpu.as_ref() else {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new(), None);
        };
        let mut sprites: Vec<UiSprite> = Vec::new();
        let mut rects = Vec::new();
        let mut hud_lines = Vec::new();
        let mut texts = Vec::new();
        // Das Feuer vor den Augen zuerst: es liegt unter allem anderen (s. under_sprites).
        self.build_fire_overlay(gpu, &mut sprites);
        gpu.under_sprites.set(sprites.len());
        // Tearing-Test im Spiel, ganz hinten ueber der Welt. Nur im World-Modus: im Menue ruft
        // build_menu_ui diese Funktion selbst auf und hat die Balken schon gezeichnet.
        if self.mode == AppMode::World {
            let (sw, sh) = gpu.size();
            self.push_tearing_bars(&mut rects, sw, sh);
            self.push_profiler_overlay(gpu, &mut rects, &mut texts);
        }
        // Verbindungs-Overlay: nach "Los" landet man sofort in der 3D-Ansicht, aber solange
        // noch kein Chunk da ist, sitzt man vor einer leeren Welt. Dieses Overlay zeigt bis
        // dahin die Connect-Phase (aus dem Log). Unabhaengig von show_hud -- es ist keine
        // Performance-Anzeige, sondern eine Zustandsmeldung.
        if self.connecting_view() {
            self.build_connecting_overlay(gpu, &mut rects, &mut texts);
            // Dialoge des Servers (etwa der Verhaltenskodex in der Konfigurationsphase) kommen
            // vor dem ersten Chunk -- ohne sie hinge die Verbindung unsichtbar.
            if self.dialog_shown() || self.code_of_conduct.is_some() {
                self.build_dialog_ui(gpu, &mut rects, &mut texts, &mut sprites);
            }
            return (rects, hud_lines, texts, sprites, None);
        }
        // Was zur WELT gehoert (Namensschilder, Chat, Scoreboard, Hotbar, Totem-Anzeige),
        // verschwindet hinter einem Bildschirm-Overlay -- es wird gar nicht erst gebaut.
        //
        // Reihenfolge allein reicht dafuer NICHT: der UI-Pass zeichnet TYPWEISE gebatcht
        // (erst alle Flaechen, dann alle Sprites, dann aller Text, s. render_world). Ein
        // Namensschild ist Text, die Menue-Tafel eine Flaeche -- der Text liegt damit IMMER
        // oben, egal in welcher Reihenfolge beide in die Listen geschrieben werden. Ein
        // frueherer Versuch, das per Sortierung zu loesen, konnte deshalb gar nicht wirken
        // (Nametags blieben durch das Modul-Menue sichtbar).
        //
        // Das Inventar bleibt bewusst milder: dort blendet Vanilla den Chat weiter ein, nur
        // die Weltbeschriftungen sind verdeckt.
        //
        // Die Taste "HUD verstecken" (Vanillas F1, s. hud_hidden) nimmt im Spiel alles davon
        // weg. Der Chat kommt zurueck, sobald man tippt -- sonst saehe man nicht, was man
        // schreibt. Inventar und Modul-Menue bleiben, die hat man ja gerade selbst geoeffnet.
        let versteckt = self.hud_hidden && self.mode == AppMode::World;
        let welt_hud_verdeckt = self.module_menu_open || (versteckt && self.chat_input.is_none());
        // Schreibt man direkt aufs Schild (s. sign_edit_in_world), bleibt die Welt wie sie ist.
        let schild_in_welt = self.sign_edit_in_world(gpu).is_some();
        let beschriftungen_verdeckt = (self.screen_overlay_open() && !schild_in_welt) || versteckt;
        if !beschriftungen_verdeckt {
            self.build_totem_indicator(gpu, &mut texts, &mut sprites);
        }
        if self.settings.show_hud && !versteckt {
            self.push_hud_perf_texts(gpu, COLOR_PERF, &mut texts);
        }
        if self.debug_overlay && self.mode == AppMode::World && !versteckt {
            self.build_debug_overlay(gpu, &mut rects, &mut hud_lines, &mut texts);
        }
        if self.mode == AppMode::World {
            // Bei offenem Inventar zeigt Vanilla die HUD-Hotbar nicht mehr -- sie steckt
            // dann als eigene Slot-Reihe im Bildschirm selbst.
            if !beschriftungen_verdeckt {
                self.begin_text_area(gpu, &mut texts, ShadowArea::Hotbar);
                self.build_hotbar(gpu, &mut sprites, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                self.build_arrow_hud(gpu, &mut sprites, &mut texts);
                self.begin_text_area(gpu, &mut texts, ShadowArea::Hotbar);
                self.build_armor_hud(gpu, &mut rects, &mut sprites, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                if self.settings.module_keystrokes && self.conn_status == ConnStatus::Connected {
                    self.push_keystrokes(gpu, &mut rects, &mut texts);
                }
                if self.settings.module_session_info && self.conn_status == ConnStatus::Connected {
                    self.push_session_info(gpu, &mut rects, &mut texts);
                }
                self.build_toteming_hud(gpu, &mut rects, &mut hud_lines, &mut sprites, &mut texts);
                self.build_gamemode_wheel(gpu, &mut hud_lines, &mut sprites, &mut texts);
                // Gui.renderHotbarAndDecorations: Herzen, Ruestung, Hunger und Luft nur, wenn
                // man Schaden nehmen kann, die Erfahrungsleiste nur mit Erfahrung -- beides gilt
                // fuer Ueberleben und Abenteuer, nicht fuer Kreativ und Zuschauer.
                if !matches!(self.game_mode, GAME_MODE_CREATIVE | GAME_MODE_SPECTATOR) {
                    self.build_status_bars(gpu, &mut sprites, &mut texts);
                    if self.settings.module_saturation_hud {
                        self.build_saturation_bar(gpu, &mut rects);
                    }
                    if self.settings.hotbar_air_bar {
                        self.build_air_bar(gpu, &mut rects);
                    }
                }
                self.begin_text_area(gpu, &mut texts, ShadowArea::Hotbar);
                self.build_held_name(gpu, &mut rects, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                self.build_boss_bars(gpu, &self.boss_bars, &mut sprites, &mut texts);
                self.build_voice_hud(gpu, &mut texts);
                if self.settings.show_effects_hud && self.conn_status == ConnStatus::Connected {
                    self.build_effect_hud(gpu, &mut sprites, &mut texts, false, 1.0);
                }
                self.build_sound_meter(gpu, &mut rects, &mut texts);
                self.build_world_labels(gpu, &mut rects, &mut texts);
            }
            // Schilder gehoeren zur Welt, nicht zum HUD: auch bei F1 und unter Fenstern.
            self.build_sign_texts(gpu);
            if !welt_hud_verdeckt {
                // Der Chat ist fuer die Textauswahl ein eigener Bereich (und darin Verlauf,
                // Eingabe und Liste der gesendeten Nachrichten je einer, s. selection_boundary).
                selection_boundary(&mut texts);
                self.begin_text_area(gpu, &mut texts, ShadowArea::Chat);
                self.build_chat_overlay(gpu, &mut rects, &mut texts);
                Self::end_text_area(gpu, &mut texts);
                selection_boundary(&mut texts);
                self.build_server_hud(gpu, &mut rects, &mut texts);
                if self.player_list_held && self.conn_status == ConnStatus::Connected {
                    self.begin_text_area(gpu, &mut texts, ShadowArea::PlayerList);
                    self.build_player_list(gpu, &mut rects, &mut sprites, &mut texts);
                    Self::end_text_area(gpu, &mut texts);
                }
            }
        }
        // BILDSCHIRM-OVERLAYS GANZ ZULETZT (s. screen_overlay_open): Inventar und
        // Modul-Menue liegen ueber ALLEM, auch ueber Namensschildern und Chat.
        //
        // Der UI-Pass hat keinen Tiefentest -- allein die Reihenfolge in diesen Vecs
        // entscheidet, was oben liegt (s. render_world). Frueher standen die beiden Overlays
        // weiter oben, und build_world_labels/build_chat_overlay haben danach in dieselben Vecs
        // geschrieben: ein Namensschild eines Spielers hinter dem Menue wurde dadurch MITTEN
        // INS Menue gezeichnet, ebenso jede eintreffende Chatzeile.
        //
        // Was Addon-Module zeichnen (s. build_addon_hud), unter Menues und Overlays. Nur in der
        // Welt und nicht per F1 versteckt -- show_hud ist der Schalter der FPS-Anzeige.
        if self.mode == AppMode::World && !versteckt {
            self.build_addon_hud(gpu, &mut rects, &mut texts, &mut sprites);
        }
        // Das Abdunkeln des Menues (s. build_module_menu) deckt jetzt ebenfalls Chat und
        // Namensschilder mit ab -- genau so soll es sein: ein Overlay verdeckt die Welt
        // vollstaendig, nicht nur die Haelfte davon.
        let mut doll_split = None;
        if self.mode == AppMode::World {
            if self.book_edit.is_some() {
                self.build_book_edit_screen(gpu, &mut rects, &mut sprites, &mut texts);
            } else if self.lectern_book().is_some() || self.book_view.is_some() {
                self.build_book_screen(gpu, &mut rects, &mut sprites, &mut texts);
            } else if self.inventory.screen_open {
                self.begin_text_area(gpu, &mut texts, ShadowArea::Inventory);
                doll_split = self.build_inventory_screen(gpu, &mut rects, &mut sprites, &mut texts);
                Self::end_text_area(gpu, &mut texts);
            }
            if self.sign_edit.is_some() {
                self.build_sign_editor(gpu, &mut rects, &mut sprites, &mut texts);
            }
            if self.module_menu_open {
                self.build_module_menu(gpu, &mut rects, &mut texts);
                // Das Modul-Menue legt sich ueber alles -- dann waere die Figur ein Stueck
                // 3D-Geometrie mitten darin.
                doll_split = None;
            }
            // Dialoge des Servers liegen ueber allem.
            if self.dialog_shown() || self.code_of_conduct.is_some() {
                self.build_dialog_ui(gpu, &mut rects, &mut texts, &mut sprites);
                doll_split = None;
            }
        }
        // Gehaltene Reconnect-Taste: das Bild dunkelt bis zum Ausloesen ab, wie osus Neustart.
        if let Some(f) = self.reconnect_progress() {
            let (sw, sh) = gpu.size();
            rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.0, 0.0, 0.0, f], ..UiRect::FLAT });
        }
        self.push_selection_rects(gpu, &mut rects);
        (rects, hud_lines, texts, sprites, doll_split)
    }

    // Das Entity, durch dessen Augen ein Zuschauer gerade sieht -- solange es noch da ist.
    pub(crate) fn spectated_entity(&self) -> Option<&world::Entity> {
        self.entities.get(self.camera_entity?)
    }

    // Ob das Fadenkreuz gerade gezeichnet wird. Eine Quelle fuer Kontur (UI-Pass) und
    // Fadenkreuz-Pass, damit nie die Kontur ohne das Kreuz stehenbleibt.
    pub(crate) fn crosshair_visible(&self) -> bool {
        // Bei offener Chat-Eingabe ebenfalls unsichtbar: die Eingabezeile sitzt jetzt genau
        // an der Fadenkreuz-Position (s. chat_input_rect) -- beides gleichzeitig saehe aus,
        // als stuende das Kreuz mitten im Text. Als Zuschauer gar nicht.
        self.mode == AppMode::World
            && self.game_mode != GAME_MODE_SPECTATOR
            && self.perspective.is_first_person()
            && !self.screen_overlay_open()
            && self.chat_input.is_none()
            && !self.hud_hidden
            && !self.connecting_view()
    }

    // Verbindungsbildschirm: im Spiel, aber noch ohne einen Chunk vom Server. Dahinter steht
    // die zuletzt gespeicherte Welt dieses Servers (s. lod_preview_saved) wie ein Standbild --
    // abgedunkelt, ohne HUD, die Kamera fest; der Knopf Abbrechen trennt.
    pub(crate) fn connecting_view(&self) -> bool {
        self.mode == AppMode::World && self.chunks_received == 0
    }

    // --- Hotbar und Inventar ------------------------------------------------------------

    // Beschriftung eines Skalierungswerts: 0 zeigt zusaetzlich, was Auto gerade ergibt --
    // sonst waere "Auto" eine Angabe, die man nicht nachpruefen kann.
    pub(crate) fn scale_label(setting: u32, screen: (f32, f32)) -> String {
        if setting == 0 {
            format!("Auto (x{})", inventory::gui_scale(screen) as u32)
        } else {
            format!("x{setting}")
        }
    }

    // GUI-Skalierung aus einer Einstellung: 0 heisst Auto (Vanillas "Auto", so gross wie das
    // 320x240-Grundraster erlaubt), sonst die feste Stufe, hoechstens inventory::max_gui_scale.
    pub(crate) fn gui_scale_of(setting: u32, screen: (f32, f32)) -> f32 {
        if setting == 0 {
            inventory::gui_scale(screen)
        } else {
            setting.clamp(1, inventory::max_gui_scale(screen)) as f32
        }
    }

    // Groesste feste Stufe fuer die Plus-Knoepfe der Skalierungen.
    pub(crate) fn max_gui_scale_setting(&self) -> u32 {
        self.gpu.as_ref().map_or(4, |g| inventory::max_gui_scale(g.size()))
    }

    // Skalierung des Inventarfensters bzw. der Hotbar -- zwei getrennte Einstellungen, weil
    // man die Hotbar im Kampf gern klein haelt, das Inventar zum Sortieren aber gross.
    pub(crate) fn inventory_scale(&self, screen: (f32, f32)) -> f32 {
        Self::gui_scale_of(self.settings.inventory_scale, screen)
    }

    // Gui.tick fuer den Itemnamen ueber der Hotbar: ein anderes Item oder ein anderer Name
    // setzt ihn auf zwei Sekunden, sonst zaehlt er herunter. Anders als in Vanilla nur fuer
    // Items, die einen Tooltip haetten (s. item_needs_tooltip) -- welches gewoehnliche Item man
    // haelt, zeigt schon die Hotbar.
    pub(crate) fn tick_held_name(&mut self) {
        let schluessel = self
            .own_hand_stack(protocol::packets::HAND_MAIN)
            .filter(|s| !s.tooltip_hidden && item_shows_held_name(s))
            .map(|s| {
                let name: String = self.held_name_spans(s).iter().map(|t| t.text.as_str()).collect();
                format!("{}:{name}", s.item_id)
            });
        match schluessel {
            None => self.held_name_timer = 0,
            Some(ref k) if self.held_name_last.as_ref() != Some(k) => self.held_name_timer = HELD_NAME_TICKS,
            Some(_) => self.held_name_timer = (self.held_name_timer - 1).max(0),
        }
        self.held_name_last = schluessel;
    }

    // Der Name eines Items, wie ihn der Tooltip in seiner ersten Zeile zeigt (eigener Name,
    // Seltenheitsfarbe, Buchtitel ...).
    pub(crate) fn held_name_spans(&self, stack: &protocol::item::ItemStack) -> Vec<protocol::nbt::TextSpan> {
        protocol::nbt::with_lang_area(protocol::nbt::LangArea::Items, || self.item_tooltip_lines_in_area(stack)).into_iter().next().unwrap_or_default()
    }

    // Gui.renderSelectedItemName: mittig ueber der Hotbar, im Kreativmodus tiefer (dort fehlen
    // Herzen und Ruestung), in den letzten zehn Ticks ausblendend.
    pub(crate) fn build_held_name(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if self.held_name_timer <= 0 || self.conn_status != ConnStatus::Connected {
            return;
        }
        let Some(stack) = self.own_hand_stack(protocol::packets::HAND_MAIN) else { return };
        let spans = self.held_name_spans(stack);
        if spans.is_empty() {
            return;
        }
        // Nur den Namen, fuer den der Timer laeuft: nach einem Slotwechsel steht bis zum
        // naechsten Tick sonst der Name des neuen Items da, auch wenn es gar keinen zeigt.
        let name: String = spans.iter().map(|t| t.text.as_str()).collect();
        if stack.tooltip_hidden || !item_shows_held_name(stack) || self.held_name_last.as_deref() != Some(format!("{}:{name}", stack.item_id).as_str()) {
            return;
        }
        let alpha = (self.held_name_timer as f32 * 256.0 / 10.0).min(255.0) / 255.0;
        let g = self.held_name_scale(gpu);
        let Rect { x, y, .. } = self.held_name_rect_for(gpu, span_row_width(gpu, &spans, g));
        let vorher = texts.len();
        push_styled_row(gpu, &spans, x, y, COLOR_WHITE, g, texts, &mut |r, mut c| {
            c[3] *= alpha;
            rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT });
        });
        // Marker tragen ihren Code in einer negativen Deckkraft und bleiben unberuehrt.
        for t in texts[vorher..].iter_mut().filter(|t| t.color[3] > 0.0) {
            t.color[3] *= alpha;
        }
    }

    // Schriftfaktor des Itemnamens: Hotbar-Skala mal eigener Prozentwert, auf Viertel gerundet
    // (wie title_scale).
    pub(crate) fn held_name_scale(&self, gpu: &GpuState) -> f32 {
        let g = self.hotbar_scale(gpu.size()) * self.settings.hud_held_name_scale as f32 / 100.0;
        (g * 4.0).round().max(1.0) / 4.0
    }

    // Wo ein Itemname der Breite `w` steht. Ungezogen an der Hotbar wie in Vanilla (mittig, 37
    // ueber ihrer Oberkante, im Kreativmodus tiefer), sonst um die gespeicherte Mitte.
    pub(crate) fn held_name_rect_for(&self, gpu: &GpuState, w: f32) -> Rect {
        let (sw, sh) = gpu.size();
        let h = gpu.text_height(self.held_name_scale(gpu));
        let set = &self.settings;
        let (x, y) = if set.hud_held_name_y < 0.0 {
            let hotbar = self.hud_hotbar_rect(gpu);
            let g = self.hotbar_scale(gpu.size());
            let ueber = if matches!(self.game_mode, GAME_MODE_CREATIVE | GAME_MODE_SPECTATOR) { HELD_NAME_ABOVE_HOTBAR - 14.0 } else { HELD_NAME_ABOVE_HOTBAR };
            (hotbar.x + (hotbar.w - w) / 2.0, hotbar.y - ueber * g)
        } else {
            (set.hud_held_name_x * sw - w / 2.0, set.hud_held_name_y * sh)
        };
        Rect { x: x.clamp(0.0, (sw - w).max(0.0)).round(), y: y.clamp(0.0, (sh - h).max(0.0)).round(), w, h }
    }

    // Griff im HUD-Editor: der Name des gehaltenen Items, ohne einen ein Platzhalter.
    pub(crate) fn hud_held_name_rect(&self, gpu: &GpuState) -> Rect {
        let g = self.held_name_scale(gpu);
        let w = match self.own_hand_stack(protocol::packets::HAND_MAIN) {
            Some(stack) => span_row_width(gpu, &self.held_name_spans(stack), g),
            None => gpu.measure_text("Itemname", g),
        };
        self.held_name_rect_for(gpu, w.max(gpu.measure_text("Itemname", g)))
    }

    pub(crate) fn hotbar_scale(&self, screen: (f32, f32)) -> f32 {
        Self::gui_scale_of(self.settings.hud_hotbar_scale, screen)
    }

    // Grundskala der HUD-Teile, die nicht an der Hotbar haengen (Effekte, Pfeile, Spielerliste,
    // Bossleisten ...): Vanillas Auto-Stufe, damit sie nicht mitwachsen, wenn man nur die
    // Hotbar groesser oder kleiner stellt.
    pub(crate) fn hud_scale(&self, screen: (f32, f32)) -> f32 {
        Self::gui_scale_of(0, screen)
    }

    // Textskalierung des Chats: Prozent von UI_SCALE_SMALL, auf Viertel gerundet -- die
    // Schrift liegt in Viertel-Fassungen vor (s. font_atlas::DM_SANS_QUARTERS) und bleibt nur
    // dann pixelgenau.
    pub(crate) fn chat_scale(&self) -> f32 {
        let roh = UI_SCALE_SMALL * self.settings.chat_scale_percent as f32 / 100.0;
        ((roh * 4.0).round() / 4.0).max(0.5)
    }

    // Was der Fadenkreuz-Pass ausser dem Kreuz zeichnet: den eigenen Schild-Cooldown (Zahl oder
    // Kreis) und die Pfeil-Zahl -- beide dort, damit sie auf Wunsch die Fadenkreuz-Farbe
    // bekommen (Gegenfarbe zum Untergrund, s. crosshair.wgsl).
    pub(crate) fn crosshair_extras(&self) -> CrosshairExtras {
        let mut extras = CrosshairExtras {
            shape: self.settings.crosshair_shape,
            size: self.settings.crosshair_size(),
            ring_outline: self.settings.crosshair_ring_outline,
            ..CrosshairExtras::default()
        };
        if self.settings.crosshair_custom {
            let p = &self.settings.crosshair_pattern;
            extras.custom = Some(CustomCrosshair { fill: p.runs(crosshair_editor::Pixel::Fill), outline: p.runs(crosshair_editor::Pixel::Outline) });
        }
        let Some(gpu) = self.gpu.as_ref() else { return extras };
        let (sw, sh) = gpu.size();
        let set = &self.settings;
        // Timer: waehrend man spannt oder isst, gehoert der Kreis ihm -- auch ueber dem
        // Schild-Cooldown (gewuenscht).
        let timer = self.use_timer_ring();
        // Der Kreis gehoert zum Fadenkreuz: bei offenem Chat oder einem anderen Fenster
        // verschwindet er mit ihm.
        let ring_frei = self.chat_input.is_none() && !self.screen_overlay_open();
        if let Some(anteil) = timer.filter(|a| *a > 0.0 && ring_frei) {
            extras.ring = Some(((0.5, 0.5), anteil));
        }
        if let Some((rest, anteil)) = self.own_shield_cooldown() {
            if timer.is_some() {
            } else if set.shield_cd_self_mode == 1 {
                // Der Kreis FUELLT sich: leer im Moment des Bruchs, voll, wenn der Schild wieder
                // geladen ist (gewuenscht) -- `anteil` ist der verbleibende Rest, also umgedreht.
                if ring_frei {
                    extras.ring = Some(((0.5, 0.5), 1.0 - anteil));
                }
            } else {
                let font = set.effective_font(ShadowArea::ShieldCountdown);
                extras.texts.push(CrosshairText {
                    text: format!("{:.1}", rest / 20.0),
                    center_px: (set.hud_shield_cd_x * sw, set.hud_shield_cd_y * sh),
                    scale: SHIELD_CD_TEXT_SCALE,
                    white: set.shield_cd_text_style == 1,
                    font,
                    alpha: 1.0,
                });
            }
        }
        if let Some((text, rect, scale)) = self.arrow_number(gpu) {
            extras.texts.push(CrosshairText {
                text,
                center_px: (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                scale,
                white: set.arrow_text_style == 1,
                font: set.effective_font(ShadowArea::ArrowNumber),
                alpha: set.arrow_opacity as f32 / 100.0,
            });
        }
        extras
    }

    // Die laufende Totem-Anzeige zeichnen: das Flug-Item in der Bildschirmmitte, in der
    // Farbe der Hand, auf deren Seite versetzt, mit der Hand als Text darunter.
    pub(crate) fn build_totem_indicator(&self, gpu: &GpuState, texts: &mut Vec<UiText>, sprites: &mut Vec<UiSprite>) {
        let Some(anim) = self.totem_animation.as_ref() else { return };
        let (sw, sh) = gpu.size();
        let tick_progress = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0);
        let frame = anim.frame(tick_progress, sw / sh);
        // Grundgroesse an der Bildschirmhoehe: die Anzeige soll auf jedem Bildschirm gleich
        // gross wirken, nicht in Pixeln festgenagelt sein.
        let basis = sh * 0.22;
        let breite = basis * frame.scale * frame.squash;
        let hoehe = basis * frame.scale;
        let cx = sw / 2.0 + frame.offset_x * sh * 0.5;
        let cy = sh / 2.0 + frame.offset_y * sh * 0.5;
        // Die letzten Ticks ausblenden, damit die Anzeige nicht mitten im Bild verschwindet.
        let alpha = (anim.progress(tick_progress) * 6.0).min(1.0).min((1.0 - anim.progress(tick_progress)) * 6.0).clamp(0.0, 1.0);
        self.item_icons.push_tinted(
            sprites,
            "totem_of_undying",
            cx - breite / 2.0,
            cy - hoehe / 2.0,
            breite,
            hoehe,
            anim.hand.tint(),
            alpha,
        );
        // Der Text ist der eindeutigste Kanal: Farbe und Seite muss man gelernt haben, das
        // Wort nicht. Er sitzt UNTER dem Item, damit er nicht von ihm verdeckt wird.
        let label = anim.hand.label();
        let w = gpu.measure_text(label, UI_SCALE_SMALL);
        let [r, g, b] = anim.hand.tint();
        texts.push(UiText {
            text: label.to_string(),
            x: (sw / 2.0 - w / 2.0).round(),
            y: (sh / 2.0 + basis * 0.55).round(),
            color: [r, g, b, alpha],
            scale: UI_SCALE_SMALL,
        });
    }

    // --- Totem-Modul (s. modules.rs) ---

    // Liegt in dem Slot ein Totem der Unsterblichkeit?
    // Wie Vanilla beim Tod: was rettet, entscheidet die death_protection-Component, nicht der Name.
    pub(crate) fn slot_ist_totem(&self, slot: usize) -> bool {
        self.inventory.player.get(slot).and_then(|s| s.as_ref()).is_some_and(world::item_behavior::death_protection)
    }

    // Was liegt JETZT in den beiden Haenden? Die Haupthand ist der aktive Hotbar-Slot --
    // dieselbe Zuordnung, die auch der Server benutzt.
    pub(crate) fn totem_in_haenden(&self) -> (bool, bool) {
        let main = self.slot_ist_totem(inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8));
        let off = self.slot_ist_totem(inventory::PLAYER_OFFHAND);
        (main, off)
    }

    // Bildschirm-Rechteck des Hotbar-Sheets. hud_hotbar_x ist die normierte MITTE (nicht die
    // linke Kante wie bei den Textelementen), hud_hotbar_y die normierte UNTERKANTE -- so
    // bleibt die Standardposition (0.5 / 1.0) bei jeder Aufloesung mittig am unteren Rand,
    // ohne von der (skalierungsabhaengigen) Sheetgroesse abzuhaengen.
    // Sprachchat-Anzeige links neben der Hotbar (wie die HUD-Symbole der Mod): taub, stumm,
    // sprechen bzw. fluestern -- oder grau, wenn die Sprechtaste gehalten wird, die Verbindung
    // aber nicht steht.
    pub(crate) fn build_voice_hud(&self, gpu: &GpuState, texts: &mut Vec<UiText>) {
        let v = &self.settings.voice;
        if !self.settings.module_voice_chat || !v.hud_icons || self.voice.status == voicechat::Status::Off {
            return;
        }
        let verbunden = self.voice.status == voicechat::Status::Connected;
        let (text, farbe) = if verbunden && v.disabled {
            ("\u{266a} Taub", 0xFF5555)
        } else if verbunden && v.muted {
            ("\u{266a} Stumm", 0xFF5555)
        } else if self.voice.sending {
            if self.voice.whispering {
                ("\u{266a} Flüstern", 0x55FFFF)
            } else {
                ("\u{266a} Sprechen", 0x55FF55)
            }
        } else if self.push_to_talk_held && self.voice.mic.is_none() {
            ("\u{266a} kein Mikrofon", 0xFF5555)
        } else if self.push_to_talk_held && !verbunden {
            ("\u{266a} nicht verbunden", 0xAAAAAA)
        } else {
            return;
        };
        let rect = self.hud_hotbar_rect(gpu);
        let scale = self.hotbar_scale(gpu.size()).round().max(1.0);
        let breite = gpu.measure_text(text, scale);
        let hoehe = gpu.text_height(scale);
        texts.push(UiText {
            text: text.to_string(),
            x: (rect.x - breite - 6.0 * scale).round(),
            y: (rect.y + rect.h - hoehe - 2.0 * scale).round(),
            color: rgb_to_rgba(farbe),
            scale,
        });
    }

    pub(crate) fn hud_hotbar_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let scale = self.hotbar_scale((sw, sh));
        let w = inventory::HOTBAR_W * scale;
        let h = inventory::HOTBAR_H * scale;
        let x = (self.settings.hud_hotbar_x * sw - w / 2.0).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_hotbar_y * sh - h).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // Ein Item mit Stackgroesse und Haltbarkeitsbalken in ein Slot-Quadrat zeichnen.
    // Die Eingaben der Item-Modell-Auswertung fuer einen Stapel an Anzeigeort `context`.
    pub(crate) fn item_model_input<'a>(&'a self, stack: &'a protocol::item::ItemStack, context: &'a str) -> item_model_input::StackInput<'a> {
        item_model_input::StackInput::new(stack, context, &self.enchantments, &self.trim_registries.material_ids)
    }

    pub(crate) fn push_item(&self, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>, stack: &protocol::item::ItemStack, x: f32, y: f32, size: f32) {
        self.push_item_limited(sprites, texts, stack, x, y, size, DurabilityBarLimit::Always, 0.0);
        // Verstaerkter (S) oder verlaengerter (L) Trank -- die Farbe allein unterscheidet die
        // drei Fassungen nicht. Nur hier, nicht in der Hotbar (die zeichnet ueber
        // push_item_limited). Halb so gross wie die Anzahl, auf ganze Pixel gerundet, damit die
        // Schrift scharf bleibt. Direkt links neben dem Flaschenhals: rechte Kante bei Pixel 6,
        // Unterkante bei 6,5 des 16er-Bilds -- Trink- und Wurftrank haben dort beide frei.
        let zeigen = |c: char| match c {
            'S' => self.settings.potion_mark_strong,
            'L' => self.settings.potion_mark_long,
            'W' => self.settings.potion_mark_weak,
            _ => true,
        };
        if let Some(c) = world::potion::variant_mark(stack).filter(|&c| zeigen(c)) {
            let scale = (size / 32.0).round().max(1.0);
            let text = c.to_string();
            let px = size / 16.0;
            let breite = self.gpu.as_ref().map_or(0.0, |g| g.measure_text(&text, scale));
            let hoehe = self.gpu.as_ref().map_or(0.0, |g| g.text_height(scale));
            let (tx, ty) = ((x + 6.0 * px - breite).round(), (y + 6.5 * px - hoehe).round());
            texts.push(UiText { text, x: tx, y: ty, color: [1.0, 1.0, 1.0, 1.0], scale });
        }
    }

    // Wie push_item, aber der Haltbarkeitsbalken erscheint erst, wenn hoechstens der Anteil
    // Grenze `limit` erreicht ist (Hotbar-Einstellung "Haltbarkeit ab").
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_item_limited(
        &self,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
        stack: &protocol::item::ItemStack,
        x: f32,
        y: f32,
        size: f32,
        limit: DurabilityBarLimit,
        pop: f32,
    ) {
        let Some(name) = world::item_registry::item_name(stack.item_id) else {
            return;
        };
        let erstes = sprites.len();
        self.push_item_icon_only(sprites, stack, x, y, size);
        // Aufnahme-Animation: nur das Bild, um (8, 12) des Slots schmaler und hoeher, wie
        // Vanillas Gui.renderSlot. Anzahl und Balken bleiben stehen.
        if pop > 0.0 {
            let g = 1.0 + pop / 5.0;
            let (sx, sy) = (1.0 / g, (g + 1.0) / 2.0);
            let (mx, my) = (x + size * 0.5, y + size * 0.75);
            for sprite in &mut sprites[erstes..] {
                for c in &mut sprite.corners {
                    *c = [mx + (c[0] - mx) * sx, my + (c[1] - my) * sy];
                }
            }
        }
        // DrawContext.drawItemBar: Vanilla legt den Balken (13x1 bei (2, 13)) auf einen
        // schwarzen 13x2 -- unten ein Schatten. Hier stattdessen eine Kontur: ein
        // Bildschirmpixel Schwarz rundherum (gewuenscht, wie die Text-Kontur).
        let balken = item_bar(stack, name).filter(|_| match limit {
            DurabilityBarLimit::Always => true,
            DurabilityBarLimit::Rest(r) => r >= 1.0 || durability_rest(stack, name).is_some_and(|rest| rest <= r + 1e-4),
            DurabilityBarLimit::Points(p) => durability_points(stack, name).is_some_and(|rest| rest <= p),
        });
        if let (Some((schritt, farbe)), Some(weiss)) = (balken, self.ui_atlas.rect(assets::UI_WHITE)) {
            let px = size / 16.0;
            let uv = weiss.sub(1, 1, 2, 2);
            sprites.push(UiSprite::quad(x + 2.0 * px - 1.0, y + 13.0 * px - 1.0, 13.0 * px + 2.0, px + 2.0, uv, [0.0, 0.0, 0.0, 1.0], SpriteAtlas::Ui));
            let [r, g, b] = farbe.map(world::sky::srgb_to_linear);
            sprites.push(UiSprite::quad(x + 2.0 * px, y + 13.0 * px, schritt * px, px, uv, [r, g, b, 1.0], SpriteAtlas::Ui));
        }
        // DrawContext.drawCooldownProgress: weisser Schleier (0x7FFFFFFF) von unten, so hoch
        // wie der Rest der Abklingzeit. Vanilla rechnet in 16 GUI-Pixeln und rundet dort --
        // hier dieselbe Rundung auf dem 16er-Raster des Slots.
        if !self.item_cooldowns.is_empty() {
            let f = self.item_cooldowns.progress(stack, self.tick_progress());
            if f > 0.0 {
                if let Some(weiss) = self.ui_atlas.rect(assets::UI_WHITE) {
                    let px = size / 16.0;
                    let oben = y + (16.0 * (1.0 - f)).floor() * px;
                    let hoehe = (16.0 * f).ceil() * px;
                    sprites.push(UiSprite::quad(x, oben, size, hoehe, weiss.sub(1, 1, 2, 2), [1.0, 1.0, 1.0, 127.0 / 255.0], SpriteAtlas::Ui));
                }
            }
        }
        if stack.count > 1 {
            self.push_count_label(texts, &stack.count.to_string(), x, y, size);
        }
    }

    // Nur das Bild eines Items (mit Glint), ohne Balken, Abklingzeit und Anzahl -- so zeichnet
    // Vanilla auch das im Buendel gewaehlte Item ins offene Buendel.
    pub(crate) fn push_item_icon_only(&self, sprites: &mut Vec<UiSprite>, stack: &protocol::item::ItemStack, x: f32, y: f32, size: f32) {
        let Some(name) = world::item_registry::item_name(stack.item_id) else {
            return;
        };
        let erstes = sprites.len();
        // item_model-Komponente: der Server will das Item mit einem anderen Modell zeigen --
        // soweit wir fuer dieses Modell ein Icon haben (Modelle aus Server-Resourcepacks nicht).
        let modell = stack.item_model.as_deref().map(|m| m.strip_prefix("minecraft:").unwrap_or(m)).filter(|m| self.item_icons.has(m));
        let banner = (name == "shield").then(|| world::entity::ShieldBanner::from_stack(stack)).flatten();
        // Zeigt item_model auf einen Kopf (Golden Head: Honigflasche als player_head), wird er
        // wie ein echter Kopf mit dem Skin aus der profile-Komponente gezeichnet.
        // Die gui-Haltung kommt vom KOPF-Modell, nicht vom Item: sonst stand der Golden Head
        // mit der Haltung der Honigflasche (flach, ungedreht) im Slot.
        let kopf = world::item_registry::item_name(world::entity::model_item_id(stack)).and_then(|m| self.skull_texture(m, stack).map(|t| (m, t)));
        if let Some((kopf_modell, (tex, hut))) = kopf {
            self.item_icons.push_skull(sprites, kopf_modell, tex, hut, x, y, size, 1.0);
        } else if let (None, Some(banner)) = (modell, banner) {
            self.push_shield_banner_icon(sprites, banner, x, y, size);
        } else {
            let tint = world::potion::stack_tint(stack);
            // Die Definition des Modells je Stapel auswerten (Verzauberung, custom_model_data,
            // Ladung ...). Das Schild bleibt beim Entity-Modell unter seinem Namen.
            let id = model_definition_id(stack, name);
            let input = self.item_model_input(stack, "gui");
            match self.item_icons.stack_keys(&id, &input).filter(|_| !self.item_icons.has_entity_model(&id)) {
                Some(keys) => {
                    for key in keys {
                        match key {
                            Some(key) => self.item_icons.push(sprites, key, tint, x, y, size, 1.0),
                            // Das im Buendel gewaehlte Item zwischen Rueck- und Vorderseite.
                            None => {
                                if let Some(gewaehlt) = usize::try_from(stack.bundle_selected).ok().and_then(|i| world::bundle::contents(stack).get(i)) {
                                    self.push_item_icon_only(sprites, gewaehlt, x, y, size);
                                }
                            }
                        }
                    }
                }
                None => self.item_icons.push(sprites, modell.unwrap_or_else(|| item_icon_name(name, stack)), tint, x, y, size, 1.0),
            }
        }
        // Glint: jedes Quad des Icons (auch jede Wuerfelseite eines Blockitems) noch einmal,
        // direkt dahinter, als Glint-Zwilling (s. shape_sprites und sprite.wgsl::fs_glint).
        if world::enchantment::has_glint(stack) {
            for i in erstes..sprites.len() {
                let icon = sprites[i];
                if let Some(atlas) = icon.atlas.glint() {
                    sprites.push(UiSprite { atlas, ..icon });
                }
            }
        }
    }

    // Zahl rechts unten in den Slot, wie in Vanilla. Skalierung an die Slotgroesse
    // gekoppelt, damit sie bei jeder GUI-Skalierung sitzt.
    // Ganze Stufe: die Zahl im Slot soll scharf bleiben (s. UiText::scale).
    pub(crate) fn push_count_label(&self, texts: &mut Vec<UiText>, text: &str, x: f32, y: f32, size: f32) {
        let scale = (size / 16.0).round().max(1.0);
        let width = self.gpu.as_ref().map(|g| g.measure_text(text, scale)).unwrap_or(0.0);
        let height = self.gpu.as_ref().map(|g| g.text_height(scale)).unwrap_or(0.0);
        texts.push(UiText { text: text.to_string(), x: (x + size - width).round(), y: (y + size - height).round(), color: [1.0, 1.0, 1.0, 1.0], scale });
    }

    // Toteming Pro Max (s. toteming.rs): die X neben dem Fadenkreuz (TotemWarningHud) und
    // die Totems an ihrer kuenftigen Inventar-Position (TotemSlotOverlayHud). Beide lesen
    // dieselbe Zahl fehlender Totems, damit sie nie Verschiedenes behaupten.
    pub(crate) fn build_toteming_hud(
        &self,
        gpu: &GpuState,
        rects: &mut Vec<UiRect>,
        lines: &mut Vec<UiLine>,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
    ) {
        let t = &self.settings.toteming;
        if !(t.warning || t.slots) || self.conn_status != ConnStatus::Connected || self.game_mode == GAME_MODE_SPECTATOR {
            return;
        }
        let hotbar_totem = (0..9).any(|i| self.slot_ist_totem(inventory::PLAYER_HOTBAR_START + i));
        let offhand_totem = self.slot_ist_totem(inventory::PLAYER_OFFHAND);
        let state = toteming::TotemState {
            offhand_totem,
            // Vanilla fragt nach der Komponente BLOCKS_ATTACKS.
            offhand_shield: self
                .inventory
                .player
                .get(inventory::PLAYER_OFFHAND)
                .and_then(|s| s.as_ref())
                .is_some_and(|stack| world::item_behavior::blocks_attacks(stack).is_some()),
            hotbar_totem,
            any_totem: hotbar_totem
                || offhand_totem
                || (inventory::PLAYER_MAIN_START..inventory::PLAYER_HOTBAR_START).any(|i| self.slot_ist_totem(i)),
        };
        let missing = toteming::count_missing(state);
        if missing == 0 {
            return;
        }
        let (sw, sh) = gpu.size();

        if t.warning {
            // Ein GUI-Pixel der Mod = ein Pixel der Hotbar-Skalierung.
            let unit = self.hud_scale(gpu.size());
            let (cx, cy) = ((sw / 2.0).round(), (sh / 2.0).round());
            let off = t.offset as f32 * unit;
            // Zweithand links, Haupthand rechts -- der Client kennt nur Rechtshaender.
            // Scharfe, geglaettete Striche statt der Pixeltreppe der Mod, mit schwarzer Kontur
            // (Settings: toteming.shadow) -- alle Konturen zuerst, sonst laege die Kontur des
            // zweiten Strichs ueber dem ersten.
            let mut kreuze: Vec<(f32, [f32; 4])> = Vec::new();
            if !state.offhand_totem {
                kreuze.push((cx - off, argb_to_rgba(t.offhand_color.argb() as i32)));
            }
            if !state.hotbar_totem {
                kreuze.push((cx + off, argb_to_rgba(t.hotbar_color.argb() as i32)));
            }
            let dicke = toteming::cross_thickness(unit);
            let kontur = (unit * 0.5).max(1.0);
            let feder = 1.0;
            if t.shadow {
                for &(x, _) in &kreuze {
                    for (p0, p1) in toteming::cross_segments((x, cy), t.size, unit, kontur) {
                        lines.push(UiLine { p0, p1, color: [0.0, 0.0, 0.0, 0.85], width: dicke + 2.0 * kontur, feather: feder, quad: None, quad_colors: None, top: false });
                    }
                }
            }
            for &(x, farbe) in &kreuze {
                for (p0, p1) in toteming::cross_segments((x, cy), t.size, unit, 0.0) {
                    lines.push(UiLine { p0, p1, color: farbe, width: dicke, feather: feder, quad: None, quad_colors: None, top: false });
                }
            }
        }

        if t.slots {
            // Dieselbe Aufteilung, die das Inventar gleich beim Oeffnen benutzt -- der Zeiger
            // steht dann in der Bildschirmmitte.
            let layout = inventory::player_screen_layout(gpu.size(), self.inventory_scale(gpu.size()));
            let px = layout.scale;
            let totems: Vec<inventory::SlotRect> = layout
                .slots
                .iter()
                .filter(|s| (inventory::PLAYER_MAIN_START..inventory::PLAYER_HOTBAR_START + 9).contains(&s.index) && self.slot_ist_totem(s.index))
                .copied()
                .collect();
            for s in toteming::overlay_slots(&totems, (sw / 2.0, sh / 2.0), missing, t.extra) {
                match t.backdrop {
                    toteming::Backdrop::Box => {
                        let (x0, y0, groesse) = (s.x - px, s.y - px, s.size + 2.0 * px);
                        let rand = argb_to_rgba(0xC0FF_FFFFu32 as i32);
                        rects.push(UiRect { rect: Rect { x: x0, y: y0, w: groesse, h: groesse }, color: argb_to_rgba(0x9000_0000u32 as i32), ..UiRect::FLAT });
                        rects.push(UiRect { rect: Rect { x: x0, y: y0, w: groesse, h: px }, color: rand, ..UiRect::FLAT });
                        rects.push(UiRect { rect: Rect { x: x0, y: y0 + groesse - px, w: groesse, h: px }, color: rand, ..UiRect::FLAT });
                        rects.push(UiRect { rect: Rect { x: x0, y: y0 + px, w: px, h: groesse - 2.0 * px }, color: rand, ..UiRect::FLAT });
                        rects.push(UiRect { rect: Rect { x: x0 + groesse - px, y: y0 + px, w: px, h: groesse - 2.0 * px }, color: rand, ..UiRect::FLAT });
                    }
                    toteming::Backdrop::Shadow => rects.push(UiRect {
                        rect: Rect { x: s.x + 2.0 * px, y: s.y + 2.0 * px, w: s.size, h: s.size },
                        color: argb_to_rgba(0x7000_0000u32 as i32),
                        ..UiRect::FLAT
                    }),
                    toteming::Backdrop::None => {}
                }
                if let Some(stack) = self.inventory.player.get(s.index).and_then(|x| x.as_ref()) {
                    self.push_item(sprites, texts, stack, s.x, s.y, s.size);
                }
            }
        }
    }

    // Pfeil-Modul: die Munition, die Bogen/Armbrust in der Hand gerade verschiessen wuerden,
    // als Slot-Icon links unter dem Fadenkreuz. Die Zahl ist ALLES Passende im Inventar
    // (gleiches Item, gleicher Trank) -- nicht nur der eine Stapel, den Vanilla zuerst nimmt.
    pub(crate) fn build_arrow_hud(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        if !(self.settings.module_arrows && self.settings.arrow_indicator && self.crosshair_visible())
            || self.conn_status != ConnStatus::Connected
        {
            return;
        }
        let Some(anzeige) = self.arrow_hud_stack() else { return };
        // Die Zahl zeichnet im Spiel der Fadenkreuz-Pass (s. arrow_number), hier nur das Icon.
        self.push_arrow_indicator(gpu, &anzeige, sprites, texts, false);
    }

    // Die Pfeil-Zahl im Spiel: Text, das Rechteck, in dem sie steht, und ihr Zeichenfaktor --
    // dieselbe Lage wie in push_arrow_indicator. None, wenn keine Zahl zu zeigen ist.
    pub(crate) fn arrow_number(&self, gpu: &GpuState) -> Option<(String, Rect, f32)> {
        if !(self.settings.module_arrows && self.settings.arrow_indicator && self.crosshair_visible()) || self.conn_status != ConnStatus::Connected {
            return None;
        }
        let rect = self.hud_arrows_rect(gpu);
        let scale = (rect.w / 16.0).round().max(1.0);
        let Some(stack) = self.arrow_hud_stack() else {
            // Keine Munition: nach einem Spannversuch blinkt eine 0 (Pfeil-Modul, "0 blinkt").
            let seit = self.arrow_empty_at?.elapsed().as_secs_f32();
            if self.settings.arrow_empty_feedback != ARROW_EMPTY_FEEDBACK_BLINK || seit >= ARROW_EMPTY_BLINK_SECS || (seit * 8.0) as u32 % 2 == 1 {
                return None;
            }
            let atlas = gpu.hud_font(self.settings.effective_font(ShadowArea::ArrowNumber));
            let w = atlas.glyph('0').map_or(0.0, |g| g.advance_px * scale);
            let h = atlas.cell_height_px() * scale;
            return Some(("0".to_string(), Rect { x: rect.x + (rect.w - w) / 2.0, y: rect.y + (rect.h - h) / 2.0, w, h }, scale));
        };
        let text = stack.count.to_string();
        // Mit der Schrift vermessen, in der die Zahl auch gezeichnet wird.
        let atlas = gpu.hud_font(self.settings.effective_font(ShadowArea::ArrowNumber));
        let w: f32 = text.chars().filter_map(|c| atlas.glyph(c)).map(|g| g.advance_px * scale).sum();
        let h = atlas.cell_height_px() * scale;
        if self.settings.arrow_number_only {
            return Some((text, Rect { x: rect.x + (rect.w - w) / 2.0, y: rect.y + (rect.h - h) / 2.0, w, h }, scale));
        }
        (stack.count > 1).then_some((text, Rect { x: rect.x + rect.w - w, y: rect.y + rect.h - h, w, h }, scale))
    }

    // Pfeil-Modul, "Wackeln": waagerechter Versatz des Fadenkreuzes in Bildschirmpixeln nach
    // einem Spannversuch ohne Munition -- ein kurzes, abklingendes Kopfschuetteln.
    pub(crate) fn arrow_empty_shake_px(&self) -> f32 {
        if !self.settings.module_arrows || self.settings.arrow_empty_feedback != ARROW_EMPTY_FEEDBACK_SHAKE {
            return 0.0;
        }
        let Some(seit) = self.arrow_empty_at.map(|t| t.elapsed().as_secs_f32()) else { return 0.0 };
        if seit >= ARROW_EMPTY_SHAKE_SECS {
            return 0.0;
        }
        let abklingen = 1.0 - seit / ARROW_EMPTY_SHAKE_SECS;
        let px = self.gpu.as_ref().map_or(2.0, |g| self.hud_scale(g.size()));
        (seit * std::f32::consts::TAU * 12.0).sin() * 3.0 * px * abklingen
    }

    // Entity-Event 3: ein Lebewesen ist gestorben. Ab jetzt laeuft seine deathTime; bei
    // Spielern mit Todesanimation Explosion fliegen die Teile los (aus der Haltung im Stand),
    // dazu eine Explosionswolke wie bei TNT. Wurfgeschosse schicken dasselbe Event beim
    // Aufprall -- sie haben kein Modell und zaehlen nicht.
    pub(crate) fn entity_died(&mut self, entity_id: i32) {
        if self.own_entity_id == Some(entity_id) || self.dying.contains_key(&entity_id) {
            return;
        }
        let Some(e) = self.entities.get(entity_id) else { return };
        if !is_player_like(e.entity_type) && self.model_for_type(e.entity_type).is_none() {
            return;
        }
        self.dying.insert(entity_id, 0);
        if self.settings.death_animation != DEATH_ANIM_EXPLOSION || !is_player_like(e.entity_type) || e.invisible {
            return;
        }
        let progress = self.tick_progress();
        let feet = e.render_pos(progress);
        // Starb er in der Luft, fallen die Teile bis zum Boden darunter (hoechstens 8 Bloecke).
        let (bx, bz) = (feet.0.floor() as i32, feet.2.floor() as i32);
        let fuss = feet.1.floor() as i32;
        let drop = (0..8)
            .map(|d| fuss - d)
            .find(|&y| self.world.block_at(bx, y, bz) != world::STATE_AIR)
            .map_or(0.0, |y| (feet.1 - (y + 1) as f64).max(0.0) as f32);
        let model_scale = e.scale * world::entity_model::PLAYER_MODEL_SCALE;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64) ^ entity_id as u64;
        let burst = death_burst::Burst::new(e.uuid, feet, e.render_yaw(progress), e.scale, model_scale, e.armor, drop, seed);
        let (breite, hoehe) = e.hit_size();
        let mitte = DVec3::new(feet.0, feet.1 + hoehe * 0.5, feet.2);
        self.death_bursts.push(burst);
        let knall = protocol::particle::ParticleEffect { type_id: protocol::particle::PARTICLE_EXPLOSION, options: protocol::particle::ParticleOptions::None };
        self.with_particles(|p, ctx, _| {
            p.add_world_particle(&knall, mitte, DVec3::ZERO, ctx);
            p.add_death_poof(particles::EmitterBody { pos: DVec3::new(feet.0, feet.1, feet.2), width: breite, height: hoehe }, ctx);
        });
    }

    pub(crate) fn tick_death_bursts(&mut self) {
        self.death_bursts.retain_mut(death_burst::Burst::tick);
        // Entfernte Leichen vergessen (Entity-IDs werden nicht so bald neu vergeben).
        let entities = &self.entities;
        self.dying.retain(|id, _| entities.get(*id).is_some());
        for t in self.dying.values_mut() {
            *t += 1;
        }
    }

    // Vanillas deathTime samt Zwischentick (LivingEntityRenderState.deathTime), nur mit
    // Todesanimation Vanilla -- sonst kippt nichts.
    pub(crate) fn death_time(&self, entity_id: i32) -> Option<f32> {
        let t = *self.dying.get(&entity_id)?;
        (self.settings.death_animation == DEATH_ANIM_VANILLA).then(|| t as f32 + 1.0 + self.tick_progress())
    }

    // Session-Info: alle anderen Spieler im Umkreis gelten als gerade getroffen.
    pub(crate) fn session_engage_near(&mut self, at: (f64, f64, f64), radius: f64) {
        let now = Instant::now();
        let own = self.own_entity_id;
        let ids: Vec<i32> = self
            .entities
            .iter()
            .filter(|(id, e)| Some(**id) != own && world::entity_type_name(e.entity_type) == Some("player"))
            .filter(|(_, e)| {
                let (x, y, z) = e.target_pos();
                (x - at.0).powi(2) + (y - at.1).powi(2) + (z - at.2).powi(2) <= radius * radius
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.session.engage(id, now);
        }
    }

    // Entity-Event 3: man selbst oder ein anderer Spieler ist gestorben.
    pub(crate) fn session_death(&mut self, entity_id: i32) {
        let now = Instant::now();
        if self.own_entity_id == Some(entity_id) {
            self.session.own_died(now);
        } else if let Some(e) = self.entities.get(entity_id).filter(|e| world::entity_type_name(e.entity_type) == Some("player")) {
            let name = self.player_names.get(&e.uuid).map(String::as_str);
            self.session.player_died(entity_id, name, now);
        }
    }

    // Titel (`addressed`) oder Chatzeile -- ungefaerbt und VOR dem Eigennamen-Umschreiben,
    // sonst stuende dort der Alias statt des Namens.
    pub(crate) fn session_text(&mut self, text: &str, addressed: bool) {
        let name = self.own_name().map(str::to_string);
        self.session.text(text, name.as_deref(), addressed, Instant::now());
    }

    // Groesse einer HUD-Einheit der Session-Info in Bildschirmpixeln (wie keys_unit).
    pub(crate) fn session_unit(&self, gpu: &GpuState) -> f32 {
        let u = self.hud_scale(gpu.size()) * self.settings.hud_session_scale as f32 / 100.0;
        (u * 4.0).round().max(2.0) / 4.0
    }

    // Was auf der Tafel steht: (Beschriftung, Wert) je eingeschalteter Zeile.
    pub(crate) fn session_lines(&self) -> Vec<(&'static str, String)> {
        let s = &self.session;
        session_info::LINES
            .iter()
            .filter(|(bit, _)| self.settings.session_lines & bit != 0)
            .map(|&(bit, label)| {
                let wert = match bit {
                    session_info::LINE_KILLS => s.kills.to_string(),
                    session_info::LINE_DEATHS => s.deaths.to_string(),
                    session_info::LINE_KD => s.kd_text(),
                    _ => s.wins.to_string(),
                };
                (label, wert)
            })
            .collect()
    }

    // Masse der Tafel in HUD-Einheiten: Kopfzeile, die grosse Uhr, darunter je Zeile eine.
    // Die Breite waechst mit der Uhr (ab 100 Stunden), sonst steht sie still.
    pub(crate) fn session_size(&self, gpu: &GpuState) -> (f32, f32) {
        let u = self.session_unit(gpu);
        let klein = gpu.text_height(u) / u;
        let gross = gpu.text_height(u * 1.5) / u;
        let uhr = gpu.measure_text(&session_info::clock_text(self.session.start.elapsed()), u * 1.5) / u;
        let w = (uhr + 2.0 * SESSION_PAD).max(SESSION_MIN_W);
        let h = SESSION_PAD + klein + 2.0 * SESSION_PAD + gross + SESSION_PAD + self.session_lines().len() as f32 * (klein + SESSION_LINE_GAP) + SESSION_PAD;
        (w, h)
    }

    pub(crate) fn hud_session_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let u = self.session_unit(gpu);
        let (w, h) = self.session_size(gpu);
        let (w, h) = ((w * u).round(), (h * u).round());
        let x = (self.settings.hud_session_x * sw).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_session_y * sh).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // Die Tafel: Kopfzeile "Session-Info" in der Akzentfarbe auf eigener Flaeche, darunter
    // die Spielzeit gross und je Zeile links die Beschriftung, rechts der Wert.
    pub(crate) fn push_session_info(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let rect = self.hud_session_rect(gpu);
        let u = self.session_unit(gpu);
        let alpha = self.settings.session_opacity as f32 / 100.0;
        let mit_alpha = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * alpha];
        let pad = (SESSION_PAD * u).round();
        let klein = gpu.text_height(u);
        rects.push(UiRect { rect, color: mit_alpha(theme::get(theme::Slot::BgPanel)), radius: 3.0 * u, ..UiRect::FLAT });
        let kopf = Rect { x: rect.x, y: rect.y, w: rect.w, h: (klein + 2.0 * pad).round() };
        rects.push(UiRect { rect: kopf, color: mit_alpha(theme::get(theme::Slot::Surface2)), radius: 3.0 * u, ..UiRect::FLAT });
        texts.push(gpu.centered_text(kopf, "Session-Info".to_string(), theme::get(theme::Slot::AccentText), u));
        let mut y = kopf.y + kopf.h + pad;
        let uhr = session_info::clock_text(self.session.start.elapsed());
        texts.push(UiText { text: uhr, x: rect.x + pad, y: y.round(), color: theme::text(), scale: u * 1.5 });
        y += gpu.text_height(u * 1.5) + pad;
        for (label, wert) in self.session_lines() {
            texts.push(UiText { text: label.to_string(), x: rect.x + pad, y: y.round(), color: theme::text(), scale: u });
            let breite = gpu.measure_text(&wert, u);
            texts.push(UiText { text: wert, x: (rect.x + rect.w - pad - breite).round(), y: y.round(), color: theme::get(theme::Slot::AccentText), scale: u });
            y += klein + SESSION_LINE_GAP * u;
        }
    }

    // Groesse einer HUD-Einheit der Keystrokes in Bildschirmpixeln.
    pub(crate) fn keys_unit(&self, gpu: &GpuState) -> f32 {
        let u = self.hud_scale(gpu.size()) * self.settings.hud_keys_scale as f32 / 100.0;
        (u * 4.0).round().max(2.0) / 4.0
    }

    pub(crate) fn hud_keys_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let u = self.keys_unit(gpu);
        let (w, h) = ((keystrokes::WIDTH * u).round(), (keystrokes::HEIGHT * u).round());
        let x = (self.settings.hud_keys_x * sw).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_keys_y * sh).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // Die Keystrokes: jede Taste eine abgerundete Flaeche in der Panel-Farbe des Themes mit
    // ihrer Belegung darauf, gedrueckt in der Akzentfarbe. Gedrueckt heisst PHYSISCH unten
    // (binds_down) -- eine umgeschaltete Sprint- oder Schleichtaste leuchtet also nicht dauernd.
    // Beim Tippen im Chat und unter Fenstern bleibt alles dunkel.
    pub(crate) fn push_keystrokes(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let rect = self.hud_keys_rect(gpu);
        let u = self.keys_unit(gpu);
        let kb = self.settings.keybinds;
        let eingabe_frei = self.mode == AppMode::World && self.chat_input.is_none() && !self.screen_overlay_open();
        let flaeche = theme::get(theme::Slot::BgPanel);
        let alpha = self.settings.keys_opacity as f32 / 100.0;
        let scale = (u * 4.0).round().max(4.0) / 4.0;
        for (taste, [x, y, w, h]) in keystrokes::layout() {
            let aktion = match taste {
                keystrokes::Key::Forward => KeyBindAction::Forward,
                keystrokes::Key::Left => KeyBindAction::Left,
                keystrokes::Key::Backward => KeyBindAction::Backward,
                keystrokes::Key::Right => KeyBindAction::Right,
                keystrokes::Key::Jump => KeyBindAction::Jump,
                keystrokes::Key::Attack => KeyBindAction::Attack,
                keystrokes::Key::Use => KeyBindAction::Use,
            };
            let unten = eingabe_frei && kb.binds(aktion).any(|b| self.binds_down.contains(&b));
            let r = Rect { x: (rect.x + x * u).round(), y: (rect.y + y * u).round(), w: (w * u).round(), h: (h * u).round() };
            let (grund, schrift) = if unten {
                (theme::get(theme::Slot::Accent), theme::get(theme::Slot::TextOnAccent))
            } else {
                ([flaeche[0], flaeche[1], flaeche[2], flaeche[3] * alpha], theme::text())
            };
            rects.push(UiRect { rect: r, color: grund, radius: 2.0 * u, ..UiRect::FLAT });
            if taste == keystrokes::Key::Jump {
                // Die Leertaste traegt statt eines Namens einen Strich.
                let (sw, sh) = ((8.0 * u).round(), u.round().max(1.0));
                rects.push(UiRect { rect: Rect { x: (r.x + (r.w - sw) / 2.0).round(), y: (r.y + (r.h - sh) / 2.0).round(), w: sw, h: sh }, color: schrift, ..UiRect::FLAT });
                continue;
            }
            let text = match kb.binds(aktion).next() {
                Some(Bind::Mouse(BindMouse::Left)) => "LMB".to_string(),
                Some(Bind::Mouse(BindMouse::Right)) => "RMB".to_string(),
                Some(bind) => bind_display(bind),
                None => "-".to_string(),
            };
            // Lange Namen so weit verkleinern, dass sie auf die Taste passen.
            let mut g = scale;
            while g > 1.0 && gpu.measure_text(&text, g) > r.w - 2.0 * u {
                g -= 0.25;
            }
            texts.push(gpu.centered_text(r, text, schrift, g));
        }
    }

    // Ruestungs-HUD im Spiel: nur mit Verbindung, nicht als Zuschauer und nur, wenn man
    // ueberhaupt etwas traegt.
    pub(crate) fn build_armor_hud(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        if !self.settings.module_armor_hud || self.conn_status != ConnStatus::Connected || self.game_mode == GAME_MODE_SPECTATOR {
            return;
        }
        let getragen = self.worn_armor();
        let teile: [Option<&protocol::item::ItemStack>; 4] = std::array::from_fn(|i| getragen[i].or_else(|| self.taken_off_armor(i)));
        if teile.iter().all(Option::is_none) {
            return;
        }
        self.push_armor_hud(gpu, teile, getragen.map(|t| t.is_none()), rects, sprites, texts);
    }

    // Einmal je Tick: das getragene Teil je Slot merken (s. armor_last_worn).
    pub(crate) fn remember_worn_armor(&mut self) {
        for i in 0..4 {
            let Some(stack) = self.worn_armor()[i] else { continue };
            if self.armor_last_worn[i].as_ref() != Some(stack) {
                self.armor_last_worn[i] = Some(stack.clone());
            }
        }
    }

    // Das zuletzt in Slot `i` getragene Teil, wenn es jetzt im Inventar liegt. Genau dieses
    // Teil (gleicher Schaden, gleiche Verzauberung ...), nicht irgendeins derselben Art.
    fn taken_off_armor(&self, i: usize) -> Option<&protocol::item::ItemStack> {
        let zuletzt = self.armor_last_worn[i].as_ref()?;
        self.inventory.player[inventory::PLAYER_MAIN_START..=inventory::PLAYER_OFFHAND].iter().flatten().find(|s| *s == zuletzt)
    }

    // Helm, Brust, Hose und Schuhe aus dem eigenen Inventar, None fuer leere Slots.
    pub(crate) fn worn_armor(&self) -> [Option<&protocol::item::ItemStack>; 4] {
        std::array::from_fn(|i| {
            self.inventory.player.get(inventory::PLAYER_ARMOR_START + i).and_then(|s| s.as_ref()).filter(|s| s.item_id != 0 && s.count > 0)
        })
    }

    // Masse des Ruestungs-HUD: Kantenlaenge eines Item-Bilds, ein Bildpixel, Textfaktor der Zahl,
    // Abstand zwischen zwei Bildern und der Rand, um den die Zahl ("000") ueber ein Bild
    // hinausragt (links, oben, rechts, unten). Gemessen in der Schrift des Bereichs
    // Ruestungs-HUD.
    pub(crate) fn armor_hud_metrics(&self, gpu: &GpuState) -> ArmorHudMetrics {
        let set = &self.settings;
        let _schrift = self.area_font_scope(gpu, ShadowArea::ArmorHud);
        let size = (16.0 * self.hud_scale(gpu.size()) * set.hud_armor_scale as f32 / 100.0).round().max(8.0);
        let px = (size / 16.0).round().max(1.0);
        let text_scale = (px * set.armor_hud_number_scale as f32 / 100.0).max(0.5);
        let text = (gpu.measure_text("000", text_scale), gpu.text_height(text_scale));
        let (nx, ny) = armor_hud::number_origin((0.0, 0.0), size, px, (set.armor_hud_number_x, set.armor_hud_number_y), text);
        let rand = [(-nx).max(0.0), (-ny).max(0.0), (nx + text.0 - size).max(0.0), (ny + text.1 - size).max(0.0)].map(f32::ceil);
        ArmorHudMetrics { size, px, text_scale, gap: set.armor_hud_gap as f32 * px, rand }
    }

    // Die vier Bilder samt Rand fuer die Zahl, ab der gespeicherten oberen linken Ecke -- gleich
    // gross, egal wie viele Teile man traegt, damit das Element beim An- und Ausziehen nicht
    // springt.
    pub(crate) fn hud_armor_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let m = self.armor_hud_metrics(gpu);
        let reihe = (4.0 * m.size + 3.0 * m.gap).max(m.size);
        let (w, h) = if armor_hud::is_vertical(self.settings.armor_hud_layout) { (m.size, reihe) } else { (reihe, m.size) };
        let (w, h) = (w + m.rand[0] + m.rand[2], h + m.rand[1] + m.rand[3]);
        let x = (self.settings.hud_armor_x * sw).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_armor_y * sh).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // Die Teile an ihren Plaetzen (s. armor_hud::cells), je mit der Haltbarkeit an der
    // eingestellten Stelle -- im Spiel und als Vorschau im HUD-Editor.
    // `ausgezogen`: das Teil liegt nur im Inventar -- sein Bild blass, die Zahl nicht. Solange
    // eins abgelegt ist, haben die noch getragenen einen gelben Rahmen.
    pub(crate) fn push_armor_hud(
        &self,
        gpu: &GpuState,
        teile: [Option<&protocol::item::ItemStack>; 4],
        ausgezogen: [bool; 4],
        rects: &mut Vec<UiRect>,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
    ) {
        let set = &self.settings;
        let rect = self.hud_armor_rect(gpu);
        let m = self.armor_hud_metrics(gpu);
        let schritt = m.size + m.gap;
        let hochkant = armor_hud::is_vertical(set.armor_hud_layout);
        let ans_ende = if hochkant { set.armor_hud_sort_vertical == 1 } else { set.armor_hud_sort_flat == 1 };
        let zellen = armor_hud::cells(teile.map(|t| t.is_some()), set.armor_hud_layout, ans_ende);
        let (x0, y0) = (rect.x + m.rand[0], rect.y + m.rand[1]);
        self.begin_text_area(gpu, texts, ShadowArea::ArmorHud);
        let text_h = gpu.text_height(m.text_scale);
        let etwas_abgelegt = teile.iter().zip(ausgezogen).any(|(t, aus)| t.is_some() && aus);
        for ((stack, zelle), ausgezogen) in teile.into_iter().zip(zellen).zip(ausgezogen) {
            let (Some(stack), Some(zelle)) = (stack, zelle) else { continue };
            let bild = if hochkant { (x0, y0 + zelle as f32 * schritt) } else { (x0 + zelle as f32 * schritt, y0) };
            if etwas_abgelegt && !ausgezogen {
                // Knapp ausserhalb des Bilds, ein Bildpixel breit.
                let r = Rect { x: bild.0.round() - m.px, y: bild.1.round() - m.px, w: m.size + 2.0 * m.px, h: m.size + 2.0 * m.px };
                rects.push(UiRect::framed(r, [0.0; 4], m.px, m.px, COLOR_ARMOR_KEPT));
            }
            let erstes = sprites.len();
            self.push_item_icon_only(sprites, stack, bild.0.round(), bild.1.round(), m.size);
            if ausgezogen {
                for s in &mut sprites[erstes..] {
                    s.color[3] *= 0.3;
                }
            }
            let Some(rest) = world::item_registry::item_name(stack.item_id).and_then(|name| durability_points(stack, name)) else { continue };
            let text = rest.to_string();
            let tw = gpu.measure_text(&text, m.text_scale);
            let (x, y) = armor_hud::number_origin(bild, m.size, m.px, (set.armor_hud_number_x, set.armor_hud_number_y), (tw, text_h));
            let max = world::item_registry::item_name(stack.item_id).and_then(|name| max_damage_of(stack, name)).unwrap_or(0);
            let color = if rest < armor_hud::LOW_DURABILITY {
                COLOR_ARMOR_LOW
            } else if set.armor_hud_green && max > 0 && rest as f32 > max as f32 * 0.87 {
                COLOR_ARMOR_HIGH
            } else {
                COLOR_WHITE
            };
            texts.push(UiText { text, x: x.round(), y: y.round(), color, scale: m.text_scale });
        }
        Self::end_text_area(gpu, texts);
    }

    // Wo die Pfeil-Anzeige sitzt: ein Quadrat um die gespeicherte Mitte, so gross wie ein
    // Hotbar-Slot mal der eigenen Groesse aus dem HUD-Editor.
    pub(crate) fn hud_arrows_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let size = (16.0 * self.hud_scale(gpu.size()) * self.settings.hud_arrows_scale as f32 / 100.0).round();
        let cx = self.settings.hud_arrows_x * sw;
        let cy = self.settings.hud_arrows_y * sh;
        Rect { x: (cx - size / 2.0).round(), y: (cy - size / 2.0).round(), w: size, h: size }
    }

    // Die Anzeige selbst -- im Spiel und als Vorschau im HUD-Editor. Mit Icon steht die Zahl
    // wie im Slot unten rechts (und fehlt bei genau einem Pfeil); ohne Icon steht sie immer da,
    // mittig im selben Quadrat.
    // `mit_zahl`: die Zahl als UI-Text mitzeichnen (Vorschau im HUD-Editor). Im Spiel kommt sie
    // aus dem Fadenkreuz-Pass (s. arrow_number).
    pub(crate) fn push_arrow_indicator(
        &self,
        gpu: &GpuState,
        stack: &protocol::item::ItemStack,
        sprites: &mut Vec<UiSprite>,
        texts: &mut Vec<UiText>,
        mit_zahl: bool,
    ) {
        let rect = self.hud_arrows_rect(gpu);
        let alpha = self.settings.arrow_opacity as f32 / 100.0;
        let scale = (rect.w / 16.0).round().max(1.0);
        let text = stack.count.to_string();
        let (w, h) = (gpu.measure_text(&text, scale), gpu.text_height(scale));
        if self.settings.arrow_number_only {
            if mit_zahl {
                let (x, y) = (rect.x + (rect.w - w) / 2.0, rect.y + (rect.h - h) / 2.0);
                texts.push(UiText { text, x: x.round(), y: y.round(), color: [1.0, 1.0, 1.0, alpha], scale });
            }
            return;
        }
        if let Some(name) = world::item_registry::item_name(stack.item_id) {
            self.item_icons.push(sprites, item_icon_name(name, stack), world::potion::stack_tint(stack), rect.x, rect.y, rect.w, alpha);
        }
        if stack.count > 1 && mit_zahl {
            texts.push(UiText { text, x: (rect.x + rect.w - w).round(), y: (rect.y + rect.h - h).round(), color: [1.0, 1.0, 1.0, alpha], scale });
        }
    }

    // Die Munition fuer die Waffe in der Hand (PlayerEntity.getProjectileType), mit der
    // Gesamtzahl gleicher Stapel als `count`. None ohne Bogen/Armbrust oder ohne Munition.
    pub(crate) fn arrow_hud_stack(&self) -> Option<protocol::item::ItemStack> {
        let ist_waffe = |hand: i32| {
            self.own_hand_stack(hand)
                .and_then(|s| world::item_registry::item_name(s.item_id))
                .filter(|n| matches!(*n, "bow" | "crossbow"))
        };
        // Vanilla fragt die Haupthand zuerst.
        let waffe = ist_waffe(protocol::packets::HAND_MAIN).or_else(|| ist_waffe(protocol::packets::HAND_OFF))?;
        self.projectile_for(waffe)
    }

    // PlayerEntity.getProjectileType fuer diese Waffe ("bow"/"crossbow"), ohne den
    // Kreativ-Ersatzpfeil: die gewaehlte Munition mit der Gesamtzahl gleicher Stapel als
    // `count`, None ohne Munition.
    pub(crate) fn projectile_for(&self, waffe: &str) -> Option<protocol::item::ItemStack> {
        let ist_pfeil = |s: &protocol::item::ItemStack| {
            matches!(world::item_registry::item_name(s.item_id), Some("arrow" | "spectral_arrow" | "tipped_arrow"))
        };
        // Die Armbrust nimmt eine Rakete nur aus einer HAND, im Inventar sucht sie nur Pfeile.
        let in_hand = |s: &protocol::item::ItemStack| {
            ist_pfeil(s) || (waffe == "crossbow" && world::item_registry::item_name(s.item_id) == Some("firework_rocket"))
        };
        let hand = |h: i32| self.own_hand_stack(h).filter(|s| in_hand(s)).cloned();
        let slot = |i: usize| self.inventory.player.get(i).and_then(|s| s.as_ref()).filter(|st| st.item_id != 0 && st.count > 0);
        let gewaehlt = hand(protocol::packets::HAND_OFF).or_else(|| hand(protocol::packets::HAND_MAIN)).or_else(|| {
            // Vanillas Inventar-Reihenfolge: Hotbar, dann die drei Reihen, dann die Zweithand.
            (0..9)
                .map(|i| inventory::PLAYER_HOTBAR_START + i)
                .chain((0..27).map(|i| inventory::PLAYER_MAIN_START + i))
                .chain(std::iter::once(inventory::PLAYER_OFFHAND))
                .filter_map(slot)
                .find(|s| ist_pfeil(s))
                .cloned()
        })?;
        let gesamt: i32 = (inventory::PLAYER_MAIN_START..=inventory::PLAYER_OFFHAND)
            .filter_map(slot)
            .filter(|s| s.item_id == gewaehlt.item_id && s.potion == gewaehlt.potion)
            .map(|s| s.count)
            .sum();
        Some(protocol::item::ItemStack { count: gesamt, ..gewaehlt })
    }

    // Vanillas popTime minus Tick-Anteil: 5 direkt nach der Aufnahme, nach fuenf Ticks 0.
    pub(crate) fn hotbar_pop_amount(&self, index: usize) -> f32 {
        self.hotbar_pop.get(index).copied().flatten().map_or(0.0, |t| (5.0 - t.elapsed().as_secs_f32() * 20.0).max(0.0))
    }

    // Den Zweithand-Slot neben der Hotbar (Vanillas hud/hotbar_offhand_*) zeichnen wir bewusst
    // nicht -- unnoetig, man sieht das Item in der Hand.
    pub(crate) fn build_hotbar(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        // Vor der ersten Inventar-Nachricht (oder ohne Verbindung) waere die Hotbar leer --
        // dann auch das Sheet weglassen, statt einen leeren Balken zu zeigen.
        // Zuschauer haben keine Hotbar (Gui: dort steht das Zuschauermenue).
        if self.conn_status != ConnStatus::Connected || self.game_mode == GAME_MODE_SPECTATOR {
            return;
        }
        let Some(hotbar) = self.ui_atlas.rect("hud/hotbar") else { return };
        let rect = self.hud_hotbar_rect(gpu);
        let scale = self.hotbar_scale(gpu.size());
        sprites.push(UiSprite::quad(rect.x, rect.y, rect.w, rect.h, hotbar.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));

        for (index, slot) in inventory::hotbar_slots((rect.x, rect.y), scale).into_iter().enumerate() {
            if let Some(stack) = self.inventory.hotbar(index) {
                let grenze = if self.settings.hotbar_durability_absolute {
                    DurabilityBarLimit::Points(self.settings.hotbar_durability_points as i32)
                } else {
                    DurabilityBarLimit::Rest(self.settings.hotbar_durability_percent as f32 / 100.0)
                };
                self.push_item_limited(sprites, texts, stack, slot.x, slot.y, slot.size, grenze, self.hotbar_pop_amount(index));
            }
        }

        // Auswahlrahmen zuletzt, damit er ueber den Icons der Nachbarslots liegt (er ragt
        // einen Pixel darueber hinaus -- genau wie in Vanilla).
        if let Some(selection) = self.ui_atlas.rect("hud/hotbar_selection") {
            let x = rect.x + (self.inventory.held_slot as f32 * 20.0 - 1.0) * scale;
            let y = rect.y - scale;
            let size = inventory::HOTBAR_SELECTION * scale;
            sprites.push(UiSprite::quad(x, y, size, size * (23.0 / 24.0), selection.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
        }
    }

    // Statusleisten ueber der Hotbar: Leben, Ruestung, Nahrung, Luft und die Erfahrungsleiste.
    //
    // Alle Masse sind Vanillas InGameHud.renderStatusBars in GUI-Einheiten, nur eben RELATIV
    // zum Hotbar-Rechteck statt absolut zum Bildschirmrand -- damit wandern die Leisten mit,
    // wenn die Hotbar im HUD-Editor verschoben wird. Die Umrechnung: Vanilla setzt die
    // Herzreihe auf `height - 39`, die Hotbar-Oberkante liegt bei `height - 22`, also 17
    // Einheiten darueber. Genauso die Erfahrungsleiste (`height - 29` = 7 darueber) und die
    // Stufenzahl (`height - 35` = 13 darueber).
    //
    // NICHT nachgebaut (bewusst, keine Luecke aus Versehen):
    //   - das Wackeln der Herzen bei wenig Leben und der Regenerations-Hupf. Beide haengen an
    //     einem Zufallsgenerator, den Vanilla mit der Tickzahl seedet; sie sind Zierrat und
    //     kosten in einem PvP-HUD nur Ruhe.
    // Sound-Debug-Modul: zwei senkrechte Pegelbalken am linken Rand, L und R. Gemessen wird
    // die Spitze des Mixer-Ausgangs (s. audio::take_peak_levels), vor der Begrenzung -- ein
    // roter Balken heisst, der Kanal uebersteuert. Die Anzeige faellt um 1,5 je Sekunde, damit
    // kurze Klaenge sichtbar bleiben.
    pub(crate) fn build_sound_meter(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if !self.settings.module_sound_debug {
            return;
        }
        let (neu_l, neu_r) = audio::take_peak_levels();
        let (alt_l, alt_r, zuletzt) = self.sound_meter.get();
        let jetzt = Instant::now();
        let fall = zuletzt.map_or(0.0, |t| jetzt.duration_since(t).as_secs_f32() * 1.5);
        let l = neu_l.max(alt_l - fall).max(0.0);
        let r = neu_r.max(alt_r - fall).max(0.0);
        self.sound_meter.set((l, r, Some(jetzt)));

        let scale = self.hud_scale(gpu.size());
        let (_, sh) = gpu.size();
        let (w, h, abstand) = (6.0 * scale, 80.0 * scale, 3.0 * scale);
        let x0 = (8.0 * scale).round();
        let y0 = ((sh - h) / 2.0).round();
        for (i, (pegel, name)) in [(l, "L"), (r, "R")].into_iter().enumerate() {
            let x = x0 + i as f32 * (w + abstand);
            rects.push(UiRect { rect: Rect { x: x - scale, y: y0 - scale, w: w + 2.0 * scale, h: h + 2.0 * scale }, color: [0.0, 0.0, 0.0, 0.6], ..UiRect::FLAT });
            let fuellung = pegel.min(1.0) * h;
            let farbe = if pegel >= 1.0 {
                [1.0, 0.2, 0.2, 1.0]
            } else if pegel >= 0.7 {
                [1.0, 0.85, 0.2, 1.0]
            } else {
                [0.3, 0.9, 0.35, 1.0]
            };
            rects.push(UiRect { rect: Rect { x, y: y0 + h - fuellung, w, h: fuellung }, color: farbe, ..UiRect::FLAT });
            let tw = gpu.measure_text(name, scale);
            texts.push(UiText { text: name.to_string(), x: (x + (w - tw) / 2.0).round(), y: y0 + h + 3.0 * scale, color: [1.0, 1.0, 1.0, 1.0], scale });
        }
    }

    // Die eigenen Effekte aufsteigend sortiert (MobEffectInstance.compareTo). `nur_symbol`:
    // nur die mit Symbol-Flag (die HUD-Anzeige; die Liste im Inventar zeigt alle).
    pub(crate) fn effect_entries(&self, nur_symbol: bool) -> Vec<EffectEntry> {
        let now = Instant::now();
        let mut liste: Vec<EffectEntry> = self
            .shown_effects
            .iter()
            .filter(|(_, e)| !nur_symbol || e.show_icon())
            .map(|(&effect, e)| EffectEntry { effect, ambient: e.ambient(), amplifier: e.amplifier, remaining: e.remaining(now), exact: e.remaining_exact(now), received: e.duration })
            .collect();
        liste.sort_by(effect_order);
        liste
    }

    // Was die HUD-Anzeige zeigt, auf Wunsch ohne ambiente Effekte. Im Editor ohne echte
    // Effekte Beispiele (Schnelligkeit, Staerke, Vergiftung), damit es etwas zum Anfassen gibt.
    pub(crate) fn hud_effect_list(&self, beispiel: bool) -> Vec<EffectEntry> {
        let mut liste = self.effect_entries(true);
        if self.settings.effects_hide_ambient {
            liste.retain(|e| !e.ambient);
        }
        if !liste.is_empty() || !beispiel {
            return liste;
        }
        let e = |effect: i32, remaining: i32| EffectEntry { effect, ambient: false, amplifier: 0, remaining, exact: remaining as f64, received: remaining };
        let mut liste = vec![e(protocol::packets::MOB_EFFECT_SPEED, 3600), e(4, 2400), e(18, 900)];
        liste.sort_by(effect_order);
        liste
    }

    // Detaillierte Effektanzeige: je Effekt Icon, Name und Timer in einer Zeile. Ab dem festen
    // Rand (Settings::effects_align) untereinander -- die Timer dann als Spalte hinter dem
    // laengsten Namen -- bzw. nebeneinander. `g` = Bildschirmpixel je GUI-Pixel. Rueckgabe:
    // Plaetze relativ zur linken oberen Ecke und die Groesse der Anzeige.
    pub(crate) fn effect_detail_layout(&self, gpu: &GpuState, liste: &[EffectEntry], g: f32) -> (Vec<EffectDetailPlace>, f32, f32) {
        let set = &self.settings;
        let (symbol, nach_symbol) = if set.effects_icon { (18.0 * g, 4.0 * g) } else { (0.0, 0.0) };
        let luecke = if set.effects_name { 6.0 * g } else { 0.0 };
        let zeile = symbol.max(gpu.text_height(g));
        let breite = |t: &str| gpu.measure_text(t, g);
        // Der laengste Effekt zuerst, also am festen Rand: laeuft ein kuerzerer ab, verschwindet
        // er hinten, und die laengeren bleiben stehen, statt nachzuruecken.
        let mut liste = liste.to_vec();
        liste.sort_by(effect_longest_first);
        let mut plaetze: Vec<EffectDetailPlace> = liste
            .iter()
            .map(|e| EffectDetailPlace {
                entry: *e,
                x: 0.0,
                y: 0.0,
                h: zeile,
                name: if set.effects_name { effect_display_name(e) } else { String::new() },
                name_x: symbol + nach_symbol,
                timer: format_effect_timer(e.remaining, set.effects_timer_style),
                timer_x: 0.0,
            })
            .collect();
        let n = plaetze.len();
        if n == 0 {
            return (plaetze, 0.0, 0.0);
        }
        if matches!(set.effects_align, EFFECTS_ALIGN_LEFT | EFFECTS_ALIGN_RIGHT) {
            let abstand = 10.0 * g;
            let breiten: Vec<f32> = plaetze.iter().map(|p| symbol + nach_symbol + breite(&p.name) + luecke + breite(&p.timer)).collect();
            let gesamt = breiten.iter().sum::<f32>() + (n - 1) as f32 * abstand;
            let mut x = 0.0;
            for (p, w) in plaetze.iter_mut().zip(breiten) {
                p.x = if set.effects_align == EFFECTS_ALIGN_RIGHT { gesamt - x - w } else { x };
                p.name_x = p.x + symbol + nach_symbol;
                p.timer_x = p.name_x + breite(&p.name) + luecke;
                x += w + abstand;
            }
            return (plaetze, gesamt, zeile);
        }
        let abstand = 2.0 * g;
        let name_w = plaetze.iter().map(|p| breite(&p.name)).fold(0.0, f32::max);
        let timer_w = plaetze.iter().map(|p| breite(&p.timer)).fold(0.0, f32::max);
        for (i, p) in plaetze.iter_mut().enumerate() {
            let reihe = if set.effects_align == EFFECTS_ALIGN_BOTTOM { n - 1 - i } else { i };
            p.y = reihe as f32 * (zeile + abstand);
            p.timer_x = p.name_x + name_w + luecke;
        }
        (plaetze, symbol + nach_symbol + name_w + luecke + timer_w, n as f32 * (zeile + abstand) - abstand)
    }

    // Gui.extractEffects: je Effekt der 24x24-Rahmen und darin das 18x18-Symbol -- oder die
    // detaillierte Liste (s. effect_detail_layout). `deckkraft` blasst alles ab (Editor bei
    // ausgeschalteter Anzeige).
    pub(crate) fn build_effect_hud(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>, beispiel: bool, deckkraft: f32) {
        let liste = self.hud_effect_list(beispiel);
        if self.settings.effects_detailed {
            self.begin_text_area(gpu, texts, ShadowArea::Effects);
            let g = self.effects_scale(gpu.size());
            let rect = self.hud_effects_rect(gpu, beispiel);
            let text_h = gpu.text_height(g);
            let set = &self.settings;
            for p in self.effect_detail_layout(gpu, &liste, g).0 {
                let (x, y) = (rect.x + p.x, rect.y + p.y);
                if set.effects_icon {
                    let symbol = format!("mob_effect/{}", protocol::packets::mob_effect_name(p.entry.effect));
                    // Ohne Vanillas Blinken: das Ende zeigt hier der Timer.
                    if let Some(r) = self.ui_atlas.rect(&symbol) {
                        sprites.push(UiSprite::quad(x.round(), (y + (p.h - 18.0 * g) / 2.0).round(), 18.0 * g, 18.0 * g, r.uv, [1.0, 1.0, 1.0, deckkraft], SpriteAtlas::Ui));
                    }
                }
                let farbe = |effekt: bool| {
                    let [r, gr, b, _] = if effekt { rgb_to_rgba(world::potion::effect_color(p.entry.effect)) } else { [1.0; 4] };
                    [r, gr, b, deckkraft]
                };
                let ty = (y + (p.h - text_h) / 2.0).round();
                // Dunkle Effektfarben (Absorption, Dunkelheit, Blindheit ...) sind auf dunklem
                // Grund kaum zu lesen: dort bekommt farbiger Text einen weissen Rand statt des
                // schwarzen (s. effect_color_is_dark).
                let dunkel = effect_is_dark(p.entry.effect);
                let weisser_rand = |texts: &mut Vec<UiText>, farbig: bool| {
                    if dunkel && farbig {
                        push_span_shadow(texts, Some(0xFFFF_FFFF));
                    }
                };
                if !p.name.is_empty() {
                    let name_farbe = farbe(set.effects_name_color);
                    weisser_rand(texts, set.effects_name_color);
                    texts.push(UiText { text: p.name, x: (rect.x + p.name_x).round(), y: ty, color: name_farbe, scale: g });
                }
                let timer_farbe = match effect_timer_blink(&p.entry, set.effects_timer_blink) {
                    Some(rot) => {
                        let [r, gr, b, _] = if rot { rgb_to_rgba(EFFECTS_BLINK_RED) } else { [1.0; 4] };
                        [r, gr, b, deckkraft]
                    }
                    None => {
                        weisser_rand(texts, set.effects_timer_color);
                        farbe(set.effects_timer_color)
                    }
                };
                texts.push(UiText { text: p.timer, x: (rect.x + p.timer_x).round(), y: ty, color: timer_farbe, scale: g });
            }
            Self::end_text_area(gpu, texts);
            return;
        }
        let (plaetze, breite, _) = effect_hud_layout(&liste);
        if plaetze.is_empty() {
            return;
        }
        let rect = self.hud_effects_rect(gpu, beispiel);
        let g = self.effects_scale(gpu.size());
        let rechts = rect.x + breite * g;
        for (e, gx, gy) in plaetze {
            let (x, y) = ((rechts + gx * g).round(), (rect.y + gy * g).round());
            let rahmen = if e.ambient { "hud/effect_background_ambient" } else { "hud/effect_background" };
            if let Some(r) = self.ui_atlas.rect(rahmen) {
                sprites.push(UiSprite::quad(x, y, 24.0 * g, 24.0 * g, r.uv, [1.0, 1.0, 1.0, deckkraft], SpriteAtlas::Ui));
            }
            let symbol = format!("mob_effect/{}", protocol::packets::mob_effect_name(e.effect));
            if let Some(r) = self.ui_atlas.rect(&symbol) {
                let a = effect_icon_alpha(&e) * deckkraft;
                sprites.push(UiSprite::quad(x + 3.0 * g, y + 3.0 * g, 18.0 * g, 18.0 * g, r.uv, [1.0, 1.0, 1.0, a], SpriteAtlas::Ui));
            }
        }
    }

    // Neun-Teile-Sprite (gui.scaling nine_slice): Ecken unverzerrt, Kanten und Mitte gestreckt.
    // `rand` in Sprite-Pixeln, `g` = Bildschirmpixel je Sprite-Pixel.
    pub(crate) fn push_nine_slice(&self, sprites: &mut Vec<UiSprite>, name: &str, x: f32, y: f32, w: f32, h: f32, rand: u32, g: f32) {
        let Some(r) = self.ui_atlas.rect(name) else { return };
        let (sw, sh) = (r.width, r.height);
        let rp = rand as f32 * g;
        let spalten = [(0, rand, x, rp), (rand, sw - 2 * rand, x + rp, w - 2.0 * rp), (sw - rand, rand, x + w - rp, rp)];
        let zeilen = [(0, rand, y, rp), (rand, sh - 2 * rand, y + rp, h - 2.0 * rp), (sh - rand, rand, y + h - rp, rp)];
        for (qy, qh, zy, zh) in zeilen {
            for (qx, qw, zx, zw) in spalten {
                if zw > 0.0 && zh > 0.0 {
                    sprites.push(UiSprite::quad(zx, zy, zw, zh, r.sub(qx, qy, qw, qh), [1.0; 4], SpriteAtlas::Ui));
                }
            }
        }
    }

    // EffectsInInventory: die Liste rechts neben dem Spielerinventar. Ist genug Platz (120
    // GUI-Pixel), stehen Name und Restdauer neben dem Symbol, sonst nur das Symbol -- und der
    // Text als Hinweis unter dem Zeiger. Ab sechs Effekten ruecken die Zeilen zusammen.
    pub(crate) fn build_inventory_effects(&self, gpu: &GpuState, layout: &inventory::ScreenLayout, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let liste = self.effect_entries(false);
        let g = layout.scale;
        let x0 = layout.bg_x + layout.bg_w + 2.0 * g;
        let platz = ((gpu.size().0 - x0) / g).floor();
        if liste.is_empty() || platz < 32.0 {
            return;
        }
        let max_breite = if platz >= 120.0 { platz - 7.0 } else { 32.0 };
        let schritt = if liste.len() > 5 { (132 / (liste.len() - 1)) as f32 } else { 33.0 };
        let breite_von = |text: &str| gpu.measure_text(text, g) / g;
        let (cx, cy) = self.cursor_pos;
        let mut hinweis: Option<(String, String)> = None;
        let mut y0 = layout.bg_y;
        for e in &liste {
            let name = effect_display_name(e);
            let dauer = format_effect_duration(e.remaining);
            let breite = max_breite.min((32.0 + breite_von(&name) + 7.0).max(32.0 + breite_von(&dauer) + 7.0));
            let rahmen = if e.ambient { "container/inventory/effect_background_ambient" } else { "container/inventory/effect_background" };
            self.push_nine_slice(sprites, rahmen, x0, y0, breite * g, 32.0 * g, 4, g);
            let text_breite = breite - 32.0 - 7.0;
            let kompakt = if text_breite > 0.0 {
                let gekuerzt = breite_von(&name) > text_breite;
                let zeile = if gekuerzt { clip_with_ellipsis(&name, text_breite, &breite_von) } else { name.clone() };
                let tx = (x0 + 32.0 * g).round();
                texts.push(UiText { text: zeile, x: tx, y: (y0 + 7.0 * g).round(), color: [1.0; 4], scale: g });
                texts.push(UiText { text: dauer.clone(), x: tx, y: (y0 + 16.0 * g).round(), color: rgb_to_rgba(0x808080), scale: g });
                gekuerzt
            } else {
                true
            };
            if kompakt && cx >= x0 && cx <= x0 + breite * g && cy >= y0 && cy <= y0 + schritt * g {
                hinweis = Some((name, dauer));
            }
            let symbol = format!("mob_effect/{}", protocol::packets::mob_effect_name(e.effect));
            if let Some(r) = self.ui_atlas.rect(&symbol) {
                sprites.push(UiSprite::quad(x0 + 7.0 * g, y0 + 7.0 * g, 18.0 * g, 18.0 * g, r.uv, [1.0; 4], SpriteAtlas::Ui));
            }
            y0 += schritt * g;
        }
        // Der Hinweis mit Vanillas dunklem Tooltip-Grund. Der Grund ist ein Sprite (die
        // weisse Flaeche, getoent), weil Rechtecke VOR den Sprites gezeichnet werden und
        // sonst unter den Effektrahmen laegen.
        if let (Some((name, dauer)), Some(weiss)) = (hinweis, self.ui_atlas.rect(assets::UI_WHITE)) {
            let zeilen_h = gpu.text_height(UI_SCALE_SMALL);
            let breite = gpu.measure_text(&name, UI_SCALE_SMALL).max(gpu.measure_text(&dauer, UI_SCALE_SMALL));
            let (tx, ty) = ((cx + 12.0).round(), (cy - 12.0).round());
            sprites.push(UiSprite::quad(tx - 4.0, ty - 4.0, breite + 8.0, zeilen_h * 2.0 + 8.0, weiss.sub(1, 1, 2, 2), [0.06, 0.0, 0.06, 0.94], SpriteAtlas::Ui));
            texts.push(UiText { text: name, x: tx, y: ty, color: [1.0; 4], scale: UI_SCALE_SMALL });
            texts.push(UiText { text: dauer, x: tx, y: (ty + zeilen_h).round(), color: rgb_to_rgba(0x808080), scale: UI_SCALE_SMALL });
        }
    }

    // Saettigungs-Modul: ein Balken direkt ueber der Nahrung, so breit wie ihre zehn Symbole,
    // mit runden Enden und duennen schwarzen Linien. Zehn Felder zu je 2 Punkten, von rechts
    // gefuellt wie die Nahrung; ein ungerader Rest fuellt ein halbes Feld. Leere Felder sind
    // durchsichtig.
    pub(crate) fn build_saturation_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>) {
        // Ziel in halben Feldern, angezeigt wird ein weich nachlaufender Wert (Halbwertszeit
        // rund 70 ms) -- der Balken gleitet statt zu springen.
        let ziel = (self.food_saturation.max(0.0).floor()).min(20.0);
        let jetzt = Instant::now();
        let gezeigt = match self.saturation_shown.get() {
            Some((alt, wann)) => {
                let dt = jetzt.duration_since(wann).as_secs_f32().min(0.5);
                let neu = alt + (ziel - alt) * (1.0 - (-dt * 10.0).exp());
                if (neu - ziel).abs() < 0.01 { ziel } else { neu }
            }
            None => ziel,
        };
        self.saturation_shown.set(Some((gezeigt, jetzt)));
        let oben = -STATUS_BARS_ABOVE_HOTBAR - self.settings.saturation_hud_height as f32 - 1.0;
        self.push_status_bar(gpu, rects, oben, gezeigt, self.settings.saturation_hud_rgb());
    }

    // Luft als Balken (Hotbar-Einstellung "Sauerstoff"): wie der Saettigungsbalken, blau, dort,
    // wo sonst die Blasen stehen -- nur, solange die Luft nicht voll ist.
    pub(crate) fn build_air_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>) {
        if self.conn_status != ConnStatus::Connected || self.air_ticks >= MAX_AIR_TICKS {
            return;
        }
        let dicke = self.settings.saturation_hud_height as f32 + 1.0;
        let saettigung_h = if self.settings.module_saturation_hud { dicke } else { 0.0 };
        let halbe = self.air_ticks.max(0) as f32 * 20.0 / MAX_AIR_TICKS as f32;
        self.push_status_bar(gpu, rects, -STATUS_BARS_ABOVE_HOTBAR - saettigung_h - dicke, halbe, COLOR_AIR_BAR);
    }

    // Ein Balken ueber der Nahrung, so breit wie ihre zehn Symbole, mit Oberkante `oben` (GUI-
    // Punkte relativ zur Hotbar), `gezeigt` halben Feldern Fuellung von rechts und Hoehe, Grund
    // und Raster aus den Einstellungen des Saettigungsbalkens.
    fn push_status_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, oben: f32, gezeigt: f32, rgb: [f32; 3]) {
        let rect = self.hud_hotbar_rect(gpu);
        let scale = self.hotbar_scale(gpu.size());
        let links = STATUS_BAR_WIDTH - 9.0 * STATUS_ICON_PITCH - STATUS_ICON;
        let dicke = self.settings.saturation_hud_height as f32;
        let px = |g: f32| (rect.x + g * scale).round();
        let (x0, x1) = (px(links), px(STATUS_BAR_WIDTH));
        let y0 = (rect.y + oben * scale).round();
        let h = (dicke * scale).round();
        let radius = h / 2.0;
        let linie = (scale * 0.5).round().max(1.0);
        let raster = [0.0, 0.0, 0.0, self.settings.saturation_hud_grid_alpha as f32 / 100.0];
        // Felder von rechts: Feld j reicht von grenze(j + 1) bis grenze(j).
        let grenze = |j: f32| x1 - (x1 - x0) * j / 10.0;
        // Grund und Fuellung liegen INNERHALB des Rahmens: um die Linienbreite eingerueckt, mit
        // dem entsprechend kleineren Radius -- sonst schauen sie an den runden Enden heraus.
        let innen = Rect { x: x0 + linie, y: y0 + linie, w: x1 - x0 - 2.0 * linie, h: h - 2.0 * linie };
        let innen_radius = (radius - linie).max(0.0);
        if self.settings.saturation_hud_bg_alpha > 0 {
            rects.push(UiRect::rounded(innen, [0.0, 0.0, 0.0, self.settings.saturation_hud_bg_alpha as f32 / 100.0], innen_radius));
        }
        if gezeigt > 0.0 {
            let fuell_links = grenze(gezeigt / 2.0).max(innen.x);
            let fuellung = Rect { x: fuell_links, w: innen.x + innen.w - fuell_links, ..innen };
            rects.push(UiRect::rounded(fuellung, [rgb[0], rgb[1], rgb[2], 1.0], innen_radius.min(fuellung.w / 2.0)));
        }
        if raster[3] > 0.0 {
            for j in 1..10 {
                let x = grenze(j as f32).round();
                rects.push(UiRect::rounded(Rect { x: x - linie / 2.0, y: y0 + linie, w: linie, h: h - 2.0 * linie }, raster, 0.0));
            }
            rects.push(UiRect::framed(Rect { x: x0, y: y0, w: x1 - x0, h }, [0.0; 4], radius, linie, raster));
        }
    }

    pub(crate) fn build_status_bars(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        let rect = self.hud_hotbar_rect(gpu);
        let scale = self.hotbar_scale(gpu.size());
        // Ein 9x9-Symbol an eine GUI-Koordinate setzen, mit Deckkraft.
        let icon_alpha = |sprites: &mut Vec<UiSprite>, name: &str, gx: f32, gy: f32, alpha: f32| {
            if let Some(sprite) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(
                    (rect.x + gx * scale).round(),
                    (rect.y + gy * scale).round(),
                    STATUS_ICON * scale,
                    STATUS_ICON * scale,
                    sprite.uv,
                    [1.0, 1.0, 1.0, alpha],
                    SpriteAtlas::Ui,
                ));
            }
        };
        let icon = |sprites: &mut Vec<UiSprite>, name: &str, gx: f32, gy: f32| icon_alpha(sprites, name, gx, gy, 1.0);
        let jetzt = Instant::now();
        // Herzart wie Gui.HeartType.forPlayer: Gift vor Wither vor Frost (komplett eingefroren
        // = Entity.isFullyFrozen, 140 Ticks).
        let art = if self.shown_effects.contains_key(&protocol::packets::MOB_EFFECT_POISON) {
            "poisoned_"
        } else if self.shown_effects.contains_key(&protocol::packets::MOB_EFFECT_WITHER) {
            "withered_"
        } else if self.own_frozen_ticks >= 140 {
            "frozen_"
        } else {
            ""
        };
        let herz = |name: &str| heart_variant(name, art, self.hardcore);
        let behaelter = herz("hud/heart/container");

        // Zeilenaufteilung der Herzen: mehr als 10 Herzen stapeln sich, und je mehr Zeilen es
        // werden, desto enger ruecken sie zusammen (Vanilla: max(10 - (rows - 2), 3)).
        // Vanilla rundet AUF (InGameHud.renderStatusBars: MathHelper.ceil(getHealth())) --
        // abgerundet fehlte bei krummen Werten (UHC ohne Regeneration: 19.3) ein halbes Herz.
        let health = self.health.min(self.max_health).ceil();
        let hearts = ((self.max_health / 2.0).ceil() as i32).max(1);
        // Absorption (InGameHud.renderHealthBar): eigene goldene Herzen HINTER den normalen,
        // mit leerem Behaelter darunter -- und sie zaehlen bei der Zeilenzahl mit, damit
        // ruecken auch Herzzeilen und Ruestung zusammen bzw. hoch.
        let absorption = self.absorption.ceil() as i32;
        let absorption_hearts = (absorption as f32 / 2.0).ceil() as i32;
        let rows = (((self.max_health + absorption as f32) / 2.0 / 10.0).ceil() as i32).max(1);
        let line_height = (10 - (rows - 2)).max(3) as f32;
        let heart_row_y = -STATUS_BARS_ABOVE_HOTBAR;

        // Seit wann jedes Absorptionsherz da ist: verschwundene vergessen, neue ab jetzt.
        let seit = {
            let mut seit = self.absorption_since.borrow_mut();
            seit.truncate(absorption_hearts as usize);
            seit.resize(absorption_hearts as usize, jetzt);
            seit.clone()
        };

        // Von hinten nach vorn, damit die obere Zeile unter der unteren liegt -- genau wie
        // Vanillas absteigende Schleife.
        for l in (0..hearts + absorption_hearts).rev() {
            let gx = (l % 10) as f32 * STATUS_ICON_PITCH;
            let gy = heart_row_y - (l / 10) as f32 * line_height;
            if l >= hearts {
                // Neue Absorptionsherzen blenden ein und gleiten dabei von unten an ihren Platz.
                let t = (jetzt.duration_since(seit[(l - hearts) as usize]).as_secs_f32() / ABSORPTION_FADE_SECS).min(1.0);
                let t = 1.0 - (1.0 - t) * (1.0 - t);
                let gy = gy + (1.0 - t) * ABSORPTION_FADE_RISE;
                let sprite = absorbing_heart_sprite(absorption, l - hearts);
                if sprite == Some("hud/heart/absorbing_half") {
                    // Beim halben nur die linke Haelfte des Behaelters: rechts umrandet er
                    // kein Herz.
                    if let Some(behaelter) = self.ui_atlas.rect(&behaelter) {
                        sprites.push(UiSprite::quad(
                            (rect.x + gx * scale).round(),
                            (rect.y + gy * scale).round(),
                            5.0 * scale,
                            STATUS_ICON * scale,
                            behaelter.sub(0, 0, 5, 9),
                            [1.0, 1.0, 1.0, t],
                            SpriteAtlas::Ui,
                        ));
                    }
                } else {
                    icon_alpha(sprites, &behaelter, gx, gy, t);
                }
                if let Some(sprite) = sprite {
                    icon_alpha(sprites, &herz(sprite), gx, gy, t);
                }
            } else {
                icon(sprites, &behaelter, gx, gy);
                if let Some(sprite) = heart_sprite(health, l) {
                    icon(sprites, &herz(sprite), gx, gy);
                }
            }
        }

        // Ruestung: eine Zeile ueber der obersten Herzzeile. Ohne Ruestung zeichnet Vanilla
        // gar nichts -- auch keine leeren Symbole. Aendert sich die Zahl der Herzzeilen, gleitet
        // sie zum neuen Platz (wie der Saettigungsbalken, s. build_saturation_bar).
        let ziel = heart_row_y - (rows - 1) as f32 * line_height - STATUS_ROW_PITCH;
        let armor_y = match self.armor_row_shown.get() {
            Some((alt, wann)) => {
                let dt = jetzt.duration_since(wann).as_secs_f32().min(0.5);
                let neu = alt + (ziel - alt) * (1.0 - (-dt * 12.0).exp());
                if (neu - ziel).abs() < 0.01 { ziel } else { neu }
            }
            None => ziel,
        };
        self.armor_row_shown.set(Some((armor_y, jetzt)));
        if self.armor > 0 {
            let gy = armor_y;
            for l in 0..10 {
                let gx = l as f32 * STATUS_ICON_PITCH;
                icon(sprites, armor_sprite(self.armor, l), gx, gy);
            }
        }

        // Nahrung: rechte Seite, von rechts nach links aufgefuellt.
        for j in 0..10 {
            let gx = STATUS_BAR_WIDTH - j as f32 * STATUS_ICON_PITCH - STATUS_ICON;
            icon(sprites, "hud/food_empty", gx, heart_row_y);
            if let Some(sprite) = food_sprite(self.food_level, j) {
                icon(sprites, sprite, gx, heart_row_y);
            }
        }

        // Platz fuer den Saettigungsbalken (s. build_saturation_bar): die Luft rueckt darueber.
        let saettigung_h = if self.settings.module_saturation_hud { self.settings.saturation_hud_height as f32 + 1.0 } else { 0.0 };

        // Luft: nur wenn sie nicht voll ist -- ueber der Nahrungsleiste. Als Balken zeichnet
        // sie build_air_bar. Vanilla blendet sie
        // ausserdem beim Auftauchen noch kurz ein, solange sie sich wieder fuellt; genau das
        // ergibt sich hier von selbst, weil der Server den steigenden Wert mitschickt.
        if self.air_ticks < MAX_AIR_TICKS && !self.settings.hotbar_air_bar {
            // Genau EINE Zeile ueber der Nahrung. In Vanilla steht das verschraenkt da: erst
            // `r = n - 10`, dann wegen der gezeichneten Nahrungsleiste noch einmal `r -= 10`,
            // und getAirBubbleY() addiert ueber `getHeartRows(0) - 1 = -1` zehn wieder drauf.
            // Unterm Strich bleibt eine Zeile -- die beiden letzten Schritte heben sich auf,
            // solange man auf keinem Reittier sitzt (Reittiere fuehrt der Client nicht).
            let gy = heart_row_y - STATUS_ROW_PITCH - saettigung_h;
            let voll = air_bubbles(self.air_ticks);
            for n in 1..=10 {
                let gx = STATUS_BAR_WIDTH - (n - 1) as f32 * STATUS_ICON_PITCH - STATUS_ICON;
                if n <= voll {
                    icon(sprites, "hud/air", gx, gy);
                } else if n == voll + 1 {
                    // Die eine Blase, die gerade platzt.
                    icon(sprites, "hud/air_bursting", gx, gy);
                }
            }
        }

        self.build_experience_bar(gpu, rect, scale, sprites, texts);
    }

    // Bossbars (BossBarHud.render): je Bar 182x5, die erste 12 unter der Oberkante, der Name 9
    // darueber, die naechste 19 tiefer; Schluss, sobald ein Drittel der Bildhoehe
    // durchlaufen ist. Die Lage kommt aus dem HUD-Editor (s. hud_bossbar_rect) -- im Standard
    // genau Vanillas oben mittig.
    pub(crate) fn build_boss_bars(&self, gpu: &GpuState, bars: &[BossBarState], sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        if bars.is_empty() {
            return;
        }
        const BREITE: f32 = 182.0;
        const HOEHE: f32 = 5.0;
        let (sw, sh) = gpu.size();
        let scale = self.bossbar_scale((sw, sh));
        let rect = self.hud_bossbar_rect(gpu);
        let x = rect.x;
        // Der Name mittig ueber der Bar -- im Standard Vanillas Bildschirmmitte, die von der
        // Bar-Mitte um bis zu einen GUI-Pixel abweicht (die Bar steht auf ganzen GUI-Pixeln).
        let mitte = (self.settings.hud_bossbar_x * sw).clamp(rect.x + rect.w / 2.0 - scale, rect.x + rect.w / 2.0 + scale);
        let mut y = 12.0f32;
        let jetzt = Instant::now();
        self.begin_text_area(gpu, texts, ShadowArea::Title);
        for bar in bars {
            let sy = (rect.y + y * scale).round();
            let lage = |sprites: &mut Vec<UiSprite>, name: &str, w: f32| {
                if let Some(r) = self.ui_atlas.rect(name) {
                    sprites.push(UiSprite::quad(x.round(), sy, w * scale, HOEHE * scale, r.sub(0, 0, w as u32, HOEHE as u32), [1.0; 4], SpriteAtlas::Ui));
                }
            };
            let farbe = BOSS_BAR_COLORS.get(bar.color as usize).copied().unwrap_or("white");
            let kerben = (bar.style >= 1).then(|| BOSS_BAR_NOTCHES.get(bar.style as usize - 1).copied()).flatten();
            lage(sprites, &format!("boss_bar/{farbe}_background"), BREITE);
            if let Some(k) = kerben {
                lage(sprites, &format!("boss_bar/notched_{k}_background"), BREITE);
            }
            // MathHelper.lerpPositive(percent, 0, 182): abgerundet, mindestens 0.
            let gefuellt = (bar.percent(jetzt) * BREITE).floor().clamp(0.0, BREITE);
            if gefuellt > 0.0 {
                lage(sprites, &format!("boss_bar/{farbe}_progress"), gefuellt);
                if let Some(k) = kerben {
                    lage(sprites, &format!("boss_bar/notched_{k}_progress"), gefuellt);
                }
            }
            // Auf Viertel gerundet wie title_scale (bei ganzer GUI-Skala unveraendert).
            let text_scale = ((scale * 4.0).round() / 4.0).max(1.0);
            let name = self.ego_spans(&bar.name);
            let w = span_row_width(gpu, &name, text_scale);
            let tx = (mitte - w / 2.0).round();
            let ty = (rect.y + (y - 9.0) * scale).round();
            let weiss = self.ui_atlas.rect(assets::UI_WHITE).map(|r| r.uv);
            push_styled_row(gpu, &name, tx, ty, COLOR_WHITE, text_scale, texts, &mut |r, c| {
                if let Some(uv) = weiss {
                    sprites.push(UiSprite::quad(r.x, r.y, r.w, r.h, uv, c, SpriteAtlas::Ui));
                }
            });
            y += 19.0;
            if y * scale >= sh / 3.0 {
                break;
            }
        }
        Self::end_text_area(gpu, texts);
    }

    // Wie viele Bossbars build_boss_bars zeichnet (mindestens eine, fuer den Platzhalter).
    pub(crate) fn boss_bars_drawn(&self, scale: f32, sh: f32) -> usize {
        let mut n = 0;
        let mut y = 12.0f32;
        for _ in 0..self.boss_bars.len().max(1) {
            n += 1;
            y += 19.0;
            if y * scale >= sh / 3.0 {
                break;
            }
        }
        n
    }

    // Griff-/Zeichen-Rechteck der Bossbars: 182 GUI-Pixel breit, vom ersten Namen (3 unter der
    // Oberkante, darueber frei) bis unter die letzte Bar. hud_bossbar_x ist die Mitte, auf
    // ganze GUI-Pixel wie Vanillas `scaledWidth / 2 - 91`.
    pub(crate) fn hud_bossbar_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let scale = self.bossbar_scale((sw, sh));
        let w = 182.0 * scale;
        let h = (19.0 * self.boss_bars_drawn(scale, sh) as f32 - 2.0) * scale;
        let x = (((self.settings.hud_bossbar_x * sw / scale).floor() - 91.0) * scale).clamp(0.0, (sw - w).max(0.0)).round();
        let y = (self.settings.hud_bossbar_y * sh).clamp(0.0, (sh - h).max(0.0)).round();
        Rect { x, y, w, h }
    }

    // BossBarS2CPacket auf die Liste anwenden (BossBarHud.handlePacket). Aenderungen fuer
    // eine unbekannte Bar laufen ins Leere.
    pub(crate) fn on_boss_event(&mut self, uuid: [u8; 16], event: protocol::packets::BossEvent) {
        use protocol::packets::BossEvent as E;
        let jetzt = Instant::now();
        if let E::Add { name, progress, color, style, .. } = event {
            let neu = BossBarState { uuid, name, shown: progress, target: progress, set_at: jetzt, color, style };
            match self.boss_bars.iter_mut().find(|b| b.uuid == uuid) {
                Some(b) => *b = neu,
                None => self.boss_bars.push(neu),
            }
            return;
        }
        if event == E::Remove {
            self.boss_bars.retain(|b| b.uuid != uuid);
            return;
        }
        let Some(bar) = self.boss_bars.iter_mut().find(|b| b.uuid == uuid) else { return };
        match event {
            E::Progress(p) => {
                // ClientBossBar.setPercent: vom gerade angezeigten Stand aus 100 ms zum neuen.
                bar.shown = bar.percent(jetzt);
                bar.target = p;
                bar.set_at = jetzt;
            }
            E::Name(name) => bar.name = name,
            E::Style { color, style } => {
                bar.color = color;
                bar.style = style;
            }
            E::Flags(_) | E::Add { .. } | E::Remove => {}
        }
    }

    // Erfahrungsleiste samt Stufenzahl (Vanilla: hud.bar.ExperienceBar + Bar.drawExperienceLevel).
    pub(crate) fn build_experience_bar(&self, gpu: &GpuState, rect: Rect, scale: f32, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let Some(background) = self.ui_atlas.rect("hud/experience_bar_background") else { return };
        let y = (rect.y - XP_BAR_ABOVE_HOTBAR * scale).round();
        sprites.push(UiSprite::quad(
            rect.x,
            y,
            STATUS_BAR_WIDTH * scale,
            XP_BAR_HEIGHT * scale,
            background.uv,
            [1.0, 1.0, 1.0, 1.0],
            SpriteAtlas::Ui,
        ));
        // Vanilla rechnet mit 183 (nicht 182) und schneidet den Ueberhang ab -- so ist die
        // Leiste bei Fortschritt 1.0 sicher ganz voll und nicht einen Pixel zu kurz.
        let gefuellt = (self.xp_progress * (STATUS_BAR_WIDTH + 1.0)).floor().min(STATUS_BAR_WIDTH);
        if gefuellt >= 1.0 {
            if let Some(progress) = self.ui_atlas.rect("hud/experience_bar_progress") {
                sprites.push(UiSprite::quad(
                    rect.x,
                    y,
                    gefuellt * scale,
                    XP_BAR_HEIGHT * scale,
                    progress.sub(0, 0, gefuellt as u32, XP_BAR_HEIGHT as u32),
                    [1.0, 1.0, 1.0, 1.0],
                    SpriteAtlas::Ui,
                ));
            }
        }

        if self.xp_level > 0 {
            let text = self.xp_level.to_string();
            let text_scale = scale.round().max(1.0);
            let width = gpu.measure_text(&text, text_scale);
            let x = (rect.x + (STATUS_BAR_WIDTH * scale - width) / 2.0).round();
            let ty = (rect.y - XP_LEVEL_ABOVE_HOTBAR * scale).round();
            // Vanilla zeichnet die Zahl vier Mal schwarz um einen Pixel versetzt und einmal
            // gruen obendrauf -- ein Umriss, damit sie auf jedem Untergrund lesbar bleibt.
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                texts.push(UiText {
                    text: text.clone(),
                    x: x + dx * scale,
                    y: ty + dy * scale,
                    color: [0.0, 0.0, 0.0, 1.0],
                    scale: text_scale,
                });
            }
            texts.push(UiText { text, x, y: ty, color: COLOR_XP_LEVEL, scale: text_scale });
        }
    }

    // Die gelisteten Eintraege in Vanillas Reihenfolge (PlayerListHud.ENTRY_ORDERING):
    // hoehere list_order zuerst, Zuschauer ans Ende, dann nach Teamname und zuletzt nach
    // Spielername ohne Gross-/Kleinschreibung. Hoechstens 80.
    pub(crate) fn player_list_entries(&self) -> Vec<(&[u8; 16], &TabEntry)> {
        let mut liste: Vec<(&[u8; 16], &TabEntry)> = self.tab_entries.iter().filter(|(_, e)| e.listed).collect();
        liste.sort_by(|(_, a), (_, b)| {
            let team = |e: &TabEntry| self.sb_entry_team.get(&e.name).cloned().unwrap_or_default();
            b.list_order
                .cmp(&a.list_order)
                .then_with(|| (a.game_mode == GAME_MODE_SPECTATOR as i32).cmp(&(b.game_mode == GAME_MODE_SPECTATOR as i32)))
                .then_with(|| team(a).cmp(&team(b)))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        liste.truncate(PLAYER_LIST_MAX_ENTRIES);
        liste
    }

    // PlayerListHud.getPlayerName: der Anzeigename des Servers, sonst der Name im Kleid
    // seines Teams (Team.decorateName -- Prefix, Name, Suffix, alles in der Teamfarbe).
    pub(crate) fn player_list_name(&self, entry: &TabEntry) -> Vec<protocol::nbt::TextSpan> {
        let spans = match &entry.display_name {
            Some(name) => name.clone(),
            None => {
                let farbe = self.sb_entry_team.get(&entry.name).and_then(|t| self.sb_teams.get(t)).and_then(|t| protocol::nbt::formatting_color(t.color));
                protocol::nbt::legacy_to_spans(&self.decorate_team_name(&entry.name), farbe)
            }
        };
        self.ego_rewrite(&spans).unwrap_or(spans)
    }

    // ReadableScoreboardScore.getFormattedScore: das Zahlenformat des Scores, sonst das des
    // Objectives, sonst Vanillas Vorgabe (gelbe Zahl). Ohne Score gar nichts.
    pub(crate) fn player_list_score_text(&self, objective: &str, holder: &str) -> Vec<protocol::nbt::TextSpan> {
        use protocol::packets::NumberFormat;
        let Some((score, _)) = self.sb_scores.get(objective).and_then(|m| m.get(holder)) else { return Vec::new() };
        let schluessel = (objective.to_string(), holder.to_string());
        let format = self.sb_score_formats.get(&schluessel).or_else(|| self.sb_objective_formats.get(objective));
        match format {
            Some(NumberFormat::Blank) => Vec::new(),
            Some(NumberFormat::Fixed(text)) => text.clone(),
            Some(NumberFormat::Styled(farbe)) => {
                vec![protocol::nbt::TextSpan { text: score.to_string(), color: *farbe, ..Default::default() }]
            }
            None => vec![protocol::nbt::TextSpan { text: score.to_string(), color: Some(PLAYER_LIST_SCORE_COLOR), ..Default::default() }],
        }
    }

    // Atlas-Rechteck eines 8x8-Ausschnitts aus dem Skin (Gesicht bzw. Hutlage).
    // Ein Gesicht (8x8 ab px/py) aus dem geladenen Skin. Ist er (noch) nicht im Atlas, steht
    // schwarz-lila da statt eines Standard-Gesichts -- das saehe aus wie ein echter Skin.
    pub(crate) fn skin_face_uv(&self, uuid: &[u8; 16], px: f32, py: f32) -> [f32; 4] {
        let [u0, v0, u1, v1] = self
            .skins
            .get(uuid)
            .and_then(|s| s.slot)
            .and_then(|slot| self.entity_atlas.skin_slot_rect(slot))
            .or_else(|| self.entity_atlas.rect(assets::MISSING_SKIN))
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let (du, dv) = ((u1 - u0) / 64.0, (v1 - v0) / 64.0);
        [u0 + px * du, v0 + py * dv, u0 + (px + 8.0) * du, v0 + (py + 8.0) * dv]
    }

    // Vanillas PlayerListHud.render, Mass fuer Mass in GUI-Einheiten (mal `g`, dem
    // Hotbar-Massstab). Bewusste Abweichungen:
    //   - Koepfe gibt es immer, nicht nur bei verschluesselter Verbindung: Vanilla laesst sie
    //     auf Offline-Servern weg, weil es dort keine Skins gibt -- der schwarz-lila Kopf
    //     stoert aber nicht und haelt die Spalten gleich breit.
    //   - Kein Blinken der Herzen bei Aenderung und keine kursive Schrift fuer Zuschauer
    //     (die Schrift hat keinen Kursivschnitt); Zuschauer sind wie in Vanilla blasser.
    pub(crate) fn build_player_list(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, sprites: &mut Vec<UiSprite>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        let g = self.player_list_scale(gpu.size());
        let breite = |spans: &[protocol::nbt::TextSpan]| -> f32 { spans.iter().map(|s| gpu.measure_text(&s.text, g)).sum() };
        let schreibe = |texts: &mut Vec<UiText>, spans: &[protocol::nbt::TextSpan], x: f32, y: f32, alpha: f32| {
            let mut x = x.round();
            for span in spans {
                let mut farbe = span.color.map(rgb_to_rgba).unwrap_or(COLOR_WHITE);
                farbe[3] *= alpha;
                push_span_shadow(texts, span.shadow);
                texts.push(UiText { text: span.text.clone(), x, y: y.round(), color: farbe, scale: g });
                x += gpu.measure_text(&span.text, g);
            }
        };
        let flaeche = |rects: &mut Vec<UiRect>, x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4]| {
            let (x0, y0, x1, y1) = (x0.round(), y0.round(), x1.round(), y1.round());
            rects.push(UiRect { rect: Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, color, ..UiRect::FLAT });
        };
        let symbol = |sprites: &mut Vec<UiSprite>, name: &str, x: f32, y: f32, w: f32, h: f32| {
            if let Some(r) = self.ui_atlas.rect(name) {
                sprites.push(UiSprite::quad(x.round(), y.round(), w * g, h * g, r.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
            }
        };

        let liste = self.player_list_entries();
        let objective = self.sb_list_objective.as_deref().filter(|o| self.sb_objective_titles.contains_key(*o));
        let herzen = objective.is_some_and(|o| self.sb_objective_hearts.contains(o));
        let leer = gpu.measure_text(" ", g);

        // Namen und Scores vorbereiten, dabei die breitesten merken (j und k in Vanilla).
        let mut max_name = 0.0f32;
        let mut max_score = 0.0f32;
        let mut zeilen: Vec<(Vec<protocol::nbt::TextSpan>, Vec<protocol::nbt::TextSpan>, i32)> = Vec::with_capacity(liste.len());
        for (_, entry) in &liste {
            let name = self.player_list_name(entry);
            max_name = max_name.max(breite(&name));
            let (mut score_text, mut score) = (Vec::new(), 0);
            if let Some(obj) = objective {
                score = self.sb_scores.get(obj).and_then(|m| m.get(&entry.name)).map(|(s, _)| *s).unwrap_or(0);
                if !herzen {
                    score_text = self.player_list_score_text(obj, &entry.name);
                    let w = breite(&score_text);
                    max_score = max_score.max(if w > 0.0 { leer + w } else { 0.0 });
                }
            }
            zeilen.push((name, score_text, score));
        }

        // Spalten: so viele, dass keine laenger als 20 Zeilen wird.
        let n = liste.len();
        let mut spalten = 1usize;
        let mut reihen = n;
        while reihen > PLAYER_LIST_MAX_ROWS {
            spalten += 1;
            reihen = n.div_ceil(spalten);
        }
        let score_breite = match objective {
            Some(_) if herzen => PLAYER_LIST_HEARTS_WIDTH * g,
            Some(_) => max_score,
            None => 0.0,
        };
        let p = spalten as f32;
        let spalte = ((p * (9.0 * g + max_name + score_breite + 13.0 * g)).min(sw - 50.0 * g) / p).floor();
        // Fest oben mittig wie in Vanilla: Text bei 10 * g, der Hintergrund beginnt g darueber.
        let _ = sh;
        let mut y = (10.0 * g).round();
        let mut t = spalte * p + (p - 1.0) * 5.0 * g;
        let umbrechen = |spans: &[protocol::nbt::TextSpan]| -> Vec<Vec<protocol::nbt::TextSpan>> {
            if spans.is_empty() {
                return Vec::new();
            }
            wrap_label_lines(&split_lines_vanilla(spans), sw - 50.0 * g, |text| gpu.measure_text(text, g))
        };
        let kopf = umbrechen(&self.tab_header);
        let fuss = umbrechen(&self.tab_footer);
        for zeile in kopf.iter().chain(fuss.iter()) {
            t = t.max(breite(zeile));
        }
        let zeile_h = 9.0 * g;
        let mitte = (sw / 2.0).round();
        let links = mitte - (spalte * p + (p - 1.0) * 5.0 * g) / 2.0;

        if !kopf.is_empty() {
            flaeche(rects, mitte - t / 2.0 - g, y - g, mitte + t / 2.0 + g, y + kopf.len() as f32 * zeile_h, black_veil(PLAYER_LIST_BG_ALPHA));
            for zeile in &kopf {
                schreibe(texts, zeile, mitte - breite(zeile) / 2.0, y, 1.0);
                y += zeile_h;
            }
            y += g;
        }

        flaeche(rects, mitte - t / 2.0 - g, y - g, mitte + t / 2.0 + g, y + reihen as f32 * zeile_h, black_veil(PLAYER_LIST_BG_ALPHA));
        for (w, ((uuid, entry), (name, score_text, score))) in liste.iter().zip(&zeilen).enumerate() {
            let (u, x) = (w / reihen.max(1), w % reihen.max(1));
            let spalte_x = links + u as f32 * (spalte + 5.0 * g);
            let z = y + x as f32 * zeile_h;
            flaeche(rects, spalte_x, z, spalte_x + spalte, z + 8.0 * g, white_veil(PLAYER_LIST_ROW_ALPHA));
            let zuschauer = entry.game_mode == GAME_MODE_SPECTATOR as i32;

            // Kopf: Gesicht, darueber die Hutlage (sofern der Spieler sie zeigt). Bevorzugt
            // aus dem eigenen Kopf-Platz (s. store_tab_head), sonst aus dem Skin-Platz.
            let eigener = self.tab_heads.get(*uuid).and_then(|(slot, _)| self.entity_atlas.head_slot_rect(*slot));
            let (kopf_uv, hut_uv) = match eigener {
                Some([u0, v0, u1, v1]) => {
                    let mitte = (u0 + u1) / 2.0;
                    ([u0, v0, mitte, v1], [mitte, v0, u1, v1])
                }
                None => (self.skin_face_uv(uuid, 8.0, 8.0), self.skin_face_uv(uuid, 40.0, 8.0)),
            };
            sprites.push(UiSprite::quad(spalte_x.round(), z.round(), 8.0 * g, 8.0 * g, kopf_uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Entity));
            if entry.show_hat {
                sprites.push(UiSprite::quad(spalte_x.round(), z.round(), 8.0 * g, 8.0 * g, hut_uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Entity));
            }
            let text_x = spalte_x + 9.0 * g;
            // Die Deckkraft wie bei white_veil umgerechnet: weisse Schrift ueber dunklem Grund.
            schreibe(texts, name, text_x, z, if zuschauer { world::sky::srgb_to_linear(PLAYER_LIST_SPECTATOR_ALPHA) } else { 1.0 });

            if objective.is_some() && !zuschauer {
                let aa = text_x + max_name + g;
                let ab = aa + score_breite;
                if ab - aa > 5.0 * g {
                    if herzen {
                        self.build_player_list_hearts(gpu, texts, sprites, *score, aa, ab, z, g);
                    } else if !score_text.is_empty() {
                        schreibe(texts, score_text, ab - breite(score_text), z, 1.0);
                    }
                }
            }

            // Ping-Balken (PlayerListHud.renderLatencyIcon).
            let ping = match entry.latency {
                l if l < 0 => "icon/ping_unknown",
                l if l < 150 => "icon/ping_5",
                l if l < 300 => "icon/ping_4",
                l if l < 600 => "icon/ping_3",
                l if l < 1000 => "icon/ping_2",
                _ => "icon/ping_1",
            };
            symbol(sprites, ping, spalte_x + spalte - 11.0 * g, z, 10.0, 8.0);
        }

        if !fuss.is_empty() {
            y += reihen as f32 * zeile_h + g;
            flaeche(rects, mitte - t / 2.0 - g, y - g, mitte + t / 2.0 + g, y + fuss.len() as f32 * zeile_h, black_veil(PLAYER_LIST_BG_ALPHA));
            for zeile in &fuss {
                schreibe(texts, zeile, mitte - breite(zeile) / 2.0, y, 1.0);
                y += zeile_h;
            }
        }
    }

    // PlayerListHud.renderHearts ohne das Blinken: Herzcontainer bis mindestens 10, darin
    // die vollen/halben Herzen des Scores (ab dem elften goldene). Ist fuer Herzen kein Platz
    // (Abstand <= 3 Einheiten), steht stattdessen "<Leben>hp" in einer Farbe von Rot nach Gruen.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_player_list_hearts(
        &self,
        gpu: &GpuState,
        texts: &mut Vec<UiText>,
        sprites: &mut Vec<UiSprite>,
        score: i32,
        links: f32,
        rechts: f32,
        y: f32,
        g: f32,
    ) {
        let volle = (score.max(0) + 1) / 2;
        let container = score.max(20) / 2;
        if volle <= 0 {
            return;
        }
        let abstand = (((rechts - links) / g - 4.0) / container as f32).min(9.0).floor();
        if abstand <= 3.0 {
            let anteil = (score as f32 / 20.0).clamp(0.0, 1.0);
            let rgb = (((1.0 - anteil) * 255.0) as u32) << 16 | ((anteil * 255.0) as u32) << 8;
            let leben = score as f32 / 2.0;
            let mut text = format!("{leben:?}hp");
            if rechts - gpu.measure_text(&text, g) < links {
                text = format!("{leben:?}");
            }
            let w = gpu.measure_text(&text, g);
            texts.push(UiText { text, x: ((rechts + links - w) / 2.0).round(), y: y.round(), color: rgb_to_rgba(rgb), scale: g });
            return;
        }
        let mut herz = |name: &str, l: i32| {
            if let Some(r) = self.ui_atlas.rect(name) {
                let x = (links + l as f32 * abstand * g).round();
                sprites.push(UiSprite::quad(x, y.round(), 9.0 * g, 9.0 * g, r.uv, [1.0, 1.0, 1.0, 1.0], SpriteAtlas::Ui));
            }
        };
        for l in volle..container {
            herz("hud/heart/container", l);
        }
        for l in 0..volle {
            herz("hud/heart/container", l);
            if l * 2 + 1 < score {
                herz(if l >= 10 { "hud/heart/absorbing_full" } else { "hud/heart/full" }, l);
            }
            if l * 2 + 1 == score {
                herz(if l >= 10 { "hud/heart/absorbing_half" } else { "hud/heart/half" }, l);
            }
        }
    }

    // Scoreboard-Seitenleiste (rechts), Titel/Untertitel (Mitte) und Actionbar (ueber der
    // Hotbar) -- Serverinhalt, immer sichtbar im World-Modus (wie der Chat, nicht an show_hud
    // gekoppelt). Farben/Formatierung der Text-Components sind (noch) auf Weiss reduziert.
    pub(crate) fn build_server_hud(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {

        // --- Scoreboard-Seitenleiste (Position aus dem HUD-Editor, s. hud_scoreboard_rect) ---
        if let Some((title, lines)) = self.scoreboard_lines_shown() {
            self.begin_text_area(gpu, texts, ShadowArea::Scoreboard);
            let rect = self.hud_scoreboard_rect(gpu);
            let (scale, line_h, title_h, pad) = self.scoreboard_metrics(gpu);
            rects.push(UiRect { rect, color: COLOR_SCOREBOARD_BG, ..UiRect::FLAT });
            // Ein UiText je Abschnitt (eine Zeile kennt nur eine Farbe) -- dieselbe
            // Aufteilung wie im Chat.
            // Jede Zeile als GANZES auf wechselnde Zahlen pruefen (s. stable_numbers): die
            // Zahl steht meist in einem eigenen Farbabschnitt, und der allein verraet nicht,
            // was hinter ihm rutschen wuerde.
            let voll = |spans: &[protocol::nbt::TextSpan]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
            gpu.observe_numbers(std::iter::once(voll(&title)).chain(lines.iter().map(|z| voll(z))).collect::<Vec<_>>().iter().map(|s| s.as_str()));
            let tw = stable_row_width(gpu, &title, scale);
            let tx = (rect.x + (rect.w - tw) / 2.0).round();
            push_stable_spans(gpu, &title, tx, (rect.y + (title_h - gpu.text_height(scale)) / 2.0).round(), COLOR_HUD, scale, texts, rects);
            let mut y = rect.y + title_h;
            for zeile in lines {
                push_stable_spans(gpu, &zeile, (rect.x + pad).round(), y.round(), COLOR_WHITE, scale, texts, rects);
                y += line_h;
            }
            Self::end_text_area(gpu, texts);
        }

        // --- Titel / Untertitel: jeder an seiner eigenen Stelle (s. HUD-Editor) ---
        let total_secs = (self.title_fade_in + self.title_stay + self.title_fade_out).max(0) as f32 / 20.0;
        if (self.title_text.is_some() || self.subtitle_text.is_some()) && self.title_set_at.elapsed().as_secs_f32() <= total_secs {
            self.begin_text_area(gpu, texts, ShadowArea::Title);
            // Ein UiText je Farbabschnitt, nebeneinander, in der Ausrichtung der Zeile.
            for (spans, subtitle) in [(&self.title_text, false), (&self.subtitle_text, true)] {
                let Some(spans) = spans else { continue };
                let block = self.hud_title_line_rect(gpu, subtitle);
                let a = if subtitle { self.settings.hud_subtitle_align } else { self.settings.hud_title_align };
                let scale = self.title_scale(subtitle);
                let w = span_row_width(gpu, spans, scale);
                let x = align_left(align_anchor(block.x, block.w, a), w, a).round();
                push_styled_row(gpu, spans, x, block.y, COLOR_WHITE, scale, texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
            }
            Self::end_text_area(gpu, texts);
        }

        // --- Schlaf-Hinweis: Vanilla zeigt im Bett einen eigenen Bildschirm mit "Bett
        // verlassen"-Knopf. Wir bleiben im Spiel und blenden nur ein, wie man wieder aufsteht.
        if self.sleeping_at.is_some() {
            let (sw, sh) = gpu.size();
            let text = "Schleichen: aufstehen".to_string();
            let w = gpu.measure_text(&text, UI_SCALE_NORMAL);
            texts.push(UiText { text, x: ((sw - w) / 2.0).round(), y: (sh * 0.62).round(), color: COLOR_WHITE, scale: UI_SCALE_NORMAL });
        }

        // --- Actionbar (Position aus dem HUD-Editor, s. hud_actionbar_rect) ---
        if let Some((spans, at)) = &self.action_bar {
            if at.elapsed() < ACTION_BAR_DURATION {
                self.begin_text_area(gpu, texts, ShadowArea::ActionBar);
                let rect = self.hud_actionbar_rect(gpu);
                // Ein UiText je Abschnitt, nebeneinandergesetzt -- eine Zeile kennt nur eine
                // Farbe (dieselbe Aufteilung wie im Chat). Ausgerichtet am Anker (s. HUD-Editor).
                let a = self.settings.hud_actionbar_align;
                let gesamt = span_row_width(gpu, spans, UI_SCALE_NORMAL);
                let x = align_left(align_anchor(rect.x, rect.w, a), gesamt, a).round();
                push_styled_row(gpu, spans, x, rect.y, COLOR_WHITE, UI_SCALE_NORMAL, texts, &mut |r, c| rects.push(UiRect { rect: r, color: c, ..UiRect::FLAT }));
                Self::end_text_area(gpu, texts);
            }
        }
    }
}
