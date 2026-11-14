// Partikel aus der Welt, Traenke, Items, Pakete, Explosionen, Emitter und Bloecke.

use super::*;

// Wo ein Emitter-Partikel entsteht: auf dem Rand der Hitbox, dort wo der Strahl aus der
// Koerpermitte in Flugrichtung `v` sie verlaesst -- es ist sofort draussen (Nutzer 2026-11-12;
// Vanilla setzt es in die Mitte +- ein Viertel der Breite und Hoehe).
pub(super) fn body_edge(b: EmitterBody, v: DVec3) -> DVec3 {
    let mitte = DVec3::new(b.pos.x, b.pos.y + b.height * 0.5, b.pos.z);
    let halb = DVec3::new(b.width * 0.5, b.height * 0.5, b.width * 0.5);
    let t = (halb.x / v.x.abs()).min(halb.y / v.y.abs()).min(halb.z / v.z.abs());
    if t.is_finite() { mitte + v * t } else { mitte }
}

impl Particles {
    // Ein Trank-Wirbel an einem Entity (LivingEntity.tickStatusEffects): Vanilla gibt als
    // "Geschwindigkeit" immer (1, 1, 1) mit.
    // Level.addParticle aus einem Entity-Tick (force = false, important = false).
    pub fn add_world_particle(&mut self, effect: &ParticleEffect, pos: DVec3, vel: DVec3, ctx: &SpawnContext) {
        self.add_client(effect, false, false, pos, vel, ctx);
    }

    // Eine Spur: `anzahl` Partikel (Vanillas Zahl) gleichmaessig auf der Strecke von `von` bis
    // kurz vor `von + weg`. Mehr Menge als Vanilla verteilt die Spur selbst auf die Strecke --
    // Kopien in push laegen als Haeufchen aufeinander (Nutzer 2026-11-12, wie tick_emitter).
    pub fn add_trail(&mut self, effect: &ParticleEffect, von: DVec3, weg: DVec3, vel: DVec3, anzahl: u32, ctx: &SpawnContext) {
        let n = self.trail_count(effect, von, anzahl, ctx);
        self.emit_scaled = n > anzahl;
        for i in 0..n {
            self.add_client(effect, false, false, von + weg * (f64::from(i) / f64::from(n)), vel, ctx);
        }
        self.emit_scaled = false;
    }

    // Wie viele Partikel add_trail fuer `anzahl` setzt: bei mehr Menge entsprechend mehr, bei
    // weniger bleibt es bei `anzahl`, und push laesst welche weg.
    pub(super) fn trail_count(&self, effect: &ParticleEffect, von: DVec3, anzahl: u32, ctx: &SpawnContext) -> u32 {
        let faktor = kind_of(effect.type_id).map_or(1.0, |k| self.tuning_of(own_at(ctx, k, von), k).spawn_factor(KINDS[k].spawn));
        if faktor > 1.0 { (anzahl as f32 * faktor).round() as u32 } else { anzahl }
    }

    pub fn add_potion_swirl(&mut self, argb: u32, pos: DVec3, ctx: &SpawnContext) {
        let effect = ParticleEffect { type_id: pp::PARTICLE_ENTITY_EFFECT, options: ParticleOptions::Tinted(argb) };
        self.add_client(&effect, false, false, pos, DVec3::ONE, ctx);
    }

    // FireSmokeParticle / LargeFireSmokeParticle (AscendingParticle mit 0.1er Rauschen,
    // Farbfaktor 0.3, Grundlebensdauer 8, Auftrieb -0.1).
    pub(super) fn smoke(&mut self, t: i32, pos: DVec3, v: DVec3, scale_mul: f32) -> Option<Particle> {
        let first = self.first_sprite(t)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.gravity = -0.1;
        p.ascending = true;
        p.vel *= f64::from(0.1f32);
        p.vel += v;
        let f = rng.next_float() * 0.3;
        p.color = [f, f, f, 1.0];
        p.scale *= 0.75 * scale_mul;
        p.max_age = ((8.0 / (f64::from(rng.next_float()) * 0.8 + 0.2) * f64::from(scale_mul)) as i32).max(1);
        p.grows_in = true;
        p.animated_sprites = Some(t);
        p.sheet = Sheet::Opaque;
        p.uv = sprite_by_age(self.sprite_list(t), p.age, p.max_age);
        Some(p)
    }

    // RedDustParticle (AbstractDustParticle). DustParticleEffect klemmt die Groesse auf 0.01..4.
    pub(super) fn dust(&mut self, pos: DVec3, v: DVec3, color: u32, scale: f32) -> Option<Particle> {
        let t = pp::PARTICLE_DUST;
        let first = self.first_sprite(t)?;
        let scale = scale.clamp(0.01, 4.0);
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, v).billboard(rng, first);
        p.velocity_multiplier = 0.96;
        p.ascending = true;
        p.vel *= f64::from(0.1f32);
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
    pub(super) fn item_shard(&mut self, pos: DVec3, v: DVec3, name: &str) -> Option<Particle> {
        let sprite = *self.item_uvs.get(name)?;
        let rng = &mut self.rng;
        let mut p = Particle::with_velocity(rng, pos, DVec3::ZERO).billboard(rng, sprite);
        p.gravity = 1.0;
        p.scale /= 2.0;
        let su = rng.next_float() * 3.0;
        let sv = rng.next_float() * 3.0;
        p.vel = p.vel * f64::from(0.1f32) + v;
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
        let mitte = DVec3::new(f64::from(pos.0) + 0.5, f64::from(pos.1), f64::from(pos.2) + 0.5);
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
            let v = DVec3::new((f64::from(r.next_float()) - 0.5) * 0.1, f64::from(r.next_float()) * 0.1 + 0.1, 0.0);
            let v = rot_y(rot_x(v, xr), yr);
            let tiefe = -f64::from(r.next_float()) * 0.6 - 0.3;
            let p = DVec3::new((f64::from(r.next_float()) - 0.5) * 0.3, tiefe, 0.6);
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
                DVec3::new((f64::from(r.next_float()) - 0.5) * 0.08, (f64::from(r.next_float()) - 0.5) * 0.08, (f64::from(r.next_float()) - 0.5) * 0.08)
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
            let ort = DVec3::new(pos.x + f64::from(winkel.cos() * abstand), pos.y, pos.z + f64::from(winkel.sin() * abstand));
            if particle.type_id == pp::PARTICLE_ENTITY_EFFECT {
                let e = if waiting && self.rng.next_int(2) == 0 { &weiss } else { particle };
                self.add_client(e, false, true, ort, DVec3::ZERO, ctx);
            } else if waiting {
                self.add_client(particle, false, true, ort, DVec3::ZERO, ctx);
            } else {
                let vx = (0.5 - self.rng.next_double()) * 0.15;
                let vz = (0.5 - self.rng.next_double()) * 0.15;
                self.add_client(particle, false, true, ort, DVec3::new(vx, f64::from(0.01f32), vz), ctx);
            }
        }
    }

    // BlockDustParticle: ein zufaelliges Viertel der Partikeltextur des Blocks, grau
    // abgedunkelt und mit der Biomfarbe des Blocks gefaerbt wie im Gelaende (Gras-Pflanzen,
    // Laub). Der Grasblock bleibt ungefaerbt: seine Partikeltextur ist die Erdseite.
    pub(super) fn block_dust(&mut self, pos: DVec3, v: DVec3, state: u32, block_pos: (i32, i32, i32), ctx: &SpawnContext) -> Option<Particle> {
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
        let (ox, oy, oz) = (f64::from(pk.offset.0), f64::from(pk.offset.1), f64::from(pk.offset.2));
        let speed = f64::from(pk.speed);
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
        // Erst die schon fliegenden Partikel wegstossen (kein Vanilla), dann die neuen der
        // Explosion.
        let mut stoss = ExplosionPush::new(center, e.radius, ctx.world, ctx.shapes);
        self.push_and_step(&mut stoss, 0, true, ctx);
        // Emitter aus demselben Tick (Totem-Pop durch diese Explosion): auch ihre spaeteren
        // Partikel bekommen den Stoss (s. Emitter::explosion).
        for em in self.emitters.iter_mut().filter(|em| em.fresh) {
            em.explosion = Some((center, e.radius));
        }
        self.add_client(&e.particle, false, false, center, DVec3::new(1.0, 0.0, 0.0), ctx);
        let own = own_at(ctx, KIND_EXPLOSION, center);
        if self.tuning_of(own, KIND_EXPLOSION).enabled && !e.block_particles.is_empty() {
            self.bursts.push(ExplosionBurst {
                own,
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
        // Die eigene Figur liefert `body` mit genau der Lage von ctx.player (s. App::with_particles),
        // jede andere Entity hat ihre eigene.
        let own = body(entity).is_some_and(|b| ctx.player.is_some_and(|me| me.pos == b.pos));
        let mut em = Emitter { entity, effect: ParticleEffect { type_id, options: ParticleOptions::None }, age: 0, max_age, fresh: true, explosion: None, own };
        // "Alle auf einmal": alle Ticks des Emitters jetzt (das Totem etwa sonst 30 Ticks lang).
        let sofort = kind_of(type_id).is_some_and(|k| self.tuning_of(own, k).all_at_once && KINDS[k].spawn.has_all_at_once());
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

    // Explosions-Rueckstoss auf die Partikel ab `pending[ab..]` (mit `bestand` auch auf alle
    // schon fliegenden). Wer gestossen wird, macht sofort seinen ersten Schritt -- unabhaengig
    // vom Tempo, ein ganzer Tick (Nutzer 2026-11-11): sonst stand er bis zum naechsten bzw.
    // uebernaechsten Spieltick still (neue Partikel kommen erst am Ende eines Ticks in den
    // Bestand), und die Wolke flog spuerbar nach dem Knall los. Er ist damit einen Tick voraus.
    pub(super) fn push_and_step(&mut self, stoss: &mut ExplosionPush, ab: usize, bestand: bool, ctx: &SpawnContext) {
        let (mut spawns, mut boeen) = (Vec::new(), Vec::new());
        let sprites = &self.sprites;
        let fliegende = self.particles.iter_mut().filter(|_| bestand);
        for p in fliegende.chain(self.pending[ab..].iter_mut()) {
            if p.explosion_knockback(stoss) {
                Self::tick_one(p, sprites, ctx, &mut spawns, &mut boeen);
            }
        }
        self.spawn_emitted(spawns, boeen, ctx);
    }

    // EmitterParticle.tick. false = fertig.
    pub(super) fn tick_emitter(&mut self, em: &mut Emitter, body: &dyn Fn(i32) -> Option<EmitterBody>, ctx: &SpawnContext) -> bool {
        let Some(b) = body(em.entity) else { return false };
        let vorher = self.pending.len();
        // Mehr Menge als Vanilla: der Emitter wuerfelt entsprechend oefter, jedes Partikel mit
        // eigener Richtung -- Kopien in push laegen als Haeufchen beieinander (Nutzer 2026-11-11).
        let faktor = kind_of(em.effect.type_id).map_or(1.0, |k| self.tuning_of(em.own, k).spawn_factor(KINDS[k].spawn));
        let versuche = if faktor > 1.0 { (16.0 * faktor).round() as usize } else { 16 };
        self.emit_own = Some(em.own);
        self.emit_scaled = faktor > 1.0;
        for _ in 0..versuche {
            let d = f64::from(self.rng.next_float() * 2.0 - 1.0);
            let e = f64::from(self.rng.next_float() * 2.0 - 1.0);
            let f = f64::from(self.rng.next_float() * 2.0 - 1.0);
            if d * d + e * e + f * f > 1.0 {
                continue;
            }
            let v = DVec3::new(d, e + 0.2, f);
            let effect = em.effect.clone();
            self.add_client(&effect, false, false, body_edge(b, v), v, ctx);
        }
        self.emit_own = None;
        self.emit_scaled = false;
        if let Some((mitte, radius)) = em.explosion {
            let mut stoss = ExplosionPush::new(mitte, radius, ctx.world, ctx.shapes);
            self.push_and_step(&mut stoss, vorher, false, ctx);
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
        let origin = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
        for b in world::outline_boxes(state, shapes) {
            let min = DVec3::new(f64::from(b.min[0]), f64::from(b.min[1]), f64::from(b.min[2]));
            let size = DVec3::new(f64::from(b.max[0]), f64::from(b.max[1]), f64::from(b.max[2])) - min;
            let size = size.min(DVec3::ONE);
            let steps = |s: f64| ((s / 0.25).ceil() as i32).max(2);
            let (ni, nj, nk) = (steps(size.x), steps(size.y), steps(size.z));
            for l in 0..ni {
                for m in 0..nj {
                    for n in 0..nk {
                        let g = (f64::from(l) + 0.5) / f64::from(ni);
                        let h = (f64::from(m) + 0.5) / f64::from(nj);
                        let o = (f64::from(n) + 0.5) / f64::from(nk);
                        let p = origin + DVec3::new(g * size.x + min.x, h * size.y + min.y, o * size.z + min.z);
                        if let Some(p) = self.block_dust(p, DVec3::new(g - 0.5, h - 0.5, o - 0.5), state, pos, ctx) {
                            self.push(p, KIND_BLOCK, ctx);
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
        let o = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
        let r = &mut self.rng;
        let inset = f64::from(0.1f32);
        let mut x = o.x + r.next_double() * (max.x - min.x - f64::from(0.2f32)) + inset + min.x;
        let mut y = o.y + r.next_double() * (max.y - min.y - f64::from(0.2f32)) + inset + min.y;
        let mut z = o.z + r.next_double() * (max.z - min.z - f64::from(0.2f32)) + inset + min.z;
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
            self.push(p.speed(0.2).scaled(0.6), KIND_BLOCK, ctx);
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

    pub(super) fn animate_block(&mut self, c: (i32, i32, i32), radius: i32, ctx: &SpawnContext, sounds: &mut Vec<AmbientSound>) {
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
    pub(super) fn bubble_column_tick(&mut self, pos: (i32, i32, i32), drag: bool, ctx: &SpawnContext, sounds: &mut Vec<AmbientSound>) {
        let o = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
        let sound = if drag {
            let effect = ParticleEffect { type_id: pp::PARTICLE_CURRENT_DOWN, options: ParticleOptions::None };
            self.add_client(&effect, false, true, o + DVec3::new(0.5, 0.8, 0.0), DVec3::ZERO, ctx);
            "minecraft:block.bubble_column.whirlpool_ambient"
        } else {
            let effect = ParticleEffect { type_id: pp::PARTICLE_BUBBLE_COLUMN_UP, options: ParticleOptions::None };
            let up = DVec3::new(0.0, 0.04, 0.0);
            self.add_client(&effect, false, true, o + DVec3::new(0.5, 0.0, 0.5), up, ctx);
            let r = &mut self.rng;
            let streu = DVec3::new(f64::from(r.next_float()), f64::from(r.next_float()), f64::from(r.next_float()));
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
}
