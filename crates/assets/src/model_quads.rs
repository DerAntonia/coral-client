// Item-Modelle aus Elementen ("elements" im Modell-JSON) als fertige Flaechen-Vierecke.
//
// Vanilla-Items sind fast immer flach (item/generated) oder ein Blockwuerfel; Resourcepacks
// ersetzen aber gern einzelne Items durch echte 3D-Modelle (etwa der End-Kristall aus
// einem Crystal-PvP-Pack: drei Wuerfel, zwei davon um 45 Grad gedreht). Hier wird ein solches
// Modell einmal aufgeloest -- Hand (app::held_item) und Inventar-Symbol (app::item_icons)
// zeichnen dieselben Vierecke, nur verschieden eingebettet.
//
// Nachgebaut ist Vanillas BakedQuadFactory, soweit Items es brauchen: Elementdrehung um
// eine Achse (samt "rescale"), Flaechen-UV (explizit oder Vanillas Vorgabe), Texturvariablen.
// Nicht nachgebaut: die UV-Drehung einer Flaeche ("rotation" in der Flaeche) und
// "shade": false -- beides selten bei Items.

use crate::blocks::{load_model_chain, resolve_var, strip_ns, Element};
use crate::installation::MinecraftInstallation;
use crate::shapes::default_face_uv;

// Eine Flaeche: Ecken im Modellraum in BLOECKEN (0..1 fuer ein 16er-Element), Reihenfolge
// links oben, rechts oben, rechts unten, links unten von aussen gesehen -- dieselbe wie
// world::item_model::emit_block_item. `uv` in Texturpixeln (0..16), zu den Ecken gehoeren
// (u0,v0), (u1,v0), (u1,v1), (u0,v1).
#[derive(Clone, Debug, PartialEq)]
pub struct ModelQuad {
    pub corners: [[f32; 3]; 4],
    pub normal: [f32; 3],
    pub uv: [f32; 4],
    // Texturpfad unter textures/, ohne Endung -- z.B. "item/end_crystal".
    pub texture: String,
    // FACE_* der Flaeche VOR der Elementdrehung (fuer die feste Flaechenhelligkeit).
    pub face: usize,
    // Drehung des Texturausschnitts in 90-Grad-Schritten (Flaechen-"rotation"/90).
    pub uv_rotation: u32,
    // false = "shade": false im Element -- ohne Richtungshelligkeit zeichnen.
    pub shade: bool,
    // tintindex der Flaeche: welcher Eintrag der "tints"-Liste der Item-Definition sie faerbt.
    pub tint_index: Option<i64>,
}

// Die Vierecke des Item-Modells, oder None, wenn das Modell keine eigenen Elemente hat
// (flache Items, Blockitems, Sondermodelle).
pub fn item_model_quads(mc: &mut MinecraftInstallation, model: &str) -> Option<Vec<ModelQuad>> {
    let model = strip_ns(model).to_string();
    if model.starts_with("block/") {
        return None;
    }
    let (textures, elements) = load_model_chain(mc, &model).ok()?;
    let elements = elements.filter(|e| !e.is_empty())?;
    let mut out = Vec::new();
    for element in &elements {
        element_quads(element, &textures, &mut out);
    }
    (!out.is_empty()).then_some(out)
}

// Die Vierecke eines BLOCK-Modells samt Blockstate-Drehung (x zuerst, dann y, je in
// 90-Grad-Schritten um die Blockmitte -- dieselbe Konvention wie shapes::rotate_box). Fuer
// Modelle mit gedrehten Elementen (Feuer, Laterne, Kerzen, ...), die sich nicht als
// achsenparallele Boxen darstellen lassen. None = Modell nicht ladbar oder ohne Elemente.
pub(crate) fn block_model_quads(mc: &mut MinecraftInstallation, model: &str, x_deg: i32, y_deg: i32) -> Option<Vec<ModelQuad>> {
    let model = strip_ns(model).to_string();
    let (textures, elements) = load_model_chain(mc, &model).ok()?;
    let elements = elements.filter(|e| !e.is_empty())?;
    let mut out = Vec::new();
    for element in &elements {
        element_quads(element, &textures, &mut out);
    }
    let x_steps = (x_deg / 90).rem_euclid(4);
    let y_steps = (y_deg / 90).rem_euclid(4);
    // Ein 90-Grad-Schritt um die Blockmitte, in Bloecken (0..1): s. shapes::rotate_pair(_x).
    let dreh_x = |p: [f32; 3]| [p[0], p[2], 1.0 - p[1]];
    let dreh_y = |p: [f32; 3]| [1.0 - p[2], p[1], p[0]];
    // Normalen drehen ohne die Verschiebung um die Mitte.
    let dreh_x_n = |n: [f32; 3]| [n[0], n[2], -n[1]];
    let dreh_y_n = |n: [f32; 3]| [-n[2], n[1], n[0]];
    for q in &mut out {
        for _ in 0..x_steps {
            q.corners = q.corners.map(dreh_x);
            q.normal = dreh_x_n(q.normal);
        }
        for _ in 0..y_steps {
            q.corners = q.corners.map(dreh_y);
            q.normal = dreh_y_n(q.normal);
        }
    }
    (!out.is_empty()).then_some(out)
}

// Ob eines der Elemente eines Modells eine eigene Drehung traegt.
pub(crate) fn model_has_rotated_element(mc: &mut MinecraftInstallation, model: &str) -> bool {
    load_model_chain(mc, strip_ns(model)).ok().and_then(|(_, e)| e).is_some_and(|e| e.iter().any(|el| el.rotation.is_some()))
}

// Alle Texturpfade eines Modells (aufgeloeste Flaechen-Texturvariablen).
pub(crate) fn model_textures(mc: &mut MinecraftInstallation, model: &str) -> Vec<String> {
    let Ok((textures, Some(elements))) = load_model_chain(mc, strip_ns(model)) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for e in &elements {
        for face in e.faces.values() {
            if let Some(t) = resolve_var(&textures, face.texture.clone()) {
                let t = strip_ns(&t).to_string();
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
    }
    out
}

const FACE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const NORMALS: [[f32; 3]; 6] = [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];

fn element_quads(e: &Element, textures: &std::collections::HashMap<String, String>, out: &mut Vec<ModelQuad>) {
    let [x0, y0, z0] = e.from.map(|v| v / 16.0);
    let [x1, y1, z1] = e.to.map(|v| v / 16.0);
    let c = |x: f32, y: f32, z: f32| [x, y, z];
    // Wie emit_block_item: je Flaeche die vier Ecken, von aussen gesehen im Uhrzeigersinn.
    let geometrie: [[[f32; 3]; 4]; 6] = [
        [c(x0, y0, z1), c(x1, y0, z1), c(x1, y0, z0), c(x0, y0, z0)],
        [c(x0, y1, z0), c(x1, y1, z0), c(x1, y1, z1), c(x0, y1, z1)],
        [c(x1, y1, z0), c(x0, y1, z0), c(x0, y0, z0), c(x1, y0, z0)],
        [c(x0, y1, z1), c(x1, y1, z1), c(x1, y0, z1), c(x0, y0, z1)],
        [c(x0, y1, z0), c(x0, y1, z1), c(x0, y0, z1), c(x0, y0, z0)],
        [c(x1, y1, z1), c(x1, y1, z0), c(x1, y0, z0), c(x1, y0, z1)],
    ];
    let drehung = e.rotation.as_ref().and_then(ElementRotation::parse);
    for (i, name) in FACE_NAMES.iter().enumerate() {
        let Some(face) = e.faces.get(*name) else { continue };
        let Some(texture) = resolve_var(textures, face.texture.clone()) else { continue };
        let uv = face.uv.unwrap_or_else(|| default_face_uv(i, e.from, e.to));
        let mut corners = geometrie[i];
        let mut normal = NORMALS[i];
        if let Some(d) = &drehung {
            corners = corners.map(|p| d.apply(p));
            normal = d.rotate(normal);
        }
        let uv_rotation = (face.rotation.unwrap_or(0).rem_euclid(360) / 90) as u32;
        out.push(ModelQuad { corners, normal, uv, texture: strip_ns(&texture).to_string(), face: i, uv_rotation, shade: e.shade, tint_index: face.tintindex });
    }
}

// Elementdrehung: Winkel um EINE Achse durch `origin` (Pixel). `rescale` streckt die beiden
// anderen Achsen um 1/cos(Winkel), damit ein um 45 Grad gedrehtes Element die Zelle wieder
// ausfuellt (BakedQuadFactory.rotateVertex).
struct ElementRotation {
    axis: usize,
    angle: f32,
    origin: [f32; 3],
    rescale: bool,
}

impl ElementRotation {
    fn parse(v: &serde_json::Value) -> Option<Self> {
        let axis = match v.get("axis")?.as_str()? {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            _ => return None,
        };
        let angle = v.get("angle")?.as_f64()? as f32;
        let origin = match v.get("origin").and_then(|o| o.as_array()) {
            Some(a) if a.len() == 3 => [0, 1, 2].map(|i| a[i].as_f64().unwrap_or(8.0) as f32 / 16.0),
            _ => [0.5; 3],
        };
        let rescale = v.get("rescale").and_then(|r| r.as_bool()).unwrap_or(false);
        Some(Self { axis, angle, origin, rescale })
    }

    // Rechtshaendige Drehung um die Achse (RotationAxis.POSITIVE_X/Y/Z.rotationDegrees).
    fn rotate(&self, p: [f32; 3]) -> [f32; 3] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let [x, y, z] = p;
        match self.axis {
            0 => [x, y * c - z * s, y * s + z * c],
            1 => [x * c + z * s, y, -x * s + z * c],
            _ => [x * c - y * s, x * s + y * c, z],
        }
    }

    fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        let rel = [p[0] - self.origin[0], p[1] - self.origin[1], p[2] - self.origin[2]];
        let mut r = self.rotate(rel);
        if self.rescale {
            let f = 1.0 / self.angle.to_radians().cos().abs().max(1e-3);
            for (i, v) in r.iter_mut().enumerate() {
                if i != self.axis {
                    *v *= f;
                }
            }
        }
        [r[0] + self.origin[0], r[1] + self.origin[1], r[2] + self.origin[2]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drehung_um_y_ist_rechtshaendig() {
        let d = ElementRotation { axis: 1, angle: 90.0, origin: [0.0; 3], rescale: false };
        // +X dreht um +Y nach -Z.
        let p = d.rotate([1.0, 0.0, 0.0]);
        assert!((p[0]).abs() < 1e-5 && (p[2] + 1.0).abs() < 1e-5, "{p:?}");
    }

    #[test]
    fn drehung_um_den_mittelpunkt_laesst_ihn_stehen() {
        let d = ElementRotation { axis: 0, angle: -45.0, origin: [0.5; 3], rescale: false };
        assert_eq!(d.apply([0.5, 0.5, 0.5]), [0.5, 0.5, 0.5]);
    }
}

#[cfg(test)]
mod pack_namespace_tests {
    // Ein Item-Modell in einem eigenen Namespace eines Resourcepacks: die UHC-Sturmwand
    // (assets/uhc/items/storm_wall.json -> uhc:item/storm_wall, ein Quad mit tintindex 0 und
    // Trankfarbe). Ohne Installation oder Pack wird nichts geprueft.
    #[test]
    fn modell_aus_fremdem_namespace_mit_toenung() {
        let pack = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Dokumente/uhc-battle-royale/docs/resourcepack");
        if !pack.exists() {
            return;
        }
        let Ok(mut mc) = crate::MinecraftInstallation::locate("1.21.11", vec![pack]) else { return };
        let Some(crate::ItemWorldTextures::Model(quads)) = crate::item_world_textures(&mut mc, "uhc:storm_wall") else {
            panic!("uhc:storm_wall nicht als Modell aufgeloest");
        };
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].tint_index, Some(0));
        assert_eq!(quads[0].texture, "block/white_wool");
        let tints = crate::item_tints(&mut mc, "uhc:storm_wall");
        assert_eq!(tints.len(), 1);
        assert!(tints[0].1, "Lage 0 traegt die Trankfarbe");
    }
}
