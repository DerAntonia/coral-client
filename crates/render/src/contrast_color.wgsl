// Gegenfarbe zum Untergrund fuer Fadenkreuz und Blockumriss (Modus Komplementaer). Wird an
// crosshair.wgsl (app) und block.wgsl angehaengt, s. render::CONTRAST_COLOR_WGSL.
//
// Die kraeftige Farbe (HSV-Saettigung und -Helligkeit voll), die im OKLab-Raum am weitesten
// vom Untergrund weg ist -- mit doppelt gezaehltem Helligkeitsunterschied: duenne Linien
// erkennt das Auge vor allem am Hell-Dunkel-Unterschied, ein gleich heller Farbton
// verschwimmt. Mit diesem Gewicht gewinnen ueber den ganzen Farbraum nur zwei: Gelb (hellste
// kraeftige Farbe) und Blau (dunkelste). Das Kreuz ist dadurch ueberall mindestens 2,2-mal
// heller oder dunkler als der Untergrund.
//
// Ein- und Ausgabe sind linear (sRGB-Ansicht beim Lesen, sRGB-Ziel beim Schreiben).

// OKLab aus linearem RGB.
fn contrast_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
    let lms = pow(max(vec3<f32>(l, m, s), vec3<f32>(0.0)), vec3<f32>(1.0 / 3.0));
    return vec3<f32>(
        0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
        1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
        0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z,
    );
}

// OKLab-Abstand mit doppelt gezaehlter Helligkeit (L).
fn contrast_distance(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let d = a - b;
    return length(vec3<f32>(2.0 * d.x, d.y, d.z));
}

const CONTRAST_GELB: vec3<f32> = vec3<f32>(1.0, 1.0, 0.0);
const CONTRAST_BLAU: vec3<f32> = vec3<f32>(0.0, 0.0, 1.0);

// Abstand von Gelb (x) und Blau (y) zu diesem Untergrund. Summiert ueber mehrere Pixel ergibt
// das eine Farbe fuer eine ganze Flaeche (s. crosshair.wgsl).
fn contrast_scores(underneath: vec3<f32>) -> vec2<f32> {
    let bg = contrast_oklab(underneath);
    return vec2<f32>(contrast_distance(contrast_oklab(CONTRAST_GELB), bg), contrast_distance(contrast_oklab(CONTRAST_BLAU), bg));
}

fn contrast_pick(scores: vec2<f32>) -> vec3<f32> {
    return select(CONTRAST_BLAU, CONTRAST_GELB, scores.x >= scores.y);
}

fn contrast_color(underneath: vec3<f32>) -> vec3<f32> {
    return contrast_pick(contrast_scores(underneath));
}
