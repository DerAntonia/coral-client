// Die Methoden von App, nach Themen auf Dateien verteilt. Jede Datei ist ein eigener
// `impl App`-Block; als Untermodule der Wurzel sehen sie alle privaten Felder und Typen aus main.rs.

mod addon_host;
mod anims;
mod chat;
mod combat;
mod connection;
mod controls;
mod debug;
mod events;
mod features;
mod hud;
mod interaction;
mod main_loop;
mod menu;
mod module_menu;
mod movement;
mod recording;
mod replay;
mod snapshot;
mod render;
mod screens;
mod settings_screens;
mod setup;
mod skins;
mod sound;
mod tabs;
mod window_events;

pub(crate) use recording::AppRecorder;
pub(crate) use replay::{RecordingEntry, Replay};
pub(crate) use tabs::ServerTab;
