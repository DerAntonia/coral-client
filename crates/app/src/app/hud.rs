// HUD: Positionen und Verschieben der Elemente, Hotbar, Leisten, Effekte, Tab-Liste, Session-Info.

use crate::{theme, App, GpuState, Rect, UI_SCALE_SMALL, UiRect, UiText, Button, ButtonAction, styled_button, SHIELD_CD_TEXT_SCALE, CROSSHAIR_OUTLINE_PX, CROSSHAIR_SHAPES, CROSSHAIR_HANDLE_MARGIN, stable_row_width, UI_SCALE_NORMAL, align_left, UI_SCALE_TITLE, ShadowArea, EFFECTS_ALIGN_RIGHT, EFFECTS_ALIGN_BOTTOM, effect_hud_layout, HudElement, Settings, HUD_SNAP_PX, align_anchor, effects_detail_anchor, latch_width, COLOR_PERF, FPS_LIMIT_UNLIMITED, UiLine, UiSprite, AppMode, ConnStatus, GAME_MODE_CREATIVE, GAME_MODE_SPECTATOR, selection_boundary, inventory, item_shows_held_name, HELD_NAME_TICKS, span_row_width, push_styled_row, COLOR_WHITE, HELD_NAME_ABOVE_HOTBAR, CrosshairExtras, CustomCrosshair, crosshair_editor, CrosshairText, physics, voicechat, rgb_to_rgba, item_model_input, DurabilityBarLimit, item_bar, durability_rest, durability_points, SpriteAtlas, model_definition_id, item_icon_name, toteming, argb_to_rgba, ARROW_EMPTY_FEEDBACK_BLINK, ARROW_EMPTY_BLINK_SECS, ARROW_EMPTY_FEEDBACK_SHAKE, ARROW_EMPTY_SHAKE_SECS, ARROW_EMPTY_TINT, is_player_like, DEATH_ANIM_EXPLOSION, death_burst, DVec3, particles, DEATH_ANIM_VANILLA, Instant, session_info, SESSION_PAD, SESSION_MIN_W, SESSION_LINE_GAP, keystrokes, KeyBindAction, Bind, BindMouse, bind_display, max_damage_of, ArmorHudMetrics, armor_hud, COLOR_ARMOR_KEPT, COLOR_ARMOR_LOW, COLOR_ARMOR_HIGH, EffectEntry, effect_order, EffectDetailPlace, effect_longest_first, effect_display_name, format_effect_timer, EFFECTS_ALIGN_LEFT, effect_is_dark, push_span_shadow, effect_timer_blink, EFFECTS_BLINK_RED, effect_icon_alpha, format_effect_duration, clip_with_ellipsis, STATUS_BARS_ABOVE_HOTBAR, MAX_AIR_TICKS, COLOR_AIR_BAR, STATUS_BAR_WIDTH, STATUS_ICON_PITCH, STATUS_ICON, heart_variant, ABSORPTION_FADE_SECS, ABSORPTION_FADE_RISE, absorbing_heart_sprite, heart_sprite, STATUS_ROW_PITCH, armor_sprite, food_sprite, air_bubbles, BossBarState, BOSS_BAR_COLORS, BOSS_BAR_NOTCHES, XP_BAR_ABOVE_HOTBAR, XP_BAR_HEIGHT, XP_LEVEL_ABOVE_HOTBAR, COLOR_XP_LEVEL, TabEntry, PLAYER_LIST_MAX_ENTRIES, PLAYER_LIST_SCORE_COLOR, PLAYER_LIST_MAX_ROWS, PLAYER_LIST_HEARTS_WIDTH, wrap_label_lines, split_lines_vanilla, black_veil, PLAYER_LIST_BG_ALPHA, white_veil, PLAYER_LIST_ROW_ALPHA, PLAYER_LIST_SPECTATOR_ALPHA, COLOR_SCOREBOARD_BG, push_stable_spans, COLOR_HUD, ACTION_BAR_DURATION};

mod layout;
mod main_hud;
mod crosshair_totem;
mod items;
mod pvp;
mod armor_arrows;
mod bars_effects;
mod status_bars;
mod player_list;

impl App {
    // Hand-Debug: die Knoepfe rechts oben im Schwarzen, neben dem verkleinerten Bild (s.
    // GpuState::draw_hand_debug_ui). 0 = Arm wie mit leerer Hand, 1 = Standard (Werte des Items
    // loeschen), 2 = die Pfeiltasten zwischen Faust und Item umschalten. Darunter 3..=6: der
    // Bogen ungespannt und in den drei Spannstufen in die Hand (s. App::hand_debug_bow), egal was
    // man wirklich haelt; der gewaehlte steht in Klammern, nochmal klicken nimmt ihn weg.
    pub(crate) fn hand_debug_buttons(&self, gpu: &GpuState) -> Vec<(Rect, String)> {
        let (sw, _) = gpu.size();
        let h = gpu.text_height(UI_SCALE_SMALL) + 16.0;
        let modus = if self.hand_debug_item_mode { "Pfeile: Item" } else { "Pfeile: Hand" };
        let breite = |t: &str| gpu.measure_text(t, UI_SCALE_SMALL) + 32.0;
        // Rechtsbuendig nebeneinander, von rechts nach links gelegt.
        let reihe = |texte: Vec<String>, y: f32| {
            let mut x = sw - 20.0;
            let mut out: Vec<(Rect, String)> = texte
                .into_iter()
                .rev()
                .map(|t| {
                    let w = breite(&t);
                    x -= w;
                    let r = Rect { x, y, w, h };
                    x -= 10.0;
                    (r, t)
                })
                .collect();
            out.reverse();
            out
        };
        let gewaehlt = self.hand_debug_bow.as_ref().map(|(k, _)| *k);
        let boegen = ["Bogen", "Bogen 0", "Bogen 1", "Bogen 2"]
            .iter()
            .enumerate()
            .map(|(k, t)| if gewaehlt == Some(k as u8) { format!("[{t}]") } else { t.to_string() })
            .collect();
        let mut knoepfe = reihe(vec!["Leere Hand".to_string(), "Standard".to_string(), modus.to_string()], 20.0);
        knoepfe.extend(reihe(boegen, 20.0 + h + 10.0));
        let haende = [("Bogenhand", !self.hand_debug_string), ("Sehnenhand", self.hand_debug_string)]
            .iter()
            .map(|(t, an)| if *an { format!("[{t}]") } else { t.to_string() })
            .collect();
        knoepfe.extend(reihe(haende, 20.0 + 2.0 * (h + 10.0)));
        knoepfe
    }

    // Hand-Debug: Knoepfe und darunter die Werte des gehaltenen Items samt Tasten, rechtsbuendig.
    pub(crate) fn build_hand_debug_ui(&self, gpu: &GpuState) -> (Vec<UiRect>, Vec<UiText>) {
        use theme::Slot;
        let (sw, _) = gpu.size();
        let (cx, cy) = self.cursor_pos;
        let mut rects = Vec::new();
        let mut texts = Vec::new();
        let knoepfe = self.hand_debug_buttons(gpu);
        for (r, text) in knoepfe.iter().cloned() {
            let b = Button { slider: None, action: ButtonAction::Nothing, rect: r, label: String::new(), enabled: true, color: theme::get(Slot::Surface2), scale: UI_SCALE_SMALL };
            let (flaeche, farbe) = styled_button(&b, self.hover_level(r, r.contains(cx, cy)));
            rects.push(flaeche);
            texts.push(gpu.centered_text(r, text, farbe, UI_SCALE_SMALL));
        }
        let (main, off) = self.own_hand_items();
        let hand = if main.is_some() { protocol::packets::HAND_MAIN } else { protocol::packets::HAND_OFF };
        let item = main.or(off);
        let (name, w) = match self.hand_debug_string.then(|| self.string_hand()).flatten() {
            Some((_, name)) => {
                let w = self.string_hand_tuning(&name);
                (name, w)
            }
            None if self.hand_debug_string => ("-".to_string(), [0; 16]),
            None => (item.and_then(|i| self.tuning_name(i, hand)).unwrap_or_else(|| "-".to_string()), item.map(|i| self.hand_tuning(i, hand)).unwrap_or_default()),
        };
        let zeilen = [
            format!("{name}  Faust  x {:+.3}  y {:+.3}  z {:+.3}", w[0] as f32 / 1000.0, w[1] as f32 / 1000.0, w[2] as f32 / 1000.0),
            format!("Drehung  x {:+}  y {:+}  z {:+}  Arm {:+} Grad", w[3], w[4], w[5], w[6]),
            format!("Schulter  x {:+}  y {:+}  z {:+} Grad", w[7], w[8], w[9]),
            format!("Item  x {:+.3}  y {:+.3}  z {:+.3}  Drehung x {:+}  y {:+}  z {:+} Grad", w[10] as f32 / 1000.0, w[11] as f32 / 1000.0, w[12] as f32 / 1000.0, w[13], w[14], w[15]),
            "Pfeiltasten: links/rechts, hoch/runter; PgUp/PgDn: vor/zurück".to_string(),
        ];
        let hinweise: &[&str] = if self.hand_debug_item_mode {
            &["Mit Alt: Item um den Griff drehen (hoch/runter kippen, links/rechts schwenken, PgUp/PgDn rollen)"]
        } else {
            &["Mit Alt: hoch/runter kippen, links/rechts schwenken, PgUp/PgDn rollen, Komma/Punkt Arm verdrehen", "Mit Strg: Schulter um die Faust drehen"]
        };
        let zeilen = zeilen.into_iter().chain(hinweise.iter().map(std::string::ToString::to_string)).chain(["Rücktaste: Arm wie mit leerer Hand / Standard".to_string()]);
        let zeile_h = gpu.text_height(UI_SCALE_SMALL) + 6.0;
        let mut y = knoepfe.iter().map(|(r, _)| r.y + r.h).fold(0.0, f32::max) + 16.0;
        for text in zeilen {
            let x = (sw - 20.0 - gpu.measure_text(&text, UI_SCALE_SMALL)).round();
            texts.push(UiText { text, x, y: y.round(), color: theme::text(), scale: UI_SCALE_SMALL });
            y += zeile_h;
        }
        (rects, texts)
    }
}
