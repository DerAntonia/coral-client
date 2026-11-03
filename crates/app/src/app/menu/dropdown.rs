// Auswahllisten (Dropdowns).

use super::*;

impl App {
    // Welche Auswahl ein Knopf aufklappt, statt weiterzuschalten.
    pub(crate) const fn choice_of(action: ButtonAction) -> Option<Choice> {
        Some(match action {
            ButtonAction::CycleProjection => Choice::Projection,
            ButtonAction::CycleParticles(own) => Choice::Particles(own),
            ButtonAction::CycleDeathAnimation => Choice::DeathAnimation,
            ButtonAction::CycleWallpaper => Choice::Wallpaper,
            ButtonAction::CycleLanguage(row) => Choice::Language(row),
            ButtonAction::CycleHoldUseRepeat => Choice::HoldUseRepeat,
            ButtonAction::CycleArrowEmptyFeedback => Choice::ArrowEmptyFeedback,
            ButtonAction::CycleSkyMode => Choice::SkyMode,
            ButtonAction::CycleWeatherMode => Choice::WeatherMode,
            ButtonAction::CycleEgoDeco => Choice::EgoDeco,
            ButtonAction::CycleEgoShadow => Choice::EgoShadow,
            ButtonAction::CycleEgoSound => Choice::EgoSound,
            ButtonAction::OpenChoice(c) => c,
            _ => return None,
        })
    }

    // Die Werte einer Auswahl und welcher gerade gilt (None: keiner, z.B. gemischte Sprachen).
    pub(crate) fn choice_options(&self, c: Choice) -> (Vec<String>, Option<usize>) {
        let s = &self.settings;
        let namen = |l: &[&str]| l.iter().map(std::string::ToString::to_string).collect::<Vec<_>>();
        match c {
            Choice::Projection => (namen(&PROJECTION_NAMES), Some(s.projection_mode as usize)),
            Choice::Particles(own) => (namen(&PARTICLE_MODE_NAMES), Some(if own { s.particles_mode_own } else { s.particles_mode } as usize)),
            Choice::DeathAnimation => (namen(&DEATH_ANIM_NAMES), Some(s.death_animation as usize)),
            Choice::Wallpaper => (wallpaper::WallpaperMode::ALL.iter().map(|m| m.label().to_string()).collect(), Some(s.wallpaper as usize)),
            Choice::Language(row) => (i18n::Language::ALL.iter().map(|l| l.label().to_string()).collect(), self.language_of_row(row).map(|l| l.index() as usize)),
            Choice::HoldUseRepeat => (HoldUseRepeat::ALL.iter().map(|h| h.label().to_string()).collect(), HoldUseRepeat::ALL.iter().position(|h| *h == s.hold_use_repeat)),
            Choice::ArrowEmptyFeedback => (namen(&ARROW_EMPTY_FEEDBACK_NAMES), Some(s.arrow_empty_feedback as usize)),
            Choice::SkyMode => (SkyMode::ALL.iter().map(|m| m.label().to_string()).collect(), SkyMode::ALL.iter().position(|m| *m == s.sky_mode)),
            Choice::WeatherMode => (WeatherMode::ALL.iter().map(|m| m.label().to_string()).collect(), WeatherMode::ALL.iter().position(|m| *m == s.weather_mode)),
            Choice::EgoDeco => (ego_name::Deco::ALL.iter().map(|d| d.label().to_string()).collect(), Some(s.ego_deco as usize)),
            Choice::EgoShadow => (ego_name::Deco::ALL.iter().map(|d| d.label_or("Standard").to_string()).collect(), Some(s.ego_shadow as usize)),
            Choice::EgoSound => (ego_name::SOUNDS.iter().map(|(_, l)| l.to_string()).collect(), ego_name::SOUNDS.iter().position(|(e, _)| *e == s.ego_sound)),
            Choice::AudioDevice => (self.audio_device_options(), Some(self.audio_device_index())),
            Choice::AudioBuffer => (
                AUDIO_BUFFER_CHOICES.iter().map(|&f| audio_buffer_label(f)).collect(),
                AUDIO_BUFFER_CHOICES.iter().position(|&f| f == s.audio_buffer_frames),
            ),
            Choice::CrosshairShape => ((0..=CROSSHAIR_SHAPE_CUSTOM).map(|i| crosshair_shape_label(i).to_string()).collect(), Some(s.crosshair_shape_choice())),
            Choice::Addon(m, k) => match self.addon_setting(m, k).map(|d| d.kind) {
                Some(addons::SettingKind::Choice { options }) => {
                    let wert = self.addon_value(m, k);
                    let jetzt = options.iter().position(|o| *o == wert);
                    (options, jetzt)
                }
                _ => (Vec::new(), None),
            },
        }
    }

    // Wert i einer Auswahl setzen. Die Durchschalt-Auswahlen setzen den Wert davor und schalten
    // einmal weiter -- so laeuft genau einmal, was am Weiterschalten haengt (Hintergrund laden,
    // Sprache anwenden, Ton vorspielen).
    pub(crate) fn pick_choice(&mut self, c: Choice, i: usize) {
        let (optionen, jetzt) = self.choice_options(c);
        let n = optionen.len();
        if i >= n || jetzt == Some(i) {
            return;
        }
        match c {
            Choice::AudioDevice => {
                let name = if i == 0 { String::new() } else { optionen[i].clone() };
                let opt = (!name.is_empty()).then_some(name.as_str());
                match self.audio.set_output_device(opt) {
                    Ok(()) => self.settings.audio_output_device = name,
                    Err(e) => self.push_log(format!("Ausgabegerät konnte nicht gesetzt werden: {e}")),
                }
                self.persist_settings();
                return;
            }
            Choice::AudioBuffer => {
                let frames = AUDIO_BUFFER_CHOICES[i];
                match self.audio.set_buffer_frames(audio_buffer_request(frames)) {
                    Ok(()) => self.settings.audio_buffer_frames = frames,
                    Err(e) => self.push_log(format!("Puffer {} geht mit diesem Gerät nicht: {e}", audio_buffer_label(frames))),
                }
                self.persist_settings();
                return;
            }
            Choice::CrosshairShape => {
                self.settings.set_crosshair_shape_choice(i);
                self.persist_settings();
                return;
            }
            Choice::Addon(m, k) => {
                self.set_addon_value(m, k, optionen[i].clone());
                self.persist_settings();
                return;
            }
            _ => {}
        }
        let vorher = (i + n - 1) % n;
        let s = &mut self.settings;
        let weiter = match c {
            Choice::Projection => {
                s.projection_mode = vorher as u8;
                ButtonAction::CycleProjection
            }
            Choice::Particles(own) => {
                *s.particles_mode_of(own) = vorher as u8;
                ButtonAction::CycleParticles(own)
            }
            Choice::DeathAnimation => {
                s.death_animation = vorher as u8;
                ButtonAction::CycleDeathAnimation
            }
            Choice::Wallpaper => {
                s.wallpaper = vorher as u8;
                ButtonAction::CycleWallpaper
            }
            Choice::Language(row) => {
                // Zeile 0 schaltet von der Sprache der Oberflaeche aus alle zusammen weiter.
                let v = i18n::Language::ALL[vorher].index();
                match row {
                    0 | 1 => s.lang_ui = v,
                    2 => s.lang_items = v,
                    3 => s.lang_effects = v,
                    _ => s.lang_chat = v,
                }
                ButtonAction::CycleLanguage(row)
            }
            Choice::HoldUseRepeat => {
                s.hold_use_repeat = HoldUseRepeat::ALL[vorher];
                ButtonAction::CycleHoldUseRepeat
            }
            Choice::ArrowEmptyFeedback => {
                s.arrow_empty_feedback = vorher as u8;
                ButtonAction::CycleArrowEmptyFeedback
            }
            Choice::SkyMode => {
                s.sky_mode = SkyMode::ALL[vorher];
                ButtonAction::CycleSkyMode
            }
            Choice::WeatherMode => {
                s.weather_mode = WeatherMode::ALL[vorher];
                ButtonAction::CycleWeatherMode
            }
            Choice::EgoDeco => {
                s.ego_deco = ego_name::Deco::ALL[vorher];
                ButtonAction::CycleEgoDeco
            }
            Choice::EgoShadow => {
                s.ego_shadow = ego_name::Deco::ALL[vorher];
                ButtonAction::CycleEgoShadow
            }
            Choice::EgoSound => {
                s.ego_sound = ego_name::SOUNDS[vorher].0.to_string();
                ButtonAction::CycleEgoSound
            }
            Choice::AudioDevice | Choice::AudioBuffer | Choice::CrosshairShape | Choice::Addon(..) => return,
        };
        if matches!(c, Choice::ArrowEmptyFeedback | Choice::SkyMode | Choice::WeatherMode | Choice::EgoDeco | Choice::EgoShadow | Choice::EgoSound) {
            self.apply_module_action(weiter);
            self.persist_settings();
        } else {
            self.run_menu_action(weiter);
        }
    }

    // Die offene Liste, sofern sie noch zur Ansicht passt, in der sie aufging.
    pub(crate) fn dropdown_active(&self) -> Option<&Dropdown> {
        self.dropdown.as_ref().filter(|d| d.mode == self.mode && d.screen == self.screen && d.module_menu == self.module_menu_shown())
    }

    // Knopf einer Auswahl angeklickt: Liste darunter auf, ein zweiter Klick schliesst sie.
    pub(crate) fn toggle_dropdown(&mut self, choice: Choice, anchor: Rect) {
        if self.dropdown_active().is_some_and(|d| d.choice == choice) {
            self.dropdown = None;
            self.play_ui_sound(ui_sound::UiSound::Close);
            return;
        }
        let (optionen, jetzt) = self.choice_options(choice);
        let sichtbar = optionen.len().min(DROPDOWN_MAX_ITEMS);
        // Der gewaehlte Eintrag steht moeglichst in der Mitte.
        let scroll = jetzt.unwrap_or(0).saturating_sub(sichtbar / 2).min(optionen.len() - sichtbar);
        self.dropdown = Some(Dropdown { choice, anchor, scroll: smooth_scroll::SmoothScroll::at(scroll as f32), mode: self.mode, screen: self.screen, module_menu: self.module_menu_shown() });
        self.play_ui_sound(ui_sound::UiSound::Open);
    }

    // Kasten der Liste und die sichtbaren Eintraege (Index, Flaeche), in Bildschirmkoordinaten:
    // unter dem Knopf, passt sie dort nicht hin, darueber. Beim gleitenden Scrollen ragen die
    // Eintraege am Rand nur halb herein -- ihre Flaeche ist dann auf den Innenraum gekuerzt
    // (s. dropdown_clip), zum Treffen wie zum Zeichnen.
    pub(crate) fn dropdown_layout(&self, d: &Dropdown) -> (Rect, Vec<(usize, Rect)>) {
        let Some(gpu) = self.gpu.as_ref() else { return (Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, Vec::new()) };
        let (sw, sh) = gpu.size();
        let optionen = self.choice_options(d.choice).0;
        let sichtbar = optionen.len().min(DROPDOWN_MAX_ITEMS);
        let breiteste = optionen.iter().map(|o| gpu.measure_text(o, UI_SCALE_SMALL)).fold(0.0, f32::max);
        let w = d.anchor.w.max(breiteste + 2.0 * DROPDOWN_PAD + 40.0).min(sw - 16.0);
        let h = sichtbar as f32 * DROPDOWN_ITEM_H + 2.0 * DROPDOWN_PAD;
        let unten = d.anchor.y + d.anchor.h + 4.0;
        let y = if unten + h <= sh - 8.0 { unten } else { (d.anchor.y - 4.0 - h).max(8.0) };
        let x = d.anchor.x.min(sw - w - 8.0).max(8.0);
        let kasten = Rect { x, y, w, h };
        let balken = if optionen.len() > sichtbar { DROPDOWN_BAR_W + DROPDOWN_PAD } else { 0.0 };
        let stand = d.scroll.stand;
        let erster = d.scroll.first();
        let eintraege = (erster..(erster + sichtbar + 1).min(optionen.len()))
            .map(|i| {
                let r = Rect { x: x + DROPDOWN_PAD, y: y + DROPDOWN_PAD + (i as f32 - stand) * DROPDOWN_ITEM_H, w: w - 2.0 * DROPDOWN_PAD - balken, h: DROPDOWN_ITEM_H };
                (i, r)
            })
            .filter(|(_, r)| dropdown_clip(kasten, *r).is_some())
            .collect();
        (kasten, eintraege)
    }

    // Linke Taste bei offener Liste: Eintrag waehlen oder daneben schliessen. Die Liste liegt ueber
    // allem; der Klick daneben tut sonst nichts (wie beim Farbfenster). true = verbraucht.
    pub(crate) fn dropdown_press(&mut self) -> bool {
        let Some(d) = self.dropdown_active() else {
            self.dropdown = None;
            return false;
        };
        let choice = d.choice;
        let (kasten, eintraege) = self.dropdown_layout(d);
        let (cx, cy) = self.cursor_pos;
        if let Some(&(i, _)) = eintraege.iter().find(|(_, r)| dropdown_clip(kasten, *r).is_some_and(|r| r.contains(cx, cy))) {
            self.dropdown = None;
            self.play_ui_sound(ui_sound::UiSound::Click);
            self.pick_choice(choice, i);
        } else if !kasten.contains(cx, cy) {
            self.dropdown = None;
            self.play_ui_sound(ui_sound::UiSound::Close);
        }
        true
    }

    // Zeiger im Kasten der offenen Liste.
    pub(crate) fn pointer_in_dropdown(&self) -> bool {
        let (cx, cy) = self.cursor_pos;
        self.dropdown_active().is_some_and(|d| self.dropdown_layout(d).0.contains(cx, cy))
    }

    // Mausrad ueber offener Liste: eine Zeile je Raste.
    pub(crate) fn dropdown_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        let Some(choice) = self.dropdown_active().map(|d| d.choice) else { return };
        let n = self.choice_options(choice).0.len();
        let max = n.saturating_sub(DROPDOWN_MAX_ITEMS);
        let weg = self.wheel_units(delta, 1.0, DROPDOWN_ITEM_H);
        if let Some(d) = self.dropdown.as_mut() {
            d.scroll.push(weg, 1.0, max as f32);
        }
    }

    // Geschlossener Knopf einer Auswahl: Wert linksbuendig, rechts ein Pfeil (offen nach oben),
    // offen mit Akzentrand -- wie die Aufklapplisten in osu!lazer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_dropdown_face(&self, gpu: &GpuState, rect: Rect, label: &str, color: [f32; 4], scale: f32, offen: bool, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if offen {
            rects.push(UiRect::framed(rect, [0.0; 4], 6.0, 1.5, theme::get(theme::Slot::Accent)));
        }
        let text = Self::fit_text(gpu, label, rect.w - 12.0 - 32.0, scale);
        let y = (rect.y + (rect.h - gpu.text_height(scale)) / 2.0).round();
        texts.push(UiText { text, x: rect.x + 12.0, y, color, scale });
        // Pfeil aus kleinen Quadraten: V, offen ^.
        let (cx, cy) = ((rect.x + rect.w - 20.0).round(), (rect.y + rect.h / 2.0).round());
        for k in 0..5 {
            let dy = if offen { 2.0 - k as f32 } else { k as f32 - 2.0 };
            for dx in [-4.0 + k as f32, 4.0 - k as f32] {
                rects.push(UiRect { rect: Rect { x: cx + dx - 1.0, y: cy + dy - 1.0, w: 2.0, h: 2.0 }, color, ..UiRect::FLAT });
            }
        }
    }

    // Offene Liste zeichnen: Kasten mit Akzentrand, der gewaehlte Eintrag hinterlegt, der unter
    // dem Zeiger hell (mit demselben Nachleuchten wie die Knoepfe), bei Ueberlaenge ein Balken.
    // Die Eintraege sind eine eigene Gruppe, beschnitten auf den Innenraum: am Rand ragt ein
    // Eintrag beim Gleiten halb herein, samt halbem Text.
    pub(crate) fn build_dropdown(&self, overlay: &mut UiOverlay) {
        let (Some(gpu), Some(d)) = (self.gpu.as_ref(), self.dropdown_active()) else { return };
        use theme::Slot;
        let g = theme::get;
        let (optionen, jetzt) = self.choice_options(d.choice);
        let (kasten, eintraege) = self.dropdown_layout(d);
        let mut rects = vec![UiRect::shadow(kasten, 8.0, 8.0, 24.0, 0.55), UiRect::framed(kasten, g(Slot::BgPanel), 8.0, 1.5, g(Slot::Accent))];
        let (mut zeilen, mut texts) = (Vec::new(), Vec::new());
        let (cx, cy) = self.cursor_pos;
        let innen = (kasten.y + DROPDOWN_PAD, kasten.y + kasten.h - DROPDOWN_PAD);
        let max = optionen.len().saturating_sub(DROPDOWN_MAX_ITEMS) as f32;
        push_fade_marker(&mut zeilen, &mut texts, Some(ScrollFade::new(innen.0, innen.1, DROPDOWN_ITEM_H, d.scroll.stand, max)));
        for (i, r) in eintraege {
            let Some(sichtbar) = dropdown_clip(kasten, r) else { continue };
            // Nachleuchten am Eintrag, nicht an seiner Lage -- die wandert beim Gleiten.
            let schluessel = Rect { y: -100_000.0 - i as f32 * DROPDOWN_ITEM_H, ..r };
            let k = self.hover_level(schluessel, sichtbar.contains(cx, cy)).min(1.0);
            if jetzt == Some(i) {
                zeilen.push(UiRect::rounded(sichtbar, mix_color(g(Slot::AccentSurface), g(Slot::AccentSurfaceHover), k), 6.0));
            } else if k > 0.0 {
                zeilen.push(UiRect::rounded(sichtbar, theme::with_alpha(g(Slot::Surface3), k), 6.0));
            }
            let Some(name) = optionen.get(i) else { continue };
            let text = Self::fit_text(gpu, name, r.w - 24.0, UI_SCALE_SMALL);
            let y = (r.y + (r.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
            let farbe = if jetzt == Some(i) { g(Slot::AccentText) } else { theme::text() };
            texts.push(UiText { text, x: r.x + 12.0, y, color: farbe, scale: UI_SCALE_SMALL });
        }
        let n = optionen.len();
        if n > DROPDOWN_MAX_ITEMS {
            let max = (n - DROPDOWN_MAX_ITEMS) as f32;
            let bahn = Rect { x: kasten.x + kasten.w - DROPDOWN_PAD - DROPDOWN_BAR_W, y: innen.0, w: DROPDOWN_BAR_W, h: innen.1 - innen.0 };
            rects.push(UiRect::rounded(bahn, g(Slot::Surface1), DROPDOWN_BAR_W / 2.0));
            let anteil = DROPDOWN_MAX_ITEMS as f32 / n as f32;
            let daumen_h = (bahn.h * anteil).max(24.0);
            let daumen_y = bahn.y + (bahn.h - daumen_h) * (d.scroll.stand / max).clamp(0.0, 1.0);
            let ueber = bahn.contains(cx, cy);
            rects.push(UiRect::rounded(Rect { y: daumen_y, h: daumen_h, ..bahn }, g(if ueber { Slot::Accent } else { Slot::Border2 }), DROPDOWN_BAR_W / 2.0));
            // Am Ende angekommen: eine Linie im Abstand zwischen Rahmen und erstem bzw. letztem
            // Eintrag, eingeblendet auf dem letzten Viertel Eintrag davor.
            let breite = kasten.w - 2.0 * DROPDOWN_PAD - DROPDOWN_BAR_W - DROPDOWN_PAD;
            for (deckung, y) in [((1.0 - d.scroll.stand * 4.0).clamp(0.0, 1.0), kasten.y + DROPDOWN_PAD / 2.0 - 1.0), ((1.0 - (max - d.scroll.stand) * 4.0).clamp(0.0, 1.0), kasten.y + kasten.h - DROPDOWN_PAD / 2.0 - 1.0)] {
                if deckung > 0.0 {
                    let linie = Rect { x: kasten.x + DROPDOWN_PAD + 6.0, y: y.round(), w: breite - 12.0, h: 2.0 };
                    rects.push(UiRect::rounded(linie, theme::with_alpha(g(Slot::Accent), deckung), 1.0));
                }
            }
        }
        overlay.groups.push(UiOverlayGroup { rects, texts: Vec::new(), sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: None });
        let clip = Rect { x: kasten.x, y: innen.0, w: kasten.w, h: innen.1 - innen.0 };
        overlay.groups.push(UiOverlayGroup { rects: zeilen, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0, clip: Some(clip) });
    }
}
