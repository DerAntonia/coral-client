// Discord-Status ("Spielt Coral Client", s. ModuleId::DiscordStatus) ueber Discords lokale
// IPC-Schnittstelle -- dieselbe, die jeder Rich-Presence-Client benutzt:
//
//   * ein Unix-Socket discord-ipc-0..9 im Laufzeitverzeichnis (XDG_RUNTIME_DIR, bei Flatpak-
//     und Snap-Discord in deren Unterordnern),
//   * Rahmen = Opcode (u32 LE) + Laenge (u32 LE) + JSON,
//   * Opcode 0 HANDSHAKE {"v":1,"client_id":...}, danach Opcode 1 FRAME mit
//     {"cmd":"SET_ACTIVITY","args":{"pid":...,"activity":{...}},"nonce":...}.
//
// Was hinter "Spielt" steht, ist der NAME der Discord-Anwendung zur client_id -- den legt man
// im Discord-Entwicklerportal fest, nicht hier. Ohne eigene Application-ID (Settings::
// discord_client_id) gilt DEFAULT_CLIENT_ID, die Discord-Anwendung des Projekts.
//
// Alles laeuft in einem eigenen Thread: Discord kann fehlen, spaeter starten oder die
// Verbindung trennen, und nichts davon darf das Spiel aufhalten.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Was der Status zeigen soll. `details` ist die erste Zeile unter dem Spielnamen, `state` die
// zweite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub details: String,
    pub state: String,
    // Beginn in Unix-Sekunden -- Discord zaehlt daraus die verstrichene Zeit hoch.
    pub start: u64,
}

// Zustand der Verbindung, fuer die Anzeige im Modul-Menue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    NoClientId,
    NotFound,
    Connected,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Off => "aus",
            Status::NoClientId => "keine Application-ID",
            Status::NotFound => "Discord nicht gefunden",
            Status::Connected => "verbunden",
        }
    }
}

enum Command {
    // Neue ID und/oder neuer Inhalt; None = Status entfernen (Modul aus).
    Set { client_id: String, activity: Option<Activity> },
}

pub struct DiscordPresence {
    tx: Sender<Command>,
    status: std::sync::Arc<std::sync::Mutex<Status>>,
    last: Option<(String, Option<Activity>)>,
}

impl DiscordPresence {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let status = std::sync::Arc::new(std::sync::Mutex::new(Status::Off));
        let s = status.clone();
        let _ = std::thread::Builder::new().name("discord-rpc".into()).spawn(move || worker(rx, s));
        Self { tx, status, last: None }
    }

    // Jeden Frame aufrufbar: schickt nur, wenn sich etwas geaendert hat.
    pub fn update(&mut self, client_id: &str, activity: Option<Activity>) {
        let neu = (client_id.to_string(), activity);
        if self.last.as_ref() == Some(&neu) {
            return;
        }
        self.last = Some(neu.clone());
        let _ = self.tx.send(Command::Set { client_id: neu.0, activity: neu.1 });
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| *s).unwrap_or(Status::Off)
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// Die Discord-Anwendung "coral-client" (Entwicklerportal, Konto derantonia). Nicht geheim: die
// ID steht in jedem Status, den Discord anzeigt. Fuer Rich Presence braucht die Anwendung
// keine Verifizierung -- die gilt nur fuer Bots auf mehr als 100 Servern.
pub const DEFAULT_CLIENT_ID: &str = "1555687664508997734";

// Eine Discord-Application-ID ist eine Snowflake: nur Ziffern, 17 bis 20 Stellen.
pub fn valid_client_id(id: &str) -> bool {
    (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
// Wie oft ein neuer Verbindungsversuch kommt, solange Discord nicht erreichbar ist.
const RETRY: Duration = Duration::from_secs(15);

fn worker(rx: Receiver<Command>, status: std::sync::Arc<std::sync::Mutex<Status>>) {
    let set_status = |s: Status| {
        if let Ok(mut g) = status.lock() {
            *g = s;
        }
    };
    let mut client_id = String::new();
    let mut activity: Option<Activity> = None;
    let mut conn: Option<Connection> = None;
    let mut dirty = false;
    loop {
        // Auf den naechsten Befehl warten -- ohne Verbindung hoechstens RETRY lang, dann neuer
        // Versuch.
        match rx.recv_timeout(RETRY) {
            Ok(Command::Set { client_id: id, activity: a }) => {
                if id != client_id {
                    conn = None;
                    client_id = id;
                }
                activity = a;
                dirty = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if activity.is_none() {
            // Modul aus: Status wegnehmen und Verbindung schliessen.
            if let Some(c) = conn.as_mut() {
                let _ = c.set_activity(None);
            }
            conn = None;
            set_status(Status::Off);
            dirty = false;
            continue;
        }
        if !valid_client_id(&client_id) {
            conn = None;
            set_status(Status::NoClientId);
            continue;
        }
        if conn.is_none() {
            conn = Connection::open(&client_id);
            dirty = true;
        }
        let Some(c) = conn.as_mut() else {
            set_status(Status::NotFound);
            continue;
        };
        if dirty {
            if c.set_activity(activity.as_ref()).is_err() {
                conn = None;
                set_status(Status::NotFound);
                continue;
            }
            dirty = false;
        }
        set_status(Status::Connected);
    }
}

struct Connection {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    nonce: u64,
}

impl Connection {
    #[cfg(unix)]
    fn open(client_id: &str) -> Option<Self> {
        socket_candidates().iter().find_map(|pfad| Self::open_at(pfad, client_id))
    }

    #[cfg(unix)]
    fn open_at(pfad: &std::path::Path, client_id: &str) -> Option<Self> {
        let stream = std::os::unix::net::UnixStream::connect(pfad).ok()?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let mut c = Connection { stream, nonce: 0 };
        let hallo = format!("{{\"v\":1,\"client_id\":\"{}\"}}", json_escape(client_id));
        c.send(OP_HANDSHAKE, &hallo).ok()?;
        // Antwort ist READY -- oder ein Fehler (unbekannte ID), dann schliesst Discord.
        match c.recv() {
            Ok((OP_FRAME, text)) if text.contains("\"READY\"") => Some(c),
            _ => None,
        }
    }

    #[cfg(not(unix))]
    fn open(_client_id: &str) -> Option<Self> {
        None
    }

    fn set_activity(&mut self, activity: Option<&Activity>) -> std::io::Result<()> {
        self.nonce += 1;
        let payload = set_activity_payload(std::process::id(), activity, self.nonce);
        self.send(OP_FRAME, &payload)?;
        // Die Antwort nur abholen, damit der Puffer nicht volllaeuft; ihr Inhalt ist egal.
        self.recv().map(|_| ())
    }

    #[cfg(unix)]
    fn send(&mut self, op: u32, json: &str) -> std::io::Result<()> {
        use std::io::Write;
        self.stream.write_all(&frame(op, json))
    }

    #[cfg(not(unix))]
    fn send(&mut self, _op: u32, _json: &str) -> std::io::Result<()> {
        Err(std::io::Error::other("kein IPC"))
    }

    #[cfg(unix)]
    fn recv(&mut self) -> std::io::Result<(u32, String)> {
        use std::io::Read;
        let mut kopf = [0u8; 8];
        self.stream.read_exact(&mut kopf)?;
        let op = u32::from_le_bytes([kopf[0], kopf[1], kopf[2], kopf[3]]);
        let len = u32::from_le_bytes([kopf[4], kopf[5], kopf[6], kopf[7]]) as usize;
        if len > 1 << 20 {
            return Err(std::io::Error::other("Rahmen zu groß"));
        }
        let mut body = vec![0u8; len];
        self.stream.read_exact(&mut body)?;
        Ok((op, String::from_utf8_lossy(&body).into_owned()))
    }

    #[cfg(not(unix))]
    fn recv(&mut self) -> std::io::Result<(u32, String)> {
        Err(std::io::Error::other("kein IPC"))
    }
}

// Wo Discord seinen Socket anlegt: das Laufzeitverzeichnis (bzw. TMPDIR, /tmp), dort direkt
// oder im Ordner einer Flatpak-/Snap-Fassung.
#[cfg(unix)]
fn socket_candidates() -> Vec<std::path::PathBuf> {
    let mut basen: Vec<std::path::PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .collect();
    basen.push("/tmp".into());
    let unterordner = ["", "app/com.discordapp.Discord", "app/com.discordapp.DiscordCanary", "app/dev.vencord.Vesktop", "snap.discord", "snap.discord-canary"];
    let mut out = Vec::new();
    for basis in &basen {
        for unter in unterordner {
            for i in 0..10 {
                out.push(basis.join(unter).join(format!("discord-ipc-{i}")));
            }
        }
    }
    out
}

fn frame(op: u32, json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + json.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out
}

fn set_activity_payload(pid: u32, activity: Option<&Activity>, nonce: u64) -> String {
    let inhalt = match activity {
        None => "null".to_string(),
        Some(a) => format!(
            "{{\"details\":\"{}\",\"state\":\"{}\",\"timestamps\":{{\"start\":{}}}}}",
            json_escape(&a.details),
            json_escape(&a.state),
            a.start
        ),
    };
    format!("{{\"cmd\":\"SET_ACTIVITY\",\"args\":{{\"pid\":{pid},\"activity\":{inhalt}}},\"nonce\":\"{nonce}\"}}")
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rahmen_hat_opcode_und_laenge_in_little_endian() {
        let f = frame(OP_FRAME, "{}");
        assert_eq!(&f[..8], &[1, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(&f[8..], b"{}");
    }

    #[test]
    fn aktivitaet_wird_als_json_verschickt() {
        let a = Activity { details: "Beta \"3\"".into(), state: "auf mcpvp.club".into(), start: 1700000000 };
        let p = set_activity_payload(42, Some(&a), 7);
        assert_eq!(
            p,
            "{\"cmd\":\"SET_ACTIVITY\",\"args\":{\"pid\":42,\"activity\":{\"details\":\"Beta \\\"3\\\"\",\"state\":\"auf mcpvp.club\",\"timestamps\":{\"start\":1700000000}}},\"nonce\":\"7\"}"
        );
        assert!(set_activity_payload(1, None, 1).contains("\"activity\":null"));
    }

    #[cfg(unix)]
    #[test]
    // Gegen einen nachgebauten Discord-Socket: Handshake mit der ID, READY, dann die
    // Aktivitaet als FRAME.
    fn handshake_und_aktivitaet_gegen_einen_falschen_discord() {
        use std::io::{Read, Write};
        let dir = std::env::temp_dir().join(format!("coral-client-discord-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let pfad = dir.join("discord-ipc-0");
        let _ = std::fs::remove_file(&pfad);
        let listener = std::os::unix::net::UnixListener::bind(&pfad).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let lies = |s: &mut std::os::unix::net::UnixStream| {
                let mut kopf = [0u8; 8];
                s.read_exact(&mut kopf).unwrap();
                let len = u32::from_le_bytes([kopf[4], kopf[5], kopf[6], kopf[7]]) as usize;
                let mut body = vec![0u8; len];
                s.read_exact(&mut body).unwrap();
                (u32::from_le_bytes([kopf[0], kopf[1], kopf[2], kopf[3]]), String::from_utf8(body).unwrap())
            };
            let hallo = lies(&mut s);
            s.write_all(&frame(OP_FRAME, "{\"cmd\":\"DISPATCH\",\"evt\":\"READY\"}")).unwrap();
            let aktivitaet = lies(&mut s);
            s.write_all(&frame(OP_FRAME, "{}")).unwrap();
            (hallo, aktivitaet)
        });
        let mut c = Connection::open_at(&pfad, "1234567890123456789").expect("READY");
        let a = Activity { details: "Coral Client".into(), state: "im Menü".into(), start: 5 };
        c.set_activity(Some(&a)).unwrap();
        let (hallo, aktivitaet) = server.join().unwrap();
        assert_eq!(hallo, (OP_HANDSHAKE, "{\"v\":1,\"client_id\":\"1234567890123456789\"}".to_string()));
        assert_eq!(aktivitaet.0, OP_FRAME);
        assert!(aktivitaet.1.contains("\"details\":\"Coral Client\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn application_id_ist_eine_snowflake() {
        assert!(valid_client_id("1234567890123456789"));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("12345"));
        assert!(!valid_client_id("12345678901234567a9"));
    }
}
