// Skin-Tab der Einstellungen: zwei Spalten mit Pack-Kacheln. Links steht, was im Skin-Ordner
// liegt und nicht gewaehlt ist (zuletzt herausgenommene oben), rechts der gewaehlte Stapel wie
// in Vanillas Pack-Liste: das obere Pack ueberschreibt die darunter. Hier steht nur, was ohne
// Fenster und GPU auskommt -- Spalten, Verschieben, Pack-Infos und der Dateibaum eines Packs,
// in dem sich Dateien und Ordner unter assets/minecraft ausschalten lassen.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use protocol::nbt::TextSpan;

// Kantenlaenge der Pack-Bilder im Icon-Atlas.
pub const ICON_PX: u32 = 128;
// Plaetze im Icon-Atlas (ICON_GRID^2).
pub const ICON_GRID: u32 = 16;
// Der letzte Platz gehoert Vanillas eigenem Pack-Bild (die feste Kachel unten rechts).
pub const VANILLA_SLOT: usize = (ICON_GRID * ICON_GRID - 1) as usize;

const SCAN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Available,
    Selected,
}

// Linke Spalte: zuerst die gemerkte Reihenfolge (s. Settings::skin_order), dann was neu dazu
// kam, alphabetisch. Gewaehltes und nicht mehr Vorhandenes faellt weg.
// Platzhalter im Stapel fuer das, was kein Ordner im Skin-Ordner ist: Vanilla selbst (alles
// darunter fuellt nur noch, was Vanilla nicht hat) und die Packs, die der Server geschickt hat.
// Spitze Klammern, weil kein Ordnername unter Windows sie tragen kann.
pub const VANILLA: &str = "<vanilla>";
pub const SERVER: &str = "<server>";

pub fn is_marker(name: &str) -> bool {
    name == VANILLA || name == SERVER
}

// Den Stapel in Ordnung bringen: Vanilla genau einmal (fehlt es, ganz unten -- so verhielt sich
// der Stapel, bevor man es verschieben konnte), der Server-Eintrag nur, solange der Server
// Packs geschickt hat (`server` = Some), und dann neu an diesem Platz -- dem gemerkten fuer
// diesen Server, sonst 0 = ganz oben.
pub fn normalize_stack(stack: &mut Vec<String>, server: Option<usize>) {
    let mut gesehen = false;
    stack.retain(|n| n != VANILLA || !std::mem::replace(&mut gesehen, true));
    if !gesehen {
        stack.push(VANILLA.to_string());
    }
    match server {
        None => stack.retain(|n| n != SERVER),
        Some(platz) if !stack.iter().any(|n| n == SERVER) => stack.insert(platz.min(stack.len()), SERVER.to_string()),
        Some(_) => {}
    }
}

pub fn available_column(all: &[String], order: &[String], selected: &[String]) -> Vec<String> {
    let frei = |n: &String| all.contains(n) && !selected.contains(n);
    let mut out: Vec<String> = Vec::new();
    for n in order.iter().filter(|n| frei(n)) {
        if !out.contains(n) {
            out.push(n.clone());
        }
    }
    let mut neu: Vec<String> = all.iter().filter(|n| frei(n) && !out.contains(n)).cloned().collect();
    neu.sort();
    out.extend(neu);
    out
}

// Ein Pack von Platz `from` an Platz `to` verschieben, auch in die andere Spalte. `to` zaehlt
// unter den Kacheln, die ohne das gezogene Pack in der Zielspalte stehen.
pub fn move_skin(left: &mut Vec<String>, right: &mut Vec<String>, from: (Column, usize), to: (Column, usize)) {
    let quelle = match from.0 {
        Column::Available => &mut *left,
        Column::Selected => &mut *right,
    };
    if from.1 >= quelle.len() {
        return;
    }
    let name = quelle.remove(from.1);
    let ziel = match to.0 {
        Column::Available => left,
        Column::Selected => right,
    };
    let i = to.1.min(ziel.len());
    ziel.insert(i, name);
}

// Was eine Kachel ausser dem Namen zeigt: die Beschreibung aus pack.mcmeta und pack.png.
pub struct PackInfo {
    pub description: Vec<TextSpan>,
    // ICON_PX x ICON_PX RGBA.
    pub icon: Option<Vec<u8>>,
}

pub fn read_pack_info(dir: &Path) -> PackInfo {
    let description = std::fs::read(dir.join("pack.mcmeta"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(&b)).ok())
        .and_then(|v| v.get("pack")?.get("description").cloned())
        .map(|d| protocol::nbt::json_text_to_spans(&d.to_string()))
        .unwrap_or_default();
    let icon = std::fs::read(dir.join("pack.png")).ok().and_then(|b| decode_icon(&b));
    PackInfo { description, icon }
}

// Ein Pack-Bild auf ICON_PX bringen. Kleine Bilder sind fast immer Pixelkunst und werden
// ohne Glaetten vergroessert, grosse geglaettet verkleinert.
pub fn decode_icon(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    if img.width() == 0 || img.height() == 0 {
        return None;
    }
    let filter = if img.width() < ICON_PX { image::imageops::FilterType::Nearest } else { image::imageops::FilterType::Triangle };
    let img = if img.width() == ICON_PX && img.height() == ICON_PX { img } else { image::imageops::resize(&img, ICON_PX, ICON_PX, filter) };
    Some(img.into_raw())
}

pub struct CatalogEntry {
    pub description: Vec<TextSpan>,
    pub icon_slot: Option<usize>,
    stamp: Stamp,
}

type Stamp = (Option<SystemTime>, Option<SystemTime>);

fn stamp(dir: &Path) -> Stamp {
    let zeit = |n: &str| std::fs::metadata(dir.join(n)).and_then(|m| m.modified()).ok();
    (zeit("pack.mcmeta"), zeit("pack.png"))
}

// Die Packs im Skin-Ordner samt Beschreibung und Platz im Icon-Atlas. Der Ordner wird
// hoechstens einmal je Sekunde gelesen, die Infos eines Packs nur neu, wenn sich pack.mcmeta
// oder pack.png geaendert haben.
#[derive(Default)]
pub struct Catalog {
    pub names: Vec<String>,
    entries: HashMap<String, CatalogEntry>,
    last_scan: Option<Instant>,
    next_slot: usize,
    // Vanillas Pack-Bild liegt schon im Atlas (s. VANILLA_SLOT).
    pub vanilla_icon_done: bool,
}

impl Catalog {
    pub fn get(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.get(name)
    }

    // Beim naechsten refresh sofort neu lesen (z.B. beim Oeffnen des Tabs).
    pub fn invalidate(&mut self) {
        self.last_scan = None;
    }

    // Liefert die Bilder, die in den Icon-Atlas muessen: (Platz, RGBA).
    pub fn refresh(&mut self, dir: Option<&Path>, names: impl FnOnce() -> Vec<String>) -> Vec<(usize, Vec<u8>)> {
        let jetzt = Instant::now();
        if self.last_scan.is_some_and(|t| jetzt.duration_since(t) < SCAN_INTERVAL) {
            return Vec::new();
        }
        self.last_scan = Some(jetzt);
        let Some(dir) = dir else {
            self.names.clear();
            return Vec::new();
        };
        self.names = names();
        let mut hochladen = Vec::new();
        for name in &self.names {
            let pfad = dir.join(name);
            let neu = stamp(&pfad);
            if self.entries.get(name).is_some_and(|e| e.stamp == neu) {
                continue;
            }
            let info = read_pack_info(&pfad);
            let alter_platz = self.entries.get(name).and_then(|e| e.icon_slot);
            let icon_slot = info.icon.map(|rgba| {
                let platz = alter_platz.unwrap_or_else(|| {
                    let p = self.next_slot;
                    self.next_slot += 1;
                    p
                });
                hochladen.push((platz, rgba));
                platz
            });
            let icon_slot = icon_slot.filter(|&p| p < VANILLA_SLOT);
            self.entries.insert(name.clone(), CatalogEntry { description: info.description, icon_slot, stamp: neu });
        }
        hochladen.retain(|(p, _)| *p < VANILLA_SLOT);
        hochladen
    }
}

// Ein Eintrag im Dateibaum eines Packs, Pfad relativ zu assets/minecraft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
}

// Alles unter `root`, der Tiefe nach: je Ordner erst die Unterordner, dann die Dateien,
// beides alphabetisch.
pub fn list_tree(root: &Path) -> Vec<FileEntry> {
    let mut out = Vec::new();
    walk(root, "", 0, &mut out);
    out
}

fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<FileEntry>) {
    let Ok(eintraege) = std::fs::read_dir(dir) else { return };
    let mut ordner: Vec<(String, PathBuf)> = Vec::new();
    let mut dateien: Vec<String> = Vec::new();
    for e in eintraege.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.path().is_dir() {
            ordner.push((name, e.path()));
        } else {
            dateien.push(name);
        }
    }
    ordner.sort();
    dateien.sort();
    for (name, pfad) in ordner {
        let path = format!("{rel}{name}");
        out.push(FileEntry { path: path.clone(), name, depth, is_dir: true });
        walk(&pfad, &format!("{path}/"), depth + 1, out);
    }
    for name in dateien {
        out.push(FileEntry { path: format!("{rel}{name}"), name, depth, is_dir: false });
    }
}

// Wie list_tree, nur aus Dateipfaden (relativ zu assets/minecraft) statt aus einem Ordner --
// fuer Packs, die als Zip vorliegen (Server-Packs).
pub fn tree_from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<FileEntry> {
    #[derive(Default)]
    struct Knoten {
        ordner: std::collections::BTreeMap<String, Knoten>,
        dateien: std::collections::BTreeSet<String>,
    }
    fn aus(k: &Knoten, rel: &str, depth: usize, out: &mut Vec<FileEntry>) {
        for (name, kind) in &k.ordner {
            let path = format!("{rel}{name}");
            out.push(FileEntry { path: path.clone(), name: name.clone(), depth, is_dir: true });
            aus(kind, &format!("{path}/"), depth + 1, out);
        }
        for name in &k.dateien {
            out.push(FileEntry { path: format!("{rel}{name}"), name: name.clone(), depth, is_dir: false });
        }
    }
    let mut wurzel = Knoten::default();
    for p in paths {
        let teile: Vec<&str> = p.split('/').filter(|t| !t.is_empty()).collect();
        let Some((datei, ordner)) = teile.split_last() else { continue };
        let mut k = &mut wurzel;
        for o in ordner {
            k = k.ordner.entry(o.to_string()).or_default();
        }
        k.dateien.insert(datei.to_string());
    }
    let mut out = Vec::new();
    aus(&wurzel, "", 0, &mut out);
    out
}

// Die Zeilen, die bei diesen aufgeklappten Ordnern zu sehen sind (Indizes in `entries`).
pub fn visible_rows(entries: &[FileEntry], expanded: &HashSet<String>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut zu: Option<usize> = None;
    for (i, e) in entries.iter().enumerate() {
        if let Some(d) = zu {
            if e.depth > d {
                continue;
            }
            zu = None;
        }
        out.push(i);
        if e.is_dir && !expanded.contains(&e.path) {
            zu = Some(e.depth);
        }
    }
    out
}

fn parent(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(p, _)| p)
}

fn children<'a>(entries: &'a [FileEntry], dir: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    entries.iter().filter(move |e| parent(&e.path) == Some(dir)).map(|e| e.path.as_str())
}

pub fn is_off(excluded: &[String], path: &str) -> bool {
    assets::path_excluded(excluded, path)
}

// Eine Zeile des Dateibaums an- oder ausschalten. Die Liste bleibt so kurz wie moeglich: ein
// ausgeschalteter Ordner steht fuer alles darin, und sind alle Eintraege eines Ordners aus,
// steht statt ihrer der Ordner. Wer in einem ausgeschalteten Ordner etwas einschaltet, teilt
// ihn auf: aus bleiben alle Geschwister auf dem Weg dorthin.
pub fn toggle(excluded: &mut Vec<String>, entries: &[FileEntry], path: &str) {
    if is_off(excluded, path) {
        if let Some(i) = excluded.iter().position(|e| e == path) {
            excluded.remove(i);
        } else if let Some(i) = excluded.iter().position(|e| path.strip_prefix(e.as_str()).is_some_and(|r| r.starts_with('/'))) {
            let mut ordner = excluded.remove(i);
            loop {
                let mut weiter = None;
                for kind in children(entries, &ordner) {
                    if kind == path || path.strip_prefix(kind).is_some_and(|r| r.starts_with('/')) {
                        weiter = Some(kind.to_string());
                    } else {
                        excluded.push(kind.to_string());
                    }
                }
                match weiter {
                    Some(k) if k != path => ordner = k,
                    _ => break,
                }
            }
        }
    } else {
        excluded.retain(|e| !e.strip_prefix(path).is_some_and(|r| r.starts_with('/')));
        excluded.push(path.to_string());
        let mut pfad = path.to_string();
        while let Some(ordner) = parent(&pfad).map(str::to_string) {
            let kinder: Vec<&str> = children(entries, &ordner).collect();
            if kinder.is_empty() || !kinder.iter().all(|k| excluded.iter().any(|e| e == k)) {
                break;
            }
            excluded.retain(|e| !kinder.contains(&e.as_str()));
            excluded.push(ordner.clone());
            pfad = ordner;
        }
    }
    excluded.sort();
}

#[cfg(test)]
mod tests {
    #[test]
    fn stapel_bekommt_vanilla_unten_und_server_oben() {
        let mut st = vec!["a".to_string(), "b".to_string()];
        normalize_stack(&mut st, None);
        assert_eq!(st, ["a", "b", VANILLA]);
        normalize_stack(&mut st, Some(0));
        assert_eq!(st, [SERVER, "a", "b", VANILLA]);
        // Verschoben bleibt verschoben.
        let mut st = vec!["a".to_string(), VANILLA.to_string(), SERVER.to_string(), VANILLA.to_string()];
        normalize_stack(&mut st, Some(0));
        assert_eq!(st, ["a", VANILLA, SERVER]);
        normalize_stack(&mut st, None);
        assert_eq!(st, ["a", VANILLA]);
        // Gemerkter Platz, notfalls ganz unten.
        normalize_stack(&mut st, Some(1));
        assert_eq!(st, ["a", SERVER, VANILLA]);
        let mut st = vec![VANILLA.to_string()];
        normalize_stack(&mut st, Some(5));
        assert_eq!(st, [VANILLA, SERVER]);
    }

    #[test]
    fn baum_aus_pfaden_wie_aus_ordner() {
        let baum = tree_from_paths(["textures/block/stone.png", "sounds.json", "textures/item/a.png", "textures/block/dirt.png"]);
        let zeilen: Vec<(usize, &str, bool)> = baum.iter().map(|e| (e.depth, e.path.as_str(), e.is_dir)).collect();
        assert_eq!(
            zeilen,
            [
                (0, "textures", true),
                (1, "textures/block", true),
                (2, "textures/block/dirt.png", false),
                (2, "textures/block/stone.png", false),
                (1, "textures/item", true),
                (2, "textures/item/a.png", false),
                (0, "sounds.json", false),
            ]
        );
    }

    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn links_zuletzt_herausgenommene_oben_neue_alphabetisch_darunter() {
        let alle = s(&["A", "B", "C", "D"]);
        assert_eq!(available_column(&alle, &s(&["C", "X", "A"]), &s(&["B"])), s(&["C", "A", "D"]));
    }

    #[test]
    fn verschieben_innerhalb_und_zwischen_den_spalten() {
        let (mut l, mut r) = (s(&["A", "B", "C"]), s(&["X", "Y"]));
        move_skin(&mut l, &mut r, (Column::Available, 0), (Column::Available, 2));
        assert_eq!(l, s(&["B", "C", "A"]));
        move_skin(&mut l, &mut r, (Column::Available, 1), (Column::Selected, 1));
        assert_eq!((l.clone(), r.clone()), (s(&["B", "A"]), s(&["X", "C", "Y"])));
        move_skin(&mut l, &mut r, (Column::Selected, 2), (Column::Available, 0));
        assert_eq!((l, r), (s(&["Y", "B", "A"]), s(&["X", "C"])));
    }

    fn baum() -> Vec<FileEntry> {
        let e = |path: &str, depth, is_dir| FileEntry { path: path.to_string(), name: path.rsplit('/').next().unwrap().to_string(), depth, is_dir };
        vec![
            e("textures", 0, true),
            e("textures/block", 1, true),
            e("textures/block/dirt.png", 2, false),
            e("textures/block/stone.png", 2, false),
            e("textures/item", 1, true),
            e("textures/item/apple.png", 2, false),
            e("sounds.json", 0, false),
        ]
    }

    #[test]
    fn einschalten_in_ausgeschaltetem_ordner_teilt_ihn_auf() {
        let b = baum();
        let mut aus = s(&["textures"]);
        toggle(&mut aus, &b, "textures/block/stone.png");
        assert_eq!(aus, s(&["textures/block/dirt.png", "textures/item"]));
        assert!(!is_off(&aus, "textures/block/stone.png"));
        assert!(is_off(&aus, "textures/item/apple.png"));
    }

    #[test]
    fn alle_geschwister_aus_fasst_zum_ordner_zusammen() {
        let b = baum();
        let mut aus = Vec::new();
        toggle(&mut aus, &b, "textures/block/dirt.png");
        toggle(&mut aus, &b, "textures/block/stone.png");
        assert_eq!(aus, s(&["textures/block"]));
        toggle(&mut aus, &b, "textures/item");
        assert_eq!(aus, s(&["textures"]));
        toggle(&mut aus, &b, "textures");
        assert!(aus.is_empty());
    }

    #[test]
    fn zugeklappte_ordner_verbergen_ihren_inhalt() {
        let b = baum();
        let mut auf = HashSet::new();
        assert_eq!(visible_rows(&b, &auf), vec![0, 6]);
        auf.insert("textures".to_string());
        assert_eq!(visible_rows(&b, &auf), vec![0, 1, 4, 6]);
    }
}
