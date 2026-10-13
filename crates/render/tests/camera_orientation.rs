// Haelt die Kamera-Konvention fest: sie ist die von Minecraft, weil der Yaw spaeter
// unveraendert an den Server geht (Set Player Rotation). Ein Vorzeichenfehler faellt hier
// sonst erst auf, wenn der Server einen woanders hinschauen sieht als man selbst.

use glam::Vec3;
use render::Camera;

fn approx(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-5
}

#[test]
fn yaw_zero_looks_south() {
    let cam = Camera::new(glam::DVec3::ZERO);
    assert!(approx(cam.forward(), Vec3::Z), "yaw=0 muss nach +Z (Sueden) schauen, war {:?}", cam.forward());
}

#[test]
fn rechts_von_sueden_ist_westen() {
    let cam = Camera::new(glam::DVec3::ZERO);
    assert!(approx(cam.right(), -Vec3::X), "Blick nach Sueden -> rechts ist -X (Westen), war {:?}", cam.right());
}

// Der eigentliche Fehler, der im Client sichtbar wurde: Maus nach rechts erhoeht den Yaw,
// die Kamera drehte sich aber nach links.
#[test]
fn steigender_yaw_dreht_nach_rechts() {
    let mut cam = Camera::new(glam::DVec3::ZERO);
    let right_before = cam.right();

    cam.yaw += 10.0; // Maus nach rechts (Grad)
    let moved = cam.forward() - Vec3::Z;

    assert!(moved.dot(right_before) > 0.0, "Blickrichtung muss sich zur rechten Seite verschieben, ging aber nach {moved:?}");
}

#[test]
fn viertel_drehung_rechts_schaut_nach_westen() {
    let mut cam = Camera::new(glam::DVec3::ZERO);
    cam.yaw = 90.0;
    assert!(approx(cam.forward(), -Vec3::X), "yaw=90 Grad muss nach -X (Westen) schauen, war {:?}", cam.forward());
}

#[test]
fn positiver_pitch_schaut_nach_oben() {
    let mut cam = Camera::new(glam::DVec3::ZERO);
    cam.pitch = 30.0;
    assert!(cam.forward().y > 0.0, "positiver Pitch muss nach oben schauen");
}
