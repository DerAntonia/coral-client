// Regen und Schnee -- wie Vanillas WeatherEffectRenderer.
//
// Um die Kamera steht in jeder Blockspalte innerhalb des Radius ein senkrechtes Quad, eine
// Blockbreite breit, quer zur Linie Kamera -> Spalte gedreht (man sieht es also immer von
// vorn). Es reicht von Radius-Bloecken unter bis Radius-Bloecke ueber der Kamera, unten
// begrenzt durch den hoechsten Block, der Bewegung blockiert oder Fluessigkeit enthaelt
// (Heightmap MOTION_BLOCKING) -- unter einem Dach regnet es nicht. Ist dieser Block hoeher
// als das obere Ende, entfaellt die Spalte.
//
// Die Textur (textures/environment/rain.png bzw. snow.png) wiederholt sich: je Block ein
// Viertel ihrer Hoehe, die volle Breite auf dem Quad. Bewegt wird nur die UV:
//   - Regen faellt je Tick um (3..4)/32 Bildhoehen, der Faktor und eine Phase sind je Spalte
//     zufaellig, damit benachbarte Spalten nicht im Gleichtakt fallen.
//   - Schnee sinkt langsam (eine Bildhoehe in 512 Ticks) und treibt je Spalte mit eigener,
//     normalverteilter Geschwindigkeit seitlich und etwas schneller oder langsamer nach unten.
// Deckkraft: Regenstaerke mal 1 (Regen) bzw. 0.8 (Schnee) an der Kamera, nach aussen ueber das
// Quadrat des Abstands auf die Haelfte fallend.
//
// Modul "Wetter": statt Vanillas Quader (Quadrat von Spalten, jede +-Radius hoch) eine Kugel
// mit Hohlkugel innen -- der Radius gilt in allen drei Achsen. Die Spalten werden dafuer auf
// die Kugel zugeschnitten (Hoehe je nach waagerechtem Abstand), den genauen, weichen Rand
// schneidet der Fragment-Shader nach dem 3D-Abstand je Pixel (s. render fs_weather).
//
// Was Niederschlag ist, entscheidet je Spalte das Biom auf Kamerahoehe (world::weather::
// precipitation); das Modul "Wetter" kann es fest vorgeben.
//
// BEWUSST ANDERS als Vanilla:
//   - Die V-Koordinate haengt an der Welthoehe, nicht an den Enden des Quads. In Vanilla
//     verschiebt sich das Muster jedesmal, wenn die Kamera einen Block hoeher oder tiefer
//     kommt (die Enden wandern mit); hier steht es still.
//   - Das Quad dreht sich zur echten Kameraposition statt zum Block der Kamera -- es springt
//     beim Gehen nicht von Block zu Block. Damit hat auch die Spalte der Kamera selbst eine
//     Richtung (in Vanilla ist sie dort unbestimmt).
//   - Der Zufall je Spalte kommt aus einem eigenen Hash, nicht aus Vanillas Saat.
//   - Keine Lichtkarte: wie die Partikel zeichnet der Niederschlag mit voller Helligkeit.
//   - Spritzer am Boden und das Regengeraeusch fehlen (noch).

use glam::DVec3;
use render::WeatherVertex;

// Aeusserer Radius: Vanillas Option "Wetterradius" (Vorgabe 10, dort 3..10); das Modul
// "Wetter" laesst bis 16 zu (so viel fasst render::WEATHER_VERTEX_CAPACITY).
pub const RADIUS_MIN: u32 = 3;
pub const RADIUS_DEFAULT: u32 = 10;
pub const RADIUS_MAX: u32 = 16;

// Wo Niederschlag faellt. `sphere` = false: Vanillas Quader mit Kantenlaenge 2 * outer (inner
// gilt dann nicht). `sphere` = true: zwischen zwei Kugeln um die Kamera -- innen frei, damit
// Regen und Schnee nicht vor dem Gesicht haengen (0 = keine innere Kugel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub inner: u32,
    pub outer: u32,
    pub sphere: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rain,
    Snow,
}

// Hoechster blockierender Block je Spalte, einmal pro Tick fuer ein Quadrat um die Kamera.
// Das Bild baut pro Frame darauf auf (die UV laeuft mit dem Teil-Tick weiter).
pub struct Precipitation {
    // Zaehler der Client-Ticks -- die Uhr der Fallbewegung.
    ticks: u64,
    origin: (i32, i32),
    size: i32,
    // i32::MIN = nichts in der Spalte (oder nicht geladen): Niederschlag bis ganz unten.
    tops: Vec<i32>,
}

impl Precipitation {
    pub fn new() -> Self {
        Self { ticks: 0, origin: (0, 0), size: 0, tops: Vec::new() }
    }

    // Ein Client-Tick. `top(x, z)` = Y des hoechsten blockierenden Blocks. `measure` = false
    // spart die Messung, solange nichts faellt -- die Uhr laeuft trotzdem weiter.
    pub fn tick(&mut self, center: (i32, i32), radius: u32, measure: bool, top: impl Fn(i32, i32) -> Option<i32>) {
        self.ticks = self.ticks.wrapping_add(1);
        if !measure {
            self.size = 0;
            return;
        }
        // Ein Block Rand: die Kamera kann bis zum naechsten Tick ueber eine Blockgrenze gehen.
        let r = radius as i32 + 1;
        self.origin = (center.0 - r, center.1 - r);
        self.size = 2 * r + 1;
        self.tops.clear();
        for dz in 0..self.size {
            for dx in 0..self.size {
                self.tops.push(top(self.origin.0 + dx, self.origin.1 + dz).unwrap_or(i32::MIN));
            }
        }
    }

    fn top_at(&self, x: i32, z: i32) -> Option<i32> {
        let (dx, dz) = (x - self.origin.0, z - self.origin.1);
        if dx < 0 || dz < 0 || dx >= self.size || dz >= self.size {
            return None;
        }
        Some(self.tops[(dz * self.size + dx) as usize])
    }

    // Die Quads dieses Frames in `out` (augenrelativ, wie die Partikel): erst alle Regen-,
    // dann alle Schnee-Vertices. Rueckgabe: Anzahl Regen- und Schnee-Vertices.
    pub fn build(
        &self,
        eye: DVec3,
        tick_progress: f32,
        bounds: Bounds,
        intensity: f32,
        kind: impl Fn(i32, i32, i32) -> Option<Kind>,
        out: &mut Vec<WeatherVertex>,
    ) -> [u32; 2] {
        out.clear();
        if intensity <= 0.0 || self.size == 0 {
            return [0, 0];
        }
        let r = bounds.outer as i32;
        let (aussen, innen) = (bounds.outer as f64, bounds.inner as f64);
        let (bx, by, bz) = (eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
        let mut schnee = Vec::new();
        for z in bz - r..=bz + r {
            for x in bx - r..=bx + r {
                let Some(top) = self.top_at(x, z) else { continue };
                // Vanillas Heightmap zaehlt den Block DARUEBER.
                let boden = top.saturating_add(1) as f64;
                let (unten, oben) = if bounds.sphere {
                    // Das Quad steht quer zur Linie Kamera -> Spaltenmitte, die Mitte ist also
                    // sein naechster Punkt: waagerecht `d` entfernt.
                    let d_sq = (x as f64 + 0.5 - eye.x).powi(2) + (z as f64 + 0.5 - eye.z).powi(2);
                    if d_sq >= aussen * aussen {
                        continue;
                    }
                    let h = (aussen * aussen - d_sq).sqrt();
                    let (unten, oben) = ((eye.y - h).max(boden), eye.y + h);
                    // Ganz in der inneren Kugel (auch die Ecken, 0.5 seitlich): nichts zu sehen.
                    let fern_sq = d_sq + 0.25 + (oben - eye.y).abs().max((unten - eye.y).abs()).powi(2);
                    if fern_sq <= innen * innen {
                        continue;
                    }
                    (unten, oben)
                } else {
                    ((by - r).max(top.saturating_add(1)) as f64, (by + r).max(top.saturating_add(1)) as f64)
                };
                if oben <= unten {
                    continue;
                }
                let Some(art) = kind(x, by, z) else { continue };
                let ziel = if art == Kind::Rain { &mut *out } else { &mut schnee };
                self.push_column(ziel, eye, tick_progress, bounds, intensity, art, x, z, unten, oben);
            }
        }
        let regen = out.len() as u32;
        out.extend_from_slice(&schnee);
        [regen, schnee.len() as u32]
    }

    #[allow(clippy::too_many_arguments)]
    fn push_column(
        &self,
        out: &mut Vec<WeatherVertex>,
        eye: DVec3,
        tick_progress: f32,
        bounds: Bounds,
        intensity: f32,
        kind: Kind,
        x: i32,
        z: i32,
        bottom: f64,
        top: f64,
    ) {
        let cx = (x as f64 + 0.5 - eye.x) as f32;
        let cz = (z as f64 + 0.5 - eye.z) as f32;
        let abstand_sq = cx * cx + cz * cz;
        // Halbe Breite quer zur Blickrichtung auf die Spalte.
        let (hx, hz) = if abstand_sq > 1e-6 {
            let len = abstand_sq.sqrt();
            (-cz / len * 0.5, cx / len * 0.5)
        } else {
            (0.5, 0.0)
        };
        let nah = match kind {
            Kind::Rain => 1.0,
            Kind::Snow => 0.8,
        };
        let radius = bounds.outer as f32;
        let aussen = (abstand_sq / (radius * radius)).min(1.0);
        let alpha = (nah + (0.5 - nah) * aussen) * intensity;
        let (u0, v0) = column_offsets(kind, x, z, self.ticks, tick_progress);
        let y_oben = (top - eye.y) as f32;
        let y_unten = (bottom - eye.y) as f32;
        // V waechst nach unten; ein fester Punkt des Musters sitzt bei y = 4 * (v0 - v).
        let v_oben = v0 - top as f32 * 0.25;
        let v_unten = v0 - bottom as f32 * 0.25;
        let color = [1.0, 1.0, 1.0, alpha];
        let radii = if bounds.sphere { [bounds.inner as f32, radius] } else { [0.0; 2] };
        let ecke = |s: f32, y: f32, v: f32| WeatherVertex { position: [cx + s * hx, y, cz + s * hz], color, uv: [u0 + (s + 1.0) * 0.5, v], radii };
        let (lo, ro, ru, lu) = (ecke(-1.0, y_oben, v_oben), ecke(1.0, y_oben, v_oben), ecke(1.0, y_unten, v_unten), ecke(-1.0, y_unten, v_unten));
        out.extend_from_slice(&[lo, ro, ru, lo, ru, lu]);
    }
}

// Verschiebung der Textur einer Spalte (U, V) zur Zeit `ticks` + `tick_progress`.
fn column_offsets(kind: Kind, x: i32, z: i32, ticks: u64, tick_progress: f32) -> (f32, f32) {
    let mut zufall = ColumnRandom::new(x, z);
    match kind {
        Kind::Rain => {
            // Periode 2^17 Ticks wie Vanilla; der Rest mod 32 haelt die Zahl klein (32 ganze
            // Bildhoehen -- das Muster wiederholt sich ohnehin nach einer).
            let phase = (zufall.next() * 256.0).floor();
            let tempo = 3.0 + zufall.next();
            let t = (ticks % 131_072) as f32 + phase + tick_progress;
            (0.0, -(t / 32.0 * tempo) % 32.0)
        }
        Kind::Snow => {
            let t = (ticks % 131_072) as f32 + tick_progress;
            let u = zufall.next() + t * 0.01 * zufall.gaussian();
            let v = zufall.next() + t * 0.001 * zufall.gaussian() - ((ticks % 512) as f32 + tick_progress) / 512.0;
            (u, v)
        }
    }
}

// Fester Zufall je Spalte (splitmix64 ueber die Koordinaten): dieselbe Spalte faellt jeden
// Frame gleich.
struct ColumnRandom(u64);

impl ColumnRandom {
    fn new(x: i32, z: i32) -> Self {
        Self(((x as u32 as u64) << 32 | z as u32 as u64) ^ 0x5745_5454_4552_0001)
    }

    fn next_u64(&mut self) -> u64 {
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

    // Standardnormalverteilt (Box-Muller).
    fn gaussian(&mut self) -> f32 {
        let a = 1.0 - self.next();
        let b = self.next();
        (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offen(_: i32, _: i32) -> Option<i32> {
        None
    }

    fn quader(outer: u32) -> Bounds {
        Bounds { inner: 0, outer, sphere: false }
    }

    #[test]
    fn dach_haelt_den_regen_ab() {
        let mut p = Precipitation::new();
        // Ueber (0, 0) ein Dach auf y = 80, sonst nichts.
        p.tick((0, 0), 3, true, |x, z| (x == 0 && z == 0).then_some(80));
        let mut out = Vec::new();
        let eye = DVec3::new(0.5, 70.0, 0.5);
        let [regen, schnee] = p.build(eye, 0.0, quader(3), 1.0, |_, _, _| Some(Kind::Rain), &mut out);
        assert_eq!(schnee, 0);
        // 7 x 7 Spalten, die unter dem Dach faellt weg (Dach 81 > 70 + 3).
        assert_eq!(regen, (7 * 7 - 1) * 6);
        // Jede Spalte reicht von 67 bis 73 (Kamera +- Radius).
        for v in &out {
            assert!(v.position[1] == -3.0 || v.position[1] == 3.0, "{v:?}");
        }
    }

    #[test]
    fn boden_begrenzt_unten_und_arten_getrennt() {
        let mut p = Precipitation::new();
        p.tick((0, 0), 3, true, |_, _| Some(68));
        let mut out = Vec::new();
        let eye = DVec3::new(0.5, 70.0, 0.5);
        // Westhaelfte Schnee, Osthaelfte Regen, Spalte x = 0 nichts.
        let art = |x: i32, _: i32, _: i32| match x {
            0 => None,
            x if x < 0 => Some(Kind::Snow),
            _ => Some(Kind::Rain),
        };
        let [regen, schnee] = p.build(eye, 0.5, quader(3), 1.0, art, &mut out);
        assert_eq!((regen, schnee), (3 * 7 * 6, 3 * 7 * 6));
        // Boden = 68 + 1: unten bei y = 69 statt 67.
        let tiefste = out.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        assert_eq!(tiefste, -1.0);
        // Schnee ist an der Kamera blasser als Regen.
        let a_regen = out[..regen as usize].iter().map(|v| v.color[3]).fold(0.0, f32::max);
        let a_schnee = out[regen as usize..].iter().map(|v| v.color[3]).fold(0.0, f32::max);
        assert!(a_regen > a_schnee && a_schnee <= 0.8, "{a_regen} {a_schnee}");
    }

    #[test]
    fn ohne_staerke_oder_messung_nichts() {
        let mut p = Precipitation::new();
        let mut out = Vec::new();
        p.tick((0, 0), 3, false, offen);
        assert_eq!(p.build(DVec3::ZERO, 0.0, quader(3), 1.0, |_, _, _| Some(Kind::Rain), &mut out), [0, 0]);
        p.tick((0, 0), 3, true, offen);
        assert_eq!(p.build(DVec3::ZERO, 0.0, quader(3), 0.0, |_, _, _| Some(Kind::Rain), &mut out), [0, 0]);
    }

    // Regen faellt: ein fester Punkt des Musters (y = 4 * (v0 - v)) sinkt mit der Zeit, und
    // zwar um (3..4)/32 Bildhoehen = (3..4)/8 Bloecke je Tick. Schnee viel langsamer.
    #[test]
    fn regen_faellt_nach_unten_schnee_langsamer() {
        let fall = |kind, ticks: u64| {
            let (_, a) = column_offsets(kind, 5, -9, ticks, 0.0);
            let (_, b) = column_offsets(kind, 5, -9, ticks + 1, 0.0);
            (b - a) * 4.0
        };
        let regen = fall(Kind::Rain, 1000);
        assert!((-0.5..=-0.375).contains(&regen), "Regen je Tick {regen} Bloecke");
        // Schnee: im Mittel 4/512 Bloecke je Tick, je Spalte gestreut.
        let schnitt = (0..200).map(|i| column_offsets(Kind::Snow, i, 3, 1001, 0.0).1 - column_offsets(Kind::Snow, i, 3, 1000, 0.0).1).sum::<f32>() / 200.0 * 4.0;
        assert!((-0.012..=-0.004).contains(&schnitt), "Schnee je Tick {schnitt} Bloecke");
    }

    // Kugel: keine Spalte ausserhalb, die Hoehe schrumpft nach aussen, ganz innen nichts,
    // und der Shader bekommt die Radien fuer den genauen Rand.
    #[test]
    fn kugel_schneidet_in_allen_achsen() {
        let mut p = Precipitation::new();
        p.tick((0, 0), 8, true, offen);
        let eye = DVec3::new(0.5, 70.0, 0.5);
        let mut quads = Vec::new();
        let kugel = Bounds { inner: 4, outer: 8, sphere: true };
        p.build(eye, 0.0, kugel, 1.0, |_, _, _| Some(Kind::Rain), &mut quads);
        let mut quader_quads = Vec::new();
        p.build(eye, 0.0, quader(8), 1.0, |_, _, _| Some(Kind::Rain), &mut quader_quads);
        assert!(quads.len() < quader_quads.len(), "Ecken des Quaders fallen weg");
        for q in quads.chunks(6) {
            // Vertex 0 oben, 2 unten; die Mitte der Oberkante aus 0 und 1.
            let d = ((q[0].position[0] + q[1].position[0]) / 2.0).hypot((q[0].position[2] + q[1].position[2]) / 2.0);
            assert!(d < 8.0, "Spalte bei {d} ausserhalb der Kugel");
            let h = (64.0 - d * d).sqrt();
            assert!((q[0].position[1] - h).abs() < 1e-4 && (q[2].position[1] + h).abs() < 1e-4, "Hoehe +-{h}: {:?}", q[0].position);
            assert_eq!(q[0].radii, [4.0, 8.0]);
        }
        assert!(quader_quads.iter().all(|v| v.radii == [0.0; 2]), "ohne Kugel schneidet der Shader nichts");
        // Eine Spalte, die ganz in der inneren Kugel liegt: Kugel aussen knapp, innen gross.
        let mut eng = Vec::new();
        p.build(eye, 0.0, Bounds { inner: 7, outer: 5, sphere: true }, 1.0, |_, _, _| Some(Kind::Rain), &mut eng);
        assert!(eng.is_empty(), "alles in der inneren Kugel");
    }

    // Das Quad steht quer zur Linie Kamera -> Spalte.
    #[test]
    fn quad_steht_quer_zur_blickrichtung() {
        let mut p = Precipitation::new();
        p.tick((0, 0), 3, true, offen);
        let mut out = Vec::new();
        p.build(DVec3::new(0.5, 70.0, 0.5), 0.0, quader(3), 1.0, |x, _, z| (x == 2 && z == 0).then_some(Kind::Rain), &mut out);
        assert_eq!(out.len(), 6);
        let (a, b) = (out[0].position, out[1].position);
        // Spalte liegt in +X: das Quad laeuft entlang Z.
        assert!((a[0] - b[0]).abs() < 1e-6 && (a[2] - b[2]).abs() > 0.99);
    }
}

// Diagnose auf der echten GPU mit den echten Texturen aus dem Minecraft-Jar: dieselben Quads wie
// im Spiel, durch den ganzen BlockRenderer. Zaehlt die Pixel, die nicht schwarz geblieben sind.
// Braucht GPU und ~/.minecraft/versions/26.2/26.2.jar, deshalb #[ignore].
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
        let alpha_summe: u64 = schnee.0.chunks(4).map(|p| p[3] as u64).sum();
        println!("Schnee-Textur {}x{}, Alpha-Summe {alpha_summe}", schnee.1, schnee.2);
        r.set_weather_textures(&device, &queue, (&regen.0, regen.1, regen.2), (&schnee.0, schnee.1, schnee.2));

        let eye = DVec3::new(0.5, 103.6, 16.5);
        let mut p = Precipitation::new();
        for _ in 0..100 {
            p.tick((0, 16), 16, true, |_, _| Some(102));
        }
        for (name, art) in [("Regen", Kind::Rain), ("Schnee", Kind::Snow)] {
            let mut out = Vec::new();
            let n = p.build(eye, 0.3, Bounds { inner: 0, outer: 16, sphere: true }, 1.0, |_, _, _| Some(art), &mut out);
            r.set_weather_vertices(&queue, &out, n[0], n[1]);
            let mut kamera = render::Camera::new(eye);
            kamera.pitch = -20.0;
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
            let lese = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (W * W * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
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
            assert!(hell > 1000, "{name} kaum sichtbar");
        }
    }
}
