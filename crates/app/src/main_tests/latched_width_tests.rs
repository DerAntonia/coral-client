use super::latch_width as einrasten;

#[test]
// 119 -> 120 -> 99 -> 1000 -> 60: die Breite waechst mit, faellt aber nie zurueck.
fn breite_waechst_und_schrumpft_nie() {
    let mut w = std::collections::HashMap::new();
    assert_eq!(einrasten(&mut w, "fps", 30.0), 30.0);
    assert_eq!(einrasten(&mut w, "fps", 32.0), 32.0);
    assert_eq!(einrasten(&mut w, "fps", 24.0), 32.0);
    assert_eq!(einrasten(&mut w, "fps", 40.0), 40.0);
    assert_eq!(einrasten(&mut w, "fps", 24.0), 40.0);
    assert_eq!(einrasten(&mut w, "work", 10.0), 10.0, "je Schlüssel eigene Breite");
}
