// Gemeinsamer Vertex-Speicher fuer das opake Gelaende aller Spalten.
//
// Frueher hatte jede Spalte ihren eigenen Vertex-Puffer und damit ihren eigenen Zeichenaufruf
// samt Puffer-Bindung -- bei Sichtweite 32 ~1800 Aufrufe je Frame, ~0,8 ms CPU nur fuers
// Aufzeichnen. Liegen alle Spalten in wenigen grossen Puffern, zeichnet EIN indirekter
// Mehrfach-Aufruf je Puffer alles (s. BlockRenderer::prepare_terrain_draws).
//
// Seiten statt eines einzigen Puffers: wgpu erlaubt ohne erhoehte Limits hoechstens 256 MB je
// Puffer, das Gelaende ist bei Sichtweite 32 gut dreimal so gross. Innerhalb einer Seite
// verwaltet eine Freiliste die Bereiche (in Vertices), benachbarte freie Bereiche werden
// zusammengelegt.
//
// Wiederverwendung eines freigegebenen Bereichs ist sicher: queue.write_buffer laeuft in
// Queue-Reihenfolge -- alle schon abgeschickten Frames lesen noch die alten Daten, erst die
// danach abgeschickten sehen die neuen.

use world::TerrainVertex;

const VERTEX_BYTES: u64 = std::mem::size_of::<TerrainVertex>() as u64;

pub struct ArenaPage {
    pub buffer: wgpu::Buffer,
    free: FreeList,
}

// Freie Bereiche (Start, Laenge) einer Seite in Vertices, nach Start sortiert, Nachbarn
// zusammengelegt.
pub(crate) struct FreeList(Vec<(u32, u32)>);

impl FreeList {
    pub(crate) fn new(len: u32) -> Self {
        Self(vec![(0, len)])
    }

    // Erster passender Bereich; gibt seinen Start zurueck.
    pub(crate) fn take(&mut self, len: u32) -> Option<u32> {
        let i = self.0.iter().position(|&(_, l)| l >= len)?;
        let (start, l) = self.0[i];
        if l == len {
            self.0.remove(i);
        } else {
            self.0[i] = (start + len, l - len);
        }
        Some(start)
    }

    pub(crate) fn give(&mut self, start: u32, len: u32) {
        let free = &mut self.0;
        let i = free.partition_point(|&(s, _)| s < start);
        free.insert(i, (start, len));
        // Mit dem Nachfolger, dann mit dem Vorgaenger zusammenlegen.
        if i + 1 < free.len() && free[i].0 + free[i].1 == free[i + 1].0 {
            free[i].1 += free[i + 1].1;
            free.remove(i + 1);
        }
        if i > 0 && free[i - 1].0 + free[i - 1].1 == free[i].0 {
            free[i - 1].1 += free[i].1;
            free.remove(i);
        }
    }
}

pub struct TerrainArena {
    pub pages: Vec<ArenaPage>,
    page_vertices: u32,
}

// Wo die Vertices einer Spalte liegen.
#[derive(Clone, Copy, Debug)]
pub struct ArenaSlot {
    pub page: u32,
    pub start: u32,
    pub len: u32,
}

impl TerrainArena {
    pub fn new(device: &wgpu::Device) -> Self {
        let max = device.limits().max_buffer_size.min(256 << 20);
        Self { pages: Vec::new(), page_vertices: (max / VERTEX_BYTES) as u32 }
    }

    // Platz fuer `vertices` belegen und hochladen. None nur, wenn eine einzelne Spalte
    // groesser als eine ganze Seite waere.
    pub fn insert(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vertices: &[TerrainVertex]) -> Option<ArenaSlot> {
        let len = vertices.len() as u32;
        if len == 0 || len > self.page_vertices {
            return None;
        }
        let slot = match self.find(len) {
            Some(slot) => slot,
            None => {
                self.pages.push(ArenaPage {
                    buffer: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("terrain arena page"),
                        size: self.page_vertices as u64 * VERTEX_BYTES,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    free: FreeList::new(self.page_vertices),
                });
                self.find(len)?
            }
        };
        queue.write_buffer(&self.pages[slot.page as usize].buffer, slot.start as u64 * VERTEX_BYTES, bytemuck::cast_slice(vertices));
        Some(slot)
    }

    // Erster passender freier Bereich ueber alle Seiten.
    fn find(&mut self, len: u32) -> Option<ArenaSlot> {
        self.pages
            .iter_mut()
            .enumerate()
            .find_map(|(page, p)| p.free.take(len).map(|start| ArenaSlot { page: page as u32, start, len }))
    }

    pub fn remove(&mut self, slot: ArenaSlot) {
        if let Some(page) = self.pages.get_mut(slot.page as usize) {
            page.free.give(slot.start, slot.len);
        }
    }

    // Vertices einer Spalte ab `offset` (in Vertices, relativ zum Spaltenanfang) ersetzen.
    pub fn write(&self, queue: &wgpu::Queue, slot: ArenaSlot, offset: u32, vertices: &[TerrainVertex]) {
        if offset as usize + vertices.len() > slot.len as usize {
            return;
        }
        let Some(page) = self.pages.get(slot.page as usize) else { return };
        queue.write_buffer(&page.buffer, (slot.start + offset) as u64 * VERTEX_BYTES, bytemuck::cast_slice(vertices));
    }

    pub fn clear(&mut self) {
        for page in &mut self.pages {
            page.free = FreeList::new(self.page_vertices);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FreeList;

    #[test]
    fn bereiche_ueberlappen_nie_und_fliessen_zurueck() {
        let mut f = FreeList::new(100);
        let a = f.take(30).unwrap();
        let b = f.take(30).unwrap();
        let c = f.take(30).unwrap();
        assert_eq!((a, b, c), (0, 30, 60));
        assert_eq!(f.take(20), None, "nur noch 10 frei");
        // Mitte freigeben: passt genau wieder hinein, mehr nicht.
        f.give(b, 30);
        assert_eq!(f.take(31), None);
        assert_eq!(f.take(30), Some(30));
        // Alles freigeben, in wilder Reihenfolge: am Ende wieder EIN Bereich.
        f.give(60, 30);
        f.give(0, 30);
        f.give(30, 30);
        assert_eq!(f.0, vec![(0, 100)]);
        assert_eq!(f.take(100), Some(0));
    }
}
