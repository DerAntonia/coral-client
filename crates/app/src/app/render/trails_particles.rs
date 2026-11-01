// Spuren, Timer-Ring, Schild-Abklingzeit und Partikel.

use super::*;

impl App {
    // Einmal pro Tick: jede Flugbahn um die aktuelle Position ihres Pfeils verlaengern und
    // abgelaufene Punkte abschneiden. Jeder Punkt altert fuer sich (Tick seiner Aufnahme) --
    // die Spur wird also vom alten Ende her aufgeraeumt, als liefe ein zweiter Pfeil hinterher.
    // Ist der Pfeil einmal gelandet, kommt nie wieder etwas dazu, auch wenn er danach noch
    // einmal losfliegt (z. B. weil der Block unter ihm abgebaut wird).
    pub(crate) fn tick_trails(&mut self) {
        self.trail_tick += 1;
        if !(self.settings.module_arrows && self.settings.arrow_trail) {
            self.trails.clear();
            return;
        }
        let jetzt = self.trail_tick;
        let grenze = self.trail_lifetime_ticks();
        for t in self.trails.values_mut() {
            t.lebt = false;
        }
        for (id, e) in self.entities.iter() {
            if arrow_texture(e.entity_type).is_none() {
                continue;
            }
            let t = self.trails.entry(*id).or_default();
            t.lebt = true;
            if t.gelandet {
                continue;
            }
            // Mit dem Abschuss-Versatz: die Spur beginnt dort, wo man den Pfeil SIEHT
            // (s. world::Entity::launch_render_offset), nicht an seiner Rechenposition.
            let (x, y, z) = e.render_pos(1.0);
            let v = e.launch_render_offset(1.0);
            let pos = DVec3::new(x + f64::from(v[0]), y + f64::from(v[1]), z + f64::from(v[2]));
            if t.punkte.back().is_none_or(|(p, _)| p.distance_squared(pos) > 1e-6) {
                t.punkte.push_back((pos, jetzt));
            }
            // Ein Pfeil, der schon steckt, als man ihn zum ersten Mal sieht, bekommt so gar
            // keine Spur (ein einzelner Punkt zeichnet nichts).
            if e.in_ground() {
                t.gelandet = true;
            }
        }
        let alter = |tick: u64| jetzt.saturating_sub(tick) as f32;
        self.trails.retain(|_, t| {
            // Der vorderste Punkt bleibt, solange das Stueck zum naechsten noch sichtbar ist --
            // es wird beim Zeichnen genau an der Ablaufgrenze abgeschnitten (s. emit_trails).
            while t.punkte.len() >= 2 && alter(t.punkte[1].1) >= grenze {
                t.punkte.pop_front();
            }
            if t.punkte.len() == 1 && alter(t.punkte[0].1) >= grenze && (t.gelandet || !t.lebt) {
                t.punkte.pop_front();
            }
            t.lebt || !t.punkte.is_empty()
        });
    }

    // Wie lange ein Spurpunkt insgesamt lebt: Standzeit + Ausblenden, in Ticks.
    pub(crate) fn trail_lifetime_ticks(&self) -> f32 {
        (self.settings.trail_hold_tenths + self.settings.trail_fade_tenths) as f32 * 2.0
    }

    // Die Flugbahnen als Baender, die sich zur Kamera drehen. Ausgeblendet wird ueber die
    // Breite (die Entity-Pipeline mischt nicht): jeder Punkt steht erst die eingestellte Zeit
    // in voller Breite und laeuft dann ueber die Fade-Dauer auf null. Die weisse Atlas-Kachel
    // mit voller Vertexfarbe ergibt reines Weiss.
    pub(crate) fn emit_trails(&self, eye: DVec3, progress: f32, out: &mut Vec<world::BlockVertex>) {
        if self.trails.is_empty() {
            return;
        }
        let [u0, v0, u1, v1] = self.entity_atlas.white_rect();
        let uv = [f32::midpoint(u0, u1), f32::midpoint(v0, v1)];
        let halten = self.settings.trail_hold_tenths as f32 * 2.0;
        let fade = self.settings.trail_fade_tenths as f32 * 2.0;
        let grenze = halten + fade;
        let breite = |alter: f32| {
            let f = if alter <= halten {
                1.0
            } else if fade > 0.0 {
                1.0 - (alter - halten) / fade
            } else {
                0.0
            };
            TRAIL_WIDTH * 0.5 * f.clamp(0.0, 1.0)
        };
        let jetzt = self.trail_tick as f32 + progress;
        for (id, t) in &self.trails {
            let mut punkte: Vec<(Vec3, f32)> = t.punkte.iter().map(|(p, tick)| ((*p - eye).as_vec3(), jetzt - *tick as f32)).collect();
            if t.lebt && !t.gelandet {
                if let Some(e) = self.entities.get(*id) {
                    let (x, y, z) = e.render_pos(progress);
                    let v = e.launch_render_offset(progress);
                    punkte.push(((DVec3::new(x, y, z) - eye).as_vec3() + Vec3::from(v), 0.0));
                }
            }
            for paar in punkte.windows(2) {
                let ((mut a, mut alter_a), (b, alter_b)) = (paar[0], paar[1]);
                if alter_b >= grenze {
                    continue;
                }
                // Das aelteste Stueck genau an der Ablaufgrenze abschneiden: so wandert das
                // Ende gleichmaessig, statt Stueck fuer Stueck wegzuspringen.
                if alter_a > grenze {
                    let t = (alter_a - grenze) / (alter_a - alter_b);
                    a += (b - a) * t;
                    alter_a = grenze;
                }
                // Quer zur Flugrichtung UND zur Blickrichtung -- so zeigt das Band immer seine
                // Breitseite zur Kamera.
                let quer = (b - a).cross((a + b) * 0.5);
                if quer.length_squared() < 1e-12 {
                    continue;
                }
                let quer = quer.normalize();
                let (qa, qb) = (quer * breite(alter_a), quer * breite(alter_b));
                let ecke = |p: Vec3| world::BlockVertex { position: [p.x, p.y, p.z], color: [1.0; 3], uv };
                let (p1, p2, p3, p4) = (ecke(a - qa), ecke(a + qa), ecke(b + qb), ecke(b - qb));
                // Beide Wicklungen: die Pipeline verwirft Rueckseiten.
                out.extend_from_slice(&[p1, p2, p3, p1, p3, p4, p1, p3, p2, p1, p4, p3]);
            }
        }
    }

    // Der eigene Schild-Cooldown (Schild-Modul): Rest in Ticks und verbleibender Anteil 0..1,
    // oder None ohne Cooldown bzw. wenn die Anzeige gerade nicht passt.
    // Modul "Timer": Anteil 0..1 des Kreises. Beim Essen fuellt er sich bis "fertig". Bogen und
    // Armbrust laufen andersherum: der Kreis LEERT sich und ist weg, sobald der Bogen ganz
    // gespannt (kritischer Pfeil) bzw. die Armbrust geladen ist -- beim Zielen stoert dann
    // nichts mehr am Fadenkreuz (gewuenscht). None = kein Kreis.
    pub(crate) fn use_timer_ring(&self) -> Option<f32> {
        if !self.settings.module_use_timer || self.is_dead || self.inventory.screen_open || self.hud_hidden {
            return None;
        }
        let (name, stack) = self.own_used_item()?;
        let pull = stack.as_ref().map_or(25, |st| self.enchantments.crossbow_pull_ticks(st));
        let voll = consumable_optimizer::timer_ticks(name, stack.as_ref(), pull)?;
        let fortschritt = if voll == 0 { 1.0 } else { ((self.own_use_ticks as f32 + self.tick_progress()) / voll as f32).min(1.0) };
        if matches!(name, "bow" | "crossbow") {
            // 0 = gespannt: kein Kreis, aber auch kein Schild-Cooldown bis zum Loslassen.
            return Some(1.0 - fortschritt);
        }
        Some(fortschritt)
    }

    pub(crate) fn own_shield_cooldown(&self) -> Option<(f32, f32)> {
        if !(self.settings.module_shields && self.settings.shield_cd_self) || self.is_dead || self.inventory.screen_open || self.hud_hidden {
            return None;
        }
        let shield = protocol::item::ItemStack::new(world::item_registry::item_id_by_name("shield")?, 1);
        let rest = self.item_cooldowns.remaining_ticks(&shield, self.tick_progress());
        (rest > 0.0).then(|| (rest, self.item_cooldowns.progress(&shield, self.tick_progress())))
    }

    // Entity-Ereignis vom Server. Bisher interessiert nur eines davon: das gepoppte Totem am
    // EIGENEN Spieler. Fremde Pops (Status 35 an einer anderen EntityId) werden verworfen --
    // die Hand eines anderen Spielers koennte man ohnehin nicht bestimmen, dessen Haende
    // kennen wir nicht.
    // Die Partikel-Quads dieses Bildes in self.particle_vertices. Sie liegen in der Bildebene
    // der Kamera (Vanillas Rotator.ALL_AXIS: die Kameradrehung selbst).
    pub(crate) fn build_particle_vertices(&mut self) -> render::ParticleDraws {
        let forward = self.camera.view_forward();
        // Die Rechte aus dem Gierwinkel statt aus forward x Y: bei senkrechtem Blick waere das
        // Kreuzprodukt null. Die Frontansicht blickt um 180 Grad gedreht (s. view_forward).
        let right = self.camera.view_right();
        let up = right.cross(forward).normalize_or_zero();
        let tick_progress = self.tick_progress();
        let mut out = std::mem::take(&mut self.particle_vertices);
        self.particles.set_tuning(self.settings.particle_tuning);
        let mut draws = self.particles.build_vertices(self.camera.position, right, up, tick_progress, &mut out);
        // XP-Kugeln ans Ende der durchscheinenden Partikel (gleicher Atlas, gleiche Mischung).
        let kugeln = self.experience_orb_vertices(right, up, tick_progress);
        let an = (draws.opaque + draws.translucent) as usize;
        draws.translucent += kugeln.len() as u32;
        out.splice(an..an, kugeln);
        self.particle_vertices = out;
        draws
    }

    // ExperienceOrbRenderer: ein zur Kamera gedrehtes Viereck, 0,3 Bloecke breit, 0,1 ueber
    // der Kugelposition und nach oben versetzt (das Bild reicht von -0,25 bis 0,75). Welches
    // der 16 Bilder, sagt der Wert der Kugel (ExperienceOrb.getIcon); die Farbe pulsiert mit
    // dem Alter zwischen Gruen und Gelb, halb durchsichtig.
    pub(crate) fn experience_orb_vertices(&self, right: Vec3, up: Vec3, tick_progress: f32) -> Vec<render::ParticleVertex> {
        let Some(&[u0, v0, u1, v1]) = self.particle_atlas.sprites("experience_orb").first() else { return Vec::new() };
        let mut out = Vec::new();
        for (_, e) in self.entities.iter() {
            if world::entity_type_name(e.entity_type) != Some("experience_orb") || e.invisible {
                continue;
            }
            let bild = experience_orb_icon(e.orb_value);
            let (du, dv) = ((u1 - u0) / 4.0, (v1 - v0) / 4.0);
            let (a, b) = (u0 + du * (bild % 4) as f32, v0 + dv * (bild / 4) as f32);
            let (x, y, z) = e.render_pos(tick_progress);
            let mitte = (DVec3::new(x, y + 0.1, z) - self.camera.position).as_vec3() + up * 0.075;
            let f = e.age_ticks(tick_progress) / 2.0;
            let farbe = [(f.sin() + 1.0) * 0.5, 1.0, ((f + 4.188_790_3).sin() + 1.0) * 0.1, 128.0 / 255.0];
            let (r, u) = (right * 0.15, up * 0.15);
            let ecken = [(mitte + r - u, [a + du, b + dv]), (mitte + r + u, [a + du, b]), (mitte - r + u, [a, b]), (mitte - r - u, [a, b + dv])];
            for i in [0, 1, 2, 0, 2, 3] {
                let (pos, uv) = ecken[i];
                out.push(render::ParticleVertex { position: pos.to_array(), color: farbe, uv });
            }
        }
        out
    }

    // Die Partikel zusammen mit allem, was sie aus der Welt lesen: Kamera, Stufe, Bloecke und
    // wo ein Entity steht (fuer die Emitter). Als Closure statt als Rueckgabewert, weil die
    // Welt nur geliehen werden kann, solange `self.particles` getrennt davon veraenderbar
    // bleibt.
    // LivingEntity.tickStatusEffects, Client-Zweig: jedes Lebewesen mit Trankeffekten streut
    // ab und zu einen Wirbel in einer seiner Effektfarben: mit Chance 1 / (i * j) je Tick,
    // i = 15 unsichtbar bzw. 4 sonst, j = 5 fuer Ambient-Effekte bzw. 1 sonst, an einer
    // zufaelligen Stelle des Koerpers.
    //
    // Fehlte frueher ganz.
    // AbstractArrow.tick im Client: ein fliegender Krit-Pfeil streut je Tick vier Krit-
    // Partikel entlang seiner Geschwindigkeit, jeder mit der Gegengeschwindigkeit und 0.2 nach
    // oben -- die Spur, an der man einen voll gespannten Schuss erkennt.
    pub(crate) fn tick_crit_trails(&mut self) {
        let spuren: Vec<_> = self.entities.iter().filter_map(|(_, e)| e.crit_trail()).collect();
        if spuren.is_empty() {
            return;
        }
        let krit = protocol::particle::ParticleEffect { type_id: protocol::particle::PARTICLE_CRIT, options: protocol::particle::ParticleOptions::None };
        self.with_particles(|p, ctx, _| {
            for ((x, y, z), (vx, vy, vz)) in spuren {
                for i in 0..4 {
                    let t = f64::from(i) / 4.0;
                    p.add_world_particle(&krit, DVec3::new(x + vx * t, y + vy * t, z + vz * t), DVec3::new(-vx, -vy + 0.2, -vz), ctx);
                }
            }
        });
    }

    pub(crate) fn tick_potion_swirls(&mut self) {
        let mut quellen: Vec<(Vec<u32>, bool, bool, DVec3, f64, f64)> = Vec::new();
        for (_, e) in self.entities.iter() {
            if e.potion_swirls.is_empty() {
                continue;
            }
            let (w, h) = e.hit_size();
            let (x, y, z) = e.hit_pos();
            quellen.push((e.potion_swirls.clone(), e.potion_swirls_ambient, e.invisible, DVec3::new(x, y, z), w, h));
        }
        if !self.own_potion_swirls.is_empty() {
            quellen.push((
                self.own_potion_swirls.clone(),
                self.own_potion_swirls_ambient,
                self.own_invisible,
                self.physics.pos,
                0.6,
                self.physics.current_height(),
            ));
        }
        if quellen.is_empty() {
            return;
        }
        self.with_particles(|p, ctx, _| {
            for (farben, ambient, unsichtbar, pos, w, h) in quellen {
                let i = if unsichtbar { 15 } else { 4 };
                let j = if ambient { 5 } else { 1 };
                if p.rng().next_int(i * j) != 0 {
                    continue;
                }
                let farbe = farben[p.rng().next_int(farben.len() as i32) as usize];
                let r = p.rng();
                let x = pos.x + w * (2.0 * r.next_double() - 1.0) * 0.5;
                let y = pos.y + h * r.next_double();
                let z = pos.z + w * (2.0 * r.next_double() - 1.0) * 0.5;
                p.add_potion_swirl(farbe, DVec3::new(x, y, z), ctx);
            }
        });
    }

    // AreaEffectCloud.clientTick fuer jede Trankwolke in der Welt (Verweiltrank, Drachenatem).
    pub(crate) fn tick_area_clouds(&mut self) {
        let wolken: Vec<(DVec3, f32, bool, protocol::particle::ParticleEffect)> = self
            .entities
            .iter()
            .filter(|(_, e)| world::entity_type_name(e.entity_type) == Some("area_effect_cloud"))
            .map(|(_, e)| {
                let (x, y, z) = e.pos;
                (DVec3::new(x, y, z), e.cloud_radius, e.cloud_waiting, e.cloud_particle.clone())
            })
            .collect();
        if wolken.is_empty() {
            return;
        }
        self.with_particles(|p, ctx, _| {
            for (pos, radius, wartet, partikel) in &wolken {
                p.tick_area_cloud(*pos, *radius, *wartet, partikel, ctx);
            }
        });
    }

    // Kruemel, waehrend andere essen (ItemStack.onUseTick -> Consumable): ab gut einem Fuenftel
    // der Essdauer alle vier Ticks fuenf Stueck vor dem Gesicht. Der Client rechnet das fuer
    // jeden Spieler selbst, der Server schickt dafuer nichts.
    pub(crate) fn tick_eating_particles(&mut self) {
        let mut quellen: Vec<(&'static str, DVec3, f32, f32)> = Vec::new();
        for (_, e) in self.entities.iter() {
            let Some((item, ticks)) = e.used_item() else { continue };
            let Some(stack) = world::item_components::resolve(item.stack) else { continue };
            let Some(dauer) = world::item_behavior::consume_ticks(&stack) else { continue };
            if !eating_crumbs_due(dauer, ticks) || !world::item_behavior::consume_particles(&stack) {
                continue;
            }
            let Some(name) = particle_item_name(&item) else { continue };
            let (x, y, z) = e.pos;
            quellen.push((name, DVec3::new(x, y + e.eye_height(), z), e.yaw_deg, e.pitch_deg));
        }
        if quellen.is_empty() {
            return;
        }
        self.with_particles(|p, ctx, _| {
            for (name, auge, yaw, pitch) in quellen {
                p.add_item_use_particles(name, 5, auge, yaw, pitch, ctx);
            }
        });
    }

    // Dasselbe fuer die eigene Figur; `fertig` = das Essen ist gerade beendet, dann sechzehn
    // Kruemel auf einmal (Consumable.onConsume).
    pub(crate) fn own_eating_particles(&mut self, fertig: bool) {
        let Some((_, Some(stack))) = self.own_used_item() else { return };
        let Some(dauer) = world::item_behavior::consume_ticks(&stack) else { return };
        if !world::item_behavior::consume_particles(&stack) || (!fertig && !eating_crumbs_due(dauer, self.own_use_ticks)) {
            return;
        }
        let Some(name) = particle_item_name(&world::HeldItem::from_stack(&stack)) else { return };
        let (auge, yaw, pitch) = (self.physics.eye_pos(), self.camera.yaw, self.look_pitch_deg());
        self.with_particles(|p, ctx, _| p.add_item_use_particles(name, if fertig { 16 } else { 5 }, auge, yaw, pitch, ctx));
    }

    // LivingEntity.breakItem (Entity-Ereignisse 47..52, 65, 68): ein Item ist zerbrochen. Der
    // Klang kommt nicht vom Server, jeder Client spielt ihn selbst -- dazu fuenf Splitter.
    pub(crate) fn item_broken(&mut self, entity_id: i32, status: i8) {
        let eigener = self.own_entity_id == Some(entity_id);
        let item = if eigener {
            let (haupt, zweit) = self.own_hand_items();
            let ruestung = self.own_armor();
            match status {
                47 => haupt,
                48 => zweit,
                49..=52 => ruestung[(52 - status) as usize],
                _ => None,
            }
        } else {
            let Some(e) = self.entities.get(entity_id) else { return };
            match status {
                47 => e.main_hand,
                48 => e.off_hand,
                49..=52 => e.armor[(52 - status) as usize],
                65 => e.body_item,
                _ => None,
            }
        };
        let Some(item) = item else { return };
        let (pos, auge, yaw, pitch) = if eigener {
            (self.physics.pos, self.physics.eye_pos(), self.camera.yaw, self.look_pitch_deg())
        } else {
            let Some(e) = self.entities.get(entity_id) else { return };
            let (x, y, z) = e.pos;
            (DVec3::new(x, y, z), DVec3::new(x, y + e.eye_height(), z), e.yaw_deg, e.pitch_deg)
        };
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let tonhoehe = 0.8 + (zufallswert(self.type_pitch_seed) + 1.0) * 0.5 * 0.4;
        self.play_event_at_scaled("entity.item.break", pos, 0.8, tonhoehe, protocol::packets::SoundCategory::Players);
        if let Some(name) = particle_item_name(&item) {
            self.with_particles(|p, ctx, _| p.add_item_use_particles(name, 5, auge, yaw, pitch, ctx));
        }
    }

    // Snowball/ThrownEgg.handleEntityEvent(3): das Wurfgeschoss ist aufgeschlagen.
    pub(crate) fn thrown_item_hit(&mut self, entity_id: i32) {
        let Some(e) = self.entities.get(entity_id) else { return };
        let typ = world::entity_type_name(e.entity_type);
        let streuen = match typ {
            Some("snowball") => false,
            Some("egg") => true,
            _ => return,
        };
        let name = e.item.as_ref().and_then(particle_item_name).or(typ).unwrap_or("snowball");
        let (x, y, z) = e.pos;
        let pos = DVec3::new(x, y, z);
        self.with_particles(|p, ctx, _| p.add_thrown_item_hit(name, pos, streuen, ctx));
    }

    pub(crate) fn with_particles(
        &mut self,
        f: impl FnOnce(&mut particles::Particles, &particles::SpawnContext, &dyn Fn(i32) -> Option<particles::EmitterBody>),
    ) {
        self.particles.set_tuning(self.settings.particle_tuning);
        let ctx = particles::SpawnContext {
            camera: self.camera.view_position(),
            mode: particles::ParticleMode::from_index(self.settings.particles_mode),
            world: &self.world,
            shapes: &self.block_shapes,
            biomes: &self.biome_palette,
            player: Some(particles::NearPlayer { pos: self.physics.pos, vel_y: self.physics.vel.y }),
        };
        let entities = &self.entities;
        let own_id = self.own_entity_id;
        // Die eigene Figur steht nicht in `entities` (dort nur die anderen).
        let own = particles::EmitterBody { pos: self.physics.pos, width: self.physics.width(), height: self.physics.current_height() };
        let body = move |id: i32| -> Option<particles::EmitterBody> {
            if Some(id) == own_id {
                return Some(own);
            }
            let e = entities.get(id)?;
            let (width, height) = e.hit_size();
            Some(particles::EmitterBody { pos: DVec3::new(e.pos.0, e.pos.1, e.pos.2), width, height })
        };
        f(&mut self.particles, &ctx, &body);
        for t in self.particles.take_new_unsupported() {
            self.push_log(format!("Partikel noch nicht umgesetzt: {}", protocol::particle::particle_name(t).unwrap_or("?")));
        }
    }
}
