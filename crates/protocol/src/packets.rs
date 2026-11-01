// Packet-Definitionen fuer Handshaking/Status/Login-States, Protokoll-Version 774
// (Minecraft 1.21.11). Referenz: https://minecraft.wiki/w/Java_Edition_protocol/Packets

use crate::buffer::{PacketReader, PacketWriter, ReadError};
use crate::item::{read_optional_slot, ItemStack, SlotError};
use crate::nbt;
use crate::uuid::Uuid;
mod handshake_login;
mod configuration;
mod play_core;
mod chat_titles;
mod player_state;
mod scoreboard;
mod serverbound;
mod entities;
mod entity_data;
mod containers;
mod player_info;
mod combat_registries;
mod commands_sounds;
mod custom_cookies_packs;
mod links_dialogs;
pub use handshake_login::*;
pub use configuration::*;
pub use play_core::*;
pub use chat_titles::*;
pub use player_state::*;
pub use scoreboard::*;
pub use serverbound::*;
pub use entities::*;
pub use entity_data::*;
pub use containers::*;
pub use player_info::*;
pub use combat_registries::*;
pub use commands_sounds::*;
pub use custom_cookies_packs::*;
pub use links_dialogs::*;

pub const PROTOCOL_VERSION: i32 = 774;

pub const INTENT_STATUS: i32 = 1;
pub const INTENT_LOGIN: i32 = 2;

#[cfg(test)]
mod chat_signing_tests;

#[cfg(test)]
mod inventory_tests;

#[cfg(test)]
mod custom_payload_tests;

#[cfg(test)]
mod tests;
