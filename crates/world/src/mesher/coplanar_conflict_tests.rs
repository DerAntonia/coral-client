use super::*;

fn box_at(min: [f32; 3], max: [f32; 3]) -> ShapeBox {
    ShapeBox { min, max, faces: [true; 6], content: [false; 6], uv: [None; 6] }
}

// Nachgestellt: Mauer-Pfosten (4..12 in X/Z, Y 0..16) + ein "tall" verbundener Arm
// (X 5..11, Z 0..8, Y 0..16 -- reicht ueber die Blockmitte hinaus in den Pfosten hinein,
// s. template_wall_post.json/template_wall_side_tall.json). Beide haben ihre Oben-
// Flaeche exakt bei y=1 UND ueberlappen sich im Grundriss -- genau der Z-Fighting-Fall.
#[test]
fn pfosten_und_verbundener_arm_haben_einen_konflikt() {
    let pfosten = box_at([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]);
    let arm = box_at([0.3125, 0.0, 0.0], [0.6875, 1.0, 0.5]);
    let shapes = [pfosten, arm];
    // Der Pfosten (Index 0) hat keine FRUEHEREN Boxen zum Konfligieren -- er gewinnt.
    assert_eq!(coplanar_conflict_count(&shapes, 0, FACE_UP), 0);
    // Der Arm (Index 1) konfligiert mit dem Pfosten (Index 0) -- genau einmal.
    assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_UP), 1);
    // Dieselbe Logik gilt fuer die Untenflaeche (beide Boxen beginnen bei y=0).
    assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_DOWN), 1);
}

// Zwei Boxen, die sich NICHT im Grundriss ueberlappen (z.B. Pfosten + Arm nach Norden UND
// Arm nach Sueden, die beide nur bis zur Mitte reichen und sich in Z nicht beruehren),
// duerfen keinen Konflikt melden.
#[test]
fn nicht_ueberlappende_boxen_haben_keinen_konflikt() {
    let a = box_at([0.3125, 0.0, 0.0], [0.6875, 1.0, 0.5]);
    let b = box_at([0.3125, 0.0, 0.5], [0.6875, 1.0, 1.0]);
    // Dieselbe X-Spanne, direkt aneinandergrenzende (nicht ueberlappende) Z-Spannen.
    let shapes = [a, b];
    assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_UP), 0);
}

// Verschiedene Hoehen (z.B. ein "low" statt "tall" verbundener Arm, Oben-Flaeche bei
// y=14/16 statt y=16/16) duerfen ebenfalls keinen Konflikt melden -- der Arm liegt dann
// vollstaendig UNTER der Pfostenflaeche und ist ohnehin unsichtbar, kein Z-Fighting.
#[test]
fn unterschiedliche_hoehen_haben_keinen_konflikt() {
    let pfosten = box_at([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]);
    let niedriger_arm = box_at([0.3125, 0.0, 0.0], [0.6875, 14.0 / 16.0, 0.5]);
    let shapes = [pfosten, niedriger_arm];
    assert_eq!(coplanar_conflict_count(&shapes, 1, FACE_UP), 0);
}
