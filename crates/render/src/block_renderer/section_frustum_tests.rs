use super::*;
use glam::{Mat4, Vec3};

#[test]
// Kamera auf Hoehe 0, Blick waagerecht nach +Z, 70 Grad: von einer Spalte 32..48 voraus
// bleiben nur die Sections um die Augenhoehe, die hoch oben und tief unten fallen weg.
fn nur_sections_im_bild_bleiben() {
    let proj = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.05, 1000.0);
    let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
    let frustum = Frustum::from_view_proj(proj * view);
    let alle = (1u64 << 24) - 1;
    let drin = sections_in_frustum((0, 2), -64, alle, &frustum, DVec3::ZERO);
    // Augenhoehe 0 liegt in Section 4 (ab -64).
    assert!(drin & (1 << 4) != 0, "{drin:024b}");
    assert_eq!(drin & 1, 0, "y -64..-48 liegt unter dem Bild: {drin:024b}");
    assert_eq!(drin & (1 << 23), 0, "y 304..320 liegt ueber dem Bild: {drin:024b}");
    // Hinter der Kamera gar nichts.
    assert_eq!(sections_in_frustum((0, -3), -64, alle, &frustum, DVec3::ZERO), 0);
}
