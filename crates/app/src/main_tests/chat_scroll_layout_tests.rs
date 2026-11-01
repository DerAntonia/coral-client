use super::chat_scroll_layout;

// Wo eine Zeile GEZEICHNET wird, in Zeilen unterhalb der Blockoberkante. Der Text klebt
// an der Unterkante des (mitgewachsenen) Blocks -- genau wie in draw_chat_lines.
fn lagen(gesamt: usize, basis: usize, max_wachstum: usize, scroll: usize) -> Vec<(usize, usize)> {
    let (wachstum, anfang, ende) = chat_scroll_layout(gesamt, basis, max_wachstum, scroll);
    let hoehe = basis + wachstum;
    let oben = hoehe - (ende - anfang);
    (anfang..ende).map(|i| (i, oben + (i - anfang))).collect()
}

// Die Beschwerde im Klartext: "wenn ich eine Zeile nach oben scrolle, dann rueckt das
// eine op failed eine Zeile nach unten" -- und zwar OHNE dass unten etwas wegfaellt,
// solange unter dem Chat noch Platz ist.
#[test]
fn eine_radstufe_rueckt_alles_um_genau_eine_zeile_nach_unten() {
    let (gesamt, basis, platz) = (60, 18, 15);
    for scroll in 1..=platz {
        let vorher: std::collections::HashMap<_, _> = lagen(gesamt, basis, platz, scroll - 1).into_iter().collect();
        let nachher: std::collections::HashMap<_, _> = lagen(gesamt, basis, platz, scroll).into_iter().collect();
        for (zeile, alte_lage) in &vorher {
            let neue_lage = nachher.get(zeile).unwrap_or_else(|| panic!("Zeile {zeile} ist bei Radstufe {scroll} verschwunden"));
            assert_eq!(*neue_lage, alte_lage + 1, "Zeile {zeile} muss bei Radstufe {scroll} genau eine Zeile tiefer stehen");
        }
    }
}

// Die oberste Zeile bleibt dabei liegen, es steht nur eine aeltere Nachricht darin --
// sonst saehe es aus, als spraenge der ganze Block.
#[test]
fn die_oberste_zeile_bleibt_liegen_und_wird_aelter() {
    let (gesamt, basis, platz) = (60, 18, 15);
    for scroll in 0..=platz {
        let l = lagen(gesamt, basis, platz, scroll);
        assert_eq!(l.first().expect("nicht leer").1, 0, "oberste Zeile bei Radstufe {scroll}");
        assert_eq!(l.first().expect("nicht leer").0, gesamt - basis - scroll, "und zwar eine immer aeltere");
    }
}

// Ist der Platz unter dem Chat aufgebraucht, blaettert es wie gewohnt weiter: die
// neueste Zeile faellt unten weg, die Bewegung bleibt aber eine Zeile pro Radstufe.
#[test]
fn nach_dem_platz_blaettert_es_wieder_gewoehnlich() {
    let (gesamt, basis, platz) = (60, 18, 15);
    let vorher: std::collections::HashMap<_, _> = lagen(gesamt, basis, platz, platz).into_iter().collect();
    let nachher: std::collections::HashMap<_, _> = lagen(gesamt, basis, platz, platz + 1).into_iter().collect();
    assert!(!nachher.contains_key(&(gesamt - 1)), "die neueste Zeile fällt jetzt weg");
    for (zeile, alte_lage) in vorher.iter().filter(|(z, _)| nachher.contains_key(z)) {
        assert_eq!(nachher[zeile], alte_lage + 1, "Zeile {zeile} rückt weiter um eine Zeile");
    }
    let (wachstum, _, _) = chat_scroll_layout(gesamt, basis, platz, platz + 5);
    assert_eq!(wachstum, platz, "der Block wächst nicht über den Platz hinaus");
}

// Kurzer Verlauf: es darf nie mehr gezeichnet werden, als es gibt.
#[test]
fn ein_kurzer_verlauf_bleibt_in_seinen_grenzen() {
    let (wachstum, anfang, ende) = chat_scroll_layout(3, 18, 15, 0);
    assert_eq!((wachstum, anfang, ende), (0, 0, 3));
}
