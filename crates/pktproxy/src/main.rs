// Reiner Diagnose-Proxy: sitzt zwischen einem Minecraft-Client und dem echten Server,
// leitet jedes Byte unveraendert weiter (der eigentliche Traffic ist davon nicht betroffen)
// und protokolliert nebenbei jedes Paket (Richtung, Packet-ID, Laenge, Zeitstempel seit
// Verbindungsaufbau) in eine Log-Datei. Zweck: den exakten Paket-Strom eines ECHTEN Vanilla-
// Clients gegen Coral Client vergleichen koennen, ohne einen eigenen Fabric-Mod schreiben zu
// muessen (etwa fuer GroundSpoof/Simulation-Diagnosen).
//
// Aufruf: pktproxy [listen_addr] [upstream_addr] [log_path]
// Default: 127.0.0.1:25566 -> 127.0.0.1:25565, Log nach /tmp/pktproxy.log
// Client dann testweise gegen 127.0.0.1:25566 verbinden lassen statt gegen den echten Port.
//
// Die AEUSSERE Laengen-Praefix-Framing (VarInt) ist UNABHAENGIG von Kompression -- nur der
// INHALT des Frames aendert sich (Set Compression schaltet um). Weiterleiten ist deshalb
// immer korrekt, selbst wenn die Kompressions-Erkennung fuer's Loggen mal daneben liegt.

mod mitm;

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use flate2::read::ZlibDecoder;
use protocol::varint::{decode_varint, encode_varint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

// None = noch keine Kompression, Some(threshold) = aktiv (Wert selbst wird fuers Loggen
// nicht gebraucht, nur ob ueberhaupt gesetzt).
type CompressionState = Arc<Mutex<Option<i32>>>;

// Eine Aufgabe, die alles Protokollierte in die Datei schreibt. Beide Modi haengen daran.
fn spawn_logger(log_path: &str) -> io::Result<tokio::sync::mpsc::UnboundedSender<String>> {
    let log_file = std::fs::OpenOptions::new().create(true).append(true).open(log_path)?;
    let (log_tx, mut log_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut file = log_file;
        // Millisekunden seit Log-Start vor jeder Zeile. LimboFilter prueft nicht nur, WAS der
        // Client schickt, sondern auch WANN: finishCheck() wirft raus, wenn die Fallpruefung
        // vor FALLING_CHECK_TOTAL_TIME fertig ist. Eine Tickschleife, die nach einem langsamen
        // Frame mehrere Ticks nachholt, schickt die Positionen dann im Buendel statt alle
        // 50 ms -- im Paketstrom ist das unsichtbar, in den Zeitstempeln nicht.
        let start = std::time::Instant::now();
        while let Some(line) = log_rx.recv().await {
            let _ = writeln!(file, "[{:>7.1}ms] {line}", start.elapsed().as_secs_f64() * 1000.0);
            let _ = file.flush();
        }
    });
    Ok(log_tx)
}

// Trennt "--account <name>" aus der Argumentliste heraus und liefert den Rest samt Namen.
// Ein alleinstehendes --account ohne Namen zaehlt als nicht angegeben.
fn split_account_flag(args: &[String]) -> (Vec<String>, Option<String>) {
    let mut rest = Vec::with_capacity(args.len());
    let mut konto = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--account" {
            konto = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        rest.push(args[i].clone());
        i += 1;
    }
    (rest, konto)
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let alle: Vec<String> = std::env::args().collect();
    // --account <name> darf ueberall stehen, deshalb vorab herausloesen. Was uebrig bleibt,
    // sind die Positionsargumente.
    let (args, konto) = split_account_flag(&alle);
    // MITM-Modus: pktproxy --mitm <upstream> [listen] [log] [--account <name>]
    //
    // Braucht es, sobald der Server im Online-Mode ist -- dann ist der Byte-Weiterleiter
    // unten blind (s. mitm.rs). Der Proxy meldet sich dort selbst an und spricht zum
    // Client unverschluesselt. Ohne --account nimmt er das im Konto-Manager von Coral Client
    // ausgewaehlte Konto, mit --account das gleichnamige Konto aus Prism/PolyMC/MultiMC.
    // Abruf-Modus: pktproxy --fetch <server> [sekunden]. Verbindet sich selbst (aktives
    // Coral-Konto) ueber net::join, ohne zu bewegen, und trennt nach der Zeit wieder. Mit
    // CORAL_PKTRAW=<datei> landet dabei jedes Paket roh in der Datei -- fuer Daten, an die man
    // sonst nur mit einem echten Client kommt (Limbo-Welten von Botfiltern).
    if args.get(1).map(String::as_str) == Some("--fetch") {
        let server = args.get(2).cloned().ok_or_else(|| io::Error::other("--fetch <server> [sekunden]"))?;
        let sekunden: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5);
        // CORAL_FETCH_OFFLINE=<name>: ohne Konto als dieser Name (nur lokale Server) -- etwa
        // als stehendes Schlagziel auf dem Testserver.
        let offline = std::env::var("CORAL_FETCH_OFFLINE").ok().filter(|n| !n.is_empty());
        let credentials = match &offline {
            Some(_) => None,
            None => Some(mitm::active_account_credentials().await?),
        };
        let name = offline.clone().unwrap_or_default();
        println!("Abruf von {server} als {} fuer {sekunden} s", credentials.as_ref().map_or(name.as_str(), |c| c.username.as_str()));
        let (ev_tx, ev_rx) = std::sync::mpsc::channel();
        let (_out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        let rotation = std::sync::Arc::new(net::SharedLook::default());
        let task = tokio::spawn(async move { net::join(&server, &name, credentials, net::Events::new(ev_tx), out_rx, rotation).await });
        let ende = std::time::Instant::now() + std::time::Duration::from_secs(sekunden);
        while std::time::Instant::now() < ende && !task.is_finished() {
            while let Ok(e) = ev_rx.try_recv() {
                match e {
                    net::ClientEvent::Log(l) => println!("{l}"),
                    net::ClientEvent::Disconnected(g) => println!("getrennt: {g}"),
                    _ => {}
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        task.abort();
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--mitm") {
        let upstream = args.get(2).cloned().unwrap_or_else(|| "127.0.0.1:25565".to_string());
        let listen = args.get(3).cloned().unwrap_or_else(|| "127.0.0.1:25566".to_string());
        let log_path = args.get(4).cloned().unwrap_or_else(|| "/tmp/pktproxy-mitm.log".to_string());
        let log_tx = spawn_logger(&log_path)?;
        // Immer auch roh: jedes Paket mit allen Bytes (s. net::write_raw), damit sich hinterher
        // alles auswerten laesst -- auch was beim Mitschneiden noch keiner wissen wollte.
        mitm::open_raw(&format!("{log_path}.bin"))?;
        println!("Log: {log_path}");
        return mitm::run(&listen, &upstream, konto.as_deref(), log_tx).await;
    }
    let listen_addr = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1:25566".to_string());
    let upstream_addr = args.get(2).cloned().unwrap_or_else(|| "127.0.0.1:25565".to_string());
    let log_path = args.get(3).cloned().unwrap_or_else(|| "/tmp/pktproxy.log".to_string());

    let log_tx = spawn_logger(&log_path)?;

    let listener = TcpListener::bind(&listen_addr).await?;
    println!("pktproxy: hoere auf {listen_addr}, leite weiter an {upstream_addr}, Log: {log_path}");

    let mut session_id: u64 = 0;
    loop {
        let (client_stream, peer) = listener.accept().await?;
        session_id += 1;
        let sid = session_id;
        let upstream_addr = upstream_addr.clone();
        let log_tx = log_tx.clone();
        let _ = log_tx.send(format!("=== Session {sid}: Verbindung von {peer} ==="));

        tokio::spawn(async move {
            let server_stream = match TcpStream::connect(&upstream_addr).await {
                Ok(s) => s,
                Err(e) => {
                    let _ = log_tx.send(format!("=== Session {sid}: Upstream-Verbindung fehlgeschlagen: {e} ==="));
                    return;
                }
            };
            let _ = client_stream.set_nodelay(true);
            let _ = server_stream.set_nodelay(true);

            let (client_read, client_write) = client_stream.into_split();
            let (server_read, server_write) = server_stream.into_split();

            let compression: CompressionState = Arc::new(Mutex::new(None));
            let play_reached = Arc::new(AtomicBool::new(false));
            let start = Instant::now();

            let c2s = tokio::spawn(relay(
                client_read,
                server_write,
                "C2S",
                compression.clone(),
                play_reached.clone(),
                log_tx.clone(),
                start,
                sid,
            ));
            let s2c = tokio::spawn(relay(
                server_read,
                client_write,
                "S2C",
                compression.clone(),
                play_reached.clone(),
                log_tx.clone(),
                start,
                sid,
            ));

            let _ = c2s.await;
            let _ = s2c.await;
            let _ = log_tx.send(format!("=== Session {sid}: beendet ==="));
        });
    }
}

async fn read_varint_from_stream<R: AsyncReadExt + Unpin>(stream: &mut R) -> io::Result<i32> {
    let mut value: i32 = 0;
    for i in 0..5 {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        value |= ((byte[0] & 0x7F) as i32) << (7 * i);
        if byte[0] & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt zu lang"))
}

#[allow(clippy::too_many_arguments)]
async fn relay(
    mut src: OwnedReadHalf,
    mut dst: OwnedWriteHalf,
    direction: &'static str,
    compression: CompressionState,
    play_reached: Arc<AtomicBool>,
    log_tx: UnboundedSender<String>,
    start: Instant,
    sid: u64,
) {
    loop {
        let frame_len = match read_varint_from_stream(&mut src).await {
            Ok(n) if n >= 0 => n as usize,
            _ => break,
        };
        let mut raw = vec![0u8; frame_len];
        if src.read_exact(&mut raw).await.is_err() {
            break;
        }

        // Weiterleiten: IMMER, unabhaengig davon, ob das Loggen unten die Details richtig
        // erkennt -- die aeussere Framing-Laenge ist bereits bekannt und korrekt.
        let mut out = encode_varint(frame_len as i32);
        out.extend_from_slice(&raw);
        if dst.write_all(&out).await.is_err() {
            break;
        }

        // Kompressionsstatus fuer diese Session einmalig auslesen (wird von beiden
        // Richtungen geteilt -- s. CompressionState-Kommentar oben).
        let comp = *compression.lock().await;
        let was_playish = play_reached.load(Ordering::Relaxed);
        if raw.len() > 1000 {
            play_reached.store(true, Ordering::Relaxed);
        }

        let (id, note, body) = describe_frame(&raw, comp);

        // Set Compression (Login-State, S2C, Packet-ID 3, ein einzelnes VarInt als Payload)
        // erkennen und den geteilten Zustand EINMALIG umschalten -- danach nutzen beide
        // Richtungen automatisch das komprimierte Format.
        if comp.is_none() && direction == "S2C" && id == Some(3) && raw.len() >= 2 && raw.len() <= 6 {
            if let Ok((threshold, _)) = decode_varint(&raw, 1) {
                *compression.lock().await = Some(threshold);
                let _ = log_tx.send(format!(
                    "{:>8}ms [S{sid}] S2C *** Set Compression erkannt, threshold={threshold} ***",
                    start.elapsed().as_millis()
                ));
            }
        }

        let name = id.map(|i| name_for(direction, was_playish, i)).unwrap_or_else(|| "?".to_string());
        let id_str = id.map(|i| format!("0x{i:02X}")).unwrap_or_else(|| "??".to_string());
        let extra = id.map(|i| extra_for(direction, was_playish, i, &body)).unwrap_or_default();
        let _ = log_tx.send(format!(
            "{:>8}ms [S{sid}] {direction} id={id_str:<6} {name:<28} raw_len={:<5} {note} {extra}",
            start.elapsed().as_millis(),
            raw.len()
        ));
    }
}

// Best-effort Interpretation eines schon vollstaendig gelesenen Frames (nur fuers Loggen,
// niemals fuer die Weiterleitung -- die ist oben bereits passiert). Gibt (Packet-ID, Notiz,
// Restkoerper NACH der Packet-ID) zurueck, None als ID bei jedem Parse-Fehler (z.B. wenn die
// Kompressions-Erkennung noch nicht zugeschlagen hat, waehrend der Server schon komprimiert).
fn describe_frame(raw: &[u8], compression: Option<i32>) -> (Option<i32>, String, Vec<u8>) {
    match compression {
        None => match decode_varint(raw, 0) {
            Ok((id, off)) => (Some(id), String::new(), raw[off..].to_vec()),
            Err(_) => (None, "parse-error (id)".to_string(), Vec::new()),
        },
        Some(_) => {
            let (data_len, offset) = match decode_varint(raw, 0) {
                Ok(v) => v,
                Err(_) => return (None, "parse-error (data_len)".to_string(), Vec::new()),
            };
            if data_len == 0 {
                match decode_varint(raw, offset) {
                    Ok((id, off2)) => (Some(id), "unkomprimiert".to_string(), raw[off2..].to_vec()),
                    Err(_) => (None, "parse-error (id, unkomprimiert)".to_string(), Vec::new()),
                }
            } else {
                let mut decoder = ZlibDecoder::new(&raw[offset..]);
                let mut out = Vec::with_capacity(data_len as usize);
                match decoder.read_to_end(&mut out) {
                    Ok(_) => match decode_varint(&out, 0) {
                        Ok((id, off2)) => (Some(id), format!("komprimiert, entpackt={}", out.len()), out[off2..].to_vec()),
                        Err(_) => (None, "parse-error (id, entpackt)".to_string(), Vec::new()),
                    },
                    Err(_) => (None, "zlib-Fehler".to_string(), Vec::new()),
                }
            }
        }
    }
}

// Big-Endian-Lesehilfen fuers Body (Netzwerk-Byte-Reihenfolge, wie der Rest des Protokolls).
fn read_f64(body: &[u8], off: usize) -> Option<f64> {
    body.get(off..off + 8).map(|b| f64::from_be_bytes(b.try_into().unwrap()))
}
fn read_f32(body: &[u8], off: usize) -> Option<f32> {
    body.get(off..off + 4).map(|b| f32::from_be_bytes(b.try_into().unwrap()))
}
fn read_i32(body: &[u8], off: usize) -> Option<i32> {
    body.get(off..off + 4).map(|b| i32::from_be_bytes(b.try_into().unwrap()))
}

// Deckt genau die Felder ab, die fuer den Held-Jump/BadPacketsN-Vergleich interessant sind:
// Position + Flags bei den Move-Player-Varianten, die rohen Input-Flags
// bei Player Input. Alles andere bleibt leer -- kein Anspruch auf Vollstaendigkeit.
pub(crate) fn extra_for(direction: &str, play_reached: bool, id: i32, body: &[u8]) -> String {
    if !play_reached {
        return String::new();
    }
    if direction == "C2S" {
        match id {
            0x1D => {
                // MovePlayerPos: x,y,z (f64), flags (u8)
                if let (Some(x), Some(y), Some(z)) = (read_f64(body, 0), read_f64(body, 8), read_f64(body, 16)) {
                    let flags = body.get(24).copied().unwrap_or(0);
                    // VOLLE Praezision, nicht gerundet: Sonar vergleicht die Landehoehe exakt
                    // (`if (collisionOffsetY != 0) fail(...)`) und den Spawn exakt
                    // (`checkState(x == SPAWN_X_POSITION)`). Ein 91.00000000000001 saehe
                    // gerundet aus wie 91.0000, waere fuer Sonar aber ein Fehlschlag.
                    return format!("pos=({x},{y},{z}) flags={flags:#04x}");
                }
            }
            0x1E => {
                // MovePlayerPosRot: x,y,z (f64), yaw,pitch (f32), flags (u8)
                if let (Some(x), Some(y), Some(z)) = (read_f64(body, 0), read_f64(body, 8), read_f64(body, 16)) {
                    let (yaw, pitch) = (read_f32(body, 24), read_f32(body, 28));
                    let flags = body.get(32).copied().unwrap_or(0);
                    return format!(
                        "pos=({x},{y},{z}) yaw={} pitch={} flags={flags:#04x}",
                        yaw.map(|v| format!("{v:.2}")).unwrap_or_else(|| "?".into()),
                        pitch.map(|v| format!("{v:.2}")).unwrap_or_else(|| "?".into())
                    );
                }
            }
            0x1F => {
                // MovePlayerRot: yaw,pitch (f32), flags (u8) -- keine Position.
                if let (Some(yaw), Some(pitch)) = (read_f32(body, 0), read_f32(body, 4)) {
                    let flags = body.get(8).copied().unwrap_or(0);
                    return format!("yaw={yaw:.2} pitch={pitch:.2} flags={flags:#04x}");
                }
            }
            0x20 => {
                // MovePlayerStatusOnly: nur Flags.
                let flags = body.first().copied().unwrap_or(0);
                return format!("flags={flags:#04x}");
            }
            0x2A => {
                // PlayerInput: 1 Byte Bitflags (forward/backward/left/right/jump/sneak/sprint).
                let flags = body.first().copied().unwrap_or(0);
                return format!(
                    "flags={flags:#04x} fwd={} back={} left={} right={} jump={} sneak={} sprint={}",
                    flags & 1 != 0,
                    flags & 2 != 0,
                    flags & 4 != 0,
                    flags & 8 != 0,
                    flags & 16 != 0,
                    flags & 32 != 0,
                    flags & 64 != 0
                );
            }
            0x29 => {
                // PlayerCommand: EntityId (VarInt), Aktion (VarInt), Daten (VarInt).
                if let Ok((entity, off)) = decode_varint(body, 0) {
                    if let Ok((aktion, off2)) = decode_varint(body, off) {
                        let name = match aktion {
                            0 => "SNEAK_START",
                            1 => "SNEAK_STOP",
                            2 => "BED_LEAVE",
                            3 => "SPRINT_START",
                            4 => "SPRINT_STOP",
                            5 => "HORSE_JUMP_START",
                            6 => "HORSE_JUMP_STOP",
                            7 => "INVENTORY_OPEN",
                            8 => "ELYTRA_START",
                            _ => "?",
                        };
                        let daten = decode_varint(body, off2).map(|(d, _)| d).unwrap_or(-1);
                        return format!("entity={entity} aktion={aktion} ({name}) daten={daten}");
                    }
                }
            }
            _ => {}
        }
    } else if direction == "S2C" && id == 0x18 {
        // Plugin-Nachricht vom Server. Der Kanal minecraft:brand verraet, WELCHE Software
        // gerade mit uns spricht -- bei einem Botcheck also den Limbo/Antibot statt des
        // eigentlichen Servers. Ohne das raet man, gegen wessen Pruefcode man debuggt.
        let mut r = protocol::buffer::PacketReader::new(body);
        if let Ok(kanal) = r.read_string() {
            let rest: String = r
                .remaining()
                .iter()
                .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                .collect();
            return format!("kanal={kanal} daten={rest}");
        }
    } else if direction == "S2C" && id == 0x20 {
        // Disconnect im Play-State: der Grund ist eine NBT-Textkomponente. Statt NBT zu
        // parsen werden hier die lesbaren Zeichenketten herausgezogen -- fuer den Kickgrund
        // reicht das und es kann an keinem Format scheitern.
        let mut teile = Vec::new();
        let mut aktuell = String::new();
        for &b in body {
            if (0x20..0x7f).contains(&b) {
                aktuell.push(b as char);
            } else {
                if aktuell.len() >= 4 {
                    teile.push(std::mem::take(&mut aktuell));
                } else {
                    aktuell.clear();
                }
            }
        }
        if aktuell.len() >= 4 {
            teile.push(aktuell);
        }
        return format!("Grund: {}", teile.join(" | "));
    } else if direction == "S2C" && id == 0x46 {
        // Synchronize Player Position (S2C): TeleportId (VarInt), x,y,z (f64), velX/Y/Z
        // (f64), yaw,pitch (f32), Flags (i32) -- s. protocol::packets::read_player_position
        // fuers exakte Feldlayout. Wichtig fuers Debuggen von Teleport-Loops:
        // zeigt, WOHIN der Server tatsaechlich korrigiert.
        if let Ok((teleport_id, off)) = decode_varint(body, 0) {
            if let (Some(x), Some(y), Some(z)) = (read_f64(body, off), read_f64(body, off + 8), read_f64(body, off + 16)) {
                let (yaw, pitch) = (read_f32(body, off + 48), read_f32(body, off + 52));
                if let Some(flags) = read_i32(body, off + 56) {
                    return format!(
                        "id={teleport_id} pos=({x:.4},{y:.4},{z:.4}) yaw={} pitch={} flags={flags:#x}",
                        yaw.map(|v| format!("{v:.2}")).unwrap_or_else(|| "?".into()),
                        pitch.map(|v| format!("{v:.2}")).unwrap_or_else(|| "?".into())
                    );
                }
            }
        }
    } else if direction == "S2C" && id == protocol::packets::CB_PLAY_CHUNK_DATA {
        // Die Spalte (x, z als i32 am Anfang), um fehlende Chunks zwischen zwei Clients
        // vergleichen zu koennen.
        if let (Some(x), Some(z)) = (read_i32(body, 0), read_i32(body, 4)) {
            return format!("chunk=({x},{z})");
        }
    } else if direction == "S2C" && id == protocol::packets::CB_PLAY_LIGHT_UPDATE {
        // Licht-Update: x und z als VarInt.
        if let Ok((x, off)) = decode_varint(body, 0) {
            if let Ok((z, _)) = decode_varint(body, off) {
                return format!("chunk=({x},{z})");
            }
        }
    } else if direction == "S2C" && id == protocol::packets::CB_PLAY_FORGET_LEVEL_CHUNK {
        if let Some(packed) = body.get(0..8).map(|b| i64::from_be_bytes(b.try_into().unwrap())) {
            return format!("chunk=({},{})", packed as i32, (packed >> 32) as i32);
        }
    }
    String::new()
}

// Reine Lesbarkeits-Hilfe fuers Log -- deckt nur die Pakete ab, die fuer die GroundSpoof/
// Simulation-Diagnose interessant sind (s. protocol::packets fuer die vollstaendige Liste).
// "play_reached" ist eine grobe Heuristik (erstes S2C-Paket >1000 Bytes, typischerweise
// Chunk-Daten) -- vor Play-State bleiben IDs unbenannt, weil dieselbe Nummer dort etwas
// anderes bedeutet.
pub(crate) fn name_for(direction: &str, play_reached: bool, id: i32) -> String {
    if !play_reached {
        return String::new();
    }
    let name = if direction == "C2S" {
        match id {
            0x00 => "ConfirmTeleport",
            0x08 => "Chat",
            0x0B => "ClientCommand",
            0x0C => "ClientTickEnd",
            0x19 => "Interact",
            0x1B => "KeepAlive",
            0x1D => "MovePlayerPos",
            0x1E => "MovePlayerPosRot",
            0x1F => "MovePlayerRot",
            0x20 => "MovePlayerStatusOnly",
            0x29 => "PlayerCommand",
            0x2A => "PlayerInput",
            0x3C => "Swing",
            _ => "",
        }
    } else {
        match id {
            0x01 => "AddEntity",
            0x08 => "BlockUpdate",
            0x20 => "Disconnect",
            0x23 => "EntityPositionSync",
            0x25 => "ForgetLevelChunk",
            0x2B => "KeepAlive",
            0x2C => "ChunkData",
            0x30 => "Login",
            0x33 => "MoveEntityPos",
            0x34 => "MoveEntityPosRot",
            0x36 => "MoveEntityRot",
            0x46 => "PlayerPosition",
            0x4B => "RemoveEntities",
            0x51 => "RotateHead",
            0x52 => "SectionBlocksUpdate",
            0x66 => "SetHealth",
            0x77 => "SystemChat",
            0x7B => "TeleportEntity",
            _ => "",
        }
    };
    name.to_string()
}
