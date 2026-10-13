// Shader-Pfad, Licht-Pass: aus dem G-Buffer wird das HDR-Bild (s. shader_path.rs).
//
// Albedo ist die reine Farbe (ohne Vanillas Seiten- und Eckabdunklung); hier kommt das Licht
// dazu:
// - Himmel: Umgebungslicht nach dem Himmelslicht vom Server, mit der Tageszeit gedimmt, mal
//   der gebackenen Sicht auf den Himmel.
// - Sonne bzw. Mond: gerichtetes Licht mit Schatten. Der Schatten ist ein Strahl durch das
//   Voxel-Abbild der Welt (s. voxel_volume.rs) -- exakte Blockformen, Laub laesst Licht
//   gesprenkelt durch, Glas faerbt es, Wasser schluckt es mit der Tiefe.
// - Blocklicht: farbig, vom Client ausgebreitet (s. world::light).
// Wo kein Gelaende liegt, steht der Himmel.

struct LightParams {
    // Himmelsfarbe, linear; w = Tageslicht 0..1.
    sky: vec4<f32>,
    // Bildschirm -> Sichtraum und Sichtraum -> Renderraum (Ursprung = Auge).
    inv_proj: mat4x4<f32>,
    inv_view: mat4x4<f32>,
    // Richtung zur Sonne bzw. zum Mond (Welt); w = 1, wenn Schatten verfolgt werden.
    light_dir: vec4<f32>,
    // Farbe * Staerke des gerichteten Lichts; w = Anteil des Umgebungslichts am Himmelslicht.
    light_color: vec4<f32>,
    // Auge = cam_block + cam_frac (Welt-Blockkoordinaten).
    cam_block: vec4<i32>,
    cam_frac: vec4<f32>,
    // Richtung zur Sonne (Welt), auch nachts; der Mond steht gegenueber. w = Lichtstreuung
    // der Atmosphaere (1 = Erde), skaliert alle Streukoeffizienten.
    sun_dir: vec4<f32>,
    // Senkrechte auf der Sonnenbahn: Sonne und Mond liegen mit einer Kante quer dazu.
    // w = 1: Himmel als Atmosphaere berechnen (Oberwelt), sonst der einfache Verlauf.
    sun_axis: vec4<f32>,
    // Licht-Raster der Blocklicht-Quellen: xyz = Ursprung (Weltblock), w = 1 an.
    light_grid: vec4<i32>,
    // x = tan(halber Winkel) der Blocklicht-Quellen: wie schnell ihr Halbschatten waechst;
    // y = Faktor auf Sonnen-/Mond- und Himmelslicht (Lichtstaerken); z = 1: Fernlichter an
    // (s. far_light); w = 1: Lichtkarten-Ansicht (s. lightmap_debug).
    block_soft: vec4<f32>,
};

@group(0) @binding(0) var albedo_tex: texture_2d<f32>;
@group(0) @binding(1) var normal_tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: LightParams;
@group(0) @binding(3) var light_tex: texture_2d<f32>;
@group(0) @binding(4) var depth_tex: texture_depth_2d;
// Voxel-Abbild: Klasse je Block, Achsen (x, z, y), Ringpuffer ueber 256 Bloecke.
@group(0) @binding(5) var voxels: texture_3d<u32>;
// Je Slot (16^3): welche Section er haelt; w = 0 unbekannt, 1 Bloecke, 2 nur Luft.
@group(0) @binding(6) var<storage, read> slots: array<vec4<i32>>;
// Je Klasse: x = Material | Boxanzahl << 8 | erste Box << 12, y = Toenung RGB8.
@group(0) @binding(7) var<storage, read> class_headers: array<vec2<u32>>;
// Je Box min/max in 1/16 Block, je 5 Bit.
@group(0) @binding(8) var<storage, read> class_boxes: array<u32>;
// Atlas-Kacheln der Kreuz-Modelle [u0, v0, u1, v1] und der Block-Atlas selbst.
@group(0) @binding(9) var<storage, read> cross_uvs: array<vec4<f32>>;
@group(0) @binding(10) var atlas_tex: texture_2d<f32>;
// Sonnenschatten dieses Frames, gemittelt mit den letzten (s. fs_shadow, shadow_temporal.wgsl).
@group(0) @binding(11) var shadow_tex: texture_2d<f32>;
// Je Zelle (8^3 Bloecke, 16^3 Zellen ab params.light_grid.xyz) bis zu LIGHTS_PER_CELL Quellen:
// x = Position im Raster (je Achse 10 Bit, +256), y = Emission (R | G << 4 | B << 8 | echte Stufe << 12); y = 0
// beendet die Liste.
@group(0) @binding(12) var<storage, read> light_grid: array<vec2<u32>>;
// Lightmaps des Blocklichts (s. cs_lm_faces): u32, hintereinander
//   [LM_SLOT_BASE ..)  je Voxel-Slot die Seite + 1 (0 = keine),
//   [LM_INDEX_BASE ..) je Seite und Blockflaeche (Block * 6 + Richtung) die Flaeche + 1 in
//                      dieser Seite (0 = kein Licht, LM_VOLL = kein Platz mehr),
@group(0) @binding(16) var<storage, read_write> lm: array<atomic<u32>>;
// Je Seite und Flaeche LM_LAGEN Licht-Ebenen, eine je Quelle (s. cs_lm_layers, Format dort).
@group(0) @binding(19) var<storage, read_write> lm_lagen: array<vec4<u32>>;
//   [LM_BAKE_BASE ..)  nur beim Backen: je Seite die Zahl ihrer Flaechen, dahinter je Seite
//                      und Flaeche ihre Blockflaeche.
// Die Back-Auftraege dieses Frames: xyz = Section, w = Seite; anzahl.x = wie viele, y = Bit je
// Auftrag: nur Blocklicht (s. cs_lm_layers), z = unterster Block der Welt (i32).
// Ein Auftrag ist ein Schritt: w = Seite | Schritt << 16 -- je Schritt LM_SCHRITT_FLAECHEN
// Flaechen der Seite (s. cs_lm_layers); Flaechen vormerken und verteilen nur im Schritt 0.
struct BakeJobs {
    jobs: array<vec4<i32>, 16>,
    anzahl: vec4<u32>,
};
@group(0) @binding(18) var<uniform> bake: BakeJobs;
// Fernlichter (s. far_light), die naechsten zuerst: xyz = Weltblock, w = Emission (R | G << 4
// | B << 8, hoechstens 15 je Kanal wie die Ausbreitung) | echte Stufe << 12; w = 0 beendet.
@group(0) @binding(14) var<storage, read> far_lights: array<vec4<i32>>;
// Fernlicht dieses Frames, gemittelt mit den letzten (s. fs_far_light).
@group(0) @binding(15) var far_light_tex: texture_2d<f32>;
// Je Block sein State (Achsen wie `voxels`): Farbe beim Rueckwurf, Material und Textur in
// Spiegelungen (s. block_farbe, material_an, block_texel).
@group(0) @binding(20) var voxel_states: texture_3d<u32>;
// Je State STATE_INFO_WORTE Worte, 256 States je Zeile (s. world::voxel::STATE_INFO_WORTE):
// Atlas-Rechteck je Seite, Biom-Toenung, Material.
@group(0) @binding(21) var state_info: texture_2d<u32>;
// Spiegelungen (s. fs_reflexion), gemittelt im Verlauf.
@group(0) @binding(22) var reflexion_tex: texture_2d<f32>;

const LIGHT_CELL: i32 = 8;
const LIGHT_CELLS: i32 = 16;
const LIGHTS_PER_CELL: i32 = 16;
const LIGHT_POS_BIAS: i32 = 256;
const FAR_LIGHTS_MAX: i32 = 32;
// Anteil des Blocklichts, der auch im Schatten bleibt: Licht, das von Waenden und Boden
// zurueckgeworfen wird.
const BLOCK_BOUNCE: f32 = 0.05;
// Anteil des Himmelslichts, den der Boden unter einer geneigten Flaeche zurueckwirft.
const BODEN_RUECKWURF: f32 = 0.3;

const LM_PAGES: u32 = 768u;
const LM_FACES: u32 = 1024u;
// So viele, wie eine Raster-Zelle Quellen haben kann: keine faellt heraus.
const LM_LAGEN: u32 = 16u;
// Flaechen je Back-Schritt (s. BakeJobs): eine Seite in LM_FACES / LM_SCHRITT_FLAECHEN
// Schritten, verteilt ueber die Frames.
const LM_SCHRITT_FLAECHEN: u32 = 64u;
const LM_SLOT_BASE: u32 = 0u;
const LM_INDEX_BASE: u32 = 4096u;
const LM_BAKE_BASE: u32 = 4096u + 768u * 24576u;
// Je Seite und Flaeche die Sicht auf den Himmel an 4x4 Punkten (s. back_himmel), je 8 Bit:
// Wort j = Zeile v = j / 3, Byte i = Spalte u = i / 3.
const LM_HIMMEL_BASE: u32 = LM_BAKE_BASE + LM_PAGES + LM_PAGES * LM_FACES;
// Zurueckgeworfenes Blocklicht je Flaeche an 4x4 Punkten (wie die Himmelssicht), je ein Wort
// RGB9E5 (s. back_rueckwurf).
const LM_RUECK_BASE: u32 = LM_HIMMEL_BASE + LM_PAGES * LM_FACES * 4u;
const RUECK_STRAHLEN: u32 = 512u;
// Sicht-Raster: je Seite ein Zaehler, dahinter LM_FEIN_PLAETZE Raster zu FEIN_N x FEIN_N
// Werten (8 Bit, FEIN_WORTE Worte) -- fuer jede Flaeche, die eine Quelle nur zum Teil sieht
// (s. cs_lm_layers, cs_lm_fein, fein_sicht).
const LM_FEIN_ZAEHLER_BASE: u32 = LM_RUECK_BASE + LM_PAGES * LM_FACES * 16u;
const LM_FEIN_BASE: u32 = LM_FEIN_ZAEHLER_BASE + LM_PAGES;
const LM_FEIN_PLAETZE: u32 = 512u;
// Je feinem Raster zwei Worte Kopf: Platz der Flaeche | Back-Schritt << 16, Block der Quelle
// relativ zur Flaeche (wie Bit 0-26 einer Ebene).
const LM_FEIN_KOPF_BASE: u32 = LM_FEIN_BASE + LM_PAGES * LM_FEIN_PLAETZE * FEIN_WORTE;
// 16 x 16 Punkte: so fein wie die Texel eines Blocks.
const FEIN_N: u32 = 16u;
const FEIN_WORTE: u32 = 64u;
// Strahlen je Punkt des feinen Rasters (feste Spirale: glatt, nicht verrauscht).
const FEIN_STRAHLEN: i32 = 128;
const RUECK_WEITE: f32 = 16.0;
// Weitere Rueckwuerfe (s. back_rueckwurf): hoechstens so viel des Lichts bleibt je Weg im Raum.
const RUECK_GESCHLOSSEN: f32 = 0.75;
// Biom-Toenung beim Rueckwurf: feste Mittelwerte statt der Biom-Karten (linear).
const TOENUNG_GRAS: vec3<f32> = vec3<f32>(0.28, 0.51, 0.10);
const TOENUNG_LAUB: vec3<f32> = vec3<f32>(0.18, 0.41, 0.03);
const TOENUNG_TROCKEN: vec3<f32> = vec3<f32>(0.37, 0.24, 0.08);
// Strahlen je Punkt und wie weit sie laufen: was so weit frei ist, sieht den Himmel (ob der
// dort wirklich offen ist, sagt das Himmelslicht vom Server).
const HIMMEL_STRAHLEN: u32 = 32u;
const HIMMEL_WEITE: f32 = 32.0;
const LM_VOLL: u32 = 0xFFFFFFFFu;
// Als erste Ebene einer Flaeche ausserhalb des Licht-Rasters (eine Section ragt ueber seinen
// Rand): dort kennt der Baker die nahen Quellen nicht, es bleibt beim Licht der Ausbreitung
// und dem Fernlicht des Licht-Passes. Die Himmelssicht gilt trotzdem.
const LM_AUSSEN: u32 = 0xFFFFFFFFu;
// Vorlaeufig im Flaechen-Index: diese Flaeche braucht eine Karte (s. cs_lm_assign).
const LM_GEWOLLT: u32 = 0xFFFFFFFEu;
// Strahlen je Texel und Quelle beim Backen: fest verteilt, nicht zufaellig -- das Ergebnis
// bleibt von Frame zu Frame gleich.
const LM_STRAHLEN: i32 = 32;
// Abtastpunkte je Flaechenrand beim Suchen der Schattengrenzen (s. cs_lm_layers).
const LM_RAND: u32 = 16u;
const LM_RAND_ABSTAND: f32 = 0.01;

const MAT_LEAVES: u32 = 1u;
const MAT_WATER: u32 = 2u;
const MAT_GLASS: u32 = 3u;
const MAT_PLANT: u32 = 4u;
const MAT_CROSS: u32 = 5u;
const MAT_PLANES: u32 = 6u;

// Modell aus flachen Ebenen (Seegras): je Ebene drei Eintraege in cross_uvs (s.
// world::voxel::MAT_PLANES). Wie beim Kreuz entscheidet das Alpha der Textur.
fn planes_block_ray(c: u32, o: vec3<f32>, d: vec3<f32>, cell: vec3<i32>, t0: f32, t1: f32) -> bool {
    let h = class_headers[c].x;
    let count = (h >> 8u) & 15u;
    let start = h >> 12u;
    let lo = o - vec3<f32>(cell);
    let groesse = vec2<f32>(textureDimensions(atlas_tex));
    for (var i = 0u; i < count; i++) {
        let a = cross_uvs[start + i * 3u];
        let e = cross_uvs[start + i * 3u + 1u];
        let r = cross_uvs[start + i * 3u + 2u];
        let achse = u32(a.x);
        var lage: f32;
        var richtung: f32;
        if (achse == 0u) {
            lage = lo.x;
            richtung = d.x;
        } else if (achse == 1u) {
            lage = lo.y;
            richtung = d.y;
        } else {
            lage = lo.z;
            richtung = d.z;
        }
        let t = (a.y - lage) / richtung;
        if (!(t >= t0 && t <= t1)) {
            continue;
        }
        let p = lo + d * t;
        var s: f32;
        var q: f32;
        if (achse == 0u) {
            s = p.z;
            q = p.y;
        } else if (achse == 1u) {
            s = p.x;
            q = p.z;
        } else {
            s = p.x;
            q = p.y;
        }
        if (s < a.z || s > a.w || q < e.x || q > e.y) {
            continue;
        }
        let fs = (s - a.z) / max(a.w - a.z, 1e-4);
        let fq = (q - e.x) / max(e.y - e.x, 1e-4);
        // Senkrechte Ebenen: v von unten (v1) nach oben (v0); waagerechte: v mit z.
        let v = select(mix(r.w, r.y, fq), mix(r.y, r.w, fq), achse == 1u);
        let uv = vec2<f32>(mix(r.x, r.z, fs), v);
        let texel = clamp(vec2<i32>(uv * groesse), vec2<i32>(0), vec2<i32>(groesse) - vec2<i32>(1));
        if (textureLoad(atlas_tex, texel, 0).a > 0.5) {
            return true;
        }
    }
    return false;
}

// Kreuz-Modell: die zwei diagonalen Ebenen wie im Mesher (x = z und x = 1 - z, volle Hoehe).
// Wo der Strahl eine Ebene schneidet, entscheidet das Alpha der Textur -- Gras wirft so den
// Schatten seiner Halme, nicht den einer Box.
fn cross_blocks_ray(c: u32, o: vec3<f32>, d: vec3<f32>, cell: vec3<i32>, t0: f32, t1: f32) -> bool {
    let r = cross_uvs[class_headers[c].y];
    let lo = o - vec3<f32>(cell);
    let groesse = vec2<f32>(textureDimensions(atlas_tex));
    for (var ebene = 0; ebene < 2; ebene++) {
        var t: f32;
        if (ebene == 0) {
            t = (lo.z - lo.x) / (d.x - d.z);
        } else {
            t = (1.0 - lo.x - lo.z) / (d.x + d.z);
        }
        if (!(t >= t0 && t <= t1)) {
            continue;
        }
        let p = lo + d * t;
        if (any(p < vec3<f32>(0.0)) || any(p > vec3<f32>(1.0))) {
            continue;
        }
        // UV wie mesher::push_quad_with_uv: u laeuft mit x, v von unten (v1) nach oben (v0).
        let uv = vec2<f32>(mix(r.x, r.z, p.x), mix(r.w, r.y, p.y));
        let texel = clamp(vec2<i32>(uv * groesse), vec2<i32>(0), vec2<i32>(groesse) - vec2<i32>(1));
        if (textureLoad(atlas_tex, texel, 0).a > 0.5) {
            return true;
        }
    }
    return false;
}

// Ein Dreieck, das den ganzen Bildschirm bedeckt.
@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

// Dieselbe Kurve wie block.wgsl::scene_light (Wasser/Glas zeichnen dort mit ihr).
fn light_curve(l: vec3<f32>) -> vec3<f32> {
    return l / (vec3<f32>(4.0) - 3.0 * l);
}

fn hash3(p: vec3<i32>) -> f32 {
    var h = u32(p.x) * 73856093u ^ u32(p.y) * 19349663u ^ u32(p.z) * 83492791u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xFFFFu) / 65535.0;
}

// Ein Strahlabschnitt [t0, t1] in der Zelle `cell` (relativ zu cam_block) trifft Klasse `c`:
// wie viel Licht kommt durch?
fn cell_transmit(c: u32, o: vec3<f32>, d: vec3<f32>, inv: vec3<f32>, cell: vec3<i32>, t0: f32, t1: f32) -> vec3<f32> {
    let h = class_headers[c];
    let material = h.x & 255u;
    // Ebenen-Modelle: Anzahl und Anfang zeigen in cross_uvs, nicht auf Boxen.
    if (material == MAT_PLANES) {
        return select(vec3<f32>(1.0), vec3<f32>(0.0), planes_block_ray(c, o, d, cell, t0, t1));
    }
    let count = (h.x >> 8u) & 15u;
    let start = h.x >> 12u;
    var weg = 0.0;
    var eintritt = 1e30;
    for (var i = 0u; i < count; i++) {
        let b = class_boxes[start + i];
        let lo = vec3<f32>(cell) + vec3<f32>(f32(b & 31u), f32((b >> 5u) & 31u), f32((b >> 10u) & 31u)) / 16.0;
        let hi = vec3<f32>(cell) + vec3<f32>(f32((b >> 15u) & 31u), f32((b >> 20u) & 31u), f32((b >> 25u) & 31u)) / 16.0;
        let a = (lo - o) * inv;
        let e = (hi - o) * inv;
        let rein = max(max(max(min(a.x, e.x), min(a.y, e.y)), min(a.z, e.z)), t0);
        let raus = min(min(min(max(a.x, e.x), max(a.y, e.y)), max(a.z, e.z)), t1);
        if (rein < raus) {
            weg = max(weg, raus - rein);
            eintritt = min(eintritt, rein);
        }
    }
    if (weg <= 0.0) {
        return vec3<f32>(1.0);
    }
    let tint = unpack4x8unorm(h.y).rgb;
    switch material {
        case MAT_LEAVES: {
            // Laub als Viertelblock-Wuerfel, je etwa jeder zweite besetzt (fest nach Position,
            // nicht zufaellig je Bildpunkt): der Strahl laeuft exakt durch dieses Raster, der
            // Schatten ist eine feste, scharfe Form.
            let teil = 4.0;
            let p0 = (o + d * (eintritt + 1e-4) - vec3<f32>(cell)) * teil;
            var zelle = clamp(vec3<i32>(floor(p0)), vec3<i32>(0), vec3<i32>(3));
            let schritt = vec3<i32>(select(vec3<f32>(-1.0), vec3<f32>(1.0), d > vec3<f32>(0.0)));
            let tdelta = abs(inv) / teil;
            let basis = vec3<f32>(cell) * teil;
            var tmax = ((basis + vec3<f32>(zelle) + select(vec3<f32>(0.0), vec3<f32>(1.0), d > vec3<f32>(0.0))) / teil - o) * inv;
            let ende = eintritt + weg;
            for (var i = 0; i < 12; i++) {
                if (any(zelle < vec3<i32>(0)) || any(zelle > vec3<i32>(3))) {
                    break;
                }
                if (hash3((cell + params.cam_block.xyz) * 4 + zelle) < 0.5) {
                    return vec3<f32>(0.0);
                }
                let tn = min(min(tmax.x, tmax.y), tmax.z);
                if (tn >= ende) {
                    break;
                }
                if (tmax.x <= tmax.y && tmax.x <= tmax.z) {
                    zelle.x += schritt.x;
                    tmax.x += tdelta.x;
                } else if (tmax.y <= tmax.z) {
                    zelle.y += schritt.y;
                    tmax.y += tdelta.y;
                } else {
                    zelle.z += schritt.z;
                    tmax.z += tdelta.z;
                }
            }
            return vec3<f32>(0.85);
        }
        case MAT_CROSS: {
            if (cross_blocks_ray(c, o, d, cell, t0, t1)) {
                return vec3<f32>(0.0);
            }
            return vec3<f32>(1.0);
        }
        case MAT_PLANT: {
            // Gras, Blumen, Spinnweben: duenne Kreuz-Ebenen, die nur einen Teil verdecken.
            return vec3<f32>(0.5);
        }
        case MAT_WATER: {
            return pow(tint, vec3<f32>(weg));
        }
        case MAT_GLASS: {
            return tint;
        }
        default: {
            return vec3<f32>(0.0);
        }
    }
}

fn slot_of(sec: vec3<i32>) -> vec4<i32> {
    let s = sec & vec3<i32>(15);
    return slots[(s.y * 16 + s.z) * 16 + s.x];
}

// Strahl ab `o` (Weltblock-Koordinaten relativ zu cam_block) in Richtung `d`: Durchlass bis
// `max_t`. Leere Sections werden in einem Schritt uebersprungen, in den anderen geht es von
// Block zu Block (Amanatides/Woo). Unbekanntes (ausserhalb des Abbilds, noch nicht
// geschrieben) zaehlt als frei; w = 1 meldet, dass der Strahl dort endete -- dann weiss nur
// noch das Himmelslicht des Servers, ob es hell ist.
// `treffer`: Abstand, an dem der Strahl zum ersten Mal Licht verlor (-1 = nie) -- daraus
// laesst sich ablesen, wie weit das Hindernis weg ist.
fn trace(o: vec3<f32>, d_roh: vec3<f32>, max_t: f32, treffer: ptr<function, f32>) -> vec4<f32> {
    // Achsen ohne Anteil (mittags ist die Ost-West-Richtung exakt 0) bekommen einen winzigen
    // POSITIVEN Anteil -- und zwar ueberall: Schrittrichtung, Kehrwert und Startabstaende
    // muessen dasselbe Vorzeichen sehen. Frueher galt der Ersatz nur im Kehrwert, waehrend
    // `d > 0` die exakte 0 als negativ wertete: der Strahl lief seitwaerts oder rueckwaerts
    // (Schattenflecken an falschen Stellen, Licht hinter Waenden).
    let d = select(d_roh, vec3<f32>(1e-6), abs(d_roh) < vec3<f32>(1e-6));
    let inv = 1.0 / d;
    let schritt = vec3<i32>(select(vec3<f32>(-1.0), vec3<f32>(1.0), d > vec3<f32>(0.0)));
    let tdelta = abs(inv);
    var durch = vec3<f32>(1.0);
    var t = 0.0;
    // Die Zelle wird Schritt fuer Schritt weitergereicht, auch ueber Section-Grenzen -- nie
    // aus der Position neu bestimmt. Frueher sprang der Strahl an jeder Grenze ein Stueck
    // weiter und rechnete die Zelle neu: lief er dort knapp an einer Blockecke vorbei, war
    // sie uebersprungen, und Licht kam durch einen haarfeinen Spalt (helle Linien hinter
    // Ecken).
    var cell = vec3<i32>(floor(o));
    var tmax = (vec3<f32>(cell) + select(vec3<f32>(0.0), vec3<f32>(1.0), d > vec3<f32>(0.0)) - o) * inv;
    for (var schritte = 0; schritte < 1024; schritte++) {
        if (t >= max_t) {
            break;
        }
        let w = cell + params.cam_block.xyz;
        let sec = w >> vec3<u32>(4u);
        let slot = slot_of(sec);
        if (slot.w == 0 || any(slot.xyz != sec)) {
            return vec4<f32>(durch, 1.0);
        }
        if (slot.w == 2) {
            // Leere Section in einem Schritt: bis an ihren Rand, genau in die Zelle dahinter.
            let lo = sec * 16 - params.cam_block.xyz;
            let ende = (vec3<f32>(select(lo, lo + vec3<i32>(16), d > vec3<f32>(0.0))) - o) * inv;
            let te = min(min(ende.x, ende.y), ende.z);
            // Achsen, die hier nicht austreten, liegen noch in dieser Section.
            let innen = clamp(vec3<i32>(floor(o + d * te)), lo, lo + vec3<i32>(15));
            let draussen = select(lo - vec3<i32>(1), lo + vec3<i32>(16), d > vec3<f32>(0.0));
            cell = select(innen, draussen, ende <= vec3<f32>(te));
            t = te;
            tmax = (vec3<f32>(cell) + select(vec3<f32>(0.0), vec3<f32>(1.0), d > vec3<f32>(0.0)) - o) * inv;
            continue;
        }
        let tn = min(min(tmax.x, tmax.y), tmax.z);
        let c = textureLoad(voxels, vec3<i32>(w.x & 255, w.z & 255, w.y & 255), 0).r;
        if (c == 1u) {
            if (*treffer < 0.0) {
                *treffer = t;
            }
            return vec4<f32>(0.0);
        }
        if (c != 0u) {
            let zelle = cell_transmit(c, o, d, inv, cell, t, tn);
            if (*treffer < 0.0 && min(zelle.r, min(zelle.g, zelle.b)) < 0.999) {
                *treffer = t;
            }
            durch *= zelle;
            if (max(durch.r, max(durch.g, durch.b)) < 0.02) {
                return vec4<f32>(0.0);
            }
        }
        t = tn;
        if (t >= max_t) {
            return vec4<f32>(durch, 0.0);
        }
        if (tmax.x <= tmax.y && tmax.x <= tmax.z) {
            cell.x += schritt.x;
            tmax.x += tdelta.x;
        } else if (tmax.y <= tmax.z) {
            cell.y += schritt.y;
            tmax.y += tdelta.y;
        } else {
            cell.z += schritt.z;
            tmax.z += tdelta.z;
        }
    }
    return vec4<f32>(durch, 0.0);
}

// Himmelsverlauf aus der Himmelsfarbe des Frames (params.sky, Vanillas Tagesfarbe): oben
// tiefer und satter, zum Horizont heller und blasser (mehr Luft im Blick), unter dem Horizont
// wieder dunkler. Steht die Sonne tief, gluehen Horizont und Gegend um die Sonne orange-rot;
// die Glut reicht auf der Sonnenseite weiter hinauf.
fn sky_gradient(blick: vec3<f32>, sonne: vec3<f32>) -> vec3<f32> {
    let basis = params.sky.rgb;
    let hell = dot(basis, vec3<f32>(0.2126, 0.7152, 0.0722));
    let zenit = basis * vec3<f32>(0.55, 0.72, 1.05);
    let horizont = mix(basis, vec3<f32>(hell * 1.6), 0.55) * 1.25;
    let h = blick.y;
    var farbe = mix(zenit, horizont, pow(1.0 - clamp(h, 0.0, 1.0), 4.0));
    if (h < 0.0) {
        farbe = mix(horizont, basis * 0.55, smoothstep(0.0, 0.35, -h));
    }
    // Daemmerung: am staerksten, wenn die Sonne um den Horizont steht.
    let daemmerung = 1.0 - smoothstep(0.0, 0.35, abs(sonne.y + 0.02));
    if (daemmerung > 0.0) {
        let seite = dot(normalize(vec2<f32>(blick.x, blick.z) + vec2<f32>(1e-5)), normalize(vec2<f32>(sonne.x, sonne.z) + vec2<f32>(1e-5))) * 0.5 + 0.5;
        let hoehe = exp(-max(h, 0.0) * mix(9.0, 3.5, seite)) * smoothstep(-0.25, 0.0, h);
        let glut = mix(vec3<f32>(0.9, 0.35, 0.12), vec3<f32>(1.0, 0.6, 0.25), seite);
        farbe = mix(farbe, glut * (0.25 + 0.75 * seite), daemmerung * hoehe * (0.35 + 0.65 * seite));
    }
    return farbe;
}

// --- Atmosphaere ------------------------------------------------------------------------
//
// Einfachstreuung in einer kugelfoermigen Lufthuelle (Erdradius, 100 km Atmosphaere).
// Fuer jeden Himmelspunkt laeuft ein Strahl vom Auge durch die Luft; an jedem Schritt kommt
// Sonnenlicht an (gedaempft auf seinem Weg dorthin) und wird mit einem Teil in Blickrichtung
// gestreut (gedaempft auf dem Weg zum Auge).
//
// Zwei Streuer. Jeder hat einen Streuquerschnitt: die effektive Trefferflaeche eines
// Teilchens fuer Licht, wie pi r^2 eine geometrische Trefferflaeche ist -- mal der
// Teilchendichte ergibt das den Streukoeffizienten beta (Treffer je Meter):
// - Rayleigh (Luftmolekuele): Querschnitt ~ 1/Wellenlaenge^4, Blau streut etwa 5,5-mal so
//   stark wie Rot. Daher der blaue Himmel, der hellere, blassere Horizont (langer Weg, alles
//   mehrfach gestreut und wieder herausgestreut) und das Rot der Daemmerung (auf dem langen
//   Weg der flachen Sonne ist das Blau schon weggestreut).
// - Mie (Dunst, Aerosole): kaum farbabhaengig, stark nach vorn -- der weisse Hof um die Sonne.
// Dichten fallen mit der Hoehe exponentiell ab (Skalenhoehe 8 km bzw. 1,2 km). Schritte
// quadratisch verteilt: dicht beim Auge, wo die Luft am dichtesten ist -- 16 + 6 Schritte
// liegen dann innerhalb weniger Prozent an einer Rechnung mit 200 + 40.
const ATM_PLANET: f32 = 6371e3;
const ATM_TOP: f32 = 6471e3;
const ATM_RAYLEIGH: vec3<f32> = vec3<f32>(5.802e-6, 13.558e-6, 33.1e-6);
const ATM_MIE: f32 = 3.996e-6;
const ATM_MIE_EXT: f32 = 4.4e-6;
const ATM_H_R: f32 = 8000.0;
const ATM_H_M: f32 = 1200.0;
const ATM_G: f32 = 0.76;
// Helligkeit der Sonne: so, dass der Mittagshimmel etwa so hell ist wie Vanillas Himmel.
const ATM_SUN: f32 = 36.0;

// Abstand bis zur Kugel mit Radius r (von innen), bzw. -1, wenn der Strahl vorher den Boden
// trifft und `boden` gesetzt ist.
fn atm_far(o: vec3<f32>, d: vec3<f32>, r: f32) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - r * r;
    return -b + sqrt(max(b * b - c, 0.0));
}

fn atm_hits_ground(o: vec3<f32>, d: vec3<f32>) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - ATM_PLANET * ATM_PLANET;
    let disc = b * b - c;
    if (b < 0.0 && disc > 0.0) {
        return -b - sqrt(disc);
    }
    return -1.0;
}

// Optische Tiefe (Rayleigh, Mie) von p bis zum Rand der Atmosphaere in Richtung d.
fn atm_optical_depth(p: vec3<f32>, d: vec3<f32>) -> vec2<f32> {
    let laenge = atm_far(p, d, ATM_TOP);
    var tiefe = vec2<f32>(0.0);
    for (var j = 0; j < 6; j++) {
        let b0 = f32(j) / 6.0;
        let b1 = f32(j + 1) / 6.0;
        let q = p + d * ((b0 * b0 + b1 * b1) * 0.5 * laenge);
        let h = length(q) - ATM_PLANET;
        tiefe += vec2<f32>(exp(-h / ATM_H_R), exp(-h / ATM_H_M)) * ((b1 * b1 - b0 * b0) * laenge);
    }
    return tiefe;
}

fn atm_extinction(tiefe: vec2<f32>) -> vec3<f32> {
    return exp(-(ATM_RAYLEIGH * tiefe.x + vec3<f32>(ATM_MIE_EXT * tiefe.y)) * params.sun_dir.w);
}

// Streulicht aus Richtung `blick` fuer eine Lichtquelle in Richtung `licht`.
fn atmosphere(blick: vec3<f32>, licht: vec3<f32>) -> vec3<f32> {
    let o = vec3<f32>(0.0, ATM_PLANET + 200.0, 0.0);
    // Kein Erdboden im Blick: Minecraft hat keinen Planeten unter dem Horizont. Mit Boden
    // wurde der Weg durch die Luft knapp unter dem Horizont schlagartig kurz -- hinter dem
    // Ende der Sichtweite stand dann ein dunkler Streifen mit harter Kante. Unter dem Horizont
    // gilt deshalb der Himmel am Horizont (s. sky_color: dort sanft abgedunkelt).
    let laenge = atm_far(o, blick, ATM_TOP);
    var tiefe = vec2<f32>(0.0);
    var summe_r = vec3<f32>(0.0);
    var summe_m = vec3<f32>(0.0);
    for (var i = 0; i < 16; i++) {
        let a0 = f32(i) / 16.0;
        let a1 = f32(i + 1) / 16.0;
        let schritt = (a1 * a1 - a0 * a0) * laenge;
        let p = o + blick * ((a0 * a0 + a1 * a1) * 0.5 * laenge);
        let h = length(p) - ATM_PLANET;
        let dichte = vec2<f32>(exp(-h / ATM_H_R), exp(-h / ATM_H_M)) * schritt;
        tiefe += dichte;
        // Liegt der Punkt im Erdschatten, kommt kein Sonnenlicht an.
        if (atm_hits_ground(p, licht) > 0.0) {
            continue;
        }
        let durch = atm_extinction(tiefe + atm_optical_depth(p, licht));
        summe_r += durch * dichte.x;
        summe_m += durch * dichte.y;
    }
    let mu = dot(blick, licht);
    let phase_r = 3.0 / (16.0 * 3.14159265) * (1.0 + mu * mu);
    let g2 = ATM_G * ATM_G;
    let phase_m = 3.0 / (8.0 * 3.14159265) * ((1.0 - g2) * (1.0 + mu * mu)) / ((2.0 + g2) * pow(1.0 + g2 - 2.0 * ATM_G * mu, 1.5));
    return (summe_r * ATM_RAYLEIGH * phase_r + summe_m * ATM_MIE * phase_m) * params.sun_dir.w;
}

// Wie viel vom Licht einer Quelle in Richtung `d` beim Auge ankommt (Sonnenscheibe: am
// Horizont rot und schwach).
fn atm_transmittance(d: vec3<f32>) -> vec3<f32> {
    let o = vec3<f32>(0.0, ATM_PLANET + 200.0, 0.0);
    if (atm_hits_ground(o, d) > 0.0) {
        return vec3<f32>(0.0);
    }
    return atm_extinction(atm_optical_depth(o, d));
}

// Wie viel einer quadratischen Scheibe (halbe Kantenlaenge `halb`, in Tangens-Einheiten) den
// Punkt `q` bedeckt: innen 1, aussen 0, dazwischen ein Uebergang von gut zwei Pixeln -- ohne
// ihn hatte die gedrehte Kante Treppenstufen.
fn scheibe_deckung(q: vec2<f32>, halb: f32) -> f32 {
    return 1.0 - smoothstep(halb - 0.003, halb + 0.001, max(q.x, q.y));
}

// Sterne: kleine Quadrate wie in Vanilla, fest am Himmel, der sich mit Sonne und Mond dreht.
// `d` ist die Blickrichtung in diesem mitdrehenden Rahmen. Der Himmel wird als Wuerfel
// aufgeteilt, je Seite STERN_RASTER^2 Felder; in etwa jedem dreissigsten steht ein Stern
// (zusammen rund 1600, Vanilla hat 1500) mit eigener Lage, Groesse und Helligkeit.
const STERN_RASTER: f32 = 90.0;

fn stars(d: vec3<f32>) -> f32 {
    let a = abs(d);
    var uv: vec2<f32>;
    var seite: i32;
    if (a.x >= a.y && a.x >= a.z) {
        uv = d.yz / a.x;
        seite = select(1, 0, d.x > 0.0);
    } else if (a.y >= a.z) {
        uv = d.xz / a.y;
        seite = select(3, 2, d.y > 0.0);
    } else {
        uv = d.xy / a.z;
        seite = select(5, 4, d.z > 0.0);
    }
    let g = (uv * 0.5 + 0.5) * STERN_RASTER;
    let feld = vec2<i32>(floor(g));
    let schluessel = vec3<i32>(feld, seite * 1000 + 7);
    if (hash3(schluessel) > 0.033) {
        return 0.0;
    }
    // Lage im Feld (mit Rand, damit der Stern nicht ans Nachbarfeld stoesst), Groesse und
    // Helligkeit aus weiteren Hashes.
    let mitte = vec2<f32>(feld) + vec2<f32>(0.25) + 0.5 * vec2<f32>(hash3(schluessel + vec3<i32>(1, 0, 0)), hash3(schluessel + vec3<i32>(0, 1, 0)));
    let halb = 0.06 + 0.06 * hash3(schluessel + vec3<i32>(0, 0, 1));
    let abstand = max(abs(g.x - mitte.x), abs(g.y - mitte.y));
    // Weicher Rand von etwa einem Pixel (ein Feld ist rund 16 Pixel breit).
    let deckung = 1.0 - smoothstep(halb - 0.04, halb + 0.04, abstand);
    return deckung * (0.5 + 0.5 * hash3(schluessel + vec3<i32>(1, 1, 1)));
}

// Sonne und Mond als quadratische Scheiben wie in Vanilla, mit weichem Schein um die Sonne.
// Die Kanten der Scheiben laufen entlang der Sonnenbahn bzw. quer dazu.
fn sky_color(blick: vec3<f32>) -> vec3<f32> {
    let sonne = params.sun_dir.xyz;
    let achse = params.sun_axis.xyz;
    let quer = cross(achse, sonne);
    let atmo = params.sun_axis.w > 0.5;
    var farbe: vec3<f32>;
    if (atmo) {
        // Sonne und, schwach, der Mond streuen beide. Dazu ein Rest von Vanillas Nachthimmel,
        // damit die Nacht nicht schwarz ist.
        // Unterhalb von HORIZONT_WEICH laeuft die Hoehe sanft gegen 0, statt am Horizont
        // abzubrechen: gleiche Steigung an der Uebergangsstelle, ein einziger Verlauf ohne
        // Knick. Vorher stand der Himmel unter dem Horizont fest auf der Horizontfarbe, und
        // der Knick im Verlauf sah aus wie eine Linie.
        let HORIZONT_WEICH = 0.08;
        let hoehe = select(HORIZONT_WEICH * exp((blick.y - HORIZONT_WEICH) / HORIZONT_WEICH), blick.y, blick.y >= HORIZONT_WEICH);
        let seitwaerts = normalize(vec2<f32>(blick.x, blick.z) + vec2<f32>(1e-5, 0.0)) * sqrt(max(1.0 - hoehe * hoehe, 0.0));
        let flach = vec3<f32>(seitwaerts.x, hoehe, seitwaerts.y);
        farbe = atmosphere(flach, sonne) * ATM_SUN + atmosphere(flach, -sonne) * (ATM_SUN * 0.004);
        farbe += params.sky.rgb * (1.0 - params.sky.w) * 0.5;
        farbe *= mix(1.0, 0.45, smoothstep(0.0, 0.4, -blick.y));
        // Sterne wie in Vanilla nach dem Sonnenstand: ab der Daemmerung sichtbar, nachts voll.
        // Ueber dem Horizont, zum Horizont hin schwaecher (dickere Luft).
        // Sonnenwinkel aus der Hoehe: sonne.y = cos(Winkel) * cos(Neigung), und die Achse der
        // Sonnenbahn hat z = cos(Neigung) (s. shader_path::sun_path_axis).
        let cos_winkel = sonne.y / max(achse.z, 1e-4);
        let nacht = clamp(1.0 - (cos_winkel * 2.0 + 0.25), 0.0, 1.0);
        if (nacht > 0.0 && blick.y > 0.0) {
            let im_himmel = vec3<f32>(dot(blick, sonne), dot(blick, quer), dot(blick, achse));
            farbe += vec3<f32>(0.9, 0.92, 1.0) * stars(im_himmel) * nacht * smoothstep(0.0, 0.25, blick.y);
        }
    } else {
        farbe = sky_gradient(blick, sonne);
    }
    let s = dot(blick, sonne);
    if (s > 0.0) {
        let q = abs(vec2<f32>(dot(blick, quer), dot(blick, achse))) / s;
        let auf = smoothstep(-0.1, 0.05, sonne.y);
        // Schein: am Horizont waermer.
        // Enger heller Hof plus ein schwacher weiter -- ein einzelner breiter Hof legte einen
        // hellen Streifen bis zum Horizont.
        let schein = (pow(max(s, 0.0), 600.0) * 1.2 + pow(max(s, 0.0), 12.0) * 0.12) * auf;
        // Die Atmosphaere bringt ihren Hof selbst mit (Mie-Streuung).
        if (!atmo) {
            farbe += mix(vec3<f32>(1.0, 0.5, 0.2), vec3<f32>(1.0, 0.9, 0.7), smoothstep(0.0, 0.4, sonne.y)) * schein;
        }
        let deckung = scheibe_deckung(q, 0.075);
        if (deckung > 0.0) {
            if (atmo) {
                // Die Scheibe hinter der Luft, tief am Horizont rot und schwach. Sie leuchtet
                // ZUSAETZLICH zum Himmel: frueher ersetzte sie ihn, und kurz vor dem Untergang
                // (fast nichts kommt mehr durch) stand ein dunkles Quadrat im Abendrot.
                farbe += atm_transmittance(sonne) * 8.0 * deckung;
            } else {
                farbe = mix(farbe, vec3<f32>(6.0, 5.4, 4.2), auf * deckung);
            }
        }
    } else {
        let mond = -sonne;
        let m = -s;
        let q = abs(vec2<f32>(dot(blick, quer), dot(blick, achse))) / m;
        let auf = smoothstep(-0.1, 0.05, mond.y);
        let deckung = scheibe_deckung(q, 0.05);
        if (deckung > 0.0) {
            farbe = max(farbe, mix(farbe, vec3<f32>(1.1, 1.15, 1.25), auf * deckung));
        }
    }
    return farbe;
}

// Eine Gelaende-Flaeche im G-Buffer: Normale (Welt) und Startpunkt fuer Strahlen.
struct Flaeche {
    n: vec3<f32>,
    // Punkt auf der Flaeche selbst, wie o relativ zu cam_block.
    b: vec3<f32>,
    // Weltblock-Koordinaten relativ zu cam_block, etwas vor der Flaeche.
    o: vec3<f32>,
};

fn flaeche(p: vec4<f32>, pixel: vec2<i32>) -> Flaeche {
    let n_view = textureLoad(normal_tex, pixel, 0).xyz * 2.0 - vec3<f32>(1.0);
    var n = normalize((params.inv_view * vec4<f32>(n_view, 0.0)).xyz);
    // Die Normale kommt aus Bildschirm-Ableitungen und wird in der Ferne unruhig. Blockflaechen
    // liegen fast alle achsenparallel: auf die naechste Achse einrasten, sonst startet der
    // Schattenstrahl mal vor, mal hinter der Flaeche (Schattenflecken).
    let an = abs(n);
    let groesste = max(an.x, max(an.y, an.z));
    if (groesste > 0.9) {
        n = select(vec3<f32>(0.0), sign(n), an == vec3<f32>(groesste));
    }
    // Position aus der Tiefe (Renderraum, Ursprung = Auge), dann in Blockkoordinaten relativ
    // zu cam_block. Blockflaechen liegen auf 1/16-Ebenen: entlang einer achsenparallelen
    // Normale auf die naechste einrasten, das gleicht die Tiefen-Ungenauigkeit in der Ferne
    // aus. Dann etwas von der Flaeche weg, damit Strahlen nicht im eigenen Block starten.
    let size = vec2<f32>(textureDimensions(depth_tex));
    let ndc = vec2<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0);
    let z = textureLoad(depth_tex, pixel, 0);
    let v = params.inv_proj * vec4<f32>(ndc, z, 1.0);
    let welt = (params.inv_view * vec4<f32>(v.xyz / v.w, 1.0)).xyz;
    var o = welt + params.cam_frac.xyz;
    o = select(o, round(o * 16.0) / 16.0, abs(n) > vec3<f32>(0.99));
    let b = o;
    o += n * (0.01 + 0.001 * length(welt));
    return Flaeche(n, b, o);
}

// Bildschirm-Rauschen (Interleaved Gradient Noise): dreht das Strahlmuster je Pixel, und
// jeden Frame ein Stueck weiter (Frame in cam_block.w) -- der Verlauf (shadow_temporal.wgsl)
// mittelt ueber die Frames, die Strahlen decken so mit der Zeit die ganze Scheibe ab.
fn ign(p: vec2<f32>) -> f32 {
    let q = p + vec2<f32>(5.588238) * f32(params.cam_block.w % 64);
    return fract(52.9829189 * fract(dot(q, vec2<f32>(0.06711056, 0.00583715))));
}

// Schatten-Pass: SONNE_STRAHLEN Strahlen ueber die Sonnenscheibe verteilt (Groesse:
// cam_frac.w = tan des halben Sonnendurchmessers). rgb = ankommendes Licht. Der Halbschatten
// entsteht im Verlauf: Frame fuer Frame treffen die Strahlen andere Stellen der Scheibe.
const SONNE_STRAHLEN: i32 = 2;

@fragment
fn fs_shadow(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(p.xy);
    if (textureLoad(albedo_tex, pixel, 0).a < 0.5) {
        return vec4<f32>(1.0, 1.0, 1.0, -1.0);
    }
    let f = flaeche(p, pixel);
    let l = params.light_dir.xyz;
    if (params.light_dir.w < 0.5 || dot(f.n, l) <= 0.0) {
        return vec4<f32>(1.0, 1.0, 1.0, -1.0);
    }
    // Nur wo ein Strahl ins Unbekannte laeuft (ausserhalb des Abbilds), haelt das Himmelslicht
    // des Servers Sonne aus Hoehlen heraus.
    let offen = clamp((textureLoad(light_tex, pixel, 0).a * 15.0 - 6.0) / 6.0, 0.0, 1.0);
    let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(l.y) > 0.9);
    let u = normalize(cross(l, hilf));
    let w = cross(l, u);
    let drehung = ign(p.xy) * 6.2831853;
    // Radius je Frame neu gewuerfelt (wie die Drehung), sonst saehe jeder Pixel nur feste Ringe.
    let radius = fract(ign(p.yx + vec2<f32>(17.0, 31.0)));
    var durch = vec3<f32>(0.0);
    for (var k = 0; k < SONNE_STRAHLEN; k++) {
        let r = sqrt(fract(radius + f32(k) / f32(SONNE_STRAHLEN))) * params.cam_frac.w;
        let winkel = f32(k) * 2.39996 + drehung;
        let dir = normalize(l + (u * cos(winkel) + w * sin(winkel)) * r);
        var t_hit = -1.0;
        let s = trace(f.o, dir, 160.0, &t_hit);
        durch += s.rgb * select(1.0, offen, s.w > 0.5);
    }
    return vec4<f32>(durch / f32(SONNE_STRAHLEN), 0.0);
}

// --- Lightmaps fuer Blocklicht ----------------------------------------------------------
//
// Das farbige Blocklicht der Ausbreitung (wie Vanilla, s. world::light) sagt nicht, woher es
// kommt, und kriecht um jede Ecke. Deshalb bekommt jede Blockflaeche, die eine Quelle
// erreicht, eine eigene Lichtkarte: je Quelle eine Ebene mit ihren Schattengrenzen als Linien
// (s. Schattengrenzen unten). Gebacken wird nur, wenn sich in der Naehe etwas aendert (s.
// VoxelVolume::update_lights), je Frame so viele Sections, wie in die Zeit passen; alles
// andere liest der Licht-Pass nur noch aus dem Speicher.
//
// Platz: Seiten zu je einer Section, LM_PAGES Stueck; je Seite bis LM_FACES Flaechen. Was
// keinen Platz bekommt, behaelt das Licht der Ausbreitung.

// Richtungen wie world::light: unten, oben, Norden (-z), Sueden (+z), Westen (-x), Osten (+x).
fn lm_normale(d: u32) -> vec3<i32> {
    switch d {
        case 0u: { return vec3<i32>(0, -1, 0); }
        case 1u: { return vec3<i32>(0, 1, 0); }
        case 2u: { return vec3<i32>(0, 0, -1); }
        case 3u: { return vec3<i32>(0, 0, 1); }
        case 4u: { return vec3<i32>(-1, 0, 0); }
        default: { return vec3<i32>(1, 0, 0); }
    }
}

// Die beiden Achsen in der Flaeche (Texel-u und -v).
fn lm_achse_u(d: u32) -> vec3<f32> {
    return select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), d >= 4u);
}

fn lm_achse_v(d: u32) -> vec3<f32> {
    return select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), d < 2u);
}

// Klasse des Voxels an `rel` (relativ zu cam_block); ausserhalb des Abbilds Luft.
fn voxel_at(rel: vec3<i32>) -> u32 {
    let w = rel + params.cam_block.xyz;
    let sec = w >> vec3<u32>(4u);
    let slot = slot_of(sec);
    if (slot.w != 1 || any(slot.xyz != sec)) {
        return 0u;
    }
    return textureLoad(voxels, vec3<i32>(w.x & 255, w.z & 255, w.y & 255), 0).r;
}

// Anfang der Quellen-Liste der Licht-Raster-Zelle um `p`, -1 ausserhalb des Rasters.
fn light_cell(p: vec3<f32>) -> i32 {
    let rel = vec3<i32>(floor(p)) + params.cam_block.xyz - params.light_grid.xyz;
    if (any(rel < vec3<i32>(0)) || any(rel >= vec3<i32>(LIGHT_CELL * LIGHT_CELLS))) {
        return -1;
    }
    let zelle = rel / LIGHT_CELL;
    return ((zelle.y * LIGHT_CELLS + zelle.z) * LIGHT_CELLS + zelle.x) * LIGHTS_PER_CELL;
}

// Untere Ecke des Blocks einer Quelle aus dem Licht-Raster, relativ zu cam_block.
fn light_block(e: vec2<u32>) -> vec3<i32> {
    return vec3<i32>(vec3<u32>(e.x & 1023u, (e.x >> 10u) & 1023u, (e.x >> 20u) & 1023u)) + params.light_grid.xyz - params.cam_block.xyz - vec3<i32>(LIGHT_POS_BIAS);
}

fn light_color_of(e: vec2<u32>) -> vec3<f32> {
    return vec3<f32>(f32(e.y & 15u), f32((e.y >> 4u) & 15u), f32((e.y >> 8u) & 15u));
}

// Wie weit eine Quelle reicht: ihre echte Stufe (Fernlichter ueber 15, s. far_light).
fn light_range(e: vec2<u32>) -> f32 {
    let farbe = light_color_of(e);
    return max(max(farbe.r, max(farbe.g, farbe.b)), f32((e.y >> 12u) & 255u));
}

// Strahlen je Quelle: ohne Weichheit (0 Grad) zeigen alle in dieselbe Richtung, einer reicht.
fn lm_strahlen() -> i32 {
    return select(LM_STRAHLEN, 1, params.block_soft.x < 1e-5);
}

// Wie hell Quelle `e` (Block `lo`, relativ zu cam_block) eine Stelle `o` kurz vor einer
// Flaeche mit Normale `n` bei freier Sicht macht, in der Einheit von light_curve: so hell,
// wie ihre Stufe nach dem Abstand noch ist, mal Lage zur Quelle. `eigen`: die Flaeche gehoert
// zum Block der Quelle (Glowstone, Laterne) -- volles Licht.
fn light_strength(e: vec2<u32>, lo: vec3<f32>, o: vec3<f32>, n: vec3<f32>, eigen: bool) -> vec3<f32> {
    let farbe = light_color_of(e);
    let stufe = max(max(farbe.r, max(farbe.g, farbe.b)), 1.0);
    let zu = lo + vec3<f32>(0.5) - o;
    let abstand = length(zu);
    if (abstand >= light_range(e)) {
        return vec3<f32>(0.0);
    }
    let nah = light_curve(max(farbe - vec3<f32>(abstand), vec3<f32>(0.0)) / 15.0);
    // Fernlicht: was ueber Stufe 15 hinausgeht (s. far_light), gerichteter als das nahe.
    let fern = farbe / stufe * max(fern_kurve((light_range(e) - abstand) / 15.0) - fern_kurve((stufe - abstand) / 15.0), 0.0) / (1.0 - BLOCK_BOUNCE);
    if (eigen) {
        return nah + fern;
    }
    // Wie schraeg das Licht auftrifft (Lambert): streifend kommt fast nichts an. Frueher 0,6 +
    // 0,4 * cos -- dann leuchtete Licht, das flach ueber den Boden streicht (Lampe auf
    // Bodenhoehe hinter einem Spalt), den Boden noch weit hinaus hell aus.
    let ndl = dot(n, zu / abstand);
    if (ndl <= 0.0) {
        return vec3<f32>(0.0);
    }
    return (nah + fern) * ndl;
}

// Strahl von `o` in Richtung `dir` zur Quelle in Block `lo`, bis zum Eintritt in ihren Block
// (der eigene Block -- Fackelstiel, Laternenkaefig -- soll sie nicht verdecken). Durchlass
// rgb; `treffer`: Abstand zum ersten Hindernis (-1 = keins). Er endet nur einen Hauch vor dem
// Block: frueher 0,01 davor, und was er auf diesem letzten Stueck streifte -- die Ecke eines
// Nachbarblocks der Quelle --, verdeckte nichts. Licht kam als feine Linie an Ecken vorbei.
fn ray_to_light(o: vec3<f32>, dir: vec3<f32>, lo: vec3<f32>, treffer: ptr<function, f32>) -> vec3<f32> {
    return trace(o, dir, light_ray_length(o, dir, lo), treffer).rgb;
}

// Wie weit ein Strahl von `o` in Richtung `dir` zur Quelle in Block `lo` laeuft: bis kurz vor
// ihren Block. Strahlen auf den Rand ihrer Scheibe (Weichheit) verfehlen den Block oft --
// die laufen bis dorthin, wo sie der Mitte am naechsten kommen. Frueher galt auch fuer sie der
// Eintritt in den Block, und der liegt bei einem Fehlschuss irgendwo davor: ein Strahl auf
// einen Teil der Scheibe, der in der Wand neben der Quelle steckt, kam durch die Wand.
fn light_ray_length(o: vec3<f32>, dir: vec3<f32>, lo: vec3<f32>) -> f32 {
    let d = select(dir, vec3<f32>(1e-6), abs(dir) < vec3<f32>(1e-6));
    let inv = 1.0 / d;
    let a = (lo - o) * inv;
    let b = (lo + vec3<f32>(1.0) - o) * inv;
    let rein = max(max(min(a.x, b.x), min(a.y, b.y)), min(a.z, b.z));
    let raus = min(min(max(a.x, b.x), max(a.y, b.y)), max(a.z, b.z));
    if (rein <= raus && raus > 0.0) {
        return max(rein - 1e-4, 0.0);
    }
    return max(dot(lo + vec3<f32>(0.5) - o, dir), 0.0);
}

// Sicht auf Quelle `lo` von `o` aus mit `strahlen` festen Strahlen ueber ihre Scheibe
// (Vogel-Spirale, params.block_soft), die Spirale um `drehung` (0..1) gedreht.
fn light_visibility_n(o: vec3<f32>, lo: vec3<f32>, strahlen: i32, drehung: f32) -> vec3<f32> {
    let mitte_dir = normalize(lo + vec3<f32>(0.5) - o);
    let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(mitte_dir.y) > 0.9);
    let u = normalize(cross(mitte_dir, hilf));
    let w = cross(mitte_dir, u);
    var durch = vec3<f32>(0.0);
    for (var s = 0; s < strahlen; s++) {
        let r = sqrt((f32(s) + 0.5) / f32(strahlen)) * params.block_soft.x;
        let winkel = f32(s) * 2.39996 + drehung * 6.2831853;
        var t_hit = -1.0;
        durch += ray_to_light(o, normalize(mitte_dir + (u * cos(winkel) + w * sin(winkel)) * r), lo, &t_hit);
    }
    return durch / f32(strahlen);
}

// Dasselbe je Pixel im Licht-Pass, fuer Flaechen, deren Schatten sich weder als Linien noch als
// Raster beschreiben laesst (s. cs_lm_layers): lm_strahlen() Strahlen. Jeder Strahl ist frei
// oder nicht -- mit derselben Spirale ueberall lagen die Spruenge auf durchgehenden Linien
// (Streifen im Licht). Die Spirale ist darum je Stelle der Welt anders gedreht: feines Korn,
// das an der Welt haengt und nicht wandert.
fn light_visibility_rays(o: vec3<f32>, lo: vec3<f32>) -> vec3<f32> {
    let stelle = vec3<i32>(floor((o + vec3<f32>(params.cam_block.xyz & vec3<i32>(1023))) * 64.0));
    return light_visibility_n(o, lo, lm_strahlen(), hash3(stelle));
}

// Erster Back-Schritt, je Blockflaeche einer Section ein Thread: braucht die Flaeche eine
// Lichtkarte? Ja, wenn sie zu sehen sein kann (fester Block mit achsenparallelen Flaechen,
// davor weder ein voller Block noch derselbe Block -- zwischen zwei Schneeschichten,
// Glasscheiben, Laubbloecken sieht man nichts): jede braucht ihre Sicht auf den Himmel.
// Vorgemerkt wird nur; die Plaetze verteilt cs_lm_assign.
@compute @workgroup_size(64)
fn cs_lm_faces(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.y >= bake.anzahl.x || id.x >= 4096u * 6u) {
        return;
    }
    let job = bake.jobs[id.y];
    if ((u32(job.w) >> 16u) != 0u) {
        return;
    }
    let seite = u32(job.w) & 0xFFFFu;
    let i = id.x / 6u;
    let d = id.x % 6u;
    let rel = job.xyz * 16 + vec3<i32>(i32(i % 16u), i32(i / 256u), i32((i / 16u) % 16u)) - params.cam_block.xyz;
    let klasse = voxel_at(rel);
    if (klasse == 0u) {
        return;
    }
    let material = class_headers[klasse].x & 255u;
    if (material == MAT_PLANT || material == MAT_CROSS) {
        return;
    }
    let n = lm_normale(d);
    let vor = voxel_at(rel + n);
    if (vor == 1u || vor == klasse) {
        return;
    }
    // Davor nichts Geladenes (unter der Welt, Rand des Abbilds): nie zu sehen. Gerade die
    // Unterseiten am Weltboden fuellten sonst eine Seite halb.
    let vor_welt = rel + n + params.cam_block.xyz;
    let vor_slot = slot_of(vor_welt >> vec3<u32>(4u));
    if (vor_welt.y < bitcast<i32>(bake.anzahl.z) || vor_slot.w == 0 || any(vor_slot.xyz != vor_welt >> vec3<u32>(4u))) {
        return;
    }
    // Auch ausserhalb des Licht-Rasters (s. LM_AUSSEN): welche Flaechen einen Platz bekommen,
    // haengt nur an den Bloecken -- sonst verschoeben sich die Plaetze, wenn das Raster
    // wandert, und die Himmelssicht (die dabei bleibt) laege auf fremden Flaechen.
    atomicStore(&lm[LM_INDEX_BASE + seite * 24576u + id.x], LM_GEWOLLT);
}

// Zweiter Back-Schritt, je Seite eine Gruppe: die vorgemerkten Flaechen bekommen ihre
// Plaetze in fester Reihenfolge (nach Blockflaeche), nicht in der zufaelligen, in der die
// Threads fertig werden -- dieselbe Welt ergibt dieselben Karten. Reichen die Plaetze nicht,
// gehen immer dieselben leer aus.
var<workgroup> lm_summen: array<u32, 256>;

@compute @workgroup_size(256)
fn cs_lm_assign(@builtin(workgroup_id) gruppe: vec3<u32>, @builtin(local_invocation_id) lokal: vec3<u32>) {
    if (gruppe.y >= bake.anzahl.x) {
        return;
    }
    let w = u32(bake.jobs[gruppe.y].w);
    if ((w >> 16u) != 0u) {
        return;
    }
    let seite = w & 0xFFFFu;
    let index = LM_INDEX_BASE + seite * 24576u;
    // Je Thread 96 aufeinanderfolgende Blockflaechen.
    let anfang = lokal.x * 96u;
    var zahl = 0u;
    for (var k = 0u; k < 96u; k++) {
        zahl += u32(atomicLoad(&lm[index + anfang + k]) == LM_GEWOLLT);
    }
    lm_summen[lokal.x] = zahl;
    workgroupBarrier();
    // Wie viele davor (Praefixsumme ueber die Threads, Hillis-Steele).
    for (var schritt = 1u; schritt < 256u; schritt *= 2u) {
        var plus = 0u;
        if (lokal.x >= schritt) {
            plus = lm_summen[lokal.x - schritt];
        }
        workgroupBarrier();
        lm_summen[lokal.x] += plus;
        workgroupBarrier();
    }
    var platz = lm_summen[lokal.x] - zahl;
    for (var k = 0u; k < 96u; k++) {
        let f = anfang + k;
        if (atomicLoad(&lm[index + f]) != LM_GEWOLLT) {
            continue;
        }
        if (platz >= LM_FACES) {
            atomicStore(&lm[index + f], LM_VOLL);
        } else {
            atomicStore(&lm[LM_BAKE_BASE + LM_PAGES + seite * LM_FACES + platz], f);
            atomicStore(&lm[index + f], platz + 1u);
        }
        platz += 1u;
    }
    if (lokal.x == 255u) {
        atomicStore(&lm[LM_BAKE_BASE + seite], min(lm_summen[255], LM_FACES));
    }
}

// --- Schattengrenzen statt Texel --------------------------------------------------------
//
// Mit Weichheit (der Normalfall) wird gerechnet wie bei einem Raytracer: je Flaeche und Quelle
// pruefen 32 Strahlen an jedem Punkt eines 16x16-Rasters, ob die Flaeche die Scheibe der Quelle
// ganz, gar nicht oder zum Teil sieht (s. pruef_punkt). Ganz: frei, gar nicht: keine Ebene, zum
// Teil: ein Sicht-Raster mit 128 Strahlen je Punkt (s. cs_lm_fein), im Licht-Pass bilinear
// gelesen -- die Punkte am Rand liegen auf der Blockkante, die Nachbarflaeche misst dort
// dasselbe. Frueher standen hier Naeherungen (Linien, Kantencodes, ein 8x8-Raster), jede mit
// ihren eigenen Fehlern: Treppen und Knicke an den Schattenraendern.
//
// Ohne Weichheit (harte Schatten) bleibt es bei Linien: Eine Schattenkante auf einer
// Blockflaeche ist eine gerade Linie, die Kante des Hindernisses von der Quelle aus auf die
// Flaeche projiziert. Statt der Helligkeit in einem Raster (das eine schraege Kante nur als
// Treppe kennt) haelt jede Flaeche je Quelle eine Ebene mit den Stellen, an denen ihre
// Schattengrenzen den Flaechenrand schneiden -- daraus wird die Linie im Licht-Pass je Pixel
// exakt. Die Helligkeit selbst (Abstand, Lage zur Quelle) rechnet der Licht-Pass ohnehin
// genau; gebacken wird nur die Sicht.
//
// Lage auf der Flaeche: u, v in [0, 1] entlang lm_achse_u/_v. Der Rand als eine Zahl
// s in [0, 4): unten (v = 0) von u = 0 bis 1, rechts (u = 1) hinauf, oben zurueck, links
// hinab -- einmal herum, gegen den Uhrzeigersinn.
//
// Eine Ebene (vec4<u32>):
//   x: Bit 0-26 Block der Quelle relativ zum Block der Flaeche (je Achse 9 Bit, +256),
//      Bit 27-28 Art: 0 keine (Ende der Liste), 1 frei, 2 Linien, 3 Strahlen (s. unten),
//      Bit 29 zwei Linien statt einer, Bit 30 die Kappen sind hell statt Schatten,
//      Bit 31 bei Art 2: die Linien liegen auf dem um LM_WEIT vergroesserten Quadrat (eine Kante
//      neben der Flaeche, deren Halbschatten hereinreicht); Bit 31 und 30 bei Art 3: ein
//      Sicht-Raster statt Strahlen, z = sein Platz im Vorrat der Seite (s. fein_sicht);
//   y: Emission wie im Licht-Raster (Bit 0-19); Bit 20-31 die unteren 3 Bit der Kanten-
//      abstaende (je Linie 6 Bit: Anfang, Ende);
//   z, w: je eine Linie -- Anfang s und Ende s (je 11 Bit, in 1/512), an beiden Enden wie weit
//      die Kante des Hindernisses, die den Schatten wirft, von der Quelle weg ist (je die
//      oberen 5 von 8 Bit, s. pack_kante, linie_kanten). Daraus rechnet der Licht-Pass die
//      Breite des Halbschattens je Pixel (s. lage_sicht).
// Eine Linie von A nach B schneidet eine Kappe ab: das Stueck rechts von ihr (der Rand von A
// nach B gegen den Uhrzeigersinn). Die Kappen sind Schatten, ausser Bit 30.
//
// Gesucht wird so: LM_RAND Punkte je Rand auf Sicht pruefen, darunter die Ecken (eine Linie,
// die eine Ecke abschneidet, trennt immer die Ecke selbst vom Rest), wo die Sicht wechselt per
// Halbierung genau suchen, und an 3x3 Punkten innen gegenpruefen. Was nicht passt (mehr als
// zwei Linien, eine Ecke des Schattens mitten auf der Flaeche, eine Schattenspitze, die den
// Rand nicht beruehrt, Buntglas), bekommt Art 3: der Licht-Pass prueft die Sicht dort je
// Pixel mit Strahlen. Etwa 100 Strahlen je Flaeche und Quelle statt 256 Texel mal 32.

// Der Rand liegt um LM_RAND_ABSTAND innerhalb der Flaeche (s. back_punkt): dort wird gemessen,
// und die Punkte dort liegen genau auf der Schattenlinie -- die Linie selbst reicht im
// Licht-Pass ueber die ganze Flaeche.
fn rand_punkt(s: f32) -> vec2<f32> {
    return rand_punkt_m(s, 0.0);
}

// Dasselbe auf dem Rand des um `m` Bloecke je Seite vergroesserten Quadrats (s. LM_WEIT).
fn rand_punkt_m(s: f32, m: f32) -> vec2<f32> {
    let t = fract(s);
    var q = vec2<f32>(0.0, 1.0 - t);
    switch u32(floor(s)) & 3u {
        case 0u: { q = vec2<f32>(t, 0.0); }
        case 1u: { q = vec2<f32>(1.0, t); }
        case 2u: { q = vec2<f32>(1.0 - t, 1.0); }
        default: {}
    }
    return mix(vec2<f32>(LM_RAND_ABSTAND - m), vec2<f32>(1.0 + m - LM_RAND_ABSTAND), q);
}

// Rahmen einer Ebene mit Linien: Bit 31 heisst, ihre Linien liegen auf dem vergroesserten
// Quadrat (s. LM_WEIT).
fn lage_rahmen(lage: vec4<u32>) -> f32 {
    return select(0.0, LM_WEIT, (lage.x >> 31u) != 0u);
}

// Abstand von `p` zur Linie A -> B, positiv links (ausserhalb der Kappe).
fn linien_abstand(a: vec2<f32>, b: vec2<f32>, p: vec2<f32>) -> f32 {
    let r = b - a;
    return (r.x * (p.y - a.y) - r.y * (p.x - a.x)) / max(length(r), 1e-4);
}

// Wie weit der Rand von `von` bis `bis` (gegen den Uhrzeigersinn) hoechstens von der Linie
// zwischen beiden abliegt: 0, wenn beide Enden am selben Rand liegen.
fn rand_dicke(von: f32, bis: f32, m: f32) -> f32 {
    let a = rand_punkt_m(von, m);
    let b = rand_punkt_m(bis, m);
    let laenge = (bis - von + 4.0) % 4.0;
    var dicke = 0.0;
    for (var c = 1u; c <= 4u; c++) {
        let ab = (f32(c) - von + 4.0) % 4.0;
        if (ab > 0.0 && ab < laenge) {
            dicke = max(dicke, abs(linien_abstand(a, b, rand_punkt_m(f32(c % 4u), m))));
        }
    }
    return dicke;
}

// Kappen, die duenner sind als das, gelten nicht: eine Schattengrenze genau auf der Blockkante
// laege sonst als Linie am Rand, im Licht-Pass ein Streifen halber Helligkeit entlang der Kante.
const LM_DUENN: f32 = 0.02;

// Der Halbschatten einer Kante reicht oft mehrere Bloecke ueber die Flaechen hinaus, die die
// Kante selbst kreuzt. Eine Flaeche darin speichert dieselbe Kante: gesucht auf dem Rand eines
// um so viele Bloecke je Seite vergroesserten Quadrats, in derselben Ebene. So rechnet jede
// Flaeche im Halbschatten mit derselben Linie, und der Verlauf geht glatt ueber die Blockkanten.
const LM_WEIT: f32 = 3.0;
// Abtastpunkte je Seite dieses Quadrats.
const LM_RAND_WEIT: u32 = 24u;

// Eine Flaeche beim Backen.
struct BackFlaeche {
    rel: vec3<i32>,
    n: vec3<f32>,
    ua: vec3<f32>,
    va: vec3<f32>,
    // Abstand der Flaechenebene von der Blockmitte entlang n (0,5 = am Blockrand; eine
    // Schneeschicht oben -0,375) und ob der Block in dieser Richtung nur eine Ebene hat.
    ebene: f32,
    eindeutig: bool,
};

// Wo die Flaeche `d` eines Blocks der Klasse `klasse` wirklich liegt (s. BackFlaeche.ebene):
// aus den Boxen seiner Form. Mehrere Ebenen in dieser Richtung (Treppe, Zaun) oder Ebenen-
// Modelle: nicht eindeutig.
fn lm_ebene(klasse: u32, d: u32) -> vec2<f32> {
    if (klasse <= 1u) {
        return vec2<f32>(0.5, 1.0);
    }
    let h = class_headers[klasse];
    if ((h.x & 255u) == MAT_PLANES) {
        return vec2<f32>(0.5, 0.0);
    }
    let count = (h.x >> 8u) & 15u;
    let start = h.x >> 12u;
    // Bit-Lage der Achse in einer Box: x 0, y 5, z 10 (Richtung d: y, y, z, z, x, x).
    let achse = select(select(0u, 10u, d / 2u == 1u), 5u, d / 2u == 0u);
    let plus = (d & 1u) == 1u;
    var weit = -1.0;
    var zweite = false;
    for (var i = 0u; i < count; i++) {
        let b = class_boxes[start + i];
        let lo = f32((b >> achse) & 31u) / 16.0;
        let hi = f32((b >> (15u + achse)) & 31u) / 16.0;
        let w = select(0.5 - lo, hi - 0.5, plus);
        if (weit > -1.0 && abs(w - weit) > 1e-3) {
            zweite = true;
        }
        weit = max(weit, w);
    }
    if (count == 0u) {
        return vec2<f32>(0.5, 1.0);
    }
    return vec2<f32>(weit, select(1.0, 0.0, zweite));
}

// Punkt der Flaeche bei `uv`, knapp davor. Vom Rand um LM_RAND_ABSTAND nach innen geholt: ein
// Strahl genau von der Kante zwischen zwei Bloecken schluepft sonst durch den Spalt zur Quelle.
fn back_punkt(fl: BackFlaeche, uv: vec2<f32>) -> vec3<f32> {
    return back_punkt_m(fl, uv, 0.0);
}

// Dasselbe im Quadrat, das um `m` Bloecke je Seite vergroessert ist (s. LM_WEIT): auch neben
// der Flaeche, in ihrer Ebene.
fn back_punkt_m(fl: BackFlaeche, uv: vec2<f32>, m: f32) -> vec3<f32> {
    let p = clamp(uv, vec2<f32>(LM_RAND_ABSTAND - m), vec2<f32>(1.0 + m - LM_RAND_ABSTAND));
    return vec3<f32>(fl.rel) + vec3<f32>(0.5) + fl.n * (fl.ebene + 0.01) + fl.ua * (p.x - 0.5) + fl.va * (p.y - 0.5);
}

// Harte Sicht von `uv` auf die Mitte der Quelle: 1 frei, 0 verdeckt (auch abgewandt),
// -1 teilweise (Buntglas, gefaerbt).
fn back_sicht(fl: BackFlaeche, lo: vec3<f32>, uv: vec2<f32>, m: f32) -> f32 {
    let o = back_punkt_m(fl, uv, m);
    let dir = normalize(lo + vec3<f32>(0.5) - o);
    if (dot(dir, fl.n) <= 0.0) {
        return 0.0;
    }
    var t_hit = -1.0;
    let d = ray_to_light(o, dir, lo, &t_hit);
    let mittel = (d.r + d.g + d.b) / 3.0;
    if (max(d.r, max(d.g, d.b)) - min(d.r, min(d.g, d.b)) > 0.2 || (mittel > 0.4 && mittel < 0.6)) {
        return -1.0;
    }
    return select(0.0, 1.0, mittel >= 0.5);
}

// Abstand bis zum ersten Hindernis auf dem Strahl von `uv` zur Mitte der Quelle, -1 keins.
fn back_treffer(fl: BackFlaeche, lo: vec3<f32>, uv: vec2<f32>, m: f32) -> f32 {
    let o = back_punkt_m(fl, uv, m);
    let zu = lo + vec3<f32>(0.5) - o;
    var t_hit = -1.0;
    let egal = ray_to_light(o, normalize(zu), lo, &t_hit);
    return t_hit;
}

// Wie weit die Kante, die die Schattenlinie bei `uv` wirft, von der Mitte der Quelle weg ist
// (0: unbekannt, hart). Gemessen mit Strahlen knapp auf der verdeckten Seite (`rechts`, in uv):
// sie treffen das Hindernis nicht genau an der Kante, sondern -- laeuft der Strahl fast parallel
// zu seiner Seite, wie durch einen Schlitz -- ein Stueck dahinter oder davor, um so mehr, je
// weiter er von der Linie weg ist. Darum an zwei Stellen gemessen und auf die Linie selbst
// zurueckgerechnet.
fn back_kante(fl: BackFlaeche, lo: vec3<f32>, uv: vec2<f32>, rechts: vec2<f32>, m: f32) -> f32 {
    let t1 = back_treffer(fl, lo, clamp(uv + rechts, vec2<f32>(0.001 - m), vec2<f32>(0.999 + m)), m);
    if (t1 < 0.0) {
        return 0.0;
    }
    let t2 = back_treffer(fl, lo, clamp(uv + 2.0 * rechts, vec2<f32>(0.001 - m), vec2<f32>(0.999 + m)), m);
    let d_licht = length(lo + vec3<f32>(0.5) - back_punkt_m(fl, uv, m));
    let t = select(t1, 2.0 * t1 - t2, t2 >= 0.0);
    return d_licht - clamp(t, 0.01, d_licht - 0.05);
}

// Wie viel Himmel die Flaeche bei `uv` sieht, 0..1: HIMMEL_STRAHLEN feste Strahlen ueber die
// Halbkugel, nach dem Kosinus verteilt (so viele, wie Licht aus der Richtung auf die Flaeche
// faellt). Was HIMMEL_WEITE Bloecke frei bleibt, zaehlt -- nach oben voll, nach unten so viel,
// wie der ferne Boden zurueckwirft (BODEN_RUECKWURF). Jede Flaeche nimmt dieselben Richtungen:
// zwei Flaechen sehen an derselben Stelle dasselbe, ohne Rauschen.
// Mehrere Threads teilen sich die Strahlen eines Punkts (s. cs_lm_layers): back_himmel_teil
// summiert jeden `teile`-ten ab `teil`.
fn back_himmel(fl: BackFlaeche, uv: vec2<f32>) -> f32 {
    return back_himmel_teil(fl, uv, 0u, 1u) / f32(HIMMEL_STRAHLEN);
}

fn back_himmel_teil(fl: BackFlaeche, uv: vec2<f32>, teil: u32, teile: u32) -> f32 {
    let o = back_punkt(fl, uv);
    var summe = 0.0;
    for (var k = teil; k < HIMMEL_STRAHLEN; k += teile) {
        let r = sqrt((f32(k) + 0.5) / f32(HIMMEL_STRAHLEN));
        let winkel = f32(k) * 2.39996;
        let dir = normalize(fl.ua * (r * cos(winkel)) + fl.va * (r * sin(winkel)) + fl.n * sqrt(max(1.0 - r * r, 0.0)));
        var egal = -1.0;
        let d = trace(o, dir, HIMMEL_WEITE, &egal);
        summe += dot(d.rgb, vec3<f32>(1.0 / 3.0)) * select(BODEN_RUECKWURF, 1.0, dir.y > 0.0);
    }
    return summe;
}

// HDR-Farbe in einem Wort: je Kanal 9 Bit Mantisse, 5 Bit gemeinsamer Exponent.
fn rgb9e5_packen(farbe: vec3<f32>) -> u32 {
    let c = clamp(farbe, vec3<f32>(0.0), vec3<f32>(65408.0));
    let m = max(c.r, max(c.g, c.b));
    var e = max(-16, i32(floor(log2(max(m, 1e-20))))) + 16;
    var teiler = exp2(f32(e - 24));
    if (floor(m / teiler + 0.5) >= 512.0) {
        e += 1;
        teiler *= 2.0;
    }
    let q = min(vec3<u32>(floor(c / teiler + 0.5)), vec3<u32>(511u));
    return q.r | q.g << 9u | q.b << 18u | u32(e) << 27u;
}

fn rgb9e5_lesen(w: u32) -> vec3<f32> {
    return vec3<f32>(vec3<u32>(w, w >> 9u, w >> 18u) & vec3<u32>(511u)) * exp2(f32(w >> 27u) - 24.0);
}

const STATE_INFO_WORTE: u32 = 16u;

fn state_wort(state: u32, k: u32) -> u32 {
    return textureLoad(state_info, vec2<i32>(i32((state % 256u) * STATE_INFO_WORTE + k), i32(state / 256u)), 0).r;
}

fn state_an(rel: vec3<i32>) -> u32 {
    let w = rel + params.cam_block.xyz;
    return textureLoad(voxel_states, vec3<i32>(w.x & 255, w.z & 255, w.y & 255), 0).r;
}

// Biom-Toenung `art` (0 keine, 1 Gras, 2 Laub, 3 trockenes Laub) auf `farbe`.
fn toenen(farbe: vec3<f32>, art: u32) -> vec3<f32> {
    switch (art) {
        case 1u: {
            return farbe * TOENUNG_GRAS;
        }
        case 2u: {
            return farbe * TOENUNG_LAUB;
        }
        case 3u: {
            return farbe * TOENUNG_TROCKEN;
        }
        default: {
            return farbe;
        }
    }
}

// Atlas-Stelle (0..1) an `uv` (0..1) auf Seite `d` des States.
fn atlas_stelle(state: u32, d: u32, uv: vec2<f32>) -> vec2<f32> {
    let ecke = state_wort(state, d);
    let groesse = state_wort(state, 6u + d);
    let e = vec2<f32>(f32(ecke & 65535u), f32(ecke >> 16u)) / 65535.0;
    let g = vec2<f32>(f32(groesse & 65535u), f32(groesse >> 16u)) / 65535.0;
    return e + g * uv;
}

// Farbe der Seite `d` (Reihenfolge wie lm_normale) des Blocks an `rel`: das Mittel ihrer
// Atlas-Kachel (deren kleinste Mip-Stufe), mal der Biom-Toenung.
fn block_farbe(rel: vec3<i32>, d: u32) -> vec3<f32> {
    let state = state_an(rel);
    let stufe = i32(textureNumLevels(atlas_tex)) - 1;
    let groesse = vec2<f32>(textureDimensions(atlas_tex, stufe));
    let uv = atlas_stelle(state, d, vec2<f32>(0.5));
    let farbe = textureLoad(atlas_tex, vec2<i32>(min(uv * groesse, groesse - vec2<f32>(1.0))), stufe).rgb;
    return toenen(farbe, (state_wort(state, 12u) >> (2u * d)) & 3u);
}

// Texel der Seite `d` des Blocks an `rel` an der Stelle `p` (Weltpunkt relativ zu cam_block
// auf der Seite), mit Biom-Toenung -- was eine Spiegelung dort zeigt.
fn block_texel(rel: vec3<i32>, d: u32, p: vec3<f32>) -> vec3<f32> {
    let state = state_an(rel);
    let w = p + vec3<f32>(params.cam_block.xyz & vec3<i32>(1023));
    // Seiten: oben in der Textur ist oben in der Welt.
    var uv = vec2<f32>(fract(dot(w, lm_achse_u(d))), fract(dot(w, lm_achse_v(d))));
    if (d >= 2u) {
        uv.y = 1.0 - uv.y;
    }
    let groesse = vec2<f32>(textureDimensions(atlas_tex, 0));
    let stelle = atlas_stelle(state, d, uv);
    let farbe = textureLoad(atlas_tex, vec2<i32>(min(stelle * groesse, groesse - vec2<f32>(1.0))), 0).rgb;
    return toenen(farbe, (state_wort(state, 12u) >> (2u * d)) & 3u);
}

// Material des Blocks an `rel` (s. world::material): x = Metall, y = Rauheit, z = Reflexion
// senkrecht (Nichtmetall), w = 1 glaenzt (sonst matt, keine Spiegelung).
fn material_an(rel: vec3<i32>) -> vec4<f32> {
    let m = state_wort(state_an(rel), 13u);
    return vec4<f32>(vec3<f32>(vec3<u32>(m, m >> 8u, m >> 16u) & vec3<u32>(255u)) / 255.0, f32((m >> 24u) & 1u));
}

// Direktes Blocklicht bei `o` (knapp vor einer Flaeche mit Normale `n` des Blocks `zelle`), in
// der Einheit von light_strength: eine der Quellen der Raster-Zelle, nach ihrer Helligkeit dort
// gewaehlt (`xi` 0..1), mit einem Strahl auf Punkt `s` ihrer Scheibe -- ueber viele Aufrufe
// gemittelt die Summe aller mit weichem Schatten. Ist der Block selbst eine Quelle: nichts,
// sein Leuchten ist schon direktes Licht.
fn rueck_direkt(o: vec3<f32>, n: vec3<f32>, zelle: vec3<i32>, xi: f32, s: u32) -> vec3<f32> {
    let anfang = light_cell(o);
    if (anfang < 0) {
        return vec3<f32>(0.0);
    }
    var summe = 0.0;
    for (var k = 0; k < LIGHTS_PER_CELL; k++) {
        let e = light_grid[anfang + k];
        if (e.y == 0u) {
            break;
        }
        let lo = light_block(e);
        if (all(lo == zelle)) {
            return vec3<f32>(0.0);
        }
        summe += dot(light_strength(e, vec3<f32>(lo), o, n, false), vec3<f32>(1.0 / 3.0));
    }
    if (summe <= 0.0) {
        return vec3<f32>(0.0);
    }
    var rest = xi * summe;
    var wahl = vec2<u32>(0u);
    var staerke = vec3<f32>(0.0);
    var anteil = 0.0;
    for (var k = 0; k < LIGHTS_PER_CELL; k++) {
        let e = light_grid[anfang + k];
        if (e.y == 0u) {
            break;
        }
        let st = light_strength(e, vec3<f32>(light_block(e)), o, n, false);
        let h = dot(st, vec3<f32>(1.0 / 3.0));
        if (h > 0.0) {
            wahl = e;
            staerke = st;
            anteil = h;
            rest -= h;
            if (rest <= 0.0) {
                break;
            }
        }
    }
    let lo = vec3<f32>(light_block(wahl));
    let mitte_dir = normalize(lo + vec3<f32>(0.5) - o);
    let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(mitte_dir.y) > 0.9);
    let u = normalize(cross(mitte_dir, hilf));
    let w = cross(mitte_dir, u);
    let r = sqrt(fract(f32(s) * 0.618034 + 0.31)) * params.block_soft.x;
    let winkel = f32(s) * 2.39996;
    var t_hit = -1.0;
    let durch = ray_to_light(o, normalize(mitte_dir + (u * cos(winkel) + w * sin(winkel)) * r), lo, &t_hit);
    return staerke * durch * (summe / anteil);
}

// Wo ein Strahl ab `o` in Richtung `dir` (bis RUECK_WEITE) auf einen Block trifft: xyz =
// Stelle knapp vor der getroffenen Seite, w = Seite (Reihenfolge wie lm_normale); w = -1
// nichts getroffen, oder nur Glas, Laub, Wasser auf dem Weg.
fn rueck_treffer(o: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    var t_hit = -1.0;
    let d = trace(o, dir, RUECK_WEITE, &t_hit);
    if (t_hit < 1e-3 || max(d.r, max(d.g, d.b)) > 0.02) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    // Getroffen beim Eintritt in die Zelle: die Seite ist die Achse, deren Zellgrenze dort liegt.
    let p = o + dir * t_hit;
    let ab = abs(p - round(p));
    var seite = 4u + u32(dir.x < 0.0);
    if (ab.y <= ab.x && ab.y <= ab.z) {
        seite = u32(dir.y < 0.0);
    } else if (ab.z <= ab.x) {
        seite = 2u + u32(dir.z < 0.0);
    }
    return vec4<f32>(p + vec3<f32>(lm_normale(seite)) * 0.01, f32(seite));
}

// Richtung ueber die Halbkugel um `n`, nach dem Kosinus verteilt: `a` 0..1 von der Mitte zum
// Rand (flaechentreu), `winkel` herum.
fn kosinus_richtung(n: vec3<f32>, a: f32, winkel: f32) -> vec3<f32> {
    let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let u = normalize(cross(n, hilf));
    let w = cross(n, u);
    let r = sqrt(a);
    return normalize(u * (r * cos(winkel)) + w * (r * sin(winkel)) + n * sqrt(max(1.0 - a, 0.0)));
}

// Zufallszahl 0..1 fuer Strahl `k`, je `salz` unabhaengig (PCG-Hash) -- fuer alle Punkte
// dieselbe Folge, damit zwei Flaechen an derselben Stelle dasselbe rechnen.
fn rueck_zufall(k: u32, salz: u32) -> f32 {
    var h = k * 747796405u + salz * 2891336453u + 1u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h = (h >> 22u) ^ h;
    return f32(h >> 8u) / 16777216.0;
}

// Aus `x` 0..1 eine Stelle -1..1, zur Mitte hin linear haeufiger (Zelt: wie der Licht-Pass
// zwischen den Punkten mischt).
fn rueck_zelt(x: f32) -> f32 {
    return select(1.0 - sqrt(2.0 - 2.0 * x), sqrt(2.0 * x) - 1.0, x < 0.5);
}

// Geht die Flaeche `fl` in Richtung (su, sv) (je -1, 0, 1 Bloecke entlang ua, va) in derselben
// Ebene weiter? Der Nachbar ist derselbe Block, davor Luft.
fn rueck_weiter(fl: BackFlaeche, su: i32, sv: i32) -> bool {
    let nb = fl.rel + vec3<i32>(fl.ua) * su + vec3<i32>(fl.va) * sv;
    return voxel_at(nb) == voxel_at(fl.rel) && voxel_at(nb + vec3<i32>(fl.n)) == 0u;
}

// Blocklicht, das die Flaeche bei `uv` von den Flaechen um sie herum zurueckgeworfen bekommt,
// in der Einheit von light_strength. RUECK_STRAHLEN Strahlen ueber die Halbkugel, nach dem
// Kosinus verteilt: trifft einer einen Block, kommt dessen direktes Licht dort mal seiner Farbe
// an (eine matte Flaeche wirft E * Albedo / pi je Raumwinkel zurueck; ueber die Halbkugel nach
// dem Kosinus gemittelt gibt das genau E * Albedo). Von dort geht ein zweiter Strahl weiter:
// das Licht, das zweimal zurueckgeworfen wurde -- so kommt es auch um Ecken, hinter die das
// direkte Licht nirgends faellt. Was danach noch zwischen den Flaechen hin und her geht, ist
// geschaetzt: je Weg bleibt im Mittel so viel, wie die Strahlen hier Bloecke treffen mal deren
// Farbe (geometrische Reihe auf den zweiten Rueckwurf). Eine Lampe wirft nichts zurueck (ihr
// Leuchten ist das direkte Licht).
// Der Wert gilt nicht fuer den Punkt allein, sondern fuer die Gegend, ueber die der Licht-Pass
// zwischen den Punkten mischt (ein Drittel Block je Seite, zur Mitte hin staerker): jeder
// Strahl startet woanders darin, auch auf den Nachbarflaechen derselben Ebene. Was auf keine
// Flaeche faellt (Wand, Kante), zaehlt nicht. Welche Startpunkte gelten, haengt so nur an der
// Stelle in der Welt: zwei Flaechen rechnen an ihrem gemeinsamen Rand genau dasselbe, ohne
// Sprung -- auch an der Ecke eines Blocks, wo das Licht ganz nah an der Wand steil anders ist
// (das darf nicht ein Drittel Block weit gelten).
// Mehrere Threads teilen sich die Strahlen eines Punkts (s. cs_lm_layers): jeder summiert
// jeden `teile`-ten ab `teil` (RueckTeil), rueck_fertig rechnet aus den Summen das Licht.
struct RueckTeil {
    erster: vec3<f32>,
    zweiter: vec3<f32>,
    bleibt: vec3<f32>,
    zahl: f32,
};

fn rueck_fertig(t: RueckTeil) -> vec3<f32> {
    let n_strahlen = max(t.zahl, 1.0);
    return (t.erster + t.zweiter / (vec3<f32>(1.0) - min(t.bleibt / n_strahlen, vec3<f32>(RUECK_GESCHLOSSEN)))) / n_strahlen;
}

fn back_rueckwurf_teil(fl: BackFlaeche, uv: vec2<f32>, teil: u32, teile: u32) -> RueckTeil {
    var erster = vec3<f32>(0.0);
    var zweiter = vec3<f32>(0.0);
    var bleibt = vec3<f32>(0.0);
    var zahl = 0.0;
    // Welche der 3x3 Flaechen um diese weitergehen (die Mitte immer). Eine schraeg daneben nur,
    // wenn auch eine der beiden geraden zwischen ihr und dieser weitergeht: stehen dort zwei
    // Bloecke Ecke an Ecke, liegt sie auf der anderen Seite (sonst kaeme Licht durch die Ecke).
    var weiter: array<bool, 9>;
    for (var w = 0; w < 9; w++) {
        weiter[w] = w == 4 || rueck_weiter(fl, w % 3 - 1, w / 3 - 1);
    }
    for (var w = 0; w < 9; w += 2) {
        if (w != 4) {
            weiter[w] = weiter[w] && (weiter[w - w % 3 + 1] || weiter[3 + w % 3]);
        }
    }
    for (var k = teil; k < RUECK_STRAHLEN; k += teile) {
        // Gegend und Richtung zusammen gleichmaessig verteilt (R4-Folge, Roberts 2018): nahe
        // einer Wand sieht jeder Startpunkt anders, unabhaengig gezogen rauscht das stark.
        let r4 = fract(vec4<f32>(0.5) + f32(k) * vec4<f32>(0.8566748838545029, 0.7338918566271260, 0.6287067210378086, 0.5385972572236101));
        let q = uv + vec2<f32>(rueck_zelt(r4.z), rueck_zelt(r4.w)) / 3.0;
        let zelle_q = clamp(vec2<i32>(floor(q)), vec2<i32>(-1), vec2<i32>(1));
        if (!weiter[(zelle_q.y + 1) * 3 + zelle_q.x + 1]) {
            continue;
        }
        // Wie back_punkt LM_RAND_ABSTAND vom Rand jeder Flaeche weg.
        let p = vec2<f32>(zelle_q) + clamp(q - vec2<f32>(zelle_q), vec2<f32>(LM_RAND_ABSTAND), vec2<f32>(1.0 - LM_RAND_ABSTAND));
        let o = vec3<f32>(fl.rel) + vec3<f32>(0.5) + fl.n * (fl.ebene + 0.01) + fl.ua * (p.x - 0.5) + fl.va * (p.y - 0.5);
        zahl += 1.0;
        let r = sqrt(r4.x);
        let winkel = r4.y * 6.2831853;
        let dir = normalize(fl.ua * (r * cos(winkel)) + fl.va * (r * sin(winkel)) + fl.n * sqrt(max(1.0 - r * r, 0.0)));
        let h = rueck_treffer(o, dir);
        if (h.w < 0.0) {
            continue;
        }
        let seite = u32(h.w);
        let n = vec3<f32>(lm_normale(seite));
        let zelle = vec3<i32>(floor(h.xyz - n * 0.02));
        let farbe = block_farbe(zelle, seite);
        bleibt += farbe;
        if (rueck_lampe(h.xyz, zelle)) {
            continue;
        }
        erster += farbe * rueck_direkt(h.xyz, n, zelle, rueck_zufall(k, 5u), k);
        let dir2 = kosinus_richtung(n, rueck_zufall(k, 6u), rueck_zufall(k, 7u) * 6.2831853);
        let h2 = rueck_treffer(h.xyz, dir2);
        if (h2.w < 0.0) {
            continue;
        }
        let seite2 = u32(h2.w);
        let n2 = vec3<f32>(lm_normale(seite2));
        let zelle2 = vec3<i32>(floor(h2.xyz - n2 * 0.02));
        zweiter += farbe * block_farbe(zelle2, seite2) * rueck_direkt(h2.xyz, n2, zelle2, rueck_zufall(k, 8u), k + 7u);
    }
    return RueckTeil(erster, zweiter, bleibt, zahl);
}

// Ist der Block `zelle` (Stelle `o` davor) eine Quelle aus dem Licht-Raster?
fn rueck_lampe(o: vec3<f32>, zelle: vec3<i32>) -> bool {
    let anfang = light_cell(o);
    if (anfang < 0) {
        return false;
    }
    for (var k = 0; k < LIGHTS_PER_CELL; k++) {
        let e = light_grid[anfang + k];
        if (e.y == 0u) {
            break;
        }
        if (all(light_block(e) == zelle)) {
            return true;
        }
    }
    return false;
}

// Ein Wert eines Rasters (Spalte i, Zeile j), 0..1 -- aus den gepackten Worten.
fn raster_wert(w: u32, i: u32) -> f32 {
    return f32((w >> (8u * (i & 3u))) & 255u) / 255.0;
}

// Bilinear zwischen den FEIN_N x FEIN_N Punkten eines feinen Rasters ab `basis` in lm.
fn fein_sicht(basis: u32, uv: vec2<f32>) -> f32 {
    let s = f32(FEIN_N - 1u);
    let x = clamp(uv * s, vec2<f32>(0.0), vec2<f32>(s));
    let i = min(floor(x), vec2<f32>(s - 1.0));
    let a = x - i;
    let c = u32(i.x);
    let r = u32(i.y);
    let zeile = FEIN_N / 4u;
    let w00 = atomicLoad(&lm[basis + r * zeile + c / 4u]);
    let w01 = atomicLoad(&lm[basis + r * zeile + (c + 1u) / 4u]);
    let w10 = atomicLoad(&lm[basis + (r + 1u) * zeile + c / 4u]);
    let w11 = atomicLoad(&lm[basis + (r + 1u) * zeile + (c + 1u) / 4u]);
    let unten = mix(raster_wert(w00, c), raster_wert(w01, c + 1u), a.x);
    let oben = mix(raster_wert(w10, c), raster_wert(w11, c + 1u), a.x);
    return mix(unten, oben, a.y);
}

// Weiche Sicht einer Raster-Ebene (Art 3, Bit 31 und 30; z = Platz in der Seite) der Flaeche
// Nummer `flaeche`.
fn raster_ebene_sicht(lage: vec4<u32>, flaeche: u32, uv: vec2<f32>) -> f32 {
    return fein_sicht(LM_FEIN_BASE + ((flaeche / LM_FACES) * LM_FEIN_PLAETZE + lage.z) * FEIN_WORTE, uv);
}

// Marke in z einer Ebene, die ein Raster braucht (s. cs_lm_layers): die Quelle ist nur von
// einem Teil der Flaeche zu sehen.
const LM_RASTER_NOETIG: u32 = 0xFFFFFFFFu;

// Prueft an `o`, ob die Scheibe der Quelle `lo` ganz, gar nicht oder zum Teil zu sehen ist (s.
// cs_lm_layers): PRUEF_STRAHLEN Strahlen ueber die ganze Scheibe, je Punkt um `drehung` (0..1)
// gedreht -- ein sichtbares Stueck, das keiner trifft, ist kleiner als etwa 1 / PRUEF_STRAHLEN
// der Scheibe. Vier Strahlen verfehlten bis ein Viertel, acht auf dem Rand schmale Streifen
// durch einen Spalt. Bits wie lm_klasse: 1 frei, 2 verdeckt, 4 zum Teil, 8 gefaerbt.
const PRUEF_STRAHLEN: i32 = 32;

fn pruef_punkt(o: vec3<f32>, lo: vec3<f32>, drehung: f32) -> u32 {
    let mitte_dir = normalize(lo + vec3<f32>(0.5) - o);
    let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(mitte_dir.y) > 0.9);
    let u = normalize(cross(mitte_dir, hilf));
    let w = cross(mitte_dir, u);
    var bits = 0u;
    for (var s = 0; s < PRUEF_STRAHLEN; s++) {
        let r = sqrt((f32(s) + 0.5) / f32(PRUEF_STRAHLEN)) * params.block_soft.x;
        let winkel = f32(s) * 2.39996 + drehung * 6.2831853;
        var t_hit = -1.0;
        let d = ray_to_light(o, normalize(mitte_dir + (u * cos(winkel) + w * sin(winkel)) * r), lo, &t_hit);
        let v = dot(d, vec3<f32>(1.0 / 3.0));
        if (max(d.r, max(d.g, d.b)) - min(d.r, min(d.g, d.b)) > 0.05) {
            bits |= 8u;
        }
        bits |= select(select(4u, 2u, v < 0.001), 1u, v > 0.999);
        // Beides gesehen: zum Teil, mehr muss nicht geprueft werden.
        if ((bits & 3u) == 3u) {
            break;
        }
    }
    return bits;
}

// Block der Quelle Nummer `k` in der Raster-Zelle der Flaeche, w = 1, wenn sie geprueft werden
// muss (s. cs_lm_layers: sie leuchtet die Flaeche an, ist nicht ihr eigener Block, die Flaeche
// hat nur eine Ebene).
fn back_quelle(fl: BackFlaeche, k: u32) -> vec4<f32> {
    let mitte = back_punkt(fl, vec2<f32>(0.5));
    let basis = light_cell(mitte);
    if (basis < 0 || !fl.eindeutig) {
        return vec4<f32>(0.0);
    }
    let e = light_grid[basis + i32(k)];
    if (e.y == 0u) {
        return vec4<f32>(0.0);
    }
    let lb = light_block(e);
    let zu = vec3<f32>(lb) + vec3<f32>(0.5) - mitte;
    if (length(zu) >= light_range(e) + 1.0 || dot(zu, fl.n) <= -0.6 || all(lb == fl.rel) || any(vec3<u32>(lb - fl.rel + vec3<i32>(256)) > vec3<u32>(511u))) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(vec3<f32>(lb), 1.0);
}

// Die Ebene der Quelle Nummer `k` in der Raster-Zelle der Flaeche (Art 0: keine).
fn back_lage(fl: BackFlaeche, k: u32) -> vec4<u32> {
    let mitte = back_punkt(fl, vec2<f32>(0.5));
    let basis = light_cell(mitte);
    if (basis < 0) {
        return vec4<u32>(0u);
    }
    let e = light_grid[basis + i32(k)];
    if (e.y == 0u) {
        return vec4<u32>(0u);
    }
    let lb = light_block(e);
    let lo = vec3<f32>(lb);
    let zu = lo + vec3<f32>(0.5) - mitte;
    if (length(zu) >= light_range(e) + 1.0 || dot(zu, fl.n) <= -0.6) {
        return vec4<u32>(0u);
    }
    let versatz = vec3<u32>(lb - fl.rel + vec3<i32>(256));
    if (any(versatz > vec3<u32>(511u))) {
        return vec4<u32>(0u);
    }
    let kopf = versatz.x | versatz.y << 9u | versatz.z << 18u;
    if (all(lb == fl.rel)) {
        return vec4<u32>(kopf | 1u << 27u, e.y, 0u, 0u);
    }
    let strahlen = vec4<u32>(kopf | 3u << 27u, e.y, 0u, 0u);
    // Mehrere Ebenen (Treppe): Linien gaelten nur fuer eine -- je Pixel pruefen.
    if (!fl.eindeutig) {
        return strahlen;
    }
    // Harte Linien auf der Flaeche; laesst es sich so nicht beschreiben (mehr als zwei Linien,
    // eine Schattenecke mitten auf der Flaeche, Buntglas), Strahlen je Pixel -- ohne Weichheit
    // ist das ein Strahl, ohne Rauschen.
    var weich: array<f32, 9>;
    let eng = back_linien(fl, lo, kopf, e.y, 0.0, weich, false);
    if (((eng.x >> 27u) & 3u) == 3u) {
        return strahlen;
    }
    return eng;
}


// Marke fuer back_linien: die Linien stimmen hart, aber nicht mit der weichen Sicht.
const LM_WEICH_FALSCH: u32 = 0xFFFFFFFEu;

// Linien einer Quelle `lo` fuer die Flaeche: harte Schattengrenzen auf dem Rand des Quadrats um
// die Flaeche (`m` Bloecke je Seite groesser, 0 = die Flaeche selbst), an Punkten innen
// gegengeprueft, mit `weich` (3x3 weiche Sicht, falls `weich_an`) verglichen. Rueckgabe: die
// Ebene (frei, keine, Linien), Strahlen, wenn es sich hart nicht beschreiben laesst, oder
// LM_WEICH_FALSCH.
fn back_linien(fl: BackFlaeche, lo: vec3<f32>, kopf: u32, emission: u32, m: f32, weich_in: array<f32, 9>, weich_an: bool) -> vec4<u32> {
    var weich = weich_in;
    let strahlen = vec4<u32>(kopf | 3u << 27u, emission, 0u, 0u);
    // Buntglas auf dem Weg: nur Strahlen kennen die Farbe (w = 1 merkt das fuer back_lage).
    let farbig = vec4<u32>(kopf | 3u << 27u, emission, 0u, 1u);
    let falsch = vec4<u32>(LM_WEICH_FALSCH, emission, 0u, 0u);
    let rahmen = select(0u, 1u << 31u, m > 0.0);
    let je_seite = select(LM_RAND, LM_RAND_WEIT, m > 0.0);

    // Rand abtasten: wo wechselt die Sicht?
    let schritt = 1.0 / f32(je_seite);
    let anzahl = 4u * je_seite;
    var sicht: array<bool, 96>;
    for (var r = 0u; r < anzahl; r++) {
        let v = back_sicht(fl, lo, rand_punkt_m(f32(r) * schritt, m), m);
        if (v < 0.0) {
            return farbig;
        }
        sicht[r] = v > 0.5;
    }
    // Wechsel: Rand-Stelle (genau gesucht) und ob dort der Schatten beginnt.
    var wechsel_s: array<f32, 4>;
    var wechsel_rein: array<bool, 4>;
    var wechsel = 0u;
    for (var r = 0u; r < anzahl; r++) {
        let naechster = (r + 1u) % anzahl;
        if (sicht[r] == sicht[naechster]) {
            continue;
        }
        if (wechsel >= 4u) {
            return strahlen;
        }
        var a = f32(r) * schritt;
        var b = a + schritt;
        for (var h = 0; h < 10; h++) {
            let mitte_s = 0.5 * (a + b);
            let v = back_sicht(fl, lo, rand_punkt_m(mitte_s, m), m);
            if (v < 0.0) {
                return farbig;
            }
            if ((v > 0.5) == sicht[r]) {
                a = mitte_s;
            } else {
                b = mitte_s;
            }
        }
        wechsel_s[wechsel] = (0.5 * (a + b)) % 4.0;
        wechsel_rein[wechsel] = sicht[r];
        wechsel += 1u;
    }
    if (wechsel == 0u) {
        // Ueberall am Rand gleich: innen gegenpruefen.
        for (var q = 0; q < 9; q++) {
            let uv = vec2<f32>(f32(q % 3) + 1.0, f32(q / 3) + 1.0) * 0.25;
            if ((back_sicht(fl, lo, uv, 0.0) > 0.5) != sicht[0]) {
                return strahlen;
            }
        }
        // Kein harter Rand -- aber der Halbschatten eines Randes weiter weg kann hereinreichen
        // (weich gegengeprueft), sonst hoerte der weiche Verlauf an der Blockkante auf.
        if (weich_an && (sicht[0] || dot(lo + vec3<f32>(0.5) - back_punkt(fl, vec2<f32>(0.5)), fl.n) > 0.0)) {
            for (var q = 0; q < 9; q++) {
                if (select(weich[q] > 0.005, weich[q] < 0.995, sicht[0])) {
                    return falsch;
                }
            }
        }
        if (sicht[0]) {
            return vec4<u32>(kopf | 1u << 27u, emission, 0u, 0u);
        }
        return vec4<u32>(0u);
    }
    // Linien: je Kappe von ihrem Anfang zum naechsten Wechsel. Bei zwei Linien zwei Lesarten
    // (Schatten- oder Lichtkappen) -- es gilt die, die innen stimmt.
    for (var lesart = 0u; lesart < 2u; lesart++) {
        if (lesart == 1u && wechsel == 2u) {
            break;
        }
        var kappen = array<vec2<f32>, 2>();
        var zahl = 0u;
        // Die Kappe nimmt fast die ganze Flaeche ein (nur eine Linie, auf der anderen Seite ein
        // duenner Streifen).
        var ganz = false;
        for (var w = 0u; w < wechsel; w++) {
            // Lesart 0: Kappen beginnen, wo der Schatten beginnt; Lesart 1: wo das Licht beginnt.
            if (wechsel_rein[w] == (lesart == 0u)) {
                let kappe = vec2<f32>(wechsel_s[w], wechsel_s[(w + 1u) % wechsel]);
                if (rand_dicke(kappe.x, kappe.y, m) < LM_DUENN) {
                    continue;
                }
                if (wechsel == 2u && rand_dicke(kappe.y, kappe.x, m) < LM_DUENN) {
                    ganz = true;
                    continue;
                }
                kappen[zahl] = kappe;
                zahl += 1u;
            }
        }
        // Zwei Linien, die fast aufeinander liegen (zwei Schatten treffen sich in einer Kante,
        // dahinter genau diagonal): der Streifen dazwischen ist in Wahrheit keiner -- in dieser
        // Lesart bliebe eine halb helle Naht. Die andere Lesart (kleine Kappen an den Ecken)
        // stimmt dann.
        if (zahl == 2u) {
            let a0 = rand_punkt_m(kappen[0].x, m);
            let b0 = rand_punkt_m(kappen[0].y, m);
            let a1 = rand_punkt_m(kappen[1].x, m);
            let b1 = rand_punkt_m(kappen[1].y, m);
            let mp = 0.25 * (a0 + b0 + a1 + b1);
            if (abs(linien_abstand(a0, b0, mp)) < 0.02 && abs(linien_abstand(a1, b1, mp)) < 0.02) {
                continue;
            }
        }
        var passt = true;
        for (var q = 0; q < 9; q++) {
            let uv = vec2<f32>(f32(q % 3) + 1.0, f32(q / 3) + 1.0) * 0.25;
            var in_kappe = ganz;
            var knapp = false;
            for (var z = 0u; z < zahl; z++) {
                let abstand = linien_abstand(rand_punkt_m(kappen[z].x, m), rand_punkt_m(kappen[z].y, m), uv);
                in_kappe = in_kappe || abstand < 0.0;
                knapp = knapp || abs(abstand) < 0.02;
            }
            // Direkt auf einer Linie entscheidet die Rundung -- dieser Punkt prueft nichts.
            if (knapp) {
                continue;
            }
            let frei = select(!in_kappe, in_kappe, lesart == 1u);
            let v = back_sicht(fl, lo, uv, 0.0);
            if (v < 0.0 || (v > 0.5) != frei) {
                passt = false;
                break;
            }
        }
        if (!passt) {
            continue;
        }
        // Keine Kappe blieb: ueberall gleich -- Lesart 0 frei (oder ganz Schatten), Lesart 1
        // ganz Schatten.
        if (zahl == 0u) {
            // Eine Flaeche ganz in einer Kappe: dieselbe Pruefung wie ohne Rand (weich).
            if (weich_an) {
                for (var q = 0; q < 9; q++) {
                    if (select(weich[q] > 0.005, weich[q] < 0.995, lesart == 0u && !ganz)) {
                        return falsch;
                    }
                }
            }
            if (lesart == 0u && !ganz) {
                return vec4<u32>(kopf | 1u << 27u, emission, 0u, 0u);
            }
            return vec4<u32>(0u);
        }
        // Wie weit die Kante, die eine Linie wirft, von der Quelle weg ist: an beiden Enden
        // gemessen, genau dort, wo die Linie den Rand kreuzt -- an derselben Stelle wie die
        // Flaeche nebenan. Entlang der Linie aendert er sich kaum (anders als die Breite des
        // Halbschattens, die mit dem Abstand immer schneller waechst).
        var kanten: array<vec2<f32>, 2>;
        for (var z = 0u; z < zahl; z++) {
            let a = rand_punkt_m(kappen[z].x, m);
            let b = rand_punkt_m(kappen[z].y, m);
            let rechts = normalize(vec2<f32>(b.y - a.y, a.x - b.x)) * select(0.02, -0.02, lesart == 1u);
            kanten[z] = vec2<f32>(back_kante(fl, lo, a, rechts, m), back_kante(fl, lo, b, rechts, m));
        }
        let kopf_linien = kopf | 2u << 27u | select(0u, 1u << 29u, zahl == 2u) | lesart << 30u | rahmen;
        let lage = linien_lage(kopf_linien, emission, kappen, kanten, zahl, 1.0);
        // Weich gegenpruefen (s. back_lage): stimmt der Halbschatten der Linien nicht, kommt
        // das Kanten-Modell oder ein Raster dran.
        if (weich_an) {
            for (var q = 0; q < 9; q++) {
                let uv = vec2<f32>(f32(q % 3), f32(q / 3)) * 0.5;
                let d_licht = length(lo + vec3<f32>(0.5) - back_punkt(fl, uv));
                if (abs(weich[q] - lage_sicht(lage, uv, 0.002, d_licht)) > 0.015) {
                    return falsch;
                }
            }
        }
        return lage;
    }
    return strahlen;
}

// Die Ebene der Quelle Nummer `k` in der Raster-Zelle der Flaeche (Art 0: keine), nach der
// Pruefung `klasse` (s. lm_klasse): sieht die Flaeche sie ganz, frei; gar nicht, keine Ebene;
// zum Teil, ein Sicht-Raster (z = LM_RASTER_NOETIG, Platz vergibt cs_lm_layers). Buntglas auf
// dem Weg und Bloecke mit mehreren Ebenen in dieser Richtung (Treppe): Strahlen je Pixel. Ohne
// Weichheit harte Linien (s. back_lage).
fn strahl_lage(fl: BackFlaeche, k: u32, klasse: u32, weich_an: bool) -> vec4<u32> {
    if (!weich_an) {
        return back_lage(fl, k);
    }
    let mitte = back_punkt(fl, vec2<f32>(0.5));
    let basis = light_cell(mitte);
    if (basis < 0) {
        return vec4<u32>(0u);
    }
    let e = light_grid[basis + i32(k)];
    if (e.y == 0u) {
        return vec4<u32>(0u);
    }
    let lb = light_block(e);
    let zu = vec3<f32>(lb) + vec3<f32>(0.5) - mitte;
    if (length(zu) >= light_range(e) + 1.0 || dot(zu, fl.n) <= -0.6) {
        return vec4<u32>(0u);
    }
    let versatz = vec3<u32>(lb - fl.rel + vec3<i32>(256));
    if (any(versatz > vec3<u32>(511u))) {
        return vec4<u32>(0u);
    }
    let kopf = versatz.x | versatz.y << 9u | versatz.z << 18u;
    if (all(lb == fl.rel)) {
        return vec4<u32>(kopf | 1u << 27u, e.y, 0u, 0u);
    }
    if (!fl.eindeutig || (klasse & 8u) != 0u) {
        return vec4<u32>(kopf | 3u << 27u, e.y, 0u, 0u);
    }
    if ((klasse & 6u) == 0u) {
        return vec4<u32>(kopf | 1u << 27u, e.y, 0u, 0u);
    }
    if ((klasse & 5u) == 0u) {
        return vec4<u32>(0u);
    }
    return vec4<u32>(kopf | 3u << 27u | 1u << 31u, e.y, LM_RASTER_NOETIG, 0u);
}

var<workgroup> lm_neu: array<vec4<u32>, 16>;
// Je Quelle, was die Pruefung gesehen hat (s. cs_lm_layers): Bit 0 frei, 1 verdeckt, 2 zum
// Teil, 3 gefaerbt (Buntglas).
var<workgroup> lm_klasse: array<atomic<u32>, 16>;
// Vier Zeilen eines Sicht-Rasters beim gemeinsamen Rechnen (s. cs_lm_fein).
var<workgroup> lm_fein: array<atomic<u32>, 16>;
var<workgroup> lm_weiter: u32;
var<workgroup> lm_himmel: array<f32, 64>;
// Teilsummen des Rueckwurfs je Thread (s. back_rueckwurf_teil): erster + Zahl, zweiter, bleibt.
var<workgroup> lm_rueck: array<array<vec4<f32>, 3>, 64>;

// Dritter Back-Schritt, je Flaeche eine Gruppe, je Quelle ihrer Raster-Zelle ein Thread:
// die Ebene dieser Quelle, und je Thread ein Punkt der Himmelssicht (ausser bake.anzahl.y sagt
// fuer diesen Auftrag: nur Licht). Danach schreibt Thread 0 die Ebenen der Reihe nach -- feste
// Reihenfolge, dieselbe Welt ergibt dieselben Ebenen.
// Die Flaeche auf Platz `platz` der Seite `seite` eines Back-Auftrags (nach cs_lm_assign).
fn back_flaeche(job: vec4<i32>, seite: u32, platz: u32) -> BackFlaeche {
    let flaeche = atomicLoad(&lm[LM_BAKE_BASE + LM_PAGES + seite * LM_FACES + platz]);
    let i = flaeche / 6u;
    let d = flaeche % 6u;
    var fl: BackFlaeche;
    fl.rel = job.xyz * 16 + vec3<i32>(i32(i % 16u), i32(i / 256u), i32((i / 16u) % 16u)) - params.cam_block.xyz;
    fl.n = vec3<f32>(lm_normale(d));
    fl.ua = lm_achse_u(d);
    fl.va = lm_achse_v(d);
    let ebene = lm_ebene(voxel_at(fl.rel), d);
    fl.ebene = ebene.x;
    fl.eindeutig = ebene.y > 0.5;
    return fl;
}

// Sicht auf die Quelle `lo` fuer den Rasterpunkt `uv`, gemittelt ueber seine Gegend: so weit,
// wie der Licht-Pass zwischen den Punkten mischt (`weite` je Seite, zur Mitte hin staerker),
// FEIN_STRAHLEN Strahlen, jeder von woanders darin und auf eine andere Stelle der Scheibe
// (R4-Folge, wie beim Rueckwurf). Ein Punkt allein misst nur seine Stelle: ein Schatten, der
// schmaler ist als der Abstand der Punkte (die Stange eines Eisengitters, 2/16 Block), fiel mal
// auf einen, mal dazwischen -- im Bild eine Zackenkette statt eines Bands. Startpunkte
// auf den Nachbarflaechen gelten, wenn die Flaeche dort in derselben Ebene weitergeht (s.
// rueck_weiter): zwei Flaechen messen an ihrem gemeinsamen Rand dieselbe Gegend.
// Welche der 3x3 Flaechen um `fl` in derselben Ebene weitergehen, Bit w = (sv + 1) * 3 + su + 1
// (die Mitte immer); eine schraeg daneben nur, wenn auch eine gerade dazwischen weitergeht (wie
// beim Rueckwurf).
fn weiter_maske(fl: BackFlaeche) -> u32 {
    var weiter = 1u << 4u;
    for (var w = 0; w < 9; w++) {
        if (w != 4 && rueck_weiter(fl, w % 3 - 1, w / 3 - 1)) {
            weiter |= 1u << u32(w);
        }
    }
    for (var w = 0; w < 9; w += 2) {
        let gerade = (weiter >> u32(w - w % 3 + 1)) & 1u | (weiter >> u32(3 + w % 3)) & 1u;
        if (w != 4 && gerade == 0u) {
            weiter &= ~(1u << u32(w));
        }
    }
    return weiter;
}

// Stelle `q` (uv, darf bis eine Flaeche ueber den Rand reichen) in der Ebene der Flaeche, knapp
// davor; w = 0, wenn die Flaeche dort nicht weitergeht (`weiter` s. weiter_maske).
fn gegend_ort(fl: BackFlaeche, q: vec2<f32>, weiter: u32) -> vec4<f32> {
    let zelle_q = clamp(vec2<i32>(floor(q)), vec2<i32>(-1), vec2<i32>(1));
    if (((weiter >> u32((zelle_q.y + 1) * 3 + zelle_q.x + 1)) & 1u) == 0u) {
        return vec4<f32>(0.0);
    }
    let p = vec2<f32>(zelle_q) + clamp(q - vec2<f32>(zelle_q), vec2<f32>(LM_RAND_ABSTAND), vec2<f32>(1.0 - LM_RAND_ABSTAND));
    return vec4<f32>(vec3<f32>(fl.rel) + vec3<f32>(0.5) + fl.n * (fl.ebene + 0.01) + fl.ua * (p.x - 0.5) + fl.va * (p.y - 0.5), 1.0);
}

// `weiter`: s. weiter_maske (in cs_lm_fein einmal je Gruppe).
fn sicht_gegend(fl: BackFlaeche, lo: vec3<f32>, uv: vec2<f32>, weite: f32, weiter: u32) -> f32 {
    var summe = 0.0;
    var zahl = 0.0;
    for (var k = 0; k < FEIN_STRAHLEN; k++) {
        let r4 = fract(vec4<f32>(0.5) + f32(k) * vec4<f32>(0.8566748838545029, 0.7338918566271260, 0.6287067210378086, 0.5385972572236101));
        let ort = gegend_ort(fl, uv + vec2<f32>(rueck_zelt(r4.z), rueck_zelt(r4.w)) * weite, weiter);
        if (ort.w < 0.5) {
            continue;
        }
        let o = ort.xyz;
        let mitte_dir = normalize(lo + vec3<f32>(0.5) - o);
        let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(mitte_dir.y) > 0.9);
        let u = normalize(cross(mitte_dir, hilf));
        let w = cross(mitte_dir, u);
        let r = sqrt(r4.x) * params.block_soft.x;
        let winkel = r4.y * 6.2831853;
        var t_hit = -1.0;
        summe += dot(ray_to_light(o, normalize(mitte_dir + (u * cos(winkel) + w * sin(winkel)) * r), lo, &t_hit), vec3<f32>(1.0 / 3.0));
        zahl += 1.0;
    }
    return summe / max(zahl, 1.0);
}

// Vierter Back-Schritt: die Sicht-Raster, die cs_lm_layers in diesem Schritt vergeben hat --
// je vier Zeilen eine Gruppe (gruppe.x), je Raster der Seite gruppe.y, je Auftrag gruppe.z; je
// Punkt ein Thread (s. sicht_gegend).
@compute @workgroup_size(64)
fn cs_lm_fein(@builtin(workgroup_id) gruppe: vec3<u32>, @builtin(local_invocation_id) lokal: vec3<u32>) {
    if (gruppe.z >= bake.anzahl.x) {
        return;
    }
    let job = bake.jobs[gruppe.z];
    let seite = u32(job.w) & 0xFFFFu;
    let slot = gruppe.y;
    if (slot >= min(atomicLoad(&lm[LM_FEIN_ZAEHLER_BASE + seite]), LM_FEIN_PLAETZE)) {
        return;
    }
    let kopf = LM_FEIN_KOPF_BASE + (seite * LM_FEIN_PLAETZE + slot) * 2u;
    let k0 = atomicLoad(&lm[kopf]);
    if ((k0 >> 16u) != (u32(job.w) >> 16u)) {
        return;
    }
    let fl = back_flaeche(job, seite, k0 & 0xFFFFu);
    let q = atomicLoad(&lm[kopf + 1u]);
    let lo = vec3<f32>(fl.rel + vec3<i32>(vec3<u32>(q & 511u, (q >> 9u) & 511u, (q >> 18u) & 511u)) - vec3<i32>(256));
    let i = lokal.x % FEIN_N;
    let zeile = lokal.x / FEIN_N;
    let j = gruppe.x * 4u + zeile;
    if (lokal.x < 16u) {
        atomicStore(&lm_fein[lokal.x], 0u);
    }
    // Welche der 3x3 Flaechen um diese in derselben Ebene weitergehen (die Mitte immer); eine
    // schraeg daneben nur, wenn auch eine gerade dazwischen weitergeht (wie beim Rueckwurf).
    if (lokal.x == 0u) {
        lm_weiter = weiter_maske(fl);
    }
    workgroupBarrier();
    let sn = f32(FEIN_N - 1u);
    let v = sicht_gegend(fl, lo, vec2<f32>(f32(i), f32(j)) / sn, 1.0 / sn, lm_weiter);
    atomicOr(&lm_fein[zeile * (FEIN_N / 4u) + i / 4u], u32(round(clamp(v, 0.0, 1.0) * 255.0)) << (8u * (i & 3u)));
    workgroupBarrier();
    if (lokal.x < 16u) {
        atomicStore(&lm[LM_FEIN_BASE + (seite * LM_FEIN_PLAETZE + slot) * FEIN_WORTE + gruppe.x * 16u + lokal.x], atomicLoad(&lm_fein[lokal.x]));
    }
}

@compute @workgroup_size(64)
fn cs_lm_layers(@builtin(workgroup_id) gruppe: vec3<u32>, @builtin(local_invocation_id) lokal: vec3<u32>) {
    if (gruppe.y >= bake.anzahl.x) {
        return;
    }
    let job = bake.jobs[gruppe.y];
    let seite = u32(job.w) & 0xFFFFu;
    let platz = gruppe.x + (u32(job.w) >> 16u) * LM_SCHRITT_FLAECHEN;
    if (platz >= min(atomicLoad(&lm[LM_BAKE_BASE + seite]), LM_FACES)) {
        return;
    }
    let fl = back_flaeche(job, seite, platz);
    let aussen = light_cell(back_punkt(fl, vec2<f32>(0.5))) < 0;
    // Welche Quellen die Flaeche ganz, gar nicht oder nur zum Teil sieht: an den FEIN_N x FEIN_N
    // Punkten ihres Rasters und eine Reihe darueber hinaus auf den Nachbarflaechen derselben Ebene
    // (s. pruef_punkt; nur mit Weichheit, ohne gibt es harte Linien, s. back_lage). So weit
    // mittelt das Raster (s. sicht_gegend): pruefte sie nur die Flaeche selbst, galt eine als
    // dunkel, deren Nachbar am gemeinsamen Rand noch etwas Licht hat -- eine Stufe je Block, wo
    // der Schatten ganz dunkel wird.
    if (lokal.x < 16u) {
        atomicStore(&lm_klasse[lokal.x], 0u);
    }
    if (lokal.x == 0u) {
        lm_weiter = weiter_maske(fl);
    }
    workgroupBarrier();
    let weich_an = lm_strahlen() > 1;
    if (weich_an) {
        let sn = f32(FEIN_N - 1u);
        let n = FEIN_N + 2u;
        for (var k = 0u; k < 16u; k++) {
            let quelle = back_quelle(fl, k);
            if (quelle.w < 0.5) {
                continue;
            }
            var bits = 0u;
            for (var t = lokal.x; t < n * n; t += 64u) {
                let uv = (vec2<f32>(f32(t % n), f32(t / n)) - vec2<f32>(1.0)) / sn;
                let ort = gegend_ort(fl, uv, lm_weiter);
                if (ort.w > 0.5) {
                    bits |= pruef_punkt(ort.xyz, quelle.xyz, fract(f32(t) * 0.618034));
                }
            }
            atomicOr(&lm_klasse[k], bits);
        }
    }
    workgroupBarrier();
    if (lokal.x < 16u) {
        lm_neu[lokal.x] = strahl_lage(fl, lokal.x, atomicLoad(&lm_klasse[lokal.x]), weich_an);
    }
    workgroupBarrier();
    // Wer ein Raster braucht, bekommt einen Platz im Vorrat der Seite (Thread 0, die Quellen der
    // Reihe nach); gerechnet wird es in cs_lm_fein, ueber die ganze GPU verteilt. Ist der Vorrat
    // leer: Strahlen je Pixel.
    if (lokal.x == 0u) {
        for (var k = 0u; k < 16u; k++) {
            var lage = lm_neu[k];
            if (((lage.x >> 27u) & 3u) != 3u || lage.z != LM_RASTER_NOETIG) {
                continue;
            }
            let fein = atomicAdd(&lm[LM_FEIN_ZAEHLER_BASE + seite], 1u);
            if (fein < LM_FEIN_PLAETZE) {
                let kopf = LM_FEIN_KOPF_BASE + (seite * LM_FEIN_PLAETZE + fein) * 2u;
                atomicStore(&lm[kopf], platz | (u32(job.w) >> 16u) << 16u);
                atomicStore(&lm[kopf + 1u], lage.x & 0x7FFFFFFu);
                lage.x |= 3u << 30u;
                lage.z = fein;
            } else {
                lage.x &= ~(3u << 30u);
                lage.z = 0u;
            }
            lm_neu[k] = lage;
        }
    }
    workgroupBarrier();
    // Je Punkt des 4x4-Rasters vier Threads, jeder ein Viertel der Strahlen (Punkt = lokal % 16).
    let punkt = lokal.x % 16u;
    let teil = lokal.x / 16u;
    let uv_punkt = vec2<f32>(f32(punkt % 4u), f32(punkt / 4u)) / 3.0;
    // Die Himmelssicht haengt nur an den Bloecken: nach einer Aenderung nur des Lichts bleibt
    // sie.
    let mit_himmel = ((bake.anzahl.y >> gruppe.y) & 1u) == 0u;
    if (mit_himmel) {
        lm_himmel[lokal.x] = back_himmel_teil(fl, uv_punkt, teil, 4u);
    }
    // Zurueckgeworfenes Licht haengt am Licht: immer neu. Ausserhalb des Licht-Rasters kennt es
    // keine Quellen.
    var r = RueckTeil(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    if (!aussen) {
        r = back_rueckwurf_teil(fl, uv_punkt, teil, 4u);
    }
    lm_rueck[lokal.x][0] = vec4<f32>(r.erster, r.zahl);
    lm_rueck[lokal.x][1] = vec4<f32>(r.zweiter, 0.0);
    lm_rueck[lokal.x][2] = vec4<f32>(r.bleibt, 0.0);
    workgroupBarrier();
    if (lokal.x < 16u) {
        var summe = RueckTeil(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
        for (var t = 0u; t < 4u; t++) {
            let w = lm_rueck[lokal.x + 16u * t];
            summe.erster += w[0].xyz;
            summe.zahl += w[0].w;
            summe.zweiter += w[1].xyz;
            summe.bleibt += w[2].xyz;
        }
        let rueck = select(rgb9e5_packen(rueck_fertig(summe)), 0u, aussen);
        atomicStore(&lm[LM_RUECK_BASE + (seite * LM_FACES + platz) * 16u + lokal.x], rueck);
    }
    if (lokal.x == 0u) {
        if (mit_himmel) {
            let ziel = LM_HIMMEL_BASE + (seite * LM_FACES + platz) * 4u;
            for (var j = 0u; j < 4u; j++) {
                var wort = 0u;
                for (var i = 0u; i < 4u; i++) {
                    let p = j * 4u + i;
                    let h = (lm_himmel[p] + lm_himmel[p + 16u] + lm_himmel[p + 32u] + lm_himmel[p + 48u]) / f32(HIMMEL_STRAHLEN);
                    wort |= u32(round(clamp(h, 0.0, 1.0) * 255.0)) << (8u * i);
                }
                atomicStore(&lm[ziel + j], wort);
            }
        }
        let basis = (seite * LM_FACES + platz) * LM_LAGEN;
        var j = 0u;
        if (aussen) {
            lm_lagen[basis] = vec4<u32>(LM_AUSSEN);
            j = 1u;
        }
        for (var k = 0u; k < 16u; k++) {
            if (!aussen && ((lm_neu[k].x >> 27u) & 3u) != 0u && j < LM_LAGEN) {
                lm_lagen[basis + j] = lm_neu[k];
                j += 1u;
            }
        }
        for (; j < LM_LAGEN; j++) {
            lm_lagen[basis + j] = vec4<u32>(0u);
        }
    }
}

// Ebene mit `zahl` Linien (Kappen auf dem Rand als s, Abstaende Quelle -- Kante an ihren
// Enden mal `faktor`), Kopf schon mit Art und Bits.
fn linien_lage(kopf: u32, emission: u32, kappen_in: array<vec2<f32>, 2>, kanten_in: array<vec2<f32>, 2>, zahl: u32, faktor: f32) -> vec4<u32> {
    var kappen = kappen_in;
    var kanten = kanten_in;
    var daten = vec2<u32>(0u);
    var fein = 0u;
    for (var z = 0u; z < zahl; z++) {
        let qa = pack_kante(kanten[z].x * faktor);
        let qb = pack_kante(kanten[z].y * faktor);
        daten[z] = u32(clamp(round(kappen[z].x * 512.0), 0.0, 2047.0)) | u32(clamp(round(kappen[z].y * 512.0), 0.0, 2047.0)) << 11u | (qa >> 3u) << 22u | (qb >> 3u) << 27u;
        fein |= ((qa & 7u) | (qb & 7u) << 3u) << (20u + 6u * z);
    }
    return vec4<u32>(kopf, emission | fein, daten.x, daten.y);
}

// Abstand Quelle -- Kante in 8 Bit, fein bei kleinen: (q / 255)^2 * 64 Bloecke.
fn pack_kante(a: f32) -> u32 {
    return u32(clamp(round(sqrt(clamp(a, 0.0, 64.0) / 64.0) * 255.0), 0.0, 255.0));
}

fn unpack_kante(q: u32) -> f32 {
    let x = f32(q) / 255.0;
    return x * x * 64.0;
}

// Abstand Quelle -- Kante der Linie `z` einer Ebene an ihrem Anfang (x) und Ende (y).
fn linie_kanten(lage: vec4<u32>, z: u32) -> vec2<f32> {
    let daten = select(lage.z, lage.w, z == 1u);
    let fein = lage.y >> (20u + 6u * z);
    return vec2<f32>(unpack_kante(((daten >> 22u) & 31u) << 3u | (fein & 7u)), unpack_kante((daten >> 27u) << 3u | ((fein >> 3u) & 7u)));
}

// Halbschatten einer runden Quelle an einer geraden Kante: welcher Teil der Scheibe hinter
// der Kante liegt, wenn die Kante `x` Radien neben ihrer Mitte vorbeilaeuft (Flaeche des
// Kreisabschnitts). Statt eines beliebigen weichen Uebergangs genau die Kurve, die die
// Strahlen ergeben.
fn scheiben_anteil(x: f32) -> f32 {
    let c = clamp(x, -1.0, 1.0);
    return (acos(c) - c * sqrt(1.0 - c * c)) / 3.14159265;
}

// Sicht einer Ebene mit Linien an `uv` (`d_licht` von dort bis zur Mitte der Quelle): weich
// ueber die Breite ihres Halbschattens, mindestens `aa` (Kantenglaettung). Die Breite je Pixel
// aus dem Abstand der Kante zur Quelle (a): die Quelle erscheint von jeder Stelle aus gleich
// gross (Winkel params.block_soft.x), der Halbschatten reicht also so weit, wie sich ihr Strahl
// an der Kante um ihren Radius verschiebt -- halbe Breite tan * t * d / a mit t = d - a, dem
// Abstand von hier zur Kante. Mit dem Abstand waechst sie immer schneller: der Lichtkegel
// hinter einem Schlitz weitet sich in einer Kurve.
fn lage_sicht(lage: vec4<u32>, uv: vec2<f32>, aa: f32, d_licht: f32) -> f32 {
    let zahl = select(1u, 2u, ((lage.x >> 29u) & 1u) != 0u);
    let hell = ((lage.x >> 30u) & 1u) != 0u;
    let m = lage_rahmen(lage);
    var kappe = 0.0;
    for (var z = 0u; z < zahl; z++) {
        let daten = select(lage.z, lage.w, z == 1u);
        let a = rand_punkt_m(f32(daten & 2047u) / 512.0, m);
        let b = rand_punkt_m(f32((daten >> 11u) & 2047u) / 512.0, m);
        // Abstand der Kante entlang der Linie zwischen ihren beiden Enden.
        let t = clamp(dot(uv - a, b - a) / max(dot(b - a, b - a), 1e-6), 0.0, 1.0);
        let kanten = linie_kanten(lage, z);
        let kante = mix(kanten.x, kanten.y, t);
        var halb = aa;
        if (kante > 1e-3) {
            halb = max(params.block_soft.x * max(d_licht - kante, 0.0) * d_licht / kante, aa);
        }
        kappe += scheiben_anteil(linien_abstand(a, b, uv) / halb);
    }
    // Die Kappen ueberlappen nicht: zusammengezaehlt bleibt, wo sich zwei an einer Linie
    // treffen, keine Naht, und ein schmaler Spalt zwischen ihnen ist so hell, wie er breit ist.
    kappe = min(kappe, 1.0);
    return select(1.0 - kappe, kappe, hell);
}

// Richtung (wie lm_normale) einer achsenparallelen Normale.
fn lm_richtung(n: vec3<f32>) -> u32 {
    let an = abs(n);
    if (an.y > 0.99) {
        return select(0u, 1u, n.y > 0.0);
    }
    if (an.z > 0.99) {
        return select(2u, 3u, n.z > 0.0);
    }
    return select(4u, 5u, n.x > 0.0);
}

// Die Flaeche im G-Buffer in den Lichtkarten: ihr Platz ueber alle Seiten (lm_basis: Index
// ihrer ersten Ebene in lm_lagen), -1 keine Karte (schraege Flaeche, Section ohne Seite, kein
// Platz -- dann gilt die Ausbreitung), -2 Karte ohne Platz (nicht zu sehen).
fn lm_platz(f: Flaeche) -> i32 {
    let an = abs(f.n);
    if (max(an.x, max(an.y, an.z)) < 0.99) {
        return -1;
    }
    let w = vec3<i32>(floor(f.b - f.n * 0.01)) + params.cam_block.xyz;
    let sec = w >> vec3<u32>(4u);
    let s = sec & vec3<i32>(15);
    let slot_index = u32((s.y * 16 + s.z) * 16 + s.x);
    if (any(slots[slot_index].xyz != sec)) {
        return -1;
    }
    let seite = atomicLoad(&lm[LM_SLOT_BASE + slot_index]);
    if (seite == 0u) {
        return -1;
    }
    let lokal = u32(((w.y & 15) * 16 + (w.z & 15)) * 16 + (w.x & 15));
    let platz = atomicLoad(&lm[LM_INDEX_BASE + (seite - 1u) * 24576u + lokal * 6u + lm_richtung(f.n)]);
    if (platz == LM_VOLL) {
        return -1;
    }
    if (platz == 0u) {
        return -2;
    }
    return i32((seite - 1u) * LM_FACES + platz - 1u);
}

fn lm_basis(f: Flaeche) -> i32 {
    let platz = lm_platz(f);
    return select(platz, platz * i32(LM_LAGEN), platz >= 0);
}

// Gebackene Sicht der Flaeche auf den Himmel an ihrer Stelle (bilinear zwischen den 4x4
// Punkten), -1 ohne Karte.
fn himmel_sicht(f: Flaeche) -> f32 {
    let platz = lm_platz(f);
    if (platz < 0) {
        return -1.0;
    }
    let x = clamp(lm_uv(f) * 3.0, vec2<f32>(0.0), vec2<f32>(3.0));
    let i = min(floor(x), vec2<f32>(2.0));
    let a = x - i;
    let basis = LM_HIMMEL_BASE + u32(platz) * 4u;
    let unten = atomicLoad(&lm[basis + u32(i.y)]);
    let oben = atomicLoad(&lm[basis + u32(i.y) + 1u]);
    let s = u32(i.x) * 8u;
    let w = vec4<f32>(vec4<u32>(unten >> s, unten >> (s + 8u), oben >> s, oben >> (s + 8u)) & vec4<u32>(255u)) / 255.0;
    return mix(mix(w.x, w.y, a.x), mix(w.z, w.w, a.x), a.y);
}

// Gebackenes zurueckgeworfenes Blocklicht der Flaeche (s. back_rueckwurf), bilinear zwischen
// ihren 4x4 Punkten; a = 0 ohne Karte (oder ausserhalb des Licht-Rasters).
fn rueckwurf_at(f: Flaeche) -> vec4<f32> {
    let platz = lm_platz(f);
    if (platz < 0 || lm_lagen[u32(platz) * LM_LAGEN].x == LM_AUSSEN) {
        return vec4<f32>(0.0);
    }
    let x = clamp(lm_uv(f) * 3.0, vec2<f32>(0.0), vec2<f32>(3.0));
    let i = min(floor(x), vec2<f32>(2.0));
    let a = x - i;
    let basis = LM_RUECK_BASE + u32(platz) * 16u + u32(i.y) * 4u + u32(i.x);
    let w00 = rgb9e5_lesen(atomicLoad(&lm[basis]));
    let w10 = rgb9e5_lesen(atomicLoad(&lm[basis + 1u]));
    let w01 = rgb9e5_lesen(atomicLoad(&lm[basis + 4u]));
    let w11 = rgb9e5_lesen(atomicLoad(&lm[basis + 5u]));
    return vec4<f32>(mix(mix(w00, w10, a.x), mix(w01, w11, a.x), a.y), 1.0);
}

// Licht, das zwischen nahen Flaechen hin und her geworfen wird, hellt verdeckte Stellen auf
// -- umso mehr, je heller die Flaeche selbst ist. Angenaehert aus der Sicht `v` allein
// (Kurve nach Jimenez et al. 2016, "Practical Real-Time Strategies for Accurate Indirect
// Occlusion").
fn mehrfach_rueckwurf(v: f32, albedo: vec3<f32>) -> vec3<f32> {
    let a = 2.0404 * albedo - vec3<f32>(0.3324);
    let b = -4.7951 * albedo + vec3<f32>(0.6417);
    let c = 2.7552 * albedo + vec3<f32>(0.6903);
    return max(vec3<f32>(v), ((v * a + b) * v + c) * v);
}

fn lm_uv(f: Flaeche) -> vec2<f32> {
    let d = lm_richtung(f.n);
    return vec2<f32>(fract(dot(f.b, lm_achse_u(d))), fract(dot(f.b, lm_achse_v(d))));
}

// Block der Quelle einer Ebene, relativ zu cam_block.
fn lage_quelle(f: Flaeche, lage: vec4<u32>) -> vec3<i32> {
    let versatz = vec3<i32>(vec3<u32>(lage.x & 511u, (lage.x >> 9u) & 511u, (lage.x >> 18u) & 511u)) - vec3<i32>(256);
    return vec3<i32>(floor(f.b - f.n * 0.01)) + versatz;
}

// Direktes Blocklicht einer Flaeche im G-Buffer aus ihren Ebenen: rgb, a = 1 gefunden.
fn lightmap_at(f: Flaeche) -> vec4<f32> {
    let basis = lm_basis(f);
    if (basis == -1) {
        return vec4<f32>(0.0);
    }
    if (basis == -2) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (lm_lagen[u32(basis)].x == LM_AUSSEN) {
        return vec4<f32>(0.0);
    }
    let uv = lm_uv(f);
    // Kantenglaettung: etwa ein Pixel, waechst mit dem Abstand.
    let aa = 0.003 + 0.0008 * length(f.o);
    let block = vec3<i32>(floor(f.b - f.n * 0.01));
    var licht = vec3<f32>(0.0);
    for (var k = 0u; k < LM_LAGEN; k++) {
        let lage = lm_lagen[u32(basis) + k];
        let art = (lage.x >> 27u) & 3u;
        if (art == 0u) {
            break;
        }
        let quelle = lage_quelle(f, lage);
        let lo = vec3<f32>(quelle);
        let staerke = light_strength(vec2<u32>(0u, lage.y), lo, f.o, f.n, all(quelle == block));
        if (all(staerke <= vec3<f32>(0.0))) {
            continue;
        }
        if (art == 1u) {
            licht += staerke;
        } else if (art == 2u) {
            licht += staerke * lage_sicht(lage, uv, aa, length(lo + vec3<f32>(0.5) - f.o));
        } else if ((lage.x >> 31u) != 0u) {
            licht += staerke * raster_ebene_sicht(lage, u32(basis) / LM_LAGEN, uv);
        } else {
            licht += staerke * light_visibility_rays(f.o, lo);
        }
    }
    return vec4<f32>(licht, 1.0);
}

// Wie viel die Lampen an der Stelle `b` einer Flaeche mit Normale `n` ankommen lassen, so wie
// der Licht-Pass es aus den Lichtkarten liest -- aus der Karte der Flaeche, die dort liegt (auch
// neben der Flaeche im Bild, s. lightmap_debug): die Sicht je Quelle (frei, Linien, Kanten,
// Raster, Strahlen), gewichtet nach ihrer Helligkeit dort. -1: keine Karte oder keine Quelle.
// `b` liegt auf der Flaeche (danach wird die Karte gesucht), Licht und Strahlen gehen wie im
// Licht-Pass vom Punkt `vor` davor aus (s. flaeche).
fn debug_sicht(n: vec3<f32>, b: vec3<f32>, vor: vec3<f32>) -> f32 {
    let p = b + vor;
    let f = Flaeche(n, b, p);
    let basis = lm_basis(f);
    if (basis < 0 || lm_lagen[u32(basis)].x == LM_AUSSEN) {
        return -1.0;
    }
    let uv = lm_uv(f);
    let block = vec3<i32>(floor(b - n * 0.01));
    var summe = 0.0;
    var gewicht = 0.0;
    for (var k = 0u; k < LM_LAGEN; k++) {
        let lage = lm_lagen[u32(basis) + k];
        let art = (lage.x >> 27u) & 3u;
        if (art == 0u) {
            break;
        }
        let quelle = lage_quelle(f, lage);
        let lo = vec3<f32>(quelle);
        let staerke = light_strength(vec2<u32>(0u, lage.y), lo, p, n, all(quelle == block));
        let g = max(staerke.r, max(staerke.g, staerke.b));
        if (g <= 0.0) {
            continue;
        }
        var v = 1.0;
        if (art == 2u) {
            v = lage_sicht(lage, uv, 0.002, length(lo + vec3<f32>(0.5) - p));
        } else if (art == 3u && (lage.x >> 31u) != 0u) {
            v = raster_ebene_sicht(lage, u32(basis) / LM_LAGEN, uv);
        } else if (art == 3u) {
            // Wie im Licht-Pass (lm_strahlen, je Stelle gedreht): mit den vielen Strahlen der
            // Back-Pruefung, fuenfmal je Pixel fuer die Grenze, war die Ansicht zu langsam.
            v = dot(light_visibility_rays(p, lo), vec3<f32>(1.0 / 3.0));
        }
        summe += g * v;
        gewicht += g;
    }
    return select(-1.0, summe / max(gewicht, 1e-6), gewicht > 0.0);
}

// Lichtkarten-Ansicht (Einstellung, block_soft.w = 1): wie viel Licht der Lampen ankommt, aus
// den Lichtkarten (s. debug_sicht) -- gelb am meisten, blau am wenigsten, schwarz keins. Rot
// die Grenze, wo es aufhoert, gruen, wo es voll ankommt (dazwischen der Halbschatten): auch wo es an einer Blockkante springt (die Werte daneben kommen
// aus der Karte der Nachbarflaeche). Flaechen ohne Lichtkarte dunkelgrau.
fn lightmap_debug(f: Flaeche) -> vec3<f32> {
    let basis = lm_basis(f);
    if (basis == -1 || (basis >= 0 && lm_lagen[u32(max(basis, 0))].x == LM_AUSSEN)) {
        return vec3<f32>(0.06);
    }
    let vor = f.o - f.b;
    let v = max(debug_sicht(f.n, f.b, vor), 0.0);
    // Etwa ein Pixel in Bloecken (wie die Kantenglaettung in lightmap_at).
    let px = 0.002 + 0.0008 * length(f.o);
    let schwelle = 0.01;
    // Ohne Sprung: von schwarz (kein Licht) ueber blau nach gelb (alles).
    let farbe = mix(vec3<f32>(0.1, 0.15, 1.0), vec3<f32>(1.0, 0.95, 0.1), smoothstep(0.0, 1.0, v)) * smoothstep(0.0, 0.15, v);
    // Abstand zur Grenze: Wert durch Steigung, ueber ein Pixel gemessen -- links, rechts, oben,
    // unten in der Ebene, notfalls schon auf der Flaeche nebenan.
    let d = lm_richtung(f.n);
    let du = lm_achse_u(d) * px;
    let dv = lm_achse_v(d) * px;
    let g = vec2<f32>(max(debug_sicht(f.n, f.b + du, vor), 0.0) - max(debug_sicht(f.n, f.b - du, vor), 0.0), max(debug_sicht(f.n, f.b + dv, vor), 0.0) - max(debug_sicht(f.n, f.b - dv, vor), 0.0)) / (2.0 * px);
    let steigung = max(length(g), 1e-3);
    let abstand = abs(v - schwelle) / steigung;
    // Gruen, wo das Licht voll ankommt (aussen am Halbschatten).
    let voll = abs(v - 0.99) / steigung;
    let mit_rot = mix(farbe, vec3<f32>(1.0, 0.0, 0.0), 1.0 - smoothstep(1.0 * px, 2.0 * px, abstand));
    return mix(mit_rot, vec3<f32>(0.0, 0.9, 0.2), 1.0 - smoothstep(1.0 * px, 2.0 * px, voll));
}

// Lichtkarten-Ansicht Abweichung (block_soft.w = 2): je Pixel, wie weit die gespeicherte Sicht
// (frei, Linien, Raster) von echten Strahlen abweicht (128, die Quelle als Scheibe)
// -- die groesste ueber alle Quellen, nach ihrer Helligkeit hier gewichtet. Schwarz gleich, rot
// zu hell, blau zu dunkel, voll bei 25 %. Darunter schwach das Bild, die Blockkanten grau.
// Flaechen je Pixel mit Strahlen weichen nur um ihr Korn ab.
fn lightmap_error(f: Flaeche, albedo: vec3<f32>) -> vec3<f32> {
    let basis = lm_basis(f);
    let grund = vec3<f32>(0.12 * dot(albedo, vec3<f32>(0.33)));
    if (basis < 0 || lm_lagen[u32(basis)].x == LM_AUSSEN) {
        return grund + vec3<f32>(0.15, 0.0, 0.15);
    }
    let uv = lm_uv(f);
    let block = vec3<i32>(floor(f.b - f.n * 0.01));
    var zu_hell = 0.0;
    var zu_dunkel = 0.0;
    for (var k = 0u; k < LM_LAGEN; k++) {
        let lage = lm_lagen[u32(basis) + k];
        let art = (lage.x >> 27u) & 3u;
        if (art == 0u) {
            break;
        }
        let quelle = lage_quelle(f, lage);
        if (all(quelle == block)) {
            continue;
        }
        let lo = vec3<f32>(quelle);
        let staerke = light_strength(vec2<u32>(0u, lage.y), lo, f.o, f.n, false);
        let gewicht = min(max(staerke.r, max(staerke.g, staerke.b)) * 4.0, 1.0);
        if (gewicht <= 0.0) {
            continue;
        }
        var gespeichert = 1.0;
        if (art == 2u) {
            gespeichert = lage_sicht(lage, uv, 0.003 + 0.0008 * length(f.o), length(lo + vec3<f32>(0.5) - f.o));
        } else if (art == 3u) {
            if ((lage.x >> 31u) != 0u) {
                gespeichert = raster_ebene_sicht(lage, u32(basis) / LM_LAGEN, uv);
            } else {
                continue;
            }
        }
        // 128 Strahlen, die Spirale je Stelle gedreht: mit festen Strahlen zeigte die Ansicht
        // vor allem ihre eigenen Stufen (Streifen).
        let stelle = vec3<i32>(floor((f.o + vec3<f32>(params.cam_block.xyz & vec3<i32>(1023))) * 64.0));
        let wahr = dot(light_visibility_n(f.o, lo, 128, hash3(stelle)), vec3<f32>(1.0 / 3.0));
        let d = (gespeichert - wahr) * gewicht;
        zu_hell = max(zu_hell, d);
        zu_dunkel = max(zu_dunkel, -d);
    }
    let px = 0.002 + 0.0008 * length(f.o);
    let kante = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    let raster = vec3<f32>(0.3) * (1.0 - smoothstep(0.5 * px, 1.0 * px, kante));
    return max(grund + vec3<f32>(zu_hell, 0.0, zu_dunkel) * 4.0, raster);
}

// Helligkeit zu einer Lichtstufe (x = Stufe / 15) wie light_curve, ueber 1 hinaus weiter:
// mit derselben Steigung angesetzt, dann logarithmisch -- Stufe 30 ist knapp viermal, Stufe
// 75 gut siebenmal so hell wie 15, ohne Knick dazwischen.
fn fern_kurve(x: f32) -> f32 {
    if (x <= 0.0) {
        return 0.0;
    }
    if (x <= 1.0) {
        return x / (4.0 - 3.0 * x);
    }
    return 1.0 + 4.0 * log(x);
}

// Fernlicht: Quellen mit Lichtstaerke ueber 15 (s. world::light::emission_level). Bis 15
// Bloecke tragen sie zum Blocklicht der Ausbreitung bei wie jede Quelle; hier kommt nur
// hinzu, was darueber hinausgeht -- nah an der Quelle die zusaetzliche Helligkeit, weiter weg
// alles. Es faellt wie Blocklicht um eine Stufe je Block ab, reicht aber so weit wie die
// Stufe hoch ist, und kommt nur an, wo der Strahl zur Quelle frei ist (weich wie
// light_visibility_rays, gleicher Kegel). Rueckgabe: rgb = zusaetzliches Licht (in der
// Einheit von light_curve), a = kleinster Abstand zu einem Hindernis (-1 = keins).
fn far_light(f: Flaeche, rauschen: f32) -> vec4<f32> {
    var licht = vec3<f32>(0.0);
    var hindernis = 1e30;
    for (var k = 0; k < FAR_LIGHTS_MAX; k++) {
        let q = far_lights[k];
        if (q.w == 0) {
            break;
        }
        let e = u32(q.w);
        let farbe = vec3<f32>(f32(e & 15u), f32((e >> 4u) & 15u), f32((e >> 8u) & 15u));
        let nah = max(max(farbe.r, max(farbe.g, farbe.b)), 1.0);
        let stufe = f32((e >> 12u) & 255u);
        let lo = vec3<f32>(q.xyz - params.cam_block.xyz);
        let zu = lo + vec3<f32>(0.5) - f.o;
        let abstand = length(zu);
        if (abstand >= stufe) {
            continue;
        }
        let mehr = fern_kurve((stufe - abstand) / 15.0) - fern_kurve((nah - abstand) / 15.0);
        if (mehr <= 0.0) {
            continue;
        }
        let anteil = farbe / nah * mehr;
        if (all(f.o > lo - vec3<f32>(0.1)) && all(f.o < lo + vec3<f32>(1.1))) {
            licht += anteil;
            continue;
        }
        let mitte_dir = zu / abstand;
        let ndl = dot(f.n, mitte_dir);
        if (ndl <= 0.0) {
            continue;
        }
        let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(mitte_dir.y) > 0.9);
        let u = normalize(cross(mitte_dir, hilf));
        let w = cross(mitte_dir, u);
        let xi = fract(rauschen + f32(k) * 0.618034);
        let winkel = (rauschen + f32(k) * 0.381966) * 6.2831853 * 7.0;
        let dir = normalize(mitte_dir + (u * cos(winkel) + w * sin(winkel)) * sqrt(xi) * params.block_soft.x);
        var t_hit = -1.0;
        let durch = trace(f.o, dir, light_ray_length(f.o, dir, lo), &t_hit);
        if (t_hit >= 0.0) {
            hindernis = min(hindernis, t_hit);
        }
        licht += anteil * durch.rgb * ndl;
    }
    return vec4<f32>(licht, select(-1.0, hindernis, hindernis < 1e29));
}

@fragment
fn fs_far_light(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(p.xy);
    if (params.block_soft.z < 0.5 || textureLoad(albedo_tex, pixel, 0).a < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    // Wo eine Lichtkarte liegt, steckt das Fernlicht schon darin (s. baked_far_light).
    let f = flaeche(p, pixel);
    if (params.light_grid.w != 0 && lightmap_at(f).a > 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    return far_light(f, ign(p.xy));
}

// --- Spiegelungen ------------------------------------------------------------------------
//
// Glaenzende Bloecke (s. world::material: Metalle, Eis, Quarz, ...) spiegeln, was in Richtung
// des gespiegelten Blicks liegt: ein Strahl je Pixel durch das Voxel-Abbild. Raue Flaechen
// streuen ihn um die Spiegelrichtung (GGX), jeden Frame anders -- der Verlauf (wie beim
// Fernlicht) mittelt daraus die Unschaerfe. Getroffene Bloecke zeigen ihre Textur im Licht
// ihrer Lichtkarte, dazu Himmel und Sonne; ohne Treffer der Himmel.

// So weit sieht eine Spiegelung (Bloecke).
const REFLEXION_WEITE: f32 = 96.0;

// Microfacet-Normale um `n` nach GGX mit Alpha `a` (= Rauheit^2), `xi` 0..1 zweimal.
fn ggx_normale(n: vec3<f32>, a: f32, xi: vec2<f32>) -> vec3<f32> {
    let cos_t = sqrt((1.0 - xi.x) / (1.0 + (a * a - 1.0) * xi.x));
    let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
    let winkel = xi.y * 6.2831853;
    let hilf = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let u = normalize(cross(n, hilf));
    let w = cross(n, u);
    return normalize(u * (sin_t * cos(winkel)) + w * (sin_t * sin(winkel)) + n * cos_t);
}

// Licht, das eine getroffene Flaeche (Seite `d` des Blocks `zelle`, Punkt `p` darauf) in die
// Spiegelung zurueckwirft: ihre Textur im Licht der Lichtkarte, des Himmels und der Sonne.
fn treffer_licht(p: vec3<f32>, d: u32, zelle: vec3<i32>) -> vec3<f32> {
    let n = vec3<f32>(lm_normale(d));
    let hf = Flaeche(n, p, p + n * 0.01);
    let farbe = block_texel(zelle, d, p);
    // Eine Lampe leuchtet selbst.
    if (rueck_lampe(hf.o, zelle)) {
        return farbe * 1.4;
    }
    var licht = vec3<f32>(0.015);
    // Himmel: gebackene Sicht, ohne Karte ein Strahl nach oben.
    var sicht = himmel_sicht(hf);
    if (sicht < 0.0) {
        var egal = -1.0;
        let hoch = trace(hf.o, vec3<f32>(0.0, 1.0, 0.0), 256.0, &egal);
        sicht = dot(hoch.rgb, vec3<f32>(1.0 / 3.0)) * (0.5 + 0.5 * n.y);
    }
    let daylight = params.sky.w;
    let streuung = select(1.0, params.sun_dir.w, params.sun_axis.w > 0.5);
    licht += light_curve(vec3<f32>(1.0)) * params.block_soft.y * (0.2 + 0.8 * daylight) * mix(vec3<f32>(0.55, 0.62, 0.95), vec3<f32>(1.0), daylight)
        * params.light_color.w * streuung * sicht;
    // Blocklicht aus der Lichtkarte (direkt und zurueckgeworfen).
    if (params.light_grid.w != 0) {
        let karte = lightmap_at(hf);
        if (karte.a > 0.5) {
            licht += (karte.rgb + rueckwurf_at(hf).rgb) * 1.4;
        }
    }
    // Sonne bzw. Mond, ein Strahl (harter Schatten).
    let l = params.light_dir.xyz;
    let ndotl = dot(n, l);
    if (ndotl > 0.0 && params.light_dir.w > 0.5) {
        var egal = -1.0;
        licht += params.light_color.rgb * ndotl * trace(hf.o, l, 160.0, &egal).rgb;
    }
    return farbe * licht;
}

// Spiegel-Pass: rgb = gespiegeltes Licht (vor Fresnel), a = 0; matte Pixel schwarz.
@fragment
fn fs_reflexion(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(p.xy);
    if (textureLoad(albedo_tex, pixel, 0).a < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    let f = flaeche(p, pixel);
    let m = material_an(vec3<i32>(floor(f.b - f.n * 0.01)));
    if (m.w < 0.5) {
        return vec4<f32>(0.0);
    }
    let blick = normalize(f.b - params.cam_frac.xyz);
    let alpha = max(m.y * m.y, 0.002);
    // Zwei unabhaengige Zahlen: beide aus ign blieben nach dem Mitteln als feine diagonale
    // Streifen stehen (sichtbar auf Eisen).
    let xi = vec2<f32>(ign(p.xy), hash3(vec3<i32>(pixel, params.cam_block.w)));
    var r = reflect(blick, ggx_normale(f.n, alpha, xi));
    if (dot(r, f.n) <= 0.0) {
        r = reflect(blick, f.n);
    }
    var t_hit = -1.0;
    let durch = trace(f.o, r, REFLEXION_WEITE, &t_hit);
    if (t_hit < 0.0) {
        // Frei bis zum Rand: Himmel, nur wo der Strahl nicht im Unbekannten endet (Hoehlen).
        let offen = select(1.0, clamp((textureLoad(light_tex, pixel, 0).a * 15.0 - 6.0) / 6.0, 0.0, 1.0), durch.w > 0.5);
        return vec4<f32>(sky_color(r) * durch.rgb * offen, 0.0);
    }
    // Getroffen beim Eintritt in eine Zelle oder an einer Teilflaeche: die Seite ist die
    // Achse, deren 1/16-Ebene dort liegt.
    let q = f.o + r * t_hit;
    let ab = abs(q * 16.0 - round(q * 16.0));
    var d = 4u + u32(r.x < 0.0);
    if (ab.y <= ab.x && ab.y <= ab.z) {
        d = u32(r.y < 0.0);
    } else if (ab.z <= ab.x) {
        d = 2u + u32(r.z < 0.0);
    }
    let zelle = vec3<i32>(floor(q + r * 0.001));
    return vec4<f32>(treffer_licht(q, d, zelle), 0.0);
}

// Spiegelung am Pixel, gemittelt mit den Nachbarn auf derselben glaenzenden Flaeche (gleiche
// Ebene, gleiche Normale, auch glaenzend) -- je rauer, desto weiter auseinander. Der Verlauf
// allein laesst bei rauen Flaechen sichtbares Rauschen stehen.
fn reflexion_gefiltert(pixel: vec2<i32>, f: Flaeche, rauheit: f32) -> vec3<f32> {
    let mitte = textureLoad(reflexion_tex, pixel, 0).rgb;
    let abstand = i32(round(rauheit * 4.0));
    if (abstand == 0) {
        return mitte;
    }
    let groesse = vec2<i32>(textureDimensions(reflexion_tex));
    var summe = mitte;
    var gewicht = 1.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let q = pixel + vec2<i32>(dx, dy) * abstand;
            if (any(q < vec2<i32>(0)) || any(q >= groesse) || textureLoad(albedo_tex, q, 0).a < 0.5) {
                continue;
            }
            let nf = flaeche(vec4<f32>(vec2<f32>(q) + vec2<f32>(0.5), 0.0, 1.0), q);
            if (dot(nf.n, f.n) < 0.95 || abs(dot(nf.b - f.b, f.n)) > 0.02 || material_an(vec3<i32>(floor(nf.b - nf.n * 0.01))).w < 0.5) {
                continue;
            }
            let g = select(1.0, 0.7, dx != 0 && dy != 0);
            summe += textureLoad(reflexion_tex, q, 0).rgb * g;
            gewicht += g;
        }
    }
    return summe / gewicht;
}

// Lichtkarten-Ansicht Art (block_soft.w = 3): wie das Licht der hellsten Quelle hier gespeichert
// ist -- gruen frei, gelb Sicht-Raster (heller, wo mehr zu sehen ist), blau Linien, rot
// Strahlen je Pixel, grau keine Ebene, lila ausserhalb des Licht-Rasters, schwarz keine Karte.
// Die Blockgrenzen dunkel, damit man sieht, was an einer Flaeche haengt.
fn lightmap_art(f: Flaeche) -> vec3<f32> {
    let uv = lm_uv(f);
    let px = 0.002 + 0.0008 * length(f.o);
    let grenze = select(1.0, 0.45, any(min(uv, vec2<f32>(1.0) - uv) < vec2<f32>(px)));
    let basis = lm_basis(f);
    if (basis < 0) {
        return vec3<f32>(0.0);
    }
    if (lm_lagen[u32(basis)].x == LM_AUSSEN) {
        return vec3<f32>(0.5, 0.2, 0.6) * grenze;
    }
    let block = vec3<i32>(floor(f.b - f.n * 0.01));
    var beste = vec4<u32>(0u);
    var hell = 0.0;
    for (var k = 0u; k < LM_LAGEN; k++) {
        let lage = lm_lagen[u32(basis) + k];
        if (((lage.x >> 27u) & 3u) == 0u) {
            break;
        }
        let quelle = lage_quelle(f, lage);
        let st = light_strength(vec2<u32>(0u, lage.y), vec3<f32>(quelle), f.o, f.n, all(quelle == block));
        let g = max(st.r, max(st.g, st.b));
        if (g > hell) {
            hell = g;
            beste = lage;
        }
    }
    let art = (beste.x >> 27u) & 3u;
    var farbe = vec3<f32>(0.3);
    if (art == 1u) {
        farbe = vec3<f32>(0.2, 0.8, 0.2);
    } else if (art == 2u) {
        farbe = vec3<f32>(0.2, 0.4, 1.0);
    } else if (art == 3u && (beste.x >> 31u) != 0u) {
        farbe = vec3<f32>(1.0, 0.9, 0.2) * (0.3 + 0.7 * raster_ebene_sicht(beste, u32(basis) / LM_LAGEN, uv));
    } else if (art == 3u) {
        farbe = vec3<f32>(1.0, 0.15, 0.1);
    }
    return farbe * grenze;
}

@fragment
fn fs_light(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(p.xy);
    let albedo = textureLoad(albedo_tex, pixel, 0);
    if (albedo.a < 0.5) {
        let size = vec2<f32>(textureDimensions(albedo_tex));
        let ndc = vec2<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0);
        let v = params.inv_proj * vec4<f32>(ndc, 1.0, 1.0);
        let blick = normalize((params.inv_view * vec4<f32>(v.xyz / v.w, 0.0)).xyz);
        return vec4<f32>(sky_color(blick), 1.0);
    }
    let licht = textureLoad(light_tex, pixel, 0);
    let daylight = params.sky.w;
    let f = flaeche(p, pixel);
    if (params.block_soft.w > 2.5) {
        return vec4<f32>(lightmap_art(f), 1.0);
    }
    if (params.block_soft.w > 1.5) {
        return vec4<f32>(lightmap_error(f, albedo.rgb), 1.0);
    }
    if (params.block_soft.w > 0.5) {
        return vec4<f32>(lightmap_debug(f), 1.0);
    }
    let n = f.n;
    let verfolgen = params.light_dir.w > 0.5;

    // Himmelslicht vom Server, nur nach oben korrigiert: ist senkrecht darueber im Abbild
    // frei, ist das offener Himmel -- auch wenn der Server dort dunkel meldet. PvP-Arenen
    // sind oft ohne Neuberechnung des Lichts gebaut, ihr Himmelslicht ist dann fleckig.
    var himmel_stufe = licht.a;
    if (verfolgen && himmel_stufe < 0.99) {
        var egal = -1.0;
        let hoch = trace(f.o, vec3<f32>(0.0, 1.0, 0.0), 256.0, &egal);
        himmel_stufe = max(himmel_stufe, dot(hoch.rgb, vec3<f32>(1.0 / 3.0)));
    }

    // Himmel als Umgebungslicht: Vanillas Kurve, nachts auf 20 % und leicht blau. Ein Teil
    // davon (light_color.w) bleibt Umgebung, der Rest kommt gerichtet von Sonne/Mond.
    let himmel = light_curve(vec3<f32>(himmel_stufe)) * params.block_soft.y * (0.2 + 0.8 * daylight) * mix(vec3<f32>(0.55, 0.62, 0.95), vec3<f32>(1.0), daylight);
    // Das Mesh traegt im Shader-Pfad weder Vanillas feste Helligkeit je Seite noch ihre
    // Eckenabdunklung (s. world::mesher::set_flat_face_shade, ao_strength): Schatten kommen
    // nur von echten Lichtquellen.
    // - Umgebungslicht vom Himmel ist in der Luft gestreutes Licht -- so viel, wie die
    //   Einstellung Lichtstreuung sagt (sun_dir.w, 1 = Erde; ohne Atmosphaere wie gehabt).
    //   Den Himmel sieht eine Flaeche je nach Neigung ganz (oben), halb (Seite) oder gar nicht
    //   (unten); was sie nicht sieht, ist Boden, der einen Teil zurueckwirft.
    // - Direktes Licht (Lichtkarten, Fernlicht, Sonne) kommt per Strahl, mit seiner echten
    //   Richtung.
    // - Wie viel Himmel sie sieht, ist gebacken (s. back_himmel): Ecken, Nischen, Raeume
    //   bekommen weniger -- ein geschlossener Raum keinen, auch wenn der Server dort Himmels-
    //   licht meldet. Ohne Karte (weit weg, schraeg): nach der Neigung allein; zum Rand der
    //   Karten hin dorthin uebergeblendet.
    let hemi = 0.5 + 0.5 * n.y;
    var sicht = vec3<f32>(hemi + BODEN_RUECKWURF * (1.0 - hemi));
    if (params.light_grid.w != 0) {
        let gebacken = himmel_sicht(f);
        if (gebacken >= 0.0) {
            sicht = mix(mehrfach_rueckwurf(gebacken, albedo.rgb), sicht, smoothstep(48.0, 56.0, length(f.o)));
        }
    }
    let streuung = select(1.0, params.sun_dir.w, params.sun_axis.w > 0.5);
    var umgebung = himmel * params.light_color.w * streuung * sicht;
    var direkt = vec3<f32>(0.0);
    let flut = light_curve(licht.rgb) * 1.4;
    // Mit Lichtkarte: direktes Licht von dort und das gebackene zurueckgeworfene (s.
    // back_rueckwurf). Ohne den Rueckwurf bleibt von der Ausbreitung ein fester Anteil (sonst
    // kaeme Licht um jede Ecke). Ohne Karte: die Ausbreitung ganz.
    var karte = vec4<f32>(0.0);
    var rueck = vec4<f32>(0.0);
    if (params.light_grid.w != 0) {
        karte = lightmap_at(f);
        rueck = rueckwurf_at(f);
    }
    if (karte.a > 0.5 && rueck.a > 0.5) {
        umgebung += rueck.rgb * 1.4;
        direkt += karte.rgb * 1.4;
    } else if (karte.a > 0.5) {
        umgebung += flut * BLOCK_BOUNCE;
        direkt += karte.rgb * 1.4 * (1.0 - BLOCK_BOUNCE);
    } else {
        umgebung += flut;
    }
    if (params.block_soft.z > 0.5) {
        direkt += textureLoad(far_light_tex, pixel, 0).rgb * 1.4;
    }
    var farbe = vec3<f32>(0.015) + umgebung + direkt;

    // Gerichtetes Licht mit dem Schatten aus fs_shadow (gemittelt).
    let l = params.light_dir.xyz;
    let ndotl = dot(n, l);
    if (ndotl > 0.0 && verfolgen) {
        farbe += params.light_color.rgb * ndotl * textureLoad(shadow_tex, pixel, 0).rgb;
    } else if (!verfolgen) {
        let offen = clamp((himmel_stufe * 15.0 - 6.0) / 6.0, 0.0, 1.0);
        farbe += params.light_color.rgb * max(ndotl, 0.0) * offen;
    }
    // Glaenzt der Block: Fresnel (Schlick, mit Rauheit gedaempft) zwischen gestreutem und
    // gespiegeltem Licht. Metalle streuen nichts und spiegeln in ihrer eigenen Farbe.
    let m = material_an(vec3<i32>(floor(f.b - n * 0.01)));
    if (m.w > 0.5) {
        let blick = normalize(f.b - params.cam_frac.xyz);
        let f0 = mix(vec3<f32>(m.z), albedo.rgb, m.x);
        let kante = pow(1.0 - clamp(dot(n, -blick), 0.0, 1.0), 5.0);
        let fresnel = f0 + (max(vec3<f32>(1.0 - m.y), f0) - f0) * kante;
        let spiegel = reflexion_gefiltert(pixel, f, m.y);
        let gestreut = albedo.rgb * farbe * (1.0 - m.x);
        return vec4<f32>(gestreut * (vec3<f32>(1.0) - fresnel) + spiegel * fresnel, 1.0);
    }
    return vec4<f32>(albedo.rgb * farbe, 1.0);
}
