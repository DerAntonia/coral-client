use super::*;

// Die BREITE der Kanten laesst sich hier nicht mehr pruefen: sie entsteht erst im
// Vertex-Shader nach der Projektion (s. block.wgsl::vs_hitbox). Was die CPU-Seite
// liefert, ist reine Kanten-Topologie -- und genau die wird hier festgenagelt.
// Dass der Shader selbst uebersetzt, deckt tests/shaders.rs ab.

fn edge(a: Vec3, b: Vec3) -> Vec<render::HitboxVertex> {
    let mut out = Vec::new();
    push_hitbox_edge(&mut out, a, b, [1.0, 1.0, 1.0, 1.0], 1.0);
    out
}

// Ein Band sind zwei Dreiecke = 6 Vertices, und ALLE tragen dieselben beiden Endpunkte.
// Das ist die zentrale Zusicherung fuer vs_hitbox: der Shader bildet aus a und b die
// Bildschirmrichtung der Kante -- traegt auch nur ein Vertex andere Endpunkte, zeigt die
// Normale dort woanders hin und das Band verdreht sich.
#[test]
fn ein_segment_sind_sechs_vertices_mit_identischen_endpunkten() {
    let (a, b) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.8, 0.0));
    let out = edge(a, b);
    assert_eq!(out.len(), 6);
    for v in &out {
        assert_eq!(v.pos_a, [a.x, a.y, a.z]);
        assert_eq!(v.pos_b, [b.x, b.y, b.z]);
    }
}

// Die sechs Vertices muessen alle VIER Ecken des Bandes abdecken (beide Seiten an beiden
// Endpunkten) -- sonst waere es kein Rechteck, sondern ein Dreieck oder eine entartete
// Flaeche. Zwei der Ecken kommen doppelt vor (die geteilte Diagonale der zwei Dreiecke).
#[test]
fn die_sechs_vertices_decken_alle_vier_bandecken_ab() {
    let out = edge(Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.8, 0.0));
    for ecke in [[-1.0, 0.0], [-1.0, 1.0], [1.0, 0.0], [1.0, 1.0]] {
        assert!(out.iter().any(|v| v.side_and_end == ecke), "Bandecke {ecke:?} fehlt");
    }
    // Nur -1/+1 als Seite und 0/1 als Endpunkt -- alles andere waere ein Tippfehler in
    // der Ecktabelle, den der Shader still als schiefes Band zeichnen wuerde.
    for v in &out {
        assert!(v.side_and_end[0] == -1.0 || v.side_and_end[0] == 1.0, "Seite {} ungültig", v.side_and_end[0]);
        assert!(v.side_and_end[1] == 0.0 || v.side_and_end[1] == 1.0, "Endpunkt {} ungültig", v.side_and_end[1]);
    }
}

// Eine entartete Kante (a == b) hat keine Richtung -- sie liefert gar keine Geometrie,
// statt ein Band mit undefinierter Normale zu erzeugen.
#[test]
fn entartete_kante_erzeugt_keine_geometrie() {
    assert!(edge(Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, 1.0)).is_empty());
}

// Die Farbe geht unveraendert an alle sechs Vertices -- ein Verlauf innerhalb einer Kante
// waere hier nie beabsichtigt.
#[test]
fn farbe_liegt_an_allen_vertices_an() {
    let mut out = Vec::new();
    push_hitbox_edge(&mut out, Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 0.0, 2.0), [0.25, 0.5, 0.75, 0.4], 1.5);
    assert!(out.iter().all(|v| v.color == [0.25, 0.5, 0.75, 0.4] && v.half_width == 1.5));
}
