// Blockberuehrung: Kollisionseffekte, Blasensaeulen, Betreten und Honig.

use super::*;

impl PlayerPhysics {
    // Entity.tickBlockCollision() -> checkBlockCollision(): jede Blockzelle, die die
    // Spielerbox am ENDE der Bewegung beruehrt, bekommt onEntityCollision. Nachgebildet ist
    // davon nur der Teil, der die Bewegung betrifft (slowMovement, s.
    // world::slow_movement_multiplier) -- Feuer, Portale, Trichter und der ganze Rest sind
    // Server-Sache und hier ohne Wirkung.
    //
    // Die Box wird wie in Vanilla um 1.0E-5 nach INNEN geschrumpft
    // (calculateDefaultBoundingBox, dann contract): buendiges Anliegen an einer
    // Zellgrenze zaehlt damit NICHT als Beruehrung. Ohne das Schrumpfen loeste ein Netz
    // schon aus, wenn man exakt an seiner Kante steht -- der Server sieht es dann nicht, und
    // die Simulationen laufen auseinander.
    //
    // Die Formpruefung aus Vanilla faellt weg, weil sie fuer unseren einzigen Fall immer
    // wahr ist: getInsideCollisionShape() ist in AbstractBlock der VOLLE Wuerfel, und
    // CobwebBlock ueberschreibt das nicht -- `voxelShape == VoxelShapes.fullCube()` greift
    // also sofort. Kaeme hier je ein Block mit kleinerer Innenform dazu, muesste die
    // Ueberlappung wirklich gegen dessen Form geprueft werden.
    pub(crate) fn tick_block_collision(&mut self, input: &Input, world: &World, shapes: &world::BlockShapes) {
        // Entity.tickBlockCollisions(): VOR der Zellen-Schleife und nur am Boden bekommt der
        // Block unter den Fuessen (getLandingPos) onSteppedOn.
        if self.on_ground {
            self.stepped_on_block(input, world);
        }
        const SHRINK: f64 = 1.0e-5;
        let half = self.width() / 2.0;
        let min = DVec3::new(self.pos.x - half + SHRINK, self.pos.y + SHRINK, self.pos.z - half + SHRINK);
        let max = DVec3::new(self.pos.x + half - SHRINK, self.pos.y + self.current_height() - SHRINK, self.pos.z + half - SHRINK);
        // Leere Box (Schrumpfen groesser als die Box selbst) beruehrt nichts.
        if min.x > max.x || min.y > max.y || min.z > max.z {
            return;
        }
        // Netz und Honig einmal gemeinsam einsammeln, statt beim ersten Netz auszusteigen:
        // Vanilla ruft onEntityCollision fuer JEDE beruehrte Zelle, ein Netz darf ein Honig-
        // Rutschen in einer anderen Zelle also nicht verschlucken. Innerhalb einer Blockart
        // reicht die erste Zelle (mehrere setzen denselben Wert nacheinander).
        let mut netz: Option<[f64; 3]> = None;
        let mut honig: Vec<(i32, i32, i32)> = Vec::new();
        for bx in min.x.floor() as i32..=max.x.floor() as i32 {
            for by in min.y.floor() as i32..=max.y.floor() as i32 {
                for bz in min.z.floor() as i32..=max.z.floor() as i32 {
                    let state = world.block_at(bx, by, bz);
                    if netz.is_none() {
                        netz = world::slow_movement_multiplier(state, input.weaving);
                    }
                    if world::block_registry::block_name(state) == Some("honey_block") {
                        honig.push((bx, by, bz));
                    }
                }
            }
        }
        // Player.makeStuckInBlock: fliegend haelt kein Netz fest.
        if let (Some(m), false) = (netz, self.abilities.flying) {
            // Entity.slowMovement(): setzt den Faktor UND ruft onLanding() -- deshalb
            // schuetzt ein Netz vor Fallschaden, auch wenn man nur hindurchfaellt.
            self.movement_multiplier = DVec3::new(m[0], m[1], m[2]);
            self.fall_distance = 0.0;
        }
        // Jede beruehrte Honigzelle darf es versuchen: isSliding prueft die Hoehe GEGEN DIE
        // JEWEILIGE ZELLE, eine tiefere kann also ablehnen, waehrend die daruberliegende
        // zustimmt. Die erste, die greift, gewinnt -- ein zweiter Durchlauf waere ohnehin
        // wirkungslos (danach liegt die Geschwindigkeit ueber der -0.08-Schwelle).
        for pos in honig {
            if self.slide_down_honey(pos) {
                break;
            }
        }
        self.bubble_columns(min, max, world, shapes);
    }

    // BubbleColumnBlock.entityInside -> Entity.onInside/onAboveBubbleColumn (1.21.11,
    // nachgesehen): JEDE beruehrte Saeulenzelle schiebt einmal, in Vanillas Reihenfolge der
    // Zellen (BlockPos.betweenClosed: z aussen, y, x innen) -- die Obergrenzen greifen sonst
    // an anderer Stelle. Ist ueber der Zelle weder Kollisionsform noch Fluid, gilt sie als
    // Oberkante: dort staerker, und ohne Ruecksetzen der Fallstrecke.
    //
    // ECHTER BUG gewesen (Grim "Simulation" x26 auf anticheat-test.com): Blasensaeulen
    // fehlten ganz -- der Server schob uns, wir nicht.
    pub(crate) fn bubble_columns(&mut self, min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) {
        // Player.onInside/onAboveBubbleColumn: fliegend nichts.
        if self.abilities.flying {
            return;
        }
        for bz in min.z.floor() as i32..=max.z.floor() as i32 {
            for by in min.y.floor() as i32..=max.y.floor() as i32 {
                for bx in min.x.floor() as i32..=max.x.floor() as i32 {
                    let state = world.block_at(bx, by, bz);
                    if world::block_registry::block_name(state) != Some("bubble_column") {
                        continue;
                    }
                    let runter = world::state_props::property(state, "drag") == Some("true");
                    let oben = world.block_at(bx, by + 1, bz);
                    let oberkante = world::collision_boxes(oben, shapes).is_empty() && world::fluid_kind(oben).is_none();
                    let y = self.vel.y;
                    self.vel.y = match (oberkante, runter) {
                        (true, true) => (y - 0.03).max(-0.9),
                        (true, false) => (y + 0.1).min(1.8),
                        (false, true) => (y - 0.03).max(-0.3),
                        (false, false) => (y + 0.06).min(0.7),
                    };
                    if !oberkante {
                        self.fall_distance = 0.0;
                    }
                }
            }
        }
    }

    // SlimeBlock.onSteppedOn: wer ueber einen Schleimblock LAEUFT, wird horizontal stark
    // gebremst -- unabhaengig von der Reibung (0.8), die zusaetzlich wirkt. Nur bei
    // betragsmaessig kleiner Vertikalgeschwindigkeit (< 0.1), also im Gehen und nicht mitten
    // im Absprung, und nicht beim Schleichen (bypassesSteppingEffects() == isSneaking(), beim
    // Client die reine Taste). Genau deshalb kommt man schleichend normal ueber Schleim.
    pub(crate) fn stepped_on_block(&mut self, input: &Input, world: &World) {
        if world::block_registry::block_name(block_at_pos(world, self.landing_pos(world))) != Some("slime_block") {
            return;
        }
        let d = self.vel.y.abs();
        if d < 0.1 && !input.sneak {
            let e = 0.4 + d * 0.2;
            self.vel.x *= e;
            self.vel.z *= e;
        }
    }

    // HoneyBlock.onEntityCollision/isSliding/updateSlidingVelocity: an der SEITE eines
    // Honigblocks nach unten rutschen statt frei zu fallen.
    //
    // Die beiden Umrechnungen sind der Kern und leicht zu uebersehen: Vanilla prueft und setzt
    // die Geschwindigkeit, NACHDEM travel() Schwerkraft und Luftwiderstand schon draufgerechnet
    // hat, rechnet fuer seine Grenzwerte aber mit dem Wert von DAVOR --
    //   getOldVelocityY(d) = d / 0.98F + 0.08   (Schwerkraft/Daempfung herausgerechnet)
    //   getNewVelocityY(d) = (d - 0.08) * 0.98F (Zielwert wieder hineingerechnet)
    // -- damit die Rutschgeschwindigkeit nach dem naechsten Tick exakt -0.05 ist. Unsere
    // tick_block_collision laeuft an derselben Stelle (nach tick_air), die Umrechnung passt
    // also unveraendert. 0.98F ist ein float, deshalb hier ueber f32 gerechnet.
    pub(crate) fn slide_down_honey(&mut self, pos: (i32, i32, i32)) -> bool {
        const DRAG_F32: f64 = 0.98f32 as f64;
        let old_y = |v: f64| v / DRAG_F32 + 0.08;
        // isSliding(): am Boden rutscht niemand; wer schon fast oben auf dem Block steht
        // ebenso wenig; und es braucht echten Fall (> 0.08 nach Herausrechnen).
        if self.on_ground || self.pos.y > f64::from(pos.1) + 0.9375 - 1.0e-7 || old_y(self.vel.y) >= -0.08 {
            return false;
        }
        // Der Mittelpunktsabstand: nur an der SEITE des Blocks wird gerutscht, nicht wenn man
        // mittig ueber/in ihm haengt. 0.4375 + halbe Spielerbreite = 0.7375.
        let dx = (f64::from(pos.0) + 0.5 - self.pos.x).abs();
        let dz = (f64::from(pos.2) + 0.5 - self.pos.z).abs();
        let grenze = 0.4375 + self.width() / 2.0;
        if !(dx + 1.0e-7 > grenze || dz + 1.0e-7 > grenze) {
            return false;
        }
        // updateSlidingVelocity(): oberhalb von -0.13 nur die Y-Bremse, darunter zusaetzlich
        // die Horizontalgeschwindigkeit im selben Verhaeltnis stauchen (sonst wuerde man beim
        // Abbremsen seitlich wegschiessen).
        let vor = old_y(self.vel.y);
        if vor < -0.13 {
            let d = -0.05 / vor;
            self.vel.x *= d;
            self.vel.z *= d;
        }
        self.vel.y = (-0.05 - 0.08) * DRAG_F32;
        // entity.onLanding(): Rutschen setzt die Fallhoehe zurueck -- deshalb nimmt man an
        // einer Honigwand keinen Fallschaden.
        self.fall_distance = 0.0;
        true
    }
}
