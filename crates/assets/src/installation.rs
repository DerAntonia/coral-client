// Laedt Asset-Index + Objekte aus einer lokalen Minecraft-Installation (offiziell oder Prism,
// s. source.rs). Liest NUR
// bereits vorhandene lokale Dateien -- laedt/downloaded nichts selbst (die jeweilige
// Version muss vorher mindestens einmal ueber den offiziellen Launcher gestartet
// worden sein, damit Version-JSON + Asset-Index + Objects lokal vorliegen).

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::source::active_source;

#[derive(Debug)]
pub enum AssetsError {
    MinecraftDirNotFound,
    VersionNotFound { version: String, path: PathBuf },
    AssetIndexNotFound { index_id: String, path: PathBuf },
    AssetNotFound { virtual_path: String },
    Io(std::io::Error),
    Json(serde_json::Error),
    Image(image::ImageError),
    Zip(zip::result::ZipError),
    // Font-Definition im Jar sieht anders aus als erwartet (z.B. nach einem Versionswechsel).
    FontFormat(String),
}

impl std::fmt::Display for AssetsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetsError::MinecraftDirNotFound => {
                write!(f, "keine lokale .minecraft-Installation gefunden")
            }
            AssetsError::VersionNotFound { version, path } => {
                write!(f, "Version '{version}' nicht installiert (erwartet: {})", path.display())
            }
            AssetsError::AssetIndexNotFound { index_id, path } => write!(
                f,
                "Asset-Index '{index_id}' nicht lokal vorhanden (erwartet: {}) -- Version einmal ueber den offiziellen Launcher starten",
                path.display()
            ),
            AssetsError::AssetNotFound { virtual_path } => {
                write!(f, "Asset nicht im Index: {virtual_path}")
            }
            AssetsError::Io(e) => write!(f, "IO-Fehler: {e}"),
            AssetsError::Json(e) => write!(f, "JSON-Fehler: {e}"),
            AssetsError::Image(e) => write!(f, "Bild-Dekodierfehler: {e}"),
            AssetsError::Zip(e) => write!(f, "Client-Jar-Lesefehler: {e}"),
            AssetsError::FontFormat(msg) => write!(f, "Font-Definition unerwartet: {msg}"),
        }
    }
}

impl std::error::Error for AssetsError {}

impl From<std::io::Error> for AssetsError {
    fn from(e: std::io::Error) -> Self {
        AssetsError::Io(e)
    }
}

impl From<serde_json::Error> for AssetsError {
    fn from(e: serde_json::Error) -> Self {
        AssetsError::Json(e)
    }
}

impl From<image::ImageError> for AssetsError {
    fn from(e: image::ImageError) -> Self {
        AssetsError::Image(e)
    }
}

impl From<zip::result::ZipError> for AssetsError {
    fn from(e: zip::result::ZipError) -> Self {
        AssetsError::Zip(e)
    }
}

#[derive(Deserialize)]
struct AssetIndexFile {
    objects: HashMap<String, AssetObject>,
}

#[derive(Deserialize, Clone)]
struct AssetObject {
    hash: String,
}

pub struct DecodedTexture {
    pub width: u32,
    pub height: u32,
    // RGBA8, Zeile fuer Zeile von oben.
    pub rgba: Vec<u8>,
}

pub struct MinecraftInstallation {
    // Enthaelt indexes/ und objects/ (bei Prism im Datenordner, nicht in der Instanz).
    assets_dir: PathBuf,
    objects: HashMap<String, AssetObject>,
    // Client-Jar enthaelt bei modernen Versionen die eigentlichen Block-/Item-Texturen
    // und Modelle als eingebautes Standard-Resourcepack (assets/minecraft/...) -- das
    // geteilte Objects-System (assets/objects/) deckt heute primaer Sounds/GUI/Lang ab.
    client_jar: zip::ZipArchive<File>,
    // Bereits entpackte Resourcepack-Ordner (jeder enthaelt selbst ein "assets/"-
    // Unterverzeichnis, exakt wie eine entpackte .zip) -- werden in load_bytes() VOR dem
    // Objects-Store/Jar geprueft. Gestapelt wie in Vanillas Pack-Liste: der ERSTE Eintrag
    // liegt oben und gewinnt, jeder weitere fuellt nur, was die darueber nicht mitbringen.
    // Leer = reines Vanilla-Verhalten, unveraendert.
    //
    // Ein Pack ist ein Ordner oder ein Zip (Server-Resourcepacks, s. javazip) und kann
    // mehrere Ebenen haben: seine Overlays fuer diese Version liegen ueber dem Pack selbst
    // (s. pack_layers).
    layers: Vec<PackLayer>,
    // So viele der `layers` liegen UEBER Vanilla; der Rest darunter fuellt nur noch, was weder
    // die Packs darueber noch Vanilla haben (eigene Modelle, CIT, zusaetzliche Klaenge ...).
    above_vanilla: usize,
    // Sprite-Quellen aller Atlas-Definitionen, beim ersten fehlenden Bild eingelesen (s.
    // atlas_sources), dazu die schon dekodierten Sammelbilder fuer "unstitch".
    atlas_sources: Option<crate::atlas_sources::AtlasSources>,
    sheets: HashMap<String, Option<DecodedTexture>>,
    // Schon aufgeloeste Blockstate-Dateien (s. load_blockstate) -- sie werden je Block
    // mehrfach gelesen, und die Pruefung liest die Modelle mit.
    blockstates: HashMap<String, Vec<u8>>,
}

// Resourcepack-Format dieser Version (version.json im Jar: pack_version.resource_major).
pub const RESOURCE_PACK_FORMAT: u32 = 75;

#[derive(Clone)]
enum PackSource {
    Dir(PathBuf),
    Zip(Arc<crate::javazip::JavaZip>),
}

// Eine Ebene: ein Pack, oder einer seiner Overlay-Ordner (`prefix` = "ordner/").
#[derive(Clone)]
struct PackLayer {
    source: PackSource,
    prefix: String,
    // Im Pack ausgeschaltete Dateien und Ordner, relativ zu "assets/minecraft/" (s. PackDir).
    excluded: Arc<Vec<String>>,
}

// Ein Resourcepack fuer MinecraftInstallation::locate_packs: Ordner oder Zip, dazu was darin
// ausgeschaltet ist -- Dateien und Ordner relativ zu "assets/minecraft/" (z.B.
// "textures/block" oder "sounds.json"). Ausgeschaltetes liest sich, als fehle es im Pack: die
// Ebenen darunter (am Ende Vanilla) fuellen es auf.
pub struct PackDir {
    pub path: PathBuf,
    pub excluded: Vec<String>,
}

// Ob `rest` (relativ zu "assets/minecraft/") eine der Angaben selbst ist oder darin liegt.
pub fn path_excluded(excluded: &[String], rest: &str) -> bool {
    excluded.iter().any(|e| rest == e || rest.strip_prefix(e.as_str()).is_some_and(|r| r.starts_with('/')))
}

impl PackLayer {
    fn excluded(&self, path: &str) -> bool {
        !self.excluded.is_empty() && path.strip_prefix("assets/minecraft/").is_some_and(|rest| path_excluded(&self.excluded, rest))
    }

    // `path` relativ zur Pack-Wurzel, z.B. "assets/minecraft/font/default.json".
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        if self.excluded(path) {
            return None;
        }
        match &self.source {
            PackSource::Dir(dir) => std::fs::read(dir.join(&self.prefix).join(path)).ok(),
            PackSource::Zip(zip) => zip.read(&format!("{}{path}", self.prefix)),
        }
    }

    // Alle Dateien unter `dir` (relativ zur Ebene, mit "/" am Ende), relativ zu `dir`.
    fn list(&self, dir: &str) -> Vec<String> {
        let mut out = self.list_all(dir);
        if !self.excluded.is_empty() {
            out.retain(|n| !self.excluded(&format!("{dir}{n}")));
        }
        out
    }

    fn list_all(&self, dir: &str) -> Vec<String> {
        match &self.source {
            PackSource::Dir(root) => {
                let mut out = Vec::new();
                list_dir(&root.join(&self.prefix).join(dir), "", &mut out);
                out
            }
            PackSource::Zip(zip) => {
                let voll = format!("{}{dir}", self.prefix);
                zip.names().iter().filter_map(|n| n.strip_prefix(&voll)).filter(|n| !n.is_empty() && !n.ends_with('/')).map(str::to_string).collect()
            }
        }
    }
}

fn list_dir(dir: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(eintraege) = std::fs::read_dir(dir) else { return };
    for e in eintraege.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let pfad = format!("{rel}{name}");
        if e.path().is_dir() {
            list_dir(&e.path(), &format!("{pfad}/"), out);
        } else {
            out.push(pfad);
        }
    }
}

// Ob eine Formatangabe aus pack.mcmeta diese Version einschliesst. Formen: Zahl, [min, max],
// {"min_inclusive", "max_inclusive"}; bei min_format/max_format auch [Haupt, Neben].
fn format_bound(v: &serde_json::Value) -> Option<u32> {
    v.as_u64().or_else(|| v.as_array()?.first()?.as_u64()).map(|n| n as u32)
}

fn overlay_matches(entry: &serde_json::Value) -> bool {
    let (min, max) = match (entry.get("min_format").and_then(format_bound), entry.get("max_format").and_then(format_bound)) {
        (Some(a), Some(b)) => (a, b),
        _ => match entry.get("formats") {
            Some(serde_json::Value::Array(a)) if a.len() == 2 => (a[0].as_u64().unwrap_or(0) as u32, a[1].as_u64().unwrap_or(0) as u32),
            Some(f @ serde_json::Value::Object(_)) => (
                f.get("min_inclusive").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                f.get("max_inclusive").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            ),
            Some(f) => match f.as_u64() {
                Some(n) => (n as u32, n as u32),
                None => return false,
            },
            None => return false,
        },
    };
    (min..=max).contains(&RESOURCE_PACK_FORMAT)
}

// Die Ebenen eines Packs, oberste zuerst: die passenden Overlays (der zuletzt aufgefuehrte
// liegt oben, wie bei Vanilla), darunter das Pack selbst.
fn pack_layers(path: &Path, excluded: Vec<String>) -> Vec<PackLayer> {
    let source = if path.is_dir() {
        PackSource::Dir(path.to_path_buf())
    } else {
        match crate::javazip::JavaZip::open(path) {
            Ok(zip) => PackSource::Zip(Arc::new(zip)),
            Err(e) => {
                eprintln!("[assets] Resourcepack {} nicht lesbar: {e}", path.display());
                return Vec::new();
            }
        }
    };
    let basis = PackLayer { source, prefix: String::new(), excluded: Arc::new(excluded) };
    let mut layers = Vec::new();
    if let Some(meta) = basis.read("pack.mcmeta").and_then(|b| serde_json::from_slice::<serde_json::Value>(strip_bom(&b)).ok()) {
        if let Some(serde_json::Value::Array(eintraege)) = meta.get("overlays").and_then(|o| o.get("entries")) {
            for e in eintraege.iter().rev() {
                let Some(dir) = e.get("directory").and_then(|d| d.as_str()) else { continue };
                if overlay_matches(e) {
                    layers.push(PackLayer { source: basis.source.clone(), prefix: format!("{dir}/"), excluded: basis.excluded.clone() });
                }
            }
        }
    }
    layers.push(basis);
    layers
}

// JSON aus Packs beginnt gelegentlich mit einer UTF-8-Byte-Order-Mark, die Java ueberliest.
pub(crate) fn strip_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(b"\xEF\xBB\xBF".as_slice()).unwrap_or(b)
}

// Namensraeume einer Ebene (die Ordner unter assets/), wie Vanilla nur gueltige Namen.
fn layer_namespaces(layer: &PackLayer) -> Vec<String> {
    let gueltig = |n: &str| !n.is_empty() && n.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.'));
    let mut out: Vec<String> = Vec::new();
    for pfad in layer.list("assets/") {
        if let Some((ns, _)) = pfad.split_once('/') {
            if gueltig(ns) && !out.iter().any(|o| o == ns) {
                out.push(ns.to_string());
            }
        }
    }
    out
}

impl MinecraftInstallation {
    // `version` z.B. "1.21.11" -- muss in der gewaehlten Installation schon vorliegen (also
    // mindestens einmal im offiziellen bzw. Prism-Launcher gestartet worden sein).
    // `resource_pack_dirs`: schon entpackte Resourcepack-Ordner, oberster zuerst (s. Feld-
    // kommentar) -- leer bzw. None fuer reines Vanilla-Verhalten. Nimmt alles, was sich
    // durchlaufen laesst, damit `None`/`Some(dir)` fuer ein einzelnes Pack weiter passen.
    pub fn locate(version: &str, resource_pack_dirs: impl IntoIterator<Item = PathBuf>) -> Result<Self, AssetsError> {
        Self::locate_packs(version, resource_pack_dirs.into_iter().map(|path| PackDir { path, excluded: Vec::new() }))
    }

    // Wie locate, je Pack mit dem, was darin ausgeschaltet ist (s. PackDir).
    pub fn locate_packs(version: &str, packs: impl IntoIterator<Item = PackDir>) -> Result<Self, AssetsError> {
        Self::locate_stack(version, packs, Vec::new())
    }

    // Wie locate_packs, dazu Packs UNTER Vanilla (oben zuerst).
    pub fn locate_stack(version: &str, above: impl IntoIterator<Item = PackDir>, below: impl IntoIterator<Item = PackDir>) -> Result<Self, AssetsError> {
        // Welche Installation: die im Launcher gewaehlte (s. source.rs), sonst die Standard-
        // .minecraft.
        let source = active_source().ok_or(AssetsError::MinecraftDirNotFound)?;
        let files = source.resolve(version)?;

        let index_path = files.assets_dir.join("indexes").join(format!("{}.json", files.asset_index));
        let index: AssetIndexFile = serde_json::from_slice(&std::fs::read(&index_path)?)?;

        let jar_file = File::open(&files.client_jar).map_err(|_| AssetsError::VersionNotFound { version: version.to_string(), path: files.client_jar.clone() })?;
        let client_jar = zip::ZipArchive::new(jar_file)?;

        let mut layers: Vec<PackLayer> = above.into_iter().flat_map(|p| pack_layers(&p.path, p.excluded)).collect();
        let above_vanilla = layers.len();
        layers.extend(below.into_iter().flat_map(|p| pack_layers(&p.path, p.excluded)));
        Ok(Self { assets_dir: files.assets_dir, objects: index.objects, client_jar, layers, above_vanilla, atlas_sources: None, sheets: HashMap::new(), blockstates: HashMap::new() })
    }

    // `virtual_path` z.B. "minecraft/textures/block/stone.png". Prueft zuerst die
    // Resourcepack-Ordner von oben nach unten (s. resource_pack_dirs-Feldkommentar), dann das
    // geteilte Objects-System, faellt zuletzt auf das Client-Jar zurueck.
    pub fn load_bytes(&mut self, virtual_path: &str) -> Result<Vec<u8>, AssetsError> {
        if virtual_path.contains("/blockstates/") {
            if let Some(bytes) = self.blockstates.get(virtual_path) {
                return Ok(bytes.clone());
            }
            let bytes = self.load_blockstate(virtual_path)?;
            self.blockstates.insert(virtual_path.to_string(), bytes.clone());
            return Ok(bytes);
        }
        let pfad = format!("assets/{virtual_path}");
        for layer in &self.layers[..self.above_vanilla] {
            if let Some(bytes) = layer.read(&pfad) {
                return Ok(bytes);
            }
        }

        if let Some(object) = self.objects.get(virtual_path) {
            let object_path = self.assets_dir.join("objects").join(&object.hash[0..2]).join(&object.hash);
            return Ok(std::fs::read(object_path)?);
        }

        let jar_entry_name = format!("assets/{virtual_path}");
        if let Ok(mut entry) = self.client_jar.by_name(&jar_entry_name) {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf)?;
            return Ok(buf);
        }

        for layer in &self.layers[self.above_vanilla..] {
            if let Some(bytes) = layer.read(&pfad) {
                return Ok(bytes);
            }
        }

        Err(AssetsError::AssetNotFound { virtual_path: virtual_path.to_string() })
    }

    // Blockstate-Dateien: Vanilla stapelt die Dateien aller Packs und nimmt je Zustand die
    // oberste passende Variante; was sich nicht lesen laesst, faellt dabei weg. Alte Packs aus
    // 1.8 schreiben noch "normal" statt "" und Modelle ohne "block/" -- ihre Datei verdeckte
    // sonst die darunter, und der Block war schwarz-lila. Hier gilt die oberste Datei, die
    // eine lesbare Variante hat und deren Modelle es gibt; ein Pack, dessen Blockstate auf ein
    // fehlendes Modell zeigt, faellt also auf die Ebene darunter zurueck statt auf das
    // Fehlermodell wie in Vanilla.
    fn load_blockstate(&mut self, virtual_path: &str) -> Result<Vec<u8>, AssetsError> {
        let pfad = format!("assets/{virtual_path}");
        let oben: Vec<Vec<u8>> = self.layers[..self.above_vanilla].iter().filter_map(|l| l.read(&pfad)).collect();
        for bytes in oben {
            if self.blockstate_usable(&bytes) {
                return Ok(bytes);
            }
        }
        if let Some(object) = self.objects.get(virtual_path) {
            let object_path = self.assets_dir.join("objects").join(&object.hash[0..2]).join(&object.hash);
            return Ok(std::fs::read(object_path)?);
        }
        if let Ok(mut entry) = self.client_jar.by_name(&pfad) {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf)?;
            return Ok(buf);
        }
        let unten: Vec<Vec<u8>> = self.layers[self.above_vanilla..].iter().filter_map(|l| l.read(&pfad)).collect();
        for bytes in unten {
            if self.blockstate_usable(&bytes) {
                return Ok(bytes);
            }
        }
        Err(AssetsError::AssetNotFound { virtual_path: virtual_path.to_string() })
    }

    fn blockstate_usable(&mut self, bytes: &[u8]) -> bool {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(strip_bom(bytes)) else { return false };
        if v.get("multipart").is_some_and(|m| m.is_array()) {
            return true;
        }
        let Some(varianten) = v.get("variants").and_then(|v| v.as_object()) else { return false };
        // Ein Schluessel nennt Eigenschaften ("a=b,c=d") oder ist leer; "normal" nennt keine.
        let lesbar: Vec<&serde_json::Value> = varianten.iter().filter(|(k, _)| k.split(',').all(|t| t.is_empty() || t.contains('='))).map(|(_, v)| v).collect();
        let modelle: Vec<String> = lesbar
            .iter()
            .flat_map(|v| match v {
                serde_json::Value::Array(a) => a.iter().collect::<Vec<_>>(),
                v => vec![*v],
            })
            .filter_map(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string))
            .collect();
        !lesbar.is_empty() && modelle.iter().all(|m| {
            let (ns, p) = m.split_once(':').unwrap_or(("minecraft", m));
            self.load_bytes(&format!("{ns}/models/{p}.json")).is_ok()
        })
    }

    /// Breite und Hoehe des Vanilla-Bildes unter diesem Pfad, ohne Resourcepacks. GUI-Sheets
    /// werden in Vanilla-Pixeln adressiert; ein hochaufgeloestes Pack-Bild ist nur feiner
    /// gerastert, nicht groesser (s. ui_atlas::AtlasRect). Liest nur den PNG-Kopf.
    pub fn vanilla_image_size(&mut self, virtual_path: &str) -> Option<(u32, u32)> {
        let mut kopf = [0u8; 24];
        if let Some(object) = self.objects.get(virtual_path) {
            let object_path = self.assets_dir.join("objects").join(&object.hash[0..2]).join(&object.hash);
            std::io::Read::read_exact(&mut File::open(object_path).ok()?, &mut kopf).ok()?;
        } else {
            let mut entry = self.client_jar.by_name(&format!("assets/{virtual_path}")).ok()?;
            std::io::Read::read_exact(&mut entry, &mut kopf).ok()?;
        }
        png_size(&kopf)
    }

    /// Vanillas eigenes Pack-Bild (pack.png in der Wurzel des Client-Jars), roh.
    pub fn vanilla_pack_icon(&mut self) -> Option<Vec<u8>> {
        let mut entry = self.client_jar.by_name("pack.png").ok()?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf).ok()?;
        Some(buf)
    }

    /// Bringt ein Resourcepack diese Datei mit (statt nur Vanilla)? Fuer Dinge, die dieser
    /// Client selbst anders macht als Vanilla und nur dann nachbaut, wenn ein Pack sie
    /// ausdruecklich ersetzt -- die Core-Shader (s. render::vanilla_shader).
    pub fn pack_provides(&self, virtual_path: &str) -> bool {
        // Unter Vanilla ersetzt ein Pack nichts, was Vanilla selbst hat.
        let pfad = format!("assets/{virtual_path}");
        self.layers[..self.above_vanilla].iter().any(|layer| layer.read(&pfad).is_some())
    }

    /// Wie `load_bytes`, aber ALLE Ebenen statt nur der obersten -- Vanilla zuerst, das
    /// oberste Resourcepack zuletzt.
    ///
    /// Der Unterschied ist keine Feinheit, sondern der Unterschied zwischen "ersetzen" und
    /// "ergaenzen". Fuer eine Textur ist die oberste Ebene die richtige Antwort: es gibt genau
    /// ein Bild fuer `stone.png`. Fuer eine LISTE ist sie es nicht -- `sounds.json` bringt
    /// jede Ebene eigene Eintraege mit, und Vanilla fuehrt sie zusammen, statt die untere
    /// wegzuwerfen (s. SoundManager.SoundList.register).
    ///
    /// ECHTER BUG gewesen: mit `load_bytes` gelesen, lieferte ein Resourcepack mit eigener
    /// sounds.json (19 Eintraege) genau 19 bekannte Klaenge -- Vanillas uebrige 1818 hat der
    /// Client nie gesehen, und im Spiel blieb praktisch alles stumm.
    ///
    /// Vanilla und Client-Jar zaehlen zusammen als EINE Ebene (das Jar ist Vanillas
    /// eingebautes Standard-Pack, der Objects-Store dieselben Daten fuer diese Version) --
    /// sonst stuenden Vanillas Eintraege doppelt in jeder zusammengefuehrten Liste.
    pub fn load_layers(&mut self, virtual_path: &str) -> Vec<Vec<u8>> {
        let mut ebenen = Vec::new();
        let pfad = format!("assets/{virtual_path}");
        for layer in self.layers[self.above_vanilla..].iter().rev() {
            if let Some(bytes) = layer.read(&pfad) {
                ebenen.push(bytes);
            }
        }

        if let Some(object) = self.objects.get(virtual_path) {
            let object_path = self.assets_dir.join("objects").join(&object.hash[0..2]).join(&object.hash);
            if let Ok(bytes) = std::fs::read(object_path) {
                ebenen.push(bytes);
            }
        } else if let Ok(mut entry) = self.client_jar.by_name(&format!("assets/{virtual_path}")) {
            let mut buf = Vec::new();
            if std::io::Read::read_to_end(&mut entry, &mut buf).is_ok() {
                ebenen.push(buf);
            }
        }

        // Von unten nach oben: das oberste Pack kommt zuletzt, wie Vanillas Reihenfolge.
        for layer in self.layers[..self.above_vanilla].iter().rev() {
            if let Some(bytes) = layer.read(&pfad) {
                ebenen.push(bytes);
            }
        }

        ebenen
    }

    /// Die IDs ("ns:pfad") aller Item-Definitionen (`assets/<ns>/items/<pfad>.json`) aus Jar und
    /// Packs.
    pub fn item_definition_ids(&mut self) -> Vec<String> {
        self.json_ids("items")
    }

    // Alle "ns:pfad" zu `assets/<ns>/<ordner>/<pfad>.json` aller Ebenen, Jar zuerst.
    fn json_ids(&mut self, ordner: &str) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        let mut gesehen = std::collections::HashSet::new();
        let mut merke = |ns: &str, rest: &str| {
            if let Some(pfad) = rest.strip_suffix(".json") {
                let id = format!("{ns}:{pfad}");
                if gesehen.insert(id.clone()) {
                    ids.push(id);
                }
            }
        };
        let trenner = format!("/{ordner}/");
        for i in 0..self.client_jar.len() {
            let Some(name) = self.client_jar.name_for_index(i) else { continue };
            if let Some((ns, rest)) = name.strip_prefix("assets/").and_then(|r| r.split_once(trenner.as_str())) {
                if !ns.contains('/') {
                    merke(ns, rest);
                }
            }
        }
        for layer in &self.layers {
            for ns in layer_namespaces(layer) {
                for rest in layer.list(&format!("assets/{ns}/{ordner}/")) {
                    merke(&ns, &rest);
                }
            }
        }
        ids
    }

    /// Alle Schriftdefinitionen (`assets/<ns>/font/<pfad>.json`) aller Ebenen: je Schrift-ID
    /// ("ns:pfad") die Dateien von unten nach oben, Vanilla (Jar) zuerst -- wie load_layers.
    pub fn font_definitions(&mut self) -> Vec<(String, Vec<Vec<u8>>)> {
        self.json_ids("font")
            .into_iter()
            .map(|id| {
                let (ns, pfad) = id.split_once(':').unwrap_or(("minecraft", &id));
                let dateien = self.load_layers(&format!("{ns}/font/{pfad}.json"));
                (id, dateien)
            })
            .collect()
    }

    pub fn load_texture(&mut self, virtual_path: &str) -> Result<DecodedTexture, AssetsError> {
        let bytes = match self.load_bytes(virtual_path) {
            Ok(b) => b,
            // Keine Datei dieses Namens: vielleicht ein Sprite, den eine Atlas-Definition
            // woanders herholt (s. atlas_sources).
            Err(e) => return self.load_atlas_sprite(virtual_path).ok_or(e),
        };
        Self::decode_texture(&bytes)
    }

    fn decode_texture(bytes: &[u8]) -> Result<DecodedTexture, AssetsError> {
        match image::load_from_memory(bytes) {
            Ok(img) => {
                let img = img.to_rgba8();
                let (width, height) = img.dimensions();
                Ok(DecodedTexture { width, height, rgba: img.into_raw() })
            }
            Err(e) => decode_png_lenient(bytes).ok_or(AssetsError::Image(e)),
        }
    }

    // Alle Atlas-Definitionen aller Ebenen einlesen (einmal).
    fn atlas_sources(&mut self) -> &crate::atlas_sources::AtlasSources {
        if self.atlas_sources.is_none() {
            let mut quellen = crate::atlas_sources::AtlasSources::default();
            for id in self.json_ids("atlases") {
                let (ns, pfad) = id.split_once(':').unwrap_or(("minecraft", &id));
                for datei in self.load_layers(&format!("{ns}/atlases/{pfad}.json")) {
                    quellen.add_file(&datei);
                }
            }
            self.atlas_sources = Some(quellen);
        }
        self.atlas_sources.get_or_insert_with(Default::default)
    }

    fn load_atlas_sprite(&mut self, virtual_path: &str) -> Option<DecodedTexture> {
        use crate::atlas_sources::SpriteSource;
        let quellen = self.atlas_sources();
        if quellen.is_empty() {
            return None;
        }
        for quelle in quellen.lookup(virtual_path) {
            match quelle {
                SpriteSource::File(pfad) => {
                    if let Some(t) = self.load_bytes(&pfad).ok().and_then(|b| Self::decode_texture(&b).ok()) {
                        return Some(t);
                    }
                }
                SpriteSource::Region { image, divisor, rect } => {
                    if !self.sheets.contains_key(&image) {
                        let bild = self.load_bytes(&image).ok().and_then(|b| Self::decode_texture(&b).ok());
                        self.sheets.insert(image.clone(), bild);
                    }
                    let Some(Some(bild)) = self.sheets.get(&image) else { continue };
                    let (x, y, w, h) = crate::atlas_sources::region_pixels((bild.width, bild.height), divisor, rect);
                    if w == 0 || h == 0 {
                        continue;
                    }
                    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
                    for zeile in y..y + h {
                        let start = ((zeile * bild.width + x) * 4) as usize;
                        rgba.extend_from_slice(&bild.rgba[start..start + (w * 4) as usize]);
                    }
                    return Some(DecodedTexture { width: w, height: h, rgba });
                }
            }
        }
        None
    }
}

// Groesse aus dem IHDR-Block, der bei PNG immer direkt auf die Signatur folgt.
fn png_size(kopf: &[u8; 24]) -> Option<(u32, u32)> {
    if &kopf[..8] != b"\x89PNG\r\n\x1a\n" || &kopf[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(kopf[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(kopf[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

// PNG ohne Pruefsummen-Kontrolle. Server-Packs nullen gern die CRCs ihrer Bilder, damit
// gewoehnliche Werkzeuge sie nicht oeffnen; Minecrafts Bildleser (stb_image) prueft sie nicht
// und zeigt sie trotzdem an.
fn decode_png_lenient(bytes: &[u8]) -> Option<DecodedTexture> {
    let mut options = png::DecodeOptions::default();
    options.set_ignore_crc(true);
    options.set_ignore_adler32(true);
    let mut decoder = png::Decoder::new_with_options(std::io::Cursor::new(bytes), options);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (width, height) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == (width * height * 4) as usize).then_some(DecodedTexture { width, height, rgba })
}

impl MinecraftInstallation {
}

// Ein nachgeladenes Skin-PNG in 64x64-RGBA umwandeln. Eigene Funktion statt load_texture,
// weil der Skin nicht aus der Installation kommt, sondern aus dem Netz (s. auth::
// fetch_skin_png) -- die Bildbibliothek steckt aber hier.
//
// Alte 64x32-Skins (vor 1.8) werden auf 64x64 gelegt und der Rest transparent gelassen: die
// zweite Lage (Jacke, Aermel, Hosen) gab es damals nicht, und die Modell-Aussenlagen sampeln
// dann eben Nichts statt Muell.
// Ein Cape-Bild. Anders als Skins werden Capes NICHT umgeschrieben: Vanilla ruft den
// Remapper nur fuer den Typ SKIN auf (PlayerSkinProvider, nur beim Typ SKIN), Capes
// landen so, wie sie kommen, in der Textur.
//
// Deshalb ist hier auch die Groesse fest: Mojangs Capes sind 64x32 (an einem echten Abruf
// nachgeprueft), und der Atlas-Platz ist genau so gross. Ein abweichendes Bild wird
// abgelehnt statt verzerrt -- ein falsch skalierter Umhang faellt sonst als "Textur kaputt"
// auf, ohne dass man die Ursache sieht.
pub fn decode_cape_png(png: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    if img.dimensions() != (64, 32) {
        return None;
    }
    Some(img.into_raw())
}

pub fn decode_skin_png(png: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    if width != 64 || (height != 64 && height != 32) {
        return None;
    }
    let src = img.into_raw();
    let mut out = vec![0u8; (64 * 64 * 4) as usize];
    let copy_rows = height.min(64) as usize;
    out[..copy_rows * 64 * 4].copy_from_slice(&src[..copy_rows * 64 * 4]);
    prepare_skin(&mut out, height == 32);
    Some(out)
}

// Was Vanilla nach dem Laden mit jedem Skin macht (SkinTextureDownloader):
//   - Alte 64x32-Skins haben keine eigenen linken Gliedmassen. Sie bekommen die rechten,
//     gespiegelt, in die untere Haelfte -- sonst fehlten linker Arm und linkes Bein ganz.
//   - Die Grundschicht (Kopf, Koerper, Arme, Beine) ist immer deckend: ein Alpha darin wird
//     verworfen, sonst waeren NPC-Skins mit durchsichtiger Grundschicht unsichtbar.
//   - Bei 64x32 eine ganz deckende Hutschicht (alte Skins ohne Transparenz) ganz entfernen.
fn prepare_skin(rgba: &mut [u8], legacy: bool) {
    let idx = |x: usize, y: usize| (y * 64 + x) * 4;
    if legacy {
        // (Quelle x, y, Ziel x, y, Breite, Hoehe): Bein- und Armseiten, je waagrecht gespiegelt.
        const SPIEGEL: [(usize, usize, usize, usize, usize, usize); 12] = [
            (4, 16, 20, 48, 4, 4),
            (8, 16, 24, 48, 4, 4),
            (0, 20, 24, 52, 4, 12),
            (4, 20, 20, 52, 4, 12),
            (8, 20, 16, 52, 4, 12),
            (12, 20, 28, 52, 4, 12),
            (44, 16, 36, 48, 4, 4),
            (48, 16, 40, 48, 4, 4),
            (40, 20, 40, 52, 4, 12),
            (44, 20, 36, 52, 4, 12),
            (48, 20, 32, 52, 4, 12),
            (52, 20, 44, 52, 4, 12),
        ];
        for (sx, sy, zx, zy, w, h) in SPIEGEL {
            for y in 0..h {
                for x in 0..w {
                    let (q, z) = (idx(sx + x, sy + y), idx(zx + w - 1 - x, zy + y));
                    let px: [u8; 4] = rgba[q..q + 4].try_into().unwrap_or_default();
                    rgba[z..z + 4].copy_from_slice(&px);
                }
            }
        }
    }
    let deckend = |rgba: &mut [u8], (x0, y0, x1, y1): (usize, usize, usize, usize)| {
        for y in y0..y1 {
            for x in x0..x1 {
                rgba[idx(x, y) + 3] = 255;
            }
        }
    };
    deckend(rgba, (0, 0, 32, 16));
    if legacy && (0..32).all(|y| (32..64).all(|x| rgba[idx(x, y) + 3] >= 128)) {
        for y in 0..32 {
            for x in 32..64 {
                rgba[idx(x, y) + 3] = 0;
            }
        }
    }
    deckend(rgba, (0, 16, 64, 32));
    deckend(rgba, (16, 48, 48, 64));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ausgeschaltetes_fehlt_im_pack() {
        let dir = std::env::temp_dir().join(format!("coral-client-pack-test-{}", std::process::id()));
        let tex = dir.join("assets/minecraft/textures/block");
        std::fs::create_dir_all(&tex).unwrap();
        std::fs::write(tex.join("stone.png"), b"a").unwrap();
        std::fs::write(tex.join("dirt.png"), b"b").unwrap();
        std::fs::write(dir.join("assets/minecraft/sounds.json"), b"{}").unwrap();
        let layer = |aus: &[&str]| pack_layers(&dir, aus.iter().map(|s| s.to_string()).collect()).pop().unwrap();
        let alles = layer(&[]);
        assert!(alles.read("assets/minecraft/textures/block/stone.png").is_some());
        let ohne_ordner = layer(&["textures/block"]);
        assert!(ohne_ordner.read("assets/minecraft/textures/block/stone.png").is_none());
        assert!(ohne_ordner.read("assets/minecraft/sounds.json").is_some());
        assert!(ohne_ordner.list("assets/minecraft/textures/").is_empty());
        let ohne_datei = layer(&["textures/block/stone.png"]);
        assert_eq!(ohne_datei.list("assets/minecraft/textures/block/"), vec!["dirt.png".to_string()]);
        // Nur ganze Namen: "textures/bl" schaltet "textures/block" nicht mit aus.
        assert!(layer(&["textures/bl"]).read("assets/minecraft/textures/block/dirt.png").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alter_skin_bekommt_linke_gliedmassen() {
        let mut rgba = vec![0u8; 64 * 64 * 4];
        // Rechtes Bein, Vorderseite (4..8, 20..32) links rot, rechts gruen.
        for y in 20..32 {
            for x in 4..8 {
                let i = (y * 64 + x) * 4;
                rgba[i..i + 4].copy_from_slice(if x == 4 { &[255, 0, 0, 255] } else { &[0, 255, 0, 255] });
            }
        }
        prepare_skin(&mut rgba, true);
        // Linkes Bein, Vorderseite (20..24, 52..64): gespiegelt, rot also rechts.
        let px = |x: usize, y: usize| rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].to_vec();
        assert_eq!(px(23, 52), vec![255, 0, 0, 255]);
        assert_eq!(px(20, 52), vec![0, 255, 0, 255]);
        // Grundschicht deckend, auch wo der Skin durchsichtig war.
        assert_eq!(px(40, 20)[3], 255);
        assert_eq!(px(0, 0)[3], 255);
    }

    // Alte Blockstates ("normal", Modell ohne "block/") verdecken die darunter nicht.
    #[test]
    fn unbrauchbarer_blockstate_faellt_durch() {
        let dir = std::env::temp_dir().join(format!("coral-client-bs-test-{}", std::process::id()));
        let bs = dir.join("assets/minecraft/blockstates");
        std::fs::create_dir_all(&bs).unwrap();
        std::fs::write(bs.join("stone.json"), br#"{"variants":{"normal":{"model":"stone"}}}"#).unwrap();
        std::fs::write(bs.join("grass_block.json"), br#"{"variants":{"snowy=false":{"model":"grass_normal"},"snowy=true":{"model":"grass_snowed"}}}"#).unwrap();
        std::fs::write(bs.join("dirt.json"), br#"{"variants":{"":[{"model":"minecraft:block/dirt"},{"model":"block/dirt","y":90}]}}"#).unwrap();
        let Ok(mut mc) = MinecraftInstallation::locate("1.21.11", [dir.clone()]) else { return };
        let gelesen = |mc: &mut MinecraftInstallation, b: &str| String::from_utf8(mc.load_bytes(&format!("minecraft/blockstates/{b}.json")).unwrap()).unwrap();
        assert!(!gelesen(&mut mc, "stone").contains("normal"));
        assert!(!gelesen(&mut mc, "grass_block").contains("grass_normal"));
        assert!(gelesen(&mut mc, "dirt").contains("\"y\":90"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
