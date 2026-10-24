// Fernsicht des Moduls Chunk-Saver (s. render::lod_renderer und das Crate chunk_saver).
//
// Keine Vertex-Puffer: je Flaeche liegen vier u32 in einem Speicherpuffer (chunk_saver::mesh),
// der Vertex-Shader baut daraus die sechs Ecken der zwei Dreiecke. Die Textur wiederholt sich
// je Block (fract der Blockkoordinate), die Mip-Stufe wird selbst bestimmt -- die Hardware
// saehe an den fract-Spruengen riesige Ableitungen.
//
// Tiefe: eigener Puffer mit umgekehrtem Z und unendlicher Fernebene (Loeschwert 0, Vergleich
// Greater), weil die Fernsicht weit hinter der Fernebene des Gelaendes (1000 Bloecke) liegt.

struct Globals {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    // x = Projektionsmodus, y = tan(fov/2), z = Seitenverhaeltnis (wie block.wgsl), w = Staerke
    // der Eckabdunklung ("Weiche Beleuchtung", 0..1).
    params: vec4<f32>,
    // xy = Augenposition innerhalb seines Chunks (x, z in 0..16), z = hoechste Mip-Stufe des
    // Atlas, w = Texel je Block.
    eye: vec4<f32>,
    // xy = Chunk des Auges, z = 1: Spaltenmaske pruefen.
    eye_chunk: vec4<i32>,
    // xy = Mitte des Sichtkreises relativ zu eye_chunk, z = Radius^2 in Chunks.
    clip: vec4<i32>,
};

@group(0) @binding(0) var<uniform> camera: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
// Je State 12 Worte (s. world::voxel::STATE_INFO_WORTE, die ersten zwoelf): je Seite die Ecke
// seines Atlas-Rechtecks (u, v je 16 Bit), dann je Seite seine Groesse.
@group(0) @binding(3) var<storage, read> state_rects: array<u32>;
// Je gezeichnetem Knoten: xyz = Ecke relativ zum Auge, w = Voxelgroesse in Bloecken.
@group(0) @binding(4) var<storage, read> nodes: array<vec4<f32>>;
// 256 x 256 Chunks, ringfoermig adressiert: 1 = dort zeichnet echtes Gelaende.
@group(0) @binding(5) var column_mask: texture_2d<u32>;
@group(1) @binding(0) var<storage, read> quads: array<u32>;

// Nichtlineare Projektionen, wie in block.wgsl (dort beschrieben) -- die Fernsicht muss sich
// genauso kruemmen wie das Gelaende davor.
fn warp_cylindrical(p: vec3<f32>, d: f32, hx: f32, hy: f32) -> vec2<f32> {
    // Horizontaler Winkel linear auf die Bildbreite (Zylindermantel), Vertikale perspektivisch
    // relativ zum horizontalen Radius.
    let phi = atan2(p.x, d);
    let rho = sqrt(p.x * p.x + d * d);
    return vec2<f32>(phi / atan(hx), (p.y / rho) / hy);
}

fn warp_stereographic(p: vec3<f32>, d: f32, hx: f32, hy: f32) -> vec2<f32> {
    // Azimutale stereografische (Fischauge-)Projektion: Radius 2*tan(theta/2) statt tan(theta),
    // je Achse am jeweiligen FOV-Rand normiert, damit das eingestellte Sichtfeld stimmt.
    let rt = length(p.xy);
    if (rt < 1e-5) {
        return vec2<f32>(0.0, 0.0);
    }
    let theta = atan2(rt, d);
    let rs = 2.0 * tan(theta * 0.5);
    let dir = p.xy / rt;
    let edge_x = 2.0 * tan(atan(hx) * 0.5);
    let edge_y = 2.0 * tan(atan(hy) * 0.5);
    return vec2<f32>(dir.x * rs / edge_x, dir.y * rs / edge_y);
}

fn warp_panini(p: vec3<f32>, d: f32, hx: f32, hy: f32) -> vec2<f32> {
    // Panini-Projektion (Kompromiss aus rektilinear und zylindrisch), Parameter dd=1 (Standard-
    // "kompakte" Panini). Horizontal ueber den Panini-Skalierungsterm, vertikal perspektivisch;
    // horizontal am FOV-Rand normiert.
    let dd = 1.0;
    let az = atan2(p.x, d);
    let rho = sqrt(p.x * p.x + d * d);
    let s = (dd + 1.0) / (dd + cos(az));
    let px = s * sin(az);
    let py = s * (p.y / rho);
    let az_e = atan(hx);
    let px_e = ((dd + 1.0) / (dd + cos(az_e))) * sin(az_e);
    return vec2<f32>(px / px_e, py / hy);
}

fn project(view_pos: vec4<f32>) -> vec4<f32> {
    let std_clip = camera.proj * view_pos;
    let mode = camera.params.x;
    let d = -view_pos.z; // Vorwaerts-Tiefe (Kamera blickt entlang -Z)
    // Normal-Modus oder hinter/auf der Kameraebene: unveraenderte Standardprojektion.
    if (mode < 0.5 || d <= 0.001) {
        return std_clip;
    }
    let hy = camera.params.y;            // tan(fov/2)
    let hx = hy * camera.params.z;       // tan(fov/2) * aspect
    let p = view_pos.xyz;
    var ndc: vec2<f32>;
    if (mode < 1.5) {
        ndc = warp_panini(p, d, hx, hy);
    } else if (mode < 2.5) {
        ndc = warp_stereographic(p, d, hx, hy);
    } else {
        ndc = warp_cylindrical(p, d, hx, hy);
    }
    // x/y ueber w = d zuruueckrechnen (nach dem Perspektiv-Divide bleibt genau ndc uebrig),
    // z aus der Standardprojektion uebernehmen -> identische Tiefe.
    return vec4<f32>(ndc * d, std_clip.z, d);
}

// Je Seite: Normalenachse, Plus-Richtung, Flaechenachsen u und v -- wie chunk_saver::mesh::FACES.
fn axis(i: u32) -> vec3<f32> {
    return vec3<f32>(f32(i == 0u), f32(i == 1u), f32(i == 2u));
}

struct Face {
    n: u32,
    plus: bool,
    u: u32,
    v: u32,
};

fn face_info(face: u32) -> Face {
    switch face {
        case 0u: { return Face(1u, false, 0u, 2u); }
        case 1u: { return Face(1u, true, 0u, 2u); }
        case 2u: { return Face(2u, false, 0u, 1u); }
        case 3u: { return Face(2u, true, 0u, 1u); }
        case 4u: { return Face(0u, false, 2u, 1u); }
        default: { return Face(0u, true, 2u, 1u); }
    }
}

// Vanillas Richtungs-Schattierung: oben 1, unten 0,5, Nord/Sued 0,8, West/Ost 0,6.
fn face_shade(face: u32) -> f32 {
    switch face {
        case 0u: { return 0.5; }
        case 1u: { return 1.0; }
        case 2u, 3u: { return 0.8; }
        default: { return 0.6; }
    }
}

struct LodOut {
    @builtin(position) clip_position: vec4<f32>,
    // Augenrelative Position (fuer Maske und Nebel).
    @location(0) rel: vec3<f32>,
    // Blockkoordinaten in der Flaeche, ungewickelt (Textur wiederholt sich je Block).
    @location(1) tex: vec2<f32>,
    @location(2) @interpolate(flat) rect: vec4<f32>,
    @location(3) color: vec3<f32>,
    // Ein Viertelblock von der Flaeche in ihren eigenen Block hinein (s. covered).
    @location(4) @interpolate(flat) inward: vec3<f32>,
};

@vertex
fn vs_lod(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> LodOut {
    let q = vi / 6u;
    let w0 = quads[q * 4u];
    let w1 = quads[q * 4u + 1u];
    let tint = quads[q * 4u + 2u];
    let form = quads[q * 4u + 3u];
    let state = w1 & 65535u;
    let face = (w0 >> 15u) & 7u;
    let f = face_info(face);
    let size = vec2<f32>(f32(((w0 >> 18u) & 31u) + 1u), f32(((w0 >> 23u) & 31u) + 1u));
    // Huellbox des Voxels in Bruchteilen (s. chunk_saver::node::pack_shape).
    let lo = vec3<f32>(f32(form & 31u), f32((form >> 5u) & 31u), f32((form >> 10u) & 31u)) / 16.0;
    let hi = vec3<f32>(f32((form >> 15u) & 31u), f32((form >> 20u) & 31u), f32((form >> 25u) & 31u)) / 16.0;
    // Ecken 0,1,2, 0,2,3 des Rechtecks (0,0) (1,0) (1,1) (0,1); fuer oben, Norden und Osten
    // andersherum, damit alle Flaechen von aussen gegen den Uhrzeigersinn liegen.
    var order = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    var corners = array<vec2<f32>, 4>(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    var c = corners[order[vi % 6u]];
    if (face == 1u || face == 2u || face == 5u) {
        c = c.yx;
    }
    let base = vec3<f32>(f32(w0 & 31u), f32((w0 >> 5u) & 31u), f32((w0 >> 10u) & 31u));
    let au = axis(f.u);
    let av = axis(f.v);
    let an = axis(f.n);
    // Die Seite liegt auf dem Rand der Huellbox; ueber das Rechteck reicht sie vom Anfang der
    // ersten bis zum Ende der letzten Zelle.
    let ebene = select(dot(lo, an), dot(hi, an), f.plus);
    let u0 = dot(lo, au);
    let v0 = dot(lo, av);
    let u1 = size.x - 1.0 + dot(hi, au);
    let v1 = size.y - 1.0 + dot(hi, av);
    let local = base + an * ebene + au * mix(u0, u1, c.x) + av * mix(v0, v1, c.y);
    let node = nodes[ii];
    let blocks = local * node.w;
    let rel = node.xyz + blocks;
    var out: LodOut;
    out.clip_position = project(camera.view * vec4<f32>(rel, 1.0));
    out.rel = rel;
    out.inward = an * select(0.25, -0.25, f.plus);
    // Seitenflaechen: Texturoberkante oben (v zaehlt nach unten).
    let bu = dot(blocks, au);
    let bv = dot(blocks, av);
    out.tex = select(vec2<f32>(bu, -bv), vec2<f32>(bu, bv), f.n == 1u);
    let corner = state_rects[state * 12u + face];
    let extent = state_rects[state * 12u + 6u + face];
    out.rect = vec4<f32>(f32(corner & 65535u), f32(corner >> 16u), f32(extent & 65535u), f32(extent >> 16u)) / 65535.0;
    // Eckabdunklung wie Vanillas weiche Beleuchtung: je abdunkelndem Nachbarn 0,2 weniger, in
    // der eingestellten Staerke -- genau wie beim nahen Gelaende (1 - (1 - ao) * Staerke).
    let ecke = u32(c.x) + 2u * u32(c.y);
    let ao = 1.0 - 0.2 * f32((w1 >> (16u + 3u * ecke)) & 7u) * camera.params.w;
    let shade = face_shade(face) * ao;
    out.color = vec3<f32>(f32(tint & 255u), f32((tint >> 8u) & 255u), f32((tint >> 16u) & 255u)) / 255.0 * shade;
    return out;
}

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

// Liegt die Stelle in einem Chunk, den das echte Gelaende zeichnet? Die Maske ist ringfoermig
// (256 Chunks), geprueft wird deshalb nur in der Naehe des Auges -- geladen ist ohnehin nur
// dort etwas.
// Gefragt wird ein Viertelblock innerhalb des Blocks, dem die Flaeche gehoert: eine Seite auf
// einer Chunkgrenze laege sonst je nach Rundung mal im einen, mal im anderen Chunk und
// blinkte beim Bewegen an der Grenze zum echten Gelaende.
fn covered(rel: vec3<f32>) -> bool {
    if (camera.eye_chunk.z == 0) {
        return false;
    }
    let d = vec2<i32>(floor((camera.eye.xy + rel.xz) / 16.0));
    if (any(abs(d) >= vec2<i32>(120))) {
        return false;
    }
    let c = (camera.eye_chunk.xy + d) & vec2<i32>(255);
    return textureLoad(column_mask, c, 0).r != 0u;
}

// Liegt die Stelle ausserhalb des Sichtkreises? Gezaehlt wird in ganzen Chunks wie beim echten
// Gelaende (s. LodFrame::clip), sonst stuende hinter dessen Rand ein Saum aus Knotenstuecken.
fn beyond_radius(rel: vec3<f32>) -> bool {
    let d = vec2<i32>(floor((camera.eye.xy + rel.xz) / 16.0)) - camera.clip.xy;
    return d.x * d.x + d.y * d.y > camera.clip.z;
}

fn sample_face(in: LodOut) -> vec4<f32> {
    // Mip-Stufe aus der ungewickelten Koordinate, begrenzt auf die Stufen, in denen eine
    // Kachel noch fuer sich steht (darueber mischen sich Nachbarkacheln). Gefiltert wird
    // bilinear -- in der Ferne ist ein Block nur wenige Pixel gross, scharfe Texel ergaeben
    // dort nur Wellenmuster (etwa bei Eis). Damit dabei keine Nachbarkachel hereinblutet,
    // bleibt die Koordinate einen halben Texel der gewaehlten Stufe von der Kachelkante weg.
    let texel = in.tex * camera.eye.w;
    let rho = max(length(dpdx(texel)), length(dpdy(texel)));
    let level = clamp(log2(max(rho, 1e-6)), 0.0, camera.eye.z);
    let rand = 0.5 * exp2(ceil(level)) / camera.eye.w;
    let uv = in.rect.xy + clamp(fract(in.tex), vec2<f32>(rand), vec2<f32>(1.0 - rand)) * in.rect.zw;
    return textureSampleLevel(atlas, atlas_sampler, uv, level);
}

@fragment
fn fs_lod(in: LodOut) -> @location(0) vec4<f32> {
    let tex = sample_face(in);
    if (covered(in.rel + in.inward) || beyond_radius(in.rel + in.inward)) {
        discard;
    }
    // Deckend wie das nahe Gelaende: Alpha zaehlt hier nicht (Laub hat im Atlas ohnehin volle
    // Deckkraft, Eis zeichnet auch Vanilla als Cutout ohne Durchblick).
    let rgb = srgb_to_linear(linear_to_srgb(tex.rgb) * in.color);
    return vec4<f32>(rgb, 1.0);
}

@fragment
fn fs_lod_translucent(in: LodOut) -> @location(0) vec4<f32> {
    let tex = sample_face(in);
    if (covered(in.rel + in.inward) || beyond_radius(in.rel + in.inward)) {
        discard;
    }
    let rgb = srgb_to_linear(linear_to_srgb(tex.rgb) * in.color);
    return vec4<f32>(rgb, tex.a);
}
