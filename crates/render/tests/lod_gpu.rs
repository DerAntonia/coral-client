// Fernsicht (lod_renderer) auf der echten GPU: ein Knoten mit einem Boden wird von oben und
// von der Seite gezeichnet und zurueckgelesen. Prueft Ecken-Aufbau und Windung (sonst fehlen
// Flaechen), Schattierung je Seite und Spaltenmaske.

use chunk_saver::node::{voxel, Node, NodeKey, N};
use chunk_saver::states::{LodStates, CAT_AIR, CAT_OPAQUE};
use glam::DVec3;
use render::lod_renderer::{LodFrame, LodRenderer};

const W: u32 = 64;
const H: u32 = 64;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SKY: wgpu::Color = wgpu::Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu() -> Option<Gpu> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let features = adapter.features() & (wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: features, ..Default::default() }, None)).ok()?;
    Some(Gpu { device, queue })
}

// Weisser Atlas mit zwei Mip-Stufen; State 1 belegt ihn auf allen Seiten ganz.
fn renderer(g: &Gpu) -> LodRenderer {
    let mut lod = LodRenderer::new(&g.device, FORMAT, W, H);
    let atlas = g.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 2, height: 2, depth_or_array_layers: 1 },
        mip_level_count: 2,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, size) in [(0u32, 2u32), (1, 1)] {
        g.queue.write_texture(
            wgpu::ImageCopyTexture { texture: &atlas, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &vec![255u8; (size * size * 4) as usize],
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: Some(size) },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
    }
    lod.set_atlas(std::sync::Arc::new(atlas));
    let mut info = vec![0u32; 2 * 16];
    for face in 0..6 {
        info[16 + 6 + face] = 0xFFFF | 0xFFFF << 16;
    }
    lod.set_state_rects(&g.device, &g.queue, &info, 16);
    lod
}

// Boden aus State 1 bis y = 3 im Knoten (0, 0, 0).
fn floor_mesh() -> chunk_saver::NodeMesh {
    let mut n = Node::empty();
    for z in 0..N {
        for x in 0..N {
            for y in 0..4 {
                n.set(x, y, z, voxel(1, 0));
            }
        }
    }
    chunk_saver::mesh::mesh_node(&n, &Default::default(), &LodStates::from_categories(vec![CAT_AIR, CAT_OPAQUE]), &world::biome_palette::BiomePalette::fallback())
}

fn draw(g: &Gpu, lod: &mut LodRenderer, camera: &render::Camera) -> Vec<[u8; 4]> {
    let key = NodeKey::new(0, 0, 0, 0);
    lod.prepare(&g.device, &g.queue, &LodFrame { draw: &[key], camera, aspect: 1.0, mask: true, ao_strength: 1.0, clip: ((0, 0), 64) });
    let target = g.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let readback = g.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (W * H * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(SKY), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(lod.depth_attachment()),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        lod.render(&mut pass);
    }
    enc.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer { buffer: &readback, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(H) } },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    g.queue.submit([enc.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    g.device.poll(wgpu::Maintain::Wait);
    let data = readback.slice(..).get_mapped_range().to_vec();
    data.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect()
}

fn px(img: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    img[(y * W + x) as usize]
}

#[test]
fn boden_von_oben_und_von_der_seite() {
    let Some(g) = gpu() else {
        eprintln!("keine GPU -- uebersprungen");
        return;
    };
    g.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut lod = renderer(&g);
    lod.upload(&g.device, &g.queue, NodeKey::new(0, 0, 0, 0), &floor_mesh());

    // Von oben, mitten ueber dem Boden: die Oberseite (Schattierung 1.0) fuellt das Bild.
    let mut cam = render::Camera::new(DVec3::new(16.0, 20.0, 16.0));
    cam.pitch = -89.0;
    let img = draw(&g, &mut lod, &cam);
    assert_eq!(px(&img, 32, 32), [255, 255, 255, 255], "Oberseite fehlt oder ist falsch schattiert");

    // Von der Seite, vor der Westkante (x < 0), Blick nach Osten (+x): man sieht die Westseite
    // (Schattierung 0.6) unten im Bild und darueber den Himmel.
    let mut cam = render::Camera::new(DVec3::new(-10.0, 2.0, 16.0));
    cam.yaw = -90.0;
    let img = draw(&g, &mut lod, &cam);
    let mitte = px(&img, 32, 34);
    assert!(mitte[0] > 120 && mitte[0] < 220 && mitte[0] == mitte[2], "Westseite: {mitte:?}");
    assert_eq!(px(&img, 32, 2), [0, 0, 255, 255], "oben Himmel");

    // Liegt der Chunk unter dem Auge als geladen in der Maske, zeichnet die Fernsicht dort
    // nichts -- von oben bleibt der Himmel. (Mitten im Chunk, nicht auf seiner Kante.)
    let mut cam = render::Camera::new(DVec3::new(24.0, 20.0, 24.0));
    cam.pitch = -89.0;
    lod.set_column_covered(1, 1, true);
    let img = draw(&g, &mut lod, &cam);
    assert_eq!(px(&img, 32, 32), [0, 0, 255, 255], "gedeckter Chunk muss frei bleiben");
    lod.set_column_covered(1, 1, false);

    assert!(pollster::block_on(g.device.pop_error_scope()).is_none());
}
