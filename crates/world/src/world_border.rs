// Die Weltgrenze, wie der Server sie meldet (s. protocol::packets::BorderUpdate).
//
// Eine quadratische Grenze um eine Mitte. Ihre Groesse steht entweder still oder wandert
// ueber eine Zahl von Ticks von einer Groesse zur anderen; jeder Client-Tick zaehlt einen
// Tick herunter. Fuer das Bild wird zwischen der Groesse des letzten und des aktuellen
// Ticks interpoliert, damit eine schrumpfende Grenze nicht im Tick-Raster ruckelt.
//
// Ausserhalb ist alles fest: wer nahe genug an der Grenze steht, stoesst sich an ihr wie an
// einer Wand (s. collision_boxes). Der Server macht das genauso -- ohne diese Wand liefe die
// eigene Vorhersage durch und der Server setzte einen zurueck.

pub use protocol::packets::BorderUpdate;

// Startwerte einer frischen Welt, bevor der Server etwas schickt: eine Grenze, die so gross
// ist wie die ganze Welt, 5 Bloecke und 15 Sekunden Warnung.
pub const DEFAULT_SIZE: f64 = 5.9999968e7;
pub const DEFAULT_ABSOLUTE_MAX_SIZE: i32 = 29_999_984;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Extent {
    Static(f64),
    // Wandernde Groesse: `remaining` Ticks bis zum Ziel, `size`/`previous` die Groesse dieses
    // und des vorigen Ticks.
    Moving { from: f64, to: f64, duration: f64, remaining: i64, size: f64, previous: f64 },
}

impl Extent {
    fn moving(from: f64, to: f64, ticks: i64) -> Self {
        let size = from;
        Self::Moving { from, to, duration: ticks as f64, remaining: ticks, size, previous: size }
    }

    fn size_at(&self, partial: f32) -> f64 {
        match *self {
            Self::Static(s) => s,
            Self::Moving { size, previous, .. } => previous + (size - previous) * partial as f64,
        }
    }
}

// Wohin die Grenze sich gerade bewegt. Bestimmt die Farbe der Wand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStatus {
    Growing,
    Shrinking,
    Stationary,
}

impl BorderStatus {
    // Farben der Wand: gruen beim Wachsen, rot beim Schrumpfen, blau im Stillstand.
    pub fn color(self) -> u32 {
        match self {
            Self::Growing => 0x40_FF_80,
            Self::Shrinking => 0xFF_30_30,
            Self::Stationary => 0x20_A0_FF,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBorder {
    center: (f64, f64),
    absolute_max_size: i32,
    pub warning_time: i32,
    pub warning_blocks: i32,
    extent: Extent,
}

impl Default for WorldBorder {
    fn default() -> Self {
        Self { center: (0.0, 0.0), absolute_max_size: DEFAULT_ABSOLUTE_MAX_SIZE, warning_time: 15, warning_blocks: 5, extent: Extent::Static(DEFAULT_SIZE) }
    }
}

impl WorldBorder {
    pub fn apply(&mut self, update: BorderUpdate) {
        match update {
            BorderUpdate::Initialize { center, old_size, new_size, lerp_ticks, absolute_max_size, warning_blocks, warning_time } => {
                self.center = center;
                if lerp_ticks > 0 {
                    self.lerp_size(old_size, new_size, lerp_ticks);
                } else {
                    self.extent = Extent::Static(new_size);
                }
                self.absolute_max_size = absolute_max_size;
                self.warning_blocks = warning_blocks;
                self.warning_time = warning_time;
            }
            BorderUpdate::Center(x, z) => self.center = (x, z),
            BorderUpdate::LerpSize { old_size, new_size, lerp_ticks } => self.lerp_size(old_size, new_size, lerp_ticks),
            BorderUpdate::Size(s) => self.extent = Extent::Static(s),
            BorderUpdate::WarningTime(t) => self.warning_time = t,
            BorderUpdate::WarningBlocks(b) => self.warning_blocks = b,
        }
    }

    // Gleiche Groessen oder keine Dauer: gar nicht erst wandern.
    fn lerp_size(&mut self, from: f64, to: f64, ticks: i64) {
        self.extent = if from == to || ticks <= 0 { Extent::Static(to) } else { Extent::moving(from, to, ticks) };
    }

    // Ein Client-Tick: die wandernde Groesse einen Schritt weiter, am Ziel steht sie still.
    pub fn tick(&mut self) {
        if let Extent::Moving { from, to, duration, remaining, size, previous } = &mut self.extent {
            *remaining -= 1;
            *previous = *size;
            let t = (*duration - *remaining as f64) / *duration;
            *size = if t < 1.0 { *from + (*to - *from) * t } else { *to };
            if *remaining <= 0 {
                self.extent = Extent::Static(*to);
            }
        }
    }

    pub fn status(&self) -> BorderStatus {
        match self.extent {
            Extent::Static(_) => BorderStatus::Stationary,
            Extent::Moving { from, to, .. } if to < from => BorderStatus::Shrinking,
            Extent::Moving { .. } => BorderStatus::Growing,
        }
    }

    pub fn size(&self) -> f64 {
        self.extent.size_at(1.0)
    }

    pub fn center(&self) -> (f64, f64) {
        self.center
    }

    // Die vier Kanten (min_x, max_x, min_z, max_z), `partial` zwischen dem letzten und dem
    // aktuellen Tick. Nie weiter draussen als die absolute Hoechstgroesse.
    pub fn bounds(&self, partial: f32) -> [f64; 4] {
        let half = self.extent.size_at(partial) / 2.0;
        let max = self.absolute_max_size as f64;
        let c = |v: f64| v.clamp(-max, max);
        [c(self.center.0 - half), c(self.center.0 + half), c(self.center.1 - half), c(self.center.1 + half)]
    }

    // Abstand zur naechsten Kante; drinnen positiv, draussen negativ.
    pub fn distance_to_border(&self, x: f64, z: f64) -> f64 {
        let [min_x, max_x, min_z, max_z] = self.bounds(0.0);
        (x - min_x).min(max_x - x).min(z - min_z).min(max_z - z)
    }

    pub fn is_within_bounds(&self, x: f64, z: f64, margin: f64) -> bool {
        let [min_x, max_x, min_z, max_z] = self.bounds(0.0);
        x >= min_x - margin && x < max_x + margin && z >= min_z - margin && z < max_z + margin
    }

    // Die Grenze als Kollisionsform fuer eine Box mit Mitte (x, z), die sich ueber `swept`
    // (Ausdehnung in x und z der in Bewegungsrichtung aufgezogenen Box) bewegt -- leer, wenn
    // die Box weit genug weg ist oder schon draussen steht (dann haelt die Wand nicht fest).
    // Vier Boxen fuer alles ausserhalb, die Kanten auf ganze Bloecke nach aussen gerundet.
    pub fn collision_boxes(&self, x: f64, z: f64, swept: (f64, f64)) -> Vec<([f64; 3], [f64; 3])> {
        let d = swept.0.abs().max(swept.1.abs()).max(1.0);
        if !(self.distance_to_border(x, z) < d * 2.0 && self.is_within_bounds(x, z, d)) {
            return Vec::new();
        }
        let [min_x, max_x, min_z, max_z] = self.bounds(0.0);
        let (x0, x1, z0, z1) = (min_x.floor(), max_x.ceil(), min_z.floor(), max_z.ceil());
        const FERN: f64 = 1.0e9;
        vec![
            ([-FERN, -FERN, -FERN], [x0, FERN, FERN]),
            ([x1, -FERN, -FERN], [FERN, FERN, FERN]),
            ([x0, -FERN, -FERN], [x1, FERN, z0]),
            ([x0, -FERN, z1], [x1, FERN, FERN]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grenze(center: (f64, f64), size: f64) -> WorldBorder {
        let mut b = WorldBorder::default();
        b.apply(BorderUpdate::Center(center.0, center.1));
        b.apply(BorderUpdate::Size(size));
        b
    }

    #[test]
    fn frische_welt_hat_die_grosse_grenze() {
        let b = WorldBorder::default();
        assert_eq!(b.bounds(1.0), [-29_999_984.0, 29_999_984.0, -29_999_984.0, 29_999_984.0]);
        assert_eq!(b.status(), BorderStatus::Stationary);
    }

    #[test]
    fn kanten_um_die_mitte() {
        let b = grenze((100.0, -50.0), 20.0);
        assert_eq!(b.bounds(0.5), [90.0, 110.0, -60.0, -40.0]);
        assert_eq!(b.distance_to_border(92.0, -50.0), 2.0);
        assert_eq!(b.distance_to_border(80.0, -50.0), -10.0);
    }

    #[test]
    fn schrumpfen_zaehlt_ticks_und_steht_am_ende_still() {
        let mut b = grenze((0.0, 0.0), 100.0);
        b.apply(BorderUpdate::LerpSize { old_size: 100.0, new_size: 50.0, lerp_ticks: 10 });
        assert_eq!(b.status(), BorderStatus::Shrinking);
        assert_eq!(b.size(), 100.0);
        b.tick();
        assert_eq!(b.size(), 95.0);
        // Zwischen den Ticks: halbe Strecke zwischen 100 und 95.
        assert_eq!(b.bounds(0.5)[1], 97.5 / 2.0);
        for _ in 0..9 {
            b.tick();
        }
        assert_eq!(b.size(), 50.0);
        assert_eq!(b.status(), BorderStatus::Stationary);
    }

    #[test]
    fn gleiche_groessen_wandern_nicht() {
        let mut b = grenze((0.0, 0.0), 100.0);
        b.apply(BorderUpdate::LerpSize { old_size: 30.0, new_size: 30.0, lerp_ticks: 100 });
        assert_eq!(b.status(), BorderStatus::Stationary);
        assert_eq!(b.size(), 30.0);
    }

    #[test]
    fn wand_nur_in_der_naehe_und_nur_von_innen() {
        let b = grenze((0.0, 0.0), 20.4);
        // Spieler (0.6 breit) mitten drin: keine Wand.
        assert!(b.collision_boxes(0.0, 0.0, (0.6, 0.6)).is_empty());
        // Knapp vor der Ostkante: die Wand steht bei aufgerundet x = 11.
        let boxes = b.collision_boxes(9.5, 0.0, (0.6, 0.6));
        assert_eq!(boxes.len(), 4);
        assert_eq!(boxes[1].0[0], 11.0);
        assert_eq!(boxes[0].1[0], -11.0);
        // Weit draussen haelt sie nicht fest.
        assert!(b.collision_boxes(40.0, 0.0, (0.6, 0.6)).is_empty());
    }
}
