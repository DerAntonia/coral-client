// Verlaufshimmel fuer den Leistungs-Pfad (s. gradient_sky.wgsl): ein Vollbild-Dreieck, das
// jedes Pixel aus seiner Blickrichtung faerbt. Gezeichnet vor Fernsicht und Gelaende, statt
// das Bild mit der Himmelsfarbe zu loeschen.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

// Was der Himmel in einem Frame zeigt.
#[derive(Clone, Copy, Debug)]
pub struct GradientFrame {
    // Himmelsfarbe der Tageszeit, linear.
    pub sky: [f32; 3],
    // 1 = klar, 0 = voller Regen (nimmt die blaue Stunde weg).
    pub clear: f32,
    // Sonnenstand (world::sky::sky_angle, 0 = Mittag).
    pub sky_angle: f32,
    pub sun: bool,
    pub moon: bool,
    pub stars: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    inv_view_proj: [[f32; 4]; 4],
    sky: [f32; 4],
    sun_dir: [f32; 4],
    sun_axis: [f32; 4],
    extra: [f32; 4],
}

pub struct GradientSky {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
}

impl GradientSky {
    // `format`: das Format der Ansicht, in die gezeichnet wird (die sRGB-Ansicht des Bildes).
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gradient sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gradient_sky.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gradient sky"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gradient sky"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gradient sky"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gradient sky"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gradient sky"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: "vs_sky", compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs_sky",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self { pipeline, uniforms, group }
    }

    // `view_proj`: Projektion mal reine Blickdrehung (ohne Verschiebung).
    pub fn draw<'a>(&'a self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'a>, view_proj: Mat4, frame: &GradientFrame) {
        let sonne = crate::shader_path::celestial_light(frame.sky_angle).sun;
        let achse: Vec3 = crate::shader_path::sun_path_axis();
        let params = Params {
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            sky: [frame.sky[0], frame.sky[1], frame.sky[2], frame.clear],
            sun_dir: [sonne.x, sonne.y, sonne.z, frame.sun as u32 as f32],
            sun_axis: [achse.x, achse.y, achse.z, frame.moon as u32 as f32],
            extra: [frame.stars as u32 as f32, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&params));
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn shader_ist_gueltig() {
        let quelle = include_str!("gradient_sky.wgsl");
        let modul = naga::front::wgsl::parse_str(quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(quelle)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&modul)
            .expect("gradient_sky.wgsl ungueltig");
    }
}
