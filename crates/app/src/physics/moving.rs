// Bewegungseingabe, Bewegen mit Kollision und Kolben.

use super::*;

impl PlayerPhysics {
    // Der aktuelle Wert von MOVEMENT_SPEED (Vanillas getSpeed(), gesetzt in
    // PlayerEntity.aiStep aus dem Attribut).
    //
    // Der Grundwert kommt vom Server (Input::movement_speed, Vorgabe 0.1) -- ein Plugin, das
    // die Laufgeschwindigkeit setzt, oder eine Verzauberung mit Tempo-Modifikator aendert ihn.
    // Sprint (x1.3) und Schnelligkeit/Langsamkeit legt die Physik weiter selbst an, genau wie
    // Vanillas Client: sie sind aus dem Paket herausgerechnet (s. protocol LOCAL_SPEED_MODIFIERS),
    // sonst zaehlten sie doppelt.
    //
    // Geklemmt wie Vanillas ClampedEntityAttribute (0..1024) -- sonst liefe man ab
    // Langsamkeit VII rueckwaerts.
    pub(crate) fn movement_speed(&self, input: &Input) -> f64 {
        let mut speed = input.movement_speed;
        if self.sprinting {
            speed *= SPRINT_SPEED_MULT;
        }
        (speed * input.effect_speed_factor).clamp(MOVEMENT_SPEED_MIN, MOVEMENT_SPEED_MAX)
    }

    pub(crate) fn add_move_input(&mut self, input: &Input, accel: f64) {
        let raw_fwd = f64::from(input.forward);
        let raw_strafe = f64::from(input.strafe);
        let raw_len = (raw_fwd * raw_fwd + raw_strafe * raw_strafe).sqrt();
        if raw_len <= 1.0e-7 {
            return;
        }
        // KeyboardInput: Rohinput auf Einheitslaenge normalisieren.
        let (u_fwd, u_strafe) = (raw_fwd / raw_len, raw_strafe / raw_len);
        // applyMovementSpeedFactors: × 0.98, × 0.3 nur bei shouldSlowDown() == isInSneakingPose().
        // Das ist das um einen Tick nachlaufende Bewegungs-Feld (in_sneaking_pose_move), NICHT
        // die aktuelle Hitbox-Pose (self.crouching) -- s. Feld-Kommentar. Ohne diesen Lag schickt
        // der Client beim Sneak-Loslassen im selben Tick schon 0.98*accel, waehrend Grim (wie
        // Vanilla) noch 0.294*accel erwartet -> grosser Simulation-Flag im Crouch->Sprint-Uebergang.
        // Reihenfolge wie in applyMovementSpeedFactors: erst 0.98, dann der Item-Faktor,
        // dann der Schleich-Faktor. Der Item-Faktor fehlte -- GrimAC rechnete ihn mit (es
        // sieht unsere USE_ITEM-Pakete), wir nicht, und die Differenz war im consoledebug als
        // Verhaeltnis 0.3/0.2 = 1.5 messbar.
        // Der Schleich-Faktor ist Vanillas SNEAKING_SPEED-ATTRIBUT, kein fester Wert: die
        // Verzauberung "Schleichender Schritt" hebt ihn je Stufe um 0.15 ueber die 0.3
        // hinaus. Ohne das schleicht man mit Schleichendem Schritt III mit 0.3 statt 0.75 --
        // GrimAC rechnet mit dem Attribut und flaggt die Differenz.
        let factor = 0.98
            * if input.using_item { input.item_use_speed } else { 1.0 }
            * if self.in_sneaking_pose_move { input.sneaking_speed } else { 1.0 };
        // applyDirectionalMovementSpeedFactors: Richtungs-Multiplikator, Laenge auf <=1 gedeckelt.
        let (af, as_) = (u_fwd.abs(), u_strafe.abs());
        let ratio = if af > as_ {
            as_ / af
        } else if as_ > 0.0 {
            af / as_
        } else {
            0.0
        };
        let dir_mult = (1.0 + ratio * ratio).sqrt();
        let h = (factor * dir_mult).min(1.0);
        let (fwd, strafe) = (u_fwd * h, u_strafe * h);
        // movementInputToVelocity: |input| <= 1, also keine erneute Normalisierung; × accel, drehen.
        // Vanilla nutzt MathHelper.sin/cos (Float-Tabelle), NICHT exaktes sin/cos -- sonst weicht
        // die Bewegungsrichtung um ~1e-15 ab (sichtbar an anticheat-test.coms Strafe/GCD-Anzeige:
        // xE-15 statt vanilla xE-30). yaw_rad ist bereits yaw_grad*(pi/180) in float (to_radians).
        let (sin_y, cos_y) = (f64::from(mc_sin(f64::from(input.yaw_rad))), f64::from(mc_cos(f64::from(input.yaw_rad))));
        self.vel.x += strafe * accel * cos_y - fwd * accel * sin_y;
        self.vel.z += fwd * accel * cos_y + strafe * accel * sin_y;
    }

    // move(MoverType.SELF, ...): Position anhand von self.vel fortschreiben, an Kollisionen
    // klemmen und die entsprechenden Geschwindigkeitskomponenten nullen -- identisch fuer
    // Luft/Wasser/Lava (Teil der Bewegungs-Engine, nicht von travelIn*() selbst).
    pub(crate) fn move_and_zero(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) -> (bool, bool, bool, bool) {
        // Sneak-Kantenschutz VOR der Kollisionsbewegung (wie Vanillas move(): erst
        // adjustMovementForSneaking, dann collide). clipAtLedge() == isSneaking() -> self.crouching;
        // isStandingOnSurface ~ on_ground; nur bei nicht-aufwaerts gerichteter Bewegung.
        // WICHTIG: nur die BEWEGUNG dieses Ticks beschneiden, NICHT self.vel -- Vanilla behaelt die
        // Geschwindigkeit (nur das Positions-Delta wird geklemmt), sonst weicht der naechste Tick
        // von Grims Simulation ab. Die Kollisions-Nullung unten greift dadurch an der Kante nicht
        // (geklemmte Achse kollidiert nicht), self.vel laeuft also unveraendert in Reibung/naechsten
        // Tick weiter -- exakt wie Vanilla.
        // Entity.move(), ganz am Anfang und VOR adjustMovementForSneaking: ist ein
        // movementMultiplier gesetzt (Laengenquadrat ueber 1e-7, etwa durch Spinnweben), wird
        // die Bewegung damit multipliziert, der Multiplikator geleert und die Geschwindigkeit
        // auf null gesetzt.
        // Zwei Dinge auf einmal: die Strecke DIESES Ticks wird gestaucht, und die
        // Geschwindigkeit wird auf null gesetzt -- der naechste Tick faengt also bei 0 an und
        // baut nur die eine Tick-Beschleunigung (Schwerkraft, Eingabe) neu auf. Genau daher
        // kommt das Kriechen im Netz: kein Aufsummieren ueber mehrere Ticks.
        //
        // Das Nullen betrifft self.vel, das Stauchen NUR die Bewegung -- deshalb wird der
        // Faktor auf `requested` gelegt und nicht auf self.vel: unten haengt
        // horizontal_collision am Vergleich von angefragter zu erreichter Bewegung, und
        // Vanilla vergleicht dort ebenfalls gegen die bereits gestauchte Bewegung.
        // Entity.move mit noPhysics (Zuschauer): einfach versetzen, keine Kollision.
        if input.spectator {
            self.pos += self.vel;
            self.horizontal_collision = false;
            self.minor_horizontal_collision = false;
            return (false, false, false, false);
        }
        let mut requested = self.vel;
        if self.movement_multiplier.length_squared() > 1.0e-7 {
            requested *= self.movement_multiplier;
            self.movement_multiplier = DVec3::ZERO;
            self.vel = DVec3::ZERO;
        }
        let mut move_vel = requested;
        // Auf `requested` statt self.vel: Vanillas adjustMovementForSneaking bekommt die
        // Bewegung uebergeben, also die schon gestauchte -- und self.vel ist nach dem Nullen
        // oben ohnehin nicht mehr die Bewegung dieses Ticks.
        // Player.maybeBackOffFromEdge: nicht fliegend.
        if self.sneak_edge_protection_enabled
            && self.crouching
            && !self.abilities.flying
            && self.is_standing_on_surface(world, shapes)
            && requested.y <= 0.0
        {
            let (dx, dz) = adjust_for_sneaking(self.pos, requested.x, requested.z, world, shapes, self.shape_context(), self.body);
            move_vel.x = dx;
            move_vel.z = dz;
        }
        self.finish_move(requested, move_vel, Some(input), input.sneak, world, shapes)
    }

    // Ein Kolben schiebt den Spieler (Entity.move mit MoverType.PISTON, s. app::pistons): kein
    // Kantenschutz, sonst derselbe Weg wie die eigene Bewegung. Vorher begrenzt Vanilla den
    // Schub je Tick und Achse auf insgesamt 0.51 -- mehrere Kolben (oder ein Block, der zwei
    // Zellen beruehrt) schieben zusammen also nie weiter als ein einzelner.
    pub fn piston_move(&mut self, movement: DVec3, game_tick: u64, world: &World, shapes: &world::BlockShapes) {
        let movement = self.limit_piston_movement(movement, game_tick);
        if movement == DVec3::ZERO {
            return;
        }
        let mut requested = movement;
        if self.movement_multiplier.length_squared() > 1.0e-7 {
            requested *= self.movement_multiplier;
            self.movement_multiplier = DVec3::ZERO;
            self.vel = DVec3::ZERO;
        }
        let input = self.last_input;
        self.finish_move(requested, requested, input.as_ref(), self.sneak_key, world, shapes);
    }

    // Entity.limitPistonMovement: nur EINE Achse (die erste von x, y, z mit Bewegung), deren
    // Summe ueber den Tick auf +-0.51 geklemmt wird.
    pub(crate) fn limit_piston_movement(&mut self, movement: DVec3, game_tick: u64) -> DVec3 {
        if movement.length_squared() <= 1.0e-7 {
            return movement;
        }
        if game_tick != self.piston_deltas_tick {
            self.piston_deltas = [0.0; 3];
            self.piston_deltas_tick = game_tick;
        }
        let Some(achse) = (0..3).find(|&i| movement[i] != 0.0) else { return DVec3::ZERO };
        let summe = (movement[achse] + self.piston_deltas[achse]).clamp(-0.51, 0.51);
        let schritt = summe - self.piston_deltas[achse];
        self.piston_deltas[achse] = summe;
        if schritt.abs() <= 1.0e-5 {
            return DVec3::ZERO;
        }
        let mut out = DVec3::ZERO;
        out[achse] = schritt;
        out
    }

    // Der Rest von Entity.move ab der Kollision: `requested` ist die angefragte Bewegung (nach
    // dem Stauchen im Netz), `move_vel` die nach dem Kantenschutz. `input` fehlt nur, solange
    // noch kein Tick lief; `sneak` ist die Sneak-Taste (Schleim federt dann nicht).
    pub(crate) fn finish_move(
        &mut self,
        requested: DVec3,
        move_vel: DVec3,
        input: Option<&Input>,
        sneak: bool,
        world: &World,
        shapes: &world::BlockShapes,
    ) -> (bool, bool, bool, bool) {
        let (new_pos, hit_x, hit_y_down, hit_y_up, hit_z) =
            move_and_collide(self.pos, move_vel, self.current_height(), self.on_ground, world, shapes, self.shape_context(), self.body);
        // Vanillas `vec3d` in Entity.move: die TATSAECHLICH zurueckgelegte Bewegung, also nach
        // Kantenschutz UND Kollision. Genau daran haengen unten horizontalCollision und die
        // Fallhoehe -- beide duerfen NICHT aus den Kollisions-Flags abgeleitet werden.
        let achieved = new_pos - self.pos;
        self.pos = new_pos;
        // Entity.move (Zeile 762-765): `if (Math.abs(movement.y) > 0.0 ||
        // isLogicalSideForUpdatingMovement()) { verticalCollision = movement.y != vec3d.y;
        // groundCollision = verticalCollision && movement.y < 0.0; setMovement(groundCollision,
        // ...) }`. Fuer den EIGENEN Spieler ist isLogicalSideForUpdatingMovement() wahr, der
        // Zweig laeuft also IMMER -- auch bei movement.y == 0, und dann ist das Ergebnis
        // zwangslaeufig `false`.
        //
        // ECHTER BUG gewesen (Grim "Simulation" beim Aufhoeren zu
        // crawlen, Offsets 0.009800 / 0.005351 / 0.002922): hier stand `if move_vel.y != 0.0`
        // um genau diese Zuweisung, d.h. bei movement.y == 0 blieb der ALTE Bodenkontakt
        // stehen. Anlass war eine Kosmetik-Meldung ("am Boden: false" im F3-Menue direkt nach
        // einer Server-Korrektur) -- die Anzeige stimmte damit, die Physik nicht mehr.
        //
        // Wirkung, aus Grims Offsets zurueckgerechnet: die drei Werte zerfallen mit
        // exakt 0.54602 bzw. 0.54607 pro Tick, also der Bodenreibung 0.6 × 0.91 = 0.546. Es ist
        // damit EINE einmalige Geschwindigkeits-Injektion, die danach nur noch ausklingt. Vier
        // Ticks Zerfall zurueckgerechnet ergibt 0.11027 -- und die Differenz zwischen
        // Boden- und Luftbeschleunigung bei vollem Vorwaerts-Input ist
        // 0.98 × (0.1 × 0.21600002/0.546³ − 0.02) = 0.11045, also 0.16 % daneben. Genau ein
        // Tick, in dem wir mit Boden- und Grim mit Luftbeschleunigung gerechnet hat.
        //
        // Der Fall trat nach JEDEM Teleport/Setback auf (dort ist vel exakt 0, also
        // movement.y == 0): der Setback erzeugte selbst den naechsten Flag, der zum naechsten
        // Setback fuehrte -- die Schleife, die auch nach den Posen-Korrekturen blieb.
        //
        // Der eine Tick "am Boden: false" nach einem Teleport ist also KEIN Fehler, sondern
        // genau das, was Vanilla dort meldet (und was GrimAC erwartet). Bitte nicht wieder
        // "reparieren".
        self.on_ground = hit_y_down;
        // Entity.move() ruft setMovement(groundCollision, horizontalCollision, vec3d) genau
        // hier -- also NACH dem Setzen von onGround und VOR getLandingPos/onEntityLand/
        // getVelocityMultiplier, die alle den Standblock brauchen. `achieved` ist Vanillas
        // vec3d, die tatsaechlich zurueckgelegte Bewegung.
        self.update_supporting_block(self.on_ground, achieved, world, shapes);
        // ECHTER BUG gewesen: hier stand `hit_x || hit_z`, also NUR die Kollisions-Flags des
        // Sweeps. Vanilla (Entity.move) vergleicht stattdessen die ANGEFRAGTE mit der
        // tatsaechlichen Bewegung:
        //   horizontalCollision = !approximatelyEquals(movement.x, vec3d.x) || (... .z)
        // Der Unterschied ist genau der Sneak-Kantenschutz: der beschneidet die Bewegung, ohne
        // dass irgendetwas kollidiert. Vanilla meldet dabei horizontalCollision = true, wir
        // meldeten false -- und dieses Bit geht in JEDEM Bewegungspaket an den Server.
        self.horizontal_collision = !approximately_equals(requested.x, achieved.x) || !approximately_equals(requested.z, achieved.z);
        // Entity.move(), direkt hinter horizontalCollision: minorHorizontalCollision gilt nur bei
        // horizontaler Kollision, und dann nach isHorizontalCollisionMinor(Bewegung).
        // ACHTUNG: Vanilla uebergibt die TATSAECHLICHE Bewegung (vec3d == `achieved`), nicht
        // die Differenz zur angefragten.
        self.minor_horizontal_collision = self.horizontal_collision && input.is_some_and(|input| is_horizontal_collision_minor(input, achieved));
        // Entity.move() ruft hier checkFallDamage auf, und LivingEntity holt darin den
        // Wasserschub nach (updateInWaterStateAndDoWaterCurrentPushing), solange isInWater()
        // noch falsch ist.
        // Das ist KEINE Doppelung des Schubs aus baseTick, sondern ein NACHSCHLAG fuer genau
        // den einen Tick, in dem man ins Wasser EINTAUCHT: isInWater() stammt aus baseTick,
        // wurde also an der Position VOR der Bewegung gemessen. Wer diesen Tick von oberhalb
        // der Wasseroberflaeche hineinfaellt, war dort noch trocken -- der Schub wird deshalb
        // erst jetzt, mit der neuen Box, nachgeholt.
        //
        // Der Nachschlag wirkt sich in DIESEM Tick seitlich nicht mehr aus (bewegt wurde schon),
        // aber handleRelativeFrictionAndCalculateMovement liest die Geschwindigkeit erst NACH
        // move() aus, travelInAir multipliziert sie also noch mit g = 0.91. Der Schub landet
        // damit um 0.91 gedaempft im naechsten Tick, wo dann der regulaere baseTick-Schub
        // dazukommt.
        //
        // Gemessen auf der Eis-/Wasserplattform von anticheat-test.com (Vanilla gegen uns,
        // beide durch denselben MITM-Proxy): Vanilla bewegt sich im Landetick um 0.026740,
        // wir bewegten uns um 0.014000. Die Differenz 0.012740 ist exakt 0.014 × 0.91, und
        // 0.014 × 0.91 + 0.014 = 0.026740. Danach folgen beide Reihen identisch
        // v <- 0.8 * v + 0.014.
        if !self.touching_water {
            let half = self.width() / 2.0;
            let hoehe = self.current_height();
            let box_min = DVec3::new(self.pos.x - half, self.pos.y, self.pos.z - half);
            let box_max = DVec3::new(self.pos.x + half, self.pos.y + hoehe, self.pos.z + half);
            if let Some(scan) = fluid_height_scan(box_min, box_max, world, shapes, world::FluidKind::Water) {
                apply_fluid_push(&mut self.vel, &scan, WATER_PUSH_SPEED);
                // updateInWaterStateAndDoWaterCurrentPushing() setzt bei Treffer beides:
                // resetFallDistance() und wasTouchingWater = true.
                self.fall_distance = 0.0;
                self.touching_water = true;
            }
        }
        // Entity.fall(): erst die Fallhoehe fortschreiben, dann beim Bodenkontakt nullen --
        // in dieser Reihenfolge, sonst stuende nach der Landung noch der letzte Zuwachs drin.
        // heightDifference ist die tatsaechliche Vertikalbewegung, und Vanilla castet sie auf
        // float, bevor sie von der (doppelt genauen) Fallhoehe abgezogen wird.
        if !self.touching_water && achieved.y < 0.0 {
            self.fall_distance -= f64::from(achieved.y as f32);
        }
        if self.on_ground {
            self.fall_distance = 0.0;
        }
        if hit_x {
            self.vel.x = 0.0;
        }
        // Die Y-Nullung IST Vanillas Block.onEntityLand (`setVelocity(vel.multiply(1, 0, 1))`),
        // aufgerufen in Entity.move, sobald die Vertikalbewegung beschnitten wurde. Genau diese
        // Standardfassung ueberschreibt der Schleimblock -- deshalb steht die Fallunterscheidung
        // hier und nicht irgendwo daneben.
        //
        // Ausloeser bleibt bewusst das Kollisions-Flag des Sweeps (nicht Vanillas Vergleich
        // `movement.y != vec3d.y`): dieser Zweig ist seit Langem gegen Grim eingemessen, und
        // die Stufenhilfe veraendert die erreichte Y-Bewegung, ohne dass etwas kollidiert
        // waere. Fuer die Landung selbst ist beides deckungsgleich.
        if hit_y_down || hit_y_up {
            let landing = block_at_pos(world, self.landing_pos(world));
            let restitution = world::block_registry::block_name(landing).map_or(0.0, block_bounce_restitution);
            // onEntityLand: Schleichen (bypassesLandingEffects() == isSneaking(), und
            // ClientPlayerEntity.isSneaking() ist die reine Sneak-TASTE) faellt auf die
            // Standardfassung zurueck -- daher das lautlose Landen mit gedruecktem Shift, auf
            // Schleim wie auf dem Bett.
            if restitution > 0.0 && !sneak {
                // bounce(): NUR eine negative Geschwindigkeit wird umgedreht. Sonst bleibt sie
                // stehen -- Vanilla nullt in diesem Zweig ausdruecklich nicht, weshalb ein
                // Deckenstoss ueber einem Schleimblock den Aufwaertsschwung behaelt.
                if self.vel.y < 0.0 {
                    self.vel.y = -self.vel.y * restitution;
                }
            } else {
                self.vel.y = 0.0;
            }
        }
        if hit_z {
            self.vel.z = 0.0;
        }
        // Entity.move(), allerletzter Schritt: die Geschwindigkeit wird waagerecht mit
        // getVelocityMultiplier() multipliziert. Steht damit VOR Schwerkraft/Reibung des
        // aufrufenden travel() -- der Faktor bremst also die Geschwindigkeit fuer den naechsten
        // Tick, nicht die Strecke dieses Ticks. Genau das erzeugt das zaehe Gefuehl auf
        // Seelensand/Honig: pro Tick 0.4 auf beide Horizontalachsen, waehrend die Eingabe
        // jeden Tick nur den kleinen Beschleunigungsbetrag wieder draufgibt.
        let f = self.velocity_multiplier(world);
        if f != 1.0 {
            self.vel.x *= f;
            self.vel.z *= f;
        }
        (hit_x, hit_y_down, hit_y_up, hit_z)
    }
}
