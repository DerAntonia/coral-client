use super::*;

// Eine Zeile aus gleich breiten Zeichen (10 Pixel), Oberkante `top`, 16 Pixel hoch.
fn zeile(text: &str, x: f32, top: f32) -> TextRun {
    let glyphs = text.chars().enumerate().map(|(i, c)| (c, x + i as f32 * 10.0, x + (i + 1) as f32 * 10.0)).collect();
    TextRun { top, bottom: top + 16.0, glyphs, group: 0 }
}

fn auswahl(runs: &[TextRun], a: (f32, f32), b: (f32, f32)) -> String {
    selection_text(runs, &select_in_runs(runs, a, b))
}

#[test]
// Wie im Browser: von der Mitte der ersten Zeile bis in die zweite hinein, in
// Lesereihenfolge -- nicht als Rechteck.
fn auswahl_folgt_der_lesereihenfolge() {
    let runs = [zeile("hallo welt", 0.0, 0.0), zeile("zweite zeile", 0.0, 20.0)];
    assert_eq!(auswahl(&runs, (60.0, 8.0), (60.0, 28.0)), "welt\nzweite");
    // Rueckwaerts gezogen ergibt dasselbe.
    assert_eq!(auswahl(&runs, (60.0, 28.0), (60.0, 8.0)), "welt\nzweite");
}

#[test]
fn innerhalb_einer_zeile() {
    let runs = [zeile("abcdef", 0.0, 0.0)];
    assert_eq!(auswahl(&runs, (12.0, 5.0), (38.0, 5.0)), "bcd");
    // Ein Punkt ueber allem Text faengt am Anfang an.
    assert_eq!(auswahl(&runs, (0.0, -50.0), (28.0, 5.0)), "abc");
}

#[test]
// Getrennte Stuecke derselben Zeile (Beschriftung links, Wert rechts) bekommen ein
// Leerzeichen dazwischen; direkt aneinanderstossende (Farbwechsel im Chat) nicht.
fn stuecke_einer_zeile_werden_sinnvoll_verbunden() {
    let getrennt = [zeile("Breite", 0.0, 0.0), zeile("160", 300.0, 0.0)];
    assert_eq!(auswahl(&getrennt, (0.0, 5.0), (400.0, 5.0)), "Breite 160");
    let stossend = [zeile("<Steve> ", 0.0, 0.0), zeile("hi", 80.0, 0.0)];
    assert_eq!(auswahl(&stossend, (0.0, 5.0), (200.0, 5.0)), "<Steve> hi");
}

#[test]
// Zwei Listen nebeneinander (Log links, Serverliste rechts): die Auswahl bleibt in der
// Liste, in der sie beginnt, auch wenn sie ueber die andere hinweggezogen wird.
fn auswahl_bleibt_in_ihrem_bereich() {
    let mut log = [zeile("log eins", 0.0, 0.0), zeile("log zwei", 0.0, 20.0)];
    let mut server = [zeile("server a", 300.0, 0.0), zeile("server b", 300.0, 20.0)];
    server.iter_mut().for_each(|r| r.group = 1);
    log.iter_mut().for_each(|r| r.group = 0);
    let runs: Vec<TextRun> = log.into_iter().chain(server).collect();
    assert_eq!(auswahl(&runs, (0.0, 5.0), (400.0, 25.0)), "log eins\nlog zwei");
    // Rueckwaerts aus der Serverliste nach links unten gezogen: nur Serverliste.
    assert_eq!(auswahl(&runs, (305.0, 5.0), (0.0, 25.0)), "server a");
    assert_eq!(auswahl(&runs, (305.0, 5.0), (400.0, 25.0)), "server a\nserver b");
}

fn einheit(runs: &[TextRun], a: (f32, f32), b: (f32, f32), clicks: u8) -> String {
    selection_text(runs, &select_in_runs_by_unit(runs, a, b, clicks))
}

#[test]
// Doppelklick: das Wort unter dem Zeiger, auch ueber einen Farbwechsel hinweg.
fn doppelklick_waehlt_das_wort() {
    let runs = [zeile("hallo we", 0.0, 0.0), zeile("lt da", 80.0, 0.0)];
    assert_eq!(einheit(&runs, (65.0, 5.0), (65.0, 5.0), 2), "welt");
    assert_eq!(einheit(&runs, (15.0, 5.0), (15.0, 5.0), 2), "hallo");
    // Ziehen nach dem Doppelklick erweitert in ganzen Woertern.
    assert_eq!(einheit(&runs, (15.0, 5.0), (65.0, 5.0), 2), "hallo welt");
    // Getrennt stehende Texte sind verschiedene Woerter.
    let getrennt = [zeile("Breite", 0.0, 0.0), zeile("160", 300.0, 0.0)];
    assert_eq!(einheit(&getrennt, (305.0, 5.0), (305.0, 5.0), 2), "160");
    // Kein Text unter dem Zeiger: keine Auswahl.
    assert_eq!(einheit(&runs, (500.0, 5.0), (500.0, 5.0), 2), "");
}

#[test]
fn dreifachklick_nimmt_keinen_text_daneben_mit() {
    // Knopfbeschriftung links, Statistik rechts auf derselben Hoehe.
    let runs = [zeile("Disconnect", 0.0, 0.0), zeile("Chunks: 213", 300.0, 0.0)];
    assert_eq!(einheit(&runs, (320.0, 5.0), (320.0, 5.0), 3), "Chunks: 213");
    assert_eq!(einheit(&runs, (15.0, 5.0), (15.0, 5.0), 3), "Disconnect");
}

#[test]
fn dreifachklick_waehlt_die_zeile() {
    let runs = [zeile("hallo we", 0.0, 0.0), zeile("lt da", 80.0, 0.0), zeile("zweite zeile", 0.0, 20.0)];
    assert_eq!(einheit(&runs, (15.0, 5.0), (15.0, 5.0), 3), "hallo welt da");
    assert_eq!(einheit(&runs, (15.0, 25.0), (15.0, 5.0), 3), "hallo welt da\nzweite zeile");
}
