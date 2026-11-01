// Zuruecksetzen, Zustand ueber Respawns, Bodenkontakt, Augenhoehe und Pose.

use super::*;

impl PlayerPhysics {
    // Ein neuer Spieler (Login, auch nach einem Serverwechsel hinter einem Proxy, und
    // Respawn): Vanilla baut dann einen frischen LocalPlayer -- er gleitet nicht, steht in
    // normaler Pose, hat keinen Schwung und ist nicht gefallen. Sonst flog man nach einem
    // Wechsel mitten im Elytra-Flug auf dem neuen Server weiter.
    pub const fn reset_for_new_player(&mut self) {
        self.gliding = false;
        self.glide_pose = false;
        self.glide_start_requested = false;
        self.vel = DVec3::ZERO;
        self.fall_distance = 0.0;
    }

    // Zustand, den ein Server-Teleport NICHT anfasst (s. Aufrufstelle in main.rs): die
    // Position kommt vom Server, der Tastenzustand und die Koerperhaltung nicht.
    //
    // Sneak-Taste: PlayerPhysics::new() setzt prev_sneak auf false -- der naechste Tick sah
    // nach JEDEM Setback wie ein frischer Tastendruck aus und liess die Schleich-
    // Verlangsamung einen Tick lang weg (daraus wurde eine Setback-Endlosschleife).
    //
    // Pose: GrimAC behaelt player.pose ueber einen Teleport hinweg und baut nur die Box an
    // der neuen Position neu auf (MovementCheckRunner.handleTeleport: `player.boundingBox =
    // getCollisionBoxForPlayer(player, x, y, z)`, KEIN updatePlayerPose). Vanilla kennt
    // ebenfalls keinen Posen-Reset beim Teleport. Wuerden wir hier auf Stehen zuruecksetzen,
    // rechnete der erste Tick nach einem Setback mit 1.8, waehrend Grim weiter 1.5 (bzw. 0.6
    // beim Kriechen) ansetzt -- genau der Hitbox-Versatz, den ein Setback eigentlich
    // beenden soll.
    //
    // BODENKONTAKT (2026-09-22, aus einem Grim-consoledebug hergeleitet): genauso wenig darf
    // ein Teleport on_ground neu bestimmen. Vanillas setValuesFromPositionPacket setzt NUR
    // Position, Delta und Rotation (`entity.setPos`, `setDeltaMovement`, `setYRot/setXRot`,
    // `setOldPosAndRot`) -- `onGround` und `mainSupportingBlockPos` fasst es nicht an, die
    // aendert erst der naechste `move()`. Wir haben hier frueher mit einem Probe-Sweep neu
    // gemessen; nach einer Enderperle landet man in der Luft, also stand on_ground auf false,
    // waehrend Vanilla (und damit Grim) den Bodenkontakt von VOR dem Wurf behielt. Die Folge
    // war ein einziger Tick mit der falschen Reibung: wir 0.91 statt 0.546.
    //
    // Gemessen im Flag: die waagerechte Bewegung im Tick davor war 0.177512968, und
    //     0.177512968 * (0.91 - 0.546) = 0.06461472
    // gegen Grims angezeigte 0.06461471 -- der Rest der Reihe zerfiel danach sauber mit 0.546.
    pub const fn carry_over_state(&self) -> CarryOverState {
        CarryOverState {
            prev_sneak: self.prev_sneak,
            prev_jump: self.prev_jump,
            gliding: self.gliding,
            glide_pose: self.glide_pose,
            swimming: self.swimming,
            crouching: self.crouching,
            on_ground: self.on_ground,
            supporting_block: self.supporting_block,
            force_update_supporting_block: self.force_update_supporting_block,
            abilities: self.abilities,
            jump_trigger_time: self.jump_trigger_time,
            sprinting: self.sprinting,
        }
    }

    pub const fn restore_carry_over_state(&mut self, state: CarryOverState) {
        self.prev_sneak = state.prev_sneak;
        self.prev_jump = state.prev_jump;
        self.gliding = state.gliding;
        self.glide_pose = state.glide_pose;
        self.swimming = state.swimming;
        self.crouching = state.crouching;
        self.on_ground = state.on_ground;
        self.supporting_block = state.supporting_block;
        self.force_update_supporting_block = state.force_update_supporting_block;
        self.abilities = state.abilities;
        self.jump_trigger_time = state.jump_trigger_time;
        self.sprinting = state.sprinting;
    }

    // PlayerEntity.isStandingOnSurface(stepHeight) -- die Bedingung, unter der der
    // Sneak-Kantenschutz ueberhaupt greift:
    //   isOnGround() || fallDistance < stepHeight && !isSpaceAroundPlayerEmpty(0, 0, stepHeight - fallDistance)
    //
    // ECHTER BUG gewesen: hier stand nur `self.on_ground`. Der zweite Zweig ist aber genau
    // der, der beim Schleichen ueber eine Kante zaehlt -- wer sich schon einen Moment in der
    // Luft befindet (Stufe hinunter, Kante gerade verlassen), aber noch keine halbe
    // Schritthoehe gefallen ist, wird von Vanilla WEITERHIN festgehalten. Ohne diesen Zweig
    // laeuft man in genau diesen Ticks weiter, waehrend die Server-Simulation klemmt.
    pub(crate) fn is_standing_on_surface(&self, world: &World, shapes: &world::BlockShapes) -> bool {
        if self.on_ground {
            return true;
        }
        let step = self.body.step_height;
        self.fall_distance < step && !space_around_player_empty(self.pos, 0.0, 0.0, step - self.fall_distance, world, shapes, self.shape_context(), self.body)
    }

    pub fn refresh_on_ground(&mut self, world: &World, shapes: &world::BlockShapes) {
        let height = self.current_height();
        // on_ground=false: dieser Probe-Sweep darf NIE steppen -- er fragt nur "steht unter
        // mir etwas", und ein Schritt daraus wuerde die Position verfaelschen.
        let (_, _, hit_y_down, _, _) =
            move_and_collide(self.pos, DVec3::new(0.0, -0.01, 0.0), height, false, world, shapes, self.shape_context(), self.body);
        self.on_ground = hit_y_down;
        // Nach einem Teleport gehoert auch der Standblock neu bestimmt -- der alte liegt in
        // aller Regel ganz woanders. Ohne Bewegung dieses Ticks entfaellt der zweite Anlauf.
        self.force_update_supporting_block = true;
        self.update_supporting_block(self.on_ground, DVec3::ZERO, world, shapes);
    }

    // Augenhoehe der aktuellen Pose -- Vanillas Entity.getStandingEyeHeight(), also der
    // eyeHeight-Eintrag der Pose in PlayerLikeEntity.POSE_DIMENSIONS.
    //
    // ACHTUNG: das ist der ZIELWERT. Die KAMERA nimmt ihn nicht direkt, sondern naehert sich
    // ihm pro Tick um die Haelfte an (s. App::tick_camera_eye_height) -- fuer alles andere
    // (Fluessigkeits-/Blickpruefungen) ist dagegen dieser harte Wert der richtige, genau wie
    // in Vanilla.
    pub fn eye_height(&self) -> f32 {
        if self.sleeping {
            return SLEEPING_HEIGHT as f32;
        }
        // SPIN_ATTACK und GLIDING haben dieselben Masse wie SWIMMING (EntityPose-Tabelle in
        // PlayerEntity).
        self.body.scaled(if self.swimming || self.spin_attack || self.glide_pose {
            SWIMMING_EYE_HEIGHT
        } else if self.crouching {
            CROUCHING_EYE_HEIGHT
        } else {
            EYE_HEIGHT
        })
    }

    pub fn eye_pos(&self) -> DVec3 {
        self.pos + DVec3::new(0.0, f64::from(self.eye_height()), 0.0)
    }

    pub fn current_height(&self) -> f64 {
        if self.sleeping {
            return SLEEPING_HEIGHT;
        }
        self.pose_height(if self.swimming || self.spin_attack || self.glide_pose {
            SWIMMING_HEIGHT
        } else if self.crouching {
            CROUCHING_HEIGHT
        } else {
            PLAYER_HEIGHT
        })
    }

    // Hoehe einer (nicht schlafenden) Pose samt SCALE.
    pub(crate) fn pose_height(&self, h: f32) -> f64 {
        f64::from(self.body.scaled(h))
    }

    pub fn width(&self) -> f64 {
        self.body.width()
    }

    // isUnderWater(): Augenposition liegt innerhalb einer Wasserzelle, UNTER deren
    // tatsaechlichem Fuellstand (nicht nur "beruehrt" wie fluid_height_scan fuer den Koerper
    // -- der Kopf muss wirklich eingetaucht sein, s. updateSwimming()-Kommentar).
    pub(crate) fn is_underwater(&self, world: &World) -> bool {
        let eye = self.eye_pos();
        let (bx, by, bz) = (eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
        let state = world.block_at(bx, by, bz);
        if world::fluid_kind(state) != Some(world::FluidKind::Water) {
            return false;
        }
        let Some(h) = world::fluid_height_at(world, bx, by, bz) else {
            return false;
        };
        eye.y - f64::from(by) <= h
    }
}
