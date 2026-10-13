// Shader-Pfad, Tonemap: HDR-Bild -> Bildschirmformat (s. shader_path.rs).
//
// Kurve: die Hochlicht-Kompression aus Khronos "PBR Neutral" (aus der veroeffentlichten
// Beschreibung nachgebaut). Farben bis 0,76 bleiben unveraendert, darueber wird zur 1 hin
// weich zusammengedrueckt und leicht entsaettigt -- das Bild sieht ohne helle Lichtquellen
// aus wie im PvP-Pfad, spaetere helle Effekte (Bloom, Sonne) brennen aber nicht hart aus.
// Den Fuss-Versatz des Originals (zieht 0,04 ab) lassen wir weg: er gleicht dort den
// 4-%-Glanzanteil physikalischer Materialien aus, den es hier nicht gibt -- mit ihm wuerde
// alles sichtbar dunkler.

struct TonemapParams {
    // x = Belichtung (Faktor vor der Kurve).
    exposure: vec4<f32>,
};

@group(0) @binding(0) var hdr_tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: TonemapParams;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn pbr_neutral(c_in: vec3<f32>) -> vec3<f32> {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    var c = c_in;
    let peak = max(c.r, max(c.g, c.b));
    if (peak < start) {
        return c;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c = c * (new_peak / peak);
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(c, vec3<f32>(new_peak), g);
}

@fragment
fn fs_tonemap(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let hdr = textureLoad(hdr_tex, vec2<i32>(p.xy), 0).rgb;
    return vec4<f32>(pbr_neutral(hdr * params.exposure.x), 1.0);
}
