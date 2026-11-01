// Atlas-Kacheln hochladen und anordnen.

// Kacheln in `texture` ersetzen: je Kachel eine Liste von Mip-Stufen (`[0]` = volle Groesse,
// beliebig viele Stufen), Ursprung und Kantenlaenge. `staging` ist ein wiederverwendeter
// Zwischenpuffer des Aufrufers.
//
// EIN write_buffer plus Kopierbefehle statt je Kachel und Stufe ein write_texture: wgpu legt
// fuer jedes write_texture einen eigenen Staging-Puffer an.
pub fn upload_tiles(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    staging: &mut Option<wgpu::Buffer>,
    tiles: &[(&[Vec<u8>], (u32, u32), u32)],
) {
    let Some((data, regions)) = tile_layout(texture, tiles) else { return };
    let needed = data.len() as u64;
    if staging.as_ref().is_none_or(|b| b.size() < needed) {
        *staging = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("atlas upload buffer"),
            size: needed.next_power_of_two(),
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    let buffer = staging.as_ref().unwrap();
    queue.write_buffer(buffer, 0, &data);
    queue.submit(Some(tile_copies(device, buffer, texture, &regions)));
}

// Wie upload_tiles, aber nur aufgezeichnet: ein fertiger Befehlspuffer mit eigenem Zwischenpuffer,
// den ein anderer Thread zu einem selbst gewaehlten Zeitpunkt abschickt. So kann der
// Animations-Thread der App die Arbeit machen, ohne dass seine Kopien irgendwann mitten in einen
// Frame auf der GPU fallen (s. App::submit_block_animations). Ohne write_buffer: das liefe ueber
// die Warteschlange und kaeme mit dem naechsten Submit irgendeines Threads.
#[must_use] 
pub fn encode_tiles(device: &wgpu::Device, texture: &wgpu::Texture, tiles: &[(&[Vec<u8>], (u32, u32), u32)]) -> Option<wgpu::CommandBuffer> {
    let (data, regions) = tile_layout(texture, tiles)?;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("atlas animation buffer"),
        size: data.len() as u64,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    buffer.slice(..).get_mapped_range_mut().copy_from_slice(&data);
    buffer.unmap();
    Some(tile_copies(device, &buffer, texture, &regions))
}

// (Stufe, Ursprung, Kante, Offset, Zeilenlaenge im Puffer)
pub(super) type TileRegion = (u32, (u32, u32), u32, u64, u32);

// Pufferinhalt und Kopierbereiche fuer upload_tiles/encode_tiles; None = nichts zu tun. Die
// Laenge ist ein Vielfaches von 512 Byte.
pub(super) fn tile_layout(texture: &wgpu::Texture, tiles: &[(&[Vec<u8>], (u32, u32), u32)]) -> Option<(Vec<u8>, Vec<TileRegion>)> {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let max_level = texture.mip_level_count() as usize;
    // Gleich grosse Kacheln einer Stufe liegen im Puffer NEBENEINANDER in einem Streifen,
    // aufgefuellt auf die 256-Byte-Zeilenausrichtung wird nur die ganze Streifenzeile.
    // Frueher bekam jede Kachelzeile ihre eigene Auffuellung: eine 32er-Kachel (128 Byte je
    // Zeile) belegte mit Mips ~16 KB statt ~5 KB, und seit alle Animationen zeitversetzt
    // laufen (~150 Kacheln je Tick) waren das 2,5 MB je Tick -- Spitzen bis 3 ms.
    let mut groups: Vec<((u32, u32), Vec<(&[u8], (u32, u32))>)> = Vec::new();
    for &(mips, origin, tile_size) in tiles {
        for (level, rgba) in mips.iter().enumerate().take(max_level) {
            let size = tile_size >> level;
            if size == 0 || rgba.len() < (size * size * 4) as usize {
                break;
            }
            let key = (level as u32, size);
            let entry = (rgba.as_slice(), (origin.0 >> level, origin.1 >> level));
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => list.push(entry),
                None => groups.push((key, vec![entry])),
            }
        }
    }
    let mut regions: Vec<TileRegion> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    for ((level, size), list) in &groups {
        let tile_row = (*size * 4) as usize;
        let padded = ((list.len() * tile_row) as u32).div_ceil(align) * align;
        let base = data.len();
        data.resize(base + padded as usize * *size as usize, 0);
        for (k, (rgba, origin)) in list.iter().enumerate() {
            for y in 0..*size as usize {
                let dst = base + y * padded as usize + k * tile_row;
                data[dst..dst + tile_row].copy_from_slice(&rgba[y * tile_row..(y + 1) * tile_row]);
            }
            regions.push((*level, *origin, *size, (base + k * tile_row) as u64, padded));
        }
        // Naechster Streifen wieder auf 512 Byte -- s. frueheren Kommentar, schadet nicht.
        data.resize(data.len().div_ceil(512) * 512, 0);
    }
    (!regions.is_empty()).then_some((data, regions))
}

pub(super) fn tile_copies(device: &wgpu::Device, buffer: &wgpu::Buffer, texture: &wgpu::Texture, regions: &[TileRegion]) -> wgpu::CommandBuffer {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("atlas upload") });
    for &(level, origin, size, offset, padded) in regions {
        encoder.copy_buffer_to_texture(
            wgpu::ImageCopyBuffer {
                buffer,
                layout: wgpu::ImageDataLayout { offset, bytes_per_row: Some(padded), rows_per_image: Some(size) },
            },
            wgpu::ImageCopyTexture {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d { x: origin.0, y: origin.1, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
    }
    encoder.finish()
}
