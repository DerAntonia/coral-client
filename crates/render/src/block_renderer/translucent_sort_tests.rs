use super::*;

// Ein achsenparalleles Quad (6 Vertices wie push_quad_with_uv) auf Hoehe z.
fn quad_bei(z: f32) -> Vec<BlockVertex> {
    let v = |x: f32, y: f32| BlockVertex { position: [x, y, z], color: [1.0; 3], uv: [0.0; 2] };
    vec![v(0.0, 0.0), v(0.0, 1.0), v(1.0, 1.0), v(0.0, 0.0), v(1.0, 1.0), v(1.0, 0.0)]
}

// Ruft die ECHTE Sortierung auf (sortiere_quads, dieselbe Funktion, die sort_translucent
// benutzt) -- nur ohne Buffer-Upload. Frueher stand hier eine nachgebaute Kopie der
// Rechnung; die haette jede kuenftige Aenderung an der echten Sortierung stillschweigend
// mitbestanden, ohne sie je zu pruefen.
fn sortiere(verts: &[BlockVertex], camera: Vec3) -> Vec<BlockVertex> {
    let mut v = verts.to_vec();
    sortiere_quads(&mut v, camera, &mut Vec::new(), &mut Vec::new());
    v
}

// Der Fehler in Reinform (nach Norden richtig, nach Sueden falsch): dieselbe
// Geometrie muss aus entgegengesetzten Blickrichtungen in UMGEKEHRTER Reihenfolge
// herauskommen. Eine feste Rasterreihenfolge kann das nicht leisten.
#[test]
fn gegenrichtungen_ergeben_umgekehrte_reihenfolge() {
    let mut verts = quad_bei(0.0);
    verts.extend(quad_bei(10.0));

    let von_sueden = sortiere(&verts, Vec3::new(0.5, 0.5, -20.0));
    let von_norden = sortiere(&verts, Vec3::new(0.5, 0.5, 30.0));

    // Von Sueden (z=-20) ist das ferne Quad z=10 -> zuerst.
    assert_eq!(von_sueden[0].position[2], 10.0, "von Sueden muss das ferne Quad zuerst kommen");
    // Von Norden (z=30) genau andersherum.
    assert_eq!(von_norden[0].position[2], 0.0, "von Norden muss das andere Quad zuerst kommen");
}

// Immer von HINTEN nach VORN -- das ist die Bedingung, unter der Alpha-Blending ohne
// Tiefenschreiben ueberhaupt richtig herauskommt.
#[test]
fn sortiert_von_hinten_nach_vorn() {
    let mut verts = quad_bei(5.0);
    verts.extend(quad_bei(1.0));
    verts.extend(quad_bei(9.0));
    let camera = Vec3::new(0.5, 0.5, 0.0);

    let sortiert = sortiere(&verts, camera);
    let z: Vec<f32> = sortiert.chunks_exact(VERTS_PER_QUAD).map(|q| q[0].position[2]).collect();
    assert_eq!(z, vec![9.0, 5.0, 1.0], "Abstand muss abnehmen, nicht zunehmen");
}

// Quads duerfen beim Umsortieren nicht zerrissen werden -- ein halbes Quad ergaebe ein
// Loch. Deshalb arbeitet die Sortierung auf 6er-Bloecken, nicht auf Dreiecken.
#[test]
fn quads_bleiben_unzerrissen() {
    let mut verts = quad_bei(0.0);
    verts.extend(quad_bei(10.0));
    let sortiert = sortiere(&verts, Vec3::new(0.5, 0.5, -20.0));

    assert_eq!(sortiert.len(), verts.len(), "es darf kein Vertex verlorengehen");
    for quad in sortiert.chunks_exact(VERTS_PER_QUAD) {
        let z = quad[0].position[2];
        assert!(quad.iter().all(|v| v.position[2] == z), "ein Quad wurde ueber zwei Positionen zerrissen");
    }
}

// Gleich weit entfernte Quads muessen IMMER dieselbe Reihenfolge bekommen. sortiere_quads
// benutzt sort_unstable (schneller, aber ohne Stabilitaetsgarantie) und faengt das mit dem
// Quad-Index als zweitem Sortierkriterium ab -- ohne den koennten zwei deckungsgleiche
// Flaechen von Frame zu Frame die Plaetze tauschen und sichtbar flimmern.
#[test]
fn gleiche_abstaende_behalten_eine_feste_reihenfolge() {
    let mut verts = Vec::new();
    for _ in 0..8 {
        verts.extend(quad_bei(3.0));
    }
    let camera = Vec3::new(0.5, 0.5, 0.0);
    // Zweimal sortieren -- das zweite Mal auf dem schon sortierten Ergebnis, wie es beim
    // Weitergehen tatsaechlich passiert.
    let einmal = sortiere(&verts, camera);
    let zweimal = sortiere(&einmal, camera);
    let positionen = |v: &[BlockVertex]| v.iter().map(|v| v.position).collect::<Vec<_>>();
    assert_eq!(positionen(&einmal), positionen(&zweimal), "gleiche Abstaende duerfen nicht von Durchgang zu Durchgang umspringen");
}

// Die Arbeitspuffer werden ueber Aufrufe hinweg wiederverwendet (das ist der Sinn der
// Auslagerung). Ein zweiter Aufruf mit denselben, bereits gefuellten Puffern muss deshalb
// dasselbe liefern wie ein erster mit frischen -- sonst schleppt er Reste mit.
#[test]
fn wiederverwendete_arbeitspuffer_liefern_dasselbe() {
    let mut verts = quad_bei(1.0);
    verts.extend(quad_bei(7.0));
    verts.extend(quad_bei(4.0));
    let camera = Vec3::new(0.5, 0.5, 0.0);

    let (mut keys, mut scratch) = (Vec::new(), Vec::new());
    let mut a = verts.clone();
    sortiere_quads(&mut a, camera, &mut keys, &mut scratch);
    // Zweiter Durchgang mit denselben Puffern, aber anderer Kamera und anderer Laenge.
    let mut b = quad_bei(2.0);
    b.extend(quad_bei(9.0));
    sortiere_quads(&mut b, Vec3::new(0.5, 0.5, 20.0), &mut keys, &mut scratch);

    let positionen = |v: &[BlockVertex]| v.iter().map(|v| v.position).collect::<Vec<_>>();
    assert_eq!(positionen(&a), positionen(&sortiere(&verts, camera)));
    assert_eq!(b.len(), 2 * VERTS_PER_QUAD, "kein Rest aus dem vorherigen Durchgang");
    assert_eq!(b[0].position[2], 2.0, "von z=20 aus ist z=2 das fernere Quad");
}
