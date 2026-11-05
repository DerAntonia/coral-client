use super::*;

#[test]
fn umkehrung_hebt_auf() {
    let a = Affine::translation([0.3, -1.2, 2.0])
        .then(Affine::rotation_xyz_deg(20.0, -35.0, 70.0))
        .then(Affine { m: [[0.4, 0.0, 0.0], [0.0, 0.6, 0.0], [0.0, 0.0, 1.5]], t: [0.1, 0.0, -0.2] });
    for p in [[0.0, 0.0, 0.0], [1.0, -2.0, 0.5], [-0.3, 0.7, 4.0]] {
        let q = a.inverse().then(a).apply(p);
        assert!((0..3).all(|i| (q[i] - p[i]).abs() < 1e-4), "{p:?} -> {q:?}");
    }
}

// Ruestung am Armorstand: Kopf einen Pixel tiefer, Beine ab 11; beim kleinen Stand wie
// dessen Koerper umgeformt (Kopf 12.75 und 3/4, Rest halb und 12 tiefer).
#[test]
fn armorstand_ruestung_sitzt_wie_der_stand() {
    use crate::armor::ArmorSlot;
    let teil = |small: bool, slot: ArmorSlot, name: &str| {
        armor_stand_armor_models(small)[slot.index()].parts.iter().find(|p| p.name == name).cloned().expect("Teil")
    };
    assert_eq!(teil(false, ArmorSlot::Head, "head").pivot, [0.0, 1.0, 0.0]);
    assert_eq!(teil(false, ArmorSlot::Feet, "right_leg").pivot, [-1.9, 11.0, 0.0]);
    let kopf = teil(true, ArmorSlot::Head, "head");
    assert_eq!((kopf.pivot, kopf.scale), ([0.0, 12.75, 0.0], [0.75; 3]));
    let bein = teil(true, ArmorSlot::Legs, "left_leg");
    assert_eq!((bein.pivot, bein.scale), ([0.95, 17.5, 0.0], [0.5; 3]));
}

fn gleiten(ticks: f32, pitch_deg: f32, velocity: [f32; 3]) -> Glide {
    Glide { ticks, pitch_deg, yaw_deg: 0.0, velocity }
}

// Voll in der Flughaltung, geradeaus geschaut, ohne Seitendrift: dieselbe Drehung um
// -90 - pitch wie beim Schwimmen im Wasser -- die Figur muss exakt gleich liegen.
#[test]
fn gleitflug_liegt_wie_schwimmen() {
    let glide = gleiten(20.0, 0.0, [0.0, -0.1, 0.0]);
    assert_eq!(glide.progress(), 1.0);
    let lean = Lean { amount: 1.0, pitch_deg: 0.0, in_water: true, swimming: false };
    let a = entity_transform_gliding([0.0; 3], 0.0, 1.0, PLAYER_MODEL_SCALE, &glide);
    let b = entity_transform_leaning([0.0; 3], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean);
    for p in [[0.0, -24.0 / 16.0, 0.0], [0.3, 0.0, 0.2]] {
        let (pa, pb) = (a.apply(p), b.apply(p));
        assert!((0..3).all(|i| (pa[i] - pb[i]).abs() < 1e-5), "{pa:?} != {pb:?}");
    }
}

#[test]
fn gleitflug_setzt_langsam_ein_und_senkt_den_kopf() {
    assert!(gleiten(2.0, 0.0, [0.0; 3]).progress() < 0.1, "am Anfang noch fast aufrecht");
    assert!(!gleiten(4.0, 0.0, [0.0; 3]).head_down());
    assert!(gleiten(5.0, 0.0, [0.0; 3]).head_down());
    let pose = biped_pose_full(0.0, 30.0, 0.0, 0.0, BipedExtras { glide: Some(gleiten(10.0, 30.0, [0.0; 3])), ..Default::default() });
    assert_eq!(pose.get("head").rotation[0], -std::f32::consts::FRAC_PI_4);
}

#[test]
fn schneller_flug_daempft_die_gliedmassen() {
    assert_eq!(gleiten(10.0, 0.0, [0.1, 0.0, 0.0]).limb_amplitude_inverse(), 1.0, "langsam: ungedaempft");
    assert!(gleiten(10.0, 0.0, [1.5, 0.0, 0.0]).limb_amplitude_inverse() > 100.0);
}

// Blick nach Sueden (+Z), Flug nach Osten (+X): eine Vierteldrehung. Senkrechter Flug hat
// keine waagerechte Richtung -- dann gar keine Drehung.
#[test]
fn flugrichtung_dreht_die_figur() {
    let quer = gleiten(10.0, 0.0, [1.0, 0.0, 0.0]).flying_rotation_rad().unwrap();
    assert!((quer.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "{quer}");
    assert!(gleiten(10.0, 0.0, [0.0, 0.0, 1.0]).flying_rotation_rad().unwrap().abs() < 1e-3, "geradeaus");
    assert_eq!(gleiten(10.0, 0.0, [0.0, -1.0, 0.0]).flying_rotation_rad(), None);
}

#[test]
fn elytra_breitet_sich_im_gleitflug_aus() {
    let angelegt = elytra_pose(None, false).get("left_wing").rotation;
    let waagerecht = elytra_pose(Some(&gleiten(10.0, 0.0, [1.0, 0.0, 0.0])), false).get("left_wing").rotation;
    let sturz = elytra_pose(Some(&gleiten(10.0, 90.0, [0.0, -2.0, 0.0])), false).get("left_wing").rotation;
    assert!((waagerecht[2] + std::f32::consts::FRAC_PI_2).abs() < 1e-5, "waagerecht: ganz ausgebreitet");
    assert!((sturz[2] - angelegt[2]).abs() < 1e-5, "senkrechter Sturz: wieder angelegt");
    assert_eq!(elytra_pose(None, false).get("right_wing").rotation[2], -angelegt[2], "rechts gespiegelt");
}

// Der Kopf des Spielermodells ist ein 8x8x8-Kasten mit Pivot (0,0,0) und Offset
// (-4,-8,-4). Nach der Einbettung (s. entity_transform) muss er GENAU oben am Koerper
// sitzen. Mit PLAYER_MODEL_SCALE sind das Unterkante 0.9375 * 1.501 = 1.4072 und Oberkante
// 1.4072 + 0.9375 * 8/16 = 1.8759 Bloecke.
// Y ist im Modellraum nach unten, das scale(-1,-1,1) dreht es -- genau deshalb wird aus
// dem Kopf-Offset -8 eine Oberkante ueber dem Pivot und nicht darunter.
#[test]
fn kopf_sitzt_nach_der_einbettung_oben() {
    let model = crate::entity_model::player_model(false);
    let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
    let mut out = Vec::new();
    let head = model.parts.iter().find(|p| p.name == "head").expect("head fehlt");
    emit_part(
        &Part::new("head", head.pivot, head.cuboids.clone()),
        &Pose::new(),
        base,
        model.tex_size,
        [0.0, 0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        false,
        &mut out,
    );

    let min_y = out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
    let max_y = out.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
    assert!((min_y - 1.4072).abs() < 1e-3, "Kopf-Unterkante {min_y}, erwartet 1.4072");
    assert!((max_y - 1.8759).abs() < 1e-3, "Kopf-Oberkante {max_y}, erwartet 1.8759");
}

// DER Test fuer die Blickhoehe: die Augenhoehe des Spielers (1.62, s.
// app::physics::EYE_HEIGHT) muss auf der AUGENREIHE der Skin-Textur landen.
//
// Der Kopf ist 8 Modellpixel hoch, die Augen sitzen in der Standard-Textur rund 4.4 Pixel
// unter dem Scheitel. Mit PLAYER_MODEL_SCALE liegt der Scheitel bei 1.876, also 0.256
// Bloecke ueber der Augenhoehe -- das sind genau jene 4.4 Kopfpixel. OHNE den Faktor
// stuende der Scheitel bei 2.001, der Abstand waere 0.381 Bloecke bzw. 6.1 Kopfpixel, und
// man saehe einem gleich hohen Spieler bei Blickneigung 0 auf die Nase statt in die Augen.
#[test]
fn augenhoehe_trifft_die_augenreihe_des_skins() {
    const AUGENHOEHE: f32 = 1.62; // app::physics::EYE_HEIGHT, hier nicht erreichbar
    let model = crate::entity_model::player_model(false);
    let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
    let mut out = Vec::new();
    let head = model.parts.iter().find(|p| p.name == "head").expect("head fehlt");
    emit_part(
        &Part::new("head", head.pivot, head.cuboids.clone()),
        &Pose::new(),
        base,
        model.tex_size,
        [0.0, 0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        false,
        &mut out,
    );

    let unterkante = out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
    let oberkante = out.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
    assert!(
        AUGENHOEHE > unterkante && AUGENHOEHE < oberkante,
        "die Augenhoehe {AUGENHOEHE} muss ueberhaupt im Kopf liegen ({unterkante}..{oberkante})"
    );
    // In Kopfpixeln unter dem Scheitel gerechnet -- so ist die Zahl direkt mit der
    // Skin-Textur vergleichbar (Kopf = 8 Pixel).
    let pixel_unter_scheitel = (oberkante - AUGENHOEHE) / (oberkante - unterkante) * 8.0;
    assert!(
        (pixel_unter_scheitel - 4.4).abs() < 0.5,
        "Augen sollten ~4.4 Kopfpixel unter dem Scheitel liegen, sind {pixel_unter_scheitel}"
    );
}

// Die Fuesse muessen auf der Entity-Position stehen (y=0), nicht darueber schweben oder
// im Boden stecken: Beine sind 12 Pixel lang mit Pivot y=12, also 1.501 - 24/16 = 0.001.
//
// Geprueft wird das BASISBEIN. Der Gesamt-Tiefstpunkt liegt bewusst etwas darunter: die
// Hosen-Aussenlage ist um 0.25 Pixel dilatiert und ragt entsprechend unter den Fuss --
// das ist Vanilla-Verhalten, keine Abweichung (der erste Anlauf dieses Tests hat genau
// deshalb angeschlagen).
#[test]
fn fuesse_stehen_auf_der_entity_position() {
    let model = player_model(false);
    let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
    let leg = model.parts.iter().find(|p| p.name == "right_leg").expect("right_leg fehlt");

    let mut ohne_hose = Vec::new();
    emit_part(
        &Part::new("right_leg", leg.pivot, leg.cuboids.clone()),
        &Pose::new(),
        base,
        model.tex_size,
        [0.0, 0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        false,
        &mut ohne_hose,
    );
    let sohle = ohne_hose.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
    assert!((sohle - 0.001).abs() < 1e-4, "Fusssohle {sohle}, erwartet 0.001");

    let mut alles = Vec::new();
    emit(&model, &Pose::new(), base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut alles);
    let tiefster = alles.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
    let hosen_dilation = 0.25 / 16.0;
    assert!(
        tiefster >= 0.001 - hosen_dilation - 1e-4,
        "tiefster Punkt {tiefster} darf hoechstens um die Hosen-Dilation unter die Sohle reichen"
    );
}

// Gierung: bei bodyYaw 0 schaut ein Entity in Vanilla nach SUEDEN (+Z). Die Nase des
// Kopfes sitzt im Modellraum bei z=-4, das 180-Grad-Drehen von entity_transform muss sie
// also auf +Z bringen.
#[test]
fn gierung_null_schaut_nach_sueden() {
    let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
    // Punkt vor dem Gesicht im Modellraum (Pixel), z=-4 ist die Vorderseite.
    let front = base.apply([0.0, 0.0, -4.0 / 16.0]);
    assert!(front[2] > 0.1, "Vorderseite muss bei Gierung 0 nach +Z zeigen, ist z={}", front[2]);
}

// Eine Drehung um 90 Grad muss die Vorderseite nach WESTEN (-X) bringen -- Vanillas
// Gierung waechst im Uhrzeigersinn von Sueden aus.
#[test]
fn gierung_neunzig_schaut_nach_westen() {
    let base = entity_transform([0.0, 0.0, 0.0], 90.0, 1.0, PLAYER_MODEL_SCALE);
    let front = base.apply([0.0, 0.0, -4.0 / 16.0]);
    assert!(front[0] < -0.1, "Gierung 90 muss nach -X zeigen, ist x={}", front[0]);
}

// Der duenne Arm (Slim-Skin) ist 3 statt 4 Pixel breit -- sonst waere die Alex-Variante
// nur eine andere Textur auf demselben Koerper.
#[test]
fn slim_hat_schmalere_arme_als_classic() {
    let breite = |slim: bool| {
        let model = player_model(slim);
        let arm = model.parts.iter().find(|p| p.name == "right_arm").expect("right_arm fehlt");
        arm.cuboids[0].size[0]
    };
    assert_eq!(breite(false), 4.0);
    assert_eq!(breite(true), 3.0);
}

// Die Aussenlagen (Hut, Jacke, Aermel, Hosen) sind dilatierte Kopien -- ohne Dilation
// laegen sie exakt auf der Basisschicht und wuerden z-fighten.
#[test]
fn aussenlagen_sind_dilatiert() {
    let model = player_model(false);
    let head = model.parts.iter().find(|p| p.name == "head").unwrap();
    let hat = head.children.iter().find(|p| p.name == "hat").expect("hat fehlt");
    assert_eq!(hat.cuboids[0].dilation, [0.5; 3], "Hut nutzt Vanillas Dilation 0.5");
    let body = model.parts.iter().find(|p| p.name == "body").unwrap();
    let jacket = body.children.iter().find(|p| p.name == "jacket").expect("jacket fehlt");
    assert_eq!(jacket.cuboids[0].dilation, [0.25; 3]);
}

// Schwimmen legt die Figur waagerecht. Pruefung: ohne Neigung ist der Koerper hoeher als
// lang, mit voller Neigung laenger als hoch.
#[test]
fn schwimmen_legt_den_koerper_waagerecht() {
    let model = player_model(false);
    let masse = |lean: &Lean| {
        let base = entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, lean);
        let mut out = Vec::new();
        emit(&model, &Pose::new(), base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
        let hoehe =
            out.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max) - out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        let laenge =
            out.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max) - out.iter().map(|v| v.position[2]).fold(f32::MAX, f32::min);
        (hoehe, laenge)
    };
    let (h_stehend, l_stehend) = masse(&Lean::NONE);
    assert!(h_stehend > l_stehend, "stehend muss hoeher als lang sein ({h_stehend} / {l_stehend})");

    let schwimmend = Lean { amount: 1.0, pitch_deg: 0.0, in_water: true, swimming: true };
    let (h_schwimmend, l_schwimmend) = masse(&schwimmend);
    assert!(l_schwimmend > h_schwimmend, "schwimmend muss laenger als hoch sein ({h_schwimmend} / {l_schwimmend})");
    assert!(l_schwimmend > l_stehend * 2.0, "die Figur muss sich wirklich strecken");
}

// Die Neigung muss GLEITEND greifen -- Vanilla laeuft mit 0.09 pro Tick hoch. Bei halbem
// Wert muss die Figur genau dazwischen liegen, sonst klappt sie in einem Frame um.
#[test]
fn neigung_greift_gleitend() {
    let winkel = |amount: f32| {
        let lean = Lean { amount, pitch_deg: 0.0, in_water: true, swimming: false };
        let base = entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean);
        // Ein Punkt ueber dem Kopf im Modellraum: je waagerechter der Koerper, desto
        // weiter wandert er nach vorn und desto weniger nach oben.
        base.apply([0.0, -24.0 / 16.0, 0.0])
    };
    let aufrecht = winkel(0.0);
    let halb = winkel(0.5);
    let voll = winkel(1.0);
    assert!(aufrecht[1] > halb[1] && halb[1] > voll[1], "die Hoehe muss monoton fallen");
    assert!(voll[1].abs() < 0.05, "bei voller Neigung liegt der Kopf praktisch auf Fusshoehe");
}

// Im Wasser richtet sich der Koerper nach der Blickneigung, beim Kriechen an Land NICHT
// (dort liegt er stur waagerecht) -- genau die Unterscheidung, die Vanilla ueber
// isTouchingWater macht.
#[test]
fn blickneigung_wirkt_nur_im_wasser() {
    let punkt = |in_water: bool, pitch: f32| {
        let lean = Lean { amount: 1.0, pitch_deg: pitch, in_water, swimming: false };
        entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean).apply([0.0, -24.0 / 16.0, 0.0])
    };
    // Im Wasser: nach unten schauen (positiver Vanilla-Pitch) aendert die Haltung.
    assert!((punkt(true, 0.0)[1] - punkt(true, 40.0)[1]).abs() > 0.1, "im Wasser muss der Blick die Haltung drehen");
    // An Land: die Blickneigung darf nichts aendern.
    let a = punkt(false, 0.0);
    let b = punkt(false, 40.0);
    assert!((a[1] - b[1]).abs() < 1e-5 && (a[2] - b[2]).abs() < 1e-5, "beim Kriechen liegt der Koerper unabhaengig vom Blick");
}

// Der Kopf muss beim Schwimmen Richtung -45 Grad gezogen werden, sonst schaut die Figur
// bei waagerechtem Koerper in den Boden.
#[test]
fn kopf_wird_beim_schwimmen_angehoben() {
    let ohne = biped_pose_leaning(0.0, 0.0, 0.0, 0.0, false, 0.0, 0.0).get("head").rotation[0];
    let voll = biped_pose_leaning(0.0, 0.0, 0.0, 0.0, false, 1.0, 0.0).get("head").rotation[0];
    assert_eq!(ohne, 0.0);
    assert!((voll + std::f32::consts::FRAC_PI_4).abs() < 1e-5, "bei voller Neigung genau -45 Grad, ist {voll}");
    let halb = biped_pose_leaning(0.0, 0.0, 0.0, 0.0, false, 0.5, 0.0).get("head").rotation[0];
    assert!(halb < 0.0 && halb > voll, "die Haelfte muss dazwischen liegen ({halb})");
}

// Sprint-Schwimmen verschiebt den Koerper zusaetzlich (Vanilla: translate(0,-1,0.3)) --
// reines Kriechen nicht.
#[test]
fn sprintschwimmen_verschiebt_zusaetzlich() {
    let punkt = |swimming: bool| {
        let lean = Lean { amount: 1.0, pitch_deg: 0.0, in_water: true, swimming };
        entity_transform_leaning([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE, &lean).apply([0.0, 0.0, 0.0])
    };
    assert_ne!(punkt(true), punkt(false), "isSwimming muss einen Versatz erzeugen");
}

// Ein 8x8x8-Kasten bei UV (0,0) auf einer 64x64-Textur: die Nordseite (das Gesicht) muss
// auf dem Ausschnitt u 8..16, v 8..16 liegen -- genau dort sitzt das Gesicht in jedem
// Minecraft-Skin.
#[test]
fn kopf_uv_trifft_das_gesichtsfeld() {
    let cuboid = Cuboid::new([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]);
    let mut out = Vec::new();
    emit_cuboid(&cuboid, Affine::IDENTITY, AxisShades::of(Affine::IDENTITY), [64.0, 64.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], false, &mut out);
    // Nordseite ist die vierte der sechs Seiten, je 6 Vertices.
    let north = &out[18..24];
    let us: Vec<f32> = north.iter().map(|v| v.uv[0] * 64.0).collect();
    let vs: Vec<f32> = north.iter().map(|v| v.uv[1] * 64.0).collect();
    assert_eq!(us.iter().copied().fold(f32::MAX, f32::min), 8.0);
    assert_eq!(us.iter().copied().fold(f32::MIN, f32::max), 16.0);
    assert_eq!(vs.iter().copied().fold(f32::MAX, f32::min), 8.0);
    assert_eq!(vs.iter().copied().fold(f32::MIN, f32::max), 16.0);
}

// Jede Seite jedes Kastens muss zwei Dreiecke liefern -- ein fehlendes Face ist im Spiel
// ein Loch, durch das man ins Modell sieht.
#[test]
fn jeder_kasten_liefert_sechs_vollstaendige_seiten() {
    for slim in [false, true] {
        let model = player_model(slim);
        let mut kaesten = 0;
        fn zaehle(part: &Part, kaesten: &mut usize) {
            *kaesten += part.cuboids.len();
            for child in &part.children {
                zaehle(child, kaesten);
            }
        }
        for part in &model.parts {
            zaehle(part, &mut kaesten);
        }
        let mut out = Vec::new();
        emit(&model, &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
        assert_eq!(out.len(), kaesten * 36, "slim={slim}: 6 Seiten * 2 Dreiecke * 3 Vertices pro Kasten");
    }
}

// Verkettung: erst drehen, dann verschieben ist NICHT dasselbe wie umgekehrt. Ohne
// korrektes then() sitzen Gliedmassen an der falschen Stelle, sobald sie gedreht sind.
#[test]
fn verkettung_haelt_die_reihenfolge_ein() {
    let drehen = Affine::rotation_y_deg(90.0);
    let schieben = Affine::translation([1.0, 0.0, 0.0]);
    // erst schieben, dann drehen
    let a = drehen.then(schieben).apply([0.0, 0.0, 0.0]);
    // erst drehen, dann schieben
    let b = schieben.then(drehen).apply([0.0, 0.0, 0.0]);
    assert!((a[0]).abs() < 1e-5 && (a[2] + 1.0).abs() < 1e-5, "erst schieben, dann drehen: {a:?}");
    assert!((b[0] - 1.0).abs() < 1e-5 && b[2].abs() < 1e-5, "erst drehen, dann schieben: {b:?}");
}

// Eine Pose-Drehung muss auf die GRUNDdrehung addieren, nicht sie ersetzen -- sonst
// verliert z.B. der Kuh-Koerper seine feste 90-Grad-Kippung, sobald er animiert wird.
#[test]
fn pose_addiert_auf_die_grunddrehung() {
    let part = Part::new("test", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])]).rotated([
        std::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    ]);
    let mut pose = Pose::new();
    pose.rotate("test", [std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
    let mut mit = Vec::new();
    emit_part(&part, &pose, Affine::IDENTITY, [64.0, 64.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], false, &mut mit);
    let mut ohne = Vec::new();
    emit_part(&part, &Pose::new(), Affine::IDENTITY, [64.0, 64.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0], false, &mut ohne);
    // 90 + 90 Grad um X: der Punkt (0,0,1/16) landet bei -1/16 in Z, nicht bei +1/16 in Y.
    let punkt_mit = mit.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
    let punkt_ohne = ohne.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
    assert!(punkt_mit < punkt_ohne, "zwei mal 90 Grad muss weiter drehen als einmal");
}

// AxisShades (Licht einmal je Teil) gegen die fruehere Rechnung (Normale je Seite gedreht,
// normiert, beleuchtet): Bit fuer Bit gleiche Farben, auch gespiegelt, zweiseitig, mit
// Schraeglagen, Skalierung und Spiegelung durch eine negative Skala.
#[test]
fn achsen_licht_wie_je_seite() {
    let alt = |cuboid: &Cuboid, world: Affine, two_sided: bool| -> Vec<[f32; 3]> {
        let m = if cuboid.mirror { -1.0 } else { 1.0 };
        let normals: [[f32; 3]; 6] = [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [-m, 0.0, 0.0], [0.0, 0.0, -1.0], [m, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let mut farben = Vec::new();
        for (i, n) in normals.iter().enumerate() {
            if cuboid.faces & (1 << i) == 0 {
                continue;
            }
            let normale = world.apply_direction(*n);
            let shade = diffuse_shade(normale);
            farben.extend([[0.8 * shade, 0.9 * shade, shade]; 6]);
            if two_sided {
                let hinten = diffuse_shade([-normale[0], -normale[1], -normale[2]]);
                farben.extend([[0.8 * hinten, 0.9 * hinten, hinten]; 6]);
            }
        }
        farben
    };
    let welten = [
        Affine::IDENTITY,
        Affine::rotation_xyz_deg(20.0, -35.0, 70.0),
        Affine::rotation_zyx(1.3, -2.9, 0.4).then(Affine::scale([0.4, 1.7, 0.9])),
        Affine::translation([3.0, -1.0, 2.0]).then(Affine::rotation_xyz_deg(-90.0, 180.0, 45.0)).then(Affine::scale([-1.0, 1.0, 1.0])),
        Affine::rotation_xyz_deg(0.0, 90.0, 0.0),
    ];
    let mut kasten = Cuboid::new([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]);
    let mut geprueft = 0;
    for world in welten {
        for mirror in [false, true] {
            for faces in [0b11_1111u8, 0b10_1101] {
                for two_sided in [false, true] {
                    kasten.mirror = mirror;
                    kasten.faces = faces;
                    let mut out = Vec::new();
                    emit_cuboid(&kasten, world, AxisShades::of(world), [64.0, 64.0], [0.0, 0.0, 1.0, 1.0], [0.8, 0.9, 1.0], two_sided, &mut out);
                    let neu: Vec<[u32; 3]> = out.iter().map(|v| v.color.map(f32::to_bits)).collect();
                    let erwartet: Vec<[u32; 3]> = alt(&kasten, world, two_sided).iter().map(|c| c.map(f32::to_bits)).collect();
                    assert_eq!(neu, erwartet, "mirror {mirror}, faces {faces:#b}, two_sided {two_sided}, {world:?}");
                    geprueft += 1;
                }
            }
        }
    }
    assert_eq!(geprueft, 40);
}

// emit_into schreibt genau vertex_count Vertices, und dieselben wie emit/emit_two_sided --
// auch mit versteckten Teilen (samt Kindern) und gedrehten Gliedern.
#[test]
fn emit_into_wie_emit() {
    let tex = [0.1, 0.2, 0.6, 0.9];
    let base = Affine::translation([0.5, -1.0, 2.0]).then(Affine::rotation_xyz_deg(10.0, 40.0, -5.0));
    let mut gedreht = biped_pose(0.4, -0.3, 1.2, 0.8, true, 7.0);
    gedreht.set("head", PartPose { rotation: [0.3, 0.0, 0.0], pivot_offset: [0.0; 3], rotation_matrix: None, hidden: true });
    for model in [player_model(false), player_model(true), biped_model("x", 0.5)] {
        for pose in [Pose::new(), gedreht.clone()] {
            for two_sided in [false, true] {
                let mut erwartet = Vec::new();
                if two_sided {
                    emit_two_sided(&model, &pose, base, tex, 0.8, &mut erwartet);
                } else {
                    emit(&model, &pose, base, tex, 0.8, &mut erwartet);
                }
                assert_eq!(vertex_count(&model, &pose, two_sided), erwartet.len());
                let mut abschnitt = vec![bytemuck::Zeroable::zeroed(); erwartet.len()];
                emit_into(&model, &pose, base, tex, [0.8; 3], two_sided, &mut abschnitt);
                assert!(bytemuck::cast_slice::<_, u8>(&abschnitt) == bytemuck::cast_slice::<_, u8>(&erwartet));
            }
        }
    }
}
