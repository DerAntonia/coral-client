// Partikel -- Vanillas ParticleManager samt der Partikelarten, die im Kampf zaehlen.
//
// Alles hier ist aus dem Yarn-1.21.11-Jar abgeschrieben (Particle, BillboardParticle,
// DamageParticle, TotemParticle/AnimatedParticle, ExplosionLargeParticle,
// ExplosionEmitterParticle, ExplosionSmokeParticle, FireSmokeParticle/AscendingParticle,
// RedDustParticle/AbstractDustParticle, SweepAttackParticle, BlockDustParticle,
// EmitterParticle, ClientWorld.addParticle/addBlockBreakParticles/spawnBlockBreakingParticle,
// BlockParticleEffectsManager). Die Reihenfolge der Zufallszuege folgt Vanilla, damit die
// Verteilungen stimmen -- die Werte selbst sind natuerlich andere als auf einem echten Client.
//
// Bewusst NICHT drin: die Lichtkarte. Der Client rendert fullbright (s. Speicher-Notiz), also
// traegt jeder Partikel volle Helligkeit -- auch die, die in Vanilla im Dunkeln abdunkeln.
//
// Nicht umgesetzte Typen (Flammen, Herzen, Noten, ...) werden still verworfen; welche das im
// laufenden Spiel waren, zaehlt `unsupported` fuer die Debug-Anzeige.

use std::collections::VecDeque;

use glam::{DVec3, Vec3};
use protocol::particle::{self as pp, ParticleEffect, ParticleOptions};
use render::{ParticleDraws, ParticleVertex};
use rustc_hash::FxHashMap as HashMap;

// Motion Blur der Partikel (s. Particles::build_vertices): Eingabe `ticks` (Zeit seit dem
// vorigen Bild in Ticks) und `eye_step` (Auge jetzt minus Auge im vorigen Bild), Ausgabe je
// Ecke die Lage im vorigen Bild und ob sich ueberhaupt ein Partikel bewegt hat.
#[derive(Default)]
pub struct ParticlePrev {
    pub ticks: f32,
    pub eye_step: Vec3,
    pub positions: Vec<[f32; 3]>,
    pub moving: bool,
}

mod kinds;
mod spawners;
mod ticking;

// EvictingQueue-Groesse je Blatt in Vanillas ParticleRenderer: bei vollem Blatt verdraengt ein
// neuer Partikel den aeltesten.
const MAX_PER_SHEET: usize = 16_384;
// ClientWorld.addParticle: ohne `force` entstehen Partikel nur bis 32 Bloecke um die Kamera.
// Hier einstellbar (Particles::spawn_distance), das ist der Vanilla-Wert.
const VANILLA_SPAWN_DISTANCE: f64 = 32.0;
// Particle.MAX_SQUARED_COLLISION_CHECK_DISTANCE -- schnellere Partikel pruefen keine Kollision.
const MAX_COLLISION_MOVE_SQ: f64 = 100.0 * 100.0;
// BlockParticleEffectsManager.field_62022: hoechstens so viele Explosions-Blockpartikel je Tick.
const MAX_EXPLOSION_BLOCK_PARTICLES: i32 = 512;
// Rueckstoss einer Explosion auf Partikel (kein Vanilla, s. ExplosionPush): so viel staerker
// als bei einem Entity -- Partikel sind leicht.
const EXPLOSION_PUSH_FACTOR: f64 = 2.0;

// Der Stoss einer Explosion (Zentrum, Radius aus dem Paket) auf leichte Koerper (Partikel,
// Schneeflocken), in Bloecken je Tick -- None, wenn einer ausser Reichweite oder abgeschirmt
// ist. Gerechnet wie beim Entity (weg vom Zentrum, Staerke 1 - Abstand / doppelter Radius),
// mal EXPLOSION_PUSH_FACTOR. Die Deckung ist bewusst sehr grob: EIN Strahl vom Zentrum zum
// Koerper, und jeder Block mit Kollisionsform dazwischen schirmt ganz ab (Vanilla tastet das
// Entity mit vielen Strahlen ab und stoesst anteilig).
//
// Einmal je Explosion fuer alle Koerper: die Strahlen laufen fast alle durch dieselben Bloecke,
// deshalb merkt sie sich je Block, ob er abschirmt (die Kollisionsform zu bestimmen ist das
// Teure -- ohne das kostete eine Explosion bei vollem Partikelbestand 1,9 ms).
pub(crate) struct ExplosionPush<'a> {
    center: DVec3,
    radius: f32,
    world: &'a world::World,
    shapes: &'a world::BlockShapes,
    // Wuerfel um das Zentrum mit Kantenlaenge 2 * halb + 1: 0 = noch nicht gefragt, 1 = frei,
    // 2 = schirmt ab. Leer bei sehr grossen Explosionen (dann jedes Mal gefragt).
    cells: Vec<u8>,
    ecke: glam::IVec3,
    kante: i32,
}

// Groesster Wuerfel fuer das Gedaechtnis (halbe Kante); groessere Explosionen fragen direkt.
const EXPLOSION_CACHE_HALF_MAX: i32 = 24;

impl<'a> ExplosionPush<'a> {
    pub(crate) fn new(center: DVec3, radius: f32, world: &'a world::World, shapes: &'a world::BlockShapes) -> Self {
        let halb = (f64::from(radius) * 2.0).ceil() as i32 + 1;
        let (kante, cells) = if (0..=EXPLOSION_CACHE_HALF_MAX).contains(&halb) {
            let kante = 2 * halb + 1;
            (kante, vec![0u8; (kante * kante * kante) as usize])
        } else {
            (0, Vec::new())
        };
        Self { center, radius, world, shapes, cells, ecke: center.floor().as_ivec3() - glam::IVec3::splat(halb), kante }
    }

    pub(crate) fn at(&mut self, pos: DVec3) -> Option<DVec3> {
        let weg = pos - self.center;
        let laenge = weg.length();
        let abstand = laenge / (f64::from(self.radius) * 2.0);
        if abstand.is_nan() || abstand > 1.0 || laenge < 1e-9 || self.ray_blocked(pos) {
            return None;
        }
        Some(weg / laenge * (1.0 - abstand) * EXPLOSION_PUSH_FACTOR)
    }

    fn solid(&mut self, z: glam::IVec3) -> bool {
        let frage = |s: &Self| !world::collision_boxes(s.world.block_at(z.x, z.y, z.z), s.shapes).is_empty();
        let r = z - self.ecke;
        if self.cells.is_empty() || r.min_element() < 0 || r.max_element() >= self.kante {
            return frage(self);
        }
        let i = ((r.z * self.kante + r.y) * self.kante + r.x) as usize;
        if self.cells[i] == 0 {
            self.cells[i] = if frage(self) { 2 } else { 1 };
        }
        self.cells[i] == 2
    }

    // Liegt zwischen Zentrum und `bis` ein Block mit Kollisionsform? Schritt fuer Schritt durch
    // die Bloecke auf der Strecke; der Block des Zentrums und der des Ziels zaehlen nicht.
    fn ray_blocked(&mut self, bis: DVec3) -> bool {
        let von = self.center;
        let d = bis - von;
        let mut zelle = von.floor().as_ivec3();
        let ziel = bis.floor().as_ivec3();
        // Nicht signum: das ist bei 0.0 schon 1.
        let richtung = |l: f64| i32::from(l > 0.0) - i32::from(l < 0.0);
        let schritt = glam::IVec3::new(richtung(d.x), richtung(d.y), richtung(d.z));
        // Strecke (in Anteilen von d) je Block und bis zur naechsten Blockgrenze je Achse; eine
        // Achse ohne Bewegung erreicht nie eine.
        let je_block = |l: f64| if l == 0.0 { f64::INFINITY } else { 1.0 / l.abs() };
        let je_block = DVec3::new(je_block(d.x), je_block(d.y), je_block(d.z));
        let grenze = |p: f64, z: i32, s: i32, j: f64| match s {
            0 => f64::INFINITY,
            1 => (f64::from(z + 1) - p) * j,
            _ => (p - f64::from(z)) * j,
        };
        let mut naechste = DVec3::new(
            grenze(von.x, zelle.x, schritt.x, je_block.x),
            grenze(von.y, zelle.y, schritt.y, je_block.y),
            grenze(von.z, zelle.z, schritt.z, je_block.z),
        );
        while zelle != ziel {
            if naechste.x <= naechste.y && naechste.x <= naechste.z {
                if naechste.x > 1.0 {
                    break;
                }
                zelle.x += schritt.x;
                naechste.x += je_block.x;
            } else if naechste.y <= naechste.z {
                if naechste.y > 1.0 {
                    break;
                }
                zelle.y += schritt.y;
                naechste.y += je_block.y;
            } else {
                if naechste.z > 1.0 {
                    break;
                }
                zelle.z += schritt.z;
                naechste.z += je_block.z;
            }
            if zelle == ziel {
                break;
            }
            if self.solid(zelle) {
                return true;
            }
        }
        false
    }
}

// --- java.util.Random ---------------------------------------------------------------------
//
// Dasselbe LCG wie Minecrafts CheckedRandom/LocalRandom. Nicht wegen gleicher Werte (die Saat
// ist ohnehin eine andere), sondern weil nextGaussian und nextInt(bound) ihre Verteilung genau
// so haben sollen wie in Vanilla -- und das ist mit dem Original am einfachsten sicher.
pub struct JavaRandom {
    seed: i64,
    next_gaussian: Option<f64>,
}

impl JavaRandom {
    const MULT: i64 = 0x5DEECE66D;
    const MASK: i64 = (1 << 48) - 1;

    pub const fn new(seed: i64) -> Self {
        Self { seed: (seed ^ Self::MULT) & Self::MASK, next_gaussian: None }
    }

    const fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(Self::MULT).wrapping_add(0xB)) & Self::MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self, bound: i32) -> i32 {
        debug_assert!(bound > 0);
        if bound & -bound == bound {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1u32 << 24) as f32
    }

    pub fn next_double(&mut self) -> f64 {
        ((i64::from(self.next(26)) << 27) + i64::from(self.next(27))) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    pub fn next_gaussian(&mut self) -> f64 {
        if let Some(g) = self.next_gaussian.take() {
            return g;
        }
        loop {
            let v1 = 2.0 * self.next_double() - 1.0;
            let v2 = 2.0 * self.next_double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let m = (-2.0 * s.ln() / s).sqrt();
                self.next_gaussian = Some(v2 * m);
                return v1 * m;
            }
        }
    }
}

// Vanillas ParticlesMode (Einstellung "Partikel").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleMode {
    All,
    Decreased,
    Minimal,
}

impl ParticleMode {
    pub const fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Decreased,
            2 => Self::Minimal,
            _ => Self::All,
        }
    }
}

// --- Partikel-Anpassung (kein Vanilla) ------------------------------------------------------
//
// Je Partikelart ein eigener Satz: an/aus, Lebensdauer, Deckkraft und Tempo der Bewegung. Eine
// "Art" fasst Registry-Typen zusammen, die man im Spiel als dasselbe sieht (die Explosion samt
// ihrem Emitter und den Truemmern, kleine und grosse Boee, ...). Index = Position in KINDS,
// so steht es auch in den Einstellungen.
pub struct ParticleKind {
    // Schluessel in der Einstellungsdatei -- nie umbenennen.
    pub key: &'static str,
    pub label: &'static str,
    types: &'static [i32],
    pub spawn: SpawnKind,
    // Kann an der eigenen Figur entstehen, weil man es selbst ausloest oder abbekommt (Treffer,
    // Totem, Traenke, Windkugel, Rakete, Sprint-/Abbau-Staub, Essen). Alle anderen kommen aus der
    // Umgebung (Bloecke, Explosionen, Mobs) und gelten auch an der eigenen Figur als andere --
    // die Anpassung der eigenen zeigt sie gar nicht erst (s. particle_kinds_for_own).
    pub can_be_own: bool,
}

// Wie eine Art entsteht -- daran haengt, welche Regler sie ausser Dauer/Deckkraft/Tempo hat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    // Ein Partikel je Ereignis (Schwungangriff): nichts weiter einzustellen.
    Single,
    // Viele auf ein Ereignis (Blockstaub, Trank-Spritzer): Menge.
    Burst,
    // Wie Burst, aber ueber mehrere Ticks ausgestossen (Totem 30, Krit 3, Explosion 8): Menge
    // und "alle auf einmal".
    BurstEmitter,
    // Laufend, solange der Zustand haelt (Wirbel um Wesen, Blasensaeule): Intervall.
    Lingering,
}

impl SpawnKind {
    pub const fn has_amount(self) -> bool {
        matches!(self, Self::Burst | Self::BurstEmitter)
    }

    pub fn has_all_at_once(self) -> bool {
        self == Self::BurstEmitter
    }

    pub fn has_interval(self) -> bool {
        self == Self::Lingering
    }
}

// Alphabetisch nach Namen -- so stehen sie auch in den Einstellungen.
pub const KIND_BLOCK: usize = 1;
pub const KIND_EXPLOSION: usize = 3;
pub const KINDS: [ParticleKind; 20] = [
    ParticleKind {
        key: "bubbles",
        label: "Blasensäule",
        types: &[pp::PARTICLE_BUBBLE_COLUMN_UP, pp::PARTICLE_CURRENT_DOWN],
        spawn: SpawnKind::Lingering,
        can_be_own: false,
    },
    ParticleKind { key: "block", label: "Blockstaub", types: &[pp::PARTICLE_BLOCK], spawn: SpawnKind::Burst, can_be_own: true },
    ParticleKind { key: "end_rod", label: "Endstab", types: &[pp::PARTICLE_END_ROD], spawn: SpawnKind::Lingering, can_be_own: false },
    ParticleKind {
        key: "explosion",
        label: "Explosion",
        types: &[pp::PARTICLE_EXPLOSION, pp::PARTICLE_EXPLOSION_EMITTER],
        spawn: SpawnKind::BurstEmitter,
        can_be_own: false,
    },
    ParticleKind { key: "flash", label: "Feuerwerks-Blitz", types: &[pp::PARTICLE_FLASH], spawn: SpawnKind::Single, can_be_own: true },
    ParticleKind {
        key: "flame",
        label: "Flammen",
        types: &[pp::PARTICLE_FLAME, pp::PARTICLE_SOUL_FIRE_FLAME, pp::PARTICLE_SMALL_FLAME, pp::PARTICLE_COPPER_FIRE_FLAME],
        spawn: SpawnKind::Lingering,
        can_be_own: false,
    },
    ParticleKind {
        key: "item_shards",
        label: "Item-Splitter",
        types: &[pp::PARTICLE_ITEM, pp::PARTICLE_ITEM_SLIME, pp::PARTICLE_ITEM_COBWEB, pp::PARTICLE_ITEM_SNOWBALL],
        spawn: SpawnKind::Burst,
        can_be_own: true,
    },
    ParticleKind { key: "crit", label: "Krit", types: &[pp::PARTICLE_CRIT], spawn: SpawnKind::BurstEmitter, can_be_own: true },
    ParticleKind { key: "smoke", label: "Rauch", types: &[pp::PARTICLE_SMOKE, pp::PARTICLE_LARGE_SMOKE], spawn: SpawnKind::Burst, can_be_own: false },
    ParticleKind { key: "poof", label: "Rauchwölkchen", types: &[pp::PARTICLE_POOF], spawn: SpawnKind::Burst, can_be_own: true },
    ParticleKind { key: "dust", label: "Redstone-Staub", types: &[pp::PARTICLE_DUST], spawn: SpawnKind::Lingering, can_be_own: false },
    ParticleKind { key: "damage_indicator", label: "Schadensherzen", types: &[pp::PARTICLE_DAMAGE_INDICATOR], spawn: SpawnKind::Burst, can_be_own: true },
    ParticleKind { key: "sweep", label: "Schwungangriff", types: &[pp::PARTICLE_SWEEP_ATTACK], spawn: SpawnKind::Single, can_be_own: true },
    ParticleKind { key: "sculk_soul", label: "Sculk-Seele", types: &[pp::PARTICLE_SCULK_SOUL], spawn: SpawnKind::Burst, can_be_own: false },
    ParticleKind { key: "totem", label: "Totem", types: &[pp::PARTICLE_TOTEM_OF_UNDYING], spawn: SpawnKind::BurstEmitter, can_be_own: true },
    // Die Wolke eines zerplatzenden Wurf- oder Verweiltranks (effect/instant_effect) -- ein
    // Schwall auf ein Ereignis, anders als die Wirbel darunter.
    ParticleKind {
        key: "potion_splash",
        label: "Trank-Spritzer",
        types: &[pp::PARTICLE_EFFECT, pp::PARTICLE_INSTANT_EFFECT],
        spawn: SpawnKind::Burst,
        can_be_own: true,
    },
    // Die Wirbel um Wesen mit Effekten und ueber Trankwolken am Boden: laufend, solange der
    // Effekt haelt. Der Schluessel stammt aus der Zeit, als die Spritzer noch dazugehoerten.
    ParticleKind { key: "potion", label: "Trankeffekte", types: &[pp::PARTICLE_ENTITY_EFFECT], spawn: SpawnKind::Lingering, can_be_own: true },
    ParticleKind { key: "enchanted_hit", label: "Verzauberungs-Treffer", types: &[pp::PARTICLE_ENCHANTED_HIT], spawn: SpawnKind::BurstEmitter, can_be_own: true },
    ParticleKind {
        key: "gust",
        label: "Windböe",
        types: &[pp::PARTICLE_GUST, pp::PARTICLE_SMALL_GUST, pp::PARTICLE_GUST_EMITTER_LARGE, pp::PARTICLE_GUST_EMITTER_SMALL],
        spawn: SpawnKind::BurstEmitter,
        can_be_own: true,
    },
    ParticleKind { key: "cloud", label: "Wolke", types: &[pp::PARTICLE_CLOUD, pp::PARTICLE_SNEEZE], spawn: SpawnKind::Burst, can_be_own: false },
];

// Die Arten einer Anpassung mit ihrem Index in KINDS: fuer die eigenen nur die, die an der
// eigenen Figur entstehen koennen.
pub fn particle_kinds_for_own(own: bool) -> Vec<(usize, &'static ParticleKind)> {
    KINDS.iter().enumerate().filter(|(_, k)| !own || k.can_be_own).collect()
}

fn kind_of(type_id: i32) -> Option<usize> {
    KINDS.iter().position(|k| k.types.contains(&type_id))
}

// Die Werte einer Art in Prozent: Lebensdauer, Tempo, Menge und Intervall relativ zu
// Vanilla, Deckkraft absolut. Alles 100 und "alle auf einmal" aus ist Vanilla. Menge zaehlt
// nur bei Schwall-Arten, Intervall nur bei verweilenden, "alle auf einmal" nur bei Emittern
// (s. SpawnKind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticleTuning {
    pub enabled: bool,
    pub lifetime_pct: u32,
    pub opacity_pct: u32,
    pub speed_pct: u32,
    pub amount_pct: u32,
    pub interval_pct: u32,
    pub size_pct: u32,
    pub spread_pct: u32,
    pub all_at_once: bool,
}

impl Default for ParticleTuning {
    fn default() -> Self {
        Self {
            enabled: true,
            lifetime_pct: 100,
            opacity_pct: 100,
            speed_pct: 100,
            amount_pct: 100,
            interval_pct: 100,
            size_pct: 100,
            spread_pct: 100,
            all_at_once: false,
        }
    }
}

impl ParticleTuning {
    pub fn size(&self) -> f32 {
        self.size_pct.max(1) as f32 / 100.0
    }

    pub fn spread(&self) -> f32 {
        self.spread_pct.max(1) as f32 / 100.0
    }

    fn speed(&self) -> f32 {
        self.speed_pct.max(1) as f32 / 100.0
    }

    fn opacity(&self) -> f32 {
        self.opacity_pct.min(100) as f32 / 100.0
    }

    // Wie viele Partikel je Vanilla-Partikel entstehen (Menge bzw. Kehrwert des Intervalls).
    fn spawn_factor(&self, spawn: SpawnKind) -> f32 {
        if spawn.has_amount() {
            self.amount_pct as f32 / 100.0
        } else if spawn.has_interval() {
            100.0 / self.interval_pct.max(1) as f32
        } else {
            1.0
        }
    }
}

// BillboardParticle.RenderType -- welcher Atlas und welche Pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sheet {
    Opaque,
    Translucent,
    Block,
}

// Wie tick() weiterlaeuft. Die meisten Arten erben Particle.tick; Explosion und Schwung
// bewegen sich gar nicht, der Explosions-Emitter zeichnet nichts und spawnt nur.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Behavior {
    Physics,
    Frozen,
    ExplosionEmitter,
    // GustEmitterParticle (Windkugel-Explosion): zeichnet nichts, spawnt Boeen. `large`
    // waehlt die Fabrik-Werte (gross: Streuung 3, 7 Ticks, jeden Tick; klein: 1, 3, jeden
    // dritten).
    GustEmitter { large: bool },
    // CurrentDownParticle (Strudel ueber Magma): sinkt gleichmaessig und kreist dabei, der
    // Winkel waechst je Tick um 0.08.
    CurrentDown { angle: f32 },
}

impl Behavior {
    const fn renders(self) -> bool {
        !matches!(self, Self::ExplosionEmitter | Self::GustEmitter { .. })
    }
}

// Der Teil eines Entities, den ein Emitter braucht (Entity.getBodyX/Y/Z).
#[derive(Clone, Copy, Debug)]
pub struct EmitterBody {
    pub pos: DVec3,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug)]
struct Particle {
    last: DVec3,
    pos: DVec3,
    vel: DVec3,
    spacing_xz: f64,
    spacing_y: f64,
    on_ground: bool,
    collides: bool,
    stopped: bool,
    // Kam `stopped` von einer Landung (Fall nach unten)? S. move_by.
    landed: bool,
    dead: bool,
    age: i32,
    max_age: i32,
    gravity: f32,
    velocity_multiplier: f32,
    ascending: bool,
    scale: f32,
    color: [f32; 4],
    uv: [f32; 4],
    // Typ, aus dessen Spriteliste der Partikel beim Altern das Bild zieht (updateSprite).
    animated_sprites: Option<i32>,
    // getSize waechst in den ersten 1/32 der Lebensdauer von 0 hoch (Damage/Ascending/Dust).
    grows_in: bool,
    // DamageParticle.tick: gruen *= 0.96, blau *= 0.9 je Tick -- Krits laufen ins Rot.
    fades_to_red: bool,
    // AnimatedParticle.tick: ab der halben Lebensdauer ausblenden.
    fades_out: bool,
    // SimpleAnimatedParticle.setFadeColor: beim Ausblenden laeuft die Farbe je Tick um ein
    // Fuenftel auf diese zu (Endstab: ins Gelbliche).
    fade_color: Option<[f32; 3]>,
    // FlameParticle.getQuadSize: schrumpft zum Ende hin auf die Haelfte (quadratisch).
    shrinks: bool,
    // PlayerCloudParticle.tick: sinkt zu einem Spieler unter sich in 2 Bloecken Umkreis.
    follows_player: bool,
    // FireworkParticles.OverlayParticle (flash): Groesse und Deckkraft haengen am Alter samt
    // angebrochenem Tick, s. size und build_vertices.
    flash: bool,
    // SpellParticle.tick: Deckkraft je Tick um 5 % zurueck zu der, mit der das Partikel
    // entstand -- nicht zu voll deckend. Ambient-Effekte (Leuchtfeuer, versteckte
    // Server-Effekte) kommen mit 38/255 und bleiben so fast durchsichtig; liefe die Deckkraft
    // gegen 1, wuerde etwa ein Blindheits-Wirbel als schwarze Flocken sichtbar.
    alpha_target: Option<f32>,
    // Blasen: verschwinden, sobald ihr Block kein Wasser mehr ist.
    needs_water: bool,
    behavior: Behavior,
    sheet: Sheet,
    // Index in KINDS -- welche Anpassung fuer diesen Partikel gilt -- und ob die eigene (an
    // der eigenen Figur entstanden, s. own_at) oder die der anderen.
    kind: u8,
    own: bool,
    // Registry-ID des Partikeltyps, -1 fuer Partikel ohne eigenen Typ (Blockstaub & Co.) --
    // nur fuer die Debug-Anzeige (s. Particles::type_counts).
    type_id: i32,
    // Angebrochener Tick bei Tempo != 100 % (s. Particles::tick): wie weit der Partikel seit
    // seinem letzten eigenen Tick schon ist, in seinen Ticks.
    phase: f32,
    // Erst gezeichnet, wenn er sich einmal bewegt hat (age >= 1): Totem-Partikel (Nutzer
    // 2026-11-11). Neue Partikel stehen bis zu ihrem ersten eigenen Tick am Startpunkt -- das
    // ist ein, bei Tempo unter 100 % zwei Spielticks --, und die ganze Wolke stand dort als
    // Klumpen am Spieler.
    hide_until_moved: bool,
}

impl Particle {
    // Particle(world, x, y, z): Box 0.2, Lebensdauer 4 / (0.1..1.0).
    fn base(rng: &mut JavaRandom, pos: DVec3) -> Self {
        let max_age = (4.0 / (rng.next_float() * 0.9 + 0.1)) as i32;
        Self {
            last: pos,
            pos,
            vel: DVec3::ZERO,
            spacing_xz: 0.2,
            spacing_y: 0.2,
            on_ground: false,
            collides: true,
            stopped: false,
            landed: false,
            dead: false,
            age: 0,
            max_age,
            gravity: 0.0,
            velocity_multiplier: 0.98,
            ascending: false,
            scale: 0.0,
            color: [1.0; 4],
            uv: [0.0; 4],
            animated_sprites: None,
            grows_in: false,
            fades_to_red: false,
            fades_out: false,
            fade_color: None,
            shrinks: false,
            follows_player: false,
            flash: false,
            alpha_target: None,
            needs_water: false,
            behavior: Behavior::Physics,
            sheet: Sheet::Opaque,
            kind: 0,
            own: false,
            type_id: -1,
            phase: 0.0,
            hide_until_moved: false,
        }
    }

    // Particle(world, x, y, z, vx, vy, vz): die mitgegebene Geschwindigkeit wird verrauscht und
    // auf einen zufaelligen Betrag um 0.06 normiert, dazu +0.1 nach oben.
    fn with_velocity(rng: &mut JavaRandom, pos: DVec3, v: DVec3) -> Self {
        let mut p = Self::base(rng, pos);
        let jitter = |c: f64, rng: &mut JavaRandom| c + f64::from((rng.next_float() * 2.0 - 1.0) * 0.4);
        let (vx, vy, vz) = (jitter(v.x, rng), jitter(v.y, rng), jitter(v.z, rng));
        let d = f64::from((rng.next_float() + rng.next_float() + 1.0) * 0.15);
        let e = (vx * vx + vy * vy + vz * vz).sqrt();
        p.vel = DVec3::new(vx / e * d * f64::from(0.4f32), vy / e * d * f64::from(0.4f32) + f64::from(0.1f32), vz / e * d * f64::from(0.4f32));
        p
    }

    // BillboardParticle-Konstruktor: Grundgroesse 0.1..0.2.
    fn billboard(mut self, rng: &mut JavaRandom, uv: [f32; 4]) -> Self {
        self.scale = 0.1 * (rng.next_float() * 0.5 + 0.5) * 2.0;
        self.uv = uv;
        self
    }

    // Particle.move(float speed) -- NICHT die Bewegung, sondern ein Tempo-Faktor.
    fn speed(mut self, s: f32) -> Self {
        let s = f64::from(s);
        self.vel.x *= s;
        self.vel.y = (self.vel.y - f64::from(0.1f32)) * s + f64::from(0.1f32);
        self.vel.z *= s;
        self
    }

    // BillboardParticle.scale -> Particle.scale: Bild UND Kollisionsbox.
    fn scaled(mut self, s: f32) -> Self {
        self.scale *= s;
        self.set_spacing(0.2 * f64::from(s), 0.2 * f64::from(s));
        self
    }

    const fn set_spacing(&mut self, xz: f64, y: f64) {
        // setBoundingBoxSpacing haelt die Box um denselben Mittelpunkt und dieselbe
        // Unterkante -- also schlicht die Position.
        self.spacing_xz = xz;
        self.spacing_y = y;
    }

    fn bounds(&self) -> (DVec3, DVec3) {
        let h = self.spacing_xz / 2.0;
        (
            DVec3::new(self.pos.x - h, self.pos.y, self.pos.z - h),
            DVec3::new(self.pos.x + h, self.pos.y + self.spacing_y, self.pos.z + h),
        )
    }

    fn size(&self, tick_progress: f32) -> f32 {
        if self.flash {
            // Waechst ueber die vier Ticks auf 7.1 und faellt wieder (halbe Sinuswelle).
            return 7.1 * ((self.age as f32 + tick_progress - 1.0) * 0.25 * std::f32::consts::PI).sin();
        }
        if self.shrinks {
            let s = (self.age as f32 + tick_progress) / self.max_age as f32;
            return self.scale * (1.0 - s * s * 0.5);
        }
        if self.grows_in {
            self.scale * ((self.age as f32 + tick_progress) / self.max_age as f32 * 32.0).clamp(0.0, 1.0)
        } else {
            self.scale
        }
    }

    // Particle.move(dx, dy, dz): gegen die Kollisionsformen (ShapeContext.absent, wie bei
    // Vanillas `null`-Entity), Stopp bei senkrechtem Anschlag.
    //
    // Abweichung von Vanilla: dort bleibt ein gelandeter Partikel fuer immer stehen, auch wenn
    // der Block darunter verschwindet. Hier faellt er weiter (gewuenscht), sobald unter ihm
    // wieder Platz ist. Nur nach einer Landung -- an der Decke Angeschlagenes bleibt stehen.
    fn move_by(&mut self, d: DVec3, world: &world::World, shapes: &world::BlockShapes) {
        if self.stopped {
            if !self.landed {
                return;
            }
            let (min, max) = self.bounds();
            let probe = DVec3::new(0.0, -0.01, 0.0);
            if crate::physics::collide_box(min, max, probe, world, shapes).y > -0.005 {
                return;
            }
            self.stopped = false;
            self.landed = false;
        }
        let wanted = d;
        let mut d = d;
        if self.collides && d != DVec3::ZERO && d.length_squared() < MAX_COLLISION_MOVE_SQ {
            let (min, max) = self.bounds();
            d = crate::physics::collide_box(min, max, d, world, shapes);
        }
        self.pos += d;
        if wanted.y.abs() >= f64::from(1.0e-5f32) && d.y.abs() < f64::from(1.0e-5f32) {
            self.stopped = true;
            self.landed = wanted.y < 0.0;
        }
        self.on_ground = wanted.y != d.y && wanted.y < 0.0;
        if wanted.x != d.x {
            self.vel.x = 0.0;
        }
        if wanted.z != d.z {
            self.vel.z = 0.0;
        }
    }

    // Rueckstoss einer Explosion (s. ExplosionPush). Nur Partikel mit gewoehnlicher Physik --
    // die uebrigen ziehen ihre eigene Bahn. Ein gelandeter liegt nur und fliegt wieder los.
    // true = gestossen; der Aufrufer laesst ihn dann gleich seinen ersten Schritt machen (s.
    // Particles::push_and_step).
    fn explosion_knockback(&mut self, push: &mut ExplosionPush) -> bool {
        if self.behavior != Behavior::Physics {
            return false;
        }
        let Some(stoss) = push.at(self.pos) else { return false };
        self.vel += stoss;
        if self.landed {
            self.stopped = false;
            self.landed = false;
        }
        true
    }

    // Particle.tick.
    fn tick_physics(&mut self, world: &world::World, shapes: &world::BlockShapes) {
        self.last = self.pos;
        let age = self.age;
        self.age += 1;
        if age >= self.max_age {
            self.dead = true;
            return;
        }
        self.vel.y -= 0.04 * f64::from(self.gravity);
        let before_y = self.pos.y;
        self.move_by(self.vel, world, shapes);
        if self.ascending && self.pos.y == before_y {
            self.vel.x *= 1.1;
            self.vel.z *= 1.1;
        }
        let m = f64::from(self.velocity_multiplier);
        self.vel *= m;
        if self.on_ground {
            self.vel.x *= f64::from(0.7f32);
            self.vel.z *= f64::from(0.7f32);
        }
    }
}

// Ein Geraeusch aus animate_blocks, an der Blockecke wie in Vanilla.
pub struct AmbientSound {
    pub name: &'static str,
    pub pos: DVec3,
    pub volume: f32,
    pub pitch: f32,
}

// FluidState.isIn(FluidTags.WATER) am Block unter `pos` -- auch Blasensaeulen, Seegras und
// waterlogged Bloecke.
fn in_water(world: &world::World, pos: DVec3) -> bool {
    let state = world.block_at(pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
    world::fluid_kind(state) == Some(world::FluidKind::Water)
}

// Wie viele Bilder einer Reihe vor den durchsichtigen am Ende liegen. Manche Packs (etwa
// "cpvp aesthetic" fuers Totem) leeren die letzten Bilder, damit die Animation kuerzer wirkt;
// dort endet der Partikel fuers Auge schon beim ersten leeren Bild (s. Particles::visible_end).
// Durchsichtig heisst: kein Pixel kommt ueber die Schwelle des Shaders (Alpha 0,1).
fn visible_frames(list: &[[f32; 4]], rgba: &[u8], size: u32) -> usize {
    let leer = |&[u0, v0, u1, v1]: &[f32; 4]| {
        let px = |t: f32| ((t * size as f32).round() as usize).min(size as usize);
        let (x0, x1, y0, y1) = (px(u0.min(u1)), px(u0.max(u1)), px(v0.min(v1)), px(v0.max(v1)));
        (y0..y1).all(|y| (x0..x1).all(|x| rgba.get((y * size as usize + x) * 4 + 3).is_none_or(|&a| a < 26)))
    };
    list.len() - list.iter().rev().take_while(|uv| leer(uv)).count()
}

// Das Rechteck der bemalten Pixel (Alpha ueber der Schwelle des Shaders) eines Bildes als
// Anteil (links, oben, rechts, unten) in Richtung u0->u1 und v0->v1. None ohne bemalte Pixel.
fn painted_bounds([u0, v0, u1, v1]: [f32; 4], rgba: &[u8], size: u32) -> Option<[f32; 4]> {
    let px = |t: f32| ((t * size as f32).round() as usize).min(size as usize);
    let (x0, x1, y0, y1) = (px(u0.min(u1)), px(u0.max(u1)), px(v0.min(v1)), px(v0.max(v1)));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut bereich: Option<[usize; 4]> = None;
    for y in y0..y1 {
        for x in x0..x1 {
            if rgba.get((y * size as usize + x) * 4 + 3).is_some_and(|&a| a >= 26) {
                let b = bereich.get_or_insert([x, y, x, y]);
                *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
        }
    }
    let [bx0, by0, bx1, by1] = bereich?;
    let (w, h) = ((x1 - x0) as f32, (y1 - y0) as f32);
    // Pixelkanten als Anteil; bei gespiegeltem UV von der anderen Seite gezaehlt.
    let (mut l, mut r) = ((bx0 - x0) as f32 / w, (bx1 + 1 - x0) as f32 / w);
    let (mut o, mut un) = ((by0 - y0) as f32 / h, (by1 + 1 - y0) as f32 / h);
    if u1 < u0 {
        (l, r) = (1.0 - r, 1.0 - l);
    }
    if v1 < v0 {
        (o, un) = (1.0 - un, 1.0 - o);
    }
    Some([l, o, r, un])
}

// SpriteProvider.getSprite(age, maxAge).
fn sprite_by_age(list: &[[f32; 4]], age: i32, max_age: i32) -> [f32; 4] {
    if list.is_empty() {
        return [0.0; 4];
    }
    let i = (age * (list.len() as i32 - 1) / max_age.max(1)).clamp(0, list.len() as i32 - 1);
    list[i as usize]
}

struct Emitter {
    entity: i32,
    effect: ParticleEffect,
    age: i32,
    max_age: i32,
    // Seit dem letzten Partikel-Tick entstanden? Ein Totem-Pop durch eine Explosion kommt im
    // selben Server-Tick an wie sie, aber VOR ihr (ServerLevel.explode verletzt erst und
    // schickt dann das Explosionspaket).
    fresh: bool,
    // Die Explosion (Zentrum, Radius), die diesen Emitter ausgeloest hat: jeder seiner
    // Partikel bekommt ihren Stoss mit, auch die, die erst Ticks spaeter entstehen (Nutzer
    // 2026-11-11 -- sonst sammelte sich die Totem-Wolke 1,5 s lang ungestossen am Spieler).
    explosion: Option<(DVec3, f32)>,
    // Gehoert der Emitter zur eigenen Figur? Dann sind seine Partikel eigene, sonst nicht --
    // unabhaengig davon, wo sie entstehen (ein Krit am Gegner direkt neben einem war frueher
    // ein eigener).
    own: bool,
}

// BlockParticleEffectsManager.Entry.
struct ExplosionBurst {
    // An der eigenen Figur (s. own_at): welche Anpassung und Stufe fuer die Truemmer gilt.
    own: bool,
    center: DVec3,
    radius: f32,
    block_count: i32,
    particles: Vec<pp::BlockParticle>,
}

// Was der Partikelbau von der Welt wissen muss. Ein Buendel statt fuenf Parameter, weil
// dieselben Werte durch jede Spawn-Kette gereicht werden (Emitter -> addParticleClient ->
// Fabrik).
pub struct SpawnContext<'a> {
    pub camera: DVec3,
    // Partikel-Stufe fuer die anderen und fuer die eigenen Partikel (s. own_at).
    pub mode: ParticleMode,
    pub mode_own: ParticleMode,
    pub world: &'a world::World,
    pub shapes: &'a world::BlockShapes,
    // Biomfarben fuer den Staub gefaerbter Bloecke (Gras, Laub), s. block_dust.
    pub biomes: &'a world::BiomePalette,
    // Die eigene Figur, fuer Wolken, die einem Spieler folgen (PlayerCloudParticle). Andere
    // Spieler zaehlen hier nicht mit.
    pub player: Option<NearPlayer>,
}

#[derive(Clone, Copy, Debug)]
pub struct NearPlayer {
    pub pos: DVec3,
    pub vel_y: f64,
}

// So weit um den Kasten der eigenen Figur (0.6 breit, 1.8 hoch, `pos` an den Fuessen) zaehlt
// ein Partikel als eigenes: Treffer, Effekte und das Totem erscheinen rund um den Koerper.
const OWN_MARGIN: f64 = 0.5;

// "Eigene" Partikel: die an der eigenen Figur entstehen. Der Server sagt nicht, wem ein
// Partikel gilt -- es zaehlt der Ort, an dem es entsteht.
fn own_at(ctx: &SpawnContext, kind: usize, pos: DVec3) -> bool {
    KINDS[kind].can_be_own
        && ctx.player.is_some_and(|me| {
        let d = pos - me.pos;
        d.x.abs() <= 0.3 + OWN_MARGIN && d.z.abs() <= 0.3 + OWN_MARGIN && d.y >= -OWN_MARGIN && d.y <= 1.8 + OWN_MARGIN
    })
}

impl SpawnContext<'_> {
    const fn mode_for(&self, own: bool) -> ParticleMode {
        if own { self.mode_own } else { self.mode }
    }
}

pub struct Particles {
    rng: JavaRandom,
    // Sprites je Partikeltyp (Index = Registry-ID), in JSON-Reihenfolge (s.
    // assets::ParticleAtlas).
    sprites: Vec<Vec<[f32; 4]>>,
    // Je Partikeltyp: wie viele Bilder vor den durchsichtigen am Ende der Reihe liegen (s.
    // visible_frames). Leer oder >= Anzahl: keine leeren am Ende.
    visible_frames: Vec<usize>,
    // Je Bild im Partikel-Atlas (Schluessel: die UV-Bits): das Rechteck der bemalten Pixel als
    // Anteil des Bildes (links, oben, rechts, unten; 0..1 in Richtung u0->u1 bzw. v0->v1). Daraus
    // Kollisionsbox und Bildlage (s. fit_painted_box, build_vertices) -- als Anteil, damit es
    // mit hochaufgeloesten Packs genauso stimmt.
    painted: HashMap<[u32; 4], [f32; 4]>,
    // Partikeltextur je Blockname im BLOCK-Atlas (assets::BlockAtlas::particle_uvs).
    block_uvs: HashMap<String, [f32; 4]>,
    // Splitterbild je Itemname im Partikel-Atlas (s. assets::ParticleAtlas::item_sprites).
    item_uvs: HashMap<String, [f32; 4]>,
    particles: VecDeque<Particle>,
    // Neu erzeugte Partikel landen erst am Ende des Ticks im Bestand (ParticleManager.newParticles).
    pending: Vec<Particle>,
    emitters: Vec<Emitter>,
    bursts: Vec<ExplosionBurst>,
    // Typ-IDs, die ankamen, aber nicht gezeichnet werden -- mit Anzahl, fuer die Debug-Anzeige.
    pub unsupported: HashMap<i32, u32>,
    // Davon die erstmals gesehenen, bis die App sie ins Log schreibt (s. take_new_unsupported).
    new_unsupported: Vec<i32>,
    // Partikel-Anpassung je Art (Index = KINDS). "Explosion aus" laesst auch die Truemmer weg:
    // ein Resourcepack kann das nicht sauber leeren -- das CPvP-Pack etwa laesst in jeder
    // Explosions- und Rauchtextur einen Pixel stehen, der beim Crystallen als weisse Punkte
    // bleibt.
    // Index 0 die anderen, 1 die eigenen Partikel (s. own_at).
    tuning: [[ParticleTuning; KINDS.len()]; 2],
    // Guthaben je Art fuer Menge und Intervall: jedes Vanilla-Partikel bringt den Faktor ein,
    // es entstehen so viele, wie ganze Einheiten zusammenkommen. So ergeben 50 % genau jedes
    // zweite, auch wenn je Ereignis nur eins kommt. Wie `tuning` je andere/eigene.
    credit: [[f32; KINDS.len()]; 2],
    // Waehrend ein Emitter ausstoesst (s. tick_emitter): ob seine Partikel eigene sind, und ob
    // er die Menge schon selbst vervielfacht hat (dann keine Kopien in push).
    emit_own: Option<bool>,
    emit_scaled: bool,
    // Partikel ausblenden (Video): so viele ms vor ihrem Ende werden sie durchsichtig, 0 = aus
    // (s. fade_out).
    pub fade_out_ms: u32,
    // Bis zu dieser Entfernung (Bloecke) um die Kamera entstehen Partikel ohne `force`
    // (Settings::particle_distance, Vanilla 32).
    pub spawn_distance: f64,
}

impl Particles {
    pub fn new(atlas: &assets::ParticleAtlas, block_uvs: HashMap<String, [f32; 4]>) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
        let mut me = Self {
            rng: JavaRandom::new(seed),
            sprites: Vec::new(),
            visible_frames: Vec::new(),
            painted: HashMap::default(),
            block_uvs: HashMap::default(),
            item_uvs: HashMap::default(),
            particles: VecDeque::new(),
            pending: Vec::new(),
            emitters: Vec::new(),
            bursts: Vec::new(),
            unsupported: HashMap::default(),
            new_unsupported: Vec::new(),
            tuning: [[ParticleTuning::default(); KINDS.len()]; 2],
            credit: [[0.0; KINDS.len()]; 2],
            emit_own: None,
            emit_scaled: false,
            fade_out_ms: 0,
            spawn_distance: VANILLA_SPAWN_DISTANCE,
        };
        me.set_textures(atlas, block_uvs);
        me
    }

    // Nach einem Resourcepack-Wechsel: neue Atlas-Koordinaten. Laufende Partikel behalten ihr
    // altes UV bis zum naechsten Bildwechsel -- das sind hoechstens ein paar Ticks.
    pub fn set_textures(&mut self, atlas: &assets::ParticleAtlas, block_uvs: HashMap<String, [f32; 4]>) {
        self.sprites = pp::PARTICLE_NAMES.iter().map(|n| atlas.sprites(n).to_vec()).collect();
        self.visible_frames = self.sprites.iter().map(|list| visible_frames(list, &atlas.rgba, atlas.size)).collect();
        self.painted = self.sprites.iter().flatten().filter_map(|&uv| painted_bounds(uv, &atlas.rgba, atlas.size).map(|b| (uv.map(f32::to_bits), b))).collect();
        self.block_uvs = block_uvs;
        self.item_uvs = atlas.item_sprites().map(|(item, uv)| (item.to_string(), uv)).collect();
    }

    // Neue Anpassung aus den Einstellungen. Was jetzt aus ist, verschwindet sofort; Dauer,
    // Deckkraft und Tempo gelten auch fuer die schon fliegenden Partikel (die Dauer nur fuer
    // neue).
    // `other` gilt fuer die anderen Partikel, `own` fuer die an der eigenen Figur.
    pub fn set_tuning(&mut self, other: [ParticleTuning; KINDS.len()], own: [ParticleTuning; KINDS.len()]) {
        let tuning = [other, own];
        if tuning == self.tuning {
            return;
        }
        self.tuning = tuning;
        let an = |p: &Particle| tuning[usize::from(p.own)][p.kind as usize].enabled;
        self.particles.retain(an);
        self.pending.retain(an);
        self.bursts.retain(|b| tuning[usize::from(b.own)][KIND_EXPLOSION].enabled);
    }

    fn tuning_of(&self, own: bool, kind: usize) -> ParticleTuning {
        self.tuning[usize::from(own)][kind]
    }

    // ParticleManager.clearParticles -- beim Welt- oder Dimensionswechsel.
    pub fn clear(&mut self) {
        self.particles.clear();
        self.pending.clear();
        self.emitters.clear();
        self.bursts.clear();
    }

    // Der Zufall der Partikel -- zugleich Vanillas ClientWorld.random, aus dem auch der
    // Explosionston seine Tonhoehe zieht.
    pub const fn rng(&mut self) -> &mut JavaRandom {
        &mut self.rng
    }

    // Wie viele Partikel welcher Art gerade leben, haeufigste zuerst: der Partikeltyp, wo es
    // einen gibt, sonst die Art aus der Partikel-Anpassung. Fuer die Debug-Anzeige (F3).
    pub fn type_counts(&self) -> Vec<(String, usize)> {
        let mut zaehler: HashMap<String, usize> = HashMap::default();
        for p in self.particles.iter().filter(|p| p.behavior.renders()) {
            let name = pp::particle_name(p.type_id).map_or_else(|| KINDS[p.kind as usize].key.to_string(), str::to_string);
            *zaehler.entry(name).or_default() += 1;
        }
        let mut liste: Vec<(String, usize)> = zaehler.into_iter().collect();
        liste.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        liste
    }

    // Partikeltypen, die seit dem letzten Aufruf zum ersten Mal ankamen und nicht umgesetzt sind.
    pub fn take_new_unsupported(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.new_unsupported)
    }

    fn sprite_list(&self, type_id: i32) -> &[[f32; 4]] {
        usize::try_from(type_id).ok().and_then(|i| self.sprites.get(i)).map(Vec::as_slice).unwrap_or(&[])
    }

    fn random_sprite(&mut self, type_id: i32) -> Option<[f32; 4]> {
        let n = self.sprite_list(type_id).len();
        if n == 0 {
            return None;
        }
        let i = self.rng.next_int(n as i32) as usize;
        Some(self.sprite_list(type_id)[i])
    }

    fn first_sprite(&self, type_id: i32) -> Option<[f32; 4]> {
        self.sprite_list(type_id).first().copied()
    }

    // ClientWorld.getParticlesMode: "wichtige" Partikel kommen auch bei Minimal in jedem
    // zehnten Fall durch, bei Verringert faellt jeder dritte weg.
    fn effective_mode(&mut self, mode: ParticleMode, important: bool) -> ParticleMode {
        let mut m = mode;
        if important && m == ParticleMode::Minimal && self.rng.next_int(10) == 0 {
            m = ParticleMode::Decreased;
        }
        if m == ParticleMode::Decreased && self.rng.next_int(3) == 0 {
            m = ParticleMode::Minimal;
        }
        m
    }

    // ClientWorld.addParticle(effect, force, canSpawnOnMinimal, ...).
    fn add_client(&mut self, effect: &ParticleEffect, force: bool, important: bool, pos: DVec3, vel: DVec3, ctx: &SpawnContext) {
        // Typen ohne Art sind nicht umgesetzt -- create zaehlt sie nur.
        let kind = kind_of(effect.type_id).unwrap_or(0);
        self.add_client_as(kind, effect, force, important, pos, vel, ctx);
    }

    // Wie add_client, aber fuer die Anpassung als Partikel der Art `kind` (die Truemmer einer
    // Explosion sind Rauch und Woelkchen, gehoeren aber zur Explosion).
    #[allow(clippy::too_many_arguments)]
    fn add_client_as(&mut self, kind: usize, effect: &ParticleEffect, force: bool, important: bool, pos: DVec3, vel: DVec3, ctx: &SpawnContext) {
        let own = self.emit_own.unwrap_or_else(|| own_at(ctx, kind, pos));
        if !self.tuning_of(own, kind).enabled {
            return;
        }
        let force = force || pp::particle_always_spawns(effect.type_id);
        let mode = self.effective_mode(ctx.mode_for(own), important);
        if !force && (ctx.camera.distance_squared(pos) > self.spawn_distance * self.spawn_distance || mode == ParticleMode::Minimal) {
            return;
        }
        if let Some(mut p) = self.create(effect, pos, vel, ctx) {
            p.type_id = effect.type_id;
            // Ein Emitter-Partikel (Explosion, Windkugel) mit "alle auf einmal": seinen ganzen
            // Ausstoss jetzt, statt ueber seine Ticks verteilt.
            if !p.behavior.renders() && self.tuning_of(own, kind).all_at_once && KINDS[kind].spawn.has_all_at_once() {
                self.emit_all_now(p, ctx);
                return;
            }
            self.push(p, kind, ctx);
        }
    }

    // Alle Ticks eines Emitter-Partikels sofort durchlaufen und ausstossen.
    fn emit_all_now(&mut self, mut p: Particle, ctx: &SpawnContext) {
        let mut spawns = Vec::new();
        let mut boeen = Vec::new();
        for _ in 0..=p.max_age.max(0) {
            if p.dead {
                break;
            }
            Self::tick_one(&mut p, &self.sprites, ctx, &mut spawns, &mut boeen);
        }
        self.spawn_emitted(spawns, boeen, ctx);
    }

    // Wie viele Partikel fuer ein Vanilla-Partikel dieser Art entstehen (s. credit).
    fn spawn_count(&mut self, own: bool, kind: usize) -> u32 {
        let faktor = self.tuning_of(own, kind).spawn_factor(KINDS[kind].spawn);
        if faktor == 1.0 {
            return 1;
        }
        let guthaben = &mut self.credit[usize::from(own)][kind];
        *guthaben += faktor;
        let n = guthaben.floor();
        *guthaben -= n;
        n as u32
    }

    // In den Bestand (am Tickende), mit der Lebensdauer der Art. Emitter zeichnen nichts --
    // ihre Dauer bliebe sonst nicht die Dauer eines Partikels, sondern die Menge, die sie
    // ausstossen.
    fn push(&mut self, mut p: Particle, kind: usize, ctx: &SpawnContext) {
        p.own = self.emit_own.unwrap_or_else(|| own_at(ctx, kind, p.pos));
        let t = self.tuning_of(p.own, kind);
        if !t.enabled {
            return;
        }
        // Spread: der Anfangsschwung -- bei mehr fliegen die Partikel weiter auseinander.
        if t.spread_pct != 100 && p.behavior.renders() {
            p.vel *= f64::from(t.spread());
        }
        if t.lifetime_pct != 100 && p.behavior.renders() {
            p.max_age = ((p.max_age as f32 * t.lifetime_pct as f32 / 100.0).round() as i32).max(1);
        }
        p.kind = kind as u8;
        p.hide_until_moved = kind_of(pp::PARTICLE_TOTEM_OF_UNDYING) == Some(kind);
        // Menge/Intervall nur fuer sichtbare Partikel -- ein Emitter, der wegfiele, nahme
        // seinen ganzen Ausstoss mit. Hat ein Emitter die Menge schon selbst vervielfacht
        // (jedes mit eigener Richtung), keine Kopien mehr.
        let n = if !p.behavior.renders() || self.emit_scaled { 1 } else { self.spawn_count(p.own, kind) };
        for _ in 1..n {
            // Mehr als Vanilla: leicht gestreute Kopien, sonst laegen sie genau aufeinander.
            let mut kopie = p.clone();
            let r = &mut self.rng;
            kopie.vel += DVec3::new(r.next_gaussian() * 0.02, r.next_gaussian() * 0.02, r.next_gaussian() * 0.02);
            self.pending.push(kopie);
        }
        if n > 0 {
            self.pending.push(p);
        }
    }
}

// BlockState.hasBlockBreakParticles -- in Blocks.java (1.21.11) ruft genau zwei Bloecke
// `noBlockBreakParticles()` auf: Barriere und Strukturleere (per grep ueber den 
// Quelltext gezaehlt). Luft und der bewegte Kolben scheitern schon in BlockDustParticle.create.
// Ein Item-Partikel, das nur ueber den Namen geht (Kruemel, Splitter, Flaschenscherben).
fn item_effect(item: &str) -> ParticleEffect {
    ParticleEffect { type_id: pp::PARTICLE_ITEM, options: ParticleOptions::Item { item_id: u32::MAX, item_model: Some(item.to_string()) } }
}

// Vec3.xRot / yRot: Drehung um die x- bzw. y-Achse.
fn rot_x(v: DVec3, a: f32) -> DVec3 {
    let (c, s) = (f64::from(a.cos()), f64::from(a.sin()));
    DVec3::new(v.x, v.y * c + v.z * s, v.z * c - v.y * s)
}

fn rot_y(v: DVec3, a: f32) -> DVec3 {
    let (c, s) = (f64::from(a.cos()), f64::from(a.sin()));
    DVec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
}

fn has_block_break_particles(name: &str) -> bool {
    !matches!(name, "air" | "cave_air" | "void_air" | "barrier" | "moving_piston" | "structure_void")
}

#[cfg(test)]
mod tests;
