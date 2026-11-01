use super::*;

#[test]
// Die ersten Werte von new Random(42) -- aus der JDK-Dokumentation bekannt und leicht
// nachpruefbar. Stimmt das LCG, stimmen auch Gauss und nextInt.
fn java_random_stimmt_mit_der_jdk_folge_ueberein() {
    let mut r = JavaRandom::new(42);
    assert_eq!(r.next_int(10), 0);
    assert_eq!(r.next_int(10), 3);
    let mut r = JavaRandom::new(42);
    assert_eq!(r.next(32), -1170105035);
}

#[test]
fn sprite_nach_alter_laeuft_die_liste_ab() {
    let list: Vec<[f32; 4]> = (0..8).map(|i| [i as f32; 4]).collect();
    assert_eq!(sprite_by_age(&list, 0, 10)[0], 0.0);
    assert_eq!(sprite_by_age(&list, 10, 10)[0], 7.0);
    assert_eq!(sprite_by_age(&list, 5, 10)[0], 3.0);
}

// Eine Welt mit einer fuenf Bloecke hohen Blasensaeule bei x=8, z=8 (y 0..=4), darueber
// Luft. Die Saeule hat keine Kollision.
fn welt_mit_saeule(drag: bool) -> (world::World, world::BlockShapes, u32) {
    let mut w = world::World::new();
    for cx in -2..=2 {
        for cz in -2..=2 {
            let sections = (0..24).map(|_| world::ChunkSection::from_states(vec![0u32; 4096])).collect();
            w.insert_column(cx, cz, sections);
        }
    }
    let wert = if drag { "true" } else { "false" };
    let saeule = (0..world::state_count() as u32)
        .find(|&s| world::block_name(s) == Some("bubble_column") && world::state_props::property(s, "drag") == Some(wert))
        .expect("bubble_column");
    for y in 0..=4 {
        w.set_block(8, y, 8, saeule);
    }
    let mut per_state = vec![None; world::state_count()];
    per_state[saeule as usize] = Some(Vec::new());
    (w, world::BlockShapes::new(per_state), saeule)
}

fn partikel_mit_bildern() -> Particles {
    Particles {
        rng: JavaRandom::new(7),
        sprites: vec![vec![[0.0, 0.0, 1.0, 1.0]]; pp::PARTICLE_NAMES.len()],
        block_uvs: HashMap::default(),
        item_uvs: HashMap::default(),
        particles: VecDeque::new(),
        pending: Vec::new(),
        emitters: Vec::new(),
        bursts: Vec::new(),
        unsupported: HashMap::default(),
        new_unsupported: Vec::new(),
        tuning: [ParticleTuning::default(); KINDS.len()],
        credit: [0.0; KINDS.len()],
    }
}

fn blasen_laufen_lassen(drag: bool) -> Vec<Particle> {
    let (w, shapes, _) = welt_mit_saeule(drag);
    let ctx = SpawnContext { camera: DVec3::new(8.5, 2.0, 8.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut p = partikel_mit_bildern();
    let mut gesehen = Vec::new();
    for _ in 0..200 {
        p.animate_blocks((8, 2, 8), &ctx);
        p.tick(&|_| None, &ctx);
        for t in &p.particles {
            assert!(in_water(&w, t.pos), "Blase ausserhalb des Wassers: {:?}", t.pos);
        }
        gesehen.extend(p.particles.iter().filter(|t| t.age > 5).cloned());
    }
    assert!(!gesehen.is_empty(), "die Saeule hat keine Blasen gemacht");
    gesehen
}

// Zerplatzender Trank: hundert Effekt-Partikel in der Trankfarbe (die Splitter brauchen ein
// Bild im Atlas, das hat der Test nicht), alle steigen zunaechst.
#[test]
fn trank_zerplatzt_in_seiner_farbe() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut p = partikel_mit_bildern();
    p.add_potion_splash((0, 5, 0), 0xFF0000, false, &ctx);
    assert_eq!(p.pending.len(), 100);
    assert!(p.pending.iter().all(|t| t.color[0] >= 0.74 && t.color[1] == 0.0 && t.color[2] == 0.0 && t.color[3] == 1.0));
    // Eigene Art, getrennt von den Wirbeln um Wesen.
    assert!(p.pending.iter().all(|t| KINDS[t.kind as usize].key == "potion_splash"));
}

// Menge 50 %: von hundert Spritzern bleibt genau die Haelfte; 200 % verdoppelt.
#[test]
fn menge_skaliert_den_schwall() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let spritzer = KINDS.iter().position(|k| k.key == "potion_splash").unwrap();
    for (menge, erwartet) in [(50, 50), (200, 200)] {
        let mut p = partikel_mit_bildern();
        let mut tuning = [ParticleTuning::default(); KINDS.len()];
        tuning[spritzer].amount_pct = menge;
        p.set_tuning(tuning);
        p.add_potion_splash((0, 5, 0), 0xFF0000, false, &ctx);
        assert_eq!(p.pending.len(), erwartet, "Menge {menge} %");
    }
}

// Die Arten stehen alphabetisch, und jede hat genau die Regler, die zu ihr passen.
#[test]
fn arten_alphabetisch_und_spritzer_getrennt() {
    let falten = |s: &str| s.to_lowercase().replace('ä', "a").replace('ö', "o").replace('ü', "u");
    assert!(KINDS.windows(2).all(|w| falten(w[0].label) < falten(w[1].label)), "nicht alphabetisch");
    assert_ne!(kind_of(pp::PARTICLE_EFFECT), kind_of(pp::PARTICLE_ENTITY_EFFECT));
    assert!(KINDS[kind_of(pp::PARTICLE_ENTITY_EFFECT).unwrap()].spawn.has_interval());
    assert!(KINDS[kind_of(pp::PARTICLE_TOTEM_OF_UNDYING).unwrap()].spawn.has_all_at_once());
}

// Kruemel vor dem Gesicht: mit Bild fuenf Splitter, alle vor den Augen in Blickrichtung
// (Blick nach Sueden = +z) und unterhalb davon. Ohne Bild (Pack hat "particle" geleert)
// keiner.
#[test]
fn kruemel_vor_dem_gesicht() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut p = partikel_mit_bildern();
    let auge = DVec3::new(0.5, 7.0, 0.5);
    p.add_item_use_particles("bread", 5, auge, 0.0, 0.0, &ctx);
    assert!(p.pending.is_empty());
    p.item_uvs.insert("bread".into(), [0.0, 0.0, 0.5, 0.5]);
    p.add_item_use_particles("bread", 5, auge, 0.0, 0.0, &ctx);
    assert_eq!(p.pending.len(), 5);
    for t in &p.pending {
        assert!((t.pos.z - auge.z - 0.6).abs() < 1e-6, "nicht vor dem Gesicht: {:?}", t.pos);
        assert!(t.pos.y < auge.y - 0.29 && t.pos.y > auge.y - 0.91);
        assert_eq!(KINDS[t.kind as usize].key, "item_shards");
    }
}

// Trankwolke: solange sie wartet, hoechstens zwei Wirbel nahe der Mitte; danach so viele,
// wie die Flaeche gross ist, alle im Kreis und alle unter "Trankeffekte".
#[test]
fn trankwolke_fuellt_ihren_kreis() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut p = partikel_mit_bildern();
    let mitte = DVec3::new(0.5, 5.0, 0.5);
    let gruen = ParticleEffect { type_id: pp::PARTICLE_ENTITY_EFFECT, options: ParticleOptions::Tinted(0xFF00_FF00) };
    for _ in 0..10 {
        p.tick_area_cloud(mitte, 3.0, true, &gruen, &ctx);
    }
    assert!(p.pending.len() <= 20);
    assert!(p.pending.iter().all(|t| t.pos.distance(mitte) < 0.5));
    p.pending.clear();
    p.tick_area_cloud(mitte, 3.0, false, &gruen, &ctx);
    assert_eq!(p.pending.len(), 29);
    assert!(p.pending.iter().all(|t| (t.pos - mitte).length() < 3.1 && t.color[1] == 1.0));
    assert!(p.pending.iter().all(|t| KINDS[t.kind as usize].key == "potion"));
}

// Effekt-Wirbel behalten die Deckkraft, mit der sie geschickt wurden: ein Ambient-Effekt
// (Deckkraft 38) bleibt fast durchsichtig, ein normaler voll deckend.
#[test]
fn effekt_wirbel_behalten_ihre_deckkraft() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut p = partikel_mit_bildern();
    let ort = DVec3::new(0.5, 5.0, 0.5);
    // Blindheit, einmal als Ambient-Effekt, einmal normal.
    p.add_potion_swirl(0x261F_1F23, ort, &ctx);
    p.add_potion_swirl(0xFF1F_1F23, ort, &ctx);
    assert_eq!(p.pending.len(), 2);
    let (mut spawns, mut boeen) = (Vec::new(), Vec::new());
    for teil in &mut p.pending {
        teil.max_age = 1000;
        for _ in 0..60 {
            Particles::tick_one(teil, &[], &ctx, &mut spawns, &mut boeen);
        }
    }
    assert!((p.pending[0].color[3] - 38.0 / 255.0).abs() < 1e-3, "Ambient: {}", p.pending[0].color[3]);
    assert!((p.pending[1].color[3] - 1.0).abs() < 1e-3);
}

// Ein gelandeter Partikel faellt weiter, sobald der Block unter ihm weg ist (in Vanilla
// bliebe er in der Luft stehen, s. Particle::move_by).
#[test]
fn gelandeter_partikel_faellt_weiter_wenn_der_boden_fehlt() {
    let (mut w, _, _) = welt_mit_saeule(false);
    let stein = (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).expect("stone");
    w.set_block(2, 5, 2, stein);
    let shapes = world::BlockShapes::new(vec![None; world::state_count()]);
    let mut p = Particle::base(&mut JavaRandom::new(1), DVec3::new(2.5, 6.5, 2.5));
    p.max_age = 1000;
    p.gravity = 1.0;
    for _ in 0..40 {
        p.tick_physics(&w, &shapes);
    }
    assert!(p.stopped && (p.pos.y - 6.0).abs() < 1e-3, "nicht gelandet: {:?}", p.pos);
    w.set_block(2, 5, 2, 0);
    for _ in 0..10 {
        p.tick_physics(&w, &shapes);
    }
    assert!(p.pos.y < 5.9, "bleibt in der Luft stehen: {:?}", p.pos);
}

#[test]
// Ueber Seelensand steigen die Blasen und verschwinden an der Oberflaeche.
fn blasensaeule_steigt_und_bleibt_im_wasser() {
    let blasen = blasen_laufen_lassen(false);
    assert!(blasen.iter().all(|b| b.vel.y > 0.0), "aufsteigende Saeule hat sinkende Blasen");
}

#[test]
// Ueber Magma sinken sie im Sog.
fn magmasaeule_zieht_nach_unten() {
    let blasen = blasen_laufen_lassen(true);
    assert!(blasen.iter().all(|b| b.vel.y < 0.0 && matches!(b.behavior, Behavior::CurrentDown { .. })));
}

#[test]
// Krit-Partikel: Geschwindigkeit fliegt gegen Luftwiderstand 0.7 aus, gruen und blau
// verblassen -- ein Krit wird zum Ende hin rot.
fn krit_wird_rot() {
    let mut p = Particle::base(&mut JavaRandom::new(1), DVec3::ZERO);
    p.color = [0.8, 0.8, 0.8, 1.0];
    p.max_age = 100;
    p.fades_to_red = true;
    for _ in 0..10 {
        Particles::tick_one_without_world(&mut p);
    }
    assert!(p.color[0] > p.color[1] && p.color[1] > p.color[2]);
}

#[test]
fn arten_stimmen_mit_ihren_konstanten() {
    assert_eq!(KINDS[KIND_EXPLOSION].key, "explosion");
    assert_eq!(KINDS[KIND_BLOCK].key, "block");
    assert_eq!(kind_of(pp::PARTICLE_EXPLOSION_EMITTER), Some(KIND_EXPLOSION));
    let mut keys: Vec<_> = KINDS.iter().map(|k| k.key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), KINDS.len(), "Schluessel doppelt");
}

fn krit_paket() -> pp::LevelParticles {
    pp::LevelParticles {
        particle: ParticleEffect { type_id: pp::PARTICLE_CRIT, options: ParticleOptions::None },
        force_spawn: true,
        important: false,
        pos: (8.5, 10.0, 8.5),
        offset: (0.0, 0.0, 0.0),
        speed: 0.0,
        count: 0,
    }
}

#[test]
// Aus: nichts entsteht, und was schon fliegt, verschwindet mit dem Ausschalten.
fn ausgeschaltete_art_verschwindet() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut p = partikel_mit_bildern();
    p.on_packet(&krit_paket(), &ctx);
    p.tick(&|_| None, &ctx);
    assert_eq!(p.particles.len(), 1);
    let mut aus = [ParticleTuning::default(); KINDS.len()];
    aus[kind_of(pp::PARTICLE_CRIT).unwrap()].enabled = false;
    p.set_tuning(aus);
    assert!(p.particles.is_empty());
    p.on_packet(&krit_paket(), &ctx);
    p.tick(&|_| None, &ctx);
    assert!(p.particles.is_empty());
}

#[test]
// Doppelte Dauer: doppelte Lebensdauer. Halbes Tempo: halb so viele eigene Ticks, und das
// Bild gleitet dazwischen ohne Sprung weiter.
fn dauer_und_tempo() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut normal = partikel_mit_bildern();
    normal.on_packet(&krit_paket(), &ctx);
    let vanilla = normal.pending[0].max_age;

    let mut p = partikel_mit_bildern();
    let mut t = [ParticleTuning::default(); KINDS.len()];
    let krit = kind_of(pp::PARTICLE_CRIT).unwrap();
    t[krit].lifetime_pct = 200;
    t[krit].speed_pct = 50;
    p.set_tuning(t);
    p.on_packet(&krit_paket(), &ctx);
    assert_eq!(p.pending[0].max_age, vanilla * 2);
    p.tick(&|_| None, &ctx);
    let alter = p.particles[0].age;
    let mut vorher: Option<Vec3> = None;
    for _ in 0..6 {
        for schritt in 0..=4 {
            let mut out = Vec::new();
            p.build_vertices(DVec3::ZERO, Vec3::X, Vec3::Y, schritt as f32 / 4.0, &mut out);
            let mitte = out.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / out.len() as f32;
            if let Some(v) = vorher {
                assert!((mitte - v).length() < 0.2, "Sprung im Bild: {v} -> {mitte}");
            }
            vorher = Some(mitte);
        }
        p.tick(&|_| None, &ctx);
    }
    assert_eq!(p.particles[0].age - alter, 3, "6 Spielticks bei 50 % = 3 eigene");
}
