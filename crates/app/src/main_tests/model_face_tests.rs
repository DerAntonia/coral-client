use super::*;

// Vanillas Kolben (Kopf nach Norden): die Ostseite traegt "rotation": 90, das Holzband
// oben in piston_side muss dann an der Nordkante (z = 0) liegen.
#[test]
fn rotation_90_dreht_im_uhrzeigersinn() {
    let q = assets::ModelQuad {
        corners: [[1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 0.0, 16.0, 16.0],
        texture: "block/piston_side".into(),
        face: 5,
        uv_rotation: 1,
        shade: true,
        tint_index: None,
    };
    let f = model_face(&q, [0.0, 0.0, 1.0, 1.0]);
    for (ecke, uv) in f.corners.iter().zip(f.uvs) {
        assert_eq!(uv[1] == 0.0, ecke[2] == 0.0, "Ecke {ecke:?} hat uv {uv:?}");
    }
}
