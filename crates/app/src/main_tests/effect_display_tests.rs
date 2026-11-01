use super::*;

fn e(effect: i32, remaining: i32) -> EffectEntry {
    EffectEntry { effect, ambient: false, amplifier: 0, remaining, exact: f64::from(remaining), received: remaining }
}

#[test]
// Gui.extractEffects: gute Effekte oben von rechts nach links, alle anderen (auch neutrale
// wie Leuchten) eine Reihe tiefer; gezeichnet in umgekehrter Sortierung.
fn hud_reihen_wie_vanilla() {
    let mut liste = vec![e(protocol::packets::MOB_EFFECT_SPEED, 100), e(4, 200), e(18, 50), e(23, 300)];
    liste.sort_by(effect_order);
    let (plaetze, breite, hoehe) = effect_hud_layout(&liste);
    assert_eq!((breite, hoehe), (50.0, 51.0));
    // Die laengste Dauer steht ganz rechts: Staerke (200) vor Schnelligkeit (100).
    let pos = |id: i32| plaetze.iter().find(|(x, _, _)| x.effect == id).map(|(_, x, y)| (*x, *y)).unwrap();
    assert_eq!(pos(4), (-25.0, 1.0));
    assert_eq!(pos(protocol::packets::MOB_EFFECT_SPEED), (-50.0, 1.0));
    assert_eq!(pos(23), (-25.0, 27.0));
    assert_eq!(pos(18), (-50.0, 27.0));
    // Nur gute Effekte: eine Reihe.
    assert_eq!(effect_hud_layout(&[e(4, 10)]).2, 25.0);
}

#[test]
// Ambiente Effekte stehen hinter allen anderen, unendliche hinter den endlichen.
fn sortierung_nach_compare_to() {
    let mut amb = e(1, 10);
    amb.ambient = true;
    let mut liste = [amb, e(2, protocol::packets::MOB_EFFECT_INFINITE), e(3, 500), e(4, 20)];
    liste.sort_by(effect_order);
    assert_eq!(liste.iter().map(|x| x.effect).collect::<Vec<_>>(), vec![4, 3, 2, 1]);
}

#[test]
fn blinken_und_dauer() {
    assert_eq!(effect_icon_alpha(&e(1, 201)), 1.0);
    assert_eq!(effect_icon_alpha(&e(1, protocol::packets::MOB_EFFECT_INFINITE)), 1.0);
    // Bei 200 Ticks: 0.5 + cos(40 pi) * 0 = 0.5.
    assert!((effect_icon_alpha(&e(1, 200)) - 0.5).abs() < 1e-4);
    assert_eq!(effect_icon_alpha(&e(1, 0)), 0.25);
    assert_eq!(format_effect_duration(20 * 75), "01:15");
    assert_eq!(format_effect_duration(20 * 3725), "01:02:05");
}

#[test]
fn kuerzen_mit_auslassung() {
    let breite = |t: &str| t.chars().count() as f32;
    assert_eq!(clip_with_ellipsis("Schnelligkeit", 8.0, &breite), "Schne...");
}
