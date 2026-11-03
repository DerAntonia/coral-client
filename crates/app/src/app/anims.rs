// Mehr Animationen (Video-Tab, s. world::more_anims) an den Spielerfiguren: sammelt je Figur,
// was die Schicht wissen muss -- Leiter, Pulverschnee, Essen, Reittier, Boot -- und glaettet
// die fertige Haltung.

use crate::{DVec3, App, physics, ENTITY_BRIGHTNESS, push_map_quad};
use world::more_anims::{self as anims, MapHold};

// Unter diesem Schluessel steht die eigene Figur in der Glaettung (Entity-IDs sind >= 0).
const OWN_SMOOTHING_KEY: i32 = i32::MIN;
// Wie lange eine Figur ungesehen in der Glaettung bleibt.
const SMOOTHING_KEEP: std::time::Duration = std::time::Duration::from_secs(2);

// Die Blockmodelle der Laternen in diesen Haenden.
pub(crate) fn lantern_models(haende: [Option<world::HeldItem>; 2]) -> impl Iterator<Item = String> {
    haende.into_iter().flatten().filter_map(|h| world::item_registry::item_name(h.item_id).and_then(anims::lantern_block_model))
}

// Ab dieser Fallgeschwindigkeit (s. anims::Input::fall_speed) rudert die Figur.
const FALL_SHOWN: f32 = 1.0 / 7.0;
// Je Tick die Chance, dass ein gestreicheltes Tier Herzchen zeigt.
const PET_HEART_CHANCE: f64 = 0.015;

fn bow_arm(use_pose: Option<(bool, world::entity_model::UsePose)>) -> Option<bool> {
    use_pose.filter(|(_, h)| *h == world::entity_model::UsePose::Bow).map(|(r, _)| r)
}

fn crossbow_arm(use_pose: Option<(bool, world::entity_model::UsePose)>) -> Option<bool> {
    use_pose.filter(|(_, h)| h.two_handed()).map(|(r, _)| r)
}

// Trifft der Blick (Strahl ab `auge`, Laenge `weit`) die Box (Fuesse, Breite, Hoehe)?
fn ray_hits_box(auge: DVec3, blick: DVec3, weit: f64, fuesse: DVec3, (breite, hoehe): (f64, f64)) -> bool {
    let min = DVec3::new(fuesse.x - breite / 2.0, fuesse.y, fuesse.z - breite / 2.0);
    let max = DVec3::new(fuesse.x + breite / 2.0, fuesse.y + hoehe, fuesse.z + breite / 2.0);
    let (mut t0, mut t1) = (0.0f64, weit);
    for a in 0..3 {
        let (o, d, lo, hi) = (auge[a], blick[a], min[a], max[a]);
        if d.abs() < 1e-9 {
            if o < lo || o > hi {
                return false;
            }
            continue;
        }
        let (mut ta, mut tb) = ((lo - o) / d, (hi - o) / d);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return false;
        }
    }
    true
}

// Blickrichtung aus Gierung und Neigung (Grad, Vanilla-Konvention).
fn look_vector(yaw_deg: f32, pitch_deg: f32) -> DVec3 {
    let (yaw, pitch) = (f64::from(yaw_deg).to_radians(), f64::from(pitch_deg).to_radians());
    DVec3::new(-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos())
}

// Was die Haende fuer Karte und Hochhalten bedeuten: (rechts hoch, links hoch) und die Karte.
fn hand_poses(main: Option<world::HeldItem>, off: Option<world::HeldItem>, left_handed: bool) -> ((bool, bool), MapHold) {
    let name = |h: Option<world::HeldItem>| h.and_then(|h| world::item_registry::item_name(h.item_id)).unwrap_or("");
    let (haupt, neben) = (name(main), name(off));
    let haupt_rechts = !left_handed;
    let karte = if anims::is_map(haupt) {
        if off.is_none() { MapHold::Both } else { MapHold::Arm(haupt_rechts) }
    } else if anims::is_map(neben) {
        MapHold::Arm(!haupt_rechts)
    } else {
        MapHold::None
    };
    let (haupt_hoch, neben_hoch) = (anims::held_up(haupt), anims::held_up(neben));
    let hoch = if haupt_rechts { (haupt_hoch, neben_hoch) } else { (neben_hoch, haupt_hoch) };
    (hoch, karte)
}

impl App {
    // Die eingeschalteten Animationen (Video-Tab, Abschnitt Animationen); mit dem
    // Hauptschalter aus keine.
    pub(crate) const fn anim_features(&self) -> u32 {
        if self.settings.anims_enabled { self.settings.anim_flags } else { 0 }
    }

    // Modified View Model: eingeschaltet, oder gerade beim Item-Ansehen -- das braucht die
    // haltende Hand auch, wenn es sonst aus ist.
    pub(crate) const fn view_model_on(&self) -> bool {
        self.settings.modified_view_model || self.own_inspect.is_some()
    }

    // Haelt sich jemand mit den Fuessen an dieser Stelle an einer Leiter fest? Dann die Gierung,
    // mit der er zur Wand schaut. Ranken zaehlen nicht (dort gibt es keine Wand), eine offene
    // Falltuer ueber einer gleich ausgerichteten Leiter schon (s. Physics::is_climbing).
    pub(crate) fn ladder_yaw(&self, feet: DVec3, on_ground: bool) -> Option<f32> {
        if on_ground {
            return None;
        }
        let (x, y, z) = (feet.x.floor() as i32, feet.y.floor() as i32, feet.z.floor() as i32);
        let state = self.world.block_at(x, y, z);
        let name = world::block_registry::block_name(state)?;
        let leiter = if name == "ladder" {
            state
        } else if name.ends_with("trapdoor") && world::state_props::property(state, "open") == Some("true") {
            let unten = self.world.block_at(x, y - 1, z);
            (world::block_registry::block_name(unten) == Some("ladder")
                && world::state_props::property(unten, "facing") == world::state_props::property(state, "facing"))
            .then_some(unten)?
        } else {
            return None;
        };
        // Die Leiter zeigt von der Wand weg, der Kletternde schaut zur Wand.
        match world::state_props::property(leiter, "facing")? {
            "north" => Some(0.0),
            "east" => Some(90.0),
            "south" => Some(180.0),
            "west" => Some(270.0),
            _ => None,
        }
    }

    // Steht ein fremder Spieler auf festem Boden? Der Server sagt es uns nicht, also: Fuesse
    // auf einer Blockkante und darunter etwas, das kein Luftblock und keine Leiter ist.
    pub(crate) fn remote_on_ground(&self, feet: DVec3) -> bool {
        if feet.y - feet.y.floor() > 1.0e-3 {
            return false;
        }
        let unten = self.world.block_at(feet.x.floor() as i32, feet.y.floor() as i32 - 1, feet.z.floor() as i32);
        unten != world::STATE_AIR && world::block_registry::block_name(unten).is_some_and(|n| !physics::is_climbable(n))
    }

    // Steckt die Figur im Pulverschnee (Fuesse oder Brust)?
    pub(crate) fn in_powder_snow(&self, feet: DVec3) -> bool {
        (0..2).any(|dy| {
            let s = self.world.block_at(feet.x.floor() as i32, feet.y.floor() as i32 + dy, feet.z.floor() as i32);
            world::block_registry::block_name(s) == Some("powder_snow")
        })
    }

    // Lenkt diese Figur ein Pferd oder Boot? (Pferd: dessen Laufphase; Boot: Ruderzeiten.)
    fn ridden(&self, vehicle: i32, seat: usize, progress: f32) -> (Option<f32>, Option<[f32; 2]>) {
        if seat != 0 {
            return (None, None);
        }
        let Some(v) = self.entities.get(vehicle) else { return (None, None) };
        let Some(name) = world::entity_type_name(v.entity_type) else { return (None, None) };
        if world::mob_catalog::is_boat(name) {
            return (None, Some([v.rowing_time(0, progress), v.rowing_time(1, progress)]));
        }
        if world::riding::steering_kind(name) == Some(world::riding::Steering::Horse) && v.saddled {
            return (Some(v.limb_swing(progress).0), None);
        }
        (None, None)
    }

    // Eingaben fuer eine fremde Spielerfigur. `taken`: Arme mit Schild oder Speer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn remote_anim_input(
        &self,
        id: i32,
        e: &world::Entity,
        progress: f32,
        pitch_deg: f32,
        limb: f32,
        swinging: Option<bool>,
        taken: (bool, bool),
        ladder: Option<f64>,
        glide: Option<&world::Glide>,
        lean: &world::Lean,
        use_pose: Option<(bool, world::entity_model::UsePose)>,
    ) -> anims::Input {
        let features = self.anim_features();
        let (x, y, z) = e.render_pos(progress);
        let feet = DVec3::new(x, y, z);
        let eating = e
            .used_item()
            .filter(|(h, _)| world::entity::held_eaten_or_drunk(h))
            .and_then(|(_, ticks)| Some((e.using_item_right_arm()?, ticks as f32 + progress)));
        let (hold_up, map) = hand_poses(e.main_hand, e.off_hand, e.left_handed);
        let (horse, boat) = self.entities.vehicle_seat(id).map_or((None, None), |(v, seat)| self.ridden(v, seat.index, progress));
        anims::Input {
            features,
            pitch_deg,
            age: e.age_ticks(progress),
            limb,
            swinging,
            taken,
            eating,
            hold_up,
            map,
            ladder,
            crawling: lean.amount > 0.0 && !lean.in_water,
            burning: e.on_fire,
            freezing: features & anims::FREEZING != 0 && self.in_powder_snow(feet),
            swap: e.swap.progress(progress),
            glide_velocity: glide.map(|g| g.velocity),
            horse,
            boat,
            sleeping: e.pose == world::entity::POSE_SLEEPING,
            petting: self.anim_petting.contains(&id).then_some(!e.left_handed),
            hugging: self.anim_hugging.contains(&id),
            sprinting: e.sprinting,
            fall_speed: (ladder.is_none() && !e.gliding && !lean.in_water && !self.remote_on_ground(feet))
                .then(|| (e.last_drop() / 3.5) as f32)
                .filter(|&v| v > FALL_SHOWN),
            bow_arm: bow_arm(use_pose),
            crossbow_arm: crossbow_arm(use_pose),
            ladder_free: (ladder.is_some() && e.sneaking() && e.last_drop() == 0.0).then_some(!e.left_handed),
            vanilla_only: (lean.amount > 0.0 && lean.in_water)
                || e.living_flags & protocol::packets::LIVING_FLAG_USING_RIPTIDE != 0
                || self.death_time(id).is_some(),
        }
    }

    // Schwimm-/Kriech-Neigung der eigenen Figur fuers Bild: zwischen den Ticks interpoliert wie
    // Vanillas getLeaningPitch(tickDelta). Der rohe Tick-Wert sprang 20-mal pro Sekunde um
    // 0.09 -- die Figur legte sich in Stufen hin statt weich.
    pub(crate) fn own_lean(&self, tick_progress: f32) -> f32 {
        let t = tick_progress.clamp(0.0, 1.0);
        self.own_last_leaning_pitch + (self.own_leaning_pitch - self.own_last_leaning_pitch) * t
    }

    // Fluggeschwindigkeit der eigenen Figur fuers Bild, zwischen den Ticks interpoliert. Der
    // rohe Tick-Wert liess die Drehung im Gleitflug 20-mal pro Sekunde springen.
    pub(crate) fn own_glide_velocity(&self, tick_progress: f32) -> [f32; 3] {
        let v = self.own_last_tick_vel.lerp(self.own_tick_vel, f64::from(tick_progress.clamp(0.0, 1.0)));
        [v.x as f32, v.y as f32, v.z as f32]
    }

    // Eingaben fuer die eigene Figur (dritte Person).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn own_anim_input(
        &self,
        tick_progress: f32,
        pitch_deg: f32,
        limb: f32,
        swinging: Option<bool>,
        taken: (bool, bool),
        ladder: Option<f64>,
        glide: Option<&world::Glide>,
        use_pose: Option<(bool, world::entity_model::UsePose)>,
    ) -> anims::Input {
        let features = self.anim_features();
        let (main, off) = self.own_hand_items();
        let benutzt = self.own_using_item_right_arm();
        let eating = benutzt.and_then(|rechts| {
            let hand = if self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
            let item = world::HeldItem::from_stack(self.own_hand_stack(hand)?);
            world::entity::held_eaten_or_drunk(&item).then_some((rechts, self.own_use_ticks as f32 + tick_progress))
        });
        let (hold_up, map) = hand_poses(main, off, false);
        let (horse, boat) = self.own_ride.as_ref().map_or((None, None), |r| self.ridden(r.vehicle, r.seat.index, tick_progress));
        let lean = self.own_lean(tick_progress);
        anims::Input {
            features,
            pitch_deg,
            age: self.own_age_ticks + tick_progress,
            limb,
            swinging,
            taken,
            eating,
            hold_up,
            map,
            ladder,
            crawling: lean > 0.0 && !self.physics.touching_water,
            burning: self.own_on_fire && !self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_FIRE_RESISTANCE),
            freezing: features & anims::FREEZING != 0 && self.in_powder_snow(self.physics.pos),
            swap: self.own_swap.progress(tick_progress),
            glide_velocity: glide.map(|g| g.velocity),
            horse,
            boat,
            sleeping: self.sleeping_at.is_some(),
            petting: self.own_entity_id.is_some_and(|id| self.anim_petting.contains(&id)).then_some(true),
            hugging: self.own_entity_id.is_some_and(|id| self.anim_hugging.contains(&id)),
            sprinting: self.physics.sprinting,
            // Wie die Mod: bei der eigenen Figur erst ab 3 Bloecken Fallhoehe.
            fall_speed: (ladder.is_none() && !self.physics.gliding && !self.physics.on_ground && !self.physics.abilities.flying && !self.physics.touching_water && self.physics.fall_distance > 3.0)
                .then(|| (self.physics.vel.length_squared() / 11.0) as f32)
                .filter(|&v| v > FALL_SHOWN),
            bow_arm: bow_arm(use_pose),
            crossbow_arm: crossbow_arm(use_pose),
            ladder_free: (ladder.is_some() && self.physics.crouching && self.physics.vel.y.abs() < 0.1).then_some(true),
            vanilla_only: lean > 0.0 && self.physics.touching_water,
        }
    }

    // Glaettung je Figur (s. anims::Smoothing). `own`: die eigene Figur statt Entity `id`.
    // Schild-Arme springen sofort.
    pub(crate) fn smooth_anims(&self, id: Option<i32>, pose: &mut world::Pose, schild: (bool, bool)) {
        if self.anim_features() & anims::SMOOTH == 0 {
            return;
        }
        let jetzt = std::time::Instant::now();
        let mut tabelle = self.anim_smoothing.borrow_mut();
        let eintrag = tabelle.entry(id.unwrap_or(OWN_SMOOTHING_KEY)).or_insert((jetzt, anims::Smoothing::default()));
        let dt = jetzt.duration_since(eintrag.0).as_secs_f32();
        eintrag.0 = jetzt;
        eintrag.1.apply(pose, dt, [schild.0, schild.1, false, false]);
    }

    // Mehr Animationen, BODY_TURN: beim Essen und Blocken holt der eigene Koerper die Haelfte des
    // Wegs zum Kopf auf. NACH own_body_yaw.tick aufrufen: frueher stand das in tick_anims, das
    // VOR dem Koerper-Tick laeuft -- die halbe Drehung lag dann vor dem last_yaw-Schnappschuss,
    // wurde nie interpoliert, und die Figur sprang beim Essen jeden Tick um bis zu 20 Grad.
    pub(crate) fn turn_own_body_on_use(&mut self) {
        if self.anim_features() & anims::BODY_TURN == 0 {
            return;
        }
        let (haupt, neben) = self.own_hand_items();
        let item = if self.using_offhand { neben } else { haupt };
        if self.using_item && item.is_some_and(|h| world::entity::turns_body_to_head(&h)) {
            self.own_body_yaw.turn_to_head();
        }
    }

    // Je Tick: wer schleichend ein Haustier streichelt oder einen schleichenden Spieler umarmt
    // (Blick trifft es auf einem Block Entfernung, kaum Hoehenunterschied). Gestreichelte Tiere
    // zeigen ab und zu Herzchen.
    pub(crate) fn tick_anims(&mut self) {
        let an = self.anim_features();
        self.entities.body_turn_on_use = an & anims::BODY_TURN != 0;
        self.anim_petting.clear();
        self.anim_hugging.clear();
        if an & (anims::PET | anims::HUG) == 0 {
            return;
        }
        // (Id, Fuesse, Augenhoehe, Gierung, Neigung) aller schleichenden Spieler, die eigene Figur
        // eingeschlossen.
        let mut schleicher: Vec<(i32, DVec3, f64, f32, f32)> = self
            .entities
            .iter()
            .filter(|(_, e)| world::is_player(e.entity_type) && e.sneaking())
            .map(|(&id, e)| (id, DVec3::new(e.pos.0, e.pos.1, e.pos.2), e.eye_height(), e.yaw_deg, e.pitch_deg))
            .collect();
        if let (Some(id), true) = (self.own_entity_id, self.physics.crouching) {
            schleicher.push((id, self.physics.pos, f64::from(self.physics.eye_height()), self.camera.yaw, self.look_pitch_deg()));
        }
        let mut herzen: Vec<DVec3> = Vec::new();
        for (id, fuesse, augen, yaw, pitch) in schleicher {
            let auge = fuesse + DVec3::new(0.0, augen, 0.0);
            let blick = look_vector(yaw, pitch);
            for (&anderer, e) in self.entities.iter() {
                if anderer == id {
                    continue;
                }
                let pos = DVec3::new(e.pos.0, e.pos.1, e.pos.2);
                if pos.distance_squared(auge) > 9.0 || !ray_hits_box(auge, blick, 1.0, pos, e.hit_size()) {
                    continue;
                }
                let name = world::entity_type_name(e.entity_type);
                if an & anims::PET != 0 && matches!(name, Some("wolf" | "cat")) && (pos.y - fuesse.y).abs() < 0.6 {
                    self.anim_petting.insert(id);
                    herzen.push(pos);
                    break;
                }
                if an & anims::HUG != 0 && world::is_player(e.entity_type) && e.sneaking() && (pos.y - fuesse.y).abs() < 0.3 {
                    self.anim_hugging.insert(id);
                    break;
                }
            }
        }
        if herzen.is_empty() {
            return;
        }
        let herz = protocol::particle::ParticleEffect {
            type_id: protocol::particle::PARTICLE_NAMES.iter().position(|n| *n == "heart").unwrap_or(0) as i32,
            options: protocol::particle::ParticleOptions::None,
        };
        self.with_particles(|p, ctx, _| {
            for pos in herzen {
                if p.rng().next_double() >= PET_HEART_CHANCE {
                    continue;
                }
                // Animal.handleEntityEvent(18): sieben Herzen um das Tier.
                for _ in 0..7 {
                    let r = p.rng();
                    let (dx, dz, dy) = (r.next_double() - 0.5, r.next_double() - 0.5, r.next_double() * 0.8);
                    let v = DVec3::new(r.next_gaussian() * 0.02, r.next_gaussian() * 0.02, r.next_gaussian() * 0.02);
                    p.add_world_particle(&herz, pos + DVec3::new(dx, 0.5 + dy, dz), v, ctx);
                }
            }
        });
    }

    // Was die Haende einer Spielerfigur zeigen (rechts, links): im Bett nichts, und beim Bogen-
    // spannen und mit der Armbrust im Anschlag nicht das Item der anderen Hand -- die haelt ja
    // den Bogen bzw. die Armbrust mit.
    pub(crate) const fn shown_hands(
        &self,
        arms: (Option<world::HeldItem>, Option<world::HeldItem>),
        use_pose: Option<(bool, world::entity_model::UsePose)>,
        sleeping: bool,
    ) -> (Option<world::HeldItem>, Option<world::HeldItem>) {
        use world::entity_model::UsePose;
        let an = self.anim_features();
        if sleeping && an & anims::BED != 0 {
            return (None, None);
        }
        match use_pose {
            Some((rechts, UsePose::Bow | UsePose::CrossbowCharge { .. } | UsePose::CrossbowHold)) if an & anims::HIDE_OFFHAND != 0 => {
                if rechts { (arms.0, None) } else { (None, arms.1) }
            }
            _ => arms,
        }
    }

    // Was an Haenden und Ruecken anders aussieht als in Vanilla: Karten zeigen ihren Inhalt,
    // Laternen pendeln als Block an der Hand, das zuletzt gehaltene Schwert steckt am Ruecken.
    // Liefert die Haende (rechts, links), deren Item danach noch normal zu zeichnen ist.
    // `figur`: Schluessel der Figur (Entity-Id, eigene i32::MIN), `blick`: Gierung/Neigung,
    // `tick`: ganzzahliger Tick der Figur und Zwischentick-Anteil.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_hand_extras(
        &self,
        figur: i32,
        haende: (Option<world::HeldItem>, Option<world::HeldItem>),
        haupt_rechts: bool,
        blick: (f32, f32),
        tick: (i64, f32),
        schwert: Option<world::HeldItem>,
        ruestung: &[Option<world::HeldItem>; 4],
        model: &world::entity_model::Model,
        pose: &world::Pose,
        base: world::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) -> (Option<world::HeldItem>, Option<world::HeldItem>) {
        use world::entity_model::{part_transform, rotation_x_deg, rotation_z_deg, Affine};
        let an = self.anim_features();
        let mut rest = haende;
        for rechts in [true, false] {
            let Some(item) = (if rechts { haende.0 } else { haende.1 }) else { continue };
            let name = world::item_registry::item_name(item.item_id).unwrap_or("");
            let andere_leer = (if rechts { haende.1 } else { haende.0 }).is_none();
            let Some(arm) = part_transform(model, pose, base, if rechts { "right_arm" } else { "left_arm" }) else { continue };
            let s = if rechts { 1.0 } else { -1.0 };
            if let (true, true, Some(id)) = (an & anims::MAP != 0, anims::is_map(name), item.map_id) {
                // Die Karte flach vor der Hand, mit beiden Haenden gross, mit einer klein.
                let haupt = rechts == haupt_rechts;
                let hand = if haupt {
                    arm.then(rotation_x_deg(-90.0))
                        .then(Affine::rotation_y_deg(205.0))
                        .then(rotation_z_deg(10.0))
                        .then(Affine::translation([s / 16.0, 0.09 + if andere_leer { 0.15 } else { 0.0 }, -0.625]))
                } else {
                    arm.then(rotation_x_deg(-90.0)).then(Affine::rotation_y_deg(200.0)).then(Affine::translation([s / 16.0, 0.125, -0.625]))
                };
                let karte = if !haupt || !andere_leer {
                    hand.then(Affine::rotation_y_deg(160.0))
                        .then(rotation_z_deg(180.0))
                        .then(Affine::scale([0.38; 3]))
                        .then(Affine::translation([-0.1, -1.2, 0.0]))
                        .then(Affine::scale([0.009_812_5; 3]))
                } else if !haupt_rechts {
                    hand.then(Affine::rotation_y_deg(154.5))
                        .then(rotation_z_deg(166.5))
                        .then(Affine::scale([0.38; 3]))
                        .then(Affine::translation([0.585, -1.225, 0.15]))
                        .then(Affine::scale([0.013_812_5; 3]))
                } else {
                    hand.then(Affine::rotation_y_deg(155.0))
                        .then(rotation_z_deg(213.5))
                        .then(Affine::scale([0.38; 3]))
                        .then(Affine::translation([-0.955, -1.8, 0.0]))
                        .then(Affine::scale([0.013_812_5; 3]))
                };
                self.emit_map_two_sided(id, karte, out);
            } else if let (true, Some(modell)) = (an & anims::LANTERN != 0, anims::lantern_block_model(name)) {
                let aufhaengung = arm
                    .then(Affine::scale([0.6; 3]))
                    .then(Affine::translation([if rechts { -0.5 } else { -0.45 }, 0.4, -0.3]))
                    .then(Affine::translation([0.5, 0.6, 0.5]));
                let p = aufhaengung.apply([0.0; 3]);
                let welt = [self.camera.position.x + f64::from(p[0]), self.camera.position.y + f64::from(p[1]), self.camera.position.z + f64::from(p[2])];
                let (kipp_x, kipp_z) = self.lantern_swing.borrow_mut().entry((figur, rechts)).or_default().update(welt, tick.0, tick.1, blick.0, blick.1);
                let m = aufhaengung
                    .then(rotation_x_deg(-90.0))
                    .then(rotation_x_deg(kipp_x))
                    .then(rotation_z_deg(kipp_z))
                    .then(Affine::translation([-0.5, -0.6, -0.5]));
                self.held_items.emit_model_raw(&modell, m, ENTITY_BRIGHTNESS, out);
            } else {
                continue;
            }
            if rechts {
                rest.0 = None;
            } else {
                rest.1 = None;
            }
        }
        // Das Schwert am Ruecken, ueber der Huefte der Gegenseite -- nur, wenn es gerade in
        // keiner Hand liegt.
        let in_der_hand = |h: world::HeldItem| [haende.0, haende.1].into_iter().flatten().any(|x| x.item_id == h.item_id);
        if let Some(sword) = schwert.filter(|h| an & anims::SWORD != 0 && !in_der_hand(*h)) {
            if let Some(koerper) = part_transform(model, pose, base, "body") {
                let elytra = |h: &world::HeldItem| world::item_registry::item_name(h.item_id) == Some("elytra");
                let ruestung_an = ruestung[world::armor::ArmorSlot::Legs.index()].is_some()
                    || ruestung[world::armor::ArmorSlot::Chest.index()].is_some_and(|h| !elytra(&h));
                let x = if ruestung_an { 0.3 } else { 0.28 } * if haupt_rechts { 1.0 } else { -1.0 };
                let m = koerper.then(Affine::translation([x, 0.85, 0.25])).then(rotation_x_deg(-80.0)).then(Affine::rotation_y_deg(180.0));
                let start = out.len();
                self.held_items.set_skull(None);
                self.held_items.emit_fixed(sword.model_id, m, ENTITY_BRIGHTNESS, out);
                if sword.enchanted {
                    self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                        self.held_items.emit_fixed(sword.model_id, m, 1.0, l);
                    });
                }
            }
        }
        rest
    }

    // Eine Karte in der Hand einer Figur: Hintergrund und Bild, von beiden Seiten sichtbar.
    fn emit_map_two_sided(&self, id: i32, m: world::Affine, out: &mut Vec<world::BlockVertex>) {
        let start = out.len();
        let grund = if self.maps.get(id).is_some() { "map/map_background_checkerboard" } else { "map/map_background" };
        if let Some([u0, v0, u1, v1]) = self.entity_atlas.rect(grund) {
            let ecke = |x: f32, y: f32, u: f32, v: f32| world::BlockVertex { position: m.apply([x, y, 0.0]), color: [ENTITY_BRIGHTNESS; 3], uv: [u, v] };
            let (a, b, c, d) = (ecke(-7.0, 135.0, u0, v1), ecke(135.0, 135.0, u1, v1), ecke(135.0, -7.0, u1, v0), ecke(-7.0, -7.0, u0, v0));
            out.extend([a, b, c, a, c, d]);
        }
        if let Some(rect) = self.map_rect(id, false) {
            push_map_quad(m, rect, ENTITY_BRIGHTNESS, out);
        }
        // Rueckseite: dieselben Dreiecke andersherum.
        let rueck: Vec<world::BlockVertex> = out[start..].iter().rev().copied().collect();
        out.extend(rueck);
    }

    // Figuren, die eine Weile nicht gezeichnet wurden, aus der Glaettung werfen.
    pub(crate) fn prune_anim_smoothing(&self) {
        let jetzt = std::time::Instant::now();
        self.anim_smoothing.borrow_mut().retain(|_, (zuletzt, _)| jetzt.duration_since(*zuletzt) < SMOOTHING_KEEP);
    }
}

// Leichte Items: beim Ansehen immer nah an die Augen, nie jongliert.
const LIGHT_ITEMS: &[&str] = &["wheat_seeds", "beetroot_seeds", "melon_seeds", "pumpkin_seeds", "torchflower_seeds", "pitcher_pod"];

// Schwer: alles mit Haltbarkeit (max_damage), auch unzerbrechlich.
fn heavy(stack: &protocol::item::ItemStack, name: &str) -> bool {
    if world::item_components::loaded() {
        return world::item_components::Components::of(stack).max_damage() > 0;
    }
    stack.max_damage.or_else(|| crate::default_max_damage(name)).is_some()
}

// Item ansehen (s. world::item_inspect): nur die eigene Ego-Hand.
impl App {
    // Welche Ansehen-Animation die Hand gerade haette. Die Nebenhand macht nur beim Schild mit.
    // Die eigene Animation je Item (wie in CS) nur mit Haltbarkeit; alles andere wird jongliert
    // (s. juggle_kind).
    fn inspect_kind(&self, hand: i32) -> Option<world::item_inspect::Kind> {
        let Some(stack) = self.own_hand_stack(hand).filter(|s| s.item_id != 0) else {
            return (hand == protocol::packets::HAND_MAIN).then_some(world::item_inspect::Kind::EmptyHand);
        };
        let name = world::item_registry::item_name(stack.item_id)?;
        let kind = world::item_inspect::kind(name, world::block_registry::is_block(name), world::item_behavior::eaten_or_drunk(stack))?;
        if hand != protocol::packets::HAND_MAIN {
            return (kind == world::item_inspect::Kind::Shield).then_some(kind);
        }
        // Traenke und Wurfitems werden hochgeworfen statt jongliert.
        if heavy(stack, name) || kind == world::item_inspect::Kind::Throwable {
            return Some(kind);
        }
        Some(self.juggle_kind(stack, name))
    }

    // Ohne Haltbarkeit: leichte Items (Samen) und solche, bei denen die andere Hand belegt ist,
    // nah an die Augen. Sonst mit allen Items beider Haende jonglieren -- ab 3 mit dreien, mit 2
    // mit zweien, eins wird um den Koerper gereicht. Nie im Wasser und nie mit einem schweren
    // Item (mit Haltbarkeit) in der Nebenhand.
    fn juggle_kind(&self, main: &protocol::item::ItemStack, name: &str) -> world::item_inspect::Kind {
        use world::item_inspect::Kind;
        use world::juggle::Trick;
        if LIGHT_ITEMS.contains(&name) {
            return Kind::CloseLook;
        }
        let neben = self.own_hand_stack(protocol::packets::HAND_OFF);
        let neben_schwer = neben.is_some_and(|s| heavy(s, world::item_registry::item_name(s.item_id).unwrap_or("")));
        let im_wasser = self.physics.touching_water;
        let anzahl = main.count + if neben_schwer { 0 } else { neben.map_or(0, |s| s.count) };
        match anzahl {
            _ if im_wasser || neben_schwer => {}
            3.. => return Kind::Juggle(Trick::Three),
            2 => return Kind::Juggle(Trick::Two),
            _ => {}
        }
        Kind::CloseLook
    }

    // Der laufende Trick und wie lange er schon laeuft (Ticks).
    pub(crate) fn juggle_progress(&self, tick_progress: f32) -> Option<(world::juggle::Trick, f32)> {
        let (_, ticks) = self.own_inspect?;
        match self.inspect_kind(protocol::packets::HAND_MAIN)? {
            world::item_inspect::Kind::Juggle(trick) => Some((trick, ticks as f32 + tick_progress)),
            _ => None,
        }
    }

    // Welche Items fliegen, in der Reihenfolge von world::juggle (Item i beginnt in der Hand
    // seines ersten Wurfs): links das der Nebenhand, solange sie welche hat, sonst Haupthand.
    pub(crate) fn juggle_items(&self, trick: world::juggle::Trick) -> Vec<world::HeldItem> {
        let (main, off) = self.own_hand_items();
        let Some(main) = main else { return Vec::new() };
        let mut rest_main = self.own_hand_stack(protocol::packets::HAND_MAIN).map_or(0, |s| s.count);
        let mut rest_off = self.own_hand_stack(protocol::packets::HAND_OFF).map_or(0, |s| s.count);
        let state = world::juggle::state(trick, 0.0);
        let mut items = vec![main; trick.balls()];
        let mut nimm = |links: bool| {
            if (links && rest_off > 0) || rest_main <= 0 {
                rest_off -= 1;
                off.unwrap_or(main)
            } else {
                rest_main -= 1;
                main
            }
        };
        for links in [true, false] {
            for (i, h) in state.balls.iter().enumerate() {
                if *h == world::juggle::Holder::Hand(usize::from(links)) {
                    items[i] = nimm(links);
                }
            }
        }
        items
    }

    // Ego-Ansicht beim Jonglieren: beide Arme an ihren Faeusten, die Items an ihren Stellen.
    pub(crate) fn emit_juggle_first_person(&self, trick: world::juggle::Trick, t: f32, sway: world::Affine, out: &mut Vec<world::BlockVertex>) {
        let state = world::juggle::state(trick, t);
        if !self.own_invisible {
            let (_, tex) = self.own_hand_model_and_texture();
            for (i, faust) in state.hands.iter().enumerate() {
                let right = i == 0;
                // Auf dem Bild leicht nach innen gelehnt wie die leere Hand, egal wo im Bild.
                let arm = sway.then(world::item_inspect::arm_on_screen(*faust, right, world::juggle::ARM_SCREEN_LEAN_DEG)).then(world::first_person_arm_transform(right, 0.0, 0.0));
                let modell = world::player_arm_model(self.own_slim(), right);
                world::emit_entity_model_two_sided(&modell, &world::first_person_arm_pose(right), arm, tex, ENTITY_BRIGHTNESS, out);
            }
        }
        // Zur Kamera gedreht wie im Inventar, so gross wie in der Hand, die Mitte an der Stelle
        // des Items.
        // Im Flug dreht es sich um seine Mitte (s. juggle::State::ball_spin).
        for ((item, p), (achse, winkel)) in self.juggle_items(trick).into_iter().zip(&state.ball_pos).zip(&state.ball_spin) {
            let Some((c, k)) = self.held_items.gui_center_and_hand_scale(item.model_id) else { continue };
            let base = sway
                .then(world::Affine::translation(*p))
                .then(world::Affine::rotation_axis_angle(*winkel, *achse))
                .then(world::Affine::scale([k; 3]))
                .then(world::Affine::translation([-c[0], -c[1], -c[2]]));
            self.held_items.emit_display(item.model_id, item.potion_color, base, 6, ENTITY_BRIGHTNESS, out);
        }
    }

    // Dritte Person beim Jonglieren: die Armwinkel setzen; zurueck kommt, wo die Items in der
    // Welt sind (s. emit_juggle_third_person).
    pub(crate) fn juggle_third_person(&self, tick_progress: f32, pose: &mut world::Pose) -> Option<(world::juggle::Trick, f32)> {
        let (trick, t) = self.juggle_progress(tick_progress)?;
        let arme = world::juggle::third_person_arms(trick, t);
        // Aus der Haltung hinein wie die Ego-Haende (s. juggle::RAMP).
        let f = (t / world::juggle::RAMP).clamp(0.0, 1.0);
        for (teil, arm) in ["right_arm", "left_arm"].into_iter().zip(arme) {
            let jetzt = pose.get(teil).rotation;
            pose.rotate(teil, std::array::from_fn(|i| jetzt[i] + (arm[i] - jetzt[i]) * f));
        }
        Some((trick, t))
    }

    pub(crate) fn emit_juggle_third_person(
        &self,
        (trick, t): (world::juggle::Trick, f32),
        model: &world::Model,
        pose: &world::Pose,
        base: world::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) {
        let state = world::juggle::state(trick, t);
        // Die Hand am Ende des Arm-Quaders (Modellpixel, s. held_item::hand_pre_transform).
        let hand = |i: usize| {
            let (teil, x) = if i == 0 { ("right_arm", -1.0) } else { ("left_arm", 1.0) };
            world::entity_model::part_transform(model, pose, base, teil).map(|m| m.apply([x / 16.0, 10.0 / 16.0, 0.0]))
        };
        let (Some(rechts), Some(links)) = (hand(0), hand(1)) else { return };
        let haende = [rechts, links];
        // Ein Bogen in Ego-Einheiten ist hier etwas hoeher, die Figur steht weiter weg.
        let ueber = [0.0, 0.12, 0.0];
        // So gross und so gedreht wie in der Hand (Lage des Modells, s. held_item::hand_pre_transform),
        // die Mitte an der Stelle des Items.
        let lage = world::Affine { m: base.m, t: [0.0; 3] }.then(world::rotation_x_deg(-90.0)).then(world::Affine::rotation_y_deg(180.0));
        for ((item, h), (achse, winkel)) in self.juggle_items(trick).into_iter().zip(&state.balls).zip(&state.ball_spin) {
            let p = match *h {
                world::juggle::Holder::Hand(i) => haende[i],
                world::juggle::Holder::Flight { from, to, u, apex } => {
                    let (a, b) = (haende[from], haende[to]);
                    let p: [f32; 3] = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * u);
                    [p[0], p[1] + 1.6 * apex * 4.0 * u * (1.0 - u), p[2]]
                }
            };
            let p = [p[0] + ueber[0], p[1] + ueber[1], p[2] + ueber[2]];
            // Im Flug um die eigene Mitte gedreht wie in der Ego-Ansicht.
            let lage = world::Affine::rotation_axis_angle(*winkel, *achse).then(lage);
            let c = lage.apply(self.held_items.third_person_center(item.model_id).unwrap_or([0.0; 3]));
            let m = world::Affine { m: lage.m, t: [p[0] - c[0], p[1] - c[1], p[2] - c[2]] };
            self.held_items.emit_display(item.model_id, item.potion_color, m, 2, ENTITY_BRIGHTNESS, out);
        }
    }

    fn inspect_ticks(&self) -> f32 {
        [protocol::packets::HAND_MAIN, protocol::packets::HAND_OFF].into_iter().filter_map(|h| self.inspect_kind(h)).map(world::item_inspect::Kind::ticks).fold(0.0, f32::max)
    }

    // Tastendruck: von vorn beginnen (auch mitten in einer laufenden). Jonglieren laeuft bis zur
    // naechsten Aktion, ein zweiter Druck beendet es.
    pub(crate) fn start_inspect(&mut self) {
        if self.juggle_progress(0.0).is_some() {
            self.own_inspect = None;
            return;
        }
        if self.using_item || self.inspect_ticks() == 0.0 {
            return;
        }
        let haupt = self.own_hand_items().0.map_or(0, |h| h.item_id);
        self.own_inspect = Some((haupt, 0));
        self.own_inspect_kind = self.inspect_kind(protocol::packets::HAND_MAIN);
    }

    // Je Tick: weiterzaehlen, zu Ende oder abbrechen -- bei Schlag, Benutzung, anderem Item oder
    // wenn sich die Animation aendert (ins Wasser, anderes in der Nebenhand).
    pub(crate) fn tick_inspect(&mut self) {
        let Some((item, ticks)) = self.own_inspect else { return };
        let haupt = self.own_hand_items().0.map_or(0, |h| h.item_id);
        let gleich = self.inspect_kind(protocol::packets::HAND_MAIN) == self.own_inspect_kind;
        let weiter = item == haupt && gleich && self.own_swing_ticks == 0 && !self.using_item && ((ticks + 1) as f32) < self.inspect_ticks();
        self.own_inspect = weiter.then_some((item, ticks + 1));
    }

    // Laeuft fuer diese Hand gerade ein Ansehen: welches und wie weit (0..1)?
    pub(crate) fn inspect_progress(&self, hand: i32, tick_progress: f32) -> Option<(world::item_inspect::Kind, f32)> {
        let ((_, ticks), kind) = (self.own_inspect?, self.inspect_kind(hand)?);
        let t = (ticks as f32 + tick_progress) / kind.ticks();
        (t < 1.0).then_some((kind, t))
    }

    // Wie inspect_pose, fuer ein gehaltenes Item mit der Pose `ruhe` (ohne Schlag und Anlegen):
    // flache Items (Kind::turns_around_center) drehen sich um ihre eigene Mitte.
    pub(crate) fn inspect_pose_item(&self, item: world::HeldItem, hand: i32, ruhe: world::Affine, tick_progress: f32) -> world::entity_model::Affine {
        let Some((kind, t)) = self.inspect_progress(hand, tick_progress) else { return world::entity_model::Affine::IDENTITY };
        let right = hand == protocol::packets::HAND_MAIN;
        let s = if right { 1.0 } else { -1.0 };
        let sicht = |m: [f32; 3]| self.item_shift(item, right, 1.0).then(ruhe).apply([s * m[0], m[1], m[2]]);
        if kind.flips() {
            if let (Some(g), Some(m)) = (self.holding_grip(item, hand, tick_progress), self.holding_center(item, hand, tick_progress)) {
                return world::item_inspect::tool_toss_transform(kind, t, right, sicht(g), sicht(m));
            }
        }
        let mitte = kind.turns_around_center().then(|| self.holding_center(item, hand, tick_progress)).flatten().map(sicht);
        if let (world::item_inspect::Kind::CloseLook, Some(m)) = (kind, mitte) {
            if let Some(n) = self.held_items.first_person_face_normal(item.model_id, right) {
                // Die Normale wie ein Punkt durch dieselbe Kette, als Richtung.
                let (a, b) = (sicht([0.0; 3]), sicht(n));
                return world::item_inspect::close_look_transform(t, right, m, [b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
            }
        }
        world::item_inspect::transform_at(kind, t, right, mitte)
    }

    // Vor die gewohnte Kette dieser Hand (s. build_hand_vertices); ohne Ansehen die Einheit.
    pub(crate) fn inspect_pose(&self, hand: i32, tick_progress: f32) -> world::entity_model::Affine {
        let ruhe = world::entity_model::Affine::IDENTITY;
        let (Some((_, ticks)), Some(kind)) = (self.own_inspect, self.inspect_kind(hand)) else { return ruhe };
        let t = (ticks as f32 + tick_progress) / kind.ticks();
        if t >= 1.0 {
            return ruhe;
        }
        world::item_inspect::transform(kind, t, hand == protocol::packets::HAND_MAIN)
    }

    // Die Mitte des Items dieser Hand, am selben Modell wie beim Zeichnen (s. holding_grip).
    pub(crate) fn holding_center(&self, item: world::HeldItem, hand: i32, tick_progress: f32) -> Option<[f32; 3]> {
        let right = hand == protocol::packets::HAND_MAIN;
        let benutzt = self.own_using_item_right_arm() == Some(right);
        match self.hand_model_of(hand, benutzt, self.hand_use_ticks(tick_progress)).filter(|m| self.held_items.has_model(m)) {
            Some(m) => self.held_items.first_person_center_model(&m, right),
            None => self.held_items.first_person_center(item.model_id, right),
        }
    }

    // Der Griffpunkt des Items dieser Hand im Modell, am selben Modell wie beim Zeichnen (s.
    // emit_first_person_item).
    pub(crate) fn holding_grip(&self, item: world::HeldItem, hand: i32, tick_progress: f32) -> Option<[f32; 3]> {
        let right = hand == protocol::packets::HAND_MAIN;
        let armbrust = world::item_registry::item_name(item.item_id) == Some("crossbow");
        // Die Spannstufen-Bilder des Bogens liegen deckungsgleich, nur Sehne und Wurfarme sind
        // anders -- ihr Griff aus der Kontur waere es auch. Er bleibt der des ungespannten Bogens,
        // sonst sitzen Faust und Item-Drehpunkt (s. item_turn) je Stufe woanders.
        let bogen = world::item_registry::item_name(item.item_id) == Some("bow");
        let benutzt = self.own_using_item_right_arm() == Some(right) && !bogen;
        let gewaehlt = self.hand_model_of(hand, benutzt, self.hand_use_ticks(tick_progress)).filter(|m| self.held_items.has_model(m));
        let griff = match &gewaehlt {
            Some(m) => self.held_items.first_person_grip_model(m, item.potion_color, armbrust, right),
            None => self.held_items.first_person_grip(item.model_id, item.potion_color, armbrust, right),
        };
        // Fuers Log (client.log): je Item und Modell einmal, woher der Griff kommt.
        if right {
            let schluessel = (item.model_id, gewaehlt.clone());
            if self.hand_grip_logged.borrow().as_ref() != Some(&schluessel) {
                eprintln!("[hand] item={:?} modell={:?} griff={:?}", world::item_registry::item_name(item.model_id), gewaehlt, griff);
                *self.hand_grip_logged.borrow_mut() = Some(schluessel);
            }
        }
        griff
    }

    // Hand-Debug: die von Hand eingestellte Verschiebung des Items selbst (Shift + Pfeiltasten,
    // Settings::hand_fist_items), links gespiegelt. Kommt vor die Item-Pose, nur ans Item, die Hand bleibt.
    // `anteil`: wie stark (1 = ganz, s. tuning_share).
    pub(crate) fn item_shift(&self, item: world::HeldItem, right: bool, anteil: f32) -> world::Affine {
        if !self.view_model_on() {
            return world::Affine::IDENTITY;
        }
        let w = self.hand_tuning(item, if right { protocol::packets::HAND_MAIN } else { protocol::packets::HAND_OFF });
        let s = if right { 1.0 } else { -1.0 };
        world::Affine::translation([s * w[10] as f32, w[11] as f32, w[12] as f32].map(|v| v / 1000.0 * anteil))
    }

    // Wie stark die eigene Item-Verschiebung und -Drehung gilt: beim Essen und Trinken blendet
    // sie aus, waehrend das Item zum Mund geht -- Vanillas Ess-Pose bringt es selbst vor die
    // Mitte, die Werte sind fuers Halten gestellt.
    pub(crate) fn tuning_share(&self, hand: i32, benutzt: bool, use_ticks: f32) -> f32 {
        let Some(stack) = self.own_hand_stack(hand).filter(|_| benutzt) else { return 1.0 };
        let Some(dauer) = world::item_behavior::consume_ticks(stack) else { return 1.0 };
        let dauer = dauer as f32;
        1.0 - world::entity_model::eat_lift(dauer - use_ticks + 1.0, dauer)
    }

    // Hand-Debug: die Drehung des Items selbst um seinen Griff (Alt im Item-Modus), in den
    // Sichtachsen der Ruhe-Pose `ruhe`, links gespiegelt. Kommt hinter die Item-Pose, dreht so
    // beim Schlag mit; die Hand bleibt.
    pub(crate) fn item_turn(&self, item: world::HeldItem, hand: i32, ruhe: world::Affine, anteil: f32) -> world::Affine {
        let w = self.hand_tuning(item, hand);
        if !self.view_model_on() || w[13..16] == [0; 3] {
            return world::Affine::IDENTITY;
        }
        let right = hand == protocol::packets::HAND_MAIN;
        let s = if right { 1.0 } else { -1.0 };
        let Some(griff) = self.holding_grip(item, hand, 0.0) else { return world::Affine::IDENTITY };
        let g = ruhe.apply([s * griff[0], griff[1], griff[2]]);
        let dreh = world::Affine::translation(g)
            .then(world::Affine::rotation_xyz_deg(anteil * w[13] as f32, anteil * s * w[14] as f32, anteil * s * w[15] as f32))
            .then(world::Affine::translation([-g[0], -g[1], -g[2]]));
        ruhe.inverse().then(dreh).then(ruhe)
    }

    // Hand-Debug: die Werte des gehaltenen Items so setzen, dass der Arm wie mit leerer Hand
    // liegt (s. world::item_inspect::empty_hand_tuning). Haupthand, sonst Nebenhand. Liegt er
    // schon so und `umschalten` (Ruecktaste), geht er zurueck auf den Standard ohne Werte (Lage
    // aus dem Griff des Items).
    pub(crate) fn hand_tuning_like_empty(&mut self, umschalten: bool) {
        let (main, off) = self.own_hand_items();
        let (item, hand) = match (main, off) {
            (Some(i), _) => (i, protocol::packets::HAND_MAIN),
            (None, Some(i)) => (i, protocol::packets::HAND_OFF),
            (None, None) => return,
        };
        let Some(name) = self.tuning_name(item, hand) else { return };
        let name = name.as_str();
        let Some(griff) = self.holding_grip(item, hand, 0.0) else { return };
        let right = hand == protocol::packets::HAND_MAIN;
        let s = if right { 1.0 } else { -1.0 };
        let benutzt = self.own_using_item_right_arm() == Some(right);
        let ruhe = self.first_person_item_pose(hand, right, benutzt, self.hand_use_ticks(0.0), 0.0, 0.0);
        let faust = world::item_inspect::fist_at(ruhe.apply([s * griff[0], griff[1], griff[2]]), right);
        // Die Werte gelten fuer die rechte Hand und werden links gespiegelt.
        let faust = [s * faust[0], faust[1], faust[2]];
        let mut leer = world::item_inspect::empty_hand_tuning(faust);
        let jetzt = self.settings.hand_fist_items.get(name).copied().unwrap_or_default();
        // Das verschobene und gedrehte Item bleibt, wie es ist.
        leer[10..16].copy_from_slice(&jetzt[10..16]);
        if umschalten && jetzt == leer {
            self.settings.hand_fist_items.remove(name);
        } else {
            self.settings.hand_fist_items.insert(name.to_string(), leer);
        }
        self.persist_settings();
    }

    // Ego-Ansicht: der Arm, der das Item dieser Hand haelt (s. world::item_inspect::arm_to).
    // `sway`: das Nachschwingen, `item_pose`: die Pose des Items ohne Ansehen, `ruhe`: dieselbe
    // ohne Schlag und Anlegen, `ansehen`: die Ansehen-Drehung davor. Faust und Arm sitzen wie in
    // der Ruhe; Schlag und Anlegen nehmen sie dann starr mit, damit das Item fest in der Hand
    // bleibt. Nicht beim Schild (die Hand liegt dahinter) und nicht unsichtbar.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_holding_arm(
        &self,
        item: world::HeldItem,
        sway: world::entity_model::Affine,
        ansehen: world::entity_model::Affine,
        item_pose: world::entity_model::Affine,
        ruhe: world::entity_model::Affine,
        hand: i32,
        tick_progress: f32,
        anteil: f32,
        out: &mut Vec<world::BlockVertex>,
    ) {
        let name = world::item_registry::item_name(item.item_id);
        if self.own_invisible || name == Some("shield") || !self.view_model_on() {
            return;
        }
        let right = hand == protocol::packets::HAND_MAIN;
        let Some(griff) = self.holding_grip(item, hand, tick_progress) else { return };
        let lauf = self.inspect_progress(hand, tick_progress);
        let s = if right { 1.0 } else { -1.0 };
        let punkt = [s * griff[0], griff[1], griff[2]];
        let mut griff_sicht = ansehen.then(ruhe).apply(punkt);
        // Nah ansehen: die Faust haelt das Item von unten, die eigenen Hand-Werte blenden dabei
        // aus (wie beim Essen).
        let mut werte_anteil = anteil;
        if let Some((_, t)) = lauf.filter(|(k, _)| *k == world::item_inspect::Kind::CloseLook) {
            if let Some(m) = self.holding_center(item, hand, tick_progress) {
                let w = world::item_inspect::close_look_weight(t);
                let mitte = ansehen.then(self.item_shift(item, right, 1.0)).then(ruhe).apply([s * m[0], m[1], m[2]]);
                let unter = [mitte[0], mitte[1] - world::hold::CLOSE_LOOK_FIST_BELOW * world::item_inspect::close_look_size(t), mitte[2]];
                griff_sicht = std::array::from_fn(|i| griff_sicht[i] + (unter[i] - griff_sicht[i]) * w);
                werte_anteil *= 1.0 - w;
            }
        }
        // Hochgeworfen fliegt das Item allein: die Hand bleibt unten und faengt es.
        if let Some((k, t)) = lauf.filter(|(k, _)| k.tossed()) {
            let r = ruhe.apply(punkt);
            let d = world::item_inspect::toss_hand_offset(k, t, right);
            griff_sicht = [r[0] + d[0], r[1] + d[1], r[2] + d[2]];
        }
        let mut ziel = world::item_inspect::fist_at(griff_sicht, right);
        ziel[0] += s * world::hold::EAT_FIST_RIGHT * (1.0 - anteil);
        // Je Item von Hand nachgestellt (Hand-Debug, Pfeiltasten): Verschiebung der Faust, dann
        // Drehung des Arms um sie (Alt + Pfeiltasten), um seine eigene Laenge (Alt + , .) und
        // Drehung der Schulter, aus der der Arm kommt, um die Faust (Strg + Pfeiltasten; Faust und
        // Item bleiben, der Arm verdreht sich nicht), links gespiegelt. Beim Essen ausgeblendet
        // wie die Item-Werte (`anteil`, s. tuning_share).
        let w = self.hand_tuning(item, hand).map(|v| (v as f32 * werte_anteil).round() as i32);
        // Von der Ruhe zur echten Pose, im Sichtraum nach dem Ansehen.
        let mit = ansehen.then(item_pose).then(ruhe.inverse()).then(ansehen.inverse());
        let arm = sway.then(mit).then(tuned_arm(ziel, &w, right)).then(world::first_person_arm_transform(right, 0.0, 0.0));
        let (_, tex) = self.own_hand_model_and_texture();
        let modell = world::player_arm_model(self.own_slim(), right);
        world::emit_entity_model_two_sided(&modell, &world::first_person_arm_pose(right), arm, tex, ENTITY_BRIGHTNESS, out);
    }

    // Ego-Ansicht: die Hand an der Sehne (s. string_hand), ohne Item, von der Faust der leeren
    // Hand aus mit ihren Werten gestellt.
    pub(crate) fn emit_string_arm(&self, hand: i32, name: &str, sway: world::Affine, out: &mut Vec<world::BlockVertex>) {
        if self.own_invisible {
            return;
        }
        let right = hand == protocol::packets::HAND_MAIN;
        let w = self.string_hand_tuning(name);
        let arm = sway.then(tuned_arm(world::item_inspect::rest_fist(right), &w, right)).then(world::first_person_arm_transform(right, 0.0, 0.0));
        let (_, tex) = self.own_hand_model_and_texture();
        let modell = world::player_arm_model(self.own_slim(), right);
        world::emit_entity_model_two_sided(&modell, &world::first_person_arm_pose(right), arm, tex, ENTITY_BRIGHTNESS, out);
    }

    // Dritte Person: den Arm der Hand in die Ansehen-Haltung blenden; zurueck kommt die
    // Item-Drehung an der Hand (ohne Ansehen die Einheit). Die Haupthand ist der rechte Arm.
    // Dazu, wie hoch das Item hochgeworfen ueber der Hand ist (Bloecke, s. third_person_item_extra).
    pub(crate) fn inspect_third_person(&self, hand: i32, tick_progress: f32, pose: &mut world::Pose) -> (world::entity_model::Affine, f32) {
        let ruhe = (world::entity_model::Affine::IDENTITY, 0.0);
        let (Some((_, ticks)), Some(kind)) = (self.own_inspect, self.inspect_kind(hand)) else { return ruhe };
        let t = (ticks as f32 + tick_progress) / kind.ticks();
        if t >= 1.0 {
            return ruhe;
        }
        let rechts = hand == protocol::packets::HAND_MAIN;
        let tp = world::item_inspect::third_person(kind, t, rechts);
        let teil = if rechts { "right_arm" } else { "left_arm" };
        let jetzt = pose.get(teil).rotation;
        pose.rotate(teil, std::array::from_fn(|i| jetzt[i] + (tp.arm[i] - jetzt[i]) * tp.blend));
        (tp.item, tp.lift)
    }

    // Die Zusatz-Transformation des Items an der Hand in F5: die Drehung, und hochgeworfen
    // `lift` Bloecke senkrecht nach oben in der Welt -- in den Raum der Hand umgerechnet, die
    // Hand selbst ist ja gedreht.
    pub(crate) fn third_person_item_extra(&self, (drehung, lift): (world::Affine, f32), right: bool, model: &world::Model, pose: &world::Pose, base: world::Affine) -> world::Affine {
        if lift <= 0.0 {
            return drehung;
        }
        let teil = if right { "right_arm" } else { "left_arm" };
        let Some(arm) = world::entity_model::part_transform(model, pose, base, teil) else { return drehung };
        let zurueck = crate::held_item::hand_pre_transform(arm, right).inverse();
        let hand = crate::held_item::hand_pre_transform(arm, right).apply([0.0; 3]);
        let (a, b) = (zurueck.apply(hand), zurueck.apply([hand[0], hand[1] + lift, hand[2]]));
        world::Affine::translation([b[0] - a[0], b[1] - a[1], b[2] - a[2]]).then(drehung)
    }
}

// Der Arm aus den Hand-Debug-Werten `w` (s. Settings::hand_fist_items): die Faust von `ziel` um
// ihre Verschiebung, dann die Drehung des Arms um sie, um seine eigene Laenge und die der
// Schulter um sie, links gespiegelt. Davor gehoert noch first_person_arm_transform.
fn tuned_arm(ziel: [f32; 3], w: &[i32; 16], right: bool) -> world::Affine {
    let s = if right { 1.0 } else { -1.0 };
    let o = [w[0], w[1], w[2]].map(|v| v as f32 / 1000.0);
    let ziel = [ziel[0] + s * o[0], ziel[1] + o[1], ziel[2] + o[2]];
    let r = [w[3], w[4], w[5]].map(|v| v as f32);
    let p = world::item_inspect::shoulder(right);
    let schulter = world::Affine::translation(ziel)
        .then(world::Affine::rotation_xyz_deg(w[7] as f32, s * w[8] as f32, s * w[9] as f32))
        .then(world::Affine::translation([-ziel[0], -ziel[1], -ziel[2]]))
        .apply(p);
    let drehung = world::Affine::translation(ziel)
        .then(world::Affine::rotation_xyz_deg(r[0], s * r[1], s * r[2]))
        .then(world::Affine::rotation_axis_angle((s * w[6] as f32).to_radians(), world::item_inspect::arm_axis(ziel, schulter)))
        .then(world::Affine::translation([-ziel[0], -ziel[1], -ziel[2]]));
    drehung.then(world::item_inspect::arm_from(ziel, schulter, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str) -> Option<world::HeldItem> {
        let id = (0..2000).find(|&i| world::item_registry::item_name(i) == Some(name)).unwrap();
        Some(world::HeldItem::plain(id))
    }

    #[test]
    fn karte_allein_mit_beiden_haenden_sonst_mit_einer() {
        assert_eq!(hand_poses(item("filled_map"), None, false).1, MapHold::Both);
        assert_eq!(hand_poses(item("filled_map"), item("torch"), false).1, MapHold::Arm(true));
        assert_eq!(hand_poses(item("stone"), item("filled_map"), false).1, MapHold::Arm(false));
        assert_eq!(hand_poses(item("stone"), item("filled_map"), true).1, MapHold::Arm(true));
    }

    #[test]
    fn hochhalten_am_richtigen_arm() {
        assert_eq!(hand_poses(item("torch"), item("compass"), false).0, (true, true));
        assert_eq!(hand_poses(item("stone"), item("lantern"), false).0, (false, true));
        assert_eq!(hand_poses(item("stone"), item("lantern"), true).0, (true, false));
    }
}
