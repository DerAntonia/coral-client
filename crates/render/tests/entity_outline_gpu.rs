// Der Leucht-Umriss auf der echten GPU: ein weisses Quadrat oben links in der Silhouette muss
// im Bild als heller Rand um genau dieses Quadrat erscheinen -- innen und weit weg bleibt es
// dunkel, und unten rechts (gespiegelt) passiert nichts. Ohne GPU wird nichts geprueft.
use render::entity_outline::{EntityOutline, FORMAT};

const W: u32 = 64;

#[test]
fn weisses_quadrat_bekommt_einen_hellen_rand() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let mut outline = EntityOutline::new(&device, FORMAT);
    outline.prepare(&device, &queue, (W, W));

    // Silhouette: Quadrat x, y in 8..24, weiss und deckend.
    let mut sil = vec![0u8; (W * W * 4) as usize];
    for y in 8..24 {
        for x in 8..24 {
            sil[((y * W + x) * 4) as usize..][..4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    let ausdehnung = wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 };
    queue.write_texture(outline.silhouette_texture().as_image_copy(), &sil, wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(W) }, ausdehnung);

    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: ausdehnung,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut enc = device.create_command_encoder(&Default::default());
    outline.apply(&mut enc, &ziel.create_view(&Default::default()));
    let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (W * W * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(ziel.as_image_copy(), wgpu::ImageCopyBuffer { buffer: &lese, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(W) } }, ausdehnung);
    queue.submit([enc.finish()]);
    lese.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let bild = lese.slice(..).get_mapped_range().to_vec();
    let hell = |x: u32, y: u32| bild[((y * W + x) * 4) as usize];

    // Auf der Kante (oben, links, rechts, unten) hell.
    for (x, y) in [(16, 8), (8, 16), (23, 16), (16, 23)] {
        assert!(hell(x, y) > 80, "Rand bei ({x}, {y}) fehlt: {}", hell(x, y));
    }
    // Mitte des Quadrats, weit weg und die gespiegelte Stelle unten rechts: dunkel.
    for (x, y) in [(16, 16), (50, 50), (16, 56), (56, 16)] {
        assert!(hell(x, y) < 10, "bei ({x}, {y}) darf nichts sein: {}", hell(x, y));
    }
}
