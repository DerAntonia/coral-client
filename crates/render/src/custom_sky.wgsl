// Himmel aus Resourcepacks (OptiFine-Format), eine Ebene pro Zeichenaufruf (s. custom_sky.rs).
//
// Statt sechs Wuerfelflaechen als Geometrie zu zeichnen, rechnet jedes Pixel seine
// Blickrichtung aus und schlaegt sie im 3x2-Raster des Bildes nach. Das Ergebnis ist dasselbe
// wie ein Wuerfel um die Kamera (Richtung -> Flaeche -> Stelle auf der Flaeche), nur ohne
// Tiefe und ohne Naht an den Kanten.
//
// Raster der Quelle, Zellen je ein Drittel breit und halb hoch:
//
//   Zeile 0:  unten (-Y)   oben (+Y)   +X
//   Zeile 1:  +Z           -X          -Z
//
// (Achsen im Raum der Ebene, also nach Basis- und Ebenendrehung, s. to_local.) Wie jede Zelle
// liegt, steht in sky_uv; die Faelle sind aus den Flaechendrehungen nachgerechnet und an einem
// echten Pack geprueft (die Kanten benachbarter Flaechen passen aneinander, s. app::custom_sky).

struct Layer {
    // Bildschirm -> Welt-Richtung (Projektion mal reine Blickdrehung, invertiert).
    inv_view_proj: mat4x4<f32>,
    // Welt-Richtung -> Raum der Ebene.
    to_local: mat3x3<f32>,
    // Farbfaktor der Mischart (Helligkeit in RGB und/oder A, s. Blend::color).
    color: vec4<f32>,
    // xy = halbes Texel in Zellen-Einheiten (gegen Ueberblutung an den Zellraendern).
    texel: vec4<f32>,
};

@group(0) @binding(0) var sky_tex: texture_2d<f32>;
@group(0) @binding(1) var sky_sampler: sampler;
@group(1) @binding(0) var<uniform> layer: Layer;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> VsOut {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = 1.0 - f32(i & 2u) * 2.0;
    var out: VsOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

fn sky_uv(l: vec3<f32>) -> vec2<f32> {
    let a = abs(l);
    var cell: vec2<f32>;
    var x: f32;
    var z: f32;
    if (a.y >= a.x && a.y >= a.z) {
        if (l.y < 0.0) {
            cell = vec2<f32>(0.0, 0.0);
            x = l.z / l.y;
            z = -l.x / l.y;
        } else {
            cell = vec2<f32>(1.0, 0.0);
            x = -l.z / l.y;
            z = -l.x / l.y;
        }
    } else if (a.x >= a.z) {
        if (l.x > 0.0) {
            cell = vec2<f32>(2.0, 0.0);
            x = l.z / l.x;
            z = -l.y / l.x;
        } else {
            cell = vec2<f32>(1.0, 1.0);
            x = l.z / l.x;
            z = l.y / l.x;
        }
    } else {
        if (l.z > 0.0) {
            cell = vec2<f32>(0.0, 1.0);
            x = -l.x / l.z;
            z = -l.y / l.z;
        } else {
            cell = vec2<f32>(2.0, 1.0);
            x = -l.x / l.z;
            z = l.y / l.z;
        }
    }
    let t = clamp(vec2<f32>(x, z) * 0.5 + 0.5, layer.texel.xy, 1.0 - layer.texel.xy);
    return (cell + t) / vec2<f32>(3.0, 2.0);
}

@fragment
fn fs_sky(in: VsOut) -> @location(0) vec4<f32> {
    let p0 = layer.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let p1 = layer.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let d = p1.xyz / p1.w - p0.xyz / p0.w;
    let l = normalize(layer.to_local * d);
    let c = textureSampleLevel(sky_tex, sky_sampler, sky_uv(l), 0.0) * layer.color;
    if (c.a == 0.0) {
        discard;
    }
    return c;
}
