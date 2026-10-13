// Liest Zip-Archive so, wie Javas java.util.zip.ZipFile es tut -- fuer Server-Resourcepacks.
//
// Viele Server "schuetzen" ihr Pack, indem sie das Archiv absichtlich verbiegen: falsche
// Endsatz-Zaehler, Muell-Signaturen am Anfang und Ende, Eintraege mit Schraegstrich am Namen
// ("pack.mcmeta/"), Tarn-Ordner. Ein gewoehnlicher Zip-Leser (auch der zip-Crate, unzip,
// Pythons zipfile) gibt dabei auf. Minecraft liest das Pack trotzdem, weil Java nur wenige
// Dinge prueft -- und genau diese wenigen Regeln bildet dieser Leser nach:
//
//   - Der Endsatz (END) wird von hinten gesucht. Passt seine Kommentarlaenge nicht zur
//     Dateilaenge, gilt er nur, wenn an den daraus berechneten Stellen wirklich das
//     Zentralverzeichnis (CEN) und ein lokaler Kopf (LOC) beginnen; sonst weitersuchen.
//   - Massgeblich sind die Angaben im Zentralverzeichnis (Name, Methode, Groesse, Versatz).
//     Vom lokalen Kopf zaehlen nur Signatur und die Laengen von Name und Extrafeld.
//   - Gleiche Namen: der spaetere Eintrag gewinnt (Javas Hashkette fuegt vorn ein).
//   - Ein Name ohne Treffer wird noch einmal mit angehaengtem "/" gesucht.
//
// Das ganze Archiv liegt im Speicher (Server-Packs sind ein paar MB bis gut 100 MB).

use std::collections::HashMap;
use std::io::Read;

const END_SIG: &[u8; 4] = b"PK\x05\x06";
const CEN_SIG: &[u8; 4] = b"PK\x01\x02";
const LOC_SIG: &[u8; 4] = b"PK\x03\x04";
const END_LEN: usize = 22;
const CEN_LEN: usize = 46;
const LOC_LEN: usize = 30;

#[derive(Clone, Copy)]
struct Entry {
    method: u16,
    compressed: u32,
    size: u32,
    // Versatz des lokalen Kopfes relativ zum Archivanfang (`base`).
    offset: u32,
}

pub struct JavaZip {
    data: Vec<u8>,
    // Wo das Archiv in `data` beginnt -- bei vorangestelltem Muell nicht 0.
    base: usize,
    entries: HashMap<String, Entry>,
    // Alle Namen in Verzeichnis-Reihenfolge (fuer Auflistungen), ohne Doppelte.
    names: Vec<String>,
}

fn u16_at(d: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(p..p + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(p..p + 4)?.try_into().ok()?))
}

impl JavaZip {
    pub fn open(path: &std::path::Path) -> std::io::Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> std::io::Result<Self> {
        let kaputt = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
        let (end, cen_len, cen_off) = find_end(&data).ok_or_else(|| kaputt("kein gueltiger Zip-Endsatz"))?;
        let cen_pos = end - cen_len;
        let base = cen_pos - cen_off;

        let mut entries = HashMap::new();
        let mut names = Vec::new();
        let mut p = cen_pos;
        while p + CEN_LEN <= end {
            if &data[p..p + 4] != CEN_SIG {
                return Err(kaputt("Zentralverzeichnis kaputt"));
            }
            let field = |o: usize| u16_at(&data, p + o).unwrap_or(0) as usize;
            let method = field(10) as u16;
            let compressed = u32_at(&data, p + 20).unwrap_or(0);
            let size = u32_at(&data, p + 24).unwrap_or(0);
            let (name_len, extra_len, comment_len) = (field(28), field(30), field(32));
            let offset = u32_at(&data, p + 42).unwrap_or(0);
            let name_end = p + CEN_LEN + name_len;
            if name_end > end {
                return Err(kaputt("Name ragt aus dem Zentralverzeichnis"));
            }
            // Java liest Namen als UTF-8; nicht dekodierbare Eintraege sind fuer Minecraft
            // ohnehin unbrauchbar (keine gueltige Ressourcen-ID).
            if let Ok(name) = std::str::from_utf8(&data[p + CEN_LEN..name_end]) {
                if matches!(method, 0 | 8) && entries.insert(name.to_string(), Entry { method, compressed, size, offset }).is_none() {
                    names.push(name.to_string());
                }
            }
            p = name_end + extra_len + comment_len;
        }
        Ok(Self { data, base, entries, names })
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    fn entry(&self, name: &str) -> Option<Entry> {
        self.entries.get(name).or_else(|| self.entries.get(&format!("{name}/"))).copied()
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.entry(name)?;
        let loc = self.base.checked_add(e.offset as usize)?;
        if self.data.get(loc..loc + 4)? != LOC_SIG {
            return None;
        }
        let start = loc + LOC_LEN + u16_at(&self.data, loc + 26)? as usize + u16_at(&self.data, loc + 28)? as usize;
        let raw = self.data.get(start..start.checked_add(e.compressed as usize)?)?;
        match e.method {
            0 => Some(raw.to_vec()),
            _ => {
                let mut out = Vec::with_capacity(e.size as usize);
                flate2::read::DeflateDecoder::new(raw).read_to_end(&mut out).ok()?;
                Some(out)
            }
        }
    }
}

// Endsatz von hinten suchen (s. Kopfkommentar). Rueckgabe: (Position, CEN-Laenge, CEN-Versatz).
fn find_end(d: &[u8]) -> Option<(usize, usize, usize)> {
    if d.len() < END_LEN {
        return None;
    }
    let tiefste = d.len().saturating_sub(END_LEN + 0xFFFF);
    let mut i = d.len() - END_LEN;
    loop {
        if &d[i..i + 4] == END_SIG {
            let cen_len = u32_at(d, i + 12)? as usize;
            let cen_off = u32_at(d, i + 16)? as usize;
            let comment = u16_at(d, i + 20)? as usize;
            let stimmig = || {
                let Some(cen_pos) = i.checked_sub(cen_len) else { return false };
                let Some(loc_pos) = cen_pos.checked_sub(cen_off) else { return false };
                d.get(cen_pos..cen_pos + 4) == Some(CEN_SIG) && d.get(loc_pos..loc_pos + 4) == Some(LOC_SIG)
            };
            if (i + END_LEN + comment == d.len() || stimmig()) && cen_len <= i && cen_off <= i - cen_len {
                return Some((i, cen_len, cen_off));
            }
        }
        if i == tiefste {
            return None;
        }
        i -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ein kleines Archiv von Hand: Eintraege (Name, Inhalt), unkomprimiert.
    fn archiv(eintraege: &[(&str, &[u8])], muell_vorn: &[u8], kommentar_luege: bool) -> Vec<u8> {
        let mut d = muell_vorn.to_vec();
        let base = d.len();
        let mut cen = Vec::new();
        for (name, inhalt) in eintraege {
            let off = (d.len() - base) as u32;
            d.extend_from_slice(LOC_SIG);
            d.extend_from_slice(&[0; 22]);
            d.extend_from_slice(&(name.len() as u16).to_le_bytes());
            d.extend_from_slice(&0u16.to_le_bytes());
            d.extend_from_slice(name.as_bytes());
            d.extend_from_slice(inhalt);
            cen.extend_from_slice(CEN_SIG);
            cen.extend_from_slice(&[0; 6]);
            cen.extend_from_slice(&0u16.to_le_bytes()); // Methode: gespeichert
            cen.extend_from_slice(&[0; 8]);
            cen.extend_from_slice(&(inhalt.len() as u32).to_le_bytes());
            cen.extend_from_slice(&(inhalt.len() as u32).to_le_bytes());
            cen.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cen.extend_from_slice(&[0; 12]);
            cen.extend_from_slice(&off.to_le_bytes());
            cen.extend_from_slice(name.as_bytes());
        }
        let cen_off = (d.len() - base) as u32;
        d.extend_from_slice(&cen);
        d.extend_from_slice(END_SIG);
        d.extend_from_slice(&[0; 4]);
        d.extend_from_slice(&999u16.to_le_bytes()); // falsche Zaehler -- Java ignoriert sie
        d.extend_from_slice(&999u16.to_le_bytes());
        d.extend_from_slice(&(cen.len() as u32).to_le_bytes());
        d.extend_from_slice(&cen_off.to_le_bytes());
        d.extend_from_slice(&(if kommentar_luege { 500u16 } else { 0 }).to_le_bytes());
        if kommentar_luege {
            d.extend_from_slice(b"PK\x05\x06PK\x01\x02Muell");
        }
        d
    }

    #[test]
    fn verbogenes_archiv_wie_java() {
        let d = archiv(&[("pack.mcmeta/", b"{}"), ("a.txt", b"alt"), ("a.txt", b"neu")], b"PK\x05\x06PK\x03\x04Muell", true);
        let z = JavaZip::from_bytes(d).expect("Java liest das auch");
        // Schraegstrich-Trick: gefunden ueber den zweiten Versuch mit "/".
        assert_eq!(z.read("pack.mcmeta").as_deref(), Some(&b"{}"[..]));
        // Doppelter Name: der spaetere gewinnt.
        assert_eq!(z.read("a.txt").as_deref(), Some(&b"neu"[..]));
        assert_eq!(z.names().len(), 2);
        assert!(z.read("fehlt").is_none());
    }

    #[test]
    fn gewoehnliches_archiv() {
        let z = JavaZip::from_bytes(archiv(&[("assets/x.json", b"[1]")], b"", false)).unwrap();
        assert_eq!(z.read("assets/x.json").as_deref(), Some(&b"[1]"[..]));
    }
}
