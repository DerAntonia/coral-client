// Motion Blur und Hand Blur auf der echten GPU: alle Durchgaenge einmal mit Bewegung durchlaufen
// (Welt mit Kamera und bewegtem Entity, Hand aus Zwischenlagen). Geprueft wird, dass Pipelines,
// Bindungen und Ziele zusammenpassen -- wgpu bricht bei jedem Fehler ab. Ohne GPU nichts.
use glam::DVec3;
use render::{BlockRenderer, Camera};
use world::BlockVertex;

const W: u32 = 64;

fn dreieck(dx: f32, z: f32) -> Vec<BlockVertex> {
    [[-0.5 + dx, -0.5, z], [0.5 + dx, -0.5, z], [dx, 0.5, z]].into_iter().map(|position| BlockVertex { position, color: [1.0; 3], uv: [0.5, 0.5] }).collect()
}

#[test]
fn alle_durchgaenge_laufen() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("keine GPU -- nichts geprueft");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let weiss = vec![255u8; 16 * 16 * 4];
    let mut r = BlockRenderer::new(&device, &queue, format, W, W, std::slice::from_ref(&weiss), 16, &weiss, 16, &weiss, 16);
    r.keep_depth = true;
    let mut mb = render::motion_blur::MotionBlur::new(&device, format);
    let ziel = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = ziel.create_view(&Default::default());
    let mut verwischt = (false, false);
    for bild in 0..3 {
        let mut camera = Camera::new(DVec3::new(f64::from(bild) * 0.05, 0.0, 0.0));
        camera.yaw = bild as f32 * 2.0;
        r.update_camera(&queue, &camera, 1.0);
        let entity = dreieck(bild as f32 * 0.2, 3.0);
        r.set_entity_vertices(&queue, &entity, &[[0.0; 4]; 3], &[], (0, 0, 0));
        let vorher: Vec<[f32; 3]> = dreieck((bild as f32 - 1.0) * 0.2, 3.0).iter().map(|v| v.position).collect();
        r.set_entity_prev_positions(&queue, &vorher, true);
        r.set_hand_vertices(&queue, &dreieck(bild as f32 * 0.3, -2.0));
        r.update_hand_camera(&queue, 1.0);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(r.depth_attachment()),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            r.render(&mut pass, &mut |_, _| {});
        }
        let welt = mb.encode(&device, &queue, &mut encoder, &ziel, &view, &r, &camera, 1.0, 1.0, 1.0, 1.0 / 60.0, &mut || None);
        let hand = r.render_hand_blurred(&device, &queue, &mut encoder, &view, (W, W), 1.0, 1.0, 1.0 / 60.0, render::hand_blur::Curve::Ascending, None);
        queue.submit(Some(encoder.finish()));
        device.poll(wgpu::Maintain::Wait);
        verwischt = (verwischt.0 || welt, verwischt.1 || hand);
    }
    assert!(verwischt.0, "Welt nie verwischt");
    assert!(verwischt.1, "Hand nie verwischt");
}
