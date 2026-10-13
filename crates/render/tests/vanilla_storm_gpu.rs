// Der Sturmwand-Shader des UHC-Packs auf der echten GPU: ein einziges Markierungs-Quad wie das
// Item-Display des Plugins, gezeichnet durch das Pack-Programm (vanilla_shader +
// vanilla_draw). Von innen in die Wand geschaut muss das Bild rot werden; ohne Marker bleibt
// es schwarz. Braucht die Installation, das Pack und eine GPU -- fehlt etwas, wird nichts
// geprueft.
use render::vanilla_draw::{VanillaDraw, VanillaFrame, VanillaVertex, OVERLAY_NONE};

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
        draw.draw_items(&mut pass);
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
fn die_sturmwand_erscheint_von_innen() {
    let pack = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Dokumente/uhc-battle-royale/docs/resourcepack");
    let Ok(mut mc) = assets::MinecraftInstallation::locate("1.21.11", vec![pack.clone()]) else { return };
    if !pack.exists() {
        return;
    }
    let mut load = |p: &str| mc.load_bytes(p).ok().and_then(|b| String::from_utf8(b).ok());
    let program = render::vanilla_shader::compile("minecraft/shaders/core/rendertype_item_entity_translucent_cull", &mut load).unwrap();

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
    draw.set_item_program(&device, Some(program)).expect("Pipeline");

    // Kamera bei y = 100 ueber der Kartenmitte, Blick nach +Z (Yaw 0).
    let frame = VanillaFrame {
        view: glam::Mat4::look_to_rh(glam::Vec3::ZERO, glam::Vec3::Z, glam::Vec3::Y),
        proj_gl: glam::Mat4::perspective_rh_gl(70f32.to_radians(), 1.0, 0.05, 1000.0),
        screen: [W as f32, W as f32],
        camera: glam::DVec3::new(0.0, 100.0, 0.0),
        game_time: 0.0,
        fog_color: [0.0; 4],
        fog: [1e6, 1e6, 1e6, 1e6, 1e6, 1e6],
        sky_factor: 1.0,
        brightness: 0.5,
    };
    draw.update(&queue, &frame);

    // Der Marker: ein Quad mit Seitenlaenge = Radius 50 um die Mitte (0, 0, 0), Farbe
    // (200, Radius*4 >> 8, Radius*4 & 0xFF) -- so baut ihn das Plugin.
    let r = 50.0f32;
    let farbe = [200.0 / 255.0, 0.0, 200.0 / 255.0, 1.0];
    let ecke = |x: f32, y: f32| VanillaVertex { position: [x, y - 100.0, 0.0], color: farbe, uv0: [0.0, 0.0], uv1: OVERLAY_NONE, uv2: [240, 240], normal: [0.0, 0.0, -1.0], ..Default::default() };
    let quad = [ecke(-r / 2.0, -r / 2.0), ecke(r / 2.0, -r / 2.0), ecke(r / 2.0, r / 2.0), ecke(-r / 2.0, r / 2.0)];

    draw.set_item_vertices(&device, &queue, &[]);
    let leer = bild(&device, &queue, &draw);
    assert!(leer.chunks(4).all(|p| p[0] == 0), "ohne Marker bleibt das Bild schwarz");

    draw.set_item_vertices(&device, &queue, &quad);
    let mit = bild(&device, &queue, &draw);
    let rote = mit.chunks(4).filter(|p| p[0] > 20 && p[0] as u16 > p[1] as u16 + 15 && p[0] as u16 > p[2] as u16 + 15).count();
    eprintln!("rote Pixel: {rote} von {}", W * W);
    // MC_STORM_DUMP=datei.ppm: das Bild zum Ansehen ablegen.
    if let Some(pfad) = std::env::var_os("MC_STORM_DUMP") {
        let mut ppm = format!("P6 {W} {W} 255\n").into_bytes();
        ppm.extend(mit.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
        std::fs::write(pfad, ppm).unwrap();
    }
    // Die Wand fuellt von innen das ganze Bild (Hoehe 40..400, man steht bei 100).
    assert!(rote > (W * W) as usize * 9 / 10, "die Wand fehlt: {rote} rote Pixel");
}
