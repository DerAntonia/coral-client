struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec4<f32>,
    @location(2) shape: vec4<f32>,
    @location(3) border_color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) local: vec4<f32>,
    @location(3) shape: vec4<f32>,
    @location(4) border_color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    out.local = local;
    out.shape = shape;
    out.border_color = border_color;
    return out;
}

// Abstand zum Rand eines Rechtecks mit runden Ecken (negativ = innen), in Pixeln.
fn rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - (half - vec2<f32>(radius));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Ungeformte Flaechen (Linien, Graphen, alte UI): einfach die Farbe.
    if (in.shape.x <= 0.0) {
        return in.color;
    }
    let d = rounded_box(in.local.xy, in.shape.xy, in.shape.z);
    let soft = in.local.z;
    // Deckung der Aussenkante: mit weicher Kante ein Verlauf ueber `soft` Pixel (Schatten),
    // sonst ein Pixel Kantenglaettung.
    var outer: f32;
    if (soft > 0.0) {
        outer = 1.0 - smoothstep(-soft * 0.5, soft * 0.5, d);
    } else {
        outer = clamp(0.5 - d, 0.0, 1.0);
    }
    // Rahmen: alles naeher als `shape.w` am Rand nimmt die Rahmenfarbe.
    var c = in.color;
    if (in.shape.w > 0.0) {
        let inner = clamp(0.5 - (d + in.shape.w), 0.0, 1.0);
        c = mix(in.border_color, in.color, inner);
    }
    return vec4<f32>(c.rgb, c.a * outer);
}
