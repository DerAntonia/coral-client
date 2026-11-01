use super::*;

fn nahe(a: [f32; 3], b: [f32; 3]) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 1e-4, "{a:?} statt {b:?}");
    }
}

// Die drei Vollfarben-Ecken -- Standard-Referenzwerte fuer jede HSV->RGB-Formel.
#[test]
fn vollfarben_bei_saettigung_und_helligkeit_eins() {
    nahe(hsv_to_rgb(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]);
    nahe(hsv_to_rgb(120.0, 1.0, 1.0), [0.0, 1.0, 0.0]);
    nahe(hsv_to_rgb(240.0, 1.0, 1.0), [0.0, 0.0, 1.0]);
}

// Saettigung 0 muss GRAU ergeben, unabhaengig vom Ton -- ein haeufiger Randfall, den eine
// falsch abgeschriebene Formel leicht verfehlt.
#[test]
fn saettigung_null_ist_grau_unabhaengig_vom_ton() {
    for hue in [0.0, 90.0, 180.0, 270.0, 359.0] {
        nahe(hsv_to_rgb(hue, 0.0, 0.7), [0.7, 0.7, 0.7]);
    }
}

// Ton ist zyklisch: 0 und 360 Grad muessen dieselbe Farbe ergeben.
#[test]
fn ton_bei_0_und_360_ist_identisch() {
    nahe(hsv_to_rgb(0.0, 0.8, 0.9), hsv_to_rgb(360.0, 0.8, 0.9));
}

// Die Kernrechnung der Farbschritte fuer den Rand-Fall: Ton WRAPS (0
// minus einen Schritt muss auf den hoechsten Vielfachen-Wert unter 360 zurueckfallen,
// nicht ins Negative rutschen), Saettigung/Helligkeit KLEMMEN stattdessen am Rand (wie
// ueberall sonst im Menue, s. InventoryTransparencyPrev/Next).
#[test]
fn ton_wraps_saettigung_klemmt_am_rand() {
    let wrapped = (0i32 - HSV_HUE_STEP as i32).rem_euclid(HSV_HUE_MAX as i32);
    assert_eq!(wrapped, (HSV_HUE_MAX - HSV_HUE_STEP) as i32, "Ton muss über 0 nach unten wrappen");
    let clamped = (0i32 - HSV_SAT_VAL_STEP as i32).clamp(HSV_SAT_VAL_MIN as i32, HSV_SAT_VAL_MAX as i32);
    assert_eq!(clamped, HSV_SAT_VAL_MIN as i32, "Sättigung/Helligkeit dürfen nicht unter das Minimum rutschen");
}
