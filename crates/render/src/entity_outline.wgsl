// Leucht-Umriss (Glowing), die Schritte nach der Silhouette (s. entity_outline.rs):
//
//   1. Kanten: wo die Deckkraft der Silhouette zum Nachbarpixel springt, entsteht Rand. Die
//      Farbe ist der Mittelwert der Teamfarben ringsum (gewichtet mit ihrer Deckkraft).
//   2. Unschaerfe waagerecht, dann senkrecht: ein Rand von wenigen Pixeln Breite, weich.
//   3. Ueber das Weltbild legen, gemischt nach der Deckkraft des Randes.

struct Params {
    // Richtung der Unschaerfe (1,0) bzw. (0,1); bei den anderen Schritten unbenutzt.
    dir: vec2<f32>,
    // Groesse eines Pixels der Quelle in Texturkoordinaten.
    texel: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Ein Dreieck ueber den ganzen Bildschirm.
@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> FullscreenOut {
    let ecke = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FullscreenOut;
    out.position = vec4<f32>(ecke * 2.0 - 1.0, 0.0, 1.0);
    // Texturkoordinaten laufen nach unten, der Bildschirm nach oben.
    out.uv = vec2<f32>(ecke.x, 1.0 - ecke.y);
    return out;
}

fn hole(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src, src_sampler, uv, 0.0);
}

@fragment
fn fs_edges(in: FullscreenOut) -> @location(0) vec4<f32> {
    let dx = vec2<f32>(params.texel.x, 0.0);
    let dy = vec2<f32>(0.0, params.texel.y);
    let mitte = hole(in.uv);
    var n = array<vec4<f32>, 4>(hole(in.uv - dx), hole(in.uv + dx), hole(in.uv - dy), hole(in.uv + dy));
    var sprung = 0.0;
    var farbe = mitte.rgb * mitte.a;
    for (var k = 0; k < 4; k++) {
        sprung += abs(mitte.a - n[k].a);
        farbe += n[k].rgb * n[k].a;
    }
    return vec4<f32>(farbe * 0.2, clamp(sprung, 0.0, 1.0));
}

// Fuenf Pixel breit ueber drei bilineare Zugriffe: zwei, die je zwei Pixel mitteln, und der
// aeusserste mit halbem Gewicht. Die Farbe wird gemittelt, die Deckkraft aufsummiert -- so
// bleibt der Rand nach dem Verwischen kraeftig.
@fragment
fn fs_blur(in: FullscreenOut) -> @location(0) vec4<f32> {
    let schritt = params.dir * params.texel;
    let summe = hole(in.uv - schritt * 1.5) + hole(in.uv + schritt * 0.5) + hole(in.uv + schritt * 2.0) * 0.5;
    return vec4<f32>(summe.rgb / 2.5, summe.a);
}

@fragment
fn fs_composite(in: FullscreenOut) -> @location(0) vec4<f32> {
    return hole(in.uv);
}
