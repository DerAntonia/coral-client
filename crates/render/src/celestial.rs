// Sonne und Mond: Richtung und Licht nach dem Sonnenstand. Gebraucht vom Verlaufshimmel des
// PvP-Pfads (s. gradient_sky) und vom Shader-Pfad (s. shader_path, nur mit Feature shader_path).

use glam::Vec3;

// Abweichung der Sonnenbahn vom Zenit nach Sueden: so werfen Bloecke auch mittags Schatten.
const SUN_PATH_TILT_DEGREES: f32 = 30.0;

// Gerichtetes Licht eines Frames (s. celestial_light).
pub struct CelestialLight {
    // Richtung ZUR Lichtquelle, Welt.
    pub dir: Vec3,
    // Richtung zur Sonne, auch nachts (der Mond steht gegenueber) -- fuer die Scheiben am
    // Himmel.
    pub sun: Vec3,
    // Farbe * Staerke.
    pub color: [f32; 3],
}

// Senkrechte auf der Ebene der Sonnenbahn (Osten x Mittagssonne).
#[must_use] 
pub fn sun_path_axis() -> Vec3 {
    let tilt = SUN_PATH_TILT_DEGREES.to_radians();
    Vec3::X.cross(Vec3::new(0.0, tilt.cos(), tilt.sin()))
}

// Sonne oder Mond nach dem Sonnenstand (world::sky::sky_angle, 0 = Mittag). Die Sonne
// laeuft von Osten (+x) ueber Sueden nach Westen; unter dem Horizont uebernimmt der Mond,
// schwach und blaeulich. Am Horizont blenden beide aus, statt umzuspringen.
#[must_use] 
pub fn celestial_light(sky_angle: f32) -> CelestialLight {
    let a = sky_angle * std::f32::consts::TAU;
    let tilt = SUN_PATH_TILT_DEGREES.to_radians();
    let (x, y) = (-a.sin(), a.cos());
    let sun = Vec3::new(x, y * tilt.cos(), y * tilt.sin());
    let smooth = |e0: f32, e1: f32, v: f32| {
        let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    if sun.y >= 0.0 {
        let s = smooth(-0.02, 0.15, sun.y) * 0.75;
        let warm = smooth(0.0, 0.4, sun.y);
        let mix = |a: f32, b: f32| a + (b - a) * warm;
        CelestialLight { dir: sun, sun, color: [mix(1.0, 1.0) * s, mix(0.55, 0.95) * s, mix(0.3, 0.88) * s] }
    } else {
        let s = smooth(-0.02, 0.15, -sun.y) * 0.12;
        CelestialLight { dir: -sun, sun, color: [0.6 * s, 0.7 * s, 1.0 * s] }
    }
}

