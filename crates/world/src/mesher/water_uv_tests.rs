use super::*;

const STILL: [f32; 4] = [0.0, 0.0, 0.25, 0.25];
const FLOW: [f32; 4] = [0.5, 0.5, 0.75, 0.75];

fn in_kachel(uv: [f32; 2], rect: [f32; 4]) -> bool {
    uv[0] >= rect[0] - 1e-5 && uv[0] <= rect[2] + 1e-5 && uv[1] >= rect[1] - 1e-5 && uv[1] <= rect[3] + 1e-5
}

// Ohne Stroemung: Standtextur, volle Kachel, Reihenfolge wie Vanillas Still-Zweig.
#[test]
fn ruhiges_wasser_nutzt_die_standkachel() {
    let uv = water_top_corner_uvs(Some([0.0, 0.0, 0.0]), STILL, Some(FLOW));
    assert_eq!(uv, [[0.0, 0.0], [0.0, 0.25], [0.25, 0.25], [0.25, 0.0]]);
}

// Fehlt water_flow im Atlas (Resourcepack ohne die Datei), muss sauber die Standtextur
// herauskommen -- nicht etwa ein Griff in eine fremde Atlas-Kachel.
#[test]
fn ohne_fliesskachel_bleibt_es_bei_der_standtextur() {
    let uv = water_top_corner_uvs(Some([1.0, 0.0, 0.0]), STILL, None);
    assert!(uv.iter().all(|&p| in_kachel(p, STILL)), "muss in der Standkachel bleiben, war {uv:?}");
}

// Bei Stroemung wird die FLIESSKACHEL benutzt, und alle vier Ecken muessen INNERHALB von
// ihr liegen -- Vanillas +-0.25-Ausschnitt sorgt genau dafuer. Liefen sie heraus, zoege
// das benachbarte Atlas-Kacheln ins Wasser (sichtbar als fremde Textur-Fetzen).
#[test]
fn stroemung_bleibt_innerhalb_der_fliesskachel() {
    // Rundum durch alle Richtungen, nicht nur eine -- der Ausschnitt muss bei JEDEM
    // Winkel passen.
    for schritt in 0..16 {
        let winkel = f64::from(schritt) * std::f64::consts::TAU / 16.0;
        let uv = water_top_corner_uvs(Some([winkel.cos(), 0.0, winkel.sin()]), STILL, Some(FLOW));
        for p in uv {
            assert!(in_kachel(p, FLOW), "Winkel {winkel}: UV {p:?} liegt ausserhalb der Fliesskachel {FLOW:?}");
        }
    }
}

// Entgegengesetzte Stroemungen muessen unterschiedliche UVs ergeben -- sonst waere die
// Drehung wirkungslos und Wasser sae flussauf wie flussab gleich aus.
#[test]
fn gegenrichtungen_ergeben_verschiedene_uvs() {
    let nach_osten = water_top_corner_uvs(Some([1.0, 0.0, 0.0]), STILL, Some(FLOW));
    let nach_westen = water_top_corner_uvs(Some([-1.0, 0.0, 0.0]), STILL, Some(FLOW));
    assert_ne!(nach_osten, nach_westen, "die Drehung muss die Richtung wirklich abbilden");
}

// Eine Vierteldrehung der Stroemung muss die UVs zyklisch weiterschieben: die Ecken
// rotieren gegeneinander, statt sich neu zu erfinden. Damit steht fest, dass wirklich
// GEDREHT und nicht bloss verschoben wird.
#[test]
fn vierteldrehung_rotiert_die_ecken() {
    let a = water_top_corner_uvs(Some([1.0, 0.0, 0.0]), STILL, Some(FLOW));
    let b = water_top_corner_uvs(Some([0.0, 0.0, 1.0]), STILL, Some(FLOW));
    let gleich = |p: [f32; 2], q: [f32; 2]| (p[0] - q[0]).abs() < 1e-5 && (p[1] - q[1]).abs() < 1e-5;
    // Jede Ecke von b muss einer Ecke von a entsprechen (dieselbe Punktmenge, andere
    // Zuordnung) -- genau das Kennzeichen einer Drehung um die Kachelmitte.
    for q in b {
        assert!(a.iter().any(|&p| gleich(p, q)), "Ecke {q:?} kommt in der ungedrehten Menge {a:?} nicht vor");
    }
}

// Die SEITENFLAECHEN tragen die nach unten laufende Fliessgrafik -- Vanilla nimmt dort
// immer sprite2 (water_flow), nie die Standtextur. Genau das hat gefehlt: "die yx- und
// yz-Planes, also die 'Wasser fliesst nach unten'-Textur, wurden nicht angezeigt".
//
// Geprueft wird die exakte Vanilla-Formel: waagerecht getFrameU(0.0)..getFrameU(0.5) --
// also die LINKE HAELFTE der Kachel --, senkrecht getFrameV((1-h)*0.5)..getFrameV(0.5).
#[test]
fn volle_seitenflaeche_nimmt_die_obere_linke_viertelkachel() {
    let uv = water_side_corner_uvs(FLOW, 1.0, 1.0);
    // Kachel 0.5..0.75: halbe Breite = 0.5..0.625, obere Haelfte = 0.5..0.625.
    assert_eq!(uv, [[0.5, 0.625], [0.5, 0.5], [0.625, 0.5], [0.625, 0.625]]);
}

// Eine halbhohe Wasserflaeche darf die Textur NICHT stauchen, sondern muss sie
// beschneiden: die Unterkante bleibt bei 0.5, nur die Oberkante rutscht nach unten.
// Waere es eine Stauchung, liefe die Fliessgrafik an einer Stufe schneller als daneben.
#[test]
fn halbe_hoehe_beschneidet_statt_zu_stauchen() {
    let voll = water_side_corner_uvs(FLOW, 1.0, 1.0);
    let halb = water_side_corner_uvs(FLOW, 0.5, 0.5);
    assert_eq!(halb[0], voll[0], "die Unterkante liegt bei jeder Hoehe auf derselben V-Koordinate");
    assert_eq!(halb[3], voll[3]);
    let hoehe = |q: [[f32; 2]; 4]| q[0][1] - q[1][1];
    assert!(
        (hoehe(halb) - hoehe(voll) / 2.0).abs() < 1e-6,
        "halbe Wasserhoehe muss genau den halben V-Ausschnitt nehmen: {halb:?} vs {voll:?}"
    );
}

// Ungleiche Eckhoehen (fliessendes Wasser hat vier verschiedene) muessen sich in
// UNTERSCHIEDLICHEN V-Koordinaten der beiden oberen Ecken niederschlagen -- sonst laege
// die Textur schief zur schraegen Oberkante.
#[test]
fn schraege_oberkante_ergibt_verschiedene_oberkanten_uvs() {
    let uv = water_side_corner_uvs(FLOW, 1.0, 0.5);
    assert_ne!(uv[1][1], uv[2][1], "die beiden oberen Ecken muessen verschiedene V-Werte haben");
}
