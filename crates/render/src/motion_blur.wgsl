// Bewegungsunschaerfe, s. motion_blur.rs. Drei Durchgaenge: Geschwindigkeit je Pixel
// (fs_velocity), ihr Maximum je Kachel (fs_tilemax) und das Verwischen (fs_gather).

struct Params {
    // Bildschirm dieses Bildes (NDC samt Tiefe) -> Clip-Raum des vorigen.
    reproject: mat4x4<f32>,
    // Dasselbe fuer den eigenen Spieler, der mit dem Auge mitgeht (ohne dessen Schritt).
    reproject_own: mat4x4<f32>,
    // NDC samt Tiefe -> Sichtraum: fuer die Entfernung eines Pixels.
    inv_proj: mat4x4<f32>,
    // x = Faktor auf die Strecke eines Bildes mit der Kamera, y = laengste Strecke als Anteil
    // der Bildbreite, zw = Bildgroesse in Pixeln.
    misc: vec4<f32>,
    // x = 1: die Maske des eigenen Spielers gilt in diesem Bild; y = Kachelgroesse in Pixeln;
    // z = 1: die Bewegung der Entities gilt in diesem Bild; w = Faktor auf ihre eigene Bewegung.
    own: vec4<f32>,
};

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var own_mask: texture_2d<f32>;
@group(0) @binding(5) var object_motion: texture_2d<f32>;
@group(0) @binding(6) var velocity: texture_2d<f32>;
@group(0) @binding(7) var tile_max: texture_2d<f32>;

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

// Halbe Strecke, die der Pixel in diesem Bild auf dem Schirm zuruecklegt (Pixel), als Radius
// der Unschaerfe. Jeder Pixel als ruhende Welt mit der Kamera (der eigene Spieler ohne den
// Schritt des Auges), mit eigener Staerke; Entities dazu mit dem, was sie sich selbst bewegt
// haben -- ihre ganze Bewegung weniger der der ruhenden Welt an derselben Stelle.
@fragment
fn fs_velocity(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(pos.xy);
    let size = params.misc.zw;
    let rand = vec2<i32>(size) - 1;
    let mit_maske = params.own.x > 0.5;
    let mit_objekten = params.own.z > 0.5;
    // Die naechste Tiefe im Kreuz um den Pixel: der Rand eines nahen Objekts zieht so mit dem
    // Objekt mit statt mit dem Hintergrund. Was dort liegt (eigener Spieler, Entity), kommt mit.
    var d = textureLoad(depth, px, 0);
    var nah = px;
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
            nah = nachbarn[k];
        }
    }
    let uv = pos.xy / size;
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    var m = params.reproject;
    if (mit_maske && textureLoad(own_mask, nah, 0).r > 0.5) {
        m = params.reproject_own;
    }
    let vorher = m * ndc;
    // Hinter der vorigen Kamera: keine sinnvolle Strecke.
    var ruhend = vec2<f32>(0.0);
    if (vorher.w > 1e-6) {
        ruhend = (uv - vec2<f32>(vorher.x / vorher.w * 0.5 + 0.5, 0.5 - vorher.y / vorher.w * 0.5)) * size;
    }
    var weg = ruhend * params.misc.x;
    let objekt = textureLoad(object_motion, nah, 0);
    if (mit_objekten && objekt.z > 0.5) {
        weg += (vec2<f32>(objekt.x * 0.5, -objekt.y * 0.5) * size - ruhend) * params.own.w;
    }
    let grenze = params.misc.y * size.x;
    let laenge = length(weg);
    if (laenge > grenze) {
        weg *= grenze / laenge;
    }
    return vec4<f32>(weg * 0.5, 0.0, 0.0);
}

// Die laengste Geschwindigkeit einer Kachel: so weiss ein ruhender Hintergrundpixel, dass in
// seiner Naehe etwas vorbeizieht, das ueber ihn verwischt.
@fragment
fn fs_tilemax(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let kachel = i32(params.own.y);
    let rand = vec2<i32>(params.misc.zw) - 1;
    let start = vec2<i32>(pos.xy) * kachel;
    var best = vec2<f32>(0.0);
    for (var y = 0; y < kachel; y++) {
        for (var x = 0; x < kachel; x++) {
            let v = textureLoad(velocity, min(start + vec2<i32>(x, y), rand), 0).xy;
            if (dot(v, v) > dot(best, best)) {
                best = v;
            }
        }
    }
    return vec4<f32>(best, 0.0, 0.0);
}

// Entfernung des Pixels von der Kamera (Bloecke).
fn entfernung(px: vec2<i32>, size: vec2<f32>) -> f32 {
    let uv = (vec2<f32>(px) + 0.5) / size;
    let v = params.inv_proj * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, textureLoad(depth, px, 0), 1.0);
    return -v.z / v.w;
}

// Wie sehr a vor b liegt, weich ueber einen Bereich, der mit der Entfernung waechst: 1 = davor
// oder gleich auf, 0 = deutlich dahinter.
fn davor(a: f32, b: f32) -> f32 {
    return clamp(1.0 - (a - b) / max(0.1, 0.05 * b), 0.0, 1.0);
}

fn kegel(abstand: f32, radius: f32) -> f32 {
    return clamp(1.0 - abstand / max(radius, 1e-3), 0.0, 1.0);
}

fn zylinder(abstand: f32, radius: f32) -> f32 {
    return 1.0 - smoothstep(0.95 * radius, 1.05 * radius, abstand);
}

// Verwischen mit der groessten Geschwindigkeit der Nachbarkacheln: jede Probe zaehlt, soweit sie
// mit ihrer eigenen Bewegung ueber diesen Pixel reicht (sie liegt davor), soweit dieser Pixel mit
// seiner Bewegung ueber sie reicht (sie liegt dahinter), oder wenn beide gleich verwischen. So
// zieht ein bewegtes Entity seine Spur auch ueber den ruhenden Hintergrund.
@fragment
fn fs_gather(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(pos.xy);
    let size = params.misc.zw;
    let rand = vec2<i32>(size) - 1;
    let mitte = textureLoad(scene, px, 0);
    let kachel = i32(params.own.y);
    let kacheln = vec2<i32>(textureDimensions(tile_max)) - 1;
    let k = px / kachel;
    var vn = vec2<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let v = textureLoad(tile_max, clamp(k + vec2<i32>(x, y), vec2<i32>(0), kacheln), 0).xy;
            if (dot(v, v) > dot(vn, vn)) {
                vn = v;
            }
        }
    }
    let rn = length(vn);
    if (rn < 0.5) {
        return mitte;
    }
    let vp = textureLoad(velocity, px, 0).xy;
    let rp = length(vp);
    let zp = entfernung(px, size);
    let n = clamp(i32(ceil(rn * 2.0 / PROBEN_ABSTAND)), 2, MAX_PROBEN);
    let s = streuung(pos.xy);
    let w0 = 1.0 / max(rp, 1.0);
    var summe = mitte.rgb * w0;
    var gewicht = w0;
    for (var i = 0; i < n; i++) {
        let t = (f32(i) + s) / f32(n) * 2.0 - 1.0;
        let p = pos.xy + vn * t;
        let q = clamp(vec2<i32>(p), vec2<i32>(0), rand);
        let abstand = length(vn * t);
        let zq = entfernung(q, size);
        let rq = length(textureLoad(velocity, q, 0).xy);
        let vorne = davor(zq, zp);
        let hinten = davor(zp, zq);
        let w = vorne * kegel(abstand, rq) + hinten * kegel(abstand, rp) + zylinder(abstand, rq) * zylinder(abstand, rp) * 2.0;
        summe += textureSampleLevel(scene, linear_sampler, p / size, 0.0).rgb * w;
        gewicht += w;
    }
    return vec4<f32>(summe / gewicht, mitte.a);
}
