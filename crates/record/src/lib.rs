// coral-record (s. PLAN-coral-record.md): statt eines Videos wird aufgezeichnet, woraus das Bild
// entsteht -- Paketstrom, Kamera, Maus und der lokale Zustand, den der Server nicht
// zurueckschickt. Daraus soll sich spaeter ein Video in beliebiger Aufloesung rendern lassen.
//
// Aufbau:
// - Jeder Erzeuger (Netz-Task einer Verbindung, Hauptthread) hat einen eigenen Producer, also
//   einen eigenen Puffer. Kein Mutex mit dem Render-Thread: ein voller Puffer geht per try_send
//   an den Schreib-Thread, ist der Kanal voll, wird verworfen und gezaehlt (s. dropped).
// - Der Schreib-Thread (niedrige Prioritaet) sortiert die Eintraege je Sitzung nach Zeit, packt
//   sie in Bloecke von etwa einer Sekunde, komprimiert sie (zlib) und schreibt sie. Platte und
//   Kompression passieren nur dort.
// - Eine Datei je Sitzung (Join bis Disconnect oder Server-Wechsel), s. Session.
//
// Reihenfolge: innerhalb einer Spur bleibt sie immer erhalten (ein Erzeuger, stabile Sortierung).
// Zwischen den Spuren sortiert der Schreib-Thread nach Zeit; ein Eintrag, der laenger als
// LATE_MS im Puffer seines Erzeugers lag, kann dabei in einem spaeteren Block landen.
//
// Dateiformat (alles Little Endian):
//   Kopf:   "CREC", u16 Version, u32 Laenge, dann Header::encode.
//   Block:  'B', f64 Start, f64 Ende, u32 Anzahl, u32 Rohlaenge, u32 komprimierte Laenge, zlib.
//           Roh je Eintrag: u8 Spur, f64 Zeit, VarInt Laenge, Daten.
//   Index:  'I', u32 Anzahl, je Block (f64 Start, u64 Dateiposition), u64 Position des Index,
//           "CEND". Fehlt er (Absturz), findet der Leser die Bloecke ueber ihre Koepfe.
// Zeiten in ms relativ zum Sitzungsstart, die Uhrzeit des Starts steht im Kopf.

mod read;
mod replay;
mod scrub;

pub use read::{BlockInfo, Entry, ReadError, Reader};
pub use replay::{load_tracks, summary, Next, PacketSource, ReplayClock, Summary, Tracks};
pub use scrub::{scrub_c2s_play, Scrubbed};

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const MAGIC: &[u8; 4] = b"CREC";
pub const VERSION: u16 = 1;
const BLOCK: u8 = b'B';
const INDEX: u8 = b'I';
const END: &[u8; 4] = b"CEND";
// Kopf eines Blocks: Kennung, Start, Ende, Anzahl, Rohlaenge, komprimierte Laenge.
const BLOCK_HEAD: usize = 1 + 8 + 8 + 4 + 4 + 4;

// So lange darf ein Eintrag beim Erzeuger liegen, bevor er rausgeht.
const FLUSH_EVERY: Duration = Duration::from_millis(200);
// Ein so voller Puffer geht sofort raus.
const FLUSH_BYTES: usize = 256 * 1024;
// Erst was so alt ist, wird geschrieben -- Puffer anderer Erzeuger sind dann laengst da.
const LATE_MS: f64 = 1000.0;
// Blocklaenge (Zeitspanne) und wie viele Puffer der Kanal haelt.
const BLOCK_MS: f64 = 1000.0;
const CHANNEL_LEN: usize = 512;
// So lange wartet eine Aufnahme ab der Mitte auf ihren Snapshot, bevor sie ohne ihn schreibt.
const SNAPSHOT_WAIT: Duration = Duration::from_mins(1);
// Rohgroesse eines Snapshot-Blocks.
const SNAPSHOT_BLOCK: usize = 4 << 20;

// Die Spuren (s. Plan, Abschnitt Spuren).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tag {
    // Paket vom Server (Packet-ID + Felder, entpackt).
    S,
    // Paket an den Server (ohne Schluessel und Signaturen, s. scrub).
    C,
    // Kamera: Position, Yaw, Pitch, FOV, Perspektive.
    K,
    // Mausbewegung, roh, mit Zeit des Kernels.
    M,
    // Lokaler Zustand, nur bei Aenderung.
    L,
    // Markierung (s. Mark).
    E,
    // Snapshot: der Zustand beim Einschalten mitten im Spiel, je Eintrag ein dekodiertes
    // Ereignis (s. net::snapshot). Steht vor allem anderen; ein leerer Eintrag schliesst ihn ab.
    Z,
}

impl Tag {
    pub const ALL: [Self; 7] = [Self::S, Self::C, Self::K, Self::M, Self::L, Self::E, Self::Z];

    #[must_use] 
    pub const fn byte(self) -> u8 {
        match self {
            Self::S => b'S',
            Self::C => b'C',
            Self::K => b'K',
            Self::M => b'M',
            Self::L => b'L',
            Self::E => b'E',
            Self::Z => b'Z',
        }
    }

    #[must_use] 
    pub fn from_byte(b: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.byte() == b)
    }
}

// Erstes Byte eines E-Eintrags, danach Text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    // Der Tab dieser Sitzung ist jetzt vorn bzw. hinten.
    Front = 1,
    Back = 2,
    // Die aktiven Client-Packs, je Zeile eins.
    Packs = 3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub protocol: i32,
    // Client-Version (s. set_client_version), vom Schreiber eingesetzt.
    pub client: String,
    pub server: String,
    // Uhrzeit beim Start dieser Datei, Unix in ms (setzt Session::open).
    pub wall_ms: f64,
    pub uuid: [u8; 16],
    pub name: String,
    // Lief schon beim Join, enthaelt also den Anfangszustand. Sonst (mitten im Spiel
    // eingeschaltet) steht er als Snapshot am Anfang (Spur Z).
    pub from_join: bool,
}

impl Header {
    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&self.protocol.to_le_bytes());
        put_str(&mut b, &self.client);
        put_str(&mut b, &self.server);
        b.extend_from_slice(&self.wall_ms.to_le_bytes());
        b.extend_from_slice(&self.uuid);
        put_str(&mut b, &self.name);
        b.push(u8::from(self.from_join));
        b
    }
}

fn put_str(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(&(s.len() as u32).to_le_bytes());
    b.extend_from_slice(s.as_bytes());
}

fn put_varint(b: &mut Vec<u8>, mut v: u32) {
    loop {
        if v < 0x80 {
            b.push(v as u8);
            return;
        }
        b.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static CLIENT: Mutex<String> = Mutex::new(String::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static DROPPED: AtomicU64 = AtomicU64::new(0);
// Geleerte Puffer zur Wiederverwendung. Nur per try_lock: wer ihn nicht sofort bekommt,
// legt einen neuen Puffer an, statt zu warten.
static POOL: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static TX: OnceLock<SyncSender<Msg>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();

// Modul an/aus. Laufende Verbindungen folgen beim naechsten Paket (s. PacketRecorder).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Relaxed)
}

// Ordner der Aufnahmen.
pub fn set_dir(dir: PathBuf) {
    if let Ok(mut d) = DIR.lock() {
        *d = Some(dir);
    }
}

pub fn set_client_version(v: String) {
    if let Ok(mut c) = CLIENT.lock() {
        *c = v;
    }
}

// Verworfene Puffer, weil der Schreib-Thread nicht nachkam.
pub fn dropped() -> u64 {
    DROPPED.load(Relaxed)
}

// Gemeinsamer Nullpunkt aller Erzeuger.
pub fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

fn ms(at: Instant) -> f64 {
    at.saturating_duration_since(epoch()).as_secs_f64() * 1000.0
}

enum Msg {
    Open { id: u64, path: PathBuf, header: Header, start: f64 },
    Chunk(Vec<u8>),
    Close(u64),
    Finish(mpsc::Sender<()>),
}

fn sender() -> &'static SyncSender<Msg> {
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(CHANNEL_LEN);
        // Scheitert der Start, faellt rx weg und jedes Senden zaehlt als verworfen.
        let _ = std::thread::Builder::new().name("coral-record".into()).spawn(move || writer_loop(rx));
        tx
    })
}

fn take_buffer() -> Vec<u8> {
    POOL.try_lock().ok().and_then(|mut p| p.pop()).unwrap_or_else(|| Vec::with_capacity(64 * 1024))
}

fn recycle(mut b: Vec<u8>) {
    b.clear();
    if let Ok(mut p) = POOL.try_lock() {
        if p.len() < 32 {
            p.push(b);
        }
    }
}

// Puffer eines Erzeugers. Eintrag im Puffer: u64 Sitzung, u8 Spur, f64 Zeit (ms seit epoch),
// u32 Laenge, Daten.
pub struct Producer {
    buf: Vec<u8>,
    // Wann der aelteste Eintrag im Puffer dazukam.
    first: Option<Instant>,
    // Wartet bei vollem Kanal, statt zu verwerfen. Nur fuer den Snapshot, der in einem eigenen
    // Thread kodiert wird und nicht luecken darf.
    blocking: bool,
}

impl Default for Producer {
    fn default() -> Self {
        Self::new()
    }
}

impl Producer {
    #[must_use] 
    pub const fn new() -> Self {
        Self { buf: Vec::new(), first: None, blocking: false }
    }

    #[must_use] 
    pub const fn blocking() -> Self {
        Self { buf: Vec::new(), first: None, blocking: true }
    }

    pub fn push(&mut self, session: u64, tag: Tag, at: Instant, data: &[u8]) {
        self.push_parts(session, tag, at, &[data]);
    }

    // Ein Eintrag aus mehreren Stuecken (s. Scrubbed::Strip) -- ohne Zwischenkopie.
    pub fn push_parts(&mut self, session: u64, tag: Tag, at: Instant, parts: &[&[u8]]) {
        if session == 0 {
            return;
        }
        if self.buf.capacity() == 0 {
            self.buf = take_buffer();
        }
        let len: usize = parts.iter().map(|p| p.len()).sum();
        self.buf.extend_from_slice(&session.to_le_bytes());
        self.buf.push(tag.byte());
        self.buf.extend_from_slice(&ms(at).to_le_bytes());
        self.buf.extend_from_slice(&(len as u32).to_le_bytes());
        for p in parts {
            self.buf.extend_from_slice(p);
        }
        if self.first.is_none() {
            self.first = Some(Instant::now());
        }
        if self.buf.len() >= FLUSH_BYTES {
            self.flush();
        }
    }

    // Raus, wenn der aelteste Eintrag lange genug liegt.
    pub fn flush_due(&mut self) {
        if self.first.is_some_and(|t| t.elapsed() >= FLUSH_EVERY) {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        self.first = None;
        if self.buf.is_empty() {
            return;
        }
        let buf = std::mem::take(&mut self.buf);
        if self.blocking {
            if sender().send(Msg::Chunk(buf)).is_err() {
                DROPPED.fetch_add(1, Relaxed);
            }
            return;
        }
        if let Err(TrySendError::Full(Msg::Chunk(b)) | TrySendError::Disconnected(Msg::Chunk(b))) = sender().try_send(Msg::Chunk(buf)) {
            DROPPED.fetch_add(1, Relaxed);
            recycle(b);
        }
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.flush();
    }
}

// Eine Aufnahme-Datei. Lebt so lange wie die Verbindung; Drop schliesst sie und setzt den
// Slot wieder auf 0.
pub struct Session {
    id: u64,
    producer: Producer,
    // Hier steht die Sitzungsnummer fuer die anderen Erzeuger (Kamera, Maus im Hauptthread).
    slot: Option<Arc<AtomicU64>>,
}

impl Session {
    // Err mit dem Grund, wenn keine Aufnahme laeuft (Modul aus, kein Ordner ...). Ok traegt
    // den Pfad mit; ob die Datei angelegt werden konnte, meldet der Schreib-Thread selbst.
    pub fn open(mut header: Header, slot: Option<Arc<AtomicU64>>) -> Result<(Self, PathBuf), String> {
        if !enabled() {
            return Err("Modul aus".into());
        }
        let dir = DIR.lock().ok().and_then(|d| d.clone()).ok_or("kein Ordner fuer Aufnahmen")?;
        header.client = CLIENT.lock().map(|c| c.clone()).unwrap_or_default();
        header.wall_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        let id = NEXT_ID.fetch_add(1, Relaxed);
        let path = dir.join(file_name(&header.server));
        let start = ms(Instant::now());
        sender().try_send(Msg::Open { id, path: path.clone(), header, start }).map_err(|_| "Schreib-Thread nicht erreichbar".to_string())?;
        if let Some(s) = &slot {
            s.store(id, Relaxed);
        }
        Ok((Self { id, producer: Producer::new(), slot }, path))
    }

    #[must_use] 
    pub const fn id(&self) -> u64 {
        self.id
    }

    pub fn push(&mut self, tag: Tag, data: &[u8]) {
        self.push_parts(tag, &[data]);
    }

    pub fn push_parts(&mut self, tag: Tag, parts: &[&[u8]]) {
        self.producer.push_parts(self.id, tag, Instant::now(), parts);
        self.producer.flush_due();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(s) = &self.slot {
            let _ = s.compare_exchange(self.id, 0, Relaxed, Relaxed);
        }
        self.producer.flush();
        let _ = sender().try_send(Msg::Close(self.id));
    }
}

// Pakete einer Verbindung: S und C, C im Play-State ohne Schluessel und Signaturen (s. scrub).
// Lebt ab der Configuration so lange wie die Verbindung und folgt dem Modul-Schalter: an = ab
// sofort eine neue Datei, aus = Datei schliessen. Nur eine Datei, die schon beim Join lief,
// hat den Anfangszustand (Header::from_join).
pub struct PacketRecorder {
    header: Header,
    slot: Option<Arc<AtomicU64>>,
    session: Option<Session>,
    // Bekommt die Sitzungsnummer, wenn eine Aufnahme mitten im Spiel beginnt: dann muss der
    // Zustand bis hierher als Snapshot nachkommen (s. Tag::Z).
    on_mid_start: Option<Box<dyn FnMut(u64) + Send>>,
    play: bool,
    // Zuletzt gesehener Stand des Schalters.
    an: bool,
}

impl PacketRecorder {
    #[must_use] 
    pub fn new(header: Header, slot: Option<Arc<AtomicU64>>) -> Self {
        let mut r = Self { header, slot, session: None, on_mid_start: None, play: false, an: enabled() };
        if r.an {
            r.start(true);
        }
        r
    }

    #[must_use] 
    pub fn on_mid_start(mut self, f: Box<dyn FnMut(u64) + Send>) -> Self {
        self.on_mid_start = Some(f);
        self
    }

    fn start(&mut self, from_join: bool) {
        let header = Header { from_join, ..self.header.clone() };
        match Session::open(header, self.slot.clone()) {
            Ok((s, pfad)) => {
                let ab = if from_join { "" } else { " (ab der Mitte, mit Snapshot)" };
                eprintln!("coral-record: Aufnahme nach {}{ab}", pfad.display());
                let id = s.id();
                self.session = Some(s);
                if !from_join {
                    if let Some(f) = &mut self.on_mid_start {
                        f(id);
                    }
                }
            }
            Err(grund) => eprintln!("coral-record: keine Aufnahme ({grund})"),
        }
    }

    // Dem Schalter folgen. Ein Atomic-Load je Paket.
    fn sync(&mut self) {
        let an = enabled();
        if an == self.an {
            return;
        }
        self.an = an;
        if an {
            self.start(false);
        } else if self.session.take().is_some() {
            eprintln!("coral-record: Aufnahme beendet");
        }
    }

    // Ab hier gelten die Play-IDs; zurueck in der Configuration wieder nicht.
    pub const fn set_play(&mut self, play: bool) {
        self.play = play;
    }

    pub fn s2c(&mut self, payload: &[u8]) {
        self.sync();
        if let Some(s) = &mut self.session {
            s.push(Tag::S, payload);
        }
    }

    pub fn c2s(&mut self, payload: &[u8]) {
        self.sync();
        let Some(s) = &mut self.session else { return };
        if !self.play {
            return s.push(Tag::C, payload);
        }
        match scrub_c2s_play(payload) {
            Scrubbed::Keep => s.push(Tag::C, payload),
            Scrubbed::Strip(a, b) => s.push_parts(Tag::C, &[a, &[0], b]),
            Scrubbed::Drop => {}
        }
    }
}

// Alles Offene auf die Platte, z. B. beim Beenden. Wartet hoechstens `timeout`.
pub fn finish(timeout: Duration) {
    let Some(tx) = TX.get() else { return };
    let (ack_tx, ack_rx) = mpsc::channel();
    let ende = Instant::now() + timeout;
    let mut msg = Msg::Finish(ack_tx);
    loop {
        match tx.try_send(msg) {
            Ok(()) => break,
            Err(TrySendError::Full(m)) if Instant::now() < ende => {
                msg = m;
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return,
        }
    }
    let _ = ack_rx.recv_timeout(ende.saturating_duration_since(Instant::now()));
}

// <Datum>_<Uhrzeit>_<Server>.crec, Ortszeit.
fn file_name(server: &str) -> String {
    let server: String = server.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-') { c.to_ascii_lowercase() } else { '_' }).collect();
    format!("{}_{server}.crec", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"))
}

fn writer_loop(rx: Receiver<Msg>) {
    // SAFETY: setpriority auf den eigenen Thread (unter Linux gilt PRIO_PROCESS mit 0 nur fuer
    // den aufrufenden Thread), keine Zeiger.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
    let mut sessions: HashMap<u64, SessionFile> = HashMap::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Msg::Open { id, path, header, start }) => {
                if let Some(f) = SessionFile::create(&path, &header, start) {
                    sessions.insert(id, f);
                }
            }
            Ok(Msg::Chunk(buf)) => {
                demux(&buf, &mut sessions);
                recycle(buf);
            }
            Ok(Msg::Close(id)) => {
                if let Some(f) = sessions.remove(&id) {
                    f.finish();
                }
            }
            Ok(Msg::Finish(ack)) => {
                for (_, f) in sessions.drain() {
                    f.finish();
                }
                let _ = ack.send(());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                for (_, f) in sessions.drain() {
                    f.finish();
                }
                return;
            }
        }
        let cut = ms(Instant::now()) - LATE_MS;
        sessions.retain(|_, f| f.write_ready(cut));
    }
}

// Verteilt einen Puffer auf die Sitzungen. Unbekannte (schon geschlossene) fallen weg.
fn demux(buf: &[u8], sessions: &mut HashMap<u64, SessionFile>) {
    let mut i = 0;
    while i + 21 <= buf.len() {
        let session = u64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
        let tag = buf[i + 8];
        let t = f64::from_le_bytes(buf[i + 9..i + 17].try_into().unwrap());
        let len = u32::from_le_bytes(buf[i + 17..i + 21].try_into().unwrap()) as usize;
        let data = &buf[(i + 21).min(buf.len())..(i + 21 + len).min(buf.len())];
        i += 21 + len;
        if let (Some(f), Some(tag)) = (sessions.get_mut(&session), Tag::from_byte(tag)) {
            f.add(t, tag, data);
        }
    }
}

// Eine offene Aufnahme im Schreib-Thread.
struct SessionFile {
    out: BufWriter<File>,
    // Sitzungsstart in ms seit epoch.
    start: f64,
    // Noch nicht geschriebene Eintraege: Zeit (relativ), Spur, Daten.
    pending: Vec<(f64, Tag, Vec<u8>)>,
    index: Vec<(f64, u64)>,
    pos: u64,
    // Aufnahme ab der Mitte: der Snapshot (Spur Z) steht vor allem anderen in der Datei. Bis
    // er abgeschlossen ist, wird nichts geschrieben.
    snapshot: Vec<Vec<u8>>,
    awaiting: Option<Instant>,
}

impl SessionFile {
    // Fehler gehen ins Log (stderr landet in client.log): der Netz-Task hat die Datei da
    // schon als gestartet gemeldet.
    fn create(path: &Path, header: &Header, start: f64) -> Option<Self> {
        let f = Self::try_create(path, header, start);
        if let Err(e) = &f {
            eprintln!("[coral-record] {} nicht anlegbar: {e}", path.display());
        }
        f.ok()
    }

    fn try_create(path: &Path, header: &Header, start: f64) -> std::io::Result<Self> {
        let parent = path.parent().ok_or_else(|| std::io::Error::other("kein Ordner"))?;
        std::fs::create_dir_all(parent)?;
        // Zwei Sitzungen in derselben Sekunde (zwei Tabs): die zweite bekommt eine Nummer.
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let mut path = path.to_path_buf();
        let mut n = 2;
        while path.exists() {
            path.set_file_name(format!("{stem}~{n}.crec"));
            n += 1;
        }
        let mut out = BufWriter::new(File::create(&path)?);
        let body = header.encode();
        out.write_all(MAGIC)?;
        out.write_all(&VERSION.to_le_bytes())?;
        out.write_all(&(body.len() as u32).to_le_bytes())?;
        out.write_all(&body)?;
        out.flush()?;
        let pos = (4 + 2 + 4 + body.len()) as u64;
        let awaiting = (!header.from_join).then(Instant::now);
        Ok(Self { out, start, pending: Vec::new(), index: Vec::new(), pos, snapshot: Vec::new(), awaiting })
    }

    fn add(&mut self, t: f64, tag: Tag, data: &[u8]) {
        if tag == Tag::Z {
            if self.awaiting.is_none() {
                return;
            }
            if data.is_empty() {
                self.awaiting = None;
                eprintln!("coral-record: Snapshot vollstaendig ({} Eintraege)", self.snapshot.len());
            } else {
                self.snapshot.push(data.to_vec());
            }
            return;
        }
        self.pending.push((t - self.start, tag, data.to_vec()));
    }

    // Kommt der Snapshot nicht rechtzeitig, bleibt die Datei ohne ihn (nicht abspielbar).
    fn snapshot_ready(&mut self, force: bool) -> bool {
        let Some(seit) = self.awaiting else { return true };
        if !force && seit.elapsed() < SNAPSHOT_WAIT {
            return false;
        }
        eprintln!("coral-record: Snapshot unvollstaendig ({} Eintraege), Aufnahme ohne Anfangszustand", self.snapshot.len());
        self.awaiting = None;
        self.snapshot.clear();
        true
    }

    // Der Snapshot als eigene Bloecke bei t = 0, vor dem ersten Paket.
    fn write_snapshot(&mut self) -> std::io::Result<()> {
        let alle = std::mem::take(&mut self.snapshot);
        let mut block = Vec::new();
        let mut groesse = 0;
        for data in alle {
            groesse += data.len();
            block.push((0.0, Tag::Z, data));
            if groesse >= SNAPSHOT_BLOCK {
                self.write_entries(std::mem::take(&mut block))?;
                groesse = 0;
            }
        }
        if !block.is_empty() {
            self.write_entries(block)?;
        }
        Ok(())
    }

    // Schreibt Bloecke von BLOCK_MS, solange einer bis `cut` (ms seit epoch) voll ist.
    // false = Schreibfehler, die Sitzung wird aufgegeben.
    fn write_ready(&mut self, cut: f64) -> bool {
        if !self.snapshot_ready(false) {
            return true;
        }
        if !self.snapshot.is_empty() && self.write_snapshot().is_err() {
            return false;
        }
        let cut = cut - self.start;
        self.pending.sort_by(|a, b| a.0.total_cmp(&b.0));
        while self.pending.first().is_some_and(|e| e.0 <= cut - BLOCK_MS) {
            if self.write_next_block().is_err() {
                return false;
            }
        }
        true
    }

    // Die Eintraege der naechsten BLOCK_MS als ein Block (pending ist sortiert).
    fn write_next_block(&mut self) -> std::io::Result<()> {
        let grenze = self.pending[0].0 + BLOCK_MS;
        let n = self.pending.partition_point(|e| e.0 < grenze).max(1);
        self.write_block(n)
    }

    fn write_block(&mut self, n: usize) -> std::io::Result<()> {
        let entries: Vec<_> = self.pending.drain(..n).collect();
        self.write_entries(entries)
    }

    fn write_entries(&mut self, entries: Vec<(f64, Tag, Vec<u8>)>) -> std::io::Result<()> {
        let mut raw = Vec::new();
        for (t, tag, data) in &entries {
            raw.push(tag.byte());
            raw.extend_from_slice(&t.to_le_bytes());
            put_varint(&mut raw, data.len() as u32);
            raw.extend_from_slice(data);
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw)?;
        let comp = z.finish()?;
        let (start, end) = (entries[0].0, entries[entries.len() - 1].0);
        self.out.write_all(&[BLOCK])?;
        self.out.write_all(&start.to_le_bytes())?;
        self.out.write_all(&end.to_le_bytes())?;
        self.out.write_all(&(entries.len() as u32).to_le_bytes())?;
        self.out.write_all(&(raw.len() as u32).to_le_bytes())?;
        self.out.write_all(&(comp.len() as u32).to_le_bytes())?;
        self.out.write_all(&comp)?;
        // Jeder Block ganz auf die Platte: nach einem Absturz fehlt hoechstens der letzte.
        self.out.flush()?;
        self.index.push((start, self.pos));
        self.pos += (BLOCK_HEAD + comp.len()) as u64;
        Ok(())
    }

    // Rest schreiben, Index anhaengen.
    fn finish(mut self) {
        self.snapshot_ready(true);
        if !self.snapshot.is_empty() && self.write_snapshot().is_err() {
            return;
        }
        self.pending.sort_by(|a, b| a.0.total_cmp(&b.0));
        while !self.pending.is_empty() {
            if self.write_next_block().is_err() {
                return;
            }
        }
        let mut b = vec![INDEX];
        b.extend_from_slice(&(self.index.len() as u32).to_le_bytes());
        for (t, pos) in &self.index {
            b.extend_from_slice(&t.to_le_bytes());
            b.extend_from_slice(&pos.to_le_bytes());
        }
        b.extend_from_slice(&self.pos.to_le_bytes());
        b.extend_from_slice(END);
        let _ = self.out.write_all(&b).and_then(|()| self.out.flush());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("crec-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn header() -> Header {
        Header { protocol: 774, client: "test".into(), server: "localhost:25565".into(), wall_ms: 1.7e12, uuid: [7; 16], name: "Spieler".into(), from_join: true }
    }

    // Schreiben und Lesen ergeben dieselben Eintraege, nach Zeit sortiert, ueber mehrere Bloecke.
    #[test]
    fn hin_und_zurueck() {
        let path = temp_dir("rund").join("a.crec");
        let mut f = SessionFile::create(&path, &header(), 100.0).unwrap();
        for i in 0..50 {
            f.add(100.0 + f64::from(i) * 100.0, Tag::S, &[i as u8; 3]);
        }
        f.add(150.0, Tag::K, b"kamera");
        f.finish();
        let mut r = Reader::open(&path).unwrap();
        assert_eq!(r.header, header());
        assert!(r.complete);
        assert!(r.blocks.len() >= 4, "{} Bloecke", r.blocks.len());
        let mut alle = Vec::new();
        r.for_each(|e| alle.push(e)).unwrap();
        assert_eq!(alle.len(), 51);
        assert!(alle.windows(2).all(|w| w[0].t <= w[1].t));
        assert_eq!(alle[0], Entry { tag: Tag::S, t: 0.0, data: vec![0; 3] });
        assert_eq!(alle[1], Entry { tag: Tag::K, t: 50.0, data: b"kamera".to_vec() });
    }

    // Ohne Index und mit abgeschnittenem letzten Block: alle ganzen Bloecke bleiben lesbar.
    #[test]
    fn abgebrochene_datei_bleibt_lesbar() {
        let path = temp_dir("abbruch").join("a.crec");
        let mut f = SessionFile::create(&path, &header(), 0.0).unwrap();
        for i in 0..30 {
            f.add(f64::from(i) * 200.0, Tag::C, &[1, 2, 3, i as u8]);
        }
        assert!(f.write_ready(10_000.0));
        let bloecke = f.index.len();
        assert!(bloecke >= 2);
        drop(f);
        let len = std::fs::metadata(&path).unwrap().len();
        // Ein halber Block dahinter, wie nach einem Absturz mitten im Schreiben.
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[BLOCK, 1, 2, 3]).unwrap();
        drop(file);
        let mut r = Reader::open(&path).unwrap();
        assert!(!r.complete);
        assert_eq!(r.blocks.len(), bloecke);
        let mut n = 0;
        r.for_each(|_| n += 1).unwrap();
        assert!(n > 0 && n <= 30);
        assert!(std::fs::metadata(&path).unwrap().len() > len);
    }

    // Ab der Mitte: nichts wird geschrieben, bis der Snapshot abgeschlossen ist; dann steht er
    // in eigenen Bloecken vor dem ersten Paket, und die Wiedergabe bekommt ihn vorab.
    #[test]
    fn snapshot_steht_vor_den_paketen() {
        let path = temp_dir("snapshot").join("a.crec");
        let kopf = Header { from_join: false, ..header() };
        let mut f = SessionFile::create(&path, &kopf, 0.0).unwrap();
        f.add(10.0, Tag::S, &[0x26, 1]);
        f.add(20.0, Tag::K, b"k");
        f.add(30.0, Tag::Z, b"eins");
        assert!(f.write_ready(100_000.0));
        assert!(f.index.is_empty(), "vor dem Abschluss geschrieben");
        f.add(40.0, Tag::Z, b"zwei");
        f.add(50.0, Tag::Z, &[]);
        f.add(2000.0, Tag::S, &[0x27]);
        assert!(f.write_ready(100_000.0));
        f.finish();
        let s = summary(&path).unwrap();
        assert!(s.playable && !s.header.from_join);
        let uhr = Arc::new(ReplayClock::new(1e9));
        let mut q = PacketSource::open(&path, uhr).unwrap();
        assert_eq!(q.take_snapshot(), vec![b"eins".to_vec(), b"zwei".to_vec()]);
        let mut pakete = Vec::new();
        while let Ok(Next::Packet(p)) = q.next_due() {
            pakete.push(p);
        }
        assert_eq!(pakete, vec![vec![0x26, 1], vec![0x27]]);
    }

    // Ohne Abschluss des Snapshots bleibt die Datei ohne ihn: nicht abspielbar.
    #[test]
    fn halber_snapshot_faellt_weg() {
        let path = temp_dir("halb").join("a.crec");
        let mut f = SessionFile::create(&path, &Header { from_join: false, ..header() }, 0.0).unwrap();
        f.add(10.0, Tag::Z, b"eins");
        f.add(20.0, Tag::S, &[0x26]);
        f.finish();
        assert!(!summary(&path).unwrap().playable);
    }

    // Der ganze Weg ueber Producer, Kanal und Schreib-Thread. Der einzige Test mit den globalen
    // Schaltern.
    #[test]
    fn sitzung_ueber_den_schreib_thread() {
        let dir = temp_dir("thread");
        set_dir(dir.clone());
        set_client_version("v-test".into());
        set_enabled(true);
        let slot = Arc::new(AtomicU64::new(0));
        let mut rec = PacketRecorder::new(header(), Some(Arc::clone(&slot)));
        let id = slot.load(Relaxed);
        assert_ne!(id, 0);
        rec.s2c(&[0x26, 1, 2]);
        rec.c2s(&[0x00, 9]);
        let mut app = Producer::new();
        app.push(id, Tag::K, Instant::now(), b"k");
        app.flush();
        // Mitten in der Verbindung aus und wieder an: erste Datei zu, zweite ohne Anfangszustand.
        set_enabled(false);
        rec.s2c(&[0x27]);
        assert_eq!(slot.load(Relaxed), 0);
        set_enabled(true);
        rec.s2c(&[0x28]);
        assert_ne!(slot.load(Relaxed), 0);
        drop(rec);
        assert_eq!(slot.load(Relaxed), 0);
        finish(Duration::from_secs(5));
        let mut dateien: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|d| d.unwrap().path()).collect();
        dateien.sort();
        assert_eq!(dateien.len(), 2);
        let lesen = |p: &Path| {
            let mut r = Reader::open(p).unwrap();
            let mut tags = Vec::new();
            r.for_each(|e| tags.push(e.tag)).unwrap();
            tags.sort_by_key(|t| t.byte());
            (r, tags)
        };
        let (r, tags) = lesen(&dateien[0]);
        assert_eq!(r.header.client, "v-test");
        assert!(r.complete && r.header.from_join);
        assert_eq!(tags, vec![Tag::C, Tag::K, Tag::S]);
        let (r, tags) = lesen(&dateien[1]);
        assert!(r.complete && !r.header.from_join);
        assert_eq!(tags, vec![Tag::S]);
        set_enabled(false);
    }
}
