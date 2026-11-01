// Sortieren, Sichtbarkeit, Index- und Indirect-Buffer, Atlas-Bind-Groups, Umriss-Komplement und
// Tiefenpuffer.

use super::*;

// Sortiert Spaltenschluessel nach Abstand zu `center` AUFSTEIGEND (s. draw_order).
//
// Quadrierter Abstand statt Wurzel: spart die sqrt und ordnet identisch (beide Seiten >= 0).
// In i64 gerechnet, nicht i32: Chunk-Koordinaten reichen bis ~1.9 Mio (Weltgrenze 30 Mio
// Bloecke / 16), ein dx von wenigen Millionen quadriert liegt weit ueber i32::MAX. Im
// Release haette das still falsch herum sortiert, im Debug-Build panisch abgebrochen. Tritt
// nur auf, wenn weit entfernte Spalten noch geladen sind (Teleport), ist dann aber sicher.
//
// (cx, cz) als Tiebreaker, damit gleich weit entfernte Spalten -- ein ganzer Ring hat
// denselben Abstand -- eine STABILE Reihenfolge bekommen. Ohne das haenge sie wieder an der
// HashMap-Iterationsreihenfolge, und genau die Instabilitaet soll hier ja verschwinden.
pub(super) fn sortiere_nach_abstand(ziel: &mut [(i32, i32)], center: (i32, i32)) {
    ziel.sort_unstable_by_key(|&(cx, cz)| {
        let dx = i64::from(cx) - i64::from(center.0);
        let dz = i64::from(cz) - i64::from(center.1);
        (dx * dx + dz * dz, cx, cz)
    });
}


// Liegt die Spalte (ganze Weltspanne in y) im Sichtkegel? AABB AUGENRELATIV, in f64
// gerechnet: der Sichtkegel kommt aus einer View-Matrix, die die Kamera im Ursprung sieht (s.
// Camera::render_view_position). Mit absoluten Weltkoordinaten waere der Test gegen den
// falschen Raum gelaufen -- und bei grossen Koordinaten obendrein in f32 zerfallen.
// Die Bits aus `mask`, deren Section (mit einem Block Rand) im Sichtkegel liegt.
pub(super) fn sections_in_frustum((cx, cz): (i32, i32), min_y: i32, mask: u64, frustum: &Frustum, eye: DVec3) -> u64 {
    let (x0, z0) = (f64::from(cx * SECTION_SIZE - 1), f64::from(cz * SECTION_SIZE - 1));
    let rand = f64::from(SECTION_SIZE + 2);
    let mut rest = mask;
    let mut drin = 0;
    while rest != 0 {
        let s = rest.trailing_zeros();
        rest &= rest - 1;
        let y0 = f64::from(min_y + s as i32 * SECTION_SIZE - 1);
        let min = (DVec3::new(x0, y0, z0) - eye).as_vec3();
        let max = (DVec3::new(x0 + rand, y0 + rand, z0 + rand) - eye).as_vec3();
        if frustum.intersects_aabb(min, max) {
            drin |= 1 << s;
        }
    }
    drin
}

pub(super) fn column_visible((cx, cz): (i32, i32), frustum: &Frustum, eye: DVec3) -> bool {
    let min = (DVec3::new(f64::from(cx * SECTION_SIZE), f64::from(WORLD_CULL_MIN_Y), f64::from(cz * SECTION_SIZE)) - eye).as_vec3();
    let max =
        (DVec3::new(f64::from(cx * SECTION_SIZE + SECTION_SIZE), f64::from(WORLD_CULL_MAX_Y), f64::from(cz * SECTION_SIZE + SECTION_SIZE)) - eye)
            .as_vec3();
    frustum.intersects_aabb(min, max)
}

pub(super) fn create_indirect_buffer(device: &wgpu::Device, entries: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terrain indirect draws"),
        size: wgpu::BufferAddress::from(entries) * std::mem::size_of::<IndexedIndirect>() as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

// Der gemeinsame Flaechen-Index-Puffer (s. BlockRenderer::terrain_pipeline): fuer `quads`
// Flaechen je 0,1,2, 0,2,3 auf die vier Vertices der Flaeche.
pub(super) fn create_quad_index_buffer(device: &wgpu::Device, queue: &wgpu::Queue, quads: u32) -> wgpu::Buffer {
    let indices: Vec<u32> = (0..quads).flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i)).collect();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("quad index buffer"),
        size: (indices.len() * 4) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&indices));
    buffer
}


// Mittelpunkt eines Quads -- Sortierschluessel fuer die Reihenfolge durchsichtiger Flaechen.
// Ordnet die Quads in `verts` von HINTEN nach VORN (fernstes zuerst) -- allokationsfrei ueber
// die uebergebenen Arbeitspuffer. Sortiert werden nur (Abstand, Quad-Index)-Paare (8 Byte)
// statt Slices oder gar ganzer Quads (192 Byte).
// Ein Vertex-Format, dessen Liste aus gleich grossen Flaechen besteht (sortiere_quads).
pub(super) trait QuadVertex: Copy {
    const PER_QUAD: usize;
    fn pos(&self) -> [f32; 3];
}

impl QuadVertex for BlockVertex {
    const PER_QUAD: usize = VERTS_PER_QUAD;
    fn pos(&self) -> [f32; 3] {
        self.position
    }
}

impl QuadVertex for TerrainVertex {
    const PER_QUAD: usize = 4;
    fn pos(&self) -> [f32; 3] {
        self.position()
    }
}

pub(super) fn sortiere_quads<V: QuadVertex>(verts: &mut Vec<V>, camera: Vec3, keys: &mut Vec<(f32, u32)>, scratch: &mut Vec<V>) {
    keys.clear();
    keys.extend(verts.chunks_exact(V::PER_QUAD).enumerate().map(|(i, q)| (quad_center(q).distance_squared(camera), i as u32)));
    // total_cmp ist NaN-sicher; der Index als zweites Kriterium macht das Ergebnis bei exakt
    // gleichen Abstaenden eindeutig -- sonst koennte sort_unstable zwei deckungsgleiche Quads
    // von Frame zu Frame vertauschen und sichtbar flimmern lassen.
    keys.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    scratch.clear();
    scratch.reserve(verts.len());
    for &(_, i) in keys.iter() {
        let start = i as usize * V::PER_QUAD;
        scratch.extend_from_slice(&verts[start..start + V::PER_QUAD]);
    }
    // Tauschen statt kopieren: `verts` bekommt die sortierte Fassung, der Arbeitspuffer behaelt
    // die alte Allokation und ist beim naechsten Aufruf schon gross genug.
    std::mem::swap(verts, scratch);
}

pub(super) fn quad_center<V: QuadVertex>(quad: &[V]) -> Vec3 {
    let mut c = Vec3::ZERO;
    for v in quad {
        c += Vec3::from(v.pos());
    }
    c / quad.len() as f32
}

// Laedt die Atlas-Mipmap-Kette als GPU-Textur hoch und baut die zugehoerige Bindgroup
// (Textur + Sampler). Die Atlas-PNG-Texel sind bereits sRGB-kodiert (Vanilla-Assets) --
// deshalb Rgba8UnormSrgb, damit die GPU beim Sampeln korrekt nach linear konvertiert und die
// Farben nicht ausgewaschen wirken.
//
// `mips[0]` ist die volle Aufloesung, jede weitere Stufe (aus assets::BlockAtlas, JE KACHEL
// EINZELN verkleinert -- kein Atlas-Bleeding, s. dortigen Kommentar) halb so gross.
pub(super) fn create_glint_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    item: &wgpu::TextureView,
    entity: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glint bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(item) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(entity) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: uniform.as_entire_binding() },
        ],
    })
}

pub(super) fn create_atlas_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("atlas bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

// Textur + Bindgroup GEGEN EINEN BESTEHENDEN Layout bauen (s. atlas_bind_group_layout-
// Feldkommentar) -- getrennt von create_atlas_bind_group_layout, damit ein Atlas-Rebuild
// (Skin-Tab "Laden") den Layout nicht anfassen muss, an den die Pipelines gebunden sind.
pub(super) fn create_atlas_texture_and_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mips: &[Vec<u8>],
    size: u32,
    layout: &wgpu::BindGroupLayout,
) -> (wgpu::BindGroup, wgpu::BindGroup, wgpu::Texture) {
    let mip_level_count = mips.len() as u32;
    let extent = wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("block atlas"),
        size: extent,
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        // Die Gamma-Sicht (Rgba8Unorm) brauchen Vanillas Core-Shader aus Resourcepacks: sie
        // rechnen mit den Texeln, wie sie in der Datei stehen (s. vanilla_draw).
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    for (level, level_rgba) in mips.iter().enumerate() {
        let level_size = (size >> level).max(1);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            level_rgba,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * level_size), rows_per_image: Some(level_size) },
            wgpu::Extent3d { width: level_size, height: level_size, depth_or_array_layers: 1 },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // Nearest INNERHALB einer Mip-Stufe (scharfe Minecraft-Pixel aus der Naehe, kein
    // Verwaschen einzelner Bloecke UND kein Bleeding an Kachelgrenzen, s. dortigen
    // Atlas-Bleeding-Kommentar), aber Linear ZWISCHEN zwei Mip-Stufen -- genau die
    // "GL_NEAREST_MIPMAP_LINEAR"-Kombination, die aus der Distanz weich zwischen den
    // Mip-Stufen ueberblendet, ohne aus der Naehe zu verschwimmen.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("block atlas sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    // Fuer die Einstellung Mipmapping aus: dieselbe Textur, auf Stufe 0 festgehalten.
    let sampler_flat = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("block atlas sampler ohne mips"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        lod_max_clamp: 0.0,
        ..Default::default()
    });
    let gruppe = |sampler: &wgpu::Sampler| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    };
    (gruppe(&sampler), gruppe(&sampler_flat), texture)
}

// Blockumriss in der Gegenfarbe des Bildes darunter (wie das Fadenkreuz im Modus
// Komplementaer). Ein Shader kann sein eigenes Ziel nicht lesen; deshalb nach dem Welt-Durchgang
// eine Kopie des Bildes und ein eigener kleiner Durchgang, der aus ihr liest. Getestet wird
// gegen die Tiefe der Welt (die dafuer stehen bleibt, s. depth_attachment) -- so verschwindet
// der Umriss weiter hinter Waenden. Die Kanten sind dieselben wie sonst (vs_hitbox).
pub(super) const OUTLINE_COMPLEMENT_CAPACITY: usize = 16_384;

pub(super) struct OutlineComplement {
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) buffer: wgpu::Buffer,
    pub(super) count: u32,
    pub(super) format: wgpu::TextureFormat,
    // Die Bildkopie samt Bindung, je Groesse einmal angelegt.
    pub(super) copy: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
}

impl OutlineComplement {
    pub(super) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        camera_layout: &wgpu::BindGroupLayout,
        vertex_layout: wgpu::VertexBufferLayout<'_>,
        format: wgpu::TextureFormat,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline complement layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline complement pipeline layout"),
            bind_group_layouts: &[camera_layout, &layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline complement pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_hitbox",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_hitbox_complement",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline complement vertex buffer"),
            size: (std::mem::size_of::<HitboxVertex>() * OUTLINE_COMPLEMENT_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, layout, buffer, count: 0, format, copy: None }
    }

    pub(super) fn set_vertices(&mut self, queue: &wgpu::Queue, vertices: &[HitboxVertex]) {
        let n = vertices.len().min(OUTLINE_COMPLEMENT_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.count = n as u32;
    }

    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::Texture,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        camera: &wgpu::BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        let size = (color.width(), color.height());
        if self.copy.as_ref().is_none_or(|(_, _, s)| *s != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("outline complement scene copy"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: color.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[self.format],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(self.format), ..Default::default() });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("outline complement bind group"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) }],
            });
            self.copy = Some((texture, bind_group, size));
        }
        let Some((kopie, bind_group, _)) = self.copy.as_ref() else { return };
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture { texture: color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyTexture { texture: kopie, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("outline complement pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        let bytes = u64::from(self.count) * std::mem::size_of::<HitboxVertex>() as u64;
        pass.set_vertex_buffer(0, self.buffer.slice(..bytes));
        pass.draw(0..self.count, 0..1);
    }
}

pub(super) fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

// Instanz-Daten einer Spalte (s. update_column_offsets): Spaltenecke auf Hoehe min_y (der
// Bezugspunkt der Vertex-Positionen, s. world::TerrainVertex) minus abgerundetes Auge -- ganze
// Bloecke, in f32 exakt --, dahinter der Nachkomma-Anteil des Auges.
pub(super) fn column_offset(key: (i32, i32), min_y: i32, eye: DVec3) -> [f32; 6] {
    let auge_ganz = eye.floor();
    let base = DVec3::new(f64::from(key.0 * SECTION_SIZE), f64::from(min_y), f64::from(key.1 * SECTION_SIZE));
    let off = (base - auge_ganz).as_vec3();
    let rest = (eye - auge_ganz).as_vec3();
    [off.x, off.y, off.z, rest.x, rest.y, rest.z]
}
