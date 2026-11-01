use super::*;
use world::ChunkSection;

// Baut eine Testwelt mit einer durchgehend soliden Flaeche bei y=floor_y (Zustand 1 =
// "solide", s. world::is_solid) ueber mehrere Chunk-Spalten (5x5, also 80x80 Bloecke um
// den Ursprung) -- eine einzelne 16x16-Spalte ist bei ~100 Ticks Sprint/Lauf-Strecke zu
// klein, der Spieler laeuft sonst mitten im Test ueber die Kante ins Leere.
fn world_with_floor(floor_y: i32) -> World {
    let mut w = World::new();
    let section_index = (floor_y - world::MIN_Y).div_euclid(16);
    let local_y = (floor_y - world::MIN_Y).rem_euclid(16);
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == section_index {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((local_y * 16 + z) * 16 + x) as usize] = 1;
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Boden NUR fuer Welt-X < edge_x -> eine Abbruchkante bei x=edge_x (fuer den Sneak-Test).
fn world_with_floor_up_to_x(floor_y: i32, edge_x: i32) -> World {
    let mut w = World::new();
    let section_index = (floor_y - world::MIN_Y).div_euclid(16);
    let local_y = (floor_y - world::MIN_Y).rem_euclid(16);
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == section_index {
                    for x in 0..16 {
                        let world_x = cx * 16 + x;
                        if world_x < edge_x {
                            for z in 0..16 {
                                states[((local_y * 16 + z) * 16 + x) as usize] = 1;
                            }
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// TridentItem.onStoppedUsing, Sog III (Staerke 3.0): waagerecht nach Sueden (Yaw 0,
// Pitch 0) gibt es genau 3.0 in +Z; am Boden hebt Vanilla zusaetzlich um 1.1999999 an.
#[test]
fn sog_schub_und_anheben_wie_vanilla() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    p.on_ground = true;
    p.start_riptide(0.0, 0.0, 3.0, &world, &shapes);
    assert!((p.vel.z - 3.0).abs() < 1e-3, "Schub in Blickrichtung: {:?}", p.vel);
    assert!(p.vel.x.abs() < 1e-3 && p.vel.y.abs() < 1e-3);
    assert!((p.pos.y - (1.0 + RIPTIDE_GROUND_LIFT)).abs() < 1e-9, "am Boden angehoben: {}", p.pos.y);
    assert!(!p.on_ground);
    assert_eq!(p.riptide_ticks, RIPTIDE_TICKS);

    // Senkrecht nach oben (Vanilla-Pitch -90), in der Luft: kein Anheben, voller Schub hoch.
    let mut q = PlayerPhysics::new(DVec3::new(8.5, 10.0, 8.5));
    q.on_ground = false;
    q.start_riptide(0.0, -90.0, 1.5, &world, &shapes);
    assert!((q.vel.y - 1.5).abs() < 1e-3, "{:?}", q.vel);
    assert_eq!(q.pos.y, 10.0);
}

// tickRiptide: ein Lebewesen in der Bahn beendet den Sog mit Rueckprall, ein Nicht-
// Lebewesen tut nichts (und verhindert sogar den Wand-Abbruch), sonst beendet eine Wand ihn.
#[test]
fn sog_endet_an_lebewesen_und_waenden() {
    let mut p = PlayerPhysics::new(DVec3::new(0.0, 5.0, 0.0));
    p.riptide_ticks = 20;
    p.vel = DVec3::new(0.0, 0.0, 2.0);
    let vorher = DVec3::new(0.0, 5.0, -2.0);
    let gegner = (DVec3::new(-0.3, 5.0, -1.3), DVec3::new(0.3, 6.8, -0.7), true);
    p.tick_riptide(vorher, &[gegner]);
    assert_eq!(p.riptide_ticks, 0);
    assert!((p.vel.z - -0.4).abs() < 1e-9, "Rückprall mit -0.2: {:?}", p.vel);

    let mut q = PlayerPhysics::new(DVec3::new(0.0, 5.0, 0.0));
    q.riptide_ticks = 20;
    q.horizontal_collision = true;
    let pfeil = (DVec3::new(-0.1, 5.0, -1.1), DVec3::new(0.1, 5.2, -0.9), false);
    q.tick_riptide(vorher, &[pfeil]);
    assert_eq!(q.riptide_ticks, 19, "nur Nicht-Lebewesen getroffen: weder Abbruch noch Wand-Abbruch");

    q.tick_riptide(vorher, &[]);
    assert_eq!(q.riptide_ticks, 0, "Wand beendet den Sog");
}

// Die Pose SPIN_ATTACK haengt am Server-Flag, ist 0.6 hoch und geht vor dem Schleichen.
#[test]
fn sog_pose_aus_dem_flag() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    p.using_riptide = true;
    let mut input = idle_input(0.0);
    input.sneak = true;
    p.tick(&input, &world, &shapes);
    assert!(p.spin_attack && !p.crouching);
    assert_eq!(p.current_height(), SWIMMING_HEIGHT_F64);
    assert_eq!(p.eye_height(), SWIMMING_EYE_HEIGHT);
    p.using_riptide = false;
    p.tick(&input, &world, &shapes);
    assert!(!p.spin_attack && p.crouching);
}

// Vanillas bekannte Endgeschwindigkeiten: 4.317 Bloecke/s gehend = 0.2159 pro Tick,
// 5.612 sprintend = 0.2806. Beides muss die eigene Physik treffen -- daran haengt nicht
// nur das Spielgefuehl, sondern auch die Schrittanimation der Modelle
// (world::LimbAnimator rechnet min(Strecke * 4, 1)).
#[test]
fn strecke_pro_tick_trifft_vanilla() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let messe = |sprint: bool| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = sprint;
        // Genug Ticks, um die Endgeschwindigkeit zu erreichen.
        for _ in 0..80 {
            p.tick(&input, &world, &shapes);
        }
        let before = p.pos;
        p.tick(&input, &world, &shapes);
        ((p.pos.x - before.x).powi(2) + (p.pos.z - before.z).powi(2)).sqrt()
    };
    let gehen = messe(false);
    let sprinten = messe(true);
    assert!((gehen - 0.2159).abs() < 0.002, "Gehen: {gehen:.4} Blöcke/Tick, erwartet 0.2159");
    assert!((sprinten - 0.2806).abs() < 0.002, "Sprinten: {sprinten:.4} Blöcke/Tick, erwartet 0.2806");
    // Und die Folge fuer die Animation: Sprinten deckelt die Amplitude, Gehen nicht.
    assert!((gehen as f32 * 4.0) < 1.0, "gehend darf die Amplitude NICHT ausreizen");
    assert!((sprinten as f32 * 4.0) >= 1.0, "sprintend muss sie ausreizen");
}

// --- Schnelligkeit und Langsamkeit ---------------------------------------------------

#[test]
fn der_faktor_trifft_die_vanilla_stufen() {
    // Stufe I ist Staerke 0. Vanilla rechnet grundwert * (staerke + 1).
    assert!((effect_speed_factor(Some(0), None) - 1.2).abs() < 1e-6, "Schnelligkeit I = +20 %");
    assert!((effect_speed_factor(Some(1), None) - 1.4).abs() < 1e-6, "Schnelligkeit II = +40 %");
    assert!((effect_speed_factor(None, Some(0)) - 0.85).abs() < 1e-6, "Langsamkeit I = -15 %");
    assert!((effect_speed_factor(None, Some(1)) - 0.7).abs() < 1e-6, "Langsamkeit II = -30 %");
    assert_eq!(effect_speed_factor(None, None), 1.0, "ohne Effekt darf sich nichts ändern");
}

#[test]
// Beide Effekte sind ADD_MULTIPLIED_TOTAL, und die legt Vanilla NACHEINANDER multiplikativ
// an (`wert *= 1 + modifikator`), nicht als Summe. Wer die Prozente addiert, kommt bei
// 1.05 statt 1.02 heraus -- knapp daneben und im Spiel kaum zu bemerken, fuer GrimAC aber
// ein Unterschied.
fn zwei_effekte_multiplizieren_sich_sie_addieren_sich_nicht() {
    let beides = effect_speed_factor(Some(0), Some(0));
    assert!((beides - 1.2 * 0.85).abs() < 1e-6, "{beides} statt {}", 1.2 * 0.85);
    assert!((beides - 1.05).abs() > 0.02, "das wäre die addierte Rechnung");
}

#[test]
// Ab Langsamkeit VII (Staerke 6) ist der Faktor NEGATIV: 1 - 0.15*7 = -0.05. Ungeklemmt
// liefe man rueckwaerts, statt stillzustehen -- das ist der Grund fuer die Untergrenze 0
// der ClampedEntityAttribute.
fn langsamkeit_sieben_haelt_still_statt_rueckwaerts_zu_laufen() {
    assert!(effect_speed_factor(None, Some(6)) < 0.0, "der reine Faktor ist negativ");

    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.effect_speed_factor = effect_speed_factor(None, Some(6));
    // Erst absetzen lassen: frisch erzeugt gilt der Spieler als nicht am Boden, und in der
    // LUFT wirkt der Effekt bewusst nicht (s. die_luftkontrolle_bleibt_von_den_effekten_
    // unberuehrt). Vanilla verhaelt sich genauso -- auch mit Langsamkeit VII laesst sich
    // im Sprung noch steuern.
    for _ in 0..5 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.on_ground, "Testaufbau: der Spieler muss am Boden stehen");
    input.forward = 1.0;
    let start = p.pos;
    for _ in 0..40 {
        p.tick(&input, &world, &shapes);
    }
    let strecke = ((p.pos.x - start.x).powi(2) + (p.pos.z - start.z).powi(2)).sqrt();
    assert!(strecke < 1e-6, "mit Langsamkeit VII darf man sich nicht bewegen, lief aber {strecke}");
}

#[test]
// Die Gegenprobe zu strecke_pro_tick_trifft_vanilla, mit denselben bekannten Zahlen:
// 4.317 Bloecke/s gehend, mal dem Effektfaktor.
fn strecke_pro_tick_folgt_dem_effektfaktor() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let messe = |sprint: bool, faktor: f64| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = sprint;
        input.effect_speed_factor = faktor;
        for _ in 0..80 {
            p.tick(&input, &world, &shapes);
        }
        let before = p.pos;
        p.tick(&input, &world, &shapes);
        ((p.pos.x - before.x).powi(2) + (p.pos.z - before.z).powi(2)).sqrt()
    };
    let schnell_1 = messe(false, effect_speed_factor(Some(0), None));
    assert!((schnell_1 - 0.2159 * 1.2).abs() < 0.003, "Gehen mit Schnelligkeit I: {schnell_1:.4}, erwartet {:.4}", 0.2159 * 1.2);
    let langsam_1 = messe(false, effect_speed_factor(None, Some(0)));
    assert!((langsam_1 - 0.2159 * 0.85).abs() < 0.003, "Gehen mit Langsamkeit I: {langsam_1:.4}, erwartet {:.4}", 0.2159 * 0.85);
    // Sprint und Effekt greifen ineinander, ohne sich gegenseitig zu verschlucken.
    let sprint_schnell = messe(true, effect_speed_factor(Some(1), None));
    assert!((sprint_schnell - 0.2806 * 1.4).abs() < 0.005, "Sprinten mit Schnelligkeit II: {sprint_schnell:.4}");
}

#[test]
// PlayerEntity.getOffGroundSpeed() gibt ein FESTES Paar zurueck (0.02 / 0.026 sprintend),
// nicht das Bewegungstempo-Attribut. Schnelligkeit darf die Luftkontrolle deshalb NICHT
// veraendern -- und das ist keine Kleinigkeit: die Luftbeschleunigung ist genau die
// Groesse, an der GrimAC waehrend eines Sprungs rechnet.
fn die_luftkontrolle_bleibt_von_den_effekten_unberuehrt() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let messe = |faktor: f64| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut input = idle_input(0.0);
        input.effect_speed_factor = faktor;
        // Erst springen (ohne Vorwaertsdrang, damit der Sprung nichts mitnimmt), dann in
        // der Luft Gas geben.
        input.jump = true;
        p.tick(&input, &world, &shapes);
        input.jump = false;
        input.forward = 1.0;
        let vorher = (p.vel.x, p.vel.z);
        p.tick(&input, &world, &shapes);
        ((p.vel.x - vorher.0).powi(2) + (p.vel.z - vorher.1).powi(2)).sqrt()
    };
    let ohne = messe(1.0);
    let mit = messe(effect_speed_factor(Some(1), None));
    assert!(ohne > 0.0, "Testaufbau: in der Luft muss überhaupt beschleunigt werden");
    assert!((ohne - mit).abs() < 1e-9, "Luftbeschleunigung ohne {ohne} vs. mit Schnelligkeit {mit}");
}

// GrimACs SprintE (grim.sprint.wall) flaggte im Kaktus-Labyrinth auf anticheat-test.com.
// Ursache: minorHorizontalCollision fehlte. Hier die beiden Enden der Vanilla-Regel --
// geradewegs in die Wand ist ein Anprall (Sprint bricht ab), schraeg daran entlang ist
// ein Streifen (Sprint laeuft weiter, < 8 Grad Abweichung).
#[test]
fn streifender_wandkontakt_bricht_den_sprint_nicht_ab() {
    // Blick nach +Z (yaw_rad = 0), Vorwaertsdrang: gewollte Richtung ist +Z.
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    // Frontal: die Z-Achse ist weg, es bleibt nur ein Rest auf X -- 90 Grad daneben.
    assert!(!is_horizontal_collision_minor(&input, DVec3::new(0.02, 0.0, 0.0)));
    // Streifen: fast die volle gewollte Bewegung, nur leicht zur Seite abgelenkt
    // (atan(0.01/0.2) = 2.9 Grad < 8).
    assert!(is_horizontal_collision_minor(&input, DVec3::new(0.01, 0.0, 0.2)));
    // Knapp jenseits der Grenze: atan(0.2/1.0) = 11.3 Grad > 8.
    assert!(!is_horizontal_collision_minor(&input, DVec3::new(0.2, 0.0, 1.0)));
    // Steht man praktisch still, ist nichts "minor" -- Vanillas 1e-5-Ausstieg.
    assert!(!is_horizontal_collision_minor(&input, DVec3::new(0.0, 0.0, 0.001)));
    // Ohne Eingabe ebenfalls nicht (i < 1e-5).
    let leer = idle_input(0.0);
    assert!(!is_horizontal_collision_minor(&leer, DVec3::new(0.0, 0.0, 0.2)));
}

// Die Grenze selbst: exakt Vanillas 0.13962634 rad. Ein Winkel knapp darunter zaehlt als
// Streifen, knapp darueber nicht -- damit ist auch das Vorzeichen der Drehung mitgeprueft.
#[test]
fn die_acht_grad_grenze_liegt_auf_vanillas_wert() {
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    let bei = |grad: f64| {
        let r = grad.to_radians();
        is_horizontal_collision_minor(&input, DVec3::new(r.sin(), 0.0, r.cos()))
    };
    let grenze_grad = f64::from(0.13962634f32) * 180.0 / std::f64::consts::PI;
    assert!(bei(grenze_grad - 0.01), "knapp innerhalb der Grenze muss streifend sein");
    assert!(!bei(grenze_grad + 0.01), "knapp außerhalb der Grenze darf es nicht sein");
    // Seitenverkehrt darf es keinen Unterschied machen.
    assert!(bei(-(grenze_grad - 0.01)));
    assert!(!bei(-(grenze_grad + 0.01)));
}

// Fall in einer mitgeschnittenen Welt: alle Chunk-Pakete aus einem Rohmitschnitt
// (pktproxy, CORAL_PKTRAW; Format s. net::write_raw), min_y der Dimension, Startposition.
// Druckt die Position je Tick -- zum Vergleich mit dem, was ein Client dort schickt.
// CORAL_FALL_RAW=<datei> CORAL_FALL_MIN_Y=0 CORAL_FALL_START=0.5,100,0.5 \
//   cargo test -p app --bins fall_auf_mitschnitt -- --ignored --nocapture
#[test]
#[ignore]
fn fall_auf_mitschnitt() {
    let roh = std::fs::read(std::env::var("CORAL_FALL_RAW").expect("CORAL_FALL_RAW")).unwrap();
    let min_y: i32 = std::env::var("CORAL_FALL_MIN_Y").ok().and_then(|v| v.parse().ok()).unwrap_or(-64);
    let start: Vec<f64> = std::env::var("CORAL_FALL_START").unwrap_or("0.5,100,0.5".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let mut w = World::new();
    w.set_min_y(min_y);
    let mut i = 0;
    while i + 17 <= roh.len() {
        let len = u32::from_be_bytes(roh[i + 13..i + 17].try_into().unwrap()) as usize;
        let payload = &roh[i + 17..i + 17 + len];
        if roh[i] == b'S' && payload.first() == Some(&(protocol::packets::CB_PLAY_CHUNK_DATA as u8)) {
            if let Ok(c) = protocol::chunk::parse_chunk_data(payload) {
                let sections = c.sections.iter().map(|s| ChunkSection::from_states_and_biomes(s.block_states.clone(), s.biomes.clone())).collect();
                w.insert_column(c.x, c.z, sections);
            }
        }
        i += 17 + len;
    }
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(start[0], start[1], start[2]));
    let mut input = idle_input(0.0);
    // CORAL_FALL_FLOAT=1: wie mit Modul Botcheck-Pass (s. Input::float_drag).
    input.float_drag = std::env::var_os("CORAL_FALL_FLOAT").is_some();
    for t in 0..30 {
        p.tick(&input, &w, &shapes);
        println!("{t:2} pos=({:?}, {:?}, {:?}) boden={}", p.pos.x, p.pos.y, p.pos.z, p.on_ground);
    }
}

fn idle_input(yaw_rad: f32) -> Input {
    Input {
        forward: 0.0,
        strafe: 0.0,
        jump: false,
        sneak: false,
        sprint_held: false,
        using_item: false,
        item_use_speed: ITEM_USE_SPEED_MULT,
        item_use_can_sprint: false,
        food_level: 20,
        blindness: false,
        effect_speed_factor: 1.0,
        movement_speed: BASE_SPEED,
        sneaking_speed: DEFAULT_SNEAKING_SPEED,
        water_movement_efficiency: 0.0,
        slow_falling: false,
        levitation: None,
        jump_boost: None,
        dolphins_grace: false,
        weaving: false,
        float_drag: false,
        yaw_rad,
        look_dir_y: 0.0,
        yaw_deg: yaw_rad.to_degrees(),
        pitch_deg: 0.0,
        glider_equipped: false,
        spectator: false,
        spectator_speed: 0.0,
    }
}

fn cobweb_state() -> u32 {
    (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("cobweb")).expect("cobweb unbekannt")
}

// Wie all_full_cubes(), aber mit LEERER Form fuers Spinnennetz -- sonst waere es in den
// Tests eine Wand, auf der man steht, statt eines Blocks, durch den man sich bewegt.
//
// Im echten Spiel kommt das von allein: die Formtabelle wird aus den MODELLEN gebaut, und
// das Netz ist ein Kreuz-Block (zwei diagonale Ebenen), dessen Boxliste damit leer ist --
// s. cross_raw in main.rs. Nur der Test-Stub all_full_cubes() macht aus jedem Block einen
// Vollwuerfel und muesste hier sonst als Boden herhalten.
// Vanillas Herausschieben aus einem Block (moveTowardsClosestSpace). Getestet wird die
// Regel selbst, nicht der ganze Tick: steckt man drin, setzt sie 0.1 Richtung der
// naechsten freien Spalte -- und wer frei steht oder in Glas steht, wird nicht geschoben
// (Glas erstickt in Vanilla ausdruecklich nicht).
#[test]
fn stecken_im_block_schiebt_mit_null_komma_eins_heraus() {
    let bloeck = |name: &str| {
        (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap_or_else(|| panic!("{name} unbekannt"))
    };
    let shapes = world::BlockShapes::all_full_cubes();

    // Frei stehen: keine Verschiebung.
    let welt = world_with_floor(0);
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    p.move_towards_closest_space(8.5, 8.5, &welt, &shapes);
    assert_eq!(p.vel, DVec3::ZERO, "ohne Block darf nichts geschoben werden");

    // In Obsidian: die naechste Zellgrenze gewinnt. Bei x=8.8 ist das Osten (0.2 statt
    // 0.8 nach Westen), also +0.1 auf X.
    let mut welt = world_with_floor(0);
    welt.set_block(8, 1, 8, bloeck("obsidian"));
    welt.set_block(8, 2, 8, bloeck("obsidian"));
    let mut p = PlayerPhysics::new(DVec3::new(8.8, 1.0, 8.5));
    p.move_towards_closest_space(8.8, 8.5, &welt, &shapes);
    assert!((p.vel.x - 0.1).abs() < 1e-12 && p.vel.z == 0.0, "nach Osten heraus, war {:?}", p.vel);

    // Dieselbe Lage, aber Fuss in Richtung Norden am naechsten: dann Z.
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.1));
    p.move_towards_closest_space(8.5, 8.1, &welt, &shapes);
    assert!((p.vel.z + 0.1).abs() < 1e-12 && p.vel.x == 0.0, "nach Norden heraus, war {:?}", p.vel);

    // Glas erstickt nicht -- kein Herausschieben.
    let mut welt = world_with_floor(0);
    welt.set_block(8, 1, 8, bloeck("glass"));
    welt.set_block(8, 2, 8, bloeck("glass"));
    let mut p = PlayerPhysics::new(DVec3::new(8.8, 1.0, 8.5));
    p.move_towards_closest_space(8.8, 8.5, &welt, &shapes);
    assert_eq!(p.vel, DVec3::ZERO, "in Glas wird nicht geschoben");
}

// Trampelpfad erstickt, ist aber nur 15/16 hoch. Wer auf einem Pfad steht und mit der
// Ecke ueber einen gleich hohen Nachbarpfad ragt, steckt NICHT darin -- Vanilla prueft die
// Kollisionsform, nicht die Zelle. Ein voller Block daneben (bis 1.0) schiebt dagegen.
#[test]
fn pfad_neben_pfad_schiebt_nicht_aber_gras_daneben_schon() {
    let bloeck = |name: &str| {
        (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap_or_else(|| panic!("{name} unbekannt"))
    };
    let shapes = world::BlockShapes::all_full_cubes();
    let mut welt = world_with_floor(-5);
    welt.set_block(8, 0, 8, bloeck("dirt_path"));
    welt.set_block(9, 0, 8, bloeck("dirt_path"));
    // Auf dem Pfad (Oberkante 15/16), nah an der Ostkante: die Ecke ragt nach x=9.
    let mut p = PlayerPhysics::new(DVec3::new(8.85, 0.9375, 8.5));
    p.move_towards_closest_space(9.06, 8.5, &welt, &shapes);
    assert_eq!(p.vel, DVec3::ZERO, "gleich hoher Pfad daneben liegt unter den Fuessen");

    welt.set_block(9, 0, 8, bloeck("grass_block"));
    let mut p = PlayerPhysics::new(DVec3::new(8.85, 0.9375, 8.5));
    p.move_towards_closest_space(9.06, 8.5, &welt, &shapes);
    assert!((p.vel.x + 0.1).abs() < 1e-12, "Gras ragt 1/16 in die Box, zurueck nach Westen: {:?}", p.vel);
}

// Blasensaeule aus Seelensand, drei Zellen hoch, Luft darueber. Der Spieler (1.8 hoch)
// steht in den unteren beiden: zwei Innenzellen, keine Oberkante -- also zweimal +0.06.
// Mit dem Kopf in der obersten Zelle kommt deren Oberkanten-Schub (+0.1) dazu.
#[test]
fn blasensaeule_schiebt_je_beruehrter_zelle() {
    let hoch = (0..world::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("bubble_column") && world::state_props::property(s, "drag") == Some("false"))
        .unwrap();
    let welt = world_with_floor_and_blocks(0, &[((8, 1, 8), hoch), ((8, 2, 8), hoch), ((8, 3, 8), hoch)]);
    let shapes = world::BlockShapes::new(vec![None; world::state_count()]);
    let input = idle_input(0.0);

    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    p.fall_distance = 3.0;
    p.tick_block_collision(&input, &welt, &shapes);
    assert!((p.vel.y - 0.12).abs() < 1e-12, "zwei Innenzellen: {}", p.vel.y);
    assert_eq!(p.fall_distance, 0.0);

    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.5, 8.5));
    p.tick_block_collision(&input, &welt, &shapes);
    assert!((p.vel.y - 0.22).abs() < 1e-12, "zwei Innenzellen + Oberkante: {}", p.vel.y);

    // Fliegend wirkt nichts.
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    p.abilities.flying = true;
    p.tick_block_collision(&input, &welt, &shapes);
    assert_eq!(p.vel.y, 0.0);
}

fn shapes_ohne_netz_kollision() -> world::BlockShapes {
    let mut per_state: Vec<Option<Vec<world::ShapeBox>>> = vec![None; world::state_count()];
    per_state[cobweb_state() as usize] = Some(Vec::new());
    world::BlockShapes::new(per_state)
}

// Welt mit Boden bei floor_y und einer Saeule aus Spinnennetz darueber, an der Stelle,
// an der die Tests den Spieler hinsetzen.
fn world_with_cobweb_column(floor_y: i32, at: (i32, i32), hoehe: i32) -> World {
    let mut w = world_with_floor(floor_y);
    for dy in 1..=hoehe {
        w.set_block(at.0, floor_y + dy, at.1, cobweb_state());
    }
    w
}

// Der Faktor greift erst im FOLGENDEN Tick (Vanilla setzt ihn in tickBlockCollision nach
// travel und verbraucht ihn im move des naechsten Ticks). Ein Tick zu frueh angewandt
// waere ein Simulationsunterschied bei jedem Netz-Eintritt.
//
// Waagerecht geprueft, nicht im Fall: die Schwerkraft wirkt in travelInAir() erst NACH
// der Bewegung, ein aus dem Stand fallender Spieler legt im ersten Tick also ohnehin
// nichts zurueck -- daran waere der Ein-Tick-Versatz nicht zu erkennen.
#[test]
fn spinnennetz_bremst_erst_den_naechsten_tick() {
    let shapes = shapes_ohne_netz_kollision();
    // Boden bei y=0, Netz in den beiden Zellen darueber -- der Spieler steht IM Netz.
    let mit_netz = world_with_cobweb_column(0, (8, 8), 2);
    let ohne_netz = world_with_floor(0);

    // Gegen denselben Lauf ohne Netz gemessen: der Faktor ist damit exakt die einzige
    // Differenz, ohne dass der Test Vanillas Beschleunigungsformel nachrechnen muesste.
    let schritte = |welt: &World| {
        let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
        p.on_ground = true;
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        let mut out = Vec::new();
        for _ in 0..2 {
            let vorher = p.pos;
            p.tick(&input, welt, &shapes);
            out.push((p.pos - vorher).with_y(0.0).length());
        }
        out
    };
    let netz = schritte(&mit_netz);
    let frei = schritte(&ohne_netz);

    // Tick 1: das Netz wurde in diesem Tick noch nicht beruehrt -- identische Strecke.
    assert!((netz[0] - frei[0]).abs() < 1.0e-12, "erster Tick unverändert: {} vs {}", netz[0], frei[0]);
    assert!(frei[0] > 0.09, "Vorbedingung: es wurde überhaupt gelaufen ({})", frei[0]);
    // Tick 2 verbraucht den Faktor: exakt ein Viertel der freien Strecke.
    assert!((netz[1] - frei[1] * 0.25).abs() < 1.0e-12, "zweiter Tick ist ein Viertel: {} vs {}", netz[1], frei[1]);
}

// Kein Aufsummieren: weil move() die Geschwindigkeit nullt, faengt jeder Tick im Netz bei
// 0 an und faellt immer wieder nur die eine gestauchte Schwerkraft-Strecke. Genau das
// unterscheidet ein Netz von "etwas langsamer fallen".
#[test]
fn im_spinnennetz_beschleunigt_der_fall_nicht() {
    let world = world_with_cobweb_column(0, (8, 8), 8);
    let shapes = shapes_ohne_netz_kollision();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 6.0, 8.5));
    let input = idle_input(0.0);
    // Zwei Ticks Vorlauf, damit der Faktor sicher anliegt.
    p.tick(&input, &world, &shapes);
    p.tick(&input, &world, &shapes);

    let mut strecken = Vec::new();
    for _ in 0..4 {
        let vor = p.pos.y;
        p.tick(&input, &world, &shapes);
        strecken.push(vor - p.pos.y);
    }
    let erste = strecken[0];
    for s in &strecken {
        assert!((s - erste).abs() < 1.0e-9, "jeder Netz-Tick fällt gleich weit: {strecken:?}");
    }
    // Gegen Vanilla nachgerechnet: die Geschwindigkeit startet dank des Nullens bei 0,
    // travelInAir macht daraus -GRAVITY * VERTICAL_DRAG, davon bleiben 5%.
    let erwartet = GRAVITY * VERTICAL_DRAG * f64::from(0.05f32);
    assert!((erste - erwartet).abs() < 1.0e-9, "Fallstrecke im Netz: {erste}, erwartet {erwartet}");
}

// slowMovement() ruft onLanding() -- deshalb schuetzt ein Netz vor Fallschaden, selbst
// wenn man nur hindurchfaellt.
#[test]
fn spinnennetz_setzt_die_fallhoehe_zurueck() {
    let world = world_with_cobweb_column(0, (8, 8), 1);
    let shapes = shapes_ohne_netz_kollision();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 20.0, 8.5));
    let input = idle_input(0.0);
    // Frei fallen, bis Fallhoehe da ist, aber noch weit ueber dem Netz.
    for _ in 0..10 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.fall_distance > 1.0, "Vorbedingung: es wurde gefallen ({})", p.fall_distance);

    // In die Netz-Zelle setzen und einen Tick laufen lassen.
    p.pos = DVec3::new(8.5, 1.0, 8.5);
    p.tick(&input, &world, &shapes);
    assert_eq!(p.fall_distance, 0.0, "das Netz nullt die Fallhöhe");
}

// Buendiges Anliegen an der Zellgrenze ist KEINE Beruehrung -- Vanilla schrumpft die Box
// dafuer um 1.0E-5. Ohne das loeste ein Netz eine Zelle zu frueh aus.
#[test]
fn netz_an_der_zellgrenze_bremst_nicht() {
    let shapes = shapes_ohne_netz_kollision();
    // Die Box endet exakt bei x=9.0, das Netz fuellt die Zelle 9.0..10.0.
    let start = DVec3::new(9.0 - WIDTH_F64 / 2.0, 1.0, 8.5);
    let input = idle_input(0.0);

    let mut mit_netz = world_with_floor(0);
    mit_netz.set_block(9, 1, 8, cobweb_state());
    let ohne_netz = world_with_floor(0);

    // Gegen den netzfreien Lauf gepruefen statt gegen eine Zahl: was hier zaehlt, ist
    // allein, dass das Netz nebenan KEINEN Unterschied macht.
    let laufe = |welt: &World| {
        let mut p = PlayerPhysics::new(start);
        let mut i = input;
        i.forward = 1.0;
        for _ in 0..5 {
            p.tick(&i, welt, &shapes);
        }
        p.pos
    };
    let a = laufe(&mit_netz);
    let b = laufe(&ohne_netz);
    assert!((a - b).length() < 1.0e-12, "Netz an der Zellgrenze darf nichts ändern: {a} vs {b}");
}

// mc_sin muss Vanillas MathHelper.sin (Float-Tabelle) treffen, NICHT das exakte f64-sin --
// sonst driftet die Schwimm-Physik ueber viele Ticks von Grim weg. Referenzwert 0.783306598
// stammt aus der per Hand nachgebauten Vanilla-Tabelle (pitch=-51.57 Grad).
#[test]
fn mc_sin_trifft_vanilla_tabelle_nicht_exaktes_sin() {
    // pitch so, dass Blick-Y ~0.7833 (steil nach oben). look_dir_y_from_pitch nutzt +pitch
    // (unsere Konvention) -> mc_sin(pitch_rad).
    let pitch_deg = 51.5668_f32;
    let d = f64::from(look_dir_y_from_pitch(pitch_deg));
    let exact = f64::from(pitch_deg).to_radians().sin();
    // Muss die Tabellen-Naeherung sein, messbar (~2.6e-5) vom exakten sin entfernt.
    assert!((d - 0.7833066).abs() < 1e-5, "mc_sin-Wert {d} weicht von der Vanilla-Tabelle ab");
    assert!((d - exact).abs() > 1e-5, "mc_sin darf NICHT das exakte f64-sin sein (war {d} vs exakt {exact})");
}

#[test]
fn mc_cos_ist_tabellen_cos_und_bei_null_genau_eins() {
    // MathHelper.cos(0) muss exakt 1.0 sein (Tabellen-Index 16384 = pi/2 -> sin(pi/2)=1).
    assert_eq!(mc_cos(0.0), 1.0);
    assert_eq!(mc_sin(0.0), 0.0);
    // Bei einem schraegen Yaw die Tabellen-Naeherung, messbar vom exakten cos entfernt.
    let yaw_rad = f64::from(40.0_f32.to_radians());
    let table = f64::from(mc_cos(yaw_rad));
    let exact = yaw_rad.cos();
    assert!((table - exact).abs() > 1e-6, "mc_cos muss die Vanilla-Tabelle sein (war {table} vs exakt {exact})");
}

fn kreativ(p: &mut PlayerPhysics) {
    p.abilities = Abilities { may_fly: true, instabuild: true, invulnerable: true, ..Abilities::default() };
}

// Auf dem Boden stehend (Boden bei y 0, Fuesse auf 1), Kreativ.
fn kreativ_am_boden(world: &World, shapes: &world::BlockShapes) -> PlayerPhysics {
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    kreativ(&mut p);
    for _ in 0..3 {
        p.tick(&idle_input(0.0), world, shapes);
    }
    assert!(p.on_ground);
    p
}

#[test]
fn doppelsprung_startet_den_flug_und_springt_ab() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = kreativ_am_boden(&world, &shapes);
    let sprung = Input { jump: true, ..idle_input(0.0) };
    // Erster Druck: ein normaler Sprung, das Fenster beginnt.
    p.tick(&sprung, &world, &shapes);
    assert!(!p.abilities.flying);
    p.tick(&idle_input(0.0), &world, &shapes);
    // Zweiter Druck im Fenster: Flug an.
    p.tick(&sprung, &world, &shapes);
    assert!(p.abilities.flying);
    assert!(p.abilities_changed);
    // Ohne Taste haelt der Flug die Hoehe (0.6 je Tick statt Schwerkraft).
    p.abilities_changed = false;
    for _ in 0..40 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(p.abilities.flying && !p.abilities_changed);
    assert!(p.vel.y.abs() < 1e-6, "{}", p.vel.y);
}

#[test]
fn zwei_druecke_ausserhalb_des_fensters_schalten_nicht() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = kreativ_am_boden(&world, &shapes);
    let sprung = Input { jump: true, ..idle_input(0.0) };
    p.tick(&sprung, &world, &shapes);
    for _ in 0..FLY_TOGGLE_WINDOW {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    p.tick(&sprung, &world, &shapes);
    assert!(!p.abilities.flying);
    // Ohne Erlaubnis nie.
    let mut s = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    for i in 0..6 {
        s.tick(&Input { jump: i % 2 == 0, ..idle_input(0.0) }, &world, &shapes);
    }
    assert!(!s.abilities.flying);
}

#[test]
fn steigen_und_sinken_mit_dreifacher_fluggeschwindigkeit() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 20.0, 8.5));
    kreativ(&mut p);
    p.abilities.flying = true;
    let hoch = Input { jump: true, ..idle_input(0.0) };
    // v <- (v + 0.15) * 0.6 strebt gegen 0.225.
    for _ in 0..60 {
        p.tick(&hoch, &world, &shapes);
    }
    assert!((p.vel.y - 0.225).abs() < 1e-6, "{}", p.vel.y);
    let runter = Input { sneak: true, ..idle_input(0.0) };
    for _ in 0..60 {
        p.tick(&runter, &world, &shapes);
    }
    assert!((p.vel.y + 0.225).abs() < 1e-6, "{}", p.vel.y);
    assert!(!p.crouching, "fliegend schleicht man nicht");
}

#[test]
fn landen_beendet_den_flug() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 3.0, 8.5));
    kreativ(&mut p);
    p.abilities.flying = true;
    let runter = Input { sneak: true, ..idle_input(0.0) };
    for _ in 0..40 {
        p.tick(&runter, &world, &shapes);
        if !p.abilities.flying {
            break;
        }
    }
    assert!(!p.abilities.flying && p.on_ground && p.abilities_changed);
}

#[test]
fn horizontal_fliegend_schneller_und_sprintend_doppelt() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let tempo = |sprint: bool| {
        let mut p = PlayerPhysics::new(DVec3::new(0.5, 20.0, 0.5));
        kreativ(&mut p);
        p.abilities.flying = true;
        let vor = Input { forward: 1.0, sprint_held: sprint, ..idle_input(0.0) };
        for _ in 0..80 {
            p.tick(&vor, &world, &shapes);
        }
        p.vel.z
    };
    // Gleichgewicht: a * 0.98 * 0.91 / (1 - 0.91) mit a = 0.05 bzw. 0.1.
    let erwartet = 0.05 * 0.98 * 0.91 / 0.09;
    assert!((tempo(false) - erwartet).abs() < 1e-3, "{}", tempo(false));
    assert!((tempo(true) - 2.0 * erwartet).abs() < 2e-3, "{}", tempo(true));
}

#[test]
fn im_flug_startet_keine_elytra() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 20.0, 8.5));
    kreativ(&mut p);
    p.abilities.flying = true;
    p.tick(&gleit_input(false, true), &world, &shapes);
    p.tick(&gleit_input(true, true), &world, &shapes);
    assert!(!p.gliding && !p.glide_start_requested);
}

#[test]
fn zuschauer_fliegt_durch_den_boden() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 3.0, 8.5));
    p.abilities = Abilities { may_fly: true, invulnerable: true, ..Abilities::default() };
    let runter = Input { sneak: true, spectator: true, ..idle_input(0.0) };
    for _ in 0..40 {
        p.tick(&runter, &world, &shapes);
    }
    assert!(p.abilities.flying, "Zuschauer fliegen immer");
    assert!(p.pos.y < -3.0, "{}", p.pos.y);
    assert!(!p.on_ground);
}

fn gleit_input(jump: bool, elytra: bool) -> Input {
    Input { jump, glider_equipped: elytra, ..idle_input(0.0) }
}

// Fallen lassen, bis man sicher in der Luft ist (kein Bodenkontakt, Taste los).
fn in_der_luft(world: &World, shapes: &world::BlockShapes) -> PlayerPhysics {
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 40.0, 8.5));
    for _ in 0..3 {
        p.tick(&gleit_input(false, true), world, shapes);
    }
    assert!(!p.on_ground);
    p
}

#[test]
fn elytra_startet_nur_beim_druecken_in_der_luft_und_mit_elytra() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut ohne = in_der_luft(&world, &shapes);
    ohne.tick(&gleit_input(true, false), &world, &shapes);
    assert!(!ohne.gliding, "ohne Elytra kein Gleiten");

    let mut p = in_der_luft(&world, &shapes);
    p.tick(&gleit_input(true, true), &world, &shapes);
    assert!(p.gliding && p.glide_start_requested, "Drücken in der Luft startet den Flug");
    // Die Pose zieht am Tick-Ende nach: 0.6 hoch, Augen auf 0.4.
    assert!((p.current_height() - SWIMMING_HEIGHT_F64).abs() < 1e-9);

    // Gehaltene Taste startet nicht erneut (nach einer Server-Absage z.B.).
    p.gliding = false;
    p.glide_start_requested = false;
    p.tick(&gleit_input(true, true), &world, &shapes);
    assert!(!p.gliding, "Halten ist kein neuer Druck");

    // Am Boden nie.
    let mut boden = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    boden.refresh_on_ground(&world, &shapes);
    boden.tick(&gleit_input(true, true), &world, &shapes);
    assert!(!boden.gliding);
}

// Ein Tick Gleitformel, von Hand nach LivingEntity.calcGlidingVelocity gerechnet: Blick
// waagerecht nach +Z, Start mit 0.5 vorwaerts und 0.5 abwaerts.
#[test]
// Der Server beendet den Flug, die Pose steht aber noch einen Tick auf GLIDING: Vanilla
// zaehlt das als Kriechen und nimmt die Eingabe mal 0.3 (Grim-Flag beim Elytra-Hoppen).
fn erster_tick_nach_dem_gleiten_kriecht() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = in_der_luft(&world, &shapes);
    p.tick(&gleit_input(true, true), &world, &shapes);
    p.tick(&gleit_input(false, true), &world, &shapes);
    assert!(p.gliding && p.glide_pose);
    // Metadaten vom Server: Flug aus.
    p.gliding = false;
    let mut zwilling = PlayerPhysics::new(p.pos);
    zwilling.vel = p.vel;
    zwilling.fall_distance = p.fall_distance;
    let vor = Input { forward: 1.0, glider_equipped: true, ..idle_input(0.0) };
    let (v0, z0) = (p.vel, zwilling.vel);
    p.tick(&vor, &world, &shapes);
    zwilling.tick(&vor, &world, &shapes);
    let schub = |a: DVec3, b: DVec3| ((b.x - a.x * 0.91).powi(2) + (b.z - a.z * 0.91).powi(2)).sqrt();
    let verhaeltnis = schub(v0, p.vel) / schub(z0, zwilling.vel);
    assert!((verhaeltnis - 0.3).abs() < 1e-6, "Faktor war {verhaeltnis}");
    assert!(!p.glide_pose, "danach steht die Pose wieder");
}

// Serverwechsel mitten im Flug: der neue Spieler gleitet nicht und hat keinen Schwung, auch
// nicht nach dem Teleport, der Pose und Haltung uebernimmt.
#[test]
fn neuer_spieler_fliegt_nicht_weiter() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = in_der_luft(&world, &shapes);
    p.tick(&gleit_input(true, true), &world, &shapes);
    p.tick(&gleit_input(false, true), &world, &shapes);
    assert!(p.gliding && p.glide_pose);
    p.reset_for_new_player();
    let uebernommen = p.carry_over_state();
    let mut neu = PlayerPhysics::new(p.pos);
    neu.restore_carry_over_state(uebernommen);
    assert!(!neu.gliding && !neu.glide_pose && !neu.glide_start_requested);
    assert_eq!(p.vel, DVec3::ZERO);
}

#[test]
fn gleitformel_trifft_die_handrechnung() {
    let rot = rotation_vector(0.0, 0.0);
    assert_eq!(rot, DVec3::new(0.0, 0.0, 1.0));
    let v = calc_gliding_velocity(DVec3::new(0.0, -0.5, 0.5), rot, 0.0, GRAVITY);
    // y: -0.5 + 0.08*(-1+0.75) = -0.52; Umsetzung ins Vorwaerts: +0.052 -> -0.468.
    // z: 0.5 + 0.052 = 0.552, dann 10 % zurueck auf die alte Waagerechte 0.5 -> 0.5468.
    let erwartet = DVec3::new(0.0, -0.468 * f64::from(0.98f32), 0.5468 * f64::from(0.99f32));
    assert!((v - erwartet).length() < 1e-12, "{v:?} != {erwartet:?}");
}

#[test]
fn gleiten_traegt_weiter_als_fallen() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut frei = in_der_luft(&world, &shapes);
    let mut gleiter = in_der_luft(&world, &shapes);
    gleiter.tick(&gleit_input(true, true), &world, &shapes);
    frei.tick(&gleit_input(true, false), &world, &shapes);
    for _ in 0..30 {
        gleiter.tick(&gleit_input(false, true), &world, &shapes);
        frei.tick(&gleit_input(false, false), &world, &shapes);
    }
    assert!(gleiter.gliding);
    assert!(gleiter.pos.y > frei.pos.y + 5.0, "gleitend deutlich hoeher: {} vs {}", gleiter.pos.y, frei.pos.y);
    assert!(gleiter.pos.z > 8.5 + 3.0, "gleitend nach vorn getragen: z={}", gleiter.pos.z);
}

#[test]
fn rakete_zieht_auf_die_blickrichtung() {
    let rot = rotation_vector(0.0, 0.0);
    // Aus dem Stand: 0.1 Schub + halber Weg zu 1.5.
    let v = firework_boost(DVec3::ZERO, rot);
    assert!((v - DVec3::new(0.0, 0.0, 0.85)).length() < 1e-12);
    // Dauerschub ohne Luftwiderstand pendelt sich bei 1.7 ein.
    let mut v = DVec3::ZERO;
    for _ in 0..60 {
        v = firework_boost(v, rot);
    }
    assert!((v.z - 1.7).abs() < 1e-9);
}

#[test]
fn gleitend_haelt_keine_ranke_fest() {
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 5.0, 8.5));
    let mut world = world_with_floor(0);
    let vine = state_of_block("vine");
    world.set_block(8, 5, 8, vine);
    assert!(p.is_climbing(&world));
    p.gliding = true;
    assert!(!p.is_climbing(&world));
}

#[test]
fn faellt_und_landet_auf_dem_boden() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
    for _ in 0..200 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(p.on_ground, "sollte nach 10s Fallzeit auf dem Boden stehen");
    assert!((p.pos.y - 1.0).abs() < 1.0e-3, "Fuesse sollten exakt auf der Blockoberkante stehen, y={}", p.pos.y);
    // Kein exaktes 0.0: Vanilla haelt im Stand einen kleinen negativen "Rest" (Gravitation
    // minus Daempfung EINES Ticks), der beim naechsten Tick sofort wieder an der Kollision
    // abgeschnitten wird -- -0.0784 ist also der korrekte, vanilla-getreue Ruhewert.
    assert!((-0.1..=0.0).contains(&p.vel.y), "vel.y im Stand unplausibel: {}", p.vel.y);
}

#[test]
fn sprung_erreicht_plausible_vanilla_hoehe() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    // Erst ankommen lassen (on_ground = true).
    for _ in 0..5 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(p.on_ground);

    let mut input = idle_input(0.0);
    input.jump = true;
    let mut max_height = p.pos.y;
    for i in 0..40 {
        if i == 1 {
            input.jump = false; // nur einmal springen, nicht bei jedem Bodenkontakt erneut
        }
        p.tick(&input, &world, &shapes);
        max_height = max_height.max(p.pos.y);
    }
    // Bekannte Vanilla-Sprunghoehe: ca. 1.25 Bloecke.
    assert!((1.0..1.4).contains(&(max_height - 1.0)), "Sprunghöhe unplausibel: {}", max_height - 1.0);
    assert!(p.on_ground, "sollte nach dem Sprung wieder landen");
}

#[test]
fn laufen_naehert_sich_der_bekannten_vanilla_gehgeschwindigkeit() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    for _ in 0..100 {
        p.tick(&input, &world, &shapes); // bis zum Geschwindigkeits-Gleichgewicht laufen lassen
    }
    // Tatsaechliche Positionsaenderung messen, nicht self.vel: die gespeicherte
    // Geschwindigkeit ist bereits das Tick-Ende-Ergebnis NACH der Daempfung, waehrend
    // die Bewegung selbst mit der Geschwindigkeit VOR der Daempfung erfolgt (s.
    // tick()) -- self.vel unterschaetzt die reale Schrittweite um genau diesen Faktor.
    let before = p.pos;
    p.tick(&input, &world, &shapes);
    let speed_per_second = (p.pos - before).length() / f64::from(TICK_DT);
    // Bekannte Vanilla-Gehgeschwindigkeit: ca. 4.3 Bloecke/s.
    assert!((4.0..4.6).contains(&speed_per_second), "Gehgeschwindigkeit unplausibel: {speed_per_second}");
}

// Vanillas Stopp-Bedingungen fragen die SPRINT-TASTE nicht ab
// (shouldStopSprinting/shouldStopSwimSprinting). Ein laufender Sprint ueberlebt das
// Loslassen -- vorher endete er bei uns sofort.
#[test]
fn losgelassene_sprinttaste_beendet_den_sprint_nicht() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    p.tick(&input, &world, &shapes);
    assert!(p.sprinting, "mit gedrückter Taste muss der Sprint starten");

    input.sprint_held = false;
    for _ in 0..20 {
        p.tick(&input, &world, &shapes);
        assert!(p.sprinting, "ohne Taste, aber mit Vorwärtseingabe läuft der Sprint weiter");
    }
    // Ohne Vorwaertseingabe endet er -- das ist Vanillas `!hasForwardMovement`.
    input.forward = 0.0;
    p.tick(&input, &world, &shapes);
    assert!(!p.sprinting, "ohne Vorwärtseingabe endet der Sprint");
}

// canSprintOrFly() -> HungerManager.canSprint() -> foodLevel > 6. Bei 6 ist Schluss,
// und zwar sowohl fuers Starten als auch fuer den laufenden Sprint.
#[test]
fn hunger_sperrt_den_sprint_ab_sechs() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;

    input.food_level = 7;
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    p.tick(&input, &world, &shapes);
    assert!(p.sprinting, "7 ist noch über der Grenze");

    // Waehrend des Laufens leer werden beendet den Sprint.
    input.food_level = 6;
    p.tick(&input, &world, &shapes);
    assert!(!p.sprinting, "bei 6 endet auch ein laufender Sprint");

    let mut p2 = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    p2.tick(&input, &world, &shapes);
    assert!(!p2.sprinting, "bei 6 laesst sich gar nicht erst starten");
}

// canSprint() faengt mit !hasBlindnessEffect() an -- blind geht nichts.
#[test]
fn blindheit_sperrt_den_sprint() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    p.tick(&input, &world, &shapes);
    assert!(p.sprinting);
    input.blindness = true;
    p.tick(&input, &world, &shapes);
    assert!(!p.sprinting, "Blindheit beendet auch einen laufenden Sprint");
}

// isBlockedFromSprinting(): eine laufende Item-Benutzung sperrt den START
// (UseEffectsComponent.DEFAULT hat can_sprint = false). Ein LAUFENDER Sprint bleibt
// davon unberuehrt -- shouldStopSprinting() kennt die Bedingung nicht.
#[test]
fn item_benutzung_sperrt_nur_den_sprintstart() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    input.using_item = true;
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    p.tick(&input, &world, &shapes);
    assert!(!p.sprinting, "während einer Item-Benutzung startet kein Sprint");

    input.using_item = false;
    p.tick(&input, &world, &shapes);
    assert!(p.sprinting);
    input.using_item = true;
    p.tick(&input, &world, &shapes);
    assert!(p.sprinting, "ein laufender Sprint überlebt den Beginn einer Benutzung");
}

#[test]
fn sprinten_ist_schneller_als_gehen() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut walk = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut sprint = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut walk_input = idle_input(0.0);
    walk_input.forward = 1.0;
    let mut sprint_input = walk_input;
    sprint_input.sprint_held = true;
    for _ in 0..100 {
        walk.tick(&walk_input, &world, &shapes);
        sprint.tick(&sprint_input, &world, &shapes);
    }
    let walk_speed = (walk.vel.x * walk.vel.x + walk.vel.z * walk.vel.z).sqrt();
    let sprint_speed = (sprint.vel.x * sprint.vel.x + sprint.vel.z * sprint.vel.z).sqrt();
    assert!(sprint_speed > walk_speed * 1.15, "Sprint ({sprint_speed}) sollte spürbar schneller sein als Gehen ({walk_speed})");
}

// Gegenstueck zum Loslass-Test: die Verlangsamung laggt der Taste in BEIDE Richtungen um
// einen Tick nach. GrimACs PlayerBaseTick liest dafuer `wasSneaking`, also die Taste vom
// letzten Tick -- genau wie Vanilla, das inSneakingPose VOR input.tick() setzt.
//
// Ich hatte hier zwischenzeitlich "greift sofort beim Druecken" stehen, aus einer Messung
// geraten, die eine andere Ursache hatte. Der Test steht jetzt auf der Formel aus Grims
// Quelltext statt auf einer Vermutung.
#[test]
fn sneak_verlangsamung_laggt_einen_tick_beim_druecken() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut gehen = idle_input(0.0);
    gehen.forward = 1.0;
    for _ in 0..40 {
        p.tick(&gehen, &world, &shapes);
    }
    let mut mit_sneak = gehen;
    mit_sneak.sneak = true;
    p.tick(&mit_sneak, &world, &shapes);
    assert!(!p.in_sneaking_pose_move, "Druck-Tick rechnet noch OHNE Verlangsamung (wasSneaking war false)");
    p.tick(&mit_sneak, &world, &shapes);
    assert!(p.in_sneaking_pose_move, "ab dem Tick danach greift sie");
}

// Ohne gedrueckte Taste, aber unter zu niedriger Decke: dort erzwingt die Formel die
// Schleich-Pose SOFORT -- der Zweig `!canEnterPose(STANDING)` kennt keinen Nachlauf.
#[test]
fn niedrige_decke_verlangsamt_ohne_taste_und_ohne_nachlauf() {
    let (world, shapes, pos) = welt_der_schleich_luecke();
    let mut p = PlayerPhysics::new(pos);
    p.refresh_on_ground(&world, &shapes);
    let mut ohne = idle_input(181.0f32.to_radians());
    ohne.forward = 1.0;
    p.tick(&ohne, &world, &shapes);
    assert!(!pose_fits(p.pos, HEIGHT_F64, &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default()), "Test setzt die niedrige Decke voraus");
    assert!(p.in_sneaking_pose_move, "unter der Decke gilt die Verlangsamung ab dem ersten Tick");
}

// Ein Server-Teleport (Setback) darf den Tastenzustand nicht vergessen -- sonst sieht der
// Tick danach wie ein frischer Tastendruck aus und die Verlangsamung setzt einen Tick
// lang aus. Genau daraus wurde im Mitschnitt eine Setback-Endlosschleife.
// Der Tick direkt nach einem Teleport/Setback: dort ist die Geschwindigkeit genullt, die
// angefragte Bewegung also y = 0 -- und Vanilla setzt onGround dann auf FALSE (Entity.move
// Zeile 762-765: der Zweig laeuft fuer den eigenen Spieler immer, groundCollision verlangt
// movement.y < 0). Der Folgetick rechnet damit mit LUFT-Beschleunigung.
//
// Genau das war die Ursache der Simulation-Flags (0.009800 / 0.005351 /
// 0.002922, Zerfall 0.546 = Bodenreibung): wir behielten den Bodenkontakt, beschleunigten
// mit dem Bodenwert und lagen damit einmalig 0.98 × (0.1 × 0.216/0.546³ − 0.02) = 0.1104
// ueber Grims Vorhersage. Weil jeder Flag einen Setback ausloest und jeder Setback diesen
// Tick erneut erzeugt, hielt sich die Schleife selbst am Leben.
#[test]
fn erster_tick_nach_teleport_meldet_keinen_bodenkontakt() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    // So sieht der Zustand direkt nach einem Server-Teleport aus (s. main.rs: neues
    // PlayerPhysics + refresh_on_ground).
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    p.refresh_on_ground(&world, &shapes);
    assert!(p.on_ground, "refresh_on_ground sieht den Boden unter den Füßen");
    assert_eq!(p.vel, DVec3::ZERO, "ein Teleport nullt die Geschwindigkeit");

    let mut lauf = idle_input(0.0);
    lauf.forward = 1.0;
    p.tick(&lauf, &world, &shapes);
    assert!(!p.on_ground, "movement.y == 0 -> kein Bodenkontakt, genau wie in Vanilla");

    // Und der Folgetick beschleunigt deshalb mit dem LUFT-Wert. Ohne diesen Tick waere die
    // Beschleunigung um rund 0.11 hoeher -- die gemessene Differenz zu Grim.
    let vor = p.vel.z;
    p.tick(&lauf, &world, &shapes);
    let zugewachsen = p.vel.z / 0.91 - vor; // 0.91 = Luftwiderstand, in der Luft ohne Bodenreibung
    let luft = 0.98 * AIR_ACCEL;
    assert!((zugewachsen - luft).abs() < 1.0e-6, "Luft-Beschleunigung erwartet ({luft}), war {zugewachsen}");

    // Danach ist der Bodenkontakt wieder da (die Schwerkraft hat vel.y auf -0.0784 gezogen).
    assert!(p.on_ground, "im nächsten Tick steht er wieder");
}

// Nach einer Enderperle teleportiert der Server uns mitten in die Luft. Vanilla laesst
// onGround dabei unveraendert (setValuesFromPositionPacket fasst es nicht an), der
// naechste Tick rechnet also noch mit BODENreibung 0.546 -- nicht mit Luft 0.91.
//
// Genau das zeigte GrimAC (consoledebug): ein einziger Tick daneben,
// Offset 0.06461471, danach sauberer Zerfall mit 0.546. Der Betrag ist die waagerechte
// Bewegung des Ticks davor mal der Reibungsdifferenz:
//     0.177512968 * (0.91 - 0.546) = 0.06461472
#[test]
fn ein_teleport_laesst_den_bodenkontakt_stehen() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut laufen = idle_input(0.0);
    laufen.forward = 1.0;
    laufen.sprint_held = true;
    for _ in 0..20 {
        p.tick(&laufen, &world, &shapes);
    }
    assert!(p.on_ground, "nach zwanzig Lauf-Ticks steht er");
    let vorher = DVec3::new(p.vel.x, 0.0, p.vel.z).length();

    // So macht es der PlayerPosition-Handler in main.rs: Ziel hoch in der Luft, der
    // eigene Schwung bleibt (Enderperle, alle DELTA-Bits relativ).
    let gerettet = p.carry_over_state();
    let schwung = p.vel;
    let mut nach_perle = PlayerPhysics::new(DVec3::new(40.0, 8.0, 40.0));
    nach_perle.restore_carry_over_state(gerettet);
    nach_perle.vel = schwung;
    assert!(nach_perle.on_ground, "der Teleport darf den Bodenkontakt nicht löschen");

    // Der erste Tick nach dem Teleport traegt die waagerechte Geschwindigkeit mit der
    // BODENreibung weiter. Ohne Eingabe kommt keine Beschleunigung dazu, der Faktor ist
    // damit direkt messbar: Vanilla nimmt f * 0.91 mit f = 0.6, also 0.546.
    let mut ohne_eingabe = idle_input(0.0);
    ohne_eingabe.forward = 0.0;
    ohne_eingabe.sprint_held = false;
    nach_perle.tick(&ohne_eingabe, &world, &shapes);
    let danach = DVec3::new(nach_perle.vel.x, 0.0, nach_perle.vel.z).length();
    let faktor = danach / vorher;
    assert!(
        (faktor - 0.546).abs() < 1.0e-6,
        "nach dem Teleport muss die Bodenreibung 0.546 greifen, war {faktor} (Luft wäre 0.91)"
    );
}

#[test]
fn setback_vergisst_die_sneak_taste_nicht() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut mit_sneak = idle_input(0.0);
    mit_sneak.forward = 1.0;
    mit_sneak.sneak = true;
    for _ in 0..10 {
        p.tick(&mit_sneak, &world, &shapes);
    }
    assert!(p.carry_over_state().prev_sneak, "Sneak lief, der Zustand muss gesetzt sein");
    assert!(p.crouching, "nach zehn Sneak-Ticks ist die Schleich-Pose gesetzt");

    // So macht es der PlayerPosition-Handler in main.rs.
    let gerettet = p.carry_over_state();
    let mut nach_setback = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    nach_setback.restore_carry_over_state(gerettet);
    // Auch die Pose gehoert zum geretteten Zustand: Grim behaelt seine ueber den Teleport
    // hinweg, ein Reset auf Stehen waere direkt nach dem Setback wieder ein Hitbox-Versatz.
    assert!(nach_setback.crouching, "der Setback lässt niemanden aufstehen");
    nach_setback.refresh_on_ground(&world, &shapes);
    nach_setback.tick(&mit_sneak, &world, &shapes);
    assert!(nach_setback.in_sneaking_pose_move, "direkt nach dem Setback muss weiter geschlichen werden");
}

// Vanillas applyMovementSpeedFactors multipliziert waehrend einer Item-Benutzung
// zusaetzlich mit 0.2 (UseEffectsComponent.DEFAULT.speedMultiplier). Das fehlte, und
// GrimAC rechnete es mit -- im consoledebug als Verhaeltnis 0.3/0.2 = 1.5 messbar
// (unsere Beschleunigung 0.03823018 gegen Grims 0.02549019).
#[test]
fn item_benutzung_daempft_die_eingabe_auf_ein_fuenftel() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let schritt = |using_item: bool| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut i = idle_input(0.0);
        i.forward = 1.0;
        i.using_item = using_item;
        for _ in 0..40 {
            p.tick(&i, &world, &shapes);
        }
        let vor = p.pos;
        p.tick(&i, &world, &shapes);
        let d = p.pos - vor;
        (d.x * d.x + d.z * d.z).sqrt()
    };
    let verhaeltnis = schritt(true) / schritt(false);
    assert!(
        (verhaeltnis - ITEM_USE_SPEED_MULT).abs() < 1.0e-6,
        "Benutzung muss exakt {ITEM_USE_SPEED_MULT} der normalen Geschwindigkeit sein, war {verhaeltnis}"
    );
}

// Und die beiden Faktoren multiplizieren sich, genau wie in applyMovementSpeedFactors.
#[test]
fn schleichen_und_benutzen_multiplizieren_sich() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let schritt = |sneak: bool, using_item: bool| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut i = idle_input(0.0);
        i.forward = 1.0;
        i.sneak = sneak;
        i.using_item = using_item;
        for _ in 0..40 {
            p.tick(&i, &world, &shapes);
        }
        let vor = p.pos;
        p.tick(&i, &world, &shapes);
        let d = p.pos - vor;
        (d.x * d.x + d.z * d.z).sqrt()
    };
    let verhaeltnis = schritt(true, true) / schritt(false, false);
    let erwartet = DEFAULT_SNEAKING_SPEED * ITEM_USE_SPEED_MULT;
    assert!((verhaeltnis - erwartet).abs() < 1.0e-6, "erwartet {erwartet}, war {verhaeltnis}");
}

// Grims SprintE ("grim.sprint.wall", Sprinting while colliding with a wall) flaggt,
// wenn der Spieler im Tick NACH einer HARTEN Horizontalkollision immer noch als
// sprintend gesendet wird. Vanilla beendet den Sprint dort (ClientPlayerEntity.
// shouldStopRunSprinting: horizontalCollision && !minorHorizontalCollision).
//
// Massgeblich ist die HARTE Kollision, nicht jede: der allererste Beruehrungs-Tick
// beschneidet die Bewegung oft nur um einen Bruchteil (knapp ueber der 1e-5-Schwelle von
// approximatelyEquals). Die zurueckgelegte Richtung bleibt dabei praktisch die gewollte,
// Vanilla wertet das als Streifen und sprintet weiter -- und Grim rechnet genauso
// (wasHardHorizontalCollision = horizontalCollision && !softHorizontalCollision).
// Der frueher hier stehende Anspruch "nach der ERSTEN Kollision nie wieder sprinten" war
// strenger als Vanilla und ist genau die Abweichung, die den Sprint aufblinken liess.
#[test]
fn sprint_endet_an_der_wand() {
    let world = world_with_floor_and_wall(0, 10);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.strafe = 1.0; // yaw 0: +X, auf die Wand bei x=10 zu
    input.forward = 1.0;
    input.sprint_held = true;

    let mut sprint_nach_hartem_anprall = 0;
    let mut prallte_hart_an = false;
    for _ in 0..80 {
        p.tick(&input, &world, &shapes);
        if prallte_hart_an && p.sprinting {
            sprint_nach_hartem_anprall += 1;
        }
        prallte_hart_an |= p.horizontal_collision && !p.minor_horizontal_collision;
    }
    assert!(prallte_hart_an, "Test setzt voraus, dass die Wand überhaupt hart getroffen wird");
    assert_eq!(sprint_nach_hartem_anprall, 0, "nach dem harten Anprall darf kein Tick mehr sprintend gesendet werden");
}

// Die Gegenprobe zum Test darueber: an einer Wand ENTLANG (Blick 5 Grad in die Wand
// hinein, Vorwaertsdrang) bleibt der Sprint stehen. Genau das konnte unsere alte Fassung
// nicht -- sie brach bei jedem Wandkontakt ab, startete bei gehaltener Taste im naechsten
// Tick neu, und dieses Aufblinken war der SprintE-Flag im Kaktus-Labyrinth.
#[test]
fn an_der_wand_entlang_bleibt_der_sprint_stehen() {
    let world = world_with_floor_and_wall(0, 10);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    // Blick fast parallel zur Wand (Wand liegt bei x=10, wir laufen nach +Z), nur 5 Grad
    // in sie hinein -- unterhalb von Vanillas 8-Grad-Grenze. Vorzeichen: bei yaw 0 zeigt
    // der Vorwaertsdrang nach +Z, und vel.x = -sin(yaw) -- fuer +X (zur Wand hin) muss
    // yaw also NEGATIV sein.
    let mut input = idle_input(-5.0f32.to_radians());
    input.forward = 1.0;
    input.sprint_held = true;

    // Erst mal an die Wand heranlaufen.
    for _ in 0..120 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.horizontal_collision, "Testaufbau: wir müssen an der Wand anliegen");
    assert!(p.minor_horizontal_collision, "5 Grad in die Wand ist ein Streifen, kein Anprall");
    assert!(p.sprinting, "beim Entlangstreifen darf der Sprint nicht abbrechen");
}

// Auf den eigenen, noch unbestaetigten Block muss man sofort weiterbauen koennen -- der
// Strahl sieht die Vorhersage-Schicht, genau wie Vanillas Strahl seine ClientWorld sieht
// (s. Kommentar ueber raycast_block: hier stand zwischenzeitlich server_block_at, was das
// Weiterbauen bis zur Bestaetigung blockierte).
#[test]
fn strahl_trifft_den_eigenen_vorhergesagten_block() {
    let mut world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let eye = DVec3::new(8.5, 3.0, 8.5);
    let nach_unten = DVec3::new(0.0, -1.0, 0.0);

    // Ohne Vorhersage trifft der Strahl den Boden bei y=0.
    let boden = raycast_block(eye, nach_unten, 4.5, &world, &shapes).expect("Boden");
    assert_eq!(boden.block, (8, 0, 8));

    // Jetzt ein vorhergesagter Block direkt darueber: DEN muss der Strahl treffen, und
    // zwar auf der Oberseite -- sonst landet der naechste Block in seiner Position statt
    // darauf.
    let stein = (0..world::block_registry::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).expect("Stein");
    world.set_pending_block(8, 1, 8, stein);
    let geist = raycast_block(eye, nach_unten, 4.5, &world, &shapes).expect("Geisterblock");
    assert_eq!(geist.block, (8, 1, 8), "der vorhergesagte Block liegt vor dem Boden");
    assert_eq!(geist.face, protocol::packets::FACE_UP);
}

// Rechtsklick-Strahl: Trefferseite und Trefferstelle sind das, was im Paket landet --
// eine falsche Seite platziert den Block auf der falschen Wuerfelflaeche.
#[test]
fn strahl_von_oben_trifft_die_oberseite() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    // Auge ueber der Bodenplatte bei y=0 (also Oberkante y=1), Blick gerade nach unten.
    let eye = DVec3::new(8.25, 3.0, 8.75);
    let hit = raycast_block(eye, DVec3::new(0.0, -1.0, 0.0), 4.5, &world, &shapes).expect("Boden muss getroffen werden");
    assert_eq!(hit.block, (8, 0, 8));
    assert_eq!(hit.face, protocol::packets::FACE_UP, "von oben kommend ist es die Oberseite");
    assert!((hit.cursor.1 - 1.0).abs() < 1.0e-5, "Oberkante des Blocks: {:?}", hit.cursor);
    // Die waagerechte Trefferstelle ist der Bruchteil innerhalb des Blocks.
    assert!((hit.cursor.0 - 0.25).abs() < 1.0e-5, "{:?}", hit.cursor);
    assert!((hit.cursor.2 - 0.75).abs() < 1.0e-5, "{:?}", hit.cursor);
    assert!(!hit.inside);
}

#[test]
fn strahl_von_der_seite_trifft_die_seitenflaeche() {
    let world = world_with_floor_and_wall(0, 10);
    let shapes = world::BlockShapes::all_full_cubes();
    // Auge links der Wand bei x=10, Blick in +X.
    let eye = DVec3::new(8.0, 2.0, 8.5);
    let hit = raycast_block(eye, DVec3::new(1.0, 0.0, 0.0), 4.5, &world, &shapes).expect("Wand muss getroffen werden");
    assert_eq!(hit.block, (10, 2, 8));
    assert_eq!(hit.face, protocol::packets::FACE_WEST, "in +X laufend trifft man die Westseite");
    assert!((hit.cursor.0 - 0.0).abs() < 1.0e-5, "genau auf der Blockkante: {:?}", hit.cursor);
}

#[test]
fn strahl_ins_leere_trifft_nichts() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let eye = DVec3::new(8.0, 20.0, 8.0);
    assert!(raycast_block(eye, DVec3::new(0.0, 1.0, 0.0), 4.5, &world, &shapes).is_none());
    // Und der Boden liegt ausserhalb der Reichweite, obwohl der Strahl dorthin zeigt.
    assert!(raycast_block(eye, DVec3::new(0.0, -1.0, 0.0), 4.5, &world, &shapes).is_none());
}

// Die geflaggte Stelle aus Grims consoledebug ("test sneak", 2026-08-05), genau aus dessen
// Blockdump nachgebaut:
//   y=-11, x=7,   z=8 : oak_slab[type=top]   -> Decke der Luecke, belegt -10.5 .. -10.0
//   y=-12, x=6/8, z=8 : dirt                 -> die Wangen daneben
//   y=-13             : Boden
// Unter der Stufe bleiben von -12 bis -10.5 GENAU 1.5 Bloecke -- die Schleich-Hitbox.
// Grim rechnete dort mit Sneak-Faktor (Simulated input 0.0294), unser Client ohne
// (Client input 0.098) -> Offset 0.0686, Setback-Schleife.
fn welt_der_schleich_luecke() -> (World, world::BlockShapes, DVec3) {
    let state_von = |name: &str, props: &[(&str, &str)]| -> u32 {
        (0..world::block_registry::state_count() as u32)
            .find(|&s| world::block_name(s) == Some(name) && props.iter().all(|(k, v)| world::state_props::property(s, k) == Some(v)))
            .unwrap_or_else(|| panic!("Blockstate {name}{props:?} fehlt"))
    };
    let dirt = state_von("dirt", &[]);
    let slab = state_von("oak_slab", &[("type", "top")]);

    // Nur die obere Stufe braucht eine echte Form; alles andere ist Vollwuerfel (None).
    let mut per_state: Vec<Option<Vec<world::ShapeBox>>> = vec![None; world::block_registry::state_count()];
    per_state[slab as usize] = Some(vec![world::ShapeBox {
        min: [0.0, 0.5, 0.0],
        max: [1.0, 1.0, 1.0],
        faces: [true; 6],
        content: [false; 6],
        uv: [None; 6],
    }]);
    let shapes = world::BlockShapes::new(per_state);

    let setze = |states: &mut [u32], x: i32, y: i32, z: i32, state: u32| {
        let local_y = (y - world::MIN_Y).rem_euclid(16);
        states[((local_y * 16 + z) * 16 + x) as usize] = state;
    };
    let mut w = World::new();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == (-13 - world::MIN_Y).div_euclid(16) {
                    for x in 0..16 {
                        for z in 0..16 {
                            setze(&mut states, x, -13, z, dirt);
                        }
                    }
                    if cx == 0 && cz == 0 {
                        setze(&mut states, 6, -12, 8, dirt);
                        setze(&mut states, 8, -12, 8, dirt);
                        setze(&mut states, 6, -11, 8, dirt);
                        setze(&mut states, 8, -11, 8, dirt);
                        setze(&mut states, 7, -11, 8, slab);
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    (w, shapes, DVec3::new(7.473631490959559, -12.0, 9.202014757858826))
}

// Der ZEITPUNKT des Posenwechsels (sonst flaggt Schleichen direkt in einen
// 1.5-Bloecke-Gang). Vanilla ruft updatePose() als letzte
// Anweisung von PlayerEntity.tick(), GrimAC genauso (MovementCheckRunner: doBaseTick ->
// livingEntityAIStep -> updatePlayerPose). Die Hitbox eines Ticks ist damit IMMER die Pose
// vom Ende des VORIGEN Ticks -- auf dem Tick, an dem Sneak gedrueckt wird, gilt also noch
// die Stehbox. Vorher aktualisierten wir die Pose am Tick-ANFANG und schlupften dadurch
// einen Tick zu frueh unter die Stufe, waehrend Grims Vorhersage dort noch anstiess.
#[test]
fn posenwechsel_gilt_erst_ab_dem_naechsten_tick() {
    let (world, shapes, pos) = welt_der_schleich_luecke();
    // Knapp VOR der Luecke: die Stufe steht bei z=8 (Zelle 8.0..9.0), die Spielerbox ist
    // 0.6 breit -- ab z < 9.3 ragt sie unter die Stufe.
    const KANTE: f64 = 9.3;
    let mut p = PlayerPhysics::new(DVec3::new(pos.x, pos.y, 9.35));
    p.refresh_on_ground(&world, &shapes);
    let mut i = idle_input(181.0f32.to_radians());
    i.forward = 1.0;
    i.sneak = true;

    // Erster Tick MIT gedruecktem Sneak: die Pose ist noch STANDING, die Stufe blockt.
    p.tick(&i, &world, &shapes);
    assert!(p.pos.z >= KANTE - 1.0e-9, "am Sneak-Tick gilt noch die Stehbox, die Stufe muss blocken -- z={} (Kante {KANTE})", p.pos.z);
    // Aber die Pose steht am ENDE dieses Ticks bereits auf Schleichen.
    assert!(p.crouching, "am Tick-Ende ist die Schleich-Pose gesetzt");

    // Ab dem naechsten Tick passt die Schleichbox unter die Stufe -- sonst waere der Test
    // oben auch dann gruen, wenn wir gar nicht mehr hineinkaemen.
    for _ in 0..20 {
        p.tick(&i, &world, &shapes);
    }
    assert!(p.pos.z < KANTE, "schleichend muss man unter die Stufe kommen -- z={}", p.pos.z);
}

// Der Kern des Mitschnitts: unter der Stufe MUSS der Sneak-Faktor greifen, sonst laufen
// wir mit 0.098 los, waehrend Grim 0.0294 erwartet.
#[test]
fn in_der_schleich_luecke_greift_der_sneak_faktor() {
    let (world, shapes, pos) = welt_der_schleich_luecke();
    let mut p = PlayerPhysics::new(pos);
    p.refresh_on_ground(&world, &shapes);
    let mut i = idle_input(181.0f32.to_radians());
    i.forward = 1.0;
    i.sneak = true;
    p.tick(&i, &world, &shapes);
    assert!(
        p.in_sneaking_pose_move,
        "unter der oberen Stufe (1.5 Bloecke Luft) muss die Schleich-Pose greifen -- crouch={} swim={} passt1.8={} passt1.5={}",
        p.crouching,
        p.swimming,
        pose_fits(p.pos, HEIGHT_F64, &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default()),
        pose_fits(p.pos, CROUCHING_HEIGHT_F64, &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default())
    );

    // Und OHNE Sneak-Taste ebenso: unter einer zu niedrigen Decke erzwingt Vanilla die
    // Schleich-Pose von sich aus (inSneakingPose = ... || !canChangeIntoPose(STANDING)),
    // der ×0.3 gilt dann auch ungedrueckt. Ein frisch gebautes PlayerPhysics ist zugleich
    // der Zustand direkt nach einem Server-Setback -- auch dort muss es sofort stimmen.
    let mut q = PlayerPhysics::new(pos);
    q.refresh_on_ground(&world, &shapes);
    let mut ohne = idle_input(181.0f32.to_radians());
    ohne.forward = 1.0;
    q.tick(&ohne, &world, &shapes);
    assert!(q.in_sneaking_pose_move, "unter der Stufe gilt der Schleich-Faktor auch ohne gedrückte Taste");
}

// Regressionstest fuer den grossen Crouch->Sprint-Uebergangs-Flag (s. "test 13",
// Offset 0.089): Vanilla setzt das isInSneakingPose()-FELD (shouldSlowDown()) in
// ClientPlayerEntity.tickMovement() VOR input.tick(), die Bewegungs-Verlangsamung
// (×0.3) laggt der Sneak-Taste also um genau EINEN Tick nach. Auf dem Tick, an dem Sneak
// losgelassen wird, muss die Verlangsamung darum noch greifen -- der Client darf NICHT
// schon im selben Tick auf volle Geschwindigkeit (0.98) springen. Vorher tat er genau das
// und ueberschoss Grims Simulation massiv.
// Grim rechnet mit GENAU dem Faktor 0.3 auf dem Bewegungs-Input (EntityAttributes
// SNEAKING_SPEED). Aus einem echten consoledebug-Mitschnitt ("test sneak"):
//   Client input:    0.09800001233816147
//   Simulated input: 0.02940000407397747   = 0.098 * 0.3
// Weil die Beschleunigung linear in den Input eingeht, muss sich das Verhaeltnis 1:1 in
// der Schrittweite im Gleichgewicht wiederfinden -- ein Test, der die Zahl festnagelt,
// statt nur einen Bereich abzustecken.
// Schleichender Schritt: der Faktor ist das SNEAKING_SPEED-Attribut, kein fester Wert.
// Mit Stufe III (0.3 + 3 x 0.15 = 0.75) muss die Schrittweite genau 0.75 der
// Gehgeschwindigkeit sein -- rechnete der Client weiter mit 0.3, waere er in jedem
// Schleich-Tick weniger als halb so schnell wie der Server (und wie GrimAC) erwartet.
#[test]
fn schleichtempo_folgt_dem_attribut() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let schritt = |sneak: bool, sneaking_speed: f64| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut i = idle_input(0.0);
        i.forward = 1.0;
        i.sneak = sneak;
        i.sneaking_speed = sneaking_speed;
        for _ in 0..40 {
            p.tick(&i, &world, &shapes);
        }
        let before = p.pos;
        p.tick(&i, &world, &shapes);
        let d = p.pos - before;
        (d.x * d.x + d.z * d.z).sqrt()
    };
    let gehen = schritt(false, DEFAULT_SNEAKING_SPEED);
    let verhaeltnis = schritt(true, 0.75) / gehen;
    assert!((verhaeltnis - 0.75).abs() < 1.0e-6, "Schleichender Schritt III = 0.75, war {verhaeltnis}");
}

// Grundtempo aus dem Attribut-Paket (Plugin-Walkspeed): der Client muss damit rechnen,
// nicht mit den fest verdrahteten 0.1. Ein Server, der das Tempo verdoppelt, erwartet
// auch die doppelte Schrittweite.
#[test]
fn grundtempo_folgt_dem_attribut() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let schritt = |tempo: f64| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut i = idle_input(0.0);
        i.forward = 1.0;
        i.movement_speed = tempo;
        for _ in 0..60 {
            p.tick(&i, &world, &shapes);
        }
        let before = p.pos;
        p.tick(&i, &world, &shapes);
        let d = p.pos - before;
        (d.x * d.x + d.z * d.z).sqrt()
    };
    let verhaeltnis = schritt(BASE_SPEED * 2.0) / schritt(BASE_SPEED);
    assert!((verhaeltnis - 2.0).abs() < 1.0e-3, "doppeltes Grundtempo = doppelte Schrittweite, war {verhaeltnis}");
}

#[test]
fn schleichen_ist_exakt_drei_zehntel_der_gehgeschwindigkeit() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let schritt = |sneak: bool| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
        let mut i = idle_input(0.0);
        i.forward = 1.0;
        i.sneak = sneak;
        for _ in 0..40 {
            p.tick(&i, &world, &shapes);
        }
        let before = p.pos;
        p.tick(&i, &world, &shapes);
        let d = p.pos - before;
        assert!(p.on_ground, "Messung nur im Gleichgewicht am Boden gültig");
        (d.x * d.x + d.z * d.z).sqrt()
    };
    let verhaeltnis = schritt(true) / schritt(false);
    assert!(
        (verhaeltnis - DEFAULT_SNEAKING_SPEED).abs() < 1.0e-6,
        "Schleichen muss exakt {DEFAULT_SNEAKING_SPEED} der Gehgeschwindigkeit sein, war {verhaeltnis}"
    );
}

#[test]
fn sneak_verlangsamung_laggt_einen_tick_beim_loslassen() {
    let world = world_with_floor(0);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut sneak_input = idle_input(0.0);
    sneak_input.forward = 1.0;
    sneak_input.sneak = true;
    // Ins Schleich-Gleichgewicht laufen (~0.065 Bloecke/Tick Schrittweite).
    for _ in 0..80 {
        p.tick(&sneak_input, &world, &shapes);
    }
    // Schrittweite (Positionsdelta) messen, nicht self.vel: self.vel ist schon der
    // gedaempfte Tick-Ende-Wert, die Bewegung selbst (und damit das, was Grim sieht)
    // laeuft mit der Geschwindigkeit VOR der Daempfung -- s. laufen_naehert_sich_...
    let before = p.pos;
    p.tick(&sneak_input, &world, &shapes);
    let sneak_step = (p.pos - before).length();
    assert!((0.05..0.08).contains(&sneak_step), "Schleich-Schrittweite unplausibel: {sneak_step}");

    // Tick 1 nach dem Loslassen: Verlangsamung laggt -> Schrittweite bleibt auf
    // Schleich-Niveau, springt NICHT auf ~0.13 (das waere der Bug ohne Lag).
    let mut release_input = sneak_input;
    release_input.sneak = false;
    let before = p.pos;
    p.tick(&release_input, &world, &shapes);
    let release_step = (p.pos - before).length();
    assert!(
        release_step < 0.09,
        "auf dem Loslass-Tick muss die Sneak-Verlangsamung noch greifen (erwartet ~0.065), war {release_step}"
    );

    // Tick 2: jetzt ist die Verlangsamung weg, Schrittweite klettert Richtung Geh-Terminal.
    let before = p.pos;
    p.tick(&release_input, &world, &shapes);
    let next_step = (p.pos - before).length();
    assert!(next_step > 0.11, "einen Tick nach dem Loslassen muss die volle Geschwindigkeit greifen, war {next_step}");
}

// Baut world_with_floor() zusaetzlich eine 3 Bloecke hohe Wand bei Welt-X=10 ein --
// in EINEM Rutsch, denn World::insert_column() ERSETZT eine Spalte komplett statt sie
// zu mischen; ein zweiter Aufruf fuer dieselbe Spalte wuerde den zuvor gesetzten Boden
// wieder loeschen.
fn world_with_floor_and_wall(floor_y: i32, wall_x: i32) -> World {
    let mut w = World::new();
    let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
    let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
    let wall_bottom = floor_y + 1;
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == floor_section {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                        }
                    }
                }
                let section_base_y = world::MIN_Y + i * 16;
                for wy in wall_bottom..wall_bottom + 3 {
                    if wy >= section_base_y && wy < section_base_y + 16 {
                        let local_x = wall_x.rem_euclid(16);
                        if wall_x.div_euclid(16) == cx {
                            let ly = wy - section_base_y;
                            for z in 0..16 {
                                states[((ly * 16 + z) * 16 + local_x) as usize] = 1;
                            }
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

#[test]
fn wand_blockiert_horizontale_bewegung() {
    // Wand direkt vor dem Spieler in +X-Richtung, bei Welt-X=10.
    let world = world_with_floor_and_wall(0, 10);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0); // yaw 0 = Blick nach +Z, wir wollen +X -> strafe
    input.strafe = 1.0;
    for _ in 0..200 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.pos.x <= 9.7 + 1.0e-3, "sollte an der Wand bei x=10 stehen bleiben, x={}", p.pos.x);
    assert!(p.horizontal_collision);
}

// Vanilla (Entity.move) leitet horizontalCollision aus dem Vergleich "angefragte gegen
// tatsaechliche Bewegung" ab, NICHT aus den Kollisions-Flags. Der Sneak-Kantenschutz
// beschneidet die Bewegung ohne jede Kollision -- Vanilla meldet trotzdem true. Das Bit
// geht in jedem Bewegungspaket an den Server, eine Abweichung faellt dort auf.
#[test]
fn kantenschutz_meldet_horizontale_kollision_wie_vanilla() {
    let world = world_with_floor_up_to_x(0, 10);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut sneaker = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.0));
    let mut sneak_in = idle_input(0.0);
    sneak_in.strafe = 1.0; // yaw 0: +X, also auf die Kante bei x=10 zu
    sneak_in.sneak = true;
    for _ in 0..200 {
        sneaker.tick(&sneak_in, &world, &shapes);
    }
    // An der Kante angekommen: der Schutz klemmt die Bewegung jeden Tick weg, obwohl
    // nichts kollidiert -- genau dann muss das Bit gesetzt sein.
    assert!(sneaker.horizontal_collision, "an der Kante geklemmt, aber horizontalCollision=false (x={})", sneaker.pos.x);
}

// Fallhoehe: Entity.fall() schreibt sie mit der TATSAECHLICHEN Vertikalbewegung fort und
// nullt sie beim Bodenkontakt. Sie ist hier kein Selbstzweck -- der Sneak-Kantenschutz
// haengt daran (s. is_standing_on_surface).
#[test]
fn fallhoehe_waechst_beim_fallen_und_wird_bei_der_landung_genullt() {
    let world = world_with_floor_up_to_x(0, 32);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(8.0, 6.0, 8.0));
    for _ in 0..5 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(!p.on_ground, "Test setzt voraus, dass er noch fällt");
    assert!(p.fall_distance > 0.5, "Fallhöhe müsste nach 5 Ticks freiem Fall wachsen: {}", p.fall_distance);

    for _ in 0..60 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(p.on_ground, "haette laengst landen muessen");
    assert_eq!(p.fall_distance, 0.0, "Bodenkontakt nullt die Fallhöhe");
}

// Der zweite Zweig von isStandingOnSurface: kurz nach dem Verlassen der Kante ist man
// nicht mehr on_ground, aber noch keine Schritthoehe gefallen -- Vanilla haelt einen
// weiterhin fest. Vorher griff der Schutz hier nicht, und genau in diesen Ticks lief man
// weiter, waehrend die Server-Simulation klemmte.
#[test]
fn kantenschutz_greift_auch_kurz_nach_dem_abheben() {
    let world = world_with_floor_up_to_x(0, 10);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(9.0, 1.0, 8.0));
    p.crouching = true;
    // Nicht am Boden, aber erst ein winziges Stueck gefallen: der Block liegt noch
    // innerhalb der verbleibenden Schritthoehe.
    p.on_ground = false;
    p.fall_distance = 0.1;
    assert!(p.is_standing_on_surface(&world, &shapes), "Boden in Reichweite -> Vanilla hält fest");

    // Schon weiter gefallen als eine ganze Schritthoehe: jetzt ist Schluss.
    p.fall_distance = 0.7;
    assert!(!p.is_standing_on_surface(&world, &shapes), "über der Schritthöhe darf nichts mehr klemmen");
}

#[test]
fn sneaken_verhindert_das_fallen_ueber_die_kante() {
    // Boden nur bis Welt-X < 10 (Abbruchkante bei x=10). Spieler links davon, laeuft nach +X.
    let world = world_with_floor_up_to_x(0, 10);
    let shapes = world::BlockShapes::all_full_cubes();

    // Mit Sneak: darf ueber die Kante lehnen, aber NICHT herunterfallen (bleibt am Boden, y~1).
    let mut sneaker = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.0));
    let mut sneak_in = idle_input(0.0);
    sneak_in.strafe = 1.0; // yaw 0: +X
    sneak_in.sneak = true;
    for _ in 0..200 {
        sneaker.tick(&sneak_in, &world, &shapes);
    }
    assert!(sneaker.on_ground, "Schleicher muss auf dem Block bleiben, ist aber gefallen (y={})", sneaker.pos.y);
    assert!(sneaker.pos.y > 0.5, "Schleicher darf nicht herunterfallen, y={}", sneaker.pos.y);
    // Er darf leicht ueber die Kante lehnen, aber nicht weit ins Leere laufen.
    assert!(sneaker.pos.x < 10.5, "Schleicher darf nicht ueber die Kante hinauslaufen, x={}", sneaker.pos.x);

    // Kontrolle OHNE Sneak: laeuft ueber die Kante und faellt.
    let mut walker = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.0));
    let mut walk_in = idle_input(0.0);
    walk_in.strafe = 1.0;
    for _ in 0..200 {
        walker.tick(&walk_in, &world, &shapes);
    }
    assert!(walker.pos.y < 0.0, "ohne Sneak muss man ueber die Kante fallen, y={}", walker.pos.y);
}

// Regressionstest fuer einen echten Bug: sweep_axis hat die zu
// scannenden Block-Reihen bisher nur aus der Spieler-Y-Position berechnet. Ein Zaun
// (oder eine Mauer/ein Zauntor) sitzt zwar in EINER Zelle, seine Kollisionsbox reicht
// aber bis 1.5 Bloecke hoch (s. world::vanilla_shapes) -- ragt also in die Zelle
// DARUEBER. Sobald der Spieler beim Springen hoch genug war, fiel die Zaun-Zelle aus
// dem Scan-Bereich, und man ist mitten durch den eigentlich noch blockierenden oberen
// Teil gelaufen (auf dem Server als "moved wrongly" geflaggt worden). Simuliert genau
// dieses Szenario: mehrfach wiederholt gegen einen einzelnen, unverbundenen Zaunpfosten
// anspringen -- muss auch nach vielen Sprung-Versuchen an Ort und Stelle bleiben.
// Baut world_with_floor() zusaetzlich eine Wassersaeule (echte "water"-Quellblock-States,
// level=0, s. world::fluid) von water_bottom bis water_top (inklusive) ein.
fn world_with_floor_and_water(floor_y: i32, water_bottom: i32, water_top: i32) -> World {
    let water_state = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
        .expect("water-Quellblock-State nicht in der Registry gefunden");

    let mut w = World::new();
    let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
    let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == floor_section {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                        }
                    }
                }
                let section_base_y = world::MIN_Y + i * 16;
                for wy in water_bottom..=water_top {
                    if wy >= section_base_y && wy < section_base_y + 16 {
                        let ly = wy - section_base_y;
                        for x in 0..16usize {
                            for z in 0..16usize {
                                states[(ly as usize * 16 + z) * 16 + x] = water_state;
                            }
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Wie world_with_floor_and_water, aber mit EINER flachen Wasserschicht (fliessendes
// Wasser mit "level"-Property statt Quellblock, s. world::fluid::amount_from_level) auf
// dem Boden -- fuer den Wasser-Sprung-Cooldown-Test unten braucht es echt seichtes
// Wasser (Hoehe <= FLUID_JUMP_THRESHOLD=0.4), ein normaler Quellblock (level=0, Hoehe
// ~0.89) waere zu tief fuer den "an Land springen"-Zweig.
// Wasserlaeufer (WATER_MOVEMENT_EFFICIENCY): mit der Verzauberung schwimmt man spuerbar
// schneller -- travelInWater zieht sowohl die Bremse (0.8 -> 0.546) als auch die
// Beschleunigung (0.02 -> Landtempo) Richtung Land. Ohne das Attribut bliebe es beim
// langsamen Grundwert, waehrend Server und GrimAC mit dem schnellen rechnen.
#[test]
fn wasserlaeufer_macht_schwimmen_schneller() {
    let world = world_with_floor_and_water(0, 1, 8);
    let shapes = world::BlockShapes::all_full_cubes();
    let schritt = |effizienz: f64| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 4.0, 8.0));
        let mut i = idle_input(0.0);
        i.forward = 1.0;
        i.water_movement_efficiency = effizienz;
        for _ in 0..60 {
            p.tick(&i, &world, &shapes);
        }
        let before = p.pos;
        p.tick(&i, &world, &shapes);
        let d = p.pos - before;
        (d.x * d.x + d.z * d.z).sqrt()
    };
    let ohne = schritt(0.0);
    let mit = schritt(1.0);
    assert!(mit > ohne * 1.5, "Wasserläufer III muss deutlich schneller sein: {mit} gegen {ohne}");
}

fn world_with_floor_and_shallow_water(floor_y: i32, level: u8) -> World {
    let water_state = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some(&level.to_string()))
        .expect("fließender water-State mit diesem level nicht in der Registry gefunden");

    let mut w = World::new();
    let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
    let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
    let water_section = (floor_y + 1 - world::MIN_Y).div_euclid(16);
    let water_local_y = (floor_y + 1 - world::MIN_Y).rem_euclid(16);
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == floor_section {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                        }
                    }
                }
                if i == water_section {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((water_local_y * 16 + z) * 16 + x) as usize] = water_state;
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Echter Vanilla-Mechanismus ("water hopping", s. try_jump-Kommentar):
// haelt man Springen in seichtem Wasser (Hoehe <= 0.4) gedrueckt, bekommt man in
// Vanilla nur alle 10 Ticks (0.5s) einen echten Sprung (jumpingCooldown), nicht jeden
// Tick, in dem man gerade wieder Bodenkontakt hat -- Wasser bremst die
// Aufwaertsgeschwindigkeit staerker als Luft, man waere sonst ohne dieses Gate viel
// schneller wieder sprungbereit als an Land.
#[test]
fn wasser_sprung_hat_dieselbe_abklingzeit_wie_landsprung() {
    let world = world_with_floor_and_shallow_water(0, 6); // Hoehe 2/9 ~ 0.22, klar seicht
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    // Erst absetzen lassen (on_ground wird erst durch echte Kollision im ersten Tick
    // gesetzt, s. andere Tests dieser Datei), sonst wuerde der allererste Tick
    // faelschlich noch als "in der Luft" gelten und den Kick- statt den Sprung-Zweig
    // nehmen.
    for _ in 0..5 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(p.on_ground, "sollte nach dem Absetzen auf dem Boden stehen");
    let mut input = idle_input(0.0);
    input.jump = true;

    // Erster Tick: sollte sofort einen echten Sprung ausloesen (Cooldown startet bei 0).
    p.tick(&input, &world, &shapes);
    assert!(p.vel.y > 0.3, "erster Sprung sollte volle Hoehe geben, vel.y={}", p.vel.y);

    // Waehrend der Spieler noch in der Luft ist (jump weiter gehalten), darf KEIN
    // zweiter voller Sprung dazukommen -- ohne den Cooldown wuerde jedes erneute
    // "on_ground"-Antippen sofort wieder JUMP_VELOCITY setzen.
    let mut max_vel_y_after_first_tick = f64::MIN;
    for _ in 0..8 {
        p.tick(&input, &world, &shapes);
        max_vel_y_after_first_tick = max_vel_y_after_first_tick.max(p.vel.y);
    }
    assert!(
        max_vel_y_after_first_tick < 0.3,
        "innerhalb der 10-Tick-Abklingzeit sollte kein zweiter voller Sprung passieren, max vel.y={max_vel_y_after_first_tick}"
    );
}

// Tiefes Wasser (weit ueber und unter dem Spieler, keine Bodenberuehrung), mit einem
// einzelnen 1 Block hohen, untergetauchten Wandsegment quer zur Schwimmrichtung -- fuer
// den jump_out_of_fluid-Test unten: schwimmt man dagegen, MUSS horizontal_collision
// greifen, waehrend darueber noch reichlich Wasser (kein Fels) fuer den Frei-Test ist.
fn world_with_deep_water_and_underwater_wall(wall_x: i32, wall_y: i32) -> World {
    let water_state = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
        .expect("water-Quellblock-State nicht in der Registry gefunden");

    let mut w = World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                let section_base_y = world::MIN_Y + i * 16;
                for wy in -20..=40 {
                    if wy >= section_base_y && wy < section_base_y + 16 {
                        let ly = wy - section_base_y;
                        for x in 0..16usize {
                            for z in 0..16usize {
                                states[(ly as usize * 16 + z) * 16 + x] = water_state;
                            }
                        }
                    }
                }
                if wall_x.div_euclid(16) == cx && wall_y >= section_base_y && wall_y < section_base_y + 16 {
                    let ly = (wall_y - section_base_y) as usize;
                    let local_x = wall_x.rem_euclid(16) as usize;
                    for z in 0..16usize {
                        states[(ly * 16 + z) * 16 + local_x] = 1;
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Vanilla-Mechanismus "water hopping" (resetVerticalVelocityInFluid): schwimmt man OHNE zu
// springen horizontal gegen eine Kante und ist DARUEBER LUFT, klettert man automatisch ein
// Stueck heraus (fester vel.y=0.3-Kick). ENTSCHEIDEND (als
// GrimAC-Flag-Ursache erkannt): Vanillas Bedingung ist doesNotCollide(...) und das verlangt
// die Zielbox frei von Bloecken UND FLUID (Entity.doesNotCollide: isSpaceEmpty && !contains-
// Fluid). In TIEFEM Wasser gegen eine voll untergetauchte Wand ist ueber der Kante wieder
// Wasser -> KEIN Hop. Frueher (nur pose_fits, ohne Fluid-Check) hopte der Client hier
// faelschlich und wich horizontal von Grims Prediction ab.
#[test]
fn schwimmen_gegen_untergetauchte_wand_in_tiefem_wasser_klettert_nicht_heraus() {
    let wall_x = 10;
    let wall_y = 5;
    let world = world_with_deep_water_and_underwater_wall(wall_x, wall_y);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(5.0, f64::from(wall_y), 5.0));
    let mut input = idle_input(0.0); // yaw 0 = Blick nach +Z, wir wollen +X -> strafe
    input.strafe = 1.0;

    let mut saw_kick = false;
    for _ in 0..150 {
        p.tick(&input, &world, &shapes);
        if p.horizontal_collision && (p.vel.y - 0.3).abs() < 1e-9 {
            saw_kick = true;
            break;
        }
    }
    assert!(
        !saw_kick,
        "unter Wasser (Ziel über der Kante ist wieder Wasser) darf KEIN 0.3-Hop feuern -- das war die GrimAC-Abweichung"
    );
}

// Ein Becken mit Ufer: Boden bei y=0, ab x >= shore_x Fels bis zur Wasseroberkante, davor
// Wasser -- ueber beidem Luft. Genau die Geometrie, an der "water hopping" in Vanilla
// stattfindet (Gegenstueck zu world_with_deep_water_and_underwater_wall, wo ueber der
// Kante WIEDER Wasser steht und der Hop deshalb ausbleiben MUSS).
fn world_with_pool_and_shore(shore_x: i32, water_top: i32) -> World {
    let water_state = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
        .expect("water-Quellblock-State nicht in der Registry gefunden");

    let mut w = World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                let section_base_y = world::MIN_Y + i * 16;
                for wy in 0..=water_top {
                    if wy < section_base_y || wy >= section_base_y + 16 {
                        continue;
                    }
                    let ly = (wy - section_base_y) as usize;
                    for lx in 0..16usize {
                        let world_x = cx * 16 + lx as i32;
                        // y=0 ist ueberall Beckenboden, darueber Ufer-Fels bzw. Wasser.
                        let state = if wy == 0 || world_x >= shore_x { 1 } else { water_state };
                        for lz in 0..16usize {
                            states[(ly * 16 + lz) * 16 + lx] = state;
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Positive Seite von resetVerticalVelocityInFluid: schwimmt man am Beckenrand gegen das
// Ufer und ist ueber der Kante LUFT (kein Block, kein Fluid), setzt Vanilla vel.y hart auf
// 0.3F und man klettert heraus -- ohne Sprung-Cooldown, weshalb sich das bei anhaltendem
// Wandkontakt jeden Tick wiederholen kann ("water hopping"). Der Kick ist exakt der
// double-Wert des FLOATS 0.3F; 0.3 waere um 1.2e-8 daneben und driftet ueber eine
// Hop-Serie von Grims Prediction weg.
#[test]
fn schwimmen_gegen_das_ufer_klettert_heraus() {
    let shore_x = 10;
    let water_top = 9; // Wasseroberflaeche bei y=10.0, darueber Luft
    let world = world_with_pool_and_shore(shore_x, water_top);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(5.0, 8.0, 5.0));
    let mut input = idle_input(0.0); // yaw 0 = Blick nach +Z, +X kommt ueber strafe
    input.strafe = 1.0;
    input.jump = true; // hochschwimmen, bis die Fuesse die Kante erreichen

    let mut saw_kick = false;
    for _ in 0..200 {
        p.tick(&input, &world, &shapes);
        if p.vel.y == f64::from(0.3f32) {
            saw_kick = true;
        }
        if p.pos.x >= f64::from(shore_x) && p.on_ground {
            break;
        }
    }
    assert!(saw_kick, "am Ufer mit Luft darüber MUSS der 0.3F-Kick feuern");
    assert!(p.pos.y > f64::from(water_top), "der Hop muss ueber die Kante bringen, nicht darunter haengen bleiben: y={}", p.pos.y);
}

// Regressionstest: Wasser fehlte in der
// begehbaren Blockliste, der Spieler stand oben auf dem Wasserspiegel wie auf festem
// Boden. Jetzt muss er einsinken -- Fuesse deutlich unter der Wasseroberflaeche.
#[test]
fn spieler_sinkt_ins_wasser_statt_obendrauf_zu_stehen() {
    let world = world_with_floor_and_water(0, 1, 10);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 15.0, 8.0));
    for _ in 0..60 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(p.pos.y < 9.0, "sollte im Wasser eingesunken sein, nicht obendrauf stehen: y={}", p.pos.y);
}

// Wie world_with_floor_and_water(), aber mit Seegras statt echten Wasser-Quellbloecken
// -- Seegras traegt in Vanilla keine "waterlogged"-Property (existiert nie trocken),
// muss fuer die Schwimmphysik aber trotzdem als Wasser zaehlen (s. world::fluid::
// always_wet). Regressionstest: im Seegras lief man,
// als sei da kein Wasser.
fn world_with_floor_and_seagrass(floor_y: i32, grass_bottom: i32, grass_top: i32) -> World {
    let seagrass_state = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("seagrass"))
        .expect("seagrass-State nicht in der Registry gefunden");

    let mut w = World::new();
    let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
    let floor_local_y = (floor_y - world::MIN_Y).rem_euclid(16);
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == floor_section {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((floor_local_y * 16 + z) * 16 + x) as usize] = 1;
                        }
                    }
                }
                let section_base_y = world::MIN_Y + i * 16;
                for wy in grass_bottom..=grass_top {
                    if wy >= section_base_y && wy < section_base_y + 16 {
                        let ly = wy - section_base_y;
                        for x in 0..16usize {
                            for z in 0..16usize {
                                states[(ly as usize * 16 + z) * 16 + x] = seagrass_state;
                            }
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

#[test]
fn spieler_sinkt_im_seegras_statt_frei_zu_fallen() {
    let world = world_with_floor_and_seagrass(-50, -10, 40);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 30.0, 8.0));
    for _ in 0..40 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(
        p.vel.y.abs() < GRAVITY,
        "im Seegras sollte genauso gebremst gesunken werden wie im Wasser, nicht frei gefallen: vel.y={}",
        p.vel.y
    );
}

// travelInWater(): Gravitation im Wasser ist nur d/16 statt d -- deutlich langsameres
// Sinken als der freie Fall. Vergleicht die Sinkgeschwindigkeit nach Erreichen des
// eingeschwungenen Zustands gegen die volle Luft-Gravitation.
#[test]
fn sinken_im_wasser_ist_viel_langsamer_als_freier_fall() {
    let world = world_with_floor_and_water(-50, -10, 40);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 30.0, 8.0));
    for _ in 0..40 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(
        p.vel.y.abs() < GRAVITY,
        "Sink-Geschwindigkeit im Wasser sollte deutlich unter der vollen Gravitation liegen: vel.y={}",
        p.vel.y
    );
}

// jumpInLiquid(): Space gedrueckt halten in tiefem Wasser muss nach oben schwimmen
// lassen (+0.04/Tick, kein Cooldown) statt wie an Land nur einmal abzuspringen.
#[test]
fn hochschwimmen_mit_gehaltenem_sprung_in_tiefem_wasser() {
    let world = world_with_floor_and_water(-50, -10, 40);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
    // Erst ein paar Ticks ohne Eingabe, damit der Spieler wirklich mitten im Wasser
    // treibt (nicht gerade erst eingetaucht).
    for _ in 0..10 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    let before = p.pos.y;
    let mut input = idle_input(0.0);
    input.jump = true;
    for _ in 0..20 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.pos.y > before + 1.0, "sollte durch Hochschwimmen deutlich aufgestiegen sein: vorher={before}, nachher={}", p.pos.y);
}

// Regressionstest: im Wasser Jump+Sneak flaggte heftig.
// swimUpward (+0.04, Jump) und knockDownwards (-0.04, Sneak) sind in Vanilla zwei getrennte
// Checks und heben sich bei gleichzeitigem Druecken zu netto 0 auf. Vorher verschluckte ein
// if/else if den Sneak-Kick -> netto +0.04/Tick -> die Y-Position lief von Grims Simulation
// weg. Ergo muss Jump+Sneak vertikal IDENTISCH zu voelligem Nichtstun (Idle) treiben.
#[test]
fn jump_und_sneak_im_wasser_heben_sich_auf() {
    let world = world_with_floor_and_water(-50, -10, 40);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut idle = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
    let mut jump_sneak = PlayerPhysics::new(DVec3::new(8.0, 10.0, 8.0));
    // Erst frei treiben lassen (nicht gerade erst eingetaucht).
    for _ in 0..10 {
        idle.tick(&idle_input(0.0), &world, &shapes);
        jump_sneak.tick(&idle_input(0.0), &world, &shapes);
    }
    let mut js_input = idle_input(0.0);
    js_input.jump = true;
    js_input.sneak = true;
    for _ in 0..30 {
        idle.tick(&idle_input(0.0), &world, &shapes);
        jump_sneak.tick(&js_input, &world, &shapes);
    }
    // Beide Kicks heben sich exakt auf -> vertikal identisch zu Idle (kein Aufstieg).
    assert!(
        (idle.pos.y - jump_sneak.pos.y).abs() < 1.0e-6,
        "Jump+Sneak muss vertikal exakt wie Idle treiben (Kicks heben sich auf): idle={}, jump+sneak={}",
        idle.pos.y,
        jump_sneak.pos.y
    );
}

#[test]
fn zaun_bleibt_auch_beim_springen_undurchdringlich() {
    let fence_state = (0..world::block_registry::state_count() as u32)
        .find(|&s| {
            world::block_name(s) == Some("oak_fence")
                && world::state_props::property(s, "north") == Some("false")
                && world::state_props::property(s, "south") == Some("false")
                && world::state_props::property(s, "east") == Some("false")
                && world::state_props::property(s, "west") == Some("false")
                && world::state_props::property(s, "waterlogged") == Some("false")
        })
        .expect("oak_fence-State nicht in der Registry gefunden");

    let mut world = World::new();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == 4 {
                    // Index innerhalb der Section: (y * 16 + z) * 16 + x.
                    let index = |x: usize, y: usize, z: usize| (y * 16 + z) * 16 + x;
                    // Boden bei Welt-Y=0 (Section 4 beginnt bei MIN_Y + 4*16 = 0).
                    for x in 0..16 {
                        for z in 0..16 {
                            states[index(x, 0, z)] = 1;
                        }
                    }
                    // Einzelner Zaunpfosten bei Welt (8,1,8) -- lokal x=8,z=8, y=1.
                    if cx == 0 && cz == 0 {
                        states[index(8, 1, 8)] = fence_state;
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            world.insert_column(cx, cz, sections);
        }
    }
    let shapes = world::BlockShapes::all_full_cubes();

    // Mittig durch die Pfosten-Zelle (Welt-Z=8.5), Anlauf in +X.
    let mut p = PlayerPhysics::new(DVec3::new(5.0, 1.0, 8.5));
    let mut input = idle_input(0.0);
    input.strafe = 1.0;
    input.sprint_held = true;
    for tick in 0..300 {
        input.jump = tick % 5 == 0; // alle 0.25s erneut anspringen
        p.tick(&input, &world, &shapes);
    }
    assert!(p.pos.x < 8.375, "Spieler haette am Zaunpfosten (x=8.375) haengen bleiben muessen, ist aber bei x={}", p.pos.x);
}

// Regressionstest fuer isSwimming() (s. PlayerLikeEntity.POSE_DIMENSIONS-Kommentar):
// Einstieg braucht sowohl Sprinten als auch VOLLSTAENDIG untergetaucht zu sein -- bloss
// an der Wasseroberflaeche zu sprinten (Kopf noch in der Luft) reicht NICHT.
#[test]
fn schwimm_pose_braucht_sprinten_und_vollstaendiges_untertauchen() {
    // Wasser von y=1 bis y=10 -- Spieler bei y=1 (Fuesse), Augenhoehe 1.62 -> Kopf bei
    // y=2.62, also klar unter Wasser (bis y=10).
    let world = world_with_floor_and_water(0, 1, 10);
    let shapes = world::BlockShapes::all_full_cubes();

    // Vollstaendig untergetaucht ist NICHT "isPartlyTouchingWater()" (s. Kommentar in
    // tick()) -- ein frischer Sprint darf hier also direkt ab Tick 0 starten.
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    // Zwei Ticks: Tick 1 startet den Sprint, die Schwimm-Pose (updateSwimming aus baseTick,
    // s. tick()) greift erst EINEN Tick nach dem Sprint-Start -- also ab Tick 2.
    p.tick(&input, &world, &shapes);
    p.tick(&input, &world, &shapes);
    assert!(p.swimming, "sprintend UND vollständig untergetaucht muss die Schwimm-Pose auslösen");

    // Ohne Sprinten (nur im Wasser) keine Schwimm-Pose.
    let mut p2 = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let idle = idle_input(0.0);
    p2.tick(&idle, &world, &shapes);
    assert!(!p2.swimming, "ohne Sprinten keine Schwimm-Pose, nur normales Plantschen");
}

// Regressionstest: auf der Wasseroberflaeche liess sich sprinten, in Vanilla nicht. ClientPlayerEntity.canStartSprinting()/canSprint() lassen einen
// NEUEN Sprint nicht zu, waehrend man (auch nur oberflaechlich) Wasser beruehrt.
#[test]
fn kein_neuer_sprint_an_der_wasseroberflaeche() {
    // Duennes Wasser nur bei y=1 -- Spieler steht mit den Fuessen genau darin, wie beim
    // Stehen an der Oberflaeche.
    let world = world_with_floor_and_water(0, 1, 1);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    for _ in 0..10 {
        p.tick(&input, &world, &shapes);
        assert!(!p.sprinting, "ein NEUER Sprint darf nicht beginnen, während man Wasser berührt");
    }
}

// Regressionstest fuer den NACHFOLGENDEN Bug-Report ("kann sprinten an der
// Wasseroberflaeche, wenn Sprintlock an ist"): ausserhalb der Schwimm-Pose gilt fuer einen
// bereits laufenden Sprint shouldStopSprinting() -> canSprint(false), NICHT das
// permissivere shouldStopSwimSprinting() -- ein an Land begonnener Sprint muss also sofort
// stoppen, sobald man (ohne in die Schwimm-Pose zu kippen) auch nur oberflaechlich Wasser
// beruehrt. Mit Sprint-Umschalten (sprint_held bleibt dauerhaft true, kein Loslassen an der
// Wasserkante wie bei gehaltener Taste) fiel das zuvor bei praktisch jedem Wasserkontakt auf.
#[test]
fn laufender_sprint_stoppt_an_der_wasseroberflaeche_ausserhalb_der_schwimm_pose() {
    let world = world_with_floor_and_water(0, 1, 1);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut already = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    already.sprinting = true; // schon VOR dem Wasserkontakt am Sprinten, an Land (nicht Schwimm-Pose)
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    already.tick(&input, &world, &shapes);
    assert!(!already.sprinting, "ein an Land laufender Sprint muss stoppen, sobald man (außerhalb der Schwimm-Pose) Wasser berührt");
}

// Regressionstest fuer den NACHFOLGENDEN Bug-Report ("kann nicht mehr sprint-schwimmen,
// auch wenn der Kopf im Wasser ist"): isPartlyTouchingWater() (Entity.java) ist NICHT
// "beruehrt Wasser", sondern explizit "beruehrt Wasser UND ist NICHT untergetaucht" --
// vollstaendig untergetaucht (Kopf unter Wasser) darf ein frischer Sprint jederzeit
// starten, nur die reine Oberflaechenberuehrung (s. Test oben) blockiert den Start.
#[test]
fn neuer_sprint_darf_vollstaendig_untergetaucht_starten() {
    let world = world_with_floor_and_water(0, 1, 10);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    assert!(!p.sprinting);
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    p.tick(&input, &world, &shapes);
    assert!(p.sprinting, "vollständig untergetaucht muss ein frischer Sprint sofort starten dürfen");
}

// Regressionstest fuer den Sprint-Flacker am Wasser->Luft-Austritt (Grim consoledebug 01:29,
// O~0.0137): beim Hochschwimmen und Austauchen darf der Sprint NICHT fuer einen Tick abfallen
// und sofort wieder anspringen (das schickte ein spurious STOP_SPRINTING und desynchronisierte
// Grims Sprint-Zustand). Sobald man beim Sprint-Schwimmen austaucht, muss der Sprint als
// Land-Sprint nahtlos weiterlaufen (Vanilla: isSwimming schon aus -> shouldStopSprinting-Zweig).
#[test]
fn sprint_flackert_nicht_beim_austauchen() {
    let world = world_with_floor_and_water(-30, -29, 0); // Oberflaeche y=1, Luft darueber
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, -6.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    input.jump = true;
    input.look_dir_y = 0.7; // Blick nach oben -> schwimmt hoch und taucht aus
    let mut states = Vec::new();
    for _ in 0..40 {
        p.tick(&input, &world, &shapes);
        states.push(p.sprinting);
    }
    // Sobald Sprint einmal lief, darf es keinen true->false->true-Flacker geben, solange
    // vorwaerts + sprint_held gehalten wird und man aus dem Wasser austaucht.
    let mut flicker = false;
    for w in states.windows(3) {
        if w[0] && !w[1] && w[2] {
            flicker = true;
        }
    }
    assert!(!flicker, "Sprint darf am Wasser->Luft-Austritt nicht für einen Tick abfallen (Flacker): {states:?}");
}

// Regressionstest fuer den 0.0178-Z-Offset-Flag beim Sprint-Rausschwimmen (test 13,
// algebraisch aus Grims P/A hergeleitet: A.z=0.20172798 = 0.026×0.98×1.0-Accel, P.z=0.18389193
// = 0.026×0.98×0.3-Accel). Am Austritts-Tick ist der Spieler isCrawling() (Schwimm-Pose vom
// ENDE des Vorticks -- updatePose laeuft in Vanilla am Tick-ENDE --, Fuesse aber schon aus dem
// Wasser), Vanilla wendet deshalb den ×0.3-Bewegungsfaktor an, OHNE dass Sneak gehalten wird.
// Geprueft ueber das direkte Verhaeltnis der Horizontal-Beschleunigung mit vs. ohne
// Schwimm-Pose-Rest -- muss DEFAULT_SNEAKING_SPEED (0.3) sein.
#[test]
fn crawling_beim_rausschwimmen_verlangsamt_horizontal_um_030() {
    // Wasser tief unten, Spieler klar in der Luft darueber -> water_height dieses Ticks = None
    // (Luft-Dispatch), waehrend was_swimming aus dem Vortick-Pose-Rest kommt.
    let world = world_with_floor_and_water(-30, -29, -20);
    let shapes = world::BlockShapes::all_full_cubes();

    let run = |swim_pose_rest: bool| -> f64 {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 0.0, 8.0));
        p.sprinting = true; // Sprint laeuft bereits (beeinflusst beide Laeufe gleich)
        p.swimming = swim_pose_rest; // Pose-Rest vom ENDE des letzten Ticks (isInSwimmingPose)
        let mut input = idle_input(0.0);
        input.forward = 1.0;
        input.sprint_held = true;
        assert!(!input.sneak, "Test setzt voraus, dass Sneak NICHT gehalten wird");
        let vz_before = p.vel.z;
        p.tick(&input, &world, &shapes);
        p.vel.z - vz_before
    };

    let dz_crawling = run(true);
    let dz_normal = run(false);
    assert!(dz_normal.abs() > 1e-9, "Kontroll-Lauf muss überhaupt beschleunigen: {dz_normal}");
    let ratio = dz_crawling / dz_normal;
    assert!(
        (ratio - DEFAULT_SNEAKING_SPEED).abs() < 1e-6,
        "Crawling-Austritt muss die Horizontal-Accel um DEFAULT_SNEAKING_SPEED (={DEFAULT_SNEAKING_SPEED}) \
             daempfen (isCrawling->shouldSlowDown), Verhaeltnis war {ratio} \
             (crawling={dz_crawling}, normal={dz_normal})"
    );
}

// Regressionstest fuer den Wasser-Setback beim schleichenden Reinlaufen (Grim consoledebug
// 00:43, P.y=-0.19992 Crouch-Sink vs A.y=-0.04 Schwimmen, O~0.16): Vanillas updateSwimming
// laeuft in baseTick() VOR der Sprint-Entscheidung/travel() (LivingEntity.tick 2633 vs 2672),
// die Schwimm-Pose greift also erst EINEN Tick NACH dem Sprint-Start. Auf dem Sprint-Start-
// Tick muss darum noch Crouch-Physik laufen (raw_swimming/swimming noch aus), erst der
// naechste Tick schwimmt.
#[test]
fn schwimm_pose_laggt_einen_tick_nach_dem_sprint_start() {
    let world = world_with_floor_and_water(0, 1, 10);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;

    // Tick 1: Sprint startet (untergetaucht erlaubt), aber Schwimm-Pose noch NICHT --
    // updateSwimming sah isSprinting vom Vortick (false).
    p.tick(&input, &world, &shapes);
    assert!(p.sprinting, "Sprint muss untergetaucht sofort starten");
    assert!(!p.swimming, "auf dem Sprint-Start-Tick darf die Schwimm-Pose noch NICHT aktiv sein (Ein-Tick-Lag)");

    // Tick 2: jetzt sah updateSwimming isSprinting=true -> Schwimm-Pose an.
    p.tick(&input, &world, &shapes);
    assert!(p.swimming, "einen Tick nach dem Sprint-Start muss die Schwimm-Pose greifen");
}

// Regressionstest: im Wasser fuehrt Sneak + W in Vanilla in den Schwimm-Modus. canStartSprinting() hebt die Sneak-Sperre auf, sobald man vollstaendig
// untergetaucht ist (`!shouldSlowDown() || isSubmergedInWater()`). Trotz gedruecktem Sneak
// muss ein frischer Sprint (und damit die Schwimm-Pose) untergetaucht also starten duerfen.
#[test]
fn untergetaucht_startet_sprint_auch_mit_gedruecktem_sneak() {
    let world = world_with_floor_and_water(0, 1, 10);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    input.sneak = true;
    // Ein paar Ticks, damit der Spieler sicher untergetaucht ist und die Schwimm-Pose greift.
    for _ in 0..5 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.sprinting, "untergetaucht muss der Sprint trotz Sneak starten (canStartSprinting-Ausnahme)");
    assert!(p.swimming, "mit laufendem Sprint + untergetaucht muss die Schwimm-Pose aktiv sein");
}

// Sobald die Schwimm-Pose einmal aktiv ist, reicht zum WEITERSCHWIMMEN blosse
// Wasserberuehrung (nicht mehr volles Untertauchen) -- man bleibt beim Auftauchen an der
// Oberflaeche also in der Pose, solange man weiter sprintet (s. updateSwimming()).
#[test]
fn bleibt_beim_auftauchen_in_der_schwimm_pose() {
    // Duennes Wasser nur bei y=1 (Fuesse gerade so drin, Kopf bei y=2.62 klar drueber --
    // "aufgetaucht", aber die Fuesse beruehren noch Wasser).
    let world = world_with_floor_and_water(0, 1, 1);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    p.swimming = true; // simuliert "war schon am Schwimmen, bevor aufgetaucht wurde"
    p.sprinting = true; // s. Kommentar oben: ein NEUER Sprint startet nicht im Wasser
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    p.tick(&input, &world, &shapes);
    assert!(p.swimming, "an der Oberfläche weitersprintend muss die Pose erhalten bleiben");
}

// Regressionstest fuer PlayerLikeEntity.POSE_DIMENSIONS: Schwimm-Pose ist 0.6x0.6 statt
// 0.6x1.8, Augenhoehe 0.4 statt 1.62 -- NICHT geraten, sondern aus dem lokalen
// Yarn-1.21.11-Jar nachgesehen (PlayerEntity erbt von PlayerLikeEntity,
// das die Pose-Tabelle traegt -- eine fruehere Suche nur in PlayerEntity/LivingEntity hat
// sie deshalb uebersehen).
#[test]
fn schwimm_pose_hat_die_richtige_kollisionsgroesse() {
    let world = world_with_floor_and_water(0, 1, 10);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    // Ein-Tick-Lag der Schwimm-Pose nach dem Sprint-Start (s. tick()) -> zwei Ticks.
    p.tick(&input, &world, &shapes);
    p.tick(&input, &world, &shapes);
    assert!(p.swimming);
    assert!((p.eye_pos().y - p.pos.y - 0.4).abs() < 1.0e-5, "Augenhöhe beim Schwimmen muss 0.4 sein, war {}", p.eye_pos().y - p.pos.y);
}

// Regressionstest fuer PlayerEntity.travel()s Blickrichtungs-Zug (s. apply_swim_pitch_pull-
// Kommentar): beim Sprint-Schwimmen mit Wasser rundum steuert allein die Blickrichtung die
// Vertikalgeschwindigkeit -- nach unten schauen muss abwaerts ziehen, nach oben schauen
// (bei noch Wasser ueber dem Kopf) aufwaerts, jeweils OHNE Springen/Schleichen zu halten.
#[test]
fn sprint_schwimmen_folgt_der_blickrichtung() {
    let world = world_with_floor_and_water(-10, -9, 20); // tiefes, allseitig umschlossenes Wasser
    let shapes = world::BlockShapes::all_full_cubes();

    let mut down = PlayerPhysics::new(DVec3::new(8.0, 0.0, 8.0));
    down.swimming = true;
    let mut input_down = idle_input(0.0);
    input_down.forward = 1.0;
    input_down.sprint_held = true;
    input_down.look_dir_y = -1.0; // senkrecht nach unten geschaut
    for _ in 0..5 {
        down.tick(&input_down, &world, &shapes);
    }
    assert!(down.vel.y < -0.05, "beim Sprint-Schwimmen nach unten geschaut muss abwaerts gezogen werden, vel.y={}", down.vel.y);

    let mut up = PlayerPhysics::new(DVec3::new(8.0, 0.0, 8.0));
    up.swimming = true;
    let mut input_up = idle_input(0.0);
    input_up.forward = 1.0;
    input_up.sprint_held = true;
    input_up.look_dir_y = 1.0; // senkrecht nach oben geschaut, aber noch Wasser ueber dem Kopf
    for _ in 0..5 {
        up.tick(&input_up, &world, &shapes);
    }
    assert!(
        up.vel.y > 0.02,
        "beim Sprint-Schwimmen nach oben geschaut (mit Wasser ueber dem Kopf) muss aufwaerts gezogen werden, vel.y={}",
        up.vel.y
    );
}

// Regressionstest fuer "wenn ich beim Schwimmen meinen Kopf hitte buggt es voll rum":
// der Fluessigkeits-BERUEHRUNGS-Scan lief frueher immer mit der hartkodierten Stehhoehe
// (1.8) statt der TATSAECHLICHEN aktuellen Boxhoehe -- unter Dauerdruck gegen eine knapp
// ueber der Schwimm-Pose (0.6) liegende Unterwasser-Decke soll die Schwimm-Pose stabil
// bleiben, nicht zwischen 0.6 und 1.8 hin- und herkippen.
fn thin_water_pocket(floor_y: i32, water_y: i32, ceiling_y: i32) -> World {
    let water_state = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
        .expect("water state");
    let mut w = World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                let section_base_y = world::MIN_Y + i * 16;
                for (y, state) in [(floor_y, 1u32), (water_y, water_state), (ceiling_y, 1u32)] {
                    if y >= section_base_y && y < section_base_y + 16 {
                        let ly = y - section_base_y;
                        for x in 0..16usize {
                            for z in 0..16usize {
                                states[(ly as usize * 16 + z) * 16 + x] = state;
                            }
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Die Eis-/Wasserplattform von anticheat-test.com: Eis bei y=90 (Oberkante 91.0),
// darueber EINE Schicht Wasser bei y=91. Damit die Stroemung eine Richtung hat, sind die
// Zellen ab x=9 fliessendes Wasser (level=1) statt Quelle -- das Gefaelle zeigt nach +x,
// der Schub ist damit die volle 0.014 in +x, genau wie im echten Mitschnitt gemessen.
fn eis_wasser_plattform() -> World {
    let quelle = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("0"))
        .expect("water level=0");
    let fliessend = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("water") && world::state_props::property(s, "level") == Some("1"))
        .expect("water level=1");
    let eis = (0..world::block_registry::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("ice"))
        .expect("ice");
    let mut w = World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                let section_base_y = world::MIN_Y + i * 16;
                for (y, welt_x_quelle) in [(90, None), (91, Some(9))] {
                    if y < section_base_y || y >= section_base_y + 16 {
                        continue;
                    }
                    let ly = (y - section_base_y) as usize;
                    for x in 0..16usize {
                        for z in 0..16usize {
                            let welt_x = cx * 16 + x as i32;
                            let state = match welt_x_quelle {
                                None => eis,
                                Some(grenze) if welt_x >= grenze => fliessend,
                                Some(_) => quelle,
                            };
                            states[(ly * 16 + z) * 16 + x] = state;
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Gemessen mit pktproxy --mitm auf modern.anticheat-test.com: Vanilla und Coral Client fallen
// von y=100 auf die Plattform. Der Fall ist auf vier Nachkommastellen deckungsgleich, aber
// im Landetick bewegte sich Vanilla um 0.0267 nach +x und wir nur um 0.0140.
//
// Ursache war der fehlende Nachschlag in Entity.move()/LivingEntity.checkFallDamage
// (Wasserschub nachholen, solange isInWater() noch falsch ist).
// Im Tick des Eintauchens war isInWater() noch falsch (aus baseTick, vor der Bewegung),
// der Schub kam also erst nach move() -- seitlich wirkungslos, aber noch mit g = 0.91
// gedaempft und damit im naechsten Tick wirksam. 0.014 × 0.91 + 0.014 = 0.02674.
#[test]
fn landehoehe_ist_exakt_die_blockoberkante() {
    let world = eis_wasser_plattform();
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 100.0, 8.5));
    let input = idle_input(0.0);
    for _ in 0..40 {
        p.tick(&input, &world, &shapes);
        if p.on_ground {
            break;
        }
    }
    // Sonar vergleicht die Landehoehe EXAKT:
    //   collisionOffsetY = (PLATFORM_Y_POSITION + blockHeight) - y; if (collisionOffsetY != 0) fail
    // Ein Ergebnis von 91.00000000000001 faellt dort durch, sieht im Log aber wie 91.0000 aus.
    assert_eq!(p.pos.y.to_bits(), 91.0f64.to_bits(), "Landehöhe ist {:.17}, nicht exakt 91.0", p.pos.y);
}

#[test]
fn eintauchen_holt_den_stroemungsschub_noch_im_falltick_nach() {
    let world = eis_wasser_plattform();
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 100.0, 8.5));
    let input = idle_input(0.0);

    let mut landetick_dx = None;
    let mut vorher_x = p.pos.x;
    for _ in 0..40 {
        let war_in_der_luft = !p.on_ground;
        p.tick(&input, &world, &shapes);
        if war_in_der_luft && p.on_ground {
            landetick_dx = Some(p.pos.x - vorher_x);
            break;
        }
        vorher_x = p.pos.x;
    }

    let dx = landetick_dx.expect("der Spieler muss auf der Plattform landen");
    assert!(
        (p.pos.y - 91.0).abs() < 1e-9,
        "die Eisoberkante liegt bei y=91.0, gelandet wurde bei {}",
        p.pos.y
    );
    assert!(
        (dx - 0.02674).abs() < 1e-6,
        "im Landetick erwartet Vanilla 0.014 × 0.91 + 0.014 = 0.02674, gemessen {dx}"
    );

    // Danach nur noch ein Schub pro Tick: v <- 0.8 * v + 0.014. Der Nachschlag darf sich
    // NICHT wiederholen, sobald man schon im Wasser steht.
    let mut erwartet = dx;
    for tick in 0..6 {
        let vorher = p.pos.x;
        p.tick(&input, &world, &shapes);
        erwartet = 0.8 * erwartet + 0.014;
        let gemessen = p.pos.x - vorher;
        assert!(
            (gemessen - erwartet).abs() < 1e-6,
            "Tick {tick} nach der Landung: erwartet {erwartet}, gemessen {gemessen}"
        );
    }
}

#[test]
fn schwimm_pose_flackert_nicht_unter_niedriger_decke() {
    let world = thin_water_pocket(0, 1, 2); // Wasser NUR bei y=1..2, Decke bei y=2..3
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    p.swimming = true;
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    input.look_dir_y = 1.0; // staendig nach oben gegen die Decke druecken

    let mut flips = 0;
    let mut was_swimming = true;
    for _ in 0..100 {
        p.tick(&input, &world, &shapes);
        if p.swimming != was_swimming {
            flips += 1;
        }
        was_swimming = p.swimming;
    }
    assert_eq!(flips, 0, "Schwimm-Pose sollte unter Dauerdruck gegen eine niedrige Decke NICHT flackern");
}

// Regressionstest: an der Decke geschwommen darf der Kopf beim Sprint-Ende nicht in
// die Decke clippen. Ohne den
// canChangeIntoPose()-Fitting-Fallback (s. pose_fits-Kommentar) wuerde raw_swimming beim
// Loslassen von Sprint sofort false werden und current_height() auf Stehhoehe (1.8)
// zurueckspringen -- die dann sofort in die Decke ragt (Kollisionsbox ueberlappt
// Solides). Mit dem Fallback muss die Pose zwangsweise bei SWIMMING (0.6) bleiben.
#[test]
fn kein_decken_clipping_beim_aufhoeren_zu_sprinten() {
    let world = thin_water_pocket(0, 1, 2); // Wasser NUR bei y=1..2, Decke bei y=2..3
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let mut input = idle_input(0.0);
    input.forward = 1.0;
    input.sprint_held = true;
    input.look_dir_y = 1.0; // gegen die Decke schwimmen
    for _ in 0..30 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.swimming, "sollte vor dem Loslassen von Sprint in der Schwimm-Pose sein");

    // Sprint loslassen, weiter gegen die Decke gedrueckt -- die Pose MUSS wegen fehlenden
    // Platzes bei SWIMMING bleiben, nicht auf Stehhoehe zurueckspringen.
    input.sprint_held = false;
    for _ in 0..10 {
        p.tick(&input, &world, &shapes);
        assert!(p.swimming, "Pose sollte trotz losgelassenem Sprint bei SWIMMING bleiben (kein Platz zum Stehen)");
        assert!(
            pose_fits(p.pos, p.current_height(), &world, &shapes, world::ShapeContext::ABSENT, BodyAttributes::default()),
            "die tatsächlich benutzte Hitbox darf nie in die Decke clippen"
        );
    }
}
// ---------------------------------------------------------------------------------
// Seelensand / Honig / Schleim -- die drei Bloecke mit eigener Bewegungsmechanik.
// Alle Zahlen aus SoulSandBlock/HoneyBlock/SlimeBlock + Blocks (1.21.11).
// ---------------------------------------------------------------------------------

fn state_of_block(name: &str) -> u32 {
    (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some(name)).unwrap_or_else(|| panic!("{name} unbekannt"))
}

// Wie world_with_floor, aber der Boden besteht aus einem benannten Block.
fn world_with_floor_of(floor_y: i32, name: &str) -> World {
    let state = state_of_block(name);
    let mut w = World::new();
    let section_index = (floor_y - world::MIN_Y).div_euclid(16);
    let local_y = (floor_y - world::MIN_Y).rem_euclid(16);
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == section_index {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((local_y * 16 + z) * 16 + x) as usize] = state;
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

// Nach Osten laufen (Strafe, damit die Blickrichtung egal bleibt) -- step_tests hat ein
// gleichnamiges Gegenstueck, das in diesem Modul aber nicht sichtbar ist.
fn lauf_input() -> Input {
    let mut i = idle_input(0.0);
    i.strafe = 1.0;
    i
}

// Auf einem Schleimblock kommt man NICHT durch Hinfallen zur Ruhe -- jeder Aufprall
// springt zurueck. Deshalb hier von vornherein aufsetzen und nur ausschwingen lassen,
// bis die Restbewegung unter der Vanilla-Nullungsschwelle liegt.
fn ruht_auf(world: &World, shapes: &world::BlockShapes, start: DVec3) -> PlayerPhysics {
    let mut p = PlayerPhysics::new(start);
    p.refresh_on_ground(world, shapes);
    for _ in 0..30 {
        p.tick(&idle_input(0.0), world, shapes);
    }
    assert!(p.on_ground && p.vel.y.abs() < 0.1, "Testaufbau: sollte ruhig aufliegen, vel.y={}", p.vel.y);
    p
}

fn steht_still_auf(world: &World, shapes: &world::BlockShapes, start: DVec3) -> PlayerPhysics {
    let mut p = PlayerPhysics::new(start);
    for _ in 0..10 {
        p.tick(&idle_input(0.0), world, shapes);
    }
    assert!(p.on_ground, "Testaufbau: sollte am Boden stehen, y={}", p.pos.y);
    p
}

// SoulSandBlock.getCollisionShape: Block.createColumnShape(16, 0, 14) -- die Oberkante
// liegt bei 14/16, man sinkt also 2 Pixel ein. Nur die KOLLISION ist niedriger, der
// Umriss bleibt der Vollwuerfel (sonst zielte man daneben).
#[test]
fn seelensand_laesst_zwei_pixel_einsinken() {
    let world = world_with_floor_of(0, "soul_sand");
    let shapes = world::BlockShapes::all_full_cubes();
    let p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
    assert!((p.pos.y - 0.875).abs() < 1.0e-6, "Fuesse sollten auf 14/16 stehen, y={}", p.pos.y);

    let sand = state_of_block("soul_sand");
    let kollision = world::collision_boxes(sand, &shapes);
    assert_eq!(kollision.len(), 1);
    assert!((kollision[0].max[1] - 0.875).abs() < 1.0e-6, "Kollisionshöhe 14/16");
    let umriss = world::outline_boxes(sand, &shapes);
    assert!((umriss[0].max[1] - 1.0).abs() < 1.0e-6, "Umriss bleibt der Vollwürfel");
    // getCameraCollisionShape ist bei Seelensand ausdruecklich wieder der Vollwuerfel.
    let kamera = world::camera_collision_boxes(sand, &shapes);
    assert!((kamera[0].max[1] - 1.0).abs() < 1.0e-6, "Kameraform ist der Vollwürfel");
}

// velocityMultiplier(0.4F) auf Seelensand UND Honig: die Endgeschwindigkeit im Gehen muss
// deutlich unter der auf Stein liegen. Der Faktor wirkt pro Tick auf die Geschwindigkeit,
// die Eingabe baut sie nur mit dem kleinen Beschleunigungsbetrag wieder auf.
#[test]
fn seelensand_und_honig_bremsen_das_gehen() {
    let shapes = world::BlockShapes::all_full_cubes();
    let tempo = |block: &str| {
        let world = world_with_floor_of(0, block);
        let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
        for _ in 0..60 {
            p.tick(&lauf_input(), &world, &shapes);
        }
        (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt()
    };
    let stein = tempo("stone");
    let sand = tempo("soul_sand");
    let honig = tempo("honey_block");
    assert!(sand < stein * 0.6, "Seelensand muss deutlich bremsen: {sand} gegen {stein}");
    assert!(honig < stein * 0.6, "Honig muss deutlich bremsen: {honig} gegen {stein}");
}

// Sprungkraft II: 0.42 + 0.1 * 2 in float, dann wie jeder Sprung Schwerkraft und Daempfung.
#[test]
fn sprungkraft_erhoeht_den_absprung() {
    let shapes = world::BlockShapes::all_full_cubes();
    let world = world_with_floor_of(0, "stone");
    let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
    let mut input = idle_input(0.0);
    input.jump = true;
    input.jump_boost = Some(1);
    p.tick(&input, &world, &shapes);
    let erwartet = (f64::from(0.42f32 + 0.1f32 * 2.0) - GRAVITY) * VERTICAL_DRAG;
    assert!((p.vel.y - erwartet).abs() < 1.0e-9, "{} statt {erwartet}", p.vel.y);
}

// Sanfter Fall: fallend nur 0.01 Schwerkraft -- die Endgeschwindigkeit liegt bei
// 0.01*0.98/0.02 = 0.49 statt ~3.9. Nach oben wirkt die volle Schwerkraft.
#[test]
fn sanfter_fall_bremst_nur_das_fallen() {
    let shapes = world::BlockShapes::all_full_cubes();
    let world = world_with_floor(0);
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 200.0, 8.0));
    let mut input = idle_input(0.0);
    input.slow_falling = true;
    for _ in 0..200 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.vel.y < 0.0 && p.vel.y > -0.5, "Endgeschwindigkeit mit Sanftem Fall: {}", p.vel.y);
    // Aufwaerts: dieselbe Rechnung wie ohne Effekt.
    let mut hoch = PlayerPhysics::new(DVec3::new(8.0, 100.0, 8.0));
    hoch.vel.y = 0.5;
    hoch.tick(&input, &world, &shapes);
    assert!((hoch.vel.y - (0.5 - GRAVITY) * VERTICAL_DRAG).abs() < 1.0e-9, "{}", hoch.vel.y);
}

// Schwebeeffekt I: statt Schwerkraft zieht es auf 0.05 hoch -- man steigt.
#[test]
fn schwebeeffekt_hebt_an() {
    let shapes = world::BlockShapes::all_full_cubes();
    let world = world_with_floor(0);
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 50.0, 8.0));
    let mut input = idle_input(0.0);
    input.levitation = Some(0);
    let start = p.pos.y;
    for _ in 0..40 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.pos.y > start + 1.0, "Schwebeeffekt muss heben: {} -> {}", start, p.pos.y);
    assert!(p.vel.y > 0.0 && p.vel.y < 0.05, "{}", p.vel.y);
}

// jumpVelocityMultiplier(0.5F) beim Honigblock: (float)(0.42 * 0.5) = 0.21 statt 0.42.
#[test]
fn honig_halbiert_die_absprunggeschwindigkeit() {
    let shapes = world::BlockShapes::all_full_cubes();
    let absprung = |block: &str| {
        let world = world_with_floor_of(0, block);
        let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
        let mut input = idle_input(0.0);
        input.jump = true;
        p.tick(&input, &world, &shapes);
        p.vel.y
    };
    // Nach dem Sprungtick hat travelInAir Schwerkraft und Daempfung schon angewandt:
    // (v - 0.08) * 0.98.
    let erwartet = |v: f64| (v - GRAVITY) * VERTICAL_DRAG;
    assert!((absprung("stone") - erwartet(JUMP_VELOCITY)).abs() < 1.0e-9);
    assert!(
        (absprung("honey_block") - erwartet(f64::from(JUMP_VELOCITY as f32 * 0.5))).abs() < 1.0e-9,
        "Honigsprung sollte halbe Absprunggeschwindigkeit haben, war {}",
        absprung("honey_block")
    );
}

// SlimeBlock.onEntityLand: die Standardfassung (Block.onEntityLand) nullt vel.y, der
// Schleimblock dreht sie stattdessen um -- man springt mit voller Fallgeschwindigkeit
// zurueck (Faktor 1.0 fuer LivingEntity).
#[test]
fn schleimblock_wirft_den_fall_zurueck() {
    let world = world_with_floor_of(0, "slime_block");
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 6.0, 8.0));
    let mut hoechster_rueckstoss: f64 = 0.0;
    for _ in 0..40 {
        p.tick(&idle_input(0.0), &world, &shapes);
        hoechster_rueckstoss = hoechster_rueckstoss.max(p.vel.y);
    }
    assert!(hoechster_rueckstoss > 0.3, "hätte zurückspringen müssen, größtes vel.y = {hoechster_rueckstoss}");
    assert!(p.pos.y > 1.5, "sollte nach dem Absprung wieder deutlich ueber dem Block sein, y={}", p.pos.y);
}

// bypassesLandingEffects() == isSneaking(): mit gehaltener Sneak-Taste faellt der
// Schleimblock auf die Standardfassung zurueck -- kein Absprung, ruhige Landung.
#[test]
fn schleichen_verhindert_den_schleim_absprung() {
    let world = world_with_floor_of(0, "slime_block");
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 6.0, 8.0));
    let mut input = idle_input(0.0);
    input.sneak = true;
    for _ in 0..40 {
        p.tick(&input, &world, &shapes);
        assert!(p.vel.y <= 0.0, "mit Sneak darf es keinen Rueckstoss geben, vel.y={}", p.vel.y);
    }
    assert!(p.on_ground, "sollte still auf dem Schleim liegen bleiben");
}

// SlimeBlock.onSteppedOn: horizontal stark gebremst, solange |vel.y| < 0.1. Der Effekt
// dreht das Vorzeichen der Reibung um -- Schleim ist mit 0.8 GLATTER als Stein (0.6), wer
// darueber laeuft, muesste also eigentlich SCHNELLER werden. Genau umgekehrt kommt es
// heraus, und nur onSteppedOn erklaert das.
#[test]
fn schleim_bremst_das_laufen_trotz_glatterer_reibung() {
    let shapes = world::BlockShapes::all_full_cubes();
    let tempo = |block: &str| {
        let world = world_with_floor_of(0, block);
        let mut p = ruht_auf(&world, &shapes, DVec3::new(8.0, 1.0, 8.0));
        for _ in 0..60 {
            p.tick(&lauf_input(), &world, &shapes);
        }
        (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt()
    };
    let schleim = tempo("slime_block");
    let stein = tempo("stone");
    assert!(schleim < stein * 0.7, "Schleim muss trotz 0.8er Reibung deutlich langsamer sein: {schleim} gegen {stein}");
}

// Und der Sneak-Zweig davon, ohne die Sneak-Verlangsamung der EINGABE als Stoergroesse:
// eine mitgegebene Geschwindigkeit ohne Bewegungseingabe ausrollen lassen. Ohne Sneak
// kommt zur Reibung (0.8 * 0.91) noch onSteppedOn dazu, mit Sneak nicht.
#[test]
fn schleichen_schaltet_die_schleim_bremse_ab() {
    let world = world_with_floor_of(0, "slime_block");
    let shapes = world::BlockShapes::all_full_cubes();
    let ausrollen = |sneak: bool| {
        let mut p = ruht_auf(&world, &shapes, DVec3::new(8.0, 1.0, 8.0));
        p.vel.x = 0.2;
        // Auf Schleim pendelt ein ruhender Spieler in einem winzigen Restsprung
        // (vel.y ~ -0.0016) -- das liegt UNTER der 0.003-Klemmung aus tickMovement, die
        // Bewegung dieses Ticks waere also y = 0 und damit (wie in Vanilla, s.
        // move_and_collide) gar kein Bodenkontakt. onSteppedOn braucht aber genau den.
        // Deshalb hier die normale Schwerkraft-Geschwindigkeit setzen: der Tick, in dem
        // man tatsaechlich auf den Schleim gedrueckt wird.
        p.vel.y = -GRAVITY * VERTICAL_DRAG;
        let mut input = idle_input(0.0);
        input.sneak = sneak;
        p.tick(&input, &world, &shapes);
        p.vel.x
    };
    let reibung = 0.8 * 0.91;
    assert!((ausrollen(true) - 0.2 * reibung).abs() < 1.0e-9, "mit Sneak nur die Reibung, war {}", ausrollen(true));
    // Der Unterschied ist genau der Faktor e = 0.4 + |vel.y| * 0.2 aus onSteppedOn. Der
    // Ruhewert von vel.y liegt knapp ueber 0, e also dicht ueber 0.4.
    let faktor = ausrollen(false) / ausrollen(true);
    assert!((0.4..=0.42).contains(&faktor), "onSteppedOn-Faktor unplausibel: {faktor}");
}

// ---------------------------------------------------------------------------------
// Bett: 9/16 hohe Matratze mit zwei Pfosten, und ein Absprung mit Faktor 0.66.
// Zahlen aus BedBlock (1.21.11), gegengeprueft an GrimACs CollisionData.BED und
// BlockProperties.getBlockBounceRestitution.
// ---------------------------------------------------------------------------------

// Ohne eigene Form bekaeme das Bett den Vollwuerfel-Fallback (das Bettmodell wird in
// Vanilla von einem BlockEntity gezeichnet, aus den Modelldateien kommt nichts).
// `ends_with("_bed")` als Erkennung: die Registry darf keinen anderen Block so nennen,
// sonst bekaeme der still eine Bettform und einen Absprung.
#[test]
fn nur_echte_betten_heissen_auf_bed() {
    let farben = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    let mut gefunden: Vec<&str> = Vec::new();
    for state in 0..world::state_count() as u32 {
        if let Some(name) = world::block_name(state) {
            if name.ends_with("_bed") && !gefunden.contains(&name) {
                gefunden.push(name);
            }
        }
    }
    assert_eq!(gefunden.len(), farben.len(), "unerwartete Blöcke auf _bed: {gefunden:?}");
    for farbe in farben {
        let name = format!("{farbe}_bed");
        assert!(gefunden.contains(&name.as_str()), "{name} fehlt");
    }
}

// Geprueft wird die FORM, nicht ihre Zerlegung in Kaesten: seit die Formen aus dem Jar
// kommen (world::vanilla_collision), zerlegt Vanillas VoxelShape das Bett in vier Kaesten
// statt in "Matratze plus zwei Pfosten". Dieselbe Form, andere Aufteilung -- ein Test auf
// die Kastenzahl wuerde nur die Zerlegung festschreiben. Deshalb Stichproben im Raum.
#[test]
fn bett_ist_neun_sechzehntel_hoch_mit_zwei_pfosten() {
    let shapes = world::BlockShapes::all_full_cubes();
    let drin = |state: u32, p: [f32; 3]| {
        world::collision_boxes(state, &shapes)
            .iter()
            .any(|b| (0..3).all(|i| p[i] > b.min[i] && p[i] < b.max[i]))
    };
    let bett = state_with("red_bed", &[("facing", "north"), ("part", "foot"), ("occupied", "false")]);

    // Die Matratze: 3/16 bis 9/16, ueber die ganze Zelle.
    assert!(drin(bett, [0.5, 6.0 / 16.0, 0.5]), "Mitte der Matratze fehlt");
    assert!(!drin(bett, [0.5, 10.0 / 16.0, 0.5]), "ueber 9/16 darf nichts mehr sein");
    assert!(!drin(bett, [0.5, 1.0 / 16.0, 0.5]), "unter der Matratze ist Luft, außer an den Pfosten");

    // foot + facing=north -> die Pfosten stehen im Sueden (hohes z), und zwar an beiden
    // Ecken bis hinunter auf 0.
    assert!(drin(bett, [1.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0]), "Pfosten Südwest fehlt");
    assert!(drin(bett, [15.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0]), "Pfosten Südost fehlt");
    assert!(!drin(bett, [1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0]), "im Norden darf kein Pfosten stehen");

    // Der Kopfteil desselben Betts traegt seine Pfosten am anderen Ende (niedriges z).
    let kopf = state_with("red_bed", &[("facing", "north"), ("part", "head"), ("occupied", "false")]);
    assert!(drin(kopf, [1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0]), "Pfosten Nordwest fehlt");
    assert!(drin(kopf, [15.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0]), "Pfosten Nordost fehlt");
    assert!(!drin(kopf, [1.0 / 16.0, 1.0 / 16.0, 15.0 / 16.0]), "im Süden darf kein Pfosten stehen");
}

// Und die Hoehe muss sich auch in der Physik zeigen: Fuesse auf 9/16, nicht auf 1.0.
#[test]
fn man_steht_auf_dem_bett_auf_neun_sechzehnteln() {
    let bett = state_with("white_bed", &[("facing", "east"), ("part", "foot"), ("occupied", "false")]);
    let world = world_with_floor_and_blocks(0, &[((9, 1, 9), bett)]);
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(9.5, 4.0, 9.5));
    let mut input = idle_input(0.0);
    input.sneak = true; // ohne Sneak springt man auf dem Bett, s. naechster Test
    for _ in 0..40 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.on_ground, "sollte auf dem Bett liegen");
    assert!((p.pos.y - (1.0 + 9.0 / 16.0)).abs() < 1.0e-9, "Fuesse auf 9/16 ueber der Zelle, y={}", p.pos.y);
}

// BedBlock.onEntityLand: wie der Schleimblock, nur mit 0.66 -- und ebenso per Sneak
// abschaltbar (bypassesLandingEffects).
#[test]
fn bett_wirft_mit_zwei_dritteln_zurueck() {
    let bett = state_with("blue_bed", &[("facing", "north"), ("part", "foot"), ("occupied", "false")]);
    // Eine kleine Bettflaeche, damit der Spieler beim Auf und Ab nicht daneben landet.
    let cells: Vec<((i32, i32, i32), u32)> = (-2..=2).flat_map(|dx| (-2..=2).map(move |dz| ((9 + dx, 1, 9 + dz), bett))).collect();
    let world = world_with_floor_and_blocks(0, &cells);
    let shapes = world::BlockShapes::all_full_cubes();

    let absprung = |sneak: bool| {
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
        let mut input = idle_input(0.0);
        input.sneak = sneak;
        let mut vor_der_landung = 0.0;
        let mut hoechster: f64 = 0.0;
        for _ in 0..30 {
            let vorher = p.vel.y;
            p.tick(&input, &world, &shapes);
            if p.vel.y > 0.0 && hoechster == 0.0 {
                vor_der_landung = vorher;
            }
            hoechster = hoechster.max(p.vel.y);
        }
        (vor_der_landung, hoechster)
    };
    let (vorher, rueckstoss) = absprung(false);
    assert!(rueckstoss > 0.0, "ohne Sneak muss das Bett zurückwerfen");
    // Der Rueckstoss ist -vel.y * 0.66, danach wirken Schwerkraft und Daempfung des Ticks.
    let erwartet = (-vorher * f64::from(0.66f32) - GRAVITY) * VERTICAL_DRAG;
    assert!((rueckstoss - erwartet).abs() < 1.0e-9, "Rückstoß {rueckstoss} statt {erwartet}");
    assert_eq!(absprung(true).1, 0.0, "mit Sneak kein Absprung");
}

// Schlafen: die Pose ist 0.2 x 0.2 mit 0.2 Augenhoehe (PlayerLikeEntity.POSE_DIMENSIONS),
// und sie hat Vorrang -- auch mit gedrueckter Schleichtaste wird nicht in die Hockpose
// gewechselt. Die Bewegungssperre selbst sitzt im Aufrufer (isImmobile, s. main.rs).
#[test]
fn schlafpose_ist_zwei_zehntel_hoch_und_hat_vorrang() {
    let world = world_with_floor_of(0, "stone");
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.5, 1.0, 8.5));
    p.refresh_on_ground(&world, &shapes);
    p.sleeping = true;
    let mut input = idle_input(0.0);
    input.sneak = true;
    p.tick(&input, &world, &shapes);
    assert_eq!(p.current_height(), SLEEPING_HEIGHT);
    assert_eq!(p.eye_height(), SLEEPING_HEIGHT as f32);
    assert!(!p.crouching, "SLEEPING geht der Hockpose vor");
    assert!(!p.swimming);

    // Und wach ist wieder alles normal.
    p.sleeping = false;
    p.tick(&input, &world, &shapes);
    assert_eq!(p.current_height(), CROUCHING_HEIGHT_F64);
}

// ---------------------------------------------------------------------------------
// Klettern: Leiter, Ranken, Geruest, Falltuer-ueber-Leiter.
// Zahlen aus LivingEntity.applyClimbingSpeed/applyMovementInput + ScaffoldingBlock/
// LadderBlock (1.21.11), gegengeprueft an GrimACs
// PredictionEngineNormal.handleOnClimbable und CollisionData.
// ---------------------------------------------------------------------------------

fn state_with(name: &str, props: &[(&str, &str)]) -> u32 {
    (0..world::state_count() as u32)
        .find(|&s| world::block_name(s) == Some(name) && props.iter().all(|(k, v)| world::state_props::property(s, k) == Some(*v)))
        .unwrap_or_else(|| panic!("{name} mit {props:?} unbekannt"))
}

// Steinboden bei floor_y, darauf einzeln gesetzte Bloecke.
fn world_with_floor_and_blocks(floor_y: i32, cells: &[((i32, i32, i32), u32)]) -> World {
    let stein = state_of_block("stone");
    let mut w = World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                let base_y = world::MIN_Y + i * 16;
                if floor_y >= base_y && floor_y < base_y + 16 {
                    let ly = floor_y - base_y;
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((ly * 16 + z) * 16 + x) as usize] = stein;
                        }
                    }
                }
                for ((bx, by, bz), state) in cells {
                    if by >= &base_y && by < &(base_y + 16) && bx.div_euclid(16) == cx && bz.div_euclid(16) == cz {
                        let (lx, ly, lz) = (bx.rem_euclid(16), by - base_y, bz.rem_euclid(16));
                        states[((ly * 16 + lz) * 16 + lx) as usize] = *state;
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            w.insert_column(cx, cz, sections);
        }
    }
    w
}

fn leiter_welt() -> (World, world::BlockShapes) {
    // Leiter mit facing=north: die 3/16-Platte liegt bei z 13..16 der Zelle, der Spieler
    // steht mit seiner 0.6er Box (z 9.2..9.8) davor in derselben Zelle.
    let leiter = state_with("ladder", &[("facing", "north"), ("waterlogged", "false")]);
    let cells: Vec<_> = (1..8).map(|y| ((9, y, 9), leiter)).collect();
    (world_with_floor_and_blocks(0, &cells), world::BlockShapes::all_full_cubes())
}

// LadderBlock.getOutlineShape: createCuboidZShape(16, 13, 16), pro Richtung gedreht. Eigene
// Kollisionsform gibt es nicht, Umriss und Kollision sind dasselbe -- und beide duerfen
// NICHT aus dem (dickenlosen) Modell kommen.
#[test]
fn leiter_hat_die_vanilla_kollisionsform() {
    let shapes = world::BlockShapes::all_full_cubes();
    let boxen = |facing: &str| {
        let s = state_with("ladder", &[("facing", facing), ("waterlogged", "false")]);
        let k = world::collision_boxes(s, &shapes);
        assert_eq!(k, world::outline_boxes(s, &shapes), "Umriss und Kollision sind bei der Leiter dieselbe Form");
        assert_eq!(k.len(), 1);
        (k[0].min, k[0].max)
    };
    assert_eq!(boxen("north"), ([0.0, 0.0, 13.0 / 16.0], [1.0, 1.0, 1.0]));
    assert_eq!(boxen("south"), ([0.0, 0.0, 0.0], [1.0, 1.0, 3.0 / 16.0]));
    assert_eq!(boxen("west"), ([13.0 / 16.0, 0.0, 0.0], [1.0, 1.0, 1.0]));
    assert_eq!(boxen("east"), ([0.0, 0.0, 0.0], [3.0 / 16.0, 1.0, 1.0]));
}

// Mit gehaltener Sprungtaste steigt man auf: applyMovementInput setzt vel.y auf 0.2, davon
// bleiben nach Schwerkraft und Daempfung (0.2 - 0.08) * 0.98 uebrig.
#[test]
fn leiter_wird_mit_sprungtaste_erklettert() {
    let (world, shapes) = leiter_welt();
    let mut p = PlayerPhysics::new(DVec3::new(9.5, 1.0, 9.5));
    p.refresh_on_ground(&world, &shapes);
    let mut input = idle_input(0.0);
    input.jump = true;
    let start = p.pos.y;
    for _ in 0..20 {
        p.tick(&input, &world, &shapes);
    }
    assert!(p.pos.y > start + 1.5, "haette die Leiter hochsteigen muessen, y={} statt >{}", p.pos.y, start + 1.5);
    let erwartet = (CLIMB_UP_SPEED - GRAVITY) * VERTICAL_DRAG;
    assert!((p.vel.y - erwartet).abs() < 1.0e-9, "Steiggeschwindigkeit {} statt {erwartet}", p.vel.y);
}

// Ohne Eingabe rutscht man mit genau 0.15 pro Tick hinunter (y wird auf -0.15 begrenzt,
// BEVOR die Bewegung stattfindet) -- deutlich langsamer als der freie Fall.
#[test]
fn leiter_bremst_das_hinabrutschen_auf_015() {
    let (world, shapes) = leiter_welt();
    let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
    for _ in 0..5 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    let vorher = p.pos.y;
    p.tick(&idle_input(0.0), &world, &shapes);
    let strecke = vorher - p.pos.y;
    assert!((strecke - CLIMB_SPEED).abs() < 1.0e-9, "Rutschstrecke {strecke} statt {CLIMB_SPEED}");
    // applyClimbingSpeed ruft onLanding() VOR der Bewegung, Entity.fall() schreibt danach
    // die Strecke DIESES Ticks wieder hinein. Die Fallhoehe kann also nie ueber einen
    // einzelnen Kletter-Tick hinauswachsen -- deshalb kein Fallschaden von der Leiter.
    assert!(p.fall_distance <= CLIMB_SPEED + 1.0e-9, "Fallhöhe darf sich nicht aufsummieren, war {}", p.fall_distance);
}

// isHoldingOntoLadder() == isSneaking(): schleichend haelt man sich fest und steht still.
#[test]
fn schleichen_haelt_an_der_leiter_fest() {
    let (world, shapes) = leiter_welt();
    let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
    let mut input = idle_input(0.0);
    input.sneak = true;
    for _ in 0..3 {
        p.tick(&input, &world, &shapes);
    }
    let gehalten = p.pos.y;
    for _ in 0..20 {
        p.tick(&input, &world, &shapes);
    }
    assert_eq!(p.pos.y, gehalten, "mit Sneak darf sich an der Leiter nichts bewegen");
}

// Ranken sind derselbe Mechanismus (BlockTags.CLIMBABLE), obwohl sie gar keine
// Kollisionsbox haben -- man klettert IN ihnen, nicht an ihnen.
#[test]
fn ranken_werden_genauso_erklettert() {
    let ranke = state_of_block("vine");
    let cells: Vec<_> = (1..8).map(|y| ((9, y, 9), ranke)).collect();
    let world = world_with_floor_and_blocks(0, &cells);
    let shapes = world::BlockShapes::all_full_cubes();
    assert!(world::collision_boxes(ranke, &shapes).is_empty(), "Ranken kollidieren in Vanilla nicht");
    let mut p = PlayerPhysics::new(DVec3::new(9.5, 6.0, 9.5));
    // Ein paar Ticks, bis die Schwerkraft die Geschwindigkeit ueberhaupt an die Klemmung
    // herangefuehrt hat -- vorher ist die Bewegung noch langsamer als 0.15.
    for _ in 0..5 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    let vorher = p.pos.y;
    p.tick(&idle_input(0.0), &world, &shapes);
    assert!((vorher - p.pos.y - CLIMB_SPEED).abs() < 1.0e-9, "auch in Ranken gilt die 0.15er Bremse");
}

// Der Fall, der bei Ranken anders liegt als bei der Leiter: Ranken haben GAR KEINE
// Kollisionsbox, der Aufstieg haengt in Vanilla aber an `horizontalCollision`. Die noetige
// Kollision liefert die Wand HINTER der Ranke -- der Spieler steht in der Rankenzelle und
// drueckt gegen den Stein dahinter. Ohne diesen Test waere unbelegt, ob das bei uns
// zusammenpasst.
#[test]
fn an_der_wandranke_klettert_man_durch_druecken_hoch() {
    let ranke = state_of_block("vine");
    let stein = state_of_block("stone");
    let mut cells: Vec<((i32, i32, i32), u32)> = Vec::new();
    for y in 1..10 {
        cells.push(((9, y, 9), ranke)); // Ranke, in der der Spieler steht
        cells.push(((10, y, 9), stein)); // die tragende Wand oestlich davon
    }
    let world = world_with_floor_and_blocks(0, &cells);
    let shapes = world::BlockShapes::all_full_cubes();

    let mut p = PlayerPhysics::new(DVec3::new(9.5, 1.0, 9.5));
    p.refresh_on_ground(&world, &shapes);
    let start = p.pos.y;
    // Nur nach Osten druecken, NICHT springen -- der Aufstieg muss allein aus der
    // Wandkollision kommen.
    for _ in 0..30 {
        p.tick(&lauf_input(), &world, &shapes);
    }
    assert!(p.horizontal_collision, "Testaufbau: sollte an der Wand anliegen");
    assert!(p.pos.y > start + 2.0, "haette an der Ranke hochklettern muessen, y={} statt >{}", p.pos.y, start + 2.0);
    let erwartet = (CLIMB_UP_SPEED - GRAVITY) * VERTICAL_DRAG;
    assert!((p.vel.y - erwartet).abs() < 1.0e-9, "Steiggeschwindigkeit {} statt {erwartet}", p.vel.y);
}

// Eine OFFENE Falltuer ueber einer gleich ausgerichteten Leiter zaehlt als kletterbar
// (canEnterTrapdoor) -- geschlossen oder falsch gedreht dagegen nicht.
#[test]
fn offene_falltuer_ueber_leiter_ist_kletterbar() {
    let shapes = world::BlockShapes::all_full_cubes();
    let leiter = state_with("ladder", &[("facing", "north"), ("waterlogged", "false")]);
    let luke = |offen: &str, facing: &str| {
        state_with(
            "oak_trapdoor",
            &[("open", offen), ("facing", facing), ("half", "bottom"), ("powered", "false"), ("waterlogged", "false")],
        )
    };
    let pruefe = |luken_state: u32| {
        let world = world_with_floor_and_blocks(0, &[((9, 1, 9), leiter), ((9, 2, 9), luken_state)]);
        let mut p = PlayerPhysics::new(DVec3::new(9.5, 2.5, 9.5));
        p.sneak_key = false;
        p.is_climbing(&world)
    };
    assert!(pruefe(luke("true", "north")), "offen und gleich ausgerichtet: kletterbar");
    assert!(!pruefe(luke("false", "north")), "geschlossen: nicht kletterbar");
    assert!(!pruefe(luke("true", "south")), "andere Richtung als die Leiter: nicht kletterbar");
    let _ = shapes;
}

// Geruest: oben drauf traegt die Deckplatte (Fuesse bei y+1), beim Schleichen faellt sie
// weg und man steigt mit Kletter-Geschwindigkeit hinunter -- und zwar OHNE das
// Festhalten, das die Leiter beim Schleichen bewirkt (ScaffoldingBlock ist davon
// ausdruecklich ausgenommen).
#[test]
fn geruest_traegt_oben_und_laesst_beim_schleichen_hinab() {
    let geruest = state_with("scaffolding", &[("bottom", "false"), ("waterlogged", "false")]);
    let cells: Vec<_> = (1..5).map(|y| ((9, y, 9), geruest)).collect();
    let world = world_with_floor_and_blocks(0, &cells);
    let shapes = world::BlockShapes::all_full_cubes();

    // Oben aufsetzen: die Deckplatte des obersten Geruests (Zelle y=4) endet bei y=5.
    let mut p = PlayerPhysics::new(DVec3::new(9.5, 5.0, 9.5));
    p.refresh_on_ground(&world, &shapes);
    for _ in 0..5 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    assert!(p.on_ground && (p.pos.y - 5.0).abs() < 1.0e-9, "sollte oben auf dem Geruest stehen, y={}", p.pos.y);

    // Schleichen: Deckplatte weg, kontrolliert hinabsteigen.
    let mut input = idle_input(0.0);
    input.sneak = true;
    let mut voriges_y = p.pos.y;
    let mut schritte = Vec::new();
    for _ in 0..10 {
        p.tick(&input, &world, &shapes);
        schritte.push(voriges_y - p.pos.y);
        voriges_y = p.pos.y;
    }
    assert!(p.pos.y < 4.0, "haette absteigen muessen, y={}", p.pos.y);
    let groesster = schritte.iter().copied().fold(0.0f64, f64::max);
    assert!(groesster <= CLIMB_SPEED + 1.0e-9, "nie schneller als Kletter-Geschwindigkeit, war {groesster}");
}

// DER Grim-Fall, der das Standblock-Modell erzwungen hat (Honig flaggte beim Betreten
// und Verlassen): ein Honigblock im Steinboden ist eine 1/16 tiefe
// Mulde. Wer mit einem Fuss darueber steht, aber noch vom Stein getragen wird, darf NICHT
// gebremst werden -- Vanilla und Grim fragen den Standblock, nicht floor(y - 0.5).
#[test]
fn honigmulde_bremst_erst_wenn_man_wirklich_darauf_steht() {
    let honig = state_of_block("honey_block");
    let stein = state_of_block("stone");
    let mut world = World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if world::MIN_Y + i * 16 <= 0 && 0 < world::MIN_Y + (i + 1) * 16 {
                    let ly = (0 - world::MIN_Y).rem_euclid(16);
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((ly * 16 + z) * 16 + x) as usize] = if x == 10 && cx == 0 { honig } else { stein };
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            world.insert_column(cx, cz, sections);
        }
    }
    let shapes = world::BlockShapes::all_full_cubes();

    // Mit einem Fuss ueber der Mulde (Box 9.8..10.4), getragen vom Stein bei x=9.
    let mut p = PlayerPhysics::new(DVec3::new(10.1, 1.0, 8.5));
    p.refresh_on_ground(&world, &shapes);
    assert!(p.on_ground);
    assert_eq!(p.supporting_block, Some((9, 0, 8)), "der Stein trägt, nicht der Honig");
    assert_eq!(p.velocity_multiplier(&world), 1.0, "über der Mulde stehend darf nichts bremsen");
    assert_eq!(p.jump_multiplier(&world), 1.0);
    // Die naive Abfrage haette hier den Honig erwischt -- genau das war der Flag.
    assert_eq!(world::block_name(world.block_at(10, 0, 8)), Some("honey_block"));

    // Und wirklich auf dem Honig stehend greift der Faktor dann auch.
    let mut auf_honig = PlayerPhysics::new(DVec3::new(10.5, 0.9375, 8.5));
    auf_honig.refresh_on_ground(&world, &shapes);
    assert_eq!(auf_honig.supporting_block, Some((10, 0, 8)));
    assert_eq!(auf_honig.velocity_multiplier(&world), SOUL_SAND_HONEY_SPEED);
    assert_eq!(auf_honig.jump_multiplier(&world), 0.5);
}

// Ohne Standblock (in der Luft) bleibt der naive Fallback -- sonst haette ein Sprung ueber
// die Mulde gar keinen Bezugsblock mehr.
#[test]
fn ohne_standblock_gilt_der_naive_fallback() {
    let world = world_with_floor_of(0, "stone");
    let shapes = world::BlockShapes::all_full_cubes();
    let mut p = PlayerPhysics::new(DVec3::new(8.3, 10.0, 8.7));
    p.tick(&idle_input(0.0), &world, &shapes);
    assert!(!p.on_ground);
    assert_eq!(p.supporting_block, None);
    assert_eq!(p.velocity_affecting_pos(&world), (8, (p.pos.y - f64::from(0.500001f32)).floor() as i32, 8));
}

// HoneyBlock.isSliding/updateSlidingVelocity: an der SEITE einer Honigwand rutscht man mit
// konstanter Geschwindigkeit herunter statt frei zu fallen, und die Fallhoehe bleibt bei 0
// (onLanding -> kein Fallschaden).
#[test]
fn honigwand_bremst_den_fall_auf_rutschgeschwindigkeit() {
    // Wand aus Honig bei x >= 9, Spieler faellt links davor.
    let honig = state_of_block("honey_block");
    let mut world = World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                let section_base_y = world::MIN_Y + i * 16;
                for ly in 0..16 {
                    let wy = section_base_y + ly;
                    if (1..40).contains(&wy) && cx == 0 {
                        for z in 0..16 {
                            states[((ly * 16 + z) * 16 + 9) as usize] = honig;
                        }
                    }
                }
                sections.push(ChunkSection::from_states(states));
            }
            world.insert_column(cx, cz, sections);
        }
    }
    let shapes = world::BlockShapes::all_full_cubes();
    // Direkt an die Wand gedrueckt: die Honigkollision ist um 1/16 eingerueckt, die
    // Spielerbox steht also ein Stueck IN der Honigzelle -- genau das laesst das
    // Rutschen ueberhaupt greifen.
    // Ein Tausendstel Luft zur Wand: nach einer echten Kollisionsbewegung bleibt genau so
    // ein Spalt stehen, und der Test haengt nicht daran, ob eine exakt buendige Box als
    // Beruehrung zaehlt. Der Mittelpunktsabstand (0.7385) liegt weiter ueber der
    // Rutschschwelle 0.7375.
    let mut p = PlayerPhysics::new(DVec3::new(9.0 + 0.0625 - 0.3 - 0.001, 30.0, 8.5));
    // Erst ein paar Ticks fallen lassen, damit die Rutschbedingung (< -0.08) greift.
    for _ in 0..10 {
        p.tick(&idle_input(0.0), &world, &shapes);
    }
    // getNewVelocityY(-0.05) = (-0.05 - 0.08) * 0.98F
    let rutsch = (-0.05 - 0.08) * f64::from(0.98f32);
    assert!((p.vel.y - rutsch).abs() < 1.0e-9, "sollte auf Rutschgeschwindigkeit geklemmt sein, vel.y={}", p.vel.y);
    assert_eq!(p.fall_distance, 0.0, "Rutschen ruft onLanding, die Fallhöhe bleibt 0");
    // Und ohne Wand faellt derselbe Spieler in derselben Zeit viel schneller.
    let leer = world_with_floor_of(-40, "stone");
    let mut frei = PlayerPhysics::new(DVec3::new(8.5, 30.0, 8.5));
    for _ in 0..10 {
        frei.tick(&idle_input(0.0), &leer, &shapes);
    }
    assert!(frei.vel.y < rutsch * 3.0, "freier Fall muss viel schneller sein: {}", frei.vel.y);
}

// MOVEMENT_EFFICIENCY (Seelenlaeufigkeit auf Seelensand): Mth.lerp(eff, Blockfaktor, 1)
// in float. Ohne Attribut bleibt die Bremse 0.4F.
#[test]
fn seelenlaeufigkeit_hebt_die_seelensand_bremse_auf() {
    let shapes = world::BlockShapes::all_full_cubes();
    let world = world_with_floor_of(0, "soul_sand");
    let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.5, 2.0, 8.5));
    assert_eq!(p.velocity_multiplier(&world), SOUL_SAND_HONEY_SPEED);
    p.body.movement_efficiency = 1.0;
    assert_eq!(p.velocity_multiplier(&world), 1.0);
    p.body.movement_efficiency = 0.5;
    assert_eq!(p.velocity_multiplier(&world), f64::from(0.4f32 + 0.5f32 * (1.0f32 - 0.4f32)));
}

// SCALE: Breite, Hoehe und Augenhoehe der Pose mal den float (EntityDimensions.scale).
#[test]
fn groesse_skaliert_box_und_augenhoehe() {
    let mut p = PlayerPhysics::new(DVec3::ZERO);
    assert_eq!(p.width(), WIDTH_F64);
    assert_eq!(p.current_height(), HEIGHT_F64);
    p.body.scale = 2.0;
    assert_eq!(p.width(), f64::from(PLAYER_WIDTH * 2.0));
    assert_eq!(p.current_height(), f64::from(PLAYER_HEIGHT * 2.0));
    assert_eq!(p.eye_height(), EYE_HEIGHT * 2.0);
}

// JUMP_STRENGTH: (float)Attribut * Blockfaktor, dann Schwerkraft und Daempfung des Ticks.
#[test]
fn sprungkraft_vom_server() {
    let shapes = world::BlockShapes::all_full_cubes();
    let world = world_with_floor_of(0, "stone");
    let mut p = steht_still_auf(&world, &shapes, DVec3::new(8.0, 4.0, 8.0));
    p.body.jump_strength = 0.84;
    let mut input = idle_input(0.0);
    input.jump = true;
    p.tick(&input, &world, &shapes);
    let erwartet = (f64::from(0.84f32) - GRAVITY) * VERTICAL_DRAG;
    assert!((p.vel.y - erwartet).abs() < 1.0e-9, "{} statt {}", p.vel.y, erwartet);
}

// GRAVITY: der Fall rechnet mit dem Attribut statt mit 0.08.
#[test]
fn schwerkraft_vom_server() {
    let shapes = world::BlockShapes::all_full_cubes();
    let world = world_with_floor_of(0, "stone");
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 20.0, 8.0));
    p.body.gravity = 0.04;
    p.tick(&idle_input(0.0), &world, &shapes);
    assert!((p.vel.y - -0.04 * VERTICAL_DRAG).abs() < 1.0e-12, "{}", p.vel.y);
}
