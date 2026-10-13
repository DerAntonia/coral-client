// Die echten Vanilla-Kollisions- und Umrissformen, aus vanilla_collision_data.txt (Dump aus
// dem 1.21.11-Jar, s. Kopf jener Datei).
//
// Das ersetzt die bisherige Herleitung aus den Render-Modellen. Der Unterschied ist kein
// Schoenheitsfehler: Vanillas getCollisionShape ist bei etlichen Bloecken bewusst eine ANDERE
// Form als das Modell, und jede Abweichung ist ein dauerhafter Versatz gegenueber der
// Server-Simulation. GrimAC rechnet mit den echten Formen und meldete uns dafuer Simulation
// (Offsets von exakt 1/16 und 2/16), Phase und GroundSpoof.
//
// Die Tabelle wird beim ersten Zugriff einmal geparst und danach nur noch gelesen.

use crate::block_shapes::ShapeBox;
use std::sync::OnceLock;

const DATA: &str = include_str!("vanilla_collision_data.txt");

pub struct VanillaShapes {
    // Alle vorkommenden Formen, entdoppelt. Index 0 = Vollwuerfel, 1 = leer.
    table: Vec<Vec<ShapeBox>>,
    // Je State-ID der Index in `table`. Kein Eintrag (kurzer Vektor) = Vollwuerfel.
    collision: Vec<u16>,
    outline: Vec<u16>,
}

fn parse() -> VanillaShapes {
    let mut table: Vec<Vec<ShapeBox>> = Vec::new();
    let mut collision: Vec<u16> = Vec::new();
    let mut outline: Vec<u16> = Vec::new();

    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_ascii_whitespace();
        match it.next() {
            Some("S") => {
                let index: usize = it.next().and_then(|v| v.parse().ok()).expect("S-Zeile ohne Index");
                let zahlen: Vec<f32> = it.map(|v| v.parse().expect("ungueltige Zahl in S-Zeile")).collect();
                assert!(zahlen.len().is_multiple_of(6), "S {index}: Zahlen nicht durch 6 teilbar");
                let boxen = zahlen
                    .chunks_exact(6)
                    .map(|b| ShapeBox {
                        min: [b[0], b[1], b[2]],
                        max: [b[3], b[4], b[5]],
                        faces: [true; 6],
                        content: [false; 6],
                        uv: [None; 6],
                    })
                    .collect();
                // Die Indizes kommen aufsteigend und lueckenlos aus dem Dumper.
                assert_eq!(index, table.len(), "Formtabelle hat eine Luecke bei {index}");
                table.push(boxen);
            }
            Some("M") => {
                let state: usize = it.next().and_then(|v| v.parse().ok()).expect("M-Zeile ohne State");
                let ci: u16 = it.next().and_then(|v| v.parse().ok()).expect("M-Zeile ohne Kollisionsindex");
                let oi: u16 = it.next().and_then(|v| v.parse().ok()).expect("M-Zeile ohne Umrissindex");
                if collision.len() <= state {
                    collision.resize(state + 1, 0);
                    outline.resize(state + 1, 0);
                }
                collision[state] = ci;
                outline[state] = oi;
            }
            _ => panic!("unbekannte Zeile in vanilla_collision_data.txt: {line}"),
        }
    }
    VanillaShapes { table, collision, outline }
}

fn shapes() -> &'static VanillaShapes {
    static SHAPES: OnceLock<VanillaShapes> = OnceLock::new();
    SHAPES.get_or_init(parse)
}

// Kennen wir diese State-ID ueberhaupt? States jenseits der Tabelle (ein Server mit anderer
// Version, ein Sentinel) haben keine gedumpte Form -- dort bleibt es beim bisherigen Weg.
pub fn is_known(state: u32) -> bool {
    (state as usize) < shapes().collision.len()
}

// Vanillas getCollisionShape(EmptyBlockGetter, ZERO) fuer diesen State. Leerer Schnipsel =
// begehbar.
pub fn collision_boxes(state: u32) -> Option<&'static [ShapeBox]> {
    let s = shapes();
    let idx = *s.collision.get(state as usize)?;
    Some(&s.table[idx as usize])
}

// Vanillas getShape (Umriss) -- fuers Zielen. Bei vielen Bloecken dieselbe Form wie oben, bei
// Seelensand/Kaktus/Lesepult ausdruecklich nicht.
pub fn outline_boxes(state: u32) -> Option<&'static [ShapeBox]> {
    let s = shapes();
    let idx = *s.outline.get(state as usize)?;
    Some(&s.table[idx as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zustand(name: &str) -> u32 {
        (0..crate::block_registry::state_count() as u32)
            .find(|&s| crate::block_registry::block_name(s) == Some(name))
            .unwrap_or_else(|| panic!("Block {name} nicht in der Registry"))
    }

    fn hoehe(state: u32) -> f32 {
        collision_boxes(state).unwrap().iter().map(|b| b.max[1]).fold(0.0f32, f32::max)
    }

    // Die Tabelle muss ueberhaupt zu UNSEREN State-IDs passen -- ein Versatz waere der
    // schlimmste denkbare Fehler (jeder Block bekaeme die Form eines anderen).
    #[test]
    fn die_tabelle_deckt_alle_unsere_states_ab() {
        assert!(shapes().collision.len() >= crate::block_registry::state_count(), "Dump kennt weniger States als die Registry");
        assert!(is_known(0), "Luft fehlt");
        assert!(is_known(crate::block_registry::state_count() as u32 - 1), "letzter State fehlt");
    }

    // Genau die Faelle, bei denen GrimAC geflaggt hat. Die Zahlen stehen so im Vanilla-Quelltext
    // (SoulSandBlock 14/16, FarmBlock 15/16, CactusBlock 1..15 auf 15/16 Hoehe).
    #[test]
    fn die_geflaggten_bloecke_haben_ihre_echte_form() {
        assert_eq!(hoehe(zustand("soul_sand")), 0.875);
        assert_eq!(hoehe(zustand("farmland")), 0.9375);
        assert_eq!(hoehe(zustand("dirt_path")), 0.9375);
        assert_eq!(hoehe(zustand("mud")), 0.875);

        let kaktus = collision_boxes(zustand("cactus")).unwrap();
        assert_eq!(kaktus.len(), 1);
        assert_eq!(kaktus[0].min, [0.0625, 0.0, 0.0625]);
        assert_eq!(kaktus[0].max, [0.9375, 0.9375, 0.9375]);
        // Der Umriss ist NICHT dieselbe Form -- er geht bis ganz oben.
        assert_eq!(outline_boxes(zustand("cactus")).unwrap()[0].max[1], 1.0);

        // Das Lesepult: zwei Kaesten, nicht der Vollwuerfel-Notnagel, auf den sein gedrehtes
        // Modell uns bisher zurueckfallen liess.
        let pult = collision_boxes(zustand("lectern")).unwrap();
        assert_eq!(pult.len(), 2);
        assert_eq!(hoehe(zustand("lectern")), 0.875);
    }

    // Stein bleibt ein Vollwuerfel, eine Blume bleibt begehbar -- die beiden Massenfaelle.
    #[test]
    fn vollwuerfel_und_leer_bleiben_was_sie_sind() {
        let stein = collision_boxes(zustand("stone")).unwrap();
        assert_eq!(stein.len(), 1);
        assert_eq!(stein[0].min, [0.0, 0.0, 0.0]);
        assert_eq!(stein[0].max, [1.0, 1.0, 1.0]);
        assert!(collision_boxes(zustand("dandelion")).unwrap().is_empty());
        assert!(collision_boxes(crate::chunk::STATE_AIR).unwrap().is_empty());
    }
}
