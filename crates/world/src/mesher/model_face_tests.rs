use super::*;

fn normale(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

#[test]
fn modell_oberseite_zeigt_wie_die_wuerfel_oberseite() {
    let w = face_quad_corners([0.0; 3], [1.0; 3], [0, 1, 0]);
    let wuerfel = normale(w[0], w[1], w[2]);
    // assets::ModelQuad, Oberseite: (x0,z0), (x1,z0), (x1,z1), (x0,z1).
    let m = [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]];
    let modell = normale(m[0], m[2], m[1]);
    assert!(wuerfel[1] > 0.0 && modell[1] > 0.0, "beide Umlaeufe muessen dieselbe Seite als vorn werten: {wuerfel:?} / {modell:?}");
}
