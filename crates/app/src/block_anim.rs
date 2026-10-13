// Animierte Blocktexturen (Lava, Prismarin, Feuer ...) ausserhalb des Render-Threads rechnen.
//
// Ein neues Bild einer Kachel heisst: Frame (ggf. zwischen zwei Frames interpoliert) auf die
// Atlas-Kachelgroesse bringen und alle Mip-Stufen samt Alpha-Korrektur bauen. Einzeln billig,
// aber die interpolierten Animationen aendern sich JEDEN Tick, und alle zusammen kosteten im
// Render-Thread bis ~5 ms -- im Frame-Profiler genau jeder sechste Frame (ein Tick bei 120 FPS),
// der dadurch zu spaet beim Monitor ankam: der Riss sprang fuer einen Frame an eine andere
// Stelle. Jetzt rechnet ein eigener Thread, der Render-Thread laedt nur noch fertige Bilder
// hoch. Ein Bild kommt dadurch ein, zwei Frames spaeter -- bei 20 Ticks/s unsichtbar.
//
// Seit jede animierte Textur mehrere zeitversetzte Kopien hat (s. assets::ANIM_VARIANTS),
// aendern sich je Tick ~300 Kacheln -- ~900 Kopierbefehle samt Submit kosteten im
// Render-Thread im Mittel 0,16 ms, in Spitzen ueber 3 ms, und im Render-Ahead-0-Modus landet
// das voll auf Frame und Latenz. Deshalb laedt der Thread, sobald er die GPU kennt (s.
// UploadTarget), auch selbst hoch -- aber nur in der Pause nach einem Present (s.
// submit_window). Die Kopien kosten die GPU einige Zehntel Millisekunden, und wann sie laufen,
// entscheidet ueber den Riss (Profiler, jeweils nur Frames mit Animations-Kopien betroffen):
// - irgendwann abgeschickt: mal mitten im Frame, der dann bis ~1,3 ms zu spaet fertig wurde;
// - direkt nach dem Present: ohne Hardware-Flip VOR der Kopie des X-Servers auf den
//   Bildschirm, der Bildwechsel kam jedes Mal spaeter;
// - zu Beginn des Frames: der Frame wartete auf sie und kam um ~0,2 ms zu spaet.
// Ohne Ziel gehen die Bilder wie frueher ueber drain() an den Aufrufer.
//
// Bilder ohne Interpolation wiederholen sich (die Folge laeuft im Kreis) und werden im Thread
// zwischengespeichert; nach dem ersten Durchlauf kostet eine solche Animation nichts mehr.
//
// Feuerkacheln tragen zusaetzlich eine Deckkraft (Modul "Dynamisches Feuer", s.
// App::fire_alpha): sie wird hier ins Alpha aller Mip-Stufen eingerechnet. Feuer liegt im
// durchscheinenden Pass (s. world::mesher::is_fire), die Bloecke blenden damit ohne neues Meshen.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use assets::{AnimationState, TileAnimation};

// Deckkraft in Zwanzigsteln (die Mod blendet in 20 Ticks um je 0,05) -- ganzzahlig, damit sie
// als Teil des Stands verglichen und zwischengespeichert werden kann.
pub const FADE_FULL: u8 = 20;

type Key = (AnimationState, u8);

fn apply_fade(mips: &[Vec<u8>], fade: u8) -> Vec<Vec<u8>> {
    let f = fade as u32;
    mips.iter()
        .map(|level| {
            let mut level = level.clone();
            for px in level.chunks_exact_mut(4) {
                px[3] = ((px[3] as u32 * f + FADE_FULL as u32 / 2) / FADE_FULL as u32) as u8;
            }
            level
        })
        .collect()
}

// Feuer liegt im durchscheinenden Pass, also ohne Alpha-Test: die gemittelten Randtexel der
// kleineren Mip-Stufen (Alpha irgendwo zwischen 0 und 255) standen dort als blasse, halb
// deckende Flaechen im Bild -- in der Ferne die ganzen hohen Feuerebenen ueber dem Block. Im Opak-Pass schnitte der Alpha-
// Test sie bei 0.5 ab; genau das hier, bevor die Deckkraft des Moduls dazukommt.
fn binarize_alpha(mips: &mut [Vec<u8>]) {
    for level in mips {
        for px in level.chunks_exact_mut(4) {
            px[3] = if px[3] >= 128 { 255 } else { 0 };
        }
    }
}

// Wohin der Thread selbst hochlaedt: der Atlas und die GPU, zu der er gehoert. Kommt mit
// jedem Atlas-Neubau neu (s. App::connect_block_animations).
pub struct UploadTarget {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub texture: Arc<wgpu::Texture>,
}

// Pause nach einem Present, in der die Kopien laufen duerfen: ab 1 ms danach (die Kopie des
// X-Servers auf den Bildschirm ist dann eingereiht) bis 3,5 ms danach (der naechste Frame
// beginnt bei 120 Hz fruehestens ~5 ms nach dem Present).
const WINDOW_START_NS: i64 = 1_000_000;
const WINDOW_END_NS: i64 = 3_500_000;
// Laenger wird nicht gewartet: kommt kein Present (Fenster verdeckt, Ladebildschirm) oder liegen
// sie dichter als die Pause (ohne FPS-Grenze), geht es eben so raus.
const MAX_WAIT_NS: i64 = 10_000_000;

// Wartet bis in die naechste Pause nach einem Present (`last_present` in
// vblank::monotonic_ns, 0 = unbekannt).
fn submit_window(last_present: &AtomicI64) {
    let start = crate::vblank::monotonic_ns();
    loop {
        let p = last_present.load(Ordering::Relaxed);
        let now = crate::vblank::monotonic_ns();
        let seit = now - p;
        if p == 0 || now - start > MAX_WAIT_NS || (WINDOW_START_NS..=WINDOW_END_NS).contains(&seit) {
            return;
        }
        // Vor der Pause: bis zu ihrem Anfang schlafen. Danach: aufs naechste Present warten.
        let schlaf = if seit < WINDOW_START_NS { WINDOW_START_NS - seit } else { 250_000 };
        std::thread::sleep(Duration::from_nanos(schlaf as u64));
    }
}

enum Message {
    Request(Vec<(usize, Key)>),
    Target(UploadTarget),
}

struct Rendered {
    index: usize,
    mips: Arc<Vec<Vec<u8>>>,
}

pub struct BlockAnimations {
    animations: Arc<Vec<TileAnimation>>,
    // Zuletzt angefordert (nicht: hochgeladen) je Kachel -- angefordert wird nur bei Aenderung.
    requested: Vec<Option<Key>>,
    // Welche Animationen Feuer sind (bekommen die Deckkraft, alle anderen immer FADE_FULL).
    is_fire: Vec<bool>,
    tx: Sender<Message>,
    rx: Receiver<Rendered>,
    // Zeitpunkt des letzten Presents (s. presented).
    last_present: Arc<AtomicI64>,
}

impl BlockAnimations {
    pub fn new(animations: Vec<TileAnimation>) -> Self {
        let animations = Arc::new(animations);
        let (tx, worker_rx) = std::sync::mpsc::channel::<Message>();
        let (worker_tx, rx) = std::sync::mpsc::channel();
        let last_present = Arc::new(AtomicI64::new(0));
        let present_seen = Arc::clone(&last_present);
        let shared = Arc::clone(&animations);
        let feuer: Vec<bool> = animations.iter().map(|a| assets::is_fire_texture(&a.path)).collect();
        // Endet von selbst, sobald dieses Objekt (und damit der Sender) weg ist -- beim
        // Atlas-Neubau entsteht ein neues.
        let _ = std::thread::Builder::new().name("block-anim".into()).spawn(move || {
            let mut cache: HashMap<(usize, Key), Arc<Vec<Vec<u8>>>> = HashMap::new();
            let mut target: Option<UploadTarget> = None;
            while let Ok(first) = worker_rx.recv() {
                // Aufgelaufene Anfragen zusammenziehen: gilt nur noch der neueste Stand je Kachel.
                let mut latest: HashMap<usize, Key> = HashMap::new();
                let mut take = |message: Message, latest: &mut HashMap<usize, Key>| match message {
                    Message::Request(batch) => latest.extend(batch),
                    Message::Target(t) => target = Some(t),
                };
                take(first, &mut latest);
                while let Ok(more) = worker_rx.try_recv() {
                    take(more, &mut latest);
                }
                let mut fertig: Vec<(Arc<Vec<Vec<u8>>>, (u32, u32), u32)> = Vec::new();
                for (index, key) in latest {
                    let Some(animation) = shared.get(index) else { continue };
                    let (state, fade) = key;
                    let mips = match cache.get(&(index, key)) {
                        Some(mips) => Arc::clone(mips),
                        None => {
                            let mut mips = animation.render(state);
                            if feuer.get(index).copied().unwrap_or(false) {
                                binarize_alpha(&mut mips);
                            }
                            if fade < FADE_FULL {
                                mips = apply_fade(&mips, fade);
                            }
                            let mips = Arc::new(mips);
                            // Nur volle Deckkraft: Zwischenstufen gibt es nur waehrend der einen
                            // Sekunde des Blendens.
                            if !state.is_interpolated() && fade == FADE_FULL {
                                cache.insert((index, key), Arc::clone(&mips));
                            }
                            mips
                        }
                    };
                    if target.is_some() {
                        fertig.push((mips, animation.origin, animation.tile_size));
                    } else if worker_tx.send(Rendered { index, mips }).is_err() {
                        return;
                    }
                }
                if let (Some(t), false) = (&target, fertig.is_empty()) {
                    let tiles: Vec<(&[Vec<u8>], (u32, u32), u32)> = fertig.iter().map(|(m, o, s)| (m.as_slice(), *o, *s)).collect();
                    if let Some(befehle) = render::encode_tiles(&t.device, &t.texture, &tiles) {
                        submit_window(&present_seen);
                        t.queue.submit(Some(befehle));
                    }
                }
            }
        });
        let count = animations.len();
        let is_fire = animations.iter().map(|a| assets::is_fire_texture(&a.path)).collect();
        Self { animations, requested: vec![None; count], is_fire, tx, rx, last_present }
    }

    // Neue Staende fuer Spieltick `tick` beim Thread anfordern (nur, was sich geaendert hat).
    // `fire_fade` = Deckkraft der Feuerkacheln in Zwanzigsteln (FADE_FULL = voll).
    pub fn request(&mut self, tick: u64, fire_fade: u8) {
        let mut batch = Vec::new();
        for (index, animation) in self.animations.iter().enumerate() {
            let fade = if self.is_fire[index] { fire_fade.min(FADE_FULL) } else { FADE_FULL };
            let key = (animation.state_at(tick), fade);
            if self.requested[index] != Some(key) {
                self.requested[index] = Some(key);
                batch.push((index, key));
            }
        }
        if !batch.is_empty() {
            let _ = self.tx.send(Message::Request(batch));
        }
    }

    // Ab jetzt laedt der Thread selbst in diesen Atlas hoch (s. Modulkommentar). Alles bereits
    // Angeforderte wird neu angefordert, damit der neue Atlas vollstaendig ist.
    pub fn set_target(&mut self, target: UploadTarget) {
        let _ = self.tx.send(Message::Target(target));
        self.requested.iter_mut().for_each(|r| *r = None);
    }

    // Ein Frame wurde eben praesentiert (Zeit in vblank::monotonic_ns) -- danach ist Pause.
    pub fn presented(&self, at_ns: i64) {
        self.last_present.store(at_ns, Ordering::Relaxed);
    }

    // Alles, was der Thread seit dem letzten Aufruf fertig hat, mit Ursprung und Kachelgroesse.
    pub fn drain(&self) -> Vec<(Arc<Vec<Vec<u8>>>, (u32, u32), u32)> {
        let mut out: HashMap<usize, Rendered> = HashMap::new();
        while let Ok(rendered) = self.rx.try_recv() {
            out.insert(rendered.index, rendered);
        }
        out.into_values()
            .filter_map(|r| {
                let animation = self.animations.get(r.index)?;
                Some((r.mips, animation.origin, animation.tile_size))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deckkraft_skaliert_nur_alpha() {
        let mips = vec![vec![10, 20, 30, 255, 1, 2, 3, 0], vec![5, 6, 7, 128]];
        assert_eq!(apply_fade(&mips, FADE_FULL), mips);
        let halb = apply_fade(&mips, FADE_FULL / 2);
        assert_eq!(halb[0], vec![10, 20, 30, 128, 1, 2, 3, 0]);
        assert_eq!(halb[1], vec![5, 6, 7, 64]);
        assert!(apply_fade(&mips, 0).iter().flatten().skip(3).step_by(4).all(|&a| a == 0));
    }

    // Echte Installation: jede Animation kommt aus dem Thread zurueck, und ein unveraenderter
    // Tick fordert nichts neu an.
    #[test]
    #[ignore]
    fn thread_liefert_alle_kacheln() {
        let mut mc = assets::MinecraftInstallation::locate("1.21.11", None).expect("Installation fehlt");
        let atlas = assets::BlockAtlas::build(&mut mc, &world::block_registry::all_block_names()).expect("Atlas");
        let count = atlas.animations.len();
        let mut anims = BlockAnimations::new(atlas.animations);
        anims.request(0, FADE_FULL);
        let mut got = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while got < count && std::time::Instant::now() < deadline {
            got += anims.drain().len();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(got, count);
        anims.request(0, FADE_FULL);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(anims.drain().is_empty(), "gleicher Tick, nichts Neues");
    }
}
