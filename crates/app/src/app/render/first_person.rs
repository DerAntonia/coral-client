// Schwung, Blockauswahl, Abbau-Risse, Puppe, Totem, Hand und Item in der Ich-Sicht.

use super::*;

impl App {
    // Vanillas getHandSwingProgress fuer den EIGENEN Spieler: 0, solange nicht geschlagen
    // wird, sonst 0..1 ueber die Schwungdauer. Unser Zaehler laeuft rueckwaerts (s.
    // own_swing_ticks).
    //
    // Denselben Wert brauchen ZWEI Ansichten: der Arm in der Ego-Perspektive und das eigene
    // Modell in der 3. Person. Frueher rechnete ihn nur die erste aus -- in der 3. Person
    // stand die eigene Figur beim Schlagen still.
    pub(crate) fn own_hand_swing(&self, tick_progress: f32) -> f32 {
        if self.own_swing_ticks == 0 {
            return 0.0;
        }
        swing_progress_from_ticks(self.own_swing_ticks, tick_progress, self.own_swing_duration())
    }

    // LivingEntity.getHandSwingDuration fuer uns selbst: am Haupthand-Item (Speere 13..23
    // Ticks, sonst 6). Wechselt man mitten im Schwung das Item, laeuft der Zaehler einfach mit
    // der neuen Dauer weiter -- swing_progress_from_ticks deckelt, es springt also nichts.
    pub(crate) fn own_swing_duration(&self) -> u32 {
        self.own_hand_stack(protocol::packets::HAND_MAIN).map_or(PLAYER_SWING_TICKS, |s| world::item_behavior::swing(s).1)
    }

    pub(crate) fn build_block_selection_vertices(&self) -> Vec<render::HitboxVertex> {
        // Wie Vanilla: mit verstecktem HUD (F1) auch kein Blockrahmen. Als Zuschauer keiner --
        // dort baut und oeffnet man nichts, der Rahmen stuende nur im Bild. Beim Schreiben aufs
        // Schild auch nicht -- der Rahmen laege genau um den Text.
        if self.hud_hidden || self.game_mode == GAME_MODE_SPECTATOR || self.sign_edit.is_some() {
            return Vec::new();
        }
        let CrosshairTarget::Block(pos, _) = self.crosshair_target() else { return Vec::new() };
        // Blockumriss-Modul: beim Abbauen dieses Blocks keinen Rahmen (s.
        // Settings::block_outline_hide_mining).
        if self.settings.module_block_outline && self.settings.block_outline_hide_mining && self.breaking_block && self.breaking_pos == Some(pos) {
            return Vec::new();
        }
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        let mut boxes = world::outline_boxes(state, &self.block_shapes);
        if boxes.is_empty() {
            return Vec::new();
        }
        // Zufallsversatz des Umrisses (Blumen, Bambus, Tropfstein) wie beim Zielen (s. physics).
        let (odx, odz) = world::block_name(state).map_or((0.0, 0.0), |n| world::outline_xz_offset(n, pos.0, pos.2));
        for b in &mut boxes {
            b.min[0] += odx;
            b.max[0] += odx;
            b.min[2] += odz;
            b.max[2] += odz;
        }
        let (color, half_width) = if self.settings.module_block_outline {
            (self.settings.block_outline_rgba(), self.settings.block_outline_width_px as f32 / 2.0)
        } else {
            vanilla_block_outline(self.gpu.as_ref().map_or(1920.0, |g| g.size().0))
        };
        let eye = self.camera.position;
        let mut out = Vec::new();
        for shape in boxes {
            // Augenrelativ in f64 gerechnet und ERST danach auf f32 verkuerzt -- bei
            // sechsstelligen Weltkoordinaten waere die Summe in f32 laengst gerastert und
            // der Umriss wuerde sichtbar zittern (derselbe Grund wie bei ColumnOrigin).
            //
            // Dazu ein kleines Stueck zum Auge gezogen (s. OUTLINE_PULL): auf dem Bildschirm
            // an derselben Stelle, in der Tiefe aber VOR den Flaechen, auf denen die Kante
            // liegt. Vorher lag das Band genau in deren Ebene und rang mit ihnen um den
            // Tiefentest -- vorn kam nur die Haelfte ueber der Luft durch, hinten (wo beide
            // Seiten Flaechen sind) oft gar nichts. Echte Verdecker davor verdecken weiterhin.
            let ecke = |v: [f32; 3]| {
                ((DVec3::new(f64::from(pos.0) + f64::from(v[0]), f64::from(pos.1) + f64::from(v[1]), f64::from(pos.2) + f64::from(v[2])) - eye) * (1.0 - OUTLINE_PULL))
                    .as_vec3()
            };
            let (min, max) = (ecke(shape.min), ecke(shape.max));
            let corner = |mask: [bool; 3]| {
                Vec3::new(if mask[0] { max.x } else { min.x }, if mask[1] { max.y } else { min.y }, if mask[2] { max.z } else { min.z })
            };
            const EDGES: [([bool; 3], [bool; 3]); 12] = [
                ([false, false, false], [true, false, false]), ([false, true, false], [true, true, false]),
                ([false, false, true], [true, false, true]), ([false, true, true], [true, true, true]),
                ([false, false, false], [false, true, false]), ([true, false, false], [true, true, false]),
                ([false, false, true], [false, true, true]), ([true, false, true], [true, true, true]),
                ([false, false, false], [false, false, true]), ([true, false, false], [true, false, true]),
                ([false, true, false], [false, true, true]), ([true, true, false], [true, true, true]),
            ];
            for (a, b) in EDGES {
                push_hitbox_edge(&mut out, corner(a), corner(b), color, half_width);
            }
        }
        out
    }

    // Vanillas Abbau-Animation: die Risstextur (destroy_stage_0..9) ueber der echten Form
    // des Blocks, den wir gerade abbauen -- s. world::mesher::block_breaking_vertices.
    //
    // Die Stufe ist Vanillas MultiPlayerGameMode.getDestroyStage(): `(int)(Fortschritt * 10)`,
    // und BEI FORTSCHRITT 0 noch gar nichts (dort liefert Vanilla -1). Mehr als Stufe 9 gibt
    // es nicht -- bei 1.0 ist der Block ohnehin weg.
    //
    // AUGENRELATIV wie die Geisterbloecke (s. ghost_vertices): Blockposition minus Auge in
    // f64, erst das Ergebnis nach f32.
    pub(crate) fn build_crumbling_vertices(&self) -> Vec<world::mesher::BlockVertex> {
        if !self.breaking_block || self.breaking_progress <= 0.0 {
            return Vec::new();
        }
        let Some(pos) = self.breaking_pos else { return Vec::new() };
        let stage = ((self.breaking_progress * 10.0) as usize).min(assets::DESTROY_STAGE_COUNT - 1);
        let Some(stage_uv) = self.block_uvs.destroy_stage_rect(stage) else { return Vec::new() };
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        if state == world::chunk::STATE_AIR {
            return Vec::new();
        }
        let base = (DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)) - self.camera.position).as_vec3();
        let block_at = |x: i32, y: i32, z: i32| self.world.block_at(x, y, z);
        world::mesher::block_breaking_vertices(base.into(), pos, state, stage_uv, &self.block_shapes, &block_at)
    }

    // Die Spielerfigur im Inventar -- Vanillas "Puppe" (InventoryScreen.drawEntity).
    //
    // Rueckgabe: (Vertices, Sichtmatrix, Projektionsmatrix) oder None, wenn gerade kein
    // Bildschirm mit Figur offen ist. Die Geometrie ist dieselbe wie beim eigenen Modell in
    // der 3. Person -- Skin, Ruestung, beide Haende -- nur mit einer eigenen Pose und einer
    // eigenen, ORTHOGRAFISCHEN Kamera.
    pub(crate) fn build_doll(&self, screen: (f32, f32)) -> Option<(Vec<world::BlockVertex>, glam::Mat4, glam::Mat4)> {
        let layout = if self.creative_shown() {
            self.creative_layout(screen)?
        } else {
            inventory::screen_layout(&self.inventory, screen, self.inventory_scale(screen))
        };
        let rect = layout.doll?;
        if self.open_menu_type() == Some(inventory::MENU_ENCHANTMENT) {
            return self.build_enchant_book(rect, screen);
        }
        let (mx, my) = self.cursor_pos;
        let (cx, cy) = rect.center();

        // Vanillas Blickfolge aus InventoryScreen.drawEntity: i = atan(dx / 40), j = atan(dy / 40)
        // mit dx/dy = Figurmitte minus Zeiger. Koerper 180 + 20i, Blick (und Kopf) 180 + 40i,
        // Neigung -20j.
        //
        // Die 40 sind PIXEL, die atan-Werte sind RADIANT und werden danach mit 20 bzw. 40 als
        // GRAD weiterverrechnet -- das ist keine Schlamperei von mir, sondern steht so im
        // Jar. Ein "sauberes" to_degrees() dazwischen liesse die Figur der Maus rund
        // 57-mal zu weit folgen.
        // Die 40 sind GUI-Pixel; `cx`/`mx` stehen in Bildschirmpixeln, also mit der
        // GUI-Skalierung umgerechnet (pixels_per_meter ist DOLL_SIZE mal genau dieser Skala).
        let gui_scale = rect.pixels_per_meter / inventory::DOLL_SIZE;
        let i = ((cx - mx) / (40.0 * gui_scale)).atan();
        let j = ((cy - my) / (40.0 * gui_scale)).atan();
        // Vanilla setzt bodyYaw = 180 + i*20 und schaut die Figur durch einen gespiegelten
        // Matrix-Stack an (scale(size, size, -size) zusammen mit rotateZ(PI)). Netto ist das
        // dasselbe wie eine zusaetzliche halbe Drehung um die Y-Achse -- diese Kamera spiegelt
        // NICHT (s. doll_camera), also steckt die halbe Drehung hier im Winkel:
        // 180 + i*20 + 180 = i*20.
        let body_yaw = i * 20.0;
        // headYaw - bodyYaw: der Kopf dreht sich doppelt so weit wie der Rumpf, die Pose
        // braucht nur die DIFFERENZ (s. biped_pose_full). Ein RELATIVER Winkel, den die halbe
        // Drehung oben nicht beruehrt.
        let head_rel = i * 20.0;
        let pitch = -j * 20.0;

        let (model, tex) = self.own_player_model_and_texture();
        let (main, off) = self.doll_hand_items();
        let pose = world::entity_model::biped_pose_full(
            head_rel,
            pitch,
            // Die Figur im Inventar steht still: kein Laufschwingen, kein Schlag. Vanilla
            // zeichnet zwar das echte Spieler-Entity, aber mit angehaltenen Animationen --
            // `limbDistance`/`limbAngle` bleiben auf ihrem Stand, und in Ruhe sind sie null.
            0.0,
            0.0,
            world::entity_model::BipedExtras {
                // Die Figur im Inventar sitzt mit, wenn man reitet (Vanilla zeichnet dort das
                // echte Spieler-Entity).
                riding: self.own_ride.is_some(),
                age: self.own_age_ticks,
                item_right: main.is_some(),
                item_left: off.is_some(),
                block_right: blocking_arms(self.own_using_item_right_arm(), (main, off)).0,
                block_left: blocking_arms(self.own_using_item_right_arm(), (main, off)).1,
                spear_right: is_spear_item(main),
                spear_left: is_spear_item(off),
                use_pose: self.use_pose(self.own_using_item_right_arm(), (main, off), true, self.own_use_ticks as f32, false),
                ..Default::default()
            },
        );
        let base = world::entity_transform([0.0, 0.0, 0.0], body_yaw, 1.0, world::entity_model::PLAYER_MODEL_SCALE);

        let mut out = Vec::with_capacity(512);
        world::emit_entity_model_two_sided(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
        self.emit_armor(&self.own_armor(), &self.armor_models, &pose, base, &mut out, None, None);
        self.emit_held_items((main, off), blocking_arms(self.own_using_item_right_arm(), (main, off)), self.own_using_item_right_arm(), (0.0, true), (world::Affine::IDENTITY, world::Affine::IDENTITY), model, &pose, base, None, &mut out);
        let own_cape = self.own_uuid.and_then(|uuid| self.cape_texture(&uuid));
        // Fluegel wie in der 3. Person, in Ruhe angelegt; mit ihnen haengt der Umhang als Bild
        // darauf statt am Rumpf (s. emit_elytra).
        let elytra = self.emit_elytra(&self.own_armor(), own_cape, None, false, base, &mut out);
        if let Some(cape_tex) = own_cape.filter(|_| !elytra) {
            // Der Umhang haengt in Ruhe gerade nach unten -- alle drei Winkel null.
            let cape_pose = world::cape_pose(&pose, 0.0, 0.0, 0.0);
            world::emit_entity_model(&self.cape_model, &cape_pose, base, cape_tex, ENTITY_BRIGHTNESS, &mut out);
        }

        let (view, proj) = doll_camera(rect, screen);
        Some((out, view, proj))
    }

    // Vanillas schwebendes Pop-Totem (InGameOverlayRenderer.renderFloatingItem), 1:1: im
    // Sichtraum mit dem Hand-Sichtfeld von 70 Grad, deshalb im selben Durchgang wie die Hand
    // -- aber in JEDER Perspektive, wie in Vanilla.
    pub(crate) fn emit_floating_totem(&self, out: &mut Vec<world::BlockVertex>) {
        let Some((ticks, off_x, off_y)) = self.floating_totem else { return };
        let Some(totem) = world::item_registry::item_id_by_name("totem_of_undying") else { return };
        let tick_progress = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0);
        let i = (modules::TOTEM_ANIMATION_TICKS - ticks) as f32;
        let f = (i + tick_progress) / modules::TOTEM_ANIMATION_TICKS as f32;
        let k = modules::animation_phase(f);
        let aspect = self.gpu.as_ref().map_or(16.0 / 9.0, |g| g.size().0 / g.size().1.max(1.0));
        let m = off_x * 0.3 * aspect;
        let n = off_y * 0.3;
        let schwung = (k * 2.0).sin().abs();
        use world::entity_model::Affine;
        let pre = Affine::translation([m * schwung, n * schwung, -10.0 + 9.0 * k.sin()])
            .then(Affine::scale([0.8; 3]))
            .then(Affine::rotation_y_deg(900.0 * k.sin().abs()))
            .then(world::rotation_x_deg(6.0 * (f * 8.0).cos()))
            .then(Affine::rotation_axis_angle((6.0 * (f * 8.0).cos()).to_radians(), [0.0, 0.0, 1.0]));
        // Volle Helligkeit wie Vanilla (Licht 15728880).
        self.held_items.emit_fixed(totem, pre, 1.0, out);
    }

    // Hand-Debug: auf welchen Anteil das Bild verkleinert wird (s. Gpu::render_world), None = aus.
    // Nur in der Ego-Ansicht, wo es eine Hand gibt.
    pub(crate) fn hand_debug_scale(&self) -> Option<f32> {
        (self.settings.module_hand_debug && self.perspective.is_first_person() && self.mode == AppMode::World).then_some(0.5)
    }

    pub(crate) fn build_hand_vertices(&self) -> Vec<world::BlockVertex> {
        // GameRenderer.renderItemInHand: nicht als Zuschauer.
        if self.mode != AppMode::World || self.is_dead || self.game_mode == GAME_MODE_SPECTATOR {
            return Vec::new();
        }
        // Versteckter HUD (F1) nimmt wie in Vanilla auch die Hand weg -- das Totem bleibt, es
        // ist eine Meldung, kein HUD. Beim Verbinden ebenso keine Hand.
        if !self.perspective.is_first_person() || self.hud_hidden || self.connecting_view() {
            let mut out = Vec::new();
            self.emit_floating_totem(&mut out);
            return out;
        }
        let tick_progress = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0);
        let swing_progress = self.own_hand_swing(tick_progress);
        let equip_progress = self.own_last_equip_progress + (self.own_equip_progress - self.own_last_equip_progress) * tick_progress;

        // 1. Nachschwingen.
        // Ohne Umbruch auf +-180 Grad wie Vanilla (yBob): sonst kippte die Differenz bei
        // schnellen Drehungen das Vorzeichen, und die Hand sprang auf die andere Seite.
        let render_yaw = self.own_last_render_yaw + (self.own_render_yaw - self.own_last_render_yaw) * tick_progress;
        let render_pitch = self.own_last_render_pitch + (self.own_render_pitch - self.own_last_render_pitch) * tick_progress;
        let sway = world::rotation_x_deg((self.look_pitch_deg() - render_pitch) * 0.1)
            .then(world::Affine::rotation_y_deg((self.camera.yaw - render_yaw) * 0.1));
        // Hand-Debug: das ganze Bild der Hand auf dem Schirm nach links oben geschoben, damit
        // auch der Arm unter dem Bildrand zu sehen ist. Eine Scherung statt einer Verschiebung:
        // x und y wandern mit der Tiefe mit, so rueckt jeder Punkt auf dem Schirm gleich weit,
        // und der Blickwinkel auf Hand und Item bleibt derselbe.
        let schirm = match self.hand_debug_scale() {
            // Dieselbe Abbildung wie fuers Bild (s. Gpu::render_world): auf `s` verkleinert, die
            // linke obere Ecke bleibt stehen -- im Bild x' = s*x + (s-1), y' = s*y + (1-s). Hier
            // als Scherung im Sichtraum (x und y wandern mit der Tiefe), gemessen in der festen
            // Hand-Projektion; der Blickwinkel auf Hand und Item bleibt derselbe.
            Some(s) => {
                let halb_y = (render::block_renderer::HAND_FOV_DEGREES / 2.0).to_radians().tan();
                let halb_x = halb_y * self.gpu.as_ref().map_or(16.0 / 9.0, |g| g.size().0 / g.size().1.max(1.0));
                world::Affine { m: [[s, 0.0, (1.0 - s) * halb_x], [0.0, s, -(1.0 - s) * halb_y], [0.0, 0.0, 1.0]], t: [0.0; 3] }
            }
            None => world::Affine::IDENTITY,
        };
        // Licht wie Vanilla (GameRenderer.renderItemInHand): vor der Hand-Kette steht die
        // umgekehrte Kameradrehung, die Flaechen werden also in WELTrichtung beleuchtet
        // (Lighting.Entry.LEVEL) -- eine Handflaeche nach Norden ist so hell wie jede Nordflaeche
        // und aendert sich beim Umdrehen. Gebaut wird darum kameraausgerichtet in Weltlage, am
        // Ende kommen die Positionen per `zurueck` wieder in den Sichtraum.
        let blick = glam::Mat4::look_to_rh(glam::Vec3::ZERO, self.camera.forward(), glam::Vec3::Y);
        let drehung = |m: glam::Mat4| world::Affine { m: std::array::from_fn(|i| std::array::from_fn(|j| m.col(j)[i])), t: [0.0; 3] };
        let zurueck = schirm.then(drehung(blick));
        // Auch der Glint (eigener Puffer, s. push_glint) entsteht in Weltlage.
        let glint_anfang = self.glint_out.borrow().len();
        let sway = drehung(blick.transpose()).then(sway);

        let mut out = Vec::with_capacity(128);

        // Jonglieren (s. world::juggle): beide Arme und die fliegenden Items statt der Haende.
        if let Some((trick, t)) = self.juggle_progress(tick_progress) {
            self.emit_juggle_first_person(trick, t, sway, &mut out);
            for v in out.iter_mut().chain(self.glint_out.borrow_mut()[glint_anfang..].iter_mut()) {
                v.position = zurueck.apply(v.position);
            }
            self.emit_floating_totem(&mut out);
            return out;
        }

        // ENTWEDER Arm ODER Item -- nicht beides. Vanilla zeichnet den nackten Arm nur bei
        // LEERER Hand (HeldItemRenderer.renderFirstPersonItem ruft dann renderArmHoldingItem,
        // sonst geht es in den Item-Zweig). Wer ein Schwert
        // haelt, sieht in Vanilla das Schwert und keinen Arm daneben.
        //
        // Nur die Haupthand: fuer die Nebenhand zeichnet Vanilla einen zweiten, gespiegelten
        // Arm -- den gibt es hier noch nicht (es wird genau EIN Arm gebaut).
        // Karten in der Hand (ItemInHandRenderer): mit leerer Nebenhand haelt die Haupthand sie mit
        // beiden Armen vor sich, sonst haengt sie einhaendig neben dem Arm.
        let karte = |hand| self.own_hand_stack(hand).and_then(|s| s.map_id);
        let neben_karte = karte(protocol::packets::HAND_OFF);
        let neben_leer = self.own_hand_items().1.is_none();
        let sehne = self.string_hand();
        match (self.own_hand_items().0, karte(protocol::packets::HAND_MAIN)) {
            _ if sehne.as_ref().is_some_and(|(h, _)| *h == protocol::packets::HAND_MAIN) => {
                if let Some((h, name)) = &sehne {
                    self.emit_string_arm(*h, name, sway, &mut out);
                }
            }
            (Some(_), Some(id)) if neben_leer => self.emit_two_handed_map(id, sway, equip_progress, swing_progress, &mut out),
            (Some(_), Some(id)) => self.emit_one_handed_map(id, sway, true, equip_progress, swing_progress, &mut out),
            (None, _) => {
                // 2. + 3.
                let ansehen = self.inspect_pose(protocol::packets::HAND_MAIN, tick_progress);
                let base = sway.then(ansehen).then(world::first_person_arm_transform(true, equip_progress, swing_progress));
                let (model, tex) = self.own_hand_model_and_texture();
                let pose = world::first_person_arm_pose(true);
                world::emit_entity_model_two_sided(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
            }
            (Some(main), None) => {
                // Das Item haengt NICHT am Armmodell: Vanilla setzt es mit einer eigenen
                // Kette in denselben Sichtraum (s. world::first_person_item_transform).
                let benutzt = self.own_using_item_right_arm() == Some(true);
                let f = self.hand_use_ticks(tick_progress);
                let pose = self.first_person_item_pose(protocol::packets::HAND_MAIN, true, benutzt, f, equip_progress, swing_progress);
                // Anders als Vanilla haelt eine Hand das Item (s. emit_holding_arm). Die Hand folgt
                // der Pose OHNE die eigene Item-Verschiebung und -Drehung, die bewegen nur das Item.
                let ruhe = self.first_person_item_pose(protocol::packets::HAND_MAIN, true, benutzt, f, 0.0, 0.0);
                let ansehen = self.inspect_pose_item(main, protocol::packets::HAND_MAIN, ruhe, tick_progress);
                let anteil = self.tuning_share(protocol::packets::HAND_MAIN, benutzt, f);
                let item_base = sway.then(ansehen).then(self.item_shift(main, true, anteil)).then(pose).then(self.item_turn(main, protocol::packets::HAND_MAIN, ruhe, anteil));
                self.emit_holding_arm(main, sway, ansehen, pose, ruhe, protocol::packets::HAND_MAIN, tick_progress, anteil, &mut out);
                // Blockt die HAUPThand gerade? (Rechtshaendig gedacht wie ueberall hier.)
                let (block_main, _) = blocking_arms(self.own_using_item_right_arm(), self.own_hand_items());
                self.emit_first_person_item(main, item_base, true, block_main, protocol::packets::HAND_MAIN, benutzt, f, &mut out);
            }
        }

        // DIE NEBENHAND. Vanilla zeichnet sie als eigenen, gespiegelten Durchgang
        // (HeldItemRenderer.renderFirstPersonItem mit Arm.LEFT) -- bisher fehlte sie hier ganz,
        // und damit auch jedes Schild in der Ego-Perspektive. Schilde liegen praktisch immer in der Nebenhand.
        //
        // BEWUSSTE VEREINFACHUNG: Anlege- und Schlagfortschritt stehen auf null. Beide werden
        // in diesem Client nur fuer die HAUPThand gefuehrt (ein Zaehler, s. own_equip_progress
        // / own_hand_swing); den der Haupthand einfach mitzubenutzen waere schlechter als
        // keiner -- das Schild wippte dann bei jedem Hotbar-Wechsel und bei jedem Schlag mit,
        // obwohl sich in der Nebenhand gar nichts geaendert hat.
        if let Some((h @ protocol::packets::HAND_OFF, name)) = &sehne {
            self.emit_string_arm(*h, name, sway, &mut out);
        } else if let Some(id) = neben_karte {
            self.emit_one_handed_map(id, sway, false, 0.0, 0.0, &mut out);
        } else if let Some(off) = self.own_hand_items().1 {
            let benutzt = self.own_using_item_right_arm() == Some(false);
            let f = self.hand_use_ticks(tick_progress);
            let ansehen = self.inspect_pose(protocol::packets::HAND_OFF, tick_progress);
            let pose = self.first_person_item_pose(protocol::packets::HAND_OFF, false, benutzt, f, 0.0, 0.0);
            let anteil = self.tuning_share(protocol::packets::HAND_OFF, benutzt, f);
            let item_base = sway.then(ansehen).then(self.item_shift(off, false, anteil)).then(pose).then(self.item_turn(off, protocol::packets::HAND_OFF, pose, anteil));
            self.emit_holding_arm(off, sway, ansehen, pose, pose, protocol::packets::HAND_OFF, tick_progress, anteil, &mut out);
            let (_, block_off) = blocking_arms(self.own_using_item_right_arm(), self.own_hand_items());
            self.emit_first_person_item(off, item_base, false, block_off, protocol::packets::HAND_OFF, benutzt, f, &mut out);
        }
        for v in out.iter_mut().chain(self.glint_out.borrow_mut()[glint_anfang..].iter_mut()) {
            v.position = zurueck.apply(v.position);
        }
        self.emit_floating_totem(&mut out);
        out
    }

    // ItemInHandRenderer.renderTwoHandedMap: die Karte mittig vor sich, nach dem Blick nach unten
    // gekippt (calculateMapTilt), beide Arme darunter.
    pub(crate) fn emit_two_handed_map(&self, id: i32, sway: world::Affine, equip: f32, swing: f32, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_x_deg, Affine};
        let pi = std::f32::consts::PI;
        let wurzel = swing.max(0.0).sqrt();
        let (a, b) = (-0.2 * (swing * pi).sin(), -0.4 * (wurzel * pi).sin());
        let neigung = map_tilt(self.look_pitch_deg());
        let basis = sway
            .then(Affine::translation([0.0, -a / 2.0, b]))
            .then(Affine::translation([0.0, 0.04 + equip * -1.2 + neigung * -0.5, -0.72]))
            .then(rotation_x_deg(neigung * -85.0));
        if !self.own_invisible {
            let arme = basis.then(Affine::rotation_y_deg(90.0));
            self.emit_map_hand(arme, true, out);
            self.emit_map_hand(arme, false, out);
        }
        let karte = basis.then(rotation_x_deg((wurzel * pi).sin() * 20.0)).then(Affine::scale([2.0; 3]));
        self.emit_map_in_hand(id, karte, out);
    }

    // ItemInHandRenderer.renderOneHandedMap: der Arm leicht nach aussen gerollt, die Karte daneben.
    pub(crate) fn emit_one_handed_map(&self, id: i32, sway: world::Affine, rechts: bool, equip: f32, swing: f32, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_x_deg, rotation_z_deg, Affine};
        let pi = std::f32::consts::PI;
        let s = if rechts { 1.0 } else { -1.0 };
        let basis = sway.then(Affine::translation([s * 0.125, -0.125, 0.0]));
        if !self.own_invisible {
            let arm = basis.then(rotation_z_deg(s * 10.0)).then(world::first_person_arm_transform(rechts, equip, swing));
            let (_, tex) = self.own_hand_model_and_texture();
            let modell = world::player_arm_model(self.own_slim(), rechts);
            world::emit_entity_model_two_sided(&modell, &world::first_person_arm_pose(rechts), arm, tex, ENTITY_BRIGHTNESS, out);
        }
        let wurzel = swing.max(0.0).sqrt();
        let f10 = (wurzel * pi).sin();
        let karte = basis
            .then(Affine::translation([s * 0.51, -0.08 + equip * -1.2, -0.75]))
            .then(Affine::translation([s * -0.5 * f10, 0.4 * (wurzel * 2.0 * pi).sin() - 0.3 * f10, -0.3 * (swing * pi).sin()]))
            .then(rotation_x_deg(f10 * -45.0))
            .then(Affine::rotation_y_deg(s * f10 * -30.0));
        self.emit_map_in_hand(id, karte, out);
    }

    // ItemInHandRenderer.renderMapHand: ein Arm unter der beidhaendig gehaltenen Karte.
    pub(crate) fn emit_map_hand(&self, basis: world::Affine, rechts: bool, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_x_deg, rotation_z_deg, Affine};
        let s = if rechts { 1.0 } else { -1.0 };
        let m = basis
            .then(Affine::rotation_y_deg(92.0))
            .then(rotation_x_deg(45.0))
            .then(rotation_z_deg(s * -41.0))
            .then(Affine::translation([s * 0.3, -1.1, 0.45]));
        let (_, tex) = self.own_hand_model_and_texture();
        let modell = world::player_arm_model(self.own_slim(), rechts);
        world::emit_entity_model_two_sided(&modell, &world::first_person_arm_pose(rechts), m, tex, ENTITY_BRIGHTNESS, out);
    }

    // ItemInHandRenderer.renderMap: Hintergrund (mit Daten das Schachbrett) und das Kartenbild
    // samt Markierungen, 0.38 gross und um die Mitte gelegt.
    pub(crate) fn emit_map_in_hand(&self, id: i32, basis: world::Affine, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_z_deg, Affine};
        let m = basis
            .then(Affine::rotation_y_deg(180.0))
            .then(rotation_z_deg(180.0))
            .then(Affine::scale([0.38; 3]))
            .then(Affine::translation([-0.5, -0.5, 0.0]))
            .then(Affine::scale([1.0 / 128.0; 3]));
        let bild = self.map_rect(id, false);
        let grund = if self.maps.get(id).is_some() { "map/map_background_checkerboard" } else { "map/map_background" };
        if let Some([u0, v0, u1, v1]) = self.entity_atlas.rect(grund) {
            let ecke = |x: f32, y: f32, u: f32, v: f32| world::BlockVertex { position: m.apply([x, y, 0.0]), color: [ENTITY_BRIGHTNESS; 3], uv: [u, v] };
            let (a, b, c, d) = (ecke(-7.0, 135.0, u0, v1), ecke(135.0, 135.0, u1, v1), ecke(135.0, -7.0, u1, v0), ecke(-7.0, -7.0, u0, v0));
            out.extend([a, b, c, a, c, d]);
        }
        if let Some(rect) = bild {
            push_map_quad(m, rect, ENTITY_BRIGHTNESS, out);
        }
    }

    // Ein Item in der eigenen Hand (Ego-Ansicht). Trennt nur den Sonderfall Schild ab, das
    // ueber ein Entity-Modell statt sein Item-Modell gezeichnet wird (s. emit_shield).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_first_person_item(
        &self,
        item: world::HeldItem,
        base: world::entity_model::Affine,
        right_hand: bool,
        blocking: bool,
        hand: i32,
        benutzt: bool,
        use_ticks: f32,
        out: &mut Vec<world::BlockVertex>,
    ) {
        if world::item_registry::item_name(item.item_id) == Some("shield") {
            self.emit_shield(base, right_hand, true, blocking, item.enchanted, item.banner, out);
            return;
        }
        // Gespannter Bogen / geladene Armbrust: ein ANDERES Modell, kein anderer Winkel
        // (s. hand_item_model). Solange es noch nicht geladen ist, bleibt es beim Grundmodell
        // -- besser ein Bild zu spaet als ein Frame lang eine leere Hand.
        if let Some(modell) = self.hand_model_of(hand, benutzt, use_ticks) {
            let modell = modell.as_str();
            if self.held_items.has_model(modell) {
                let start = out.len();
                self.held_items.emit_first_person_model(modell, base, right_hand, ENTITY_BRIGHTNESS, out);
                if item.enchanted {
                    self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                        self.held_items.emit_first_person_model_local(modell, base, right_hand, l);
                    });
                }
                return;
            }
        }
        let start = out.len();
        self.held_items.set_skull(self.held_skull_texture(&item));
        self.held_items.emit_first_person(item.model_id, item.potion_color, base, right_hand, ENTITY_BRIGHTNESS, out);
        if item.enchanted {
            self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                self.held_items.emit_first_person_local(item.model_id, item.potion_color, base, right_hand, l);
            });
        }
    }

    // Das Zustandsmodell des Items in dieser Hand, falls es eines gibt (s. hand_item_model).
    // Welches Modell die eigene Hand gerade zeigt, wenn es nicht das gewoehnliche des Items ist:
    // die Item-Definition mit Benutzung (Spannstufen), Ladung und Components ausgewertet und mit
    // dem Ergebnis eines schlichten Stapels desselben Items verglichen. None = das gewoehnliche
    // (dafuer hat HeldItems den vollstaendigen Weg mit Traenken, Koepfen und Sondermodellen).
    pub(crate) fn hand_model_of(&self, hand: i32, benutzt: bool, use_ticks: f32) -> Option<String> {
        let stack = self.own_hand_stack(hand)?;
        let _name = world::item_registry::item_name(stack.item_id)?;
        let kontext = if hand == protocol::packets::HAND_MAIN { "firstperson_righthand" } else { "firstperson_lefthand" };
        let benutzung = benutzt.then(|| item_model_input::UseState {
            ticks: use_ticks,
            duration: world::item_behavior::consume_ticks(stack).unwrap_or(72_000) as f32,
            pull_ticks: self.enchantments.crossbow_pull_ticks(stack) as f32,
        });
        self.model_override(stack, kontext, benutzung)
    }

    // Das Modell, das die Item-Definition fuer diesen Stapel an `kontext` waehlt -- None, wenn
    // es dasselbe ist wie bei einem schlichten Stapel desselben Items (dann bleibt es beim
    // vollstaendigen Weg in HeldItems mit Traenken, Koepfen und Sondermodellen).
    pub(crate) fn model_override(&self, stack: &protocol::item::ItemStack, kontext: &str, benutzung: Option<item_model_input::UseState>) -> Option<String> {
        let name = world::item_registry::item_name(stack.item_id)?;
        let def = self.item_icons.definition(&model_definition_id(stack, name))?;
        let mut input = self.item_model_input(stack, kontext);
        input.using = benutzung;
        let zweige = def.evaluate(&input);
        let mut schlicht = protocol::item::ItemStack::new(stack.item_id, 1);
        schlicht.item_model = stack.item_model.clone();
        if zweige == def.evaluate(&self.item_model_input(&schlicht, kontext)) {
            return None;
        }
        let blatt = def.leaves.get(*zweige.first()? as usize)?;
        (blatt.special.is_none() || crate::held_item::special_model(&blatt.model).is_some()).then(|| blatt.model.clone())
    }

    // Dasselbe fuer ein Item in der Hand eines Entities (3. Person). `benutzt`: diese Hand
    // benutzt das Item gerade -- nur beim Dreizack weitergegeben (items/trident.json waehlt
    // damit die Wurfhaltung trident_throwing). Bogen und Armbrust fragen dazu die Spanndauer,
    // die der Client bei Fremden nicht kennt; sie bleiben beim Ruhemodell.
    pub(crate) fn held_model_override(&self, item: &world::HeldItem, right: bool, benutzt: bool) -> Option<String> {
        let stack = world::item_components::resolve(item.stack)?;
        let wurf = benutzt && world::entity::held_use_animation(item) == world::item_behavior::USE_ANIM_TRIDENT;
        let benutzung = wurf.then_some(item_model_input::UseState { ticks: 0.0, duration: 72_000.0, pull_ticks: 0.0 });
        self.model_override(&stack, if right { "thirdperson_righthand" } else { "thirdperson_lefthand" }, benutzung)
    }

    // Wie das Item dieser Hand in der Ego-Ansicht liegt. Ohne laufende Benutzung ist das
    // Vanillas gewoehnliche Kette (Anlegen + Schwung); Bogen und Armbrust haben eigene
    // Haltungen (HeldItemRenderer.renderFirstPersonItem).
    //
    // Vanilla behandelt die
    // Armbrust in einem EIGENEN Zweig -- beim Laden liegt sie quer vor der Kamera, geladen
    // haelt die Haupthand sie ein Stueck nach aussen gedreht, damit der Pfeil sichtbar ist.
    pub(crate) fn first_person_item_pose(
        &self,
        hand: i32,
        right_hand: bool,
        benutzt: bool,
        use_ticks: f32,
        equip_progress: f32,
        swing_progress: f32,
    ) -> world::entity_model::Affine {
        let normal = || world::entity_model::first_person_item_transform(right_hand, equip_progress, swing_progress);
        let Some(stack) = self.own_hand_stack(hand) else { return normal() };
        let Some(name) = world::item_registry::item_name(stack.item_id) else { return normal() };
        if benutzt {
            if let Some(dauer) = world::item_behavior::consume_ticks(stack) {
                // Vanilla rechnet mit der VERBLEIBENDEN Zeit (getItemUseTimeLeft - Bild + 1),
                // hier zaehlt use_ticks aufwaerts.
                let dauer = dauer as f32;
                let rest = dauer - use_ticks + 1.0;
                return world::entity_model::first_person_eat_pose(right_hand, equip_progress, rest, dauer);
            }
        }
        // Speer: Rechtsklick halten = Ansturm (UseAction.SPEAR), sonst sticht er statt zu
        // schwingen (SwingAnimationType.STAB), s. world::first_person_stab_transform.
        if let Some(kinetik) = world::item_behavior::kinetic(stack) {
            if benutzt {
                return world::entity_model::first_person_spear_charge(right_hand, use_ticks, kinetik);
            }
            return world::entity_model::first_person_stab_transform(right_hand, equip_progress, swing_progress);
        }
        match name {
            "crossbow" => {
                let geladen = stack.is_charged();
                if benutzt && !geladen {
                    let pull = crossbow_pull_progress(use_ticks, self.enchantments.crossbow_pull_ticks(stack) as f32).min(1.0);
                    return world::entity_model::first_person_crossbow_pull_pose(right_hand, equip_progress, use_ticks, pull);
                }
                // Geladen und nicht am Schlagen: Vanilla nur fuer die HAUPThand.
                let basis = normal();
                if geladen && swing_progress < 0.001 && hand == protocol::packets::HAND_MAIN {
                    basis.then(world::entity_model::first_person_crossbow_charged_offset(right_hand))
                } else {
                    basis
                }
            }
            "trident" if benutzt => world::entity_model::first_person_trident_pose(right_hand, equip_progress, (use_ticks / 10.0).min(1.0)),
            "bow" if benutzt => {
                // Vanillas Spannkurve: g = (g*g + 2g) / 3 auf f/20.
                let t = use_ticks / 20.0;
                let pull = ((t * t + t * 2.0) / 3.0).min(1.0);
                // Modified View Model: wie in echt vor die Augen gezogen -- die Halte-Lage bleibt
                // (s. Hand-Debug-Werte des Bogens je Spannstufe), nur zur Bildmitte verschoben.
                // Ohne Vanillas Spann-Dehnung: die eigene Drehung kippt deren Achse ins Bild, der
                // Bogen wuerde sichtbar groesser.
                if self.view_model_on() {
                    let u = (use_ticks / world::hold::BOW_DRAW_TICKS).clamp(0.0, 1.0);
                    let w = u * u * (3.0 - 2.0 * u);
                    let s = if right_hand { 1.0 } else { -1.0 };
                    let [dx, dy, dz] = world::hold::BOW_DRAW_TO;
                    return world::Affine::translation([s * dx * w, dy * w, dz * w]).then(normal());
                }
                world::entity_model::first_person_bow_pose(right_hand, equip_progress, use_ticks, pull)
            }
            _ => normal(),
        }
    }

    // Arm-Modell + Skin-Rechteck, nach derselben Regel wie das ganze Spielermodell.
    // Hat der eigene Skin schmale Arme?
    pub(crate) fn own_slim(&self) -> bool {
        let uuid = self.own_uuid.unwrap_or([0; 16]);
        self.skins.get(&uuid).is_some_and(|s| s.slim)
    }

    pub(crate) fn own_hand_model_and_texture(&self) -> (&world::Model, [f32; 4]) {
        const KEIN_SPIELER: [u8; 16] = [0; 16];
        let uuid = self.own_uuid.unwrap_or(KEIN_SPIELER);
        let skin = self.skins.get(&uuid).copied();
        let slim = skin.is_some_and(|s| s.slim);
        let model = if slim { &self.hand_model_slim } else { &self.hand_model_wide };
        let tex = skin
            .and_then(|s| s.slot)
            .and_then(|slot| self.entity_atlas.skin_slot_rect(slot))
            .or_else(|| self.entity_atlas.rect(assets::MISSING_SKIN))
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        (model, tex)
    }

    // Modell + Textur fuer den eigenen Spieler (3.-Person-Ansicht). Dieselbe Suche wie bei
    // Fremden, nur mit der eigenen UUID. Solange die noch nicht da ist (Login-Success kommt
    // vor dem Play-State) oder der Server keine Textur-Property schickt (Offline-Mode),
    // steht schwarz-lila da (s. player_model_and_texture).
    pub(crate) fn own_player_model_and_texture(&self) -> (&world::Model, [f32; 4]) {
        // Die Null-UUID kann kein Spieler sein, ist also garantiert ein Fehlschlag in
        // `skins` -- damit ist "UUID noch unbekannt" derselbe Pfad wie "kein Skin geladen".
        const KEIN_SPIELER: [u8; 16] = [0; 16];
        self.player_model_and_texture(self.own_uuid.as_ref().unwrap_or(&KEIN_SPIELER))
    }

    // Atlas-Rechteck des Umhangs eines Spielers, oder None wenn er keinen hat (oder er noch
    // nicht geladen ist). Kein Standard-Umhang -- ohne Cape-Property gibt es keinen, und
    // Vanilla zeichnet dann auch keinen.
    pub(crate) fn cape_texture(&self, uuid: &[u8; 16]) -> Option<[f32; 4]> {
        let slot = self.skins.get(uuid)?.cape_slot?;
        self.entity_atlas.cape_slot_rect(slot)
    }

    // Modell + Atlas-Rechteck fuer einen fremden Spieler: sein eigener Skin-Platz, wenn
    // geladen, sonst schwarz-lila -- bewusst kein Standard-Skin, der saehe aus wie ein echter.
    // Das MODELL (slim/classic) haengt an der Textur-Property und gilt auch ohne geladenes
    // Bild; ohne Property classic.
    pub(crate) fn player_model_and_texture(&self, uuid: &[u8; 16]) -> (&world::Model, [f32; 4]) {
        let skin = self.skins.get(uuid).copied();
        let slim = skin.is_some_and(|s| s.slim);
        let model = if slim { &self.player_slim } else { &self.player_wide };
        let tex = skin
            .and_then(|s| s.slot)
            .and_then(|slot| self.entity_atlas.skin_slot_rect(slot))
            .or_else(|| self.entity_atlas.rect(assets::MISSING_SKIN))
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        (model, tex)
    }
}
