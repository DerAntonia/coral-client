// Settings laden und aus Text lesen.

use super::*;

impl Settings {
    // Fehlt oder klemmt die Datei, gelten die Standardwerte: Einstellungen sind nichts,
    // wofuer ein Client den Start verweigern sollte.
    pub(crate) fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        let privat = private_settings_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::parse(&privat);
        };
        // Der private Teil zuletzt: er gilt vor dem, was in der Config steht.
        let settings = Self::parse(&format!("{text}\n{privat}"));
        // Config von vor der Trennung (oder von Hand mit privaten Werten ergaenzt): gleich
        // aufteilen, damit beim Weitergeben kein Konto mitwandert.
        if !split_private_settings(&text).1.is_empty() {
            let _ = settings.save();
        }
        settings
    }

    // Getrennt vom Dateizugriff, damit das Klemmen der Werte pruefbar ist (s. Tests
    // unten): eine von Hand editierte Datei darf den Client nicht in einen unmoeglichen
    // Zustand bringen, sondern muss auf gueltige Werte zurechtgebogen werden.
    // Farben eines Themes; ein verschwundenes eigenes Theme faellt auf den Standard zurueck.
    pub(crate) fn theme_colors(&self, wahl: ThemeChoice) -> theme::Colors {
        match wahl {
            ThemeChoice::Preset(i) => theme::PRESETS.get(i).map_or_else(theme::default_colors, |p| p.colors),
            ThemeChoice::Custom(i) => self.custom_themes.get(i).map_or_else(theme::default_colors, |t| t.colors),
        }
    }

    pub(crate) fn select_theme(&mut self, wahl: ThemeChoice) {
        self.theme_choice = wahl;
        self.theme = self.theme_colors(wahl);
        theme::apply(&self.theme);
    }

    pub(crate) fn parse(text: &str) -> Self {
        Self::parse_onto(Self::default(), text)
    }

    // Wie parse, aber auf `settings` statt auf den Standardwerten -- so entstehen die
    // Standardwerte selbst (s. Default): die Grundwerte aus base, darueber default-settings.conf.
    pub(crate) fn parse_onto(mut settings: Self, text: &str) -> Self {
        // Fernsicht aus einer settings.conf von vor der LOD-Render-Distance: dort zeichnete sie
        // nur mit eingeschaltetem Modul und "Fernsicht zeichnen", in "chunk_saver_distance".
        let mut lod_neu = false;
        let (mut alt_zeichnen, mut alt_weite) = (true, lod::DISTANCE_LEGACY_DEFAULT);
        // Dateien von vor der Theme-Farbe "Markierung": sie folgt dem Akzent wie in den Presets,
        // sonst galte ein Preset mit anderem Akzent danach als eigene Aenderung.
        let mut markierung_gelesen = false;
        let mut akzent2_gelesen = false;
        let mut akzent2_toene_gelesen = false;
        let mut theme_wahl: Option<String> = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if Self::parse_keys_display(&mut settings, key, value)
                || Self::parse_keys_hud_chat(&mut settings, key, value)
                || Self::parse_keys_shader_crosshair(&mut settings, key, value)
                || Self::parse_keys_render_modules(&mut settings, key, value)
                || Self::parse_keys_hitbox_armor(&mut settings, key, value)
                || Self::parse_keys_modules(&mut settings, key, value)
                || Self::parse_keys_sky_skins(&mut settings, key, value)
            {
                continue;
            }
            match key {
                k if k.starts_with("addon:") => {
                    settings.addon_values.insert(k["addon:".len()..].to_string(), value.to_string());
                }
                k if k.starts_with("ui_volume_") => {
                    let i = ui_sound::UiSound::ALL.iter().position(|s| k == format!("ui_volume_{}", s.key()));
                    if let (Some(i), Some(v)) = (i, parse_volume(value)) {
                        settings.ui_sound_volume[i] = v;
                    }
                }
                k if k.starts_with("volume_") => {
                    let i = VOLUME_CATEGORIES.iter().position(|c| k == format!("volume_{}", c.key));
                    if let (Some(i), Some(v)) = (i, parse_volume(value)) {
                        settings.category_volume[i] = v;
                    }
                }
                k if ShadowArea::ALL.iter().any(|a| a.key() == k) => {
                    if let (Some(a), Ok(v)) = (ShadowArea::ALL.iter().find(|a| a.key() == k), value.parse::<u8>()) {
                        settings.area_shadow[a.index()] = v.min(AREA_SHADOW_CHOICES - 1);
                    }
                }
                k if ShadowArea::ALL.iter().any(|a| a.font_key() == k) => {
                    if let (Some(a), Ok(v)) = (ShadowArea::ALL.iter().find(|a| a.font_key() == k), value.parse::<u8>()) {
                        settings.area_font[a.index()] = if (v as usize) <= FontKind::ALL.len() { v } else { 0 };
                    }
                }
                "wallpaper" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.wallpaper = wallpaper::WallpaperMode::from_index(v).index();
                    }
                }
                "font" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.font = if (v as usize) < FontKind::ALL.len() { v } else { 0 };
                    }
                }
                // Frueher ein eigener Schalter "Explosion AUS", jetzt die Art Explosion.
                "explosion_particles" => {
                    settings.particle_tuning[particles::KIND_EXPLOSION].enabled = value != "false";
                    settings.particle_tuning_own[particles::KIND_EXPLOSION].enabled = value != "false";
                }
                // Die eigenen Partikel uebernehmen erst die Werte der anderen -- so bleibt eine
                // Datei von vor der Trennung gleich; ihr eigener Eintrag kommt danach (s. serialize).
                "particle_tuning" => {
                    read_particle_tuning(&mut settings.particle_tuning, value);
                    settings.particle_tuning_own = settings.particle_tuning;
                }
                "particle_tuning_own" => {
                    let mut eigene = [particles::ParticleTuning::default(); particles::KINDS.len()];
                    read_particle_tuning(&mut eigene, value);
                    settings.particle_tuning_own = eigene;
                }
                _ if teams::Side::ALL.iter().any(|s| s.key() == key) => {
                    let seite = *teams::Side::ALL.iter().find(|s| s.key() == key).expect("eben geprueft");
                    settings.team_hitboxes[seite.index()] = teams::TeamHitbox::decode(value, seite.default_style());
                }
                _ if key.strip_prefix("hitbox_").and_then(hitbox_kinds::HitboxKind::from_key).is_some() => {
                    let art = key.strip_prefix("hitbox_").and_then(hitbox_kinds::HitboxKind::from_key).expect("eben geprueft");
                    settings.module_hitbox_styles[art.index()] = hitbox_kinds::HitboxStyle::decode(value, art.default_style());
                }
                "trail_hold_tenths" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.trail_hold_tenths = v;
                    }
                }
                "trail_fade_tenths" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.trail_fade_tenths = v;
                    }
                }
                "shield_cd_self" => settings.shield_cd_self = value == "true",
                "shield_cd_players" => settings.shield_cd_players = value == "true",
                "shield_hold_players" => settings.shield_hold_players = value == "true",
                "shield_cd_mark" => settings.shield_cd_mark = u8::from(value == "1"),
                "shield_hold_mark" => settings.shield_hold_mark = u8::from(value == "1"),
                "shield_cd_self_mode" => settings.shield_cd_self_mode = u8::from(value == "1"),
                // Frueher eigene Schrift-Einstellungen im Modul (FontKind-Index, ohne
                // "Allgemein") -- als feste Bereichs-Schrift uebernommen.
                "shield_cd_font" => {
                    if let Some(v) = value.parse::<u8>().ok().filter(|v| (*v as usize) < FontKind::ALL.len()) {
                        settings.area_font[ShadowArea::ShieldCountdown.index()] = v + 1;
                    }
                }
                "shield_cd_text_style" => settings.shield_cd_text_style = u8::from(value == "1"),
                "arrow_text_style" => settings.arrow_text_style = u8::from(value == "1"),
                "arrow_text_font" => {
                    if let Some(v) = value.parse::<u8>().ok().filter(|v| (*v as usize) < FontKind::ALL.len()) {
                        settings.area_font[ShadowArea::ArrowNumber.index()] = v + 1;
                    }
                }
                "shield_cd_hue" | "shield_cd_saturation" | "shield_cd_value" | "shield_cd_alpha" | "shield_hold_hue"
                | "shield_hold_saturation" | "shield_hold_value" | "shield_hold_alpha" => {
                    if let Ok(v) = value.parse::<u32>() {
                        let (farbe, achse) = match key {
                            "shield_cd_hue" => (ShieldColor::Cooldown, DamageTintAxis::Hue),
                            "shield_cd_saturation" => (ShieldColor::Cooldown, DamageTintAxis::Saturation),
                            "shield_cd_value" => (ShieldColor::Cooldown, DamageTintAxis::Value),
                            "shield_cd_alpha" => (ShieldColor::Cooldown, DamageTintAxis::Alpha),
                            "shield_hold_hue" => (ShieldColor::Hold, DamageTintAxis::Hue),
                            "shield_hold_saturation" => (ShieldColor::Hold, DamageTintAxis::Saturation),
                            "shield_hold_value" => (ShieldColor::Hold, DamageTintAxis::Value),
                            _ => (ShieldColor::Hold, DamageTintAxis::Alpha),
                        };
                        let (min, max, _) = achse.range();
                        if let Some(wert) = settings.shield_color_mut(farbe, achse) {
                            *wert = v.clamp(min, max);
                        }
                    }
                }
                "chunk_saver_render" => alt_zeichnen = value == "true",
                "chunk_saver_render_saved" => settings.chunk_saver_render_saved = value == "true",
                "creative_op_tab" => settings.creative_op_tab = value == "true",
                // Alte Formen von server_close_blocked: An/Aus (An = alles bleibt offen), dann
                // No UI close mit 0 nie, 1 immer, 2 nur das eigene Inventar.
                "no_ui_close_s2c" => settings.server_close_blocked = if value == "true" { u8::MAX } else { 0 },
                "no_ui_close" => {
                    settings.server_close_blocked = match value {
                        "1" => u8::MAX,
                        "2" => 1 << SERVER_CLOSE_INVENTORY_ROW,
                        _ => 0,
                    }
                }
                "server_close_blocked" => settings.server_close_blocked = value.parse().unwrap_or(0),
                "server_open" => {
                    for (ziel, v) in settings.server_open.iter_mut().zip(value.split(',')) {
                        *ziel = v.parse::<u8>().unwrap_or(SERVER_OPEN_ALLOW).min(SERVER_OPEN_IGNORE);
                    }
                }
                "no_chat_clear" => settings.no_chat_clear = value == "true",
                "audio_auto_optimize" => settings.audio_auto_optimize = value == "true",
                "skip_third_back" => settings.skip_third_back = value == "true",
                "skip_third_front" => settings.skip_third_front = value == "true",
                "sign_edit_in_world" => settings.sign_edit_in_world = value != "false",
                "no_slot_set" => settings.no_slot_set = value.parse::<u8>().unwrap_or(SERVER_SET_FOLLOW).min(SERVER_SET_RESET),
                "no_rotate_set" => settings.no_rotate_set = value.parse::<u8>().unwrap_or(SERVER_SET_FOLLOW).min(SERVER_SET_RESET),
                "potion_mark_strong" => settings.potion_mark_strong = value != "false",
                "potion_mark_long" => settings.potion_mark_long = value != "false",
                "potion_mark_weak" => settings.potion_mark_weak = value != "false",
                "inventory_merge_hover" => settings.inventory_merge_hover = value == "true",
                "slot_hover_fade_ms" => settings.slot_hover_fade_ms = value.parse::<u32>().unwrap_or(100).min(SLOT_HOVER_FADE_MAX),
                "slot_hover_color" => {
                    for (i, (ziel, v)) in settings.slot_hover_color.iter_mut().zip(value.split('/')).enumerate() {
                        if let Ok(v) = v.parse::<u32>() {
                            *ziel = if i == 0 { v % 360 } else { v.min(100) };
                        }
                    }
                }
                "swap_anim_ms" => settings.swap_anim_ms = value.parse::<u32>().unwrap_or(150).min(SWAP_ANIM_MAX),
                "chunk_saver_threads" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.chunk_saver_threads = v.clamp(1, lod::THREADS_MAX);
                    }
                }
                "chunk_saver_detail" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.chunk_saver_threshold = v;
                    }
                }
                "chunk_saver_distance" => {
                    if let Ok(v) = value.parse::<u32>() {
                        alt_weite = v.clamp(lod::DISTANCE_STEP, lod::DISTANCE_MAX);
                    }
                }
                "lod_render_distance" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.lod_render_distance = v.min(lod::DISTANCE_MAX);
                        lod_neu = true;
                    }
                }
                k if k.starts_with("scroll_") => {
                    if let Ok(v) = value.parse::<usize>() {
                        settings.settings_scrolls.insert(k["scroll_".len()..].to_string(), v);
                    }
                }
                "theme_choice" => theme_wahl = Some(value.to_string()),
                "custom_theme" => settings.custom_themes.extend(parse_custom_theme(value)),
                k if k.starts_with("theme_") => {
                    let slot = theme::Slot::ALL.into_iter().find(|s| &k["theme_".len()..] == s.key());
                    if let (Some(slot), Some(c)) = (slot, theme::parse_hex(value)) {
                        settings.theme[slot.index()] = c;
                        markierung_gelesen |= slot == theme::Slot::Selection;
                        akzent2_gelesen |= slot == theme::Slot::Accent2;
                        akzent2_toene_gelesen |= slot == theme::Slot::Accent2Hover;
                    }
                }
                "lang_ui" | "lang_items" | "lang_effects" | "lang_chat" => {
                    if let Ok(v) = value.parse::<u8>() {
                        let v = i18n::Language::from_index(v).index();
                        match key {
                            "lang_ui" => settings.lang_ui = v,
                            "lang_items" => settings.lang_items = v,
                            "lang_effects" => settings.lang_effects = v,
                            _ => settings.lang_chat = v,
                        }
                    }
                }
                "msa_refresh_token" => settings.msa_refresh_token = value.to_string(),
                "account_name" => settings.account_name = value.to_string(),
                "mc_access_token" => settings.mc_access_token = value.to_string(),
                "account_uuid" => settings.account_uuid = value.to_string(),
                "block_rng_rotation" => settings.block_rng_rotation = value == "true",
                // Alter Schluessel aus der Zeit des reinen An/Aus-Schalters.
                "smooth_lighting" => settings.smooth_lighting_percent = if value == "true" { SMOOTH_LIGHTING_MAX } else { 0 },
                "smooth_lighting_percent" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.smooth_lighting_percent = v;
                    }
                }
                "motion_blur_percent" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.motion_blur_percent = v.min(MOTION_BLUR_MAX);
                    }
                }
                "jump_cooldown_enabled" => settings.jump_cooldown_enabled = value == "true",
                "sprint_on_start" => settings.sprint_on_start = value == "true",
                "caps_lock_blocked" => settings.caps_lock_blocked = value == "true",
                "sneak_edge_protection_enabled" => settings.sneak_edge_protection_enabled = value == "true",
                "snap_tap_enabled" => settings.snap_tap_enabled = value == "true",
                "use_while_mining" => settings.use_while_mining = value == "true",
                "hold_use_repeat_rockets" => settings.hold_use_repeat_rockets = value == "true",
                "no_extinguish_dc_ghost" => settings.no_extinguish_dc_ghost = value == "true",
                "hold_use_repeat" => settings.hold_use_repeat = HoldUseRepeat::from_key(value),
                // Tastenbelegungen: EIN Zweig fuer alle Aktionen statt einer Zeile je
                // Aktion (s. KeyBindAction::settings_key) -- sonst muesste jede neue Aktion
                // an vier Stellen nachgetragen werden. Die Schluesselnamen bleiben
                // unveraendert, alte Konfigdateien lesen sich also weiter.
                _ if key.starts_with("keybind_mode_") => {
                    let name = &key["keybind_mode_".len()..];
                    if let Some(action) = KeyBindAction::ALL.into_iter().find(|a| a.settings_key() == name) {
                        set_keybind_mode_from_str(&mut settings.keybind_modes, action, value);
                    }
                }
                _ if settings.toteming.parse_line(key, value) => {}
                _ if settings.voice.parse_line(key, value) => {}
                _ if key.starts_with("keybind_") => {
                    let name = &key["keybind_".len()..];
                    if let Some(action) = KeyBindAction::ALL.into_iter().find(|a| a.settings_key() == name) {
                        set_keybind_from_str(&mut settings.keybinds, action, value);
                    }
                }
                _ => {}
            }
        }
        if !lod_neu && settings.module_chunk_saver && alt_zeichnen {
            settings.lod_render_distance = alt_weite;
        }
        if !markierung_gelesen {
            settings.theme[theme::Slot::Selection.index()] = settings.theme[theme::Slot::Accent.index()];
        }
        if !akzent2_gelesen {
            settings.theme[theme::Slot::Accent2.index()] = theme::accent2_for(settings.theme[theme::Slot::Accent.index()]);
        }
        if !akzent2_toene_gelesen {
            theme::derive_accent2_tones(&mut settings.theme);
        }
        let wahl = match theme_wahl.as_deref().and_then(|v| v.split_once(':')) {
            Some(("preset", name)) => theme::PRESETS.iter().position(|p| p.name == name).map(ThemeChoice::Preset),
            Some(("custom", i)) => i.parse().ok().filter(|&i| i < settings.custom_themes.len()).map(ThemeChoice::Custom),
            _ => None,
        };
        settings.theme_choice = match wahl {
            Some(w) => w,
            // Datei von vor den eigenen Themes: ein Preset, wenn die Farben genau eines sind,
            // sonst werden die geaenderten Farben ein eigenes Theme.
            None => if let Some(i) = theme::preset_of(&settings.theme) { ThemeChoice::Preset(i) } else {
                settings.custom_themes.push(CustomTheme { name: "Eigenes Theme".to_string(), colors: settings.theme });
                ThemeChoice::Custom(settings.custom_themes.len() - 1)
            },
        };
        settings.theme = settings.theme_colors(settings.theme_choice);
        // Ohne "X/Y getrennt" stehen beide Regler auf demselben Wert (s. ButtonAction::
        // SensitivityUp); aeltere Dateien hatten dort noch ein ungenutztes Y.
        if !settings.mouse_sens_split {
            settings.mouse_sensitivity_y = settings.mouse_sensitivity;
        }
        settings
    }

    // Schluessel von "show_tearing_test" bis "hud_held_name_x". true = erkannt.
    fn parse_keys_display(settings: &mut Self, key: &str, value: &str) -> bool {
        match key {
            "show_tearing_test" => settings.show_tearing_test = value == "true",
            "show_hud" => settings.show_hud = value == "true",
            // Alter Schluessel: darin stand der INDEX in die frueher feste Optionsliste.
            // Beim Laden auf den echten Hz-Wert heben, damit eine bestehende
            // settings.conf ihren gewaehlten Deckel behaelt.
            "fps_limit" => {
                if let Ok(i) = value.parse::<usize>() {
                    settings.fps_limit = LEGACY_FPS_LIMIT_OPTIONS.get(i).copied().flatten().unwrap_or(FPS_LIMIT_MONITOR);
                }
            }
            "fps_limit_hz" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.fps_limit = v;
                }
            }
            "cursor_circle" => settings.cursor_circle = value == "true",
            "hold_confirm_ms" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.hold_confirm_ms = v;
                }
            }
            "reconnect_hold_ms" => {
                if let Ok(v) = value.parse::<i32>() {
                    settings.reconnect_hold_ms = v;
                }
            }
            "cursor_width" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.cursor_width = v;
                }
            }
            "cursor_size" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.cursor_size = v;
                }
            }
            "cursor_sensitivity" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.cursor_sensitivity = v;
                }
            }
            "mouse_sens_split" => settings.mouse_sens_split = value == "true",
            "mouse_sensitivity_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.mouse_sensitivity_y = v;
                }
            }
            "mouse_sensitivity" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.mouse_sensitivity = v;
                }
            }
            "mesh_columns_per_frame" => {
                if let Ok(v) = value.parse::<usize>() {
                    settings.mesh_columns_per_frame = v;
                }
            }
            "max_render_distance" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.max_render_distance = v;
                }
            }
            "chunk_update_threads" => {
                if let Ok(v) = value.parse::<usize>() {
                    settings.chunk_update_threads = v;
                }
            }
            "frame_latency" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.frame_latency = v.clamp(FRAME_LATENCY_MIN, FRAME_LATENCY_MAX);
                }
            }
            "hud_perf_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_perf_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_perf_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_perf_y = v.clamp(0.0, 1.0);
                }
            }
            "hud_actionbar_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_actionbar_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_actionbar_align" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.hud_actionbar_align = v.min(2);
                }
            }
            "hud_title_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_title_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_title_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_title_y = v.clamp(0.0, 1.0);
                }
            }
            "hud_title_align" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.hud_title_align = v.min(2);
                }
            }
            "hud_subtitle_align" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.hud_subtitle_align = v.min(2);
                }
            }
            "hud_subtitle_x" | "hud_subtitle_y" | "hud_effects_x" | "hud_effects_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    let v = v.clamp(0.0, 1.0);
                    match key {
                        "hud_subtitle_x" => settings.hud_subtitle_x = v,
                        "hud_subtitle_y" => settings.hud_subtitle_y = v,
                        "hud_effects_x" => settings.hud_effects_x = v,
                        _ => settings.hud_effects_y = v,
                    }
                }
            }
            "show_effects_hud" => settings.show_effects_hud = value == "true",
            "show_effects_inventory" => settings.show_effects_inventory = value == "true",
            "effects_detailed" => settings.effects_detailed = value == "true",
            "effects_hide_ambient" => settings.effects_hide_ambient = value == "true",
            "effects_icon" => settings.effects_icon = value == "true",
            "effects_name" => settings.effects_name = value == "true",
            // Frueher ein Schalter (true/false), jetzt EFFECTS_BLINK_MODES.
            "effects_timer_blink" => {
                settings.effects_timer_blink = match value {
                    "true" => EFFECTS_BLINK_ON,
                    "false" => EFFECTS_BLINK_OFF,
                    v => v.parse::<u8>().map_or(EFFECTS_BLINK_ON, |m| m.min(EFFECTS_BLINK_MODES.len() as u8 - 1)),
                }
            }
            "effects_name_color" => settings.effects_name_color = value == "true",
            "effects_timer_color" => settings.effects_timer_color = value == "true",
            "effects_align" | "effects_timer_style" => {
                if let Ok(v) = value.parse::<u8>() {
                    match key {
                        "effects_align" => settings.effects_align = v.min(EFFECTS_ALIGNS.len() as u8 - 1),
                        _ => settings.effects_timer_style = v.min(EFFECTS_TIMER_STYLES.len() as u8 - 1),
                    }
                }
            }
            "hud_effects_detail_x" | "hud_effects_detail_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    let v = v.clamp(0.0, 1.0);
                    if key == "hud_effects_detail_x" {
                        settings.hud_effects_detail_x = v;
                    } else {
                        settings.hud_effects_detail_y = v;
                    }
                }
            }
            "hud_title_scale" | "hud_subtitle_scale" | "hud_effects_scale" | "hud_bossbar_scale" | "hud_scoreboard_scale" | "hud_arrows_scale" | "hud_held_name_scale" | "hud_armor_scale" | "hud_keys_scale" | "hud_session_scale" => {
                if let Ok(v) = value.parse::<u32>() {
                    match key {
                        "hud_title_scale" => settings.hud_title_scale = v,
                        "hud_subtitle_scale" => settings.hud_subtitle_scale = v,
                        "hud_bossbar_scale" => settings.hud_bossbar_scale = v,
                        "hud_scoreboard_scale" => settings.hud_scoreboard_scale = v,
                        "hud_arrows_scale" => settings.hud_arrows_scale = v,
                        "hud_held_name_scale" => settings.hud_held_name_scale = v,
                        "hud_armor_scale" => settings.hud_armor_scale = v,
                        "hud_keys_scale" => settings.hud_keys_scale = v,
                        "hud_session_scale" => settings.hud_session_scale = v,
                        _ => settings.hud_effects_scale = v,
                    }
                }
            }
            "hud_actionbar_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_actionbar_y = v.clamp(0.0, 1.0);
                }
            }
            "hud_held_name_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_held_name_x = v.clamp(0.0, 1.0);
                }
            }
            _ => return false,
        }
        true
    }

    // Schluessel von "hud_held_name_y" bis "master_volume". true = erkannt.
    fn parse_keys_hud_chat(settings: &mut Self, key: &str, value: &str) -> bool {
        match key {
            "hud_held_name_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_held_name_y = if v < 0.0 { -1.0 } else { v.min(1.0) };
                }
            }
            "hud_scoreboard_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_scoreboard_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_scoreboard_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_scoreboard_y = v.clamp(0.0, 1.0);
                }
            }
            "hud_hotbar_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_hotbar_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_hotbar_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_hotbar_y = v.clamp(0.0, 1.0);
                }
            }
            "hud_bossbar_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_bossbar_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_bossbar_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_bossbar_y = v.clamp(0.0, 1.0);
                }
            }
            "inventory_scale" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.inventory_scale = v.min(inventory::GUI_SCALE_SETTING_MAX);
                }
            }
            "hotbar_durability_absolute" => settings.hotbar_durability_absolute = value == "true",
            "hotbar_air_bar" => settings.hotbar_air_bar = value == "true",
            "spectator_speed" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.spectator_speed = spectator::nearest_speed(v);
                }
            }
            "hotbar_durability_points" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.hotbar_durability_points = nearest_durability_points(v);
                }
            }
            "hotbar_durability_percent" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.hotbar_durability_percent = v.clamp(HOTBAR_DURABILITY_STEP, 100);
                }
            }
            "hud_hotbar_scale" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.hud_hotbar_scale = v.min(inventory::GUI_SCALE_SETTING_MAX);
                }
            }
            // Alte Fassung (Auto/x1..x4) -> Prozent: xk ist k/2 von UI_SCALE_SMALL (x2).
            "hud_chat_scale" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_scale_percent = if v == 0 { 100 } else { (v.min(4) * 50).max(CHAT_SCALE_MIN) };
                }
            }
            "chat_scale_percent" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_scale_percent = v;
                }
            }
            "chat_width" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_width = v;
                }
            }
            "chat_lines" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_lines = v;
                }
            }
            "chat_lines_open" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_lines_open = v;
                }
            }
            "chat_open_dim" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_open_dim = v;
                }
            }
            "chat_fade_secs" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_fade_secs = v;
                }
            }
            "chat_bedrock" => settings.chat_bedrock = value == "1",
            "chat_line_spacing" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_line_spacing = v.min(CHAT_LINE_SPACING_MAX);
                }
            }
            "chat_background" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.chat_background = v;
                }
            }
            "hud_chat_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_chat_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_chat_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_chat_y = v.clamp(0.0, 1.0);
                }
            }
            "master_volume" => {
                if let Some(v) = parse_volume(value) {
                    settings.master_volume = v;
                }
            }
            _ => return false,
        }
        true
    }

    // Schluessel von "inventory_cursor_to_slot" bis "shadow_thickness". true = erkannt.
    fn parse_keys_shader_crosshair(settings: &mut Self, key: &str, value: &str) -> bool {
        match key {
            "inventory_cursor_to_slot" => settings.inventory_cursor_to_slot = value == "true",
            "shader_block_shadows" => settings.shader_block_shadows = value == "true",
            // Frueher ein Schalter: "true" ist die Ansicht der Arten.
            "shader_lightmap_debug" => settings.shader_lightmap_debug = if value == "true" { 1 } else { value.parse().unwrap_or(0).min(LIGHTMAP_DEBUG_NAMES.len() as u8 - 1) },
            "light_factors" => {
                settings.light_factors = value
                    .split(';')
                    .filter_map(|e| {
                        let (name, pct) = e.split_once(':')?;
                        let pct = pct.trim().parse::<u32>().ok()?;
                        // Fruehere Versionen fuehrten Wandfackeln einzeln.
                        let name = world::light::light_source_name(name.trim());
                        (!name.is_empty() && pct != 100).then(|| (name.to_string(), pct))
                    })
                    .collect();
                settings.light_factors.sort();
                settings.light_factors.dedup_by(|a, b| a.0 == b.0);
            }
            "shader_block_softness_deg" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.shader_block_softness_deg = v;
                }
            }
            "shader_sun_size_tenths" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.shader_sun_size_tenths = v;
                }
            }
            "shader_scattering_percent" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.shader_scattering_percent = v;
                }
            }
            "inventory_transparency" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.inventory_transparency = v;
                }
            }
            // Aeltere Dateien ohne eigenen Schalter: der Ring folgte der Kontur des Kreuzes.
            "crosshair_outline" => {
                settings.crosshair_outline = value == "true";
                settings.crosshair_ring_outline = settings.crosshair_outline;
            }
            "crosshair_ring_outline" => settings.crosshair_ring_outline = value == "true",
            "crosshair_custom" => settings.crosshair_custom = value == "true",
            "crosshair_shape" => {
                if let Ok(v) = value.parse::<usize>() {
                    settings.crosshair_shape = v.min(CROSSHAIR_SHAPES.len() - 1);
                }
            }
            "crosshair_sizes" => {
                for (i, teil) in value.split(',').take(CROSSHAIR_SHAPES.len()).enumerate() {
                    if let Ok(v) = teil.trim().parse::<u32>() {
                        settings.crosshair_sizes[i] = v;
                    }
                }
            }
            "crosshair_pixels" => {
                if let Some(p) = crosshair_editor::Pattern::decode(value) {
                    settings.crosshair_pattern = p;
                }
            }
            "crosshair_brightness_boost" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.crosshair_brightness_boost = v;
                }
            }
            "crosshair_color_mode" => {
                if let Ok(v) = value.parse::<usize>() {
                    settings.crosshair_color_mode = v.min(CROSSHAIR_COLOR_MODES.len() - 1);
                    // Aeltere Dateien: 1 Weiss, 2 Schwarz, 3 Rot als eigene Modi. Jetzt
                    // Einfarbig in dieser Farbe; neuere Dateien schreiben die Farbe danach.
                    let alt = match v {
                        2 => Some((0, 0, 0)),
                        3 => Some((0, 100, 100)),
                        _ => None,
                    };
                    if let Some((h, s, w)) = alt {
                        (settings.crosshair_hue, settings.crosshair_saturation, settings.crosshair_value) = (h, s, w);
                    }
                }
            }
            "crosshair_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.crosshair_hue = v % 360;
                }
            }
            "crosshair_saturation" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.crosshair_saturation = v.min(100);
                }
            }
            "crosshair_value" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.crosshair_value = v.min(100);
                }
            }
            "fov" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.fov = v;
                }
            }
            "fov_third_back" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.fov_third_back = v;
                }
            }
            "fov_third_front" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.fov_third_front = v;
                }
            }
            "camera_distance_back" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.camera_distance_back = v;
                }
            }
            "camera_distance_front" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.camera_distance_front = v;
                }
            }
            "camera_y_offset_back" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.camera_y_offset_back = v;
                }
            }
            "camera_y_offset_front" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.camera_y_offset_front = v;
                }
            }
            "tear_reserve_us" => {
                if let Ok(v) = value.parse::<i32>() {
                    settings.tear_reserve_us = v;
                }
            }
            "tear_timing_us" => {
                if let Ok(v) = value.parse::<i32>() {
                    settings.tear_timing_us = v;
                }
            }
            // Frueher "Feinabgleich": wie weit nach vorn, also das Timing mit Minus.
            "tear_latency_us" => {
                if let Ok(v) = value.parse::<i32>() {
                    settings.tear_timing_us = -v;
                }
            }
            "tear_position" => {
                if let Ok(v) = value.parse::<u8>() {
                    // Alte Dateien: 1..3 waren Oben/Unten/Austastluecke -- jede Steuerung wird "An".
                    settings.tear_position = u8::from(v != 0);
                }
            }
            "projection_mode" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.projection_mode = if (v as usize) < PROJECTION_NAMES.len() { v } else { 0 };
                }
            }
            "text_shadow" => settings.text_shadow = value == "true",
            "stable_numbers" => settings.stable_numbers = value == "true",
            "font_capitals" => settings.font_capitals = value == "true",
            "shadow_half" => {
                for (feld, v) in settings.shadow_half.iter_mut().zip(value.split(',')) {
                    *feld = v.trim() == "1";
                }
            }
            "topbar_uptime" => settings.topbar_uptime = value == "true",
            "text_shadow_style" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.text_shadow_style = v.min(3);
                }
            }
            "shadow_thickness" | "shadow_opacity" => {
                let (feld, (min, max)) = if key == "shadow_thickness" {
                    (&mut settings.shadow_thickness, SHADOW_THICKNESS_RANGE)
                } else {
                    (&mut settings.shadow_opacity, (0, 100))
                };
                for (ziel, v) in feld.iter_mut().zip(value.split(',')) {
                    if let Ok(v) = v.trim().parse::<u8>() {
                        *ziel = v.clamp(min, max);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    // Schluessel von "cave_culling" bis "teams_own_color". true = erkannt.
    fn parse_keys_render_modules(settings: &mut Self, key: &str, value: &str) -> bool {
        match key {
            "cave_culling" => settings.cave_culling = value == "true",
            "mipmapping" => settings.mipmapping = value == "true",
            "render_path" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.render_path = v.min(RENDER_PATH_NAMES.len() as u8 - 1);
                }
            }
            "particles_mode" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.particles_mode = if (v as usize) < PARTICLE_MODE_NAMES.len() { v } else { 0 };
                    settings.particles_mode_own = settings.particles_mode;
                }
            }
            "particles_mode_own" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.particles_mode_own = if (v as usize) < PARTICLE_MODE_NAMES.len() { v } else { 0 };
                }
            }
            "cmaa2_enabled" => settings.cmaa2_enabled = value == "true",
            "audio_output_device" => settings.audio_output_device = value.to_string(),
            "audio_buffer_frames" => {
                if let Ok(v) = value.parse::<u32>() {
                    if AUDIO_BUFFER_CHOICES.contains(&v) {
                        settings.audio_buffer_frames = v;
                    }
                }
            }
            "ui_pitch_semitones" => {
                if let Ok(v) = value.parse::<i32>() {
                    settings.ui_pitch_semitones = v;
                }
            }
            "module_totem_indicator" => settings.module_totem_indicator = value == "true",
            "module_totem_sound" => settings.module_totem_sound = value == "true",
            "module_ghost_tolerance" => settings.module_ghost_tolerance = value == "true",
            "module_crystal_optimizer" => settings.module_crystal_optimizer = value == "true",
            "module_consumable_optimizer" => settings.module_consumable_optimizer = value == "true",
            "eat_slot_lock" => settings.eat_slot_lock = value == "true",
            "eat_aggressive_prediction" => settings.eat_aggressive_prediction = value == "true",
            "eat_hold_through_ms" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.eat_hold_through_ms = v;
                }
            }
            "module_use_timer" => settings.module_use_timer = value == "true",
            "module_no_tool_break" => settings.module_no_tool_break = value == "true",
            "module_dynamic_fire" => settings.module_dynamic_fire = value == "true",
            "module_zoom" => settings.module_zoom = value == "true",
            "module_voice_chat" => settings.module_voice_chat = value == "true",

            "sens_calc_mc_percent" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.sens_calc_mc_percent = v;
                }
            }
            "sens_calc_dpi" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.sens_calc_dpi = v;
                }
            }
            "zoom_factor_tenths" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.zoom_factor_tenths = v;
                }
            }
            "zoom_sens_percent" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.zoom_sens_percent = v;
                }
            }
            "module_botcheck_pass" => settings.module_botcheck_pass = value == "true",
            "no_tool_break_reserve" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.no_tool_break_reserve = v;
                }
            }
            "glint_speed" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.glint_speed = v;
                }
            }
            "glint_strength" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.glint_strength = v;
                }
            }
            "player_list_scale" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.player_list_scale = v;
                }
            }
            "scroll_inertia" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.scroll_inertia = v.min(100);
                }
            }
            "scroll_distance" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.scroll_distance = v.clamp(SCROLL_DISTANCE_MIN, SCROLL_DISTANCE_MAX);
                }
            }
            "menu_backdrop_opacity" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.menu_backdrop_opacity = v;
                }
            }
            "entity_render_distance" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.entity_render_distance = v;
                }
            }
            "block_entity_render_distance" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.block_entity_render_distance = v;
                }
            }
            "ghost_tolerance_ms" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.ghost_tolerance_ms = v;
                }
            }
            "module_hitbox_enabled" => settings.module_hitbox_enabled = value == "true",
            "module_hitbox_show_smoothed" => settings.module_hitbox_show_smoothed = value == "true",
            "module_hitbox_show_raw" => settings.module_hitbox_show_raw = value == "true",
            "module_hitbox_dotted" => settings.module_hitbox_dotted = value == "true",
            "module_hitbox_items" => settings.module_hitbox_items = value == "true",
            "module_hitbox_self" => settings.module_hitbox_self = value == "true",
            "module_teams" => settings.module_teams = value == "true",
            "teams_match" => settings.team_match = teams::Matchers::decode(value),
            "teams_own_color" => settings.team_own_color = u32::from_str_radix(value.trim_start_matches('#'), 16).ok().map(|c| c & 0xFF_FFFF),
            _ => return false,
        }
        true
    }

    // Schluessel von "module_hitbox_density_px" bis "hud_arrows_y". true = erkannt.
    fn parse_keys_hitbox_armor(settings: &mut Self, key: &str, value: &str) -> bool {
        match key {
            "module_hitbox_density_px" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_density_px = v;
                }
            }
            "module_hitbox_opacity" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_opacity = v;
                }
            }
            "module_hitbox_smoothed_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_smoothed_hue = v.clamp(HSV_HUE_MIN, HSV_HUE_MAX);
                }
            }
            "module_hitbox_smoothed_saturation" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_smoothed_saturation = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "module_hitbox_smoothed_value" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_smoothed_value = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "module_hitbox_raw_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_raw_hue = v.clamp(HSV_HUE_MIN, HSV_HUE_MAX);
                }
            }
            "module_hitbox_raw_saturation" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_raw_saturation = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "module_hitbox_raw_value" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_hitbox_raw_value = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "damage_tint_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.damage_tint_hue = v.clamp(HSV_HUE_MIN, HSV_HUE_MAX);
                }
            }
            "damage_tint_saturation" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.damage_tint_saturation = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "damage_tint_value" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.damage_tint_value = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "damage_tint_alpha" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.damage_tint_alpha = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "damage_tint_skin_only" => settings.damage_tint_skin_only = value == "true",
            "module_shields" => settings.module_shields = value == "true",
            "module_arrows" => settings.module_arrows = value == "true",
            "module_armor_hud" => settings.module_armor_hud = value == "true",
            "hud_armor_x" | "hud_armor_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    let v = v.clamp(0.0, 1.0);
                    if key == "hud_armor_x" {
                        settings.hud_armor_x = v;
                    } else {
                        settings.hud_armor_y = v;
                    }
                }
            }
            "armor_hud_layout" => settings.armor_hud_layout = value.parse::<u8>().unwrap_or(0).min(armor_hud::LAYOUT_FLAT_HELMET_LEFT),
            "armor_hud_gap" => {
                if let Ok(v) = value.parse::<i32>() {
                    settings.armor_hud_gap = v;
                }
            }
            "armor_hud_number_scale" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.armor_hud_number_scale = v;
                }
            }
            "armor_hud_number_x" | "armor_hud_number_y" => {
                if let Ok(v) = value.parse::<i32>() {
                    if key == "armor_hud_number_x" {
                        settings.armor_hud_number_x = v;
                    } else {
                        settings.armor_hud_number_y = v;
                    }
                }
            }
            "armor_hud_sort_vertical" => settings.armor_hud_sort_vertical = value.parse::<u8>().unwrap_or(0).min(1),
            "armor_hud_sort_flat" => settings.armor_hud_sort_flat = value.parse::<u8>().unwrap_or(0).min(1),
            "armor_hud_green" => settings.armor_hud_green = value == "true",
            "armor_hud_ring" => settings.armor_hud_ring = value == "true",
            "module_keystrokes" => settings.module_keystrokes = value == "true",
            "hurt_cam" => settings.hurt_cam = value == "true",
            "shoulder_cam" => settings.shoulder_cam = value == "true",
            "shoulder_offset_cm" => {
                if let Ok(v) = value.parse::<i32>() {
                    settings.shoulder_offset_cm = v.clamp(SHOULDER_OFFSET_MIN, SHOULDER_OFFSET_MAX);
                }
            }
            "shoulder_crosshair" => settings.shoulder_crosshair = value == "true",
            "shoulder_fade" => settings.shoulder_fade = value == "true",
            "shoulder_decoupled" => settings.shoulder_decoupled = value == "true",
            "module_freelook" => settings.module_freelook = value == "true",
            "freelook_view" => settings.freelook_view = value.parse::<u8>().unwrap_or(FREELOOK_VIEW_BACK).min(FREELOOK_VIEW_FRONT),
            "armor_trims" => settings.armor_trims = value == "true",
            "modified_view_model" => settings.modified_view_model = value == "true",
            "fire_mode" => settings.fire_mode = value.parse::<u8>().unwrap_or(0).min(FIRE_MODE_OFF),
            "hurt_cam_strength" | "fire_height" | "fire_crosshair_saturation" | "fire_crosshair_value" | "fire_crosshair_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    match key {
                        "hurt_cam_strength" => settings.hurt_cam_strength = v,
                        "fire_height" => settings.fire_height = v,
                        "fire_crosshair_saturation" => settings.fire_crosshair_saturation = v.min(100),
                        "fire_crosshair_value" => settings.fire_crosshair_value = v.min(100),
                        _ => settings.fire_crosshair_hue = v % 360,
                    }
                }
            }
            "hud_keys_x" | "hud_keys_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    let v = v.clamp(0.0, 1.0);
                    if key == "hud_keys_x" {
                        settings.hud_keys_x = v;
                    } else {
                        settings.hud_keys_y = v;
                    }
                }
            }
            "keys_opacity" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.keys_opacity = v;
                }
            }
            "module_session_info" => settings.module_session_info = value == "true",
            "death_animation" => settings.death_animation = value.parse::<u8>().ok().filter(|&v| (v as usize) < DEATH_ANIM_NAMES.len()).unwrap_or(DEATH_ANIM_VANILLA),
            "hud_session_x" | "hud_session_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    let v = v.clamp(0.0, 1.0);
                    if key == "hud_session_x" {
                        settings.hud_session_x = v;
                    } else {
                        settings.hud_session_y = v;
                    }
                }
            }
            "session_opacity" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.session_opacity = v;
                }
            }
            "session_lines" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.session_lines = v & 0x0F;
                }
            }
            // Das fruehere Trails-Modul lebt jetzt als Schalter im Pfeil-Modul.
            "module_trails" | "arrow_trail" => settings.arrow_trail = value == "true",
            "arrow_indicator" => settings.arrow_indicator = value == "true",
            "arrow_number_only" => settings.arrow_number_only = value == "true",
            "arrow_white" => settings.arrow_white = value == "true",
            "arrow_opacity" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.arrow_opacity = v;
                }
            }
            "hud_arrows_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_arrows_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_arrows_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_arrows_y = v.clamp(0.0, 1.0);
                }
            }
            _ => return false,
        }
        true
    }

    // Schluessel von "hud_shield_cd_x" bis "module_chunk_saver". true = erkannt.
    fn parse_keys_modules(settings: &mut Self, key: &str, value: &str) -> bool {
        match key {
            "hud_shield_cd_x" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_shield_cd_x = v.clamp(0.0, 1.0);
                }
            }
            "hud_shield_cd_y" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.hud_shield_cd_y = v.clamp(0.0, 1.0);
                }
            }
            // Vorgaenger-Schalter "Ruestung faerben": AN hiess "nicht nur der Skin".
            "damage_tint_armor" => settings.damage_tint_skin_only = value != "true",
            "damage_tint_ticks" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.damage_tint_ticks = v;
                }
            }
            "module_crit_color_enabled" => settings.module_crit_color_enabled = value == "true",
            "module_crit_color_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_crit_color_hue = v.clamp(HSV_HUE_MIN, HSV_HUE_MAX);
                }
            }
            "module_crit_color_saturation" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_crit_color_saturation = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "module_crit_color_value" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_crit_color_value = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "module_crit_color_alpha" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_crit_color_alpha = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "module_block_outline" => settings.module_block_outline = value == "true",
            "module_saturation_hud" => settings.module_saturation_hud = value == "true",
            "module_ego_name" => settings.module_ego_name = value == "true",
            "ego_alias" => settings.ego_alias = value.trim().chars().take(EGO_ALIAS_MAX).collect(),
            "ego_deco" => settings.ego_deco = ego_name::Deco::parse(value).unwrap_or(settings.ego_deco),
            "stack_count_colors" => {
                for (i, c) in value.split(',').take(2).enumerate() {
                    let teile: Vec<u32> = c.split('/').filter_map(|v| v.trim().parse().ok()).collect();
                    if let [h, s, v] = teile[..] {
                        settings.stack_count_colors[i] = [h % 360, s.min(100), v.min(100)];
                    }
                }
            }
            "ego_colors" => {
                for (i, c) in value.split(',').take(4).enumerate() {
                    let teile: Vec<u32> = c.split('/').filter_map(|v| v.trim().parse().ok()).collect();
                    if let [h, s, v, a] = teile[..] {
                        settings.ego_colors[i] = [h % 360, s.min(100), v.min(100), a.min(100)];
                    }
                }
            }
            // Frueher ein Schalter: "true" ist der einfarbige Schatten.
            "ego_shadow" => {
                settings.ego_shadow = match value {
                    "true" => ego_name::Deco::Solid,
                    "false" => ego_name::Deco::Off,
                    v => ego_name::Deco::parse(v).unwrap_or(settings.ego_shadow),
                }
            }
            "ego_light" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.ego_light = v;
                }
            }
            "ego_shadow_light" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.ego_shadow_light = v;
                }
            }
            "ego_speed" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.ego_speed = v;
                }
            }
            "ego_sound" => settings.ego_sound = value.trim().to_string(),
            "ego_volume" => {
                if let Some(v) = parse_volume(value) {
                    settings.ego_volume = v;
                }
            }
            "saturation_hud_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.saturation_hud_hue = v % 360;
                }
            }
            "saturation_hud_bg_alpha" | "saturation_hud_grid_alpha" => {
                if let Ok(v) = value.parse::<u32>() {
                    let feld = if key == "saturation_hud_bg_alpha" { &mut settings.saturation_hud_bg_alpha } else { &mut settings.saturation_hud_grid_alpha };
                    *feld = v.min(100);
                }
            }
            "saturation_hud_height" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.saturation_hud_height = v;
                }
            }
            "saturation_hud_saturation" | "saturation_hud_value" => {
                if let Ok(v) = value.parse::<u32>() {
                    let feld = if key == "saturation_hud_value" { &mut settings.saturation_hud_value } else { &mut settings.saturation_hud_saturation };
                    *feld = v.min(100);
                }
            }
            "arrow_empty_feedback" => {
                if let Ok(v) = value.parse::<u8>() {
                    settings.arrow_empty_feedback = v.min(ARROW_EMPTY_FEEDBACK_NAMES.len() as u8 - 1);
                }
            }
            "module_show_markers" => settings.module_show_markers = value == "true",
            "module_sound_debug" => settings.module_sound_debug = value == "true",
            "module_hand_debug" => settings.module_hand_debug = value == "true",
            "hand_fist_items" => {
                // Aeltere Eintraege haben weniger Werte (ohne Verdrehen): mit 0 auffuellen.
                let roh: std::collections::BTreeMap<String, Vec<i32>> = serde_json::from_str(value).unwrap_or_default();
                settings.hand_fist_items = roh.into_iter().map(|(k, v)| (k, std::array::from_fn(|i| v.get(i).copied().unwrap_or(0)))).collect();
            }
            "module_connection_debug" => settings.module_connection_debug = value == "true",
            "module_delay" => settings.module_delay = value == "true",
            "lag_delay_ms" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.lag_delay_ms = v;
                }
            }
            "module_discord_status" => settings.module_discord_status = value == "true",
            "module_input_log_enabled" => settings.module_input_log_enabled = value == "true",
            "module_coral_record" => settings.module_coral_record = value == "true",
            "module_autoclicker" => settings.module_autoclicker = value == "true",
            "autoclicker_cps" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.autoclicker_cps = v;
                }
            }
            "module_color" => settings.module_color = value == "true",
            "module_sustainability" => settings.module_sustainability = value == "true",
            "sustain_background" => settings.sustain_background = value == "true",
            "sustain_pause" => settings.sustain_pause = value == "true",
            // Gespeichert wird, was AUS ist -- so sind spaeter hinzugekommene Animationen
            // bei bestehenden Configs gleich an (bzw. aus, wenn sie DEFAULT_OFF sind).
            "anim_off" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.anim_flags = world::more_anims::ALL & !v;
                }
            }
            "anims_enabled" => settings.anims_enabled = value == "true",
            // Alte Fassung (nur die ersten zwoelf Animationen, gespeichert war, was AN ist).
            "anim_flags" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.anim_flags = (v & 0xFFF) | (world::more_anims::DEFAULT & !0xFFF);
                }
            }
            "module_chunk_saver" => settings.module_chunk_saver = value == "true",
            _ => return false,
        }
        true
    }

    // Schluessel von "color_saturation" bis "settings_category". true = erkannt.
    fn parse_keys_sky_skins(settings: &mut Self, key: &str, value: &str) -> bool {
        match key {
            "color_saturation" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.color_saturation = v;
                }
            }
            "discord_client_id" => {
                let id: String = value.chars().filter(char::is_ascii_digit).collect();
                // Die eingefuegte Vorgabe-ID gilt als Vorgabe (s. DiscordPasteClientId).
                if id != discord_rpc::DEFAULT_CLIENT_ID {
                    settings.discord_client_id = id;
                }
            }
            "block_outline_hue" | "block_outline_saturation" | "block_outline_value" | "block_outline_alpha" => {
                if let Ok(v) = value.parse::<u32>() {
                    let axis = match key {
                        "block_outline_hue" => DamageTintAxis::Hue,
                        "block_outline_saturation" => DamageTintAxis::Saturation,
                        "block_outline_value" => DamageTintAxis::Value,
                        _ => DamageTintAxis::Alpha,
                    };
                    let (min, max, _) = axis.range();
                    *settings.block_outline_mut(axis) = v.clamp(min, max);
                }
            }
            "block_outline_width_px" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.block_outline_width_px = v;
                }
            }
            "block_outline_hide_mining" => settings.block_outline_hide_mining = value == "true",
            "block_outline_complementary" => settings.block_outline_complementary = value == "true",
            "module_sky_color_enabled" => settings.module_sky_color_enabled = value == "true",
            // Alte Fassung: feste Farbe an = Einfarbig, aus = Pack-Himmel bzw. Tageszeit.
            "module_sky_fixed_color" => settings.sky_mode = if value == "true" { SkyMode::Solid } else { SkyMode::Skin },
            "sky_mode" => {
                if let Some(m) = SkyMode::from_key(value) {
                    settings.sky_mode = m;
                }
            }
            "sky_sun" => settings.sky_sun = value == "true",
            "sky_moon" => settings.sky_moon = value == "true",
            "sky_stars" => settings.sky_stars = value == "true",
            "module_sky_fixed_time" => settings.module_sky_fixed_time = value == "true",
            "module_weather" => settings.module_weather = value == "true",
            "weather_mode" => {
                if let Some(m) = WeatherMode::from_key(value) {
                    settings.weather_mode = m;
                }
            }
            "weather_radius" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.weather_radius = v;
                }
            }
            "weather_radius_inner" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.weather_radius_inner = v;
                }
            }
            "module_sky_time" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_sky_time = v % SKY_TIME_DAY;
                }
            }
            "module_sky_color_hue" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_sky_color_hue = v.clamp(HSV_HUE_MIN, HSV_HUE_MAX);
                }
            }
            "module_sky_color_saturation" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_sky_color_saturation = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "module_sky_color_value" => {
                if let Ok(v) = value.parse::<u32>() {
                    settings.module_sky_color_value = v.clamp(HSV_SAT_VAL_MIN, HSV_SAT_VAL_MAX);
                }
            }
            "active_skin" => {
                settings.active_skins = parse_skin_stack(value);
                skin_tab::normalize_stack(&mut settings.active_skins, None);
            }
            "skin_order" => settings.skin_order = parse_skin_stack(value),
            "skin_exclusions" => settings.skin_exclusions = serde_json::from_str(value).unwrap_or_default(),
            "server_pack_positions" => settings.server_pack_positions = serde_json::from_str(value).unwrap_or_default(),
            "skin_auto_unzip" => settings.skin_auto_unzip = value == "true",
            "latin_macrons" => settings.latin_macrons = value == "true",
            "english_loanwords" => settings.english_loanwords = value == "true",
            "settings_category" => settings.settings_category = value.to_string(),
            _ => return false,
        }
        true
    }
}

// Eintrag particle_tuning(_own): je Art, die darin steht, ihre Werte nach `ziel`.
fn read_particle_tuning(ziel: &mut [particles::ParticleTuning; particles::KINDS.len()], value: &str) {
    let mut gelesen: std::collections::HashMap<&str, particles::ParticleTuning> = std::collections::HashMap::new();
    for eintrag in value.split(';') {
        let Some((key, werte)) = eintrag.split_once(':') else { continue };
        let w: Vec<u32> = werte.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let klemmen = |v: u32, stufen: &[u32]| v.clamp(stufen[0], *stufen.last().unwrap());
        // Vier Werte aus der Zeit vor Menge/Intervall, sonst sieben.
        let (an, dauer, deckkraft, tempo, menge, intervall, auf_einmal) = match w[..] {
            [an, dauer, deckkraft, tempo] => (an, dauer, deckkraft, tempo, 100, 100, 0),
            [an, dauer, deckkraft, tempo, menge, intervall, auf_einmal] => (an, dauer, deckkraft, tempo, menge, intervall, auf_einmal),
            _ => continue,
        };
        gelesen.insert(
            key.trim(),
            particles::ParticleTuning {
                enabled: an != 0,
                lifetime_pct: klemmen(dauer, &PARTICLE_LIFETIME_STEPS),
                opacity_pct: deckkraft.clamp(PARTICLE_OPACITY_STEP, 100),
                speed_pct: klemmen(tempo, &PARTICLE_SPEED_STEPS),
                amount_pct: klemmen(menge, &PARTICLE_AMOUNT_STEPS),
                interval_pct: klemmen(intervall, &PARTICLE_INTERVAL_STEPS),
                all_at_once: auf_einmal != 0,
            },
        );
    }
    for (i, k) in particles::KINDS.iter().enumerate() {
        // Die Trank-Spritzer gehoerten frueher zu "potion" und erben dessen Werte,
        // bis sie eigene haben.
        let wert = gelesen.get(k.key).or_else(|| (k.key == "potion_splash").then(|| gelesen.get("potion")).flatten());
        if let Some(t) = wert {
            ziel[i] = *t;
        }
    }
}
