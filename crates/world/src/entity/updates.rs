// Entities ticken, Schaden, Ausruestung, Bewegung, Teleport und Anvisieren.

use super::*;
use super::anim_debug as anim;

impl Entities {
    // Einmal pro CLIENT-Tick aufrufen, nicht pro Frame: Position, Rotation, Schrittanimation
    // und Umhang sind allesamt Tick-Groessen (s. Entity::tick). Pro Frame gerechnet haengen
    // sie an der Bildrate -- genau das, was hier vermieden wird.
    //
    // Auch fuer STILLSTEHENDE Entities: die schicken keine Bewegungspakete, und gerade im
    // Stillstand faellt eine eingefrorene Armhaltung auf.
    // `eigener`: der eigene Spieler, wenn ihm Erfahrungsorbs folgen koennen (lebt, kein
    // Zuschauer) -- er steht nicht in dieser Liste, zaehlt fuer die Orbs aber mit (s. tick_orb).
    pub fn tick(&mut self, world: &crate::chunk::World, shapes: &crate::BlockShapes, eigener: Option<OrbPlayer>) {
        let spieler: Vec<OrbPlayer> = if self.by_id.values().any(|e| e.orb_physics) {
            eigener
                .into_iter()
                .chain(
                    self.by_id
                        .iter()
                        .filter(|(id, e)| is_player(e.entity_type) && Some(**id) != eigener.map(|p| p.id))
                        .map(|(&id, e)| OrbPlayer { id, pos: e.pos, eye_height: e.eye_height() }),
                )
                .collect()
        } else {
            Vec::new()
        };
        for e in self.by_id.values_mut() {
            e.tick(world, shapes, &spieler, self.body_turn_on_use);
        }
        // NACH dem Ticken der Fahrzeuge: der Passagier soll die Position DIESES Ticks
        // uebernehmen, nicht die des vorigen.
        self.tick_passengers();
    }

    // Schadens-Meldung des Servers auf ein Entity anwenden. Unbekannte IDs werden still
    // uebergangen: das Paket kann ein Entity betreffen, das wir gar nicht verfolgen.
    pub fn damaged(&mut self, id: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.on_damaged();
        }
    }

    // Ein Schlag-Paket des Servers auf ein Entity anwenden. Unbekannte IDs werden still
    // uebergangen -- wie bei `damaged` kann das Paket ein Entity betreffen, das wir nicht
    // verfolgen.
    pub fn swing_hand(&mut self, id: i32, off_hand: bool) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.swing_hand(off_hand);
        }
    }

    // Meldung eines kritischen Treffers auf ein Entity (s. Entity::on_crit).
    pub fn crit(&mut self, id: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.on_crit();
        }
    }

    // Set Equipment anwenden. Von den Tier-Slots zaehlen Sattel und Koerper fuers Reiten.
    pub fn set_equipment(&mut self, id: i32, slot: u8, item: Option<HeldItem>) {
        let Some(entity) = self.by_id.get_mut(&id) else { return };
        match slot {
            protocol::packets::EQUIPMENT_SLOT_MAINHAND => entity.main_hand = item,
            protocol::packets::EQUIPMENT_SLOT_OFFHAND => entity.off_hand = item,
            protocol::packets::EQUIPMENT_SLOT_BODY => entity.body_item = item,
            protocol::packets::EQUIPMENT_SLOT_SADDLE => entity.saddled = item.is_some(),
            _ => {
                if let Some(armor_slot) = crate::armor::ArmorSlot::from_equipment_slot(slot) {
                    entity.armor[armor_slot.index()] = item;
                }
            }
        }
    }

    pub fn remove(&mut self, ids: &[i32]) {
        for id in ids {
            if anim::anim_debug_an() && self.by_id.get(id).is_some_and(anim::anim_relevant) {
                let e = &self.by_id[id];
                anim::anim_log(format_args!("entfernt id={id} typ={} pos={}", entity_type_name(e.entity_type).unwrap_or("?"), anim::fmt_pos(e.pos)));
            }
            self.by_id.remove(id);
            // Mit dem Entity auch seine Mitfahr-Beziehungen aufloesen -- in beide
            // Richtungen, sonst haengt ein Passagier an einem Fahrzeug, das es nicht mehr
            // gibt (und bliebe fuer immer an dessen letzter Stelle stehen).
            self.vehicle_of.remove(id);
            self.vehicle_of.retain(|_, (v, _)| v != id);
        }
    }

    pub fn clear(&mut self) {
        self.by_id.clear();
        self.vehicle_of.clear();
    }

    // Relative Bewegung (Move Entity Pos / Pos+Rot). Das Delta bezieht sich auf die
    // autoritative SERVER-Position (Vanillas TrackedPosition.withDelta), NICHT auf die
    // gerade interpolierte -- sonst zoege sich der Interpolationsrueckstand durch jedes
    // weitere Paket und das Entity bliebe dauerhaft hinter seiner echten Position zurueck.
    pub fn move_relative(&mut self, id: i32, dx: f64, dy: f64, dz: f64) {
        let vorher = self.by_id.get(&id).map(|e| e.pos);
        if let Some(e) = self.by_id.get_mut(&id) {
            e.tracked_pos = (e.tracked_pos.0 + dx, e.tracked_pos.1 + dy, e.tracked_pos.2 + dz);
            e.refresh(Some(e.tracked_pos), None, None, Instant::now());
        }
        self.anim_log_pos(id, "bewegung_relativ", vorher);
    }

    // Set Entity Motion (0x63) fuer ein fremdes Entity -- Vanillas Entity.setVelocityClient.
    // Zaehlt nur fuer Projektile, alle anderen bewegen sich bei uns ueber ihre Positionspakete.
    pub fn set_velocity(&mut self, id: i32, vel: (f64, f64, f64)) {
        if let Some(e) = self.by_id.get_mut(&id) {
            if e.projectile.is_some() || e.item_physics || e.block_physics || e.orb_physics {
                e.vel = vel;
                // Eine neue Geschwindigkeit heisst: es fliegt wieder -- aber nur eine echte
                // (AbstractArrow.lerpMotion: lengthSqr > 0). Null laesst den Pfeil stecken.
                if vel.0 * vel.0 + vel.1 * vel.1 + vel.2 * vel.2 > 0.0 {
                    e.in_ground = false;
                }
            }
        }
    }

    // Blockzustand eines fallenden Blocks aus dem Spawn-Paket (s. Entity::falling_block).
    pub fn set_frame_facing(&mut self, id: i32, facing: i32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.frame_facing = facing.clamp(0, 5) as u8;
        }
    }

    pub fn set_falling_block(&mut self, id: i32, state: u32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            if e.block_physics {
                e.falling_block = Some(state);
            }
        }
    }

    // Den optischen Abschuss-Versatz setzen (s. Entity::launch_render_offset). Nur fuer
    // Projektile -- alles andere hat keinen Schuetzen.
    pub fn set_launch_offset(&mut self, id: i32, offset: (f32, f32, f32)) {
        if let Some(e) = self.by_id.get_mut(&id) {
            if e.projectile.is_some() {
                e.launch_offset = offset;
                e.launch_ticks = LAUNCH_BLEND_TICKS;
            }
        }
    }

    // Absolute Position (Entity Position Sync / Teleport Entity).
    pub fn set_position(&mut self, id: i32, x: f64, y: f64, z: f64) {
        let vorher = self.by_id.get(&id).map(|e| e.pos);
        if let Some(e) = self.by_id.get_mut(&id) {
            e.tracked_pos = (x, y, z);
            e.refresh(Some((x, y, z)), None, None, Instant::now());
        }
        self.anim_log_pos(id, "position_absolut", vorher);
    }

    // Teleport Entity: anders als Position Sync greift er auch bei einem selbst gelenkten
    // Fahrzeug (ClientPlayNetworkHandler.onEntityPosition setzt die Stelle in jedem Fall).
    pub fn teleport(&mut self, id: i32, x: f64, y: f64, z: f64, yaw_deg: f32, pitch_deg: f32) {
        if self.by_id.get(&id).is_some_and(|e| e.driven) {
            if let Some(e) = self.by_id.get_mut(&id) {
                e.tracked_pos = (x, y, z);
            }
            self.place_driven(id, (x, y, z), yaw_deg, pitch_deg);
            return;
        }
        self.set_position(id, x, y, z);
        self.set_rotation(id, yaw_deg, pitch_deg);
    }

    // Reine Rotation (Move Entity Rot): Position bleibt, wo sie ist -- Vanillas
    // updateTrackedAngles fuellt den Positionsteil aus dem laufenden Ziel auf.
    pub fn set_rotation(&mut self, id: i32, yaw_deg: f32, pitch_deg: f32) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.refresh(None, Some(yaw_deg), Some(pitch_deg), Instant::now());
        }
        self.anim_log_pos(id, "rotation", None);
    }

    // Raycast gegen die Entity-Trefferboxen: fusszentrierte AABB an der TICK-quantisierten
    // Position (s. INTERPOLATION_STEPS), Groesse nach Typ und Pose (s. hit_box_size). Liefert
    // die Entity-ID mit dem NAECHSTEN Eintrittspunkt innerhalb von `max_dist` Bloecken, sonst
    // None.
    //
    // Genommen wird der Tick-Stand, nicht der Zwischentick-Stand des Bildes: das ist die Box,
    // auf die sich Server und GrimAC geeinigt haben.
    #[must_use] 
    pub fn pick(&self, origin: (f64, f64, f64), dir: (f64, f64, f64), max_dist: f64) -> Option<i32> {
        self.pick_hit(origin, dir, max_dist).map(|(id, _)| id)
    }

    // Wie pick, zusaetzlich mit der Strahllaenge bis zum Eintritt in die Box (in Einheiten
    // von `dir`) -- der Rechtsklick braucht die Trefferstelle (INTERACT_AT, s. app).
    #[must_use] 
    pub fn pick_hit(&self, origin: (f64, f64, f64), dir: (f64, f64, f64), max_dist: f64) -> Option<(i32, f64)> {
        let mut best: Option<(i32, f64)> = None;
        for (&id, e) in &self.by_id {
            // Nicht-angreifbare Ziele (Items, XP-Kugeln, Pfeile) ueberspringen -- ein
            // Attack darauf waere ein sofortiger Server-Kick (s. is_attackable).
            if !is_attackable(e.entity_type) {
                continue;
            }
            // Display-Entities sind reine Anzeige: Vanillas Entity.canHit() ist ohne
            // LivingEntity-Ueberschreibung `false`, und ihre Box hat mit WIDTH/HEIGHT = 0
            // ohnehin kein Volumen. Ohne diese Zeile schluckte ein Hologramm, das vor einem
            // Gegner haengt, den Schlag -- und zwar unsichtbar, weil man nur den Text sieht.
            if e.display.is_some() {
                continue;
            }
            // Ein interaction-Entity traegt seine Groesse in den Metadaten (s. interaction_size):
            // damit machen Server Display-Entities anklickbar und schlagbar. Mit den festen
            // Typmassen (0 x 0) liess es sich nie treffen (etwa auf mcpvp.club).
            let (width, height) = e.hit_size();
            let half = width / 2.0;
            let (x, y, z) = e.hit_pos();
            let min = (x - half, y, z - half);
            let max = (x + half, y + height, z + half);
            if let Some(t) = ray_aabb(origin, dir, min, max) {
                if t <= max_dist && best.is_none_or(|(_, bt)| t < bt) {
                    best = Some((id, t));
                }
            }
        }
        best
    }

    // Die Reichweite eines Items mit attack_range (Speere, Server-Waffen), wie Vanillas
    // ProjectileUtil.getHitEntitiesAlong: getroffen wird, was der Abschnitt des Strahls von
    // `t0` bis `t1` beruehrt (Einheiten von `dir`, das normiert sein muss) -- ein Gegner naeher
    // als `t0` also nicht. Mit `margin` > 0 zaehlt auch ein knapper Vorbeischuss an der um
    // `margin` vergroesserten Box, aber nur, wenn vom Beruehrpunkt zur Boxmitte kein Block vor
    // dem Gegner liegt: `free(von, nach)` sagt, welcher Anteil der Strecke frei ist (1 = ganz).
    // Zurueck: das naechste Ziel und der Abstand seines Trefferpunkts vom Ursprung.
    pub fn pick_segment(
        &self,
        origin: (f64, f64, f64),
        dir: (f64, f64, f64),
        t0: f64,
        t1: f64,
        margin: f64,
        free: &dyn Fn((f64, f64, f64), (f64, f64, f64)) -> f64,
    ) -> Option<(i32, f64)> {
        let punkt = |o: (f64, f64, f64), d: (f64, f64, f64), t: f64| (o.0 + d.0 * t, o.1 + d.1 * t, o.2 + d.2 * t);
        let start = punkt(origin, dir, t0);
        let mut best: Option<(i32, f64)> = None;
        for (&id, e) in &self.by_id {
            // Dieselbe Auswahl wie pick_hit.
            if !is_attackable(e.entity_type) || e.display.is_some() {
                continue;
            }
            let (width, height) = e.hit_size();
            let half = width / 2.0;
            let (x, y, z) = e.hit_pos();
            let (min, max) = ((x - half, y, z - half), (x + half, y + height, z + half));
            let innen = |p: (f64, f64, f64)| p.0 >= min.0 && p.0 <= max.0 && p.1 >= min.1 && p.1 <= max.1 && p.2 >= min.2 && p.2 <= max.2;
            let dist = if innen(start) {
                Some(t0)
            } else if let Some(t) = ray_aabb(origin, dir, min, max).filter(|t| (t0..=t1).contains(t)) {
                Some(t)
            } else if margin > 0.0 {
                let gross = ((min.0 - margin, min.1 - margin, min.2 - margin), (max.0 + margin, max.1 + margin, max.2 + margin));
                ray_aabb(origin, dir, gross.0, gross.1).filter(|t| (t0..=t1).contains(t)).and_then(|t| {
                    let p = punkt(origin, dir, t);
                    let mitte = (x, y + height / 2.0, z);
                    let f = free(p, mitte).clamp(0.0, 1.0);
                    let weg = ((mitte.0 - p.0) * f, (mitte.1 - p.1) * f, (mitte.2 - p.2) * f);
                    let s = ray_aabb(p, weg, min, max).filter(|s| *s <= 1.0)?;
                    let q = punkt(p, weg, s);
                    Some(((q.0 - origin.0).powi(2) + (q.1 - origin.1).powi(2) + (q.2 - origin.2).powi(2)).sqrt())
                })
            } else {
                None
            };
            if let Some(d) = dist {
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((id, d));
                }
            }
        }
        best
    }
}
