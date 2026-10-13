// Verzauberungs-Glint (der violette Schimmer) ueber 3D-Items: in der eigenen Hand und an
// Entities (Items in fremden Haenden, getragene Ruestung).
//
// Vanilla zeichnet das Item ein zweites Mal mit der Glint-Textur (RenderPipelines.GLINT):
// ohne Tiefenschreiben, mit der Mischung BlendFunction.GLINT (SRC_COLOR, ONE -- die
// Glintfarbe QUADRIERT addiert) und einer Texturmatrix, die mit der Zeit wandert
// (TextureTransform.getGlintTransformation). Genau so hier, mit drei Abweichungen:
//
//   1. Tiefentest LESS_EQUAL plus leichter Bias statt EQUAL. Items in der Welt laufen durch
//      vs_entity, der Glint durch vs_main -- zwei Einstiegspunkte, deren Tiefe nicht
//      bitgleich garantiert ist. Damit der Glint trotzdem nicht auf den DURCHSICHTIGEN Texeln
//      eines Sprites landet (dort hat das Item keine Tiefe geschrieben), sampelt der Glint-
//      Shader die Item-Textur mit und verwirft dieselben Texel wie das Item selbst.
//   2. Die Glint-UVs sind die LOKALEN Koordinaten der jeweiligen Textur (0..1 ueber das
//      Sprite bzw. die Modelltextur), vom Aufrufer schon mit Vanillas Massstab multipliziert
//      (s. GLINT_SCALE_*). Vanilla nimmt die Atlas-UVs -- die haengen an der Atlasgroesse.
//   3. Vanilla addiert im sRGB-Raum (d + s^2), unser Ziel ist aber ein sRGB-Framebuffer,
//      der LINEAR mischt. Eine blosse lineare Addition war sichtbar zu schwach (auf mittel-
//      hellen Items nur rund ein Drittel des Vanilla-Glints). Deshalb rechnet der Shader die
//      Farbe d des Items darunter selbst nach (Textur mal Schattierung) und gibt genau
//      lin(d + s^2) - lin(d) aus -- additiv auf das Item gelegt ergibt das Vanillas Wert.
//
// Die Vertices sind gewoehnliche BlockVertex, aber mit umgewidmeter Farbe (die der Glint
// nicht braucht): color.xy = Glint-UV, |color.z| = Schattierung des Items darunter, das
// VORZEICHEN von color.z = welche Glint-Textur (s. GLINT_TEXTURE_*).
// `uv` bleibt die Atlas-UV des Items -- fuer die Durchsichtigkeits-Maske aus Punkt 1.

use world::BlockVertex;

// Vanillas Glint-Massstaebe (TextureTransform: GLINT 8.0, ENTITY_GLINT 0.5,
// ARMOR_ENTITY_GLINT 0.16), umgerechnet auf lokale UVs.
//
// Items: Vanilla multipliziert die ATLAS-UV mit 8. Der Item-Atlas von 1.21.11 ist 512x512
// (atlases/items.json: 785 Texturen 16x16, 7 mit 32x32 und 64 Trim-Varianten -- zusammen
// rund 225000 Pixel, das passt gerade nicht mehr in 256x512), ein 16px-Sprite ueberspannt
// darin 16/512. In lokalen Koordinaten (0..1 je Sprite) also 8 * 16/512 = 0.25: ein Glint-
// Texel ist halb so gross wie ein Item-Pixel -- so steht es auch auf Vanilla-Screenshots.
// Ein Resourcepack mit groesseren Texturen vergroessert Vanillas Atlas und damit den Glint;
// das bildet dieser feste Wert nicht nach.
pub const GLINT_SCALE_ITEM: f32 = 8.0 * 16.0 / 512.0;
// Das Schild (RenderLayers.entityGlint, Massstab 0.5): auch hier nimmt Vanilla die ATLAS-
// UVs -- ShieldModelRenderer zeichnet ueber ein Sprite des Atlas shield_patterns
// (Sprite.getTextureSpecificVertexConsumer rechnet die Modell-UVs vor dem Glint auf den
// Atlas um). Der Atlas haelt 45 Texturen zu 64x64 (shield_base, shield_base_nopattern und
// 43 Muster), das sind 512x512; die Modelltextur ueberspannt darin 64/512.
pub const GLINT_SCALE_SHIELD: f32 = 0.5 * 64.0 / 512.0;
// Ruestung zeichnet Vanilla direkt mit der Modelltextur, ohne Atlas -- dort gilt die Zahl
// unveraendert.
pub const GLINT_SCALE_ARMOR: f32 = 0.16;

// color.z der Glint-Vertices: welche Textur. Items UND das Schild nehmen
// enchanted_glint_item.png, nur Ruestung enchanted_glint_armor.png: RenderLayers.GLINT und
// ENTITY_GLINT binden beide ITEM_ENCHANTMENT_GLINT, allein ARMOR_ENTITY_GLINT bindet
// ENTITY_ENCHANTMENT_GLINT -- der Name "entity" fuehrt hier in die Irre.
pub const GLINT_TEXTURE_ITEM: f32 = 1.0;
pub const GLINT_TEXTURE_ARMOR: f32 = -1.0;

// Die Glint-Twins zu einem schon ausgegebenen Item: `normal` sind seine Vertices mit den
// echten Atlas-UVs, `lokal` DIESELBE Geometrie noch einmal mit lokalen UVs (0..1 je Textur,
// ausgegeben mit dem Textur-Rechteck [0, 0, 1, 1]). Beide muessen aus demselben Aufruf mit
// derselben Transformation stammen -- dann stimmen sie Vertex fuer Vertex ueberein.
pub fn glint_vertices(normal: &[BlockVertex], lokal: &[BlockVertex], scale: f32, texture: f32, out: &mut Vec<BlockVertex>) {
    debug_assert_eq!(normal.len(), lokal.len(), "Glint braucht dieselbe Geometrie zweimal");
    out.extend(normal.iter().zip(lokal).map(|(n, l)| {
        // Getoente Ebenen (Leder) haben je Kanal eine andere Farbe -- der Mittelwert reicht
        // fuer die Helligkeitskorrektur. Nie ganz 0, sonst ginge das Vorzeichen verloren.
        let schatten = ((n.color[0] + n.color[1] + n.color[2]) / 3.0).max(1.0e-3);
        BlockVertex { position: n.position, color: [l.uv[0] * scale, l.uv[1] * scale, texture * schatten], uv: n.uv }
    }));
}

// Die Texturmatrix als zwei Zeilen einer 2x3-Affinen: uv' = (r0 . (u, v, 1), r1 . (u, v, 1)).
//
// TextureTransform.getGlintTransformation: l = ms * glintSpeed * 8, dann
// translation(-f, g) * rotateZ(pi/18) * scale -- der Massstab steckt schon in den Vertices.
// `speed` ist die Option glintSpeed (0..1, Vorgabe 0.5).
pub fn glint_matrix(ms: u64, speed: f32) -> [[f32; 4]; 2] {
    let l = (ms as f64 * speed as f64 * 8.0) as u64;
    let f = (l % 110_000) as f32 / 110_000.0;
    let g = (l % 30_000) as f32 / 30_000.0;
    let (s, c) = (std::f32::consts::PI / 18.0).sin_cos();
    [[c, -s, -f, 0.0], [s, c, g, 0.0]]
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlintUniform {
    pub rows: [[f32; 4]; 2],
    // x = Staerke (GlintAlpha), Rest Fuellung.
    pub params: [f32; 4],
}

pub(crate) fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("glint bind group layout"),
        entries: &[
            texture(0),
            texture(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
        ],
    })
}

// Eine Glint-Textur (RGBA8, sRGB-kodiert wie jede Vanilla-Textur).
pub fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8], width: u32, height: u32) -> wgpu::TextureView {
    let extent = wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glint texture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let weiss = [255u8; 4];
    let daten = if rgba.len() as u32 == extent.width * extent.height * 4 { rgba } else { &weiss[..] };
    let extent = if daten.len() == 4 { wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 } } else { extent };
    queue.write_texture(
        wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        daten,
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(extent.width * 4), rows_per_image: Some(extent.height) },
        extent,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

// Der Glint-Sampler: wiederholend (die Texturmatrix schiebt die UVs endlos weiter) und
// NEAREST. Die .mcmeta beider Texturen sagt zwar "blur": true, im Spiel haben die Glint-
// Streifen aber harte, treppenfoermige Kanten (am Vanilla-Screenshot nachgemessen) -- mit
// linearem Filter verschwammen sie.
pub fn create_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("glint sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

// Ein Vertexpuffer fuer Glint-Twins mit fester Kapazitaet (Hand, Welt-Entities, Figur).
pub(crate) struct GlintBuffer {
    buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
}

impl GlintBuffer {
    pub fn new(device: &wgpu::Device, label: &str, capacity: usize) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (std::mem::size_of::<BlockVertex>() * capacity) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer, capacity, count: 0 }
    }

    pub fn set(&mut self, queue: &wgpu::Queue, vertices: &[BlockVertex]) {
        let n = vertices.len().min(self.capacity);
        if n > 0 {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        self.count = n as u32;
    }

    // Zeichnet die Twins. Pipeline und Bindgroups 0..2 setzt der Aufrufer; Slot 1 braucht
    // den Null-Versatz, weil der Glint durch vs_main laeuft (s. block.wgsl).
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, zero_offset: &'a wgpu::Buffer) {
        if self.count == 0 {
            return;
        }
        let bytes = self.count as u64 * std::mem::size_of::<BlockVertex>() as u64;
        pass.set_vertex_buffer(0, self.buffer.slice(..bytes));
        pass.set_vertex_buffer(1, zero_offset.slice(..));
        pass.draw(0..self.count, 0..1);
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_wandert_mit_der_zeit_und_dreht_um_zehn_grad() {
        let null = glint_matrix(0, 0.5);
        assert_eq!(null[0][2], 0.0);
        assert_eq!(null[1][2], 0.0);
        assert!((null[0][0] - 10f32.to_radians().cos()).abs() < 1e-6);
        // l = ms * 4: nach 1000 ms ist l = 4000 -> f = 4000/110000, g = 4000/30000.
        let eins = glint_matrix(1000, 0.5);
        assert!((eins[0][2] - -4000.0 / 110_000.0).abs() < 1e-6);
        assert!((eins[1][2] - 4000.0 / 30_000.0).abs() < 1e-6);
        // Nach einer vollen g-Periode (30000/4 ms) steht g wieder auf 0.
        assert!(glint_matrix(7500, 0.5)[1][2].abs() < 1e-6);
        // Geschwindigkeit 0: steht still.
        assert_eq!(glint_matrix(123_456, 0.0)[0][2], 0.0);
    }

    #[test]
    fn twins_behalten_position_und_atlas_uv() {
        let n = BlockVertex { position: [1.0, 2.0, 3.0], color: [0.5; 3], uv: [0.25, 0.75] };
        let l = BlockVertex { position: [1.0, 2.0, 3.0], color: [0.5; 3], uv: [1.0, 0.5] };
        let mut out = Vec::new();
        glint_vertices(&[n], &[l], 0.5, GLINT_TEXTURE_ITEM, &mut out);
        assert_eq!(out[0].position, n.position);
        assert_eq!(out[0].uv, n.uv, "Atlas-UV bleibt fuer die Durchsichtigkeits-Maske");
        assert_eq!(out[0].color, [0.5, 0.25, 0.5], "z = Schattierung, positiv = Item-Textur");
        out.clear();
        glint_vertices(&[n], &[l], 0.5, GLINT_TEXTURE_ARMOR, &mut out);
        assert_eq!(out[0].color[2], -0.5, "negativ = Ruestungs-Textur");
    }
}
