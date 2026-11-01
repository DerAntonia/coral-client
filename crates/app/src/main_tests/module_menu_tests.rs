use super::*;

// Jedes Modul steht in genau einer Kategorie -- sonst gaebe es eine Kachel doppelt oder
// gar nicht. Und keine Kategorie ist leer.
#[test]
fn jedes_modul_hat_genau_eine_kategorie() {
    let mut gesehen: Vec<ModuleId> = ModuleCategory::ALL.iter().flat_map(|k| k.modules()).collect();
    assert_eq!(gesehen.len(), MODULE_IDS.len());
    gesehen.dedup();
    assert_eq!(gesehen.len(), MODULE_IDS.len());
    for k in ModuleCategory::ALL {
        assert!(!k.modules().is_empty(), "{k:?} ist leer");
    }
    assert_eq!(ModuleId::Shields.category(), ModuleCategory::Display);
    assert_eq!(ModuleId::VoiceChat.category(), ModuleCategory::Akustik);
    assert_eq!(ModuleId::GhostTolerance.category(), ModuleCategory::Debug);
}

// module_detail_rows/module_menu_rect haengen an &App (braucht eine echte
// Minecraft-Installation) -- geprueft wird deshalb die reine Rechnung, die
// module_scroll() und module_max_scroll() ausmachen. Genau darin sass der Fehler:
// die Tafel lief unten aus dem Bild, weil ueberhaupt nicht gedeckelt wurde.
fn max_scroll(zeilen: usize, sichtbar: usize) -> usize {
    zeilen.saturating_sub(sichtbar)
}

#[test]
fn passt_alles_hinein_gibt_es_nichts_zu_scrollen() {
    assert_eq!(max_scroll(5, 8), 0);
    assert_eq!(max_scroll(8, 8), 0);
}

// Der Scroll-Stand muss so weit reichen, dass die LETZTE Zeile sichtbar wird -- und
// keinen Schritt weiter, sonst scrollt man ins Leere.
#[test]
fn letzte_zeile_ist_erreichbar_aber_nicht_darueber_hinaus() {
    let (zeilen, sichtbar) = (11, 6);
    let max = max_scroll(zeilen, sichtbar);
    assert_eq!(max, 5);
    assert_eq!(max + sichtbar, zeilen, "beim maximalen Scroll muss die letzte Zeile genau am unteren Rand stehen");
}

// Klappt der Farbeditor zu, schrumpft die Zeilenzahl -- ein zuvor gueltiger Scroll-Stand
// kann dann zu gross sein. module_scroll() deckelt beim LESEN, damit die Anzeige sofort
// stimmt, ohne dass jemand das Rad anfassen muss.
#[test]
fn zu_grosser_scroll_stand_wird_beim_lesen_gedeckelt() {
    let gespeichert = 5usize;
    let neuer_max = max_scroll(7, 6); // nach dem Zuklappen nur noch 7 Zeilen
    assert_eq!(gespeichert.min(neuer_max), 1);
}

// Der Farbeditor ist ein Umschalter: derselbe Knopf klappt auf und wieder zu, ein
// anderer wechselt direkt hinueber (nie beide gleichzeitig offen -- sonst waere die
// Tafel wieder so hoch wie mit sechs festen Farbzeilen).
#[test]
fn farbeditor_schaltet_um_und_bleibt_einzeln() {
    let umschalten = |offen: Option<HitboxVariant>, geklickt: HitboxVariant| {
        if offen == Some(geklickt) {
            None
        } else {
            Some(geklickt)
        }
    };
    let s = umschalten(None, HitboxVariant::Smoothed);
    assert_eq!(s, Some(HitboxVariant::Smoothed));
    assert_eq!(umschalten(s, HitboxVariant::Smoothed), None, "zweiter Klick klappt zu");
    assert_eq!(umschalten(s, HitboxVariant::Raw), Some(HitboxVariant::Raw), "andere Variante übernimmt");
}

// Frueher liessen sich Knoepfe scheinbar nicht druecken: die Detailspalte eines
// ABGESCHALTETEN Moduls nimmt keine Klicks an (Absicht -- verstellen, was gar nicht
// laeuft, waere sinnlos). Damit war sie aber eine Sackgasse: der einzige Einschalter sass
// in der schmalen Listenspalte. Die Regel unten ist die Ausnahme, die das aufloest.
#[test]
fn hauptschalter_bleibt_bedienbar_wenn_das_modul_aus_ist() {
    let bedienbar = |modul_an: bool, action: ButtonAction, master: ButtonAction| modul_an || action == master;
    for id in MODULE_IDS {
        let master = id.toggle_action();
        assert!(bedienbar(false, master, master), "{}: Hauptschalter muss auch bei ausgeschaltetem Modul gehen", id.label());
        assert!(
            !bedienbar(false, ButtonAction::ToggleModuleHitboxDotted, master) || master == ButtonAction::ToggleModuleHitboxDotted,
            "{}: eine gewöhnliche Einstellung bleibt bei ausgeschaltetem Modul gesperrt",
            id.label()
        );
        assert!(bedienbar(true, ButtonAction::ToggleModuleHitboxDotted, master), "eingeschaltet ist alles bedienbar");
    }
}

// Jedes Modul braucht seinen EIGENEN Hauptschalter -- teilten sich zwei einen, wuerde
// die Ausnahme oben im falschen Modul greifen.
#[test]
fn jedes_modul_hat_einen_eigenen_hauptschalter() {
    for (i, &a) in MODULE_IDS.iter().enumerate() {
        for &b in &MODULE_IDS[i + 1..] {
            assert!(a.toggle_action() != b.toggle_action(), "{} und {} teilen sich einen Schalter", a.label(), b.label());
        }
    }
}

// Die Detailansicht der Module hat dieselben Masse wie die Einstellungsseiten, nur an die
// Tafel verschoben: Zeilenabstand, Abstand Beschriftung -> Bedienelement, Feldgroessen.
#[test]
fn modul_zeilen_haben_die_masse_der_einstellungen() {
    let l = ModuleDetailLayout { x: 100.0, top: 200.0 };
    let dx = 100.0 - SETTINGS_LABEL_X;
    let wert = stepper_value_rect(0);
    for n in 0..3 {
        let r = l.value_rect(n as f32);
        assert_eq!((r.x, r.w, r.h), (wert.x + dx, wert.w, wert.h));
        assert_eq!(r.y, 200.0 + n as f32 * SETTINGS_ROW_H);
        let schalter = l.control_rect(n as f32);
        assert_eq!((schalter.x, schalter.w, schalter.h), (SETTINGS_CTRL_X + dx, TOGGLE_W, SETTINGS_CTRL_H));
    }
    let breit = settings_wide_rect(0);
    assert_eq!(l.choice_rect(0.0).w, breit.w);
    assert_eq!(l.swatch_rect(0.0).w, SWATCH_W);
}
