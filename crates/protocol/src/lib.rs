// Packet-Definitionen, (De)Serialisierung, Versionierung fuer Protokoll-Version 774 (1.21.11).

pub mod buffer;
pub mod chunk;
pub mod item;
pub mod nbt;
pub mod packets;
pub mod particle;
pub mod recipe;
pub mod sign;
// Erzeugt aus dem Client-Jar, s. scripts/gen-sound-events.sh.
pub mod sound_events;
pub mod uuid;
pub mod varint;
