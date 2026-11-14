// Render-Order auf der echten GPU (s. render::render_order): ein "Spieler" (blaues Viereck im
// Entity-Puffer, Klasse PvP), davor ein rotes deckendes Partikel, dahinter eine gruene
// Hitbox-Kante. Ohne Einstellung gewinnt, was vorn steht; mit "immer ueber" das Obere, auch
// wenn es hinten steht. Ohne GPU nichts.
use glam::DVec3;
use render::render_order::{cover, RenderOrder, CLASS_PVP};
use render::{BlockRenderer, Camera, HitboxVertex, ParticleDraws, ParticleVertex};
use world::BlockVertex;

const W: u32 = 64;

fn pass_ziel(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = ziel.create_view(&Default::default());
    (ziel, view)
}

// Die Mitte des Bildes als rgb. `scharf`: die Hitboxen wie mit Bewegungsunschaerfe erst in
// einem zweiten Durchgang auf der stehengebliebenen Tiefe (s. BlockRenderer::render_hitboxes).
fn mitte(device: &wgpu::Device, queue: &wgpu::Queue, r: &mut BlockRenderer, scharf: bool) -> [u8; 3] {
    r.keep_depth = scharf;
    let (ziel, view) = pass_ziel(device);
    let mut encoder = device.create_command_encoder(&Default::default());
    let farbe = |load| Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[farbe(wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
            depth_stencil_attachment: Some(r.depth_attachment()),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        r.render(&mut pass, &mut |_, _| {});
    }
    if scharf {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[farbe(wgpu::LoadOp::Load)],
            depth_stencil_attachment: Some(r.depth_attachment_load()),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        r.render_hitboxes(&mut pass);
    }
    let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(W * W * 4), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    encoder.copy_texture_to_buffer(
        ziel.as_image_copy(),
        wgpu::ImageCopyBuffer { buffer: &lese, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(W) } },
        wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));
    lese.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let bild = lese.slice(..).get_mapped_range().to_vec();
    let i = ((W / 2 * W + W / 2) * 4) as usize;
    [bild[i], bild[i + 1], bild[i + 2]]
}

// Ein Viereck quer zur Blickrichtung in der Entfernung `z`, in beiden Wicklungen (das Culling
// soll hier keine Rolle spielen).
fn viereck<T>(z: f32, halb: f32, ecke: impl Fn([f32; 3]) -> T) -> Vec<T> {
    let p = [[-halb, -halb, z], [halb, -halb, z], [halb, halb, z], [-halb, halb, z]];
    [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2].into_iter().map(|i| ecke(p[i])).collect()
}

fn rot(c: [u8; 3]) -> bool {
    c[0] > 150 && c[1] < 80 && c[2] < 80
}
fn gruen(c: [u8; 3]) -> bool {
    c[1] > 150 && c[0] < 80 && c[2] < 80
}
fn blau(c: [u8; 3]) -> bool {
    c[2] > 150 && c[0] < 80 && c[1] < 80
}

#[test]
fn immer_ueber_schlaegt_die_tiefe() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- nichts geprueft");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let weiss = vec![255u8; 16 * 16 * 4];
    let mut r = BlockRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8UnormSrgb, W, W, std::slice::from_ref(&weiss), 16, &weiss, 16, &weiss, 16);
    // Vor update_camera: die Hitbox-Parameter gehen mit der Kamera in denselben Puffer.
    r.set_hitbox_params((W as f32, W as f32));
    r.update_camera(&queue, &Camera::new(DVec3::ZERO), 1.0);

    // Der Spieler: blau, 4 Bloecke entfernt.
    let spieler = viereck(4.0, 1.0, |position| BlockVertex { position, color: [0.0, 0.0, 1.0], uv: [0.5, 0.5] });
    let tints = vec![[0.0f32; 4]; spieler.len()];
    r.set_entity_vertices(&queue, &spieler, &tints, &[], (0, 0, 0));
    r.set_entity_classes(vec![(0..spieler.len() as u32, CLASS_PVP)]);
    let nur_spieler = mitte(&device, &queue, &mut r, false);
    assert!(blau(nur_spieler), "der Spieler allein: {nur_spieler:?}");

    // Ein deckendes rotes Partikel davor.
    let partikel = viereck(2.0, 0.3, |position| ParticleVertex { position, color: [1.0, 0.0, 0.0, 1.0], uv: [0.5, 0.5] });
    r.set_particle_vertices(&queue, &partikel, ParticleDraws { opaque: partikel.len() as u32, translucent: 0, block: 0 });
    let vorgabe = mitte(&device, &queue, &mut r, false);
    assert!(rot(vorgabe), "ohne Einstellung verdeckt das Partikel davor: {vorgabe:?}");
    r.render_order = RenderOrder { pvp: cover::WATER | cover::PARTICLES, ..RenderOrder::default() };
    let drueber = mitte(&device, &queue, &mut r, false);
    assert!(blau(drueber), "Spieler ueber Partikel: {drueber:?}");
    // Durchscheinende Partikel ebenso.
    r.set_particle_vertices(&queue, &partikel, ParticleDraws { opaque: 0, translucent: partikel.len() as u32, block: 0 });
    let drueber = mitte(&device, &queue, &mut r, false);
    assert!(blau(drueber), "Spieler ueber durchscheinendem Partikel: {drueber:?}");
    r.render_order = RenderOrder::default();
    let vorgabe = mitte(&device, &queue, &mut r, false);
    assert!(rot(vorgabe), "ohne Einstellung liegt auch das durchscheinende davor: {vorgabe:?}");
    r.set_particle_vertices(&queue, &[], ParticleDraws::default());

    // Eine gruene Hitbox-Kante HINTER dem Spieler, quer durch die Bildmitte.
    let mut kante = Vec::new();
    const ECKEN: [[f32; 2]; 6] = [[-1.0, 0.0], [-1.0, 1.0], [1.0, 1.0], [-1.0, 0.0], [1.0, 1.0], [1.0, 0.0]];
    kante.extend(ECKEN.map(|side_and_end| HitboxVertex { pos_a: [-0.5, 0.0, 6.0], pos_b: [0.5, 0.0, 6.0], side_and_end, color: [0.0, 1.0, 0.0, 1.0], half_width: 3.0 }));
    r.set_hitbox_vertices(&queue, &kante);
    for scharf in [false, true] {
        r.render_order = RenderOrder::default();
        let vorgabe = mitte(&device, &queue, &mut r, scharf);
        assert!(blau(vorgabe), "ohne Einstellung verdeckt der Spieler die Kante dahinter (scharf {scharf}): {vorgabe:?}");
        r.render_order = RenderOrder { hitbox: cover::WATER | cover::PLAYERS, ..RenderOrder::default() };
        let drueber = mitte(&device, &queue, &mut r, scharf);
        assert!(gruen(drueber), "Hitbox ueber Spielern (scharf {scharf}): {drueber:?}");
    }
    // Ohne den Spieler steht die Kante in beiden Faellen da.
    r.set_entity_vertices(&queue, &[], &[], &[], (0, 0, 0));
    r.set_entity_classes(Vec::new());
    r.render_order = RenderOrder::default();
    assert!(gruen(mitte(&device, &queue, &mut r, false)));
}
