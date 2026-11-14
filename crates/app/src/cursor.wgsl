// Mauszeiger als analytisch kantengeglaettete Form (statt harter Dreiecke).
//
// Warum kein Dreiecks-Fuellen mehr: die Kanten der Form laufen schraeg zum Pixelraster.
// Ohne MSAA fuellt die Rasterisierung ein Pixel entweder ganz oder gar nicht -- die Schraegkanten wurden zur Treppe, und weil der
// Zeiger sich staendig bewegt, flimmerte diese Treppe bei jeder Bewegung.
//
// Stattdessen: ein einziges Quad ueber der Bounding-Box, und je Pixel wird der vorzeichen-
// behaftete Abstand (SDF) zur Form ausgerechnet. Die Deckung eines Pixels ist dann
// clamp(0.5 - d, 0, 1) -- fuer gerade Kanten ist das exakt der Flaechenanteil, den die Kante
// vom Pixel abdeckt. Kostet einen Bruchteil von MSAA (kein groesserer Framebuffer, kein
// Resolve-Durchgang) und glaettet unabhaengig davon, ob der Rest des Bildes geglaettet wird.

struct CursorUniform {
    // Die vier Punkte des Pfeils (Spitze, rechter Fluegel, Kerbe, unterer Fluegel) in
    // Bildschirm-Pixeln. Je zwei in einem vec4, weil Uniform-Arrays 16-Byte-Schrittweite
    // verlangen.
    points: array<vec4<f32>, 2>,
    // xy = Mitte des Rings, zw = Versatz des Schattens (Pixel).
    ring: vec4<f32>,
    // [Kontur, Inneres] -- a ist die Deckkraft.
    colors: array<vec4<f32>, 2>,
    // Akzentfarbe beim Druecken: rgb = Farbe, a = wie weit das Innere dorthin gemischt wird.
    additive: vec4<f32>,
    // Zeichenflaeche (x0, y0, x1, y1) in Pixeln -- Bounding-Box plus Rand fuer die Glaettung.
    bounds: vec4<f32>,
    // Bildschirmgroesse in Pixeln (xy), z = Konturdicke, w = Aussenradius des Rings (Pixel).
    screen: vec4<f32>,
    // x = wie weit die Form ueber die vier Punkte hinausreicht (runde Ecken), y = Deckkraft
    // des Schattens.
    form: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: CursorUniform;

const PFEIL_N: u32 = 4u;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

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

// Vorzeichenbehafteter Abstand zum Viereck der vier Punkte (negativ = innen).
// Standardverfahren (Inigo Quilez, sdPolygon): kuerzester Abstand zu allen Kanten, und ein
// Vorzeichen aus dem Kreuzungszaehler -- funktioniert auch fuer die Form mit Kerbe, bei der
// eine einfache "innerhalb aller Halbebenen"-Pruefung falsch waere.
fn sd_pfeil(p: vec2<f32>) -> f32 {
    var d = dot(p - pt(0u), p - pt(0u));
    var s = 1.0;
    for (var i = 0u; i < PFEIL_N; i = i + 1u) {
        let vi = pt(i);
        let vj = pt((i + PFEIL_N - 1u) % PFEIL_N);
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

// Abstand zur ganzen Form (negativ = innen): der Pfeil, ringsum um form.x erweitert (das
// rundet die Ecken aussen), und die Scheibe des Rings.
fn sd_form(p: vec2<f32>) -> f32 {
    return min(sd_pfeil(p) - u.form.x, length(p - u.ring.xy) - u.screen.w);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.clip_position.xy;
    // Ein Abstand d zur Form ergibt beides: die Deckung am Aussenrand und die Grenze zwischen
    // Kontur und Innerem bei d = -Konturdicke. So ist die Kontur ueberall gleich dick und an
    // beiden Kanten geglaettet; beim Ring ist das Innere sein Loch.
    let d = sd_form(p);
    let deckung = clamp(0.5 - d, 0.0, 1.0);
    let innen_anteil = clamp(-d - u.screen.z + 0.5, 0.0, 1.0);
    let innen = vec4<f32>(mix(u.colors[1].rgb, u.additive.rgb, u.additive.a), u.colors[1].a);
    // Mit der Deckkraft vormultipliziert mischen, sonst faerbte das fast durchsichtige Innere
    // den Uebergang zur Kontur dunkel.
    let form_a = mix(u.colors[0].a, innen.a, innen_anteil) * deckung;
    let form_rgb = mix(u.colors[0].rgb * u.colors[0].a, innen.rgb * innen.a, innen_anteil) * deckung;
    // Der Schatten: dieselbe Form, versetzt, schwarz -- nur dort, wo die Form selbst nicht ist.
    let schatten_a = clamp(0.5 - sd_form(p - u.ring.zw), 0.0, 1.0) * u.form.y * (1.0 - deckung);
    let alpha = form_a + schatten_a;
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(form_rgb / alpha, alpha);
}
