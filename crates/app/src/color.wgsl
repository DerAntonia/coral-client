// Modul "Farbe" (s. ModuleId::Color): traegt das fertige Bild (offscreen_view, Welt + HUD)
// statt per copy_texture_to_texture ueber diesen Durchgang in die Swapchain-Textur und
// veraendert dabei die Saettigung. 0 = schwarzweiss, 1 = unveraendert, darueber kraeftiger.
//
// Gerechnet wird LINEAR: offscreen_view ist sRGB, das Lesen liefert also lineare Werte, und
// das sRGB-Renderziel kodiert beim Schreiben wieder zurueck (wie cmaa2_blit.wgsl). Die
// Helligkeit eines Pixels ist die Rec.-709-Luminanz -- Gruen zaehlt am meisten, Blau am
// wenigsten, so bleibt ein schwarzweisses Bild so hell, wie das Auge das farbige empfindet.
//
// Kosten: ein Lesen und Schreiben je Pixel, also etwa so viel wie die Kopie, die er ersetzt
// -- und er laeuft nur, wenn das Modul an ist und die Saettigung nicht 100 % ist. Gelesen
// wird per textureLoad am exakten Pixel, ohne Sampler und Filterung.
@group(0) @binding(0) var src: texture_2d<f32>;

// 16 Byte: Uniform-Bloecke muessen in WGSL ein Vielfaches von 16 gross sein. Aufgefuellt
// mit drei einzelnen f32 -- ein vec3 waere auf 16 ausgerichtet und machte daraus 32 Byte.
struct ColorSettings {
    saturation: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};
@group(0) @binding(1) var<uniform> settings: ColorSettings;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// Fullscreen-Dreieck ohne Vertex-Puffer, wie in cmaa2_blit.wgsl.
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let uv = vec2<f32>(f32((vertex_index << 1u) & 2u), f32(vertex_index & 2u));
    out.clip_position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(in.clip_position.xy), 0);
    let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let rgb = clamp(mix(vec3<f32>(luma), c.rgb, settings.saturation), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, c.a);
}
