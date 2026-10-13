// Niederschlag auf der echten GPU: ein Quad vor der Kamera durch den ganzen BlockRenderer
// (Pipeline, Textur, Puffer, Zeichenreihenfolge). In der Bildmitte muss die Texturfarbe
// stehen. Ohne GPU wird nichts geprueft.
use glam::DVec3;
use render::{BlockRenderer, Camera, WeatherVertex};

const W: u32 = 64;

fn mitte(radii: [f32; 2]) -> Option<[u8; 4]> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let leer = vec![0u8; 16 * 16 * 4];
    let mut r = BlockRenderer::new(&device, &queue, format, W, W, std::slice::from_ref(&leer), 16, &leer, 16, &leer, 16);
    // Regen: weisse, deckende 2x2-Textur; Schnee bleibt leer.
    r.set_weather_textures(&device, &queue, (&[255u8; 16], 2, 2), (&[0u8; 4], 1, 1));
    let ecke = |x: f32, y: f32| WeatherVertex { position: [x, y, 3.0], color: [1.0; 4], uv: [x, y], radii };
    let quad = [ecke(-1.0, 1.0), ecke(1.0, 1.0), ecke(1.0, -1.0), ecke(-1.0, 1.0), ecke(1.0, -1.0), ecke(-1.0, -1.0)];
    r.set_weather_vertices(&queue, &quad, 6, 0);
    // Kamera im Ursprung, Blick nach +Z (Gierwinkel 0).
    r.update_camera(&queue, &Camera::new(DVec3::ZERO), 1.0);

    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let zv = ziel.create_view(&Default::default());
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &zv,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(r.depth_attachment()),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        r.render(&mut pass, &mut |_, _| {});
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
    let i = ((W / 2 * W + W / 2) * 4) as usize;
    Some([daten[i], daten[i + 1], daten[i + 2], daten[i + 3]])
}

#[test]
fn regen_quad_vor_der_kamera_ist_sichtbar() {
    let Some(px) = mitte([0.0, 0.0]) else { return };
    println!("Mitte ohne Kugel: {px:?}");
    assert!(px[0] > 200, "ohne Kugel: Mitte {px:?}");
    // Kugel aussen 16: das Quad liegt 3 Bloecke vor der Kamera, also voll sichtbar.
    let px = mitte([0.0, 16.0]).unwrap();
    assert!(px[0] > 200, "Kugel 0..16: Mitte {px:?}");
    // Innen 5: das Quad liegt in der inneren Kugel und muss verschwinden.
    let px = mitte([5.0, 16.0]).unwrap();
    assert!(px[0] < 10, "Kugel 5..16: Mitte {px:?}");
}
