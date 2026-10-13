// Sichtkegel-Ebenen aus der View-Projektions-Matrix (Gribb/Hartmann-Verfahren) --
// damit lassen sich Chunk-Spalten ausserhalb des Sichtfelds vor dem Draw-Call
// aussortieren, statt sie wie bisher unbedingt jeden Frame zu zeichnen (der Grund fuer
// den Lag-Einbruch ab ~2000 gemeshten Spalten bei hoher Sichtweite).

use glam::{Mat4, Vec3, Vec4Swizzles};

// Ebenen als (Normale, Distanz), sodass ein Punkt p genau dann VOR der Ebene liegt,
// wenn normal.dot(p) + d >= 0. Reihenfolge egal, alle sechs muessen erfuellt sein.
pub struct Frustum {
    planes: [(Vec3, f32); 6],
}

impl Frustum {
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
    pub fn without_far(mut self) -> Self {
        self.planes[5] = (Vec3::ZERO, 1.0);
        self
    }

    // Grobtest: liegt die AABB komplett auf der Aussenseite irgendeiner Ebene, ist sie
    // sicher unsichtbar (kein exaktes Culling noetig -- ein paar falsch-positive Treffer
    // an den Raendern kosten nur ein paar zusaetzliche, tatsaechlich unsichtbare Draws).
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
}
