// Shader-Pfad: Verlauf fuer Sonnenschatten, Blocklicht-Schatten und Fernlicht (s.
// deferred_light.wgsl: fs_shadow, fs_block_shadow, fs_far_light).
//
// Die Strahl-Passes rechnen je Pixel nur wenige Strahlen, jeden Frame an anderen Stellen der
// Sonnenscheibe bzw. des Kegels einer Quelle -- ein verrauschtes Bild. Statt es raeumlich
// weichzuzeichnen (frueher drei Passes je Schattenart, mit Zaehnen und Streifen, wo der
// Weichzeichner das Rauschmuster nicht ganz traf), merkt sich jeder Frame sein Ergebnis: der
// naechste sucht fuer jeden Pixel dieselbe Stelle der Welt im letzten Bild (zurueckprojiziert
// mit der letzten Kamera) und mischt nur einen kleinen Teil neu hinein. Ueber einige Frames
// mitteln sich so viele Strahlen; der Halbschatten entsteht von selbst aus der Groesse der
// Sonnenscheibe, ganz ohne Abstandsschaetzung.
//
// Damit nichts nachzieht (ein Block wird gesetzt, ein Schatten wandert), bleibt der Verlauf
// in Bewegung in den Grenzen, die die 3x3 Nachbarn dieses Frames vorgeben; im Stand nicht --
// dort mittelt er immer laenger, und eine Weltaenderung setzt ihn zurueck (s.
// ShaderPath::prepare). Wo es keinen Verlauf gibt
// (gerade erst ins Bild gekommen, erster Frame), gilt das Mittel dieser Nachbarn.
//
// Ausgang: rgb = gemittelt, a = lineare Tiefe des Pixels (daran erkennt der naechste Frame,
// ob der Verlauf zur selben Flaeche gehoert).

struct TemporalParams {
    inv_proj: mat4x4<f32>,
    inv_view: mat4x4<f32>,
    // Ansicht * Projektion des letzten Frames (Renderraum des letzten Frames).
    prev_view_proj: mat4x4<f32>,
    // xyz = Auge jetzt minus Auge im letzten Frame; w = Anteil des neuen Frames (1 = Verlauf
    // verwerfen).
    bewegung: vec4<f32>,
    // x = near, y = far der Projektion; z = 1: Verlauf in den Grenzen der Nachbarn halten.
    tiefe: vec4<f32>,
};

@group(0) @binding(0) var depth_tex: texture_depth_2d;
@group(0) @binding(1) var input_tex: texture_2d<f32>;
@group(0) @binding(2) var history_tex: texture_2d<f32>;
@group(0) @binding(3) var<uniform> params: TemporalParams;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn linear_depth(z: f32) -> f32 {
    let n = params.tiefe.x;
    let f = params.tiefe.y;
    return n * f / (f - z * (f - n));
}

@fragment
fn fs_temporal(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(p.xy);
    let groesse = vec2<i32>(textureDimensions(input_tex));
    let mitte = textureLoad(input_tex, px, 0);
    let z_roh = textureLoad(depth_tex, px, 0);
    if (z_roh >= 1.0) {
        return vec4<f32>(mitte.rgb, -1.0);
    }
    let z = linear_depth(z_roh);

    // Nachbarn derselben Flaeche: Grenzen fuer den Verlauf und Ersatz ohne Verlauf.
    var lo = mitte.rgb;
    var hi = mitte.rgb;
    var summe = mitte.rgb;
    var anzahl = 1.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let q = clamp(px + vec2<i32>(dx, dy), vec2<i32>(0), groesse - vec2<i32>(1));
            if (abs(linear_depth(textureLoad(depth_tex, q, 0)) - z) > 0.05 * z + 0.05) {
                continue;
            }
            let c = textureLoad(input_tex, q, 0).rgb;
            lo = min(lo, c);
            hi = max(hi, c);
            summe += c;
            anzahl += 1.0;
        }
    }
    let ersatz = summe / anzahl;

    // Dieselbe Stelle im letzten Frame.
    let size = vec2<f32>(groesse);
    let ndc = vec2<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0);
    let v = params.inv_proj * vec4<f32>(ndc, z_roh, 1.0);
    let welt = (params.inv_view * vec4<f32>(v.xyz / v.w, 1.0)).xyz;
    let vorher = params.prev_view_proj * vec4<f32>(welt + params.bewegung.xyz, 1.0);
    var gilt = params.bewegung.w < 1.0 && vorher.w > 0.0;
    var verlauf = vec4<f32>(0.0);
    if (gilt) {
        let ndc_v = vorher.xy / vorher.w;
        let pv = vec2<i32>(floor(vec2<f32>(ndc_v.x * 0.5 + 0.5, 0.5 - ndc_v.y * 0.5) * size));
        gilt = all(pv >= vec2<i32>(0)) && all(pv < groesse);
        if (gilt) {
            verlauf = textureLoad(history_tex, pv, 0);
            // vorher.w ist die lineare Tiefe im letzten Frame.
            gilt = verlauf.a > 0.0 && abs(verlauf.a - vorher.w) < 0.03 * vorher.w + 0.05;
        }
    }
    if (!gilt) {
        return vec4<f32>(ersatz, z);
    }
    // Etwas Luft ueber die Grenzen, sonst zerrt das Rauschen der Nachbarn am Mittel.
    let spiel = (hi - lo) * 0.25 + 0.15 * max(hi, vec3<f32>(0.1));
    let alt = select(verlauf.rgb, clamp(verlauf.rgb, lo - spiel, hi + spiel), params.tiefe.z > 0.5);
    return vec4<f32>(mix(alt, mitte.rgb, params.bewegung.w), z);
}
