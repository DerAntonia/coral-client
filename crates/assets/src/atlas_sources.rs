// Sprite-Quellen der Atlas-Definitionen (`assets/<ns>/atlases/<name>.json`).
//
// In Vanilla ist der Name eines Sprites nicht zwingend der Pfad seiner Datei: die Atlas-
// Definition sagt, woher die Sprites kommen. Fast immer ist das ein ganzer Ordner unter
// gleichem Namen ("directory"), dann sind Sprite und Datei dasselbe. Packs koennen aber auch
// einzelne Dateien unter anderem Namen einhaengen ("single") oder ein Sammelbild in Stuecke
// schneiden ("unstitch") -- die Heldeninsel liefert so alle Item-Texturen als wenige grosse
// Bilder. Ohne das fand der Client die Sprites nicht und zeigte das Vanilla-Item.
//
// Hier steht nur die Zuordnung Sprite -> Quelle; ausgeschnitten wird in
// MinecraftInstallation::load_texture.

use std::collections::HashMap;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpriteSource {
    // Eine Datei unter anderem Namen (virtueller Pfad wie fuer load_bytes).
    File(String),
    // Ein Ausschnitt eines Sammelbildes: Bildpfad und Rechteck in Einheiten des Teilers --
    // ein Bild doppelter Aufloesung liefert denselben Ausschnitt doppelt so fein.
    Region { image: String, divisor: (f64, f64), rect: (f64, f64, f64, f64) },
}

#[derive(Default, Debug)]
pub(crate) struct AtlasSources {
    sprites: HashMap<String, SpriteSource>,
    // "directory"-Quellen: (Ordner unter textures/, Praefix des Sprite-Namens).
    directories: Vec<(String, String)>,
}

// "ns:pfad" bzw. "pfad" -> ("ns", "pfad").
fn split_id(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("minecraft", id))
}

fn full_id(id: &str) -> String {
    let (ns, p) = split_id(id);
    format!("{ns}:{p}")
}

fn texture_path(id: &str) -> String {
    let (ns, p) = split_id(id);
    format!("{ns}/textures/{p}.png")
}

impl AtlasSources {
    // Eine Atlas-Datei einlesen (mehrere Dateien desselben Atlas: von unten nach oben, die
    // Quellen haengen sich aneinander wie in Vanilla).
    pub(crate) fn add_file(&mut self, json: &[u8]) {
        let Ok(v) = serde_json::from_slice::<Value>(crate::installation::strip_bom(json)) else { return };
        let Some(quellen) = v.get("sources").and_then(Value::as_array) else { return };
        for q in quellen {
            let typ = q.get("type").and_then(Value::as_str).map(|t| t.strip_prefix("minecraft:").unwrap_or(t));
            match typ {
                Some("single") => {
                    let Some(res) = q.get("resource").and_then(Value::as_str) else { continue };
                    let sprite = q.get("sprite").and_then(Value::as_str).unwrap_or(res);
                    if full_id(sprite) != full_id(res) {
                        self.sprites.insert(full_id(sprite), SpriteSource::File(texture_path(res)));
                    }
                }
                Some("directory") => {
                    let (Some(src), Some(prefix)) = (q.get("source").and_then(Value::as_str), q.get("prefix").and_then(Value::as_str)) else { continue };
                    let src = src.trim_end_matches('/').to_string();
                    // Ordner und Praefix gleich: Sprite-Name ist der Dateipfad, nichts zu tun.
                    if format!("{src}/") != prefix {
                        self.directories.push((src, prefix.to_string()));
                    }
                }
                Some("unstitch") => {
                    let Some(res) = q.get("resource").and_then(Value::as_str) else { continue };
                    let teiler = |k: &str| q.get(k).and_then(Value::as_f64).filter(|d| *d > 0.0).unwrap_or(1.0);
                    let divisor = (teiler("divisor_x"), teiler("divisor_y"));
                    for r in q.get("regions").and_then(Value::as_array).into_iter().flatten() {
                        let Some(sprite) = r.get("sprite").and_then(Value::as_str) else { continue };
                        let zahl = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                        let rect = (zahl("x"), zahl("y"), zahl("width"), zahl("height"));
                        self.sprites.insert(full_id(sprite), SpriteSource::Region { image: texture_path(res), divisor, rect });
                    }
                }
                _ => {}
            }
        }
    }

    // Woher der Sprite zu diesem Texturpfad ("ns/textures/pfad.png") kommt, wenn nicht aus
    // genau dieser Datei.
    pub(crate) fn lookup(&self, virtual_path: &str) -> Vec<SpriteSource> {
        let Some((ns, rest)) = virtual_path.split_once("/textures/") else { return Vec::new() };
        let Some(pfad) = rest.strip_suffix(".png") else { return Vec::new() };
        let mut out = Vec::new();
        if let Some(q) = self.sprites.get(&format!("{ns}:{pfad}")) {
            out.push(q.clone());
        }
        for (src, prefix) in &self.directories {
            if let Some(r) = pfad.strip_prefix(prefix.as_str()) {
                out.push(SpriteSource::File(format!("{ns}/textures/{src}/{r}.png")));
            }
        }
        out
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sprites.is_empty() && self.directories.is_empty()
    }
}

// Den Ausschnitt in Bildpixeln (Vanillas Unstitcher: Teiler -> Bildgroesse, abgerundet).
pub(crate) fn region_pixels(image: (u32, u32), divisor: (f64, f64), rect: (f64, f64, f64, f64)) -> (u32, u32, u32, u32) {
    let sx = image.0 as f64 / divisor.0;
    let sy = image.1 as f64 / divisor.1;
    let x = (rect.0 * sx).floor().max(0.0) as u32;
    let y = (rect.1 * sy).floor().max(0.0) as u32;
    let w = ((rect.2 * sx).floor() as u32).min(image.0.saturating_sub(x));
    let h = ((rect.3 * sy).floor() as u32).min(image.1.saturating_sub(y));
    (x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unstitch_single_und_directory() {
        let mut a = AtlasSources::default();
        a.add_file(
            br#"{"sources":[
                {"type":"directory","source":"item","prefix":"item/"},
                {"type":"directory","source":"eigen","prefix":"sachen/"},
                {"type":"single","resource":"x:bild/a","sprite":"x:item/b"},
                {"type":"minecraft:unstitch","resource":"4d:atlas/od/.../v7/.","divisor_x":768.0,"divisor_y":752.0,
                 "regions":[{"sprite":"ok:item/f8/.../50/.","x":256,"y":128,"width":128,"height":128}]}
            ]}"#,
        );
        assert!(a.lookup("minecraft/textures/item/stone.png").is_empty(), "Ordner gleich Praefix: keine Umleitung");
        assert_eq!(a.lookup("minecraft/textures/sachen/kiste.png"), vec![SpriteSource::File("minecraft/textures/eigen/kiste.png".into())]);
        assert_eq!(a.lookup("x/textures/item/b.png"), vec![SpriteSource::File("x/textures/bild/a.png".into())]);
        assert_eq!(
            a.lookup("ok/textures/item/f8/.../50/..png"),
            vec![SpriteSource::Region { image: "4d/textures/atlas/od/.../v7/..png".into(), divisor: (768.0, 752.0), rect: (256.0, 128.0, 128.0, 128.0) }]
        );
        assert_eq!(region_pixels((1536, 1504), (768.0, 752.0), (256.0, 128.0, 128.0, 128.0)), (512, 256, 256, 256));
    }
}
