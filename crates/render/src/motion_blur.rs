// Bewegungsunschaerfe (Render-Tab, Motion Blur > Welt): jeder Pixel bekommt seine Bewegung auf
// dem Schirm seit dem vorigen Bild, und entlang dieser Bewegung wird verwischt (s.
// motion_blur.wgsl). Ruhende Welt: der Pixel wird ueber seine Tiefe in die Welt zurueckgerechnet
// und mit der Kamera des vorigen Bildes wieder projiziert. Bewegte Entities bringen ihre eigene
// Bewegung mit: jede ihrer Ecken kennt ihre Lage im vorigen Bild (s. BlockRenderer::
// render_entity_motion) -- so verwischt ein schneller Spieler auch, wenn man selbst stillsteht.
//
// Damit die Spur eines bewegten Objekts auch ueber den ruhenden Hintergrund zieht und nicht nur
// innerhalb seiner Umrisse, wird die groesste Geschwindigkeit je Kachel gesucht; jeder Pixel
// verwischt mit der groessten seiner Nachbarkacheln, und jede Probe zaehlt nach Tiefe und
// eigener Bewegung (Rekonstruktionsfilter, s. fs_gather).
//
// Ausnahme der eigene Spieler in der 3. Person: er fliegt mit der Kamera mit und steht im Bild
// still. Als ruhende Welt gerechnet verwischte er so stark wie der Boden in seiner Tiefe. Seine
// Pixel (eine Maske, s. BlockRenderer::render_own_mask) rechnen darum ohne den Schritt des
// Auges -- er bleibt scharf und zieht keine Schliere.
//
// Fuer die Latenz: kein Bildverlauf, kein Warten auf die GPU -- eine Kopie des Weltbilds und ein
// paar Vollbild-Durchgaenge nach der Welt, vor der Hand. Steht alles still, faellt es weg.

use std::time::Instant;

use glam::{DMat4, DVec3};

use crate::camera::Camera;

// Laengste Strecke als Anteil der Bildbreite: ein schneller Flick verwischt nicht das ganze Bild.
const MAX_ANTEIL: f32 = 0.25;
// Laengere Pause zwischen zwei Weltbildern (Menue, Ladebildschirm) bzw. groesserer Sprung des
// Auges (Teleport, Respawn): kein Verwischen ueber die Luecke.
const MAX_PAUSE_S: f64 = 0.25;
const MAX_SPRUNG_BLOECKE: f64 = 8.0;
// Kantenlaenge einer Kachel fuer die groesste Geschwindigkeit (Pixel). Weiter als eine Kachel
// in jede Richtung reicht die Spur eines Objekts nicht ueber den Hintergrund.
const KACHEL: u32 = 24;
// Maske des eigenen Spielers: 1 = er ist dort zu sehen.
pub const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
// Bewegung der Entities je Pixel (NDC-Strecke xy, z = 1 wo eins ist).
pub const OBJECT_MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const VELOCITY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

struct Ziele {
    texture: wgpu::Texture,
    mask_view: wgpu::TextureView,
    object_view: wgpu::TextureView,
    velocity_view: wgpu::TextureView,
    tile_view: wgpu::TextureView,
    velocity_group: wgpu::BindGroup,
    tile_group: wgpu::BindGroup,
    gather_group: wgpu::BindGroup,
    size: (u32, u32),
}

struct Vorher {
    eye: DVec3,
    view_proj: DMat4,
    at: Instant,
}

pub struct MotionBlur {
    velocity_layout: wgpu::BindGroupLayout,
    tile_layout: wgpu::BindGroupLayout,
    gather_layout: wgpu::BindGroupLayout,
    velocity_pipeline: wgpu::RenderPipeline,
    tile_pipeline: wgpu::RenderPipeline,
    gather_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    // Kopie des Weltbilds und die Zwischenbilder samt Bindungen -- je Bildgroesse neu.
    ziele: Option<Ziele>,
    vorher: Option<Vorher>,
    // Geglaettete Zeit zwischen zwei Weltbildern in Sekunden (0 = noch keine).
    bildzeit_s: f64,
}

fn eintrag(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None }
}

fn textur(filterable: bool) -> wgpu::BindingType {
    wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }
}

const TIEFE: wgpu::BindingType = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
const UNIFORM: wgpu::BindingType = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None };

impl MotionBlur {
    // `format`: das Format des Weltbilds (offscreen_texture), in das zurueckgeschrieben wird.
    #[must_use]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("motion blur shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("motion_blur.wgsl").into()),
        });
        let layout_aus = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries });
        let velocity_layout = layout_aus("motion blur velocity layout", &[eintrag(1, TIEFE), eintrag(3, UNIFORM), eintrag(4, textur(false)), eintrag(5, textur(false))]);
        let tile_layout = layout_aus("motion blur tile layout", &[eintrag(3, UNIFORM), eintrag(6, textur(false))]);
        let gather_layout = layout_aus(
            "motion blur gather layout",
            &[
                eintrag(0, textur(true)),
                eintrag(1, TIEFE),
                eintrag(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                eintrag(3, UNIFORM),
                eintrag(6, textur(false)),
                eintrag(7, textur(false)),
            ],
        );
        let pipeline = |label: &str, layout: &wgpu::BindGroupLayout, fs: &str, ziel: wgpu::TextureFormat| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[layout], push_constant_ranges: &[] });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs_fullscreen", compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: fs,
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: ziel, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let velocity_pipeline = pipeline("motion blur velocity pipeline", &velocity_layout, "fs_velocity", VELOCITY_FORMAT);
        let tile_pipeline = pipeline("motion blur tile pipeline", &tile_layout, "fs_tilemax", VELOCITY_FORMAT);
        let gather_pipeline = pipeline("motion blur pipeline", &gather_layout, "fs_gather", format);
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
            size: 224,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { velocity_layout, tile_layout, gather_layout, velocity_pipeline, tile_pipeline, gather_pipeline, sampler, params, ziele: None, vorher: None, bildzeit_s: 0.0 }
    }

    // Vergessen, wo die Kamera stand (abgeschaltet, Shader-Pfad): das naechste Bild nach dem
    // Wiedereinschalten verwischt nicht ueber die ganze Zwischenzeit.
    pub fn reset(&mut self) {
        self.vorher = None;
        self.bildzeit_s = 0.0;
    }

    fn ziele(&mut self, device: &wgpu::Device, color: &wgpu::Texture, depth: &wgpu::TextureView) {
        let size = (color.width(), color.height());
        if self.ziele.as_ref().is_some_and(|z| z.size == size) {
            return;
        }
        let bild = |label: &str, w: u32, h: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let ziel = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT;
        let texture = bild("motion blur source", size.0, size.1, color.format(), wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let ansicht = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let view = ansicht(&texture);
        let mask_view = ansicht(&bild("motion blur own player mask", size.0, size.1, MASK_FORMAT, ziel));
        let object_view = ansicht(&bild("motion blur object motion", size.0, size.1, OBJECT_MOTION_FORMAT, ziel));
        let velocity_view = ansicht(&bild("motion blur velocity", size.0, size.1, VELOCITY_FORMAT, ziel));
        let tile_view = ansicht(&bild("motion blur tile max", size.0.div_ceil(KACHEL), size.1.div_ceil(KACHEL), VELOCITY_FORMAT, ziel));
        let gruppe = |label: &str, layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry]| device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries });
        let tv = wgpu::BindingResource::TextureView;
        // Die Tiefe entsteht bei derselben Groessenaenderung neu (BlockRenderer::resize), die
        // Bindungen haengen also an derselben Bedingung.
        let velocity_group = gruppe(
            "motion blur velocity bind group",
            &self.velocity_layout,
            &[
                wgpu::BindGroupEntry { binding: 1, resource: tv(depth) },
                wgpu::BindGroupEntry { binding: 3, resource: self.params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: tv(&mask_view) },
                wgpu::BindGroupEntry { binding: 5, resource: tv(&object_view) },
            ],
        );
        let tile_group = gruppe(
            "motion blur tile bind group",
            &self.tile_layout,
            &[wgpu::BindGroupEntry { binding: 3, resource: self.params.as_entire_binding() }, wgpu::BindGroupEntry { binding: 6, resource: tv(&velocity_view) }],
        );
        let gather_group = gruppe(
            "motion blur bind group",
            &self.gather_layout,
            &[
                wgpu::BindGroupEntry { binding: 0, resource: tv(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: tv(depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: self.params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: tv(&velocity_view) },
                wgpu::BindGroupEntry { binding: 7, resource: tv(&tile_view) },
            ],
        );
        self.ziele = Some(Ziele { texture, mask_view, object_view, velocity_view, tile_view, velocity_group, tile_group, gather_group, size });
    }

    // Verwischt `color` (das Weltbild, `color_view` seine Ansicht) nach der Bewegung seit dem
    // letzten Aufruf. `renderer` liefert die Tiefe des Welt-Durchgangs, die Maske des eigenen
    // Spielers und die Bewegung der Entities.
    //
    // `strength` 1.0 = die Bewegung waehrend eines Monitorbilds (`display_interval_s`), mittig
    // um den Pixel verteilt -- fuer die Bewegung mit der Kamera; `entity_strength` ebenso fuer
    // die eigene Bewegung der Entities (unabhaengig, 0 = sie verwischen nur mit der Kamera). Laeuft das Spiel schneller als der Monitor, wird die Strecke eines
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
        entity_strength: f32,
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
        let objekte = entity_strength > 0.0 && (renderer.has_entity_motion() || renderer.has_particle_motion());
        let kamera = strength > 0.0 && !reproject.abs_diff_eq(DMat4::IDENTITY, 1e-7);
        if !kamera && !(objekte && (renderer.entities_moving() || renderer.particles_moving())) {
            return false;
        }
        // Fuer den eigenen Spieler: er haengt am Auge, nur die Drehung (und in der 3. Person der
        // Kameraversatz um ihn herum) zaehlt.
        let reproject_own = vorher.view_proj * view_proj.inverse();
        let inv_proj = camera.proj_matrix(aspect).inverse();
        let depth = renderer.depth_view();
        let depth_lesen = renderer.depth_sample_view();
        let skala = (f64::from(display_interval_s) / self.bildzeit_s.max(1e-4)).max(1.0);
        let (faktor, faktor_objekte) = (f64::from(strength) * skala, f64::from(entity_strength) * skala);
        self.ziele(device, color, depth_lesen);
        let Some(ziele) = self.ziele.as_ref() else { return false };
        let size = ziele.size;

        let eigener = renderer.has_own_player();
        let mut daten = [0.0f32; 56];
        daten[..16].copy_from_slice(&reproject.as_mat4().to_cols_array());
        daten[16..32].copy_from_slice(&reproject_own.as_mat4().to_cols_array());
        daten[32..48].copy_from_slice(&inv_proj.to_cols_array());
        daten[48..52].copy_from_slice(&[faktor as f32, MAX_ANTEIL, size.0 as f32, size.1 as f32]);
        daten[52..56].copy_from_slice(&[if eigener { 1.0 } else { 0.0 }, KACHEL as f32, if objekte { 1.0 } else { 0.0 }, faktor_objekte as f32]);
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&daten));
        // Erst jetzt, wo sicher gezeichnet wird, Zeitstempel holen: vom Anfang des ersten
        // Durchgangs bis zum Ende der Unschaerfe.
        let stempel = reserve_timestamps();
        let mut erster = true;
        let mut stempel_von = |ende: bool| {
            let anfang = std::mem::take(&mut erster);
            stempel.as_ref().filter(|_| anfang || ende).map(|(qs, b, e)| wgpu::RenderPassTimestampWrites {
                query_set: qs,
                beginning_of_pass_write_index: anfang.then_some(*b),
                end_of_pass_write_index: ende.then_some(*e),
            })
        };
        let leer = |view| wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } };
        // Maske des eigenen Spielers und Bewegung der Entities: gegen die Tiefe der Welt, nur gelesen.
        let mit_tiefe = || wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
            stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
        };
        if eigener {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("motion blur own player mask"),
                color_attachments: &[Some(leer(&ziele.mask_view))],
                depth_stencil_attachment: Some(mit_tiefe()),
                timestamp_writes: stempel_von(false),
                occlusion_query_set: None,
            });
            renderer.render_own_mask(&mut pass);
        }
        if objekte {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("motion blur entity motion"),
                color_attachments: &[Some(leer(&ziele.object_view))],
                depth_stencil_attachment: Some(mit_tiefe()),
                timestamp_writes: stempel_von(false),
                occlusion_query_set: None,
            });
            renderer.render_entity_motion(queue, &mut pass, vorher.view_proj.as_mat4());
            renderer.render_ghost_motion(queue, &mut pass, vorher.view_proj.as_mat4());
            renderer.render_particle_motion(queue, &mut pass, vorher.view_proj.as_mat4());
        }
        let vollbild = |encoder: &mut wgpu::CommandEncoder, label: &str, ziel: &wgpu::TextureView, laden: bool, pipeline: &wgpu::RenderPipeline, gruppe: &wgpu::BindGroup, stempel: Option<wgpu::RenderPassTimestampWrites<'_>>| {
            let load = if laden { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: ziel, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: stempel,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, gruppe, &[]);
            pass.draw(0..3, 0..1);
        };
        vollbild(encoder, "motion blur velocity", &ziele.velocity_view, false, &self.velocity_pipeline, &ziele.velocity_group, stempel_von(false));
        vollbild(encoder, "motion blur tile max", &ziele.tile_view, false, &self.tile_pipeline, &ziele.tile_group, stempel_von(false));
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture { texture: color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyTexture { texture: &ziele.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        vollbild(encoder, "motion blur pass", color_view, true, &self.gather_pipeline, &ziele.gather_group, stempel_von(true));
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
