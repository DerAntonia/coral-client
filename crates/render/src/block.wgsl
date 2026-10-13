struct Camera {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    // x = Projektionsmodus (0=Normal,1=Panini,2=Stereografisch,3=Zylindrisch),
    // y = tan(fov/2), z = Seitenverhaeltnis (Breite/Hoehe), w = Tageslicht 0..1 (nur die
    // Welt-Kamera; Shader-Pfad, s. scene_light).
    params: vec4<f32>,
    // Hitbox-Anzeige (s. vs_hitbox): x = gewuenschte halbe Linienbreite in Bildschirmpixeln,
    // y = tatsaechlich gezeichnete halbe Breite (>= x, s. HITBOX_MIN_RASTER_HALF_PX),
    // z/w = Bildschirmgroesse in Pixeln.
    hitbox: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;

// Nichtlineare Projektionen als Vertex-Warp (gleiche Technik wie OptiFine/Iris "Panini"):
// Die Sichtraum-Position wird in normierte Bildkoordinaten (NDC, [-1,1]) umgerechnet und
// anschliessend ueber den Standard-Perspektiv-Divide zurueckprojiziert. Die Tiefe (z/w) wird
// dabei UNVERAENDERT aus der Standard-Projektion uebernommen, damit Tiefentest/-schreiben
// bit-genau wie im Normal-Modus bleiben (nur x/y wandern).
//
// GRENZE: Die Warp wirkt nur an den Eckpunkten -- zwischen zwei Vertices bleiben Kanten
// gerade. Bei fein unterteilter Geometrie (viele kleine Flaechen) sieht die Kruemmung
// stimmig aus, grosse zusammengefasste Flaechen (Greedy-Mesh: ganze Boeden/Waende als eine
// Quad) kruemmen sich hingegen nicht mit. Exakt waere ein Screen-Space-Resample (groesserer
// Umbau) -- hier bewusst die pragmatische Vertex-Variante.

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

// Block-/Entity-Textur-Atlas. Nearest-Sampling ohne Mips -- Minecraft-Texturen sind
// punktgesampelt (scharfe Pixel), kein bilineares Verwaschen.
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

const MIPMAP_LOD_BIAS: f32 = -1.0;

// Vertexfarben (Richtungs-Shade, AO, Biom-Toenung, Entity-/Partikelfarben) sind Vanilla-
// Werte und gelten dort im sRGB-Raum: Vanilla multipliziert die gammakodierten Texel direkt.
// Der Atlas ist dagegen Rgba8UnormSrgb und liefert LINEARE Texel -- mit diesen multipliziert
// wirkte jeder Faktor c wie c^(1/2.2): die Grastoenung (0.57, 0.74, 0.35) kam als
// (0.77, 0.87, 0.62) an (blasses, graues Gras), und die Flaechen-Shades 0.6/0.8 als
// 0.79/0.90 (kaum Helligkeitsunterschied zwischen den Seiten -- Items/Bloecke wirkten flach,
// die Tiefe war nicht mehr ablesbar). Deshalb: zurueck nach sRGB, dort mischen, wieder
// linear fuer das sRGB-Ziel.
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

// Textur (linear, aus dem sRGB-Atlas) mal Vanilla-Vertexfarbe, gerechnet wie in Vanilla.
fn tint_srgb(tex_linear: vec3<f32>, color: vec3<f32>) -> vec3<f32> {
    return srgb_to_linear(linear_to_srgb(tex_linear) * color);
}

// Spalten-lokale Position relativ zum Auge: erst der ganzzahlige Spaltenversatz (die Summe ist
// exakt), dann der fuer alle Spalten gleiche Nachkomma-Anteil des Auges. So rechnen zwei
// Nachbarspalten ihre gemeinsame Kante bitgleich -- s. BlockRenderer::update_column_offsets.
fn column_relative(position: vec3<f32>, column_offset: vec3<f32>, eye_frac: vec3<f32>) -> vec3<f32> {
    return (position + column_offset) - eye_frac;
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(
    // SPALTEN-LOKALE Position (x,z in 0..16, y absolut) -- s. world::mesher::ColumnOrigin.
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Versatz der Spalte, RELATIV ZUM AUGE, einmal pro Zeichenaufruf (Instanz-Rate).
    // Erst diese Addition setzt die Spalte an ihren Platz -- und weil beide Summanden klein
    // sind (lokal 0..16, Versatz hoechstens Sichtweite), bleibt das Ergebnis in f32 genau.
    // Bei absoluten Weltkoordinaten war genau hier die Praezision verloren, lange bevor der
    // Vertex den Shader erreichte. Fuer Puffer, die schon augenrelativ ankommen (Entities,
    // Hand, Geisterbloecke) ist der Versatz null. In ganzen Bloecken, den Nachkomma-Anteil
    // des Auges zieht column_relative danach ab.
    @location(3) column_offset: vec3<f32>,
    @location(5) eye_frac: vec3<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = project(camera.view * vec4<f32>(column_relative(position, column_offset, eye_frac), 1.0));
    out.color = color;
    out.uv = uv;
    return out;
}

// Opakes Gelaende im kompakten Format (world::TerrainVertex): Farbe als RGBA8 (kommt als
// 0..1 an), UV als u16 in 1/65536 des Atlas. Sonst genau wie vs_main.
@vertex
fn vs_terrain(
    @location(0) packed: vec4<u32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<u32>,
    @location(3) column_offset: vec3<f32>,
    @location(5) eye_frac: vec3<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    // xyz relativ zur Section in 1/2048 Block, um 8 verschoben; w = Section ab min_y (s.
    // world::TerrainVertex). Der Spaltenversatz zeigt auf die Spaltenecke auf Hoehe min_y.
    let position = vec3<f32>(packed.xyz) * (1.0 / 2048.0) - vec3<f32>(8.0) + vec3<f32>(0.0, f32(packed.w & 31u) * 16.0, 0.0);
    out.clip_position = project(camera.view * vec4<f32>(column_relative(position, column_offset, eye_frac), 1.0));
    out.color = color.rgb;
    // Hart abschneiden (world::TerrainVertex::CUTOUT_MARK): weiter wie bei BlockVertex
    // gekennzeichnet, s. fs_main_translucent.
    if (color.a > 0.999) {
        out.color = vec3<f32>(-1.0) - color.rgb;
    }
    out.uv = vec2<f32>(uv) * (1.0 / 65536.0);
    return out;
}

// --- Shader-Pfad: G-Buffer ------------------------------------------------------------
//
// Dieselbe Gelaende-Geometrie wie vs_terrain, zusaetzlich die Sichtraum-Position: aus ihren
// Bildschirm-Ableitungen entsteht die Flaechennormale, ohne dass das Mesh eine tragen muss
// (Bloecke sind ohnehin flach). Geschrieben werden Albedo (Textur * Vertexfarbe -- in diesem
// Schritt noch samt Richtungs-Schattierung und Eckabdunklung, damit das Bild wie im PvP-Pfad
// aussieht) und die Normale im Sichtraum.
struct GBufferVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) view_pos: vec3<f32>,
    // Blocklicht RGB und Himmelslicht, je 0..1 (s. unpack_light).
    @location(3) light: vec4<f32>,
};

// Licht des Vertex (s. world::TerrainVertex::LIGHT_*): untere 8 Bit im Farb-Alpha, obere 11
// ueber dem Section-Index. Himmel 0..15, Blocklicht je Kanal in halben Stufen 0..30.
fn unpack_light(packed_w: u32, alpha: f32) -> vec4<f32> {
    return unpack_light_bits(u32(round(alpha * 255.0)) | ((packed_w >> 5u) << 8u));
}

// Dasselbe aus dem ganzen gepackten Wert (world::light::pack_vertex_light).
fn unpack_light_bits(l: u32) -> vec4<f32> {
    let block = vec3<f32>(f32((l >> 4u) & 31u), f32((l >> 9u) & 31u), f32((l >> 14u) & 31u)) / 30.0;
    return vec4<f32>(block, f32(l & 15u) / 15.0);
}

// Helligkeit aus Licht, im linearen HDR-Raum. Vanillas Kurve je Lichtstufe (l / (4 - 3l),
// ohne Umgebungsanhebung), Himmel mit der Tageszeit gedimmt (nachts wie Vanilla auf 20 % und
// leicht blau), farbiges Blocklicht etwas ueber 1, damit eine Fackel im Dunkeln leuchtet.
// Muss mit deferred_light.wgsl uebereinstimmen.
fn light_curve(l: vec3<f32>) -> vec3<f32> {
    return l / (vec3<f32>(4.0) - 3.0 * l);
}

fn scene_light(light: vec4<f32>, daylight: f32) -> vec3<f32> {
    let himmel = light_curve(vec3<f32>(light.a)) * (0.2 + 0.8 * daylight) * mix(vec3<f32>(0.55, 0.62, 0.95), vec3<f32>(1.0), daylight);
    let block = light_curve(light.rgb) * 1.4;
    return vec3<f32>(0.015) + himmel + block;
}

@vertex
fn vs_terrain_gbuffer(
    @location(0) packed: vec4<u32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<u32>,
    @location(3) column_offset: vec3<f32>,
    @location(5) eye_frac: vec3<f32>,
) -> GBufferVertexOutput {
    var out: GBufferVertexOutput;
    let position = vec3<f32>(packed.xyz) * (1.0 / 2048.0) - vec3<f32>(8.0) + vec3<f32>(0.0, f32(packed.w & 31u) * 16.0, 0.0);
    let view_pos = camera.view * vec4<f32>(column_relative(position, column_offset, eye_frac), 1.0);
    out.clip_position = project(view_pos);
    out.color = color.rgb;
    out.uv = vec2<f32>(uv) * (1.0 / 65536.0);
    out.view_pos = view_pos.xyz;
    out.light = unpack_light(packed.w, color.a);
    return out;
}

// Deckende Geisterbloecke im Shader-Pfad: BlockVertex (augenrelativ) mit Licht je Vertex aus
// eigenem Puffer, sonst wie vs_terrain_gbuffer.
@vertex
fn vs_block_gbuffer(
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) light: u32,
) -> GBufferVertexOutput {
    var out: GBufferVertexOutput;
    let view_pos = camera.view * vec4<f32>(position, 1.0);
    out.clip_position = project(view_pos);
    out.color = color;
    out.uv = uv;
    out.view_pos = view_pos.xyz;
    out.light = unpack_light_bits(light);
    return out;
}

struct GBufferOutput {
    @location(0) albedo: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) light: vec4<f32>,
};

@fragment
fn fs_terrain_gbuffer(in: GBufferVertexOutput) -> GBufferOutput {
    // Ableitungen VOR dem discard, solange alle Fragmente des Quads noch mitrechnen.
    let roh = normalize(cross(dpdx(in.view_pos), dpdy(in.view_pos)));
    // Sichtbare Flaechen zeigen zur Kamera (Rueckseiten werden weggeculled): die Normale so
    // drehen, dass sie gegen die Blickrichtung weist -- dann ist das Vorzeichen der
    // Bildschirmachsen egal.
    let n = select(roh, -roh, dot(roh, in.view_pos) > 0.0);
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.5) {
        discard;
    }
    var out: GBufferOutput;
    // Alpha 1 markiert "hier ist Gelaende" -- der Licht-Pass setzt sonst den Himmel ein.
    out.albedo = vec4<f32>(tint_srgb(tex.rgb, in.color), 1.0);
    out.normal = vec4<f32>(n * 0.5 + vec3<f32>(0.5), 1.0);
    out.light = in.light;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Negativer Bias verschiebt die von der GPU aus den UV-Ableitungen automatisch gewaehlte
    // Mip-Stufe Richtung schaerfer -- ohne Bias wirkten schon nahe Flaechen (v.a. Boden/Waende
    // in flachem Blickwinkel, wo die Ableitung in eine Achse gross ist) unnoetig verschwommen.
    // Anisotropes Filtern waere die "richtige" Loesung dafuer, ist aber mit dem bewusst
    // Nearest-gesampelten Atlas (scharfe Minecraft-Pixel aus der Naehe) nicht kombinierbar --
    // WebGPU/wgpu verlangt fuer anisotropy_clamp>1 zwingend lineares Mag-/Min-/Mipmap-Filtern.
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    // Alpha-Test: durchsichtige Texel (Glas-/Laub-Raender) verwerfen statt schwarz zu
    // zeichnen. Kein echtes Alpha-Blending in diesem Pass -- Wasser laeuft ueber
    // fs_main_translucent/eine eigene Pipeline mit echtem Blending (s. render::BlockRenderer).
    if (tex.a < 0.5) {
        discard;
    }
    // color = Richtungs-Shade (bei Bloecken) bzw. Volltonfarbe (bei Entity-Boxen).
    return vec4<f32>(tint_srgb(tex.rgb, in.color), 1.0);
}

// Riss-Ueberlagerung beim Abbauen (s. render::BlockRenderer::crumbling_pipeline).
//
// Vanillas rendertype_crumbling.fsh: Textur mal Vertexfarbe (die ist hier immer weiss, s.
// SheetedDecalTextureGenerator.setColor), verwerfen unter Alpha 0,1 -- der Rest der
// Abdunklung kommt aus der multiplikativen Mischung der Pipeline, nicht aus dem Shader.
// Die Schwelle ist bewusst 0,1 und nicht die 0,5 der Blockflaechen: die weichen Raender der
// Risstextur sollen mitgezeichnet werden.
@fragment
fn fs_crumbling(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.1) {
        discard;
    }
    // Vanilla mischt 2*Quelle*Ziel mit sRGB-WERTEN (GL ohne sRGB-Framebuffer). Unsere
    // Oberflaeche ist sRGB, die Mischung laeuft also LINEAR -- 2*lin(s)*lin(d) waere viel zu
    // dunkel, und die hellen Risslinien (155 = mal 1,22) dunkelten ab statt aufzuhellen.
    // Mit Gamma 2,2 gilt lin(2*s*d) = (2*s)^2,2 * lin(d); die Pipeline rechnet 2*Quelle*Ziel,
    // also Quelle = (2*s)^2,2 / 2.
    let s = linear_to_srgb(tex.rgb);
    return vec4<f32>(pow(2.0 * s, vec3<f32>(2.2)) * 0.5, 1.0);
}

// --- Entities mit Schadens-Faerbung ------------------------------------------------------
//
// Eigener Zweig statt vs_main/fs_main, weil Vanillas Schadens-Rot KEIN Multiplikator ist.
// Die Vertexfarbe wird mit der Textur MULTIPLIZIERT -- damit laesst sich nur abdunkeln, nie
// zu einer Farbe hin aufhellen. Vanilla mischt stattdessen NACH dem Texturzugriff
// (entity.fsh):
//
//     color.rgb = mix(overlayColor.rgb, color.rgb, overlayColor.a);
//
// Die Hurt-Zeile der OverlayTexture ist ARGB(178, 255, 0, 0) -- also reines Rot mit
// overlayColor.a = 178/255 = 0.698, was 30.2 % Rot beimischt. Weil dort ALLE Spalten der
// Zeile denselben Wert tragen, blendet die Faerbung ueber die 10 Hurt-Ticks NICHT aus.
//
// `tint` traegt hier rgb = Farbe und a = ANTEIL dieser Farbe (0 = unveraendert, 1 = voll) --
// also Vanillas overlayColor.a bereits umgedreht, damit die Einstellung "Deckkraft" heisst
// und wie ueberall sonst 0 % = unsichtbar bedeutet.
//
// Der Spalten-Versatz faellt weg: Entity-Vertices kommen schon augenrelativ an (s. vs_main).
struct EntityOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
};

@vertex
fn vs_entity(
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
) -> EntityOutput {
    var out: EntityOutput;
    out.clip_position = project(camera.view * vec4<f32>(position, 1.0));
    out.color = color;
    out.uv = uv;
    out.tint = tint;
    return out;
}

@fragment
fn fs_entity(in: EntityOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.5) {
        discard;
    }
    // Auch die Schadens-Mischung passiert in Vanilla auf gammakodierten Werten.
    let base = linear_to_srgb(tex.rgb) * in.color;
    return vec4<f32>(srgb_to_linear(mix(base, in.tint.rgb, in.tint.a)), 1.0);
}

// Leucht-Umriss (Glowing): nur die Silhouette, deckend in der Farbe aus `tint` (Teamfarbe),
// in ein eigenes Bild ohne Tiefe -- s. entity_outline.wgsl fuer den Rest. Durchsichtige
// Texel gehoeren nicht zur Silhouette.
@fragment
fn fs_entity_outline(in: EntityOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.1) {
        discard;
    }
    return vec4<f32>(in.tint.rgb, 1.0);
}

// --- Shader-Pfad: Entities und Hand mit Licht ---------------------------------------------
//
// Entities kommen nach dem Licht-Pass und der Tonemap direkt ins fertige Bild (s.
// render_forward_rest). Damit sie zum Gelaende passen, rechnen sie hier dasselbe nach: Farbe
// linear mal scene_light, danach dieselbe Tonemap-Kurve. Das Licht kommt je Entity von der
// CPU (Augenposition, s. app::entity_light); Bit 31 heisst unbeleuchtet (Feuer, Spuren).

// Kopie von tonemap.wgsl::pbr_neutral (Belichtung 1) -- muss mit ihr uebereinstimmen.
fn tonemap_neutral(c_in: vec3<f32>) -> vec3<f32> {
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

fn packed_scene_light(l: u32, daylight: f32) -> vec3<f32> {
    if ((l & 0x80000000u) != 0u) {
        return vec3<f32>(1.0);
    }
    return scene_light(unpack_light_bits(l), daylight);
}

struct EntityLitOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) light: vec3<f32>,
};

@vertex
fn vs_entity_lit(
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) light: u32,
) -> EntityLitOutput {
    var out: EntityLitOutput;
    out.clip_position = project(camera.view * vec4<f32>(position, 1.0));
    out.color = color;
    out.uv = uv;
    out.tint = tint;
    out.light = packed_scene_light(light, camera.params.w);
    return out;
}

@fragment
fn fs_entity_lit(in: EntityLitOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.5) {
        discard;
    }
    let base = linear_to_srgb(tex.rgb) * in.color;
    return vec4<f32>(tonemap_neutral(srgb_to_linear(mix(base, in.tint.rgb, in.tint.a)) * in.light), 1.0);
}

@fragment
fn fs_entity_translucent_lit(in: EntityLitOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.1) {
        discard;
    }
    let base = linear_to_srgb(tex.rgb) * in.color;
    return vec4<f32>(tonemap_neutral(srgb_to_linear(mix(base, in.tint.rgb, in.tint.a)) * in.light), tex.a);
}

// Die Ego-Hand: Licht an der Augenposition aus dem Hand-Kamera-Uniform (hitbox.x, Bits).
@fragment
fn fs_hand_lit(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.5) {
        discard;
    }
    let licht = packed_scene_light(bitcast<u32>(camera.hitbox.x), camera.params.w);
    return vec4<f32>(tonemap_neutral(tint_srgb(tex.rgb, in.color) * licht), 1.0);
}

// Vanillas rendertype_entity_translucent: erst unter 0.1 verworfen, das Alpha der Textur
// wird gemischt statt geschnitten (Windkugel: durchweg Alpha 127).
@fragment
fn fs_entity_translucent(in: EntityOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.1) {
        discard;
    }
    let base = linear_to_srgb(tex.rgb) * in.color;
    return vec4<f32>(srgb_to_linear(mix(base, in.tint.rgb, in.tint.a)), tex.a);
}

// Fuer die Wasser-Pipeline (echtes Alpha-Blending, kein Tiefenschreiben, s.
// render::BlockRenderer::translucent_pipeline): kein discard, und das echte, aus der
// Vanilla-Textur gesampelte Alpha wird durchgereicht statt hart auf 1.0 gesetzt -- genau
// das macht Wasser sichtbar durchscheinend (water_still.png traegt in Vanilla schon ein
// Alpha von 180/255, per PIL gegen die echte Textur geprueft).
// Geisterbloecke: eine noch nicht vom Server bestaetigte Platzierung (s. app::PendingBlock).
// Dieselbe Geometrie wie ein gewoehnlicher Block, aber halb durchsichtig -- man sieht sofort,
// wo der Block landet, und ebenso sofort, ob er bestaetigt wurde (dann zeichnet ihn der
// normale Chunk-Pass undurchsichtig). Der Alpha-Test bleibt, damit Glasraender nicht als
// graue Flaechen erscheinen.
const GHOST_ALPHA: f32 = 0.5;

@fragment
fn fs_main_ghost(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    if (tex.a < 0.5) {
        discard;
    }
    return vec4<f32>(tint_srgb(tex.rgb, in.color), GHOST_ALPHA);
}

// --- Hitbox-Anzeige (Modul, s. app::build_hitbox_vertices) -------------------------------
//
// Eine Kante ist ein Band, das ERST NACH der Projektion auf Breite gebracht wird: beide
// Endpunkte werden projiziert, die Verbreiterung geschieht in BILDSCHIRMPIXELN senkrecht zur
// projizierten Linie. Damit ist die Breite exakt die eingestellte Pixelzahl -- unabhaengig
// von Entfernung, Blickwinkel UND der nichtlinearen Projektion (s. project()).
//
// Genau daran scheiterten alle drei fruehreren CPU-Fassungen (der Reihe nach):
// ein achsenparalleler duenner Kasten verkuerzte sich je nach Blickwinkel; ein
// kamerazugewandtes Band mit EINER Entfernung je Box stimmte aus der Naehe nicht mehr; und
// selbst mit Entfernung je Kante blieb der FOV-Stretch zum Bildschirmrand hin uebrig, weil
// die Projektion dort staerker aufzieht als in der Bildmitte. Alle drei verschwinden, sobald
// die Breite nach der Projektion gemessen wird statt davor.
//
// Die TIEFE bleibt dabei die des echten 3D-Punkts (z/w aus der Projektion, nur x/y wandern)
// -- der Tiefentest greift also ganz normal, die Anzeige verschwindet weiterhin hinter
// Waenden. Das ist der Grund, warum das hier im Vertex-Shader passiert und nicht als
// 2D-Overlay auf der CPU: ein 2D-Pass haette keinen Tiefenpuffer und damit Roentgenblick.

struct HitboxOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Lage QUER zum Band: -1/+1 an der GEWUENSCHTEN Kante. Die Geometrie ist ggf. breiter
    // (mindestens camera.hitbox.y), dort geht der Betrag ueber 1 hinaus und der Fragment-Shader
    // blendet ihn aus.
    @location(1) across: f32,
};

@vertex
fn vs_hitbox(
    // Beide Endpunkte der Kante, augenrelativ -- IDENTISCH fuer alle 6 Vertices des Bandes.
    @location(0) pos_a: vec3<f32>,
    @location(1) pos_b: vec3<f32>,
    // x = Seite (-1/+1), y = an welchem Endpunkt dieser Vertex sitzt (0 = a, 1 = b).
    // Beide Endpunkte getrennt zu uebergeben statt "eigener + anderer Punkt" ist Absicht:
    // sonst zeigte die Richtung a->b fuer die b-Vertices rueckwaerts, die Normale kippte mit,
    // und das Band waere in sich verdreht.
    @location(2) side_and_end: vec2<f32>,
    // rgb + Deckkraft dieser Kante.
    @location(3) color: vec4<f32>,
    // Gewuenschte halbe Linienbreite in Pixeln (je Kante: Hitbox-Anzeige und Blockumriss
    // sind unabhaengig einstellbar).
    @location(4) half_width: f32,
) -> HitboxOutput {
    var out: HitboxOutput;
    out.color = color;

    let clip_a = project(camera.view * vec4<f32>(pos_a, 1.0));
    let clip_b = project(camera.view * vec4<f32>(pos_b, 1.0));

    // Hinter (oder auf) der Kameraebene laesst sich keine Bildschirmposition bilden -- die
    // Division unten wuerde Unsinn liefern und die Kante quer ueber den Bildschirm ziehen.
    // Solche Kanten werden verworfen, indem der Vertex aus dem Clip-Volumen geschoben wird
    // (z < 0 wird garantiert weggeschnitten). Betrifft nur Faelle, in denen man ohnehin im
    // Entity steht.
    if (clip_a.w < 1e-4 || clip_b.w < 1e-4) {
        out.clip_position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        out.across = 0.0;
        return out;
    }

    // Halbe Bildschirmgroesse: ein NDC-Schritt von 1.0 entspricht genau so vielen Pixeln.
    let half_screen = camera.hitbox.zw * 0.5;
    let px_a = clip_a.xy / clip_a.w * half_screen;
    let px_b = clip_b.xy / clip_b.w * half_screen;

    var dir = px_b - px_a;
    let len = length(dir);
    // Auf einen Punkt zusammengefallene Kante (genau von vorn gesehen): irgendeine Richtung
    // tut es, sichtbar ist davon ohnehin nur ein Punkt.
    if (len < 1e-6) {
        dir = vec2<f32>(1.0, 0.0);
    } else {
        dir = dir / len;
    }
    let normal = vec2<f32>(-dir.y, dir.x);

    let at_b = side_and_end.y;
    let clip_self = mix(clip_a, clip_b, at_b);
    let px_self = mix(px_a, px_b, at_b);

    let raster_half_px = max(half_width, camera.hitbox.y);
    let px_final = px_self + normal * side_and_end.x * raster_half_px;
    let ndc_final = px_final / half_screen;

    // x/y aus der Bildschirmrechnung, z/w UNVERAENDERT aus der Projektion des echten
    // 3D-Punkts -- nur so bleibt der Tiefentest exakt der eines Punkts auf der Kante.
    out.clip_position = vec4<f32>(ndc_final * clip_self.w, clip_self.z, clip_self.w);
    // Auf die GEWUENSCHTE Breite normiert: an der Geometriekante steht damit
    // raster_half/gewuenscht, also >= 1, sobald aufgeweitet wurde.
    out.across = side_and_end.x * (raster_half_px / max(half_width, 1e-6));
    return out;
}

// Analytische Kantenglaettung ueber die Bildschirm-Ableitung (fwidth) statt harter
// Pixelabschneidung: ohne das flackerte eine sehr duenne Kante zwischen 1 und 2 vollen
// Pixeln, je nachdem, wie sie zufaellig auf das Pixelraster fiel (bei
// eingestellten "1 px" mal 1, mal 2 Pixel breit auf einem FHD-Monitor). fwidth(across) ist
// die Aenderung von `across` zwischen zwei Nachbarpixeln, also "wie viel davon ist ein Pixel
// breit" -- smoothstep faedelt die Kante genau ueber diese Breite aus.
@fragment
fn fs_hitbox(in: HitboxOutput) -> @location(0) vec4<f32> {
    let aa = max(fwidth(in.across) * 0.5, 1e-4);
    let coverage = 1.0 - smoothstep(1.0 - aa, 1.0 + aa, abs(in.across));
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}

// Blockumriss im Modus Komplementaer (s. OutlineComplement): dieselben Kanten wie fs_hitbox,
// die Farbe aber je Pixel die Gegenfarbe des Bildes darunter -- Farbton +180 Grad, Saettigung
// und Helligkeit voll, wie beim Fadenkreuz (app crosshair.wgsl). `scene_copy` ist eine Kopie
// des Weltbilds von direkt vor diesem Durchgang; das Ziel selbst laesst sich nicht lesen.
// Eigene Bindung 2 in Gruppe 1: die Pipeline bindet dort nur diese Kopie, nicht den Atlas.
@group(1) @binding(2) var scene_copy: texture_2d<f32>;

fn complement_of(c: vec3<f32>) -> vec3<f32> {
    let maxc = max(c.r, max(c.g, c.b));
    let d = maxc - min(c.r, min(c.g, c.b));
    var h = 0.0;
    if (d > 0.0001) {
        if (maxc == c.r) {
            h = (c.g - c.b) / d;
        } else if (maxc == c.g) {
            h = (c.b - c.r) / d + 2.0;
        } else {
            h = (c.r - c.g) / d + 4.0;
        }
    }
    // Sechstel des Farbkreises, um drei weiter gedreht (= +180 Grad), auf [0,6).
    var hp = h + 3.0;
    if (hp < 0.0) {
        hp = hp + 6.0;
    }
    if (hp >= 6.0) {
        hp = hp - 6.0;
    }
    if (hp < 1.0) {
        return vec3<f32>(1.0, hp, 0.0);
    } else if (hp < 2.0) {
        return vec3<f32>(2.0 - hp, 1.0, 0.0);
    } else if (hp < 3.0) {
        return vec3<f32>(0.0, 1.0, hp - 2.0);
    } else if (hp < 4.0) {
        return vec3<f32>(0.0, 4.0 - hp, 1.0);
    } else if (hp < 5.0) {
        return vec3<f32>(hp - 4.0, 0.0, 1.0);
    }
    return vec3<f32>(1.0, 0.0, 6.0 - hp);
}

@fragment
fn fs_hitbox_complement(in: HitboxOutput) -> @location(0) vec4<f32> {
    let aa = max(fwidth(in.across) * 0.5, 1e-4);
    let coverage = 1.0 - smoothstep(1.0 - aa, 1.0 + aa, abs(in.across));
    let groesse = vec2<i32>(textureDimensions(scene_copy));
    let pixel = clamp(vec2<i32>(in.clip_position.xy), vec2<i32>(0), groesse - 1);
    let darunter = textureLoad(scene_copy, pixel, 0).rgb;
    return vec4<f32>(complement_of(darunter), in.color.a * coverage);
}

// Shader-Pfad: Wasser/Glas mit demselben Licht wie das Gelaende. Gezeichnet wird nach dem
// Tonemap ins Bildschirmformat -- dort ist bis 1 nichts komprimiert, also nur begrenzen.
struct LitVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: vec3<f32>,
};

@vertex
fn vs_terrain_lit(
    @location(0) packed: vec4<u32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<u32>,
    @location(3) column_offset: vec3<f32>,
    @location(5) eye_frac: vec3<f32>,
) -> LitVertexOutput {
    var out: LitVertexOutput;
    let position = vec3<f32>(packed.xyz) * (1.0 / 2048.0) - vec3<f32>(8.0) + vec3<f32>(0.0, f32(packed.w & 31u) * 16.0, 0.0);
    out.clip_position = project(camera.view * vec4<f32>(column_relative(position, column_offset, eye_frac), 1.0));
    out.color = color.rgb;
    out.uv = vec2<f32>(uv) * (1.0 / 65536.0);
    out.light = scene_light(unpack_light(packed.w, color.a), camera.params.w);
    return out;
}

@fragment
fn fs_terrain_translucent_lit(in: LitVertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    return vec4<f32>(min(tint_srgb(tex.rgb, in.color) * in.light, vec3<f32>(1.0)), tex.a);
}

@fragment
fn fs_main_translucent(in: VertexOutput) -> @location(0) vec4<f32> {
    // Negativer Bias verschiebt die von der GPU aus den UV-Ableitungen automatisch gewaehlte
    // Mip-Stufe Richtung schaerfer -- ohne Bias wirkten schon nahe Flaechen (v.a. Boden/Waende
    // in flachem Blickwinkel, wo die Ableitung in eine Achse gross ist) unnoetig verschwommen.
    // Anisotropes Filtern waere die "richtige" Loesung dafuer, ist aber mit dem bewusst
    // Nearest-gesampelten Atlas (scharfe Minecraft-Pixel aus der Naehe) nicht kombinierbar --
    // WebGPU/wgpu verlangt fuer anisotropy_clamp>1 zwingend lineares Mag-/Min-/Mipmap-Filtern.
    var tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    // Feuer (Farbe gekennzeichnet, s. world::BlockVertex::with_cutout_mark) wie Vanillas
    // Cutout hart: genau EINE Mip-Stufe, also Alpha 0 oder voll statt halb durchsichtiger
    // Mischung zweier Stufen. Die Stufe richtet sich nach der KLEINEREN Ableitung -- im
    // flachen Blickwinkel waehlte die groessere eine winzige Stufe, deren Deckungskorrektur
    // eine 1-Pixel-Flamme ueber die halbe Kachel zog.
    let texel = vec2<f32>(textureDimensions(atlas));
    let dx = dpdx(in.uv) * texel;
    let dy = dpdy(in.uv) * texel;
    var farbe = in.color;
    if (in.color.r < -0.5) {
        farbe = vec3<f32>(-1.0) - in.color;
        let stufe = max(floor(0.5 * log2(max(min(dot(dx, dx), dot(dy, dy)), 1e-12)) + MIPMAP_LOD_BIAS + 0.5), 0.0);
        tex = textureSampleLevel(atlas, atlas_sampler, in.uv, stufe);
    }
    return vec4<f32>(tint_srgb(tex.rgb, farbe), tex.a);
}

// --- Partikel (s. app::particles) --------------------------------------------------------
//
// Kamerazugewandte Quads, auf der CPU fertig aufgespannt und augenrelativ. Eigenes
// Vertexformat, weil Partikel eine ECHTE Deckkraft je Vertex tragen (Totem-Glitzer blendet
// aus) -- BlockVertex hat nur rgb.
//
// Vanillas particle.fsh: `color = texture * vertexColor; if (color.a < 0.1) discard;`. Die
// Helligkeit aus der Lichtkarte faellt weg -- der Client rendert bewusst fullbright.
struct ParticleOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_particle(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
) -> ParticleOutput {
    var out: ParticleOutput;
    out.clip_position = project(camera.view * vec4<f32>(position, 1.0));
    out.color = color;
    out.uv = uv;
    return out;
}

// OPAQUE_PARTICLE: Cutout, keine Mischung, schreibt Tiefe.
@fragment
fn fs_particle_opaque(in: ParticleOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    let a = tex.a * in.color.a;
    if (a < 0.1) {
        discard;
    }
    return vec4<f32>(tint_srgb(tex.rgb, in.color.rgb), 1.0);
}

// TRANSLUCENT_PARTICLE: echte Mischung (Totem, Blockstaub).
@fragment
fn fs_particle_translucent(in: ParticleOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    let a = tex.a * in.color.a;
    if (a < 0.1) {
        discard;
    }
    return vec4<f32>(tint_srgb(tex.rgb, in.color.rgb), a);
}

// Regen und Schnee (s. app::precipitation): Textur wiederholend (Sampler), ohne Mipmaps und
// ohne Mindestdeckkraft -- setzt der Regen ein, blendet seine Staerke von 0 an weich auf,
// statt erst bei 10 % schlagartig zu erscheinen wie mit der Schwelle der Partikel.
//
// Kugelgrenzen (Modul "Wetter"): radii = (innen, aussen) in Bloecken um die Kamera, 0 = keine.
// Gemessen wird je Pixel der 3D-Abstand zur Kamera -- ein Quad haengt senkrecht durch beide
// Kugeln, das ginge mit Werten an den Ecken nicht. Innen blendet es ueber 1,5 Bloecke ein,
// aussen ueber 2 aus.
const WEATHER_INNER_FADE: f32 = 1.5;
const WEATHER_OUTER_FADE: f32 = 2.0;

struct WeatherOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    // Augenrelative Position (die Vertices sind es schon).
    @location(2) rel: vec3<f32>,
    @location(3) @interpolate(flat) radii: vec2<f32>,
};

@vertex
fn vs_weather(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) radii: vec2<f32>,
) -> WeatherOutput {
    var out: WeatherOutput;
    out.clip_position = project(camera.view * vec4<f32>(position, 1.0));
    out.color = color;
    out.uv = uv;
    out.rel = position;
    out.radii = radii;
    return out;
}

@fragment
fn fs_weather(in: WeatherOutput) -> @location(0) vec4<f32> {
    let tex = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    var a = tex.a * in.color.a;
    let d = length(in.rel);
    if (in.radii.x > 0.0) {
        a *= clamp((d - in.radii.x) / WEATHER_INNER_FADE, 0.0, 1.0);
    }
    if (in.radii.y > 0.0) {
        a *= clamp((in.radii.y - d) / WEATHER_OUTER_FADE, 0.0, 1.0);
    }
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(tint_srgb(tex.rgb, in.color.rgb), a);
}

// --- Verzauberungs-Glint (s. render::glint) ----------------------------------------------
//
// Vertex-Shader: vs_main (dieselbe Geometrie wie das Item). color.xy traegt hier die
// Glint-UV, color.z die Wahl der Textur, `uv` die Atlas-UV des Items fuer die Maske.
struct GlintParams {
    // Zeilen der 2x3-Texturmatrix (TextureTransform.getGlintTransformation).
    row0: vec4<f32>,
    row1: vec4<f32>,
    // x = GlintAlpha (Optionen: glintStrength).
    params: vec4<f32>,
};
@group(2) @binding(0) var glint_item_texture: texture_2d<f32>;
@group(2) @binding(1) var glint_entity_texture: texture_2d<f32>;
@group(2) @binding(2) var glint_sampler: sampler;
@group(2) @binding(3) var<uniform> glint: GlintParams;

@fragment
fn fs_glint(in: VertexOutput) -> @location(0) vec4<f32> {
    // Alle Texturzugriffe VOR dem ersten discard: textureSample darf nicht in
    // ungleichmaessigem Kontrollfluss stehen. Aus demselben Grund werden beide Glint-
    // Texturen gelesen und erst danach gewaehlt.
    let maske = textureSampleBias(atlas, atlas_sampler, in.uv, MIPMAP_LOD_BIAS);
    let p = vec3<f32>(in.color.xy, 1.0);
    let guv = vec2<f32>(dot(glint.row0.xyz, p), dot(glint.row1.xyz, p));
    let a = textureSample(glint_item_texture, glint_sampler, guv);
    let b = textureSample(glint_entity_texture, glint_sampler, guv);
    let c = select(b, a, in.color.z > 0.0);
    // Nur wo das Item selbst gezeichnet wurde -- dieselbe Alpha-Schwelle wie fs_main.
    if (maske.a < 0.5 || c.a < 0.1) {
        discard;
    }
    // glint.fsh: color * GlintAlpha, gemischt mit (SRC_COLOR, ONE) -- Vanilla addiert also
    // s^2 auf den GAMMAKODIERTEN Wert d des Items. Unser Ziel mischt linear (ONE, ONE), also
    // wird genau die Differenz ausgegeben, die d auf d + s^2 hebt (s. render::glint).
    let s = linear_to_srgb(c.rgb) * glint.params.x;
    let d = linear_to_srgb(maske.rgb) * abs(in.color.z);
    return vec4<f32>(srgb_to_linear(min(d + s * s, vec3<f32>(1.0))) - srgb_to_linear(d), 0.0);
}
