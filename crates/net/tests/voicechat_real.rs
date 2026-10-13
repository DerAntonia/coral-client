// Sprachchat gegen einen echten Server mit Simple-Voice-Chat-Plugin: zwei Testspieler
// betreten die Welt (beide am Spawn, also in Hoerweite), melden die Kanaele an, holen ihr
// Secret und bauen die UDP-Verbindung auf. Der erste spricht einen Ton, der zweite muss ihn
// als Ton des ersten Spielers hoeren -- danach das Stopp-Paket als Ende der Wortmeldung.
//
// Manuell: Testserver auf 127.0.0.1:25565 (offline-mode, Plugin installiert), dann
//   cargo test -p net --test voicechat_real -- --ignored --nocapture

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

struct Bot {
    events: Receiver<net::ClientEvent>,
    out: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    voice: Option<voice::VoiceClient>,
    verbunden: bool,
}

impl Bot {
    fn start(rt: &tokio::runtime::Runtime, name: String) -> Bot {
        let (tx, rx) = std::sync::mpsc::channel();
        let (out, out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        rt.spawn(async move {
            let rotation = std::sync::Arc::new(net::SharedLook::default());
            let _ = net::join("127.0.0.1:25565", &name, None, net::Events::new(tx), out_rx, rotation).await;
        });
        Bot { events: rx, out, voice: None, verbunden: false }
    }

    // Netz-Ereignisse abarbeiten; Rueckgabe: empfangene Sprach-Ereignisse.
    fn pump(&mut self) -> Vec<voice::VoiceEvent> {
        while let Ok(event) = self.events.try_recv() {
            match event {
                net::ClientEvent::Connected => {
                    self.out.send(protocol::packets::register_channels_payload(&voice::packets::CLIENTBOUND_CHANNELS)).unwrap();
                    self.out.send(protocol::packets::custom_payload(voice::packets::CH_REQUEST_SECRET, &voice::packets::request_secret_payload())).unwrap();
                }
                net::ClientEvent::CustomPayload { channel, data } if channel == voice::packets::CH_SECRET => {
                    let info = voice::packets::read_secret(&data).expect("Secret lesbar");
                    self.voice = Some(voice::VoiceClient::connect(info, "127.0.0.1").expect("UDP"));
                }
                _ => {}
            }
        }
        let ereignisse = self.voice.as_ref().map(|v| v.poll()).unwrap_or_default();
        if ereignisse.iter().any(|e| matches!(e, voice::VoiceEvent::Connected)) {
            self.verbunden = true;
            self.out.send(protocol::packets::custom_payload(voice::packets::CH_UPDATE_STATE, &voice::packets::update_state_payload(false))).unwrap();
        }
        ereignisse
    }
}

#[test]
#[ignore]
fn zwei_spieler_hoeren_sich() {
    let rt = tokio::runtime::Runtime::new().expect("Runtime");
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 10000;
    let mut a = Bot::start(&rt, format!("VA{t}"));
    let mut b = Bot::start(&rt, format!("VB{t}"));
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) && !(a.verbunden && b.verbunden) {
        a.pump();
        b.pump();
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(a.verbunden && b.verbunden, "keine Sprachverbindung (A {} B {})", a.verbunden, b.verbunden);
    println!("[test] beide verbunden");
    // Kurz warten, bis der Server beide als nah genug fuehrt.
    std::thread::sleep(Duration::from_millis(500));
    let sprecher = a.voice.as_ref().unwrap().info.player;

    let mut gehoert = 0usize;
    let mut lautester = 0u16;
    let mut ende = false;
    for k in 0..50 {
        let rahmen: Vec<i16> = (0..960).map(|i| (((k * 960 + i) as f32 * 300.0 / 48000.0 * std::f32::consts::TAU).sin() * 6000.0) as i16).collect();
        a.voice.as_ref().unwrap().send_frame(&rahmen, false);
        std::thread::sleep(Duration::from_millis(20));
        a.pump();
        for e in b.pump() {
            if let voice::VoiceEvent::Audio { sender, pcm, source, .. } = e {
                assert_eq!(sender, sprecher, "Ton vom falschen Absender");
                assert!(matches!(source, voice::SoundSource::Player { .. }), "{source:?}");
                gehoert += 1;
                lautester = lautester.max(pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0));
            }
        }
    }
    a.voice.as_ref().unwrap().stop_talking();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(2) && !ende {
        a.pump();
        for e in b.pump() {
            if let voice::VoiceEvent::Audio { pcm, .. } = e {
                if pcm.is_empty() {
                    ende = true;
                } else {
                    gehoert += 1;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    println!("[test] B hat {gehoert} Rahmen gehoert, Spitze {lautester}, Ende {ende}");
    assert!(gehoert >= 45, "nur {gehoert} von 50 Rahmen angekommen");
    assert!(lautester > 2000, "Ton zu leise ({lautester})");
    assert!(ende, "kein Ende der Wortmeldung");
}
