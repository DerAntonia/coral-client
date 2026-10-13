// Vanillas eigener Grenz-Shader (rendertype_world_border aus dem Jar) auf der echten GPU: 16
// Bloecke vor der Ostkante, Blick nach Osten. Die Wand muss blau (Stillstand) und additiv im
// Bild stehen, mit den Luecken ihres Kraftfeld-Musters. Braucht die Installation und eine GPU
// -- fehlt etwas, wird nichts geprueft.
use render::vanilla_draw::{world_border_draw, VanillaDraw, VanillaFrame};

const W: u32 = 256;

fn bild(device: &wgpu::Device, queue: &wgpu::Queue, draw: &VanillaDraw) -> Vec<u8> {
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
    let tiefe = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let (zv, tv) = (ziel.create_view(&Default::default()), tiefe.create_view(&Default::default()));
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &zv,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &tv,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        draw.draw_border(&mut pass);
    }
    let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (W * W * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        ziel.as_image_copy(),
        wgpu::ImageCopyBuffer { buffer: &lese, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(W) } },
        wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    lese.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let daten = lese.slice(..).get_mapped_range().to_vec();
    daten
}

#[test]
fn die_grenze_erscheint_blau_mit_muster() {
    let Ok(mut mc) = assets::MinecraftInstallation::locate("1.21.11", Vec::new()) else { return };
    let mut load = |p: &str| mc.load_bytes(p).ok().and_then(|b| String::from_utf8(b).ok());
    let program = render::vanilla_shader::compile("minecraft/shaders/core/rendertype_world_border", &mut load).expect("Vanillas Grenz-Shader");
    let tex = mc.load_texture("minecraft/textures/misc/forcefield.png").expect("forcefield.png");

    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let atlas = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    let mut draw = VanillaDraw::new(&device, &queue, wgpu::TextureFormat::Rgba8UnormSrgb, &atlas);
    draw.set_border_program(&device, &queue, Some(program), (tex.width, tex.height, &tex.rgba)).expect("Pipeline");

    let kamera = glam::DVec3::new(84.0, 70.0, 0.0);
    let view = glam::Mat4::look_to_rh(glam::Vec3::ZERO, glam::Vec3::X, glam::Vec3::Y);
    let frame = VanillaFrame {
        view,
        proj_gl: glam::Mat4::perspective_rh_gl(70f32.to_radians(), 1.0, 0.05, 1000.0),
        screen: [W as f32, W as f32],
        camera: kamera,
        game_time: 0.0,
        fog_color: [0.0; 4],
        fog: [1e6, 1e6, 1e6, 1e6, 1e6, 1e6],
        sky_factor: 1.0,
        brightness: 0.5,
    };
    draw.update(&queue, &frame);

    let mut grenze = world::world_border::WorldBorder::default();
    grenze.apply(world::world_border::BorderUpdate::Size(200.0));
    let wand = world_border_draw(&grenze, 1.0, kamera, 64.0).expect("Grenze in Sichtweite");
    draw.set_border(&queue, view, 0, Some(&wand));
    let mit = bild(&device, &queue, &draw);
    draw.set_border(&queue, view, 0, None);
    let leer = bild(&device, &queue, &draw);
    assert!(leer.chunks(4).all(|p| p[2] == 0), "ohne Grenze bleibt das Bild schwarz");

    let blaue = mit.chunks(4).filter(|p| p[2] > 30 && p[2] > p[0]).count();
    let schwarze = mit.chunks(4).filter(|p| p[0] == 0 && p[1] == 0 && p[2] == 0).count();
    eprintln!("blaue Pixel: {blaue}, schwarze: {schwarze} von {}", W * W);
    // MC_BORDER_DUMP=datei.ppm: das Bild zum Ansehen ablegen.
    if let Some(pfad) = std::env::var_os("MC_BORDER_DUMP") {
        let mut ppm = format!("P6 {W} {W} 255\n").into_bytes();
        ppm.extend(mit.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
        std::fs::write(pfad, ppm).unwrap();
    }
    // Die Wand fuellt das Bild, das Muster laesst Luecken.
    assert!(blaue > (W * W) as usize / 10, "die Wand fehlt: {blaue} blaue Pixel");
    assert!(schwarze > (W * W) as usize / 10, "das Muster fehlt: nur {schwarze} Luecken");
}
