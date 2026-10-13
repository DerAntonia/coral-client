// Sprachchat, kompatibel zu Simple Voice Chat (s. packets fuer das Protokoll).

pub mod client;
pub mod crypto;
pub mod packets;
pub mod processing;

pub use client::{VoiceClient, VoiceEvent};
pub use packets::{PlayerState, SecretInfo, SoundSource};
