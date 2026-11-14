use super::*;

// Kupfergolem-Statue als Icon wie Vanilla (-0.5, statue_item_transform, Statuen-Haltung, gui-Haltung
// des Templates [30, 45, 180], 2 hoch, 0.55): Kopf ueber den Fuessen, waagrecht mittig, und
// das Gesicht (Modell -z) zum Betrachter (+z) -- frueher zeigte das Icon den Ruecken.
#[test]
fn kupfergolem_statue_steht_aufrecht() {
    use world::entity_model::{self as em, Affine, Cuboid, Part};
    let mut wurzel = Part::new("root", [0.0, 24.0, 0.0], Vec::new());
    wurzel.children.push(Part::new("kopf", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-4.0, -24.0, -4.0], [8.0, 8.0, 8.0])]));
    wurzel.children.push(Part::new("fuss", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-4.0, -5.0, -4.0], [8.0, 5.0, 8.0])]));
    wurzel.children.push(Part::new("nase", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [-1.0, -20.0, -6.0], [2.0, 2.0, 2.0])]));
    let model = world::Model { texture: "", tex_size: [64.0, 64.0], parts: vec![wurzel] };
    let boxes = em::emit_boxes(&model, &em::copper_golem_statue_pose(&model), Affine::translation([-0.5; 3]).then(held_item::statue_item_transform()), [0.0, 0.0, 1.0, 1.0]);
    let gui = Affine::translation([0.0, 2.0 / 16.0, 0.0]).then(Affine::rotation_xyz_deg(30.0, 45.0, 180.0)).then(Affine::scale([0.55; 3]));
    let mitte = |b: &Vec<world::BlockVertex>, a: usize| b.iter().map(|v| gui.apply(v.position)[a]).sum::<f32>() / b.len() as f32;
    // Bildschirm-y waechst nach unten: Kopf hat das groessere y nach der Haltung.
    assert!(mitte(&boxes[0], 1) > mitte(&boxes[1], 1), "Kopf {} nicht ueber den Fuessen {}", mitte(&boxes[0], 1), mitte(&boxes[1], 1));
    assert!(mitte(&boxes[0], 0).abs() < 0.01 && mitte(&boxes[1], 0).abs() < 0.01, "nicht waagrecht mittig");
    assert!(mitte(&boxes[2], 2) > mitte(&boxes[0], 2), "Gesicht zeigt nicht zum Betrachter");
    // Senkrecht: Kopfoberkante bis Fusssohle liegen um die Feldmitte (Vanilla: 2 Pixel hoch).
    let ys: Vec<f32> = boxes[..2].iter().flatten().map(|v| gui.apply(v.position)[1]).collect();
    let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
    assert!(f32::midpoint(lo, hi).abs() < 0.2, "senkrecht weit aus der Mitte: {lo}..{hi}");
}

// Abgerundetes Pack-Bild: die aeusserste Ecke ist weg, Mitte und Kantenmitte bleiben.
#[test]
fn pack_bild_ecken_abgerundet() {
    let mut bild = vec![255u8; 16 * 16 * 4];
    round_icon_corners(&mut bild, 16, 4.0);
    let alpha = |x: u32, y: u32| bild[((y * 16 + x) * 4 + 3) as usize];
    assert_eq!(alpha(0, 0), 0);
    assert_eq!(alpha(15, 15), 0);
    assert_eq!(alpha(8, 8), 255);
    assert_eq!(alpha(8, 0), 255);
    assert_eq!(alpha(0, 8), 255);
}

#[test]
fn effekt_timer_arten() {
    // 95 Sekunden: Sekunden, Minuten:Sekunden, Minuten mit Zehnteln (abgerundet).
    let t = 95 * 20 + 7;
    assert_eq!(format_effect_timer(t, 0), "95");
    assert_eq!(format_effect_timer(t, 1), "1:35");
    assert_eq!(format_effect_timer(t, 2), "1.5");
    assert_eq!(format_effect_timer(59 * 20, 2), "0.9");
    assert_eq!(format_effect_timer(3725 * 20, 1), "1:02:05");
    assert_eq!(format_effect_timer(-5, 1), "0:00");
}

#[test]
fn timer_blinkt_in_den_letzten_10_sekunden() {
    let e = |remaining| EffectEntry { effect: 0, ambient: false, amplifier: 0, remaining, exact: f64::from(remaining), received: 600 };
    let an = EFFECTS_BLINK_ON;
    assert_eq!(effect_timer_blink(&e(201), an), None);
    assert_eq!(effect_timer_blink(&e(200), an), Some(true));
    assert_eq!(effect_timer_blink(&e(190), an), Some(false));
    assert_eq!(effect_timer_blink(&e(180), an), Some(true));
    assert_eq!(effect_timer_blink(&e(100), EFFECTS_BLINK_OFF), None);
    assert_eq!(effect_timer_blink(&e(protocol::packets::MOB_EFFECT_INFINITE), an), None);
    // Dynamisch: mit 30 Sekunden bekommen blinkt er, mit 15 (oder genau 20) nicht.
    assert_eq!(effect_timer_blink(&e(200), EFFECTS_BLINK_DYNAMIC), Some(true));
    let kurz = |received| EffectEntry { received, ..e(200) };
    assert_eq!(effect_timer_blink(&kurz(300), EFFECTS_BLINK_DYNAMIC), None);
    assert_eq!(effect_timer_blink(&kurz(400), EFFECTS_BLINK_DYNAMIC), None);
    assert_eq!(effect_timer_blink(&kurz(300), an), Some(true));
    // Vergiftung (ID 18) warnt nie.
    let gift = EffectEntry { effect: 18, ..e(100) };
    assert!(!world::potion::effect_beneficial(18));
    assert_eq!(effect_timer_blink(&gift, an), None);
}

#[test]
fn laengster_effekt_zuerst() {
    let e = |effect, remaining: i32| EffectEntry { effect, ambient: false, amplifier: 0, remaining, exact: f64::from(remaining), received: remaining };
    let mut liste = [e(1, 100), e(5, protocol::packets::MOB_EFFECT_INFINITE), e(3, 900), e(2, 900)];
    liste.sort_by(effect_longest_first);
    assert_eq!(liste.iter().map(|e| e.effect).collect::<Vec<_>>(), [5, 2, 3, 1]);
}

#[test]
fn effekt_anker_am_festen_rand() {
    let r = Rect { x: 10.0, y: 20.0, w: 100.0, h: 50.0 };
    assert_eq!(effects_detail_anchor(r, 0), (10.0, 20.0));
    assert_eq!(effects_detail_anchor(r, EFFECTS_ALIGN_BOTTOM), (10.0, 70.0));
    assert_eq!(effects_detail_anchor(r, EFFECTS_ALIGN_LEFT), (10.0, 20.0));
    assert_eq!(effects_detail_anchor(r, EFFECTS_ALIGN_RIGHT), (110.0, 20.0));
}

#[test]
// Titel, Untertitel und Effekte sind eigene Elemente (die Spielerliste steht nicht mehr
// im Editor); alles Neue ueberlebt Speichern und Laden.
fn titel_untertitel_und_effekte_sind_eigene_elemente() {
    for e in [HudElement::Title, HudElement::Subtitle, HudElement::Effects, HudElement::BossBar] {
        assert!(HudElement::ALL.contains(&e));
    }
    let set = Settings {
        hud_subtitle_x: 0.2,
        hud_subtitle_y: 0.8,
        hud_subtitle_align: 0,
        hud_title_scale: 150,
        hud_subtitle_scale: 70,
        hud_effects_x: 0.3,
        hud_effects_y: 0.1,
        hud_effects_scale: 120,
        show_effects_hud: false,
        show_effects_inventory: false,
        effects_detailed: true,
        effects_hide_ambient: true,
        hud_effects_detail_x: 0.4,
        hud_effects_detail_y: 0.6,
        effects_align: EFFECTS_ALIGN_BOTTOM,
        effects_icon: false,
        effects_name: false,
        effects_timer_blink: EFFECTS_BLINK_DYNAMIC,
        effects_name_color: true,
        effects_timer_style: 2,
        effects_timer_color: true,
        hud_bossbar_x: 0.25,
        hud_bossbar_y: 0.6,
        hud_bossbar_scale: 150,
        ..Default::default()
    };
    let g = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!((g.hud_bossbar_x, g.hud_bossbar_y, g.hud_bossbar_scale), (0.25, 0.6, 150));
    // Standard: Vanillas Lage oben mittig.
    assert_eq!((Settings::base().hud_bossbar_x, Settings::base().hud_bossbar_y), (0.5, 0.0));
    assert!(g.effects_detailed && !g.effects_icon && g.effects_name_color && g.effects_timer_color);
    assert!(!g.effects_name && g.effects_timer_blink == EFFECTS_BLINK_DYNAMIC && g.effects_hide_ambient);
    // Alte Datei mit dem Schalter.
    assert_eq!(Settings::parse_onto(Settings::base(), "effects_timer_blink=false\n").effects_timer_blink, EFFECTS_BLINK_OFF);
    assert_eq!(Settings::parse_onto(Settings::base(), "effects_timer_blink=true\n").effects_timer_blink, EFFECTS_BLINK_ON);
    assert_eq!((g.hud_effects_detail_x, g.hud_effects_detail_y, g.effects_align, g.effects_timer_style), (0.4, 0.6, EFFECTS_ALIGN_BOTTOM, 2));
    assert_eq!((g.hud_subtitle_x, g.hud_subtitle_y, g.hud_subtitle_align), (0.2, 0.8, 0));
    assert_eq!((g.hud_title_scale, g.hud_subtitle_scale), (150, 70));
    assert_eq!((g.hud_effects_x, g.hud_effects_y, g.hud_effects_scale), (0.3, 0.1, 120));
    assert!(!g.show_effects_hud && !g.show_effects_inventory);
    assert!(Settings::base().show_effects_hud && Settings::base().show_effects_inventory);
}

// Scoreboard: die gewaehlte Ecke bleibt stehen, die Leiste waechst von ihr weg; die Ecke
// ueberlebt Speichern und Laden.
#[test]
fn scoreboard_feste_ecke() {
    let r = Rect { x: 100.0, y: 50.0, w: 80.0, h: 120.0 };
    for ecke in 0..SCOREBOARD_ALIGNS.len() as u8 {
        let anker = scoreboard_anchor(r, ecke);
        assert_eq!(scoreboard_origin(anker, r.w, r.h, ecke), (r.x, r.y), "Ecke {ecke}");
        // Hoeher geworden: unten verankert waechst sie nach oben, oben verankert nach unten.
        let (_, y) = scoreboard_origin(anker, r.w, r.h + 40.0, ecke);
        assert_eq!(y, if ecke >= 2 { r.y - 40.0 } else { r.y }, "Ecke {ecke}");
    }
    assert_eq!(scoreboard_anchor(r, 3), (180.0, 170.0));
    let set = Settings { scoreboard_align: 3, ..Settings::base() };
    assert_eq!(Settings::parse_onto(Settings::base(), &set.serialize()).scoreboard_align, 3);
    assert_eq!(Settings::parse_onto(Settings::base(), "scoreboard_align=9\n").scoreboard_align, 3);
}

// Leiste oben beim Pausieren: startet ganz ueber dem Bildrand, steht nach der Einblendzeit.
#[test]
fn pausenleiste_gleitet_herein() {
    assert_eq!(pause_bar_slide_dy(0.0), -TOPBAR_H);
    assert!(pause_bar_slide_dy(PAUSE_BAR_SLIDE_SECS / 2.0) > -TOPBAR_H / 4.0, "abbremsend: zur Haelfte fast da");
    assert_eq!(pause_bar_slide_dy(PAUSE_BAR_SLIDE_SECS), 0.0);
    assert_eq!(pause_bar_slide_dy(5.0), 0.0);
}
