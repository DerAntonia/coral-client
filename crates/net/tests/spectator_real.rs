// Zuschauer gegen einen echten Server: oeffnet ein Rechtsklick auf eine Kiste ihr Fenster, und
// wie weit darf ein Zuschauer mit EINEM Bewegungspaket springen, ohne zurueckgesetzt zu werden?
// Davon haengt ab, wie der Klick-Teleport (s. App::spectator_teleport) laufen kann.
//
// Manuell: Testserver auf 127.0.0.1:25565 (offline-mode, Konsole ueber console.in), dann
//   cargo test -p net --test spectator_real -- --ignored --nocapture

use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const CONSOLE: &str = concat!(env!("HOME"), "/Dokumente/Minecraft-Server/paper-server/console.in");

struct Bot {
    events: Receiver<net::ClientEvent>,
    out: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    id: Option<i32>,
    verbunden: bool,
    pos: Option<(f64, f64, f64)>,
    zurueckgesetzt: u32,
    fenster: bool,
}

impl Bot {
    fn pump(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                net::ClientEvent::Connected => self.verbunden = true,
                net::ClientEvent::OwnEntityId(id) => self.id = Some(id),
                net::ClientEvent::PlayerPosition(sync) => {
                    // Relative Flags spielen fuer /tp und Rueckstellungen hier keine Rolle.
                    self.pos = Some(sync.pos);
                    self.zurueckgesetzt += 1;
                }
                net::ClientEvent::OpenScreen(_) => self.fenster = true,
                _ => {}
            }
        }
    }

    fn tick_mit(&mut self, pakete: Vec<Vec<u8>>) {
        if pakete.is_empty() {
            self.out.send(protocol::packets::move_player_status_only_payload(false, false)).unwrap();
        }
        for p in pakete {
            self.out.send(p).unwrap();
        }
        self.out.send(protocol::packets::client_tick_end_payload()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        self.pump();
    }

    fn tick(&mut self) {
        self.tick_mit(Vec::new());
    }
}

fn konsole(befehl: &str) {
    let mut f = std::fs::OpenOptions::new().write(true).open(CONSOLE).expect("console.in");
    writeln!(f, "{befehl}").unwrap();
}

#[test]
#[ignore]
fn zuschauer_kiste_und_sprungweite() {
    let rt = tokio::runtime::Runtime::new().expect("Runtime");
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 10000;
    let name = format!("SP{t}");
    let (tx, rx) = std::sync::mpsc::channel();
    let (out, out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let n = name.clone();
    rt.spawn(async move {
        let rotation = std::sync::Arc::new(net::SharedLook::default());
        let _ = net::join("127.0.0.1:25565", &n, None, net::Events::new(tx), out_rx, rotation).await;
    });
    let mut bot = Bot { events: rx, out, id: None, verbunden: false, pos: None, zurueckgesetzt: 0, fenster: false };
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) && !(bot.verbunden && bot.id.is_some()) {
        bot.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(bot.verbunden, "nicht verbunden");
    for _ in 0..20 {
        bot.tick();
    }
    konsole(&format!("gamemode spectator {name}"));
    konsole(&format!("tp {name} 0.5 200 0.5"));
    konsole("setblock 0 200 3 chest");
    for _ in 0..40 {
        bot.tick();
    }
    println!("[test] Position nach /tp: {:?}", bot.pos);

    // Kiste: Rechtsklick auf die Nordseite (Flaeche 2), Mitte.
    bot.tick_mit(vec![protocol::packets::use_item_on_payload(protocol::packets::HAND_MAIN, 0, 200, 3, 2, 0.5, 0.5, 0.0, false, 1)]);
    for _ in 0..20 {
        bot.tick();
    }
    println!("[test] Kiste als Zuschauer geoeffnet: {}", bot.fenster);
    if bot.fenster {
        bot.out.send(protocol::packets::close_container_payload(1)).unwrap();
    }

    // Sprungweiten: je ein Paket mit `d` Bloecken nach +x, danach 10 Ticks Ruhe.
    for d in [4.0, 8.0, 9.9, 10.5, 15.0, 30.0, 100.0] {
        let Some((x, y, z)) = bot.pos else { break };
        let vorher = bot.zurueckgesetzt;
        bot.tick_mit(vec![protocol::packets::move_player_pos_payload(x + d, y, z, false, false)]);
        let ziel = (x + d, y, z);
        for _ in 0..10 {
            bot.tick();
        }
        let zurueck = bot.zurueckgesetzt > vorher;
        if !zurueck {
            bot.pos = Some(ziel);
        }
        println!("[test] Sprung {d}: zurueckgesetzt {zurueck}, jetzt {:?}", bot.pos);
    }
    // Mehrere Pakete in EINEM Tick, je 9 Bloecke weiter.
    for anzahl in [2usize, 5, 10] {
        let Some((x, y, z)) = bot.pos else { break };
        let vorher = bot.zurueckgesetzt;
        let pakete = (1..=anzahl).map(|i| protocol::packets::move_player_pos_payload(x + 9.0 * i as f64, y, z, false, false)).collect();
        bot.tick_mit(pakete);
        for _ in 0..10 {
            bot.tick();
        }
        let zurueck = bot.zurueckgesetzt > vorher;
        if !zurueck {
            bot.pos = Some((x + 9.0 * anzahl as f64, y, z));
        }
        println!("[test] {anzahl} x 9 in einem Tick: zurueckgesetzt {zurueck}, jetzt {:?}", bot.pos);
    }
    // /tp als Chat-Befehl ohne OP.
    let vorher = bot.zurueckgesetzt;
    bot.tick_mit(vec![protocol::packets::chat_command_payload("tp @s 0.5 210 0.5")]);
    for _ in 0..20 {
        bot.tick();
    }
    println!("[test] /tp ohne OP: bewegt {}", bot.zurueckgesetzt > vorher);
}
