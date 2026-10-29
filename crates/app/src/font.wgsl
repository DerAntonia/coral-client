struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // Atlas-Rechteck des Glyphen (u0, v0, u1, v1) -- ausserhalb davon zaehlt nichts.
    @location(2) bounds: vec4<f32>,
    // x = Art (0 Glyph, 1 Kontur, 2 Vanilla-Schatten, 3 weich unten, 4 weich rundherum; +8 =
    // farbiges Bild aus dem Pack-Glyphenblatt), yz = UV je Bildschirmpixel, w = Schattenversatz
    // bzw. Unschaerfe in Bildschirmpixeln, bei der Kontur ihre Dicke.
    @location(3) mode: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) bounds: vec4<f32>,
    @location(4) mode: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    out.bounds = bounds;
    out.mode = mode;
    return out;
}

// Weltschrift mit Tiefentest (Schilder, s. App::build_sign_texts): auf der CPU projiziert wie
// die Text-Displays, die Tiefe (NDC) steht in mode.w -- Glyphen der Art 0 lesen mode.w nicht.
@vertex
fn vs_world(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) bounds: vec4<f32>,
    @location(4) mode: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(position, mode.w, 1.0);
    out.uv = uv;
    out.color = color;
    out.bounds = bounds;
    out.mode = vec4<f32>(mode.xyz, 0.0);
    return out;
}

@group(0) @binding(0) var atlas_texture: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
// Glyphenblatt der Resourcepack-Schriften: farbige Pixelbilder, ungefiltert gelesen.
@group(0) @binding(2) var pack_texture: texture_2d<f32>;

// Ein Texel des Glyphenblatts (naechster Nachbar), ausserhalb [0,1] durchsichtig.
fn pack_texel(p: vec2<f32>) -> vec4<f32> {
    let groesse = vec2<f32>(textureDimensions(pack_texture));
    let t = vec2<i32>(floor(p * groesse));
    let innen = all(p >= vec2<f32>(0.0)) && all(p < vec2<f32>(1.0));
    return select(vec4<f32>(0.0), textureLoad(pack_texture, clamp(t, vec2<i32>(0), vec2<i32>(groesse) - 1), 0), innen);
}

// Deckung des EIGENEN Glyphen an Stelle p -- ausserhalb seines Rechtecks 0, damit Kontur und
// Schatten nicht in den Nachbarn im Atlas greifen.
fn coverage(p: vec2<f32>, b: vec4<f32>, pack: bool) -> f32 {
    let innen = p.x >= b.x && p.y >= b.y && p.x <= b.z && p.y <= b.w;
    let c = select(textureSampleLevel(atlas_texture, atlas_sampler, p, 0.0).r, pack_texel(p).a, pack);
    return select(0.0, c, innen);
}

// Deckung eines Texels des eigenen Glyphen; ausserhalb seines Rechtecks 0.
fn texel_deckung(t: vec2<i32>, groesse: vec2<f32>, b: vec4<f32>, pack: bool) -> f32 {
    let mitte = (vec2<f32>(t) + 0.5) / groesse;
    let innen = mitte.x >= b.x && mitte.y >= b.y && mitte.x <= b.z && mitte.y <= b.w;
    let tc = clamp(t, vec2<i32>(0), vec2<i32>(groesse) - 1);
    let c = select(textureLoad(atlas_texture, tc, 0).r, textureLoad(pack_texture, tc, 0).a, pack);
    return select(0.0, c, innen);
}

// Wie coverage, aber immer bilinear zwischen den Texeln -- fuer den weichen Schatten. Die
// Minecraft-Schrift liegt Pixel fuer Pixel im Atlas und wird mit Nearest gelesen: jede Probe
// des Schattens waere eine harte Stufe, und weil die Proben etwa ein Schriftpixel auseinander
// liegen, sah der Schatten treppig aus statt weich.
fn coverage_weich(p: vec2<f32>, b: vec4<f32>, pack: bool) -> f32 {
    let groesse = select(vec2<f32>(textureDimensions(atlas_texture)), vec2<f32>(textureDimensions(pack_texture)), pack);
    let q = p * groesse - 0.5;
    let t = vec2<i32>(floor(q));
    let f = q - floor(q);
    let oben = mix(texel_deckung(t, groesse, b, pack), texel_deckung(t + vec2<i32>(1, 0), groesse, b, pack), f.x);
    let unten = mix(texel_deckung(t + vec2<i32>(0, 1), groesse, b, pack), texel_deckung(t + vec2<i32>(1, 1), groesse, b, pack), f.x);
    return mix(oben, unten, f.y);
}

// Die Oberflaeche ist sRGB, gemischt wird also linear: eine halb deckende Kante dunkler
// Schrift auf hellem Grund (Akzent-Knoepfe) wird so nur gut ein Viertel dunkler statt halb,
// die Schrift wirkt vom Grund aufgefressen. Fuer schwarze Schrift ergibt 1 - (1 - a)^2,2 genau
// die Mischung im sRGB-Raum, unabhaengig vom Grund; zu hellerer Schrift hin laeuft der
// Ausgleich aus. Helle Schrift bleibt wie sie ist.
fn dunkel_deckung(a: f32, farbe: vec3<f32>) -> f32 {
    let dunkel = clamp(1.0 - max(max(farbe.r, farbe.g), farbe.b) / 0.2, 0.0, 1.0);
    return 1.0 - pow(1.0 - a, 1.0 + 1.2 * dunkel);
}

// Kontur und Schatten sind je EIN Viereck, dessen Deckung hier ausgerechnet wird -- ohne den
// Glyphen selbst. Jeder Pixel bekommt so genau eine Lage (Rand ODER Glyph) mit der Deckkraft
// des Textes. Frueher waren es acht verschobene Kopien, die sich beim Ausblenden zu fast
// deckendem Schwarz stapelten: der Text wurde grau statt durchsichtig.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let glyph = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    let pack = in.mode.x > 7.5;
    let art = in.mode.x - select(0.0, 8.0, pack);
    if art < 0.5 {
        if pack {
            // Wie Vanilla: Bildfarbe mal Textfarbe -- weisser Text zeigt das Bild unveraendert.
            return pack_texel(in.uv) * in.color;
        }
        return vec4<f32>(in.color.rgb, in.color.a * dunkel_deckung(glyph, in.color.rgb));
    }
    let px = in.mode.yz;
    let selbst = coverage(in.uv, in.bounds, pack);
    var rand = 0.0;
    if art < 1.5 {
        // Kontur, w = Dicke in Bildschirmpixeln: jeder Nachbar bis dorthin, der Rand der Dicke
        // anteilig. Der Abstand ist fast der euklidische, nur die Diagonale zum direkten
        // Nachbarn zaehlt noch ganz -- bei Dicke 1 sind es so genau die acht Nachbarn.
        let dicke = in.mode.w;
        let r = i32(ceil(dicke));
        for (var dy = -r; dy <= r; dy++) {
            for (var dx = -r; dx <= r; dx++) {
                if dx != 0 || dy != 0 {
                    let d = vec2<f32>(f32(dx), f32(dy));
                    let abstand = max(max(abs(d.x), abs(d.y)), length(d) - 0.42);
                    let w = clamp(dicke + 1.0 - abstand, 0.0, 1.0);
                    if w > 0.0 {
                        rand = max(rand, w * coverage(in.uv + d * px, in.bounds, pack));
                    }
                }
            }
        }
    } else if art < 2.5 {
        rand = coverage(in.uv - vec2<f32>(in.mode.w, in.mode.w) * px, in.bounds, pack);
    } else {
        // Weicher Schatten unten (wie in osu!): die Deckung um den nach oben versetzten Punkt
        // gemittelt, 5x5 Proben ueber die Unschaerfe-Breite verteilt, und deutlich blasser.
        // Rundherum (Art 4) dasselbe ohne Versatz. Der Glyph selbst liegt darueber, deshalb
        // hier ohne (1 - selbst).
        let versatz = select(in.mode.w, 0.0, art > 3.5);
        let mitte = in.uv - vec2<f32>(0.0, versatz) * px;
        var summe = 0.0;
        var gewicht = 0.0;
        for (var dy = -2; dy <= 2; dy++) {
            for (var dx = -2; dx <= 2; dx++) {
                let w = 3.0 - 0.5 * f32(abs(dx) + abs(dy));
                summe += w * coverage_weich(mitte + vec2<f32>(f32(dx), f32(dy)) * px * (in.mode.w * 0.5), in.bounds, pack);
                gewicht += w;
            }
        }
        // Die Staerke darf hier ueber 1 liegen (der Regler reicht weiter als die fruehere volle
        // Staerke), die Deckung nicht.
        return vec4<f32>(in.color.rgb, min(in.color.a * 0.6 * summe / gewicht, 1.0));
    }
    // Zusammen mit dem Glyphen darueber soll der Pixel genau max(Rand, Glyph) decken (mal der
    // Deckkraft des Textes). Frueher stand hier Rand * (1 - Glyph): an halb gedeckten Kanten
    // schien dann der Grund durch beide Lagen -- im Inventar die Item-Pixel durch die Zahl,
    // die oben ohne Kontur und an Ecken wie angebissen aussah.
    let ziel = in.color.a * max(rand, selbst);
    let glyph_deckt = in.color.a * selbst;
    return vec4<f32>(in.color.rgb, max(ziel - glyph_deckt, 0.0) / max(1.0 - glyph_deckt, 1e-4));
}
