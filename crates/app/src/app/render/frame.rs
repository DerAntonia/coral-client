// Ein Welt-Frame, Hintergrundbild, Feuer-Overlay und Hurt-Cam.

use super::*;

impl App {
    pub(crate) fn draw_world_frame(&mut self, acquired: Option<(wgpu::SurfaceTexture, f32)>, with_hud: bool) -> Option<FrameTiming> {
        use profiler::Phase;
        let t = Instant::now();
        if with_hud {
            self.chat_selection_autoscroll();
        }
        let upload_ms = self.advance_atlas_animations();
        self.profiler.add(Phase::Animations, t);
        self.profiler.add_ms(Phase::Animations, -upload_ms);
        self.profiler.add_ms(Phase::AnimUpload, upload_ms);
        self.update_zoom();
        self.camera.fov_degrees = match self.hero_fov {
            Some(fov) => fov,
            None => zoomed_fov(self.settings.fov_of(self.perspective), self.settings.zoom_factor_tenths, self.zoom_amount),
        };
        self.camera.projection = self.settings.projection_mode;
        self.apply_replay_view();
        self.record_frame();
        let ui_started = Instant::now();
        // Ohne HUD (hinter einem Menue) zeichnet render_menu Benachrichtigungen und Zeiger.
        let (mut overlay, (hud_rects, hud_lines, hud_texts, hud_sprites, doll_split)) = if with_hud {
            self.tick_notifications();
            self.poll_resource_pack_job();
            let mut overlay = self.build_notifications();
            // Im Modul-Menue im Spiel gibt es ebenfalls Farbfelder und das Namensfenster.
            self.build_color_popup(&mut overlay);
            self.build_dropdown(&mut overlay);
            self.build_theme_name_dialog(&mut overlay);
            (overlay, self.build_hud())
        } else {
            (UiOverlay::default(), Default::default())
        };
        if with_hud {
            self.update_falling_glyphs(&hud_texts, &mut overlay);
        }
        self.profiler.add(Phase::Hud, ui_started);
        let t = Instant::now();
        // VOR dem Bauen der Vertices: neu aufgetauchte Items in den Atlas
        // holen (braucht &mut self und die GPU-Warteschlange, das Bauen
        // selbst nicht).
        self.ensure_held_item_textures();
        self.ensure_map_textures();
        self.ensure_trim_textures();
        self.scroll_wind_charge_texture();
        // Dasselbe fuer Skins, die aus dem Atlas verdraengt wurden, deren
        // Spieler aber wieder da ist. Kostet fast nichts, solange nichts
        // fehlt -- die Anfrage-Sperre laesst pro Verdraengung genau einen
        // Nachladeversuch durch (s. refresh_missing_skins).
        self.refresh_missing_skins();
        self.request_head_skins();
        self.profiler.add(Phase::Textures, t);
        let t = Instant::now();
        // Glint-Twins (s. glint_out): je Bau leeren und gleich danach abholen.
        self.glint_out.borrow_mut().clear();
        // Items in der Welt ueber den Core-Shader eines Resourcepacks, falls einer ihn ersetzt
        // (s. held_item::HeldItems::set_vanilla_route).
        let pack_items = self.gpu.as_ref().is_some_and(|g| g.block_renderer.vanilla.has_item_program());
        self.held_items.set_vanilla_route(pack_items);
        self.update_cull_frustum();
        let (entity_verts, entity_tints, entity_lights, entity_translucent, (outline_verts, outline_colors), entity_sharp, entity_ranges) = self.build_entity_vertices();
        self.upload_skinning();
        // Motion Blur: wo die Ecken der Entities im vorigen Bild lagen (s. entity_motion).
        let entity_prev = if self.settings.motion_blur_entities_percent > 0 {
            let lagen: Vec<[f32; 3]> = entity_verts.iter().map(|v| v.position).collect();
            Some(self.entity_motion.step(&lagen, &entity_ranges, self.camera.position))
        } else {
            self.entity_motion.reset();
            None
        };
        self.entity_vertex_hint = entity_verts.len();
        self.held_items.set_vanilla_route(false);
        let vanilla_items = self.vanilla_item_lights(self.held_items.take_vanilla_vertices());
        let entity_glint = self.glint_out.take();
        self.profiler.add(Phase::Entities, t);
        let t = Instant::now();
        let mut hitbox_verts = self.build_hitbox_vertices(self.gpu.as_ref().map_or(1080.0, |g| g.size().1));
        let selection_verts = self.build_block_selection_vertices();
        // Komplementaer: eigener Durchgang nach der Welt (s. render::OutlineComplement). Nur im
        // PvP-Pfad -- der Shader-Pfad zeichnet den Umriss wie bisher in seiner Farbe.
        let komplementaer = self.settings.module_block_outline
            && self.settings.block_outline_complementary
            && self.gpu.as_ref().is_some() && !crate::shader_path_on();
        let outline_complement = if komplementaer { selection_verts } else { hitbox_verts.extend(selection_verts); Vec::new() };
        self.profiler.add(Phase::Hitboxes, t);
        let t = Instant::now();
        let hand_verts = self.build_hand_vertices();
        // Shader-Pfad: die Hand im Licht an den Augen (s. render::BlockRenderer::hand_light).
        let hand_light = (crate::shader_path_on()).then(|| self.light_near(self.camera.position));
        let hand_glint = self.glint_out.take();
        self.profiler.add(Phase::Hand, t);
        // Fadenkreuz nur in der Ego-Ansicht -- in der 3. Person saesse es
        // in der Bildmitte, waehrend der Blickstrahl vom Spieler ausgeht.
        // Bei offenem Inventar auch kein Fadenkreuz -- man zielt gerade
        // nicht in die Welt, sondern sortiert Items.
        let wackeln = self.arrow_empty_shake_px() / self.gpu.as_ref().map_or(1920.0, |g| g.size().0);
        let crosshair_center = (with_hud && self.crosshair_visible())
            .then(|| self.shoulder_crosshair_pos().map_or((0.5 + wackeln, 0.5), |(x, y)| (x + wackeln, y)));
        // Feuer-Modus "Fadenkreuz": beim Brennen stehen Kreuz und Ring in der eigenen Farbe --
        // auch anstelle der Komplementaerfarbe. Die Zahlen daneben (Pfeile, Countdown) bleiben
        // in der normalen Fadenkreuz-Farbe (s. CrosshairExtras::fire_rgb). Nur wenn das Feuer
        // auch Schaden macht: nicht im Kreativ- oder Zuschauermodus, nicht mit Feuerresistenz.
        let brennt = self.settings.fire_mode == FIRE_MODE_CROSSHAIR
            && self.own_on_fire
            && !matches!(self.game_mode, GAME_MODE_CREATIVE | GAME_MODE_SPECTATOR)
            && !self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_FIRE_RESISTANCE);
        let crosshair_mode = self.settings.crosshair_color_mode as u32;
        let crosshair_rgb = self.settings.crosshair_rgb();
        let crosshair_outline = self.settings.crosshair_outline;
        let mut crosshair_extras = if with_hud { self.crosshair_extras() } else { CrosshairExtras::default() };
        crosshair_extras.fire_rgb = brennt.then(|| self.settings.fire_crosshair_rgb());
        // Nur der Komplementaer-Modus kann aufgehellt werden -- eine fest gewaehlte Farbe
        // ist ja bereits genau die gewaehlte Farbe.
        let crosshair_boost =
            if crosshair_mode == 0 { self.settings.crosshair_brightness_boost as f32 / 100.0 } else { 0.0 };
        // Bei offenem Inventar zeichnen wir den Zeiger selbst (die Maus ist
        // im Vollbild gefangen, es gibt keinen System-Cursor). Haengt gerade
        // ein Stapel an der Maus, ersetzt DER den Zeiger -- sonst laege die
        // Pfeilspitze mitten im Item.
        let inventory_cursor = (with_hud
            && (self.module_menu_open
                || self.hand_debug_scale().is_some()
                || self.chat_input.is_some()
                || self.connecting_view()
                || self.dialog_shown()
                || self.code_of_conduct.is_some()
                || (self.inventory.screen_open && self.inventory.cursor.is_none())))
            .then_some(self.cursor_pos)
            // Spielmodus-Rad: derselbe Zeiger, an der Stelle, die das Rad fuehrt.
            .or_else(|| self.gamemode_wheel_cursor().filter(|_| with_hud));
        // Geisterbloecke JEDEN Frame neu bauen. Ihre Vertices sind
        // augenrelativ (s. ghost_vertices) -- ein zwischengespeicherter Satz
        // behaelt den Versatz zur ALTEN Kameraposition und wandert damit beim
        // Gehen sichtbar mit. Es sind hoechstens ein paar Dutzend
        // unbestaetigte Platzierungen (s. GHOST_VERTEX_CAPACITY), der Neubau
        // kostet also nichts, was ein Zwischenspeicher wert waere.
        let t = Instant::now();
        self.step_ao_fades();
        let (ghost_verts, ghost_solid, ghost_translucent, ghost_ranges) = self.ghost_vertices();
        // Motion Blur der bewegten Bloecke darin (fallende Bloecke, TNT, Kolben ...), wie bei
        // den Entities nur ueber den deckenden Teil.
        let ghost_prev = if self.settings.motion_blur_entities_percent > 0 {
            let lagen: Vec<[f32; 3]> = ghost_verts[..ghost_solid].iter().map(|v| v.position).collect();
            Some(self.block_motion.step(&lagen, &ghost_ranges, self.camera.position))
        } else {
            self.block_motion.reset();
            None
        };
        // Shader-Pfad: Licht je Vertex fuer den deckenden Teil (s. BlockRenderer::set_ghost_vertices).
        let ghost_lights: Vec<u32> = if crate::shader_path_on() {
            let eye = self.camera.position;
            ghost_verts[..ghost_solid].iter().map(|v| self.light_near(eye + glam::Vec3::from(v.position).as_dvec3())).collect()
        } else {
            Vec::new()
        };
        let crumbling_verts = self.build_crumbling_vertices();
        let particle_draws = self.build_particle_vertices();
        let [regen, schnee] = self.build_weather_vertices();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.block_renderer.set_particle_vertices(&gpu.queue, &self.particle_vertices, particle_draws);
            gpu.block_renderer.set_particle_prev_positions(&gpu.queue, &self.particle_prev.positions, self.particle_prev.moving);
            // Texturen neu hochladen, wenn das Modul umschaltet (s. WeatherTexture::chosen).
            let modul = self.settings.module_weather;
            if self.weather_textures_uploaded != Some(modul) {
                // Als Teilchen-Blaetter: Kacheln plus schwere Kacheln, Regen grau (s. weather_sheet).
                let [r, s] = &self.weather_textures;
                let blatt = |t: Option<&assets::DecodedTexture>, grau: bool| {
                    let (rgba, w, h) = weather_texture_part(t);
                    precipitation::weather_sheet(rgba, w, h, grau)
                };
                let (regen, schnee) = (blatt(r.chosen(modul), true), blatt(s.chosen(modul), false));
                gpu.block_renderer.set_weather_textures(&gpu.device, &gpu.queue, sheet_part(regen.as_ref()), sheet_part(schnee.as_ref()));
                self.weather_layouts = [regen.as_ref().map_or_else(Default::default, |b| b.3), schnee.as_ref().map_or_else(Default::default, |b| b.3)];
                self.weather_textures_uploaded = Some(modul);
            }
            gpu.block_renderer.set_weather_vertices(&gpu.queue, &self.weather_vertices, regen, schnee);
        }
        self.profiler.add(Phase::WorldExtras, t);
        self.update_voxel_volume();
        self.update_chunk_saver();
        self.update_custom_sky();
        // Vor dem &mut-Ausleihen der GPU berechnen -- danach ist `self` nicht
        // mehr lesbar.
        let sky = self.sky_clear_color();
        let sky_module = self.sky_color_module_active();
        #[cfg(feature = "shader_path")]
        let atmosphere = !matches!(self.dimension_type_name.as_str(), "minecraft:the_nether" | "minecraft:the_end");
        let cursor_fx = self.cursor_fx_frame();
        // Ebenfalls vor dem &mut-Ausleihen der GPU: build_doll liest das
        // Inventar, den Skin und die Zeigerposition.
        let t = Instant::now();
        let doll = self.gpu.as_ref().filter(|_| with_hud).and_then(|gpu| self.build_doll(gpu.size()));
        let doll_glint = self.glint_out.take();
        self.profiler.add(Phase::Doll, t);
        let t_upload = Instant::now();
        // Die wandernde Glint-Matrix haengt an der Uhr, nicht am Tick.
        let glint_ms = self.start_time.elapsed().as_millis() as u64;
        let glint_speed = self.settings.glint_speed as f32 / 100.0;
        let glint_strength = self.settings.glint_strength as f32 / 100.0;
        // Die Weltgrenze in Sichtweite (Vanilla: die eingestellte Sichtweite in Bloecken).
        let border_draw = render::vanilla_draw::world_border_draw(&self.world.border, self.tick_progress(), self.camera.position, f64::from(self.settings.max_render_distance) * 16.0);
        let upload_ms = &mut 0.0f32;
        let hand_debug = self.hand_debug_scale();
        let (debug_flaechen, debug_texte) = match (hand_debug, self.gpu.as_ref()) {
            (Some(_), Some(gpu)) => self.build_hand_debug_ui(gpu),
            _ => (Vec::new(), Vec::new()),
        };
        let timing = self.gpu
            .as_mut()
            .and_then(|gpu| {
                gpu.block_renderer.set_ghost_vertices(&gpu.queue, &ghost_verts, ghost_solid, ghost_translucent, &ghost_lights);
                match &ghost_prev {
                    Some((vorher, bewegt)) => gpu.block_renderer.set_ghost_prev_positions(&gpu.queue, vorher, *bewegt),
                    None => gpu.block_renderer.set_ghost_prev_positions(&gpu.queue, &[], false),
                }
                gpu.block_renderer.set_crumbling_vertices(&gpu.queue, &crumbling_verts);
                gpu.block_renderer.set_entity_vertices(&gpu.queue, &entity_verts, &entity_tints, &entity_lights, entity_translucent);
                match &entity_prev {
                    Some((vorher, bewegt)) => gpu.block_renderer.set_entity_prev_positions(&gpu.queue, vorher, *bewegt),
                    None => gpu.block_renderer.set_entity_prev_positions(&gpu.queue, &[], false),
                }
                gpu.block_renderer.set_sharp_entities(entity_sharp);
                gpu.block_renderer.set_outline_vertices(&gpu.queue, &outline_verts, &outline_colors);
                gpu.block_renderer.vanilla.set_item_vertices(&gpu.device, &gpu.queue, &vanilla_items);
                gpu.block_renderer.vanilla.set_border(&gpu.queue, self.camera.view_matrix(), glint_ms, border_draw.as_ref());
                gpu.block_renderer.vanilla_game_time = self.world_time_of_day.rem_euclid(24000) as f32 / 24000.0;
                gpu.block_renderer.set_entity_glint_vertices(&gpu.queue, &entity_glint);
                gpu.block_renderer.update_glint(&gpu.queue, glint_ms, glint_speed, glint_strength);
                gpu.update_gui_glint(glint_ms, glint_speed, glint_strength);
                // Linienbreite + Bildschirmgroesse: die Verbreiterung passiert
                // im Vertex-Shader, nicht in den Vertices (s. vs_hitbox).
                gpu.block_renderer.set_hitbox_params(gpu.size());
                gpu.block_renderer.set_hitbox_vertices(&gpu.queue, &hitbox_verts);
                gpu.block_renderer.set_outline_complement_vertices(&gpu.queue, &outline_complement);
                gpu.block_renderer.set_hand_vertices(&gpu.queue, &hand_verts);
                gpu.block_renderer.hand_light = hand_light;
                gpu.block_renderer.set_hand_glint_vertices(&gpu.queue, &hand_glint);
                // Spielerfigur im Inventar: Geometrie UND Kamera stehen erst
                // hier fest (beides haengt an der Fenstergroesse), und die
                // leere Liste raeumt sie wieder weg, sobald das Inventar zu
                // ist -- sonst haenge sie im naechsten Frame noch im Puffer.
                if let Some((verts, view, proj)) = &doll {
                    gpu.block_renderer.update_doll_camera(&gpu.queue, *view, *proj);
                    gpu.block_renderer.set_doll_vertices(&gpu.queue, verts);
                    gpu.block_renderer.set_doll_glint_vertices(&gpu.queue, &doll_glint);
                } else {
                    gpu.block_renderer.set_doll_vertices(&gpu.queue, &[]);
                    gpu.block_renderer.set_doll_glint_vertices(&gpu.queue, &[]);
                }
                *upload_ms = t_upload.elapsed().as_secs_f32() * 1000.0;
                // Tageslicht fuer den Shader-Pfad (Himmelslicht nachts gedimmt, s. block.wgsl).
                // Mit festem Himmel (Modul Himmelsfarbe) gilt auch das Licht fest: die
                // Helligkeit der gewaehlten Farbe -- sonst stuende nachts ein heller Himmel
                // ueber einer dunklen Welt.
                gpu.block_renderer.daylight = if sky_module {
                    self.settings.module_sky_color_value as f32 / 100.0
                } else {
                    world::sky::daylight_factor(self.world_time_of_day)
                };
                gpu.sky_angle = world::sky::sky_angle(self.world_time_of_day);
                #[cfg(feature = "shader_path")]
                {
                    gpu.atmosphere = atmosphere;
                }
                gpu.motion_blur_strength = self.settings.motion_blur_percent as f32 / 100.0;
                gpu.motion_blur_entity_strength = self.settings.motion_blur_entities_percent as f32 / 100.0;
                gpu.motion_blur_interval_s = self.monitor_frame_interval.as_secs_f32();
                gpu.hand_blur_strength = self.settings.hand_blur_percent as f32 / 100.0;
                gpu.hand_blur_curve = render::hand_blur::Curve::from_index(self.settings.hand_blur_curve);
                gpu.render_world(
                    &self.camera,
                    sky,
                    &hud_rects,
                    &hud_lines,
                    &hud_texts,
                    &hud_sprites,
                    &overlay,
                    inventory_cursor,
                    cursor_fx,
                    crosshair_center,
                    crosshair_mode,
                    crosshair_rgb,
                    crosshair_boost,
                    crosshair_outline,
                    crosshair_extras,
                    // CMAA2 deaktiviert (s. CMAA2_ROW-Kommentar), unabhaengig
                    // vom gespeicherten Settings-Wert -- die Umschalt-UI ist
                    // ausgehaengt, ein alter true-Wert aus settings.conf soll
                    // trotzdem nicht mehr greifen.
                    false,
                    self.settings.color_saturation(),
                    doll_split,
                    hand_debug,
                    (&debug_flaechen, &debug_texte),
                    acquired,
                )
            });
        self.profiler.add_ms(Phase::Upload, *upload_ms);
        timing
    }

    // Menues ueber der Welt (Pause, Einstellungen, HUD-Editor, ...) zeigen das letzte
    // Weltbild statt Schwarz, solange man verbunden ist -- wie in Vanilla.
    pub(crate) fn world_backdrop_visible(&self) -> bool {
        self.mode == AppMode::Menu && self.conn_status == ConnStatus::Connected
    }

    // Die eingestellte Quelle (neu) laden, s. wallpaper::Loader::request. Verkleinert wird
    // auf den Monitor, nicht aufs Fenster: sonst waere das Bild nach dem Maximieren unscharf.
    pub(crate) fn request_wallpaper(&mut self, force: bool) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let ziel = gpu.window.current_monitor().map_or((gpu.config.width, gpu.config.height), |m| (m.size().width, m.size().height));
        let modus = wallpaper::WallpaperMode::from_index(self.settings.wallpaper);
        let gespeichert = shared_config_dir().map(|d| wallpaper::stored_path(&d));
        self.wallpaper_checked_at = Instant::now();
        self.wallpaper_loader.request(modus, gespeichert, hero_scene::path(), ziel, force);
    }

    // Einmal pro Durchlauf: fertige Bilder hochladen, Dialog-Ergebnisse umsetzen, und beim
    // Desktop-Hintergrund ab und zu nachsehen, ob er gewechselt wurde.
    pub(crate) fn poll_wallpaper(&mut self) {
        for event in self.wallpaper_loader.poll() {
            match event {
                wallpaper::Event::Loaded(bild) => {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.set_wallpaper(Some(&bild));
                    }
                }
                wallpaper::Event::Cleared => {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.set_wallpaper(None);
                    }
                }
                wallpaper::Event::Failed(e) => {
                    self.push_log(format!("Hintergrundbild: {e}"));
                    self.notify(notifications::Notification::warning("Hintergrundbild nicht geladen", e));
                }
                wallpaper::Event::Imported => {
                    self.settings.wallpaper = wallpaper::WallpaperMode::File.index();
                    self.persist_settings();
                    self.request_wallpaper(true);
                }
                wallpaper::Event::ImportFailed(e) => {
                    self.push_log(format!("Hintergrundbild: {e}"));
                    self.notify(notifications::Notification::error("Bild nicht übernommen", e));
                }
                wallpaper::Event::PickCancelled => {}
            }
        }
        if wallpaper::WallpaperMode::from_index(self.settings.wallpaper) == wallpaper::WallpaperMode::Desktop
            && self.wallpaper_visible()
            && self.wallpaper_checked_at.elapsed() >= WALLPAPER_DESKTOP_RECHECK
        {
            self.request_wallpaper(false);
        }
    }

    // Feuer vor den Augen, Modus Vanilla (ScreenEffectRenderer.submitFire): zwei Bilder der
    // Feuertextur in den unteren Ecken, je eine Einheit gross, eine halbe Einheit vor dem Auge,
    // 0,24 zur Seite, 0,3 nach unten und um 10 Grad zur Mitte gedreht -- gesehen mit 70 Grad
    // Sichtfeld wie die Hand. "Hoehe" schiebt beide nach unten, bei 0 % ganz aus dem Bild.
    // In Streifen zerlegt, weil die Sprites ihre UVs linear ueber zwei Dreiecke legen und das
    // schraeg stehende Bild sonst einen Knick in der Diagonale bekaeme.
    pub(crate) fn build_fire_overlay(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>) {
        if self.mode != AppMode::World
            || !self.own_on_fire
            || self.game_mode == GAME_MODE_SPECTATOR
            || !self.perspective.is_first_person()
            || self.settings.fire_mode != FIRE_MODE_VANILLA
            || self.settings.fire_height == 0
        {
            return;
        }
        let Some(feuer) = self.ui_atlas.rect(&format!("tex/{}", assets::FIRE_OVERLAY)) else { return };
        let (sw, sh) = gpu.size();
        let tan = 35f32.to_radians().tan();
        let aspect = sw / sh.max(1.0);
        let senken = (1.0 - self.settings.fire_height as f32 / 100.0) * 0.55;
        let [u0, v0, u1, v1] = feuer.uv;
        const STREIFEN: usize = 8;
        for (seite, winkel) in [(0.24f32, -10f32), (-0.24, 10.0)] {
            let (sin, cos) = winkel.to_radians().sin_cos();
            let bild = |lx: f32, ly: f32| {
                let (x, z) = (lx * cos - 0.5 * sin + seite, -lx * sin - 0.5 * cos);
                let y = ly - 0.3 - senken;
                let (nx, ny) = (x / (-z * tan * aspect), y / (-z * tan));
                [f32::midpoint(nx, 1.0) * sw, (1.0 - ny) / 2.0 * sh]
            };
            for i in 0..STREIFEN {
                let (a, b) = (i as f32 / STREIFEN as f32, (i + 1) as f32 / STREIFEN as f32);
                let (xa, xb) = (a - 0.5, b - 0.5);
                // Links im Bild liegt das rechte Ende der Textur (gespiegelt wie in Vanilla).
                let (ua, ub) = (u1 + (u0 - u1) * a, u1 + (u0 - u1) * b);
                sprites.push(UiSprite {
                    corners: [bild(xa, 0.5), bild(xb, 0.5), bild(xb, -0.5), bild(xa, -0.5)],
                    uv: [ua, v0, ub, v1],
                    color: [1.0, 1.0, 1.0, 0.9],
                    atlas: SpriteAtlas::Ui,
                });
            }
        }
    }

    // Hurt-Cam: Vanillas Verlauf (GameRenderer.bobHurt) -- schnell hin und weich zurueck ueber
    // die zehn Ticks nach dem Treffer --, aber als reines Rollen um die Blickachse und mit
    // jedem Treffer zur anderen Seite.
    pub(crate) fn hurt_cam_roll(&self) -> f32 {
        if !self.settings.hurt_cam || self.own_hurt_ticks == 0 || self.game_mode == GAME_MODE_SPECTATOR {
            return 0.0;
        }
        let t = ((f32::from(self.own_hurt_ticks) - self.tick_progress()) / f32::from(world::MAX_HURT_TIME)).clamp(0.0, 1.0);
        let verlauf = (t.powi(4) * std::f32::consts::PI).sin();
        self.hurt_cam_side * verlauf * HURT_CAM_MAX_DEGREES * self.settings.hurt_cam_strength as f32 / 100.0
    }
}

// Bild eines Wetterblatts fuer set_weather_textures; ohne Blatt ein durchsichtiger Pixel.
fn sheet_part(b: Option<&(Vec<u8>, u32, u32, precipitation::SheetLayout)>) -> (&[u8], u32, u32) {
    b.map_or((&[0u8, 0, 0, 0][..], 1, 1), |b| (&b.0[..], b.1, b.2))
}
