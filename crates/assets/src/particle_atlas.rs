// Atlas der Partikel-Sprites -- Vanillas SpriteAtlasTexture.PARTICLE_ATLAS_TEXTURE.
//
// Jeder Partikeltyp nennt in `particles/<name>.json` eine GEORDNETE Liste von Texturen unter
// `textures/particle/`. Die Reihenfolge ist Bedeutung, nicht Zufall: gealterte Partikel laufen
// die Liste von vorn nach hinten ab (SpriteProvider.getSprite(age, maxAge)), andere ziehen
// zufaellig daraus (getSprite(random)). Deshalb bekommt jeder Typ hier seine Liste in genau
// dieser Reihenfolge, auch wenn mehrere Typen dieselben Bilder teilen (generic_0..7).
//
// Typen ohne JSON (block, item, ...) sampeln einen ANDEREN Atlas und fehlen hier absichtlich.

use std::collections::HashMap;

use serde::Deserialize;

use crate::entity_atlas::TexRect;
use crate::installation::MinecraftInstallation;

const PADDING: u32 = 1;

#[derive(Deserialize)]
struct ParticleDefinition {
    #[serde(default)]
    textures: Vec<String>,
}

pub struct ParticleAtlas {
    pub rgba: Vec<u8>,
    pub size: u32,
    // Partikeltyp (ohne Namensraum) -> Sprites in JSON-Reihenfolge.
    sprites: HashMap<String, Vec<TexRect>>,
    // Itemname -> Splitterbild (s. ui_atlas::item_particle_texture).
    items: HashMap<String, TexRect>,
}

impl ParticleAtlas {
    pub fn sprites(&self, particle: &str) -> &[TexRect] {
        self.sprites.get(particle).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn item_sprites(&self) -> impl Iterator<Item = (&str, TexRect)> {
        self.items.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

// Erstes Bild (Animationen stehen senkrecht gestapelt), auf hoechstens `max` Pixel Kante
// verkleinert. Jedes n-te Pixel statt eines Mittelwerts: Pixelkunst bleibt scharf und
// durchsichtige Raender bleiben durchsichtig.
fn first_frame_scaled(w: u32, h: u32, rgba: &[u8], max: u32) -> (u32, u32, Vec<u8>) {
    let h = h.min(w).max(1);
    let schritt = w.div_ceil(max).max(1);
    let (nw, nh) = (w / schritt, h / schritt);
    if schritt == 1 {
        return (w, h, rgba[..(w * h * 4) as usize].to_vec());
    }
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let i = (((y * schritt) * w + x * schritt) * 4) as usize;
            out.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    (nw, nh, out)
}

// Sprite-ID "namensraum:pfad" -> Pfad unterhalb von assets/. Ohne Namensraum gilt
// "minecraft" (Identifier.of). Resourcepacks duerfen auf eigene Namensraeume zeigen.
fn sprite_path(id: &str) -> String {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("{ns}/textures/particle/{path}.png")
}

// Groesste Kantenlaenge eines Splitterbilds im Atlas. Ein Splitter zeigt nur ein Viertel des
// Bilds, winzig klein -- bei HD-Packs (128 px je Item) wuerde der Atlas sonst mit rund 1500
// Items riesig.
const ITEM_SPRITE_MAX: u32 = 32;

// Baut den Atlas fuer die genannten Partikeltypen. Fehlende Definitionen oder Bilder werden
// ausgelassen: ein Resourcepack, das einzelne Partikel leert, darf das Rendern nicht
// verhindern -- der Typ zeichnet dann eben nichts.
pub fn build(mc: &mut MinecraftInstallation, particle_names: &[&str], items: &[String]) -> ParticleAtlas {
    let mut lists: Vec<(String, Vec<String>)> = Vec::new();
    let mut images: HashMap<String, (u32, u32, Vec<u8>)> = HashMap::new();
    for name in particle_names {
        let Ok(bytes) = mc.load_bytes(&format!("minecraft/particles/{name}.json")) else { continue };
        let Ok(def) = serde_json::from_slice::<ParticleDefinition>(&bytes) else { continue };
        let textures: Vec<String> = def.textures.clone();
        for tex in &textures {
            if images.contains_key(tex) {
                continue;
            }
            let Ok(img) = mc.load_texture(&sprite_path(tex)) else { continue };
            // Hoeher als breit = senkrecht gestapelte Animationsbilder (.mcmeta). Das erste
            // Bild genuegt -- animierte Partikelsprites gibt es in Vanilla nur bei vibration.
            let h = img.height.min(img.width).max(1);
            let rgba = img.rgba[..(img.width * h * 4) as usize].to_vec();
            images.insert(tex.clone(), (img.width, h, rgba));
        }
        lists.push(((*name).to_string(), textures));
    }
    // Die XP-Kugel ist kein Partikel, wird aber genauso zur Kamera gedreht gezeichnet -- ihr
    // Bild (4x4 Groessenstufen) liegt deshalb mit im Atlas, als Pseudo-Typ "experience_orb".
    if let Ok(img) = mc.load_texture("minecraft/textures/entity/experience_orb.png") {
        let key = "@experience_orb".to_string();
        images.insert(key.clone(), (img.width, img.height, img.rgba));
        lists.push(("experience_orb".to_string(), vec![key]));
    }
    // Splitter der Item-Partikel (Typ "item"), je Item ein Bild.
    let mut item_keys: Vec<(String, String)> = Vec::new();
    for item in items {
        let Some(tex) = crate::ui_atlas::item_particle_texture(mc, item) else { continue };
        let key = format!("@item_tex/{tex}");
        if !images.contains_key(&key) {
            let Ok(img) = mc.load_texture(&crate::blocks::resource_path("textures", &tex, ".png")) else { continue };
            images.insert(key.clone(), first_frame_scaled(img.width, img.height, &img.rgba, ITEM_SPRITE_MAX));
        }
        item_keys.push((item.clone(), key));
    }

    let mut order: Vec<&String> = images.keys().collect();
    order.sort_by_key(|k| (std::cmp::Reverse(images[*k].1), std::cmp::Reverse(images[*k].0), (*k).clone()));

    // Kleinste Zweierpotenz, in die der Regal-Packer alles unterbringt.
    let mut size = 64u32;
    let placed = loop {
        if let Some(p) = pack(&order, &images, size) {
            break p;
        }
        size *= 2;
    };

    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let mut rects: HashMap<&str, TexRect> = HashMap::new();
    for (name, (x, y)) in order.iter().zip(&placed) {
        let (w, h, src) = &images[*name];
        for row in 0..*h {
            let s = (row * w * 4) as usize;
            let d = (((y + row) * size + x) * 4) as usize;
            rgba[d..d + (w * 4) as usize].copy_from_slice(&src[s..s + (w * 4) as usize]);
        }
        let f = size as f32;
        rects.insert(name.as_str(), [*x as f32 / f, *y as f32 / f, (x + w) as f32 / f, (y + h) as f32 / f]);
    }

    let sprites = lists
        .into_iter()
        .map(|(name, textures)| {
            let list = textures.iter().filter_map(|t| rects.get(t.as_str()).copied()).collect();
            (name, list)
        })
        .collect();
    let items = item_keys.into_iter().filter_map(|(item, key)| rects.get(key.as_str()).map(|r| (item, *r))).collect();
    ParticleAtlas { rgba, size, sprites, items }
}

fn pack(order: &[&String], images: &HashMap<String, (u32, u32, Vec<u8>)>, size: u32) -> Option<Vec<(u32, u32)>> {
    let (mut x, mut y, mut row_h) = (0u32, 0u32, 0u32);
    let mut out = Vec::with_capacity(order.len());
    for name in order {
        let (w, h, _) = images[*name];
        if w + PADDING > size {
            return None;
        }
        if x + w + PADDING > size {
            y += row_h + PADDING;
            x = 0;
            row_h = 0;
        }
        if y + h + PADDING > size {
            return None;
        }
        out.push((x, y));
        x += w + PADDING;
        row_h = row_h.max(h);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sprite_ids_mit_und_ohne_namensraum() {
        assert_eq!(super::sprite_path("minecraft:critical_hit"), "minecraft/textures/particle/critical_hit.png");
        assert_eq!(super::sprite_path("generic_0"), "minecraft/textures/particle/generic_0.png");
        assert_eq!(super::sprite_path("meinpack:funke"), "meinpack/textures/particle/funke.png");
    }
}
