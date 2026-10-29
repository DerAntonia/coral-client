// Das Modul-Menue: Zeilen, Kacheln, Layout und Modul-Aktionen.

use crate::*;

impl App {
    // Der Hauptschalter des gerade gezeigten Moduls. Er steht ZWEIMAL im Menue: klein am
    // rechten Rand der linken Liste (Ueberblick ueber alle Module) und als oberste Zeile der
    // Detailspalte. Das ist Absicht und kein Versehen: frueher liessen sich Knoepfe scheinbar
    // nicht druecken. Ursache war, dass die Detailspalte eines ABGESCHALTETEN Moduls
    // komplett unbedienbar ist (s. module_menu_buttons: `enabled`), der einzige Weg, es
    // einzuschalten, aber der leicht zu uebersehende Schalter in der schmalen Listenspalte
    // war. Wer das Modul anklickt und dann rechts nichts bewegen kann, hat keinen Hinweis,
    // was ihm fehlt. Jetzt ist die erste Zeile der Spalte genau dieser Schalter.
    pub(crate) fn module_master_row(&self) -> ModuleRow {
        ModuleRow::Toggle {
            label: "Modul".to_string(),
            action: self.module_menu_selected.toggle_action(),
            on: self.module_menu_selected.enabled(&self.settings),
        }
    }

    // Farbfeld einer Schild-Farbe; aufgeklappt mit ihren Reglern darunter (wie beim Crit-Modul).
    pub(crate) fn push_shield_color_rows(&self, rows: &mut Vec<ModuleRow>, farbe: ShieldColor, label: &str) {
        let [r, g, b, _] = self.settings.shield_color_rgba(farbe);
        let offen = self.shield_color_editor == Some(farbe);
        rows.push(ModuleRow::Swatch {
            label: label.to_string(),
            action: ButtonAction::ShieldColorEditor(farbe),
            rgb: [r, g, b],
        });
        if !offen {
            return;
        }
        for &achse in ShieldColor::axes(self.settings.shield_mark(farbe)) {
            let wert = self.settings.shield_color_of(farbe, achse);
            let (min, max, _) = achse.range();
            rows.push(ModuleRow::Stepper {
                label: format!("    {}", achse.label()),
                prev: ButtonAction::ShieldColorPrev(farbe, achse),
                next: ButtonAction::ShieldColorNext(farbe, achse),
                text: format!("{wert} {}", achse.unit()),
                can_prev: achse.cyclic() || wert > min,
                can_next: achse.cyclic() || wert < max,
                slider: Some((wert as f64, min as f64, max as f64)),
            });
        }
    }

    // "Helligkeit" des Regenbogens beim Namen bzw. Schatten: wie weit er zu Weiss hin geht.
    pub(crate) fn ego_light_row(schatten: bool, wert: u32) -> ModuleRow {
        ModuleRow::Stepper {
            label: "Helligkeit".to_string(),
            prev: ButtonAction::EgoLight(schatten, false),
            next: ButtonAction::EgoLight(schatten, true),
            text: format!("{wert} %"),
            can_prev: wert > 0,
            can_next: wert < 100,
            slider: Some((wert as f64, 0.0, 100.0)),
        }
    }

    // Die Detailzeilen des ausgewaehlten Moduls, als Daten statt als fertige Knoepfe/Texte.
    // Zeile 0 ist immer der Hauptschalter (s. module_master_row), danach kommen die
    // modul-eigenen Einstellungen.
    pub(crate) fn module_detail_rows(&self) -> Vec<ModuleRow> {
        let set = &self.settings;
        let mut rows = vec![self.module_master_row()];
        match self.module_menu_selected {
            ModuleId::Addon(m) => self.push_addon_rows(m, &mut rows),
            ModuleId::Totem => rows.push(ModuleRow::Toggle {
                label: "Signalton".to_string(),
                action: ButtonAction::ToggleModuleTotemSound,
                on: set.module_totem_sound,
            }),
            ModuleId::Hitbox => {
                // Je Zeile der Schalter und rechts daneben sein Farbfeld; die Regler stecken im
                // Farbfenster (s. ColorPopup).
                for (variant, label, on, action) in [
                    (HitboxVariant::Smoothed, "Geglättet", set.module_hitbox_show_smoothed, ButtonAction::ToggleModuleHitboxSmoothed),
                    (HitboxVariant::Raw, "Roh (Server)", set.module_hitbox_show_raw, ButtonAction::ToggleModuleHitboxRaw),
                ] {
                    let (hue, saturation, value) = self.hitbox_color_of(variant);
                    let rgb = hsv_to_rgb(hue as f32, saturation as f32 / 100.0, value as f32 / 100.0);
                    rows.push(ModuleRow::ToggleSwatch { label: label.to_string(), action, on, swatch: ButtonAction::HitboxColorEditor(variant), rgb });
                }
                let art_zeile = |art: hitbox_kinds::HitboxKind, label: &str| {
                    let st = set.module_hitbox_styles[art.index()];
                    ModuleRow::ToggleSwatch {
                        label: label.to_string(),
                        action: ButtonAction::ToggleHitboxKind(art),
                        on: st.on,
                        swatch: ButtonAction::OpenColorPicker(ColorTarget::HitboxKind(art)),
                        rgb: hsv_to_rgb(st.hue as f32, st.saturation as f32 / 100.0, st.value as f32 / 100.0),
                    }
                };
                // Die Farbe der Items ist die von "Geglättet", eine eigene gibt es unten mit
                // Ueberschreiben.
                rows.push(ModuleRow::Toggle { label: "Item-Hitbox".to_string(), action: ButtonAction::ToggleModuleHitboxItems, on: set.module_hitbox_items });
                rows.push(ModuleRow::Toggle { label: "Eigene Hitbox".to_string(), action: ButtonAction::ToggleModuleHitboxSelf, on: set.module_hitbox_self });
                rows.push(art_zeile(hitbox_kinds::HitboxKind::Cobweb, "Spinnennetz-Hitbox"));
                rows.push(art_zeile(hitbox_kinds::HitboxKind::Water, "Wasser-Hitbox"));
                rows.push(ModuleRow::Toggle {
                    label: "Gepunktet".to_string(),
                    action: ButtonAction::ToggleModuleHitboxDotted,
                    on: set.module_hitbox_dotted,
                });
                rows.push(ModuleRow::Stepper {
                    label: "Dichte".to_string(),
                    prev: ButtonAction::HitboxDensityPrev,
                    next: ButtonAction::HitboxDensityNext,
                    text: format!("{} px", set.module_hitbox_density_px),
                    can_prev: set.module_hitbox_density_px > HITBOX_DENSITY_MIN,
                    can_next: set.module_hitbox_density_px < HITBOX_DENSITY_MAX,
                    slider: Some((set.module_hitbox_density_px as f64, HITBOX_DENSITY_MIN as f64, HITBOX_DENSITY_MAX as f64)),
                });
                rows.push(ModuleRow::Stepper {
                    label: "Deckkraft".to_string(),
                    prev: ButtonAction::HitboxOpacityPrev,
                    next: ButtonAction::HitboxOpacityNext,
                    text: format!("{} %", set.module_hitbox_opacity),
                    can_prev: set.module_hitbox_opacity > 0,
                    can_next: set.module_hitbox_opacity < 100,
                    slider: Some((set.module_hitbox_opacity as f64, 0.0, 100.0)),
                });
                // Je PvP-Entity: ob es die Vorgaben von oben ueberschreibt, und wenn ja, ob es
                // sichtbar ist und in welcher Farbe und Deckkraft (im Farbfenster). Ohne
                // Ueberschreiben sind Sichtbar und Farbe ausgegraut. Was hier fehlt (Mobs), hat
                // immer die Vorgaben.
                rows.push(ModuleRow::Heading { label: "Individuelle Farben".to_string(), columns: &["Überschreiben", "Sichtbar"] });
                for art in hitbox_kinds::HitboxKind::entities() {
                    let st = set.module_hitbox_styles[art.index()];
                    rows.push(ModuleRow::TwoTogglesSwatch {
                        label: art.label().to_string(),
                        first: (ButtonAction::ToggleHitboxKindOverride(art), st.custom),
                        second: (ButtonAction::ToggleHitboxKind(art), st.on),
                        swatch: ButtonAction::OpenColorPicker(ColorTarget::HitboxKind(art)),
                        rgb: hsv_to_rgb(st.hue as f32, st.saturation as f32 / 100.0, st.value as f32 / 100.0),
                    });
                }
            }
            ModuleId::Teams => {
                // Verbuendet ist, wer bei einer eingeschalteten Erkennung passt.
                rows.push(ModuleRow::Heading { label: "Erkennung".to_string(), columns: &[] });
                for (i, label) in teams::Matchers::LABELS.iter().enumerate() {
                    rows.push(ModuleRow::Toggle { label: label.to_string(), action: ButtonAction::TeamMatch(i), on: set.team_match.get(i) });
                }
                // Die eigene Teamfarbe: automatisch oder eine der Namensfarben, die gerade
                // jemand traegt (Hex-Werte tippt niemand ab).
                let rgb = |c: u32| [((c >> 16) & 0xFF) as f32 / 255.0, ((c >> 8) & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0];
                rows.push(ModuleRow::ColorPicks {
                    label: "Eigene Teamfarbe".to_string(),
                    first: ("Automatisch", ButtonAction::TeamOwnColor(None), set.team_own_color.is_none()),
                    colors: self
                        .team_color_suggestions()
                        .into_iter()
                        .map(|c| (ButtonAction::TeamOwnColor(Some(c)), rgb(c), set.team_own_color == Some(c)))
                        .collect(),
                });
                for seite in teams::Side::ALL {
                    let st = set.team_hitboxes[seite.index()];
                    rows.push(ModuleRow::Heading { label: format!("{}-Hitboxen", seite.label()), columns: &[] });
                    rows.push(ModuleRow::Toggle { label: "Überschreiben".to_string(), action: ButtonAction::TeamToggle(seite), on: st.on });
                    rows.push(ModuleRow::Pick {
                        label: "Farbe".to_string(),
                        options: vec![
                            ("Namensschild", ButtonAction::TeamNametag(seite, true), st.nametag),
                            ("Fest", ButtonAction::TeamNametag(seite, false), !st.nametag),
                        ],
                        // Das Farbfeld nur mit fester Farbe -- mit Namensschild gibt es nichts einzustellen.
                        swatch: (!st.nametag).then(|| {
                            (
                                ButtonAction::OpenColorPicker(ColorTarget::Team(seite)),
                                hsv_to_rgb(st.hue as f32, st.saturation as f32 / 100.0, st.value as f32 / 100.0),
                            )
                        }),
                    });
                    rows.push(ModuleRow::Stepper {
                        label: "Deckkraft".to_string(),
                        prev: ButtonAction::TeamOpacity(seite, false),
                        next: ButtonAction::TeamOpacity(seite, true),
                        text: format!("{} %", st.opacity),
                        can_prev: st.opacity > 0,
                        can_next: st.opacity < 100,
                        slider: Some((st.opacity as f64, 0.0, 100.0)),
                    });
                    rows.push(ModuleRow::Stepper {
                        label: "Dichte".to_string(),
                        prev: ButtonAction::TeamDensity(seite, false),
                        next: ButtonAction::TeamDensity(seite, true),
                        text: format!("{} px", st.density_px),
                        can_prev: st.density_px > HITBOX_DENSITY_MIN,
                        can_next: st.density_px < HITBOX_DENSITY_MAX,
                        slider: Some((st.density_px as f64, HITBOX_DENSITY_MIN as f64, HITBOX_DENSITY_MAX as f64)),
                    });
                    rows.push(ModuleRow::Toggle { label: "Gepunktet".to_string(), action: ButtonAction::TeamDotted(seite), on: st.dotted });
                }
            }
            ModuleId::Arrows => {
                let zehntel = |t: u32| format!("{}.{} s", t / 10, t % 10);
                // Wie die Anzeige aussieht, stellt ihr Fenster im HUD-Editor ein.
                rows.push(ModuleRow::Toggle { label: "Anzeige".to_string(), action: ButtonAction::ToggleArrowIndicator, on: set.arrow_indicator });
                rows.push(ModuleRow::Choice {
                    label: "Ohne Pfeile".to_string(),
                    action: ButtonAction::CycleArrowEmptyFeedback,
                    text: ARROW_EMPTY_FEEDBACK_NAMES[set.arrow_empty_feedback as usize].to_string(),
                });
                rows.push(ModuleRow::Toggle { label: "Weißer Pfeil".to_string(), action: ButtonAction::ToggleArrowWhite, on: set.arrow_white });
                rows.push(ModuleRow::Toggle { label: "Trail".to_string(), action: ButtonAction::ToggleArrowTrail, on: set.arrow_trail });
                rows.push(ModuleRow::Stepper {
                    label: "  Zeit bis Fade".to_string(),
                    prev: ButtonAction::TrailHoldPrev,
                    next: ButtonAction::TrailHoldNext,
                    text: zehntel(set.trail_hold_tenths),
                    can_prev: set.trail_hold_tenths > 0,
                    can_next: set.trail_hold_tenths < TRAIL_HOLD_MAX_TENTHS,
                    slider: Some((set.trail_hold_tenths as f64, 0.0, TRAIL_HOLD_MAX_TENTHS as f64)),
                });
                rows.push(ModuleRow::Stepper {
                    label: "  Fade-Dauer".to_string(),
                    prev: ButtonAction::TrailFadePrev,
                    next: ButtonAction::TrailFadeNext,
                    text: zehntel(set.trail_fade_tenths),
                    can_prev: set.trail_fade_tenths > 0,
                    can_next: set.trail_fade_tenths < TRAIL_FADE_MAX_TENTHS,
                    slider: Some((set.trail_fade_tenths as f64, 0.0, TRAIL_FADE_MAX_TENTHS as f64)),
                });
            }
            ModuleId::ArmorHud => {
                rows.push(ModuleRow::Choice { label: "Aussehen".to_string(), action: ButtonAction::OpenHudConfig(HudElement::Armor), text: "Im HUD-Editor".to_string() });
            }
            ModuleId::Keystrokes => {
                rows.push(ModuleRow::Choice { label: "Aussehen".to_string(), action: ButtonAction::OpenHudConfig(HudElement::Keystrokes), text: "Im HUD-Editor".to_string() });
            }
            ModuleId::SessionInfo => {
                for (bit, label) in session_info::LINES {
                    rows.push(ModuleRow::Toggle { label: label.to_string(), action: ButtonAction::ToggleSessionLine(bit), on: set.session_lines & bit != 0 });
                }
                rows.push(ModuleRow::Choice { label: "Aussehen".to_string(), action: ButtonAction::OpenHudConfig(HudElement::Session), text: "Im HUD-Editor".to_string() });
                rows.push(ModuleRow::Choice { label: "Zähler".to_string(), action: ButtonAction::ResetSession, text: "Auf null setzen".to_string() });
            }
            ModuleId::Shields => {
                // Wie Countdown bzw. Kreis aussehen, stellt der HUD-Editor ein: der Countdown
                // hat dort ein eigenes Fenster, der Kreis gehoert zum Fadenkreuz.
                rows.push(ModuleRow::Toggle { label: "Cooldown: eigener".to_string(), action: ButtonAction::ToggleShieldCdSelf, on: set.shield_cd_self });
                rows.push(pick_two("  Anzeige", ButtonAction::CycleShieldSelfMode, SHIELD_SELF_MODE_NAMES, set.shield_cd_self_mode as usize));
                let ziel = if set.shield_cd_self_mode == 0 { HudElement::ShieldCooldown } else { HudElement::Crosshair };
                rows.push(ModuleRow::Choice { label: "  Aussehen".to_string(), action: ButtonAction::OpenHudConfig(ziel), text: "Im HUD-Editor".to_string() });
                for (farbe, label, action, on) in [
                    (ShieldColor::Cooldown, "Cooldown: Spieler", ButtonAction::ToggleShieldCdPlayers, set.shield_cd_players),
                    (ShieldColor::Hold, "Hochhalten: Spieler", ButtonAction::ToggleShieldHoldPlayers, set.shield_hold_players),
                ] {
                    rows.push(ModuleRow::Toggle { label: label.to_string(), action, on });
                    let mark = set.shield_mark(farbe);
                    let namen = [ShieldMark::Model.label(), ShieldMark::Hitbox.label()];
                    rows.push(pick_two("  Anzeige", ButtonAction::CycleShieldMark(farbe), namen, mark.index() as usize));
                    let farb_label = match mark {
                        ShieldMark::Hitbox => "  Hitbox-Farbe",
                        ShieldMark::Model => "  Farbfilter",
                    };
                    self.push_shield_color_rows(&mut rows, farbe, farb_label);
                }
            }
            ModuleId::SkyColor => {
                // Uhrzeit oben: sie gilt auch fuer den Himmel aus Resourcepacks, die Farbe nur,
                // solange kein Pack-Himmel sie verdeckt.
                rows.push(ModuleRow::Toggle { label: "Feste Uhrzeit".to_string(), action: ButtonAction::ToggleSkyFixedTime, on: set.module_sky_fixed_time });
                if set.module_sky_fixed_time {
                    rows.push(ModuleRow::Stepper {
                        label: "  Uhrzeit".to_string(),
                        prev: ButtonAction::SkyTimePrev,
                        next: ButtonAction::SkyTimeNext,
                        text: sky_time_label(set.module_sky_time),
                        can_prev: true,
                        can_next: true,
                        slider: Some((set.module_sky_time as f64, 0.0, (SKY_TIME_DAY - SKY_TIME_STEP) as f64)),
                    });
                }
                rows.push(ModuleRow::Choice { label: "Himmel".to_string(), action: ButtonAction::CycleSkyMode, text: set.sky_mode.label().to_string() });
                match set.sky_mode {
                    SkyMode::Solid => {}
                    SkyMode::Gradient => {
                        rows.push(ModuleRow::Toggle { label: "  Sonne".to_string(), action: ButtonAction::ToggleSkySun, on: set.sky_sun });
                        rows.push(ModuleRow::Toggle { label: "  Mond".to_string(), action: ButtonAction::ToggleSkyMoon, on: set.sky_moon });
                        rows.push(ModuleRow::Toggle { label: "  Sterne".to_string(), action: ButtonAction::ToggleSkyStars, on: set.sky_stars });
                        return rows;
                    }
                    SkyMode::Skin => return rows,
                }
                // Farbfeld; die Regler stecken im Farbfenster (s. ColorPopup).
                rows.push(ModuleRow::Swatch {
                    label: "  Farbe".to_string(),
                    action: ButtonAction::OpenColorPicker(ColorTarget::Sky),
                    rgb: set.sky_color_rgb(),
                });
            }
            ModuleId::Weather => {
                rows.push(ModuleRow::Choice { label: "Wetter".to_string(), action: ButtonAction::CycleWeatherMode, text: set.weather_mode.label().to_string() });
                rows.push(ModuleRow::Stepper {
                    label: "Von".to_string(),
                    prev: ButtonAction::WeatherRadiusInnerPrev,
                    next: ButtonAction::WeatherRadiusInnerNext,
                    text: format!("{} Blöcke", set.weather_radius_inner),
                    can_prev: set.weather_radius_inner > 0,
                    can_next: set.weather_radius_inner + 1 < set.weather_radius,
                    slider: Some((set.weather_radius_inner as f64, 0.0, set.weather_radius.saturating_sub(1) as f64)),
                });
                rows.push(ModuleRow::Stepper {
                    label: "Bis".to_string(),
                    prev: ButtonAction::WeatherRadiusPrev,
                    next: ButtonAction::WeatherRadiusNext,
                    text: format!("{} Blöcke", set.weather_radius),
                    can_prev: set.weather_radius > precipitation::RADIUS_MIN.max(set.weather_radius_inner + 1),
                    can_next: set.weather_radius < precipitation::RADIUS_MAX,
                    slider: Some((set.weather_radius as f64, precipitation::RADIUS_MIN as f64, precipitation::RADIUS_MAX as f64)),
                });
            }
            ModuleId::TotemWarning | ModuleId::TotemSlots | ModuleId::PopSounds | ModuleId::PopVisuals => {
                rows.extend(self.toteming_rows(self.module_menu_selected));
            }
            ModuleId::CrystalOptimizer | ModuleId::UseTimer | ModuleId::BotcheckPass | ModuleId::DynamicFire | ModuleId::ShowMarkers | ModuleId::SoundDebug | ModuleId::ConnectionDebug | ModuleId::FpsDisplay | ModuleId::InputLog => {}
            ModuleId::Sustainability => {
                rows.push(ModuleRow::Toggle {
                    label: "Im Hintergrund pausieren".to_string(),
                    action: ButtonAction::ToggleSustainBackground,
                    on: set.sustain_background,
                });
                rows.push(ModuleRow::Toggle { label: "3D im Pausemenü pausieren".to_string(), action: ButtonAction::ToggleSustainPause, on: set.sustain_pause });
            }
            ModuleId::ChunkSaver => rows.extend(self.chunk_saver_rows()),
            ModuleId::Color => {
                rows.push(ModuleRow::Stepper {
                    label: "Sättigung".to_string(),
                    prev: ButtonAction::ColorSaturationPrev,
                    next: ButtonAction::ColorSaturationNext,
                    text: format!("{} %", set.color_saturation),
                    can_prev: set.color_saturation > 0,
                    can_next: set.color_saturation < COLOR_SATURATION_MAX,
                    slider: Some((set.color_saturation as f64, 0.0, COLOR_SATURATION_MAX as f64)),
                });
            }
            ModuleId::DiscordStatus => {
                // Eine eigene ID kommt aus der Zwischenablage (Discord-Entwicklerportal ->
                // Anwendung -> "Application ID" kopieren) -- ein Klick statt eines Textfelds;
                // ein weiterer Klick fuehrt zur Vorgabe zurueck (s. DiscordPasteClientId).
                // Leer = die Anwendung des Projekts (s. discord_rpc::DEFAULT_CLIENT_ID).
                let id = if set.discord_client_id.is_empty() { "Coral Client (Vorgabe)".to_string() } else { set.discord_client_id.clone() };
                rows.push(ModuleRow::Choice { label: "Application-ID".to_string(), action: ButtonAction::DiscordPasteClientId, text: id });
                let status = if set.module_discord_status { self.discord.status() } else { discord_rpc::Status::Off };
                rows.push(ModuleRow::Choice { label: "Status".to_string(), action: ButtonAction::DiscordPasteClientId, text: status.label().to_string() });
            }
            ModuleId::BlockOutline => {
                // Komplementaer oder Einfarbig, rechts daneben die Farbe; ein Klick darauf oeffnet
                // das Farbfenster (Ton, Saettigung, Helligkeit, Deckkraft) daneben.
                let [r, g, b, _] = set.block_outline_rgba();
                let komplementaer = set.block_outline_complementary;
                rows.push(ModuleRow::Pick {
                    label: "Farbe".to_string(),
                    options: vec![
                        ("Komplementär", ButtonAction::SetBlockOutlineComplementary(true), komplementaer),
                        ("Einfarbig", ButtonAction::SetBlockOutlineComplementary(false), !komplementaer),
                    ],
                    swatch: Some((ButtonAction::BlockOutlineEditor, [r, g, b])),
                });
                rows.push(ModuleRow::Stepper {
                    label: "Breite".to_string(),
                    prev: ButtonAction::BlockOutlineWidthPrev,
                    next: ButtonAction::BlockOutlineWidthNext,
                    text: format!("{} px", set.block_outline_width_px),
                    can_prev: set.block_outline_width_px > BLOCK_OUTLINE_WIDTH_MIN,
                    can_next: set.block_outline_width_px < BLOCK_OUTLINE_WIDTH_MAX,
                    slider: Some((set.block_outline_width_px as f64, BLOCK_OUTLINE_WIDTH_MIN as f64, BLOCK_OUTLINE_WIDTH_MAX as f64)),
                });
                rows.push(ModuleRow::Toggle {
                    label: "Beim Abbauen ausblenden".to_string(),
                    action: ButtonAction::ToggleBlockOutlineHideMining,
                    on: set.block_outline_hide_mining,
                });
            }
            ModuleId::VoiceChat => {
                let v = &set.voice;
                // Zahlenwert als Regler: Wert und Grenzen, die Pfeile schlagen an den Grenzen an.
                let stepper = |rows: &mut Vec<ModuleRow>, label: &str, knob: VoiceKnob, text: String, wert: i64, min: i64, max: i64| {
                    rows.push(ModuleRow::Stepper {
                        label: label.to_string(),
                        prev: ButtonAction::VoiceStep(knob, false),
                        next: ButtonAction::VoiceStep(knob, true),
                        text,
                        can_prev: wert > min,
                        can_next: wert < max,
                        slider: Some((wert as f64, min as f64, max as f64)),
                    });
                };
                let schalter = |rows: &mut Vec<ModuleRow>, label: &str, flag: VoiceFlag, on: bool| {
                    rows.push(ModuleRow::Toggle { label: label.to_string(), action: ButtonAction::VoiceFlag(flag), on });
                };
                let auswahl = |rows: &mut Vec<ModuleRow>, label: &str, flag: VoiceFlag, text: String| {
                    rows.push(ModuleRow::Choice { label: label.to_string(), action: ButtonAction::VoiceFlag(flag), text });
                };
                stepper(&mut rows, "Lautstärke der anderen", VoiceKnob::Volume, format!("{} %", v.volume_percent), v.volume_percent as i64, 0, voicechat::VOLUME_MAX as i64);
                stepper(
                    &mut rows,
                    "Mikrofon-Verstärkung",
                    VoiceKnob::MicGain,
                    format!("{:+} dB", v.mic_gain_db),
                    v.mic_gain_db as i64,
                    voicechat::GAIN_DB_MIN as i64,
                    voicechat::GAIN_DB_MAX as i64,
                );
                rows.push(pick_two("Aktivierung", ButtonAction::VoiceFlag(VoiceFlag::Activation), ["Sprechtaste", "Sprachaktivierung"], v.voice_activation as usize));
                if v.voice_activation {
                    // Mit dem aktuellen Pegel daneben, damit sich die Schwelle einstellen laesst.
                    let jetzt = if self.voice.mic.is_some() { format!(" (jetzt {:.0})", self.voice.level_db) } else { String::new() };
                    stepper(
                        &mut rows,
                        "Aktivierungsschwelle",
                        VoiceKnob::Threshold,
                        format!("{} dB{jetzt}", v.threshold_db),
                        v.threshold_db as i64,
                        voicechat::THRESHOLD_DB_MIN as i64,
                        0,
                    );
                }
                let nachlauf = if v.voice_activation { v.voice_delay_frames } else { v.ptt_delay_frames };
                stepper(&mut rows, "Nachlauf", VoiceKnob::Delay, voicechat::frames_label(nachlauf), nachlauf as i64, 0, voicechat::DELAY_MAX_FRAMES as i64);
                schalter(&mut rows, "Mikrofon stumm", VoiceFlag::Muted, v.muted);
                schalter(&mut rows, "Sprachchat taub", VoiceFlag::Disabled, v.disabled);
                schalter(&mut rows, "Stumm beim Betreten", VoiceFlag::MuteOnJoin, v.mute_on_join);
                schalter(&mut rows, "Automatische Verstärkung", VoiceFlag::Agc, v.agc);
                schalter(&mut rows, "Rauschunterdrückung", VoiceFlag::Denoise, v.denoise);
                auswahl(&mut rows, "3D-Klang", VoiceFlag::AudioType, v.audio_type.label().to_string());
                stepper(
                    &mut rows,
                    "Ausgabepuffer",
                    VoiceKnob::OutputBuffer,
                    voicechat::frames_label(v.output_buffer_frames),
                    v.output_buffer_frames as i64,
                    1,
                    voicechat::BUFFER_MAX_FRAMES as i64,
                );
                stepper(
                    &mut rows,
                    "Paket-Schwelle",
                    VoiceKnob::PacketThreshold,
                    v.packet_threshold.to_string(),
                    v.packet_threshold as i64,
                    0,
                    voicechat::PACKET_THRESHOLD_MAX as i64,
                );
                auswahl(&mut rows, "Mikrofon", VoiceFlag::MicDevice, if v.mic_device.is_empty() { "Standard".to_string() } else { v.mic_device.clone() });
                schalter(&mut rows, "Mikrofon mithören", VoiceFlag::Monitor, self.voice.monitor);
                schalter(&mut rows, "Symbol am Namensschild", VoiceFlag::NametagIcons, v.nametag_icons);
                schalter(&mut rows, "Anzeige im HUD", VoiceFlag::HudIcons, v.hud_icons);
            }
            ModuleId::Zoom => {
                rows.push(ModuleRow::Stepper {
                    label: "Zoomstärke".to_string(),
                    prev: ButtonAction::ZoomFactorPrev,
                    next: ButtonAction::ZoomFactorNext,
                    text: format!("{:.1} x", set.zoom_factor_tenths as f32 / 10.0),
                    can_prev: set.zoom_factor_tenths > ZOOM_FACTOR_MIN,
                    can_next: set.zoom_factor_tenths < ZOOM_FACTOR_MAX,
                    slider: Some((set.zoom_factor_tenths as f64, ZOOM_FACTOR_MIN as f64, ZOOM_FACTOR_MAX as f64)),
                });
                rows.push(ModuleRow::Stepper {
                    label: "Sens-Koeffizient".to_string(),
                    prev: ButtonAction::ZoomSensPrev,
                    next: ButtonAction::ZoomSensNext,
                    text: format!("{:.2}", set.zoom_sens_percent as f32 / 100.0),
                    can_prev: set.zoom_sens_percent > ZOOM_SENS_MIN,
                    can_next: set.zoom_sens_percent < ZOOM_SENS_MAX,
                    slider: Some((set.zoom_sens_percent as f64, ZOOM_SENS_MIN as f64, ZOOM_SENS_MAX as f64)),
                });
            }
            ModuleId::ConsumableOptimizer => {
                rows.push(ModuleRow::Stepper {
                    label: "Durchdrücken bei Rest".to_string(),
                    prev: ButtonAction::EatHoldThroughPrev,
                    next: ButtonAction::EatHoldThroughNext,
                    text: if set.eat_hold_through_ms == 0 { "aus".to_string() } else { format!("{} ms", set.eat_hold_through_ms) },
                    can_prev: set.eat_hold_through_ms > 0,
                    can_next: set.eat_hold_through_ms < EAT_HOLD_THROUGH_MAX,
                    slider: Some((set.eat_hold_through_ms as f64, 0.0, EAT_HOLD_THROUGH_MAX as f64)),
                });
                rows.push(ModuleRow::Toggle {
                    label: "Slotwechsel erst nach dem Essen".to_string(),
                    action: ButtonAction::ToggleEatSlotLock,
                    on: set.eat_slot_lock,
                });
                rows.push(ModuleRow::Toggle {
                    label: "Aggressive Prediction".to_string(),
                    action: ButtonAction::ToggleEatAggressivePrediction,
                    on: set.eat_aggressive_prediction,
                });
            }
            ModuleId::NoToolBreak => rows.push(ModuleRow::Stepper {
                label: "Sperren ab Resthaltbarkeit".to_string(),
                prev: ButtonAction::NoToolBreakReservePrev,
                next: ButtonAction::NoToolBreakReserveNext,
                text: set.no_tool_break_reserve.to_string(),
                can_prev: set.no_tool_break_reserve > NO_TOOL_BREAK_MIN_RESERVE,
                can_next: set.no_tool_break_reserve < NO_TOOL_BREAK_MAX_RESERVE,
                slider: Some((set.no_tool_break_reserve as f64, NO_TOOL_BREAK_MIN_RESERVE as f64, NO_TOOL_BREAK_MAX_RESERVE as f64)),
            }),
            ModuleId::SaturationHud => {
                rows.push(ModuleRow::Swatch {
                    label: "Farbe".to_string(),
                    action: ButtonAction::OpenColorPicker(ColorTarget::SaturationHud),
                    rgb: set.saturation_hud_rgb(),
                });
                rows.push(ModuleRow::Stepper {
                    label: "Dicke".to_string(),
                    prev: ButtonAction::SaturationHeightPrev,
                    next: ButtonAction::SaturationHeightNext,
                    text: format!("{} px", set.saturation_hud_height),
                    can_prev: set.saturation_hud_height > SATURATION_BAR_HEIGHT_MIN,
                    can_next: set.saturation_hud_height < SATURATION_BAR_HEIGHT_MAX,
                    slider: Some((set.saturation_hud_height as f64, SATURATION_BAR_HEIGHT_MIN as f64, SATURATION_BAR_HEIGHT_MAX as f64)),
                });
                for (raster, label, wert) in [(false, "Grund", set.saturation_hud_bg_alpha), (true, "Raster", set.saturation_hud_grid_alpha)] {
                    rows.push(ModuleRow::Stepper {
                        label: label.to_string(),
                        prev: ButtonAction::SaturationAlpha(raster, false),
                        next: ButtonAction::SaturationAlpha(raster, true),
                        text: format!("{wert} %"),
                        can_prev: wert > 0,
                        can_next: wert < 100,
                        slider: Some((wert as f64, 0.0, 100.0)),
                    });
                }
            }
            ModuleId::EgoName => {
                use ego_name::Deco;
                rows.push(ModuleRow::Choice {
                    label: "Anzeigen als".to_string(),
                    action: ButtonAction::EditEgoAlias,
                    text: if set.ego_alias.is_empty() { "eigener Name".to_string() } else { set.ego_alias.clone() },
                });
                rows.push(ModuleRow::Choice { label: "Färbung".to_string(), action: ButtonAction::CycleEgoDeco, text: set.ego_deco.label().to_string() });
                if matches!(set.ego_deco, Deco::Solid | Deco::Gradient | Deco::Flowing) {
                    let erste = if set.ego_deco.two_colors() { "Farbe 1" } else { "Farbe" };
                    rows.push(ModuleRow::Swatch { label: erste.to_string(), action: ButtonAction::OpenColorPicker(ColorTarget::EgoName(0)), rgb: set.ego_rgb(0) });
                }
                if set.ego_deco.two_colors() {
                    rows.push(ModuleRow::Swatch { label: "Farbe 2".to_string(), action: ButtonAction::OpenColorPicker(ColorTarget::EgoName(1)), rgb: set.ego_rgb(1) });
                }
                if set.ego_deco == Deco::Rainbow {
                    rows.push(Self::ego_light_row(false, set.ego_light));
                }
                if set.ego_deco.moves() || set.ego_shadow.moves() {
                    rows.push(ModuleRow::Stepper {
                        label: "Tempo".to_string(),
                        prev: ButtonAction::EgoSpeed(false),
                        next: ButtonAction::EgoSpeed(true),
                        text: format!("{:.1} /s", set.ego_speed as f32 / 10.0),
                        can_prev: set.ego_speed > EGO_SPEED_MIN,
                        can_next: set.ego_speed < EGO_SPEED_MAX,
                        slider: Some((set.ego_speed as f64, EGO_SPEED_MIN as f64, EGO_SPEED_MAX as f64)),
                    });
                }
                rows.push(ModuleRow::Choice { label: "Schatten".to_string(), action: ButtonAction::CycleEgoShadow, text: set.ego_shadow.label_or("Standard").to_string() });
                if set.ego_shadow != Deco::Off {
                    // Die erste Schattenfarbe traegt die Deckkraft -- auch beim Regenbogen.
                    let erste = match set.ego_shadow {
                        Deco::Rainbow => "Schatten-Deckkraft",
                        d if d.two_colors() => "Schattenfarbe 1",
                        _ => "Schattenfarbe",
                    };
                    rows.push(ModuleRow::Swatch { label: erste.to_string(), action: ButtonAction::OpenColorPicker(ColorTarget::EgoName(2)), rgb: set.ego_rgb(2) });
                }
                if set.ego_shadow.two_colors() {
                    rows.push(ModuleRow::Swatch { label: "Schattenfarbe 2".to_string(), action: ButtonAction::OpenColorPicker(ColorTarget::EgoName(3)), rgb: set.ego_rgb(3) });
                }
                if set.ego_shadow == Deco::Rainbow {
                    rows.push(Self::ego_light_row(true, set.ego_shadow_light));
                }
                rows.push(ModuleRow::Choice {
                    label: "Ton bei Erwähnung".to_string(),
                    action: ButtonAction::CycleEgoSound,
                    text: ego_name::sound_label(&set.ego_sound).to_string(),
                });
                if !set.ego_sound.is_empty() {
                    rows.push(ModuleRow::Stepper {
                        label: "Lautstärke".to_string(),
                        prev: ButtonAction::EgoVolume(false),
                        next: ButtonAction::EgoVolume(true),
                        text: volume_text(set.ego_volume),
                        can_prev: set.ego_volume > 0.0,
                        // Ohne Obergrenze; der Regler reicht bis 200 %, darueber tippen oder Pfeile.
                        can_next: true,
                        slider: Some((set.ego_volume as f64, 0.0, set.ego_volume.max(200.0) as f64)),
                    });
                }
            }
            ModuleId::Autoclicker => rows.push(ModuleRow::Stepper {
                label: "Klicks je Sekunde".to_string(),
                prev: ButtonAction::AutoclickerCpsPrev,
                next: ButtonAction::AutoclickerCpsNext,
                text: format!("{} CPS", set.autoclicker_cps),
                can_prev: set.autoclicker_cps > AUTOCLICKER_CPS_MIN,
                can_next: set.autoclicker_cps < AUTOCLICKER_CPS_MAX,
                slider: Some((set.autoclicker_cps as f64, AUTOCLICKER_CPS_MIN as f64, AUTOCLICKER_CPS_MAX as f64)),
            }),
            ModuleId::Delay => rows.push(ModuleRow::Stepper {
                label: "Verzögerung je Richtung".to_string(),
                prev: ButtonAction::LagDelayPrev,
                next: ButtonAction::LagDelayNext,
                text: format!("{} ms", set.lag_delay_ms),
                can_prev: set.lag_delay_ms > 0,
                can_next: set.lag_delay_ms < LAG_DELAY_MAX_MS,
                slider: Some((set.lag_delay_ms as f64, 0.0, LAG_DELAY_MAX_MS as f64)),
            }),
            ModuleId::GhostTolerance => rows.push(ModuleRow::Stepper {
                label: "Toleranz".to_string(),
                prev: ButtonAction::GhostTolerancePrev,
                next: ButtonAction::GhostToleranceNext,
                text: format!("{} ms", set.ghost_tolerance_ms),
                can_prev: set.ghost_tolerance_ms > GHOST_TOLERANCE_MIN_MS,
                can_next: set.ghost_tolerance_ms < GHOST_TOLERANCE_MAX_MS,
                slider: Some((set.ghost_tolerance_ms as f64, GHOST_TOLERANCE_MIN_MS as f64, GHOST_TOLERANCE_MAX_MS as f64)),
            }),
        }
        // Was gerade nicht wirkt, steht gar nicht erst da: bei ausgeschaltetem Modul nur der
        // Hauptschalter, sonst ohne die Zeilen, die die uebrigen Einstellungen abschalten.
        if !self.module_menu_selected.enabled(set) {
            rows.truncate(1);
        }
        rows.retain(|r| !self.module_row_greyed(r));
        rows
    }

    // Detailzeilen der vier Toteming-Module (s. toteming.rs), ohne den Hauptschalter.
    pub(crate) fn toteming_rows(&self, id: ModuleId) -> Vec<ModuleRow> {
        use toteming::{Action, SoundSlot};
        let t = &self.settings.toteming;
        let toggle = |label: &str, action: Action, on: bool| ModuleRow::Toggle { label: label.to_string(), action: ButtonAction::Toteming(action), on };
        // Umlaufende Auswahl: die Pfeile schlagen nie an.
        let cycle = |label: &str, prev: Action, next: Action, text: String| ModuleRow::Stepper {
            label: label.to_string(),
            prev: ButtonAction::Toteming(prev),
            next: ButtonAction::Toteming(next),
            text,
            can_prev: true,
            can_next: true,
            slider: None,
        };
        let range = |label: &str, prev: Action, next: Action, text: String, value: u32, min: u32, max: u32| ModuleRow::Stepper {
            label: label.to_string(),
            prev: ButtonAction::Toteming(prev),
            next: ButtonAction::Toteming(next),
            text,
            can_prev: value > min,
            can_next: value < max,
            slider: Some((value as f64, min as f64, max as f64)),
        };
        let mut rows = Vec::new();
        match id {
            ModuleId::TotemWarning => {
                rows.push(cycle("Zweithand-X", Action::OffhandColor(-1), Action::OffhandColor(1), t.offhand_color.label().to_string()));
                rows.push(cycle("Hotbar-X", Action::HotbarColor(-1), Action::HotbarColor(1), t.hotbar_color.label().to_string()));
                rows.push(range("Abstand", Action::Offset(-1), Action::Offset(1), format!("{} px", t.offset), t.offset, toteming::MIN_OFFSET, toteming::MAX_OFFSET));
                rows.push(range("Größe", Action::Size(-1), Action::Size(1), format!("{}", t.size), t.size, toteming::MIN_SIZE, toteming::MAX_SIZE));
                rows.push(toggle("Kontur", Action::ToggleShadow, t.shadow));
            }
            ModuleId::TotemSlots => {
                rows.push(cycle("Untergrund", Action::Backdrop(-1), Action::Backdrop(1), t.backdrop.label().to_string()));
                rows.push(toggle("Ein Totem extra", Action::ToggleExtra, t.extra));
            }
            ModuleId::PopSounds => {
                for slot in SoundSlot::ALL {
                    let s = t.slot(slot);
                    rows.push(cycle(slot.label(), Action::Sound(slot, -1), Action::Sound(slot, 1), toteming::sound_label(&s.sound)));
                    rows.push(range("  Lautstärke", Action::Volume(slot, -1), Action::Volume(slot, 1), format!("{} %", s.volume), s.volume, 0, toteming::VOLUME_MAX));
                    rows.push(range(
                        "  Tonhöhe",
                        Action::Pitch(slot, -1),
                        Action::Pitch(slot, 1),
                        format!("{:.2}", s.pitch_factor()),
                        s.pitch,
                        toteming::PITCH_MIN,
                        toteming::PITCH_MAX,
                    ));
                }
            }
            ModuleId::PopVisuals => {
                rows.push(toggle("Eigenes Flug-Totem aus", Action::ToggleHideOwnFloating, t.hide_own_floating));
                rows.push(toggle("Eigene Partikel aus", Action::ToggleHideOwnParticles, t.hide_own_particles));
                rows.push(toggle("Fremde Partikel aus", Action::ToggleHideOtherParticles, t.hide_other_particles));
            }
            _ => {}
        }
        rows
    }

    // HSV-Werte einer Variante -- eine Quelle fuer Anzeige (module_detail_rows) und
    // Vertexbau (build_hitbox_vertices).
    pub(crate) fn hitbox_color_of(&self, variant: HitboxVariant) -> (u32, u32, u32) {
        let set = &self.settings;
        match variant {
            HitboxVariant::Smoothed => {
                (set.module_hitbox_smoothed_hue, set.module_hitbox_smoothed_saturation, set.module_hitbox_smoothed_value)
            }
            HitboxVariant::Raw => (set.module_hitbox_raw_hue, set.module_hitbox_raw_saturation, set.module_hitbox_raw_value),
        }
    }

    // Wie viele Zeilen der AKTUELLEN Ansicht gleichzeitig hineinpassen: Kachelreihen in der
    // Uebersicht, Einstellungszeilen in der Detailansicht.
    pub(crate) fn module_visible_rows(&self, gpu: &GpuState) -> usize {
        let panel = self.module_menu_rect(gpu);
        let scale = self.module_scale(gpu);
        if self.module_menu_detail {
            let inner = panel.h - MODULE_DETAIL_PX_HEAD - MODULE_DETAIL_PX_FOOT;
            (inner / SETTINGS_ROW_H).floor().max(1.0) as usize
        } else {
            let inner = panel.h / scale - Self::MODULE_TOP_H - Self::MODULE_FOOT_H + Self::MODULE_TILE_GAP;
            (inner / (Self::MODULE_TILE_H + Self::MODULE_TILE_GAP)).floor().max(1.0) as usize
        }
    }

    // Zeilenzahl der aktuellen Ansicht (Kachelreihen bzw. Einstellungszeilen).
    pub(crate) fn module_total_rows(&self, gpu: &GpuState) -> usize {
        if self.module_menu_detail {
            self.module_detail_rows().len()
        } else {
            self.module_menu_category.modules().len().div_ceil(self.module_cols(gpu))
        }
    }

    // Kachelspalten: bis zu MODULE_TILE_COLS, weniger, wenn eine Kachel sonst zu schmal fuer
    // ihren Namen neben dem Schalter wuerde (4:3- und 5:4-Bildschirme, kleine Fenster).
    pub(crate) fn module_cols(&self, gpu: &GpuState) -> usize {
        let panel = self.module_menu_rect(gpu);
        let scale = self.module_scale(gpu);
        let gap = Self::MODULE_TILE_GAP * scale;
        let raster = panel.w - (Self::MODULE_CAT_W + 2.0 * Self::MODULE_PAD) * scale;
        // Platz fuer einen Namen wie "Hitbox display" (Schrift in festen Pixeln) plus Schalter.
        let min_w = MODULE_TILE_NAME_PX + (6.0 + 30.0 + 3.0 + 2.0) * scale;
        (((raster + gap) / (min_w + gap)).floor() as usize).clamp(1, Self::MODULE_TILE_COLS)
    }

    // Um wie viele Zeilen sich die Ansicht hoechstens verschieben laesst.
    pub(crate) fn module_max_scroll(&self, gpu: &GpuState) -> usize {
        self.module_total_rows(gpu).saturating_sub(self.module_visible_rows(gpu))
    }

    // Die ganz sichtbaren Zeilen der aktuellen Ansicht beim gleitenden Stand: Index und Platz
    // (0 = oberste Zeile, mit Bruchteil). Was ueber den Rand gleitet, verschwindet ganz. Schrumpft
    // die Ansicht (Farbeditor zugeklappt), federt der Stand in tick_ui_anim zurueck.
    pub(crate) fn module_rows_shown(&self, gpu: &GpuState) -> Vec<(usize, f32)> {
        let stand = self.module_menu_scroll.stand;
        let visible = self.module_visible_rows(gpu);
        let von = self.module_menu_scroll.first();
        (von..(von + visible + 1).min(self.module_total_rows(gpu)))
            .map(|i| (i, i as f32 - stand))
            .filter(|&(_, n)| n >= -0.01 && n <= (visible - 1) as f32 + 0.01)
            .collect()
    }

    // `weg` Zeilen Richtung Ende (s. App::wheel_units).
    pub(crate) fn scroll_module_menu(&mut self, weg: f32) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let max = self.module_max_scroll(gpu) as f32;
        self.module_menu_scroll.push(weg, SETTINGS_SCROLL_ROWS_PER_NOTCH, max);
    }

    // Die Tafel: feste Groesse (Kategorien + drei Kachelspalten), an der GUI-Skalierung des
    // Inventars ausgerichtet und am Bildschirm gekappt. Was nicht hineinpasst, wird gescrollt
    // -- frueher wuchs die Tafel mit der Modulliste und lief unten aus dem Bild.
    pub(crate) fn module_menu_rect(&self, gpu: &GpuState) -> Rect {
        let (sw, sh) = gpu.size();
        let scale = self.module_scale(gpu);
        let grid_w = Self::MODULE_TILE_COLS as f32 * Self::MODULE_TILE_W + (Self::MODULE_TILE_COLS as f32 - 1.0) * Self::MODULE_TILE_GAP;
        let mut w = (Self::MODULE_CAT_W + 2.0 * Self::MODULE_PAD + grid_w) * scale;
        let mut h = Self::MODULE_PANEL_H * scale;
        // Die Detailansicht rechnet in den festen Massen der Einstellungsseiten (s.
        // ModuleDetailLayout) und braucht dafuer mindestens so viel Platz.
        if self.module_menu_detail {
            w = w.max(MODULE_DETAIL_PX_W + 2.0 * MODULE_DETAIL_PX_PAD);
            h = h.max(MODULE_DETAIL_PX_HEAD + 9.0 * SETTINGS_ROW_H + MODULE_DETAIL_PX_FOOT);
        }
        let w = w.min(sw - 2.0 * Self::MODULE_SCREEN_MARGIN);
        let h = h.min(sh - 2.0 * Self::MODULE_SCREEN_MARGIN);
        // Als Einstellungsseite oben links, wo die anderen Seiten ihre Reiter haben, ohne eigene
        // Flaeche (in Inhaltskoordinaten, s. content_offset) -- mittig mit eigenem Hintergrund
        // stand ringsum ein dunkler Rand.
        if let Some((bereich_w, bereich_h)) = self.module_settings_area(gpu) {
            let oben = SETTINGS_FIRST_Y - 44.0;
            // Die Kacheln fuellen den ganzen Platz bis zur rechten Leiste (s. module_tiles).
            let x = (SETTINGS_LABEL_X - 8.0 * scale).round();
            let (w, h) = if self.module_menu_detail {
                (w.min(bereich_w), h.min(bereich_h - oben))
            } else {
                (bereich_w + SETTINGS_LABEL_X - x - Self::MODULE_SCREEN_MARGIN, bereich_h - oben)
            };
            return Rect { x, y: oben, w, h };
        }
        Rect { x: ((sw - w) / 2.0).round(), y: ((sh - h) / 2.0).round(), w, h }
    }

    // Platz fuer die Tafel auf der Einstellungsseite (Breite, Hoehe), None ausserhalb davon.
    pub(crate) fn module_settings_area(&self, gpu: &GpuState) -> Option<(f32, f32)> {
        if self.mode != AppMode::Menu || self.screen != MenuScreen::SettingsModules {
            return None;
        }
        let (sw, sh) = gpu.size();
        let (dx, dy) = self.content_offset();
        Some((sw - dx - self.side_strip_w(sw) - SETTINGS_LABEL_X, sh - dy - Self::MODULE_SCREEN_MARGIN))
    }

    // GUI-Massstab der Tafel: der des Inventars, aber hoechstens so gross, dass sie ganz in
    // ihren Platz passt (Einstellungsseite: neben Leiste und Kategorien, sonst der Bildschirm).
    // Nicht auf ganze Zahlen gerundet: auf 4:3 fiele sie sonst von 3 auf 1, mit winzigen
    // Schaltern neben unveraendert grosser Schrift.
    pub(crate) fn module_scale(&self, gpu: &GpuState) -> f32 {
        let (sw, sh) = gpu.size();
        let scale = self.inventory_scale((sw, sh));
        let (bereich_w, bereich_h) = self
            .module_settings_area(gpu)
            .unwrap_or((sw - 2.0 * Self::MODULE_SCREEN_MARGIN, sh - 2.0 * Self::MODULE_SCREEN_MARGIN));
        let grid_w = Self::MODULE_TILE_COLS as f32 * Self::MODULE_TILE_W + (Self::MODULE_TILE_COLS as f32 - 1.0) * Self::MODULE_TILE_GAP;
        let passt = (bereich_w / (Self::MODULE_CAT_W + 2.0 * Self::MODULE_PAD + grid_w)).min(bereich_h / Self::MODULE_PANEL_H);
        scale.min(passt).max(1.0)
    }

    // Geometrie der Detailansicht: eine Einstellungsspalte mittig im ganzen Fenster.
    pub(crate) fn module_detail_layout(&self, gpu: &GpuState) -> ModuleDetailLayout {
        let panel = self.module_menu_rect(gpu);
        ModuleDetailLayout {
            x: (panel.x + ((panel.w - MODULE_DETAIL_PX_W) / 2.0).max(MODULE_DETAIL_PX_PAD)).round(),
            top: panel.y + MODULE_DETAIL_PX_HEAD,
        }
    }

    // Kopf der Detailansicht: Zurueck-Pfeil links, "Zu" rechts, beide so gross wie auf den
    // Einstellungsseiten.
    pub(crate) fn module_detail_back_rect(panel: Rect) -> Rect {
        Rect { x: panel.x + MODULE_DETAIL_PX_PAD, y: panel.y + 20.0, w: STEPPER_BTN_W, h: SETTINGS_CTRL_H }
    }

    pub(crate) fn module_detail_close_rect(panel: Rect) -> Rect {
        Rect { x: panel.x + panel.w - MODULE_DETAIL_PX_PAD - 120.0, y: panel.y + 20.0, w: 120.0, h: SETTINGS_CTRL_H }
    }

    // Kacheln der aktuellen Kategorie: (Modul, Rechteck), nur die sichtbaren.
    pub(crate) fn module_tiles(&self, gpu: &GpuState) -> Vec<(ModuleId, Rect)> {
        let panel = self.module_menu_rect(gpu);
        let scale = self.module_scale(gpu);
        let x0 = panel.x + (Self::MODULE_CAT_W + Self::MODULE_PAD) * scale;
        let gap = Self::MODULE_TILE_GAP * scale;
        let spalten = self.module_cols(gpu);
        let cols = spalten as f32;
        // Die Breite folgt der Tafel -- auf kleinen Bildschirmen wird sie gekappt.
        let tile_w = ((panel.x + panel.w - Self::MODULE_PAD * scale - x0) - (cols - 1.0) * gap) / cols;
        let tile_h = Self::MODULE_TILE_H * scale;
        let top = panel.y + Self::MODULE_TOP_H * scale;
        let reihen = self.module_rows_shown(gpu);
        self.module_menu_category
            .modules()
            .into_iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let (reihe, spalte) = (i / spalten, i % spalten);
                let &(_, n) = reihen.iter().find(|&&(r, _)| r == reihe)?;
                Some((id, Rect { x: x0 + spalte as f32 * (tile_w + gap), y: top + n * (tile_h + gap), w: tile_w, h: tile_h }))
            })
            .collect()
    }

    // An/Aus-Schalter rechts in einer Kachel, ueber ihre ganze Hoehe.
    pub(crate) fn module_tile_toggle_rect(tile: Rect, scale: f32) -> Rect {
        let rand = 3.0 * scale;
        Rect { x: tile.x + tile.w - 30.0 * scale - rand, y: tile.y + rand, w: 30.0 * scale, h: tile.h - 2.0 * rand }
    }

    // Die klickbaren Elemente des Modul-Menues. Eine Quelle fuer Zeichnen UND
    // Trefferpruefung, genau wie bei buttons() -- sonst laufen die beiden auseinander.
    pub(crate) fn module_menu_buttons(&self, gpu: &GpuState) -> Vec<Button> {
        let panel = self.module_menu_rect(gpu);
        let scale = self.module_scale(gpu);
        let mut out = Vec::new();

        let zu = if self.module_menu_detail {
            Self::module_detail_close_rect(panel)
        } else {
            Rect { x: panel.x + panel.w - 50.0 * scale, y: panel.y + 4.0 * scale, w: 44.0 * scale, h: 16.0 * scale }
        };
        // Als Einstellungsseite gibt es nichts zu schliessen -- dafuer ist die Leiste links da.
        if self.module_menu_detail || self.module_settings_area(gpu).is_none() {
            out.push(Self::nav_button(ButtonAction::CloseModuleMenu, zu, "Schließen", UI_SCALE_SMALL));
        }

        if !self.module_menu_detail {
            // Kategorien links: eine Zeile je Kategorie, die gewaehlte hervorgehoben.
            for (i, &kategorie) in ModuleCategory::all().iter().enumerate() {
                out.push(Button {
                    action: ButtonAction::SelectModuleCategory(kategorie),
                    rect: Rect {
                        x: panel.x + 4.0 * scale,
                        y: panel.y + (Self::MODULE_TOP_H + i as f32 * Self::MODULE_ROW_H) * scale,
                        w: (Self::MODULE_CAT_W - 4.0) * scale,
                        // Volle Zeilenhoehe zum Treffen, die Luecke kommt erst beim Zeichnen.
                        h: Self::MODULE_ROW_H * scale,
                    },
                    label: String::new(),
                    enabled: true,
                    // Surface2 wie die Knoepfe der Einstellungsseiten: nur dafuer leuchtet die
                    // Flaeche beim Hover auf (s. styled_button).
                    color: if kategorie == self.module_menu_category { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                    scale: UI_SCALE_SMALL,
                    slider: None,
                });
            }
            // Kacheln: der Schalter ZUERST, damit er vor der Kachelflaeche getroffen wird
            // (die Trefferpruefung nimmt den ersten passenden Knopf).
            for (id, tile) in self.module_tiles(gpu) {
                let on = id.enabled(&self.settings);
                out.push(Button {
                    action: id.toggle_action(),
                    rect: Self::module_tile_toggle_rect(tile, scale),
                    label: if on { "AN" } else { "AUS" }.to_string(),
                    enabled: true,
                    color: if on { theme::get(theme::Slot::AccentSurface) } else { theme::get(theme::Slot::Surface2) },
                    scale: UI_SCALE_SMALL,
                    slider: None,
                });
                out.push(Button {
                    action: ButtonAction::SelectModule(id),
                    rect: tile,
                    label: String::new(),
                    enabled: true,
                    color: theme::get(theme::Slot::Surface2),
                    scale: UI_SCALE_SMALL,
                    slider: None,
                });
            }
            return out;
        }

        // Detailansicht: Zurueck-Pfeil oben links, darunter die Einstellungen des Moduls. Die
        // Bedienelemente eines abgeschalteten Moduls bleiben sichtbar, nehmen aber keine
        // Klicks an (s. Button::enabled).
        out.push(Self::nav_button(ButtonAction::ModuleBack, Self::module_detail_back_rect(panel), "←", UI_SCALE_SMALL));
        let layout = self.module_detail_layout(gpu);
        let enabled = self.module_menu_selected.enabled(&self.settings);
        // Der Hauptschalter ist die EINE Ausnahme von `enabled`: er muss auch dann gehen,
        // wenn das Modul aus ist -- sonst gaebe es keinen Weg zurueck (s. module_master_row).
        let master = self.module_menu_selected.toggle_action();
        let mut rows: Vec<Option<ModuleRow>> = self.module_detail_rows().into_iter().map(Some).collect();
        for (k, n) in self.module_rows_shown(gpu) {
            let Some(row) = rows.get_mut(k).and_then(Option::take) else { continue };
            match row {
                ModuleRow::Toggle { action, on, .. } => out.push(Button {
                    rect: layout.control_rect(n),
                    enabled: enabled || action == master,
                    ..Self::toggle(action, 0, on)
                }),
                ModuleRow::Stepper { prev, next, slider: Some((wert, min, max)), .. } => {
                    out.push(Self::step_slider(prev, next, layout.value_rect(n), wert, min, max, enabled));
                }
                ModuleRow::Stepper { prev, next, can_prev, can_next, slider: None, .. } => {
                    let [down, up] = Self::stepper_at(prev, next, layout.value_rect(n), can_prev, can_next);
                    out.push(Button { enabled: down.enabled && enabled, ..down });
                    out.push(Button { enabled: up.enabled && enabled, ..up });
                }
                // Das Farbfeld IST der Knopf: ein Klick klappt den Farbeditor auf bzw. wieder
                // zu. Die Beschriftung bleibt leer, damit build_module_menu es als Farbflaeche
                // statt als Textknopf zeichnet.
                ModuleRow::Swatch { action, .. } => out.push(Button {
                    action,
                    rect: layout.swatch_rect(n),
                    label: String::new(),
                    enabled,
                    color: theme::get(theme::Slot::Surface2),
                    scale: UI_SCALE_SMALL,
                    slider: None,
                }),
                ModuleRow::Choice { action, text, .. } => out.push(Button {
                    action,
                    rect: layout.choice_rect(n),
                    label: text,
                    enabled,
                    color: theme::get(theme::Slot::Surface2),
                    scale: UI_SCALE_SMALL,
                    slider: None,
                }),
                ModuleRow::Pick { options, swatch, .. } => {
                    out.extend(pick_buttons(layout.choice_rect(n), &options, enabled));
                    if let Some((action, _)) = swatch {
                        out.push(Button {
                            action,
                            rect: layout.swatch_after_choice_rect(n),
                            label: String::new(),
                            enabled,
                            color: theme::get(theme::Slot::Surface2),
                            scale: UI_SCALE_SMALL,
                            slider: None,
                        });
                    }
                }
                ModuleRow::ColorPicks { first, colors, .. } => {
                    out.extend(pick_buttons(layout.color_picks_first_rect(n), &[first], enabled));
                    for (i, (action, _, gewaehlt)) in colors.into_iter().enumerate() {
                        out.push(Button {
                            action: if gewaehlt { ButtonAction::Nothing } else { action },
                            rect: layout.color_pick_rect(n, i),
                            label: String::new(),
                            enabled,
                            color: theme::get(theme::Slot::Surface2),
                            scale: UI_SCALE_SMALL,
                            slider: None,
                        });
                    }
                }
                ModuleRow::ToggleSwatch { action, on, swatch, .. } => {
                    out.push(Button { rect: layout.control_rect(n), enabled, ..Self::toggle(action, 0, on) });
                    out.push(Button {
                        action: swatch,
                        rect: layout.swatch_after_toggle_rect(n),
                        label: String::new(),
                        enabled,
                        color: theme::get(theme::Slot::Surface2),
                        scale: UI_SCALE_SMALL,
                        slider: None,
                    });
                }
                ModuleRow::TwoTogglesSwatch { first, second, swatch, .. } => {
                    // Der zweite Schalter und die Farbe wirken nur mit dem ersten.
                    let dahinter = enabled && first.1;
                    out.push(Button { rect: layout.control_rect(n), enabled, ..Self::toggle(first.0, 0, first.1) });
                    out.push(Button { rect: layout.second_toggle_rect(n), enabled: dahinter, ..Self::toggle(second.0, 0, second.1) });
                    out.push(Button {
                        action: swatch,
                        rect: layout.swatch_after_two_toggles_rect(n),
                        label: String::new(),
                        enabled: dahinter,
                        color: theme::get(theme::Slot::Surface2),
                        scale: UI_SCALE_SMALL,
                        slider: None,
                    });
                }
                ModuleRow::Heading { .. } => {}
            }
        }
        out
    }

    // Zeilen, die im aktuellen Zustand des Moduls nichts bewirken und darum ausgegraut sind:
    // die Uhrzeit des Himmels bei Einfarbig.
    pub(crate) fn module_row_greyed(&self, row: &ModuleRow) -> bool {
        let aktion = match row {
            ModuleRow::Toggle { action, .. }
            | ModuleRow::Choice { action, .. }
            | ModuleRow::Swatch { action, .. }
            | ModuleRow::ToggleSwatch { action, .. } => *action,
            ModuleRow::Pick { options, .. } => options.first().map_or(ButtonAction::Nothing, |o| o.1),
            ModuleRow::ColorPicks { first, .. } => first.1,
            ModuleRow::TwoTogglesSwatch { first, .. } => first.0,
            ModuleRow::Stepper { prev, .. } => *prev,
            ModuleRow::Heading { .. } => return false,
        };
        self.settings.sky_mode == SkyMode::Solid && matches!(aktion, ButtonAction::ToggleSkyFixedTime | ButtonAction::SkyTimePrev)
    }

    // Text auf eine Breite kuerzen ("..." am Ende), fuer die Kachelbeschriftungen.
    pub(crate) fn fit_text(gpu: &GpuState, text: &str, max_w: f32, scale: f32) -> String {
        // Gekuerzt wird der uebersetzte Text (s. i18n), sonst passte die Laenge nicht.
        let text = i18n::tr(text);
        if gpu.measure_raw(&text, scale) <= max_w {
            return text.into_owned();
        }
        let mut s: String = text.into_owned();
        while !s.is_empty() && gpu.measure_raw(&format!("{s}..."), scale) > max_w {
            s.pop();
        }
        format!("{}...", s.trim_end())
    }

    // Texte einer Einstellungsseite so einpassen, dass sie nichts anderes ueberdecken: Text in
    // einem Bedienelement (Knopf, Wertfeld) bleibt in dessen Rand, eine Beschriftung endet vor
    // dem naechsten Element rechts daneben (oder vor `rechts`). Erst eine Viertelstufe kleiner
    // (bleibt scharf), hoechstens bis drei Viertel, dann mit "..." gekuerzt.
    pub(crate) fn fit_texts(gpu: &GpuState, rects: &[UiRect], texts: &mut [UiText], rechts: f32) {
        // Grosse Flaechen (Tafeln, Zeilen-Hintergruende) zaehlen weder als Rahmen noch als Nachbar.
        let element = |r: &Rect| r.w < 600.0 && r.h <= 64.0;
        for t in texts.iter_mut() {
            let w = gpu.measure_text(&t.text, t.scale);
            let h = gpu.text_height(t.scale);
            let (mx, my) = (t.x + w / 2.0, t.y + h / 2.0);
            let rahmen = rects
                .iter()
                .map(|r| r.rect)
                .filter(|r| element(r) && r.h >= 16.0 && r.contains(mx, my))
                .min_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)));
            let (platz, mitte) = match rahmen {
                Some(r) => {
                    let mittig = (mx - (r.x + r.w / 2.0)).abs() < 2.0;
                    if mittig { (r.w - 12.0, Some(r)) } else { (r.x + r.w - 6.0 - t.x, None) }
                }
                None => {
                    let naechstes = rects
                        .iter()
                        .map(|r| r.rect)
                        .filter(|r| element(r) && r.h >= 4.0 && r.x > t.x + 4.0 && r.y < t.y + h && r.y + r.h > t.y)
                        .map(|r| r.x)
                        .fold(rechts, f32::min);
                    (naechstes - 12.0 - t.x, None)
                }
            };
            if w <= platz || platz < 24.0 {
                continue;
            }
            let mut scale = t.scale;
            while scale - 0.25 >= t.scale * 0.75 - 1e-3 && gpu.measure_text(&t.text, scale) > platz {
                scale -= 0.25;
            }
            if gpu.measure_text(&t.text, scale) > platz {
                t.text = Self::fit_text(gpu, &t.text, platz, scale);
            }
            t.y = (t.y + (h - gpu.text_height(scale)) / 2.0).round();
            t.scale = scale;
            if let Some(r) = mitte {
                t.x = (r.x + (r.w - gpu.measure_text(&t.text, scale)) / 2.0).round();
            }
        }
    }

    // Tafel des Modul-Menues zeichnen: Uebersicht (Kategorien + Kacheln) oder Detailansicht.
    pub(crate) fn build_module_menu(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let panel = self.module_menu_rect(gpu);
        let scale = self.module_scale(gpu);
        // Im Spiel abdunkeln wie hinter dem Inventar, mit derselben Einstellung -- ein Overlay,
        // zwei verschiedene Hintergruende waere ohne Grund inkonsistent. Als Einstellungsseite
        // weder das noch eine eigene Tafel: die Seite selbst ist der Hintergrund.
        if self.module_settings_area(gpu).is_none() {
            let dim = self.settings.inventory_transparency as f32 / 100.0;
            let (sw, sh) = gpu.size();
            rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.0, 0.0, 0.0, dim], ..UiRect::FLAT });
            rects.push(UiRect { rect: panel, color: theme::get(theme::Slot::BgPanel), ..UiRect::FLAT });
        }
        let buttons = self.module_menu_buttons(gpu);

        if !self.module_menu_detail {
            texts.push(UiText { text: "Module".to_string(), x: panel.x + 8.0 * scale, y: panel.y + 6.0 * scale, color: theme::text(), scale: UI_SCALE_SMALL });
            // Trennlinie zwischen Kategorien und Kacheln.
            let trenn_x = panel.x + Self::MODULE_CAT_W * scale + 2.0 * scale;
            rects.push(UiRect { rect: Rect { x: trenn_x, y: panel.y + Self::MODULE_TOP_H * scale, w: 1.0, h: panel.h - Self::MODULE_TOP_H * scale }, color: COLOR_GRID_LINE, ..UiRect::FLAT });
            for button in &buttons {
                if let ButtonAction::SelectModuleCategory(kategorie) = button.action {
                    // Die gewaehlte wie der aktive Eintrag der Seitenleiste (Theme-Farbe).
                    // Rand und Hover wie die Knoepfe der Einstellungsseiten (styled_button).
                    let flaeche = Rect { h: button.rect.h - 2.0 * scale, ..button.rect };
                    let (px, py) = self.pointer();
                    let (stil, schrift) = styled_button(button, self.hover_level(button.rect, button.hits(px, py)));
                    rects.push(UiRect { rect: flaeche, ..stil });
                    texts.push(UiText {
                        text: Self::fit_text(gpu, kategorie.label(), button.rect.w - 6.0 * scale, UI_SCALE_SMALL),
                        x: button.rect.x + 6.0 * scale,
                        y: (flaeche.y + (flaeche.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round(),
                        color: schrift,
                        scale: UI_SCALE_SMALL,
                    });
                }
            }
            let (px, py) = self.pointer();
            for (id, tile) in self.module_tiles(gpu) {
                let on = id.enabled(&self.settings);
                // Wie ein Knopf der Einstellungsseiten: Rand, beim Hover heller (s. styled_button).
                // Eingeschaltete mit Akzentrand -- man sieht den Stand der ganzen Kategorie auf
                // einen Blick.
                let k = self.hover_level(tile, tile.contains(px, py));
                let g = theme::get;
                let flaeche = mix_color(g(theme::Slot::Surface2), g(theme::Slot::Surface3), k);
                let rand = if on { g(theme::Slot::Accent) } else { mix_color(g(theme::Slot::Border2), g(theme::Slot::BorderStrong), k.min(1.0)) };
                rects.push(UiRect::framed(tile, flaeche, 6.0, 1.0, rand));
                let text_x = tile.x + 6.0 * scale;
                let schalter = Self::module_tile_toggle_rect(tile, scale);
                // Nur der Name, mittig in der Hoehe: die Beschreibung passte nie ganz hinein und
                // steht in der Detailansicht ohnehin.
                let name = Self::fit_text(gpu, id.label(), schalter.x - text_x - 2.0 * scale, UI_SCALE_SMALL);
                let y = (tile.y + (tile.h - gpu.text_height(UI_SCALE_SMALL)) / 2.0).round();
                texts.push(UiText { text: name, x: text_x, y, color: theme::text(), scale: UI_SCALE_SMALL });
            }
            if self.module_menu_category.modules().is_empty() {
                texts.push(UiText {
                    text: "Keine Module".to_string(),
                    x: panel.x + (Self::MODULE_CAT_W + Self::MODULE_PAD) * scale,
                    y: panel.y + Self::MODULE_TOP_H * scale,
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
            }
            let top = panel.y + Self::MODULE_TOP_H * scale;
            let hoehe = self.module_visible_rows(gpu) as f32 * (Self::MODULE_TILE_H + Self::MODULE_TILE_GAP) * scale;
            self.draw_module_scrollbar(gpu, panel, top, hoehe, rects);
        } else {
            let layout = self.module_detail_layout(gpu);
            let th = gpu.text_height(UI_SCALE_SMALL);
            // Kopf: Name rechts neben dem Zurueck-Pfeil, darunter die Kurzbeschreibung, dann eine
            // Linie wie unter den Reitern der Einstellungsseiten.
            let zurueck = Self::module_detail_back_rect(panel);
            let zu = Self::module_detail_close_rect(panel);
            let name_x = zurueck.x + zurueck.w + 16.0;
            texts.push(UiText {
                text: Self::fit_text(gpu, self.module_menu_selected.label(), zu.x - 16.0 - name_x, UI_SCALE_SMALL),
                x: name_x,
                y: (zurueck.y + (zurueck.h - th) / 2.0).round(),
                color: theme::text(),
                scale: UI_SCALE_SMALL,
            });
            let innen_w = panel.w - 2.0 * MODULE_DETAIL_PX_PAD;
            texts.push(UiText {
                text: Self::fit_text(gpu, self.module_menu_selected.description(), innen_w, UI_SCALE_SMALL),
                x: panel.x + MODULE_DETAIL_PX_PAD,
                y: zurueck.y + zurueck.h + 12.0,
                color: theme::text_2(),
                scale: UI_SCALE_SMALL,
            });
            rects.push(UiRect {
                rect: Rect { x: panel.x + MODULE_DETAIL_PX_PAD, y: layout.top - 14.0, w: innen_w, h: 1.0 },
                color: theme::get(theme::Slot::Border1),
                ..UiRect::FLAT
            });
            let enabled = self.module_menu_selected.enabled(&self.settings);
            let (px, py) = self.pointer();
            let rows = self.module_detail_rows();
            for (k, n) in self.module_rows_shown(gpu) {
                let Some(row) = rows.get(k) else { continue };
                // Unterkategorie wie ein Abschnitt der Einstellungen: Linie ueber die ganze
                // Breite, der Name rechts darueber; darunter die Spaltenkoepfe, je mittig ueber
                // ihrem Schalter.
                if let ModuleRow::Heading { label, columns } = row {
                    let rechts = panel.x + panel.w - MODULE_DETAIL_PX_PAD;
                    let linie_y = (layout.row_y(n) + 16.0).round();
                    rects.push(UiRect { rect: Rect { x: layout.x, y: linie_y, w: rechts - layout.x, h: 1.0 }, color: theme::get(theme::Slot::Border1), ..UiRect::FLAT });
                    let th = gpu.text_height(UI_SCALE_SMALL);
                    let name_x = rechts - gpu.measure_text(label, UI_SCALE_SMALL);
                    texts.push(UiText { text: label.clone(), x: name_x.round(), y: (linie_y - 4.0 - th).round(), color: theme::text(), scale: UI_SCALE_SMALL });
                    for (i, kopf) in columns.iter().enumerate() {
                        let spalte = Rect { x: layout.ctrl_x() + i as f32 * (TOGGLE_W + STEPPER_GAP), y: linie_y + 8.0, w: TOGGLE_W, h: th };
                        texts.push(gpu.centered_text(spalte, kopf.to_string(), theme::text(), UI_SCALE_SMALL));
                    }
                    continue;
                }
                texts.push(UiText {
                    text: Self::fit_text(gpu, row.label(), layout.ctrl_x() - 12.0 - layout.x, UI_SCALE_SMALL),
                    x: layout.x,
                    y: layout.label_y(gpu, n),
                    color: theme::text(),
                    scale: UI_SCALE_SMALL,
                });
                match row {
                    // Regler wie in den Einstellungen: Bahn mit Daumen, die Zahl im Feld rechts,
                    // die Einheit dahinter. Klick und Ziehen laufen ueber module_menu_buttons.
                    ModuleRow::Stepper { prev, next, text, slider: Some((wert, min, max)), .. } => {
                        let Some(sl) = Self::step_slider(*prev, *next, layout.value_rect(n), *wert, *min, *max, enabled).slider else { continue };
                        let (kasten, zahl) = push_slider_unit(gpu, sl.value_box(), text, texts);
                        rects.push(UiRect::framed(kasten, theme::get(theme::Slot::SurfaceSunken), 6.0, 1.0, theme::get(theme::Slot::Border1)));
                        match &self.editing_step {
                            Some((p, buf)) if p == prev => self.push_value_input(gpu, kasten, zahl, buf, rects, texts),
                            _ => texts.push(gpu.centered_text(kasten, zahl, theme::text(), UI_SCALE_SMALL)),
                        }
                        let t = ((wert - min) / (max - min).max(1e-9)) as f32;
                        let aktiv = self.step_drag == Some(*prev);
                        let hover = sl.rect().contains(px, py);
                        Self::push_slider_visual(rects, sl.track(), t, hover || aktiv, aktiv, enabled);
                    }
                    // Auswahl mit Pfeilen: der Wert im Feld zwischen ihnen.
                    ModuleRow::Stepper { text, slider: None, .. } => {
                        let rect = layout.value_rect(n);
                        rects.push(UiRect { rect, color: theme::get(theme::Slot::SurfaceSunken), ..UiRect::FLAT });
                        texts.push(gpu.centered_text(rect, Self::fit_text(gpu, text, rect.w - 12.0, UI_SCALE_SMALL), theme::text(), UI_SCALE_SMALL));
                    }
                    // Farbfeld wie ueberall: die Farbe mit schwarzem Rand; ein Klick oeffnet den
                    // Farbeditor (s. color_target_of).
                    ModuleRow::Swatch { rgb, .. } | ModuleRow::ToggleSwatch { rgb, .. } | ModuleRow::TwoTogglesSwatch { rgb, .. } => {
                        let rect = match row {
                            ModuleRow::Swatch { .. } => layout.swatch_rect(n),
                            ModuleRow::ToggleSwatch { .. } => layout.swatch_after_toggle_rect(n),
                            _ => layout.swatch_after_two_toggles_rect(n),
                        };
                        // Ohne den ersten Schalter wirkt die Farbe nicht: abgeblendet, kein Hover.
                        let wirkt = !matches!(row, ModuleRow::TwoTogglesSwatch { first: (_, false), .. });
                        let rand = if wirkt && rect.contains(px, py) { theme::text_2() } else { [0.0, 0.0, 0.0, 1.0] };
                        let deckung = if wirkt { 1.0 } else { 0.3 };
                        rects.push(UiRect::framed(rect, [rgb[0], rgb[1], rgb[2], deckung], 6.0, 2.0, rand));
                    }
                    // Farbfeld neben der Auswahl: voll nur mit dem letzten Wert (Einfarbig).
                    ModuleRow::Pick { options, swatch: Some((_, rgb)), .. } => {
                        let rect = layout.swatch_after_choice_rect(n);
                        let wirkt = options.last().is_some_and(|o| o.2);
                        let rand = if rect.contains(px, py) { theme::text_2() } else { [0.0, 0.0, 0.0, 1.0] };
                        rects.push(UiRect::framed(rect, [rgb[0], rgb[1], rgb[2], if wirkt { 1.0 } else { 0.3 }], 6.0, 2.0, rand));
                    }
                    // Die Farbfelder zur Wahl: der gewaehlte mit Akzentrahmen, der Rest mit
                    // schwarzem (Hover hell).
                    ModuleRow::ColorPicks { colors, .. } => {
                        for (i, (_, rgb, gewaehlt)) in colors.iter().enumerate() {
                            let rect = layout.color_pick_rect(n, i);
                            let rand = if *gewaehlt {
                                theme::get(theme::Slot::Accent)
                            } else if rect.contains(px, py) {
                                theme::text_2()
                            } else {
                                [0.0, 0.0, 0.0, 1.0]
                            };
                            rects.push(UiRect::framed(rect, [rgb[0], rgb[1], rgb[2], 1.0], 6.0, if *gewaehlt { 3.0 } else { 2.0 }, rand));
                        }
                    }
                    ModuleRow::Toggle { .. } | ModuleRow::Choice { .. } | ModuleRow::Pick { .. } | ModuleRow::Heading { .. } => {}
                }
            }
            self.draw_module_scrollbar(gpu, panel, layout.top, self.module_visible_rows(gpu) as f32 * SETTINGS_ROW_H, rects);
        }

        // Knoepfe mit Beschriftung (Zu, Zurueck, Schalter, Pfeile). Die ohne Beschriftung
        // (Kategorien, Kacheln, Farbfelder) haben sich oben schon selbst gezeichnet.
        for button in buttons {
            if button.label.is_empty() {
                continue;
            }
            let (px, py) = self.pointer();
            let (flaeche, color) = styled_button(&button, self.hover_level(button.rect, button.hits(px, py)));
            rects.push(flaeche);
            if let Some(c) = Self::choice_of(button.action) {
                let offen = self.dropdown_active().is_some_and(|d| d.choice == c);
                self.push_dropdown_face(gpu, button.rect, &button.label, color, button.scale, offen, rects, texts);
                continue;
            }
            let label = Self::fit_text(gpu, &button.label, button.rect.w - 12.0, button.scale);
            texts.push(gpu.centered_text(button.rect, label, color, button.scale));
        }
    }

    // Scrollbalken am rechten Rand der Tafel, sobald nicht alles gleichzeitig passt --
    // dieselbe Optik wie in der Tastenbelegungs-Tabelle (s. build_settings_controls_tab).
    pub(crate) fn draw_module_scrollbar(&self, gpu: &GpuState, panel: Rect, top: f32, hoehe: f32, rects: &mut Vec<UiRect>) {
        let max_scroll = self.module_max_scroll(gpu);
        if max_scroll == 0 {
            return;
        }
        let scale = self.module_scale(gpu);
        let scroll = self.module_menu_scroll.stand;
        let bar = Rect { x: panel.x + panel.w - 4.0 * scale, y: top, w: 3.0 * scale, h: hoehe };
        rects.push(UiRect { rect: bar, color: theme::get(theme::Slot::Surface1), ..UiRect::FLAT });
        let anteil = self.module_visible_rows(gpu) as f32 / self.module_total_rows(gpu).max(1) as f32;
        let thumb_h = (bar.h * anteil).max(6.0 * scale);
        let thumb_y = bar.y + (bar.h - thumb_h) * (scroll / max_scroll as f32).clamp(0.0, 1.0);
        let color = if scroll > 0.5 { theme::accent() } else { theme::get(theme::Slot::Border2) };
        rects.push(UiRect { rect: Rect { y: thumb_y, h: thumb_h, ..bar }, color, ..UiRect::FLAT });
    }

    // Ein Klick im Modul-Menue. Getrennt von der Menue-Klick-Behandlung, weil die Knoepfe
    // hier aus module_menu_buttons kommen und nicht aus buttons().
    pub(crate) fn apply_module_action(&mut self, action: ButtonAction) {
        match action {
            ButtonAction::ToggleAddonModule(_) | ButtonAction::AddonToggle(..) | ButtonAction::AddonStep(..) | ButtonAction::AddonButton(..) => self.addon_action(action),
            // Modulwechsel setzt Scroll und offenen Farbeditor zurueck: beide gehoeren zum
            // vorherigen Modul und ergaeben im neuen keinen Sinn.
            // Kachel angeklickt: dessen Einstellungen fuellen das Fenster.
            ButtonAction::SelectModule(id) => {
                self.module_menu_selected = id;
                self.module_menu_detail = true;
                self.module_menu_scroll = Default::default();
                self.shield_color_editor = None;
            }
            ButtonAction::SelectModuleCategory(kategorie) => {
                self.module_menu_category = kategorie;
                self.module_menu_detail = false;
                self.module_menu_scroll = Default::default();
            }
            ButtonAction::ModuleBack => {
                self.module_menu_detail = false;
                self.module_menu_category = self.module_menu_selected.category();
                self.module_menu_scroll = Default::default();
            }
            // Oeffnet das Farbfenster, bevor es hier ankaeme (s. color_target_of).
            ButtonAction::HitboxColorEditor(_) => {}
            ButtonAction::ToggleModuleTotem => {
                self.settings.module_totem_indicator = !self.settings.module_totem_indicator;
                self.publish_totem_sound_enabled();
                // Modul aus heisst auch: eine gerade laufende Anzeige weg. Sonst bliebe sie
                // noch bis zu zwei Sekunden stehen, obwohl man sie gerade abgeschaltet hat.
                if !self.settings.module_totem_indicator {
                    self.totem_animation = None;
                }
            }
            ButtonAction::ToggleModuleTotemSound => {
                self.settings.module_totem_sound = !self.settings.module_totem_sound;
                self.publish_totem_sound_enabled();
            }
            ButtonAction::Toteming(action) => {
                // Die eigenen Dateien nur fuer das Weiterschalten des Klangs einlesen -- und
                // dabei den Zwischenspeicher leeren, damit eine ersetzte Datei neu klingt.
                let custom = if matches!(action, toteming::Action::Sound(..)) {
                    self.custom_pop_sounds.forget();
                    toteming::CustomSounds::scan()
                } else {
                    Vec::new()
                };
                self.settings.toteming.apply(action, &custom);
                self.publish_own_pop_plan();
                if let Some(slot) = action.sound_slot() {
                    self.preview_pop_sound(slot);
                }
            }
            ButtonAction::ToggleModuleHitbox => self.settings.module_hitbox_enabled = !self.settings.module_hitbox_enabled,
            ButtonAction::ToggleModuleHitboxSmoothed => {
                self.settings.module_hitbox_show_smoothed = !self.settings.module_hitbox_show_smoothed
            }
            ButtonAction::ToggleModuleHitboxRaw => self.settings.module_hitbox_show_raw = !self.settings.module_hitbox_show_raw,
            ButtonAction::ToggleModuleHitboxDotted => self.settings.module_hitbox_dotted = !self.settings.module_hitbox_dotted,
            ButtonAction::ToggleModuleHitboxItems => self.settings.module_hitbox_items = !self.settings.module_hitbox_items,
            ButtonAction::ToggleModuleHitboxSelf => self.settings.module_hitbox_self = !self.settings.module_hitbox_self,
            ButtonAction::ToggleHitboxKind(art) => {
                let st = &mut self.settings.module_hitbox_styles[art.index()];
                st.on = !st.on;
            }
            ButtonAction::ToggleHitboxKindOverride(art) => {
                let st = &mut self.settings.module_hitbox_styles[art.index()];
                st.custom = !st.custom;
            }
            ButtonAction::HitboxDensityPrev => {
                self.settings.module_hitbox_density_px =
                    self.settings.module_hitbox_density_px.saturating_sub(HITBOX_DENSITY_STEP).max(HITBOX_DENSITY_MIN)
            }
            ButtonAction::HitboxDensityNext => {
                self.settings.module_hitbox_density_px =
                    (self.settings.module_hitbox_density_px + HITBOX_DENSITY_STEP).min(HITBOX_DENSITY_MAX)
            }
            ButtonAction::HitboxOpacityPrev => self.settings.module_hitbox_opacity = self.settings.module_hitbox_opacity.saturating_sub(5),
            ButtonAction::HitboxOpacityNext => self.settings.module_hitbox_opacity = (self.settings.module_hitbox_opacity + 5).min(100),
            ButtonAction::ToggleModuleSkyColor => self.settings.module_sky_color_enabled = !self.settings.module_sky_color_enabled,
            ButtonAction::ToggleModuleGhostTolerance => {
                self.settings.module_ghost_tolerance = !self.settings.module_ghost_tolerance
            }
            ButtonAction::ToggleModuleCrystalOptimizer => {
                self.settings.module_crystal_optimizer = !self.settings.module_crystal_optimizer;
                // Eingeschaltet waehrend einer Verbindung: melden, wie die Mod es beim Beitreten tut.
                if self.settings.module_crystal_optimizer && self.conn_status == ConnStatus::Connected {
                    self.announce_crystal_optimizer();
                }
            }
            ButtonAction::ToggleModuleNoToolBreak => self.settings.module_no_tool_break = !self.settings.module_no_tool_break,
            ButtonAction::ToggleModuleBotcheckPass => self.settings.module_botcheck_pass = !self.settings.module_botcheck_pass,
            ButtonAction::ToggleModuleDynamicFire => self.settings.module_dynamic_fire = !self.settings.module_dynamic_fire,
            ButtonAction::ToggleModuleZoom => self.settings.module_zoom = !self.settings.module_zoom,
            ButtonAction::ToggleModuleVoiceChat => {
                self.settings.module_voice_chat = !self.settings.module_voice_chat;
                self.voice_settings_changed();
            }
            ButtonAction::VoiceStep(knob, hoch) => self.voice_step(knob, hoch),
            ButtonAction::VoiceFlag(flag) => self.voice_flag(flag),
            ButtonAction::ZoomFactorPrev => {
                self.settings.zoom_factor_tenths = self.settings.zoom_factor_tenths.saturating_sub(ZOOM_FACTOR_STEP).max(ZOOM_FACTOR_MIN)
            }
            ButtonAction::ZoomFactorNext => self.settings.zoom_factor_tenths = (self.settings.zoom_factor_tenths + ZOOM_FACTOR_STEP).min(ZOOM_FACTOR_MAX),
            ButtonAction::ZoomSensPrev => self.settings.zoom_sens_percent = self.settings.zoom_sens_percent.saturating_sub(ZOOM_SENS_STEP).max(ZOOM_SENS_MIN),
            ButtonAction::ZoomSensNext => self.settings.zoom_sens_percent = (self.settings.zoom_sens_percent + ZOOM_SENS_STEP).min(ZOOM_SENS_MAX),
            ButtonAction::ToggleModuleShields => self.settings.module_shields = !self.settings.module_shields,
            ButtonAction::ToggleModuleUseTimer => self.settings.module_use_timer = !self.settings.module_use_timer,
            ButtonAction::EatHoldThroughPrev => self.settings.eat_hold_through_ms = self.settings.eat_hold_through_ms.saturating_sub(EAT_HOLD_THROUGH_STEP),
            ButtonAction::ToggleEatSlotLock => self.settings.eat_slot_lock = !self.settings.eat_slot_lock,
            ButtonAction::ToggleEatAggressivePrediction => self.settings.eat_aggressive_prediction = !self.settings.eat_aggressive_prediction,
            ButtonAction::EatHoldThroughNext => {
                self.settings.eat_hold_through_ms = (self.settings.eat_hold_through_ms + EAT_HOLD_THROUGH_STEP).min(EAT_HOLD_THROUGH_MAX)
            }
            ButtonAction::ToggleModuleConsumableOptimizer => {
                self.settings.module_consumable_optimizer = !self.settings.module_consumable_optimizer;
                if self.settings.module_consumable_optimizer && self.conn_status == ConnStatus::Connected {
                    self.announce_consumable_optimizer();
                }
                if !self.settings.module_consumable_optimizer {
                    self.consumable.stop_waiting();
                }
            }
            ButtonAction::ToggleModuleArrows => self.settings.module_arrows = !self.settings.module_arrows,
            ButtonAction::ToggleModuleArmorHud => self.settings.module_armor_hud = !self.settings.module_armor_hud,
            ButtonAction::ToggleModuleTeams => self.settings.module_teams = !self.settings.module_teams,
            ButtonAction::TeamToggle(seite) => {
                let st = &mut self.settings.team_hitboxes[seite.index()];
                st.on = !st.on;
            }
            ButtonAction::TeamMatch(i) => self.settings.team_match.toggle(i),
            ButtonAction::TeamOwnColor(c) => self.settings.team_own_color = c,
            ButtonAction::TeamNametag(seite, nametag) => self.settings.team_hitboxes[seite.index()].nametag = nametag,
            ButtonAction::TeamOpacity(seite, mehr) => {
                let st = &mut self.settings.team_hitboxes[seite.index()];
                st.opacity = if mehr { (st.opacity + 5).min(100) } else { st.opacity.saturating_sub(5) };
            }
            ButtonAction::TeamDensity(seite, mehr) => {
                let st = &mut self.settings.team_hitboxes[seite.index()];
                st.density_px = if mehr {
                    (st.density_px + HITBOX_DENSITY_STEP).min(HITBOX_DENSITY_MAX)
                } else {
                    st.density_px.saturating_sub(HITBOX_DENSITY_STEP).max(HITBOX_DENSITY_MIN)
                };
            }
            ButtonAction::TeamDotted(seite) => {
                let st = &mut self.settings.team_hitboxes[seite.index()];
                st.dotted = !st.dotted;
            }
            ButtonAction::ToggleModuleKeystrokes => self.settings.module_keystrokes = !self.settings.module_keystrokes,
            ButtonAction::ToggleModuleSessionInfo => self.settings.module_session_info = !self.settings.module_session_info,
            ButtonAction::ToggleSessionLine(bit) => self.settings.session_lines ^= bit,
            ButtonAction::ResetSession => self.session.reset(Instant::now()),
            ButtonAction::ToggleArrowIndicator => self.settings.arrow_indicator = !self.settings.arrow_indicator,
            ButtonAction::ToggleArrowNumberOnly => self.settings.arrow_number_only = !self.settings.arrow_number_only,
            ButtonAction::ToggleArrowWhite => self.settings.arrow_white = !self.settings.arrow_white,
            ButtonAction::ArrowOpacityPrev => {
                self.settings.arrow_opacity = self.settings.arrow_opacity.saturating_sub(ARROW_OPACITY_STEP).max(ARROW_OPACITY_MIN)
            }
            ButtonAction::ArrowOpacityNext => self.settings.arrow_opacity = (self.settings.arrow_opacity + ARROW_OPACITY_STEP).min(100),
            ButtonAction::ToggleArrowTrail => self.settings.arrow_trail = !self.settings.arrow_trail,
            ButtonAction::TrailHoldPrev => self.settings.trail_hold_tenths = self.settings.trail_hold_tenths.saturating_sub(TRAIL_TENTHS_STEP),
            ButtonAction::TrailHoldNext => {
                self.settings.trail_hold_tenths = (self.settings.trail_hold_tenths + TRAIL_TENTHS_STEP).min(TRAIL_HOLD_MAX_TENTHS)
            }
            ButtonAction::TrailFadePrev => self.settings.trail_fade_tenths = self.settings.trail_fade_tenths.saturating_sub(TRAIL_TENTHS_STEP),
            ButtonAction::TrailFadeNext => {
                self.settings.trail_fade_tenths = (self.settings.trail_fade_tenths + TRAIL_TENTHS_STEP).min(TRAIL_FADE_MAX_TENTHS)
            }
            ButtonAction::ToggleShieldCdSelf => self.settings.shield_cd_self = !self.settings.shield_cd_self,
            ButtonAction::ToggleShieldCdPlayers => self.settings.shield_cd_players = !self.settings.shield_cd_players,
            ButtonAction::ToggleShieldHoldPlayers => self.settings.shield_hold_players = !self.settings.shield_hold_players,
            ButtonAction::ShieldColorEditor(farbe) => {
                self.shield_color_editor = if self.shield_color_editor == Some(farbe) { None } else { Some(farbe) };
            }
            ButtonAction::CycleShieldMark(farbe) => {
                let neu = match self.settings.shield_mark(farbe) {
                    ShieldMark::Model => ShieldMark::Hitbox,
                    ShieldMark::Hitbox => ShieldMark::Model,
                };
                match farbe {
                    ShieldColor::Cooldown => self.settings.shield_cd_mark = neu.index(),
                    ShieldColor::Hold => self.settings.shield_hold_mark = neu.index(),
                }
            }
            ButtonAction::CycleShieldSelfMode => self.settings.shield_cd_self_mode ^= 1,
            ButtonAction::CycleShieldCdTextStyle => self.settings.shield_cd_text_style ^= 1,
            ButtonAction::CycleArrowTextStyle => self.settings.arrow_text_style ^= 1,
            ButtonAction::CycleArrowEmptyFeedback => {
                self.settings.arrow_empty_feedback = (self.settings.arrow_empty_feedback + 1) % ARROW_EMPTY_FEEDBACK_NAMES.len() as u8
            }
            ButtonAction::ToggleModuleShowMarkers => {
                self.settings.module_show_markers = !self.settings.module_show_markers;
                // Steckt im Mesh: die geladene Welt muss neu gebaut werden.
                for (cx, cz) in self.world.column_keys().collect::<Vec<_>>() {
                    self.world.mark_dirty(cx, cz);
                }
            }
            ButtonAction::ToggleModuleSoundDebug => self.settings.module_sound_debug = !self.settings.module_sound_debug,
            ButtonAction::ToggleModuleConnectionDebug => self.settings.module_connection_debug = !self.settings.module_connection_debug,
            ButtonAction::ToggleModuleDelay => {
                self.settings.module_delay = !self.settings.module_delay;
                self.sync_lag_delay();
            }
            ButtonAction::ToggleHud => self.settings.show_hud = !self.settings.show_hud,
            ButtonAction::ToggleModuleDiscordStatus => self.settings.module_discord_status = !self.settings.module_discord_status,
            ButtonAction::ToggleModuleInputLog => {
                self.settings.module_input_log_enabled = !self.settings.module_input_log_enabled;
                // Den Logger sofort mitziehen: einschalten beginnt beim naechsten Ereignis eine
                // neue Sitzung, ausschalten leert den Puffer und schliesst die Datei.
                self.input_log.set_enabled(self.settings.module_input_log_enabled);
            }
            ButtonAction::ToggleModuleColor => self.settings.module_color = !self.settings.module_color,
            ButtonAction::ToggleModuleSustainability => self.settings.module_sustainability = !self.settings.module_sustainability,
            ButtonAction::ToggleSustainBackground => self.settings.sustain_background = !self.settings.sustain_background,
            ButtonAction::ToggleSustainPause => self.settings.sustain_pause = !self.settings.sustain_pause,
            ButtonAction::ToggleModuleChunkSaver
            | ButtonAction::ToggleChunkSaverRenderSaved
            | ButtonAction::ChunkSaverThreadsPrev
            | ButtonAction::ChunkSaverThreadsNext
            | ButtonAction::ChunkSaverThresholdPrev
            | ButtonAction::ChunkSaverThresholdNext
            | ButtonAction::ChunkSaverDistancePrev
            | ButtonAction::ChunkSaverDistanceNext => lod::apply_action(&mut self.settings, action),
            ButtonAction::ColorSaturationPrev => {
                self.settings.color_saturation = self.settings.color_saturation.saturating_sub(COLOR_SATURATION_STEP)
            }
            ButtonAction::ColorSaturationNext => {
                self.settings.color_saturation = (self.settings.color_saturation + COLOR_SATURATION_STEP).min(COLOR_SATURATION_MAX)
            }
            ButtonAction::DiscordPasteClientId => {
                // Wechselt zwischen Vorgabe und eigener ID: steht eine eigene drin, fuehrt der
                // Klick zur Vorgabe zurueck; sonst kommt eine aus der Zwischenablage. Die ID der
                // Vorgabe selbst zaehlt als Vorgabe (leer), damit sie spaeteren Aenderungen folgt.
                if !self.settings.discord_client_id.is_empty() {
                    self.settings.discord_client_id.clear();
                } else {
                    let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
                    let id: String = text.trim().chars().filter(char::is_ascii_digit).collect();
                    if discord_rpc::valid_client_id(&id) && id != discord_rpc::DEFAULT_CLIENT_ID {
                        self.settings.discord_client_id = id;
                    }
                }
            }
            ButtonAction::ShieldColorPrev(farbe, achse) => adjust_shield_color(&mut self.settings, farbe, achse, -1),
            ButtonAction::ShieldColorNext(farbe, achse) => adjust_shield_color(&mut self.settings, farbe, achse, 1),
            ButtonAction::NoToolBreakReservePrev => {
                self.settings.no_tool_break_reserve = self.settings.no_tool_break_reserve.saturating_sub(1).max(NO_TOOL_BREAK_MIN_RESERVE)
            }
            ButtonAction::NoToolBreakReserveNext => {
                self.settings.no_tool_break_reserve = (self.settings.no_tool_break_reserve + 1).min(NO_TOOL_BREAK_MAX_RESERVE)
            }
            ButtonAction::ToggleModuleAutoclicker => self.settings.module_autoclicker = !self.settings.module_autoclicker,
            ButtonAction::ToggleModuleSaturationHud => self.settings.module_saturation_hud = !self.settings.module_saturation_hud,
            ButtonAction::SaturationHeightPrev => {
                self.settings.saturation_hud_height = self.settings.saturation_hud_height.saturating_sub(1).max(SATURATION_BAR_HEIGHT_MIN)
            }
            ButtonAction::SaturationHeightNext => self.settings.saturation_hud_height = (self.settings.saturation_hud_height + 1).min(SATURATION_BAR_HEIGHT_MAX),
            ButtonAction::SaturationAlpha(raster, hoch) => {
                let feld = if raster { &mut self.settings.saturation_hud_grid_alpha } else { &mut self.settings.saturation_hud_bg_alpha };
                *feld = if hoch { (*feld + 5).min(100) } else { feld.saturating_sub(5) };
            }
            ButtonAction::ToggleModuleEgoName => self.settings.module_ego_name = !self.settings.module_ego_name,
            ButtonAction::EditEgoAlias => {
                self.theme_name_dialog = Some(ThemeNameDialog {
                    field: text_field::TextField::new(&self.settings.ego_alias, EGO_ALIAS_MAX),
                    dragging: false,
                    last_press: None,
                    purpose: NameDialogPurpose::EgoAlias,
                });
            }
            ButtonAction::CycleEgoDeco => self.settings.ego_deco = self.settings.ego_deco.next(),
            ButtonAction::CycleEgoShadow => self.settings.ego_shadow = self.settings.ego_shadow.next(),
            ButtonAction::EgoLight(schatten, hoch) => {
                let feld = if schatten { &mut self.settings.ego_shadow_light } else { &mut self.settings.ego_light };
                *feld = if hoch { (*feld + 5).min(100) } else { feld.saturating_sub(5) };
            }
            ButtonAction::EgoSpeed(hoch) => {
                let v = self.settings.ego_speed;
                self.settings.ego_speed = if hoch { (v + 1).min(EGO_SPEED_MAX) } else { v.saturating_sub(1).max(EGO_SPEED_MIN) };
            }
            ButtonAction::CycleEgoSound => {
                self.settings.ego_sound = ego_name::next_sound(&self.settings.ego_sound).to_string();
                self.play_ego_sound();
            }
            ButtonAction::EgoVolume(hoch) => {
                let v = self.settings.ego_volume;
                self.settings.ego_volume = if hoch { v + 10.0 } else { (v - 10.0).max(0.0) };
            }
            ButtonAction::AutoclickerCpsPrev => self.settings.autoclicker_cps = self.settings.autoclicker_cps.saturating_sub(1).max(AUTOCLICKER_CPS_MIN),
            ButtonAction::AutoclickerCpsNext => self.settings.autoclicker_cps = (self.settings.autoclicker_cps + 1).min(AUTOCLICKER_CPS_MAX),
            // Das Minimum ist derzeit 0 (dann wirkungslos), soll aber eine Grenze bleiben.
            #[allow(clippy::unnecessary_min_or_max)]
            ButtonAction::GhostTolerancePrev => {
                self.settings.ghost_tolerance_ms =
                    self.settings.ghost_tolerance_ms.saturating_sub(GHOST_TOLERANCE_STEP_MS).max(GHOST_TOLERANCE_MIN_MS)
            }
            ButtonAction::LagDelayPrev => {
                self.settings.lag_delay_ms = self.settings.lag_delay_ms.saturating_sub(LAG_DELAY_STEP_MS);
                self.sync_lag_delay();
            }
            ButtonAction::LagDelayNext => {
                self.settings.lag_delay_ms = (self.settings.lag_delay_ms + LAG_DELAY_STEP_MS).min(LAG_DELAY_MAX_MS);
                self.sync_lag_delay();
            }
            ButtonAction::GhostToleranceNext => {
                self.settings.ghost_tolerance_ms = (self.settings.ghost_tolerance_ms + GHOST_TOLERANCE_STEP_MS).min(GHOST_TOLERANCE_MAX_MS)
            }
            ButtonAction::CycleSkyMode => self.settings.sky_mode = self.settings.sky_mode.next(),
            ButtonAction::ToggleSkySun => self.settings.sky_sun = !self.settings.sky_sun,
            ButtonAction::ToggleSkyMoon => self.settings.sky_moon = !self.settings.sky_moon,
            ButtonAction::ToggleSkyStars => self.settings.sky_stars = !self.settings.sky_stars,
            ButtonAction::ToggleSkyFixedTime => self.settings.module_sky_fixed_time = !self.settings.module_sky_fixed_time,
            ButtonAction::SkyTimePrev => adjust_sky_time(&mut self.settings, -1),
            ButtonAction::SkyTimeNext => adjust_sky_time(&mut self.settings, 1),
            ButtonAction::ToggleModuleWeather => self.settings.module_weather = !self.settings.module_weather,
            ButtonAction::CycleWeatherMode => self.settings.weather_mode = self.settings.weather_mode.next(),
            ButtonAction::WeatherRadiusPrev => {
                let set = &mut self.settings;
                set.weather_radius = set.weather_radius.saturating_sub(1).max(precipitation::RADIUS_MIN).max(set.weather_radius_inner + 1)
            }
            ButtonAction::WeatherRadiusInnerPrev => self.settings.weather_radius_inner = self.settings.weather_radius_inner.saturating_sub(1),
            ButtonAction::WeatherRadiusInnerNext => {
                let set = &mut self.settings;
                set.weather_radius_inner = (set.weather_radius_inner + 1).min(set.weather_radius.saturating_sub(1))
            }
            ButtonAction::WeatherRadiusNext => self.settings.weather_radius = (self.settings.weather_radius + 1).min(precipitation::RADIUS_MAX),
            ButtonAction::ToggleModuleBlockOutline => self.settings.module_block_outline = !self.settings.module_block_outline,
            // Oeffnet das Farbfenster, schon beim Klick (s. color_target_of).
            ButtonAction::BlockOutlineEditor => {}
            ButtonAction::BlockOutlineWidthPrev => {
                self.settings.block_outline_width_px = self.settings.block_outline_width_px.saturating_sub(1).max(BLOCK_OUTLINE_WIDTH_MIN)
            }
            ButtonAction::BlockOutlineWidthNext => {
                self.settings.block_outline_width_px = (self.settings.block_outline_width_px + 1).min(BLOCK_OUTLINE_WIDTH_MAX)
            }
            ButtonAction::ToggleBlockOutlineHideMining => self.settings.block_outline_hide_mining = !self.settings.block_outline_hide_mining,
            ButtonAction::SetBlockOutlineComplementary(an) => self.settings.block_outline_complementary = an,
            ButtonAction::CloseModuleMenu => self.module_menu_open = false,
            ButtonAction::OpenHudConfig(element) => {
                // Aus dem Spiel (Modul-Menue ueber der Welt) erst ins Menue, wie mit Esc.
                if self.mode == AppMode::World {
                    self.module_menu_open = false;
                    self.mode = AppMode::Menu;
                    self.screen = MenuScreen::Main;
                    self.clear_controls();
                }
                self.hud_editor_return = self.screen;
                self.screen = MenuScreen::HudEditor;
                self.hud_selected = Some(element);
                self.hud_drag_offset = None;
                self.crosshair_painter = None;
            }
            _ => {}
        }
    }

    // Ist die Modul-Tafel gerade zu sehen -- als Overlay im Spiel oder als Einstellungs-Screen?
    pub(crate) fn module_menu_shown(&self) -> bool {
        match self.mode {
            AppMode::World => self.module_menu_open,
            AppMode::Menu => self.screen == MenuScreen::SettingsModules,
        }
    }

    // Taste "Sprachchat: Einstellungen oeffnen": das Modul-Menue gleich auf dem Sprachchat;
    // ist es dort schon offen, schliesst ein zweiter Druck es wieder.
    pub(crate) fn open_voice_config(&mut self) {
        if self.mode != AppMode::World {
            return;
        }
        let schon_da = self.module_menu_open && self.module_menu_detail && self.module_menu_selected == ModuleId::VoiceChat;
        if !self.module_menu_open || schon_da {
            self.toggle_module_menu();
        }
        if self.module_menu_open {
            self.module_menu_category = ModuleId::VoiceChat.category();
            self.apply_module_action(ButtonAction::SelectModule(ModuleId::VoiceChat));
        }
    }

    pub(crate) fn toggle_module_menu(&mut self) {
        if self.mode != AppMode::World {
            return;
        }
        self.module_menu_open = !self.module_menu_open;
        self.play_ui_sound(if self.module_menu_open { ui_sound::UiSound::Open } else { ui_sound::UiSound::Close });
        if self.module_menu_open {
            // Zeiger in die Mitte, wie beim Oeffnen des Inventars.
            if let Some(gpu) = self.gpu.as_ref() {
                self.cursor_pos = (gpu.size().0 / 2.0, gpu.size().1 / 2.0);
            }
            // Gehaltene Tasten loslassen: das Overlay sperrt die Bewegungseingabe (s.
            // screen_overlay_open), und ein noch als gedrueckt gefuehrtes W wuerde beim
            // Schliessen als Dauerlauf weiterlaufen.
            self.clear_controls();
        } else {
            // Beim Schliessen: was noch gehalten wird, gilt wieder.
            self.restore_held_controls();
        }
    }
}
