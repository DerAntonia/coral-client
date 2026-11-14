// coral-clip: wie ein Instant Replay. Statt in eine Datei laufen die Eintraege einer Verbindung
// in einen Puffer im Speicher (ClipRing, im Schreib-Thread); erst auf Wunsch wird daraus eine
// .crec-Datei, abspielbar wie jede Aufnahme.
//
// Ein Clip aus der Mitte braucht den Zustand an seinem Anfang. Den liefert die App: sie bittet
// regelmaessig um einen Haltepunkt (ClipLink::request). Der Netz-Task setzt ihn als Markierung
// GENAU zwischen zwei Pakete und meldet ihn der App im Ereignisstrom, die an dieser Stelle
// ihren Zustand einsammelt (unkodiert, die Chunk-Spalten als Arc). Kodiert und geschrieben wird
// der Zustand erst beim Speichern. Was vor dem aeltesten noch gebrauchten Haltepunkt liegt,
// faellt weg (clip_trim).
//
// Haltepunkt 0 ist der Beginn der Sitzung: lief das Modul schon beim Join, braucht er keinen
// Zustand.

use super::*;
use std::sync::atomic::AtomicU32;

// Mehr als das haelt ein Puffer nicht (Rohbytes). Kommt nur vor, wenn keine Haltepunkte mehr
// kommen, etwa in einem Tab, der hinten liegt.
const CLIP_CAP: usize = 512 << 20;
// Grundlast je Eintrag im Puffer (Zeit, Spur, Vec).
const ENTRY_COST: usize = 48;
// Haltepunkte durch Teleportationen kommen hoechstens so dicht (manche Server teleportieren
// in Serie, und jeder Haltepunkt kostet die App das Einsammeln ihres Zustands).
const TELEPORT_GAP: Duration = Duration::from_secs(1);

static CLIP_ENABLED: AtomicBool = AtomicBool::new(false);

// Modul coral-clip an/aus. Laufende Verbindungen folgen beim naechsten Paket.
pub fn set_clip_enabled(on: bool) {
    CLIP_ENABLED.store(on, Relaxed);
}

pub fn clip_enabled() -> bool {
    CLIP_ENABLED.load(Relaxed)
}

// ms seit dem gemeinsamen Nullpunkt (s. epoch) -- die Zeit der Haltepunkte.
pub fn now_ms() -> f64 {
    ms(Instant::now())
}

// Verbindet App, Netz-Task und Schreib-Thread fuer den Puffer einer Verbindung (je Tab einer).
#[derive(Default)]
pub struct ClipLink {
    // Sitzungsnummer des Puffers, 0 = keiner.
    slot: AtomicU64,
    // Nummer des zuletzt erbetenen Haltepunkts.
    want: AtomicU32,
    // Der aelteste Haltepunkt, ab dem der Puffer noch lueckenlos ist (u32::MAX = keiner).
    floor: AtomicU32,
}

impl ClipLink {
    pub fn session(&self) -> u64 {
        self.slot.load(Relaxed)
    }

    // Bittet um einen Haltepunkt vor dem naechsten Paket. Die Antwort kommt als ClipEvent.
    pub fn request(&self) {
        self.want.fetch_add(1, Relaxed);
    }

    pub fn floor(&self) -> u32 {
        self.floor.load(Relaxed)
    }

    pub(crate) fn release(&self, id: u64) {
        let _ = self.slot.compare_exchange(id, 0, Relaxed, Relaxed);
    }
}

// Was der Netz-Task der App meldet, an genau der Stelle im Ereignisstrom.
#[derive(Clone, Copy, Debug)]
pub enum ClipEvent {
    // Ein Puffer hat begonnen. `from_join`: er enthaelt den Anfangszustand (Haltepunkt 0).
    Started { session: u64, from_join: bool, t_ms: f64 },
    // Haltepunkt `seq`: alles davor ist verarbeitet.
    Checkpoint { session: u64, seq: u32, t_ms: f64 },
}

// Der Puffer-Teil eines PacketRecorder.
pub(crate) struct ClipTap {
    link: Arc<ClipLink>,
    session: Option<Session>,
    an: bool,
    seen: u32,
    // Wann zuletzt eine Teleportation einen Haltepunkt gesetzt hat.
    teleport: Option<Instant>,
    on_event: Box<dyn FnMut(ClipEvent) + Send>,
}

impl ClipTap {
    pub(crate) fn new(link: Arc<ClipLink>, on_event: Box<dyn FnMut(ClipEvent) + Send>, header: &Header) -> Self {
        let mut t = Self { link, session: None, an: clip_enabled(), seen: 0, teleport: None, on_event };
        if t.an {
            t.start(header, true);
        }
        t
    }

    fn start(&mut self, header: &Header, from_join: bool) {
        self.seen = self.link.want.load(Relaxed);
        match Session::open_clip(Header { from_join, ..header.clone() }, &self.link) {
            Ok(s) => {
                (self.on_event)(ClipEvent::Started { session: s.id(), from_join, t_ms: now_ms() });
                self.session = Some(s);
            }
            Err(grund) => eprintln!("coral-clip: kein Puffer ({grund})"),
        }
    }

    // Vor jedem Paket: Modul umgeschaltet? Haltepunkt erbeten (nur vor Paketen vom Server)?
    // `teleport`: das Paket versetzt den eigenen Spieler -- davor liegt immer ein Haltepunkt
    // (Nutzer 2026-11-12), der Clip kann also genau mit der Teleportation beginnen.
    pub(crate) fn sync(&mut self, header: &Header, s2c: bool, teleport: bool) -> Option<&mut Session> {
        let an = clip_enabled();
        if an != self.an {
            self.an = an;
            if an {
                self.start(header, false);
            } else {
                self.session = None;
            }
        }
        let s = self.session.as_mut()?;
        if teleport && self.teleport.is_none_or(|t| t.elapsed() >= TELEPORT_GAP) {
            self.teleport = Some(Instant::now());
            self.link.request();
        }
        let want = self.link.want.load(Relaxed);
        if s2c && want != self.seen {
            self.seen = want;
            let mut m = [Mark::Checkpoint as u8, 0, 0, 0, 0];
            m[1..].copy_from_slice(&want.to_le_bytes());
            s.push(Tag::E, &m);
            (self.on_event)(ClipEvent::Checkpoint { session: s.id(), seq: want, t_ms: now_ms() });
        }
        Some(s)
    }
}

impl Session {
    pub(crate) fn open_clip(mut header: Header, link: &Arc<ClipLink>) -> Result<Self, String> {
        header.client = CLIENT.lock().map(|c| c.clone()).unwrap_or_default();
        header.wall_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        let id = NEXT_ID.fetch_add(1, Relaxed);
        let start = ms(Instant::now());
        sender().try_send(Msg::OpenClip { id, header, start, link: Arc::clone(link) }).map_err(|_| "Schreib-Thread nicht erreichbar".to_string())?;
        link.floor.store(0, Relaxed);
        link.slot.store(id, Relaxed);
        Ok(Self { id, producer: Producer::new(), slot: None, clip: Some(Arc::clone(link)) })
    }
}

// Alles vor dem Haltepunkt `seq` wird nicht mehr gebraucht.
pub fn clip_trim(session: u64, seq: u32) {
    let _ = sender().try_send(Msg::ClipTrim { id: session, seq });
}

// Schreibt den Puffer `session` ab dem Haltepunkt `seq` in eine neue Datei. Zurueck kommen die
// Sitzungsnummer der Datei und ein Kanal mit dem Pfad oder dem Grund des Scheiterns. Ab einem
// Haltepunkt > 0 wartet die Datei auf ihren Snapshot (Spur Z an die neue Nummer, mit leerem
// Eintrag am Ende) und danach auf close.
//
// Ueberlappt der Clip mit dem vorigen desselben Puffers (sein Anfang liegt nicht nach dessen
// Ende), entsteht keine neue Datei: was seitdem dazukam, wird an die vorige angehaengt, nichts
// steht doppelt (Nutzer 2026-11-12). Dann braucht es weder Snapshot noch close (s. ClipSaved).
pub fn clip_save(session: u64, seq: u32) -> Option<(u64, Receiver<Result<ClipSaved, String>>)> {
    let dir = DIR.lock().ok().and_then(|d| d.clone())?;
    let file = NEXT_ID.fetch_add(1, Relaxed);
    let (reply, rx) = mpsc::channel();
    sender().try_send(Msg::ClipSave { clip: session, file, dir, seq, reply }).ok()?;
    Some((file, rx))
}

// Antwort auf clip_save.
#[derive(Clone, Debug)]
pub struct ClipSaved {
    pub path: PathBuf,
    // Die Datei wartet auf ihren Snapshot und danach auf close.
    pub needs_snapshot: bool,
    // Keine neue Datei: der vorige Clip wurde verlaengert.
    pub extended: bool,
}

// Schliesst eine Datei aus clip_save. Wartet, wenn der Kanal voll ist (nicht im Hauptthread).
pub fn close(file: u64) {
    let _ = sender().send(Msg::Close(file));
}

fn marker_seq(tag: Tag, data: &[u8]) -> Option<u32> {
    (tag == Tag::E && data.len() == 5 && data[0] == Mark::Checkpoint as u8).then(|| u32::from_le_bytes(data[1..].try_into().unwrap()))
}

// Was nur bei Aenderung aufgezeichnet wird und darum am Anfang eines Clips fehlen wuerde: der
// letzte lokale Zustand, vorn/hinten und die Packs.
#[derive(Clone, Default)]
struct Sticky {
    state: Option<Vec<u8>>,
    front: Option<Vec<u8>>,
    packs: Option<Vec<u8>>,
}

impl Sticky {
    fn note(&mut self, tag: Tag, data: &[u8]) {
        match (tag, data.first()) {
            (Tag::L, _) => self.state = Some(data.to_vec()),
            (Tag::E, Some(&m)) if m == Mark::Front as u8 || m == Mark::Back as u8 => self.front = Some(data.to_vec()),
            (Tag::E, Some(&m)) if m == Mark::Packs as u8 => self.packs = Some(data.to_vec()),
            _ => {}
        }
    }
}

// Der zuletzt geschriebene Clip eines Puffers.
struct LastClip {
    // Sitzungsnummer der Datei, solange sie noch offen ist (wartet auf Snapshot und close).
    file: u64,
    // Nach dem Schliessen: was zum Anhaengen noetig ist.
    done: Option<Finished>,
    // Alle Eintraege mit kleinerer Ankunftsnummer stehen schon drin.
    written: u64,
    // Zeit des letzten Eintrags darin.
    end: f64,
}

pub(crate) struct ClipRing {
    header: Header,
    start: f64,
    // Zeit, Spur, Daten, Ankunftsnummer.
    entries: Vec<(f64, Tag, Vec<u8>, u64)>,
    next_n: u64,
    bytes: usize,
    sticky: Sticky,
    link: Arc<ClipLink>,
    last: Option<LastClip>,
}

impl ClipRing {
    pub(crate) fn new(header: Header, start: f64, link: Arc<ClipLink>) -> Self {
        Self { header, start, entries: Vec::new(), next_n: 0, bytes: 0, sticky: Sticky::default(), link, last: None }
    }

    pub(crate) fn add(&mut self, t: f64, tag: Tag, data: &[u8]) {
        if tag == Tag::Z {
            return;
        }
        if let Some(seq) = marker_seq(tag, data) {
            // Nach einem Ueberlauf ist der erste neue Haltepunkt wieder ein Anfang.
            let _ = self.link.floor.compare_exchange(u32::MAX, seq, Relaxed, Relaxed);
        }
        self.bytes += data.len() + ENTRY_COST;
        self.entries.push((t, tag, data.to_vec(), self.next_n));
        self.next_n += 1;
        if self.bytes > CLIP_CAP {
            self.overflow();
        }
    }

    // Nach Zeit. Stabil: innerhalb eines Erzeugers bleibt die Reihenfolge, also auch die Lage
    // eines Haltepunkts zwischen den Paketen.
    fn sort(&mut self) {
        self.entries.sort_by(|a, b| a.0.total_cmp(&b.0));
    }

    fn marker_pos(&self, seq: u32) -> Option<usize> {
        if self.link.floor() > seq {
            return None;
        }
        if seq == 0 {
            return Some(0);
        }
        self.entries.iter().position(|e| marker_seq(e.1, &e.2) == Some(seq))
    }

    fn drop_front(&mut self, n: usize) {
        for (_, tag, data, _) in self.entries.drain(..n) {
            self.sticky.note(tag, &data);
        }
        self.bytes = self.entries.iter().map(|e| e.2.len() + ENTRY_COST).sum();
    }

    pub(crate) fn trim(&mut self, seq: u32) {
        self.sort();
        if let Some(pos) = self.marker_pos(seq) {
            self.drop_front(pos);
            self.link.floor.fetch_max(seq, Relaxed);
        }
    }

    // Zu gross: alles vor dem juengsten Haltepunkt weg. Reicht das nicht, alles.
    fn overflow(&mut self) {
        self.sort();
        let letzter = self.entries.iter().enumerate().rev().find_map(|(i, e)| marker_seq(e.1, &e.2).map(|seq| (i, seq)));
        match letzter {
            Some((pos, seq)) if pos > 0 => {
                self.drop_front(pos);
                self.link.floor.store(seq, Relaxed);
            }
            _ => {
                self.drop_front(self.entries.len());
                self.link.floor.store(u32::MAX, Relaxed);
            }
        }
        eprintln!("coral-clip: Puffer zu gross, Anfang verworfen");
    }

    // Wo ein Clip ab dem Haltepunkt `seq` beginnt: Stelle im (sortierten) Puffer und Zeit.
    fn clip_start(&mut self, seq: u32) -> Result<(usize, f64), String> {
        self.sort();
        let pos = self.marker_pos(seq).ok_or("der Anfang liegt nicht mehr im Puffer")?;
        if seq == 0 && !self.header.from_join {
            return Err("kein Anfangszustand".into());
        }
        Ok((pos, if seq == 0 { self.start } else { self.entries[pos].0 }))
    }

    // Der Schreib-Thread hat die Datei `file` geschlossen.
    pub(crate) fn file_closed(&mut self, file: u64, fertig: &Option<Finished>) {
        if let Some(l) = self.last.as_mut().filter(|l| l.file == file && l.done.is_none()) {
            l.done.clone_from(fertig);
            if fertig.is_none() {
                self.last = None;
            }
        }
    }

    // Speichert ab dem Haltepunkt `seq`: haengt an den vorigen Clip an, wenn der neue mit ihm
    // ueberlappt, sonst entsteht die Datei `file` (offen in `sessions`, wenn sie noch auf ihren
    // Snapshot wartet).
    pub(crate) fn save_into(&mut self, sessions: &mut HashMap<u64, SessionFile>, file: u64, seq: u32, dir: &Path) -> Result<ClipSaved, String> {
        let (_, t0) = self.clip_start(seq)?;
        let ende = self.entries.last().map_or(t0, |e| e.0);
        if let Some(path) = self.extend(sessions, t0) {
            return Ok(ClipSaved { path, needs_snapshot: false, extended: true });
        }
        let (f, fertig) = self.save(seq, dir)?;
        let path = f.path.clone();
        let done = if fertig {
            Some(f.finish().ok_or("Datei nicht schreibbar")?)
        } else {
            sessions.insert(file, f);
            None
        };
        self.last = Some(LastClip { file, done, written: self.next_n, end: ende });
        Ok(ClipSaved { path, needs_snapshot: !fertig, extended: false })
    }

    // Haengt alles seit dem vorigen Clip an dessen Datei an, wenn ein Clip ab `t0` mit ihm
    // ueberlappt. None: kein voriger, keine Ueberlappung oder die Datei ist nicht mehr da.
    fn extend(&mut self, sessions: &mut HashMap<u64, SessionFile>, t0: f64) -> Option<PathBuf> {
        let last = self.last.as_mut()?;
        if t0 > last.end {
            return None;
        }
        // Was spaet eintraf, aber vor dem bisherigen Ende liegt, kommt an das Ende.
        let (geschrieben, ende) = (last.written, last.end);
        let neu = self.entries.iter().filter(|e| e.3 >= geschrieben && marker_seq(e.1, &e.2).is_none());
        let path = if let Some(f) = sessions.get_mut(&last.file).filter(|_| last.done.is_none()) {
            for e in neu {
                f.add(e.0.max(ende), e.1, &e.2);
            }
            f.path.clone()
        } else {
            let weiter = last.done.as_ref().and_then(|d| SessionFile::reopen(d).ok()).and_then(|mut f| {
                for e in neu {
                    f.add(e.0.max(ende), e.1, &e.2);
                }
                f.finish()
            });
            let Some(d) = weiter else {
                self.last = None;
                return None;
            };
            let path = d.path.clone();
            last.done = Some(d);
            path
        };
        last.written = self.next_n;
        last.end = self.entries.last().map_or(ende, |e| e.0.max(ende));
        Some(path)
    }

    // Die Datei ab dem Haltepunkt `seq`. `true` dazu: sie ist vollstaendig (kein Snapshot noetig).
    pub(crate) fn save(&mut self, seq: u32, dir: &Path) -> Result<(SessionFile, bool), String> {
        let (pos, t0) = self.clip_start(seq)?;
        let from_join = seq == 0;
        let header = Header { from_join, wall_ms: self.header.wall_ms + (t0 - self.start), ..self.header.clone() };
        let path = dir.join(format!("clip_{}", file_name(&header.server)));
        let mut f = SessionFile::create(&path, &header, t0).ok_or("Datei nicht anlegbar")?;
        let mut sticky = self.sticky.clone();
        for (_, tag, data, _) in &self.entries[..pos] {
            sticky.note(*tag, data);
        }
        for (tag, data) in [(Tag::E, sticky.front), (Tag::E, sticky.packs), (Tag::L, sticky.state)] {
            if let Some(d) = data {
                f.add(t0, tag, &d);
            }
        }
        for (t, tag, data, _) in &self.entries[pos..] {
            if marker_seq(*tag, data).is_none() {
                f.add(t.max(t0), *tag, data);
            }
        }
        Ok((f, from_join))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kopf(from_join: bool) -> Header {
        Header { protocol: 774, client: "test".into(), server: "localhost:25565".into(), wall_ms: 1.7e12, uuid: [7; 16], name: "Spieler".into(), from_join }
    }

    fn marke(seq: u32) -> Vec<u8> {
        let mut m = vec![Mark::Checkpoint as u8];
        m.extend_from_slice(&seq.to_le_bytes());
        m
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("crec-clip-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn lies(f: SessionFile) -> (Header, Vec<Entry>) {
        let path = f.path.clone();
        f.finish();
        let mut r = Reader::open(&path).unwrap();
        let mut alle = Vec::new();
        r.for_each(|e| alle.push(e)).unwrap();
        (r.header.clone(), alle)
    }

    // Zehn Pakete, je 100 ms, vor dem sechsten der Haltepunkt 1; dazu Zustand und Packs davor.
    fn ring(from_join: bool) -> ClipRing {
        let mut r = ClipRing::new(kopf(from_join), 1000.0, Arc::new(ClipLink::default()));
        r.add(1010.0, Tag::E, &[Mark::Packs as u8, b'p']);
        r.add(1020.0, Tag::L, b"alt");
        r.add(1030.0, Tag::L, b"neu");
        for i in 0..10u8 {
            if i == 5 {
                r.add(1000.0 + f64::from(i) * 100.0, Tag::E, &marke(1));
            }
            r.add(1000.0 + f64::from(i) * 100.0, Tag::S, &[i]);
        }
        r
    }

    #[test]
    fn clip_ab_join_enthaelt_alles() {
        let dir = temp_dir("join");
        let (f, fertig) = ring(true).save(0, &dir).unwrap();
        assert!(fertig);
        let (kopf, alle) = lies(f);
        assert!(kopf.from_join);
        let pakete: Vec<u8> = alle.iter().filter(|e| e.tag == Tag::S).map(|e| e.data[0]).collect();
        assert_eq!(pakete, (0..10).collect::<Vec<u8>>());
        // Der Haltepunkt selbst steht nicht in der Datei.
        assert!(alle.iter().all(|e| marker_seq(e.tag, &e.data).is_none()));
    }

    #[test]
    fn clip_ab_haltepunkt_beginnt_genau_dort_und_kennt_den_zustand_davor() {
        let dir = temp_dir("mitte");
        let (f, fertig) = ring(true).save(1, &dir).unwrap();
        assert!(!fertig);
        let (kopf, alle) = lies(f);
        assert!(!kopf.from_join);
        assert!((kopf.wall_ms - (1.7e12 + 500.0)).abs() < 1e-3);
        let pakete: Vec<(u8, f64)> = alle.iter().filter(|e| e.tag == Tag::S).map(|e| (e.data[0], e.t)).collect();
        assert_eq!(pakete.iter().map(|p| p.0).collect::<Vec<_>>(), vec![5, 6, 7, 8, 9]);
        assert!(pakete[0].1.abs() < 1e-9 && (pakete[4].1 - 400.0).abs() < 1e-9);
        // Zustand und Packs von davor stehen am Anfang, der ueberholte Zustand nicht.
        let zustand: Vec<&[u8]> = alle.iter().filter(|e| e.tag == Tag::L).map(|e| e.data.as_slice()).collect();
        assert_eq!(zustand, vec![b"neu".as_slice()]);
        assert!(alle.iter().any(|e| e.tag == Tag::E && e.data == [Mark::Packs as u8, b'p'] && e.t == 0.0));
    }

    #[test]
    fn kuerzen_wirft_den_anfang_weg_und_behaelt_den_zustand() {
        let dir = temp_dir("kurz");
        let mut r = ring(true);
        r.trim(1);
        assert_eq!(r.link.floor(), 1);
        assert_eq!(r.entries.iter().filter(|e| e.1 == Tag::S).count(), 5);
        assert!(r.save(0, &dir).is_err());
        let (f, _) = r.save(1, &dir).unwrap();
        let (_, alle) = lies(f);
        assert!(alle.iter().any(|e| e.tag == Tag::L && e.data == b"neu"));
    }

    #[test]
    fn puffer_ab_der_mitte_hat_keinen_haltepunkt_null() {
        let dir = temp_dir("ohne");
        assert!(ring(false).save(0, &dir).is_err());
        assert!(ring(false).save(1, &dir).is_ok());
    }

    fn pakete(alle: &[Entry]) -> Vec<u8> {
        alle.iter().filter(|e| e.tag == Tag::S).map(|e| e.data[0]).collect()
    }

    fn lies_pfad(path: &Path) -> (bool, Vec<Entry>) {
        let mut r = Reader::open(path).unwrap();
        let mut alle = Vec::new();
        r.for_each(|e| alle.push(e)).unwrap();
        (r.complete, alle)
    }

    #[test]
    fn ueberlappender_clip_verlaengert_die_offene_datei() {
        let dir = temp_dir("offen");
        let mut sessions = HashMap::new();
        let mut r = ring(true);
        let erster = r.save_into(&mut sessions, 7, 1, &dir).unwrap();
        assert!(erster.needs_snapshot && !erster.extended && sessions.contains_key(&7));
        // Fuenf Sekunden spaeter, derselbe Haltepunkt ist noch der Anfang: nichts doppelt.
        r.add(2000.0, Tag::S, &[10]);
        r.add(2100.0, Tag::S, &[11]);
        let zweiter = r.save_into(&mut sessions, 8, 1, &dir).unwrap();
        assert!(zweiter.extended && !zweiter.needs_snapshot);
        assert_eq!(zweiter.path, erster.path);
        assert!(!sessions.contains_key(&8));
        let fertig = sessions.remove(&7).unwrap().finish();
        r.file_closed(7, &fertig);
        assert_eq!(pakete(&lies_pfad(&erster.path).1), vec![5, 6, 7, 8, 9, 10, 11]);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        // Und noch einmal, jetzt ist die Datei geschlossen: sie wird wieder geoeffnet.
        r.add(2200.0, Tag::S, &[12]);
        assert!(r.save_into(&mut sessions, 9, 1, &dir).unwrap().extended);
        let (vollstaendig, alle) = lies_pfad(&erster.path);
        assert!(vollstaendig);
        assert_eq!(pakete(&alle), vec![5, 6, 7, 8, 9, 10, 11, 12]);
        assert!(alle.windows(2).all(|w| w[0].t <= w[1].t));
    }

    #[test]
    fn clip_ohne_ueberlappung_wird_eine_eigene_datei() {
        let dir = temp_dir("eigen");
        let mut sessions = HashMap::new();
        let mut r = ring(true);
        let erster = r.save_into(&mut sessions, 7, 0, &dir).unwrap();
        assert!(!erster.needs_snapshot && !erster.extended);
        // Der naechste Clip beginnt an einem Haltepunkt nach dem Ende des ersten.
        r.add(3000.0, Tag::S, &[20]);
        r.add(4000.0, Tag::E, &marke(2));
        r.add(4000.0, Tag::S, &[21]);
        let zweiter = r.save_into(&mut sessions, 8, 2, &dir).unwrap();
        assert!(!zweiter.extended && zweiter.needs_snapshot);
        assert_ne!(zweiter.path, erster.path);
        assert_eq!(pakete(&lies_pfad(&erster.path).1), (0..10).collect::<Vec<u8>>());
        sessions.remove(&8).unwrap().finish();
        assert_eq!(pakete(&lies_pfad(&zweiter.path).1), vec![21]);
    }

    #[test]
    fn geloeschter_clip_wird_nicht_verlaengert() {
        let dir = temp_dir("weg");
        let mut sessions = HashMap::new();
        let mut r = ring(true);
        let erster = r.save_into(&mut sessions, 7, 0, &dir).unwrap();
        std::fs::remove_file(&erster.path).unwrap();
        r.add(2000.0, Tag::S, &[10]);
        let zweiter = r.save_into(&mut sessions, 8, 0, &dir).unwrap();
        assert!(!zweiter.extended);
        assert_eq!(pakete(&lies_pfad(&zweiter.path).1), (0..11).collect::<Vec<u8>>());
    }

    #[test]
    fn spaet_eingetroffene_eintraege_landen_an_ihrer_zeit() {
        let dir = temp_dir("spaet");
        let mut r = ring(true);
        // Kamera eines anderen Erzeugers, kommt nach allem an, gehoert aber vor Paket 7.
        r.add(1650.0, Tag::K, b"k");
        let (f, _) = r.save(1, &dir).unwrap();
        let (_, alle) = lies(f);
        let i = alle.iter().position(|e| e.tag == Tag::K).unwrap();
        assert_eq!(alle[i - 1].data, vec![6]);
        assert_eq!(alle[i + 1].data, vec![7]);
    }
}
