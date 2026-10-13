// Kopiert die CMAA2-Ausgabetextur (linear, s. cmaa2.wgsl) in die Swapchain-Textur.
//
// Ein gewoehnliches copy_texture_to_texture geht hier nicht: die Swapchain-Textur ist
// sRGB-kodiert (surface_format), eine per STORAGE_BINDING beschreibbare Speicher-Textur darf
// das in WebGPU nicht sein -- deshalb liegt das CMAA2-Zwischenergebnis linear (Rgba8Unorm)
// vor, und dieser Fragment-Shader schreibt es in ein sRGB-Renderziel: die GPU kodiert beim
// Schreiben automatisch nach sRGB, genau wie beim gewoehnlichen Zeichnen der Welt in
// offscreen_texture. Keine manuelle Gammakorrektur noetig.
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Fullscreen-Dreieck aus drei Vertices ohne eigenen Vertex-Puffer -- das ueberstehende
// Drittel ausserhalb von [-1,1] wird vom Rasterizer automatisch verworfen. Standard-Trick,
// spart einen eigenen Vertex-Puffer fuer ein simples Rechteck.
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let uv = vec2<f32>(f32((vertex_index << 1u) & 2u), f32(vertex_index & 2u));
    out.uv = uv;
    out.clip_position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(src, src_sampler, in.uv);
}
