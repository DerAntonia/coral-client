// Zeichnen mit Vanillas Core-Shadern aus Resourcepacks (s. vanilla_shader.rs).
//
// Ein Pack-Shader erwartet genau das, was Vanilla ihm gibt: Vertices im Vanilla-Format
// (Position, Color, UV0, UV1, UV2, Normal), Vanillas Uniform-Bloecke und die Sampler
// (Sampler0 = Atlas, Sampler1 = Overlay, Sampler2 = Lightmap). Diese Datei baut all das aus
// dem Zustand dieses Clients nach und haelt die Pipelines der Programme, die ein Pack
// ueberschreibt.
//
// Zwei Programme laufen hier: rendertype_item_entity_translucent_cull, mit dem Vanilla Items
// in der Welt zeichnet (Displays, liegende Items, Items in fremden Haenden), nur wenn ein Pack
// es ersetzt; und rendertype_world_border fuer die Weltgrenze, IMMER -- aus dem Pack oder
// sonst Vanillas eigenes aus dem Jar. Weitere Rendertypen haengen sich an dieselben Uniforms
// und Bindungen.

use std::borrow::Cow;

use crate::vanilla_shader::{self, Program};

// Ein Vertex in Vanillas Format POSITION_COLOR_TEXTURE_OVERLAY_LIGHT_NORMAL (VertexFormats.
// ENTITY). Farbe und Normale als Gleitkommazahlen statt Bytes -- der Shader sieht dieselben
// Werte, nur ohne Rundung auf 1/255 bzw. 1/127.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VanillaVertex {
    // Kamerarelativ (Vanilla: Position relativ zur Kamera, Drehung steckt in ModelViewMat).
    pub position: [f32; 3],
    // RGBA 0..1 -- bei Items die Toenung der Flaeche (Trankfarbe ...), sonst weiss.
    pub color: [f32; 4],
    pub uv0: [f32; 2],
    // Overlay-Koordinate (OverlayTexture.DEFAULT_UV = (0, 10): kein Rot, kein Weiss).
    pub uv1: [i32; 2],
    // Licht: (Blocklicht * 16, Himmelslicht * 16), wie LightmapTextureManager.pack.
    pub uv2: [i32; 2],
    pub normal: [f32; 3],
    // UV1/UV2 noch einmal als Gleitkomma -- Vanillas Shader deklarieren sie mal als ivec2, mal
    // als vec2 (item_entity_translucent_cull: UV1 vec2, UV2 ivec2), und das Vertex-Format muss
    // zur Deklaration passen. Wird beim Hochladen aus uv1/uv2 gefuellt.
    pub uv1f: [f32; 2],
    pub uv2f: [f32; 2],
}

pub const OVERLAY_NONE: [i32; 2] = [0, 10];

// Versatz und Format eines Attributs im Vertex fuer den deklarierten Typ (Reihenfolge der
// Attribute wie vanilla_shader::ATTRIBUTES). None = so deklariert passt es nicht.
fn attribute_layout(index: usize, ty: &str) -> Option<(u64, wgpu::VertexFormat)> {
    use wgpu::VertexFormat as F;
    Some(match (index, ty) {
        (0, "vec3") => (0, F::Float32x3),
        (1, "vec4") => (12, F::Float32x4),
        (2, "vec2") => (28, F::Float32x2),
        (3, "ivec2") => (36, F::Sint32x2),
        (3, "vec2") => (64, F::Float32x2),
        (4, "ivec2") => (44, F::Sint32x2),
        (4, "vec2") => (72, F::Float32x2),
        (5, "vec3") => (52, F::Float32x3),
        _ => return None,
    })
}

// Was ein Bild an Vanilla-Zustand braucht. Alles kommt vom Aufrufer, der die Welt kennt.
#[derive(Clone, Copy, Debug)]
pub struct VanillaFrame {
    // Drehung der Kamera (dieselbe Matrix wie fuer die eigene Geometrie, die kamerarelativ ist).
    pub view: glam::Mat4,
    // Projektion in OpenGL-Konvention (Tiefe -1..1), wie Vanilla sie an ProjMat gibt.
    pub proj_gl: glam::Mat4,
    pub screen: [f32; 2],
    // Weltposition der Kamera.
    pub camera: glam::DVec3,
    // GameTime: (Weltalter mod 24000) / 24000.
    pub game_time: f32,
    pub fog_color: [f32; 4],
    // FogEnvironmentalStart/End, FogRenderDistanceStart/End, FogSkyEnd, FogCloudsEnd.
    pub fog: [f32; 6],
    // Fuer die Lightmap: Tageslicht 0..1 (SkyFactor) und Helligkeit (Gamma-Einstellung 0..1).
    pub sky_factor: f32,
    pub brightness: f32,
}

// Vanillas Lightmap (core/lightmap.fsh), auf der CPU fuer die Oberwelt: Umgebungslicht 0,
// Blockfaktor 1.5, Himmelsfarbe weiss, ohne Dunkelheit und Nachtsicht.
pub fn lightmap_rgba(sky_factor: f32, brightness: f32) -> Vec<u8> {
    let hell = |level: f32| level / (4.0 - 3.0 * level);
    let mut out = Vec::with_capacity(16 * 16 * 4);
    for sky in 0..16 {
        for block in 0..16 {
            let b = hell(block as f32 / 15.0) * 1.5;
            let s = hell(sky as f32 / 15.0) * sky_factor;
            let mut c = [b, b * ((b * 0.6 + 0.4) * 0.6 + 0.4), b * (b * b * 0.6 + 0.4)];
            for k in &mut c {
                *k += s;
                *k = *k + (0.75 - *k) * 0.04;
                *k = k.clamp(0.0, 1.0);
            }
            let max = c[0].max(c[1]).max(c[2]);
            if max > 0.0 {
                let inv = 1.0 - max;
                let skaliert = 1.0 - inv * inv * inv * inv;
                for k in &mut c {
                    let ng = *k * (skaliert / max);
                    *k += (ng - *k) * brightness;
                }
            }
            for k in &mut c {
                *k += (0.75 - *k) * 0.04;
            }
            out.extend(c.iter().map(|k| (k.clamp(0.0, 1.0) * 255.0).round() as u8));
            out.push(255);
        }
    }
    out
}

// OverlayTexture: Zeilen 0..7 Schadensrot (Alpha 178), darunter weiss mit nach rechts
// abnehmender Deckkraft (Aufblitzen der Creeper). Hier nur das, was Vanilla fest hineinschreibt.
fn overlay_rgba() -> Vec<u8> {
    let mut out = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            if y < 8 {
                out.extend_from_slice(&[255, 0, 0, 178]);
            } else {
                let a = ((1.0 - x as f32 / 15.0 * 0.75) * 255.0) as u8;
                out.extend_from_slice(&[255, 255, 255, a]);
            }
        }
    }
    out
}

// std140-Inhalte der Uniform-Bloecke, Byte fuer Byte (s. vanilla_shader::UNIFORM_BLOCKS).
struct Std140(Vec<u8>);

impl Std140 {
    fn new(size: usize) -> Self {
        Self(vec![0; size])
    }
    fn f32(&mut self, at: usize, v: f32) -> &mut Self {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(&mut self, at: usize, v: i32) -> &mut Self {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
        self
    }
    fn vec(&mut self, at: usize, v: &[f32]) -> &mut Self {
        for (i, x) in v.iter().enumerate() {
            self.f32(at + 4 * i, *x);
        }
        self
    }
    fn mat4(&mut self, at: usize, m: &glam::Mat4) -> &mut Self {
        self.vec(at, &m.to_cols_array())
    }
}

const BLOCK_SIZES: [u64; 6] = [160, 64, 64, 48, 32, 16];

pub struct VanillaDraw {
    surface_format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    uniforms: Vec<wgpu::Buffer>,
    lightmap: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    item_pipeline: Option<wgpu::RenderPipeline>,
    // Vierecke zu je vier Vertices, gezeichnet ueber Indizes 0,1,2 / 2,3,0 -- wie Vanillas
    // DrawMode.QUADS. Pack-Shader verlassen sich darauf (gl_VertexID & 3 ist die Ecke).
    item_buffer: wgpu::Buffer,
    item_capacity: u64,
    item_count: u32,
    quad_indices: wgpu::Buffer,
    quad_index_capacity: u64,
    last_lightmap: Option<(u32, u32)>,
    // Fuer weitere Bindegruppen mit anderem Sampler0 bzw. eigenen DynamicTransforms.
    overlay_view: wgpu::TextureView,
    lightmap_view: wgpu::TextureView,
    linear: wgpu::Sampler,
    // Weltgrenze: eigene DynamicTransforms (Farbe, Versatz, Texturverschiebung), die
    // Kraftfeld-Textur als Sampler0, hoechstens vier Waende zu je vier Vertices.
    border_pipeline: Option<wgpu::RenderPipeline>,
    border_bind_group: Option<wgpu::BindGroup>,
    border_transforms: wgpu::Buffer,
    border_buffer: wgpu::Buffer,
    border_count: u32,
}

// Die Weltgrenze fuer ein Bild (WorldBorderRenderer): Waende, Farbe samt Deckkraft und der
// Versatz, den Vanilla als ModelOffset gibt.
#[derive(Clone, Debug)]
pub struct BorderDraw {
    pub vertices: Vec<VanillaVertex>,
    pub model_offset: [f32; 3],
    pub color: [f32; 4],
}

// Die Grenze so, wie Vanilla sie zeichnet: nur innerhalb der Sichtweite `render_distance`
// (Bloecke), um so deckender, je naeher man ihr kommt ((1 - Abstand/Sichtweite)^4, draussen
// voll). Jede Wand reicht in ihrer Richtung von Kamera - Sichtweite bis Kamera + Sichtweite,
// beschnitten auf die Grenze, und in der Hoehe von -h bis +h (h = Vanillas Tiefen-Fernebene,
// viermal die Sichtweite). Die Positionen relativ zur Ecke (Wandanfang x, 0, Wandanfang z),
// den Rest bis zur Kamera traegt `model_offset`. Textur: zwei Bloecke je Wiederholung.
// None = nichts zu sehen.
pub fn world_border_draw(border: &world::world_border::WorldBorder, partial: f32, camera: glam::DVec3, render_distance: f64) -> Option<BorderDraw> {
    let [min_x, max_x, min_z, max_z] = border.bounds(partial);
    let d = render_distance;
    let (x, z) = (camera.x, camera.z);
    let tief_drin = x < max_x - d && x > min_x + d && z < max_z - d && z > min_z + d;
    let weit_draussen = x < min_x - d || x > max_x + d || z < min_z - d || z > max_z + d;
    if tief_drin || weit_draussen {
        return None;
    }
    let abstand = (x - min_x).min(max_x - x).min(z - min_z).min(max_z - z);
    let alpha = (1.0 - abstand / d).powi(4).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return None;
    }
    let h = (d * 4.0) as f32;
    let z0 = (z - d).floor().max(min_z);
    let z1 = (z + d).ceil().min(max_z);
    let x0 = (x - d).floor().max(min_x);
    let x1 = (x + d).ceil().min(max_x);
    // U beginnt je nach Paritaet des Wandanfangs bei 0 oder 0.5, damit das Muster an der
    // Welt haengt und nicht an der Kamera.
    let u_z = (z0.floor() as i64 & 1) as f32 * 0.5;
    let u_x = (x0.floor() as i64 & 1) as f32 * 0.5;
    let len_z = ((z1 - z0) / 2.0) as f32;
    let len_x = ((x1 - x0) / 2.0) as f32;
    let (v_unten, v_oben) = (h, 0.0);
    let v = |p: [f32; 3], uv: [f32; 2]| VanillaVertex { position: p, color: [1.0; 4], uv0: uv, uv1: OVERLAY_NONE, uv2: [240, 240], normal: [0.0, 1.0, 0.0], ..Default::default() };
    let (sx, sz) = ((x1 - x0) as f32, (z1 - z0) as f32);
    let (ost, sued) = ((max_x - x0) as f32, (max_z - z0) as f32);
    // Abstand der Kamera zu jeder Wand; gezeichnet wird, was naeher als die Sichtweite liegt.
    let waende: [(f64, [VanillaVertex; 4]); 4] = [
        // Sued (z = max_z)
        (max_z - z, [
            v([0.0, -h, sued], [u_x, v_unten]),
            v([sx, -h, sued], [len_x + u_x, v_unten]),
            v([sx, h, sued], [len_x + u_x, v_oben]),
            v([0.0, h, sued], [u_x, v_oben]),
        ]),
        // West (x = min_x)
        (x - min_x, [
            v([0.0, -h, 0.0], [u_z, v_unten]),
            v([0.0, -h, sz], [len_z + u_z, v_unten]),
            v([0.0, h, sz], [len_z + u_z, v_oben]),
            v([0.0, h, 0.0], [u_z, v_oben]),
        ]),
        // Nord (z = min_z)
        (z - min_z, [
            v([sx, -h, 0.0], [u_x, v_unten]),
            v([0.0, -h, 0.0], [len_x + u_x, v_unten]),
            v([0.0, h, 0.0], [len_x + u_x, v_oben]),
            v([sx, h, 0.0], [u_x, v_oben]),
        ]),
        // Ost (x = max_x)
        (max_x - x, [
            v([ost, -h, sz], [u_z, v_unten]),
            v([ost, -h, 0.0], [len_z + u_z, v_unten]),
            v([ost, h, 0.0], [len_z + u_z, v_oben]),
            v([ost, h, sz], [u_z, v_oben]),
        ]),
    ];
    let vertices: Vec<VanillaVertex> = waende.iter().filter(|(a, _)| *a < d).flat_map(|(_, q)| *q).collect();
    if vertices.is_empty() {
        return None;
    }
    let farbe = border.status().color();
    let kanal = |s: u32| ((farbe >> s) & 0xFF) as f32 / 255.0;
    Some(BorderDraw {
        vertices,
        model_offset: [(x0 - x) as f32, -camera.y as f32, (z0 - z) as f32],
        color: [kanal(16), kanal(8), kanal(0), alpha as f32],
    })
}

impl VanillaDraw {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat, entity_atlas: &wgpu::Texture) -> Self {
        let mut entries = Vec::new();
        for (_, binding) in vanilla_shader::UNIFORM_BLOCKS {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            });
        }
        for i in 0..vanilla_shader::SAMPLERS.len() {
            let (t, s) = vanilla_shader::sampler_bindings(i);
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: t,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            });
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: s,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("vanilla shader bind group layout"), entries: &entries });
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("vanilla shader pipeline layout"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let uniforms: Vec<wgpu::Buffer> = BLOCK_SIZES
            .iter()
            .map(|&size| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("vanilla shader uniforms"),
                    size,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let klein = |label, rgba: &[u8]| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::ImageCopyTexture { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                rgba,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(64), rows_per_image: Some(16) },
                wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            );
            t
        };
        let overlay = klein("vanilla overlay", &overlay_rgba());
        let lightmap = klein("vanilla lightmap", &lightmap_rgba(1.0, 0.5));
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("vanilla shader sampler"),
                mag_filter: filter,
                min_filter: filter,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            })
        };
        let nearest = sampler(wgpu::FilterMode::Nearest);
        let linear = sampler(wgpu::FilterMode::Linear);
        // Gamma-Sicht des Atlas: Vanilla sampelt die Texel so, wie sie in der Datei stehen.
        let atlas_view = entity_atlas.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8Unorm), ..Default::default() });
        let overlay_view = overlay.create_view(&Default::default());
        let lightmap_view = lightmap.create_view(&Default::default());
        let mut bg = Vec::new();
        for (i, (_, binding)) in vanilla_shader::UNIFORM_BLOCKS.iter().enumerate() {
            bg.push(wgpu::BindGroupEntry { binding: *binding, resource: uniforms[i].as_entire_binding() });
        }
        // Sampler0 nearest wie Vanillas Atlas, Sampler1/2 linear wie Overlay und Lightmap.
        for (i, (view, smp)) in [(&atlas_view, &nearest), (&overlay_view, &linear), (&lightmap_view, &linear)].into_iter().enumerate() {
            let (t, s) = vanilla_shader::sampler_bindings(i);
            bg.push(wgpu::BindGroupEntry { binding: t, resource: wgpu::BindingResource::TextureView(view) });
            bg.push(wgpu::BindGroupEntry { binding: s, resource: wgpu::BindingResource::Sampler(smp) });
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("vanilla shader bind group"), layout: &layout, entries: &bg });
        let item_capacity = 4096;
        let item_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vanilla item vertices"),
            size: item_capacity * std::mem::size_of::<VanillaVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (quad_indices, quad_index_capacity) = quad_index_buffer(device, queue, item_capacity);
        let border_transforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vanilla border transforms"),
            size: BLOCK_SIZES[0],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let border_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vanilla border vertices"),
            size: 16 * std::mem::size_of::<VanillaVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            surface_format,
            layout,
            pipeline_layout,
            uniforms,
            lightmap,
            bind_group,
            item_pipeline: None,
            item_buffer,
            item_capacity,
            item_count: 0,
            quad_indices,
            quad_index_capacity,
            last_lightmap: None,
            overlay_view,
            lightmap_view,
            linear,
            border_pipeline: None,
            border_bind_group: None,
            border_transforms,
            border_buffer,
            border_count: 0,
        }
    }

    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    // Das Programm fuer Items in der Welt setzen (None = Vanillas eigenes, also der normale
    // Weg dieses Clients). Liefert den Grund, wenn das Programm sich nicht bauen liess.
    pub fn set_item_program(&mut self, device: &wgpu::Device, program: Option<Program>) -> Result<(), String> {
        self.item_pipeline = None;
        let Some(program) = program else { return Ok(()) };
        // RenderPipelines.RENDERTYPE_ITEM_ENTITY_TRANSLUCENT_CULL: durchscheinend gemischt,
        // Rueckseiten weg, Tiefe testen und schreiben.
        let pipeline = self.build_pipeline(device, program, wgpu::BlendState::ALPHA_BLENDING, Some(wgpu::Face::Back), true, Default::default())?;
        self.item_pipeline = Some(pipeline);
        Ok(())
    }

    // Das Programm fuer die Weltgrenze und ihre Kraftfeld-Textur (RGBA8, Breite x Hoehe).
    // Ohne Programm wird keine Grenze gezeichnet.
    pub fn set_border_program(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, program: Option<Program>, forcefield: (u32, u32, &[u8])) -> Result<(), String> {
        self.border_pipeline = None;
        let Some(program) = program else { return Ok(()) };
        // RenderPipelines.WORLD_BORDER: additiv ueber das Bild (Quelle * Alpha + Ziel), beide
        // Seiten, Tiefe testen ohne zu schreiben, um 3 zur Kamera gezogen, damit sie auf
        // Bloecken genau an der Grenze nicht flimmert.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
        };
        let bias = wgpu::DepthBiasState { constant: -3, slope_scale: -3.0, clamp: 0.0 };
        let pipeline = self.build_pipeline(device, program, blend, None, false, bias)?;
        let (w, h, rgba) = forcefield;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vanilla forcefield"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&Default::default());
        // Die Textur wiederholt sich ueber die ganze Wand.
        let wiederholt = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vanilla forcefield sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let mut bg = Vec::new();
        for (i, (_, binding)) in vanilla_shader::UNIFORM_BLOCKS.iter().enumerate() {
            let buf = if i == 0 { &self.border_transforms } else { &self.uniforms[i] };
            bg.push(wgpu::BindGroupEntry { binding: *binding, resource: buf.as_entire_binding() });
        }
        for (i, (view, smp)) in [(&view, &wiederholt), (&self.overlay_view, &self.linear), (&self.lightmap_view, &self.linear)].into_iter().enumerate() {
            let (t, s) = vanilla_shader::sampler_bindings(i);
            bg.push(wgpu::BindGroupEntry { binding: t, resource: wgpu::BindingResource::TextureView(view) });
            bg.push(wgpu::BindGroupEntry { binding: s, resource: wgpu::BindingResource::Sampler(smp) });
        }
        self.border_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("vanilla border bind group"), layout: &self.layout, entries: &bg }));
        self.border_pipeline = Some(pipeline);
        Ok(())
    }

    pub fn has_border_program(&self) -> bool {
        self.border_pipeline.is_some()
    }

    // Die Grenze fuer dieses Bild (None = keine). `view`: dieselbe Kameradrehung wie in
    // update(); `millis`: laufende Uhr fuer das Wandern der Textur (alle 3 Sekunden einmal).
    pub fn set_border(&mut self, queue: &wgpu::Queue, view: glam::Mat4, millis: u64, draw: Option<&BorderDraw>) {
        let Some(draw) = draw.filter(|_| self.border_pipeline.is_some()) else {
            self.border_count = 0;
            return;
        };
        let t = (millis % 3000) as f32 / 3000.0;
        let mut dyn_t = Std140::new(BLOCK_SIZES[0] as usize);
        dyn_t.mat4(0, &view).vec(64, &draw.color).vec(80, &draw.model_offset).mat4(96, &glam::Mat4::from_translation(glam::Vec3::new(t, t, 0.0)));
        queue.write_buffer(&self.border_transforms, 0, &dyn_t.0);
        let n = draw.vertices.len().min(16);
        let voll: Vec<VanillaVertex> = draw.vertices[..n].iter().map(|v| VanillaVertex { uv1f: v.uv1.map(|x| x as f32), uv2f: v.uv2.map(|x| x as f32), ..*v }).collect();
        queue.write_buffer(&self.border_buffer, 0, bytemuck::cast_slice(&voll));
        self.border_count = n as u32 / 4 * 4;
    }

    // Die Grenze mit ihrem Programm. Danach muss der Aufrufer seine eigenen Bindungen neu
    // setzen (wie bei draw_items).
    pub fn draw_border<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) -> bool {
        let (Some(pipeline), Some(bind_group)) = (&self.border_pipeline, &self.border_bind_group) else { return false };
        if self.border_count == 0 {
            return false;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, self.border_buffer.slice(..self.border_count as u64 * std::mem::size_of::<VanillaVertex>() as u64));
        pass.set_index_buffer(self.quad_indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.border_count / 4 * 6, 0, 0..1);
        true
    }

    pub fn has_item_program(&self) -> bool {
        self.item_pipeline.is_some()
    }

    // Die Uniforms fuer ein Bild aus der Kamera dieses Clients. `game_time`: Tageszeit
    // 0..1 (GameTime), `daylight`: Tageslicht fuer die Lightmap.
    pub fn update_from_camera(&mut self, queue: &wgpu::Queue, camera: &crate::Camera, size: (u32, u32), game_time: f32, daylight: f32) {
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let frame = VanillaFrame {
            view: camera.view_matrix(),
            // Dieselbe Projektion wie die eigene (Camera::proj_matrix), nur mit OpenGL-Tiefe.
            proj_gl: glam::Mat4::perspective_rh_gl(camera.fov_degrees.to_radians(), aspect, 0.05, 1000.0),
            screen: [size.0 as f32, size.1 as f32],
            camera: camera.position,
            game_time,
            // Nebel traegt dieser Client selbst; Deckkraft 0 heisst fuer apply_fog: keiner.
            fog_color: [0.0; 4],
            fog: [f32::MAX, f32::MAX, f32::MAX, f32::MAX, f32::MAX, f32::MAX],
            sky_factor: daylight,
            brightness: 0.5,
        };
        self.update(queue, &frame);
    }

    fn build_pipeline(
        &self,
        device: &wgpu::Device,
        program: Program,
        blend: wgpu::BlendState,
        cull: Option<wgpu::Face>,
        depth_write: bool,
        bias: wgpu::DepthBiasState,
    ) -> Result<wgpu::RenderPipeline, String> {
        let mut attributes = Vec::new();
        for var in &program.attributes {
            let index = vanilla_shader::ATTRIBUTES.iter().position(|a| *a == var.name).ok_or_else(|| format!("Attribut {}", var.name))?;
            let (offset, format) = attribute_layout(index, &var.ty).ok_or_else(|| format!("Attribut {} als {}", var.name, var.ty))?;
            attributes.push(wgpu::VertexAttribute { format, offset, shader_location: index as u32 });
        }
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&program.name), source: wgpu::ShaderSource::Naga(Cow::Owned(program.vertex)) });
        let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&program.name), source: wgpu::ShaderSource::Naga(Cow::Owned(program.fragment)) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&program.name),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: "main",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<VanillaVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: "main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: self.surface_format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: cull, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::block_renderer::DEPTH_FORMAT,
                depth_write_enabled: depth_write,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias,
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        match pollster::block_on(device.pop_error_scope()) {
            Some(e) => Err(e.to_string()),
            None => Ok(pipeline),
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, f: &VanillaFrame) {
        let mut dyn_t = Std140::new(160);
        dyn_t.mat4(0, &f.view).vec(64, &[1.0, 1.0, 1.0, 1.0]).vec(80, &[0.0, 0.0, 0.0]).mat4(96, &glam::Mat4::IDENTITY);
        let mut proj = Std140::new(64);
        proj.mat4(0, &f.proj_gl);
        let block = f.camera.floor();
        let mut globals = Std140::new(64);
        globals
            .i32(0, block.x as i32)
            .i32(4, block.y as i32)
            .i32(8, block.z as i32)
            .vec(16, &[(f.camera.x - block.x) as f32, (f.camera.y - block.y) as f32, (f.camera.z - block.z) as f32])
            .vec(32, &f.screen)
            .f32(40, 1.0)
            .f32(44, f.game_time);
        let mut fog = Std140::new(48);
        fog.vec(0, &f.fog_color).vec(16, &f.fog);
        let l0 = glam::Vec3::new(0.2, 1.0, -0.7).normalize();
        let l1 = glam::Vec3::new(-0.2, 1.0, 0.7).normalize();
        let mut light = Std140::new(32);
        light.vec(0, &l0.to_array()).vec(16, &l1.to_array());
        let mut internal = Std140::new(16);
        internal.vec(0, &[f.screen[0], f.screen[1], if self.surface_format.is_srgb() { 1.0 } else { 0.0 }, 0.0]);
        for (buf, data) in self.uniforms.iter().zip([dyn_t, proj, globals, fog, light, internal]) {
            queue.write_buffer(buf, 0, &data.0);
        }
        // Lightmap nur neu, wenn sich ihre Eingaben spuerbar geaendert haben.
        let key = ((f.sky_factor * 255.0) as u32, (f.brightness * 255.0) as u32);
        if self.last_lightmap != Some(key) {
            self.last_lightmap = Some(key);
            queue.write_texture(
                wgpu::ImageCopyTexture { texture: &self.lightmap, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &lightmap_rgba(f.sky_factor, f.brightness),
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(64), rows_per_image: Some(16) },
                wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
            );
        }
    }

    pub fn set_item_vertices(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vertices: &[VanillaVertex]) {
        let n = vertices.len() as u64;
        if n > self.item_capacity {
            self.item_capacity = n.next_power_of_two();
            self.item_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("vanilla item vertices"),
                size: self.item_capacity * std::mem::size_of::<VanillaVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if n > self.quad_index_capacity {
            (self.quad_indices, self.quad_index_capacity) = quad_index_buffer(device, queue, self.item_capacity);
        }
        if !vertices.is_empty() {
            let voll: Vec<VanillaVertex> = vertices
                .iter()
                .map(|v| VanillaVertex { uv1f: v.uv1.map(|x| x as f32), uv2f: v.uv2.map(|x| x as f32), ..*v })
                .collect();
            queue.write_buffer(&self.item_buffer, 0, bytemuck::cast_slice(&voll));
        }
        self.item_count = vertices.len() as u32 / 4 * 4;
    }

    // Die Items mit dem Pack-Programm. Danach muss der Aufrufer seine eigenen Bindungen neu
    // setzen (Gruppe 0 ist hier eine andere).
    pub fn draw_items<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) -> bool {
        let Some(pipeline) = &self.item_pipeline else { return false };
        if self.item_count == 0 {
            return false;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.item_buffer.slice(..self.item_count as u64 * std::mem::size_of::<VanillaVertex>() as u64));
        pass.set_index_buffer(self.quad_indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.item_count / 4 * 6, 0, 0..1);
        true
    }
}

// Indizes fuer `vertices` Vertices in Vierergruppen: je Viereck 0,1,2 und 2,3,0.
fn quad_index_buffer(device: &wgpu::Device, queue: &wgpu::Queue, vertices: u64) -> (wgpu::Buffer, u64) {
    let quads = vertices / 4;
    let mut indices: Vec<u32> = Vec::with_capacity(quads as usize * 6);
    for q in 0..quads as u32 {
        let b = q * 4;
        indices.extend_from_slice(&[b, b + 1, b + 2, b + 2, b + 3, b]);
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vanilla quad indices"),
        size: (indices.len().max(6) * 4) as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !indices.is_empty() {
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&indices));
    }
    (buffer, quads * 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightmap_ist_dunkel_ohne_licht_und_hell_bei_vollem() {
        let lm = lightmap_rgba(1.0, 0.5);
        let px = |block: usize, sky: usize| &lm[(sky * 16 + block) * 4..(sky * 16 + block) * 4 + 3];
        assert!(px(0, 0)[0] < 40, "{:?}", px(0, 0));
        assert!(px(15, 15)[0] > 240 && px(0, 15)[0] > 200, "{:?}", px(0, 15));
        // Blocklicht ist gelblich: Rot ueber Blau.
        assert!(px(10, 0)[0] > px(10, 0)[2]);
    }

    fn grenze(size: f64) -> world::world_border::WorldBorder {
        let mut b = world::world_border::WorldBorder::default();
        b.apply(world::world_border::BorderUpdate::Size(size));
        b
    }

    #[test]
    fn grenze_nur_in_sichtweite_und_nur_die_nahe_wand() {
        let b = grenze(200.0);
        // Mitten drin, alle Kanten 100 weit, Sichtweite 64: nichts.
        assert!(world_border_draw(&b, 1.0, glam::DVec3::new(0.0, 70.0, 0.0), 64.0).is_none());
        // 16 vor der Ostkante: nur die Ostwand, halbwegs durchsichtig.
        let d = world_border_draw(&b, 1.0, glam::DVec3::new(84.0, 70.0, 0.0), 64.0).expect("Grenze sichtbar");
        assert_eq!(d.vertices.len(), 4);
        let a = d.color[3];
        assert!((a - 0.75f32.powi(4)).abs() < 1e-6, "Deckkraft {a}");
        // Die Wand steht kamerarelativ bei x = 100 - 84 = 16.
        let x = d.vertices[0].position[0] + d.model_offset[0];
        assert_eq!(x, 16.0);
        // Blau im Stillstand.
        assert_eq!(&d.color[..3], &[0x20 as f32 / 255.0, 0xA0 as f32 / 255.0, 1.0]);
        // Draussen, aber nah: voll deckend.
        let d = world_border_draw(&b, 1.0, glam::DVec3::new(110.0, 70.0, 0.0), 64.0).expect("Grenze sichtbar");
        assert_eq!(d.color[3], 1.0);
        // Weit draussen: nichts.
        assert!(world_border_draw(&b, 1.0, glam::DVec3::new(300.0, 70.0, 0.0), 64.0).is_none());
    }

    #[test]
    fn grenze_in_der_ecke_zeigt_zwei_waende() {
        let b = grenze(200.0);
        let d = world_border_draw(&b, 1.0, glam::DVec3::new(90.0, 0.0, -90.0), 64.0).expect("Grenze sichtbar");
        assert_eq!(d.vertices.len(), 8);
    }

    #[test]
    fn vertex_ist_so_gross_wie_das_layout_annimmt() {
        assert_eq!(std::mem::size_of::<VanillaVertex>(), 80);
        assert_eq!(std::mem::offset_of!(VanillaVertex, uv1f), 64);
        assert_eq!(std::mem::offset_of!(VanillaVertex, uv1), 36);
        assert_eq!(std::mem::offset_of!(VanillaVertex, normal), 52);
    }
}
