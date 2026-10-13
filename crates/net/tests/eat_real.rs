// Essen gegen einen echten Server: wann gilt ein Goldapfel beim Server als gegessen, und was
// bricht es ab? Ein Testspieler tickt wie ein Client (alle 50 ms Bewegung + Tick-Ende),
// schickt USE_ITEM und je nach Fall nach n Ticks RELEASE_USE_ITEM -- gemessen wird, ob und
// wann der Server "fertig" meldet (Entity-Ereignis 9) und ob der Apfel weg ist.
//
// Manuell: Testserver auf 127.0.0.1:25565 (offline-mode, Konsole ueber console.in), dann
//   cargo test -p net --test eat_real -- --ignored --nocapture

use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const CONSOLE: &str = concat!(env!("HOME"), "/Dokumente/Minecraft-Server/paper-server/console.in");
const HOTBAR_0: i16 = 36;

struct Bot {
    events: Receiver<net::ClientEvent>,
    out: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    id: Option<i32>,
    verbunden: bool,
    aepfel: i32,
    fertig: bool,
    benutzt: bool,
}

impl Bot {
    fn pump(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                net::ClientEvent::Connected => self.verbunden = true,
                net::ClientEvent::OwnEntityId(id) => self.id = Some(id),
                net::ClientEvent::ContainerSlot(u) if u.sync_id == 0 && u.slot == HOTBAR_0 => {
                    self.aepfel = u.stack.map_or(0, |s| s.count);
                }
                net::ClientEvent::ContainerContent(c) if c.sync_id == 0 => {
                    if let Some(s) = c.slots.get(HOTBAR_0 as usize) {
                        self.aepfel = s.as_ref().map_or(0, |s| s.count);
                    }
                }
                net::ClientEvent::EntityEvent { entity_id, status: 9 } if Some(entity_id) == self.id => self.fertig = true,
                net::ClientEvent::EntityMeta(m) if Some(m.entity_id) == self.id => {
                    if let Some(f) = m.living_flags {
                        self.benutzt = f & protocol::packets::LIVING_FLAG_USING_ITEM != 0;
                    }
                }
                _ => {}
            }
        }
    }

    fn tick(&mut self) {
        self.out.send(protocol::packets::move_player_status_only_payload(true, false)).unwrap();
        self.out.send(protocol::packets::client_tick_end_payload()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        self.pump();
    }
}

fn konsole(befehl: &str) {
    let mut f = std::fs::OpenOptions::new().write(true).open(CONSOLE).expect("console.in");
    writeln!(f, "{befehl}").unwrap();
}

// Ein Bissen: USE_ITEM, nach `loslassen` Ticks RELEASE (None = nie), dann 40 Ticks warten.
// Rueckgabe: (Server meldete fertig, Apfel weg).
fn bissen(bot: &mut Bot, seq: &mut i32, loslassen: Option<u32>) -> (bool, bool) {
    bissen_mit(bot, seq, loslassen, None)
}

// Wie bissen, dazu nach `nochmal` Ticks ein zweites USE_ITEM (Taste gehalten, Wiederholung).
fn bissen_mit(bot: &mut Bot, seq: &mut i32, loslassen: Option<u32>, nochmal: Option<u32>) -> (bool, bool) {
    for _ in 0..10 {
        bot.tick();
    }
    let vorher = bot.aepfel;
    bot.fertig = false;
    *seq += 1;
    bot.out.send(protocol::packets::use_item_payload(protocol::packets::HAND_MAIN, *seq, 0.0, 0.0)).unwrap();
    for t in 1..=60u32 {
        bot.tick();
        if nochmal == Some(t) {
            *seq += 1;
            bot.out.send(protocol::packets::use_item_payload(protocol::packets::HAND_MAIN, *seq, 0.0, 0.0)).unwrap();
        }
        if loslassen == Some(t) {
            bot.out.send(protocol::packets::player_action_payload(protocol::packets::PLAYER_ACTION_RELEASE_USE_ITEM)).unwrap();
        }
    }
    (bot.fertig, bot.aepfel < vorher)
}

#[test]
#[ignore]
fn wann_ist_ein_goldapfel_gegessen() {
    let rt = tokio::runtime::Runtime::new().expect("Runtime");
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 10000;
    let name = format!("EA{t}");
    let (tx, rx) = std::sync::mpsc::channel();
    let (out, out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let n = name.clone();
    rt.spawn(async move {
        let rotation = std::sync::Arc::new(net::SharedLook::default());
        let _ = net::join("127.0.0.1:25565", &n, None, net::Events::new(tx), out_rx, rotation).await;
    });
    let mut bot = Bot { events: rx, out, id: None, verbunden: false, aepfel: 0, fertig: false, benutzt: false };
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) && !(bot.verbunden && bot.id.is_some()) {
        bot.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(bot.verbunden, "nicht verbunden");
    for _ in 0..20 {
        bot.tick();
    }
    konsole(&format!("clear {name}"));
    konsole(&format!("give {name} golden_apple 64"));
    for _ in 0..20 {
        bot.tick();
    }
    assert!(bot.aepfel > 0, "keine Aepfel bekommen");
    let mut seq = 0;
    let mut zeilen = Vec::new();
    // Erster Bissen nach dem Geben scheitert stets (Testaufbau) -- nur zum Einschwingen.
    bissen(&mut bot, &mut seq, None);
    for (loslassen, nochmal) in [(None, None), (Some(31), None), (Some(32), None), (Some(45), Some(31)), (Some(45), Some(32)), (Some(45), Some(33)), (None, Some(32))] {
        let (fertig, weg) = bissen_mit(&mut bot, &mut seq, loslassen, nochmal);
        let z = format!("loslassen {loslassen:?}, zweites USE {nochmal:?}: fertig {fertig}, Apfel weg {weg}, Aepfel {}", bot.aepfel);
        println!("[test] {z}");
        zeilen.push(z);
    }
    let _ = bot.benutzt;
}
