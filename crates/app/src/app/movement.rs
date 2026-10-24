// Bewegung, Fahrzeuge und Positionspakete.

use crate::*;

impl App {
    pub(crate) fn update_movement(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_movement_update).as_secs_f32();
        self.last_movement_update = now;

        // Getickt wird, solange die VERBINDUNG steht -- nicht, solange der Weltbildschirm
        // oben ist. Vanilla pausiert den Tick NUR im Einzelspieler: MinecraftClient.paused
        // ist `isIntegratedServerRunning() && currentScreen.shouldPause() && !server
        // .isRemote()` (1.21.11 nachgesehen). Auf einem echten Server laeuft der
        // Client-Tick mit offenem Pausemenue/Einstellungen ungebremst weiter, samt
        // Bewegungspaketen und ClientTickEnd.
        //
        // ECHTER BUG gewesen (Grim "BadPacketsR" = grim.badpackets.position_starvation):
        // hier stand `if self.mode != AppMode::World`, der Client stellte also mit dem Druck
        // auf Escape sofort das Senden von Positionspaketen ein -- waehrend der Netz-Task
        // die Transaktionen (Ping/Pong) munter weiter beantwortete. Genau dieses Muster
        // sucht der Check: ueber 2 Sekunden kein PLAYER_POSITION(_AND_ROTATION), obwohl
        // Transaktionen beantwortet werden. Ein Vanilla-Client kann das nicht erzeugen,
        // deshalb reicht Grim schon ein einziges Pausemenue von mehr als zwei Sekunden.
        if self.conn_status != ConnStatus::Connected {
            // Ohne Verbindung gibt es nichts zu ticken -- und der alte Tick-Termin MUSS hier
            // weg. Bleibt er stehen, zeigt er nach dem Verlassen der Welt in die
            // Vergangenheit, next_wake liefert einen bereits verstrichenen Zeitpunkt, und die
            // Fensterschleife wacht ohne Pause wieder auf: ein Busy-Loop im Menue, der einen
            // Kern voll auslastet.
            self.ticking = false;
            self.tick_deadline = None;
            return;
        }

        // Eigene Spalte noch nicht geladen? Dann Zeitguthaben GAR NICHT erst aufstauen (s.
        // unten fuers eigentliche Tick-Gating). ECHTER BUG gewesen (der Spieler
        // wurde sofort nach dem ersten Bewegen zum Spawn zurueckteleportiert): vorher lief
        // dt hier IMMER in physics_accum, auch waehrend der Ladewartezeit -- sobald die
        // Spalte fertig war, wurden dadurch auf einen Schlag bis zu 5 aufgestaute Ticks in
        // unter 1ms Echtzeit nachgeholt (im Log klar sichtbar: 5 Bewegungspakete mit
        // identischem Zeitstempel direkt nach dem Laden). Das sieht fuer Grim ununterscheidbar
        // von einem Timing-Cheat aus (5 Spielzuege in einer Millisekunde statt ueber 250ms
        // verteilt) und wurde sofort mit einer harten Spawn-Korrektur beantwortet.
        let (player_cx, player_cz) = (
            (self.physics.pos.x / world::SECTION_SIZE as f64).floor() as i32,
            (self.physics.pos.z / world::SECTION_SIZE as f64).floor() as i32,
        );
        let own_column_loaded = self.world.column_arc(player_cx, player_cz).is_some();

        // Das Zeitguthaben laeuft jetzt IMMER, auch ohne geladene Spalte -- es wird unten in
        // jedem Fall verbraucht, entweder als voller Tick oder als Warte-Tick (s.
        // wait_tick). Frueher stand hier `else { 0.0 }`: das verhinderte zwar den
        // Nachhol-Schub, liess aber die Bewegungspakete waehrend der Ladewartezeit komplett
        // ausfallen -- dieselbe BadPacketsR-Luecke wie beim Pausemenue oben, nur beim Join.
        // Der Warte-Tick loest beides zugleich, weil er das Guthaben verbraucht, ohne Physik
        // zu rechnen: es kann sich nichts aufstauen, was danach in einer Millisekunde
        // nachgeholt wuerde.
        //
        // Die Nachhol-Deckelung liegt bei ZEHN Ticks, nicht mehr bei fuenf: Vanillas
        // Renderschleife holt pro Frame `Math.min(10, i)` Ticks nach und verwirft den Rest
        // (MinecraftClient.java, 1.21.11 nachgesehen) -- der Rest der Zeit ist damit auch
        // dort verloren. Mit nur fuenf verwarfen WIR Ticks in Lagen, in denen Vanilla noch
        // sauber aufholt (jeder Haenger zwischen 250 und 500 ms), und genau das ist Grims
        // NegativeTimer: der Check schreibt pro Tickpaket 50 ms gut und flaggt, sobald das
        // Guthaben mehr als 1200 ms hinter der Echtzeit liegt -- verworfene Ticks sind
        // dauerhaft fehlende 50-ms-Gutschriften.
        // Die Deckelung ist zugleich die EINZIGE Stelle, an der Tickzeit dauerhaft verloren
        // geht -- und genau die zaehlt GrimACs NegativeTimer mit ("Sent movement packets
        // slower than the expected client tick rate"): der Check schreibt pro Tickpaket 50 ms
        // gut und meldet, sobald das Guthaben mehr als 1200 ms hinter der Echtzeit liegt. Ein
        // Haenger von mehr als 500 ms verwirft hier also Ticks, deren 50-ms-Gutschriften Grim
        // nie erhaelt -- und die Schuld bleibt fuer den Rest der Sitzung stehen.
        //
        // Deshalb wird sie gemessen und geloggt: sonst raet man beim naechsten
        // NegativeTimer-Flag wieder, ob die Physik schuld ist oder schlicht ein Ruckler.
        let gewuenscht = self.physics_accum + dt;
        self.physics_accum = gewuenscht.min(physics::TICK_DT * 10.0);
        let verworfen = gewuenscht - self.physics_accum;
        if verworfen > 0.0 {
            self.verworfene_tickzeit += verworfen;
            let now = Instant::now();
            if now.duration_since(self.letzte_tickverlust_meldung) >= Duration::from_secs(2) {
                self.letzte_tickverlust_meldung = now;
                eprintln!(
                    "[tick] Hänger: {:.0} ms Tickzeit verworfen ({:.0} Ticks), Sitzungssumme {:.2} s -- so entsteht Grims NegativeTimer",
                    verworfen * 1000.0,
                    verworfen / physics::TICK_DT,
                    self.verworfene_tickzeit
                );
            }
        }
        // Wird gleich unten (nach der Tick-Schleife) in einen Weckzeitpunkt umgerechnet.
        self.ticking = true;

        // Bei offenem Inventar steht der Spieler still (Vanilla) -- die Tasten behalten
        // ihren Zustand (auch die Umschalt-Modi), wirken aber nicht auf die Physik. Gate
        // hier statt in action_active, weil die Physik die Felder direkt liest und ein
        // umgebogenes action_active die Umschalt-Logik durcheinanderbraechte.
        // SnapTap: bei zwei gegensaetzlichen Richtungen gewinnt die zuletzt gedrueckte,
        // statt dass sie sich aufheben (s. Settings::snap_tap_enabled). Ohne den Schalter
        // bleibt es bei Vanillas Verhalten -- beide gedrueckt heisst Stillstand. Bildschirm-
        // Gate steckt mit drin (s. movement_input_flags).
        let (move_forward, move_backward, move_left, move_right, jump, sneak, sprint) = self.movement_input_flags();
        // PlayerEntity.isImmobile() ist `super.isImmobile() || isSleeping()`, und Vanillas
        // tickMovement nullt daraufhin forwardSpeed/sidewaysSpeed/jumping. travel() laeuft
        // trotzdem weiter -- der Schlafende sinkt also von der Einlegehoehe (0.6875) auf die
        // Matratze (9/16), genau wie in Vanilla. Deshalb hier nur die Eingabe nullen und
        // nicht etwa die Physik anhalten.
        //
        // Die Schleichtaste ist bewusst ausgenommen: sie ist unser "Bett verlassen"
        // (s. unten). Vanilla nutzt dafuer den Schlaf-Bildschirm, den wir nicht haben.
        let schlaeft = self.sleeping_at.is_some();
        let (move_forward, move_backward, move_left, move_right, jump, sprint) = if schlaeft {
            (false, false, false, false, false, false)
        } else {
            (move_forward, move_backward, move_left, move_right, jump, sprint)
        };
        if schlaeft {
            // LivingEntity.tickMovement(): `if (isSleeping()) setPitch(0)`. Die Neigung geht so
            // auch ins Bewegungspaket -- Kamera und gesendete Rotation bleiben damit einig.
            self.camera.pitch = 0.0;
            if sneak {
                self.leave_bed();
            }
        }
        let mut forward = 0.0f32;
        if move_forward {
            forward += 1.0;
        }
        if move_backward {
            forward -= 1.0;
        }
        // Vanillas eigene Konvention ist hier gegenlaeufig zur Intuition: KeyboardInput.tick()
        // berechnet den Strafe-Wert als calculateImpulse(left, right), und dessen erstes
        // Argument (left) bestimmt bei genau einer gedrueckten Taste das Vorzeichen -- Links
        // gedrueckt ergibt +1, Rechts -1 (aus dem 1.21.11-Client verifiziert).
        // Vorher stand das hier vertauscht, was Links/Rechts im Spiel spiegelverkehrt machte.
        let mut strafe = 0.0f32;
        if move_left {
            strafe += 1.0;
        }
        if move_right {
            strafe -= 1.0;
        }
        // `mut`, weil using_item erst NACH flush_input_actions() feststeht -- s. dort.
        let mut input = physics::Input {
            forward,
            strafe,
            jump,
            sneak,
            sprint_held: sprint,
            using_item: self.using_item,
            item_use_speed: use_speed_f64(self.own_use_effects().speed_multiplier),
            item_use_can_sprint: self.own_use_effects().can_sprint,
            food_level: self.food_level,
            blindness: self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_BLINDNESS),
            effect_speed_factor: self.effect_speed_factor(),
            movement_speed: self.attr_movement_speed,
            sneaking_speed: self.attr_sneaking_speed,
            water_movement_efficiency: self.attr_water_movement_efficiency,
            slow_falling: self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_SLOW_FALLING),
            levitation: self.own_effects.get(&protocol::packets::MOB_EFFECT_LEVITATION).copied(),
            jump_boost: self.own_effects.get(&protocol::packets::MOB_EFFECT_JUMP_BOOST).copied(),
            dolphins_grace: self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_DOLPHINS_GRACE),
            weaving: self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_WEAVING),
            float_drag: self.settings.module_botcheck_pass,
            yaw_rad: self.camera.yaw.to_radians(),
            // Vanillas getRotationVector().y nutzt MathHelper.sin (Float-Tabelle), NICHT exaktes
            // f64-sin -- fuer bit-genaue Schwimm-Physik gegen Grim, s. physics::look_dir_y_from_pitch.
            look_dir_y: physics::look_dir_y_from_pitch(self.camera.pitch),
            yaw_deg: self.camera.yaw,
            pitch_deg: self.look_pitch_deg(),
            glider_equipped: self.own_glider_equipped(),
            spectator: self.game_mode == GAME_MODE_SPECTATOR,
            spectator_speed: self.settings.spectator_speed as f64 / 20.0,
        };
        // LocalPlayer.isControlledCamera: wer durch fremde Augen sieht, bewegt sich nicht
        // selbst (das Eingabe-Paket mit Schleichen geht trotzdem raus -- damit verlaesst man
        // die fremde Sicht).
        if self.spectated_entity().is_some() {
            input.forward = 0.0;
            input.strafe = 0.0;
            input.jump = false;
            input.sneak = false;
        }

        // Eigene Spalte noch nicht geladen? Dann NICHT ticken (auch keine Schwerkraft) --
        // sonst faellt der Spieler kurz durch die noch leere Welt, bevor die echten
        // Chunk-Daten ankommen (Play-State-Chunks treffen asynchron nach dem PlayerPosition-
        // Sync ein, s. camera_placed-Setzung). Genau das war ein echter Bug:
        // beim allerersten Landen rutscht man dadurch einen ganzen Block zu tief in den
        // Boden (Grim: "GroundSpoof"/"Simulation" ab dem ersten Tick, nie wieder erholt --
        // laut net_debug.log Landung bei Y=-13 statt Y=-12 direkt nach Spawn).
        // Vanilla haelt den Spieler ebenso fest, bis die eigene Spalte da ist (kein "durchs
        // Nichts fallen" beim Verbinden). own_column_loaded wurde oben schon fuers
        // physics_accum-Gating berechnet, hier nur weiterverwendet.

        while self.physics_accum >= physics::TICK_DT {
            self.physics_accum -= physics::TICK_DT;
            self.profiler.tick();
            self.addon_tick();
            // Vanillas ClientPacketListener.tick() -> notifyPlayerLoaded() steht vor dem
            // Spieler-Tick: erst meldet sich der Client fertig, dann darf LocalPlayer.tick()
            // ueberhaupt etwas tun.
            self.tick_player_loaded();
            // Noch nicht fertig geladen: LocalPlayer.tick() laeuft bei Vanilla gar nicht, es
            // geht also weder Eingabe noch Position raus. Das ClientTickEnd schickt Vanilla
            // trotzdem -- das steht in Minecraft.tick() ausserhalb dieser Sperre.
            if !self.client_loaded {
                self.send_client_tick_end();
                continue;
            }
            // Spalte noch nicht da: nur die Pakete, keine Physik (s. wait_tick).
            if !own_column_loaded {
                self.wait_tick();
                continue;
            }
            // Zustand VOR diesem Tick sichern -- das ist der Interpolations-Startpunkt
            // fuers Rendern, bis der naechste Tick faellig wird.
            self.physics_render_prev = self.physics.pos;
            // Erst die gepufferten Eingaben (Hotbar-Abgleich + Aktionen), dann die Physik --
            // Vanillas Reihenfolge im Client-Tick, und die einzige, die Grims Post-Check
            // durchgehen laesst (s. flush_input_actions).
            // Vanillas PlayerEntity.tick() zaehlt ticksSinceLastAttack hoch -- und zwar VOR
            // handleInputEvents, damit ein Schlag in diesem Tick den frisch erhoehten Stand
            // sieht. Der Deckel haelt den Zaehler nur klein, die Aufladung ist laengst voll.
            self.ticks_since_last_attack = (self.ticks_since_last_attack + 1).min(100);
            // ...und resetTicksSince(), wenn im VORIGEN Tick das Item in der Hand gewechselt hat
            // (s. unten).
            if std::mem::take(&mut self.attack_hand_changed) {
                self.ticks_since_last_attack = 0;
            }
            self.flush_input_actions();
            // Vanilla bemerkt den Itemwechsel erst im Spieler-Tick, NACH den Eingaben dieses
            // Ticks: Slottaste und Klick im selben Tick stechen also noch mit der Aufladung des
            // alten Items (Lunge-Swap auf den Speer). Unsere Slottasten wechseln schon beim
            // Druck -- verglichen wird deshalb erst hier, gewirkt hat es ab dem naechsten Tick.
            // Vanilla vergleicht den Stack, wir den Namen: fuer die Aufladung zaehlt nur, ob das
            // Item eine andere Angriffsgeschwindigkeit mitbringt.
            let hand = self.main_hand_item_name();
            if hand != self.last_attack_hand_item {
                self.last_attack_hand_item = hand;
                self.attack_hand_changed = true;
            }
            // ECHTER BUG gewesen (Grim "NoSlow" beim Spammen von Schild und Bogen):
            // `input` wird VOR der Tickschleife gebaut, using_item stand darin also noch auf
            // dem Stand vom VORIGEN Tick. tick_item_use() (in flush_input_actions) setzt es
            // aber genau hier -- die Verlangsamung begann und endete dadurch je einen Tick
            // zu spaet.
            //
            // Vanilla kennt das nicht: Minecraft.tick() ruft handleKeybinds() (und damit
            // Start/Ende der Benutzung) VOR level.tickEntities(), die Bewegung DIESES Ticks
            // sieht isUsingItem() also bereits im neuen Zustand.
            //
            // Warum es erst beim SPAMMEN auffiel: Grims NoSlow ist kein eigener Test, sondern
            // der Simulations-Offset, gezaehlt nur solange packetStateData.isSlowedByUsingItem()
            // gilt -- und es flaggt erst beim ZWEITEN Tick in Folge (flaggedLastTick). Beim
            // Halten gibt es nur je einen falschen Tick am Anfang und am Ende, beim Spammen
            // dagegen laufend welche, und damit auch aufeinanderfolgende.
            //
            // Grim nimmt diesen Zustand uebrigens NICHT aus unserem Paket, sondern aus den
            // Entity-Metadaten, die der Server daraufhin zurueckschickt (Bit 0x01, s.
            // PacketSelfMetadataListener), transaktions-synchronisiert. Unsere lokale
            // Verlangsamung muss also zu der Paketfolge passen, die wir senden.
            // Consumable Optimizer: die Verlangsamung endet mit dem lokalen Ende des Essens,
            // ohne auf den Server zu warten (s. consumable_optimizer.rs).
            input.using_item = self.using_item;
            let wirkung = self.own_use_effects();
            input.item_use_speed = use_speed_f64(wirkung.speed_multiplier);
            input.item_use_can_sprint = wirkung.can_sprint;
            // PlayerEntity.tick -> itemCooldownManager.update(): im Spieler-Tick, also NACH
            // handleInputEvents -- ein Klick im Tick des Ablaufs ist noch gesperrt.
            self.item_cooldowns.update();
            self.tick_bundle_hover();
            self.physics.jump_cooldown_enabled = self.settings.jump_cooldown_enabled;
            self.physics.sneak_edge_protection_enabled = self.settings.sneak_edge_protection_enabled;
            let before_tick = self.physics.pos;
            // Umhang, Teil 1 von 2. Vanillas Reihenfolge im Spieler-Tick, und sie ist hier
            // wichtig: AbstractClientPlayerEntity.tick() ruft state.tick(pos, velocity) VOR
            // super.tick(), also mit dem Stand vom Ende des letzten Ticks. Danach setzt
            // tickPlayerMovement die Laufgroesse -- ebenfalls noch vor der Bewegung, mit der
            // Geschwindigkeit von VORHER. Beides also hier, vor physics.tick().
            self.own_cape.tick([self.physics.pos.x, self.physics.pos.y, self.physics.pos.z]);
            let waagerecht = (self.physics.vel.x * self.physics.vel.x + self.physics.vel.z * self.physics.vel.z).sqrt() as f32;
            // AbstractClientPlayerEntity.tickPlayerMovement: nur am Boden, nicht tot, nicht
            // schwimmend -- sonst 0. Die Deckelung auf 0.1 begrenzt, wie weit der Umhang beim
            // Laufen ausschlaegt, unabhaengig von der Geschwindigkeit.
            let laufgroesse = if self.physics.on_ground && !self.is_dead && !self.physics.raw_swimming { waagerecht.min(0.1) } else { 0.0 };
            self.own_cape.tick_movement(laufgroesse);
            if self.own_ride.is_some() {
                // Reitend bewegt sich nicht der Spieler, sondern (falls man lenkt) das
                // Fahrzeug; der Spieler landet nach dem Entity-Tick auf seinem Sitz.
                let tasten = vehicle::BoatInput { left: move_left, right: move_right, forward: move_forward, back: move_backward };
                self.tick_own_vehicle(tasten);
            } else if self.tick_spectator_flight(&input) {
                // Klick-Teleport des Zuschauers: dieser Tick ist ein Stueck des Wegs.
            } else {
                // Zuschauer schiebt niemand (LivingEntity.isPushable).
                if self.game_mode != GAME_MODE_SPECTATOR {
                    let schub = self.entity_push();
                    self.physics.vel.x += schub.x;
                    self.physics.vel.z += schub.z;
                }
                self.physics.tick(&input, &self.world, &self.block_shapes);
            }
            // LivingEntity.tickRiptide -- direkt nach der Bewegung (s. physics::tick_riptide).
            if self.physics.riptide_ticks > 0 {
                let andere = self.riptide_entity_boxes();
                self.physics.tick_riptide(before_tick, &andere);
            }
            // Schrittgeraeusche -- in Entity.move, also direkt nach der Bewegung.
            self.tick_step_sounds(before_tick);
            // Flug umgeschaltet (Doppelsprung, Landung): LocalPlayer.onUpdateAbilities meldet es
            // noch aus aiStep, also vor dem Bewegungspaket dieses Ticks.
            if std::mem::take(&mut self.physics.abilities_changed) {
                if let Some(tx) = &self.outgoing_tx {
                    interaktions_log(if self.physics.abilities.flying { "abilities fliegt" } else { "abilities landet" });
                    let _ = tx.send(protocol::packets::player_abilities_payload(self.physics.abilities.flying));
                }
            }
            // Elytra-Start: lokal hat der Tick den Flug schon begonnen, jetzt das Paket --
            // Vanilla schickt es aus tickMovement, also vor dem Bewegungspaket dieses Ticks.
            if std::mem::take(&mut self.physics.glide_start_requested) {
                if let (Some(tx), Some(id)) = (&self.outgoing_tx, self.own_entity_id) {
                    interaktions_log("start_fall_flying");
                    let _ = tx.send(protocol::packets::player_command_start_fall_flying_payload(id));
                }
            }
            self.tick_firework_boost();
            // Schrittanimation: gefuettert mit der TATSAECHLICH zurueckgelegten Strecke
            // dieses Ticks, genau wie LivingEntity.updateLimbs (getX() - lastX). Nicht mit
            // der Geschwindigkeit -- die ist nach der Reibungsdaempfung nur rund halb so
            // gross, und damit war die ganze Schwingung zu klein und zu langsam (s.
            // world::LimbAnimator-Kommentar).
            let moved = ((self.physics.pos.x - before_tick.x).powi(2) + (self.physics.pos.z - before_tick.z).powi(2)).sqrt() as f32;
            self.own_limbs.tick(moved, 1.0);
            // Alter fuer die Ruhe-Bewegung der Arme -- eigenes und das aller Entities. Im
            // TICK, nicht beim Paketeintreffen: ein stillstehendes Entity schickt keine
            // Bewegungspakete, und gerade im Stillstand faellt eine eingefrorene Armhaltung auf.
            self.own_age_ticks += 1.0;
            // Wie bei fremden Entities: ein Tick, ein Schritt (s. world::Entity::tick).
            self.own_hurt_ticks = self.own_hurt_ticks.saturating_sub(1);
            self.tick_shield_cooldowns();
            self.tick_held_name();
            self.remember_worn_armor();
            self.tick_block_lids();
            self.tick_enchant_book();
            self.tick_trails();
            self.tick_death_bursts();
            self.tick_custom_sky();
            self.tick_weather();
            self.world.border.tick();
            self.own_ticks_since_hurt = self.own_ticks_since_hurt.saturating_add(1);
            self.own_crit_grace = self.own_crit_grace.saturating_sub(1);
            // Die Tageszeit zwischen zwei Server-Paketen selbst weiterdrehen -- aber nur, wenn
            // sie ueberhaupt laeuft (doDaylightCycle). Auf einem PvP-Server mit fester Zeit
            // wuerde sonst genau das passieren, was der Server nie tut: der Himmel zieht los.
            if self.world_time_ticking {
                self.world_time_of_day = self.world_time_of_day.wrapping_add(1);
            }
            // Ein Tick fuer alle Entities: Position/Rotation einen Interpolationsschritt
            // weiter, Kopfdrehung nach, Schrittanimation und Umhang -- und das Alter fuer die
            // Ruhe-Bewegung der Arme (s. Entities::tick).
            // Erfahrungsorbs fliegen auch zum eigenen Spieler -- solange er lebt und nicht zuschaut.
            let eigener = (self.health > 0.0 && self.game_mode != GAME_MODE_SPECTATOR).then(|| world::OrbPlayer {
                id: self.own_entity_id.unwrap_or(i32::MIN),
                pos: (self.physics.pos.x, self.physics.pos.y, self.physics.pos.z),
                eye_height: self.physics.eye_height() as f64,
            });
            self.sync_own_body();
            self.entities.tick(&self.world, &self.block_shapes, eigener);
            self.update_team_facts();
            self.seat_own_player();
            self.tick_potion_swirls();
            self.tick_crit_trails();
            self.tick_area_clouds();
            self.tick_eating_particles();
            // Umgebungspartikel der Bloecke um den Spieler (Blasensaeulen), dann Explosions-
            // Truemmer (ClientWorld.tick) und der ParticleManager -- in Vanilla alles im
            // Client-Tick, nach den Entities.
            let mitte = (self.physics.pos.x.floor() as i32, self.physics.pos.y.floor() as i32, self.physics.pos.z.floor() as i32);
            let mut umgebung = Vec::new();
            self.with_particles(|p, ctx, _| umgebung = p.animate_blocks(mitte, ctx));
            for klang in umgebung {
                let seed = self.particles.rng().next_int(i32::MAX) as i64;
                self.play_sound_effect(protocol::packets::SoundEffect {
                    name: klang.name.to_string(),
                    category: protocol::packets::SoundCategory::Blocks,
                    position: Some((klang.pos.x, klang.pos.y, klang.pos.z)),
                    entity_id: None,
                    volume: klang.volume,
                    pitch: klang.pitch,
                    seed,
                }, "client");
            }
            self.with_particles(|p, ctx, body| p.tick(body, ctx));
            // Koerperdrehung nachfuehren (s. world::BodyYaw). Gefuettert mit der ECHTEN
            // Strecke dieses Ticks -- Vanilla nimmt dort getX()-lastX, dieselbe Groesse, die
            // auch die Schrittanimation treibt.
            self.own_swing_ticks = self.own_swing_ticks.saturating_sub(1);
            // ClientPlayerEntity.tickMovement: renderYaw/renderPitch laufen der Blickrichtung
            // mit 0.5 pro Tick nach (s. own_render_yaw).
            self.own_last_render_yaw = self.own_render_yaw;
            self.own_last_render_pitch = self.own_render_pitch;
            self.own_render_yaw += world::wrap_degrees(self.camera.yaw - self.own_render_yaw) * 0.5;
            self.own_render_pitch += (self.look_pitch_deg() - self.own_render_pitch) * 0.5;
            // HeldItemRenderer.updateHeldItems: die Hand folgt der Angriffs-Aufladung, mit
            // Vanillas Schrittweite von hoechstens 0.4 pro Tick.
            self.own_last_equip_progress = self.own_equip_progress;
            let tempo = self.attack_speed_attribute();
            let ziel = equip_target(self.ticks_since_last_attack, tempo);
            self.own_equip_progress = equip_step(self.own_equip_progress, ziel);
            // PlayerEntity.isBlocking: Schild schon laenger als seine Block-Verzoegerung (5 Ticks)
            // oben -- dann darf der Kopf nur 15 statt 50 Grad gegen den Koerper verdreht sein.
            let blockt = match self.own_using_item_right_arm() {
                Some(rechts) => {
                    let (haupt, zweit) = self.own_hand_items();
                    let item = if rechts { haupt } else { zweit };
                    item.and_then(|i| world::entity::held_blocks_attacks(&i))
                }
                None => None,
            };
            self.own_use_ticks = if self.own_using_item_right_arm().is_some() { self.own_use_ticks.saturating_add(1) } else { 0 };
            self.own_eating_particles(false);
            self.tick_aggressive_use_cancel();
            self.tick_consumable_optimizer();
            self.own_body_yaw.set_blocking(blockt.is_some_and(|b| self.own_use_ticks >= world::entity::block_delay_ticks(&b)));
            self.own_body_yaw.tick(
                self.camera.yaw,
                self.physics.pos.x - before_tick.x,
                self.physics.pos.z - before_tick.z,
                self.own_swing_ticks > 0,
            );
            // Im Boot zeigt der Koerper in Fahrtrichtung (clampPassengerYaw setzt bodyYaw).
            if let Some(boot) = self.own_boat_yaw() {
                self.own_body_yaw.set_yaw(boot);
            }
            // Umhang, Teil 2 von 2: ClientPlayerEntity.move ruft nach der Bewegung
            // addDistanceMoved(hypot(dx, dz) * 0.6) -- dieselbe Strecke wie oben, den Faktor
            // 0.6 setzt CapeState selbst. Das ist die Phase des Lauf-Schwingens.
            self.own_cape.add_distance_moved(moved);
            // Totem-Modul: Handinhalt dieses Ticks merken (die Historie deckt den Fall ab,
            // dass die Slot-Aktualisierung VOR dem Statuspaket ankommt, s.
            // modules::TotemTracker) und eine laufende Anzeige weiterzaehlen.
            let (main_hand_totem, off_hand_totem) = self.totem_in_haenden();
            // Die Hand ENTSCHEIDEN und veroeffentlichen, solange der Tracker noch den Stand
            // von vorher haelt -- danach ueberschreibt tick() ihn. Der Sofort-Ton-Haken im
            // Netzwerk-Task liest nur diesen Wert (s. start_connect), rechnet also nichts
            // nach: eine Entscheidung, drei Kanaele (Farbe, Seite, Ton).
            let hand = self.totem_tracker.resolve(main_hand_totem, off_hand_totem);
            self.totem_hand_published.store(hand.to_published(), std::sync::atomic::Ordering::Relaxed);
            self.totem_tracker.tick(main_hand_totem, off_hand_totem);
            // Pop-Klaenge: neu aufloesen, falls sich eine Zutat geaendert hat (Einstellung,
            // Hauptlautstaerke, erst jetzt geladener Klang-Index).
            self.publish_own_pop_plan();
            self.tick_crystal_opt_out_notice();
            if let Some(anim) = self.totem_animation.as_mut() {
                if !anim.tick() {
                    self.totem_animation = None;
                }
            }
            // InGameOverlayRenderer.tickFloatingItemTimer.
            if let Some((ticks, _, _)) = self.floating_totem.as_mut() {
                *ticks = ticks.saturating_sub(1);
                if *ticks == 0 {
                    self.floating_totem = None;
                }
            }
            // LivingEntity.updateLeaningPitch: in der Schwimm-Pose hoch, sonst runter, je
            // 0.09 pro Tick.
            let step = if self.physics.swimming { 0.09 } else { -0.09 };
            self.own_leaning_pitch = (self.own_leaning_pitch + step).clamp(0.0, 1.0);
            self.own_gliding_ticks = if self.physics.gliding { self.own_gliding_ticks.saturating_add(1) } else { 0 };
            self.tick_fire_alpha();
            // Den TATSAECHLICH von der Physik entschiedenen Sprint-Zustand melden (self.
            // physics.sprinting NACH dem Tick), nicht eine separat nachgebaute Naeherung der
            // Gate-Formel: die fruehere Fassung hier ("sprint_held && forward>0 && !sneak")
            // liess die isPartlyTouchingWater()-Sperre aus physics.rs::tick() aus. Bei
            // gehaltener Sprint-Taste faellt das kaum auf (man laesst sie im Wasser meist eh
            // los), aber mit Sprint-Umschalten (sprint_held bleibt dauerhaft true) meldete
            // das hier JEDEN Tick mit forward>0 faelschlich "sprinting=true" an den Server,
            // obwohl die eigene Physik den Start korrekt blockierte -- der Server liess einen
            // dadurch trotzdem im Wasser-Rand-Bereich neu lossprinten (mit Sprintlock
            // am Wasserrand).
            //
            // ECHTER BUG gewesen (Grim "Post: entity action"): dieser Aufruf
            // stand bisher NACH der gesamten Tick-Schleife, also nur EINMAL pro Frame statt
            // einmal PRO TICK. Holt ein Frame mehrere faellige Ticks nach (Frame-Ruckler),
            // kam das Sprint-Paket dadurch erst ganz am Schluss an -- nach bereits
            // abgeschlossenen ClientTickEnd-Paketen fuer die frueheren Ticks dieses Batches.
            // Vanillas ClientPlayerEntity.tick() ruft sendSprintingPacket() aber VOR dem
            // Bewegungspaket JEDES einzelnen Ticks auf (s. sendMovementPackets()-Kommentar).
            //
            // Reihenfolge gegenueber dem PlayerInput-Paket: Vanillas ClientPlayerEntity.tick()
            // schickt erst PlayerInput, dann sendMovementPackets() -> sendSprintingPacket()
            // -> Positionspaket. Deshalb steht der Sprint-Aufruf unten NACH send_player_input.
            //
            // PlayerInput GENAU EINMAL PRO TICK (vor ClientTickEnd), wie Vanillas
            // sendMovementPackets. ECHTER BUG gewesen (Grim "BadPacketsZ" =
            // grim.badpackets.duplicate_player_input): stand frueher EINMAL PRO FRAME vor der
            // Tick-Schleife -- bei hoher FPS aendern sich in EINEM Tick mehrere Tasten, es gingen
            // dann mehrere PlayerInput-Pakete zwischen zwei ClientTickEnd raus (Grim erlaubt genau
            // eins pro Tick). In der Schleife sendet last_sent_player_input je Tick hoechstens
            // einmal (Folge-Catch-up-Ticks im selben Frame haben denselben Input -> kein zweites
            // Paket), also nie ein Duplikat innerhalb eines Ticks.
            self.send_player_input();
            if self.own_ride.is_some() {
                self.send_riding_packets();
            } else {
                self.send_player_command_sprint(self.physics.sprinting);
                self.send_position_update();
            }
            // Kolben: Vanillas Block-Entities ticken NACH dem Spieler-Tick, der sein
            // Bewegungspaket schon verschickt hat -- ein Schub geht also erst mit dem naechsten
            // Paket raus, genau wie dort.
            let schiebbar = !self.is_dead && self.game_mode != GAME_MODE_SPECTATOR && self.own_ride.is_none();
            self.pistons.tick(&mut self.world, &self.block_shapes, &mut self.physics, schiebbar);
            self.send_client_tick_end();
            // Erst NACH dem Bewegungspaket dreht das Boot den Mitfahrer mit (Vanilla:
            // updatePassengerPosition folgt auf player.tick()).
            self.turn_with_vehicle();
            // Kamera-Augenhoehe an die Pose annaehern -- Vanillas Camera.updateEyeHeight(),
            // einmal pro Tick, s. Feldkommentar zu camera_eye_height.
            self.camera_eye_height_prev = self.camera_eye_height;
            self.camera_eye_height = physics::approach_eye_height(self.camera_eye_height, self.physics.eye_height());
        }

        // Der naechste Tick-Termin, aus dem Rest-Zeitguthaben. Nach der Tick-Schleife ist
        // physics_accum immer kleiner als ein Tick, der Abstand also positiv.
        self.tick_deadline = self.ticking.then(|| tick_deadline(self.last_movement_update, self.physics_accum));

        // Zwischenbild: bei mehr Frames als Ticks (praktisch immer, 20 Ticks/s vs. z.B.
        // 240 FPS) sonst wuerde die Kamera 19 von 20 Frames lang stillstehen und dann
        // sichtbar "springen" -- stattdessen gleitet sie mit dem Rest-Bruchteil
        // (physics_accum/TICK_DT) sanft vom vorletzten zum letzten Tick-Ergebnis.
        let alpha = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0) as f64;
        // Die Drehung durchs Boot gleitend nachziehen (s. ride_yaw_spin).
        let (schritt, schon) = self.ride_yaw_spin;
        if schritt != 0.0 && (alpha as f32) > schon {
            self.camera.yaw += schritt * (alpha as f32 - schon);
            self.ride_yaw_spin.1 = alpha as f32;
        }
        let interpolated_feet = self.physics_render_prev.lerp(self.physics.pos, alpha);
        // Kamera bleibt f32 (Render-Pipeline) -- nur die Physik selbst rechnet f64. Die
        // Augenhoehe wird MIT DEMSELBEN Tick-Bruchteil interpoliert wie die Position, exakt wie
        // Vanillas Camera.update():
        //     lerp(tickProgress, lastY, getY()) + lerp(tickProgress, lastCameraY, cameraY)
        // Die beiden Stuetzwerte sind die pro Tick angenaeherte Hoehe, nicht die harte Pose-
        // Augenhoehe (s. Feldkommentar zu camera_eye_height).
        let eye_height = self.camera_eye_height_prev + (self.camera_eye_height - self.camera_eye_height_prev) * alpha as f32;
        self.camera.position = interpolated_feet + DVec3::new(0.0, eye_height as f64, 0.0);
        // Zuschauer in einem fremden Entity: dessen Augen und Blick (Camera.setup mit dem
        // Kamera-Entity statt dem Spieler).
        if let Some(e) = self.spectated_entity() {
            let t = alpha as f32;
            let (x, y, z) = e.render_pos(t);
            let (yaw, pitch) = (e.view_yaw(t), e.render_pitch(t));
            self.camera.position = DVec3::new(x, y + e.eye_height(), z);
            self.camera.yaw = yaw;
            self.camera.pitch = -pitch;
        }
        self.update_perspective_offset();
    }

    // Steigt ein Rechtsklick auf dieses Entity ein? Boote immer (ausser schleichend), die
    // einfache Lore nur, solange niemand drin sitzt.
    pub(crate) fn click_boards_vehicle(&self, id: i32) -> bool {
        if self.sneak_held || self.game_mode == GAME_MODE_SPECTATOR {
            return false;
        }
        let Some(typ) = self.entities.get(id).and_then(|e| world::entity_type_name(e.entity_type)) else { return false };
        world::riding::boat_kind(typ).is_some() || (typ == "minecart" && !self.entities.has_passengers(id))
    }

    // EntityPassengersSetS2CPacket fuer den eigenen Spieler (ClientPlayNetworkHandler.
    // onEntityPassengersSet): steht er in der Liste, sitzt er auf diesem Fahrzeug; fehlt er in
    // der Liste SEINES Fahrzeugs, ist er abgestiegen. Ein unbekanntes Fahrzeug zaehlt nicht.
    pub(crate) fn update_own_ride(&mut self, vehicle: i32, passengers: &[i32]) {
        let Some(own) = self.own_entity_id else { return };
        let Some(fahrzeug) = self.entities.get(vehicle) else { return };
        let (pos, yaw, typ) = (fahrzeug.pos, fahrzeug.yaw_deg, world::entity_type_name(fahrzeug.entity_type).unwrap_or(""));
        match passengers.iter().position(|p| *p == own) {
            Some(index) => {
                let seat = world::riding::Seat { index, count: passengers.len() };
                if let Some(ride) = self.own_ride.as_mut().filter(|r| r.vehicle == vehicle) {
                    ride.seat = seat;
                    return;
                }
                // Von einem Fahrzeug direkt auf ein anderes: erst das alte loslassen.
                self.dismount_own();
                let mut ride = vehicle::OwnRide::new(vehicle, seat);
                ride.sim_pos = DVec3::new(pos.0, pos.1, pos.2);
                ride.sim_yaw = yaw;
                self.own_ride = Some(ride);
                self.shared_rotation.set_riding(true);
                // Ins Boot gestiegen: Blick in Fahrtrichtung.
                if world::riding::boat_kind(typ).is_some() {
                    self.camera.yaw = yaw;
                }
                self.physics.vel = DVec3::ZERO;
                // "Druecke %s zum Absteigen" -- mit der eigenen Schleichtaste.
                let taste = self.settings.keybinds.binds(KeyBindAction::Sneak).next().map(bind_label).unwrap_or_default();
                let vorlage = protocol::nbt::translate("mount.onboard").unwrap_or_else(|| "%s: Absteigen".to_string());
                let text = vorlage.replacen("%1$s", &taste, 1).replacen("%s", &taste, 1);
                self.action_bar = Some((vec![protocol::nbt::TextSpan { text, color: None, link: None, action: None, style: 0, shadow: None }], Instant::now()));
            }
            None => {
                if self.own_ride.as_ref().is_some_and(|r| r.vehicle == vehicle) {
                    self.dismount_own();
                }
            }
        }
    }

    // Absteigen auf dem Client: nur die Beziehung loesen. Wohin man kommt, sagt der Server
    // gleich mit einem Teleport (ServerPlayerEntity.requestTeleportAndDismount).
    pub(crate) fn dismount_own(&mut self) {
        if self.own_ride.take().is_some() {
            self.shared_rotation.set_riding(false);
            self.entities.set_driven(None);
            self.physics.vel = DVec3::ZERO;
            // Den Rest der Bootsdrehung noch mitnehmen, dann ist Schluss.
            let (schritt, schon) = self.ride_yaw_spin;
            self.camera.yaw += schritt * (1.0 - schon);
            self.ride_yaw_spin = (0.0, 0.0);
        }
    }

    // Lenkt der eigene Spieler das Fahrzeug gerade selbst? Vanillas getControllingPassenger je
    // Fahrzeug, nur fuer den vorderen Platz. Bisher bewegt der Client nur Boote selbst; alles
    // andere bleibt beim Server (man sitzt, das Tier steht oder laeuft, wie der Server will).
    pub(crate) fn own_ride_steering(&self, ride: &vehicle::OwnRide) -> Option<world::riding::Steering> {
        if ride.seat.index != 0 {
            return None;
        }
        let e = self.entities.get(ride.vehicle)?;
        match world::riding::steering_kind(world::entity_type_name(e.entity_type)?)? {
            world::riding::Steering::Boat => Some(world::riding::Steering::Boat),
            _ => None,
        }
    }

    // Ein Tick des Fahrzeugs, auf dem man sitzt -- steht an der Stelle der Spielerphysik, also
    // vor dem Entity-Tick (wie in Vanilla, wo das Fahrzeug vor seinen Passagieren tickt).
    pub(crate) fn tick_own_vehicle(&mut self, tasten: vehicle::BoatInput) {
        let Some(mut ride) = self.own_ride.take() else { return };
        let lenkung = self.own_ride_steering(&ride);
        let lenkt = lenkung.is_some();
        if lenkt != ride.controlling {
            ride.controlling = lenkt;
            ride.boat = vehicle::BoatSim::default();
            self.entities.set_driven(lenkt.then_some(ride.vehicle));
        }
        if lenkung == Some(world::riding::Steering::Boat) {
            if let Some(e) = self.entities.get(ride.vehicle) {
                let mut pos = DVec3::new(e.pos.0, e.pos.1, e.pos.2);
                let mut yaw = e.yaw_deg;
                ride.boat.tick(&mut pos, &mut yaw, ride.boat_input, true, &self.world, &self.block_shapes);
                ride.sim_pos = pos;
                ride.sim_yaw = yaw;
                ride.sim_pitch = 0.0;
                self.entities.drive(ride.vehicle, (pos.x, pos.y, pos.z), yaw, 0.0);
                // AbstractBoatEntity.tick schickt den Ruderstand JEDEN Tick, nicht nur bei
                // Aenderung -- und noch vor den Paketen des Spielers.
                if let Some(tx) = &self.outgoing_tx {
                    if self.conn_status == ConnStatus::Connected {
                        let _ = tx.send(protocol::packets::paddle_boat_payload(ride.boat.paddles.0, ride.boat.paddles.1));
                    }
                }
            }
        }
        // ClientPlayerEntity.tickRiding: die Tasten DIESES Ticks gehen erst nach dem Boot-Tick
        // ans Boot, wirken also im naechsten.
        ride.boat_input = tasten;
        self.own_ride = Some(ride);
        // canSprint: reitend nur auf einem Fahrzeug, das selbst sprinten kann (Kamel) --
        // keins der hier gelenkten.
        self.physics.sprinting = false;
        self.physics.vel = DVec3::ZERO;
        self.physics.on_ground = false;
        self.physics.horizontal_collision = false;
        self.physics.crouching = false;
        self.physics.swimming = false;
    }

    // Den Spieler auf seinen Sitz setzen (Entity.updatePassengerPosition), nach dem
    // Entity-Tick, damit er die Stelle DIESES Ticks bekommt.
    pub(crate) fn seat_own_player(&mut self) {
        let Some(ride) = &self.own_ride else { return };
        if let Some(p) = self.entities.seat_pos(ride.vehicle, ride.seat, "player") {
            self.physics.pos = DVec3::new(p.0, p.1, p.2);
        }
    }

    // Reitend schickt Vanilla statt der Positionspakete JEDEN Tick die Blickrichtung
    // (LookAndOnGround) und, wenn es selbst lenkt, den Stand des Fahrzeugs (VehicleMove) samt
    // Sprintmeldung -- ClientPlayerEntity.tick, Zweig hasVehicle().
    pub(crate) fn send_riding_packets(&mut self) {
        let Some(tx) = &self.outgoing_tx else { return };
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        let yaw_deg = self.camera.yaw;
        let pitch_deg = self.look_pitch_deg();
        self.shared_rotation.set(yaw_deg, pitch_deg);
        self.shared_rotation.set_riding(true);
        let (on_ground, kollision) = (self.physics.on_ground, self.physics.horizontal_collision);
        let _ = tx.send(protocol::packets::move_player_rot_payload(yaw_deg, pitch_deg, on_ground, kollision));
        interaktions_log("movement");
        // Der "zuletzt gesendet"-Stand der Blickrichtung zieht mit; die Position nicht (Vanilla
        // fasst lastX/Y/Z reitend nicht an -- nach dem Absteigen geht also gleich eine raus).
        self.last_sent_yaw_deg = yaw_deg;
        self.last_sent_pitch_deg = pitch_deg;
        self.last_sent_on_ground = on_ground;
        self.last_sent_horizontal_collision = kollision;
        if self.own_ride.as_ref().is_some_and(|r| r.controlling) {
            self.send_vehicle_move();
            self.send_player_command_sprint(self.physics.sprinting);
        }
    }

    pub(crate) fn send_vehicle_move(&self) {
        let (Some(tx), Some(ride)) = (&self.outgoing_tx, &self.own_ride) else { return };
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        let p = ride.sim_pos;
        let _ = tx.send(protocol::packets::move_vehicle_payload((p.x, p.y, p.z), ride.sim_yaw, ride.sim_pitch, ride.boat.on_ground));
    }

    // VehicleMoveS2CPacket (ClientPlayNetworkHandler.onVehicleMove): der Server war mit der
    // gesendeten Stelle nicht einverstanden. Weicht seine um mehr als 1e-5 ab, springt das
    // Fahrzeug dorthin; geantwortet wird in jedem Fall mit dem eigenen Stand.
    pub(crate) fn on_vehicle_move(&mut self, pos: (f64, f64, f64), yaw_deg: f32, pitch_deg: f32) {
        let Some(ride) = self.own_ride.as_mut().filter(|r| r.controlling) else { return };
        let neu = DVec3::new(pos.0, pos.1, pos.2);
        if neu.distance(ride.sim_pos) > 1.0e-5 {
            ride.sim_pos = neu;
            ride.sim_yaw = yaw_deg;
            ride.sim_pitch = pitch_deg;
            let id = ride.vehicle;
            self.entities.place_driven(id, pos, yaw_deg, pitch_deg);
            self.seat_own_player();
            self.physics_render_prev = self.physics.pos;
        }
        self.send_vehicle_move();
    }

    // Nach dem Tick: das Boot nimmt den Mitfahrer in seiner Drehung mit und haelt seinen Blick
    // in 105 Grad zur Fahrtrichtung (AbstractBoatEntity.updatePassengerPosition).
    pub(crate) fn turn_with_vehicle(&mut self) {
        // Was vom vorigen Schritt noch nicht in der Kamera ist, jetzt ganz.
        let (schritt, schon) = self.ride_yaw_spin;
        self.camera.yaw += schritt * (1.0 - schon);
        self.ride_yaw_spin = (0.0, 0.0);
        let Some(ride) = &self.own_ride else { return };
        if !ride.controlling {
            self.clamp_yaw_to_boat();
            return;
        }
        self.ride_yaw_spin = (ride.boat.yaw_velocity, 0.0);
        self.clamp_yaw_to_boat();
    }

    // Drehung des Boots, in dem man sitzt (None = kein Boot): die gelenkte aus der eigenen
    // Rechnung, eine fremde aus der Entity.
    pub(crate) fn own_boat_yaw(&self) -> Option<f32> {
        let ride = self.own_ride.as_ref()?;
        let e = self.entities.get(ride.vehicle)?;
        world::riding::boat_kind(world::entity_type_name(e.entity_type)?)?;
        Some(if ride.controlling { ride.sim_yaw } else { e.yaw_deg })
    }

    pub(crate) fn clamp_yaw_to_boat(&mut self) {
        // Gegen die Drehung, die das Boot nach diesem Tick hat, und mit der noch ausstehenden
        // Mitdrehung.
        let Some(boot) = self.own_boat_yaw() else { return };
        let offen = self.ride_yaw_spin.0 * (1.0 - self.ride_yaw_spin.1);
        self.camera.yaw = vehicle::clamp_boat_passenger_yaw(self.camera.yaw + offen, boot) - offen;
    }

    pub(crate) fn send_position_update(&mut self) {
        let Some(tx) = &self.outgoing_tx else { return };
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        let pos = self.physics.pos;
        // xRot (Pitch) ist in Vanilla das Gegenteil unserer Konvention: bei uns bedeutet ein
        // steigender Wert "nach oben schauen" (s. camera.rs), bei Vanilla "nach unten" --
        // deshalb hier das Vorzeichen drehen, nicht schon beim Maus-Delta (s. dortiger
        // Kommentar zur Yaw-Konvention, der bewusst NICHT gedreht wird).
        let yaw_deg = self.camera.yaw;
        let pitch_deg = self.look_pitch_deg();
        // Dem Netz-Task die aktuelle Blickrichtung mitteilen -- er braucht sie fuer die
        // sofortige Teleport-Bestaetigungs-Bewegung (s. connection.rs, SharedRotation).
        self.shared_rotation.set(yaw_deg, pitch_deg);
        self.shared_rotation.set_riding(false);
        let on_ground = self.physics.on_ground;
        let horizontal_collision = self.physics.horizontal_collision;

        // ClientPlayerEntity.sendMovementPackets() genau nachgebaut (s. Feldkommentar an
        // last_sent_pos): Positions-Schwelle 2e-4 (quadriert), erzwungene Aktualisierung
        // alle 20 Ticks, Blickrichtung nur bei ECHTER Aenderung (!= 0.0, kein Schwellwert).
        self.ticks_since_position_sent += 1;
        let moved = (pos - self.last_sent_pos).length_squared() > (2.0e-4f64).powi(2) || self.ticks_since_position_sent >= 20;
        let looked = yaw_deg != self.last_sent_yaw_deg || pitch_deg != self.last_sent_pitch_deg;

        let payload = if moved && looked {
            Some(protocol::packets::move_player_pos_rot_payload(pos.x, pos.y, pos.z, yaw_deg, pitch_deg, on_ground, horizontal_collision))
        } else if moved {
            Some(protocol::packets::move_player_pos_payload(pos.x, pos.y, pos.z, on_ground, horizontal_collision))
        } else if looked {
            Some(protocol::packets::move_player_rot_payload(yaw_deg, pitch_deg, on_ground, horizontal_collision))
        } else if on_ground != self.last_sent_on_ground || horizontal_collision != self.last_sent_horizontal_collision {
            Some(protocol::packets::move_player_status_only_payload(on_ground, horizontal_collision))
        } else {
            None
        };
        // Fuer die Paketreihenfolge zaehlt nur, OB ein Bewegungspaket rausgeht -- daran endet
        // fuer Grim der Tick (s. interaktions_log).
        interaktions_log(if payload.is_some() { "movement" } else { "movement(-)" });
        // TEMPORAERES Debug-Logging (CORAL_NET_DEBUG=1), um Grim-Flags
        // (Simulation/GroundSpoof) gegen die tatsaechlich gesendeten Pakete abzugleichen,
        // statt weiter blind zu raten.
        if std::env::var_os("CORAL_NET_DEBUG").is_some() {
            let kind = match &payload {
                Some(p) if p[0] as i32 == protocol::packets::SB_PLAY_MOVE_PLAYER_POS_ROT => "Full",
                Some(p) if p[0] as i32 == protocol::packets::SB_PLAY_MOVE_PLAYER_POS => "Pos",
                Some(p) if p[0] as i32 == protocol::packets::SB_PLAY_MOVE_PLAYER_ROT => "Rot",
                Some(_) => "StatusOnly",
                None => "-",
            };
            // UTC-Uhrzeit ohne Zeitzonen-Crate, nur zum Abgleich mit den (lokalen)
            // Zeitstempeln des Grim-Server-Logs -- Offset dabei manuell dazurechnen.
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            let secs_of_day = now.as_secs() % 86400;
            let (h, m, s, ms) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60, now.subsec_millis());
            eprintln!(
                "[net] {h:02}:{m:02}:{s:02}.{ms:03}Z {kind:10} pos=({:.4},{:.4},{:.4}) yaw={yaw_deg:.2} pitch={pitch_deg:.2} on_ground={on_ground} hcol={horizontal_collision}",
                pos.x, pos.y, pos.z
            );
        }
        if let Some(payload) = payload {
            let _ = tx.send(payload);
        }

        if moved {
            self.last_sent_pos = pos;
            self.ticks_since_position_sent = 0;
        }
        if looked {
            self.last_sent_yaw_deg = yaw_deg;
            self.last_sent_pitch_deg = pitch_deg;
        }
        self.last_sent_on_ground = on_ground;
        self.last_sent_horizontal_collision = horizontal_collision;
    }

    // ClientTickEndC2SPacket (s. protocol::packets::SB_PLAY_CLIENT_TICK_END-Kommentar):
    // einmal pro Physik-Tick, direkt nach dem Positions-Update dieses Ticks -- fehlte
    // komplett, liess Grims TickTimer-Check ("did not send client tick end packet") und in
    // dessen Folge auch andere Bewegungs-Checks (Simulation/GroundSpoof) durchfallen,
    // obwohl die Physik selbst korrekt war.
    // Tick OHNE Physik. Solange die eigene Spalte fehlt, bleibt der Spieler bewusst stehen
    // (s. update_movement: sonst faellt er durch die noch leere Welt), die PAKETE muessen
    // aber im 20-Hz-Takt weiterlaufen -- Vanilla sendet sie ab dem allerersten Tick, und ein
    // Aussetzer von mehr als zwei Sekunden bei weiterlaufenden Transaktionen ist Grims
    // BadPacketsR (s. update_movement).
    //
    // Erst ab dem ersten Positions-Sync des Servers (camera_placed): davor kennen wir unsere
    // echte Position gar nicht und wuerden dem Server den Startwert melden statt des
    // Spawnpunkts.
    pub(crate) fn wait_tick(&mut self) {
        if !self.camera_placed {
            return;
        }
        self.send_player_input();
        self.send_player_command_sprint(self.physics.sprinting);
        self.send_position_update();
        self.send_client_tick_end();
    }

    pub(crate) fn send_client_tick_end(&mut self) {
        self.dig_packet_this_tick = false;
        let Some(tx) = &self.outgoing_tx else { return };
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        interaktions_log("tick_end");
        let _ = tx.send(protocol::packets::client_tick_end_payload());
    }

    // PlayerInputC2SPacket: forward/backward/left/right/jump/sneak/sprint als Bitflags, NUR
    // bei Aenderung gesendet (s. protocol::packets::SB_PLAY_PLAYER_INPUT-Kommentar) -- fuer
    // die serverseitige Bewegungs-Vorhersage/-Validierung, ABER NICHT die Quelle fuer
    // isSprinting()/isSwimming() (s. send_player_command_sprint darunter, das ist die
    // eigentlich relevante Quelle fuer die Pose-Kollisionsbox).
    // Nach Login/Respawn vergessen, was wir zuletzt gesendet haben -- damit der naechste Tick
    // Tastenzustand und Sprint ZWINGEND neu schickt.
    //
    // ECHTER BUG gewesen, und der Grund fuer die uebrig gebliebenen Simulation-Flags: sowohl
    // PlayerInput als auch der Sprint-Befehl gehen nur bei AENDERUNG raus. Vanilla kommt damit
    // durch, weil ein Respawn dort eine NEUE ClientPlayerEntity baut -- deren lastPlayerInput
    // steht auf DEFAULT, der naechste Tick meldet den echten Tastenzustand also von selbst neu.
    // Unser Client lebt ueber den Respawn hinweg weiter und hielt seinen "zuletzt gesendet"-
    // Stand fest. GrimAC setzt auf seiner Seite aber genau dann zurueck (PacketPlayerRespawn:
    // `knownInput = KnownInput.DEFAULT`, `isSprinting = false`, aus dem Jar nachgesehen).
    //
    // Wirkung: wer nach dem Respawn W gedrueckt HAELT, ohne die Taste noch einmal loszulassen,
    // laeuft fuer Grim mit `forward = false`. Und dessen Vorhersage hat dabei keinerlei
    // Spielraum -- PredictionEngine.loopVectors setzt fuer Clients mit ClientTickEnd
    // forwardMin == forwardMax == knownInput. Grim sagt dann exakt 0 Bewegung voraus, waehrend
    // wir laufen. Genau das stand im Mitschnitt: `P: 0.0 0.0 8.88E-16` gegen `A: … 0.0294`.
    //
    // Zurueckgesetzt wird auf den DEFAULT-Zustand (alles losgelassen, kein Sprint), nicht auf
    // "noch nie etwas gesendet": genau das ist Grims Annahme nach dem Respawn, und genau so
    // startet auch Vanillas neue ClientPlayerEntity. Mit "nie gesendet" ginge zusaetzlich ein
    // leeres PlayerInput samt STOP_SPRINTING raus, das Vanilla nicht schickt.
    // Vanilla baut bei Login und Respawn einen FRISCHEN Spieler, dessen Entity-Flags alle aus
    // sind. Der neue Server schickt nur Abweichungen vom Standard -- "gleitet nicht" kommt also
    // nie an, ohne das hier flog man nach einem Serverwechsel mit der Elytra weiter.
    pub(crate) fn reset_own_entity_flags(&mut self) {
        self.physics.reset_for_new_player();
        self.own_gliding_ticks = 0;
        self.own_on_fire = false;
        self.own_frozen_ticks = 0;
        self.own_invisible = false;
        self.own_glowing = false;
        // Ebenso die Trank-Partikel: "keine" ist der Standard und kommt nach dem Respawn nicht
        // eigens an -- sonst wirbeln die Partikel aus dem alten Leben (Goldapfel, Traenke) ewig
        // weiter, auch als Zuschauer.
        self.own_potion_swirls.clear();
        self.own_potion_swirls_ambient = false;
    }

    pub(crate) fn forget_sent_input_state(&mut self) {
        self.last_sent_player_input = Some((false, false, false, false, false, false, false));
        self.last_sent_sprinting = Some(false);
    }

    pub(crate) fn send_player_input(&mut self) {
        let Some(tx) = &self.outgoing_tx else { return };
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        let current = self.movement_input_flags();
        if self.last_sent_player_input == Some(current) {
            return;
        }
        self.last_sent_player_input = Some(current);
        let (forward, backward, left, right, jump, sneak, sprint) = current;
        let _ = tx.send(protocol::packets::player_input_payload(forward, backward, left, right, jump, sneak, sprint));
        if std::env::var_os("CORAL_SNEAK_DEBUG").is_some() {
            eprintln!("[input] gesendet: vor={forward} zur={backward} li={left} re={right} spr={jump} sneak={sneak} sprint={sprint}");
        }
    }

    // Player Command (Sprint-Start/Stop), NUR bei Aenderung gesendet -- die tatsaechliche
    // Quelle fuer LivingEntity.setSprinting() serverseitig (s. protocol::packets::
    // SB_PLAY_PLAYER_COMMAND-Kommentar). Ohne das benutzt der Server dauerhaft die
    // Steh-Hitbox fuer uns, egal was wir lokal an Schwimm-Pose berechnen -- fuehrte zu
    // wiederholten Server-Positionskorrekturen, sobald die (lokal schon aktive) Schwimm-Box
    // etwas erlaubte, das die Steh-Box serverseitig nicht haette.
    pub(crate) fn send_player_command_sprint(&mut self, sprinting: bool) {
        let Some(tx) = &self.outgoing_tx else { return };
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        let Some(entity_id) = self.own_entity_id else {
            return;
        };
        if self.last_sent_sprinting == Some(sprinting) {
            return;
        }
        self.last_sent_sprinting = Some(sprinting);
        let _ = tx.send(protocol::packets::player_command_sprint_payload(entity_id, sprinting));
    }
}
