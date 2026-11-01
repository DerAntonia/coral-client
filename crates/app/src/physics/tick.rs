// Ein Physik-Tick.

use super::*;

impl PlayerPhysics {
    // Ein einzelner Vanilla-Tick. Muss mit fester Schrittweite (TICK_DT) aufgerufen werden.
    pub fn tick(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        // Muss als Allererstes stehen: jede Kollisionsabfrage dieses Ticks baut ihren
        // ShapeContext daraus (s. shape_context).
        self.sneak_key = input.sneak;
        self.last_input = Some(*input);
        // Player.tick, vor aiStep: im Zuschauermodus nie am Boden.
        if input.spectator {
            self.on_ground = false;
        }
        // Flugzustand vom Anfang des Ticks: LocalPlayer.aiStep liest ihn fuer die Schleich-Pose
        // und den Sprint, BEVOR der Doppelsprung ihn umschalten kann.
        let was_flying = self.abilities.flying;
        // shouldTravelInFluid(): (isInWater() || isInLava()) && isAffectedByFluids() &&
        // !canStandOnFluid() -- die letzten beiden sind fuer einen normalen Spieler immer
        // wahr/falsch (nur Spectator bzw. Strider weichen ab, beides hier nicht modelliert),
        // bleibt also nur die Fluessigkeits-Beruehrung. isInWater() gewinnt bei Ueberlappung
        // (travelInFluid() prueft erst Wasser), s. updateFluidHeightAndDoFluidPushing im
        // Client: Fuellstand wird VOR der Bewegung dieses Ticks anhand der Position vom Ende
        // des LETZTEN Ticks bestimmt (baseTick() laeuft vor aiStep()/travel()).
        //
        // WICHTIG, echter Bug gewesen (Flackern, wenn man beim Schwimmen mit dem Kopf
        // anstoesst): der Scan MUSS mit der Box vom Ende des LETZTEN Ticks
        // laufen (self.current_height(), noch VOR dem Pose-Update unten) -- NICHT hart mit
        // der Stehhoehe, wie eine fruehere Fassung annahm. Vanillas eigener
        // updateFluidHeightAndDoFluidPushing() nutzt die tatsaechliche,
        // aktuelle Box, nie eine hartkodierte Stehhoehe. Mit der zu grossen Stehhoehe-Box
        // reichte der Scan bei einer knapp ueber der Schwimm-Pose (0.6) liegenden
        // Unterwasser-Decke manchmal noch in die Deckenzelle (keine Wasserzelle mehr
        // gefunden) und manchmal knapp nicht (Rundungsrauschen an der Zellgrenze) -- die
        // Schwimm-Pose kippte dadurch JEDEN Tick zwischen 0.6 und 1.8, was Position und
        // Kamera wild durcheinanderwarf.
        //
        // Diese Box wird jetzt AUCH fuer die Sprint-Startbedingung unten gebraucht
        // (isPartlyTouchingWater()), muss also VOR der Sprint-Entscheidung stehen.
        let scan_height = self.current_height();
        let half = self.width() / 2.0;
        let box_min = DVec3::new(self.pos.x - half, self.pos.y, self.pos.z - half);
        let box_max = DVec3::new(self.pos.x + half, self.pos.y + scan_height, self.pos.z + half);
        let water_height = fluid_height_scan(box_min, box_max, world, shapes, world::FluidKind::Water);
        let lava_height = fluid_height_scan(box_min, box_max, world, shapes, world::FluidKind::Lava);

        // Stroemungsschub, bevor irgendetwas anderes an der Geschwindigkeit passiert --
        // Vanilla macht das in Entity.updateWaterState(), also in baseTick() und damit vor
        // travel(). Reihenfolge WASSER, dann LAVA: updateWaterState() ruft erst
        // checkWaterState() (Wasser, 0.014) und danach updateMovementInFluid(LAVA, ...).
        // Beide wirken unabhaengig, es ist kein Entweder-Oder -- in einem Block, der beides
        // beruehrt, addieren sich die Schuebe.
        // Player.isPushedByFluid: fliegend treibt keine Stroemung.
        if !was_flying {
            if let Some(scan) = &water_height {
                apply_fluid_push(&mut self.vel, scan, WATER_PUSH_SPEED);
            }
            if let Some(scan) = &lava_height {
                apply_fluid_push(&mut self.vel, scan, LAVA_PUSH_SPEED);
            }
        }

        // updateSwimming(): einmal drin (Kopf war untergetaucht), reicht Sprinten + blosse
        // Wasserberuehrung zum Weiterschwimmen -- deshalb bleibt man beim Auftauchen an der
        // Oberflaeche in der Schwimm-Pose, solange man weiter sprintet.
        let underwater = self.is_underwater(world);

        // ClientPlayerEntity.canStartSprinting()/canSprint(): ein NEUER Sprint laesst sich
        // nicht beginnen, waehrend man Entity.isPartlyTouchingWater() ist -- das ist NICHT
        // "beruehrt ueberhaupt Wasser", sondern per Definition (isTouchingWater() &&
        // !isSubmergedInWater()) NUR die Oberflaechenberuehrung (Fuesse/Koerper im Wasser,
        // Kopf noch drueber). Vollstaendig untergetaucht (Kopf unter Wasser, isSubmergedInWater()
        // true) ist davon ausdruecklich AUSGENOMMEN -- Sprint-Schwimmen mit bereits
        // eingetauchtem Kopf darf also jederzeit neu beginnen, nur an-der-Oberflaeche-Stehen
        // blockiert den Start. Am Yarn-1.21.11-Jar nachgesehen (ClientPlayerEntity.
        // canStartSprinting()/canSprint()); canSprintOrFly() (Hunger) und
        // Blindness/Elytra/Item-Nutzung sind hier nicht modelliert (immer erlaubt), s.
        // Klassenkommentar oben zu bewusst fehlenden Systemen.
        //
        // WICHTIG, echter Bug gewesen (mit Sprintlock liess sich an der
        // Wasseroberflaeche sprinten): ein schon laufender Sprint darf NICHT
        // pauschal beim Hineinlaufen ins Wasser weiterlaufen -- das gilt nur, solange man
        // bereits in der Schwimm-Pose ist (shouldStopSwimSprinting(), deutlich permissiver:
        // reine Wasserberuehrung reicht). Ausserhalb der Schwimm-Pose (normales Laufen, z.B.
        // nur knietief im Wasser ohne dass die Pose kippt) gilt stattdessen
        // shouldStopSprinting() -> canSprint(false), und DAS verlangt genau wie beim Start
        // !isPartlyTouchingWater() -- ein an Land begonnener Sprint stoppt also sofort, sobald
        // auch nur die Fuesse Wasser beruehren, falls die Schwimm-Pose (noch) nicht aktiv ist.
        // was_swimming_pose ist bewusst die Pose vom ENDE des LETZTEN Ticks (self.swimming ist
        // an dieser Stelle noch nicht fuer diesen Tick aktualisiert) -- genau wie Vanillas
        // eigenes isSwimming() beim Sprint-Entscheid VOR der Pose-Neuberechnung dieses Ticks
        // abgefragt wird.
        let partly_touching_water = water_height.is_some() && !underwater;
        let touching_water = water_height.is_some();

        // updateSwimming() (Entity.java:535) laeuft in baseTick() -- und baseTick() laeuft in
        // super.tick() (LivingEntity.tick Zeile 2633) VOR tickMovement() (Zeile 2672), in dem
        // erst die Sprint-Entscheidung (setSprinting) faellt und danach travel() die Bewegung
        // rechnet. isSwimming fuer Physik UND Pose DIESES Ticks wird also noch mit isSprinting
        // vom ENDE des LETZTEN Ticks bestimmt (self.sprinting ist hier absichtlich noch NICHT
        // fuer diesen Tick aktualisiert). Schwimmen greift damit erst genau EINEN Tick NACH dem
        // Sprint-Start.
        //
        // ECHTER BUG gewesen (Wasser-Setback beim schleichenden Reinlaufen, laut Grim
        // consoledebug): vorher stand diese Zeile NACH der Sprint-Entscheidung und nutzte
        // das frische self.sprinting -> wir wandten Schwimm-Physik (Pitch-Pull) im selben Tick
        // an, in dem Sprint startete. Grim (= Vanilla) rechnete fuer diesen Tick noch Crouch-Sink
        // (-0.19992), wir schon Schwimmen (-0.04) -> Offset ~0.16, der sich bei jeder Wieder-
        // holung zum Setback akkumulierte. updateSwimming-CONTINUE (bereits am Schwimmen) braucht
        // nur Sprint + Wasserberuehrung, -START zusaetzlich volles Untertauchen (isSubmergedInWater).
        // Player.updateSwimming: fliegend nie (und Player.isSwimming ist fuer Zuschauer immer aus).
        self.raw_swimming = if was_flying || input.spectator {
            false
        } else if self.raw_swimming {
            self.sprinting && touching_water
        } else {
            self.sprinting && touching_water && underwater
        };

        // ACHTUNG, hier steht bewusst KEIN Pose-Update mehr. Die Hitbox-Pose wird am ENDE
        // von tick() gesetzt (s. update_pose): Vanillas PlayerEntity.tick() ruft updatePose()
        // als allerletzte Anweisung, nach super.tick() und damit nach travel(). Die Pose, mit
        // der ein Tick seine Bewegung rechnet, ist also immer die vom ENDE des VORIGEN Ticks.
        // self.swimming/self.crouching sind an dieser Stelle folglich genau das -- was fuer
        // die Fluessigkeits-Box oben (current_height) und crawling_move unten auch gewollt ist.
        //
        // ECHTER BUG gewesen (Flags beim Schleichen direkt in einen 1.5-Bloecke-Gang und beim
        // Aufstehen aus dem Kriechen): vorher stand die Pose-Aufloesung HIER, also VOR der Bewegung und mit der
        // Sneak-Taste DIESES Ticks. Damit wechselte unsere Hitbox einen ganzen Tick frueher
        // als bei Vanilla/GrimAC. Am Umschalt-Tick rechneten wir mit 1.5, waehrend Grims
        // Simulation (GrimPlayer.boundingBox stammt aus updatePlayerPose vom Ende des VORIGEN
        // Ticks, MovementCheckRunner Zeile 436) noch 1.8 ansetzte: wir kamen in den Gang
        // hinein, Grims Vorhersage stiess an die Decke -> Offset in X/Z -> Simulation-Flag.
        // Beim Aufstehen aus dem Crawlen dasselbe mit umgekehrtem Vorzeichen.
        //
        // Bewegungs-Verlangsamung (in_sneaking_pose_move, s. Feld-Kommentar) hat davon
        // unabhaengig ihren eigenen Zeitpunkt: sie gehoert zu tickMovement, laeuft also VOR
        // der Bewegung, mit den Posen-Pruefungen an der Position vom Ende des letzten Ticks.
        // isInSneakingPose, nach GrimACs PlayerBaseTick (im Grim-Jar nachgesehen) --
        // und die deckt sich mit Vanillas ClientPlayerEntity.tickMovement:
        //
        //   isSlowMovement = !wasFlying && !isSwimming
        //       && canEnterPose(CROUCHING, lastX, lastY, lastZ)
        //       && (wasSneaking || !isInBed && !canEnterPose(STANDING, lastX, lastY, lastZ))
        //     || (pose == SWIMMING || ...) && !wasTouchingWater
        //
        // Zwei Dinge daran sind entscheidend, und beide hatte ich zwischendurch falsch:
        //   - `wasSneaking`, also die Taste vom LETZTEN Tick. Der Faktor laggt der Taste in
        //     BEIDE Richtungen um einen Tick nach, nicht nur beim Loslassen. Ich hatte hier
        //     zeitweise `input.sneak || prev_sneak` stehen ("beim Druecken sofort") -- das war
        //     aus einer Messung geraten, die eine andere Ursache hatte.
        //   - Die Posen-Pruefungen laufen auf der Position vom ENDE des letzten Ticks. Genau
        //     das ist self.pos an dieser Stelle: bewegt wird erst weiter unten.
        let steh_passt = pose_fits(self.pos, self.pose_height(PLAYER_HEIGHT), world, shapes, self.shape_context(), self.body);
        let hock_passt = pose_fits(self.pos, self.pose_height(CROUCHING_HEIGHT), world, shapes, self.shape_context(), self.body);
        // LocalPlayer.aiStep: `crouching = !abilities.flying && ...` -- fliegend schleicht niemand.
        let in_sneaking_pose = !was_flying && !self.raw_swimming && hock_passt && (self.prev_sneak || !steh_passt);
        // shouldSlowDown() = isInSneakingPose() || isCrawling() (ClientPlayerEntity Zeile 641-642)
        // -- BEIDE loesen in applyMovementSpeedFactors den ×0.3-Bewegungsfaktor (SNEAKING_SPEED)
        // aus, nicht nur Sneak. isCrawling() = isInSwimmingPose() && !isTouchingWater() (Entity
        // Zeile 2666-2667): die Pose ist noch SWIMMING, die Fuesse aber schon aus dem Wasser --
        // genau der Wasseraustritts-Tick beim Sprint-Rausschwimmen. Vanillas updatePose() laeuft
        // am ENDE des Ticks (PlayerEntity.tick Zeile 292, NACH super.tick()->tickMovement); die
        // fuer die Bewegung gelesene Pose ist also IMMER die vom ENDE des LETZTEN Ticks -- und
        // genau das ist self.swimming an dieser Stelle (aktualisiert wird es erst unten in
        // update_pose). Deshalb reicht der aktuelle touching_water-Stand (water_height dieses
        // Ticks) als !isTouchingWater.
        //
        // ECHTER BUG gewesen (s. test 13, 0.0178 Z-Offset beim Sprint-Rausschwimmen, algebraisch
        // aus Grims P/A hergeleitet): wir liessen den ×0.3 am Austritts-Tick weg und schickten
        // 0.026×0.98×1.0 = 0.02548 Accel, waehrend Grim (= Vanilla, gecrawlt) 0.026×0.98×0.3 =
        // 0.007644 erwartete -> genau 0.0178 Differenz in Z, danach mit 0.91/Tick abklingend.
        //
        // Dasselbe nach dem Elytra-Flug: LivingEntity.isInSwimmingPose zaehlt auch die Pose
        // GLIDING, sobald das Gleit-Flag aus ist. Beendet der Server den Flug (Landung), steht
        // die Pose im naechsten Tick noch auf GLIDING -- der Tick gilt als Kriechen, mit
        // ×0.3. Sichtbar als Grim-Simulation beim Elytra-Hoppen: 0.0178 zu viel Vorwaerts-
        // Beschleunigung im ersten Luft-Tick nach dem Absprung, danach abklingend.
        let glide_pose_ohne_flug = self.glide_pose && !self.gliding;
        let crawling_move = (self.swimming || glide_pose_ohne_flug) && water_height.is_none();
        self.in_sneaking_pose_move = in_sneaking_pose || crawling_move;
        self.prev_sneak = input.sneak;

        // Diagnose CORAL_SNEAK_DEBUG=1 -- schreibt GENAU die Terme, aus denen sich der
        // ×0.3-Faktor ergibt (s. add_move_input). Anlass: ein consoledebug-Mitschnitt zeigte
        // schleichend dauerhaft "Client input 0.098 / Simulated input 0.0294", der Faktor
        // fehlte also bei uns -- auf ebenem Boden stimmt er aber exakt (s. Test
        // schleichen_ist_exakt_drei_zehntel_der_gehgeschwindigkeit). Die Abweichung haengt
        // folglich am Ort, und nur diese Zeile sagt, welcher der Zweige dort kippt: die
        // Sneak-Taste selbst, die Posen-Aufloesung (passt 1.8/1.5 ueberhaupt?) oder die
        // Schwimm-/Kriech-Sonderfaelle.
        if std::env::var_os("CORAL_SNEAK_DEBUG").is_some() {
            eprintln!(
                "[tick] pos=({:.4},{:.4},{:.4}) vor={:.1} strafe={:.1} sneak={} benutzt={} prev={} crouch={} swim={} raw_swim={} kriechen={} pose_move={} passt1.8={} passt1.5={} faktor={:.4}",
                self.pos.x,
                self.pos.y,
                self.pos.z,
                input.forward,
                input.strafe,
                input.sneak,
                input.using_item,
                self.prev_sneak,
                self.crouching,
                self.swimming,
                self.raw_swimming,
                crawling_move,
                self.in_sneaking_pose_move,
                steh_passt,
                hock_passt,
                0.98 * if input.using_item { input.item_use_speed } else { 1.0 } * if self.in_sneaking_pose_move { DEFAULT_SNEAKING_SPEED } else { 1.0 },
            );
        }

        // Vanillas Herausschieben aus einem Block (LocalPlayer.aiStep, moveTowardsClosestSpace):
        // ohne noPhysics wird an den vier Ecken (x +/- 0.35*Breite, z +/- 0.35*Breite) je einmal
        // Richtung freien Platz geschoben.
        //
        // Die PHASE ist festgelegt: in aiStep direkt nach input.tick() und dem Auto-Sprung,
        // also VOR der Sprint-Entscheidung und lange vor travel() -- s. Modulkommentar zu den
        // vier Tick-Phasen.
        //
        // Nicht mit noPhysics (Zuschauer).
        if !input.spectator {
            for (dx, dz) in [(-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (1.0, 1.0)] {
                let ecke = self.width() * 0.35;
                self.move_towards_closest_space(self.pos.x + dx * ecke, self.pos.z + dz * ecke, world, shapes);
            }
        }

        // Das `!sneak` gilt NUR am Boden/ausserhalb der Schwimm-Pose. ECHTER BUG gewesen (im
        // Schwimmen schleichend schwamm man langsamer als in Vanilla):
        // beim Sprint-Schwimmen stoppt Sneak den Sprint in Vanilla NICHT -- ClientPlayer-
        // Entity.tickMovement prueft waehrend isSwimming() ausschliesslich shouldStopSwim-
        // Sprinting(), und das kennt bei Vorwaertsbewegung keinen Sneak-Stop (nur
        // !canSprint || !touchingWater). Sneak zieht einen also per knockDownwards nach unten,
        // OHNE die Schwimm-Geschwindigkeit zu killen. Vorher haben wir den Sprint faelschlich
        // beendet -> zu langsam -> GrimAC-Simulation-Flag beim Abtauchen.
        //
        // `|| underwater`: ECHTER BUG gewesen (im Wasser Sneak + W fuehrt in Vanilla in den
        // Schwimm-/Sprint-Modus, hier nicht). canStartSprinting() hebt die
        // Sneak-Sperre auf, sobald man vollstaendig untergetaucht ist:
        // `(!shouldSlowDown() || isSubmergedInWater())` (ClientPlayerEntity Zeile 1091). Ohne
        // dieses Oder konnte man untergetaucht + Sneak nie einen Sprint starten -> kam nie in die
        // Schwimm-Pose. underwater == is_underwater() == isSubmergedInWater() (Auge im Wasser).
        // Sprint-Stop-Zweig an Vanillas isSwimming() (= self.raw_swimming, oben mit Ein-Tick-Lag
        // gesetzt) haengen, NICHT an der Schwimm-POSE (self.swimming). ECHTER BUG gewesen (Grim
        // consoledebug 01:29, O~0.0137 am Wasser->Luft-Austritt): am Austritts-Tick ist die Pose
        // noch Schwimmen, isSwimming (raw_swimming) aber schon aus (baseTick sah touching_water=
        // false). Vanilla nutzt dann die LAND-Regel (shouldStopSprinting -> Sprint laeuft weiter),
        // wir nutzten faelschlich die Schwimm-Regel (shouldStopSwimSprinting, touching_water=false
        // -> Stop) und schalteten den naechsten Tick wieder an -> ein-Tick-Sprint-Flacker ->
        // kurzer horizontaler Geschwindigkeitsversatz gegen Grim.
        // ECHTER BUG gewesen (Grim "SprintE", stableKey grim.sprint.wall, aus dem Grim-Jar
        // nachgesehen -- "Sprinting while colliding with a wall"): hier stand `!input.sneak`,
        // also die SNEAK-TASTE. Vanilla fragt an dieser Stelle aber die POSE ab
        // (canStartSprinting: `!shouldSlowDown() || isSubmergedInWater()`, und shouldSlowDown()
        // ist isInSneakingPose() || isCrawling()). Der Unterschied zaehlt genau dort, wo es
        // geflaggt hat: unter einer zu niedrigen Decke erzwingt Vanilla die Schleich-Pose auch
        // OHNE gedrueckte Taste und kann dort folglich keinen Sprint starten. Wir sahen nur die
        // Taste, starteten den Sprint an der Stufe immer wieder neu und meldeten ihn beim
        // Dagegendruecken weiter -- genau das Bild, das SprintE sucht.
        // Deshalb steht die Posen-Aufloesung jetzt VOR dieser Entscheidung: Vanillas
        // tickMovement setzt inSneakingPose als Allererstes, lange vor dem Sprint-Block.
        //
        // Und die Posen-Sperre gehoert AUSSCHLIESSLICH in den Start-Zweig: Vanilla hat sie nur
        // in canStartSprinting(), shouldStopSprinting() kennt keine Sneak-/Posen-Bedingung. Ein
        // LAUFENDER Sprint endet also nicht dadurch, dass man in die Schleich-/Kriech-Pose
        // kommt -- er endet an der Wand, im Wasser oder ohne Vorwaertseingabe.
        // VOLLSTAENDIGER Nachbau von Vanillas Sprint-Block. Zuvor stand hier
        // `wants_sprint = sprint_held && forward > 0` als GEMEINSAME Bedingung fuers Starten
        // UND Stoppen. Das war an drei Stellen falsch:
        //
        //   1. Vanillas Stopp-Bedingungen kennen die SPRINT-TASTE ueberhaupt nicht
        //      (shouldStopSprinting/shouldStopSwimSprinting fragen sie nicht ab). Wer im
        //      Lauf die Taste loslaesst, sprintet in Vanilla WEITER, bis er stehen bleibt
        //      oder gegen etwas laeuft. Wir hoerten sofort auf -- und meldeten das auch so
        //      an den Server.
        //   2. Nahrung und Blindheit fehlten ganz (canSprint -> !hasBlindnessEffect() &&
        //      canSprintOrFly(), und canSprintOrFly ist `foodLevel > 6`).
        //   3. Beim Schwimm-Sprint ist Vanillas Stopp milder: shouldStopSwimSprinting()
        //      beendet ihn ohne Vorwaertseingabe NUR, wenn man dabei weder am Boden ist noch
        //      schleicht (`!hasForwardMovement && !isOnGround && !sneak`).
        //
        // canSprint(allowTouchingWater) aus ClientPlayerEntity, ohne Reittier (kein Fahrzeug
        // im Client) -- die Wasserbedingung kommt je Aufrufstelle dazu.
        // hasEnoughFoodToDoExhaustiveManoeuvres: wer fliegen darf, braucht keinen Hunger-Vorrat.
        let kann_sprinten_grund = !input.blindness && (input.food_level > 6 || self.abilities.may_fly);
        // isSprintingPossible(abilities.flying): fliegend zaehlt die Wasserberuehrung nicht.
        let wasser_sperrt = partly_touching_water && !was_flying;
        // canStartSprinting(): zusaetzlich Vorwaertseingabe, kein benutztes Item (Vanillas
        // isBlockedFromSprinting; UseEffectsComponent.DEFAULT hat can_sprint = false, also
        // sperrt jede laufende Benutzung), nicht in Schleich-/Kriech-Pose, nicht im Wasser.
        let kann_starten = kann_sprinten_grund
            && input.forward > 0.0
            && (!input.using_item || input.item_use_can_sprint)
            && !wasser_sperrt
            && (!self.in_sneaking_pose_move || underwater)
            // canStartSprinting: `(!isGliding() || isSubmergedInWater())`.
            && (!self.gliding || underwater);
        // Nur der START haengt an der Taste -- Vanilla setzt den Sprint nur, wenn
        // canStartSprinting() gilt und die Sprinttaste gehalten wird.
        if kann_starten && input.sprint_held {
            self.sprinting = true;
        }
        if self.sprinting {
            self.sprinting = if self.raw_swimming {
                // shouldStopSwimSprinting(): !canSprint(true) || !touchingWater
                //   || (!forward && !onGround && !sneak). canSprint(true) laesst die
                // Wasserbedingung weg.
                kann_sprinten_grund && touching_water && (input.forward > 0.0 || self.on_ground || input.sneak)
            } else {
                // shouldStopRunSprinting(): !isSprintingPossible(flying) || !hasForwardImpulse()
                //   || (horizontalCollision && !minorHorizontalCollision). isSprintingPossible(false)
                // enthaelt die Wasserbedingung.
                //
                // ECHTER BUG gewesen (GrimAC "SprintE" = grim.sprint.wall, etwa im
                // Kaktus-Labyrinth auf anticheat-test.com): hier stand nur
                // `!self.horizontal_collision`, das minorHorizontalCollision fehlte ganz. Wir
                // brachen den Sprint also auch beim blossen STREIFEN einer Wand ab, wo Vanilla
                // weitersprintet -- und starteten ihn bei gehaltener Sprinttaste im naechsten Tick
                // sofort neu. Dieses Aufblinken (Stop/Start je Tick) ist genau das Muster, das
                // SprintE als "sprintet gegen eine Wand" auslegt, und die Geschwindigkeits-
                // differenz Gehen/Sprinten ging zusaetzlich jeden Tick in Simulation ein.
                kann_sprinten_grund
                    && input.forward > 0.0
                    && !wasser_sperrt
                    && (!self.horizontal_collision || self.minor_horizontal_collision)
            };
        }

        // ClientPlayerEntity.tickMovement, direkt nach dem Sprint-Block: Sprung in der Luft
        // DRUECKEN (nicht halten -- die Taste vom letzten Tick muss los gewesen sein), nicht an
        // einer Leiter, und PlayerEntity.checkGliding: noch nicht am Gleiten, gleitfaehig
        // (canGlide) und nicht im Wasser. Dann startGliding() SOFORT lokal -- die Bewegung
        // dieses Ticks laeuft schon als Gleitflug -- und START_FALL_FLYING an den Server.
        //
        // canGlide: nicht am Boden, nicht auf einem Reittier (gibt es hier nicht), kein
        // Schwebeeffekt, nicht im Kreativflug (gibt es hier nicht), Elytra angelegt.
        let sprung_gedrueckt = input.jump && !self.prev_jump;
        self.prev_jump = input.jump;
        // LocalPlayer.aiStep, direkt nach dem Sprint-Block: Flug umschalten. Zuschauer fliegen
        // immer; sonst schalten zwei Sprung-Druecke im Fenster um (nicht schwimmend), und wer
        // vom Boden aus losfliegt, springt dabei ab.
        let mut flug_umgeschaltet = false;
        if self.abilities.may_fly {
            if input.spectator {
                if !self.abilities.flying {
                    self.abilities.flying = true;
                    flug_umgeschaltet = true;
                    self.abilities_changed = true;
                }
            } else if sprung_gedrueckt {
                if self.jump_trigger_time == 0 {
                    self.jump_trigger_time = FLY_TOGGLE_WINDOW;
                } else if !self.raw_swimming {
                    self.abilities.flying = !self.abilities.flying;
                    if self.abilities.flying && self.on_ground {
                        self.try_jump(input, world);
                    }
                    flug_umgeschaltet = true;
                    self.abilities_changed = true;
                    self.jump_trigger_time = 0;
                }
            }
        }
        // Player.canGlide: nicht im Kreativflug -- und nicht in dem Tick, der ihn umschaltet.
        if sprung_gedrueckt
            && !flug_umgeschaltet
            && !self.abilities.flying
            && !self.is_climbing(world)
            && !self.gliding
            && !self.on_ground
            && input.levitation.is_none()
            && input.glider_equipped
            && !touching_water
        {
            self.gliding = true;
            self.glide_start_requested = true;
        }

        // LocalPlayer.aiStep: fliegend heben Springen und Schleichen um je das Dreifache der
        // Fluggeschwindigkeit (float wie in Vanilla). Danach Player.aiStep: das Fenster laeuft
        // ab, und fliegend gibt es keine Fallhoehe.
        if self.abilities.flying {
            let richtung = i32::from(input.jump) - i32::from(input.sneak);
            if richtung != 0 {
                self.vel.y += f64::from(richtung as f32 * self.abilities.fly_speed * 3.0);
            }
        }
        self.jump_trigger_time = self.jump_trigger_time.saturating_sub(1);
        if self.abilities.flying {
            self.fall_distance = 0.0;
        }

        // LivingEntity.tickMovement(): jumpingCooldown wird JEDEN Tick unbedingt
        // runtergezaehlt, danach -- unabhaengig von Wasser/Lava/Luft -- sofort auf 0
        // zurueckgesetzt, sobald Springen nicht (mehr) gehalten wird (s. try_jump-
        // Kommentar). Muss vor dem Wasser/Lava/Luft-Dispatch stehen, weil beide Zweige
        // denselben Zaehler lesen.
        if self.jump_cooldown > 0 {
            self.jump_cooldown -= 1;
        }
        if !input.jump {
            self.jump_cooldown = 0;
        }

        // LivingEntity.tickMovement(): sehr kleine Restgeschwindigkeit JEDEN Tick hart auf
        // 0 klemmen, BEVOR travel() sie weiterverarbeitet -- sonst naehert sich Reibung nur
        // asymptotisch an 0 an und bleibt fuer immer ein winziges Stueck von 0 entfernt.
        // Spieler sind dabei ein Sonderfall: X/Z werden GEMEINSAM genullt (nicht pro Achse
        // einzeln wie bei anderen Entities), sobald die HORIZONTALE Geschwindigkeit
        // (Quadrat) unter 9.0E-6 faellt (= Betrag < 0.003); Y unabhaengig bei |vel.y| <
        // 0.003. Bisher komplett gefehlt -- echter Bug gewesen (dauerhafter
        // kleiner Simulation-Restfehler bei Grim, ~0.004, weil Grims eigene Simulation
        // diesen Schnitt macht und unsere nicht).
        if self.vel.x * self.vel.x + self.vel.z * self.vel.z < 9.0e-6 {
            self.vel.x = 0.0;
            self.vel.z = 0.0;
        }
        if self.vel.y.abs() < 0.003 {
            self.vel.y = 0.0;
        }

        // ZURUECKGENOMMEN: ein Ein-Tick-Lag auf dem Medium-Dispatch
        // (Wasser/Luft) wurde versucht, um einen sehr schmalen Oberflaechen-Sonderfall zu
        // fixen -- hat aber SYMMETRISCH auch den viel haeufigeren Wasser-EINTRITT verzoegert
        // und dadurch insgesamt MEHR Flags ausgeloest, nicht weniger. Nur an einem einzelnen
        // synthetischen Fall verifiziert, nicht gegen normales Spielverhalten getestet -- war
        // ein echter Fehler, nicht nur unbestaetigt. Dispatch bleibt bis auf Weiteres live
        // (water_height/lava_height von diesem Tick), wie vor diesem Versuch.
        // Fuer die Posen-Diagnose (s. unten): Position VOR der Bewegung dieses Ticks.
        let pos_vor_bewegung = self.pos;
        // LivingEntity.tickGliding -> limitFallDistance, vor travel(): solange man nicht
        // steil stuerzt, zaehlt die Fallhoehe hoechstens einen Block.
        if self.gliding && self.vel.y > -0.5 && self.fall_distance > 1.0 {
            self.fall_distance = 1.0;
        }
        self.touching_water = water_height.is_some();
        // Player.travel: fliegend laeuft das normale travel, danach gilt aber die vertikale
        // Geschwindigkeit von VORHER, mal 0.6 -- Schwerkraft und Luftbremse fallen damit weg.
        // Fluessigkeiten wirken fliegend nicht (isAffectedByFluids).
        let fliegt = self.abilities.flying;
        let y_vorher = self.vel.y;
        let zuschauer_flug = input.spectator && input.spectator_speed > 0.0;
        if zuschauer_flug {
            // Entity.move mit noPhysics: einfach versetzen.
            self.vel = spectator_velocity(input);
            self.pos += self.vel;
            self.horizontal_collision = false;
            self.minor_horizontal_collision = false;
        } else if let (Some(g), false) = (&water_height, fliegt) {
            self.tick_water(input, world, shapes, g.height);
        } else if let (Some(g), false) = (&lava_height, fliegt) {
            self.tick_lava(input, world, shapes, g.height);
        } else if self.gliding {
            self.tick_glide(input, world, shapes);
        } else {
            self.tick_air(input, world, shapes);
        }
        if fliegt && !zuschauer_flug {
            self.vel.y = y_vorher * 0.6;
        }

        // LivingEntity.tickMovement(): direkt NACH travel() -- diese Reihenfolge ist der
        // ganze Punkt (s. movement_multiplier). Zuschauer beruehren keine Bloecke.
        if !input.spectator {
            self.tick_block_collision(input, world, shapes);
        }
        // Ende von LocalPlayer.aiStep: wer fliegend aufsetzt, hoert auf zu fliegen (Zuschauer nie).
        if self.on_ground && self.abilities.flying && !input.spectator {
            self.abilities.flying = false;
            self.abilities_changed = true;
        }

        // updatePose() als ALLERLETZTES im Tick, an der Position NACH der Bewegung -- genau
        // wie PlayerEntity.tick() (Zeile 292 im Yarn-1.21.11-Jar) und wie GrimAC
        // (MovementCheckRunner Zeile 436: doBaseTick -> livingEntityAIStep -> updatePlayerPose).
        let was_swimming = self.swimming;
        let was_crouching = self.crouching;
        self.update_pose(input, world, shapes);

        // Posen-Diagnose (CORAL_POSE_DEBUG=1): genau EINE Zeile pro Posenwechsel, mit
        // allem, was noetig ist, um sie gegen GrimACs consoledebug zu legen -- Grims A-Zeile
        // (actualMovement) gehoert zu derselben Position wie `nach` hier, und die Pose, mit
        // der Grim DIESEN Tick gerechnet hat, ist die Spalte `vor`. Wer beim Posenwechsel
        // flaggt, sieht hier sofort, ob unsere Umschaltung einen Tick vor oder hinter Grims
        // liegt, statt dass wieder geraten wird.
        if (was_swimming != self.swimming || was_crouching != self.crouching) && std::env::var_os("CORAL_POSE_DEBUG").is_some() {
            let name = |swim: bool, crouch: bool| {
                if swim {
                    "SWIMMING"
                } else if crouch {
                    "CROUCHING"
                } else {
                    "STANDING"
                }
            };
            eprintln!(
                "[pose] {} -> {} (sneak={} raw_swim={}) hoehe {:.1} -> {:.1} vor=({:.5},{:.5},{:.5}) nach=({:.5},{:.5},{:.5})",
                name(was_swimming, was_crouching),
                name(self.swimming, self.crouching),
                input.sneak,
                self.raw_swimming,
                self.pose_height(if was_swimming {
                    SWIMMING_HEIGHT
                } else if was_crouching {
                    CROUCHING_HEIGHT
                } else {
                    PLAYER_HEIGHT
                }),
                self.current_height(),
                pos_vor_bewegung.x,
                pos_vor_bewegung.y,
                pos_vor_bewegung.z,
                self.pos.x,
                self.pos.y,
                self.pos.z,
            );
        }

        // TEMPORAERES Debug-Logging (CORAL_SWIM_DEBUG=1), um das Flackern beim
        // Kopfanstossen im Schwimmen an echten Spieldaten nachzuvollziehen, statt weiter blind
        // Szenarien zu konstruieren: mehrere synthetische Repros blieben stabil, der Bug trat im echten
        // Spiel aber weiterhin auf.
        if was_swimming != self.swimming && std::env::var_os("CORAL_SWIM_DEBUG").is_some() {
            eprintln!(
                "[swim] {} -> {} pos={:?} sprinting={} water_height={:?} underwater={} eye_y={:.4}",
                was_swimming,
                self.swimming,
                self.pos,
                self.sprinting,
                water_height.as_ref().map(|s| s.height),
                underwater,
                self.eye_pos().y
            );
        }
    }
}
