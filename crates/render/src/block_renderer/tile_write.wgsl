// Animierte Kacheln in eine Mip-Stufe des Atlas schreiben (s. tiles.rs, TileWriter): je Kachel
// ein Viereck genau ueber ihren Pixeln, jedes Pixel liest sein RGBA unveraendert aus dem Puffer.
// Das Ziel ist die Rohsicht (Rgba8Unorm) des sRGB-Atlas -- die Bytes landen ohne Umrechnung.

// Pixel aller Kacheln dieses Aufrufs hintereinander, je Kachel Zeile fuer Zeile, ein u32 je
// Pixel (R im untersten Byte).
@group(0) @binding(0) var<storage, read> pixel: array<u32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    // Ursprung (x, y) in der Stufe, Kantenlaenge, erstes Pixel im Puffer.
    @location(0) @interpolate(flat) ziel: vec4<u32>,
}

@vertex
fn vs_tile(@builtin(vertex_index) i: u32, @location(0) ziel: vec4<u32>, @location(1) stufe: u32) -> VsOut {
    var ecken = array<vec2<u32>, 6>(vec2(0u, 0u), vec2(1u, 0u), vec2(1u, 1u), vec2(0u, 0u), vec2(1u, 1u), vec2(0u, 1u));
    let px = vec2<f32>(ziel.xy + ecken[i] * ziel.z) / f32(stufe);
    var out: VsOut;
    out.pos = vec4<f32>(px.x * 2.0 - 1.0, 1.0 - px.y * 2.0, 0.0, 1.0);
    out.ziel = ziel;
    return out;
}

@fragment
fn fs_tile(in: VsOut) -> @location(0) vec4<f32> {
    // Pixelmitte x + 0.5 -> abgeschnitten das Pixel selbst.
    let lokal = vec2<u32>(in.pos.xy) - in.ziel.xy;
    return unpack4x8unorm(pixel[in.ziel.w + lokal.y * in.ziel.z + lokal.x]);
}
