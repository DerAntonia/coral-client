// Ruestungs-HUD: die vier getragenen Ruestungsteile als Item-Bild, darueber die verbleibende
// Haltbarkeit als Zahl. Verschiebbar im HUD-Editor (HudElement::Armor), eingeschaltet im
// Modul-Menue.
//
// Hier steht nur die Anordnung -- welches Teil in welcher Zelle landet. Gezeichnet wird in
// main.rs (build_armor_hud), weil dort Item-Bilder und Schrift liegen.

// Ausrichtung (Settings::armor_hud_layout).
pub const LAYOUT_VERTICAL: u8 = 0;
pub const LAYOUT_FLAT_HELMET_RIGHT: u8 = 1;
pub const LAYOUT_FLAT_HELMET_LEFT: u8 = 2;
pub const LAYOUT_NAMES: [&str; 3] = ["Hoch", "Flach, Helm rechts", "Flach, Helm links"];

// Wohin die Teile ruecken, wenn eins fehlt -- hochkant nach oben/unten, flach nach links/rechts.
pub const SORT_VERTICAL_NAMES: [&str; 2] = ["Oben", "Unten"];
pub const SORT_FLAT_NAMES: [&str; 2] = ["Links", "Rechts"];

// Ab weniger als so vielen Haltbarkeitspunkten steht die Zahl in Orange.
pub const LOW_DURABILITY: i32 = 50;

// Abstand zwischen zwei Teilen in Bildpixeln (Settings::armor_hud_gap). Negativ schiebt die
// Bilder uebereinander -- sie haben selbst durchsichtigen Rand. Die Zahl zaehlt nicht mit.
pub const GAP_MIN: i32 = -12;
pub const GAP_MAX: i32 = 16;
// Groesse der Zahl in Prozent ihrer Grundgroesse (Settings::armor_hud_number_scale).
pub const NUMBER_SCALE_MIN: u32 = 50;
pub const NUMBER_SCALE_MAX: u32 = 300;
pub const NUMBER_SCALE_STEP: u32 = 10;

pub fn is_vertical(layout: u8) -> bool {
    layout == LAYOUT_VERTICAL
}

// Verschiebung der Zahl gegen die Bildmitte, in Bildpixeln (Settings::armor_hud_number_x/_y).
pub const NUMBER_OFFSET_MIN: i32 = -24;
pub const NUMBER_OFFSET_MAX: i32 = 24;

// Obere linke Ecke der Zahl: ihre Mitte sitzt auf der Mitte des Bilds (`bild` obere linke
// Ecke, `icon` Kantenlaenge), verschoben um `offset` Bildpixel (`px` gross); `text` ist die
// Groesse der Zahl. Positives y schiebt nach unten.
pub fn number_origin(bild: (f32, f32), icon: f32, px: f32, offset: (i32, i32), text: (f32, f32)) -> (f32, f32) {
    let mitte = (bild.0 + icon / 2.0 + offset.0 as f32 * px, bild.1 + icon / 2.0 + offset.1 as f32 * px);
    (mitte.0 - text.0 / 2.0, mitte.1 - text.1 / 2.0)
}

// Zelle (0 = oben bzw. links) jedes Teils in der Reihenfolge Helm, Brust, Hose, Schuhe; None
// fuer fehlende Teile. Die vorhandenen behalten ihre Reihenfolge und ruecken lueckenlos an den
// Anfang (`ans_ende` = false: oben/links) oder ans Ende (unten/rechts) der vier Zellen.
pub fn cells(present: [bool; 4], layout: u8, ans_ende: bool) -> [Option<usize>; 4] {
    let reihe: [usize; 4] = if layout == LAYOUT_FLAT_HELMET_RIGHT { [3, 2, 1, 0] } else { [0, 1, 2, 3] };
    let anzahl = present.iter().filter(|&&p| p).count();
    let mut zelle = if ans_ende { 4 - anzahl } else { 0 };
    let mut out = [None; 4];
    for teil in reihe {
        if present[teil] {
            out[teil] = Some(zelle);
            zelle += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zahl_sitzt_um_die_bildmitte() {
        // 32er-Bild bei (100, 50), Zahl 12x16: bei 0/0 mittig, 13 Bildpixel hoch = 26 nach oben.
        assert_eq!(number_origin((100.0, 50.0), 32.0, 2.0, (0, 0), (12.0, 16.0)), (110.0, 58.0));
        assert_eq!(number_origin((100.0, 50.0), 32.0, 2.0, (-4, -13), (12.0, 16.0)), (102.0, 32.0));
    }

    #[test]
    fn alle_teile_hochkant_von_oben() {
        assert_eq!(cells([true; 4], LAYOUT_VERTICAL, false), [Some(0), Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn fehlende_brust_rueckt_nach() {
        let ohne_brust = [true, false, true, true];
        // Nach oben/links: der Helm bleibt oben, Hose und Schuhe ruecken auf.
        assert_eq!(cells(ohne_brust, LAYOUT_VERTICAL, false), [Some(0), None, Some(1), Some(2)]);
        // Nach unten/rechts: der Helm rueckt zur Hose hin.
        assert_eq!(cells(ohne_brust, LAYOUT_VERTICAL, true), [Some(1), None, Some(2), Some(3)]);
    }

    #[test]
    fn helm_rechts_kehrt_die_reihe_um() {
        assert_eq!(cells([true; 4], LAYOUT_FLAT_HELMET_RIGHT, false), [Some(3), Some(2), Some(1), Some(0)]);
        // Nur Helm und Schuhe, nach links geschoben: Schuhe links, Helm daneben.
        assert_eq!(cells([true, false, false, true], LAYOUT_FLAT_HELMET_RIGHT, false), [Some(1), None, None, Some(0)]);
        assert_eq!(cells([true, false, false, true], LAYOUT_FLAT_HELMET_LEFT, true), [Some(2), None, None, Some(3)]);
    }
}
