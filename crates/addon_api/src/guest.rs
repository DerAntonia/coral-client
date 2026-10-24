// Die Addon-Seite auf wasm32: Speicher fuer den Client, Aufruf der Hooks und client::* als
// Weg zurueck. Der Client schreibt Eingaben in Speicher, den er sich ueber coral_alloc holt,
// und gibt Antworten mit coral_free wieder frei; beides sind Box<[u8]> fester Laenge.

use crate::{decode, encode, Addon, Answer, Call, Event, Flow, Reply};
use alloc::boxed::Box;
use alloc::vec;

#[link(wasm_import_module = "coral")]
extern "C" {
    // Schickt einen kodierten Call; Rueckgabe ist die Laenge der Antwort (0 = keine).
    fn call(ptr: *const u8, len: u32) -> u32;
    // Kopiert die Antwort des letzten call() nach dst.
    fn answer(dst: *mut u8);
}

pub fn alloc(len: u32) -> u32 {
    Box::into_raw(vec![0u8; len as usize].into_boxed_slice()) as *mut u8 as u32
}

pub fn free(ptr: u32, len: u32) {
    if ptr != 0 {
        // SAFETY: ptr/len stammen aus alloc() bzw. aus event(), beide Box<[u8]> genau dieser Laenge.
        drop(unsafe { Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize)) });
    }
}

pub fn event<T: Addon + Default>(slot: &mut Option<T>, ptr: u32, len: u32) -> u64 {
    // SAFETY: der Client hat den Puffer mit alloc() geholt und gibt ihn hier ab.
    let eingabe = unsafe { Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize)) };
    let addon = slot.get_or_insert_with(T::default);
    let antwort = match decode::<Event>(&eingabe) {
        Some(Event::Init { .. }) => Reply::Registration(addon.register()),
        Some(e) => Reply::Flow(addon.on_event(e)),
        None => Reply::Flow(Flow::Continue),
    };
    drop(eingabe);
    let bytes = encode(&antwort).into_boxed_slice();
    let n = bytes.len() as u64;
    (Box::into_raw(bytes) as *mut u8 as u64) << 32 | n
}

fn request(c: &Call) -> Answer {
    let b = encode(c);
    // SAFETY: call liest nur b, answer schreibt genau n Bytes nach buf.
    let n = unsafe { call(b.as_ptr(), b.len() as u32) };
    if n == 0 {
        return Answer::None;
    }
    let mut buf = vec![0u8; n as usize];
    unsafe { answer(buf.as_mut_ptr()) };
    decode(&buf).unwrap_or_default()
}

// Alles, was ein Addon vom Client will: Abfragen geben den Stand von jetzt zurueck, Aktionen
// laufen direkt nach dem aktuellen Hook.
pub mod client {
    use super::request;
    use crate::*;
    use alloc::string::String;
    use alloc::vec::Vec;

    pub fn call(c: Call) -> Answer {
        request(&c)
    }

    pub fn player() -> PlayerState {
        match request(&Call::Player) {
            Answer::Player(p) => p,
            _ => PlayerState::default(),
        }
    }

    pub fn inventory() -> Vec<Option<Item>> {
        match request(&Call::Inventory) {
            Answer::Inventory(i) => i,
            _ => Vec::new(),
        }
    }

    pub fn entities(radius: f64) -> Vec<EntityInfo> {
        match request(&Call::Entities { radius }) {
            Answer::Entities(e) => e,
            _ => Vec::new(),
        }
    }

    pub fn block(x: i32, y: i32, z: i32) -> Option<String> {
        match request(&Call::Block { x, y, z }) {
            Answer::Block(b) => b,
            _ => None,
        }
    }

    pub fn target() -> Target {
        match request(&Call::Target) {
            Answer::Target(t) => t,
            _ => Target::Miss,
        }
    }

    pub fn players() -> Vec<PlayerEntry> {
        match request(&Call::Players) {
            Answer::Players(p) => p,
            _ => Vec::new(),
        }
    }

    pub fn screen() -> ScreenState {
        match request(&Call::Screen) {
            Answer::Screen(s) => s,
            _ => ScreenState::default(),
        }
    }

    pub fn setting(module: &str, key: &str) -> Value {
        match request(&Call::GetSetting { module: module.into(), key: key.into() }) {
            Answer::Value(v) => v,
            _ => Value::None,
        }
    }

    pub fn hud(module: &str, items: Vec<HudItem>) {
        request(&Call::Hud { module: module.into(), items });
    }

    pub fn chat(text: &str) {
        request(&Call::Chat { text: text.into() });
    }

    pub fn print(text: &str) {
        request(&Call::Print { text: text.into() });
    }

    pub fn notify(title: &str, text: &str) {
        request(&Call::Notify { title: title.into(), text: text.into() });
    }

    pub fn sound(event: &str, volume: f32, pitch: f32) {
        request(&Call::Sound { event: event.into(), volume, pitch });
    }

    pub fn log(text: &str) {
        request(&Call::Log { text: text.into() });
    }

    pub fn set_setting(module: &str, key: &str, value: Value) {
        request(&Call::SetSetting { module: module.into(), key: key.into(), value });
    }

    pub fn send_packet(data: Vec<u8>) {
        request(&Call::SendPacket { data });
    }

    pub fn set_rotation(yaw: f32, pitch: f32) {
        request(&Call::SetRotation { yaw, pitch });
    }

    pub fn select_slot(slot: u8) {
        request(&Call::SelectSlot { slot });
    }

    pub fn attack() {
        request(&Call::Attack);
    }

    pub fn use_item() {
        request(&Call::Use);
    }

    pub fn drop_item(whole_stack: bool) {
        request(&Call::Drop { whole_stack });
    }

    pub fn swap_hands() {
        request(&Call::SwapHands);
    }

    pub fn click_slot(slot: i16, button: i8, mode: i32) {
        request(&Call::ClickSlot { slot, button, mode });
    }

    pub fn set_input(input: Input) {
        request(&Call::SetInput { input });
    }

    pub fn close_screen() {
        request(&Call::CloseScreen);
    }
}
