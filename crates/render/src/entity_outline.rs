// Leucht-Umriss (Glowing) wie in Vanilla: leuchtende Entities werden ein zweites Mal als
// flache Silhouette in ihrer Teamfarbe in ein eigenes Bild gezeichnet -- ohne Tiefe, darum
// sieht man sie durch Waende. Aus diesem Bild entsteht ein weicher Rand (Kanten, Unschaerfe in
// beiden Richtungen, s. entity_outline.wgsl), der nach der Welt und vor der Hand ueber das
// Bild gelegt wird.
//
// Die Mischung passiert in Gamma-Werten, wie in Vanilla: das Umriss-Bild haelt die Teamfarbe
// unveraendert, und das Weltbild wird ueber seine Ansicht ohne sRGB beschrieben.

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

pub struct EntityOutline {
    size: (u32, u32),
    // Silhouette und Zwischenbild; die Schritte wechseln zwischen beiden hin und her.
    silhouette: Target,
    swap: Target,
    layout: wgpu::BindGroupLayout,
    edges_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    // Je Schritt eigene Parameter: Kanten, Unschaerfe waagerecht, senkrecht.
    params: [wgpu::Buffer; 3],
    bind_groups: Option<[wgpu::BindGroup; 4]>,
}

fn target(device: &wgpu::Device, label: &str, (w, h): (u32, u32)) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    Target { texture, view }
}

impl EntityOutline {
    // `target_format`: das Format, in das der Umriss am Ende gemischt wird (die Ansicht des
    // Weltbilds ohne sRGB).
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("entity outline shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("entity_outline.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("entity outline bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("entity outline pipeline layout"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = |label: &str, entry: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs_fullscreen", compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: entry,
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        // Ueber das Bild nach der Deckkraft des Randes; die Deckkraft des Bildes bleibt.
        let ueberblenden = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let edges_pipeline = pipeline("entity outline edges", "fs_edges", FORMAT, None);
        let blur_pipeline = pipeline("entity outline blur", "fs_blur", FORMAT, None);
        let composite_pipeline = pipeline("entity outline composite", "fs_composite", target_format, Some(ueberblenden));
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("entity outline sampler"),
                mag_filter: filter,
                min_filter: filter,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            })
        };
        let params = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("entity outline params"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        Self {
            size: (0, 0),
            silhouette: target(device, "entity outline silhouette", (1, 1)),
            swap: target(device, "entity outline swap", (1, 1)),
            layout,
            edges_pipeline,
            blur_pipeline,
            composite_pipeline,
            nearest: sampler(wgpu::FilterMode::Nearest),
            linear: sampler(wgpu::FilterMode::Linear),
            params,
            bind_groups: None,
        }
    }

    // Bilder und Bindungen an die Bildgroesse anpassen (nur wenn sie sich geaendert hat).
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) {
        if self.size == size && self.bind_groups.is_some() {
            return;
        }
        self.size = size;
        self.silhouette = target(device, "entity outline silhouette", size);
        self.swap = target(device, "entity outline swap", size);
        let texel = [1.0 / size.0.max(1) as f32, 1.0 / size.1.max(1) as f32];
        for (buf, dir) in self.params.iter().zip([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]) {
            queue.write_buffer(buf, 0, bytemuck::cast_slice(&[dir[0], dir[1], texel[0], texel[1]]));
        }
        let gruppe = |quelle: &wgpu::TextureView, sampler: &wgpu::Sampler, params: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("entity outline bind group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(quelle) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                ],
            })
        };
        self.bind_groups = Some([
            // Kanten: Silhouette -> Zwischenbild, pixelgenau.
            gruppe(&self.silhouette.view, &self.nearest, &self.params[0]),
            // Unschaerfe waagerecht: Zwischenbild -> Silhouette, bilinear.
            gruppe(&self.swap.view, &self.linear, &self.params[1]),
            // Unschaerfe senkrecht: Silhouette -> Zwischenbild.
            gruppe(&self.silhouette.view, &self.linear, &self.params[2]),
            // Ueberblenden: Zwischenbild -> Weltbild.
            gruppe(&self.swap.view, &self.nearest, &self.params[0]),
        ]);
    }

    // Alle Schritte: Silhouetten aus `renderer`, Rand bauen, ueber `target` legen (die Ansicht
    // des Weltbilds ohne sRGB, Groesse `size`). Ohne leuchtende Entities passiert nichts.
    pub fn encode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, renderer: &crate::BlockRenderer, target: &wgpu::TextureView, size: (u32, u32)) {
        if !renderer.has_outlines() {
            return;
        }
        self.prepare(device, queue, size);
        {
            let mut pass = durchgang(encoder, "entity outline silhouettes", &self.silhouette.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            renderer.render_outline_silhouettes(&mut pass);
        }
        self.apply(encoder, target);
    }

    // Das Silhouetten-Bild (nach prepare), fuer Tests.
    pub fn silhouette_texture(&self) -> &wgpu::Texture {
        &self.silhouette.texture
    }

    // Die Schritte nach der Silhouette: Rand bauen und ueber `target` legen.
    pub fn apply(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(groups) = &self.bind_groups else { return };
        let leer = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let schritte: [(&str, &wgpu::RenderPipeline, &wgpu::BindGroup, &wgpu::TextureView, wgpu::LoadOp<wgpu::Color>); 4] = [
            ("entity outline edges", &self.edges_pipeline, &groups[0], &self.swap.view, leer),
            ("entity outline blur x", &self.blur_pipeline, &groups[1], &self.silhouette.view, leer),
            ("entity outline blur y", &self.blur_pipeline, &groups[2], &self.swap.view, leer),
            ("entity outline composite", &self.composite_pipeline, &groups[3], target, wgpu::LoadOp::Load),
        ];
        for (label, pipeline, group, view, load) in schritte {
            let mut pass = durchgang(encoder, label, view, load);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn durchgang<'e>(encoder: &'e mut wgpu::CommandEncoder, label: &str, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}
