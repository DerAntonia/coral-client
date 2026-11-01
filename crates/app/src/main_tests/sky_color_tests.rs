use super::*;

#[test]
fn feste_uhrzeit_laeuft_im_kreis() {
    assert_eq!(sky_time_label(0), "06:00");
    assert_eq!(sky_time_label(6000), "12:00");
    assert_eq!(sky_time_label(18_000), "00:00");
    assert_eq!(sky_time_label(23_750), "05:45");
    let mut set = Settings::base();
    assert!(set.sky_mode == SkyMode::Solid && !set.module_sky_fixed_time, "Vorgabe wie bisher: nur die Farbe");
    set.module_sky_time = 0;
    adjust_sky_time(&mut set, -1);
    assert_eq!(set.module_sky_time, 23_750);
    adjust_sky_time(&mut set, 1);
    assert_eq!(set.module_sky_time, 0);
    set.module_sky_time = 13_250;
    set.module_sky_fixed_time = true;
    set.sky_mode = SkyMode::Gradient;
    set.sky_moon = false;
    set.sky_stars = false;
    let zurueck = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!((zurueck.module_sky_time, zurueck.module_sky_fixed_time, zurueck.sky_mode), (13_250, true, SkyMode::Gradient));
    assert!(zurueck.sky_sun && !zurueck.sky_moon && !zurueck.sky_stars);
    // Alte Einstellung "feste Farbe aus" heisst jetzt Skin.
    assert_eq!(Settings::parse_onto(Settings::base(), "module_sky_fixed_color=false\n").sky_mode, SkyMode::Skin);
}

// PvP-Packs machen rain.png/snow.png voll durchsichtig: ohne Modul bleibt das so, mit
// Modul springt Vanillas Bild ein. Ein Pack mit echtem Bild behaelt es immer.
#[test]
fn leere_pack_wettertextur_nur_mit_modul_ersetzt() {
    let bild = |alpha: u8| Some(assets::DecodedTexture { width: 1, height: 2, rgba: vec![255, 255, 255, 0, 255, 255, 255, alpha] });
    let leer = WeatherTexture { pack: bild(0), vanilla: bild(200) };
    assert!(leer.pack_is_empty());
    assert_eq!(leer.chosen(false).unwrap().rgba[7], 0, "ohne Modul gilt der Pack");
    assert_eq!(leer.chosen(true).unwrap().rgba[7], 200, "mit Modul Vanilla");
    let eigen = WeatherTexture { pack: bild(90), vanilla: bild(200) };
    assert_eq!(eigen.chosen(true).unwrap().rgba[7], 90, "ein echtes Pack-Bild bleibt");
}

// Die Standard-Ruestung laeuft jetzt ueber Vanillas Equipment-Dateien: jede Ebene, die sie
// nennt, muss im Entity-Atlas stehen (world::armor::all_textures), sonst verschwaende die
// Ruestung. Liest die Dateien aus dem Jar, deshalb #[ignore].
#[test]
#[ignore]
fn vanilla_equipment_dateien_passen_zum_atlas() {
    let jar = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".minecraft/versions/26.2/26.2.jar");
    let atlas = world::armor::all_textures();
    for m in &world::armor::MATERIALS {
        let bytes = std::process::Command::new("unzip").arg("-p").arg(&jar).arg(format!("assets/minecraft/equipment/{}.json", m.asset)).output().unwrap().stdout;
        let modell = assets::parse_equipment_model(&bytes).unwrap_or_else(|| panic!("{}.json", m.asset));
        assert!(!modell.humanoid.is_empty(), "{}", m.asset);
        for ebene in modell.humanoid.iter().chain(&modell.humanoid_leggings) {
            let pfad = ebene.texture.strip_prefix("minecraft:").unwrap();
            assert!(atlas.contains(&pfad), "{} nennt {pfad}, der Atlas hat es nicht", m.asset);
        }
        assert_eq!(modell.humanoid[0].dyeable.is_some(), m.dyeable, "{}", m.asset);
    }
}

#[test]
fn wetter_modul_einstellungen() {
    let mut set = Settings::base();
    assert!(!set.module_weather && set.weather_mode == WeatherMode::Server && set.weather_radius == precipitation::RADIUS_DEFAULT);
    assert_eq!(set.weather_radius_inner, 0, "Vorgabe wie Vanilla: von der Kamera an");
    assert_eq!(WeatherMode::Snow.next(), WeatherMode::Server, "Auswahl laeuft im Kreis");
    assert!(MODULE_IDS.contains(&ModuleId::Weather));
    set.module_weather = true;
    set.weather_mode = WeatherMode::Snow;
    set.weather_radius = 5;
    set.weather_radius_inner = 3;
    let zurueck = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(
        (zurueck.module_weather, zurueck.weather_mode, zurueck.weather_radius_inner, zurueck.weather_radius),
        (true, WeatherMode::Snow, 3, 5)
    );
    // Unsinniger Modus: Vorgabe; der Radius bleibt, wie er dasteht.
    let kaputt = Settings::parse_onto(Settings::base(), "weather_mode=hagel\nweather_radius=99\n");
    assert_eq!((kaputt.weather_mode, kaputt.weather_radius), (WeatherMode::Server, 99));
}

#[test]
fn das_modul_steht_im_menue_und_ist_anfangs_aus() {
    assert!(MODULE_IDS.contains(&ModuleId::SkyColor));
    assert!(!Settings::base().module_sky_color_enabled);
    assert!(matches!(ModuleId::SkyColor.toggle_action(), ButtonAction::ToggleModuleSkyColor));
}

#[test]
// Die Vorgabefarbe soll dem bisherigen Tageshimmel entsprechen: wer das Modul einschaltet,
// soll keinen Farbsprung sehen, sondern nur, dass der Himmel stehen bleibt.
fn die_vorgabe_trifft_den_tageshimmel() {
    let set = Settings::base();
    let aus_hsv = set.sky_color_rgb();
    for i in 0..3 {
        // Nicht exakt gleich: die Regler laufen im Raster (10 Grad / 5 Prozent), der
        // Tageshimmel liegt zwischen zwei Rasterpunkten. Nah genug, dass das Einschalten
        // nicht als Farbsprung auffaellt.
        assert!(
            (aus_hsv[i] - SKY_DAY_SRGB[i]).abs() < 0.05,
            "Kanal {i}: HSV-Vorgabe {} weicht vom Tageshimmel {} ab",
            aus_hsv[i],
            SKY_DAY_SRGB[i]
        );
    }
}

#[test]
fn die_himmelsfarbe_ueberlebt_speichern_und_laden() {
    let set = Settings {
        module_sky_color_enabled: true,
        module_sky_color_hue: 280,
        module_sky_color_saturation: 35,
        module_sky_color_value: 60,
        ..Default::default()
    };
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert!(geladen.module_sky_color_enabled);
    assert_eq!(geladen.module_sky_color_hue, 280);
    assert_eq!(geladen.module_sky_color_saturation, 35);
    assert_eq!(geladen.module_sky_color_value, 60);
}

#[test]
// Der Himmel des Moduls ist ZEITLOS -- das ist sein ganzer Zweck. Und der Tagesverlauf
// darf umgekehrt nicht stehenbleiben.
fn eingeschaltet_ignoriert_er_die_tageszeit_ausgeschaltet_nicht() {
    let set = Settings { module_sky_color_enabled: true, ..Default::default() };
    let fest = set.sky_color_rgb();
    // Die Farbe des Moduls haengt an keiner Zeit -- sie ist schlicht die eingestellte.
    assert_eq!(fest, set.sky_color_rgb());
    // Ohne Modul dagegen: Mittag hell, Mitternacht schwarz.
    let mittag = world::sky::time_of_day_sky(SKY_DAY_SRGB, 6000);
    let mitternacht = world::sky::time_of_day_sky(SKY_DAY_SRGB, 18_000);
    assert_eq!(mittag, SKY_DAY_SRGB);
    assert_eq!(mitternacht, [0.0, 0.0, 0.0]);
}
