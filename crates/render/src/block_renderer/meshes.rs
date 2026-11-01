// Wetter, Partikel, Groesse, Atlas, Spalten-Meshes, Sortieren und Entity-Atlas.

use super::*;

impl BlockRenderer {
    // textures/environment/rain.png und snow.png als RGBA8 (Breite, Hoehe). Neu gesetzt auch
    // nach einem Resourcepack-Wechsel. Die Textur wiederholt sich in beide Richtungen: ein
    // Quad ist eine Bildbreite breit und je Block ein Viertel Bildhoehe hoch, und die
    // Fallbewegung schiebt die UVs endlos weiter.
    pub fn set_weather_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rain: (&[u8], u32, u32), snow: (&[u8], u32, u32)) {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("weather sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let gruppe = |(rgba, w, h): (&[u8], u32, u32)| {
            let view = crate::glint::upload_texture(device, queue, rgba, w, h);
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("weather bind group"),
                layout: &self.atlas_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            })
        };
        self.weather_bind_groups = Some([gruppe(rain), gruppe(snow)]);
    }

    // Die Niederschlags-Quads dieses Frames: erst `rain` Vertices Regen, dann `snow` Schnee.
    pub fn set_weather_vertices(&mut self, queue: &wgpu::Queue, vertices: &[WeatherVertex], rain: u32, snow: u32) {
        let n = vertices.len().min(WEATHER_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.weather_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        let rain = rain.min(n as u32);
        self.weather_draws = [rain, snow.min(n as u32 - rain)];
    }

    // Resourcepack-Wechsel: der Partikel-Atlas kann dabei die Groesse aendern, deshalb eine neue
    // Textur samt Bindgroup statt eines write_texture in die alte.
    pub fn rebuild_particle_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8], size: u32) {
        let (bind_group, _, _texture) =
            create_atlas_texture_and_bind_group(device, queue, std::slice::from_ref(&rgba.to_vec()), size, &self.atlas_bind_group_layout);
        self.particle_bind_group = bind_group;
    }

    // Die Partikel-Quads dieses Frames. `draws` teilt `vertices` in die drei hintereinander
    // liegenden Bereiche auf (s. ParticleDraws); was ueber die Kapazitaet hinausgeht, faellt
    // von hinten weg.
    pub fn set_particle_vertices(&mut self, queue: &wgpu::Queue, vertices: &[ParticleVertex], draws: ParticleDraws) {
        let n = vertices.len().min(PARTICLE_VERTEX_CAPACITY);
        if n > 0 {
            queue.write_buffer(&self.particle_buffer, 0, bytemuck::cast_slice(&vertices[..n]));
        }
        let mut rest = n as u32;
        let mut take = |want: u32| {
            let got = want.min(rest);
            rest -= got;
            got
        };
        self.particle_draws = ParticleDraws { opaque: take(draws.opaque), translucent: take(draws.translucent), block: take(draws.block) };
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.depth_view = create_depth_view(device, width, height);
    }

    // Kompletter Atlas-Austausch (Skin-Tab "Laden", s. App::reload_resource_pack) -- anders
    // als upload_atlas_tiles ersetzt das die GANZE Textur samt Mip-Kette, weil sich die
    // Kachelgroesse (und damit die Atlas-Kantenlaenge) durch ein anderes Resourcepack aendern
    // kann (HD statt 16px). Baut GEGEN DENSELBEN atlas_bind_group_layout wie beim ersten Mal
    // (s. dortigen Feldkommentar) -- die Pipelines bleiben dadurch weiter gueltig, es muss
    // nichts neu kompiliert werden. Bereits gemeshte Chunk-Vertexbuffer bleiben ebenfalls
    // gueltig: ihre UV-Bruchteile sind relativ (0..1) und haengen nur von der Kachel-INDEX-
    // Reihenfolge ab, nicht von tile_size/Atlasgroesse (s. assets::blocks::BlockAtlas::build,
    // uv_of-Kommentar) -- der Aufrufer mesht trotzdem sicherheitshalber alles neu (falls das
    // neue Pack zusaetzlich Modelle/Blockstates aendert, was die Index-Reihenfolge verschieben
    // koennte).
    pub(super) const fn atlas_group(&self) -> &wgpu::BindGroup {
        if self.mipmapping {
            &self.atlas_bind_group
        } else {
            &self.atlas_bind_group_flat
        }
    }

    pub fn rebuild_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas_mips: &[Vec<u8>], atlas_size: u32) {
        let (atlas_bind_group, atlas_bind_group_flat, atlas_texture) =
            create_atlas_texture_and_bind_group(device, queue, atlas_mips, atlas_size, &self.atlas_bind_group_layout);
        self.atlas_bind_group = atlas_bind_group;
        self.atlas_bind_group_flat = atlas_bind_group_flat;
        self.atlas_texture = std::sync::Arc::new(atlas_texture);
    }

    // Mehrere Atlas-Kacheln auf einmal ersetzen: je Kachel eine Liste von Mip-Stufen (`[0]` =
    // volle Groesse, beliebig viele Stufen -- Wasser nimmt nur Stufe 0), Ursprung und Kantenlaenge.
    //
    // EIN write_buffer plus Kopierbefehle statt je Kachel und Stufe ein write_texture: wgpu legt
    // fuer jedes write_texture einen eigenen Staging-Puffer an. Mit ~50 animierten Kacheln und
    // je sechs Stufen waren das bei gleichzeitigem Framewechsel ueber hundert Aufrufe und
    // mehrere Millisekunden in einem Frame -- im Frame-Profiler als Ausreisser alle 100 ms.
    pub fn upload_atlas_tiles(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tiles: &[(&[Vec<u8>], (u32, u32), u32)]) {
        upload_tiles(device, queue, &self.atlas_texture, &mut self.atlas_upload_buffer, tiles);
    }

    // Der Atlas selbst -- fuer den Animations-Thread der App, der seine Kacheln direkt
    // hochlaedt (s. upload_tiles).
    pub fn atlas_texture_shared(&self) -> std::sync::Arc<wgpu::Texture> {
        std::sync::Arc::clone(&self.atlas_texture)
    }

    // Setzt (oder ersetzt) die beiden Meshes (opak + Wasser-Translucent, s. world::mesher::
    // mesh_column-Kommentar) einer Chunk-Spalte. Ein leeres Mesh entfernt den jeweiligen
    // Eintrag -- sonst blieben Spalten ohne sichtbare Flaechen als leere Buffer liegen.
    pub fn set_column_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: (i32, i32),
        vertices: &[TerrainVertex],
        groups: [FaceGroup; FACE_GROUPS],
        runs: Vec<SectionRun>,
        visibility: Vec<world::visibility::SectionVis>,
        min_y: i32,
        translucent_vertices: &[TerrainVertex],
    ) {
        self.occlusion.set_column(key, visibility, min_y);
        self.set_terrain_mesh(device, queue, key, vertices, groups, runs, min_y);
        if let Some(old) = self.translucent_meshes.remove(&key) {
            self.translucent_arena.remove(old.slot);
        }
        self.ensure_quad_indices(device, queue, translucent_vertices.len().div_ceil(4) as u32);
        if let Some(slot) = self.translucent_arena.insert(device, queue, translucent_vertices) {
            self.translucent_generation += 1;
            let generation = self.translucent_generation;
            let mesh = TranslucentMesh { slot, offset_slot: 0, min_y, cpu_vertices: translucent_vertices.to_vec(), generation, in_flight: false };
            self.translucent_meshes.insert(key, mesh);
        }
        // Neue Geometrie ist unsortiert -- gezielt nur DIESE Spalte vormerken. (Frueher stand
        // hier translucent_sort_pos = None, was die Sortierung aller Spalten neu angestossen
        // hat. Mit dem Frame-Budget waere das fatal: waehrend Chunks streamen kommt praktisch
        // jeden Frame ein neues Mesh an, die Runde finge staendig von vorn an und die
        // entfernteren Spalten kaemen nie dran.)
        self.sort_dirty.insert(key);
        // Schluesselmenge kann sich geaendert haben (leere Vertices entfernen den Eintrag,
        // s. set_mesh) -- Reihenfolge neu aufbauen, s. draw_order.
        self.order_dirty = true;
    }

    // Sortiert die durchsichtigen Quads JEDER Spalte von HINTEN nach VORN, bezogen auf die
    // Kameraposition, und laedt sie neu hoch.
    //
    // WARUM das noetig ist: der Translucent-Pass schreibt keine Tiefe (s.
    // translucent_pipeline), also entscheidet allein die Zeichenreihenfolge, welche Flaeche
    // oben blendet. Der Mesher gibt die Zellen einer Spalte in fester Rasterreihenfolge aus
    // (aufsteigend nach z) -- diese Reihenfolge ist damit aus EINER Blickrichtung zufaellig
    // richtig und aus der Gegenrichtung zwangslaeufig falsch. Der Fehler war genau
    // das: nach Norden immer richtig, nach Sueden immer falsch,
    // sichtbar als blaue Glaskante an einem Wasserbecken.
    //
    // Nur die durchsichtige Geometrie wird sortiert -- sie ist ein winziger Bruchteil des
    // Gelaendes. Opake Spalten haben gar keine CPU-Kopie (s. ChunkMesh::cpu_vertices).
    //
    // `min_move` verhindert, dass bei jedem Frame alles neu sortiert und hochgeladen wird:
    // erst wenn sich die Kamera weit genug bewegt hat, kann sich die Reihenfolge ueberhaupt
    // sinnvoll aendern. Vanilla macht dasselbe (dort: Neusortierung, sobald die Kamera die
    // Sektion wechselt oder sich weit genug bewegt hat).
    // `camera` ist die ABSOLUTE Augenposition in der Welt (f64). Die Vertices der Spalten
    // sind dagegen spalten-lokal (s. ColumnOrigin), also wird die Kamera fuer jede Spalte in
    // deren lokalen Raum umgerechnet -- wieder in f64, damit die Differenz bei grossen
    // Weltkoordinaten nicht schon vor der Sortierung zerfaellt.
    pub fn sort_translucent(&mut self, queue: &wgpu::Queue, camera: DVec3, min_move: f64) {
        let bewegt = match self.translucent_sort_pos {
            Some(letzte) => letzte.distance_squared(camera) >= min_move * min_move,
            None => true,
        };
        if bewegt {
            self.translucent_sort_pos = Some(camera);
            self.sort_dirty.extend(self.translucent_meshes.keys().copied());
        }
        // Erst abholen, was der Sortier-Thread fertig hat, und hochladen -- veraltete
        // Ergebnisse (Spalte inzwischen neu gemesht oder entladen) fallen weg.
        let mut budget = SORT_QUADS_PER_FRAME;
        while budget > 0 {
            let Ok(job) = self.sort_thread.rx.try_recv() else { break };
            let quads = job.verts.len() / TerrainVertex::PER_QUAD;
            self.sort_in_flight_quads = self.sort_in_flight_quads.saturating_sub(quads);
            let Some(mesh) = self.translucent_meshes.get_mut(&job.key) else { continue };
            if mesh.generation != job.generation {
                continue;
            }
            self.translucent_arena.write(queue, mesh.slot, 0, &job.verts);
            mesh.cpu_vertices = job.verts;
            mesh.in_flight = false;
            budget = budget.saturating_sub(quads);
        }
        if self.sort_dirty.is_empty() {
            return;
        }

        // Abgearbeitet wird ueber MEHRERE Frames: translucent_order ist nach Abstand aufsteigend
        // sortiert (s. update_draw_order), also kommt zuerst dran, was am naechsten und damit am
        // sichtbarsten falsch waere. Unterwegs und hochgeladen wird je Frame nur ein Quad-Budget
        // -- ohne das schriebe ein grosser Ozean in EINEM Frame Hunderttausende Quads neu (32
        // Byte je Vertex, 192 je Quad): ein sichtbarer Ruckler bei jedem Schritt. Eine Spalte, die gerade unterwegs ist, bleibt
        // vorgemerkt und geht nach ihrer Rueckkehr noch einmal los.
        let order = std::mem::take(&mut self.translucent_order);
        for key in &order {
            if self.sort_in_flight_quads >= 2 * SORT_QUADS_PER_FRAME {
                break;
            }
            if !self.sort_dirty.contains(key) {
                continue;
            }
            let Some(mesh) = self.translucent_meshes.get_mut(key) else { continue };
            if mesh.in_flight {
                continue;
            }
            self.sort_dirty.remove(key);
            if mesh.cpu_vertices.len() <= TerrainVertex::PER_QUAD {
                continue; // ein einzelnes Quad kann sich nicht mit sich selbst vertauschen
            }
            let quads = mesh.cpu_vertices.len() / TerrainVertex::PER_QUAD;
            let base = DVec3::new(f64::from(key.0 * SECTION_SIZE), f64::from(mesh.min_y), f64::from(key.1 * SECTION_SIZE));
            let job = SortJob { key: *key, generation: mesh.generation, verts: std::mem::take(&mut mesh.cpu_vertices), camera: (camera - base).as_vec3() };
            match self.sort_thread.tx.send(job) {
                Ok(()) => {
                    mesh.in_flight = true;
                    self.sort_in_flight_quads += quads;
                }
                Err(e) => mesh.cpu_vertices = e.0.verts,
            }
        }
        self.translucent_order = order;
        // Schluessel, die gar kein Mesh (mehr) haben, bleiben sonst ewig in der Menge liegen.
        self.sort_dirty.retain(|k| self.translucent_meshes.contains_key(k));
    }

    // Baut draw_order/translucent_order neu, wenn noetig. MUSS vor render() laufen, und zwar
    // VOR begin_render_pass: render() haelt danach `&'a self`-Verweise auf die Vertex-Buffer,
    // ein &mut self waere dort nicht mehr moeglich.
    pub fn update_draw_order(&mut self, center_chunk: (i32, i32)) {
        if !self.order_dirty && self.order_center == Some(center_chunk) {
            return;
        }
        self.order_center = Some(center_chunk);
        self.order_dirty = false;
        self.draw_order.clear();
        self.draw_order.extend(self.meshes.keys().copied());
        sortiere_nach_abstand(&mut self.draw_order, center_chunk);
        self.translucent_order.clear();
        self.translucent_order.extend(self.translucent_meshes.keys().copied());
        sortiere_nach_abstand(&mut self.translucent_order, center_chunk);
    }

    // Wirft die Meshes einer entladenen Spalte weg -- der ChunkMesh-Drop gibt den
    // wgpu::Buffer und damit den VRAM frei. Gegenstueck zu World::remove_column im Renderer.
    pub fn remove_column_mesh(&mut self, key: (i32, i32)) {
        if let Some(mesh) = self.meshes.remove(&key) {
            self.terrain_arena.remove(mesh.slot);
        }
        if let Some(mesh) = self.translucent_meshes.remove(&key) {
            self.translucent_arena.remove(mesh.slot);
        }
        self.occlusion.remove_column(key);
        self.sort_dirty.remove(&key);
        self.order_dirty = true;
    }

    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
        self.terrain_arena.clear();
        self.translucent_meshes.clear();
        self.translucent_arena.clear();
        self.draw_order.clear();
        self.translucent_order.clear();
        // Nicht nur die Listen leeren: order_center behaelt sonst die alte Spalte, und nach
        // einem Reconnect an derselben Position wuerde update_draw_order sich fuer fertig
        // halten und die neu geladenen Meshes nie einsortieren.
        self.order_center = None;
        self.order_dirty = true;
        self.translucent_sort_pos = None;
        self.sort_dirty.clear();
        self.occlusion.clear();
    }

    // Ueberschreibt einen zusammenhaengenden Vertex-Bereich im OPAKEN Mesh einer Spalte ab
    // `start` -- fuer das Einblenden der Eckabdunklung nach einer Platzierung (s.
    // app::AoFade). Fehlt der Mesh oder passt der Bereich nicht, passiert nichts.
    // Opakes Gelaende einer Spalte: Vertex-Puffer im kompakten Format, gezeichnet ueber den
    // gemeinsamen Index-Puffer (der dafuer notfalls waechst).
    pub(super) fn set_terrain_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: (i32, i32),
        vertices: &[TerrainVertex],
        groups: [FaceGroup; FACE_GROUPS],
        runs: Vec<SectionRun>,
        min_y: i32,
    ) {
        if let Some(old) = self.meshes.remove(&key) {
            self.terrain_arena.remove(old.slot);
        }
        self.ensure_quad_indices(device, queue, vertices.len().div_ceil(4) as u32);
        if let Some(slot) = self.terrain_arena.insert(device, queue, vertices) {
            let sections = runs.iter().fold(0u64, |m, r| m | 1u64.checked_shl(u32::from(r.section)).unwrap_or(0));
            self.meshes.insert(key, TerrainMesh { slot, groups, runs, sections, min_y, offset_slot: 0 });
        }
    }

    // Der gemeinsame Index-Puffer muss fuer `quads` Flaechen reichen.
    pub(super) fn ensure_quad_indices(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, quads: u32) {
        if quads > self.quad_index_capacity {
            self.quad_index_capacity = quads.next_power_of_two();
            self.quad_index_buffer = create_quad_index_buffer(device, queue, self.quad_index_capacity);
        }
    }

    pub fn write_column_vertices(&self, queue: &wgpu::Queue, key: (i32, i32), start: u32, vertices: &[TerrainVertex]) {
        let Some(mesh) = self.meshes.get(&key) else { return };
        self.terrain_arena.write(queue, mesh.slot, start, vertices);
    }

    // Fuer F3: belegte Sections der Spalte im Mesh und was das Hoehlen-Culling davon zeigt.
    pub fn column_debug(&self, key: (i32, i32)) -> (u64, u64) {
        let sections = self.meshes.get(&key).map_or(0, |m| m.sections);
        let sichtbar = if self.cave_culling { self.occlusion.mask(key) } else { u64::MAX };
        (sections, sichtbar)
    }

    pub fn has_column_mesh(&self, key: (i32, i32)) -> bool {
        self.meshes.contains_key(&key)
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    // Gesamtgroesse aller Chunk-Vertex-Buffer in Byte -- zum Mitschauen, wie teuer die
    // aktuelle Sichtweite wirklich ist (der Face-Culling-Mesher ist nicht sparsam).
    pub fn vertex_bytes(&self) -> usize {
        self.meshes.values().map(|m| m.slot.len as usize).sum::<usize>() * std::mem::size_of::<TerrainVertex>()
            + self.translucent_meshes.values().map(|m| m.slot.len as usize).sum::<usize>() * std::mem::size_of::<TerrainVertex>()
    }

    // Entity-Boxen fuer diesen Frame setzen (Vertices in Weltkoordinaten, wie die
    // Chunk-Meshes). Wird jeden Frame aufgerufen; ein leerer Slice blendet alle aus.
    // Einen nachgeladenen Spieler-Skin in einen reservierten Platz des Entity-Atlas
    // schreiben (s. assets::entity_atlas). Nur dieser Bereich wird ueberschrieben, die
    // Geometrie und ihre UVs bleiben unveraendert -- deshalb braucht es keinen Atlas-Neubau
    // und keinen zweiten Draw-Call.
    pub fn upload_skin(&self, queue: &wgpu::Queue, origin: (u32, u32), size: u32, rgba: &[u8]) {
        self.upload_entity_region(queue, origin, size, size, rgba);
    }

    // Umhaenge sind 64x32, nicht quadratisch -- deshalb eine Fassung mit getrennter Breite
    // und Hoehe (s. assets::entity_atlas::CAPE_WIDTH/CAPE_HEIGHT).
    pub fn upload_cape(&self, queue: &wgpu::Queue, origin: (u32, u32), width: u32, height: u32, rgba: &[u8]) {
        self.upload_entity_region(queue, origin, width, height, rgba);
    }

    // Item-Texturen (das, was Spieler in den Haenden halten). Wie beim Umhang mit getrennter
    // Breite und Hoehe, weil eine 16x16-Textur einen 32er-Platz nur teilweise belegt -- mit
    // der Platzgroesse statt der Bildgroesse wuerde write_texture ueber das Bild hinauslesen.
    pub fn upload_item(&self, queue: &wgpu::Queue, origin: (u32, u32), width: u32, height: u32, rgba: &[u8]) {
        self.upload_entity_region(queue, origin, width, height, rgba);
    }

    // Die GANZE Entity-Textur neu schreiben -- nach einem Resourcepack-Wechsel. Ohne das
    // blieben Ruestung, Schild, Mob-Texturen und Betten auf dem ALTEN Pack stehen: der Atlas
    // wird zwar neu gebaut, seine Pixel lagen aber seit dem Start unveraendert auf der GPU.
    // Schlimmer noch als "altes Bild": bringt das Pack eine andere Texturgroesse mit,
    // verschiebt sich die Packung, und die neuen Rechtecke zeigen in fremde Pixel.
    //
    // Die reservierten Nachladeplaetze (Skins, Umhaenge, Handgegenstaende) sind danach LEER --
    // der Aufrufer muss sie fuer ungueltig erklaeren, damit sie neu angefordert werden.
    pub fn rebuild_entity_atlas(&self, queue: &wgpu::Queue, rgba: &[u8], size: u32) {
        if rgba.len() < (size * size * 4) as usize {
            return;
        }
        self.upload_entity_region(queue, (0, 0), size, size, rgba);
    }

    pub(super) fn upload_entity_region(&self, queue: &wgpu::Queue, origin: (u32, u32), width: u32, height: u32, rgba: &[u8]) {
        if rgba.len() < (width * height * 4) as usize {
            return;
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.entity_atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: origin.0, y: origin.1, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }
}
