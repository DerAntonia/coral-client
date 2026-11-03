// contrast_color (Gegenfarbe fuer Fadenkreuz und Blockumriss) auf der echten GPU: Stichproben
// treffen ihre Farbe, und auf einem Gitter durch den ganzen Farbraum ist das Ergebnis immer Gelb
// oder Blau und deutlich heller oder dunkler als der Untergrund. Ohne GPU wird nichts geprueft.
use wgpu::util::DeviceExt;

const GELB: [f32; 3] = [1.0, 1.0, 0.0];
const BLAU: [f32; 3] = [0.0, 0.0, 1.0];

// Untergrund (sRGB) -> erwartete Farbe.
const STICHPROBEN: [([u8; 3], [f32; 3]); 9] = [
    ([0, 0, 0], GELB),
    ([255, 255, 255], BLAU),
    ([128, 128, 128], GELB),
    ([255, 0, 0], GELB),
    ([0, 255, 0], BLAU),
    ([0, 0, 255], GELB),
    ([255, 255, 0], BLAU),
    ([255, 0, 255], GELB),
    ([135, 165, 195], BLAU),
];

fn linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

// Relative Leuchtdichte aus linearem RGB.
fn leuchtdichte(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn rechnen(device: &wgpu::Device, queue: &wgpu::Queue, eingabe: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let quelle = format!(
        "{}\n{}",
        "@group(0) @binding(0) var<storage, read> eingabe: array<vec4<f32>>;
         @group(0) @binding(1) var<storage, read_write> ausgabe: array<vec4<f32>>;
         @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
             if (id.x < arrayLength(&eingabe)) {
                 ausgabe[id.x] = vec4<f32>(contrast_color(eingabe[id.x].rgb), 1.0);
             }
         }",
        render::CONTRAST_COLOR_WGSL
    );
    let modul = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(quelle.into()) });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &modul,
        entry_point: "main",
        compilation_options: Default::default(),
        cache: None,
    });
    let groesse = std::mem::size_of_val(eingabe) as u64;
    let ein = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(eingabe), usage: wgpu::BufferUsages::STORAGE });
    let aus = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: groesse, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
    let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: groesse, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let bindung = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: ein.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: aus.as_entire_binding() },
        ],
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindung, &[]);
        pass.dispatch_workgroups((eingabe.len() as u32).div_ceil(64), 1, 1);
    }
    enc.copy_buffer_to_buffer(&aus, 0, &lese, 0, groesse);
    queue.submit([enc.finish()]);
    lese.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let daten = bytemuck::cast_slice(&lese.slice(..).get_mapped_range()).to_vec();
    daten
}

#[test]
fn gelb_oder_blau_und_ueberall_hell_dunkel_kontrast() {
    let instance = wgpu::Instance::default();
    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();

    // Eingaben linear, wie aus der sRGB-Ansicht gelesen: erst die Stichproben, dann 16^3 Gitter.
    let lin = |c: [f32; 3]| c.map(linear);
    let mut untergruende: Vec<[f32; 3]> = STICHPROBEN.iter().map(|(u, _)| u.map(|v| f32::from(v) / 255.0)).collect();
    for r in 0..16u8 {
        for g in 0..16u8 {
            for b in 0..16u8 {
                untergruende.push([r, g, b].map(|v| f32::from(v * 17) / 255.0));
            }
        }
    }
    let eingabe: Vec<[f32; 4]> = untergruende.iter().map(|&c| { let l = lin(c); [l[0], l[1], l[2], 1.0] }).collect();
    let ausgabe: Vec<[f32; 3]> = rechnen(&device, &queue, &eingabe).iter().map(|a| [a[0], a[1], a[2]]).collect();

    for ((untergrund, ziel), aus) in STICHPROBEN.iter().zip(&ausgabe) {
        assert_eq!(aus, ziel, "Untergrund {untergrund:?}");
    }
    // Kontrast wie bei WCAG: (hell + 0.05) / (dunkel + 0.05). Am knappsten liegt er knapp ueber 2.2.
    let mut schlechtester = (f32::MAX, [0.0; 3]);
    for (c, aus) in untergruende.iter().zip(&ausgabe) {
        assert!(*aus == GELB || *aus == BLAU, "{c:?} -> {aus:?}");
        let (a, b) = (leuchtdichte(lin(*c)) + 0.05, leuchtdichte(*aus) + 0.05);
        let kontrast = a.max(b) / a.min(b);
        if kontrast < schlechtester.0 {
            schlechtester = (kontrast, *c);
        }
    }
    assert!(schlechtester.0 >= 2.2, "zu wenig Hell-Dunkel-Kontrast: {schlechtester:?}");
}
