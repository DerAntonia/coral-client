// Paket-Mitschnitt direkt in der Verbindung -- der Ersatz fuer den externen pktproxy
// (crates/pktproxy) im Normalbetrieb.
//
// Warum hier und nicht als Proxy: als CLIENT sitzen wir ohnehin an beiden Enden des
// Bytestroms. Connection::send() bekommt den fertigen Payload VOR der Kompression,
// Connection::recv() gibt ihn NACH der Dekompression zurueck -- beide Male also exakt
// "Packet-ID + Felder", ohne dass irgendwer Framing, Zlib-Zustand oder Verschluesselung
// nachbauen muesste. Genau das war am Proxy die fehleranfaellige Stelle (er musste die
// Kompressions-Umschaltung erraten, s. dessen Modul-Kommentar).
//
// Der pktproxy bleibt trotzdem liegen: er kann etwas, das wir hier grundsaetzlich nicht
// koennen -- den Paketstrom eines ECHTEN Vanilla-Clients mitschneiden, zum Vergleich.
//
// Zwei Betriebsarten, beide ohne Zutun beim Start:
//   1. Ringpuffer (immer an): die letzten RING_LEN Pakete stehen im Speicher. Bei einem
//      Disconnect mit Fehler werden sie nach /tmp geschrieben. Das ist der Fall, fuer den
//      das hier gebaut wurde -- ein Kick wegen "invalid packets" sagt nicht, WELCHES Paket
//      es war, und beim naechsten Versuch ist der Moment schon vorbei.
//   2. Vollmitschnitt (CORAL_PKTLOG=<pfad> gesetzt): zusaetzlich jedes Paket sofort in die
//      Datei. Fuer lange Sitzungen, in denen der Ringpuffer zu kurz waere.
//   3. Rohmitschnitt (CORAL_PKTRAW=<pfad> gesetzt): jedes Paket mit ALLEN Bytes, damit sich
//      hinterher alles auswerten laesst, auch was beim Mitschneiden noch keiner wissen wollte.
//      Format je Paket: 'C' oder 'S', Sitzung (u32 BE), Unix-Zeit in ms (f64 BE, mit Bruchteilen),
//      Laenge (u32 BE), Payload (Packet-ID + Felder, entpackt). Dasselbe schreibt pktproxy.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

use protocol::packets;
use protocol::varint::decode_varint;

// Wie viele Pakete der Ringpuffer haelt. Bei ~20 gesendeten und ~40 empfangenen Paketen pro
// Sekunde sind 400 gut fuenf Sekunden Vorgeschichte -- genug, um vom Kick zurueck zu dem
// Paket zu schauen, das ihn ausgeloest hat, ohne dauerhaft nennenswert Speicher zu halten.
const RING_LEN: usize = 400;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    C2S,
    S2C,
}

impl Dir {
    fn label(self) -> &'static str {
        match self {
            Dir::C2S => "C2S",
            Dir::S2C => "S2C",
        }
    }
}

struct Entry {
    seconds: f64,
    dir: Dir,
    id: Option<i32>,
    len: usize,
    extra: String,
}

impl Entry {
    fn line(&self) -> String {
        let id = match self.id {
            Some(id) => format!("{id:#04x}"),
            None => "??".to_string(),
        };
        let extra = if self.extra.is_empty() { String::new() } else { format!("  {}", self.extra) };
        format!("[{:9.3}] {} id={id} len={}{extra}", self.seconds, self.dir.label(), self.len)
    }
}

pub struct PacketLog {
    start: Instant,
    ring: VecDeque<Entry>,
    // Some = Vollmitschnitt aktiv (CORAL_PKTLOG). Ein Schreibfehler schaltet ihn ab, statt bei
    // jedem Paket erneut zu scheitern -- ein kaputtes Log darf die Verbindung nicht stoeren.
    file: Option<File>,
    // Erst im Play-State sind die Packet-IDs die aus PlayStateFactories; vorher (Handshake/
    // Login/Configuration) wuerde eine Feld-Deutung nur Unsinn erzeugen.
    play: bool,
    // Rohmitschnitt (s. Modulkopf, Betriebsart 3).
    raw: Option<File>,
}

// Ein Paket im Rohformat (s. Modulkopf) -- auch fuer pktproxy.
// Die Zeit ist die Uhrzeit (Unix, ms), damit sich Dateien verschiedener Prozesse und
// Verbindungen direkt nebeneinanderlegen lassen.
pub fn write_raw(out: &mut impl Write, s2c: bool, session: u32, payload: &[u8]) -> std::io::Result<()> {
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    out.write_all(if s2c { b"S" } else { b"C" })?;
    out.write_all(&session.to_be_bytes())?;
    out.write_all(&millis.to_be_bytes())?;
    out.write_all(&(payload.len() as u32).to_be_bytes())?;
    out.write_all(payload)
}

impl PacketLog {
    pub fn new() -> Self {
        let file = std::env::var("CORAL_PKTLOG").ok().filter(|p| !p.is_empty()).and_then(|path| match File::create(&path) {
            Ok(f) => {
                println!("[pktlog] Vollmitschnitt nach {path}");
                Some(f)
            }
            Err(e) => {
                println!("[pktlog] {path} nicht schreibbar ({e}) -- nur Ringpuffer");
                None
            }
        });
        // Anhaengen: mehrere Verbindungen nacheinander (Transfer, pktproxy-Sitzungen) landen
        // alle in derselben Datei, keine ueberschreibt die vorige.
        let raw = std::env::var("CORAL_PKTRAW")
            .ok()
            .filter(|p| !p.is_empty())
            .and_then(|path| std::fs::OpenOptions::new().create(true).append(true).open(&path).ok());
        Self { start: Instant::now(), ring: VecDeque::with_capacity(RING_LEN), file, raw, play: false }
    }

    // Ab hier gelten die Play-State-IDs (s. `play`-Feldkommentar).
    pub fn enter_play(&mut self) {
        self.play = true;
        self.note("--- Play-State erreicht ---");
    }

    // Freitext-Marke in denselben Strom, damit Zustandswechsel im Mitschnitt sichtbar sind.
    pub fn note(&mut self, text: &str) {
        let seconds = self.start.elapsed().as_secs_f64();
        self.push(Entry { seconds, dir: Dir::C2S, id: None, len: 0, extra: text.to_string() });
    }

    // `payload` ist der unkomprimierte Paketinhalt: VarInt-Packet-ID, dann die Felder.
    pub fn record(&mut self, dir: Dir, payload: &[u8]) {
        let seconds = self.start.elapsed().as_secs_f64();
        if let Some(raw) = &mut self.raw {
            if write_raw(raw, dir == Dir::S2C, 0, payload).is_err() {
                self.raw = None;
            }
        }
        let (id, body) = match decode_varint(payload, 0) {
            Ok((id, off)) => (Some(id), &payload[off..]),
            Err(_) => (None, payload),
        };
        let extra = match id {
            Some(id) if self.play => describe(dir, id, body),
            _ => String::new(),
        };
        self.push(Entry { seconds, dir, id, len: payload.len(), extra });
    }

    fn push(&mut self, entry: Entry) {
        if let Some(file) = &mut self.file {
            if writeln!(file, "{}", entry.line()).is_err() {
                self.file = None;
            }
        }
        if self.ring.len() == RING_LEN {
            self.ring.pop_front();
        }
        self.ring.push_back(entry);
    }

    // Schreibt den Ringpuffer nach /tmp und gibt den Pfad zurueck. None, wenn nichts
    // mitgeschnitten wurde oder die Datei nicht angelegt werden konnte -- der Aufrufer haengt
    // den Pfad nur an eine Meldung an, ein Fehlschlag darf ihn nicht weiter stoeren.
    pub fn dump(&self, reason: &str) -> Option<String> {
        if self.ring.is_empty() {
            return None;
        }
        let path = format!("/tmp/coral-client-pktlog-{}.txt", std::process::id());
        let mut file = File::create(&path).ok()?;
        writeln!(file, "# Coral Client Paket-Mitschnitt, letzte {} Pakete vor dem Abbruch", self.ring.len()).ok()?;
        writeln!(file, "# Grund: {reason}").ok()?;
        writeln!(file, "# Zeit in Sekunden seit Verbindungsaufbau. C2S = wir -> Server.").ok()?;
        for entry in &self.ring {
            writeln!(file, "{}", entry.line()).ok()?;
        }
        Some(path)
    }
}

// Big-Endian, wie der Rest des Protokolls.
fn read_f64(body: &[u8], off: usize) -> Option<f64> {
    body.get(off..off + 8).map(|b| f64::from_be_bytes(b.try_into().unwrap()))
}
fn read_f32(body: &[u8], off: usize) -> Option<f32> {
    body.get(off..off + 4).map(|b| f32::from_be_bytes(b.try_into().unwrap()))
}

// Feld-Deutung fuer die Pakete, bei denen der blosse Blick auf ID und Laenge nicht reicht:
// alles, was Position/Blickrichtung/Eingaben traegt. Bewusst KEIN Anspruch auf
// Vollstaendigkeit -- fuer jedes andere Paket sind ID und Laenge die Information, und eine
// halb geratene Deutung waere schlechter als gar keine.
fn describe(dir: Dir, id: i32, body: &[u8]) -> String {
    match (dir, id) {
        (Dir::C2S, packets::SB_PLAY_MOVE_PLAYER_POS) => {
            let (Some(x), Some(y), Some(z)) = (read_f64(body, 0), read_f64(body, 8), read_f64(body, 16)) else {
                return "pos=<zu kurz>".to_string();
            };
            format!("pos=({x:.4},{y:.4},{z:.4}) flags={:#04x}", body.get(24).copied().unwrap_or(0))
        }
        (Dir::C2S, packets::SB_PLAY_MOVE_PLAYER_POS_ROT) => {
            let (Some(x), Some(y), Some(z)) = (read_f64(body, 0), read_f64(body, 8), read_f64(body, 16)) else {
                return "pos=<zu kurz>".to_string();
            };
            let (Some(yaw), Some(pitch)) = (read_f32(body, 24), read_f32(body, 28)) else {
                return format!("pos=({x:.4},{y:.4},{z:.4}) rot=<zu kurz>");
            };
            format!("pos=({x:.4},{y:.4},{z:.4}) yaw={yaw:.2} pitch={pitch:.2} flags={:#04x}", body.get(32).copied().unwrap_or(0))
        }
        (Dir::C2S, packets::SB_PLAY_MOVE_PLAYER_ROT) => {
            let (Some(yaw), Some(pitch)) = (read_f32(body, 0), read_f32(body, 4)) else {
                return "rot=<zu kurz>".to_string();
            };
            format!("yaw={yaw:.2} pitch={pitch:.2} flags={:#04x}", body.get(8).copied().unwrap_or(0))
        }
        (Dir::C2S, packets::SB_PLAY_MOVE_PLAYER_STATUS_ONLY) => {
            format!("flags={:#04x}", body.first().copied().unwrap_or(0))
        }
        (Dir::C2S, packets::SB_PLAY_PLAYER_INPUT) => {
            let f = body.first().copied().unwrap_or(0);
            format!(
                "flags={f:#04x} fwd={} back={} left={} right={} jump={} sneak={} sprint={}",
                f & 1 != 0,
                f & 2 != 0,
                f & 4 != 0,
                f & 8 != 0,
                f & 16 != 0,
                f & 32 != 0,
                f & 64 != 0
            )
        }
        (Dir::C2S, packets::SB_PLAY_SWING) => match decode_varint(body, 0) {
            // Hand: 0 = Haupthand, 1 = Nebenhand.
            Ok((0, _)) => "swing haupthand".to_string(),
            Ok((1, _)) => "swing nebenhand".to_string(),
            _ => "swing".to_string(),
        },
        (Dir::C2S, packets::SB_PLAY_USE_ITEM) => match decode_varint(body, 0) {
            Ok((hand, _)) => format!("use_item hand={hand}"),
            Err(_) => "use_item".to_string(),
        },
        // EntityId (VarInt), Mode (VarInt), mountJumpHeight (VarInt) -- s.
        // packets::player_command_sprint_payload. Der Modus ist die Information: ein
        // START_SPRINTING ohne vorheriges STOP (oder umgekehrt) ist genau das Muster, nach
        // dem serverseitige Pruefungen suchen.
        (Dir::C2S, packets::SB_PLAY_PLAYER_COMMAND) => {
            let Ok((entity, off)) = decode_varint(body, 0) else { return String::new() };
            let Ok((mode, _)) = decode_varint(body, off) else { return format!("player_command entity={entity}") };
            let name = match mode {
                0 => "STOP_SLEEPING",
                1 => "START_SPRINTING",
                2 => "STOP_SPRINTING",
                3 => "START_RIDING_JUMP",
                4 => "STOP_RIDING_JUMP",
                5 => "OPEN_INVENTORY",
                6 => "START_FALL_FLYING",
                _ => "?",
            };
            format!("player_command entity={entity} mode={mode} ({name})")
        }
        // Action (VarInt), BlockPos (i64), Face (u8), Sequence (VarInt).
        (Dir::C2S, packets::SB_PLAY_PLAYER_ACTION) => match decode_varint(body, 0) {
            Ok((action, _)) => {
                let name = match action {
                    0 => "START_DESTROY_BLOCK",
                    1 => "ABORT_DESTROY_BLOCK",
                    2 => "STOP_DESTROY_BLOCK",
                    3 => "DROP_ALL_ITEMS",
                    4 => "DROP_ITEM",
                    5 => "RELEASE_USE_ITEM",
                    6 => "SWAP_ITEM_WITH_OFFHAND",
                    _ => "?",
                };
                format!("player_action={action} ({name})")
            }
            Err(_) => String::new(),
        },
        (Dir::C2S, packets::SB_PLAY_INTERACT) => match decode_varint(body, 0) {
            Ok((entity, _)) => format!("interact entity={entity}"),
            Err(_) => "interact".to_string(),
        },
        (Dir::C2S, packets::SB_PLAY_PONG) => "pong".to_string(),
        (Dir::C2S, packets::SB_PLAY_CLIENT_TICK_END) => "tick_end".to_string(),
        (Dir::C2S, packets::SB_PLAY_CONFIRM_TELEPORT) => match decode_varint(body, 0) {
            Ok((tp, _)) => format!("teleport_id={tp}"),
            Err(_) => String::new(),
        },
        // Nur die Entity-ID (erstes Feld aller dieser Pakete). Die Deltas selbst sind fuer die
        // Frage, wie OFT ein bestimmtes Entity geschickt wird, ohne Belang -- und ohne ID laesst
        // sich die Summe ueber alle Entities nicht auf einzelne herunterrechnen.
        (Dir::S2C, packets::CB_PLAY_MOVE_ENTITY_POS)
        | (Dir::S2C, packets::CB_PLAY_MOVE_ENTITY_POS_ROT)
        | (Dir::S2C, packets::CB_PLAY_MOVE_ENTITY_ROT)
        | (Dir::S2C, packets::CB_PLAY_ENTITY_POSITION_SYNC)
        | (Dir::S2C, packets::CB_PLAY_ROTATE_HEAD) => match decode_varint(body, 0) {
            Ok((entity, _)) => format!("entity={entity}"),
            Err(_) => String::new(),
        },
        (Dir::S2C, packets::CB_PLAY_PLAYER_ROTATION) => {
            let (Some(yaw), Some(pitch)) = (read_f32(body, 0), read_f32(body, 5)) else {
                return "ROTATION <zu kurz>".to_string();
            };
            format!(
                "ROTATION yaw={yaw:.2}{} pitch={pitch:.2}{}",
                if body.get(4) == Some(&1) { " (rel)" } else { "" },
                if body.get(9) == Some(&1) { " (rel)" } else { "" }
            )
        }
        (Dir::S2C, packets::CB_PLAY_PLAYER_POSITION) => {
            // TeleportId (VarInt), dann x,y,z (f64).
            let Ok((tp, off)) = decode_varint(body, 0) else { return String::new() };
            let (Some(x), Some(y), Some(z)) = (read_f64(body, off), read_f64(body, off + 8), read_f64(body, off + 16)) else {
                return format!("teleport_id={tp} pos=<zu kurz>");
            };
            // Danach Geschwindigkeit (3x f64), Yaw, Pitch (f32) und die Relativ-Flags (i32).
            let flags = body.get(off + 56..off + 60).map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]));
            match flags {
                Some(f) => format!("TELEPORT teleport_id={tp} pos=({x:.4},{y:.4},{z:.4}) flags={f:#x}"),
                None => format!("TELEPORT teleport_id={tp} pos=({x:.4},{y:.4},{z:.4})"),
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> PacketLog {
        let mut l = PacketLog::new();
        l.file = None; // Test darf nie in eine echte Datei schreiben, auch nicht mit CORAL_PKTLOG
        l
    }

    #[test]
    fn ringpuffer_haelt_hoechstens_ring_len_eintraege() {
        let mut l = log();
        for _ in 0..RING_LEN + 50 {
            l.record(Dir::C2S, &[0x00]);
        }
        assert_eq!(l.ring.len(), RING_LEN, "der Ringpuffer darf nicht unbegrenzt wachsen");
    }

    #[test]
    fn packet_id_wird_als_varint_vom_payload_getrennt() {
        let mut l = log();
        l.record(Dir::S2C, &[0x3B, 0xAA, 0xBB]);
        let e = l.ring.back().unwrap();
        assert_eq!(e.id, Some(0x3B));
        assert_eq!(e.len, 3, "len ist der GANZE Payload inklusive ID-Byte");
    }

    #[test]
    fn deutung_bleibt_vor_dem_play_state_leer() {
        let mut l = log();
        // Dieselbe ID wie MovePlayerPos, aber im Login-State -- da bedeutet sie etwas
        // voellig anderes und darf nicht als Position gelesen werden.
        let mut payload = vec![packets::SB_PLAY_MOVE_PLAYER_POS as u8];
        payload.extend_from_slice(&[0u8; 25]);
        l.record(Dir::C2S, &payload);
        assert!(l.ring.back().unwrap().extra.is_empty());

        l.enter_play();
        l.record(Dir::C2S, &payload);
        assert!(l.ring.back().unwrap().extra.starts_with("pos="), "im Play-State muss die Position gedeutet werden");
    }

    #[test]
    fn zu_kurzes_bewegungspaket_stuerzt_nicht_ab() {
        let mut l = log();
        l.enter_play();
        // Abgeschnitten nach der ID -- darf keine Panik ausloesen (Slice-Indizierung!).
        l.record(Dir::C2S, &[packets::SB_PLAY_MOVE_PLAYER_POS_ROT as u8, 0x01, 0x02]);
        assert_eq!(l.ring.back().unwrap().extra, "pos=<zu kurz>");
    }
}
