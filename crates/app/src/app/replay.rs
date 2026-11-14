// Wiedergabe von coral-record-Aufnahmen (s. crates/record, net::replay). Die Liste der
// Aufnahmen liegt im Connect-Menue links (Kachel "Aufnahmen" unten links). Abgespielt wird ueber
// denselben Weg wie eine Verbindung (start_session), nur kommen die Pakete aus der Datei.
//
// Der eigene Spieler bekommt keine Eingaben: Kamera aus der Spur K, Hotbar-Slot, Benutzen,
// Schleichen und Schwung aus L. Die Tasten steuern stattdessen die Wiedergabe (s. replay_key).

use crate::{theme, shared_config_dir, Rect, server_panel, App, Button, ButtonAction, UI_SCALE_SMALL, UiRect, UiText, UI_SCALE_NORMAL, AppMode, MenuScreen, DVec3, GpuState, Perspective};
use std::path::PathBuf;
use std::sync::Arc;

// Ein Eintrag der Liste: Kopf und Laenge (s. record::summary).
pub(crate) struct RecordingEntry {
    pub(crate) path: PathBuf,
    pub(crate) header: record::Header,
    pub(crate) duration_ms: f64,
    // Ab dem Join oder mit Snapshot.
    pub(crate) playable: bool,
}

pub(crate) struct Replay {
    pub(crate) path: PathBuf,
    pub(crate) server: String,
    pub(crate) clock: Arc<record::ReplayClock>,
    // Kamera, Zustand, Laenge -- liest ein Hintergrund-Thread (s. record::load_tracks).
    tracks: Option<Arc<record::Tracks>>,
    loading: Option<std::sync::mpsc::Receiver<Result<record::Tracks, String>>>,
    // Index des zuletzt angewandten L-Eintrags (+1), damit nur Aenderungen greifen.
    state_applied: usize,
    // Bis hierher sind die Pakete abgearbeitet (ms Aufnahmezeit, s. ClientEvent::ReplayAt).
    processed_ms: f64,
    // Gespult: dorthin wird gerade nachgeholt (in der Leiste blau), bis processed_ms dort ist.
    seek_to: Option<f64>,
    // Mit F5 selbst gewaehlte Ansicht -- gilt statt der aufgenommenen, solange die Wiedergabe laeuft.
    perspective: Option<Perspective>,
    // Inventar-Taste in der Wiedergabe gedrueckt: Inventar bleibt offen (Umschalter). Sonst
    // offen, wenn es in der Aufnahme offen war.
    inventory_forced: bool,
    recorded_inventory: bool,
    // Leiste gerade gedrueckt (Ziehen): beim Loslassen dorthin.
    bar_drag: bool,
}

// Die Leiste oben (Nutzer 2026-11-12, sonst wie osu): Rand links/rechts, Abstand oben, Hoehe
// normal und beim Hovern (sie waechst nach unten), und wie weit unter ihr der Hover noch greift.
const BAR_MARGIN_X: f32 = 0.05;
const BAR_TOP: f32 = 14.0;
const BAR_H: f32 = 6.0;
const BAR_H_HOVER: f32 = 22.0;
const BAR_HIT_H: f32 = 40.0;
// So lange darf drain_events je Bild in der Wiedergabe arbeiten -- beim Nachholen nach dem
// Spulen bleibt das Bild so fluessig, und die Leiste zeigt den Fortschritt.
pub(crate) const REPLAY_DRAIN_BUDGET: std::time::Duration = std::time::Duration::from_millis(8);

// Spulen je Pfeiltaste.
const SEEK_MS: f64 = 5000.0;
const SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

// Zeilen der Liste.
const ROW_H: f32 = 56.0;
const ROW_GAP: f32 = 6.0;

pub(crate) fn recordings_dir() -> Option<PathBuf> {
    shared_config_dir().map(|d| d.join("recordings"))
}

fn zeit(ms: f64) -> String {
    let s = (ms / 1000.0).max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

// Die Flaeche der Liste: links neben der Serverliste, wo frueher das Log stand.
fn panel() -> Rect {
    let p = server_panel();
    Rect { x: 40.0, y: p.y, w: (p.x - 80.0).max(300.0), h: p.h }
}

fn row_rect(i: usize) -> Rect {
    let p = panel();
    Rect { x: p.x + 12.0, y: p.y + 48.0 + i as f32 * (ROW_H + ROW_GAP), w: p.w - 24.0, h: ROW_H }
}

impl App {
    pub(crate) fn toggle_recordings(&mut self) {
        self.recordings_open = !self.recordings_open;
        if self.recordings_open {
            self.refresh_recordings();
        }
    }

    // Neueste zuerst (die Dateinamen beginnen mit Datum und Uhrzeit).
    pub(crate) fn refresh_recordings(&mut self) {
        let mut liste = Vec::new();
        if let Some(dir) = recordings_dir() {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = e.path();
                if path.extension().is_some_and(|x| x == "crec") {
                    if let Ok(s) = record::summary(&path) {
                        liste.push(RecordingEntry { path, header: s.header, duration_ms: s.duration_ms, playable: s.playable });
                    }
                }
            }
        }
        liste.sort_by(|a, b| b.path.cmp(&a.path));
        self.recordings = liste;
    }

    fn rows_visible(&self) -> usize {
        let p = panel();
        (((p.h - 48.0) / (ROW_H + ROW_GAP)).floor().max(0.0) as usize).min(self.recordings.len())
    }

    pub(crate) fn recordings_buttons(&self, out: &mut Vec<Button>) {
        if !self.recordings_open {
            return;
        }
        for i in 0..self.rows_visible() {
            let e = &self.recordings[i];
            out.push(Button { action: ButtonAction::PlayRecording(i), rect: row_rect(i), label: String::new(), enabled: e.playable, color: [0.0; 4], scale: UI_SCALE_SMALL, slider: None });
        }
    }

    pub(crate) fn build_recordings_panel(&self, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        use theme::Slot;
        if !self.recordings_open {
            return;
        }
        let g = theme::get;
        let p = panel();
        rects.push(UiRect::framed(p, g(Slot::BgPanel), 10.0, 1.0, g(Slot::Border1)));
        texts.push(UiText { text: "Aufnahmen".into(), x: p.x + 16.0, y: p.y + 14.0, color: theme::text(), scale: UI_SCALE_NORMAL });
        if self.recordings.is_empty() {
            let hinweis = "Noch keine Aufnahmen -- Modul Coral-Record einschalten und spielen.";
            texts.push(UiText { text: hinweis.into(), x: p.x + 16.0, y: p.y + 56.0, color: theme::text_2(), scale: UI_SCALE_SMALL });
            return;
        }
        let (cx, cy) = self.cursor_pos;
        for i in 0..self.rows_visible() {
            let e = &self.recordings[i];
            let r = row_rect(i);
            let abspielbar = e.playable;
            let k = self.hover_level(r, abspielbar && r.contains(cx, cy));
            rects.push(UiRect::framed(r, crate::mix_color(g(Slot::Surface2), g(Slot::Surface3), k), 8.0, 1.0, g(Slot::Border1)));
            let start = chrono::DateTime::from_timestamp_millis(e.header.wall_ms as i64)
                .map_or_else(|| "?".to_string(), |t| t.with_timezone(&chrono::Local).format("%d.%m.%Y %H:%M").to_string());
            let farbe = if abspielbar { theme::text() } else { g(Slot::TextDisabled) };
            texts.push(UiText { text: format!("{start}  {}", e.header.server), x: r.x + 12.0, y: r.y + 9.0, color: farbe, scale: UI_SCALE_SMALL });
            let zweite = if abspielbar {
                format!("{} · {}", zeit(e.duration_ms), e.header.name)
            } else {
                format!("{} · beginnt mitten im Spiel, ohne Anfangszustand", zeit(e.duration_ms))
            };
            texts.push(UiText { text: zweite, x: r.x + 12.0, y: r.y + 31.0, color: if abspielbar { theme::text_2() } else { g(Slot::TextDisabled) }, scale: UI_SCALE_SMALL });
        }
    }

    pub(crate) fn play_recording(&mut self, i: usize) {
        let Some(e) = self.recordings.get(i) else { return };
        if !e.playable {
            return;
        }
        let path = e.path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let pfad = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(record::load_tracks(&pfad).map_err(|e| e.to_string()));
        });
        self.replay = Some(Replay {
            path,
            server: e.header.server.clone(),
            clock: Arc::new(record::ReplayClock::new(0.0)),
            tracks: None,
            loading: Some(rx),
            state_applied: 0,
            processed_ms: 0.0,
            seek_to: None,
            perspective: None,
            inventory_forced: false,
            recorded_inventory: false,
            bar_drag: false,
        });
        self.recordings_open = false;
        self.begin_replay();
    }

    // (Neu) abspielen ab der Uhr der Wiedergabe: die Welt von vorn, alles bis zur Uhrzeit ohne
    // Warten (s. record::replay).
    fn begin_replay(&mut self) {
        let Some(r) = self.replay.as_mut() else { return };
        r.state_applied = 0;
        r.processed_ms = 0.0;
        // Neuer Durchlauf ab dem Anfang: alles bis zur Uhr wird nachgeholt (blau).
        let ziel = r.clock.now_ms();
        r.seek_to = (ziel > 0.0).then_some(ziel);
        let start = (r.path.clone(), Arc::clone(&r.clock), r.server.clone());
        self.column_history.clear();
        self.world_reset_debug = None;
        self.start_session(Some(start));
        self.mode = AppMode::World;
        self.screen = MenuScreen::Main;
    }

    pub(crate) const fn replay_active(&self) -> bool {
        self.replay.is_some()
    }

    // Tasten in der Wiedergabe. true = verbraucht.
    pub(crate) fn replay_key(&mut self, code: winit::keyboard::KeyCode) -> bool {
        use winit::keyboard::KeyCode;
        let Some(r) = self.replay.as_ref() else { return false };
        let clock = Arc::clone(&r.clock);
        let dauer = r.tracks.as_ref().map_or(f64::INFINITY, |t| t.duration_ms);
        match code {
            KeyCode::Space => clock.set_paused(!clock.paused()),
            KeyCode::ArrowRight => self.replay_seek((clock.now_ms() + SEEK_MS).min(dauer)),
            KeyCode::ArrowLeft => self.replay_seek((clock.now_ms() - SEEK_MS).max(0.0)),
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let i = SPEEDS.iter().position(|&s| s >= clock.speed()).unwrap_or(2);
                let j = if code == KeyCode::ArrowUp { (i + 1).min(SPEEDS.len() - 1) } else { i.saturating_sub(1) };
                clock.set_speed(SPEEDS[j]);
            }
            _ => return false,
        }
        true
    }

    // Zu einer Aufnahmezeit springen. Nach vorn stellt nur die Uhr, die Pakete bis dahin werden
    // nachgeholt (wie in osu). Zurueck geht nur mit einem neuen Durchlauf ab dem Anfang (die
    // Welt laesst sich nicht zurueckdrehen, s. record::replay), der bis zum Ziel nachholt.
    pub(crate) fn replay_seek(&mut self, ziel: f64) {
        let Some(r) = self.replay.as_mut() else { return };
        let ziel = ziel.max(0.0);
        if ziel >= r.processed_ms {
            r.clock.seek(ziel);
            r.seek_to = Some(ziel);
            return;
        }
        let neu = record::ReplayClock::new(ziel);
        neu.set_speed(r.clock.speed());
        neu.set_paused(r.clock.paused());
        r.clock = Arc::new(neu);
        self.begin_replay();
    }

    // Abgearbeiteter Stand vom Netz-Task (s. ClientEvent::ReplayAt).
    pub(crate) fn replay_processed(&mut self, t: f64) {
        let Some(r) = self.replay.as_mut() else { return };
        r.processed_ms = t;
        let dauer = r.tracks.as_ref().map_or(f64::INFINITY, |t| t.duration_ms);
        if r.seek_to.is_some_and(|z| t >= z - 100.0 || t >= dauer - 200.0) {
            r.seek_to = None;
        }
    }

    // Die Leiste: Trefferflaeche (zum Hovern und Klicken) und die Bahn selbst bei Anteil
    // `hover` (0 normal, 1 ganz hoch). In Bildschirmpixeln.
    fn replay_bar_rects(screen: (f32, f32), hover: f32) -> (Rect, Rect) {
        let (sw, _) = screen;
        let x = (sw * BAR_MARGIN_X).round();
        let w = sw - 2.0 * x;
        let h = BAR_H + (BAR_H_HOVER - BAR_H) * hover;
        (Rect { x, y: 0.0, w, h: BAR_TOP + BAR_HIT_H }, Rect { x, y: BAR_TOP, w, h: h.round() })
    }

    fn replay_bar_time_at(&self, px: f32) -> Option<f64> {
        let gpu = self.gpu.as_ref()?;
        let dauer = self.replay.as_ref()?.tracks.as_ref()?.duration_ms;
        let (_, bahn) = Self::replay_bar_rects(gpu.size(), 0.0);
        Some((f64::from((px - bahn.x) / bahn.w)).clamp(0.0, 1.0) * dauer)
    }

    // Linke Maustaste in der Wiedergabe: auf der Leiste dorthin springen, Ziehen springt beim
    // Loslassen nach. true = auf der Leiste.
    pub(crate) fn replay_mouse_left(&mut self, pressed: bool) -> bool {
        let Some(gpu) = self.gpu.as_ref() else { return false };
        let (treffer, _) = Self::replay_bar_rects(gpu.size(), 0.0);
        let (px, py) = self.cursor_pos;
        let Some(r) = self.replay.as_mut() else { return false };
        if pressed {
            if !treffer.contains(px, py) {
                return false;
            }
            r.bar_drag = true;
        } else {
            if !std::mem::take(&mut r.bar_drag) {
                return false;
            }
        }
        if let Some(ziel) = self.replay_bar_time_at(px) {
            // Beim Loslassen nur, wenn wirklich gezogen wurde.
            let jetzt = self.replay.as_ref().map_or(0.0, |r| r.clock.now_ms());
            if pressed || (ziel - jetzt).abs() > 250.0 {
                self.replay_seek(ziel);
            }
        }
        true
    }

    // Tasten in der Wiedergabe, die das Zuschauen betreffen: Perspektive (F5), Freelook und
    // Inventar. Alles andere bleibt aus -- der eigene Spieler bekommt keine Eingaben.
    pub(crate) fn replay_view_action(&mut self, action: crate::KeyBindAction, pressed: bool) {
        use crate::KeyBindAction as A;
        match action {
            A::TogglePerspective if pressed && !self.freelook_held => {
                self.perspective = self.next_perspective();
                if let Some(r) = self.replay.as_mut() {
                    r.perspective = Some(self.perspective);
                }
            }
            A::Freelook => self.apply_key_action(action, pressed),
            A::OpenInventory if pressed => {
                if let Some(r) = self.replay.as_mut() {
                    r.inventory_forced = !r.inventory_forced;
                }
            }
            _ => {}
        }
    }

    // Einmal je Bild, bevor die Kamera gebraucht wird: Blick aus K (zwischen zwei Eintraegen
    // gemittelt), Zustand aus L.
    pub(crate) fn apply_replay_view(&mut self) {
        let Some(r) = self.replay.as_mut() else { return };
        if let Some(rx) = &r.loading {
            match rx.try_recv() {
                Ok(Ok(t)) => {
                    r.tracks = Some(Arc::new(t));
                    r.loading = None;
                }
                Ok(Err(e)) => {
                    r.loading = None;
                    self.push_log(format!("Aufnahme: Spuren nicht lesbar ({e})"));
                    return;
                }
                Err(_) => return,
            }
        }
        let Some(tracks) = r.tracks.clone() else { return };
        let t = r.clock.now_ms().min(tracks.duration_ms);
        let eigene_ansicht = r.perspective;
        if let Some((pos, yaw, pitch, fov, perspektive)) = camera_at(&tracks.camera, t) {
            self.camera.position = pos;
            self.camera.yaw = yaw;
            self.camera.pitch = pitch;
            self.camera.fov_degrees = fov;
            // Selbst gewaehlte Ansicht (F5) vor der aufgenommenen; Freelook fuehrt seine eigene.
            if !self.freelook_held {
                self.perspective = eigene_ansicht.unwrap_or(perspektive);
            }
            // Der eigene Koerper (3. Person, Schatten) steht dort, wo die Kamera ist.
            self.physics.pos = pos - DVec3::new(0.0, f64::from(self.physics.eye_height()), 0.0);
            // Der Koerper wird zwischen zwei Tick-Positionen gemittelt (physics_render_prev ->
            // pos). Die Kamera kommt hier aber je Bild aus der Aufnahme -- ohne das hinge der
            // Koerper bis zu einen Tick hinterher und sprang je Tick nach: in F5 ruckelte alles
            // (Nutzer 2026-11-13).
            self.physics_render_prev = self.physics.pos;
        }
        let n = tracks.state.partition_point(|(ts, _)| *ts <= t);
        let r = self.replay.as_mut().expect("oben geprueft");
        if n != r.state_applied {
            r.state_applied = n;
            if let Some((_, s)) = n.checked_sub(1).and_then(|i| tracks.state.get(i)) {
                self.apply_replay_state(s);
            }
        }
        // Inventar: offen, wenn selbst aufgemacht (Taste) oder in der Aufnahme offen. Ein Container
        // des Servers (Truhe ...) kommt mit den Paketen und bleibt dessen Sache.
        let r = self.replay.as_ref().expect("oben geprueft");
        let offen = r.inventory_forced || r.recorded_inventory;
        if self.inventory.open.is_none() {
            self.inventory.screen_open = offen;
        }
    }

    // Ein L-Eintrag (Aufbau s. app/recording.rs, record_state).
    fn apply_replay_state(&mut self, s: &[u8]) {
        if s.len() < 21 {
            return;
        }
        self.inventory.held_slot = (s[0] as usize).min(8);
        let flag = |i: u32| s[1] & (1 << i) != 0;
        self.sneak_held = flag(0);
        self.using_item = flag(2);
        self.using_offhand = flag(3);
        self.own_swing_ticks = u32::from(s[2]);
        self.own_use_ticks = u32::from(u16::from_le_bytes([s[3], s[4]]));
        if let Some(r) = self.replay.as_mut() {
            r.recorded_inventory = flag(5);
        }
    }

    // Die Leiste oben, sonst wie in osu: ueber die ganze Breite, beim Hovern hoeher. Weiss ist
    // gespielt (abgearbeitet), blau wird nach dem Spulen gerade nachgeholt. Darunter links die
    // Zeit (dazu Pause/Tempo, wenn nicht normal), rechts die Laenge; beim Hovern unter dem
    // Zeiger die Zeit an dieser Stelle.
    pub(crate) fn build_replay_bar(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        use theme::Slot;
        let Some(r) = self.replay.as_ref() else { return };
        let g = theme::get;
        let dauer = r.tracks.as_ref().map_or(0.0, |t| t.duration_ms);
        let jetzt = r.clock.now_ms().min(dauer.max(0.0));
        let (treffer, _) = Self::replay_bar_rects(gpu.size(), 0.0);
        let (px, py) = self.cursor_pos;
        let zeiger_frei = !self.freelook_held;
        let ueber = zeiger_frei && (r.bar_drag || treffer.contains(px, py));
        let hoch = self.hover_level(treffer, ueber);
        let (_, bahn) = Self::replay_bar_rects(gpu.size(), hoch);
        rects.push(UiRect { rect: bahn, color: g(Slot::Surface3), ..UiRect::FLAT });
        if dauer > 0.0 {
            let x_bei = |ms: f64| bahn.x + bahn.w * (ms / dauer).clamp(0.0, 1.0) as f32;
            let weiss_bis = if r.seek_to.is_some() { r.processed_ms.min(jetzt) } else { jetzt };
            rects.push(UiRect { rect: Rect { w: (x_bei(weiss_bis) - bahn.x).max(0.0), ..bahn }, color: theme::text(), ..UiRect::FLAT });
            if let Some(ziel) = r.seek_to {
                let von = x_bei(r.processed_ms.min(ziel));
                rects.push(UiRect { rect: Rect { x: von, w: (x_bei(ziel) - von).max(0.0), ..bahn }, color: g(Slot::Accent), ..UiRect::FLAT });
            }
        }
        let zeile_y = bahn.y + bahn.h + 6.0;
        let tempo = r.clock.speed();
        let mut links = zeit(jetzt);
        if r.clock.paused() {
            links.push_str("  ·  Pause");
        }
        if (tempo - 1.0).abs() > 1e-6 {
            links.push_str(&format!("  ·  {tempo}×"));
        }
        texts.push(UiText { text: links, x: bahn.x, y: zeile_y, color: theme::text(), scale: UI_SCALE_SMALL });
        let rechts = zeit(dauer);
        let rw = gpu.measure_text(&rechts, UI_SCALE_SMALL);
        texts.push(UiText { text: rechts, x: bahn.x + bahn.w - rw, y: zeile_y, color: theme::text(), scale: UI_SCALE_SMALL });
        if ueber && dauer > 0.0 {
            let ms = (f64::from((px - bahn.x) / bahn.w)).clamp(0.0, 1.0) * dauer;
            let tip = format!("{} ({:.0} %)", zeit(ms), ms / dauer * 100.0);
            let tw = gpu.measure_text(&tip, UI_SCALE_SMALL);
            let th = gpu.text_height(UI_SCALE_SMALL);
            let kasten = Rect { x: (px - tw / 2.0 - 8.0).clamp(bahn.x, bahn.x + bahn.w - tw - 16.0).round(), y: (zeile_y + th + 10.0).round(), w: tw + 16.0, h: th + 10.0 };
            rects.push(UiRect::framed(kasten, g(Slot::BgPanel), 6.0, 1.0, g(Slot::Border1)));
            texts.push(UiText { text: tip, x: kasten.x + 8.0, y: kasten.y + 5.0, color: theme::text(), scale: UI_SCALE_SMALL });
        }
    }
}

// Blick zur Zeit `t`: zwischen den beiden umgebenden K-Eintraegen linear, Yaw ueber den kurzen
// Weg. Aufbau eines Eintrags s. app/recording.rs (record_frame).
fn camera_at(camera: &[(f64, [u8; 37])], t: f64) -> Option<(DVec3, f32, f32, f32, Perspective)> {
    let n = camera.partition_point(|(tk, _)| *tk <= t);
    let lies = |k: &[u8; 37]| {
        let f64_at = |a: usize| f64::from_le_bytes(k[a..a + 8].try_into().unwrap());
        let f32_at = |a: usize| f32::from_le_bytes(k[a..a + 4].try_into().unwrap());
        (DVec3::new(f64_at(0), f64_at(8), f64_at(16)), f32_at(24), f32_at(28), f32_at(32), k[36])
    };
    let (a, b) = match (n.checked_sub(1).and_then(|i| camera.get(i)), camera.get(n)) {
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) => (a, a),
        (None, Some(b)) => (b, b),
        (None, None) => return None,
    };
    let (pa, ya, pia, fa, pera) = lies(&a.1);
    let (pb, yb, pib, fb, _) = lies(&b.1);
    let f = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
    let dyaw = ((yb - ya + 540.0).rem_euclid(360.0)) - 180.0;
    let perspektive = Perspective::ALL.get(pera as usize).copied().unwrap_or(Perspective::FirstPerson);
    Some((pa + (pb - pa) * f, ya + dyaw * f as f32, pia + (pib - pia) * f as f32, fa + (fb - fa) * f as f32, perspektive))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(pos: [f64; 3], yaw: f32) -> [u8; 37] {
        let mut b = [0u8; 37];
        for (i, v) in pos.iter().enumerate() {
            b[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        b[24..28].copy_from_slice(&yaw.to_le_bytes());
        b[32..36].copy_from_slice(&70f32.to_le_bytes());
        b
    }

    #[test]
    fn kamera_wird_zwischen_zwei_eintraegen_gemittelt() {
        let spur = vec![(0.0, k([0.0, 64.0, 0.0], 170.0)), (100.0, k([10.0, 64.0, 0.0], -170.0))];
        let (pos, yaw, _, fov, _) = camera_at(&spur, 50.0).unwrap();
        assert!((pos.x - 5.0).abs() < 1e-9);
        // Ueber den kurzen Weg: 170 -> 190 (= -170), Mitte 180.
        assert!((yaw - 180.0).abs() < 1e-3, "{yaw}");
        assert_eq!(fov, 70.0);
        assert_eq!(camera_at(&spur, 500.0).unwrap().0.x, 10.0);
        assert!(camera_at(&[], 0.0).is_none());
    }
}
