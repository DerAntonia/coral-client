use super::*;

#[test]
// Die Kontur ist EIN Viereck, einen Bildschirmpixel groesser als der Glyph; die UV laufen im
// selben Verhaeltnis ueber das Glyphenrechteck hinaus, das Rechteck selbst bleibt in bounds.
fn kontur_ist_ein_viereck_einen_pixel_groesser() {
    let q = text_border_quad(TextShadow::Outline, 1.0, [10.0, 20.0, 26.0, 36.0], [0.0, 0.0], [0.5, 0.5], 2.0, false, false, [0.0, 0.0, 0.0, 0.5], (100.0, 100.0));
    let v = q[0];
    assert!((v.position[0] - (9.0 / 100.0 * 2.0 - 1.0)).abs() < 1e-6);
    assert_eq!(v.bounds, [0.0, 0.0, 0.5, 0.5]);
    assert_eq!(v.mode[0], TEXT_MODE_OUTLINE);
    assert!((v.mode[1] - 0.5 / 16.0).abs() < 1e-6);
    assert!((v.uv[0] + 0.5 / 16.0).abs() < 1e-6);
    // Deckkraft bleibt die des Textes -- sie wird NICHT mehr achtfach gestapelt.
    assert_eq!(v.color[3], 0.5);
}

#[test]
// Vanillas Schatten: ein Schriftpixel (= Skala) nach rechts unten, ein Viertel der Farbe.
fn vanilla_schatten_liegt_einen_schriftpixel_versetzt() {
    let q = text_border_quad(TextShadow::Vanilla, 1.0, [0.0, 0.0, 12.0, 12.0], [0.0, 0.0], [1.0, 1.0], 3.0, true, false, [0.25, 0.25, 0.25, 1.0], (100.0, 100.0));
    assert_eq!(q[0].mode[0], TEXT_MODE_SHADOW);
    assert_eq!(q[0].mode[3], 3.0);
    // Rechte untere Ecke um 3 px verlaengert.
    assert!((q[2].position[0] - (15.0 / 100.0 * 2.0 - 1.0)).abs() < 1e-6);
}

#[test]
// Weisser Text: Vanillas Schatten ist 63/255 auf dem Bildschirm, also dunkel -- nicht ein
// Viertel des LINEAREN Werts (das waere ~137/255, fast weiss auf dunklem Grund).
fn vanilla_schatten_ist_ein_viertel_im_anzeige_raum() {
    let s = vanilla_shadow_color([1.0, 1.0, 1.0, 0.5]);
    assert!((world::sky::linear_to_srgb(s[0]) - 0.25).abs() < 1e-4);
    assert!(s[0] < 0.06);
    assert_eq!(s[3], 0.5);
}

#[test]
fn schattenfarbe_des_servers_als_marker() {
    let mut texts = Vec::new();
    push_span_shadow(&mut texts, None);
    assert!(texts.is_empty(), "ohne eigene Farbe kein Marker");
    push_span_shadow(&mut texts, Some(0x80FF_0000));
    push_span_shadow(&mut texts, Some(0));
    let rot = span_shadow_marker(&texts[0]).unwrap();
    assert!((rot[0] - 1.0).abs() < 1e-6 && rot[1] == 0.0 && rot[2] == 0.0);
    assert!((rot[3] - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(span_shadow_marker(&texts[1]).unwrap()[3], 0.0, "Alpha 0 = kein Schatten");
    // Fett ist EIN Text hinter einem Marker, kein zweiter versetzter Text.
    let mut fett = Vec::new();
    push_styled_piece(protocol::nbt::STYLE_BOLD, None, "Ab".into(), 0.0, 0.0, [1.0; 4], 2.0, &mut fett);
    assert_eq!(fett.len(), 2);
    assert!(is_bold_marker(&fett[0]) && fett[1].text == "Ab");
    assert!(!is_bold_marker(&texts[0]) && span_shadow_marker(&fett[0]).is_none());
    assert_eq!((shadow_marker(&fett[0]), font_marker(&fett[0])), (None, None));
    // Die anderen Marker lesen ihn nicht als ihren.
    assert_eq!(shadow_marker(&texts[0]), None);
    assert_eq!(font_marker(&texts[0]), None);
    assert!(!is_selection_boundary(&texts[0]));
}

#[test]
fn marker_und_einstellungen() {
    let mut texts = Vec::new();
    push_shadow_marker(&mut texts, Some((TextShadow::Vanilla, 0.5, true)));
    push_shadow_marker(&mut texts, None);
    selection_boundary(&mut texts);
    assert_eq!(shadow_marker(&texts[0]), Some(Some((TextShadow::Vanilla, 0.5, true))));
    assert_eq!(shadow_marker(&texts[1]), Some(None));
    assert_eq!(shadow_marker(&texts[2]), None);
    assert!(is_selection_boundary(&texts[2]) && !is_selection_boundary(&texts[0]));

    let mut set = Settings::base();
    assert_eq!(set.global_text_shadow(), TextShadow::Outline);
    set.text_shadow_style = 1;
    assert_eq!(set.global_text_shadow(), TextShadow::Vanilla);
    set.area_shadow[ShadowArea::Chat.index()] = 3;
    assert_eq!(set.shadow_of(ShadowArea::Chat), Some(TextShadow::Off));
    assert_eq!(set.shadow_of(ShadowArea::Hotbar), None);
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.text_shadow_style, 1);
    assert_eq!(geladen.area_shadow, set.area_shadow);

    // Staerke je Kaestchen: bei der Kontur die Dicke, sonst die Deckkraft.
    set.shadow_thickness[1 + ShadowArea::Hotbar.index()] = 25;
    set.shadow_opacity[0] = 40;
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.shadow_strength(1 + ShadowArea::Hotbar.index(), TextShadow::Outline), 2.5);
    assert_eq!(geladen.shadow_strength(0, TextShadow::Vanilla), 0.4);
    assert_eq!(Settings::parse_onto(Settings::base(), "shadow_thickness=99,1\n").shadow_thickness[..2], [SHADOW_THICKNESS_RANGE.1, SHADOW_THICKNESS_RANGE.0]);
}

#[test]
// Schrift je Bereich: Marker hin und zurueck, keiner verwechselt sich mit den anderen,
// und die Einstellung ueberlebt Speichern/Laden.
fn schrift_marker_und_einstellung() {
    let mut texts = Vec::new();
    for k in FontKind::ALL {
        push_font_marker(&mut texts, Some(k));
    }
    push_font_marker(&mut texts, None);
    for (i, k) in FontKind::ALL.into_iter().enumerate() {
        assert_eq!(font_marker(&texts[i]), Some(Some(k)));
        assert_eq!(shadow_marker(&texts[i]), None);
        assert!(!is_selection_boundary(&texts[i]));
    }
    assert_eq!(font_marker(&texts[FontKind::ALL.len()]), Some(None));
    let mut grenze = Vec::new();
    selection_boundary(&mut grenze);
    push_shadow_marker(&mut grenze, None);
    assert!(grenze.iter().all(|t| font_marker(t).is_none()));

    let mut set = Settings::base();
    assert_eq!(set.font_of(ShadowArea::Inventory), None);
    set.area_font[ShadowArea::Inventory.index()] = 3;
    set.area_font[ShadowArea::Chat.index()] = FontKind::ALL.len() as u8;
    set.area_shadow[ShadowArea::Inventory.index()] = 2;
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.font_of(ShadowArea::Inventory), Some(FontKind::Outfit));
    assert_eq!(geladen.font_of(ShadowArea::Chat), FontKind::ALL.last().copied());
    assert_eq!(geladen.shadow_of(ShadowArea::Inventory), Some(TextShadow::Vanilla));
    assert_eq!(geladen.font_of(ShadowArea::Hotbar), None);
}
