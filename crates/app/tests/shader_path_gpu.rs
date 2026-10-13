// Der Shader-Pfad gegen eine echte GPU: Pipelines anlegen und einen Frame mit allen Passes
// (Schatten, Weichzeichner, Licht, Tonemap) aufnehmen. wgpu prueft Bindungen und Nutzung
// erst zur Laufzeit -- ein Fehler dort waere sonst erst im Spiel ein Absturz. Ohne GPU
// (CI) wird der Test uebersprungen.

#[test]
fn shader_path_frame_ohne_validierungsfehler() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    // Mit Zeitstempeln, falls moeglich: dann misst der Shader-Pfad die Back-Zeit (BakeTimer).
    let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: features, required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);

    let atlas = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut sp = render::shader_path::ShaderPath::new(&device, format, classes, atlas);
    // Tests: eine ganze Seite je Frame (im Spiel ein Schritt, s. LM_BAKE_PER_FRAME).
    sp.voxels_mut().set_bake_budget(render::voxel_volume::LM_BAKE_PER_FRAME);
    sp.ensure_size(&device, 64, 48);
    // Eine Section mit Steinboden und einer Fackel darauf: das Licht-Raster bekommt eine Quelle,
    // der Licht-Pass verfolgt Blocklicht-Schatten.
    let fackel = (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some("torch")).expect("torch");
    let stein = (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some("stone")).expect("stone");
    let mut states = vec![world::STATE_AIR; 4096];
    for i in 0..256 {
        states[i] = stein;
    }
    states[(16 + 8) * 16 + 8] = fackel;
    // Eine Mauer zwei Bloecke vor der Fackel: der Boden dahinter liegt im Schatten.
    states[(16 + 10) * 16 + 8] = stein;
    let section = world::ChunkSection::from_states(states);
    // Fackel auf 300 %: Stufe 42, also auch ein Fernlicht mit eigenem Pass.
    lichtstaerken_setzen();
    sp.voxels_mut().update(&queue, [0, 4, 0], |sx, sy, sz| (sx == 0 && sy == 4 && sz == 0).then_some(&section), 16);
    sp.voxels_mut().update_lights(&queue, [8, 70, 8]);
    assert!(sp.voxels_mut().light_origin().is_some());
    assert!(sp.voxels_mut().has_far_lights());

    let output = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 64, height: 48, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let color = sp.gbuffer_attachments();
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &color,
            depth_stencil_attachment: Some(sp.depth_attachment_keep()),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    let frame = render::shader_path::LightFrame {
        sky: wgpu::Color::BLUE,
        daylight: 1.0,
        view: glam::Mat4::IDENTITY,
        proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
        eye: glam::DVec3::new(0.5, 70.0, 0.5),
        celestial: render::shader_path::celestial_light(0.0),
        shadows: true,
        atmosphere: true,
        scattering: 1.0,
        sun_size_deg: 2.0,
        block_shadows: true,
        block_softness_deg: 6.0,
        sun_factor: 1.0,
        lightmap_debug: 0,
    };
    // Drei Frames: der erste ohne Verlauf, danach wechseln die Verlaufsbilder die Seite.
    // Ab dem zweiten Frame backt er auch Lichtkarten (prepare schaltet sie ein).
    let mut gebacken = 0;
    // Je Frame ein Submit wie im Spiel (der BakeTimer liest seine Messung erst danach).
    for _ in 0..5 {
        sp.voxels_mut().update_lights(&queue, [8, 70, 8]);
        gebacken += sp.voxels_mut().bake_jobs().len();
        sp.prepare(&queue, &frame);
        sp.shadows(&mut encoder, &mut |_| None);
        sp.light(&mut encoder, None);
        sp.tonemap(&queue, &mut encoder, &output, 1.0, None);
        queue.submit([std::mem::replace(&mut encoder, device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default())).finish()]);
        device.poll(wgpu::Maintain::Poll);
    }
    assert!(gebacken > 0, "keine Lichtkarte gebacken");
    println!("Backen: {:?} ms je Section (Zeitstempel: {})", sp.bake_ms_per_section(), !features.is_empty());
    queue.submit([encoder.finish()]);
    device.poll(wgpu::Maintain::Wait);
    if let Some(e) = pollster::block_on(device.pop_error_scope()) {
        panic!("wgpu-Validierung: {e}");
    }

    // Ebenen (s. deferred_light.wgsl, cs_lm_layers): neben der Fackel frei, hinter der Mauer
    // keine (ganz im Schatten); dazwischen, wo die Kante der Mauer hinfaellt, ein Sicht-Raster.
    let neben = ebenen_der_flaeche(&device, &queue, sp.voxels_mut(), 9, 8);
    let hinter = ebenen_der_flaeche(&device, &queue, sp.voxels_mut(), 8, 11);
    let kante = ebenen_der_flaeche(&device, &queue, sp.voxels_mut(), 9, 11);
    println!("Ebenen: neben {:?}, hinter {:?}, Kante {:?}", art(&neben), art(&hinter), art(&kante));
    assert_eq!(art(&neben), 1, "neben der Fackel nicht frei");
    assert_eq!(art(&hinter), 0, "hinter der Mauer nicht im Schatten");
    assert!(art(&kante) == 3 && kante[0] >> 30 == 3, "an der Schattenkante kein Sicht-Raster");
    // Das Raster: auf der Flaeche (x 9..10, z 11..12) faellt die Kante der Mauer (x = 9 bei
    // z = 10) von der Fackel aus schraeg -- an einem Ende hell, am anderen dunkel.
    let seite = lesen(&device, &queue, sp.voxels_mut().lightmap_buffer(), 1024, 1)[0] as u64;
    let fw = render::voxel_volume::LM_FEIN_WORTE as u64;
    let raster = lesen(&device, &queue, sp.voxels_mut().lightmap_buffer(), render::voxel_volume::LM_FEIN_BASE as u64 + ((seite - 1) * render::voxel_volume::LM_FEIN_PLAETZE as u64 + kante[2] as u64) * fw, fw);
    let werte: Vec<u32> = raster.iter().flat_map(|w| (0..4).map(move |i| w >> (8 * i) & 255)).collect();
    println!("Raster: kleinster {}, groesster {}", werte.iter().min().unwrap(), werte.iter().max().unwrap());
    assert!(*werte.iter().min().unwrap() < 30 && *werte.iter().max().unwrap() > 225, "Raster an der Kante ohne Hell und Dunkel");

    // Dieselbe Welt noch einmal backen: genau dieselben Texel (die Reihenfolge der Flaechen
    // in der Seite darf sich aendern, ihr Licht nicht). Neu geschrieben mit gleichem Inhalt
    // baeckt nichts neu -- also Lichtkarten aus und wieder an.
    sp.voxels_mut().mark_all_stale();
    sp.voxels_mut().update(&queue, [0, 4, 0], |sx, sy, sz| (sx == 0 && sy == 4 && sz == 0).then_some(&section), 16);
    sp.voxels_mut().set_lightmaps(&queue, None);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut neu_gebacken = 0;
    for _ in 0..3 {
        sp.voxels_mut().update_lights(&queue, [8, 70, 8]);
        neu_gebacken += sp.voxels_mut().bake_jobs().len();
        sp.prepare(&queue, &frame);
        sp.shadows(&mut encoder, &mut |_| None);
        queue.submit([std::mem::replace(&mut encoder, device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default())).finish()]);
    }
    queue.submit([encoder.finish()]);
    device.poll(wgpu::Maintain::Wait);
    assert!(neu_gebacken > 0, "nicht neu gebacken");
    assert_eq!(ebenen_der_flaeche(&device, &queue, sp.voxels_mut(), 9, 8), neben, "neu gebacken anders");
    // Der Platz des Rasters im Vorrat darf anders sein (Atomics), die Ebene sonst nicht.
    let ohne_platz = |mut e: Vec<u32>| {
        e[2] = 0;
        e
    };
    assert_eq!(ohne_platz(ebenen_der_flaeche(&device, &queue, sp.voxels_mut(), 9, 11)), ohne_platz(kante.clone()), "neu gebacken anders");
}

// Ausgabe einer Szene sammeln und am Ende am Stueck drucken: die Tests laufen parallel.
macro_rules! say {
    ($log:expr, $($t:tt)*) => {{
        use std::fmt::Write as _;
        let _ = writeln!($log, $($t)*);
    }};
}

// `f` fuer alle `dinge`, auf alle Kerne verteilt (die Vergleichsrechnungen auf der CPU sind
// das Langsame an diesen Tests); Ergebnisse in derselben Reihenfolge.
fn parallel<T: Sync, R: Send>(dinge: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let kerne = std::thread::available_parallelism().map_or(4, |n| n.get());
    let stueck = dinge.len().div_ceil(kerne).max(1);
    std::thread::scope(|s| {
        let teile: Vec<_> = dinge.chunks(stueck).map(|teil| s.spawn(|| teil.iter().map(&f).collect::<Vec<R>>())).collect();
        teile.into_iter().flat_map(|t| t.join().unwrap()).collect()
    })
}

// Art der ersten Ebene: 0 keine, 1 frei, 2 Linien, 3 Strahlen.
fn art(ebenen: &[u32]) -> u32 {
    ebenen[0] >> 27 & 3
}

// Die Lichtstaerken sind global und die Tests laufen parallel: alle setzen dieselbe Liste
// (Fackel und Glowstone dreifach), sonst setzt einer die Quelle des anderen zurueck.
fn lichtstaerken_setzen() {
    world::light::set_light_factors(vec![("torch".to_string(), 3.0), ("glowstone".to_string(), 3.0), ("lantern".to_string(), 3.0)]);
}

fn lesen(device: &wgpu::Device, queue: &wgpu::Queue, puffer: &wgpu::Buffer, start: u64, anzahl: u64) -> Vec<u32> {
    let ziel = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: anzahl * 4, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    enc.copy_buffer_to_buffer(puffer, start * 4, &ziel, 0, anzahl * 4);
    queue.submit([enc.finish()]);
    ziel.slice(..).map_async(wgpu::MapMode::Read, |_| ());
    device.poll(wgpu::Maintain::Wait);
    let werte = bytemuck::cast_slice(&ziel.slice(..).get_mapped_range()).to_vec();
    werte
}

// Die Ebenen der Lichtkarte auf der Oberseite des Bodenblocks (x, 64, z) in Section (0, 4, 0)
// (Aufteilung s. deferred_light.wgsl, lm und lm_lagen).
fn ebenen_der_flaeche(device: &wgpu::Device, queue: &wgpu::Queue, voxels: &render::voxel_volume::VoxelVolume, x: u64, z: u64) -> Vec<u32> {
    // Section (0, 4, 0) liegt in Slot (4 * 16 + 0) * 16 + 0.
    let seite = lesen(device, queue, voxels.lightmap_buffer(), 1024, 1)[0];
    assert!(seite > 0, "Section ohne Lichtkarten-Seite");
    let flaeche = (z * 16 + x) * 6 + 1;
    let platz = lesen(device, queue, voxels.lightmap_buffer(), 4096 + (seite as u64 - 1) * 24576 + flaeche, 1)[0];
    assert!(platz > 0 && platz != u32::MAX, "keine Lichtkarte fuer ({x}, {z})");
    let n = render::voxel_volume::LM_LAGEN as u64;
    lesen(device, queue, voxels.lightmap_layer_buffer(), ((seite as u64 - 1) * render::voxel_volume::LM_FACES as u64 + platz as u64 - 1) * n * 4, n * 4)
}

// Die Lichtkarten gegen eine unabhaengige Rechnung: fuer jede gebackene Flaeche und jede
// Laterne an einem Raster von Punkten, ob die Linien der Ebene dieselbe Sicht ergeben wie ein
// Strahl durch die Bloecke (auf der CPU). Szenen wie im Spiel gesehen: eine Laterne in einer
// Nische einer Wand; zwei Laternen auf dem Boden mit einem Block dazwischen.
#[test]
fn lichtkarten_stimmen_mit_strahlen() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let atlas = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let mut sp = render::shader_path::ShaderPath::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, classes, atlas);
    // Tests: eine ganze Seite je Frame (im Spiel ein Schritt, s. LM_BAKE_PER_FRAME).
    sp.voxels_mut().set_bake_budget(render::voxel_volume::LM_BAKE_PER_FRAME);
    sp.ensure_size(&device, 64, 48);

    let laterne = (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some("lantern")).expect("lantern");
    let stein = (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some("stone")).expect("stone");
    // Sections (0, 4, 0) und (0, 4, 1): Weltbloecke x 0..16, y 64..80, z 0..32.
    let mut fest = vec![false; 2 * 4096];
    let idx = |x: i32, y: i32, z: i32| ((z / 16) * 4096 + ((y - 64) * 16 + z % 16) * 16 + x) as usize;
    let mut setzen = |x: i32, y: i32, z: i32| fest[idx(x, y, z)] = true;
    for x in 0..16 {
        for z in 0..32 {
            setzen(x, 64, z);
        }
    }
    // Wand bei z = 16 mit Loch bei (8, 67): die erste Reihe der naechsten Section. Ein Strahl,
    // der die Grenze knapp neben einer Kante des Lochs kreuzte, sprang frueher ueber die erste
    // Zelle dahinter hinweg -- Licht kam durch einen haarfeinen Spalt.
    for x in 5..12 {
        for y in 65..70 {
            if (x, y) != (8, 67) {
                setzen(x, y, 16);
            }
        }
    }
    // Wand bei z = 10 (x 5..11, y 65..69) mit Nische bei (8, 67), Rueckwand dahinter.
    for x in 5..12 {
        for y in 65..70 {
            if (x, y) != (8, 67) {
                setzen(x, y, 10);
            }
        }
    }
    setzen(8, 67, 11);
    // Ein Block zwischen zwei Laternen auf dem Boden.
    setzen(5, 65, 4);
    // Zwei Saeulen, die sich nur in einer Kante beruehren (x 4, z 20), die Laterne genau
    // diagonal davor: ihr Schatten trifft sich auf den Bodenflaechen genau auf der Diagonale.
    for y in 65..70 {
        setzen(3, y, 20);
        setzen(4, y, 19);
    }
    // Ein geschlossener Raum (Huelle x 10..14, y 65..69, z 24..28): innen sieht nichts den
    // Himmel.
    for x in 10..15 {
        for y in 65..70 {
            for z in 24..29 {
                if x == 10 || x == 14 || y == 65 || y == 69 || z == 24 || z == 28 {
                    setzen(x, y, z);
                }
            }
        }
    }
    let lichter = [(8, 67, 10), (3, 65, 3), (12, 65, 3), (8, 67, 13), (8, 70, 24)];
    let mut states = vec![world::STATE_AIR; 2 * 4096];
    for (i, &f) in fest.iter().enumerate() {
        if f {
            states[i] = stein;
        }
    }
    for &(x, y, z) in &lichter {
        states[idx(x, y, z)] = laterne;
    }
    let sections = [world::ChunkSection::from_states(states[..4096].to_vec()), world::ChunkSection::from_states(states[4096..].to_vec())];
    sp.voxels_mut().update(&queue, [0, 4, 0], |sx, sy, sz| (sx == 0 && sy == 4 && (0..2).contains(&sz)).then(|| &sections[sz as usize]), 16);
    sp.voxels_mut().set_world_bottom(64);
    let frame = render::shader_path::LightFrame {
        sky: wgpu::Color::BLUE,
        daylight: 1.0,
        view: glam::Mat4::IDENTITY,
        proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
        eye: glam::DVec3::new(8.5, 70.0, 8.5),
        celestial: render::shader_path::celestial_light(0.0),
        shadows: true,
        atmosphere: true,
        scattering: 1.0,
        sun_size_deg: 2.0,
        block_shadows: true,
        block_softness_deg: 0.0,
        sun_factor: 1.0,
        lightmap_debug: 0,
    };
    // Backen, bis nichts mehr ansteht.
    for runde in 0..20 {
        let t0 = std::time::Instant::now();
        sp.voxels_mut().update_lights(&queue, [8, 70, 8]);
        let auftraege = sp.voxels_mut().bake_jobs().len();
        sp.prepare(&queue, &frame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        sp.shadows(&mut encoder, &mut |_| None);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::Maintain::Wait);
        if auftraege > 0 {
            println!("Runde {runde}: {auftraege} Sections in {:?}", t0.elapsed());
        }
    }
    if let Some(e) = pollster::block_on(device.pop_error_scope()) {
        panic!("wgpu-Validierung: {e}");
    }

    let voxels = sp.voxels_mut();
    let n_lagen = render::voxel_volume::LM_LAGEN as u64;
    let faces = render::voxel_volume::LM_FACES as u64;

    // Strahl auf der CPU, exakt Zelle fuer Zelle (Amanatides/Woo), bis er den Block der Quelle
    // betritt -- ohne Rand davor: ein Strahl, der vorher die Ecke eines Nachbarblocks streift,
    // ist verdeckt.
    let frei = |o: [f32; 3], l: (i32, i32, i32)| {
        let ziel = [l.0 as f64 + 0.5, l.1 as f64 + 0.5, l.2 as f64 + 0.5];
        let o = o.map(|v| v as f64);
        let d = [ziel[0] - o[0], ziel[1] - o[1], ziel[2] - o[2]];
        let mut zelle = o.map(|v| v.floor() as i32);
        let schritt = d.map(|v| if v > 0.0 { 1 } else { -1 });
        let mut tmax: [f64; 3] = std::array::from_fn(|k| if d[k] == 0.0 { f64::INFINITY } else { ((zelle[k] + (schritt[k] > 0) as i32) as f64 - o[k]) / d[k] });
        let tdelta: [f64; 3] = std::array::from_fn(|k| if d[k] == 0.0 { f64::INFINITY } else { 1.0 / d[k].abs() });
        for _ in 0..200 {
            if zelle == [l.0, l.1, l.2] {
                return true;
            }
            let b = zelle;
            let innen = (0..16).contains(&b[0]) && (64..80).contains(&b[1]) && (0..32).contains(&b[2]);
            if innen && (fest[idx(b[0], b[1], b[2])] || lichter.iter().any(|&(x, y, z)| [x, y, z] == b)) {
                return false;
            }
            let k = if tmax[0] <= tmax[1] && tmax[0] <= tmax[2] { 0 } else if tmax[1] <= tmax[2] { 1 } else { 2 };
            zelle[k] += schritt[k];
            tmax[k] += tdelta[k];
        }
        true
    };
    let normalen = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
    let rand = |s: f32| {
        let t = s.fract();
        let q = match (s.floor() as i32).rem_euclid(4) {
            0 => [t, 0.0],
            1 => [1.0, t],
            2 => [1.0 - t, 1.0],
            _ => [0.0, 1.0 - t],
        };
        q.map(|v| 0.01 + 0.98 * v)
    };
    let mut fehler = Vec::new();
    let mut geprueft = 0;
    for sz in 0..2usize {
    // Section (0, 4, sz) liegt in Slot (4 * 16 + sz) * 16 + 0.
    let seite = lesen(&device, &queue, voxels.lightmap_buffer(), 1024 + 16 * sz as u64, 1)[0];
    assert!(seite > 0, "Section {sz} ohne Seite");
    let index = lesen(&device, &queue, voxels.lightmap_buffer(), 4096 + (seite as u64 - 1) * 24576, 24576);
    let lagen = lesen(&device, &queue, voxels.lightmap_layer_buffer(), (seite as u64 - 1) * faces * n_lagen * 4, faces * n_lagen * 4);
    // Kein Platzmangel: jede sichtbare Flaeche hat einen Platz (die Unterseiten des Bodens
    // zeigen ins Ungeladene und zaehlen nicht).
    let voll = index.iter().filter(|&&p| p == u32::MAX).count();
    let unten = (0..256).filter(|&b| index[b * 6] != 0).count();
    println!("Section {sz}: {} Flaechen, {voll} ohne Platz, {unten} Unterseiten unten", index.iter().filter(|&&p| p != 0 && p != u32::MAX).count());
    assert_eq!(voll, 0, "Section {sz}: Flaechen ohne Platz");
    assert_eq!(unten, 0, "Section {sz}: Unterseiten zum Ungeladenen haben Plaetze");
    for f in 0..24576usize {
        let platz = index[f];
        if platz == 0 || platz == u32::MAX {
            continue;
        }
        let (i, d) = (f / 6, f % 6);
        let b = [(i % 16) as i32, 64 + (i / 256) as i32, 16 * sz as i32 + ((i / 16) % 16) as i32];
        let n = normalen[d];
        let (ua, va) = if d < 2 { ([1, 0, 0], [0, 0, 1]) } else if d < 4 { ([1, 0, 0], [0, 1, 0]) } else { ([0, 0, 1], [0, 1, 0]) };
        let ebenen = &lagen[(platz as usize - 1) * n_lagen as usize * 4..platz as usize * n_lagen as usize * 4];
        for &l in &lichter {
            if [l.0, l.1, l.2] == b {
                continue;
            }
            // Die Ebene dieser Laterne (Block relativ zur Flaeche).
            let ebene = ebenen.chunks(4).take_while(|e| e[0] >> 27 & 3 != 0).find(|e| {
                let v = [e[0] & 511, e[0] >> 9 & 511, e[0] >> 18 & 511].map(|v| v as i32 - 256);
                [b[0] + v[0], b[1] + v[1], b[2] + v[2]] == [l.0, l.1, l.2]
            });
            // Zwei Linien, die fast aufeinander liegen: dazwischen bliebe eine Naht, im Licht-
            // Pass halb hell (Schattenkappen) oder halb dunkel (Lichtkappen).
            if let Some(e) = ebene.filter(|e| e[0] >> 27 & 3 == 2 && e[0] >> 29 & 1 != 0) {
                let mitte = |daten: u32| {
                    let a = rand((daten & 2047) as f32 / 512.0);
                    let bb = rand((daten >> 11 & 2047) as f32 / 512.0);
                    (a, bb, [(a[0] + bb[0]) / 2.0, (a[1] + bb[1]) / 2.0])
                };
                let (a0, b0, m0) = mitte(e[2]);
                let (a1, b1, m1) = mitte(e[3]);
                let m = [(m0[0] + m1[0]) / 2.0, (m0[1] + m1[1]) / 2.0];
                let abstand = |a: [f32; 2], bb: [f32; 2]| {
                    let r = [bb[0] - a[0], bb[1] - a[1]];
                    ((r[0] * (m[1] - a[1]) - r[1] * (m[0] - a[0])) / (r[0] * r[0] + r[1] * r[1]).sqrt().max(1e-4)).abs()
                };
                if abstand(a0, b0) < 0.03 && abstand(a1, b1) < 0.03 {
                    fehler.push(format!("Block {b:?} Seite {d} Licht {l:?}: Naht zwischen zwei Linien bei {m:?}, Lichtkappen {}", e[0] >> 30 & 1));
                }
            }
            // Eine Linie, auf deren einer Seite fast nichts liegt (beide Enden am selben Rand
            // oder knapp um eine Ecke): im Licht-Pass ein Streifen halber Helligkeit am Rand.
            if let Some(e) = ebene.filter(|e| e[0] >> 27 & 3 == 2) {
                let zahl = if e[0] >> 29 & 1 != 0 { 2 } else { 1 };
                for z in 0..zahl {
                    let sa = (e[2 + z] & 2047) as f32 / 512.0;
                    let sb = (e[2 + z] >> 11 & 2047) as f32 / 512.0;
                    let dicke = |von: f32, bis: f32| {
                        let (a, bb) = (rand(von), rand(bis));
                        let r = [bb[0] - a[0], bb[1] - a[1]];
                        let laenge = (bis - von).rem_euclid(4.0);
                        (1..=4)
                            .filter(|&c| {
                                let ab = (c as f32 - von).rem_euclid(4.0);
                                ab > 0.0 && ab < laenge
                            })
                            .map(|c| {
                                let p = rand((c % 4) as f32);
                                ((r[0] * (p[1] - a[1]) - r[1] * (p[0] - a[0])) / (r[0] * r[0] + r[1] * r[1]).sqrt().max(1e-4)).abs()
                            })
                            .fold(0.0f32, f32::max)
                    };
                    let duenn = dicke(sa, sb).min(dicke(sb, sa));
                    if duenn < 0.02 {
                        fehler.push(format!("Block {b:?} Seite {d} Licht {l:?}: Linie {sa}..{sb} schneidet nur {duenn} ab"));
                    }
                }
            }
            for q in 0..400 {
                let uv = [0.05 + 0.9 * (q % 20) as f32 / 19.0, 0.05 + 0.9 * (q / 20) as f32 / 19.0];
                let p: [f32; 3] = std::array::from_fn(|k| b[k] as f32 + 0.5 + n[k] as f32 * 0.51 + ua[k] as f32 * (uv[0] - 0.5) + va[k] as f32 * (uv[1] - 0.5));
                let zu: [f32; 3] = [l.0 as f32 + 0.5 - p[0], l.1 as f32 + 0.5 - p[1], l.2 as f32 + 0.5 - p[2]];
                let dist = (zu[0] * zu[0] + zu[1] * zu[1] + zu[2] * zu[2]).sqrt();
                if dist >= 14.0 || (0..3).map(|k| zu[k] * n[k] as f32).sum::<f32>() <= 0.05 * dist {
                    continue;
                }
                let wahr = frei(p, l);
                let (vorhersage, knapp) = match ebene {
                    None => (false, false),
                    Some(e) => match e[0] >> 27 & 3 {
                        1 => (true, false),
                        3 => continue,
                        _ => {
                            let zahl = if e[0] >> 29 & 1 != 0 { 2 } else { 1 };
                            let mut kappe = false;
                            let mut knapp = false;
                            for z in 0..zahl {
                                let daten = e[2 + z];
                                let a = rand((daten & 2047) as f32 / 512.0);
                                let bb = rand((daten >> 11 & 2047) as f32 / 512.0);
                                let r = [bb[0] - a[0], bb[1] - a[1]];
                                let abstand = (r[0] * (uv[1] - a[1]) - r[1] * (uv[0] - a[0])) / (r[0] * r[0] + r[1] * r[1]).sqrt().max(1e-4);
                                kappe |= abstand < 0.0;
                                knapp |= abstand.abs() < 0.03;
                            }
                            (if e[0] >> 30 & 1 != 0 { kappe } else { !kappe }, knapp)
                        }
                    },
                };
                if knapp {
                    continue;
                }
                geprueft += 1;
                if vorhersage != wahr {
                    fehler.push(format!("Block {b:?} Seite {d} Licht {l:?} uv {uv:?}: Karte {vorhersage}, Strahl {wahr}, Ebene {:?}", ebene.map(|e| [e[0] >> 27 & 3, e[0] >> 29 & 3])));
                }
            }
        }
    }
    }
    // Himmelssicht (s. back_himmel): 4x4 Werte je Flaeche, Oberseite (Richtung 1).
    let himmel = |x: i32, y: i32, z: i32| -> Vec<f32> {
        let sz = (z / 16) as u64;
        let seite = lesen(&device, &queue, voxels.lightmap_buffer(), 1024 + 16 * sz, 1)[0] as u64;
        let lokal = (((y - 64) * 16 + z % 16) * 16 + x) as u64;
        let platz = lesen(&device, &queue, voxels.lightmap_buffer(), 4096 + (seite - 1) * 24576 + lokal * 6 + 1, 1)[0] as u64;
        assert!(platz > 0 && platz != u32::MAX as u64, "Oberseite von ({x}, {y}, {z}) ohne Platz");
        let basis = 4096 + 768 * 24576 + 768 + 768 * faces + ((seite - 1) * faces + platz - 1) * 4;
        lesen(&device, &queue, voxels.lightmap_buffer(), basis, 4).iter().flat_map(|w| (0..4).map(move |b| (w >> (8 * b) & 255) as f32 / 255.0)).collect()
    };
    let raum = himmel(12, 65, 26);
    let frei = himmel(14, 64, 20);
    let ecke = himmel(1, 64, 19);
    println!("Himmel: im Raum {raum:?}\n  frei {frei:?}\n  an der Saeule {ecke:?}");
    assert!(raum.iter().all(|&v| v < 0.02), "geschlossener Raum sieht Himmel: {raum:?}");
    assert!(frei.iter().all(|&v| v > 0.8), "freier Boden sieht zu wenig Himmel: {frei:?}");

    println!("{geprueft} Punkte geprueft, {} falsch", fehler.len());
    for f in fehler.iter().take(40) {
        println!("  {f}");
    }
    assert!(fehler.is_empty(), "{} Punkte falsch", fehler.len());

    // Das Licht-Raster wandert so, dass sein Rand durch die Sections geht (Ursprung x = 8): die
    // Sections werden nur fuers Blocklicht neu gebacken. Jede Flaeche behaelt ihre
    // Himmelssicht -- ihr Platz darf nicht vom Raster abhaengen.
    let faces = render::voxel_volume::LM_FACES as u64;
    let himmel_je_flaeche = |device: &wgpu::Device, queue: &wgpu::Queue, voxels: &render::voxel_volume::VoxelVolume| {
        let mut alle = std::collections::HashMap::new();
        for sz in 0..2u64 {
            let seite = lesen(device, queue, voxels.lightmap_buffer(), 1024 + 16 * sz, 1)[0] as u64;
            let index = lesen(device, queue, voxels.lightmap_buffer(), 4096 + (seite - 1) * 24576, 24576);
            let werte = lesen(device, queue, voxels.lightmap_buffer(), 4096 + 768 * 24576 + 768 + 768 * faces + (seite - 1) * faces * 4, faces * 4);
            for (f, &platz) in index.iter().enumerate() {
                if platz != 0 && platz != u32::MAX {
                    let p = (platz - 1) as usize * 4;
                    alle.insert((sz, f), werte[p..p + 4].to_vec());
                }
            }
        }
        alle
    };
    let vorher = himmel_je_flaeche(&device, &queue, sp.voxels_mut());
    let mut neu_gebacken = 0;
    let frame = render::shader_path::LightFrame { eye: glam::DVec3::new(76.5, 70.0, 8.5), ..frame };
    for _ in 0..10 {
        sp.voxels_mut().update_lights(&queue, [76, 70, 8]);
        neu_gebacken += sp.voxels_mut().bake_jobs().len();
        sp.prepare(&queue, &frame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        sp.shadows(&mut encoder, &mut |_| None);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::Maintain::Wait);
    }
    assert!(neu_gebacken > 0, "Rand des Rasters nicht neu gebacken");
    let nachher = himmel_je_flaeche(&device, &queue, sp.voxels_mut());
    assert_eq!(vorher.len(), nachher.len(), "andere Flaechen nach dem Wandern");
    let anders = vorher.iter().filter(|(k, v)| nachher.get(k) != Some(v)).count();
    assert_eq!(anders, 0, "{anders} Flaechen mit anderer Himmelssicht nach dem Wandern des Rasters");
}

// Wie die Sicht auf eine Quelle gespeicherter Ebenen an `uv` aussieht -- dieselbe Rechnung wie
// lage_sicht/fein_sicht im Shader (Format s. deferred_light.wgsl, cs_lm_layers). None: je
// Pixel mit Strahlen.
// `tan`: Weichheit (tan des halben Winkels), `p`: die Stelle (Welt), `mitte`: Mitte der Quelle.
// `raster`: die 64 Worte des Sicht-Rasters der Ebene (bei Art 3, Bit 31), sonst egal.
fn gespeicherte_sicht(e: &[u32], raster: &[u32], uv: [f32; 2], tan: f32, p: [f32; 3], mitte: [f32; 3]) -> Option<f32> {
    let d_licht = (0..3).map(|k| (mitte[k] - p[k]).powi(2)).sum::<f32>().sqrt();
    // Rand des Quadrats, bei Bit 31 (Linien) um LM_WEIT = 3 Bloecke je Seite vergroessert.
    let m = if e[0] >> 31 != 0 { 3.0 } else { 0.0 };
    let rand = |s: f32| {
        let t = s.fract();
        let q = match (s.floor() as i32).rem_euclid(4) {
            0 => [t, 0.0],
            1 => [1.0, t],
            2 => [1.0 - t, 1.0],
            _ => [0.0, 1.0 - t],
        };
        q.map(|v| 0.01 - m + (0.98 + 2.0 * m) * v)
    };
    let kante = |q: u32| (q as f32 / 255.0).powi(2) * 64.0;
    let scheibe = |x: f32| {
        let c = x.clamp(-1.0, 1.0);
        (c.acos() - c * (1.0 - c * c).sqrt()) / std::f32::consts::PI
    };
    match e[0] >> 27 & 3 {
        0 => Some(0.0),
        1 => Some(1.0),
        2 => {
            let zahl = if e[0] >> 29 & 1 != 0 { 2 } else { 1 };
            let mut kappe = 0.0;
            for z in 0..zahl {
                let d = e[2 + z];
                let a = rand((d & 2047) as f32 / 512.0);
                let b = rand((d >> 11 & 2047) as f32 / 512.0);
                let r = [b[0] - a[0], b[1] - a[1]];
                let l2 = (r[0] * r[0] + r[1] * r[1]).max(1e-6);
                let t = (((uv[0] - a[0]) * r[0] + (uv[1] - a[1]) * r[1]) / l2).clamp(0.0, 1.0);
                let fein = e[1] >> (20 + 6 * z);
                let (qa, qb) = ((d >> 22 & 31) << 3 | fein & 7, (d >> 27) << 3 | fein >> 3 & 7);
                let k = kante(qa) * (1.0 - t) + kante(qb) * t;
                let halb = if k > 1e-3 { (tan * (d_licht - k).max(0.0) * d_licht / k).max(0.002) } else { 0.002 };
                let abstand = (r[0] * (uv[1] - a[1]) - r[1] * (uv[0] - a[0])) / l2.sqrt().max(1e-4);
                kappe += scheibe(abstand / halb);
            }
            let kappe: f32 = kappe.min(1.0);
            Some(if e[0] >> 30 & 1 != 0 { kappe } else { 1.0 - kappe })
        }
        _ if e[0] >> 31 != 0 => {
            // 16 x 16 Werte zu 8 Bit, Zeile fuer Zeile (s. fein_sicht).
            let n = 16;
            let s = (n - 1) as f32;
            let wert = |i: usize, j: usize| (raster[j * (n / 4) + i / 4] >> (8 * (i % 4)) & 255) as f32 / 255.0;
            let x = [(uv[0] * s).clamp(0.0, s), (uv[1] * s).clamp(0.0, s)];
            let i = [x[0].floor().min(s - 1.0), x[1].floor().min(s - 1.0)];
            let a = [x[0] - i[0], x[1] - i[1]];
            let (c, r) = (i[0] as usize, i[1] as usize);
            let unten = wert(c, r) * (1.0 - a[0]) + wert(c + 1, r) * a[0];
            let oben = wert(c, r + 1) * (1.0 - a[0]) + wert(c + 1, r + 1) * a[0];
            Some(unten * (1.0 - a[1]) + oben * a[1])
        }
        _ => None,
    }
}

// Ein Fernlicht hinter einem Durchgang auf Bodenhoehe: sein Licht streift flach ueber den
// Boden (so im Spiel gesehen -- Zacken, Streifen, Kanten im Lichtstreifen). Fuer jede
// Bodenflaeche wird das Gespeicherte (Linien, Raster) an 10x10 Punkten gegen die weiche Sicht
// aus 1024 Strahlen auf der CPU gehalten, und an den Kanten zwischen zwei Flaechen, wie weit
// beide Seiten auseinanderspringen. Gibt Zahlen aus; schlaegt fehl, wenn es sichtbar falsch ist.
#[test]
fn streifendes_fernlicht() {
    // Kasten um die Quelle (8, 65, 2), Durchgang (8, 65, 3) nach +z, oben und seitlich zu; zwei
    // Saeulen im Lichtstreifen (Kantenschatten neben dem flachen Streifen).
    let mut bloecke = vec![(7, 65, 2), (9, 65, 2), (8, 65, 1), (8, 66, 2), (7, 65, 3), (9, 65, 3), (8, 66, 3), (7, 66, 3), (9, 66, 3), (7, 66, 2), (9, 66, 2)];
    for y in 65..68 {
        bloecke.push((7, y, 9));
        bloecke.push((10, y, 14));
    }
    fernlicht_szene("Durchgang", (8, 65, 2), &bloecke, 10.0);
}

// Wie im Spiel gesehen (Korn im Halbschatten an einer Wand): eine Wand 3 hoch bei z = 20, davor
// eine Reihe Bloecke 2 hoch, die Quelle seitlich davor am Boden.
#[test]
fn schatten_an_der_wand() {
    let mut bloecke = Vec::new();
    for x in 2..14 {
        for y in 65..68 {
            bloecke.push((x, y, 20));
        }
    }
    for x in 6..10 {
        for y in 65..67 {
            bloecke.push((x, y, 17));
        }
    }
    bloecke.push((5, 65, 15));
    fernlicht_szene("Wand", (3, 65, 11), &bloecke, 10.0);
}

// Quelle einen Block ueber dem Boden hinter einer Wand (z = 3) mit einem Schlitz, 1 breit und
// 2 hoch: seine senkrechten Kanten werfen Linien ueber den Boden (so im Spiel gesehen).
#[test]
fn fernlicht_durch_schlitz() {
    let mut bloecke = vec![(8, 65, 1)];
    for x in 0..16 {
        for y in 65..69 {
            if !(x == 8 && (65..67).contains(&y)) {
                bloecke.push((x, y, 3));
            }
        }
    }
    fernlicht_szene("Schlitz, 10 Grad", (8, 66, 1), &bloecke, 10.0);
    fernlicht_szene("Schlitz, 3 Grad", (8, 66, 1), &bloecke, 3.0);
}

// Quelle in einem Raum (x 5..11, z 1..6, Decke y 69), Tuer 1 breit und 2 hoch in der Vorderwand
// (z = 7): das Licht faellt durch die Tuer auf den Boden davor (so im Spiel gesehen, mit ganzen
// Flaechen ohne Licht mitten im Verlauf).
#[test]
fn fernlicht_durch_tuer() {
    let mut bloecke = Vec::new();
    for x in 4..13 {
        for y in 65..70 {
            for z in 0..8 {
                let rand = x == 4 || x == 12 || z == 0 || z == 7 || y == 69;
                let tuer = z == 7 && x == 8 && (65..67).contains(&y);
                if rand && !tuer {
                    bloecke.push((x, y, z));
                }
            }
        }
    }
    fernlicht_szene("Tuer, 10 Grad", (8, 66, 3), &bloecke, 10.0);
    fernlicht_szene("Tuer, 3 Grad", (8, 66, 3), &bloecke, 3.0);
}

fn fernlicht_szene(name: &str, licht: (i32, i32, i32), bloecke: &[(i32, i32, i32)], weich_grad: f32) {
    let mut log = String::new();
    say!(log, "=== {name} ===");
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: features, required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let atlas = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let mut sp = render::shader_path::ShaderPath::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, classes, atlas);
    // Tests: eine ganze Seite je Frame (im Spiel ein Schritt, s. LM_BAKE_PER_FRAME).
    sp.voxels_mut().set_bake_budget(render::voxel_volume::LM_BAKE_PER_FRAME);
    sp.ensure_size(&device, 64, 48);

    // Glowstone dreifach (Stufe 45, s. lichtstaerken_setzen).
    lichtstaerken_setzen();
    let state = |name: &str| (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some(name)).unwrap();
    let (glow, stein) = (state("glowstone"), state("stone"));
    let mut fest = vec![false; 2 * 4096];
    let idx = |x: i32, y: i32, z: i32| ((z / 16) * 4096 + ((y - 64) * 16 + z % 16) * 16 + x) as usize;
    for x in 0..16 {
        for z in 0..32 {
            fest[idx(x, 64, z)] = true;
        }
    }
    for &(x, y, z) in bloecke {
        fest[idx(x, y, z)] = true;
    }
    let mut states = vec![world::STATE_AIR; 2 * 4096];
    for (i, &f) in fest.iter().enumerate() {
        if f {
            states[i] = stein;
        }
    }
    states[idx(licht.0, licht.1, licht.2)] = glow;
    let sections = [world::ChunkSection::from_states(states[..4096].to_vec()), world::ChunkSection::from_states(states[4096..].to_vec())];
    sp.voxels_mut().update(&queue, [0, 4, 0], |sx, sy, sz| (sx == 0 && sy == 4 && (0..2).contains(&sz)).then(|| &sections[sz as usize]), 16);
    sp.voxels_mut().set_world_bottom(64);
    let frame = render::shader_path::LightFrame {
        sky: wgpu::Color::BLUE,
        daylight: 0.0,
        view: glam::Mat4::IDENTITY,
        proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
        eye: glam::DVec3::new(8.5, 70.0, 8.5),
        celestial: render::shader_path::celestial_light(0.0),
        shadows: true,
        atmosphere: true,
        scattering: 1.0,
        sun_size_deg: 2.0,
        block_shadows: true,
        block_softness_deg: weich_grad,
        sun_factor: 1.0,
        lightmap_debug: 0,
    };
    let t0 = std::time::Instant::now();
    for _ in 0..20 {
        sp.voxels_mut().update_lights(&queue, [8, 70, 8]);
        sp.prepare(&queue, &frame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        sp.shadows(&mut encoder, &mut |_| None);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::Maintain::Wait);
    }
    say!(log, "Backen: {:?} fuer 2 Sections, GPU {:?} ms je Back-Schritt (1/16 Seite)", t0.elapsed(), sp.bake_ms_per_section());
    if let Some(e) = pollster::block_on(device.pop_error_scope()) {
        panic!("wgpu-Validierung: {e}");
    }

    // Weiche Sicht auf der CPU: wie light_visibility_n im Shader (Vogel-Spirale ueber die
    // Scheibe, jeder Strahl bis zum Block der Quelle bzw. seiner naechsten Stelle an ihrer
    // Mitte), aber mit 1024 Strahlen und exakter Zellfolge.
    let tan = (weich_grad.to_radians() * 0.5).tan() as f64;
    let mitte = [licht.0 as f64 + 0.5, licht.1 as f64 + 0.5, licht.2 as f64 + 0.5];
    let frei = |o: [f64; 3], d: [f64; 3]| {
        let inv = d.map(|v| 1.0 / if v.abs() < 1e-9 { 1e-9 } else { v });
        let lo = [licht.0 as f64, licht.1 as f64, licht.2 as f64];
        let a: [f64; 3] = std::array::from_fn(|k| (lo[k] - o[k]) * inv[k]);
        let b: [f64; 3] = std::array::from_fn(|k| (lo[k] + 1.0 - o[k]) * inv[k]);
        let rein = (0..3).map(|k| a[k].min(b[k])).fold(f64::MIN, f64::max);
        let raus = (0..3).map(|k| a[k].max(b[k])).fold(f64::MAX, f64::min);
        let ende = if rein <= raus && raus > 0.0 { (rein - 1e-4).max(0.0) } else { (0..3).map(|k| (mitte[k] - o[k]) * d[k]).sum::<f64>().max(0.0) };
        let mut zelle = o.map(|v| v.floor() as i32);
        let schritt = d.map(|v| if v > 0.0 { 1 } else { -1 });
        let mut tmax: [f64; 3] = std::array::from_fn(|k| ((zelle[k] + (schritt[k] > 0) as i32) as f64 - o[k]) * inv[k]);
        let tdelta: [f64; 3] = std::array::from_fn(|k| inv[k].abs());
        let mut t = 0.0;
        while t < ende {
            let innen = (0..16).contains(&zelle[0]) && (64..80).contains(&zelle[1]) && (0..32).contains(&zelle[2]);
            if !innen {
                return true;
            }
            if zelle != [licht.0, licht.1, licht.2] && fest[idx(zelle[0], zelle[1], zelle[2])] {
                return false;
            }
            let k = if tmax[0] <= tmax[1] && tmax[0] <= tmax[2] { 0 } else if tmax[1] <= tmax[2] { 1 } else { 2 };
            t = tmax[k];
            zelle[k] += schritt[k];
            tmax[k] += tdelta[k];
        }
        true
    };
    let weich = |o: [f64; 3]| {
        let zu: [f64; 3] = std::array::from_fn(|k| mitte[k] - o[k]);
        let l = (zu[0] * zu[0] + zu[1] * zu[1] + zu[2] * zu[2]).sqrt();
        let m = zu.map(|v| v / l);
        let hilf = if m[1].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let kreuz = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let norm = |a: [f64; 3]| {
            let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            a.map(|v| v / l)
        };
        let u = norm(kreuz(m, hilf));
        let w = kreuz(m, u);
        let n = 1024;
        let mut summe = 0;
        for s in 0..n {
            let r = ((s as f64 + 0.5) / n as f64).sqrt() * tan;
            let wink = s as f64 * 2.39996;
            let d = norm(std::array::from_fn(|k| m[k] + (u[k] * wink.cos() + w[k] * wink.sin()) * r));
            summe += frei(o, d) as u32;
        }
        summe as f32 / n as f32
    };

    // Ebenen der Quelle je Bodenflaeche (Oberseite von (x, 64, z)).
    let voxels = sp.voxels_mut();
    let n_lagen = render::voxel_volume::LM_LAGEN as u64;
    let faces = render::voxel_volume::LM_FACES as u64;
    let mut ebene = std::collections::HashMap::new();
    let mut raster_von: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::new();
    // Je Art der Ebene der Quelle: wie viele Flaechen aller Richtungen (nicht nur des Bodens).
    let mut alle_arten: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut strahlen_flaechen = Vec::new();
    for sz in 0..2u64 {
        let seite = lesen(&device, &queue, voxels.lightmap_buffer(), 1024 + 16 * sz, 1)[0] as u64;
        let index = lesen(&device, &queue, voxels.lightmap_buffer(), 4096 + (seite - 1) * 24576, 24576);
        let lagen = lesen(&device, &queue, voxels.lightmap_layer_buffer(), (seite - 1) * faces * n_lagen * 4, faces * n_lagen * 4);
        for (f, &platz) in index.iter().enumerate() {
            if platz == 0 || platz == u32::MAX {
                continue;
            }
            let (b, d) = (f / 6, f % 6);
            let (x, y, z) = ((b % 16) as i32, 64 + (b / 256) as i32, 16 * sz as i32 + (b / 16 % 16) as i32);
            let e = &lagen[(platz as usize - 1) * n_lagen as usize * 4..platz as usize * n_lagen as usize * 4];
            let eigene = e.chunks(4).take_while(|e| e[0] >> 27 & 3 != 0).find(|e| {
                let v = [e[0] & 511, e[0] >> 9 & 511, e[0] >> 18 & 511].map(|v| v as i32 - 256);
                [x + v[0], y + v[1], z + v[2]] == [licht.0, licht.1, licht.2]
            });
            let name = match eigene {
                None => "keine",
                Some(e) => match e[0] >> 27 & 3 {
                    1 => "frei",
                    2 => "Linien",
                    _ if e[0] >> 31 != 0 && e[0] >> 30 & 1 != 0 => "Fein",
                    _ if e[0] >> 31 != 0 => "Raster",
                    _ if e[0] >> 30 & 1 != 0 => "Kanten",
                    _ => "Strahlen",
                },
            };
            *alle_arten.entry(name).or_default() += 1;
            if name == "Strahlen" {
                strahlen_flaechen.push((x, y, z, d));
            }
        }
        for x in 0..16i32 {
            for zl in 0..16i32 {
                let platz = index[((zl * 16 + x) * 6 + 1) as usize];
                if platz == 0 || platz == u32::MAX {
                    continue;
                }
                let e = &lagen[(platz as usize - 1) * n_lagen as usize * 4..platz as usize * n_lagen as usize * 4];
                let z = 16 * sz as i32 + zl;
                let eigene = e.chunks(4).take_while(|e| e[0] >> 27 & 3 != 0).find(|e| {
                    let v = [e[0] & 511, e[0] >> 9 & 511, e[0] >> 18 & 511].map(|v| v as i32 - 256);
                    [x + v[0], 64 + v[1], z + v[2]] == [licht.0, licht.1, licht.2]
                });
                let e4 = eigene.map_or([0u32; 4], |e| [e[0], e[1], e[2], e[3]]);
                if e4[0] >> 27 & 3 == 3 && e4[0] >> 31 != 0 && e4[0] >> 30 & 1 != 0 {
                    let fw = render::voxel_volume::LM_FEIN_WORTE as u64;
                    let basis = render::voxel_volume::LM_FEIN_BASE as u64 + ((seite - 1) * render::voxel_volume::LM_FEIN_PLAETZE as u64 + e4[2] as u64) * fw;
                    raster_von.insert((x, z), lesen(&device, &queue, voxels.lightmap_buffer(), basis, fw));
                }
                ebene.insert((x, z), e4);
            }
        }
    }

    // Die Stelle uv der Bodenflaeche (x, z), knapp darueber.
    let stelle = |x: i32, z: i32, uv: [f32; 2]| [x as f32 + uv[0], 65.01, z as f32 + uv[1]];
    let mitte32 = mitte.map(|v| v as f32);
    let art_name = |e: &[u32; 4]| match e[0] >> 27 & 3 {
        0 => "keine",
        1 => "frei",
        2 => "Linien",
        _ if e[0] >> 31 != 0 && e[0] >> 30 & 1 != 0 => "Fein",
        _ if e[0] >> 31 != 0 => "Raster",
        _ if e[0] >> 30 & 1 != 0 => "Kanten",
        _ => "Strahlen",
    };
    let mut je_art: std::collections::BTreeMap<&str, (usize, f32, f32)> = Default::default();
    let mut schlimmste = Vec::new();
    // Eine harte Stufe der Sicht innerhalb eines Punktabstands des Rasters (1/15 Block) um die
    // Stelle: ein Raster zeichnet sie dort weich, das zaehlt fuer sich (s. unten).
    let hart = |px: f64, pz: f64| {
        let w = weich([px, 65.01, pz]);
        let d = 1.0 / 15.0;
        [(d, 0.0), (-d, 0.0), (0.0, d), (0.0, -d)].iter().any(|&(a, b)| (weich([px + a, 65.01, pz + b]) - w).abs() > 0.15)
    };
    let flaechen: Vec<((i32, i32), [u32; 4])> = ebene.iter().filter(|(k, _)| k.1 >= 4).map(|(k, e)| (*k, *e)).collect();
    let ergebnisse = parallel(&flaechen, |&((x, z), e)| {
        let mut max_f = 0.0f32;
        let mut summe = 0.0;
        let mut wo = ([0.0f32; 2], 0.0f32, 0.0f32);
        let mut an_stufe = 0.0f32;
        for q in 0..100 {
            let uv = [0.05 + 0.1 * (q % 10) as f32, 0.05 + 0.1 * (q / 10) as f32];
            let Some(g) = gespeicherte_sicht(&e, raster_von.get(&(x, z)).map_or(&[][..], |r| &r[..]), uv, tan as f32, stelle(x, z, uv), mitte32) else { continue };
            let w = weich([x as f64 + uv[0] as f64, 65.01, z as f64 + uv[1] as f64]);
            let f = (g - w).abs();
            summe += f;
            // Feines Raster an einer harten Stufe: fuer sich gezaehlt.
            if f > 0.1 && art_name(&e) == "Fein" && hart(x as f64 + uv[0] as f64, z as f64 + uv[1] as f64) {
                an_stufe = an_stufe.max(f);
                continue;
            }
            if f > max_f {
                max_f = f;
                wo = (uv, g, w);
            }
        }
        (max_f, summe, wo, an_stufe)
    });
    let an_stufe = ergebnisse.iter().map(|e| e.3).fold(0.0f32, f32::max);
    let ergebnisse: Vec<_> = ergebnisse.into_iter().map(|(a, b, c, _)| (a, b, c)).collect();
    for (&((x, z), e), (max_f, summe, wo)) in flaechen.iter().zip(ergebnisse) {
        let e = &e;
        let eintrag = je_art.entry(art_name(e)).or_default();
        eintrag.0 += 1;
        eintrag.1 += summe / 100.0;
        eintrag.2 = eintrag.2.max(max_f);
        schlimmste.push((max_f, x, z, art_name(e), wo));
    }
    say!(log, "Alle Flaechen je Art: {alle_arten:?}");
    if !strahlen_flaechen.is_empty() {
        say!(log, "  Strahlen je Pixel: {:?}", &strahlen_flaechen[..strahlen_flaechen.len().min(20)]);
    }
    say!(log, "Je Art: Flaechen, mittlerer Fehler, groesster Fehler (Sicht 0..1; Raster an harten Stufen, 1/15 Block weich gezeichnet: bis {an_stufe:.3})");
    for (art, (n, summe, max)) in &je_art {
        say!(log, "  {art:9} {n:4}  {:.3}  {max:.3}", summe / *n as f32);
    }
    schlimmste.sort_by(|a, b| b.0.total_cmp(&a.0));
    // Feines Raster der schlimmsten Flaeche: gespeicherte Punkte gegen die genaue Sicht dort.
    if let Some(&(_, x, z, "Fein", (uv, _, _))) = schlimmste.first() {
        let r = &raster_von[&(x, z)];
        let i0 = ((uv[0] * 15.0).floor() as usize).min(14);
        let j0 = ((uv[1] * 15.0).floor() as usize).min(14);
        for j in j0.saturating_sub(1)..(j0 + 3).min(16) {
            let mut zeile = String::new();
            for i in i0.saturating_sub(2)..(i0 + 3).min(16) {
                let g = (r[j * 4 + i / 4] >> (8 * (i % 4)) & 255) as f32 / 255.0;
                let u = (i as f64 / 15.0).clamp(0.01, 0.99);
                let v = (j as f64 / 15.0).clamp(0.01, 0.99);
                let w = weich([x as f64 + u, 65.01, z as f64 + v]);
                zeile += &format!(" ({i},{j}) {g:.3}/{w:.3}");
            }
            say!(log, "  Fein ({x}, {z}):{zeile}");
        }
    }
    say!(log, "Schlimmste Flaechen:");
    for (f, x, z, art, (uv, g, w)) in schlimmste.iter().take(12) {
        say!(log, "  ({x:2}, {z:2}) {art:9} Fehler {f:.3} bei uv {uv:.2?}: gespeichert {g:.3}, Strahlen {w:.3}");
    }

    // Spruenge an den Kanten: dieselbe Stelle, von beiden Flaechen aus gelesen (Strahlen je
    // Pixel: die genaue Sicht, die Spirale ist dort nur gedreht).
    let lesen_an = |x: i32, z: i32, uv: [f32; 2]| {
        let e = ebene.get(&(x, z)).copied().unwrap_or([0; 4]);
        gespeicherte_sicht(&e, raster_von.get(&(x, z)).map_or(&[][..], |r| &r[..]), uv, tan as f32, stelle(x, z, uv), mitte32).unwrap_or_else(|| weich([x as f64 + uv[0] as f64, 65.01, z as f64 + uv[1] as f64]))
    };
    let mut spruenge = Vec::new();
    let mut alle_kanten = Vec::new();
    for x in 0..16 {
        for z in 4..32 {
            for t in [0.25f32, 0.5, 0.75] {
                if !ebene.contains_key(&(x, z)) {
                    continue;
                }
                // Eine harte Stufe genau am Rand zeichnen beide Flaechen anders weich: fuer sich.
                // Stufen ins Schwarze zaehlen auch dort (s. unten).
                if x + 1 < 16 && ebene.contains_key(&(x + 1, z)) {
                    let (a, b) = (lesen_an(x, z, [0.999, t]), lesen_an(x + 1, z, [0.001, t]));
                    let s = ((a - b).abs(), x, z, "x+", art_name(&ebene[&(x, z)]), art_name(&ebene[&(x + 1, z)]), a.min(b), a.max(b));
                    if !hart(x as f64 + 1.0, z as f64 + t as f64) {
                        spruenge.push(s);
                    }
                    alle_kanten.push(s);
                }
                if z + 1 < 32 && ebene.contains_key(&(x, z + 1)) {
                    let (a, b) = (lesen_an(x, z, [t, 0.999]), lesen_an(x, z + 1, [t, 0.001]));
                    let s = ((a - b).abs(), x, z, "z+", art_name(&ebene[&(x, z)]), art_name(&ebene[&(x, z + 1)]), a.min(b), a.max(b));
                    if !hart(x as f64 + t as f64, z as f64 + 1.0) {
                        spruenge.push(s);
                    }
                    alle_kanten.push(s);
                }
            }
        }
    }
    spruenge.sort_by(|a, b| b.0.total_cmp(&a.0));
    let ueber = |g: f32| spruenge.iter().filter(|s| s.0 > g).count();
    say!(log, "Kanten: {} Stellen, ueber 0,02: {}, ueber 0,05: {}, ueber 0,1: {}", spruenge.len(), ueber(0.02), ueber(0.05), ueber(0.1));
    for (s, x, z, r, a, b, _, _) in spruenge.iter().take(12) {
        say!(log, "  ({x:2}, {z:2}) {r} {a} | {b}: Sprung {s:.3}");
    }
    // Stufen ins Schwarze: auf der einen Seite gar kein Licht, auf der anderen noch etwas. Klein
    // gemessen, im Bild aber deutlich -- eine helle Quelle macht aus 1 % Sicht sichtbares Licht
    // (eine Stufe je Block, wo der Schatten ganz dunkel wird).
    let schwarz: Vec<_> = alle_kanten.iter().filter(|s| s.6 < 0.001 && s.7 > 0.004).collect();
    say!(log, "Stufen ins Schwarze: {}", schwarz.len());
    for (_, x, z, r, a, b, lo, hi) in schwarz.iter().take(6) {
        say!(log, "  ({x:2}, {z:2}) {r} {a} | {b}: {lo:.4} gegen {hi:.4}");
    }
    println!("{log}");
    // Sichtbar falsch: mehr als 10 % daneben oder ein Sprung von mehr als 10 % an einer Kante.
    assert!(schlimmste.first().is_none_or(|s| s.0 < 0.1), "{name}: gespeicherte Sicht bis {:.3} daneben", schlimmste[0].0);
    assert_eq!(ueber(0.1), 0, "{name}: Spruenge ueber 0,1 an Kanten");
    assert!(schwarz.is_empty(), "{name}: {} Stufen ins Schwarze an Kanten", schwarz.len());
    // Den Faktor nicht zuruecksetzen: die andere Szene laeuft vielleicht gerade parallel.
}

// Eine Quelle wird gesetzt, nachdem alles gebacken ist (so im Spiel): auch Flaechen zwei
// Sections weiter, die ihr Fernlicht erreicht, muessen sie danach kennen -- frueher wurden nur
// die Nachbar-Sections der Aenderung neu gebacken, weiter weg blieb es dunkel.
#[test]
fn fernlicht_nachtraeglich_gesetzt() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    let atlas = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let mut sp = render::shader_path::ShaderPath::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, classes, atlas);
    // Tests: eine ganze Seite je Frame (im Spiel ein Schritt, s. LM_BAKE_PER_FRAME).
    sp.voxels_mut().set_bake_budget(render::voxel_volume::LM_BAKE_PER_FRAME);
    sp.ensure_size(&device, 64, 48);
    lichtstaerken_setzen();
    let state = |name: &str| (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some(name)).unwrap();
    let (glow, stein) = (state("glowstone"), state("stone"));
    // Drei Sections hintereinander (z 0..48), Boden bei y = 64.
    let boden = |mit_licht: bool| -> Vec<world::ChunkSection> {
        (0..3)
            .map(|sz| {
                let mut st = vec![world::STATE_AIR; 4096];
                for i in 0..256 {
                    st[i] = stein;
                }
                if mit_licht && sz == 0 {
                    st[(16 + 2) * 16 + 8] = glow;
                }
                world::ChunkSection::from_states(st)
            })
            .collect()
    };
    let frame = render::shader_path::LightFrame {
        sky: wgpu::Color::BLUE,
        daylight: 0.0,
        view: glam::Mat4::IDENTITY,
        proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
        eye: glam::DVec3::new(8.5, 70.0, 24.5),
        celestial: render::shader_path::celestial_light(0.0),
        shadows: true,
        atmosphere: true,
        scattering: 1.0,
        sun_size_deg: 2.0,
        block_shadows: true,
        block_softness_deg: 0.0,
        sun_factor: 1.0,
        lightmap_debug: 0,
    };
    let backen = |sp: &mut render::shader_path::ShaderPath| {
        for _ in 0..30 {
            sp.voxels_mut().update_lights(&queue, [8, 70, 24]);
            sp.prepare(&queue, &frame);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            sp.shadows(&mut encoder, &mut |_| None);
            queue.submit([encoder.finish()]);
            device.poll(wgpu::Maintain::Wait);
        }
    };
    let ohne = boden(false);
    sp.voxels_mut().update(&queue, [0, 4, 1], |sx, sy, sz| (sx == 0 && sy == 4 && (0..3).contains(&sz)).then(|| &ohne[sz as usize]), 64);
    sp.voxels_mut().set_world_bottom(64);
    backen(&mut sp);
    // Jetzt die Quelle (Stufe 45) in Section 0, wie ein Block, den man setzt.
    let mit = boden(true);
    sp.voxels_mut().note_changes(false, &[(0, 0)]);
    sp.voxels_mut().update(&queue, [0, 4, 1], |sx, sy, sz| (sx == 0 && sy == 4 && (0..3).contains(&sz)).then(|| &mit[sz as usize]), 64);
    backen(&mut sp);
    // Oberseite von (8, 64, 40): Section 2, 38 Bloecke von der Quelle.
    let voxels = sp.voxels_mut();
    let seite = lesen(&device, &queue, voxels.lightmap_buffer(), 1024 + 16 * 2, 1)[0] as u64;
    assert!(seite > 0, "Section 2 ohne Seite");
    let platz = lesen(&device, &queue, voxels.lightmap_buffer(), 4096 + (seite - 1) * 24576 + ((8 * 16 + 8) * 6 + 1), 1)[0] as u64;
    assert!(platz > 0 && platz != u32::MAX as u64, "Flaeche ohne Platz");
    let n = render::voxel_volume::LM_LAGEN as u64;
    let ebenen = lesen(&device, &queue, voxels.lightmap_layer_buffer(), ((seite - 1) * render::voxel_volume::LM_FACES as u64 + platz - 1) * n * 4, n * 4);
    println!("Ebenen der fernen Flaeche: {:?}", ebenen.chunks(4).take_while(|e| e[0] >> 27 & 3 != 0).map(|e| e[0] >> 27 & 3).collect::<Vec<_>>());
    assert!(ebenen[0] >> 27 & 3 != 0, "Flaeche 38 Bloecke weiter kennt die neue Quelle nicht");
}

// Zurueckgeworfenes Licht (s. deferred_light.wgsl, back_rueckwurf) in einem geschlossenen Raum
// mit einer Trennwand: die Lampe steht in der einen Haelfte, die andere bekommt nur Licht, das
// von Boden, Waenden und Decke zurueckkommt. Alle Bloecke grau (Albedo ~0,5). Gegen zwei
// Rechnungen auf der CPU: dasselbe Modell (zwei Rueckwuerfe, weitere als geometrische Reihe)
// mit viel mehr Strahlen -- prueft die GPU -- und echte Pfadverfolgung ueber alle Rueckwuerfe
// -- zeigt, wie gut das Modell ist.
#[test]
fn rueckwurf_im_geschlossenen_raum() {
    // Raum x 0..15, y 64..69, z 0..15: Boden, Decke, Waende; Trennwand bei z = 7 von x = 0
    // bis 10 (Durchgang x 11..14). Klein genug, dass das Licht (15 Bloecke) hinten ankommt.
    let fest = |x: i32, y: i32, z: i32| (y == 64 || y == 69 || x == 0 || x == 15 || z == 0 || z >= 15) && y <= 69 || z == 7 && x <= 10 && y <= 69;
    let proben: Vec<(&str, i32, i32, i32, u64)> = vec![
        ("Boden neben Lampe", 6, 64, 3, 1),
        ("Boden hinter Trennwand", 5, 64, 8, 1),
        ("Boden hinter Trennwand", 5, 64, 11, 1),
        ("Boden hinten", 3, 64, 13, 1),
        ("Boden am Durchgang", 12, 64, 8, 1),
        ("Wand hinter Trennwand", 15, 66, 11, 4),
    ];
    let m = rueckwurf_szene("Geschlossener Raum", fest, &[((3, 66, 3), "sea_lantern")], &proben, true);
    assert!(m.fehler_max < 0.15, "GPU weicht bis {:.0} % vom Modell ab", m.fehler_max * 100.0);
    assert!(m.sprung_max < 0.02, "Spruenge an Flaechenraendern bis {:.0} %", m.sprung_max * 100.0);
    assert!(m.fleck_mittel < 0.02, "Flecken im Mittel {:.1} % des Lichts", m.fleck_mittel * 100.0);
}

// Offener Boden mit ein paar Bloecken und drei farbigen Lampen, wie im Spiel gesehen ("Flecken
// auf dem Boden"): wie weit der Rueckwurf je Bodenstelle vom Modell mit vielen Strahlen
// abweicht, gemessen am ganzen Licht dort (direkt + zurueckgeworfen) -- so viel davon ist als
// Fleck zu sehen.
#[test]
fn rueckwurf_offener_boden() {
    // Dazu zwei Bloecke Ecke an Ecke ((8, 65, 14) und (9, 65, 15)): die Nische dahinter bei
    // (8, 64, 15) sieht das Licht der Lampe dahinter nicht -- auch nicht ueber die Ecke.
    let bloecke = [(4, 65, 6), (5, 65, 9), (10, 65, 12), (10, 66, 12), (10, 67, 12), (10, 68, 12), (3, 65, 20), (12, 65, 24), (8, 65, 14), (9, 65, 15)];
    let fest = move |x: i32, y: i32, z: i32| y == 64 || bloecke.contains(&(x, y, z));
    let lichter = [((5, 65, 7), "verdant_froglight"), ((13, 65, 18), "pearlescent_froglight"), ((7, 65, 26), "sea_lantern"), ((11, 65, 12), "sea_lantern")];
    let nische = [("Nische hinter der Ecke", 8, 64, 15, 1), ("Boden jenseits der Ecke", 9, 64, 14, 1)];
    let m = rueckwurf_szene("Offener Boden", fest, &lichter, &nische, true);
    // Das Modell selbst rauscht mit 1024 Strahlen um ein paar Prozent.
    assert!(m.fleck_max < 0.05, "Flecken bis {:.1} % des Lichts", m.fleck_max * 100.0);
    assert!(m.fleck_mittel < 0.01, "Flecken im Mittel {:.1} % des Lichts", m.fleck_mittel * 100.0);
    assert!(m.sprung_max < 0.02, "Spruenge an Flaechenraendern bis {:.0} %", m.sprung_max * 100.0);
}

struct RueckMessung {
    fehler_max: f64,
    sprung_max: f64,
    fleck_max: f64,
    fleck_mittel: f64,
}

// Szene in Section (0, 4, 0..2) bauen (x 0..15, y 64..79, z 0..31; `fest` sagt, wo ein grauer
// Block steht), backen und den Rueckwurf messen: an den `proben` (Name, Block, Seite) gegen
// Modell und -- mit `pfade` -- Pfadverfolgung; auf allen Bodenflaechen (Oberseite y = 64) die
// Flecken; die Spruenge zwischen Nachbarflaechen.
fn rueckwurf_szene(name: &str, fest_an: impl Fn(i32, i32, i32) -> bool, lichter: &[((i32, i32, i32), &str)], proben: &[(&str, i32, i32, i32, u64)], pfade_an: bool) -> RueckMessung {
    let mut log = String::new();
    say!(log, "=== {name} ===");
    let mut messung = RueckMessung { fehler_max: 0.0, sprung_max: 0.0, fleck_max: 0.0, fleck_mittel: 0.0 };
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return messung;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    // Atlas aus einer grauen Kachel (sRGB 188): jede Seite jedes Blocks hat diese Farbe.
    let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture { texture: &atlas_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &[188u8; 16 * 16 * 4],
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(64), rows_per_image: Some(16) },
        wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
    );
    let albedo = ((188.0f64 / 255.0 + 0.055) / 1.055).powf(2.4);
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let mut sp = render::shader_path::ShaderPath::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, classes, atlas_tex.create_view(&wgpu::TextureViewDescriptor::default()));
    // Tests: eine ganze Seite je Frame (im Spiel ein Schritt, s. LM_BAKE_PER_FRAME).
    sp.voxels_mut().set_bake_budget(render::voxel_volume::LM_BAKE_PER_FRAME);
    sp.ensure_size(&device, 64, 48);
    lichtstaerken_setzen();

    let state = |name: &str| (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some(name)).unwrap();
    let stein = state("stone");
    // Je Lampe: Block, Mitte, Farbe (Stufen je Kanal), Reichweite.
    let lampen: Vec<([i32; 3], [f64; 3], [f64; 3], f64)> = lichter
        .iter()
        .map(|&((x, y, z), n)| {
            let st = state(n);
            let e = world::light::emission(st);
            let farbe = [e & 15, e >> 4 & 15, e >> 8 & 15].map(|v| v as f64);
            let weit = farbe.iter().cloned().fold(0.0, f64::max).max(world::light::emission_level(st) as f64);
            ([x, y, z], [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5], farbe, weit)
        })
        .collect();
    let idx = |x: i32, y: i32, z: i32| ((z / 16) * 4096 + ((y - 64) * 16 + z % 16) * 16 + x) as usize;
    let mut fest = vec![false; 2 * 4096];
    let mut states = vec![world::STATE_AIR; 2 * 4096];
    for x in 0..16 {
        for y in 64..80 {
            for z in 0..32 {
                if fest_an(x, y, z) {
                    fest[idx(x, y, z)] = true;
                    states[idx(x, y, z)] = stein;
                }
            }
        }
    }
    for &((x, y, z), n) in lichter {
        fest[idx(x, y, z)] = true;
        states[idx(x, y, z)] = state(n);
    }
    let sections = [world::ChunkSection::from_states(states[..4096].to_vec()), world::ChunkSection::from_states(states[4096..].to_vec())];
    sp.voxels_mut().update(&queue, [0, 4, 0], |sx, sy, sz| (sx == 0 && sy == 4 && (0..2).contains(&sz)).then(|| &sections[sz as usize]), 16);
    sp.voxels_mut().set_world_bottom(64);
    let frame = render::shader_path::LightFrame {
        sky: wgpu::Color::BLUE,
        daylight: 0.0,
        view: glam::Mat4::IDENTITY,
        proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
        eye: glam::DVec3::new(8.5, 66.0, 8.5),
        celestial: render::shader_path::celestial_light(0.0),
        shadows: true,
        atmosphere: true,
        scattering: 1.0,
        sun_size_deg: 2.0,
        block_shadows: true,
        block_softness_deg: 0.0,
        sun_factor: 1.0,
        lightmap_debug: 0,
    };
    let t0 = std::time::Instant::now();
    for _ in 0..20 {
        sp.voxels_mut().update_lights(&queue, [8, 66, 8]);
        sp.prepare(&queue, &frame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        sp.shadows(&mut encoder, &mut |_| None);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::Maintain::Wait);
    }
    say!(log, "Backen: {:?} fuer 2 Sections", t0.elapsed());
    if let Some(e) = pollster::block_on(device.pop_error_scope()) {
        panic!("wgpu-Validierung: {e}");
    }

    // --- CPU: Strahlen durch die Bloecke, direktes Licht wie light_strength (ohne Fernlicht).
    let besetzt = |c: [i32; 3]| (0..16).contains(&c[0]) && (64..80).contains(&c[1]) && (0..32).contains(&c[2]) && fest[idx(c[0], c[1], c[2])];
    // Erster besetzter Block auf dem Strahl bis `weit`: (Abstand, Zelle, Normale der Seite).
    let treffer = |o: [f64; 3], d: [f64; 3], weit: f64| -> Option<(f64, [i32; 3], [f64; 3])> {
        let inv = d.map(|v| 1.0 / if v.abs() < 1e-12 { 1e-12 } else { v });
        let mut zelle = o.map(|v| v.floor() as i32);
        let schritt = d.map(|v| if v > 0.0 { 1 } else { -1 });
        let mut tmax: [f64; 3] = std::array::from_fn(|k| ((zelle[k] + (schritt[k] > 0) as i32) as f64 - o[k]) * inv[k]);
        let tdelta: [f64; 3] = std::array::from_fn(|k| inv[k].abs());
        let mut t = 0.0;
        let mut n = [0.0; 3];
        while t < weit {
            if besetzt(zelle) {
                return Some((t, zelle, n));
            }
            let k = if tmax[0] <= tmax[1] && tmax[0] <= tmax[2] { 0 } else if tmax[1] <= tmax[2] { 1 } else { 2 };
            t = tmax[k];
            zelle[k] += schritt[k];
            tmax[k] += tdelta[k];
            n = [0.0; 3];
            n[k] = -schritt[k] as f64;
        }
        None
    };
    let kurve = |l: f64| l / (4.0 - 3.0 * l);
    let direkt = |o: [f64; 3], n: [f64; 3]| -> [f64; 3] {
        let mut summe = [0.0; 3];
        for &(block, mitte, farbe, weit) in &lampen {
            let zu: [f64; 3] = std::array::from_fn(|k| mitte[k] - o[k]);
            let ab = (zu[0] * zu[0] + zu[1] * zu[1] + zu[2] * zu[2]).sqrt();
            let ndl = (0..3).map(|k| n[k] * zu[k] / ab).sum::<f64>();
            if ab >= weit || ndl <= 0.0 {
                continue;
            }
            // Frei bis zum Block der Quelle.
            if let Some((_, z, _)) = treffer(o, zu.map(|v| v / ab), ab) {
                if z != block {
                    continue;
                }
            }
            for k in 0..3 {
                summe[k] += kurve(((farbe[k] - ab).max(0.0)) / 15.0) * ndl;
            }
        }
        summe
    };
    let basis = |n: [f64; 3]| {
        let hilf = if n[1].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let kreuz = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let u = kreuz(n, hilf);
        let l = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
        let u = u.map(|v| v / l);
        (u, kreuz(n, u))
    };
    let richtung = |n: [f64; 3], a: f64, b: f64| {
        let (u, w) = basis(n);
        let r = a.sqrt();
        let wink = b * std::f64::consts::TAU;
        let z = (1.0 - a).max(0.0).sqrt();
        std::array::from_fn::<f64, 3, _>(|k| u[k] * r * wink.cos() + w[k] * r * wink.sin() + n[k] * z)
    };
    let ist_lampe = |z: [i32; 3]| lampen.iter().any(|l| l.0 == z);
    let halton = |i: u32, b: u32| {
        let (mut f, mut r, mut i) = (1.0, 0.0, i);
        while i > 0 {
            f /= b as f64;
            r += f * (i % b) as f64;
            i /= b;
        }
        r
    };
    // Dasselbe Modell wie der Shader, mit `zahl` Strahlen (Halton) und allen Quellen je Treffer:
    // fuer Punkt (i, j) der Flaeche `d` des Blocks `b`, gemittelt ueber dieselbe Gegend
    // (s. back_rueckwurf).
    let modell = |b: [i32; 3], d: u64, i: usize, j: usize, zahl: u32| -> [f64; 3] {
        let (n, ua, va): ([i32; 3], [i32; 3], [i32; 3]) = match d {
            1 => ([0, 1, 0], [1, 0, 0], [0, 0, 1]),
            0 => ([0, -1, 0], [1, 0, 0], [0, 0, 1]),
            _ => ([-1, 0, 0], [0, 0, 1], [0, 1, 0]),
        };
        let nf = n.map(|v| v as f64);
        let weiter = |su: i32, sv: i32| {
            let nb: [i32; 3] = std::array::from_fn(|k| b[k] + ua[k] * su + va[k] * sv);
            let davor: [i32; 3] = std::array::from_fn(|k| nb[k] + n[k]);
            besetzt(nb) && !ist_lampe(nb) && !besetzt(davor)
        };
        let uv = [i as f64 / 3.0, j as f64 / 3.0];
        let mut erster = [0.0; 3];
        let mut zweiter = [0.0; 3];
        let mut bleibt = 0.0;
        let mut gezaehlt = 0;
        for s in 0..zahl {
            // Unabhaengig von den Richtungen (Halton in hohen Basen haengt bei wenigen tausend
            // Punkten zusammen und verzerrt die Gegend).
            let zufall = |salz: u64| {
                let mut h = (s as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15) ^ salz.wrapping_mul(0xD1B54A32D192ED03);
                h ^= h >> 31;
                h = h.wrapping_mul(0xBF58476D1CE4E5B9);
                h ^= h >> 29;
                (h >> 11) as f64 / (1u64 << 53) as f64
            };
            let q = [uv[0] + (zufall(1) + zufall(2) - 1.0) / 3.0, uv[1] + (zufall(3) + zufall(4) - 1.0) / 3.0];
            let zq = q.map(|v| (v.floor() as i32).clamp(-1, 1));
            if zq != [0, 0] && !weiter(zq[0], zq[1]) {
                continue;
            }
            if zq[0] != 0 && zq[1] != 0 && !weiter(zq[0], 0) && !weiter(0, zq[1]) {
                continue;
            }
            gezaehlt += 1;
            let p: [f64; 2] = std::array::from_fn(|k| zq[k] as f64 + (q[k] - zq[k] as f64).clamp(0.01, 0.99));
            let o: [f64; 3] = std::array::from_fn(|k| b[k] as f64 + 0.5 + nf[k] * 0.51 + ua[k] as f64 * (p[0] - 0.5) + va[k] as f64 * (p[1] - 0.5));
            let d = richtung(nf, halton(s + 1, 2), halton(s + 1, 3));
            let Some((t, z, nh)) = treffer(o, d, 16.0) else { continue };
            if t < 1e-3 {
                continue;
            }
            bleibt += albedo;
            if ist_lampe(z) {
                continue;
            }
            let p: [f64; 3] = std::array::from_fn(|k| o[k] + d[k] * t + nh[k] * 0.01);
            let e = direkt(p, nh);
            for k in 0..3 {
                erster[k] += albedo * e[k];
            }
            let d2 = richtung(nh, halton(s + 1, 5), halton(s + 1, 7));
            let Some((t2, z2, nh2)) = treffer(p, d2, 16.0) else { continue };
            if t2 < 1e-3 || ist_lampe(z2) {
                continue;
            }
            let p2: [f64; 3] = std::array::from_fn(|k| p[k] + d2[k] * t2 + nh2[k] * 0.01);
            let e2 = direkt(p2, nh2);
            for k in 0..3 {
                zweiter[k] += albedo * albedo * e2[k];
            }
        }
        let zahl = gezaehlt.max(1) as f64;
        let anteil = (bleibt / zahl).min(0.75);
        std::array::from_fn(|k| (erster[k] + zweiter[k] / (1.0 - anteil)) / zahl)
    };
    // Pfadverfolgung: alle Rueckwuerfe, je Punkt 4096 Pfade bis 8 Rueckwuerfe.
    let mut zufall = 0x9E3779B97F4A7C15u64;
    let mut rnd = move || {
        zufall ^= zufall << 13;
        zufall ^= zufall >> 7;
        zufall ^= zufall << 17;
        (zufall >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut pfade = |o0: [f64; 3], n0: [f64; 3]| -> [f64; 3] {
        let zahl = 4096;
        let mut summe = [0.0; 3];
        for _ in 0..zahl {
            let (mut o, mut n, mut gewicht) = (o0, n0, 1.0);
            for _ in 0..8 {
                let d = richtung(n, rnd(), rnd());
                let Some((t, z, nh)) = treffer(o, d, 64.0) else { break };
                if t < 1e-3 || ist_lampe(z) {
                    break;
                }
                gewicht *= albedo;
                let p: [f64; 3] = std::array::from_fn(|k| o[k] + d[k] * t + nh[k] * 0.01);
                let e = direkt(p, nh);
                for k in 0..3 {
                    summe[k] += gewicht * e[k];
                }
                o = p;
                n = nh;
            }
        }
        summe.map(|v| v / zahl as f64)
    };

    // --- GPU-Werte lesen: die 16 Punkte der Flaeche `d` des Blocks (x, y, z).
    let voxels = sp.voxels_mut();
    let faces = render::voxel_volume::LM_FACES as u64;
    let gpu = |x: i32, y: i32, z: i32, d: u64| -> Option<Vec<[f64; 3]>> {
        let seite = lesen(&device, &queue, voxels.lightmap_buffer(), 1024 + 16 * (z / 16) as u64, 1)[0] as u64;
        let lokal = (((y & 15) * 16 + (z & 15)) * 16 + (x & 15)) as u64;
        let platz = lesen(&device, &queue, voxels.lightmap_buffer(), 4096 + (seite - 1) * 24576 + lokal * 6 + d, 1)[0] as u64;
        if platz == 0 || platz == u32::MAX as u64 {
            return None;
        }
        let w = lesen(&device, &queue, voxels.lightmap_buffer(), render::voxel_volume::LM_RUECK_BASE as u64 + ((seite - 1) * faces + platz - 1) * 16, 16);
        Some(w.iter().map(|&w| [w, w >> 9, w >> 18].map(|m| (m & 511) as f64 * 2f64.powi((w >> 27) as i32 - 24))).collect())
    };
    let hell = |c: [f64; 3]| (c[0] + c[1] + c[2]) / 3.0;
    // Punkt (i, j) der Flaeche wie back_punkt: Lage knapp davor und Normale (Oberseite: u = x,
    // v = z; Unterseite ebenso; Westseite: u = z, v = y).
    let punkt = |x: i32, y: i32, z: i32, d: u64, i: usize, j: usize| {
        let uv = [i as f64 / 3.0, j as f64 / 3.0].map(|v| v.clamp(0.01, 0.99));
        match d {
            1 => ([x as f64 + uv[0], y as f64 + 1.01, z as f64 + uv[1]], [0.0, 1.0, 0.0]),
            0 => ([x as f64 + uv[0], y as f64 - 0.01, z as f64 + uv[1]], [0.0, -1.0, 0.0]),
            _ => ([x as f64 - 0.01, y as f64 + uv[1], z as f64 + uv[0]], [-1.0, 0.0, 0.0]),
        }
    };
    if !proben.is_empty() {
        say!(log, "Rueckwurf (Einheit von light_strength), Albedo {albedo:.3}:");
    }
    for &(pname, x, y, z, d) in proben {
        let Some(werte) = gpu(x, y, z, d) else {
            say!(log, "  {pname} ({x}, {y}, {z}): keine Karte");
            continue;
        };
        for (i, j) in [(0usize, 0usize), (2, 1), (3, 3), (3, 0)] {
            let (o, n) = punkt(x, y, z, d, i, j);
            let g = werte[j * 4 + i];
            let m = modell([x, y, z], d, i, j, 4096);
            let p = if pfade_an { hell(pfade(o, n)) } else { f64::NAN };
            // Gemessen an der Helligkeit dort, wenigstens 0,02 (gut ein Zehntel des Rueckwurfs
            // neben einer Lampe): im Dunkeln zaehlt das Rauschen nicht.
            let fehler = (hell(g) - hell(m)).abs() / hell(m).max(0.02);
            messung.fehler_max = messung.fehler_max.max(fehler);
            say!(log, "  {pname:24} ({x:2},{y},{z:2}) uv ({i},{j}): GPU {:.4}  Modell {:.4}  Pfade {p:.4}  GPU/Modell {:+.0} %  Modell/Pfade {:+.0} %", hell(g), hell(m), (hell(g) / hell(m).max(1e-9) - 1.0) * 100.0, (hell(m) / p.max(1e-9) - 1.0) * 100.0);
        }
    }
    // Flecken auf dem Boden: an jedem Punkt jeder zweiten Bodenflaeche die Abweichung vom
    // Modell (8192 Strahlen -- nahe an Waenden rauscht es mit weniger selbst um 10 %), gemessen
    // am ganzen Licht dort.
    let mut stellen = Vec::new();
    for x in 0..16 {
        for z in (0..32).step_by(2) {
            let Some(werte) = gpu(x, 64, z, 1) else { continue };
            for (i, j) in [(1usize, 1usize), (2, 3)] {
                stellen.push((x, z, i, j, hell(werte[j * 4 + i])));
            }
        }
    }
    let mut flecken: Vec<_> = parallel(&stellen, |&(x, z, i, j, g)| {
        let (o, n) = punkt(x, 64, z, 1, i, j);
        let m = hell(modell([x, 64, z], 1, i, j, 8192));
        let ganz = hell(direkt(o, n)) + m;
        // Wenigstens an 0,02 gemessen, wie oben: auf fast Schwarz sieht man keinen Fleck.
        (ganz >= 0.01).then(|| ((g - m).abs() / ganz.max(0.02), x, z, i, j, g, m, ganz))
    })
    .into_iter()
    .flatten()
    .collect();
    flecken.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mittel = flecken.iter().map(|f| f.0).sum::<f64>() / flecken.len().max(1) as f64;
    say!(log, "Flecken auf dem Boden: {} Punkte, mittel {:.2} %, ueber 2 %: {}, ueber 5 %: {}", flecken.len(), mittel * 100.0, flecken.iter().filter(|f| f.0 > 0.02).count(), flecken.iter().filter(|f| f.0 > 0.05).count());
    for &(f, x, z, i, j, g, m, ganz) in flecken.iter().take(8) {
        say!(log, "  ({x:2}, {z:2}) ({i},{j}): {:.1} % -- GPU {g:.4}, Modell {m:.4}, ganzes Licht {ganz:.4}", f * 100.0);
    }
    messung.fleck_max = flecken.first().map_or(0.0, |f| f.0);
    messung.fleck_mittel = mittel;
    // Spruenge zwischen Nachbarflaechen am selben Rand, gemessen an der Helligkeit dort
    // (wenigstens 0,02, wie oben).
    let mut spruenge = Vec::new();
    for x in 0..15 {
        for z in 0..32 {
            let (Some(a), Some(b)) = (gpu(x, 64, z, 1), gpu(x + 1, 64, z, 1)) else { continue };
            for j in 0..4 {
                let (hl, hr) = (hell(a[j * 4 + 3]), hell(b[j * 4]));
                spruenge.push(((hl - hr).abs() / hl.max(hr).max(0.02), x, z, j, hl, hr));
            }
        }
    }
    spruenge.sort_by(|a, b| b.0.total_cmp(&a.0));
    messung.sprung_max = spruenge.first().map_or(0.0, |s| s.0);
    say!(log, "Groesster Sprung an Flaechenraendern: {:.1} %", messung.sprung_max * 100.0);
    for &(s, x, z, j, hl, hr) in spruenge.iter().take(6) {
        say!(log, "  ({x:2}|{:2}, {z:2}) j {j}: {:.1} % -- links {hl:.4}, rechts {hr:.4}, Modell {:.4}", x + 1, s * 100.0, hell(modell([x, 64, z], 1, 3, j, 1024)));
    }
    println!("{log}");
    messung
}

// Im Spiel gesehen: vom Chunk weglaufen und zurueck -- dann rauschte das Licht dort (Fernlicht
// aus dem Strahl-Pass statt aus der Lichtkarte), bis ein gesetzter Block neu backen liess. Eine
// Section am Rand des Licht-Rasters bekommt fuer ihre Flaechen draussen die Marke LM_AUSSEN;
// wandert das Raster so, dass sie ganz hineinfaellt, muss sie neu gebacken werden.
#[test]
fn lichtkarten_nach_wandern_des_rasters() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    let atlas = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let mut sp = render::shader_path::ShaderPath::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, classes, atlas);
    // Tests: eine ganze Seite je Frame (im Spiel ein Schritt, s. LM_BAKE_PER_FRAME).
    sp.voxels_mut().set_bake_budget(render::voxel_volume::LM_BAKE_PER_FRAME);
    sp.ensure_size(&device, 64, 48);
    lichtstaerken_setzen();
    let state = |name: &str| (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some(name)).unwrap();
    let mut states = vec![world::STATE_AIR; 4096];
    for i in 0..256 {
        states[i] = state("stone");
    }
    states[(16 + 8) * 16 + 4] = state("glowstone");
    let section = world::ChunkSection::from_states(states);
    sp.voxels_mut().update(&queue, [0, 4, 0], |sx, sy, sz| (sx == 0 && sy == 4 && sz == 0).then_some(&section), 16);
    sp.voxels_mut().set_world_bottom(64);
    let backen = |sp: &mut render::shader_path::ShaderPath, x: i32| {
        let frame = render::shader_path::LightFrame {
            sky: wgpu::Color::BLUE,
            daylight: 0.0,
            view: glam::Mat4::IDENTITY,
            proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
            eye: glam::DVec3::new(x as f64 + 0.5, 70.0, 8.5),
            celestial: render::shader_path::celestial_light(0.0),
            shadows: true,
            atmosphere: true,
            scattering: 1.0,
            sun_size_deg: 2.0,
            block_shadows: true,
            block_softness_deg: 6.0,
            sun_factor: 1.0,
            lightmap_debug: 0,
        };
        for _ in 0..10 {
            sp.voxels_mut().update_lights(&queue, [x, 70, 8]);
            sp.prepare(&queue, &frame);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            sp.shadows(&mut encoder, &mut |_| None);
            queue.submit([encoder.finish()]);
            device.poll(wgpu::Maintain::Wait);
        }
    };
    // Raster ab x = 8: die Section (x 0..16) halb drin, Bodenflaeche bei x = 2 draussen.
    backen(&mut sp, 72);
    assert_eq!(ebenen_der_flaeche(&device, &queue, sp.voxels_mut(), 2, 8)[0], u32::MAX, "Flaeche ausserhalb des Rasters ohne Marke");
    // Zurueck: das Raster faengt bei x = -56 an, die Section liegt ganz darin.
    backen(&mut sp, 8);
    let ebenen = ebenen_der_flaeche(&device, &queue, sp.voxels_mut(), 2, 8);
    assert_ne!(ebenen[0], u32::MAX, "nach dem Zuruecklaufen noch als ausserhalb markiert");
    assert_ne!(art(&ebenen), 0, "nach dem Zuruecklaufen keine Ebene der Quelle");
}

// Was kostet es auf der CPU, wenn man in einen neuen Chunk laeuft? Ein Wuerfel aus Sections
// voller Bloecke mit Quellen darin: Sections schreiben (VoxelVolume::update) und das
// Licht-Raster nach einem Schritt ueber eine Zellgrenze neu bauen (update_lights).
#[test]
fn kosten_beim_chunkwechsel() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let mut v = render::voxel_volume::VoxelVolume::new(&device, classes);
    lichtstaerken_setzen();
    let state = |name: &str| (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some(name)).unwrap();
    // Halb Stein, halb Luft, alle 7 Bloecke eine Fackel.
    let mut states = vec![world::STATE_AIR; 4096];
    for (i, s) in states.iter_mut().enumerate() {
        if i < 2048 {
            *s = state("stone");
        } else if i % 7 == 0 && (i / 256) % 2 == 0 {
            *s = state("torch");
        }
    }
    let section = world::ChunkSection::from_states(states);
    v.set_lightmaps(&queue, Some(0.05));
    let t = std::time::Instant::now();
    v.update(&queue, [0, 4, 0], |_, sy, _| (2..7).contains(&sy).then_some(&section), 4096);
    println!("Alle Sections schreiben: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    v.update_lights(&queue, [8, 70, 8]);
    println!("Licht-Raster bauen, Seiten verteilen: {:?}", t.elapsed());
    // Im Spiel baut sich das Raster im Hintergrund: der Frame selbst wartet nicht darauf.
    v.set_async_lights(true);
    for x in [16, 24, 32] {
        let t = std::time::Instant::now();
        v.update(&queue, [x / 16, 4, 0], |_, sy, _| (2..7).contains(&sy).then_some(&section), 8);
        let schreiben = t.elapsed();
        let t = std::time::Instant::now();
        v.update_lights(&queue, [x, 70, 8]);
        let im_frame = t.elapsed();
        let t = std::time::Instant::now();
        let ziel = [x, 70, 8].map(|e: i32| e.div_euclid(8) * 8 - 64);
        while v.light_origin() != Some(ziel) {
            assert!(t.elapsed() < std::time::Duration::from_secs(10), "Licht-Raster nach 10 s nicht fertig");
            v.update_lights(&queue, [x, 70, 8]);
        }
        println!("Schritt nach x = {x}: 8 Sections schreiben {schreiben:?}, Licht-Raster im Frame {im_frame:?}, im Hintergrund fertig nach {:?}", t.elapsed());
        assert!(im_frame < std::time::Duration::from_millis(3), "Licht-Raster haelt den Frame auf: {im_frame:?}");
    }
    device.poll(wgpu::Maintain::Wait);
}

// Benchmark: in die Welt laden. Ein Dorf auf 4 x 4 Chunks -- Boden, Haeuser mit Tuer und Fenstern,
// in jedem eine Seelenlaterne (normal hell), auf dem Platz eine Laterne mit 300 % (Fernlicht, Stufe
// 45). Alles wird zum ersten Mal gebacken, mit demselben Zeitbudget je Frame wie im Spiel (s.
// ShaderPath, BakeTimer). Gemessen: Frames, bis nichts mehr ansteht, und die Zeit je Frame (mit
// Warten auf die GPU, also vor allem das Backen). Laeuft nur ausdruecklich:
//   cargo test --release -p app --test shader_path_gpu benchmark_welt_laden -- --ignored --nocapture
#[test]
#[ignore]
fn benchmark_welt_laden() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    let features = if std::env::var("OHNE_ZEITSTEMPEL").is_ok() { wgpu::Features::empty() } else { adapter.features() & wgpu::Features::TIMESTAMP_QUERY };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: features, required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    let atlas = device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
    let mut sp = render::shader_path::ShaderPath::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, classes, atlas);
    sp.ensure_size(&device, 64, 48);
    lichtstaerken_setzen();
    let state = |name: &str| (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some(name)).unwrap();
    let (stein, bretter, glas, laterne, seele) = (state("stone"), state("oak_planks"), state("glass"), state("lantern"), state("soul_lantern"));

    // Welt: Sections x 0..4, z 0..4; y 48..63 Stein, y 64 Boden, darauf das Dorf.
    let mut welt = std::collections::HashMap::<(i32, i32, i32), Vec<u32>>::new();
    let mut setzen = |x: i32, y: i32, z: i32, st: u32| {
        let s = welt.entry((x >> 4, y >> 4, z >> 4)).or_insert_with(|| vec![world::STATE_AIR; 4096]);
        s[(((y & 15) * 16 + (z & 15)) * 16 + (x & 15)) as usize] = st;
    };
    for x in 0..64 {
        for z in 0..64 {
            for y in 48..=64 {
                setzen(x, y, z, stein);
            }
        }
    }
    // Haeuser 7 x 7, 5 hoch, Dach, Tuer nach Sueden, je Wand ein Fenster.
    let mut haeuser = 0;
    for hx in [4, 20, 40, 52] {
        for hz in [4, 22, 44] {
            if (hx, hz) == (20, 22) {
                continue;
            }
            haeuser += 1;
            for x in hx..hx + 7 {
                for z in hz..hz + 7 {
                    for y in 65..=69 {
                        let wand = x == hx || x == hx + 6 || z == hz || z == hz + 6;
                        let dach = y == 69;
                        if !(wand || dach) {
                            continue;
                        }
                        let tuer = z == hz + 6 && x == hx + 3 && y <= 66;
                        let fenster = y == 67 && ((z == hz || z == hz + 6) && x == hx + 1 || (x == hx || x == hx + 6) && z == hz + 3);
                        if tuer {
                            continue;
                        }
                        setzen(x, y, z, if fenster { glas } else { bretter });
                    }
                }
            }
            setzen(hx + 3, 68, hz + 3, seele);
        }
    }
    // Platz: ein Pfosten mit der hellen Laterne darauf.
    for y in 65..=67 {
        setzen(31, y, 31, bretter);
    }
    setzen(31, 68, 31, laterne);
    let sections: std::collections::HashMap<(i32, i32, i32), world::ChunkSection> = welt.into_iter().map(|(k, v)| (k, world::ChunkSection::from_states(v))).collect();
    let anzahl_sections = sections.len();
    sp.voxels_mut().update(&queue, [2, 4, 2], |sx, sy, sz| sections.get(&(sx, sy, sz)), 4096);
    sp.voxels_mut().set_world_bottom(48);
    sp.voxels_mut().set_async_lights(true);

    let frame = render::shader_path::LightFrame {
        sky: wgpu::Color::BLUE,
        daylight: 0.0,
        view: glam::Mat4::IDENTITY,
        proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
        eye: glam::DVec3::new(31.5, 66.0, 34.5),
        celestial: render::shader_path::celestial_light(0.0),
        shadows: true,
        atmosphere: true,
        scattering: 1.0,
        sun_size_deg: 2.0,
        block_shadows: true,
        block_softness_deg: 6.0,
        sun_factor: 1.0,
        lightmap_debug: 0,
    };
    let start = std::time::Instant::now();
    let mut zeiten = Vec::new();
    let mut cpu_zeiten: Vec<f64> = Vec::new();
    let mut schritte = 0;
    let mut ruhig = 0;
    let mut frames = 0;
    while ruhig < 30 && frames < 20000 {
        frames += 1;
        let t = std::time::Instant::now();
        sp.voxels_mut().update_lights(&queue, [31, 66, 34]);
        let auftraege = sp.voxels_mut().bake_jobs().len();
        sp.prepare(&queue, &frame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        sp.shadows(&mut encoder, &mut |_| None);
        queue.submit([encoder.finish()]);
        let cpu = t.elapsed().as_secs_f64() * 1000.0;
        device.poll(wgpu::Maintain::Wait);
        if auftraege > 0 {
            schritte += auftraege;
            cpu_zeiten.push(cpu);
            zeiten.push(t.elapsed().as_secs_f64() * 1000.0);
            ruhig = 0;
        } else {
            ruhig += 1;
        }
    }
    let gesamt = start.elapsed();
    zeiten.sort_by(f64::total_cmp);
    let summe: f64 = zeiten.iter().sum();
    let perzentil = |p: f64| zeiten.get(((zeiten.len() as f64 - 1.0) * p) as usize).copied().unwrap_or(0.0);
    println!("=== Welt laden: Dorf, {haeuser} Haeuser, {anzahl_sections} Sections mit Bloecken ===");
    println!("Back-Schritte: {schritte} (je Seite {}), in {} Frames mit Backen", render::voxel_volume::LM_SCHRITTE, zeiten.len());
    println!("Zeit bis fertig: {gesamt:?} (davon in Frames mit Backen {summe:.0} ms)");
    println!("Frame mit Backen: Median {:.2} ms, 99 % {:.2} ms, hoechstens {:.2} ms", perzentil(0.5), perzentil(0.99), perzentil(1.0));
    cpu_zeiten.sort_by(f64::total_cmp);
    println!("davon CPU bis zum Abschicken: Median {:.2} ms, hoechstens {:.2} ms", cpu_zeiten.get(cpu_zeiten.len() / 2).copied().unwrap_or(0.0), cpu_zeiten.last().copied().unwrap_or(0.0));
    println!("GPU je Back-Schritt (gemessen): {:?} ms", sp.bake_ms_per_section());
    // Feine Raster je Seite (Zaehler, s. LM_FEIN_ZAEHLER_BASE).
    let zaehler = lesen(&device, &queue, sp.voxels_mut().lightmap_buffer(), render::voxel_volume::LM_FEIN_ZAEHLER_BASE as u64, 768);
    let mut je_seite: Vec<u32> = zaehler.into_iter().filter(|&z| z > 0).collect();
    je_seite.sort_unstable();
    println!("Feine Raster: {} Seiten mit welchen, zusammen {}, je Seite {:?}", je_seite.len(), je_seite.iter().sum::<u32>(), je_seite);
    assert!(frames < 20000, "nach 20000 Frames nicht fertig");
}

// Im Spiel gesehen: einen Chunk neu laden -- danach ein Strich im Licht, bis ein gesetzter Block
// neu backen liess. Zwei Sections nebeneinander (x 0..32), ein Raum ueber beide mit zwei
// Lampen; die rechte wird entladen und wieder geladen, wie im Spiel (Licht-Raster im
// Hintergrund, ein Schritt je Frame). Danach muss jede Flaeche dieselbe Lichtkarte haben wie nach
// einem frischen Backen.
#[test]
fn chunk_neu_laden() {
    // Nah (ganz in einem Frame) und drei Sections weg (in Schritten).
    for auge in [[16, 67, 8], [16, 67, 56]] {
        chunk_neu_laden_von(auge);
    }
}

fn chunk_neu_laden_von(auge: [i32; 3]) {
    println!("=== Auge {auge:?} ===");
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: render::voxel_volume::VoxelVolume::required_limits(&adapter), ..Default::default() }, None)).unwrap();
    let neu = || {
        // Graue Kachel: ohne Farbe wirft nichts Licht zurueck.
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &atlas_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &[188u8; 16 * 16 * 4],
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(64), rows_per_image: Some(16) },
            wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
        );
        let atlas = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let classes = world::voxel::VoxelClasses::build(&world::BlockShapes::all_full_cubes(), &world::CrossBlocks::none(), &world::BlockUvs::new(Vec::new(), [[0.0; 4]; 6]));
        let mut sp = render::shader_path::ShaderPath::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, classes, atlas);
        sp.ensure_size(&device, 64, 48);
        sp.voxels_mut().set_world_bottom(64);
        sp
    };
    lichtstaerken_setzen();
    let state = |name: &str| (0..world::state_count() as u32).find(|&st| world::block_name(st) == Some(name)).unwrap();
    let stein = state("stone");
    let mut states = vec![vec![world::STATE_AIR; 4096], vec![world::STATE_AIR; 4096]];
    for x in 0..32i32 {
        for y in 64..74i32 {
            for z in 0..16i32 {
                let wand = y == 64 || y == 73 || x == 0 || x == 31 || z == 0 || z == 15;
                // Ein Pfeiler mitten im Raum fuer Schatten ueber die Grenze.
                let pfeiler = (15..17).contains(&x) && z == 6 && y < 69;
                if wand || pfeiler {
                    states[(x / 16) as usize][(((y - 64) * 16 + z) * 16 + x % 16) as usize] = stein;
                }
            }
        }
    }
    states[0][((2 * 16 + 9) * 16 + 12) as usize] = state("lantern");
    states[1][((3 * 16 + 4) * 16 + 5) as usize] = state("glowstone");
    let sections: Vec<world::ChunkSection> = states.into_iter().map(world::ChunkSection::from_states).collect();
    let licht = |auge: [i32; 3]| render::shader_path::LightFrame {
        sky: wgpu::Color::BLUE,
        daylight: 0.0,
        view: glam::Mat4::IDENTITY,
        proj: glam::Mat4::perspective_rh(1.2, 4.0 / 3.0, 0.05, 1000.0),
        eye: glam::DVec3::new(auge[0] as f64 + 0.5, auge[1] as f64, auge[2] as f64 + 0.5),
        celestial: render::shader_path::celestial_light(0.0),
        shadows: true,
        atmosphere: true,
        scattering: 1.0,
        sun_size_deg: 2.0,
        block_shadows: true,
        block_softness_deg: 6.0,
        sun_factor: 1.0,
        lightmap_debug: 0,
    };
    let geladen = |rechts: bool| move |sx: i32, sy: i32, sz: i32| -> Option<usize> { (sy == 4 && sz == 0 && (sx == 0 || (sx == 1 && rechts))).then_some(sx as usize) };
    let frames_bei = |sp: &mut render::shader_path::ShaderPath, rechts: bool, n: usize, auge: [i32; 3]| {
        let frame = licht(auge);
        for _ in 0..n {
            let welche = geladen(rechts);
            sp.voxels_mut().update(&queue, auge.map(|e: i32| e.div_euclid(16)), |sx, sy, sz| welche(sx, sy, sz).map(|i| &sections[i]), 96);
            sp.voxels_mut().update_lights(&queue, auge);
            sp.prepare(&queue, &frame);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            sp.shadows(&mut encoder, &mut |_| None);
            queue.submit([encoder.finish()]);
            device.poll(wgpu::Maintain::Wait);
        }
    };
    let frames = |sp: &mut render::shader_path::ShaderPath, rechts: bool, n: usize| frames_bei(sp, rechts, n, auge);
    // Je Flaeche (Block, Richtung) der beiden Sections: Ebenen, Himmel, Raster, Rueckwurf.
    let lesen_alles = |sp: &mut render::shader_path::ShaderPath| -> std::collections::BTreeMap<(i32, i32, i32, u64), Vec<u32>> {
        use render::voxel_volume as vv;
        let v = sp.voxels_mut();
        let mut alle = std::collections::BTreeMap::new();
        for sx in 0..2u64 {
            let seite = lesen(&device, &queue, v.lightmap_buffer(), 1024 + sx, 1)[0] as u64;
            if seite == 0 {
                continue;
            }
            let p = seite - 1;
            let tafel = lesen(&device, &queue, v.lightmap_buffer(), 4096 + p * 24576, 24576);
            let f = vv::LM_FACES as u64;
            let lagen = lesen(&device, &queue, v.lightmap_layer_buffer(), p * f * vv::LM_LAGEN as u64 * 4, f * vv::LM_LAGEN as u64 * 4);
            let himmel = lesen(&device, &queue, v.lightmap_buffer(), vv::LM_HIMMEL_BASE as u64 + p * f * 4, f * 4);
            let raster = vec![0u32; f as usize * 32];
            let rueck = lesen(&device, &queue, v.lightmap_buffer(), vv::LM_RUECK_BASE as u64 + p * f * 16, f * 16);
            let fein_n = (vv::LM_FEIN_PLAETZE * vv::LM_FEIN_WORTE) as u64;
            let fein = lesen(&device, &queue, v.lightmap_buffer(), vv::LM_FEIN_BASE as u64 + p * fein_n, fein_n);
            for (k, &platz) in tafel.iter().enumerate() {
                let (lokal, d) = (k / 6, k as u64 % 6);
                let block = ((lokal % 16) as i32 + sx as i32 * 16, (lokal / 256) as i32 + 64, ((lokal / 16) % 16) as i32);
                let mut w = vec![platz.min(1)];
                if platz != 0 && platz != u32::MAX {
                    let q = platz as usize - 1;
                    let l = vv::LM_LAGEN * 4;
                    let mut e = lagen[q * l..q * l + l].to_vec();
                    // Platz im Vorrat der feinen Raster: die Reihenfolge vergeben Atomics.
                    for lage in e.chunks_mut(4) {
                        if (lage[0] >> 27 & 3) == 3 && (lage[2] as usize) < vv::LM_FEIN_PLAETZE {
                            lage[2] = 0;
                        }
                    }
                    // Nur die Raster, die eine Ebene benutzt (Art 3, Bit 31, nicht fein; z = Platz):
                    // die anderen Plaetze behalten alte Worte, sie werden nicht geleert.
                    let mut r = vec![0u32; 32];
                    for lage in lagen[q * l..q * l + l].chunks(4) {
                        if (lage[0] >> 27 & 3) == 3 && lage[0] >> 31 != 0 && (lage[0] >> 30 & 1) == 0 && lage[2] < 2 {
                            let z = lage[2] as usize;
                            r[z * 16..z * 16 + 16].copy_from_slice(&raster[q * 32 + z * 16..q * 32 + z * 16 + 16]);
                        }
                    }
                    // Die feinen Raster, die eine Ebene benutzt (Bit 30), angehaengt.
                    let mut feine = Vec::new();
                    for lage in lagen[q * l..q * l + l].chunks(4) {
                        if (lage[0] >> 27 & 3) == 3 && lage[0] >> 31 != 0 && (lage[0] >> 30 & 1) != 0 && (lage[2] as usize) < vv::LM_FEIN_PLAETZE {
                            let z = lage[2] as usize * vv::LM_FEIN_WORTE;
                            feine.extend(&fein[z..z + vv::LM_FEIN_WORTE]);
                        }
                    }
                    w.extend(&e);
                    w.extend(&himmel[q * 4..q * 4 + 4]);
                    w.extend(&r);
                    w.extend(&rueck[q * 16..q * 16 + 16]);
                    w.extend(&feine);
                } else {
                    w[0] = platz;
                }
                if platz != 0 {
                    alle.insert((block.0, block.1, block.2, d), w);
                }
            }
        }
        alle
    };

    let mut referenz = neu();
    referenz.voxels_mut().set_bake_budget(render::voxel_volume::LM_BAKE_PER_FRAME);
    frames(&mut referenz, true, 20);
    let soll = lesen_alles(&mut referenz);
    drop(referenz);

    // Wie im Spiel: Licht-Raster im Hintergrund, ein Schritt je Frame.
    let mut sp = neu();
    sp.voxels_mut().set_async_lights(true);
    frames(&mut sp, true, 200);
    let vorher = lesen_alles(&mut sp);
    sp.voxels_mut().note_changes(false, &[(1, 0)]);
    frames(&mut sp, false, 200);
    sp.voxels_mut().note_changes(false, &[(1, 0)]);
    frames(&mut sp, true, 200);
    let ist = lesen_alles(&mut sp);
    // Weit weglaufen (aus dem Voxel-Wuerfel und dem Licht-Raster) und zurueck, in Schritten.
    for x in (16..=400).step_by(4).chain((16..=400).rev().step_by(4)) {
        frames_bei(&mut sp, true, 2, [x, auge[1], auge[2]]);
    }
    frames(&mut sp, true, 200);
    let zurueck = lesen_alles(&mut sp);
    // Das Licht-Raster (128 Bloecke um die Kamera) schneidet die Szene: Section 1 liegt schon
    // darin, die Laterne in Section 0 noch nicht -- ihr Rueckwurf blieb zu dunkel, bis ein Block
    // gesetzt wurde (im Spiel ein Strich an der Grenze). Dann zur Szene. Nur von nah: drei
    // Sections weg reicht das Raster nie ueber den ganzen Rueckwurf (s. rueckwurf_ganz), dort
    // backt auch frisch nicht alles.
    let raster_schnitt = (auge[2] < 16).then(|| {
        let mut sp2 = neu();
        sp2.voxels_mut().set_async_lights(true);
        frames_bei(&mut sp2, true, 200, [80, auge[1], auge[2]]);
        frames(&mut sp2, true, 200);
        lesen_alles(&mut sp2)
    });
    // Wie im Spiel: der Server entlaedt die Chunks hinter der Sichtweite und schickt sie auf dem
    // Rueckweg erst nach und nach -- die Sections stehen eine Weile leer im Wuerfel, eine wird
    // vor der anderen geladen.
    let mut verzoegert = Vec::new();
    for (zuerst, abstand) in [(0usize, 3usize), (1, 3), (0, 20), (1, 20)] {
        let mut geladen_jetzt = [true, true];
        let mut warte = [0usize; 2];
        let schritt = |sp: &mut render::shader_path::ShaderPath, x: i32, geladen_jetzt: &mut [bool; 2], warte: &mut [usize; 2]| {
            let cx = x.div_euclid(16);
            for c in 0..2usize {
                let nah = (c as i32 - cx).abs() <= 4;
                if !nah && geladen_jetzt[c] {
                    geladen_jetzt[c] = false;
                    sp.voxels_mut().note_changes(false, &[(c as i32, 0)]);
                }
                if nah && !geladen_jetzt[c] {
                    warte[c] += 1;
                    let faellig = if c == zuerst { abstand } else { 2 * abstand };
                    if warte[c] >= faellig {
                        geladen_jetzt[c] = true;
                        warte[c] = 0;
                        sp.voxels_mut().note_changes(false, &[(c as i32, 0)]);
                    }
                }
            }
            let welche = *geladen_jetzt;
            let frame = licht([x, auge[1], auge[2]]);
            let a = [x, auge[1], auge[2]];
            sp.voxels_mut().update(&queue, a.map(|e: i32| e.div_euclid(16)), |sx, sy, sz| (sy == 4 && sz == 0 && (0..2).contains(&sx) && welche[sx as usize]).then(|| &sections[sx as usize]), 96);
            sp.voxels_mut().update_lights(&queue, a);
            sp.prepare(&queue, &frame);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            sp.shadows(&mut encoder, &mut |_| None);
            queue.submit([encoder.finish()]);
            device.poll(wgpu::Maintain::Wait);
        };
        for x in (16..=400).step_by(4).chain((16..=400).rev().step_by(4)) {
            for _ in 0..2 {
                schritt(&mut sp, x, &mut geladen_jetzt, &mut warte);
            }
        }
        for _ in 0..200 {
            schritt(&mut sp, auge[0], &mut geladen_jetzt, &mut warte);
        }
        verzoegert.push((format!("Zurueck, Section {zuerst} zuerst, {abstand} Frames Abstand"), lesen_alles(&mut sp)));
    }

    let vergleich = |name: &str, ist: &std::collections::BTreeMap<(i32, i32, i32, u64), Vec<u32>>| {
        let mut falsch = Vec::new();
        // Rueckwurf auf 1 % gleich: das Raster legt die Quellen je nach Bauweise (ganz oder
        // verschoben) in anderer Reihenfolge ab, die Summen runden dann im letzten Bit anders.
        let farbe = |w: u32| [w & 511, w >> 9 & 511, w >> 18 & 511].map(|m| m as f64 * 2f64.powi((w >> 27) as i32 - 24));
        let gleich = |v: &Vec<u32>, w: &Vec<u32>| {
            v.len() == w.len()
                && (0..v.len()).all(|j| {
                    if !(101..117).contains(&j) || v[0] != 1 {
                        return v[j] == w[j];
                    }
                    let (a, b) = (farbe(v[j]), farbe(w[j]));
                    (0..3).all(|c| (a[c] - b[c]).abs() <= 0.01 * a[c].max(b[c]) + 1e-4)
                })
        };
        for (k, w) in &soll {
            match ist.get(k) {
                Some(v) if gleich(v, w) => {}
                Some(v) => {
                    let teile = [("Ebenen", 1, 65), ("Himmel", 65, 69), ("Raster", 69, 101), ("Rueckwurf", 101, 117), ("Fein", 117, usize::MAX)];
                    let welche: Vec<&str> = teile.iter().filter(|(_, a, b)| v.get(*a..(*b).min(v.len())) != w.get(*a..(*b).min(w.len()))).map(|t| t.0).collect();
                    falsch.push(format!("{k:?} {} {welche:?}", if v[0] != w[0] { format!("Platz {} statt {}", v[0], w[0]) } else { String::new() }));
                    if welche.contains(&"Rueckwurf") {
                        let hell = |w: &[u32]| w[101..117].iter().map(|&w| ((w & 511) + (w >> 9 & 511) + (w >> 18 & 511)) as f64 * 2f64.powi((w >> 27) as i32 - 24) / 3.0).sum::<f64>() / 16.0;
                        falsch.push(format!("    Rueckwurf {:.3} statt {:.3}", hell(v), hell(w)));
                    }
                    let diff: Vec<String> = (1..65).filter(|&j| v[j] != w[j]).map(|j| format!("{}: {:08x} statt {:08x}", j - 1, v[j], w[j])).collect();
                    if !diff.is_empty() {
                        falsch.push(format!("    {}", diff.join(", ")));
                    }
                }
                None => falsch.push(format!("{k:?} fehlt")),
            }
        }
        for k in ist.keys().filter(|k| !soll.contains_key(k)) {
            falsch.push(format!("{k:?} zu viel"));
        }
        let fehlen = falsch.iter().filter(|f| f.ends_with("fehlt")).count();
        falsch.sort_by_key(|f| f.ends_with("fehlt"));
        println!("{name}: {} von {} Flaechen anders ({fehlen} fehlen)", falsch.len(), soll.len());
        for f in falsch.iter().take(30) {
            println!("  {f}");
        }
        falsch.len()
    };
    let a = vergleich("Erstes Laden", &vorher);
    let b = vergleich("Nach Neuladen", &ist);
    let c = vergleich("Nach Weglaufen und zurueck", &zurueck);
    let e = raster_schnitt.map_or(0, |r| vergleich("Licht-Raster schnitt die Szene", &r));
    let d: usize = e + verzoegert.iter().map(|(name, ist)| vergleich(name, ist)).sum::<usize>();
    assert!(a == 0 && b == 0 && c == 0 && d == 0, "Lichtkarten weichen ab");
}
