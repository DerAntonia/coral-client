// Keystrokes: die Bewegungstasten, Springen und die beiden Maustasten als kleine Tastatur im
// HUD, gedrueckte Tasten hervorgehoben. Verschiebbar im HUD-Editor (HudElement::Keystrokes),
// eingeschaltet im Modul-Menue.
//
// Hier steht nur die Anordnung in HUD-Einheiten; gezeichnet wird in main.rs (push_keystrokes),
// in den Farben des Themes.

// Welche Belegung eine Taste zeigt (und ob sie gedrueckt ist).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Forward,
    Left,
    Backward,
    Right,
    Jump,
    Attack,
    Use,
}

// Kantenlaenge einer Taste und Abstand zwischen zwei Tasten, in HUD-Einheiten.
const KEY: f32 = 20.0;
const GAP: f32 = 2.0;
// Die Leertaste ist ein flacher Balken ueber die ganze Breite.
const SPACE_H: f32 = 10.0;

// Groesse des ganzen Blocks in HUD-Einheiten.
pub const WIDTH: f32 = 3.0 * KEY + 2.0 * GAP;
pub const HEIGHT: f32 = 3.0 * KEY + SPACE_H + 3.0 * GAP;

// Jede Taste mit ihrem Rechteck (x, y, w, h) in HUD-Einheiten ab der oberen linken Ecke:
// W oben mittig, darunter A S D, dann die Leertaste, ganz unten linke und rechte Maustaste.
pub fn layout() -> [(Key, [f32; 4]); 7] {
    let zeile = |i: f32| i * (KEY + GAP);
    let spalte = zeile;
    let maus_y = 2.0 * (KEY + GAP) + SPACE_H + GAP;
    let maus_w = (WIDTH - GAP) / 2.0;
    [
        (Key::Forward, [spalte(1.0), 0.0, KEY, KEY]),
        (Key::Left, [spalte(0.0), zeile(1.0), KEY, KEY]),
        (Key::Backward, [spalte(1.0), zeile(1.0), KEY, KEY]),
        (Key::Right, [spalte(2.0), zeile(1.0), KEY, KEY]),
        (Key::Jump, [0.0, zeile(2.0), WIDTH, SPACE_H]),
        (Key::Attack, [0.0, maus_y, maus_w, KEY]),
        (Key::Use, [maus_w + GAP, maus_y, maus_w, KEY]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasten_bleiben_im_block_und_ueberlappen_nicht() {
        let tasten = layout();
        for (_, [x, y, w, h]) in tasten {
            assert!(x >= 0.0 && y >= 0.0 && x + w <= WIDTH + 1e-4 && y + h <= HEIGHT + 1e-4);
        }
        for (i, (_, a)) in tasten.iter().enumerate() {
            for (_, b) in &tasten[i + 1..] {
                let getrennt = a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1];
                assert!(getrennt, "{a:?} {b:?}");
            }
        }
    }
}
