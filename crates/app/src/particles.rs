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

// EvictingQueue-Groesse je Blatt in Vanillas ParticleRenderer: bei vollem Blatt verdraengt ein
// neuer Partikel den aeltesten.
const MAX_PER_SHEET: usize = 16_384;
// ClientWorld.addParticle: ohne `force` entstehen Partikel nur bis 32 Bloecke um die Kamera.
const MAX_SPAWN_DISTANCE_SQ: f64 = 1024.0;
// Particle.MAX_SQUARED_COLLISION_CHECK_DISTANCE -- schnellere Partikel pruefen keine Kollision.
const MAX_COLLISION_MOVE_SQ: f64 = 100.0 * 100.0;
// BlockParticleEffectsManager.field_62022: hoechstens so viele Explosions-Blockpartikel je Tick.
const MAX_EXPLOSION_BLOCK_PARTICLES: i32 = 512;

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

    pub fn new(seed: i64) -> Self {
        Self { seed: (seed ^ Self::MULT) & Self::MASK, next_gaussian: None }
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(Self::MULT).wrapping_add(0xB)) & Self::MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self, bound: i32) -> i32 {
        debug_assert!(bound > 0);
        if bound & -bound == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
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
        (((self.next(26) as i64) << 27) + self.next(27) as i64) as f64 * (1.0 / (1u64 << 53) as f64)
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
    pub fn from_index(i: u8) -> Self {
        match i {
            1 => ParticleMode::Decreased,
            2 => ParticleMode::Minimal,
            _ => ParticleMode::All,
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
    pub fn has_amount(self) -> bool {
        matches!(self, SpawnKind::Burst | SpawnKind::BurstEmitter)
    }

    pub fn has_all_at_once(self) -> bool {
        self == SpawnKind::BurstEmitter
    }

    pub fn has_interval(self) -> bool {
        self == SpawnKind::Lingering
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
    },
    ParticleKind { key: "block", label: "Blockstaub", types: &[pp::PARTICLE_BLOCK], spawn: SpawnKind::Burst },
    ParticleKind { key: "end_rod", label: "Endstab", types: &[pp::PARTICLE_END_ROD], spawn: SpawnKind::Lingering },
    ParticleKind {
        key: "explosion",
        label: "Explosion",
        types: &[pp::PARTICLE_EXPLOSION, pp::PARTICLE_EXPLOSION_EMITTER],
        spawn: SpawnKind::BurstEmitter,
    },
    ParticleKind { key: "flash", label: "Feuerwerks-Blitz", types: &[pp::PARTICLE_FLASH], spawn: SpawnKind::Single },
    ParticleKind {
        key: "flame",
        label: "Flammen",
        types: &[pp::PARTICLE_FLAME, pp::PARTICLE_SOUL_FIRE_FLAME, pp::PARTICLE_SMALL_FLAME, pp::PARTICLE_COPPER_FIRE_FLAME],
        spawn: SpawnKind::Lingering,
    },
    ParticleKind {
        key: "item_shards",
        label: "Item-Splitter",
        types: &[pp::PARTICLE_ITEM, pp::PARTICLE_ITEM_SLIME, pp::PARTICLE_ITEM_COBWEB, pp::PARTICLE_ITEM_SNOWBALL],
        spawn: SpawnKind::Burst,
    },
    ParticleKind { key: "crit", label: "Krit", types: &[pp::PARTICLE_CRIT], spawn: SpawnKind::BurstEmitter },
    ParticleKind { key: "smoke", label: "Rauch", types: &[pp::PARTICLE_SMOKE, pp::PARTICLE_LARGE_SMOKE], spawn: SpawnKind::Burst },
    ParticleKind { key: "poof", label: "Rauchwölkchen", types: &[pp::PARTICLE_POOF], spawn: SpawnKind::Burst },
    ParticleKind { key: "dust", label: "Redstone-Staub", types: &[pp::PARTICLE_DUST], spawn: SpawnKind::Lingering },
    ParticleKind { key: "damage_indicator", label: "Schadensherzen", types: &[pp::PARTICLE_DAMAGE_INDICATOR], spawn: SpawnKind::Burst },
    ParticleKind { key: "sweep", label: "Schwungangriff", types: &[pp::PARTICLE_SWEEP_ATTACK], spawn: SpawnKind::Single },
    ParticleKind { key: "sculk_soul", label: "Sculk-Seele", types: &[pp::PARTICLE_SCULK_SOUL], spawn: SpawnKind::Burst },
    ParticleKind { key: "totem", label: "Totem", types: &[pp::PARTICLE_TOTEM_OF_UNDYING], spawn: SpawnKind::BurstEmitter },
    // Die Wolke eines zerplatzenden Wurf- oder Verweiltranks (effect/instant_effect) -- ein
    // Schwall auf ein Ereignis, anders als die Wirbel darunter.
    ParticleKind {
        key: "potion_splash",
        label: "Trank-Spritzer",
        types: &[pp::PARTICLE_EFFECT, pp::PARTICLE_INSTANT_EFFECT],
        spawn: SpawnKind::Burst,
    },
    // Die Wirbel um Wesen mit Effekten und ueber Trankwolken am Boden: laufend, solange der
    // Effekt haelt. Der Schluessel stammt aus der Zeit, als die Spritzer noch dazugehoerten.
    ParticleKind { key: "potion", label: "Trankeffekte", types: &[pp::PARTICLE_ENTITY_EFFECT], spawn: SpawnKind::Lingering },
    ParticleKind { key: "enchanted_hit", label: "Verzauberungs-Treffer", types: &[pp::PARTICLE_ENCHANTED_HIT], spawn: SpawnKind::BurstEmitter },
    ParticleKind {
        key: "gust",
        label: "Windböe",
        types: &[pp::PARTICLE_GUST, pp::PARTICLE_SMALL_GUST, pp::PARTICLE_GUST_EMITTER_LARGE, pp::PARTICLE_GUST_EMITTER_SMALL],
        spawn: SpawnKind::BurstEmitter,
    },
    ParticleKind { key: "cloud", label: "Wolke", types: &[pp::PARTICLE_CLOUD, pp::PARTICLE_SNEEZE], spawn: SpawnKind::Burst },
];

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
    pub all_at_once: bool,
}

impl Default for ParticleTuning {
    fn default() -> Self {
        Self { enabled: true, lifetime_pct: 100, opacity_pct: 100, speed_pct: 100, amount_pct: 100, interval_pct: 100, all_at_once: false }
    }
}

impl ParticleTuning {
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
    fn renders(self) -> bool {
        !matches!(self, Behavior::ExplosionEmitter | Behavior::GustEmitter { .. })
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
    // Index in KINDS -- welche Anpassung fuer diesen Partikel gilt.
    kind: u8,
    // Registry-ID des Partikeltyps, -1 fuer Partikel ohne eigenen Typ (Blockstaub & Co.) --
    // nur fuer die Debug-Anzeige (s. Particles::type_counts).
    type_id: i32,
    // Angebrochener Tick bei Tempo != 100 % (s. Particles::tick): wie weit der Partikel seit
    // seinem letzten eigenen Tick schon ist, in seinen Ticks.
    phase: f32,
}

impl Particle {
    // Particle(world, x, y, z): Box 0.2, Lebensdauer 4 / (0.1..1.0).
    fn base(rng: &mut JavaRandom, pos: DVec3) -> Self {
        let max_age = (4.0 / (rng.next_float() * 0.9 + 0.1)) as i32;
        Particle {
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
            type_id: -1,
            phase: 0.0,
        }
    }

    // Particle(world, x, y, z, vx, vy, vz): die mitgegebene Geschwindigkeit wird verrauscht und
    // auf einen zufaelligen Betrag um 0.06 normiert, dazu +0.1 nach oben.
    fn with_velocity(rng: &mut JavaRandom, pos: DVec3, v: DVec3) -> Self {
        let mut p = Self::base(rng, pos);
        let jitter = |c: f64, rng: &mut JavaRandom| c + ((rng.next_float() * 2.0 - 1.0) * 0.4) as f64;
        let (vx, vy, vz) = (jitter(v.x, rng), jitter(v.y, rng), jitter(v.z, rng));
        let d = ((rng.next_float() + rng.next_float() + 1.0) * 0.15) as f64;
        let e = (vx * vx + vy * vy + vz * vz).sqrt();
        p.vel = DVec3::new(vx / e * d * 0.4f32 as f64, vy / e * d * 0.4f32 as f64 + 0.1f32 as f64, vz / e * d * 0.4f32 as f64);
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
        let s = s as f64;
        self.vel.x *= s;
        self.vel.y = (self.vel.y - 0.1f32 as f64) * s + 0.1f32 as f64;
        self.vel.z *= s;
        self
    }

    // BillboardParticle.scale -> Particle.scale: Bild UND Kollisionsbox.
    fn scaled(mut self, s: f32) -> Self {
        self.scale *= s;
        self.set_spacing(0.2 * s as f64, 0.2 * s as f64);
        self
    }

    fn set_spacing(&mut self, xz: f64, y: f64) {
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
        if wanted.y.abs() >= 1.0e-5f32 as f64 && d.y.abs() < 1.0e-5f32 as f64 {
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

    // Particle.tick.
    fn tick_physics(&mut self, world: &world::World, shapes: &world::BlockShapes) {
        self.last = self.pos;
        let age = self.age;
        self.age += 1;
        if age >= self.max_age {
            self.dead = true;
            return;
        }
        self.vel.y -= 0.04 * self.gravity as f64;
        let before_y = self.pos.y;
        self.move_by(self.vel, world, shapes);
        if self.ascending && self.pos.y == before_y {
            self.vel.x *= 1.1;
            self.vel.z *= 1.1;
        }
        let m = self.velocity_multiplier as f64;
        self.vel *= m;
        if self.on_ground {
            self.vel.x *= 0.7f32 as f64;
            self.vel.z *= 0.7f32 as f64;
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
}

// BlockParticleEffectsManager.Entry.
struct ExplosionBurst {
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
    pub mode: ParticleMode,
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

pub struct Particles {
    rng: JavaRandom,
    // Sprites je Partikeltyp (Index = Registry-ID), in JSON-Reihenfolge (s.
    // assets::ParticleAtlas).
    sprites: Vec<Vec<[f32; 4]>>,
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
    tuning: [ParticleTuning; KINDS.len()],
    // Guthaben je Art fuer Menge und Intervall: jedes Vanilla-Partikel bringt den Faktor ein,
    // es entstehen so viele, wie ganze Einheiten zusammenkommen. So ergeben 50 % genau jedes
    // zweite, auch wenn je Ereignis nur eins kommt.
    credit: [f32; KINDS.len()],
}

impl Particles {
    pub fn new(atlas: &assets::ParticleAtlas, block_uvs: HashMap<String, [f32; 4]>) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0);
        let mut me = Self {
            rng: JavaRandom::new(seed),
            sprites: Vec::new(),
            block_uvs: HashMap::default(),
            item_uvs: HashMap::default(),
            particles: VecDeque::new(),
            pending: Vec::new(),
            emitters: Vec::new(),
            bursts: Vec::new(),
            unsupported: HashMap::default(),
            new_unsupported: Vec::new(),
            tuning: [ParticleTuning::default(); KINDS.len()],
            credit: [0.0; KINDS.len()],
        };
        me.set_textures(atlas, block_uvs);
        me
    }

    // Nach einem Resourcepack-Wechsel: neue Atlas-Koordinaten. Laufende Partikel behalten ihr
    // altes UV bis zum naechsten Bildwechsel -- das sind hoechstens ein paar Ticks.
    pub fn set_textures(&mut self, atlas: &assets::ParticleAtlas, block_uvs: HashMap<String, [f32; 4]>) {
        self.sprites = pp::PARTICLE_NAMES.iter().map(|n| atlas.sprites(n).to_vec()).collect();
        self.block_uvs = block_uvs;
        self.item_uvs = atlas.item_sprites().map(|(item, uv)| (item.to_string(), uv)).collect();
    }

    // Neue Anpassung aus den Einstellungen. Was jetzt aus ist, verschwindet sofort; Dauer,
    // Deckkraft und Tempo gelten auch fuer die schon fliegenden Partikel (die Dauer nur fuer
    // neue).
    pub fn set_tuning(&mut self, tuning: [ParticleTuning; KINDS.len()]) {
        if tuning == self.tuning {
            return;
        }
        self.tuning = tuning;
        let an = |p: &Particle| tuning[p.kind as usize].enabled;
        self.particles.retain(an);
        self.pending.retain(an);
        if !tuning[KIND_EXPLOSION].enabled {
            self.bursts.clear();
        }
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
    pub fn rng(&mut self) -> &mut JavaRandom {
        &mut self.rng
    }

    // Wie viele Partikel welcher Art gerade leben, haeufigste zuerst: der Partikeltyp, wo es
    // einen gibt, sonst die Art aus der Partikel-Anpassung. Fuer die Debug-Anzeige (F3).
    pub fn type_counts(&self) -> Vec<(String, usize)> {
        let mut zaehler: HashMap<String, usize> = HashMap::default();
        for p in self.particles.iter().filter(|p| p.behavior.renders()) {
            let name = pp::particle_name(p.type_id).map(str::to_string).unwrap_or_else(|| KINDS[p.kind as usize].key.to_string());
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
        if !self.tuning[kind].enabled {
            return;
        }
        let force = force || pp::particle_always_spawns(effect.type_id);
        let mode = self.effective_mode(ctx.mode, important);
        if !force && (ctx.camera.distance_squared(pos) > MAX_SPAWN_DISTANCE_SQ || mode == ParticleMode::Minimal) {
            return;
        }
        if let Some(mut p) = self.create(effect, pos, vel, ctx) {
            p.type_id = effect.type_id;
            // Ein Emitter-Partikel (Explosion, Windkugel) mit "alle auf einmal": seinen ganzen
            // Ausstoss jetzt, statt ueber seine Ticks verteilt.
            if !p.behavior.renders() && self.tuning[kind].all_at_once && KINDS[kind].spawn.has_all_at_once() {
                self.emit_all_now(p, ctx);
                return;
            }
            self.push(p, kind);
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
    fn spawn_count(&mut self, kind: usize) -> u32 {
        let faktor = self.tuning[kind].spawn_factor(KINDS[kind].spawn);
        if faktor == 1.0 {
            return 1;
        }
        self.credit[kind] += faktor;
        let n = self.credit[kind].floor();
        self.credit[kind] -= n;
        n as u32
    }

    // In den Bestand (am Tickende), mit der Lebensdauer der Art. Emitter zeichnen nichts --
    // ihre Dauer bliebe sonst nicht die Dauer eines Partikels, sondern die Menge, die sie
    // ausstossen.
    fn push(&mut self, mut p: Particle, kind: usize) {
        let t = self.tuning[kind];
        if !t.enabled {
            return;
        }
        if t.lifetime_pct != 100 && p.behavior.renders() {
            p.max_age = ((p.max_age as f32 * t.lifetime_pct as f32 / 100.0).round() as i32).max(1);
        }
        p.kind = kind as u8;
        // Menge/Intervall nur fuer sichtbare Partikel -- ein Emitter, der wegfiele, nahme
        // seinen ganzen Ausstoss mit.
        let n = if p.behavior.renders() { self.spawn_count(kind) } else { 1 };
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

    // Die Fabriken aus ParticleSpriteManager.registerDefaultFactories -- nur die umgesetzten.
    fn create(&mut self, effect: &ParticleEffect, pos: DVec3, v: DVec3, ctx: &SpawnContext) -> Option<Particle> {
        let t = effect.type_id;
        match t {
            pp::PARTICLE_CRIT => self.damage(t, pos, v),
            pp::PARTICLE_DAMAGE_INDICATOR => {
                // DefaultFactory: +1 nach oben, und die Lebensdauer erst NACH dem ersten Tick
                // im Konstruktor auf 20 gesetzt.
                let mut p = self.damage(t, pos, v + DVec3::Y)?;
                p.max_age = 20;
                Some(p)
            }
            pp::PARTICLE_ENCHANTED_HIT => {
                let mut p = self.damage(t, pos, v)?;
                p.color[0] *= 0.3;
                p.color[1] *= 0.8;
                Some(p)
            }
            pp::PARTICLE_TOTEM_OF_UNDYING => self.totem(pos, v),
            pp::PARTICLE_ENTITY_EFFECT => match effect.options {
                ParticleOptions::Tinted(argb) => self.spell(t, pos, v, argb),
                _ => None,
            },
            // SpellParticleOption: Farbe (ohne Deckkraft, also voll deckend) und Staerke. Die
            // Staerke streckt die Bewegung (Particle.setPower): waagrecht mal Staerke, senkrecht
            // um 0.1 herum.
            pp::PARTICLE_EFFECT | pp::PARTICLE_INSTANT_EFFECT => match effect.options {
                ParticleOptions::Effect { color, power } => {
                    let mut p = self.spell(t, pos, v, 0xFF00_0000 | (color & 0x00FF_FFFF))?;
                    let power = power as f64;
                    p.vel.x *= power;
                    p.vel.y = (p.vel.y - 0.1f32 as f64) * power + 0.1f32 as f64;
                    p.vel.z *= power;
                    Some(p)
                }
                _ => None,
            },
            pp::PARTICLE_ITEM => match &effect.options {
                ParticleOptions::Item { item_id, item_model } => {
                    // Das Modell zaehlt; eines, das hier kein Bild hat (eigenes Modell eines
                    // Packs), faellt auf das Item selbst zurueck.
                    let modell = item_model.as_deref().map(|m| m.strip_prefix("minecraft:").unwrap_or(m));
                    let name = modell.filter(|m| self.item_uvs.contains_key(*m)).or_else(|| world::item_registry::item_name(*item_id))?;
                    self.item_shard(pos, v, name)
                }
                _ => None,
            },
            pp::PARTICLE_ITEM_SLIME => self.item_shard(pos, v, "slime_ball"),
            pp::PARTICLE_ITEM_COBWEB => self.item_shard(pos, v, "cobweb"),
            pp::PARTICLE_ITEM_SNOWBALL => self.item_shard(pos, v, "snowball"),
            pp::PARTICLE_EXPLOSION => self.frozen(t, pos, v.x, |rng| 6 + rng.next_int(4), |vx| 2.0 * (1.0 - vx as f32 * 0.5)),
            pp::PARTICLE_SWEEP_ATTACK => self.frozen(t, pos, v.x, |_| 4, |vx| 1.0 - vx as f32 * 0.5),
            pp::PARTICLE_EXPLOSION_EMITTER => {
                let mut p = Particle::with_velocity(&mut self.rng, pos, DVec3::ZERO);
                p.max_age = 8;
                p.behavior = Behavior::ExplosionEmitter;
                Some(p)
            }
            pp::PARTICLE_GUST => self.gust(t, pos, 1.0),
            // GustParticle.SmallGustFactory: dieselbe Boee, scale(0.15).
            pp::PARTICLE_SMALL_GUST => self.gust(t, pos, 0.15),
            pp::PARTICLE_GUST_EMITTER_LARGE | pp::PARTICLE_GUST_EMITTER_SMALL => {
                let large = t == pp::PARTICLE_GUST_EMITTER_LARGE;
                let mut p = Particle::base(&mut self.rng, pos);
                p.max_age = if large { 7 } else { 3 };
                p.behavior = Behavior::GustEmitter { large };
                Some(p)
            }
            pp::PARTICLE_POOF => self.poof(pos, v),
            pp::PARTICLE_FLAME | pp::PARTICLE_SOUL_FIRE_FLAME | pp::PARTICLE_COPPER_FIRE_FLAME => self.flame(t, pos, v, 1.0),
            pp::PARTICLE_SMALL_FLAME => self.flame(t, pos, v, 0.5),
            pp::PARTICLE_END_ROD => self.end_rod(pos, v),
            pp::PARTICLE_CLOUD => self.cloud(t, pos, v),
            pp::PARTICLE_SNEEZE => {
                let mut p = self.cloud(t, pos, v)?;
                p.color = [0.22, 1.0, 0.53, 0.4];
                Some(p)
            }
            pp::PARTICLE_SCULK_SOUL => self.soul(t, pos, v),
            pp::PARTICLE_FLASH => match effect.options {
                ParticleOptions::Tinted(argb) => self.flash(pos, argb),
                _ => None,
            },
            pp::PARTICLE_BUBBLE_COLUMN_UP => self.bubble_column_up(pos, v),
            pp::PARTICLE_CURRENT_DOWN => self.current_down(pos),
            pp::PARTICLE_SMOKE => self.smoke(t, pos, v, 1.0),
            pp::PARTICLE_LARGE_SMOKE => self.smoke(t, pos, v, 2.5),
            pp::PARTICLE_DUST => match effect.options {
                ParticleOptions::Dust { color, scale } => self.dust(pos, v, color, scale),
                _ => None,
            },
            pp::PARTICLE_BLOCK => match effect.options {
                ParticleOptions::BlockState(state) => {
                    let block_pos = (pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
                    self.block_dust(pos, v, state, block_pos, ctx)
                }
                _ => None,
            },
            _ => {
                let n = self.unsupported.entry(t).or_insert(0);
                if *n == 0 {
                    self.new_unsupported.push(t);
                }
                *n += 1;
                None
            }
        }
    }

    // DamageParticle (crit, enchanted_hit, damage_indicator).
    fn damage(&mut self, t: i32, pos: DVec3, v: DVec3) -> Option<Particle> {
        let uv = self.random_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, uv);
        p.velocity_multiplier = 0.7;
        p.gravity = 0.5;
        p.vel *= 0.1f32 as f64;
        p.vel += v * 0.4;
        let j = rng.next_float() * 0.3 + 0.6;
        p.color = [j, j, j, 1.0];
        p.scale *= 0.75;
        p.max_age = ((6.0 / (rng.next_float() as f64 * 0.8 + 0.6)) as i32).max(1);
        p.collides = false;
        p.grows_in = true;
        p.fades_to_red = true;
        p.sheet = Sheet::Opaque;
        // Der Konstruktor ruft tick() selbst auf -- ohne Welt, weil collides schon aus ist.
        Self::tick_one_without_world(&mut p);
        Some(p)
    }

    // Ein Particle.tick ohne Kollision (DamageParticle-Konstruktor).
    fn tick_one_without_world(p: &mut Particle) {
        p.last = p.pos;
        let age = p.age;
        p.age += 1;
        if age >= p.max_age {
            p.dead = true;
        } else {
            p.vel.y -= 0.04 * p.gravity as f64;
            p.pos += p.vel;
            p.vel *= p.velocity_multiplier as f64;
        }
        if p.fades_to_red {
            p.color[1] *= 0.96;
            p.color[2] *= 0.9;
        }
    }

    // TotemParticle (AnimatedParticle mit Auftrieb 1.25).
    fn totem(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
        let t = pp::PARTICLE_TOTEM_OF_UNDYING;
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, first);
        p.velocity_multiplier = 0.6;
        p.gravity = 1.25;
        p.vel = v;
        p.scale *= 0.75;
        p.max_age = 60 + rng.next_int(12);
        p.color = if rng.next_int(4) == 0 {
            [0.6 + rng.next_float() * 0.2, 0.6 + rng.next_float() * 0.3, rng.next_float() * 0.2, 1.0]
        } else {
            [0.1 + rng.next_float() * 0.2, 0.4 + rng.next_float() * 0.3, rng.next_float() * 0.2, 1.0]
        };
        p.animated_sprites = Some(t);
        p.fades_out = true;
        p.sheet = Sheet::Translucent;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // ExplosionLargeParticle / SweepAttackParticle: stehen still und laufen nur die Bilder ab.
    fn frozen(
        &mut self,
        t: i32,
        pos: DVec3,
        vx: f64,
        max_age: impl Fn(&mut JavaRandom) -> i32,
        scale: impl Fn(f64) -> f32,
    ) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, first);
        p.max_age = max_age(rng);
        let f = rng.next_float() * 0.6 + 0.4;
        p.color = [f, f, f, 1.0];
        p.scale = scale(vx);
        p.behavior = Behavior::Frozen;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Opaque;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // GustParticle: steht still, voll hell, 12..15 Ticks, Bild nach Alter.
    fn gust(&mut self, t: i32, pos: DVec3, scale: f32) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, first);
        p.max_age = 12 + rng.next_int(4);
        p.scale = scale;
        p.set_spacing(1.0, 1.0);
        p.behavior = Behavior::Frozen;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Opaque;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // SoulParticle ueber RisingParticle (sculk_soul, die leuchtende Fassung): steigt mit
    // einem Hundertstel der verrauschten Grundgeschwindigkeit plus der mitgegebenen, Daempfung
    // 0.96, Bild nach Alter, anderthalbfach gross. Das Leuchten entfaellt -- hier ist ohnehin
    // alles voll hell.
    fn soul(&mut self, t: i32, pos: DVec3, v: DVec3) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, v).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.vel = p.vel * 0.01f32 as f64 + v;
        let streuen = |rng: &mut JavaRandom| ((rng.next_float() - rng.next_float()) * 0.05) as f64;
        p.pos += DVec3::new(streuen(rng), streuen(rng), streuen(rng));
        p.last = p.pos;
        p.max_age = (8.0 / (rng.next_float() as f64 * 0.8 + 0.2)) as i32 + 4;
        p = p.scaled(1.5);
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Translucent;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // FireworkParticles.OverlayParticle (flash): steht still, vier Ticks, in der Farbe aus dem
    // Paket (s. size/build_vertices fuer Groesse und Deckkraft).
    fn flash(&mut self, pos: DVec3, argb: u32) -> Option<Particle> {
        let uv = self.random_sprite(pp::PARTICLE_FLASH)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, uv);
        p.max_age = 4;
        let kanal = |shift: u32| ((argb >> shift) & 0xFF) as f32 / 255.0;
        p.color = [kanal(16), kanal(8), kanal(0), kanal(24)];
        p.flash = true;
        p.sheet = Sheet::Translucent;
        Some(p)
    }

    // FlameParticle ueber RisingParticle: wie die Seele steigt sie mit einem Hundertstel der
    // eigenen Zufallsbewegung plus der mitgegebenen, aber mit festem Zufallsbild, ohne
    // Kollision und kleiner werdend (s. Particle::size). small_flame ist halb so gross.
    fn flame(&mut self, t: i32, pos: DVec3, v: DVec3, scale: f32) -> Option<Particle> {
        let uv = self.random_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, v).billboard(rng, uv);
        p.velocity_multiplier = 0.96;
        p.vel = p.vel * 0.01f32 as f64 + v;
        let streuen = |rng: &mut JavaRandom| ((rng.next_float() - rng.next_float()) * 0.05) as f64;
        p.pos += DVec3::new(streuen(rng), streuen(rng), streuen(rng));
        p.last = p.pos;
        p.max_age = (8.0 / (rng.next_float() as f64 * 0.8 + 0.2)) as i32 + 4;
        if scale != 1.0 {
            p = p.scaled(scale);
        }
        p.collides = false;
        p.shrinks = true;
        p.sheet = Sheet::Opaque;
        Some(p)
    }

    // EndRodParticle (SimpleAnimatedParticle): fliegt mit der mitgegebenen Geschwindigkeit,
    // Daempfung 0.91, sinkt ganz leicht (0.0125), 60..71 Ticks, ohne Kollision. Ab der halben
    // Lebensdauer blendet er aus und laeuft dabei ins Cremegelbe (0xF2DEC9).
    fn end_rod(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
        let t = pp::PARTICLE_END_ROD;
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, first);
        p.velocity_multiplier = 0.91;
        p.gravity = 0.0125;
        p.vel = v;
        p.scale *= 0.75;
        p.max_age = 60 + rng.next_int(12);
        p.collides = false;
        p.fades_out = true;
        p.fade_color = Some([0xF2 as f32 / 255.0, 0xDE as f32 / 255.0, 0xC9 as f32 / 255.0]);
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Translucent;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // PlayerCloudParticle (cloud, sneeze): die eigene Zufallsbewegung zu einem Zehntel plus
    // die mitgegebene, Daempfung 0.96, hell, fast doppelt so gross, waechst wie Rauch ein,
    // ohne Kollision. Laeuft die Bilder nach Alter ab und haengt sich an einen Spieler darunter.
    fn cloud(&mut self, t: i32, pos: DVec3, v: DVec3) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.vel = p.vel * 0.1f32 as f64 + v;
        let f = 1.0 - rng.next_float() * 0.3;
        p.color = [f, f, f, 1.0];
        p.scale *= 1.875;
        let grund = (8.0 / (rng.next_float() as f64 * 0.8 + 0.3)) as i32;
        p.max_age = (grund as f32 * 2.5).max(1.0) as i32;
        p.collides = false;
        p.grows_in = true;
        p.follows_player = true;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Translucent;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // ExplosionSmokeParticle (poof).
    fn poof(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
        let t = pp::PARTICLE_POOF;
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, first);
        p.gravity = -0.1;
        p.velocity_multiplier = 0.9;
        let j = |c: f64, rng: &mut JavaRandom| c + ((rng.next_float() * 2.0 - 1.0) * 0.05) as f64;
        p.vel = DVec3::new(j(v.x, rng), j(v.y, rng), j(v.z, rng));
        let f = rng.next_float() * 0.3 + 0.7;
        p.color = [f, f, f, 1.0];
        p.scale = 0.1 * (rng.next_float() * rng.next_float() * 6.0 + 1.0);
        p.max_age = (16.0 / (rng.next_float() as f64 * 0.8 + 0.2)) as i32 + 2;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Opaque;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // BubbleColumnUpParticle: kleine Blase, die mit Auftrieb 0.125 steigt, bis sie das Wasser
    // verlaesst. Die mitgegebene Geschwindigkeit zaehlt nur zu einem Fuenftel.
    fn bubble_column_up(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
        let uv = self.random_sprite(pp::PARTICLE_BUBBLE_COLUMN_UP)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, uv);
        p.gravity = -0.125;
        p.velocity_multiplier = 0.85;
        p.set_spacing(0.02f32 as f64, 0.02f32 as f64);
        p.scale *= rng.next_float() * 0.6 + 0.2;
        let j = |c: f64, rng: &mut JavaRandom| c * 0.2f32 as f64 + ((rng.next_float() * 2.0 - 1.0) * 0.02) as f64;
        p.vel = DVec3::new(j(v.x, rng), j(v.y, rng), j(v.z, rng));
        p.max_age = (40.0 / (rng.next_float() as f64 * 0.8 + 0.2)) as i32;
        p.needs_water = true;
        p.sheet = Sheet::Opaque;
        Some(p)
    }

    // CurrentDownParticle: Blase im Sog einer Magma-Saeule, 30..89 Ticks, ohne Kollision.
    fn current_down(&mut self, pos: DVec3) -> Option<Particle> {
        let uv = self.random_sprite(pp::PARTICLE_CURRENT_DOWN)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, uv);
        p.max_age = (rng.next_float() * 60.0) as i32 + 30;
        p.collides = false;
        p.vel = DVec3::new(0.0, -0.05, 0.0);
        p.set_spacing(0.02f32 as f64, 0.02f32 as f64);
        p.scale *= rng.next_float() * 0.6 + 0.2;
        p.behavior = Behavior::CurrentDown { angle: 0.0 };
        p.sheet = Sheet::Opaque;
        Some(p)
    }

    // SpellParticle ueber EntityEffectFactory: die Wirbel von Trankeffekten (und geworfenen
    // Traenken). Farbe und Deckkraft kommen aus dem TintedParticleEffect.
    //
    //     Startgeschwindigkeit x/z zufaellig in -0.5..0.5, y aus dem Paket
    //     Daempfung 0.96, Schwerkraft -0.1 (steigt auf)
    //     y-Geschwindigkeit mal 0.2; sind x und z beide 0, werden sie mal 0.1 genommen
    //     Groesse mal 0.75, Lebensdauer 8 / (Zufall * 0.8 + 0.2) Ticks, keine Kollision
    //
    // effect und instant_effect sind derselbe Partikel mit eigenem Bild, Farbe und Staerke
    // (s. create): `t` waehlt das Bild.
    fn spell(&mut self, t: i32, pos: DVec3, v: DVec3, argb: u32) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let (vx, vz) = (0.5 - rng.next_double(), 0.5 - rng.next_double());
        let mut p = Particle::with_velocity(rng, pos, DVec3::new(vx, v.y, vz)).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.gravity = -0.1;
        p.ascending = true;
        p.vel.y *= 0.2f32 as f64;
        if v.x == 0.0 && v.z == 0.0 {
            p.vel.x *= 0.1f32 as f64;
            p.vel.z *= 0.1f32 as f64;
        }
        p.scale *= 0.75;
        p.max_age = (8.0 / (rng.next_double() * 0.8 + 0.2)) as i32;
        p.collides = false;
        let kanal = |shift: u32| ((argb >> shift) & 0xFF) as f32 / 255.0;
        p.color = [kanal(16), kanal(8), kanal(0), kanal(24)];
        p.alpha_target = Some(p.color[3]);
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Translucent;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // Ein Trank-Wirbel an einem Entity (LivingEntity.tickStatusEffects): Vanilla gibt als
    // "Geschwindigkeit" immer (1, 1, 1) mit.
    // Level.addParticle aus einem Entity-Tick (force = false, important = false).
    pub fn add_world_particle(&mut self, effect: &ParticleEffect, pos: DVec3, vel: DVec3, ctx: &SpawnContext) {
        self.add_client(effect, false, false, pos, vel, ctx);
    }

    pub fn add_potion_swirl(&mut self, argb: u32, pos: DVec3, ctx: &SpawnContext) {
        let effect = ParticleEffect { type_id: pp::PARTICLE_ENTITY_EFFECT, options: ParticleOptions::Tinted(argb) };
        self.add_client(&effect, false, false, pos, DVec3::ONE, ctx);
    }

    // FireSmokeParticle / LargeFireSmokeParticle (AscendingParticle mit 0.1er Rauschen,
    // Farbfaktor 0.3, Grundlebensdauer 8, Auftrieb -0.1).
    fn smoke(&mut self, t: i32, pos: DVec3, v: DVec3, scale_mul: f32) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.gravity = -0.1;
        p.ascending = true;
        p.vel *= 0.1f32 as f64;
        p.vel += v;
        let f = rng.next_float() * 0.3;
        p.color = [f, f, f, 1.0];
        p.scale *= 0.75 * scale_mul;
        p.max_age = ((8.0 / (rng.next_float() as f64 * 0.8 + 0.2) * scale_mul as f64) as i32).max(1);
        p.grows_in = true;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Opaque;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // RedDustParticle (AbstractDustParticle). DustParticleEffect klemmt die Groesse auf 0.01..4.
    fn dust(&mut self, pos: DVec3, v: DVec3, color: u32, scale: f32) -> Option<Particle> {
        let t = pp::PARTICLE_DUST;
        let first = self.first_sprite(t)?;
        let scale = scale.clamp(0.01, 4.0);
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, v).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.ascending = true;
        p.vel *= 0.1f32 as f64;
        p.scale *= 0.75 * scale;
        let i = (8.0 / (rng.next_double() * 0.8 + 0.2)) as i32;
        p.max_age = ((i as f32 * scale).max(1.0)) as i32;
        let f = rng.next_float() * 0.4 + 0.6;
        let channel = |shift: u32| ((color >> shift) & 0xFF) as f32 / 255.0;
        let darken = |c: f32, rng: &mut JavaRandom| (rng.next_float() * 0.2 + 0.8) * c * f;
        p.color = [darken(channel(16), rng), darken(channel(8), rng), darken(channel(0), rng), 1.0];
        p.grows_in = true;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Opaque;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // BreakingItemParticle: ein zufaelliges Viertel des Partikelbilds des Items, halb so gross,
    // mit voller Schwerkraft. Die eigene Zufallsbewegung zaehlt nur zu einem Zehntel, dazu
    // kommt die mitgegebene. Ohne Bild im Atlas (Modell ohne Partikeltextur) kein Splitter.
    fn item_shard(&mut self, pos: DVec3, v: DVec3, name: &str) -> Option<Particle> {
        let sprite = *self.item_uvs.get(name)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, sprite);
        p.gravity = 1.0;
        p.scale /= 2.0;
        let su = rng.next_float() * 3.0;
        let sv = rng.next_float() * 3.0;
        p.vel = p.vel * 0.1f32 as f64 + v;
        let fu = |f: f32| sprite[0] + (sprite[2] - sprite[0]) * f;
        let fv = |f: f32| sprite[1] + (sprite[3] - sprite[1]) * f;
        p.uv = [fu((su + 1.0) / 4.0), fv(sv / 4.0), fu(su / 4.0), fv((sv + 1.0) / 4.0)];
        p.sheet = Sheet::Opaque;
        Some(p)
    }

    // Ein Wurf- oder Verweiltrank zerplatzt (Level-Ereignis 2002, Sofort-Tranke 2007): acht
    // Splitter der Flasche und hundert Effekt-Partikel in der Trankfarbe, die aus dem
    // Aufschlagpunkt ringfoermig auseinanderfliegen -- je weiter, desto schneller.
    pub fn add_potion_splash(&mut self, pos: (i32, i32, i32), rgb: u32, instant: bool, ctx: &SpawnContext) {
        let mitte = DVec3::new(pos.0 as f64 + 0.5, pos.1 as f64, pos.2 as f64 + 0.5);
        let splitter = item_effect("splash_potion");
        for _ in 0..8 {
            let v = DVec3::new(self.rng.next_gaussian() * 0.15, self.rng.next_double() * 0.2, self.rng.next_gaussian() * 0.15);
            self.add_client(&splitter, false, false, mitte, v, ctx);
        }
        let t = if instant { pp::PARTICLE_INSTANT_EFFECT } else { pp::PARTICLE_EFFECT };
        let kanal = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
        let (r, g, b) = (kanal(16), kanal(8), kanal(0));
        for _ in 0..100 {
            let weite = self.rng.next_double() * 4.0;
            let winkel = self.rng.next_double() * std::f64::consts::PI * 2.0;
            let (vx, vz) = (winkel.cos() * weite, winkel.sin() * weite);
            let vy = 0.01 + self.rng.next_double() * 0.5;
            let f = 0.75 + self.rng.next_float() * 0.25;
            let byte = |c: f32| ((c * f).clamp(0.0, 1.0) * 255.0).round() as u32;
            let color = (byte(r) << 16) | (byte(g) << 8) | byte(b);
            let effect = ParticleEffect { type_id: t, options: ParticleOptions::Effect { color, power: weite as f32 } };
            let ort = mitte + DVec3::new(vx * 0.1, 0.3, vz * 0.1);
            self.add_client(&effect, false, false, ort, DVec3::new(vx, vy, vz), ctx);
        }
    }

    // LivingEntity.spawnItemParticles: Kruemel beim Essen und Splitter eines zerbrochenen
    // Werkzeugs. Sie entstehen vor dem Gesicht (0.6 vor, 0.3 bis 0.9 unter den Augen, etwas
    // seitlich gestreut) und fliegen leicht nach oben, beides mit dem Blick gedreht.
    // `pitch_deg` in Vanillas Richtung (positiv = nach unten).
    #[allow(clippy::too_many_arguments)]
    pub fn add_item_use_particles(&mut self, item: &str, count: u32, eye: DVec3, yaw_deg: f32, pitch_deg: f32, ctx: &SpawnContext) {
        let effect = item_effect(item);
        let (xr, yr) = ((-pitch_deg).to_radians(), (-yaw_deg).to_radians());
        for _ in 0..count {
            let r = &mut self.rng;
            let v = DVec3::new((r.next_float() as f64 - 0.5) * 0.1, r.next_float() as f64 * 0.1 + 0.1, 0.0);
            let v = rot_y(rot_x(v, xr), yr);
            let tiefe = -(r.next_float() as f64) * 0.6 - 0.3;
            let p = DVec3::new((r.next_float() as f64 - 0.5) * 0.3, tiefe, 0.6);
            let p = rot_y(rot_x(p, xr), yr) + eye;
            self.add_client(&effect, false, false, p, v + DVec3::new(0.0, 0.05, 0.0), ctx);
        }
    }

    // Ein Schneeball oder Ei schlaegt auf (Entity-Ereignis 3): acht Splitter des geworfenen
    // Items. Beim Ei fliegen sie leicht auseinander, beim Schneeball fallen sie einfach.
    pub fn add_thrown_item_hit(&mut self, item: &str, pos: DVec3, streuen: bool, ctx: &SpawnContext) {
        let effect = item_effect(item);
        for _ in 0..8 {
            let v = if streuen {
                let r = &mut self.rng;
                DVec3::new((r.next_float() as f64 - 0.5) * 0.08, (r.next_float() as f64 - 0.5) * 0.08, (r.next_float() as f64 - 0.5) * 0.08)
            } else {
                DVec3::ZERO
            };
            self.add_client(&effect, false, false, pos, v, ctx);
        }
    }

    // AreaEffectCloud.clientTick, ein Tick einer Trankwolke am Boden (Verweiltrank, Drachenatem):
    // Partikel gleichmaessig ueber die Kreisflaeche, so viele wie die Flaeche gross ist. Solange
    // die Wolke noch wartet (die ersten Ticks), nur zwei in der Mitte und nur in jedem zweiten
    // Tick; Effekt-Wirbel sind dann zur Haelfte weiss. Andere Partikelarten treiben leicht.
    pub fn tick_area_cloud(&mut self, pos: DVec3, radius: f32, waiting: bool, particle: &ParticleEffect, ctx: &SpawnContext) {
        if waiting && self.rng.next_int(2) == 0 {
            return;
        }
        let (anzahl, r) = if waiting { (2, 0.2f32) } else { ((std::f32::consts::PI * radius * radius).ceil() as i32, radius) };
        let weiss = ParticleEffect { type_id: pp::PARTICLE_ENTITY_EFFECT, options: ParticleOptions::Tinted(0xFFFF_FFFF) };
        for _ in 0..anzahl.clamp(0, 4096) {
            let winkel = self.rng.next_float() * std::f32::consts::TAU;
            let abstand = self.rng.next_float().sqrt() * r;
            let ort = DVec3::new(pos.x + (winkel.cos() * abstand) as f64, pos.y, pos.z + (winkel.sin() * abstand) as f64);
            if particle.type_id == pp::PARTICLE_ENTITY_EFFECT {
                let e = if waiting && self.rng.next_int(2) == 0 { &weiss } else { particle };
                self.add_client(e, false, true, ort, DVec3::ZERO, ctx);
            } else if waiting {
                self.add_client(particle, false, true, ort, DVec3::ZERO, ctx);
            } else {
                let vx = (0.5 - self.rng.next_double()) * 0.15;
                let vz = (0.5 - self.rng.next_double()) * 0.15;
                self.add_client(particle, false, true, ort, DVec3::new(vx, 0.01f32 as f64, vz), ctx);
            }
        }
    }

    // BlockDustParticle: ein zufaelliges Viertel der Partikeltextur des Blocks, grau
    // abgedunkelt und mit der Biomfarbe des Blocks gefaerbt wie im Gelaende (Gras-Pflanzen,
    // Laub). Der Grasblock bleibt ungefaerbt: seine Partikeltextur ist die Erdseite.
    fn block_dust(&mut self, pos: DVec3, v: DVec3, state: u32, block_pos: (i32, i32, i32), ctx: &SpawnContext) -> Option<Particle> {
        let name = world::block_name(state)?;
        if !has_block_break_particles(name) {
            return None;
        }
        let sprite = *self.block_uvs.get(name)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, v).billboard(rng, sprite);
        p.gravity = 1.0;
        let farben = || ctx.biomes.colors_of(ctx.world.biome_at(block_pos.0, block_pos.1, block_pos.2));
        let [r, g, b] = match world::biome_tint::tint_kind(name) {
            world::biome_tint::TintKind::GrassAll => farben().grass,
            world::biome_tint::TintKind::FoliageAll => farben().foliage,
            world::biome_tint::TintKind::DryFoliageAll => farben().dry_foliage,
            world::biome_tint::TintKind::Fixed(rgb) => [((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0],
            world::biome_tint::TintKind::None | world::biome_tint::TintKind::GrassTop => [1.0; 3],
        };
        p.color = [0.6 * r, 0.6 * g, 0.6 * b, 1.0];
        p.scale /= 2.0;
        let su = rng.next_float() * 3.0;
        let sv = rng.next_float() * 3.0;
        // Sprite.getFrameU/V; minU > maxU ist Vanillas eigene Spiegelung.
        let fu = |f: f32| sprite[0] + (sprite[2] - sprite[0]) * f;
        let fv = |f: f32| sprite[1] + (sprite[3] - sprite[1]) * f;
        p.uv = [fu((su + 1.0) / 4.0), fv(sv / 4.0), fu(su / 4.0), fv((sv + 1.0) / 4.0)];
        p.sheet = Sheet::Block;
        Some(p)
    }

    // --- Ausloeser ---------------------------------------------------------------------

    // ClientPlayNetworkHandler.onParticle.
    pub fn on_packet(&mut self, pk: &pp::LevelParticles, ctx: &SpawnContext) {
        let pos = DVec3::new(pk.pos.0, pk.pos.1, pk.pos.2);
        let (ox, oy, oz) = (pk.offset.0 as f64, pk.offset.1 as f64, pk.offset.2 as f64);
        let speed = pk.speed as f64;
        if pk.count == 0 {
            let vel = DVec3::new(speed * ox, speed * oy, speed * oz);
            self.add_client(&pk.particle, pk.force_spawn, pk.important, pos, vel, ctx);
            return;
        }
        for _ in 0..pk.count.min(MAX_PER_SHEET as i32) {
            let g = self.rng.next_gaussian() * ox;
            let h = self.rng.next_gaussian() * oy;
            let j = self.rng.next_gaussian() * oz;
            let k = self.rng.next_gaussian() * speed;
            let l = self.rng.next_gaussian() * speed;
            let m = self.rng.next_gaussian() * speed;
            self.add_client(&pk.particle, pk.force_spawn, pk.important, pos + DVec3::new(g, h, j), DVec3::new(k, l, m), ctx);
        }
    }

    // ClientPlayNetworkHandler.onExplosion (ohne Ton und Rueckstoss).
    //
    // Die Art "Explosion" umfasst Wolke und Truemmer (add_client ordnet die Wolke zu) -- die
    // Boee einer Windkugel ist keine Explosion im Sinne der Anpassung, sie hat ihre eigene.
    pub fn on_explosion(&mut self, e: &pp::Explosion, ctx: &SpawnContext) {
        let center = DVec3::new(e.center.0, e.center.1, e.center.2);
        self.add_client(&e.particle, false, false, center, DVec3::new(1.0, 0.0, 0.0), ctx);
        if self.tuning[KIND_EXPLOSION].enabled && !e.block_particles.is_empty() {
            self.bursts.push(ExplosionBurst {
                center,
                radius: e.radius,
                block_count: e.block_count,
                particles: e.block_particles.clone(),
            });
        }
    }

    // ParticleManager.addEmitter: Krit (3 Ticks) und Totem (30 Ticks). Der Konstruktor tickt
    // einmal sofort -- der erste Schwall kommt also im selben Frame wie das Paket.
    pub fn add_emitter(
        &mut self,
        entity: i32,
        type_id: i32,
        max_age: i32,
        body: &dyn Fn(i32) -> Option<EmitterBody>,
        ctx: &SpawnContext,
    ) {
        let mut em = Emitter { entity, effect: ParticleEffect { type_id, options: ParticleOptions::None }, age: 0, max_age };
        // "Alle auf einmal": alle Ticks des Emitters jetzt (das Totem etwa sonst 30 Ticks lang).
        let sofort = kind_of(type_id).is_some_and(|k| self.tuning[k].all_at_once && KINDS[k].spawn.has_all_at_once());
        if sofort {
            for _ in 0..max_age.max(1) {
                if !self.tick_emitter(&mut em, body, ctx) {
                    break;
                }
            }
            return;
        }
        if self.tick_emitter(&mut em, body, ctx) {
            self.emitters.push(em);
        }
    }

    // EmitterParticle.tick. false = fertig.
    fn tick_emitter(&mut self, em: &mut Emitter, body: &dyn Fn(i32) -> Option<EmitterBody>, ctx: &SpawnContext) -> bool {
        let Some(b) = body(em.entity) else { return false };
        for _ in 0..16 {
            let d = (self.rng.next_float() * 2.0 - 1.0) as f64;
            let e = (self.rng.next_float() * 2.0 - 1.0) as f64;
            let f = (self.rng.next_float() * 2.0 - 1.0) as f64;
            if d * d + e * e + f * f > 1.0 {
                continue;
            }
            let pos = DVec3::new(b.pos.x + b.width * (d / 4.0), b.pos.y + b.height * (0.5 + e / 4.0), b.pos.z + b.width * (f / 4.0));
            let effect = em.effect.clone();
            self.add_client(&effect, false, false, pos, DVec3::new(d, e + 0.2, f), ctx);
        }
        em.age += 1;
        em.age < em.max_age
    }

    // LivingEntity.addDeathParticles (Status 60): 20 Rauchwoelkchen um den Koerper.
    pub fn add_death_poof(&mut self, b: EmitterBody, ctx: &SpawnContext) {
        let effect = ParticleEffect { type_id: pp::PARTICLE_POOF, options: ParticleOptions::None };
        for _ in 0..20 {
            let d = self.rng.next_gaussian() * 0.02;
            let e = self.rng.next_gaussian() * 0.02;
            let f = self.rng.next_gaussian() * 0.02;
            let x = b.pos.x + b.width * (2.0 * self.rng.next_double() - 1.0);
            let y = b.pos.y + b.height * self.rng.next_double();
            let z = b.pos.z + b.width * (2.0 * self.rng.next_double() - 1.0);
            self.add_client(&effect, false, false, DVec3::new(x - d * 10.0, y - e * 10.0, z - f * 10.0), DVec3::new(d, e, f), ctx);
        }
    }

    // ClientWorld.addBlockBreakParticles (Level Event 2001 und der eigene Abbau): ein Raster
    // von Staubkoernern je Umrisskasten, mindestens 2 je Achse, sonst eins je Viertelblock.
    // Geht AN der Partikel-Stufe vorbei direkt in den Bestand, wie in Vanilla.
    pub fn add_block_break(&mut self, pos: (i32, i32, i32), state: u32, ctx: &SpawnContext) {
        let shapes = ctx.shapes;
        let origin = DVec3::new(pos.0 as f64, pos.1 as f64, pos.2 as f64);
        for b in world::outline_boxes(state, shapes) {
            let min = DVec3::new(b.min[0] as f64, b.min[1] as f64, b.min[2] as f64);
            let size = DVec3::new(b.max[0] as f64, b.max[1] as f64, b.max[2] as f64) - min;
            let size = size.min(DVec3::ONE);
            let steps = |s: f64| ((s / 0.25).ceil() as i32).max(2);
            let (ni, nj, nk) = (steps(size.x), steps(size.y), steps(size.z));
            for l in 0..ni {
                for m in 0..nj {
                    for n in 0..nk {
                        let g = (l as f64 + 0.5) / ni as f64;
                        let h = (m as f64 + 0.5) / nj as f64;
                        let o = (n as f64 + 0.5) / nk as f64;
                        let p = origin + DVec3::new(g * size.x + min.x, h * size.y + min.y, o * size.z + min.z);
                        if let Some(p) = self.block_dust(p, DVec3::new(g - 0.5, h - 0.5, o - 0.5), state, pos, ctx) {
                            self.push(p, KIND_BLOCK);
                        }
                    }
                }
            }
        }
    }

    // ClientWorld.spawnBlockBreakingParticle: ein Staubkorn je Abbau-Tick auf der getroffenen
    // Flaeche, etwas davor. `face` im Vanilla-Index (0 unten, 1 oben, 2 N, 3 S, 4 W, 5 O).
    pub fn add_block_hit(&mut self, pos: (i32, i32, i32), face: u8, ctx: &SpawnContext) {
        let (world, shapes) = (ctx.world, ctx.shapes);
        let state = world.block_at(pos.0, pos.1, pos.2);
        let boxes = world::outline_boxes(state, shapes);
        if boxes.is_empty() {
            return;
        }
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for b in &boxes {
            min = min.min(Vec3::from(b.min));
            max = max.max(Vec3::from(b.max));
        }
        let (min, max) = (min.as_dvec3(), max.as_dvec3());
        let o = DVec3::new(pos.0 as f64, pos.1 as f64, pos.2 as f64);
        let r = &mut self.rng;
        let inset = 0.1f32 as f64;
        let mut x = o.x + r.next_double() * (max.x - min.x - 0.2f32 as f64) + inset + min.x;
        let mut y = o.y + r.next_double() * (max.y - min.y - 0.2f32 as f64) + inset + min.y;
        let mut z = o.z + r.next_double() * (max.z - min.z - 0.2f32 as f64) + inset + min.z;
        match face {
            0 => y = o.y + min.y - inset,
            1 => y = o.y + max.y + inset,
            2 => z = o.z + min.z - inset,
            3 => z = o.z + max.z + inset,
            4 => x = o.x + min.x - inset,
            5 => x = o.x + max.x + inset,
            _ => {}
        }
        if let Some(p) = self.block_dust(DVec3::new(x, y, z), DVec3::ZERO, state, pos, ctx) {
            self.push(p.speed(0.2).scaled(0.6), KIND_BLOCK);
        }
    }

    // --- Umgebung ----------------------------------------------------------------------

    // ClientWorld.doRandomBlockDisplayTicks: je Tick 667 zufaellige Bloecke im Wuerfel mit
    // Kantenlaenge 16 um den Spieler und noch einmal so viele mit 32, jeder darf eigene
    // Partikel und Geraeusche machen (Block.randomDisplayTick). Die Versaetze sind Differenzen
    // zweier Zufallszahlen, die Mitte kommt also deutlich oefter dran als der Rand.
    //
    // Umgesetzt ist bisher nur die Blasensaeule. Die Geraeusche gibt die Funktion zurueck, die
    // App spielt sie.
    pub fn animate_blocks(&mut self, center: (i32, i32, i32), ctx: &SpawnContext) -> Vec<AmbientSound> {
        let mut sounds = Vec::new();
        for _ in 0..667 {
            self.animate_block(center, 16, ctx, &mut sounds);
            self.animate_block(center, 32, ctx, &mut sounds);
        }
        sounds
    }

    fn animate_block(&mut self, c: (i32, i32, i32), radius: i32, ctx: &SpawnContext, sounds: &mut Vec<AmbientSound>) {
        let r = &mut self.rng;
        let x = c.0 + r.next_int(radius) - r.next_int(radius);
        let y = c.1 + r.next_int(radius) - r.next_int(radius);
        let z = c.2 + r.next_int(radius) - r.next_int(radius);
        let state = ctx.world.block_at(x, y, z);
        if state == world::STATE_AIR {
            return;
        }
        if world::block_name(state) == Some("bubble_column") {
            let drag = world::state_props::property(state, "drag") == Some("true");
            self.bubble_column_tick((x, y, z), drag, ctx, sounds);
        }
    }

    // BubbleColumnBlock.randomDisplayTick. `drag` = Magma darunter: Sog nach unten statt
    // aufsteigender Blasen. Der Sog-Partikel sitzt in Vanilla nicht mittig in z, sondern an
    // der Blockkante -- uebernommen, damit es gleich aussieht.
    fn bubble_column_tick(&mut self, pos: (i32, i32, i32), drag: bool, ctx: &SpawnContext, sounds: &mut Vec<AmbientSound>) {
        let o = DVec3::new(pos.0 as f64, pos.1 as f64, pos.2 as f64);
        let sound = if drag {
            let effect = ParticleEffect { type_id: pp::PARTICLE_CURRENT_DOWN, options: ParticleOptions::None };
            self.add_client(&effect, false, true, o + DVec3::new(0.5, 0.8, 0.0), DVec3::ZERO, ctx);
            "minecraft:block.bubble_column.whirlpool_ambient"
        } else {
            let effect = ParticleEffect { type_id: pp::PARTICLE_BUBBLE_COLUMN_UP, options: ParticleOptions::None };
            let up = DVec3::new(0.0, 0.04, 0.0);
            self.add_client(&effect, false, true, o + DVec3::new(0.5, 0.0, 0.5), up, ctx);
            let r = &mut self.rng;
            let streu = DVec3::new(r.next_float() as f64, r.next_float() as f64, r.next_float() as f64);
            self.add_client(&effect, false, true, o + streu, up, ctx);
            "minecraft:block.bubble_column.upwards_ambient"
        };
        let r = &mut self.rng;
        if r.next_int(200) == 0 {
            let volume = 0.2 + r.next_float() * 0.2;
            let pitch = 0.9 + r.next_float() * 0.15;
            sounds.push(AmbientSound { name: sound, pos: o, volume, pitch });
        }
    }

    // --- Tick --------------------------------------------------------------------------

    // Ein Spieltick: erst die Explosions-Blockpartikel (ClientWorld.tick), dann der
    // ParticleManager (bestehende Partikel, Emitter, dann die neuen uebernehmen).
    pub fn tick(&mut self, body: &dyn Fn(i32) -> Option<EmitterBody>, ctx: &SpawnContext) {
        self.tick_explosion_bursts(ctx);

        let mut spawns: Vec<(DVec3, f64)> = Vec::new();
        let mut boeen: Vec<(DVec3, f64)> = Vec::new();
        // Tempo != 100 %: ein Partikel tickt so oft, wie sich sein Zeitguthaben auf ganze
        // Ticks summiert -- bei 50 % jeden zweiten Spieltick, bei 200 % zweimal je Spieltick.
        // Zwischen seinen Ticks gleitet das Bild ueber `phase` weiter (s. build_vertices).
        let tuning = self.tuning;
        let sprites = &self.sprites;
        for p in self.particles.iter_mut() {
            let speed = tuning[p.kind as usize].speed();
            if speed == 1.0 {
                Self::tick_one(p, sprites, ctx, &mut spawns, &mut boeen);
                continue;
            }
            p.phase += speed;
            while p.phase >= 1.0 && !p.dead {
                p.phase -= 1.0;
                Self::tick_one(p, sprites, ctx, &mut spawns, &mut boeen);
            }
        }
        self.particles.retain(|p| !p.dead);
        self.spawn_emitted(spawns, boeen, ctx);

        let mut emitters = std::mem::take(&mut self.emitters);
        emitters.retain_mut(|em| self.tick_emitter(em, body, ctx));
        emitters.append(&mut self.emitters);
        self.emitters = emitters;

        for p in self.pending.drain(..) {
            self.particles.push_back(p);
        }
        // EvictingQueue: das Aelteste faellt raus. Hier ueber alle Blaetter zusammen -- mehr
        // als 16384 gleichzeitig sieht man ohnehin nur bei Massen-Explosionen.
        while self.particles.len() > MAX_PER_SHEET {
            self.particles.pop_front();
        }
    }

    // Was die Emitter-Partikel in ihren Ticks ausgeloest haben (s. tick_one).
    fn spawn_emitted(&mut self, spawns: Vec<(DVec3, f64)>, boeen: Vec<(DVec3, f64)>, ctx: &SpawnContext) {
        // ExplosionEmitterParticle.tick: sechs Explosionsbilder je Tick im 4er-Umkreis.
        let explosion = ParticleEffect { type_id: pp::PARTICLE_EXPLOSION, options: ParticleOptions::None };
        for (center, progress) in spawns {
            let r = &mut self.rng;
            let d = center.x + (r.next_double() - r.next_double()) * 4.0;
            let e = center.y + (r.next_double() - r.next_double()) * 4.0;
            let f = center.z + (r.next_double() - r.next_double()) * 4.0;
            self.add_client(&explosion, false, false, DVec3::new(d, e, f), DVec3::new(progress, 0.0, 0.0), ctx);
        }

        // GustEmitterParticle.tick: drei Boeen je Ausloesung, um +-Streuung verteilt.
        let boee = ParticleEffect { type_id: pp::PARTICLE_GUST, options: ParticleOptions::None };
        for (center, streuung) in boeen {
            let r = &mut self.rng;
            let d = center.x + (r.next_double() - r.next_double()) * streuung;
            let e = center.y + (r.next_double() - r.next_double()) * streuung;
            let f = center.z + (r.next_double() - r.next_double()) * streuung;
            self.add_client(&boee, false, false, DVec3::new(d, e, f), DVec3::ZERO, ctx);
        }
    }

    // Ein Tick eines Partikels: Bewegung nach seiner Art, dann Bild nach Alter und Ausblenden.
    // Was Emitter ausloesen, sammeln `spawns` (Explosion) und `boeen` (Windkugel).
    fn tick_one(p: &mut Particle, sprites: &[Vec<[f32; 4]>], ctx: &SpawnContext, spawns: &mut Vec<(DVec3, f64)>, boeen: &mut Vec<(DVec3, f64)>) {
        let shapes = ctx.shapes;
        match p.behavior {
            Behavior::Physics => {
                if p.collides {
                    p.tick_physics(ctx.world, shapes);
                } else {
                    // Ohne Welt, aber danach wie jeder andere weiter (Sprite nach Alter,
                    // Ausblenden) -- die Damage-Partikel nutzen beides nicht.
                    Self::tick_one_without_world(p);
                }
                if p.needs_water && !p.dead && !in_water(ctx.world, p.pos) {
                    p.dead = true;
                }
            }
            Behavior::CurrentDown { angle } => {
                p.last = p.pos;
                let age = p.age;
                p.age += 1;
                if age >= p.max_age {
                    p.dead = true;
                } else {
                    // Der Schub kreist mit dem Winkel und wird gleich wieder auf 7 %
                    // gedaempft -- daher die kleinen Spiralen beim Sinken.
                    p.vel.x = (p.vel.x + (0.6 * angle.cos()) as f64) * 0.07;
                    p.vel.z = (p.vel.z + (0.6 * angle.sin()) as f64) * 0.07;
                    p.move_by(p.vel, ctx.world, shapes);
                    if !in_water(ctx.world, p.pos) || p.on_ground {
                        p.dead = true;
                    }
                    p.behavior = Behavior::CurrentDown { angle: angle + 0.08 };
                }
            }
            Behavior::Frozen => {
                p.last = p.pos;
                let age = p.age;
                p.age += 1;
                if age >= p.max_age {
                    p.dead = true;
                }
            }
            Behavior::GustEmitter { large } => {
                let (streuung, intervall) = if large { (3.0, 0) } else { (1.0, 2) };
                if p.age % (intervall + 1) == 0 {
                    for _ in 0..3 {
                        boeen.push((p.pos, streuung));
                    }
                }
                let age = p.age;
                p.age += 1;
                if age == p.max_age {
                    p.dead = true;
                }
            }
            Behavior::ExplosionEmitter => {
                for _ in 0..6 {
                    spawns.push((p.pos, p.age as f64 / p.max_age as f64));
                }
                p.age += 1;
                if p.age == p.max_age {
                    p.dead = true;
                }
            }
        }
        if p.dead {
            return;
        }
        if let Some(t) = p.animated_sprites {
            if let Some(list) = sprites.get(t as usize) {
                p.uv = sprite_by_age(list, p.age, p.max_age);
            }
        }
        if p.fades_out && p.age > p.max_age / 2 {
            p.color[3] = 1.0 - (p.age as f32 - (p.max_age / 2) as f32) / p.max_age as f32;
            if let Some(ziel) = p.fade_color {
                for i in 0..3 {
                    p.color[i] += (ziel[i] - p.color[i]) * 0.2;
                }
            }
        }
        if p.follows_player {
            if let Some(spieler) = ctx.player.filter(|s| s.pos.distance_squared(p.pos) <= 4.0) {
                if p.pos.y > spieler.pos.y {
                    p.pos.y += (spieler.pos.y - p.pos.y) * 0.2;
                    p.vel.y += (spieler.vel_y - p.vel.y) * 0.2;
                }
            }
        }
        if let Some(ziel) = p.alpha_target {
            p.color[3] += (ziel - p.color[3]) * 0.05;
        }
    }

    // BlockParticleEffectsManager.tick.
    fn tick_explosion_bursts(&mut self, ctx: &SpawnContext) {
        if self.bursts.is_empty() {
            return;
        }
        let bursts = std::mem::take(&mut self.bursts);
        if ctx.mode != ParticleMode::All {
            return;
        }
        let total: i32 = bursts.iter().map(|b| b.block_count.max(0)).sum();
        if total <= 0 {
            return;
        }
        for _ in 0..total.min(MAX_EXPLOSION_BLOCK_PARTICLES) {
            let mut pick = self.rng.next_int(total);
            let Some(b) = bursts.iter().find(|b| {
                pick -= b.block_count.max(0);
                pick < 0
            }) else {
                continue;
            };
            self.explosion_block_particle(b, ctx);
        }
    }

    // BlockParticleEffectsManager.addEffect.
    fn explosion_block_particle(&mut self, b: &ExplosionBurst, ctx: &SpawnContext) {
        let r = &mut self.rng;
        let dir = DVec3::new(
            (r.next_float() * 2.0 - 1.0) as f64,
            (r.next_float() * 2.0 - 1.0) as f64,
            (r.next_float() * 2.0 - 1.0) as f64,
        )
        .normalize_or_zero();
        let f = (r.next_float().cbrt() * b.radius) as f64;
        let offset = dir * f;
        let p = b.center + offset;
        if ctx.world.block_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) != world::STATE_AIR {
            return;
        }
        let g = 0.5 / (f / b.radius as f64 + 0.1) * r.next_float() as f64 * r.next_float() as f64 + 0.3;
        let total: i32 = b.particles.iter().map(|e| e.weight.max(0)).sum();
        if total <= 0 {
            return;
        }
        let mut pick = r.next_int(total);
        let Some(bp) = b.particles.iter().find(|e| {
            pick -= e.weight.max(0);
            pick < 0
        }) else {
            return;
        };
        let pos = b.center + offset * bp.scaling as f64;
        let vel = dir * (g * bp.speed as f64);
        let effect = bp.particle.clone();
        self.add_client_as(KIND_EXPLOSION, &effect, false, false, pos, vel, ctx);
    }

    // --- Zeichnen ----------------------------------------------------------------------

    // Kamerazugewandte Quads fuer dieses Bild, nach Blatt sortiert (opak, durchscheinend,
    // Blockstaub -- die Reihenfolge von render::ParticleDraws). `eye` ist der Ursprung des
    // augenrelativen Renderraums, `right`/`up` spannen die Bildebene auf.
    pub fn build_vertices(&self, eye: DVec3, right: Vec3, up: Vec3, tick_progress: f32, out: &mut Vec<ParticleVertex>) -> ParticleDraws {
        out.clear();
        let mut draws = ParticleDraws::default();
        for (sheet, count) in [
            (Sheet::Opaque, &mut draws.opaque),
            (Sheet::Translucent, &mut draws.translucent),
            (Sheet::Block, &mut draws.block),
        ] {
            let before = out.len();
            for p in self.particles.iter().filter(|p| self.sheet_of(p) == sheet && p.behavior.renders()) {
                let t = self.tuning[p.kind as usize];
                // Bei eigenem Tempo laeuft die Zeit des Partikels anders: `phase` ist schon
                // vergangen, der Rest des Spielticks zaehlt mal Tempo. Ueber 1 hinaus (mehr als
                // ein eigener Tick je Spieltick) geht es den letzten Schritt entlang weiter.
                let progress = if t.speed_pct == 100 { tick_progress } else { p.phase + tick_progress * t.speed() };
                let center = (p.last.lerp(p.pos, progress as f64) - eye).as_vec3();
                let s = p.size(progress);
                let mut color = p.color;
                if p.flash {
                    // Die Deckkraft aus den Optionen ueberschreibt Vanilla jedes Bild hiermit.
                    color[3] = 0.6 - (p.age as f32 + progress - 1.0) * 0.25 * 0.5;
                }
                color[3] *= t.opacity();
                let (r, u) = (right * s, up * s);
                let [u0, v0, u1, v1] = p.uv;
                // BillboardParticleSubmittable: (1,-1) (1,1) (-1,1) (-1,-1) mit
                // (maxU,maxV) (maxU,minV) (minU,minV) (minU,maxV).
                let corners = [
                    (center + r - u, [u1, v1]),
                    (center + r + u, [u1, v0]),
                    (center - r + u, [u0, v0]),
                    (center - r - u, [u0, v1]),
                ];
                for i in [0, 1, 2, 0, 2, 3] {
                    let (pos, uv) = corners[i];
                    out.push(ParticleVertex { position: pos.to_array(), color, uv });
                }
            }
            *count = (out.len() - before) as u32;
        }
        draws
    }

    // Opake Partikel mischen nicht -- mit weniger Deckkraft gehen sie ins durchscheinende
    // Blatt (derselbe Atlas, nur mit Mischung).
    fn sheet_of(&self, p: &Particle) -> Sheet {
        if p.sheet == Sheet::Opaque && self.tuning[p.kind as usize].opacity_pct < 100 {
            Sheet::Translucent
        } else {
            p.sheet
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
    let (c, s) = (a.cos() as f64, a.sin() as f64);
    DVec3::new(v.x, v.y * c + v.z * s, v.z * c - v.y * s)
}

fn rot_y(v: DVec3, a: f32) -> DVec3 {
    let (c, s) = (a.cos() as f64, a.sin() as f64);
    DVec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
}

fn has_block_break_particles(name: &str) -> bool {
    !matches!(name, "air" | "cave_air" | "void_air" | "barrier" | "moving_piston" | "structure_void")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Die ersten Werte von new Random(42) -- aus der JDK-Dokumentation bekannt und leicht
    // nachpruefbar. Stimmt das LCG, stimmen auch Gauss und nextInt.
    fn java_random_stimmt_mit_der_jdk_folge_ueberein() {
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(10), 0);
        assert_eq!(r.next_int(10), 3);
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next(32), -1170105035);
    }

    #[test]
    fn sprite_nach_alter_laeuft_die_liste_ab() {
        let list: Vec<[f32; 4]> = (0..8).map(|i| [i as f32; 4]).collect();
        assert_eq!(sprite_by_age(&list, 0, 10)[0], 0.0);
        assert_eq!(sprite_by_age(&list, 10, 10)[0], 7.0);
        assert_eq!(sprite_by_age(&list, 5, 10)[0], 3.0);
    }

    // Eine Welt mit einer fuenf Bloecke hohen Blasensaeule bei x=8, z=8 (y 0..=4), darueber
    // Luft. Die Saeule hat keine Kollision.
    fn welt_mit_saeule(drag: bool) -> (world::World, world::BlockShapes, u32) {
        let mut w = world::World::new();
        for cx in -2..=2 {
            for cz in -2..=2 {
                let sections = (0..24).map(|_| world::ChunkSection::from_states(vec![0u32; 4096])).collect();
                w.insert_column(cx, cz, sections);
            }
        }
        let wert = if drag { "true" } else { "false" };
        let saeule = (0..world::state_count() as u32)
            .find(|&s| world::block_name(s) == Some("bubble_column") && world::state_props::property(s, "drag") == Some(wert))
            .expect("bubble_column");
        for y in 0..=4 {
            w.set_block(8, y, 8, saeule);
        }
        let mut per_state = vec![None; world::state_count()];
        per_state[saeule as usize] = Some(Vec::new());
        (w, world::BlockShapes::new(per_state), saeule)
    }

    fn partikel_mit_bildern() -> Particles {
        Particles {
            rng: JavaRandom::new(7),
            sprites: vec![vec![[0.0, 0.0, 1.0, 1.0]]; pp::PARTICLE_NAMES.len()],
            block_uvs: HashMap::default(),
            item_uvs: HashMap::default(),
            particles: VecDeque::new(),
            pending: Vec::new(),
            emitters: Vec::new(),
            bursts: Vec::new(),
            unsupported: HashMap::default(),
            new_unsupported: Vec::new(),
            tuning: [ParticleTuning::default(); KINDS.len()],
            credit: [0.0; KINDS.len()],
        }
    }

    fn blasen_laufen_lassen(drag: bool) -> Vec<Particle> {
        let (w, shapes, _) = welt_mit_saeule(drag);
        let ctx = SpawnContext { camera: DVec3::new(8.5, 2.0, 8.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let mut p = partikel_mit_bildern();
        let mut gesehen = Vec::new();
        for _ in 0..200 {
            p.animate_blocks((8, 2, 8), &ctx);
            p.tick(&|_| None, &ctx);
            for t in &p.particles {
                assert!(in_water(&w, t.pos), "Blase ausserhalb des Wassers: {:?}", t.pos);
            }
            gesehen.extend(p.particles.iter().filter(|t| t.age > 5).cloned());
        }
        assert!(!gesehen.is_empty(), "die Saeule hat keine Blasen gemacht");
        gesehen
    }

    // Zerplatzender Trank: hundert Effekt-Partikel in der Trankfarbe (die Splitter brauchen ein
    // Bild im Atlas, das hat der Test nicht), alle steigen zunaechst.
    #[test]
    fn trank_zerplatzt_in_seiner_farbe() {
        let (w, shapes, _) = welt_mit_saeule(false);
        let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let mut p = partikel_mit_bildern();
        p.add_potion_splash((0, 5, 0), 0xFF0000, false, &ctx);
        assert_eq!(p.pending.len(), 100);
        assert!(p.pending.iter().all(|t| t.color[0] >= 0.74 && t.color[1] == 0.0 && t.color[2] == 0.0 && t.color[3] == 1.0));
        // Eigene Art, getrennt von den Wirbeln um Wesen.
        assert!(p.pending.iter().all(|t| KINDS[t.kind as usize].key == "potion_splash"));
    }

    // Menge 50 %: von hundert Spritzern bleibt genau die Haelfte; 200 % verdoppelt.
    #[test]
    fn menge_skaliert_den_schwall() {
        let (w, shapes, _) = welt_mit_saeule(false);
        let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let spritzer = KINDS.iter().position(|k| k.key == "potion_splash").unwrap();
        for (menge, erwartet) in [(50, 50), (200, 200)] {
            let mut p = partikel_mit_bildern();
            let mut tuning = [ParticleTuning::default(); KINDS.len()];
            tuning[spritzer].amount_pct = menge;
            p.set_tuning(tuning);
            p.add_potion_splash((0, 5, 0), 0xFF0000, false, &ctx);
            assert_eq!(p.pending.len(), erwartet, "Menge {menge} %");
        }
    }

    // Die Arten stehen alphabetisch, und jede hat genau die Regler, die zu ihr passen.
    #[test]
    fn arten_alphabetisch_und_spritzer_getrennt() {
        let falten = |s: &str| s.to_lowercase().replace('ä', "a").replace('ö', "o").replace('ü', "u");
        assert!(KINDS.windows(2).all(|w| falten(w[0].label) < falten(w[1].label)), "nicht alphabetisch");
        assert_ne!(kind_of(pp::PARTICLE_EFFECT), kind_of(pp::PARTICLE_ENTITY_EFFECT));
        assert!(KINDS[kind_of(pp::PARTICLE_ENTITY_EFFECT).unwrap()].spawn.has_interval());
        assert!(KINDS[kind_of(pp::PARTICLE_TOTEM_OF_UNDYING).unwrap()].spawn.has_all_at_once());
    }

    // Kruemel vor dem Gesicht: mit Bild fuenf Splitter, alle vor den Augen in Blickrichtung
    // (Blick nach Sueden = +z) und unterhalb davon. Ohne Bild (Pack hat "particle" geleert)
    // keiner.
    #[test]
    fn kruemel_vor_dem_gesicht() {
        let (w, shapes, _) = welt_mit_saeule(false);
        let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let mut p = partikel_mit_bildern();
        let auge = DVec3::new(0.5, 7.0, 0.5);
        p.add_item_use_particles("bread", 5, auge, 0.0, 0.0, &ctx);
        assert!(p.pending.is_empty());
        p.item_uvs.insert("bread".into(), [0.0, 0.0, 0.5, 0.5]);
        p.add_item_use_particles("bread", 5, auge, 0.0, 0.0, &ctx);
        assert_eq!(p.pending.len(), 5);
        for t in &p.pending {
            assert!((t.pos.z - auge.z - 0.6).abs() < 1e-6, "nicht vor dem Gesicht: {:?}", t.pos);
            assert!(t.pos.y < auge.y - 0.29 && t.pos.y > auge.y - 0.91);
            assert_eq!(KINDS[t.kind as usize].key, "item_shards");
        }
    }

    // Trankwolke: solange sie wartet, hoechstens zwei Wirbel nahe der Mitte; danach so viele,
    // wie die Flaeche gross ist, alle im Kreis und alle unter "Trankeffekte".
    #[test]
    fn trankwolke_fuellt_ihren_kreis() {
        let (w, shapes, _) = welt_mit_saeule(false);
        let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let mut p = partikel_mit_bildern();
        let mitte = DVec3::new(0.5, 5.0, 0.5);
        let gruen = ParticleEffect { type_id: pp::PARTICLE_ENTITY_EFFECT, options: ParticleOptions::Tinted(0xFF00_FF00) };
        for _ in 0..10 {
            p.tick_area_cloud(mitte, 3.0, true, &gruen, &ctx);
        }
        assert!(p.pending.len() <= 20);
        assert!(p.pending.iter().all(|t| t.pos.distance(mitte) < 0.5));
        p.pending.clear();
        p.tick_area_cloud(mitte, 3.0, false, &gruen, &ctx);
        assert_eq!(p.pending.len(), 29);
        assert!(p.pending.iter().all(|t| (t.pos - mitte).length() < 3.1 && t.color[1] == 1.0));
        assert!(p.pending.iter().all(|t| KINDS[t.kind as usize].key == "potion"));
    }

    // Effekt-Wirbel behalten die Deckkraft, mit der sie geschickt wurden: ein Ambient-Effekt
    // (Deckkraft 38) bleibt fast durchsichtig, ein normaler voll deckend.
    #[test]
    fn effekt_wirbel_behalten_ihre_deckkraft() {
        let (w, shapes, _) = welt_mit_saeule(false);
        let ctx = SpawnContext { camera: DVec3::new(0.5, 6.0, 0.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let mut p = partikel_mit_bildern();
        let ort = DVec3::new(0.5, 5.0, 0.5);
        // Blindheit, einmal als Ambient-Effekt, einmal normal.
        p.add_potion_swirl(0x261F_1F23, ort, &ctx);
        p.add_potion_swirl(0xFF1F_1F23, ort, &ctx);
        assert_eq!(p.pending.len(), 2);
        let (mut spawns, mut boeen) = (Vec::new(), Vec::new());
        for teil in p.pending.iter_mut() {
            teil.max_age = 1000;
            for _ in 0..60 {
                Particles::tick_one(teil, &[], &ctx, &mut spawns, &mut boeen);
            }
        }
        assert!((p.pending[0].color[3] - 38.0 / 255.0).abs() < 1e-3, "Ambient: {}", p.pending[0].color[3]);
        assert!((p.pending[1].color[3] - 1.0).abs() < 1e-3);
    }

    // Ein gelandeter Partikel faellt weiter, sobald der Block unter ihm weg ist (in Vanilla
    // bliebe er in der Luft stehen, s. Particle::move_by).
    #[test]
    fn gelandeter_partikel_faellt_weiter_wenn_der_boden_fehlt() {
        let (mut w, _, _) = welt_mit_saeule(false);
        let stein = (0..world::state_count() as u32).find(|&s| world::block_name(s) == Some("stone")).expect("stone");
        w.set_block(2, 5, 2, stein);
        let shapes = world::BlockShapes::new(vec![None; world::state_count()]);
        let mut p = Particle::base(&mut JavaRandom::new(1), DVec3::new(2.5, 6.5, 2.5));
        p.max_age = 1000;
        p.gravity = 1.0;
        for _ in 0..40 {
            p.tick_physics(&w, &shapes);
        }
        assert!(p.stopped && (p.pos.y - 6.0).abs() < 1e-3, "nicht gelandet: {:?}", p.pos);
        w.set_block(2, 5, 2, 0);
        for _ in 0..10 {
            p.tick_physics(&w, &shapes);
        }
        assert!(p.pos.y < 5.9, "bleibt in der Luft stehen: {:?}", p.pos);
    }

    #[test]
    // Ueber Seelensand steigen die Blasen und verschwinden an der Oberflaeche.
    fn blasensaeule_steigt_und_bleibt_im_wasser() {
        let blasen = blasen_laufen_lassen(false);
        assert!(blasen.iter().all(|b| b.vel.y > 0.0), "aufsteigende Saeule hat sinkende Blasen");
    }

    #[test]
    // Ueber Magma sinken sie im Sog.
    fn magmasaeule_zieht_nach_unten() {
        let blasen = blasen_laufen_lassen(true);
        assert!(blasen.iter().all(|b| b.vel.y < 0.0 && matches!(b.behavior, Behavior::CurrentDown { .. })));
    }

    #[test]
    // Krit-Partikel: Geschwindigkeit fliegt gegen Luftwiderstand 0.7 aus, gruen und blau
    // verblassen -- ein Krit wird zum Ende hin rot.
    fn krit_wird_rot() {
        let mut p = Particle::base(&mut JavaRandom::new(1), DVec3::ZERO);
        p.color = [0.8, 0.8, 0.8, 1.0];
        p.max_age = 100;
        p.fades_to_red = true;
        for _ in 0..10 {
            Particles::tick_one_without_world(&mut p);
        }
        assert!(p.color[0] > p.color[1] && p.color[1] > p.color[2]);
    }

    #[test]
    fn arten_stimmen_mit_ihren_konstanten() {
        assert_eq!(KINDS[KIND_EXPLOSION].key, "explosion");
        assert_eq!(KINDS[KIND_BLOCK].key, "block");
        assert_eq!(kind_of(pp::PARTICLE_EXPLOSION_EMITTER), Some(KIND_EXPLOSION));
        let mut keys: Vec<_> = KINDS.iter().map(|k| k.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), KINDS.len(), "Schluessel doppelt");
    }

    fn krit_paket() -> pp::LevelParticles {
        pp::LevelParticles {
            particle: ParticleEffect { type_id: pp::PARTICLE_CRIT, options: ParticleOptions::None },
            force_spawn: true,
            important: false,
            pos: (8.5, 10.0, 8.5),
            offset: (0.0, 0.0, 0.0),
            speed: 0.0,
            count: 0,
        }
    }

    #[test]
    // Aus: nichts entsteht, und was schon fliegt, verschwindet mit dem Ausschalten.
    fn ausgeschaltete_art_verschwindet() {
        let (w, shapes, _) = welt_mit_saeule(false);
        let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let mut p = partikel_mit_bildern();
        p.on_packet(&krit_paket(), &ctx);
        p.tick(&|_| None, &ctx);
        assert_eq!(p.particles.len(), 1);
        let mut aus = [ParticleTuning::default(); KINDS.len()];
        aus[kind_of(pp::PARTICLE_CRIT).unwrap()].enabled = false;
        p.set_tuning(aus);
        assert!(p.particles.is_empty());
        p.on_packet(&krit_paket(), &ctx);
        p.tick(&|_| None, &ctx);
        assert!(p.particles.is_empty());
    }

    #[test]
    // Doppelte Dauer: doppelte Lebensdauer. Halbes Tempo: halb so viele eigene Ticks, und das
    // Bild gleitet dazwischen ohne Sprung weiter.
    fn dauer_und_tempo() {
        let (w, shapes, _) = welt_mit_saeule(false);
        let ctx = SpawnContext { camera: DVec3::new(8.5, 10.0, 8.5), mode: ParticleMode::All, world: &w, shapes: &shapes, biomes: &world::BiomePalette::fallback(), player: None };
        let mut normal = partikel_mit_bildern();
        normal.on_packet(&krit_paket(), &ctx);
        let vanilla = normal.pending[0].max_age;

        let mut p = partikel_mit_bildern();
        let mut t = [ParticleTuning::default(); KINDS.len()];
        let krit = kind_of(pp::PARTICLE_CRIT).unwrap();
        t[krit].lifetime_pct = 200;
        t[krit].speed_pct = 50;
        p.set_tuning(t);
        p.on_packet(&krit_paket(), &ctx);
        assert_eq!(p.pending[0].max_age, vanilla * 2);
        p.tick(&|_| None, &ctx);
        let alter = p.particles[0].age;
        let mut vorher: Option<Vec3> = None;
        for _ in 0..6 {
            for schritt in 0..=4 {
                let mut out = Vec::new();
                p.build_vertices(DVec3::ZERO, Vec3::X, Vec3::Y, schritt as f32 / 4.0, &mut out);
                let mitte = out.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / out.len() as f32;
                if let Some(v) = vorher {
                    assert!((mitte - v).length() < 0.2, "Sprung im Bild: {v} -> {mitte}");
                }
                vorher = Some(mitte);
            }
            p.tick(&|_| None, &ctx);
        }
        assert_eq!(p.particles[0].age - alter, 3, "6 Spielticks bei 50 % = 3 eigene");
    }
}
