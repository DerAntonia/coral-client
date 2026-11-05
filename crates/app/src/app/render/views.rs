// Katalog-Entities, Ruestungsstaender, Hintergrundbilder, letzte Ansicht, Hero-Szene, Voxel-Volumen.

use super::*;

// Die Lore vor ihrem scale(-1, -1, 1) (AbstractMinecartRenderer.oldRender): ueber den Fuessen
// um 0.375 angehoben und gedreht. Darauf sitzen das Modell und der Anzeigeblock.
pub(crate) fn minecart_frame(feet: [f32; 3], yaw: f32) -> world::Affine {
    world::Affine::translation(feet).then(world::Affine::translation([0.0, 0.375, 0.0])).then(world::Affine::rotation_y_deg(180.0 - yaw))
}

// Der Anzeigeblock einer Lore (getDisplayBlockState: der eigene, sonst die Vorgabe der
// Lorenart) und seine Blockecke relativ zu minecart_frame: scale 0.75, translate(-0.5,
// (Versatz - 8) / 16, 0.5), rotateY 90. None ohne Block (Luft).
pub(crate) fn minecart_content(e: &world::Entity, name: &str) -> Option<(u32, world::Affine)> {
    use world::Affine;
    let (vorgabe, versatz) = world::mob_catalog::minecart_default_display(name, e.minecart_fuel);
    let state = e.minecart_block.or(vorgabe).filter(|s| *s != world::STATE_AIR && !world::block_registry::air_variants().contains(s))?;
    let versatz = e.minecart_offset.unwrap_or(versatz);
    let ecke = Affine::scale([0.75; 3]).then(Affine::translation([-0.5, (versatz - 8) as f32 / 16.0, 0.5])).then(Affine::rotation_y_deg(90.0));
    Some((state, ecke))
}

impl App {
    // Ein Entity mit Modell aus world::mob_catalog: Geometrie aus den Modelldaten, Grundtextur
    // plus Schichten (Dorfbewohner), gemeinsame Laufanimation. Boote und Loren stehen nicht
    // wie Lebewesen in der Welt, sondern um ihre Mitte gedreht und 0.375 hoeher
    // (AbstractBoatEntityRenderer / AbstractMinecartEntityRenderer).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_catalog_entity(
        &self,
        e: &world::Entity,
        feet: [f32; 3],
        body_yaw: f32,
        head_rel: f32,
        pitch: f32,
        (swing, amount): (f32, f32),
        // Per GPU-Skinning (s. skin_frame), mit dem Motion-Blur-Schluessel des Entities.
        skin: Option<u64>,
        out: &mut Vec<world::BlockVertex>,
    ) {
        use world::entity_model::Affine;
        use world::mob_catalog::{self as katalog, Anim};
        let Some(name) = world::entity_type_name(e.entity_type) else { return };
        let Some(look) = katalog::mob_look(name) else { return };
        let layer = if e.baby { look.baby_layer.unwrap_or(look.layer) } else { look.layer };
        let Some(model) = world::model_data::layer_model(layer) else { return };
        let grund = if e.baby { look.baby_texture.unwrap_or(look.texture) } else { look.texture };
        let mut texturen = vec![grund.to_string()];
        if matches!(name, "villager" | "zombie_villager") {
            texturen.extend(katalog::villager_layers(name == "zombie_villager", e.villager_data, e.baby));
        }
        let base = if katalog::is_boat(name) {
            Affine::translation(feet)
                .then(Affine::rotation_y_deg(180.0 - body_yaw))
                .then(Affine::translation([0.0, 0.375, 0.0]))
                .then(Affine::scale([-1.0, -1.0, 1.0]))
                .then(Affine::rotation_y_deg(90.0))
        } else if katalog::is_minecart(name) {
            minecart_frame(feet, body_yaw).then(Affine::scale([-1.0, -1.0, 1.0]))
        } else {
            world::entity_transform(feet, body_yaw, e.scale, 1.0)
        };
        let haelt = e.main_hand.is_some() || e.off_hand.is_some();
        let mut pose = katalog::walk_pose(model, look.anim, head_rel, pitch, swing, amount, haelt);
        if katalog::is_boat(name) {
            katalog::boat_paddle_pose(model, &mut pose, [e.rowing_time(0, self.tick_progress()), e.rowing_time(1, self.tick_progress())]);
        }
        if look.anim == Anim::Walker {
            apply_item_arm_poses(&mut pose, e.arm_items().0.is_some(), e.arm_items().1.is_some());
        }
        // Fehlt die Grundtextur, das schwarz-pinke Karo; die Ebenen darueber sind Beiwerk.
        for (i, tex) in texturen.iter().enumerate() {
            let rect = if i == 0 { Some(self.entity_atlas.rect_or_missing(tex)) } else { self.entity_atlas.rect(tex) };
            if let Some(rect) = rect {
                match skin {
                    Some(key) => self.push_skinned_body(model, &pose, base, rect, false, key),
                    None => world::emit_entity_model(model, &pose, base, rect, ENTITY_BRIGHTNESS, out),
                }
            }
        }
        // Truhe, Shulkerkiste und Statue in der Lore sind keine Blockmodelle: Vanilla zeichnet
        // sie ueber ihr Sondermodell (SubmitNodeCollection.submitBlock ->
        // SpecialBlockModelRenderer), wie das Item in der Hand. Alle anderen Bloecke laufen
        // ueber den Block-Atlas, s. display_block_vertices.
        if katalog::is_minecart(name) {
            let inhalt = minecart_content(e, name).and_then(|(state, ecke)| Some((world::block_name(state).and_then(crate::held_item::special_item)?, ecke)));
            if let Some(((art, textur), ecke)) = inhalt {
                let ecke = minecart_frame(feet, body_yaw).then(ecke);
                let rect = self.entity_atlas.rect_or_missing(&textur);
                match (art, skin) {
                    (crate::held_item::SpecialKind::Chest, Some(key)) => {
                        let em = world::entity_model::chest_model(world::ChestHalf::Single);
                        self.push_skinned_body(em, &world::entity_model::chest_pose(0.0), ecke, rect, false, key);
                    }
                    _ => crate::held_item::emit_special(art, rect, ecke, ENTITY_BRIGHTNESS, out),
                }
            }
        }
        self.emit_held_items(e.arm_items(), self.entity_blocking_arms(e), e.using_item_right_arm(), (0.0, true), (world::Affine::IDENTITY, world::Affine::IDENTITY), model, &pose, base, skin, out);
    }

    // Ein Armorstand (ArmorStandRenderer): der Koerper in seinen sechs Gelenkwinkeln, Ruestung
    // und Gehaltenes in denselben Winkeln. Unsichtbar faellt nur der Koerper weg -- Koepfe und
    // Ruestung auf unsichtbaren Staendern bleiben stehen, wie in Vanilla.
    pub(crate) fn emit_armor_stand(
        &self,
        e: &world::Entity,
        feet: [f32; 3],
        body_yaw: f32,
        // Per GPU-Skinning (s. skin_frame), mit dem Motion-Blur-Schluessel des Entities.
        skin: Option<u64>,
        out: &mut Vec<world::BlockVertex>,
        haut: &mut Vec<std::ops::Range<usize>>,
    ) {
        use protocol::packets::{ARMOR_STAND_FLAG_NO_BASEPLATE, ARMOR_STAND_FLAG_SHOW_ARMS, ARMOR_STAND_FLAG_SMALL};
        let flags = e.armor_stand_flags();
        let small = flags & ARMOR_STAND_FLAG_SMALL != 0;
        let [kopf, rumpf, linker_arm, rechter_arm, linkes_bein, rechtes_bein] = e.armor_stand_poses.map(|r| r.map(f32::to_radians));
        let mut pose = world::Pose::new();
        pose.rotate("head", kopf);
        // Die Staebe am Rumpf folgen dem Rumpf (ArmorStandModel.setupAnim).
        for teil in ["body", "right_body_stick", "left_body_stick", "shoulder_stick"] {
            pose.rotate(teil, rumpf);
        }
        pose.rotate("left_arm", linker_arm);
        pose.rotate("right_arm", rechter_arm);
        pose.rotate("left_leg", linkes_bein);
        pose.rotate("right_leg", rechtes_bein);
        // Die Bodenplatte dreht gegen den Stand und bleibt so am Blockraster ausgerichtet.
        pose.rotate("base_plate", [0.0, -body_yaw.to_radians(), 0.0]);
        let base = world::entity_transform(feet, body_yaw, e.scale, 1.0);
        let layer = if small { "armor_stand_small#main" } else { "armor_stand#main" };
        let textur = world::mob_catalog::mob_look("armor_stand").map(|look| self.entity_atlas.rect_or_missing(look.texture));
        if let (false, Some(model), Some(tex)) = (e.invisible, world::model_data::layer_model(layer), textur) {
            let mut koerper = pose.clone();
            let mut verstecken = |teil: &'static str| {
                let mut p = koerper.get(teil);
                p.hidden = true;
                koerper.set(teil, p);
            };
            if flags & ARMOR_STAND_FLAG_SHOW_ARMS == 0 {
                verstecken("left_arm");
                verstecken("right_arm");
            }
            if flags & ARMOR_STAND_FLAG_NO_BASEPLATE != 0 {
                verstecken("base_plate");
            }
            if let Some(key) = skin {
                self.push_skinned_body(model, &koerper, base, tex, false, key);
            } else {
                let start = out.len();
                world::emit_entity_model(model, &koerper, base, tex, ENTITY_BRIGHTNESS, out);
                haut.push(start..out.len());
            }
        }
        let models = world::entity_model::armor_stand_armor_models(small);
        self.emit_armor(&e.armor, models, &pose, base, out, None, skin);
        // Items sitzen auch ohne sichtbare Arme in der Hand (ArmorStandModel.translateToHand);
        // die Brust-Ebene hat beide Arme in den Winkeln des Standes.
        let arme = &models[world::armor::ArmorSlot::Chest.index()];
        self.emit_held_items(e.arm_items(), (false, false), None, (0.0, true), (world::Affine::IDENTITY, world::Affine::IDENTITY), arme, &pose, base, skin, out);
    }

    // Hinter allen Menues ohne Verbindung, nicht nur auf dem alten Startbildschirm: der Client
    // startet in der Serverliste, dort war das Bild sonst nie zu sehen. Der HUD-Editor hat
    // seinen eigenen Hintergrund (letzter Weltblick), der Todesbildschirm die Welt.
    pub(crate) fn wallpaper_visible(&self) -> bool {
        self.mode == AppMode::Menu
            && self.conn_status == ConnStatus::Disconnected
            && !matches!(self.screen, MenuScreen::HudEditor | MenuScreen::Death)
    }

    // HUD-Editor ohne Welt dahinter: der letzte Weltblick (s. last_view.rs) statt Schwarz.
    pub(crate) fn last_view_visible(&self) -> bool {
        self.mode == AppMode::Menu && self.screen == MenuScreen::HudEditor && self.conn_status != ConnStatus::Connected
    }

    // Beim Verlassen einer Welt das letzte Weltbild als letzten Weltblick behalten und speichern.
    // VOR reset_world, solange die Welt noch da ist.
    //
    // Dafuer wird die Welt eigens noch einmal gezeichnet, mit allem ausgeblendet wie bei F1 (kein
    // HUD, keine Hand, kein Blockrahmen). Vorher wurde nur gelesen, was gerade in der
    // Offscreen-Textur stand: beim Trennen aus dem Spiel heraus war das ein Bild MIT HUD und
    // wurde verworfen -- der Blick blieb dann der einer frueheren Sitzung.
    pub(crate) fn keep_last_view(&mut self) {
        let Some(bild) = self.capture_clean_view() else { return };
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_last_view(&bild);
        }
        // Zugleich der Blick dieses Servereintrags fuers naechste Verbinden -- wenn er auf
        // "Letzter Moment" steht (s. ServerEntry::view_last).
        if self.connect_view_last() {
            self.store_connect_view(&bild);
        }
        if let Some(dir) = shared_config_dir() {
            last_view::save(last_view::path(&dir), bild);
        }
    }

    // Steht der Servereintrag, mit dem gerade verbunden ist, auf "Letzter Moment"? Ohne Eintrag
    // (Quick Connect) nach der Adresse.
    pub(crate) fn connect_view_last(&self) -> bool {
        let adresse = self.server_address.trim();
        match self.servers.iter().find(|e| e.address.trim().eq_ignore_ascii_case(adresse)) {
            Some(e) => e.view_last.unwrap_or_else(|| server_view_last_default(&e.name, &e.address)),
            None => server_view_last_default("", adresse),
        }
    }

    // Ein Bild als Verbindungsbild dieses Servereintrags hochladen und speichern.
    pub(crate) fn store_connect_view(&mut self, bild: &wallpaper::Decoded) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        gpu.set_connect_view(bild);
        self.connect_view_for = Some(self.server_address.clone());
        self.connect_view_rx = None;
        if let Some(dir) = shared_config_dir() {
            last_view::save(last_view::server_path(&dir, &self.server_address), bild.clone());
        }
    }

    // "Erster Moment": eine Sekunde nach dem ersten Chunk (dann steht das Gelaende um einen
    // herum) einmal je Verbindung aufnehmen. Transfers zaehlen zur selben Verbindung.
    pub(crate) fn tick_connect_view_capture(&mut self) {
        if self.connect_view_taken || self.mode != AppMode::World || self.chunks_received == 0 || !self.camera_placed {
            return;
        }
        if self.connect_view_last() {
            return;
        }
        let seit = *self.connect_view_first_at.get_or_insert_with(Instant::now);
        if seit.elapsed() < CONNECT_VIEW_FIRST_DELAY {
            return;
        }
        self.connect_view_taken = true;
        if let Some(bild) = self.capture_clean_view() {
            self.store_connect_view(&bild);
        }
    }

    // Launcher-Hintergrundbild (s. hero_scene.rs): fehlt die Datei, die Szene ohne Verbindung in
    // die leere Welt laden, warten, bis alles gemesht ist, einmal ohne HUD zeichnen und
    // speichern. Verbindet man dazwischen, wird abgebrochen und beim naechsten Start neu versucht.
    pub(crate) fn tick_hero_scene(&mut self) {
        if self.hero_done {
            return;
        }
        let Some(seit) = self.hero_building else {
            // Erst wenn der Start durch ist: Packs geladen, keine Verbindung.
            if self.gpu.is_none()
                || self.conn_status != ConnStatus::Disconnected
                || self.pack_load_job.is_some()
                || self.app_started.elapsed() < HERO_START_DELAY
            {
                return;
            }
            if hero_scene::path().is_none() || hero_scene::is_current() {
                self.hero_done = true;
                return;
            }
            self.world.set_min_y(HERO_MIN_Y);
            let Some(szene) = hero_scene::load(HERO_MIN_Y, HERO_SECTIONS) else {
                self.hero_done = true;
                return;
            };
            self.apply_biome_registry(&szene.biomes);
            for (cx, cz, sections, licht) in szene.columns {
                self.world.insert_column(cx, cz, sections);
                self.world.apply_sky_light(cx, cz, &licht);
            }
            self.physics.pos = szene.pos;
            self.physics_render_prev = szene.pos;
            self.camera.yaw = szene.yaw;
            self.camera.pitch = szene.pitch;
            self.hero_fov = Some(szene.fov);
            self.hero_render_distance = Some(szene.render_distance);
            // Wie nach der ersten Position vom Server: erst dann waehlt enforce_render_limits die
            // Spalten aus, die gezeichnet werden -- ohne das blieb das Bild leer.
            self.camera_placed = true;
            self.world_time_of_day = szene.time_of_day;
            self.world_time_ticking = false;
            self.hero_building = Some(Instant::now());
            return;
        };
        if self.conn_status != ConnStatus::Disconnected {
            self.hero_building = None;
            self.hero_done = true;
            self.hero_fov = None;
            self.hero_render_distance = None;
            return;
        }
        let fertig = self.world.dirty_count() == 0 && self.mesh_jobs_pending.is_empty() && self.pack_load_job.is_none();
        if !fertig || seit.elapsed() < HERO_SETTLE {
            if seit.elapsed() > HERO_GIVE_UP {
                self.hero_building = None;
                self.hero_done = true;
                self.hero_fov = None;
                self.hero_render_distance = None;
                self.reset_world();
            }
            return;
        }
        self.hero_building = None;
        self.hero_done = true;
        // Ohne Verbindung fuehrt update_movement die Kamera nicht nach -- sie stuende sonst noch
        // dort, wo sie vor der Szene war, und saehe nur Himmel.
        self.camera.position = self.physics.pos + DVec3::new(0.0, f64::from(self.physics.eye_height()), 0.0);
        self.push_log(format!(
            "Launcher-Bild: {} Spalten gemesht, Kamera {:.1} {:.1} {:.1}",
            self.render_limit_meshed.len(),
            self.camera.position.x,
            self.camera.position.y,
            self.camera.position.z
        ));
        if let (Some(bild), Some(pfad)) = (self.capture_clean_view(), hero_scene::path()) {
            // Steht das Hintergrundbild auf "Coral", gleich zeigen -- die Datei schreibt ein
            // Thread erst noch.
            if wallpaper::WallpaperMode::from_index(self.settings.wallpaper) == wallpaper::WallpaperMode::Coral {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.set_wallpaper(Some(&bild));
                }
            }
            last_view::save(pfad, bild);
            if let Some(v) = hero_scene::version_path() {
                let _ = std::fs::write(v, hero_scene::VERSION);
            }
        }
        self.hero_fov = None;
        self.hero_render_distance = None;
        self.reset_world();
        self.world_time_ticking = true;
    }

    // Die Welt einmal ohne HUD und Hand zeichnen und auslesen (s. keep_last_view).
    pub(crate) fn capture_clean_view(&mut self) -> Option<wallpaper::Decoded> {
        // Ohne geladene Welt (Abbruch beim Verbinden) gibt es nichts Neues zu zeigen.
        if self.gpu.is_none() || self.world.column_keys().next().is_none() {
            return None;
        }
        let versteckt = self.hud_hidden;
        let modus = self.mode;
        self.hud_hidden = true;
        // Im Menue-Modus zeichnet build_hand_vertices die Hand nie (auch kein Totem).
        self.mode = AppMode::Menu;
        let _ = self.draw_world_frame(None, false);
        self.hud_hidden = versteckt;
        self.mode = modus;
        self.gpu.as_ref()?.read_world_view()
    }

    // Verbindungsbildschirm mit dem letzten Blick dieses Servereintrags statt der leeren Welt.
    pub(crate) fn connect_view_shown(&self) -> bool {
        self.connecting_view()
            && self.connect_view_for.as_deref() == Some(self.server_address.as_str())
            && self.gpu.as_ref().is_some_and(|g| g.connect_view.is_some())
    }

    // Beim Verbinden: den gespeicherten Blick dieses Servereintrags laden, falls nicht schon da.
    pub(crate) fn request_connect_view(&mut self) {
        if self.connect_view_for.as_deref() == Some(self.server_address.as_str()) {
            return;
        }
        self.connect_view_for = None;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.connect_view = None;
        }
        self.connect_view_rx =
            shared_config_dir().map(|dir| (self.server_address.clone(), last_view::load(last_view::server_path(&dir, &self.server_address))));
    }

    // Den gespeicherten letzten Weltblick einmal nach dem Start laden und hochladen.
    pub(crate) fn poll_last_view(&mut self) {
        if let Some((fuer, rx)) = &self.connect_view_rx {
            match rx.try_recv() {
                Ok(bild) => {
                    let fuer = fuer.clone();
                    self.connect_view_rx = None;
                    if let (Some(bild), Some(gpu)) = (bild, self.gpu.as_mut()) {
                        gpu.set_connect_view(&bild);
                        self.connect_view_for = Some(fuer);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.connect_view_rx = None,
            }
        }
        if !self.last_view_requested && self.gpu.is_some() {
            self.last_view_requested = true;
            self.last_view_rx = shared_config_dir().map(|dir| last_view::load(last_view::path(&dir)));
        }
        let Some(rx) = &self.last_view_rx else { return };
        match rx.try_recv() {
            Ok(bild) => {
                self.last_view_rx = None;
                // Schon ein frischerer da (Welt verlassen, waehrend noch geladen wurde)? Dann
                // bleibt der.
                if let (Some(bild), Some(gpu)) = (bild, self.gpu.as_mut()) {
                    if gpu.last_view.is_none() {
                        gpu.set_last_view(&bild);
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.last_view_rx = None,
        }
    }

    // Ein Weltbild zeichnen: Vertices bauen, hochladen, GpuState::render_world. Mit
    // `acquired` = None nur ins offscreen_view, ohne HUD -- das laufende Bild hinter dem
    // Pausemenue (s. world_backdrop_visible), statt eines eingefrorenen.
    // Shader-Pfad: Voxel-Abbild um die Kamera nachfuehren (s. render::voxel_volume). Legt den
    // Shader-Pfad beim ersten Mal an -- die Voxel-Klassen brauchen die Blockformen der App.
    // Weltaenderungen werden auch ohne Shader-Pfad abgeholt, sonst sammeln sie sich an.
    pub(crate) fn update_voxel_volume(&mut self) {
        let (cleared, columns) = self.world.take_voxel_changes();
        #[cfg(not(feature = "shader_path"))]
        let _ = (cleared, columns);
        #[cfg(feature = "shader_path")]
        self.update_voxel_volume_in(cleared, columns);
    }

    #[cfg(feature = "shader_path")]
    fn update_voxel_volume_in(&mut self, cleared: bool, columns: Vec<(i32, i32)>) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        if !crate::shader_path_on() {
            if let Some(sp) = gpu.shader_path.as_mut() {
                sp.voxels_mut().note_changes(cleared, &columns);
            }
            return;
        }
        let format = gpu.config.format;
        let (shapes, cross, uvs) = (&self.block_shapes, &self.cross_blocks, &self.block_uvs);
        let sp = gpu.shader_path.get_or_insert_with(|| {
            let atlas = gpu.block_renderer.atlas_texture().create_view(&wgpu::TextureViewDescriptor::default());
            let mut sp = render::shader_path::ShaderPath::new(&gpu.device, format, world::voxel::VoxelClasses::build(shapes, cross, uvs), atlas);
            // Das Licht-Raster im Hintergrund bauen: sonst ruckelt jeder Schritt ueber eine
            // Zellgrenze (s. VoxelVolume::set_async_lights).
            sp.voxels_mut().set_async_lights(true);
            sp
        });
        let voxels = sp.voxels_mut();
        voxels.note_changes(cleared, &columns);
        voxels.set_world_bottom(self.world.min_y());
        let eye = self.camera.position;
        let s = f64::from(world::SECTION_SIZE);
        let center = [(eye.x / s).floor() as i32, (eye.y / s).floor() as i32, (eye.z / s).floor() as i32];
        let welt = &self.world;
        voxels.update(
            &gpu.queue,
            center,
            |sx, sy, sz| {
                let column = welt.column(sx, sz)?;
                let i = sy - column.min_y.div_euclid(world::SECTION_SIZE);
                usize::try_from(i).ok().and_then(|i| column.sections.get(i))
            },
            crate::VOXEL_SECTIONS_PER_FRAME,
        );
        voxels.update_lights(&gpu.queue, [eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32]);
    }
}
