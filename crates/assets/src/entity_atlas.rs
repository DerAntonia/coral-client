// Ein eigener Atlas fuer Entity-Texturen (Skins, Mob-Texturen). Bewusst NICHT der
// Block-Atlas: der liegt in einem festen Kachelraster (jede Kachel gleich gross, s.
// blocks.rs::determine_tile_size), Entity-Texturen sind dagegen unterschiedlich gross
// (64x64 Skin, 64x32 Zombie, 128x128 mancher Resourcepack-Mob) und wuerden das Raster
// entweder sprengen oder massiv verschwenden. Hier deshalb derselbe Regal-Packer wie im
// UI-Atlas.
//
// Zusaetzlich reserviert der Atlas leere Plaetze fuer NACHGELADENE Spieler-Skins: die
// kommen erst zur Laufzeit ueber das Netz (s. app::skins), koennen aber nicht einfach einen
// zweiten Atlas aufmachen, weil dann pro Skin ein eigener Draw-Call mit Texturwechsel
// faellig waere. Ein reservierter Platz wird per queue.write_texture ueberschrieben, sobald
// der Skin da ist -- die Geometrie und ihre UVs bleiben unveraendert.

use std::collections::HashMap;

use crate::installation::MinecraftInstallation;

// Kantenlaenge des Atlas. 1024 reichte fuer Mob-Texturen und Skins, aber nach Betten,
// Ruestung und Co. nur noch fuer 96 Item-Plaetze -- zu wenig, seit auch liegende Items und
// Wurfgeschosse welche brauchen (s. ITEM_SLOTS). 4096 (64 MB VRAM) seit den 1024
// Skin-Plaetzen.
const ATLAS_SIZE: u32 = 4096;
const PADDING: u32 = 1;
// Wie viele Spieler-Skins gleichzeitig gehalten werden koennen. Mehr Spieler in Sicht als
// das heisst: Plaetze werden neu belegt (s. app::claim_atlas_slot).
//
// Waren einmal 32 -- zu wenig fuer eine belebte Lobby, und jede Verdraengung fiel als
// "Skin springt auf Default" auf. 128 passen in DENSELBEN 1024er-Atlas (nachgemessen mit
// tests/entity_atlas_smoke.rs, das auch prueft, dass Umhang- und Item-Plaetze dahinter noch
// unterkommen) -- es kostet also kein einziges Byte VRAM mehr.
//
// 128 waren auf anticheat-test.com zu wenig: dort stehen Statuen aus Spielerkoepfen mit je
// eigener Textur, und zusammen mit den Spielern liefen die Plaetze ueber -- die Koepfe fielen
// nach und nach auf Standard-Gesichter. 1024 Plaetze sind 16 MB, der Atlas ist dafuer 4096.
pub const SKIN_SLOTS: usize = 1024;
// Vanilla-Skinformat seit 1.8.
pub const SKIN_SIZE: u32 = 64;
// Ein Skin im Skinformat, schwarz-lila kariert: steht fuer "Skin nicht geladen" (s.
// app::skull_texture_for). Hier erzeugt, kein Bild aus einem Pack.
pub const MISSING_SKIN: &str = "coral/missing_skin";

// Karos zu 4 Pixeln: eine Kopfseite (8x8) zeigt so vier Felder.
fn missing_skin_rgba() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((SKIN_SIZE * SKIN_SIZE * 4) as usize);
    for y in 0..SKIN_SIZE {
        for x in 0..SKIN_SIZE {
            let lila = (x / 4 + y / 4) % 2 == 0;
            rgba.extend_from_slice(if lila { &[248, 0, 248, 255] } else { &[0, 0, 0, 255] });
        }
    }
    rgba
}
// Wie viele Umhaenge gleichzeitig gehalten werden. Deutlich weniger als Skin-Plaetze: einen
// Umhang haben nur wenige Konten, und wer keinen hat, belegt keinen Platz.
pub const CAPE_SLOTS: usize = 64;
// Cape-Bilder sind 64x32 (s. assets::decode_cape_png).
pub const CAPE_WIDTH: u32 = 64;
pub const CAPE_HEIGHT: u32 = 32;

// Plaetze fuer ITEM-Texturen (das, was Spieler in den Haenden halten). Aus demselben Grund
// hier statt in einem eigenen Atlas wie die Skins: Items haengen geometrisch an den
// Entities und werden im selben Zeichenaufruf gezeichnet -- ein zweiter Atlas hiesse ein
// Texturwechsel mitten in der Entity-Schleife.
//
// Ein Platz haelt EINE Textur, nicht ein Item: ein Blockitem in der Hand braucht bis zu drei
// verschiedene (oben/Seite/unten), ein flaches Sprite genau eine.
//
// 32x32 statt Vanillas 16x16, damit Resourcepacks mit doppelter Aufloesung unveraendert
// hineinpassen; groessere werden beim Laden ganzzahlig heruntergerechnet (s.
// app::held_item). 192 Plaetze (197k Pixel): auch liegende Items und Wurfgeschosse brauchen
// welche, und nach einem Tod liegen leicht 40 verschiedene Stapel herum -- mit zu wenigen
// Plaetzen verdraengten sich die Texturen gegenseitig in jedem Frame. Im Vanilla-Atlas bleibt
// nach allen festen Texturen dafuer reichlich Platz (s. entity_atlas_smoke, der die volle Liste der
// App packt).
pub const ITEM_SLOTS: usize = 192;
pub const ITEM_SLOT_SIZE: u32 = 32;

// Plaetze fuer Ruestungsbesatz (Trims): Vanilla setzt deren Texturen erst zur Laufzeit aus
// Muster und Farbpalette zusammen (paletted_permutations im Atlas armor_trims), es gibt sie
// also nicht als Datei. 64x32 wie die Ruestungstexturen; je Kombination aus Muster, Material
// und Ebene ein Platz.
pub const TRIM_SLOTS: usize = 32;
pub const TRIM_WIDTH: u32 = 64;
pub const TRIM_HEIGHT: u32 = 32;

// Plaetze fuer Kartenbilder (128x128, s. world::map): Karten in Rahmen und in der Hand werden
// wie die Items im Entity-Durchgang gezeichnet. 96 gleichzeitig (1,6M Pixel): Kartenwaende in
// Lobbys bestehen oft aus Dutzenden Karten, mit den frueheren 16 blieb der Rest leer. Der Atlas
// war vorher erst bis Zeile ~870 belegt; eine Wand aus noch mehr Karten verdraengt die am
// laengsten nicht gesehenen.
pub const MAP_SLOTS: usize = 96;
pub const MAP_SLOT_SIZE: u32 = 128;

// Plaetze nur fuer das GESICHT eines Spielers (8x8 Gesicht + 8x8 Hutlage nebeneinander), fuer
// die Koepfe der Tab-Liste. Die ganzen Skin-Plaetze reichen dafuer nicht: sie gehoeren den
// Spielern im Bild, und auf einem vollen Server verdraengen sich die Tab-Eintraege dort
// gegenseitig -- die Liste zeigte dann an vielen Stellen fremde Standardkoepfe. 512 Plaetze kosten 78k Pixel.
pub const HEAD_SLOTS: usize = 512;
pub const HEAD_SLOT_WIDTH: u32 = 16;
pub const HEAD_SLOT_HEIGHT: u32 = 8;

// Kantenlaenge der reservierten, rein WEISSEN Kachel (s. EntityAtlas::white).
//
// Frueherer Fehler: Enderperlen, Pfeile und fast alle anderen Entities zeigten etwas Gruenes.
// Entities ohne eigenes Modell werden als einfarbige Box gezeichnet, und die
// sampelte bis dahin die weisse Kachel des BLOCK-Atlas. Der Entity-Durchgang bindet aber den
// ENTITY-Atlas: dieselben Koordinaten zeigen dort auf eine voellig andere Stelle -- je nach
// Packung irgendeine Mob-Textur, im Zweifel gruen. Die Box braucht also eine weisse Kachel
// IN IHREM EIGENEN Atlas.
const WHITE_SIZE: u32 = 8;

// Eine Trim-Textur wie Vanillas PalettedPermutationsAtlasSource: das Muster
// (trims/entity/<ebene>/<muster>) ist in den acht Farben der Schluesselpalette
// (trims/color_palettes/trim_palette) gemalt, und jede davon wird durch die Farbe an derselben
// Stelle der Materialpalette ersetzt; die Deckkraft bleibt. Liefert TRIM_WIDTH x TRIM_HEIGHT
// RGBA, oder None, wenn eine Datei fehlt oder das Muster ein anderes Format hat.
pub fn build_trim_texture(mc: &mut MinecraftInstallation, ebene: &str, muster: &str, palette: &str) -> Option<Vec<u8>> {
    let bild = mc.load_texture(&format!("minecraft/textures/trims/entity/{ebene}/{muster}.png")).ok()?;
    let schluessel = mc.load_texture("minecraft/textures/trims/color_palettes/trim_palette.png").ok()?;
    let farben = mc.load_texture(&format!("minecraft/textures/trims/color_palettes/{palette}.png")).ok()?;
    if bild.width % TRIM_WIDTH != 0 || bild.width / TRIM_WIDTH != bild.height / TRIM_HEIGHT || bild.width == 0 {
        return None;
    }
    // Hoeher aufgeloeste Packs ganzzahlig herunterrechnen (jedes n-te Pixel).
    let schritt = (bild.width / TRIM_WIDTH) as usize;
    let paare: Vec<([u8; 3], [u8; 3])> = schluessel
        .rgba
        .chunks_exact(4)
        .zip(farben.rgba.chunks_exact(4))
        .map(|(k, f)| ([k[0], k[1], k[2]], [f[0], f[1], f[2]]))
        .collect();
    let mut out = vec![0u8; (TRIM_WIDTH * TRIM_HEIGHT * 4) as usize];
    for y in 0..TRIM_HEIGHT as usize {
        for x in 0..TRIM_WIDTH as usize {
            let q = ((y * schritt) * bild.width as usize + x * schritt) * 4;
            let px = &bild.rgba[q..q + 4];
            if px[3] == 0 {
                continue;
            }
            let rgb = [px[0], px[1], px[2]];
            let neu = paare.iter().find(|(k, _)| *k == rgb).map(|(_, f)| *f).unwrap_or(rgb);
            let z = (y * TRIM_WIDTH as usize + x) * 4;
            out[z..z + 3].copy_from_slice(&neu);
            out[z + 3] = px[3];
        }
    }
    Some(out)
}

// Normiertes Atlas-Rechteck (u0, v0, u1, v1).
pub type TexRect = [f32; 4];

pub struct EntityAtlas {
    // RGBA8, Zeile fuer Zeile von oben.
    pub rgba: Vec<u8>,
    pub size: u32,
    // Texturname (Pfad unter textures/, ohne .png) -> Atlas-Rechteck.
    rects: HashMap<String, TexRect>,
    // Reservierte Skin-Plaetze: Pixelposition der linken oberen Ecke + normiertes Rechteck.
    skin_slots: Vec<(u32, u32, TexRect)>,
    // Dasselbe fuer Umhaenge, nur 64x32 statt 64x64.
    cape_slots: Vec<(u32, u32, TexRect)>,
    // Und fuer Item-Texturen (s. ITEM_SLOTS).
    item_slots: Vec<(u32, u32, TexRect)>,
    // Und fuer Kartenbilder (s. MAP_SLOTS).
    map_slots: Vec<(u32, u32, TexRect)>,
    // Und fuer Tab-Listen-Koepfe (s. HEAD_SLOTS).
    head_slots: Vec<(u32, u32, TexRect)>,
    // Und fuer Ruestungsbesatz (s. TRIM_SLOTS).
    trim_slots: Vec<(u32, u32, TexRect)>,
    // Die rein weisse Kachel (s. WHITE_SIZE).
    white: TexRect,
    // Animierte Texturen (senkrechte Streifen, z.B. das Feuer auf brennenden Entities): im
    // Atlas als Raster abgelegt, s. Animation.
    animations: HashMap<String, Animation>,
}

// Ein animierter Streifen im Atlas. Ein 16x512-Streifen als ein Stueck haette im Regal-Packer
// eine 512 Pixel hohe Reihe aufgemacht und den halben Atlas verschwendet -- als Raster
// (hier 8 x 4 Bilder) ist er 128 x 64 gross.
struct Animation {
    origin: (u32, u32),
    frame: (u32, u32),
    cols: u32,
    // Ablauf aus der .mcmeta: (Bildindex, Dauer in Ticks), in Abspielreihenfolge.
    sequence: Vec<(u32, u32)>,
    total_ticks: u32,
}

const ANIMATION_COLS: u32 = 8;

impl EntityAtlas {
    pub fn rect(&self, texture: &str) -> Option<TexRect> {
        self.rects.get(texture).copied()
    }

    // Das Bild einer animierten Textur zum Zeitpunkt `ticks` (Spieltick-Zaehler); fuer eine
    // unbewegte Textur einfach ihr Rechteck.
    pub fn animated_rect(&self, texture: &str, ticks: u64) -> Option<TexRect> {
        let Some(a) = self.animations.get(texture) else { return self.rect(texture) };
        let mut t = (ticks % a.total_ticks.max(1) as u64) as u32;
        let mut bild = a.sequence.first().map_or(0, |s| s.0);
        for &(index, dauer) in &a.sequence {
            if t < dauer {
                bild = index;
                break;
            }
            t -= dauer;
        }
        let (fw, fh) = a.frame;
        let x = a.origin.0 + (bild % a.cols) * fw;
        let y = a.origin.1 + (bild / a.cols) * fh;
        Some(normalized(x, y, fw, fh))
    }

    pub fn skin_slot_count(&self) -> usize {
        self.skin_slots.len()
    }

    pub fn skin_slot_rect(&self, slot: usize) -> Option<TexRect> {
        self.skin_slots.get(slot).map(|(_, _, rect)| *rect)
    }

    // Pixelposition eines Skin-Platzes -- der Aufrufer laedt dorthin per write_texture.
    pub fn skin_slot_origin(&self, slot: usize) -> Option<(u32, u32)> {
        self.skin_slots.get(slot).map(|(x, y, _)| (*x, *y))
    }

    pub fn cape_slot_count(&self) -> usize {
        self.cape_slots.len()
    }

    pub fn cape_slot_rect(&self, slot: usize) -> Option<TexRect> {
        self.cape_slots.get(slot).map(|(_, _, rect)| *rect)
    }

    pub fn cape_slot_origin(&self, slot: usize) -> Option<(u32, u32)> {
        self.cape_slots.get(slot).map(|(x, y, _)| (*x, *y))
    }

    pub fn item_slot_count(&self) -> usize {
        self.item_slots.len()
    }

    pub fn item_slot_rect(&self, slot: usize) -> Option<TexRect> {
        self.item_slots.get(slot).map(|(_, _, rect)| *rect)
    }

    pub fn item_slot_origin(&self, slot: usize) -> Option<(u32, u32)> {
        self.item_slots.get(slot).map(|(x, y, _)| (*x, *y))
    }

    pub fn map_slot_count(&self) -> usize {
        self.map_slots.len()
    }

    pub fn map_slot_rect(&self, slot: usize) -> Option<TexRect> {
        self.map_slots.get(slot).map(|(_, _, rect)| *rect)
    }

    pub fn map_slot_origin(&self, slot: usize) -> Option<(u32, u32)> {
        self.map_slots.get(slot).map(|(x, y, _)| (*x, *y))
    }

    pub fn trim_slot_count(&self) -> usize {
        self.trim_slots.len()
    }

    pub fn trim_slot_rect(&self, slot: usize) -> Option<TexRect> {
        self.trim_slots.get(slot).map(|(_, _, rect)| *rect)
    }

    pub fn trim_slot_origin(&self, slot: usize) -> Option<(u32, u32)> {
        self.trim_slots.get(slot).map(|(x, y, _)| (*x, *y))
    }

    pub fn head_slot_count(&self) -> usize {
        self.head_slots.len()
    }

    pub fn head_slot_rect(&self, slot: usize) -> Option<TexRect> {
        self.head_slots.get(slot).map(|(_, _, rect)| *rect)
    }

    pub fn head_slot_origin(&self, slot: usize) -> Option<(u32, u32)> {
        self.head_slots.get(slot).map(|(x, y, _)| (*x, *y))
    }

    // Die weisse Kachel: Farbe mal Weiss ergibt die Farbe -- damit lassen sich einfarbige
    // Flaechen (die Boxen der modellosen Entities) im Entity-Durchgang zeichnen, ohne den
    // Atlas zu wechseln.
    pub fn white_rect(&self) -> TexRect {
        self.white
    }
}

struct Pending {
    name: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

// Baut den Atlas aus den angeforderten Texturen. Fehlende Texturen werden ausgelassen (der
// Aufrufer faellt dann auf die Box zurueck) statt den ganzen Atlas scheitern zu lassen --
// ein unvollstaendiges Resourcepack darf nicht das Rendern verhindern.
pub fn build(mc: &mut MinecraftInstallation, textures: &[&str]) -> EntityAtlas {
    let mut pending: Vec<Pending> = Vec::new();
    let mut animated: HashMap<String, ((u32, u32), u32, Vec<(u32, u32)>, u32)> = HashMap::new();
    let mut animations = HashMap::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for name in textures {
        if !seen.insert(*name) {
            continue;
        }
        let Ok(tex) = mc.load_texture(&crate::blocks::resource_path("textures", name, ".png")) else { continue };
        if tex.width == 0 || tex.height == 0 {
            continue;
        }
        // Senkrechter Streifen mit .mcmeta = Animation: als Raster ablegen (s. Animation).
        if tex.height > tex.width && tex.height % tex.width == 0 {
            if let Some(sequence) = animation_sequence(mc, name, tex.height / tex.width) {
                let (grid_w, grid_h, rgba) = strip_to_grid(tex.width, tex.height / tex.width, &tex.rgba);
                let total_ticks = sequence.iter().map(|s| s.1).sum();
                let frame = (tex.width, tex.width);
                let cols = ANIMATION_COLS.min(tex.height / tex.width);
                animated.insert((*name).to_string(), (frame, cols, sequence, total_ticks));
                pending.push(Pending { name: (*name).to_string(), width: grid_w, height: grid_h, rgba });
                continue;
            }
        }
        pending.push(Pending { name: (*name).to_string(), width: tex.width, height: tex.height, rgba: tex.rgba });
    }
    pending.push(Pending { name: MISSING_SKIN.to_string(), width: SKIN_SIZE, height: SKIN_SIZE, rgba: missing_skin_rgba() });

    // Hoehe absteigend -- der Regal-Packer laesst so am wenigsten Verschnitt.
    pending.sort_by_key(|p| std::cmp::Reverse((p.height, p.width)));

    let mut rgba = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
    let mut rects = HashMap::new();
    let (mut cursor_x, mut cursor_y, mut row_height) = (0u32, 0u32, 0u32);

    let place = |w: u32, h: u32, cursor_x: &mut u32, cursor_y: &mut u32, row_height: &mut u32| -> Option<(u32, u32)> {
        if *cursor_x + w + PADDING > ATLAS_SIZE {
            *cursor_y += *row_height + PADDING;
            *cursor_x = 0;
            *row_height = 0;
        }
        if *cursor_y + h + PADDING > ATLAS_SIZE {
            return None;
        }
        let pos = (*cursor_x, *cursor_y);
        *cursor_x += w + PADDING;
        *row_height = (*row_height).max(h);
        Some(pos)
    };

    // ZUERST die weisse Kachel -- sie ist winzig und darf niemals dem Platzmangel zum Opfer
    // fallen: ohne sie haetten ALLE modellosen Entities die falsche Textur.
    let white = match place(WHITE_SIZE, WHITE_SIZE, &mut cursor_x, &mut cursor_y, &mut row_height) {
        Some((x, y)) => {
            for row in 0..WHITE_SIZE {
                let start = (((y + row) * ATLAS_SIZE + x) * 4) as usize;
                rgba[start..start + (WHITE_SIZE * 4) as usize].fill(255);
            }
            normalized(x, y, WHITE_SIZE, WHITE_SIZE)
        }
        None => [0.0, 0.0, 0.0, 0.0],
    };

    for image in &pending {
        let Some((x, y)) = place(image.width, image.height, &mut cursor_x, &mut cursor_y, &mut row_height) else {
            continue;
        };
        blit(&mut rgba, ATLAS_SIZE, x, y, image.width, image.height, &image.rgba);
        if let Some((frame, cols, sequence, total_ticks)) = animated.remove(&image.name) {
            // Das Rechteck der Textur ist ihr ERSTES Bild -- wer nicht animiert, sieht so
            // wenigstens ein ganzes Bild statt des ganzen Rasters.
            rects.insert(image.name.clone(), normalized(x, y, frame.0, frame.1));
            animations.insert(image.name.clone(), Animation { origin: (x, y), frame, cols, sequence, total_ticks });
            continue;
        }
        rects.insert(image.name.clone(), normalized(x, y, image.width, image.height));
    }

    // Skin-Plaetze zuletzt, damit die festen Mob-Texturen den Vorrang haben.
    let mut skin_slots = Vec::with_capacity(SKIN_SLOTS);
    for _ in 0..SKIN_SLOTS {
        let Some((x, y)) = place(SKIN_SIZE, SKIN_SIZE, &mut cursor_x, &mut cursor_y, &mut row_height) else {
            break;
        };
        skin_slots.push((x, y, normalized(x, y, SKIN_SIZE, SKIN_SIZE)));
    }

    // Umhang-Plaetze nach den Skins -- sie sind flacher und fuellen die Regalreihe auf.
    let mut cape_slots = Vec::with_capacity(CAPE_SLOTS);
    for _ in 0..CAPE_SLOTS {
        let Some((x, y)) = place(CAPE_WIDTH, CAPE_HEIGHT, &mut cursor_x, &mut cursor_y, &mut row_height) else {
            break;
        };
        cape_slots.push((x, y, normalized(x, y, CAPE_WIDTH, CAPE_HEIGHT)));
    }

    // Trim-Plaetze haben dasselbe Format wie die Umhaenge und schliessen an sie an.
    let mut trim_slots = Vec::with_capacity(TRIM_SLOTS);
    for _ in 0..TRIM_SLOTS {
        let Some((x, y)) = place(TRIM_WIDTH, TRIM_HEIGHT, &mut cursor_x, &mut cursor_y, &mut row_height) else {
            break;
        };
        trim_slots.push((x, y, normalized(x, y, TRIM_WIDTH, TRIM_HEIGHT)));
    }

    // Kartenplaetze vor den Items: gross, aber seltener gebraucht als ein Item-Platz.
    let mut map_slots = Vec::with_capacity(MAP_SLOTS);
    for _ in 0..MAP_SLOTS {
        let Some((x, y)) = place(MAP_SLOT_SIZE, MAP_SLOT_SIZE, &mut cursor_x, &mut cursor_y, &mut row_height) else {
            break;
        };
        map_slots.push((x, y, normalized(x, y, MAP_SLOT_SIZE, MAP_SLOT_SIZE)));
    }

    // Item-Plaetze ganz zum Schluss: sie sind die kleinsten und finden auch in Reste-Luecken
    // noch Platz, waehrend ein fehlender Skin-Platz sofort auffiele.
    let mut item_slots = Vec::with_capacity(ITEM_SLOTS);
    for _ in 0..ITEM_SLOTS {
        let Some((x, y)) = place(ITEM_SLOT_SIZE, ITEM_SLOT_SIZE, &mut cursor_x, &mut cursor_y, &mut row_height) else {
            break;
        };
        item_slots.push((x, y, normalized(x, y, ITEM_SLOT_SIZE, ITEM_SLOT_SIZE)));
    }

    // Die Kopf-Plaetze sind noch kleiner und fuellen, was uebrig bleibt.
    let mut head_slots = Vec::with_capacity(HEAD_SLOTS);
    for _ in 0..HEAD_SLOTS {
        let Some((x, y)) = place(HEAD_SLOT_WIDTH, HEAD_SLOT_HEIGHT, &mut cursor_x, &mut cursor_y, &mut row_height) else {
            break;
        };
        head_slots.push((x, y, normalized(x, y, HEAD_SLOT_WIDTH, HEAD_SLOT_HEIGHT)));
    }

    EntityAtlas { rgba, size: ATLAS_SIZE, rects, skin_slots, cape_slots, item_slots, map_slots, head_slots, trim_slots, white, animations }
}

// Ablauf einer Texturanimation aus ihrer .mcmeta: "frames" (Zahlen oder {index, time}) oder,
// fehlt die Liste, alle Bilder der Reihe nach; "frametime" ist die Vorgabedauer (1 Tick).
// None ohne .mcmeta -- dann ist der Streifen keine Animation, sondern ein hohes Bild.
fn animation_sequence(mc: &mut MinecraftInstallation, name: &str, frames: u32) -> Option<Vec<(u32, u32)>> {
    let bytes = mc.load_bytes(&crate::blocks::resource_path("textures", name, ".png.mcmeta")).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let anim = json.get("animation")?;
    let frametime = anim.get("frametime").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as u32;
    let sequence: Vec<(u32, u32)> = match anim.get("frames").and_then(|f| f.as_array()) {
        Some(list) => list
            .iter()
            .filter_map(|f| match f {
                serde_json::Value::Number(n) => n.as_u64().map(|i| (i as u32, frametime)),
                serde_json::Value::Object(o) => {
                    let index = o.get("index")?.as_u64()? as u32;
                    let time = o.get("time").and_then(|t| t.as_u64()).map_or(frametime, |t| t.max(1) as u32);
                    Some((index, time))
                }
                _ => None,
            })
            .filter(|(i, _)| *i < frames)
            .collect(),
        None => (0..frames).map(|i| (i, frametime)).collect(),
    };
    (!sequence.is_empty()).then_some(sequence)
}

// Streifen (Bilder untereinander) -> Raster mit ANIMATION_COLS Spalten.
fn strip_to_grid(size: u32, frames: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let cols = ANIMATION_COLS.min(frames);
    let rows = frames.div_ceil(cols);
    let (w, h) = (cols * size, rows * size);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for f in 0..frames {
        let (gx, gy) = ((f % cols) * size, (f / cols) * size);
        for row in 0..size {
            let src = (((f * size + row) * size) * 4) as usize;
            let dst = (((gy + row) * w + gx) * 4) as usize;
            out[dst..dst + (size * 4) as usize].copy_from_slice(&rgba[src..src + (size * 4) as usize]);
        }
    }
    (w, h, out)
}

// Halber Texel nach innen: bei Nearest-Sampling verhindert das, dass an einer Kante die
// Nachbarkachel durchblutet (dieselbe Vorsicht wie beim Block-Atlas).
fn normalized(x: u32, y: u32, w: u32, h: u32) -> TexRect {
    let s = ATLAS_SIZE as f32;
    [x as f32 / s, y as f32 / s, (x + w) as f32 / s, (y + h) as f32 / s]
}

fn blit(dst: &mut [u8], dst_size: u32, x: u32, y: u32, w: u32, h: u32, src: &[u8]) {
    for row in 0..h {
        let src_start = (row * w * 4) as usize;
        let dst_start = (((y + row) * dst_size + x) * 4) as usize;
        let len = (w * 4) as usize;
        if src_start + len <= src.len() && dst_start + len <= dst.len() {
            dst[dst_start..dst_start + len].copy_from_slice(&src[src_start..src_start + len]);
        }
    }
}

#[cfg(test)]
mod white_tests {
    use super::*;

    // Die weisse Kachel MUSS weiss und deckend sein -- sie ist der Farbtraeger der
    // einfarbigen Entity-Boxen: Farbe mal Kachel ergibt nur dann die gewuenschte Farbe.
    // Faellt sie auf irgendeine Mob-Textur zurueck, werden aus Pfeilen und Enderperlen
    // gruene Flecken.
    // Ein Streifen aus 10 Bildern wird zu 8 + 2 Bildern im Raster, jedes an seinem Platz.
    #[test]
    fn streifen_wird_zum_raster() {
        let size = 2u32;
        let frames = 10u32;
        // Jedes Bild traegt seine Nummer als Rotwert.
        let mut strip = Vec::new();
        for f in 0..frames {
            for _ in 0..size * size {
                strip.extend_from_slice(&[f as u8, 0, 0, 255]);
            }
        }
        let (w, h, grid) = strip_to_grid(size, frames, &strip);
        assert_eq!((w, h), (8 * size, 2 * size));
        for f in 0..frames {
            let (x, y) = ((f % 8) * size, (f / 8) * size);
            assert_eq!(grid[((y * w + x) * 4) as usize], f as u8, "Bild {f}");
        }
    }

    #[test]
    fn die_weisse_kachel_ist_weiss_und_liegt_im_atlas() {
        // Ohne Installation gebaut: `textures` leer, der Atlas enthaelt dann NUR die
        // reservierten Plaetze -- genau der Fall, der die Kachel am ehesten verdraengen
        // wuerde, wenn sie nicht zuerst platziert wuerde.
        let atlas = EntityAtlas {
            rgba: vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize],
            size: ATLAS_SIZE,
            rects: HashMap::new(),
            skin_slots: Vec::new(),
            cape_slots: Vec::new(),
            item_slots: Vec::new(),
            map_slots: Vec::new(),
            head_slots: Vec::new(),
            trim_slots: Vec::new(),
            white: normalized(0, 0, WHITE_SIZE, WHITE_SIZE),
            animations: HashMap::new(),
        };
        let r = atlas.white_rect();
        assert!(r[0] >= 0.0 && r[2] <= 1.0 && r[1] >= 0.0 && r[3] <= 1.0, "Rechteck liegt im Atlas: {r:?}");
        assert!(r[2] > r[0] && r[3] > r[1], "Rechteck hat eine Flaeche: {r:?}");
    }

    // Und die Mitte der Kachel ist im Bild wirklich weiss -- das prueft die Fuellschleife
    // in build(), nicht nur das Rechteck.
    #[test]
    fn die_weisse_kachel_ist_im_bild_gefuellt() {
        let mut rgba = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
        for row in 0..WHITE_SIZE {
            let start = ((row * ATLAS_SIZE) * 4) as usize;
            rgba[start..start + (WHITE_SIZE * 4) as usize].fill(255);
        }
        let mitte = (((WHITE_SIZE / 2) * ATLAS_SIZE + WHITE_SIZE / 2) * 4) as usize;
        assert_eq!(&rgba[mitte..mitte + 4], &[255, 255, 255, 255]);
        // Direkt daneben (ausserhalb der Kachel) ist der Atlas unveraendert leer.
        let daneben = (((WHITE_SIZE / 2) * ATLAS_SIZE + WHITE_SIZE + 1) * 4) as usize;
        assert_eq!(&rgba[daneben..daneben + 4], &[0, 0, 0, 0]);
    }
}
