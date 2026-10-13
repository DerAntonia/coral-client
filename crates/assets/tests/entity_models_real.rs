// Extrahiert die Entity-Modelldaten (falls noetig) aus der echten lokalen Installation (Java + Client-Jar) und
// prueft sie. Braucht ein installiertes 1.21.11, daher #[ignore]:
//   cargo test -p assets --test entity_models_real -- --ignored

use world::entity_model::{biped_model, emit, entity_transform, Model, Part, Pose};
use world::mob_catalog::{mob_look, walk_pose, Anim};
use world::model_data::layer_model;

const VERSION: &str = "1.21.11";

fn laden() {
    static EINMAL: std::sync::Once = std::sync::Once::new();
    EINMAL.call_once(|| {
        if assets::game_data_status(VERSION) != assets::GameDataStatus::Current {
            assets::extract_game_data(VERSION, &|_| {}).expect("Spieldaten nicht extrahierbar");
        }
        let text = assets::stored_entity_models(VERSION).expect("Modelldaten fehlen");
        world::model_data::install(&text);
    });
}

fn find<'a>(part: &'a Part, name: &str) -> Option<&'a Part> {
    if part.name == name {
        return Some(part);
    }
    part.children.iter().find_map(|c| find(c, name))
}

fn kasten(m: &Model) -> ([f32; 3], [f32; 3], usize) {
    let mut out = Vec::new();
    emit(m, &Pose::new(), entity_transform([0.0; 3], 0.0, 1.0, 1.0), [0.0, 0.0, 1.0, 1.0], 1.0, &mut out);
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in &out {
        for i in 0..3 {
            lo[i] = lo[i].min(v.position[i]);
            hi[i] = hi[i].max(v.position[i]);
        }
    }
    (lo, hi, out.len())
}

#[test]
#[ignore]
// Der Dorfbewohner: 64x64, Kopf mit Nase und Hut, verschraenkte Arme, zwei Beine, und er
// steht auf dem Boden.
fn dorfbewohner_ist_vollstaendig() {
    laden();
    let m = layer_model("villager#main").expect("villager");
    assert_eq!(m.tex_size, [64.0, 64.0]);
    let root = &m.parts[0];
    for teil in ["head", "nose", "hat", "hat_rim", "arms", "body", "jacket", "right_leg", "left_leg"] {
        assert!(find(root, teil).is_some(), "{teil} fehlt");
    }
    assert!(find(find(root, "head").unwrap(), "nose").is_some(), "die Nase haengt am Kopf");
    let (lo, hi, _) = kasten(m);
    assert!(lo[1].abs() < 0.01 && hi[1] > 1.9 && hi[1] < 2.1, "{lo:?} {hi:?}");
    assert_eq!(layer_model("ghast#main").unwrap().parts[0].scale, [4.5; 3], "eingebackene Skalierung");
}

#[test]
#[ignore]
// Das geladene Zombie-Modell liegt exakt dort, wo das handgeschriebene (im Spiel geprueft)
// liegt: Pivots, Drehungen und die Wurzel werden also richtig gelesen.
fn zombie_aus_daten_gleicht_dem_handgeschriebenen() {
    laden();
    let geladen = kasten(layer_model("zombie#main").unwrap());
    let hand = kasten(&biped_model("entity/zombie/zombie", 0.0));
    assert_eq!(geladen.2, hand.2);
    for i in 0..3 {
        assert!((geladen.0[i] - hand.0[i]).abs() < 1e-4 && (geladen.1[i] - hand.1[i]).abs() < 1e-4, "{geladen:?} vs {hand:?}");
    }
}

#[test]
#[ignore]
// Jeder Eintrag des Katalogs zeigt auf ein Modell, das es gibt -- ein Tippfehler im Layer
// liesse den Mob wieder als Kasten erscheinen.
fn alle_katalog_layer_existieren() {
    laden();
    let mut geprueft = 0;
    for n in world::entity_types::ENTITY_TYPE_NAMES {
        let Some(l) = mob_look(n) else { continue };
        assert!(layer_model(l.layer).is_some(), "{n}: Layer {} fehlt", l.layer);
        if let Some(b) = l.baby_layer {
            assert!(layer_model(b).is_some(), "{n}: Baby-Layer {b} fehlt");
        }
        geprueft += 1;
    }
    assert!(geprueft > 80, "nur {geprueft} Typen im Katalog");
}

#[test]
#[ignore]
fn illager_zeigt_nur_ein_armpaar() {
    laden();
    let m = layer_model("pillager#main").unwrap();
    let leer = walk_pose(m, Anim::Illager, 0.0, 0.0, 0.0, 0.0, false);
    assert!(leer.get("right_arm").hidden && leer.get("left_arm").hidden && !leer.get("arms").hidden);
    let haelt = walk_pose(m, Anim::Illager, 0.0, 0.0, 0.0, 0.0, true);
    assert!(haelt.get("arms").hidden && !haelt.get("right_arm").hidden);
}
