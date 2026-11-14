use super::*;

// Die Enderperle. ThrownEnderpearl teleportiert einen Spieler mit Geschwindigkeit 0,
// relativer Rotation und allen DELTA-Bits -- also Geschwindigkeit 0 im Paket, aber ALLE vier DELTA-Bits gesetzt. Vanilla addiert
// die 0 zur eigenen Geschwindigkeit: der Schwung bleibt komplett erhalten. Genau das
// hat der Client bisher weggeworfen (er nullte bei jedem Teleport), und genau deshalb
// hat jede Perle geflaggt.
#[test]
fn perle_nimmt_den_schwung_mit() {
    use protocol::packets::{
        POSITION_FLAG_REL_DELTA_X, POSITION_FLAG_REL_DELTA_Y, POSITION_FLAG_REL_DELTA_Z, POSITION_FLAG_REL_PITCH,
        POSITION_FLAG_REL_YAW, POSITION_FLAG_ROTATE_DELTA,
    };
    let perle = POSITION_FLAG_REL_YAW
        | POSITION_FLAG_REL_PITCH
        | POSITION_FLAG_REL_DELTA_X
        | POSITION_FLAG_REL_DELTA_Y
        | POSITION_FLAG_REL_DELTA_Z
        | POSITION_FLAG_ROTATE_DELTA;
    let schwung = DVec3::new(0.34, -0.29, 0.12);
    // Rotation ist bei der Perle relativ mit 0, alte und neue Blickrichtung sind also
    // gleich -- ROTATE_DELTA dreht dann um 0 Grad und aendert nichts.
    let danach = teleport_velocity(schwung, DVec3::ZERO, perle, (73.5, -12.0), (73.5, -12.0));
    assert!((danach - schwung).length() < 1e-12, "Perle muss den Schwung behalten: {danach:?} statt {schwung:?}");

    // Ein gewoehnlicher absoluter Teleport (keine DELTA-Bits, z.B. /tp oder ein Setback)
    // ERSETZT die Geschwindigkeit durch den Paketwert -- hier also null.
    let danach = teleport_velocity(schwung, DVec3::ZERO, 0, (73.5, -12.0), (0.0, 0.0));
    assert_eq!(danach, DVec3::ZERO, "absoluter Teleport setzt die Geschwindigkeit");

    // Und ein Teleport, der eine Geschwindigkeit VORGIBT, setzt genau die.
    let vorgabe = DVec3::new(0.0, 0.42, 0.0);
    assert_eq!(teleport_velocity(schwung, vorgabe, 0, (0.0, 0.0), (0.0, 0.0)), vorgabe);
}

// ROTATE_DELTA dreht den mitgenommenen Schwung in die neue Blickrichtung. Dreht der
// Teleport einen um 90 Grad, muss aus "nach vorn" wieder "nach vorn" werden -- in
// Weltkoordinaten also eine um 90 Grad gedrehte Geschwindigkeit.
#[test]
fn rotate_delta_dreht_den_schwung_mit() {
    use protocol::packets::{POSITION_FLAG_REL_DELTA_X, POSITION_FLAG_REL_DELTA_Y, POSITION_FLAG_REL_DELTA_Z, POSITION_FLAG_ROTATE_DELTA};
    let flags = POSITION_FLAG_REL_DELTA_X | POSITION_FLAG_REL_DELTA_Y | POSITION_FLAG_REL_DELTA_Z | POSITION_FLAG_ROTATE_DELTA;
    let schwung = DVec3::new(1.0, 0.0, 0.0);
    let danach = teleport_velocity(schwung, DVec3::ZERO, flags, (0.0, 0.0), (90.0, 0.0));
    // yRot(alt - neu = -90 Grad): x' = x*cos + z*sin = -0, z' = z*cos - x*sin = +1
    assert!(danach.x.abs() < 1e-6 && (danach.z - 1.0).abs() < 1e-6, "um 90 Grad gedreht erwartet, war {danach:?}");
}

// Die Statusleisten zaehlen in halben Punkten -- genau hier sitzen die
// Ab-eins-daneben-Fehler. Durchgerechnet an den Werten, die im Spiel wirklich vorkommen.
#[test]
fn herzen_zeigen_halbe_und_volle_schritte() {
    // 20 Leben = zehn volle Herzen.
    assert_eq!(heart_sprite(20.0, 9), Some("hud/heart/full"));
    // 19 Leben: das zehnte Herz (Index 9) deckt die Punkte 19/20 -- also halb.
    assert_eq!(heart_sprite(19.0, 9), Some("hud/heart/half"));
    assert_eq!(heart_sprite(19.0, 8), Some("hud/heart/full"));
    // 18 Leben: das zehnte Herz ist ganz leer, gezeichnet wird nur der Behaelter.
    assert_eq!(heart_sprite(18.0, 9), None);
    // Bruchteile zaehlen aufwaerts: 18.5 fuellt das zehnte Herz noch nicht halb (dafuer
    // braeuchte es 19), 19.5 fuellt es nicht voll.
    assert_eq!(heart_sprite(18.5, 9), None);
    assert_eq!(heart_sprite(19.5, 9), Some("hud/heart/half"));
    // Ein Treffer auf 0.5 Leben laesst genau ein halbes Herz stehen.
    assert_eq!(heart_sprite(1.0, 0), Some("hud/heart/half"));
    assert_eq!(heart_sprite(0.0, 0), None);
    // Goldener Apfel: 4 Absorption = zwei volle goldene Herzen, 3 = eins voll, eins halb.
    assert_eq!(absorbing_heart_sprite(4, 1), Some("hud/heart/absorbing_full"));
    assert_eq!(absorbing_heart_sprite(4, 2), None);
    assert_eq!(absorbing_heart_sprite(3, 1), Some("hud/heart/absorbing_half"));
}

#[test]
fn herzarten_heissen_wie_vanillas_sprites() {
    assert_eq!(heart_variant("hud/heart/full", "", false), "hud/heart/full");
    assert_eq!(heart_variant("hud/heart/half", "poisoned_", false), "hud/heart/poisoned_half");
    assert_eq!(heart_variant("hud/heart/full", "frozen_", true), "hud/heart/frozen_hardcore_full");
    assert_eq!(heart_variant("hud/heart/full", "", true), "hud/heart/hardcore_full");
    assert_eq!(heart_variant("hud/heart/container", "withered_", true), "hud/heart/container_hardcore");
    assert_eq!(heart_variant("hud/heart/container", "poisoned_", false), "hud/heart/container");
    // Absorption bleibt bei Gift, wird aber bei Wither zum Wither-Herz.
    assert_eq!(heart_variant("hud/heart/absorbing_half", "poisoned_", true), "hud/heart/absorbing_hardcore_half");
    assert_eq!(heart_variant("hud/heart/absorbing_full", "withered_", false), "hud/heart/withered_full");
}

#[test]
fn ruestung_zeichnet_auch_die_leeren_symbole() {
    // Volle Diamantruestung = 20 Punkte = zehn volle Symbole.
    assert_eq!(armor_sprite(20, 0), "hud/armor_full");
    assert_eq!(armor_sprite(20, 9), "hud/armor_full");
    // Ein ungerader Wert endet auf einem halben Symbol, danach kommen leere.
    assert_eq!(armor_sprite(7, 2), "hud/armor_full");
    assert_eq!(armor_sprite(7, 3), "hud/armor_half");
    assert_eq!(armor_sprite(7, 4), "hud/armor_empty");
}

#[test]
fn nahrung_folgt_derselben_halbschritt_zaehlung() {
    assert_eq!(food_sprite(20, 9), Some("hud/food_full"));
    assert_eq!(food_sprite(7, 3), Some("hud/food_half"));
    assert_eq!(food_sprite(7, 4), None);
    // Genau die Sprint-Schwelle (Vanilla: foodLevel > 6): drei volle Keulen plus eine
    // halbe -- der Wert, bei dem der Sprint gerade noch geht.
    assert_eq!(food_sprite(7, 0), Some("hud/food_full"));
}

#[test]
fn luftblasen_verschwinden_erst_bei_wirklich_null() {
    assert_eq!(air_bubbles(MAX_AIR_TICKS), 10);
    // Ein einziger Tick Restluft muss noch EINE Blase zeigen (deshalb aufrunden).
    assert_eq!(air_bubbles(1), 1);
    assert_eq!(air_bubbles(0), 0);
    // Halbe Luft = halbe Leiste.
    assert_eq!(air_bubbles(MAX_AIR_TICKS / 2), 5);
}

// Die Aufladeschwelle entscheidet, WANN ein Schlag den Sprint zuruecksetzt -- und damit,
// warum schnelles Kristall-Klicken den Sprint stehen laesst, ein gezielter Schwertschlag
// aber nicht. Nachgerechnet an Vanillas Formel, nicht an gefuehlten Werten.
#[test]
fn angriffs_aufladung_folgt_vanillas_schwelle() {
    // Diamantschwert: Tempo 1.6 -> 12.5 Ticks je Ladung. Die Schwelle 0.9 liegt bei
    // (t + 0.5) / 12.5 > 0.9, also t > 10.75 -- ab Tick 11.
    assert!(!angriff_ist_aufgeladen(10, 1.6));
    assert!(angriff_ist_aufgeladen(11, 1.6));

    // Blosse Hand: Tempo 4.0 -> 5 Ticks. (t + 0.5) / 5 > 0.9 -> t > 4, also ab Tick 5.
    assert!(!angriff_ist_aufgeladen(4, 4.0));
    assert!(angriff_ist_aufgeladen(5, 4.0));

    // Klickspam auf einen Kristall: direkt nach dem letzten Schlag ist nichts geladen,
    // der Sprint bleibt also stehen.
    assert!(!angriff_ist_aufgeladen(0, 1.6));
    assert!(!angriff_ist_aufgeladen(1, 1.6));

    // Streitkolben ist die langsamste Waffe (Tempo 0.6 -> 33.3 Ticks).
    assert!(!angriff_ist_aufgeladen(29, 0.6));
    assert!(angriff_ist_aufgeladen(30, 0.6));
}

#[test]
fn hand_folgt_der_aufladung_hoch_drei() {
    // Direkt nach dem Schlag: praktisch ganz weg.
    assert!(equip_target(0, 1.6) > 0.99);
    // Schwert (12.5 Ticks) nach 6 von 12.5 Ticks Ladung: Ladung hoch drei.
    let ladung: f32 = 6.0 / 12.5;
    assert!((equip_target(5, 1.6) - (1.0 - ladung * ladung * ladung)).abs() < 1e-5);
    // Voll geladen: ganz oben.
    assert_eq!(equip_target(12, 1.6), 0.0);
    assert_eq!(equip_target(100, 4.0), 0.0);
    // Hoechstens 0.4 pro Tick, in beide Richtungen.
    assert!((equip_step(0.0, 1.0) - 0.4).abs() < 1e-6);
    assert!((equip_step(1.0, 0.0) - 0.6).abs() < 1e-6);
}

// java.util.Random(0).nextFloat() und der zweite Wert -- dieselbe Folge wie in Vanilla.
#[test]
fn java_random_stimmt_mit_java_ueberein() {
    let mut r = JavaRandom::new(0);
    assert!((r.next_float() - 0.730_967_76).abs() < 1e-6);
    assert!((r.next_float() - 0.831_441).abs() < 1e-6);
}

#[test]
fn liegender_stapel_zeigt_je_nach_anzahl_kopien() {
    assert_eq!(dropped_item_rendered_amount(1), 1);
    assert_eq!(dropped_item_rendered_amount(16), 2);
    assert_eq!(dropped_item_rendered_amount(17), 3);
    assert_eq!(dropped_item_rendered_amount(48), 4);
    assert_eq!(dropped_item_rendered_amount(64), 5);
    // Flach (Sprite, 1/32 tief): hintereinander, symmetrisch um die Mitte.
    let flach = dropped_item_copies(64, 936, 1.0 / 32.0);
    assert_eq!(flach.len(), 5);
    let schritt = 1.5 / 32.0;
    assert!((flach[0].t[2] + 2.0 * schritt).abs() < 1e-6);
    assert!((flach[4].t[2] - 2.0 * schritt).abs() < 1e-6);
    // Dick (Block): erste Kopie genau in der Mitte, die anderen hoechstens 0.15 daneben.
    let dick = dropped_item_copies(20, 1, 0.25);
    assert_eq!(dick.len(), 3);
    assert_eq!(dick[0].t, [0.0; 3]);
    assert!(dick[1..].iter().all(|a| a.t.iter().all(|v| v.abs() <= 0.15)));
    // Derselbe Seed, dieselbe Streuung.
    assert_eq!(dropped_item_copies(20, 1, 0.25)[2].t, dick[2].t);
}

#[test]
fn schwung_beginnt_bei_null_und_endet_in_ruhelage() {
    // Angestossen, noch nicht getickt: kein Ausschlag, egal wo im Tick.
    assert_eq!(swing_progress_from_ticks(PLAYER_SWING_TICKS + 1, 0.9, PLAYER_SWING_TICKS), 0.0);
    // Erster Tick: von 0 an.
    assert_eq!(swing_progress_from_ticks(PLAYER_SWING_TICKS, 0.0, PLAYER_SWING_TICKS), 0.0);
    // Letzter Tick: bis genau 1, nie darueber.
    assert_eq!(swing_progress_from_ticks(1, 1.0, PLAYER_SWING_TICKS), 1.0);
    assert!(swing_progress_from_ticks(1, 0.5, PLAYER_SWING_TICKS) < 1.0);
}

// SnapTap: bei zwei gegensaetzlichen Tasten gewinnt die ZULETZT gedrueckte. Ohne den
// Schalter bleiben beide aktiv (Vanilla: sie heben sich in der Summe auf).
#[test]
fn snap_tap_laesst_die_zuletzt_gedrueckte_taste_gewinnen() {
    // links seit Druck 1 gehalten, rechts seit Druck 2 -- rechts ist neuer.
    let (links, rechts) = ((true, 1), (true, 2));
    assert_eq!(resolve_snap_tap(links, rechts, false), (true, true), "ohne SnapTap bleibt es bei Vanilla");
    assert_eq!(resolve_snap_tap(links, rechts, true), (false, true), "rechts wurde spaeter gedrueckt");
    // Links neu gedrueckt (hoehere Nummer) dreht es um.
    assert_eq!(resolve_snap_tap((true, 3), rechts, true), (true, false));
    // Nur eine Richtung gedrueckt: unveraendert, egal wie alt der Druck ist.
    assert_eq!(resolve_snap_tap((false, 1), rechts, true), (false, true));
    assert_eq!(resolve_snap_tap(links, (false, 2), true), (true, false));
    assert_eq!(resolve_snap_tap((false, 0), (false, 0), true), (false, false));
}

// Die Konto-UUID geht als 16 Rohbytes in jede Chat-Signatur -- ein Fehler hier macht
// JEDE signierte Nachricht ungueltig.
#[test]
fn uuid_hex_wird_zu_rohbytes() {
    let with_dashes = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
    let without = "069a79f444e94726a5befca90e38aaf5";
    let expected = [0x06, 0x9a, 0x79, 0xf4, 0x44, 0xe9, 0x47, 0x26, 0xa5, 0xbe, 0xfc, 0xa9, 0x0e, 0x38, 0xaa, 0xf5];
    assert_eq!(uuid_bytes_from_hex(without), Some(expected));
    assert_eq!(uuid_bytes_from_hex(with_dashes), Some(expected));
    assert_eq!(uuid_bytes_from_hex("zzzz"), None);
}

// MAX_VISIBLE_SERVERS wird aus der Panel-Hoehe gerechnet -- diese Rechnung darf nicht
// still danebenliegen: die letzte Kachel muss ueber der Fussleiste enden (sonst laege
// der "Bearbeiten"-Knopf unter einer Kachel), und eine weitere darf nicht mehr passen
// (sonst verschenkte das Panel eine Zeile). Layout-Kollisionen durch hartkodierte
// y-Werte sind hier schon oefter passiert, s. SETTINGS_ROW_COUNT.
// Jeder Regler hat einen echten Bereich, und das Vergroessern per Rechtsklick fuehrt nie
// ueber die Grenze, die das Eintippen auch hat.
#[test]
fn regler_bereiche_sind_stimmig() {
    for ev in EditableValue::ALL {
        let Some(r) = ev.slider() else { continue };
        assert!(r.min < r.soft && r.soft <= r.hard, "{r:?}");
        assert!(ev.slider_value_rect().x > ev.slider_track().x + ev.slider_track().w);
    }
}

// Kommt ein UI-Klang oder eine Lautstaerke-Kategorie dazu, muss ihr Regler auch in
// EditableValue::ALL stehen -- sonst ist er zu sehen, laesst sich aber nicht ziehen
// (so war es beim "Aufploppen").
#[test]
fn jede_lautstaerke_hat_einen_regler() {
    for i in 0..ui_sound::UiSound::ALL.len() {
        assert!(EditableValue::ALL.contains(&EditableValue::UiSoundVolume(i)), "UI-Klang {i}");
    }
    for i in 0..VOLUME_CATEGORIES.len() {
        assert!(EditableValue::ALL.contains(&EditableValue::CategoryVolume(i)), "Kategorie {i}");
    }
}

// Alle Regler gleich lang, egal wie breit ihre Zahl ist -- die Einstellungsseiten ebenso
// wie die alten Stepper mit Regler (HUD-Fenster, Module).
#[test]
fn alle_regler_bahnen_gleich_lang() {
    for ev in EditableValue::ALL.into_iter().filter(|ev| ev.slider().is_some()) {
        assert_eq!(ev.slider_track().w, ev.track_w());
        assert_eq!(ev.slider_track().x, ev.slider_rect().x + 6.0);
    }
    let breit = App::wide_slider(App::step_slider(ButtonAction::Nothing, ButtonAction::Nothing, stepper_value_rect(0), 1.0, 0.0, 2.0, true));
    let schmal = App::step_slider(ButtonAction::Nothing, ButtonAction::Nothing, stepper_value_rect(0), 1.0, 0.0, 2.0, true);
    let (b, s) = (breit.slider.unwrap(), schmal.slider.unwrap());
    assert_eq!((b.track().x, b.track().w), (s.track().x, s.track().w));
    assert_eq!(breit.rect.w, b.rect().w, "der breite Regler trifft bis an sein Wertfeld");
    assert!(b.value_box().x > b.track().x + b.track().w);
}

// Die Zahl im Feld eines Reglers bzw. die getippte: Einheit, Komma, Vorzeichen, Uhrzeit;
// Woerter haben keine.
#[test]
fn regler_zahl_aus_feld() {
    assert_eq!(slider_number("12,5 %"), Some((12.5, false)));
    assert_eq!(slider_number("1.5 s"), Some((1.5, false)));
    assert_eq!(slider_number("-3 Halbtöne"), Some((-3.0, false)));
    assert_eq!(slider_number("x2.0"), Some((2.0, false)));
    assert_eq!(slider_number("18:30"), Some((18.5, true)));
    assert_eq!(slider_number("7:"), Some((7.0, true)));
    assert_eq!(slider_number("Aus"), None);
    assert_eq!(slider_number("unbegrenzt"), None);
    // Der Uhrzeit-Regler beginnt um 06:00 und laeuft ueber Mitternacht weiter.
    assert_eq!(clock_from_day_start(6.0), 0.0);
    assert!(clock_from_day_start(3.0) > clock_from_day_start(23.0));
}

// Jeder Marker wird nur von seinem eigenen Leser erkannt. Der Chat-Marker fuer ruhige
// Zahlen hatte denselben Code wie der Rand "Aussen" und schaltete im Chat jeden Rand ab.
#[test]
fn text_marker_eindeutig() {
    let mut texts = Vec::new();
    push_plain_numbers_marker(&mut texts, true);
    push_plain_numbers_marker(&mut texts, false);
    for t in &texts {
        assert_eq!(shadow_marker(t), None);
        assert!(span_shadow_marker(t).is_none() && !is_bold_marker(t) && !is_typed_marker(t) && !is_selection_boundary(t));
        assert_eq!(font_marker(t), None);
    }
    let mut raender = Vec::new();
    for stil in [TextShadow::Outline, TextShadow::Vanilla, TextShadow::Off, TextShadow::Below, TextShadow::Around] {
        push_shadow_marker(&mut raender, Some((stil, 1.0, false)));
    }
    push_shadow_marker(&mut raender, None);
    for t in &raender {
        assert_eq!(plain_numbers_marker(t), None);
        assert_eq!(font_marker(t), None);
    }
}

// Schwarze und fast schwarze Schrift bekommt keinen Rand, dunkelgraue (Vanilla §8) schon.
#[test]
fn schwarz_ohne_rand() {
    assert!(is_dark([0.0, 0.0, 0.0, 1.0]));
    assert!(!is_dark([0.09, 0.09, 0.09, 1.0]));
    // TextOnAccent der Voreinstellungen Rosa und Nord.
    assert!(is_dark(theme::rgba(0x22040F)));
    assert!(is_dark(theme::rgba(0x1E2430)));
    // Vanilla-Dunkelblau (§1) behaelt ihn.
    assert!(!is_dark(theme::rgba(0x0000AA)));
}

// Der Ring liegt um die Mitte des Punkts, auch wenn die bei ungerader Groesse mitten auf
// einem Pixel sitzt.
#[test]
fn ring_mitte_wie_punkt_mitte() {
    for size in 2..9 {
        let teile = crosshair_parts(CROSSHAIR_SHAPE_DOT, size, (100.0, 100.0), false);
        let Some((CrosshairPart::Disc { center, .. }, _)) = teile.first().copied() else { panic!() };
        let v = crosshair_center_offset(CROSSHAIR_SHAPE_DOT, size);
        assert_eq!(center, (100.0 + v, 100.0 + v), "Größe {size}");
    }
    assert_eq!(crosshair_center_offset(0, 5), 0.0, "das Kreuz sitzt auf der Pixelecke");
}

#[test]
fn server_karten_passen_zwischen_die_leisten() {
    let n = server_rows_visible();
    assert!(n >= 1);
    let last = server_card_rect(n - 1, 0.0);
    assert!(last.y + last.h <= server_list_bottom(), "letzte Karte endet bei {}, Liste bis {}", last.y + last.h, server_list_bottom());
    let next = server_card_rect(n, 0.0);
    assert!(next.y + next.h > server_list_bottom(), "es passt noch eine Karte -- server_rows_visible zu klein");
}

// servers.conf wird beim Start gelesen, ohne dass wir sie geschrieben haben muessen
// (von Hand editiert, halb geschriebene Datei nach einem Absturz). Kaputte Zeilen
// duerfen die uebrigen Eintraege nicht mitreissen.
#[test]
fn parse_servers_ignoriert_kaputte_zeilen() {
    let servers = parse_servers(
        "Hypixel\tmc.hypixel.net\n\
             ohne_tab_also_ungueltig\n\
             \tnamenlos.example.com\n\
             Leere Adresse\t   \n\
             \n\
             Lokal\t127.0.0.1:25566\n",
    );
    let got: Vec<(&str, &str)> = servers.iter().map(|s| (s.name.as_str(), s.address.as_str())).collect();
    assert_eq!(got, vec![("Hypixel", "mc.hypixel.net"), ("", "namenlos.example.com"), ("Lokal", "127.0.0.1:25566")]);
}

#[test]
fn motd_rand_faellt_weg_ausrichtung_bleibt() {
    let span = |t: &str| protocol::nbt::TextSpan { text: t.to_string(), ..Default::default() };
    let mut zeilen = vec![vec![span("      "), span("CatPvP  "), span(" ")], vec![span("    Duels"), span("   ")]];
    trim_motd_padding(&mut zeilen);
    let text: Vec<String> = zeilen.iter().map(|z| z.iter().map(|s| s.text.as_str()).collect()).collect();
    assert_eq!(text, vec!["  CatPvP".to_string(), "Duels".to_string()]);
}

#[test]
fn parse_servers_liest_verbindungsbild_und_namen_aus() {
    let servers = parse_servers("A\ta.net\nB\tb.net\tlast\nC\tc.net\t-\tnoname\nD\td.net\tfirst\tnoname\n");
    let got: Vec<(&str, Option<bool>, bool)> = servers.iter().map(|s| (s.address.as_str(), s.view_last, s.show_name)).collect();
    assert_eq!(got, vec![("a.net", None, true), ("b.net", Some(true), true), ("c.net", None, false), ("d.net", Some(false), false)]);
}

// Das FPS-Limit stand frueher als INDEX in settings.conf ("fps_limit=4" = 5. Eintrag der
// damaligen Liste = 120). Seit man die Zahl frei eintippen kann, steht dort der Hz-Wert
// ("fps_limit_hz") -- eine bestehende Datei muss ihren gewaehlten Deckel trotzdem
// behalten, sonst faenden alle Nutzer nach dem Update wortlos Monitor-Refresh vor.
#[test]
fn altes_fps_limit_wird_auf_den_hz_wert_gehoben() {
    assert_eq!(Settings::parse_onto(Settings::base(), "fps_limit=4\n").fps_limit, 120);
    assert_eq!(Settings::parse_onto(Settings::base(), "fps_limit=0\n").fps_limit, FPS_LIMIT_MONITOR);
    // Neuer Schluessel gewinnt und traegt beliebige Zwischenwerte, die nie in der Liste standen.
    assert_eq!(Settings::parse_onto(Settings::base(), "fps_limit_hz=165\n").fps_limit, 165);
    assert_eq!(Settings::parse_onto(Settings::base(), &format!("fps_limit_hz={FPS_LIMIT_UNLIMITED}\n")).fps_limit, FPS_LIMIT_UNLIMITED);
    // Auch ein absurder Wert bleibt, wie er dasteht.
    assert_eq!(Settings::parse_onto(Settings::base(), "fps_limit_hz=999999\n").fps_limit, 999_999);
}

// Die Pfeile laufen weiterhin durch die Voreinstellungen -- auch von einem frei
// getippten Zwischenwert aus, der in der Liste gar nicht vorkommt. An den Enden gibt es
// keinen Schritt mehr (dort blendet der Pfeil ab, s. buttons()).
#[test]
fn fps_stepper_findet_die_naechste_stufe() {
    assert_eq!(fps_limit_step(90, true), Some(120), "von 90 aufwärts zur nächsten Stufe");
    assert_eq!(fps_limit_step(90, false), Some(75), "von 90 abwaerts zur vorherigen");
    assert_eq!(fps_limit_step(FPS_LIMIT_MONITOR, false), None, "unter Monitor-Refresh gibt es nichts");
    assert_eq!(fps_limit_step(FPS_LIMIT_UNLIMITED, true), None, "ueber unbegrenzt gibt es nichts");
    assert_eq!(fps_limit_step(240, true), Some(FPS_LIMIT_UNLIMITED), "letzte Stufe ist unbegrenzt");
}

// "-1" heisst genau bei den echten Obergrenzen "unbegrenzt". Bei FOV oder
// Empfindlichkeit gibt es keinen unendlichen Wert -- dort darf das Minuszeichen erst
// gar nicht eingetippt werden koennen (s. Tastatur-Zweig in window_event).
#[test]
fn unbegrenzt_gilt_nur_fuer_obergrenzen() {
    for ev in [EditableValue::FpsLimit, EditableValue::EntityRenderDistance] {
        assert!(ev.allows_unlimited());
    }
    for ev in [EditableValue::MaxRenderDistance, EditableValue::Fov(Perspective::FirstPerson), EditableValue::MouseSensitivity, EditableValue::MeshBudget] {
        assert!(!ev.allows_unlimited());
    }
}

// F5 laeuft zyklisch durch alle drei Perspektiven und landet wieder am Anfang -- wie
// Vanillas Perspective.next() (VALUES[(ordinal + 1) % VALUES.length]).
#[test]
fn f5_schaltet_zyklisch_durch_alle_perspektiven() {
    let mut p = Perspective::FirstPerson;
    let mut seen = Vec::new();
    for _ in 0..Perspective::ALL.len() {
        seen.push(p);
        p = p.next();
    }
    assert_eq!(p, Perspective::FirstPerson, "nach einer Runde wieder am Anfang");
    for expected in Perspective::ALL {
        assert!(seen.contains(&expected), "{expected:?} wurde nie erreicht");
    }
    // Genau eine ist Ego, genau eine ist Frontansicht.
    assert_eq!(Perspective::ALL.iter().filter(|p| p.is_first_person()).count(), 1);
    assert_eq!(Perspective::ALL.iter().filter(|p| p.is_front_view()).count(), 1);
}

// Maustasten sind belegbar wie Tasten -- vorher trug Keybinds nur KeyCode, damit war
// eine Maustaste schlicht nicht darstellbar.
#[test]
fn hotbar_blaettern_liegt_standardmaessig_auf_dem_rad() {
    // Auch eine alte settings.conf ohne die beiden Eintraege bekommt das Rad -- sonst
    // liesse sich nach dem Update die Hotbar nicht mehr blaettern.
    let kb = Settings::parse_onto(Settings::base(), "keybind_forward=KeyW\n").keybinds;
    assert_eq!(kb.bind_at(KeyBindAction::HotbarPrev, 0), Some(Bind::Mouse(BindMouse::WheelUp)));
    assert_eq!(kb.bind_at(KeyBindAction::HotbarNext, 0), Some(Bind::Mouse(BindMouse::WheelDown)));
    assert!(KeyBindAction::HotbarPrev.is_press_action() && KeyBindAction::HotbarNext.is_press_action());
}

#[test]
fn maustasten_sind_belegbar_und_ueberleben_die_datei() {
    let mut kb = Keybinds::default();
    kb.set_at(KeyBindAction::Sprint, 0, Bind::Mouse(BindMouse::Back));
    assert_eq!(kb.bind_at(KeyBindAction::Sprint, 0), Some(Bind::Mouse(BindMouse::Back)));
    assert!(kb.is_bound(Bind::Mouse(BindMouse::Back)));
    // Die mittlere Taste waehlt ab Werk den Block aus (Vanillas Mittelklick).
    assert!(kb.is_bound(Bind::Mouse(BindMouse::Middle)));
    assert!(!kb.is_bound(Bind::Mouse(BindMouse::Forward)));

    // Name -> Datei -> Name muss verlustfrei durchlaufen, fuer JEDE Maustaste.
    for button in [BindMouse::Left, BindMouse::Right, BindMouse::Middle, BindMouse::Back, BindMouse::Forward, BindMouse::Other(9), BindMouse::WheelUp, BindMouse::WheelDown] {
        let name = button.name();
        assert_eq!(bind_from_name(&name), Some(Bind::Mouse(button)), "Rundreise fehlgeschlagen für {name}");
    }
    assert_eq!(
        Settings::parse_onto(Settings::base(), "keybind_sprint=Maus zurück\n").keybinds.bind_at(KeyBindAction::Sprint, 0),
        Some(Bind::Mouse(BindMouse::Back))
    );

    // Eine settings.conf aus der Zeit vor den Maustasten enthaelt nur KeyCode-Namen --
    // die muessen unveraendert weiter gelesen werden.
    assert_eq!(Settings::parse_onto(Settings::base(), "keybind_sprint=KeyR\n").keybinds.bind_at(KeyBindAction::Sprint, 0), Some(Bind::Key(KeyCode::KeyR)));
    // Unsinn im Wert laesst den Standard stehen, statt die Aktion unbelegt zu machen.
    assert_eq!(
        Settings::parse_onto(Settings::base(), "keybind_sprint=VoelligerUnsinn\n").keybinds.bind_at(KeyBindAction::Sprint, 0),
        Keybinds::default().bind_at(KeyBindAction::Sprint, 0)
    );
}

// Mehrfachbelegung: eine Aktion kann bis zu MAX_BINDS_PER_ACTION Eingaben tragen, und
// JEDE davon loest sie aus. Vorher ging genau eine.
#[test]
fn mehrere_eingaben_pro_aktion() {
    let mut kb = Keybinds::default();
    kb.set_at(KeyBindAction::Sprint, 1, Bind::Mouse(BindMouse::Back));
    assert_eq!(kb.count(KeyBindAction::Sprint), 2);
    // Beide zaehlen als belegt -- daran haengt, ob der Tastaturpfad das Event verbraucht.
    assert!(kb.is_bound(Bind::Key(KeyCode::ControlLeft)));
    assert!(kb.is_bound(Bind::Mouse(BindMouse::Back)));

    // Bis zum Maximum lassen sich Plaetze anlegen, darueber hinaus nichts mehr.
    let weitere = [KeyCode::KeyH, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL];
    for (slot, taste) in (2..MAX_BINDS_PER_ACTION).zip(weitere) {
        kb.set_at(KeyBindAction::Sprint, slot, Bind::Key(taste));
    }
    assert_eq!(kb.count(KeyBindAction::Sprint), MAX_BINDS_PER_ACTION);
    kb.set_at(KeyBindAction::Sprint, MAX_BINDS_PER_ACTION, Bind::Key(KeyCode::KeyG));
    assert_eq!(kb.count(KeyBindAction::Sprint), MAX_BINDS_PER_ACTION);

    // Loeschen rueckt nach, damit kein Loch entsteht (die Tabelle zeichnet der Reihe nach).
    kb.set_at(KeyBindAction::Forward, 1, Bind::Key(KeyCode::ArrowUp));
    kb.remove_at(KeyBindAction::Forward, 0);
    assert_eq!(kb.bind_at(KeyBindAction::Forward, 0), Some(Bind::Key(KeyCode::ArrowUp)));
    assert_eq!(kb.bind_at(KeyBindAction::Forward, 1), None);

    // Eine Aktion darf auch voellig unbelegt sein.
    kb.remove_at(KeyBindAction::Forward, 0);
    assert_eq!(kb.count(KeyBindAction::Forward), 0);
    assert!(!kb.is_bound(Bind::Key(KeyCode::ArrowUp)));
}

// Modifier: eine Belegung mit zweiter Taste greift nur, wenn beide unten sind; sie ueberlebt
// das Speichern als "Modifier+Taste", eine neue Haupttaste behaelt ihn, Loeschen nimmt ihn mit.
#[test]
fn belegung_mit_modifier_greift_nur_mit_beiden_tasten() {
    let mut kb = Keybinds::default();
    let (e, strg) = (Bind::Key(KeyCode::KeyE), Bind::Key(KeyCode::ControlLeft));
    let a = KeyBindAction::OpenInventory;
    kb.set_modifier_at(a, 0, Some(strg));
    assert_eq!(kb.modifier_at(a, 0), Some(strg));
    assert!(!kb.matches(a, e, |b| b == e), "ohne Strg kein Treffer");
    assert!(kb.matches(a, e, |b| b == e || b == strg));
    assert!(!kb.held(a, |b| b == e));
    assert!(kb.held(a, |b| b == e || b == strg));
    assert!(kb.modifier_released(a, strg, |b| b == e), "Strg zuerst losgelassen: die Aktion endet");
    // Die Taste selbst kann nicht ihr eigener Modifier sein.
    kb.set_modifier_at(a, 0, Some(e));
    assert_eq!(kb.modifier_at(a, 0), Some(strg));
    // Speichern und Lesen; eine alte Zeile ohne "+" bleibt ohne Modifier.
    let text = binds_to_str(&kb, a);
    assert_eq!(text, "ControlLeft+KeyE");
    let mut gelesen = Keybinds::default();
    set_keybind_from_str(&mut gelesen, a, &text);
    assert_eq!((gelesen.bind_at(a, 0), gelesen.modifier_at(a, 0)), (Some(e), Some(strg)));
    set_keybind_from_str(&mut gelesen, a, "KeyR");
    assert_eq!(gelesen.modifier_at(a, 0), None);
    // Neue Haupttaste: der Modifier bleibt. Dieselbe Taste ohne Modifier ist eine eigene Belegung.
    kb.set_at(a, 0, Bind::Key(KeyCode::KeyR));
    assert_eq!(kb.modifier_at(a, 0), Some(strg));
    kb.set_at(a, 1, Bind::Key(KeyCode::KeyR));
    assert_eq!(kb.count(a), 2);
    // Loeschen rueckt nach und nimmt den Modifier mit.
    kb.remove_at(a, 0);
    assert_eq!((kb.bind_at(a, 0), kb.modifier_at(a, 0), kb.count(a)), (Some(Bind::Key(KeyCode::KeyR)), None, 1));
}

// Eine Eingabe darf an mehreren Aktionen haengen (apply_bind loest alle aus). In derselben
// Aktion steht sie nur einmal.
#[test]
fn eine_eingabe_darf_mehreren_aktionen_gehoeren() {
    let mut kb = Keybinds::default();
    let w = Bind::Key(KeyCode::KeyW);
    // W ist Standard fuer Vorwaerts -- zusaetzlich auf Rueckwaerts legen.
    kb.set_at(KeyBindAction::Backward, 1, w);
    assert_eq!(kb.bind_at(KeyBindAction::Backward, 1), Some(w));
    assert!(kb.binds(KeyBindAction::Forward).any(|b| b == w), "W muss bei Vorwärts bleiben");
    // Dieselbe Taste ein zweites Mal in derselben Aktion: wandert, statt doppelt zu stehen.
    kb.set_at(KeyBindAction::Backward, 2, w);
    assert_eq!(kb.binds(KeyBindAction::Backward).filter(|&b| b == w).count(), 1);
}

// settings.conf: mehrere Belegungen kommagetrennt, eine alte Datei mit genau einem Namen
// laeuft unveraendert durch, ein leerer Wert heisst "unbelegt".
#[test]
fn mehrfachbelegung_ueberlebt_die_datei() {
    let parsed = Settings::parse_onto(Settings::base(), "keybind_sprint=ControlLeft,Maus zurück\n").keybinds;
    assert_eq!(parsed.bind_at(KeyBindAction::Sprint, 0), Some(Bind::Key(KeyCode::ControlLeft)));
    assert_eq!(parsed.bind_at(KeyBindAction::Sprint, 1), Some(Bind::Mouse(BindMouse::Back)));

    // Rundreise ueber den Schreib-Formatierer.
    assert_eq!(binds_to_str(&parsed, KeyBindAction::Sprint), "ControlLeft,Maus zurück");

    // Alte Datei: ein Name, keine Kommas.
    assert_eq!(Settings::parse_onto(Settings::base(), "keybind_jump=Space\n").keybinds.count(KeyBindAction::Jump), 1);
    // Leer = bewusst unbelegt, nicht "Standard nehmen".
    assert_eq!(Settings::parse_onto(Settings::base(), "keybind_jump=\n").keybinds.count(KeyBindAction::Jump), 0);
}

// Belegung und H/T- bzw. "Erweitert"-Kaestchen duerfen sich nicht ueberlagern.
#[test]
fn keybind_felder_ueberlagern_sich_nicht() {
    let feld = keybind_rect(0);
    for wartet in [false, true] {
        assert!(feld.x + feld.w < keybind_mode_rect(0, wartet).x, "Belegung läuft ins Kästchen daneben");
    }
}

// Jede Aktion steht genau einmal auf der Steuerungs-Seite, jede Einstellung ebenso, und
// die Zeilen passen in die Abschnitts-Bitmaske (128 Bit, s. SettingsPage::section_starts).
#[test]
fn steuerungs_seite_zeigt_jede_aktion_genau_einmal() {
    for aktion in KeyBindAction::ALL {
        let n = controls_rows().iter().filter(|&&z| z == ControlsRow::Bind(aktion)).count();
        assert_eq!(n, 1, "{} steht {n}-mal auf der Seite", aktion.label());
    }
    for (i, a) in controls_rows().iter().enumerate() {
        assert!(!controls_rows()[i + 1..].contains(a), "Zeile {i} doppelt");
    }
    assert!(controls_rows().len() < 128);
    let abschnitte = controls_sections();
    assert_eq!(abschnitte[0], ("Maus", 0));
    assert!(abschnitte.windows(2).all(|w| w[0].1 < w[1].1), "Abschnitte in Reihenfolge, keiner leer");
}

// Lesbare Tastennamen: "A" statt "KeyA", "1" statt "Digit1", ausgeschriebene Sondertasten.
#[test]
fn tastennamen_sind_lesbar() {
    assert_eq!(bind_display(Bind::Key(KeyCode::KeyA)), "A");
    assert_eq!(bind_display(Bind::Key(KeyCode::Digit1)), "1");
    assert_eq!(bind_display(Bind::Key(KeyCode::ControlLeft)), "Strg links");
    assert_eq!(bind_display(Bind::Key(KeyCode::Numpad5)), "Num 5");
    assert_eq!(bind_display(Bind::Key(KeyCode::F5)), "F5");
    assert_eq!(bind_display(Bind::Mouse(BindMouse::Back)), "Maustaste 4");
}

// Die beiden Eingabewege (winit im Fenster, evdev im Raw-Pfad) muessen dieselbe
// Maustaste auf denselben Bind abbilden -- sonst belegte man sie im Menue und sie taete
// im Spiel nichts, weil der Raw-Pfad einen anderen Wert liefert. evdev nennt die
// Daumentasten Side/Extra, winit Back/Forward.
#[test]
fn winit_und_evdev_liefern_denselben_maus_bind() {
    let paare = [
        (MouseButton::Left, raw_input::MouseButton::Left),
        (MouseButton::Right, raw_input::MouseButton::Right),
        (MouseButton::Middle, raw_input::MouseButton::Middle),
        (MouseButton::Back, raw_input::MouseButton::Side),
        (MouseButton::Forward, raw_input::MouseButton::Extra),
    ];
    for (winit, raw) in paare {
        assert_eq!(BindMouse::from_winit(winit), Some(BindMouse::from_raw(raw)), "{winit:?} und {raw:?} müssen zusammenfallen");
    }
}

// Der Perspektivwechsel ist eine normale, umbelegbare Aktion -- vorher hing er als
// fest verdrahtete F5-Abfrage in window_event und tauchte in der Tastenbelegung nicht
// auf. F5 bleibt der Standard.
// Die Warteschlange ist der Kern des Rechtsklicks: zwei Klicks innerhalb eines Ticks
// duerfen nicht zu einem verschmelzen. Vanilla feuert BEIDE im SELBEN Tick ab, und genau
// daran haengt der Anchor-Airplace -- beide Klicks tragen dann dieselbe Zielposition.
#[test]
fn zwei_klicks_in_einem_tick_ergeben_zwei_benutzungen() {
    // Zwei gemerkte Klicks, Taste nicht mehr gehalten.
    let (anzahl, cd) = item_uses_this_tick(2, 0, false, true);
    assert_eq!(anzahl, 2, "beide Klicks gehören in DIESEN Tick (Vanillas while-Schleife)");
    assert_eq!(cd, ITEM_USE_COOLDOWN_TICKS);

    // Danach ist Ruhe, solange die Taste nicht gehalten wird.
    assert_eq!(item_uses_this_tick(0, cd, false, true).0, 0);
}

// Die Gegenprobe zum Airplace: vier schnelle Klicks bleiben vier Benutzungen und werden
// nicht ueber vier Ticks verteilt. Frueher lieferte diese Stelle genau einen pro Tick.
#[test]
fn gemerkte_klicks_werden_nicht_auf_mehrere_ticks_verteilt() {
    assert_eq!(item_uses_this_tick(4, 0, false, true).0, 4);
    // Auch waehrend eines laufenden Cooldowns -- Vanillas doItemUse prueft ihn nicht.
    assert_eq!(item_uses_this_tick(3, 3, false, true).0, 3);
}

// Gehaltene Taste: Vanillas zweiter Zweig, alle 4 Ticks einmal.
#[test]
fn gehaltene_taste_feuert_im_vanilla_takt() {
    let mut cooldown = 0;
    let mut treffer = Vec::new();
    for tick in 0..12 {
        let (anzahl, cd) = item_uses_this_tick(0, cooldown, true, true);
        cooldown = cd;
        if anzahl > 0 {
            treffer.push(tick);
        }
    }
    assert_eq!(treffer, vec![0, 4, 8], "gehalten platziert Vanilla alle {ITEM_USE_COOLDOWN_TICKS} Ticks");
}

#[test]
fn ohne_taste_und_ohne_schlange_passiert_nichts() {
    assert_eq!(item_uses_this_tick(0, 0, false, true), (0, 0));
}

// Abgeschaltetes Wiederholen (s. Settings::hold_use_repeat): aus dem Halten wird EIN
// Klick. Der erste Tick feuert noch -- der Klick, der die Taste gedrueckt hat, ist ja
// gewollt -- und danach ist Ruhe, solange man haelt.
#[test]
fn ohne_wiederholung_setzt_gehaltenes_druecken_nur_einmal() {
    let mut cooldown = 0;
    let mut treffer = Vec::new();
    for tick in 0..12 {
        // Die Taste wird ueber den ersten Tick hinaus gehalten; gefeuert wird nur der
        // gemerkte Klick vom Tastendruck.
        let schlange = u32::from(tick == 0);
        let (anzahl, cd) = item_uses_this_tick(schlange, cooldown, true, false);
        cooldown = cd;
        if anzahl > 0 {
            treffer.push(tick);
        }
    }
    assert_eq!(treffer, vec![0], "ohne Wiederholung darf Halten nur den einen Klick setzen");
}

// Der Schalter darf NUR das Halten betreffen. Gemerkte Klicks sind echte Tastendruecke --
// wer viermal klickt, will viermal setzen, egal wie der Schalter steht.
#[test]
fn abgeschaltete_wiederholung_verschluckt_keine_echten_klicks() {
    assert_eq!(item_uses_this_tick(2, 0, false, false).0, 2, "beide gemerkten Klicks gehören dem Nutzer, nicht der Wiederholung");
}

// Zwei Slottasten im selben Tick: der erste gilt sofort, der zweite wartet einen Tick --
// und ein Klick dazwischen gehoert dem ERSTEN Slot, einer danach dem zweiten.
#[test]
fn slottasten_werden_der_reihe_nach_abgearbeitet() {
    let mut q = SlotQueue::default();
    assert_eq!(q.press(2, true), Some(2), "freie Bahn: sofort");
    // Slot 2 ist noch nicht gesendet -> Slot 4 wartet.
    assert_eq!(q.press(4, false), None);
    assert!(q.add_use(4), "der Klick nach Slot 4 hängt an Slot 4");
    assert!(q.add_action(PendingInputAction::Attack));
    assert_eq!(q.next_for_tick(false), None, "Slot 2 hatte seinen Tick noch nicht");
    assert_eq!(q.next_for_tick(true), Some(QueuedSlot { slot: 4, uses: 1, actions: vec![PendingInputAction::Attack] }));
    assert!(!q.add_use(4), "leere Schlange: Klicks gelten dem aktuellen Slot");
}

// Waehrend der Server noch isst: Durchscrollen ueber drei Slots ergibt EINEN vorgemerkten
// Wechsel; haengt an ihm schon ein Klick, beginnt ein neuer Eintrag.
#[test]
fn mausrad_waehrend_des_essens_landet_in_der_schlange() {
    let mut q = SlotQueue::default();
    q.scroll(0, false);
    q.scroll(0, false);
    q.scroll(0, false);
    assert_eq!(q.entries.len(), 1);
    assert_eq!(q.entries[0].slot, 3);
    q.add_use(4);
    q.scroll(0, true);
    assert_eq!(q.entries.iter().map(|e| e.slot).collect::<Vec<_>>(), vec![3, 2]);
    let mut q = SlotQueue::default();
    q.scroll(0, true);
    assert_eq!(q.entries[0].slot, 8, "von Slot 1 nach links ist Slot 9");
}

#[test]
fn slotschlange_ist_gedeckelt() {
    let mut q = SlotQueue::default();
    for i in 0..10 {
        q.press(i % 9, false);
    }
    assert_eq!(q.entries.len(), SlotQueue::MAX);
}

#[test]
fn wiederholen_ist_vorgabe_wie_in_vanilla() {
    assert_eq!(Settings::base().hold_use_repeat, HoldUseRepeat::Always);
    // Alte Schalter-Werte werden weiter verstanden.
    assert_eq!(Settings::parse_onto(Settings::base(), "hold_use_repeat=false\n").hold_use_repeat, HoldUseRepeat::Never);
    assert_eq!(Settings::parse_onto(Settings::base(), "hold_use_repeat=true\n").hold_use_repeat, HoldUseRepeat::Always);
    for modus in [HoldUseRepeat::Always, HoldUseRepeat::ExceptBlocks, HoldUseRepeat::Never] {
        let set = Settings { hold_use_repeat: modus, ..Default::default() };
        assert_eq!(Settings::parse_onto(Settings::base(), &set.serialize()).hold_use_repeat, modus, "{modus:?} muss das Speichern überleben");
    }
}

#[test]
fn perspektivwechsel_ist_ein_umbelegbarer_keybind() {
    assert!(KeyBindAction::ALL.contains(&KeyBindAction::TogglePerspective));
    assert_eq!(Keybinds::default().bind_at(KeyBindAction::TogglePerspective, 0), Some(Bind::Key(KeyCode::F5)));

    // Umbelegen und wieder auslesen -- und der Wert muss die Datei ueberleben.
    let mut kb = Keybinds::default();
    kb.set_at(KeyBindAction::TogglePerspective, 0, Bind::Key(KeyCode::KeyP));
    assert_eq!(kb.bind_at(KeyBindAction::TogglePerspective, 0), Some(Bind::Key(KeyCode::KeyP)));
    assert_eq!(
        Settings::parse_onto(Settings::base(), "keybind_toggle_perspective=KeyP\n").keybinds.bind_at(KeyBindAction::TogglePerspective, 0),
        Some(Bind::Key(KeyCode::KeyP))
    );

    // Einmal-Aktion: kein Halten/Umschalten (das waere zweimal dasselbe) und kein
    // Dauerzustand -- deshalb ist der Modus-Knopf im Steuerung-Tab abgeblendet. KEINE
    // Einmal-Aktionen sind die sieben Bewegungs-/Haltezustaende und das
    // Stapel-Verschieben: letzteres ist ein Modifikator, der beim Klick ABGEFRAGT wird,
    // also einen Zustand halten muss (und darum auch umschaltbar sein darf).
    let hold_actions = [
        KeyBindAction::Forward,
        KeyBindAction::Backward,
        KeyBindAction::Left,
        KeyBindAction::Right,
        KeyBindAction::Jump,
        KeyBindAction::Sneak,
        KeyBindAction::Sprint,
        KeyBindAction::InventoryMoveStack,
        // Rechtsklick haelt ebenfalls einen Zustand: gehalten platziert er im
        // Vanilla-Takt weiter (s. App::tick_item_use).
        KeyBindAction::Use,
        KeyBindAction::PlayerList,
        KeyBindAction::Zoom,
        KeyBindAction::Freelook,
        // Sprachchat: gesendet wird, solange sie gehalten ist.
        KeyBindAction::PushToTalk,
        KeyBindAction::InspectItem,
        KeyBindAction::VoiceWhisper,
        // Spielmodus-Rad: offen, solange gehalten.
        KeyBindAction::GameModeWheel,
        KeyBindAction::Reconnect,
    ];
    for action in KeyBindAction::ALL {
        let expected_press = !hold_actions.contains(&action);
        assert_eq!(action.is_press_action(), expected_press, "{}: falsche Aktionsart", action.label());
    }
    // Die Inventar-Aktionen haben ab Werk KEINE eigene Taste: ohne Belegung gilt die
    // ihres Welt-Gegenstuecks (Q wirft auch im Inventar, Shift verschiebt dort Stapel).
    // Waeren sie fest doppelt belegt, loeste ein Druck zwei Dinge aus, sobald jemand die
    // Taste noch fuer etwas anderes vergibt (etwa Linksshift zusaetzlich
    // auf Hotbar-Platz 1: Item wurde geworfen UND getauscht).
    let standard = Keybinds::default();
    for (welt, inventar, taste) in [
        (KeyBindAction::Sneak, KeyBindAction::InventoryMoveStack, KeyCode::ShiftLeft),
        (KeyBindAction::DropItem, KeyBindAction::InventoryDropItem, KeyCode::KeyQ),
    ] {
        assert!(standard.binds(welt).any(|b| b == Bind::Key(taste)), "{}: Standardtaste fehlt", welt.label());
        assert_eq!(standard.count(inventar), 0, "{}: darf ab Werk keine eigene Taste haben", inventar.label());
        assert!(inventar.inventory_fallback() == Some(welt), "{}: falsches Gegenstück", inventar.label());
    }

    // Eine eigene Inventar-Belegung nimmt der Welt-Aktion nichts weg.
    let mut kb = Keybinds::default();
    kb.set_at(KeyBindAction::InventoryMoveStack, 0, Bind::Key(KeyCode::ShiftLeft));
    assert!(kb.binds(KeyBindAction::Sneak).any(|b| b == Bind::Key(KeyCode::ShiftLeft)), "Schleichen darf nichts verlieren");
    assert_eq!(kb.count(KeyBindAction::InventoryMoveStack), 1);

    // Verteilen-Ziehen: Vanillas Bitpackung. Ein Dreher hier verteilt still nichts mehr.
    assert_eq!(quick_craft_button(0, 0), 0, "links, Anfang");
    assert_eq!(quick_craft_button(1, 0), 1, "links, Slot");
    assert_eq!(quick_craft_button(2, 0), 2, "links, Ende");
    assert_eq!(quick_craft_button(0, 1), 4, "rechts, Anfang");
    assert_eq!(quick_craft_button(1, 1), 5, "rechts, Slot");
    assert_eq!(quick_craft_button(2, 1), 6, "rechts, Ende");

    // Jede Aktion braucht einen eigenen Konfig-Schluessel, sonst ueberschreiben sich
    // zwei Belegungen beim Speichern gegenseitig.
    let mut keys: Vec<&str> = KeyBindAction::ALL.iter().map(|a| a.settings_key()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), KeyBindAction::ALL.len(), "zwei Aktionen teilen sich einen Konfig-Schlüssel");
    // ... und die neuen Belegungen muessen den Weg durch settings.conf ueberleben.
    let saved = Settings::parse_onto(Settings::base(), "keybind_hotbar_5=KeyB\nkeybind_drop_item=KeyG\n");
    assert_eq!(saved.keybinds.bind_at(KeyBindAction::Hotbar(5), 0), Some(Bind::Key(KeyCode::KeyB)));
    assert_eq!(saved.keybinds.bind_at(KeyBindAction::DropItem, 0), Some(Bind::Key(KeyCode::KeyG)));
    // Alle Aktionen brauchen einen eigenen, stabilen Index (fuer keybind_physical_down).
    let mut indices: Vec<usize> = KeyBindAction::ALL.iter().map(|a| a.index()).collect();
    indices.sort_unstable();
    indices.dedup();
    assert_eq!(indices.len(), KeyBindAction::ALL.len(), "zwei Aktionen teilen sich einen Index");
    assert!(indices.iter().all(|i| *i < KeyBindAction::ALL.len()), "Index außerhalb von keybind_physical_down");
}

// Jede Perspektive hat ihr EIGENES Sichtfeld -- ein Wert darf die anderen nicht mitziehen.
// Und jede belegt eine eigene Zeile im Video-Tab, sonst lieferten zwei Stepper auf
// dieselbe Einstellung.
#[test]
fn jede_perspektive_hat_ein_eigenes_sichtfeld() {
    let mut set = Settings::base();
    for (i, p) in Perspective::ALL.into_iter().enumerate() {
        set.set_fov_of(p, 80.0 + i as f32 * 10.0);
    }
    for (i, p) in Perspective::ALL.into_iter().enumerate() {
        assert_eq!(set.fov_of(p), 80.0 + i as f32 * 10.0, "{p:?} wurde von einer anderen Perspektive überschrieben");
    }
    // Klemmen gilt fuer alle drei gleichermassen.
    for p in Perspective::ALL {
        set.set_fov_of(p, 999.0);
        assert_eq!(set.fov_of(p), FOV_MAX);
        set.set_fov_of(p, 0.0);
        assert_eq!(set.fov_of(p), FOV_MIN);
    }
    // Linke Spalte: Sichtfeld je Perspektive, danach Projektion und Platzhalter. Die
    // Kamerazeilen liegen in der RECHTEN Spalte und duerfen sich dort untereinander
    // nicht ueberlagern -- links und rechts derselben Zeilennummer ist dagegen normal.
    let left: Vec<usize> = Perspective::ALL.iter().map(|p| p.fov_row()).collect();
    assert!(!left.contains(&PROJECTION_ROW), "keine Sichtfeld-Zeile darf auf der Projektionszeile liegen");
    let mut sorted = left.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), left.len(), "zwei Perspektiven teilen sich eine Sichtfeld-Zeile");

    let mut right: Vec<usize> = Vec::new();
    for p in Perspective::ALL {
        right.extend(p.distance_row());
        right.extend(p.y_offset_row());
    }
    let mut sorted_right = right.clone();
    sorted_right.sort_unstable();
    sorted_right.dedup();
    assert_eq!(sorted_right.len(), right.len(), "zwei Kamerazeilen überlagern sich");

    // Nur die 3.-Person-Ansichten haben ueberhaupt Kamerazeilen.
    assert_eq!(Perspective::FirstPerson.distance_row(), None);
    assert_eq!(Perspective::FirstPerson.y_offset_row(), None);
    assert!(Perspective::THIRD_PERSON.iter().all(|p| p.distance_row().is_some() && p.y_offset_row().is_some()));

    // Die letzte Platzhalter-Zeile muss auf einen 1080p-Bildschirm passen -- vier weitere
    // LINKE Zeilen haetten sie darueber hinausgeschoben, deshalb liegen die Kamerawerte
    // rechts (s. Perspective::distance_row).
    let last = settings_row_y(CHUNK_FADE_ROW) + SETTINGS_CTRL_H;
    assert!(last < 1080.0, "Video-Tab läuft unten aus dem Bild (letzte Zeile endet bei {last})");
}

// LOD-Render-Distance: eigene Einstellung im Video-Tab, ueberlebt settings.conf, und eine
// alte settings.conf (Fernsicht hing am Modul und an "Fernsicht zeichnen") wird uebernommen.
// Die Anpassung der eigenen Partikel zeigt nur Arten, die an der eigenen Figur entstehen
// koennen; je Art zwei Zeilen und ein Abschnitt (Trennlinie mit Namen), ohne Reiter.
#[test]
fn eigene_partikel_nur_was_an_einem_selbst_entsteht() {
    let eigene: Vec<&str> = particles::particle_kinds_for_own(true).iter().map(|(_, k)| k.key).collect();
    for k in ["crit", "totem", "potion", "potion_splash", "damage_indicator", "gust", "block"] {
        assert!(eigene.contains(&k), "{k} fehlt");
    }
    for k in ["bubbles", "end_rod", "explosion", "flame", "smoke", "dust", "sculk_soul", "cloud"] {
        assert!(!eigene.contains(&k), "{k} gehoert nicht zu den eigenen");
    }
    assert_eq!(particles::particle_kinds_for_own(false).len(), particles::KINDS.len());
    for own in [false, true] {
        let abschnitte = particle_sections(own);
        assert_eq!(abschnitte.len(), particles::particle_kinds_for_own(own).len());
        assert!(abschnitte.iter().enumerate().all(|(i, &(_, row))| row == 2 * i));
    }
}

#[test]
fn partikel_anpassung_wird_gespeichert_und_alter_explosionsschalter_uebernommen() {
    let mut set = Settings::parse_onto(Settings::base(), "");
    assert!(set.particle_tuning.iter().all(|t| *t == particles::ParticleTuning::default()));
    let herzen = particles::KINDS.iter().position(|k| k.key == "damage_indicator").unwrap();
    set.particle_tuning[herzen] =
        particles::ParticleTuning { enabled: false, lifetime_pct: 200, opacity_pct: 40, speed_pct: 50, amount_pct: 25, interval_pct: 100, size_pct: 150, spread_pct: 50, all_at_once: true };
    let text = set.serialize();
    assert!(text.contains("particle_tuning=damage_indicator:0,200,40,50,25,100,1,150,50\n"), "nur Abweichungen: {text}");
    assert_eq!(Settings::parse_onto(Settings::base(), &text).particle_tuning, set.particle_tuning);
    // Alte Zeile mit vier Werten: "potion" galt damals auch fuer die Spritzer.
    let alt = Settings::parse_onto(Settings::base(), "particle_tuning=potion:1,50,100,100\n");
    let art = |key: &str| particles::KINDS.iter().position(|k| k.key == key).unwrap();
    assert_eq!(alt.particle_tuning[art("potion")].lifetime_pct, 50);
    assert_eq!(alt.particle_tuning[art("potion_splash")].lifetime_pct, 50);
    assert_eq!(alt.particle_tuning[art("potion")].amount_pct, 100);
    // Vor der Trennung in eigene und andere Partikel galt eine Anpassung fuer beide.
    assert_eq!(alt.particle_tuning_own, alt.particle_tuning);
    let alt = Settings::parse_onto(Settings::base(), "particles_mode=2\n");
    assert_eq!((alt.particles_mode, alt.particles_mode_own), (2, 2));
    // Getrennt eingestellt: beide ueberleben das Speichern, die eigenen ohne die der anderen.
    let mut getrennt = Settings::parse_onto(Settings::base(), "");
    getrennt.particles_mode_own = 1;
    getrennt.particle_tuning_own[herzen].enabled = false;
    let gelesen = Settings::parse_onto(Settings::base(), &getrennt.serialize());
    assert_eq!((gelesen.particles_mode, gelesen.particles_mode_own), (0, 1));
    assert_eq!(gelesen.particle_tuning_own, getrennt.particle_tuning_own);
    assert!(gelesen.particle_tuning[herzen].enabled);
    let alt = Settings::parse_onto(Settings::base(), "explosion_particles=false\n");
    assert!(!alt.particle_tuning[particles::KIND_EXPLOSION].enabled);
    assert!(alt.particle_tuning[0].enabled);
    assert_eq!(step_in(&PARTICLE_SPEED_STEPS, 100, false), 75);
    assert_eq!(step_in(&PARTICLE_SPEED_STEPS, 400, true), 400);
    assert_eq!(step_in(&PARTICLE_SPEED_STEPS, 110, true), 125);
}

#[test]
fn lod_render_distance_wird_gespeichert_und_uebernommen() {
    let mut set = Settings::base();
    assert_eq!(set.lod_render_distance, lod::DISTANCE_OFF, "ohne Zutun keine Fernsicht");
    set.lod_render_distance = 256;
    assert_eq!(Settings::parse_onto(Settings::base(), &set.serialize()).lod_render_distance, 256);
    set.lod_render_distance = lod::DISTANCE_OFF;
    set.module_chunk_saver = true;
    let wieder = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(wieder.lod_render_distance, lod::DISTANCE_OFF, "aus bleibt aus, auch mit Modul");
    assert!(wieder.module_chunk_saver);

    let alt = |text: &str| Settings::parse_onto(Settings::base(), text).lod_render_distance;
    assert_eq!(alt("module_chunk_saver=true\nchunk_saver_render=true\nchunk_saver_distance=320\n"), 320);
    assert_eq!(alt("module_chunk_saver=true\n"), lod::DISTANCE_LEGACY_DEFAULT);
    assert_eq!(alt("module_chunk_saver=true\nchunk_saver_render=false\nchunk_saver_distance=320\n"), lod::DISTANCE_OFF);
    assert_eq!(alt("module_chunk_saver=false\nchunk_saver_distance=320\n"), lod::DISTANCE_OFF);

    // Stufen: aus, 32er-Schritte, das Maximum 650 als eigene letzte Stufe.
    assert_eq!(lod::distance_step(lod::DISTANCE_OFF, true), 32);
    assert_eq!(lod::distance_step(32, false), lod::DISTANCE_OFF);
    assert_eq!(lod::distance_step(640, true), lod::DISTANCE_MAX);
    assert_eq!(lod::distance_step(lod::DISTANCE_MAX, false), 640);
    assert_eq!(lod::distance_step(lod::DISTANCE_OFF, false), lod::DISTANCE_OFF);
}

// Beide 3.-Person-Abstaende sind getrennt einstellbar, werden auf Vanillas
// Attributbereich (0..32) geklemmt und ueberleben einzeln in settings.conf.
#[test]
fn kameraabstaende_sind_getrennt_und_geklemmt() {
    let mut set = Settings::base();
    set.set_camera_distance_of(Perspective::ThirdPersonBack, 6.5);
    set.set_camera_distance_of(Perspective::ThirdPersonFront, 2.0);
    assert_eq!(set.camera_distance_of(Perspective::ThirdPersonBack), 6.5);
    assert_eq!(set.camera_distance_of(Perspective::ThirdPersonFront), 2.0);
    for p in Perspective::THIRD_PERSON {
        set.set_camera_distance_of(p, 999.0);
        assert_eq!(set.camera_distance_of(p), CAMERA_DISTANCE_MAX);
        set.set_camera_distance_of(p, -5.0);
        assert_eq!(set.camera_distance_of(p), CAMERA_DISTANCE_MIN);
    }
    // Die Ego-Ansicht hat keinen Abstand -- Setzen laeuft ins Leere, Lesen liefert 0.
    set.set_camera_distance_of(Perspective::FirstPerson, 10.0);
    assert_eq!(set.camera_distance_of(Perspective::FirstPerson), 0.0);

    // Hoehenversatz: eigene Werte, eigener Bereich, und Minus ist hier ERLAUBT (anders
    // als bei den Obergrenzen bedeutet "-1" einen Block tiefer, nicht "unbegrenzt").
    set.set_camera_y_offset_of(Perspective::ThirdPersonBack, 1.25);
    set.set_camera_y_offset_of(Perspective::ThirdPersonFront, -0.75);
    assert_eq!(set.camera_y_offset_of(Perspective::ThirdPersonBack), 1.25);
    assert_eq!(set.camera_y_offset_of(Perspective::ThirdPersonFront), -0.75);
    for p in Perspective::THIRD_PERSON {
        set.set_camera_y_offset_of(p, 99.0);
        assert_eq!(set.camera_y_offset_of(p), CAMERA_Y_OFFSET_MAX);
        set.set_camera_y_offset_of(p, -99.0);
        assert_eq!(set.camera_y_offset_of(p), CAMERA_Y_OFFSET_MIN);
    }
    assert_eq!(set.camera_y_offset_of(Perspective::FirstPerson), 0.0);
    for p in Perspective::THIRD_PERSON {
        assert!(EditableValue::CameraYOffset(p).allows_negative(), "Minus muss eintippbar sein");
        assert!(!EditableValue::CameraYOffset(p).allows_unlimited(), "-1 ist hier ein echter Wert, kein 'unbegrenzt'");
    }

    let parsed =
        Settings::parse_onto(Settings::base(), "camera_distance_back=7.5\ncamera_distance_front=1.5\ncamera_y_offset_back=2.0\ncamera_y_offset_front=-1.5\n");
    assert_eq!(parsed.camera_y_offset_back, 2.0);
    assert_eq!(parsed.camera_y_offset_front, -1.5);
    assert_eq!(parsed.camera_distance_back, 7.5);
    assert_eq!(parsed.camera_distance_front, 1.5);
    // Alte Datei ohne die Schluessel bleibt auf Vanillas 4.0.
    assert_eq!(Settings::parse_onto(Settings::base(), "fov=90\n").camera_distance_back, THIRD_PERSON_DISTANCE);
}

// Die drei Sichtfelder muessen einzeln in settings.conf ueberleben. Frueher gab es nur
// "fov" -- eine alte Datei behaelt dessen Wert fuer den Ego-Modus, die beiden neuen
// starten auf ihrem Standard.
#[test]
fn sichtfelder_ueberleben_das_speichern_einzeln() {
    let parsed = Settings::parse_onto(Settings::base(), "fov=100\nfov_third_back=120\nfov_third_front=45\n");
    assert_eq!(parsed.fov, 100.0);
    assert_eq!(parsed.fov_third_back, 120.0);
    assert_eq!(parsed.fov_third_front, 45.0);
    // Alte Datei ohne die neuen Schluessel: Ego uebernimmt, der Rest bleibt Standard.
    let old = Settings::parse_onto(Settings::base(), "fov=95\n");
    assert_eq!(old.fov, 95.0);
    assert_eq!(old.fov_third_back, Settings::base().fov_third_back);
}

// Zwei Wertfelder duerfen sich nie ueberlappen -- sonst faenge ein Klick das falsche
// (oder gar keines) ab. Betrifft nur Felder auf DEMSELBEN Screen; verschiedene Screens
// teilen sich dieselben Zeilen-Rechtecke voellig legitim.
#[test]
fn editierbare_wertfelder_ueberlappen_sich_nicht() {
    for (i, a) in EditableValue::ALL.iter().enumerate() {
        for b in &EditableValue::ALL[i + 1..] {
            if a.screen() != b.screen() {
                continue;
            }
            // Dicke und Deckkraft eines Kaestchens teilen sich die Stelle, sichtbar ist nur eins.
            if let (EditableValue::ShadowThickness(i), EditableValue::ShadowOpacity(j)) = (a, b) {
                if i == j {
                    continue;
                }
            }
            let (ra, rb) = (a.rect(), b.rect());
            let overlaps = ra.x < rb.x + rb.w && rb.x < ra.x + ra.w && ra.y < rb.y + rb.h && rb.y < ra.y + ra.h;
            assert!(!overlaps, "zwei Wertfelder auf demselben Screen überlappen sich");
        }
    }
}

// Eine kaputte oder von Hand editierte Datei darf nichts umwerfen: unbekannte
// Schluessel werden ignoriert, unmoegliche Werte auf den gueltigen Bereich geklemmt.
#[test]
fn parse_klemmt_unsinnige_werte() {
    let settings = Settings::parse_onto(Settings::base(), 
        "show_tearing_test=true\n\
             show_hud=false\n\
             fps_limit=99\n\
             mouse_sensitivity=5.0\n\
             mesh_columns_per_frame=0\n\
             present_mode=GibtsNicht\n\
             ui_pitch_semitones=999\n\
             voellig_unbekannt=42\n\
             kaputte zeile ohne gleichheitszeichen\n",
    );

    assert!(settings.show_tearing_test);
    assert!(!settings.show_hud);
    // "fps_limit" ist der ALTE Schluessel (Index in die frueher feste Liste). 99 liegt
    // ausserhalb -> Monitor-Refresh statt eines Absturzes beim Indizieren.
    assert_eq!(settings.fps_limit, FPS_LIMIT_MONITOR, "unmöglicher Alt-Index fällt auf Monitor-Refresh");
    assert!(settings.mouse_sensitivity > SENSITIVITY_MAX, "Empfindlichkeit bleibt, wie gespeichert");
    // Tonhoehe wird bewusst NICHT geklemmt (s. Kommentar an Settings::ui_pitch_semitones)
    // -- 999 ist ein extremer, aber gueltiger Wert und muss unveraendert uebernommen werden.
    assert_eq!(settings.ui_pitch_semitones, 999);
    assert_eq!(settings.mesh_columns_per_frame, 0, "bleibt, wie gespeichert");
}

fn team(sicht: u8) -> TeamInfo {
    TeamInfo { prefix: String::new(), suffix: String::new(), name_tag_visibility: sicht, color: -1, collision_rule: 0 }
}

// Die Regel, die das doppelte Schild verursacht hat: Server mit eigenen Rang-Hologrammen
// stellen das Vanilla-Namensschild auf NEVER.
#[test]
fn team_auf_never_unterdrueckt_den_namen() {
    let t = team(TEAM_VISIBILITY_NEVER);
    assert!(!team_allows_name(Some(&t), None, Some("rot")));
    assert!(!team_allows_name(Some(&t), Some("rot"), Some("rot")), "auch im eigenen Team nicht");
}

#[test]
fn ohne_team_bleibt_der_name_sichtbar() {
    assert!(team_allows_name(None, None, None));
    assert!(team_allows_name(Some(&team(TEAM_VISIBILITY_ALWAYS)), Some("blau"), Some("rot")));
}

#[test]
// "hideForOtherTeams": nur Mitglieder desselben Teams sehen den Namen.
fn hide_for_other_teams_zeigt_nur_den_eigenen_leuten() {
    let t = team(TEAM_VISIBILITY_HIDE_FOR_OTHER_TEAMS);
    assert!(team_allows_name(Some(&t), Some("rot"), Some("rot")), "eigenes Team sieht ihn");
    assert!(!team_allows_name(Some(&t), Some("blau"), Some("rot")), "fremdes Team nicht");
    // Ohne eigenes Team zaehlt Vanilla das als sichtbar (abstractTeam2 == null -> bl).
    assert!(team_allows_name(Some(&t), None, Some("rot")));
}

#[test]
fn hide_for_own_team_ist_genau_umgekehrt() {
    let t = team(TEAM_VISIBILITY_HIDE_FOR_OWN_TEAM);
    assert!(!team_allows_name(Some(&t), Some("rot"), Some("rot")));
    assert!(team_allows_name(Some(&t), Some("blau"), Some("rot")));
    assert!(team_allows_name(Some(&t), None, Some("rot")));
}

// Vanillas Regeln fuer Spielernamen (s. player_label_visible). Genau diese vier fehlten,
// und jede einzelne war im Spiel als Fehler sichtbar.
#[test]
fn der_eigene_name_wird_nie_gezeichnet() {
    assert!(!player_label_visible(true, false, false, 1.0));
}

#[test]
// Der Fall: ein unsichtbarer Spieler, dessen Name frei in der Luft hing.
fn ein_unsichtbarer_spieler_traegt_keinen_namen() {
    assert!(!player_label_visible(false, true, false, 1.0));
}

#[test]
fn ueber_vierundsechzig_bloecken_endet_der_name() {
    assert!(player_label_visible(false, false, false, 63.0 * 63.0));
    assert!(!player_label_visible(false, false, false, 64.0 * 64.0), "genau 64 ist schon zu weit");
    assert!(!player_label_visible(false, false, false, 100.0 * 100.0));
}

#[test]
// Schleichend halbiert Vanilla die Sichtweite -- ein Gegner im Busch soll sich nicht
// ueber die halbe Karte verraten.
fn schleichend_ist_bei_zweiunddreissig_schluss() {
    assert!(player_label_visible(false, false, true, 31.0 * 31.0));
    assert!(!player_label_visible(false, false, true, 33.0 * 33.0));
    // Stehend ist dieselbe Entfernung noch in Ordnung.
    assert!(player_label_visible(false, false, false, 33.0 * 33.0));
}

// Eine feste Breite je Zeichen -- damit sind die Umbrueche im Test nachrechenbar,
// ohne Font-Atlas und ohne Fenster.
fn mono(text: &str) -> f32 {
    text.chars().count() as f32
}

#[test]
fn umbruch_bricht_an_wortgrenzen() {
    let zeilen = wrap_measured(mono, "eins zwei drei vier", 10.0);
    assert_eq!(zeilen, vec!["eins zwei", "drei vier"]);
}

// Ein Wort, das allein schon zu breit ist, MUSS hart zerlegt werden -- sonst laeuft es
// aus dem Bild (etwa eine JSON-Kick-Meldung, die rechts aus dem Fenster lief).
#[test]
fn ein_zu_langes_wort_wird_hart_zerlegt() {
    let zeilen = wrap_measured(mono, "abcdefghij", 4.0);
    assert_eq!(zeilen, vec!["abcd", "efgh", "ij"]);
    for z in &zeilen {
        assert!(mono(z) <= 4.0, "Zeile zu breit: {z}");
    }
}

// Und im Verbund: kurze Woerter davor und danach gehen nicht verloren.
#[test]
fn kurzes_und_langes_wort_zusammen() {
    let zeilen = wrap_measured(mono, "ab abcdefghij cd", 4.0);
    let wieder: String = zeilen.concat();
    assert!(wieder.contains("abcdefghij"), "das lange Wort muss vollständig bleiben: {zeilen:?}");
    assert!(wieder.ends_with("cd"), "das Wort danach darf nicht verlorengehen: {zeilen:?}");
    for z in &zeilen {
        assert!(mono(z) <= 4.0, "Zeile zu breit: {z}");
    }
}

// Zeichenweise, nicht byteweise: Umlaute duerfen nicht zerrissen werden.
#[test]
fn der_harte_umbruch_zerreisst_keine_zeichen() {
    let zeilen = wrap_measured(mono, "äöüäöüäöü", 2.0);
    assert_eq!(zeilen.concat(), "äöüäöüäöü");
    for z in &zeilen {
        assert!(z.chars().count() <= 2, "Zeile zu breit: {z}");
    }
}

// Selbst wenn nicht einmal ein einziges Zeichen passt, darf der Text nicht still
// verschwinden -- lieber eine zu breite Zeile als gar keine.
#[test]
fn zu_schmales_panel_verschluckt_nichts() {
    let zeilen = wrap_measured(mono, "abc", 0.0);
    assert!(!zeilen.is_empty());
    assert_eq!(zeilen.concat(), "abc");
}

// Token-Feld: das laengste Ende, das passt -- auch mit Umlauten und wenn nichts passt.
#[test]
fn token_feld_zeigt_das_laengste_passende_ende() {
    assert_eq!(fitting_suffix(mono, "abc", 5.0), "abc");
    assert_eq!(fitting_suffix(mono, "abcdefghij", 4.0), "ghij");
    assert_eq!(fitting_suffix(mono, "äöüß", 3.0), "öüß");
    assert_eq!(fitting_suffix(mono, "abc", 0.0), "");
    let lang = "x".repeat(5000);
    assert_eq!(fitting_suffix(mono, &lang, 100.0).len(), 100);
}

fn span(text: &str, color: Option<u32>) -> protocol::nbt::TextSpan {
    protocol::nbt::TextSpan { text: text.to_string(), color, link: None, action: None, style: 0, shadow: None }
}

fn zeilen_text(zeilen: &[ChatLine]) -> Vec<String> {
    zeilen.iter().map(|z| z.spans.iter().map(|s| s.text.as_str()).collect()).collect()
}

// Farbiger Umbruch: die Abschnitte behalten ihre Farbe, auch wenn mitten in einem
// Abschnitt umgebrochen wird -- und keine Zeile ist samt Einzug breiter als erlaubt.
#[test]
fn farbiger_umbruch_behaelt_die_farben() {
    let zeile = vec![span("rot rot", Some(0xFF0000)), span(" blau blau", Some(0x0000FF))];
    let zeilen = wrap_chat_message(&zeile, 8.0, mono);
    assert!(zeilen.len() > 1, "bei 8 Zeichen Breite muss umgebrochen werden: {zeilen:?}");
    for z in &zeilen {
        let breite: f32 = z.spans.iter().map(|s| mono(&s.text)).sum();
        assert!(z.indent + breite <= 8.0, "Zeile zu breit: {z:?}");
        assert!(z.spans.iter().all(|s| s.color.is_some()), "die Farbe darf beim Umbrechen nicht verlorengehen: {z:?}");
    }
    let zusammen: String = zeilen_text(&zeilen).concat();
    assert!(zusammen.contains("rot") && zusammen.contains("blau"), "{zusammen:?}");
}

// Ein Wort, das allein zu breit ist, wird zerlegt statt aus dem Bild zu laufen -- mit
// seiner Farbe, und auch die eingerueckten Folgezeilen bleiben in der Breite.
#[test]
fn ein_zu_langes_chatwort_wird_zerlegt() {
    let zeile = vec![span("abcdefghij", Some(0x00FF00))];
    let zeilen = wrap_chat_message(&zeile, 4.0, mono);
    assert!(zeilen.len() >= 3, "{zeilen:?}");
    for z in &zeilen {
        let breite: f32 = z.spans.iter().map(|s| mono(&s.text)).sum();
        assert!(z.indent + breite <= 4.0, "Zeile zu breit: {z:?}");
        assert!(z.spans.iter().all(|s| s.color == Some(0x00FF00)));
    }
    assert_eq!(zeilen_text(&zeilen).concat(), "abcdefghij");
}

// Welt-Beschriftungen zerlegen ein zu breites Wort NICHT -- so macht es Vanilla dort.
#[test]
fn beschriftung_laesst_das_lange_wort_ganz() {
    let zeilen = wrap_label_lines(&[vec![span("abcdefghij", None)]], 4.0, mono);
    assert_eq!(zeilen.len(), 1);
    assert_eq!(zeilen[0][0].text, "abcdefghij");
}

// Frueher waren Folgezeilen einer Nachricht nicht als solche zu erkennen. Sie sind jetzt
// fest um zwei Leerzeichen eingerueckt -- auch hinter einem Absender NICHT bis zum
// Nachrichtentext (so gewollt) --, und das Leerzeichen an der Umbruchstelle faellt weg.
#[test]
fn folgezeilen_sind_fest_eingerueckt() {
    let msg = vec![span("<Steve> ", Some(0xFFFFFF)), span("eins zwei drei vier fuenf", None)];
    let zeilen = wrap_chat_message(&msg, 20.0, mono);
    assert!(zeilen.len() >= 2, "{zeilen:?}");
    assert_eq!(zeilen[0].indent, 0.0);
    for z in &zeilen[1..] {
        assert_eq!(z.indent, mono("  "), "fest zwei Leerzeichen");
        assert!(!z.spans[0].text.starts_with(' '), "kein Leerzeichen am Zeilenanfang: {z:?}");
    }
}

// Frueher liefen mehrzeilige Servernachrichten ineinander. "\n" beginnt eine neue Zeile
// OHNE Einzug (die Zeile hat der Server so gesetzt), fuehrende Leerzeichen bleiben stehen.
#[test]
fn zeilenumbrueche_im_text_werden_umgesetzt() {
    let zeilen = wrap_chat_message(&[span("Kopf\n  eingerückt\nFuß", Some(0xAAAAAA))], 100.0, mono);
    assert_eq!(zeilen_text(&zeilen), vec!["Kopf", "  eingerückt", "Fuß"]);
    assert!(zeilen.iter().all(|z| z.indent == 0.0));
}

// Eine leere Nachricht nimmt wie in Vanilla eine Zeile ein (Platz unter Logo-Zeichen).
#[test]
fn leere_nachricht_ist_eine_zeile() {
    assert_eq!(wrap_chat_message(&[], 100.0, mono).len(), 1);
    assert_eq!(wrap_chat_message(&[span("", None)], 100.0, mono).len(), 1);
}

// Nackte Adressen werden klickbar, Satzzeichen dahinter gehoeren nicht dazu, und ein
// vom Server gesetzter Link bleibt unangetastet.
#[test]
fn adressen_im_chat_werden_zu_links() {
    let out = linkify_chat(vec![span("siehe https://example.org/a?b=1. danke", Some(1))]);
    let link: Vec<_> = out.iter().filter(|s| s.link.is_some()).collect();
    assert_eq!(link.len(), 1);
    assert_eq!(link[0].text, "https://example.org/a?b=1");
    assert_eq!(link[0].link.as_deref(), Some("https://example.org/a?b=1"));
    assert_eq!(out.iter().map(|s| s.text.as_str()).collect::<String>(), "siehe https://example.org/a?b=1. danke");
    assert!(out.iter().all(|s| s.color == Some(1)));
    let server = protocol::nbt::TextSpan { text: "hier".into(), color: None, link: Some("https://x.y".into()), action: None, style: 0, shadow: None };
    assert_eq!(linkify_chat(vec![server.clone()]), vec![server]);
}

// Weiche Beleuchtung: Standard 100 % (wie Vanilla), der alte An/Aus-Schluessel wird
// uebernommen, und jede Staerke ueberlebt die settings.conf.
#[test]
fn weiche_beleuchtung_ist_an_und_wird_gespeichert() {
    assert_eq!(Settings::parse_onto(Settings::base(), "").smooth_lighting_percent, 100, "Vanillas Vorgabe ist an");
    assert_eq!(Settings::parse_onto(Settings::base(), "smooth_lighting=false\n").smooth_lighting_percent, 0);
    assert_eq!(Settings::parse_onto(Settings::base(), "smooth_lighting=true\n").smooth_lighting_percent, 100);
    assert_eq!(Settings::parse_onto(Settings::base(), "smooth_lighting_percent=250\n").smooth_lighting_percent, 250);
    let mut s = Settings::parse_onto(Settings::base(), "");
    s.smooth_lighting_percent = 40;
    assert_eq!(Settings::parse_onto(Settings::base(), &s.serialize()).smooth_lighting_percent, 40, "die Stärke muss eine Speicherrunde überleben");
}

fn standard_timing() -> i32 {
    Settings::parse_onto(Settings::base(), "show_hud=true\n").tear_timing_us
}

// Ruestungs-HUD, Keystrokes, Hurt-Cam und Feuer ueberleben eine Speicherrunde.
#[test]
fn hud_module_und_video_werte_werden_gespeichert() {
    let mut s = Settings::parse_onto(Settings::base(), "show_hud=true\n");
    assert!(!s.module_armor_hud && !s.module_keystrokes && s.hurt_cam && s.armor_trims);
    s.armor_trims = false;
    s.module_armor_hud = true;
    s.armor_hud_layout = armor_hud::LAYOUT_FLAT_HELMET_RIGHT;
    s.armor_hud_sort_flat = 1;
    s.hud_armor_scale = 150;
    s.armor_hud_gap = -5;
    s.armor_hud_number_scale = 150;
    s.armor_hud_number_x = 4;
    s.armor_hud_number_y = -20;
    s.module_keystrokes = true;
    s.keys_opacity = 40;
    s.hurt_cam = false;
    s.hurt_cam_strength = 35;
    s.fire_mode = FIRE_MODE_CROSSHAIR;
    s.fire_height = 45;
    s.fire_crosshair_hue = 200;
    let g = Settings::parse_onto(Settings::base(), &s.serialize());
    assert!(g.module_armor_hud && g.module_keystrokes && !g.hurt_cam && !g.armor_trims);
    assert_eq!((g.armor_hud_layout, g.armor_hud_sort_flat, g.hud_armor_scale), (armor_hud::LAYOUT_FLAT_HELMET_RIGHT, 1, 150));
    assert_eq!((g.armor_hud_gap, g.armor_hud_number_scale, g.armor_hud_number_x, g.armor_hud_number_y), (-5, 150, 4, -20));
    assert_eq!((g.keys_opacity, g.hurt_cam_strength, g.fire_mode, g.fire_height, g.fire_crosshair_hue), (40, 35, FIRE_MODE_CROSSHAIR, 45, 200));
}

// Der Name ueber der Hotbar erscheint nur bei umbenannten Items oder solchen mit Lore, nicht
// schon bei Verzauberung oder Trim.
#[test]
fn private_werte_landen_in_der_privaten_datei() {
    let mut set = Settings::base();
    set.account_name = "Spieler".into();
    set.tear_timing_us = -1400;
    set.fov = 100.0;
    let (oeffentlich, privat) = split_private_settings(&set.serialize());
    assert!(privat.contains("account_name=Spieler") && privat.contains("tear_timing_us=-1400"));
    assert!(!oeffentlich.contains("account_name") && !oeffentlich.contains("tear_") && oeffentlich.contains("fov=100"));
    // Beide Teile zusammen ergeben wieder dieselben Einstellungen.
    let zurueck = Settings::parse_onto(Settings::base(), &format!("{oeffentlich}\n{privat}"));
    assert_eq!((zurueck.account_name.as_str(), zurueck.tear_timing_us, zurueck.fov), ("Spieler", -1400, 100.0));
    // Standardwerte fuer alle enthalten nichts Privates.
    assert!(split_private_settings(DEFAULT_SETTINGS_CONF).1.is_empty());
}

#[test]
fn standardwerte_kommen_aus_default_settings_conf() {
    let set = Settings::default();
    assert_eq!(set.fov, 110.0);
    assert_eq!(set.theme_choice, ThemeChoice::Preset(theme::PRESETS.iter().position(|p| p.name == "Rosa").unwrap()));
    assert!(set.msa_refresh_token.is_empty() && set.mc_access_token.is_empty(), "kein Konto in den Standardwerten");
    // Jeder Schluessel der Datei muss etwas setzen: einer, den parse nicht kennt, ginge still verloren.
    let geschrieben = set.serialize();
    for zeile in DEFAULT_SETTINGS_CONF.lines().filter(|z| !z.starts_with('#')) {
        assert!(geschrieben.lines().any(|g| g == zeile), "{zeile} kommt nicht an");
    }
}

#[test]
// Pop-Optik > "Eigene Partikel aus" gibt es nicht mehr: ein alter Wert An wird zu
// Video > Eigene Partikel > Totem aus, aber nur, wenn die Pop-Optik an war.
fn alter_pop_schalter_wird_eigene_totem_partikel() {
    let totem = crate::particles::KINDS.iter().position(|k| k.key == "totem").unwrap();
    let alt = Settings::parse_onto(Settings::base(), "toteming_hide_own_particles=true\ntoteming_visuals=true\n");
    assert!(!alt.particle_tuning_own[totem].enabled && alt.particle_tuning[totem].enabled);
    let modul_aus = Settings::parse_onto(Settings::base(), "toteming_hide_own_particles=true\ntoteming_visuals=false\n");
    assert!(modul_aus.particle_tuning_own[totem].enabled);
    assert!(!Settings::default().particle_tuning_own[totem].enabled, "Standard wie vorher: eigene Pop-Wolke aus");
    assert!(!alt.serialize().contains("hide_own_particles"));
}

#[test]
// Einfrieren haengt hinten an: gespeicherte Werte 0..2 behalten ihre Bedeutung.
fn todesanimation_einfrieren_ist_der_vierte_wert() {
    assert_eq!(DEATH_ANIM_NAMES[DEATH_ANIM_FREEZE as usize], "Einfrieren");
    let set = Settings::parse_onto(Settings::base(), "death_animation=3\n");
    assert_eq!(set.death_animation, DEATH_ANIM_FREEZE);
}

#[test]
fn disconnect_fenster_zeigt_nur_die_servermeldung() {
    assert_eq!(disconnect_popup_text("Protokoll-Fehler: Server-Disconnect im Play-State: Du wurdest gekickt"), "Du wurdest gekickt");
    assert_eq!(disconnect_popup_text("Protokoll-Fehler: Server hat Login abgelehnt: You are already online!"), "You are already online!");
    assert_eq!(disconnect_popup_text("Timed out (30s keine Daten vom Server)"), "Timed out (30s keine Daten vom Server)");
}

#[test]
fn hotbar_name_nur_bei_eigenem_namen_oder_lore() {
    let mut stack = protocol::item::ItemStack::new(1, 1);
    stack.trim = Some((0, 0));
    assert!(!item_shows_held_name(&stack));
    stack.lore = vec![Vec::new()];
    assert!(item_shows_held_name(&stack));
    stack.lore.clear();
    stack.custom_name = Some("Klinge".to_string());
    assert!(item_shows_held_name(&stack));
}

// Riss-Position und Dynamisches Feuer ueberleben eine Speicherrunde; Unsinn faellt auf "Aus".
#[test]
fn riss_position_und_feuer_werden_gespeichert() {
    let standard = Settings::parse_onto(Settings::base(), "show_hud=true\n");
    assert_eq!(standard.tear_position, 0);
    assert!(standard.module_dynamic_fire, "Vorgabe an");
    let mut s = standard;
    s.tear_position = 1;
    s.module_dynamic_fire = false;
    let gelesen = Settings::parse_onto(Settings::base(), &s.serialize());
    assert_eq!(gelesen.tear_position, 1);
    assert!(!gelesen.module_dynamic_fire);
    // Alte Werte (1..3 = Oben/Unten/Austastluecke) werden "Scanout-Timing".
    assert_eq!(Settings::parse_onto(Settings::base(), "tear_position=3\n").tear_position, 1);
    assert_eq!(standard_timing(), TEAR_TIMING_DEFAULT_US);
    let mut t = Settings::parse_onto(Settings::base(), "show_hud=true\n");
    t.tear_timing_us = -1250;
    assert_eq!(Settings::parse_onto(Settings::base(), &t.serialize()).tear_timing_us, -1250);
    assert_eq!(Settings::parse_onto(Settings::base(), "tear_timing_us=99999\n").tear_timing_us, 99999);
    // Der alte Feinabgleich schob nach vorn: 1,3 ms werden -1,3 ms.
    assert_eq!(Settings::parse_onto(Settings::base(), "tear_latency_us=1300\n").tear_timing_us, -1300);
    assert_eq!(Settings::parse_onto(Settings::base(), "show_hud=true\n").tear_reserve_us, TEAR_RESERVE_DEFAULT_US);
    let mut r = Settings::parse_onto(Settings::base(), "show_hud=true\n");
    r.tear_reserve_us = 350;
    assert_eq!(Settings::parse_onto(Settings::base(), &r.serialize()).tear_reserve_us, 350);
    assert_eq!(Settings::parse_onto(Settings::base(), "tear_reserve_us=-5\n").tear_reserve_us, -5);
}

// Die Vorhersage-Toleranz ueberlebt Speichern/Laden, auch ausserhalb des Reglers.
// Bei 720p passen nicht mehr alle Zeilen der Haupt-Einstellungen untereinander -- die
// Seite scrollt. Bei Full HD passt alles bis zum Abschnitt System, die Render-Order darunter
// (seit 2026-11-12) erreicht man dort durch Scrollen; ab 1440p passt alles. Die Fussleiste
// darf nie unter den Rand rutschen.
#[test]
fn einstellungen_scrollen_erst_wenn_es_eng_wird() {
    assert!(settings_visible_rows(720.0) < SETTINGS_ROW_COUNT);
    assert!(settings_visible_rows(1080.0) >= RENDER_ORDER_HEAD_ROW, "{}", settings_visible_rows(1080.0));
    assert!(settings_visible_rows(1440.0) >= SETTINGS_ROW_COUNT, "{}", settings_visible_rows(1440.0));
    for h in [600.0, 720.0, 1080.0, 1440.0] {
        let zeilen = settings_visible_rows(h).min(SETTINGS_ROW_COUNT);
        assert!(SETTINGS_CONTENT_DY + SETTINGS_FIRST_Y + zeilen as f32 * SETTINGS_ROW_H <= h, "letzte Zeile unter dem Rand bei {h}");
    }
}

// Menues sehen bei jeder Aufloesung gleich aus (Nutzer 2026-11-12): sie rechnen in der
// Bezugshoehe, also passen ueberall gleich viele Zeilen, und die Breite folgt dem Fenster.
#[test]
fn menues_rechnen_bei_jeder_aufloesung_in_der_bezugshoehe() {
    let zeilen_fhd = settings_visible_rows(1080.0);
    for (w, h) in [(1280.0_f32, 720.0_f32), (1920.0, 1080.0), (2560.0, 1440.0), (3440.0, 1440.0), (3840.0, 2160.0)] {
        let s = menu_scale_for(h);
        assert!((h / s - MENU_REF_H).abs() < 1e-3, "{h}");
        assert_eq!(settings_visible_rows(h / s), zeilen_fhd, "{h}");
        // 16:9 ist ueberall 1920 breit, 21:9 breiter: Knoepfe am Rand bleiben am Rand.
        assert!((w / s - w / h * MENU_REF_H).abs() < 1e-2);
    }
    assert_eq!(menu_scale_for(1080.0), 1.0);
}

// Beim Umrechnen in echte Pixel wachsen Flaechen samt Rundung und Schatten mit; Marker
// behalten ihre Daten, nur der Listenrand-Verlauf traegt Pixel.
#[test]
fn flaechen_wachsen_mit_dem_massstab_marker_bleiben() {
    let r = UiRect { soft: 3.0, ..UiRect::framed(Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 }, [1.0; 4], 6.0, 1.0, [1.0; 4]) };
    let g = scale_ui_rect(&r, 2.0);
    assert_eq!((g.rect.x, g.rect.y, g.rect.w, g.rect.h), (20.0, 40.0, 60.0, 80.0));
    assert_eq!((g.radius, g.border, g.soft), (12.0, 2.0, 6.0));
    // Ein Rahmen verschwindet beim Verkleinern nicht.
    assert_eq!(scale_ui_rect(&r, 0.5).border, 1.0);
    let (mut rects, mut texts) = (Vec::new(), Vec::new());
    push_clip_marker(&mut rects, &mut texts, true);
    let fade = ScrollFade { top: 100.0, bottom: 300.0, zone: 25.0, edge_top: 0.4, edge_bottom: 1.0 };
    push_fade_marker(&mut rects, &mut texts, Some(fade));
    assert_eq!(clip_marker_rect(&scale_ui_rect(&rects[0], 1.5)), Some(true));
    let gross = fade_marker_rect(&scale_ui_rect(&rects[1], 1.5)).flatten().expect("Verlauf");
    assert_eq!(gross, ScrollFade { top: 150.0, bottom: 450.0, zone: 37.5, edge_top: 0.4, edge_bottom: 1.0 });
}

// Ein vergroessert gezeichnetes Server-Icon tastet einen halben Texel weiter innen ab, sonst
// zieht der Rand das Nachbarbild herein; in eigener Groesse oder kleiner bleibt es genau.
#[test]
fn vergroessertes_icon_ohne_nachbarbild() {
    let icon = UiSprite::quad(10.0, 10.0, 64.0, 64.0, server_icon_uv(9), [1.0; 4], SpriteAtlas::ServerIcons);
    let gross = scale_ui_sprite(&icon, 1.25);
    let halb = 0.5 / 512.0;
    assert_eq!(gross.uv, [icon.uv[0] + halb, icon.uv[1] + halb, icon.uv[2] - halb, icon.uv[3] - halb]);
    // Ganze Menue-Pixel landen auf ganzen echten -- wie die Flaeche unter dem Icon, sonst
    // schaut sie an einer Kante hervor.
    assert_eq!(gross.corners[2], [93.0, 93.0]);
    let feld = scale_ui_rect(&UiRect::rounded(Rect { x: 10.0, y: 10.0, w: 64.0, h: 64.0 }, [1.0; 4], 6.0), 1.25);
    assert_eq!([feld.rect.x, feld.rect.y, feld.rect.x + feld.rect.w, feld.rect.y + feld.rect.h], [gross.corners[0][0], gross.corners[0][1], 93.0, 93.0]);
    // Was gleitet, bleibt stufenlos.
    assert_eq!(scale_edge(10.5, 1.5), 15.75);
    assert_eq!(scale_ui_sprite(&icon, 0.75).uv, icon.uv);
}

// Durchsichtiges Weiss neben der Form eines Icons nimmt deren Farbe an (bleibt durchsichtig),
// damit vergroessert kein heller Saum entsteht; sichtbare Pixel und ferne bleiben.
#[test]
fn durchsichtiger_icon_rand_nimmt_die_farbe_der_form_an() {
    let mut bild = [255u8, 255, 255, 0].repeat(16);
    bild[(4 + 1) * 4..(4 + 1) * 4 + 4].copy_from_slice(&[180, 0, 170, 255]);
    bleed_icon_edges(&mut bild, 4);
    assert_eq!(&bild[0..4], &[180, 0, 170, 0], "Nachbar der Form");
    assert_eq!(&bild[(4 + 1) * 4..(4 + 1) * 4 + 4], &[180, 0, 170, 255], "die Form selbst");
    assert_eq!(&bild[(3 * 4 + 3) * 4..(3 * 4 + 3) * 4 + 4], &[255, 255, 255, 0], "weit weg");
}

// Ein Menue-Teil zwischen dem HUD (Modul-Menue im Spiel): nur sein Stueck der Listen wird
// umgerechnet, das HUD davor bleibt; der Text steht zwischen zwei Markern.
#[test]
fn menue_teil_im_hud_wird_allein_umgerechnet() {
    let flaeche = |x: f32| UiRect::rounded(Rect { x, y: 10.0, w: 20.0, h: 20.0 }, [1.0; 4], 0.0);
    let text = |t: &str| UiText { text: t.into(), x: 10.0, y: 10.0, color: [1.0; 4], scale: 2.0 };
    let (mut rects, mut texts) = (vec![flaeche(10.0)], vec![text("hud")]);
    let ab = (rects.len(), texts.len());
    rects.push(flaeche(30.0));
    texts.push(text("menue"));
    scale_ui_tail(&mut rects, &mut texts, ab, 1.5);
    assert_eq!((rects[0].rect.x, rects[1].rect.x, rects[1].rect.w), (10.0, 45.0, 30.0));
    assert_eq!(texts.len(), 4);
    assert_eq!(text_scale_marker_of(&texts[0]), None);
    assert_eq!(text_scale_marker_of(&texts[1]), Some(Some(1.5)));
    assert_eq!(texts[2].text, "menue");
    assert_eq!(text_scale_marker_of(&texts[3]), Some(None));
    // Massstab 1: nichts zu tun, keine Marker.
    let mut nur = vec![text("a")];
    scale_ui_tail(&mut rects, &mut nur, (0, 0), 1.0);
    assert_eq!(nur.len(), 1);
}

// Was man in ein Kaestchen des Schrift-Tabs zieht, steht danach genau so darin.
#[test]
fn schrift_kaestchen_speichern_was_hineinfaellt() {
    let mut set = Settings::parse_onto(Settings::base(), "");
    for tile in FontTile::palette() {
        for slot in FontSlot::all().filter(|s| s.accepts(tile)) {
            match (slot, tile) {
                (FontSlot::DefaultFont, FontTile::Font(k)) => set.font = area_font_code(Some(k)) - 1,
                (FontSlot::DefaultShadow, FontTile::Shadow(stil)) => set.set_global_text_shadow(stil),
                (FontSlot::AreaFont(a), FontTile::Font(k)) => set.area_font[a.index()] = area_font_code(Some(k)),
                (FontSlot::AreaShadow(a), FontTile::Shadow(stil)) => set.area_shadow[a.index()] = area_shadow_code(stil),
                _ => unreachable!(),
            }
            assert_eq!(set.font_slot(slot), Some(tile), "{slot:?}");
        }
    }
    assert_eq!(area_font_code(None), 0);
    // Je Bereich ein Schrift-Kaestchen, ein Rand-Kaestchen nur, wo es einen Rand gibt.
    assert_eq!(FontSlot::all().count(), 2 + TEXT_AREA_COUNT + ShadowArea::ALL.iter().filter(|a| a.has_shadow()).count());
}

#[test]
fn reglerwert_in_zahl_und_einheit() {
    assert_eq!(split_slider_value("50 %"), ("50", Some("%")));
    assert_eq!(split_slider_value("+0.25 Blöcke"), ("+0.25", Some("Blöcke")));
    assert_eq!(split_slider_value("110 °"), ("110", Some("°")));
    assert_eq!(split_slider_value("Monitor"), ("Monitor", None));
    assert_eq!(split_slider_value("Aus"), ("Aus", None));
    assert_eq!(split_slider_value("- x"), ("- x", None));
}

// Abschnitte: vor jedem Abschnitt unterhalb der obersten Zeile Platz fuer die Trennlinie;
// steht er oben, keinen. Gescrollt wird so weit, dass die letzte Zeile passt.
#[test]
fn abschnitte_im_zeilenraster() {
    let seite = SettingsPage { rows: 7, sections: &[("Allgemein", 0), ("Tränke", 4)], tabs: true };
    let starts = seite.section_starts();
    assert_eq!(starts, 1 << 4);
    let y = |row, erste: usize| settings_row_y_in(row, erste as f32, starts, f32::INFINITY);
    assert_eq!(y(0, 0), Some(SETTINGS_FIRST_Y));
    assert_eq!(y(3, 0), Some(SETTINGS_FIRST_Y + 3.0 * SETTINGS_ROW_H));
    assert_eq!(y(4, 0), Some(SETTINGS_FIRST_Y + 4.0 * SETTINGS_ROW_H + SETTINGS_SECTION_GAP));
    assert_eq!(y(4, 4), Some(SETTINGS_FIRST_Y));
    assert_eq!(y(5, 4), Some(SETTINGS_FIRST_Y + SETTINGS_ROW_H));
    assert_eq!(y(3, 4), None);
    let unten = SETTINGS_FIRST_Y + 5.0 * SETTINGS_ROW_H;
    assert_eq!(settings_row_y_in(6, 0.0, starts, unten), None);
    let max = settings_max_first(seite.rows, starts, unten);
    assert!(settings_row_y_in(6, max as f32, starts, unten).is_some());
    assert!(settings_row_y_in(6, max as f32 - 1.0, starts, unten).is_none());
    // Gleitend: halb zwischen zwei Zeilen steht alles um eine halbe Zeile hoeher.
    assert_eq!(y(5, 4).map(|v| v - SETTINGS_ROW_H / 2.0), settings_row_y_in(5, 4.5, starts, f32::INFINITY));
    assert_eq!(settings_row_y_in(4, 4.5, starts, f32::INFINITY), None);
    assert_eq!(settings_max_first(seite.rows, starts, f32::INFINITY), 0);
    // Jede Seite mit Abschnitten: der erste beginnt bei 0, alle aufsteigend und innerhalb.
    for (screen, _) in SETTINGS_CATEGORIES {
        let Some(p) = screen.settings_page() else { continue };
        assert!(p.rows < 128);
        assert!(p.sections.first().is_none_or(|&(_, r)| r == 0));
        assert!(p.sections.windows(2).all(|w| w[0].1 < w[1].1 && w[1].1 < p.rows));
    }
}

// Team-Kollisionsregeln (EntityPredicates.canBePushedBy): ohne Team schubst alles,
// NEVER auf einer Seite nie, Regel 2 nur zwischen verschiedenen, Regel 3 nur im selben Team.
#[test]
fn team_regeln_fuers_schubsen() {
    assert!(team_allows_push(None, 0, None, 0));
    assert!(!team_allows_push(Some("a"), 1, None, 0));
    assert!(!team_allows_push(None, 0, Some("b"), 1));
    assert!(team_allows_push(Some("a"), 2, Some("a"), 0));
    assert!(!team_allows_push(Some("a"), 2, Some("b"), 0));
    assert!(!team_allows_push(Some("a"), 3, Some("a"), 0));
    assert!(team_allows_push(Some("a"), 3, Some("b"), 0));
    assert_eq!(uuid_hyphenated(&[0x12; 16]), "12121212-1212-1212-1212-121212121212");
}

#[test]
fn entity_renderdistanz_laden() {
    assert_eq!(Settings::parse_onto(Settings::base(), "").entity_render_distance, ENTITY_DISTANCE_DEFAULT);
    assert_eq!(Settings::parse_onto(Settings::base(), "entity_render_distance=64\n").entity_render_distance, 64);
    assert_eq!(Settings::parse_onto(Settings::base(), "entity_render_distance=3\n").entity_render_distance, 3);
    assert_eq!(Settings::parse_onto(Settings::base(), "entity_render_distance=99999\n").entity_render_distance, 99999);
    assert_eq!(Settings::parse_onto(Settings::base(), "").block_entity_render_distance, BLOCK_ENTITY_DISTANCE_DEFAULT);
    let mut set = Settings::parse_onto(Settings::base(), "");
    set.block_entity_render_distance = 96;
    assert_eq!(Settings::parse_onto(Settings::base(), &set.serialize()).block_entity_render_distance, 96);
}

#[test]
fn vorhersage_toleranz_wird_gelesen() {
    let n = Settings::parse_onto(Settings::base(), "module_no_tool_break=false\nno_tool_break_reserve=0\n");
    assert!(!n.module_no_tool_break);
    assert_eq!(n.no_tool_break_reserve, 0);
    assert!(Settings::parse_onto(Settings::base(), "show_hud=true\n").module_no_tool_break, "Vorgabe an");
    let s = Settings::parse_onto(Settings::base(), "ghost_tolerance_ms=275\nmodule_ghost_tolerance=false\n");
    assert_eq!(s.ghost_tolerance_ms, 275);
    assert!(!s.module_ghost_tolerance);
    let zu_gross = Settings::parse_onto(Settings::base(), "ghost_tolerance_ms=99999\n");
    assert_eq!(zu_gross.ghost_tolerance_ms, 99999);
    // Ohne Eintrag bleibt es beim Standard -- eine alte settings.conf darf den Regler
    // nicht auf 0 stellen (das hiesse: immer halbdurchsichtig).
    assert_eq!(Settings::parse_onto(Settings::base(), "show_hud=true\n").ghost_tolerance_ms, GHOST_TOLERANCE_DEFAULT_MS);
}

// Die Schrittweite muss beide Grenzen exakt treffen, sonst bleibt der Regler kurz davor
// haengen und die Pfeile sehen aus, als waeren sie kaputt.
#[test]
fn die_toleranz_schritte_treffen_beide_grenzen() {
    assert_eq!(GHOST_TOLERANCE_MIN_MS % GHOST_TOLERANCE_STEP_MS, 0);
    assert_eq!(GHOST_TOLERANCE_MAX_MS % GHOST_TOLERANCE_STEP_MS, 0);
    assert_eq!(GHOST_TOLERANCE_DEFAULT_MS % GHOST_TOLERANCE_STEP_MS, 0);
}

// Eine frei eingetippte Empfindlichkeit muss bis auf VIER Nachkommastellen so dastehen,
// wie sie eingegeben wurde -- eine gerundete Anzeige liest sich wie eine verworfene
// Eingabe (mit nur einer Stelle wurde aus 0.85 eine "0.9", mit zweien aus 1.2345 eine
// "1.23"). Nachlaufende Nullen fallen weg, damit die Stepper-Werte kurz bleiben.
#[test]
fn empfindlichkeit_zeigt_vier_nachkommastellen_ohne_nullen() {
    assert_eq!(format_sensitivity(0.0012345), "1.2345", "die vierte Stelle muss stehen bleiben");
    assert_eq!(format_sensitivity(0.00085), "0.85");
    assert_eq!(format_sensitivity(0.0025), "2.5", "Stepper-Werte bleiben kurz");
    assert_eq!(format_sensitivity(SENSITIVITY_MAX), "10");
}

// Der laengste ueberhaupt erreichbare Anzeigewert muss noch in den Eingabepuffer passen
// (7 Zeichen, s. Tasteneingabe im Editiermodus). "10.0000" ist mit genau 7 Zeichen der
// Grenzfall -- waere der Puffer kuerzer, koennte man den eigenen angezeigten Wert nicht
// wieder eintippen.
#[test]
fn vier_nachkommastellen_passen_in_den_eingabepuffer() {
    assert_eq!(format!("{:.4}", SENSITIVITY_MAX * 1000.0).chars().count(), 7);
    assert_eq!(format!("{CURSOR_SENSITIVITY_MAX:.4}").chars().count(), 6);
}

// Zeiger- und Kamera-Empfindlichkeit sind GETRENNTE Einstellungen: dieselbe Zahl bedeutet
// bei beiden etwas anderes (Radiant/Pixel gegen Pixel/Pixel), deshalb hat der Zeiger keinen
// 1000er-Faktor in der Anzeige.
#[test]
fn zeiger_empfindlichkeit_wird_unskaliert_angezeigt() {
    assert_eq!(format_cursor_sensitivity(DEFAULT_CURSOR_SENSITIVITY), "1");
    assert_eq!(format_cursor_sensitivity(0.5), "0.5");
    assert_eq!(format_cursor_sensitivity(1.2345), "1.2345");
}

// Die beiden Empfindlichkeiten duerfen sich nicht gegenseitig verstellen -- sie liegen auf
// demselben Screen direkt untereinander, ein vertauschter Zeilenindex faellt sonst erst im
// Spiel auf.
#[test]
fn empfindlichkeiten_sind_unabhaengig() {
    // Getrennte Schrittweiten und getrennte Grenzen -- ein versehentlich gemeinsam
    // genutzter Bereich faellt hier sofort auf.
    let zeiger = step_cursor_sensitivity(DEFAULT_CURSOR_SENSITIVITY, true);
    assert!(zeiger > DEFAULT_CURSOR_SENSITIVITY, "der Zeiger muss sich bewegen");
    assert!(
        zeiger <= CURSOR_SENSITIVITY_MAX && zeiger != step_sensitivity(DEFAULT_MOUSE_SENSITIVITY, true),
        "die beiden Regler dürfen nicht dieselbe Skala benutzen"
    );

    let mut runter = DEFAULT_CURSOR_SENSITIVITY;
    for _ in 0..100 {
        runter = step_cursor_sensitivity(runter, false);
    }
    assert_eq!(runter, CURSOR_SENSITIVITY_MIN, "muss am Minimum stehenbleiben");

    let mut hoch = DEFAULT_MOUSE_SENSITIVITY;
    for _ in 0..100 {
        hoch = step_sensitivity(hoch, true);
    }
    assert_eq!(hoch, SENSITIVITY_MAX, "die Kamera muss an ihrem eigenen Maximum stehenbleiben");
}

// Beide Zeilen des Steuerung-Screens sind echte, anklickbare Wertfelder auf getrennten
// Zeilen -- der Ueberlappungstest oben deckt nur ab, dass sie sich nicht kreuzen.
#[test]
fn steuerung_hat_zwei_echte_empfindlichkeitszeilen() {
    assert_ne!(controls_row(ControlsRow::SensX), controls_row(ControlsRow::CursorSens));
    assert!(EditableValue::MouseSensitivity.screen() == MenuScreen::SettingsControls);
    assert!(EditableValue::CursorSensitivity.screen() == MenuScreen::SettingsControls);
    assert!(EditableValue::CursorSensitivity.allows_decimal(), "sonst liesse sich kein 0.5 eintippen");
    assert!(controls_row(ControlsRow::SneakProtection) > controls_row(ControlsRow::CursorSens));
}

// Vorzeichen nur bei Abweichung von null -- sonst stuende ueberall ein unnoetiges "+0"
// bzw. ein redundantes "-0".
#[test]
fn tonhoehe_zeigt_vorzeichen_nur_bei_abweichung() {
    assert_eq!(format_ui_pitch(0), "0");
    assert_eq!(format_ui_pitch(3), "+3");
    assert_eq!(format_ui_pitch(-5), "-5");
}

#[test]
// Der Zufallswert muss im vollen Bereich -1..1 liegen und sich mit dem Seed aendern --
// sonst waere "zufaellig" in Wahrheit immer derselbe Wert.
fn zufallswert_liegt_in_bereich_und_streut() {
    let werte: Vec<f32> = (0..50).map(zufallswert).collect();
    for v in &werte {
        assert!(*v >= -1.0 && *v <= 1.0, "{v}");
    }
    assert!(werte.windows(2).any(|w| (w[0] - w[1]).abs() > 0.01), "Werte dürfen nicht alle gleich sein");
}

// Zeilenband: die Zeichenbereiche werden an den Markern geteilt, auch wenn ein Bereich
// (z. B. ein Schriftwechsel) mitten in einem beschnittenen Stueck anfaengt.
#[test]
fn beschneiden_teilt_an_den_markern() {
    let clips = [(6, true), (30, false), (36, true), (60, false)];
    assert_eq!(clip_segments(0..72, &clips), vec![(0..6, false), (6..30, true), (30..36, false), (36..60, true), (60..72, false)]);
    assert_eq!(clip_segments(12..40, &clips), vec![(12..30, true), (30..36, false), (36..40, true)]);
    assert_eq!(clip_segments(0..10, &[]), vec![(0..10, false)]);
    // Marker am Ende ohne Vertices danach: nichts Leeres.
    assert_eq!(clip_segments(0..6, &[(6, true)]), vec![(0..6, false)]);
}

// GUI-Skalierung: 0 muss Vanillas Auto-Rechnung ergeben (so gross wie das 320x240-Raster
// erlaubt), 1..7 die feste Stufe -- unabhaengig von der Aufloesung.
#[test]
fn skalierung_null_ist_auto_der_rest_fest() {
    let hd = (1920.0, 1080.0);
    let klein = (800.0, 600.0);
    assert_eq!(App::gui_scale_of(0, hd), inventory::gui_scale(hd));
    assert_eq!(App::gui_scale_of(0, klein), inventory::gui_scale(klein));
    assert_eq!(App::gui_scale_of(0, hd), 4.0, "1920x1080 erlaubt Vanillas höchste Stufe");
    assert_eq!(App::gui_scale_of(0, klein), 2.0, "800x600 traegt nur x2");
    for stufe in 1..=7u32 {
        assert_eq!(App::gui_scale_of(stufe, hd), stufe as f32);
        // Feste Stufen ignorieren die Aufloesung -- genau der Sinn der Einstellung.
        assert_eq!(App::gui_scale_of(stufe, klein), stufe as f32);
    }
    // Ein von Hand zu gross eingetragener Wert darf nicht durchschlagen.
    assert_eq!(App::gui_scale_of(99, hd), 7.0);
    // Hohe Aufloesungen: Auto wie Vanilla ohne Deckel, feste Stufen bis dorthin.
    let uhd = (3840.0, 2160.0);
    assert_eq!(App::gui_scale_of(0, uhd), 9.0);
    assert_eq!(App::gui_scale_of(7, uhd), 7.0);
    assert_eq!(App::gui_scale_of(99, uhd), 9.0);
    assert_eq!(inventory::max_gui_scale(klein), 7, "ueberall bis x7");
}

// Die Skalierungen sind drei GETRENNTE Einstellungen -- eine zu aendern darf die anderen
// nicht mitziehen.
#[test]
fn skalierungen_sind_unabhaengig() {
    let text = "inventory_scale=3\nhud_hotbar_scale=1\nhud_chat_scale=4\n";
    let settings = Settings::parse_onto(Settings::base(), text);
    assert_eq!(settings.inventory_scale, 3);
    assert_eq!(settings.hud_hotbar_scale, 1);
    // Die alte Chat-Stufe x4 wird zu 200 % (x4 ist das Doppelte von UI_SCALE_SMALL).
    assert_eq!(settings.chat_scale_percent, 200);
    assert_eq!(Settings::parse_onto(Settings::base(), "hud_chat_scale=0\n").chat_scale_percent, 100, "Auto = 100 %");
    // Hohe Stufen fuer grosse Bildschirme bleiben erhalten, Unsinn wird beim Einlesen geklemmt.
    assert_eq!(Settings::parse_onto(Settings::base(), "inventory_scale=9\n").inventory_scale, 9);
    assert_eq!(Settings::parse_onto(Settings::base(), "inventory_scale=999\n").inventory_scale, inventory::GUI_SCALE_SETTING_MAX);
}

// Die Kontur gehoert in den Fadenkreuz-Pass, nicht in den UI-Pass. Sie MUSS also als eigene
// Vertices mit gesetztem Flag im selben Puffer stehen und VOR dem Kreuz liegen -- sonst
// deckt sie das Kreuz zu, oder sie landet (wie frueher) in der Textur, aus der das Kreuz
// seine Gegenfarbe liest.
#[test]
fn kontur_steckt_im_fadenkreuz_puffer_und_liegt_davor() {
    let screen = (1920.0, 1080.0);
    let ohne = crosshair_vertices(0, 10, (960.0, 540.0), screen, false);
    let mit = crosshair_vertices(0, 10, (960.0, 540.0), screen, true);
    assert_eq!(ohne.len(), 12, "nur die zwei Balken des Kreuzes");
    assert_eq!(mit.len(), 24, "Kontur kommt als zwei weitere Balken dazu");
    assert!(ohne.iter().all(|v| v.kind == CROSSHAIR_KIND_FILL), "ohne Kontur darf kein Vertex markiert sein");
    assert!(mit[..12].iter().all(|v| v.kind == CROSSHAIR_KIND_OUTLINE), "die ersten zwölf sind die Kontur");
    assert!(mit[12..].iter().all(|v| v.kind == CROSSHAIR_KIND_FILL), "das Kreuz kommt danach und deckt die Mitte ab");
    // Das Kreuz selbst bleibt unveraendert, egal ob die Kontur an ist.
    assert_eq!(mit[12..].iter().map(|v| v.position).collect::<Vec<_>>(), ohne.iter().map(|v| v.position).collect::<Vec<_>>());
    // Beim Punkt genauso: erst der schwarze, groessere Kreis, dann der eigentliche.
    let punkt = crosshair_vertices(CROSSHAIR_SHAPE_DOT, 4, (960.0, 540.0), screen, true);
    assert_eq!(punkt.len(), 12);
    assert!(punkt[..6].iter().all(|v| v.kind == CROSSHAIR_KIND_DISC_OUTLINE && v.circle == [0.0, 3.0]));
    assert!(punkt[6..].iter().all(|v| v.kind == CROSSHAIR_KIND_DISC && v.circle == [0.0, 2.0]));
}

fn balken(teil: CrosshairPart) -> Rect {
    match teil {
        CrosshairPart::Bar(r) => r,
        CrosshairPart::Disc { .. } => panic!("Balken erwartet"),
    }
}

// Die Kontur muss GENAU einen Pixel breiter sein als das Kreuz -- auf jeder Seite, an
// beiden Balken. Sonst ist sie an einer Kante zwei Pixel dick oder fehlt ganz.
#[test]
fn kontur_ist_genau_einen_pixel_breiter_als_das_kreuz() {
    let teile = crosshair_parts(0, 10, (100.0, 50.0), true);
    let (senkrecht, waagerecht) = (balken(teile[0].0), balken(teile[1].0));
    assert!(teile[0].1 && teile[1].1 && !teile[2].1 && !teile[3].1, "erst Kontur, dann Kreuz");
    assert_eq!(senkrecht.w, CROSSHAIR_THICKNESS_PX + 2.0);
    assert_eq!(senkrecht.h, 10.0 + 2.0);
    assert_eq!(waagerecht.w, 10.0 + 2.0);
    assert_eq!(waagerecht.h, CROSSHAIR_THICKNESS_PX + 2.0);
    // Beide Balken teilen sich exakt die Mitte des Kreuzes.
    assert_eq!(senkrecht.x + senkrecht.w / 2.0, 100.0);
    assert_eq!(senkrecht.y + senkrecht.h / 2.0, 50.0);
    assert_eq!(waagerecht.x + waagerecht.w / 2.0, 100.0);
    assert_eq!(waagerecht.y + waagerecht.h / 2.0, 50.0);
}

// Krumme Mitten muessen auf ganze Pixel einrasten, sonst verteilt der Rasterizer die
// Kante ueber zwei Spalten und es sieht nach Kantenglaettung aus (die es hier nicht gibt).
#[test]
fn kontur_rastet_auf_ganze_pixel_ein() {
    let senkrecht = balken(crosshair_parts(0, 10, (100.4, 50.6), true)[0].0);
    assert_eq!(senkrecht.x, 100.0 - (CROSSHAIR_THICKNESS_PX / 2.0 + 1.0));
    assert_eq!(senkrecht.y, 51.0 - (5.0 + 1.0));
    assert_eq!(senkrecht.x.fract(), 0.0);
    assert_eq!(senkrecht.y.fract(), 0.0);
}

// Der Fadenkreuz-Shader muss sich uebersetzen lassen (Kreise kamen als neue Art dazu).
#[test]
fn fadenkreuz_shader_ist_gueltig() {
    let quelle = format!("{}\n{}", include_str!("../crosshair.wgsl"), render::CONTRAST_COLOR_WGSL);
    let modul = naga::front::wgsl::parse_str(&quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&quelle)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&modul)
        .expect("crosshair.wgsl ungueltig");
}

#[test]
fn schrift_shader_ist_gueltig() {
    let quelle = include_str!("../font.wgsl");
    let modul = naga::front::wgsl::parse_str(quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(quelle)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&modul)
        .expect("font.wgsl ungueltig");
}

// Mit vs_label (verdeckbare Weltbeschriftungen) hat auch quad.wgsl zwei Einstiege.
#[test]
fn flaechen_shader_ist_gueltig() {
    let quelle = include_str!("../quad.wgsl");
    let modul = naga::front::wgsl::parse_str(quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(quelle)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&modul)
        .expect("quad.wgsl ungueltig");
}

// "Aussen" ist der weiche Schatten ohne Versatz: das Viereck ragt rundherum gleich weit.
#[test]
fn schatten_aussen_ragt_rundherum_gleich_weit() {
    let q = text_border_quad(TextShadow::Around, 1.0, [10.0, 10.0, 20.0, 30.0], [0.0; 2], [1.0; 2], 2.0, true, false, [0.0, 0.0, 0.0, 1.0], (100.0, 100.0));
    assert_eq!(q[0].mode[0], TEXT_MODE_AROUND);
    let xs: Vec<f32> = q.iter().map(|v| v.position[0]).collect();
    let ys: Vec<f32> = q.iter().map(|v| v.position[1]).collect();
    let (x0, x1) = (xs.iter().copied().fold(f32::MAX, f32::min), xs.iter().copied().fold(f32::MIN, f32::max));
    let (y0, y1) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MIN, f32::max));
    // NDC: 100 px = 2.0, also 1 px = 0.02; der Text ist 10 x 20 px.
    assert!(((x1 - x0) / 0.02 - 10.0 - ((y1 - y0) / 0.02 - 20.0)).abs() < 1e-3);
    assert_eq!(area_shadow_style(5), Some(TextShadow::Around));
}

// Punkt und Kreis sind echte Kreise um die Mitte: der Punkt eine volle Scheibe, der Kreis
// ein Ring in Linienstaerke. Ungerader Durchmesser sitzt auf einer Pixelmitte.
#[test]
fn punkt_und_kreis_sind_kreise() {
    let scheibe = |teil: CrosshairPart| match teil {
        CrosshairPart::Disc { center, inner, outer } => (center, inner, outer),
        CrosshairPart::Bar(_) => panic!("Kreis erwartet"),
    };
    assert_eq!(scheibe(crosshair_parts(CROSSHAIR_SHAPE_DOT, 4, (100.0, 50.0), false)[0].0), ((100.0, 50.0), 0.0, 2.0));
    assert_eq!(scheibe(crosshair_parts(CROSSHAIR_SHAPE_DOT, 5, (100.0, 50.0), false)[0].0), ((100.5, 50.5), 0.0, 2.5));
    assert_eq!(scheibe(crosshair_parts(CROSSHAIR_SHAPE_CIRCLE, 12, (100.0, 50.0), false)[0].0), ((100.0, 50.0), 4.0, 6.0));
    // Die Kontur des Rings liegt innen und aussen je einen Pixel daneben.
    assert_eq!(scheibe(crosshair_parts(CROSSHAIR_SHAPE_CIRCLE, 12, (100.0, 50.0), true)[0].0), ((100.0, 50.0), 3.0, 7.0));
    // Das offene Kreuz hat vier Arme und eine freie Mitte.
    let arme = crosshair_parts(CROSSHAIR_SHAPE_OPEN, 14, (100.0, 50.0), false);
    assert_eq!(arme.len(), 4);
    assert!(arme.iter().all(|(t, _)| !balken(*t).contains(100.0, 50.0)));
}

// Form und Groesse je Form ueberleben Speichern und Laden; Unsinn wird geklemmt.
#[test]
fn fadenkreuz_form_und_groessen_werden_gespeichert() {
    let mut set = Settings::base();
    set.set_crosshair_shape_choice(CROSSHAIR_SHAPE_DOT);
    set.step_crosshair_size(3);
    let geladen = Settings::parse_onto(Settings::base(), &set.serialize());
    assert_eq!(geladen.crosshair_shape, CROSSHAIR_SHAPE_DOT);
    assert_eq!(geladen.crosshair_size(), CROSSHAIR_SHAPES[CROSSHAIR_SHAPE_DOT].3 + 3);
    assert_eq!(geladen.crosshair_sizes[0], CROSSHAIR_SHAPES[0].3, "das Kreuz behält seine Größe");
    let unsinn = Settings::parse_onto(Settings::base(), "crosshair_shape=99\ncrosshair_sizes=1,999\n");
    assert_eq!(unsinn.crosshair_shape, CROSSHAIR_SHAPES.len() - 1);
    assert_eq!(unsinn.crosshair_sizes[0], 1, "Größen bleiben, wie gespeichert");
    assert_eq!(unsinn.crosshair_sizes[1], 999);
    // Die Auswahl endet bei "Eigene" und schaltet dort den Pixel-Editor-Stand ein.
    let mut set = Settings::base();
    set.set_crosshair_shape_choice(10);
    assert!(set.crosshair_custom);
    assert_eq!(set.crosshair_shape_choice(), CROSSHAIR_SHAPE_CUSTOM);
    set.set_crosshair_shape_choice(CROSSHAIR_SHAPE_OPEN);
    assert!(!set.crosshair_custom);
}

// Die Aufhellung bleibt, wie gespeichert, und ist standardmaessig aus (reine Gegenfarbe wie bisher).
#[test]
fn aufhellung_bleibt_und_ist_standardmaessig_aus() {
    assert_eq!(Settings::base().crosshair_brightness_boost, 0);
    assert_eq!(Settings::parse_onto(Settings::base(), "crosshair_brightness_boost=40\n").crosshair_brightness_boost, 40);
    assert_eq!(Settings::parse_onto(Settings::base(), "crosshair_brightness_boost=250\n").crosshair_brightness_boost, 250);
}

// Der Aufhellungs-Regler gehoert NUR zum Komplementaer-Modus -- bei einer festen Farbe
// gaebe es nichts aufzuhellen. Getestet ueber die Zeilenzahl, weil davon auch abhaengt,
// wo "Zuruecksetzen" sitzt (frueher lag der Button auf der zweiten Zeile).
#[test]
fn aufhellungs_zeile_nur_im_komplementaer_modus() {
    // Farbe + Kontur + Kontur des Rings + Aufhellung bzw. ohne Aufhellung, je dazu "Form"
    // und "Groesse"/"Pixel bearbeiten" direkt darunter.
    let mit_modus = |crosshair_color_mode| Settings { crosshair_color_mode, ..Default::default() };
    assert_eq!(hud_config_rows(HudElement::Crosshair, &mit_modus(CROSSHAIR_MODE_COMPLEMENTARY)), 7);
    assert_eq!(crosshair_shape_row(CROSSHAIR_MODE_COMPLEMENTARY), 4);
    for feste_farbe in 1..CROSSHAIR_COLOR_MODES.len() {
        assert_eq!(hud_config_rows(HudElement::Crosshair, &mit_modus(feste_farbe)), 6, "Modus {feste_farbe} hat keine Aufhellung");
        assert_eq!(crosshair_shape_row(feste_farbe), 3);
    }
    // Hotbar: Skalierung, Haltbarkeit ab, Grenze prozentual/pauschal, Sauerstoff, Zahl-Farbe,
    // Auswahl gleitet.
    assert_eq!(hud_config_rows(HudElement::Hotbar, &mit_modus(CROSSHAIR_MODE_COMPLEMENTARY)), 6);
    // Und der Button rutscht entsprechend mit, statt auf der Zeile zu liegen.
    assert!(hud_reset_button(2).y > hud_reset_button(1).y);
}

#[test]
fn parse_ohne_inhalt_gibt_standardwerte() {
    let settings = Settings::parse_onto(Settings::base(), "");
    assert_eq!(settings.mesh_columns_per_frame, DEFAULT_MESH_COLUMNS_PER_FRAME);
    assert_eq!(settings.mouse_sensitivity, DEFAULT_MOUSE_SENSITIVITY);
    assert!(settings.show_hud);
    assert_eq!(settings.crosshair_color_mode, 0, "Standard-Farbmodus ist Komplementär");
}

// Ein per Hand editierter, zu grosser Modus-Index darf nicht aus CROSSHAIR_COLOR_MODES
// herausindizieren (s. Settings::parse-Kommentar zum generellen Klemm-Prinzip) --
// sonst paniert render_world beim naechsten Frame.
#[test]
fn crosshair_farbmodus_wird_geklemmt() {
    let settings = Settings::parse_onto(Settings::base(), "crosshair_color_mode=999\n");
    assert_eq!(settings.crosshair_color_mode, CROSSHAIR_COLOR_MODES.len() - 1);
}

// Der Hotspot muss auch DURCH die Effekte hindurch die Spitze bleiben: gestaucht und
// gedreht wird um die Spitze, nicht um die Mitte. Sonst wanderte beim Klicken genau der
// Punkt weg, auf den man gezeigt hat -- und man traefe den Knopf daneben.
#[test]
fn die_effekte_verschieben_die_spitze_nicht() {
    let screen = (1920.0, 1080.0);
    let tip_pos = (400.0, 300.0);
    for fx in [
        CursorFxFrame { scale: 0.90, rotation_deg: 0.0, additive: 1.0, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 },
        CursorFxFrame { scale: 1.0, rotation_deg: 137.0, additive: 0.0, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 },
        CursorFxFrame { scale: 0.9, rotation_deg: -204.0, additive: 0.4, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 },
    ] {
        let u = cursor_polygons(tip_pos, screen, fx);
        // Der Klickpunkt ist die Aussenkante der runden Spitze: Punkt 0 liegt genau die
        // Rundung davon weg, egal wie gedreht oder gestaucht.
        let d = ((u.points[0][0] - tip_pos.0).powi(2) + (u.points[0][1] - tip_pos.1).powi(2)).sqrt();
        assert!((d - u.form[0]).abs() < 1e-3, "{fx:?}: Spitze {d} weg, Rundung {}", u.form[0]);
    }
}

#[test]
// Gestaucht heisst KLEINER, nicht nur anders. Ein vertauschtes Vorzeichen oder ein
// vergessener Faktor faellt hier auf, und sonst erst im Spiel.
fn beim_druecken_wird_der_zeiger_kleiner() {
    let screen = (1920.0, 1080.0);
    let tip = (400.0, 300.0);
    let spanne = |fx: CursorFxFrame| {
        let u = cursor_polygons(tip, screen, fx);
        u.bounds[3] - u.bounds[1]
    };
    let ruhe = spanne(CursorFxFrame { scale: 1.0, rotation_deg: 0.0, additive: 0.0, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 });
    let gedrueckt = spanne(CursorFxFrame { scale: 0.90, rotation_deg: 0.0, additive: 0.0, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 });
    assert!(gedrueckt < ruhe, "gedrückt {gedrueckt} muss kleiner sein als in Ruhe {ruhe}");
}

#[test]
// Das Aufleuchten steckt in der Farbe, nicht in der Form -- es darf keinen Punkt bewegen.
fn das_aufleuchten_aendert_nur_die_farbe() {
    let screen = (1920.0, 1080.0);
    let tip = (400.0, 300.0);
    let ohne = cursor_polygons(tip, screen, CursorFxFrame { scale: 1.0, rotation_deg: 0.0, additive: 0.0, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 });
    let mit = cursor_polygons(tip, screen, CursorFxFrame { scale: 1.0, rotation_deg: 0.0, additive: 1.0, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 });
    assert_eq!(ohne.points, mit.points);
    assert_eq!(ohne.colors, mit.colors, "die Grundfarben bleiben, das Leuchten kommt DAZU");
    assert_eq!(ohne.additive[3], 0.0);
    assert_eq!(mit.additive[3], CURSOR_PRESS_TINT);
    let [r, g, b, _] = theme::get(theme::Slot::Accent);
    assert_eq!([mit.additive[0], mit.additive[1], mit.additive[2]], [r, g, b], "Leuchten in der Akzentfarbe des Themes");
    let rechts = cursor_polygons(tip, screen, CursorFxFrame { scale: 1.0, rotation_deg: 0.0, additive: 1.0, right: 1.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 });
    let [r, g, b, _] = theme::get(theme::Slot::Accent2);
    assert_eq!([rechts.additive[0], rechts.additive[1], rechts.additive[2]], [r, g, b], "Rechtsklick leuchtet in Akzent 2");
}

// Die Breite streckt nur die Fluegel: Spitze und Kerbe bleiben, der Pfeil wird schmaler.
#[test]
fn zeigerbreite_streckt_nur_die_fluegel() {
    let voll = cursor_shape(1.0);
    let schmal = cursor_shape(0.5);
    let abstand = |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    assert!(voll.iter().zip(CURSOR_SHAPE).all(|(&a, b)| abstand(a, b) < 1e-3), "100 % ist die Vorlage: {voll:?}");
    assert_eq!((schmal[0], schmal[2]), (voll[0], voll[2]), "Spitze und Kerbe bleiben stehen");
    assert!(abstand(schmal[1], schmal[3]) < abstand(voll[1], voll[3]) * 0.6, "halbe Breite macht den Pfeil schmaler");
}

// Die Kontur waechst mit dem Regler und mit der Zeigergroesse.
#[test]
fn die_kontur_waechst_mit_regler_und_groesse() {
    let screen = (1920.0, 1080.0);
    let tip = (400.0, 300.0);
    let dicke = |scale: f32, outline: f32| {
        cursor_polygons(tip, screen, CursorFxFrame { scale, rotation_deg: 0.0, additive: 0.0, right: 0.0, circle: true, width: 1.0, outline, fill_alpha: 0.5 }).screen[2]
    };
    assert!((dicke(1.0, 2.0) - 2.0 * dicke(1.0, 1.0)).abs() < 1e-4);
    assert!((dicke(2.0, 1.0) - 2.0 * dicke(1.0, 1.0)).abs() < 1e-4);
    assert_eq!(dicke(1.0, 0.0), 0.0);
}

#[test]
fn cursor_tip_landet_exakt_auf_tip_pos() {
    let screen = (1920.0, 1080.0);
    let tip_pos = (400.0, 300.0);
    // Ruhezustand: volle Groesse, ungedreht, kein Leuchten.
    let u = cursor_polygons(tip_pos, screen, CursorFxFrame { scale: 1.0, rotation_deg: 0.0, additive: 0.0, right: 0.0, circle: true, width: 1.0, outline: 1.0, fill_alpha: 0.5 });

    // Die runde Spitze: der erste Punkt liegt genau die Rundung von tip_pos weg, die sichtbare
    // Aussenkante (Punkt minus Rundung) also genau auf tip_pos.
    let spitze = (u.points[0][0], u.points[0][1]);
    let d = ((spitze.0 - tip_pos.0).powi(2) + (spitze.1 - tip_pos.1).powi(2)).sqrt();
    assert!((d - u.form[0]).abs() < 1e-3, "Spitze {spitze:?} liegt {d} von {tip_pos:?}, Rundung {}", u.form[0]);

    // Kein NaN, keine Explosion durch einen Vorzeichenfehler in Rotation/Skalierung:
    // alle Punkte liegen in der Naehe der Spitze (die Form ist nur 40 px hoch).
    for pair in &u.points {
        for (x, y) in [(pair[0], pair[1]), (pair[2], pair[3])] {
            assert!(x.is_finite() && y.is_finite());
            assert!((x - tip_pos.0).abs() < 2.0 * CURSOR_HEIGHT_PX, "x weit weg: {x}");
            assert!((y - tip_pos.1).abs() < 2.0 * CURSOR_HEIGHT_PX, "y weit weg: {y}");
        }
    }

    // Die Zeichenflaeche muss die ganze Form samt Glaettungsrand enthalten -- ist sie zu
    // klein, wird der Zeiger stillschweigend beschnitten.
    for pair in &u.points {
        for (x, y) in [(pair[0], pair[1]), (pair[2], pair[3])] {
            assert!(x > u.bounds[0] && x < u.bounds[2], "x {x} außerhalb der Zeichenfläche");
            assert!(y > u.bounds[1] && y < u.bounds[3], "y {y} außerhalb der Zeichenfläche");
        }
    }
    assert_eq!(u.screen[..2], [screen.0, screen.1]);
    assert!(u.screen[2] > 0.0, "Kontur fehlt");
    assert!(u.screen[3] > 0.0, "Ringradius fehlt");
    assert!(tip_pos.0 > u.bounds[0] && tip_pos.1 > u.bounds[1], "die Spitze liegt in der Zeichenfläche");
}

// Die Atlas-Platzvergabe (s. claim_atlas_slot). Ihr eigentlicher Zweck ist, dass zwei
// Spieler NIE denselben Platz belegen -- passiert das, tragen sie denselben Skin.
#[test]
fn platzvergabe_gibt_jedem_seinen_eigenen_platz() {
    let mut order = std::collections::VecDeque::new();
    let uuid = |n: u8| [n; 16];
    let mut vergeben = Vec::new();
    for n in 0..3u8 {
        let (slot, evicted) = claim_atlas_slot(&mut order, 3, uuid(n), None, |_| false).expect("Platz");
        assert_eq!(evicted, None, "solange frei ist, wird nichts verdrängt");
        vergeben.push(slot);
    }
    vergeben.sort_unstable();
    assert_eq!(vergeben, vec![0, 1, 2], "drei Spieler, drei verschiedene Plätze");
}

#[test]
// Ein schon vergebener Platz wird wiederverwendet (Skin-Wechsel im laufenden Spiel), und
// die Reihenfolge-Schlange darf davon nicht wachsen.
fn bekannter_spieler_behaelt_seinen_platz() {
    let mut order = std::collections::VecDeque::new();
    let (slot, _) = claim_atlas_slot(&mut order, 4, [1; 16], None, |_| false).expect("Platz");
    let (wieder, evicted) = claim_atlas_slot(&mut order, 4, [1; 16], Some(slot), |_| false).expect("Platz");
    assert_eq!(wieder, slot);
    assert_eq!(evicted, None);
    assert_eq!(order.len(), 1, "kein zweiter Eintrag für denselben Spieler");
}

#[test]
// Ist alles belegt, fliegt der AELTESTE raus und der Neue bekommt GENAU dessen Platz.
//
// Das ist der Kern des Umbaus: die fruehere Fassung merkte sich in der Schlange nur die
// UUID und suchte den Platz des Verdraengten hinterher in der Skin-Tabelle. Fehlte er
// dort (Spieler weg, Eintrag geraeumt), fiel sie auf Platz 0 zurueck -- und ueberschrieb
// damit den Skin eines voellig unbeteiligten, noch sichtbaren Spielers.
fn voller_atlas_verdraengt_den_aeltesten_und_erbt_dessen_platz() {
    let mut order = std::collections::VecDeque::new();
    let uuid = |n: u8| [n; 16];
    let mut plaetze = std::collections::HashMap::new();
    for n in 0..2u8 {
        let (slot, _) = claim_atlas_slot(&mut order, 2, uuid(n), None, |_| false).expect("Platz");
        plaetze.insert(n, slot);
    }
    let (slot, evicted) = claim_atlas_slot(&mut order, 2, uuid(9), None, |_| false).expect("Platz");
    assert_eq!(evicted, Some(uuid(0)), "der aelteste muss weichen");
    assert_eq!(slot, plaetze[&0], "und genau sein Platz wird übernommen");
    // Der zweite Spieler behaelt seinen -- er darf vom Verdraengen nicht betroffen sein.
    let (unveraendert, _) = claim_atlas_slot(&mut order, 2, uuid(1), Some(plaetze[&1]), |_| false).expect("Platz");
    assert_eq!(unveraendert, plaetze[&1]);
    assert_ne!(unveraendert, slot, "zwei Spieler dürfen nie denselben Platz haben");
}

#[test]
// Ein Atlas ohne Plaetze (zu klein) darf nicht in Platz 0 schreiben, sondern gar nicht.
fn ohne_plaetze_wird_nichts_vergeben() {
    let mut order = std::collections::VecDeque::new();
    assert_eq!(claim_atlas_slot(&mut order, 0, [1; 16], None, |_| false), None);
}

#[test]
// Bogen: zu frueh losgelassen bis zum ersten Pfeil, kurz vor voll bis voll, sonst nicht.
fn bogen_durchdruecken() {
    assert!(bow_hold_through(1, 0), "unter der Mindestladung immer");
    assert!(!bow_hold_through(BOW_MIN_SHOT_TICKS, 0), "Grenze aus: ab dem ersten Pfeil los");
    assert!(bow_hold_through(17, 150), "3 Ticks = 150 ms Rest");
    assert!(!bow_hold_through(16, 150), "4 Ticks Rest sind zu viel");
    assert!(!bow_hold_through(BOW_FULL_SHOT_TICKS, 500), "voll: los");
}

#[test]
// Armbrust: nur kurz vor geladen weiterhalten, Grenze aus = nie, geladen = los.
fn armbrust_durchdruecken() {
    assert!(crossbow_hold_through(22, 25, 150), "3 Ticks = 150 ms Rest");
    assert!(!crossbow_hold_through(21, 25, 150), "4 Ticks Rest sind zu viel");
    assert!(!crossbow_hold_through(24, 25, 0), "Grenze aus");
    assert!(!crossbow_hold_through(25, 25, 500), "geladen: los");
    assert!(crossbow_hold_through(12, 15, 150), "Schnellladen II: 15 Ticks");
}

#[test]
// Alle Platzinhaber gebraucht: es weicht der entfernteste, nicht der aelteste.
fn voll_weicht_der_entfernteste() {
    let mut order = std::collections::VecDeque::new();
    for n in 0..3u8 {
        claim_atlas_slot(&mut order, 3, [n; 16], None, |_| false).expect("Platz");
    }
    let abstand = |u: &[u8; 16]| Some([2.0, 50.0, 7.0][u[0] as usize]);
    let (slot, evicted) = claim_atlas_slot_near(&mut order, 3, [9; 16], None, abstand).expect("Platz");
    assert_eq!(evicted, Some([1; 16]));
    assert_eq!(slot, 1);
    // Ein ungebrauchter geht trotzdem vor, auch wenn er naeher waere.
    let abstand = |u: &[u8; 16]| if u[0] == 2 { None } else { Some(100.0) };
    let (_, evicted) = claim_atlas_slot_near(&mut order, 3, [8; 16], None, abstand).expect("Platz");
    assert_eq!(evicted, Some([2; 16]));
}

#[test]
// Der Fehler: ein Spieler, den man gerade vor sich hat, verlor seinen Skin an
// irgendeinen Namen, der weit weg der Tabellenliste beitrat -- allein weil sein Platz
// laenger belegt war. Verdraengt werden darf nur, wen man gerade NICHT sieht.
fn sichtbarer_spieler_wird_nicht_verdraengt() {
    let mut order = std::collections::VecDeque::new();
    let uuid = |n: u8| [n; 16];
    let mut plaetze = std::collections::HashMap::new();
    for n in 0..3u8 {
        let (slot, _) = claim_atlas_slot(&mut order, 3, uuid(n), None, |_| false).expect("Platz");
        plaetze.insert(n, slot);
    }
    // 0 und 1 stehen im Bild, 2 nicht -- obwohl 0 der aelteste Eintrag ist.
    let sichtbar = |u: &[u8; 16]| *u == uuid(0) || *u == uuid(1);
    let (slot, evicted) = claim_atlas_slot(&mut order, 3, uuid(9), None, sichtbar).expect("Platz");
    assert_eq!(evicted, Some(uuid(2)), "der Unsichtbare muss weichen, nicht der älteste");
    assert_eq!(slot, plaetze[&2]);
}

#[test]
// Sind ausnahmslos alle Platzinhaber sichtbar, gibt es keine schmerzfreie Wahl mehr --
// dann faellt es auf "aeltester zuerst" zurueck, statt gar keinen Platz zu vergeben (der
// Neue bliebe sonst dauerhaft ohne Skin).
fn wenn_alle_sichtbar_sind_gewinnt_wieder_das_alter() {
    let mut order = std::collections::VecDeque::new();
    let uuid = |n: u8| [n; 16];
    for n in 0..2u8 {
        claim_atlas_slot(&mut order, 2, uuid(n), None, |_| false).expect("Platz");
    }
    let (_, evicted) = claim_atlas_slot(&mut order, 2, uuid(9), None, |_| true).expect("Platz");
    assert_eq!(evicted, Some(uuid(0)));
}

#[test]
// Nach dem Verdraengen darf der Verdraengte nicht ZWEIMAL in der Schlange stehen: sonst
// waechst sie ueber die Kapazitaet hinaus und `order.len() < capacity` vergibt Plaetze,
// die es gar nicht gibt.
fn die_schlange_waechst_nicht_ueber_die_kapazitaet() {
    let mut order = std::collections::VecDeque::new();
    let uuid = |n: u8| [n; 16];
    for n in 0..10u8 {
        claim_atlas_slot(&mut order, 3, uuid(n), None, |u| *u == uuid(0)).expect("Platz");
    }
    assert_eq!(order.len(), 3, "die Schlange ist so lang wie der Atlas Plätze hat");
    let mut slots: Vec<usize> = order.iter().map(|(_, s)| *s).collect();
    slots.sort_unstable();
    assert_eq!(slots, vec![0, 1, 2], "jeder Platz genau einmal");
}

// --- Tick-Taktung (s. next_wake/tick_deadline) ---

#[test]
// Der Kern der Entkopplung: liegt ein Tick VOR dem naechsten Frame, muss die Schleife
// fuer den Tick aufwachen. Vorher schlief sie bis zum Frame-Termin, und der Tick kam
// entsprechend zu spaet -- dieser Verzug landet auf jedem Bewegungspaket.
fn tick_termin_schlaegt_frame_termin() {
    let jetzt = Instant::now();
    let frame = jetzt + Duration::from_millis(8);
    let tick = jetzt + Duration::from_millis(3);
    assert_eq!(next_wake(frame, Some(tick)), tick);
}

#[test]
// Umgekehrt darf ein spaeterer Tick den Frame nicht verzoegern -- sonst saenke die
// Bildrate.
fn spaeterer_tick_verzoegert_das_bild_nicht() {
    let jetzt = Instant::now();
    let frame = jetzt + Duration::from_millis(4);
    let tick = jetzt + Duration::from_millis(40);
    assert_eq!(next_wake(frame, Some(tick)), frame);
}

#[test]
// Ohne laufende Physik (Menue, oder eigene Spalte noch nicht geladen) gibt es keinen
// Tick-Termin -- dann darf die Schleife nicht zusaetzlich aufwachen.
fn ohne_physik_bleibt_es_beim_frame_termin() {
    let frame = Instant::now() + Duration::from_millis(8);
    assert_eq!(next_wake(frame, None), frame);
}

#[test]
// Der Tick-Termin ist das Rest-Guthaben bis zum vollen Tick. Bei halb vollem Guthaben
// also eine halbe Tickdauer -- 25 ms.
fn tick_termin_kommt_aus_dem_restguthaben() {
    let jetzt = Instant::now();
    let halb = tick_deadline(jetzt, physics::TICK_DT / 2.0);
    let rest = halb.duration_since(jetzt).as_secs_f32();
    assert!((rest - physics::TICK_DT / 2.0).abs() < 1e-4, "Rest war {rest}");
    // Leeres Guthaben -> eine ganze Tickdauer.
    let leer = tick_deadline(jetzt, 0.0).duration_since(jetzt).as_secs_f32();
    assert!((leer - physics::TICK_DT).abs() < 1e-4, "Rest war {leer}");
}

#[test]
// Ein volles (oder durch Rundung leicht uebervolles) Guthaben darf nicht in die
// Vergangenheit zeigen und schon gar nicht ueberlaufen -- Instant-Subtraktion unter Null
// ist in Rust eine Panik.
fn volles_guthaben_ergibt_sofort_und_panickt_nicht() {
    let jetzt = Instant::now();
    assert_eq!(tick_deadline(jetzt, physics::TICK_DT), jetzt);
    assert_eq!(tick_deadline(jetzt, physics::TICK_DT * 3.0), jetzt, "auch uebervoll");
}

#[test]
// Die Zahl, um die es geht: bei 120 FPS lag der schlimmste Fall bei einem ganzen
// Frame-Intervall Verzug. Mit Tick-Aufwachen ist der Verzug hoechstens die
// Scheduler-Ungenauigkeit -- der Termin selbst stimmt exakt.
fn worst_case_verzug_verschwindet() {
    let jetzt = Instant::now();
    let frame_intervall = Duration::from_secs_f64(1.0 / 120.0);
    // Tick wird 1 ms nach dem letzten Aufwachen faellig, der Frame erst in 8,3 ms.
    let tick = jetzt + Duration::from_millis(1);
    let frame = jetzt + frame_intervall;
    let wake = next_wake(frame, Some(tick));
    let verzug = wake.duration_since(tick);
    assert_eq!(verzug, Duration::ZERO, "der Tick darf keinen Verzug mehr erben");
    assert!(wake < frame, "und er muss VOR dem Frame liegen");
}

// Angesprungene Abschnitte stehen SECTION_TOP_MARGIN tiefer als die erste Zeile; der erste
// ganz oben.
#[test]
fn abschnitt_steht_knapp_unter_den_reitern() {
    let page = MenuScreen::SettingsVideo.settings_page().unwrap();
    let starts = page.section_starts();
    assert_eq!(page.section_stand(0), 0.0);
    for &(_, row) in &page.sections[1..] {
        let y = settings_row_pos(row as f32, starts) - settings_row_pos(page.section_stand(row), starts);
        assert!((y - SECTION_TOP_MARGIN).abs() < 1e-3, "Zeile {row}: {y}");
    }
}
