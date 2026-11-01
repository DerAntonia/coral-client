use super::*;

// Der Kern der ganzen Umstellung: an einer echten DonutSMP-Position muss ein Blockeckpunkt
// noch exakt getroffen werden. Frueher standen hier absolute Weltkoordinaten in f32 --
// bei x = -186634 ist deren Aufloesung 0.015625, jeder Eckpunkt rastete also schon beim
// Meshen auf ein 1/64-Gitter (sichtbar als zitternde Geometrie).
#[test]
fn spalten_lokale_position_bleibt_bei_riesigen_koordinaten_exakt() {
    // Spalte, in der x = -186634 liegt: floor(-186634 / 16) = -11665.
    let origin = ColumnOrigin::of_column(-11665, 9624);
    assert_eq!(origin.x, -186640);
    assert_eq!(origin.z, 153984);

    // Ein Block irgendwo in dieser Spalte, plus ein feiner Modell-Versatz (z.B. die
    // Kante einer Treppenstufe bei 1/16 Block).
    let (lx, ly, lz) = origin.local(-186634, 75, 153997);
    assert_eq!((lx, ly, lz), (6.0, 75.0, 13.0), "lokale Koordinaten muessen klein und exakt sein");

    // Die f32-Aufloesung bei x = -186634 betraegt 0.015625. Alles Feinere geht dort
    // verloren -- zum Beispiel das EPS von 0.001, mit dem push_fluid_quads die
    // Wasseroberflaeche gegen die Blockkante absetzt (s. dortige Konstante).
    const EPS: f32 = 0.001;
    let lokal_mit_eps = lx + EPS;
    assert_ne!(lokal_mit_eps, lx, "lokal muss das EPS erhalten bleiben");
    assert!((lokal_mit_eps - 6.001).abs() < 1e-6, "lokal exakt: {lokal_mit_eps}");

    // Derselbe Punkt in absoluten f32-Weltkoordinaten: das EPS verschwindet spurlos --
    // genau das war der Bug (Wasserflaechen z-fighteten mit ihrer eigenen Blockkante).
    let absolut = -186634.0f32;
    assert_eq!(absolut + EPS, absolut, "absolut in f32 faellt das EPS unter den Tisch");

    // Und die Aufloesung selbst, als Beleg fuer die Groessenordnung.
    let naechster = f32::from_bits(absolut.abs().to_bits() + 1) - absolut.abs();
    assert!((naechster - 0.015625).abs() < 1e-9, "f32-Aufloesung dort: {naechster}");
}

// Der Versatz, den der Renderer je Spalte hochlaedt, wird in f64 gerechnet und erst danach
// auf f32 verkuerzt (s. BlockRenderer::update_column_offsets). Hier nachgestellt: das
// Ergebnis muss die Spalte auf den Block genau platzieren.
#[test]
fn augenrelativer_versatz_bleibt_genau() {
    let origin = ColumnOrigin::of_column(-11665, 9624);
    let auge = (-186634.2972f64, 75.8339, 153997.0023);

    let versatz_x = (f64::from(origin.x) - auge.0) as f32;
    let versatz_z = (f64::from(origin.z) - auge.2) as f32;

    // Lokale Position des Blocks + Versatz = Blockposition relativ zum Auge.
    let (lx, _, lz) = origin.local(-186634, 75, 153997);
    let rel_x = lx + versatz_x;
    let rel_z = lz + versatz_z;

    // Erwartung direkt in f64 gerechnet.
    let soll_x = (-186634.0f64 - auge.0) as f32;
    let soll_z = (153997.0f64 - auge.2) as f32;

    assert!((rel_x - soll_x).abs() < 1e-4, "x: {rel_x} statt {soll_x}");
    assert!((rel_z - soll_z).abs() < 1e-4, "z: {rel_z} statt {soll_z}");
}
