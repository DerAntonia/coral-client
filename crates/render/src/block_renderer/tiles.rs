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

// Animierte Kacheln GEZEICHNET statt kopiert, fuer den Animations-Thread der App (s.
// block_anim.rs): je Mip-Stufe EIN Zeichenaufruf mit einem Viereck je Kachel, das seine Pixel
// aus einem Puffer liest. Gleiches Ergebnis wie die Kopien, Byte fuer Byte.
//
// Warum: je Tick aendern sich ~290 Kacheln, mit allen Mip-Stufen ~1750 Kopierbefehle
// (gemessen 2026-11-04, Vanilla 1.21.11, 32er-Kacheln). Jeder copy_buffer_to_texture ist im
// Radeon-Treiber ein eigener kleiner Meta-Durchgang; zusammen 12,8 % aller CPU-Samples des
// Clients (perf, Release-Build). Zeichnen sind ~6 Durchgaenge je Tick.
//
// Aufgezeichnet, nicht abgeschickt: der Thread schickt den Befehlspuffer zu einem selbst
// gewaehlten Zeitpunkt ab, damit die Arbeit nicht mitten in einen Frame auf der GPU faellt (s.
// block_anim::submit_window). Ohne write_buffer: das liefe ueber die Warteschlange und kaeme mit
// dem naechsten Submit irgendeines Threads.
pub struct TileWriter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    // Je Mip-Stufe die Rohsicht als Zeichenziel und ihre Kantenlaenge.
    levels: Vec<(wgpu::TextureView, u32)>,
}

// Eine Kachel im Zeichenaufruf (s. tile_write.wgsl).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TileInstance {
    // Ursprung x, y in der Stufe, Kantenlaenge, erstes Pixel im Puffer.
    ziel: [u32; 4],
    // Kantenlaenge der ganzen Stufe.
    stufe: u32,
}

impl TileWriter {
    // Fuer genau diesen Atlas (er braucht RENDER_ATTACHMENT, s. buffers.rs) -- nach einem
    // Atlas-Neubau einen neuen anlegen.
    #[must_use]
    pub fn new(device: &wgpu::Device, texture: &wgpu::Texture) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("atlas tile write shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tile_write.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas tile write layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("atlas tile write pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("atlas tile write pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_tile",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TileInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Uint32x4, 1 => Uint32],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_tile",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let size = texture.width();
        let levels = (0..texture.mip_level_count())
            .map(|level| {
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("atlas tile write target"),
                    format: Some(wgpu::TextureFormat::Rgba8Unorm),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                });
                (view, (size >> level).max(1))
            })
            .collect();
        Self { pipeline, layout, levels }
    }

    // Je Kachel ihre Mip-Stufen (`[0]` = volle Groesse), Ursprung und Kantenlaenge wie bei
    // upload_tiles. None = nichts zu tun.
    #[must_use]
    pub fn encode(&self, device: &wgpu::Device, tiles: &[(&[Vec<u8>], (u32, u32), u32)]) -> Option<wgpu::CommandBuffer> {
        use wgpu::util::DeviceExt;
        let mut je_stufe: Vec<Vec<TileInstance>> = vec![Vec::new(); self.levels.len()];
        let mut data: Vec<u8> = Vec::new();
        for &(mips, origin, tile_size) in tiles {
            for (level, rgba) in mips.iter().enumerate().take(self.levels.len()) {
                let size = tile_size >> level;
                let bytes = (size * size * 4) as usize;
                if size == 0 || rgba.len() < bytes {
                    break;
                }
                let erstes = (data.len() / 4) as u32;
                data.extend_from_slice(&rgba[..bytes]);
                je_stufe[level].push(TileInstance { ziel: [origin.0 >> level, origin.1 >> level, size, erstes], stufe: self.levels[level].1 });
            }
        }
        if data.is_empty() {
            return None;
        }
        let pixel = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("atlas tile write pixels"), contents: &data, usage: wgpu::BufferUsages::STORAGE });
        let alle: Vec<TileInstance> = je_stufe.iter().flatten().copied().collect();
        let instanzen = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("atlas tile write tiles"), contents: bytemuck::cast_slice(&alle), usage: wgpu::BufferUsages::VERTEX });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas tile write bind group"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: pixel.as_entire_binding() }],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("atlas tile write") });
        let stride = std::mem::size_of::<TileInstance>() as u64;
        let mut start = 0u64;
        for (liste, (view, _)) in je_stufe.iter().zip(&self.levels) {
            let n = liste.len() as u64;
            if n == 0 {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("atlas tile write pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_vertex_buffer(0, instanzen.slice(start * stride..(start + n) * stride));
            pass.draw(0..6, 0..n as u32);
            drop(pass);
            start += n;
        }
        Some(encoder.finish())
    }
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
