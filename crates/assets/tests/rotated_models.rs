// Bloecke mit gedrehten Modell-Elementen (Feuer, Laterne) als echte Vierecke statt Vollwuerfel.
// #[ignore], weil es die lokale Installation braucht -- manuell:
// cargo test -p assets --test rotated_models -- --ignored --nocapture
use assets::MinecraftInstallation;

const FEUER_BODEN: [(&str, &str); 6] = [("age", "0"), ("east", "false"), ("north", "false"), ("south", "false"), ("up", "false"), ("west", "false")];

#[test]
#[ignore]
fn bodenfeuer_sind_gekippte_ebenen_und_vier_seiten() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let quads = assets::build_model_quads(&mut mc, &[(0, "fire", &FEUER_BODEN)]);
    let quads = quads.get(&0).expect("Feuer muss Modell-Vierecke bekommen");
    // fire_floor0: vier gekippte Ebenen; dazu je Himmelsrichtung fire_side0 mit Vorder- und
    // Rueckseite (Boden-Feuer ohne brennbaren Nachbarn bekommt alle vier Seiten).
    assert_eq!(quads.len(), 4 + 4 * 2);
    let hoehe = quads.iter().flat_map(|q| q.corners.iter().map(|c| c[1])).fold(0.0f32, f32::max);
    assert!(hoehe > 1.3 && hoehe < 1.5, "Feuer ragt 22,4/16 = 1,4 Bloecke hoch, war {hoehe}");
    assert!(quads.iter().all(|q| q.texture.starts_with("block/fire_")));
    assert!(quads.iter().all(|q| !q.shade), "Feuer hat \"shade\": false");
    // Die Seiten stehen an allen vier Zellraendern (y-Drehung 0/90/180/270).
    let seiten_nahe = |achse: usize, wert: f32| quads.iter().any(|q| q.corners.iter().all(|c| (c[achse] - wert).abs() < 0.02));
    assert!(seiten_nahe(2, 0.0) && seiten_nahe(2, 1.0) && seiten_nahe(0, 0.0) && seiten_nahe(0, 1.0));

    let atlas = assets::BlockAtlas::build(&mut mc, &["fire"]).expect("Atlas");
    assert!(atlas.texture_uvs.contains_key("block/fire_0") && atlas.texture_uvs.contains_key("block/fire_1"));
}

#[test]
#[ignore]
fn laterne_bekommt_vierecke() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let props: [(&str, &str); 2] = [("hanging", "false"), ("waterlogged", "false")];
    let quads = assets::build_model_quads(&mut mc, &[(0, "lantern", &props)]);
    let quads = quads.get(&0).expect("Laterne muss Modell-Vierecke bekommen");
    let breite = quads.iter().flat_map(|q| q.corners.iter().map(|c| c[0])).fold(0.0f32, f32::max);
    assert!(breite < 0.8, "Laterne ist schmaler als ein Block, war bis {breite}");
}

#[test]
#[ignore]
// Eine Ecke aus Glasscheibe (Nord + Ost verbunden): Pfosten, zwei Arme und die beiden
// "noside"-Flaechen nach Sued und West -- jede Flaeche mit IHRER Textur (Scheibe oder Kante).
fn glasscheibe_ecke_traegt_je_flaeche_ihre_textur() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let props: [(&str, &str); 5] = [("east", "true"), ("north", "true"), ("south", "false"), ("waterlogged", "false"), ("west", "false")];
    let quads = assets::build_model_quads(&mut mc, &[(0, "glass_pane", &props)]);
    let quads = quads.get(&0).expect("Glasscheibe muss Modell-Vierecke bekommen");
    let scheibe = quads.iter().filter(|q| q.texture == "block/glass").count();
    let kante = quads.iter().filter(|q| q.texture == "block/glass_pane_top").count();
    assert_eq!(scheibe + kante, quads.len(), "nur Scheibe und Kante: {:?}", quads.iter().map(|q| &q.texture).collect::<Vec<_>>());
    // Je Arm zwei Scheibenseiten, dazu die beiden noside-Flaechen.
    assert_eq!(scheibe, 2 * 2 + 2);
    assert!(kante >= 4, "Kanten: Stirnseiten der Arme und die Oberseiten");
}
