// Regen und Schnee als einzelne Teilchen, nach dem Vorbild der Mod particle-rain (nur deren
// Regen und Schnee; Nebel, Pfuetzenringe, Spritzer, Schlieren an Waenden, Staub und
// Steppenlaeufer stehen fuer den Shader-Pfad auf der Liste, s. Memory).
//
// Statt Vanillas senkrechter Streifen je Blockspalte fallen Tropfen und Flocken einzeln.
//
// Regen:
//   - entsteht oben am Rand einer Kugel um die Kamera (Radius = Wetterradius), ueber einer
//     gleichverteilten Stelle im Kreis darunter, nur ueber dem obersten blockierenden Block
//     der Spalte.
//   - Jeden Tick: Schwerkraft bis zur echten Endgeschwindigkeit (8 m/s, s. ARTEN),
//     Luftwiderstand 0.98, Wind (Boeen aus zwei Sinuswellen ueber Ort und Zeit, erst ab y 50
//     und bis y 90 ansteigend). Bei Gewitter weht er staerker, und es kommen schwere Tropfen
//     dazu.
//   - Boden ist die Oberkante des obersten blockierenden Blocks der Spalte (auch Wasser).
//     Regen faellt hinein und wird an der Bodenkante abgeschnitten. Treibt ein Tropfen
//     seitlich in eine Wand oder in eine Spalte mit anderem Niederschlag (Regen ueber einem
//     Schneebiom), schrumpft er weg.
//   - Ab zwei Dritteln des Radius blendet er aus, ausserhalb des Radius verschwindet er.
//   - Er steht laengs seiner Fallrichtung RELATIV zur Kamera (wer laeuft, sieht ihn schraeg)
//     und ist zur Kamera gedreht.
//
// Schnee (eigene Loesung, nicht aus particle-rain; Entwurf des Nutzers): einzelne Flocken in einem
// Bereich um das Auge.
//   - Eine Karte sagt fuer jeden Block im Umkreis 2r, ob dort Schnee sein kann: nicht, wo ein
//     Block ist, der Schnee aufhaelt, und nicht im Schutz eines Daches (rote Zone: von oben
//     nach unten gefuellt kommt kein Schnee dorthin). Sichtbar ist der Bereich, in dem auf der Linie vom Auge hoechstens r freie
//     Strecke liegt (r = Wetterradius bzw. ohne Modul SNOW_RADIUS): ohne Hindernis die Kugel
//     mit Radius r, unter einem Dach um die blockierte Strecke nach aussen geschoben. S.
//     snow_map.
//   - Die Flocken (s. snow_flakes) entstehen im obersten Block des Bereichs jeder Spalte und in
//     Bloecken, die neu in den Bereich kommen, fallen (Regler "Schneetempo") und wandern dabei
//     zufaellig zur Seite. Kommen sie in einen Block ausserhalb des Bereichs oder in eine Spalte
//     ohne Schnee-Biom, verschwinden sie sofort; ebenso alle Flocken eines Blocks, der den
//     Bereich verlaesst. Kein Ausblenden.
//   - Dichte nach Regler "Menge" (bei 70 % so viele wie DROPS_AT_DEFAULT in der Kugel mit
//     SNOW_RADIUS), mal Regenstaerke (beginnender Schnee wird dichter).
//
// Die Bilder kommen aus Vanillas rain.png/snow.png (bzw. dem Pack): das Blatt wird in
// quadratische Kacheln zu je halber Breite zerlegt (bei Vanilla 16 Stueck), dazu vier schwere
// Kacheln, die je ein Viertel der Kacheln zusammenlegen (s. weather_sheet). Regen wird dabei
// hellgrau und bekommt beim Entstehen einen Hauch der Wasserfarbe des Bioms.
//
// Eigene Entscheidungen: Licht wie bei den uebrigen Partikeln voll (keine Lichtkarte); der
// Boden ist die Oberkante des obersten blockierenden Blocks der Spalte statt eines Strahls
// gegen die echten Kollisionsformen; Barrieren halten nichts auf (s. App::tick_weather).

use glam::{DVec3, Vec3};
use render::WeatherVertex;

mod snow_flakes;
mod snow_map;

pub use snow_map::SnowChanges;

// Radius: im Modul "Wetter" einstellbar, sonst DISTANCE_DEFAULT (Regen) bzw. SNOW_RADIUS.
pub const RADIUS_MIN: u32 = 3;
pub const RADIUS_DEFAULT: u32 = 10;
pub const RADIUS_MAX: u32 = 16;
pub const DISTANCE_DEFAULT: u32 = 16;

// Schnee ohne Wetter-Modul in dieser Kugel (mit Modul der eingestellte Radius): Bloecke mit
// hoechstens SNOW_RADIUS freier Strecke zum Auge, nie weiter als SNOW_RADIUS_OUTER.
pub const SNOW_RADIUS: u32 = 10;
pub const SNOW_RADIUS_OUTER: u32 = 20;
// Regler "Schneeradius" und "Maximaler Schneeradius" im Modul Wetter; der maximale ist nie
// kleiner als der Schneeradius.
pub const SNOW_RADIUS_MIN: u32 = 3;
pub const SNOW_RADIUS_MAX: u32 = 16;
pub const SNOW_RADIUS_OUTER_MAX: u32 = 32;

// Menge in Prozent (Regler "Menge" im Modul Wetter). Bei DENSITY_DEFAULT hoechstens
// DROPS_AT_DEFAULT Tropfen gleichzeitig (particle-rains Vorgabe) bzw. so viele Flocken in der
// Schneekugel, die Zahl waechst mit der Menge; MAX_DROPS ist, was in
// render::WEATHER_VERTEX_CAPACITY passt.
pub const DENSITY_MIN: u32 = 10;
pub const DENSITY_DEFAULT: u32 = 70;
pub const DENSITY_MAX: u32 = 200;
pub const DENSITY_STEP: u32 = 10;
const DROPS_AT_DEFAULT: usize = 1500;
pub const MAX_DROPS: usize = render::WEATHER_VERTEX_CAPACITY / 6;

// Tropfen leben hoechstens so viele Ticks.
const LIFETIME: u32 = 3000;

// Schneetempo (Regler im Modul Wetter) in Zehntel Bloecken je Sekunde; echter Schnee faellt
// etwa 0,5-1,5 m/s.
pub const SNOW_SPEED_MIN: u32 = 2;
pub const SNOW_SPEED_DEFAULT: u32 = 10;
pub const SNOW_SPEED_MAX: u32 = 40;
pub const SNOW_SPEED_STEP: u32 = 1;

// Anteil der Flocken, die bei Gewitter schwer sind (particle-rain: Dichte 0.4 zu 0.3).
const HEAVY_SHARE: f32 = 4.0 / 7.0;

// Radien der Kugeln um die Kamera, in denen Regen bzw. Schnee faellt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub rain: u32,
    pub snow: u32,
    // Weiter weg vom Auge nie Schnee (Halbkante der Schneekarte).
    pub snow_max: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rain,
    Snow,
}

// Eine Teilchenart. `density` = Anteil der Versuche, die einen Tropfen erzeugen (nur Regen).
struct Art {
    kind: Kind,
    heavy: bool,
    // Endgeschwindigkeit in Bloecken je Tick (1 Block = 1 m, 20 Ticks je Sekunde); fuer Schnee
    // ein Faktor auf das eingestellte Schneetempo.
    fall: f32,
    wind: f32,
    storm_wind: f32,
    rotation: f32,
    size: f32,
    density: f32,
}

const ARTEN: [Art; 4] = [
    // Regentropfen fallen echt 6-9 m/s: 8 m/s, schwere 9 m/s.
    Art { kind: Kind::Rain, heavy: false, fall: 0.4, wind: 0.4, storm_wind: 0.8, rotation: 0.0, size: 0.75, density: 1.0 },
    Art { kind: Kind::Rain, heavy: true, fall: 0.45, wind: 0.4, storm_wind: 0.8, rotation: 0.0, size: 0.75, density: 1.0 },
    Art { kind: Kind::Snow, heavy: false, fall: 1.0, wind: 0.01, storm_wind: 0.1, rotation: 0.4, size: 0.75, density: 0.3 },
    Art { kind: Kind::Snow, heavy: true, fall: 1.5, wind: 0.01, storm_wind: 0.1, rotation: 0.4, size: 0.75, density: 0.4 },
];

const fn snow_art(heavy: bool) -> &'static Art {
    if heavy { &ARTEN[3] } else { &ARTEN[2] }
}

// Wind: Staerke, Boeen-Frequenz ueber den Ort, Wandergeschwindigkeit ueber die Zeit.
const WIND_STRENGTH: f32 = 0.03;
const WIND_FREQUENCY: f32 = 0.15;
const WIND_SPEED: f32 = 0.1;

#[derive(Clone, Copy, Debug)]
struct Drop {
    pos: DVec3,
    prev: DVec3,
    // Bloecke je Tick.
    vel: Vec3,
    // Endgeschwindigkeit (Bloecke je Tick).
    fall: f32,
    art: u8,
    sprite: u8,
    size: f32,
    color: [f32; 3],
    alpha: f32,
    // Wegschrumpfen (Wand, anderer Niederschlag): Fortschritt von 1 nach 0 (vorheriger Tick,
    // dieser Tick).
    hit: Option<(f32, f32)>,
    // Boden der Spalte im letzten Tick (fuers Abschneiden).
    ground: f64,
    age: u32,
}

// Aufteilung eines Wetterbildes (s. weather_sheet): `rows` Zeilen normaler Kacheln in zwei
// Spalten, darunter zwei Zeilen mit den vier schweren. rows = 0: kein Bild.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SheetLayout {
    pub rows: u32,
    // Je Kachel (0..16 normale, 16..20 schwere): wo die Mitte dessen liegt, was in ihr zu sehen
    // ist (Mitte des Rechtecks um alle nicht durchsichtigen Pixel), als Abstand von der
    // Kachelmitte in Kachelbreiten (x nach rechts, y nach unten im Bild). Die Punkte eines
    // Schneebildes sitzen irgendwo in der Kachel; damit zeichnet man sie dorthin, wo die Flocke
    // ist (s. snow_quad).
    pub offsets: [[f32; 2]; 20],
}

impl SheetLayout {
    const fn normal_count(self) -> u32 {
        self.rows * 2
    }

    // Atlas-Rechteck (u0, v0, u1, v1) einer Kachel.
    fn rect(self, sprite: u8, heavy: bool) -> [f32; 4] {
        let gesamt = (self.rows + 2) as f32;
        let i = u32::from(sprite);
        let (spalte, zeile) = if heavy { (i / 2, self.rows + i % 2) } else { (i / self.rows, i % self.rows) };
        let (u0, v0) = (spalte as f32 * 0.5, zeile as f32 / gesamt);
        [u0, v0, u0 + 0.5, v0 + 1.0 / gesamt]
    }
}

// Baut aus rain.png/snow.png das Blatt fuer die Teilchen: die Kacheln (Kantenlaenge = halbe
// Bildbreite) unveraendert, darunter vier schwere Kacheln, die je ein Viertel der Kacheln per
// hoechstem Alpha zusammenlegen. Ist das Bild durchgehend nur schwach deckend, wird Alpha so
// weit wie moeglich ganzzahlig hochgezogen. `grau`: Farbe auf den hellsten Kanal (fuer die
// Wassertoenung des Regens).
pub fn weather_sheet(rgba: &[u8], width: u32, height: u32, grau: bool) -> Option<(Vec<u8>, u32, u32, SheetLayout)> {
    let s = width / 2;
    let rows = height.checked_div(s).unwrap_or(0);
    if rows == 0 || rgba.len() < (width * height * 4) as usize {
        return None;
    }
    let max_alpha = rgba.chunks_exact(4).map(|p| p[3]).max().unwrap_or(0);
    // Grau bis ganz hell: Vanillas Regen ist dunkelblau (70, 103, 195), echter Regen hell.
    let max_kanal = rgba.chunks_exact(4).filter(|p| p[3] > 0).map(|p| p[0].max(p[1]).max(p[2])).max().unwrap_or(0).max(1);
    let faktor = if max_alpha == 0 || max_alpha == 255 { 1 } else { 255 / u32::from(max_alpha) };
    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        let mut p = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
        p[3] = (u32::from(p[3]) * faktor).min(255) as u8;
        if grau {
            let g = (u32::from(p[0].max(p[1]).max(p[2])) * 255 / u32::from(max_kanal)) as u8;
            p = [g, g, g, p[3]];
        }
        p
    };
    let mut layout = SheetLayout { rows, offsets: [[0.0; 2]; 20] };
    let out_h = (rows + 2) * s;
    let mut out = vec![0u8; (width * out_h * 4) as usize];
    let mut setze = |x: u32, y: u32, p: [u8; 4], nur_staerker: bool| {
        let i = ((y * width + x) * 4) as usize;
        if !nur_staerker || p[3] > out[i + 3] {
            out[i..i + 4].copy_from_slice(&p);
        }
    };
    for y in 0..rows * s {
        for x in 0..width {
            setze(x, y, pixel(x, y), false);
        }
    }
    let n = layout.normal_count();
    let gruppe = (n / 4).max(1);
    for schwer in 0..4u32 {
        let (zs, zz) = (schwer / 2, rows + schwer % 2);
        for k in schwer * gruppe..((schwer + 1) * gruppe).min(n) {
            let (qs, qz) = (k / rows, k % rows);
            for y in 0..s {
                for x in 0..s {
                    setze(zs * s + x, zz * s + y, pixel(qs * s + x, qz * s + y), true);
                }
            }
        }
    }
    // Wo in jeder Kachel etwas zu sehen ist.
    let mitte_von = |ks: u32, kz: u32| -> [f32; 2] {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..s {
            for x in 0..s {
                if out[(((kz * s + y) * width + ks * s + x) * 4 + 3) as usize] > 0 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
        }
        if x0 == u32::MAX {
            return [0.0; 2];
        }
        [((x0 + x1 + 1) as f32 / 2.0) / s as f32 - 0.5, ((y0 + y1 + 1) as f32 / 2.0) / s as f32 - 0.5]
    };
    for k in 0..n.min(16) {
        layout.offsets[k as usize] = mitte_von(k / rows, k % rows);
    }
    for schwer in 0..4u32 {
        layout.offsets[16 + schwer as usize] = mitte_von(schwer / 2, rows + schwer % 2);
    }
    Some((out, width, out_h, layout))
}

// Kleiner Zufallsgenerator (splitmix64).
struct Rng(u64);

impl Rng {
    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    // Gleichverteilt in [0, 1).
    fn next(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n.max(1))) as u32
    }
}

// Windgeschwindigkeit (x, z) an einem Ort, Bloecke je Tick vor dem Faktor der Art.
fn wind_at(pos: DVec3, ticks: u64) -> (f32, f32) {
    let zeit = (ticks % 1_000_000) as f32 * WIND_SPEED;
    let hoehe = ((pos.y as f32 - 50.0) / 40.0).clamp(0.0, 1.0);
    let welle = |p: f32| -((p * WIND_FREQUENCY + zeit).sin() * (p * WIND_FREQUENCY * 0.5 + zeit * 0.5).sin() * WIND_STRENGTH + WIND_STRENGTH);
    (welle(pos.x as f32) * hoehe, welle(pos.z as f32) * hoehe)
}

// Ausblenden ab zwei Dritteln des Radius, bei `radius` ganz weg.
fn edge_fade(abstand: f32, radius: f32) -> f32 {
    let f = radius / 3.0;
    1.0 - ((abstand - 2.0 * f) / f).clamp(0.0, 1.0)
}

// Zufallszahl in [0, 1) aus der Nummer einer Flocke und einer Nummer (splitmix64): ihr Aussehen.
fn flake_random(id: u64, n: u32) -> f32 {
    let mut z = id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(n).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

// Flocken je Block bei dieser Menge (s. oben).
fn snow_per_block(density: u32) -> f64 {
    (DROPS_AT_DEFAULT * density as usize / DENSITY_DEFAULT as usize) as f64 / (4.0 / 3.0 * std::f64::consts::PI * f64::from(SNOW_RADIUS).powi(3))
}

// Schnee-Billboard in der Bildebene, um `roll` gedreht. `versatz`: wo in der Kachel das
// Sichtbare sitzt (s. SheetLayout::offsets) -- das Billboard wird so verschoben, dass es auf
// `mitte` liegt, die Flocke also dort zu sehen ist, wo sie ist.
#[allow(clippy::too_many_arguments)]
fn snow_quad(out: &mut Vec<WeatherVertex>, mitte: Vec3, roll: f32, groesse: f32, cam_right: Vec3, cam_up: Vec3, rect: [f32; 4], versatz: [f32; 2], alpha: f32, radii: [f32; 2]) {
    let (s, c) = roll.sin_cos();
    let (achse_a, achse_b) = ((cam_right * c + cam_up * s) * groesse, (cam_up * c - cam_right * s) * groesse);
    let mitte = mitte - achse_a * (2.0 * versatz[0]) - achse_b * (2.0 * versatz[1]);
    let [u0, v0, u1, v1] = rect;
    let color = [1.0, 1.0, 1.0, alpha];
    let ecke = |a: f32, b: f32, u: f32, v: f32| WeatherVertex { position: (mitte + achse_a * a + achse_b * b).to_array(), color, uv: [u, v], radii };
    let (lo, ro, ru, lu) = (ecke(-1.0, -1.0, u0, v0), ecke(1.0, -1.0, u1, v0), ecke(1.0, 1.0, u1, v1), ecke(-1.0, 1.0, u0, v1));
    out.extend_from_slice(&[lo, ro, ru, lo, ru, lu]);
}

// Oberster blockierender Block und Niederschlag je Spalte um die Kamera (einmal pro Tick),
// die Regentropfen und die Schneeflocken.
pub struct Precipitation {
    // Zaehler der Client-Ticks -- die Uhr des Windes.
    ticks: u64,
    origin: (i32, i32),
    size: i32,
    // i32::MIN = nichts in der Spalte (oder nicht geladen).
    tops: Vec<i32>,
    // Niederschlag je Spalte, gemessen auf Hoehe ihres Bodens (wie particle-rain).
    kinds: Vec<Option<Kind>>,
    drops: Vec<Drop>,
    // Schnee (s. oben): Karte und Bereich, die Flocken, Gewitter (schwere Flocken).
    snow_map: snow_map::SnowMap,
    snow: snow_flakes::SnowFlakes,
    storm: bool,
    rng: Rng,
    // Ticks, bis das Entstehen von Regen auf Sparflamme geht (in den letzten 100 Ticks
    // entstand nichts).
    busy: u32,
}

impl Precipitation {
    pub fn new() -> Self {
        Self {
            ticks: 0,
            origin: (0, 0),
            size: 0,
            tops: Vec::new(),
            kinds: Vec::new(),
            drops: Vec::new(),
            snow_map: snow_map::SnowMap::new(),
            snow: snow_flakes::SnowFlakes::new(),
            storm: false,
            rng: Rng(0x5745_5454_4552_0001),
            busy: 0,
        }
    }

    // Explosion (s. particles::ExplosionPush): die Schneeflocken wegstossen. Regen nicht.
    pub fn on_explosion(&mut self, center: DVec3, radius: f32, world: &world::World, shapes: &world::BlockShapes) {
        let mut stoss = crate::particles::ExplosionPush::new(center, radius, world, shapes);
        self.snow.explode(|p| stoss.at(p));
    }

    #[cfg(test)]
    fn drop_count(&self) -> usize {
        self.drops.len()
    }

    fn column(&self, x: f64, z: f64) -> Option<usize> {
        let (dx, dz) = (x.floor() as i32 - self.origin.0, z.floor() as i32 - self.origin.1);
        if dx < 0 || dz < 0 || dx >= self.size || dz >= self.size {
            return None;
        }
        Some((dz * self.size + dx) as usize)
    }

    // Boden einer Spalte: Oberkante des obersten blockierenden Blocks; None ausserhalb.
    fn surface(&self, x: f64, z: f64) -> Option<f64> {
        let top = self.tops[self.column(x, z)?];
        Some(if top == i32::MIN { f64::NEG_INFINITY } else { f64::from(top) + 1.0 })
    }

    fn kind_at(&self, x: f64, z: f64) -> Option<Kind> {
        self.kinds[self.column(x, z)?]
    }

    // Ein Client-Tick. `eye` Kamera, `rain` und
    // `thunder` die Staerken (Gewitter schon mit Regen multipliziert), `kind` der Niederschlag
    // an (x, y, z), `tint` die Regenfarbe dort, `top(x, z)` Y des hoechsten blockierenden
    // Blocks, `solid(x, y, z)` die Oberkante (0..1 im Block) dessen, was dort Flocken
    // aufhaelt, oder None, `snow_top(x, z)` Y des hoechsten solchen Blocks der Spalte,
    // `revision` der Stand der Chunk-Spalten und was sich geaendert hat (s. SnowChanges).
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        eye: DVec3,
        bounds: Bounds,
        density: u32,
        snow_speed: u32,
        rain: f32,
        thunder: f32,
        kind: impl Fn(i32, i32, i32) -> Option<Kind>,
        tint: impl Fn(i32, i32, i32) -> [f32; 3],
        top: impl Fn(i32, i32) -> Option<i32>,
        solid: impl Fn(i32, i32, i32) -> Option<f32>,
        snow_top: impl Fn(i32, i32) -> Option<i32>,
        revision: impl SnowChanges,
    ) {
        self.ticks = self.ticks.wrapping_add(1);
        if rain <= 0.0 && self.drops.is_empty() && self.snow.flakes.is_empty() {
            self.size = 0;
            return;
        }
        // Zwei Bloecke Rand: Tropfen leben bis einen halben Block ausserhalb des Radius; der
        // Schnee braucht die Spalten bis zum maximalen Schneeradius (Karte, s. oben). Das Vermessen einer Spalte ist teuer (sie wird von
        // oben abgesucht): zieht die Kamera weiter, werden nur die neuen Spalten vermessen, die
        // alten uebernommen; jeden Tick wird ein Vierzigstel der Zeilen erneuert (gesetzte
        // Bloecke sind so nach hoechstens zwei Sekunden drin).
        let r = (bounds.rain.max(bounds.snow_max.max(bounds.snow).max(1))) as i32 + 2;
        let (cx, cz) = (eye.x.floor() as i32, eye.z.floor() as i32);
        let (origin, size) = ((cx - r, cz - r), 2 * r + 1);
        let vermessen = |x: i32, z: i32| {
            let t = top(x, z).unwrap_or(i32::MIN);
            (t, kind(x, if t == i32::MIN { eye.y.floor() as i32 } else { t }, z))
        };
        if origin != self.origin || size != self.size || self.tops.len() != (size * size) as usize {
            let (alt_o, alt_s) = (self.origin, self.size);
            let (alt_t, alt_k) = (std::mem::take(&mut self.tops), std::mem::take(&mut self.kinds));
            let alt_gueltig = alt_t.len() == (alt_s * alt_s) as usize && alt_s > 0;
            for dz in 0..size {
                for dx in 0..size {
                    let (x, z) = (origin.0 + dx, origin.1 + dz);
                    let (ax, az) = (x - alt_o.0, z - alt_o.1);
                    let (t, k) = if alt_gueltig && (0..alt_s).contains(&ax) && (0..alt_s).contains(&az) {
                        let i = (az * alt_s + ax) as usize;
                        (alt_t[i], alt_k[i])
                    } else {
                        vermessen(x, z)
                    };
                    self.tops.push(t);
                    self.kinds.push(k);
                }
            }
            self.origin = origin;
            self.size = size;
        }
        for dz in (0..size).filter(|&dz| (dz as u64 + self.ticks).is_multiple_of(40)) {
            for dx in 0..size {
                let i = (dz * size + dx) as usize;
                (self.tops[i], self.kinds[i]) = vermessen(origin.0 + dx, origin.1 + dz);
            }
        }
        self.update_drops(eye, bounds, thunder);
        self.tick_snow(eye, bounds, density, snow_speed, rain, thunder, &snow_top, &solid, &revision);
        if rain > 0.0 {
            // Mit der Regenstaerke: beginnender Regen wird langsam dichter.
            let hoechstens = ((DROPS_AT_DEFAULT * density as usize / DENSITY_DEFAULT as usize) as f32 * rain.min(1.0)) as usize;
            let hoechstens = hoechstens.min(MAX_DROPS);
            self.spawn(eye, bounds, hoechstens, thunder, &tint);
        }
    }

    fn update_drops(&mut self, eye: DVec3, bounds: Bounds, thunder: f32) {
        let mut drops = std::mem::take(&mut self.drops);
        let radius = bounds.rain as f32;
        let sturm = thunder > 0.9;
        drops.retain_mut(|d| {
            let art = &ARTEN[usize::from(d.art)];
            d.prev = d.pos;
            d.age += 1;
            if d.age >= LIFETIME {
                return false;
            }
            // Mit Luftwiderstand 0.98 endet die Beschleunigung genau bei `fall`.
            d.vel.y -= d.fall * 0.02 / 0.98;
            d.pos += d.vel.as_dvec3();
            d.vel *= 0.98;
            let abstand = (d.pos - eye).length() as f32;
            if abstand > radius + 0.5 {
                return false;
            }
            let speed = d.vel.length();
            let Some(boden) = self.surface(d.pos.x, d.pos.z) else { return false };
            // In eine neue Spalte getrieben: anderer Niederschlag dort, oder seitlich in eine
            // Wand -- wie ein Aufprall wegschrumpfen.
            let alte_spalte = (d.prev.x.floor(), d.prev.z.floor());
            if d.hit.is_none() && alte_spalte != (d.pos.x.floor(), d.pos.z.floor()) && (d.pos.y < boden - 0.5 || self.kind_at(d.pos.x, d.pos.z) != Some(art.kind)) {
                d.hit = Some((1.0, 1.0));
            }
            d.ground = boden;
            if d.hit.is_none() {
                // Regen faellt in den Boden hinein (das Bild schneidet build an der Bodenkante
                // ab) und ist weg, sobald auch sein oberes Ende darin steckt.
                if d.pos.y + f64::from(d.size) < boden {
                    return false;
                }
            } else if d.pos.y < boden - 1.0 {
                return false;
            }
            if let Some((_, jetzt)) = d.hit {
                if jetzt <= 0.0 {
                    return false;
                }
                // Zweimal je Tick wie bei particle-rain (die Mod zaehlt den Aufprall doppelt).
                d.hit = Some((jetzt, jetzt - 2.0 * speed));
            }
            d.alpha = edge_fade(abstand, radius);
            let (wx, wz) = wind_at(d.pos, self.ticks);
            let staerke = if sturm { art.storm_wind } else { art.wind };
            d.vel.x += wx * staerke;
            d.vel.z += wz * staerke;
            true
        });
        self.drops = drops;
    }

    // Schnee, ein Tick (s. oben).
    #[allow(clippy::too_many_arguments)]
    fn tick_snow(
        &mut self,
        eye: DVec3,
        bounds: Bounds,
        density: u32,
        snow_speed: u32,
        rain: f32,
        thunder: f32,
        snow_top: &impl Fn(i32, i32) -> Option<i32>,
        solid: &impl Fn(i32, i32, i32) -> Option<f32>,
        revision: &impl SnowChanges,
    ) {
        let rain = f64::from(rain.clamp(0.0, 1.0));
        if rain <= 0.0 && self.snow.flakes.is_empty() {
            return;
        }
        self.storm = thunder > 0.0;
        self.snow_map.update(eye, bounds.snow.max(1) as i32, bounds.snow_max.max(bounds.snow).max(1) as i32, snow_top, solid, revision);
        let (origin, size, kinds) = (self.origin, self.size, &self.kinds);
        let schneespalte = |x: i32, z: i32| {
            let (dx, dz) = (x - origin.0, z - origin.1);
            (0..size).contains(&dx) && (0..size).contains(&dz) && kinds[(dz * size + dx) as usize] == Some(Kind::Snow)
        };
        let zeit = std::time::Instant::now();
        self.snow.tick(&self.snow_map, snow_per_block(density) * rain, f64::from(snow_speed) / 10.0 / 20.0, &schneespalte);
        self.snow_map.diag_zeit[2] += zeit.elapsed();
    }

    fn spawn(&mut self, eye: DVec3, bounds: Bounds, hoechstens: usize, thunder: f32, tint: &impl Fn(i32, i32, i32) -> [f32; 3]) {
        if self.drops.len() >= hoechstens {
            return;
        }
        // Jeden Tick die Luecke bis `hoechstens` auffuellen -- sonst verlassen beim Laufen hinten
        // mehr die Kugel, als vorne nachkommen. Kam 100 Ticks lang nichts zustande (alles unter
        // Dach oder Schnee), nur noch 4 Versuche je Tick.
        let versuche = if self.busy == 0 {
            4
        } else {
            self.busy -= 1;
            ((hoechstens - self.drops.len()) * 8).clamp(4, 4000) as u32
        };
        let radius = f64::from(bounds.rain);
        let sturm = thunder > 0.9;
        for _ in 0..versuche {
            if self.drops.len() >= hoechstens {
                break;
            }
            // Eine Stelle gleichverteilt im waagerechten Kreis um die Kamera; ob dort Regen
            // faellt, entscheidet die Spalte.
            let (abstand, winkel) = (radius * f64::from(self.rng.next()).sqrt(), std::f64::consts::TAU * f64::from(self.rng.next()));
            let (x, z) = (eye.x + abstand * winkel.cos(), eye.z + abstand * winkel.sin());
            let Some(boden) = self.surface(x, z) else { continue };
            if self.kind_at(x, z) != Some(Kind::Rain) {
                continue;
            }
            // Regen faellt schnell: er entsteht oben am Rand der Kugel und faellt durch sie
            // hindurch (am Kugelrand liegt dieser Punkt tiefer, ist dort aber schon
            // ausgeblendet).
            let h = (radius * radius - abstand * abstand).max(0.0).sqrt();
            let pos = DVec3::new(x, eye.y + h - 0.01, z);
            if boden >= pos.y.floor() {
                continue;
            }
            let chance = self.rng.next();
            for (index, art) in ARTEN.iter().enumerate() {
                if art.kind != Kind::Rain || (art.heavy && thunder <= 0.0) || art.density <= chance || self.drops.len() >= hoechstens {
                    continue;
                }
                let (wx, wz) = wind_at(pos, self.ticks);
                let staerke = if sturm { art.storm_wind } else { art.wind } * 10.0;
                let color = tint(pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
                let sprite = if art.heavy { self.rng.below(4) } else { self.rng.below(16) } as u8;
                self.drops.push(Drop {
                    pos,
                    prev: pos,
                    vel: Vec3::new(wx * staerke, -art.fall, wz * staerke),
                    fall: art.fall,
                    art: index as u8,
                    sprite,
                    size: art.size,
                    color,
                    alpha: 0.0,
                    hit: None,
                    ground: boden,
                    age: 0,
                });
                self.busy = 100;
            }
        }
    }

    // Die Quads dieses Frames in `out` (augenrelativ): erst alle Regen-, dann alle
    // Schnee-Vertices. `cam_right`/`cam_up` spannen die Bildebene auf (fuer Schnee).
    // `layouts` = Aufteilung der Regen- und der Schnee-Textur. Rueckgabe: Anzahl Regen- und
    // Schnee-Vertices.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &self,
        eye: DVec3,
        tick_progress: f32,
        cam_vel: Vec3,
        cam_right: Vec3,
        cam_up: Vec3,
        bounds: Bounds,
        intensity: f32,
        layouts: [SheetLayout; 2],
        out: &mut Vec<WeatherVertex>,
    ) -> [u32; 2] {
        out.clear();
        let intensity = intensity.clamp(0.0, 1.0);
        let tp = f64::from(tick_progress);
        let radii = [0.0, bounds.rain as f32];
        let layout = layouts[0];
        for d in &self.drops {
            let art = &ARTEN[usize::from(d.art)];
            let alpha = d.alpha * intensity;
            // Ohne Bild, oder eine Kachel, die das Blatt nicht hat.
            if layout.rows == 0 || alpha <= 0.0 || (!art.heavy && u32::from(d.sprite) >= layout.normal_count()) {
                continue;
            }
            let mitte = (d.prev + (d.pos - d.prev) * tp - eye).as_vec3();
            // Laengs der Bewegung relativ zur Kamera, quer dazu zur Kamera gedreht.
            let rel = d.vel - cam_vel;
            let laengs = rel.try_normalize().unwrap_or(Vec3::NEG_Y);
            let quer = laengs.cross(-mitte).try_normalize().unwrap_or(cam_right);
            let mut streckung = (rel.length() / d.fall.max(1e-3)).clamp(0.25, 1.0);
            if let Some((vorher, jetzt)) = d.hit {
                streckung = streckung.min(vorher + (jetzt - vorher) * tick_progress);
            }
            if streckung <= 0.0 {
                continue;
            }
            let (achse_a, achse_b) = (quer * d.size, laengs * d.size * streckung);
            let [u0, v0, u1, v1] = layout.rect(d.sprite, art.heavy);
            // Was schon in den Boden faellt: das untere Ende an der Bodenkante abschneiden.
            let mut unten = 1.0f32;
            if achse_b.y < -1e-6 {
                let boden = (d.ground - eye.y) as f32;
                unten = unten.min((mitte.y - boden) / -achse_b.y);
                if unten <= -1.0 {
                    continue;
                }
            }
            let v_unten = v0 + (v1 - v0) * (unten + 1.0) * 0.5;
            let color = [d.color[0], d.color[1], d.color[2], alpha];
            let ecke = |a: f32, b: f32, u: f32, v: f32| WeatherVertex { position: (mitte + achse_a * a + achse_b * b).to_array(), color, uv: [u, v], radii };
            // Oben = gegen die Bewegung (achse_b zeigt nach unten).
            let (lo, ro, ru, lu) = (ecke(-1.0, -1.0, u0, v0), ecke(1.0, -1.0, u1, v0), ecke(1.0, unten, u1, v_unten), ecke(-1.0, unten, u0, v_unten));
            out.extend_from_slice(&[lo, ro, ru, lo, ru, lu]);
        }
        let regen = out.len() as u32;

        let layout = layouts[1];
        if layout.rows > 0 {
            // Keine Kugelgrenze im Shader: der Bereich reicht unter Daechern weiter, und die
            // Flocken verschwinden an seinem Rand hart (s. oben).
            let radii = [0.0, 0.0];
            let kachel = |sprite: u8, heavy: bool| if heavy { Some(layout.rect(sprite % 4, true)) } else { (u32::from(sprite) < layout.normal_count()).then(|| layout.rect(sprite, false)) };
            if intensity > 0.0 {
                for f in &self.snow.flakes {
                    let sprite = (flake_random(f.id, 0) * 16.0) as u8;
                    let heavy = self.storm && flake_random(f.id, 1) < HEAVY_SHARE;
                    let Some(rect) = kachel(sprite, heavy) else { continue };
                    let versatz = if heavy { layout.offsets[16 + usize::from(sprite % 4)] } else { layout.offsets[usize::from(sprite)] };
                    let pos = f.prev + (f.pos - f.prev) * tp;
                    // Dreht sich beim Fallen.
                    let roll = flake_random(f.id, 2) * std::f32::consts::TAU + ARTEN[2].rotation * (flake_random(f.id, 3) - 0.5) * 2.0 * pos.y as f32;
                    snow_quad(out, (pos - eye).as_vec3(), roll, snow_art(heavy).size, cam_right, cam_up, rect, versatz, intensity, radii);
                }
            }
        }
        [regen, out.len() as u32 - regen]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KUGEL: Bounds = Bounds { rain: 16, snow: SNOW_RADIUS, snow_max: SNOW_RADIUS_OUTER };

    fn regen(_: i32, _: i32, _: i32) -> Option<Kind> {
        Some(Kind::Rain)
    }

    // Der Punkt einer Kachel sitzt nicht in ihrer Mitte: gezeichnet liegt er trotzdem dort, wo
    // die Flocke ist (auch gedreht), in jeder Kachel, auch der schweren.
    #[test]
    fn schneepunkt_liegt_auf_der_flocke() {
        // Zwei Kacheln zu 8 x 8 Pixeln, je ein Punkt in einer Ecke bzw. am Rand.
        let (breite, hoehe) = (16u32, 8u32);
        let mut bild = vec![0u8; (breite * hoehe * 4) as usize];
        let punkte = [(6u32, 1u32), (8 + 1, 7)];
        for (x, y) in punkte {
            bild[((y * breite + x) * 4) as usize..][..4].copy_from_slice(&[255; 4]);
        }
        let (_, _, _, layout) = weather_sheet(&bild, breite, hoehe, false).unwrap();
        assert_eq!(layout.rows, 1);
        let flocke = Vec3::new(3.0, 70.0, -2.0);
        for (kachel, (x, y)) in punkte.into_iter().enumerate() {
            assert!(layout.offsets[kachel][0].abs() > 0.2 && layout.offsets[kachel][1].abs() > 0.2, "{:?}", layout.offsets[kachel]);
            let mut out = Vec::new();
            snow_quad(&mut out, flocke, 1.0, 0.75, Vec3::X, Vec3::Y, layout.rect(kachel as u8, false), layout.offsets[kachel], 1.0, [0.0; 2]);
            let (lo, ro, lu) = (Vec3::from(out[0].position), Vec3::from(out[1].position), Vec3::from(out[5].position));
            // Mitte des Pixels in der Kachel (0..1).
            let (fu, fv) = (((x % 8) as f32 + 0.5) / 8.0, (y as f32 + 0.5) / 8.0);
            let punkt = lo + (ro - lo) * fu + (lu - lo) * fv;
            assert!((punkt - flocke).length() < 1e-4, "Kachel {kachel}: Punkt {punkt:?} statt {flocke:?}");
        }
        // Die schweren Kacheln legen Kacheln zusammen: hier je eine, der Punkt bleibt, wo er war.
        assert_eq!(layout.offsets[16], layout.offsets[0]);
        // Ohne Verschieben laege der Punkt weit daneben.
        let mut out = Vec::new();
        snow_quad(&mut out, flocke, 1.0, 0.75, Vec3::X, Vec3::Y, layout.rect(0, false), [0.0; 2], 1.0, [0.0; 2]);
        let (lo, ro, lu) = (Vec3::from(out[0].position), Vec3::from(out[1].position), Vec3::from(out[5].position));
        assert!((lo + (ro - lo) * (6.5 / 8.0) + (lu - lo) * (1.5 / 8.0) - flocke).length() > 0.5);
    }

    fn schnee(_: i32, _: i32, _: i32) -> Option<Kind> {
        Some(Kind::Snow)
    }

    fn weiss(_: i32, _: i32, _: i32) -> [f32; 3] {
        [1.0; 3]
    }

    fn ticks(p: &mut Precipitation, n: u32, eye: DVec3, rain: f32, kind: impl Fn(i32, i32, i32) -> Option<Kind> + Copy, top: impl Fn(i32, i32) -> Option<i32> + Copy) {
        for _ in 0..n {
            p.tick(eye, KUGEL, DENSITY_DEFAULT, SNOW_SPEED_DEFAULT, rain, 0.0, kind, weiss, top, |x, y, z| top(x, z).is_some_and(|t| y <= t).then_some(1.0), top, |_, _| 0);
        }
    }

    // Anzahl gezeichneter Schneeflocken (Quads).
    fn flocken_im_bild(p: &Precipitation, eye: DVec3, tp: f32) -> u32 {
        let mut out = Vec::new();
        let [_, s] = p.build(eye, tp, Vec3::ZERO, Vec3::X, Vec3::Y, KUGEL, 1.0, [SheetLayout { rows: 8, offsets: [[0.0; 2]; 20] }; 2], &mut out);
        s / 6
    }

    // Es regnet: nach ein paar Ticks gibt es Tropfen, sie fallen und bleiben im Radius.
    #[test]
    fn tropfen_entstehen_und_fallen() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        ticks(&mut p, 40, eye, 1.0, regen, |_, _| Some(60));
        assert!(p.drop_count() > 100, "nur {} Tropfen", p.drop_count());
        assert!(p.drops.iter().all(|d| d.vel.y < 0.0 && (d.pos - eye).length() <= 16.5));
        let mut out = Vec::new();
        let [r, s] = p.build(eye, 0.5, Vec3::ZERO, Vec3::X, Vec3::Y, KUGEL, 1.0, [SheetLayout { rows: 8, offsets: [[0.0; 2]; 20] }; 2], &mut out);
        assert!(r > 0 && s == 0 && r % 6 == 0);
    }

    // Ein Dach ueber allem: es entsteht kein Regen und kein sichtbarer Schnee darunter.
    #[test]
    fn dach_haelt_den_regen_ab() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 70.0, 0.5);
        ticks(&mut p, 40, eye, 1.0, regen, |_, _| Some(200));
        assert_eq!(p.drop_count(), 0);
        let mut p = Precipitation::new();
        ticks(&mut p, 40, eye, 1.0, schnee, |_, _| Some(200));
        assert_eq!(flocken_im_bild(&p, eye, 0.5), 0);
    }

    // Kein Regen: keine neuen Tropfen; die alten fallen zu Ende und verschwinden.
    #[test]
    fn ohne_regen_leert_es_sich() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        ticks(&mut p, 20, eye, 1.0, regen, |_, _| Some(95));
        assert!(p.drop_count() > 0);
        ticks(&mut p, 200, eye, 0.0, regen, |_, _| Some(95));
        assert_eq!(p.drop_count(), 0);
    }

    // Tropfen schrumpfen am Boden weg statt durch ihn hindurch zu fallen.
    #[test]
    fn tropfen_bleiben_ueber_dem_boden() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        for _ in 0..100 {
            ticks(&mut p, 1, eye, 1.0, regen, |_, _| Some(95));
            assert!(p.drops.iter().all(|d| d.pos.y >= 95.0), "Tropfen unter dem Boden");
        }
    }

    // Schnee faellt viel langsamer als Regen (Schneetempo), und im Schneebiom entsteht kein Regen.
    #[test]
    fn schnee_langsamer() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        ticks(&mut p, 30, eye, 1.0, schnee, |_, _| Some(60));
        assert_eq!(p.drop_count(), 0);
        assert!(flocken_im_bild(&p, eye, 0.5) > 0);
        // Ohne die eben entstandenen (die sind noch nicht gefallen).
        let alt: Vec<_> = p.snow.flakes.iter().filter(|f| f.prev != f.pos).collect();
        assert!(alt.len() > 100, "{}", alt.len());
        for f in alt {
            assert!((f.prev.y - f.pos.y - 0.05).abs() < 1e-9, "Schnee faellt {} je Tick", f.prev.y - f.pos.y);
        }
    }

    // Im Stehen unter freiem Himmel: nach dem Einschwingen etwa so viele Flocken wie
    // DROPS_AT_DEFAULT (am Kugelrand wandern welche hinaus, s. snow_flakes), und nach dem
    // Schnee ist alles weg.
    #[test]
    fn schnee_im_freien() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        ticks(&mut p, 600, eye, 1.0, schnee, |_, _| Some(60));
        let n = flocken_im_bild(&p, eye, 0.5) as usize;
        assert!(n > DROPS_AT_DEFAULT * 75 / 100 && n < DROPS_AT_DEFAULT * 105 / 100, "{n} Flocken");
        ticks(&mut p, 600, eye, 0.0, schnee, |_, _| Some(60));
        assert!(p.snow.flakes.is_empty());
    }

    // Wo das Biom regnet, gibt es keine Flocke (eine Haelfte Schnee, eine Regen).
    #[test]
    fn kein_schnee_im_regenbiom() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        let haelfte = |x: i32, _: i32, _: i32| Some(if x < 0 { Kind::Snow } else { Kind::Rain });
        ticks(&mut p, 300, eye, 1.0, haelfte, |_, _| Some(60));
        assert!(p.snow.flakes.len() > 300, "{}", p.snow.flakes.len());
        assert!(p.snow.flakes.iter().all(|f| f.pos.x < 0.0), "Flocke im Regenbiom");
    }

    // Weniger Regen, weniger Schnee: bei halber Staerke etwa die Haelfte.
    #[test]
    fn schwacher_schnee_zeigt_weniger() {
        let zaehle = |staerke: f32| {
            let mut p = Precipitation::new();
            let eye = DVec3::new(0.5, 100.0, 0.5);
            ticks(&mut p, 600, eye, staerke, schnee, |_, _| Some(60));
            flocken_im_bild(&p, eye, 0.5)
        };
        let (halb, voll) = (zaehle(0.5), zaehle(1.0));
        assert!(halb * 100 > voll * 40 && halb * 100 < voll * 60, "{halb} / {voll}");
    }

    // Regen wird erst an der Bodenkante abgeschnitten, nicht schon darueber unsichtbar.
    #[test]
    fn regen_endet_an_der_bodenkante() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        ticks(&mut p, 1, eye, 1.0, regen, |_, _| Some(95));
        let mut d = p.drops[0];
        d.vel = Vec3::new(0.0, -1.0, 0.0);
        d.pos = DVec3::new(5.5, 96.3, 0.5);
        d.prev = d.pos;
        d.ground = 96.0;
        d.alpha = 1.0;
        p.drops = vec![d];
        let mut out = Vec::new();
        p.build(eye, 0.0, Vec3::ZERO, Vec3::X, Vec3::Y, KUGEL, 1.0, [SheetLayout { rows: 8, offsets: [[0.0; 2]; 20] }; 2], &mut out);
        assert_eq!(out.len(), 6);
        let ys: Vec<f32> = out.iter().map(|v| v.position[1] + 100.0).collect();
        let tiefste = ys.iter().copied().fold(f32::MAX, f32::min);
        let hoechste = ys.iter().copied().fold(f32::MIN, f32::max);
        assert!((tiefste - 96.0).abs() < 1e-4, "unten {tiefste}");
        assert!((hoechste - 97.05).abs() < 1e-4, "oben {hoechste}");
    }

    // Mehr Menge, mehr Tropfen: bei 140 % entstehen etwa doppelt so viele wie bei 70 %.
    #[test]
    fn menge_skaliert_die_tropfen() {
        let zaehle = |menge: u32| {
            let mut p = Precipitation::new();
            let eye = DVec3::new(0.5, 100.0, 0.5);
            for _ in 0..10 {
                p.tick(eye, KUGEL, menge, SNOW_SPEED_DEFAULT, 1.0, 0.0, regen, weiss, |_, _| Some(0), |_, y, _| (y <= 0).then_some(1.0), |_, _| Some(0), |_, _| 0);
            }
            p.drop_count()
        };
        let (wenig, viel) = (zaehle(DENSITY_DEFAULT), zaehle(DENSITY_DEFAULT * 2));
        assert!(viel as f32 > wenig as f32 * 1.7, "{wenig} -> {viel}");
    }

    // Regen entsteht oben am Kugelrand, nicht auf Augenhoehe.
    #[test]
    fn regen_entsteht_oben() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        ticks(&mut p, 2, eye, 1.0, regen, |_, _| Some(0));
        // Die eben entstandenen (noch nicht bewegt).
        let neu: Vec<_> = p.drops.iter().filter(|d| d.age == 0).collect();
        assert!(neu.len() > 100, "{}", neu.len());
        for d in neu {
            let waagerecht = (d.pos.x - eye.x).hypot(d.pos.z - eye.z);
            let oben = eye.y + (256.0 - waagerecht * waagerecht).max(0.0).sqrt();
            assert!((d.pos.y - oben).abs() < 0.02, "Tropfen bei {} statt {oben}", d.pos.y);
        }
    }

    // Das Blatt: Vanillas 64x256 gibt 8 Zeilen zu je zwei 32er-Kacheln, dazu zwei Zeilen
    // schwerer Kacheln; Regen wird hellgrau, schwache Deckkraft hochgezogen.
    #[test]
    fn blatt_aus_vanilla_format() {
        let (w, h) = (64u32, 256u32);
        let mut bild = vec![0u8; (w * h * 4) as usize];
        // Kachel 0 (oben links) bekommt einen blauen, halb deckenden Pixel.
        bild[..4].copy_from_slice(&[40, 80, 200, 100]);
        let (out, ow, oh, layout) = weather_sheet(&bild, w, h, true).unwrap();
        assert_eq!((ow, oh, layout.rows), (64, 320, 8));
        assert_eq!(&out[..4], &[255, 255, 255, 200]);
        // Schwere Kachel 0 (Spalte 0, Zeile 8) enthaelt Kachel 0.
        let i = (8 * 32 * 64 * 4) as usize;
        assert_eq!(&out[i..i + 4], &[255, 255, 255, 200]);
        let nah = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(nah(layout.rect(0, true), [0.0, 0.8, 0.5, 0.9]));
        assert!(nah(layout.rect(9, false), [0.5, 0.1, 1.0, 0.2]));
        assert!(weather_sheet(&bild, 1, 1, false).is_none());
    }

    // Regen steht laengs seiner Bewegung: bei senkrechtem Fall ist das Quad senkrecht.
    #[test]
    fn regen_steht_laengs() {
        let mut p = Precipitation::new();
        let eye = DVec3::new(0.5, 100.0, 0.5);
        ticks(&mut p, 1, eye, 1.0, regen, |_, _| Some(0));
        let mut d = p.drops[0];
        d.vel = Vec3::new(0.0, -1.0, 0.0);
        d.pos = eye + DVec3::new(5.0, 0.0, 0.0);
        d.prev = d.pos;
        d.alpha = 1.0;
        p.drops = vec![d];
        let mut out = Vec::new();
        p.build(eye, 0.0, Vec3::ZERO, Vec3::X, Vec3::Y, KUGEL, 1.0, [SheetLayout { rows: 8, offsets: [[0.0; 2]; 20] }; 2], &mut out);
        assert_eq!(out.len(), 6);
        let (oben, unten) = (out[0].position, out[2].position);
        assert!((oben[0] - unten[0]).abs() < 1e-5 && (oben[1] - unten[1]).abs() > 1.0, "{oben:?} {unten:?}");
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;

    fn textur(name: &str) -> (Vec<u8>, u32, u32) {
        let jar = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".minecraft/versions/26.2/26.2.jar");
        let bytes = std::process::Command::new("unzip").arg("-p").arg(&jar).arg(format!("assets/minecraft/textures/environment/{name}.png")).output().unwrap().stdout;
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        let (w, h) = img.dimensions();
        (img.into_raw(), w, h)
    }

    #[test]
    #[ignore]
    fn echte_quads_sind_sichtbar() {
        let instance = wgpu::Instance::default();
        let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
        const W: u32 = 256;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let leer = vec![0u8; 16 * 16 * 4];
        let mut r = render::BlockRenderer::new(&device, &queue, format, W, W, std::slice::from_ref(&leer), 16, &leer, 16, &leer, 16);
        let (regen, schnee) = (textur("rain"), textur("snow"));
        let alpha_summe: u64 = schnee.0.chunks(4).map(|p| u64::from(p[3])).sum();
        println!("Schnee-Textur {}x{}, Alpha-Summe {alpha_summe}", schnee.1, schnee.2);
        let regen = weather_sheet(&regen.0, regen.1, regen.2, true).unwrap();
        let schnee = weather_sheet(&schnee.0, schnee.1, schnee.2, false).unwrap();
        r.set_weather_textures(&device, &queue, (&regen.0, regen.1, regen.2), (&schnee.0, schnee.1, schnee.2));
        let layouts = [regen.3, schnee.3];

        let eye = DVec3::new(0.5, 103.6, 16.5);
        let grenzen = Bounds { rain: 16, snow: SNOW_RADIUS, snow_max: SNOW_RADIUS_OUTER };
        for (name, art) in [("Regen", Kind::Rain), ("Schnee", Kind::Snow)] {
            let mut p = Precipitation::new();
            for _ in 0..100 {
                p.tick(eye, grenzen, DENSITY_DEFAULT, SNOW_SPEED_DEFAULT, 1.0, 0.0, |_, _, _| Some(art), |_, _, _| [1.0; 3], |_, _| Some(102), |_, y, _| (y <= 102).then_some(1.0), |_, _| Some(102), |_, _| 0);
            }
            let mut out = Vec::new();
            let mut kamera = render::Camera::new(eye);
            kamera.pitch = -20.0;
            let rechts = kamera.right();
            let oben = rechts.cross(kamera.forward());
            let n = p.build(eye, 0.3, Vec3::ZERO, rechts, oben, grenzen, 1.0, layouts, &mut out);
            r.set_weather_vertices(&queue, &out, n[0], n[1]);
            r.update_camera(&queue, &kamera, 1.0);
            let ziel = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let zv = ziel.create_view(&Default::default());
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &zv,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(r.depth_attachment()),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                r.render(&mut pass, &mut |_, _| {});
            }
            let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(W * W * 4), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            enc.copy_texture_to_buffer(
                ziel.as_image_copy(),
                wgpu::ImageCopyBuffer { buffer: &lese, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(W * 4), rows_per_image: Some(W) } },
                wgpu::Extent3d { width: W, height: W, depth_or_array_layers: 1 },
            );
            queue.submit([enc.finish()]);
            lese.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::Maintain::Wait);
            let daten = lese.slice(..).get_mapped_range().to_vec();
            let hell = daten.chunks(4).filter(|p| p[0] > 20).count();
            println!("{name}: {n:?} Vertices, {hell} von {} Pixeln nicht schwarz, erster Vertex {:?}", W * W, out.first());
            // Einzelne Tropfen sind duenne Striche: weit weniger Pixel als Vanillas Streifen.
            assert!(hell > 100, "{name} kaum sichtbar");
        }
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    // Rechenzeit je Tick (cargo test --release ... -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn schnee_zeit_je_tick() {
        for snow_max in [SNOW_RADIUS_OUTER, SNOW_RADIUS_OUTER_MAX] {
        let bounds = Bounds { rain: 16, snow: SNOW_RADIUS, snow_max };
        for (name, dach) in [("offen", false), ("unter Dach", true)] {
            let fest = move |x: i32, y: i32, z: i32| y <= 60 || (dach && y == 103 && x.abs() <= 15 && z.abs() <= 15);
            let top = move |x: i32, z: i32| (60..200).rev().find(|&y| fest(x, y, z));
            let mut p = Precipitation::new();
            let mut eye = DVec3::new(0.5, 100.0, 0.5);
            let tick = |p: &mut Precipitation, eye: DVec3| {
                p.tick(eye, bounds, DENSITY_DEFAULT, SNOW_SPEED_DEFAULT, 1.0, 0.0, |_, _, _| Some(Kind::Snow), |_, _, _| [1.0; 3], top, |x, y, z| fest(x, y, z).then_some(1.0), top, |_, _| 0);
            };
            let t = std::time::Instant::now();
            tick(&mut p, eye);
            let erster = t.elapsed();
            for _ in 0..600 {
                tick(&mut p, eye);
            }
            p.snow_map.diag_zeit = Default::default();
            p.snow.diag_zeit = Default::default();
            let t = std::time::Instant::now();
            for _ in 0..100 {
                tick(&mut p, eye);
            }
            let still = t.elapsed() / 100;
            println!("  still gesamt: fuellen {:?} bereich {:?} flocken {:?} {:?} spawnbloecke {} flocken {} bereich {}", p.snow_map.diag_zeit[0], p.snow_map.diag_zeit[1], p.snow_map.diag_zeit[2], p.snow.diag_zeit, p.snow.spawn.len(), p.snow.flakes.len(), p.snow_map.region_blocks().count());
            p.snow.diag_zeit = Default::default();
            // Sprinten: 5,6 Bloecke je Sekunde.
            p.snow_map.diag_zeit = Default::default();
            let (t, mut laengster) = (std::time::Instant::now(), std::time::Duration::ZERO);
            for _ in 0..100 {
                eye.x += 0.28;
                let e = std::time::Instant::now();
                tick(&mut p, eye);
                laengster = laengster.max(e.elapsed());
            }
            let laufen = t.elapsed() / 100;
            let z = std::mem::take(&mut p.snow_map.diag_zeit);
            println!("  laufen flocken {:?}", p.snow.diag_zeit);
            let mut out = Vec::new();
            let t = std::time::Instant::now();
            for _ in 0..100 {
                p.build(eye, 0.5, Vec3::ZERO, Vec3::X, Vec3::Y, bounds, 1.0, [SheetLayout { rows: 8, offsets: [[0.0; 2]; 20] }; 2], &mut out);
            }
            println!("max {snow_max} {name}: erster Tick {erster:?}, still {still:?}/Tick, laufen {laufen:?}/Tick (laengster {laengster:?}), Bild {:?}, {} Flocken; gesamt fuellen {:?} bereich {:?} flocken {:?}", t.elapsed() / 100, p.snow.flakes.len(), z[0], z[1], z[2]);
        }
        }
    }
}
