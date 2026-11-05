// Schildtext im VRAM: jede Schildseite wird einmal gesetzt (s. App::push_sign_side,
// SignLayout) und als Glyphen-Vierecke in der Textebene hochgeladen. Je Bild kommt nur noch
// eine Matrix je sichtbarer Seite dazu; projiziert und den Rand in Bildschirmpixeln
// aufgezogen wird im Vertex-Shader (s. font.wgsl::vs_sign). Vorher setzte und projizierte die
// CPU jede Glyphe in jedem Bild und lud bis zu 4 MB Vertices hoch.

use super::*;

// Ein Viereck einer Schildseite (Glyph oder sein Rand), als Instanz gezeichnet: sechs Ecken
// aus vertex_index, s. font.wgsl::vs_sign.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SignInstance {
    // Glyph-Rechteck in Schriftpixeln der Textebene (x0, y0, x1, y1). Beim Rand das des
    // Glyphen; aufgezogen wird im Shader.
    pub(crate) rect: [f32; 4],
    // Atlas-UV des Glyphen (u0, v0, u1, v1).
    pub(crate) uv: [f32; 4],
    pub(crate) color: [f32; 4],
    // x = Art wie in font.wgsl (+ TEXT_MODE_PACK), y = Dicke der Kontur bzw. Versatz des
    // Vanilla-Schattens in Schriftpixeln.
    pub(crate) mode: [f32; 4],
    // Platz der Matrix dieser Seite in SignFrames::m.
    pub(crate) seite: u32,
}

// Anfangsgroessen; beides waechst bei Bedarf.
const SIGN_INSTANCES_START: u32 = 16_384;
const SIGN_SLOTS_START: u32 = 256;
// Kopf des Matrix-Puffers (font.wgsl::SignFrames::screen), davor die Matrizen.
const SIGN_FRAMES_HEADER: u64 = 16;

pub(crate) struct SignGpu {
    pub(crate) pipeline: wgpu::RenderPipeline,
    // Ohne Tiefentest, fuer den Shader-Pfad (dort im HUD-Durchgang).
    pub(crate) hud_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    instances: wgpu::Buffer,
    kapazitaet: u32,
    // Bis hierhin belegt. Neu gesetzte Seiten kommen dahinter; wer neu gesetzt oder
    // verworfen wird, hinterlaesst ein Loch. Ist hinten kein Platz mehr, wird alles dicht
    // neu gepackt (s. upload_signs).
    ende: u32,
    frames: wgpu::Buffer,
    slot_kapazitaet: u32,
    pub(crate) bind_group: wgpu::BindGroup,
    matrizen: Vec<[f32; 16]>,
    // Die Instanzbereiche der sichtbaren Seiten dieses Bildes, aneinanderstossende vereint.
    pub(crate) draws: Vec<std::ops::Range<u32>>,
}

fn instance_buffer(device: &wgpu::Device, kapazitaet: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sign text instances"),
        size: u64::from(kapazitaet) * std::mem::size_of::<SignInstance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn frames_buffer(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, slots: u32) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sign text frames"),
        size: SIGN_FRAMES_HEADER + u64::from(slots) * std::mem::size_of::<[f32; 16]>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sign text frames"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
    });
    (buffer, bind_group)
}

impl SignGpu {
    pub(crate) fn new(device: &wgpu::Device, font_layout: &wgpu::BindGroupLayout, shader: &wgpu::ShaderModule, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sign text frames"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sign text pipeline layout"),
            bind_group_layouts: &[font_layout, &layout],
            push_constant_ranges: &[],
        });
        let f32x4 = |offset: usize, location: u32| wgpu::VertexAttribute { offset: offset as u64, shader_location: location, format: wgpu::VertexFormat::Float32x4 };
        let attribute = [
            f32x4(0, 0),
            f32x4(16, 1),
            f32x4(32, 2),
            f32x4(48, 3),
            wgpu::VertexAttribute { offset: 64, shader_location: 4, format: wgpu::VertexFormat::Uint32 },
        ];
        // Mit Tiefentest gegen die Welt: verdeckt von allem, was davor steht, auch von
        // Entities; Tiefe des Welt-Durchgangs, selbst ohne Tiefe zu schreiben.
        let pipeline = |tiefe: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sign text pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: "vs_sign",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SignInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &attribute,
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: "fs_main",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: tiefe,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let welt = pipeline(Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }));
        let hud = pipeline(None);
        let (frames, bind_group) = frames_buffer(device, &layout, SIGN_SLOTS_START);
        Self {
            pipeline: welt,
            hud_pipeline: hud,
            layout,
            instances: instance_buffer(device, SIGN_INSTANCES_START),
            kapazitaet: SIGN_INSTANCES_START,
            ende: 0,
            frames,
            slot_kapazitaet: SIGN_SLOTS_START,
            bind_group,
            matrizen: Vec::new(),
            draws: Vec::new(),
        }
    }

    // Die Schildseiten dieses Bildes zeichnen (Pipeline und Schrift setzt der Aufrufer nicht).
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, hud: bool, schrift: &wgpu::BindGroup) {
        if self.draws.is_empty() {
            return;
        }
        pass.set_pipeline(if hud { &self.hud_pipeline } else { &self.pipeline });
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.set_bind_group(0, schrift, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        for r in &self.draws {
            pass.draw(0..VERTS_PER_QUAD as u32, r.clone());
        }
    }
}

impl GpuState {
    // Nach build_sign_texts: neu gesetzte Seiten hochladen, die Matrizen der sichtbaren
    // schreiben und ihre Bereiche als Draws sammeln.
    pub(crate) fn upload_signs(&self, l: &mut SignLayouts) {
        let mut g = self.sign_gpu.borrow_mut();
        let g = &mut *g;
        let mut packen = false;
        for (schluessel, _) in &l.sichtbar {
            let Some(e) = l.seiten.get_mut(schluessel) else { continue };
            if e.gpu.is_some() {
                continue;
            }
            let n = e.instanzen.len() as u32;
            if g.ende + n > g.kapazitaet {
                packen = true;
                break;
            }
            self.queue.write_buffer(&g.instances, u64::from(g.ende) * std::mem::size_of::<SignInstance>() as u64, bytemuck::cast_slice(&e.instanzen));
            e.gpu = Some(g.ende..g.ende + n);
            g.ende += n;
        }
        if packen {
            // Alles dicht von vorn, die Loecher fallen weg; bliebe danach weniger als ein
            // Drittel frei, doppelt so gross wie noetig (sonst gleich wieder packen).
            let gesamt: u32 = l.seiten.values().map(|e| e.instanzen.len() as u32).sum();
            if gesamt * 3 / 2 > g.kapazitaet {
                g.kapazitaet = (gesamt * 2).max(SIGN_INSTANCES_START);
                g.instances = instance_buffer(&self.device, g.kapazitaet);
            }
            let mut alle = Vec::with_capacity(gesamt as usize);
            for e in l.seiten.values_mut() {
                let anfang = alle.len() as u32;
                alle.extend_from_slice(&e.instanzen);
                e.gpu = Some(anfang..alle.len() as u32);
            }
            if !alle.is_empty() {
                self.queue.write_buffer(&g.instances, 0, bytemuck::cast_slice(&alle));
            }
            g.ende = gesamt;
        }

        if l.slots > g.slot_kapazitaet {
            g.slot_kapazitaet = (l.slots * 2).max(SIGN_SLOTS_START);
            let (frames, bind_group) = frames_buffer(&self.device, &g.layout, g.slot_kapazitaet);
            g.frames = frames;
            g.bind_group = bind_group;
        }
        g.matrizen.clear();
        g.matrizen.resize(l.slots as usize, [0.0; 16]);
        g.draws.clear();
        for (schluessel, clip) in &l.sichtbar {
            let Some(e) = l.seiten.get(schluessel) else { continue };
            g.matrizen[e.slot as usize] = clip.to_cols_array();
            if let Some(r) = e.gpu.clone().filter(|r| !r.is_empty()) {
                g.draws.push(r);
            }
        }
        if g.draws.is_empty() {
            return;
        }
        g.draws.sort_unstable_by_key(|r| r.start);
        let mut vereint: Vec<std::ops::Range<u32>> = Vec::with_capacity(g.draws.len());
        for r in g.draws.drain(..) {
            match vereint.last_mut() {
                Some(v) if v.end == r.start => v.end = r.end,
                _ => vereint.push(r),
            }
        }
        g.draws = vereint;
        let (sw, sh) = self.size();
        self.queue.write_buffer(&g.frames, 0, bytemuck::cast_slice(&[sw, sh, 0.0, 0.0]));
        self.queue.write_buffer(&g.frames, SIGN_FRAMES_HEADER, bytemuck::cast_slice(&g.matrizen));
    }
}
