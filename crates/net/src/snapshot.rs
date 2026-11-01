// coral-record: der Snapshot einer Aufnahme ab der Mitte (s. record::Tag::Z). Die App erzeugt
// aus ihrem Zustand dieselben dekodierten Ereignisse, die beim Join kaemen; hier werden sie
// kodiert und in der Wiedergabe wieder ausgepackt, in den gleichen Weg wie live.
//
// Das Format ist das der Ereignis-Typen dieses Clients (postcard ueber serde). Aendert sich ein
// Typ, laesst sich ein alter Snapshot nicht mehr lesen; die Client-Version steht im Kopf.

use crate::ClientEvent;

#[must_use] 
pub fn snapshot_encode(e: &ClientEvent) -> Option<Vec<u8>> {
    postcard::to_allocvec(e).ok()
}

#[must_use] 
pub fn snapshot_decode(b: &[u8]) -> Option<ClientEvent> {
    postcard::from_bytes(b).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_hin_und_zurueck() {
        let states: Vec<u32> = (0..4096).map(|i| if i < 256 { 1 } else if i % 7 == 0 { 9 } else { 0 }).collect();
        let chunk = protocol::chunk::ChunkData {
            x: -3,
            z: 12,
            sections: vec![protocol::chunk::ChunkSectionData { block_count: 900, block_states: states.clone(), biomes: vec![2; 64] }],
            signs: Vec::new(),
            heads: Vec::new(),
            light: Some(protocol::chunk::LightData { sky: vec![(1, vec![0x5a; 2048])], sky_empty: vec![0] }),
            tail_unreadable: false,
        };
        let b = snapshot_encode(&ClientEvent::Chunk(chunk)).unwrap();
        // Palette mit zwei Bits je Block statt 4096 Zahlen.
        assert!(b.len() < 4000, "{} Bytes", b.len());
        let Some(ClientEvent::Chunk(c)) = snapshot_decode(&b) else { panic!("kein Chunk") };
        assert_eq!((c.x, c.z, c.sections[0].block_count), (-3, 12, 900));
        assert_eq!(c.sections[0].block_states, states);
        assert_eq!(c.sections[0].biomes, vec![2; 64]);
        assert_eq!(c.light.unwrap().sky_empty, vec![0]);
    }
}
