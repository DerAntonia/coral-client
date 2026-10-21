// Dekodierung des "Chunk Data and Update Light"-Packets (clientbound play).
//
// Die minecraft.wiki dokumentiert 776; unsere Zielversion ist 774. Das Format ist hier
// nach der etablierten Protokoll-Spezifikation implementiert und wird empirisch gegen
// den echten Server geprueft: parse_chunk_data() liest die Sections so lange, bis das
// Data-Feld exakt aufgebraucht ist. Bleibt am Ende ein unlesbarer Rest, behalten wir die
// Sections davor und melden den Rest (tail_unreadable): Vanilla liest nur so viele Sections,
// wie die Dimension hoch ist, und uebergeht alles dahinter -- manche Server (minemen.club)
// haengen dort Muell an. Wie viele davon echt sind, weiss erst die App (Dimensionshoehe).

use crate::buffer::{PacketReader, ReadError};

pub const SECTION_VOLUME: usize = 16 * 16 * 16;

// Block-State-ID 0 ist in modernen Versionen immer minecraft:air.
pub const STATE_ID_AIR: u32 = 0;

pub struct ChunkData {
    pub x: i32,
    pub z: i32,
    // Von unten nach oben; jede Section hat SECTION_VOLUME Block-State-IDs.
    pub sections: Vec<ChunkSectionData>,
    // Die Schilder unter den Block-Entities der Spalte (Weltkoordinaten + Text). Alle anderen
    // Block-Entities werden gelesen und verworfen.
    pub signs: Vec<((i32, i32, i32), crate::sign::SignText)>,
    // Platzierte Koepfe mit Profil (Spielerkoepfe), s. item::head_profile_from_nbt.
    pub heads: Vec<((i32, i32, i32), crate::item::HeadProfile)>,
    // Himmelslicht des Servers (s. LightData). None, wenn davor etwas nicht lesbar war.
    pub light: Option<LightData>,
    // Das Data-Feld ging nicht in ganzen Sections auf; `sections` endet vor dem Rest.
    pub tail_unreadable: bool,
}

// Himmelslicht aus "Chunk Data and Update Light" bzw. "Update Light". Ebene i ist die Section
// (unterste - 1 + i): eine unter und eine ueber der Welt kommen dazu. Das Blocklicht des
// Servers wird nur ueberlesen -- das rechnet der Client selbst, farbig (s. world::light).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightData {
    // Ebenen mit Daten (je 2048 Byte, zwei Werte je Byte, niedriges Halbbyte zuerst).
    pub sky: Vec<(usize, Vec<u8>)>,
    // Ebenen, die ganz dunkel sind.
    pub sky_empty: Vec<usize>,
}

fn read_bitset(r: &mut PacketReader<'_>) -> Result<Vec<usize>, ReadError> {
    let n = r.read_varint()?;
    if !(0..=64).contains(&n) {
        return Err(ReadError);
    }
    let mut bits = Vec::new();
    for word in 0..n as usize {
        let v = r.read_i64()? as u64;
        for b in 0..64 {
            if v >> b & 1 == 1 {
                bits.push(word * 64 + b);
            }
        }
    }
    Ok(bits)
}

// Die Lichtdaten am Ende beider Pakete: vier Masken, dann die Himmels- und Blocklicht-Felder.
pub fn read_light_data(r: &mut PacketReader<'_>) -> Result<LightData, ReadError> {
    let sky_mask = read_bitset(r)?;
    let _block_mask = read_bitset(r)?;
    let sky_empty = read_bitset(r)?;
    let _block_empty = read_bitset(r)?;
    let n = r.read_varint()?;
    if n < 0 || n as usize != sky_mask.len() {
        return Err(ReadError);
    }
    let mut sky = Vec::with_capacity(n as usize);
    for &ebene in &sky_mask {
        let len = r.read_varint()?;
        if len != 2048 {
            return Err(ReadError);
        }
        sky.push((ebene, r.read_bytes(2048)?.to_vec()));
    }
    // Blocklicht: nur darueber hinweg.
    let n = r.read_varint()?;
    for _ in 0..n.clamp(0, 64) {
        let len = r.read_varint()?;
        r.read_bytes(len.clamp(0, 2048) as usize)?;
    }
    Ok(LightData { sky, sky_empty })
}

// "Update Light" (Play, 0x2F): Chunk-X/Z als VarInt, dann dieselben Lichtdaten.
pub fn parse_light_update(payload: &[u8]) -> Result<(i32, i32, LightData), ReadError> {
    let mut r = PacketReader::new(payload);
    let _packet_id = r.read_varint()?;
    let x = r.read_varint()?;
    let z = r.read_varint()?;
    Ok((x, z, read_light_data(&mut r)?))
}

pub struct ChunkSectionData {
    pub block_count: i16,
    pub block_states: Vec<u32>,
    // 4x4x4-Biom-Raster (64 Eintraege, Registry-ID aus der Biom-Registry, s.
    // packets::read_registry_data) -- bereits vom Paletten-Format entpackt, ein Eintrag pro
    // 4x4x4-Unterwuerfel dieser Section. Index = (y*4+z)*4+x wie bei den Block-States, nur
    // mit 4 statt 16 Zellen je Achse.
    pub biomes: Vec<u32>,
}

impl ChunkSectionData {
    pub fn is_empty(&self) -> bool {
        self.block_count == 0
    }
}

// Diagnose-Hilfe: laeuft den Header ab und berichtet, was gefunden wurde, statt nur
// Err zu liefern. Damit laesst sich das Layout gegen den echten Server verifizieren.
pub fn debug_chunk_layout(payload: &[u8]) -> String {
    let mut out = String::new();
    let mut r = PacketReader::new(payload);
    macro_rules! bail {
        ($e:expr) => {
            match $e {
                Ok(v) => v,
                Err(_) => return out + " <- ABBRUCH: Daten zu Ende",
            }
        };
    }

    let _id = bail!(r.read_varint());
    let x = bail!(r.read_i32());
    let z = bail!(r.read_i32());
    out += &format!("Chunk ({x}, {z})\n");

    let hm_count = bail!(r.read_varint());
    out += &format!("Heightmaps: {hm_count}\n");
    for i in 0..hm_count {
        let t = bail!(r.read_varint());
        let len = bail!(r.read_varint());
        out += &format!("  [{i}] Typ={t}, {len} Longs\n");
        for _ in 0..len {
            bail!(r.read_i64());
        }
    }

    let data_len = bail!(r.read_varint()) as usize;
    out += &format!("Data-Feld: {data_len} Byte (Rest im Packet: {})\n", r.remaining().len());
    let data = bail!(r.read_bytes(data_len));

    // Erste Section von Hand aufschluesseln.
    let mut d = PacketReader::new(data);
    let hex: String = data.iter().take(24).map(|b| format!("{b:02x} ")).collect();
    out += &format!("Data beginnt mit: {hex}\n");

    let bc = bail!(d.read_i16());
    let bpe = bail!(d.read_u8());
    out += &format!("Section 0: block_count={bc}, bits_per_entry={bpe}\n");

    if bpe == 0 {
        let v = bail!(d.read_varint());
        out += &format!("  Single-valued, Wert={v}\n");
        let n = bail!(d.read_varint());
        out += &format!("  danach VarInt={n} (erwartet: Long-Array-Laenge 0)\n");
    } else if bpe < 9 {
        let plen = bail!(d.read_varint());
        out += &format!("  Indirect, Palette mit {plen} Eintraegen\n");
        for _ in 0..plen {
            bail!(d.read_varint());
        }
        let n = bail!(d.read_varint());
        let expected = 4096usize.div_ceil(64 / bpe as usize);
        out += &format!("  Long-Array: {n} (erwartet {expected}) {}\n", if n as usize == expected { "OK" } else { "<-- PASST NICHT" });
    } else {
        let n = bail!(d.read_varint());
        let expected = 4096usize.div_ceil(64 / bpe as usize);
        out +=
            &format!("  Direct, Long-Array: {n} (erwartet {expected}) {}\n", if n as usize == expected { "OK" } else { "<-- PASST NICHT" });
    }

    out
}

pub fn parse_chunk_data(payload: &[u8]) -> Result<ChunkData, ReadError> {
    let mut r = PacketReader::new(payload);
    let _packet_id = r.read_varint()?;

    let x = r.read_i32()?;
    let z = r.read_i32()?;

    // Heightmaps (seit 1.21.5 ein Array statt NBT): Anzahl, dann je Typ + Long-Array.
    // Inhalt interessiert uns nicht -- wir muessen nur exakt darueber hinweg.
    let heightmap_count = r.read_varint()?;
    for _ in 0..heightmap_count {
        let _type = r.read_varint()?;
        let len = r.read_varint()?;
        for _ in 0..len {
            r.read_i64()?;
        }
    }

    // Data: Prefixed Array of Byte -- enthaelt die Sections hintereinander.
    let data_len = r.read_varint()? as usize;
    let data = r.read_bytes(data_len)?;

    let (sections, tail_unreadable) = parse_sections(data);
    if sections.is_empty() && tail_unreadable {
        return Err(ReadError);
    }

    // Block Entities: je Eintrag gepacktes XZ (x << 4 | z), Hoehe, Typ und NBT. Ein Fehler
    // hier kostet nur die Schilder (und das Licht dahinter), nicht die Spalte.
    let mut signs = Vec::new();
    let mut heads = Vec::new();
    let mut lies = || -> Result<(), ReadError> {
        let n = r.read_varint()?;
        for _ in 0..n.clamp(0, 65536) {
            let xz = r.read_u8()?;
            let y = r.read_i16()? as i32;
            let _typ = r.read_varint()?;
            let data = crate::nbt::read_root(&mut r)?;
            let pos = (x * 16 + (xz >> 4) as i32, y, z * 16 + (xz & 15) as i32);
            if let Some(text) = data.as_ref().and_then(crate::sign::sign_from_nbt) {
                signs.push((pos, text));
            } else if let Some(profil) = data.as_ref().and_then(crate::item::head_profile_from_nbt) {
                heads.push((pos, profil));
            }
        }
        Ok(())
    };
    let light = match lies() {
        Ok(()) => read_light_data(&mut r).ok(),
        Err(_) => None,
    };

    Ok(ChunkData { x, z, sections, signs, heads, light, tail_unreadable })
}

// Liest Sections, bis `data` exakt aufgebraucht ist. Das ist gleichzeitig die
// Format-Verifikation: ein Off-by-one im Bit-Unpacking wuerde hier auffliegen. true im
// zweiten Wert = der Rest war keine ganze Section mehr (s. Modulkopf).
fn parse_sections(data: &[u8]) -> (Vec<ChunkSectionData>, bool) {
    let mut r = PacketReader::new(data);
    let mut sections = Vec::new();
    let eine = |r: &mut PacketReader<'_>| -> Result<ChunkSectionData, ReadError> {
        let block_count = r.read_i16()?;
        let block_states = read_paletted_container(r, SECTION_VOLUME, PaletteKind::BlockStates)?;
        let biomes = read_paletted_container(r, 64, PaletteKind::Biomes)?;
        Ok(ChunkSectionData { block_count, block_states, biomes })
    };
    while !r.is_exhausted() {
        match eine(&mut r) {
            Ok(s) => sections.push(s),
            Err(_) => return (sections, true),
        }
    }
    (sections, false)
}

#[derive(Clone, Copy)]
enum PaletteKind {
    BlockStates,
    Biomes,
}

impl PaletteKind {
    // Ab wie vielen Bits pro Eintrag auf "Direct" (rohe Registry-IDs ohne Palette)
    // umgeschaltet wird. Darunter (und ueber 0) gilt "Indirect".
    fn direct_threshold(self) -> u8 {
        match self {
            PaletteKind::BlockStates => 9,
            PaletteKind::Biomes => 4,
        }
    }
}

fn read_paletted_container(r: &mut PacketReader<'_>, entry_count: usize, kind: PaletteKind) -> Result<Vec<u32>, ReadError> {
    let bits_per_entry = r.read_u8()?;

    // Single valued: genau 0 Bit -- ein einziger Wert fuer den ganzen Container,
    // ohne jedes Long-Array.
    if bits_per_entry == 0 {
        let value = r.read_varint()? as u32;
        return Ok(vec![value; entry_count]);
    }

    // Indirect: Palette aus Registry-IDs, die Datenwerte sind Indizes darin.
    // Direct: keine Palette, die Datenwerte sind selbst die Registry-IDs.
    let palette: Option<Vec<u32>> = if bits_per_entry < kind.direct_threshold() {
        let len = r.read_varint()?;
        let mut p = Vec::with_capacity(len.max(0) as usize);
        for _ in 0..len {
            p.push(r.read_varint()? as u32);
        }
        Some(p)
    } else {
        None
    };

    // Kein VarInt-Laengenpraefix (anders als in aelteren Versionen): Die Anzahl der Longs
    // ergibt sich aus bits_per_entry und der Eintragszahl. Empirisch am Byte-Strom des
    // echten 1.21.11-Servers verifiziert.
    let entries_per_long = 64 / bits_per_entry as usize;
    if entries_per_long == 0 {
        return Err(ReadError);
    }
    let long_count = entry_count.div_ceil(entries_per_long);
    let mut longs = Vec::with_capacity(long_count);
    for _ in 0..long_count {
        longs.push(r.read_i64()? as u64);
    }

    let values = unpack(&longs, bits_per_entry, entry_count);

    match palette {
        Some(p) => values.into_iter().map(|idx| p.get(idx as usize).copied().ok_or(ReadError)).collect(),
        None => Ok(values),
    }
}

// Entpackt bit-gepackte Eintraege. Seit 1.16 gilt: Eintraege ueberspannen KEINE
// Long-Grenzen -- die obersten (64 % bits_per_entry) Bits jedes Longs sind Padding.
fn unpack(longs: &[u64], bits_per_entry: u8, entry_count: usize) -> Vec<u32> {
    let entries_per_long = 64 / bits_per_entry as usize;
    let mask = (1u64 << bits_per_entry) - 1;
    let mut out = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        let long_index = i / entries_per_long;
        let bit_offset = (i % entries_per_long) * bits_per_entry as usize;
        out.push(((longs[long_index] >> bit_offset) & mask) as u32);
    }
    out
}

#[cfg(test)]
mod light_tests {
    use super::*;
    use crate::buffer::PacketWriter;

    // Ein mitgeschnittenes Chunk-Paket pruefen (pktproxy CORAL_DUMP_CHUNK):
    // CORAL_CHUNK_DUMP=<datei> cargo test -p protocol chunk_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn chunk_dump() {
        let pfad = std::env::var("CORAL_CHUNK_DUMP").expect("CORAL_CHUNK_DUMP setzen");
        let payload = std::fs::read(pfad).unwrap();
        match parse_chunk_data(&payload) {
            Ok(c) => println!("ok: ({}, {}) {} Sections, Rest unlesbar: {}", c.x, c.z, c.sections.len(), c.tail_unreadable),
            Err(e) => println!("FEHLER: {e:?}\n{}", debug_chunk_layout(&payload)),
        }
    }

    #[test]
    fn himmelslicht_masken_und_felder() {
        let mut w = PacketWriter::new();
        w.write_varint(0x2F).write_varint(-3).write_varint(7);
        // Himmel: Ebenen 1 und 65 (zweites Wort), Block: Ebene 2, leer: Ebene 0.
        w.write_varint(2).write_i64(0b10).write_i64(0b10);
        w.write_varint(1).write_i64(0b100);
        w.write_varint(1).write_i64(0b1);
        w.write_varint(0);
        w.write_varint(2);
        w.write_varint(2048).write_raw_bytes(&[0xFF; 2048]);
        w.write_varint(2048).write_raw_bytes(&[0x21; 2048]);
        w.write_varint(1);
        w.write_varint(2048).write_raw_bytes(&[0; 2048]);
        let (x, z, l) = parse_light_update(&w.into_bytes()).unwrap();
        assert_eq!((x, z), (-3, 7));
        assert_eq!(l.sky.iter().map(|(e, d)| (*e, d[0])).collect::<Vec<_>>(), vec![(1, 0xFF), (65, 0x21)]);
        assert_eq!(l.sky_empty, vec![0]);
    }
}
