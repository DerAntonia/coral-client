// Verlaufshimmel fuer den Leistungs-Pfad (Modul Himmel, Art "Verlauf"): der Himmel des
// fruehen Shader-Pfads -- Verlauf vom Zenit zum Horizont aus der Himmelsfarbe der Tageszeit,
// blaue Stunde nach Sonnenuntergang und vor Sonnenaufgang, Daemmerungsglut auf der
// Sonnenseite, eckige Sonne mit Hof und Mond auf der geneigten Sonnenbahn. Ohne HDR: was ueber
// 1 hinausgeht, schneidet das Ziel ab.

struct Params {
    // Bildschirm -> Welt-Richtung (Projektion mal reine Blickdrehung, invertiert).
    inv_view_proj: mat4x4<f32>,
    // Himmelsfarbe der Tageszeit, linear; w = wie klar es ist (1 = kein Regen).
    sky: vec4<f32>,
    // Richtung zur Sonne (Welt), auch nachts; w = 1, wenn die Sonne gezeichnet wird.
    sun_dir: vec4<f32>,
    // Senkrechte auf der Sonnenbahn; w = 1, wenn der Mond gezeichnet wird.
    sun_axis: vec4<f32>,
    // x = 1, wenn Sterne gezeichnet werden.
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;

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

// Oben tiefer und satter, zum Horizont heller und blasser, darunter wieder dunkler. Steht die
// Sonne tief, gluehen Horizont und Gegend um die Sonne orange-rot.
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
    // Blaue Stunde: steht die Sonne knapp unter dem Horizont, ist der Himmel tiefblau statt
    // schon schwarz -- oben dunkel, zum Horizont heller. Bei der Minecraft-Sonnenbahn faellt
    // das auf etwa 19:30 bis 21:30 (morgens spiegelbildlich 2:30 bis 4:30); Regen nimmt sie weg.
    let unter = -sonne.y;
    let blau_stunde = smoothstep(0.02, 0.2, unter) * (1.0 - smoothstep(0.45, 0.72, unter)) * params.sky.w;
    if (blau_stunde > 0.0) {
        let blau = mix(vec3<f32>(0.008, 0.022, 0.15), vec3<f32>(0.06, 0.12, 0.38), pow(1.0 - clamp(h, 0.0, 1.0), 3.0));
        let unten = mix(blau, vec3<f32>(0.01, 0.02, 0.07), smoothstep(0.0, 0.35, -h));
        farbe = mix(farbe, select(blau, unten, h < 0.0), blau_stunde);
    }
    let daemmerung = 1.0 - smoothstep(0.0, 0.35, abs(sonne.y + 0.02));
    if (daemmerung > 0.0) {
        // Sonnenseite ueber die volle Blickrichtung, nicht nur die waagrechte: zum Zenit hin
        // geht sie gleichmaessig gegen die Mitte. Normiert man die waagrechte Blickrichtung,
        // laufen dort alle Himmelsrichtungen in einem Punkt zusammen und die Glut zieht einen
        // Strahl vom Zenit zur Sonne.
        let seite = dot(blick, vec3<f32>(normalize(vec2<f32>(sonne.x, sonne.z) + vec2<f32>(1e-5)), 0.0).xzy) * 0.5 + 0.5;
        let hoehe = exp(-max(h, 0.0) * mix(9.0, 3.5, seite)) * smoothstep(-0.25, 0.0, h);
        let glut = mix(vec3<f32>(0.9, 0.35, 0.12), vec3<f32>(1.0, 0.6, 0.25), seite);
        farbe = mix(farbe, glut * (0.25 + 0.75 * seite), daemmerung * hoehe * (0.35 + 0.65 * seite));
    }
    return farbe;
}

// Eine Zufallszahl 0..1 je Gitterzelle.
fn stern_hash(p: vec3<i32>) -> f32 {
    var h = u32(p.x) * 73856093u ^ u32(p.y) * 19349663u ^ u32(p.z) * 83492791u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xFFFFu) / 65535.0;
}

// Sterne wie im Shader-Pfad (deferred_light.wgsl): kleine Quadrate wie in Vanilla, fest am
// Himmel, der sich mit Sonne und Mond dreht. `d` ist die Blickrichtung in diesem mitdrehenden
// Rahmen. Der Himmel wird als Wuerfel aufgeteilt, je Seite STERN_RASTER^2 Felder; in etwa
// jedem dreissigsten steht ein Stern mit eigener Lage, Groesse und Helligkeit.
const STERN_RASTER: f32 = 90.0;

fn sterne(d: vec3<f32>) -> f32 {
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
    if (stern_hash(schluessel) > 0.033) {
        return 0.0;
    }
    // Lage im Feld (mit Rand, damit der Stern nicht ans Nachbarfeld stoesst), Groesse und
    // Helligkeit aus weiteren Hashes.
    let mitte = vec2<f32>(feld) + vec2<f32>(0.25) + 0.5 * vec2<f32>(stern_hash(schluessel + vec3<i32>(1, 0, 0)), stern_hash(schluessel + vec3<i32>(0, 1, 0)));
    let halb = 0.06 + 0.06 * stern_hash(schluessel + vec3<i32>(0, 0, 1));
    let abstand = max(abs(g.x - mitte.x), abs(g.y - mitte.y));
    // Weicher Rand von etwa einem Pixel (ein Feld ist rund 16 Pixel breit).
    let deckung = 1.0 - smoothstep(halb - 0.04, halb + 0.04, abstand);
    return deckung * (0.5 + 0.5 * stern_hash(schluessel + vec3<i32>(1, 1, 1)));
}

// Deckkraft der eckigen Scheibe bei Abstand `d` von der Mitte (halbe Kantenlaenge `r`): das
// innere Zwei-Drittel voll, das aeussere Drittel als Rand zu 60 %.
fn scheibe(d: f32, r: f32) -> f32 {
    if (d >= r) {
        return 0.0;
    }
    return select(0.6, 1.0, d < r * (2.0 / 3.0));
}

@fragment
fn fs_sky(in: VsOut) -> @location(0) vec4<f32> {
    let p0 = params.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let p1 = params.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let blick = normalize(p1.xyz / p1.w - p0.xyz / p0.w);
    let sonne = params.sun_dir.xyz;
    let achse = params.sun_axis.xyz;
    let quer = cross(achse, sonne);
    var farbe = sky_gradient(blick, sonne);
    // Sterne wie in Vanilla nach dem Sonnenstand: ab der Daemmerung sichtbar, nachts voll.
    // Ueber dem Horizont, zum Horizont hin schwaecher (dickere Luft), bei Regen weg.
    // Sonnenwinkel aus der Hoehe: sonne.y = cos(Winkel) * cos(Neigung), und die Achse der
    // Sonnenbahn hat z = cos(Neigung) (s. shader_path::sun_path_axis).
    let cos_winkel = sonne.y / max(achse.z, 1e-4);
    let nacht = clamp(1.0 - (cos_winkel * 2.0 + 0.25), 0.0, 1.0) * params.sky.w * params.extra.x;
    if (nacht > 0.0 && blick.y > 0.0) {
        let himmel = vec3<f32>(dot(blick, sonne), dot(blick, quer), dot(blick, achse));
        farbe += vec3<f32>(0.9, 0.92, 1.0) * sterne(himmel) * nacht * smoothstep(0.0, 0.25, blick.y);
    }
    let s = dot(blick, sonne);
    if (s > 0.0 && params.sun_dir.w > 0.5) {
        let q = abs(vec2<f32>(dot(blick, quer), dot(blick, achse))) / s;
        let auf = smoothstep(-0.1, 0.05, sonne.y);
        // Enger heller Hof plus ein schwacher weiter.
        let schein = (pow(max(s, 0.0), 600.0) * 1.2 + pow(max(s, 0.0), 12.0) * 0.12) * auf;
        farbe += mix(vec3<f32>(1.0, 0.5, 0.2), vec3<f32>(1.0, 0.9, 0.7), smoothstep(0.0, 0.4, sonne.y)) * schein;
        farbe = mix(farbe, vec3<f32>(1.0, 0.95, 0.8), auf * scheibe(max(q.x, q.y), 0.075));
    } else if (s < 0.0 && params.sun_axis.w > 0.5) {
        let mond = -sonne;
        let q = abs(vec2<f32>(dot(blick, quer), dot(blick, achse))) / -s;
        let auf = smoothstep(-0.1, 0.05, mond.y);
        farbe = mix(farbe, vec3<f32>(0.85, 0.88, 0.95), auf * scheibe(max(q.x, q.y), 0.05));
    }
    return vec4<f32>(clamp(farbe, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
