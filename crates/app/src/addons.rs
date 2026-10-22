// Addons: eigene Programme in ~/coral-client/mods, die neue Module ins Modul-Menue bringen.
//
// Jedes ausfuehrbare Programm im Ordner wird beim Start mitgestartet. Der Client spricht mit
// ihm ueber JSON, eine Nachricht je Zeile: an stdin schreibt der Client, von stdout liest er,
// was stderr ausgibt landet im Log. Ein Addon meldet sich mit "register" an (Kategorie,
// Module, deren Einstellungen und welche Ereignisse es hoeren will); danach schickt der
// Client ihm den Stand seiner Einstellungen und die Ereignisse, und es kann zeichnen, Chat
// senden, Toene spielen ... Das Protokoll steht in addons/README.md, ein lauffaehiges Geruest
// in addons/template.
//
// Eigene Prozesse statt geladener Bibliotheken: ein abstuerzendes Addon reisst den Client
// nicht mit, und es darf in jeder Sprache geschrieben sein.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};

// Version des Protokolls, steht im "hello".
pub const API_VERSION: u32 = 1;

// Eine Einstellung eines Addon-Moduls, wie das Addon sie angemeldet hat.
#[derive(Clone, Debug)]
pub enum SettingKind {
    Toggle,
    Slider { min: f64, max: f64, step: f64, unit: String },
    Choice { options: Vec<String> },
    Color,
    // Kein Wert: ein Klick geht als Ereignis ans Addon.
    Button,
}

#[derive(Clone, Debug)]
pub struct SettingDef {
    pub key: String,
    pub label: String,
    pub kind: SettingKind,
    // Startwert als Text (s. Addon::values), leer beim Knopf.
    pub default: String,
}

#[derive(Clone, Debug)]
pub struct ModuleDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub settings: Vec<SettingDef>,
    // Was das Modul gerade ins HUD zeichnet (s. "hud").
    pub hud: Vec<HudItem>,
}

// Ecke bzw. Mitte des Bildschirms, auf die sich x/y eines HUD-Elements beziehen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

#[derive(Clone, Debug)]
pub enum HudItem {
    Text { text: String, x: f32, y: f32, color: u32, anchor: Anchor },
    Rect { x: f32, y: f32, w: f32, h: f32, color: u32, alpha: f32, anchor: Anchor },
}

// Was ein Addon vom Client will.
#[derive(Debug)]
pub enum Request {
    Register { name: String, modules: Vec<ModuleDef>, subscribe: Vec<String> },
    Hud { module: String, items: Vec<HudItem> },
    Chat { text: String },
    Notify { title: String, text: String },
    Sound { event: String, volume: f32, pitch: f32 },
    Log { text: String },
    // Eine eigene Einstellung setzen (z.B. nach einem Knopfdruck).
    Set { module: String, key: String, value: String },
}

// Aus dem Lese-Thread an den Hauptthread: (Addon, Nachricht).
pub enum Incoming {
    Request(usize, Request),
    // Eine Zeile, die kein gueltiges Protokoll ist, oder stderr.
    Log(usize, String),
    Exited(usize),
}

pub struct Addon {
    // Dateiname ohne Endung, nur [a-z0-9_-] -- Schluessel in der Config.
    pub id: String,
    // Kategorie im Modul-Menue (aus "register", bis dahin die id).
    pub name: String,
    pub modules: Vec<ModuleDef>,
    pub subscribe: Vec<String>,
    pub registered: bool,
    stdin: Option<ChildStdin>,
    child: Child,
}

impl Addon {
    // Eine Nachricht an das Addon. Ist es weg, wird es still -- es meldet sich ueber Exited.
    pub fn send(&mut self, msg: &Value) {
        let Some(stdin) = self.stdin.as_mut() else { return };
        let ok = writeln!(stdin, "{msg}").and_then(|_| stdin.flush()).is_ok();
        if !ok {
            self.stdin = None;
        }
    }

    pub fn wants(&self, event: &str) -> bool {
        self.subscribe.iter().any(|e| e == event)
    }
}

impl Drop for Addon {
    fn drop(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Addons {
    pub list: Vec<Addon>,
    pub rx: Receiver<Incoming>,
}

// Nur Zeichen, die in einem Config-Schluessel unbedenklich sind.
pub fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| c.to_ascii_lowercase())
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

#[cfg(unix)]
fn executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn executable(path: &std::path::Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

// Alle Programme im Ordner starten. Fehler je Addon landen im Log, der Rest laeuft weiter.
pub fn start(dir: &std::path::Path, log: &mut Vec<String>) -> Addons {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut list = Vec::new();
    let _ = std::fs::create_dir_all(dir);
    let mut pfade: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| executable(p)).collect();
    pfade.sort();
    for pfad in pfade {
        let id = sanitize_id(&pfad.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        if id.is_empty() || list.iter().any(|a: &Addon| a.id == id) {
            continue;
        }
        let mut cmd = Command::new(&pfad);
        cmd.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Kein eigenes Konsolenfenster je Addon.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                log.push(format!("Addon {id} startet nicht: {e}"));
                continue;
            }
        };
        let index = list.len();
        if let Some(out) = child.stdout.take() {
            spawn_reader(index, out, tx.clone(), true);
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(index, err, tx.clone(), false);
        }
        let stdin = child.stdin.take();
        let mut addon = Addon { id: id.clone(), name: id, modules: Vec::new(), subscribe: Vec::new(), registered: false, stdin, child };
        addon.send(&json!({ "type": "hello", "api": API_VERSION, "client": env!("CARGO_PKG_VERSION") }));
        log.push(format!("Addon {} gestartet", addon.id));
        list.push(addon);
    }
    Addons { list, rx }
}

fn spawn_reader(index: usize, stream: impl std::io::Read + Send + 'static, tx: Sender<Incoming>, protokoll: bool) {
    std::thread::spawn(move || {
        for zeile in BufReader::new(stream).lines() {
            let Ok(zeile) = zeile else { break };
            if zeile.trim().is_empty() {
                continue;
            }
            let nachricht = if protokoll {
                match serde_json::from_str::<Value>(&zeile).ok().and_then(|v| parse_request(&v)) {
                    Some(r) => Incoming::Request(index, r),
                    None => Incoming::Log(index, zeile),
                }
            } else {
                Incoming::Log(index, zeile)
            };
            if tx.send(nachricht).is_err() {
                return;
            }
        }
        if protokoll {
            let _ = tx.send(Incoming::Exited(index));
        }
    });
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn number(v: &Value, key: &str, fallback: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(fallback)
}

// "#RRGGBB" oder "RRGGBB" -> 0xRRGGBB.
pub fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    (s.len() == 6).then(|| u32::from_str_radix(s, 16).ok()).flatten()
}

fn parse_anchor(s: &str) -> Anchor {
    match s {
        "top_right" => Anchor::TopRight,
        "bottom_left" => Anchor::BottomLeft,
        "bottom_right" => Anchor::BottomRight,
        "center" => Anchor::Center,
        _ => Anchor::TopLeft,
    }
}

// Ein Wert, wie ihn das Addon schickt (Zahl, Wahrheitswert, Text), als gespeicherter Text.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn parse_setting(v: &Value) -> Option<SettingDef> {
    let key = sanitize_id(v.get("key")?.as_str()?);
    let label = v.get("label").and_then(Value::as_str).map_or_else(|| key.clone(), str::to_string);
    let default = v.get("default").map(value_text).unwrap_or_default();
    let kind = match v.get("type")?.as_str()? {
        "toggle" => SettingKind::Toggle,
        "slider" => {
            let min = number(v, "min", 0.0);
            let max = number(v, "max", 100.0).max(min);
            SettingKind::Slider { min, max, step: number(v, "step", 1.0).abs().max(1e-6), unit: text(v, "unit") }
        }
        "choice" => {
            let options: Vec<String> = v.get("options")?.as_array()?.iter().filter_map(Value::as_str).map(str::to_string).collect();
            if options.is_empty() {
                return None;
            }
            SettingKind::Choice { options }
        }
        "color" => SettingKind::Color,
        "button" => SettingKind::Button,
        _ => return None,
    };
    Some(SettingDef { key, label, kind, default })
}

fn parse_hud_item(v: &Value) -> Option<HudItem> {
    let anchor = parse_anchor(v.get("anchor").and_then(Value::as_str).unwrap_or("top_left"));
    let color = v.get("color").and_then(Value::as_str).and_then(parse_color).unwrap_or(0xFF_FFFF);
    let (x, y) = (number(v, "x", 0.0) as f32, number(v, "y", 0.0) as f32);
    match v.get("kind")?.as_str()? {
        "text" => Some(HudItem::Text { text: text(v, "text"), x, y, color, anchor }),
        "rect" => Some(HudItem::Rect {
            x,
            y,
            w: number(v, "w", 0.0) as f32,
            h: number(v, "h", 0.0) as f32,
            color,
            alpha: number(v, "alpha", 1.0).clamp(0.0, 1.0) as f32,
            anchor,
        }),
        _ => None,
    }
}

pub fn parse_request(v: &Value) -> Option<Request> {
    Some(match v.get("type")?.as_str()? {
        "register" => {
            let modules = v
                .get("modules")?
                .as_array()?
                .iter()
                .filter_map(|m| {
                    let id = sanitize_id(m.get("id")?.as_str()?);
                    Some(ModuleDef {
                        name: m.get("name").and_then(Value::as_str).map_or_else(|| id.clone(), str::to_string),
                        description: text(m, "description"),
                        settings: m.get("settings").and_then(Value::as_array).map(|s| s.iter().filter_map(parse_setting).collect()).unwrap_or_default(),
                        hud: Vec::new(),
                        id,
                    })
                })
                .collect();
            let subscribe = v.get("subscribe").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            Request::Register { name: text(v, "name"), modules, subscribe }
        }
        "hud" => Request::Hud { module: sanitize_id(&text(v, "module")), items: v.get("items")?.as_array()?.iter().filter_map(parse_hud_item).collect() },
        "chat" => Request::Chat { text: text(v, "text") },
        "notify" => Request::Notify { title: text(v, "title"), text: text(v, "text") },
        "sound" => Request::Sound { event: text(v, "event"), volume: number(v, "volume", 1.0) as f32, pitch: number(v, "pitch", 1.0) as f32 },
        "log" => Request::Log { text: text(v, "text") },
        "set" => Request::Set { module: sanitize_id(&text(v, "module")), key: sanitize_id(&text(v, "key")), value: v.get("value").map(value_text).unwrap_or_default() },
        _ => return None,
    })
}

// Gespeicherter Text einer Einstellung -> JSON fuer das Addon.
pub fn value_json(kind: &SettingKind, s: &str) -> Value {
    match kind {
        SettingKind::Toggle => Value::Bool(s == "true"),
        SettingKind::Slider { .. } => s.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number),
        SettingKind::Choice { .. } | SettingKind::Color => Value::String(s.to_string()),
        SettingKind::Button => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anmeldung_wird_gelesen() {
        let v: Value = serde_json::from_str(
            r##"{"type":"register","name":"Mein Addon","subscribe":["chat"],"modules":[{"id":"Gruss!","name":"Gruß","settings":[
                {"type":"toggle","key":"an","default":true},
                {"type":"slider","key":"zahl","min":1,"max":9,"step":2,"default":3},
                {"type":"choice","key":"art","options":["a","b","c"],"default":"b"},
                {"type":"color","key":"farbe","default":"#FF8800"},
                {"type":"wackel","key":"x"}]}]}"##,
        )
        .unwrap();
        let Some(Request::Register { name, modules, subscribe }) = parse_request(&v) else { panic!() };
        assert_eq!(name, "Mein Addon");
        assert_eq!(subscribe, vec!["chat"]);
        assert_eq!(modules[0].id, "gruss_");
        assert_eq!(modules[0].settings.len(), 4, "unbekannte Arten fallen weg");
        assert_eq!(modules[0].settings[0].default, "true");
        assert_eq!(modules[0].settings[1].default, "3");
        assert_eq!(modules[0].settings[2].default, "b");
        assert_eq!(parse_color(&modules[0].settings[3].default), Some(0xFF8800));
    }

    // Der echte Weg: Programm im Ordner finden, starten, "hello" schicken, Anmeldung lesen.
    #[cfg(unix)]
    #[test]
    fn programm_wird_gestartet_und_meldet_sich_an() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("coral-addon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let skript = dir.join("Test Addon.sh");
        std::fs::write(&skript, "#!/bin/sh\nread hallo\necho '{\"type\":\"register\",\"name\":\"T\",\"modules\":[{\"id\":\"m\"}]}'\necho kaputt >&2\nread rest\n").unwrap();
        std::fs::set_permissions(&skript, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("liesmich.txt"), "kein Programm").unwrap();
        let mut log = Vec::new();
        let addons = start(&dir, &mut log);
        assert_eq!(addons.list.len(), 1, "{log:?}");
        assert_eq!(addons.list[0].id, "test_addon");
        let mut angemeldet = false;
        let mut stderr = false;
        let ende = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !(angemeldet && stderr) && std::time::Instant::now() < ende {
            match addons.rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Incoming::Request(0, Request::Register { name, modules, .. })) => angemeldet = name == "T" && modules.len() == 1,
                Ok(Incoming::Log(0, zeile)) => stderr = zeile == "kaputt",
                _ => {}
            }
        }
        drop(addons);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(angemeldet && stderr);
    }

    #[test]
    fn hud_und_aktionen() {
        let v: Value = serde_json::from_str(r#"{"type":"hud","module":"m","items":[{"kind":"text","text":"hi","x":4,"y":5,"anchor":"bottom_right","color":"00FF00"},{"kind":"blob"}]}"#).unwrap();
        let Some(Request::Hud { items, .. }) = parse_request(&v) else { panic!() };
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], HudItem::Text { color: 0x00FF00, anchor: Anchor::BottomRight, .. }));
        assert!(parse_request(&serde_json::from_str(r#"{"type":"quatsch"}"#).unwrap()).is_none());
    }
}
