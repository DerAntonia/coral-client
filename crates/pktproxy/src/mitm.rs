// MITM-Modus: mitlesen, obwohl der Server im Online-Mode ist.
//
// Der Weiterleiter in main.rs kopiert Bytes. Gegen einen Online-Mode-Server ist er damit
// blind: ab der Encryption-Response laeuft der ganze Strom durch AES-128-CFB8, und den
// Schluessel kennt nur der echte Client. Hier laeuft es deshalb anders herum -- der Proxy
// meldet sich SELBST beim Server an (mit denselben Microsoft-Zugangsdaten, die Coral Client
// benutzt, s. net::UpstreamLink) und spricht zum Vanilla-Client wie ein Offline-Mode-Server,
// also voellig unverschluesselt. Dadurch liegen beide Richtungen im Klartext vor.
//
// Was NICHT passiert: es wird nichts veraendert, gefiltert oder verzoegert. Jeder Payload
// geht unveraendert weiter, er wird nur nebenbei protokolliert.
//
// Der Kompressions-Schwellwert des Servers wird unveraendert an den Client durchgereicht.
// Damit sind beide Seiten formatgleich und der Proxy muss nie um- oder auspacken -- ausser
// fuer das Log, und das passiert ohnehin schon eine Ebene tiefer (net::Connection liefert
// fertige Payloads).

use std::io::{self, Read, Write};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use protocol::varint::{decode_varint, encode_varint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;

// Login-State-IDs, die hier von Hand gebaut werden.
const LOGIN_SUCCESS: i32 = 0x02;
const SET_COMPRESSION: i32 = 0x03;

pub async fn run(
    listen_addr: &str,
    upstream: &str,
    konto: Option<&str>,
    log_tx: UnboundedSender<String>,
) -> io::Result<()> {
    let credentials = match konto {
        Some(name) => launcher_credentials(name).await?,
        None => active_account_credentials().await?,
    };
    let listener = TcpListener::bind(listen_addr).await?;
    println!("pktproxy MITM: hoere auf {listen_addr}, meldet sich bei {upstream} an als {}", credentials.username);
    println!("Vanilla-Client jetzt auf {listen_addr} verbinden lassen.");

    let mut sid = 0u64;
    loop {
        let (client, peer) = listener.accept().await?;
        sid += 1;
        let (upstream, log_tx, credentials) = (upstream.to_string(), log_tx.clone(), credentials.clone());
        tokio::spawn(async move {
            let _ = log_tx.send(format!("=== Session {sid}: {peer} ==="));
            if let Err(e) = session(client, &upstream, &credentials, sid, &log_tx).await {
                let _ = log_tx.send(format!("=== Session {sid} beendet: {e} ==="));
            } else {
                let _ = log_tx.send(format!("=== Session {sid} beendet ==="));
            }
        });
    }
}

async fn session(
    client: TcpStream,
    upstream: &str,
    credentials: &auth::Credentials,
    sid: u64,
    log_tx: &UnboundedSender<String>,
) -> io::Result<()> {
    let _ = client.set_nodelay(true);
    let (mut cr, mut cw) = client.into_split();

    // Handshake des Clients. Letztes Feld ist die Absicht: 1 = Serverliste, 2 = Login.
    let handshake = read_payload(&mut cr, None).await?;
    roh(sid, false, &handshake);
    let intent = *handshake.last().unwrap_or(&2);
    if intent == 1 {
        // Serverlisten-Anfrage: im Status-State wird nie verschluesselt, da genuegt das
        // stumpfe Byte-Kopieren -- sonst muesste der Proxy den Ping selbst beantworten.
        let _ = log_tx.send(format!("[S{sid}] Serverlisten-Anfrage, wird roh durchgereicht"));
        return raw_relay(cr, cw, upstream, &handshake).await;
    }
    // Login Start (Name + UUID) -- verworfen, angemeldet wird mit dem gespeicherten Konto.
    let login_start = read_payload(&mut cr, None).await?;
    roh(sid, false, &login_start);
    let _ = log_tx.send(format!("[S{sid}] C2S Login Start ({} Bytes), melde upstream an ...", login_start.len()));

    let mut link = net::UpstreamLink::login(upstream, credentials).await.map_err(io::Error::other)?;
    let _ = log_tx.send(format!("[S{sid}] Upstream-Login ok: {} ({:?} Kompression)", link.username, link.compression));

    // Denselben Kompressions-Schwellwert an den Client weitergeben: ab hier sind beide
    // Seiten formatgleich.
    let comp = link.compression;
    if let Some(t) = comp {
        let mut p = encode_varint(SET_COMPRESSION);
        p.extend_from_slice(&encode_varint(t));
        write_payload(&mut cw, None, &p).await?;
    }
    // Login Success. Properties bleiben leer -- die traegt nur die Skin-Textur, und fuer
    // einen Mitschnitt ist das ohne Belang.
    let mut p = encode_varint(LOGIN_SUCCESS);
    p.extend_from_slice(&link.uuid.to_bytes());
    let name = link.username.as_bytes();
    p.extend_from_slice(&encode_varint(name.len() as i32));
    p.extend_from_slice(name);
    p.extend_from_slice(&encode_varint(0));
    write_payload(&mut cw, comp, &p).await?;

    // Login Acknowledged kommt vom echten Client und geht unveraendert weiter -- so bleibt
    // die Reihenfolge exakt die eines Vanilla-Logins.
    let ack = read_payload(&mut cr, comp).await?;
    roh(sid, false, &ack);
    link.send(&ack).await.map_err(io::Error::other)?;
    let _ = log_tx.send(format!("[S{sid}] Login Acknowledged durchgereicht, ab hier Klartext-Mitschnitt"));

    // Ab jetzt beide Richtungen gleichzeitig. Die Upstream-Seite gehoert komplett dem
    // UpstreamLink (er haelt den AES-Zustand), deshalb laeuft sie in EINER Aufgabe und der
    // Client-Sendekanal wandert mit hinein.
    let mut welt = WeltInfo::default();
    let (to_up_tx, mut to_up_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let log_c = log_tx.clone();
    let client_reader = tokio::spawn(async move {
        // Die ersten Pakete des Clients sind der Handshake (ClientInformation, Client-Brand,
        // KeepAlive). Welche ID dort was bedeutet, haengt am Protokollzustand -- statt den
        // mitzufuehren werden sie einfach roh ausgegeben, dann ist der Inhalt eindeutig.
        // Sonar prueft genau hier: Sichtweite >= 2, Brand gegen ein Muster, und KEIN zweites
        // Brand-Paket (checkState(!receivedClientBrand, "sent duplicate client brand")).
        let mut handshake_uebrig = 14u32;
        while let Ok(p) = read_payload(&mut cr, comp).await {
            log_packet(&log_c, sid, "C2S", &p, None);
            if handshake_uebrig > 0 {
                handshake_uebrig -= 1;
                let _ = log_c.send(roh_zeile(sid, &p));
            }
            if to_up_tx.send(p).is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            von_client = to_up_rx.recv() => match von_client {
                Some(p) => link.send(&p).await.map_err(io::Error::other)?,
                None => break,
            },
            von_server = link.recv() => {
                let p = von_server.map_err(io::Error::other)?;
                log_packet(log_tx, sid, "S2C", &p, Some(&mut welt));
                write_payload(&mut cw, comp, &p).await?;
            }
        }
    }
    client_reader.abort();
    Ok(())
}

// Ein Paket als Hex plus lesbare Zeichen. Damit laesst sich ohne Zustandsverfolgung ablesen,
// was in ClientInformation (Sprache, Sichtweite) und im Client-Brand wirklich steht.
fn roh_zeile(sid: u64, payload: &[u8]) -> String {
    let gekuerzt = &payload[..payload.len().min(96)];
    let hex: String = gekuerzt.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
    let text: String = gekuerzt
        .iter()
        .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
        .collect();
    format!("[S{sid}]     roh: {hex}\n[S{sid}]     txt: {text}")
}

// Rohmitschnitt aller Pakete (Format s. net::write_raw), angehaengt an <log>.bin.
static ROH: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> = std::sync::OnceLock::new();

pub fn open_raw(pfad: &str) -> io::Result<()> {
    let datei = std::fs::OpenOptions::new().create(true).append(true).open(pfad)?;
    println!("Rohmitschnitt: {pfad}");
    let _ = ROH.set(std::sync::Mutex::new(datei));
    Ok(())
}

fn roh(sid: u64, s2c: bool, payload: &[u8]) {
    if let Some(roh) = ROH.get() {
        if let Ok(mut datei) = roh.lock() {
            let _ = net::write_raw(&mut *datei, s2c, sid as u32, payload);
        }
    }
}

fn log_packet(log_tx: &UnboundedSender<String>, sid: u64, dir: &str, payload: &[u8], welt: Option<&mut WeltInfo>) {
    roh(sid, dir == "S2C", payload);
    let Ok((id, off)) = decode_varint(payload, 0) else { return };
    let body = &payload[off..];
    if let Some(welt) = welt {
        welt.beobachte(id, payload, log_tx, sid);
    }
    // CORAL_DUMP_CHUNK=x,z: das Chunk-Paket dieser Spalte roh in eine Datei, um den Parser
    // offline daran zu pruefen.
    if dir == "S2C" && id == protocol::packets::CB_PLAY_CHUNK_DATA {
        if let (Some(ziel), Some(x), Some(z)) = (std::env::var("CORAL_DUMP_CHUNK").ok(), body.get(0..4), body.get(4..8)) {
            let (x, z) = (i32::from_be_bytes(x.try_into().unwrap()), i32::from_be_bytes(z.try_into().unwrap()));
            if ziel == format!("{x},{z}") {
                let pfad = std::env::temp_dir().join(format!("chunk_{x}_{z}_S{sid}.bin"));
                let _ = std::fs::write(&pfad, payload);
                let _ = log_tx.send(format!("[S{sid}] Chunk ({x},{z}) roh nach {}", pfad.display()));
            }
        }
    }
    let name = crate::name_for(dir, true, id);
    let extra = crate::extra_for(dir, true, id, body);
    let _ = log_tx.send(format!("[S{sid}] {dir} id=0x{id:02X}   {name:<28} len={:<5} {extra}", payload.len()));
}

// Was in der Saeule unter dem Spawn wirklich steht.
//
// Ohne das kann man nur raten, warum eine Bewegung anders ausfaellt als bei Vanilla: ob
// Wasser Quelle oder fliessend ist und wie hoch es steht, entscheidet ueber den
// Stroemungsschub (Entity.updateFluidHeightAndDoFluidPushing mittelt ueber alle beruehrten
// Zellen). Deshalb liest der Proxy die Dimensionshoehe aus der Registry mit und gibt beim
// ersten Chunk (0,0) die Saeule x=0,z=0 mit echten Blocknamen aus.
#[derive(Default)]
pub struct WeltInfo {
    dimensionen: Vec<protocol::packets::DimensionInfo>,
    min_y: Option<i32>,
    saeule_gezeigt: bool,
}

impl WeltInfo {
    fn beobachte(&mut self, id: i32, payload: &[u8], log_tx: &UnboundedSender<String>, sid: u64) {
        if let Ok(Some(dims)) = protocol::packets::read_registry_dimension_types(payload) {
            for d in &dims {
                let _ = log_tx.send(format!("[S{sid}] Dimensionsart: {} min_y={} height={}", d.name, d.min_y, d.height));
            }
            self.dimensionen = dims;
            return;
        }
        if id == protocol::packets::CB_PLAY_LOGIN {
            if let Ok(index) = protocol::packets::read_login_dimension_type(payload) {
                match self.dimensionen.get(index.max(0) as usize) {
                    Some(d) => {
                        let _ = log_tx.send(format!("[S{sid}] >>> Welt: {} min_y={} height={}", d.name, d.min_y, d.height));
                        self.min_y = Some(d.min_y);
                    }
                    None => {
                        let _ = log_tx.send(format!("[S{sid}] Dimensionsart {index} nicht in der Registry"));
                    }
                }
            }
            return;
        }
        if id != protocol::packets::CB_PLAY_CHUNK_DATA || self.saeule_gezeigt {
            return;
        }
        let Ok(chunk) = protocol::chunk::parse_chunk_data(payload) else { return };
        if (chunk.x, chunk.z) != (0, 0) {
            return;
        }
        self.saeule_gezeigt = true;
        let min_y = self.min_y.unwrap_or(-64);
        let _ = log_tx.send(format!(
            "[S{sid}] >>> Saeule x=0 z=0 in Chunk (0,0), {} Sections ab y={min_y}:",
            chunk.sections.len()
        ));
        for (i, section) in chunk.sections.iter().enumerate() {
            for ly in 0..16 {
                let state = section.block_states[(ly * 16) * 16];
                if state == protocol::chunk::STATE_ID_AIR {
                    continue;
                }
                let y = min_y + i as i32 * 16 + ly as i32;
                let name = world::block_registry::block_name(state).unwrap_or("?");
                let _ = log_tx.send(format!("[S{sid}]     y={y:<5} state={state:<6} {name}"));
            }
        }
    }
}

// --- Rahmen lesen und schreiben (Client-Seite, immer unverschluesselt) -------------------

async fn read_varint_stream(r: &mut OwnedReadHalf) -> io::Result<i32> {
    let (mut n, mut shift) = (0i32, 0u32);
    for _ in 0..5 {
        let b = r.read_u8().await?;
        n |= ((b & 0x7f) as i32) << shift;
        if b & 0x80 == 0 {
            return Ok(n);
        }
        shift += 7;
    }
    Err(io::Error::other("VarInt zu lang"))
}

async fn read_payload(r: &mut OwnedReadHalf, comp: Option<i32>) -> io::Result<Vec<u8>> {
    let len = read_varint_stream(r).await?;
    if !(0..=(1 << 23)).contains(&len) {
        return Err(io::Error::other("unplausible Rahmenlaenge"));
    }
    let mut frame = vec![0u8; len as usize];
    r.read_exact(&mut frame).await?;
    let Some(_) = comp else { return Ok(frame) };
    let (data_len, off) = decode_varint(&frame, 0).map_err(|_| io::Error::other("data_len kaputt"))?;
    if data_len == 0 {
        return Ok(frame[off..].to_vec());
    }
    let mut out = Vec::with_capacity(data_len as usize);
    ZlibDecoder::new(&frame[off..]).read_to_end(&mut out)?;
    Ok(out)
}

async fn write_payload(w: &mut OwnedWriteHalf, comp: Option<i32>, payload: &[u8]) -> io::Result<()> {
    let frame = match comp {
        None => payload.to_vec(),
        Some(t) if (payload.len() as i32) < t.max(0) => {
            // Unter dem Schwellwert: data_len = 0 und der Rumpf bleibt roh.
            let mut f = encode_varint(0);
            f.extend_from_slice(payload);
            f
        }
        Some(_) => {
            let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
            e.write_all(payload)?;
            let packed = e.finish()?;
            let mut f = encode_varint(payload.len() as i32);
            f.extend_from_slice(&packed);
            f
        }
    };
    let mut out = encode_varint(frame.len() as i32);
    out.extend_from_slice(&frame);
    w.write_all(&out).await?;
    w.flush().await
}

// Serverlisten-Ping: unverschluesselt, deshalb reicht stumpfes Kopieren.
async fn raw_relay(mut cr: OwnedReadHalf, mut cw: OwnedWriteHalf, upstream: &str, handshake: &[u8]) -> io::Result<()> {
    let up = TcpStream::connect(upstream).await?;
    let (mut ur, mut uw) = up.into_split();
    // Hostname im Handshake auf den echten Server umschreiben: manche Server (anticheat-test,
    // Arvor) antworten sonst mit "Please connect using the correct domain".
    let handshake = handshake_mit_host(handshake, upstream).unwrap_or_else(|| handshake.to_vec());
    let mut hs = encode_varint(handshake.len() as i32);
    hs.extend_from_slice(&handshake);
    uw.write_all(&hs).await?;
    let hoch = tokio::spawn(async move { tokio::io::copy(&mut cr, &mut uw).await });
    let _ = tokio::io::copy(&mut ur, &mut cw).await;
    hoch.abort();
    Ok(())
}

// Handshake (id 0, Protokoll, Host, Port, Absicht) mit Host und Port von `upstream`.
fn handshake_mit_host(handshake: &[u8], upstream: &str) -> Option<Vec<u8>> {
    let (id, off) = decode_varint(handshake, 0).ok()?;
    let (protokoll, off) = decode_varint(handshake, off).ok()?;
    let (len, off) = decode_varint(handshake, off).ok()?;
    let nach_host = off + usize::try_from(len).ok()?;
    let absicht = handshake.get(nach_host + 2..)?;
    let (host, port) = upstream.rsplit_once(':').map_or((upstream, 25565), |(h, p)| (h, p.parse().unwrap_or(25565)));
    let mut out = encode_varint(id);
    out.extend_from_slice(&encode_varint(protokoll));
    out.extend_from_slice(&encode_varint(host.len() as i32));
    out.extend_from_slice(host.as_bytes());
    out.extend_from_slice(&u16::to_be_bytes(port));
    out.extend_from_slice(absicht);
    Some(out)
}

// Der Proxy meldet sich als derselbe Spieler an wie Coral Client -- zwei gleichzeitige
// Verbindungen desselben Kontos gehen nicht, nur ein Client darf also durch den Proxy.
// Holt ein im Launcher (Prism/PolyMC/MultiMC) gespeichertes Konto und macht daraus
// Zugangsdaten. Das dort liegende Token kann abgelaufen sein -- credentials_from_token
// fragt damit das Profil ab und scheitert genau dann, was hier als Fehler durchschlaegt.
// Der Name wird ohne Gross-/Kleinschreibung verglichen.
async fn launcher_credentials(name: &str) -> io::Result<auth::Credentials> {
    let konten = auth::launcher_accounts();
    let treffer = konten.iter().find(|k| k.name.eq_ignore_ascii_case(name)).ok_or_else(|| {
        let namen: Vec<&str> = konten.iter().map(|k| k.name.as_str()).collect();
        io::Error::other(if namen.is_empty() {
            format!("Konto {name:?} nicht gefunden -- im Launcher ist ueberhaupt keins gespeichert.")
        } else {
            format!("Konto {name:?} nicht gefunden. Gespeichert sind: {}", namen.join(", "))
        })
    })?;
    auth::credentials_from_token(&treffer.token)
        .await
        .map_err(|e| io::Error::other(format!("Token von {:?} nicht mehr gueltig: {e:?} -- im Launcher neu anmelden.", treffer.name)))
}

// Das im Konto-Manager von Coral Client ausgewaehlte Konto: account_uuid aus
// ~/coral-client/private-config.conf, das Konto selbst (Token, Refresh-Token, Client-ID) aus
// ~/.config/coral-client/accounts.conf. Ist das Minecraft-Token abgelaufen, holt der Proxy mit
// dem Refresh-Token ein frisches -- gespeichert wird es nicht, das macht Coral Client selbst
// beim naechsten Start.
pub async fn active_account_credentials() -> io::Result<auth::Credentials> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).ok_or_else(|| io::Error::other("HOME fehlt"))?;
    let privat = std::fs::read_to_string(home.join("coral-client").join("private-config.conf"))
        .map_err(|e| io::Error::other(format!("~/coral-client/private-config.conf nicht lesbar: {e}")))?;
    let uuid = privat
        .lines()
        .find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == "account_uuid").map(|(_, v)| v.trim().replace('-', "").to_lowercase()))
        .filter(|u| !u.is_empty())
        .ok_or_else(|| io::Error::other("In Coral Client ist kein Konto ausgewaehlt (account_uuid leer)."))?;
    let konten = dirs_config()
        .and_then(|d| std::fs::read_to_string(d.join("coral-client").join("accounts.conf")).ok())
        .ok_or_else(|| io::Error::other("~/.config/coral-client/accounts.conf nicht lesbar"))?;
    // Zeilen: konto<TAB>quelle<TAB>name<TAB>uuid<TAB>token<TAB>refresh<TAB>client-id
    let felder = konten
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|f| f.len() == 7 && f[0] == "konto" && f[3].replace('-', "").to_lowercase() == uuid)
        .ok_or_else(|| io::Error::other(format!("Ausgewaehltes Konto {uuid} steht nicht in accounts.conf")))?;
    let (quelle, name, token, refresh, client_id) = (felder[1], felder[2], felder[4], felder[5], felder[6]);
    if let Ok(c) = auth::credentials_from_token(token).await {
        return Ok(c);
    }
    if quelle != "microsoft" || refresh.is_empty() {
        return Err(io::Error::other(format!("Token von {name} abgelaufen -- in Coral Client neu anmelden.")));
    }
    let msa = auth::refresh_msa(client_id, refresh)
        .await
        .map_err(|e| io::Error::other(format!("Refresh fuer {name} fehlgeschlagen: {e:?} -- in Coral Client neu anmelden.")))?;
    auth::msa_to_minecraft(&msa.access_token).await.map_err(|e| io::Error::other(format!("Minecraft-Anmeldung fuer {name} fehlgeschlagen: {e:?}")))
}

fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
}
