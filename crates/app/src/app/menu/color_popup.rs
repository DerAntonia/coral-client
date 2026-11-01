// Farbwahl-Popup und erweiterte Tastenbelegung.

use super::*;

impl App {
    pub(crate) const fn color_axes(&self, t: ColorTarget) -> usize {
        match t {
            ColorTarget::Theme(_)
            | ColorTarget::Hitbox(_)
            | ColorTarget::Team(_)
            | ColorTarget::Sky
            | ColorTarget::Crosshair
            | ColorTarget::FireCrosshair
            | ColorTarget::StackCount(_)
            | ColorTarget::SaturationHud => 3,
            ColorTarget::EgoName(i) => if i == 2 { 4 } else { 3 },
            ColorTarget::Addon(..) => 3,
            ColorTarget::Shield(farbe) => ShieldColor::axes(self.settings.shield_mark(farbe)).len(),
            ColorTarget::BlockOutline | ColorTarget::DamageTint | ColorTarget::Crit | ColorTarget::HitboxKind(_) => 4,
        }
    }

    pub(crate) fn color_title(t: ColorTarget) -> String {
        match t {
            ColorTarget::Theme(slot) => slot.label().to_string(),
            ColorTarget::Hitbox(HitboxVariant::Smoothed) => "Hitbox geglättet".to_string(),
            ColorTarget::Hitbox(HitboxVariant::Raw) => "Hitbox roh".to_string(),
            ColorTarget::HitboxKind(art) => format!("Hitbox {}", art.label()),
            ColorTarget::Team(seite) => format!("Hitbox {}", seite.label()),
            ColorTarget::Shield(ShieldColor::Cooldown) => "Schild-Cooldown".to_string(),
            ColorTarget::Shield(ShieldColor::Hold) => "Schild gehalten".to_string(),
            ColorTarget::Sky => "Himmelsfarbe".to_string(),
            ColorTarget::BlockOutline => "Blockumriss".to_string(),
            ColorTarget::DamageTint => "Schadens-Färbung".to_string(),
            ColorTarget::Crit => "Crit-Farbe".to_string(),
            ColorTarget::Crosshair => "Kreuz-Farbe".to_string(),
            ColorTarget::FireCrosshair => "Kreuz beim Brennen".to_string(),
            ColorTarget::SaturationHud => "Sättigung".to_string(),
            ColorTarget::StackCount(true) => "Anzahl in der Hotbar".to_string(),
            ColorTarget::StackCount(false) => "Anzahl im Inventar".to_string(),
            ColorTarget::EgoName(2) => "Namensschatten".to_string(),
            ColorTarget::EgoName(3) => "Namensschatten 2".to_string(),
            ColorTarget::EgoName(i) => format!("Namensfarbe {}", i + 1),
            ColorTarget::Addon(m, _) => ModuleId::Addon(m).label().to_string(),
        }
    }

    // Stand einer Farbe: Ton 0..360, Saettigung/Helligkeit/Deckkraft 0..100 (ohne Deckkraft 100).
    pub(crate) fn color_hsva(&self, t: ColorTarget) -> [f32; 4] {
        let s = &self.settings;
        let a4 = |f: &dyn Fn(DamageTintAxis) -> u32| {
            [f(DamageTintAxis::Hue) as f32, f(DamageTintAxis::Saturation) as f32, f(DamageTintAxis::Value) as f32, f(DamageTintAxis::Alpha) as f32]
        };
        match t {
            ColorTarget::Theme(slot) => {
                let (h, sa, v) = theme::to_hsv(theme::srgb(slot));
                [h, sa * 100.0, v * 100.0, 100.0]
            }
            ColorTarget::Hitbox(variant) => {
                let (h, sa, v) = self.hitbox_color_of(variant);
                [h as f32, sa as f32, v as f32, 100.0]
            }
            ColorTarget::HitboxKind(art) => {
                let st = s.module_hitbox_styles[art.index()];
                [st.hue as f32, st.saturation as f32, st.value as f32, st.alpha as f32]
            }
            ColorTarget::Team(seite) => {
                let st = s.team_hitboxes[seite.index()];
                [st.hue as f32, st.saturation as f32, st.value as f32, 100.0]
            }
            ColorTarget::Crosshair => [s.crosshair_hue as f32, s.crosshair_saturation as f32, s.crosshair_value as f32, 100.0],
            ColorTarget::FireCrosshair => [s.fire_crosshair_hue as f32, s.fire_crosshair_saturation as f32, s.fire_crosshair_value as f32, 100.0],
            ColorTarget::SaturationHud => [s.saturation_hud_hue as f32, s.saturation_hud_saturation as f32, s.saturation_hud_value as f32, 100.0],
            ColorTarget::StackCount(hotbar) => {
                let c = s.stack_count_colors[usize::from(!hotbar)];
                [c[0] as f32, c[1] as f32, c[2] as f32, 100.0]
            }
            ColorTarget::EgoName(i) => s.ego_colors[i as usize].map(|v| v as f32),
            ColorTarget::Addon(m, k) => {
                let (h, sa, v) = theme::to_hsv(addons::parse_color(&self.addon_value(m, k)).unwrap_or(0xFF_FFFF));
                [h, sa * 100.0, v * 100.0, 100.0]
            }
            ColorTarget::Sky => [
                s.sky_color_of(SkyColorAxis::Hue) as f32,
                s.sky_color_of(SkyColorAxis::Saturation) as f32,
                s.sky_color_of(SkyColorAxis::Value) as f32,
                100.0,
            ],
            ColorTarget::Shield(farbe) => {
                let mut c = a4(&|a| s.shield_color_of(farbe, a));
                if self.color_axes(t) < 4 {
                    c[3] = 100.0;
                }
                c
            }
            ColorTarget::BlockOutline => a4(&|a| s.block_outline_of(a)),
            ColorTarget::DamageTint => a4(&|a| s.damage_tint_of(a)),
            ColorTarget::Crit => a4(&|a| s.crit_color_of(a)),
        }
    }

    pub(crate) fn set_color_hsva(&mut self, t: ColorTarget, c: [f32; 4]) {
        if let ColorTarget::Addon(m, k) = t {
            let [r, g, b] = hsv_to_rgb(c[0], c[1] / 100.0, c[2] / 100.0);
            let k8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u32;
            self.set_addon_value(m, k, format!("#{:06X}", (k8(r) << 16) | (k8(g) << 8) | k8(b)));
            return;
        }
        let u = |i: usize| c[i].round().max(0.0) as u32;
        let h = u(0) % 360;
        let set = &mut self.settings;
        match t {
            // Oben schon erledigt.
            ColorTarget::Addon(..) => {}
            // Nur eigene Themes: bei einem Preset gibt es gar keine Farbfelder zum Anklicken.
            ColorTarget::Theme(slot) => {
                if let ThemeChoice::Custom(i) = set.theme_choice {
                    set.theme[slot.index()] = theme::from_hsv(c[0], c[1] / 100.0, c[2] / 100.0);
                    theme::derive(&mut set.theme, slot);
                    if let Some(t) = set.custom_themes.get_mut(i) {
                        t.colors = set.theme;
                    }
                    theme::apply(&set.theme);
                }
            }
            ColorTarget::Hitbox(HitboxVariant::Smoothed) => {
                (set.module_hitbox_smoothed_hue, set.module_hitbox_smoothed_saturation, set.module_hitbox_smoothed_value) = (h, u(1), u(2));
            }
            ColorTarget::Hitbox(HitboxVariant::Raw) => {
                (set.module_hitbox_raw_hue, set.module_hitbox_raw_saturation, set.module_hitbox_raw_value) = (h, u(1), u(2));
            }
            ColorTarget::HitboxKind(art) => {
                let st = &mut set.module_hitbox_styles[art.index()];
                (st.hue, st.saturation, st.value, st.alpha) = (h, u(1).min(100), u(2).min(100), u(3).min(100));
            }
            ColorTarget::Team(seite) => {
                let st = &mut set.team_hitboxes[seite.index()];
                (st.hue, st.saturation, st.value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::Crosshair => {
                (set.crosshair_hue, set.crosshair_saturation, set.crosshair_value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::FireCrosshair => {
                (set.fire_crosshair_hue, set.fire_crosshair_saturation, set.fire_crosshair_value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::EgoName(i) => set.ego_colors[i as usize] = [h, u(1).min(100), u(2).min(100), if i == 2 { u(3).min(100) } else { 100 }],
            ColorTarget::StackCount(hotbar) => set.stack_count_colors[usize::from(!hotbar)] = [h, u(1).min(100), u(2).min(100)],
            ColorTarget::SaturationHud => {
                (set.saturation_hud_hue, set.saturation_hud_saturation, set.saturation_hud_value) = (h, u(1).min(100), u(2).min(100));
            }
            ColorTarget::Sky => {
                *set.sky_color_mut(SkyColorAxis::Hue) = h;
                *set.sky_color_mut(SkyColorAxis::Saturation) = u(1);
                *set.sky_color_mut(SkyColorAxis::Value) = u(2);
            }
            ColorTarget::Shield(farbe) => {
                for (i, a) in DamageTintAxis::ALL.into_iter().enumerate() {
                    if let Some(feld) = set.shield_color_mut(farbe, a) {
                        *feld = if i == 0 { h } else { u(i) };
                    }
                }
            }
            ColorTarget::BlockOutline | ColorTarget::DamageTint | ColorTarget::Crit => {
                for (i, a) in DamageTintAxis::ALL.into_iter().enumerate() {
                    let feld = match t {
                        ColorTarget::BlockOutline => set.block_outline_mut(a),
                        ColorTarget::DamageTint => set.damage_tint_mut(a),
                        _ => set.crit_color_mut(a),
                    };
                    *feld = if i == 0 { h } else { u(i) };
                }
            }
        }
    }

    // Farbe als Flaechenfarbe fuer Farbfeld und Vorschau.
    pub(crate) fn color_preview(&self, t: ColorTarget, c: [f32; 4]) -> [f32; 4] {
        match t {
            ColorTarget::Theme(_) => theme::rgba(theme::from_hsv(c[0], c[1] / 100.0, c[2] / 100.0)),
            // Die Stueckzahl steht linear in der UI (s. Settings::stack_count_rgba): so sieht
            // die Vorschau aus wie die Zahl.
            ColorTarget::StackCount(_) => {
                let rgb = hsv_to_rgb(c[0], c[1] / 100.0, c[2] / 100.0).map(world::sky::srgb_to_linear);
                [rgb[0], rgb[1], rgb[2], 1.0]
            }
            _ => {
                let rgb = hsv_to_rgb(c[0], c[1] / 100.0, c[2] / 100.0);
                [rgb[0], rgb[1], rgb[2], 1.0]
            }
        }
    }

    // Welche Farbe ein Knopf oeffnet: das neue OpenColorPicker und die alten Aufklapp-Knoepfe
    // der Module, die jetzt ebenfalls das Fenster oeffnen.
    pub(crate) const fn color_target_of(action: ButtonAction) -> Option<ColorTarget> {
        match action {
            ButtonAction::OpenColorPicker(t) => Some(t),
            ButtonAction::HitboxColorEditor(v) => Some(ColorTarget::Hitbox(v)),
            ButtonAction::ShieldColorEditor(c) => Some(ColorTarget::Shield(c)),
            ButtonAction::BlockOutlineEditor => Some(ColorTarget::BlockOutline),
            _ => None,
        }
    }

    pub(crate) fn open_color_popup(&mut self, target: ColorTarget, anchor: Rect) {
        if self.color_popup.as_ref().is_some_and(|p| p.target == target) {
            self.color_popup = None;
            return;
        }
        self.color_popup = Some(ColorPopup { target, anchor, hsva: self.color_hsva(target), drag: None });
    }

    // Fenster und seine Teile in Bildschirmkoordinaten: (Fenster, Vorschau, Spur je Achse).
    // Farbfenster, die neben statt unter ihrem Farbfeld aufgehen: Kreuz und Blockumriss.
    pub(crate) const fn color_popup_beside(t: ColorTarget) -> bool {
        matches!(t, ColorTarget::Crosshair | ColorTarget::BlockOutline)
    }

    pub(crate) fn color_popup_layout(&self, popup: &ColorPopup) -> (Rect, Rect, Vec<Rect>) {
        let (sw, sh) = ui_screen();
        let achsen = self.color_axes(popup.target);
        let h = COLOR_POPUP_PAD + 24.0 + COLOR_POPUP_PREVIEW_H + 8.0 + achsen as f32 * COLOR_POPUP_ROW_H + COLOR_POPUP_PAD;
        let a = popup.anchor;
        let (x, y) = if Self::color_popup_beside(popup.target) {
            // Rechts neben dem Farbfeld, oben buendig; passt es dort nicht hin, links davon.
            let rechts = a.x + a.w + 8.0;
            let x = if rechts + COLOR_POPUP_W <= sw - 8.0 { rechts } else { (a.x - 8.0 - COLOR_POPUP_W).max(8.0) };
            (x, a.y.min(sh - 8.0 - h).max(8.0))
        } else {
            let x = (a.x + a.w - COLOR_POPUP_W).clamp(8.0, (sw - COLOR_POPUP_W - 8.0).max(8.0));
            let unten = a.y + a.h + 8.0;
            (x, if unten + h <= sh - 8.0 { unten } else { (a.y - 8.0 - h).max(TOPBAR_H + 8.0) })
        };
        let fenster = Rect { x, y, w: COLOR_POPUP_W, h };
        let vorschau = Rect { x: x + COLOR_POPUP_PAD, y: y + COLOR_POPUP_PAD + 24.0, w: COLOR_POPUP_W - 2.0 * COLOR_POPUP_PAD, h: COLOR_POPUP_PREVIEW_H };
        let spuren = (0..achsen)
            .map(|i| Rect {
                x: vorschau.x,
                y: vorschau.y + vorschau.h + 8.0 + i as f32 * COLOR_POPUP_ROW_H + 22.0,
                w: vorschau.w,
                h: COLOR_POPUP_TRACK_H,
            })
            .collect();
        (fenster, vorschau, spuren)
    }

    // Reihenfolge der Regler wie im Design: Ton, Helligkeit, Saettigung, Deckkraft -- als
    // Index in hsva.
    pub(crate) const COLOR_POPUP_ORDER: [usize; 4] = [0, 2, 1, 3];

    // Linke Maustaste gedrueckt, waehrend das Fenster offen ist. true = Klick verbraucht.
    pub(crate) fn color_popup_press(&mut self) -> bool {
        let Some(popup) = self.color_popup.as_ref() else { return false };
        let (fenster, _, spuren) = self.color_popup_layout(popup);
        let (cx, cy) = self.cursor_pos;
        if !fenster.contains(cx, cy) {
            // Daneben: schliessen. Der Klick tut sonst nichts -- sonst loeste man beim
            // Wegklicken versehentlich einen Knopf darunter aus.
            self.color_popup = None;
            self.persist_settings();
            return true;
        }
        if let Some(i) = spuren.iter().position(|r| cx >= r.x - 6.0 && cx <= r.x + r.w + 6.0 && cy >= r.y - 10.0 && cy <= r.y + r.h + 10.0) {
            if let Some(p) = self.color_popup.as_mut() {
                p.drag = Some(i);
            }
            self.color_popup_drag();
        }
        true
    }

    pub(crate) fn color_popup_drag(&mut self) {
        let Some(popup) = self.color_popup.as_ref() else { return };
        let Some(i) = popup.drag else { return };
        let (_, _, spuren) = self.color_popup_layout(popup);
        let r = spuren[i];
        let t = ((self.cursor_pos.0 - r.x) / r.w).clamp(0.0, 1.0);
        let achse = Self::COLOR_POPUP_ORDER[i];
        let max = if achse == 0 { 359.0 } else { 100.0 };
        let target = popup.target;
        let mut hsva = popup.hsva;
        hsva[achse] = t * max;
        if let Some(p) = self.color_popup.as_mut() {
            p.hsva = hsva;
        }
        self.set_color_hsva(target, hsva);
    }

    pub(crate) fn color_popup_release(&mut self) {
        if let Some(p) = self.color_popup.as_mut() {
            if p.drag.take().is_some() {
                self.persist_settings();
            }
        }
    }

    // Das Fenster in der Overlay-Ebene (ueber allem Menuetext).
    // Fenster "Erweitert" in Bildschirmkoordinaten, mittig: (Fenster, je Belegung ein Feld,
    // "+" fuer eine weitere (None, wenn alle Plaetze voll sind), "Fertig").
    pub(crate) fn keybind_advanced_layout(&self, aktion: KeyBindAction) -> (Rect, Vec<Rect>, Option<Rect>, Rect) {
        let (sw, sh) = ui_screen();
        let (w, pad, kopf, zeile) = (400.0, 20.0, 44.0, SETTINGS_CTRL_H + 8.0);
        let belegt = self.settings.keybinds.count(aktion);
        let plus = belegt < MAX_BINDS_PER_ACTION;
        let h = pad + kopf + (belegt + usize::from(plus)) as f32 * zeile + 12.0 + SETTINGS_CTRL_H + pad;
        let fenster = Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h };
        let feld = |i: usize| Rect { x: fenster.x + pad, y: fenster.y + pad + kopf + i as f32 * zeile, w: w - 2.0 * pad, h: SETTINGS_CTRL_H };
        let felder = (0..belegt).map(feld).collect();
        let fertig = Rect { x: fenster.x + w - pad - 140.0, y: fenster.y + h - pad - SETTINGS_CTRL_H, w: 140.0, h: SETTINGS_CTRL_H };
        (fenster, felder, plus.then(|| feld(belegt)), fertig)
    }

    // Seine Knoepfe als unsichtbare Trefferflaechen -- gezeichnet wird das Fenster in der
    // Overlay-Ebene (s. build_keybind_advanced), sonst laege die Seite darueber.
    pub(crate) fn keybind_advanced_buttons(&self, aktion: KeyBindAction) -> Vec<Button> {
        let (_, felder, plus, fertig) = self.keybind_advanced_layout(aktion);
        let flaeche = |action, rect| Button { action, rect, label: String::new(), enabled: true, color: SERVER_CARD_MARKER, scale: UI_SCALE_SMALL, slider: None };
        let mut out: Vec<Button> = felder.into_iter().enumerate().map(|(i, r)| flaeche(ButtonAction::Rebind(aktion, i), r)).collect();
        if let Some(r) = plus {
            out.push(flaeche(ButtonAction::Rebind(aktion, self.settings.keybinds.count(aktion)), r));
        }
        out.push(flaeche(ButtonAction::CloseKeybindAdvanced, fertig));
        out
    }

    pub(crate) fn build_keybind_advanced(&self, overlay: &mut UiOverlay) {
        let Some(aktion) = self.keybind_advanced.filter(|_| self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsControls) else { return };
        let Some(gpu) = self.gpu.as_ref() else { return };
        use theme::Slot;
        let g = theme::get;
        let (sw, sh) = gpu.size();
        let (fenster, felder, plus, fertig) = self.keybind_advanced_layout(aktion);
        let (cx, cy) = self.cursor_pos;
        let mut rects = vec![
            UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.0, 0.0, 0.0, 0.45], ..UiRect::FLAT },
            UiRect::shadow(fenster, 8.0, 8.0, 24.0, 0.55),
            UiRect::framed(fenster, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1)),
        ];
        let mut texts = vec![UiText { text: format!("Tasten: {}", aktion.short_label()), x: fenster.x + 20.0, y: fenster.y + 22.0, color: theme::text(), scale: UI_SCALE_SMALL }];
        let knopf = |r: Rect, text: String, wartet: bool, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>| {
            let rand = if r.contains(cx, cy) { g(Slot::Border2) } else { g(Slot::Border1) };
            let flaeche = if wartet { g(Slot::AccentSurface) } else { g(Slot::Surface2) };
            rects.push(UiRect::framed(r, flaeche, 6.0, 1.0, rand));
            texts.push(gpu.centered_text(r, text, theme::text(), UI_SCALE_SMALL));
        };
        let kb = &self.settings.keybinds;
        for (i, r) in felder.into_iter().enumerate() {
            let wartet = self.awaiting_keybind == Some((aktion, i));
            let text = if wartet { "> Taste/Maus <".to_string() } else { kb.bind_at(aktion, i).map(bind_display).unwrap_or_default() };
            knopf(r, text, wartet, &mut rects, &mut texts);
        }
        if let Some(r) = plus {
            let wartet = self.awaiting_keybind == Some((aktion, kb.count(aktion)));
            knopf(r, if wartet { "> Taste/Maus <" } else { "+" }.to_string(), wartet, &mut rects, &mut texts);
        }
        knopf(fertig, "Fertig".to_string(), false, &mut rects, &mut texts);
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    pub(crate) fn build_color_popup(&self, overlay: &mut UiOverlay) {
        let (Some(gpu), Some(popup)) = (self.gpu.as_ref(), self.color_popup.as_ref()) else { return };
        use theme::Slot;
        let g = theme::get;
        let (fenster, vorschau, spuren) = self.color_popup_layout(popup);
        let mut rects = vec![UiRect::shadow(fenster, 8.0, 8.0, 24.0, 0.55), UiRect::framed(fenster, g(Slot::BgPanel), 8.0, 1.0, g(Slot::Border1))];
        let mut texts = Vec::new();
        texts.push(UiText {
            text: Self::color_title(popup.target),
            x: fenster.x + COLOR_POPUP_PAD,
            y: fenster.y + COLOR_POPUP_PAD,
            color: theme::text(),
            scale: UI_SCALE_SMALL,
        });
        let c = popup.hsva;
        let farbe = self.color_preview(popup.target, c);
        let mit_deckkraft = self.color_axes(popup.target) == 4;
        // Vorschau; mit Deckkraft ueber einem Schachbrett, damit man sie sieht.
        if mit_deckkraft {
            let feld = 12.0;
            let (nx, ny) = ((vorschau.w / feld).ceil() as usize, (vorschau.h / feld).ceil() as usize);
            rects.push(UiRect::rounded(vorschau, theme::rgba(0x2A2F36), 6.0));
            for yy in 0..ny {
                for xx in 0..nx {
                    if (xx + yy) % 2 == 0 {
                        let r = Rect { x: vorschau.x + xx as f32 * feld, y: vorschau.y + yy as f32 * feld, w: feld, h: feld };
                        let r = Rect { w: r.w.min(vorschau.x + vorschau.w - r.x), h: r.h.min(vorschau.y + vorschau.h - r.y), ..r };
                        rects.push(UiRect { rect: r, color: theme::rgba(0x3A414B), ..UiRect::FLAT });
                    }
                }
            }
        }
        rects.push(UiRect::framed(vorschau, theme::with_alpha(farbe, if mit_deckkraft { c[3] / 100.0 } else { 1.0 }), 6.0, 2.0, [0.0, 0.0, 0.0, 1.0]));
        let namen = ["Farbton", "Helligkeit", "Sättigung", "Deckkraft"];
        for (i, spur) in spuren.iter().enumerate() {
            let achse = Self::COLOR_POPUP_ORDER[i];
            let wert = c[achse];
            let (text, max) = if achse == 0 { (format!("{}°", wert.round() as u32), 360.0) } else { (format!("{} %", wert.round() as u32), 100.0) };
            let ly = spur.y - 20.0;
            texts.push(UiText { text: namen[i].to_string(), x: spur.x, y: ly, color: theme::text_2(), scale: TOPBAR_SUB_SCALE });
            let tw = gpu.measure_text(&text, TOPBAR_SUB_SCALE);
            texts.push(UiText { text, x: (spur.x + spur.w - tw).round(), y: ly, color: theme::text(), scale: TOPBAR_SUB_SCALE });
            // Verlauf der Spur: Stuecke, die je von Farbe zu Farbe verlaufen (fein genug, dass
            // auch der Farbton-Kreis rund wirkt).
            const STUECKE: usize = 48;
            let farbe_bei = |t: f32| {
                let mut probe = c;
                probe[3] = 100.0;
                match achse {
                    0 => (probe[0], probe[1], probe[2]) = (t * 360.0, 100.0, 100.0),
                    3 => {}
                    _ => probe[achse] = t * 100.0,
                }
                let mut f = self.color_preview(popup.target, probe);
                if achse == 3 {
                    f[3] = t;
                }
                f
            };
            for k in 0..STUECKE {
                let (t0, t1) = (k as f32 / STUECKE as f32, (k + 1) as f32 / STUECKE as f32);
                let (x0, x1) = (spur.x + spur.w * t0, spur.x + spur.w * t1);
                rects.push(UiRect { rect: Rect { x: x0, y: spur.y, w: x1 - x0, h: spur.h }, color: farbe_bei(t0), color_right: Some(farbe_bei(t1)), ..UiRect::FLAT });
            }
            rects.push(UiRect::framed(*spur, [0.0; 4], 3.0, 1.0, g(Slot::Border2)));
            let tx = spur.x + spur.w * self.glide_slider(SliderKey::Color(i), (wert / max).clamp(0.0, 1.0));
            let daumen = Rect { x: (tx - 5.0).round(), y: spur.y + spur.h / 2.0 - 10.0, w: 10.0, h: 20.0 };
            rects.push(UiRect::shadow(daumen, 3.0, 1.0, 3.0, 0.6));
            rects.push(UiRect::framed(daumen, [1.0; 4], 3.0, 2.0, [0.0, 0.0, 0.0, 1.0]));
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
    }

    // --- Addons (s. addons.rs) ---
}
