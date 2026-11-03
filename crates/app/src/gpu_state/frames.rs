// Frame holen und zeigen, Menue- und Welt-Frame.

use super::*;

impl GpuState {
    // Holt das naechste Swapchain-Bild -- der Punkt, an dem Fifo auf den VBlank wartet.
    // Liefert das Bild und die Wartezeit in ms (s. FrameTiming).
    //
    // Wird aufgerufen, BEVOR der Frame gebaut wird, nicht erst beim Zeichnen: erst nach dem
    // Warten ist der Abstand bis zur Anzeige fest (ein VBlank). Die Uhrzeit, aus der Kamera,
    // Entities und alles andere Bewegte interpoliert werden, muss DANACH gelesen werden.
    // Vorher stand das Warten am Ende -- zwischen Zeitmessung und Anzeige lagen dann Meshing,
    // Pakete und Vertex-Aufbau, die je Frame unterschiedlich lange dauern, waehrend das Bild
    // trotzdem immer am VBlank erschien. Jedes Bild zeigte so einen leicht falschen
    // Zeitpunkt: im Spiel sichtbares Ruckeln (sichtbar im Tearing-Test, im Menue ohne
    // diese Arbeit kaum zu sehen).
    pub(crate) fn acquire_frame(&mut self) -> Option<(wgpu::SurfaceTexture, f32)> {
        // get_current_texture() ist der Punkt, an dem Fifo tatsaechlich wartet (s.
        // render_menu). Zeitmessung VOR der Zero-Modus-Fence-Wait, sonst waere deren
        // Wartezeit im HUD unsichtbar.
        let acquire_started = Instant::now();
        self.wait_for_previous_frame_if_zero_mode();
        // Ohne eingerichtete Swapchain wuerde get_current_texture in wgpu panicken.
        if !self.surface_ok && (Instant::now() < self.surface_retry_at || !self.configure_surface()) {
            return None;
        }
        if let Ok(frame) = self.surface.get_current_texture() { Some((frame, acquire_started.elapsed().as_secs_f32() * 1000.0)) } else {
            self.configure_surface();
            None
        }
    }

    // Gemeinsames Ende von render_menu/render_world, mit getrennter Zeitmessung.
    pub(crate) fn submit_and_present(
        &mut self,
        mut encoder: wgpu::CommandEncoder,
        frame: wgpu::SurfaceTexture,
        wait_ms: f32,
        work_started: Instant,
    ) -> FrameTiming {
        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.end_frame(&mut encoder);
        }
        let screenshot = std::mem::take(&mut self.screenshot_requested)
            .then(|| screenshot::copy(&self.device, &mut encoder, &frame.texture, self.config.format));
        let encode_ms = work_started.elapsed().as_secs_f32() * 1000.0;
        let t = Instant::now();
        self.last_submission = Some(self.queue.submit(Some(encoder.finish())));
        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.after_submit();
        }
        let submit_ms = t.elapsed().as_secs_f32() * 1000.0;
        // Riss-Steuerung: erst die GPU fertig werden lassen, dann bis auf den berechneten
        // Zeitpunkt warten und DANN praesentieren (s. App::tear_slot). Der Bildwechsel haengt so
        // nicht mehr an der schwankenden Frame-Arbeit, nur noch an diesem einen Zeitpunkt.
        let mut ready_ns = None;
        let mut idle_ms = wait_ms;
        if let (Some(at), Some(index)) = (self.present_at_ns.take(), &self.last_submission) {
            let t = Instant::now();
            self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(index.clone()));
            let ready = vblank::monotonic_ns();
            ready_ns = Some(ready);
            wait_until_ns(at);
            idle_ms += t.elapsed().as_secs_f32() * 1000.0;
        }
        let t = Instant::now();
        let present_ns = vblank::monotonic_ns();
        frame.present();
        #[cfg(windows)]
        vblank::note_present();
        let present_ms = t.elapsed().as_secs_f32() * 1000.0;
        if let Some(pending) = screenshot {
            screenshot::finish(&self.device, pending);
        }
        // Present ist Warten auf Treiber und Compositor, keine eigene Arbeit -- wie das Warten
        // in get_current_texture(). Haelt das System den Bildschirm an (Screenshot-Werkzeug),
        // stand das sonst als Arbeitsspitze in der Leistungsanzeige (25 statt 2,5 ms).
        idle_ms += present_ms;
        let gpu_done_ns = match (ready_ns, &self.last_submission, self.measure_gpu_done) {
            // GPU war schon vor dem Present fertig: der Wechsel folgt dem Present-Aufruf.
            (Some(_), _, _) => Some(present_ns),
            (None, Some(index), true) => {
                let t = Instant::now();
                self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(index.clone()));
                idle_ms += t.elapsed().as_secs_f32() * 1000.0;
                Some(vblank::monotonic_ns())
            }
            _ => None,
        };
        FrameTiming { idle_ms, encode_ms, submit_ms, present_ms, present_ns, gpu_done_ns, ready_ns }
    }

    pub(crate) fn render_menu(
        &mut self,
        rects: &[UiRect],
        texts: &[UiText],
        sprites: &[UiSprite],
        overlay: &UiOverlay,
        cursor: Option<(f32, f32)>,
        cursor_fx: CursorFxFrame,
        // Hintergrundbild zeichnen (nur auf dem Startbildschirm, s. App::wallpaper_visible).
        show_wallpaper: bool,
        // Das Weltbild dahinter mit dieser Helligkeit (verbunden, s. App::world_backdrop_visible),
        // None = keins.
        show_world: Option<f32>,
        // Den letzten Weltblick zeichnen (HUD-Editor ohne Verbindung, s. App::last_view_visible).
        show_last_view: bool,
        // Den letzten Blick des Servereintrags mit dieser Helligkeit (Verbindungsbildschirm, s.
        // App::connect_view_shown), None = keinen.
        show_connect_view: Option<f32>,
        // Aus acquire_frame -- das Bild wird VOR dem Aufbau des Frames geholt, s. dort.
        acquired: (wgpu::SurfaceTexture, f32),
    ) -> Option<FrameTiming> {
        // get_current_texture() ist der Punkt, an dem Fifo tatsaechlich wartet: die
        // Funktion blockiert, bis ein Swapchain-Bild frei wird -- bei Fifo also bis zum
        // naechsten VBlank. Getrennt gemessen, sonst zeigt das HUD bei Fifo fast nur
        // Wartezeit als vermeintliche "Arbeit" an (vgl. osu!lazer:
        // exakt die Zeit, die osu.Framework per WaitUntilIdle() erzwingt).
        // Zeitmessung faengt VOR der Zero-Modus-Fence-Wait an (s. Feldkommentar), nicht erst
        // danach -- sonst waere die (bei aktivem Zero-Modus oft dominante) Wartezeit auf das
        // vorherige Frame im HUD unsichtbar und man koennte nicht nachpruefen, dass da
        // ueberhaupt echt gewartet wird.
        let (frame, wait_ms) = acquired;
        let work_started = Instant::now();
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let screen_px = self.size();

        let mut quad_data: Vec<Vertex> = Vec::with_capacity(rects.len() * VERTS_PER_QUAD);
        let mut quad_clips: Vec<(u32, bool)> = Vec::new();
        let mut fade = None;
        for r in rects.iter().take(MAX_UI_RECTS) {
            if let Some(an) = clip_marker_rect(r) {
                quad_clips.push((quad_data.len() as u32, an));
                continue;
            }
            if let Some(f) = fade_marker_rect(r) {
                fade = f;
                continue;
            }
            match fade {
                Some(f) => fade_quad_verts(&rect_verts(r, screen_px), f, screen_px.1, &mut quad_data),
                None => quad_data.extend_from_slice(&rect_verts(r, screen_px)),
            }
        }
        // Geteilte Flaechen (s. fade_quad_verts) brauchen mehr Ecken: nie ueber den Puffer.
        quad_data.truncate(MAX_UI_RECTS * VERTS_PER_QUAD);
        let ov = self.overlay_vertices(overlay, screen_px);
        let quad_split = append_overlay(&mut quad_data, &ov.quads, MAX_UI_RECTS * VERTS_PER_QUAD);
        if !quad_data.is_empty() {
            self.queue.write_buffer(&self.quad_vertex_buffer, 0, bytemuck::cast_slice(&quad_data));
        }

        let mut text_data: Vec<TextVertex> = Vec::new();
        let schriften = self.shape_text(texts, screen_px, &mut text_data);
        let text_split = append_overlay(&mut text_data, &ov.text, MAX_UI_GLYPHS * VERTS_PER_QUAD);
        if !text_data.is_empty() {
            self.queue.write_buffer(&self.text_vertex_buffer, 0, bytemuck::cast_slice(&text_data));
        }

        // Texturierte Quads -- im Menue bisher nur die Hotbar-Vorschau des HUD-Editors.
        let mut sprite_data: Vec<TextVertex> = Vec::new();
        // Das Hintergrundbild als erstes Quad im selben Puffer: es wird VOR den Flaechen
        // gezeichnet (s. unten), der Rest des Puffers wie gehabt danach.
        // Verbunden: stattdessen das letzte Weltbild, abgedunkelt wie Vanillas Pausemenue.
        let hintergrund: Option<(&wgpu::BindGroup, [f32; 4], [f32; 4])> = match (self.wallpaper.as_ref().filter(|_| show_wallpaper), self.world_backdrop.as_ref().zip(show_world)) {
            (Some((bind_group, groesse)), _) => Some((bind_group, wallpaper::cover_uv(*groesse, screen_px), [1.0; 4])),
            (None, Some((bind_group, hell))) => Some((bind_group, [0.0, 0.0, 1.0, 1.0], [hell, hell, hell, 1.0])),
            _ if show_connect_view.is_some() && self.connect_view.is_some() => {
                let hell = show_connect_view.unwrap_or(1.0);
                self.connect_view.as_ref().map(|(bind_group, groesse)| (bind_group, wallpaper::cover_uv(*groesse, screen_px), [hell, hell, hell, 1.0]))
            }
            // Unabgedunkelt: im HUD-Editor soll man sehen, wie das HUD im Spiel wirkt.
            _ => self
                .last_view
                .as_ref()
                .filter(|_| show_last_view)
                .map(|(bind_group, groesse)| (bind_group, wallpaper::cover_uv(*groesse, screen_px), [1.0; 4])),
        };
        if let Some((_, [u0, v0, u1, v1], farbe)) = hintergrund {
            let (w, h) = screen_px;
            let ecken = [([0.0, 0.0], [u0, v0]), ([w, 0.0], [u1, v0]), ([w, h], [u1, v1]), ([0.0, h], [u0, v1])];
            let vertex = |i: usize| TextVertex {
                position: pixel_to_ndc(ecken[i].0[0], ecken[i].0[1], screen_px),
                uv: ecken[i].1,
                color: farbe,
                bounds: [0.0; 4],
                mode: [0.0; 4],
            };
            sprite_data.extend_from_slice(&[vertex(0), vertex(1), vertex(2), vertex(0), vertex(2), vertex(3)]);
        }
        let sprite_runs = Self::shape_sprites(sprites, screen_px, &mut sprite_data);
        let sprite_split = append_overlay_sprites(&mut sprite_data, &ov.sprites);
        if !sprite_data.is_empty() {
            self.queue.write_buffer(&self.sprite_vertex_buffer, 0, bytemuck::cast_slice(&sprite_data));
        }

        // Eigenes Uniform statt Dreiecke im quad_vertex_buffer: der Zeiger wird im Shader
        // kantengeglaettet (s. cursor.wgsl) und muss ausserdem NACH dem Text gezeichnet
        // werden (dritter Durchgang unten) -- der quad_vertex_buffer ist zu dem Zeitpunkt
        // schon an den ersten Draw-Call gebunden, ein zweites Beschreiben wuerde dessen
        // Inhalt fuer den GANZEN Frame veraendern, nicht nur ab hier.
        if let Some(pos) = cursor {
            self.queue.write_buffer(&self.cursor_uniform_buffer, 0, bytemuck::bytes_of(&cursor_polygons(pos, screen_px, cursor_fx)));
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("menu encoder") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("menu pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if let Some((bind_group, _, _)) = hintergrund {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_vertex_buffer(0, self.sprite_vertex_buffer.slice(..));
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..VERTS_PER_QUAD as u32, 0..1);
            }
            if quad_split > 0 {
                pass.set_pipeline(&self.quad_pipeline);
                pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
                draw_clipped(&mut pass, 0..quad_split, &quad_clips, screen_px, |pass, r| pass.draw(r, 0..1));
            }
            if !sprite_runs.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_vertex_buffer(0, self.sprite_vertex_buffer.slice(..));
                self.draw_sprite_runs(&mut pass, &sprite_runs);
            }
            // Text, Overlay-Ebene und -- als dritter, letzter Durchgang -- der Cursor: er
            // gehoert IMMER nach vorn, unabhaengig davon, ob eine Flaeche oder Text darunter
            // liegt. Ohne Tiefentest in diesem Pass entscheidet allein die Zeichenreihenfolge.
            self.draw_ui_top(
                &mut pass,
                (text_split, text_data.len() as u32),
                (quad_split, quad_data.len() as u32),
                &ov.ranges,
                (sprite_split, sprite_data.len() as u32),
                cursor.is_some(),
                &schriften,
            );
        }
        Some(self.submit_and_present(encoder, frame, wait_ms, work_started))
    }

    // `crosshair_center` = normierte Position (0..1, Bildmitte samt Wackeln),
    // `crosshair_mode` = Index in CROSSHAIR_COLOR_MODES.

    // Welt im Shader-Pfad (s. render::shader_path): G-Buffer, Licht, Tonemap, dann der
    // Forward-Rest ins fertige Bild. Am Ende steht die Welt wie im PvP-Pfad in offscreen_view
    // und der Tiefenpuffer ist verbraucht -- Hand, HUD und Ausgabe laufen danach unveraendert.
    // Der Shader-Pfad samt Voxel-Abbild entsteht vorher in App::update_voxel_volume.
    pub(crate) fn render_world_shader_path(&mut self, encoder: &mut wgpu::CommandEncoder, sky: wgpu::Color, camera: &render::Camera) {
        let (width, height) = (self.config.width, self.config.height);
        let Some(sp) = self.shader_path.as_mut() else { return };
        sp.ensure_size(&self.device, width, height);
        {
            let color = sp.gbuffer_attachments();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gbuffer pass"),
                color_attachments: &color,
                depth_stencil_attachment: Some(sp.depth_attachment_keep()),
                timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("G-Buffer")),
                occlusion_query_set: None,
            });
            let timer = &mut self.gpu_timer;
            self.block_renderer.render_gbuffer(&mut pass, &mut |pass, label| {
                if let Some(t) = timer.as_mut() {
                    t.mark(pass, label);
                }
            });
        }
        let frame = render::shader_path::LightFrame {
            sky,
            daylight: self.block_renderer.daylight,
            view: camera.view_matrix(),
            proj: camera.proj_matrix(width as f32 / height.max(1) as f32),
            eye: camera.position,
            celestial: render::shader_path::celestial_light(self.sky_angle),
            shadows: true,
            atmosphere: self.atmosphere,
            scattering: self.scattering,
            sun_size_deg: self.sun_size_deg,
            block_shadows: self.block_shadows,
            block_softness_deg: self.block_softness_deg,
            lightmap_debug: self.lightmap_debug,
            sun_factor: world::light::stored_light_factor(world::light::SUNLIGHT),
        };
        sp.prepare(&self.queue, &frame);
        let set = self.gpu_timer.as_ref().map(gpu_timer::GpuTimer::query_set);
        let timer = &mut self.gpu_timer;
        sp.shadows(encoder, &mut |label| {
            let (begin, end) = timer.as_mut()?.reserve(label)?;
            Some((set.as_deref()?, begin, end))
        });
        sp.light(encoder, self.gpu_timer.as_mut().and_then(|t| t.pass("Licht")));
        sp.tonemap(&self.queue, encoder, &self.offscreen_view, 1.0, self.gpu_timer.as_mut().and_then(|t| t.pass("Tonemap")));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("forward rest pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.offscreen_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(sp.depth_attachment_load()),
            timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Welt")),
            occlusion_query_set: None,
        });
        let timer = &mut self.gpu_timer;
        self.block_renderer.render_forward_rest(&mut pass, &mut |pass, label| {
            if let Some(t) = timer.as_mut() {
                t.mark(pass, label);
            }
        });
    }

    pub(crate) fn render_world(
        &mut self,
        camera: &render::Camera,
        // Die Himmelsfarbe dieses Frames, LINEAR (s. App::sky_clear_color). Frueher stand hier
        // eine Konstante -- damit war der Himmel Tag und Nacht derselbe.
        sky: wgpu::Color,
        rects: &[UiRect],
        hud_lines: &[UiLine],
        texts: &[UiText],
        // Texturierte Quads (Hotbar-/Inventar-Sheets, Item-Icons) -- gezeichnet nach den
        // farbigen Flaechen und vor dem Text, damit Stackgroessen darueber liegen.
        sprites: &[UiSprite],
        // Die oberste Ebene (Benachrichtigungen), s. UiOverlay.
        overlay: &UiOverlay,
        // Selbstgezeichneter Mauszeiger -- im Spiel nur bei offenem Inventar (sonst steuert
        // die Maus die Blickrichtung und es gibt keinen Zeiger).
        cursor: Option<(f32, f32)>,
        // Die Zeiger-Effekte dieses Frames (s. cursor_fx).
        cursor_fx: CursorFxFrame,
        // None = kein Fadenkreuz zeichnen (3.-Person-Perspektiven, s. Perspective) -- dort
        // zeigt es nicht mehr dorthin, wo man tatsaechlich hinzielt, weil die Kamera neben
        // dem Auge sitzt. Ein Fadenkreuz, das die falsche Stelle markiert, ist schlechter
        // als gar keines.
        crosshair_center: Option<(f32, f32)>,
        crosshair_mode: u32,
        // Feste Farbe im Modus Einfarbig (s. Settings::crosshair_rgb).
        crosshair_rgb: [f32; 3],
        // 0..1 -- Beimischung von Weiss, nur im Komplementaer-Modus wirksam (s. Shader).
        crosshair_boost: f32,
        crosshair_outline: bool,
        // Was der Fadenkreuz-Pass sonst noch zeichnet: HUD-Zahlen (Schild-Countdown, Pfeile)
        // und den Schild-Kreis, s. App::crosshair_extras. Wird auch ohne Fadenkreuz gezeichnet.
        crosshair_extras: CrosshairExtras,
        // CMAA2 (s. cmaa2.wgsl-Kommentar). Bei false laeuft der alte, direkte Kopierweg
        // unveraendert weiter.
        cmaa2_enabled: bool,
        // Modul "Farbe": Saettigung (1.0 = unveraendert), None = Modul aus bzw. 100 % -- dann
        // bleibt es bei der blossen Kopie.
        saturation: Option<f32>,
        // Vor welchem Sprite die Spielerfigur im Inventar gezeichnet wird (s.
        // App::build_inventory_screen). None = keine Figur in diesem Frame.
        doll_split: Option<usize>,
        // Hand-Debug (s. Settings::module_hand_debug): das fertige Bild verkleinert auf diesen
        // Anteil links oben, ringsum schwarz; die Hand kommt erst danach und ungeschnitten
        // darueber (ihre Vertices sind schon passend verkleinert, s. build_hand_vertices).
        hand_debug: Option<f32>,
        // Hand-Debug: Knoepfe und Werte im Schwarzen (s. draw_hand_debug_ui).
        hand_debug_ui: (&[UiRect], &[UiText]),
        // None = nur ins offscreen_view zeichnen, nichts praesentieren: das laufende Weltbild
        // hinter einem Menue (s. App::world_backdrop_visible), das render_menu danach zeigt.
        acquired: Option<(wgpu::SurfaceTexture, f32)>,
    ) -> Option<FrameTiming> {
        // Zeitmessung faengt VOR der Zero-Modus-Fence-Wait an (s. render_menu-Kommentar) --
        // sonst waere die Wartezeit auf das vorherige Frame im HUD unsichtbar.
        let work_started = Instant::now();

        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        self.block_renderer.update_camera(&self.queue, camera, aspect);
        if self.block_renderer.vanilla.has_item_program() || self.block_renderer.vanilla.has_border_program() {
            let (zeit, licht) = (self.block_renderer.vanilla_game_time, self.block_renderer.daylight);
            self.block_renderer.vanilla.update_from_camera(&self.queue, camera, (self.config.width, self.config.height), zeit, licht);
        }
        // Zeichenreihenfolge (Wasser hinten->vorn, opak vorn->hinten) VOR dem Render-Pass
        // festlegen -- danach haelt render() nur noch geteilte Verweise, s. update_draw_order.
        // Sortiert wird darin nur bei Spaltenwechsel/Mesh-Aenderung, nicht jeden Frame.
        let camera_chunk = (
            (camera.position.x / f64::from(world::SECTION_SIZE)).floor() as i32,
            (camera.position.z / f64::from(world::SECTION_SIZE)).floor() as i32,
        );
        self.block_renderer.update_draw_order(camera_chunk);
        // Durchsichtige Quads nach Abstand umsortieren. Ohne das entscheidet die feste
        // Rasterreihenfolge des Meshers, welche Flaeche oben blendet -- aus einer
        // Blickrichtung zufaellig richtig, aus der Gegenrichtung zwangslaeufig falsch
        // (s. sort_translucent). Ein Block Schwelle -- derselbe Wert, den Vanilla fuer seine
        // Neusortierung der Translucent-Sections nimmt; darunter bemerkt man die Aenderung
        // beim Gehen ohnehin nicht, und jede Neusortierung kostet Buffer-Uploads.
        self.block_renderer.sort_translucent(&self.queue, camera.position, 1.0);
        // Augenrelative Spalten-Versaetze fuer DIESEN Frame (s. update_column_offsets).
        // Muss vor den Render-Pass: dort haelt render() nur noch geteilte Verweise.
        //
        // Bezugspunkt ist das AUGE (camera.position), NICHT die Kameraposition
        // (view_position()): view_matrix() blickt aus render_view_position() == view_offset,
        // im hochgeladenen Raum sitzt die Kamera also bei view_offset und der Ursprung ist
        // das Auge. Genau so laden Entities und Geisterbloecke ihre Geometrie hoch
        // (s. build_entity_vertices/ghost_vertices).
        //
        // ECHTER BUG gewesen: hier stand view_position(). In der Ego-Ansicht ist der
        // Unterschied null (view_offset == 0), in der 3. PERSON aber genau der Kameraversatz
        // -- die ganze Welt war dort gegen alle Entities verschoben. Sichtbar wurde das als
        // Beine, die durch den Boden gingen, weil der Versatz auch eine
        // Hoehenkomponente hat (camera_y_offset). s. camera::bezugspunkt_tests.
        let eye = camera.position;
        self.block_renderer.update_column_offsets(&self.device, &self.queue, eye);
        self.last_sort_ms = work_started.elapsed().as_secs_f32() * 1000.0;
        let frustum = render::Frustum::from_view_proj(camera.cull_view_proj_matrix(aspect));
        self.block_renderer.prepare_terrain_draws(&self.device, &self.queue, &frustum, eye, camera.view_position());

        let screen_px = self.size();
        // Projizierte Text-Displays zuerst (unter dem HUD-Text), hoechstens die Haelfte des
        // Puffers -- der HUD-Text darf nie wegen Hologrammen fehlen.
        // Die Overlay-Ebene zuerst (s. overlay_vertices).
        let ov = self.overlay_vertices(overlay, screen_px);
        let mut text_data: Vec<TextVertex> = std::mem::take(&mut *self.world_label_glyphs.borrow_mut());
        text_data.truncate(MAX_UI_GLYPHS / 2 * VERTS_PER_QUAD);
        let label_len = text_data.len() as u32;
        let mut schriften = self.shape_text(texts, screen_px, &mut text_data);
        // Die Text-Displays vorn in der Schrift des Bereichs Nametags, ab dem HUD-Text wieder
        // in der allgemeinen (die Laeufe von shape_text setzen dort an).
        if let Some(kind) = self.label_font.get().filter(|_| label_len > 0) {
            schriften.splice(0..0, [(0, kind), (label_len, self.font_atlas.kind())]);
        }
        // Schildtext fuer den Welt-Durchgang (s. App::build_sign_texts).
        let sign_len = {
            let glyphen = self.sign_glyphs.borrow();
            if !glyphen.is_empty() {
                self.queue.write_buffer(&self.sign_text_buffer, 0, bytemuck::cast_slice(&glyphen));
            }
            glyphen.len() as u32
        };
        let sign_kind = self.sign_font.get().unwrap_or(self.font_atlas.kind());
        let text_split = append_overlay(&mut text_data, &ov.text, MAX_UI_GLYPHS * VERTS_PER_QUAD);
        if !text_data.is_empty() {
            self.queue.write_buffer(&self.text_vertex_buffer, 0, bytemuck::cast_slice(&text_data));
        }
        // Leer zurueck: so behaelt die Liste ihre Groesse und waechst im naechsten Frame nicht
        // wieder Stueck fuer Stueck (Profiler: realloc beim Bau der Namensschilder).
        let text_len = text_data.len() as u32;
        text_data.clear();
        *self.world_label_glyphs.borrow_mut() = text_data;

        // Fuers F3-Debug-Overlay (Graph-Hintergrund, s. main.rs::build_debug_overlay) --
        // derselbe quad_pipeline/quad_vertex_buffer wie im Menu (render_menu), hier aber
        // NICHT ins Menu, sondern in den Welt-HUD-Durchgang unten gezeichnet.
        //
        // Hintergruende der Text-Displays zuerst (unter allen HUD-Flaechen), gedeckelt auf ein
        // Viertel des Puffers.
        let mut quad_data: Vec<Vertex> = std::mem::take(&mut *self.world_label_quads.borrow_mut());
        quad_data.truncate(MAX_UI_RECTS / 4 * VERTS_PER_QUAD);
        let welt_quads = quad_data.len() / VERTS_PER_QUAD;
        quad_data.reserve(rects.len() * VERTS_PER_QUAD);
        for r in rects.iter().take(MAX_UI_RECTS.saturating_sub(welt_quads)) {
            quad_data.extend_from_slice(&rect_verts(r, screen_px));
        }
        let quad_split = append_overlay(&mut quad_data, &ov.quads, MAX_UI_RECTS * VERTS_PER_QUAD);
        if !quad_data.is_empty() {
            self.queue.write_buffer(&self.quad_vertex_buffer, 0, bytemuck::cast_slice(&quad_data));
        }
        let quad_len = quad_data.len() as u32;
        quad_data.clear();
        *self.world_label_quads.borrow_mut() = quad_data;

        // Liniensegmente der Graphen (UiLine) in einem EIGENEN Puffer -- bei z.B. 16 Kernen *
        // 89 Segmenten sind das leicht ueber 1000 Quads, weit mehr als MAX_UI_RECTS fuer
        // normale UI-Rechtecke vorsieht. Ein gemeinsamer Puffer haette entweder den Menu-
        // Rechteck-Puffer unnoetig aufblasen muessen oder waere bei vielen Kernen ueber die
        // Kapazitaet hinaus beschrieben worden (wgpu-Validierungsfehler).
        // Erst die normalen Linien, dahinter die obersten (top, nach dem Text gezeichnet).
        let mut line_data: Vec<Vertex> = Vec::with_capacity(hud_lines.len() * VERTS_PER_QUAD);
        let mut top_start = None;
        for line in hud_lines.iter().filter(|l| !l.top).chain(hud_lines.iter().filter(|l| l.top)) {
            if line.top && top_start.is_none() {
                top_start = Some(line_data.len() as u32);
            }
            let verts: &[Vertex] = if let Some(q) = line.quad {
                &free_quad_verts_colored(q, line.quad_colors.unwrap_or([line.color; 4]), screen_px)
            } else if line.feather > 0.0 {
                &soft_line_verts(line, screen_px)
            } else {
                &line_quad_verts(line, line.width, screen_px)
            };
            if line_data.len() + verts.len() > MAX_UI_LINE_SEGMENTS * VERTS_PER_QUAD {
                break; // Puffer voll -- Rest weglassen statt ueberlaufen
            }
            line_data.extend_from_slice(verts);
        }
        if !line_data.is_empty() {
            self.queue.write_buffer(&self.line_vertex_buffer, 0, bytemuck::cast_slice(&line_data));
        }
        let line_len = line_data.len() as u32;
        let top_start = top_start.unwrap_or(line_len).min(line_len);
        let draw_top_lines = |pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline, buffer: &wgpu::Buffer| {
            if top_start < line_len {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(top_start..line_len, 0..1);
            }
        };

        let mut sprite_data: Vec<TextVertex> = Vec::new();
        // Mit Spielerfigur wird die Sprite-Liste GETEILT: alles vor dem Einfuegepunkt (der
        // Fensterhintergrund) kommt in den ersten Durchgang, die Figur dazwischen, der Rest
        // (Item-Symbole, der Stapel an der Maus) danach. shape_sprites haengt beide Male an
        // denselben Vertexpuffer an und rechnet mit absoluten Offsets, die Laeufe passen also
        // ohne Umrechnung zusammen.
        let unter = self.under_sprites.replace(0).min(sprites.len());
        let unter_runs = Self::shape_sprites(&sprites[..unter], screen_px, &mut sprite_data);
        let sprites = &sprites[unter..];
        let doll_split = doll_split.filter(|_| self.block_renderer.has_doll()).map(|k| k.saturating_sub(unter).min(sprites.len()));
        let (sprite_runs, sprite_runs_after) = match doll_split {
            Some(k) => {
                let vorher = Self::shape_sprites(&sprites[..k], screen_px, &mut sprite_data);
                let nachher = Self::shape_sprites(&sprites[k..], screen_px, &mut sprite_data);
                (vorher, Some(nachher))
            }
            None => (Self::shape_sprites(sprites, screen_px, &mut sprite_data), None),
        };
        let sprite_split = append_overlay_sprites(&mut sprite_data, &ov.sprites);
        if !sprite_data.is_empty() {
            self.queue.write_buffer(&self.sprite_vertex_buffer, 0, bytemuck::cast_slice(&sprite_data));
        }
        let sprite_len = sprite_data.len() as u32;

        // Zeigerform als Uniform, kantengeglaettet im Shader (s. cursor.wgsl und render_menu).
        if let Some(pos) = cursor {
            self.queue.write_buffer(&self.cursor_uniform_buffer, 0, bytemuck::bytes_of(&cursor_polygons(pos, screen_px, cursor_fx)));
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("world encoder") });
        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.begin_frame();
        }
        // Bewegungsunschaerfe nur im PvP-Pfad und mit normaler Projektion: die Warp-Projektionen
        // verbiegen die Geometrie im Vertex-Shader, eine Matrix rechnet sie nicht zurueck.
        let motion_blur = self.motion_blur_strength > 0.0 && !self.use_shader_path && camera.projection == 0;
        self.block_renderer.keep_depth = motion_blur;
        if self.use_shader_path {
            self.render_world_shader_path(&mut encoder, sky, camera);
        } else {
            // Himmel aus Resourcepacks: in die Ansicht ohne sRGB (Mischung im Gamma-Raum, s.
            // render::custom_sky), geloescht mit der Himmelsfarbe in denselben Werten. Alles
            // danach laedt das Bild, statt es zu loeschen.
            let himmel = !self.sky_layers.is_empty();
            if himmel {
                let raw = self.config.format.remove_srgb_suffix();
                let view = self.offscreen_texture.create_view(&wgpu::TextureViewDescriptor { format: Some(raw), ..Default::default() });
                let gamma = |c: f64| if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
                let clear = if raw == self.config.format { sky } else { wgpu::Color { r: gamma(sky.r), g: gamma(sky.g), b: gamma(sky.b), a: sky.a } };
                let rotation = camera.view_rotation();
                let view_proj = camera.proj_matrix(aspect) * rotation;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("custom sky pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Himmel")),
                    occlusion_query_set: None,
                });
                self.custom_sky.draw(&self.queue, &mut pass, view_proj, &self.sky_layers);
            }
            // Verlaufshimmel (Modul Himmel): statt der Loeschfarbe, ebenfalls vor allem anderen.
            let verlauf = self.sky_gradient.filter(|_| !himmel);
            if let Some(frame) = &verlauf {
                let rotation = camera.view_rotation();
                let view_proj = camera.proj_matrix(aspect) * rotation;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("gradient sky pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.offscreen_view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(sky), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Himmel")),
                    occlusion_query_set: None,
                });
                self.gradient_sky.draw(&self.queue, &mut pass, view_proj, frame);
            }
            // Fernsicht (Modul Chunk-Saver) zuerst, mit eigener Tiefe; das Gelaende laedt danach
            // das Bild, statt es zu loeschen (s. render::lod_renderer).
            let lod = self.lod.as_ref().filter(|l| self.lod_active && l.has_draws());
            let sky_load = if himmel || verlauf.is_some() { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(sky) };
            if let Some(lod) = lod {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("lod pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.offscreen_view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: sky_load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(lod.depth_attachment()),
                    timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Fernsicht")),
                    occlusion_query_set: None,
                });
                lod.render(&mut pass);
            }
            let load = if lod.is_some() { wgpu::LoadOp::Load } else { sky_load };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(self.block_renderer.depth_attachment()),
                timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Welt")),
                occlusion_query_set: None,
            });
            let timer = &mut self.gpu_timer;
            self.block_renderer.render(&mut pass, &mut |pass, label| {
                if let Some(t) = timer.as_mut() {
                    t.mark(pass, label);
                }
            });
            // Schildtext gegen die Tiefe der Welt -- danach, damit davor stehende Entities ihn
            // verdecken (s. world_text_pipeline). Mit Bewegungsunschaerfe erst nach ihr.
            if sign_len > 0 && !motion_blur {
                pass.set_pipeline(&self.world_text_pipeline);
                pass.set_vertex_buffer(0, self.sign_text_buffer.slice(..));
                pass.set_bind_group(0, &self.hud_font(sign_kind).bind_group, &[]);
                pass.draw(0..sign_len, 0..1);
            }
        }

        // Blockumriss in der Gegenfarbe (Modul Blockumriss, Komplementaer): nach der Welt, vor
        // der Hand, gegen die Tiefe der Welt.
        if !self.use_shader_path {
            self.block_renderer.encode_outline_complement(&self.device, &mut encoder, &self.offscreen_texture, &self.offscreen_view);
        }

        // Leucht-Umriss: nach der Welt, vor der Hand -- wie in Vanilla liegt die Hand darueber.
        if self.block_renderer.has_outlines() {
            let raw = self.config.format.remove_srgb_suffix();
            let view = self.offscreen_texture.create_view(&wgpu::TextureViewDescriptor { format: Some(raw), ..Default::default() });
            let size = (self.config.width, self.config.height);
            self.entity_outline.encode(&self.device, &self.queue, &mut encoder, &self.block_renderer, &view, size);
        }

        // Bewegungsunschaerfe: nach Welt und Umrissen, vor der Hand -- Hand und HUD bleiben scharf.
        if motion_blur {
            let timer = &mut self.gpu_timer;
            let mut reserve = || timer.as_mut().and_then(|t| t.reserve("Motion Blur").map(|(b, e)| (t.query_set(), b, e)));
            let verwischt = self.motion_blur.encode(
                &self.device,
                &self.queue,
                &mut encoder,
                &self.offscreen_texture,
                &self.offscreen_view,
                &self.block_renderer,
                camera,
                aspect,
                self.motion_blur_strength,
                self.motion_blur_interval_s,
                &mut reserve,
            );
            // Was man lesen koennen soll, bleibt scharf: Schilder samt Text und Karten in Rahmen
            // noch einmal darueber, gegen die Tiefe der Welt. Namensschilder und Text-Displays
            // kommen ohnehin erst mit dem HUD. Ohne Unschaerfe in diesem Bild nur der
            // Schildtext, der im Welt-Durchgang ausgelassen wurde. Die Zeit zaehlt zu Motion Blur.
            let stempel = if verwischt { self.gpu_timer.as_mut().and_then(|t| t.pass("Motion Blur")) } else { None };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sharp after motion blur pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(self.block_renderer.depth_attachment_load()),
                timestamp_writes: stempel,
                occlusion_query_set: None,
            });
            if verwischt {
                self.block_renderer.render_sharp_entities(&mut pass);
            }
            if sign_len > 0 {
                pass.set_pipeline(&self.world_text_pipeline);
                pass.set_vertex_buffer(0, self.sign_text_buffer.slice(..));
                pass.set_bind_group(0, &self.hud_font(sign_kind).bind_group, &[]);
                pass.draw(0..sign_len, 0..1);
            }
        } else {
            self.motion_blur.reset();
        }

        // Ego-Hand: EIGENER Durchgang mit geleertem Tiefenpuffer und eigener Projektion --
        // genauso wie in Vanilla (GameRenderer: clearDepthTexture, dann renderHand mit
        // getFov(..., false)). Das Leeren ist kein Trick, sondern der Grund, warum die Hand nie
        // in einer Wand verschwindet.
        //
        // Das Sichtfeld ist bewusst NICHT `camera.fov_degrees`: Vanillas Hand haengt am festen
        // Wert 70 und ist von der Einstellung voellig unabhaengig (s. update_hand_camera).
        self.block_renderer.update_hand_camera(&self.queue, aspect);
        if hand_debug.is_none() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hand pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(self.block_renderer.depth_attachment()),
                timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Hand")),
                occlusion_query_set: None,
            });
            self.block_renderer.render_hand(&mut pass);
        }

        // Overlay in einem zweiten Durchgang: laedt das Bild statt es zu loeschen, und
        // ohne Tiefentest -- der HUD gehoert immer nach vorn, egal was dahinter steht.
        // Quads (Graph-Hintergruende) und Linien (Graph-Kurven) VOR dem Text, damit Text bei
        // Bedarf darueber liegen kann -- exakt dieselbe Grund-Reihenfolge wie im Menu-Pass
        // (render_menu).
        // Der Shader-Pfad hat keinen Welt-Durchgang mit dieser Tiefe: dort liegt der Schildtext
        // ohne Tiefentest unter dem HUD.
        let schilder_im_hud = self.use_shader_path && sign_len > 0;
        // Bei Hand-Debug kommt der Zeiger erst nach der Hand, ins Schwarze (s. draw_hand_debug_ui).
        let cursor_im_hud = cursor.is_some() && hand_debug.is_none();
        if quad_len > 0 || !line_data.is_empty() || text_len > 0 || !sprite_data.is_empty() || cursor_im_hud || schilder_im_hud {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world hud pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("HUD")),
                occlusion_query_set: None,
            });
            if schilder_im_hud {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_vertex_buffer(0, self.sign_text_buffer.slice(..));
                pass.set_bind_group(0, &self.hud_font(sign_kind).bind_group, &[]);
                pass.draw(0..sign_len, 0..1);
            }
            if !unter_runs.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_vertex_buffer(0, self.sprite_vertex_buffer.slice(..));
                self.draw_sprite_runs(&mut pass, &unter_runs);
            }
            if quad_split > 0 {
                pass.set_pipeline(&self.quad_pipeline);
                pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
                pass.draw(0..quad_split, 0..1);
            }
            if top_start > 0 {
                pass.set_pipeline(&self.quad_pipeline);
                pass.set_vertex_buffer(0, self.line_vertex_buffer.slice(..));
                pass.draw(0..top_start, 0..1);
            }
            if !sprite_data.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_vertex_buffer(0, self.sprite_vertex_buffer.slice(..));
                self.draw_sprite_runs(&mut pass, &sprite_runs);
            }
            // Text und Zeiger nur hier, wenn KEINE Figur dazwischenkommt -- sonst laegen sie
            // hinter ihr (s. den zweiten Durchgang unten).
            if sprite_runs_after.is_none() {
                // Zeiger ganz zuletzt -- er gehoert immer nach vorn (s. render_menu).
                self.draw_ui_top(&mut pass, (text_split, text_len), (quad_split, quad_len), &ov.ranges, (sprite_split, sprite_len), cursor_im_hud, &schriften);
                draw_top_lines(&mut pass, &self.quad_pipeline, &self.line_vertex_buffer);
            }
        }

        // Spielerfigur im Inventar: eigener Durchgang MITTEN im HUD -- vor dem
        // Fensterhintergrund, hinter den Item-Symbolen. Sie ist echte 3D-Geometrie und
        // braucht deshalb einen geleerten Tiefenpuffer (ein Arm vor dem Rumpf muss ihn
        // verdecken), genau wie die Ego-Hand weiter oben.
        if let Some(sprite_runs_after) = sprite_runs_after {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("inventory doll pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.offscreen_view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(self.block_renderer.depth_attachment()),
                    timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Figur")),
                    occlusion_query_set: None,
                });
                self.block_renderer.render_doll(&mut pass);
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world hud pass (nach der Figur)"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("HUD")),
                occlusion_query_set: None,
            });
            if !sprite_data.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_vertex_buffer(0, self.sprite_vertex_buffer.slice(..));
                self.draw_sprite_runs(&mut pass, &sprite_runs_after);
            }
            self.draw_ui_top(&mut pass, (text_split, text_len), (quad_split, quad_len), &ov.ranges, (sprite_split, sprite_len), cursor_im_hud, &schriften);
            draw_top_lines(&mut pass, &self.quad_pipeline, &self.line_vertex_buffer);
        }

        // offscreen_texture (Welt + HUD-Text, vollstaendig fertig) in die praesentierbare
        // Swapchain-Textur uebertragen -- entweder direkt kopiert (aus, wie bisher) oder erst
        // durch CMAA2 geschickt (an, s. cmaa2.wgsl: Compute-Durchgang schreibt nach
        // cmaa2_output_texture, ein Blit-Durchgang traegt das Ergebnis in die Swapchain ein,
        // weil eine sRGB-Swapchain-Textur nicht direkt per Compute beschreibbar ist).
        let Some((frame, wait_ms)) = acquired else {
            // Hintergrund fuers Menue: abschicken, das Bild holt render_menu gleich selbst.
            self.queue.submit(std::iter::once(encoder.finish()));
            self.keep_world_backdrop();
            self.offscreen_world_only = true;
            return None;
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        if let Some(anteil) = hand_debug {
            // Das Bild verkleinert links oben (Farb-Durchgang mit Viewport, s. color.wgsl), dann
            // die Hand direkt in die Swapchain -- nicht am Rand des kleinen Bildes geschnitten.
            self.queue.write_buffer(&self.color_settings_buffer, 0, bytemuck::cast_slice(&[saturation.unwrap_or(1.0), anteil, 0.0, 0.0]));
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("hand debug pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_viewport(0.0, 0.0, self.config.width as f32 * anteil, self.config.height as f32 * anteil, 0.0, 1.0);
                pass.set_pipeline(&self.color_pipeline);
                pass.set_bind_group(0, &self.color_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hand pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(self.block_renderer.depth_attachment()),
                timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Hand")),
                occlusion_query_set: None,
            });
            self.block_renderer.render_hand(&mut pass);
            drop(pass);
            self.draw_hand_debug_ui(&mut encoder, &view, hand_debug_ui, (text_len, quad_len), cursor.is_some());
        } else if cmaa2_enabled {
            {
                let mut cpass =
                    encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("cmaa2 compute"), timestamp_writes: None });
                cpass.set_pipeline(&self.cmaa2_compute_pipeline);
                cpass.set_bind_group(0, &self.cmaa2_compute_bind_group, &[]);
                let wg_x = self.config.width.div_ceil(8);
                let wg_y = self.config.height.div_ceil(8);
                cpass.dispatch_workgroups(wg_x, wg_y, 1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("cmaa2 blit pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Ausgabe")),
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.cmaa2_blit_pipeline);
                pass.set_bind_group(0, &self.cmaa2_blit_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        } else if let Some(saturation) = saturation {
            // Statt der Kopie: derselbe Weg durch den Farb-Shader (s. color.wgsl).
            self.queue.write_buffer(&self.color_settings_buffer, 0, bytemuck::cast_slice(&[saturation, 0.0, 0.0, 0.0]));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("color pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.gpu_timer.as_mut().and_then(|t| t.pass("Ausgabe")),
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.color_pipeline);
            pass.set_bind_group(0, &self.color_bind_group, &[]);
            pass.draw(0..3, 0..1);
        } else {
            encoder.copy_texture_to_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.offscreen_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyTexture {
                    texture: &frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d { width: self.config.width, height: self.config.height, depth_or_array_layers: 1 },
            );
        }

        // Crosshair als LETZTER Schritt, gezeichnet in die Swapchain-Textur (Load, nach dem
        // Blit oben) -- sampelt dabei offscreen_view (eine ANDERE Ressource als das
        // Render-Target dieser Pass, s. crosshair_vertices-Kommentar: dieselbe Textur
        // gleichzeitig lesen und beschreiben ist nicht erlaubt).
        let mut crosshair_data = Vec::new();
        if let Some(center) = crosshair_center {
            // Auf ganze Pixel gerundet -- s. crosshair_outline_rects: Kreuz und Kontur
            // muessen exakt dasselbe Raster treffen, sonst sieht der Rand einseitig aus.
            let crosshair_px = ((center.0 * screen_px.0).round(), (center.1 * screen_px.1).round());
            crosshair_data = if let Some(c) = &crosshair_extras.custom { custom_crosshair_vertices(c, crosshair_px, screen_px) } else {
                let size = if crosshair_extras.size == 0 { CROSSHAIR_SHAPES[0].3 } else { crosshair_extras.size };
                crosshair_vertices(crosshair_extras.shape, size, crosshair_px, screen_px, crosshair_outline)
            };
        }
        if let Some((mitte, anteil)) = crosshair_extras.ring {
            // Um die Mitte des Kreuzes, wie es gerade steht (auch beim Wackeln der Pfeil-Anzeige).
            let mitte = crosshair_center.unwrap_or(mitte);
            let versatz = if crosshair_extras.custom.is_some() {
                0.0
            } else {
                let size = if crosshair_extras.size == 0 { CROSSHAIR_SHAPES[0].3 } else { crosshair_extras.size };
                crosshair_center_offset(crosshair_extras.shape, size)
            };
            let mitte_px = ((mitte.0 * screen_px.0).round() + versatz, (mitte.1 * screen_px.1).round() + versatz);
            crosshair_data.extend(crosshair_ring_vertices(mitte_px, anteil, screen_px, crosshair_extras.ring_outline));
        }
        // Die Texte nach Schrift gruppiert: je Schrift ein Zeichenaufruf mit ihrer
        // Glyphen-Bindung (Gruppe 1). Kreuz und Kreis davor brauchen keine.
        let main_count = crosshair_data.len() as u32;
        // Flaeche je Farbplatz fuer die Gegenfarbe (uv links oben, rechts unten); leer = nicht da.
        let mut flaechen = [([f32::MAX; 2], [f32::MIN; 2]); CROSSHAIR_FADE_SLOTS];
        let umfassen = |f: &mut ([f32; 2], [f32; 2]), verts: &[CrosshairVertex]| {
            for v in verts {
                f.0 = [f.0[0].min(v.uv[0]), f.0[1].min(v.uv[1])];
                f.1 = [f.1[0].max(v.uv[0]), f.1[1].max(v.uv[1])];
            }
        };
        umfassen(&mut flaechen[0], &crosshair_data);
        let mut schrift_laeufe: Vec<(FontKind, std::ops::Range<u32>)> = Vec::new();
        for kind in FontKind::ALL {
            let von = crosshair_data.len();
            for text in crosshair_extras.texts.iter().filter(|t| t.font == kind) {
                let verts = self.crosshair_glyph_vertices(self.hud_font(kind), text, crosshair_outline, screen_px);
                if let Some(f) = flaechen.get_mut(text.fade_slot as usize) {
                    umfassen(f, &verts);
                }
                crosshair_data.extend(verts);
            }
            crosshair_data.truncate(CROSSHAIR_MAX_VERTICES);
            if crosshair_data.len() > von {
                schrift_laeufe.push((kind, von as u32..crosshair_data.len() as u32));
            }
        }
        if !crosshair_data.is_empty() {
            self.queue.write_buffer(&self.crosshair_vertex_buffer, 0, bytemuck::cast_slice(&crosshair_data));
            // Reihenfolge/Groesse exakt wie crosshair.wgsl::CrosshairSettings.
            let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
            let farbe = byte(crosshair_rgb[0]) | byte(crosshair_rgb[1]) << 8 | byte(crosshair_rgb[2]) << 16 | 0xff << 24;
            // Viertes Feld: Brenn-Farbe fuer Kreuz und Ring, Alpha 0 = brennt nicht.
            let feuer = crosshair_extras.fire_rgb.map_or(0, |c| byte(c[0]) | byte(c[1]) << 8 | byte(c[2]) << 16 | 0xff << 24);
            // Zeit seit dem letzten Komplementaer-Schritt, gedeckelt: nach einer Pause (Kreuz
            // aus, anderer Modus) steht die passende Farbe dann sofort da.
            let jetzt = Instant::now();
            let dt = jetzt.duration_since(self.crosshair_fade_clock).as_secs_f32().min(0.25);
            let mut crosshair_settings = [0u32; 20];
            crosshair_settings[..4].copy_from_slice(&[crosshair_mode, crosshair_boost.to_bits(), farbe, feuer]);
            for (i, (min, max)) in flaechen.iter().enumerate() {
                crosshair_settings[4 + i * 4..8 + i * 4].copy_from_slice(&[min[0], min[1], max[0], max[1]].map(f32::to_bits));
            }
            crosshair_settings[16] = dt.to_bits();
            self.queue.write_buffer(&self.crosshair_mode_buffer, 0, bytemuck::bytes_of(&crosshair_settings));
            if crosshair_mode == 0 {
                self.crosshair_fade_clock = jetzt;
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("crosshair fade"), timestamp_writes: None });
                pass.set_pipeline(&self.crosshair_fade_pipeline);
                pass.set_bind_group(0, &self.crosshair_bind_group, &[]);
                pass.dispatch_workgroups(CROSSHAIR_FADE_SLOTS as u32, 1, 1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("crosshair pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.crosshair_pipeline);
                pass.set_bind_group(0, &self.crosshair_bind_group, &[]);
                pass.set_bind_group(1, &self.font_atlas.bind_group, &[]);
                pass.set_vertex_buffer(0, self.crosshair_vertex_buffer.slice(..));
                pass.draw(0..main_count, 0..1);
                for (kind, laeufe) in schrift_laeufe {
                    pass.set_bind_group(1, &self.hud_font(kind).bind_group, &[]);
                    pass.draw(laeufe, 0..1);
                }
            }
        }

        // Das fertige Weltbild bleibt in offscreen_view stehen -- es dient den Menues als
        // Hintergrund, sobald man Esc drueckt (s. world_backdrop).
        self.keep_world_backdrop();
        self.offscreen_world_only = false;
        Some(self.submit_and_present(encoder, frame, wait_ms, work_started))
    }
}
