// Hand-Unschaerfe, s. hand_blur.rs: die aufsummierten Teilbilder der Hand (vormultipliert, Alpha =
// Deckung) ueber das Weltbild legen.

@group(0) @binding(0) var accum: texture_2d<f32>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_composite(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let a = textureLoad(accum, vec2<i32>(pos.xy), 0);
    return vec4<f32>(a.rgb, min(a.a, 1.0));
}
