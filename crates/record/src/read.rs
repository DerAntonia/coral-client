// Liest eine .crec-Datei (Format s. lib.rs). Mit Index am Ende ist die Datei sauber
// geschlossen; ohne ihn (Absturz) werden die Bloecke ueber ihre Koepfe gefunden, bis zum
// ersten unvollstaendigen.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::{Header, Tag, BLOCK, BLOCK_HEAD, END, INDEX, MAGIC};

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    Format(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Format(s) => write!(f, "kaputte Aufnahme: {s}"),
        }
    }
}

impl From<std::io::Error> for ReadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

fn kaputt<T>(s: impl Into<String>) -> Result<T, ReadError> {
    Err(ReadError::Format(s.into()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub tag: Tag,
    // ms seit Sitzungsstart.
    pub t: f64,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct BlockInfo {
    pub start: f64,
    pub end: f64,
    pub count: u32,
    pub offset: u64,
    raw_len: u32,
    comp_len: u32,
}

pub struct Reader {
    pub header: Header,
    pub blocks: Vec<BlockInfo>,
    // Index vorhanden, die Aufnahme wurde sauber beendet.
    pub complete: bool,
    file: File,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Self, ReadError> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut kopf = [0u8; 10];
        file.read_exact(&mut kopf)?;
        if &kopf[..4] != MAGIC {
            return kaputt("keine .crec-Datei");
        }
        let version = u16::from_le_bytes([kopf[4], kopf[5]]);
        if version != crate::VERSION {
            return kaputt(format!("Format-Version {version} unbekannt"));
        }
        let body_len = u32::from_le_bytes(kopf[6..10].try_into().unwrap()) as usize;
        let mut body = vec![0; body_len];
        file.read_exact(&mut body)?;
        let header = decode_header(&body)?;
        let erster = 10 + body_len as u64;
        let mut r = Self { header, blocks: Vec::new(), complete: false, file };
        if let Some(offsets) = r.index_offsets(len, erster) {
            for o in offsets {
                let b = r.block_head(o, len)?.ok_or_else(|| ReadError::Format(format!("Index zeigt auf {o}, dort ist kein Block")))?;
                r.blocks.push(b);
            }
            r.complete = true;
        } else {
            let mut o = erster;
            while let Some(b) = r.block_head(o, len)? {
                o += (BLOCK_HEAD + b.comp_len as usize) as u64;
                r.blocks.push(b);
            }
        }
        Ok(r)
    }

    // Blockpositionen aus dem Index, None ohne gueltigen Index.
    fn index_offsets(&mut self, len: u64, erster: u64) -> Option<Vec<u64>> {
        if len < erster + 12 {
            return None;
        }
        let mut ende = [0u8; 12];
        self.file.seek(SeekFrom::Start(len - 12)).ok()?;
        self.file.read_exact(&mut ende).ok()?;
        if &ende[8..] != END {
            return None;
        }
        let pos = u64::from_le_bytes(ende[..8].try_into().unwrap());
        if pos < erster || pos + 5 > len - 12 {
            return None;
        }
        self.file.seek(SeekFrom::Start(pos)).ok()?;
        let mut k = [0u8; 5];
        self.file.read_exact(&mut k).ok()?;
        let n = u64::from(u32::from_le_bytes(k[1..5].try_into().unwrap()));
        if k[0] != INDEX || pos + 5 + n * 16 != len - 12 {
            return None;
        }
        let mut daten = vec![0u8; (n * 16) as usize];
        self.file.read_exact(&mut daten).ok()?;
        Some(daten.chunks(16).map(|c| u64::from_le_bytes(c[8..16].try_into().unwrap())).collect())
    }

    // Kopf des Blocks bei `o`. None, wenn dort keiner beginnt oder er nicht ganz in der Datei
    // steht.
    fn block_head(&mut self, o: u64, len: u64) -> Result<Option<BlockInfo>, ReadError> {
        if o + BLOCK_HEAD as u64 > len {
            return Ok(None);
        }
        self.file.seek(SeekFrom::Start(o))?;
        let mut k = [0u8; BLOCK_HEAD];
        self.file.read_exact(&mut k)?;
        if k[0] != BLOCK {
            return Ok(None);
        }
        let f = |a: usize| f64::from_le_bytes(k[a..a + 8].try_into().unwrap());
        let u = |a: usize| u32::from_le_bytes(k[a..a + 4].try_into().unwrap());
        let b = BlockInfo { start: f(1), end: f(9), count: u(17), raw_len: u(21), comp_len: u(25), offset: o };
        if o + (BLOCK_HEAD + b.comp_len as usize) as u64 > len {
            return Ok(None);
        }
        Ok(Some(b))
    }

    // Die Eintraege des Blocks `i`.
    pub fn block(&mut self, i: usize) -> Result<Vec<Entry>, ReadError> {
        let b = self.blocks[i];
        self.file.seek(SeekFrom::Start(b.offset + BLOCK_HEAD as u64))?;
        let mut comp = vec![0u8; b.comp_len as usize];
        self.file.read_exact(&mut comp)?;
        let mut raw = Vec::with_capacity(b.raw_len as usize);
        flate2::read::ZlibDecoder::new(&comp[..]).read_to_end(&mut raw)?;
        let mut out = Vec::with_capacity(b.count as usize);
        let mut p = 0;
        while p < raw.len() {
            let Some(tag) = Tag::from_byte(raw[p]) else { return kaputt(format!("Spur {:#04x} unbekannt", raw[p])) };
            let Some(t) = raw.get(p + 1..p + 9) else { return kaputt("Eintrag abgeschnitten") };
            let t = f64::from_le_bytes(t.try_into().unwrap());
            let Ok((n, q)) = protocol::varint::decode_varint(&raw, p + 9) else { return kaputt("Laenge kaputt") };
            let Some(data) = raw.get(q..q + n as usize) else { return kaputt("Eintrag abgeschnitten") };
            out.push(Entry { tag, t, data: data.to_vec() });
            p = q + n as usize;
        }
        if out.len() != b.count as usize {
            return kaputt(format!("Block bei {}: {} statt {} Eintraege", b.offset, out.len(), b.count));
        }
        Ok(out)
    }

    // Alle Eintraege der Reihe nach.
    pub fn for_each(&mut self, mut f: impl FnMut(Entry)) -> Result<(), ReadError> {
        for i in 0..self.blocks.len() {
            for e in self.block(i)? {
                f(e);
            }
        }
        Ok(())
    }
}

fn decode_header(b: &[u8]) -> Result<Header, ReadError> {
    let mut c = Cursor { b, p: 0 };
    let protocol = i32::from_le_bytes(c.take(4)?.try_into().unwrap());
    let client = c.text()?;
    let server = c.text()?;
    let wall_ms = f64::from_le_bytes(c.take(8)?.try_into().unwrap());
    let uuid = c.take(16)?.try_into().unwrap();
    let name = c.text()?;
    let from_join = c.take(1)?[0] != 0;
    Ok(Header { protocol, client, server, wall_ms, uuid, name, from_join })
}

struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ReadError> {
        let s = self.b.get(self.p..self.p + n).ok_or_else(|| ReadError::Format("Kopf abgeschnitten".into()))?;
        self.p += n;
        Ok(s)
    }

    fn text(&mut self) -> Result<String, ReadError> {
        let n = u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}
