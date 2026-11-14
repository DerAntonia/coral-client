// coral-clip, App-Seite (s. record::clip): wie ein Instant Replay. Die letzten Sekunden der
// Verbindung vorn liegen in einem Puffer im Speicher; die Taste "Clip speichern" schreibt sie
// als .crec-Datei zu den Aufnahmen.
//
// Damit ein Clip mitten im Spiel beginnen kann, sammelt die App regelmaessig ihren Zustand
// ein (Haltepunkt, s. snapshot.rs: unkodiert, die Chunk-Spalten als Arc). Ein Clip beginnt
// immer an einem Haltepunkt: am juengsten, der mindestens die eingestellte Dauer alt ist. Er
// ist also mindestens so lang wie eingestellt und hoechstens um den Abstand der Haltepunkte
// laenger (Settings::clip_checkpoints je Dauer, Vorgabe vier). Dazu setzt der Netz-Task vor
// jeder Teleportation einen (s. record::clip).
//
// Nur der Tab vorn bekommt Haltepunkte; ein Tab hinten verarbeitet nichts, sein Zustand steht.

use super::snapshot::SnapshotJob;
use crate::{notifications, App, AppMode};
use std::collections::VecDeque;
use std::sync::Arc;

// Kleinster Abstand der regelmaessigen Haltepunkte in ms (s. Settings::clip_checkpoints).
const CHECKPOINT_MIN_MS: f64 = 1000.0;
// So lange wartet die App auf einen erbetenen Haltepunkt, bevor sie erneut bittet.
const REQUEST_WAIT_MS: f64 = 1000.0;
// Ab so vielen ms im Hauptthread steht das Einsammeln im Log.
const COLLECT_LOG_MS: f64 = 2.0;
// So lange nach dem Tastendruck wird geschrieben: die letzten Pakete liegen noch bis zu 200 ms
// beim Netz-Task (record::FLUSH_EVERY), sie sollen mit in den Clip.
const SAVE_DELAY_MS: f64 = 300.0;

// Ein bestellter Clip: Puffer, Haltepunkt des Anfangs und dessen Zustand.
struct PendingSave {
    due_ms: f64,
    session: u64,
    seq: u32,
    job: Option<Arc<SnapshotJob>>,
}

struct Checkpoint {
    seq: u32,
    t_ms: f64,
    // None beim Haltepunkt 0 einer Sitzung ab Join: der Anfang braucht keinen Zustand.
    job: Option<Arc<SnapshotJob>>,
}

struct ClipState {
    session: u64,
    checkpoints: VecDeque<Checkpoint>,
    // Wann zuletzt um einen Haltepunkt gebeten wurde (ms, s. record::now_ms).
    asked_ms: f64,
}

// Ein Speichervorgang, dessen Antwort noch aussteht: die Sitzungsnummer der (moeglichen) neuen
// Datei und der Zustand ihres Anfangs, falls sie einen Snapshot braucht.
struct Saving {
    antwort: std::sync::mpsc::Receiver<Result<record::ClipSaved, String>>,
    datei: u64,
    job: Option<Arc<SnapshotJob>>,
}

#[derive(Default)]
pub(crate) struct ClipBuffers {
    states: Vec<ClipState>,
    // Laufende Speichervorgaenge: der Schreib-Thread meldet, was daraus wurde.
    saving: Vec<Saving>,
    pending: Option<PendingSave>,
}

impl App {
    fn clip_ms(&self) -> f64 {
        f64::from(self.settings.clip_seconds) * 1000.0
    }

    pub(crate) fn clip_started(&mut self, session: u64, from_join: bool, t_ms: f64) {
        let mut checkpoints = VecDeque::new();
        if from_join {
            checkpoints.push_back(Checkpoint { seq: 0, t_ms, job: None });
        }
        // Ab der Mitte fehlt der Anfang: sofort um einen Haltepunkt bitten (s. tick_clip).
        let asked_ms = if from_join { t_ms } else { f64::NEG_INFINITY };
        self.clips.states.push(ClipState { session, checkpoints, asked_ms });
    }

    // Der Netz-Task hat den Haltepunkt gesetzt: alles davor ist verarbeitet, genau jetzt gilt
    // der Zustand.
    pub(crate) fn clip_checkpoint(&mut self, session: u64, seq: u32, t_ms: f64) {
        if !self.clips.states.iter().any(|s| s.session == session) {
            return;
        }
        let start = std::time::Instant::now();
        let job = Arc::new(self.collect_snapshot());
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if ms >= COLLECT_LOG_MS {
            eprintln!("coral-clip: Haltepunkt {seq} in {ms:.1} ms eingesammelt");
        }
        if let Some(s) = self.clips.states.iter_mut().find(|s| s.session == session) {
            s.checkpoints.push_back(Checkpoint { seq, t_ms, job: Some(job) });
        }
    }

    // Einmal je Bild.
    pub(crate) fn tick_clip(&mut self) {
        let mut i = 0;
        while i < self.clips.saving.len() {
            match self.clips.saving[i].antwort.try_recv() {
                Ok(ergebnis) => {
                    let vorgang = self.clips.saving.swap_remove(i);
                    self.clip_saved(vorgang, ergebnis);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.clips.saving.swap_remove(i);
                }
            }
        }
        if self.clips.pending.as_ref().is_some_and(|p| record::now_ms() >= p.due_ms) {
            if let Some(p) = self.clips.pending.take() {
                self.write_clip(p);
            }
        }
        // Puffer beendeter Verbindungen: ihre Haltepunkte halten sonst alte Chunks fest.
        let tabs = &self.tabs;
        self.clips.states.retain(|s| tabs.iter().any(|t| t.clip_link.session() == s.session));
        let Some(link) = self.tabs.get(self.active_tab).map(|t| Arc::clone(&t.clip_link)) else { return };
        let session = link.session();
        let dauer = self.clip_ms();
        let im_spiel = self.mode == AppMode::World;
        let self_checkpoints = self.settings.clip_checkpoints;
        // Ein bestellter Clip braucht seinen Anfang noch.
        let bestellt = self.clips.pending.is_some();
        let Some(st) = self.clips.states.iter_mut().find(|s| s.session == session) else { return };
        let jetzt = record::now_ms();
        // Was der Puffer nach einem Ueberlauf nicht mehr lueckenlos hat, taugt nicht als Anfang.
        let floor = link.floor();
        st.checkpoints.retain(|c| c.seq >= floor);
        // Der zweite ist alt genug: der erste wird nicht mehr gebraucht.
        while !bestellt && st.checkpoints.len() >= 2 && jetzt - st.checkpoints[1].t_ms >= dauer {
            st.checkpoints.pop_front();
            record::clip_trim(session, st.checkpoints[0].seq);
        }
        let abstand = (dauer / f64::from(self_checkpoints.max(1))).max(CHECKPOINT_MIN_MS);
        let faellig = if st.checkpoints.is_empty() { REQUEST_WAIT_MS } else { abstand };
        // Ein Haltepunkt von anderswo (Teleportation) zaehlt mit: der naechste regelmaessige
        // kommt einen Abstand nach dem juengsten.
        let juengster = st.checkpoints.back().map_or(f64::NEG_INFINITY, |c| c.t_ms).max(st.asked_ms);
        if im_spiel && jetzt - juengster >= faellig {
            st.asked_ms = jetzt;
            link.request();
        }
    }

    // Taste "Clip speichern".
    pub(crate) fn save_clip(&mut self) {
        if !self.settings.module_coral_clip {
            return;
        }
        let session = self.tabs.get(self.active_tab).map_or(0, |t| t.clip_link.session());
        let anfang = self.clips.states.iter().find(|s| s.session == session).and_then(|s| s.checkpoints.front()).map(|c| (c.seq, c.job.clone()));
        let Some((seq, job)) = anfang else {
            self.notify(notifications::Notification::warning("Kein Clip", "Der Puffer hat noch keinen Anfang."));
            return;
        };
        if self.clips.pending.is_none() {
            self.clips.pending = Some(PendingSave { due_ms: record::now_ms() + SAVE_DELAY_MS, session, seq, job });
        }
    }

    fn write_clip(&mut self, p: PendingSave) {
        self.recorder.flush();
        let Some((datei, antwort)) = record::clip_save(p.session, p.seq) else {
            self.notify(notifications::Notification::error("Clip nicht gespeichert", "Der Schreib-Thread ist nicht erreichbar."));
            return;
        };
        self.clips.saving.push(Saving { antwort, datei, job: p.job });
    }

    // Antwort des Schreib-Threads. Eine neue Datei ab einem Haltepunkt wartet jetzt auf ihren
    // Snapshot; ein Clip, der mit dem vorigen ueberlappt, hat diesen nur verlaengert (nichts
    // steht doppelt) und braucht keinen.
    fn clip_saved(&mut self, vorgang: Saving, ergebnis: Result<record::ClipSaved, String>) {
        let clip = match ergebnis {
            Ok(clip) => clip,
            Err(grund) => {
                self.notify(notifications::Notification::error("Clip nicht gespeichert", grund));
                return;
            }
        };
        if clip.needs_snapshot {
            let gestartet = match vorgang.job {
                Some(job) => super::snapshot::encode_snapshot(job, vorgang.datei, true, "coral-clip").map_err(|e| e.to_string()),
                None => Err("kein Zustand".to_string()),
            };
            if let Err(e) = gestartet {
                self.push_log(format!("coral-clip: Snapshot nicht schreibbar ({e})"));
                record::close(vorgang.datei);
            }
        }
        let titel = if clip.extended { "Clip verlängert" } else { "Clip gespeichert" };
        self.notify(notifications::Notification::success(titel, clip.path.file_name().unwrap_or_default().to_string_lossy()));
    }
}
