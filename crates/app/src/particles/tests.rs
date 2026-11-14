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
        visible_frames: Vec::new(),
        block_uvs: HashMap::default(),
        item_uvs: HashMap::default(),
        particles: VecDeque::new(),
        pending: Vec::new(),
        emitters: Vec::new(),
        bursts: Vec::new(),
        unsupported: HashMap::default(),
        new_unsupported: Vec::new(),
        tuning: [[ParticleTuning::default(); KINDS.len()]; 2],
        credit: [[0.0; KINDS.len()]; 2],
        fade_out_ms: 0,
        spawn_distance: VANILLA_SPAWN_DISTANCE,
        emit_own: None,
        emit_scaled: false,
        painted: HashMap::default(),
    }
}

fn blasen_laufen_lassen(drag: bool) -> Vec<Particle> {
    let (w, shapes, _) = welt_mit_saeule(drag);
    let ctx = SpawnContext { camera: DVec3::new(8.5, 2.0, 8.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
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
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
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
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let spritzer = KINDS.iter().position(|k| k.key == "potion_splash").unwrap();
    for (menge, erwartet) in [(50, 50), (200, 200)] {
        let mut p = partikel_mit_bildern();
        let mut tuning = [ParticleTuning::default(); KINDS.len()];
        tuning[spritzer].amount_pct = menge;
        p.set_tuning(tuning, tuning);
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
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
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
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
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
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
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
    let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut p = partikel_mit_bildern();
    p.on_packet(&krit_paket(), &ctx);
    p.tick(&|_| None, &ctx);
    assert_eq!(p.particles.len(), 1);
    let mut aus = [ParticleTuning::default(); KINDS.len()];
    aus[kind_of(pp::PARTICLE_CRIT).unwrap()].enabled = false;
    p.set_tuning(aus, aus);
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
    let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut normal = partikel_mit_bildern();
    normal.on_packet(&krit_paket(), &ctx);
    let vanilla = normal.pending[0].max_age;

    let mut p = partikel_mit_bildern();
    let mut t = [ParticleTuning::default(); KINDS.len()];
    let krit = kind_of(pp::PARTICLE_CRIT).unwrap();
    t[krit].lifetime_pct = 200;
    t[krit].speed_pct = 50;
    p.set_tuning(t, t);
    p.on_packet(&krit_paket(), &ctx);
    assert_eq!(p.pending[0].max_age, vanilla * 2);
    p.tick(&|_| None, &ctx);
    let alter = p.particles[0].age;
    let mut vorher: Option<Vec3> = None;
    for _ in 0..6 {
        for schritt in 0..=4 {
            let mut out = Vec::new();
            p.build_vertices(DVec3::ZERO, Vec3::X, Vec3::Y, schritt as f32 / 4.0, &mut out, None);
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

#[test]
// Eigene Partikel: was an der eigenen Figur entsteht, folgt der eigenen Anpassung -- ein
// Krit dort ist aus, derselbe Krit zwei Bloecke weiter nicht.
fn eigene_partikel_haben_ihre_eigene_anpassung() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let mut aus = [ParticleTuning::default(); KINDS.len()];
    aus[kind_of(pp::PARTICLE_CRIT).unwrap()].enabled = false;
    let paket = krit_paket();
    let ort = DVec3::new(paket.pos.0, paket.pos.1, paket.pos.2);
    for (abstand, erwartet) in [(0.0, 0), (2.0, 1)] {
        let me = NearPlayer { pos: ort - DVec3::new(abstand, 1.0, 0.0), vel_y: 0.0 };
        let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: Some(me) };
        let mut p = partikel_mit_bildern();
        p.set_tuning([ParticleTuning::default(); KINDS.len()], aus);
        p.on_packet(&paket, &ctx);
        assert_eq!(p.pending.len(), erwartet, "Abstand {abstand}");
    }
}

// Explosions-Rueckstoss (kein Vanilla): doppelt so stark wie beim Entity, weg vom Zentrum,
// ausser Reichweite keiner, und ein Block auf dem einzigen Strahl schirmt ganz ab.
#[test]
fn explosion_stoesst_partikel_weg_ausser_hinter_bloecken() {
    let (mut w, _, _) = welt_mit_saeule(false);
    let shapes = world::BlockShapes::new(vec![None; world::state_count()]);
    let mitte = DVec3::new(0.5, 10.5, 0.5);
    let ziel = DVec3::new(4.5, 10.5, 0.5);
    let stoss = super::ExplosionPush::new(mitte, 4.0, &w, &shapes).at(ziel).expect("in Reichweite");
    assert!((stoss.x - 2.0 * (1.0 - 4.0 / 8.0)).abs() < 1e-9 && stoss.y.abs() < 1e-9 && stoss.z.abs() < 1e-9, "{stoss:?}");
    let schraeg = super::ExplosionPush::new(mitte, 4.0, &w, &shapes).at(DVec3::new(-2.0, 13.0, 1.7)).expect("schraeg in Reichweite");
    assert!(schraeg.x < 0.0 && schraeg.y > 0.0 && schraeg.z > 0.0, "{schraeg:?}");
    assert!(super::ExplosionPush::new(mitte, 4.0, &w, &shapes).at(DVec3::new(9.0, 10.5, 0.5)).is_none(), "ausser Reichweite");
    assert!(super::ExplosionPush::new(mitte, 4.0, &w, &shapes).at(mitte).is_none(), "im Zentrum keine Richtung");
    let stein = (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).expect("stone");
    w.set_block(2, 10, 0, stein);
    assert!(super::ExplosionPush::new(mitte, 4.0, &w, &shapes).at(ziel).is_none(), "Block dazwischen schirmt ab");
    assert!(super::ExplosionPush::new(mitte, 4.0, &w, &shapes).at(DVec3::new(0.5, 10.5, 4.5)).is_some(), "andere Richtung frei");
}

// Messung: was der Explosions-Rueckstoss bei vollem Partikelbestand kostet (release: 1,9 ms
// ohne, 1,0 ms mit dem Block-Gedaechtnis von ExplosionPush).
#[test]
#[ignore]
fn bench_explosion_push() {
    let (w, _, _) = welt_mit_saeule(false);
    let shapes = world::BlockShapes::new(vec![None; world::state_count()]);
    let mitte = DVec3::new(0.5, 10.5, 0.5);
    let mut r = JavaRandom::new(3);
    let punkte: Vec<DVec3> = (0..16_384).map(|_| mitte + DVec3::new(r.next_double() * 12.0 - 6.0, r.next_double() * 12.0 - 6.0, r.next_double() * 12.0 - 6.0)).collect();
    let t = std::time::Instant::now();
    let mut n = 0;
    let mut stoss = super::ExplosionPush::new(mitte, 6.0, &w, &shapes);
    for p in &punkte {
        n += usize::from(stoss.at(*p).is_some());
    }
    eprintln!("WERT 16384 Partikel: {:.2} ms, {n} gestossen", t.elapsed().as_secs_f64() * 1000.0);
}

// Totem-Pop durch eine Explosion: der Pop kommt im selben Tick VOR dem Explosionspaket an.
// Dann bekommen auch die Partikel, die der Emitter erst danach spawnt, den Stoss -- ein Pop,
// der erst nach der Explosion beginnt, nicht.
#[test]
fn totem_pop_durch_explosion_fliegt_ganz_weg() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let body = |_: i32| Some(EmitterBody { pos: DVec3::new(8.5, 10.0, 4.5), width: 0.6, height: 1.8 });
    let explosion = pp::Explosion {
        center: (8.5, 10.9, 8.5),
        radius: 6.0,
        block_count: 0,
        knockback: None,
        particle: ParticleEffect { type_id: pp::PARTICLE_EXPLOSION, options: ParticleOptions::None },
        sound: None,
        block_particles: Vec::new(),
    };
    let totem = kind_of(pp::PARTICLE_TOTEM_OF_UNDYING).unwrap() as u8;
    // Geschwindigkeit in z der Totem-Partikel, die im Tick NACH der Explosion entstehen.
    let spaetere = |pop_zuerst: bool| {
        let mut p = partikel_mit_bildern();
        // Feste Saat, sonst schwankt der Mittelwert von Lauf zu Lauf.
        p.rng = JavaRandom::new(42);
        if pop_zuerst {
            p.add_emitter(7, pp::PARTICLE_TOTEM_OF_UNDYING, 30, &body, &ctx);
            p.on_explosion(&explosion, &ctx);
        } else {
            p.on_explosion(&explosion, &ctx);
            p.add_emitter(7, pp::PARTICLE_TOTEM_OF_UNDYING, 30, &body, &ctx);
        }
        p.tick(&body, &ctx);
        let schon = p.particles.len();
        p.tick(&body, &ctx);
        let neu: Vec<f64> = p.particles.iter().skip(schon).filter(|t| t.kind == totem).map(|t| t.vel.z).collect();
        assert!(!neu.is_empty(), "der Emitter hat nichts gespawnt");
        neu
    };
    // Ohne Stoss fliegen Totem-Partikel in alle Richtungen (bis etwa 0,7 je Tick), im Mittel
    // also nirgends hin; mit Stoss im Mittel deutlich weg von der Explosion (-z).
    let mittel = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let gestossen = spaetere(true);
    assert!(mittel(&gestossen) < -0.8, "Pop vor der Explosion: {gestossen:?}");
    let ungestossen = spaetere(false);
    assert!(mittel(&ungestossen).abs() < 0.4, "Pop nach der Explosion: {ungestossen:?}");
}

// Ein Totem-Partikel wird erst gezeichnet, wenn er sich einmal bewegt hat; ein von einer
// Explosion gestossener macht seinen ersten Schritt sofort (und ist damit gleich sichtbar).
#[test]
fn totem_erst_sichtbar_wenn_bewegt_und_stoss_bewegt_sofort() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let effekt = ParticleEffect { type_id: pp::PARTICLE_TOTEM_OF_UNDYING, options: ParticleOptions::None };
    let mut p = partikel_mit_bildern();
    let start = DVec3::new(8.5, 10.5, 4.5);
    p.add_client(&effekt, false, false, start, DVec3::ZERO, &ctx);
    let neu: Vec<Particle> = p.pending.drain(..).collect();
    p.particles.extend(neu);
    assert_eq!(p.particles.len(), 1);
    let mut verts = Vec::new();
    let zeichnen = |p: &Particles, verts: &mut Vec<ParticleVertex>| p.build_vertices(DVec3::ZERO, Vec3::X, Vec3::Y, 0.5, verts, None);
    zeichnen(&p, &mut verts);
    assert!(verts.is_empty(), "noch nicht bewegt, trotzdem gezeichnet");
    let explosion = pp::Explosion {
        center: (8.5, 10.5, 8.5),
        radius: 6.0,
        block_count: 0,
        knockback: None,
        particle: ParticleEffect { type_id: pp::PARTICLE_EXPLOSION, options: ParticleOptions::None },
        sound: None,
        block_particles: Vec::new(),
    };
    p.on_explosion(&explosion, &ctx);
    let t = p.particles.iter().find(|t| usize::from(t.kind) == kind_of(pp::PARTICLE_TOTEM_OF_UNDYING).unwrap()).expect("totem");
    assert_eq!(t.age, 1, "kein sofortiger Schritt");
    assert!(t.pos.z < start.z - 0.5, "nicht vom Fleck: {:?}", t.pos);
    zeichnen(&p, &mut verts);
    assert!(!verts.is_empty(), "bewegt, aber nicht gezeichnet");
}

// Partikel ausblenden: in den letzten fade_out_ms linear auf 0, vorher voll; ein opakes
// Partikel wechselt dabei ins durchscheinende Blatt. Auch fuer die, die in Vanilla schon
// halb ausblenden.
#[test]
fn partikel_blenden_am_lebensende_aus() {
    let mut ps = partikel_mit_bildern();
    let mut p = Particle::base(&mut JavaRandom::new(1), DVec3::ZERO);
    p.max_age = 20;
    p.sheet = Sheet::Opaque;
    assert_eq!(ps.fade_out(&p, 0.0), 1.0, "aus");
    ps.fade_out_ms = 250;
    p.age = 0;
    assert_eq!(ps.fade_out(&p, 0.0), 1.0);
    assert_eq!(ps.sheet_at(&p, 0.0), Sheet::Opaque);
    // Letztes Bild: age == max_age, noch ein Tick = 50 ms von 250.
    p.age = 20;
    assert!((ps.fade_out(&p, 0.0) - 0.2).abs() < 1e-6, "{}", ps.fade_out(&p, 0.0));
    assert!((ps.fade_out(&p, 0.5) - 0.1).abs() < 1e-6);
    assert_eq!(ps.sheet_at(&p, 0.0), Sheet::Translucent);
    p.fades_out = true;
    assert!((ps.fade_out(&p, 0.0) - 0.2).abs() < 1e-6, "auch Vanilla-Ausblender (Totem) blenden ganz aus");
}


// Mit Coral-Ausblenden faellt das Vanilla-Ausblenden (Totem: bis halbe Deckkraft) weg, ohne
// bleibt es wie in Vanilla.
#[test]
fn coral_ausblenden_ersetzt_vanilla_ausblenden() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let effekt = ParticleEffect { type_id: pp::PARTICLE_TOTEM_OF_UNDYING, options: ParticleOptions::None };
    let mut p = partikel_mit_bildern();
    p.add_client(&effekt, false, false, DVec3::new(8.5, 30.5, 8.5), DVec3::ZERO, &ctx);
    let neu: Vec<Particle> = p.pending.drain(..).collect();
    p.particles.extend(neu);
    // Mitte des Lebens, von Vanilla schon abgedunkelt; das Coral-Ausblenden hat noch nicht begonnen.
    let t = &mut p.particles[0];
    t.age = 40;
    t.max_age = 70;
    t.color[3] = 0.6;
    let mut verts = Vec::new();
    let alpha = |p: &Particles, verts: &mut Vec<ParticleVertex>| {
        p.build_vertices(DVec3::ZERO, Vec3::X, Vec3::Y, 0.0, verts, None);
        verts[0].color[3]
    };
    assert!((alpha(&p, &mut verts) - 0.6).abs() < 1e-6, "ohne Coral-Ausblenden wie Vanilla");
    p.fade_out_ms = 250;
    assert!((alpha(&p, &mut verts) - 1.0).abs() < 1e-6, "mit Coral-Ausblenden voll");
}

// Endet die Bilderreihe mit durchsichtigen Bildern (Pack kuerzt die Animation), endet das
// Ausblenden beim ersten davon -- die Animation wird nicht laenger.
#[test]
fn ausblenden_endet_beim_ersten_leeren_bild() {
    // Atlas 4x1: Bilder 0 und 1 sichtbar, 2 fast durchsichtig (unter der Schwelle), 3 leer.
    let mut rgba = vec![255u8; 16];
    rgba[2 * 4 + 3] = 20;
    rgba[3 * 4 + 3] = 0;
    let list: Vec<[f32; 4]> = (0..4).map(|i| [i as f32 / 4.0, 0.0, (i + 1) as f32 / 4.0, 1.0]).collect();
    assert_eq!(visible_frames(&list, &rgba, 4), 2);
    rgba[3] = 0;
    assert_eq!(visible_frames(&list, &rgba, 4), 2, "nur leere Bilder am Ende zaehlen");

    let mut ps = partikel_mit_bildern();
    let totem = pp::PARTICLE_TOTEM_OF_UNDYING as usize;
    ps.sprites[totem] = list;
    ps.visible_frames = vec![usize::MAX; pp::PARTICLE_NAMES.len()];
    ps.visible_frames[totem] = 2;
    ps.fade_out_ms = 100;
    let mut p = Particle::base(&mut JavaRandom::new(1), DVec3::ZERO);
    p.max_age = 30;
    p.animated_sprites = Some(pp::PARTICLE_TOTEM_OF_UNDYING);
    // Bild 2 kommt ab Alter 20 (20 * 3 / 30); davor ein Tick = 50 ms von 100.
    assert_eq!(ps.visible_end(&p), 20);
    assert_eq!(sprite_by_age(&ps.sprites[totem], 19, 30), ps.sprites[totem][1]);
    assert_eq!(sprite_by_age(&ps.sprites[totem], 20, 30), ps.sprites[totem][2]);
    p.age = 19;
    assert!((ps.fade_out(&p, 0.0) - 0.5).abs() < 1e-6, "{}", ps.fade_out(&p, 0.0));
    p.age = 20;
    assert_eq!(ps.fade_out(&p, 0.0), 0.0);
    // Ohne leere Bilder wie bisher bis max_age.
    ps.visible_frames[totem] = 4;
    assert_eq!(ps.visible_end(&p), 31);
}

// Partikel-Distanz: ohne `force` entsteht nichts jenseits von spawn_distance, darunter schon.
#[test]
fn partikel_distanz_einstellbar() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let mut ps = partikel_mit_bildern();
    let krit = ParticleEffect { type_id: pp::PARTICLE_CRIT, options: ParticleOptions::None };
    let weit = DVec3::new(40.5, 6.0, 0.5);
    ps.add_client(&krit, false, false, weit, DVec3::ZERO, &ctx);
    assert!(ps.pending.is_empty() && ps.particles.is_empty(), "40 Bloecke: Vanilla-Grenze 32");
    ps.spawn_distance = 64.0;
    ps.add_client(&krit, false, false, weit, DVec3::ZERO, &ctx);
    assert_eq!(ps.pending.len() + ps.particles.len(), 1, "mit 64 Bloecken entsteht er");
}

// Krit mit 300 % Menge an einem Gegner direkt neben der eigenen Figur: der Emitter wuerfelt
// dreimal so oft (jedes Partikel eigene Richtung, keine Haeufchen aus Kopien), und die
// Partikel zaehlen als fremde, obwohl sie im Bereich der eigenen Figur entstehen.
#[test]
fn krit_menge_ohne_haeufchen_und_fremd_neben_einem() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ich = DVec3::new(8.5, 10.0, 8.5);
    let ctx = SpawnContext {
        camera: ich,
        mode: ParticleMode::All,
        mode_own: ParticleMode::All,
        world: &w,
        shapes: &shapes,
        biomes: &world::BiomePalette::fallback(),
        player: Some(NearPlayer { pos: ich, vel_y: 0.0 }),
    };
    // Der Gegner steht 0,4 Bloecke neben einem -- mitten im Bereich, der sonst als eigen gilt.
    let gegner = DVec3::new(8.9, 10.0, 8.5);
    let body = |id: i32| Some(EmitterBody { pos: if id == 1 { ich } else { gegner }, width: 0.6, height: 1.8 });
    let krit = kind_of(pp::PARTICLE_CRIT).unwrap();
    let zaehlen = |menge: u32| {
        let mut p = partikel_mit_bildern();
        p.rng = JavaRandom::new(7);
        let mut t = [ParticleTuning::default(); KINDS.len()];
        t[krit].amount_pct = menge;
        p.set_tuning(t, t);
        p.add_emitter(2, pp::PARTICLE_CRIT, 1, &body, &ctx);
        p.pending.iter().filter(|t| t.kind as usize == krit).map(|t| (t.own, t.vel)).collect::<Vec<_>>()
    };
    let einfach = zaehlen(100);
    let dreifach = zaehlen(300);
    assert!(einfach.iter().all(|(own, _)| !own), "Krit am Gegner ist kein eigener");
    assert!(dreifach.len() > einfach.len() * 2, "{} gegen {}", dreifach.len(), einfach.len());
    // Keine Haeufchen: kaum zwei Partikel mit fast gleicher Richtung.
    let nah = |a: DVec3, b: DVec3| (a - b).length() < 0.05;
    let paare = (0..dreifach.len()).flat_map(|i| (i + 1..dreifach.len()).map(move |j| (i, j))).filter(|&(i, j)| nah(dreifach[i].1, dreifach[j].1)).count();
    assert!(paare * 10 < dreifach.len(), "{paare} fast gleiche Paare bei {}", dreifach.len());
    // Der eigene Krit bleibt eigen.
    let mut p = partikel_mit_bildern();
    p.add_emitter(1, pp::PARTICLE_CRIT, 1, &body, &ctx);
    assert!(p.pending.iter().filter(|t| t.kind as usize == krit).all(|t| t.own));
}

// Krit-Spur eines Pfeils mit 300 % Menge: zwoelf statt vier Partikel, gleichmaessig auf der
// Strecke eines Ticks verteilt -- nicht vier Haeufchen aus je drei Kopien (Nutzer 2026-11-12).
#[test]
fn spur_verteilt_mehr_menge_gleichmaessig() {
    let (w, shapes, _) = welt_mit_saeule(false);
    let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, mode_own: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
    let krit = ParticleEffect { type_id: pp::PARTICLE_CRIT, options: ParticleOptions::None };
    let art = kind_of(pp::PARTICLE_CRIT).unwrap();
    let orte = |menge: u32| {
        let mut p = partikel_mit_bildern();
        let mut t = [ParticleTuning::default(); KINDS.len()];
        t[art].amount_pct = menge;
        p.set_tuning(t, t);
        p.add_trail(&krit, DVec3::new(2.0, 6.0, 0.5), DVec3::new(3.0, 0.0, 0.0), DVec3::ZERO, 4, &ctx);
        // `last` ist der Ort vor dem Tick des Konstruktors, also wo es gesetzt wurde.
        let mut x: Vec<f64> = p.pending.iter().map(|t| t.last.x).collect();
        x.sort_by(f64::total_cmp);
        x
    };
    assert_eq!(orte(100), vec![2.0, 2.75, 3.5, 4.25]);
    let dreifach = orte(300);
    assert_eq!(dreifach.len(), 12);
    for paar in dreifach.windows(2) {
        assert!((paar[1] - paar[0] - 0.25).abs() < 1e-9, "{dreifach:?}");
    }
    // Weniger Menge: weiter vier Stellen, push laesst welche weg.
    assert_eq!(orte(50).len(), 2);
}

// Bemaltes Rechteck eines Bildes als Anteil -- auch bei doppelter Aufloesung derselbe Anteil,
// und bei gespiegeltem UV von der anderen Seite gezaehlt.
#[test]
fn bemaltes_rechteck_als_anteil() {
    // 4x4-Atlas, ein Bild ueber alles, bemalt nur Spalte 1..=2 in Zeile 2..=3.
    let bild = |n: usize| {
        let mut rgba = vec![0u8; n * n * 4];
        for y in 0..n {
            for x in 0..n {
                if (n / 4..3 * n / 4).contains(&x) && y >= n / 2 {
                    rgba[(y * n + x) * 4 + 3] = 255;
                }
            }
        }
        rgba
    };
    assert_eq!(painted_bounds([0.0, 0.0, 1.0, 1.0], &bild(4), 4), Some([0.25, 0.5, 0.75, 1.0]));
    assert_eq!(painted_bounds([0.0, 0.0, 1.0, 1.0], &bild(32), 32), Some([0.25, 0.5, 0.75, 1.0]), "HD-Pack: derselbe Anteil");
    assert_eq!(painted_bounds([0.0, 1.0, 1.0, 0.0], &bild(4), 4), Some([0.25, 0.0, 0.75, 0.5]), "v gespiegelt");
    assert_eq!(painted_bounds([0.0, 0.0, 1.0, 1.0], &[0u8; 64], 4), None, "leer");
}

#[test]
fn emitter_partikel_entstehen_auf_dem_rand_der_hitbox() {
    let b = EmitterBody { pos: DVec3::new(8.5, 10.0, 4.5), width: 0.6, height: 1.8 };
    // Zur Seite: an der Seitenwand, auf der Hoehe, die der Strahl aus der Mitte dort hat.
    let p = spawners::body_edge(b, DVec3::new(0.6, 0.3, 0.0));
    assert!((p.x - 8.8).abs() < 1e-9 && (p.y - 11.05).abs() < 1e-9 && (p.z - 4.5).abs() < 1e-9, "{p:?}");
    // Steil nach oben: auf dem Deckel.
    let p = spawners::body_edge(b, DVec3::new(0.0, 1.0, -0.1));
    assert!((p.y - 11.8).abs() < 1e-9 && (p.z - 4.41).abs() < 1e-9, "{p:?}");
    // Jede Richtung landet auf dem Rand, nie innen.
    for v in [DVec3::new(-0.2, -0.7, 0.4), DVec3::new(0.9, 0.2, 0.9), DVec3::new(0.01, 0.2, -0.5)] {
        let d = spawners::body_edge(b, v) - DVec3::new(8.5, 10.9, 4.5);
        let rand = (d.x.abs() / 0.3).max(d.y.abs() / 0.9).max(d.z.abs() / 0.3);
        assert!((rand - 1.0).abs() < 1e-9, "{v:?} -> {d:?}");
    }
}
