use super::*;

#[test]
fn naechste_spalte_kommt_zuerst() {
    let mut spalten = [(5, 0), (1, 0), (3, 0)];
    sortiere_nach_abstand(&mut spalten, (0, 0));
    assert_eq!(spalten, [(1, 0), (3, 0), (5, 0)]);
}

// Wasser wird als umgedrehte Liste gezeichnet (s. render) -- umgedreht muss also die
// FERNSTE Spalte zuerst kommen, sonst mischt naeheres Wasser unter fernerem.
#[test]
fn umgedreht_ergibt_hinten_nach_vorn() {
    let mut spalten = [(1, 0), (9, 0), (4, 0)];
    sortiere_nach_abstand(&mut spalten, (0, 0));
    let hinten_nach_vorn: Vec<_> = spalten.iter().rev().copied().collect();
    assert_eq!(hinten_nach_vorn, vec![(9, 0), (4, 0), (1, 0)]);
}

// Ein ganzer Ring hat denselben Abstand -- ohne festen Tiebreaker haenge die Reihenfolge
// wieder an der HashMap und kippte beim Laufen von Frame zu Frame.
#[test]
fn gleicher_abstand_wird_stabil_geordnet() {
    let ring = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let mut a = ring;
    let mut b = [(0, -1), (-1, 0), (0, 1), (1, 0)]; // dieselbe Menge, andere Startfolge
    sortiere_nach_abstand(&mut a, (0, 0));
    sortiere_nach_abstand(&mut b, (0, 0));
    assert_eq!(a, b, "gleich weit entfernte Spalten muessen unabhaengig von der Eingabefolge gleich landen");
}

// Die Ordnung haengt an der Kamera-Spalte, nicht am Ursprung -- sonst waere das Sortieren
// nach einem Spaltenwechsel wirkungslos.
#[test]
fn reihenfolge_folgt_der_kameraspalte() {
    let mut spalten = [(0, 0), (10, 0)];
    sortiere_nach_abstand(&mut spalten, (12, 0));
    assert_eq!(spalten, [(10, 0), (0, 0)], "von (12,0) aus ist (10,0) die naechste Spalte");
}

// Weit entfernte Spalten (Teleport, Weltgrenze) duerfen die Abstandsrechnung nicht zum
// Ueberlaufen bringen -- in i32 waere 1.9 Mio quadriert laengst ueber i32::MAX.
#[test]
fn sehr_weite_abstaende_ueberlaufen_nicht() {
    let mut spalten = [(1_800_000, 0), (5, 0)];
    sortiere_nach_abstand(&mut spalten, (0, 0));
    assert_eq!(spalten, [(5, 0), (1_800_000, 0)]);
}
