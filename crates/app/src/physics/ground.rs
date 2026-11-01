// Boden unter den Fuessen: Reibung, tragender Block, Klettern und Block-Multiplikatoren.

use super::*;

impl PlayerPhysics {
    // Nach einem Server-Teleport (/tp, Setback-Korrektur, Respawn) setzt new() on_ground
    // hart auf false, obwohl die Zielposition sehr oft schon auf einem Block steht (z.B.
    // eine Korrektur waehrend ganz normalen Laufens am Boden). Der naechste echte Tick
    // wuerde das zwar von selbst richtigstellen, aber GENAU dieser eine "false"-Report
    // direkt nach dem Teleport wurde von Grim als GroundSpoof gewertet ("claimed false" --
    // wir behaupten in der Luft zu sein, obwohl wir es nicht sind, und das kaskadierte
    // bei jeder Server-Korrektur zusaetzlich). Deshalb sofort nach dem Reset pruefen, ohne
    // auf den naechsten Tick zu warten -- Sonde mit winziger Abwaertsgeschwindigkeit durch
    // dieselbe Kollisionslogik wie ein echter Tick, aber ohne Position/Geschwindigkeit zu
    // veraendern.
    // Entity.getVelocityAffectingPos() ohne den supportingBlockPos-Cache (den wir nicht
    // fuehren): floor(x), floor(y - 0.500001), floor(z) -- Fallback-Pfad aus
    // getPosWithYOffset(), der greift, wann immer keine gecachte Standflaeche vorliegt.
    // Bei einem Spieler exakt auf einem vollen Block trifft das denselben Block wie der
    // Cache getroffen haette.
    pub(crate) fn ground_friction(&self, world: &World) -> f64 {
        let state = block_at_pos(world, self.velocity_affecting_pos(world));
        world::block_registry::block_name(state).map_or(DEFAULT_BLOCK_FRICTION, block_friction)
    }

    // Entity.getVelocityAffectingPos() = getPosWithYOffset(0.500001F): der Block, dessen
    // Reibung und Geschwindigkeitsfaktor gelten.
    pub(crate) fn velocity_affecting_pos(&self, world: &World) -> (i32, i32, i32) {
        self.pos_with_y_offset(f64::from(0.500001f32), world)
    }

    // Vanillas ShapeContext.of(entity), auf das reduziert, was eine Form aendern kann: die
    // Fusshoehe (Box-Unterkante) und isDescending() == isSneaking(). Nur das Geruest liest ihn.
    pub(crate) const fn shape_context(&self) -> world::ShapeContext {
        world::ShapeContext { feet_y: self.pos.y, descending: self.sneak_key }
    }

    // Entity.getBlockPos(): die Zelle, IN der die Fuesse stecken (BlockPos.ofFloored(getPos())).
    // Die einzige der drei Abfragen OHNE Standblock-Bezug.
    pub(crate) const fn feet_block_pos(&self) -> (i32, i32, i32) {
        (self.pos.x.floor() as i32, self.pos.y.floor() as i32, self.pos.z.floor() as i32)
    }

    // Entity.getLandingPos() = getPosWithYOffset(0.2F): der Block, auf dem man gelandet ist
    // bzw. steht. Fuer den Schleimblock-Absprung und onSteppedOn.
    pub(crate) fn landing_pos(&self, world: &World) -> (i32, i32, i32) {
        self.pos_with_y_offset(f64::from(0.2f32), world)
    }

    // Entity.getPosWithYOffset(offset). Der Kern der ganzen Standblock-Buchhaltung:
    //
    //   - OHNE bekannten Standblock der naive Fallback floor(x)/floor(y - offset)/floor(z).
    //   - MIT Standblock kommen X und Z VOM STANDBLOCK, nur Y wird weiterhin aus der
    //     Spielerposition gerechnet. Genau das ist der Unterschied, der bei Honig zaehlt: wer
    //     auf Stein steht und mit einem Fuss ueber die 1/16 tiefere Honig-Mulde ragt, bekommt
    //     naiv den Honig untergeschoben (X/Z aus der Spielermitte), tatsaechlich traegt ihn
    //     aber der Stein. Vanilla UND GrimAC rechnen mit dem Standblock (Grim:
    //     BlockProperties.getOnPos ueber MainSupportingBlockData) -- die naive Fassung wich
    //     an jedem Uebergang auf einen Honigblock um den vollen Faktor 0.4 ab.
    //   - Zaun/Mauer/Zauntor sind ausgenommen: dort zaehlt der Standblock selbst, weil ihre
    //     Kollisionsbox hoeher ist als ihre Zelle und floor(y - offset) sonst darunter zielt.
    //
    // Der Versatz ist in Vanilla ein FLOAT (0.500001F/0.2F), deshalb ueberall ueber f32
    // gereicht -- 0.500001F ist als double 0.5000010132789612, nicht 0.500001.
    pub(crate) fn pos_with_y_offset(&self, offset: f64, world: &World) -> (i32, i32, i32) {
        let Some(p) = self.supporting_block else {
            return (self.pos.x.floor() as i32, (self.pos.y - offset).floor() as i32, self.pos.z.floor() as i32);
        };
        if offset <= 1.0e-5 {
            return p;
        }
        let name = world::block_registry::block_name(block_at_pos(world, p));
        let ist_zaun = name.is_some_and(|n| n.ends_with("_fence") || n == "fence");
        let ist_mauer = name.is_some_and(|n| n.ends_with("_wall"));
        let ist_tor = name.is_some_and(|n| n.ends_with("_fence_gate"));
        if (offset > 0.5 || !ist_zaun) && !ist_mauer && !ist_tor {
            (p.0, (self.pos.y - offset).floor() as i32, p.2)
        } else {
            p
        }
    }

    // Entity.updateSupportingBlockPos(): nach JEDER Bewegung aufgerufen, mit dem Bodenkontakt
    // und der TATSAECHLICH zurueckgelegten Bewegung dieses Ticks.
    //
    // Der zweite Anlauf mit der um die Bewegung zurueckgeschobenen Box ist kein Beiwerk: wer
    // ueber eine Kante hinauslaeuft, hat am Ende des Ticks unter sich nichts mehr, gilt aber
    // noch als am Boden -- dann traegt ihn der Block, ueber dem er zu Beginn des Ticks stand.
    pub(crate) fn update_supporting_block(&mut self, on_ground: bool, movement: DVec3, world: &World, shapes: &world::BlockShapes) {
        if !on_ground {
            self.force_update_supporting_block = false;
            self.supporting_block = None;
            return;
        }
        let half = self.width() / 2.0;
        let min = DVec3::new(self.pos.x - half, self.pos.y - 1.0e-6, self.pos.z - half);
        let max = DVec3::new(self.pos.x + half, self.pos.y, self.pos.z + half);
        let mut gefunden = self.find_supporting_block(min, max, world, shapes);
        if gefunden.is_some() || self.force_update_supporting_block {
            self.supporting_block = gefunden;
        } else {
            let versatz = DVec3::new(-movement.x, 0.0, -movement.z);
            gefunden = self.find_supporting_block(min + versatz, max + versatz, world, shapes);
            self.supporting_block = gefunden;
        }
        self.force_update_supporting_block = gefunden.is_none();
    }

    // CollisionView.findSupportingBlockPos(): unter allen Bloecken, deren Kollisionsform die
    // duenne Box unter den Fuessen schneidet, gewinnt der mit dem kleinsten Abstand von der
    // Spielerposition zur BLOCKMITTE (Vec3i.getSquaredDistance(Position) rechnet ueber
    // getSquaredDistanceFromCenter, also +0.5 je Achse). Bei Gleichstand entscheidet Vanillas
    // BlockPos-Ordnung (erst Y, dann Z, dann X), und zwar so, dass der GROESSERE gewinnt --
    // deshalb hier `bisher < neu` als Ersetzungsbedingung, 1:1 wie im Jar.
    pub(crate) fn find_supporting_block(&self, min: DVec3, max: DVec3, world: &World, shapes: &world::BlockShapes) -> Option<(i32, i32, i32)> {
        let mut best: Option<(i32, i32, i32)> = None;
        let mut best_dist = f64::MAX;
        for bx in min.x.floor() as i32..=max.x.floor() as i32 {
            for by in min.y.floor() as i32..=max.y.floor() as i32 {
                for bz in min.z.floor() as i32..=max.z.floor() as i32 {
                    let state = world.block_at(bx, by, bz);
                    let (odx, odz) = world::block_name(state)
                        .map_or((0.0, 0.0), |name| world::block_xz_offset(name, bx, bz));
                    let trifft = world::collision_boxes_at(state, shapes, by, self.shape_context()).iter().any(|cbox| {
                        let bmin = DVec3::new(
                            f64::from(bx) + f64::from(odx) + f64::from(cbox.min[0]),
                            f64::from(by) + f64::from(cbox.min[1]),
                            f64::from(bz) + f64::from(odz) + f64::from(cbox.min[2]),
                        );
                        let bmax = DVec3::new(
                            f64::from(bx) + f64::from(odx) + f64::from(cbox.max[0]),
                            f64::from(by) + f64::from(cbox.max[1]),
                            f64::from(bz) + f64::from(odz) + f64::from(cbox.max[2]),
                        );
                        // Box.intersects: echte Ueberschneidung, blosses Anliegen zaehlt nicht.
                        bmin.x < max.x && bmax.x > min.x && bmin.y < max.y && bmax.y > min.y && bmin.z < max.z && bmax.z > min.z
                    });
                    if !trifft {
                        continue;
                    }
                    let dx = f64::from(bx) + 0.5 - self.pos.x;
                    let dy = f64::from(by) + 0.5 - self.pos.y;
                    let dz = f64::from(bz) + 0.5 - self.pos.z;
                    let dist = dx * dx + dy * dy + dz * dz;
                    let besser = dist < best_dist || (dist == best_dist && best.is_none_or(|b| (b.1, b.2, b.0) < (by, bz, bx)));
                    if besser {
                        best = Some((bx, by, bz));
                        best_dist = dist;
                    }
                }
            }
        }
        best
    }

    // LivingEntity.isClimbing(): entscheidend ist ALLEIN der Block, IN dem die Fuesse stecken
    // (getBlockPos()) -- nicht, ob man eine Leiter beruehrt. Deshalb klettert man auch an einer
    // Ranke, die man nur durchquert, und deshalb hoert das Klettern sofort auf, sobald die
    // Fusszelle wieder Luft ist.
    //
    // Zweiter Fall: eine OFFENE Falltuer direkt ueber einer Leiter, die in dieselbe Richtung
    // zeigt (canEnterTrapdoor) -- der bekannte Vanilla-Trick, um eine Leiter oben mit einer
    // Falltuer abzuschliessen und trotzdem durchzukommen.
    //
    // Gleitend zaehlen Ranken nicht (Block-Tag can_glide_through) -- man fliegt durch sie
    // hindurch, statt an ihnen haengen zu bleiben. NICHT nachgebildet: isSpectator(), weil der
    // Client keinen Spectator-Modus modelliert.
    pub fn is_climbing(&self, world: &World) -> bool {
        let pos = self.feet_block_pos();
        let state = block_at_pos(world, pos);
        let Some(name) = world::block_registry::block_name(state) else { return false };
        if self.gliding && CAN_GLIDE_THROUGH.contains(&name) {
            return false;
        }
        if CLIMBABLE.contains(&name) {
            return true;
        }
        if !name.ends_with("trapdoor") || world::state_props::property(state, "open") != Some("true") {
            return false;
        }
        let unten = block_at_pos(world, (pos.0, pos.1 - 1, pos.2));
        world::block_registry::block_name(unten) == Some("ladder")
            && world::state_props::property(unten, "facing") == world::state_props::property(state, "facing")
    }

    // LivingEntity.applyClimbingSpeed(), aufgerufen ZWISCHEN Beschleunigung und Bewegung:
    //
    //   onLanding();                                  // Fallhoehe zurueck -> kein Fallschaden
    //   x, z auf +/-0.15 geklemmt, y nach unten auf -0.15 begrenzt
    //   if (y < 0 && Fusszelle != Geruest && isHoldingOntoLadder() && Spieler) y = 0;
    //
    // isHoldingOntoLadder() ist beim Spieler isSneaking(), also die Sneak-TASTE: schleichend
    // haelt man sich an der Leiter fest. Das Geruest ist davon ausdruecklich ausgenommen --
    // dort ist Schleichen genau das Mittel zum Absteigen (und laesst zusaetzlich die
    // Deckplatte der Kollisionsform wegfallen, s. vanilla_shapes::scaffolding_boxes).
    pub(crate) fn apply_climbing_speed(&mut self, world: &World) {
        if !self.is_climbing(world) {
            return;
        }
        self.fall_distance = 0.0;
        self.vel.x = self.vel.x.clamp(-CLIMB_SPEED, CLIMB_SPEED);
        self.vel.z = self.vel.z.clamp(-CLIMB_SPEED, CLIMB_SPEED);
        self.vel.y = self.vel.y.max(-CLIMB_SPEED);
        let auf_geruest = world::block_registry::block_name(block_at_pos(world, self.feet_block_pos())) == Some("scaffolding");
        if self.vel.y < 0.0 && !auf_geruest && self.sneak_key {
            self.vel.y = 0.0;
        }
    }

    // Entity.getVelocityMultiplier(): erst der Block, IN dem man steht -- nur wenn der neutral
    // ist (1.0), zaehlt der Block darunter. Steht man in Wasser oder einer Blasensaeule, wird
    // der Block darunter GAR NICHT befragt: Seelensand unter Wasser bremst also nicht (dort
    // schiebt stattdessen die Blasensaeule, s. bubble_columns).
    pub(crate) fn velocity_multiplier(&self, world: &World) -> f64 {
        // PlayerEntity.getVelocityMultiplier: gleitend bremst weder Seelensand noch Honig.
        if self.gliding {
            return 1.0;
        }
        let f = self.block_speed_factor(world);
        // LivingEntity.getBlockSpeedFactor: MOVEMENT_EFFICIENCY hebt die Bremse anteilig auf
        // (Mth.lerp in float). Ohne Attribut bleibt der Faktor unberuehrt.
        if self.body.movement_efficiency == 0.0 {
            return f;
        }
        let eff = self.body.movement_efficiency as f32;
        let f = f as f32;
        f64::from(f + eff * (1.0 - f))
    }

    // Entity.getBlockSpeedFactor: der Bremsfaktor des Blocks, ohne Attribut.
    pub(crate) fn block_speed_factor(&self, world: &World) -> f64 {
        let feet = block_at_pos(world, self.feet_block_pos());
        let name = world::block_registry::block_name(feet);
        let f = name.map_or(1.0, block_velocity_multiplier);
        if matches!(name, Some("water" | "bubble_column")) {
            return f;
        }
        if f != 1.0 {
            return f;
        }
        world::block_registry::block_name(block_at_pos(world, self.velocity_affecting_pos(world)))
            .map_or(1.0, block_velocity_multiplier)
    }

    // Entity.getJumpVelocityMultiplier(): dieselbe Zwei-Stufen-Abfrage wie oben, aber OHNE die
    // Wasser-Ausnahme.
    pub(crate) fn jump_multiplier(&self, world: &World) -> f32 {
        let f = world::block_registry::block_name(block_at_pos(world, self.feet_block_pos())).map_or(1.0, block_jump_multiplier);
        if f != 1.0 {
            return f;
        }
        world::block_registry::block_name(block_at_pos(world, self.velocity_affecting_pos(world))).map_or(1.0, block_jump_multiplier)
    }
}
