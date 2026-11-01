// Fadenkreuz-Formen und ihre Teile.

use crate::*;

pub(crate) fn crosshair_shape_label(i: usize) -> &'static str {
    CROSSHAIR_SHAPES.get(i).map_or("Eigene", |s| s.0)
}

// Ein Baustein des Fadenkreuzes in Bildschirmpixeln.
#[derive(Clone, Copy)]
pub(crate) enum CrosshairPart {
    Bar(Rect),
    // Kreisscheibe (inner 0) oder Ring zwischen den beiden Radien. Der Fadenkreuz-Pass rechnet
    // die Kante je Pixel aus (s. crosshair.wgsl), deshalb ist auch ein Punkt von vier Pixeln
    // rund und kein Quadrat.
    Disc { center: (f32, f32), inner: f32, outer: f32 },
}

// Die Bausteine des eingebauten Fadenkreuzes, Kontur (true) zuerst: ohne Tiefenpuffer
// entscheidet die Reihenfolge, und die Flaeche muss die Mitte der Kontur wieder abdecken. Die
// Kontur ist jeder Baustein um CROSSHAIR_OUTLINE_PX nach allen Seiten vergroessert -- ihre
// Vereinigung minus der Flaeche ist genau ein Pixel Rand, auch an den Innenecken des Kreuzes.
//
// Ganze Pixel, deshalb round(): ein halber Pixel Versatz verteilt eine Balkenkante ueber zwei
// Pixelspalten und sieht aus wie Kantenglaettung, obwohl keine im Spiel ist. Ein Kreis mit
// ungeradem Durchmesser sitzt dagegen auf einer Pixelmitte, sonst liegt er schief im Raster.
//
// Dieselben Bausteine zeichnen das Kreuz im Spiel (s. crosshair_vertices) und die Vorschau im
// HUD-Editor. Die Kontur MUSS im Spiel im Fadenkreuz-Pass entstehen: der UI-Pass zeichnet in
// die Textur, die das Kreuz fuer seine Gegenfarbe abtastet -- eine schwarze Kontur dort machte
// die Gegenfarbe ueberall gleich.
// Wie weit die Mitte des eingebauten Kreuzes neben dem Pixelraster liegt: Punkt und Kreis mit
// ungerader Groesse sitzen mitten auf einem Pixel, alles andere auf einer Pixelecke. Der Ring
// (Schild-Cooldown, Timer) nimmt dieselbe Mitte, sonst saesse er schief um den Punkt.
pub(crate) const fn crosshair_center_offset(shape: usize, size: u32) -> f32 {
    if matches!(shape, CROSSHAIR_SHAPE_DOT | CROSSHAIR_SHAPE_CIRCLE) && size % 2 == 1 { 0.5 } else { 0.0 }
}

pub(crate) fn crosshair_parts(shape: usize, size: u32, center: (f32, f32), outline: bool) -> Vec<(CrosshairPart, bool)> {
    let (cx, cy) = (center.0.round(), center.1.round());
    let t = CROSSHAIR_THICKNESS_PX / 2.0;
    let halb = (size as f32 / 2.0).round();
    let mut flaeche = Vec::new();
    match shape {
        CROSSHAIR_SHAPE_DOT | CROSSHAIR_SHAPE_CIRCLE => {
            let versatz = crosshair_center_offset(shape, size);
            let outer = size as f32 / 2.0;
            let inner = if shape == CROSSHAIR_SHAPE_CIRCLE { (outer - CROSSHAIR_THICKNESS_PX).max(0.0) } else { 0.0 };
            flaeche.push(CrosshairPart::Disc { center: (cx + versatz, cy + versatz), inner, outer });
        }
        CROSSHAIR_SHAPE_OPEN => {
            // Vier Arme mit freier Mitte, die Luecke waechst mit der Groesse.
            let luecke = (size as f32 / 5.0).round().max(2.0);
            let arm = (halb - luecke).max(1.0);
            flaeche.push(CrosshairPart::Bar(Rect { x: cx - t, y: cy - luecke - arm, w: 2.0 * t, h: arm }));
            flaeche.push(CrosshairPart::Bar(Rect { x: cx - t, y: cy + luecke, w: 2.0 * t, h: arm }));
            flaeche.push(CrosshairPart::Bar(Rect { x: cx - luecke - arm, y: cy - t, w: arm, h: 2.0 * t }));
            flaeche.push(CrosshairPart::Bar(Rect { x: cx + luecke, y: cy - t, w: arm, h: 2.0 * t }));
        }
        _ => {
            flaeche.push(CrosshairPart::Bar(Rect { x: cx - t, y: cy - halb, w: 2.0 * t, h: 2.0 * halb }));
            flaeche.push(CrosshairPart::Bar(Rect { x: cx - halb, y: cy - t, w: 2.0 * halb, h: 2.0 * t }));
        }
    }
    let o = CROSSHAIR_OUTLINE_PX;
    let mut out = Vec::new();
    if outline {
        out.extend(flaeche.iter().map(|teil| {
            let kontur = match *teil {
                CrosshairPart::Bar(r) => CrosshairPart::Bar(Rect { x: r.x - o, y: r.y - o, w: r.w + 2.0 * o, h: r.h + 2.0 * o }),
                CrosshairPart::Disc { center, inner, outer } => CrosshairPart::Disc { center, inner: (inner - o).max(0.0), outer: outer + o },
            };
            (kontur, true)
        }));
    }
    out.extend(flaeche.into_iter().map(|teil| (teil, false)));
    out
}

// Ein Baustein als UI-Flaeche (Vorschau im HUD-Editor).
pub(crate) fn crosshair_part_rect(teil: CrosshairPart, color: [f32; 4]) -> UiRect {
    match teil {
        CrosshairPart::Bar(rect) => UiRect { rect, color, ..UiRect::FLAT },
        CrosshairPart::Disc { center, inner, outer } => {
            let rect = Rect { x: center.0 - outer, y: center.1 - outer, w: 2.0 * outer, h: 2.0 * outer };
            if inner > 0.0 {
                UiRect::framed(rect, [0.0; 4], outer, outer - inner, color)
            } else {
                UiRect::rounded(rect, color, outer)
            }
        }
    }
}

// Farbmodi fuers Kreuz, im HUD-Editor als zwei Knoepfe (s. ButtonAction::CrosshairColorPrev/
// Next) -- Label + der Wert, den crosshair.wgsl::CrosshairSettings.mode erwartet. Index 0
// (Komplementaer) ist der automatisch berechnete Standard, Einfarbig nimmt die Farbe aus
// Settings::crosshair_hue/saturation/value.
pub(crate) const CROSSHAIR_COLOR_MODES: [(&str, u32); 2] = [("Komplementär", 0), ("Einfarbig", 1)];
// Index des Komplementaer-Modus in CROSSHAIR_COLOR_MODES -- der einzige, der sich aufhellen
// laesst (s. Settings::crosshair_brightness_boost).
pub(crate) const CROSSHAIR_MODE_COMPLEMENTARY: usize = 0;
