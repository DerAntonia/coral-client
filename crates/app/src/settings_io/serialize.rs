// Settings als Text schreiben und speichern.

use super::*;

impl Settings {
    pub(crate) fn save(&self) -> std::io::Result<std::path::PathBuf> {
        let path = settings_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "kein Konfigurationsverzeichnis"))?;
        let privat_pfad = private_settings_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "kein Konfigurationsverzeichnis"))?;
        let (oeffentlich, privat) = split_private_settings(&self.serialize());
        for (pfad, inhalt) in [(&privat_pfad, privat), (&path, oeffentlich)] {
            if let Some(dir) = pfad.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(pfad, inhalt)?;
        }
        Ok(path)
    }

    // Der gesamte Dateiinhalt als Text. Getrennt vom Schreiben, damit sich der RUNDLAUF
    // (serialize -> parse) ohne Dateisystem testen laesst -- ein im Speicher-Format
    // vergessenes Feld faellt sonst erst nach einem Neustart auf.
    pub(crate) fn serialize(&self) -> String {
        let kb = &self.keybinds;
        let km = &self.keybind_modes;
        let mut body = String::new();
        kv(&mut body, "show_tearing_test", self.show_tearing_test);
        kv(&mut body, "show_hud", self.show_hud);
        kv(&mut body, "fps_limit_hz", self.fps_limit);
        kv(&mut body, "mouse_sensitivity", self.mouse_sensitivity);
        kv(&mut body, "cursor_sensitivity", self.cursor_sensitivity);
        kv(&mut body, "cursor_size", self.cursor_size);
        kv(&mut body, "cursor_circle", self.cursor_circle);
        kv(&mut body, "mesh_columns_per_frame", self.mesh_columns_per_frame);
        kv(&mut body, "frame_latency", self.frame_latency);
        kv(&mut body, "max_render_distance", self.max_render_distance);
        kv(&mut body, "chunk_update_threads", self.chunk_update_threads);
        kv(&mut body, "hud_perf_x", self.hud_perf_x);
        kv(&mut body, "hud_perf_y", self.hud_perf_y);
        kv(&mut body, "hud_actionbar_x", self.hud_actionbar_x);
        kv(&mut body, "hud_actionbar_y", self.hud_actionbar_y);
        kv(&mut body, "hud_scoreboard_x", self.hud_scoreboard_x);
        kv(&mut body, "hud_scoreboard_y", self.hud_scoreboard_y);
        kv(&mut body, "hud_hotbar_x", self.hud_hotbar_x);
        kv(&mut body, "hud_hotbar_y", self.hud_hotbar_y);
        kv(&mut body, "hud_hotbar_scale", self.hud_hotbar_scale);
        kv(&mut body, "hud_bossbar_x", self.hud_bossbar_x);
        kv(&mut body, "hud_bossbar_y", self.hud_bossbar_y);
        kv(&mut body, "hud_bossbar_scale", self.hud_bossbar_scale);
        kv(&mut body, "hud_chat_x", self.hud_chat_x);
        kv(&mut body, "hud_chat_y", self.hud_chat_y);
        kv(&mut body, "chat_scale_percent", self.chat_scale_percent);
        kv(&mut body, "chat_width", self.chat_width);
        kv(&mut body, "chat_lines", self.chat_lines);
        kv(&mut body, "chat_lines_open", self.chat_lines_open);
        kv(&mut body, "chat_background", self.chat_background);
        kv(&mut body, "chat_open_dim", self.chat_open_dim);
        kv(&mut body, "chat_fade_secs", self.chat_fade_secs);
        kv(&mut body, "master_volume", self.master_volume);
        kv(&mut body, "inventory_transparency", self.inventory_transparency);
        kv(&mut body, "inventory_cursor_to_slot", self.inventory_cursor_to_slot);
        kv(&mut body, "inventory_scale", self.inventory_scale);
        kv(&mut body, "crosshair_color_mode", self.crosshair_color_mode);
        kv(&mut body, "crosshair_hue", self.crosshair_hue);
        kv(&mut body, "crosshair_saturation", self.crosshair_saturation);
        kv(&mut body, "crosshair_value", self.crosshair_value);
        kv(&mut body, "crosshair_brightness_boost", self.crosshair_brightness_boost);
        kv(&mut body, "crosshair_outline", self.crosshair_outline);
        kv(&mut body, "crosshair_ring_outline", self.crosshair_ring_outline);
        kv(&mut body, "fov", self.fov);
        kv(&mut body, "fov_third_back", self.fov_third_back);
        kv(&mut body, "fov_third_front", self.fov_third_front);
        kv(&mut body, "camera_distance_back", self.camera_distance_back);
        kv(&mut body, "camera_distance_front", self.camera_distance_front);
        kv(&mut body, "camera_y_offset_back", self.camera_y_offset_back);
        kv(&mut body, "camera_y_offset_front", self.camera_y_offset_front);
        kv(&mut body, "projection_mode", self.projection_mode);
        kv(&mut body, "particles_mode", self.particles_mode);
        kv(&mut body, "font", self.font);
        kv(&mut body, "text_shadow", self.text_shadow);
        kv(&mut body, "wallpaper", self.wallpaper);
        kv(&mut body, "cmaa2_enabled", self.cmaa2_enabled);
        kv(&mut body, "audio_output_device", &self.audio_output_device);
        kv(&mut body, "audio_buffer_frames", self.audio_buffer_frames);
        kv(&mut body, "ui_pitch_semitones", self.ui_pitch_semitones);
        kv(&mut body, "module_totem_indicator", self.module_totem_indicator);
        kv(&mut body, "module_totem_sound", self.module_totem_sound);
        kv(&mut body, "module_ghost_tolerance", self.module_ghost_tolerance);
        kv(&mut body, "ghost_tolerance_ms", self.ghost_tolerance_ms);
        kv(&mut body, "module_hitbox_enabled", self.module_hitbox_enabled);
        kv(&mut body, "module_hitbox_show_smoothed", self.module_hitbox_show_smoothed);
        kv(&mut body, "module_hitbox_show_raw", self.module_hitbox_show_raw);
        kv(&mut body, "module_hitbox_dotted", self.module_hitbox_dotted);
        kv(&mut body, "module_hitbox_items", self.module_hitbox_items);
        kv(&mut body, "module_hitbox_self", self.module_hitbox_self);
        kv(&mut body, "module_hitbox_density_px", self.module_hitbox_density_px);
        kv(&mut body, "module_hitbox_opacity", self.module_hitbox_opacity);
        kv(&mut body, "module_hitbox_smoothed_hue", self.module_hitbox_smoothed_hue);
        kv(&mut body, "module_hitbox_smoothed_saturation", self.module_hitbox_smoothed_saturation);
        kv(&mut body, "module_hitbox_smoothed_value", self.module_hitbox_smoothed_value);
        kv(&mut body, "module_hitbox_raw_hue", self.module_hitbox_raw_hue);
        kv(&mut body, "module_hitbox_raw_saturation", self.module_hitbox_raw_saturation);
        kv(&mut body, "module_hitbox_raw_value", self.module_hitbox_raw_value);
        kv(&mut body, "damage_tint_hue", self.damage_tint_hue);
        kv(&mut body, "damage_tint_saturation", self.damage_tint_saturation);
        kv(&mut body, "damage_tint_value", self.damage_tint_value);
        kv(&mut body, "damage_tint_alpha", self.damage_tint_alpha);
        kv(&mut body, "damage_tint_skin_only", self.damage_tint_skin_only);
        kv(&mut body, "damage_tint_ticks", self.damage_tint_ticks);
        kv(&mut body, "module_shields", self.module_shields);
        kv(&mut body, "shield_cd_self", self.shield_cd_self);
        kv(&mut body, "shield_cd_players", self.shield_cd_players);
        kv(&mut body, "shield_hold_players", self.shield_hold_players);
        kv(&mut body, "shield_cd_hue", self.shield_cd_hue);
        kv(&mut body, "shield_cd_saturation", self.shield_cd_saturation);
        kv(&mut body, "shield_cd_value", self.shield_cd_value);
        kv(&mut body, "shield_hold_hue", self.shield_hold_hue);
        kv(&mut body, "shield_hold_saturation", self.shield_hold_saturation);
        kv(&mut body, "shield_hold_value", self.shield_hold_value);
        kv(&mut body, "shield_hold_alpha", self.shield_hold_alpha);
        kv(&mut body, "hud_shield_cd_x", self.hud_shield_cd_x);
        kv(&mut body, "hud_shield_cd_y", self.hud_shield_cd_y);
        kv(&mut body, "module_arrows", self.module_arrows);
        kv(&mut body, "arrow_indicator", self.arrow_indicator);
        kv(&mut body, "arrow_number_only", self.arrow_number_only);
        kv(&mut body, "arrow_opacity", self.arrow_opacity);
        kv(&mut body, "hud_arrows_x", self.hud_arrows_x);
        kv(&mut body, "hud_arrows_y", self.hud_arrows_y);
        kv(&mut body, "arrow_trail", self.arrow_trail);
        kv(&mut body, "arrow_white", self.arrow_white);
        kv(&mut body, "trail_hold_tenths", self.trail_hold_tenths);
        kv(&mut body, "trail_fade_tenths", self.trail_fade_tenths);
        kv(&mut body, "module_crit_color_enabled", self.module_crit_color_enabled);
        kv(&mut body, "module_crit_color_hue", self.module_crit_color_hue);
        kv(&mut body, "module_crit_color_saturation", self.module_crit_color_saturation);
        kv(&mut body, "module_crit_color_value", self.module_crit_color_value);
        kv(&mut body, "module_crit_color_alpha", self.module_crit_color_alpha);
        kv(&mut body, "module_block_outline", self.module_block_outline);
        kv(&mut body, "block_outline_hue", self.block_outline_hue);
        kv(&mut body, "block_outline_saturation", self.block_outline_saturation);
        kv(&mut body, "block_outline_value", self.block_outline_value);
        kv(&mut body, "block_outline_alpha", self.block_outline_alpha);
        kv(&mut body, "block_outline_width_px", self.block_outline_width_px);
        kv(&mut body, "block_outline_hide_mining", self.block_outline_hide_mining);
        kv(&mut body, "block_outline_complementary", self.block_outline_complementary);
        kv(&mut body, "module_sky_color_enabled", self.module_sky_color_enabled);
        kv(&mut body, "module_sky_color_hue", self.module_sky_color_hue);
        kv(&mut body, "module_sky_color_saturation", self.module_sky_color_saturation);
        kv(&mut body, "module_sky_color_value", self.module_sky_color_value);
        kv(&mut body, "block_rng_rotation", self.block_rng_rotation);
        kv(&mut body, "smooth_lighting_percent", self.smooth_lighting_percent);
        kv(&mut body, "motion_blur_percent", self.motion_blur_percent);
        kv(&mut body, "jump_cooldown_enabled", self.jump_cooldown_enabled);
        kv(&mut body, "sneak_edge_protection_enabled", self.sneak_edge_protection_enabled);
        kv(&mut body, "snap_tap_enabled", self.snap_tap_enabled);
        kv(&mut body, "hold_use_repeat", self.hold_use_repeat.key());
        kv(&mut body, "hold_use_repeat_rockets", self.hold_use_repeat_rockets);
        kv(&mut body, "no_extinguish_dc_ghost", self.no_extinguish_dc_ghost);
        kv(&mut body, "use_while_mining", self.use_while_mining);
        kv(&mut body, "active_skin", self.active_skins.join(SKIN_STACK_SEP));
        kv(&mut body, "lang_ui", self.lang_ui);
        kv(&mut body, "lang_items", self.lang_items);
        kv(&mut body, "lang_effects", self.lang_effects);
        kv(&mut body, "lang_chat", self.lang_chat);
        kv(&mut body, "latin_macrons", self.latin_macrons);
        kv(&mut body, "msa_refresh_token", &self.msa_refresh_token);
        kv(&mut body, "account_name", &self.account_name);
        kv(&mut body, "mc_access_token", &self.mc_access_token);
        kv(&mut body, "account_uuid", &self.account_uuid);
        // Tastenbelegungen angehaengt statt in der Liste oben -- eine Zeile je Aktion, aus
        // KeyBindAction::ALL erzeugt. Der Halten/Umschalten-Modus nur dort, wo er ueberhaupt
        // eine Bedeutung hat (Einmal-Aktionen haben keinen Dauerzustand).
        body.push_str(&format!("english_loanwords={}\n", self.english_loanwords));
        body.push_str(&format!("settings_category={}\n", self.settings_category));
        body.push_str(&format!("skin_order={}\nskin_auto_unzip={}\n", self.skin_order.join(SKIN_STACK_SEP), self.skin_auto_unzip));
        if !self.skin_exclusions.is_empty() {
            body.push_str(&format!("skin_exclusions={}\n", serde_json::to_string(&self.skin_exclusions).unwrap_or_default()));
        }
        if !self.server_pack_positions.is_empty() {
            body.push_str(&format!("server_pack_positions={}\n", serde_json::to_string(&self.server_pack_positions).unwrap_or_default()));
        }
        for (k, v) in &self.settings_scrolls {
            body.push_str(&format!("scroll_{k}={v}\n"));
        }
        match self.theme_choice {
            ThemeChoice::Preset(i) => body.push_str(&format!("theme_choice=preset:{}\n", theme::PRESETS.get(i).map_or("Standard", |p| p.name))),
            ThemeChoice::Custom(i) => body.push_str(&format!("theme_choice=custom:{i}\n")),
        }
        for t in &self.custom_themes {
            body.push_str(&format!("custom_theme={}\n", serialize_custom_theme(t)));
        }
        for slot in theme::Slot::ALL {
            body.push_str(&format!("theme_{}={:06X}\n", slot.key(), self.theme[slot.index()]));
        }
        for (c, v) in VOLUME_CATEGORIES.iter().zip(self.category_volume) {
            body.push_str(&format!("volume_{}={v}\n", c.key));
        }
        for (s, v) in ui_sound::UiSound::ALL.iter().zip(self.ui_sound_volume) {
            body.push_str(&format!("ui_volume_{}={v}\n", s.key()));
        }
        body.push_str(&self.toteming.serialize());
        body.push_str(&format!("module_crystal_optimizer={}\n", self.module_crystal_optimizer));
        body.push_str(&format!("module_consumable_optimizer={}\nmodule_use_timer={}\n", self.module_consumable_optimizer, self.module_use_timer));
        body.push_str(&format!("eat_hold_through_ms={}\n", self.eat_hold_through_ms));
        body.push_str(&format!("eat_slot_lock={}\n", self.eat_slot_lock));
        body.push_str(&format!("eat_aggressive_prediction={}\n", self.eat_aggressive_prediction));
        body.push_str(&format!("crosshair_custom={}\ncrosshair_pixels={}\n", self.crosshair_custom, self.crosshair_pattern.encode()));
        let groessen: Vec<String> = self.crosshair_sizes.iter().map(std::string::ToString::to_string).collect();
        body.push_str(&format!("crosshair_shape={}\ncrosshair_sizes={}\n", self.crosshair_shape, groessen.join(",")));
        body.push_str(&format!("module_no_tool_break={}\nno_tool_break_reserve={}\n", self.module_no_tool_break, self.no_tool_break_reserve));
        body.push_str(&format!("module_botcheck_pass={}\n", self.module_botcheck_pass));
        for art in hitbox_kinds::HitboxKind::ALL {
            body.push_str(&format!("hitbox_{}={}\n", art.key(), self.module_hitbox_styles[art.index()].encode()));
        }
        body.push_str(&format!(
            "module_teams={}\nteams_match={}\nteams_own_color={}\n",
            self.module_teams,
            self.team_match.encode(),
            self.team_own_color.map_or("auto".to_string(), |c| format!("{c:06X}"))
        ));
        for seite in teams::Side::ALL {
            body.push_str(&format!("{}={}\n", seite.key(), self.team_hitboxes[seite.index()].encode()));
        }
        body.push_str(&format!("module_dynamic_fire={}\n", self.module_dynamic_fire));
        body.push_str(&format!(
            "shoulder_cam={}\nshoulder_offset_cm={}\nshoulder_crosshair={}\nshoulder_fade={}\nshoulder_decoupled={}\nmodule_freelook={}\nfreelook_view={}\n",
            self.shoulder_cam, self.shoulder_offset_cm, self.shoulder_crosshair, self.shoulder_fade, self.shoulder_decoupled, self.module_freelook, self.freelook_view
        ));
        body.push_str(&format!(
            "module_zoom={}\nzoom_factor_tenths={}\nzoom_sens_percent={}\n",
            self.module_zoom, self.zoom_factor_tenths, self.zoom_sens_percent
        ));
        body.push_str(&format!("module_voice_chat={}\n", self.module_voice_chat));
        body.push_str(&self.voice.serialize());
        body.push_str(&format!("hotbar_durability_percent={}\n", self.hotbar_durability_percent));
        body.push_str(&format!("shader_scattering_percent={}\n", self.shader_scattering_percent));
        body.push_str(&format!("shader_sun_size_tenths={}\n", self.shader_sun_size_tenths));
        body.push_str(&format!(
            "sky_mode={}\nsky_sun={}\nsky_moon={}\nsky_stars={}\nmodule_sky_fixed_time={}\nmodule_sky_time={}\n",
            self.sky_mode.key(),
            self.sky_sun,
            self.sky_moon,
            self.sky_stars,
            self.module_sky_fixed_time,
            self.module_sky_time
        ));
        body.push_str(&format!(
            "module_weather={}\nweather_mode={}\nweather_radius_inner={}\nweather_radius={}\n",
            self.module_weather,
            self.weather_mode.key(),
            self.weather_radius_inner,
            self.weather_radius
        ));
        body.push_str(&format!("shader_block_shadows={}\n", self.shader_block_shadows));
        body.push_str(&format!("shader_lightmap_debug={}\n", self.shader_lightmap_debug));
        body.push_str(&format!("shader_block_softness_deg={}\n", self.shader_block_softness_deg));
        let faktoren: Vec<String> = self.light_factors.iter().map(|(n, p)| format!("{n}:{p}")).collect();
        body.push_str(&format!("light_factors={}\n", faktoren.join(";")));
        body.push_str(&format!(
            "hotbar_durability_absolute={}\nhotbar_durability_points={}\nhotbar_air_bar={}\n",
            self.hotbar_durability_absolute, self.hotbar_durability_points, self.hotbar_air_bar
        ));
        body.push_str(&format!("spectator_speed={}\n", self.spectator_speed));
        body.push_str(&format!("mouse_sens_split={}\nmouse_sensitivity_y={}\n", self.mouse_sens_split, self.mouse_sensitivity_y));
        body.push_str(&format!("cursor_width={}\n", self.cursor_width));
        body.push_str(&format!("hold_confirm_ms={}\nreconnect_hold_ms={}\n", self.hold_confirm_ms, self.reconnect_hold_ms));
        body.push_str(&format!("text_shadow_style={}\n", self.text_shadow_style));
        body.push_str(&format!("stable_numbers={}\nfont_capitals={}\n", self.stable_numbers, self.font_capitals));
        let halb: Vec<&str> = self.shadow_half.iter().map(|&h| if h { "1" } else { "0" }).collect();
        body.push_str(&format!("shadow_half={}\n", halb.join(",")));
        body.push_str(&format!("topbar_uptime={}\n", self.topbar_uptime));
        // Nur, was von Vanilla abweicht.
        // Erst die anderen, dann die eigenen: beim Lesen uebernehmen die eigenen zunaechst die
        // anderen (s. parse), ihr Eintrag muss also danach kommen.
        let partikel = |tuning: &[particles::ParticleTuning]| -> String {
            particles::KINDS
                .iter()
                .zip(tuning)
                .filter(|(_, t)| **t != particles::ParticleTuning::default())
                .map(|(k, t)| {
                    format!(
                        "{}:{},{},{},{},{},{},{}",
                        k.key,
                        u8::from(t.enabled),
                        t.lifetime_pct,
                        t.opacity_pct,
                        t.speed_pct,
                        t.amount_pct,
                        t.interval_pct,
                        u8::from(t.all_at_once)
                    )
                })
                .collect::<Vec<_>>()
                .join(";")
        };
        body.push_str(&format!("particle_tuning={}\n", partikel(&self.particle_tuning)));
        body.push_str(&format!("particles_mode_own={}\n", self.particles_mode_own));
        body.push_str(&format!("particle_tuning_own={}\n", partikel(&self.particle_tuning_own)));
        body.push_str(&format!("cave_culling={}\n", self.cave_culling));
        body.push_str(&format!("mipmapping={}\n", self.mipmapping));
        body.push_str(&format!("render_path={}\n", self.render_path));
        let liste = |v: &[u8]| v.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
        body.push_str(&format!("shadow_thickness={}\nshadow_opacity={}\n", liste(&self.shadow_thickness), liste(&self.shadow_opacity)));
        for a in ShadowArea::ALL {
            if a.has_shadow() {
                body.push_str(&format!("{}={}\n", a.key(), self.area_shadow[a.index()]));
            }
            body.push_str(&format!("{}={}\n", a.font_key(), self.area_font[a.index()]));
        }
        body.push_str(&format!(
            "hud_title_x={}\nhud_title_y={}\nhud_title_align={}\nhud_actionbar_align={}\n",
            self.hud_title_x, self.hud_title_y, self.hud_title_align, self.hud_actionbar_align
        ));
        body.push_str(&format!(
            "hud_title_scale={}\nhud_subtitle_x={}\nhud_subtitle_y={}\nhud_subtitle_align={}\nhud_subtitle_scale={}\n\
             hud_effects_x={}\nhud_effects_y={}\nhud_effects_scale={}\nshow_effects_hud={}\nshow_effects_inventory={}\n",
            self.hud_title_scale,
            self.hud_subtitle_x,
            self.hud_subtitle_y,
            self.hud_subtitle_align,
            self.hud_subtitle_scale,
            self.hud_effects_x,
            self.hud_effects_y,
            self.hud_effects_scale,
            self.show_effects_hud,
            self.show_effects_inventory
        ));
        body.push_str(&format!("effects_hide_ambient={}\n", self.effects_hide_ambient));
        body.push_str(&format!(
            "effects_detailed={}\nhud_effects_detail_x={}\nhud_effects_detail_y={}\neffects_align={}\neffects_icon={}\neffects_name={}\n\
             effects_name_color={}\neffects_timer_style={}\neffects_timer_color={}\neffects_timer_blink={}\n",
            self.effects_detailed,
            self.hud_effects_detail_x,
            self.hud_effects_detail_y,
            self.effects_align,
            self.effects_icon,
            self.effects_name,
            self.effects_name_color,
            self.effects_timer_style,
            self.effects_timer_color,
            self.effects_timer_blink
        ));
        body.push_str(&format!("sens_calc_mc_percent={}\nsens_calc_dpi={}\n", self.sens_calc_mc_percent, self.sens_calc_dpi));
        body.push_str(&format!("hud_scoreboard_scale={}\nhud_arrows_scale={}\n", self.hud_scoreboard_scale, self.hud_arrows_scale));
        body.push_str(&format!(
            "module_armor_hud={}\nhud_armor_x={}\nhud_armor_y={}\nhud_armor_scale={}\narmor_hud_layout={}\narmor_hud_sort_vertical={}\narmor_hud_sort_flat={}\narmor_hud_gap={}\narmor_hud_number_scale={}\narmor_hud_number_x={}\narmor_hud_number_y={}\narmor_hud_green={}\narmor_hud_ring={}\n",
            self.module_armor_hud,
            self.hud_armor_x,
            self.hud_armor_y,
            self.hud_armor_scale,
            self.armor_hud_layout,
            self.armor_hud_sort_vertical,
            self.armor_hud_sort_flat,
            self.armor_hud_gap,
            self.armor_hud_number_scale,
            self.armor_hud_number_x,
            self.armor_hud_number_y,
            self.armor_hud_green,
            self.armor_hud_ring
        ));
        body.push_str(&format!(
            "module_keystrokes={}\nhud_keys_x={}\nhud_keys_y={}\nhud_keys_scale={}\nkeys_opacity={}\n",
            self.module_keystrokes, self.hud_keys_x, self.hud_keys_y, self.hud_keys_scale, self.keys_opacity
        ));
        body.push_str(&format!("death_animation={}\n", self.death_animation));
        body.push_str(&format!(
            "module_session_info={}\nhud_session_x={}\nhud_session_y={}\nhud_session_scale={}\nsession_opacity={}\nsession_lines={}\n",
            self.module_session_info, self.hud_session_x, self.hud_session_y, self.hud_session_scale, self.session_opacity, self.session_lines
        ));
        body.push_str(&format!(
            "armor_trims={}\nmodified_view_model={}\nhurt_cam={}\nhurt_cam_strength={}\nfire_mode={}\nfire_height={}\nfire_crosshair_hue={}\nfire_crosshair_saturation={}\nfire_crosshair_value={}\n",
            self.armor_trims,
            self.modified_view_model,
            self.hurt_cam,
            self.hurt_cam_strength,
            self.fire_mode,
            self.fire_height,
            self.fire_crosshair_hue,
            self.fire_crosshair_saturation,
            self.fire_crosshair_value
        ));
        body.push_str(&format!(
            "hud_held_name_x={}\nhud_held_name_y={}\nhud_held_name_scale={}\n",
            self.hud_held_name_x, self.hud_held_name_y, self.hud_held_name_scale
        ));
        body.push_str(&format!(
            "shield_cd_alpha={}\nshield_cd_mark={}\nshield_hold_mark={}\nshield_cd_self_mode={}\nshield_cd_text_style={}\narrow_text_style={}\n",
            self.shield_cd_alpha, self.shield_cd_mark, self.shield_hold_mark, self.shield_cd_self_mode, self.shield_cd_text_style, self.arrow_text_style
        ));
        body.push_str(&format!(
            "tear_position={}\ntear_timing_us={}\ntear_reserve_us={}\n",
            self.tear_position, self.tear_timing_us, self.tear_reserve_us
        ));
        body.push_str(&format!("glint_speed={}\nglint_strength={}\n", self.glint_speed, self.glint_strength));
        body.push_str(&format!("player_list_scale={}\n", self.player_list_scale));
        body.push_str(&format!("menu_backdrop_opacity={}\n", self.menu_backdrop_opacity));
        body.push_str(&format!("scroll_inertia={}\n", self.scroll_inertia));
        body.push_str(&format!("scroll_distance={}\n", self.scroll_distance));
        body.push_str(&format!("chat_line_spacing={}\n", self.chat_line_spacing));
        body.push_str(&format!("chat_bedrock={}\n", u8::from(self.chat_bedrock)));
        body.push_str(&format!("entity_render_distance={}\n", self.entity_render_distance));
        body.push_str(&format!("block_entity_render_distance={}\n", self.block_entity_render_distance));
        body.push_str(&format!(
            "arrow_empty_feedback={}\nmodule_show_markers={}\nmodule_sound_debug={}\nmodule_hand_debug={}\nhand_fist_items={}\nmodule_connection_debug={}\n",
            self.arrow_empty_feedback,
            self.module_show_markers,
            self.module_sound_debug,
            self.module_hand_debug,
            serde_json::to_string(&self.hand_fist_items).unwrap_or_default(),
            self.module_connection_debug
        ));
        body.push_str(&format!("module_delay={}\nlag_delay_ms={}\n", self.module_delay, self.lag_delay_ms));
        body.push_str(&format!("module_discord_status={}\ndiscord_client_id={}\n", self.module_discord_status, self.discord_client_id));
        body.push_str(&format!("module_input_log_enabled={}\n", self.module_input_log_enabled));
        body.push_str(&format!("module_coral_record={}\n", self.module_coral_record));
        body.push_str(&format!("module_autoclicker={}\nautoclicker_cps={}\n", self.module_autoclicker, self.autoclicker_cps));
        body.push_str(&format!(
            "module_saturation_hud={}\nsaturation_hud_hue={}\nsaturation_hud_saturation={}\nsaturation_hud_value={}\nsaturation_hud_height={}\nsaturation_hud_bg_alpha={}\nsaturation_hud_grid_alpha={}\n",
            self.module_saturation_hud,
            self.saturation_hud_hue,
            self.saturation_hud_saturation,
            self.saturation_hud_value,
            self.saturation_hud_height,
            self.saturation_hud_bg_alpha,
            self.saturation_hud_grid_alpha
        ));
        let zahl_farben: Vec<String> = self.stack_count_colors.iter().map(|c| format!("{}/{}/{}", c[0], c[1], c[2])).collect();
        body.push_str(&format!("stack_count_colors={}\n", zahl_farben.join(",")));
        let ego_farben: Vec<String> = self.ego_colors.iter().map(|c| format!("{}/{}/{}/{}", c[0], c[1], c[2], c[3])).collect();
        body.push_str(&format!(
            "module_ego_name={}\nego_alias={}\nego_deco={}\nego_colors={}\nego_shadow={}\nego_light={}\nego_shadow_light={}\nego_speed={}\nego_sound={}\nego_volume={}\n",
            self.module_ego_name,
            self.ego_alias,
            self.ego_deco.key(),
            ego_farben.join(","),
            self.ego_shadow.key(),
            self.ego_light,
            self.ego_shadow_light,
            self.ego_speed,
            self.ego_sound,
            self.ego_volume
        ));
        for (k, v) in &self.addon_values {
            body.push_str(&format!("addon:{k}={}\n", v.replace(['\n', '\r'], " ")));
        }
        body.push_str(&format!("module_color={}\ncolor_saturation={}\n", self.module_color, self.color_saturation));
        body.push_str(&format!(
            "module_sustainability={}\nsustain_background={}\nsustain_pause={}\n",
            self.module_sustainability, self.sustain_background, self.sustain_pause
        ));
        body.push_str(&format!("anim_off={}\nanims_enabled={}\n", world::more_anims::ALL & !self.anim_flags, self.anims_enabled));
        body.push_str(&format!(
            "module_chunk_saver={}\nchunk_saver_threads={}\nchunk_saver_render_saved={}\nlod_render_distance={}\nchunk_saver_detail={}\n",
            self.module_chunk_saver,
            self.chunk_saver_threads,
            self.chunk_saver_render_saved,
            self.lod_render_distance,
            self.chunk_saver_threshold
        ));
        body.push_str(&format!("sprint_on_start={}\n", self.sprint_on_start));
        body.push_str(&format!("caps_lock_blocked={}\n", self.caps_lock_blocked));
        body.push_str(&format!("creative_op_tab={}\n", self.creative_op_tab));
        body.push_str(&format!("server_close_blocked={}\n", self.server_close_blocked));
        body.push_str(&format!("server_open={}\n", self.server_open.iter().map(u8::to_string).collect::<Vec<_>>().join(",")));
        body.push_str(&format!("no_chat_clear={}\n", self.no_chat_clear));
        body.push_str(&format!("audio_auto_optimize={}\n", self.audio_auto_optimize));
        body.push_str(&format!("sign_edit_in_world={}\n", self.sign_edit_in_world));
        body.push_str(&format!("no_slot_set={}\nno_rotate_set={}\n", self.no_slot_set, self.no_rotate_set));
        body.push_str(&format!("skip_third_back={}\nskip_third_front={}\n", self.skip_third_back, self.skip_third_front));
        body.push_str(&format!("inventory_merge_hover={}\n", self.inventory_merge_hover));
        let [h, s, v, a] = self.slot_hover_color;
        body.push_str(&format!("slot_hover_fade_ms={}\nslot_hover_color={h}/{s}/{v}/{a}\nswap_anim_ms={}\n", self.slot_hover_fade_ms, self.swap_anim_ms));
        body.push_str(&format!(
            "potion_mark_strong={}\npotion_mark_long={}\npotion_mark_weak={}\n",
            self.potion_mark_strong, self.potion_mark_long, self.potion_mark_weak
        ));
        for action in KeyBindAction::ALL {
            body.push_str(&format!("keybind_{}={}\n", action.settings_key(), binds_to_str(kb, action)));
        }
        for action in KeyBindAction::ALL.into_iter().filter(|a| a.has_mode()) {
            body.push_str(&format!("keybind_mode_{}={}\n", action.settings_key(), keybind_mode_to_str(km.get(action))));
        }
        body
    }
}

// Eine Zeile `key=value` anhaengen.
fn kv(body: &mut String, key: &str, value: impl std::fmt::Display) {
    use std::fmt::Write;
    let _ = writeln!(body, "{key}={value}");
}
