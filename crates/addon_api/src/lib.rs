// Die Schnittstelle zwischen Coral Client und seinen Addons.
//
// Ein Addon ist ein WebAssembly-Modul (.wasm) in ~/coral-client/mods. Der Client laedt es in
// seinen eigenen Prozess und ruft es an festen Stellen auf -- den Hooks (s. Hook, Event): vor
// jedem Tick, bei jedem Frame, fuer jede Chatzeile, jedes Paket, jede Taste ... Bei vielen
// Hooks darf das Addon eingreifen: abbrechen oder den Inhalt ersetzen (s. Flow). Waehrenddessen
// fragt es den Spielzustand ab und loest Aktionen aus (s. Call, client::*).
//
// Uebertragen wird alles mit postcard (kompaktes Binaerformat ueber serde). Neue Varianten
// kommen deshalb IMMER ans Ende eines Enums -- sonst verschieben sich die Nummern, und alte
// Addons lesen Unsinn.
//
// Ein Addon schreibt man so:
//
//     #[derive(Default)]
//     struct Mein;
//     impl coral_addon::Addon for Mein {
//         fn register(&mut self) -> coral_addon::Registration { ... }
//         fn on_event(&mut self, event: coral_addon::Event) -> coral_addon::Flow { ... }
//     }
//     coral_addon::export!(Mein);
//
// und baut es mit `cargo build --release --target wasm32-unknown-unknown` (crate-type cdylib).

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

// Version dieser Schnittstelle. Steht in Event::Init; der Client laedt Addons, die mit einer
// anderen Hauptversion gebaut sind, trotzdem -- sie bekommen dann nur, was sie verstehen.
pub const API_VERSION: u32 = 2;

// ---------------------------------------------------------------------------------------------
// Anmeldung

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Registration {
    // Name der eigenen Kategorie im Modul-Menue.
    pub name: String,
    // Welche Hooks das Addon hoeren will. Nur diese kosten ueberhaupt etwas.
    pub hooks: Vec<Hook>,
    pub modules: Vec<Module>,
    // Eigene Befehle ohne "/", z.B. "beispiel": "/beispiel a b" geht dann als Event::Command
    // an das Addon statt an den Server.
    pub commands: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Module {
    // Nur a-z, 0-9, _ und -; unter diesem Namen speichert der Client Schalter und Werte.
    pub id: String,
    pub name: String,
    pub description: String,
    pub settings: Vec<Setting>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Setting {
    pub key: String,
    pub label: String,
    pub kind: SettingKind,
    pub default: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum SettingKind {
    Toggle,
    Slider { min: f64, max: f64, step: f64, unit: String },
    Choice { options: Vec<String> },
    // Wert als "#RRGGBB".
    Color,
    // Kein Wert; ein Klick kommt als Event::Button.
    Button,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Value {
    pub fn as_bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Value::Text(s) => s,
            _ => "",
        }
    }
}

impl Setting {
    pub fn toggle(key: &str, label: &str, default: bool) -> Self {
        Setting { key: key.into(), label: label.into(), kind: SettingKind::Toggle, default: Value::Bool(default) }
    }

    pub fn slider(key: &str, label: &str, min: f64, max: f64, step: f64, unit: &str, default: f64) -> Self {
        Setting { key: key.into(), label: label.into(), kind: SettingKind::Slider { min, max, step, unit: unit.into() }, default: Value::Number(default) }
    }

    pub fn choice(key: &str, label: &str, options: &[&str], default: &str) -> Self {
        let options = options.iter().map(|&o| String::from(o)).collect();
        Setting { key: key.into(), label: label.into(), kind: SettingKind::Choice { options }, default: Value::Text(default.into()) }
    }

    pub fn color(key: &str, label: &str, default: &str) -> Self {
        Setting { key: key.into(), label: label.into(), kind: SettingKind::Color, default: Value::Text(default.into()) }
    }

    pub fn button(key: &str, label: &str) -> Self {
        Setting { key: key.into(), label: label.into(), kind: SettingKind::Button, default: Value::None }
    }
}

// ---------------------------------------------------------------------------------------------
// Hooks: Client -> Addon

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hook {
    // Vor jedem Client-Tick (20 je Sekunde, nur in einer Welt).
    Tick,
    // Vor jedem Bild.
    Frame,
    Join,
    Leave,
    // Eine Chatzeile kommt an. Abbrechbar, Text ersetzbar.
    ChatIn,
    // Der Spieler schickt eine Chatzeile oder einen Befehl. Abbrechbar, Text ersetzbar.
    ChatOut,
    // Ein Paket vom Server, mit Packet-ID vorne. Nur lesen: verarbeitet wird es trotzdem.
    PacketIn,
    // Ein Paket an den Server, mit Packet-ID vorne. Abbrechbar, Bytes ersetzbar.
    PacketOut,
    // Taste gedrueckt oder losgelassen. Abbrechbar: der Client sieht sie dann nicht.
    Key,
    // Maustaste. Abbrechbar.
    Mouse,
    // Mausrad. Abbrechbar.
    Scroll,
    // Der eigene Spieler ist gestorben.
    Death,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Event {
    // Erster Aufruf; die Antwort ist die Anmeldung (s. Addon::register).
    Init { api: u32, client: String },
    // Stand der Module: kommt nach der Anmeldung fuer jedes Modul und jede Einstellung und
    // danach bei jeder Aenderung im Menue.
    ModuleEnabled { module: String, on: bool },
    Setting { module: String, key: String, value: Value },
    Button { module: String, key: String },
    Tick,
    // Sekunden seit dem letzten Bild.
    Frame { dt: f32 },
    Join { server: String },
    Leave { reason: String },
    // Als reiner Text ohne Farben. Flow::ReplaceText ersetzt die Zeile (dann ohne Farben).
    ChatIn { text: String },
    ChatOut { text: String },
    // Ein eigener Befehl (s. Registration::commands): Name ohne "/" und der Rest der Zeile.
    Command { name: String, args: String },
    PacketIn { id: i32, data: Vec<u8> },
    PacketOut { id: i32, data: Vec<u8> },
    // Taste als Name wie in der Config der Tastenbelegung: "KeyW", "Space", "F5", "ShiftLeft" ...
    // Nur, waehrend man in der Welt steuert (nicht beim Tippen in Chat oder Menue).
    Key { key: String, pressed: bool },
    // 0 links, 1 rechts, 2 mitte, 3 zurueck, 4 vor, ab 5 weitere.
    Mouse { button: u8, pressed: bool },
    // Eine Raste, positiv nach oben.
    Scroll { delta: f32 },
    Death,
}

impl Event {
    pub fn hook(&self) -> Option<Hook> {
        Some(match self {
            Event::Tick => Hook::Tick,
            Event::Frame { .. } => Hook::Frame,
            Event::Join { .. } => Hook::Join,
            Event::Leave { .. } => Hook::Leave,
            Event::ChatIn { .. } => Hook::ChatIn,
            Event::ChatOut { .. } => Hook::ChatOut,
            Event::PacketIn { .. } => Hook::PacketIn,
            Event::PacketOut { .. } => Hook::PacketOut,
            Event::Key { .. } => Hook::Key,
            Event::Mouse { .. } => Hook::Mouse,
            Event::Scroll { .. } => Hook::Scroll,
            Event::Death => Hook::Death,
            Event::Init { .. } | Event::ModuleEnabled { .. } | Event::Setting { .. } | Event::Button { .. } | Event::Command { .. } => return None,
        })
    }
}

// Was ein Hook mit dem Ereignis macht. Mehrere Addons laufen nacheinander: ein Abbruch endet
// die Kette, ein ersetzter Inhalt geht an das naechste Addon weiter.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Flow {
    #[default]
    Continue,
    Cancel,
    ReplaceText(String),
    ReplaceBytes(Vec<u8>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Reply {
    Registration(Registration),
    Flow(Flow),
}

// ---------------------------------------------------------------------------------------------
// Aufrufe: Addon -> Client
//
// Abfragen antworten sofort mit dem Stand in diesem Moment. Aktionen fuehrt der Client direkt
// nach dem laufenden Hook aus, in der Reihenfolge der Aufrufe.

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Call {
    // --- Abfragen ---
    Player,
    // Alle 46 Slots des Spielerinventars in Vanilla-Reihenfolge (0 Ergebnis, 1-4 Crafting,
    // 5-8 Ruestung, 9-35 Inventar, 36-44 Hotbar, 45 Zweithand).
    Inventory,
    // Entities im Umkreis (Bloecke) um den Spieler, naechste zuerst.
    Entities { radius: f64 },
    // Block an einer Stelle, z.B. "minecraft:stone"; None, wenn der Chunk nicht geladen ist.
    Block { x: i32, y: i32, z: i32 },
    // Worauf das Fadenkreuz zeigt.
    Target,
    // Die Tab-Liste.
    Players,
    // Fenster und offene Bildschirme.
    Screen,
    // Gespeicherter Wert einer eigenen Einstellung.
    GetSetting { module: String, key: String },

    // --- Aktionen ---
    // Ersetzt, was dieses Modul ins HUD zeichnet; gezeichnet wird nur, solange es an ist.
    Hud { module: String, items: Vec<HudItem> },
    // Chatzeile oder mit "/" ein Befehl. Hoechstens 2 je Sekunde, mehr wird verworfen.
    Chat { text: String },
    // Eine Zeile nur im eigenen Chat, nicht an den Server.
    Print { text: String },
    Notify { title: String, text: String },
    // Vanilla-Klang wie "block.note_block.pling".
    Sound { event: String, volume: f32, pitch: f32 },
    Log { text: String },
    // Eigene Einstellung aendern (ohne Echo als Event::Setting).
    SetSetting { module: String, key: String, value: Value },
    // Fertig kodiertes Paket mit Packet-ID vorne; laeuft nicht durch Hook::PacketOut.
    SendPacket { data: Vec<u8> },
    // Blickrichtung in Grad, wie Vanilla (yaw 0 = Sueden, pitch positiv = nach unten).
    SetRotation { yaw: f32, pitch: f32 },
    SelectSlot { slot: u8 },
    // Wie ein Links- bzw. Rechtsklick auf das, was gerade im Fadenkreuz ist.
    Attack,
    Use,
    Drop { whole_stack: bool },
    SwapHands,
    // Klick in das offene Inventar (Slot-Nummer wie dort, button 0 links / 1 rechts, mode wie
    // Vanillas ContainerInput: 0 Klick, 1 Shift-Klick, 2 Hotbar-Tausch, 4 Werfen ...).
    ClickSlot { slot: i16, button: i8, mode: i32 },
    // Bewegungstasten zusaetzlich zu den echten druecken, bis zum naechsten SetInput.
    SetInput { input: Input },
    // Ein Bildschirm des Clients schliessen (Inventar, Chat, Menue ...).
    CloseScreen,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub enum Answer {
    #[default]
    None,
    Player(PlayerState),
    Inventory(Vec<Option<Item>>),
    Entities(Vec<EntityInfo>),
    Block(Option<String>),
    Target(Target),
    Players(Vec<PlayerEntry>),
    Screen(ScreenState),
    Value(Value),
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PlayerState {
    // false im Menue: dann ist der Rest Platzhalter.
    pub in_world: bool,
    pub name: String,
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub sprinting: bool,
    pub sneaking: bool,
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
    pub xp_level: i32,
    pub xp_progress: f32,
    // 0 Ueberleben, 1 Kreativ, 2 Abenteuer, 3 Zuschauer.
    pub game_mode: u8,
    pub selected_slot: u8,
    // z.B. "minecraft:overworld".
    pub dimension: String,
    pub server: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    // z.B. "minecraft:diamond_sword".
    pub id: String,
    pub count: i32,
    pub damage: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EntityInfo {
    pub id: i32,
    // z.B. "minecraft:zombie"; Spieler sind "minecraft:player".
    pub kind: String,
    // Spielername bzw. eigener Name, sonst leer.
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub distance: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub enum Target {
    #[default]
    Miss,
    Block { x: i32, y: i32, z: i32, face: i32, block: String },
    Entity { id: i32 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerEntry {
    pub name: String,
    pub uuid: [u8; 16],
    pub ping: i32,
    pub game_mode: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ScreenState {
    pub width: f32,
    pub height: f32,
    // HUD-Massstab: HudItem-Koordinaten werden damit multipliziert.
    pub hud_scale: f32,
    // Inventar, Chat, Menue oder ein anderer Bildschirm offen.
    pub screen_open: bool,
    pub chat_open: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

// Ecke bzw. Mitte des Bildschirms, auf die sich x/y beziehen; x/y zaehlen von dort nach innen.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

// In GUI-Einheiten (mal HUD-Massstab). Farben als 0xRRGGBB.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum HudItem {
    Text { text: String, x: f32, y: f32, color: u32, anchor: Anchor },
    Rect { x: f32, y: f32, w: f32, h: f32, color: u32, alpha: f32, anchor: Anchor },
    // Item-Symbol (16x16 GUI-Einheiten), z.B. "minecraft:totem_of_undying", mit Anzahl > 1.
    Item { id: String, count: i32, x: f32, y: f32, anchor: Anchor },
}

// ---------------------------------------------------------------------------------------------
// Kodierung (fuer Client und Addon gleich)

pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).unwrap_or_default()
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

// ---------------------------------------------------------------------------------------------
// Addon-Seite

pub trait Addon {
    fn register(&mut self) -> Registration;
    fn on_event(&mut self, event: Event) -> Flow;
}

#[cfg(target_arch = "wasm32")]
pub mod guest;

#[cfg(target_arch = "wasm32")]
pub use guest::client;

// Macht einen Typ zum Addon: erzeugt die Exporte, die der Client aufruft. Der Typ braucht
// Default; eine Instanz lebt, solange das Addon geladen ist.
#[macro_export]
macro_rules! export {
    ($t:ty) => {
        ::std::thread_local! {
            static CORAL_ADDON: ::core::cell::RefCell<::core::option::Option<$t>> = const { ::core::cell::RefCell::new(None) };
        }

        #[no_mangle]
        pub extern "C" fn coral_alloc(len: u32) -> u32 {
            $crate::guest::alloc(len)
        }

        #[no_mangle]
        pub extern "C" fn coral_free(ptr: u32, len: u32) {
            $crate::guest::free(ptr, len)
        }

        #[no_mangle]
        pub extern "C" fn coral_event(ptr: u32, len: u32) -> u64 {
            CORAL_ADDON.with(|slot| $crate::guest::event::<$t>(&mut slot.borrow_mut(), ptr, len))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn hin_und_zurueck() {
        let e = Event::PacketOut { id: 7, data: vec![7, 1, 2] };
        let b = encode(&e);
        assert!(matches!(decode::<Event>(&b), Some(Event::PacketOut { id: 7, ref data }) if data == &[7, 1, 2]));
        let r = Reply::Registration(Registration { name: "x".into(), hooks: vec![Hook::Tick], modules: vec![], commands: vec!["a".into()] });
        assert!(matches!(decode::<Reply>(&encode(&r)), Some(Reply::Registration(reg)) if reg.hooks == [Hook::Tick]));
        assert_eq!(decode::<Event>(&[250, 1]).map(|_| ()), None);
    }
}
