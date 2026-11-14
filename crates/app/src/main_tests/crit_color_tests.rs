use super::*;

#[test]
// Die Crit-Farbe ist kein Modul mehr, sondern steht im Video-Tab unter der
// Schadens-Faerbung -- und ist anfangs aus.
fn steht_im_video_tab_unter_der_schadens_faerbung_und_ist_anfangs_aus() {
    const { assert!(CRIT_COLOR_ROW > DAMAGE_TINT_TICKS_ROW) };
    const { assert!(CRIT_COLOR_ROW < VIDEO_ROW_COUNT) };
    assert!(!Settings::base().module_crit_color_enabled, "eine Abweichung von Vanilla soll man einschalten müssen");
}

#[test]
// Die Vorgabefarbe muss sich vom gewoehnlichen Treffer deutlich unterscheiden -- sonst
// taugt das Modul nichts, ohne dass man erst die Regler suchen muss.
fn die_vorgabefarbe_hebt_sich_vom_gewoehnlichen_treffer_ab() {
    let set = Settings::base();
    let normal = set.damage_tint_rgba();
    let crit = set.crit_color_rgba();
    let abstand: f32 = (0..3).map(|i| (normal[i] - crit[i]).abs()).sum();
    assert!(abstand > 1.0, "Vorgabefarben liegen zu nah beieinander: {normal:?} vs {crit:?}");
}

#[test]
// Die beiden Farben sind UNABHAENGIG -- wer die eine verstellt, darf die andere nicht
// mitziehen. Genau das waere passiert, wenn beide auf denselben Feldern saessen.
fn die_crit_farbe_und_die_gewoehnliche_stoeren_sich_nicht() {
    let mut set = Settings::base();
    let vorher = set.damage_tint_rgba();
    *set.crit_color_mut(DamageTintAxis::Hue) = 200;
    *set.crit_color_mut(DamageTintAxis::Alpha) = 10;
    assert_eq!(set.damage_tint_rgba(), vorher);
}

#[test]
fn die_crit_farbe_ueberlebt_speichern_und_laden() {
    let set = Settings {
        module_crit_color_enabled: true,
        module_crit_color_hue: 300,
        module_crit_color_saturation: 40,
        module_crit_color_value: 70,
        module_crit_color_alpha: 85,
        ..Default::default()
    };
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert!(geladen.module_crit_color_enabled);
    assert_eq!(geladen.module_crit_color_hue, 300);
    assert_eq!(geladen.module_crit_color_saturation, 40);
    assert_eq!(geladen.module_crit_color_value, 70);
    assert_eq!(geladen.module_crit_color_alpha, 85);
}

#[test]
fn der_blockumriss_ueberlebt_speichern_und_laden() {
    assert!(!Settings::base().module_block_outline, "eine Abweichung von Vanilla soll man einschalten müssen");
    let set = Settings {
        module_block_outline: true,
        block_outline_hue: 120,
        block_outline_saturation: 50,
        block_outline_value: 80,
        block_outline_alpha: 65,
        block_outline_width_px: 4,
        ..Default::default()
    };
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert!(geladen.module_block_outline);
    assert_eq!(geladen.block_outline_rgba(), set.block_outline_rgba());
    assert_eq!(geladen.block_outline_width_px, 4);
    // Reglerwerte bleiben, wie sie in der Datei stehen; Farbwerte werden geklemmt.
    let kaputt = Settings::parse_onto(Settings::base(), "block_outline_width_px=99\nblock_outline_alpha=500\n");
    assert_eq!(kaputt.block_outline_width_px, 99);
    assert_eq!(kaputt.block_outline_alpha, HSV_SAT_VAL_MAX);
}

// Linear gemischt muss dasselbe herauskommen, was Vanilla im sRGB-Raum mischt: ein
// weisser Schleier ueber Schwarz und ein schwarzer ueber Weiss.
#[test]
fn schleier_mischen_wie_vanilla_im_srgb_raum() {
    let to_srgb = |lin: f32| if lin <= 0.0031308 { lin * 12.92 } else { 1.055 * lin.powf(1.0 / 2.4) - 0.055 };
    for a in [32.0 / 255.0, 0.25, 0.5, 0.75] {
        let weiss = white_veil(a)[3]; // ueber Schwarz: linear = Deckkraft
        assert!((to_srgb(weiss) - a).abs() < 1e-3, "weiss {a}: {}", to_srgb(weiss));
        let schwarz = black_veil(a)[3]; // ueber Weiss: linear = 1 - Deckkraft
        assert!((to_srgb(1.0 - schwarz) - (1.0 - a)).abs() < 1e-3, "schwarz {a}");
    }
}

