// Entities erzeugen, Pose setzen und Metadaten anwenden.

use super::*;

impl Entities {
    pub fn spawn(
        &mut self,
        id: i32,
        uuid: [u8; 16],
        entity_type: i32,
        pos: (f64, f64, f64),
        yaw_deg: f32,
        pitch_deg: f32,
        vel: (f64, f64, f64),
    ) {
        let now = Instant::now();
        // EINE UUID, EIN Entity -- exakt Vanillas EntityIndex.add: ist die UUID schon
        // vergeben, gibt es nur eine Warnung im Log.
        //
        // Vanilla legt den Zweiten also GAR NICHT an. Ohne diese Regel stand jedes doppelt
        // gespawnte Entity zweimal in der Welt -- mit zwei Koerpern und zwei Namensschildern
        // uebereinander.
        //
        // Die Null-UUID ist ausgenommen: sie ist kein echter Schluessel, sondern das, was
        // hier steht, wenn ein Paket gar keine mitbringt.
        if uuid != [0u8; 16] && self.by_id.iter().any(|(&vid, e)| e.uuid == uuid && vid != id) {
            self.duplicate_uuids = self.duplicate_uuids.saturating_add(1);
            return;
        }
        // insert statt Entry-Pruefung: schickt der Server dieselbe ID erneut (Respawn/
        // Wiederbetreten des Trackingbereichs), ist der neue Zustand massgeblich.
        self.by_id.insert(
            id,
            Entity {
                uuid,
                entity_type,
                pos,
                last_pos: pos,
                tracked_pos: pos,
                target_pos: pos,
                target_yaw_deg: yaw_deg,
                target_pitch_deg: pitch_deg,
                // Beim Spawn steht das Entity schon dort, wo es hingehoert -- keine
                // Angleichung offen (Vanilla: der Interpolator startet mit step = 0).
                interp_steps: 0,
                vel,
                in_ground: false,
                crit_arrow: false,
                interaction_size: (1.0, 1.0),
                projectile: projectile_kind(entity_type),
                item_physics: entity_type_name(entity_type) == Some("item"),
                // Gezuendetes TNT fliegt wie ein fallender Block (PrimedTnt.tick), s.
                // tick_falling_block.
                block_physics: matches!(entity_type_name(entity_type), Some("falling_block" | "tnt")),
                orb_physics: entity_type_name(entity_type) == Some("experience_orb"),
                orb_target: None,
                falling_block: None,
                frame_facing: 3,
                frame_rotation: 0,
                baby: false,
                scale: 1.0,
                villager_data: None,
                on_ground: false,
                launch_offset: (0.0, 0.0, 0.0),
                launch_ticks: 0,
                yaw_deg,
                pitch_deg,
                last_yaw_deg: yaw_deg,
                last_pitch_deg: pitch_deg,
                updated_at: now,
                update_intervals: VecDeque::new(),
                has_moved: false,
                limbs: crate::entity_model::LimbAnimator::new(),
                cape: crate::entity_model::CapeState::new(),
                age_ticks: 0.0,
                head_yaw_deg: yaw_deg,
                last_head_yaw_deg: yaw_deg,
                body_yaw_deg: yaw_deg,
                last_body_yaw_deg: yaw_deg,
                use_ticks: 0,
                server_head_yaw_deg: yaw_deg,
                head_steps: 0,
                // Leere Haende, bis das erste Set Equipment kommt. Der Server schickt es
                // fuer jedes Entity direkt nach dem Spawn, sofern ueberhaupt etwas getragen
                // wird -- ein Entity ohne Ausruestung bekommt gar kein Paket.
                main_hand: None,
                off_hand: None,
                armor: [None, None, None, None],
                // Vanillas Vorgabewert, bis der Server etwas anderes meldet.
                pose: 0,
                hurt_ticks: 0,
                ticks_since_hurt: u32::MAX,
                hurt_by_crit: false,
                crit_grace: 0,
                driven: false,
                driven_next: None,
                saddled: false,
                body_item: None,
                hand_swinging: false,
                hand_swing_ticks: 0,
                hand_swing_progress: 0.0,
                last_hand_swing_progress: 0.0,
                swing_off_hand: false,
                custom_name: None,
                name_visible: false,
                end_crystal_show_bottom: true,
                tamable_flags: 0,
                cat_variant: None,
                potion_swirls: Vec::new(),
                potion_swirls_ambient: false,
                cloud_radius: 3.0,
                cloud_waiting: false,
                boat_paddles: [false; 2],
                paddle_positions: [0.0; 2],
                minecart_block: None,
                minecart_offset: None,
                minecart_fuel: false,
                minecart_tnt_fuse: -1,
                swap: crate::more_anims::SwapWatch::default(),
                last_sword: None,
                cloud_particle: protocol::particle::ParticleEffect {
                    type_id: protocol::particle::PARTICLE_ENTITY_EFFECT,
                    options: protocol::particle::ParticleOptions::Tinted(0xFFFF_FFFF),
                },
                firework_shooter: None,
                gliding: false,
                sprinting: false,
                gliding_ticks: 0,
                swim_amount: 0.0,
                last_swim_amount: 0.0,
                in_water: false,
                on_fire: false,
                item: None,
                item_count: 1,
                orb_value: 0,
                tnt_fuse: 80,
                tnt_block: None,
                item_seed: 187,
                invisible: false,
                glowing: false,
                living_flags: 0,
                left_handed: false,
                // Ein Display-Entity bekommt seinen Eintrag sofort, mit Vanillas
                // Vorgabewerten und leerem Inhalt: dadurch weiss der Renderer schon vor dem
                // ersten Metadaten-Paket, dass hier KEIN Kasten hingehoert.
                display: display_kind(entity_type).map(|kind| {
                    DisplayInfo::new(match kind {
                        DisplayKind::Text => DisplayContent::Text(TextDisplay::default()),
                        DisplayKind::Item => DisplayContent::Item(None),
                        // BlockDisplayEntity.initDataTracker: Luft, bis der Server etwas
                        // anderes meldet -- und Luft zeichnet nichts.
                        DisplayKind::Block => DisplayContent::Block(0),
                    })
                }),
                armor_stand_flags: 0,
                armor_stand_poses: ARMOR_STAND_DEFAULT_POSES,
            },
        );
        self.anim_log_pos(id, "spawn", None);
    }

    // Set Entity Data anwenden -- bislang nur die Pose, weil nur sie die Trefferbox aendert.
    pub fn set_pose(&mut self, id: i32, pose: u8) {
        if let Some(e) = self.by_id.get_mut(&id) {
            e.pose = pose;
        }
    }

    // Die uebrigen Metadaten: Sichtbarkeit, Namensschild und der Inhalt eines
    // Display-Entities. Der Server schickt nur GEAENDERTE Felder, deshalb wird hier jedes
    // Feld einzeln geprueft und der Rest stehen gelassen.
    //
    // Der Inhalt (Paket-Index 23) ist bei den drei Display-Typen jeweils etwas anderes und
    // wird im Parser nur am Serializer unterschieden (s. packets::read_set_entity_data).
    // Hier faellt die Entscheidung endgueltig: uebernommen wird nur, was zum TYP passt --
    // ein Text-Feld an Index 23 eines ganz anderen Entities landet damit nirgends.
    pub fn apply_metadata(&mut self, id: i32, update: &protocol::packets::EntityDataUpdate) {
        let Some(e) = self.by_id.get_mut(&id) else { return };
        if let Some(flags) = update.flags {
            e.invisible = flags & protocol::packets::ENTITY_FLAG_INVISIBLE != 0;
            e.glowing = flags & protocol::packets::ENTITY_FLAG_GLOWING != 0;
            e.gliding = flags & protocol::packets::ENTITY_FLAG_GLIDING != 0;
            e.sprinting = flags & protocol::packets::ENTITY_FLAG_SPRINTING != 0;
            e.on_fire = flags & protocol::packets::ENTITY_FLAG_ON_FIRE != 0;
        }
        if entity_type_name(e.entity_type) == Some("firework_rocket") {
            if let Some(v) = update.firework_shooter {
                e.firework_shooter = v;
            }
        }
        if is_item_frame(e.entity_type) {
            if let Some(stack) = &update.frame_item {
                e.item = stack.as_ref().map(HeldItem::from_stack);
            }
            if let Some(r) = update.frame_rotation {
                e.frame_rotation = r;
            }
            if let Some(d) = update.hanging_direction.filter(|d| *d < 6) {
                e.frame_facing = d;
            }
        }
        if entity_type_name(e.entity_type) == Some("experience_orb") {
            if let Some(v) = update.orb_value {
                e.orb_value = v;
            }
        }
        if entity_type_name(e.entity_type) == Some("tnt") {
            if let Some(v) = update.orb_value {
                e.tnt_fuse = v;
            }
            if let Some(state) = update.tnt_block_state {
                e.tnt_block = Some(state);
            }
        }
        if item_render_kind(e.entity_type).is_some() {
            if let Some(stack) = &update.item_stack {
                e.item = stack.as_ref().map(HeldItem::from_stack);
                e.item_count = stack.as_ref().map_or(0, |s| s.count);
                e.item_seed = stack.as_ref().map_or(187, |s| s.item_id as i32 + s.damage.unwrap_or(0));
            }
        }
        if let Some(links) = update.main_arm_left {
            e.left_handed = links;
        }
        if let Some(flags) = update.living_flags {
            e.living_flags = flags;
        }
        if update.villager_data.is_some() {
            e.villager_data = update.villager_data;
        }
        if let Some(b) = update.baby {
            if entity_type_name(e.entity_type).is_some_and(can_be_baby) {
                e.baby = b;
            }
        }
        if let Some(name) = &update.custom_name {
            e.custom_name = name.clone().filter(|spans| !spans.is_empty());
        }
        if let Some(visible) = update.name_visible {
            e.name_visible = visible;
        }
        if is_living(e.entity_type) {
            if let Some(farben) = &update.potion_swirls {
                e.potion_swirls = farben.clone();
            }
            if let Some(v) = update.potion_swirls_ambient {
                e.potion_swirls_ambient = v;
            }
        }
        if entity_type_name(e.entity_type) == Some("cat") {
            if let Some(v) = update.tamable_flags {
                e.tamable_flags = v;
            }
            if let Some(v) = update.cat_variant {
                e.cat_variant = Some(v);
            }
        }
        // AbstractArrow.IN_GROUND: ohne das flog ein Pfeil, der beim Laden der Gegend schon
        // steckte, bei uns los -- die Schwerkraft zog ihn aus der Wand, und er fehlte
        // dort.
        if projectile_kind(e.entity_type) == Some(ProjectileKind::Arrow) {
            // Index 8 ist beim Pfeil AbstractArrow.ID_FLAGS -- dasselbe Byte, das bei
            // Lebewesen LIVING_FLAGS heisst, der Parser legt es deshalb dort ab.
            if let Some(f) = update.living_flags {
                e.crit_arrow = f & protocol::packets::ARROW_FLAG_CRIT != 0;
            }
            if let Some(v) = update.arrow_in_ground {
                e.in_ground = v;
                if v {
                    e.vel = (0.0, 0.0, 0.0);
                }
            }
        }
        if entity_type_name(e.entity_type) == Some("interaction") {
            if let Some(v) = update.interaction_width {
                e.interaction_size.0 = f64::from(v);
            }
            if let Some(v) = update.interaction_height {
                e.interaction_size.1 = f64::from(v);
            }
        }
        if entity_type_name(e.entity_type).is_some_and(crate::mob_catalog::is_boat) {
            for (stand, neu) in e.boat_paddles.iter_mut().zip(update.boat_paddles) {
                if let Some(v) = neu {
                    *stand = v;
                }
            }
        }
        // Dieselben Indizes meinen beim Boot die Ruder -- nur bei Loren uebernehmen.
        if entity_type_name(e.entity_type).is_some_and(crate::mob_catalog::is_minecart) {
            if let Some(v) = update.minecart_block {
                e.minecart_block = v;
            }
            if let Some(v) = update.minecart_offset {
                e.minecart_offset = Some(v);
            }
            if let Some(v) = update.minecart_fuel {
                e.minecart_fuel = v;
            }
        }
        if entity_type_name(e.entity_type) == Some("area_effect_cloud") {
            if let Some(v) = update.cloud_radius {
                e.cloud_radius = v;
            }
            if let Some(v) = update.cloud_waiting {
                e.cloud_waiting = v;
            }
            if let Some(p) = &update.cloud_particle {
                e.cloud_particle = p.clone();
            }
        }
        if entity_type_name(e.entity_type) == Some("end_crystal") {
            if let Some(v) = update.end_crystal_show_bottom {
                e.end_crystal_show_bottom = v;
            }
        }
        // Dasselbe Byte tragen bei einem Display-Entity die Billboard-Flags -- deshalb nur
        // bei einem echten Armorstand uebernehmen (s. packets::ARMOR_STAND_INDEX_FLAGS).
        if entity_type_name(e.entity_type) == Some("armor_stand") {
            if let Some(flags) = update.armor_stand_flags {
                e.armor_stand_flags = flags;
            }
            for (pose, neu) in e.armor_stand_poses.iter_mut().zip(update.armor_stand_poses) {
                if let Some(neu) = neu {
                    *pose = neu;
                }
            }
            if anim_debug::anim_debug_an() && update.armor_stand_poses.iter().any(Option::is_some) {
                anim_debug::anim_log(format_args!("armorstand_pose id={id} alter={} posen={:?}", e.age_ticks, update.armor_stand_poses));
            }
        }
        let Some(kind) = display_kind(e.entity_type) else { return };
        let display = e.display.get_or_insert_with(|| {
            DisplayInfo::new(match kind {
                DisplayKind::Text => DisplayContent::Text(TextDisplay::default()),
                DisplayKind::Item => DisplayContent::Item(None),
                DisplayKind::Block => DisplayContent::Block(0),
            })
        });
        // Display.tick: eine Transformations-Aenderung startet bei Dauer != 0 von der GERADE
        // gezeigten Lage -- der Lage beim zuletzt gezeichneten Fortschritt, mit dem ALTEN Start
        // und der ALTEN Dauer (lastProgress). Deshalb zuerst die Ausgangslage, dann Start/Dauer.
        let alter = e.age_ticks;
        let aendert = update.display_translation.is_some()
            || update.display_scale.is_some()
            || update.display_left_rotation.is_some()
            || update.display_right_rotation.is_some();
        let jetzt = aendert.then(|| display.transform_with(display.last_progress.get()));
        if let Some(v) = update.display_interp_duration {
            display.interp_duration = v;
        }
        if let Some(v) = update.display_teleport_duration {
            display.teleport_duration = v;
        }
        // Vanilla uebernimmt START_INTERPOLATION erst im naechsten Display.tick, nach dessen
        // tickCount++: Beginn = (Alter nach dem naechsten Tick) + Verzoegerung. Einen Tick
        // frueher war eine Animation, die jeden Tick neu startet, beim ersten Bild schon am
        // Ziel -- sie sprang von Stufe zu Stufe statt zu gleiten.
        if let Some(v) = update.display_interp_start {
            display.interp_start = alter + 1.0 + v as f32;
        }
        if let Some(jetzt) = jetzt {
            display.interp_from = (display.interp_duration != 0).then_some(jetzt);
        }
        let log = anim_debug::anim_debug_an()
            && (aendert || update.display_interp_start.is_some() || update.display_interp_duration.is_some() || update.display_teleport_duration.is_some());
        let alter_ziel = display.target();
        if let Some(v) = update.display_translation {
            display.translation = v;
        }
        if let Some(v) = update.display_scale {
            display.scale = v;
        }
        if let Some(v) = update.display_left_rotation {
            display.left_rotation = v;
        }
        if let Some(v) = update.display_right_rotation {
            display.right_rotation = v;
        }
        if log {
            let von = display.interp_from.map_or("-".into(), |f| format!("{} L{}", anim_debug::fmt3(f.translation), anim_debug::fmt4(f.left_rotation)));
            anim_debug::anim_log(format_args!(
                "display_transform id={id} typ={} alter={alter} start_delta={:?} dauer={:?} tp_dauer={:?} -> start={} dauer={} letzter_fortschritt={:.3} von={von} altes_ziel={} L{} neues_ziel={} L{} S{} R{}",
                entity_type_name(e.entity_type).unwrap_or("?"),
                update.display_interp_start,
                update.display_interp_duration,
                update.display_teleport_duration,
                display.interp_start,
                display.interp_duration,
                display.last_progress.get(),
                anim_debug::fmt3(alter_ziel.translation),
                anim_debug::fmt4(alter_ziel.left_rotation),
                anim_debug::fmt3(display.translation),
                anim_debug::fmt4(display.left_rotation),
                anim_debug::fmt3(display.scale),
                anim_debug::fmt4(display.right_rotation),
            ));
        }
        if let Some(v) = update.display_billboard {
            display.billboard = v;
        }
        if let Some(v) = update.display_view_range {
            display.view_range = v;
        }
        match (kind, &mut display.content) {
            (DisplayKind::Text, DisplayContent::Text(text)) => {
                if let Some(spans) = &update.display_text {
                    text.spans = spans.clone();
                }
                if let Some(v) = update.text_background {
                    text.background = v;
                }
                if let Some(v) = update.text_opacity {
                    text.opacity = v;
                }
                if let Some(v) = update.text_flags {
                    text.flags = v;
                }
            }
            (DisplayKind::Item, content) => {
                if let Some(m) = update.item_display_mode {
                    display.item_mode = m;
                }
                if let Some(stack) = &update.display_item {
                    *content = DisplayContent::Item(stack.as_ref().map(HeldItem::from_stack));
                }
            }
            (DisplayKind::Block, content) => {
                if let Some(state) = update.display_block_state {
                    *content = DisplayContent::Block(state);
                }
            }
            // Typ und bereits gefuehrter Inhalt passen nicht zusammen -- kann nur durch ein
            // widerspruechliches Paket entstehen; dann lieber nichts aendern.
            _ => {}
        }
    }
}
