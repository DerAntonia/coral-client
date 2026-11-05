// Unschaerfe der Ego-Hand samt der Items darin (Motion Blur > Hand Blur). Statt Bilder zu
// speichern, wird die Hand in vielen Zwischenlagen gezeichnet: jede Ecke zwischen ihrer Lage im
// vorigen Bild und der jetzigen (s. block.wgsl::vs_hand_blur), wie die Teilbilder, die smoothie
// aus einer schnellen Aufnahme mischt. Die Teilbilder werden mit der gewaehlten Kurve gewichtet
// aufsummiert und dann ueber die Welt gelegt.
//
// Damit es durchgehend weich aussieht und nicht wie ein paar uebereinandergelegte Bilder,
// richtet sich die Zahl der Teilbilder nach der Strecke auf dem Schirm: etwa eins je Pixel,
// hoechstens MAX_SAMPLES. Alle in EINEM Durchgang: jedes Teilbild hat seinen eigenen
// Tiefenbereich, spaetere davor -- erst die Tiefe, dann die Farbe gegen diese Tiefe (was
// dahinter liegt, faellt durch). So genuegt ein einziges Leeren der Tiefe.
//
// Vergleichbar wird die Ecke nur, wenn die Hand gleich viele Ecken hat wie im vorigen Bild
// (dasselbe Item); sonst wird sie scharf gezeichnet.

use std::time::Instant;

use world::BlockVertex;

// So viele Teilbilder hoechstens (s. block.wgsl::HandBlurParams).
pub const MAX_SAMPLES: usize = 64;
// Pixel Strecke je Teilbild.
const PX_PER_SAMPLE: f32 = 1.0;
// Darunter bleibt die Hand scharf.
const MIN_SHIFT_PX: f32 = 0.5;
// Laengere Pause zwischen zwei Bildern (Menue): nichts ueber die Luecke verwischen.
const MAX_PAUSE_S: f64 = 0.25;
pub const ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// Gewichtung der Teilbilder ueber die Zeit, wie die "weighting" in smoothie. Alle schauen nur
// zurueck: in die Zukunft laesst sich nicht rechnen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Curve {
    Equal,
    Ascending,
    Gauss,
}

impl Curve {
    #[must_use]
    pub const fn from_index(i: u8) -> Self {
        match i {
            0 => Self::Equal,
            2 => Self::Gauss,
            _ => Self::Ascending,
        }
    }

    // Gewicht bei s (0 = aeltestes Teilbild, 1 = jetzt).
    fn weight(self, s: f32) -> f32 {
        match self {
            Self::Equal => 1.0,
            Self::Ascending => s,
            Self::Gauss => (-0.5 * ((s - 0.5) / 0.2).powi(2)).exp(),
        }
    }
}

// Je Teilbild: Lage zwischen vorher (0) und jetzt (1) -- unter 0, wenn die Strecke laenger als
// ein Bild ist -- und Gewicht (Summe 1).
#[must_use]
pub fn samples(curve: Curve, span: f32, n: usize) -> Vec<(f32, f32)> {
    let n = n.clamp(1, MAX_SAMPLES);
    let mut out: Vec<(f32, f32)> = (0..n)
        .map(|k| {
            let s = (k as f32 + 0.5) / n as f32;
            (1.0 - span * (1.0 - s), curve.weight(s))
        })
        .collect();
    let summe: f32 = out.iter().map(|(_, w)| w).sum();
    if summe > 0.0 {
        for (_, w) in &mut out {
            *w /= summe;
        }
    }
    out
}

struct Accum {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

pub struct HandBlur {
    depth_pipeline: wgpu::RenderPipeline,
    color_pipeline: wgpu::RenderPipeline,
    glint_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    params_bind_group: wgpu::BindGroup,
    prev_buffer: wgpu::Buffer,
    prev_glint_buffer: wgpu::Buffer,
    // Lage der Ecken im vorigen und in diesem Bild (Sichtraum der Hand).
    prev: Vec<[f32; 3]>,
    cur: Vec<[f32; 3]>,
    prev_glint_ok: bool,
    cur_glint: Vec<[f32; 3]>,
    accum: Option<Accum>,
    last_at: Option<Instant>,
    // Geglaettete Zeit zwischen zwei Bildern (0 = noch keine), s. motion_blur.
    frame_s: f64,
    paused: bool,
}

impl HandBlur {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        camera_layout: &wgpu::BindGroupLayout,
        atlas_layout: &wgpu::BindGroupLayout,
        glint_layout: &wgpu::BindGroupLayout,
        surface_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        capacity: usize,
    ) -> Self {
        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hand blur params layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hand blur params"),
            size: (MAX_SAMPLES * 16) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hand blur params bind group"),
            layout: &params_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hand blur pipeline layout"),
            bind_group_layouts: &[camera_layout, atlas_layout, glint_layout, &params_layout],
            push_constant_ranges: &[],
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BlockVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
            ],
        };
        let prev_layout = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x3 }],
        };
        // Konstantes Gewicht je Teilbild (set_blend_constant), aufaddiert.
        let summe = |alpha: wgpu::BlendComponent| wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Constant, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha,
        };
        let pipeline = |label: &str, fs: &str, blend: Option<wgpu::BlendState>, depth_write: bool, compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: "vs_hand_blur",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[vertex_layout.clone(), prev_layout.clone()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: fs,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: ACCUM_FORMAT,
                        blend,
                        write_mask: if blend.is_some() { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::empty() },
                    })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: depth_write,
                    depth_compare: compare,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let depth_pipeline = pipeline("hand blur depth pipeline", "fs_hand_blur_depth", None, true, wgpu::CompareFunction::LessEqual);
        let alpha_summe = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Constant, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let color_pipeline = pipeline("hand blur color pipeline", "fs_main", Some(summe(alpha_summe)), false, wgpu::CompareFunction::LessEqual);
        // Der Glint addiert nur Farbe, die Deckung bleibt die des Items (wie BlendFunction.GLINT).
        let alpha_bleibt = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let glint_pipeline = pipeline("hand blur glint pipeline", "fs_glint", Some(summe(alpha_bleibt)), false, wgpu::CompareFunction::LessEqual);

        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hand blur composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hand_blur.wgsl").into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hand blur composite layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("hand blur composite pipeline layout"), bind_group_layouts: &[&composite_layout], push_constant_ranges: &[] });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hand blur composite pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState { module: &composite_shader, entry_point: "vs_fullscreen", compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: "fs_composite",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // Vormultipliert ueber das Bild; dessen Alpha bleibt.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
                        alpha: alpha_bleibt,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let prev_buffer_of = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (capacity * 12) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
        };
        Self {
            depth_pipeline,
            color_pipeline,
            glint_pipeline,
            composite_pipeline,
            composite_layout,
            params,
            params_bind_group,
            prev_buffer: prev_buffer_of("hand blur prev positions"),
            prev_glint_buffer: prev_buffer_of("hand blur prev glint positions"),
            prev: Vec::new(),
            cur: Vec::new(),
            prev_glint_ok: false,
            cur_glint: Vec::new(),
            accum: None,
            last_at: None,
            frame_s: 0.0,
            paused: true,
        }
    }

    // Die Hand dieses Bildes: was bisher galt, wird zur vorigen Lage.
    pub(crate) fn set_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let jetzt = Instant::now();
        let dt = self.last_at.map_or(f64::INFINITY, |t| jetzt.duration_since(t).as_secs_f64());
        self.last_at = Some(jetzt);
        self.paused = dt > MAX_PAUSE_S;
        if !self.paused {
            self.frame_s = if self.frame_s > 0.0 { self.frame_s * 0.9 + dt * 0.1 } else { dt };
        }
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.cur.clear();
        self.cur.extend(vertices.iter().map(|v| v.position));
        if self.prev.len() == self.cur.len() && !self.prev.is_empty() {
            queue.write_buffer(&self.prev_buffer, 0, bytemuck::cast_slice(&self.prev));
        }
    }

    pub(crate) fn set_glint_vertices(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let alt = std::mem::take(&mut self.cur_glint);
        self.cur_glint.extend(vertices.iter().map(|v| v.position));
        self.prev_glint_ok = alt.len() == self.cur_glint.len() && !alt.is_empty();
        if self.prev_glint_ok {
            queue.write_buffer(&self.prev_glint_buffer, 0, bytemuck::cast_slice(&alt));
        }
    }

    // Wie viele Bilder die Spur zurueckreicht: `strength` 1.0 = ein Monitorbild, wie bei der
    // Welt (s. motion_blur::MotionBlur::encode).
    fn span(&self, strength: f32, display_interval_s: f32) -> f32 {
        strength * (f64::from(display_interval_s) / self.frame_s.max(1e-4)).max(1.0) as f32
    }

    // Groesste Strecke einer Ecke auf dem Schirm in Pixeln ueber die ganze Spur. None = nicht
    // vergleichbar (anderes Item, Pause).
    fn shift_px(&self, proj: glam::Mat4, size: (u32, u32), span: f32) -> Option<f32> {
        if self.paused || self.prev.len() != self.cur.len() || self.cur.is_empty() {
            return None;
        }
        let auf_schirm = |p: [f32; 3]| {
            let c = proj * glam::Vec4::new(p[0], p[1], p[2], 1.0);
            (c.w > 1e-4).then(|| glam::Vec2::new(c.x / c.w * size.0 as f32 * 0.5, c.y / c.w * size.1 as f32 * 0.5))
        };
        let max = self.prev.iter().zip(&self.cur).filter_map(|(a, b)| Some(auf_schirm(*a)?.distance(auf_schirm(*b)?))).fold(0.0, f32::max);
        Some(max * span)
    }

    // Teilbilder fuer dieses Bild, oder None: dann scharf zeichnen.
    pub(crate) fn plan(&self, proj: glam::Mat4, size: (u32, u32), strength: f32, display_interval_s: f32, curve: Curve) -> Option<Vec<(f32, f32)>> {
        if strength <= 0.0 {
            return None;
        }
        let span = self.span(strength, display_interval_s);
        let px = self.shift_px(proj, size, span)?;
        if px < MIN_SHIFT_PX {
            return None;
        }
        let n = ((px / PX_PER_SAMPLE).ceil() as usize).clamp(2, MAX_SAMPLES);
        Some(samples(curve, span, n))
    }

    pub(crate) fn write_params(&self, queue: &wgpu::Queue, samples: &[(f32, f32)]) {
        let n = samples.len() as f32;
        let daten: Vec<[f32; 4]> = samples
            .iter()
            .enumerate()
            // Teilbild k in den Tiefenbereich [(n-1-k)/n, (n-k)/n]: spaetere davor.
            .map(|(k, &(t, _))| [t, 1.0 / n, (n - 1.0 - k as f32) / n, 0.0])
            .collect();
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&daten));
    }

    pub(crate) fn ensure_accum(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.accum.as_ref().is_some_and(|a| a.size == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hand blur accum"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ACCUM_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hand blur composite bind group"),
            layout: &self.composite_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
        });
        self.accum = Some(Accum { view, bind_group, size });
    }

    pub(crate) fn accum_view(&self) -> Option<&wgpu::TextureView> {
        self.accum.as_ref().map(|a| &a.view)
    }

    // Alle Teilbilder: erst die Tiefe, dann die Farbe (und der Glint) mit ihrem Gewicht.
    // Kamera, Atlas und Glint (Gruppen 0..2) hat der Aufrufer gebunden.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        samples: &[(f32, f32)],
        hand: &'a wgpu::Buffer,
        count: u32,
        glint: Option<(&'a wgpu::Buffer, u32)>,
    ) {
        let bytes = u64::from(count) * std::mem::size_of::<BlockVertex>() as u64;
        let glint = glint.filter(|(_, n)| *n > 0 && self.prev_glint_ok && *n as usize == self.cur_glint.len());
        pass.set_bind_group(3, &self.params_bind_group, &[]);
        for (k, &(_, w)) in samples.iter().enumerate() {
            let k = k as u32;
            pass.set_vertex_buffer(0, hand.slice(..bytes));
            pass.set_vertex_buffer(1, self.prev_buffer.slice(..u64::from(count) * 12));
            pass.set_pipeline(&self.depth_pipeline);
            pass.draw(0..count, k..k + 1);
            let w = f64::from(w);
            pass.set_blend_constant(wgpu::Color { r: w, g: w, b: w, a: w });
            pass.set_pipeline(&self.color_pipeline);
            pass.draw(0..count, k..k + 1);
            if let Some((buffer, n)) = glint {
                pass.set_vertex_buffer(0, buffer.slice(..u64::from(n) * std::mem::size_of::<BlockVertex>() as u64));
                pass.set_vertex_buffer(1, self.prev_glint_buffer.slice(..u64::from(n) * 12));
                pass.set_pipeline(&self.glint_pipeline);
                pass.draw(0..n, k..k + 1);
            }
        }
    }

    // Die Summe ueber das Bild legen.
    pub(crate) fn composite<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let Some(accum) = self.accum.as_ref() else { return };
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &accum.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gewichte_summieren_zu_eins_und_reichen_bis_jetzt() {
        for curve in [Curve::Equal, Curve::Ascending, Curve::Gauss] {
            let s = samples(curve, 1.0, 16);
            assert_eq!(s.len(), 16);
            assert!((s.iter().map(|(_, w)| w).sum::<f32>() - 1.0).abs() < 1e-5);
            assert!(s.first().unwrap().0 < 0.1 && s.last().unwrap().0 > 0.9);
        }
    }

    #[test]
    fn aufsteigend_zaehlt_das_juengste_am_meisten() {
        let s = samples(Curve::Ascending, 1.0, 8);
        assert!(s.windows(2).all(|p| p[1].1 > p[0].1));
    }

    // Laengere Spur als ein Bild: die aeltesten Teilbilder liegen vor der vorigen Lage.
    #[test]
    fn spur_ueber_ein_bild_hinaus() {
        let s = samples(Curve::Equal, 2.0, 4);
        assert!(s[0].0 < 0.0 && s[3].0 > 0.5);
    }

    #[test]
    fn composite_shader_ist_gueltig() {
        let quelle = include_str!("hand_blur.wgsl");
        let modul = naga::front::wgsl::parse_str(quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(quelle)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&modul).expect("hand_blur.wgsl ungueltig");
    }
}
