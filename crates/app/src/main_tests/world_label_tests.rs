use super::*;

fn span(text: &str, color: Option<u32>) -> protocol::nbt::TextSpan {
    protocol::nbt::TextSpan { text: text.to_string(), color, link: None, action: None, style: 0, shadow: None }
}

// Ein Zeilenumbruch MITTEN in einem Abschnitt teilt ihn, die Farbe laeuft weiter.
#[test]
fn zeilenumbruch_teilt_den_abschnitt_und_behaelt_die_farbe() {
    let lines = split_label_lines(&[span("oben\nunten", Some(0xFF0000))]);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0][0].text, "oben");
    assert_eq!(lines[1][0].text, "unten");
    assert_eq!(lines[1][0].color, Some(0xFF0000));
}

// Ein abschliessender Umbruch soll KEINE leere Zeile erzeugen (die zoege den
// Hintergrund um eine Zeile in die Laenge), eine Leerzeile mittendrin dagegen schon --
// die ist gesetzter Abstand.
#[test]
fn leerzeile_am_ende_faellt_weg_mittendrin_bleibt_sie() {
    assert_eq!(split_label_lines(&[span("a\n", None)]).len(), 1);
    assert_eq!(split_label_lines(&[span("a\n\nb", None)]).len(), 3);
}

// Umbruch an Wortgrenzen; das Leerzeichen am Zeilenende verschwindet.
#[test]
fn umbruch_trennt_an_wortgrenzen() {
    // Messung: ein Zeichen = ein Pixel, damit die Erwartung im Test lesbar bleibt.
    let breite = |t: &str| t.chars().count() as f32;
    let zeilen = wrap_label_lines(&[vec![span("aaa bbb ccc", None)]], 7.0, breite);
    assert_eq!(zeilen.len(), 2);
    assert_eq!(zeilen[0][0].text, "aaa bbb");
    assert_eq!(zeilen[1][0].text, "ccc");
}

// Ein einzelnes Wort, das allein schon zu breit ist, wird nicht zerschnitten (Vanillas
// TextRenderer.wrapLines tut das auch nicht) -- es darf aber die naechsten Woerter nicht
// mit in seine Zeile ziehen.
#[test]
fn zu_langes_wort_bleibt_ganz() {
    let breite = |t: &str| t.chars().count() as f32;
    let zeilen = wrap_label_lines(&[vec![span("unteilbarwort kurz", None)]], 5.0, breite);
    assert_eq!(zeilen.len(), 2);
    assert_eq!(zeilen[0][0].text, "unteilbarwort");
    assert_eq!(zeilen[1][0].text, "kurz");
}

// Ohne Umbruchbreite (Namensschild) bleibt alles in einer Zeile.
#[test]
fn ohne_umbruchbreite_bleibt_die_zeile_ganz() {
    let breite = |t: &str| t.chars().count() as f32;
    let zeilen = wrap_label_lines(&[vec![span("sehr langer name", None)]], f32::INFINITY, breite);
    assert_eq!(zeilen.len(), 1);
    assert_eq!(zeilen[0][0].text, "sehr langer name");
}

// Vanillas Hintergrundfarbe ist ARGB, nicht RGBA -- ein vertauschtes Alpha faerbte den
// Schleier hinter jedem Hologramm falsch.
#[test]
fn argb_wird_richtig_zerlegt() {
    // TextDisplayEntity.INITIAL_BACKGROUND = 25 % Schwarz.
    let rgba = argb_to_rgba(protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND);
    assert_eq!(rgba[0..3], [0.0, 0.0, 0.0]);
    assert!((rgba[3] - 0.25).abs() < 0.01, "Alpha war {}", rgba[3]);
    // Alpha 0 heisst "kein Hintergrund" und muss als solches ankommen.
    assert_eq!(argb_to_rgba(0x00FF_FFFF)[3], 0.0);
}

// §7 (0xAAAAAA) muss linear ~0.402 sein, sonst zeigt das sRGB-Ziel es heller als Vanilla.
#[test]
fn textfarben_werden_nach_linear_gewandelt() {
    let grau = rgb_to_rgba(0xAAAAAA);
    assert!((grau[0] - 0.402).abs() < 0.002, "war {}", grau[0]);
    assert_eq!(rgb_to_rgba(0xFFFFFF), [1.0, 1.0, 1.0, 1.0]);
}
