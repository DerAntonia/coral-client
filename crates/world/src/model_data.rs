// Die Geometrie der Entity-Modelle (Kaesten, Drehpunkte, Grunddrehungen, Texturgroesse) als
// Daten statt als handgeschriebene Funktionen: je Modell-Layer (z.B. "villager#main") der
// ganze Teilebaum. Die Daten liegen NICHT im Repo -- assets::entity_model_data erzeugt sie aus
// dem Client-Jar der Installation des Nutzers, die App reicht sie per install() herein. Bis
// dahin (oder ohne Java) gibt es keine Modelle, und die betroffenen Entities bleiben Kaesten.
//
// Format, eine Zeile je Eintrag:
//   L <layer> <texbreite> <texhoehe>
//   P <tiefe> <name> <px py pz> <pitch yaw roll> <sx sy sz>     -- Teil, Kinder folgen tiefer
//   C <u v> <ox oy oz> <bx by bz> <dx dy dz> <tu tv> <mirror> <seitenmaske>
// Die Seitenmaske zaehlt unten, oben, Nord, Sued, West, Ost (Vanillas Direction-Reihenfolge).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::entity_model::{Cuboid, Model, Part};

static MODELS: OnceLock<HashMap<&'static str, Model>> = OnceLock::new();

// Die Modelldaten (s. Format oben) einmal einsetzen. false = schon geschehen.
pub fn install(text: &str) -> bool {
    MODELS.set(parse(text)).is_ok()
}

pub fn is_installed() -> bool {
    MODELS.get().is_some()
}

// Modell zu einem Layer-Namen ("villager#main"). Die Texturangabe ist leer -- welche Textur
// darauf kommt, entscheidet der Aufrufer.
pub fn layer_model(layer: &str) -> Option<&'static Model> {
    MODELS.get()?.get(layer)
}

// Teilnamen leben so lange wie das Programm (Part::name ist &'static str); gleiche Namen
// teilen sich einen String.
fn intern(name: &str, pool: &mut HashMap<String, &'static str>) -> &'static str {
    if let Some(s) = pool.get(name) {
        return s;
    }
    let s: &'static str = Box::leak(name.to_string().into_boxed_str());
    pool.insert(name.to_string(), s);
    s
}

// Seitenmaske aus der Datei (Direction-Reihenfolge) in die von emit_cuboid (unten, oben,
// West, Nord, Ost, Sued).
fn faces_from_directions(mask: u8) -> u8 {
    const ZIEL: [u8; 6] = [0, 1, 3, 5, 2, 4];
    (0..6).filter(|&i| mask & (1 << i) != 0).fold(0, |m, i| m | 1 << ZIEL[i])
}

fn parse(text: &str) -> HashMap<&'static str, Model> {
    let mut pool = HashMap::new();
    let mut models = HashMap::new();
    // Offene Teile von der Wurzel bis zum zuletzt gelesenen.
    let mut stack: Vec<Part> = Vec::new();
    let mut current: Option<(&'static str, [f32; 2])> = None;
    fn close_to(stack: &mut Vec<Part>, depth: usize) {
        while stack.len() > depth + 1 {
            let kind = stack.pop().expect("Teil");
            stack.last_mut().expect("Eltern").children.push(kind);
        }
    }
    let finish = |stack: &mut Vec<Part>, current: Option<(&'static str, [f32; 2])>, models: &mut HashMap<&'static str, Model>| {
        close_to(stack, 0);
        if let (Some((name, tex)), Some(root)) = (current, stack.pop()) {
            models.insert(name, Model { texture: "", tex_size: tex, parts: vec![root] });
        }
        stack.clear();
    };
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        match it.next() {
            Some("L") => {
                finish(&mut stack, current.take(), &mut models);
                let name = intern(it.next().unwrap_or(""), &mut pool);
                let w: f32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(64.0);
                let h: f32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(32.0);
                current = Some((name, [w, h]));
            }
            Some("P") => {
                let depth: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let name = intern(it.next().unwrap_or(""), &mut pool);
                let v: Vec<f32> = it.map(|x| x.parse().unwrap_or(0.0)).collect();
                if v.len() < 9 {
                    continue;
                }
                close_to(&mut stack, depth.saturating_sub(1));
                let mut part = Part::new(name, [v[0], v[1], v[2]], Vec::new()).rotated([v[3], v[4], v[5]]);
                part.scale = [v[6], v[7], v[8]];
                stack.push(part);
            }
            Some("C") => {
                let v: Vec<f32> = it.map(|x| x.parse().unwrap_or(0.0)).collect();
                if v.len() < 15 {
                    continue;
                }
                let mut c = Cuboid::new([v[0], v[1]], [v[2], v[3], v[4]], [v[5], v[6], v[7]]).with_tex_scale(v[11], v[12]);
                c.dilation = [v[8], v[9], v[10]];
                c.mirror = v[13] != 0.0;
                c.faces = faces_from_directions(v[14] as u8);
                if let Some(part) = stack.last_mut() {
                    part.cuboids.push(c);
                }
            }
            _ => {}
        }
    }
    finish(&mut stack, current.take(), &mut models);
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ein kleines Modell im Dateiformat: Wurzel mit Kopf (samt Nase) und Bein, eine Seite
    // weggelassen, ungleichmaessig gedehnt und skaliert.
    const PROBE: &str = "L probe#main 64 32\n\
P 0 root 0 1.5 0 0 0 0 0.5 0.5 0.5\n\
P 1 head 0 0 0 0.1 0 0 1 1 1\n\
C 0 0 -4 -8 -4 8 8 8 0 0 0 1 1 0 63\n\
P 2 nose 0 -2 0 0 0 0 1 1 1\n\
C 24 0 -1 -1 -6 2 4 2 0.1 0.2 0.3 1 1 1 4\n\
P 1 leg 2 12 0 0 0 0 1 1 1\n\
C 0 16 -2 0 -2 4 12 4 0 0 0 1 1 0 63\n\
L zweites#main 16 16\n\
P 0 root 0 0 0 0 0 0 1 1 1\n";

    #[test]
    fn baum_kaesten_und_felder() {
        let m = parse(PROBE);
        assert_eq!(m.len(), 2);
        let p = &m["probe#main"];
        assert_eq!(p.tex_size, [64.0, 32.0]);
        let root = &p.parts[0];
        assert_eq!(root.scale, [0.5; 3]);
        assert_eq!(root.children.iter().map(|c| c.name).collect::<Vec<_>>(), ["head", "leg"]);
        let kopf = &root.children[0];
        assert_eq!(kopf.rotation, [0.1, 0.0, 0.0]);
        let nase = &kopf.children[0].cuboids[0];
        assert_eq!(nase.dilation, [0.1, 0.2, 0.3]);
        assert!(nase.mirror);
        assert_eq!(nase.faces, 1 << 3, "nur Nord (Direction-Bit 2) -> emit_cuboid-Bit 3");
        assert_eq!(root.children[1].cuboids[0].size, [4.0, 12.0, 4.0]);
        assert!(m["zweites#main"].parts[0].children.is_empty());
    }

    #[test]
    fn seitenmaske() {
        assert_eq!(faces_from_directions(0b11_1111), 0b11_1111);
        assert_eq!(faces_from_directions(1 << 4), 1 << 2, "West");
        assert_eq!(faces_from_directions(1 << 5), 1 << 4, "Ost");
    }
}
