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
    pub fn update_column_offsets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, eye: DVec3) {
        let mut data = std::mem::take(&mut self.column_offset_scratch);
        data.clear();
        let mut slot = 0u32;
        let mut versatz = |key: &(i32, i32), min_y: i32| {
            data.extend_from_slice(&column_offset(*key, min_y, eye));
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
        // Die Tiefe bleibt nur stehen, wenn der komplementaere Blockumriss sie danach braucht.
        let store = if self.outline_complement.count > 0 { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard };
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.depth_view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store }),
            stencil_ops: None,
        }
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

        // Opake Partikel vor dem Wasser: sie schreiben Tiefe wie jede andere opake Geometrie.
        let d = self.particle_draws;
        let particle_bytes = |from: u32, count: u32| {
            let size = std::mem::size_of::<ParticleVertex>() as u64;
            self.particle_buffer.slice(u64::from(from) * size..u64::from(from + count) * size)
        };
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
            pass.set_pipeline(if lit { &self.lit_terrain_translucent_pipeline } else { &self.terrain_translucent_pipeline });
            // Von HINTEN nach VORN (s. prepare_terrain_draws). Index- und Versatz-Puffer muessen
            // neu gesetzt werden: Geister, Risse und Partikel davor binden eigene Puffer.
            pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_vertex_buffer(1, self.column_offset_buffer.slice(..));
            self.translucent_draws.draw(pass, &self.translucent_arena.pages, self.multi_draw);
        }

        if self.entity_vertex_count > 0 {
            mark(pass, "Entities");
            // NACH dem Wasser: das mischt ohne Tiefe zu schreiben und hat Spieler und Mobs
            // darunter/dahinter blau eingefaerbt (Nutzerwunsch: nicht einfaerben). Die Tiefe der
            // opaken Welt verdeckt Entities weiter korrekt.
            // Entities sampeln den ENTITY-Atlas, nicht den Block-Atlas -- Skins und
            // Mob-Texturen liegen dort (s. new()). Danach wieder zurueckbinden, damit die
            // Partikel unten wieder die Blocktextur treffen.
            pass.set_bind_group(1, &self.entity_bind_group, &[]);
            // Eigene Pipeline: die Schadens-Faerbung mischt NACH dem Texturzugriff und laesst
            // sich deshalb nicht ueber die Vertexfarbe der Block-Pipeline machen
            // (s. block.wgsl::fs_entity).
            // Shader-Pfad mit Licht je Vertex, sofern es fuer alle Vertices vorliegt.
            let beleuchtet = lit && self.entity_light_count == self.entity_vertex_count;
            pass.set_pipeline(if beleuchtet { &self.lit_entity_pipeline } else { &self.entity_pipeline });
            let bytes = u64::from(self.entity_vertex_count) * std::mem::size_of::<BlockVertex>() as u64;
            let tint_bytes = u64::from(self.entity_vertex_count) * std::mem::size_of::<[f32; 4]>() as u64;
            pass.set_vertex_buffer(0, self.entity_buffer.slice(..bytes));
            pass.set_vertex_buffer(1, self.entity_tint_buffer.slice(..tint_bytes));
            if beleuchtet {
                let light_bytes = u64::from(self.entity_vertex_count) * std::mem::size_of::<u32>() as u64;
                pass.set_vertex_buffer(2, self.entity_light_buffer.slice(..light_bytes));
            }
            let deckend = self.entity_vertex_count - self.entity_translucent_count;
            let kopf = self.entity_head_translucent;
            pass.draw(kopf..deckend, 0..1);
            if self.entity_translucent_count > 0 || kopf > 0 {
                pass.set_pipeline(if beleuchtet { &self.lit_entity_translucent_pipeline } else { &self.entity_translucent_pipeline });
                if kopf > 0 {
                    pass.draw(0..kopf, 0..1);
                }
                if self.entity_translucent_count > 0 {
                    pass.draw(deckend..self.entity_vertex_count, 0..1);
                }
            }

            // Glint verzauberter Items/Ruestung -- gleich danach, solange der Entity-Atlas
            // noch gebunden ist (die Maske sampelt ihn).
            self.draw_glint(pass, &self.entity_glint);
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

        // Hitbox-Anzeige (Modul): zeichnet gar keine Textur, die Farbe kommt aus dem Vertex
        // (s. fs_hitbox). Sie steht hier, weil die Kanten NACH der Projektion auf Pixelbreite
        // gebracht werden und dafuer gegen die fertige Tiefe der Welt testen muessen.
        // Nach dem Wasser, damit es sie nicht blau einfaerbt (wie bei den Entities oben).
        if self.hitbox_vertex_count > 0 {
            mark(pass, "Sonstiges");
            pass.set_pipeline(&self.hitbox_pipeline);
            let bytes = u64::from(self.hitbox_vertex_count) * std::mem::size_of::<HitboxVertex>() as u64;
            pass.set_vertex_buffer(0, self.hitbox_buffer.slice(..bytes));
            pass.draw(0..self.hitbox_vertex_count, 0..1);
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
}
