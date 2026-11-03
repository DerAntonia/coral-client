// Zeigt die am staerksten unterscheidbare Farbe zum Pixel darunter: contrast_color aus
// render/src/contrast_color.wgsl, das beim Laden an diese Datei angehaengt wird.
@group(0) @binding(0) var sample_tex: texture_2d<f32>;
@group(0) @binding(1) var sample_sampler: sampler;

// Farbmodus, im HUD-Editor waehlbar (s. CROSSHAIR_COLOR_MODES in main.rs): 0 = Komplementaer
// (Standard), 1 = Einfarbig in `color` -- fuer Faelle, in denen die automatische Gegenfarbe
// eher stoert (z.B. sehr bewegte/bunte Untergruende).
//
// `brightness_boost` (0..1) hellt NUR die Komplementaerfarbe auf, s. fs_main. `color` ist
// RGBA8, R im niedrigsten Byte. 80 Byte: Uniform-Bloecke muessen in WGSL ein Vielfaches von
// 16 gross sein, `areas` liegt dafuer passend ab Byte 16.
struct CrosshairSettings {
    mode: u32,
    brightness_boost: f32,
    color: u32,
    // Brenn-Farbe (Feuer-Modus "Fadenkreuz"), nur fuer Kreuz (0) und Kreis/Ring (5) -- Zahlen
    // behalten `color`. Alpha-Byte 0 = brennt nicht.
    fire: u32,
    // Flaechen (uv: links oben xy, rechts unten zw) je Farbplatz (s. FADE_PLAETZE). Im
    // Komplementaer-Modus bekommt jeder Platz eine Farbe, gewaehlt ueber seine ganze Flaeche
    // (s. cs_fade). Leer (zw nicht rechts unter xy) = der Platz ist gerade nicht zu sehen.
    areas: array<vec4<f32>, 3>,
    // Sekunden seit dem letzten cs_fade.
    dt: f32,
};
@group(0) @binding(2) var<uniform> settings: CrosshairSettings;

// Komplementaer-Farbe ueber mehrere Frames (s. cs_fade): `ziel` 0 = Gelb, 1 = Blau, -1 =
// gerade nicht zu sehen; `t` die gezeigte Mischung.
struct CrosshairFade {
    ziel: f32,
    t: f32,
    _pad: vec2<f32>,
};
// Farbplaetze: 0 Kreuz und Ring, 1 Schild-Countdown, 2 Pfeilzahl. Die Zahlen stehen woanders
// als das Kreuz und brauchen die Gegenfarbe zu ihrem eigenen Untergrund.
const FADE_PLAETZE: u32 = 3u;
@group(0) @binding(3) var<storage, read_write> fade: array<CrosshairFade, 3>;
// Wechselt die Farbe, wird in dieser Zeit ueberblendet.
const FADE_DAUER: f32 = 0.1;

// Font-Atlas fuer HUD-Zahlen (Schild-Countdown, Pfeile), die hier statt im UI-Pass gezeichnet
// werden, damit sie die Fadenkreuz-Farbe bekommen koennen. Einkanalig: Deckung im Rotanteil.
@group(1) @binding(0) var glyph_tex: texture_2d<f32>;
@group(1) @binding(1) var glyph_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) glyph_uv: vec2<f32>,
    // Art des Dreiecks (s. main.rs::CrosshairVertex::kind): 0 Flaeche, 1 Kontur, 2 Glyphe in
    // Fadenkreuz-Farbe, 3 Glyphe schwarz, 4 Glyphe weiss, 5 Kreis in Fadenkreuz-Farbe, 6 Kreis
    // schwarz. Per-Vertex statt per-Uniform, weil
    // alles in EINEM Zeichenaufruf rausgeht. @interpolate(flat): je Dreieck konstant, eine
    // Interpolation koennte auf der gemeinsamen Kante Zwischenwerte erzeugen.
    @location(2) @interpolate(flat) kind: f32,
    @location(3) @interpolate(flat) alpha: f32,
    // Kreise: innerer und aeusserer Radius in Pixeln (glyph_uv ist dann der Abstand zur Mitte).
    // Glyphen: x = Farbplatz (s. FADE_PLAETZE).
    @location(4) @interpolate(flat) circle: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) glyph_uv: vec2<f32>,
    @location(3) kind: f32,
    @location(4) alpha: f32,
    @location(5) circle: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.glyph_uv = glyph_uv;
    out.kind = kind;
    out.alpha = alpha;
    out.circle = circle;
    return out;
}

// Eine Gegenfarbe fuer die ganze Flaeche eines Platzes: 7x7 Punkte darueber, je Punkt die
// Abstaende von Gelb und Blau aufsummiert, die Farbe mit der groesseren Summe gewinnt. Je
// Pixel einzeln waere das Kreuz z.B. auf einem Spinnennetz bunt gescheckt. Laeuft einmal je
// Frame vor dem Fadenkreuz-Pass (ein Thread je Platz), gezeichnet wird nur mit `fade[i].t`.
@compute @workgroup_size(1)
fn cs_fade(@builtin(workgroup_id) id: vec3<u32>) {
    let platz = id.x;
    let area = settings.areas[platz];
    // Nicht zu sehen: beim naechsten Auftauchen gleich die passende Farbe, ohne Ueberblenden.
    if (area.z <= area.x || area.w <= area.y) {
        fade[platz].ziel = -1.0;
        return;
    }
    let groesse = vec2<f32>(textureDimensions(sample_tex));
    let hoechstens = vec2<i32>(groesse) - 1;
    var summe = vec2<f32>(0.0);
    for (var j = 0; j < 7; j++) {
        for (var i = 0; i < 7; i++) {
            let t = (vec2<f32>(f32(i), f32(j)) + 0.5) / 7.0;
            let uv = mix(area.xy, area.zw, t);
            let pixel = clamp(vec2<i32>(uv * groesse), vec2<i32>(0), hoechstens);
            summe += contrast_scores(textureLoad(sample_tex, pixel, 0).rgb);
        }
    }
    let wahl = select(1.0, 0.0, summe.x >= summe.y);
    var f = fade[platz];
    if (f.ziel < 0.0) {
        f.t = wahl;
    }
    f.ziel = wahl;
    let schritt = settings.dt / FADE_DAUER;
    f.t = clamp(f.t + clamp(f.ziel - f.t, -schritt, schritt), 0.0, 1.0);
    fade[platz] = f;
}

// Die Fadenkreuz-Farbe, je nach Modus; `platz` s. FADE_PLAETZE.
fn crosshair_color(platz: u32) -> vec3<f32> {
    if (settings.mode == 1u) {
        return unpack4x8unorm(settings.color).rgb;
    }
    // Modus 0 (Standard): Gegenfarbe zum Untergrund. Aufhellen heisst hier: Richtung Weiss
    // mischen -- hebt die Leuchtdichte, wenn die Gegenfarbe von sich aus dunkel ist (reines
    // Blau hat rund 7 % der Leuchtdichte von Weiss).
    let color = mix(CONTRAST_GELB, CONTRAST_BLAU, fade[min(platz, FADE_PLAETZE - 1u)].t);
    return mix(color, vec3<f32>(1.0, 1.0, 1.0), clamp(settings.brightness_boost, 0.0, 1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Texturzugriff vorab und unbedingt: textureSample darf nur in einheitlichem
    // Kontrollfluss stehen.
    let glyph = textureSample(glyph_tex, glyph_sampler, in.glyph_uv).r;
    var coverage = 1.0;
    if (in.kind > 1.5 && in.kind < 4.5) {
        coverage = glyph;
    }
    // Kreise: Deckung aus dem Abstand zur Mitte, mit einem Pixel weicher Kante -- so ist auch
    // ein Punkt von wenigen Pixeln rund statt eckig.
    if (in.kind > 4.5) {
        let d = length(in.glyph_uv);
        coverage = clamp(in.circle.y - d + 0.5, 0.0, 1.0);
        if (in.circle.x > 0.0) {
            coverage = coverage * clamp(d - in.circle.x + 0.5, 0.0, 1.0);
        }
    }
    if (coverage < 0.01) {
        discard;
    }
    let a = coverage * in.alpha;
    // Kontur (1), Glyphen-Kontur (3) und Kreis-Kontur (6): immer schwarz, unabhaengig vom
    // Farbmodus. Sie wird VOR der Fuellung gezeichnet, die deckt die Mitte wieder ab -- uebrig
    // bleibt der Rand.
    if (abs(in.kind - 1.0) < 0.5 || abs(in.kind - 3.0) < 0.5 || abs(in.kind - 6.0) < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, a);
    }
    if (abs(in.kind - 4.0) < 0.5) {
        return vec4<f32>(1.0, 1.0, 1.0, a);
    }
    let feuer = unpack4x8unorm(settings.fire);
    if (feuer.a > 0.5 && (in.kind < 0.5 || abs(in.kind - 5.0) < 0.5)) {
        return vec4<f32>(feuer.rgb, a);
    }
    // Glyphen tragen ihren Farbplatz in circle.x (s. CrosshairVertex::circle).
    let platz = select(0u, u32(in.circle.x), abs(in.kind - 2.0) < 0.5);
    return vec4<f32>(crosshair_color(platz), a);
}
