// GPU-Skinning der Entity-Modelle (PLAN-skinning.md, Phase 2): feste Netze einmal je Modell auf
// der GPU (world::entity_model::skin_mesh), pro Frame nur Instanzen und eine Matrix je Teil.
// Gezeichnet mit block.wgsl::vs_entity_skinned und denselben Fragment-Shadern wie die
// CPU-Entities -- direkt nach deren deckendem Teil (s. BlockRenderer::draw_after_terrain).
//
// Ablauf je Frame: set_instances() mit allen Instanzen (nach Netz sortiert oder nicht -- hier
// wird gruppiert) und dem Matrixpuffer, dann draw() im Welt-Durchgang. Ein Aufruf je Netz.

use world::entity_model::{Affine, SkinVertex};

// Eine Instanz: ein Modell in einer Pose. 48 Byte.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkinInstance {
    // Atlas-Rechteck der Modelltextur (u0, v0, u1, v1).
    pub rect: [f32; 4],
    // Grundfarbe samt Helligkeit (wie `color` bei emit_into).
    pub color: [f32; 3],
    // Index der Matrix von Teil 0 dieser Instanz im Matrixpuffer.
    pub matrix: u32,
    // Schadens-Faerbung wie der Tint-Puffer der CPU-Entities (s. block.wgsl::fs_entity).
    pub tint: [f32; 4],
}

// Eine Teil-Matrix: drei Zeilen (Drehung | Verschiebung). Die Nullmatrix laesst ein Teil auf
// einen Punkt fallen -- so verschwinden versteckte Teile.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkinMatrix(pub [[f32; 4]; 3]);

impl SkinMatrix {
    pub const HIDDEN: Self = Self([[0.0; 4]; 3]);

    #[must_use]
    pub fn from_affine(a: Affine) -> Self {
        Self([0, 1, 2].map(|i| [a.m[i][0], a.m[i][1], a.m[i][2], a.t[i]]))
    }

    // Die Matrizen einer Instanz, versteckte Teile als Nullmatrix (s. part_matrices).
    pub fn extend_from_parts(out: &mut Vec<Self>, parts: &[Option<Affine>]) {
        out.extend(parts.iter().map(|p| p.map_or(Self::HIDDEN, Self::from_affine)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(u32);

const MATRIX_BINDING: u32 = 5;
const PREV_BINDING: u32 = 6;
// prev_view_proj des Bewegungs-Durchgangs (block.wgsl::EntityMotionParams).
const MOTION_PARAMS_BINDING: u32 = 4;

pub struct Skinning {
    // Alle Netze hintereinander, dazu die CPU-Kopie zum Umziehen in einen groesseren Puffer.
    mesh_data: Vec<SkinVertex>,
    mesh_buffer: wgpu::Buffer,
    meshes: Vec<(u32, u32)>,
    instance_buffer: wgpu::Buffer,
    matrix_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pub(crate) pipeline: wgpu::RenderPipeline,
    // Je Netz die Instanzen dieses Frames (Bereich im Instanzpuffer).
    draws: Vec<(MeshId, std::ops::Range<u32>)>,
    // Motion Blur (s. set_prev_matrices): die Matrizen des vorigen Bildes, gleich lang und in
    // derselben Reihenfolge wie die aktuellen, und die Pipeline ins Bewegungsbild.
    prev_buffer: wgpu::Buffer,
    motion_params: wgpu::Buffer,
    motion_layout: wgpu::BindGroupLayout,
    motion_bind_group: wgpu::BindGroup,
    motion_pipeline: wgpu::RenderPipeline,
    matrix_count: usize,
    prev_ok: bool,
    moving: bool,
    // Leucht-Umriss (s. set_outline_instances): eigene Instanzen mit der Umrissfarbe als
    // Tint, dieselben Matrizen, gezeichnet ins Silhouetten-Bild.
    outline_buffer: wgpu::Buffer,
    outline_draws: Vec<(MeshId, std::ops::Range<u32>)>,
    outline_pipeline: wgpu::RenderPipeline,
    // Glint verzauberter Instanzen (s. set_glint_instances): tint = (Massstab, Texturwahl).
    // Die Pipeline entsteht mit den Glint-Bindungen (s. create_glint_pipeline).
    glint_buffer: wgpu::Buffer,
    glint_draws: Vec<(MeshId, std::ops::Range<u32>)>,
    glint_pipeline: Option<wgpu::RenderPipeline>,
    mesh_layout: wgpu::VertexBufferLayout<'static>,
    instance_layout: wgpu::VertexBufferLayout<'static>,
}

const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint32, 3 => Uint32];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x3, 7 => Uint32, 6 => Float32x4];

fn buffer(device: &wgpu::Device, label: &str, bytes: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: bytes.max(256), usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

// Groesse nach Bedarf, verdoppelt, damit nicht jeder Frame neu anlegt.
fn wachsen(device: &wgpu::Device, buf: &mut wgpu::Buffer, label: &str, needed: u64, usage: wgpu::BufferUsages) -> bool {
    if buf.size() >= needed {
        return false;
    }
    *buf = buffer(device, label, needed.next_power_of_two(), usage);
    true
}

impl Skinning {
    // `layouts` = Gruppe 0 (Kamera) und 1 (Atlas) der Entity-Pipeline; `primitive`,
    // `depth_stencil` und `target` wie bei ihr, damit sich Tiefe und Culling gleich verhalten.
    #[must_use]
    pub(crate) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layouts: [&wgpu::BindGroupLayout; 2],
        target: wgpu::ColorTargetState,
        depth_stencil: wgpu::DepthStencilState,
    ) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin matrices layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: MATRIX_BINDING,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned entity pipeline layout"),
            bind_group_layouts: &[layouts[0], layouts[1], &bind_group_layout],
            push_constant_ranges: &[],
        });
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SkinVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &MESH_ATTRIBUTES,
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SkinInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned entity pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_entity_skinned",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[mesh_layout.clone(), instance_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_entity",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(target)],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(depth_stencil.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let storage = wgpu::BufferUsages::STORAGE;
        let matrix_buffer = buffer(device, "skin matrices", 0, storage);
        let bind_group = Self::bind(device, &bind_group_layout, &matrix_buffer);

        let speicher = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let motion_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin motion layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: MOTION_PARAMS_BINDING,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                speicher(MATRIX_BINDING),
                speicher(PREV_BINDING),
            ],
        });
        let motion_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned entity motion pipeline layout"),
            bind_group_layouts: &[layouts[0], layouts[1], &motion_layout],
            push_constant_ranges: &[],
        });
        let motion_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned entity motion pipeline"),
            layout: Some(&motion_pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_entity_skinned_motion",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[mesh_layout.clone(), instance_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_entity_motion",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: crate::motion_blur::OBJECT_MOTION_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
            // Wie die Entity-Bewegung: gegen die Tiefe der Welt, nur gelesen.
            depth_stencil: Some(wgpu::DepthStencilState { depth_write_enabled: false, ..depth_stencil }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Wie die Silhouetten-Pipeline der CPU-Entities: ohne Tiefe (der Umriss scheint durch
        // Waende) und beidseitig (nur die Flaeche zaehlt).
        let outline_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned entity outline pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_entity_skinned",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[mesh_layout.clone(), instance_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_entity_outline",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: crate::entity_outline::FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let prev_buffer = buffer(device, "skin prev matrices", 0, storage);
        let motion_params = buffer(device, "skin motion params", 64, wgpu::BufferUsages::UNIFORM);
        let motion_bind_group = Self::bind_motion(device, &motion_layout, &motion_params, &matrix_buffer, &prev_buffer);
        Self {
            mesh_data: Vec::new(),
            mesh_buffer: buffer(device, "skin meshes", 0, wgpu::BufferUsages::VERTEX),
            meshes: Vec::new(),
            instance_buffer: buffer(device, "skin instances", 0, wgpu::BufferUsages::VERTEX),
            matrix_buffer,
            bind_group_layout,
            bind_group,
            pipeline,
            draws: Vec::new(),
            prev_buffer,
            motion_params,
            motion_layout,
            motion_bind_group,
            motion_pipeline,
            matrix_count: 0,
            prev_ok: false,
            moving: false,
            outline_buffer: buffer(device, "skin outline instances", 0, wgpu::BufferUsages::VERTEX),
            outline_draws: Vec::new(),
            outline_pipeline,
            glint_buffer: buffer(device, "skin glint instances", 0, wgpu::BufferUsages::VERTEX),
            glint_draws: Vec::new(),
            glint_pipeline: None,
            mesh_layout,
            instance_layout,
        }
    }

    // Die Glint-Pipeline: wie die Glint-Pipeline der Ecken (`template` liefert Ziel, Mischung,
    // Tiefe samt Bias und Culling), nur mit vs_entity_skinned_glint und den Matrizen in Gruppe 3.
    pub(crate) fn create_glint_pipeline(
        &mut self,
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layouts: [&wgpu::BindGroupLayout; 3],
        target: wgpu::ColorTargetState,
        primitive: wgpu::PrimitiveState,
        depth_stencil: wgpu::DepthStencilState,
    ) {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned glint pipeline layout"),
            bind_group_layouts: &[layouts[0], layouts[1], layouts[2], &self.bind_group_layout],
            push_constant_ranges: &[],
        });
        self.glint_pipeline = Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned glint pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_entity_skinned_glint",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[self.mesh_layout.clone(), self.instance_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_glint",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(target)],
            }),
            primitive,
            depth_stencil: Some(depth_stencil),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        }));
    }

    // Nach Netz sortiert und zu Bereichen zusammengefasst (ein Aufruf je Netz).
    fn group(instances: &[(MeshId, SkinInstance)]) -> (Vec<SkinInstance>, Vec<(MeshId, std::ops::Range<u32>)>) {
        let mut sortiert: Vec<&(MeshId, SkinInstance)> = instances.iter().collect();
        sortiert.sort_by_key(|(m, _)| m.0);
        let mut draws: Vec<(MeshId, std::ops::Range<u32>)> = Vec::new();
        for (i, (m, _)) in sortiert.iter().enumerate() {
            let i = i as u32;
            match draws.last_mut() {
                Some((letztes, bereich)) if letztes == m => bereich.end = i + 1,
                _ => draws.push((*m, i..i + 1)),
            }
        }
        (sortiert.iter().map(|(_, i)| *i).collect(), draws)
    }

    fn bind_motion(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, params: &wgpu::Buffer, matrices: &wgpu::Buffer, prev: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skin motion"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: MOTION_PARAMS_BINDING, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: MATRIX_BINDING, resource: matrices.as_entire_binding() },
                wgpu::BindGroupEntry { binding: PREV_BINDING, resource: prev.as_entire_binding() },
            ],
        })
    }

    fn bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, matrices: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skin matrices"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: MATRIX_BINDING, resource: matrices.as_entire_binding() }],
        })
    }

    // Ein festes Netz hochladen (einmal je Modell und Seitigkeit). Waechst der Puffer, zieht
    // alles in einen neuen um.
    pub fn add_mesh(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vertices: &[SkinVertex]) -> MeshId {
        let first = self.mesh_data.len() as u32;
        self.mesh_data.extend_from_slice(vertices);
        let bytes = std::mem::size_of_val(self.mesh_data.as_slice()) as u64;
        if wachsen(device, &mut self.mesh_buffer, "skin meshes", bytes, wgpu::BufferUsages::VERTEX) {
            queue.write_buffer(&self.mesh_buffer, 0, bytemuck::cast_slice(&self.mesh_data));
        } else {
            let offset = u64::from(first) * std::mem::size_of::<SkinVertex>() as u64;
            queue.write_buffer(&self.mesh_buffer, offset, bytemuck::cast_slice(vertices));
        }
        self.meshes.push((first, vertices.len() as u32));
        MeshId(self.meshes.len() as u32 - 1)
    }

    // Die Instanzen dieses Frames samt ihrer Matrizen (SkinInstance::matrix zeigt hinein).
    // Gleiche Netze werden zu einem Aufruf zusammengefasst; die Reihenfolge innerhalb eines
    // Netzes bleibt.
    pub fn set_instances(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, instances: &[(MeshId, SkinInstance)], matrices: &[SkinMatrix]) {
        self.draws.clear();
        self.outline_draws.clear();
        self.glint_draws.clear();
        self.prev_ok = false;
        self.moving = false;
        self.matrix_count = matrices.len();
        if matrices.is_empty() {
            return;
        }
        let (daten, draws) = Self::group(instances);
        self.draws = draws;
        if !daten.is_empty() {
            wachsen(device, &mut self.instance_buffer, "skin instances", std::mem::size_of_val(daten.as_slice()) as u64, wgpu::BufferUsages::VERTEX);
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&daten));
        }
        let groesse = std::mem::size_of_val(matrices) as u64;
        let neu_m = wachsen(device, &mut self.matrix_buffer, "skin matrices", groesse, wgpu::BufferUsages::STORAGE);
        let neu_p = wachsen(device, &mut self.prev_buffer, "skin prev matrices", groesse, wgpu::BufferUsages::STORAGE);
        if neu_m {
            self.bind_group = Self::bind(device, &self.bind_group_layout, &self.matrix_buffer);
        }
        if neu_m || neu_p {
            self.motion_bind_group = Self::bind_motion(device, &self.motion_layout, &self.motion_params, &self.matrix_buffer, &self.prev_buffer);
        }
        if !matrices.is_empty() {
            queue.write_buffer(&self.matrix_buffer, 0, bytemuck::cast_slice(matrices));
        }
    }

    #[must_use]
    pub fn has_draws(&self) -> bool {
        !self.draws.is_empty()
    }

    // Leucht-Umriss: Instanzen mit der Umrissfarbe als Tint, auf die Matrizen aus dem
    // set_instances davor. Danach aufrufen -- set_instances leert sie.
    pub fn set_outline_instances(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, instances: &[(MeshId, SkinInstance)]) {
        let (daten, draws) = Self::group(instances);
        self.outline_draws = draws;
        if !daten.is_empty() {
            wachsen(device, &mut self.outline_buffer, "skin outline instances", std::mem::size_of_val(daten.as_slice()) as u64, wgpu::BufferUsages::VERTEX);
            queue.write_buffer(&self.outline_buffer, 0, bytemuck::cast_slice(&daten));
        }
    }

    // Glint: Instanzen mit tint = (Massstab, Texturwahl, 0, 0), auf die Matrizen aus dem
    // set_instances davor (das leert sie).
    pub fn set_glint_instances(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, instances: &[(MeshId, SkinInstance)]) {
        let (daten, draws) = Self::group(instances);
        self.glint_draws = draws;
        if !daten.is_empty() {
            wachsen(device, &mut self.glint_buffer, "skin glint instances", std::mem::size_of_val(daten.as_slice()) as u64, wgpu::BufferUsages::VERTEX);
            queue.write_buffer(&self.glint_buffer, 0, bytemuck::cast_slice(&daten));
        }
    }

    // Den Glint ueber die eben gezeichneten Instanzen. Gruppe 0 und 1 muessen gebunden sein,
    // `glint_group` ist Gruppe 2 der Glint-Pipeline (Texturen, Parameter).
    pub(crate) fn draw_glint<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, glint_group: &'a wgpu::BindGroup) {
        let Some(pipeline) = self.glint_pipeline.as_ref().filter(|_| !self.glint_draws.is_empty()) else { return };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(2, glint_group, &[]);
        pass.set_bind_group(3, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.mesh_buffer.slice(..));
        pass.set_vertex_buffer(1, self.glint_buffer.slice(..));
        for (mesh, instanzen) in &self.glint_draws {
            let (first, count) = self.meshes[mesh.0 as usize];
            pass.draw(first..first + count, instanzen.clone());
        }
    }

    #[must_use]
    pub fn has_outlines(&self) -> bool {
        !self.outline_draws.is_empty()
    }

    // Die Silhouetten ins Umriss-Bild. Gruppe 0 und 1 (Kamera, Entity-Atlas) muessen gebunden sein.
    pub(crate) fn draw_outline<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.outline_draws.is_empty() {
            return;
        }
        pass.set_pipeline(&self.outline_pipeline);
        pass.set_bind_group(2, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.mesh_buffer.slice(..));
        pass.set_vertex_buffer(1, self.outline_buffer.slice(..));
        for (mesh, instanzen) in &self.outline_draws {
            let (first, count) = self.meshes[mesh.0 as usize];
            pass.draw(first..first + count, instanzen.clone());
        }
    }

    // Motion Blur: die Matrizen des vorigen Bildes zu denen aus set_instances (gleich lang,
    // relativ zum vorigen Auge). `moving`: ob sich eine Instanz selbst bewegt hat. Passt die
    // Laenge nicht, gibt es fuer die Instanzen in diesem Bild keine eigene Bewegung.
    pub fn set_prev_matrices(&mut self, queue: &wgpu::Queue, prev: &[SkinMatrix], moving: bool) {
        self.prev_ok = !self.draws.is_empty() && prev.len() == self.matrix_count && !prev.is_empty();
        if self.prev_ok {
            queue.write_buffer(&self.prev_buffer, 0, bytemuck::cast_slice(prev));
        }
        self.moving = self.prev_ok && moving;
    }

    #[must_use]
    pub const fn has_motion(&self) -> bool {
        self.prev_ok
    }

    #[must_use]
    pub const fn moving(&self) -> bool {
        self.moving
    }

    // Die Bewegung der Instanzen ins Bewegungsbild (s. BlockRenderer::render_entity_motion).
    // Gruppe 0 und 1 (Kamera, Entity-Atlas) muessen gebunden sein.
    pub(crate) fn draw_motion<'a>(&'a self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'a>, prev_view_proj: glam::Mat4) {
        if !self.prev_ok || self.draws.is_empty() {
            return;
        }
        queue.write_buffer(&self.motion_params, 0, bytemuck::cast_slice(&prev_view_proj.to_cols_array()));
        pass.set_pipeline(&self.motion_pipeline);
        pass.set_bind_group(2, &self.motion_bind_group, &[]);
        pass.set_vertex_buffer(0, self.mesh_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        for (mesh, instanzen) in &self.draws {
            let (first, count) = self.meshes[mesh.0 as usize];
            pass.draw(first..first + count, instanzen.clone());
        }
    }

    // Zeichnen. Gruppe 0 und 1 (Kamera, Entity-Atlas) muessen schon gebunden sein; Gruppe 2
    // und die Pipeline setzt das hier selbst.
    pub(crate) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.draws.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(2, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.mesh_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        for (mesh, instanzen) in &self.draws {
            let (first, count) = self.meshes[mesh.0 as usize];
            pass.draw(first..first + count, instanzen.clone());
        }
    }
}
