// Ersticken, freier Platz, Sprungbereitschaft, Fluessigkeit verlassen, Pose und Riptide.

use super::*;

impl PlayerPhysics {
    // Steckt die Spielerbox in dieser Blockspalte in einem erstickenden Block?
    //
    // ClientPlayerEntity.wouldCollideAt (1.21.11-Jar): eine Box ueber die GANZE Zelle in X/Z,
    // in Y ueber die eigene Hoehe, um 1e-7 geschrumpft, und dann World.canCollide -- also die
    // KOLLISIONSFORMEN der erstickenden Bloecke (BlockCollisionSpliterator mit forEntity), die
    // diese Box wirklich schneiden. Nicht bloss "in der Zelle steht ein erstickender Block":
    // Trampelpfad und Ackerboden ersticken, sind aber nur 15/16 hoch. Wer auf einem Pfad steht,
    // hat daneben einen gleich hohen Pfad UNTER den Fuessen, nicht in der Box. Wir zaehlten die
    // ganze Zelle und schoben dort mit 0.1 weg, Vanilla (und Grim) nicht -- sichtbar als
    // Simulation-Flags beim Springen von Pfad zu Pfad.
    pub(crate) fn suffocates_at(&self, bx: i32, bz: i32, world: &World, shapes: &world::BlockShapes) -> bool {
        const EPS: f64 = 1.0e-7;
        let min = DVec3::new(f64::from(bx) + EPS, self.pos.y + EPS, f64::from(bz) + EPS);
        let max = DVec3::new(f64::from(bx) + 1.0 - EPS, self.pos.y + self.current_height() - EPS, f64::from(bz) + 1.0 - EPS);
        let boxen = collect_boxes(min, max, world, &|state, by| {
            if world::is_suffocating(state, shapes) {
                world::collision_boxes_at(state, shapes, by, world::ShapeContext::ABSENT)
            } else {
                Vec::new()
            }
        });
        !boxen.is_empty()
    }

    // Vanillas LocalPlayer.moveTowardsClosestSpace(): steckt diese Ecke der Spielerbox in
    // einem erstickenden Block, bekommt die Geschwindigkeit 0.1 in die Richtung der NAECHSTEN
    // freien Nachbarspalte gesetzt (gesetzt, nicht addiert) -- so schiebt es einen in Vanilla
    // aus einem Block heraus, in den man hineingeraten ist oder in den jemand gebaut hat.
    //
    // ECHTE ABWEICHUNG GEWESEN: der Client hatte das gar nicht. Wer uns
    // in Obsidian einmauerte, bekam damit nicht Vanillas Ergebnis -- ein Vorteil, den es nicht
    // geben soll. GrimAC flaggt beides nicht: es simuliert das Herausschieben nicht, sondern
    // macht das erlaubte Fenster auf der betroffenen Achse um +-0.15 auf (PlayerBaseTick.
    // moveTowardsClosestSpaceModern), und 0.1 liegt darin.
    //
    // Die Richtungsreihenfolge (West, Ost, Nord, Sued) und der ECHT-kleiner-Vergleich sind
    // Vanillas -- bei gleichem Abstand gewinnt die zuerst gepruefte Richtung.
    pub(crate) fn move_towards_closest_space(&mut self, x: f64, z: f64, world: &World, shapes: &world::BlockShapes) {
        let (bx, bz) = (x.floor() as i32, z.floor() as i32);
        if !self.suffocates_at(bx, bz, world, shapes) {
            return;
        }
        let (f, g) = (x - f64::from(bx), z - f64::from(bz));
        let mut ziel: Option<(i32, i32)> = None;
        let mut kleinster = f64::MAX;
        // (Schritt x, Schritt z, Abstand bis zur Zellgrenze in dieser Richtung)
        for (sx, sz, abstand) in [(-1, 0, f), (1, 0, 1.0 - f), (0, -1, g), (0, 1, 1.0 - g)] {
            if abstand < kleinster && !self.suffocates_at(bx + sx, bz + sz, world, shapes) {
                kleinster = abstand;
                ziel = Some((sx, sz));
            }
        }
        match ziel {
            Some((sx, 0)) => self.vel.x = 0.1 * f64::from(sx),
            Some((_, sz)) => self.vel.z = 0.1 * f64::from(sz),
            None => {}
        }
    }

    // s. jump_cooldown_enabled-Feldkommentar: AUS macht jeden Sprung sofort wieder
    // moeglich, unabhaengig vom Zaehlerstand.
    pub(crate) const fn jump_ready(&self) -> bool {
        !self.jump_cooldown_enabled || self.jump_cooldown == 0
    }

    // LivingEntity.resetVerticalVelocityInFluid(): am Ende von travelInWater()/
    // travelInLava() automatisch ein Stueck herausklettern, wenn man im Wasser/in Lava
    // horizontal gegen eine Kante/Wand schwimmt UND darueber Platz ist -- ohne das wuerde
    // man hilflos gegen jede Kante schwimmen, statt drueberzukommen. Setzt vel.y HART auf
    // 0.3 (kein max() wie beim Sprung -- eine echte Ersetzung), OHNE jump_cooldown und
    // OHNE gehaltenes Springen als Voraussetzung: reine Horizontalkollision reicht.
    //
    // Das ist der TATSAECHLICHE Grund fuer "water hopping" (echter Vanilla-Mechanismus): schwimmt man kontinuierlich gegen dieselbe Kante, loest JEDER Tick mit
    // Wandkontakt erneut den vollen 0.3-Kick aus -- viel oefter, als ein normaler
    // Sprung-Cooldown erlauben wuerde, weil hier gar kein Cooldown greift.
    //
    // Vanilla (1.21.11-Jar, LivingEntity): bei horizontaler Kollision wird geprueft, ob die
    // Box um (vel.x, vel.y + 0.6 - aktuelle Hoehe + y, vel.z) verschoben frei waere; dann wird
    // vel.y auf 0.3 gesetzt. `y` ist die Y-Position VOR der Bewegung dieses Ticks (in
    // travelInFluid vor updateVelocity/move gemerkt), self.pos ist hier -- nach move_and_zero, ganz am Ende von
    // tick_water/tick_lava -- die Position DANACH. Der Versatz peilt also ab der alten Position
    // 0.6 Bloecke hoch, nicht ab der schon fortgeschrittenen.
    //
    // Drei Punkte, an denen das vorher nicht Vanilla war (deshalb war es testweise ganz
    // abgeschaltet, statt zu oft zu feuern):
    //   1. Fluid-Test. doesNotCollide() ist isSpaceEmpty(box) && !containsFluid(box).
    //      containsFluid zaehlt JEDE Zelle mit nicht-leerem FluidState -- unabhaengig vom
    //      Fuellstand. Wir hatten stattdessen fluid_height_scan benutzt, das die Box um 0.001
    //      schrumpft und nur anschlaegt, wenn die Fluid-OBERFLAECHE ueber der Boxunterkante
    //      liegt. Genau der Fall am Beckenrand: die Zielbox liegt in halbhohem Fliesswasser,
    //      Vanilla sagt "belegt, kein Hop", wir sagten "frei, Hop" -- ein 0.3-Kick, den Grim
    //      nicht mitrechnet, also grim.simulation. Das war der eigentliche Fehler.
    //   2. Rechenreihenfolge des Versatzes. Java rechnet ((v.y + 0.6f) - getY()) + y und legt
    //      das auf box.minY == getY() drauf. Algebraisch y + v.y + 0.6, in f64 aber nicht
    //      bitgleich -- und Grim rechnet Javas Reihenfolge. Hier jetzt genauso Schritt fuer
    //      Schritt.
    //   3. 0.6F und 0.3F sind FLOATS. Als double sind das 0.6000000238418579 bzw.
    //      0.30000001192092896, nicht 0.6/0.3. Der Kick war um 1.2e-8 zu klein -- pro Tick
    //      nichts, ueber eine Hop-Serie gegen dieselbe Kante aber genau die Art Drift, die
    //      am Ende als Offset auffaellt.
    //
    // Kein jump_cooldown und kein gehaltenes Springen: reine Horizontalkollision reicht. Das
    // ist der Grund, warum "water hopping" ueberhaupt geht -- schwimmt man dauerhaft gegen
    // dieselbe Kante, feuert JEDER Tick mit Wandkontakt den vollen Kick erneut. Das ist echte
    // Vanilla-Mechanik, kein Zusatz von uns, und wird deshalb auch nicht per Einstellung
    // gedrosselt (Settings::jump_cooldown_enabled betrifft nur echte Spruenge).
    pub(crate) fn jump_out_of_fluid(&mut self, world: &World, shapes: &world::BlockShapes, y_before: f64) {
        if !self.horizontal_collision {
            return;
        }
        // Exakt Javas Term- und Klammerfolge, s. Punkt 2/3 oben.
        let offset_y = self.vel.y + f64::from(0.6f32) - self.pos.y + y_before;
        let target = DVec3::new(self.pos.x + self.vel.x, self.pos.y + offset_y, self.pos.z + self.vel.z);
        let height = self.current_height();
        let half = self.width() / 2.0;
        let box_min = DVec3::new(target.x - half, target.y, target.z - half);
        let box_max = DVec3::new(target.x + half, target.y + height, target.z + half);
        if box_is_empty(box_min, box_max, world, shapes, self.shape_context()) && !contains_fluid(box_min, box_max, world) {
            self.vel.y = f64::from(0.3f32);
        }
    }

    // PlayerEntity.travel(): waehrend der Schwimm-Pose (s. self.swimming) wird die
    // Vertikalgeschwindigkeit VOR der eigentlichen travelInWater()-Bewegung Richtung
    // Blickrichtung gezogen -- deshalb schwimmt man beim Sprint-Schwimmen tatsaechlich in die
    // Richtung, in die man schaut (nach oben/unten schauen + vorwaerts sprinten reicht zum
    // Auf-/Abtauchen, ohne zusaetzlich Springen/Schleichen zu halten). NICHT geraten, sondern
    // aus dem lokalen Yarn-1.21.11-Jar nachgesehen: d ist die Y-Komponente des Blickvektors
    // (+ = hoch), der Faktor e ist 0.085 bei d < -0.2 (steil nach unten = schnellere
    // Annaeherung), sonst 0.06. Ist d <= 0, springt man, oder steht Fluessigkeit direkt ueber
    // dem Kopf, rueckt vel.y um e * (d - vel.y) Richtung d.
    // Der dritte Bedingungszweig (Wasser direkt ueber dem Kopf) verhindert, dass blosses
    // Nach-oben-Schauen ohne Springen einen schon aus dem Wasser zieht, sobald die Oberflaeche
    // erreicht ist -- solange noch Wasser ueber dem Kopf ist, greift der Zug trotzdem, auch
    // ohne gehaltenes Springen.
    pub(crate) fn apply_swim_pitch_pull(&mut self, input: &Input, world: &World) {
        // ECHTER BUG gewesen (nach oben schwimmen ohne Leertaste
        // -> Server-Setback beim Reinspringen/Auftauchen). Vanilla PlayerEntity.travel gated
        // diesen Blick-nach-oben-Zug auf `isSwimming()` -- das ist der ECHTE Sprint-Schwimm-
        // Zustand (bei uns `raw_swimming`), NICHT die blosse Schwimm-POSE (`self.swimming`).
        // Die Pose kann per Fallback aktiv sein (untergetaucht, wo Steh-/Crouch-Pose nicht
        // passt), ohne dass isSwimming() gilt. Vorher zogen wir dann faelschlich vel.y Richtung
        // Blickrichtung nach oben (im consoledebug als A.y = +0.04, waehrend Grim weiter sank
        // -> Offset > 0.1 -> sofortiger Setback). raw_swimming == isSwimming(), swimming == Pose.
        if !self.raw_swimming {
            return;
        }
        let d = f64::from(input.look_dir_y);
        let e = if d < -0.2 { 0.085 } else { 0.06 };
        let (bx, by, bz) = ((self.pos.x).floor() as i32, (self.pos.y + 0.9).floor() as i32, (self.pos.z).floor() as i32);
        let water_above_head = world::fluid_kind(world.block_at(bx, by, bz)).is_some();
        if d <= 0.0 || input.jump || water_above_head {
            self.vel.y += (d - self.vel.y) * e;
        }
    }

    // Kompletter Vanilla-Bewegungs-Input-Pfad (nach dem Yarn-1.21.11-Client):
    //
    //   KeyboardInput.tick():          movementVector = normiert(sideways, forward)
    //   ClientPlayerEntity.applyMovementSpeedFactors():
    //                                  vec = movementVector * 0.98 (float)
    //                                  if (shouldSlowDown()) vec *= SNEAKING_SPEED (=0.3)
    //                                  vec = applyDirectionalMovementSpeedFactors(vec)
    //   ClientPlayerEntity.applyDirectionalMovementSpeedFactors(vec):
    //                                  f = |vec|; dir = vec/f
    //                                  g = getDirectionalMovementSpeedMultiplier(dir)   // 1 (kardinal), sqrt2 (diagonal)
    //                                  vec = dir * min(f*g, 1.0)
    //   Entity.movementInputToVelocity(input, speed, yaw):
    //                                  if (|input|^2 > 1) input = normalize(input)      // hier nie, |input| <= 1
    //                                  vel += rotate(input * speed, yaw)
    //
    // ECHTE BUGS gewesen:
    //   - "Vorwaerts 2% zu schnell": das 0.98 fehlte ganz.
    //   - "Sneak in der Luft flaggt": Sneak (0.3) lag auf der accel statt auf dem Input VOR der
    //     Richtungs-Skalierung. Dadurch war Diagonal-Sneak bei uns 0.294*accel statt Vanillas
    //     0.416*accel (Ratio 0.72). Der Richtungs-Multiplikator skaliert Diagonalen wieder hoch
    //     (gedeckelt auf 1.0), was eine reine Nach-der-Normalisierung-Daempfung nie reproduziert.
    //   - Sneak-Verlangsamung haengt am isInSneakingPose()-FELD (shouldSlowDown()), das Vanilla
    //     VOR input.tick() setzt -> es laggt der Taste um einen Tick nach: in_sneaking_pose_move,
    //     NICHT die aktuelle Hitbox-Pose self.crouching und NICHT input.sneak.
    // updatePose()/getExpectedPose() + canChangeIntoPose()-Fallback-Kette als reine Funktion
    // von `want_crouch` (der gewuenschten Schleich-Absicht) -> (swimming, crouching). Wird
    // zweimal aufgerufen: mit der aktuellen Sneak-Taste fuer die Hitbox (self.crouching) und
    // mit der Taste vom letzten Tick fuer die Bewegungs-Verlangsamung (in_sneaking_pose_move).
    // SWIMMING (kleinste Pose) passt immer; sonst gewuenschte Pose, dann Crouch-Fallback unter
    // einer Decke, sonst zwingend SWIMMING.
    // Vanillas PlayerEntity.updatePose() (Zeile 354), 1:1 -- inklusive der beiden Details,
    // die eine fruehere Fassung nicht hatte:
    //
    //   1. Der ZEITPUNKT. Aufgerufen wird das als letzte Anweisung von PlayerEntity.tick(),
    //      also NACH der Bewegung und an der Position nach der Bewegung. Deshalb steht der
    //      Aufruf am Ende von tick() und nicht mehr am Anfang (s. dortiger Kommentar).
    //   2. Das SWIMMING-Gate: `if (canChangeIntoPose(SWIMMING))`. Passt nicht einmal die
    //      kleinste Pose (0.6), wird GAR NICHTS geaendert -- die alte Pose bleibt stehen.
    //      Vorher fielen wir in so einem Fall auf SWIMMING zurueck, was in einer 0.5 hohen
    //      Luecke (z.B. unter einer Bodenplatte auf einer Bodenplatte) eine Pose erzeugte,
    //      die Vanilla dort nie annimmt.
    pub(crate) fn update_pose(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        if !pose_fits(self.pos, self.pose_height(SWIMMING_HEIGHT), world, shapes, self.shape_context(), self.body) {
            return;
        }
        // getDesiredPose() (vereinfacht, s. Feld-Kommentar): Schleichen zaehlt nur, wenn nicht
        // geschwommen wird (SWIMMING steht in Vanillas Prioritaetskette vor CROUCHING), und
        // liest die AKTUELLE Sneak-Taste -- die zu diesem Zeitpunkt, am Tick-Ende, dieselbe
        // ist, die Vanillas isSneaking() nach tickMovement dieses Ticks liefert.
        // SLEEPING steht ganz oben in der Kette: solange der Server uns im Bett fuehrt, wird
        // gar keine andere Pose ausgehandelt.
        // PlayerEntity.getExpectedPose: ... SWIMMING, GLIDING, dann SPIN_ATTACK, erst danach
        // CROUCHING. SPIN_ATTACK ist so klein wie SWIMMING und passt damit immer, sobald das
        // Gate oben durch ist -- ein Rueckfall wie bei STANDING/CROUCHING entfaellt.
        let glide = !self.sleeping && !self.raw_swimming && self.gliding;
        let spin = !self.sleeping && !self.raw_swimming && !glide && self.using_riptide;
        // getDesiredPose: fliegend kein Schleichen. Zuschauer bekommen die gewuenschte Pose,
        // ohne dass sie passen muss (updatePlayerPose).
        let schleichen = !self.raw_swimming && input.sneak && !self.abilities.flying;
        let (swim, crouch) = if self.sleeping || spin || glide {
            (false, false)
        } else if input.spectator {
            (self.raw_swimming, schleichen)
        } else {
            self.resolve_pose(schleichen, world, shapes)
        };
        self.swimming = swim;
        self.crouching = crouch;
        self.spin_attack = spin;
        self.glide_pose = glide;
    }

    // Loslassen eines Dreizacks mit Sog (TridentItem.onStoppedUsing, Client-Zweig): Schub in
    // Blickrichtung mit der Staerke aus der Verzauberung, dazu 20 Riptide-Ticks. Steht man,
    // hebt Vanilla einen zusaetzlich um 1.1999999 an (`move(MovementType.SELF, ...)`), damit
    // der Schub nicht am Boden verpufft.
    //
    // Alles in float wie in Vanilla: Winkel mal (float) PI/180, MathHelper.sin/cos aus der
    // Tabelle, MathHelper.sqrt als float. Erst addVelocity macht double daraus.
    //
    // `yaw_deg`/`pitch_deg` sind VANILLAS Winkel (Pitch positiv = nach unten).
    pub fn start_riptide(&mut self, yaw_deg: f32, pitch_deg: f32, strength: f32, world: &World, shapes: &world::BlockShapes) {
        const RADIANS_PER_DEGREE: f32 = (std::f64::consts::PI / 180.0) as f32;
        let yaw = f64::from(yaw_deg * RADIANS_PER_DEGREE);
        let pitch = f64::from(pitch_deg * RADIANS_PER_DEGREE);
        let mut x = -mc_sin(yaw) * mc_cos(pitch);
        let mut y = -mc_sin(pitch);
        let mut z = mc_cos(yaw) * mc_cos(pitch);
        let laenge = (x * x + y * y + z * z).sqrt();
        x *= strength / laenge;
        y *= strength / laenge;
        z *= strength / laenge;
        self.vel += DVec3::new(f64::from(x), f64::from(y), f64::from(z));
        self.riptide_ticks = RIPTIDE_TICKS;
        if self.on_ground {
            // Entity.move(SELF) nach oben: gegen Decken geklemmt; eine Bewegung nach oben ist
            // nie ein Bodenkontakt. Stoesst man oben an, nullt Block.onEntityLand die
            // Y-Geschwindigkeit -- wie bei jeder Vertikalkollision.
            let hub = DVec3::new(0.0, RIPTIDE_GROUND_LIFT, 0.0);
            let (neu, _, _, oben_an, _) =
                move_and_collide(self.pos, hub, self.current_height(), self.on_ground, world, shapes, self.shape_context(), self.body);
            self.pos = neu;
            self.on_ground = false;
            if oben_an {
                self.vel.y = 0.0;
            }
        }
    }

    // LivingEntity.tickMovement -> tickRiptide, direkt nach der Bewegung eines Ticks: der
    // Aufrufer ruft das unmittelbar nach tick() auf (dazwischen haengt in Vanilla nichts davon
    // ab). `vorher` ist die Position VOR der Bewegung dieses Ticks -- Vanilla prueft die
    // VEREINIGUNG der Box davor und danach. `andere` sind die Boxen aller anderen Entities
    // (min, max, ist ein Lebewesen) -- Vanillas getOtherEntities(this, box).
    //
    // Trifft die Box ein LEBEWESEN, endet der Sog und man prallt mit -0.2 zurueck. Trifft sie
    // nur Nicht-Lebewesen (Item, Pfeil ...), passiert gar nichts -- auch der Wand-Abbruch
    // entfaellt dann, das ist Vanillas else-if. Sonst beendet eine Wand den Sog.
    pub fn tick_riptide(&mut self, vorher: DVec3, andere: &[(DVec3, DVec3, bool)]) {
        if self.riptide_ticks == 0 {
            return;
        }
        self.riptide_ticks -= 1;
        let half = self.width() / 2.0;
        let hoehe = self.current_height();
        let min = DVec3::new(vorher.x.min(self.pos.x) - half, vorher.y.min(self.pos.y), vorher.z.min(self.pos.z) - half);
        let max = DVec3::new(vorher.x.max(self.pos.x) + half, vorher.y.max(self.pos.y) + hoehe, vorher.z.max(self.pos.z) + half);
        let getroffen: Vec<bool> = andere
            .iter()
            .filter(|(a, b, _)| a.x < max.x && b.x > min.x && a.y < max.y && b.y > min.y && a.z < max.z && b.z > min.z)
            .map(|&(_, _, lebt)| lebt)
            .collect();
        if !getroffen.is_empty() {
            if getroffen.contains(&true) {
                self.riptide_ticks = 0;
                self.vel *= -0.2;
            }
        } else if self.horizontal_collision {
            self.riptide_ticks = 0;
        }
    }

    pub(crate) fn resolve_pose(&self, want_crouch: bool, world: &World, shapes: &world::BlockShapes) -> (bool, bool) {
        if self.raw_swimming {
            return (true, false);
        }
        let desired_height = self.pose_height(if want_crouch { CROUCHING_HEIGHT } else { PLAYER_HEIGHT });
        if pose_fits(self.pos, desired_height, world, shapes, self.shape_context(), self.body) {
            (false, want_crouch)
        } else if !want_crouch && pose_fits(self.pos, self.pose_height(CROUCHING_HEIGHT), world, shapes, self.shape_context(), self.body) {
            (false, true)
        } else {
            (true, false)
        }
    }
}
