// Bewegung in Luft, beim Gleiten, in Wasser und Lava, Schwimmen und Springen.

use super::*;

impl PlayerPhysics {
    pub(crate) fn tick_air(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        // `f`: Reibung des Blocks unter den Fuessen, wie in travelInAir() VOR der Bewegung
        // dieses Ticks gelesen (also der Bodenkontakt-Zustand vom Ende des letzten Ticks).
        // In der Luft zaehlt immer 1.0.
        let f = if self.on_ground { self.ground_friction(world) } else { 1.0 };
        // Vanilla: Blockreibung (float) × 0.91F, das Produkt noch als float.
        let horizontal_drag = if input.float_drag { f64::from(f as f32 * 0.91f32) } else { f * 0.91 };

        let speed = self.movement_speed(input);
        // Sneak NICHT mehr hier auf die accel: die Sneak-Verlangsamung (SNEAKING_SPEED = 0.3)
        // gehoert in Vanilla auf den BEWEGUNGS-INPUT (ClientPlayerEntity.applyMovementSpeed-
        // Factors), nicht auf die Geschwindigkeit -- und zwar VOR der Richtungs-Skalierung, s.
        // add_move_input. Sprint (×1.3) bleibt dagegen ein echter Speed-Attribut-Modifikator.
        // getFrictionInfluencedSpeed(): am Boden aus der Reibung rueckgerechnet (das
        // Zusammenspiel mit horizontal_drag unten erzeugt die Zielgeschwindigkeit von
        // selbst als Gleichgewicht, ohne dass wir eine Hoechstgeschwindigkeit fest
        // verdrahten), in der Luft die Luftkontroll-Basis (AIR_ACCEL = 0.02).
        //
        // ECHTER BUG gewesen (Flags nur noch beim Springen): der
        // Sprint-Multiplikator (×1.3) -- und analog Sneak -- wirkt AUCH auf die Luftkontrolle,
        // nicht nur am Boden. GrimAC bestaetigte es im consoledebug eindeutig: waehrend eines
        // Sprint-Sprungs war A/P = 1/1.3 = 0.769 in BEIDEN Horizontalachsen (wir zu langsam,
        // weil AIR_ACCEL fest bei 0.02 blieb statt 0.026). Deshalb hier denselben
        // Geschwindigkeits-Faktor (speed/BASE_SPEED) auf AIR_ACCEL anwenden wie am Boden.
        //
        // WICHTIG fuer die Luft: dort steht KEIN Attributwert, sondern ein festes Paar.
        // PlayerEntity.getOffGroundSpeed() gibt schlicht 0.026 beim Sprinten und sonst 0.02
        // (als float) zurueck (am Bytecode nachgelesen) -- dass der Sprintwert genau AIR_ACCEL * 1.3 ist,
        // ist ein Zufall der gewaehlten Zahlen und keine Regel. Schnelligkeit und Langsamkeit
        // aendern die Luftkontrolle deshalb NICHT. Frueher stand hier `AIR_ACCEL * (speed /
        // BASE_SPEED)`, was mit dem Effektfaktor still falsch geworden waere.
        // Player.getFlyingSpeed: fliegend die Fluggeschwindigkeit, sprintend doppelt.
        let accel = if self.on_ground {
            speed * (0.21600002 / (f * f * f))
        } else if self.abilities.flying {
            f64::from(self.abilities.fly_speed * if self.sprinting { 2.0 } else { 1.0 })
        } else if self.sprinting {
            AIR_ACCEL * SPRINT_SPEED_MULT
        } else {
            AIR_ACCEL
        };

        // Sprung: Vanilla erlaubt pro gehaltener Taste jeden Tick erneut zu springen,
        // sobald wieder Bodenkontakt besteht -- kein Einzel-Press noetig. jump_cooldown
        // ist an Land praktisch immer schon 0 (die Sprung-Flugzeit ist laenger als die
        // 10-Tick-Abklingzeit, s. try_jump-Kommentar), die Pruefung hier ist trotzdem
        // noetig, weil beide Zweige denselben Zaehler teilen.
        // Fliegend kein Sprung: LivingEntity.aiStep springt nur mit isAffectedByFluids().
        if input.jump && self.on_ground && self.jump_ready() && !self.abilities.flying {
            self.try_jump(input, world);
        }

        self.add_move_input(input, accel);
        // LivingEntity.applyMovementInput(): updateVelocity -> applyClimbingSpeed -> move.
        // Die Klemmung gehoert also ZWISCHEN Beschleunigung und Bewegung, nicht davor oder
        // danach -- sonst legt man im Kletter-Tick eine andere Strecke zurueck als der Server.
        // Zuschauer klettern nie (onClimbable).
        if !input.spectator {
            self.apply_climbing_speed(world);
        }
        self.move_and_zero(input, world, shapes);
        // ... und direkt NACH move(): bei horizontaler Kollision oder gehaltenem Sprung an einer
        // Leiter (isClimbing) wird vel.y auf 0.2 gesetzt. `jumping` ist beim Client schlicht
        // die gehaltene Sprungtaste. Das ist der eigentliche Aufstieg: gegen die Leiter druecken
        // ODER springen. isClimbing() wird hier ERNEUT gefragt, mit der Position NACH der
        // Bewegung -- wer die Kletterzelle in diesem Tick verlassen hat, bekommt den Schub nicht
        // mehr.
        if (self.horizontal_collision || input.jump) && !input.spectator && self.is_climbing(world) {
            self.vel.y = CLIMB_UP_SPEED;
        }

        // Gravitation + Daempfung: NACH der Kollision, wie Vanillas travelInAir -- betrifft
        // die Geschwindigkeit fuer den NAECHSTEN Tick, nicht die gerade zurueckgelegte
        // Strecke dieses Ticks.
        // travelMidAir: Schwebeeffekt ERSETZT die Schwerkraft, Sanfter Fall schwaecht sie ab
        // (s. effective_gravity -- gefragt wird mit der Geschwindigkeit NACH move()).
        match input.levitation {
            Some(stufe) => self.vel.y += (0.05 * f64::from(stufe + 1) - self.vel.y) * 0.2,
            None => self.vel.y -= effective_gravity(self.vel.y, input, self.body.gravity),
        }
        self.vel.x *= horizontal_drag;
        self.vel.z *= horizontal_drag;
        self.vel.y *= float_or(input, VERTICAL_DRAG);
    }

    // LivingEntity.travelGliding: an einer Leiter endet der Flug (dann gilt die normale
    // Luftbewegung), sonst rechnet calc_gliding_velocity die neue Geschwindigkeit und der
    // Spieler bewegt sich damit -- ohne Eingabe-Beschleunigung und ohne Luftreibung, die
    // Daempfung steckt schon in der Formel.
    pub(crate) fn tick_glide(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        if self.is_climbing(world) {
            self.tick_air(input, world, shapes);
            self.gliding = false;
            return;
        }
        // Das Springen steht in LivingEntity.tickMovement vor travel() und gilt auch gleitend
        // -- relevant nur am Boden, bevor der Server das Flag geloescht hat.
        if input.jump && self.on_ground && self.jump_ready() {
            self.try_jump(input, world);
        }
        let rotation = rotation_vector(input.pitch_deg, input.yaw_deg);
        self.vel = calc_gliding_velocity(self.vel, rotation, input.pitch_deg, effective_gravity(self.vel.y, input, self.body.gravity));
        self.move_and_zero(input, world, shapes);
    }

    // travelInWater(): Schwimmen -- Bremsfaktor statt Bodenreibung, konstante (statt
    // reibungsabhaengige) Beschleunigung, gedaempfte statt volle Gravitation.
    pub(crate) fn tick_water(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes, water_height: f64) {
        // Y-Position VOR der Bewegung dieses Ticks merken (wie Vanillas travelInFluid), fuer resetVerticalVelocityInFluid am Ende (s. jump_out_of_fluid).
        let y_before = self.pos.y;

        // TEMPORAERES Debug-Logging (CORAL_WATER_DEBUG=1), um den Z-Offset beim
        // Sprint-Schwimmen an echten Spieldaten nachzuvollziehen (0.0178 Offset an einem
        // reproduzierbaren Tick, synthetisch nicht nachstellbar).
        if std::env::var_os("CORAL_WATER_DEBUG").is_some() {
            let (bx, by, bz) = (self.pos.x.floor() as i32, (self.pos.y + 0.9).floor() as i32, self.pos.z.floor() as i32);
            let water_above_head = world::fluid_kind(world.block_at(bx, by, bz)).is_some();
            eprintln!(
                "[water] pos=({:.4},{:.4},{:.4}) vel=({:.5},{:.5},{:.5}) water_height={:.4} raw_sw={} swim={} sprint={} on_ground={} hcol={} water_above_head={} sneak={} jump={} look_y={:.3}",
                self.pos.x,
                self.pos.y,
                self.pos.z,
                self.vel.x,
                self.vel.y,
                self.vel.z,
                water_height,
                self.raw_swimming,
                self.swimming,
                self.sprinting,
                self.on_ground,
                self.horizontal_collision,
                water_above_head,
                input.sneak,
                input.jump,
                input.look_dir_y
            );
        }

        self.apply_swim_input(input, world, water_height, true);
        self.apply_swim_pitch_pull(input, world);

        let was_falling = self.vel.y <= 0.0; // travelInFluid(): bl VOR moveRelative/move
        // travelInFluid holt die Schwerkraft ebenfalls VOR der Bewegung.
        let gravity = effective_gravity(self.vel.y, input, self.body.gravity);
        // Wasserlaeufer (WATER_MOVEMENT_EFFICIENCY). travelInWater():
        //   h = Attributwert, ausserhalb des Bodenkontakts halbiert
        //   wenn h > 0:  f rueckt um h auf 0.54600006 zu    (Bremse Richtung Landreibung)
        //                g += (getSpeed() - g) * h          (Beschleunigung Richtung Landtempo)
        // Ohne das schwimmt man mit Wasserlaeufer III so langsam wie ganz ohne -- und der
        // Server (bzw. GrimAC, der dasselbe Attribut liest) rechnet die ganze Zeit schneller.
        let efficiency = if self.on_ground { input.water_movement_efficiency } else { input.water_movement_efficiency * 0.5 };
        let accel = if efficiency > 0.0 {
            FLUID_ACCEL + (self.movement_speed(input) - FLUID_ACCEL) * efficiency
        } else {
            FLUID_ACCEL
        };
        self.add_move_input(input, accel);
        self.move_and_zero(input, world, shapes);
        // travelInWater() hat denselben Kletter-Aufstieg wie die Luft, aber in ZWEI Punkten
        // schmaler: nur ueber die Horizontalkollision (die Sprungtaste zaehlt hier nicht) und
        // ohne die 0.15er Klemmung, denn travelInWater ruft applyMovementInput gar nicht auf.
        // Er steht VOR der Wasserbremse -- die 0.8 wirken also noch auf die 0.2.
        // travelInLava kennt gar keinen Kletterzweig (im Jar nachgesehen), deshalb steht das
        // hier und nicht in tick_lava.
        if self.horizontal_collision && self.is_climbing(world) {
            self.vel.y = CLIMB_UP_SPEED;
        }

        // Reihenfolge wie in travelInWater: erst Sprint/Grundbremse, dann der Wasserlaeufer,
        // und die Gunst des Delfins setzt am Ende HART 0.96 (sie ersetzt, sie mischt nicht).
        let mut f = float_or(input, if self.sprinting { WATER_SPRINT_SLOW_DOWN } else { WATER_SLOW_DOWN });
        if efficiency > 0.0 {
            f += (WATER_EFFICIENCY_TARGET_DRAG - f) * efficiency;
        }
        if input.dolphins_grace {
            f = DOLPHINS_GRACE_SLOW_DOWN;
        }
        self.vel.x *= f;
        self.vel.y *= float_or(input, FLUID_VERTICAL_DRAG);
        self.vel.z *= f;
        self.vel = fluid_falling_adjusted_movement(self.vel, gravity, was_falling, self.sprinting);
        // resetVerticalVelocityInFluid() -- letzter Schritt von travelInWater(), s. dort.
        self.jump_out_of_fluid(world, shapes, y_before);
    }

    // travelInLava(): staerker gebremst als Wasser, kein Schwimm-Feingefuehl (kein
    // getFluidFallingAdjustedMovement ausserhalb der Flach-Verzweigung), dafuer ein fester
    // Sink-Zug -d/4 auf JEDEN Tick obendrauf -- der eigentliche Grund, warum man in Lava
    // absackt statt zu schweben.
    pub(crate) fn tick_lava(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes, lava_height: f64) {
        let y_before = self.pos.y;
        self.apply_swim_input(input, world, lava_height, false);

        let was_falling = self.vel.y <= 0.0;
        let gravity = effective_gravity(self.vel.y, input, self.body.gravity);
        self.add_move_input(input, FLUID_ACCEL);
        self.move_and_zero(input, world, shapes);

        if lava_height <= FLUID_JUMP_THRESHOLD {
            self.vel.x *= LAVA_SLOW_DOWN;
            self.vel.y *= FLUID_VERTICAL_DRAG;
            self.vel.z *= LAVA_SLOW_DOWN;
            self.vel = fluid_falling_adjusted_movement(self.vel, gravity, was_falling, self.sprinting);
        } else {
            self.vel *= LAVA_SLOW_DOWN;
        }
        self.vel.y -= gravity / 4.0;
        // resetVerticalVelocityInFluid() -- letzter Schritt von travelInLava(), identisch zu
        // Wasser (derselbe Kick, dieselbe Bedingung).
        self.jump_out_of_fluid(world, shapes, y_before);
    }

    // aiStep()s Sprung-/Schwimm-Verzweigung, auf den Wasser-/Lava-Fall reduziert (der
    // "onGround -> jumpFromGround()"-Zweig bleibt unveraendert wie an Land): in flachem
    // Wasser/Lava am Boden ganz normal hochspringen (echter Sprung, s. try_jump/
    // jump_cooldown), sonst hoch- bzw. (beim Schleichen) herunterschwimmen -- Letzteres
    // OHNE Cooldown, jeden Tick um FLUID_VERTICAL_KICK.
    //
    // Der echte Sprung hier respektiert denselben jump_cooldown-Gate wie an Land (s.
    // try_jump-Kommentar) -- korrekt fuer sich genommen, war aber NICHT die Ursache des
    // "water hopping" (s. jump_out_of_fluid-Kommentar fuer die
    // tatsaechliche Ursache).
    // is_water: knockDownwards() (Sneak = abtauchen) ist in Vanilla WASSER-spezifisch
    // (ClientPlayerEntity.tickMovement: `isTouchingWater() && sneak && shouldSwimInFluids()`),
    // NICHT in Lava -- deshalb der Parameter. Der Swim-Up-Kick (knockUpwards/Jump-in-Fluid)
    // gilt dagegen fuer beide Fluide (LivingEntity.tickMovement).
    pub(crate) fn apply_swim_input(&mut self, input: &Input, world: &World, fluid_height: f64, is_water: bool) {
        // Sneak = abtauchen (knockDownwards, -0.04): ClientPlayerEntity.tickMovement (Zeile 811),
        // WASSER-spezifisch und voellig UNABHAENGIG vom Springen. Laeuft in Vanilla VOR dem
        // Swim-Up (super.tickMovement -> LivingEntity Zeile 2955), deshalb hier zuerst.
        //
        // ECHTER BUG gewesen (Jump und Sneak gleichzeitig im Wasser flaggte heftig): vorher ein if/else if, bei dem der Jump-Zweig den Sneak-Kick
        // verschluckte. Vanilla feuert beide Checks getrennt im selben Tick -> Jump+Sneak =
        // +0.04 (swimUpward) - 0.04 (knockDownwards) = netto 0. Wir schickten netto +0.04 pro
        // Tick -> die Y-Position lief Tick fuer Tick von Grims Simulation weg -> harter Flag.
        // knockDownwards() selbst hat KEINE Tiefen-Bedingung; am Boden in flachem Wasser tut es
        // nichts (Bodenkollision nullt vel.y), es driftet also nicht.
        if is_water && input.sneak {
            self.vel.y -= FLUID_VERTICAL_KICK;
        }
        // Jump = hochschwimmen (swimUpward, +0.04) bzw. echter Sprung in flachem Wasser/Lava am
        // Boden (LivingEntity.tickMovement Zeile 2955-2976). shouldSwimInFluids() ist fuer einen
        // normalen Spieler immer true (Zeile 565), deshalb nicht modelliert. Der echte Sprung
        // ueberschreibt einen vorher gesetzten Sneak-Kick per vel.y.max(JUMP_VELOCITY) -- exakt
        // wie Vanilla (knockDownwards vor jump(), jump() nutzt Math.max).
        if input.jump {
            let swim_up = fluid_height > 0.0 && !(self.on_ground && fluid_height <= FLUID_JUMP_THRESHOLD);
            if swim_up {
                self.vel.y += FLUID_VERTICAL_KICK;
            } else if self.on_ground && self.jump_ready() {
                self.try_jump(input, world);
            }
        }
    }

    // LivingEntity.jump(): setzt die volle Sprunggeschwindigkeit (nie NIEDRIGER als eine
    // schon vorhandene, s. Math.max) und -- unabhaengig davon, ob an Land oder im Wasser/
    // Lava gesprungen wurde -- den Sprint-Schub in Blickrichtung, falls gerade gesprintet
    // wird. Startet danach die 10-Tick-Abklingzeit (s. jump_cooldown-Feldkommentar), aus
    // dem lokalen Yarn-1.21.11-Jar nachgesehen (LivingEntity.jump()/tickMovement()).
    pub(crate) fn try_jump(&mut self, input: &Input, world: &World) {
        // LivingEntity.getJumpVelocity(): (float)JUMP_STRENGTH * strength *
        // getJumpVelocityMultiplier(). Der Multiplikator ist nur beim Honigblock != 1.0.
        // Die Multiplikation absichtlich in f32 (Vanilla rechnet die ganze Zeile in float),
        // der neutrale Fall aber unveraendert ueber die f64-Konstante -- sonst wuerde JEDER
        // Sprung um die float-Rundung von 0.42 (1.3e-8) verschoben, ohne dass hier irgendein
        // Honig im Spiel ist.
        let mult = self.jump_multiplier(world);
        // Sprungkraft kommt in derselben float-Rechnung obendrauf: 0.1 * (Stufe + 1).
        // JUMP_STRENGTH vom Server (s. BodyAttributes), Vorgabe 0.42F.
        let staerke = self.body.jump_strength as f32;
        let jump_velocity = match input.jump_boost {
            Some(stufe) => f64::from(staerke * mult + 0.1f32 * (stufe as f32 + 1.0)),
            None if mult == 1.0 && self.body.jump_strength == DEFAULT_JUMP_STRENGTH => JUMP_VELOCITY,
            None => f64::from(staerke * mult),
        };
        self.vel.y = self.vel.y.max(jump_velocity);
        if self.sprinting {
            // MathHelper.sin/cos (Float-Tabelle) wie Vanillas LivingEntity.jump(), NICHT exaktes
            // sin/cos -- sonst weicht der Sprint-Schub minimal ab (s. add_move_input-Kommentar).
            self.vel.x -= f64::from(mc_sin(f64::from(input.yaw_rad))) * SPRINT_JUMP_BOOST;
            self.vel.z += f64::from(mc_cos(f64::from(input.yaw_rad))) * SPRINT_JUMP_BOOST;
        }
        self.jump_cooldown = 10; // wirkungslos, solange jump_cooldown_enabled aus ist, s. jump_ready()
    }
}
