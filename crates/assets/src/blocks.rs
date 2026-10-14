// Loest Blocknamen ueber die Vanilla-Modell-JSONs zu ihren sechs Flaechentexturen auf und
// packt alle benoetigten Texturen in einen einzigen Atlas (eine GPU-Textur).
//
// Ablauf pro Block (z.B. "grass_block"):
//   blockstates/<name>.json  -> waehlt ein Modell (MVP: erste Variante, Properties ignoriert)
//   models/block/<modell>.json + parent-Kette -> merged `textures`-Map + naechstgelegene
//   `elements`. Aus dem ersten Voll-Wuerfel-Element (from~[0,0,0] to~[16,16,16]) die sechs
//   Flaechen (down/up/north/south/east/west), deren `#var`-Texturverweise ueber die
//   textures-Map aufgeloest werden.
//
// Bewusst NICHT (MVP, s. Briefing "haesslich/unvollstaendig ok"): Blockstate-Properties
// (Ausrichtung von Staemmen etc.), Multipart (Zaeune/Mauern), Nicht-Wuerfel-Formen (Stufen,
// Pflanzen), Biome-Tint (Gras/Laub erscheinen ungetoent), Modell-Rotation. Animierte Texturen
// (.mcmeta) landen mit ihrem ersten Frame im Atlas, die weiteren tauscht der Aufrufer zur
// Laufzeit ein (s. TileAnimation). Was nicht als Voll-Wuerfel aufloest,
// bekommt seine Partikeltextur auf alle Flaechen -- sichtbar und ungefaehr richtig.

use std::collections::HashMap;

use serde::Deserialize;

use crate::installation::{AssetsError, MinecraftInstallation};

// Flaechenreihenfolge im Atlas-Ergebnis. Muss mit der Nutzung im Mesher uebereinstimmen.
pub const FACE_DOWN: usize = 0;
pub const FACE_UP: usize = 1;
pub const FACE_NORTH: usize = 2;
pub const FACE_SOUTH: usize = 3;
pub const FACE_WEST: usize = 4;
pub const FACE_EAST: usize = 5;
pub(crate) const FACE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

// UV-Rechteck im Atlas (normiert 0..1), Reihenfolge u0,v0,u1,v1.
pub type UvRect = [f32; 4];

pub struct BlockAtlas {
    // Mipmap-Kette des quadratischen RGBA8-Atlas, Index 0 = volle Aufloesung (Zeile fuer
    // Zeile von oben), jede weitere Stufe halb so gross -- s. MIPMAP_LEVELS/build_tile_mips.
    pub mips: Vec<Vec<u8>>,
    // Kantenlaenge von mips[0] (Level 0). Die UV-Rechtebte (s. uvs) sind unabhaengig vom
    // Mip-Level immer relativ zur VOLLEN Aufloesung normiert -- die GPU waehlt die passende
    // Mip-Stufe selbst anhand des Sichtabstands.
    pub size: u32,
    // Kantenlaenge EINER Kachel im Atlas (bisher immer fest 16 -- jetzt die groesste
    // tatsaechlich vorkommende Texturbreite unter allen geladenen Block-Texturen, auf die
    // naechste Zweierpotenz gerundet, min. 16, gedeckelt bei 256, s. determine_tile_size).
    // Kleinere Texturen (z.B. Vanilla-Reste bei einem Teil-Resourcepack) werden per
    // Nearest-Neighbor auf diese eine Groesse hochskaliert -- EIN einheitliches Raster statt
    // Rechteck-Packing, damit die ganze Atlas-/UV-Mathematik unveraendert bleibt.
    pub tile_size: u32,
    // Pro Blockname die sechs Flaechen-UVs (Index FACE_*).
    pub uvs: HashMap<String, [UvRect; 6]>,
    // Pro Blockname die Inhaltsflaechen-UVs (s. resolve_block_faces-Kommentar, z.B.
    // Kesselinhalt) -- None wo der Block keine eigenstaendige Inhaltsflaeche hat (die
    // grosse Mehrheit), bzw. der ganze Blockeintrag fehlt.
    pub content_uvs: HashMap<String, [Option<UvRect>; 6]>,
    // Pro Blockname die Overlay-Flaechen-UVs (s. resolve_block_faces-Kommentar, z.B.
    // grass_block-Seiten: Basis-Element ohne tintindex + Overlay-Element MIT tintindex auf
    // EXAKT derselben Box) -- eine EIGENE, UNGETOENTE Atlas-Kachel (nicht wie frueher fest
    // in die Basistextur eingerechnet, s. GRASS_SIDE_TINT-Historie), damit der Mesher sie
    // als zweite, per Biom-Vertexfarbe eingefaerbte Geometrie-Ebene zeichnen kann (echtes
    // Alpha-Blending ueber die Translucent-Pipeline, s. world::mesher). None wo der Block
    // keine solche Overlay-Flaeche hat (die grosse Mehrheit).
    pub overlay_uvs: HashMap<String, [Option<UvRect>; 6]>,
    // Flaechen (und Overlay) der WEITEREN Modelle eines Blocks, nach Modellname -- `uvs` kennt
    // je Block nur das erste Modell. Ohne das sah jeder Respawn-Anker ungeladen aus
    // (respawn_anchor_1..4 nennen eine andere Oberseite), ebenso ein brennender
    // Ofen oder verschneites Gras. Welcher State welches Modell hat, s. state_variant_models.
    pub variant_uvs: HashMap<String, ([UvRect; 6], [Option<UvRect>; 6])>,
    // Pro Blockname die Partikeltextur (Modellfeld "particle", sonst irgendeine Textur der
    // Modellkette) -- Vanillas BlockModels.getModelParticleSprite. Daraus kommt der
    // Blockstaub beim Abbauen und Zerbrechen. Weicht oft von allen Flaechen ab: der
    // Grasblock staubt Erde, das Bett Eichenbretter.
    pub particle_uvs: HashMap<String, UvRect>,
    // Pro Blockname (nur fuer Bloecke mit Vanillas pseudozufaelliger Block-Rotation, s.
    // world::render_rng-Modulkommentar) die vollstaendig aufgeloesten sechs Flaechen-UVs
    // JEDER Zufallsvariante -- Index 0 ist immer identisch mit uvs[block] (die kanonische,
    // unrotierte Zuordnung). Fehlt komplett fuer die weit ueberwiegende Mehrheit der
    // Bloecke (kein Array-Selektor im Blockstate-JSON).
    //
    // WICHTIG: das ist NICHT einfach uvs[block] mit rotate_face_uvs() vertauscht (permute_
    // faces_x/y tauscht nur, WELCHE der sechs Flaechen-Texturen an welcher Richtung
    // landet) -- bei Bloecken wie Erde/Sand/Stein/Grasblock tragen mehrere/alle Flaechen
    // ohnehin DIESELBE Textur, eine reine Flaechen-Umsortierung waere dort unsichtbar.
    // Vanillas eigentlicher Effekt ist eine ECHTE Drehung/Spiegelung DES TEXTURBILDS auf
    // der jeweiligen Flaeche (am deutlichsten sichtbar: der Grasblock-Deckel dreht sich
    // wirklich) -- ein [u0,v0,u1,v1]-Rechteck kann eine 90/270-Grad-Drehung nicht
    // ausdruecken (braucht vertauschte Eckzuordnung statt reiner Intervallverschiebung),
    // deshalb werden hier fuer Oben/Unten echte, vorab pixelweise gedrehte/gespiegelte
    // Zusatzkacheln gebacken (s. rotate_tile_90/mirror_tile). Die vier Seitenflaechen
    // bleiben reine UV-Zuweisung (Umsortierung + Spiegel-Flag), das reicht dort aus.
    pub rng_face_uvs: HashMap<String, Vec<[UvRect; 6]>>,
    // ALLE Animationsframes von water_still.png (s. load_tile_frames-Kommentar), plus die
    // Pixelkoordinate (Level-0-Atlas, oben-links) der Wasser-Kachel -- damit der Aufrufer
    // (render::BlockRenderer::update_water_frame) periodisch NUR diese eine Kachelregion
    // per queue.write_texture() neu hochladen kann, statt den ganzen Atlas neu zu bauen
    // oder gar neu zu meshen (exakt Vanillas eigene Technik: die Geometrie/UV bleibt fix,
    // nur die Pixel darunter wechseln). Leer/None, falls water_still.png fehlt oder das
    // Wasser-Tile aus irgendeinem Grund nicht im Atlas gelandet ist.
    pub water_animation_frames: Vec<Vec<u8>>,
    pub water_tile_origin: Option<(u32, u32)>,
    // Alle uebrigen animierten Kacheln (Lava, Feuer, Portal, Prismarin, Magma, Seelaterne ...):
    // alles mit einer "animation" in der .mcmeta, ausser den beiden Wassertexturen, die oben
    // ihren eigenen Weg haben.
    pub animations: Vec<TileAnimation>,
    // Dasselbe noch einmal fuer water_flow.png -- Vanillas ZWEITE Wassertextur, die auf
    // fliessenden Zellen liegt und in Fliessrichtung gedreht wird (s.
    // world::mesher::push_fluid_quads). Sie ist eine eigenstaendige Kachel im Atlas, kein
    // Blockflaechen-Eintrag: "water" als Block zeigt weiterhin auf water_still, welche der
    // beiden eine Zelle bekommt, entscheidet erst der Mesher pro Zelle.
    //
    // water_flow.png ist in Vanilla 32 statt 16 Pixel breit (32x1024 = 32 Frames a 32x32).
    // Das ist hier unkritisch: load_tile_frames liest die NATIVE Breite und skaliert jeden
    // Frame selbst auf die Atlas-Kachelgroesse.
    pub water_flow_uv: Option<UvRect>,
    // UV von water_overlay (s. WATER_OVERLAY_TEXTURE) -- nicht animiert, deshalb nur das UV
    // ohne Frames/Ursprung.
    pub water_overlay_uv: Option<UvRect>,
    pub water_flow_animation_frames: Vec<Vec<u8>>,
    pub water_flow_tile_origin: Option<(u32, u32)>,
    // UV von lava_flow (s. LAVA_FLOW_TEXTURE). Animiert wird die Kachel ueber `animations`.
    pub lava_flow_uv: Option<UvRect>,
    // Fallback-UV fuer Bloecke/States ohne aufloesbare Textur (Missing-Kachel).
    pub missing: [UvRect; 6],
    // Jede geladene Kachel nach ihrem Texturpfad ("block/fire_0") -- fuer Modell-Vierecke,
    // die ihre Textur direkt nennen statt ueber Blockname+Flaeche (s. shapes::build_model_quads).
    pub texture_uvs: HashMap<String, UvRect>,
    // Austauschbare Kacheln je Texturpfad (derzeit nur Feuer, s. FIRE_VARIANTS): alle UVs, aus
    // denen der Mesher je Block eine waehlt. Gleiche Liste fuer beide Texturen eines Paars.
    pub texture_variants: HashMap<String, Vec<UvRect>>,
    // Uebrige animierte Kacheln (Seegras, Prismarin, animierte Erze aus Resourcepacks ...):
    // je Basiskachel die Kachelindizes ihrer zeitversetzten Kopien (s. ANIM_VARIANTS). Der
    // Mesher biegt die UVs eines Blocks per Positions-Hash auf eine davon um, s.
    // world::BlockUvs::with_anim_variants. Kachelindex = Zeile * tiles_per_row + Spalte.
    pub anim_variant_tiles: Vec<(u32, Vec<u32>)>,
    pub tiles_per_row: u32,
    // Rein weisse Kachel -- die einfarbigen Entity-Boxen sampeln sie, damit Farbe * Weiss
    // = Farbe herauskommt (der Shader multipliziert immer mit der Atlas-Textur).
    pub white_uv: UvRect,
    // UVs der zehn Riss-Stufen (Index = Stufe 0..9, s. DESTROY_STAGE_COUNT). None, wo die
    // Textur fehlt -- dann bleibt der Block beim Abbauen einfach ohne Risse.
    pub destroy_stage_uvs: [Option<UvRect>; DESTROY_STAGE_COUNT],
}

#[derive(Deserialize)]
pub(crate) struct BlockstateJson {
    // BTreeMap statt HashMap: first_model() (unten) nimmt zum Bau des EINEN Textur-Sets
    // pro Blockname (nicht pro State) einfach den ERSTEN Eintrag -- mit einer HashMap ist
    // deren Iterationsreihenfolge zufaellig pro Prozess-Start, d.h. bei Bloecken mit
    // texturrelevanten Varianten (z.B. grass_block: "snowy=false" vs. "snowy=true" mit der
    // eigenen, tatsaechlich schnee-bedeckten "grass_block_snow"-Seitentextur) waere
    // zufaellig mal die eine, mal die andere Textur im ganzen Atlas gelandet -- fuer JEDEN
    // grass_block der Welt, nicht nur schneebedeckte. Eine BTreeMap sortiert nach dem
    // Selektor-String; "snowy=false" < "snowy=true" alphabetisch, landet also
    // deterministisch zuerst -- der sinnvollere Default. shapes.rs (echte Pro-State-
    // Aufloesung ueber matches_selector) ist von dieser Reihenfolge nicht betroffen, die
    // prueft ohnehin jeden Eintrag gegen die echten Properties.
    #[serde(default)]
    pub(crate) variants: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub(crate) multipart: Vec<MultipartCase>,
}

#[derive(Deserialize)]
pub(crate) struct MultipartCase {
    pub(crate) apply: serde_json::Value,
    // Fehlt bei multipart-Eintraegen ohne Bedingung (gilt dann immer, z.B. der Zaun-Pfosten).
    #[serde(default)]
    pub(crate) when: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub(crate) struct ModelJson {
    pub(crate) parent: Option<String>,
    // "ambientocclusion": false schaltet Vanillas Eckabdunklung fuer dieses Modell ab
    // (JsonUnbakedModel.ambientOcclusion) -- erbt sich die Parent-Kette hinauf, Vorgabe true.
    #[serde(default)]
    pub(crate) ambientocclusion: Option<bool>,
    #[serde(default)]
    pub(crate) textures: HashMap<String, String>,
    pub(crate) elements: Option<Vec<Element>>,
    // "display": wie das Modell in Hand/GUI/auf dem Boden sitzt. Roh gehalten, weil nur
    // einzelne Eintraege gebraucht werden (s. load_model_display).
    #[serde(default)]
    pub(crate) display: Option<serde_json::Value>,
}

#[derive(Deserialize, Clone)]
pub(crate) struct Element {
    pub(crate) from: [f32; 3],
    pub(crate) to: [f32; 3],
    #[serde(default)]
    pub(crate) faces: HashMap<String, Face>,
    // Nur zur Erkennung vorhanden (nicht angewendet): ein Element mit eigener
    // Rotation (z.B. die diagonalen "cross"-Ebenen von Blumen/Gras) laesst sich nicht als
    // achsenparallele Box darstellen -- shapes.rs faellt in diesem Fall bewusst auf die
    // bisherige Vollwuerfel-Behandlung zurueck, statt eine falsche Box zu erzeugen.
    #[serde(default)]
    pub(crate) rotation: Option<serde_json::Value>,
    // "shade": false = keine Richtungshelligkeit (Feuer, Kreuz-Pflanzen). Vorgabe true.
    #[serde(default = "shade_default")]
    pub(crate) shade: bool,
}

fn shade_default() -> bool {
    true
}

// Manche Resourcepacks (PvP-Packs) haengen an ein Kreuz-Modell wie cobweb.json zusaetzlich
// flache, unrotierte "Hitbox"-Elemente (0 Dicke in einer Achse, eigene "hitbox"-Textur) an,
// um im eigenen Editor eine sichtbare Blockumriss-Hilfe zu haben -- reine Deko ohne
// Geometrie-Volumen, in Vanilla nicht vorhanden. Ein *echtes* zusaetzliches Element (z.B.
// die Zapfen-Box bei sea_pickle.json, der Griff bei lever.json) hat dagegen immer ein
// echtes Volumen (from != to in allen 3 Achsen). Nur an dieser Volumen-Eigenschaft laesst
// sich das unterscheiden, nicht an "hat Rotation oder nicht" allein. Von shapes.rs
// (Kreuz-Erkennung) UND resolve_block_faces (Textur-Auswahl) genutzt -- beide muessen
// dieselben Elemente als "nicht echt" ignorieren, sonst waehlt die Textur-Auswahl die
// unsichtbare Hitbox-Textur, obwohl der Block korrekt als Kreuz erkannt wird.
pub(crate) fn is_degenerate_decoration(e: &Element) -> bool {
    e.rotation.is_none() && (0..3).any(|i| e.from[i] == e.to[i])
}

#[derive(Deserialize, Clone)]
pub(crate) struct Face {
    pub(crate) texture: String,
    // Vorhanden = diese Flaeche wird biomabhaengig eingefaerbt. Bei Bloecken, deren
    // Modell dieselbe Richtung zweimal deklariert (z.B. grass_block: Basis-Element ohne
    // tintindex + zweites Overlay-Element MIT tintindex, s. resolve_block_faces), markiert
    // das die zweite Deklaration als eigene, ueberlagerte Faerbungs-Ebene statt die erste
    // (ungetoente) Textur einfach zu ueberschreiben.
    #[serde(default)]
    pub(crate) tintindex: Option<i64>,
    // Texturausschnitt (0..16 Pixel-Einheiten) INNERHALB der referenzierten Textur --
    // fehlt er, gilt implizit die volle Kachel (0,0,16,16). Sehr haeufig bei Nicht-
    // Vollwuerfel-Formen (Stufen, Treppen, Falllaub, ...), damit die Textur nicht ueber
    // die kleinere Flaeche gestreckt wird. Wird nur fuer die Geometrie (assets::shapes::
    // ElementBox::uv) gebraucht, nicht fuer die Atlas-Texturauswahl selbst.
    #[serde(default)]
    pub(crate) uv: Option<[f32; 4]>,
    // Drehung des Texturausschnitts auf der Flaeche (0/90/180/270), z.B. beim Deckenfeuer.
    #[serde(default)]
    pub(crate) rotation: Option<i32>,
}

// "minecraft:block/stone" / "block/stone" -> "block/stone" (Praefix weg, Namespace weg).
pub(crate) fn strip_ns(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}

// Pfad einer Ressource im Pack-Stapel: `kind` ist der Ordner unter assets/<ns>/ ("models",
// "textures", "items"), `id` eine Ressourcen-ID mit oder ohne Namespace. "block/stone" und
// "minecraft:block/stone" liegen unter minecraft/, "uhc:item/storm_wall" unter uhc/ --
// Server-Packs bringen ihre Modelle und Texturen in eigenen Namespaces mit.
pub fn resource_path(kind: &str, id: &str, ext: &str) -> String {
    match id.split_once(':') {
        Some((ns, pfad)) => format!("{ns}/{kind}/{pfad}{ext}"),
        None => format!("minecraft/{kind}/{id}{ext}"),
    }
}

// Aus dem Blockstate-JSON den Modellnamen samt x/y-Rotation der ersten/Default-Variante
// ziehen. Modellrefs stehen entweder direkt (`{model: ...}`) oder als Liste von
// Zufallsvarianten (dann die erste). Bei Multipart die erste `apply`. Welche konkrete
// Variante/welcher Property-Zustand das ist, ist fuers Texturbild fast immer egal (fast
// alle Bloecke nutzen unabhaengig vom State dieselben paar Texturen) -- nur die Rotation
// muss stimmen, sonst landet z.B. bei einem gedrehten Stamm die Rinden- statt der
// Ring-Textur auf der "oben"-Flaeche.
fn first_model(bs: &BlockstateJson) -> Option<(String, i32, i32)> {
    let pick = |v: &serde_json::Value| -> Option<(String, i32, i32)> {
        let obj = match v {
            serde_json::Value::Array(a) => a.first()?,
            other => other,
        };
        let model = obj.get("model").and_then(|m| m.as_str())?;
        let x = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let y = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        Some((strip_ns(model).to_string(), x, y))
    };
    if let Some(v) = bs.variants.values().next() {
        return pick(v);
    }
    bs.multipart.first().and_then(|c| pick(&c.apply))
}

// ALLE Eintraege derselben Selektor-Variante wie first_model() (dieselbe BTreeMap-
// Erst-Eintrag-Konvention, s. first_model-Kommentar), statt nur des ersten -- fuer Vanillas
// pseudozufaellige Block-Rotation (Erde/Sand/Stein/Grasblock/Podzol/Bedrock/...). None, wenn
// der Selektor kein Array ist oder nur einen Eintrag hat (die weit ueberwiegende Mehrheit
// aller Bloecke). Bewusst NAMEN-basiert (keine Property-Pruefung wie world::render_rng
// braucht) -- reicht hier, weil first_model() denselben Kompromiss schon fuer die normale
// Texturaufloesung macht; der Aufrufer (BlockAtlas::build) kombiniert das Ergebnis mit der
// echten, State-bewussten Auswahl aus dem world-Crate fuer die eigentliche Gating-
// Entscheidung (welcher STATE tatsaechlich rotiert, z.B. grass_block mit snowy=true nicht).
fn all_variant_entries(bs: &BlockstateJson) -> Option<Vec<(i32, i32, bool)>> {
    let serde_json::Value::Array(entries) = bs.variants.values().next()? else { return None };
    if entries.len() <= 1 {
        return None;
    }
    Some(
        entries
            .iter()
            .map(|obj| {
                let model = obj.get("model").and_then(|m| m.as_str()).unwrap_or("");
                let mirrored = strip_ns(model).ends_with("_mirrored");
                let x = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let y = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                (x, y, mirrored)
            })
            .collect(),
    )
}

// Pixel-Kachel um 90 Grad im Uhrzeigersinn gedreht (reiner Byte-Kopiervorgang, sRGB-Bytes
// unangetastet -- wie bei downsample_tile keine physikalisch korrekte Verrechnung noetig,
// nur eine Umsortierung). Ungetestet gegen echte Vanilla-Screenshots (die exakte
// Drehrichtung ist ohne Referenzbild nicht verifizierbar) -- Ziel ist echte, plausible
// Streuung zwischen Blockpositionen, keine pixelgenaue Nachbildung einer bestimmten Richtung.
fn rotate_tile_90(tile: &[u8], tile_size: u32) -> Vec<u8> {
    let n = tile_size as usize;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (sx, sy) = (y, n - 1 - x);
            let src = (sy * n + sx) * 4;
            let dst = (y * n + x) * 4;
            out[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
        }
    }
    out
}

// Horizontale Spiegelung derselben Pixel-Kachel (Gegenstueck zu mirror_face_uvs in main.rs,
// hier aber echte Pixel statt nur eines UV-Rechtecks -- noetig fuer Oben/Unten, s.
// rng_face_uvs-Feldkommentar).
fn mirror_tile(tile: &[u8], tile_size: u32) -> Vec<u8> {
    let n = tile_size as usize;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let src = (y * n + (n - 1 - x)) * 4;
            let dst = (y * n + x) * 4;
            out[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
        }
    }
    out
}

// Modell + parent-Kette laden. Liefert die gemergte textures-Map (Kind ueberschreibt
// Elternteil) und die naechstgelegene elements-Liste (Kind gewinnt, elements werden NICHT
// gemergt). Tiefe begrenzt, um zyklische/kaputte parent-Ketten abzufangen.
// Ob ein Modell Vanillas Ambient Occlusion benutzt: der erste Eintrag in der Parent-Kette
// gewinnt, fehlt er ueberall, gilt true (JsonUnbakedModel.useAmbientOcclusion).
pub(crate) fn model_uses_ambient_occlusion(mc: &mut MinecraftInstallation, model: &str) -> bool {
    let mut current = Some(model.to_string());
    for _ in 0..16 {
        let Some(name) = current.take() else { break };
        let Ok(bytes) = mc.load_bytes(&resource_path("models", strip_ns(&name), ".json")) else { break };
        let Ok(m) = serde_json::from_slice::<ModelJson>(&bytes) else { break };
        if let Some(ao) = m.ambientocclusion {
            return ao;
        }
        current = m.parent;
    }
    true
}

pub(crate) fn load_model_chain(
    mc: &mut MinecraftInstallation,
    model: &str,
) -> Result<(HashMap<String, String>, Option<Vec<Element>>), AssetsError> {
    let mut textures: HashMap<String, String> = HashMap::new();
    let mut elements: Option<Vec<Element>> = None;
    let mut current = Some(model.to_string());
    let mut depth = 0;

    while let Some(name) = current.take() {
        depth += 1;
        if depth > 16 {
            break;
        }
        let path = resource_path("models", strip_ns(&name), ".json");
        let bytes = match mc.load_bytes(&path) {
            Ok(b) => b,
            Err(_) => break, // fehlendes Modell -> mit dem, was wir haben, weitermachen
        };
        let m: ModelJson = serde_json::from_slice(&bytes)?;
        // Kind zuerst gesetzt -> Elternwerte nur einfuegen, wenn der Schluessel noch fehlt.
        for (k, v) in m.textures {
            textures.entry(k).or_insert(v);
        }
        if elements.is_none() {
            elements = m.elements;
        }
        current = m.parent;
    }
    Ok((textures, elements))
}

// EINEN display-Eintrag ("thirdperson_righthand", "gui", ...) aus der parent-Kette holen.
//
// Vanilla erbt display PRO EINTRAG, nicht als Block (JsonUnbakedModel.getTransformation:
// `parent != null && !transformations.isDefined(ctx) ? parent... : eigene`). Ein Item-Modell,
// das nur `gui` ueberschreibt, behaelt die Hand-Haltung seines Elternteils -- deshalb hier
// der erste Vorfahre, der DIESEN Schluessel setzt, statt des ersten mit irgendeinem display.
pub(crate) fn load_model_display(mc: &mut MinecraftInstallation, model: &str, key: &str) -> Option<crate::ui_atlas::ItemDisplay> {
    let mut current = Some(model.to_string());
    let mut depth = 0;
    while let Some(name) = current.take() {
        depth += 1;
        if depth > 16 {
            break;
        }
        let path = resource_path("models", strip_ns(&name), ".json");
        let bytes = mc.load_bytes(&path).ok()?;
        let m: ModelJson = serde_json::from_slice(&bytes).ok()?;
        // Fehlt einer Datei der Linkshand-Eintrag, gilt schon beim Lesen DIESER Datei ihr
        // Rechtshand-Eintrag (ItemTransforms-Deserializer) -- nicht erst der des Elternmodells.
        let rechts = key.strip_suffix("_lefthand").map(|k| format!("{k}_righthand"));
        let eintrag = m.display.as_ref().and_then(|d| d.get(key).or_else(|| rechts.as_deref().and_then(|r| d.get(r))));
        if let Some(entry) = eintrag {
            return Some(parse_display_entry(entry));
        }
        current = m.parent;
    }
    None
}

// Transformation$Deserializer: translation mal 1/16 und auf +-5 geklemmt, scale auf +-4.
// Fehlende Felder behalten ihren Standard (Drehung/Verschiebung 0, Groesse 1).
fn parse_display_entry(entry: &serde_json::Value) -> crate::ui_atlas::ItemDisplay {
    let vector = |key: &str, fallback: [f32; 3]| -> [f32; 3] {
        let Some(array) = entry.get(key).and_then(|v| v.as_array()) else { return fallback };
        if array.len() != 3 {
            return fallback;
        }
        let mut out = fallback;
        for (slot, value) in out.iter_mut().zip(array) {
            let Some(number) = value.as_f64() else { return fallback };
            *slot = number as f32;
        }
        out
    };
    let mut translation = vector("translation", [0.0; 3]);
    for value in &mut translation {
        *value = (*value * 0.0625).clamp(-5.0, 5.0);
    }
    let mut scale = vector("scale", [1.0; 3]);
    for value in &mut scale {
        *value = value.clamp(-4.0, 4.0);
    }
    crate::ui_atlas::ItemDisplay { rotation: vector("rotation", [0.0; 3]), translation, scale }
}

#[cfg(test)]
mod display_entry_tests {
    use super::*;

    // Die Klemm- und Skalierregeln sind Vanillas, nicht ausgedacht: translation kommt in
    // Modellpixeln und wird durch 16 geteilt, danach auf +-5 BLOECKE geklemmt.
    #[test]
    fn display_eintrag_folgt_vanillas_regeln() {
        let json = serde_json::json!({ "rotation": [0, -90, 55], "translation": [0, 4, 0.5], "scale": [0.85, 0.85, 0.85] });
        let d = parse_display_entry(&json);
        assert_eq!(d.rotation, [0.0, -90.0, 55.0], "Drehung bleibt in Grad, unveraendert");
        assert_eq!(d.translation, [0.0, 0.25, 0.03125], "Verschiebung in Bloecken (Pixel/16)");
        assert_eq!(d.scale, [0.85; 3]);
    }

    #[test]
    fn display_eintrag_klemmt_ausreisser() {
        let json = serde_json::json!({ "translation": [1000, 0, 0], "scale": [99, 1, 1] });
        let d = parse_display_entry(&json);
        assert_eq!(d.translation[0], 5.0, "Vanillas MAX_TRANSLATION");
        assert_eq!(d.scale[0], 4.0, "Vanillas MAX_SCALE");
    }

    // Fehlende Felder duerfen nicht auf 0 fallen -- ein scale von 0 machte das Item unsichtbar.
    #[test]
    fn fehlende_felder_behalten_ihren_standard() {
        let d = parse_display_entry(&serde_json::json!({}));
        assert_eq!(d.scale, [1.0; 3]);
        assert_eq!(d.rotation, [0.0; 3]);
        assert_eq!(d.translation, [0.0; 3]);
    }
}

// Einen Texturverweis ("#side", "#all", "block/stone") ueber die textures-Map aufloesen,
// bis ein konkreter Pfad ("block/xxx") uebrig bleibt. Gibt None bei unaufloesbarem/
// fehlendem Verweis.
pub(crate) fn resolve_var(textures: &HashMap<String, String>, mut var: String) -> Option<String> {
    for _ in 0..16 {
        if let Some(rest) = var.strip_prefix('#') {
            var = textures.get(rest)?.clone();
        } else {
            return Some(strip_ns(&var).to_string());
        }
    }
    None
}

// Die sechs Flaechen-Texturpfade eines Blocks (Index FACE_*). Braucht KEINEN Voll-Wuerfel
// mehr: sammelt die Texturreferenzen aus ALLEN Elementen des Modells ein (das erste
// Element, das eine Richtung deklariert, gewinnt) -- ein Stufen- oder Zaun-Modell hat nie
// ein einzelnes Voll-Wuerfel-Element, aber seine Elemente tragen trotzdem die echten
// Top/Seite/Boden-Texturen. Elemente mit eigener Rotation (Kreuz-Ebenen von Pflanzen)
// werden uebersprungen -- fuer die ist keine sinnvolle Richtungszuordnung moeglich, die
// Partikeltextur (Aufrufer-Fallback) genuegt dort. Die Blockstate-Rotation der von
// first_model() gewaehlten (meist ersten) Variante wird bewusst NICHT angewendet -- das
// Ergebnis hier ist die KANONISCHE (ungedrehte) Zuordnung fuer den ganzen Blocknamen, ein
// Aufruf pro Name genuegt (dieselben Texturen fuer alle States). Die tatsaechliche
// Rotation der einzelnen States (z.B. Holzstamm-"axis", Ofen-"facing") liefert separat
// assets::shapes::build_rotations, angewendet vom Aufrufer NACH dem Atlas-Bau (main.rs
// baut daraus BlockUvs::per_state) -- sonst wuerde first_model()s zufaellig gewaehlte
// Variante fuer ALLE States derselben Blockart gelten (Holzstaemme zeigten dann
// immer in eine Richtung).
// Zweites Rueckgabe-Element: pro Flaeche die Overlay-Textur, falls das Modell dieselbe
// Richtung ein zweites Mal MIT tintindex UND auf EXAKT derselben Box deklariert (s.
// Face::tintindex-Kommentar) -- z.B. grass_block: Seiten-Basis (#side, ungetoent) + Seiten-
// Overlay (#overlay, getoent), beide auf der vollen 0..16-Box. Wird beim Atlas-Bau in die
// Basistextur hineingerechnet (composite_overlay), weil beide deckungsgleich sind -- eine
// zweite Geometrie-Ebene dafuer wuerde z-fighten (s. dortigen Kommentar).
// Drittes Rueckgabe-Element: pro Flaeche die Inhaltstextur, falls die tintindex-Deklaration
// auf einer ANDEREN Box liegt als die, die diese Richtung zuerst deklariert hat -- z.B.
// Kesselinhalt (Wasser/Lava/Pulverschnee): ein eigenes, kleineres Element mit nur einer
// Oben-Flaeche, geometrisch echt getrennt vom Kesselrand. Bekommt eine EIGENE Atlas-Kachel
// (kein Compositing -- die Box ist ja schon als eigenes ElementBox/ShapeBox vorhanden, s.
// assets::shapes::ElementBox::content), damit sie nicht die Randtextur ueberschreibt.
// None (beide Rueckgabe-Elemente) fuer Bloecke ohne diese Technik (die grosse Mehrheit).
pub(crate) fn resolve_block_faces(
    mc: &mut MinecraftInstallation,
    block: &str,
) -> Option<([String; 6], [Option<String>; 6], [Option<String>; 6])> {
    let bs_bytes = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")).ok()?;
    let bs: BlockstateJson = serde_json::from_slice(&bs_bytes).ok()?;
    let (model, _x_deg, _y_deg) = first_model(&bs)?;
    resolve_model_faces(mc, &model)
}

// Die Flaechentexturen EINES Blockmodells (s. resolve_block_faces) -- auch fuer die weiteren
// Varianten eines Blocks, deren Modell andere Texturen nennt (s. BlockAtlas::variant_uvs).
pub(crate) fn resolve_model_faces(
    mc: &mut MinecraftInstallation,
    model: &str,
) -> Option<([String; 6], [Option<String>; 6], [Option<String>; 6])> {
    let (textures, elements) = load_model_chain(mc, model).ok()?;
    let elements = elements?;

    // s. is_degenerate_decoration-Kommentar: bei einem reinen Kreuz-Modell (alle "echten"
    // Elemente rotiert, z.B. cobweb.json) sind die rotierten Kreuz-Ebenen die tatsaechlich
    // sichtbare Textur -- unten wird sonst IMMER nur aus unrotierten Elementen gelesen, was
    // bei einem Pack mit zusaetzlichen unrotierten Hitbox-Deko-Elementen faelschlich deren
    // (meist unsichtbare) Textur waehlen wuerde statt der echten Kreuz-Textur.
    let real: Vec<&Element> = elements.iter().filter(|e| !is_degenerate_decoration(e)).collect();
    let is_cross_model = !real.is_empty() && real.iter().all(|e| e.rotation.is_some());

    let mut faces: [Option<String>; 6] = Default::default();
    let mut overlay: [Option<String>; 6] = Default::default();
    let mut content: [Option<String>; 6] = Default::default();
    let mut first_box_for_face: [Option<([f32; 3], [f32; 3])>; 6] = Default::default();
    for e in &elements {
        if e.rotation.is_some() != is_cross_model {
            continue;
        }
        let mut element_faces: [Option<&Face>; 6] = Default::default();
        for (i, face_name) in FACE_NAMES.iter().enumerate() {
            element_faces[i] = e.faces.get(*face_name);
        }
        for i in 0..6 {
            let Some(face) = element_faces[i] else { continue };
            if faces[i].is_none() {
                faces[i] = resolve_var(&textures, face.texture.clone());
                first_box_for_face[i] = Some((e.from, e.to));
            } else if face.tintindex.is_some() {
                if first_box_for_face[i] == Some((e.from, e.to)) {
                    overlay[i] = resolve_var(&textures, face.texture.clone());
                } else {
                    content[i] = resolve_var(&textures, face.texture.clone());
                }
            }
        }
    }
    // Kreuz-Modell mit zusaetzlichen unrotierten Deko-/Hitbox-Elementen (s.
    // is_degenerate_decoration-Kommentar, z.B. cobweb.json eines PvP-Packs): deren eigene
    // Textur (z.B. "hitbox_thin") geht oben verloren, weil die Hauptschleife bei einem
    // Kreuz-Modell ausschliesslich die rotierten Ebenen liest. Genau wie Vanilla ALLE
    // Elemente eines Custom-Modells rendert (nicht nur die geometrisch als Kreuz
    // darstellbaren), wird diese Zusatztextur hier ueber denselben "content"-Mechanismus
    // aufgeloest, den Kesselinhalt schon nutzt (s. assets::shapes::build_cross_extras fuer
    // die zugehoerige Geometrie) -- kein Block braucht dafuer eine dritte, eigene
    // Texturebene.
    if is_cross_model {
        for e in &elements {
            if e.rotation.is_some() {
                continue;
            }
            for (i, face_name) in FACE_NAMES.iter().enumerate() {
                if content[i].is_none() {
                    if let Some(face) = e.faces.get(*face_name) {
                        content[i] = resolve_var(&textures, face.texture.clone());
                    }
                }
            }
        }
    }

    // Fehlende Einzelflaechen mit irgendeiner vorhandenen fuellen (z.B. Modelle, die nur
    // "all" ueber die textures-Map, aber keine expliziten Face-Eintraege haben).
    let any = faces.iter().flatten().next().cloned()?;
    let faces = std::array::from_fn(|i| faces[i].clone().unwrap_or_else(|| any.clone()));
    Some((faces, overlay, content))
}

// Partikel-/Notfalltextur eines Blocks: aufgeloeste "particle"-Var, sonst irgendeine
// Textur aus der Modellkette. None -> Aufrufer nimmt die Missing-Kachel.
fn resolve_particle(mc: &mut MinecraftInstallation, block: &str) -> Option<String> {
    let bs_bytes = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")).ok()?;
    let bs: BlockstateJson = serde_json::from_slice(&bs_bytes).ok()?;
    let (model, _, _) = first_model(&bs)?;
    let (textures, _) = load_model_chain(mc, &model).ok()?;
    if let Some(p) = textures.get("particle").cloned() {
        if let Some(r) = resolve_var(&textures, p) {
            return Some(r);
        }
    }
    // irgendeine konkrete Textur aus der Map
    textures.values().find_map(|v| resolve_var(&textures, v.clone()))
}

// Eine `native x native`-Pixelkachel (Bytes bereits als eigenstaendiger Ausschnitt uebergeben,
// Zeile fuer Zeile) per Nearest-Neighbor auf `tile_size x tile_size` skalieren -- fuer
// `native == tile_size` (der bisherige, haeufigste Fall: Vanilla 16x16-Textur im 16px-Atlas)
// ist das eine reine 1:1-Kopie (sx==x, sy==y bei jedem Pixel), aendert also am bisherigen
// Ergebnis nichts. Nearest statt bilinear: Pixel-Art soll beim Hochskalieren scharf/blockig
// bleiben, nicht verwaschen.
fn nearest_scale_tile(native_rgba: &[u8], native: u32, tile_size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tile_size * tile_size * 4) as usize];
    for y in 0..tile_size {
        for x in 0..tile_size {
            let sx = (x * native / tile_size).min(native - 1);
            let sy = (y * native / tile_size).min(native - 1);
            let src = ((sy * native + sx) * 4) as usize;
            let dst = ((y * tile_size + x) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&native_rgba[src..src + 4]);
        }
    }
    out
}

// Groesste tatsaechlich vorkommende Texturbreite unter allen benoetigten Block-Texturen --
// naechste Zweierpotenz, min. 16 (nie kleiner als Vanilla), gedeckelt bei 256 (gegen einen
// einzelnen versehentlich riesigen Ausreisser, der den ganzen Atlas aufblaehen wuerde).
// EINE globale Groesse fuers gesamte Atlas-Raster statt Rechteck-Packing -- haelt die
// Packing-/UV-Mathematik unveraendert (s. BlockAtlas::tile_size-Feldkommentar), auf Kosten
// eines zweiten Lade-Durchgangs hier (Texturen werden gleich danach fuers eigentliche
// Kachel-Laden nochmal geoeffnet -- vertretbar, das sind ein paar hundert kleine PNGs,
// einmalig beim Start).
fn determine_tile_size(mc: &mut MinecraftInstallation, needed: &[(String, TextureSource)]) -> u32 {
    let mut max_width = 16u32;
    for (_, source) in needed {
        let TextureSource::Plain(path) = source;
        if let Ok(tex) = mc.load_texture(&resource_path("textures", path, ".png")) {
            max_width = max_width.max(tex.width);
        }
    }
    max_width.next_power_of_two().clamp(16, 256)
}

// Ein RGBA-Kachelbild (tile_size x tile_size) aus einer Texturdatei laden, hochskaliert falls
// die echte Textur kleiner ist (s. nearest_scale_tile). Animierte Texturen sind vertikale
// Streifen (Hoehe = Vielfaches der Breite) -- davon hier nur der erste Frame (das oberste
// `Breite x Breite`-Quadrat), die weiteren tauscht TileAnimation zur Laufzeit ein.
fn load_tile(mc: &mut MinecraftInstallation, texture_path: &str, tile_size: u32) -> Option<Vec<u8>> {
    let tex = mc.load_texture(&resource_path("textures", texture_path, ".png")).ok()?;
    let native = tex.width;
    if native == 0 || tex.height < native {
        return None;
    }
    let frame = &tex.rgba[0..(native * native * 4) as usize];
    Some(nearest_scale_tile(frame, native, tile_size))
}

// Wie load_tile, aber ALLE Animationsframes einer vertikal gestapelten Textur (Breite =
// Kachelbreite, Hoehe = Vielfaches davon, z.B. water_still.png: 16x512 = 32 Frames a 16x16,
// s. water_still.png.mcmeta "frametime":2) statt nur des ersten -- fuer Vanillas
// Texturanimation, die bisher komplett fehlte (s. Modulkommentar "nur der erste Frame" als
// bisherige MVP-Luecke). Nur fuer Wasser
// gebraucht (s. BlockAtlas::build-Aufrufer) -- alle anderen animierten Texturen laufen ueber
// TileAnimation.
fn load_tile_frames(mc: &mut MinecraftInstallation, texture_path: &str, tile_size: u32) -> Option<Vec<Vec<u8>>> {
    let tex = mc.load_texture(&resource_path("textures", texture_path, ".png")).ok()?;
    let native = tex.width;
    if native == 0 || tex.height < native {
        return None;
    }
    let frame_count = tex.height / native;
    let frame_bytes = (native * native * 4) as usize;
    Some(
        (0..frame_count)
            .map(|f| {
                let start = f as usize * frame_bytes;
                nearest_scale_tile(&tex.rgba[start..start + frame_bytes], native, tile_size)
            })
            .collect(),
    )
}

/// Eine animierte Atlas-Kachel nach Vanillas SpriteContents.Animation: die Textur ist ein
/// senkrechter Streifen quadratischer Frames, die .mcmeta nennt Reihenfolge und Dauer (in
/// Spielticks) und ob zwischen zwei Frames weich ueberblendet wird ("interpolate", z.B.
/// Prismarin und Magma). Der Aufrufer laedt den jeweils aktuellen Stand in die Kachel an
/// `origin` hoch -- Geometrie und UVs bleiben, nur die Pixel wechseln.
///
/// Die Frames bleiben in NATIVER Groesse gespeichert und werden erst beim Hochladen auf die
/// Kachelgroesse gebracht: mit einem HD-Pack (256er-Kacheln) kaemen sonst leicht einige hundert
/// MB vorab skalierter Frames zusammen.
pub struct TileAnimation {
    /// Pixel (Mip-Stufe 0, oben links) der Kachel im Atlas.
    pub origin: (u32, u32),
    pub tile_size: u32,
    mip_levels: u32,
    native: u32,
    strip: Vec<u8>,
    frame_count: u32,
    /// (Frame, Dauer in Ticks) in Abspielreihenfolge.
    sequence: Vec<(u32, u32)>,
    interpolate: bool,
    // Versatz in Ticks gegenueber der gemeinsamen Uhr und Tempo in RATE_UNIT-teln (RATE_UNIT =
    // wie Vanilla) -- beides nur bei den zeitversetzten Kopien (s. FIRE_VARIANTS, ANIM_VARIANTS)
    // anders, die sonst alle im selben Tick und im selben Takt das Bild wechselten.
    tick_offset: u64,
    rate: u64,
    /// Texturpfad ("block/fire_0"), fuer Tests und Fehlersuche.
    pub path: String,
}

/// Was gerade zu sehen ist. Ohne Interpolation ist `sub` immer 0 -- dann aendert sich der
/// Zustand nur beim Framewechsel, und nur dann muss hochgeladen werden.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AnimationState {
    frame: u32,
    next: u32,
    sub: u32,
    ticks: u32,
}

impl AnimationState {
    /// Zwischen zwei Frames gemischt? Solche Staende wiederholen sich kaum (jeder Tick ein
    /// anderer), ein Zwischenspeicher lohnt sich fuer sie nicht.
    pub fn is_interpolated(&self) -> bool {
        self.sub != 0
    }

    /// Welcher Frame gerade gezeigt wird (ohne Ueberblendung).
    pub fn frame_index(&self) -> u32 {
        self.frame
    }
}

impl TileAnimation {
    /// None, wenn die Textur keine Animation hat (keine .mcmeta, nur ein Frame, ...).
    pub fn load(mc: &mut MinecraftInstallation, path: &str, origin: (u32, u32), tile_size: u32, mip_levels: u32) -> Option<Self> {
        let meta: serde_json::Value = serde_json::from_slice(&mc.load_bytes(&resource_path("textures", path, ".png.mcmeta")).ok()?).ok()?;
        let animation = meta.get("animation")?;
        let tex = mc.load_texture(&resource_path("textures", path, ".png")).ok()?;
        let native = tex.width;
        if native == 0 || tex.height < native * 2 {
            return None;
        }
        let frame_count = tex.height / native;
        let frametime = animation.get("frametime").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as u32;
        let sequence: Vec<(u32, u32)> = match animation.get("frames").and_then(|f| f.as_array()) {
            Some(frames) => frames
                .iter()
                .filter_map(|f| match f {
                    serde_json::Value::Number(n) => Some((n.as_u64()? as u32, frametime)),
                    serde_json::Value::Object(o) => {
                        let index = o.get("index")?.as_u64()? as u32;
                        let time = o.get("time").and_then(|t| t.as_u64()).map_or(frametime, |t| t.max(1) as u32);
                        Some((index, time))
                    }
                    _ => None,
                })
                .filter(|(index, _)| *index < frame_count)
                .collect(),
            None => (0..frame_count).map(|f| (f, frametime)).collect(),
        };
        if sequence.len() < 2 {
            return None;
        }
        let interpolate = animation.get("interpolate").and_then(|v| v.as_bool()).unwrap_or(false);
        let strip = tex.rgba[..(native * native * 4 * frame_count) as usize].to_vec();
        Some(Self { origin, tile_size, mip_levels, native, strip, frame_count, sequence, interpolate, tick_offset: 0, rate: RATE_UNIT, path: path.to_string() })
    }

    /// Stand zum Spieltick `tick` (20 pro Sekunde, laeuft einfach durch).
    /// Gerechnet wird in RATE_UNIT-teln eines Ticks: so laeuft eine Kopie mit anderem Tempo
    /// (s. variant_timing) langsamer oder schneller, und ihre Ueberblendung bleibt trotzdem weich.
    pub fn state_at(&self, tick: u64) -> AnimationState {
        let total: u64 = self.sequence.iter().map(|&(_, t)| t as u64 * RATE_UNIT).sum();
        let mut rest = (tick * self.rate + self.tick_offset * RATE_UNIT) % total;
        for (i, &(frame, ticks)) in self.sequence.iter().enumerate() {
            let ticks = ticks * RATE_UNIT as u32;
            if rest < ticks as u64 {
                let next = self.sequence[(i + 1) % self.sequence.len()].0;
                let sub = if self.interpolate && frame != next { rest as u32 } else { 0 };
                return AnimationState { frame, next, sub, ticks };
            }
            rest -= ticks as u64;
        }
        AnimationState { frame: self.sequence[0].0, next: self.sequence[0].0, sub: 0, ticks: 1 }
    }

    /// Alle Mip-Stufen der Kachel fuer `state`, Stufe 0 zuerst.
    pub fn render(&self, state: AnimationState) -> Vec<Vec<u8>> {
        let frame_bytes = (self.native * self.native * 4) as usize;
        let frame = |f: u32| {
            let f = f.min(self.frame_count - 1) as usize;
            &self.strip[f * frame_bytes..(f + 1) * frame_bytes]
        };
        let pixels = if state.sub == 0 {
            frame(state.frame).to_vec()
        } else {
            // Vanilla (Animation.Interpolation): Farbe linear zwischen aktuellem und naechstem
            // Frame, Anteil des naechsten = sub / Dauer. Alpha bleibt beim aktuellen Frame.
            let t = state.sub as f32 / state.ticks as f32;
            let (a, b) = (frame(state.frame), frame(state.next));
            let mut out = a.to_vec();
            for i in (0..out.len()).step_by(4) {
                for c in 0..3 {
                    out[i + c] = (a[i + c] as f32 * (1.0 - t) + b[i + c] as f32 * t).round() as u8;
                }
            }
            out
        };
        build_tile_mips(&nearest_scale_tile(&pixels, self.native, self.tile_size), self.tile_size, self.mip_levels)
    }
}

// Woher die Pixel eines Atlas-Tiles kommen -- bisher immer eine einzelne Texturdatei.
// Overlay-Flaechen (z.B. grass_block-Seiten: Basis-Element ohne tintindex + Overlay-
// Element MIT tintindex auf EXAKT derselben Box, s. resolve_block_faces) bekamen frueher
// hier eine fest getoente Kachel fest in die Basistextur eingerechnet (GRASS_SIDE_TINT) --
// dadurch war die Seiten-Faerbung ein globaler fester Ton statt pro Biom. Jetzt bleibt das
// Overlay eine EIGENE, UNGETOENTE Kachel (wie Kesselinhalt/`content`, s. overlay_uvs-
// Feldkommentar) -- der Mesher zeichnet sie als zweite Geometrie-Ebene mit echter
// Biom-Vertexfarbe ueber die Translucent-Pipeline (echtes Alpha-Blending, `depth_compare:
// LessEqual` verhindert Z-Fighting mit der deckungsgleichen Basis-Flaeche darunter, s.
// render::BlockRenderer::translucent_pipeline).
enum TextureSource {
    Plain(String),
}

fn intern(key: String, source: TextureSource, needed: &mut Vec<(String, TextureSource)>) -> String {
    if !needed.iter().any(|(k, _)| k == &key) {
        needed.push((key.clone(), source));
    }
    key
}

// NUR die eingefaerbten Glas-Bloecke UND tinted_glass: per Datei-Check (nicht geraten,
// s. Kommentar unten) sind deren Texturen komplett OPAK (Alpha=255 ueberall) -- die
// Durchsichtigkeit kommt in Vanilla ausschliesslich aus einem eigenen "translucent"-
// Renderpass, den dieser Client nicht hat (bewusste MVP-Entscheidung, kein echtes
// Alpha-Blending, s. block.wgsl). Klarglas ("glass") und ALLE Glasscheiben ("*_pane",
// auch eingefaerbte) sind davon ausgenommen: deren Texturen haben schon von Vanilla aus
// ein echtes Alpha-Cutout-Muster (Rahmen opak, Mitte transparent) -- per
// `python3 -c "...PIL..."` gegen die echte 1.21.11.jar geprueft, nicht geraten (eine
// fruehere Version dieser Funktion hatte genau das geraten und lag falsch: sie hat
// Klarglas/Scheiben zusaetzlich durchloechert und ihr sauberes Rahmenmuster kaputt
// gemacht -- das sah dann wie verrauschter Schnee statt Glas aus). Fuer die bereits
// korrekten Texturen genuegt der vorhandene Cutout (Alpha<0.5 verwerfen) unveraendert.
fn is_translucent_glass(name: &str) -> bool {
    name == "tinted_glass" || name.ends_with("_stained_glass")
}

// Vanillas "Grafik: Schnell"-Verhalten fuer Laub: die echten Laubtexturen tragen ein
// Alpha-Kanal mit durchsichtigen Luecken (das "Fancy"-Look-Durchsehen). Das kostet Leistung
// (Alpha-Test/discard() im Fragment-Shader, s. block.wgsl) fuer wenig sichtbaren Gewinn, vor
// allem bei grossen Baumkronen. Auf Nutzerwunsch fest deaktiviert (nicht nur ein Toggle) --
// Alpha wird beim Atlas-Bau hart auf voll gesetzt, GENAU wie is_translucent_glass/
// dither_translucent dieselbe Kachel VOR dem Kopieren in den Atlas nachbearbeiten.
fn is_leaves(name: &str) -> bool {
    name.ends_with("_leaves")
}

// Feste Textur fuer Bloecke ohne echte Modell-Geometrie, deren Aussehen NICHT vom
// "particle"-Modellfeld abhaengen darf (s. Aufrufer-Kommentar) -- bewusst nur Wasser/Lava
// (die einzigen beiden, die dieser Client ueberhaupt aus dem Atlas texturiert). Beide gehen
// inzwischen durch dieselbe Fluid-Geometrie (s. world::mesher::push_fluid_quads), brauchen
// also beide die richtige, resourcepack-stabile Textur statt eines Zufallstreffers ueber
// "particle".
// Vanillas Fliesstextur fuer Wasser -- die schraeg gestreifte Variante, die auf bewegten
// Zellen liegt (water_still ist die ruhige). Fest verdrahtet aus demselben Grund wie
// fixed_fluid_texture: sie darf nicht ueber einen Modell-/Partikel-Umweg aufgeloest werden.
// Wie weit jede Atlaskachel nach innen rueckt, in Atlaspixeln (s. uv_of in BlockAtlas::build).
// Rundungsreste der UV-Interpolation liegen um Groessenordnungen darunter; ein 32stel Texel
// verschiebt die Texelgrenzen dagegen nicht sichtbar.
const TILE_UV_INSET_PX: f32 = 1.0 / 32.0;

const WATER_FLOW_TEXTURE: &str = "block/water_flow";

// Feuer: Vanilla hat je Feuertextur EINE Atlas-Kachel, alle Feuerbloecke mit derselben Textur
// wechseln also im selben Tick das Bild -- ein Feuerfeld flackert im Gleichschritt. Hier
// bekommt jede Feuertextur FIRE_VARIANTS Kacheln mit verschiedenem Zeitversatz, und der
// Mesher waehlt je Block eine davon (s. texture_variants). Die beiden Texturen eines Paars
// (_0/_1) teilen sich einen Topf -- Vanilla waehlt zwischen ihnen ohnehin zufaellig per
// Blockstate-Variante, hier entscheidet dieselbe Positionswahl mit.
pub const FIRE_TEXTURE_PAIRS: [[&str; 2]; 2] = [["block/fire_0", "block/fire_1"], ["block/soul_fire_0", "block/soul_fire_1"]];
pub const FIRE_VARIANTS: u64 = 4;

/// Ist `path` eine der Feuertexturen (s. FIRE_TEXTURE_PAIRS)?
pub fn is_fire_texture(path: &str) -> bool {
    FIRE_TEXTURE_PAIRS.iter().flatten().any(|p| *p == path)
}

fn fire_variant_key(path: &str, variant: u64) -> String {
    if variant == 0 { path.to_string() } else { format!("{path}\0phase{variant}") }
}

// Tempo-Einheit von TileAnimation: RATE_UNIT = Vanillas Takt.
const RATE_UNIT: u64 = 20;
// Tempi der Kopien 1.. (0,7- bis 1,5-fach), je Textur anders verteilt. Kopie 0 ist die
// Basiskachel, die auch ausserhalb der Welt gezeigt wird -- sie behaelt Vanillas Takt.
const VARIANT_RATES: [u64; 7] = [14, 16, 18, 22, 24, 27, 30];

// Zeitversatz und Tempo einer Kopie bei `total` Ticks je Durchlauf (von `count` Kopien): die
// Kopien starten gleichmaessig ueber den Durchlauf verteilt, dazu ein fester, je Textur anderer
// Rest innerhalb ihres Abschnitts -- damit auch fire_0 und fire_1 nicht im Takt laufen. Weil
// jede Kopie ein eigenes Tempo hat, laufen sie danach immer weiter auseinander und wieder
// zusammen, statt einen festen Abstand zu halten.
fn variant_timing(path: &str, variant: u64, count: u64, total: u64) -> (u64, u64) {
    let salt = path.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
    let step = (total / count).max(1);
    let rate = if variant == 0 { RATE_UNIT } else { VARIANT_RATES[((salt + variant - 1) % VARIANT_RATES.len() as u64) as usize] };
    (variant * step + salt % step, rate)
}

// Alle anderen animierten Texturen bekommen dasselbe wie Feuer: ANIM_VARIANTS Kacheln mit
// Zeitversatz und eigenem Tempo (s. variant_timing), je Block per Position eine davon -- sonst wechseln alle Seegrashalme, alle
// Prismarinbloecke, alle animierten Erze eines Resourcepacks im selben Tick das Bild. Anders
// als beim Feuer waehlt hier nicht die Modell-Aufloesung, sondern der Mesher nachtraeglich
// ueber die Kachel, auf die ein Dreieck zeigt -- so gilt es fuer jede Form (Wuerfel,
// Kreuzmodelle, Modell-Vierecke, Overlays) ohne eigenen Pfad.
//
// Ausgenommen sind die Fluessigkeiten: ihr Muster laeuft ueber Blockgrenzen hinweg und
// zerfiele mit Versatz in einzelne Kacheln. Feuer hat seinen eigenen Topf (FIRE_VARIANTS).
pub const ANIM_VARIANTS: u64 = 8;
const ANIM_SYNCED_TEXTURES: [&str; 4] = ["block/water_still", "block/water_flow", "block/lava_still", "block/lava_flow"];

fn anim_variant_key(path: &str, variant: u64) -> String {
    if variant == 0 { path.to_string() } else { format!("{path}\0anim{variant}") }
}

// Hat die Textur eine .mcmeta mit "animation"? Nur die Datei, ohne das Bild zu laden.
fn has_animation_meta(mc: &mut MinecraftInstallation, path: &str) -> bool {
    mc.load_bytes(&resource_path("textures", path, ".png.mcmeta"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|meta| meta.get("animation").is_some())
}

// Vanillas DRITTE Wassertextur: liegt auf einer Wasser-Seitenflaeche, die an Glas oder Laub
// grenzt (FluidRenderer nimmt sie, wenn der Nachbar ein TranslucentBlock oder LeavesBlock
// ist). Sie ist deutlich flacher/gleichmaessiger als
// water_still -- der Grund, warum Wasser hinter einer Glasscheibe in Vanilla als ruhige blaue
// Flaeche erscheint statt als volle Wasseroberflaeche.
const WATER_OVERLAY_TEXTURE: &str = "block/water_overlay";

// Vanillas Fliesstextur fuer Lava -- das Gegenstueck zu water_flow. Seit der Mesher Lava
// ueber dieselbe Fluid-Geometrie zeichnet wie Wasser (s. world::mesher::push_fluid_quads)
// braucht sie eine eigene Atlas-Kachel: sie haengt an keinem Blocknamen (der Block "lava"
// zeigt weiterhin auf lava_still), welche der beiden eine Zelle bekommt, entscheidet erst
// der Mesher. Ein water_overlay-Gegenstueck gibt es bewusst nicht -- FluidRenderer setzt das
// Overlay-Sprite nur im Nicht-Lava-Zweig (`if (!bl)`).
const LAVA_FLOW_TEXTURE: &str = "block/lava_flow";

// Vanillas zehn Riss-Stufen (destroy_stage_0..9) -- die Textur, die beim Abbauen ueber die
// Flaechen des Blocks gelegt wird (LevelRenderer.renderBlockDestroyAnimation ueber
// ModelBakery.DESTROY_TYPES). Sie haengt an keinem Blocknamen und an keinem Modell, wird
// also -- wie water_flow/lava_flow -- eigenstaendig in den Atlas interniert.
pub const DESTROY_STAGE_COUNT: usize = 10;

fn destroy_stage_texture(stage: usize) -> String {
    format!("block/destroy_stage_{stage}")
}

fn fixed_fluid_texture(block: &str) -> Option<&'static str> {
    match block {
        "water" => Some("block/water_still"),
        "lava" => Some("block/lava_still"),
        _ => None,
    }
}

fn force_opaque(tile: &mut [u8]) {
    for pixel in tile.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
}

impl BlockAtlas {
    // Baut den Atlas fuer alle uebergebenen Blocknamen. Fehlende/unaufloesbare Bloecke
    // bekommen die Missing-Kachel (magenta/schwarz-kariert). `block_names` sollte die im
    // Weltzustand vorkommenden Bloecke enthalten (der Aufrufer nimmt einfach alle).
    pub fn build(mc: &mut MinecraftInstallation, block_names: &[&str]) -> Result<Self, AssetsError> {
        // 1) Fuer jeden Block seine sechs Texturpfade bestimmen. Bei einer Overlay-Ebene
        // (s. TextureSource-Kommentar) ist der "Pfad" ein synthetischer composite_key statt
        // eines echten Dateipfads -- load_tile bekommt ihn nie zu sehen, er dient nur als
        // Cache-Schluessel fuer die vorgerechnete Basis+Overlay-Kachel.
        let mut block_faces: HashMap<String, [String; 6]> = HashMap::new();
        // Inhaltsflaechen (s. resolve_block_faces-Kommentar, z.B. Kesselinhalt): eigene,
        // eigenstaendige Kachel pro Block+Richtung -- getrennt von `block_faces`,
        // damit sie die normale Flaechentextur (z.B. den Kesselrand) nicht ueberschreibt.
        let mut block_content: HashMap<String, [Option<String>; 6]> = HashMap::new();
        // Overlay-Flaechen (s. overlay_uvs-Feldkommentar, z.B. grass_block-Seiten): ebenso
        // eine eigenstaendige, UNGETOENTE Kachel statt in die Basistextur eingerechnet.
        let mut block_overlay: HashMap<String, [Option<String>; 6]> = HashMap::new();
        let mut block_particle: HashMap<String, String> = HashMap::new();
        let mut needed: Vec<(String, TextureSource)> = Vec::new();
        #[allow(unused_mut, unused_variables)]
        let mut translucent_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut opaque_leaf_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        for &block in block_names {
            if block == "air" || block.ends_with("air") {
                continue;
            }
            // Wasser/Lava haben in ihrem Modell KEINE echte Geometrie (water.json/lava.json
            // deklarieren nur ein "particle"-Textur-Feld fuer den generischen Notfall-Pfad
            // unten, s. resolve_particle-Kommentar) -- Vanillas eigener FluidRenderer nutzt
            // dafuer aber NIE die particle-Textur (die ist ausschliesslich fuer Partikel-
            // effekte, z.B. Blockbruch), sondern eine feste Fluid-Sprite-Textur. Der
            // bisherige resolve_particle-Fallback traf das gewuenschte Ergebnis nur
            // zufaellig, weil Vanillas eigene particle-Textur fuer Wasser TOTAL ZUFAELLIG
            // genau "block/water_still" ist -- ein Resourcepack, das NUR die Partikel
            // abschaltet (haeufig bei PvP-Packs: "particle": "block/blank"), bricht diesen
            // Zufall und macht Wasser komplett unsichtbar (transparente Blank-Textur statt
            // der echten Wasseroberflaeche). Deshalb hier fest verdrahtet,
            // bevor der generische Pfad ueberhaupt zum Zug kommt.
            if let Some(fixed) = fixed_fluid_texture(block) {
                let interned = intern(fixed.to_string(), TextureSource::Plain(fixed.to_string()), &mut needed);
                block_faces.insert(block.to_string(), std::array::from_fn(|_| interned.clone()));
                // Wasser braucht ZUSAETZLICH die Fliesstextur im Atlas. Sie haengt an keinem
                // Blocknamen (s. water_flow_uv-Feldkommentar), muss aber trotzdem eine
                // Kachel bekommen -- deshalb hier mit-interniert, wo wir ohnehin wissen,
                // dass Wasser vorkommt.
                if block == "water" {
                    intern(WATER_FLOW_TEXTURE.to_string(), TextureSource::Plain(WATER_FLOW_TEXTURE.to_string()), &mut needed);
                    intern(WATER_OVERLAY_TEXTURE.to_string(), TextureSource::Plain(WATER_OVERLAY_TEXTURE.to_string()), &mut needed);
                }
                // Dasselbe fuer Lava -- aus demselben Grund, nur ohne Overlay-Sprite.
                if block == "lava" {
                    intern(LAVA_FLOW_TEXTURE.to_string(), TextureSource::Plain(LAVA_FLOW_TEXTURE.to_string()), &mut needed);
                }
                continue;
            }
            let translucent = is_translucent_glass(block);
            let leaves = is_leaves(block);
            let particle = resolve_particle(mc, block);
            if let Some(p) = &particle {
                let interned = intern(p.clone(), TextureSource::Plain(p.clone()), &mut needed);
                block_particle.insert(block.to_string(), interned);
            }
            match resolve_block_faces(mc, block) {
                Some((faces, overlay, content)) => {
                    let interned: [String; 6] =
                        std::array::from_fn(|i| intern(faces[i].clone(), TextureSource::Plain(faces[i].clone()), &mut needed));
                    if translucent {
                        translucent_paths.extend(interned.iter().cloned());
                    }
                    if leaves {
                        opaque_leaf_paths.extend(interned.iter().cloned());
                    }
                    block_faces.insert(block.to_string(), interned);
                    if content.iter().any(|c| c.is_some()) {
                        let interned_content: [Option<String>; 6] = std::array::from_fn(|i| {
                            content[i].as_ref().map(|path| intern(path.clone(), TextureSource::Plain(path.clone()), &mut needed))
                        });
                        block_content.insert(block.to_string(), interned_content);
                    }
                    if overlay.iter().any(|o| o.is_some()) {
                        let interned_overlay: [Option<String>; 6] = std::array::from_fn(|i| {
                            overlay[i].as_ref().map(|path| intern(path.clone(), TextureSource::Plain(path.clone()), &mut needed))
                        });
                        block_overlay.insert(block.to_string(), interned_overlay);
                    }
                }
                None => {
                    if let Some(p) = particle {
                        let interned = intern(p.clone(), TextureSource::Plain(p), &mut needed);
                        if translucent {
                            translucent_paths.insert(interned.clone());
                        }
                        if leaves {
                            opaque_leaf_paths.insert(interned.clone());
                        }
                        block_faces.insert(block.to_string(), std::array::from_fn(|_| interned.clone()));
                    }
                }
            }
        }

        // 1.2) Die weiteren Varianten-Modelle jedes Blocks (s. variant_uvs).
        let mut variant_faces: HashMap<String, ([String; 6], [Option<String>; 6])> = HashMap::new();
        for &block in block_names {
            if block == "air" || block.ends_with("air") || fixed_fluid_texture(block).is_some() {
                continue;
            }
            let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
            let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };
            let Some((erstes, _, _)) = first_model(&bs) else { continue };
            for v in bs.variants.values() {
                let obj = match v {
                    serde_json::Value::Array(a) => a.first(),
                    other => Some(other),
                };
                let Some(model) = obj.and_then(|o| o.get("model")).and_then(|m| m.as_str()).map(|m| strip_ns(m).to_string()) else { continue };
                if model == erstes || variant_faces.contains_key(&model) {
                    continue;
                }
                let Some((faces, overlay, _content)) = resolve_model_faces(mc, &model) else { continue };
                let faces = std::array::from_fn(|i| intern(faces[i].clone(), TextureSource::Plain(faces[i].clone()), &mut needed));
                let overlay = std::array::from_fn(|i| overlay[i].as_ref().map(|p| intern(p.clone(), TextureSource::Plain(p.clone()), &mut needed)));
                variant_faces.insert(model, (faces, overlay));
            }
        }

        // 1.3) Die zehn Riss-Stufen (s. DESTROY_STAGE_COUNT). Immer, unabhaengig davon,
        // welche Bloecke vorkommen: abgebaut werden kann jeder.
        for stage in 0..DESTROY_STAGE_COUNT {
            let path = destroy_stage_texture(stage);
            intern(path.clone(), TextureSource::Plain(path), &mut needed);
        }

        // 1.4) Texturen gedrehter Modell-Elemente (Feuer, Laterne, ...), s. texture_uvs.
        for &block in block_names {
            for path in crate::shapes::rotated_model_textures(mc, block) {
                intern(path.clone(), TextureSource::Plain(path), &mut needed);
            }
        }

        // 1.45) Feuer-Kopien mit Zeitversatz (s. FIRE_VARIANTS). Nur fuer Paare, deren Feuer
        // ueberhaupt gebraucht wird; beide Texturen eines Paars, auch wenn das Modell nur eine nennt.
        for pair in FIRE_TEXTURE_PAIRS {
            if !pair.iter().any(|p| needed.iter().any(|(k, _)| k == p)) {
                continue;
            }
            for path in pair {
                for variant in 0..FIRE_VARIANTS {
                    intern(fire_variant_key(path, variant), TextureSource::Plain(path.to_string()), &mut needed);
                }
            }
        }

        // 1.46) Zeitversetzte Kopien aller uebrigen animierten Texturen (s. ANIM_VARIANTS).
        let animated: Vec<String> = needed
            .iter()
            .map(|(k, _)| k.clone())
            .filter(|k| !k.contains('\0') && !is_fire_texture(k) && !ANIM_SYNCED_TEXTURES.contains(&k.as_str()))
            .filter(|k| has_animation_meta(mc, k))
            .collect();
        for path in &animated {
            for variant in 1..ANIM_VARIANTS {
                intern(anim_variant_key(path, variant), TextureSource::Plain(path.clone()), &mut needed);
            }
        }

        // 1.5) Atlas-Kachelgroesse EINMAL fuer den ganzen Bau festlegen (s. determine_tile_size/
        // BlockAtlas::tile_size-Feldkommentar) -- muss vor dem eigentlichen Kachel-Laden (2)
        // feststehen, damit jede Kachel direkt in der Zielgroesse ankommt.
        let tile_size = determine_tile_size(mc, &needed);

        // 2) Alle eindeutigen Texturen laden. Feste Sonderkacheln zuerst: Index 0 = Missing
        // (magenta), Index 1 = Weiss (fuer Entity-Boxen).
        let mut tiles: Vec<Vec<u8>> = vec![missing_tile(tile_size), vec![255u8; (tile_size * tile_size * 4) as usize]];
        let mut tile_index: HashMap<String, usize> = HashMap::new();
        for (key, source) in &needed {
            let TextureSource::Plain(path) = source;
            let tile = load_tile(mc, path, tile_size);
            if let Some(mut tile) = tile {
                // FRUEHER wurde hier fuer Glas ein Bayer-Dither eingebrannt (jedes zweite
                // Texel auf Alpha 0), um Transparenz im Opak-Pass vorzutaeuschen -- das ergab
                // Fliegengitter statt Glas. Glas laeuft jetzt ueber den Translucent-Pass mit
                // echtem Alpha-Blending (s. world::mesher::is_glass), die Kachel bleibt
                // deshalb unveraendert. `translucent_paths` markiert weiterhin, WELCHE
                // Kacheln das betrifft -- gebraucht wird die Menge aktuell aber nicht mehr.
                if opaque_leaf_paths.contains(key) {
                    force_opaque(&mut tile);
                }
                tile_index.insert(key.clone(), tiles.len());
                tiles.push(tile);
            }
        }

        // 2.5) Fuer Bloecke mit Vanillas pseudozufaelliger Block-Rotation zusaetzliche,
        // pixelweise gedrehte/gespiegelte Kacheln fuer Oben/Unten backen (s. rng_face_uvs-
        // Feldkommentar) -- MUSS vor der Atlas-Groessenberechnung (3) passieren, sonst
        // waeren die schon fest verrechneten UV-Bruchteile aller anderen Bloecke falsch
        // (sie sind relativ zur GESAMTEN Atlasgroesse normiert, s. uv_of-Kommentar unten).
        let mut rng_tile_keys: HashMap<String, Vec<[String; 6]>> = HashMap::new();
        for block in block_faces.keys().cloned().collect::<Vec<_>>() {
            let Ok(bytes) = mc.load_bytes(&format!("minecraft/blockstates/{block}.json")) else { continue };
            let Ok(bs) = serde_json::from_slice::<BlockstateJson>(&bytes) else { continue };
            let Some(variants) = all_variant_entries(&bs) else { continue };
            let faces = &block_faces[&block];
            let mut per_variant_keys: Vec<[String; 6]> = Vec::with_capacity(variants.len());
            for (x_deg, y_deg, mirrored) in variants {
                let _ = x_deg; // bei allen bisher bekannten RNG-Bloecken immer 0, s. Modulkommentar oben
                let mut face_keys = faces.clone();
                for &face_idx in &[FACE_UP, FACE_DOWN] {
                    let base_path = &faces[face_idx];
                    let steps = (y_deg / 90).rem_euclid(4) as u32;
                    if steps == 0 && !mirrored {
                        continue; // deckt sich mit der schon vorhandenen, unveraenderten Kachel
                    }
                    let rng_key = format!("{base_path}\0rot{steps}\0mir{mirrored}");
                    if !tile_index.contains_key(&rng_key) {
                        if let Some(&base_idx) = tile_index.get(base_path) {
                            let mut tile = tiles[base_idx].clone();
                            if mirrored {
                                tile = mirror_tile(&tile, tile_size);
                            }
                            for _ in 0..steps {
                                tile = rotate_tile_90(&tile, tile_size);
                            }
                            tile_index.insert(rng_key.clone(), tiles.len());
                            tiles.push(tile);
                        }
                    }
                    face_keys[face_idx] = rng_key;
                }
                per_variant_keys.push(face_keys);
            }
            rng_tile_keys.insert(block, per_variant_keys);
        }

        // 3) Atlas-Groesse: quadratisch, tile_size-Kacheln, Zweierpotenz-Kantenlaenge.
        let tile_count = tiles.len() as u32;
        let tiles_per_row = (tile_count as f64).sqrt().ceil() as u32;
        let tiles_per_row = tiles_per_row.max(1).next_power_of_two();
        let size = tiles_per_row * tile_size;
        // Jede Kachel um einen Bruchteil eines Atlaspixels nach innen (TILE_UV_INSET_PX): ein
        // Bildpunkt genau an einer Blockkante bekommt seine UV interpoliert und kann dabei um
        // Rundungsreste ueber den Kachelrand hinaus landen -- mit Nearest-Sampling ist das ein
        // Texel der NACHBARKACHEL im Atlas. In der Ferne bei flachem Blick (genau entlang
        // einer Achse liegen die Kanten waagerecht im Bild) blitzte das als gestrichelte
        // fremdfarbige Linie zwischen den Bloecken. Vanilla rueckt aus demselben Grund ein.
        let uv_of = |index: u32| -> UvRect {
            let (tx, ty) = (index % tiles_per_row, index / tiles_per_row);
            let s = tile_size as f32 / size as f32;
            let e = TILE_UV_INSET_PX / size as f32;
            [tx as f32 * s + e, ty as f32 * s + e, (tx + 1) as f32 * s - e, (ty + 1) as f32 * s - e]
        };

        // Mipmap-Kette: JE KACHEL EINZELN verkleinert (s. downsample_tile-Kommentar), erst
        // danach pro Stufe in einen eigenen (kleineren) Atlas gepackt -- ein Box-Filter direkt
        // ueber den fertigen Atlas haette an jeder Kachelgrenze fremde Nachbartexturen mit
        // eingemischt ("Atlas-Bleeding").
        let mip_levels = mipmap_levels(tile_size);
        let tile_mips: Vec<Vec<Vec<u8>>> = tiles.iter().map(|t| build_tile_mips(t, tile_size, mip_levels)).collect();
        let mips: Vec<Vec<u8>> = (0..=mip_levels)
            .map(|level| {
                let level_tile_size = tile_size >> level;
                let level_size = tiles_per_row * level_tile_size;
                let mut level_rgba = vec![0u8; (level_size * level_size * 4) as usize];
                for (i, levels) in tile_mips.iter().enumerate() {
                    let (tx, ty) = (i as u32 % tiles_per_row, i as u32 / tiles_per_row);
                    let tile = &levels[level as usize];
                    for y in 0..level_tile_size {
                        for x in 0..level_tile_size {
                            let px = tx * level_tile_size + x;
                            let py = ty * level_tile_size + y;
                            let dst = ((py * level_size + px) * 4) as usize;
                            let src = ((y * level_tile_size + x) * 4) as usize;
                            level_rgba[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
                        }
                    }
                }
                level_rgba
            })
            .collect();

        // 4) Pro Block die sechs UV-Rechtecke.
        let missing_uv = uv_of(0);
        let mut uvs: HashMap<String, [UvRect; 6]> = HashMap::new();
        for (block, faces) in &block_faces {
            let rects: [UvRect; 6] = std::array::from_fn(|i| tile_index.get(&faces[i]).map(|&idx| uv_of(idx as u32)).unwrap_or(missing_uv));
            uvs.insert(block.clone(), rects);
        }
        let mut content_uvs: HashMap<String, [Option<UvRect>; 6]> = HashMap::new();
        for (block, content) in &block_content {
            let rects: [Option<UvRect>; 6] =
                std::array::from_fn(|i| content[i].as_ref().and_then(|k| tile_index.get(k)).map(|&idx| uv_of(idx as u32)));
            content_uvs.insert(block.clone(), rects);
        }
        let mut overlay_uvs: HashMap<String, [Option<UvRect>; 6]> = HashMap::new();
        for (block, overlay) in &block_overlay {
            let rects: [Option<UvRect>; 6] =
                std::array::from_fn(|i| overlay[i].as_ref().and_then(|k| tile_index.get(k)).map(|&idx| uv_of(idx as u32)));
            overlay_uvs.insert(block.clone(), rects);
        }
        let variant_uvs: HashMap<String, ([UvRect; 6], [Option<UvRect>; 6])> = variant_faces
            .iter()
            .map(|(model, (faces, overlay))| {
                let rects = std::array::from_fn(|i| tile_index.get(&faces[i]).map(|&idx| uv_of(idx as u32)).unwrap_or(missing_uv));
                let over = std::array::from_fn(|i| overlay[i].as_ref().and_then(|k| tile_index.get(k)).map(|&idx| uv_of(idx as u32)));
                (model.clone(), (rects, over))
            })
            .collect();
        let particle_uvs: HashMap<String, UvRect> =
            block_particle.iter().filter_map(|(block, key)| tile_index.get(key).map(|&idx| (block.clone(), uv_of(idx as u32)))).collect();
        let mut rng_face_uvs: HashMap<String, Vec<[UvRect; 6]>> = HashMap::new();
        for (block, variants) in &rng_tile_keys {
            let resolved: Vec<[UvRect; 6]> = variants
                .iter()
                .map(|face_keys| std::array::from_fn(|i| tile_index.get(&face_keys[i]).map(|&idx| uv_of(idx as u32)).unwrap_or(missing_uv)))
                .collect();
            rng_face_uvs.insert(block.clone(), resolved);
        }

        // Wasser-Animation: die Kachel, auf die "water" tatsaechlich zeigt (immer
        // "block/water_still" -- water.json hat kein Modell mit Flaechen, nur eine
        // Partikeltextur, s. resolve_particle-Fallback oben), plus ALLE ihre Frames.
        let water_path = uvs.contains_key("water").then(|| "block/water_still".to_string());
        let water_tile_origin = water_path.as_ref().and_then(|p| tile_index.get(p)).map(|&idx| {
            let idx = idx as u32;
            ((idx % tiles_per_row) * tile_size, (idx / tiles_per_row) * tile_size)
        });
        let water_animation_frames = water_path.and_then(|p| load_tile_frames(mc, &p, tile_size)).unwrap_or_default();

        // Fliesstextur: dieselbe Behandlung wie water_still -- UV zum Zeichnen, Ursprung +
        // Frames fuers Animieren. Alles Option/leer, falls die Kachel nicht im Atlas landete
        // (kein Wasser in der Registry, oder ein Resourcepack ohne water_flow.png): der
        // Mesher faellt dann auf die Standtextur zurueck, statt Muell zu sampeln.
        let water_flow_index = tile_index.get(WATER_FLOW_TEXTURE).copied();
        let water_flow_uv = water_flow_index.map(|idx| uv_of(idx as u32));
        let water_flow_tile_origin = water_flow_index.map(|idx| {
            let idx = idx as u32;
            ((idx % tiles_per_row) * tile_size, (idx / tiles_per_row) * tile_size)
        });
        let water_flow_animation_frames =
            water_flow_index.and_then(|_| load_tile_frames(mc, WATER_FLOW_TEXTURE, tile_size)).unwrap_or_default();
        let water_overlay_uv = tile_index.get(WATER_OVERLAY_TEXTURE).map(|&idx| uv_of(idx as u32));
        let lava_flow_uv = tile_index.get(LAVA_FLOW_TEXTURE).map(|&idx| uv_of(idx as u32));
        let destroy_stage_uvs: [Option<UvRect>; DESTROY_STAGE_COUNT] =
            std::array::from_fn(|stage| tile_index.get(&destroy_stage_texture(stage)).map(|&idx| uv_of(idx as u32)));

        let mut animations = Vec::new();
        for (key, source) in &needed {
            let TextureSource::Plain(path) = source;
            if path == "block/water_still" || path == WATER_FLOW_TEXTURE {
                continue;
            }
            if let Some(&idx) = tile_index.get(key) {
                let idx = idx as u32;
                let origin = ((idx % tiles_per_row) * tile_size, (idx / tiles_per_row) * tile_size);
                let mut animation = TileAnimation::load(mc, path, origin, tile_size, mip_levels);
                if let Some(a) = animation.as_mut() {
                    let total = a.sequence.iter().map(|&(_, t)| t as u64).sum();
                    if is_fire_texture(path) {
                        let variant = key.split("\0phase").nth(1).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                        (a.tick_offset, a.rate) = variant_timing(path, variant, FIRE_VARIANTS, total);
                    } else if let Some(variant) = key.split("\0anim").nth(1).and_then(|v| v.parse::<u64>().ok()) {
                        // Dieselbe Verteilung wie beim Feuer; Kopie 0 ist die Basiskachel.
                        (a.tick_offset, a.rate) = variant_timing(path, variant, ANIM_VARIANTS, total);
                    }
                }
                animations.extend(animation);
            }
        }

        let texture_uvs: HashMap<String, UvRect> =
            tile_index.iter().filter(|(k, _)| !k.contains('\0')).map(|(k, &i)| (k.clone(), uv_of(i as u32))).collect();
        let mut texture_variants: HashMap<String, Vec<UvRect>> = HashMap::new();
        for pair in FIRE_TEXTURE_PAIRS {
            let pool: Vec<UvRect> = pair
                .iter()
                .flat_map(|path| (0..FIRE_VARIANTS).map(move |v| fire_variant_key(path, v)))
                .filter_map(|key| tile_index.get(&key).map(|&i| uv_of(i as u32)))
                .collect();
            if pool.len() > 1 {
                for path in pair {
                    texture_variants.insert(path.to_string(), pool.clone());
                }
            }
        }
        let anim_variant_tiles: Vec<(u32, Vec<u32>)> = animated
            .iter()
            .filter_map(|path| {
                let base = *tile_index.get(path)? as u32;
                let copies: Vec<u32> =
                    (1..ANIM_VARIANTS).filter_map(|v| tile_index.get(&anim_variant_key(path, v)).map(|&i| i as u32)).collect();
                (!copies.is_empty()).then_some((base, copies))
            })
            .collect();
        Ok(Self {
            anim_variant_tiles,
            tiles_per_row,
            texture_variants,
            destroy_stage_uvs,
            texture_uvs,
            mips,
            size,
            tile_size,
            uvs,
            content_uvs,
            overlay_uvs,
            variant_uvs,
            particle_uvs,
            rng_face_uvs,
            water_animation_frames,
            water_tile_origin,
            animations,
            water_flow_uv,
            water_overlay_uv,
            water_flow_animation_frames,
            water_flow_tile_origin,
            lava_flow_uv,
            missing: [missing_uv; 6],
            white_uv: uv_of(1),
        })
    }
}

// Anzahl ZUSAETZLICHER Mip-Stufen ueber die volle Aufloesung hinaus, aus der tatsaechlichen
// Kachelgroesse abgeleitet (log2 -- tile_size ist per determine_tile_size() immer eine
// Zweierpotenz, trailing_zeros() liefert das exakt) statt der frueheren festen `4`: bei
// 16px kommt weiterhin exakt 4 heraus (16,8,4,2,1 = 5 Stufen, identisch zum bisherigen
// Verhalten), groessere Kacheln (HD-Resourcepacks) bekommen entsprechend mehr sinnvolle
// Halbierungsschritte bis 1x1. Gedeckelt bei 6 (256px) -- mehr Stufen als der Atlas selbst
// je gross werden kann, waeren sinnlos.
fn mipmap_levels(tile_size: u32) -> u32 {
    tile_size.trailing_zeros().min(6)
}

// Verkleinert eine EINZELNE, isolierte Kachel um die Haelfte (reiner 2x2-Box-Filter in
// sRGB-kodierten Bytes, nicht linearisiert -- fuer 16x16-Pixel-Art mit hartem Kontrast macht
// eine physikalisch korrekte Mittelung in linearem Raum keinen sichtbaren Unterschied, kostet
// aber deutlich mehr Code). Arbeitet NUR auf den eigenen Pixeln der Kachel, nie auf denen
// eines Nachbarn -- das ist der ganze Punkt gegenueber einem Box-Filter ueber den fertig
// gepackten Atlas (s. build()-Kommentar zu Atlas-Bleeding).
fn downsample_tile(tile: &[u8], size: u32) -> Vec<u8> {
    let half = size / 2;
    let mut out = vec![0u8; (half * half * 4) as usize];
    for y in 0..half {
        for x in 0..half {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let src = (((y * 2 + dy) * size + (x * 2 + dx)) * 4) as usize;
                for (c, s) in sum.iter_mut().enumerate() {
                    *s += tile[src + c] as u32;
                }
            }
            let dst = ((y * half + x) * 4) as usize;
            for (c, s) in sum.iter().enumerate() {
                out[dst + c] = (s / 4) as u8;
            }
        }
    }
    out
}

// Wie downsample_tile, fuer Kacheln MIT durchsichtigen Stellen (Spinnennetz, Glas, Laub):
// die Farbe wird nach Deckkraft gewichtet gemittelt, das Alpha wie gehabt.
//
// ECHTER BUG gewesen (Spinnennetz-Umrisse wurden aus der Ferne schwarz):
// durchsichtige Texel tragen fast immer die Farbe Schwarz (0,0,0,0). Ungewichtet gemittelt
// zog jede Halbierung den Rand eines Netzfadens Richtung Schwarz, und die Deckungskorrektur
// danach machte genau diese dunklen Randtexel wieder sichtbar. Gewichtet traegt ein
// durchsichtiges Texel nichts zur Farbe bei. Gemittelt wird wie in Vanillas MipmapHelper
// (Exponent 2.2) im linearen Raum, damit helle Faeden nicht zusaetzlich nachdunkeln.
//
// Bleibt ein Ziel-Texel ganz ohne deckende Quelle, bekommt es die mittlere sichtbare Farbe
// der Kachel -- sonst schoebe die Deckungskorrektur (+0.025) spaeter doch ein schwarzes
// Texel ueber die Schwelle.
fn downsample_tile_cutout(tile: &[u8], size: u32) -> Vec<u8> {
    // Linear je Byte-Wert aus einer Tabelle statt powf je Kanal und Texel (dieselben Werte).
    static LINEAR: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let tabelle = LINEAR.get_or_init(|| std::array::from_fn(|c| (c as f32 / 255.0).powf(2.2)));
    let lin = |c: u8| tabelle[c as usize];
    let srgb = |l: f32| (l.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
    // Mittlere sichtbare Farbe der ganzen Kachel (Rueckfall, s. oben).
    let mut gesamt = [0.0f32; 3];
    let mut gesamt_gewicht = 0.0f32;
    for p in tile.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            gesamt[c] += lin(p[c]) * a;
        }
        gesamt_gewicht += a;
    }
    let rueckfall = if gesamt_gewicht > 0.0 { gesamt.map(|c| srgb(c / gesamt_gewicht)) } else { [0, 0, 0] };

    let half = size / 2;
    let mut out = vec![0u8; (half * half * 4) as usize];
    for y in 0..half {
        for x in 0..half {
            let mut farbe = [0.0f32; 3];
            let mut gewicht = 0.0f32;
            let mut alpha = 0u32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let src = (((y * 2 + dy) * size + (x * 2 + dx)) * 4) as usize;
                let a = tile[src + 3] as f32 / 255.0;
                for c in 0..3 {
                    farbe[c] += lin(tile[src + c]) * a;
                }
                gewicht += a;
                alpha += tile[src + 3] as u32;
            }
            let dst = ((y * half + x) * 4) as usize;
            let rgb = if gewicht > 0.0 { farbe.map(|c| srgb(c / gewicht)) } else { rueckfall };
            out[dst..dst + 3].copy_from_slice(&rgb);
            out[dst + 3] = (alpha / 4) as u8;
        }
    }
    out
}

// Schwelle, ab der ein Texel als "deckend" zaehlt -- Vanillas
// MipmapHelper.DEFAULT_ALPHA_THRESHOLD.
const ALPHA_THRESHOLD: f32 = 0.5;

// Hat die Kachel ueberhaupt voellig durchsichtige Texel? Nur dann ist die Deckungskorrektur
// noetig (Vanillas MipmapHelper.hasAlpha, das ueber MipmapStrategy.AUTO entscheidet: mit
// Alpha -> CUTOUT mit Korrektur, ohne -> einfache Mittelung).
fn hat_transparente_texel(tile: &[u8]) -> bool {
    tile.chunks_exact(4).any(|p| p[3] == 0)
}

// Welcher Flaechenanteil der Kachel ist "deckend genug"? -- MipmapHelper.getOpacityCoverage.
//
// Nicht einfach "Anteil der Texel ueber der Schwelle": zwischen je vier benachbarten Texeln
// wird bilinear interpoliert und 4x4 ueberabgetastet. Genau so sieht es die GPU beim
// Vergroessern auch, deshalb misst das die WAHRGENOMMENE Deckung statt der reinen Texelzahl.
fn deckungsgrad(tile: &[u8], size: u32, schwelle: f32, alpha_faktor: f32) -> f32 {
    if size < 2 {
        // Bei 1x1 gibt es kein Texelpaar zum Interpolieren -- Vanilla teilt hier durch
        // (w-1)*(h-1) und wuerde durch Null laufen. Der Alphawert selbst ist die Antwort.
        let a = tile.first().map(|_| tile[3] as f32 / 255.0).unwrap_or(0.0);
        return if a * alpha_faktor > schwelle { 1.0 } else { 0.0 };
    }
    let alpha_bei = |x: u32, y: u32| {
        let i = ((y * size + x) * 4 + 3) as usize;
        (tile[i] as f32 / 255.0 * alpha_faktor).clamp(0.0, 1.0)
    };
    let mut summe = 0.0f32;
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let (a, b) = (alpha_bei(x, y), alpha_bei(x + 1, y));
            let (c, d) = (alpha_bei(x, y + 1), alpha_bei(x + 1, y + 1));
            // Die Mischung liegt nie ueber der groessten und nie unter der kleinsten Ecke: sind
            // alle vier auf derselben Seite der Schwelle (bei Glas, Feuer, Laub fast jede Zelle),
            // steht das Ergebnis ohne die 16 Stichproben fest. Animierte Kacheln laufen hier je
            // Tick durch -- das war der groesste Posten im Animations-Thread (Profiler).
            if a.min(b).min(c).min(d) > schwelle {
                summe += 1.0;
                continue;
            }
            if a.max(b).max(c).max(d) <= schwelle {
                continue;
            }
            let mut treffer = 0.0f32;
            for q in 0..4 {
                let r = (q as f32 + 0.5) / 4.0;
                for s in 0..4 {
                    let t = (s as f32 + 0.5) / 4.0;
                    let u = a * (1.0 - t) * (1.0 - r) + b * t * (1.0 - r) + c * (1.0 - t) * r + d * t * r;
                    if u > schwelle {
                        treffer += 1.0;
                    }
                }
            }
            summe += treffer / 16.0;
        }
    }
    summe / ((size - 1) * (size - 1)) as f32
}

// Hebt das Alpha einer Mip-Stufe so an, dass ihr Deckungsgrad wieder dem des Originals
// entspricht -- MipmapHelper.adjustAlphaForTargetCoverage.
//
// DAS ist der Grund, warum Vanillas Glas aus der Entfernung sichtbar bleibt und unseres
// verschwand: Vanilla blendet es weich aus, hier war es ganz weg. Eine Glastextur ist ueberwiegend durchsichtig mit einem duennen deckenden
// Rahmen; beim Halbieren mittelt sich dessen Alpha mit dem durchsichtigen Rest, Stufe fuer
// Stufe blasser, bis nichts mehr da ist. Der gesuchte Faktor wird per Binaersuche bestimmt
// (fuenf Schritte reichen, wie in Vanilla), weil sich der Deckungsgrad nicht direkt nach dem
// Faktor aufloesen laesst.
fn korrigiere_alpha_auf_deckung(tile: &mut [u8], size: u32, ziel: f32, schwelle: f32) {
    let (mut unten, mut oben) = (0.0f32, 4.0f32);
    let mut faktor = 1.0f32;
    let mut bester = 1.0f32;
    let mut kleinster_fehler = f32::MAX;
    for _ in 0..5 {
        let ist = deckungsgrad(tile, size, schwelle, faktor);
        let fehler = (ist - ziel).abs();
        if fehler < kleinster_fehler {
            kleinster_fehler = fehler;
            bester = faktor;
        }
        if ist < ziel {
            unten = faktor;
        } else if ist > ziel {
            oben = faktor;
        } else {
            break;
        }
        faktor = (unten + oben) * 0.5;
    }
    for p in tile.chunks_exact_mut(4) {
        // Das +0.025 stammt aus Vanilla: ein kleiner Grundzuschlag, damit ganz schwache
        // Raender nicht doch noch wegkippen.
        let a = (p[3] as f32 / 255.0 * bester + 0.025).clamp(0.0, 1.0);
        p[3] = (a * 255.0).round() as u8;
    }
}

// Mip-Kette EINER Kachel: Index 0 = Original (tile_size), danach je einmal halbiert bis
// levels (bei 16px kommt exakt bis 1x1 heraus -- kein Sonderfall fuer "zu klein" noetig,
// s. mipmap_levels-Kommentar).
//
// Kacheln MIT durchsichtigen Stellen bekommen zusaetzlich Vanillas Deckungskorrektur (s.
// korrigiere_alpha_auf_deckung); voll deckende Kacheln (Stein, Erde, ...) nicht -- dort gibt
// es nichts zu erhalten, und die Korrektur waere reine Rechenzeit.
fn build_tile_mips(tile: &[u8], tile_size: u32, levels_count: u32) -> Vec<Vec<u8>> {
    let korrigieren = hat_transparente_texel(tile);
    let ziel_deckung = if korrigieren { deckungsgrad(tile, tile_size, ALPHA_THRESHOLD, 1.0) } else { 0.0 };

    let mut levels = vec![tile.to_vec()];
    let mut size = tile_size;
    for _ in 0..levels_count {
        let mut naechste =
            if korrigieren { downsample_tile_cutout(levels.last().unwrap(), size) } else { downsample_tile(levels.last().unwrap(), size) };
        size /= 2;
        if korrigieren {
            korrigiere_alpha_auf_deckung(&mut naechste, size, ziel_deckung, ALPHA_THRESHOLD);
        }
        levels.push(naechste);
    }
    levels
}

// Magenta/schwarz kariertes Feld (2x2-Karo unabhaengig von der Kachelgroesse) -- die
// klassische "fehlende Textur".
fn missing_tile(tile_size: u32) -> Vec<u8> {
    let mut t = vec![0u8; (tile_size * tile_size * 4) as usize];
    let half = (tile_size / 2).max(1);
    for y in 0..tile_size {
        for x in 0..tile_size {
            let magenta = ((x / half) + (y / half)).is_multiple_of(2);
            let c = if magenta { [248, 0, 248, 255] } else { [0, 0, 0, 255] };
            let d = ((y * tile_size + x) * 4) as usize;
            t[d..d + 4].copy_from_slice(&c);
        }
    }
    t
}

// Vanillas Deckungserhaltung beim Mipmapping (s. korrigiere_alpha_auf_deckung). Ohne sie
// verschwand Glas aus der Entfernung komplett -- die Kachel ist ueberwiegend durchsichtig,
// und blosses Mitteln loescht den duennen deckenden Rahmen Stufe fuer Stufe aus.
#[cfg(test)]
mod mipmap_alpha_tests {
    use super::*;

    // 16x16-Kachel wie Glas: durchsichtige Mitte, deckender Rand.
    fn glasartige_kachel(size: u32) -> Vec<u8> {
        let mut t = vec![0u8; (size * size * 4) as usize];
        for y in 0..size {
            for x in 0..size {
                let rand = x == 0 || y == 0 || x == size - 1 || y == size - 1;
                let i = ((y * size + x) * 4) as usize;
                t[i..i + 4].copy_from_slice(&[200, 220, 235, if rand { 255 } else { 0 }]);
            }
        }
        t
    }

    fn hoechstes_alpha(tile: &[u8]) -> u8 {
        tile.chunks_exact(4).map(|p| p[3]).max().unwrap_or(0)
    }

    // DER Fehler: nach ein paar Halbierungen war vom Glasrahmen nichts mehr uebrig.
    //
    // Geprueft werden alle Stufen bis hinunter zu 2x2. Die letzte Stufe (1x1) ist BEWUSST
    // ausgenommen: bei einem einzigen Texel gibt es keine Nachbarn zum Interpolieren, der
    // Deckungsgrad kann nur 0 oder 1 sein, und ein Zwischenwert ist grundsaetzlich nicht
    // treffbar. Vanilla laeuft dort sogar in eine Division durch Null ((w-1)*(h-1) == 0) --
    // die Stufe wird schlicht erst dann gewaehlt, wenn der Block kleiner als ein Bildpunkt
    // ist, und dann faellt sein Alpha nicht mehr auf.
    #[test]
    fn glas_verschwindet_nicht_in_den_mip_stufen() {
        let kachel = glasartige_kachel(16);
        let stufen = build_tile_mips(&kachel, 16, mipmap_levels(16));
        let mut size = 16u32;
        for (i, stufe) in stufen.iter().enumerate() {
            if size >= 2 {
                assert!(
                    hoechstes_alpha(stufe) > 128,
                    "Mip-Stufe {i} ({size}x{size}): hoechstes Alpha nur {} -- das Glas waere aus der Entfernung unsichtbar",
                    hoechstes_alpha(stufe)
                );
            }
            size /= 2;
        }
    }

    // Spinnennetz-Fall: weisser Faden auf durchsichtigem SCHWARZ. Die Mip-Stufen duerfen den
    // Faden nicht abdunkeln -- jedes sichtbare Texel bleibt weiss.
    #[test]
    fn durchsichtiges_schwarz_dunkelt_faeden_nicht_ab() {
        let size = 16u32;
        let mut t = vec![0u8; (size * size * 4) as usize];
        for i in 0..size {
            // Diagonaler Faden, ein Texel breit.
            let p = ((i * size + i) * 4) as usize;
            t[p..p + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        let stufen = build_tile_mips(&t, size, mipmap_levels(size));
        for (i, stufe) in stufen.iter().enumerate().skip(1) {
            for px in stufe.chunks_exact(4).filter(|px| px[3] > 0) {
                assert!(px[0] >= 250 && px[1] >= 250 && px[2] >= 250, "Stufe {i}: Fadentexel {px:?} ist nachgedunkelt");
            }
        }
    }

    // Gegenprobe, dass der Test oben wirklich etwas misst: OHNE Korrektur (reines Mitteln)
    // sackt dieselbe Kachel nachweislich ab.
    #[test]
    fn ohne_korrektur_wuerde_das_glas_absacken() {
        let kachel = glasartige_kachel(16);
        let mut stufe = kachel.clone();
        let mut size = 16;
        for _ in 0..3 {
            stufe = downsample_tile(&stufe, size);
            size /= 2;
        }
        assert!(hoechstes_alpha(&stufe) < 128, "reines Mitteln muesste den Rahmen ausduennen -- sonst prueft der Test darueber nichts");
    }

    // Voll deckende Kacheln (Stein, Erde) duerfen von der Korrektur gar nicht angefasst
    // werden: sie sind ueberall Alpha 255 und sollen es bleiben.
    #[test]
    fn deckende_kacheln_bleiben_unveraendert() {
        let kachel = vec![255u8; 16 * 16 * 4];
        let stufen = build_tile_mips(&kachel, 16, mipmap_levels(16));
        for (i, stufe) in stufen.iter().enumerate() {
            assert!(stufe.chunks_exact(4).all(|p| p[3] == 255), "Mip-Stufe {i} einer deckenden Kachel wurde durchsichtig");
        }
    }

    // Der Deckungsgrad misst die WAHRGENOMMENE Flaeche, nicht die Texelzahl: eine voll
    // deckende Kachel ist 1.0, eine voellig durchsichtige 0.0.
    #[test]
    fn deckungsgrad_kennt_die_extreme() {
        let voll = vec![255u8; 8 * 8 * 4];
        let leer = vec![0u8; 8 * 8 * 4];
        assert!((deckungsgrad(&voll, 8, ALPHA_THRESHOLD, 1.0) - 1.0).abs() < 1e-6);
        assert_eq!(deckungsgrad(&leer, 8, ALPHA_THRESHOLD, 1.0), 0.0);
    }

    // Die Abkuerzung fuer einseitige Zellen (s. deckungsgrad) aendert nichts: gegen die volle
    // Stichprobenrechnung auf zufaelligen Kacheln mit Glas-artigen Alphas und Zwischenwerten.
    #[test]
    fn deckungsgrad_abkuerzung_wie_volle_rechnung() {
        fn voll(tile: &[u8], size: u32, schwelle: f32, faktor: f32) -> f32 {
            let alpha = |x: u32, y: u32| (tile[((y * size + x) * 4 + 3) as usize] as f32 / 255.0 * faktor).clamp(0.0, 1.0);
            let mut summe = 0.0f32;
            for y in 0..size - 1 {
                for x in 0..size - 1 {
                    let (a, b, c, d) = (alpha(x, y), alpha(x + 1, y), alpha(x, y + 1), alpha(x + 1, y + 1));
                    let mut treffer = 0.0f32;
                    for q in 0..4 {
                        let r = (q as f32 + 0.5) / 4.0;
                        for s in 0..4 {
                            let t = (s as f32 + 0.5) / 4.0;
                            if a * (1.0 - t) * (1.0 - r) + b * t * (1.0 - r) + c * (1.0 - t) * r + d * t * r > schwelle {
                                treffer += 1.0;
                            }
                        }
                    }
                    summe += treffer / 16.0;
                }
            }
            summe / ((size - 1) * (size - 1)) as f32
        }
        let mut zufall = 0x2545_f491_4f6c_dd1du64;
        for runde in 0..200 {
            let size = [4u32, 8, 16][runde % 3];
            let tile: Vec<u8> = (0..size * size * 4)
                .map(|_| {
                    zufall ^= zufall << 13;
                    zufall ^= zufall >> 7;
                    zufall ^= zufall << 17;
                    [0u8, 255, 255, 0, (zufall >> 32) as u8][(zufall % 5) as usize]
                })
                .collect();
            for faktor in [1.0f32, 0.75, 1.9, 3.1] {
                let (schnell, langsam) = (deckungsgrad(&tile, size, ALPHA_THRESHOLD, faktor), voll(&tile, size, ALPHA_THRESHOLD, faktor));
                assert!((schnell - langsam).abs() < 1e-5, "Runde {runde}, Faktor {faktor}: {schnell} != {langsam}");
            }
        }
    }
}

#[cfg(test)]
mod animation_tests {
    use super::*;

    fn animation(sequence: Vec<(u32, u32)>, interpolate: bool) -> TileAnimation {
        // Zwei 1x1-Frames: schwarz und weiss.
        let strip = vec![0, 0, 0, 255, 255, 255, 255, 255];
        TileAnimation { origin: (0, 0), tile_size: 1, mip_levels: 0, native: 1, strip, frame_count: 2, sequence, interpolate, tick_offset: 0, rate: RATE_UNIT, path: String::new() }
    }

    #[test]
    fn animation_folgt_reihenfolge_und_dauer() {
        let a = animation(vec![(0, 2), (1, 3)], false);
        let frames: Vec<u32> = (0..6).map(|t| a.state_at(t).frame).collect();
        assert_eq!(frames, vec![0, 0, 1, 1, 1, 0], "2 Ticks Frame 0, 3 Ticks Frame 1, dann von vorn");
        assert_eq!(a.state_at(0), a.state_at(1), "ohne Interpolation kein Hochladen innerhalb eines Frames");
    }

    #[test]
    fn interpolation_blendet_zum_naechsten_frame() {
        let a = animation(vec![(0, 4), (1, 4)], true);
        assert_eq!(a.render(a.state_at(0))[0][0], 0, "Anfang: rein der aktuelle Frame");
        assert_eq!(a.render(a.state_at(2))[0][0], 128, "Haelfte: halb uebergeblendet");
        assert_eq!(a.render(a.state_at(2))[0][3], 255, "Alpha bleibt");
    }
}
