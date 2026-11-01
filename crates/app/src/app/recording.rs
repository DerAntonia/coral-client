// coral-record, App-Seite (s. crates/record): Kamera (K), Maus (M), lokaler Zustand (L) und
// Markierungen (E) fuer die Aufnahme des Tabs vorn. Die Pakete (S, C) schreibt die Verbindung
// selbst; ihre Sitzungsnummer steht im record_slot des Tabs.
//
// Maus und lokaler Zustand nur bei Fokus und im Spiel (dieselbe Regel wie input_log): die
// Raw-Maus laeuft fokusunabhaengig.

use crate::{App, Instant, AppMode, Perspective};

// Lokaler Zustand: Slot, Flags, Schwung, Benutzen, Abbaufortschritt, Abbau-Block.
const STATE_LEN: usize = 1 + 1 + 1 + 2 + 4 + 12;

#[derive(Default)]
pub(crate) struct AppRecorder {
    producer: record::Producer,
    // Die Sitzung, an die zuletzt geschrieben wurde (0 = keine).
    session: u64,
    last_state: Option<[u8; STATE_LEN]>,
}

impl AppRecorder {
    pub(crate) fn flush(&mut self) {
        self.producer.flush();
    }
}

impl App {
    fn record_session(&self) -> u64 {
        self.tabs.get(self.active_tab).map_or(0, |t| t.record_slot.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub(crate) fn record_mouse(&mut self, dx: f64, dy: f64, at: Instant) {
        if !self.focused || self.mode != AppMode::World {
            return;
        }
        let id = self.record_session();
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&(dx as f32).to_le_bytes());
        b[4..].copy_from_slice(&(dy as f32).to_le_bytes());
        self.recorder.producer.push(id, record::Tag::M, at, &b);
    }

    // Einmal je Bild, nach dem Setzen der Kamera.
    pub(crate) fn record_frame(&mut self) {
        let id = self.record_session();
        let jetzt = Instant::now();
        if id != self.recorder.session {
            let alt = self.recorder.session;
            self.recorder.producer.push(alt, record::Tag::E, jetzt, &[record::Mark::Back as u8]);
            let packs = self.settings.active_skins.join("\n");
            self.recorder.producer.push_parts(id, record::Tag::E, jetzt, &[&[record::Mark::Front as u8]]);
            self.recorder.producer.push_parts(id, record::Tag::E, jetzt, &[&[record::Mark::Packs as u8], packs.as_bytes()]);
            self.recorder.session = id;
            self.recorder.last_state = None;
        }
        if id != 0 {
            // Im Menue-Hintergrund steht die Kamera in der Startszene, nicht beim Spieler.
            if self.hero_fov.is_none() {
                let c = &self.camera;
                let mut k = [0u8; 37];
                k[..8].copy_from_slice(&c.position.x.to_le_bytes());
                k[8..16].copy_from_slice(&c.position.y.to_le_bytes());
                k[16..24].copy_from_slice(&c.position.z.to_le_bytes());
                k[24..28].copy_from_slice(&c.yaw.to_le_bytes());
                k[28..32].copy_from_slice(&c.pitch.to_le_bytes());
                k[32..36].copy_from_slice(&c.fov_degrees.to_le_bytes());
                k[36] = Perspective::ALL.iter().position(|p| *p == self.perspective).unwrap_or(0) as u8;
                self.recorder.producer.push(id, record::Tag::K, jetzt, &k);
            }
            if self.focused && self.mode == AppMode::World {
                let zustand = self.record_state();
                if self.recorder.last_state != Some(zustand) {
                    self.recorder.last_state = Some(zustand);
                    self.recorder.producer.push(id, record::Tag::L, jetzt, &zustand);
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
