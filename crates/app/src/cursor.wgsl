// Mauszeiger als analytisch kantengeglaettete Form (statt harter Dreiecke).
//
// Warum kein Dreiecks-Fuellen mehr: die Kite-Form ist um -25 Grad gedreht, ihre Kanten
// laufen also nie entlang der Pixelraster-Achsen. Ohne MSAA fuellt die Rasterisierung ein
// Pixel entweder ganz oder gar nicht -- die Schraegkanten wurden zur Treppe, und weil der
// Zeiger sich staendig bewegt, flimmerte diese Treppe bei jeder Bewegung.
//
// Stattdessen: ein einziges Quad ueber der Bounding-Box, und je Pixel wird der vorzeichen-
// behaftete Abstand (SDF) zur Form ausgerechnet. Die Deckung eines Pixels ist dann
// clamp(0.5 - d, 0, 1) -- fuer gerade Kanten ist das exakt der Flaechenanteil, den die Kante
// vom Pixel abdeckt. Kostet einen Bruchteil von MSAA (kein groesserer Framebuffer, kein
// Resolve-Durchgang) und glaettet unabhaengig davon, ob der Rest des Bildes geglaettet wird.

struct CursorUniform {
    // KITE_N Umrisspunkte des Pfeils + 2 Enden der Griff-Kapsel in Bildschirm-Pixeln. Je zwei
    // Punkte in einem vec4, weil Uniform-Arrays 16-Byte-Schrittweite verlangen.
    points: array<vec4<f32>, 9>,
    // [Kontur, Kern].
    colors: array<vec4<f32>, 2>,
    // Akzentfarbe beim Druecken: rgb = Farbe, a = wie weit der Kern dorthin gemischt wird.
    additive: vec4<f32>,
    // Zeichenflaeche (x0, y0, x1, y1) in Pixeln -- Bounding-Box plus Rand fuer die Glaettung.
    bounds: vec4<f32>,
    // Bildschirmgroesse in Pixeln (xy), z = Konturdicke, w = Kapselradius (Pixel).
    screen: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: CursorUniform;

// Punkte des Pfeil-Umrisses (s. CURSOR_KITE_POINTS in main.rs): Spitze, Bogen, Kerbe, Bogen.
const KITE_N: u32 = 16u;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// Punkt i (0..KITE_N+1) aus dem gepackten Array.
fn pt(i: u32) -> vec2<f32> {
    let v = u.points[i >> 1u];
    if ((i & 1u) == 0u) {
        return v.xy;
    }
    return v.zw;
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // Zwei Dreiecke ueber der Zeichenflaeche, ohne Vertex-Puffer.
    // `var`, nicht `let`: nur eine Variable im Speicher darf mit einem zur Uebersetzungs-
    // zeit unbekannten Index (hier vertex_index) gelesen werden.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let corner = corners[index];
    let px = mix(u.bounds.xy, u.bounds.zw, corner);
    var out: VertexOutput;
    out.clip_position = vec4<f32>((px.x / u.screen.x) * 2.0 - 1.0, 1.0 - (px.y / u.screen.y) * 2.0, 0.0, 1.0);
    return out;
}

// Vorzeichenbehafteter Abstand zum Pfeil-Umriss (negativ = innen).
// Standardverfahren (Inigo Quilez, sdPolygon): kuerzester Abstand zu allen Kanten, und ein
// Vorzeichen aus dem Kreuzungszaehler -- funktioniert auch fuer die konkave Kite-Form, bei
// der eine einfache "innerhalb aller Halbebenen"-Pruefung falsch waere.
fn sd_kite(p: vec2<f32>) -> f32 {
    var d = dot(p - pt(0u), p - pt(0u));
    var s = 1.0;
    for (var i = 0u; i < KITE_N; i = i + 1u) {
        let vi = pt(i);
        let vj = pt((i + KITE_N - 1u) % KITE_N);
        let e = vj - vi;
        let w = p - vi;
        let b = w - e * clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c = vec3<bool>(p.y >= vi.y, p.y < vj.y, e.x * w.y > e.y * w.x);
        if (all(c) || !any(c)) {
            s = -s;
        }
    }
    return s * sqrt(d);
}

// Abstand zur Kapsel (Strecke a-b mit Radius r) -- runde Enden statt Ecken.
fn sd_capsule(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.clip_position.xy;
    // Ein Abstand d zur Form (negativ = innen) ergibt beides: Deckung der Form am Aussenrand
    // und die Grenze zwischen Kontur und Kern bei d = -Konturdicke. So ist die Kontur
    // ueberall gleich dick und an beiden Kanten geglaettet.
    let d = min(sd_kite(p), sd_capsule(p, pt(KITE_N), pt(KITE_N + 1u), u.screen.w));
    let alpha = clamp(0.5 - d, 0.0, 1.0);
    if (alpha <= 0.0) {
        discard;
    }
    let kern_anteil = clamp(-d - u.screen.z + 0.5, 0.0, 1.0);
    let kern = mix(u.colors[1].rgb, u.additive.rgb, u.additive.a);
    let rgb = mix(u.colors[0].rgb, kern, kern_anteil);
    return vec4<f32>(rgb, alpha);
}
