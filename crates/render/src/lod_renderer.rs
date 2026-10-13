// GPU-Seite des Moduls Chunk-Saver: die Flaechen der Fernsicht-Knoten (chunk_saver::mesh) in
// grossen Speicherseiten, je Frame ein indirekter Mehrfach-Aufruf je Seite, eigene Tiefe.
//
// Gezeichnet wird VOR dem Gelaende in dasselbe Bild (Performance-Pfad): der Welt-Durchgang
// laedt danach das Bild statt es mit der Himmelsfarbe zu loeschen und zeichnet mit seiner
// eigenen Tiefe darueber. Das geht ohne Tiefenvergleich zwischen beiden, weil die Fernsicht
// nur ausserhalb der geladenen Chunks zeichnet (Spaltenmaske im Fragment-Shader): von der
// Kamera aus liegt dort alles hinter dem echten Gelaende.

use std::sync::Arc;

use chunk_saver::{NodeKey, NodeMesh, Quad};
use glam::{DVec3, Mat4};
use rustc_hash::FxHashMap as HashMap;

use crate::terrain_arena::FreeList;

const QUAD_BYTES: u64 = std::mem::size_of::<Quad>() as u64;
// Eine Seite fasst 64 MB, gut fuenf Millionen Flaechen.
const PAGE_BYTES: u64 = 64 << 20;
const MASK_SIZE: u32 = 256;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
// Naheebene der Fernsicht. Mit umgekehrtem Z und Fliesskomma-Tiefe ist die Genauigkeit in der
// Ferne trotzdem hoch.
const NEAR: f32 = 0.5;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    params: [f32; 4],
    eye: [f32; 4],
    eye_chunk: [i32; 4],
    // xy = Mitte des Sichtkreises relativ zum Chunk des Auges, z = Radius^2 in Chunks.
    clip: [i32; 4],
}

#[derive(Clone, Copy)]
struct Slot {
    page: u32,
    start: u32,
    len: u32,
}

struct Page {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    free: FreeList,
}

#[derive(Default)]
struct GpuMesh {
    opaque: Option<Slot>,
    translucent: Option<Slot>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawArgs {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

// Die Aufrufe eines Frames: je Lauf (Seite, erster Eintrag, Anzahl) ein Mehrfach-Aufruf.
#[derive(Default)]
struct Draws {
    runs: Vec<(u32, u32, u32)>,
    args: Vec<DrawArgs>,
}

impl Draws {
    fn push(&mut self, page: u32, arg: DrawArgs) {
        match self.runs.last_mut() {
            Some((p, _, n)) if *p == page => *n += 1,
            _ => self.runs.push((page, self.args.len() as u32, 1)),
        }
        self.args.push(arg);
    }
}

// Was der Frame zeichnen soll (s. LodRenderer::prepare).
pub struct LodFrame<'a> {
    pub draw: &'a [NodeKey],
    pub camera: &'a crate::Camera,
    pub aspect: f32,
    // Die Spaltenmaske pruefen (aus nur zum Messen).
    pub mask: bool,
    // Staerke der Eckabdunklung, 0..1 -- dieselbe wie beim nahen Gelaende ("Weiche
    // Beleuchtung", s. world::mesher::mesh_column_with_lighting).
    pub ao_strength: f32,
    // Sichtkreis: Chunk der Mitte und Radius in Chunks. Gezeichnet wird nur in Chunks, deren
    // Abstand zur Mitte (in ganzen Chunks) hoechstens der Radius ist -- dieselbe Regel, nach
    // der das echte Gelaende seine Spalten waehlt. Die Knoten selbst sind groesser als ein
    // Chunk und ragen sonst ueber den Kreis hinaus.
    pub clip: ((i32, i32), u32),
}

pub struct LodRenderer {
    opaque_pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    globals_layout: wgpu::BindGroupLayout,
    page_layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    state_rects: wgpu::Buffer,
    nodes: wgpu::Buffer,
    nodes_capacity: u32,
    mask_texture: wgpu::Texture,
    mask_view: wgpu::TextureView,
    mask: Vec<u8>,
    mask_dirty: bool,
    sampler: wgpu::Sampler,
    atlas: Option<(Arc<wgpu::Texture>, u32)>,
    bind_group: Option<wgpu::BindGroup>,
    pages: Vec<Page>,
    meshes: HashMap<NodeKey, GpuMesh>,
    opaque_draws: Draws,
    translucent_draws: Draws,
    indirect: wgpu::Buffer,
    indirect_capacity: u32,
    depth_view: wgpu::TextureView,
    multi_draw: bool,
    quads: u64,
}

fn storage_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("lod depth"),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_buffer(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

impl LodRenderer {
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lod.wgsl"), source: wgpu::ShaderSource::Wgsl(include_str!("lod.wgsl").into()) });
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lod globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: vf,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                storage_entry(3, wgpu::ShaderStages::VERTEX),
                storage_entry(4, wgpu::ShaderStages::VERTEX),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let page_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("lod quads"), entries: &[storage_entry(0, wgpu::ShaderStages::VERTEX)] });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("lod"), bind_group_layouts: &[&globals_layout, &page_layout], push_constant_ranges: &[] });
        let pipeline = |label: &str, entry: &str, blend: Option<wgpu::BlendState>, write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs_lod", buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: entry,
                    targets: &[Some(wgpu::ColorTargetState { format: color_format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: write,
                    depth_compare: wgpu::CompareFunction::Greater,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let opaque_pipeline = pipeline("lod opaque", "fs_lod", None, true);
        let translucent_pipeline = pipeline("lod translucent", "fs_lod_translucent", Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let mask_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lod column mask"),
            size: wgpu::Extent3d { width: MASK_SIZE, height: MASK_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mask_view = mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Die Mip-Stufe bestimmt der Shader selbst. Verkleinert wird bilinear (s. lod.wgsl
        // sample_face), vergroessert bleibt es scharf wie beim Gelaende.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lod atlas sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            opaque_pipeline,
            translucent_pipeline,
            globals_layout,
            page_layout,
            globals: create_buffer(device, "lod globals", std::mem::size_of::<Globals>() as u64, wgpu::BufferUsages::UNIFORM),
            state_rects: create_buffer(device, "lod state rects", 16, wgpu::BufferUsages::STORAGE),
            nodes: create_buffer(device, "lod nodes", 1024 * 16, wgpu::BufferUsages::STORAGE),
            nodes_capacity: 1024,
            mask_texture,
            mask_view,
            mask: vec![0; (MASK_SIZE * MASK_SIZE) as usize],
            mask_dirty: true,
            sampler,
            atlas: None,
            bind_group: None,
            pages: Vec::new(),
            meshes: HashMap::default(),
            opaque_draws: Draws::default(),
            translucent_draws: Draws::default(),
            indirect: create_buffer(device, "lod indirect", 4096 * 16, wgpu::BufferUsages::INDIRECT),
            indirect_capacity: 4096,
            depth_view: create_depth(device, width, height),
            multi_draw: device.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE),
            quads: 0,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.depth_view = create_depth(device, width, height);
    }

    // Atlas-Rechtecke je State und Seite: die ersten zwoelf Worte je State aus
    // world::voxel::VoxelClasses::state_info.
    pub fn set_state_rects(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, state_info: &[u32], words_per_state: usize) {
        let rects: Vec<u32> = state_info.chunks_exact(words_per_state).flat_map(|w| w[..12].iter().copied()).collect();
        self.state_rects = create_buffer(device, "lod state rects", (rects.len() * 4) as u64, wgpu::BufferUsages::STORAGE);
        queue.write_buffer(&self.state_rects, 0, bytemuck::cast_slice(&rects));
        self.bind_group = None;
    }

    // Der Block-Atlas samt Zahl seiner Mip-Stufen. Ein neuer Atlas (Resourcepack) baut die
    // Bindung neu.
    pub fn set_atlas(&mut self, atlas: Arc<wgpu::Texture>) {
        if self.atlas.as_ref().is_some_and(|(a, _)| Arc::ptr_eq(a, &atlas)) {
            return;
        }
        let mips = atlas.mip_level_count();
        self.atlas = Some((atlas, mips));
        self.bind_group = None;
    }

    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, key: NodeKey, mesh: &NodeMesh) {
        self.remove(key);
        let opaque = self.insert(device, queue, &mesh.opaque);
        let translucent = self.insert(device, queue, &mesh.translucent);
        self.meshes.insert(key, GpuMesh { opaque, translucent });
    }

    fn insert(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, quads: &[Quad]) -> Option<Slot> {
        let len = quads.len() as u32;
        let page_quads = (PAGE_BYTES / QUAD_BYTES) as u32;
        if len == 0 || len > page_quads {
            return None;
        }
        let found = self.pages.iter_mut().enumerate().find_map(|(i, p)| p.free.take(len).map(|start| (i, start)));
        let (page, start) = match found {
            Some(f) => f,
            None => {
                let buffer = create_buffer(device, "lod quad page", PAGE_BYTES, wgpu::BufferUsages::STORAGE);
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("lod quad page"),
                    layout: &self.page_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
                });
                let mut free = FreeList::new(page_quads);
                let start = free.take(len)?;
                self.pages.push(Page { buffer, bind_group, free });
                (self.pages.len() - 1, start)
            }
        };
        queue.write_buffer(&self.pages[page].buffer, start as u64 * QUAD_BYTES, bytemuck::cast_slice(quads));
        self.quads += len as u64;
        Some(Slot { page: page as u32, start, len })
    }

    pub fn remove(&mut self, key: NodeKey) {
        let Some(m) = self.meshes.remove(&key) else { return };
        for slot in [m.opaque, m.translucent].into_iter().flatten() {
            if let Some(p) = self.pages.get_mut(slot.page as usize) {
                p.free.give(slot.start, slot.len);
            }
            self.quads -= slot.len as u64;
        }
    }

    pub fn clear(&mut self) {
        let keys: Vec<NodeKey> = self.meshes.keys().copied().collect();
        for k in keys {
            self.remove(k);
        }
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    // Flaechen auf der GPU (je 16 Byte, s. chunk_saver::mesh::Quad).
    pub fn quad_count(&self) -> u64 {
        self.quads
    }

    // Zeichnet das echte Gelaende diesen Chunk? Dann laesst die Fernsicht ihn aus.
    pub fn set_column_covered(&mut self, cx: i32, cz: i32, covered: bool) {
        let i = ((cz & 255) as u32 * MASK_SIZE + (cx & 255) as u32) as usize;
        if (self.mask[i] != 0) != covered {
            self.mask[i] = covered as u8;
            self.mask_dirty = true;
        }
    }

    pub fn clear_columns(&mut self) {
        self.mask.fill(0);
        self.mask_dirty = true;
    }

    pub fn depth_attachment(&self) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.depth_view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
            stencil_ops: None,
        }
    }

    pub fn has_draws(&self) -> bool {
        !self.opaque_draws.args.is_empty() || !self.translucent_draws.args.is_empty()
    }

    // Vor dem Render-Pass: Knotenversaetze relativ zum Auge, Aufrufe, Uniform, Maske.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &LodFrame) {
        self.opaque_draws = Draws::default();
        self.translucent_draws = Draws::default();
        let Some((atlas, mips)) = self.atlas.clone() else { return };
        let eye = frame.camera.position;
        // Nur was im Sichtkegel liegt (ohne Fernebene). Deckendes von vorn nach hinten (frueher
        // Tiefentest), Durchscheinendes von hinten nach vorn.
        let frustum = crate::Frustum::from_view_proj(frame.camera.cull_view_proj_matrix(frame.aspect)).without_far();
        let mut order: Vec<(f64, NodeKey)> = frame
            .draw
            .iter()
            .filter(|k| self.meshes.contains_key(k))
            .filter_map(|k| {
                let s = k.size() as f64;
                let o = k.origin();
                let min = DVec3::new(o[0] as f64, o[1] as f64, o[2] as f64) - eye;
                let max = min + DVec3::splat(s);
                if !frustum.intersects_aabb(min.as_vec3(), max.as_vec3()) {
                    return None;
                }
                Some(((min + max).distance_squared(DVec3::ZERO), *k))
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut nodes: Vec<[f32; 4]> = Vec::with_capacity(order.len());
        for (i, (_, k)) in order.iter().enumerate() {
            let o = k.origin();
            let rel = DVec3::new(o[0] as f64, o[1] as f64, o[2] as f64) - eye;
            nodes.push([rel.x as f32, rel.y as f32, rel.z as f32, k.voxel_size() as f32]);
            if let Some(s) = self.meshes[k].opaque {
                self.opaque_draws.push(s.page, DrawArgs { vertex_count: s.len * 6, instance_count: 1, first_vertex: s.start * 6, first_instance: i as u32 });
            }
        }
        for (i, (_, k)) in order.iter().enumerate().rev() {
            if let Some(s) = self.meshes[k].translucent {
                self.translucent_draws.push(s.page, DrawArgs { vertex_count: s.len * 6, instance_count: 1, first_vertex: s.start * 6, first_instance: i as u32 });
            }
        }
        if nodes.is_empty() {
            return;
        }
        if nodes.len() as u32 > self.nodes_capacity {
            self.nodes_capacity = (nodes.len() as u32).next_power_of_two();
            self.nodes = create_buffer(device, "lod nodes", self.nodes_capacity as u64 * 16, wgpu::BufferUsages::STORAGE);
            self.bind_group = None;
        }
        queue.write_buffer(&self.nodes, 0, bytemuck::cast_slice(&nodes));
        let total = (self.opaque_draws.args.len() + self.translucent_draws.args.len()) as u32;
        if total > self.indirect_capacity {
            self.indirect_capacity = total.next_power_of_two();
            self.indirect = create_buffer(device, "lod indirect", self.indirect_capacity as u64 * 16, wgpu::BufferUsages::INDIRECT);
        }
        let mut args = self.opaque_draws.args.clone();
        args.extend_from_slice(&self.translucent_draws.args);
        queue.write_buffer(&self.indirect, 0, bytemuck::cast_slice(&args));
        if self.mask_dirty {
            self.mask_dirty = false;
            queue.write_texture(
                wgpu::ImageCopyTexture { texture: &self.mask_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &self.mask,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(MASK_SIZE), rows_per_image: Some(MASK_SIZE) },
                wgpu::Extent3d { width: MASK_SIZE, height: MASK_SIZE, depth_or_array_layers: 1 },
            );
        }
        let cam = frame.camera;
        let proj = Mat4::perspective_infinite_reverse_rh(cam.fov_degrees.to_radians(), frame.aspect, NEAR);
        let (ecx, ecz) = ((eye.x / 16.0).floor(), (eye.z / 16.0).floor());
        // Texel je Block: Kachelbreite in Texeln (Atlas-Breite mal Rechteckbreite), bei
        // Vanilla-Texturen 16. Fest 16 angenommen -- hoeher aufgeloeste Packs waehlen damit
        // eine Stufe zu grob, was in dieser Entfernung nicht auffaellt.
        let globals = Globals {
            view: cam.view_matrix().to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            params: [cam.projection as f32, (cam.fov_degrees.to_radians() * 0.5).tan(), frame.aspect, frame.ao_strength.clamp(0.0, 1.0)],
            eye: [(eye.x - ecx * 16.0) as f32, (eye.z - ecz * 16.0) as f32, mips.saturating_sub(1).min(4) as f32, 16.0],
            eye_chunk: [ecx as i32, ecz as i32, frame.mask as i32, 0],
            clip: {
                let ((mx, mz), r) = frame.clip;
                let r = r.min(i32::MAX as u32 / 4) as i32;
                [mx - ecx as i32, mz - ecz as i32, r * r, 0]
            },
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        if self.bind_group.is_none() {
            let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lod globals"),
                layout: &self.globals_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: self.state_rects.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: self.nodes.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.mask_view) },
                ],
            }));
        }
    }

    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let Some(bind_group) = self.bind_group.as_ref() else { return };
        if !self.has_draws() {
            return;
        }
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_pipeline(&self.opaque_pipeline);
        self.draw(pass, &self.opaque_draws, 0);
        if !self.translucent_draws.args.is_empty() {
            pass.set_pipeline(&self.translucent_pipeline);
            self.draw(pass, &self.translucent_draws, self.opaque_draws.args.len() as u32);
        }
    }

    fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, draws: &Draws, base: u32) {
        for &(page, first, count) in &draws.runs {
            let Some(p) = self.pages.get(page as usize) else { continue };
            pass.set_bind_group(1, &p.bind_group, &[]);
            if self.multi_draw {
                let offset = (base + first) as u64 * std::mem::size_of::<DrawArgs>() as u64;
                pass.multi_draw_indirect(&self.indirect, offset, count);
            } else {
                for a in &draws.args[first as usize..(first + count) as usize] {
                    pass.draw(a.first_vertex..a.first_vertex + a.vertex_count, a.first_instance..a.first_instance + 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_ist_gueltig() {
        let quelle = include_str!("lod.wgsl");
        let modul = naga::front::wgsl::parse_str(quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(quelle)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&modul)
            .expect("lod.wgsl ungueltig");
    }
}
