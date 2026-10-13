// Himmel aus Resourcepacks im OptiFine-Format: `optifine/sky/world0/sky1.properties`,
// `sky2.properties` ... (world0 = Oberwelt, world-1 = Nether, world1 = End; aeltere Packs
// legen dasselbe unter `mcpatcher/sky/`). Jede Datei ist eine Ebene mit einem Bild im
// 3x2-Wuerfelraster, gezeichnet in Dateireihenfolge ueber der Himmelsfarbe.
//
// Das Format ist von OptiFine, die Regeln hier folgen dem, was Skyboxify (der Fabric-Nachbau,
// den Packs heute meist voraussetzen) daraus macht -- in eigenen Worten nachgebaut:
//
//   - Zeiten "hh:mm" sind Uhrzeiten, 6:00 ist Tick 0. Eine Ebene blendet zwischen
//     startFadeIn und endFadeIn ein, zwischen startFadeOut und endFadeOut aus, alles ueber
//     Mitternacht hinweg. Ohne startFadeOut gilt endFadeOut minus die Einblenddauer. Ohne
//     Zeiten ist sie immer da.
//   - weather (clear/rain/thunder) gewichtet mit der Regen- und Gewitterstaerke.
//   - heights und biomes gelten fuer die Kamera; ihr Wechsel blendet ueber `transition` Ticks.
//   - days/daysLoop: nur an bestimmten Tagen eines Zyklus (Elden Ring Sky wechselt so durch
//     fuenf Himmel).
//   - rotate/axis/speed: die Ebene dreht sich mit dem Sonnenstand um ihre Achse; die Achse
//     steht in der Datei in OptiFines eigener Reihenfolge und wird umgestellt (s. parse_axis).
//   - blend: neun Mischarten, s. render::custom_sky::Blend.
//
// Die Bilder sind gross (Elden Ring Sky: 24 Bilder zu 6144x4096). Sie werden geladen, wenn
// ihre Ebene gleich zu sehen ist, im Hintergrund dekodiert, auf hoechstens MAX_FACE je
// Wuerfelflaeche verkleinert und nach einer Minute ohne Bedarf wieder freigegeben. Beim
// Verbinden laedt die App die Oberwelt schon vorab (s. preload), damit der Himmel beim
// Betreten der Welt fertig ist.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use glam::{Mat3, Vec3};
use render::custom_sky::{Blend, SkyLayerDraw};

// Kantenlaenge einer Wuerfelflaeche nach dem Verkleinern; ein ganzes Bild hat 3x2 davon.
pub const MAX_FACE: u32 = 2048;
// So weit (in Ticks) schaut das Laden voraus, damit ein Bild zum Einblenden schon da ist.
const PREFETCH_TICKS: i64 = 600;
// Ohne Bedarf so lange behalten, dann freigeben.
const KEEP_UNUSED: Duration = Duration::from_secs(60);
// Vorab hoechstens so viel (entpackt): uebliche Packs (gemessen 90 bis 260 MB) passen ganz
// hinein, von riesigen nur die ersten Ebenen -- der Rest kommt, wenn die Uhrzeit bekannt ist.
const PRELOAD_BYTES: u64 = 384 << 20;
// Dekodier-Threads, hoechstens.
const LOADER_THREADS: usize = 4;
const DAY: i32 = 24_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub start_in: i32,
    pub end_in: i32,
    pub start_out: i32,
    pub end_out: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerDef {
    // Pfad im Ressourcenraum ("ns/pfad.png", s. assets::MinecraftInstallation::load_bytes).
    pub texture: String,
    // None = immer an.
    pub fade: Option<Fade>,
    pub blend: Blend,
    pub rotate: bool,
    pub speed: f32,
    pub axis: Vec3,
    // clear, rain, thunder.
    pub weather: [bool; 3],
    pub heights: Vec<(i32, i32)>,
    pub biomes: Vec<String>,
    // (Zykluslaenge in Tagen, erlaubte Tage).
    pub days: Option<(i32, Vec<(i32, i32)>)>,
    pub transition: u32,
}

// Die drei Welten mit eigenem Ordner.
const WORLDS: [(i32, &str); 3] = [(0, "world0"), (-1, "world-1"), (1, "world1")];

pub fn world_index(dimension: &str) -> Option<i32> {
    match dimension {
        "minecraft:overworld" => Some(0),
        "minecraft:the_nether" => Some(-1),
        "minecraft:the_end" => Some(1),
        _ => None,
    }
}

// Alle Ebenen aller Welten aus den Packs der Installation (oberstes Pack gewinnt je Datei).
// Wie OptiFine wird hochgezaehlt, bis zehn Nummern in Folge fehlen.
pub fn load_defs(installation: &mut assets::MinecraftInstallation) -> Vec<(i32, Vec<LayerDef>)> {
    let mut out = Vec::new();
    for (world, dir) in WORLDS {
        let mut layers = Vec::new();
        let mut fehlend = 0;
        for i in 1.. {
            if fehlend > 10 {
                break;
            }
            let gefunden = ["optifine", "mcpatcher"].iter().find_map(|wurzel| {
                let pfad = format!("minecraft/{wurzel}/sky/{dir}/sky{i}.properties");
                installation.load_bytes(&pfad).ok().map(|b| (pfad, b))
            });
            let Some((pfad, bytes)) = gefunden else {
                fehlend += 1;
                continue;
            };
            fehlend = 0;
            match parse_layer(&String::from_utf8_lossy(&bytes), &pfad) {
                Some(def) => layers.push(def),
                None => eprintln!("[sky] {pfad}: nicht lesbar, uebersprungen"),
            }
        }
        if !layers.is_empty() {
            out.push((world, layers));
        }
    }
    out
}

// Java-Properties: "schluessel=wert" oder "schluessel:wert", '#'/'!' leitet Kommentare ein,
// '\' schuetzt das folgende Zeichen ("18\:10").
fn parse_properties(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for zeile in text.lines() {
        let zeile = zeile.trim();
        if zeile.is_empty() || zeile.starts_with('#') || zeile.starts_with('!') {
            continue;
        }
        let mut key = String::new();
        let mut value = String::new();
        let mut in_value = false;
        let mut escaped = false;
        for c in zeile.chars() {
            let ziel = if in_value { &mut value } else { &mut key };
            if escaped {
                ziel.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if !in_value && (c == '=' || c == ':') {
                in_value = true;
            } else {
                ziel.push(c);
            }
        }
        out.insert(key.trim().to_string(), value.trim().to_string());
    }
    out
}

pub fn parse_layer(text: &str, props_path: &str) -> Option<LayerDef> {
    let p = parse_properties(text);
    let texture = resolve_source(p.get("source").map(String::as_str), props_path)?;
    let fade = match (p.get("startFadeIn"), p.get("endFadeIn"), p.get("endFadeOut")) {
        (Some(a), Some(b), Some(d)) => {
            let (start_in, end_in, end_out) = (tick_time(a), tick_time(b), tick_time(d));
            let start_out = match p.get("startFadeOut") {
                Some(c) => tick_time(c),
                None => {
                    let s = end_out - (end_in - start_in);
                    if (start_in..=end_in).contains(&s) {
                        end_out
                    } else {
                        s
                    }
                }
            };
            let n = |t: i32| t.rem_euclid(DAY);
            Some(Fade { start_in: n(start_in), end_in: n(end_in), start_out: n(start_out), end_out: n(end_out) })
        }
        _ => None,
    };
    let blend = p.get("blend").map_or(Some(Blend::Add), |b| Blend::from_name(b))?;
    let rotate = p.get("rotate").is_none_or(|r| r.eq_ignore_ascii_case("true"));
    let speed = p.get("speed").and_then(|s| s.parse::<f32>().ok()).filter(|s| s.is_finite()).unwrap_or(1.0);
    let axis = p.get("axis").and_then(|a| parse_axis(a)).unwrap_or(Vec3::X);
    let weather = match p.get("weather") {
        Some(w) => {
            let mut out = [false; 3];
            for t in w.split_whitespace() {
                match t.to_ascii_lowercase().as_str() {
                    "clear" => out[0] = true,
                    "rain" => out[1] = true,
                    "thunder" => out[2] = true,
                    _ => {}
                }
            }
            out
        }
        None => [true, false, false],
    };
    let heights = p.get("heights").map(|h| parse_ranges(h)).unwrap_or_default();
    let biomes = p
        .get("biomes")
        .map(|b| b.split_whitespace().map(|n| if n.contains(':') { n.to_string() } else { format!("minecraft:{n}") }).collect())
        .unwrap_or_default();
    let days = p.get("days").map(|d| {
        let lang = p.get("daysLoop").and_then(|l| l.trim().parse::<i32>().ok()).filter(|l| *l > 0).unwrap_or(8);
        (lang, parse_ranges(d))
    });
    let transition = p.get("transition").and_then(|t| t.trim().parse::<u32>().ok()).unwrap_or(20);
    Some(LayerDef { texture, fade, blend, rotate, speed, axis, weather, heights, biomes, days, transition })
}

// Wo das Bild liegt. Ohne Angabe: gleicher Name wie die Datei, nur .png. "./" = neben der
// Datei, "~/" = im optifine-Ordner, "ns:pfad" = Namensraum, sonst unter minecraft/.
fn resolve_source(source: Option<&str>, props_path: &str) -> Option<String> {
    let ordner = props_path.rsplit_once('/').map_or("", |(d, _)| d);
    let Some(s) = source.map(str::trim).filter(|s| !s.is_empty()) else {
        return props_path.strip_suffix(".properties").map(|p| format!("{p}.png"));
    };
    let pfad = if let Some(rest) = s.strip_prefix("./") {
        format!("{ordner}/{rest}")
    } else if let Some(rest) = s.strip_prefix("~/") {
        format!("minecraft/optifine/{rest}")
    } else if let Some((ns, rest)) = s.split_once(':') {
        format!("{ns}/{rest}")
    } else if let Some(rest) = s.strip_prefix("assets/") {
        rest.to_string()
    } else {
        format!("minecraft/{s}")
    };
    Some(pfad)
}

// "hh:mm" -> Tick (6:00 = 0). Ungueltig -> -1, das danach wie bei Skyboxify auf 23999
// faellt statt die Ebene zu verwerfen.
fn tick_time(s: &str) -> i32 {
    let Some((h, m)) = s.trim().split_once(':') else { return -1 };
    let (Ok(h), Ok(m)) = (h.trim().parse::<i32>(), m.trim().parse::<i32>()) else { return -1 };
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) {
        return -1;
    }
    (h - 6).rem_euclid(24) * 1000 + (m as f32 / 60.0 * 1000.0) as i32
}

// OptiFines Achse "x y z" steht in einem anderen Raum als die Drehung: intern gilt (z, y, -x).
fn parse_axis(s: &str) -> Option<Vec3> {
    let v: Vec<f32> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let [x, y, z] = v[..] else { return None };
    let axis = Vec3::new(z, y, -x);
    (axis.length_squared() > 0.0).then_some(axis)
}

// "0-255", "256-10000 5", "1,3-4" -> Bereiche (auch mit negativen Zahlen: "-64-0").
fn parse_ranges(s: &str) -> Vec<(i32, i32)> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| {
            let teil = t.char_indices().skip(1).find(|&(_, c)| c == '-').map(|(i, _)| (&t[..i], &t[i + 1..]));
            match teil {
                Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
                None => t.parse().ok().map(|v| (v, v)),
            }
        })
        .collect()
}

fn in_ranges(ranges: &[(i32, i32)], v: i32) -> bool {
    ranges.iter().any(|&(a, b)| a <= v && v <= b)
}

// Liegt t im Zeitfenster [a, b], auch ueber Mitternacht?
fn in_interval(t: i32, a: i32, b: i32) -> bool {
    if !(0..DAY).contains(&t) {
        return false;
    }
    if a <= b {
        a <= t && t <= b
    } else {
        t >= a || t <= b
    }
}

fn cyclic(a: i32, b: i32) -> i32 {
    (b - a + DAY) % DAY
}

pub fn fade_alpha(fade: Option<Fade>, t: i32) -> f32 {
    let Some(f) = fade else { return 1.0 };
    let anteil = |von: i32, bis: i32| {
        let ganz = cyclic(von, bis);
        if ganz == 0 { 1.0 } else { cyclic(von, t) as f32 / ganz as f32 }
    };
    if in_interval(t, f.end_in, f.start_out) {
        1.0
    } else if in_interval(t, f.start_in, f.end_in) {
        anteil(f.start_in, f.end_in)
    } else if in_interval(t, f.start_out, f.end_out) {
        1.0 - anteil(f.start_out, f.end_out)
    } else {
        0.0
    }
}

pub fn weather_alpha(w: [bool; 3], rain: f32, thunder: f32) -> f32 {
    let mut a = 0.0;
    if w[0] {
        a += 1.0 - rain;
    }
    if w[1] {
        a += rain - thunder;
    }
    if w[2] {
        a += thunder;
    }
    a.clamp(0.0, 1.0)
}

// Zeitfenster und Tageszyklus: ausserhalb ist die Ebene ganz aus.
pub fn is_active(def: &LayerDef, day_time: i64, t: i32) -> bool {
    if let Some(f) = def.fade {
        if in_interval(t, f.end_out, f.start_in) {
            return false;
        }
    }
    let Some((lang, tage)) = &def.days else { return true };
    let start = def.fade.map_or(0, |f| f.start_in) as i64;
    let tag = (day_time - start).div_euclid(DAY as i64).rem_euclid(*lang as i64) as i32;
    tage.is_empty() || in_ranges(tage, tag)
}

// Drehwinkel in Grad. Bei nicht ganzzahligem Tempo laeuft pro Tag ein Rest auf, damit der
// Himmel nicht jeden Tag an derselben Stelle beginnt.
pub fn angle(def: &LayerDef, day_time: i64, celestial: f32) -> f32 {
    let mut tag_start = 0.0;
    if def.speed != def.speed.round() {
        let tag = (day_time + 18_000) / DAY as i64;
        tag_start = (tag as f32 * (def.speed % 1.0)) % 1.0;
    }
    360.0 * (tag_start + celestial * def.speed)
}

// Welt-Richtung -> Raum der Ebene: die Grunddrehung (-90 Grad um Y, wie beim Vanilla-Himmel)
// und die Ebenendrehung, invertiert (fuer Drehungen: transponiert).
pub fn to_local(def: &LayerDef, angle_deg: f32) -> Mat3 {
    let basis = Mat3::from_rotation_y((-90.0f32).to_radians());
    let dreh = if def.rotate { Mat3::from_axis_angle(def.axis.normalize(), angle_deg.to_radians()) } else { Mat3::IDENTITY };
    (basis * dreh).transpose()
}

// Dieselbe Abbildung wie sky_uv in render/custom_sky.wgsl (ohne die Klemmung an den
// Zellraendern) -- fuer die Pruefung an echten Bildern.
#[cfg(test)]
pub fn sky_uv(l: Vec3) -> (f32, f32) {
    let a = l.abs();
    let (cell, x, z) = if a.y >= a.x && a.y >= a.z {
        if l.y < 0.0 { ((0.0, 0.0), l.z / l.y, -l.x / l.y) } else { ((1.0, 0.0), -l.z / l.y, -l.x / l.y) }
    } else if a.x >= a.z {
        if l.x > 0.0 { ((2.0, 0.0), l.z / l.x, -l.y / l.x) } else { ((1.0, 1.0), l.z / l.x, l.y / l.x) }
    } else if l.z > 0.0 {
        ((0.0, 1.0), -l.x / l.z, -l.y / l.z)
    } else {
        ((2.0, 1.0), -l.x / l.z, l.y / l.z)
    };
    ((cell.0 + x * 0.5 + 0.5) / 3.0, (cell.1 + z * 0.5 + 0.5) / 2.0)
}

// Ein Bild, das (noch) nicht oder schon auf der GPU liegt.
enum TexState {
    Idle,
    Loading,
    Ready(u32),
    Failed,
}

struct Tex {
    path: String,
    state: TexState,
    last_needed: Instant,
    // Groesse entpackt und verkleinert in Bytes, sobald die Datei einmal gelesen war.
    size: Option<u64>,
}

struct Layer {
    def: LayerDef,
    texture: usize,
    // Blendwert fuer heights/biomes; None = noch nie ausgewertet (springt dann direkt).
    cond: Option<f32>,
}

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

// Was ein Frame braucht, um den Himmel zu bestimmen.
pub struct SkyFrame {
    pub world: i32,
    pub day_time: i64,
    pub celestial: f32,
    pub rain: f32,
    pub thunder: f32,
}

pub struct CustomSky {
    worlds: Vec<(i32, Vec<Layer>)>,
    textures: Vec<Tex>,
    to_loader: Option<Sender<(usize, Vec<u8>, u32)>>,
    from_loader: Option<Receiver<(usize, Result<Decoded, String>)>>,
}

impl CustomSky {
    pub fn new(defs: Vec<(i32, Vec<LayerDef>)>) -> Self {
        let mut textures: Vec<Tex> = Vec::new();
        let worlds = defs
            .into_iter()
            .map(|(world, layers)| {
                let layers = layers
                    .into_iter()
                    .map(|def| {
                        let texture = match textures.iter().position(|t| t.path == def.texture) {
                            Some(i) => i,
                            None => {
                                textures.push(Tex { path: def.texture.clone(), state: TexState::Idle, last_needed: Instant::now(), size: None });
                                textures.len() - 1
                            }
                        };
                        Layer { def, texture, cond: None }
                    })
                    .collect();
                (world, layers)
            })
            .collect();
        Self { worlds, textures, to_loader: None, from_loader: None }
    }

    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }

    pub fn layer_count(&self) -> usize {
        self.worlds.iter().map(|(_, l)| l.len()).sum()
    }

    // GPU-Bilder, die freigegeben werden muessen, wenn dieser Himmel ersetzt wird.
    pub fn gpu_textures(&self) -> Vec<u32> {
        self.textures.iter().filter_map(|t| if let TexState::Ready(id) = t.state { Some(id) } else { None }).collect()
    }

    fn layers_mut(&mut self, world: i32) -> Option<&mut Vec<Layer>> {
        self.worlds.iter_mut().find(|(w, _)| *w == world).map(|(_, l)| l)
    }

    // Einmal pro Spieltick: Hoehe und Biom der Kamera blenden die Ebenen ueber `transition`.
    pub fn tick(&mut self, world: i32, block_y: i32, biome: &str) {
        let Some(layers) = self.layers_mut(world) else { return };
        for layer in layers {
            let def = &layer.def;
            if def.heights.is_empty() && def.biomes.is_empty() {
                layer.cond = Some(1.0);
                continue;
            }
            let ok = (def.heights.is_empty() || in_ranges(&def.heights, block_y)) && (def.biomes.is_empty() || def.biomes.iter().any(|b| b == biome));
            let ziel = if ok { 1.0 } else { 0.0 };
            layer.cond = Some(match layer.cond {
                None => ziel,
                Some(a) if def.transition == 0 => {
                    let _ = a;
                    ziel
                }
                Some(a) => {
                    let schritt = 1.0 / def.transition as f32;
                    if ok { (a + schritt).min(1.0) } else { (a - schritt).max(0.0) }
                }
            });
        }
    }

    // Die Ebenen dieses Frames (nur die mit fertigem Bild) und welche Bilder geladen werden
    // muessen. `load` liefert die Dateibytes eines Bildes (aus der Installation).
    pub fn frame(&mut self, f: &SkyFrame, max_dim: u32, load: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Vec<SkyLayerDraw> {
        let jetzt = Instant::now();
        let t = f.day_time.rem_euclid(DAY as i64) as i32;
        let spaeter = f.day_time + PREFETCH_TICKS;
        let t_spaeter = spaeter.rem_euclid(DAY as i64) as i32;
        let mut draws = Vec::new();
        let mut gebraucht = Vec::new();
        if let Some((_, layers)) = self.worlds.iter().find(|(w, _)| *w == f.world) {
            for layer in layers {
                let def = &layer.def;
                let wetter = weather_alpha(def.weather, f.rain, f.thunder);
                let cond = layer.cond.unwrap_or(0.0);
                if wetter <= 0.0 || cond <= 0.0 {
                    continue;
                }
                let alpha = if is_active(def, f.day_time, t) { (cond * wetter * fade_alpha(def.fade, t)).clamp(0.0, 1.0) } else { 0.0 };
                let bald = is_active(def, spaeter, t_spaeter) && fade_alpha(def.fade, t_spaeter) > 0.0;
                if alpha >= 1e-4 || bald {
                    gebraucht.push(layer.texture);
                }
                if alpha < 1e-4 {
                    continue;
                }
                if let TexState::Ready(id) = self.textures[layer.texture].state {
                    let winkel = angle(def, f.day_time, f.celestial);
                    draws.push(SkyLayerDraw { texture: id, blend: def.blend, to_local: to_local(def, winkel), brightness: alpha });
                }
            }
        }
        for i in gebraucht {
            self.textures[i].last_needed = jetzt;
            if matches!(self.textures[i].state, TexState::Idle) {
                if let Some(bytes) = self.read(i, max_dim, load) {
                    self.request(i, bytes, max_dim);
                }
            }
        }
        draws
    }

    // Beim Verbinden und nach einem Dimensionswechsel: die Bilder einer Welt in
    // Dateireihenfolge laden und halten, bis PRELOAD_BYTES erreicht ist. Jeden Frame
    // aufgerufen; schon geladene gelten als gebraucht und werden nicht freigegeben. true,
    // sobald alles innerhalb des Budgets fertig (oder kaputt) ist.
    pub fn preload(&mut self, world: i32, max_dim: u32, load: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> bool {
        let Some((_, layers)) = self.worlds.iter().find(|(w, _)| *w == world) else { return true };
        let mut reihe: Vec<usize> = Vec::new();
        for layer in layers {
            if !reihe.contains(&layer.texture) {
                reihe.push(layer.texture);
            }
        }
        let jetzt = Instant::now();
        let mut summe = 0u64;
        let mut fertig = true;
        for i in reihe {
            if matches!(self.textures[i].state, TexState::Failed) {
                continue;
            }
            // Die Groesse steht erst nach dem ersten Lesen fest; was danach nicht mehr passt,
            // wird nicht noch einmal gelesen.
            let bytes = match self.textures[i].size {
                Some(n) if summe + n > PRELOAD_BYTES => break,
                Some(_) if !matches!(self.textures[i].state, TexState::Idle) => None,
                _ => match self.read(i, max_dim, load) {
                    Some(b) => Some(b),
                    None => continue,
                },
            };
            let n = self.textures[i].size.unwrap_or(0);
            if summe + n > PRELOAD_BYTES {
                break;
            }
            summe += n;
            self.textures[i].last_needed = jetzt;
            if let Some(b) = bytes.filter(|_| matches!(self.textures[i].state, TexState::Idle)) {
                self.request(i, b, max_dim);
            }
            fertig &= matches!(self.textures[i].state, TexState::Ready(_) | TexState::Failed);
        }
        fertig
    }

    // Die Datei eines Bildes lesen und seine Groesse merken; fehlt sie, gilt es als kaputt.
    fn read(&mut self, i: usize, max_dim: u32, load: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
        let tex = &mut self.textures[i];
        let Some(bytes) = load(&tex.path) else {
            eprintln!("[sky] Bild fehlt: {}", tex.path);
            tex.state = TexState::Failed;
            return None;
        };
        let (w, h) = png_size(&bytes).map_or((0, 0), |(w, h)| shrunk_size(w, h, max_dim));
        tex.size = Some(u64::from(w) * u64::from(h) * 4);
        Some(bytes)
    }

    fn request(&mut self, i: usize, bytes: Vec<u8>, max_dim: u32) {
        self.textures[i].state = TexState::Loading;
        let tx = self.to_loader.get_or_insert_with(|| {
            let (tx, rx) = start_loader();
            self.from_loader = Some(rx);
            tx
        });
        let _ = tx.send((i, bytes, max_dim));
    }

    // Fertig dekodierte Bilder abholen (gehen dann an die GPU, s. App) und lange nicht
    // gebrauchte zurueckgeben, damit ihre GPU-Bilder frei werden.
    pub fn poll(&mut self, add: &mut dyn FnMut(Decoded) -> u32) -> Vec<u32> {
        if let Some(rx) = &self.from_loader {
            while let Ok((i, ergebnis)) = rx.try_recv() {
                let Some(tex) = self.textures.get_mut(i) else { continue };
                tex.state = match ergebnis {
                    Ok(bild) => TexState::Ready(add(bild)),
                    Err(e) => {
                        eprintln!("[sky] {}: {e}", tex.path);
                        TexState::Failed
                    }
                };
            }
        }
        let mut frei = Vec::new();
        for tex in &mut self.textures {
            if let TexState::Ready(id) = tex.state {
                if tex.last_needed.elapsed() > KEEP_UNUSED {
                    frei.push(id);
                    tex.state = TexState::Idle;
                }
            }
        }
        frei
    }
}

// Ein paar Hintergrund-Threads dekodieren die Bilder (beim Vorladen sind es gleich viele auf
// einmal). Sie enden, wenn der Himmel (und mit ihm der Sender) ersetzt wird.
fn start_loader() -> (Sender<(usize, Vec<u8>, u32)>, Receiver<(usize, Result<Decoded, String>)>) {
    let (tx_job, rx_job) = std::sync::mpsc::channel::<(usize, Vec<u8>, u32)>();
    let (tx_done, rx_done) = std::sync::mpsc::channel();
    let rx_job = std::sync::Arc::new(std::sync::Mutex::new(rx_job));
    let threads = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).clamp(1, LOADER_THREADS));
    for _ in 0..threads {
        let (rx_job, tx_done) = (rx_job.clone(), tx_done.clone());
        let _ = std::thread::Builder::new().name("custom-sky".into()).spawn(move || loop {
            let job = rx_job.lock().map_err(|_| ()).and_then(|rx| rx.recv().map_err(|_| ()));
            let Ok((i, bytes, max_dim)) = job else { break };
            if tx_done.send((i, decode(&bytes, max_dim))).is_err() {
                break;
            }
        });
    }
    (tx_job, rx_done)
}

fn too_big(w: u32, h: u32, max_dim: u32) -> bool {
    w / 3 > MAX_FACE || h / 2 > MAX_FACE || w > max_dim || h > max_dim
}

// Die Groesse nach dem Verkleinern in decode.
fn shrunk_size(mut w: u32, mut h: u32, max_dim: u32) -> (u32, u32) {
    while too_big(w, h, max_dim) {
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    (w, h)
}

// Breite und Hoehe aus dem Kopf einer PNG-Datei.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let zahl = |a: usize| u32::from_be_bytes([bytes[a], bytes[a + 1], bytes[a + 2], bytes[a + 3]]);
    Some((zahl(16), zahl(20)))
}

fn decode(bytes: &[u8], max_dim: u32) -> Result<Decoded, String> {
    let bild = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
    let (mut w, mut h) = bild.dimensions();
    let mut rgba = bild.into_raw();
    while too_big(w, h, max_dim) {
        (rgba, w, h) = halve(&rgba, w, h);
    }
    Ok(Decoded { width: w, height: h, rgba })
}

// Auf halbe Groesse, je 2x2 Pixel gemittelt.
fn halve(rgba: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let px = |xx: u32, yy: u32| rgba[((yy.min(h - 1) * w + xx.min(w - 1)) * 4 + c) as usize] as u32;
                let sum = px(2 * x, 2 * y) + px(2 * x + 1, 2 * y) + px(2 * x, 2 * y + 1) + px(2 * x + 1, 2 * y + 1);
                out[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
            }
        }
    }
    (out, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(text: &str) -> LayerDef {
        parse_layer(text, "minecraft/optifine/sky/world0/sky1.properties").unwrap()
    }

    #[test]
    fn uhrzeit_in_ticks() {
        assert_eq!(tick_time("6:00"), 0);
        assert_eq!(tick_time("5:30"), 23_500);
        assert_eq!(tick_time("18:30"), 12_500);
        assert_eq!(tick_time("00:01"), 18_016);
        assert_eq!(tick_time("25:00"), -1);
    }

    #[test]
    fn properties_mit_escapes_und_kommentaren() {
        let p = parse_properties("#Sun Jul 28 2013\nstartFadeIn=18\\:10\nblend = add\n! auch Kommentar\nsource=./a.png\n");
        assert_eq!(p["startFadeIn"], "18:10");
        assert_eq!(p["blend"], "add");
        assert_eq!(p["source"], "./a.png");
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn quellen_werden_aufgeloest() {
        let props = "minecraft/optifine/sky/world0/sky3.properties";
        assert_eq!(resolve_source(None, props).as_deref(), Some("minecraft/optifine/sky/world0/sky3.png"));
        assert_eq!(resolve_source(Some("./cloud1.png"), props).as_deref(), Some("minecraft/optifine/sky/world0/cloud1.png"));
        assert_eq!(resolve_source(Some("skybox:overworld/sky1_day1.png"), props).as_deref(), Some("skybox/overworld/sky1_day1.png"));
        assert_eq!(resolve_source(Some("~/sky/x.png"), props).as_deref(), Some("minecraft/optifine/sky/x.png"));
        assert_eq!(resolve_source(Some("textures/a.png"), props).as_deref(), Some("minecraft/textures/a.png"));
    }

    // Elden Ring Sky, sky1: Tag, 5:30 bis 18:30, nur am fuenften Tag eines 5-Tage-Zyklus.
    #[test]
    fn elden_tagesebene() {
        let d = def("source=skybox:overworld/sky1_day1.png\nblend=alpha\nstartFadeIn=5:30\nendFadeIn=6:30\nstartFadeOut=17:30\nendFadeOut=18:30\nrotate=true\naxis=0.0 1.0 0.0\nspeed=1\nweather=clear\ndaysLoop=5\ndays=4\n");
        assert_eq!(d.fade, Some(Fade { start_in: 23_500, end_in: 500, start_out: 11_500, end_out: 12_500 }));
        assert_eq!(d.blend, Blend::Alpha);
        assert_eq!(d.axis, Vec3::new(0.0, 1.0, -0.0));
        assert_eq!(d.weather, [true, false, false]);
        assert_eq!(fade_alpha(d.fade, 6000), 1.0);
        assert_eq!(fade_alpha(d.fade, 0), 0.5);
        assert_eq!(fade_alpha(d.fade, 12_000), 0.5);
        assert_eq!(fade_alpha(d.fade, 18_000), 0.0);
        // Gezaehlt wird ab dem Einblenden (5:30 am Vortag): Mittag von Tag 0 gehoert schon zum
        // Zyklustag -1, also 4.
        let mittag = |tag: i64| tag * 24_000 + 6000;
        assert!(is_active(&d, mittag(0), 6000));
        assert!(!is_active(&d, mittag(1), 6000));
        assert!(!is_active(&d, mittag(4), 6000));
        assert!(is_active(&d, mittag(5), 6000));
        assert!(is_active(&d, mittag(10), 6000));
        assert!(!is_active(&d, mittag(5) + 12_000, 18_000), "nachts aus");
    }

    // AuroraPrac: kein startFadeOut -> endFadeOut minus Einblenddauer.
    #[test]
    fn ausblenden_ohne_start() {
        let d = def("startFadeIn=17:30\nendFadeIn=20:00\nendFadeOut=6:10\nblend=add\nsource=./starfield01.png\n");
        let f = d.fade.unwrap();
        assert_eq!((f.start_in, f.end_in, f.end_out), (11_500, 14_000, 166));
        assert_eq!(f.start_out, 166 - 2500 + 24_000);
        assert_eq!(d.axis, Vec3::X, "ohne Angabe: Sonnenachse");
    }

    #[test]
    fn wetter_gewichtet() {
        assert_eq!(weather_alpha([true, false, false], 0.0, 0.0), 1.0);
        assert_eq!(weather_alpha([true, false, false], 1.0, 0.0), 0.0);
        assert_eq!(weather_alpha([false, true, false], 1.0, 0.25), 0.75);
        assert_eq!(weather_alpha([false, true, true], 1.0, 1.0), 1.0);
    }

    #[test]
    fn bereiche_auch_negativ() {
        assert_eq!(parse_ranges("0-255"), vec![(0, 255)]);
        assert_eq!(parse_ranges("256-10000 5"), vec![(256, 10000), (5, 5)]);
        assert_eq!(parse_ranges("-64-0, 3"), vec![(-64, 0), (3, 3)]);
    }

    #[test]
    fn vorladen_haelt_sich_an_das_budget() {
        // Sechs Bilder, die verkleinert je 6144x4096 (96 MiB) ergeben: vier passen.
        let defs = (0..6).map(|i| def(&format!("source=./s{i}.png\n"))).collect();
        let mut sky = CustomSky::new(vec![(0, defs)]);
        let mut kopf = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        kopf.extend_from_slice(&12288u32.to_be_bytes());
        kopf.extend_from_slice(&8192u32.to_be_bytes());
        let mut gelesen = 0;
        let mut load = |_: &str| {
            gelesen += 1;
            Some(kopf.clone())
        };
        sky.preload(0, 16384, &mut load);
        assert!(!sky.preload(0, 16384, &mut load), "laedt noch");
        let laedt = sky.textures.iter().filter(|t| matches!(t.state, TexState::Loading)).count();
        assert_eq!(laedt, 4);
        // Das fuenfte wurde einmal gelesen (Groesse), danach nicht mehr.
        assert_eq!(gelesen, 5);
    }

    #[test]
    fn hoehe_blendet_ueber_transition() {
        let d = def("source=./a.png\nheights=0-255\ntransition=4\n");
        let mut sky = CustomSky::new(vec![(0, vec![d])]);
        sky.tick(0, 100, "minecraft:plains");
        assert_eq!(sky.worlds[0].1[0].cond, Some(1.0), "erster Wert springt");
        sky.tick(0, 300, "minecraft:plains");
        assert_eq!(sky.worlds[0].1[0].cond, Some(0.75));
        for _ in 0..5 {
            sky.tick(0, 300, "minecraft:plains");
        }
        assert_eq!(sky.worlds[0].1[0].cond, Some(0.0));
    }

    // Die sechs Flaechen decken alle Richtungen ab, jede Richtung landet in ihrer Zelle, und
    // die Mitte jeder Flaeche liegt in der Mitte ihrer Zelle.
    #[test]
    fn wuerfelraster() {
        let mitte = |l: Vec3| sky_uv(l);
        assert_eq!(mitte(Vec3::NEG_Y), (0.5 / 3.0, 0.25));
        assert_eq!(mitte(Vec3::Y), (1.5 / 3.0, 0.25));
        assert_eq!(mitte(Vec3::X), (2.5 / 3.0, 0.25));
        assert_eq!(mitte(Vec3::Z), (0.5 / 3.0, 0.75));
        assert_eq!(mitte(Vec3::NEG_X), (1.5 / 3.0, 0.75));
        assert_eq!(mitte(Vec3::NEG_Z), (2.5 / 3.0, 0.75));
        // Seitenflaechen: oben im Raum der Ebene ist oben in der Zelle.
        for l in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
            let (_, v_mitte) = sky_uv(l);
            let (_, v_hoch) = sky_uv((l + Vec3::Y * 0.5).normalize());
            assert!(v_hoch < v_mitte, "{l}");
        }
    }

    // An einem echten Pack nachgeprueft: entlang jeder Wuerfelkante muessen die beiden
    // angrenzenden Flaechen dieselbe Farbe zeigen. Eine falsche Zuordnung (gedrehte oder
    // vertauschte Zelle) bricht an mindestens einer Kante deutlich.
    #[test]
    #[ignore]
    fn kanten_passen_an_echtem_bild() {
        let zip = std::path::Path::new(&std::env::var("HOME").unwrap()).join("Downloads/Elden_Ring_Sky_[v1.9].zip");
        let bytes = std::process::Command::new("unzip").arg("-p").arg(&zip).arg("assets/skybox/overworld/sky1_day1.png").output().unwrap().stdout;
        let bild = decode(&bytes, 16384).unwrap();
        let farbe = |l: Vec3| {
            let (u, v) = sky_uv(l.normalize());
            let x = ((u * bild.width as f32) as u32).min(bild.width - 1);
            let y = ((v * bild.height as f32) as u32).min(bild.height - 1);
            let i = ((y * bild.width + x) * 4) as usize;
            Vec3::new(bild.rgba[i] as f32, bild.rgba[i + 1] as f32, bild.rgba[i + 2] as f32)
        };
        let mut schlimmste: f32 = 0.0;
        let achsen = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
        for a in achsen {
            for b in achsen {
                if a.dot(b) != 0.0 {
                    continue;
                }
                let c = a.cross(b);
                let mut summe = 0.0;
                for k in -8..=8 {
                    let entlang = c * (k as f32 / 10.0);
                    let kante = a + b + entlang;
                    let eps = 0.004;
                    summe += (farbe(kante - b * eps) - farbe(kante - a * eps)).length();
                }
                schlimmste = schlimmste.max(summe / 17.0);
            }
        }
        println!("groesster mittlerer Farbsprung an einer Kante: {schlimmste:.1}");
        assert!(schlimmste < 20.0, "Kanten passen nicht: {schlimmste}");
    }
}
