// Licht fuer den Shader-Pfad: Himmelslicht vom Server, farbiges Blocklicht aus dem Client.
//
// Himmelslicht schickt der Server mit jedem Chunk und in "Update Light" -- es wird nur
// gespeichert (SkyLayer, je Section 2048 Byte oder ein einziger Wert). Blocklicht rechnet der
// Client selbst, und zwar FARBIG: jede Lichtquelle hat Vanillas Leuchtstaerke und eine Farbe
// (Fackel warm, Seelenfeuer blau, Redstone rot ...), und die drei Kanaele breiten sich wie
// Vanillas Blocklicht aus -- je Block eine Stufe weniger, durch Bloecke mit Lichtdaempfung
// entsprechend mehr, an vollen Seitenflaechen (Stufen, Treppen) gar nicht.
//
// Leuchtstaerke, Lichtdaempfung und volle Seitenflaechen je Block-State stehen in
// light_data.bin, maschinell aus dem 1.21.11-Jar gelesen (BlockState.getLuminance,
// getOpacity, hasSidedTransparency, Block.isFaceFullSquare(getCullingFace)).
//
// Licht reicht hoechstens 14 Bloecke weit. Das Blocklicht einer Spalte haengt deshalb nur von
// ihr und ihren acht Nachbarn ab -- der Mesh-Worker rechnet es direkt vor dem Meshen aus
// (light_field), ohne dass irgendwo Blocklicht gespeichert oder nachgefuehrt werden muss.

use std::sync::Arc;

use crate::chunk::{ChunkColumn, SECTION_SIZE};
use crate::mesher::ColumnNeighbors;

static DATA: &[u8] = include_bytes!("light_data.bin");

fn entry(state: u32) -> [u8; 3] {
    let i = 4 + state as usize * 3;
    match DATA.get(i..i + 3) {
        Some(e) => [e[0], e[1], e[2]],
        None => [0, 0, 0],
    }
}

pub fn luminance(state: u32) -> u8 {
    entry(state)[0]
}

pub fn opacity(state: u32) -> u8 {
    entry(state)[1]
}

// Richtungen wie in light_data.bin: unten, oben, Norden (-z), Sueden (+z), Westen (-x), Osten (+x).
const DIRS: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

fn opposite(d: usize) -> usize {
    d ^ 1
}

// Sperrt die Seite `d` dieses Blocks das Licht? Nur Bloecke, deren Form fuer Licht zaehlt
// (Stufen, Treppen, Schnee ...), und nur an vollen Seitenflaechen.
fn face_blocks(state: u32, d: usize) -> bool {
    let f = entry(state)[2];
    f & 1 != 0 && f & (2 << d) != 0
}

// --- Farben ---------------------------------------------------------------------------------

// Lichtfarbe einer Quelle (hellster Kanal = 1). Ohne Eintrag weiss.
fn light_color(name: &str) -> [f32; 3] {
    const WARM: [f32; 3] = [1.0, 0.72, 0.42];
    const SEELE: [f32; 3] = [0.35, 0.75, 1.0];
    const REDSTONE: [f32; 3] = [1.0, 0.22, 0.12];
    const KALT: [f32; 3] = [0.75, 0.92, 1.0];
    const LILA: [f32; 3] = [0.72, 0.42, 1.0];
    const GRUEN: [f32; 3] = [0.5, 1.0, 0.45];
    match name {
        n if n.starts_with("soul_") => SEELE,
        "redstone_torch" | "redstone_wall_torch" | "redstone_ore" | "deepslate_redstone_ore" | "redstone_block" => REDSTONE,
        "glowstone" | "shroomlight" | "redstone_lamp" | "ochre_froglight" => [1.0, 0.82, 0.5],
        "sea_lantern" | "beacon" | "conduit" | "end_rod" | "end_gateway" | "end_portal" => KALT,
        "verdant_froglight" | "copper_torch" | "copper_wall_torch" | "copper_lantern" | "glow_lichen" | "cave_vines" | "cave_vines_plant" => GRUEN,
        "pearlescent_froglight" => [0.95, 0.6, 1.0],
        "crying_obsidian" | "respawn_anchor" | "nether_portal" | "enchanting_table" | "ender_chest" | "dragon_egg" => LILA,
        n if n.contains("amethyst") => LILA,
        n if n.contains("sculk") => [0.25, 0.8, 1.0],
        "magma_block" | "lava" | "fire" | "campfire" | "furnace" | "smoker" | "blast_furnace" => [1.0, 0.55, 0.25],
        n if n.contains("candle") => [1.0, 0.7, 0.4],
        "torch" | "wall_torch" | "lantern" | "jack_o_lantern" => WARM,
        _ => WARM,
    }
}

// --- Eigene Lichtstaerken -------------------------------------------------------------

// Faktor je Blockname auf die Leuchtstaerke (Einstellung "Lichtstaerken"), dazu ein Faktor
// fuer alle Quellen (ALL_LIGHTS). Gilt fuer das eigene Blocklicht des Shader-Pfads.
//
// Stufe 15 ist dabei keine Obergrenze: die Ausbreitung hier laeuft zwar hoechstens mit 15,
// weil eine Spalte ihr Blocklicht nur aus sich und ihren acht Nachbarn rechnet (s. oben) --
// staerkeres Licht hoerte sonst an der Chunkgrenze auf. Was darueber hinausgeht
// (emission_level), rechnet der Shader-Pfad als Fernlicht selbst, mit Strahl zur Quelle.
static LIGHT_FACTORS: std::sync::RwLock<Vec<(String, f32)>> = std::sync::RwLock::new(Vec::new());

/// Schluessel des Faktors fuer alle Lichtquellen (wirkt zusaetzlich zum eigenen).
pub const ALL_LIGHTS: &str = "*alle";
/// Schluessel des Faktors fuer Sonnen- und Himmelslicht (wertet der Shader-Pfad aus).
pub const SUNLIGHT: &str = "*sonne";

/// Faktoren je Blockname setzen (fehlende = 1). Danach muss neu gemesht werden.
pub fn set_light_factors(factors: Vec<(String, f32)>) {
    *LIGHT_FACTORS.write().unwrap_or_else(|e| e.into_inner()) = factors;
}

/// Gespeicherter Faktor eines Schluessels (Blockname, ALL_LIGHTS, SUNLIGHT), sonst 1.
pub fn stored_light_factor(key: &str) -> f32 {
    let f = LIGHT_FACTORS.read().unwrap_or_else(|e| e.into_inner());
    f.iter().find(|(n, _)| n == key).map_or(1.0, |&(_, v)| v)
}

// Wirksamer Faktor eines Blocks: sein eigener mal der fuer alle.
fn light_factor(name: &str) -> f32 {
    stored_light_factor(light_source_name(name)) * stored_light_factor(ALL_LIGHTS)
}

/// Unter welchem Namen eine Quelle in den Lichtstaerken steht: Wandfackeln sind Fackeln.
pub fn light_source_name(name: &str) -> &str {
    match name {
        "wall_torch" => "torch",
        "soul_wall_torch" => "soul_torch",
        "redstone_wall_torch" => "redstone_torch",
        "copper_wall_torch" => "copper_torch",
        n => n,
    }
}

/// Alle Bloecke, von denen mindestens ein Zustand leuchtet, alphabetisch (Wandvarianten
/// unter ihrem Grundblock, s. light_source_name).
pub fn light_source_blocks() -> Vec<&'static str> {
    let mut namen: Vec<&'static str> = (0..crate::block_registry::state_count() as u32)
        .filter(|&s| luminance(s) > 0)
        .filter_map(crate::block_registry::block_name)
        .map(light_source_name)
        .collect();
    namen.sort_unstable();
    namen.dedup();
    namen
}

// Leuchtstaerke mit Faktor, ungedeckelt.
fn scaled_luminance(state: u32) -> Option<(f32, Option<&'static str>)> {
    let l = luminance(state);
    if l == 0 {
        return None;
    }
    let name = crate::block_registry::block_name(state);
    Some((l as f32 * name.map_or(1.0, light_factor), name))
}

// Eigenes Licht eines Blocks als gepackte Kanaele (je 4 Bit: R | G << 4 | B << 8), hoechstens
// Stufe 15 (s. emission_level).
pub fn emission(state: u32) -> u16 {
    let Some((l, name)) = scaled_luminance(state) else { return 0 };
    let farbe = name.map_or([1.0; 3], light_color);
    let l = l.min(15.0);
    let k = |f: f32| ((l * f).round().max(0.0) as u16).min(15);
    k(farbe[0]) | k(farbe[1]) << 4 | k(farbe[2]) << 8
}

/// Echte Leuchtstaerke mit Faktor, auch ueber 15 (Reichweite des Fernlichts in Bloecken).
pub fn emission_level(state: u32) -> u8 {
    scaled_luminance(state).map_or(0, |(l, _)| l.round().clamp(0.0, 255.0) as u8)
}

fn channel(v: u16, c: u32) -> u16 {
    v >> (c * 4) & 15
}

// Hellster Kanal eines gepackten Blocklichts (0..15).
pub fn brightest(v: u16) -> u8 {
    channel(v, 0).max(channel(v, 1)).max(channel(v, 2)) as u8
}

// Kanalweise um `a` vermindert (nicht unter 0).
fn dim(v: u16, a: u16) -> u16 {
    let k = |c| channel(v, c).saturating_sub(a);
    k(0) | k(1) << 4 | k(2) << 8
}

fn max_channels(a: u16, b: u16) -> u16 {
    let k = |c| channel(a, c).max(channel(b, c));
    k(0) | k(1) << 4 | k(2) << 8
}

// Liegt ein Kanal von `a` ueber `b`?
fn any_brighter(a: u16, b: u16) -> bool {
    (0..3).any(|c| channel(a, c) > channel(b, c))
}

// --- Himmelslicht vom Server ------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum SkyLayer {
    Uniform(u8),
    // Zwei Werte je Byte, Index = (y * 16 + z) * 16 + x, niedriges Halbbyte zuerst.
    Data(Arc<[u8]>),
}

impl SkyLayer {
    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        match self {
            SkyLayer::Uniform(v) => *v,
            SkyLayer::Data(d) => {
                let i = ((y * 16 + z) * 16 + x) as usize;
                (d[i / 2] >> ((i & 1) * 4)) & 15
            }
        }
    }
}

// Die Ebenen einer neuen Spalte, bevor der Server etwas geschickt hat: hell (Ebene 0 liegt
// unter der Welt, die letzte darueber).
pub fn default_sky(sections: usize) -> Vec<SkyLayer> {
    vec![SkyLayer::Uniform(15); sections + 2]
}

// Lichtdaten eines Pakets in die Ebenen uebernehmen. Was das Paket nicht nennt, bleibt.
pub fn apply_sky(layers: &mut [SkyLayer], light: &protocol::chunk::LightData) {
    for &e in &light.sky_empty {
        if let Some(l) = layers.get_mut(e) {
            *l = SkyLayer::Uniform(0);
        }
    }
    for (e, data) in &light.sky {
        let Some(l) = layers.get_mut(*e) else { continue };
        // Gleichfoermige Felder (ganz hell, ganz dunkel) brauchen keine 2 KB.
        *l = match data.first() {
            Some(&b) if b & 15 == b >> 4 && data.iter().all(|&x| x == b) => SkyLayer::Uniform(b & 15),
            _ => SkyLayer::Data(Arc::from(data.as_slice())),
        };
    }
}

impl ChunkColumn {
    // Himmelslicht an spaltenlokalem x/z (0..16) und absoluter Hoehe y.
    pub fn sky_at(&self, x: i32, y: i32, z: i32) -> u8 {
        let ebene = (y - self.min_y).div_euclid(SECTION_SIZE) + 1;
        match self.sky.get(ebene.max(0) as usize) {
            Some(l) if ebene >= 0 => l.get(x, (y - self.min_y).rem_euclid(SECTION_SIZE), z),
            _ => 15,
        }
    }
}

// --- Lichtfeld fuer das Meshen ----------------------------------------------------------

// Licht um eine Spalte herum: x/z von -1 bis 16 (ein Block in die Nachbarn), y ueber die ganze
// Hoehe. Blocklicht kanalweise gepackt (s. emission), Himmelslicht 0..15.
pub struct LightField {
    min_y: i32,
    height: i32,
    block: Option<Vec<u16>>,
    sky: Vec<u8>,
}

const FIELD: i32 = 18;

impl LightField {
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (x, y, z) = (x + 1, y - self.min_y, z + 1);
        ((0..FIELD).contains(&x) && (0..self.height).contains(&y) && (0..FIELD).contains(&z)).then(|| ((y * FIELD + z) * FIELD + x) as usize)
    }

    // (Blocklicht gepackt, Himmelslicht) an spaltenlokaler Position; ausserhalb: dunkel/hell.
    pub fn at(&self, x: i32, y: i32, z: i32) -> (u16, u8) {
        match self.index(x, y, z) {
            Some(i) => (self.block.as_ref().map_or(0, |b| b[i]), self.sky[i]),
            None if y >= self.min_y + self.height => (0, 15),
            None => (0, 0),
        }
    }
}

// Blocklicht einer Spalte zum Nachschlagen (Entities, Hand, Blockentities im Shader-Pfad):
// der Teil des Lichtfelds ueber der Zielspalte, je Section nur, wenn dort ueberhaupt etwas
// leuchtet. Entsteht im Mesh-Worker mit jedem Neumeshen und ist damit nie aelter als das Mesh.
pub struct ColumnBlockLight {
    min_y: i32,
    sections: Vec<Option<Box<[u16]>>>,
}

impl ColumnBlockLight {
    // Blocklicht je Kanal (0..15) an spaltenlokalem x/z (0..16) und absoluter Hoehe y.
    pub fn at(&self, x: i32, y: i32, z: i32) -> [u8; 3] {
        let i = (y - self.min_y).div_euclid(SECTION_SIZE);
        let Some(Some(s)) = (i >= 0).then(|| self.sections.get(i as usize)).flatten() else { return [0; 3] };
        let v = s[((((y - self.min_y).rem_euclid(SECTION_SIZE)) * 16 + z) * 16 + x) as usize];
        [channel(v, 0) as u8, channel(v, 1) as u8, channel(v, 2) as u8]
    }
}

impl LightField {
    // None, wenn in der Zielspalte nichts leuchtet.
    pub fn column_block_light(&self) -> Option<ColumnBlockLight> {
        let block = self.block.as_ref()?;
        let sections = (0..self.height / SECTION_SIZE)
            .map(|si| {
                let mut werte = vec![0u16; 4096];
                let mut irgendwas = false;
                for ly in 0..SECTION_SIZE {
                    let y = si * SECTION_SIZE + ly;
                    for z in 0..16 {
                        for x in 0..16 {
                            let v = block[((y * FIELD + z + 1) * FIELD + x + 1) as usize];
                            werte[((ly * 16 + z) * 16 + x) as usize] = v;
                            irgendwas |= v != 0;
                        }
                    }
                }
                irgendwas.then(|| werte.into_boxed_slice())
            })
            .collect::<Vec<_>>();
        sections.iter().any(Option::is_some).then_some(ColumnBlockLight { min_y: self.min_y, sections })
    }
}

// Die neun Spalten als Gitter, Spaltenindex -1..=1 je Achse.
fn column_of(n: &ColumnNeighbors, cx: i32, cz: i32) -> Option<&ChunkColumn> {
    let c = match (cx, cz) {
        (0, 0) => Some(&n.target),
        (0, -1) => n.north.as_ref(),
        (0, 1) => n.south.as_ref(),
        (1, 0) => n.east.as_ref(),
        (-1, 0) => n.west.as_ref(),
        (1, -1) => n.northeast.as_ref(),
        (-1, -1) => n.northwest.as_ref(),
        (1, 1) => n.southeast.as_ref(),
        (-1, 1) => n.southwest.as_ref(),
        _ => None,
    };
    c.map(Arc::as_ref)
}

// Block-State an spaltenlokaler Position (x/z -16..32), ausserhalb geladener Spalten Luft.
fn state_at(n: &ColumnNeighbors, x: i32, y: i32, z: i32) -> u32 {
    let (cx, cz) = (x.div_euclid(16), z.div_euclid(16));
    column_of(n, cx, cz).map_or(crate::chunk::STATE_AIR, |c| c.get(x.rem_euclid(16), y, z.rem_euclid(16)))
}

pub fn light_field(n: &ColumnNeighbors) -> LightField {
    let min_y = n.target.min_y;
    let height = n.target.sections.len() as i32 * SECTION_SIZE;
    let zellen = (FIELD * FIELD * height) as usize;

    let mut sky = vec![0u8; zellen];
    for y in 0..height {
        for z in -1..FIELD - 1 {
            for x in -1..FIELD - 1 {
                let wert = column_of(n, x.div_euclid(16), z.div_euclid(16)).map_or(15, |c| c.sky_at(x.rem_euclid(16), min_y + y, z.rem_euclid(16)));
                sky[((y * FIELD + z + 1) * FIELD + x + 1) as usize] = wert;
            }
        }
    }

    LightField { min_y, height, block: block_light(n, min_y, height), sky }
}

// Blocklicht ueber die 3x3 Spalten ausbreiten und den Bereich der Zielspalte (+1) behalten.
// None, wenn in Reichweite gar nichts leuchtet -- der Normalfall ueber Tage.
fn block_light(n: &ColumnNeighbors, min_y: i32, height: i32) -> Option<Vec<u16>> {
    const W: i32 = 48;
    let idx = |x: i32, y: i32, z: i32| ((y * W + (z + 16)) * W + (x + 16)) as usize;
    let mut quellen = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let Some(c) = column_of(n, cx, cz) else { continue };
            for (si, s) in c.sections.iter().enumerate() {
                for (i, &st) in s.states().iter().enumerate() {
                    if luminance(st) == 0 {
                        continue;
                    }
                    let (x, y, z) = ((i % 16) as i32, (i / 256) as i32, ((i / 16) % 16) as i32);
                    let (wx, wz) = (cx * 16 + x, cz * 16 + z);
                    // Quellen, deren Licht die Zielspalte (+1) nicht erreichen kann, fallen weg.
                    if !(-15..=31).contains(&wx) || !(-15..=31).contains(&wz) {
                        continue;
                    }
                    quellen.push((wx, si as i32 * 16 + y, wz, emission(st)));
                }
            }
        }
    }
    if quellen.is_empty() {
        return None;
    }

    let mut licht = vec![0u16; (W * W * height) as usize];
    let mut schlange = std::collections::VecDeque::new();
    for &(x, y, z, e) in &quellen {
        let i = idx(x, y, z);
        licht[i] = max_channels(licht[i], e);
        schlange.push_back((x, y, z));
    }
    while let Some((x, y, z)) = schlange.pop_front() {
        let v = licht[idx(x, y, z)];
        if v & 0xEEE == 0 {
            continue; // nur noch Stufe 1 oder 0: reicht nicht weiter
        }
        let hier = state_at(n, x, min_y + y, z);
        for (d, (dx, dy, dz)) in DIRS.iter().enumerate() {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            if !(-16..32).contains(&nx) || !(0..height).contains(&ny) || !(-16..32).contains(&nz) {
                continue;
            }
            let dort = state_at(n, nx, min_y + ny, nz);
            if face_blocks(hier, d) || face_blocks(dort, opposite(d)) {
                continue;
            }
            let neu = dim(v, opacity(dort).max(1) as u16);
            let j = idx(nx, ny, nz);
            if any_brighter(neu, licht[j]) {
                licht[j] = max_channels(licht[j], neu);
                schlange.push_back((nx, ny, nz));
            }
        }
    }

    let mut out = vec![0u16; (FIELD * FIELD * height) as usize];
    for y in 0..height {
        for z in -1..FIELD - 1 {
            for x in -1..FIELD - 1 {
                out[((y * FIELD + z + 1) * FIELD + x + 1) as usize] = licht[idx(x, y, z)];
            }
        }
    }
    Some(out)
}

// --- Licht je Vertex ----------------------------------------------------------------------

// Weiches Licht an einer Ecke, wie Vanillas Smooth Lighting: Mittel der (bis zu) vier Bloecke
// vor der Flaeche, die an diese Ecke grenzen; undurchsichtige zaehlen nicht mit. Der Block
// schraeg gegenueber dem eigenen (dem vor der Flaeche, zu ihrer Mitte `mitte` hin) zaehlt nur,
// wenn wenigstens einer der beiden dazwischen durchsichtig ist -- sonst ist er von hier aus gar
// nicht zu erreichen, und eine Ecke im geschlossenen Raum holte Licht von draussen durch die
// Kante zweier Waende. `p` ist spaltenlokal (y absolut), `normal` zeigt aus der Flaeche heraus
// (Laenge egal). Rueckgabe: Blocklicht je Kanal und Himmelslicht, in Stufen (0..15, mit
// Bruchteil).
pub fn vertex_light(f: &LightField, n: &ColumnNeighbors, p: [f32; 3], normal: [f32; 3], mitte: [f32; 3]) -> ([f32; 3], f32) {
    let achse = (0..3).max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs())).unwrap_or(1);
    let s = [p[0] + normal[0].signum() * 0.5 * (achse == 0) as i32 as f32, p[1] + normal[1].signum() * 0.5 * (achse == 1) as i32 as f32, p[2] + normal[2].signum() * 0.5 * (achse == 2) as i32 as f32];
    let (t1, t2) = match achse {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    // Richtung zum eigenen Block je Flaechenachse.
    let zu = |t: usize| if mitte[t] >= p[t] { 0.5 } else { -0.5 };
    let zelle = |a: f32, b: f32| {
        let mut q = s;
        q[t1] += a;
        q[t2] += b;
        [q[0].floor() as i32, q[1].floor() as i32, q[2].floor() as i32]
    };
    let (a, b) = (zu(t1), zu(t2));
    let eigen = zelle(a, b);
    let seiten = [zelle(-a, b), zelle(a, -b)];
    let diagonal = zelle(-a, -b);
    let offen = |c: [i32; 3]| opacity(state_at(n, c[0], c[1], c[2])) < 15;
    let mut summe = ([0.0f32; 3], 0.0f32);
    let mut zahl = 0.0;
    let mut mit = |c: [i32; 3]| {
        let (b, h) = f.at(c[0], c[1], c[2]);
        for k in 0..3 {
            summe.0[k] += channel(b, k as u32) as f32;
        }
        summe.1 += h as f32;
        zahl += 1.0;
    };
    for c in [eigen, seiten[0], seiten[1]] {
        if offen(c) {
            mit(c);
        }
    }
    if offen(diagonal) && (offen(seiten[0]) || offen(seiten[1])) {
        mit(diagonal);
    }
    if zahl == 0.0 {
        // Ecke ganz von festen Bloecken umgeben: der Block direkt vor der Flaeche.
        let (b, h) = f.at(eigen[0], eigen[1], eigen[2]);
        return ([channel(b, 0) as f32, channel(b, 1) as f32, channel(b, 2) as f32], h as f32);
    }
    (summe.0.map(|v| v / zahl), summe.1 / zahl)
}

// Licht je Ecke fuer terrain_vertex::pack_terrain_lit. Beidseitige (schraege) Flaechen nehmen
// je Kanal das hellere ihrer beiden Seiten.
pub fn packed_vertex_light(f: &LightField, n: &ColumnNeighbors, p: [f32; 3], normal: [f32; 3], mitte: [f32; 3], beidseitig: bool) -> u32 {
    let (mut b, mut h) = vertex_light(f, n, p, normal, mitte);
    if beidseitig {
        let (b2, h2) = vertex_light(f, n, p, normal.map(|v| -v), mitte);
        for k in 0..3 {
            b[k] = b[k].max(b2[k]);
        }
        h = h.max(h2);
    }
    pack_vertex_light(b, h)
}

// Gepackt fuer den Vertex (s. TerrainVertex::LIGHT_*): Himmel 4 Bit, Block je Kanal 5 Bit in
// halben Stufen.
pub fn pack_vertex_light(block: [f32; 3], sky: f32) -> u32 {
    let b = |v: f32| ((v * 2.0).round() as u32).min(31);
    (sky.round() as u32).min(15) | b(block[0]) << 4 | b(block[1]) << 9 | b(block[2]) << 14
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::ChunkSection;

    // Erster State eines Blocks (Fackeln usw. haben ohnehin nur einen bzw. leuchten in jedem).
    fn state_of(block: &str) -> u32 {
        (0..crate::block_registry::state_count() as u32).find(|&s| crate::block_registry::block_name(s) == Some(block)).expect(block)
    }

    #[test]
    fn lichtstaerke_faktor_wirkt_auf_die_emission() {
        // Ein Block, den kein anderer Test hier benutzt: der Faktor ist global, Tests laufen
        // parallel.
        let laterne = state_of("lantern");
        let vorher = emission(laterne);
        set_light_factors(vec![("lantern".to_string(), 0.5)]);
        let halb = emission(laterne);
        set_light_factors(vec![("lantern".to_string(), 2.5)]);
        let stark = (emission(laterne), emission_level(laterne));
        set_light_factors(Vec::new());
        // Laterne: Stufe 15, roter Kanal voll -> 7,5 -> 8.
        assert_eq!(vorher & 15, 15);
        assert_eq!(halb & 15, 8);
        // Ueber 15 hinaus: Ausbreitung bleibt bei 15, der Rest wird Fernlicht.
        assert_eq!(stark.0, vorher);
        assert_eq!(stark.1, 38);
        assert_eq!(emission_level(laterne), 15);
        assert!(!light_source_blocks().contains(&"wall_torch"));
        assert_eq!(light_source_name("soul_wall_torch"), "soul_torch");
        assert!(light_source_blocks().contains(&"torch"));
        assert!(!light_source_blocks().contains(&"stone"));
    }

    #[test]
    fn vanilla_werte_aus_dem_jar() {
        assert_eq!(DATA.len(), 4 + crate::block_registry::state_count() * 3, "light_data.bin passt zur Block-Registry");
        assert_eq!(luminance(state_of("torch")), 14);
        assert_eq!(luminance(state_of("glowstone")), 15);
        assert_eq!(luminance(state_of("soul_torch")), 10);
        assert_eq!(luminance(state_of("stone")), 0);
        assert_eq!(opacity(state_of("stone")), 15);
        assert_eq!(opacity(state_of("air")), 0);
        // Seelenfackel: blau ueberwiegt.
        let e = emission(state_of("soul_torch"));
        assert!(channel(e, 2) > channel(e, 0));
        let f = emission(state_of("torch"));
        assert_eq!(channel(f, 0), 14);
        assert!(channel(f, 2) < channel(f, 0));
    }

    fn spalte(states: impl Fn(i32, i32, i32) -> u32) -> Arc<ChunkColumn> {
        let mut v = vec![0u32; 4096];
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    v[((y * 16 + z) * 16 + x) as usize] = states(x, y, z);
                }
            }
        }
        Arc::new(ChunkColumn { sections: vec![ChunkSection::from_states(v)], min_y: 0, sky: default_sky(1) })
    }

    fn nachbarn(target: Arc<ChunkColumn>, rest: Option<Arc<ChunkColumn>>) -> ColumnNeighbors {
        ColumnNeighbors {
            target,
            north: rest.clone(),
            south: rest.clone(),
            east: rest.clone(),
            west: rest.clone(),
            northeast: rest.clone(),
            northwest: rest.clone(),
            southeast: rest.clone(),
            southwest: rest,
        }
    }

    #[test]
    fn fackel_breitet_sich_aus_und_stein_haelt_auf() {
        let fackel = state_of("torch");
        let stein = state_of("stone");
        // Fackel bei (8,8,8), eine Steinwand bei x = 10.
        let c = spalte(|x, y, z| if (x, y, z) == (8, 8, 8) { fackel } else if x == 10 { stein } else { 0 });
        let n = nachbarn(c, None);
        let f = light_field(&n);
        assert_eq!(channel(f.at(8, 8, 8).0, 0), 14);
        assert_eq!(channel(f.at(7, 8, 8).0, 0), 13);
        assert_eq!(channel(f.at(8, 8, 3).0, 0), 9);
        // Hinter der Wand: nur ueber Umwege (die Wand ist ueberall), also dunkel.
        assert_eq!(f.at(11, 8, 8).0, 0);
        // Licht reicht in die Nachbarspalte hinein (x = -1 liegt in der westlichen).
        assert_eq!(channel(f.at(-1, 8, 8).0, 0), 5);
    }

    #[test]
    fn spaltenlicht_zum_nachschlagen() {
        let fackel = state_of("torch");
        let c = spalte(|x, y, z| if (x, y, z) == (8, 8, 8) { fackel } else { 0 });
        let n = nachbarn(c, None);
        let l = light_field(&n).column_block_light().expect("eine Fackel leuchtet");
        assert_eq!(l.at(8, 8, 8)[0], 14);
        assert_eq!(l.at(7, 8, 8)[0], 13);
        assert_eq!(l.at(8, 40, 8), [0; 3], "ausserhalb der Spalte dunkel");
        let dunkel = nachbarn(spalte(|_, _, _| 0), None);
        assert!(light_field(&dunkel).column_block_light().is_none());
    }

    #[test]
    fn licht_aus_der_nachbarspalte_kommt_an() {
        let fackel = state_of("torch");
        let leer = spalte(|_, _, _| 0);
        let mit = spalte(|x, y, z| if (x, y, z) == (15, 8, 8) { fackel } else { 0 });
        let n = nachbarn(leer, Some(mit));
        let f = light_field(&n);
        // Westlicher Nachbar hat die Fackel bei x = 15, also lokal x = -1.
        assert_eq!(channel(f.at(0, 8, 8).0, 0), 13);
    }

    #[test]
    fn ecke_holt_kein_licht_durch_eine_kante() {
        let fackel = state_of("torch");
        let stein = state_of("stone");
        // Oberseite des Blocks (8,7,8), Ecke (9,8,9): die beiden Nachbarn vor der Flaeche sind
        // Stein, schraeg gegenueber steht die Fackel -- nur ueber die Kante zu erreichen.
        let c = spalte(|x, y, z| match (x, y, z) {
            (9, 8, 8) | (8, 8, 9) => stein,
            (9, 8, 9) => fackel,
            _ => 0,
        });
        let n = nachbarn(c, None);
        let f = light_field(&n);
        let (b, _) = vertex_light(&f, &n, [9.0, 8.0, 9.0], [0.0, 1.0, 0.0], [8.5, 8.0, 8.5]);
        assert_eq!(b[0], channel(f.at(8, 8, 8).0, 0) as f32, "{b:?}");
    }

    #[test]
    fn ecke_mittelt_ueber_vier_bloecke() {
        let fackel = state_of("torch");
        let c = spalte(|x, y, z| if (x, y, z) == (8, 8, 8) { fackel } else { 0 });
        let n = nachbarn(c, None);
        let f = light_field(&n);
        // Oberseite eines gedachten Blocks bei (8,7,8): Ecke (8,8,8), Normale nach oben.
        let (b, _) = vertex_light(&f, &n, [8.0, 8.0, 8.0], [0.0, 1.0, 0.0], [8.5, 8.0, 8.5]);
        // Zellen (7..8, 8, 7..8): Fackel 14, zwei Nachbarn 13, die Diagonale 12.
        assert!((b[0] - 13.0).abs() < 1e-4, "{b:?}");
        let p = pack_vertex_light(b, 15.0);
        assert_eq!(p & 15, 15);
        assert_eq!(p >> 4 & 31, 26);
    }
}
