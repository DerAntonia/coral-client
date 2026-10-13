// Loest Formen gegen die echte lokale 1.21.11-Installation auf. #[ignore], weil es eine
// vorhandene Installation braucht -- manuell: cargo test -p assets --ignored shapes -- --nocapture
use assets::{build_shapes, MinecraftInstallation};

#[test]
#[ignore]
fn formen_gegen_echte_installation() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");

    // State-IDs frei erfunden (nur als Schluessel, nicht als echte Protokoll-IDs -- die
    // Aufloesung braucht nur Blockname + Properties, keine echte State-ID-Semantik).
    let states: Vec<(u32, &str, &[(&str, &str)])> = vec![
        (0, "oak_slab", &[("type", "bottom"), ("waterlogged", "false")]),
        (1, "oak_slab", &[("type", "top"), ("waterlogged", "false")]),
        (2, "oak_slab", &[("type", "double"), ("waterlogged", "false")]),
        (3, "oak_fence", &[("north", "true"), ("south", "false"), ("east", "false"), ("west", "false"), ("waterlogged", "false")]),
        (4, "oak_fence", &[("north", "false"), ("south", "false"), ("east", "false"), ("west", "false"), ("waterlogged", "false")]),
        (5, "oak_stairs", &[("facing", "north"), ("half", "bottom"), ("shape", "straight"), ("waterlogged", "false")]),
        (6, "stone", &[]),
        (7, "dandelion", &[]),
    ];

    let result = build_shapes(&mut mc, &states);

    for (id, name, _) in &states {
        match result.get(id) {
            Some(boxes) => {
                println!("{name} (state {id}): {} Box(en)", boxes.len());
                for b in boxes {
                    println!("    min={:?} max={:?} faces={:?}", b.min, b.max, b.faces);
                }
            }
            None => println!("{name} (state {id}): KEINE Form (Vollwuerfel-Fallback)"),
        }
    }

    let slab_bottom = result.get(&0).expect("Stufe unten muss aufloesen");
    assert_eq!(slab_bottom.len(), 1);
    assert!((slab_bottom[0].max[1] - 0.5).abs() < 0.01, "untere Stufe muss bei y=0.5 enden, war {}", slab_bottom[0].max[1]);

    let slab_top = result.get(&1).expect("Stufe oben muss aufloesen");
    assert!((slab_top[0].min[1] - 0.5).abs() < 0.01, "obere Stufe muss bei y=0.5 beginnen, war {}", slab_top[0].min[1]);

    let slab_double = result.get(&2).expect("Doppelstufe muss aufloesen");
    assert!((slab_double[0].max[1] - 1.0).abs() < 0.01, "Doppelstufe muss volle Hoehe haben");

    let fence_one_side = result.get(&3).expect("Zaun mit einer Seite muss aufloesen");
    assert_eq!(fence_one_side.len(), 3, "Pfosten + 2 Riegel (echtes Zaun-Seitenmodell) erwartet");

    let fence_alone = result.get(&4).expect("Zaun ohne Verbindung muss aufloesen");
    assert_eq!(fence_alone.len(), 1, "nur der Pfosten erwartet");

    // Blumen: Kreuz-Modell, kein achsenparalleles Ergebnis -- Fallback erwartet.
    assert!(!result.contains_key(&7), "Loewenzahn sollte auf den Vollwuerfel-Fallback zurueckfallen");
}

// Regressionstest fuer Falllaub (keine Kollisionsbox, richtige Grafik): das Modell ist
// eine hauchduenne (from.y == to.y), Multipart-aufgeloeste Ebene, die je nach facing/
// segment_amount unterschiedlich breit ist -- KEIN Vollwuerfel-Fallback (der waere der
// eigentliche Grund fuer "immer grau": ohne echte Form fehlt jede Richtungsinformation
// fuer die spaeter draufgerechnete Bodenverfaerbung).
#[test]
#[ignore]
fn leaf_litter_ist_eine_duenne_ebene_kein_vollwuerfel() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let states: Vec<(u32, &str, &[(&str, &str)])> = vec![
        (0, "leaf_litter", &[("facing", "north"), ("segment_amount", "1")]),
        (1, "leaf_litter", &[("facing", "east"), ("segment_amount", "2")]),
    ];
    let result = build_shapes(&mut mc, &states);

    let one_segment = result.get(&0).expect("leaf_litter (1 Segment) muss eine Form aufloesen");
    assert_eq!(one_segment.len(), 1);
    assert!(
        (one_segment[0].max[1] - one_segment[0].min[1]).abs() < 1e-4,
        "leaf_litter muss eine Nullstaerke-Ebene sein (kein Volumen), war {:?}..{:?}",
        one_segment[0].min,
        one_segment[0].max
    );
    assert!(one_segment[0].max[1] < 0.1, "leaf_litter muss knapp ueber dem Boden liegen, nicht mittig/oben");

    let two_segments = result.get(&1).expect("leaf_litter (2 Segmente) muss eine Form aufloesen");
    assert!(
        (two_segments[0].max[0] - two_segments[0].min[0] - 1.0).abs() < 1e-4,
        "2 Segmente nach Osten gedreht muessen die volle Breite (frueher Tiefe) abdecken, war {:?}..{:?}",
        two_segments[0].min,
        two_segments[0].max
    );
}

// Regressionstest fuer "Holzstämme zeigen immer in einer Richtung, statt in die richtige":
// build_rotations() muss fuer axis=x/z eine echte 90-Grad-Drehung liefern (axis=y ist die
// unrotierte Vanilla-Grundausrichtung, taucht deshalb bewusst NICHT in der Tabelle auf, s.
// dortigen Kommentar).
#[test]
#[ignore]
fn log_rotation_haengt_von_der_achse_ab() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let states: Vec<(u32, &str, &[(&str, &str)])> =
        vec![(0, "oak_log", &[("axis", "y")]), (1, "oak_log", &[("axis", "x")]), (2, "oak_log", &[("axis", "z")])];
    let rotations = assets::build_rotations(&mut mc, &states);

    assert_eq!(rotations.get(&0), None, "axis=y ist die unrotierte Grundausrichtung");
    assert_eq!(rotations.get(&1), Some(&(90, 90)), "axis=x muss gedreht sein");
    assert_eq!(rotations.get(&2), Some(&(90, 0)), "axis=z muss gedreht sein");
    assert_ne!(rotations.get(&1), rotations.get(&2), "axis=x und axis=z muessen sich unterscheiden");
}

// Regressionstest: bei Falllaub wurde immer die ganze Grafik skaliert. Das Modell deklariert echte "uv"-
// Ausschnitte (z.B. [0,0,8,8] fuer 1 Segment) INNERHALB der 16x16-Textur -- ohne die
// jetzt gebaute ElementBox::uv/ShapeBox::uv-Weiterleitung wuerde der Mesher die komplette
// Textur ueber die kleine Flaeche strecken statt nur den passenden Ausschnitt zu zeigen.
#[test]
#[ignore]
fn leaf_litter_hat_einen_uv_ausschnitt_keine_gestreckte_vollflaeche() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let states: Vec<(u32, &str, &[(&str, &str)])> = vec![(0, "leaf_litter", &[("facing", "north"), ("segment_amount", "1")])];
    let result = build_shapes(&mut mc, &states);
    let boxes = result.get(&0).expect("leaf_litter muss eine Form aufloesen");
    let up_uv = boxes[0].uv[assets::FACE_UP].expect("Oben-Flaeche muss einen UV-Ausschnitt haben");
    // [0,0,8,8]/16 = [0,0,0.5,0.5] -- ein Viertel der Textur, nicht die volle Kachel.
    assert!((up_uv[2] - 0.5).abs() < 1e-4 && (up_uv[3] - 0.5).abs() < 1e-4, "erwartete ein Viertel-Ausschnitt, war {up_uv:?}");
}

// Regressionstest fuer "manchmal werden die leaf-Viertel immer noch gestretcht": bei
// facing=east/west tauscht die Y-Rotation Breite/Tiefe der Box (s. leaf_litter_2: 8 breit x
// 16 tief wird zu 16 breit x 8 tief) -- der UV-Ausschnitt MUSS im selben Verhaeltnis
// mitdrehen, sonst wird die (jetzt falsch geformte) Textur ueber die neue Box gestreckt.
#[test]
#[ignore]
fn leaf_litter_uv_dreht_sich_mit_der_box() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let states: Vec<(u32, &str, &[(&str, &str)])> = vec![
        (0, "leaf_litter", &[("facing", "north"), ("segment_amount", "2")]),
        (1, "leaf_litter", &[("facing", "east"), ("segment_amount", "2")]),
    ];
    let result = build_shapes(&mut mc, &states);

    // OHNE Betrag verglichen (verschaerft gegenueber einer frueheren Fassung): ein
    // rotate_uv_rect-Bug hat urspruenglich eine GESPIEGELTE statt gedrehte Textur geliefert
    // (falsche Ecken-Kombination, s. rotate_uv_rect_y-Kommentar) -- das aendert nur das
    // Vorzeichen des Verhaeltnisses, nicht dessen Betrag, waere also mit einer reinen
    // abs()-Pruefung unentdeckt geblieben (genau der Fehler, der beim Zaun/Mauer als "Textur
    // nicht gerade" auffiel). Jetzt exakt (samt Vorzeichen) verglichen.
    let north = &result.get(&0).expect("facing=north muss aufloesen")[0];
    let north_box_ratio = (north.max[0] - north.min[0]) / (north.max[2] - north.min[2]);
    let north_uv = north.uv[assets::FACE_UP].expect("Oben-UV muss gesetzt sein");
    let north_uv_ratio = (north_uv[2] - north_uv[0]) / (north_uv[3] - north_uv[1]);
    assert!(
        (north_box_ratio - north_uv_ratio).abs() < 1e-3,
        "unrotiert: Box- und UV-Seitenverhaeltnis muessen uebereinstimmen (inkl. Vorzeichen)"
    );

    let east = &result.get(&1).expect("facing=east muss aufloesen")[0];
    let east_box_ratio = (east.max[0] - east.min[0]) / (east.max[2] - east.min[2]);
    let east_uv = east.uv[assets::FACE_UP].expect("Oben-UV muss gesetzt sein");
    let east_uv_ratio = (east_uv[2] - east_uv[0]) / (east_uv[3] - east_uv[1]);
    assert!(
        (east_box_ratio - east_uv_ratio).abs() < 1e-3,
        "facing=east: Box-Verhaeltnis {east_box_ratio} vs. UV-Verhaeltnis {east_uv_ratio} -- UV muss mit der Box mitrotieren (inkl. Vorzeichen), sonst gespiegelt/gestreckt"
    );
}

// Deckenranke (up=true, x=270 -- s. Ranken-Rotationsbug): muss oben in der Zelle liegen
// (y~0.95), nicht am Boden (y~0.05, der urspruengliche Bug durch vertauschte X/Y-
// Rotationschiralitaet).
#[test]
#[ignore]
fn ranke_an_der_decke_liegt_oben_nicht_unten() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let states: Vec<(u32, &str, &[(&str, &str)])> =
        vec![(0, "vine", &[("up", "true"), ("north", "false"), ("south", "false"), ("east", "false"), ("west", "false")])];
    let boxes = build_shapes(&mut mc, &states).remove(&0).expect("Ranke muss eine Form haben");
    assert_eq!(boxes.len(), 1);
    assert!(boxes[0].min[1] > 0.9, "Deckenranke muss oben liegen, war y={}", boxes[0].min[1]);
}

// Mauern/Zaunpfosten deklarieren in ihren echten Modellen (template_wall_post.json,
// oak_fence_post.json) KEIN explizites "uv" auf irgendeiner Flaeche -- Vanilla generiert dort
// automatisch einen UV-Ausschnitt aus der Element-Geometrie (s. shapes.rs::default_face_uv-
// Kommentar). Ohne diese Standard-UV-Erzeugung stretcht der Mesher die volle Atlas-Kachel
// ueber den viel schmaleren Pfosten und verzerrt sie. Pfosten ist 8/16 breit (Mauer) bzw. 4/16 breit (Zaun) -- der
// UV-Ausschnitt muss GENAUSO schmal sein wie die Box, nicht die volle 0..1-Kachel.
#[test]
#[ignore]
fn mauer_und_zaunpfosten_haben_einen_schmalen_uv_ausschnitt() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let states: Vec<(u32, &str, &[(&str, &str)])> = vec![
        (
            0,
            "andesite_wall",
            &[("up", "true"), ("north", "none"), ("south", "none"), ("east", "none"), ("west", "none"), ("waterlogged", "false")],
        ),
        (1, "oak_fence", &[("north", "false"), ("south", "false"), ("east", "false"), ("west", "false"), ("waterlogged", "false")]),
    ];
    let shapes = build_shapes(&mut mc, &states);

    let wall = shapes.get(&0).expect("Mauerpfosten muss eine Form haben");
    assert_eq!(wall.len(), 1);
    let wall_uv = wall[0].uv[2].expect("Nordseite des Mauerpfostens muss einen UV-Ausschnitt haben");
    assert!((wall_uv[2] - wall_uv[0] - 0.5).abs() < 0.01, "Mauerpfosten-UV-Breite muss ~0.5 (8/16) sein, war {:?}", wall_uv);

    let fence = shapes.get(&1).expect("Zaunpfosten muss eine Form haben");
    let fence_post = &fence[0];
    let fence_uv = fence_post.uv[2].expect("Nordseite des Zaunpfostens muss einen UV-Ausschnitt haben");
    assert!((fence_uv[2] - fence_uv[0] - 0.25).abs() < 0.01, "Zaunpfosten-UV-Breite muss ~0.25 (4/16) sein, war {:?}", fence_uv);
}

// Konsistenzpruefung: fuer Elemente OHNE explizites "uv" (also ueber default_face_uv erzeugt,
// s. dortigen Kommentar) muss das Ergebnis nach Rotation GENAU dasselbe sein wie wenn man die
// Standard-UV-Formel direkt auf die BEREITS GEDREHTE Box anwenden wuerde (die Formel ist ja
// eine reine Funktion der Geometrie -- rotierte Box rein, "richtiger" Ausschnitt raus, per
// Definition). Weicht das ab, rotiert unsere rotate_uv()-Slot-Permutation die Achsen-
// Konvention nicht konsistent mit, was sich als schiefe/verschobene Textur zeigen wuerde
// (etwa schief am Zaun).
#[test]
#[ignore]
fn rotierte_standard_uv_stimmt_mit_direkter_neuberechnung_ueberein() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    // andesite_wall mit NUR "east=low" (y=90-Rotation der wall_side, kein explizites uv im
    // Modell -- s. template_wall_side.json).
    let states: Vec<(u32, &str, &[(&str, &str)])> = vec![(
        0,
        "andesite_wall",
        &[("up", "false"), ("north", "none"), ("south", "none"), ("east", "low"), ("west", "none"), ("waterlogged", "false")],
    )];
    let shapes = build_shapes(&mut mc, &states);
    let boxes = shapes.get(&0).expect("Mauerseite muss eine Form haben");
    assert_eq!(boxes.len(), 1);
    let b = &boxes[0];

    // Erwartung direkt aus der GEDREHTEN Box-Geometrie (min/max sind schon normalisiert
    // 0..1 -- zurueck auf 0..16 fuer die Formel, die in Roh-Modelleinheiten arbeitet).
    let from16 = [b.min[0] * 16.0, b.min[1] * 16.0, b.min[2] * 16.0];
    let to16 = [b.max[0] * 16.0, b.max[1] * 16.0, b.max[2] * 16.0];
    for face in 0..6 {
        if !b.faces[face] {
            continue;
        }
        let expected_raw = assets::default_face_uv(face, from16, to16);
        let expected = [expected_raw[0] / 16.0, expected_raw[1] / 16.0, expected_raw[2] / 16.0, expected_raw[3] / 16.0];
        let actual = b.uv[face].unwrap_or_else(|| panic!("Flaeche {face} hat keinen UV-Ausschnitt"));
        for k in 0..4 {
            assert!(
                (expected[k] - actual[k]).abs() < 0.01,
                "Flaeche {face}: erwartet {:?} (aus gedrehter Geometrie neu berechnet), tatsaechlich {:?}",
                expected,
                actual
            );
        }
    }
}

// Welche Modelle Vanillas Eckabdunklung abschalten, gegen die echte Installation: Tueren und
// Leitern tragen "ambientocclusion": false in ihrer Parent-Kette, Stein und Stufen nicht. Liest
// der Loader das Feld nicht (oder die Kette falsch), haetten Tueren dunkle Raender, die es in
// Vanilla nicht gibt -- oder normale Bloecke gar keine Abdunklung.
#[test]
#[ignore]
fn ambient_occlusion_abschaltung_gegen_echte_installation() {
    let mut mc = MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
    let states: Vec<(u32, &str, &[(&str, &str)])> = vec![
        (0, "stone", &[]),
        (1, "oak_slab", &[("type", "bottom"), ("waterlogged", "false")]),
        (2, "ladder", &[("facing", "north"), ("waterlogged", "false")]),
        (3, "oak_door", &[("facing", "north"), ("half", "lower"), ("hinge", "left"), ("open", "false"), ("powered", "false")]),
        (4, "glass_pane", &[("north", "true"), ("south", "true"), ("east", "false"), ("west", "false"), ("waterlogged", "false")]),
        (5, "grass_block", &[("snowy", "false")]),
    ];
    let aus = assets::build_ao_disabled(&mut mc, &states);
    println!("AO aus fuer: {:?}", aus.iter().map(|id| states[*id as usize].1).collect::<Vec<_>>());
    assert!(!aus.contains(&0), "Stein benutzt AO");
    assert!(!aus.contains(&1), "Stufen benutzen AO");
    assert!(!aus.contains(&5), "Grasblock benutzt AO");
    assert!(aus.contains(&2), "Leiter schaltet AO ab (ladder.json)");
    assert!(aus.contains(&3), "Tuer schaltet AO ab (door_*.json)");
    assert!(aus.contains(&4), "Glasscheibe schaltet AO ab (template_glass_pane_*.json)");
}
