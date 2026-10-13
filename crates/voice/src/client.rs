// Die UDP-Verbindung zum Sprachserver: anmelden, Verbindung pruefen, Keep-Alives beantworten,
// Mikrofon-Rahmen senden, empfangene Rahmen dekodieren.
//
// Ablauf wie beim Mod-Client: jede Sekunde ein Authenticate, bis der Server es bestaetigt,
// danach jede Sekunde ein ConnectionCheck, bis auch der bestaetigt ist -- erst dann gilt die
// Verbindung als stehend. Der Server schickt regelmaessig Keep-Alives; jedes wird beantwortet,
// und bleiben sie zehn Intervalle lang aus, ist die Verbindung tot.
//
// Ein eigener Thread empfaengt und dekodiert; senden darf jeder Thread (der Socket ist geteilt,
// der Encoder steckt hinter einem Mutex). Die App holt die Ereignisse mit poll() ab.

use std::collections::{BTreeMap, HashMap};
use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use opus_rs::{Application, OpusDecoder, OpusEncoder};
use protocol::uuid::Uuid;

use crate::crypto::Cipher;
use crate::packets::{client_datagram, read_server_datagram, SecretInfo, Sound, SoundSource, UdpPacket, FRAME_SIZE, MAX_OPUS_PAYLOAD, SAMPLE_RATE};

#[derive(Debug)]
pub enum VoiceEvent {
    // Authentifiziert und geprueft -- ab jetzt wird gesendet und empfangen.
    Connected,
    // Verbindung verloren (Keep-Alive-Zeitueberschreitung, Socketfehler).
    Disconnected(String),
    // Ein dekodierter Rahmen (960 Samples, 48 kHz, mono) oder -- `pcm` leer -- das Ende einer
    // Wortmeldung.
    Audio { channel: Uuid, sender: Uuid, source: SoundSource, pcm: Vec<i16> },
}

struct Shared {
    socket: UdpSocket,
    cipher: Cipher,
    secret: [u8; 16],
    player: Uuid,
    stop: AtomicBool,
    connected: AtomicBool,
    // Wie viele Pakete auf ein fehlendes gewartet wird, bevor die Luecke verdeckt wird
    // (s. VoiceClient::set_packet_threshold).
    packet_threshold: AtomicUsize,
}

impl Shared {
    fn send(&self, packet: &UdpPacket) -> bool {
        let block = self.cipher.encrypt(&packet.encode());
        self.socket.send(&client_datagram(self.player, &block)).is_ok()
    }
}

pub struct VoiceClient {
    shared: Arc<Shared>,
    encoder: Mutex<OpusEncoder>,
    sequence: AtomicI64,
    // Wurde seit dem letzten Stopp-Paket gesprochen? Das Stopp-Paket geht nur einmal raus.
    talking: AtomicBool,
    events: Mutex<Receiver<VoiceEvent>>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub info: SecretInfo,
}

fn application(codec: u8) -> Application {
    match codec {
        1 => Application::Audio,
        2 => Application::RestrictedLowDelay,
        _ => Application::Voip,
    }
}

impl VoiceClient {
    // `server_host`: der Host, mit dem die Minecraft-Verbindung steht -- der Sprachserver
    // laeuft dort, wenn das Secret keinen eigenen nennt.
    pub fn connect(info: SecretInfo, server_host: &str) -> std::io::Result<VoiceClient> {
        let (host, port) = info.voice_address(server_host);
        let ziel = (host.as_str(), port).to_socket_addrs()?.next().ok_or_else(|| std::io::Error::other(format!("Sprachserver {host}:{port} nicht aufloesbar")))?;
        let lokal = if ziel.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
        let socket = UdpSocket::bind(lokal)?;
        socket.connect(ziel)?;
        socket.set_read_timeout(Some(Duration::from_millis(100)))?;
        let shared = Arc::new(Shared {
            socket,
            cipher: Cipher::new(&info.secret),
            secret: info.secret,
            player: info.player,
            stop: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            packet_threshold: AtomicUsize::new(3),
        });
        let mut encoder = OpusEncoder::new(SAMPLE_RATE as i32, 1, application(info.codec)).map_err(std::io::Error::other)?;
        // Wie die Mod: mit etwas Paketverlust rechnen.
        encoder.packet_loss_perc = 5;
        let (tx, rx) = channel();
        let keep_alive = Duration::from_millis(info.keep_alive.max(100) as u64);
        let s = shared.clone();
        let thread = std::thread::Builder::new().name("voicechat".into()).spawn(move || run(s, tx, keep_alive))?;
        Ok(VoiceClient {
            shared,
            encoder: Mutex::new(encoder),
            sequence: AtomicI64::new(0),
            talking: AtomicBool::new(false),
            events: Mutex::new(rx),
            thread: Some(thread),
            info,
        })
    }

    // Paket-Schwelle der Mod (audio_packet_threshold): kommt ein Paket ausser der Reihe, wird
    // auf das fehlende gewartet, bis so viele spaetere da sind. 0 = nie warten.
    pub fn set_packet_threshold(&self, n: usize) {
        self.shared.packet_threshold.store(n, Ordering::Relaxed);
    }

    pub fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::Relaxed)
    }

    // Einen Mikrofon-Rahmen (960 Samples, 48 kHz, mono) kodieren und senden. Vor dem
    // Verbindungsaufbau geht nichts raus.
    pub fn send_frame(&self, pcm: &[i16], whisper: bool) {
        if !self.is_connected() || pcm.len() != FRAME_SIZE {
            return;
        }
        let mut out = [0u8; MAX_OPUS_PAYLOAD];
        let Ok(len) = self.encoder.lock().map_err(|_| "").and_then(|mut e| e.encode_i16(pcm, FRAME_SIZE, &mut out)) else { return };
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        self.shared.send(&UdpPacket::Mic { data: out[..len].to_vec(), sequence, whisper });
        self.talking.store(true, Ordering::Relaxed);
    }

    // Ende der Wortmeldung: ein leerer Mikrofon-Rahmen, damit die anderen ihren Puffer leeren.
    // Danach faengt der Encoder frisch an (wie MicThread.flush).
    pub fn stop_talking(&self) {
        if !self.talking.swap(false, Ordering::Relaxed) || !self.is_connected() {
            return;
        }
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        self.shared.send(&UdpPacket::Mic { data: Vec::new(), sequence, whisper: false });
        if let Ok(mut e) = self.encoder.lock() {
            if let Ok(neu) = OpusEncoder::new(SAMPLE_RATE as i32, 1, application(self.info.codec)) {
                *e = neu;
                e.packet_loss_perc = 5;
            }
        }
    }

    pub fn poll(&self) -> Vec<VoiceEvent> {
        self.events.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default()
    }
}

impl Drop for VoiceClient {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// Je Kanal (Sprecher) ein eigener Decoder: Opus haelt Zustand zwischen den Rahmen. Dazu die
// Warteschlange fuer Pakete, die ausser der Reihe kamen.
struct Empfang {
    decoder: OpusDecoder,
    last_sequence: i64,
    // Erstes Byte des letzten Rahmens -- fuer die Verlustverdeckung (PLC), die opus-rs aus
    // einem Ein-Byte-Paket macht.
    last_toc: Option<u8>,
    last_seen: Instant,
    sender: Uuid,
    source: SoundSource,
    wartend: BTreeMap<i64, Vec<u8>>,
}

// Wie viele verlorene Rahmen hoechstens verdeckt werden; groessere Luecken sind eine neue
// Wortmeldung, keine Verluste.
const MAX_PLC_FRAMES: i64 = 5;

fn decode(e: &mut Empfang, data: &[u8]) -> Vec<i16> {
    let mut pcm = [0f32; FRAME_SIZE];
    match e.decoder.decode(data, FRAME_SIZE, &mut pcm) {
        Ok(n) => pcm[..n.min(FRAME_SIZE)].iter().map(|x| (x * 32767.0).clamp(-32768.0, 32767.0) as i16).collect(),
        Err(_) => Vec::new(),
    }
}

fn run(s: Arc<Shared>, tx: Sender<VoiceEvent>, keep_alive: Duration) {
    let mut authenticated = false;
    let mut last_try = Instant::now() - Duration::from_secs(10);
    let mut last_keep_alive = Instant::now();
    let mut kanaele: HashMap<[u8; 16], Empfang> = HashMap::new();
    let mut buf = [0u8; 4096];
    while !s.stop.load(Ordering::Relaxed) {
        let connected = s.connected.load(Ordering::Relaxed);
        // Anmeldung und Verbindungspruefung, je einmal pro Sekunde, bis bestaetigt.
        if !connected && last_try.elapsed() >= Duration::from_secs(1) {
            last_try = Instant::now();
            let p = if authenticated { UdpPacket::ConnectionCheck } else { UdpPacket::Authenticate { player: s.player, secret: s.secret } };
            s.send(&p);
        }
        if connected && last_keep_alive.elapsed() > keep_alive * 10 {
            s.connected.store(false, Ordering::Relaxed);
            let _ = tx.send(VoiceEvent::Disconnected("Zeitueberschreitung".into()));
            return;
        }
        let n = match s.socket.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            // Unter Windows meldet ein ICMP "Port unerreichbar" sich als Fehler beim naechsten
            // Empfang -- das ist kein Abbruch, solange noch angemeldet wird.
            Err(_) if !connected => continue,
            Err(e) => {
                s.connected.store(false, Ordering::Relaxed);
                let _ = tx.send(VoiceEvent::Disconnected(e.to_string()));
                return;
            }
        };
        let Ok(block) = read_server_datagram(&buf[..n]) else { continue };
        let Some(plain) = s.cipher.decrypt(&block) else { continue };
        let Ok(packet) = UdpPacket::decode(&plain) else { continue };
        match packet {
            UdpPacket::AuthenticateAck => {
                authenticated = true;
                last_try = Instant::now() - Duration::from_secs(10);
            }
            UdpPacket::ConnectionCheckAck if !connected => {
                s.connected.store(true, Ordering::Relaxed);
                last_keep_alive = Instant::now();
                let _ = tx.send(VoiceEvent::Connected);
            }
            UdpPacket::KeepAlive => {
                last_keep_alive = Instant::now();
                s.send(&UdpPacket::KeepAlive);
            }
            // Ping: unveraendert zurueck (der Server misst damit die Laufzeit).
            p @ UdpPacket::Ping { .. } => {
                s.send(&p);
            }
            UdpPacket::Sound(sound) => empfange(&mut kanaele, sound, &tx, s.packet_threshold.load(Ordering::Relaxed)),
            _ => {}
        }
        // Bleibt der Nachschub aus, waren die fehlenden Pakete verloren: Wartendes ausgeben.
        for (kanal, e) in kanaele.iter_mut() {
            if !e.wartend.is_empty() && e.last_seen.elapsed() > Duration::from_millis(150) {
                abarbeiten(e, Uuid(*kanal), &tx, 0);
            }
        }
        // Verstummte Kanaele nach einer Weile freigeben.
        kanaele.retain(|_, e| e.last_seen.elapsed() < Duration::from_secs(30));
    }
}

fn empfange(kanaele: &mut HashMap<[u8; 16], Empfang>, sound: Sound, tx: &Sender<VoiceEvent>, schwelle: usize) {
    let Sound { channel, sender, data, sequence, source, .. } = sound;
    let e = kanaele.entry(channel.0).or_insert_with(|| Empfang {
        decoder: OpusDecoder::new(SAMPLE_RATE as i32, 1).expect("Opus-Decoder"),
        last_sequence: sequence - 1,
        last_toc: None,
        last_seen: Instant::now(),
        sender,
        source: source.clone(),
        wartend: BTreeMap::new(),
    });
    e.last_seen = Instant::now();
    e.sender = sender;
    e.source = source;
    // Veraltet oder doppelt: verwerfen.
    if sequence <= e.last_sequence {
        return;
    }
    e.wartend.insert(sequence, data);
    abarbeiten(e, channel, tx, schwelle);
}

// Wartende Pakete in Reihenfolge ausgeben: das naechste, sobald es da ist; fehlt es, erst wenn
// mehr als `schwelle` spaetere warten (dann ist es verloren und wird verdeckt).
fn abarbeiten(e: &mut Empfang, channel: Uuid, tx: &Sender<VoiceEvent>, schwelle: usize) {
    loop {
        let Some((&seq, _)) = e.wartend.iter().next() else { return };
        if seq != e.last_sequence + 1 && e.wartend.len() <= schwelle {
            return;
        }
        let data = e.wartend.remove(&seq).unwrap_or_default();
        let luecke = seq - e.last_sequence - 1;
        e.last_sequence = seq;
        // Leer = Ende der Wortmeldung: Decoder zuruecksetzen, Wiedergabe ausklingen lassen.
        if data.is_empty() {
            e.decoder = OpusDecoder::new(SAMPLE_RATE as i32, 1).expect("Opus-Decoder");
            e.last_toc = None;
            let _ = tx.send(VoiceEvent::Audio { channel, sender: e.sender, source: e.source.clone(), pcm: Vec::new() });
            continue;
        }
        if (1..=MAX_PLC_FRAMES).contains(&luecke) {
            if let Some(toc) = e.last_toc {
                for _ in 0..luecke {
                    let pcm = decode(e, &[toc]);
                    let _ = tx.send(VoiceEvent::Audio { channel, sender: e.sender, source: e.source.clone(), pcm });
                }
            }
        }
        e.last_toc = Some(data[0]);
        let pcm = decode(e, &data);
        if !pcm.is_empty() {
            let _ = tx.send(VoiceEvent::Audio { channel, sender: e.sender, source: e.source.clone(), pcm });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::MAGIC;
    use protocol::buffer::{PacketReader, PacketWriter};

    // Ein Sprachserver im Kleinen: bestaetigt Anmeldung und Pruefung, schickt ein Keep-Alive
    // und spiegelt jeden Mikrofon-Rahmen als Ton eines anderen Spielers zurueck.
    fn testserver(secret: [u8; 16]) -> (u16, std::thread::JoinHandle<Vec<&'static str>>) {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let port = sock.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let cipher = Cipher::new(&secret);
            let mut gesehen = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok((n, von)) = sock.recv_from(&mut buf) {
                let mut r = PacketReader::new(&buf[..n]);
                assert_eq!(r.read_u8().unwrap(), MAGIC);
                let _spieler = r.read_uuid().unwrap();
                let len = r.read_varint().unwrap() as usize;
                let plain = cipher.decrypt(r.read_bytes(len).unwrap()).expect("entschluesselbar");
                let antwort = |p: UdpPacket| {
                    let block = cipher.encrypt(&p.encode());
                    let mut w = PacketWriter::new();
                    w.write_u8(MAGIC).write_varint(block.len() as i32).write_raw_bytes(&block);
                    sock.send_to(&w.into_bytes(), von).unwrap();
                };
                match UdpPacket::decode(&plain).unwrap() {
                    UdpPacket::Authenticate { secret: s, .. } => {
                        assert_eq!(s, secret);
                        gesehen.push("auth");
                        antwort(UdpPacket::AuthenticateAck);
                    }
                    UdpPacket::ConnectionCheck => {
                        gesehen.push("check");
                        antwort(UdpPacket::ConnectionCheckAck);
                        antwort(UdpPacket::KeepAlive);
                    }
                    UdpPacket::KeepAlive => gesehen.push("keepalive"),
                    UdpPacket::Mic { data, sequence, .. } => {
                        gesehen.push(if data.is_empty() { "stopp" } else { "mic" });
                        let fertig = data.is_empty();
                        antwort(UdpPacket::Sound(Sound {
                            channel: Uuid([7; 16]),
                            sender: Uuid([7; 16]),
                            data,
                            sequence,
                            source: SoundSource::Player { distance: 48.0, whisper: false },
                            category: None,
                        }));
                        if fertig {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            gesehen
        });
        (port, t)
    }

    // Fuenf Opus-Rahmen, als Pakete mit Sequenz 10..14.
    fn rahmen() -> Vec<Vec<u8>> {
        let mut enc = OpusEncoder::new(48_000, 1, Application::Voip).unwrap();
        (0..5)
            .map(|_| {
                let pcm = [1000i16; FRAME_SIZE];
                let mut out = [0u8; MAX_OPUS_PAYLOAD];
                let n = enc.encode_i16(&pcm, FRAME_SIZE, &mut out).unwrap();
                out[..n].to_vec()
            })
            .collect()
    }

    fn sende(k: &mut HashMap<[u8; 16], Empfang>, seq: i64, data: Vec<u8>, tx: &Sender<VoiceEvent>, schwelle: usize) {
        let sound = Sound { channel: Uuid([1; 16]), sender: Uuid([2; 16]), data, sequence: seq, source: SoundSource::Group, category: None };
        empfange(k, sound, tx, schwelle);
    }

    #[test]
    // 10, 12, 11: mit Schwelle 3 wird auf 11 gewartet -- drei Rahmen in Reihenfolge, keine
    // Verdeckung. Ohne Schwelle wird die Luecke sofort verdeckt und 11 kommt zu spaet.
    fn ordnet_pakete_neu() {
        let r = rahmen();
        let (tx, rx) = channel();
        let mut k = HashMap::new();
        sende(&mut k, 10, r[0].clone(), &tx, 3);
        sende(&mut k, 12, r[2].clone(), &tx, 3);
        assert_eq!(rx.try_iter().count(), 1, "12 wartet auf 11");
        sende(&mut k, 11, r[1].clone(), &tx, 3);
        assert_eq!(rx.try_iter().count(), 2, "11 und 12 kommen jetzt");

        let (tx, rx) = channel();
        let mut k = HashMap::new();
        sende(&mut k, 10, r[0].clone(), &tx, 0);
        sende(&mut k, 12, r[2].clone(), &tx, 0);
        assert_eq!(rx.try_iter().count(), 3, "10, Verdeckung fuer 11, 12");
        sende(&mut k, 11, r[1].clone(), &tx, 0);
        assert_eq!(rx.try_iter().count(), 0, "11 ist veraltet");

        // Mit Schwelle 1: 10, dann 12 und 13 -- bei zwei Wartenden wird 11 aufgegeben.
        let (tx, rx) = channel();
        let mut k = HashMap::new();
        sende(&mut k, 10, r[0].clone(), &tx, 1);
        sende(&mut k, 12, r[2].clone(), &tx, 1);
        sende(&mut k, 13, r[3].clone(), &tx, 1);
        assert_eq!(rx.try_iter().count(), 4, "10, Verdeckung, 12, 13");
    }

    #[test]
    fn verbindet_sendet_und_empfaengt() {
        let secret = [42u8; 16];
        let (port, server) = testserver(secret);
        let info = SecretInfo {
            secret,
            port: port as i32,
            player: Uuid([1; 16]),
            codec: 0,
            mtu: 1024,
            distance: 48.0,
            keep_alive: 1000,
            groups_enabled: true,
            voice_host: String::new(),
            allow_recording: false,
        };
        let client = VoiceClient::connect(info, "127.0.0.1").unwrap();
        let start = Instant::now();
        while !client.is_connected() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(client.is_connected(), "keine Verbindung");
        // Ein Ton (440 Hz) ueber drei Rahmen, dann das Ende.
        for k in 0..3 {
            let rahmen: Vec<i16> = (0..FRAME_SIZE).map(|i| (((k * FRAME_SIZE + i) as f32 * 440.0 / 48000.0 * std::f32::consts::TAU).sin() * 8000.0) as i16).collect();
            client.send_frame(&rahmen, false);
        }
        client.stop_talking();
        let gesehen = server.join().unwrap();
        assert_eq!(&gesehen[..3], ["auth", "check", "keepalive"]);
        assert_eq!(gesehen.iter().filter(|g| **g == "mic").count(), 3);
        assert_eq!(gesehen.last(), Some(&"stopp"));
        std::thread::sleep(Duration::from_millis(300));
        let ereignisse = client.poll();
        assert!(matches!(ereignisse.first(), Some(VoiceEvent::Connected)));
        let rahmen: Vec<&Vec<i16>> = ereignisse.iter().filter_map(|e| match e { VoiceEvent::Audio { pcm, .. } if !pcm.is_empty() => Some(pcm), _ => None }).collect();
        assert_eq!(rahmen.len(), 3);
        assert!(rahmen.iter().all(|r| r.len() == FRAME_SIZE));
        // Der letzte Rahmen hat den Ton (nach der Einschwingzeit des Codecs).
        assert!(rahmen[2].iter().map(|s| s.unsigned_abs()).max().unwrap() > 2000);
        assert!(matches!(ereignisse.last(), Some(VoiceEvent::Audio { pcm, .. }) if pcm.is_empty()));
    }
}
