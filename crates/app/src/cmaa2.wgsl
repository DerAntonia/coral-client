// CMAA2 (Conservative Morphological Anti-Aliasing 2) -- inspiriert von Intels
// Referenzimplementierung (https://github.com/GameTechDev/CMAA2, Apache-2.0-Lizenz), Autor
// Filip Strugar. KEINE 1:1-Portierung: Intels Original scattert Blend-Beitraege ueber eine
// per InterlockedExchange aufgebaute verkettete Liste (RWByteAddressBuffer,
// g_workingDeferredBlendItemListHeads) und aufloest sie in einem eigenen Deferred-Apply-Pass
// -- das braucht Buffer-Atomics ueber Workgroup-Grenzen hinweg, ein deutlich groesseres,
// riskanteres Stueck Arbeit als dieser erste funktionierende Durchgang.
//
// Dieser Durchgang uebernimmt stattdessen Intels KANTENERKENNUNG (Luma-Differenz zu den 4
// Nachbarn, lokal kontrastadaptiver Schwellwert -- exakt dieselbe Formel wie in
// EdgesColor2x2CS/g_CMAA2_EdgeThreshold), blendet aber als GATHER statt SCATTER: jeder Pixel
// liest nur seine eigenen Nachbarn und schreibt nur sich selbst. Keine Ueberschneidungen,
// keine Atomics noetig, dafuer noch keine echte Z-Form-/Simple-Shape-Erkennung (das kommt in
// einer spaeteren Stufe). Ergebnis: ein echter, sichtbarer Kantenglaettungs-Effekt, der
// gerade Linien/Diagonalen entschaerft, ohne flache Flaechen anzufassen (dort ist
// local_max_contrast unter dem Schwellwert, blend bleibt 0).
@group(0) @binding(0) var in_color: texture_2d<f32>;
@group(0) @binding(1) var out_color: texture_storage_2d<rgba8unorm, write>;

// HIGH-Preset aus Intels g_CMAA2_EdgeThreshold (0.15/0.10/0.07/0.05 fuer LOW/MEDIUM/HIGH/
// ULTRA) -- Qualitaetsstufen als echte Einstellung folgen in einer spaeteren Stufe, bis dahin
// fest auf HIGH.
const EDGE_THRESHOLD: f32 = 0.07;
// Intels g_CMAA2_LocalContrastAdaptationAmount: der Schwellwert steigt mit dem lokalen
// Kontrast mit, damit von Natur aus kontrastreiche Bereiche (Text, harte Schatten) nicht
// uebermaessig weichgezeichnet werden.
const LOCAL_CONTRAST_ADAPTATION_AMOUNT: f32 = 0.10;
// Deckelt den Blend-Anteil selbst bei sehr hohem Kontrast -- "conservative" in CMAA2 heisst
// gerade, dass echte Kanten sichtbar bleiben statt wegzuverschwimmen.
const MAX_BLEND: f32 = 0.5;

// Rec.709-Luma-Gewichte, wie Intels LumaMSTexture/CalcLuma.
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn load_clamped(p: vec2<i32>, dims: vec2<i32>) -> vec4<f32> {
    let cp = clamp(p, vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    return textureLoad(in_color, cp, 0);
}

@compute @workgroup_size(8, 8, 1)
fn cmaa2_cs(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = vec2<i32>(textureDimensions(in_color));
    let xy = vec2<i32>(id.xy);
    if (xy.x >= dims.x || xy.y >= dims.y) {
        return;
    }

    let center = load_clamped(xy, dims);
    let left = load_clamped(xy + vec2<i32>(-1, 0), dims);
    let right = load_clamped(xy + vec2<i32>(1, 0), dims);
    let top = load_clamped(xy + vec2<i32>(0, -1), dims);
    let bottom = load_clamped(xy + vec2<i32>(0, 1), dims);

    let lc = luma(center.rgb);
    let d_left = abs(lc - luma(left.rgb));
    let d_right = abs(lc - luma(right.rgb));
    let d_top = abs(lc - luma(top.rgb));
    let d_bottom = abs(lc - luma(bottom.rgb));

    let local_max_contrast = max(max(d_left, d_right), max(d_top, d_bottom));
    let threshold = max(EDGE_THRESHOLD, LOCAL_CONTRAST_ADAPTATION_AMOUNT * local_max_contrast);

    var out = center;
    if (local_max_contrast > threshold) {
        // Weicher Uebergang statt Alles-oder-Nichts -- sonst wuerde jeder Pixel knapp ueber
        // der Schwelle genauso stark geglaettet wie einer weit darueber.
        let blend = clamp((local_max_contrast - threshold) / local_max_contrast, 0.0, MAX_BLEND);
        let neighbor_avg = (left + right + top + bottom) * 0.25;
        out = mix(center, neighbor_avg, blend);
    }

    textureStore(out_color, xy, out);
}
