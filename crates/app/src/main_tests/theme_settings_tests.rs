use super::*;

#[test]
fn eigene_themes_ueberstehen_speichern_und_laden() {
    let mut s = Settings::parse_onto(Settings::base(), "show_hud=true\n");
    assert_eq!(s.theme_choice, ThemeChoice::Preset(0));
    let mut farben = theme::PRESETS[3].colors;
    farben[theme::Slot::Accent.index()] = 0x123456;
    s.custom_themes.push(CustomTheme { name: "Mein | Theme".to_string(), colors: farben });
    s.select_theme(ThemeChoice::Custom(0));
    let gelesen = Settings::parse_onto(Settings::base(), &s.serialize());
    assert_eq!(gelesen.custom_themes, s.custom_themes);
    assert_eq!(gelesen.theme_choice, ThemeChoice::Custom(0));
    assert_eq!(gelesen.theme, farben);
    s.select_theme(ThemeChoice::Preset(7));
    let gelesen = Settings::parse_onto(Settings::base(), &s.serialize());
    assert_eq!((gelesen.theme_choice, gelesen.theme), (ThemeChoice::Preset(7), theme::PRESETS[7].colors));
}

#[test]
fn alte_dateien_werden_preset_oder_eigenes_theme() {
    // Ein Preset ohne den Eintrag theme_choice (und ohne Markierung): bleibt das Preset.
    let rosa = &theme::PRESETS[4];
    let zeilen: String = theme::Slot::ALL
        .into_iter()
        .filter(|&sl| sl != theme::Slot::Selection)
        .map(|sl| format!("theme_{}={:06X}\n", sl.key(), rosa.colors[sl.index()]))
        .collect();
    assert_eq!(Settings::parse_onto(Settings::base(), &zeilen).theme_choice, ThemeChoice::Preset(4));
    // Geaenderte Farben: werden ein eigenes Theme.
    let geaendert = format!("{zeilen}theme_accent=ABCDEF\n");
    let s = Settings::parse_onto(Settings::base(), &geaendert);
    assert_eq!(s.theme_choice, ThemeChoice::Custom(0));
    assert_eq!(s.theme[theme::Slot::Accent.index()], 0xABCDEF);
    assert_eq!(s.custom_themes[0].colors, s.theme);
}

#[test]
fn kacheln_passen_vor_die_trennlinie() {
    let letzte = theme_tile_rect(THEME_TILE_COLS - 1);
    assert!(letzte.x + letzte.w < THEME_DIVIDER_X, "{} < {}", letzte.x + letzte.w, THEME_DIVIDER_X);
    assert!(theme::PRESETS.len() >= 16);
}
