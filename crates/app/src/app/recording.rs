// coral-record, App-Seite (s. crates/record): Kamera (K), Maus (M), lokaler Zustand (L) und
// Markierungen (E) fuer die Aufnahme des Tabs vorn. Die Pakete (S, C) schreibt die Verbindung
// selbst; ihre Sitzungsnummer steht im record_slot des Tabs. Dasselbe geht an den Puffer von
// coral-clip (s. app/clip.rs).
//
// Maus und lokaler Zustand nur bei Fokus und im Spiel (dieselbe Regel wie input_log): die
// Raw-Maus laeuft fokusunabhaengig.

use crate::{App, Instant, AppMode, Perspective};

// Lokaler Zustand: Slot, Flags, Schwung, Benutzen, Abbaufortschritt, Abbau-Block.
const STATE_LEN: usize = 1 + 1 + 1 + 2 + 4 + 12;

// Was je Ziel getrennt laeuft: die Sitzung, an die zuletzt geschrieben wurde (0 = keine), und
// der zuletzt geschriebene Zustand.
#[derive(Default)]
struct Track {
    session: u64,
    last_state: Option<[u8; STATE_LEN]>,
}

// Zwei Ziele: die Datei von coral-record und der Puffer von coral-clip (s. app/clip.rs).
#[derive(Default)]
pub(crate) struct AppRecorder {
    producer: record::Producer,
    tracks: [Track; 2],
}

impl AppRecorder {
    pub(crate) fn flush(&mut self) {
        self.producer.flush();
    }
}

impl App {
    // Die Sitzungen des Tabs vorn: Datei und Puffer (0 = keine).
    fn record_sessions(&self) -> [u64; 2] {
        self.tabs.get(self.active_tab).map_or([0, 0], |t| [t.record_slot.load(std::sync::atomic::Ordering::Relaxed), t.clip_link.session()])
    }

    pub(crate) fn record_mouse(&mut self, dx: f64, dy: f64, at: Instant) {
        if !self.focused || self.mode != AppMode::World {
            return;
        }
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&(dx as f32).to_le_bytes());
        b[4..].copy_from_slice(&(dy as f32).to_le_bytes());
        for id in self.record_sessions() {
            self.recorder.producer.push(id, record::Tag::M, at, &b);
        }
    }

    // Einmal je Bild, nach dem Setzen der Kamera.
    pub(crate) fn record_frame(&mut self) {
        let ids = self.record_sessions();
        if ids == [0, 0] && self.recorder.tracks.iter().all(|t| t.session == 0) {
            return;
        }
        let jetzt = Instant::now();
        // Im Menue-Hintergrund steht die Kamera in der Startszene, nicht beim Spieler.
        let kamera = self.hero_fov.is_none().then(|| {
            let c = &self.camera;
            let mut k = [0u8; 37];
            k[..8].copy_from_slice(&c.position.x.to_le_bytes());
            k[8..16].copy_from_slice(&c.position.y.to_le_bytes());
            k[16..24].copy_from_slice(&c.position.z.to_le_bytes());
            k[24..28].copy_from_slice(&c.yaw.to_le_bytes());
            k[28..32].copy_from_slice(&c.pitch.to_le_bytes());
            k[32..36].copy_from_slice(&c.fov_degrees.to_le_bytes());
            k[36] = Perspective::ALL.iter().position(|p| *p == self.perspective).unwrap_or(0) as u8;
            k
        });
        let zustand = (self.focused && self.mode == AppMode::World).then(|| self.record_state());
        for (i, id) in ids.into_iter().enumerate() {
            if id != self.recorder.tracks[i].session {
                let alt = self.recorder.tracks[i].session;
                self.recorder.producer.push(alt, record::Tag::E, jetzt, &[record::Mark::Back as u8]);
                let packs = self.settings.active_skins.join("\n");
                self.recorder.producer.push_parts(id, record::Tag::E, jetzt, &[&[record::Mark::Front as u8]]);
                self.recorder.producer.push_parts(id, record::Tag::E, jetzt, &[&[record::Mark::Packs as u8], packs.as_bytes()]);
                self.recorder.tracks[i] = Track { session: id, last_state: None };
            }
            if id == 0 {
                continue;
            }
            if let Some(k) = &kamera {
                self.recorder.producer.push(id, record::Tag::K, jetzt, k);
            }
            if zustand.is_some() && self.recorder.tracks[i].last_state != zustand {
                self.recorder.tracks[i].last_state = zustand;
                if let Some(z) = &zustand {
                    self.recorder.producer.push(id, record::Tag::L, jetzt, z);
                }
            }
        }
        self.recorder.producer.flush_due();
    }

    fn record_state(&self) -> [u8; STATE_LEN] {
        let mut s = [0u8; STATE_LEN];
        s[0] = self.inventory.held_slot as u8;
        let flags = [
            self.physics.crouching,
            self.physics.sprinting,
            self.using_item,
            self.using_offhand,
            self.breaking_block,
            self.inventory.screen_open,
            self.chat_input.is_some(),
            self.module_menu_open,
        ];
        s[1] = flags.iter().enumerate().fold(0, |b, (i, &an)| b | (u8::from(an) << i));
        s[2] = self.own_swing_ticks.min(255) as u8;
        s[3..5].copy_from_slice(&(self.own_use_ticks.min(u32::from(u16::MAX)) as u16).to_le_bytes());
        s[5..9].copy_from_slice(&self.breaking_progress.to_le_bytes());
        let (x, y, z) = self.breaking_pos.unwrap_or((0, 0, 0));
        s[9..13].copy_from_slice(&x.to_le_bytes());
        s[13..17].copy_from_slice(&y.to_le_bytes());
        s[17..21].copy_from_slice(&z.to_le_bytes());
        s
    }
}
