// Kompaktes Vertex-Format fuer das OPAKE Gelaende -- das, was im Performance-Pfad wirklich
// auf der GPU liegt und jeden Frame gelesen wird.
//
// Der Mesher gibt BlockVertex aus (32 Byte, je Flaeche sechs Vertices = zwei Dreiecke ohne
// Index). Bei Sichtweite 32 waren das ~1,5 GB Vertex-Daten, und der Gelaende-Draw war durch
// die schiere Menge begrenzt (Messung: ohne Alpha-Test nicht schneller, s.
// benchmarks/performance-pfad.md). Hier wird jede Flaeche auf VIER Vertices zu je 20 Byte
// gepackt; die Dreiecke kommen aus einem gemeinsamen, festen Index-Puffer (je Flaeche
// 0,1,2, 0,2,3 -- s. render::BlockRenderer). 80 statt 192 Byte je Flaeche, und ein Drittel
// weniger Vertex-Shader-Aufrufe.
//
// Position relativ zur Section (16er-Wuerfel) der Flaeche als u16 in 1/2048 Block, um 8 Bloecke
// verschoben (Bereich -8..24, Modelle ragen ueber ihre Zelle hinaus): so bleiben die Versaetze
// von 1/1024 Block gegen Z-Fighting (s. mesher::WALL_ZFIGHT_EPS) exakt, ein 16-Bit-Raster ueber
// die ganze Welthoehe haette sie verschluckt. Welche Section, steht im vierten u16. Farbe
// (Richtungs-Shade * AO * Biom, alles 0..1) als RGBA8, UV als u16 in 1/65536 des Atlas:
// Kachelgrenzen sind Zweierpotenz-Brueche und damit exakt. 16 Byte je Vertex.
//
// Bezugspunkt aller Positionen ist die Spaltenecke auf Hoehe min_y (Unterkante der Welt in
// dieser Dimension) -- "spalten-lokal" heisst hier also auch in y relativ zu min_y.

use crate::mesher::BlockVertex;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainVertex {
    // x, y, z relativ zur Section-Ecke (s. POS_SCALE/POS_BIAS). [3]: Section-Index ab min_y in
    // den unteren 5 Bit, darueber die oberen 11 Bit des Lichts (s. LIGHT_*).
    pub pos: [u16; 4],
    // RGB 0..255; Alpha = untere 8 Bit des Lichts.
    pub color: [u8; 4],
    // Atlas-UV in 1/65536 (1.0 wird zu 65535 -- die rechte bzw. untere Atlaskante).
    pub uv: [u16; 2],
}

impl TerrainVertex {
    pub const POS_SCALE: f32 = 2048.0;
    pub const POS_BIAS: f32 = 8.0;
    // Licht fuer den Shader-Pfad (s. world::light::pack_vertex_light), 19 Bit in den sonst
    // ungenutzten Bits: Himmel 0..15 in Bit 0..3, Blocklicht R/G/B in halben Stufen (0..30) in
    // Bit 4..8, 9..13, 14..18. Untere 8 Bit im Farb-Alpha, obere 11 ueber dem Section-Index.
    // Der PvP-Pfad liest davon nichts. Ohne Lichtberechnung: voller Himmel, kein Blocklicht.
    pub const SECTION_MASK: u16 = 31;
    pub const NO_LIGHT: u32 = 15;
    // Farb-Alpha ohne Licht (PvP-Pfad) fuer "hart abschneiden" (s.
    // BlockVertex::with_cutout_mark) -- ohne Licht steht dort sonst immer NO_LIGHT. Eine
    // Lichtberechnung (Shader-Pfad) ueberschreibt es; dort gibt es das Abschneiden nicht.
    pub const CUTOUT_MARK: u8 = 255;

    pub fn with_light(mut self, light: u32) -> Self {
        self.color[3] = (light & 0xFF) as u8;
        self.pos[3] = (self.pos[3] & Self::SECTION_MASK) | (((light >> 8) & 0x7FF) << 5) as u16;
        self
    }

    pub fn light(&self) -> u32 {
        self.color[3] as u32 | ((self.pos[3] >> 5) as u32) << 8
    }

    // `section`: Index der Section, zu der die Flaeche gehoert; `min_y`: Unterkante der Spalte.
    pub fn from_block(v: &BlockVertex, section: u8, min_y: i32) -> Self {
        let c = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        let t = |x: f32| (x.clamp(0.0, 1.0) * 65536.0).round().min(65535.0) as u16;
        let base_y = (min_y + section as i32 * 16) as f32;
        let q = |x: f32| ((x + Self::POS_BIAS) * Self::POS_SCALE).round().clamp(0.0, 65535.0) as u16;
        let [x, y, z] = v.position;
        let farbe = v.plain_color();
        let mut out = Self { pos: [q(x), q(y - base_y), q(z), section as u16], color: [c(farbe[0]), c(farbe[1]), c(farbe[2]), 0], uv: [t(v.uv[0]), t(v.uv[1])] }
            .with_light(Self::NO_LIGHT);
        if v.color[0] < -0.5 {
            out.color[3] = Self::CUTOUT_MARK;
        }
        out
    }

    // Spalten-lokale Position (x/z ab Spaltenecke, y ab min_y), wie der Shader sie rechnet.
    pub fn position(&self) -> [f32; 3] {
        let d = |q: u16| q as f32 / Self::POS_SCALE - Self::POS_BIAS;
        [d(self.pos[0]), d(self.pos[1]) + self.section() as f32 * 16.0, d(self.pos[2])]
    }

    pub fn section(&self) -> u8 {
        (self.pos[3] & Self::SECTION_MASK) as u8
    }
}

// Eine Flaeche, deren Eckabdunklung eingeblendet wird (s. mesher::AoFadeQuad), in diesem
// Format: `start` zeigt in den gepackten Puffer.
pub struct TerrainFadeQuad {
    pub start: u32,
    pub before: Vec<TerrainVertex>,
    pub after: Vec<TerrainVertex>,
}

// Packt Mesher-Ausgabe auf vier Vertices je Flaeche. Sechs Vertices im Muster a,b,c,a,c,d
// werden zu a,b,c,d; alles andere (ein einzelnes Dreieck) zu a,b,c,c -- das zweite Dreieck
// des festen Index-Musters ist dann entartet und zeichnet nichts. So passt JEDE Dreiecksliste
// auf den gemeinsamen Index-Puffer.
//
// `starts`: Vertex-Indizes in `verts`, die auf den gepackten Puffer umgerechnet werden (die
// Anfaenge eingeblendeter Flaechen). Ein Index, der nicht auf einem Flaechenanfang liegt,
// wird u32::MAX.
// `min_y`: Unterkante der Spalte; jede Flaeche wird relativ zu ihrer Section gespeichert (s.
// section_of).
pub fn pack_terrain(verts: &[BlockVertex], starts: &mut [u32], min_y: i32) -> Vec<TerrainVertex> {
    pack_terrain_lit(verts, starts, min_y, None)
}

// Licht je Ecke fuer den Shader-Pfad: (Position spalten-lokal mit absolutem y, Normale aus der
// Flaeche heraus, Mitte der Flaeche, beidseitig?) -> gepacktes Licht. Beidseitig sind schraege Flaechen
// (Pflanzen), deren Wicklung nichts ueber "vorn" sagt.
pub type VertexLight<'a> = &'a dyn Fn([f32; 3], [f32; 3], [f32; 3], bool) -> u32;

// Wie pack_terrain, mit Licht je Vertex (s. VertexLight).
pub fn pack_terrain_lit(verts: &[BlockVertex], starts: &mut [u32], min_y: i32, light: Option<VertexLight>) -> Vec<TerrainVertex> {
    let mut out = Vec::with_capacity(verts.len() / 6 * 4 + 4);
    let mut order: Vec<usize> = (0..starts.len()).collect();
    order.sort_unstable_by_key(|&k| starts[k]);
    let mut next = 0;
    let mut i = 0;
    while i + 3 <= verts.len() {
        while next < order.len() && (starts[order[next]] as usize) < i {
            starts[order[next]] = u32::MAX;
            next += 1;
        }
        while next < order.len() && starts[order[next]] as usize == i {
            starts[order[next]] = out.len() as u32;
            next += 1;
        }
        let q = &verts[i..(i + 6).min(verts.len())];
        let same = |a: &BlockVertex, b: &BlockVertex| bytemuck::bytes_of(a) == bytemuck::bytes_of(b);
        let ecken: [&BlockVertex; 4] = if q.len() == 6 && same(&q[3], &q[0]) && same(&q[4], &q[2]) {
            i += 6;
            [&q[0], &q[1], &q[2], &q[5]]
        } else {
            i += 3;
            [&q[0], &q[1], &q[2], &q[2]]
        };
        let positionen = ecken.map(|v| v.position);
        let group = face_group(&positionen).0;
        let section = section_of(&positionen, group, min_y);
        match light {
            None => out.extend(ecken.map(|v| TerrainVertex::from_block(v, section, min_y))),
            Some(licht) => {
                let (normal, beidseitig) = match group {
                    0 => ([1.0, 0.0, 0.0], false),
                    1 => ([-1.0, 0.0, 0.0], false),
                    2 => ([0.0, 1.0, 0.0], false),
                    3 => ([0.0, -1.0, 0.0], false),
                    4 => ([0.0, 0.0, 1.0], false),
                    5 => ([0.0, 0.0, -1.0], false),
                    _ => (quad_normal(&positionen), true),
                };
                let mitte = [0, 1, 2].map(|k| positionen.iter().map(|q| q[k]).sum::<f32>() / 4.0);
                out.extend(ecken.map(|v| TerrainVertex::from_block(v, section, min_y).with_light(licht(v.position, normal, mitte, beidseitig))));
            }
        }
    }
    for &k in &order[next..] {
        starts[k] = u32::MAX;
    }
    out
}

// Flaechen einer Spalte nach Richtung gruppiert (s. group_by_direction): Index 0..6 = +X, -X,
// +Y, -Y, +Z, -Z, dann alles Schraege (Pflanzen, gedrehte Modelle), das immer gezeichnet wird.
pub const FACE_GROUPS: usize = 7;

// Ein Abschnitt gleicher Richtung im gepackten Puffer, in Flaechen. `plane_min/max`: Lage
// der Flaechen auf ihrer Achse (spalten-lokal wie die Positionen) -- eine +X-Flaeche ist nur
// sichtbar, wenn das Auge auf ihrer Achse GROESSER ist als ihre Ebene, also braucht eine
// ganze Gruppe nur gezeichnet zu werden, wenn das Auge jenseits ihrer kleinsten Ebene steht
// (bei -X: diesseits ihrer groessten).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceGroup {
    pub first_quad: u32,
    pub quads: u32,
    pub plane_min: f32,
    pub plane_max: f32,
}

impl FaceGroup {
    // Kann von `eye` (spalten-lokal) aus irgendeine Flaeche dieser Gruppe ihre Vorderseite
    // zeigen? Gruppe 6 (schraeg) immer.
    pub fn faces_eye(&self, group: usize, eye: [f32; 3]) -> bool {
        match group {
            0 => eye[0] > self.plane_min,
            1 => eye[0] < self.plane_max,
            2 => eye[1] > self.plane_min,
            3 => eye[1] < self.plane_max,
            4 => eye[2] > self.plane_min,
            5 => eye[2] < self.plane_max,
            _ => true,
        }
    }
}

// Richtung einer Flaeche aus ihrem ersten Dreieck (Normale (b-a)x(c-a) zeigt nach aussen, s.
// mesher-Test wuerfelflaechen_winden_konsistent_nach_aussen) und ihre Ebene auf der Achse.
// Normale einer (schraegen) Flaeche aus der Wicklung, nicht normiert.
fn quad_normal(q: &[[f32; 3]; 4]) -> [f32; 3] {
    let (a, b, c) = (q[0], q[1], q[2]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

fn face_group(q: &[[f32; 3]; 4]) -> (usize, f32) {
    let (a, b, c) = (q[0], q[1], q[2]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let len = n.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
    if len <= 0.0 {
        return (6, 0.0);
    }
    for axis in 0..3 {
        let andere = (0..3).filter(|&k| k != axis).all(|k| n[k].abs() <= len * 1e-4);
        if andere && n[axis].abs() == len {
            return (axis * 2 + usize::from(n[axis] < 0.0), a[axis]);
        }
    }
    (6, 0.0)
}

// Ein zusammenhaengender Abschnitt einer Richtungsgruppe, dessen Flaechen alle zu derselben
// Section (16er-Hoehenstufe) gehoeren. Das Hoehlen-Culling (s. render::occlusion) laesst
// unsichtbare Sections weg, indem es nur die Laeufe sichtbarer Sections zeichnet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionRun {
    pub group: u8,
    pub section: u8,
    pub first_quad: u32,
    pub quads: u32,
}

// Zu welcher Section eine Flaeche gehoert: zu der des Blocks, der sie erzeugt hat. Bei
// achsenparallelen Flaechen liegt der ein Stueck gegen die Normale hinter der Ebene (die
// Oberseite eines Blocks bei y=15 liegt auf y=16, gehoert aber zu y=15), sonst zur Mitte.
// `q`: Ecken mit absolutem y (Mesher-Ausgabe).
fn section_of(q: &[[f32; 3]; 4], group: usize, min_y: i32) -> u8 {
    let y = q.iter().map(|v| v[1]).sum::<f32>() / 4.0;
    let y = match group {
        2 => y - 0.01,
        3 => y + 0.01,
        _ => y,
    };
    ((y.floor() as i32 - min_y).div_euclid(16)).clamp(0, 255) as u8
}

// Ordnet die Flaechen von pack_terrain nach Richtung und innerhalb jeder Richtung nach
// Section (stabil, sonst bleibt die Reihenfolge). `starts` (gepackte Vertex-Indizes, s.
// pack_terrain) wandern mit. Die Section jeder Flaeche hat pack_terrain schon festgelegt.
pub fn group_by_direction(packed: &[TerrainVertex], starts: &mut [u32]) -> (Vec<TerrainVertex>, [FaceGroup; FACE_GROUPS], Vec<SectionRun>) {
    let quads = packed.len() / 4;
    let info: Vec<(usize, f32, u8)> = packed
        .chunks_exact(4)
        .map(|q| {
            let (g, plane) = face_group(&[q[0].position(), q[1].position(), q[2].position(), q[3].position()]);
            (g, plane, q[0].section())
        })
        .collect();
    let mut groups = [FaceGroup::default(); FACE_GROUPS];
    for (g, plane, _) in &info {
        let group = &mut groups[*g];
        if group.quads == 0 {
            group.plane_min = *plane;
            group.plane_max = *plane;
        } else {
            group.plane_min = group.plane_min.min(*plane);
            group.plane_max = group.plane_max.max(*plane);
        }
        group.quads += 1;
    }
    let mut first = 0;
    for group in groups.iter_mut() {
        group.first_quad = first;
        first += group.quads;
    }
    // Stabil nach (Gruppe, Section) sortieren und die Laeufe ablesen.
    let mut order: Vec<usize> = (0..quads).collect();
    order.sort_by_key(|&q| (info[q].0, info[q].2));
    let mut out = Vec::with_capacity(quads * 4);
    let mut new_index = vec![0u32; quads];
    let mut runs: Vec<SectionRun> = Vec::new();
    for (ziel, &q) in order.iter().enumerate() {
        out.extend_from_slice(&packed[q * 4..q * 4 + 4]);
        new_index[q] = ziel as u32;
        let (g, _, sec) = info[q];
        match runs.last_mut() {
            Some(r) if r.group as usize == g && r.section == sec => r.quads += 1,
            _ => runs.push(SectionRun { group: g as u8, section: sec, first_quad: ziel as u32, quads: 1 }),
        }
    }
    for start in starts.iter_mut() {
        if *start != u32::MAX {
            *start = new_index.get(*start as usize / 4).map_or(u32::MAX, |&q| q * 4);
        }
    }
    (out, groups, runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32) -> BlockVertex {
        BlockVertex { position: [x, 0.0, 0.0], color: [1.0, 0.5, 0.0], uv: [0.25, 1.0] }
    }

    #[test]
    fn flaeche_wird_zu_vier_vertices() {
        let (a, b, c, d) = (v(0.0), v(1.0), v(2.0), v(3.0));
        let mut starts = [];
        let out = pack_terrain(&[a, b, c, a, c, d], &mut starts, 0);
        assert_eq!(out.iter().map(|t| t.position()[0]).collect::<Vec<_>>(), vec![0.0, 1.0, 2.0, 3.0]);
        // Alpha traegt das Licht: ohne Berechnung voller Himmel, kein Blocklicht.
        assert_eq!(out[0].color, [255, 128, 0, TerrainVertex::NO_LIGHT as u8]);
        assert_eq!(out[0].uv, [16384, 65535]);
    }

    // Feuer: das Kennzeichen kommt als Alpha an, die Farbe unveraendert.
    #[test]
    fn abschneide_kennzeichen_ueberlebt_das_packen() {
        let feuer = BlockVertex { position: [0.0; 3], color: [0.75, 0.5, 0.25], uv: [0.0; 2] }.with_cutout_mark();
        assert_eq!(feuer.plain_color(), [0.75, 0.5, 0.25]);
        let t = TerrainVertex::from_block(&feuer, 0, 0);
        assert_eq!(t.color, [191, 128, 64, TerrainVertex::CUTOUT_MARK]);
    }

    // Licht in den freien Bits verdraengt weder Section noch Position.
    #[test]
    fn licht_bits_neben_der_section() {
        let t = TerrainVertex::from_block(&BlockVertex { position: [3.0, 37.0, 5.0], color: [1.0; 3], uv: [0.0; 2] }, 2, 0);
        let licht = 0x7FFFF; // alle 19 Bit
        let l = t.with_light(licht);
        assert_eq!(l.light(), licht);
        assert_eq!(l.section(), 2);
        assert_eq!(l.position(), t.position());
        assert_eq!(l.with_light(5).light(), 5);
    }

    #[test]
    fn einzelnes_dreieck_wird_entartete_flaeche() {
        let (a, b, c, d) = (v(0.0), v(1.0), v(2.0), v(3.0));
        // Dreieck, dann eine normale Flaeche: beide Anfaenge muessen getroffen werden.
        let mut starts = [3, 0, 4];
        let out = pack_terrain(&[a, b, c, a, b, c, a, c, d], &mut starts, 0);
        assert_eq!(out.len(), 8);
        assert_eq!(out[3], out[2]);
        assert_eq!(starts, [4, 0, u32::MAX]);
    }

    // Die sechs Wuerfelseiten landen je in ihrer Gruppe, mit ihrer Ebene; ein Start wandert mit.
    #[test]
    fn wuerfelseiten_nach_richtung() {
        let mut verts = Vec::new();
        for dir in [[0, 1, 0], [1, 0, 0], [0, 0, -1], [-1, 0, 0], [0, -1, 0], [0, 0, 1]] {
            crate::mesher::push_face_quad_for_test(&mut verts, [2.0, 3.0, 4.0], [3.0, 4.0, 5.0], dir);
        }
        let mut starts = [0u32]; // die +Y-Flaeche
        let packed = pack_terrain(&verts, &mut starts, -64);
        let (out, groups, runs) = group_by_direction(&packed, &mut starts);
        assert_eq!(out.len(), 24);
        // y-Ebenen relativ zu min_y = -64.
        for (g, plane) in [(0, 3.0), (1, 2.0), (2, 68.0), (3, 67.0), (4, 5.0), (5, 4.0)] {
            assert_eq!(groups[g].quads, 1, "Gruppe {g}");
            assert_eq!(groups[g].first_quad, g as u32);
            assert_eq!(groups[g].plane_min, plane, "Gruppe {g}");
        }
        assert_eq!(groups[6].quads, 0);
        assert_eq!(starts, [8], "+Y ist Gruppe 2, also Flaeche 2");
        // Ein Block bei y=3 liegt in Section 4 (ab -64); auch seine Oberseite auf y=4.
        assert_eq!(runs.len(), 6);
        assert!(runs.iter().all(|r| r.section == 4 && r.quads == 1));
        // Das Auge ueber dem Block (y=10, also 74 ueber min_y) sieht die Oberseite, nicht die
        // Unterseite.
        assert!(groups[2].faces_eye(2, [2.5, 74.0, 4.5]));
        assert!(!groups[3].faces_eye(3, [2.5, 74.0, 4.5]));
    }

    #[test]
    // Die Versaetze gegen Z-Fighting (1/1024 Block) und Modellpixel (1/16) kommen exakt zurueck,
    // auch ueber die Section hinausragende Teile (Feuer bis 1,4 ueber der Zelle).
    fn positionen_bleiben_exakt() {
        let eps = 1.0 / 1024.0;
        for p in [[0.0, 0.0, 0.0], [eps, 15.0 + eps, 16.0 - eps], [3.0625, 17.4, -0.5], [15.9375, -0.25, 7.0]] {
            let b = BlockVertex { position: [p[0], p[1] + 32.0, p[2]], color: [1.0; 3], uv: [0.0; 2] };
            // Section 6 ab min_y -64 beginnt bei y 32.
            let t = TerrainVertex::from_block(&b, 6, -64);
            let zurueck = t.position();
            for i in 0..3 {
                let soll = if i == 1 { p[1] + 32.0 + 64.0 } else { p[i] };
                assert!((zurueck[i] - soll).abs() <= 0.5 / TerrainVertex::POS_SCALE, "{p:?}: {zurueck:?}");
            }
            if p[1] == 15.0 + eps {
                assert_eq!(zurueck[1], 15.0 + eps + 96.0, "1/1024-Versatz exakt");
            }
        }
        assert_eq!(std::mem::size_of::<TerrainVertex>(), 16);
    }
}
