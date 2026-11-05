// GPU-Skinning auf der echten GPU: dieselben Modelle einmal ueber den CPU-Weg (emit_into, die
// Entity-Pipeline) und einmal als feste Netze mit Teil-Matrizen (vs_entity_skinned) gezeichnet.
// Die Bilder muessen bis auf Rundung gleich sein: Ecken, UVs (gemusterte Textur), Licht je
// Seite, versteckte Teile, mehrere Instanzen eines Netzes und mehrere Netze. Ohne GPU nichts.
use glam::DVec3;
use render::skinning::{SkinInstance, SkinMatrix};
use render::{BlockRenderer, Camera};
use world::entity_model::{self as em, Affine, Model, PartPose, Pose};

const W: u32 = 128;

struct Szene {
    model: Model,
    pose: Pose,
    base: Affine,
    two_sided: bool,
}

fn szenen() -> Vec<Szene> {
    let spieler = em::player_model(false);
    let laufen = em::biped_pose(0.5, -0.2, 1.3, 0.9, false, 4.0);
    let mut ohne_kopf = em::biped_pose(-0.4, 0.3, 0.4, 0.6, true, 9.0);
    ohne_kopf.set("head", PartPose { hidden: true, ..ohne_kopf.get("head") });
    vec![
        Szene { model: spieler.clone(), pose: laufen, base: em::entity_transform([-0.9, -1.2, 4.0], 35.0, 1.0, em::PLAYER_MODEL_SCALE), two_sided: true },
        Szene { model: spieler, pose: ohne_kopf, base: em::entity_transform([0.9, -1.2, 4.5], -60.0, 1.0, em::PLAYER_MODEL_SCALE), two_sided: true },
        Szene { model: em::pig_model(), pose: Pose::new(), base: em::entity_transform([0.0, -1.0, 2.6], 150.0, 1.0, 1.0), two_sided: false },
    ]
}

const RECT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const FARBE: [f32; 3] = [0.9, 0.9, 0.9];

fn rendern(device: &wgpu::Device, queue: &wgpu::Queue, r: &BlockRenderer) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let ausdehnung = wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 };
    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: ausdehnung,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = ziel.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(r.depth_attachment()),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        r.render(&mut pass, &mut |_, _| {});
    }
    let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(W * W * 4), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    encoder.copy_texture_to_buffer(ziel.as_image_copy(), wgpu::ImageCopyBuffer { buffer: &lese, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(W) } }, ausdehnung);
    queue.submit(Some(encoder.finish()));
    lese.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let bild = lese.slice(..).get_mapped_range().to_vec();
    bild
}

#[test]
fn skinning_zeichnet_wie_der_cpu_weg() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- nichts geprueft");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let weiss = vec![255u8; 16 * 16 * 4];
    // Gemusterter Entity-Atlas: jede Zelle eine andere Farbe, damit falsche UVs auffallen.
    let mut muster = vec![255u8; 16 * 16 * 4];
    for y in 0..16 {
        for x in 0..16 {
            let i = (y * 16 + x) * 4;
            muster[i] = (x * 16) as u8;
            muster[i + 1] = (y * 16) as u8;
            muster[i + 2] = ((x + y) * 8) as u8;
        }
    }
    let mut r = BlockRenderer::new(&device, &queue, format, W, W, std::slice::from_ref(&weiss), 16, &muster, 16, &weiss, 16);
    let camera = Camera::new(DVec3::ZERO);
    r.update_camera(&queue, &camera, 1.0);

    // CPU-Weg.
    let mut ecken = Vec::new();
    for s in szenen() {
        let mut teil = vec![bytemuck::Zeroable::zeroed(); em::vertex_count(&s.model, &s.pose, s.two_sided)];
        em::emit_into(&s.model, &s.pose, s.base, RECT, FARBE, s.two_sided, &mut teil);
        ecken.extend(teil);
    }
    let tints = vec![[0.0f32; 4]; ecken.len()];
    r.set_entity_vertices(&queue, &ecken, &tints, &[], (0, 0, 0));
    let cpu = rendern(&device, &queue, &r);

    // Skinning: jedes Modell ein Netz, der zweite Spieler als zweite Instanz desselben Netzes.
    r.set_entity_vertices(&queue, &[], &[], &[], (0, 0, 0));
    let spieler = r.skinning.add_mesh(&device, &queue, &em::skin_mesh(&em::player_model(false), true).vertices);
    let schwein = r.skinning.add_mesh(&device, &queue, &em::skin_mesh(&em::pig_model(), false).vertices);
    let mut matrizen = Vec::new();
    let mut instanzen = Vec::new();
    for (s, netz) in szenen().iter().zip([spieler, spieler, schwein]) {
        let start = matrizen.len() as u32;
        SkinMatrix::extend_from_parts(&mut matrizen, &em::part_matrices(&s.model, &s.pose, s.base));
        instanzen.push((netz, SkinInstance { rect: RECT, color: FARBE, matrix: start, tint: [0.0; 4] }));
    }
    r.skinning.set_instances(&device, &queue, &instanzen, &matrizen);
    let gpu = rendern(&device, &queue, &r);

    // Motion Blur mit Vormatrizen (alles ein Stueck verschoben): der Bewegungs-Durchgang der
    // Instanzen muss laufen -- wgpu bricht bei falschen Bindungen oder Pipelines ab.
    let vorher: Vec<SkinMatrix> = matrizen.iter().map(|m| { let mut m = *m; m.0[0][3] -= 0.2; m }).collect();
    r.skinning.set_prev_matrices(&queue, &vorher, true);
    assert!(r.has_entity_motion() && r.entities_moving());
    r.keep_depth = true;
    let mut mb = render::motion_blur::MotionBlur::new(&device, format);
    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = ziel.create_view(&Default::default());
    let mut verwischt = false;
    for _ in 0..2 {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(r.depth_attachment()),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            r.render(&mut pass, &mut |_, _| {});
        }
        verwischt |= mb.encode(&device, &queue, &mut encoder, &ziel, &view, &r, &camera, 1.0, 1.0, 1.0, 1.0 / 60.0, &mut || None);
        queue.submit(Some(encoder.finish()));
        device.poll(wgpu::Maintain::Wait);
    }
    assert!(verwischt, "Instanzen nie verwischt");

    // Leucht-Umriss: der erste Spieler als Silhouette in Rot ins Umriss-Bild. Im Zielbild muss
    // ein roter Rand erscheinen (die Form prueft entity_outline_gpu).
    let (netz, mut umriss) = instanzen[0];
    umriss.tint = [1.0, 0.0, 0.0, 1.0];
    r.skinning.set_outline_instances(&device, &queue, &[(netz, umriss)]);
    assert!(r.has_outlines());
    let mut outline = render::entity_outline::EntityOutline::new(&device, render::entity_outline::FORMAT);
    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render::entity_outline::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    outline.encode(&device, &queue, &mut encoder, &r, &ziel.create_view(&Default::default()), (W, W));
    let ausdehnung = wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 };
    let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(W * W * 4), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    encoder.copy_texture_to_buffer(ziel.as_image_copy(), wgpu::ImageCopyBuffer { buffer: &lese, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(W) } }, ausdehnung);
    queue.submit(Some(encoder.finish()));
    lese.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let sil = lese.slice(..).get_mapped_range().to_vec();
    let rot = sil.chunks(4).filter(|p| p[0] > 100 && p[1] < 40 && p[2] < 40).count();
    eprintln!("Umriss: {rot} rote Pixel im Bild");
    assert!(rot > 20, "Umriss der Instanz fehlt: {rot} rote Pixel");

    let belegt = |b: &[u8]| b.chunks(4).filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0).count();
    let (n_cpu, n_gpu) = (belegt(&cpu), belegt(&gpu));
    assert!(n_cpu > (W * W / 20) as usize, "CPU-Bild fast leer: {n_cpu} Pixel");
    // Kanten duerfen durch Rundung um einen Pixel wandern, sonst muss es gleich sein.
    let abweichend = cpu.chunks(4).zip(gpu.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 3)).count();
    eprintln!("belegt CPU {n_cpu}, Skinning {n_gpu}, abweichend {abweichend}");
    assert!(abweichend * 200 <= n_cpu, "{abweichend} von {n_cpu} Pixeln weichen ab (Skinning {n_gpu} Pixel belegt)");
}

// Glint verzauberter Ruestung: Glint-Zwillinge aus den Ecken (render::glint::glint_vertices)
// gegen die Glint-Instanz (vs_entity_skinned_glint). Bis auf Rundung gleich.
#[test]
fn glint_der_instanz_wie_glint_der_ecken() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- nichts geprueft");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let weiss = vec![255u8; 16 * 16 * 4];
    let mut muster = vec![255u8; 16 * 16 * 4];
    for (i, p) in muster.chunks_mut(4).enumerate() {
        p[0] = (i % 16 * 12) as u8;
        p[1] = (i / 16 * 12) as u8;
        p[2] = 90;
    }
    let mut r = BlockRenderer::new(&device, &queue, format, W, W, std::slice::from_ref(&weiss), 16, &muster, 16, &weiss, 16);
    r.set_glint_textures(&device, &queue, (&muster, 16, 16), (&muster, 16, 16));
    r.update_glint(&queue, 1234, 0.5, 1.0);
    let camera = Camera::new(DVec3::ZERO);
    r.update_camera(&queue, &camera, 1.0);
    let model = em::armor_model(world::armor::ArmorSlot::Chest);
    let pose = em::biped_pose(0.3, -0.2, 1.1, 0.7, false, 5.0);
    let base = em::entity_transform([0.0, -1.2, 3.2], 25.0, 1.0, em::PLAYER_MODEL_SCALE);
    let farbe = [0.6, 0.5, 0.9];
    let (skala, textur) = (render::glint::GLINT_SCALE_ARMOR, render::glint::GLINT_TEXTURE_ARMOR);

    let mut ecken = vec![bytemuck::Zeroable::zeroed(); em::vertex_count(&model, &pose, true)];
    em::emit_into(&model, &pose, base, RECT, farbe, true, &mut ecken);
    let mut lokal = Vec::new();
    em::emit_two_sided(&model, &pose, base, [0.0, 0.0, 1.0, 1.0], 1.0, &mut lokal);
    let mut zwillinge = Vec::new();
    render::glint::glint_vertices(&ecken, &lokal, skala, textur, &mut zwillinge);
    r.set_entity_vertices(&queue, &ecken, &vec![[0.0f32; 4]; ecken.len()], &[], (0, 0, 0));
    r.set_entity_glint_vertices(&queue, &zwillinge);
    let cpu = rendern(&device, &queue, &r);

    r.set_entity_vertices(&queue, &[], &[], &[], (0, 0, 0));
    r.set_entity_glint_vertices(&queue, &[]);
    let netz = r.skinning.add_mesh(&device, &queue, &em::skin_mesh(&model, true).vertices);
    let mut matrizen = Vec::new();
    SkinMatrix::extend_from_parts(&mut matrizen, &em::part_matrices(&model, &pose, base));
    let inst = SkinInstance { rect: RECT, color: farbe, matrix: 0, tint: [0.0; 4] };
    r.skinning.set_instances(&device, &queue, &[(netz, inst)], &matrizen);
    r.skinning.set_glint_instances(&device, &queue, &[(netz, SkinInstance { tint: [skala, textur, 0.0, 0.0], ..inst })]);
    let gpu = rendern(&device, &queue, &r);

    // Ohne Glint waere das Bild dunkler: sicherstellen, dass er ueberhaupt etwas beitraegt.
    r.skinning.set_glint_instances(&device, &queue, &[]);
    let ohne = rendern(&device, &queue, &r);
    let summe = |b: &[u8]| b.iter().map(|&v| u64::from(v)).sum::<u64>();
    assert!(summe(&gpu) > summe(&ohne) + 1000, "Glint traegt nichts bei");

    let belegt = cpu.chunks(4).filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0).count();
    let abweichend = cpu.chunks(4).zip(gpu.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 3)).count();
    eprintln!("Glint: belegt {belegt}, abweichend {abweichend}");
    assert!(belegt > 200, "kaum etwas gezeichnet: {belegt}");
    assert!(abweichend * 200 <= belegt, "{abweichend} von {belegt} Pixeln weichen ab");
}
