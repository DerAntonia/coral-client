use super::*;

// Die drei Pose-Augenhoehen stammen aus PlayerLikeEntity.POSE_DIMENSIONS (Yarn-1.21.11-Jar):
// STANDING 1.62, CROUCHING 1.27 (bei 1.5 Koerperhoehe), SWIMMING/GLIDING/
// SPIN_ATTACK 0.4. Als Test festgehalten, damit die Werte nicht unbemerkt "aufgerundet"
// werden -- sie bestimmen unmittelbar, was der Spieler sieht.
#[test]
fn pose_augenhoehen_entsprechen_vanilla() {
    assert_eq!(EYE_HEIGHT, 1.62);
    assert_eq!(CROUCHING_EYE_HEIGHT, 1.27);
    assert_eq!(SWIMMING_EYE_HEIGHT, 0.4);
}

// Vanillas halbe Annaeherung pro Tick. Der erste Schritt beim Losschleichen ist der
// auffaelligste: 1.62 -> 1.445, NICHT sofort 1.27.
#[test]
fn erster_tick_geht_nur_die_halbe_strecke() {
    let nach_einem_tick = approach_eye_height(EYE_HEIGHT, CROUCHING_EYE_HEIGHT);
    assert!((nach_einem_tick - 1.445).abs() < 1e-6, "war {nach_einem_tick}");
}

// Ueber mehrere Ticks laeuft sie monoton auf den Zielwert zu und schiesst nie darueber
// hinaus -- ein Ueberschwinger waere als Wippen der Kamera sichtbar.
#[test]
fn naehert_sich_monoton_ohne_ueberschwinger() {
    let mut h = EYE_HEIGHT;
    for _ in 0..20 {
        let vorher = h;
        h = approach_eye_height(h, SWIMMING_EYE_HEIGHT);
        assert!(h < vorher, "muss fallen: {vorher} -> {h}");
        assert!(h >= SWIMMING_EYE_HEIGHT, "darf das Ziel nie unterschreiten: {h}");
    }
    assert!((h - SWIMMING_EYE_HEIGHT).abs() < 1e-3, "nach 20 Ticks praktisch angekommen, war {h}");
}

// Steht die Pose still, darf sich nichts mehr bewegen -- sonst zitterte die Kamera dauerhaft.
#[test]
fn am_ziel_bleibt_sie_stehen() {
    assert_eq!(approach_eye_height(EYE_HEIGHT, EYE_HEIGHT), EYE_HEIGHT);
}

// Gegenrichtung (Aufstehen) muss genauso funktionieren wie das Abtauchen.
#[test]
fn aufstehen_laeuft_symmetrisch() {
    let runter = EYE_HEIGHT - approach_eye_height(EYE_HEIGHT, CROUCHING_EYE_HEIGHT);
    let hoch = approach_eye_height(CROUCHING_EYE_HEIGHT, EYE_HEIGHT) - CROUCHING_EYE_HEIGHT;
    assert!((runter - hoch).abs() < 1e-6, "runter {runter}, hoch {hoch}");
}
