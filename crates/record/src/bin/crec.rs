// Werkzeug zu coral-record:
//   crec info  <datei.crec>              Kopf, Dauer, Bloecke, Eintraege und Bytes je Spur
//   crec check <datei.crec> <pktraw>     Pakete S/C gegen einen CORAL_PKTRAW-Mitschnitt derselben
//                                        Sitzung abgleichen (Selbsttest aus Phase 1 des Plans)

use std::collections::HashMap;
use std::path::Path;

use record::{scrub_c2s_play, Entry, Reader, Scrubbed, Tag};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ergebnis = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["info", datei] => info(Path::new(datei)),
        ["check", datei, raw] => check(Path::new(datei), Path::new(raw)),
        _ => Err("Aufruf: crec info <datei.crec> | crec check <datei.crec> <pktraw>".to_string()),
    };
    if let Err(e) = ergebnis {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn lesen(datei: &Path) -> Result<(Reader, Vec<Entry>), String> {
    let mut r = Reader::open(datei).map_err(|e| format!("{}: {e}", datei.display()))?;
    let mut alle = Vec::new();
    r.for_each(|e| alle.push(e)).map_err(|e| e.to_string())?;
    Ok((r, alle))
}

fn info(datei: &Path) -> Result<(), String> {
    let (r, alle) = lesen(datei)?;
    let h = &r.header;
    let uuid: String = h.uuid.iter().map(|b| format!("{b:02x}")).collect();
    println!("Server:    {}", h.server);
    println!("Spieler:   {} ({uuid})", h.name);
    println!("Protokoll: {}  Client: {}", h.protocol, h.client);
    println!("Start:     {}", chrono::DateTime::from_timestamp_millis(h.wall_ms as i64).map_or("?".into(), |t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string()));
    let dauer = alle.last().map_or(0.0, |e| e.t) / 1000.0;
    let z = alle.iter().filter(|e| e.tag == Tag::Z).count();
    let anfang = match (h.from_join, z) {
        (true, _) => "ab dem Join".to_string(),
        (false, 0) => "ab der Mitte (ohne Anfangszustand)".to_string(),
        (false, n) => format!("ab der Mitte, Snapshot mit {n} Eintraegen"),
    };
    println!("Anfang:    {anfang}");
    println!("Dauer:     {dauer:.1} s, {} Bloecke, {}", r.blocks.len(), if r.complete { "sauber beendet" } else { "OHNE Index (abgebrochen)" });
    let groesse = std::fs::metadata(datei).map_or(0, |m| m.len());
    if dauer > 0.0 {
        println!("Datei:     {:.2} MB, {:.1} MB/h", groesse as f64 / 1e6, groesse as f64 / 1e6 / dauer * 3600.0);
    }
    let mut je: HashMap<Tag, (usize, usize)> = HashMap::new();
    for e in &alle {
        let s = je.entry(e.tag).or_default();
        s.0 += 1;
        s.1 += e.data.len();
    }
    for tag in Tag::ALL {
        let (n, bytes) = je.get(&tag).copied().unwrap_or_default();
        println!("  {:?}: {n:>9} Eintraege {:>10.2} MB roh", tag, bytes as f64 / 1e6);
    }
    Ok(())
}

// Ein Paket aus dem Rohmitschnitt (Format s. net::write_raw).
struct Raw {
    s2c: bool,
    ms: f64,
    payload: Vec<u8>,
}

fn raw_lesen(datei: &Path) -> Result<Vec<Raw>, String> {
    let b = std::fs::read(datei).map_err(|e| format!("{}: {e}", datei.display()))?;
    let mut out = Vec::new();
    let mut p = 0;
    while p + 17 <= b.len() {
        let s2c = b[p] == b'S';
        let ms = f64::from_be_bytes(b[p + 5..p + 13].try_into().unwrap());
        let n = u32::from_be_bytes(b[p + 13..p + 17].try_into().unwrap()) as usize;
        let Some(payload) = b.get(p + 17..p + 17 + n) else { break };
        out.push(Raw { s2c, ms, payload: payload.to_vec() });
        p += 17 + n;
    }
    Ok(out)
}

fn check(datei: &Path, raw: &Path) -> Result<(), String> {
    let (r, alle) = lesen(datei)?;
    let raw = raw_lesen(raw)?;
    for (s2c, tag) in [(true, Tag::S), (false, Tag::C)] {
        let rec: Vec<&Entry> = alle.iter().filter(|e| e.tag == tag).collect();
        let roh: Vec<&Raw> = raw.iter().filter(|p| p.s2c == s2c).collect();
        let Some(erstes) = rec.first() else {
            println!("{tag:?}: keine Eintraege in der Aufnahme");
            continue;
        };
        // Die Aufnahme beginnt erst mit der Configuration bzw. beim Einschalten: Anfang im
        // Rohmitschnitt suchen, das gleiche Paket mit der naechstgelegenen Zeit. Kleine Pakete
        // (Bewegung, Schwung) kommen oft mehrmals kurz hintereinander.
        let start_ms = r.header.wall_ms;
        let ziel = start_ms + erstes.t;
        let anfang = roh.iter().enumerate().filter(|(_, p)| p.payload == erstes.data).min_by(|a, b| (a.1.ms - ziel).abs().total_cmp(&(b.1.ms - ziel).abs())).map(|(j, _)| j);
        let Some(mut j) = anfang else {
            println!("{tag:?}: Anfang der Aufnahme nicht im Rohmitschnitt gefunden");
            continue;
        };
        let (mut gleich, mut gekuerzt, mut ausgelassen, mut abweichend) = (0, 0, 0, 0);
        let mut max_dt: f64 = 0.0;
        let mut i = 0;
        while i < rec.len() && j < roh.len() {
            let (e, p) = (rec[i], roh[j]);
            let treffer = if e.data == p.payload {
                gleich += 1;
                true
            } else if let Scrubbed::Strip(a, b) = scrub_c2s_play(&p.payload).filter_c2s(s2c) {
                if [a, &[0], b].concat() == e.data {
                    gekuerzt += 1;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if treffer {
                max_dt = max_dt.max((start_ms + e.t - p.ms).abs());
                i += 1;
                j += 1;
            } else if !s2c && scrub_c2s_play(&p.payload) == Scrubbed::Drop {
                ausgelassen += 1;
                j += 1;
            } else {
                abweichend += 1;
                if abweichend <= 5 {
                    println!("{tag:?}: Abweichung bei Eintrag {i} ({} Bytes) / Rohpaket {j} ({} Bytes)", e.data.len(), p.payload.len());
                }
                i += 1;
                j += 1;
            }
        }
        let fehlen = rec.len() - i;
        println!("{tag:?}: {} in der Aufnahme: {gleich} gleich, {gekuerzt} ohne Signatur, {ausgelassen} bewusst ausgelassen, {abweichend} abweichend, {fehlen} ohne Gegenstueck; groesste Zeitabweichung {max_dt:.1} ms", rec.len());
    }
    Ok(())
}

trait FilterC2s<'a> {
    fn filter_c2s(self, s2c: bool) -> Scrubbed<'a>;
}

impl<'a> FilterC2s<'a> for Scrubbed<'a> {
    // Gekuerzt wird nur an den Server, nie vom Server.
    fn filter_c2s(self, s2c: bool) -> Self {
        if s2c {
            Scrubbed::Keep
        } else {
            self
        }
    }
}
