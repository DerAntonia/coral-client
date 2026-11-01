// Partikel ticken, Explosionen und Vertices bauen.

use super::*;

impl Particles {
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
        for p in &mut self.particles {
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
    pub(super) fn spawn_emitted(&mut self, spawns: Vec<(DVec3, f64)>, boeen: Vec<(DVec3, f64)>, ctx: &SpawnContext) {
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
    pub(super) fn tick_one(p: &mut Particle, sprites: &[Vec<[f32; 4]>], ctx: &SpawnContext, spawns: &mut Vec<(DVec3, f64)>, boeen: &mut Vec<(DVec3, f64)>) {
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
                    p.vel.x = (p.vel.x + f64::from(0.6 * angle.cos())) * 0.07;
                    p.vel.z = (p.vel.z + f64::from(0.6 * angle.sin())) * 0.07;
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
                    spawns.push((p.pos, f64::from(p.age) / f64::from(p.max_age)));
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
    pub(super) fn tick_explosion_bursts(&mut self, ctx: &SpawnContext) {
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
    pub(super) fn explosion_block_particle(&mut self, b: &ExplosionBurst, ctx: &SpawnContext) {
        let r = &mut self.rng;
        let dir = DVec3::new(
            f64::from(r.next_float() * 2.0 - 1.0),
            f64::from(r.next_float() * 2.0 - 1.0),
            f64::from(r.next_float() * 2.0 - 1.0),
        )
        .normalize_or_zero();
        let f = f64::from(r.next_float().cbrt() * b.radius);
        let offset = dir * f;
        let p = b.center + offset;
        if ctx.world.block_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) != world::STATE_AIR {
            return;
        }
        let g = 0.5 / (f / f64::from(b.radius) + 0.1) * f64::from(r.next_float()) * f64::from(r.next_float()) + 0.3;
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
        let pos = b.center + offset * f64::from(bp.scaling);
        let vel = dir * (g * f64::from(bp.speed));
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
                let center = (p.last.lerp(p.pos, f64::from(progress)) - eye).as_vec3();
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
    pub(super) fn sheet_of(&self, p: &Particle) -> Sheet {
        if p.sheet == Sheet::Opaque && self.tuning[p.kind as usize].opacity_pct < 100 {
            Sheet::Translucent
        } else {
            p.sheet
        }
    }
}
