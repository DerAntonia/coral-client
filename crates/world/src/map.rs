// Karten (gefuellte Karten, map_id): was der Server per map_item_data schickt -- 128x128 Farb-IDs,
// Massstab, Sperre und Markierungen -- und wie daraus das Bild wird (MapColor-Palette).
//
// Eine Farb-ID ist Grundfarbe * 4 + Helligkeit (0 dunkel 180, 1 normal 220, 2 hell 255,
// 3 dunkelst 135, jeweils /255 auf die Grundfarbe); Grundfarbe 0 ist durchsichtig.

use std::collections::HashMap;

use protocol::packets::{MapDecoration, MapItemData};

pub const MAP_SIZE: usize = 128;

// MapColor: die 62 Grundfarben (0xRRGGBB), Index = ID. Werte aus dem Spiel ausgelesen.
const BASE_COLORS: [u32; 62] = [
    0x000000, 0x7FB238, 0xF7E9A3, 0xC7C7C7, 0xFF0000, 0xA0A0FF, 0xA7A7A7, 0x007C00, 0xFFFFFF, 0xA4A8B8, 0x976D4D, 0x707070, 0x4040FF,
    0x8F7748, 0xFFFCF5, 0xD87F33, 0xB24CD8, 0x6699D8, 0xE5E533, 0x7FCC19, 0xF27FA5, 0x4C4C4C, 0x999999, 0x4C7F99, 0x7F3FB2, 0x334CB2,
    0x664C33, 0x667F33, 0x993333, 0x191919, 0xFAEE4D, 0x5CDBD5, 0x4A80FF, 0x00D93A, 0x815631, 0x700200, 0xD1B1A1, 0x9F5224, 0x95576C,
    0x706C8A, 0xBA8524, 0x677535, 0xA04D4E, 0x392923, 0x876B62, 0x575C5C, 0x7A4958, 0x4C3E5C, 0x4C3223, 0x4C522A, 0x8E3C2E, 0x251610,
    0xBD3031, 0x943F61, 0x5C191D, 0x167E86, 0x3A8E8C, 0x562C3E, 0x14B485, 0x646464, 0xD8AF93, 0x7FA796,
];

const BRIGHTNESS: [u32; 4] = [180, 220, 255, 135];

// RGBA einer Farb-ID (MapColor.getColorFromPackedId), durchsichtig fuer Grundfarbe 0 oder
// unbekannte IDs.
pub fn color_rgba(id: u8) -> [u8; 4] {
    let basis = (id >> 2) as usize;
    if basis == 0 || basis >= BASE_COLORS.len() {
        return [0, 0, 0, 0];
    }
    let c = BASE_COLORS[basis];
    let m = BRIGHTNESS[(id & 3) as usize];
    let kanal = |s: u32| (((c >> s) & 0xFF) * m / 255) as u8;
    [kanal(16), kanal(8), kanal(0), 255]
}

#[derive(Clone, Debug)]
pub struct MapState {
    pub colors: Vec<u8>,
    pub scale: u8,
    pub locked: bool,
    pub decorations: Vec<MapDecoration>,
    // Erhoeht sich bei jeder Aenderung -- wer ein Bild daraus hat, erkennt so, dass es veraltet ist.
    pub revision: u32,
}

impl Default for MapState {
    fn default() -> Self {
        MapState { colors: vec![0; MAP_SIZE * MAP_SIZE], scale: 0, locked: false, decorations: Vec::new(), revision: 0 }
    }
}

#[derive(Default)]
pub struct Maps {
    maps: HashMap<i32, MapState>,
}

impl Maps {
    // map_item_data anwenden: Kopfwerte, Markierungen (wenn mitgeschickt) und das Rechteck.
    pub fn apply(&mut self, data: MapItemData) {
        let m = self.maps.entry(data.id).or_default();
        m.scale = data.scale;
        m.locked = data.locked;
        if let Some(d) = data.decorations {
            m.decorations = d;
        }
        if let Some(p) = data.patch {
            for zeile in 0..p.height as usize {
                for spalte in 0..p.width as usize {
                    let (x, y) = (p.x as usize + spalte, p.y as usize + zeile);
                    if x < MAP_SIZE && y < MAP_SIZE {
                        m.colors[y * MAP_SIZE + x] = p.colors[zeile * p.width as usize + spalte];
                    }
                }
            }
        }
        m.revision = m.revision.wrapping_add(1);
    }

    pub fn get(&self, id: i32) -> Option<&MapState> {
        self.maps.get(&id)
    }

    // Beim Welt- bzw. Serverwechsel.
    pub fn clear(&mut self) {
        self.maps.clear();
    }
}

// Datei-Name der Markierung (textures/map/decorations/<name>.png) nach Registry-Index, und ob
// sie auch im Rahmen gezeigt wird (MapDecorationType.showOnItemFrame).
pub fn decoration_sprite(kind: i32) -> Option<(&'static str, bool)> {
    const BANNER: [&str; 16] = [
        "white_banner", "orange_banner", "magenta_banner", "light_blue_banner", "yellow_banner", "lime_banner", "pink_banner", "gray_banner",
        "light_gray_banner", "cyan_banner", "purple_banner", "blue_banner", "brown_banner", "green_banner", "red_banner", "black_banner",
    ];
    Some(match kind {
        0 => ("player", false),
        1 => ("frame", true),
        2 => ("red_marker", true),
        3 => ("blue_marker", true),
        4 => ("target_x", true),
        5 => ("target_point", true),
        6 => ("player_off_map", false),
        7 => ("player_off_limits", false),
        8 => ("woodland_mansion", true),
        9 => ("ocean_monument", true),
        10..=25 => (BANNER[(kind - 10) as usize], true),
        26 => ("red_x", true),
        27 => ("desert_village", true),
        28 => ("plains_village", true),
        29 => ("savanna_village", true),
        30 => ("snowy_village", true),
        31 => ("taiga_village", true),
        32 => ("jungle_temple", true),
        33 => ("swamp_hut", true),
        34 => ("trial_chambers", true),
        _ => return None,
    })
}

// Das Kartenbild als RGBA (128x128), mit den Markierungen darauf. `sprite` liefert das Bild einer
// Markierung (RGBA, quadratisch, meist 8x8). Wie MapRenderer: Mitte bei (x/2 + 64, y/2 + 64),
// 8 Kartenpixel gross, um rot * 22,5 Grad gedreht. `im_rahmen` laesst die Spielermarken weg.
pub fn render(state: &MapState, im_rahmen: bool, sprite: &mut dyn FnMut(&str) -> Option<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(MAP_SIZE * MAP_SIZE * 4);
    for id in &state.colors {
        rgba.extend(color_rgba(*id));
    }
    for d in &state.decorations {
        let Some((name, im_rahmen_sichtbar)) = decoration_sprite(d.kind) else { continue };
        if im_rahmen && !im_rahmen_sichtbar {
            continue;
        }
        let Some((n, bild)) = sprite(name) else { continue };
        let (cx, cy) = (d.x as f32 / 2.0 + 64.0, d.y as f32 / 2.0 + 64.0);
        let winkel = d.rot as f32 * std::f32::consts::TAU / 16.0;
        let (sin, cos) = winkel.sin_cos();
        // Je Kartenpixel im Umkreis: zurueck in die Markierung drehen und naechstes Texel nehmen.
        for py in (cy - 6.0).floor() as i32..=(cy + 6.0).ceil() as i32 {
            for px in (cx - 6.0).floor() as i32..=(cx + 6.0).ceil() as i32 {
                if px < 0 || py < 0 || px >= MAP_SIZE as i32 || py >= MAP_SIZE as i32 {
                    continue;
                }
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                let (tu, tv) = ((u + 4.0) / 8.0, (v + 4.0) / 8.0);
                if !(0.0..1.0).contains(&tu) || !(0.0..1.0).contains(&tv) {
                    continue;
                }
                let (sx, sy) = ((tu * n as f32) as usize, (tv * n as f32) as usize);
                let quelle = &bild[(sy * n as usize + sx) * 4..][..4];
                if quelle[3] < 128 {
                    continue;
                }
                let ziel = (py as usize * MAP_SIZE + px as usize) * 4;
                rgba[ziel..ziel + 4].copy_from_slice(&[quelle[0], quelle[1], quelle[2], 255]);
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::packets::MapPatch;

    #[test]
    fn farben_und_rechtecke() {
        assert_eq!(color_rgba(0), [0, 0, 0, 0]);
        // Gras normal: 0x7FB238 * 220/255.
        assert_eq!(color_rgba(4 + 1), [109, 153, 48, 255]);
        assert_eq!(color_rgba(12 * 4 + 2), [64, 64, 255, 255], "Wasser hell = Grundfarbe");
        let mut maps = Maps::default();
        maps.apply(MapItemData { id: 7, scale: 0, locked: false, decorations: None, patch: Some(MapPatch { width: 2, height: 1, x: 10, y: 20, colors: vec![5, 6] }) });
        let m = maps.get(7).unwrap();
        assert_eq!((m.colors[20 * 128 + 10], m.colors[20 * 128 + 11], m.colors[0]), (5, 6, 0));
        let bild = render(m, false, &mut |_| None);
        assert_eq!(&bild[(20 * 128 + 10) * 4..][..4], &color_rgba(5));
    }

    #[test]
    fn markierungen_im_bild() {
        let mut m = MapState::default();
        m.decorations.push(MapDecoration { kind: 2, x: 0, y: 0, rot: 0, name: None });
        m.decorations.push(MapDecoration { kind: 0, x: 40, y: 40, rot: 0, name: None });
        let rot = |_: &str| Some((8, [255u8, 0, 0, 255].repeat(64)));
        let bild = render(&m, true, &mut { rot });
        assert_eq!(&bild[(64 * 128 + 64) * 4..][..4], &[255, 0, 0, 255], "Marke in der Mitte");
        assert_eq!(bild[(84 * 128 + 84) * 4 + 3], 0, "Spielermarke nicht im Rahmen");
    }
}
