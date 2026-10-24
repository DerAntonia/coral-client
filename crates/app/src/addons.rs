// Addons: WebAssembly-Module (.wasm) in ~/coral-client/mods, die der Client in seinen Prozess
// laedt (wasmtime). Die Schnittstelle steht im Crate coral-addon (crates/addon_api): der Client
// ruft das Addon an festen Stellen auf (Hooks: Tick, Frame, Chat, Pakete, Tasten ...), das Addon
// darf dort abbrechen oder ersetzen und fragt waehrenddessen den Spielzustand ab bzw. loest
// Aktionen aus. Ein lauffaehiges Beispiel steht in addons/template, die Doku in addons/README.md.
//
// WASM statt nativer Bibliotheken: eine Datei laeuft unter Linux und Windows, und ein Fehler
// im Addon (Absturz, Endlosschleife, zu viel Speicher) beendet nur das Addon, nicht den Client.
//
// Ablauf eines Aufrufs (s. Addon::call): Event kodieren, in Speicher schreiben, den das Addon
// mit coral_alloc hergibt, coral_event aufrufen. Ruft das Addon dabei client::* auf, landet das
// in "coral.call": Abfragen beantwortet der Host sofort (s. Host), Aktionen sammelt das Addon
// in Data::queued, und die App fuehrt sie nach dem Hook aus (s. App::addon_hook).

use coral_addon as api;
use std::sync::mpsc::Receiver;
use wasmtime::{Caller, Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

pub use api::{Anchor, HudItem};

// Rechenbudget je Aufruf in wasmtime-"Fuel" (grob eine Instruktion). Reicht fuer jede
// vernuenftige Arbeit, haelt eine Endlosschleife aber nach wenigen zehn Millisekunden an.
const FUEL_PER_CALL: u64 = 50_000_000;
// Mehr Speicher bekommt ein Addon nicht.
const MAX_MEMORY: usize = 256 << 20;

// Der Weg der Pakete zum Netzwerk-Task. Hoert ein Addon Hook::PacketOut, haelt send() die
// Pakete zurueck, bis App::flush_addon_packets sie durch den Hook schickt; sonst gehen sie wie
// immer sofort raus. Gleicher Name und gleiche Form wie beim Sender, damit die vielen
// Aufrufstellen unveraendert bleiben.
#[derive(Clone)]
pub struct PacketTx {
    inner: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    held: std::rc::Rc<std::cell::RefCell<Option<Vec<Vec<u8>>>>>,
}

impl PacketTx {
    pub fn new(inner: tokio::sync::mpsc::UnboundedSender<Vec<u8>>, hold: bool) -> Self {
        PacketTx { inner, held: std::rc::Rc::new(std::cell::RefCell::new(hold.then(Vec::new))) }
    }

    pub fn send(&self, packet: Vec<u8>) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        if let Some(gehalten) = self.held.borrow_mut().as_mut() {
            gehalten.push(packet);
            return Ok(());
        }
        self.inner.send(packet)
    }

    // Am Hook vorbei (gefilterte Pakete, Call::SendPacket).
    pub fn send_direct(&self, packet: Vec<u8>) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        self.inner.send(packet)
    }

    pub fn hold(&self, on: bool) {
        let mut h = self.held.borrow_mut();
        match (on, h.take()) {
            (true, schon) => *h = Some(schon.unwrap_or_default()),
            // Abschalten: was noch wartet, geht ungefiltert raus.
            (false, Some(rest)) => {
                for p in rest {
                    let _ = self.inner.send(p);
                }
            }
            (false, None) => {}
        }
    }

    pub fn take_held(&self) -> Vec<Vec<u8>> {
        self.held.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default()
    }
}

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
    // Startwert als Text, wie er in der Config steht (s. value_text), leer beim Knopf.
    pub default: String,
}

#[derive(Clone, Debug)]
pub struct ModuleDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub settings: Vec<SettingDef>,
    // Was das Modul gerade ins HUD zeichnet (s. Call::Hud).
    pub hud: Vec<HudItem>,
}

// Was der Client fuer die Abfragen eines Addons bereitstellt (App, s. addon_host.rs).
pub trait Host {
    fn answer(&self, addon: &str, call: &api::Call) -> api::Answer;
}

// Zeiger auf den Host fuer die Dauer EINES Aufrufs (s. Addon::call). Roh, weil wasmtime nur
// 'static-Daten im Store erlaubt; ausserhalb von call() ist er None.
struct HostPtr(*const (dyn Host + 'static));

// SAFETY: der Store bleibt im Hauptthread; der Zeiger wird nur waehrend call() benutzt.
unsafe impl Send for HostPtr {}

struct Data {
    id: String,
    host: Option<HostPtr>,
    // Antwort des letzten "coral.call", abgeholt mit "coral.answer".
    answer: Vec<u8>,
    queued: Vec<api::Call>,
    limits: StoreLimits,
}

pub struct Addon {
    // Dateiname ohne Endung, nur [a-z0-9_-] -- Schluessel in der Config.
    pub id: String,
    // Kategorie im Modul-Menue (aus der Anmeldung).
    pub name: String,
    pub modules: Vec<ModuleDef>,
    pub hooks: Vec<api::Hook>,
    pub commands: Vec<String>,
    pub registered: bool,
    // Nach einem Fehler wird das Addon nicht mehr aufgerufen.
    pub dead: bool,
    store: Store<Data>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    free: TypedFunc<(u32, u32), ()>,
    event: TypedFunc<(u32, u32), u64>,
}

impl Addon {
    pub fn wants(&self, hook: api::Hook) -> bool {
        self.registered && !self.dead && self.hooks.contains(&hook)
    }

    // Ein Event an das Addon. Err (Absturz, Rechenbudget, kaputte Antwort) legt es still.
    pub fn call(&mut self, host: &dyn Host, event: &api::Event) -> Result<api::Reply, String> {
        if self.dead {
            return Err("ist abgeschaltet".into());
        }
        let ergebnis = self.call_inner(host, event);
        self.store.data_mut().host = None;
        if ergebnis.is_err() {
            self.dead = true;
            for m in &mut self.modules {
                m.hud.clear();
            }
        }
        ergebnis
    }

    fn call_inner(&mut self, host: &dyn Host, event: &api::Event) -> Result<api::Reply, String> {
        let bytes = api::encode(event);
        // SAFETY: nur die Lebensdauer wird vergessen; der Zeiger wird in call() wieder auf None
        // gesetzt, bevor `host` ungueltig werden kann.
        let ptr = unsafe { std::mem::transmute::<*const (dyn Host + '_), *const (dyn Host + 'static)>(host as *const dyn Host) };
        self.store.data_mut().host = Some(HostPtr(ptr));
        self.store.set_fuel(FUEL_PER_CALL).map_err(|e| e.to_string())?;
        let ziel = self.alloc.call(&mut self.store, bytes.len() as u32).map_err(|e| e.to_string())?;
        self.memory.write(&mut self.store, ziel as usize, &bytes).map_err(|e| e.to_string())?;
        let r = self.event.call(&mut self.store, (ziel, bytes.len() as u32)).map_err(|e| format!("{e:#}"))?;
        let (rp, rl) = ((r >> 32) as u32, r as u32);
        let mut antwort = vec![0u8; rl as usize];
        self.memory.read(&self.store, rp as usize, &mut antwort).map_err(|e| e.to_string())?;
        self.free.call(&mut self.store, (rp, rl)).map_err(|e| e.to_string())?;
        api::decode(&antwort).ok_or_else(|| "Antwort nicht lesbar".to_string())
    }

    // Die Aktionen, die das Addon seit dem letzten Abholen ausgeloest hat.
    pub fn take_queued(&mut self) -> Vec<api::Call> {
        std::mem::take(&mut self.store.data_mut().queued)
    }

    // Anmeldung uebernehmen (Namen bereinigt, s. sanitize_id).
    pub fn apply_registration(&mut self, reg: api::Registration) {
        self.name = if reg.name.trim().is_empty() { self.id.clone() } else { reg.name };
        self.hooks = reg.hooks;
        self.commands = reg.commands.iter().map(|c| c.trim_start_matches('/').to_lowercase()).filter(|c| !c.is_empty()).collect();
        self.modules = reg.modules.into_iter().map(module_def).collect();
        self.registered = true;
    }
}

fn module_def(m: api::Module) -> ModuleDef {
    let id = sanitize_id(&m.id);
    ModuleDef {
        name: if m.name.is_empty() { id.clone() } else { m.name },
        description: m.description,
        settings: m.settings.into_iter().filter_map(setting_def).collect(),
        hud: Vec::new(),
        id,
    }
}

fn setting_def(s: api::Setting) -> Option<SettingDef> {
    let key = sanitize_id(&s.key);
    let kind = match s.kind {
        api::SettingKind::Toggle => SettingKind::Toggle,
        api::SettingKind::Slider { min, max, step, unit } => {
            if !min.is_finite() || !max.is_finite() {
                return None;
            }
            SettingKind::Slider { min, max: max.max(min), step: if step.is_finite() { step.abs().max(1e-6) } else { 1.0 }, unit }
        }
        api::SettingKind::Choice { options } if !options.is_empty() => SettingKind::Choice { options },
        api::SettingKind::Choice { .. } => return None,
        api::SettingKind::Color => SettingKind::Color,
        api::SettingKind::Button => SettingKind::Button,
    };
    Some(SettingDef { label: if s.label.is_empty() { key.clone() } else { s.label }, default: value_text(&s.default), key, kind })
}

pub struct Addons {
    pub list: Vec<Addon>,
    engine: Engine,
    linker: Linker<Data>,
    // Fertig uebersetzte Module aus dem Lade-Thread: (id, Ergebnis).
    compiled: Receiver<(String, Result<Module, String>)>,
}

// Nur Zeichen, die in einem Config-Schluessel unbedenklich sind.
pub fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| c.to_ascii_lowercase())
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

// Engine und Linker anlegen und alle .wasm im Ordner im Hintergrund uebersetzen (Cranelift
// braucht je Addon einige hundert Millisekunden -- das soll den ersten Frame nicht aufhalten).
pub fn start(dir: &std::path::Path) -> Result<Addons, String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config).map_err(|e| e.to_string())?;
    let linker = linker(&engine).map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(dir);
    let mut pfade: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wasm"))).collect();
    pfade.sort();
    let (tx, compiled) = std::sync::mpsc::channel();
    let e = engine.clone();
    std::thread::spawn(move || {
        let mut gesehen = Vec::new();
        for pfad in pfade {
            let id = sanitize_id(&pfad.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            if id.is_empty() || gesehen.contains(&id) {
                continue;
            }
            gesehen.push(id.clone());
            let modul = Module::from_file(&e, &pfad).map_err(|e| format!("{e:#}"));
            if tx.send((id, modul)).is_err() {
                return;
            }
        }
    });
    Ok(Addons { list: Vec::new(), engine, linker, compiled })
}

impl Addons {
    // Fertig uebersetzte Module instanziieren. Neue Addons kommen hinten an die Liste; die
    // Indizes der vorhandenen bleiben (ModuleId::Addon zeigt darauf). Rueckgabe: Index des
    // neuen Addons bzw. Fehlertext.
    pub fn poll(&mut self) -> Vec<Result<usize, String>> {
        let mut neu = Vec::new();
        while let Ok((id, modul)) = self.compiled.try_recv() {
            neu.push(modul.and_then(|m| self.instantiate(id.clone(), &m)).map_err(|e| format!("Addon {id} laedt nicht: {e}")));
        }
        neu
    }

    fn instantiate(&mut self, id: String, module: &Module) -> Result<usize, String> {
        let data = Data { id: id.clone(), host: None, answer: Vec::new(), queued: Vec::new(), limits: StoreLimitsBuilder::new().memory_size(MAX_MEMORY).instances(1).build() };
        let mut store = Store::new(&self.engine, data);
        store.limiter(|d| &mut d.limits);
        store.set_fuel(FUEL_PER_CALL).map_err(|e| e.to_string())?;
        let instance = self.linker.instantiate(&mut store, module).map_err(|e| format!("{e:#}"))?;
        let memory = instance.get_memory(&mut store, "memory").ok_or("exportiert keinen Speicher")?;
        let alloc = instance.get_typed_func(&mut store, "coral_alloc").map_err(|e| e.to_string())?;
        let free = instance.get_typed_func(&mut store, "coral_free").map_err(|e| e.to_string())?;
        let event = instance.get_typed_func(&mut store, "coral_event").map_err(|e| e.to_string())?;
        self.list.push(Addon { name: id.clone(), id, modules: Vec::new(), hooks: Vec::new(), commands: Vec::new(), registered: false, dead: false, store, memory, alloc, free, event });
        Ok(self.list.len() - 1)
    }

    pub fn any_wants(&self, hook: api::Hook) -> bool {
        self.list.iter().any(|a| a.wants(hook))
    }
}

// Abfragen beantwortet der Host sofort; alles andere ist eine Aktion fuer nach dem Hook.
fn is_query(c: &api::Call) -> bool {
    matches!(c, api::Call::Player | api::Call::Inventory | api::Call::Entities { .. } | api::Call::Block { .. } | api::Call::Target | api::Call::Players | api::Call::Screen | api::Call::GetSetting { .. })
}

fn linker(engine: &Engine) -> wasmtime::Result<Linker<Data>> {
    let mut linker = Linker::new(engine);
    linker.func_wrap("coral", "call", |mut caller: Caller<'_, Data>, ptr: u32, len: u32| -> u32 {
        let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) else { return 0 };
        let mut buf = vec![0u8; len as usize];
        if mem.read(&caller, ptr as usize, &mut buf).is_err() {
            return 0;
        }
        let Some(call) = api::decode::<api::Call>(&buf) else { return 0 };
        let data = caller.data_mut();
        if !is_query(&call) {
            data.queued.push(call);
            data.answer.clear();
            return 0;
        }
        let antwort = match &data.host {
            // SAFETY: gesetzt nur waehrend Addon::call, solange der Host lebt (s. HostPtr).
            Some(h) => unsafe { (*h.0).answer(&data.id, &call) },
            None => api::Answer::None,
        };
        data.answer = if antwort == api::Answer::None { Vec::new() } else { api::encode(&antwort) };
        data.answer.len() as u32
    })?;
    linker.func_wrap("coral", "answer", |mut caller: Caller<'_, Data>, dst: u32| {
        let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) else { return };
        let (speicher, data) = mem.data_and_store_mut(&mut caller);
        let n = data.answer.len();
        if let Some(ziel) = speicher.get_mut(dst as usize..dst as usize + n) {
            ziel.copy_from_slice(&data.answer);
        }
    })?;
    Ok(linker)
}

// "#RRGGBB" oder "RRGGBB" -> 0xRRGGBB.
pub fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    (s.len() == 6).then(|| u32::from_str_radix(s, 16).ok()).flatten()
}

// Wert des Addons -> gespeicherter Text.
pub fn value_text(v: &api::Value) -> String {
    match v {
        api::Value::None => String::new(),
        api::Value::Bool(b) => b.to_string(),
        api::Value::Number(n) => n.to_string(),
        api::Value::Text(s) => s.clone(),
    }
}

// Gespeicherter Text -> Wert fuer das Addon.
pub fn value_of(kind: &SettingKind, s: &str) -> api::Value {
    match kind {
        SettingKind::Toggle => api::Value::Bool(s == "true"),
        SettingKind::Slider { .. } => s.parse::<f64>().map_or(api::Value::None, api::Value::Number),
        SettingKind::Choice { .. } | SettingKind::Color => api::Value::Text(s.to_string()),
        SettingKind::Button => api::Value::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHost;

    impl Host for TestHost {
        fn answer(&self, addon: &str, call: &api::Call) -> api::Answer {
            match call {
                api::Call::Block { x: 1, .. } => api::Answer::Block(Some(format!("minecraft:stone fuer {addon}"))),
                _ => api::Answer::None,
            }
        }
    }

    // Ein Addon in WAT, das bei jedem Event erst eine Abfrage und eine Aktion schickt und dann
    // eine feste Antwort gibt -- die Bytes dafuer kodiert der Test selbst.
    fn test_addon(reply: &api::Reply) -> Vec<u8> {
        let abfrage = api::encode(&api::Call::Block { x: 1, y: 2, z: 3 });
        let aktion = api::encode(&api::Call::Chat { text: "hallo".into() });
        let antwort = api::encode(reply);
        let esc = |b: &[u8]| b.iter().map(|x| format!("\\{x:02x}")).collect::<String>();
        let wat = format!(
            r#"(module
                (import "coral" "call" (func $call (param i32 i32) (result i32)))
                (import "coral" "answer" (func $answer (param i32)))
                (memory (export "memory") 1)
                (global $next (mut i32) (i32.const 4096))
                (data (i32.const 0) "{q}")
                (data (i32.const 256) "{a}")
                (data (i32.const 512) "{r}")
                (func (export "coral_alloc") (param i32) (result i32)
                    (local $p i32)
                    (local.set $p (global.get $next))
                    (global.set $next (i32.add (global.get $next) (local.get 0)))
                    (local.get $p))
                (func (export "coral_free") (param i32 i32))
                (func (export "coral_event") (param i32 i32) (result i64)
                    (drop (call $call (i32.const 0) (i32.const {ql})))
                    (call $answer (i32.const 1024))
                    (drop (call $call (i32.const 256) (i32.const {al})))
                    (i64.or (i64.const {rp_shift}) (i64.const {rl}))))"#,
            q = esc(&abfrage),
            ql = abfrage.len(),
            a = esc(&aktion),
            al = aktion.len(),
            r = esc(&antwort),
            rp_shift = 512u64 << 32,
            rl = antwort.len(),
        );
        wat::parse_str(&wat).unwrap()
    }

    fn laden(name: &str, wasm: Vec<u8>) -> Addons {
        let dir = std::env::temp_dir().join(format!("coral-addon-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Mein Addon.wasm"), wasm).unwrap();
        std::fs::write(dir.join("liesmich.txt"), "kein Addon").unwrap();
        let mut addons = start(&dir).unwrap();
        let ende = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while addons.list.is_empty() && std::time::Instant::now() < ende {
            for r in addons.poll() {
                r.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_dir_all(&dir);
        addons
    }

    #[test]
    fn anmeldung_abfrage_und_aktion() {
        let reg = api::Registration {
            name: "Mein Addon".into(),
            hooks: vec![api::Hook::ChatIn],
            modules: vec![api::Module { id: "Gruss!".into(), name: "Gruß".into(), description: String::new(), settings: vec![api::Setting::slider("zahl", "Zahl", 1.0, 9.0, 2.0, "", 3.0), api::Setting::choice("leer", "Leer", &[], "")] }],
            commands: vec!["/Hallo".into()],
        };
        let mut addons = laden("anmeldung", test_addon(&api::Reply::Registration(reg)));
        assert_eq!(addons.list.len(), 1);
        let a = &mut addons.list[0];
        assert_eq!(a.id, "mein_addon");
        let Ok(api::Reply::Registration(reg)) = a.call(&TestHost, &api::Event::Init { api: api::API_VERSION, client: "test".into() }) else { panic!() };
        a.apply_registration(reg);
        assert!(a.wants(api::Hook::ChatIn) && !a.wants(api::Hook::Tick));
        assert_eq!(a.commands, vec!["hallo"]);
        assert_eq!(a.modules[0].id, "gruss_");
        assert_eq!(a.modules[0].settings.len(), 1, "Auswahl ohne Optionen faellt weg");
        assert_eq!(a.modules[0].settings[0].default, "3");
        assert!(matches!(&a.take_queued()[..], [api::Call::Chat { text }] if text == "hallo"));
        // Die Abfrage wurde sofort beantwortet und liegt im Speicher des Addons.
        let erwartet = api::encode(&api::Answer::Block(Some("minecraft:stone fuer mein_addon".into())));
        let mut gelesen = vec![0u8; erwartet.len()];
        a.memory.read(&a.store, 1024, &mut gelesen).unwrap();
        assert_eq!(gelesen, erwartet);
    }

    // Ein echtes, mit Rust gebautes Addon (z.B. addons/template) von Anfang bis Ende:
    // CORAL_ADDON_WASM=pfad/zum/addon.wasm cargo test -p app --bins echtes_addon -- --ignored
    #[test]
    #[ignore]
    fn echtes_addon() {
        let pfad = std::env::var("CORAL_ADDON_WASM").expect("CORAL_ADDON_WASM setzen");
        let mut addons = laden("echt", std::fs::read(pfad).unwrap());
        let a = &mut addons.list[0];
        let Ok(api::Reply::Registration(reg)) = a.call(&TestHost, &api::Event::Init { api: api::API_VERSION, client: "test".into() }) else { panic!() };
        a.apply_registration(reg);
        println!("{} Module, Hooks {:?}, Befehle {:?}", a.modules.len(), a.hooks, a.commands);
        for m in ["info", "chat", "steuerung"] {
            a.call(&TestHost, &api::Event::ModuleEnabled { module: m.into(), on: true }).unwrap();
        }
        let flow = |r: Result<api::Reply, String>| match r.unwrap() {
            api::Reply::Flow(f) => f,
            api::Reply::Registration(_) => panic!(),
        };
        assert_eq!(flow(a.call(&TestHost, &api::Event::ChatOut { text: "na :shrug:".into() })), api::Flow::ReplaceText("na ¯\\_(ツ)_/¯".into()));
        assert_eq!(flow(a.call(&TestHost, &api::Event::Key { key: "KeyR".into(), pressed: true })), api::Flow::Cancel);
        assert!(matches!(&a.take_queued()[..], [api::Call::SetInput { input }] if input.forward));
        a.call(&TestHost, &api::Event::Command { name: "beispiel".into(), args: "hilfe".into() }).unwrap();
        assert!(matches!(&a.take_queued()[..], [api::Call::Print { .. }]));
        for _ in 0..1000 {
            a.call(&TestHost, &api::Event::Tick).unwrap();
        }
        println!("nach 1000 Ticks: {:?}", a.take_queued().last());
        assert!(!a.dead);
    }

    #[test]
    fn eingriff_und_endlosschleife() {
        let mut addons = laden("eingriff", test_addon(&api::Reply::Flow(api::Flow::ReplaceText("neu".into()))));
        let a = &mut addons.list[0];
        assert!(matches!(a.call(&TestHost, &api::Event::ChatIn { text: "alt".into() }), Ok(api::Reply::Flow(api::Flow::ReplaceText(t))) if t == "neu"));

        // Eine Endlosschleife endet am Rechenbudget; danach ist das Addon still.
        let wat = r#"(module
            (memory (export "memory") 1)
            (func (export "coral_alloc") (param i32) (result i32) (i32.const 0))
            (func (export "coral_free") (param i32 i32))
            (func (export "coral_event") (param i32 i32) (result i64) (loop $l (br $l)) (i64.const 0)))"#;
        let mut addons = laden("schleife", wat::parse_str(wat).unwrap());
        let a = &mut addons.list[0];
        assert!(a.call(&TestHost, &api::Event::Tick).is_err());
        assert!(a.dead);
        assert!(a.call(&TestHost, &api::Event::Tick).is_err());
    }
}
