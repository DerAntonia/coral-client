// Die GEOMETRIE eines Items, wie es in der Welt liegt (in der Hand eines Spielers).
//
// Zwei Sorten, genau wie in Vanilla -- dieselbe Zweiteilung wie bei den Inventar-Icons
// (s. app::item_icons), nur diesmal dreidimensional statt als Bildschirm-Quad:
//
//   * Blockitem (Obsidian, Kisten, ...): ein Wuerfel mit den sechs Flaechentexturen.
//   * Flaches Item (Schwert, Totem, Perle, ...): das Sprite, EXTRUDIERT -- vorne und hinten
//     eine Flaeche, dazwischen 1/16 Block Dicke, und ringsum an jeder Kante zwischen
//     sichtbarem und durchsichtigem Pixel ein schmales Randstueck. Vanilla baut daraus in
//     ItemModelGenerator ein echtes Modell; ohne die Raender waere ein Schwert von der Seite
//     unsichtbar statt ein Millimeter dickes Blatt.
//
// Alle Koordinaten hier laufen von 0 bis 1 (Vanillas Modellraum 0..16 durch 16 geteilt), mit
// dem Nullpunkt in der Modellecke. Das Zentrieren uebernimmt die Halte-Transformation, die
// mit einer abschliessenden Verschiebung um -0.5 endet (s. assets::ItemDisplay).

use crate::entity_model::{diffuse_shade, Affine};
use crate::mesher::BlockVertex;

// Vanillas ItemModelGenerator: die Sprite-Ebene liegt zwischen 7.5/16 und 8.5/16, ist also
// genau einen Sechzehntel-Block dick und mittig im Modellwuerfel.
const FRONT_Z: f32 = 8.5 / 16.0;
const BACK_Z: f32 = 7.5 / 16.0;
// Laengs eines Randstreifens bleibt die UV knapp innerhalb des Pixels (s. emit_sprite_item).
const EDGE_UV_INSET: f32 = 0.01;

// Alphaschwelle, ab der ein Pixel als vorhanden gilt. Vanilla prueft in ItemModelGenerator
// auf `alpha / 255 > 0.1` -- dieselbe Schwelle, damit weiche Kanten (z.B. der Schein am
// Totem) nicht als Rand ausgestuelpt werden.
const ALPHA_THRESHOLD: u8 = 26;

// Die Deckkraft-Maske eines Item-Sprites: fuer jeden Pixel, ob er ueberhaupt sichtbar ist.
// Genau daraus entsteht der Rand -- das Sprite selbst kommt aus dem Atlas.
#[derive(Clone, Debug)]
pub struct SpriteMask {
    pub width: u32,
    pub height: u32,
    opaque: Vec<bool>,
}

impl SpriteMask {
    // Aus RGBA-Pixeln (zeilenweise von oben, wie ueberall im Projekt).
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let mut opaque = vec![false; (width * height) as usize];
        for (index, flag) in opaque.iter_mut().enumerate() {
            *flag = rgba.get(index * 4 + 3).is_some_and(|alpha| *alpha > ALPHA_THRESHOLD);
        }
        Self { width, height, opaque }
    }

    pub fn is_opaque(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return false;
        }
        self.opaque[(y as u32 * self.width + x as u32) as usize]
    }

    pub fn any_opaque(&self) -> bool {
        self.opaque.iter().any(|o| *o)
    }
}

// Normiertes Atlas-Rechteck (u0, v0, u1, v1) -- dieselbe Bedeutung wie assets::TexRect.
pub type TexRect = [f32; 4];

// Ein flaches Item: Vorder- und Rueckseite plus die Raender entlang der Sprite-Kontur.
//
// `tex` ist der Platz des Sprites im Atlas, `mask` seine Deckkraft in Pixeln. Beide muessen
// zum SELBEN Bild gehoeren, sonst sitzt der Rand neben der Silhouette.
pub fn emit_sprite_item(mask: &SpriteMask, tex: TexRect, base: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
    if mask.width == 0 || mask.height == 0 {
        return;
    }
    let (w, h) = (mask.width as f32, mask.height as f32);
    // Pixelkoordinate -> Anteil im Modell (0..1). Y ist im Bild nach UNTEN gezaehlt, im
    // Modell nach oben: deshalb die Spiegelung, sonst stuende jedes Item auf dem Kopf.
    let px = |x: f32| x / w;
    let py = |y: f32| 1.0 - y / h;
    let uv = |x: f32, y: f32| -> [f32; 2] {
        let [u0, v0, u1, v1] = tex;
        [u0 + (x / w) * (u1 - u0), v0 + (y / h) * (v1 - v0)]
    };

    // Vorderseite (+Z) und Rueckseite (-Z) ueber das ganze Sprite. Durchsichtige Pixel
    // blendet der Shader ohnehin per Alpha aus -- dafuer braucht es keine Geometrie.
    push_sprite_quad(
        out,
        base,
        brightness,
        [0.0, 0.0, 1.0],
        [
            ([px(0.0), py(h), FRONT_Z], uv(0.0, h)),
            ([px(w), py(h), FRONT_Z], uv(w, h)),
            ([px(w), py(0.0), FRONT_Z], uv(w, 0.0)),
            ([px(0.0), py(0.0), FRONT_Z], uv(0.0, 0.0)),
        ],
    );
    push_sprite_quad(
        out,
        base,
        brightness,
        [0.0, 0.0, -1.0],
        [
            ([px(w), py(h), BACK_Z], uv(w, h)),
            ([px(0.0), py(h), BACK_Z], uv(0.0, h)),
            ([px(0.0), py(0.0), BACK_Z], uv(0.0, 0.0)),
            ([px(w), py(0.0), BACK_Z], uv(w, 0.0)),
        ],
    );

    // Der Rand: an jeder Kante, an der ein sichtbarer Pixel an einen unsichtbaren (oder an
    // den Bildrand) stoesst, ein Streifen von der Rueck- zur Vorderseite. Die UV nimmt den
    // Pixel selbst, wie Vanilla -- so hat die Schnittkante die Farbe des Randpixels.
    // Quer zum Streifen liegt die UV in der PIXELMITTE, nicht auf der Pixelgrenze: auf der
    // Grenze waehlt das Nearest-Sampling bei rechten/unteren Kanten den durchsichtigen
    // Nachbarn, der Alpha-Test verwirft den ganzen Streifen -- und je nach Haltung fehlte dem
    // Item dann jede sichtbare Dicke.
    for y in 0..mask.height as i32 {
        for x in 0..mask.width as i32 {
            if !mask.is_opaque(x, y) {
                continue;
            }
            let (fx, fy) = (x as f32, y as f32);
            // Links (-X)
            if !mask.is_opaque(x - 1, y) {
                push_sprite_quad(
                    out,
                    base,
                    brightness,
                    [-1.0, 0.0, 0.0],
                    [
                        ([px(fx), py(fy + 1.0), BACK_Z], uv(fx + 0.5, fy + 1.0 - EDGE_UV_INSET)),
                        ([px(fx), py(fy + 1.0), FRONT_Z], uv(fx + 0.5, fy + 1.0 - EDGE_UV_INSET)),
                        ([px(fx), py(fy), FRONT_Z], uv(fx + 0.5, fy + EDGE_UV_INSET)),
                        ([px(fx), py(fy), BACK_Z], uv(fx + 0.5, fy + EDGE_UV_INSET)),
                    ],
                );
            }
            // Rechts (+X)
            if !mask.is_opaque(x + 1, y) {
                push_sprite_quad(
                    out,
                    base,
                    brightness,
                    [1.0, 0.0, 0.0],
                    [
                        ([px(fx + 1.0), py(fy + 1.0), FRONT_Z], uv(fx + 0.5, fy + 1.0 - EDGE_UV_INSET)),
                        ([px(fx + 1.0), py(fy + 1.0), BACK_Z], uv(fx + 0.5, fy + 1.0 - EDGE_UV_INSET)),
                        ([px(fx + 1.0), py(fy), BACK_Z], uv(fx + 0.5, fy + EDGE_UV_INSET)),
                        ([px(fx + 1.0), py(fy), FRONT_Z], uv(fx + 0.5, fy + EDGE_UV_INSET)),
                    ],
                );
            }
            // Oben im BILD ist +Y im Modell (s. py).
            if !mask.is_opaque(x, y - 1) {
                push_sprite_quad(
                    out,
                    base,
                    brightness,
                    [0.0, 1.0, 0.0],
                    [
                        ([px(fx), py(fy), BACK_Z], uv(fx + EDGE_UV_INSET, fy + 0.5)),
                        ([px(fx), py(fy), FRONT_Z], uv(fx + EDGE_UV_INSET, fy + 0.5)),
                        ([px(fx + 1.0), py(fy), FRONT_Z], uv(fx + 1.0 - EDGE_UV_INSET, fy + 0.5)),
                        ([px(fx + 1.0), py(fy), BACK_Z], uv(fx + 1.0 - EDGE_UV_INSET, fy + 0.5)),
                    ],
                );
            }
            // Unten im Bild = -Y im Modell.
            if !mask.is_opaque(x, y + 1) {
                push_sprite_quad(
                    out,
                    base,
                    brightness,
                    [0.0, -1.0, 0.0],
                    [
                        ([px(fx + 1.0), py(fy + 1.0), BACK_Z], uv(fx + 1.0 - EDGE_UV_INSET, fy + 0.5)),
                        ([px(fx + 1.0), py(fy + 1.0), FRONT_Z], uv(fx + 1.0 - EDGE_UV_INSET, fy + 0.5)),
                        ([px(fx), py(fy + 1.0), FRONT_Z], uv(fx + EDGE_UV_INSET, fy + 0.5)),
                        ([px(fx), py(fy + 1.0), BACK_Z], uv(fx + EDGE_UV_INSET, fy + 0.5)),
                    ],
                );
            }
        }
    }
}

// Die Sprite-Vierecke oben sind gegen den Uhrzeigersinn VON AUSSEN notiert, push_quad
// erwartet aber die Reihenfolge der Blockitem-Flaechen (im Uhrzeigersinn von aussen, s.
// emit_block_item) und dreht sie erst selbst um. Ohne diese Umkehr war jede Flaeche des
// flachen Items nach innen gewickelt: die Vorderseite wurde verworfen und man sah durch das
// Item hindurch auf Rueckseite und abgewandte Raender.
fn push_sprite_quad(out: &mut Vec<BlockVertex>, base: Affine, brightness: f32, normal: [f32; 3], quad: [([f32; 3], [f32; 2]); 4]) {
    push_quad(out, base, brightness, normal, [quad[0], quad[3], quad[2], quad[1]]);
}

// Ein Blockitem: der volle Wuerfel mit je einer Textur pro Seite. Reihenfolge der Flaechen
// ist assets::FACE_* (down, up, north, south, west, east) -- dieselbe wie ueberall sonst,
// damit ein Grasblock auch in der Hand oben gruen ist.
pub fn emit_block_item(faces: &[TexRect; 6], base: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
    // Ecken des Einheitswuerfels.
    let c = |x: f32, y: f32, z: f32| [x, y, z];
    // Je Flaeche: Normale, die vier Ecken gegen den Uhrzeigersinn und die UV-Ecken dazu.
    let full = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let faces_geom: [([f32; 3], [[f32; 3]; 4]); 6] = [
        // down (-Y)
        ([0.0, -1.0, 0.0], [c(0.0, 0.0, 1.0), c(1.0, 0.0, 1.0), c(1.0, 0.0, 0.0), c(0.0, 0.0, 0.0)]),
        // up (+Y)
        ([0.0, 1.0, 0.0], [c(0.0, 1.0, 0.0), c(1.0, 1.0, 0.0), c(1.0, 1.0, 1.0), c(0.0, 1.0, 1.0)]),
        // north (-Z)
        ([0.0, 0.0, -1.0], [c(1.0, 1.0, 0.0), c(0.0, 1.0, 0.0), c(0.0, 0.0, 0.0), c(1.0, 0.0, 0.0)]),
        // south (+Z)
        ([0.0, 0.0, 1.0], [c(0.0, 1.0, 1.0), c(1.0, 1.0, 1.0), c(1.0, 0.0, 1.0), c(0.0, 0.0, 1.0)]),
        // west (-X)
        ([-1.0, 0.0, 0.0], [c(0.0, 1.0, 0.0), c(0.0, 1.0, 1.0), c(0.0, 0.0, 1.0), c(0.0, 0.0, 0.0)]),
        // east (+X)
        ([1.0, 0.0, 0.0], [c(1.0, 1.0, 1.0), c(1.0, 1.0, 0.0), c(1.0, 0.0, 0.0), c(1.0, 0.0, 1.0)]),
    ];

    for (index, (normal, corners)) in faces_geom.into_iter().enumerate() {
        let [u0, v0, u1, v1] = faces[index];
        let corner_uv = |i: usize| -> [f32; 2] {
            let [fu, fv] = full[i];
            [u0 + fu * (u1 - u0), v0 + fv * (v1 - v0)]
        };
        push_quad(
            out,
            base,
            brightness,
            normal,
            [(corners[0], corner_uv(0)), (corners[1], corner_uv(1)), (corners[2], corner_uv(2)), (corners[3], corner_uv(3))],
        );
    }
}

// Ein Item mit eigenem 3D-Modell (Resourcepack, s. assets::model_quads): beliebige, schon
// fertig gedrehte Vierecke im Modellraum 0..1, je mit Normale und den UVs seiner vier Ecken
// im Atlas. Dieselbe Einbettung und Beleuchtung wie emit_block_item.
pub fn emit_model_quads(quads: &[([[f32; 3]; 4], [f32; 3], [[f32; 2]; 4])], base: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
    for (corners, normal, uv) in quads {
        push_quad_tinted(out, base, brightness, *normal, [(corners[0], uv[0]), (corners[1], uv[1]), (corners[2], uv[2]), (corners[3], uv[3])], [1.0; 3]);
    }
}

// Wie emit_model_quads, je Viereck mit einer Toenung (tintindex der Flaeche, s.
// assets::item_tints) -- weiss = ungetoent.
pub fn emit_model_quads_tinted(quads: &[([[f32; 3]; 4], [f32; 3], [[f32; 2]; 4], [f32; 3])], base: Affine, brightness: f32, out: &mut Vec<BlockVertex>) {
    for (corners, normal, uv, tint) in quads {
        push_quad_tinted(out, base, brightness, *normal, [(corners[0], uv[0]), (corners[1], uv[1]), (corners[2], uv[2]), (corners[3], uv[3])], *tint);
    }
}

// Ein Viereck als zwei Dreiecke, mit Richtungslicht wie bei den Entity-Modellen (s.
// entity_model::emit_cuboid): die Normale wird MIT der Einbettung gedreht, damit ein
// gedrehtes Item nicht mitgedrehte Beleuchtung bekommt.
fn push_quad(out: &mut Vec<BlockVertex>, base: Affine, brightness: f32, normal: [f32; 3], quad: [([f32; 3], [f32; 2]); 4]) {
    push_quad_tinted(out, base, brightness, normal, quad, [1.0; 3]);
}

fn push_quad_tinted(out: &mut Vec<BlockVertex>, base: Affine, brightness: f32, normal: [f32; 3], quad: [([f32; 3], [f32; 2]); 4], tint: [f32; 3]) {
    let shade = brightness * diffuse_shade(base.apply_direction(normal));
    let color = [shade * tint[0], shade * tint[1], shade * tint[2]];
    // Umlaufrichtung: 0,2,1 / 0,3,2 statt 0,1,2 / 0,2,3. Die Pipeline verwirft
    // rueckwaertsgewandte Dreiecke (cull_mode: Face::Back, s. render::BlockRenderer), und
    // die Entscheidung faellt allein ueber die Umlaufrichtung -- unsere Shading-Normale
    // sieht die GPU dabei nie. Andersherum gewickelt wird jede VORDERseite verworfen und
    // man schaut ins Innere des Items, auf die eigentlich verdeckten
    // Flaechen. Die Richtung ist an den Entity-Kaesten geeicht, mit denen die Items im
    // selben Zeichenaufruf landen -- s. winding_tests.
    for &i in &[0usize, 2, 1, 0, 3, 2] {
        let (position, uv) = quad[i];
        out.push(BlockVertex { position: base.apply(position), color, uv });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maske(pixel: &[&str]) -> SpriteMask {
        let height = pixel.len() as u32;
        let width = pixel[0].len() as u32;
        let mut rgba = Vec::new();
        for row in pixel {
            for zeichen in row.chars() {
                let alpha = if zeichen == '#' { 255 } else { 0 };
                rgba.extend_from_slice(&[255, 255, 255, alpha]);
            }
        }
        SpriteMask::from_rgba(width, height, &rgba)
    }

    // Ein einzelner sichtbarer Pixel hat vier freie Kanten -> Vorder- und Rueckseite plus
    // vier Randstuecke = 6 Vierecke = 36 Vertices.
    #[test]
    fn einzelner_pixel_bekommt_ringsum_rand() {
        let mask = maske(&["...", ".#.", "..."]);
        let mut out = Vec::new();
        emit_sprite_item(&mask, [0.0, 0.0, 1.0, 1.0], Affine::translation([0.0; 3]), 1.0, &mut out);
        assert_eq!(out.len(), 6 * 6, "2 Flaechen + 4 Raender");
    }

    // Zwei benachbarte Pixel teilen sich eine Kante -- die darf KEINEN Rand bekommen, sonst
    // steckt mitten im Item eine Wand (und kostet doppelt Dreiecke).
    #[test]
    fn innenkanten_bekommen_keinen_rand() {
        let mask = maske(&["....", ".##.", "...."]);
        let mut out = Vec::new();
        emit_sprite_item(&mask, [0.0, 0.0, 1.0, 1.0], Affine::translation([0.0; 3]), 1.0, &mut out);
        // 2 Flaechen + 6 Aussenkanten (je Pixel 4 Kanten, 2 davon aneinander).
        assert_eq!(out.len(), 8 * 6);
    }

    // Ein voellig leeres Sprite darf nur die beiden (durchsichtigen) Flaechen liefern und
    // keinen Rand -- und any_opaque muss das melden, damit der Aufrufer es ganz weglassen kann.
    #[test]
    fn leeres_sprite_hat_keinen_rand() {
        let mask = maske(&["..", ".."]);
        assert!(!mask.any_opaque());
        let mut out = Vec::new();
        emit_sprite_item(&mask, [0.0, 0.0, 1.0, 1.0], Affine::translation([0.0; 3]), 1.0, &mut out);
        assert_eq!(out.len(), 2 * 6);
    }

    // Das Sprite steht aufrecht: die OBERSTE Bildzeile muss im Modell oben liegen. Ohne die
    // Y-Spiegelung in py() haengt jedes Item kopfueber in der Hand -- ein Fehler, den man im
    // Spiel sofort sieht, im Zahlenwerk aber leicht uebersieht.
    #[test]
    fn oberste_bildzeile_liegt_im_modell_oben() {
        let oben = maske(&["#", ".", "."]);
        let unten = maske(&[".", ".", "#"]);
        let mut a = Vec::new();
        let mut b = Vec::new();
        emit_sprite_item(&oben, [0.0, 0.0, 1.0, 1.0], Affine::translation([0.0; 3]), 1.0, &mut a);
        emit_sprite_item(&unten, [0.0, 0.0, 1.0, 1.0], Affine::translation([0.0; 3]), 1.0, &mut b);
        let hoechster = |v: &[BlockVertex]| v.iter().map(|x| x.position[1]).fold(f32::MIN, f32::max);
        // Die Flaechen decken beide das ganze Bild ab -- deshalb den RAND vergleichen: der
        // sitzt nur dort, wo der Pixel ist.
        let rand_y = |v: &[BlockVertex]| {
            let mittel: f32 = v.iter().skip(12).map(|x| x.position[1]).sum::<f32>() / (v.len() - 12) as f32;
            mittel
        };
        assert!(hoechster(&a) > 0.0);
        assert!(rand_y(&a) > rand_y(&b), "Pixel in der obersten Bildzeile muss im Modell hoeher liegen");
    }

    // Der Wuerfel eines Blockitems hat sechs Flaechen zu je zwei Dreiecken.
    #[test]
    fn blockitem_ist_ein_voller_wuerfel() {
        let mut out = Vec::new();
        emit_block_item(&[[0.0, 0.0, 1.0, 1.0]; 6], Affine::translation([0.0; 3]), 1.0, &mut out);
        assert_eq!(out.len(), 6 * 6);
        // ... und er fuellt genau den Einheitswuerfel aus.
        for achse in 0..3 {
            let min = out.iter().map(|v| v.position[achse]).fold(f32::MAX, f32::min);
            let max = out.iter().map(|v| v.position[achse]).fold(f32::MIN, f32::max);
            assert_eq!((min, max), (0.0, 1.0));
        }
    }
}

#[cfg(test)]
mod winding_tests {
    use super::*;
    use crate::entity_model::{Cuboid, Model, Part, Pose};

    // Geometrische Normale des ERSTEN Dreiecks eines Vierecks (Vertices 0,1,2).
    fn dreiecks_normale(v: &[BlockVertex], erster: usize) -> [f32; 3] {
        let p = |i: usize| v[erster + i].position;
        let (a, b, c) = (p(0), p(1), p(2));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]
    }

    // Zeigt die geometrische Normale in dieselbe Richtung wie die zum Shading benutzte?
    // Genau daran haengt das Rueckseiten-Culling: die Pipeline verwirft Dreiecke anhand der
    // Umlaufrichtung, nicht anhand unserer Shading-Normale. Laufen die beiden auseinander,
    // wird die Vorderseite weggeworfen und man sieht die Rueckseite.
    #[test]
    fn wuerfel_umlaufrichtung_zeigt_nach_aussen() {
        let mut out = Vec::new();
        emit_block_item(&[[0.0, 0.0, 1.0, 1.0]; 6], Affine::IDENTITY, 1.0, &mut out);
        // Reihenfolge der Flaechen in emit_block_item: down, up, north, south, west, east.
        let erwartet: [[f32; 3]; 6] =
            [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        for (flaeche, soll) in erwartet.iter().enumerate() {
            let ist = dreiecks_normale(&out, flaeche * 6);
            let punkt: f32 = ist.iter().zip(soll).map(|(a, b)| a * b).sum();
            assert!(punkt > 0.0, "Flaeche {flaeche}: Umlaufrichtung zeigt nach innen ({ist:?} gegen {soll:?})");
        }
    }

    // Dasselbe fuer ein flaches Item: ein einzelner Pixel hat Vorder-, Rueckseite und vier
    // Raender, und jede davon muss nach aussen gewickelt sein.
    #[test]
    fn sprite_umlaufrichtung_zeigt_nach_aussen() {
        let mask = SpriteMask::from_rgba(1, 1, &[255, 255, 255, 255]);
        let mut out = Vec::new();
        emit_sprite_item(&mask, [0.0, 0.0, 1.0, 1.0], Affine::IDENTITY, 1.0, &mut out);
        // Reihenfolge in emit_sprite_item: vorn, hinten, links, rechts, oben, unten.
        let erwartet: [[f32; 3]; 6] =
            [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0]];
        assert_eq!(out.len(), 6 * 6);
        for (flaeche, soll) in erwartet.iter().enumerate() {
            let ist = dreiecks_normale(&out, flaeche * 6);
            let punkt: f32 = ist.iter().zip(soll).map(|(a, b)| a * b).sum();
            assert!(punkt > 0.0, "Flaeche {flaeche}: Umlaufrichtung zeigt nach innen ({ist:?} gegen {soll:?})");
        }
    }

    // Die Randstreifen muessen den Texel ihres EIGENEN (deckenden) Pixels sampeln. Lag die
    // UV auf der Pixelgrenze, traf das Nearest-Sampling rechts/unten den durchsichtigen
    // Nachbarn, und der Alpha-Test verwarf den Streifen.
    #[test]
    fn randstreifen_sampeln_ihren_eigenen_pixel() {
        let mut rgba = vec![0u8; 3 * 3 * 4];
        rgba[(3 + 1) * 4 + 3] = 255; // nur der mittlere Pixel
        let mask = SpriteMask::from_rgba(3, 3, &rgba);
        let mut out = Vec::new();
        emit_sprite_item(&mask, [0.0, 0.0, 1.0, 1.0], Affine::IDENTITY, 1.0, &mut out);
        // Vorder- und Rueckseite ueberspringen, der Rest sind die vier Raender.
        for v in &out[12..] {
            let texel = ((v.uv[0] * 3.0).floor() as i32, (v.uv[1] * 3.0).floor() as i32);
            assert_eq!(texel, (1, 1), "Randstreifen sampelt {texel:?} statt den eigenen Pixel (uv {:?})", v.uv);
        }
    }

    // Und dieselbe Frage fuer die Entity-Modelle, gegen die sich das Item richten MUSS:
    // beide landen im selben Zeichenaufruf mit derselben Culling-Einstellung. Ein Kasten des
    // Spielermodells dient als Referenz.
    #[test]
    fn item_dreht_gleich_herum_wie_ein_entity_kasten() {
        let model = Model {
            texture: "",
            tex_size: [64.0, 64.0],
            parts: vec![Part::new("box", [0.0, 0.0, 0.0], vec![Cuboid::new([0.0, 0.0], [0.0, 0.0, 0.0], [16.0, 16.0, 16.0])])],
        };
        let mut kasten = Vec::new();
        crate::entity_model::emit(&model, &Pose::new(), Affine::IDENTITY, [0.0, 0.0, 1.0, 1.0], 1.0, &mut kasten);
        let mut item = Vec::new();
        emit_block_item(&[[0.0, 0.0, 1.0, 1.0]; 6], Affine::IDENTITY, 1.0, &mut item);

        // Die Flaeche mit der groessten Y-Ausdehnung nach oben in BEIDEN suchen und ihre
        // Umlaufrichtung vergleichen. Die Kasten-Reihenfolge in emit_cuboid ist
        // down, up, west, north, east, south -- "up" ist also das zweite Viereck.
        let kasten_up = dreiecks_normale(&kasten, 6);
        let item_up = dreiecks_normale(&item, 6);
        let gleich = kasten_up.iter().zip(item_up).map(|(a, b)| a * b).sum::<f32>() > 0.0;
        assert!(gleich, "Item-Wuerfel laeuft anders herum als ein Entity-Kasten: {item_up:?} gegen {kasten_up:?}");
    }
}

#[cfg(test)]
mod groessen_tests {
    use super::*;
    use crate::entity_model::{biped_pose, entity_transform, part_transform, rotation_x_deg, PLAYER_MODEL_SCALE};

    // Die TATSAECHLICHE Kantenlaenge eines Blockitems in der Hand eines Spielers, komplett
    // durch die echte Kette gerechnet. Vanillas Werte fuer block/block sind
    // rotation [75,45,0], translation [0,2.5,0] (Pixel) und scale 0.375; das Modell ist ein
    // voller Wuerfel von 0 bis 1. Zusammen mit PlayerEntityRenderer.scale() (0.9375) muss
    // daraus 0.375 * 0.9375 = 0.3516 Bloecke werden.
    //
    // Als Groessenvergleich steht der Kopf daneben: 8 Modellpixel = 0.5 * 0.9375 = 0.4688
    // Bloecke. Ein Block in der Hand ist also gut drei Viertel so breit wie ein Kopf -- wer
    // beim Spielen etwas deutlich Groesseres sieht, hat einen ANDEREN Fehler vor sich, nicht
    // diesen Massstab.
    #[test]
    fn blockitem_in_der_hand_hat_vanilla_groesse() {
        let model = crate::player_model(false);
        let pose = biped_pose(0.0, 0.0, 0.0, 0.0, false, 0.0);
        let base = entity_transform([0.0, 0.0, 0.0], 0.0, 1.0, PLAYER_MODEL_SCALE);
        let arm = part_transform(&model, &pose, base, "right_arm").expect("right_arm");

        let chain = arm
            .then(rotation_x_deg(-90.0))
            .then(Affine::rotation_y_deg(180.0))
            .then(Affine::translation([1.0 / 16.0, 0.125, -0.625]))
            .then(Affine::translation([0.0, 2.5 / 16.0, 0.0]))
            .then(Affine::rotation_xyz_deg(75.0, 45.0, 0.0))
            .then(Affine::scale([0.375; 3]))
            .then(Affine::translation([-0.5; 3]));

        let mut out = Vec::new();
        emit_block_item(&[[0.0, 0.0, 1.0, 1.0]; 6], chain, 1.0, &mut out);

        // Kantenlaenge ueber zwei BENACHBARTE Ecken messen (die Bounding-Box waere durch die
        // 75/45-Grad-Drehung groesser als die Kante selbst).
        //
        // Achtung auf die Vertex-Reihenfolge: push_quad gibt 0,2,1 / 0,3,2 aus (s. dortiger
        // Kommentar zum Culling). out[0] und out[1] sind damit die Ecken 0 und 2, also die
        // DIAGONALE der Flaeche -- wer die misst, bekommt die Kante mal Wurzel 2 und sucht
        // den Fehler anschliessend im falschen Code. Die Kante ist out[0] -> out[2].
        let kante = {
            let a = out[0].position;
            let b = out[2].position;
            ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
        };
        let erwartet = 0.375 * PLAYER_MODEL_SCALE;
        println!("Blockitem-Kante: {kante:.4} Bloecke (erwartet {erwartet:.4})");
        assert!((kante - erwartet).abs() < 1e-4, "Kantenlaenge {kante} statt {erwartet}");
    }
}
