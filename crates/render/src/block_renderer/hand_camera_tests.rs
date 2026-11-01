use super::*;

// Die echten Vertices der Ego-Hand -- derselbe Weg wie in App::build_hand_vertices, damit
// der Test die tatsaechliche Geometrie prueft und nicht eine Nachbildung davon. Ein erster
// Anlauf hat den Pivot des Arm-Teils von Hand nachgerechnet und dabei uebersehen, dass der
// Pivot die SCHULTER ist: die liegt korrekterweise unterhalb des Bildrands, der Arm ragt
// von unten rechts herein. Gemessen wird deshalb die ganze Punktwolke.
fn hand_vertices() -> Vec<glam::Vec3> {
    let model = world::player_arm_model(false, true);
    let pose = world::first_person_arm_pose(true);
    let base = world::first_person_arm_transform(true, 0.0, 0.0);
    let mut out = Vec::new();
    world::emit_entity_model(&model, &pose, base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
    out.into_iter().map(|v| glam::Vec3::from(v.position)).collect()
}

fn auf_den_schirm(punkt: glam::Vec3, proj: glam::Mat4) -> glam::Vec2 {
    let c = proj * punkt.extend(1.0);
    glam::Vec2::new(c.x / c.w, c.y / c.w)
}

#[test]
// Der Arm muss im Bild ankommen, und zwar in der unteren rechten Ecke. Stimmt das
// Sichtfeld nicht, rutscht er heraus, besonders bei hohen
// FOV-Einstellungen.
fn arm_ragt_von_unten_rechts_ins_bild() {
    let proj = hand_projection(16.0 / 9.0);
    let punkte: Vec<glam::Vec2> = hand_vertices().into_iter().map(|p| auf_den_schirm(p, proj)).collect();
    assert!(!punkte.is_empty(), "der Arm muss ueberhaupt Geometrie haben");
    let sichtbar = punkte.iter().filter(|s| s.x.abs() <= 1.0 && s.y.abs() <= 1.0).count();
    assert!(sichtbar > punkte.len() / 4, "zu wenig vom Arm im Bild: {sichtbar} von {}", punkte.len());
    // ... und zwar unten rechts: jeder sichtbare Punkt liegt rechts der Mitte und darunter.
    for s in punkte.iter().filter(|s| s.x.abs() <= 1.0 && s.y.abs() <= 1.0) {
        assert!(s.x > 0.0, "Arm gehoert nach rechts: {s:?}");
        assert!(s.y < 0.0, "Arm gehoert nach unten: {s:?}");
    }
}

#[test]
// Die Kernaussage: die Hand haengt NICHT am eingestellten Sichtfeld. Bei FOV 110 -- ein bei
// PvP durchaus ueblicher Wert -- rutscht der Arm sichtbar weg. Die Hand-Projektion liefert
// unveraendert dieselbe Stelle.
//
// Der Vergleichswert zeigt, wie gross der Fehler war, als hier der Einstellwert
// durchgereicht wurde.
fn hand_folgt_der_fov_einstellung_nicht() {
    let aspect = 16.0 / 9.0;
    let punkte = hand_vertices();
    let richtig = hand_projection(aspect);
    let falsch = glam::Mat4::perspective_rh(110.0f32.to_radians(), aspect, HAND_NEAR_Z, HAND_FAR_Z);
    let versatz = punkte.iter().map(|p| (auf_den_schirm(*p, richtig) - auf_den_schirm(*p, falsch)).length()).fold(0.0f32, f32::max);
    assert!(versatz > 0.2, "der Fehler war deutlich sichtbar, Versatz war nur {versatz}");
}

#[test]
// Nahebene und Fernebene stammen aus Vanillas Hud-Projektion (0.05 und 100).
// Kein Punkt des Arms darf an einer der beiden Ebenen abgeschnitten werden.
fn arm_liegt_zwischen_nah_und_fernebene() {
    for p in hand_vertices() {
        let tiefe = -p.z;
        assert!(tiefe > HAND_NEAR_Z, "an der Nahebene abgeschnitten: {tiefe}");
        assert!(tiefe < HAND_FAR_Z, "hinter der Fernebene: {tiefe}");
    }
}
