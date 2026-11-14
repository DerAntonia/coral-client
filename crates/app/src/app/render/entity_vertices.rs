// Pack-Shader, Leuchtfarben und der Aufbau aller Entity-Vertices.

use super::*;

impl App {
    // Core-Shader, die ein Resourcepack ersetzt, uebersetzen und einsetzen (s. render::
    // vanilla_shader). Ohne Ersatz bleibt alles beim eigenen Zeichenweg; scheitert die
    // Uebersetzung, steht der Grund im Log und es bleibt ebenso.
    pub(crate) fn apply_pack_shaders(&mut self) {
        const ITEM: &str = "minecraft/shaders/core/rendertype_item_entity_translucent_cull";
        let ersetzt = self.installation.pack_provides(&format!("{ITEM}.vsh")) || self.installation.pack_provides(&format!("{ITEM}.fsh"));
        let mut meldung = None;
        let program = if ersetzt {
            let installation = &mut self.installation;
            let mut load = |pfad: &str| installation.load_bytes(pfad).ok().and_then(|b| String::from_utf8(b).ok());
            match render::vanilla_shader::compile(ITEM, &mut load) {
                Ok(p) => Some(p),
                Err(e) => {
                    meldung = Some(format!("Pack-Shader {ITEM} nicht nutzbar: {e}"));
                    None
                }
            }
        } else {
            None
        };
        if let Some(gpu) = self.gpu.as_mut() {
            let aktiv = program.is_some();
            match gpu.block_renderer.vanilla.set_item_program(&gpu.device, program) {
                Err(e) => meldung = Some(format!("Pack-Shader {ITEM} nicht nutzbar: {e}")),
                Ok(()) if aktiv => meldung = Some(format!("Pack-Shader aktiv: {ITEM}")),
                Ok(()) => {}
            }
        }
        if let Some(m) = meldung {
            eprintln!("[shader] {m}");
            self.push_log(m);
        }
        self.apply_border_shader();
    }

    // Die Weltgrenze laeuft immer ueber Vanillas Programm rendertype_world_border: aus dem
    // Pack, wenn eins es ersetzt, sonst Vanillas eigenes aus dem Jar -- so sehen Grenzen aus
    // Packs genauso aus wie in Vanilla. Dazu die Kraftfeld-Textur, ebenfalls aus dem Pack.
    pub(crate) fn apply_border_shader(&mut self) {
        const BORDER: &str = "minecraft/shaders/core/rendertype_world_border";
        let installation = &mut self.installation;
        let mut load = |pfad: &str| installation.load_bytes(pfad).ok().and_then(|b| String::from_utf8(b).ok());
        let program = render::vanilla_shader::compile(BORDER, &mut load);
        let textur = self.installation.load_texture("minecraft/textures/misc/forcefield.png");
        let ersetzt = self.installation.pack_provides(&format!("{BORDER}.vsh")) || self.installation.pack_provides(&format!("{BORDER}.fsh"));
        let Some(gpu) = self.gpu.as_mut() else { return };
        let ergebnis = match (program, textur) {
            (Ok(p), Ok(tex)) => gpu.block_renderer.vanilla.set_border_program(&gpu.device, &gpu.queue, Some(p), (tex.width, tex.height, &tex.rgba)),
            (Err(e), _) => Err(e),
            (_, Err(e)) => Err(format!("forcefield.png: {e}")),
        };
        let meldung = match ergebnis {
            Err(e) => Some(format!("Weltgrenze ohne Shader {BORDER}: {e}")),
            Ok(()) if ersetzt => Some(format!("Pack-Shader aktiv: {BORDER}")),
            Ok(()) => None,
        };
        if let Some(m) = meldung {
            eprintln!("[shader] {m}");
            self.push_log(m);
        }
    }

    // Alle Entity-Vertices AUGENRELATIV: sie gehen mit Null-Versatz in den Zeichenaufruf
    // (s. render::BlockRenderer::render), rechnen die Kameraposition also selbst heraus.
    // Die Subtraktion passiert in f64, VOR der f32-Wandlung -- bei absoluten Weltkoordinaten
    // waere die Entity-Position auf einem Server wie DonutSMP schon auf 0.015625 gerastert,
    // bevor sie ueberhaupt im Puffer landet (derselbe Grund wie bei ColumnOrigin).
    // Rueckgabe ist ein PAAR gleich langer Listen: die Vertices und je Vertex die
    // Schadens-Faerbung (rgb + Anteil, s. block.wgsl::vs_entity). Getrennt statt als
    // erweiterter Vertex-Typ, damit die Chunk-Vertices (Millionen davon) nicht 16 Byte
    // breiter werden -- der Tint betrifft nur Entities.
    //
    // Die Faerbung wird nicht je Vertex berechnet, sondern je Entity EINMAL und danach mit
    // `resize` auf alle seither entstandenen Vertices ausgedehnt. Das haelt die Zuordnung
    // auch dann richtig, wenn ein Entity aus mehreren Zeichenschritten besteht (Modell +
    // Handgegenstaende + Umhang).
    // Der dritte Wert: wie viele Vertices am ENDE durchscheinend gezeichnet werden (s.
    // BlockRenderer::set_entity_vertices) -- die Windkugeln.
    // Der zweite Wert daneben: im Shader-Pfad je Vertex das Licht (s. light_near), sonst leer.
    // Die Farbe des Leucht-Umrisses (Entity.getTeamColor): die Farbe des Teams, in dem der
    // Eintrag steht (Spieler mit ihrem Namen, alle anderen mit ihrer UUID), sonst weiss.
    pub(crate) fn glow_color(&self, eintrag: Option<&String>) -> [f32; 4] {
        let farbe = eintrag
            .and_then(|n| self.sb_entry_team.get(n))
            .and_then(|t| self.sb_teams.get(t))
            .and_then(|t| protocol::nbt::formatting_color(t.color))
            .unwrap_or(0xFF_FF_FF);
        let kanal = |s: u32| ((farbe >> s) & 0xFF) as f32 / 255.0;
        [kanal(16), kanal(8), kanal(0), 1.0]
    }

    pub(crate) fn entity_glow_color(&self, e: &world::Entity) -> [f32; 4] {
        let eintrag = if world::is_player(e.entity_type) { self.player_names.get(&e.uuid).cloned() } else { Some(uuid_hyphenated(&e.uuid)) };
        self.glow_color(eintrag.as_ref())
    }

    // Rueckgabe zusaetzlich zu Vertices, Faerbung, Licht und Zahl der durchscheinenden: die
    // Silhouetten leuchtender Entities fuer den Umriss (Vertices und je Vertex die Farbe, s.
    // render::entity_outline). Unsichtbare, die leuchten, stehen NUR dort -- in Vanilla sieht
    // man von ihnen den Umriss, aber keinen Koerper.
    // Die aufgeschobenen Modelle (s. defer_model) in ihre reservierten Abschnitte, verteilt auf
    // den Frame-Pool. Wenige auf einmal lohnen das Aufwecken der Worker nicht.
    pub(crate) fn run_model_jobs(&self, jobs: &[ModelJob<'_>], out: &mut [world::BlockVertex]) {
        const PARALLEL_AB: usize = 4;
        let fuellen = |j: &ModelJob<'_>, ziel: &mut [world::BlockVertex]| {
            world::entity_model::emit_into(j.model, &j.pose, j.base, j.tex, j.color, j.two_sided, ziel);
        };
        // Die Abschnitte liegen nacheinander und ueberlappen nicht (defer_model haengt an).
        let mut abschnitte: Vec<std::sync::Mutex<&mut [world::BlockVertex]>> = Vec::with_capacity(jobs.len());
        let mut rest = out;
        let mut bis = 0;
        for j in jobs {
            debug_assert!(j.start >= bis, "ModelJob-Abschnitte ueberlappen");
            let (_, r) = std::mem::take(&mut rest).split_at_mut(j.start - bis);
            let (eigener, r) = r.split_at_mut(j.len);
            rest = r;
            bis = j.start + j.len;
            abschnitte.push(std::sync::Mutex::new(eigener));
        }
        if jobs.len() < PARALLEL_AB {
            for (j, a) in jobs.iter().zip(&abschnitte) {
                fuellen(j, &mut a.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            }
            return;
        }
        self.frame_pool.run(jobs.len(), &|i| {
            fuellen(&jobs[i], &mut abschnitte[i].lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        });
    }

    #[allow(clippy::type_complexity)]
    // Letzter Wert: die Vertex-Bereiche je Entity (fuer Motion Blur, s. entity_motion).
    #[allow(clippy::type_complexity)]
    // Nahe Entities rastern (s. block.wgsl::entity_near_discard): Spieler und Ruestung sind wie in
    // Vanilla beidseitig, jede Flaeche liegt auch als Rueckseite im Modell. Durch die Rasterloecher
    // saehe man dann die Innenseite des Hinterkopfs, und in einem Kopf drin steckte man in
    // sechs Innenseiten (Nutzer 2026-11-11). Darum wird ein Entity, das so nah ist, dass es
    // gerastert werden kann, einseitig gezeichnet -- das Rueckseiten-Culling nimmt dann alles
    // Innere weg. Nah heisst: die Kamera ist hoechstens den Raster-Abstand von seiner Trefferbox
    // entfernt, die um NEAR_SINGLE_SIDED_MARGIN groesser ist (Ruestung, schwingende Arme).
    pub(crate) fn mark_near_entity(&self, feet: DVec3, width: f64, height: f64) {
        let ab = f64::from(self.settings.entity_near_dither_cm) / 100.0;
        let nah = ab > 0.0 && {
            let auge = self.camera.position;
            let h = width / 2.0 + NEAR_SINGLE_SIDED_MARGIN;
            let dx = ((auge.x - feet.x).abs() - h).max(0.0);
            let dz = ((auge.z - feet.z).abs() - h).max(0.0);
            let dy = (feet.y - NEAR_SINGLE_SIDED_MARGIN - auge.y).max(auge.y - (feet.y + height + NEAR_SINGLE_SIDED_MARGIN)).max(0.0);
            dx * dx + dy * dy + dz * dz <= ab * ab
        };
        self.near_single_sided.set(nah);
    }

    // Beidseitig zeichnen, wo Vanilla es tut -- ausser das Entity ist gerade nah (s. mark_near_entity).
    pub(crate) fn two_sided_here(&self) -> bool {
        !self.near_single_sided.get()
    }

    pub(crate) fn build_entity_vertices(
        &self,
    ) -> (Vec<world::BlockVertex>, Vec<[f32; 4]>, Vec<u32>, (usize, usize, usize), (Vec<world::BlockVertex>, Vec<[f32; 4]>), Vec<std::ops::Range<u32>>, Vec<(u64, std::ops::Range<usize>)>) {
        // Anteil 0 = Textur unveraendert (s. fs_entity).
        const KEINE_FAERBUNG: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
        use super::skin_frame::OHNE_NAH_RASTER;
        // Shader-Pfad: Licht je Vertex, parallel zu `tints` gefuellt. Ein Wert je Entity (an
        // seinen Augen, wie Vanilla), Blockentities je Vertex, Feuer und Spuren unbeleuchtet.
        const UNBELEUCHTET: u32 = 1 << 31;
        let lit = crate::shader_path_on();
        self.prune_anim_smoothing();
        self.anim_debug_frame();
        self.near_single_sided.set(false);
        let mut lights: Vec<u32> = Vec::new();
        let tint = self.settings.damage_tint_rgba();
        // Die zweite Faerbung fuer kritische Treffer. None = Modul aus, dann bleibt es bei der
        // einen Farbe fuer alles (s. CRIT_COLOR_ROW).
        let crit_tint = self.settings.module_crit_color_enabled.then(|| self.settings.crit_color_rgba());
        let blitz_ticks = self.settings.damage_tint_ticks;
        // Vertex-Bereiche der Koerpermodelle (Skin). Vanilla faerbt nur sie: Ruestung, Items,
        // Schild und Umhang zeichnet es ohne Schadens-Overlay. Mit "Nur Skin" wird am Ende
        // alles ausserhalb dieser Bereiche wieder entfaerbt.
        let mut haut: Vec<std::ops::Range<usize>> = Vec::new();
        // Brennende Entities: (Fuesse, Breite, Hoehe). Das Feuer kommt erst nach der Schleife
        // dran -- Vanilla zeichnet es ohne Schadens-Overlay, es darf also nie mitblitzen.
        // Mit dem Schluessel fuer den Motion Blur (None = eigener Spieler, der verwischt nicht).
        let mut feuer: Vec<(Option<u64>, [f32; 3], f32, f32)> = Vec::new();
        let eye = self.camera.position;
        // Weltposition -> augenrelative f32-Position fuers Modell.
        let rel = |p: DVec3| -> [f32; 3] {
            let v = (p - eye).as_vec3();
            [v.x, v.y, v.z]
        };
        // So gross wie im letzten Frame: ein Spielermodell allein hat Hunderte Vertices, die Liste
        // wuchs sonst jeden Frame Stueck fuer Stueck (Profiler: realloc).
        let mut out = Vec::with_capacity(((self.entities.len() + 1) * 36).max(self.entity_vertex_hint));
        let mut tints: Vec<[f32; 4]> = Vec::with_capacity(out.capacity());
        // Windkugeln: Vanilla zeichnet sie mit entityTranslucent, ihre Textur hat durchweg
        // Alpha 127. Im normalen Entity-Pass (Schnitt bei 0.5) verschwand sie damit ganz.
        // Sie kommen ans Ende und laufen durchscheinend.
        let mut durchscheinend: Vec<world::BlockVertex> = Vec::new();
        // Bereiche in `durchscheinend` mit eigener Deckkraft (als Tint -1 - Deckkraft, s.
        // block.wgsl::entity_fade): ausblendende Teile der Todesexplosion.
        let mut ausblenden: Vec<(std::ops::Range<usize>, f32)> = Vec::new();
        // Leuchtende: Bereiche in `out` mit ihrer Umrissfarbe, und was nur als Umriss erscheint.
        let mut umriss: Vec<(std::ops::Range<usize>, [f32; 4])> = Vec::new();
        let mut nur_umriss: Vec<world::BlockVertex> = Vec::new();
        let mut nur_umriss_farbe: Vec<[f32; 4]> = Vec::new();
        // So viele Vertices am Anfang sind der eigene Spieler (fuers Ausblenden, s. unten).
        let mut eigene = 0;
        // Render-Order: Vertex-Bereiche in `out` mit ihrer Klasse (s. render::render_order,
        // BlockRenderer::set_entity_classes); alles uebrige ist CLASS_OTHER.
        let mut klassen: Vec<(std::ops::Range<u32>, u8)> = Vec::new();
        // Was man lesen koennen soll (Karten in Rahmen, Schilder): diese Bereiche zeichnet die
        // Bewegungsunschaerfe danach noch einmal scharf (s. BlockRenderer::render_sharp_entities).
        let mut scharf: Vec<std::ops::Range<u32>> = Vec::new();
        // Modelle, die nach den Schleifen parallel entstehen (s. defer_model, run_model_jobs).
        let mut jobs: Vec<ModelJob<'_>> = Vec::new();
        // GPU-Skinning fuer Koerper fremder Spieler und Mobs (s. skin_frame). Der eigene Spieler,
        // Leuchtende, Ruestung, Umhang und die Todesexplosion bleiben vorerst auf dem CPU-Weg.
        let skinning = self.skinning_active();

        // Eigener Spieler -- NUR in der 3. Person. Im Ego-Modus saesse die Kamera mitten in
        // dieser Box und man saehe sie von innen. Ohne sie waere die 3.-Person-Ansicht
        // sinnlos: man schaut auf die Stelle, an der man steht, und dort ist nichts.
        // Ein Kasten wie bei allen anderen Entities auch (dieser Client zeichnet keine
        // Spielermodelle), nur in Blau, damit man sich von Fremden unterscheidet.
        if !self.perspective.is_first_person() {
            let feet = self.physics_render_prev.lerp(self.physics.pos, f64::from((self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0)));
            // Der eigene Spieler mit echtem Modell -- Gierung ist die Kamera-Gierung, die
            // Schrittamplitude kommt aus der eigenen Horizontalgeschwindigkeit (die kennen
            // wir hier exakt, anders als bei fremden Entities).
            let pitch = self.look_pitch_deg();
            // Bruchteil des laufenden Ticks -- dieselbe Groesse, mit der auch die Position
            // interpoliert wird, damit Schritt und Bewegung zusammenpassen.
            let tick_progress = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0);
            let (swing, amount) = (self.own_limbs.animation_progress(tick_progress), self.own_limbs.amplitude(tick_progress));
            // Der Kopf bekommt die Drehung RELATIV zum Koerper (Vanilla:
            // LivingEntityRenderer.updateRenderState). Vorher stand hier 0.0 -- der Kopf
            // schaute damit immer starr in Koerperrichtung, und weil der Koerper direkt an der
            // Maus hing, gab es die Vanilla-Traegheit gar nicht.
            let (own_main, own_off) = self.own_hand_items();
            let (block_right, block_left) = blocking_arms(self.own_using_item_right_arm(), (own_main, own_off));
            let glide = self.physics.gliding.then_some(world::Glide {
                ticks: self.own_gliding_ticks as f32 + tick_progress,
                pitch_deg: pitch,
                yaw_deg: self.camera.yaw,
                velocity: self.own_glide_velocity(tick_progress),
            });
            // LivingEntityRenderer.updateRenderState: wer sitzt, schwingt keine Beine.
            let reitet = self.own_ride.is_some();
            let (swing, amount) = if reitet { (0.0, 0.0) } else { (swing, amount) };
            // Mehr Animationen: zur Leiter drehen wie bei Fremden (s. dort).
            let anim = self.anim_features();
            let leiter = if anim & world::more_anims::LADDER != 0 && self.physics.is_climbing(&self.world) {
                self.ladder_yaw(self.physics.pos, self.physics.on_ground)
            } else {
                None
            };
            let koerper = self.own_body_yaw.body_yaw(tick_progress);
            let (body_yaw, head_rel) = match leiter {
                Some(wand) => (wand, world::wrap_degrees(koerper + self.own_body_yaw.relative_head_yaw(tick_progress) - wand).clamp(-90.0, 90.0)),
                None => (koerper, self.own_body_yaw.relative_head_yaw(tick_progress)),
            };
            let own_hand_swing = self.own_hand_swing(tick_progress);
            let own_use_pose = self.use_pose(self.own_using_item_right_arm(), (own_main, own_off), true, self.own_use_ticks as f32 + tick_progress, own_hand_swing > 0.0);
            let mut pose = world::entity_model::biped_pose_full(
                head_rel,
                pitch,
                swing,
                amount,
                world::entity_model::BipedExtras {
                    riding: reitet,
                    crawl: anim & world::more_anims::CRAWL != 0 && self.own_lean(tick_progress) > 0.0 && !self.physics.touching_water,
                    sneaking: self.physics.crouching && leiter.is_none(),
                    lean: self.own_lean(tick_progress),
                    age: self.own_age_ticks + tick_progress,
                    // Der eigene Schwung kommt nicht vom Server zurueck (der schickt ihn an
                    // alle ausser uns), sondern aus dem eigenen Zaehler -- derselbe, der auch
                    // den Arm in der Ego-Ansicht bewegt.
                    hand_swing: own_hand_swing,
                    swing_right_arm: true,
                    item_right: own_main.is_some(),
                    item_left: own_off.is_some(),
                    block_right,
                    block_left,
                    spear_right: is_spear_item(own_main),
                    spear_left: is_spear_item(own_off),
                    stab: is_spear_item(own_main),
                    glide,
                    use_pose: own_use_pose,
                },
            );
            if anim != 0 {
                let taken = arms_taken(own_use_pose, (block_right || is_spear_item(own_main), block_left || is_spear_item(own_off)));
                let ladder = leiter.map(|_| feet.y);
                let input = self.own_anim_input(tick_progress, pitch, swing, (own_hand_swing > 0.0).then_some(true), taken, ladder, glide.as_ref(), own_use_pose);
                world::more_anims::apply(&mut pose, &input);
                self.smooth_anims(None, &mut pose, (block_right, block_left));
            }
            let ansehen = (
                self.inspect_third_person(protocol::packets::HAND_MAIN, tick_progress, &mut pose),
                self.inspect_third_person(protocol::packets::HAND_OFF, tick_progress, &mut pose),
            );
            let jongliert = self.juggle_third_person(tick_progress, &mut pose);
            // Schwimm-/Kriech-Haltung: der ganze Koerper kippt in die Waagerechte (s.
            // world::Lean). Ohne das stand die Figur beim Schwimmen aufrecht im Wasser.
            let lean = world::Lean {
                amount: self.own_lean(tick_progress),
                pitch_deg: pitch,
                in_water: self.physics.touching_water,
                // AvatarRenderer: isVisuallySwimming, also die POSE -- auch beim Kriechen an
                // Land. Mit isSwimming fehlte dort der Versatz, die Figur lag zu tief.
                swimming: self.physics.swimming,
            };
            // Der eigene Skin kommt aus derselben Quelle wie der von Fremden: dem Eintrag
            // zur EIGENEN UUID in der Spielerliste. Genau so macht es Vanilla
            // (AbstractClientPlayerEntity.getSkin() -> getPlayerListEntry(getUuid())); ein
            // Sonderweg ueber das Microsoft-Konto waere falsch, weil im Offline-Mode die
            // Konto-UUID nicht der vom Server zugewiesenen entspricht.
            let (model, tex) = self.own_player_model_and_texture();
            // Gedreht wird das MODELL mit der Koerperdrehung, nicht mit der Blickrichtung --
            // sonst dreht sich die ganze Figur samt Beinen sofort mit der Maus mit.
            let base = match &glide {
                Some(glide) => world::entity_transform_gliding(rel(feet), body_yaw, self.physics.body.scale, world::entity_model::PLAYER_MODEL_SCALE, glide),
                None => world::entity_transform_leaning(rel(feet), body_yaw, self.physics.body.scale, world::entity_model::PLAYER_MODEL_SCALE, &lean),
            };
            self.mark_near_entity(feet, self.physics.width(), self.physics.current_height());
            let eigener_start = out.len();
            let eigene_umrissfarbe = self.own_glowing.then(|| self.glow_color(self.own_uuid.and_then(|u| self.player_names.get(&u))));
            // Unsichtbar: wie bei Fremden nur Koerper und Umhang weg (s. dort).
            if !self.own_invisible {
                let start = out.len();
                defer_model(&mut jobs, &mut out, model, &pose, base, tex, [ENTITY_BRIGHTNESS; 3], self.two_sided_here());
                haut.push(start..out.len());
            } else if let Some(farbe) = eigene_umrissfarbe {
                let vorher = nur_umriss.len();
                world::emit_entity_model_two_sided(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut nur_umriss);
                nur_umriss_farbe.resize(nur_umriss_farbe.len() + nur_umriss.len() - vorher, farbe);
            }
            if self.own_on_fire && self.game_mode != GAME_MODE_SPECTATOR {
                feuer.push((None, rel(feet), self.physics.width() as f32, self.physics.current_height() as f32));
            }
            let own_armor = self.own_armor();
            self.emit_armor(&own_armor, &self.armor_models, &pose, base, &mut out, Some(&mut jobs), None);
            let stich = if is_spear_item(own_main) { self.own_hand_swing(tick_progress) } else { 0.0 };
            let haende = self.shown_hands(self.own_hand_items(), own_use_pose, self.sleeping_at.is_some());
            // Beim Jonglieren fliegen die Items, in den Haenden liegt keins (s. unten).
            let haende = if jongliert.is_some() { (None, None) } else { haende };
            let schwert = self.own_last_sword.filter(|_| !self.own_invisible && self.sleeping_at.is_none() && !reitet);
            let haende = self.emit_hand_extras(
                i32::MIN,
                haende,
                true,
                (self.camera.yaw, pitch),
                (self.own_age_ticks as i64, tick_progress),
                schwert,
                &self.own_armor(),
                model,
                &pose,
                base,
                &mut out,
            );
            let ansehen = (
                self.third_person_item_extra(ansehen.0, true, model, &pose, base),
                self.third_person_item_extra(ansehen.1, false, model, &pose, base),
            );
            // Der eigene Spieler bleibt auf dem CPU-Weg (s. skinning_active).
            self.emit_held_items(haende, (block_right, block_left), self.own_using_item_right_arm(), (stich, true), ansehen, model, &pose, base, None, &mut out);
            if let Some(j) = jongliert.filter(|_| !self.own_invisible) {
                self.emit_juggle_third_person(j, model, &pose, base, &mut out);
            }
            let own_cape = self.own_uuid.and_then(|uuid| self.cape_texture(&uuid));
            let elytra = self.emit_elytra(&own_armor, own_cape, glide.as_ref(), self.physics.crouching, base, &mut out);
            // Der Umhang: eigener Zeichenaufruf, weil er eine andere Textur benutzt als der
            // Skin. Dieselbe Einbettung und dieselbe Pose -- er haengt am Rumpf und macht
            // dessen Haltung mit (s. world::cape_model). Mit Elytra nicht: dann haengt er als
            // Bild auf den Fluegeln (s. emit_elytra).
            if let Some(cape_tex) = own_cape.filter(|_| !elytra && !self.own_invisible) {
                let (vertical, swing, side) = world::cape_angles(
                    &self.own_cape,
                    // f64 und absolut -- cape_angles braucht nur die DIFFERENZ der beiden
                    // Positionen, und die ist in f64 ohnehin exakt.
                    [self.physics.pos.x, self.physics.pos.y, self.physics.pos.z],
                    [self.physics_render_prev.x, self.physics_render_prev.y, self.physics_render_prev.z],
                    // KOERPERdrehung, nicht Blickrichtung: Vanillas updateCape rechnet mit
                    // lerpAngleDegrees(..., lastBodyYaw, bodyYaw). Der Umhang haengt am Rumpf,
                    // also muss auch seine Seitwaerts-/Wehrichtung am Rumpf haengen -- mit der
                    // Blickrichtung wuerde er beim Umschauen im Stand zur Seite schlagen,
                    // obwohl sich der Koerper gar nicht bewegt.
                    self.own_body_yaw.body_yaw(tick_progress),
                    tick_progress,
                );
                let cape_pose = world::cape_pose(&pose, vertical, swing, side);
                defer_model(&mut jobs, &mut out, &self.cape_model, &cape_pose, base, cape_tex, [ENTITY_BRIGHTNESS; 3], false);
            }
            // Der eigene Spieler blitzt genauso auf -- sichtbar nur in der 3. Person, aber
            // dort waere sein Fehlen sofort auffaellig. Die Quelle ist eine andere als bei
            // Fremden: fuer den eigenen Spieler schickt der Server KEIN Damage-Paket
            // (ServerWorld sendet es nur an die anderen Beobachter), Vanilla leitet den
            // Treffer stattdessen aus dem Sinken der eigenen Gesundheit ab -- s.
            // ClientPlayerEntity.updateHealth und App::own_hurt_ticks.
            let eigene_faerbung = match (self.own_ticks_since_hurt < blitz_ticks, crit_tint) {
                (false, _) => KEINE_FAERBUNG,
                (true, Some(crit)) if self.own_hurt_by_crit => crit,
                (true, _) => tint,
            };
            tints.resize(out.len(), eigene_faerbung);
            if lit {
                lights.resize(out.len(), self.light_near(feet + DVec3::new(0.0, self.physics.current_height() * 0.9, 0.0)));
            }
            if let Some(farbe) = eigene_umrissfarbe {
                umriss.push((eigener_start..out.len(), farbe));
            }
            eigene = out.len();
            klassen.push((0..eigene as u32, render::render_order::CLASS_PVP));
        }

        // Frueher stand hier ein `if self.entities.is_empty() { return out; }`. Das war ein
        // BUG: der Ausstieg uebersprang auch emit_beds() weiter unten -- ohne ein einziges
        // Entity in Reichweite verschwanden alle Betten. Gespart hat er nichts, die Schleife
        // ueber eine leere Tabelle tut ohnehin nichts.
        // Spieler mit gehobenem Schild (Schild-Modul): ihr ganzer Vertex-Bereich bekommt den
        // Farbfilter, soweit dort nicht schon ein Schadensblitz liegt (s. unten).
        let mut schild_bereiche: Vec<(std::ops::Range<usize>, [f32; 4])> = Vec::new();
        // Die Glint-Zwillinge dieser Spieler (s. glint_out): bei voller Deckkraft fallen sie
        // weg -- der Glanz wird ADDITIV ueber das fertige Modell gelegt und liess den Spieler
        // sonst trotz 100 % nicht ganz in der Filterfarbe erscheinen.
        let mut schild_glint: Vec<std::ops::Range<usize>> = Vec::new();
        let mut je_entity: Vec<(u64, std::ops::Range<usize>)> = Vec::with_capacity(self.entities.len());
        for (_id, e) in self.entities.iter() {
            // Todesanimation Einfrieren: ein sterbender Spieler steht DEATH_FREEZE_TICKS lang nur
            // als Skin da -- ohne Ruestung, Items, Umhang, Feuer und Faerbung, ohne Umkippen.
            let gefroren = self.settings.death_animation == DEATH_ANIM_FREEZE && is_player_like(e.entity_type) && self.dying.contains_key(_id);
            if !self.entity_in_render_distance(e) {
                continue;
            }
            let entity_start = out.len();
            let glint_start = self.glint_out.borrow().len();
            let skin_start = self.skin_instance_count();
            // Ein einziger Bruchteil fuer ALLES an diesem Entity: Position, Rotation, Schritt,
            // Alter und Umhang muessen mit demselben Wert interpolieren, sonst laufen sie
            // gegeneinander. Seit die Entity-Glaettung dem Vanilla-Modell folgt, ist das der
            // ganz normale Tick-Fortschritt des Clients (s. world::Entity::render_pos).
            let progress = self.tick_progress();
            let (x, y, z) = e.render_pos(progress);
            if self.entity_culled(*_id, e, (x, y, z)) {
                continue;
            }
            let feet = rel(DVec3::new(x, y, z));
            let (breite, hoehe) = e.hit_size();
            // Nur Spieler werden nah einseitig gezeichnet (Nutzer 2026-11-14): Armorstaende und
            // Mobs behalten ihre Innenseiten (Helm von hinten), das Raster gilt fuer alle.
            if is_player_like(e.entity_type) {
                self.mark_near_entity(DVec3::new(x, y, z), breite, hoehe);
            } else {
                self.near_single_sided.set(false);
            }
            let body_yaw = e.render_yaw(progress);
            let head_rel = e.relative_head_yaw(progress);
            let pitch = e.render_pitch(progress);
            let (swing, amount) = e.limb_swing(progress);
            // Display-Entities zuerst: sie haben kein Modell und keine Trefferbox, sondern
            // zeigen einen Gegenstand, einen Blockzustand oder Text. Text wird NICHT hier
            // gezeichnet (er ist Bildschirm-Geometrie, s. build_world_labels), die beiden
            // anderen schon.
            //
            // Ohne diesen Zweig fielen sie in den "kein Modell"-Fall ganz unten und stuenden
            // als orangefarbene, spielergrosse Kaesten in der Welt.
            if let Some(display) = &e.display {
                self.emit_display_entity(display, e.age_ticks(progress), feet, body_yaw, pitch, &mut out);
            }
            // Ein unsichtbares Entity (Entity.FLAGS Bit 5) bekommt kein Modell. Das ist der
            // Normalfall bei Hologrammen: ein unsichtbarer Armorstand traegt den Namen. Sein
            // NAMENSSCHILD zeichnet build_world_labels trotzdem -- genau wie Vanilla, das
            // renderLabelIfPresent unabhaengig von der Sichtbarkeit aufruft.
            //
            // Spieler zuerst: eigener Skin-Platz und eigenes Modell (wide/slim).
            //
            // Kein `continue` mehr in diesem Zweig, sondern if/else: am Schleifenende muss
            // die Faerbung dieses Entities auf ALLE seine Vertices ausgedehnt werden, und
            // ein Ausstieg mittendrin haette genau die Spieler ausgelassen.
            //
            // Unsichtbare SPIELER laufen trotzdem in ihren Zweig: Vanilla blendet nur den
            // Koerper (und den Umhang) aus, Ruestung, Elytra und gehaltene Items bleiben
            // sichtbar (die Feature-Renderer pruefen die Unsichtbarkeit nicht).
            else if world::entity_type_name(e.entity_type) == Some("armor_stand") {
                let skin = skinning.then(|| crate::entity_motion::key(crate::entity_motion::KIND_ENTITY, *_id));
                self.emit_armor_stand(e, feet, body_yaw, skin, &mut out, &mut haut);
            } else if e.invisible && !is_player_like(e.entity_type) && !world::entity::is_item_frame(e.entity_type) {
                // nichts zeichnen (ein unsichtbarer Rahmen zeigt seinen Inhalt weiter)
            } else if matches!(world::entity_type_name(e.entity_type), Some("interaction" | "marker" | "area_effect_cloud")) {
                // Unsichtbar auch in Vanilla (kein Modell, nur Klickflaeche bzw. Datentraeger).
                // Sonst stehen sie (etwa auf mcpvp.club) als orange Kaesten neben den NPCs.
            } else if world::entity_type_name(e.entity_type) == Some("experience_orb") {
                // Als Bild bei den Partikeln, s. experience_orb_vertices.
            } else if e.falling_block.is_some() || world::entity_type_name(e.entity_type) == Some("tnt") {
                // Als echter Block im Block-Atlas, s. display_block_vertices.
            } else if is_player_like(e.entity_type) && self.dying.contains_key(_id) && (matches!(self.settings.death_animation, DEATH_ANIM_OFF | DEATH_ANIM_EXPLOSION) || gefroren && self.dying[_id] >= DEATH_FREEZE_TICKS) {
                // Todesanimation Aus oder Explosion: die Leiche wird nicht gezeichnet (die
                // Teile der Explosion zeichnet die Schleife unten).
            } else if is_player_like(e.entity_type) {
                // ECHTER BUG gewesen: hier stand `false` fest -- ein fremder Spieler blieb
                // beim Schleichen kerzengerade stehen, obwohl die Pose vom Server ankam und
                // biped_pose sie auch auswerten kann. Sichtbar wurde es erst im Duell.
                // Die eigene Figur hatte den Zustand die ganze Zeit (s. physics.crouching
                // weiter oben) -- die beiden Aufrufstellen waren auseinandergelaufen.
                // ECHTER BUG gewesen (fremde Spieler schwangen sichtbar nicht): hier gab es den Handschlag gar nicht. Er folgt aus keiner
                // Groesse, die man mitbekommt -- er kommt als eigenes Paket (s.
                // packets::CB_PLAY_ANIMATE) und muss von dort bis hierher durchgereicht
                // werden.
                let (hand_swing, swing_right_arm) = e.hand_swing(progress);
                // Blockt der Gegner gerade? Das steht in seinem LIVING_FLAGS-Byte (s.
                // Entity::using_item_right_arm) -- die wichtigste Auskunft im Nahkampf.
                let (block_right, block_left) = self.entity_blocking_arms(e);
                // Was welcher Arm haelt: bei Linkshaendern die Haupthand links (s.
                // Entity::left_handed).
                let (arm_rechts, arm_links) = e.arm_items();
                let (speer_rechts, speer_links) = e.arm_spears();
                // Elytra-Gleitflug: ECHTER BUG gewesen -- fliegende Spieler sahen falsch aus.
                // Der Flug-Zustand kam
                // zwar vom Server an (Entity.FLAGS Bit 7), die Figur stand aber aufrecht in der
                // Luft und hatte keine Fluegel.
                let glide = e.glide(progress);
                // Schwimm-/Kriechhaltung: beim Sprint-Schwimmen standen fremde Spieler standen dabei aufrecht,
                // weil nur die eigene Figur die Neigung bekam.
                let lean = e.lean(progress);
                let reitet = self.entities.is_riding(*_id);
                let (swing, amount) = if reitet { (0.0, 0.0) } else { (swing, amount) };
                // Mehr Animationen: an der Leiter dreht sich der Koerper zur Wand, der Kopf
                // bleibt im Rahmen von 90 Grad zu ihm; schleichend haelt man sich aufrecht fest.
                let anim = self.anim_features();
                let fuesse = DVec3::new(x, y, z);
                let leiter = if anim & world::more_anims::LADDER != 0 { self.ladder_yaw(fuesse, self.remote_on_ground(fuesse)) } else { None };
                let (body_yaw, head_rel) = match leiter {
                    Some(wand) => (wand, world::wrap_degrees(body_yaw + head_rel - wand).clamp(-90.0, 90.0)),
                    None => (body_yaw, head_rel),
                };
                let benutzt = e.used_item().map_or(0.0, |(_, t)| t as f32 + progress);
                let fremd_use_pose = self.use_pose(e.using_item_right_arm(), (arm_rechts, arm_links), !e.left_handed, benutzt, hand_swing > 0.0);
                let mut pose = world::entity_model::biped_pose_full(
                    head_rel,
                    pitch,
                    swing,
                    amount,
                    world::entity_model::BipedExtras {
                        riding: reitet,
                        use_pose: fremd_use_pose,
                        crawl: anim & world::more_anims::CRAWL != 0 && lean.amount > 0.0 && !lean.in_water,
                        sneaking: e.sneaking() && leiter.is_none(),
                        lean: lean.amount,
                        age: e.age_ticks(progress),
                        hand_swing,
                        swing_right_arm,
                        item_right: arm_rechts.is_some(),
                        item_left: arm_links.is_some(),
                        block_right,
                        block_left,
                        spear_right: speer_rechts,
                        spear_left: speer_links,
                        // Die Schlagart haengt am Haupthand-Item, auch fuer die Nebenhand.
                        stab: e.main_hand_spear(),
                        glide,
                    },
                );
                if anim != 0 {
                    let input = self.remote_anim_input(
                        *_id,
                        e,
                        progress,
                        pitch,
                        swing,
                        (hand_swing > 0.0).then_some(swing_right_arm),
                        arms_taken(fremd_use_pose, (block_right || speer_rechts, block_left || speer_links)),
                        leiter.map(|_| y),
                        glide.as_ref(),
                        &lean,
                        fremd_use_pose,
                    );
                    world::more_anims::apply(&mut pose, &input);
                    self.smooth_anims(Some(*_id), &mut pose, (block_right, block_left));
                }
                let (model, tex) = self.player_model_and_texture(&e.uuid);
                let base = match &glide {
                    // Sterbend: umkippen statt Flug oder Neigung (setupRotations prueft den Tod
                    // zuerst).
                    _ if self.death_time(*_id).is_some() => world::entity_model::entity_transform_dying(
                        feet,
                        body_yaw,
                        e.scale,
                        world::entity_model::PLAYER_MODEL_SCALE,
                        self.death_time(*_id).unwrap_or_default(),
                    ),
                    // LivingEntityRenderer: das ganze Modell mal SCALE (s. Entity::scale).
                    Some(glide) => world::entity_transform_gliding(feet, body_yaw, e.scale, world::entity_model::PLAYER_MODEL_SCALE, glide),
                    None => world::entity_transform_leaning(feet, body_yaw, e.scale, world::entity_model::PLAYER_MODEL_SCALE, &lean),
                };
                // Koerper per GPU-Skinning (s. skin_frame); leuchtend zusaetzlich als Silhouette,
                // unsichtbar und leuchtend NUR als Silhouette.
                let schluessel = crate::entity_motion::key(crate::entity_motion::KIND_ENTITY, *_id);
                if !e.invisible && skinning {
                    self.push_skinned_body(model, &pose, base, tex, self.two_sided_here(), schluessel);
                } else if e.invisible && e.glowing && skinning {
                    self.push_skinned_outline_only(model, &pose, base, tex, true, schluessel, self.entity_glow_color(e));
                } else if !e.invisible {
                    let start = out.len();
                    defer_model(&mut jobs, &mut out, model, &pose, base, tex, [ENTITY_BRIGHTNESS; 3], self.two_sided_here());
                    haut.push(start..out.len());
                } else if e.glowing {
                    let vorher = nur_umriss.len();
                    world::emit_entity_model_two_sided(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut nur_umriss);
                    nur_umriss_farbe.resize(nur_umriss_farbe.len() + nur_umriss.len() - vorher, self.entity_glow_color(e));
                }
                if !gefroren {
                    self.emit_armor(&e.armor, &self.armor_models, &pose, base, &mut out, Some(&mut jobs), skinning.then_some(schluessel));
                    // Gestochen wird mit der Haupthand -- an ihrem Arm.
                    let haupthand_schwingt = swing_right_arm != e.left_handed;
                    let stich = if e.main_hand_spear() && haupthand_schwingt { hand_swing } else { 0.0 };
                    let schlaeft = e.pose == world::entity::POSE_SLEEPING;
                    let haende = self.shown_hands((arm_rechts, arm_links), fremd_use_pose, schlaeft);
                    let schwert = e.last_sword.filter(|_| !e.invisible && !schlaeft && !reitet);
                    let haende = self.emit_hand_extras(
                        *_id,
                        haende,
                        !e.left_handed,
                        (e.yaw_deg, pitch),
                        (e.age_ticks(0.0) as i64, progress),
                        schwert,
                        &e.armor,
                        model,
                        &pose,
                        base,
                        &mut out,
                    );
                    self.emit_held_items(haende, (block_right, block_left), e.using_item_right_arm(), (stich, swing_right_arm), (world::Affine::IDENTITY, world::Affine::IDENTITY), model, &pose, base, skinning.then_some(schluessel), &mut out);
                    let cape = self.cape_texture(&e.uuid);
                    let elytra = self.emit_elytra(&e.armor, cape, glide.as_ref(), e.sneaking(), base, &mut out);
                    if let Some(cape_tex) = cape.filter(|_| !elytra && !e.invisible) {
                        let (vertical, swing, side) = e.cape_angles(progress);
                        let cape_pose = world::cape_pose(&pose, vertical, swing, side);
                        if skinning {
                            self.push_skinned(&self.cape_model, &cape_pose, base, cape_tex, [ENTITY_BRIGHTNESS; 3], false, schluessel);
                        } else {
                            defer_model(&mut jobs, &mut out, &self.cape_model, &cape_pose, base, cape_tex, [ENTITY_BRIGHTNESS; 3], false);
                        }
                    }
                }
            } else if world::entity_type_name(e.entity_type) == Some("end_crystal") {
                self.emit_end_crystal(*_id, e, feet, progress, &mut out);
            } else if matches!(world::entity_type_name(e.entity_type), Some("wind_charge" | "breeze_wind_charge")) {
                if self.projectile_in_render_range(e, feet, 4.0) {
                    self.emit_wind_charge(e, feet, progress, &mut durchscheinend);
                }
            } else if let Some(pfeil_tex) = arrow_texture(e.entity_type) {
                if self.projectile_in_render_range(e, feet, 1.0) {
                    self.emit_arrow(e, feet, progress, pfeil_tex, &mut out);
                }
            } else if world::entity_type_name(e.entity_type) == Some("trident") {
                if self.projectile_in_render_range(e, feet, 1.0) {
                    self.emit_thrown_trident(e, feet, progress, &mut out);
                }
            } else if let Some(art) = world::item_render_kind(e.entity_type) {
                self.emit_item_entity(*_id, e, art, feet, progress, &mut out);
            } else if world::entity::is_item_frame(e.entity_type) {
                let start = out.len() as u32;
                self.emit_item_frame(e, feet, &mut out);
                if e.item.and_then(|h| h.map_id).is_some() {
                    scharf.push(start..out.len() as u32);
                }
            } else {
                match self.model_for_type(e.entity_type) {
                    Some((model, kind, tex)) => {
                        use world::entity_model::ModelKind;
                        let pose = match kind {
                            ModelKind::Biped | ModelKind::Enderman => {
                                // Mobs schlagen genauso -- ein Zombie holt sichtbar aus,
                                // bevor er trifft, und das ist im Nahkampf die einzige
                                // Vorwarnung, die man bekommt.
                                let (hand_swing, swing_right_arm) = e.hand_swing(progress);
                                let reitet = self.entities.is_riding(*_id);
                                let (swing, amount) = if reitet { (0.0, 0.0) } else { (swing, amount) };
                                world::entity_model::biped_pose_full(
                                    head_rel,
                                    pitch,
                                    swing,
                                    amount,
                                    world::entity_model::BipedExtras {
                                        riding: reitet,
                                        age: e.age_ticks(progress),
                                        hand_swing,
                                        swing_right_arm,
                                        item_right: e.arm_items().0.is_some(),
                                        item_left: e.arm_items().1.is_some(),
                                        block_right: self.entity_blocking_arms(e).0,
                                        block_left: self.entity_blocking_arms(e).1,
                                        ..Default::default()
                                    },
                                )
                            }
                            ModelKind::Quadruped => world::entity_model::quadruped_pose(head_rel, pitch, swing, amount),
                            ModelKind::Creeper => world::entity_model::creeper_pose(head_rel, pitch, swing, amount),
                            ModelKind::Spider => world::entity_model::spider_pose(head_rel, pitch, swing, amount),
                            // Der Schleim wird nicht animiert (Vanilla laesst ihn beim Huepfen
                            // wabbern -- dafuer brauchte es den Sprungzustand vom Server).
                            ModelKind::Slime => world::Pose::new(),
                            ModelKind::Feline => world::entity_model::feline_pose(head_rel, pitch, swing, amount, e.sitting(), e.sneaking()),
                            ModelKind::Turtle => world::entity_model::turtle_pose(head_rel, pitch, swing, amount, !e.is_in_water()),
                        };
                        // Katzen: die Farbe aus der Variante, sonst die Standard-Katze.
                        let tex = match e.cat_variant.and_then(|v| usize::try_from(v).ok()).and_then(|i| self.cat_variants.get(i).copied().flatten()) {
                            Some(pfad) if kind == ModelKind::Feline => self.entity_atlas.rect(pfad).unwrap_or(tex),
                            _ => tex,
                        };
                        let model_scale = world::entity_type_name(e.entity_type).map_or(1.0, world::entity_model::model_scale);
                        // Mobs bekommen 1.0: die 0.9375 sind eine Eigenheit von
                        // PlayerEntityRenderer.scale(), die uebrigen LivingEntityRenderer erben
                        // die leere Standardfassung (kein scale-Aufruf).
                        let mut pose = pose;
                        // Nur noch fuer die Modellarten OHNE eigenen Handschlag-Zweig
                        // (Vierbeiner, Creeper, Spinne): beim Biped steckt die Armhaltung
                        // schon in biped_pose_full, an der richtigen Stelle vor dem Schlag.
                        if !matches!(kind, ModelKind::Biped | ModelKind::Enderman) {
                            apply_item_arm_poses(&mut pose, e.arm_items().0.is_some(), e.arm_items().1.is_some());
                        }
                        let base = match self.death_time(*_id) {
                            Some(t) => world::entity_model::entity_transform_dying(feet, body_yaw, e.scale, model_scale, t),
                            None => world::entity_transform(feet, body_yaw, e.scale, model_scale),
                        };
                        if skinning {
                            self.push_skinned_body(model, &pose, base, tex, false, crate::entity_motion::key(crate::entity_motion::KIND_ENTITY, *_id));
                        } else {
                            let start = out.len();
                            defer_model(&mut jobs, &mut out, model, &pose, base, tex, [ENTITY_BRIGHTNESS; 3], false);
                            haut.push(start..out.len());
                        }
                        // Ruestung nur am Biped: die anderen Modelle haben die Teile gar
                        // nicht, die armor_model erwartet -- ein Ruestungshelm ueber einem
                        // Spinnenkopf waere falsch, nicht bloss unschoen.
                        if matches!(kind, ModelKind::Biped) {
                            self.emit_armor(&e.armor, &self.armor_models, &pose, base, &mut out, Some(&mut jobs), skinning.then(|| crate::entity_motion::key(crate::entity_motion::KIND_ENTITY, *_id)));
                        }
                        // Auch Mobs tragen etwas (Zombie mit Schaufel, Skelett mit Bogen). Die
                        // Modelle ohne Arme (Spinne, Schleim, ...) haben schlicht kein
                        // "right_arm"-Teil -- part_transform liefert dann None und es passiert
                        // nichts, ganz ohne Sonderfall pro ModelKind.
                        self.emit_held_items(
                            e.arm_items(),
                            self.entity_blocking_arms(e),
                            e.using_item_right_arm(),
                            (0.0, true),
                            (world::Affine::IDENTITY, world::Affine::IDENTITY),
                            model,
                            &pose,
                            base,
                            skinning.then(|| crate::entity_motion::key(crate::entity_motion::KIND_ENTITY, *_id)),
                            &mut out,
                        );
                    }
                    // Modell aus der Datentabelle (Dorfbewohner, Tiere, Boote, Loren, ...).
                    None if world::entity_type_name(e.entity_type).and_then(world::mob_catalog::mob_look).is_some() => {
                        let start = out.len();
                        let skin = skinning.then(|| crate::entity_motion::key(crate::entity_motion::KIND_ENTITY, *_id));
                        self.emit_catalog_entity(e, feet, body_yaw, head_rel, pitch, (swing, amount), skin, &mut out);
                        haut.push(start..out.len());
                    }
                    // Kein Modell fuer diesen Typ: ein schwarz-pink karierter Kasten (wie Vanillas
                    // Fehltextur), damit sofort auffaellt, was der Client nicht kennt.
                    // Kleinkram bleibt klein, damit eine spielergrosse Box um ein Item nicht so
                    // tut, als stuende da ein Gegner.
                    None => {
                        let (half_w, height) = if world::is_attackable(e.entity_type) {
                            (physics::PLAYER_WIDTH / 2.0, physics::PLAYER_HEIGHT)
                        } else {
                            (0.125f32, 0.25f32)
                        };
                        let (x, y, z) = (feet[0], feet[1], feet[2]);
                        out.extend(world::box_vertices(
                            [x - half_w, y, z - half_w],
                            [x + half_w, y + height, z + half_w],
                            [1.0; 3],
                            // Aus dem ENTITY-Atlas, nicht dem Block-Atlas: dieser Durchgang
                            // bindet den Entity-Atlas.
                            self.entity_atlas.missing_tile(),
                        ));
                    }
                }
            }
            if e.on_fire && !gefroren {
                let (breite, hoehe) = e.hit_size();
                feuer.push((Some(crate::entity_motion::key(crate::entity_motion::KIND_FIRE, *_id)), feet, breite as f32, hoehe as f32));
            }
            // Alles, was dieses Entity gerade beigesteuert hat, bekommt seine Faerbung.
            // is_hurt() ist bei Nicht-Lebewesen nie wahr (s. world::Entity::on_damaged), ein
            // brennendes Boot blitzt also nicht -- genau wie in Vanilla.
            //
            // War der Treffer ein KRITISCHER und ist das Modul an, gilt dessen eigene Farbe
            // (s. world::Entity::is_hurt_by_crit). Die Abfrage sitzt bewusst hier und nicht in
            // der Entity: welche Farbe gilt, ist eine Frage der Einstellungen, nicht des
            // Weltzustands.
            // Pfeile-Modul "Weisser Pfeil": die Faerbung mischt nach Textur und Licht voll auf
            // Weiss (s. block.wgsl::fs_entity) -- der Pfeil ist dann eine weisse Silhouette.
            let weisser_pfeil = self.settings.module_arrows && self.settings.arrow_white && arrow_texture(e.entity_type).is_some();
            // Sterbend: rot bis zum Ende (LivingEntityRenderer: hasRedOverlay mit deathTime > 0).
            let faerbung = match (!gefroren && (e.damage_flash(blitz_ticks) || self.dying_red(*_id)), crit_tint) {
                _ if weisser_pfeil => [1.0, 1.0, 1.0, 1.0],
                (false, _) => KEINE_FAERBUNG,
                (true, Some(crit)) if e.hurt_by_crit() => crit,
                (true, _) => tint,
            };
            tints.resize(out.len(), faerbung);
            if e.glowing && out.len() > entity_start {
                umriss.push((entity_start..out.len(), self.entity_glow_color(e)));
            }
            if lit {
                let (ex, ey, ez) = e.render_pos(progress);
                let hoehe = e.hit_size().1;
                lights.resize(out.len(), self.light_near(DVec3::new(ex, ey + hoehe * 0.85, ez)));
            }
            // "Nur Skin" loescht spaeter alle Faerbung ausserhalb von `haut` -- der weisse
            // Pfeil zaehlt dafuer als Haut, sonst verschwaende er mit der Einstellung.
            if weisser_pfeil {
                haut.push(entity_start..out.len());
            }
            // Schild-Modul, Anzeige "Modell": Cooldown oder gehobenes Schild faerbt den Spieler.
            let mut schildfilter = None;
            if !e.invisible {
                if let Some(filter) = self.shield_mark_color(*_id, e, ShieldMark::Model) {
                    schild_bereiche.push((entity_start..out.len(), filter));
                    // Bei voller Deckkraft faellt der Glanz weg (s. unten).
                    if filter[3] >= 1.0 {
                        schild_glint.push(glint_start..self.glint_out.borrow().len());
                    }
                    schildfilter = Some(filter);
                }
            }
            // Die Instanzen dieses Entities (GPU-Skinning) wie seine Ecken.
            // Render-Order: Spieler und die anderen PvP-Arten des Hitbox-Moduls bilden eine Klasse.
            let pvp = hitbox_kinds::HitboxKind::of_entity(e.entity_type).is_some_and(hitbox_kinds::HitboxKind::is_pvp);
            if pvp {
                klassen.push((entity_start as u32..out.len() as u32, render::render_order::CLASS_PVP));
                self.skin_entity_class(skin_start, render::render_order::CLASS_PVP);
            }
            if self.skin_instance_count() > skin_start {
                self.finish_skin_entity(skin_start, faerbung, schildfilter, e.glowing.then(|| self.entity_glow_color(e)));
            }
            je_entity.push((crate::entity_motion::key(crate::entity_motion::KIND_ENTITY, *_id), entity_start..out.len()));
        }
        // Das Feuer zieht mit seinem Entity mit, also eigene Bereiche fuer den Motion Blur.
        let flammen_ab = out.len() as u32;
        for (schluessel, fuesse, breite, hoehe) in feuer {
            let start = out.len();
            self.emit_entity_fire(fuesse, breite, hoehe, &mut out);
            if let Some(k) = schluessel {
                je_entity.push((k, start..out.len()));
            }
        }
        klassen.push((flammen_ab..out.len() as u32, render::render_order::CLASS_FLAME));
        self.emit_trails(eye, self.tick_progress(), &mut out);
        tints.resize(out.len(), KEINE_FAERBUNG);
        if lit {
            lights.resize(out.len(), UNBELEUCHTET);
        }
        for b in &self.death_bursts {
            let start = out.len();
            let (model, tex) = self.player_model_and_texture(&b.uuid);
            let progress = self.tick_progress();
            let deckkraft = b.alphas(progress);
            let alpha = |name: &str| deckkraft.iter().find(|(n, _)| *n == name).map_or(1.0, |(_, a)| *a);
            let feet = DVec3::new(b.feet.0, b.feet.1, b.feet.2);
            self.mark_near_entity(feet, 0.6 * f64::from(b.scale), 1.8 * f64::from(b.scale));
            let base = world::entity_model::entity_transform(rel(feet), b.yaw, b.scale, world::entity_model::PLAYER_MODEL_SCALE);
            // Nur der Skin, keine Ruestung (s. death_burst). Ausblendende Teile einzeln und
            // durchscheinend, einseitig (sonst schimmerte die Innenseite der hinteren Flaechen
            // durch).
            for &(name, a) in &deckkraft {
                if a <= 0.0 || a >= 1.0 {
                    continue;
                }
                let pose = b.pose_of(progress, &|n| n == name);
                let von = durchscheinend.len();
                world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut durchscheinend);
                ausblenden.push((von..durchscheinend.len(), a));
            }
            // Der Rest wie gewohnt deckend.
            let pose = b.pose_of(progress, &|n| alpha(n) >= 1.0);
            defer_model(&mut jobs, &mut out, model, &pose, base, tex, [ENTITY_BRIGHTNESS; 3], self.two_sided_here());
            haut.push(start..out.len());
            je_entity.push((crate::entity_motion::key(crate::entity_motion::KIND_DEATH_BURST, b.entity_id), start..out.len()));
            tints.resize(out.len(), KEINE_FAERBUNG);
            if lit {
                lights.resize(out.len(), self.light_near(feet + DVec3::new(0.0, 0.5, 0.0)));
            }
        }
        if lit {
            lights.resize(out.len(), UNBELEUCHTET);
        }
        // Ab hier liest wieder jemand Vertices aus `out` (Licht, Umriss): vorher fertig machen.
        self.run_model_jobs(&jobs, &mut out);
        // Betten sind Blockgeometrie im Entity-Puffer, kein Lebewesen -- nie gefaerbt.
        // Ab hier keine Entities mehr: Puppe, Ego-Hand und Ansichten zeichnen wie Vanilla.
        self.near_single_sided.set(false);
        let blockentities_ab = out.len() as u32;
        let skin_blockentities_ab = self.skin_instance_count();
        self.emit_beds(eye, &mut out);
        self.emit_shulker_boxes(eye, &mut out);
        self.emit_chests(eye, &mut out);
        self.emit_skulls(eye, &mut out);
        self.emit_statues(eye, &mut out);
        let start = out.len() as u32;
        self.emit_signs(eye, &mut out);
        scharf.push(start..out.len() as u32);
        klassen.push((blockentities_ab..out.len() as u32, render::render_order::CLASS_BLOCK_ENTITY));
        // Dasselbe fuer Blockentities mit Skinning (Truhen, Betten ...).
        self.skin_entity_class(skin_blockentities_ab, render::render_order::CLASS_BLOCK_ENTITY);
        let blockentities = blockentities_ab as usize..out.len();
        *self.entity_classes_out.borrow_mut() = klassen;
        tints.resize(out.len(), KEINE_FAERBUNG);
        if lit {
            for v in &out[lights.len()..] {
                lights.push(self.light_near(eye + glam::Vec3::from(v.position).as_dvec3()));
            }
        }
        if self.settings.damage_tint_skin_only {
            let mut nur_haut = vec![KEINE_FAERBUNG; tints.len()];
            for bereich in haut {
                nur_haut[bereich.clone()].copy_from_slice(&tints[bereich]);
            }
            tints = nur_haut;
        }
        // Block-Entities (Betten, Truhen, Schilder ...) nie nah rastern (Nutzer 2026-11-12): eine
        // Truhe direkt vor einem ist Teil der Welt wie ein Block. Erst nach "Nur Skin", das die
        // Liste neu aufbaut.
        tints[blockentities].fill(OHNE_NAH_RASTER);
        // Erst nach "Nur Skin": der Filter gilt ausdruecklich fuer den GANZEN Spieler samt
        // Umhang, Ruestung und Items. Wo gerade ein Schadensblitz liegt, bleibt der sichtbar.
        {
            let mut glint = self.glint_out.borrow_mut();
            for bereich in schild_glint.into_iter().rev() {
                glint.drain(bereich);
            }
        }
        for (bereich, filter) in schild_bereiche {
            for t in &mut tints[bereich] {
                if *t == KEINE_FAERBUNG {
                    *t = filter;
                }
            }
        }
        let anzahl = durchscheinend.len();
        let anfang = out.len();
        out.extend(durchscheinend);
        tints.resize(out.len(), KEINE_FAERBUNG);
        for (bereich, a) in ausblenden {
            tints[anfang + bereich.start..anfang + bereich.end].fill([0.0, 0.0, 0.0, -1.0 - a]);
        }
        // Schulterkamera nah am eigenen Spieler: er laeuft durchscheinend mit seiner Deckkraft
        // im Tint (s. block.wgsl::entity_fade), die Schadens-Faerbung entfaellt dabei.
        let kopf = if self.own_player_alpha < 1.0 { eigene } else { 0 };
        for t in &mut tints[..kopf] {
            *t = [0.0, 0.0, 0.0, -1.0 - self.own_player_alpha.max(0.0)];
        }
        if lit {
            for v in &out[lights.len()..] {
                lights.push(self.light_near(eye + glam::Vec3::from(v.position).as_dvec3()));
            }
        }
        for (bereich, farbe) in umriss {
            nur_umriss_farbe.resize(nur_umriss_farbe.len() + bereich.len(), farbe);
            nur_umriss.extend_from_slice(&out[bereich]);
        }
        (out, tints, lights, (kopf, anzahl, eigene), (nur_umriss, nur_umriss_farbe), scharf, je_entity)
    }
}
