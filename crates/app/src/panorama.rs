// Panorama als Hintergrundbild (Einstellungen -> Aussehen): die sechs Seiten panorama_0..5
// eines Skins als Wuerfel um die Kamera, gedreht wie Vanillas Titelbildschirm.
//
// Vanilla zeichnet einen Wuerfel mit Cubemap-Textur (CubeMap.render): Blickwinkel 85 Grad,
// erst um 180 Grad um X gekippt (Bildzeilen laufen nach unten), dann 10 Grad Neigung, dann die
// Drehung um Y. Seite 0 liegt bei z = +1, 1/2/3 um 90/180/-90 Grad um Y weiter, 4 oben, 5 unten
// (so zeichnen es auch aeltere Fassungen Seite fuer Seite).
//
// Hier ohne eigene Pipeline: der Wuerfel wird auf der CPU projiziert und als Dreiecke mit der
// Sprite-Pipeline gezeichnet. Die Seiten liegen nebeneinander in EINER Textur. Weil die Sprite-
// Pipeline UVs linear (nicht perspektivisch) interpoliert, ist jede Seite in ein Raster aus
// kleinen Kacheln geteilt -- in so kleinen Stuecken faellt der Unterschied nicht auf.

use std::path::{Path, PathBuf};

use glam::{Mat3, Vec3};

use crate::wallpaper::Decoded;
use crate::TextVertex;

pub(crate) const FACES: usize = 6;
// Kacheln je Seitenkante.
const GRID: usize = 12;
pub(crate) const MAX_VERTICES: usize = FACES * GRID * GRID * 6;
const FOV_DEG: f32 = 85.0;
const TILT_DEG: f32 = 10.0;
// Vanilla dreht um 0,1 Grad je Tick (Panorama-Geschwindigkeit 1), also 2 Grad je Sekunde. Die
// Einstellung ist in Prozent davon.
pub(crate) const DEG_PER_SEC: f32 = 2.0;
pub(crate) const SPEED_MIN: i32 = -500;
pub(crate) const SPEED_MAX: i32 = 500;
pub(crate) const SPEED_STEP: i32 = 10;
const DIR: &str = "assets/minecraft/textures/gui/title/background";
// Groesster Texturrand (wie wallpaper::MAX_TEXTURE_SIDE): sechs Seiten nebeneinander.
const MAX_SIDE: u32 = 8192;

fn face_path(pack: &Path, i: usize) -> PathBuf {
    pack.join(DIR).join(format!("panorama_{i}.png"))
}

// Seite 0: an ihr erkennt der Loader, ob sich das Panorama geaendert hat.
pub(crate) fn first_face(pack: &Path) -> PathBuf {
    face_path(pack, 0)
}

// Hat das Pack alle sechs Seiten?
pub(crate) fn has_panorama(pack: &Path) -> bool {
    (0..FACES).all(|i| face_path(pack, i).is_file())
}

// Die Skins mit Panorama, alphabetisch.
pub(crate) fn skins_with_panorama() -> Vec<String> {
    let Some(dir) = crate::skins_dir() else { return Vec::new() };
    crate::list_available_skins().into_iter().filter(|n| has_panorama(&dir.join(n))).collect()
}

// Der eingestellte Skin, oder der erste mit Panorama, wenn der eingestellte keins (mehr) hat.
pub(crate) fn chosen_skin(setting: &str) -> Option<String> {
    let liste = skins_with_panorama();
    liste.iter().find(|n| *n == setting).or_else(|| liste.first()).cloned()
}

// Die sechs Seiten nebeneinander in einem Bild (Seite i bei x = i * Seitenbreite). Alle Seiten
// muessen gleich gross sein, wie bei Vanilla (CubeMapTexture).
pub(crate) fn load(pack: &Path) -> Result<Decoded, String> {
    let mut seiten = Vec::with_capacity(FACES);
    for i in 0..FACES {
        let pfad = face_path(pack, i);
        let bild = image::open(&pfad).map_err(|e| format!("{}: {e}", pfad.display()))?.into_rgba8();
        seiten.push(bild);
    }
    let (w, h) = seiten[0].dimensions();
    if let Some((i, s)) = seiten.iter().enumerate().find(|(_, s)| s.dimensions() != (w, h)) {
        return Err(format!("panorama_{i}.png ist {}x{}, panorama_0.png {w}x{h}", s.width(), s.height()));
    }
    let s = (f64::from(MAX_SIDE) / f64::from(w * FACES as u32)).min(f64::from(MAX_SIDE) / f64::from(h)).min(1.0);
    let (tw, th) = (((f64::from(w) * s) as u32).max(1), ((f64::from(h) * s) as u32).max(1));
    let mut atlas = image::RgbaImage::new(tw * FACES as u32, th);
    for (i, seite) in seiten.into_iter().enumerate() {
        let seite = if (tw, th) == (w, h) { seite } else { image::imageops::resize(&seite, tw, th, image::imageops::FilterType::CatmullRom) };
        image::imageops::replace(&mut atlas, &seite, i64::from(i as u32 * tw), 0);
    }
    Ok(Decoded { width: atlas.width(), height: atlas.height(), rgba: atlas.into_raw() })
}

// Lage der Seite i im Wuerfel.
fn face_rotation(i: usize) -> Mat3 {
    match i {
        1 => Mat3::from_rotation_y(90f32.to_radians()),
        2 => Mat3::from_rotation_y(180f32.to_radians()),
        3 => Mat3::from_rotation_y(-90f32.to_radians()),
        4 => Mat3::from_rotation_x(90f32.to_radians()),
        5 => Mat3::from_rotation_x(-90f32.to_radians()),
        _ => Mat3::IDENTITY,
    }
}

// Die Dreiecke des Wuerfels fuer einen Bildschirm der Groesse `screen` (Pixel), um `spin_deg`
// gedreht. `atlas` ist die Groesse der Textur aus `load`.
pub(crate) fn vertices(spin_deg: f32, screen: (f32, f32), atlas: (u32, u32), out: &mut Vec<TextVertex>) {
    let kamera = Mat3::from_rotation_x((180.0 + TILT_DEG).to_radians()) * Mat3::from_rotation_y((-spin_deg).to_radians());
    let f = 1.0 / (FOV_DEG.to_radians() / 2.0).tan();
    let seitenverhaeltnis = screen.0.max(1.0) / screen.1.max(1.0);
    // Einen halben Texel vom Rand der Seite weg: sonst mischt die lineare Filterung am Rand
    // die Nachbarseite im Atlas mit hinein.
    let tile_w = (atlas.0 as f32 / FACES as f32).max(1.0);
    let (rand_u, rand_v) = (0.5 / tile_w, 0.5 / (atlas.1 as f32).max(1.0));
    for seite in 0..FACES {
        let m = kamera * face_rotation(seite);
        // Ecke (a, b) des Rasters: Sichtraum-Punkt, Bildschirm (NDC) und UV.
        let ecke = |a: usize, b: usize| {
            let (lu, lv) = (a as f32 / GRID as f32, b as f32 / GRID as f32);
            let p = m * Vec3::new(lu * 2.0 - 1.0, lv * 2.0 - 1.0, 1.0);
            let ndc = [f / seitenverhaeltnis * p.x / -p.z, f * p.y / -p.z];
            let uv = [(seite as f32 + lu.clamp(rand_u, 1.0 - rand_u)) / FACES as f32, lv.clamp(rand_v, 1.0 - rand_v)];
            (p.z, ndc, uv)
        };
        for b in 0..GRID {
            for a in 0..GRID {
                let ecken = [ecke(a, b), ecke(a + 1, b), ecke(a + 1, b + 1), ecke(a, b + 1)];
                // Hinter (oder fast auf) der Kamera: weglassen. Solche Kacheln liegen weit
                // ausserhalb des Bildes -- auf dem Wuerfel ist dort immer eine Koordinate +-1.
                if ecken.iter().any(|e| e.0 > -0.01) {
                    continue;
                }
                let aussen = |achse: usize, grenze: f32| ecken.iter().all(|e| e.1[achse] * grenze > 1.0);
                if aussen(0, 1.0) || aussen(0, -1.0) || aussen(1, 1.0) || aussen(1, -1.0) {
                    continue;
                }
                let v = |i: usize| TextVertex { position: ecken[i].1, uv: ecken[i].2, color: [1.0; 4], bounds: [0.0; 4], mode: [0.0; 4], depth: 0.0 };
                out.extend_from_slice(&[v(0), v(1), v(2), v(0), v(2), v(3)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Die Seiten muessen ohne Luecke aneinanderstossen: jeder Punkt der Bildmitte liegt in
    // genau einer Seite, und die Kanten benachbarter Seiten landen auf demselben Fleck.
    #[test]
    fn seiten_stossen_aneinander() {
        let kamera = Mat3::from_rotation_x((180.0 + TILT_DEG).to_radians());
        // Seite 0 und 1 teilen die Kante x = +1 von Seite 0 / x = -1 von Seite 1.
        for t in [-1.0f32, -0.3, 0.4, 1.0] {
            let a = kamera * face_rotation(0) * Vec3::new(1.0, t, 1.0);
            let b = kamera * face_rotation(1) * Vec3::new(-1.0, t, 1.0);
            assert!((a - b).length() < 1e-5, "{a} {b}");
        }
        // Oben (4) stoesst an die Oberkante (y = -1, Bildzeile 0) von Seite 0.
        let a = kamera * face_rotation(0) * Vec3::new(0.2, -1.0, 1.0);
        let b = kamera * face_rotation(4) * Vec3::new(0.2, 1.0, 1.0);
        assert!((a - b).length() < 1e-5, "{a} {b}");
    }

    // Zum Ansehen: rendert das Panorama eines Packs auf der CPU in ein PNG.
    // CORAL_PANORAMA_PACK=<Pack-Ordner> CORAL_PANORAMA_OUT=<datei.png> cargo test ... -- --ignored
    #[test]
    #[ignore]
    fn panorama_als_bild() {
        let (Some(pack), Some(ziel)) = (std::env::var_os("CORAL_PANORAMA_PACK"), std::env::var_os("CORAL_PANORAMA_OUT")) else { return };
        let atlas = load(Path::new(&pack)).unwrap();
        let (w, h) = (960usize, 540usize);
        let spin: f32 = std::env::var("CORAL_PANORAMA_SPIN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let mut out = Vec::new();
        vertices(spin, (w as f32, h as f32), (atlas.width, atlas.height), &mut out);
        let mut bild = image::RgbaImage::new(w as u32, h as u32);
        for d in out.chunks(3) {
            let px = |v: &TextVertex| ((v.position[0] + 1.0) / 2.0 * w as f32, (1.0 - v.position[1]) / 2.0 * h as f32);
            let (a, b, c) = (px(&d[0]), px(&d[1]), px(&d[2]));
            let flaeche = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if flaeche.abs() < 1e-6 {
                continue;
            }
            let (x0, x1) = (a.0.min(b.0).min(c.0).max(0.0) as usize, (a.0.max(b.0).max(c.0).ceil() as usize).min(w));
            let (y0, y1) = (a.1.min(b.1).min(c.1).max(0.0) as usize, (a.1.max(b.1).max(c.1).ceil() as usize).min(h));
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = (x as f32 + 0.5, y as f32 + 0.5);
                    let l1 = ((b.0 - p.0) * (c.1 - p.1) - (b.1 - p.1) * (c.0 - p.0)) / flaeche;
                    let l2 = ((c.0 - p.0) * (a.1 - p.1) - (c.1 - p.1) * (a.0 - p.0)) / flaeche;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                        continue;
                    }
                    let u = l1 * d[0].uv[0] + l2 * d[1].uv[0] + l3 * d[2].uv[0];
                    let v = l1 * d[0].uv[1] + l2 * d[1].uv[1] + l3 * d[2].uv[1];
                    let (tx, ty) = (((u * atlas.width as f32) as usize).min(atlas.width as usize - 1), ((v * atlas.height as f32) as usize).min(atlas.height as usize - 1));
                    let i = (ty * atlas.width as usize + tx) * 4;
                    bild.put_pixel(x as u32, y as u32, image::Rgba([atlas.rgba[i], atlas.rgba[i + 1], atlas.rgba[i + 2], 255]));
                }
            }
        }
        bild.save(Path::new(&ziel)).unwrap();
    }

    #[test]
    fn bild_ist_ganz_bedeckt() {
        let mut out = Vec::new();
        vertices(37.0, (1920.0, 1080.0), (6 * 64, 64), &mut out);
        assert!(!out.is_empty() && out.len() <= MAX_VERTICES);
        // Ein paar Stellen im Bild: jede liegt in einem Dreieck.
        let drin = |p: [f32; 2]| {
            out.chunks(3).any(|d| {
                let s = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let (x, y, z) = (s(d[0].position, d[1].position), s(d[1].position, d[2].position), s(d[2].position, d[0].position));
                (x >= 0.0 && y >= 0.0 && z >= 0.0) || (x <= 0.0 && y <= 0.0 && z <= 0.0)
            })
        };
        for p in [[0.0, 0.0], [-0.99, -0.99], [0.99, 0.99], [-0.99, 0.99], [0.99, -0.99], [0.5, -0.2]] {
            assert!(drin(p), "{p:?} nicht bedeckt");
        }
    }
}
