use super::*;

// Flaechennormale aus den ersten drei Eckpunkten (Kreuzprodukt der beiden Kanten).
// Zeigt bei Wicklung gegen den Uhrzeigersinn (von aussen gesehen) nach aussen.
fn normale(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
    [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]
}

fn skalarprodukt(a: [f32; 3], b: [i32; 3]) -> f32 {
    a[0] * b[0] as f32 + a[1] * b[1] as f32 + a[2] * b[2] as f32
}

// Alle sechs Wuerfelseiten muessen nach AUSSEN winden -- und zwar BEIDE Dreiecke jedes
// Quads (push_quad_with_uv gibt v0,v1,v2 + v0,v2,v3 aus; ein Fehler nur im zweiten
// Dreieck ergaebe ein halb durchsichtiges Quad).
#[test]
fn wuerfelflaechen_winden_konsistent_nach_aussen() {
    for dir in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
        let mut out = Vec::new();
        push_face_quad(&mut out, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], dir, [1.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(out.len(), 6, "ein Quad sind zwei Dreiecke");
        for (i, ecken) in [[0, 1, 2], [3, 4, 5]].into_iter().enumerate() {
            let n = normale(out[ecken[0]].position, out[ecken[1]].position, out[ecken[2]].position);
            assert!(
                skalarprodukt(n, dir) > 0.0,
                "Seite {dir:?}, Dreieck {i}: Normale {n:?} zeigt nach innen -- diese Flaeche waere mit Culling unsichtbar"
            );
        }
    }
}

// Pflanzen sind schraege Ebenen ohne Aussenseite -- sie MUESSEN beidseitig ausgegeben
// werden, sonst verschwinden sie mit Culling aus genau einer Blickrichtung.
#[test]
fn kreuzbloecke_sind_beidseitig() {
    let mut out = Vec::new();
    push_cross_quads(&mut out, ColumnOrigin { x: 0, z: 0 }, 0, 0, 0, [1.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    // Zwei Ebenen, jede doppelt, jedes Quad zwei Dreiecke = 4 * 6 Vertices.
    assert_eq!(out.len(), 24, "jede der beiden Ebenen muss in beiden Wicklungen vorliegen");

    // Zu jedem Dreieck muss es eines mit entgegengesetzter Normale geben.
    let normalen: Vec<[f32; 3]> =
        (0..out.len() / 3).map(|t| normale(out[3 * t].position, out[3 * t + 1].position, out[3 * t + 2].position)).collect();
    for n in &normalen {
        let gegenstueck = normalen.iter().any(|m| {
            let summe = [n[0] + m[0], n[1] + m[1], n[2] + m[2]];
            summe.iter().all(|c| c.abs() < 1e-3)
        });
        assert!(gegenstueck, "zur Normale {n:?} fehlt die Rueckseite -- die Pflanze waere von einer Seite unsichtbar");
    }
}
