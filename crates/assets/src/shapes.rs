// Loest pro Block-State seine tatsaechliche Form auf: eine Liste achsenparalleler Boxen
// (in 0..16-Blocklokal-Einheiten), aus den echten Vanilla-Blockstate-/Modell-JSONs, nicht
// aus einer von Hand kuratierten Liste. Ersetzt die bisherige Annahme "jeder solide Block
// ist ein 1x1x1-Wuerfel" fuer Rendering UND Kollision.
//
// Ablauf pro State:
//   1) Blockstate-JSON laden (einmal pro Blockname, nicht pro State).
//   2) Variante (`variants`) oder Multipart-Faelle (`multipart`) gegen die echten
//      Property-Werte DIESES States matchen (s. matches_selector/when_matches) --
//      Variant-Keys duerfen Properties auslassen, die die Form nicht beeinflussen
//      (z.B. "type=bottom" bei einer Stufe, "waterlogged" bleibt unerwaehnt).
//   3) Fuer jedes matchende Modell (bei Multipart potenziell mehrere gleichzeitig, z.B.
//      Zaunpfosten + N Seitenarme) die Elements aus der Parent-Kette holen und um die im
//      Blockstate angegebene x/y-Rotation drehen (in 90-Grad-Schritten, s. rotate_*).
//
// Bewusst NICHT unterstuetzt: Elemente mit eigener "rotation" (z.B. die diagonalen
// Kreuz-Ebenen von Blumen/Gras) -- die lassen sich nicht als achsenparallele Box
// darstellen. Ein State, dessen Modell so ein Element enthaelt, bekommt gar keine Boxen
// (leerer Vec) und faellt beim Aufrufer auf den bisherigen Vollwuerfel zurueck (s.
// world::block_shapes-Kommentar).

use std::collections::HashMap;

use crate::blocks::{is_degenerate_decoration, load_model_chain, model_uses_ambient_occlusion, strip_ns, BlockstateJson, Element};
use crate::installation::MinecraftInstallation;

// Face-Index-Konvention wie in blocks.rs (FACE_DOWN=0 .. FACE_EAST=5) -- ElementBox.faces
// ist danach indiziert, damit der Aufrufer dieselben FACE_*-Konstanten wiederverwenden kann.
const FACE_ORDER: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementBox {
    // 0..1 normiert (Element-Koordinaten waren 0..16).
    pub min: [f32; 3],
    pub max: [f32; 3],
    // Ob dieses Modell fuer die jeweilige Richtung ueberhaupt eine Flaeche deklariert --
    // manche Elemente (z.B. der Zaunpfosten an der Stelle, wo ein Arm andockt) lassen eine
    // Seite bewusst weg, weil sie nie sichtbar ist.
    pub faces: [bool; 6],
    // Ob die jeweilige Flaeche im Modell eine eigene "tintindex"-Textur traegt, die NICHT
    // mit der normalen Block-Flaechentextur identisch ist -- z.B. der Kesselinhalt (Wasser/
    // Lava/Pulverschnee: eigenes, kleineres Element NUR mit einer Oben-Flaeche, s.
    // template_cauldron_full.json) im Unterschied zu grass_block, wo Basis- und Overlay-
    // Element exakt dieselbe Box sind (dort wird die Ueberlagerung schon beim Atlas-Bau in
    // die Textur hineingerechnet, s. assets::blocks::composite_overlay -- keine eigene
    // Geometrie noetig). Fuer als "content" markierte Flaechen schlaegt der Mesher statt der
    // gewoehnlichen Block-Flaechen-UV die eigene `content_uvs`-Kachel nach (s. world::BlockUvs).
    pub content: [bool; 6],
    // Texturausschnitt (0..1 normiert, aus "uv":[x0,y0,x1,y1]/16.0) INNERHALB der Flaechen-
    // Kachel -- None = volle Kachel. S. world::ShapeBox::uv (identische Bedeutung, dorthin
    // 1:1 durchgereicht) fuer den vollen Kommentar zu Zweck/Vorzeichen.
    pub uv: [Option<[f32; 4]>; 6],
}

// Pro State-ID die Liste ihrer Boxen. Fehlt ein Eintrag (oder ist er leer), konnte keine
// Form aufgeloest werden -- der Aufrufer nimmt dann den Vollwuerfel-Fallback.
pub fn build(mc: &mut MinecraftInstallation, states: &[(u32, &str, &[(&str, &str)])]) -> HashMap<u32, Vec<ElementBox>> {
    // Nach Blockname gruppieren, damit die Blockstate-JSON pro Block nur einmal geladen wird.
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }

    let mut result = HashMap::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };

        // Modell-Chains innerhalb dieses Blocks cachen: mehrere States (z.B. verschiedene
        // waterlogged-Werte) referenzieren oft dasselbe Modell erneut.
        let mut model_cache: HashMap<String, Option<Vec<Element>>> = HashMap::new();
        let mut load_elements = |mc: &mut MinecraftInstallation, model: &str| -> Option<Vec<Element>> {
            if let Some(cached) = model_cache.get(model) {
                return cached.clone();
            }
            let elements = load_model_chain(mc, model).ok().and_then(|(_, e)| e);
            model_cache.insert(model.to_string(), elements.clone());
            elements
        };

        for (state, props) in entries {
            if let Some(boxes) = resolve_state_shape(mc, &bs, props, &mut load_elements) {
                if !boxes.is_empty() {
                    result.insert(state, boxes);
                }
            }
        }
    }
    result
}

// Welche States Vanillas Ambient Occlusion ABSCHALTEN, weil ihr Modell es tut
// ("ambientocclusion": false irgendwo in der Parent-Kette).
//
// Massgeblich ist, wie in Vanilla, das ERSTE Modell des States: BlockModelRenderer.render
// fragt `parts.getFirst().useAmbientOcclusion()`. Betroffen sind u.a. Tueren, Leitern,
// Glasscheiben, Gitter, Trichter, Kessel, Schienen und Redstone -- Modelle, deren duenne
// Geometrie mit Eckabdunklung falsch aussaehe.
pub fn build_ao_disabled(mc: &mut MinecraftInstallation, states: &[(u32, &str, &[(&str, &str)])]) -> std::collections::HashSet<u32> {
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }
    let mut cache: HashMap<String, bool> = HashMap::new();
    let mut result = std::collections::HashSet::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };
        for (state, props) in entries {
            let erstes: Option<&serde_json::Value> = if !bs.variants.is_empty() {
                bs.variants.iter().find(|(sel, _)| matches_selector(sel, props)).map(|(_, a)| a)
            } else {
                bs.multipart.iter().find(|c| c.when.as_ref().is_none_or(|w| when_matches(w, props))).map(|c| &c.apply)
            };
            let Some(apply) = erstes else { continue };
            let obj = match apply {
                serde_json::Value::Array(a) => a.first(),
                other => Some(other),
            };
            let Some(model) = obj.and_then(|o| o.get("model")).and_then(|m| m.as_str()) else { continue };
            let model = strip_ns(model).to_string();
            let nutzt = match cache.get(&model) {
                Some(&v) => v,
                None => {
                    let v = model_uses_ambient_occlusion(mc, &model);
                    cache.insert(model.clone(), v);
                    v
                }
            };
            if !nutzt {
                result.insert(state);
            }
        }
    }
    result
}

// Welche States ein reines "Kreuz"-Modell sind (Blumen/Gras/Zuckerrohr: alle Elemente
// tragen eine eigene Rotation, s. Modul-Kommentar) -- diese States bekommen von build()
// keine Boxen (append_model_boxes gibt None zurueck, s. dort), sollen aber trotzdem
// gerendert werden: als zwei diagonale Ebenen statt eines Vollwuerfels (s. world::mesher).
// Eigene, einfachere Aufloesung statt build() wiederzuverwenden, weil hier nur eine
// Ja/Nein-Frage pro State beantwortet wird, keine Geometrie.
pub fn build_cross(mc: &mut MinecraftInstallation, states: &[(u32, &str, &[(&str, &str)])]) -> std::collections::HashSet<u32> {
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }

    let mut result = std::collections::HashSet::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };

        let mut model_cache: HashMap<String, Option<Vec<Element>>> = HashMap::new();
        let mut load_elements = |mc: &mut MinecraftInstallation, model: &str| -> Option<Vec<Element>> {
            if let Some(cached) = model_cache.get(model) {
                return cached.clone();
            }
            let elements = load_model_chain(mc, model).ok().and_then(|(_, e)| e);
            model_cache.insert(model.to_string(), elements.clone());
            elements
        };

        for (state, props) in entries {
            if state_is_cross(&mut *mc, &bs, props, &mut load_elements) {
                result.insert(state);
            }
        }
    }
    result
}

// Zusaetzliche, unrotierte Elemente eines Kreuz-Modells (z.B. die 6 flachen "Hitbox"-Deko-
// Ebenen, die PvP-Packs an cobweb.json haengen, s. is_degenerate_decoration-Kommentar) --
// Vanilla rendert bei einem Custom-Modell JEDES Element, nicht nur die geometrisch als
// Kreuz darstellbaren rotierten Ebenen (die uebernimmt push_cross_quads im Mesher, hart-
// codiert wie in Vanilla selbst). Bewusst eine EIGENE, von build()/block_shapes getrennte
// Tabelle statt Wiederverwendung derselben Boxen: block_shapes MUSS fuer Kreuz-States leer
// bleiben (s. dortigen main.rs-Kommentar), sonst wuerden diese randvollen Boxen faelschlich
// Nachbarflaechen anderer Bloecke verdecken. Diese Tabelle wird stattdessen NUR additiv im
// Mesher gezeichnet (ohne jede Nachbar-Verdeckungspruefung, genau wie die Kreuz-Ebenen
// selbst), ueber `content_rect` (dieselbe Zweitextur-Mechanik wie Kesselinhalt).
pub fn build_cross_extras(mc: &mut MinecraftInstallation, states: &[(u32, &str, &[(&str, &str)])]) -> HashMap<u32, Vec<ElementBox>> {
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }

    let mut result = HashMap::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };

        let mut model_cache: HashMap<String, Option<Vec<Element>>> = HashMap::new();
        let mut load_elements = |mc: &mut MinecraftInstallation, model: &str| -> Option<Vec<Element>> {
            if let Some(cached) = model_cache.get(model) {
                return cached.clone();
            }
            let elements = load_model_chain(mc, model).ok().and_then(|(_, e)| e);
            model_cache.insert(model.to_string(), elements.clone());
            elements
        };

        for (state, props) in entries {
            if let Some(boxes) = resolve_cross_extra_boxes(mc, &bs, props, &mut load_elements) {
                if !boxes.is_empty() {
                    result.insert(state, boxes);
                }
            }
        }
    }
    result
}

fn resolve_cross_extra_boxes(
    mc: &mut MinecraftInstallation,
    bs: &BlockstateJson,
    props: &[(&str, &str)],
    load_elements: &mut impl FnMut(&mut MinecraftInstallation, &str) -> Option<Vec<Element>>,
) -> Option<Vec<ElementBox>> {
    let mut boxes = Vec::new();
    if !bs.variants.is_empty() {
        for (selector, apply) in &bs.variants {
            if matches_selector(selector, props) {
                append_cross_extra_boxes(mc, apply, load_elements, &mut boxes)?;
                break;
            }
        }
    } else {
        for case in &bs.multipart {
            let applies = match &case.when {
                None => true,
                Some(when) => when_matches(when, props),
            };
            if applies {
                append_cross_extra_boxes(mc, &case.apply, load_elements, &mut boxes)?;
            }
        }
    }
    Some(boxes)
}

// Anders als append_model_boxes (die bei JEDEM rotierten Element komplett abbricht):
// rotierte Elemente werden hier einfach uebersprungen (das ist push_cross_quads' Job) --
// nur Modelle, die ÜBERHAUPT ein rotiertes Element enthalten (also als Kreuz gilt, s.
// state_is_cross), liefern ueberhaupt Boxen; ein normales Mehrfachbox-Modell ohne jede
// Rotation (Zaun, Treppe, ...) bleibt hier bewusst leer -- das deckt schon build() ab.
fn append_cross_extra_boxes(
    mc: &mut MinecraftInstallation,
    apply: &serde_json::Value,
    load_elements: &mut impl FnMut(&mut MinecraftInstallation, &str) -> Option<Vec<Element>>,
    out: &mut Vec<ElementBox>,
) -> Option<()> {
    let obj = match apply {
        serde_json::Value::Array(a) => a.first(),
        other => Some(other),
    };
    let obj = obj?;
    let model = obj.get("model").and_then(|m| m.as_str())?;
    let model = strip_ns(model).to_string();
    let x_deg = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y_deg = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

    let elements = load_elements(mc, &model)?;
    if !elements.iter().any(|e| e.rotation.is_some()) {
        return Some(());
    }

    let mut first_box_for_face: [Option<([f32; 3], [f32; 3])>; 6] = [None; 6];
    for e in &elements {
        if e.rotation.is_some() {
            continue;
        }
        for (i, name) in FACE_ORDER.iter().enumerate() {
            if first_box_for_face[i].is_none() && e.faces.contains_key(*name) {
                first_box_for_face[i] = Some((e.from, e.to));
            }
        }
    }
    for e in &elements {
        if e.rotation.is_some() {
            continue;
        }
        out.push(element_to_box(e, x_deg, y_deg, &first_box_for_face));
    }
    Some(())
}

// Echte Modell-Vierecke fuer States, deren Modelle GEDREHTE Elemente mit ungedrehten mischen
// (Feuer: gekippte Boden-Ebenen plus flache Seiten-Ebenen; Laterne, Kerzen, Blumentopf, Hebel,
// Lagerfeuer, ...). build() gibt fuer sie keine Boxen (eine gedrehte Ebene ist keine
// achsenparallele Box), state_is_cross greift nicht (nicht ALLE Elemente gedreht) -- ohne diese
// Tabelle fiel der Mesher auf einen Vollwuerfel mit der Partikeltextur zurueck,
// und Feuer sah kaputt aus. Zufallsvarianten ("apply" als Liste): die erste, wie
// ueberall sonst in diesem Modul.
pub fn build_model_quads(mc: &mut MinecraftInstallation, states: &[(u32, &str, &[(&str, &str)])]) -> HashMap<u32, Vec<crate::ModelQuad>> {
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }
    let mut result = HashMap::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };
        let mut model_cache: HashMap<String, Option<Vec<Element>>> = HashMap::new();
        let mut load_elements = |mc: &mut MinecraftInstallation, model: &str| -> Option<Vec<Element>> {
            if let Some(cached) = model_cache.get(model) {
                return cached.clone();
            }
            let elements = load_model_chain(mc, model).ok().and_then(|(_, e)| e);
            model_cache.insert(model.to_string(), elements.clone());
            elements
        };
        for (state, props) in entries {
            if state_is_cross(&mut *mc, &bs, props, &mut load_elements) {
                continue;
            }
            let mut applies: Vec<&serde_json::Value> = Vec::new();
            if !bs.variants.is_empty() {
                if let Some((_, apply)) = bs.variants.iter().find(|(sel, _)| matches_selector(sel, props)) {
                    applies.push(apply);
                }
            } else {
                for case in &bs.multipart {
                    if case.when.as_ref().is_none_or(|w| when_matches(w, props)) {
                        applies.push(&case.apply);
                    }
                }
            }
            let mut gedreht = false;
            let mut quads = Vec::new();
            for apply in applies {
                let obj = match apply {
                    serde_json::Value::Array(a) => a.first(),
                    other => Some(other),
                };
                let Some(obj) = obj else { continue };
                let Some(model) = obj.get("model").and_then(|m| m.as_str()) else { continue };
                let x_deg = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let y_deg = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                gedreht |= crate::model_quads::model_has_rotated_element(mc, model);
                quads.extend(crate::model_quads::block_model_quads(mc, model, x_deg, y_deg).unwrap_or_default());
            }
            if (gedreht || needs_model_quads(block)) && !quads.is_empty() {
                result.insert(state, quads);
            }
        }
    }
    result
}

// Bloecke, die OHNE gedrehtes Element trotzdem als echte Modell-Vierecke gezeichnet werden:
// Pilzbloecke setzen sich aus sechs Ein-Flaechen-Modellen mit VERSCHIEDENEN Texturen
// zusammen (Hut aussen, mushroom_block_inside innen). Der Box-Weg kennt nur einen
// Texturensatz je Blockname -- dort trugen alle sechs Seiten die Hut-Textur.
//
// Falltueren: die offene Fassung dreht den Texturausschnitt ihrer schmalen Seiten per
// Flaechen-"rotation" um 90 Grad. Der Box-Weg kennt keine Flaechendrehung und zog den
// 16x3-Ausschnitt ueber die 3x16-Seite -- die Modell-Vierecke drehen ihn (ModelQuad::uv_rotation).
pub(crate) fn needs_model_quads(block: &str) -> bool {
    matches!(block, "brown_mushroom_block" | "red_mushroom_block" | "mushroom_stem")
        || block.ends_with("_trapdoor")
        // Glasscheiben und Gitter: Pfosten, Arme und die "noside"-Flaechen tragen je Seite
        // verschiedene Texturen (Scheibe, Kante) -- der Box-Weg kennt nur eine Textur je
        // Blockseite, und verbundene Scheiben zeigten die Kante quer ueber der Scheibe.
        || block.ends_with("_pane")
        || block.ends_with("_bars")
        // Kolben und glasierte Keramik drehen den Texturausschnitt einzelner Flaechen ("rotation"
        // in der Flaeche, beim Kolben die Seiten, damit die Holzkante zum Kopf zeigt). Im Box-Weg
        // fehlte das: die Seiten lagen quer, je nach Blickrichtung des Kolbens.
        || matches!(block, "piston" | "sticky_piston" | "piston_head")
        || block.ends_with("_glazed_terracotta")
}

// Alle Texturen aller Modelle eines Blocks, die ein gedrehtes Element enthalten -- die muessen
// in den Block-Atlas, damit build_model_quads-Vierecke eine Kachel finden (s. BlockAtlas::
// texture_uvs). Alle Varianten, nicht nur die erste: das Feuer braucht fire_0 UND fire_1.
pub(crate) fn rotated_model_textures(mc: &mut MinecraftInstallation, block: &str) -> Vec<String> {
    let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { return Vec::new() };
    let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { return Vec::new() };
    let mut applies: Vec<&serde_json::Value> = bs.variants.values().collect();
    applies.extend(bs.multipart.iter().map(|c| &c.apply));
    let mut models: Vec<String> = Vec::new();
    for apply in applies {
        let list: Vec<&serde_json::Value> = match apply {
            serde_json::Value::Array(a) => a.iter().collect(),
            other => vec![other],
        };
        for obj in list {
            if let Some(m) = obj.get("model").and_then(|m| m.as_str()) {
                let m = strip_ns(m).to_string();
                if !models.contains(&m) {
                    models.push(m);
                }
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    for m in models {
        if !needs_model_quads(block) && !crate::model_quads::model_has_rotated_element(mc, &m) {
            continue;
        }
        for t in crate::model_quads::model_textures(mc, &m) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

// Pro State die tatsaechliche Modell-Rotation (x/y in 90-Grad-Schritten) SEINER eigenen
// Blockstate-Variante -- anders als assets::blocks::first_model(), das fuer den ganzen
// Block-NAMEN immer nur die EINE (meist erste im JSON) Variante nimmt. Fuer Bloecke, deren
// Property nur die Rotation aendert, nicht die Textur (Holzstaemme/Knochenbloecke/Ketten:
// "axis"), baut BlockAtlas::build trotzdem nur EINEN festen Satz Flaechen-UVs pro
// Blockname (die Texturen sind ja wirklich identisch) -- diese Tabelle liefert dem
// Aufrufer (main.rs, beim Aufbau von BlockUvs::per_state) die fehlende Information, um die
// bereits aufgeloesten UVs nachtraeglich pro State richtig zu drehen (permute_faces_x/y).
// Ohne das zeigt ein Holzstamm immer in die Richtung, die first_model() zufaellig als
// erste Variante im JSON vorfand. Nur States mit x!=0 oder y!=0 stehen drin (die weit ueberwiegende Mehrheit
// aller Bloecke braucht das nicht), nur Variants (Multipart-Bloecke aendern nie NUR die
// Rotation ueber eine einzelne Property, betrifft die hier gemeinte Blockfamilie nicht).
pub fn build_rotations(mc: &mut MinecraftInstallation, states: &[(u32, &str, &[(&str, &str)])]) -> HashMap<u32, (i32, i32)> {
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }

    let mut result = HashMap::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };
        if bs.variants.is_empty() {
            continue; // Multipart -- keine einzelne Rotation, hier nicht betroffen.
        }
        for (state, props) in entries {
            for (selector, apply) in &bs.variants {
                if !matches_selector(selector, props) {
                    continue;
                }
                let obj = match apply {
                    serde_json::Value::Array(a) => a.first(),
                    other => Some(other),
                };
                if let Some(obj) = obj {
                    let x = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let y = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    if x != 0 || y != 0 {
                        result.insert(state, (x, y));
                    }
                }
                break;
            }
        }
    }
    result
}

// Pro State die Liste ALLER Zufallsvarianten (x-Rotation, y-Rotation, gespiegelt) des
// matchenden Blockstate-Selektors -- fuer Vanillas pseudozufaellige Block-Rotation (Erde,
// Sand, Stein, Grasblock, Podzol, Bedrock, ... -- s. world::render_rng-Modulkommentar).
// States, deren Selektor KEIN Array oder nur einen Eintrag hat (die weit ueberwiegende
// Mehrheit aller Bloecke), tauchen im Ergebnis gar nicht auf.
//
// Manche dieser Vanilla-Arrays wechseln nicht nur die Rotation, sondern referenzieren ein
// "..._mirrored"-Modell (z.B. stone/stone_mirrored, bedrock/bedrock_mirrored). Per
// Yarn-Modell-JSON gegengeprueft: so ein Modell ist NICHT geometrisch anders, sondern
// erbt von block/cube_mirrored, das schlicht auf JEDER der sechs Flaechen "uv": [16,0,0,16]
// statt der Standard-UV [0,0,16,16] traegt -- eine reine horizontale U-Spiegelung derselben
// Textur (s. mirror_face_uvs-Aufrufer in main.rs). Erkennung ueber den Modellnamen-Suffix
// "_mirrored" reicht: das ist Vanillas eigene, durchgaengige Namenskonvention dafuer.
// Batch-Fassung wie build_rotations() (ein Blockstate-JSON-Laden pro eindeutigem
// Blocknamen statt pro State, s. dortigen Kommentar zur by_block-Gruppierung) statt einer
// Einzel-State-Funktion -- die meisten der tausenden States teilen sich denselben
// Blocknamen, ein erneutes Laden/Parsen pro State waere reine Verschwendung.
pub fn build_random_rotations(
    mc: &mut MinecraftInstallation,
    states: &[(u32, &str, &[(&str, &str)])],
) -> HashMap<u32, Vec<(i32, i32, bool)>> {
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }

    let mut result = HashMap::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };
        if bs.variants.is_empty() {
            continue; // Multipart -- keine Zufallsvarianten in diesem Sinne.
        }
        for (state, props) in entries {
            for (selector, apply) in &bs.variants {
                if !matches_selector(selector, props) {
                    continue;
                }
                if let serde_json::Value::Array(variant_entries) = apply {
                    if variant_entries.len() > 1 {
                        let rotations: Vec<(i32, i32, bool)> = variant_entries
                            .iter()
                            .map(|obj| {
                                let mirrored =
                                    obj.get("model").and_then(|m| m.as_str()).is_some_and(|m| strip_ns(m).ends_with("_mirrored"));
                                let x = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                let y = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                (x, y, mirrored)
                            })
                            .collect();
                        result.insert(state, rotations);
                    }
                }
                break;
            }
        }
    }
    result
}

// Ein State ist "Kreuz", wenn JEDES matchende Modell NICHT leer ist und (nach Herausfiltern
// solcher flachen Deko-Elemente) AUSSCHLIESSLICH aus Elementen mit eigener Rotation besteht
// (Vanilla nutzt das nur fuer die diagonalen Kreuz-Ebenen).
fn state_is_cross(
    mc: &mut MinecraftInstallation,
    bs: &BlockstateJson,
    props: &[(&str, &str)],
    load_elements: &mut impl FnMut(&mut MinecraftInstallation, &str) -> Option<Vec<Element>>,
) -> bool {
    let mut applies: Vec<&serde_json::Value> = Vec::new();
    if !bs.variants.is_empty() {
        for (selector, apply) in &bs.variants {
            if matches_selector(selector, props) {
                applies.push(apply);
                break;
            }
        }
    } else {
        for case in &bs.multipart {
            let ok = match &case.when {
                None => true,
                Some(when) => when_matches(when, props),
            };
            if ok {
                applies.push(&case.apply);
            }
        }
    }
    if applies.is_empty() {
        return false;
    }
    for apply in applies {
        let obj = match apply {
            serde_json::Value::Array(a) => a.first(),
            other => Some(other),
        };
        let Some(obj) = obj else { return false };
        let Some(model) = obj.get("model").and_then(|m| m.as_str()) else { return false };
        let model = strip_ns(model).to_string();
        let Some(elements) = load_elements(mc, &model) else { return false };
        let real: Vec<&Element> = elements.iter().filter(|e| !is_degenerate_decoration(e)).collect();
        // Nur Drehungen um die HOCHACHSE sind Kreuz-Ebenen. Die schraegen Schienen (ein um
        // 45 Grad um x gekipptes Element) landeten sonst als Pflanzenkreuz im Bild -- sie gehoeren zu den echten
        // Modell-Vierecken (s. build_model_quads).
        // Ausserdem muss die Ebene fast die ganze Zelle breit sein (block/cross: 14.4 Pixel):
        // die Kette besteht aus zwei nur 3 Pixel breiten Ebenen mit eigenem UV-Ausschnitt und
        // stand als volles Kreuz mit der ganzen Textur da. Und eine in x gekippte Variante (liegende Kette) kann der
        // Kreuz-Pfad gar nicht drehen.
        let um_hochachse = |e: &&Element| {
            e.rotation.as_ref().and_then(|r| r.get("axis")).and_then(|a| a.as_str()) == Some("y")
                && (e.to[0] - e.from[0]).abs().max((e.to[2] - e.from[2]).abs()) >= 12.0
        };
        let gekippt = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        if real.is_empty() || gekippt || !real.iter().all(um_hochachse) {
            return false;
        }
    }
    true
}

// None = nicht darstellbar (s. append_model_boxes) -- der Aufrufer (build()) verwirft den
// State dann komplett, statt eine leere/kaputte Box-Liste einzutragen; wer per
// world::block_shapes nachschlaegt und nichts findet, faellt auf den Vollwuerfel zurueck.
fn resolve_state_shape(
    mc: &mut MinecraftInstallation,
    bs: &BlockstateJson,
    props: &[(&str, &str)],
    load_elements: &mut impl FnMut(&mut MinecraftInstallation, &str) -> Option<Vec<Element>>,
) -> Option<Vec<ElementBox>> {
    let mut boxes = Vec::new();

    if !bs.variants.is_empty() {
        // Genau eine Variante muss passen (die Selektoren sind laut Vanilla-Format
        // ueberschneidungsfrei fuer jede gueltige Property-Kombination).
        for (selector, apply) in &bs.variants {
            if matches_selector(selector, props) {
                append_model_boxes(mc, apply, load_elements, &mut boxes)?;
                break;
            }
        }
    } else {
        // Multipart: JEDEN zutreffenden Fall anwenden (nicht nur den ersten) -- ein Zaun
        // braucht Pfosten UND alle passenden Seitenarme gleichzeitig.
        for case in &bs.multipart {
            let applies = match &case.when {
                None => true,
                Some(when) => when_matches(when, props),
            };
            if applies {
                append_model_boxes(mc, &case.apply, load_elements, &mut boxes)?;
            }
        }
    }

    Some(boxes)
}

// Ein "apply"-Eintrag (direktes Objekt ODER eine Liste von Zufallsvarianten, von denen wir
// die erste nehmen, s. first_model) in seine Elements + Rotation aufloesen und anhaengen.
// None = dieses Modell enthaelt ein nicht darstellbares Element (eigene Rotation, z.B. die
// diagonalen Kreuz-Ebenen von Blumen/Gras) -- der gesamte State faellt dann auf den
// Vollwuerfel zurueck, statt eine falsche/unvollstaendige Box zu zeigen.
fn append_model_boxes(
    mc: &mut MinecraftInstallation,
    apply: &serde_json::Value,
    load_elements: &mut impl FnMut(&mut MinecraftInstallation, &str) -> Option<Vec<Element>>,
    out: &mut Vec<ElementBox>,
) -> Option<()> {
    let obj = match apply {
        serde_json::Value::Array(a) => a.first(),
        other => Some(other),
    };
    let obj = obj?;
    let model = obj.get("model").and_then(|m| m.as_str())?;
    let model = strip_ns(model).to_string();
    let x_deg = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y_deg = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

    let elements = load_elements(mc, &model)?;

    // Fuer jede Richtung die Box (from/to) des ERSTEN Elements merken, das ueberhaupt eine
    // Flaeche in dieser Richtung deklariert (unabhaengig von tintindex) -- Referenz fuer den
    // "gleiche Box"-Vergleich in element_to_box (s. ElementBox::content-Kommentar).
    let mut first_box_for_face: [Option<([f32; 3], [f32; 3])>; 6] = [None; 6];
    for e in &elements {
        if e.rotation.is_some() {
            continue;
        }
        for (i, name) in FACE_ORDER.iter().enumerate() {
            if first_box_for_face[i].is_none() && e.faces.contains_key(*name) {
                first_box_for_face[i] = Some((e.from, e.to));
            }
        }
    }

    for e in &elements {
        if e.rotation.is_some() {
            return None;
        }
        out.push(element_to_box(e, x_deg, y_deg, &first_box_for_face));
    }
    Some(())
}

// Vanillas Standard-UV, wenn ein Modell fuer eine Flaeche gar kein "uv"-Array angibt (z.B.
// JEDE Flaeche von Zaunpfosten/Mauern, s. template_wall_post.json/template_wall_side.json --
// keine hat ein explizites "uv"). Ohne diese Funktion blieb uv[i]=None -> der Mesher hat die
// VOLLE Atlas-Kachel ueber die (oft viel schmalere) Flaeche gestreckt
// und an Mauern und Zaeunen verzerrt. Die Formel ist NICHT geraten,
// sondern empirisch an echten Vanilla-Daten verifiziert, die BEIDES haben (explizites uv UND
// nicht-volle Element-Geometrie): oak_fence_side.json, "top bar"-Element from=[7,12,0]
// to=[9,15,9] -- down/up=[7,0,9,9], north=[7,1,9,4], west=[0,1,9,4] passen exakt zur Formel
// unten (bei x0=7,x1=9,y0=12,y1=15,z0=0,z1=9). Sued/Ost sind die topologischen Spiegel von
// Nord/West (Standard-Wuerfel-Abwicklungskonvention) -- fuer volle 0..16-Elemente reduziert
// sich jede der sechs Formeln auf [0,0,16,16], also identisch zum bisherigen "kein Crop"-
// Verhalten fuer die grosse Mehrheit (Vollwuerfel-Bloecke) -- reine Erweiterung, keine
// Verhaltensaenderung fuer bereits korrekt aussehende Bloecke.
pub fn default_face_uv(face_index: usize, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let [x0, y0, z0] = from;
    let [x1, y1, z1] = to;
    match face_index {
        0 | 1 => [x0, z0, x1, z1],                         // down, up
        2 => [16.0 - x1, 16.0 - y1, 16.0 - x0, 16.0 - y0], // north
        3 => [x0, 16.0 - y1, x1, 16.0 - y0],               // south
        4 => [z0, 16.0 - y1, z1, 16.0 - y0],               // west
        5 => [16.0 - z1, 16.0 - y1, 16.0 - z0, 16.0 - y0], // east
        _ => unreachable!(),
    }
}

fn element_to_box(e: &Element, x_deg: i32, y_deg: i32, first_box_for_face: &[Option<([f32; 3], [f32; 3])>; 6]) -> ElementBox {
    let mut faces = [false; 6];
    let mut content = [false; 6];
    let mut uv: [Option<[f32; 4]>; 6] = [None; 6];
    for (i, name) in FACE_ORDER.iter().enumerate() {
        let Some(face) = e.faces.get(*name) else { continue };
        faces[i] = true;
        // tintindex + eine ANDERE Box als die, die diese Richtung zuerst deklariert hat ->
        // eigenstaendiger Inhalt (Kesselinhalt), keine deckungsgleiche Ueberlagerung
        // (Gras-Seiten) -- s. ElementBox::content-Kommentar.
        content[i] = face.tintindex.is_some() && first_box_for_face[i] != Some((e.from, e.to));
        uv[i] = Some(face.uv.unwrap_or_else(|| default_face_uv(i, e.from, e.to)));
    }

    let (from, to, faces) = rotate_box(e.from, e.to, faces, x_deg, y_deg);
    let (_, _, content) = rotate_box(e.from, e.to, content, x_deg, y_deg);
    let uv = rotate_uv(uv, x_deg, y_deg);

    // Nach der Rotation muss min/max neu sortiert werden (eine Achse kann das Vorzeichen
    // wechseln, s. rotate_pair).
    let min = std::array::from_fn(|i| from[i].min(to[i]) / 16.0);
    let max = std::array::from_fn(|i| from[i].max(to[i]) / 16.0);
    let uv = uv.map(|opt| opt.map(|[u0, v0, u1, v1]| [u0 / 16.0, v0 / 16.0, u1 / 16.0, v1 / 16.0]));
    ElementBox { min, max, faces, content, uv }
}

// Dreht from/to und eine face-indizierte Zuordnung (Praesenz/Inhalt/UV-Ausschnitt -- generisch
// ueber T, damit dieselbe Funktion fuer alle drei genutzt werden kann) um x_deg (zuerst) dann
// y_deg (danach), je in 90-Grad-Schritten um den Block-Mittelpunkt (8,8,8) -- die einzigen in
// Vanilla-Blockstates vorkommenden Werte. Herleitung der Formeln s. Modul-Kommentar:
// eine "Norden"-Seite (from/to bei z=0) wird durch y=90 zur "Osten"-Seite
// (x=16), etc.
fn rotate_box<T: Copy>(from: [f32; 3], to: [f32; 3], faces: [T; 6], x_deg: i32, y_deg: i32) -> ([f32; 3], [f32; 3], [T; 6]) {
    let x_steps = (x_deg / 90).rem_euclid(4);
    let y_steps = (y_deg / 90).rem_euclid(4);

    let mut from = from;
    let mut to = to;
    let mut faces = faces;

    for _ in 0..x_steps {
        (from[1], from[2]) = rotate_pair_x(from[1], from[2]);
        (to[1], to[2]) = rotate_pair_x(to[1], to[2]);
        faces = permute_faces_x(faces);
    }
    for _ in 0..y_steps {
        (from[0], from[2]) = rotate_pair(from[0], from[2]);
        (to[0], to[2]) = rotate_pair(to[0], to[2]);
        faces = permute_faces_y(faces);
    }

    (from, to, faces)
}

// Ein 90-Grad-Schritt fuer ein Koordinatenpaar um den Mittelpunkt 8: (a,b) -> (16-b, a).
// Gilt fuer Y-Rotation (an Falllaub/Staemmen ausfuehrlich gegen echte Daten verifiziert).
fn rotate_pair(a: f32, b: f32) -> (f32, f32) {
    (16.0 - b, a)
}

// X-Rotation dreht in der ENTGEGENGESETZTEN Chiralitaet zu Y (Vanilla-eigene Konvention,
// nicht symmetrisch) -- die Umkehrfunktion von rotate_pair. Ohne diese Umkehr landet z.B.
// eine deckenhaengende Ranke (vine, up=true, x=270) bei y~0.05 (Boden) statt y~0.95 (Decke)
// -- per shapes_smoke::vine_probe gegen die echte Installation nachgewiesen und hiermit
// gefixt (s. Ranken-Rendering-Bugreport).
fn rotate_pair_x(a: f32, b: f32) -> (f32, f32) {
    (b, 16.0 - a)
}

// UV-Ausschnitte (s. ElementBox::uv) drehen -- ANDERS als faces/content reicht hier die
// reine Slot-Permutation (permute_faces_x/y) NICHT: eine Y-Rotation dreht die Geometrie
// UNTEN/OBEN in der eigenen Ebene (ihr Slot-Index bleibt gleich, permute_faces_y laesst
// 0/1 unangetastet), der zugehoerige UV-Ausschnitt muss also SELBST im Kreis gedreht
// werden (sonst passt sein Seitenverhaeltnis nicht mehr zur gedrehten Box -- sichtbar als
// gestreckte Falllaub-Viertel, betrifft Falllaub mit facing=east/west, wo die Box durch die Drehung ihr Breite/Tiefe-Verhaeltnis
// tauscht). Analog fuer West/Ost unter einer X-Rotation (deren eigene Ebene ist Y/Z).
// Nord/Sued/West/Ost unter Y- bzw. Oben/Unten/Nord/Sued unter X-Rotation wandern dagegen
// nur zwischen Slots (deren jeweils EIGENE Ebene aendert sich nicht) -- dafuer reicht die
// normale Slot-Permutation weiterhin, keine Sonderbehandlung noetig.
fn rotate_uv(mut uv: [Option<[f32; 4]>; 6], x_deg: i32, y_deg: i32) -> [Option<[f32; 4]>; 6] {
    let x_steps = (x_deg / 90).rem_euclid(4);
    let y_steps = (y_deg / 90).rem_euclid(4);

    for _ in 0..x_steps {
        uv[4] = uv[4].map(rotate_uv_rect_x); // West
        uv[5] = uv[5].map(rotate_uv_rect_x); // Ost
        uv = permute_faces_x(uv);
    }
    for _ in 0..y_steps {
        uv[0] = uv[0].map(rotate_uv_rect_y); // Unten
        uv[1] = uv[1].map(rotate_uv_rect_y); // Oben
        uv = permute_faces_y(uv);
    }
    uv
}

// Ein 90-Grad-Schritt fuer ein UV-RECHTECK unter einer Y-Rotation (0..16-Einheiten) -- NICHT
// einfach beide Eckpunkte (u0,v0) und (u1,v1) unabhaengig mit rotate_pair drehen (das war der
// urspruengliche, FALSCHE Ansatz): das dreht zwei einzelne PUNKTE korrekt, aber fuer ein
// Rechteck muss die GEGENUEBERLIEGENDE Ecken-Kombination (u0,v1) und (u1,v0) gedreht werden --
// sonst kommt eine an der falschen Achse GESPIEGELTE statt gedrehte Textur heraus (bei
// symmetrischen/unauffaelligen Texturen wie Falllaub kaum sichtbar, bei gerichteten Texturen
// wie Holzmaserung an Zaun/Mauer deutlich als "schiefe"/verkehrte Textur).
// Empirisch bestaetigt: default_face_uv() auf eine WIRKLICH gedrehte Box angewendet ergibt bei
// einer Y-Rotation der Unten-Flaeche exakt (16-z1,x0,16-z0,x1) aus dem urspruenglichen
// [x0,z0,x1,z1] -- das liefert NUR rotate_pair(u0,v1)+rotate_pair(u1,v0), nicht
// rotate_pair(u0,v0)+rotate_pair(u1,v1) (s. shapes_smoke::
// rotierte_standard_uv_stimmt_mit_direkter_neuberechnung_ueberein).
fn rotate_uv_rect_y([u0, v0, u1, v1]: [f32; 4]) -> [f32; 4] {
    let (ru0, rv0) = rotate_pair(u0, v1);
    let (ru1, rv1) = rotate_pair(u1, v0);
    [ru0, rv0, ru1, rv1]
}

// Dasselbe fuer eine X-Rotation (West/Ost-Flaechen) -- eigene Funktion statt Wiederverwendung
// von rotate_uv_rect_y, weil X in Vanilla die ENTGEGENGESETZTE Chiralitaet zu Y hat (s.
// rotate_pair_x-Kommentar): dieselbe Herleitung wie oben, aber mit rotate_pair_x UND der
// jeweils ANDEREN Ecken-Kombination (u1,v0) und (u0,v1) -- gegen dieselbe default_face_uv-
// Neuberechnung fuer eine x-rotierte Box hergeleitet und verifiziert.
fn rotate_uv_rect_x([u0, v0, u1, v1]: [f32; 4]) -> [f32; 4] {
    let (ru0, rv0) = rotate_pair_x(u1, v0);
    let (ru1, rv1) = rotate_pair_x(u0, v1);
    [ru0, rv0, ru1, rv1]
}

// FACE_DOWN=0, FACE_UP=1, FACE_NORTH=2, FACE_SOUTH=3, FACE_WEST=4, FACE_EAST=5 (s.
// assets::blocks). Ein Y-Schritt: Norden->Osten->Sueden->Westen->Norden, oben/unten fest.
// Generisch ueber T, damit blocks.rs dieselbe Permutation auch fuer Textur-Referenzen
// (statt der hier genutzten bool-Praesenz) wiederverwenden kann -- beides muss exakt
// gleich rotieren, sonst landet die Textur auf der falschen Seite. Oeffentlich (nicht nur
// crate-intern), weil main.rs sie fuer build_rotations() ebenfalls braucht (dieselbe
// Permutation auf UvRect statt auf Face-Praesenz/-Referenzen).
pub fn permute_faces_y<T: Copy>(f: [T; 6]) -> [T; 6] {
    [f[0], f[1], f[4], f[5], f[3], f[2]]
}

// Ein X-Schritt (x=90): Norden->unten->Sueden->oben->Norden, Ost/West fest -- dieselbe
// Drehrichtung wie rotate_pair_x fuer die Geometrie. Bis 2026-09-26 lief die Zuordnung
// andersherum (Norden->oben): bei Vollwuerfeln nur eine falsche Textur, bei Ebenen-Modellen
// aber eine Flaeche, die an der Oberkante liegt und nach UNTEN zeigt -- vom Backface-Culling
// verworfen. So fehlten Pilzbloecken manche Seiten: ihre
// Oben-/Unten-Seite ist template_single_face (Ebene bei z=0) mit x=270/x=90. Gegenprobe an
// Vanilla: das Kolbenmodell traegt die Platte im Norden, facing=down ist x=90 -- die Platte
// muss danach nach unten zeigen (facing=up, x=270: nach oben).
pub fn permute_faces_x<T: Copy>(f: [T; 6]) -> [T; 6] {
    [f[2], f[3], f[1], f[0], f[4], f[5]]
}

// Blockstate-Variant-Selektor ("type=bottom" oder "facing=east,half=bottom,shape=straight",
// auch "" fuer eigenschaftslose Bloecke) gegen die echten Property-Werte des States matchen.
// Der Selektor darf Properties auslassen, die die Form nicht beeinflussen -- deshalb nur
// die im Selektor GENANNTEN Paare pruefen, nicht alle Properties des States.
fn matches_selector(selector: &str, props: &[(&str, &str)]) -> bool {
    if selector.is_empty() {
        return true;
    }
    selector.split(',').all(|pair| {
        let Some((k, v)) = pair.split_once('=') else { return false };
        props.iter().any(|&(pk, pv)| pk == k && pv == v)
    })
}

// Multipart-"when": entweder ein flaches Objekt (AND aller Paare, Werte auch
// "a|b|c"-getrennt fuer ODER innerhalb eines Properties, z.B. Redstone-Verbindungen), oder
// {"OR": [...]} / {"AND": [...]} mit verschachtelten when-Objekten.
fn when_matches(when: &serde_json::Value, props: &[(&str, &str)]) -> bool {
    if let Some(arr) = when.get("OR").and_then(|v| v.as_array()) {
        return arr.iter().any(|clause| when_matches(clause, props));
    }
    if let Some(arr) = when.get("AND").and_then(|v| v.as_array()) {
        return arr.iter().all(|clause| when_matches(clause, props));
    }
    let Some(obj) = when.as_object() else { return false };
    obj.iter().all(|(k, v)| {
        let expected = v.as_str().unwrap_or("");
        let actual = props.iter().find(|&&(pk, _)| pk == k).map(|&(_, pv)| pv).unwrap_or("");
        expected.split('|').any(|opt| opt == actual)
    })
}

// Welches Modell jeder State ueber "variants" bekommt (nur die Variantenform; Multipart-Bloecke
// setzen sich aus mehreren Modellen zusammen und tauchen hier nicht auf). Gegenstueck zu
// BlockAtlas::variant_uvs: dort stehen die Texturen je Modell, hier die Zuordnung je State.
pub fn state_variant_models(mc: &mut MinecraftInstallation, states: &[(u32, &str, &[(&str, &str)])]) -> HashMap<u32, String> {
    let mut by_block: HashMap<&str, Vec<(u32, &[(&str, &str)])>> = HashMap::new();
    for &(state, block, props) in states {
        by_block.entry(block).or_default().push((state, props));
    }
    let mut out = HashMap::new();
    for (block, entries) in by_block {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
        let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };
        if bs.variants.is_empty() {
            continue;
        }
        for (state, props) in entries {
            let Some((_, apply)) = bs.variants.iter().find(|(sel, _)| matches_selector(sel, props)) else { continue };
            let obj = match apply {
                serde_json::Value::Array(a) => a.first(),
                other => Some(other),
            };
            if let Some(model) = obj.and_then(|o| o.get("model")).and_then(|m| m.as_str()) {
                out.insert(state, strip_ns(model).to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_ignoriert_nicht_genannte_properties() {
        assert!(matches_selector("type=bottom", &[("type", "bottom"), ("waterlogged", "false")]));
        assert!(!matches_selector("type=bottom", &[("type", "top"), ("waterlogged", "false")]));
        assert!(matches_selector("", &[]));
    }

    #[test]
    fn when_flach_ist_and_verknuepft() {
        let when: serde_json::Value = serde_json::json!({"north": "true", "east": "false"});
        assert!(when_matches(&when, &[("north", "true"), ("east", "false"), ("south", "true")]));
        assert!(!when_matches(&when, &[("north", "true"), ("east", "true")]));
    }

    #[test]
    fn when_or_matcht_wenn_eine_klausel_passt() {
        let when: serde_json::Value = serde_json::json!({"OR": [{"north": "low"}, {"north": "tall"}]});
        assert!(when_matches(&when, &[("north", "tall")]));
        assert!(!when_matches(&when, &[("north", "none")]));
    }

    #[test]
    fn when_pipe_werte_sind_oder_innerhalb_einer_property() {
        let when: serde_json::Value = serde_json::json!({"north": "up|side"});
        assert!(when_matches(&when, &[("north", "side")]));
        assert!(!when_matches(&when, &[("north", "none")]));
    }

    #[test]
    fn y_rotation_dreht_norden_zu_osten() {
        // Nordseiten-Element (z=0..6, x/y mittig) -- nach y=90 muss es an der Ostseite
        // (x=10..16) liegen, mit einer Nord-Flaeche, die zur Ost-Flaeche wird.
        let e = Element {
            from: [7.0, 0.0, 0.0],
            to: [9.0, 16.0, 6.0],
            faces: [("north".to_string(), crate::blocks::Face { texture: "#texture".to_string(), tintindex: None, uv: None, rotation: None })]
                .into_iter()
                .collect(),
            rotation: None,
            shade: true,
        };
        let b = element_to_box(&e, 0, 90, &[None; 6]);
        assert!((b.min[0] - 10.0 / 16.0).abs() < 1e-6, "min.x = {}", b.min[0]);
        assert!((b.max[0] - 16.0 / 16.0).abs() < 1e-6, "max.x = {}", b.max[0]);
        assert!(b.faces[5], "Ost-Flaeche muss gesetzt sein"); // FACE_EAST
        assert!(!b.faces[2], "Nord-Flaeche darf nicht mehr gesetzt sein");
    }

    #[test]
    fn x_rotation_180_flippt_oben_und_unten() {
        // Ein Element mit "up"-Flaeche muss nach x=180 stattdessen eine "down"-Flaeche haben.
        let e = Element {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 8.0, 16.0],
            faces: [("up".to_string(), crate::blocks::Face { texture: "#texture".to_string(), tintindex: None, uv: None, rotation: None })]
                .into_iter()
                .collect(),
            rotation: None,
            shade: true,
        };
        let b = element_to_box(&e, 180, 0, &[None; 6]);
        assert!(b.faces[0], "Unten-Flaeche muss gesetzt sein"); // FACE_DOWN
        assert!(!b.faces[1], "Oben-Flaeche darf nicht mehr gesetzt sein");
    }
}
