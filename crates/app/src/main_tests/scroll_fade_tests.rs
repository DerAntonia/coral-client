use super::*;

// Eine Karte, die ueber den ganzen Verlauf reicht: dieselbe Flaeche wie vorher, an der
// Kante SCROLL_FADE_EDGE Deckkraft, ab dem Ende des Verlaufs volle.
#[test]
fn grosse_flaeche_wird_am_verlauf_geteilt() {
    let screen = (1000.0, 1000.0);
    let r = UiRect { rect: Rect { x: 100.0, y: 50.0, w: 200.0, h: 300.0 }, color: [1.0; 4], ..UiRect::FLAT };
    let fade = ScrollFade { top: 100.0, bottom: 10_000.0, zone: 100.0, edge_top: SCROLL_FADE_EDGE, edge_bottom: SCROLL_FADE_EDGE };
    let mut out = Vec::new();
    fade_quad_verts(&rect_verts(&r, screen), fade, screen.1, &mut out);
    assert_eq!(out.len() % 3, 0);
    let px = |v: &Vertex| (f32::midpoint(v.position[0], 1.0) * screen.0, ndc_y_px(v.position[1], screen.1));
    let flaeche: f32 = out
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (px(&t[0]), px(&t[1]), px(&t[2]));
            ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.0
        })
        .sum();
    assert!((flaeche - 200.0 * 300.0).abs() < 1.0, "{flaeche}");
    for v in &out {
        let (_, y) = px(v);
        let soll = if y < 200.0 - 1e-3 { fade.alpha(y) } else { 1.0 };
        if (y - 100.0).abs() > 1e-2 {
            assert!((v.color[3] - soll).abs() < 1e-3, "y {y}: {} statt {soll}", v.color[3]);
        }
    }
    assert!(out.iter().any(|v| (px(v).1 - 100.0).abs() < 1e-2 && (v.color[3] - SCROLL_FADE_EDGE).abs() < 1e-3));
}

// Unten genauso, aber nur, wo es weitergeht: ganz oben gescrollt kein Verlauf oben.
#[test]
fn verlauf_nur_wo_es_weitergeht() {
    let f = ScrollFade::new(100.0, 500.0, 100.0, 0.0, 3.0);
    assert_eq!(f.alpha(100.0), 1.0);
    assert!((f.alpha(500.0) - SCROLL_FADE_EDGE).abs() < 1e-6);
    assert_eq!(f.alpha(300.0), 1.0);
    let f = ScrollFade::new(100.0, 500.0, 100.0, 3.0, 3.0);
    assert!((f.alpha(100.0) - SCROLL_FADE_EDGE).abs() < 1e-6);
    assert_eq!(f.alpha(500.0), 1.0);
}
