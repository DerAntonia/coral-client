// Texturierte 2D-Quads der Oberflaeche (Hotbar-/Inventar-Sheets, Item-Icons). Wie
// font.wgsl aufgebaut, aber die Textur liefert echtes RGBA statt nur Deckung -- die
// Vertexfarbe ist hier eine Tonung (Wuerfelseiten-Schattierung, Abdunkeln von Slots),
// kein Ersatz fuer die Texturfarbe.

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@group(0) @binding(0) var atlas_texture: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_texture, atlas_sampler, in.uv);
    return texel * in.color;
}

// --- Verzauberungs-Glint ueber Item-Icons (s. render::glint, dort die 3D-Fassung) ---------
//
// Dasselbe Quad wie das Icon noch einmal: `uv` zeigt weiter ins Icon (Maske -- nur wo das
// Icon deckt), color.xy ist die Glint-UV (lokal 0..1 je Quad, schon mit dem Massstab
// multipliziert), color.z die Helligkeit des Icon-Quads (Toenung/Wuerfelseite), color.w
// seine Deckkraft.
struct GlintParams {
    row0: vec4<f32>,
    row1: vec4<f32>,
    // x = GlintAlpha (glintStrength).
    params: vec4<f32>,
};
@group(1) @binding(0) var glint_texture: texture_2d<f32>;
@group(1) @binding(1) var glint_sampler: sampler;
@group(1) @binding(2) var<uniform> glint: GlintParams;

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0031308)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow(max((c + 0.055) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_glint(in: VertexOutput) -> @location(0) vec4<f32> {
    // Texturzugriffe vor dem discard (gleichmaessiger Kontrollfluss).
    let maske = textureSample(atlas_texture, atlas_sampler, in.uv);
    // Flache Items aus dem UI-Atlas (color.x < 0): Glint-UV aus der ATLAS-UV wie in Vanilla
    // (ItemRenderer: Textur-UV mal Glint-Massstab) -- so laeuft das Muster ueber das ganze
    // Icon, auch wenn es aus vielen Pixel-Quads besteht. Ein 16-px-Sprite ueberspannt dabei
    // ein Viertel der Glint-Textur (s. render::glint::GLINT_SCALE_ITEM), also Atlaspixel / 64.
    // Sonst (Wuerfelseiten): die Quad-lokale UV aus shape_sprites.
    let atlas_px = vec2<f32>(textureDimensions(atlas_texture));
    let lokal = select(in.color.xy, in.uv * atlas_px / 64.0, in.color.x < 0.0);
    let p = vec3<f32>(lokal, 1.0);
    let guv = vec2<f32>(dot(glint.row0.xyz, p), dot(glint.row1.xyz, p));
    let c = textureSample(glint_texture, glint_sampler, guv);
    if (maske.a < 0.1) {
        discard;
    }
    // Wie in der Welt (s. render::glint): Vanilla hebt den gammakodierten Wert d des Icons auf
    // d + s^2; das lineare Ziel bekommt genau die Differenz.
    let s = linear_to_srgb(c.rgb) * glint.params.x;
    let d = linear_to_srgb(maske.rgb * in.color.z);
    return vec4<f32>((srgb_to_linear(min(d + s * s, vec3<f32>(1.0))) - srgb_to_linear(d)) * in.color.w, 0.0);
}
