// Launcher-Hintergrundbild: eine kleine Szene aus einem Einzelspieler-Spielstand (Bloecke,
// Biome, Himmelslicht, Kamera; s. scripts/hero_scene.py), die der Client beim ersten Start mit
// den Texturen der eigenen Installation rendert und im Storage-Ordner (neben den extrahierten
// Spieldaten, s. assets::game_data_dir) als launcher-hero.png ablegt. Im Repo liegt so kein Bild aus dem Spiel, nur die Szene; der Launcher zeigt die Datei,
// sobald es sie gibt.
//
// Ablauf (s. App::tick_hero_scene): Szene in die leere Welt laden, solange keine Verbindung
// besteht, warten, bis jede Spalte gemesht und hochgeladen ist, einmal ohne HUD zeichnen,
// speichern, Welt wieder leeren.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use glam::DVec3;

const SCENE: &[u8] = include_bytes!("../../../assets/scene/launcher-hero.scene");
pub const FILE: &str = "launcher-hero.png";
const SECTION_VOLUME: usize = 16 * 16 * 16;

pub struct Scene {
    pub pos: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    // Sichtfeld beim Aufnehmen, in Grad (wie die FOV-Einstellung).
    pub fov: f32,
    pub time_of_day: i64,
    // Bis wohin gezeichnet wird, in Chunks -- eins weniger als ausgeschnitten, damit jeder
    // gezeichnete Chunk Nachbarn hat (sonst Wasserwaende am Rand).
    pub render_distance: u32,
    pub biomes: Vec<protocol::packets::BiomeInfo>,
    // Je Spalte: Chunk-Koordinaten und die Sections ab der Welt-Untergrenze, samt Himmelslicht.
    pub columns: Vec<(i32, i32, Vec<world::ChunkSection>, protocol::chunk::LightData)>,
}

pub fn path() -> Option<PathBuf> {
    Some(assets::data_dir()?.join("storage").join(FILE))
}

// Stand von Szene und Renderweg. Steht neben dem Bild ein anderer, wird neu gerendert -- sonst
// bliebe nach einer Korrektur am Rendern das alte, falsche Bild fuer immer liegen.
pub const VERSION: &str = "4";

pub fn version_path() -> Option<PathBuf> {
    Some(assets::data_dir()?.join("storage").join("launcher-hero.version"))
}

pub fn is_current() -> bool {
    path().is_some_and(|p| p.is_file()) && version_path().and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|v| v.trim() == VERSION)
}

struct Reader<'a> {
    data: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.data.get(self.p..self.p + n)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn text(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

// "name[k=v,...]" (Schluessel sortiert) je State-ID, um die Namen der Szene aufzuloesen.
fn state_lookup() -> HashMap<String, u32> {
    (0..world::state_count() as u32)
        .filter_map(|s| {
            let name = world::block_registry::block_name(s)?;
            let mut props: Vec<(&str, &str)> = world::state_props::properties_of(s).collect();
            props.sort_unstable();
            let key = if props.is_empty() {
                name.to_string()
            } else {
                format!("{name}[{}]", props.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(","))
            };
            Some((key, s))
        })
        .collect()
}

pub fn load(min_y: i32, sections: usize) -> Option<Scene> {
    let mut roh = Vec::new();
    flate2::read::ZlibDecoder::new(SCENE).read_to_end(&mut roh).ok()?;
    let mut r = Reader { data: &roh, p: 0 };
    if r.take(4)? != b"CRSC" || r.u32()? != 3 {
        return None;
    }
    let pos = DVec3::new(r.f64()?, r.f64()?, r.f64()?);
    let (yaw, pitch, fov, time_of_day, render_distance) = (r.f32()?, r.f32()?, r.f32()?, r.i64()?, r.u8()? as u32);
    let lookup = state_lookup();
    // Unbekannte Zustaende (andere Version) werden Luft statt Fehltextur.
    let zustaende: Vec<u32> = (0..r.u32()?).map(|_| r.text().map(|t| lookup.get(&t).copied().unwrap_or(0))).collect::<Option<_>>()?;
    let mut biomes = Vec::new();
    for _ in 0..r.u32()? {
        let name = format!("minecraft:{}", r.text()?);
        let (temperature, downfall, water_color, regen) = (r.f32()?, r.f32()?, r.u32()?, r.u8()?);
        biomes.push(protocol::packets::BiomeInfo { name, temperature, downfall, water_color, has_precipitation: regen != 0 });
    }
    let mut columns = Vec::new();
    for _ in 0..r.u32()? {
        let (cx, cz, n) = (r.i32()?, r.i32()?, r.u8()?);
        let mut secs: Vec<world::ChunkSection> = (0..sections).map(|_| world::ChunkSection::from_states(vec![0; SECTION_VOLUME])).collect();
        let mut licht = protocol::chunk::LightData { sky: Vec::new(), sky_empty: Vec::new() };
        for _ in 0..n {
            let sy = r.u8()? as i8 as i32;
            let states: Vec<u32> = (0..SECTION_VOLUME).map(|_| r.u16().map(|i| zustaende.get(i as usize).copied().unwrap_or(0))).collect::<Option<_>>()?;
            let biome: Vec<u32> = r.take(64)?.iter().map(|&b| b as u32).collect();
            let himmel = (r.u8()? != 0).then(|| r.take(2048).map(<[u8]>::to_vec)).flatten();
            let index = sy - min_y.div_euclid(16);
            if !(0..sections as i32).contains(&index) {
                continue;
            }
            secs[index as usize] = world::ChunkSection::from_states_and_biomes(states, biome);
            // Licht-Ebenen zaehlen ab einer Section UNTER der Welt (wie im Licht-Paket).
            if let Some(h) = himmel {
                licht.sky.push((index as usize + 1, h));
            }
        }
        columns.push((cx, cz, secs, licht));
    }
    Some(Scene { pos, yaw, pitch, fov, time_of_day, render_distance, biomes, columns })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn szene_laesst_sich_laden() {
        let s = load(-64, 24).expect("Szene nicht lesbar");
        assert!(!s.columns.is_empty());
        assert!(!s.biomes.is_empty());
        // Die Kamera steht im Riff, also unter Wasser: dort muss es Bloecke geben, die
        // aufgeloest wurden (nicht alles Luft).
        let bloecke = s.columns.iter().flat_map(|(_, _, secs, _)| secs.iter()).filter(|sec| !sec.is_empty()).count();
        assert!(bloecke > 0);
    }

    // Jeder Blockzustand der Szene muss sich in dieser Version aufloesen lassen -- sonst stuende
    // an seiner Stelle still Luft im Bild.
    #[test]
    fn alle_zustaende_bekannt() {
        let mut roh = Vec::new();
        flate2::read::ZlibDecoder::new(SCENE).read_to_end(&mut roh).unwrap();
        let mut r = Reader { data: &roh, p: 0 };
        r.take(8 + 24 + 12 + 8 + 1).unwrap();
        let lookup = state_lookup();
        let fehlend: Vec<String> = (0..r.u32().unwrap()).map(|_| r.text().unwrap()).filter(|t| !lookup.contains_key(t)).collect();
        assert!(fehlend.is_empty(), "unbekannt: {fehlend:?}");
    }
}
