// Physische Breite des Bildschirms in Millimetern, damit sich Abstaende in Zentimetern angeben
// lassen (z.B. der Rand fuer die Erklaerungen in den Einstellungen). winit kennt nur Pixel;
// die Groesse steht in der EDID des Monitors, die Linux unter /sys/class/drm/<Anschluss>/edid
// bereitstellt. Ohne EDID (anderes System, Treiber ohne sysfs) gibt es None und der Aufrufer
// nimmt einen ueblichen Wert an.

// Breite in mm des Bildschirms mit `width_px` Pixeln Breite (aktueller Modus). Sind mehrere
// angeschlossen, gewinnt der, dessen bevorzugter Modus so breit ist; sonst der erste.
pub fn width_mm(width_px: u32) -> Option<f32> {
    let edids = connected_edids();
    let passend = edids.iter().find(|e| preferred_width(e) == Some(width_px));
    passend.or(edids.first()).and_then(|e| size_mm(e))
}

#[cfg(target_os = "linux")]
fn connected_edids() -> Vec<Vec<u8>> {
    let Ok(dir) = std::fs::read_dir("/sys/class/drm") else { return Vec::new() };
    let mut pfade: Vec<_> = dir.flatten().map(|e| e.path()).collect();
    pfade.sort();
    pfade
        .into_iter()
        .filter(|p| std::fs::read_to_string(p.join("status")).is_ok_and(|s| s.trim() == "connected"))
        .filter_map(|p| std::fs::read(p.join("edid")).ok())
        .filter(|e| e.len() >= 128 && e[..8] == [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00])
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn connected_edids() -> Vec<Vec<u8>> {
    Vec::new()
}

// Breite des bevorzugten Modus: erster Timing-Block ab Byte 54.
fn preferred_width(edid: &[u8]) -> Option<u32> {
    let w = edid[56] as u32 | ((edid[58] as u32 >> 4) << 8);
    (w > 0).then_some(w)
}

// Bildgroesse: im ersten Timing-Block in mm (genauer), sonst die Basisangabe in ganzen cm.
fn size_mm(edid: &[u8]) -> Option<f32> {
    let mm = edid[66] as u32 | ((edid[68] as u32 >> 4) << 8);
    if mm > 0 {
        return Some(mm as f32);
    }
    (edid[21] > 0).then(|| edid[21] as f32 * 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liest_breite_aus_timing_block_sonst_aus_cm() {
        let mut e = vec![0u8; 128];
        e[21] = 60;
        // 1920 = 0x780: untere 8 Bit in 56, obere 4 Bit im hohen Nibble von 58.
        e[56] = 0x80;
        e[58] = 0x70;
        // 598 mm = 0x256.
        e[66] = 0x56;
        e[68] = 0x20;
        assert_eq!(preferred_width(&e), Some(1920));
        assert_eq!(size_mm(&e), Some(598.0));
        e[66] = 0;
        e[68] = 0;
        assert_eq!(size_mm(&e), Some(600.0));
    }
}
