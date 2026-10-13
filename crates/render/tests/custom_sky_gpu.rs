// Pack-Himmel auf der echten GPU: ein Bild mit einer eigenen Farbe je Zelle des 3x2-Rasters.
// Blickt die Kamera in eine Achsenrichtung, muss in der Bildmitte die Farbe der Zelle stehen,
// die zu dieser Richtung gehoert; mit halber Helligkeit ("alpha") die halbe Farbe. Ohne GPU
// wird nichts geprueft.
use glam::{Mat3, Mat4, Vec3};
use render::custom_sky::{Blend, SkyLayerDraw, SkyRenderer};

const W: u32 = 64;

// Zellfarben in Rasterreihenfolge: Zeile 0 = -Y, +Y, +X; Zeile 1 = +Z, -X, -Z.
const FARBEN: [[u8; 3]; 6] = [[200, 0, 0], [0, 200, 0], [0, 0, 200], [200, 200, 0], [0, 200, 200], [200, 0, 200]];

fn raster() -> Vec<u8> {
    // Je Zelle 4x4 Pixel, also 12x8.
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            let zelle = (y / 4) * 3 + x / 4;
            let [r, g, b] = FARBEN[zelle];
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    rgba
}

fn mitte(device: &wgpu::Device, queue: &wgpu::Queue, sky: &SkyRenderer, blick: Vec3, layer: SkyLayerDraw) -> [u8; 3] {
    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let zv = ziel.create_view(&Default::default());
    let oben = if blick.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y };
    let view_proj = Mat4::perspective_rh(70f32.to_radians(), 1.0, 0.05, 1000.0) * Mat4::look_to_rh(Vec3::ZERO, blick, oben);
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &zv,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        sky.draw(queue, &mut pass, view_proj, &[layer]);
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
    [daten[i], daten[i + 1], daten[i + 2]]
}

#[test]
fn jede_richtung_zeigt_ihre_zelle() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    println!("GPU: {}", adapter.get_info().name);
    let mut sky = SkyRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let id = sky.add_texture(&device, 12, 8, raster());
    while !sky.is_ready(id) {
        sky.upload_step(&queue);
    }
    let ebene = |brightness| SkyLayerDraw { texture: id, blend: Blend::Alpha, to_local: Mat3::IDENTITY, brightness };
    let richtungen = [Vec3::NEG_Y, Vec3::Y, Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z];
    for (blick, farbe) in richtungen.into_iter().zip(FARBEN) {
        assert_eq!(mitte(&device, &queue, &sky, blick, ebene(1.0)), farbe, "Blick {blick}");
    }
    let halb = mitte(&device, &queue, &sky, Vec3::X, ebene(0.5));
    for (h, f) in halb.iter().zip(FARBEN[2]) {
        assert!((*h as i32 - f as i32 / 2).abs() <= 1, "halbe Helligkeit: {halb:?}");
    }
    // Die Ebene gedreht: 90 Grad um Y bringt die +X-Zelle nach -Z (to_local bildet die
    // Welt-Richtung -Z auf +X ab).
    let gedreht = SkyLayerDraw { to_local: Mat3::from_rotation_y(-90f32.to_radians()), ..ebene(1.0) };
    assert_eq!(mitte(&device, &queue, &sky, Vec3::NEG_Z, gedreht), FARBEN[2]);
}
