// Toteming, Pfeile, Todeseffekte, Sitzungsinfo und Tastenanzeige.

use super::*;

impl App {
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
            .map_or(0.0, |y| (feet.1 - f64::from(y + 1)).max(0.0) as f32);
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
            // Maustasten: ein Doppelklick leuchtet in Akzent 2, solange die Taste unten ist.
            let maus = matches!(taste, keystrokes::Key::Attack | keystrokes::Key::Use);
            let doppelt = maus && kb.binds(aktion).any(|b| self.binds_down.contains(&b) && self.bind_presses.iter().any(|(p, _, d)| *p == b && *d));
            let r = Rect { x: (rect.x + x * u).round(), y: (rect.y + y * u).round(), w: (w * u).round(), h: (h * u).round() };
            let (grund, schrift) = if unten {
                let akzent = if doppelt { theme::Slot::Accent2 } else { theme::Slot::Accent };
                (theme::get(akzent), theme::get(theme::Slot::TextOnAccent))
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
}
