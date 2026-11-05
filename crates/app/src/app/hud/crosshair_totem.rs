// Fadenkreuz-Zugaben, Totem-Anzeige, Voice-HUD und Lage der Hotbar.

use super::*;

impl App {
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
                    yellow: false,
                    font,
                    alpha: 1.0,
                    fade_slot: crate::CROSSHAIR_FADE_SHIELD,
                    stable_digits: false,
                });
            }
        }
        // Mending-Ring der Ruestung: nur mit XP-Flaschen in einer Hand, dann vor Timer und Schild.
        let flaschen = [protocol::packets::HAND_MAIN, protocol::packets::HAND_OFF]
            .into_iter()
            .any(|hand| self.own_hand_stack(hand).and_then(|s| world::item_registry::item_name(s.item_id)) == Some("experience_bottle"));
        if flaschen && ring_frei && set.module_armor_hud && set.armor_hud_ring && self.game_mode != GAME_MODE_SPECTATOR {
            if let Some(anteil) = self.mending_armor_fraction() {
                extras.ring = Some(((0.5, 0.5), anteil));
            }
        }
        if let Some((text, rect, scale)) = self.arrow_number(gpu) {
            extras.texts.push(CrosshairText {
                text,
                center_px: (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                scale,
                white: set.arrow_text_style == 1,
                yellow: self.has_loaded_crossbow(),
                font: set.effective_font(ShadowArea::ArrowNumber),
                alpha: set.arrow_opacity as f32 / 100.0,
                fade_slot: crate::CROSSHAIR_FADE_ARROWS,
                stable_digits: set.stable_numbers,
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
}
