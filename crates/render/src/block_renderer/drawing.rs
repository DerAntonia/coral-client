// Spalten-Versatz, Terrain-Draws vorbereiten, Kamera und Zeichnen.

use super::*;

impl BlockRenderer {
    // Versaetze aller Spalten neu berechnen -- einmal pro Frame, VOR dem Zeichnen.
    //
    // Der Versatz ist `Spaltenbasis - Auge`, gerechnet in f64 und erst danach auf f32
    // verkuerzt. Genau darin liegt der ganze Gewinn: die Differenz ist hoechstens so gross
    // wie die Sichtweite (also ein paar hundert Bloecke), waehrend die beiden Summanden auf
    // einem Server wie DonutSMP sechsstellig sind. In f32 gerechnet waere die Differenz
    // schon auf 0.015625 gerastert -- in f64 liegt der Fehler bei rund 3e-11 und ueberlebt
    // die Verkuerzung auf f32 als kleine Zahl mit voller Genauigkeit.
    //
    // Die Plaetze werden JEDEN Frame neu vergeben (nicht dauerhaft an eine Spalte gebunden):
    // Spalten kommen und gehen staendig, und eine dauerhafte Vergabe braeuchte eine
    // Freiliste, die nichts brauchbares beitraegt -- die Schleife laeuft ohnehin ueber alle
    // Spalten, um die Versaetze zu schreiben.
    //
    // Geschrieben wird der Versatz in ZWEI Teilen: `Spaltenbasis - abgerundetes Auge` (ganze
    // Bloecke, in f32 exakt) und der Nachkomma-Anteil des Auges, fuer alle Spalten derselbe.
    // Der Shader rechnet erst lokal + ganzer Teil (exakt) und zieht dann den gemeinsamen
    // Anteil ab -- eine Kante zweier Nachbarspalten ergibt so auf beiden Seiten bitgleich
    // dieselbe Zahl. Mit einem fertig gerundeten Versatz je Spalte rechnete die eine Seite
    // `16 + rund(a)`, die andere `rund(a + 16)`, die um eine Binaerstelle auseinander liegen
    // koennen: beim perfekten Geradeausgucken gab es dann oft Luecken zwischen
    // den Meshes -- entlang einer Achse liegen die Spaltengrenzen genau waagerecht bzw.
    // senkrecht im Bild, und die Abweichung fehlt dann als ganze Pixelzeile.
    // Eine Spalte, die gerade ihr erstes Mesh bekam, ueber `sekunden` einblenden (s.
    // block.wgsl::chunk_fade_discard). 0 = sofort ganz da.
    pub fn start_column_fade(&mut self, key: (i32, i32), sekunden: f32) {
        if sekunden > 0.0 {
            self.column_fade.insert(key, (std::time::Instant::now(), sekunden));
        }
    }

    pub fn update_column_offsets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, eye: DVec3) {
        let mut data = std::mem::take(&mut self.column_offset_scratch);
        data.clear();
        let mut slot = 0u32;
        // Dahinter je Spalte, wie weit sie noch ausgeblendet ist (s. start_column_fade).
        let jetzt = std::time::Instant::now();
        self.column_fade.retain(|_, (start, dauer)| jetzt.duration_since(*start).as_secs_f32() < *dauer);
        let fade = &self.column_fade;
        let mut versatz = |key: &(i32, i32), min_y: i32| {
            data.extend_from_slice(&column_offset(*key, min_y, eye));
            data.push(fade.get(key).map_or(0.0, |(start, dauer)| 1.0 - jetzt.duration_since(*start).as_secs_f32() / dauer));
            slot += 1;
            slot - 1
        };
        for (key, mesh) in &mut self.meshes {
            mesh.offset_slot = versatz(key, mesh.min_y);
        }
        for (key, mesh) in &mut self.translucent_meshes {
            mesh.offset_slot = versatz(key, mesh.min_y);
        }

        if slot > self.column_offset_capacity {
            // Mit Reserve wachsen, sonst wird bei jeder neu geladenen Spalte neu angelegt.
            let neu = (slot * 2).max(64);
            self.column_offset_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("column offsets"),
                size: wgpu::BufferAddress::from(neu) * COLUMN_OFFSET_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.column_offset_capacity = neu;
        }
        if !data.is_empty() {
            queue.write_buffer(&self.column_offset_buffer, 0, bytemuck::cast_slice(&data));
        }
        self.column_offset_scratch = data;
    }

    // Die Gelaende-Aufrufe dieses Frames festlegen: sichtbare Spalten (Sichtkegel-Test wie
    // draw_visible_columns), von VORN nach HINTEN, je Speicherseite zusammengefasst, und in den
    // indirekten Puffer schreiben. Nach update_column_offsets (braucht die Instanz-Nummern).
    //
    // `viewer` = wo die Kamera WIRKLICH sitzt (Camera::view_position) -- in der 3. Person ein
    // Stueck hinter bzw. vor dem Auge. Davon haengt ab, welche Flaechen ihre Vorderseite
    // zeigen; der Sichtkegel-Test rechnet dagegen augenrelativ wie die Geometrie.
    // Ist diese Section nach dem Hoehlen-Culling erreichbar (s. occlusion)? Fuer Entities und
    // Blockentities, wie Vanillas LevelRenderer.isSectionCompiledAndVisible. `index` zaehlt ab
    // der Unterkante der Welt. Ohne Hoehlen-Culling und ausserhalb der Spalte immer ja.
    #[must_use]
    pub fn section_reachable(&self, column: (i32, i32), index: i32) -> bool {
        if !self.cave_culling || !(0..64).contains(&index) {
            return true;
        }
        self.occlusion.mask(column) >> index & 1 != 0
    }

    pub fn prepare_terrain_draws(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frustum: &Frustum, eye: DVec3, viewer: DVec3) {
        // Opak: von vorn nach hinten, je Seite gesammelt (die Reihenfolge zwischen den Seiten
        // ist fuer den Tiefentest egal, innerhalb einer Seite bleibt sie erhalten).
        if self.cave_culling {
            self.occlusion.update(viewer, self.cull_radius);
        }
        let pages = self.terrain_arena.pages.len();
        let mut per_page: Vec<Vec<IndexedIndirect>> = vec![Vec::new(); pages];
        let (mut gesamt, mut gezeichnet) = (0u32, 0u32);
        for key in &self.draw_order {
            let Some(mesh) = self.meshes.get(key) else { continue };
            gesamt += mesh.sections.count_ones();
            if !column_visible(*key, frustum, eye) {
                continue;
            }
            // Hoehlen-Culling: welche Sections dieser Spalte ueberhaupt erreichbar sind.
            let sichtbar = if self.cave_culling { self.occlusion.mask(*key) & mesh.sections } else { mesh.sections };
            // Sichtkegel je Section: eine Spalte am Bildrand ragt meist weit ueber und unter
            // das Bild hinaus. Ein Block Rand ringsum, weil Modelle (Feuer, hohe Pflanzen)
            // ueber ihre Zelle hinausreichen und ihre Flaechen der Section ihres Blocks gehoeren.
            let sichtbar = sections_in_frustum(*key, mesh.min_y, sichtbar, frustum, eye);
            if sichtbar == 0 {
                continue;
            }
            gezeichnet += sichtbar.count_ones();
            let Some(list) = per_page.get_mut(mesh.slot.page as usize) else { continue };
            // Auge spalten-lokal wie die Vertex-Positionen (x/z ab Spaltenecke, y ab min_y).
            let lokal = (viewer - DVec3::new(f64::from(key.0 * SECTION_SIZE), f64::from(mesh.min_y), f64::from(key.1 * SECTION_SIZE))).as_vec3();
            let lokal = [lokal.x, lokal.y, lokal.z];
            let mut gruppe_sichtbar = [false; FACE_GROUPS];
            for (g, group) in mesh.groups.iter().enumerate() {
                gruppe_sichtbar[g] = group.quads > 0 && group.faces_eye(g, lokal);
            }
            // Benachbarte sichtbare Bereiche zu einem Eintrag zusammenziehen.
            let mut offen: Option<(u32, u32)> = None;
            let mut nimm = |first: u32, quads: u32, list: &mut Vec<IndexedIndirect>| {
                offen = match offen {
                    Some((f, q)) if f + q == first => Some((f, q + quads)),
                    other => {
                        if let Some((f, q)) = other {
                            list.push(IndexedIndirect {
                                index_count: q * 6,
                                instance_count: 1,
                                first_index: f * 6,
                                base_vertex: mesh.slot.start as i32,
                                first_instance: mesh.offset_slot,
                            });
                        }
                        Some((first, quads))
                    }
                };
            };
            if sichtbar == mesh.sections {
                // Ganze Spalte erreichbar: ganze Gruppen, wie ohne Hoehlen-Culling.
                for (g, group) in mesh.groups.iter().enumerate() {
                    if gruppe_sichtbar[g] {
                        nimm(group.first_quad, group.quads, list);
                    }
                }
            } else {
                for run in &mesh.runs {
                    if gruppe_sichtbar[run.group as usize] && sichtbar & 1u64.checked_shl(u32::from(run.section)).unwrap_or(u64::MAX) != 0 {
                        nimm(run.first_quad, run.quads, list);
                    }
                }
            }
            // Letzten offenen Bereich abschliessen (ein Bereich der Laenge 0 an einer Stelle,
            // an die nichts anschliesst, schiebt ihn hinaus und wird selbst verworfen).
            nimm(u32::MAX, 0, list);
        }
        self.culling_stats = (gesamt, gezeichnet);
        self.terrain_draws.clear();
        for (page, list) in per_page.into_iter().enumerate() {
            for arg in list {
                self.terrain_draws.push(page as u32, arg);
            }
        }

        // Wasser/Glas: von HINTEN nach VORN -- Pflicht, ALPHA_BLENDING ohne Tiefenschreiben ist
        // reihenfolgeabhaengig. Ein Seitenwechsel beginnt einen neuen Lauf, so bleibt die
        // Reihenfolge auch ueber Seiten hinweg erhalten.
        self.translucent_draws.clear();
        for key in self.translucent_order.iter().rev() {
            let Some(mesh) = self.translucent_meshes.get(key) else { continue };
            if !column_visible(*key, frustum, eye) {
                continue;
            }
            // Ganz unerreichbare Spalten (alle Sections hinter Fels) zeigen auch kein Wasser.
            if self.cave_culling && self.occlusion.mask(*key) == 0 {
                continue;
            }
            let quads = mesh.slot.len.div_ceil(4);
            self.translucent_draws.push(
                mesh.slot.page,
                IndexedIndirect {
                    index_count: quads * 6,
                    instance_count: 1,
                    first_index: 0,
                    base_vertex: mesh.slot.start as i32,
                    first_instance: mesh.offset_slot,
                },
            );
        }
        if self.multi_draw {
            self.terrain_draws.upload(device, queue);
            self.translucent_draws.upload(device, queue);
        }
    }

    pub fn update_camera(&self, queue: &wgpu::Queue, camera: &Camera, aspect: f32) {
        let half_fov_tan = (camera.fov_degrees.to_radians() * 0.5).tan();
        let uniform = CameraUniform {
            view: camera.view_matrix().to_cols_array_2d(),
            proj: camera.proj_matrix(aspect).to_cols_array_2d(),
            params: [f32::from(camera.projection), half_fov_tan, aspect, self.daylight],
            hitbox: self.hitbox_params,
            near: [self.entity_near_dither, self.mip_bias, 0.0, 0.0],
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    // Die Atlas-Textur selbst -- die App braucht sie ein zweites Mal fuer die Item-Icons im
    // Inventar (Blockitems werden als isometrischer Wuerfel aus denselben Flaechen-UVs
    // gezeichnet). Eigene Bind-Group statt dieser hier, weil die 2D-Oberflaeche einen
    // Nearest-Sampler ohne Mipmaps braucht (Pixel-Art in Vergroesserung, s. app::item_icons).
    pub fn atlas_texture(&self) -> &wgpu::Texture {
        &self.atlas_texture
    }

    // Der Entity-Atlas (Skins) -- die Tab-Liste zeichnet daraus die Koepfe als 2D-Sprites.
    pub const fn entity_atlas_texture(&self) -> &wgpu::Texture {
        &self.entity_atlas_texture
    }

    pub const fn depth_attachment(&self) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        // Die Tiefe bleibt nur stehen, wenn der komplementaere Blockumriss, die
        // Bewegungsunschaerfe oder ein spaeterer Durchgang (depth_read_later) sie danach braucht.
        let store = if self.outline_complement.count > 0 || self.keep_depth || self.depth_read_later { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard };
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.depth_view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store }),
            // Stencil der Render-Order (s. crate::render_order): leer anfangen.
            stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store }),
        }
    }

    // Weiterzeichnen auf der Tiefe des Welt-Durchgangs (nur mit keep_depth oder
    // depth_read_later). Danach verworfen, ausser ein spaeterer Durchgang liest sie noch --
    // die Hand leert sie ohnehin.
    pub const fn depth_attachment_load(&self) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        let store = if self.depth_read_later { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard };
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.depth_view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store }),
            stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store }),
        }
    }

    // Die Tiefe des Welt-Durchgangs (nur mit keep_depth nach dem Durchgang noch gueltig).
    pub const fn depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    // Dieselbe Tiefe zum Lesen als Textur (ohne den Stencil).
    pub const fn depth_sample_view(&self) -> &wgpu::TextureView {
        &self.depth_sample_view
    }

    // Welche Spalten gezeichnet werden (Sichtkegel, Richtungsgruppen, Reihenfolge), hat
    // prepare_terrain_draws schon festgelegt -- hier wird nur noch aufgezeichnet.
    // `mark` kennzeichnet fuer die GPU-Zeitmessung, wo ein Abschnitt (Gelaende, Wasser, ...)
    // anfaengt -- s. app::gpu_timer. Kostet nichts, solange niemand misst.
    pub fn render<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str),
    ) {
        if self.meshes.is_empty()
            && self.translucent_meshes.is_empty()
            && self.entity_vertex_count == 0
            && !self.skinning.has_draws()
            && self.ghost_vertex_count == 0
            && self.hitbox_vertex_count == 0
            && self.crumbling_vertex_count == 0
            && self.particle_draws.opaque + self.particle_draws.translucent + self.particle_draws.block == 0
            && self.weather_draws == [0; 2]
        {
            return;
        }
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, self.atlas_group(), &[]);
        self.draw_terrain(pass, &self.terrain_pipeline, mark);
        self.draw_after_terrain(pass, mark, false);
    }

    // Shader-Pfad, Teil 1: nur das opake Gelaende, in den G-Buffer (Albedo + Normale).
    pub fn render_gbuffer<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str)) {
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, self.atlas_group(), &[]);
        self.draw_terrain(pass, &self.gbuffer_terrain_pipeline, mark);
        if self.gbuffer_ghosts() {
            mark(pass, "Sonstiges");
            let bytes = u64::from(self.ghost_solid_count) * std::mem::size_of::<BlockVertex>() as u64;
            let light_bytes = u64::from(self.ghost_solid_count) * std::mem::size_of::<u32>() as u64;
            pass.set_pipeline(&self.gbuffer_ghost_pipeline);
            pass.set_vertex_buffer(0, self.ghost_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.ghost_light_buffer.slice(..light_bytes));
            pass.draw(0..self.ghost_solid_count, 0..1);
        }
    }

    // Shader-Pfad: zeichnet render_gbuffer die deckenden Geisterbloecke (statt des Forward-
    // Rests)? Nur, wenn fuer jeden ihrer Vertices Licht da ist.
    pub(super) const fn gbuffer_ghosts(&self) -> bool {
        self.ghost_solid_count > 0 && self.ghost_light_count >= self.ghost_solid_count
    }

    // Shader-Pfad, Teil 2: alles nach dem Gelaende wie im PvP-Pfad (Wasser, Entities,
    // Partikel, ...), ins fertig beleuchtete Bild und gegen die Tiefe aus dem G-Buffer-Pass.
    pub fn render_forward_rest<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str)) {
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, self.atlas_group(), &[]);
        self.draw_after_terrain(pass, mark, true);
    }

    pub(super) fn draw_terrain<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
        mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str),
    ) {
        mark(pass, "Gelaende");
        pass.set_pipeline(pipeline);
        pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        // Opak von VORN nach HINTEN: verdeckte Fragmente fallen so schon im Early-Z-Test
        // raus, statt erst schattiert und dann ueberschrieben zu werden. Reihenfolge und
        // Sichtbarkeit stehen schon in terrain_draws (s. prepare_terrain_draws); der
        // Spalten-Versatz kommt ueber die Instanz-Nummer aus dem ganzen Versatz-Puffer.
        pass.set_vertex_buffer(1, self.column_offset_buffer.slice(..));
        self.terrain_draws.draw(pass, &self.terrain_arena.pages, self.multi_draw);
    }

    // Alles, was im Welt-Pass nach dem opaken Gelaende kommt. Erwartet Kamera- und
    // Block-Atlas-Bindung (Gruppen 0 und 1).
    // `lit`: Shader-Pfad -- Wasser/Glas mit Licht (s. lit_terrain_translucent_pipeline).
    pub(super) fn draw_after_terrain<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, mark: &mut dyn FnMut(&mut wgpu::RenderPass<'a>, &'static str), lit: bool) {

        // Geisterbloecke zwischen Opak und Wasser: sie muessen gegen die opake Tiefe testen
        // (ein Block hinter einer Wand bleibt verdeckt), sollen aber vom Wasser noch
        // ueberblendet werden koennen.
        if self.ghost_vertex_count > 0 {
            mark(pass, "Sonstiges");
            let bytes = u64::from(self.ghost_vertex_count) * std::mem::size_of::<BlockVertex>() as u64;
            pass.set_vertex_buffer(0, self.ghost_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
            // Der deckende Teil laeuft durch die GEWOEHNLICHE Blockpipeline -- er soll wie ein
            // echter Block aussehen, Tiefe schreiben eingeschlossen. Nur der Rest (zu lange
            // unbestaetigt) bekommt die halbdurchsichtige Geister-Pipeline.
            // Im Shader-Pfad stehen sie mit Licht schon im G-Buffer (s. render_gbuffer).
            if self.ghost_solid_count > 0 && !(lit && self.gbuffer_ghosts()) {
                pass.set_pipeline(&self.pipeline);
                pass.draw(0..self.ghost_solid_count, 0..1);
            }
            let durchscheinend_ende = self.ghost_solid_count + self.ghost_translucent_count;
            if self.ghost_translucent_count > 0 {
                pass.set_pipeline(&self.translucent_pipeline);
                pass.draw(self.ghost_solid_count..durchscheinend_ende, 0..1);
            }
            if durchscheinend_ende < self.ghost_vertex_count {
                pass.set_pipeline(&self.ghost_pipeline);
                pass.draw(durchscheinend_ende..self.ghost_vertex_count, 0..1);
            }
        }

        // Risse ueber dem abgebauten Block -- nach allem Opaken (sie mischen sich mit dessen
        // Farbe), aber vor Wasser und Partikeln. Vanilla zeichnet sie an derselben Stelle
        // (LevelRenderer: "destroyProgress" nach dem Terrain-Pass).
        if self.crumbling_vertex_count > 0 {
            mark(pass, "Sonstiges");
            pass.set_pipeline(&self.crumbling_pipeline);
            let bytes = u64::from(self.crumbling_vertex_count) * std::mem::size_of::<BlockVertex>() as u64;
            pass.set_vertex_buffer(0, self.crumbling_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.zero_offset_buffer.slice(..));
            pass.draw(0..self.crumbling_vertex_count, 0..1);
        }

        let d = self.particle_draws;
        let particle_bytes = |from: u32, count: u32| {
            let size = std::mem::size_of::<ParticleVertex>() as u64;
            self.particle_buffer.slice(u64::from(from) * size..u64::from(from + count) * size)
        };
        // Render-Order (s. crate::render_order): erst alles, was "immer ueber" etwas liegen
        // kann -- Entities und Hitbox-Kanten schreiben in den Stencil, was sie verdecken --,
        // danach das Verdeckbare (Partikel, Wasser/Glas, Flammen), das diese Pixel auslaesst.
        // Vorher liefen die Entities NACH dem Wasser, damit es sie nicht blau einfaerbt
        // (Nutzerwunsch); dasselbe Bild liefert jetzt die Vorgabe "ueber Wasser".
        let order = self.render_order;
        let hat_entities = self.entity_vertex_count > 0 || self.skinning.has_draws();
        // Shader-Pfad mit Licht je Vertex, sofern es fuer alle Vertices vorliegt.
        let beleuchtet = lit && self.entity_light_count == self.entity_vertex_count;
        let hitboxen = self.hitbox_vertex_count > 0;
        let kanten_vor_spielern = order.hitbox_over_players();
        if hat_entities {
            mark(pass, "Entities");
            // Entities sampeln den ENTITY-Atlas, nicht den Block-Atlas -- Skins und
            // Mob-Texturen liegen dort (s. new()).
            pass.set_bind_group(1, &self.entity_bind_group, &[]);
            self.draw_entity_class(pass, render_order::CLASS_OTHER, beleuchtet);
            self.draw_entity_class(pass, render_order::CLASS_BLOCK_ENTITY, beleuchtet);
        }
        // Hitboxen "ueber Spielern": VOR den Spielern, die diese Pixel dann auslassen -- gegen
        // die Tiefe von Welt, Blockentities und Mobs. Mit Bewegungsunschaerfe hier nur der
        // Stencil; die Farbe kommt scharf nach ihr (s. render_hitboxes).
        if hitboxen && kanten_vor_spielern {
            mark(pass, "Sonstiges");
            self.draw_hitboxes(pass, self.keep_depth);
        }
        if hat_entities {
            mark(pass, "Entities");
            pass.set_bind_group(1, &self.entity_bind_group, &[]);
            self.draw_entity_class(pass, render_order::CLASS_PVP, beleuchtet);
            // Glint verzauberter Items/Ruestung -- solange der Entity-Atlas noch gebunden ist
            // (die Maske sampelt ihn).
            pass.set_stencil_reference(0);
            self.draw_glint(pass, &self.entity_glint);
            self.skinning.draw_glint(pass, &self.glint_bind_group);
        }
        // Hitbox-Anzeige (Modul): zeichnet gar keine Textur, die Farbe kommt aus dem Vertex
        // (s. fs_hitbox). Die Kanten werden NACH der Projektion auf Pixelbreite gebracht und
        // testen gegen die fertige Tiefe von Welt und Entities.
        // Mit Bewegungsunschaerfe erst nach ihr (s. render_hitboxes), damit sie scharf bleiben.
        if hitboxen && !kanten_vor_spielern && !self.keep_depth {
            mark(pass, "Sonstiges");
            self.draw_hitboxes(pass, false);
        }
        if hat_entities {
            // Die Flammen brennender Entities: verdeckbar, also nach allen Verdeckern.
            pass.set_bind_group(1, &self.entity_bind_group, &[]);
            self.draw_entity_class(pass, render_order::CLASS_FLAME, beleuchtet);
        }
        // Ab hier nur noch Verdeckbares: Referenz 0 (s. render_order::stencil_under).
        pass.set_stencil_reference(0);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(1, self.atlas_group(), &[]);

        // Opake Partikel vor dem Wasser: sie schreiben Tiefe wie jede andere opake Geometrie.
        if d.opaque > 0 {
            mark(pass, "Partikel");
            pass.set_pipeline(&self.particle_opaque_pipeline);
            pass.set_bind_group(1, &self.particle_bind_group, &[]);
            pass.set_vertex_buffer(0, particle_bytes(0, d.opaque));
            pass.draw(0..d.opaque, 0..1);
            pass.set_bind_group(1, self.atlas_group(), &[]);
        }

        // Wasser NACH allem Opaken (s. translucent_pipeline-Kommentar): eigener
        // Pipeline-Wechsel, sonst identischer Ablauf (dieselben Bind-Groups gelten weiter).
        if !self.translucent_meshes.is_empty() {
            mark(pass, "Wasser/Glas");
            // Von HINTEN nach VORN (s. prepare_terrain_draws). Index- und Versatz-Puffer muessen
            // neu gesetzt werden: Geister, Risse und Partikel davor binden eigene Puffer.
            pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_vertex_buffer(1, self.column_offset_buffer.slice(..));
            if lit {
                pass.set_pipeline(&self.lit_terrain_translucent_pipeline);
                self.translucent_draws.draw(pass, &self.translucent_arena.pages, self.multi_draw);
            } else if order.single_translucent_pass() {
                pass.set_pipeline(&self.terrain_translucent_pipeline);
                self.translucent_draws.draw(pass, &self.translucent_arena.pages, self.multi_draw);
            } else {
                // Wasser und Glas wirken auf einen Verdecker verschieden: Wasser und Rest
                // brauchen verschiedene Masken, also zwei Zuege ueber dieselben Daten (der
                // Shader verwirft jeweils die andere Art). Zwischen Wasser und Glas stimmt die
                // Reihenfolge dann nicht mehr -- Glas liegt ueber Wasser.
                pass.set_pipeline(&self.terrain_translucent_water_pipeline);
                self.translucent_draws.draw(pass, &self.translucent_arena.pages, self.multi_draw);
                pass.set_pipeline(&self.terrain_translucent_rest_pipeline);
                self.translucent_draws.draw(pass, &self.translucent_arena.pages, self.multi_draw);
            }
        }

        // Durchscheinende Entity-Teile (Windkugeln, ausblendende Teile, der eigene Spieler nah
        // an der Schulterkamera): gemischt ueber Wasser und Glas. Sie verdecken wie Spieler.
        let kopf = self.entity_head_translucent;
        if self.entity_vertex_count > 0 && (self.entity_translucent_count > 0 || kopf > 0) {
            mark(pass, "Entities");
            let deckend = self.entity_vertex_count - self.entity_translucent_count;
            pass.set_bind_group(1, &self.entity_bind_group, &[]);
            pass.set_pipeline(if beleuchtet { &self.lit_entity_translucent_pipeline } else { &self.entity_translucent_pipeline });
            pass.set_stencil_reference(u32::from(order.of_class(render_order::CLASS_PVP)));
            self.set_entity_buffers(pass, beleuchtet);
            if kopf > 0 {
                pass.draw(0..kopf, 0..1);
            }
            if self.entity_translucent_count > 0 {
                pass.draw(deckend..self.entity_vertex_count, 0..1);
            }
            pass.set_stencil_reference(0);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, self.atlas_group(), &[]);
        }

        // Items ueber das Programm eines Resourcepacks (rendertype_item_entity_translucent_cull)
        // -- Vanilla zeichnet sie mit dem durchscheinenden Entity-Batch. Eigene Bindungen
        // (Gruppe 0 ist dort eine andere), danach die eigenen wieder herstellen.
        if self.vanilla.draw_items(pass) {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, self.atlas_group(), &[]);
        }

        // Die Weltgrenze, immer ueber Vanillas Programm (aus dem Pack oder dem Jar). Additiv und
        // ohne Tiefe zu schreiben -- sie legt sich ueber alles Feste und Durchscheinende davor.
        if self.vanilla.draw_border(pass) {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, self.atlas_group(), &[]);
        }

        // Durchscheinende Partikel zuletzt, ueber dem Wasser: sie schreiben keine Tiefe und
        // muessen deshalb auf allem liegen, was sie verdecken koennten.
        if d.translucent + d.block > 0 {
            mark(pass, "Partikel");
            pass.set_pipeline(&self.particle_translucent_pipeline);
            if d.block > 0 {
                // Blockstaub sampelt den Block-Atlas -- der ist hier noch gebunden.
                pass.set_vertex_buffer(0, particle_bytes(d.opaque + d.translucent, d.block));
                pass.draw(0..d.block, 0..1);
            }
            if d.translucent > 0 {
                pass.set_bind_group(1, &self.particle_bind_group, &[]);
                pass.set_vertex_buffer(0, particle_bytes(d.opaque, d.translucent));
                pass.draw(0..d.translucent, 0..1);
                pass.set_bind_group(1, self.atlas_group(), &[]);
            }
        }

        // Regen und Schnee ganz zuletzt, wie in Vanilla nach allem Durchscheinenden: sie
        // schreiben keine Tiefe und liegen damit ueber Wasser und Partikeln dahinter.
        if let Some(gruppen) = &self.weather_bind_groups {
            let [regen, schnee] = self.weather_draws;
            if regen + schnee > 0 {
                mark(pass, "Sonstiges");
                pass.set_pipeline(&self.weather_pipeline);
                pass.set_vertex_buffer(0, self.weather_buffer.slice(..));
                let mut start = 0;
                for (gruppe, anzahl) in gruppen.iter().zip([regen, schnee]) {
                    if anzahl > 0 {
                        pass.set_bind_group(1, gruppe, &[]);
                        pass.draw(start..start + anzahl, 0..1);
                    }
                    start += anzahl;
                }
                pass.set_bind_group(1, self.atlas_group(), &[]);
            }
        }
    }
    // Tint- (und im Shader-Pfad Licht-) Puffer der Entity-Vertices binden.
    fn set_entity_buffers<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, beleuchtet: bool) {
        let n = u64::from(self.entity_vertex_count);
        pass.set_vertex_buffer(0, self.entity_buffer.slice(..n * std::mem::size_of::<BlockVertex>() as u64));
        pass.set_vertex_buffer(1, self.entity_tint_buffer.slice(..n * std::mem::size_of::<[f32; 4]>() as u64));
        if beleuchtet {
            pass.set_vertex_buffer(2, self.entity_light_buffer.slice(..n * std::mem::size_of::<u32>() as u64));
        }
    }

    // Die deckenden Entities einer Klasse der Render-Order: die Instanzen (GPU-Skinning) und
    // die Vertex-Bereiche, mit dem Stencil-Wert der Klasse. Flammen sind keine Verdecker: sie
    // laufen mit Referenz 0 durch die verdeckbare Pipeline. Der Entity-Atlas muss gebunden sein.
    // Eigene Pipeline: die Schadens-Faerbung mischt NACH dem Texturzugriff und laesst sich
    // deshalb nicht ueber die Vertexfarbe der Block-Pipeline machen (s. block.wgsl::fs_entity).
    fn draw_entity_class<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, class: u8, beleuchtet: bool) {
        let flamme = class == render_order::CLASS_FLAME;
        pass.set_stencil_reference(if flamme { 0 } else { u32::from(self.render_order.of_class(class)) });
        if !flamme {
            self.skinning.draw(pass, class);
        }
        if self.entity_vertex_count == 0 {
            return;
        }
        let deckend = self.entity_head_translucent..self.entity_vertex_count - self.entity_translucent_count;
        let bereiche = self.entity_class_ranges(deckend, class);
        if bereiche.is_empty() {
            return;
        }
        pass.set_pipeline(if beleuchtet {
            &self.lit_entity_pipeline
        } else if flamme {
            &self.entity_flame_pipeline
        } else {
            &self.entity_pipeline
        });
        self.set_entity_buffers(pass, beleuchtet);
        for r in bereiche {
            pass.draw(r, 0..1);
        }
    }

    // Die Hitbox-Kanten mit ihrem Stencil-Wert samt Merker. `nur_stencil`: ohne Farbe (s.
    // hitbox_mark_pipeline).
    fn draw_hitboxes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, nur_stencil: bool) {
        pass.set_pipeline(if nur_stencil { &self.hitbox_mark_pipeline } else { &self.hitbox_pipeline });
        pass.set_stencil_reference(u32::from(self.render_order.of_hitbox() | cover::MARK));
        let bytes = u64::from(self.hitbox_vertex_count) * std::mem::size_of::<HitboxVertex>() as u64;
        pass.set_vertex_buffer(0, self.hitbox_buffer.slice(..bytes));
        pass.draw(0..self.hitbox_vertex_count, 0..1);
        pass.set_stencil_reference(0);
    }
}
