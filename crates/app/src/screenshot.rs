// Debug-Flag CORAL_SCREENSHOT: F2 kopiert das gerade gezeigte Bild in die Zwischenablage.
//
// Gedacht fuer Windows, wo Screenshot-Werkzeuge im Exclusive-Fullscreen nur Schwarz liefern.
// Gelesen wird die Swapchain-Textur selbst, nach dem letzten Zeichenschritt (inkl. Crosshair),
// also genau das Bild, das auch angezeigt wird. Ohne das Flag bekommt die Swapchain kein
// COPY_SRC und der Frame-Ablauf bleibt unveraendert.

pub fn enabled() -> bool {
    std::env::var_os("CORAL_SCREENSHOT").is_some()
}

/// Eine angestossene Kopie der Swapchain-Textur in einen lesbaren Puffer.
pub struct Pending {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    format: wgpu::TextureFormat,
}

/// Die Kopie in den Encoder legen, bevor er abgeschlossen wird.
pub fn copy(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture, format: wgpu::TextureFormat) -> Pending {
    let (width, height) = (texture.width(), texture.height());
    let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot readback"),
        size: padded_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(height) },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    Pending { buffer, width, height, padded_row, format }
}

/// Nach dem Submit: auf die GPU warten, Pixel nach RGBA8 umsortieren und in einem eigenen
/// Thread in die Zwischenablage legen (das Kodieren dauert, der Frame soll nicht warten).
pub fn finish(device: &wgpu::Device, pending: Pending) {
    let Pending { buffer, width, height, padded_row, format } = pending;
    let bgra = match format {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
        other => {
            println!("[screenshot] Swapchain-Format {other:?} wird nicht unterstuetzt");
            return;
        }
    };
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    if !matches!(rx.recv(), Ok(Ok(()))) {
        println!("[screenshot] Puffer liess sich nicht lesen");
        return;
    }
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    {
        let data = slice.get_mapped_range();
        for row in data.chunks_exact(padded_row as usize) {
            for px in row[..(width * 4) as usize].chunks_exact(4) {
                if bgra {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
    }
    buffer.unmap();
    std::thread::spawn(move || {
        let bild = arboard::ImageData { width: width as usize, height: height as usize, bytes: rgba.into() };
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_image(bild)) {
            Ok(()) => println!("[screenshot] {width}x{height} in der Zwischenablage"),
            Err(e) => println!("[screenshot] Zwischenablage: {e}"),
        }
    });
}
