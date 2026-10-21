// Zeigt die am staerksten unterscheidbare Farbe zum Pixel darunter: Farbton auf die
// gegenueberliegende Seite des Farbkreises (+180 Grad), Saettigung UND Helligkeit/Value
// (HSV, nicht HSL) immer maximal -- die kraeftigste, hellste Version der Gegenfarbe, egal
// wie hell/dunkel/gesaettigt der Untergrund ist. Nur der Farbton haengt vom Untergrund ab.
// Bewusst ohne den "%"-Operator (reine Additionen/Subtraktionen/Vergleiche) -- unabhaengig
// vom eigentlichen Bug (s. Git-Historie) eine robuste, leicht nachvollziehbare Umsetzung.
@group(0) @binding(0) var sample_tex: texture_2d<f32>;
@group(0) @binding(1) var sample_sampler: sampler;

// Farbmodus, im HUD-Editor waehlbar (s. CROSSHAIR_COLOR_MODES in main.rs): 0 = Komplementaer
// (Standard), 1 = Einfarbig in `color` -- fuer Faelle, in denen die automatische Gegenfarbe
// eher stoert (z.B. sehr bewegte/bunte Untergruende).
//
// `brightness_boost` (0..1) hellt NUR die Komplementaerfarbe auf, s. fs_main. `color` ist
// RGBA8, R im niedrigsten Byte. Die Struktur ist auf 16 Byte aufgefuellt: Uniform-Bloecke
// muessen in WGSL ein Vielfaches von 16 gross sein.
struct CrosshairSettings {
    mode: u32,
    brightness_boost: f32,
    color: u32,
    // Brenn-Farbe (Feuer-Modus "Fadenkreuz"), nur fuer Kreuz (0) und Kreis/Ring (5) -- Zahlen
    // behalten `color`. Alpha-Byte 0 = brennt nicht.
    fire: u32,
};
@group(0) @binding(2) var<uniform> settings: CrosshairSettings;

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

// Farbton in Grad [0,360).
fn rgb_to_hue(c: vec3<f32>) -> f32 {
    let maxc = max(c.r, max(c.g, c.b));
    let minc = min(c.r, min(c.g, c.b));
    let d = maxc - minc;
    if (d <= 0.0001) {
        return 0.0; // Grau/Schwarz/Weiss -- Farbton undefiniert, 0 (Rot) als fester Fallback.
    }
    var h = 0.0;
    if (maxc == c.r) {
        h = (c.g - c.b) / d; // liegt in (-1,1), die Korrektur unten reicht -- kein Modulo noetig.
    } else if (maxc == c.g) {
        h = (c.b - c.r) / d + 2.0;
    } else {
        h = (c.r - c.g) / d + 4.0;
    }
    h = h * 60.0;
    if (h < 0.0) {
        h = h + 360.0;
    }
    return h;
}

// HSV(h, 1, 1) -> RGB: die reine, voll gesaettigte und voll helle Farbe fuer diesen
// Farbton (Rot, Orange, Gelb, Gruen, Cyan, Blau, Magenta, ... je nach h). `h` muss in
// [0,360) liegen.
fn pure_hue_to_rgb(h: f32) -> vec3<f32> {
    let hp = h / 60.0; // [0,6)
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
    } else {
        return vec3<f32>(1.0, 0.0, 6.0 - hp);
    }
}

// Die Fadenkreuz-Farbe fuer diesen Pixel, je nach Modus.
fn crosshair_color(underneath: vec3<f32>) -> vec3<f32> {
    if (settings.mode == 1u) {
        return unpack4x8unorm(settings.color).rgb;
    }
    // Modus 0 (Standard): Komplementaerfarbe wie bisher.
    let h = rgb_to_hue(underneath);
    var new_h = h + 180.0; // h in [0,360) -> new_h in [180,540)
    if (new_h >= 360.0) {
        new_h = new_h - 360.0;
    }
    // Aufhellen heisst hier: Richtung Weiss mischen, nicht "Value hochdrehen". HSV-Value ist
    // schon auf 1 -- pure_hue_to_rgb liefert die hellste Fassung dieses Farbtons. Trotzdem
    // sind manche reinen Farbtoene von sich aus dunkel (reines Blau hat rund 7 % der
    // Leuchtdichte von Weiss und verschwindet vor dunklem Untergrund). Weiss beimischen
    // senkt die Saettigung und hebt die Leuchtdichte -- der Farbton bleibt dabei exakt
    // erhalten, die Gegenfarbe bleibt also weiterhin die Gegenfarbe.
    let color = pure_hue_to_rgb(new_h);
    return mix(color, vec3<f32>(1.0, 1.0, 1.0), clamp(settings.brightness_boost, 0.0, 1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Beide Texturzugriffe vorab und unbedingt: textureSample darf nur in einheitlichem
    // Kontrollfluss stehen.
    let underneath = textureSample(sample_tex, sample_sampler, in.uv).rgb;
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
    return vec4<f32>(crosshair_color(underneath), a);
}
