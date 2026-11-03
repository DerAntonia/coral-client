// Bewegungsunschaerfe der Kamera, s. motion_blur.rs.

struct Params {
    // Bildschirm dieses Bildes (NDC samt Tiefe) -> Clip-Raum des vorigen.
    reproject: mat4x4<f32>,
    // Dasselbe fuer den eigenen Spieler, der mit dem Auge mitgeht (ohne dessen Schritt).
    reproject_own: mat4x4<f32>,
    // x = Faktor auf die Strecke eines Bildes, y = laengste Strecke als Anteil der Bildbreite,
    // zw = Bildgroesse in Pixeln.
    misc: vec4<f32>,
    // x = 1: die Maske des eigenen Spielers gilt in diesem Bild.
    own: vec4<f32>,
};

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var own_mask: texture_2d<f32>;

// Hoechstens so viele Proben je Pixel, im Abstand von etwa PROBEN_ABSTAND Pixeln -- laengere
// Strecken werden lueckiger statt teurer.
const MAX_PROBEN: i32 = 32;
const PROBEN_ABSTAND: f32 = 2.0;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

// Je Pixel anders versetzter Start der Proben (Interleaved Gradient Noise): aus wenigen Proben
// wird feines Rauschen statt sichtbarer, versetzter Kopien des Bildes.
fn streuung(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fs_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(pos.xy);
    let size = params.misc.zw;
    let rand = vec2<i32>(size) - 1;
    let mitte = textureLoad(scene, px, 0);
    let mit_maske = params.own.x > 0.5;
    // Die naechste Tiefe im Kreuz um den Pixel: der Rand eines nahen Objekts zieht so mit dem
    // Objekt mit statt mit dem Hintergrund -- sonst bleibt an seiner Kante ein scharfer Saum.
    // Ob der naechste Nachbar zum eigenen Spieler gehoert, kommt mit.
    var d = textureLoad(depth, px, 0);
    var eigen = mit_maske && textureLoad(own_mask, px, 0).r > 0.5;
    var nachbarn = array<vec2<i32>, 4>(
        min(px + vec2<i32>(1, 0), rand),
        max(px - vec2<i32>(1, 0), vec2<i32>(0)),
        min(px + vec2<i32>(0, 1), rand),
        max(px - vec2<i32>(0, 1), vec2<i32>(0)),
    );
    for (var k = 0; k < 4; k++) {
        let dn = textureLoad(depth, nachbarn[k], 0);
        if (dn < d) {
            d = dn;
            eigen = mit_maske && textureLoad(own_mask, nachbarn[k], 0).r > 0.5;
        }
    }

    let uv = pos.xy / size;
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    var m = params.reproject;
    if (eigen) {
        m = params.reproject_own;
    }
    let vorher = m * ndc;
    // Hinter der vorigen Kamera: keine sinnvolle Strecke.
    if (vorher.w <= 1e-6) {
        return mitte;
    }
    let uv_vorher = vec2<f32>(vorher.x / vorher.w * 0.5 + 0.5, 0.5 - vorher.y / vorher.w * 0.5);
    // Strecke in Pixeln, mittig um den Pixel verteilt.
    var weg = (uv - uv_vorher) * size * params.misc.x;
    let grenze = params.misc.y * size.x;
    let laenge = length(weg);
    if (laenge > grenze) {
        weg *= grenze / laenge;
    }
    let l = min(laenge, grenze);
    if (l < 1.0) {
        return mitte;
    }
    let n = clamp(i32(ceil(l / PROBEN_ABSTAND)), 2, MAX_PROBEN);
    let s = streuung(pos.xy);
    var summe = vec3<f32>(0.0);
    var zahl = 0.0;
    for (var i = 0; i < n; i++) {
        let t = (f32(i) + s) / f32(n) - 0.5;
        let p = pos.xy + weg * t;
        // Der Hintergrund mittelt nicht ueber den eigenen Spieler hinweg -- sonst zoege er eine
        // Schliere seiner Farben hinter sich her.
        if (!eigen && mit_maske && textureLoad(own_mask, clamp(vec2<i32>(p), vec2<i32>(0), rand), 0).r > 0.5) {
            continue;
        }
        summe += textureSampleLevel(scene, linear_sampler, p / size, 0.0).rgb;
        zahl += 1.0;
    }
    if (zahl < 1.0) {
        return mitte;
    }
    return vec4<f32>(summe / zahl, mitte.a);
}
