use super::*;

// Projiziert einen Punkt der Textebene eines Displays, das `abstand` Bloecke vor der
// Kamera steht.
fn projiziere(cam: &render::Camera, basis_rot: glam::Mat4, p: (f32, f32)) -> (f32, f32) {
    let rel = cam.view_forward() * 5.0;
    let frame = glam::Mat4::from_translation(rel) * basis_rot * display_text_frame(40.0, 9.0);
    let c = cam.view_proj_matrix(16.0 / 9.0) * frame * glam::Vec4::new(p.0, p.1, 0.0, 1.0);
    (c.x / c.w, c.y / c.w)
}

#[test]
// CENTER dreht sich wie ein Namensschild zur Kamera: aus jedem Blickwinkel lesbar, also
// links oben links (kleineres NDC-x) und oben (groesseres NDC-y).
fn center_ist_aus_jedem_winkel_lesbar() {
    for (yaw, pitch) in [(0.0, 0.0), (37.0, 20.0), (-120.0, -45.0), (200.0, 60.0)] {
        let mut cam = render::Camera::new(glam::DVec3::ZERO);
        cam.yaw = yaw;
        cam.pitch = pitch;
        // camera.pitch zaehlt nach oben, Minecraft nach unten.
        let rot = display_billboard_matrix(3, 0.0, 0.0, yaw, -pitch);
        let oben_links = projiziere(&cam, rot, (0.0, 0.0));
        let oben_rechts = projiziere(&cam, rot, (40.0, 0.0));
        let unten_links = projiziere(&cam, rot, (0.0, 9.0));
        assert!(oben_rechts.0 > oben_links.0, "gespiegelt bei yaw {yaw}");
        assert!(unten_links.1 < oben_links.1, "kopfueber bei yaw {yaw}");
        assert!((oben_rechts.1 - oben_links.1).abs() < 1e-4, "schraeg bei yaw {yaw}");
    }
}

#[test]
// FIXED mit Yaw 0 steht fest: Blick nach Norden (-Z) sieht die Vorderseite, Blick nach
// Sueden (+Z) die gespiegelte Rueckseite.
fn fixed_steht_fest_im_raum() {
    let rot = display_billboard_matrix(0, 0.0, 0.0, 0.0, 0.0);
    let mut nord = render::Camera::new(glam::DVec3::ZERO);
    nord.yaw = 180.0;
    let (l, r) = (projiziere(&nord, rot, (0.0, 0.0)), projiziere(&nord, rot, (40.0, 0.0)));
    assert!(r.0 > l.0, "von vorn lesbar");
    let sued = render::Camera::new(glam::DVec3::ZERO);
    let (l, r) = (projiziere(&sued, rot, (0.0, 0.0)), projiziere(&sued, rot, (40.0, 0.0)));
    assert!(r.0 < l.0, "von hinten gespiegelt");
}

#[test]
fn affine_aus_mat4_transformiert_gleich() {
    let m = glam::Mat4::from_translation(glam::Vec3::new(1.0, 2.0, 3.0))
        * glam::Mat4::from_rotation_y(0.7)
        * glam::Mat4::from_scale(glam::Vec3::new(2.0, 0.5, 1.0));
    let p = [0.3, -1.2, 4.0];
    let a = affine_from_mat4(m).apply(p);
    let b = m.transform_point3(glam::Vec3::from(p));
    assert!((glam::Vec3::from(a) - b).length() < 1e-5, "{a:?} != {b:?}");
}

#[test]
fn joml_quaternion_gleicht_glam_bei_normierten_werten() {
    let q = glam::Quat::from_euler(glam::EulerRot::XYZ, 0.3, -1.1, 2.0);
    let a = joml_quat_matrix([q.x, q.y, q.z, q.w]);
    let b = glam::Mat4::from_quat(q);
    assert!(a.abs_diff_eq(b, 1e-5), "{a:?} != {b:?}");
}
