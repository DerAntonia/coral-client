// Sichtkegel-Ebenen aus der View-Projektions-Matrix (Gribb/Hartmann-Verfahren) --
// damit lassen sich Chunk-Spalten ausserhalb des Sichtfelds vor dem Draw-Call
// aussortieren, statt sie wie bisher unbedingt jeden Frame zu zeichnen (der Grund fuer
// den Lag-Einbruch ab ~2000 gemeshten Spalten bei hoher Sichtweite).

use glam::{Mat4, Vec3, Vec4Swizzles};

// Ebenen als (Normale, Distanz), sodass ein Punkt p genau dann VOR der Ebene liegt,
// wenn normal.dot(p) + d >= 0. Reihenfolge egal, alle sechs muessen erfuellt sein.
#[derive(Clone, Copy)]
pub struct Frustum {
    planes: [(Vec3, f32); 6],
}

impl Frustum {
    #[must_use] 
    pub fn from_view_proj(m: Mat4) -> Self {
        // Zeilen der Matrix (glam ist spaltenweise gespeichert -- row(i) holt die i-te Zeile).
        let row = |i: usize| m.row(i);
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));

        let raw = [
            r3 + r0, // links
            r3 - r0, // rechts
            r3 + r1, // unten
            r3 - r1, // oben
            r3 + r2, // nah
            r3 - r2, // fern
        ];

        let planes = raw.map(|p| {
            let normal = p.xyz();
            let len = normal.length();
            // Normalisieren, damit die Distanz in Weltkoordinaten-Einheiten vergleichbar ist
            // (sonst waere der spaetere AABB-Test gegen eine unskalierte Distanz falsch).
            (normal / len, p.w / len)
        });

        Self { planes }
    }

    // Ohne Fernebene (fuer die Fernsicht, die weit hinter der Fernebene des Gelaendes liegt).
    #[must_use] 
    pub const fn without_far(mut self) -> Self {
        self.planes[5] = (Vec3::ZERO, 1.0);
        self
    }

    // Grobtest: liegt die AABB komplett auf der Aussenseite irgendeiner Ebene, ist sie
    // sicher unsichtbar (kein exaktes Culling noetig -- ein paar falsch-positive Treffer
    // an den Raendern kosten nur ein paar zusaetzliche, tatsaechlich unsichtbare Draws).
    #[must_use] 
    pub fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        for (normal, d) in &self.planes {
            // Die von der Normale aus "am weitesten entfernte" Ecke der Box (positive
            // Vertex): liegt selbst die noch hinter der Ebene, ist die ganze Box drausen.
            let positive = Vec3::new(
                if normal.x >= 0.0 { max.x } else { min.x },
                if normal.y >= 0.0 { max.y } else { min.y },
                if normal.z >= 0.0 { max.z } else { min.z },
            );
            if normal.dot(positive) + d < 0.0 {
                return false;
            }
        }
        true
    }

    // Wie intersects_aabb fuer einen Stapel aus 64 gleichen Boxen uebereinander: Box k ist
    // (min, max) um k*step nach oben verschoben. Bit k gesetzt, wenn Box k den Test besteht.
    // Je Ebene haengt der Test nur linear von k ab, also ergibt jede Ebene eine Unter- oder
    // Obergrenze fuer k -- sechs Rechnungen statt sechs je Box. Die Grenzen werden um einen
    // kleinen Rand erweitert: Rundung darf hoechstens eine Box zu viel durchlassen, nie eine
    // sichtbare verwerfen.
    #[must_use]
    pub fn intersects_stacked_aabbs(&self, min: Vec3, max: Vec3, step: f32) -> u64 {
        const RAND: f32 = 1e-3;
        let (mut lo, mut hi) = (0.0f32, 63.0f32);
        for (normal, d) in &self.planes {
            let positive = Vec3::new(
                if normal.x >= 0.0 { max.x } else { min.x },
                if normal.y >= 0.0 { max.y } else { min.y },
                if normal.z >= 0.0 { max.z } else { min.z },
            );
            // Box k besteht diese Ebene, wenn a + b*k >= 0.
            let a = normal.dot(positive) + d;
            let b = normal.y * step;
            if b == 0.0 {
                if a < 0.0 {
                    return 0;
                }
            } else if b > 0.0 {
                lo = lo.max(-a / b);
            } else {
                hi = hi.min(-a / b);
            }
        }
        // NaN (entartete Ebene) faellt hier nicht durch: dann lieber alle Boxen.
        if lo.is_nan() || hi.is_nan() {
            return u64::MAX;
        }
        let lo = (lo - RAND).ceil().max(0.0);
        let hi = (hi + RAND).floor().min(63.0);
        if lo > hi {
            return 0;
        }
        let (lo, hi) = (lo as u32, hi as u32);
        let bis = if hi == 63 { u64::MAX } else { (1u64 << (hi + 1)) - 1 };
        bis & !((1u64 << lo) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_frustum() -> Frustum {
        let proj = Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.05, 1000.0);
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        Frustum::from_view_proj(proj * view)
    }

    #[test]
    fn box_direkt_vor_der_kamera_ist_sichtbar() {
        let f = test_frustum();
        assert!(f.intersects_aabb(Vec3::new(-1.0, -1.0, 9.0), Vec3::new(1.0, 1.0, 11.0)));
    }

    #[test]
    fn box_weit_hinter_der_kamera_ist_unsichtbar() {
        let f = test_frustum();
        assert!(!f.intersects_aabb(Vec3::new(-1.0, -1.0, -11.0), Vec3::new(1.0, 1.0, -9.0)));
    }

    #[test]
    fn box_weit_seitlich_ausserhalb_ist_unsichtbar() {
        let f = test_frustum();
        assert!(!f.intersects_aabb(Vec3::new(500.0, -1.0, 9.0), Vec3::new(502.0, 1.0, 11.0)));
    }

    // Der Stapeltest muss jede Box durchlassen, die der Einzeltest durchlaesst, und darf nur
    // an der Grenze eine zusaetzliche Box mitnehmen.
    #[test]
    fn stapeltest_deckt_sich_mit_einzeltest() {
        let mut zustand = 0x9E37_79B9_7F4A_7C15u64;
        let mut zufall = move || {
            zustand ^= zustand << 13;
            zustand ^= zustand >> 7;
            zustand ^= zustand << 17;
            (zustand >> 40) as f32 / (1u64 << 24) as f32
        };
        let mut zusatz = 0;
        for _ in 0..4000 {
            let yaw = zufall() * std::f32::consts::TAU;
            let pitch = (zufall() - 0.5) * 3.1;
            let blick = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos());
            let proj = Mat4::perspective_rh((30.0 + zufall() * 100.0).to_radians(), 16.0 / 9.0, 0.05, 400.0);
            let f = Frustum::from_view_proj(proj * Mat4::look_at_rh(Vec3::ZERO, blick, Vec3::Y));
            let f = if zufall() < 0.3 { f.without_far() } else { f };
            let x0 = (zufall() - 0.5) * 600.0;
            let z0 = (zufall() - 0.5) * 600.0;
            let y0 = -(zufall() * 600.0);
            let min = Vec3::new(x0, y0, z0);
            let max = min + Vec3::splat(18.0);
            let stapel = f.intersects_stacked_aabbs(min, max, 16.0);
            let mut einzeln = 0u64;
            for k in 0..64 {
                let hub = Vec3::new(0.0, k as f32 * 16.0, 0.0);
                if f.intersects_aabb(min + hub, max + hub) {
                    einzeln |= 1 << k;
                }
            }
            assert_eq!(einzeln & !stapel, 0, "sichtbare Box verworfen");
            zusatz += (stapel & !einzeln).count_ones();
        }
        assert!(zusatz < 40, "zu viele zusaetzliche Boxen: {zusatz}");
    }
}
