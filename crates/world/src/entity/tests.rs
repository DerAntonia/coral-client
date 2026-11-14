use super::*;

// Seitwaerts laufende Spieler duerfen keine komischen Sidesteps machen.
// Ein Spieler, der nach Sueden schaut (Yaw 0) und seitwaerts nach Osten laeuft: in Vanilla
// dreht sich der KOERPER Richtung Laufweg (-90), bis der Kopf 50 Grad verdreht ist -- er
// zeigt also nicht mehr stur in Blickrichtung.
#[test]
fn seitwaerts_laufender_spieler_dreht_den_koerper_mit() {
    let mut entities = Entities::new();
    entities.spawn(3, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    for schritt in 1..=20 {
        entities.set_position(3, f64::from(schritt) * 0.2, 64.0, 0.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    let e = entities.get(3).unwrap();
    assert!((e.render_yaw(1.0) - -50.0).abs() < 0.5, "Koerper bei -50 Grad erwartet, war {}", e.render_yaw(1.0));
    // Rueckwaerts laufen dreht den Koerper NICHT um: Laufrichtung Norden, Blick Sueden.
    let mut entities = Entities::new();
    entities.spawn(4, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    for schritt in 1..=20 {
        entities.set_position(4, 0.0, 64.0, -f64::from(schritt) * 0.2);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    assert!(entities.get(4).unwrap().render_yaw(1.0).abs() < 0.5);
}

// Kein Lebewesen (Boot, Lore, Display): die Drehung bleibt die vom Server.
#[test]
fn nicht_lebewesen_drehen_sich_wie_vom_server() {
    let mut entities = Entities::new();
    entities.spawn(5, [0u8; 16], typ("minecart"), (0.0, 64.0, 0.0), 30.0, 0.0, (0.0, 0.0, 0.0));
    for schritt in 1..=10 {
        entities.set_position(5, f64::from(schritt) * 0.3, 64.0, 0.0);
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    assert!((entities.get(5).unwrap().render_yaw(1.0) - 30.0).abs() < 1e-3);
}

// Ein Ruder dreht je Tick PI/8 weiter, im Bild dazwischen gleitend; aus heisst sofort 0.
#[test]
fn ruder_rudern_je_tick_ein_achtel_pi() {
    use std::f32::consts::PI;
    let mut entities = Entities::new();
    entities.spawn(6, [0u8; 16], typ("oak_boat"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_paddles(6, [true, false]);
    for _ in 0..3 {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    let boot = entities.get(6).unwrap();
    assert!((boot.rowing_time(0, 1.0) - 3.0 * PI / 8.0).abs() < 1e-5);
    assert!((boot.rowing_time(0, 0.5) - 2.5 * PI / 8.0).abs() < 1e-5);
    assert_eq!(boot.rowing_time(1, 1.0), 0.0);
    entities.set_paddles(6, [false, false]);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert_eq!(entities.get(6).unwrap().rowing_time(0, 1.0), 0.0);
}

// Leere Welt fuer die Tests: nur Luft, also keine Blockkollision fuer Projektile.
fn test_welt() -> crate::chunk::World {
    crate::chunk::World::new()
}

fn spawn_one(entities: &mut Entities) {
    entities.spawn(7, [0u8; 16], 1, (10.0, 64.0, -5.0), 90.0, 0.0, (0.0, 0.0, 0.0));
}

// --- Projektile ---------------------------------------------------------------------

fn typ_index(name: &str) -> i32 {
    (0..500).find(|&i| entity_type_name(i) == Some(name)).unwrap_or_else(|| panic!("Entitytyp {name} unbekannt"))
}

fn leer() -> (crate::chunk::World, crate::BlockShapes) {
    (crate::chunk::World::new(), crate::BlockShapes::all_full_cubes())
}

// --- Abschuss-Versatz ----------------------------------------------------------------

fn item(name: &str) -> HeldItem {
    let id = (0..2000)
        .find(|&i| crate::item_registry::item_name(i) == Some(name))
        .unwrap_or_else(|| panic!("Item {name} unbekannt"));
    HeldItem::plain(id)
}

#[test]
fn die_schusshand_kommt_aus_der_ausruestung() {
    let bogen = item("bow");
    let armbrust = item("crossbow");
    let schwert = item("diamond_sword");
    assert_eq!(shooting_hand(Some(&bogen), None), (false, LaunchWeapon::Bow));
    assert_eq!(shooting_hand(Some(&schwert), Some(&armbrust)), (true, LaunchWeapon::Crossbow));
    assert_eq!(shooting_hand(Some(&bogen), Some(&armbrust)), (false, LaunchWeapon::Bow), "Haupthand gewinnt");
    assert_eq!(shooting_hand(None, None), (false, LaunchWeapon::Hand), "ohne Waffe wurde geworfen");
}

// Gierwinkel 0 heisst Blick nach +Z. Die rechte Hand liegt dann auf -X, "vorwaerts" auf
// +Z, und die Hoehe bleibt die Hoehe. Die Zweithand spiegelt nur die Seite.
#[test]
fn der_versatz_liegt_bei_gierwinkel_null_auf_den_erwarteten_achsen() {
    let (x, y, z) = launch_hand_offset(0.0, 0.0, false, LaunchWeapon::Bow);
    assert!((x - -0.28).abs() < 1e-6, "x={x}");
    assert!((y - -0.10).abs() < 1e-6, "y={y}");
    assert!((z - 0.36).abs() < 1e-6, "z={z}");

    let (zx, _, zz) = launch_hand_offset(0.0, 0.0, true, LaunchWeapon::Bow);
    assert!((zx - 0.28).abs() < 1e-6, "Zweithand spiegelt die Seite: x={zx}");
    assert!((zz - z).abs() < 1e-6, "vorwaerts bleibt gleich");
}

// Die Armbrust liegt tiefer als der Bogen -- genau darum ging es bei der Anforderung.
#[test]
fn die_armbrust_liegt_tiefer_als_der_bogen() {
    let (_, bogen_y, _) = launch_hand_offset(0.0, 0.0, false, LaunchWeapon::Bow);
    let (_, armbrust_y, _) = launch_hand_offset(0.0, 0.0, false, LaunchWeapon::Crossbow);
    assert!(armbrust_y < bogen_y, "Armbrust {armbrust_y} muss unter Bogen {bogen_y} liegen");
}

// Beim Blick gerade nach unten (Pitch 90) zeigt "vorwaerts" nach -Y und "hoch" nach +Z.
#[test]
fn der_versatz_dreht_mit_dem_pitch_mit() {
    let (_, y, z) = launch_hand_offset(0.0, 90.0, false, LaunchWeapon::Bow);
    assert!((y - -0.36).abs() < 1e-6, "vorwaerts zeigt nach unten: y={y}");
    assert!((z - -0.10).abs() < 1e-6, "hoch zeigt nach hinten: z={z}");
}

// Der Versatz ist nur eine Einblendung: nach LAUNCH_BLEND_TICKS Ticks ist das Geschoss
// wieder genau auf seiner echten Bahn.
#[test]
fn der_abschuss_versatz_laeuft_auf_null_zurueck() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(1, [2u8; 16], typ_index("arrow"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 3.0));
    entities.set_launch_offset(1, (0.3, -0.1, 0.4));
    let voll = entities.get(1).expect("Pfeil da").launch_render_offset(0.0);
    assert!((voll[0] - 0.3).abs() < 1e-6, "direkt beim Spawn der ganze Versatz");

    for _ in 0..LAUNCH_BLEND_TICKS {
        entities.tick(&welt, &formen, None);
    }
    assert_eq!(entities.get(1).expect("Pfeil da").launch_render_offset(0.0), [0.0; 3]);
}

// Die Enderperle: ThrowableProjectile.tick macht applyGravity() VOR applyInertia(), und
// beides VOR der Bewegung. Nach einem Tick ist sie also schon gefallen.
//
//   vy = (0 - 0.03) * 0.99 = -0.0297
//   vz = 1.5 * 0.99        =  1.485
#[test]
fn enderperle_faellt_im_ersten_tick_schon() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(1, [1u8; 16], typ_index("ender_pearl"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 1.5));
    entities.tick(&welt, &formen, None);
    let e = entities.get(1).expect("Perle da");
    assert!((e.pos.1 - (64.0 - 0.0297)).abs() < 1e-9, "y={}", e.pos.1);
    assert!((e.pos.2 - 1.485).abs() < 1e-9, "z={}", e.pos.2);
    assert!((e.vel.1 - -0.0297).abs() < 1e-12, "vy={}", e.vel.1);
    assert!((e.vel.2 - 1.485).abs() < 1e-12, "vz={}", e.vel.2);
}

// Der Pfeil: AbstractArrow.tick bewegt ZUERST und wendet Reibung und Schwerkraft erst
// DANACH an. Nach einem Tick ist er deshalb noch auf Starthoehe -- genau der Unterschied
// zur Perle, und die Probe darauf, dass die Reihenfolge stimmt.
#[test]
// Die Windkugel fliegt zwischen den Serverpaketen (nur alle zehn Ticks) selbst weiter:
// schnurgerade, ohne Schwerkraft und ohne Reibung (AbstractWindCharge: Traegheit 1.0,
// accelerationPower 0).
fn windkugel_fliegt_gerade_mit_gleicher_geschwindigkeit() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(5, [5u8; 16], typ_index("wind_charge"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.5, 0.25, -1.0));
    for _ in 0..10 {
        entities.tick(&welt, &formen, None);
    }
    let e = entities.get(5).expect("Windkugel da");
    assert!((e.pos.0 - 5.0).abs() < 1e-9 && (e.pos.1 - 66.5).abs() < 1e-9 && (e.pos.2 + 10.0).abs() < 1e-9, "{:?}", e.pos);
    assert_eq!(e.vel, (0.5, 0.25, -1.0));
}

// Ein relatives Serverpaket rechnet auf die zuletzt GESENDETE Serverposition, nicht auf
// die eigene Vorhersage -- sonst zaehlt die Bewegung doppelt (Windkugeln
// schossen davon, Pfeile blieben vor der Wand stehen).
#[test]
fn relatives_paket_rechnet_auf_die_serverposition() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(5, [5u8; 16], typ_index("wind_charge"), (0.0, 64.0, 0.0), 0.0, 0.0, (1.0, 0.0, 0.0));
    for _ in 0..10 {
        entities.tick(&welt, &formen, None);
    }
    // Der Server meldet nach zehn Ticks dieselben zehn Bloecke.
    entities.move_relative(5, 10.0, 0.0, 0.0);
    let e = entities.get(5).expect("Windkugel da");
    assert!((e.pos.0 - 10.0).abs() < 1e-9, "doppelt gezaehlt: {:?}", e.pos);
}

#[test]
fn pfeil_faellt_im_ersten_tick_noch_nicht() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(2, [2u8; 16], typ_index("arrow"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 3.0));
    entities.tick(&welt, &formen, None);
    let e = entities.get(2).expect("Pfeil da");
    assert_eq!(e.pos.1, 64.0, "der Pfeil darf im ersten Tick nicht fallen");
    assert!((e.pos.2 - 3.0).abs() < 1e-9, "z={}", e.pos.2);
    // Reibung nach der Bewegung, dann Schwerkraft.
    assert!((e.vel.2 - 2.97).abs() < 1e-12, "vz={}", e.vel.2);
    assert!((e.vel.1 - -0.05).abs() < 1e-12, "vy={}", e.vel.1);

    // Zweiter Tick: jetzt faellt er um genau die Geschwindigkeit des ersten.
    entities.tick(&welt, &formen, None);
    let e = entities.get(2).expect("Pfeil da");
    assert!((e.pos.1 - (64.0 - 0.05)).abs() < 1e-9, "y={}", e.pos.1);
}

// Die Krit-Spur liegt hinter dem Pfeil. Im ersten Tick laege sie hinter dem Abschusspunkt
// (beim eigenen Schuss hinter dem Schuetzen) und faellt deshalb aus; ab dem zweiten reicht
// sie nicht weiter zurueck als bis dorthin.
#[test]
fn krit_spur_beginnt_nicht_hinter_dem_abschusspunkt() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(2, [2u8; 16], typ_index("arrow"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 3.0));
    entities.apply_metadata(2, &protocol::packets::EntityDataUpdate { living_flags: Some(protocol::packets::ARROW_FLAG_CRIT), ..Default::default() });
    assert!(entities.get(2).expect("Pfeil da").crit_trail().is_none(), "vor dem ersten Tick");
    entities.tick(&welt, &formen, None);
    assert!(entities.get(2).expect("Pfeil da").crit_trail().is_none(), "nach einem Tick laege sie hinter dem Start");
    entities.tick(&welt, &formen, None);
    let (pos, v) = entities.get(2).expect("Pfeil da").crit_trail().expect("ab dem zweiten Tick");
    // Das hintere Ende der Spur (zwei Tick-Strecken hinter dem Pfeil) liegt nicht vor z = 0.
    assert!(pos.2 - 2.0 * v.2 >= 0.0, "pos={pos:?} v={v:?}");
}

// Ein geworfenes Item fliegt zwischen den (seltenen) Serverpaketen selbst: es faellt, landet
// auf dem Block darunter und bleibt dort liegen, statt zu versinken.
#[test]
fn item_faellt_und_landet_auf_dem_block() {
    let (mut welt, formen) = leer();
    let luft = (0..24).map(|_| crate::chunk::ChunkSection::from_states(vec![0; crate::chunk::SECTION_VOLUME])).collect();
    welt.insert_column(0, 0, luft);
    for x in 0..=2 {
        for z in 0..=4 {
            welt.set_pending_block(x, 63, z, 1);
        }
    }
    let mut entities = Entities::new();
    entities.spawn(7, [7u8; 16], typ_index("item"), (0.5, 66.0, 0.5), 0.0, 0.0, (0.0, 0.2, 0.1));
    entities.tick(&welt, &formen, None);
    let e = entities.get(7).expect("Item da");
    assert!(e.pos.1 > 66.0, "erster Tick steigt noch: y={}", e.pos.1);
    assert!(e.pos.2 > 0.5, "und fliegt vorwaerts: z={}", e.pos.2);
    for _ in 0..60 {
        entities.tick(&welt, &formen, None);
    }
    let e = entities.get(7).expect("Item da");
    assert!((e.pos.1 - 64.0).abs() < 1e-6, "liegt auf dem Boden: y={}", e.pos.1);
    assert!(e.vel.2.abs() < 0.01, "Reibung hat es angehalten: vz={}", e.vel.2);
}

// Ein Erfahrungsorb fliegt ohne Serverpaket zum Spieler unter 8 Bloecken; einen weiter
// entfernten laesst er liegen.
#[test]
fn orb_fliegt_zum_spieler() {
    let (mut welt, formen) = leer();
    let luft = (0..24).map(|_| crate::chunk::ChunkSection::from_states(vec![0; crate::chunk::SECTION_VOLUME])).collect();
    welt.insert_column(0, 0, luft);
    for x in 0..16 {
        for z in 0..16 {
            welt.set_pending_block(x, 63, z, 1);
        }
    }
    let mut entities = Entities::new();
    entities.spawn(3, [3u8; 16], typ_index("experience_orb"), (2.5, 64.0, 2.5), 0.0, 0.0, (0.0, 0.0, 0.0));
    let nah = OrbPlayer { id: 100, pos: (7.5, 64.0, 2.5), eye_height: 1.62 };
    let fern = OrbPlayer { id: 100, pos: (12.5, 64.0, 2.5), eye_height: 1.62 };
    let mut x = Vec::new();
    for _ in 0..100 {
        entities.tick(&welt, &formen, Some(nah));
        x.push(entities.get(3).expect("Orb da").pos.0);
    }
    assert!(x[10] > 2.6, "setzt sich gleich in Bewegung: x={}", x[10]);
    assert!(x[99] > 7.0, "beim Spieler angekommen: x={}", x[99]);
    let mut weit = Entities::new();
    weit.spawn(4, [4u8; 16], typ_index("experience_orb"), (2.5, 64.0, 2.5), 0.0, 0.0, (0.0, 0.0, 0.0));
    for _ in 0..20 {
        weit.tick(&welt, &formen, Some(fern));
    }
    let e = weit.get(4).expect("Orb da");
    assert!((e.pos.0 - 2.5).abs() < 1e-9, "zu weit weg, liegt still: x={}", e.pos.0);
    assert!((e.pos.1 - 64.0).abs() < 1e-6, "auf dem Boden: y={}", e.pos.1);
}

// Ein fallender Sandblock faellt zwischen den Serverpaketen selbst und bleibt auf dem Block
// darunter stehen; seinen Zustand bekommt er aus dem Spawn-Paket.
#[test]
fn fallender_block_faellt_und_landet() {
    let (mut welt, formen) = leer();
    let luft = (0..24).map(|_| crate::chunk::ChunkSection::from_states(vec![0; crate::chunk::SECTION_VOLUME])).collect();
    welt.insert_column(0, 0, luft);
    welt.set_pending_block(0, 63, 0, 1);
    let mut entities = Entities::new();
    entities.spawn(8, [8u8; 16], typ_index("falling_block"), (0.5, 70.0, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_falling_block(8, 118);
    assert_eq!(entities.get(8).unwrap().falling_block, Some(118));
    entities.tick(&welt, &formen, None);
    assert!(entities.get(8).unwrap().pos.1 < 70.0, "faellt schon im ersten Tick");
    for _ in 0..40 {
        entities.tick(&welt, &formen, None);
    }
    let e = entities.get(8).unwrap();
    assert!((e.pos.1 - 64.0).abs() < 1e-6, "steht auf dem Block: y={}", e.pos.1);
    // Andere Typen bekommen keinen Blockzustand.
    entities.spawn(9, [9u8; 16], typ_index("item"), (0.5, 70.0, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_falling_block(9, 118);
    assert_eq!(entities.get(9).unwrap().falling_block, None);
}

// Ein interaction-Entity ist mit seiner Metadaten-Groesse treffbar (Vorgabe 1 x 1).
#[test]
fn interaction_ist_treffbar() {
    let mut entities = Entities::new();
    entities.spawn(9, [9u8; 16], typ_index("interaction"), (0.0, 64.0, 5.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let blick = (0.0, 0.0, 1.0);
    assert_eq!(entities.pick((0.0, 64.5, 0.0), blick, 6.0), Some(9));
    // Hoeher als die Vorgabe: erst mit HEIGHT = 3 trifft der Strahl auf 66.5.
    assert_eq!(entities.pick((0.0, 66.5, 0.0), blick, 6.0), None);
    entities.apply_metadata(9, &protocol::packets::EntityDataUpdate { interaction_height: Some(3.0), ..Default::default() });
    assert_eq!(entities.pick((0.0, 66.5, 0.0), blick, 6.0), Some(9));
}

// Ein interaction-Entity hat in Vanilla keinen Interpolator: seine Trefferbox steht
// sofort an der neuen Stelle, nicht erst drei Ticks spaeter. Ein Lebewesen gleitet weiter.
#[test]
fn interaction_springt_sofort_an_die_neue_stelle() {
    let mut entities = Entities::new();
    entities.spawn(9, [9u8; 16], typ_index("interaction"), (0.0, 64.0, 5.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(10, [10u8; 16], typ_index("zombie"), (0.0, 64.0, 5.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.move_relative(9, 2.0, 0.0, 0.0);
    entities.move_relative(10, 2.0, 0.0, 0.0);
    assert_eq!(entities.get(9).unwrap().hit_pos(), (2.0, 64.0, 5.0));
    assert_eq!(entities.get(10).unwrap().hit_pos(), (0.0, 64.0, 5.0));
}

// Ein Pfeil, der beim Laden der Gegend schon steckt (IN_GROUND aus den Metadaten), faellt
// nicht aus der Wand -- auch nicht durch ein Bewegungspaket mit Geschwindigkeit null.
#[test]
fn steckender_pfeil_aus_metadaten_bleibt_haengen() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(4, [4u8; 16], typ_index("arrow"), (0.0, 70.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.apply_metadata(4, &protocol::packets::EntityDataUpdate { arrow_in_ground: Some(true), ..Default::default() });
    entities.set_velocity(4, (0.0, 0.0, 0.0));
    for _ in 0..20 {
        entities.tick(&welt, &formen, None);
    }
    let e = entities.get(4).expect("Pfeil da");
    assert!(e.in_ground);
    assert_eq!(e.pos, (0.0, 70.0, 0.0));
}

// Steht ein Pfeil in der Kollisionsform eines Blocks, steckt er (AbstractArrow.tick).
#[test]
fn pfeil_im_block_steckt() {
    let (mut welt, formen) = leer();
    welt.set_pending_block(0, 64, 0, 1);
    let mut entities = Entities::new();
    entities.spawn(5, [5u8; 16], typ_index("arrow"), (0.5, 64.5, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.tick(&welt, &formen, None);
    let e = entities.get(5).expect("Pfeil da");
    assert!(e.in_ground);
    assert_eq!(e.pos, (0.5, 64.5, 0.5));
}

// Trifft ein Pfeil einen Block, bleibt er stecken: Geschwindigkeit null, und auch die
// folgenden Ticks bewegen ihn nicht mehr.
#[test]
fn pfeil_bleibt_im_block_stecken() {
    let (mut welt, formen) = leer();
    // Ein Block direkt in der Flugbahn. set_pending_block genuegt -- block_at gibt der
    // Vorhersageschicht Vorrang, und geladene Chunkdaten braucht der Test so nicht.
    welt.set_pending_block(0, 64, 3, 1);
    let mut entities = Entities::new();
    entities.spawn(3, [3u8; 16], typ_index("arrow"), (0.0, 64.5, 0.0), 0.0, 0.0, (0.0, 0.0, 5.0));
    entities.tick(&welt, &formen, None);
    let e = entities.get(3).expect("Pfeil da");
    assert!(e.in_ground, "haette stecken bleiben muessen");
    assert_eq!(e.vel, (0.0, 0.0, 0.0), "steckender Pfeil hat keine Geschwindigkeit mehr");
    // An der Blockkante, nicht dahinter.
    assert!(e.pos.2 <= 3.0 + 1e-9, "z={} liegt hinter dem Block", e.pos.2);
    let stecken = e.pos;
    entities.tick(&welt, &formen, None);
    assert_eq!(entities.get(3).expect("Pfeil da").pos, stecken, "ein steckender Pfeil bewegt sich nicht mehr");
}

// Ein Positionspaket ist fuer ein Projektil eine KORREKTUR und gilt sofort -- Vanilla
// interpoliert nur Lebendiges (Entity.moveOrInterpolateTo ohne InterpolationHandler).
// Beim Spieler bleibt die Angleichung dagegen ueber mehrere Ticks verteilt.
#[test]
fn projektile_werden_hart_gesetzt_spieler_interpoliert() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(4, [4u8; 16], typ_index("ender_pearl"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(5, [5u8; 16], typ_index("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_position(4, 10.0, 64.0, 0.0);
    entities.set_position(5, 10.0, 64.0, 0.0);
    assert_eq!(entities.get(4).expect("Perle da").pos.0, 10.0, "die Perle steht sofort am neuen Ort");
    assert_eq!(entities.get(5).expect("Spieler da").pos.0, 0.0, "der Spieler naehert sich ihm erst");
    entities.tick(&welt, &formen, None);
    let spieler = entities.get(5).expect("Spieler da").pos.0;
    assert!(spieler > 0.0 && spieler < 10.0, "der Spieler soll interpolieren, war {spieler}");
}

// --- Handschlag ---------------------------------------------------------------------

#[test]
fn ein_schlag_laeuft_genau_sechs_ticks_und_endet_bei_null() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.swing_hand(7, false);
    let e = entities.get(7).expect("Entity da");
    // Vor dem ersten Tick steht der Zaehler auf -1 -- der Schlag hat noch nicht begonnen.
    assert_eq!(e.hand_swing(0.0).0, 0.0);

    let mut werte = Vec::new();
    for _ in 0..HAND_SWING_TICKS {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
        werte.push(entities.get(7).expect("Entity da").hand_swing(1.0).0);
    }
    // 0, 1/6, ... 5/6 -- und erst der SIEBTE Tick setzt zurueck. Die zusaetzliche Stufe
    // kommt von der -1 beim Anstossen und ist kein Zaehlfehler: sie sorgt dafuer, dass der
    // Schlag bei 0 beginnt statt schon einen Schritt weit fortgeschritten zu sein.
    assert!((werte[0] - 0.0).abs() < 1e-6, "{werte:?}");
    assert!((werte[5] - 5.0 / 6.0).abs() < 1e-6, "{werte:?}");
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    // Der siebte Tick setzt den Zaehler zurueck. Der ZWISCHENSCHRITT laeuft trotzdem noch
    // sauber bis 1 durch (s. der_letzte_zwischenschritt_laeuft_vorwaerts_weiter) -- erst
    // ab dem Tick danach steht der Arm still.
    assert_eq!(entities.get(7).expect("Entity da").hand_swing(1.0).0, 1.0);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert_eq!(entities.get(7).expect("Entity da").hand_swing(0.5).0, 0.0, "danach ist der Schlag vorbei");
}

#[test]
// LivingEntity.swingHand laesst einen laufenden Schwung nur ab der HAELFTE verdraengen.
// Ohne das setzte schnelles Klicken die Animation dauernd zurueck, und der Arm zuckte
// bei jedem Klick am Anfang herum, statt durchzuschwingen.
fn ein_zweiter_schlag_verdraengt_den_ersten_erst_ab_der_haelfte() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.swing_hand(7, false);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None); // Zaehler steht jetzt auf 1

    entities.swing_hand(7, false);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    // Waere neu gestartet worden, staende der Fortschritt wieder bei 0.
    assert!((entities.get(7).expect("Entity da").hand_swing(1.0).0 - 2.0 / 6.0).abs() < 1e-6);

    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None); // 3 -- ab hier ist die Haelfte erreicht
    entities.swing_hand(7, false);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    // Neu begonnen heisst: wieder am Anfang der Bewegung (1/6), nicht kurz vor dem Ende.
    assert!(
        (entities.get(7).expect("Entity da").hand_swing(1.0).0 - 1.0 / 6.0).abs() < 1e-6,
        "ab der Haelfte darf neu begonnen werden"
    );
}

#[test]
// Am Ende springt der Fortschritt von 5/6 auf 0 zurueck. Wer da schlicht zwischen last und
// aktuell mischt, laesst den Arm im letzten Zwischenschritt die ganze Bewegung RUECKWAERTS
// durchlaufen -- ein sichtbares Zucken. Vanilla addiert deshalb bei negativer Differenz 1.
fn der_letzte_zwischenschritt_laeuft_vorwaerts_weiter() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.swing_hand(7, false);
    // Ein Tick mehr als die Schwungdauer: genau der, in dem der Zaehler auf 0 zurueckfaellt.
    for _ in 0..=HAND_SWING_TICKS {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    let e = entities.get(7).expect("Entity da");
    let (anfang, _) = e.hand_swing(0.0);
    let (mitte, _) = e.hand_swing(0.5);
    let (ende, _) = e.hand_swing(1.0);
    assert!((anfang - 5.0 / 6.0).abs() < 1e-6, "{anfang}");
    assert!(mitte > anfang && ende > mitte, "{anfang} -> {mitte} -> {ende} laeuft nicht vorwaerts");
    assert!(ende <= 1.0 + 1e-6, "{ende} laeuft ueber 1 hinaus");
}

#[test]
fn die_zweithand_schlaegt_mit_dem_linken_arm() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.swing_hand(7, true);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert!(!entities.get(7).expect("Entity da").hand_swing(0.5).1, "Zweithand = linker Arm");
    entities.swing_hand(7, false);
    for _ in 0..HAND_SWING_TICKS {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    entities.swing_hand(7, false);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert!(entities.get(7).expect("Entity da").hand_swing(0.5).1, "Haupthand = rechter Arm");
}

// --- Kritische Treffer ---------------------------------------------------------------

fn ein_spieler(entities: &mut Entities) {
    entities.spawn(7, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
}

#[test]
// Die Reihenfolge, die Vanilla wirklich schickt: erst der Schaden, dann der Crit.
fn ein_crit_nach_dem_schaden_faerbt_den_laufenden_blitz() {
    let mut entities = Entities::new();
    ein_spieler(&mut entities);
    entities.damaged(7);
    assert!(entities.get(7).expect("da").is_hurt());
    assert!(!entities.get(7).expect("da").is_hurt_by_crit(), "ohne Crit-Paket keine Crit-Farbe");
    entities.crit(7);
    assert!(entities.get(7).expect("da").is_hurt_by_crit());
}

#[test]
// Die umgekehrte Reihenfolge darf genauso gehen -- darauf zu wetten, dass zwei Pakete
// immer in derselben Folge ankommen, waere eine unnoetige Annahme.
fn ein_crit_kurz_vor_dem_schaden_zaehlt_auch_noch() {
    let mut entities = Entities::new();
    ein_spieler(&mut entities);
    entities.crit(7);
    entities.damaged(7);
    assert!(entities.get(7).expect("da").is_hurt_by_crit());
}

#[test]
// Und ein Crit, der lange her ist, darf den naechsten gewoehnlichen Treffer NICHT
// einfaerben. Genau dafuer ist das Nachlaufzeitfenster eng.
fn ein_alter_crit_faerbt_den_naechsten_treffer_nicht() {
    let mut entities = Entities::new();
    ein_spieler(&mut entities);
    entities.crit(7);
    for _ in 0..=CRIT_GRACE_TICKS {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    entities.damaged(7);
    assert!(entities.get(7).expect("da").is_hurt());
    assert!(!entities.get(7).expect("da").is_hurt_by_crit());
}

#[test]
// Nach einem Crit-Treffer folgt ein gewoehnlicher: die Markierung muss weg sein.
fn ein_gewoehnlicher_treffer_loescht_die_crit_markierung() {
    let mut entities = Entities::new();
    ein_spieler(&mut entities);
    entities.damaged(7);
    entities.crit(7);
    assert!(entities.get(7).expect("da").is_hurt_by_crit());
    for _ in 0..MAX_HURT_TIME {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    entities.damaged(7);
    assert!(entities.get(7).expect("da").is_hurt());
    assert!(!entities.get(7).expect("da").is_hurt_by_crit(), "der alte Crit darf nicht nachwirken");
}

#[test]
// Ausserhalb des Blitzes ist die Markierung bedeutungslos -- sie darf nicht allein
// faerben, sonst blieben getroffene Spieler dauerhaft bunt.
fn ohne_laufenden_blitz_gibt_es_keine_crit_farbe() {
    let mut entities = Entities::new();
    ein_spieler(&mut entities);
    entities.damaged(7);
    entities.crit(7);
    for _ in 0..MAX_HURT_TIME {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    assert!(!entities.get(7).expect("da").is_hurt());
    assert!(!entities.get(7).expect("da").is_hurt_by_crit());
}

#[test]
fn ein_crit_fuer_ein_unbekanntes_entity_wird_still_uebergangen() {
    let mut entities = Entities::new();
    entities.crit(4711);
}

#[test]
fn ein_schlag_fuer_ein_unbekanntes_entity_wird_still_uebergangen() {
    let mut entities = Entities::new();
    entities.swing_hand(4711, false);
}

fn span(text: &str) -> protocol::nbt::TextSpan {
    protocol::nbt::TextSpan { text: text.to_string(), ..Default::default() }
}

// Index 15 traegt bei einem Armorstand dessen Flags, bei einem Display den
// Billboard-Modus. Das Paket sieht in beiden Faellen gleich aus -- erst der Entity-Typ
// entscheidet. Ginge das schief, saesse das Namensschild eines Marker-Standes fast zwei
// Bloecke zu hoch.
#[test]
fn armorstand_flags_und_billboard_teilen_sich_index_15_ohne_sich_zu_stoeren() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("armor_stand"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [1u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let update = protocol::packets::EntityDataUpdate {
        armor_stand_flags: Some(protocol::packets::ARMOR_STAND_FLAG_MARKER),
        display_billboard: Some(protocol::packets::ARMOR_STAND_FLAG_MARKER),
        ..Default::default()
    };
    entities.apply_metadata(1, &update);
    entities.apply_metadata(2, &update);

    // Der Armorstand nimmt die Flags (Marker -> Hoehe 0), das Display den Billboard-Modus.
    assert_eq!(entities.get(1).unwrap().armor_stand_height(), 0.0);
    assert_eq!(entities.get(2).unwrap().display.as_ref().unwrap().billboard, protocol::packets::ARMOR_STAND_FLAG_MARKER);
    // Und umgekehrt: der Armorstand hat gar keinen Display-Eintrag.
    assert!(entities.get(1).unwrap().display.is_none());
}

// Das Benutzen-Flag samt Hand kommt im LIVING_FLAGS-Byte und wirkt sofort -- daran
// haengt das gehobene Schild eines fremden Spielers.
#[test]
fn benutzungsflag_nennt_die_hand_und_wirkt_sofort() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    assert_eq!(entities.get(1).unwrap().using_item_right_arm(), None);
    let an = protocol::packets::EntityDataUpdate {
        living_flags: Some(protocol::packets::LIVING_FLAG_USING_ITEM | protocol::packets::LIVING_FLAG_OFFHAND_ACTIVE),
        ..Default::default()
    };
    entities.apply_metadata(1, &an);
    // Nebenhand -> linker Arm, ohne einen Tick abzuwarten.
    assert_eq!(entities.get(1).unwrap().using_item_right_arm(), Some(false));
    entities.apply_metadata(1, &protocol::packets::EntityDataUpdate { living_flags: Some(0), ..Default::default() });
    assert_eq!(entities.get(1).unwrap().using_item_right_arm(), None);
}

// Linkshaender (MAIN_ARM links): die Haupthand sitzt am linken Arm, die Nebenhand am
// rechten -- Halten, Blocken und Schwingen folgen. Sonst hielten Linkshaender
// sichtbar ihr Essen, wo eigentlich ein Schild ist.
#[test]
fn linkshaender_halten_die_haupthand_links() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let apfel = HeldItem::plain(crate::item_registry::item_id_by_name("golden_apple").unwrap());
    let schild = HeldItem::plain(crate::item_registry::item_id_by_name("shield").unwrap());
    entities.set_equipment(1, protocol::packets::EQUIPMENT_SLOT_MAINHAND, Some(apfel));
    entities.set_equipment(1, protocol::packets::EQUIPMENT_SLOT_OFFHAND, Some(schild));
    assert_eq!(entities.get(1).unwrap().arm_items(), (Some(apfel), Some(schild)), "Rechtshaender wie bisher");

    entities.apply_metadata(1, &protocol::packets::EntityDataUpdate { main_arm_left: Some(true), ..Default::default() });
    let e = entities.get(1).unwrap();
    assert_eq!(e.arm_items(), (Some(schild), Some(apfel)), "Schild rechts, Apfel links");

    // Schild (Nebenhand) hoch: der RECHTE Arm blockt.
    let an = protocol::packets::EntityDataUpdate {
        living_flags: Some(protocol::packets::LIVING_FLAG_USING_ITEM | protocol::packets::LIVING_FLAG_OFFHAND_ACTIVE),
        ..Default::default()
    };
    entities.apply_metadata(1, &an);
    assert_eq!(entities.get(1).unwrap().using_item_right_arm(), Some(true));

    // Ein Schlag der Haupthand schwingt den linken Arm.
    entities.swing_hand(1, false);
    assert!(!entities.get(1).unwrap().hand_swing(0.0).1);
}

// Vanillas EntityIndex.add: eine UUID, ein Entity. Der zweite Spawn wird abgewiesen --
// sonst standen Koerper und Namensschild doppelt da.
#[test]
fn ein_zweiter_spawn_mit_derselben_uuid_wird_abgewiesen() {
    let mut entities = Entities::new();
    let uuid = [7u8; 16];
    entities.spawn(1, uuid, typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, uuid, typ("player"), (5.0, 64.0, 5.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    assert_eq!(entities.len(), 1, "der zweite Spawn darf nicht dazukommen");
    assert!(entities.get(1).is_some(), "der ERSTE bleibt stehen, genau wie in Vanilla");
    assert!(entities.get(2).is_none());
    assert_eq!(entities.duplicate_uuid_count(), 1);

    // Dieselbe ID erneut zu spawnen bleibt erlaubt -- das ist ein Neuaufbau desselben
    // Entities, kein Doppel.
    entities.spawn(1, uuid, typ("player"), (9.0, 64.0, 9.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    assert_eq!(entities.len(), 1);
    assert_eq!(entities.get(1).unwrap().pos.0, 9.0, "der neue Zustand gilt");

    // Und nach dem Entfernen darf die UUID wieder vergeben werden (Spieler betritt den
    // Trackingbereich erneut) -- sonst waere der Spieler danach unsichtbar.
    entities.remove(&[1]);
    entities.spawn(3, uuid, typ("player"), (1.0, 64.0, 1.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    assert_eq!(entities.len(), 1);
    assert!(entities.get(3).is_some(), "nach dem Entfernen muss die UUID wieder gehen");
}

// Die Null-UUID ist kein echter Schluessel -- Entities ohne UUID duerfen sich nicht
// gegenseitig verdraengen.
#[test]
fn die_null_uuid_verdraengt_nichts() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("arrow"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [0u8; 16], typ("arrow"), (1.0, 64.0, 1.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    assert_eq!(entities.len(), 2);
}

// Server setzen auch der EIGENEN Figur ein Namensschild-Display auf. Die eigene Figur
// steht nicht in der Liste -- das Display muss trotzdem aufsitzen und mitlaufen.
#[test]
fn ein_display_folgt_der_eigenen_figur() {
    let mut entities = Entities::new();
    entities.spawn(2, [2u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_own_body(7, (10.0, 70.0, 5.0), 0.0, PLAYER_BOX_HEIGHT);
    entities.set_passengers(7, &[2]);
    assert!(entities.has_passengers(7));
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    let display = entities.get(2).unwrap().pos;
    assert!((display.0 - 10.0).abs() < 1e-9 && (display.2 - 5.0).abs() < 1e-9, "{display:?}");
    assert!((display.1 - (70.0 + PLAYER_BOX_HEIGHT)).abs() < 1e-9, "{display:?}");
}

// Ein Passagier hat KEINE eigene Bewegung -- er sitzt da, wo sein Fahrzeug ist. Genau
// daran hing das statische zweite Namensschild (Display auf dem Kopf eines Spielers).
#[test]
fn ein_passagier_folgt_seinem_fahrzeug() {
    let mut entities = Entities::new();
    entities.spawn(1, [1u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [2u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_passengers(1, &[2]);

    // Das Fahrzeug bewegt sich -- der Passagier muss mitkommen, ohne eigenes Paket.
    entities.move_relative(1, 10.0, 0.0, 5.0);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    let spieler = entities.get(1).unwrap().pos;
    let display = entities.get(2).unwrap().pos;
    assert!((display.0 - spieler.0).abs() < 1e-9, "x folgt: {display:?} gegen {spieler:?}");
    assert!((display.2 - spieler.2).abs() < 1e-9, "z folgt");
    // PASSENGER-Anker ist AT_HEIGHT -- also die Spielerhoehe ueber dessen Fuessen.
    assert!((display.1 - (spieler.1 + PLAYER_BOX_HEIGHT)).abs() < 1e-9, "y = Fahrzeughoehe: {}", display.1);

    // Abgestiegen: der Passagier bleibt stehen, wo er war, statt weiter zu folgen.
    entities.set_passengers(1, &[]);
    let vorher = entities.get(2).unwrap().pos;
    entities.move_relative(1, 10.0, 0.0, 0.0);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert_eq!(entities.get(2).unwrap().pos, vorher);
}

// Verschwindet das Fahrzeug, darf der Passagier nicht an dessen letzter Stelle kleben.
#[test]
fn ein_entferntes_fahrzeug_loest_die_mitfahrt() {
    let mut entities = Entities::new();
    entities.spawn(1, [1u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [2u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_passengers(1, &[2]);
    entities.remove(&[1]);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert!(entities.get(2).is_some(), "der Passagier selbst bleibt");
}

// Vanilla laesst nur existierende Passagiere aufsteigen -- ein Paket fuer eine noch
// unbekannte Id darf keinen Traeger erzeugen, der dann keinen Namen mehr zeigt.
#[test]
fn passagiere_zaehlen_nur_wenn_sie_existieren() {
    let mut entities = Entities::new();
    entities.spawn(1, [1u8; 16], typ("player"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_passengers(1, &[2]);
    assert!(!entities.has_passengers(1));
    entities.spawn(2, [2u8; 16], typ("text_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_passengers(1, &[2]);
    assert!(entities.has_passengers(1));
    entities.remove(&[2]);
    assert!(!entities.has_passengers(1), "entfernter Passagier steigt ab");
}

// Das klassische Hologramm: unsichtbarer Traeger, sichtbarer Name.
#[test]
fn unsichtbarer_armorstand_behaelt_seinen_namen() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("armor_stand"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.apply_metadata(
        1,
        &protocol::packets::EntityDataUpdate {
            flags: Some(protocol::packets::ENTITY_FLAG_INVISIBLE),
            custom_name: Some(Some(vec![span("Shop")])),
            name_visible: Some(true),
            ..Default::default()
        },
    );
    let e = entities.get(1).unwrap();
    assert!(e.invisible);
    assert!(e.name_visible);
    assert_eq!(e.custom_name.as_ref().unwrap()[0].text, "Shop");
}

// Das Leuchten steht im selben Flag-Byte und geht mit dem naechsten Byte ohne Bit 6 aus.
#[test]
fn leuchten_kommt_aus_bit_6() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("zombie"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let flags = |f| protocol::packets::EntityDataUpdate { flags: Some(f), ..Default::default() };
    entities.apply_metadata(1, &flags(protocol::packets::ENTITY_FLAG_GLOWING | protocol::packets::ENTITY_FLAG_ON_FIRE));
    assert!(entities.get(1).unwrap().glowing);
    assert!(!entities.get(1).unwrap().invisible);
    entities.apply_metadata(1, &flags(protocol::packets::ENTITY_FLAG_ON_FIRE));
    assert!(!entities.get(1).unwrap().glowing);
}

// Ein Text-Feld an Index 23 gehoert nur einem text_display. Landete es bei einem
// item_display, zeigte dieses ploetzlich Text statt seines Gegenstandes.
#[test]
fn display_inhalt_wird_nur_beim_passenden_typ_uebernommen() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("item_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.apply_metadata(
        1,
        &protocol::packets::EntityDataUpdate { display_text: Some(vec![span("nicht hierhin")]), ..Default::default() },
    );
    let content = &entities.get(1).unwrap().display.as_ref().unwrap().content;
    assert!(matches!(content, DisplayContent::Item(None)), "item_display darf keinen Text annehmen");

    entities.apply_metadata(
        1,
        &protocol::packets::EntityDataUpdate { display_item: Some(Some(protocol::item::ItemStack::new(936, 1))), ..Default::default() },
    );
    let content = &entities.get(1).unwrap().display.as_ref().unwrap().content;
    assert!(matches!(content, DisplayContent::Item(Some(item)) if item.item_id == 936));
}

// Displays springen ohne teleport_duration sofort (Vanillas Vorgabe 0) und gleiten mit ihr
// ueber genau so viele Ticks -- ein zurueckfahrendes Tor sprang vorher nach drei Ticks.
#[test]
fn display_folgt_teleport_duration() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("block_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_position(1, 10.0, 64.0, 0.0);
    assert_eq!(entities.get(1).unwrap().pos.0, 10.0, "ohne Dauer sofort da");

    entities.apply_metadata(1, &protocol::packets::EntityDataUpdate { display_teleport_duration: Some(10), ..Default::default() });
    entities.set_position(1, 0.0, 64.0, 0.0);
    let mut wege = Vec::new();
    for _ in 0..10 {
        // Das Ziel noch einmal: darf die laufende Angleichung nicht neu starten.
        entities.set_position(1, 0.0, 64.0, 0.0);
        entities.tick(&welt, &formen, None);
        wege.push(entities.get(1).unwrap().pos.0);
    }
    assert!((wege[0] - 9.0).abs() < 1e-9, "erster Tick ein Zehntel: {wege:?}");
    assert!((wege[4] - 5.0).abs() < 1e-9, "gleichmaessig: {wege:?}");
    assert_eq!(wege[9], 0.0);
}

// Eine Transformations-Animation, die jeden Tick neu startet (Dauer 1, Verzoegerung 0), gleitet
// wie in Vanilla: Beginn erst ab dem naechsten Tick, und jeder Abschnitt startet an der Stelle,
// die zuletzt zu sehen war -- nicht am Anfang des vorigen Abschnitts.
#[test]
fn display_transformation_gleitet_ueber_abschnitte() {
    let (welt, formen) = leer();
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("item_display"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let schritt = |entities: &mut Entities, y: f32| {
        entities.apply_metadata(
            1,
            &protocol::packets::EntityDataUpdate {
                display_interp_start: Some(0),
                display_interp_duration: Some(1),
                display_translation: Some([0.0, y, 0.0]),
                ..Default::default()
            },
        );
    };
    let zeichne = |entities: &Entities, anteil: f32| {
        let e = entities.get(1).unwrap();
        e.display.as_ref().unwrap().transform_at(e.age_ticks(anteil)).translation[1]
    };
    schritt(&mut entities, 1.0);
    entities.tick(&welt, &formen, None);
    assert!((zeichne(&entities, 0.5) - 0.5).abs() < 1e-6, "erster Abschnitt halb: {}", zeichne(&entities, 0.5));
    // Der naechste Abschnitt kommt, waehrend zuletzt bei 0.5 gezeichnet wurde.
    schritt(&mut entities, 2.0);
    assert!((zeichne(&entities, 0.9) - 0.5).abs() < 1e-6, "vor dem Tick bleibt es stehen: {}", zeichne(&entities, 0.9));
    entities.tick(&welt, &formen, None);
    let mitte = zeichne(&entities, 0.5);
    assert!((mitte - 1.25).abs() < 1e-6, "von 0.5 nach 2 zur Haelfte, kein Ruecksprung: {mitte}");
}

// Ein Display-Entity darf den Angriffs-Raycast nicht abfangen: in Vanilla ist es nicht
// anvisierbar, und seine Box hat mit WIDTH/HEIGHT = 0 kein Volumen.
#[test]
fn display_entity_faengt_keinen_treffer_ab() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("text_display"), (0.0, 64.0, 2.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [1u8; 16], typ("zombie"), (0.0, 64.0, 4.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    // Strahl von hinten durch das Display hindurch auf den Zombie.
    assert_eq!(entities.pick((0.0, 64.9, 0.0), (0.0, 0.0, 1.0), 10.0), Some(2));
}

#[test]
fn spawn_und_remove_verwalten_die_liste() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    assert_eq!(entities.len(), 1);
    assert!(entities.get(7).is_some());
    entities.remove(&[7]);
    assert!(entities.is_empty());
}

#[test]
fn relative_bewegung_addiert_auf_serverposition() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.move_relative(7, 0.5, 0.0, -0.25);
    entities.move_relative(7, 0.5, 0.0, -0.25);
    let e = entities.get(7).unwrap();
    // Beide Deltas muessen sich auf die SERVER-Position beziehen, nicht auf die gerade
    // interpolierte -- sonst bliebe das Entity dauerhaft hinter seiner echten Position
    // zurueck, weil sich der Interpolationsrueckstand von Paket zu Paket aufsummiert.
    assert!((e.target_pos().0 - 11.0).abs() < 1e-9, "x={}", e.target_pos().0);
    assert!((e.target_pos().2 - -5.5).abs() < 1e-9, "z={}", e.target_pos().2);
}

#[test]
fn drei_ticks_erreichen_die_serverposition_exakt() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.set_position(7, 20.0, 64.0, -5.0);
    for _ in 0..INTERPOLATION_STEPS {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    let e = entities.get(7).unwrap();
    // Der Lerp-Nenner ist die Zahl der NOCH offenen Schritte -- der letzte Schritt ist
    // damit eine volle Angleichung und landet exakt, nicht nur nahe am Ziel.
    assert!((e.pos.0 - 20.0).abs() < 1e-9, "x={}", e.pos.0);
    assert!((e.pos.2 - -5.0).abs() < 1e-9, "z={}", e.pos.2);
}

#[test]
fn erster_tick_geht_genau_ein_drittel_der_strecke() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.set_position(7, 13.0, 64.0, -5.0); // Start x=10 -> Strecke 3
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    let e = entities.get(7).unwrap();
    assert!((e.pos.0 - 11.0).abs() < 1e-9, "1/3 von 3 Bloecken erwartet, x={}", e.pos.0);
}

#[test]
fn render_pos_liegt_zwischen_den_beiden_tickstaenden() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.set_position(7, 13.0, 64.0, -5.0);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    let e = entities.get(7).unwrap();
    // Zu Tickbeginn steht sie noch auf 10, nach dem Tick auf 11 -- das Bild mischt
    // dazwischen (Vanilla: Entity.getLerpedPos).
    assert!((e.render_pos(0.0).0 - 10.0).abs() < 1e-9);
    assert!((e.render_pos(1.0).0 - 11.0).abs() < 1e-9);
    assert!((e.render_pos(0.5).0 - 10.5).abs() < 1e-9);
}

#[test]
fn treffer_und_bildposition_stammen_aus_derselben_quelle() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.set_position(7, 13.0, 64.0, -5.0);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    let e = entities.get(7).unwrap();
    // Am Tickende sind beide identisch. Frueher liefen sie auseinander (Wanduhr fuers
    // Bild, Ticks fuer die Trefferbox) -- genau das flaggte bei Grim als "Hitboxes".
    assert_eq!(e.hit_pos(), e.render_pos(1.0));
}

#[test]
fn kopfdrehung_laeuft_ueber_den_kuerzeren_weg() {
    let mut entities = Entities::new();
    entities.spawn(7, [0u8; 16], 1, (10.0, 64.0, -5.0), 350.0, 0.0, (0.0, 0.0, 0.0));
    entities.get_mut(7).unwrap().set_head_yaw(10.0);
    for _ in 0..HEAD_YAW_STEPS {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    let e = entities.get(7).unwrap();
    // 350 -> 10 Grad sind 20 Grad vorwaerts, nicht 340 rueckwaerts. Ohne den
    // Winkel-Lerp drehte sich der Kopf einmal komplett herum.
    let ziel = crate::entity_model::wrap_degrees(e.relative_head_yaw(1.0) + e.render_yaw(1.0) - 10.0);
    assert!(ziel.abs() < 1e-3, "Kopf muss auf 10 Grad stehen, Abweichung {ziel}");
}

#[test]
fn update_mitten_in_der_angleichung_setzt_sie_fort() {
    let mut entities = Entities::new();
    spawn_one(&mut entities);
    entities.set_position(7, 13.0, 64.0, -5.0);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None); // x = 11
                     // Neues Ziel, bevor die alte Strecke fertig ist: der Startpunkt ist die AKTUELLE
                     // Teilposition, nicht die alte Serverposition -- sonst sprigt das Bild zurueck.
    entities.set_position(7, 14.0, 64.0, -5.0);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    let e = entities.get(7).unwrap();
    assert!((e.pos.0 - 12.0).abs() < 1e-9, "11 + 1/3 von 3 erwartet, x={}", e.pos.0);
}

#[test]
fn pick_trifft_nur_bei_blick_auf_die_box_und_in_reichweite() {
    let mut entities = Entities::new();
    // Entity bei x=13, Fuesse auf 64 -- Augenhoehe des Angreifers bei 65.62.
    entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let eye = (10.0, 65.62, 0.0);

    // Blick geradeaus (+x): trifft (Distanz 2.7 zur Box-Vorderkante bei 12.7).
    assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 3.0), Some(9));
    // Blick in die Gegenrichtung: kein Treffer.
    assert_eq!(entities.pick(eye, (-1.0, 0.0, 0.0), 3.0), None);
    // Zu kurze Reichweite: kein Treffer.
    assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 2.0), None);
}

#[test]
// Reichweite eines Speers: nur zwischen Mindest- und Hoechstweite; mit Zuschlag zaehlt ein
// knapper Vorbeischuss, aber nicht, wenn ein Block zwischen Beruehrpunkt und Gegner liegt.
fn pick_segment_mit_mindestweite_und_zuschlag() {
    let mut entities = Entities::new();
    // Box x 12.7..13.3, z -0.3..0.3.
    entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let eye = (10.0, 65.62, 0.0);
    let frei = |_: (f64, f64, f64), _: (f64, f64, f64)| 1.0;
    let (id, d) = entities.pick_segment(eye, (1.0, 0.0, 0.0), 2.0, 4.5, 0.0, &frei).unwrap();
    assert_eq!(id, 9);
    assert!((d - 2.7).abs() < 1e-9);
    assert_eq!(entities.pick_segment(eye, (1.0, 0.0, 0.0), 2.0, 2.5, 0.0, &frei), None, "zu kurz");
    assert_eq!(entities.pick_segment(eye, (1.0, 0.0, 0.0), 3.5, 6.0, 0.0, &frei), None, "naeher als die Mindestweite");
    // Startpunkt in der Box: Treffer am Start.
    assert_eq!(entities.pick_segment(eye, (1.0, 0.0, 0.0), 3.0, 6.0, 0.0, &frei).map(|(_, d)| d), Some(3.0));
    // 0.2 seitlich vorbei: nur mit Zuschlag, und nur ohne Block dazwischen.
    let daneben = (10.0, 65.62, 0.5);
    assert_eq!(entities.pick_segment(daneben, (1.0, 0.0, 0.0), 0.0, 4.5, 0.0, &frei), None);
    assert_eq!(entities.pick_segment(daneben, (1.0, 0.0, 0.0), 0.0, 4.5, 0.3, &frei).map(|(id, _)| id), Some(9));
    let wand = |_: (f64, f64, f64), _: (f64, f64, f64)| 0.0;
    assert_eq!(entities.pick_segment(daneben, (1.0, 0.0, 0.0), 0.0, 4.5, 0.3, &wand), None);
}

#[test]
fn pick_ueberspringt_items_und_trifft_den_mob_dahinter() {
    let item_type = ENTITY_TYPE_NAMES.iter().position(|n| *n == "item").unwrap() as i32;
    let pig_type = ENTITY_TYPE_NAMES.iter().position(|n| *n == "pig").unwrap() as i32;
    assert!(!is_attackable(item_type));
    assert!(is_attackable(pig_type));

    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], item_type, (12.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [0u8; 16], pig_type, (14.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    // Auf Schweinehoehe (0.9 Bloecke, s. vanilla_dimensions).
    let eye = (10.0, 64.5, 0.0);
    // Das Item liegt naeher in der Schusslinie -- gewaehlt werden muss trotzdem der
    // Mob dahinter, sonst kickt der Server ("attack an invalid entity").
    assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 6.0), Some(2));
}

#[test]
// Der End-Kristall ist 2 x 2 Bloecke -- mit der frueheren Spielerbox (0.6 breit) ging ein
// Blick knapp neben die Mitte daneben.
fn end_kristall_hat_seine_vanilla_box() {
    let mut entities = Entities::new();
    entities.spawn(3, [0u8; 16], typ("end_crystal"), (14.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    // 0.8 Bloecke seitlich der Mitte, in Hoehe 1.9: ausserhalb einer Spielerbox, innerhalb
    // der Kristallbox.
    assert_eq!(entities.pick((10.0, 65.9, 0.8), (1.0, 0.0, 0.0), 6.0), Some(3));
}

#[test]
fn pick_waehlt_die_naechste_von_zwei_boxen() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], player_type(), (12.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [0u8; 16], player_type(), (14.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let eye = (10.0, 65.0, 0.0);
    assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 6.0), Some(1));
}

fn player_type() -> i32 {
    ENTITY_TYPE_NAMES.iter().position(|n| *n == "player").unwrap() as i32
}

#[test]
fn geduckter_spieler_wird_nicht_ueber_den_kopf_getroffen() {
    let mut entities = Entities::new();
    entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    // Auf 65.6 zielt man beim STEHENDEN Ziel (1.8 hoch, also bis 65.8) noch auf den Kopf.
    let eye = (10.0, 65.6, 0.0);
    assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 3.0), Some(9));

    // Geduckt reicht die Box nur bis 65.5 -- der Strahl geht darueber hinweg. Genau das
    // ist der Angriff, den GrimAC als Hitboxes flaggt, wenn der Client trotzdem sendet.
    entities.set_pose(9, POSE_CROUCHING);
    assert_eq!(entities.pick(eye, (1.0, 0.0, 0.0), 3.0), None);

    // Auf Brusthoehe trifft er auch geduckt.
    assert_eq!(entities.pick((10.0, 65.2, 0.0), (1.0, 0.0, 0.0), 3.0), Some(9));
}

#[test]
// Fremde Spieler legen sich in der Schwimm-Pose waagerecht, in 0.09-Schritten je Tick wie
// LivingEntity.updateSwimAmount -- und richten sich danach genauso langsam wieder auf.
fn fremder_spieler_legt_sich_in_der_schwimmpose_hin() {
    let (world, shapes) = (crate::chunk::World::new(), crate::BlockShapes::all_full_cubes());
    let mut entities = Entities::new();
    entities.spawn(4, [0u8; 16], player_type(), (0.5, 64.0, 0.5), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_pose(4, POSE_SWIMMING);
    entities.tick(&world, &shapes, None);
    let lean = entities.get(4).unwrap().lean(1.0);
    assert!((lean.amount - 0.09).abs() < 1e-6);
    assert!(lean.swimming && !lean.in_water, "leere Welt: kriechend, nicht im Wasser");
    assert!((entities.get(4).unwrap().lean(0.5).amount - 0.045).abs() < 1e-6, "zwischen den Ticks interpoliert");
    for _ in 0..20 {
        entities.tick(&world, &shapes, None);
    }
    assert_eq!(entities.get(4).unwrap().lean(1.0).amount, 1.0);
    entities.set_pose(4, 0);
    entities.tick(&world, &shapes, None);
    assert!((entities.get(4).unwrap().lean(1.0).amount - 0.91).abs() < 1e-6);
}

#[test]
fn schwimmender_spieler_ist_nur_eine_halbe_box_hoch() {
    let mut entities = Entities::new();
    entities.spawn(9, [0u8; 16], player_type(), (13.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_pose(9, POSE_SWIMMING);
    // 0.6 hoch -> bis 64.6.
    assert_eq!(entities.pick((10.0, 64.3, 0.0), (1.0, 0.0, 0.0), 3.0), Some(9));
    assert_eq!(entities.pick((10.0, 64.9, 0.0), (1.0, 0.0, 0.0), 3.0), None);
}

#[test]
fn trefferposition_laeuft_in_drei_tick_schritten_und_landet_exakt() {
    let mut entities = Entities::new();
    entities.spawn(9, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.set_position(9, 3.0, 64.0, 0.0);

    // Vor dem ersten Tick steht die Trefferbox noch am Ausgangspunkt -- die Wanduhr
    // bewegt sie NICHT (das tut nur die Render-Position).
    assert_eq!(entities.get(9).unwrap().hit_pos().0, 0.0);
    // Nenner ist die Zahl der offenen Schritte: 1/3, dann 1/2, dann 1/1.
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert!((entities.get(9).unwrap().hit_pos().0 - 1.0).abs() < 1e-9);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert!((entities.get(9).unwrap().hit_pos().0 - 2.0).abs() < 1e-9);
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    // Letzter Schritt: exakt auf dem Ziel, nicht bloss in der Naehe -- sonst driftete die
    // Box bei jedem Update ein Stueck weiter hinter dem Server her.
    assert_eq!(entities.get(9).unwrap().hit_pos().0, 3.0);
    // Weitere Ticks ohne neues Update aendern nichts mehr.
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert_eq!(entities.get(9).unwrap().hit_pos().0, 3.0);
}

fn typ(name: &str) -> i32 {
    ENTITY_TYPE_NAMES.iter().position(|n| *n == name).unwrap_or_else(|| panic!("Typ {name} fehlt")) as i32
}

// In Vanilla blitzt NUR ein LivingEntity rot auf: Entity.onDamaged ist leer, erst
// LivingEntity.onDamaged setzt hurtTime. Die Tabelle stammt aus der Klassenhierarchie im
// Jar -- diese Stichproben sind die Gegenprobe, dass sie richtig ausgerichtet ist
// (ein Versatz um einen Index waere sonst schwer zu bemerken).
#[test]
fn lebendig_erkennt_die_vanilla_klassenhierarchie() {
    for name in ["player", "zombie", "pig", "villager", "ender_dragon"] {
        assert!(is_living(typ(name)), "{name} ist in Vanilla ein LivingEntity");
    }
    // Der Ruestungsstaender ist der Fall, den eine Handliste typischerweise falsch
    // einsortiert: er sieht aus wie Deko, IST aber ein LivingEntity.
    assert!(is_living(typ("armor_stand")), "armor_stand ist ein LivingEntity");
    for name in ["item", "arrow", "oak_boat", "minecart", "tnt", "item_frame", "painting", "end_crystal"] {
        assert!(!is_living(typ(name)), "{name} ist KEIN LivingEntity");
    }
    // Unbekannte/negative Indizes duerfen nicht in Panik enden -- der Typ kommt roh vom
    // Server, ein neuerer Server kann Typen schicken, die diese Tabelle nicht kennt.
    assert!(!is_living(-1));
    assert!(!is_living(i32::MAX));
}

// LivingEntity.onDamaged setzt hurtTime = 10, LivingEntity.tick zaehlt es um 1 herunter.
// Nach genau 10 Ticks ist Schluss -- nicht frueher (Blitz zu kurz) und nicht spaeter.
#[test]
fn schadens_blitz_haelt_genau_zehn_ticks() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    assert!(!entities.get(1).unwrap().is_hurt(), "ungetroffen blitzt nichts");

    entities.damaged(1);
    for tick in 0..MAX_HURT_TIME {
        assert!(entities.get(1).unwrap().is_hurt(), "Tick {tick}: muss noch blitzen");
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    assert!(!entities.get(1).unwrap().is_hurt(), "nach {MAX_HURT_TIME} Ticks ist der Blitz vorbei");
}

// Die einstellbare Blitzdauer zaehlt ab dem Treffer, unabhaengig von Vanillas hurtTime.
#[test]
fn schadens_blitz_dauer_ist_einstellbar() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    assert!(!entities.get(1).unwrap().damage_flash(40), "ungetroffen blitzt nichts");
    entities.damaged(1);
    for _ in 0..20 {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    let e = entities.get(1).unwrap();
    assert!(!e.damage_flash(10), "Vanilla-Dauer ist vorbei");
    assert!(e.damage_flash(21), "laengere Dauer laeuft noch");
    assert!(!e.damage_flash(20));
}

// Ein erneuter Treffer setzt den Zaehler ZURUECK auf 10 (Vanilla weist neu zu, es wird
// nichts aufaddiert) -- sonst waere schnelles Trommelfeuer nach kurzer Zeit ein
// Dauerrot statt eines Flackerns.
#[test]
fn erneuter_treffer_setzt_den_zaehler_zurueck_statt_zu_addieren() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], player_type(), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.damaged(1);
    for _ in 0..5 {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    entities.damaged(1);
    for _ in 0..(MAX_HURT_TIME - 1) {
        entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    }
    assert!(entities.get(1).unwrap().is_hurt(), "nach dem zweiten Treffer laufen wieder volle 10 Ticks");
    entities.tick(&test_welt(), &crate::BlockShapes::all_full_cubes(), None);
    assert!(!entities.get(1).unwrap().is_hurt());
}

// Ein Boot nimmt Schaden (der Server schickt dafuer auch ein Damage-Paket), blitzt in
// Vanilla aber nicht -- Entity.onDamaged tut nichts.
#[test]
fn nicht_lebewesen_blitzen_nicht() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("oak_boat"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.damaged(1);
    assert!(!entities.get(1).unwrap().is_hurt(), "ein Boot blitzt nicht");
}

// Ein Paket fuer eine unbekannte ID darf nichts tun und nicht in Panik enden -- es kann
// ein Entity betreffen, das wir gar nicht verfolgen.
#[test]
fn schaden_an_unbekannter_id_ist_folgenlos() {
    let mut entities = Entities::new();
    entities.damaged(4711);
}

// Jeder Typ mit Item-Darstellung steht in der Typtabelle (sonst griffe der Renderer nie).
#[test]
fn item_darstellung_kennt_die_vanilla_typen() {
    for name in ["egg", "snowball", "ender_pearl", "experience_bottle", "splash_potion", "lingering_potion", "eye_of_ender", "firework_rocket"] {
        assert!(matches!(item_render_kind(typ(name)), Some(ItemRenderKind::Flying { scale, .. }) if scale == 1.0), "{name}");
    }
    assert!(matches!(item_render_kind(typ("fireball")), Some(ItemRenderKind::Flying { scale, .. }) if scale == 3.0));
    assert_eq!(item_render_kind(typ("item")), Some(ItemRenderKind::Dropped));
    assert_eq!(item_render_kind(typ("zombie")), None);
}

// Der Stack an Index 8 landet nur bei Typen mit Item-Darstellung, mit Anzahl und Trankfarbe.
#[test]
fn item_stapel_aus_den_metadaten() {
    let mut entities = Entities::new();
    entities.spawn(1, [0u8; 16], typ("item"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    entities.spawn(2, [1u8; 16], typ("zombie"), (0.0, 64.0, 0.0), 0.0, 0.0, (0.0, 0.0, 0.0));
    let update = protocol::packets::EntityDataUpdate { item_stack: Some(Some(protocol::item::ItemStack::new(936, 20))), ..Default::default() };
    entities.apply_metadata(1, &update);
    entities.apply_metadata(2, &update);
    let item = entities.get(1).unwrap();
    assert_eq!(item.item.map(|i| i.item_id), Some(936));
    assert_eq!(item.item_count, 20);
    assert_eq!(item.item_seed, 936);
    assert_eq!(entities.get(2).unwrap().item, None);
}
