use super::*;
use world::ChunkSection;

// Zustands-IDs fuer die Testwelten unten. Frueher waren das die willkuerlichen IDs 1 und
// 2, deren Form der Test sich selbst zurechtlegte. Seit die Kollisionsformen aus dem Jar
// kommen (world::vanilla_collision) geht das nicht mehr -- und es soll auch nicht: ein
// Test, der einem Block eine Wunschform unterschiebt, prueft nicht mehr das, was im Spiel
// passiert. Deshalb echte Bloecke.
const SOLID: u32 = 1; // stone -- in dieser Registry ohnehin der erste Vollwuerfel
fn slab_state() -> u32 {
    (0..world::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("stone_slab") && world::state_props::property(s, "type") == Some("bottom"))
        .expect("stone_slab[type=bottom] unbekannt")
}

// Boden bei floor_y ueber 5x5 Spalten, darauf ab Welt-X >= ledge_x eine Kante aus
// `ledge_state` -- also genau die Situation "man laeuft gegen eine Stufe".
fn world_with_ledge(floor_y: i32, ledge_x: i32, ledge_state: u32) -> World {
    let mut w = World::new();
    let floor_section = (floor_y - world::MIN_Y).div_euclid(16);
    let floor_local = (floor_y - world::MIN_Y).rem_euclid(16);
    let ledge_y = floor_y + 1;
    let ledge_section = (ledge_y - world::MIN_Y).div_euclid(16);
    let ledge_local = (ledge_y - world::MIN_Y).rem_euclid(16);
    for cx in -2..=2 {
        for cz in -2..=2 {
            let mut sections = Vec::new();
            for i in 0..24 {
                let mut states = vec![0u32; 16 * 16 * 16];
                if i == floor_section {
                    for x in 0..16 {
                        for z in 0..16 {
                            states[((floor_local * 16 + z) * 16 + x) as usize] = SOLID;
                        }
                    }
                }
                if i == ledge_section {
                    for x in 0..16 {
                        if cx * 16 + x >= ledge_x {
                            for z in 0..16 {
                                states[((ledge_local * 16 + z) * 16 + x) as usize] = ledge_state;
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

// Die Formen kommen jetzt aus dem Jar-Dump, die Tabelle muss nichts mehr beisteuern.
fn shapes_with_slab() -> world::BlockShapes {
    world::BlockShapes::all_full_cubes()
}

fn walk_east(yaw_rad: f32) -> Input {
    Input {
        forward: 0.0,
        strafe: 1.0,
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

// Der eigentliche Zweck des Features: eine halbhohe Stufe (0.5 < STEP_HEIGHT) wird im
// Laufen ueberwunden, OHNE zu springen. Vorher blieb man daran haengen wie an einer Wand.
#[test]
fn halbe_stufe_wird_ohne_sprung_erstiegen() {
    let world = world_with_ledge(0, 10, slab_state());
    let shapes = shapes_with_slab();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let input = walk_east(0.0); // yaw 0 = Blick +Z, strafe schiebt nach +X
    for _ in 0..80 {
        p.tick(&input, &world, &shapes);
    }
    // Oberkante der Stufe liegt bei y = 1 (Blockzelle) + 0.5 = 1.5.
    assert!((p.pos.y - 1.5).abs() < 1.0e-6, "haette auf der Stufe stehen muessen, y={}", p.pos.y);
    assert!(p.pos.x > 10.5, "haette ueber die Kante hinauslaufen muessen, x={}", p.pos.x);
    assert!(p.on_ground, "nach dem Schritt steht man wieder am Boden");
}

// Gegenprobe: ein GANZER Block ist 1.0 hoch und damit ueber STEP_HEIGHT (0.6) -- da muss
// man weiterhin springen. Ohne diese Grenze waere aus dem Feature ein Wandlauf geworden.
#[test]
fn ganzer_block_wird_nicht_erstiegen() {
    let world = world_with_ledge(0, 10, SOLID);
    let shapes = shapes_with_slab();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let input = walk_east(0.0);
    for _ in 0..80 {
        p.tick(&input, &world, &shapes);
    }
    assert!((p.pos.y - 1.0).abs() < 1.0e-6, "darf nicht hochsteigen, y={}", p.pos.y);
    assert!(p.pos.x < 10.0, "muss vor dem Block stehen bleiben, x={}", p.pos.x);
    assert!(p.horizontal_collision, "der Block muss weiterhin horizontal blockieren");
}

// Die Weltgrenze haelt wie eine Wand: Grenze 20 um den Ursprung, Ostkante bei x = 10. Der
// Spieler (halbe Breite 0.3) bleibt mit der Mitte bei 9.7 stehen und steigt nicht hinauf.
#[test]
fn weltgrenze_haelt_wie_eine_wand() {
    let mut world = world_with_ledge(0, 40, SOLID);
    world.border.apply(protocol::packets::BorderUpdate::Center(0.0, 0.0));
    world.border.apply(protocol::packets::BorderUpdate::Size(20.0));
    let shapes = shapes_with_slab();
    let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.0));
    let input = walk_east(0.0);
    for _ in 0..80 {
        p.tick(&input, &world, &shapes);
    }
    assert!((p.pos.x - 9.7).abs() < 1.0e-6, "muss an der Grenze stehen, x={}", p.pos.x);
    assert!((p.pos.y - 1.0).abs() < 1.0e-6, "die Grenze ist keine Stufe, y={}", p.pos.y);
    assert!(p.horizontal_collision);
}

// Stepping haengt an "am Boden ODER in diesem Tick gelandet". Mitten im Aufwaertssprung
// gegen die Kante gilt beides nicht -- sonst koennte man sich an einer Wand hochziehen.
#[test]
fn kein_schritt_im_aufwaertsflug() {
    let world = world_with_ledge(0, 10, slab_state());
    let shapes = shapes_with_slab();
    let box_min = DVec3::new(9.7, 1.4, 8.0);
    let box_max = box_min + DVec3::new(f64::from(PLAYER_WIDTH), f64::from(PLAYER_HEIGHT), f64::from(PLAYER_WIDTH));
    // Nach oben UND nach +X gegen die Stufe, ohne Bodenkontakt.
    let movement = DVec3::new(0.5, 0.2, 0.0);
    let moved = adjust_movement_with_step(box_min, box_max, movement, false, &world, &shapes, world::ShapeContext::ABSENT, STEP_HEIGHT);
    assert!(moved.y <= movement.y + 1.0e-9, "ohne Bodenkontakt darf kein Schritt angehoben werden");
}

// STEP_HEIGHT 1.0 (Plugin): ein ganzer Block wird im Laufen erstiegen, mit der Vorgabe
// 0.6 bleibt man davor stehen.
#[test]
fn stufenhoehe_vom_server() {
    let world = world_with_ledge(0, 10, SOLID);
    let shapes = shapes_with_slab();
    let lauf = |step: f64| {
        let mut p = PlayerPhysics::new(DVec3::new(8.0, 1.0, 8.5));
        p.body.set_step_height(step);
        for _ in 0..40 {
            p.tick(&walk_east(0.0), &world, &shapes);
        }
        p.pos.y
    };
    assert!((lauf(STEP_HEIGHT) - 1.0).abs() < 1.0e-6);
    assert!((lauf(1.0) - 2.0).abs() < 1.0e-6, "{}", lauf(1.0));
}

// collectStepHeights: Kandidaten sind die Y-Kanten der Kollisionsboxen relativ zur
// Fussunterkante, aufsteigend, ohne Dubletten, ohne alles ueber der Stufenhoehe -- und
// ohne die Hoehe, welche die Basisbewegung ohnehin schon erreicht.
#[test]
fn stufenhoehen_werden_gefiltert_und_sortiert() {
    let boxes = [
        Aabb { min: DVec3::new(0.0, 0.0, 0.0), max: DVec3::new(1.0, 0.5, 1.0) },
        Aabb { min: DVec3::new(1.0, 0.0, 0.0), max: DVec3::new(2.0, 0.25, 1.0) },
        // Ueber der Stufenhoehe -> faellt komplett raus.
        Aabb { min: DVec3::new(2.0, 0.0, 0.0), max: DVec3::new(3.0, 0.9, 1.0) },
    ];
    let heights = collect_step_heights(0.0, &boxes, STEP_HEIGHT, -1.0);
    assert_eq!(heights, vec![0.0, 0.25, 0.5], "sortiert, entdoppelt, oberhalb 0.6 verworfen");
    // base_dy = 0.5 schliesst genau diese Hoehe aus.
    let heights = collect_step_heights(0.0, &boxes, STEP_HEIGHT, 0.5);
    assert_eq!(heights, vec![0.0, 0.25]);
}

// Direction.getCollisionOrder: Y immer zuerst, dann die betragsmaessig KLEINERE
// Horizontale, zuletzt die groessere. Direkt an der Regel geprueft statt ueber eine
// Geometrie -- in einer konstruierten Ecke haengt das Ergebnis an zu vielen anderen
// Groessen, um die Reihenfolge sauber nachzuweisen.
#[test]
fn achsenreihenfolge_folgt_vanilla() {
    use Axis::{X, Y, Z};
    assert_eq!(collision_order(DVec3::new(0.5, 0.0, 0.1)), [Y, X, Z], "|x| > |z| -> Y,X,Z");
    assert_eq!(collision_order(DVec3::new(0.1, 0.0, 0.5)), [Y, Z, X], "|x| < |z| -> Y,Z,X");
    // Vorzeichen zaehlen nicht, nur der Betrag.
    assert_eq!(collision_order(DVec3::new(-0.1, 0.0, 0.5)), [Y, Z, X]);
    assert_eq!(collision_order(DVec3::new(0.5, 0.0, -0.1)), [Y, X, Z]);
    // Gleichstand faellt wie in Vanilla auf den else-Zweig (Y,X,Z).
    assert_eq!(collision_order(DVec3::new(0.3, 0.0, 0.3)), [Y, X, Z]);
}
