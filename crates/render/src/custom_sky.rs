// Himmel aus Resourcepacks: die GPU-Seite. Welche Ebenen wann mit welcher Helligkeit und
// Drehung zu sehen sind, entscheidet app::custom_sky; hier liegen nur die Bilder, je eine
// Pipeline pro Mischart und der Zeichendurchgang.
//
// Gemischt wird wie bei OptiFine im Gamma-Raum: der Durchgang schreibt in eine Ansicht des
// Bildes OHNE sRGB-Umrechnung, und die Bilder werden roh gelesen. In einer sRGB-Ansicht
// rechnete die Hardware jede Mischung linear, und "add"/"screen"/"multiply" saehen anders aus
// als in dem Spiel, fuer das die Packs gebaut sind.
//
// Die Bilder sind gross (bis 6144x4096, s. app::custom_sky::MAX_FACE), deshalb werden sie in
// Streifen ueber mehrere Frames hochgeladen (upload_step); gezeichnet wird eine Ebene erst,
// wenn ihr Bild vollstaendig ist.

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    Add,
    Subtract,
    Multiply,
    Dodge,
    Burn,
    Screen,
    Replace,
    Overlay,
    Alpha,
}

impl Blend {
    pub const ALL: [Blend; 9] =
        [Blend::Add, Blend::Subtract, Blend::Multiply, Blend::Dodge, Blend::Burn, Blend::Screen, Blend::Replace, Blend::Overlay, Blend::Alpha];

    pub fn from_name(name: &str) -> Option<Blend> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "add" => Blend::Add,
            "subtract" => Blend::Subtract,
            "multiply" => Blend::Multiply,
            "dodge" => Blend::Dodge,
            "burn" => Blend::Burn,
            "screen" => Blend::Screen,
            "replace" => Blend::Replace,
            "overlay" => Blend::Overlay,
            "alpha" => Blend::Alpha,
            _ => return None,
        })
    }

    // Womit die Bildfarbe multipliziert wird: die Helligkeit wirkt je nach Mischart ueber
    // Alpha (wo der Faktor SRC_ALPHA mitmischt) oder direkt auf die Farbe.
    fn color(self, brightness: f32) -> [f32; 4] {
        let b = brightness;
        match self {
            Blend::Add | Blend::Replace | Blend::Alpha => [1.0, 1.0, 1.0, b],
            Blend::Multiply => [b, b, b, b],
            Blend::Subtract | Blend::Dodge | Blend::Burn | Blend::Screen | Blend::Overlay => [b, b, b, 1.0],
        }
    }

    fn state(self) -> Option<wgpu::BlendState> {
        use wgpu::BlendFactor as F;
        let (src, dst) = match self {
            Blend::Add => (F::SrcAlpha, F::One),
            Blend::Subtract => (F::OneMinusDst, F::Zero),
            Blend::Multiply => (F::Dst, F::OneMinusSrcAlpha),
            Blend::Dodge => (F::One, F::One),
            Blend::Burn => (F::Zero, F::OneMinusSrc),
            Blend::Screen => (F::One, F::OneMinusSrc),
            Blend::Replace => return None,
            Blend::Overlay => (F::Dst, F::Src),
            Blend::Alpha => (F::SrcAlpha, F::OneMinusSrcAlpha),
        };
        let color = wgpu::BlendComponent { src_factor: src, dst_factor: dst, operation: wgpu::BlendOperation::Add };
        Some(wgpu::BlendState { color, alpha: wgpu::BlendComponent::REPLACE })
    }
}

// Eine Ebene, wie sie in diesem Frame gezeichnet wird.
#[derive(Clone, Copy, Debug)]
pub struct SkyLayerDraw {
    pub texture: u32,
    pub blend: Blend,
    // Welt-Richtung -> Raum der Ebene (s. custom_sky.wgsl).
    pub to_local: Mat3,
    pub brightness: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerUniform {
    inv_view_proj: [[f32; 4]; 4],
    // mat3x3 in WGSL: drei Spalten zu je 16 Byte.
    to_local: [[f32; 4]; 3],
    color: [f32; 4],
    texel: [f32; 4],
    _pad: [f32; 28],
}

const UNIFORM_STRIDE: u64 = std::mem::size_of::<LayerUniform>() as u64;
const MAX_LAYERS: usize = 64;
// Hochladen pro Frame: ~12 MB, ein 6144er-Bild also in gut 8 Frames.
const UPLOAD_BYTES_PER_STEP: usize = 12 << 20;

struct SkyTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
    // Noch hochzuladende Pixel und die erste fehlende Zeile; None = fertig.
    pending: Option<(Vec<u8>, u32)>,
}

pub struct SkyRenderer {
    pipelines: Vec<(Blend, wgpu::RenderPipeline)>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    textures: std::collections::HashMap<u32, SkyTexture>,
    next_id: u32,
}

impl SkyRenderer {
    // `format`: das Format der Ansicht, in die gezeichnet wird (ohne sRGB, s. Kopfkommentar).
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("custom sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("custom_sky.wgsl").into()),
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("custom sky texture"),
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
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("custom sky layer"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_STRIDE),
                },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("custom sky layers"),
            size: UNIFORM_STRIDE * MAX_LAYERS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("custom sky layers"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &uniforms, offset: 0, size: wgpu::BufferSize::new(UNIFORM_STRIDE) }),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("custom sky"),
            bind_group_layouts: &[&texture_layout, &uniform_layout],
            push_constant_ranges: &[],
        });
        let pipelines = Blend::ALL
            .iter()
            .map(|&blend| {
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("custom sky"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: "vs_sky",
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: "fs_sky",
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState { format, blend: blend.state(), write_mask: wgpu::ColorWrites::COLOR })],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
                (blend, pipeline)
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("custom sky"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { pipelines, texture_layout, sampler, uniforms, uniform_group, textures: Default::default(), next_id: 1 }
    }

    // Ein Bild anlegen; hochgeladen wird es nach und nach mit upload_step.
    pub fn add_texture(&mut self, device: &wgpu::Device, width: u32, height: u32, rgba: Vec<u8>) -> u32 {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("custom sky"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("custom sky"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let id = self.next_id;
        self.next_id += 1;
        self.textures.insert(id, SkyTexture { texture, bind_group, width, height, pending: Some((rgba, 0)) });
        id
    }

    pub fn remove_texture(&mut self, id: u32) {
        self.textures.remove(&id);
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }

    pub fn is_ready(&self, id: u32) -> bool {
        self.textures.get(&id).is_some_and(|t| t.pending.is_none())
    }

    // Den naechsten Streifen eines noch unvollstaendigen Bildes hochladen.
    pub fn upload_step(&mut self, queue: &wgpu::Queue) {
        let Some(tex) = self.textures.values_mut().find(|t| t.pending.is_some()) else { return };
        let (rgba, row) = tex.pending.as_mut().unwrap();
        let bytes_per_row = tex.width * 4;
        let rows = ((UPLOAD_BYTES_PER_STEP / bytes_per_row as usize) as u32).clamp(1, tex.height - *row);
        let start = (*row * bytes_per_row) as usize;
        let end = start + (rows * bytes_per_row) as usize;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: *row, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba[start..end],
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(rows) },
            wgpu::Extent3d { width: tex.width, height: rows, depth_or_array_layers: 1 },
        );
        *row += rows;
        if *row >= tex.height {
            tex.pending = None;
        }
    }

    // Die Ebenen in den laufenden Durchgang zeichnen, in der gegebenen Reihenfolge. `view_proj`
    // ist Projektion mal BLICKDREHUNG (ohne Verschiebung): der Himmel haengt an der Richtung,
    // nicht am Ort.
    pub fn draw<'a>(&'a self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'a>, view_proj: Mat4, layers: &[SkyLayerDraw]) {
        let inv = view_proj.inverse().to_cols_array_2d();
        let layers: Vec<&SkyLayerDraw> = layers.iter().filter(|l| self.is_ready(l.texture)).take(MAX_LAYERS).collect();
        if layers.is_empty() {
            return;
        }
        let mut data = vec![LayerUniform::zeroed(); layers.len()];
        for (u, l) in data.iter_mut().zip(&layers) {
            let tex = &self.textures[&l.texture];
            let m = l.to_local.to_cols_array_2d();
            u.inv_view_proj = inv;
            u.to_local = [[m[0][0], m[0][1], m[0][2], 0.0], [m[1][0], m[1][1], m[1][2], 0.0], [m[2][0], m[2][1], m[2][2], 0.0]];
            u.color = l.blend.color(l.brightness);
            u.texel = [0.5 / (tex.width as f32 / 3.0), 0.5 / (tex.height as f32 / 2.0), 0.0, 0.0];
        }
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&data));
        for (i, l) in layers.iter().enumerate() {
            let Some((_, pipeline)) = self.pipelines.iter().find(|(b, _)| *b == l.blend) else { continue };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.textures[&l.texture].bind_group, &[]);
            pass.set_bind_group(1, &self.uniform_group, &[(i as u64 * UNIFORM_STRIDE) as u32]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_passt_zum_versatz() {
        // Dynamische Versaetze muessen auf 256 Byte liegen.
        assert_eq!(UNIFORM_STRIDE, 256);
    }
}
