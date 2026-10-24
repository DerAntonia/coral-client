// Sprachchat in der App (Modul "Sprachchat"): Secret anfordern, UDP-Verbindung halten (s.
// voice::VoiceClient), Mikrofon bei gehaltener Sprechtaste senden, empfangene Stimmen mit Ort
// in den Mixer geben und merken, wer gerade spricht (fuer das Zeichen am Namensschild).

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use protocol::uuid::Uuid;
use voice::processing::{Activation, MicProcessor, MicSettings};
use voice::{PlayerState, SecretInfo, SoundSource, VoiceClient, VoiceEvent};

// --- Einstellungen ---------------------------------------------------------------------
//
// Dieselben Stellschrauben wie die Mod (ClientConfig), mit deren Wertebereichen und
// Standards -- bis auf den Ausgabepuffer: die Mod nimmt 5 Rahmen (100 ms), hier sind es 3,
// weil Latenz in diesem Client vor Robustheit geht.

// Raeumlicher Klang (AudioType der Mod).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioType {
    // Volle Richtung.
    Normal,
    // Halbe Seitenlage -- weniger hart links/rechts.
    Reduced,
    // Ohne Richtung, nur die Entfernung zaehlt.
    Off,
}

impl AudioType {
    pub fn label(self) -> &'static str {
        match self {
            AudioType::Normal => "Normal",
            AudioType::Reduced => "Reduziert",
            AudioType::Off => "Aus",
        }
    }

    pub fn next(self) -> AudioType {
        match self {
            AudioType::Normal => AudioType::Reduced,
            AudioType::Reduced => AudioType::Off,
            AudioType::Off => AudioType::Normal,
        }
    }

    fn key(self) -> &'static str {
        match self {
            AudioType::Normal => "normal",
            AudioType::Reduced => "reduced",
            AudioType::Off => "off",
        }
    }
}

pub const VOLUME_MAX: u32 = 300;
pub const GAIN_DB_MIN: i32 = -40;
pub const GAIN_DB_MAX: i32 = 24;
pub const THRESHOLD_DB_MIN: i32 = -127;
pub const DELAY_MAX_FRAMES: u32 = 100;
pub const BUFFER_MAX_FRAMES: u32 = 16;
pub const PACKET_THRESHOLD_MAX: u32 = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSettings {
    // Lautstaerke der anderen, Prozent (0..300).
    pub volume_percent: u32,
    pub mic_gain_db: i32,
    pub voice_activation: bool,
    pub threshold_db: i32,
    pub voice_delay_frames: u32,
    pub ptt_delay_frames: u32,
    pub agc: bool,
    pub denoise: bool,
    pub audio_type: AudioType,
    pub output_buffer_frames: u32,
    pub packet_threshold: u32,
    // Leer = Standardgeraet.
    pub mic_device: String,
    pub muted: bool,
    // Sprachchat taub: nichts hoeren, nichts senden (die anderen sehen das).
    pub disabled: bool,
    pub mute_on_join: bool,
    pub nametag_icons: bool,
    pub hud_icons: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            volume_percent: 100,
            mic_gain_db: 0,
            voice_activation: false,
            threshold_db: -50,
            voice_delay_frames: 25,
            ptt_delay_frames: 5,
            agc: false,
            denoise: false,
            audio_type: AudioType::Normal,
            output_buffer_frames: 3,
            packet_threshold: 3,
            mic_device: String::new(),
            muted: false,
            disabled: false,
            mute_on_join: false,
            nametag_icons: true,
            hud_icons: true,
        }
    }
}

impl VoiceSettings {
    pub fn mic(&self) -> MicSettings {
        MicSettings {
            gain_db: self.mic_gain_db as f32,
            agc: self.agc,
            denoise: self.denoise,
            activation: if self.voice_activation { Activation::Voice } else { Activation::PushToTalk },
            threshold_db: self.threshold_db as f32,
            voice_delay_frames: self.voice_delay_frames,
            ptt_delay_frames: self.ptt_delay_frames,
        }
    }

    pub fn serialize(&self) -> String {
        format!(
            "voice_volume_percent={}\nvoice_mic_gain_db={}\nvoice_activation={}\nvoice_threshold_db={}\nvoice_delay_frames={}\n\
             voice_ptt_delay_frames={}\nvoice_agc={}\nvoice_denoise={}\nvoice_audio_type={}\nvoice_output_buffer={}\n\
             voice_packet_threshold={}\nvoice_mic_device={}\nvoice_muted={}\nvoice_disabled={}\nvoice_mute_on_join={}\n\
             voice_nametag_icons={}\nvoice_hud_icons={}\n",
            self.volume_percent,
            self.mic_gain_db,
            if self.voice_activation { "voice" } else { "ptt" },
            self.threshold_db,
            self.voice_delay_frames,
            self.ptt_delay_frames,
            self.agc,
            self.denoise,
            self.audio_type.key(),
            self.output_buffer_frames,
            self.packet_threshold,
            self.mic_device,
            self.muted,
            self.disabled,
            self.mute_on_join,
            self.nametag_icons,
            self.hud_icons,
        )
    }

    // Eine Zeile aus settings.conf; false = nicht zustaendig.
    pub fn parse_line(&mut self, key: &str, value: &str) -> bool {
        let zahl = |v: &str| v.trim().parse::<i64>().ok();
        let b = value.trim() == "true";
        match key {
            "voice_volume_percent" => self.volume_percent = zahl(value).map_or(self.volume_percent, |v| v.clamp(0, u32::MAX as i64) as u32),
            "voice_mic_gain_db" => self.mic_gain_db = zahl(value).map_or(self.mic_gain_db, |v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32),
            "voice_activation" => self.voice_activation = value.trim() == "voice",
            "voice_threshold_db" => self.threshold_db = zahl(value).map_or(self.threshold_db, |v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32),
            "voice_delay_frames" => self.voice_delay_frames = zahl(value).map_or(self.voice_delay_frames, |v| v.clamp(0, u32::MAX as i64) as u32),
            "voice_ptt_delay_frames" => self.ptt_delay_frames = zahl(value).map_or(self.ptt_delay_frames, |v| v.clamp(0, u32::MAX as i64) as u32),
            "voice_agc" => self.agc = b,
            "voice_denoise" => self.denoise = b,
            "voice_audio_type" => {
                self.audio_type = match value.trim() {
                    "reduced" => AudioType::Reduced,
                    "off" => AudioType::Off,
                    _ => AudioType::Normal,
                }
            }
            "voice_output_buffer" => self.output_buffer_frames = zahl(value).map_or(self.output_buffer_frames, |v| v.clamp(0, u32::MAX as i64) as u32),
            "voice_packet_threshold" => self.packet_threshold = zahl(value).map_or(self.packet_threshold, |v| v.clamp(0, u32::MAX as i64) as u32),
            "voice_mic_device" => self.mic_device = value.trim().to_string(),
            "voice_muted" => self.muted = b,
            "voice_disabled" => self.disabled = b,
            "voice_mute_on_join" => self.mute_on_join = b,
            "voice_nametag_icons" => self.nametag_icons = b,
            "voice_hud_icons" => self.hud_icons = b,
            _ => return false,
        }
        true
    }
}

// Rahmen (20 ms) als lesbare Zeit.
pub fn frames_label(frames: u32) -> String {
    format!("{} ms", frames * 20)
}

// Wie lange nach dem letzten Rahmen ein Spieler noch als "spricht" gilt.
const TALK_HOLD: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    // Kein Sprachchat auf diesem Server (bisher keine Antwort) oder Modul aus.
    Off,
    Connecting,
    Connected,
    Failed(String),
}

// Ein Sprecher, der gerade zu hoeren ist: Quelle und Absender, damit Ort und Pegel jedes Bild
// nachgefuehrt werden koennen.
pub struct ActiveStream {
    pub sender: Uuid,
    pub source: SoundSource,
    pub last: Instant,
}

pub struct VoiceChat {
    pub client: Option<VoiceClient>,
    pub status: Status,
    pub states: HashMap<[u8; 16], PlayerState>,
    pub streams: HashMap<u64, ActiveStream>,
    pub mic: Option<audio::mic::Microphone>,
    pub mic_rx: Option<Receiver<Vec<i16>>>,
    // Gerade am Senden (Sprechtaste gehalten bzw. Stimme erkannt, und verbunden).
    pub sending: bool,
    pub last_mic_error: Option<String>,
    pub processor: MicProcessor,
    // Pegel des letzten Mikrofon-Rahmens nach der Aufbereitung (dBFS) -- fuer die Anzeige
    // neben der Aktivierungsschwelle.
    pub level_db: f32,
    // Mikrofon mithoeren (zum Einstellen von Verstaerkung und Schwelle).
    pub monitor: bool,
    // Gerade fluesternd am Senden (fuer die HUD-Anzeige).
    pub whispering: bool,
    // Geraet, mit dem das Mikrofon gerade offen ist.
    pub mic_device_open: Option<String>,
}

impl Default for VoiceChat {
    fn default() -> Self {
        VoiceChat {
            client: None,
            status: Status::Off,
            states: HashMap::new(),
            streams: HashMap::new(),
            mic: None,
            mic_rx: None,
            sending: false,
            last_mic_error: None,
            processor: MicProcessor::default(),
            level_db: -127.0,
            monitor: false,
            whispering: false,
            mic_device_open: None,
        }
    }
}

// Strom-Kennung fuer das Mithoeren des eigenen Mikrofons.
pub const MONITOR_STREAM: u64 = u64::MAX;

// Strom-Kennung im Mixer: die ersten 8 Bytes der Kanal-UUID.
pub fn stream_id(channel: &Uuid) -> u64 {
    u64::from_be_bytes(channel.0[..8].try_into().expect("8 Bytes"))
}

// Lautstaerke ueber die Entfernung wie bei der Mod (PositionalAudioUtils.getDistanceVolume):
// linear von voll bis null an der Reichweite.
pub fn distance_volume(distance: f32, max: f32) -> f32 {
    if max <= 0.0 {
        return 1.0;
    }
    1.0 - distance.min(max) / max
}

impl VoiceChat {
    pub fn reset(&mut self) {
        *self = VoiceChat::default();
    }

    pub fn connect(&mut self, info: SecretInfo, server_host: &str) {
        self.client = None;
        self.streams.clear();
        match VoiceClient::connect(info, server_host) {
            Ok(c) => {
                self.client = Some(c);
                self.status = Status::Connecting;
            }
            Err(e) => self.status = Status::Failed(e.to_string()),
        }
    }

    // Spricht dieser Spieler gerade? Some(fluestert) bzw. None.
    pub fn talking(&self, uuid: &Uuid) -> Option<bool> {
        self.streams
            .values()
            .find(|s| s.sender == *uuid && s.last.elapsed() < TALK_HOLD)
            .map(|s| matches!(s.source, SoundSource::Player { whisper: true, .. }))
    }

    // Mikrofon oeffnen, falls noch zu. Die Rahmen kommen im Audio-Thread des Systems an und
    // werden nur in den Kanal gelegt; gesendet wird im Takt der App (s. App::voice_tick).
    // `device` leer = Standardgeraet. Ist das Mikrofon mit einem anderen Geraet offen, wird es
    // neu geoeffnet.
    pub fn ensure_mic(&mut self, device: &str) {
        if self.mic.is_some() && self.mic_device_open.as_deref() == Some(device) {
            return;
        }
        self.close_mic();
        let (tx, rx) = channel();
        let geraet = (!device.is_empty()).then_some(device);
        match audio::mic::Microphone::open(geraet, move |rahmen: &[i16]| {
            let _ = tx.send(rahmen.to_vec());
        }) {
            Ok(m) => {
                self.mic = Some(m);
                self.mic_rx = Some(rx);
                self.last_mic_error = None;
                self.mic_device_open = Some(device.to_string());
            }
            Err(e) => self.last_mic_error = Some(e.to_string()),
        }
    }

    pub fn close_mic(&mut self) {
        self.mic = None;
        self.mic_rx = None;
        self.mic_device_open = None;
    }

    pub fn poll(&mut self) -> Vec<VoiceEvent> {
        let ereignisse = self.client.as_ref().map(|c| c.poll()).unwrap_or_default();
        for e in &ereignisse {
            match e {
                VoiceEvent::Connected => self.status = Status::Connected,
                VoiceEvent::Disconnected(grund) => self.status = Status::Failed(grund.clone()),
                VoiceEvent::Audio { .. } => {}
            }
        }
        if matches!(self.status, Status::Failed(_)) {
            self.client = None;
        }
        ereignisse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lautstaerke_ueber_entfernung() {
        assert_eq!(distance_volume(0.0, 48.0), 1.0);
        assert_eq!(distance_volume(24.0, 48.0), 0.5);
        assert_eq!(distance_volume(60.0, 48.0), 0.0);
    }

    #[test]
    // Speichern und Laden ergeben dieselben Einstellungen; Werte ausserhalb der Regler
    // bleiben stehen.
    fn einstellungen_ueberleben_das_speichern() {
        let mut a = VoiceSettings { mic_gain_db: -7, voice_activation: true, audio_type: AudioType::Reduced, mic_device: "pulse".into(), denoise: true, ..Default::default() };
        a.threshold_db = -33;
        let mut b = VoiceSettings::default();
        for zeile in a.serialize().lines() {
            let (k, v) = zeile.split_once('=').unwrap();
            assert!(b.parse_line(k, v), "{k}");
        }
        assert_eq!(a, b);
        assert!(b.parse_line("voice_mic_gain_db", "99"));
        assert_eq!(b.mic_gain_db, 99);
        assert!(!b.parse_line("fremd", "1"));
    }

    #[test]
    fn strom_kennung_aus_der_kanal_uuid() {
        let mut u = [0u8; 16];
        u[7] = 3;
        assert_eq!(stream_id(&Uuid(u)), 3);
    }
}
