// Biome-Registry, Meshing anstehender Spalten und Render-Grenzen.

use super::*;

impl App {
    // Biom-Registry des Servers (oder der Launcher-Szene, s. hero_scene.rs): Namen, Klima und
    // die Farben je Biom.
    pub(crate) fn apply_biome_registry(&mut self, biomes: &[protocol::packets::BiomeInfo]) {
        self.biome_names = biomes.iter().map(|b| b.name.clone()).collect();
        self.biome_climate = biomes
            .iter()
            .map(|b| world::weather::BiomeClimate { has_precipitation: b.has_precipitation, temperature: b.temperature })
            .collect();
        // Kommt im Configuration-State, also immer VOR den ersten Chunks (Play-
        // State) -- kein Neu-Meshen bereits geladener Spalten noetig, die gibt
        // es zu diesem Zeitpunkt noch gar nicht.
        let per_biome: Vec<world::BiomeColors> = biomes
            .iter()
            .map(|b| {
                let grass =
                    self.grass_colormap.as_ref().map_or([1.0, 1.0, 1.0], |c| c.sample(b.temperature, b.downfall));
                let foliage =
                    self.foliage_colormap.as_ref().map_or([1.0, 1.0, 1.0], |c| c.sample(b.temperature, b.downfall));
                let dry_foliage =
                    self.dry_foliage_colormap.as_ref().map_or([1.0, 1.0, 1.0], |c| c.sample(b.temperature, b.downfall));
                let water = [
                    ((b.water_color >> 16) & 0xFF) as f32 / 255.0,
                    ((b.water_color >> 8) & 0xFF) as f32 / 255.0,
                    (b.water_color & 0xFF) as f32 / 255.0,
                ];
                // Etwas kraeftiger als die rohen Colormap-/Registry-Werte (auf
                // Nutzerwunsch, s. Kommentar an saturate()) -- reine
                // Darstellungsentscheidung, keine Vanilla-Farbe.
                world::BiomeColors {
                    grass: saturate(grass, 1.25),
                    foliage: saturate(foliage, 1.25),
                    dry_foliage: saturate(dry_foliage, 1.25),
                    water: saturate(water, 1.15),
                }
            })
            .collect();
        self.biome_palette = std::sync::Arc::new(world::BiomePalette::new(per_biome));
    }

    // Baut die Meshes der als "dirty" markierten Chunk-Spalten. Chunk-Geometrie bauen ist
    // reine CPU-Arbeit ohne GPU-Zugriff -- laeuft auf dem dedizierten Mesh-Pool (s. dortigen
    // Modulkommentar), damit ein Schwall neuer Chunks nicht die Frame-Zeit sprengt. Nur der
    // GPU-Upload der fertigen Ergebnisse muss auf diesem Thread bleiben (Device/Queue sind
    // dem Render-Loop zugeordnet, s. GpuState).
    pub(crate) fn mesh_pending_columns(&mut self) {
        self.expire_stale_ghosts();
        let mut fertige_spalten: Vec<(i32, i32)> = Vec::new();
        // Gleiche Abstandsgrenze wie enforce_render_limits (ohne das Anzahl-Budget -- eine
        // Spalte, die nur am Budget scheitert, raeumt der naechste Frame dort weg).
        let player_cx = (self.physics.pos.x / f64::from(world::SECTION_SIZE)).floor() as i64;
        let player_cz = (self.physics.pos.z / f64::from(world::SECTION_SIZE)).floor() as i64;
        let max_distance_sq = i64::from(self.render_distance()).pow(2);
        if let Some(gpu) = self.gpu.as_mut() {
            // Auf Settings::mesh_columns_per_frame gedeckelt (s. dortigen Feldkommentar) --
            // die eigentlich teure Stelle ist der GPU-Upload hier, nicht das Einreihen der
            // Jobs oben. Ohne Deckel wuerde ein Schwall gleichzeitig fertiger Ergebnisse
            // (z.B. direkt nach dem Verbinden) hunderte Buffer-Uploads in einem einzigen
            // Frame verursachen -- verbleibende Ergebnisse bleiben einfach im Kanal und
            // kommen naechsten Frame dran, nichts geht dabei verloren.
            for _ in 0..self.settings.mesh_columns_per_frame {
                let Ok(MeshResult { key, opaque, groups, runs, visibility, opacity, min_y, translucent: translucent_vertices, fade_quads, block_light }) =
                    self.mesh_results_rx.try_recv()
                else {
                    break;
                };
                // Der Job lief los, als die Spalte noch gebraucht wurde. Inzwischen entladen
                // (ForgetChunk) oder aus der Sichtweite gefallen: verwerfen. Sonst landete das
                // Mesh trotzdem im VRAM -- eine entladene Spalte hing dann fuer immer in der
                // Luft (niemand raeumt sie mehr weg, sie steht in keiner Spaltenliste), eine
                // eben ausgeschiedene blitzte einen Frame lang auf, bis enforce_render_limits
                // sie wieder entfernte (Flackern am Rand der Sichtweite).
                let (dx, dz) = (i64::from(key.0) - player_cx, i64::from(key.1) - player_cz);
                let in_reichweite = !self.camera_placed || dx * dx + dz * dz <= max_distance_sq;
                if self.world.column(key.0, key.1).is_none() || !in_reichweite {
                    self.mesh_jobs_pending.remove(&key);
                    self.mesh_jobs_stale_on_completion.remove(&key);
                    continue;
                }
                match block_light {
                    Some(l) => {
                        self.column_light.insert(key, l);
                    }
                    None => {
                        self.column_light.remove(&key);
                    }
                }
                gpu.block_renderer.set_column_mesh(&gpu.device, &gpu.queue, key, &opaque, groups, runs, visibility, min_y, &translucent_vertices);
                gpu.block_renderer.sight.set_column(key, min_y, opacity);
                // Ein laufendes Einblenden dieser Spalte gehoert zum ALTEN Puffer. Das neue
                // beginnt mit den Vertices "wie vorher" (s. AoFade) -- im selben Frame, bevor
                // gezeichnet wird.
                self.ao_fades.retain(|f| f.column != key);
                if !fade_quads.is_empty() {
                    for q in &fade_quads {
                        gpu.block_renderer.write_column_vertices(&gpu.queue, key, q.start, &q.before);
                    }
                    self.ao_fades.push(AoFade { column: key, quads: fade_quads, start: Instant::now() });
                }
                // Fuer enforce_render_limits: ab jetzt zaehlt diese Spalte als "hat einen
                // Mesh", auch wenn beide Vertex-Listen leer waren (reine Luft-Spalte --
                // trotzdem korrekt "behandelt", nicht erneut als Kandidat fuer einen
                // Meshing-Job gedacht).
                self.render_limit_meshed.insert(key);
                self.lod.mask_changed = true;
                self.column_meshed_at.entry(key).or_insert_with(Instant::now);
                self.mesh_jobs_pending.remove(&key);
                // Jetzt zeichnet der gewoehnliche Chunk-Pass diese Spalte -- die bestaetigten
                // Geister darin haben ihre Aufgabe erfuellt (s. GhostAwaitingMesh). Der
                // gpu-Borrow oben laeuft noch, deshalb hier nur vormerken.
                fertige_spalten.push(key);
                // Kam waehrend dieses Jobs eine weitere Dirty-Markierung rein (s.
                // mesh_jobs_stale_on_completion-Feldkommentar), ist das gerade eingetroffene
                // Ergebnis mit veralteten Nachbardaten gebaut -- sofort neu einreihen, statt
                // auf die naechste zufaellige Dirty-Markierung von aussen zu warten.
                if self.mesh_jobs_stale_on_completion.remove(&key) {
                    self.world.mark_dirty(key.0, key.1);
                }
            }
        }
        for key in fertige_spalten {
            self.drop_ghosts_of_column(key);
        }

        if self.world.dirty_count() == 0 {
            return;
        }

        // ALLE aktuell offenen Spalten auf einen Schlag in den Pool geben, nicht mehr auf
        // Settings::mesh_columns_per_frame gedeckelt (s. dortigen Kommentar): das Einreihen
        // eines Jobs in den (unbounded) Kanal ist billig -- kein Thread wird dadurch
        // zusaetzlich belegt, das machen ausschliesslich die tatsaechlichen Pool-Worker (s.
        // mesh_pool.rs), begrenzt durch Settings::chunk_update_threads.
        for key in self.world.take_dirty(usize::MAX) {
            // Schon ein Job fuer genau diese Spalte unterwegs? Dann NICHT nochmal einreihen
            // (s. mesh_jobs_pending-Feldkommentar) -- sonst bekaeme jede noch offene Spalte
            // JEDEN Frame einen weiteren redundanten Job, solange enforce_render_limits sie
            // (voellig korrekt) immer wieder dirty markiert, bis ihr erster Job zurueck ist.
            // Die Markierung geht dabei NICHT verloren (s. mesh_jobs_stale_on_completion) --
            // sie wird nur verzoegert, bis der laufende Job fertig ist.
            if !self.mesh_jobs_pending.insert(key) {
                self.mesh_jobs_stale_on_completion.insert(key);
                continue;
            }
            // Nachbarn koennen inzwischen fehlen (z.B. reset_world() zwischen take_dirty()
            // und hier) -- dann verwirft World::column_neighbors() den Job statt mit einer
            // laengst ungueltigen Zielspalte zu rechnen.
            let Some(neighbors) = self.world.column_neighbors(key.0, key.1) else {
                self.mesh_jobs_pending.remove(&key);
                continue;
            };
            let tx = self.mesh_results_tx.clone();
            // Arc-Klone der UV-/Formen-/Biom-Tabellen in den Worker: read-only, kein
            // Kopieren der Daten.
            let uvs = self.block_uvs.clone();
            let shapes = self.block_shapes.clone();
            let biomes = self.biome_palette.clone();
            let cross = self.cross_blocks.clone();
            let cross_extra_shapes = self.cross_extra_shapes.clone();
            // Im Shader-Pfad keine Eckenabdunklung im Mesh: dort kommen Schatten nur von
            // echten Lichtquellen (s. deferred_light.wgsl, fs_light).
            let ao_strength = if crate::shader_path_on() { 0.0 } else { self.settings.smooth_lighting_percent as f32 / 100.0 };
            let show_markers = self.settings.module_show_markers;
            // Licht je Vertex nur fuer den Shader-Pfad (s. world::light) -- der PvP-Pfad meshet
            // wie bisher, ohne einen einzigen Lichtwert zu berechnen.
            let mit_licht = crate::shader_path_on();
            // Gerade bestaetigte eigene Platzierungen in Reichweite dieser Spalte: ihr Schatten
            // blendet im neuen Mesh ein (s. AoFade).
            let (x0, z0) = (key.0 * world::SECTION_SIZE, key.1 * world::SECTION_SIZE);
            // Im Shader-Pfad ohne Einblenden: der Schatten ist sofort da (Nutzerwunsch).
            let fades: Vec<world::AoFadeSource> = self
                .ao_fade_sources
                .iter()
                .filter(|_| !mit_licht)
                .filter(|(p, _, _)| p.0 >= x0 - 1 && p.0 <= x0 + world::SECTION_SIZE && p.2 >= z0 - 1 && p.2 <= z0 + world::SECTION_SIZE)
                .map(|(p, s, _)| (*p, *s))
                .collect();
            self.mesh_pool.spawn(move || {
                let (vertices, translucent_vertices, fade_quads) = world::mesh_column_ao_fading(
                    key.0,
                    key.1,
                    &neighbors,
                    &uvs,
                    &shapes,
                    &biomes,
                    &cross,
                    &cross_extra_shapes,
                    ao_strength,
                    show_markers,
                    &fades,
                );
                // Opakes Gelaende auf das kompakte GPU-Format packen (s. world::terrain_vertex)
                // -- hier im Worker, nicht im Render-Thread. Eingeblendet werden nur opake
                // Flaechen; ihr Anfang wird mit umgerechnet.
                let fade_quads: Vec<world::AoFadeQuad> = fade_quads.into_iter().filter(|q| !q.translucent).collect();
                let mut starts: Vec<u32> = fade_quads.iter().map(|q| q.start).collect();
                let min_y = neighbors.target.min_y;
                let feld = mit_licht.then(|| world::light::light_field(&neighbors));
                let licht_fn = |p: [f32; 3], n: [f32; 3], mitte: [f32; 3], beidseitig: bool| match &feld {
                    Some(f) => world::light::packed_vertex_light(f, &neighbors, p, n, mitte, beidseitig),
                    None => world::TerrainVertex::NO_LIGHT,
                };
                let licht: Option<world::terrain_vertex::VertexLight> = feld.is_some().then_some(&licht_fn as world::terrain_vertex::VertexLight);
                let packed = world::pack_terrain_lit(&vertices, &mut starts, min_y, licht);
                // Nach Richtung geordnet: der Renderer laesst ganze Gruppen weg, die von der
                // Kamera wegzeigen (s. render::BlockRenderer::prepare_terrain_draws).
                let (opaque, groups, runs) = world::group_by_direction(&packed, &mut starts);
                // Sichtverbindungen je Section fuers Hoehlen-Culling (s. render::occlusion).
                let visibility = world::visibility::column_visibility(&neighbors.target.sections, &shapes);
                let opacity = world::visibility::column_opacity(&neighbors.target.sections, &shapes).into();
                let fade_quads: Vec<world::TerrainFadeQuad> = fade_quads
                    .iter()
                    .zip(starts)
                    .filter(|(_, start)| *start != u32::MAX)
                    .map(|(q, start)| world::TerrainFadeQuad {
                        start,
                        before: world::pack_terrain_lit(&q.before, &mut [], min_y, licht),
                        after: world::pack_terrain_lit(&q.after, &mut [], min_y, licht),
                    })
                    .collect();
                // Wasser/Glas ebenso kompakt (ohne Richtungsgruppen, s. render::TranslucentMesh).
                let translucent_vertices = world::pack_terrain_lit(&translucent_vertices, &mut [], min_y, licht);
                let block_light = feld.as_ref().and_then(world::light::LightField::column_block_light);
                let _ = tx.send(MeshResult { key, opaque, groups, runs, visibility, opacity, min_y, translucent: translucent_vertices, fade_quads, block_light });
            });
        }
    }

    // Client-seitige Render-Limiter (s. MAX_RENDER_DISTANCE_MIN/MAX-Kommentar): entscheidet,
    // welche geladenen Spalten TATSAECHLICH gemesht/gezeichnet werden. Laeuft jeden Frame vor
    // mesh_pending_columns -- billig genug (linearer Scan + Sortierung ueber hoechstens ein
    // paar tausend Spalten), um jeden Frame neu zu entscheiden, statt nur bei
    // Chunk-Grenzuebertritt: kein Sonderfall fuer "wann genau pruefen".
    // Sichtweite in Chunks: die Einstellung, solange die Launcher-Szene steht deren eigene (s.
    // hero_scene.rs).
    pub(crate) fn render_distance(&self) -> u32 {
        self.hero_render_distance.unwrap_or(self.settings.max_render_distance)
    }

    pub(crate) fn enforce_render_limits(&mut self) {
        self.world.light_remesh = crate::shader_path_on();
        world::mesher::set_flat_face_shade(crate::shader_path_on());
        // Vor der ersten Positions-Synchronisierung (s. ClientEvent::PlayerPosition) gibt es
        // keine verlaessliche Spielerposition -- dann lieber gar nicht limitieren, als mit
        // (0,0) zu rechnen und den Startchunk faelschlich zu bevorzugen.
        if !self.camera_placed {
            return;
        }
        let Some(gpu) = self.gpu.as_mut() else { return };
        // Hoehlen-Culling: Suchradius = Sichtweite (eine Spalte Rand dazu).
        // Nicht als Zuschauer: der steckt oft mitten im Fels, und von dort aus ist fuer die
        // Suche nichts erreichbar -- man saehe in der Hoehle nichts.
        gpu.block_renderer.cave_culling = self.settings.cave_culling && self.game_mode != GAME_MODE_SPECTATOR;
        gpu.block_renderer.mipmapping = self.settings.mipmapping;
        gpu.block_renderer.cull_radius = self.hero_render_distance.unwrap_or(self.settings.max_render_distance) as i32 + 1;

        let player_cx = (self.physics.pos.x / f64::from(world::SECTION_SIZE)).floor() as i32;
        let player_cz = (self.physics.pos.z / f64::from(world::SECTION_SIZE)).floor() as i32;

        // Quadrierter euklidischer Abstand statt Chebyshev (max(|dx|,|dz|)) -- Letzteres
        // ergibt ein QUADRAT (jede Spalte in der (2R+1)x(2R+1)-Box zaehlt gleich weit, auch
        // die Ecken bei vollem Diagonalabstand), sichtbar als eckige statt runde Sichtweiten-
        // Grenze. Quadriert statt Wurzel gezogen: spart sqrt() pro Spalte
        // und bleibt trotzdem exakt (a <= b^2 ist aequivalent zu sqrt(a) <= b fuer a,b >= 0),
        // Sortierschluessel bleibt eine reine Ganzzahl.
        let max_distance = i64::from(self.hero_render_distance.unwrap_or(self.settings.max_render_distance));
        let max_distance_sq = max_distance * max_distance;

        // Nach Distanz aufsteigend, (cx, cz) als fester Tiebreaker: so werden die naechsten
        // Spalten zuerst dirty markiert und damit zuerst gemesht.
        //
        // NEU SORTIERT wird nur, wenn sich die Reihenfolge ueberhaupt aendern kann: bei
        // Spaltenwechsel des Spielers (alle 16 Bloecke) oder wenn Spalten geladen/entladen
        // wurden (World::column_generation). Vorher lief das JEDEN Frame -- bei Sichtweite 24
        // eine Vec-Allokation ueber ~2400 Spalten plus ~27.000 Sortiervergleiche, obwohl das
        // Ergebnis fast immer identisch war. Die SCHLEIFE unten laeuft weiterhin jeden Frame:
        // sie muss es, s. den clear_dirty-Kommentar dort.
        //
        // Abstand in i64: Chunk-Koordinaten reichen bis ~1.9 Mio (Weltgrenze), quadriert
        // laege das weit ueber i32::MAX -- im Release haette das still falsch sortiert, im
        // Debug-Build abgebrochen. Betrifft weit entfernte, noch geladene Spalten (Teleport).
        let order_key = (player_cx, player_cz, self.world.column_generation());
        if self.render_limit_order_key != Some(order_key) {
            self.render_limit_order_key = Some(order_key);
            self.render_limit_order.clear();
            self.render_limit_order.extend(self.world.column_keys().map(|(cx, cz)| {
                let dx = i64::from(cx) - i64::from(player_cx);
                let dz = i64::from(cz) - i64::from(player_cz);
                (dx * dx + dz * dz, cx, cz)
            }));
            self.render_limit_order.sort_unstable();
        }
        // Herausnehmen und danach zurueckgeben: die Schleife leiht sich `self` mutable
        // (world/render_limit_meshed/gpu), waehrend sie ueber die Liste laeuft. mem::take
        // loest das ohne jede Allokation -- der Puffer selbst bleibt ueber Frames erhalten.
        let columns = std::mem::take(&mut self.render_limit_order);

        // mark_dirty() NUR fuer noch nicht gemeshte Spalten im Limit -- sonst wuerde eine
        // bereits fertige Spalte JEDEN Frame erneut als dirty markiert und liefe in einer
        // Endlosschleife immer wieder durch den Mesh-Pool. Die Liste ist nach Abstand sortiert:
        // ab der ersten Spalte ausserhalb kommt nichts mehr im Limit.
        for &(d, cx, cz) in &columns {
            if d > max_distance_sq {
                break;
            }
            if !self.render_limit_meshed.contains(&(cx, cz)) {
                self.world.mark_dirty(cx, cz);
            }
        }
        self.render_limit_order = columns;

        // Ausserhalb des Limits JEDEN Frame aufraeumen, nicht nur bei Spaltenwechsel -- sonst
        // koennte eine gerade erst dirty markierte Spalte (Job eingereiht, aber noch nicht
        // fertig) aus dem Limit fallen und ihr Job liefe trotzdem noch durch
        // (render_limit_meshed weiss ja erst NACH dem Job davon). Aber nur dort, wo wirklich
        // etwas steht: Dirty-Menge, gemeshte Spalten, Mesh-Zeitpunkte. Vorher wurde fuer JEDE
        // geladene Spalte ausserhalb in alle drei gegriffen -- mit grosser Server-Sichtweite
        // tausende Hash-Loeschungen je Frame, fast alle ins Leere (Profil: 5,8 % des
        // Haupt-Threads).
        let ausserhalb = |(cx, cz): (i32, i32)| {
            let dx = i64::from(cx) - i64::from(player_cx);
            let dz = i64::from(cz) - i64::from(player_cz);
            dx * dx + dz * dz > max_distance_sq
        };
        self.world.retain_dirty(|p| !ausserhalb(p));
        if self.render_limit_meshed.iter().any(|&p| ausserhalb(p)) {
            let raus: Vec<(i32, i32)> = self.render_limit_meshed.iter().copied().filter(|&p| ausserhalb(p)).collect();
            for p in raus {
                self.render_limit_meshed.remove(&p);
                self.lod.mask_changed = true;
                gpu.block_renderer.remove_column_mesh(p);
            }
        }
        self.column_meshed_at.retain(|&p, _| !ausserhalb(p));
    }
}
