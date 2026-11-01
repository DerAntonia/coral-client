// Partikel nach Art erzeugen.

use super::*;

impl Particles {
    // Die Fabriken aus ParticleSpriteManager.registerDefaultFactories -- nur die umgesetzten.
    pub(super) fn create(&mut self, effect: &ParticleEffect, pos: DVec3, v: DVec3, ctx: &SpawnContext) -> Option<Particle> {
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
                    let power = f64::from(power);
                    p.vel.x *= power;
                    p.vel.y = (p.vel.y - f64::from(0.1f32)) * power + f64::from(0.1f32);
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
    pub(super) fn damage(&mut self, t: i32, pos: DVec3, v: DVec3) -> Option<Particle> {
        let uv = self.random_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, uv);
        p.velocity_multiplier = 0.7;
        p.gravity = 0.5;
        p.vel *= f64::from(0.1f32);
        p.vel += v * 0.4;
        let j = rng.next_float() * 0.3 + 0.6;
        p.color = [j, j, j, 1.0];
        p.scale *= 0.75;
        p.max_age = ((6.0 / (f64::from(rng.next_float()) * 0.8 + 0.6)) as i32).max(1);
        p.collides = false;
        p.grows_in = true;
        p.fades_to_red = true;
        p.sheet = Sheet::Opaque;
        // Der Konstruktor ruft tick() selbst auf -- ohne Welt, weil collides schon aus ist.
        Self::tick_one_without_world(&mut p);
        Some(p)
    }

    // Ein Particle.tick ohne Kollision (DamageParticle-Konstruktor).
    pub(super) fn tick_one_without_world(p: &mut Particle) {
        p.last = p.pos;
        let age = p.age;
        p.age += 1;
        if age >= p.max_age {
            p.dead = true;
        } else {
            p.vel.y -= 0.04 * f64::from(p.gravity);
            p.pos += p.vel;
            p.vel *= f64::from(p.velocity_multiplier);
        }
        if p.fades_to_red {
            p.color[1] *= 0.96;
            p.color[2] *= 0.9;
        }
    }

    // TotemParticle (AnimatedParticle mit Auftrieb 1.25).
    pub(super) fn totem(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
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
    pub(super) fn frozen(
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
    pub(super) fn gust(&mut self, t: i32, pos: DVec3, scale: f32) -> Option<Particle> {
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
    pub(super) fn soul(&mut self, t: i32, pos: DVec3, v: DVec3) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, v).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.vel = p.vel * f64::from(0.01f32) + v;
        let streuen = |rng: &mut JavaRandom| f64::from((rng.next_float() - rng.next_float()) * 0.05);
        p.pos += DVec3::new(streuen(rng), streuen(rng), streuen(rng));
        p.last = p.pos;
        p.max_age = (8.0 / (f64::from(rng.next_float()) * 0.8 + 0.2)) as i32 + 4;
        p = p.scaled(1.5);
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Translucent;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // FireworkParticles.OverlayParticle (flash): steht still, vier Ticks, in der Farbe aus dem
    // Paket (s. size/build_vertices fuer Groesse und Deckkraft).
    pub(super) fn flash(&mut self, pos: DVec3, argb: u32) -> Option<Particle> {
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
    pub(super) fn flame(&mut self, t: i32, pos: DVec3, v: DVec3, scale: f32) -> Option<Particle> {
        let uv = self.random_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, v).billboard(rng, uv);
        p.velocity_multiplier = 0.96;
        p.vel = p.vel * f64::from(0.01f32) + v;
        let streuen = |rng: &mut JavaRandom| f64::from((rng.next_float() - rng.next_float()) * 0.05);
        p.pos += DVec3::new(streuen(rng), streuen(rng), streuen(rng));
        p.last = p.pos;
        p.max_age = (8.0 / (f64::from(rng.next_float()) * 0.8 + 0.2)) as i32 + 4;
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
    pub(super) fn end_rod(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
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
    pub(super) fn cloud(&mut self, t: i32, pos: DVec3, v: DVec3) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.vel = p.vel * f64::from(0.1f32) + v;
        let f = 1.0 - rng.next_float() * 0.3;
        p.color = [f, f, f, 1.0];
        p.scale *= 1.875;
        let grund = (8.0 / (f64::from(rng.next_float()) * 0.8 + 0.3)) as i32;
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
    pub(super) fn poof(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
        let t = pp::PARTICLE_POOF;
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, first);
        p.gravity = -0.1;
        p.velocity_multiplier = 0.9;
        let j = |c: f64, rng: &mut JavaRandom| c + f64::from((rng.next_float() * 2.0 - 1.0) * 0.05);
        p.vel = DVec3::new(j(v.x, rng), j(v.y, rng), j(v.z, rng));
        let f = rng.next_float() * 0.3 + 0.7;
        p.color = [f, f, f, 1.0];
        p.scale = 0.1 * (rng.next_float() * rng.next_float() * 6.0 + 1.0);
        p.max_age = (16.0 / (f64::from(rng.next_float()) * 0.8 + 0.2)) as i32 + 2;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Opaque;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // BubbleColumnUpParticle: kleine Blase, die mit Auftrieb 0.125 steigt, bis sie das Wasser
    // verlaesst. Die mitgegebene Geschwindigkeit zaehlt nur zu einem Fuenftel.
    pub(super) fn bubble_column_up(&mut self, pos: DVec3, v: DVec3) -> Option<Particle> {
        let uv = self.random_sprite(pp::PARTICLE_BUBBLE_COLUMN_UP)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, uv);
        p.gravity = -0.125;
        p.velocity_multiplier = 0.85;
        p.set_spacing(f64::from(0.02f32), f64::from(0.02f32));
        p.scale *= rng.next_float() * 0.6 + 0.2;
        let j = |c: f64, rng: &mut JavaRandom| c * f64::from(0.2f32) + f64::from((rng.next_float() * 2.0 - 1.0) * 0.02);
        p.vel = DVec3::new(j(v.x, rng), j(v.y, rng), j(v.z, rng));
        p.max_age = (40.0 / (f64::from(rng.next_float()) * 0.8 + 0.2)) as i32;
        p.needs_water = true;
        p.sheet = Sheet::Opaque;
        Some(p)
    }

    // CurrentDownParticle: Blase im Sog einer Magma-Saeule, 30..89 Ticks, ohne Kollision.
    pub(super) fn current_down(&mut self, pos: DVec3) -> Option<Particle> {
        let uv = self.random_sprite(pp::PARTICLE_CURRENT_DOWN)?;
        let rng = &mut self.rng;
        let mut p = Particle::base(rng, pos).billboard(rng, uv);
        p.max_age = (rng.next_float() * 60.0) as i32 + 30;
        p.collides = false;
        p.vel = DVec3::new(0.0, -0.05, 0.0);
        p.set_spacing(f64::from(0.02f32), f64::from(0.02f32));
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
    pub(super) fn spell(&mut self, t: i32, pos: DVec3, v: DVec3, argb: u32) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let (vx, vz) = (0.5 - rng.next_double(), 0.5 - rng.next_double());
        let mut p = Particle::with_velocity(rng, pos, DVec3::new(vx, v.y, vz)).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.gravity = -0.1;
        p.ascending = true;
        p.vel.y *= f64::from(0.2f32);
        if v.x == 0.0 && v.z == 0.0 {
            p.vel.x *= f64::from(0.1f32);
            p.vel.z *= f64::from(0.1f32);
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
}
