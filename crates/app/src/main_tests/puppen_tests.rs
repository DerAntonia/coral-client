use super::*;

// Das Buch des Zaubertischs liegt im Buchfeld (GUI (14, 14) bis (52, 45)), aufgeklappt
// breiter als zugeklappt, und vollstaendig im Tiefenbereich.
#[test]
fn zauberbuch_liegt_im_buchfeld() {
    let s = 2.0;
    let rect = inventory::DollRect { x0: 100.0 + 14.0 * s, y0: 50.0 + 14.0 * s, x1: 100.0 + 52.0 * s, y1: 50.0 + 45.0 * s, pixels_per_meter: 40.0 * s };
    let model = world::entity_model::enchanting_book_model();
    let (view, proj) = book_camera((1920.0, 1080.0));
    let ausdehnung = |open: f32| {
        let verts = enchant_book_vertices(&model, [0.0, 0.0, 1.0, 1.0], rect, open, 0.3);
        assert!(!verts.is_empty());
        let (mut x0, mut x1) = (f32::MAX, f32::MIN);
        for v in &verts {
            let ndc = nach_ndc(view, proj, glam::Vec3::from(v.position));
            let (px, py) = (f32::midpoint(ndc.x, 1.0) * 1920.0, (1.0 - ndc.y) / 2.0 * 1080.0);
            assert!(px > rect.x0 - 4.0 * s && px < rect.x1 + 4.0 * s, "x {px} ausserhalb (offen {open})");
            assert!(py > rect.y0 - 4.0 * s && py < rect.y1 + 4.0 * s, "y {py} ausserhalb (offen {open})");
            assert!(ndc.z > 0.0 && ndc.z < 1.0);
            x0 = x0.min(px);
            x1 = x1.max(px);
        }
        x1 - x0
    };
    assert!(ausdehnung(1.0) > ausdehnung(0.0) + 5.0 * s);
}

// Die Zierzeilen: fest je Samen und Option, drei oder vier Woerter.
#[test]
fn zierzeilen_haengen_am_samen() {
    assert_eq!(enchant_phrase(1234, 0), enchant_phrase(1234, 0));
    assert_ne!(enchant_phrase(1234, 0), enchant_phrase(1234, 1));
    for seed in 0..50 {
        let n = enchant_phrase(seed, 2).split(' ').count();
        assert!((3..=4).contains(&n));
    }
}

fn rect() -> inventory::DollRect {
    // Wie player_screen_layout sie bei gui_scale 2 auf einem 1920x1080-Bild baut.
    inventory::DollRect { x0: 900.0, y0: 400.0, x1: 998.0, y1: 540.0, pixels_per_meter: inventory::DOLL_SIZE * 2.0 }
}

fn nach_ndc(view: glam::Mat4, proj: glam::Mat4, punkt: glam::Vec3) -> glam::Vec3 {
    let clip = proj * view * punkt.extend(1.0);
    clip.truncate() / clip.w
}

// Der Bezugspunkt der Figur (halbe Koerperhoehe plus Vanillas 0.0625) muss GENAU in der
// Mitte des Rechtecks landen -- sonst haengt sie oben oder unten aus dem Fenster.
#[test]
fn die_figur_sitzt_mittig_im_rechteck() {
    let screen = (1920.0, 1080.0);
    let r = rect();
    let (view, proj) = doll_camera(r, screen);
    let mitte = nach_ndc(view, proj, glam::Vec3::new(0.0, physics::PLAYER_HEIGHT / 2.0 + 0.0625, 0.0));
    let (cx, cy) = r.center();
    // NDC -> Bildschirmpixel zurueckrechnen.
    let px = f32::midpoint(mitte.x, 1.0) * screen.0;
    let py = (1.0 - mitte.y) / 2.0 * screen.1;
    assert!((px - cx).abs() < 1e-3, "x {px} statt {cx}");
    assert!((py - cy).abs() < 1e-3, "y {py} statt {cy}");
}

// Der Massstab: ein Meter Modellhoehe sind genau `pixels_per_meter` Bildschirmpixel, und
// zwar nach OBEN (grosseres Modell-y = kleineres Bildschirm-y).
#[test]
fn ein_meter_sind_dreissig_gui_pixel() {
    let screen = (1920.0, 1080.0);
    let r = rect();
    let (view, proj) = doll_camera(r, screen);
    let unten = nach_ndc(view, proj, glam::Vec3::ZERO);
    let oben = nach_ndc(view, proj, glam::Vec3::new(0.0, 1.0, 0.0));
    let py = |ndc: glam::Vec3| (1.0 - ndc.y) / 2.0 * screen.1;
    let differenz = py(unten) - py(oben);
    assert!((differenz - r.pixels_per_meter).abs() < 1e-3, "ein Meter waren {differenz} px statt {}", r.pixels_per_meter);
}

// KEINE Spiegelung: die Pipeline verwirft rueckwaerts gewandte Dreiecke (cull_mode Back).
// Kippte die Umlaufrichtung, saehe man die Figur von innen -- also praktisch gar nichts.
// Geprueft wird an der projizierten FLAECHE eines Dreiecks, denn nur die xy-Ebene
// entscheidet ueber vorne/hinten.
#[test]
fn die_projektion_dreht_die_umlaufrichtung_nicht() {
    let screen = (1920.0, 1080.0);
    let (view, proj) = doll_camera(rect(), screen);
    let flaeche = |a: glam::Vec3, b: glam::Vec3, c: glam::Vec3| {
        let (a, b, c) = (nach_ndc(view, proj, a), nach_ndc(view, proj, b), nach_ndc(view, proj, c));
        (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)
    };
    // Ein Dreieck gegen den Uhrzeigersinn in der xy-Ebene (Modellraum) muss auch in NDC
    // gegen den Uhrzeigersinn liegen, also positive Flaeche behalten.
    let vorher = {
        let (a, b, c) = (glam::Vec3::ZERO, glam::Vec3::X, glam::Vec3::Y);
        (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)
    };
    let nachher = flaeche(glam::Vec3::ZERO, glam::Vec3::X, glam::Vec3::Y);
    assert!(vorher > 0.0 && nachher > 0.0, "Umlaufrichtung gekippt: {vorher} -> {nachher}");
}

// Naeher an der Kamera (groesseres Modell-z, die Kamera schaut entlang -z) muss KLEINERE
// Tiefe ergeben -- sonst verdeckt der Rucken das Gesicht.
#[test]
fn naeher_heisst_kleinere_tiefe() {
    let (view, proj) = doll_camera(rect(), (1920.0, 1080.0));
    let vorn = nach_ndc(view, proj, glam::Vec3::new(0.0, 1.0, 0.5));
    let hinten = nach_ndc(view, proj, glam::Vec3::new(0.0, 1.0, -0.5));
    assert!(vorn.z < hinten.z, "vorn {} muss kleiner sein als hinten {}", vorn.z, hinten.z);
    // Und beides muss im gueltigen Tiefenbereich 0..1 liegen.
    for z in [vorn.z, hinten.z] {
        assert!((0.0..=1.0).contains(&z), "Tiefe {z} außerhalb 0..1");
    }
}

// Die Spannstufen wie in items/bow.json und items/crossbow.json (1.21.11): der Bogen schaltet
// bei use_duration*0.05 >= 0.65 bzw. >= 0.9, die Armbrust bei pull >= 0.58 bzw. >= 1.0;
// eine geladene Armbrust zeigt ihr Geschoss. Hier mit gleich gebauten Definitionen gegen die
// Eingaben, die der Client aus Stapel und Benutzung macht.
#[test]
fn kartenneigung_wie_vanilla() {
    // Geradeaus liegt die Karte ganz gekippt vor einem, ab gut 50 Grad nach unten flach.
    assert!((map_tilt(0.0) - 1.0).abs() < 1e-6);
    assert!(map_tilt(60.0).abs() < 1e-6);
    assert!((map_tilt(27.0) - 0.5).abs() < 0.05);
}

#[test]
fn tempo_faktor_bleibt_die_dezimalzahl() {
    assert_eq!(use_speed_f64(0.2), 0.2);
    assert_eq!(use_speed_f64(1.0), 1.0);
    assert_eq!(use_speed_f64(world::item_behavior::DEFAULT_USE_SPEED), physics::ITEM_USE_SPEED_MULT);
}

#[test]
fn bogen_und_armbrust_zeigen_ihre_spannstufen() {
    let stufe = |s: f32, m: &str| format!(r#"{{"threshold":{s},"model":{{"type":"model","model":"item/{m}"}}}}"#);
    let bogen = assets::ItemDefinition::parse(
        format!(
            r#"{{"model":{{"type":"condition","property":"using_item","on_false":{{"type":"model","model":"item/bow"}},
                "on_true":{{"type":"range_dispatch","property":"use_duration","scale":0.05,"entries":[{},{}],
                "fallback":{{"type":"model","model":"item/bow_pulling_0"}}}}}}}}"#,
            stufe(0.65, "bow_pulling_1"),
            stufe(0.9, "bow_pulling_2")
        )
        .as_bytes(),
    )
    .unwrap();
    let armbrust = assets::ItemDefinition::parse(
        format!(
            r#"{{"model":{{"type":"select","property":"charge_type","cases":[
                {{"when":"arrow","model":{{"type":"model","model":"item/crossbow_arrow"}}}},
                {{"when":"rocket","model":{{"type":"model","model":"item/crossbow_firework"}}}}],
                "fallback":{{"type":"condition","property":"using_item","on_false":{{"type":"model","model":"item/crossbow"}},
                "on_true":{{"type":"range_dispatch","property":"crossbow/pull","entries":[{},{}],
                "fallback":{{"type":"model","model":"item/crossbow_pulling_0"}}}}}}}}}}"#,
            stufe(0.58, "crossbow_pulling_1"),
            stufe(1.0, "crossbow_pulling_2")
        )
        .as_bytes(),
    )
    .unwrap();
    let verzauberungen = world::enchantment::Enchantments::default();
    let id = |n: &str| world::item_registry::item_id_by_name(n).unwrap();
    let modell = |def: &assets::ItemDefinition, stack: &protocol::item::ItemStack, benutzt: Option<(f32, f32)>| {
        let mut input = item_model_input::StackInput::new(stack, "firstperson_righthand", &verzauberungen, &[]);
        input.using = benutzt.map(|(ticks, pull_ticks)| item_model_input::UseState { ticks, duration: 72_000.0, pull_ticks });
        def.leaves[def.evaluate(&input)[0] as usize].model.clone()
    };
    let b = protocol::item::ItemStack::new(id("bow"), 1);
    assert_eq!(modell(&bogen, &b, None), "item/bow", "ohne Benutzung das Grundmodell");
    assert_eq!(modell(&bogen, &b, Some((0.0, 25.0))), "item/bow_pulling_0");
    assert_eq!(modell(&bogen, &b, Some((13.0, 25.0))), "item/bow_pulling_1", "0.65 = 13 Ticks");
    assert_eq!(modell(&bogen, &b, Some((12.9, 25.0))), "item/bow_pulling_0");
    // Vanilla: 0.9 = 18 Ticks. Hier erst bei voller Kraft (20 Ticks), s. item_model_input.
    assert_eq!(modell(&bogen, &b, Some((18.0, 25.0))), "item/bow_pulling_1", "18 Ticks: noch nicht voll gespannt");
    assert_eq!(modell(&bogen, &b, Some((19.9, 25.0))), "item/bow_pulling_1");
    assert_eq!(modell(&bogen, &b, Some((20.0, 25.0))), "item/bow_pulling_2", "20 Ticks = volle Kraft");

    let mut a = protocol::item::ItemStack::new(id("crossbow"), 1);
    assert_eq!(modell(&armbrust, &a, None), "item/crossbow");
    assert_eq!(modell(&armbrust, &a, Some((0.0, 25.0))), "item/crossbow_pulling_0");
    assert_eq!(modell(&armbrust, &a, Some((14.5, 25.0))), "item/crossbow_pulling_1", "0.58 von 25 Ticks (unverzaubert)");
    assert_eq!(modell(&armbrust, &a, Some((25.0, 25.0))), "item/crossbow_pulling_2");
    // Schnellladen III: 10 Ticks Ladezeit -- nach 10 Ticks voll gespannt, nicht erst nach 25.
    assert_eq!(modell(&armbrust, &a, Some((10.0, 10.0))), "item/crossbow_pulling_2");
    assert_eq!(modell(&armbrust, &a, Some((6.0, 10.0))), "item/crossbow_pulling_1");
    // Ladezeit 0 (Schnellladen V): sofort voll, keine Division durch null.
    assert_eq!(modell(&armbrust, &a, Some((0.0, 0.0))), "item/crossbow_pulling_2");
    // Geladen schlaegt jede Spannstufe; eine Rakete unter den Geschossen zeigt die Rakete.
    a.charged_projectiles = vec![id("arrow")];
    assert_eq!(modell(&armbrust, &a, Some((0.0, 25.0))), "item/crossbow_arrow");
    a.charged_projectiles = vec![id("arrow"), id("firework_rocket")];
    assert_eq!(modell(&armbrust, &a, None), "item/crossbow_firework");
}

// Wo das Schild landet. Gerechnet gegen das ECHTE Spielermodell, weil genau hier ein
// halber Block Versatz unsichtbar bleibt, solange man nur die Kette anschaut -- und im
// Spiel als "das Schild fliegt in der Luft" auffaellt.
//
// Die Zahlen sind keine Wunschwerte: sie folgen aus Vanillas Kette (s. App::emit_shield).
// Geprueft wird die EIGENSCHAFT, die den Fehler ausmacht -- das Schild muss den Arm
// umschliessen, nicht neben ihm haengen.
#[test]
fn das_schild_haengt_am_arm_und_nicht_daneben() {
    use world::entity_model::{part_transform, player_model, shield_model, Affine, Pose};

    let model = player_model(false);
    let pose = Pose::new();
    let base = world::entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, world::entity_model::PLAYER_MODEL_SCALE);
    let arm = part_transform(&model, &pose, base, "right_arm").expect("rechter Arm");
    let hand = arm.apply([0.0, 0.0, 0.0]);

    // Dieselbe Kette wie emit_shield, mit den Werten aus shield.json (rechte Hand).
    let pre = held_item::hand_pre_transform(arm, true);
    let schild = pre
        .then(Affine::translation([10.0 / 16.0, 6.0 / 16.0, -4.0 / 16.0]))
        .then(Affine::rotation_xyz_deg(0.0, 90.0, 0.0))
        .then(Affine::translation([-0.5, -0.5, -0.5]))
        .then(Affine::scale([1.0, -1.0, -1.0]));
    let mut out = Vec::new();
    world::emit_entity_model(&shield_model(), &pose, schild, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
    let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in &out {
        for k in 0..3 {
            mn[k] = mn[k].min(v.position[k]);
            mx[k] = mx[k].max(v.position[k]);
        }
    }

    // Die Schulter liegt bei x = -0.293. Das Schild muss sie waagerecht einschliessen --
    // OHNE den -0.5-Versatz aus Transformation.apply lag es komplett links davon
    // (-1.05 bis -0.64), also gut einen halben Block neben dem Arm in der Luft.
    assert!(mn[0] < hand[0] && mx[0] > hand[0], "das Schild ({}..{}) muss den Arm bei x={} umschliessen", mn[0], mx[0], hand[0]);
    // Und senkrecht zwischen Schulter und Hand bleiben, nicht darueber hinausragen.
    assert!(mx[1] <= hand[1] + 1e-3, "das Schild ragt über die Schulter ({} > {})", mx[1], hand[1]);
    assert!(mn[1] > hand[1] - 0.8, "das Schild hängt unter der Hand ({})", mn[1]);
}

// Nur das Spielerinventar hat eine Figur -- eine Kiste nicht.
#[test]
fn nur_das_spielerinventar_hat_eine_figur() {
    let screen = (1920.0, 1080.0);
    let mut inv = inventory::Inventory::default();
    let spieler = inventory::screen_layout(&inv, screen, 2.0);
    assert!(spieler.doll.is_some(), "das Spielerinventar muss eine Figur zeigen");
    inv.open = Some(inventory::OpenContainer { sync_id: 1, screen_type: -1, title: String::new(), slots: vec![None; 27], revision: 0, data: Vec::new(), merchant: Default::default() });
    let kiste = inventory::screen_layout(&inv, screen, 2.0);
    assert!(kiste.doll.is_none(), "eine Kiste zeigt keine Figur");
}
