// Entity-Tick: Schaden, Interpolation, Geschosse, Items, Erfahrungskugeln, fallende Bloecke,
// Koerper- und Kopfdrehung, Animationen und Handschwung.

use super::*;

impl Entity {
    // Ein CLIENT-Tick dieses Entities. Reihenfolge wie in Vanilla:
    //   Entity.baseTick      -> tickLastPos()      (last_* = aktueller Stand)
    //   LivingEntity.tick    -> interpolator.tick() (ein Schritt Richtung Ziel)
    //   LivingEntity.tick    -> lerpHeadYaw()       (ein Schritt der Kopfdrehung)
    // Danach die rein clientseitigen Animationen, die Vanilla ebenfalls pro Tick aus der in
    // DIESEM Tick zurueckgelegten Strecke speist (LimbAnimator, Umhang).
    pub(crate) fn tick(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, spieler: &[OrbPlayer], body_turn: bool) {
        self.last_pos = self.pos;
        self.last_yaw_deg = self.yaw_deg;
        self.last_pitch_deg = self.pitch_deg;
        self.last_head_yaw_deg = self.head_yaw_deg;
        self.last_body_yaw_deg = self.body_yaw_deg;
        // LivingEntity.baseTick, allererste Zeile.
        self.last_hand_swing_progress = self.hand_swing_progress;

        // Projektile fliegen selbst, alle anderen werden zur Serverposition interpoliert.
        match self.projectile {
            // Selbst gelenkt: die Stelle, die der Client eben ausgerechnet hat (s. driven).
            None if self.driven => {
                if let Some((pos, yaw, pitch)) = self.driven_next.take() {
                    self.pos = pos;
                    self.target_pos = pos;
                    self.yaw_deg = yaw;
                    self.target_yaw_deg = yaw;
                    self.pitch_deg = pitch;
                    self.target_pitch_deg = pitch;
                }
                self.interp_steps = 0;
            }
            Some(kind) => self.tick_projectile(kind, world, shapes),
            None if self.item_physics => self.tick_item(world, shapes),
            None if self.block_physics => self.tick_falling_block(world, shapes),
            None if self.orb_physics => self.tick_orb(world, shapes, spieler),
            None => self.tick_interpolation(),
        }
        self.tick_head_yaw();
        self.tick_animations();
        self.tick_paddles();
        if is_player(self.entity_type) {
            let id = |h: Option<HeldItem>| h.map_or(0, |h| h.item_id);
            self.swap.tick(id(self.main_hand), id(self.off_hand));
            self.last_sword = remember_sword(self.last_sword, self.main_hand, self.off_hand);
        }
        // Erst NACH tickMovement (Bewegung, Handschwung), wie in LivingEntity.tick.
        self.tick_body_yaw(body_turn);
        // LivingEntity.tick: `if (isGliding()) glidingTicks++; else glidingTicks = 0;`
        self.gliding_ticks = if self.gliding { self.gliding_ticks.saturating_add(1) } else { 0 };
        // LivingEntity.updateSwimAmount -- haengt an isVisuallySwimming, also nur an der Pose.
        self.last_swim_amount = self.swim_amount;
        let step = if self.pose == POSE_SWIMMING { 0.09 } else { -0.09 };
        self.swim_amount = (self.swim_amount + step).clamp(0.0, 1.0);
        self.in_water = self.touches_water(world);
        self.age_ticks += 1.0;
        // LivingEntity.tick: `if (hurtTime > 0) hurtTime--;` -- ein Tick, ein Schritt.
        self.hurt_ticks = self.hurt_ticks.saturating_sub(1);
        self.ticks_since_hurt = self.ticks_since_hurt.saturating_add(1);
        self.crit_grace = self.crit_grace.saturating_sub(1);
    }

    // Der Server hat mitgeteilt, dass dieses Entity Schaden genommen hat
    // (EntityDamageS2CPacket -> Entity.onDamaged). Nur Lebewesen blitzen auf, s. is_living.
    pub(crate) fn on_damaged(&mut self) {
        if is_living(self.entity_type) {
            self.hurt_ticks = MAX_HURT_TIME;
            self.ticks_since_hurt = 0;
            // Ein Crit, der eben erst angekommen ist, gehoert zu DIESEM Treffer. Ohne das
            // Zuruecksetzen bliebe die Crit-Markierung am naechsten, gewoehnlichen Treffer
            // haengen.
            self.hurt_by_crit = self.crit_grace > 0;
        }
    }

    // Der Server meldet einen kritischen Treffer auf dieses Entity (Animation CRIT).
    pub(crate) const fn on_crit(&mut self) {
        // Der Normalfall: das Schadenspaket kam schon, der Blitz laeuft bereits.
        if self.hurt_ticks > 0 {
            self.hurt_by_crit = true;
        }
        self.crit_grace = CRIT_GRACE_TICKS;
    }

    // Blitzt das Entity gerade IN CRIT-FARBE? Immer nur zusammen mit is_hurt() zu gebrauchen:
    // die Markierung ueberlebt den Blitz, ausgewertet wird sie nur waehrend er laeuft.
    #[must_use] 
    pub const fn is_hurt_by_crit(&self) -> bool {
        self.hurt_ticks > 0 && self.hurt_by_crit
    }

    // Blitzt das Entity gerade rot? Vanillas Renderer fragt `hurtTime > 0 || deathTime > 0` --
    // den Todes-Anteil kennen wir nicht (deathTime wird nie ueber das Netz geschickt, Vanilla
    // zaehlt ihn lokal nach dem Tod-Status hoch), der Schadens-Anteil ist derselbe.
    #[must_use] 
    pub const fn is_hurt(&self) -> bool {
        self.hurt_ticks > 0
    }

    // Blitzt das Entity bei einer Blitzdauer von `ticks` gerade auf? Bei 10 dasselbe wie
    // is_hurt(); die Dauer ist einstellbar (Settings::damage_tint_ticks).
    #[must_use] 
    pub const fn damage_flash(&self, ticks: u32) -> bool {
        self.ticks_since_hurt < ticks
    }

    // Stammt der letzte Treffer von einem Crit? Nur zusammen mit damage_flash auszuwerten.
    #[must_use] 
    pub const fn hurt_by_crit(&self) -> bool {
        self.hurt_by_crit
    }

    // PositionInterpolator.tick(): der Lerp-Nenner ist die Zahl der NOCH offenen Schritte,
    // nicht die Gesamtzahl -- damit ist der letzte Schritt eine volle Angleichung und die
    // Position landet exakt auf dem Ziel, statt sich ihm nur zu naehern.
    //
    // Vanilla addiert an dieser Stelle zusaetzlich die Strecke, die das Entity im selben Tick
    // SELBST zurueckgelegt hat (der `lastPos`-Zweig, fuer Fahrzeuge, die der Client mit
    // simuliert). Fuer fremde Entities simulieren wir nichts -- diese Differenz ist bei uns
    // immer null, der Zweig damit wirkungslos und bewusst weggelassen.
    pub(crate) fn tick_interpolation(&mut self) {
        if self.interp_steps == 0 {
            return;
        }
        let d = 1.0 / f64::from(self.interp_steps);
        self.pos = (
            self.pos.0 + (self.target_pos.0 - self.pos.0) * d,
            self.pos.1 + (self.target_pos.1 - self.pos.1) * d,
            self.pos.2 + (self.target_pos.2 - self.pos.2) * d,
        );
        self.yaw_deg = lerp_angle(d as f32, self.yaw_deg, self.target_yaw_deg);
        self.pitch_deg += (self.target_pitch_deg - self.pitch_deg) * d as f32;
        self.interp_steps -= 1;
    }

    // Ein Tick Projektilflug -- s. ProjectileKind fuer Herkunft und Reihenfolge.
    pub(crate) fn tick_projectile(&mut self, kind: ProjectileKind, world: &crate::chunk::World, shapes: &crate::BlockShapes) {
        self.launch_ticks = self.launch_ticks.saturating_sub(1);
        // AbstractArrow.tick: steht der Pfeil in der Kollisionsform seines Blocks, steckt er.
        if kind == ProjectileKind::Arrow && !self.in_ground && point_in_collision(self.pos, world, shapes) {
            self.vel = (0.0, 0.0, 0.0);
            self.in_ground = true;
        }
        if self.in_ground {
            // AbstractArrow: steckt der Pfeil, ruehrt sich nichts mehr. Ein Wurfgeschoss ist
            // an der Stelle schon zu Ende, der Server raeumt es gleich weg.
            return;
        }
        let im_wasser = self.in_water(world);
        match kind {
            ProjectileKind::Throwable => {
                // applyGravity() VOR applyInertia() -- beides vor der Bewegung.
                self.vel.1 -= kind.gravity();
                let f = if im_wasser { kind.water_inertia() } else { 0.99 };
                self.vel = (self.vel.0 * f, self.vel.1 * f, self.vel.2 * f);
                self.move_projectile(world, shapes, false);
            }
            // AbstractHurtingProjectile.tick: applyInertia (hier wirkungslos), dann die Bewegung
            // bis zum ersten Block -- dort explodiert sie auf dem Server und wird entfernt.
            ProjectileKind::WindCharge => self.move_projectile(world, shapes, false),
            ProjectileKind::Arrow => {
                // Die Wasserreibung steht VOR der Bewegung, die Luftreibung DAHINTER.
                if im_wasser {
                    let f = kind.water_inertia();
                    self.vel = (self.vel.0 * f, self.vel.1 * f, self.vel.2 * f);
                }
                self.move_projectile(world, shapes, true);
                if !im_wasser {
                    self.vel = (self.vel.0 * 0.99, self.vel.1 * 0.99, self.vel.2 * 0.99);
                }
                if !self.in_ground {
                    self.vel.1 -= kind.gravity();
                }
            }
        }
        // Projectile.updateRotation: die Nase zeigt in Flugrichtung.
        let waagerecht = (self.vel.0 * self.vel.0 + self.vel.2 * self.vel.2).sqrt();
        if waagerecht > 1.0e-7 || self.vel.1.abs() > 1.0e-7 {
            let ziel_yaw = self.vel.0.atan2(self.vel.2).to_degrees() as f32;
            let ziel_pitch = self.vel.1.atan2(waagerecht).to_degrees() as f32;
            self.yaw_deg = lerp_rotation(self.yaw_deg, ziel_yaw);
            self.pitch_deg = lerp_rotation(self.pitch_deg, ziel_pitch);
            self.target_yaw_deg = self.yaw_deg;
            self.target_pitch_deg = self.pitch_deg;
        }
        // Die Vorhersage IST ab jetzt die Wahrheit, bis der Server widerspricht -- sonst
        // zoege tick_interpolation das Projektil beim naechsten Paket wieder zurueck.
        //
        // tracked_pos bleibt dagegen die SERVER-Position: auf sie rechnet der Server seine
        // relativen Bewegungspakete (EntityTrackerEntry: Delta zur zuletzt GESENDETEN
        // Position). ECHTER BUG gewesen: hier stand auch tracked_pos = pos. Jedes Delta landete
        // dann auf der schon vorausgeflogenen Vorhersage -- die Bewegung zaehlte doppelt.
        // Windkugeln (alle zehn Ticks ein Paket) schossen davon und flogen weit ueber die
        // Trackingweite hinaus, Pfeile blieben mit dem letzten Paket vor der Wand in der
        // Luft stehen.
        self.target_pos = self.pos;
        self.interp_steps = 0;
    }

    // ItemEntity.tick, der Teil, den auch der Client rechnet: Auftrieb in Wasser/Lava oder
    // Schwerkraft 0.04, dann -- solange es faellt, rutscht oder jeden vierten Tick zur Probe --
    // die Bewegung mit Blockkollision, Reibung 0.98 (am Boden mal Rutschigkeit des Blocks
    // darunter) und Aufprall (onEntityLand: vy = 0).
    pub(crate) fn tick_item(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes) {
        let (x, y, z) = self.pos;
        // Ohne geladene Spalte gibt es keinen Boden -- dann nicht ins Leere fallen lassen,
        // sondern auf die naechste Serverposition warten.
        if world.column((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16)).is_none() {
            return;
        }
        let fluid = crate::fluid::fluid_kind(world.block_at(x.floor() as i32, (y + 0.1).floor() as i32, z.floor() as i32));
        match fluid {
            Some(crate::fluid::FluidKind::Water | crate::fluid::FluidKind::Lava) => {
                let f = if fluid == Some(crate::fluid::FluidKind::Water) { 0.99 } else { 0.95 };
                let auftrieb = if self.vel.1 < 0.06 { 5.0e-4 } else { 0.0 };
                self.vel = (self.vel.0 * f, self.vel.1 + auftrieb, self.vel.2 * f);
            }
            None => self.vel.1 -= 0.04,
        }
        let waagerecht = self.vel.0 * self.vel.0 + self.vel.2 * self.vel.2;
        if !self.on_ground || waagerecht > 1.0e-5 || (self.age_ticks as u32).is_multiple_of(4) {
            let (neu, stoss) = move_with_collisions(self.pos, self.vel, 0.125, 0.25, world, shapes);
            self.on_ground = stoss[1] && self.vel.1 < 0.0;
            self.pos = neu;
            if stoss[0] {
                self.vel.0 = 0.0;
            }
            if stoss[2] {
                self.vel.2 = 0.0;
            }
            if stoss[1] {
                self.vel.1 = 0.0;
            }
            let f = if self.on_ground {
                let unten = world.block_at(self.pos.0.floor() as i32, (self.pos.1 - 0.500001).floor() as i32, self.pos.2.floor() as i32);
                block_slipperiness(unten) * 0.98
            } else {
                0.98
            };
            self.vel = (self.vel.0 * f, self.vel.1 * 0.98, self.vel.2 * f);
        }
        // tracked_pos bleibt die Serverbasis fuer relative Pakete (s. tick_projectile).
        self.target_pos = self.pos;
        self.interp_steps = 0;
    }

    // ExperienceOrb.tick, wie ihn auch der Client rechnet. Zuerst ein Schritt zur Serverposition
    // (Orbs haben in Vanilla einen eigenen Interpolator), dann die eigene Bewegung:
    // - unter Wasser (Augenhoehe 0.425) Auftrieb, sonst Schwerkraft 0.03 -- nicht, solange er
    //   in einem Block steckt;
    // - Lava wirft ihn hoch (in Vanilla mit zufaelligem seitlichem Anteil um null, den der
    //   Server mit seinem eigenen Zufall bestimmt -- hier nur der senkrechte Teil);
    // - Sog zum verfolgten Spieler: (1 - Abstand/8)^2 * 0.1 in Richtung seiner halben
    //   Augenhoehe. Verfolgt wird der naechste Spieler unter 8 Bloecken, bis er weiter weg ist;
    // - Bewegung mit Blockkollision (Hitbox 0,5), Reibung 0.98 auf allen Achsen, am Boden
    //   mal Rutschigkeit des Blocks darunter;
    // - kam er schneller als die Schwerkraft auf, springt er mit 0.4 der Fallgeschwindigkeit
    //   zurueck.
    pub(crate) fn tick_orb(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, spieler: &[OrbPlayer]) {
        self.tick_interpolation();
        let (x, y, z) = self.pos;
        if world.column((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16)).is_none() {
            return;
        }
        let steckt = point_in_collision((x, y + 0.25, z), world, shapes);
        let block = |by: f64| world.block_at(x.floor() as i32, by.floor() as i32, z.floor() as i32);
        let fluid = |by: f64| crate::fluid::fluid_kind(block(by));
        if fluid(y + 0.425) == Some(crate::fluid::FluidKind::Water) {
            self.vel = (self.vel.0 * 0.99, (self.vel.1 + 5.0e-4).min(0.06), self.vel.2 * 0.99);
        } else if !steckt {
            self.vel.1 -= ORB_GRAVITY;
        }
        if fluid(y) == Some(crate::fluid::FluidKind::Lava) {
            self.vel = (0.0, 0.2, 0.0);
        }
        // Dem bisherigen Spieler folgen, solange er unter 8 Bloecken bleibt, sonst den naechsten
        // suchen.
        let abstand2 = |p: &OrbPlayer| (p.pos.0 - x).powi(2) + (p.pos.1 - y).powi(2) + (p.pos.2 - z).powi(2);
        let bisher = self.orb_target.and_then(|id| spieler.iter().find(|p| p.id == id)).filter(|p| abstand2(p) <= ORB_FOLLOW_RANGE * ORB_FOLLOW_RANGE);
        let ziel = bisher.or_else(|| {
            spieler.iter().filter(|p| abstand2(p) < ORB_FOLLOW_RANGE * ORB_FOLLOW_RANGE).min_by(|a, b| abstand2(a).total_cmp(&abstand2(b)))
        });
        self.orb_target = ziel.map(|p| p.id);
        if let Some(p) = ziel {
            let d = (p.pos.0 - x, p.pos.1 + p.eye_height / 2.0 - y, p.pos.2 - z);
            let laenge = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt();
            if laenge > 1.0e-4 {
                let sog = (1.0 - laenge / ORB_FOLLOW_RANGE).powi(2) * 0.1 / laenge;
                self.vel = (self.vel.0 + d.0 * sog, self.vel.1 + d.1 * sog, self.vel.2 + d.2 * sog);
            }
        }
        let vy = self.vel.1;
        let (neu, stoss) = move_with_collisions(self.pos, self.vel, 0.25, 0.5, world, shapes);
        let aufgekommen = stoss[1] && vy < 0.0;
        self.on_ground = aufgekommen;
        self.pos = neu;
        if stoss[0] {
            self.vel.0 = 0.0;
        }
        if stoss[2] {
            self.vel.2 = 0.0;
        }
        if stoss[1] {
            self.vel.1 = 0.0;
        }
        let f = if self.on_ground {
            let unten = world.block_at(self.pos.0.floor() as i32, (self.pos.1 - 0.999999).floor() as i32, self.pos.2.floor() as i32);
            block_slipperiness(unten) * 0.98
        } else {
            0.98
        };
        self.vel = (self.vel.0 * f, self.vel.1 * f, self.vel.2 * f);
        if aufgekommen && vy < -ORB_GRAVITY {
            self.vel.1 = -vy * 0.4;
        }
        // Ist die Angleichung an die letzte Meldung durch, gilt die eigene Bahn (s. tick_item).
        if self.interp_steps == 0 {
            self.target_pos = self.pos;
        }
    }

    // FallingBlockEntity.tick, der Teil, den auch der Client rechnet: Schwerkraft 0.04, Bewegung
    // mit Blockkollision (Hitbox 0,98 x 0,98), danach Reibung 0.98. Abgesetzt wird der Block
    // vom Server -- der schickt dann das Block-Update und entfernt das Entity.
    pub(crate) fn tick_falling_block(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes) {
        let (x, z) = (self.pos.0.floor() as i32, self.pos.2.floor() as i32);
        if world.column(x.div_euclid(16), z.div_euclid(16)).is_none() {
            return;
        }
        self.vel.1 -= 0.04;
        let (neu, stoss) = move_with_collisions(self.pos, self.vel, 0.49, 0.98, world, shapes);
        self.on_ground = stoss[1] && self.vel.1 < 0.0;
        self.pos = neu;
        if stoss[0] {
            self.vel.0 = 0.0;
        }
        if stoss[2] {
            self.vel.2 = 0.0;
        }
        if stoss[1] {
            self.vel.1 = 0.0;
        }
        self.vel = (self.vel.0 * 0.98, self.vel.1 * 0.98, self.vel.2 * 0.98);
        // PrimedTnt bremst am Boden und zaehlt die Zuendzeit herunter.
        if entity_type_name(self.entity_type) == Some("tnt") {
            if self.on_ground {
                self.vel = (self.vel.0 * 0.7, self.vel.1 * -0.5, self.vel.2 * 0.7);
            }
            self.tnt_fuse -= 1;
        }
        self.target_pos = self.pos;
        self.interp_steps = 0;
    }

    // Steht der Mittelpunkt des Projektils in einer Fluessigkeit? Vanilla fragt isInWater()
    // gegen die ganze Hitbox; fuer ein Geschoss von 0.25 Bloecken Kantenlaenge ist der
    // Mittelpunkt nah genug und spart den Boxdurchlauf pro Tick.
    pub(crate) fn in_water(&self, world: &crate::chunk::World) -> bool {
        let (x, y, z) = (self.pos.0.floor() as i32, self.pos.1.floor() as i32, self.pos.2.floor() as i32);
        matches!(crate::fluid::fluid_kind(world.block_at(x, y, z)), Some(crate::fluid::FluidKind::Water))
    }

    // Ein Bewegungsschritt samt Blocktreffer. Vanilla schiesst dafuer einen Strahl von der
    // aktuellen Position zur angestrebten (ClipContext mit Block.COLLIDER) und bleibt am
    // Treffer stehen; der Pfeil steckt dann, das Wurfgeschoss zerplatzt dort.
    // `pfeil`: AbstractArrow.onBlockHit schiebt den Pfeil danach um 0.05 je Achse gegen die
    // Flugrichtung zurueck (Vorzeichen der Geschwindigkeit) -- die Position liegt knapp vor
    // der Flaeche, die Spitze des Modells (4 Pixel voraus) steckt im Block.
    pub(crate) fn move_projectile(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, pfeil: bool) {
        let ziel = (self.pos.0 + self.vel.0, self.pos.1 + self.vel.1, self.pos.2 + self.vel.2);
        if let Some(t) = block_hit_along(self.pos, self.vel, world, shapes) {
            self.pos = (self.pos.0 + self.vel.0 * t, self.pos.1 + self.vel.1 * t, self.pos.2 + self.vel.2 * t);
            if pfeil {
                let schub = |v: f64| if v > 0.0 { f64::from(0.05f32) } else if v < 0.0 { -f64::from(0.05f32) } else { 0.0 };
                self.pos = (self.pos.0 - schub(self.vel.0), self.pos.1 - schub(self.vel.1), self.pos.2 - schub(self.vel.2));
            }
            self.vel = (0.0, 0.0, 0.0);
            self.in_ground = true;
            return;
        }
        self.pos = ziel;
    }

    // AbstractBoat.tick: ein bewegtes Ruder dreht je Tick PI/8 weiter, ein ruhendes steht auf 0.
    pub(crate) fn tick_paddles(&mut self) {
        for (stand, an) in self.paddle_positions.iter_mut().zip(self.boat_paddles) {
            *stand = if an { *stand + std::f32::consts::PI / 8.0 } else { 0.0 };
        }
    }

    // AbstractBoat.getRowingTime: wie weit das Ruder `seite` (0 links, 1 rechts) im Bild
    // gerudert hat -- zwischen dem letzten und diesem Tick, ruhend 0.
    #[must_use] 
    pub fn rowing_time(&self, seite: usize, tick_progress: f32) -> f32 {
        if !self.boat_paddles[seite] {
            return 0.0;
        }
        let stand = self.paddle_positions[seite];
        let t = tick_progress.clamp(0.0, 1.0);
        stand - std::f32::consts::PI / 8.0 * (1.0 - t)
    }

    // LivingEntity.tick: Koerper zur Laufrichtung (s. entity_model::turn_body). Nur fuer
    // Lebewesen -- ein Boot, eine Lore oder ein Display dreht sich genau so, wie der Server sagt.
    pub(crate) fn tick_body_yaw(&mut self, body_turn: bool) {
        self.use_ticks = if self.using_item_right_arm().is_some() { self.use_ticks.saturating_add(1) } else { 0 };
        if !is_living(self.entity_type) {
            self.body_yaw_deg = self.yaw_deg;
            return;
        }
        // Mehr Animationen: beim Essen, Trinken und Blocken holt der Koerper je Tick die Haelfte
        // des Wegs zum Kopf auf -- man steht dem zugewandt, was man tut.
        if body_turn && is_player(self.entity_type) && self.used_item().is_some_and(|(h, _)| turns_body_to_head(&h)) {
            self.body_yaw_deg += crate::entity_model::wrap_degrees(self.head_yaw_deg - self.body_yaw_deg) * 0.5;
            return;
        }
        let max_relative = if self.is_blocking() {
            crate::entity_model::BLOCKING_MAX_RELATIVE
        } else {
            crate::entity_model::BodyYaw::MAX_RELATIVE
        };
        let (dx, dz) = (self.pos.0 - self.last_pos.0, self.pos.2 - self.last_pos.2);
        self.body_yaw_deg = crate::entity_model::turn_body(self.body_yaw_deg, self.yaw_deg, dx, dz, self.hand_swing_progress > 0.0, max_relative);
    }

    // PlayerEntity.isBlocking: ein Spieler, der einen Schild benutzt, und zwar schon laenger als
    // dessen Block-Verzoegerung (BlocksAttacksComponent, Schild: 0,25 s = 5 Ticks). Nur hierfuer
    // gebraucht -- das Hochreissen im BILD bleibt absichtlich sofort (s. coral-client-schild-sofort).
    pub(crate) fn is_blocking(&self) -> bool {
        let Some(haupthand) = self.using_main_hand() else { return false };
        let item = if haupthand { self.main_hand } else { self.off_hand };
        let Some(blockt) = item.and_then(|i| held_blocks_attacks(&i)) else { return false };
        is_player(self.entity_type) && self.use_ticks >= block_delay_ticks(&blockt)
    }

    // LivingEntity.tick(): lerpAngleDegrees(1/headTrackingIncrements, headYaw, serverHeadYaw),
    // danach herunterzaehlen.
    pub(crate) fn tick_head_yaw(&mut self) {
        if self.head_steps == 0 {
            return;
        }
        self.head_yaw_deg = lerp_angle(1.0 / self.head_steps as f32, self.head_yaw_deg, self.server_head_yaw_deg);
        self.head_steps -= 1;
    }

    // Schrittanimation und Umhang -- beides rechnet Vanilla pro Tick aus der Strecke dieses
    // Ticks (LivingEntity.tickMovement -> LimbAnimator.updateLimbs mit getX() - lastX).
    //
    // Frueher lief das hier beim PAKETEINTREFFEN und musste die Zahl der verstrichenen Ticks
    // aus der Wanduhr schaetzen, weil die Update-Rate des Trackers (10 Hz / 6.7 Hz) nicht die
    // Tickrate ist. Seit die Position tick-quantisiert ist, entfaellt die Schaetzung: die
    // Strecke pro Tick steht direkt da.
    pub(crate) fn tick_animations(&mut self) {
        let (dx, dz) = ((self.pos.0 - self.last_pos.0) as f32, (self.pos.2 - self.last_pos.2) as f32);
        let per_tick = (dx * dx + dz * dz).sqrt();
        self.limbs.tick(per_tick, 1.0);
        self.cape.tick([self.pos.0, self.pos.1, self.pos.2]);
        // ClientPlayerLikeState.tickMovement: am Boden die (gedeckelte) horizontale
        // Geschwindigkeit. Ob ein fremdes Entity am Boden ist, sagt uns der Server nicht --
        // die Strecke pro Tick ist die beste vorhandene Entsprechung, und sie ist genau dann
        // 0, wenn das Entity steht. add_distance_moved bleibt ungerufen: das tut Vanilla fuer
        // fremde Spieler auch nicht (s. CapeState::add_distance_moved).
        self.cape.tick_movement(per_tick.min(0.1));
        self.tick_hand_swing();
    }

    // LivingEntity.tickHandSwing, 1:1. In Vanilla ruft das nur PlayerEntity.tickMovement (und
    // HostileEntity) auf -- hier laeuft es fuer jedes Entity, was nichts kostet und nichts
    // aendert: wer nie ein Schlag-Paket bekommt, hat handSwinging = false und bleibt bei 0.
    pub(crate) fn tick_hand_swing(&mut self) {
        let dauer = self.hand_swing_duration();
        if self.hand_swinging {
            self.hand_swing_ticks += 1;
            if self.hand_swing_ticks >= dauer {
                self.hand_swing_ticks = 0;
                self.hand_swinging = false;
            }
        } else {
            self.hand_swing_ticks = 0;
        }
        self.hand_swing_progress = self.hand_swing_ticks as f32 / dauer as f32;
    }

    // LivingEntity.getHandSwingDuration: die SwingAnimationComponent des Haupthand-Items
    // (Speere 13..23 Ticks), sonst 6. Haste/Abbaulaehmung fehlen wie bisher.
    pub(crate) fn hand_swing_duration(&self) -> i32 {
        let Some(h) = self.main_hand else { return HAND_SWING_TICKS };
        match crate::item_components::resolve(h.stack) {
            Some(s) => crate::item_behavior::swing(&s).1 as i32,
            None => crate::item_registry::item_name(h.item_id).map_or(HAND_SWING_TICKS, |n| crate::item_registry::swing_duration_ticks(n) as i32),
        }
    }

    // Sticht die Figur mit einem Speer (SwingAnimationType.STAB) statt auszuholen, und haelt
    // sie ihn (ArmPose.SPEAR)? Beides haengt am Haupthand-Item.
    #[must_use] 
    pub fn main_hand_spear(&self) -> bool {
        self.main_hand.is_some_and(|h| held_piercing(&h))
    }

    pub fn off_hand_spear(&self) -> bool {
        self.off_hand.and_then(|h| crate::item_registry::item_name(h.item_id)).is_some_and(crate::item_registry::is_spear)
    }

    // LivingEntity.swingHand: ein neuer Schlag verdraengt einen laufenden NUR, wenn der schon
    // ueber die Haelfte ist. Wer schneller klickt, als die Animation laeuft, bekommt also
    // keinen Ruckel-Neustart bei jedem Klick, sondern der laufende Schwung wird zu Ende
    // gefuehrt -- genau das sieht man in Vanilla beim Spam-Klicken.
    pub(crate) fn swing_hand(&mut self, off_hand: bool) {
        if !self.hand_swinging || self.hand_swing_ticks >= self.hand_swing_duration() / 2 || self.hand_swing_ticks < 0 {
            self.hand_swing_ticks = -1;
            self.hand_swinging = true;
            self.swing_off_hand = off_hand;
        }
    }
}
