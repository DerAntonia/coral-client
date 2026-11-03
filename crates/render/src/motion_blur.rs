// Bewegungsunschaerfe der Kamera (Render-Tab, Motion Blur): jeder Pixel wird ueber seine Tiefe
// in die Welt zurueckgerechnet und mit der Kamera des vorigen Bildes wieder projiziert -- die
// Strecke dazwischen ist seine Bewegung auf dem Schirm, entlang der der Shader mittelt (s.
// motion_blur.wgsl). Nur die Kamera zaehlt: was sich selbst bewegt (Entities), verwischt allein
// mit der Kamerabewegung.
//
// Ausnahme der eigene Spieler in der 3. Person: er fliegt mit der Kamera mit und steht im Bild
// still. Als ruhende Welt gerechnet verwischte er so stark wie der Boden in seiner Tiefe. Seine
// Pixel (eine Maske, s. BlockRenderer::render_own_mask) rechnen darum ohne den Schritt des
// Auges, und der Hintergrund mittelt nicht ueber ihn hinweg -- er bleibt scharf und zieht keine
// Schliere.
//
// Fuer die Latenz: kein Bildverlauf, kein Warten auf die GPU -- eine Kopie des Weltbilds und ein
// Vollbild-Durchgang nach der Welt, vor der Hand. Steht die Kamera still, faellt beides weg.

use std::time::Instant;

use glam::{DMat4, DVec3};

use crate::camera::Camera;

// Laengste Strecke als Anteil der Bildbreite: ein schneller Flick verwischt nicht das ganze Bild.
const MAX_ANTEIL: f32 = 0.25;
// Laengere Pause zwischen zwei Weltbildern (Menue, Ladebildschirm) bzw. groesserer Sprung des
// Auges (Teleport, Respawn): kein Verwischen ueber die Luecke.
const MAX_PAUSE_S: f64 = 0.25;
const MAX_SPRUNG_BLOECKE: f64 = 8.0;
// Maske des eigenen Spielers: 1 = er ist dort zu sehen.
pub const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

struct Quelle {
    texture: wgpu::Texture,
    mask_view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

struct Vorher {
    eye: DVec3,
    view_proj: DMat4,
    at: Instant,
}

pub struct MotionBlur {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    // Kopie des Weltbilds, aus der gelesen wird, samt Bindung -- je Bildgroesse neu.
    quelle: Option<Quelle>,
    vorher: Option<Vorher>,
    // Geglaettete Zeit zwischen zwei Weltbildern in Sekunden (0 = noch keine).
    bildzeit_s: f64,
}

impl MotionBlur {
    // `format`: das Format des Weltbilds (offscreen_texture), in das zurueckgeschrieben wird.
    #[must_use]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("motion blur shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("motion_blur.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("motion blur bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("motion blur pipeline layout"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("motion blur pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_fullscreen", compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_blur",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("motion blur sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("motion blur params"),
            size: 160,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { layout, pipeline, sampler, params, quelle: None, vorher: None, bildzeit_s: 0.0 }
    }

    // Vergessen, wo die Kamera stand (abgeschaltet, Shader-Pfad): das naechste Bild nach dem
    // Wiedereinschalten verwischt nicht ueber die ganze Zwischenzeit.
    pub fn reset(&mut self) {
        self.vorher = None;
        self.bildzeit_s = 0.0;
    }

    // Verwischt `color` (das Weltbild, `color_view` seine Ansicht) nach der Kamerabewegung seit
    // dem letzten Aufruf. `depth`: die Tiefe des Welt-Durchgangs, `renderer` zeichnet daraus die
    // Maske des eigenen Spielers.
    //
    // `strength` 1.0 = die Bewegung waehrend eines Monitorbilds (`display_interval_s`), mittig
    // um den Pixel verteilt. Laeuft das Spiel schneller als der Monitor, wird die Strecke eines
    // Bildes entsprechend verlaengert -- sonst wuerde die Unschaerfe mit steigender Bildrate
    // immer schwaecher, obwohl der Monitor jedes Bild gleich lange zeigt.
    //
    // `reserve_timestamps` holt die GPU-Zeitstempel (Abfrage-Satz, Anfang, Ende) erst, wenn
    // wirklich gezeichnet wird -- sonst stuenden ungeschriebene Stempel als 0 ms in der Messung.
    // Rueckgabe: ob verwischt wurde.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::Texture,
        color_view: &wgpu::TextureView,
        renderer: &crate::BlockRenderer,
        camera: &Camera,
        aspect: f32,
        strength: f32,
        display_interval_s: f32,
        reserve_timestamps: &mut dyn FnMut() -> Option<(std::sync::Arc<wgpu::QuerySet>, u32, u32)>,
    ) -> bool {
        let jetzt = Instant::now();
        let view_proj = camera.view_proj_matrix(aspect).as_dmat4();
        let eye = camera.position;
        let Some(vorher) = self.vorher.replace(Vorher { eye, view_proj, at: jetzt }) else { return false };
        let dt = jetzt.duration_since(vorher.at).as_secs_f64();
        if dt > MAX_PAUSE_S || eye.distance(vorher.eye) > MAX_SPRUNG_BLOECKE {
            return false;
        }
        self.bildzeit_s = if self.bildzeit_s > 0.0 { self.bildzeit_s * 0.9 + dt * 0.1 } else { dt };
        let reproject = reproject_matrix(&vorher, eye, view_proj);
        if reproject.abs_diff_eq(DMat4::IDENTITY, 1e-7) {
            return false;
        }
        // Fuer den eigenen Spieler: er haengt am Auge, nur die Drehung (und in der 3. Person der
        // Kameraversatz um ihn herum) zaehlt.
        let reproject_own = vorher.view_proj * view_proj.inverse();
        let depth = renderer.depth_view();
        let faktor = f64::from(strength) * (f64::from(display_interval_s) / self.bildzeit_s.max(1e-4)).max(1.0);

        let size = (color.width(), color.height());
        if self.quelle.as_ref().is_none_or(|q| q.size != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("motion blur source"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: color.format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mask = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("motion blur own player mask"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: MASK_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
            // Die Tiefe entsteht bei derselben Groessenaenderung neu (BlockRenderer::resize),
            // die Bindung haengt also an derselben Bedingung.
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("motion blur bind group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: self.params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&mask_view) },
                ],
            });
            self.quelle = Some(Quelle { texture, mask_view, bind_group, size });
        }
        let Some(quelle) = self.quelle.as_ref() else { return false };

        let eigener = renderer.has_own_player();
        let mut daten = [0.0f32; 40];
        daten[..16].copy_from_slice(&reproject.as_mat4().to_cols_array());
        daten[16..32].copy_from_slice(&reproject_own.as_mat4().to_cols_array());
        daten[32..36].copy_from_slice(&[faktor as f32, MAX_ANTEIL, size.0 as f32, size.1 as f32]);
        daten[36] = if eigener { 1.0 } else { 0.0 };
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&daten));
        // Erst jetzt, wo sicher gezeichnet wird, Zeitstempel holen: vom Anfang des ersten
        // Durchgangs (Maske oder Unschaerfe) bis zum Ende der Unschaerfe.
        let stempel = reserve_timestamps();
        let stempel_von = |anfang: bool, ende: bool| {
            stempel.as_ref().map(|(qs, b, e)| wgpu::RenderPassTimestampWrites {
                query_set: qs,
                beginning_of_pass_write_index: anfang.then_some(*b),
                end_of_pass_write_index: ende.then_some(*e),
            })
        };
        if eigener {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("motion blur own player mask"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &quelle.mask_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: stempel_von(true, false),
                occlusion_query_set: None,
            });
            renderer.render_own_mask(&mut pass);
        }
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture { texture: color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyTexture { texture: &quelle.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("motion blur pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: stempel_von(!eigener, true),
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &quelle.bind_group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

// Bildschirm dieses Bildes (NDC samt Tiefe) -> Clip-Raum des vorigen. Gerendert wird relativ zum
// Auge (s. Camera::view_matrix): ein Punkt dieses Bildes liegt im Raum des vorigen um die
// Augenbewegung verschoben.
fn reproject_matrix(vorher: &Vorher, eye: DVec3, view_proj: DMat4) -> DMat4 {
    vorher.view_proj * DMat4::from_translation(eye - vorher.eye) * view_proj.inverse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec4;

    #[test]
    fn shader_ist_gueltig() {
        let quelle = include_str!("motion_blur.wgsl");
        let modul = naga::front::wgsl::parse_str(quelle).unwrap_or_else(|e| panic!("{}", e.emit_to_string(quelle)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&modul)
            .expect("motion_blur.wgsl ungueltig");
    }

    fn kamera(position: DVec3, yaw: f32, pitch: f32) -> Camera {
        let mut c = Camera::new(position);
        c.yaw = yaw;
        c.pitch = pitch;
        c
    }

    // Ein fester Weltpunkt, im jetzigen Bild gesehen, landet nach der Rueckrechnung genau dort,
    // wo die vorige Kamera ihn gesehen hat -- weit weg vom Nullpunkt, mit Drehung und Schritt.
    #[test]
    fn rueckrechnung_trifft_die_vorige_lage() {
        let aspect = 16.0 / 9.0;
        let alt = kamera(DVec3::new(-186_634.3, 70.62, 2_000_123.8), 30.0, -10.0);
        let neu = kamera(alt.position + DVec3::new(0.21, -0.05, 0.13), 33.5, -8.0);
        let vorher = Vorher { eye: alt.position, view_proj: alt.view_proj_matrix(aspect).as_dmat4(), at: Instant::now() };
        let view_proj = neu.view_proj_matrix(aspect).as_dmat4();
        let m = reproject_matrix(&vorher, neu.position, view_proj);
        let welt = neu.position + neu.view_forward().as_dvec3() * 6.0 + DVec3::new(0.8, 0.5, -0.3);
        let ndc = |vp: DMat4, eye: DVec3| {
            let c = vp * (welt - eye).extend(1.0);
            c / c.w
        };
        let jetzt = ndc(view_proj, neu.position);
        assert!(jetzt.x.abs() < 1.0 && jetzt.y.abs() < 1.0 && (0.0..1.0).contains(&jetzt.z), "Punkt nicht im Bild: {jetzt}");
        let erwartet = ndc(vorher.view_proj, alt.position);
        let r = m * DVec4::new(jetzt.x, jetzt.y, jetzt.z, 1.0);
        let r = r / r.w;
        assert!((r.x - erwartet.x).abs() < 1e-4 && (r.y - erwartet.y).abs() < 1e-4, "{r} statt {erwartet}");
    }

    // Ohne Bewegung ist die Rueckrechnung die Einheit -- encode laesst den Durchgang dann weg.
    #[test]
    fn stillstand_ist_die_einheit() {
        let c = kamera(DVec3::new(12.5, 64.0, -3.25), 90.0, 20.0);
        let vp = c.view_proj_matrix(1.5).as_dmat4();
        let m = reproject_matrix(&Vorher { eye: c.position, view_proj: vp, at: Instant::now() }, c.position, vp);
        assert!(m.abs_diff_eq(DMat4::IDENTITY, 1e-7));
    }
}
