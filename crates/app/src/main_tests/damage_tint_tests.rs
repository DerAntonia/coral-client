use super::*;

// Die Vorgabe muss EXAKT Vanilla sein, sonst faellt der Unterschied genau denen auf, die
// von Vanilla kommen. Vanillas Hurt-Zeile in der OverlayTexture ist ARGB(178, 255, 0, 0),
// und entity.fsh mischt mix(overlayColor.rgb, color.rgb, overlayColor.a) -- der Rot-Anteil
// ist also 1 - 178/255.
#[test]
fn vorgabe_ist_vanillas_rot_mit_vanillas_anteil() {
    let set = Settings::base();
    let [r, g, b, a] = set.damage_tint_rgba();
    assert!((r - 1.0).abs() < 1e-6, "reines Rot");
    assert!(g.abs() < 1e-6 && b.abs() < 1e-6, "kein Grün-/Blauanteil");
    let vanilla = 1.0 - 178.0 / 255.0;
    assert!((a - vanilla).abs() < 0.005, "Anteil {a} muss Vanillas {vanilla} entsprechen (auf ganze Prozent gerundet)");
}

// Die Mischung im Shader ist mix(base, tint, a) -- bei Anteil 0 bleibt die Textur
// unangetastet, bei 1 ist sie voll durchgefaerbt. Hier nachgerechnet, damit die Bedeutung
// von "Deckkraft" festgenagelt ist und nicht versehentlich invertiert wird.
#[test]
fn deckkraft_null_laesst_die_textur_unveraendert() {
    let mut set = Settings { damage_tint_alpha: 0, ..Default::default() };
    assert_eq!(set.damage_tint_rgba()[3], 0.0);
    set.damage_tint_alpha = 100;
    assert_eq!(set.damage_tint_rgba()[3], 1.0);
}

// Das Farbfeld der Schadens-Faerbung darf nicht in die Kamera-Gruppe darueber ragen.
#[test]
fn schadensfarbe_kollidiert_nicht_mit_kamera() {
    let kamera_zeilen: Vec<usize> = Perspective::ALL.iter().flat_map(|p| [p.distance_row(), p.y_offset_row()]).flatten().collect();
    for zeile in [DAMAGE_TINT_FIRST_ROW, DAMAGE_TINT_SKIN_ONLY_ROW, DAMAGE_TINT_TICKS_ROW, CRIT_COLOR_ROW] {
        assert!(!kamera_zeilen.contains(&zeile), "Zeile {zeile} kollidiert mit einer Kamerazeile");
    }
}

// Alle vier Regler ueber die Einstellungsdatei und zurueck -- ein vergessenes Feld im
// Speicher-Format faellt sonst erst nach einem Neustart auf.
#[test]
fn regler_ueberleben_speichern_und_laden() {
    let set = Settings {
        damage_tint_hue: 210,
        damage_tint_saturation: 45,
        damage_tint_value: 65,
        damage_tint_alpha: 75,
        ..Default::default()
    };
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.damage_tint_hue, 210);
    assert_eq!(geladen.damage_tint_saturation, 45);
    assert_eq!(geladen.damage_tint_value, 65);
    assert_eq!(geladen.damage_tint_alpha, 75);
}

// Kreuz-Farbe: Einfarbig samt Farbe ueberlebt Speichern und Laden, und die alten festen
// Modi (Weiss, Schwarz, Rot) werden zu Einfarbig in ihrer Farbe.
#[test]
fn kreuz_farbe_ueberlebt_speichern_und_laden() {
    let set = Settings { crosshair_color_mode: 1, crosshair_hue: 123, crosshair_saturation: 45, crosshair_value: 67, ..Default::default() };
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!((geladen.crosshair_color_mode, geladen.crosshair_hue, geladen.crosshair_saturation, geladen.crosshair_value), (1, 123, 45, 67));
    let alt = |modus: u32| {
        let s = Settings::parse_onto(Settings::base(), &format!("crosshair_color_mode={modus}\n"));
        (s.crosshair_color_mode, s.crosshair_hue, s.crosshair_saturation, s.crosshair_value)
    };
    assert_eq!(alt(0), (0, 0, 0, 100));
    assert_eq!(alt(1), (1, 0, 0, 100), "Weiss");
    assert_eq!(alt(2), (1, 0, 0, 0), "Schwarz");
    assert_eq!(alt(3), (1, 0, 100, 100), "Rot");
}

// Kontur des Rings: eigener Schalter; aeltere Dateien uebernehmen die Kontur des Kreuzes.
// Dazu der Blockumriss im Modus Komplementaer.
#[test]
fn ring_kontur_und_umriss_komplementaer_ueberleben_speichern_und_laden() {
    let set = Settings { crosshair_outline: true, crosshair_ring_outline: false, block_outline_complementary: true, ..Default::default() };
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert!(geladen.crosshair_outline && !geladen.crosshair_ring_outline && geladen.block_outline_complementary);
    assert!(Settings::parse_onto(Settings::base(), "crosshair_outline=true\n").crosshair_ring_outline, "alte Datei: Ring wie das Kreuz");
}

// Die eigenen Arten der Hitbox-Anzeige: Schalter, Farbe und Deckkraft je Art.
#[test]
fn hitbox_arten_ueberleben_speichern_und_laden() {
    use hitbox_kinds::{HitboxKind, HitboxStyle};
    let mut set = Settings::base();
    set.module_hitbox_styles[HitboxKind::EnderPearl.index()] = HitboxStyle { on: false, custom: true, hue: 12, saturation: 34, value: 56, alpha: 78, lines: 1 };
    set.module_hitbox_styles[HitboxKind::Water.index()].on = true;
    set.module_hitbox_player_raw = true;
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.module_hitbox_styles, set.module_hitbox_styles);
    assert!(geladen.module_hitbox_player_raw);
}

#[test]
fn schild_modul_einstellungen_ueberleben_speichern_und_laden() {
    let mut set = Settings::base();
    assert!(!set.module_shields, "Modul ist anfangs aus");
    set.module_shields = true;
    set.shield_cd_players = false;
    set.shield_hold_alpha = 60;
    set.shield_cd_hue = 120;
    set.hud_shield_cd_y = 0.7;
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert!(geladen.module_shields);
    assert!(!geladen.shield_cd_players);
    assert_eq!(geladen.shield_hold_alpha, 60);
    assert_eq!(geladen.shield_cd_hue, 120);
    assert!((geladen.hud_shield_cd_y - 0.7).abs() < 1e-6);
    // Die Cooldown-Farbe hat jetzt eine Deckkraft (fuer die Anzeige "Modell"); als
    // Hitbox-Farbe bekommt sie im Menue trotzdem keinen Regler (s. ShieldColor::axes).
    let vorher = set.shield_cd_alpha;
    adjust_shield_color(&mut set, ShieldColor::Cooldown, DamageTintAxis::Alpha, -1);
    assert!(set.shield_cd_alpha < vorher);
    assert!(!ShieldColor::axes(ShieldMark::Hitbox).contains(&DamageTintAxis::Alpha));
    // Ton laeuft um.
    set.shield_hold_hue = 0;
    adjust_shield_color(&mut set, ShieldColor::Hold, DamageTintAxis::Hue, -1);
    assert!(set.shield_hold_hue > 300);
    assert!(MODULE_IDS.contains(&ModuleId::Shields));
}

// Schild-Kreis: voller Kreis = alle Segmente, halber Kreis = halb so viele, mit Kontur
// doppelt; alles liegt auf dem Ring und passt in den Puffer.
#[test]
fn schild_kreis_segmente() {
    let voll = crosshair_ring_vertices((100.0, 100.0), SHIELD_RING_RADIUS_PX, 1.0, (200.0, 200.0), false);
    assert_eq!(voll.len(), SHIELD_RING_SEGMENTS * 6);
    assert_eq!(crosshair_ring_vertices((100.0, 100.0), SHIELD_RING_RADIUS_PX, 0.5, (200.0, 200.0), false).len(), SHIELD_RING_SEGMENTS / 2 * 6);
    let mit = crosshair_ring_vertices((100.0, 100.0), SHIELD_RING_RADIUS_PX, 1.0, (200.0, 200.0), true);
    assert_eq!(mit.len(), voll.len() * 2);
    assert!(24 + mit.len() <= CROSSHAIR_MAX_VERTICES);
    // Erste Ecke: innen, genau ueber der Mitte (12 Uhr).
    let [u, v] = voll[0].uv;
    let (x, y) = (u * 200.0, v * 200.0);
    assert!((x - 100.0).abs() < 1e-3 && (y - (100.0 - SHIELD_RING_RADIUS_PX + SHIELD_RING_THICKNESS_PX / 2.0)).abs() < 1e-3);
    // Eingestellter Radius: die Innenkante wandert mit.
    let gross = crosshair_ring_vertices((100.0, 100.0), 30.0, 1.0, (200.0, 200.0), false);
    assert!((gross[0].uv[1] * 200.0 - (100.0 - 30.0 + SHIELD_RING_THICKNESS_PX / 2.0)).abs() < 1e-3);
}

// Ring-Radius: Standard 11 px, Speicherrunde, Datei-Werte auf die Grenzen gekappt.
#[test]
fn ring_radius_einstellbar() {
    assert_eq!(Settings::base().crosshair_ring_radius, 11);
    let set = Settings { crosshair_ring_radius: 23, ..Settings::base() };
    assert_eq!(Settings::parse_onto(Settings::base(), &set.serialize()).crosshair_ring_radius, 23);
    assert_eq!(Settings::parse_onto(Settings::base(), "crosshair_ring_radius=999\n").crosshair_ring_radius, SHIELD_RING_RADIUS_MAX_PX);
    assert_eq!(Settings::parse_onto(Settings::base(), "crosshair_ring_radius=0\n").crosshair_ring_radius, SHIELD_RING_RADIUS_MIN_PX);
}

// Die neuen Anzeige-Einstellungen des Schild-Moduls ueberleben eine Speicherrunde.
#[test]
fn schild_anzeigen_werden_gespeichert() {
    let mut set = Settings::base();
    assert_eq!(set.shield_mark(ShieldColor::Cooldown), ShieldMark::Hitbox);
    assert_eq!(set.shield_mark(ShieldColor::Hold), ShieldMark::Model);
    set.shield_cd_mark = 0;
    set.shield_hold_mark = 1;
    set.shield_cd_self_mode = 1;
    set.area_font[ShadowArea::ShieldCountdown.index()] = 2;
    set.shield_cd_text_style = 1;
    set.arrow_text_style = 0;
    set.shield_cd_alpha = 70;
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.shield_mark(ShieldColor::Cooldown), ShieldMark::Model);
    assert_eq!(geladen.shield_mark(ShieldColor::Hold), ShieldMark::Hitbox);
    assert_eq!((geladen.shield_cd_self_mode, geladen.shield_cd_text_style, geladen.arrow_text_style), (1, 1, 0));
    assert_eq!(geladen.font_of(ShadowArea::ShieldCountdown), Some(FontKind::DmSans));
    // Alte Modul-Schriften werden als feste Bereichs-Schrift uebernommen.
    let alt = Settings::parse_onto(Settings::base(), "shield_cd_font=0\narrow_text_font=2\n");
    assert_eq!(alt.font_of(ShadowArea::ShieldCountdown), Some(FontKind::Minecraft));
    assert_eq!(alt.font_of(ShadowArea::ArrowNumber), Some(FontKind::Outfit));
    assert_eq!(geladen.shield_cd_alpha, 70);
    assert_eq!(ShieldColor::axes(ShieldMark::Hitbox).len(), 3);
    assert_eq!(ShieldColor::axes(ShieldMark::Model).len(), 4);
}

#[test]
fn sens_rechner_rechnet_wie_minecraft() {
    // 100 % = 0,5: d = 0,5, d^3*8*0,15 = 0,15 Grad je Zaehler.
    assert!((minecraft_sens_to_client(100.0).to_degrees() - 0.15).abs() < 1e-6);
    // 0 %: d = 0,2 -> 0,2^3*1,2 = 0,0096 Grad.
    assert!((minecraft_sens_to_client(0.0).to_degrees() - 0.0096).abs() < 1e-7);
    // 0,15 Grad je Zaehler bei 800 DPI: 2400 Zaehler = 3 Zoll = 7,62 cm.
    assert!((cm_per_360(0.15f32.to_radians(), 800) - 7.62).abs() < 1e-3);
    let set = Settings {
        sens_calc_mc_percent: 57.5,
        sens_calc_dpi: 1600,
        ..Default::default()
    };
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!((geladen.sens_calc_mc_percent, geladen.sens_calc_dpi), (57.5, 1600));
}

#[test]
fn schwung_startet_erst_ab_halber_dauer_neu() {
    let neu = PLAYER_SWING_TICKS + 1;
    assert_eq!(restart_swing(0, PLAYER_SWING_TICKS), neu, "kein Schwung: starten");
    assert_eq!(restart_swing(neu, PLAYER_SWING_TICKS), neu, "angestossen: bleibt angestossen");
    assert_eq!(restart_swing(PLAYER_SWING_TICKS, PLAYER_SWING_TICKS), PLAYER_SWING_TICKS, "erster Tick: weiterlaufen");
    assert_eq!(restart_swing(PLAYER_SWING_TICKS - 2, PLAYER_SWING_TICKS), PLAYER_SWING_TICKS - 2);
    assert_eq!(restart_swing(PLAYER_SWING_TICKS / 2, PLAYER_SWING_TICKS), neu, "halbe Dauer: neu");
    // Jeden Tick geschwungen (Abbauen): der Arm laeuft durch statt am Anfang zu kleben.
    let mut t = restart_swing(0, PLAYER_SWING_TICKS);
    let mut stand = Vec::new();
    for _ in 0..8 {
        t = t.saturating_sub(1);
        stand.push(t);
        t = restart_swing(t, PLAYER_SWING_TICKS);
    }
    assert_eq!(stand, [6, 5, 4, 3, 6, 5, 4, 3]);
}

#[test]
fn zoom_verengt_sichtfeld_und_sens() {
    assert_eq!(zoomed_fov(80.0, 40, 0.0), 80.0);
    assert!((zoomed_fov(80.0, 40, 1.0) - 20.0).abs() < 1e-4);
    assert!((zoomed_fov(80.0, 40, 0.5) - 40.0).abs() < 1e-3, "geometrisch: halber Weg = Wurzel der Stärke");
    assert_eq!(zoom_sens_factor(25, 0.0), 1.0);
    assert!((zoom_sens_factor(25, 1.0) - 0.25).abs() < 1e-6);
    let mut set = Settings::base();
    assert!(set.module_zoom);
    assert_eq!(set.keybinds.binds(KeyBindAction::Zoom).next(), Some(Bind::Key(KeyCode::KeyC)));
    set.zoom_factor_tenths = 65;
    set.zoom_sens_percent = 40;
    set.keybind_modes.set(KeyBindAction::Zoom, KeybindMode::Toggle);
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!((geladen.zoom_factor_tenths, geladen.zoom_sens_percent), (65, 40));
    assert!(geladen.keybind_modes.get(KeyBindAction::Zoom) == KeybindMode::Toggle);
}

#[test]
fn chat_blendet_nach_der_anzeigezeit_schnell_aus() {
    assert_eq!(chat_fade_alpha(9.0, 10), 1.0);
    assert!((chat_fade_alpha(10.0 - CHAT_FADE_OUT_SECS / 2.0, 10) - 0.5).abs() < 1e-4);
    assert_eq!(chat_fade_alpha(10.0, 10), 0.0);
    assert_eq!(chat_fade_alpha(1e6, 0), 1.0, "0 = nie ausblenden");
    assert_eq!(chat_fade_step(10, 1), 15);
    assert_eq!(chat_fade_step(10, -1), 7);
    assert_eq!(chat_fade_step(0, -1), 0);
    assert_eq!(chat_fade_step(120, 1), 120);
    assert_eq!((chat_fade_step(4, 1), chat_fade_step(4, -1)), (5, 3), "Zwischenwert rastet in Schrittrichtung ein");
    let mut set = Settings::base();
    assert_eq!(set.chat_fade_secs, 10);
    set.chat_fade_secs = 0;
    assert_eq!(Settings::parse_onto(Settings::base(), &set.serialize()).chat_fade_secs, 0);
}

#[test]
fn zeiger_in_slotmitte_ist_an_und_wird_gespeichert() {
    let mut set = Settings::base();
    assert!(set.inventory_cursor_to_slot);
    set.inventory_cursor_to_slot = false;
    assert!(!Settings::parse_onto(Settings::base(), &set.serialize()).inventory_cursor_to_slot);
}

#[test]
// Den Wind (Oeffnen/Schliessen) gibt es nur fuer Fenster -- ein Wechsel der
// Einstellungs-Kategorie oder Seite klickt nur.
fn fenster_rauschen_seitenwechsel_klicken() {
    use ui_sound::UiSound;
    let set = Settings::base();
    for fenster in [ButtonAction::OpenQuickConnect, ButtonAction::NewServer] {
        assert_eq!(ui_sound_for_action(fenster, &set), UiSound::Open);
    }
    assert_eq!(ui_sound_for_action(ButtonAction::CloseModuleMenu, &set), UiSound::Close);
    // Kontenliste und Element-Fenster im HUD-Editor klicken nur (Nutzerwunsch).
    assert_eq!(ui_sound_for_action(ButtonAction::ToggleAccountMenu, &set), UiSound::Click);
    assert_eq!(ui_sound_for_action(ButtonAction::HudDeselect, &set), UiSound::Click);
    for seite in [ButtonAction::OpenScreen(MenuScreen::SettingsAudio), ButtonAction::Back, ButtonAction::OpenParticleSettings(false)] {
        assert_eq!(ui_sound_for_action(seite, &set), UiSound::Click);
    }
}

#[test]
fn geloeschte_zeichen_fallen_an_ihrer_alten_stelle() {
    let run = |text: &str, x: f32, y: f32| TypedRun { text: text.to_string(), x, y, scale: 2.0, color: [1.0; 4], font: None, shadow: None };
    let breite = |r: &TypedRun, t: &str| t.chars().count() as f32 * 6.0 * r.scale;
    // Ruecktaste am Ende: das letzte Zeichen, hinter "hall".
    assert_eq!(erased_pieces(&[run("hallo", 10.0, 0.0)], &[run("hall", 10.0, 0.0)], breite, None, None), vec![run("o", 10.0 + 4.0 * 12.0, 0.0)]);
    // Mehrere Zeichen ueber eine Zeilengrenze (Chat bricht um): je Zeile ein Stueck.
    let alt = [run("abc", 0.0, 0.0), run("def", 0.0, 20.0)];
    let neu = [run("ab", 5.0, 0.0), run("f", 5.0, 20.0)];
    assert_eq!(erased_pieces(&alt, &neu, breite, None, None), vec![run("c", 24.0, 0.0), run("de", 0.0, 20.0)]);
    // Mitten im Wort, mit Umlauten (Byte- statt Zeichengrenzen waeren hier falsch).
    assert_eq!(erased_pieces(&[run("Grüße", 0.0, 0.0)], &[run("Grße", 0.0, 0.0)], breite, None, None), vec![run("ü", 24.0, 0.0)]);
    // Nichts geloescht, nur getippt: nichts faellt.
    assert!(erased_pieces(&[run("ab", 0.0, 0.0)], &[run("abc", 0.0, 0.0)], breite, None, None).is_empty());
}

// Wiederholter Text: welches "asd" weg ist, sagt erst der Cursor. Und ein Stueck am Cursor
// faellt buendig an ihm, auch wenn der Text davor noch gleitet (Chat, schnelles Loeschen).
#[test]
fn geloescht_wird_am_cursor() {
    let run = |text: &str, x: f32, y: f32| TypedRun { text: text.to_string(), x, y, scale: 1.0, color: [1.0; 4], font: None, shadow: None };
    let breite = |_: &TypedRun, t: &str| t.chars().count() as f32 * 10.0;
    let alt = [run("aasdasdasd", 0.0, 0.0)];
    let neu = [run("aasdasd", 0.0, 0.0)];
    // Markierung "asd" an Stelle 1..4, Cursor an ihrem Ende (x = 40).
    assert_eq!(erased_pieces(&alt, &neu, breite, Some((40.0, 0.0)), None), vec![run("asd", 10.0, 0.0)]);
    // Cursor am Ende: das letzte.
    assert_eq!(erased_pieces(&alt, &neu, breite, Some((100.0, 0.0)), None), vec![run("asd", 70.0, 0.0)]);
    // Der Text links vom Cursor stand noch 25 px zu weit links (gleitend): das Zeichen
    // faellt trotzdem direkt vor dem Cursor.
    let alt = [run("hallo", -25.0, 0.0)];
    assert_eq!(erased_pieces(&alt, &[run("hall", 0.0, 0.0)], breite, Some((50.0, 0.0)), None), vec![run("o", 40.0, 0.0)]);
    // Mausmarkierung "asd" an 1..4, der Cursor bleibt am Ende stehen: das Feld meldet die
    // Stelle selbst, und die Zeichen fallen dort, ohne an den Cursor zu ruecken.
    let alt = [run("aasdasdasd", 0.0, 0.0)];
    let neu = [run("aasdasd", 0.0, 0.0)];
    assert_eq!(erased_pieces(&alt, &neu, breite, Some((100.0, 0.0)), Some((1, 3, false))), vec![run("asd", 10.0, 0.0)]);
    // Eine Meldung, die nicht zum Text passt, wird nicht geglaubt.
    assert_eq!(erased_pieces(&alt, &neu, breite, Some((100.0, 0.0)), Some((0, 3, false))), vec![run("asd", 70.0, 0.0)]);
}

#[test]
fn kategorie_lautstaerke_wird_gespeichert_und_verrechnet() {
    use protocol::packets::SoundCategory as C;
    let mut set = Settings::base();
    set.master_volume = 50.5;
    let spieler = VOLUME_CATEGORIES.iter().position(|c| c.category == C::Players).unwrap();
    let ui = VOLUME_CATEGORIES.iter().position(|c| c.category == C::Ui).unwrap();
    set.category_volume[spieler] = 40.0;
    set.category_volume[ui] = 12.25;
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.category_volume, set.category_volume);
    assert_eq!(geladen.master_volume, 50.5);
    assert!((geladen.sound_gain(C::Players) - 0.202).abs() < 1e-6);
    assert!((geladen.sound_gain(C::Blocks) - 0.505).abs() < 1e-6);
    // Die Oberflaeche haengt auch an der Hauptlautstaerke.
    assert!((geladen.sound_gain(C::Ui) - 0.1225 * 0.505).abs() < 1e-6);
    // Die Lautstaerke je UI-Klang uebersteht Speichern und Laden.
    let mut set = Settings::base();
    set.ui_sound_volume[ui_sound::UiSound::Erase.index()] = 35.5;
    set.ui_sound_volume[ui_sound::UiSound::Hover.index()] = 0.0;
    assert_eq!(Settings::parse_onto(Settings::base(), &set.serialize()).ui_sound_volume, set.ui_sound_volume);
}

#[test]
fn nur_skin_und_dauer_ueberleben_speichern_und_laden() {
    let set = Settings::base();
    // Vorgabe wie Vanilla: nur der Skin blitzt, 10 Ticks.
    assert!(set.damage_tint_skin_only);
    assert_eq!(set.damage_tint_ticks, 10);
    let mut set = set;
    set.damage_tint_skin_only = false;
    set.damage_tint_ticks = 23;
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert!(!geladen.damage_tint_skin_only);
    // Der alte Schalter "Ruestung faerben = AN" heisst jetzt "Nur Skin = AUS".
    assert!(!Settings::parse_onto(Settings::base(), "damage_tint_armor=true").damage_tint_skin_only);
    assert_eq!(geladen.damage_tint_ticks, 23);
    // Werte ausserhalb des Reglers bleiben stehen.
    assert_eq!(Settings::parse_onto(Settings::base(), "damage_tint_ticks=0").damage_tint_ticks, 0);
    assert_eq!(Settings::parse_onto(Settings::base(), "damage_tint_ticks=999").damage_tint_ticks, 999);
    assert_eq!(damage_tint_duration_text(10), "0,50 s");
    assert_eq!(damage_tint_duration_text(1), "0,05 s");
}
