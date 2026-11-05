// TileWriter (animierte Kacheln per Zeichenaufruf in den Atlas) auf der echten GPU: Ergebnis
// Byte fuer Byte wie mit den Kopierbefehlen von upload_tiles, in allen Mip-Stufen, auch mit
// halb durchsichtigen und ganz durchsichtigen Pixeln. Ohne GPU wird nichts geprueft.
use wgpu::util::DeviceExt;

const GROESSE: u32 = 64;
const STUFEN: u32 = 7;

// Einfacher deterministischer Zufall (xorshift), damit jedes Byte etwas anderes ist.
fn bytes(n: usize, saat: &mut u32) -> Vec<u8> {
    (0..n)
        .map(|_| {
            *saat ^= *saat << 13;
            *saat ^= *saat >> 17;
            *saat ^= *saat << 5;
            (*saat >> 24) as u8
        })
        .collect()
}

fn atlas(device: &wgpu::Device, queue: &wgpu::Queue, hintergrund: &[Vec<u8>]) -> wgpu::Texture {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: GROESSE, height: GROESSE, depth_or_array_layers: 1 },
            mip_level_count: STUFEN,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &hintergrund.concat(),
    );
    texture
}

// Alle Stufen zuruecklesen, ohne Zeilenauffuellung.
fn lesen(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<Vec<u8>> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let puffer: Vec<(wgpu::Buffer, u32, u32)> = (0..STUFEN)
        .map(|level| {
            let kante = (GROESSE >> level).max(1);
            let zeile = (kante * 4).div_ceil(256) * 256;
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(zeile * kante), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTexture { texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::ImageCopyBuffer { buffer: &b, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(zeile), rows_per_image: Some(kante) } },
                wgpu::Extent3d { width: kante, height: kante, depth_or_array_layers: 1 },
            );
            (b, zeile, kante)
        })
        .collect();
    queue.submit(Some(encoder.finish()));
    puffer
        .iter()
        .map(|(b, zeile, kante)| {
            b.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::Maintain::Wait);
            let roh = b.slice(..).get_mapped_range();
            let mut out = Vec::new();
            for y in 0..*kante as usize {
                let start = y * *zeile as usize;
                out.extend_from_slice(&roh[start..start + *kante as usize * 4]);
            }
            out
        })
        .collect()
}

#[test]
fn zeichnen_wie_kopieren() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    eprintln!("GPU: {}", adapter.get_info().name);

    let mut saat = 0x1234_5678;
    let hintergrund: Vec<Vec<u8>> = (0..STUFEN).map(|l| bytes(((GROESSE >> l).max(1).pow(2) * 4) as usize, &mut saat)).collect();
    // Kacheln wie im Spiel: Ursprung auf ihrem Raster, je Stufe halb so gross; dazu eine
    // Kachel mit weniger Stufen und eine mit ganz und halb durchsichtigen Pixeln.
    let kachel = |kante: u32, stufen: u32, saat: &mut u32| -> Vec<Vec<u8>> { (0..stufen).map(|l| bytes(((kante >> l).pow(2) * 4) as usize, saat)).collect() };
    let mut durchsichtig = kachel(16, 5, &mut saat);
    for stufe in &mut durchsichtig {
        for (i, px) in stufe.chunks_exact_mut(4).enumerate() {
            px[3] = [0, 255, 1, 128][i % 4];
        }
    }
    let kacheln: Vec<(Vec<Vec<u8>>, (u32, u32), u32)> = vec![
        (kachel(32, 6, &mut saat), (0, 0), 32),
        (kachel(16, 5, &mut saat), (32, 0), 16),
        (kachel(16, 2, &mut saat), (48, 16), 16),
        (durchsichtig, (32, 32), 16),
    ];
    let liste: Vec<(&[Vec<u8>], (u32, u32), u32)> = kacheln.iter().map(|(m, o, s)| (m.as_slice(), *o, *s)).collect();

    let kopiert = atlas(&device, &queue, &hintergrund);
    let mut zwischen = None;
    render::upload_tiles(&device, &queue, &kopiert, &mut zwischen, &liste);

    let gezeichnet = atlas(&device, &queue, &hintergrund);
    let writer = render::TileWriter::new(&device, &gezeichnet);
    queue.submit(writer.encode(&device, &liste));

    let a = lesen(&device, &queue, &kopiert);
    let b = lesen(&device, &queue, &gezeichnet);
    assert_ne!(a, hintergrund, "die Kopien haben nichts geaendert -- Test prueft nichts");
    for (level, (a, b)) in a.iter().zip(&b).enumerate() {
        let abweichend = a.iter().zip(b).filter(|(x, y)| x != y).count();
        assert_eq!(abweichend, 0, "Stufe {level}: {abweichend} Bytes anders");
    }
}
