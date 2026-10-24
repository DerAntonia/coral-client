// Debug-Overlay, Verbindungsanzeige, Tearing-Steuerung und Profiler.

use crate::*;

impl App {
    // F3-Debug-Overlay: oben links, wie in Vanilla. Zeigt nur Werte, die dieser Client
    // tatsaechlich fuehrt (kein Nachbau von Vanilla-Feldern wie Biom-Registry-ID oder
    // Ticket-System, die es hier gar nicht gibt) -- Position/Chunk/Blickrichtung aus der
    // Physik/Kamera, plus die Welt-Zaehler, die schon beim Chunk-Meshing-Debugging dieser
    // Sitzung nuetzlich waren (s. cols/dirty/meshed).
    // Diagnose-Block fuers F3-Overlay: wie alt/wie weit auseinander die drei Positionen eines
    // fremden Spielers sind. Gemessen wird am NAECHSTEN Spieler -- der ist im PvP der
    // relevante, und anders als ein Raycast-Ziel muss man ihn nicht erst treffen, damit die
    // Anzeige etwas zeigt.
    //
    // Was hier NICHT steht, und warum: die echte Netz-Laufzeit (RTT) ist client-seitig im
    // Play-State nicht messbar. Sowohl KeepAlive als auch Ping/Pong gehen vom SERVER aus --
    // wir sind immer der Antwortende, nie der Fragende, es gibt also keinen eigenen
    // Zeitstempel, gegen den wir rechnen koennten. Alles unten ist deshalb die Verzoegerung
    // AB Paketankunft; die Laufzeit davor kommt additiv obendrauf.
    // Bruchteil des laufenden Physik-Ticks (0..1) -- der Wert, mit dem alles Tick-Gebundene
    // zwischen zwei Ticks gleitet (eigene Position, Entities, Animationen).
    pub(crate) fn tick_progress(&self) -> f32 {
        (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0)
    }

    // Shader-Pfad: derselbe Sonnenstrahl wie im Licht-Pass, hier auf der CPU durch die echten
    // Blockdaten -- ab der angeschauten Stelle. Trifft er hier einen Block, die Stelle ist im
    // Bild aber hell, liegt der Fehler im Shader; trifft er nichts, wirft das, was dort steht,
    // keinen Schatten (Display-Entities, Blockentities, Pflanzen).
    pub(crate) fn sun_ray_debug_lines(&self) -> Vec<String> {
        if self.settings.render_path != 1 {
            return Vec::new();
        }
        let sonne = render::shader_path::celestial_light(world::sky::sky_angle(self.world_time_of_day));
        let d = sonne.dir.as_dvec3();
        let kopf = format!(
            "-- Sonne: Tageszeit {} | Richtung {:.2} {:.2} {:.2} --",
            self.world_time_of_day.rem_euclid(world::sky::DAY_TICKS),
            d.x,
            d.y,
            d.z
        );
        let eye = self.physics.eye_pos();
        let blick = self.camera.forward().as_dvec3();
        let Some(hit) = physics::raycast_block(eye, blick, 64.0, &self.world, &self.block_shapes) else {
            return vec![kopf, "Sonnenstrahl: kein Block im Blick".to_string()];
        };
        let normale = match hit.face {
            0 => DVec3::NEG_Y,
            1 => DVec3::Y,
            2 => DVec3::NEG_Z,
            3 => DVec3::Z,
            4 => DVec3::NEG_X,
            _ => DVec3::X,
        };
        let start = DVec3::new(hit.block.0 as f64, hit.block.1 as f64, hit.block.2 as f64)
            + DVec3::new(hit.cursor.0 as f64, hit.cursor.1 as f64, hit.cursor.2 as f64)
            + normale * 0.02;
        if normale.dot(d) <= 0.0 {
            return vec![kopf, format!("Sonnenstrahl: Fläche {:?} zeigt von der Sonne weg", hit.block)];
        }
        let mut t = 0.0;
        while t < 160.0 {
            let p = start + d * t;
            let b = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let state = self.world.chunk_block_at(b.0, b.1, b.2);
            if state != world::STATE_AIR {
                let name = world::block_name(state).unwrap_or("?");
                let lokal = p - DVec3::new(b.0 as f64, b.1 as f64, b.2 as f64);
                let drin = !self.block_shapes.has_resolved_shape(state)
                    || self.block_shapes.shapes_of(state).iter().any(|s| (0..3).all(|a| lokal[a] >= s.min[a] as f64 && lokal[a] <= s.max[a] as f64));
                if drin {
                    let wirft = if self.block_shapes.has_resolved_shape(state) || name == "water" || name == "lava" { "" } else { " (wirft im Shader KEINEN Schatten)" };
                    return vec![kopf, format!("Sonnenstrahl ab {:?}: trifft {name} bei {:?} nach {t:.1} Blöcken{wirft}", hit.block, b)];
                }
            }
            t += 0.05;
        }
        vec![kopf, format!("Sonnenstrahl ab {:?}: frei bis zum Himmel", hit.block)]
    }

    pub(crate) fn entity_latency_lines(&self) -> Vec<String> {
        let now = Instant::now();
        let eye = self.physics.pos;
        let abstand = |e: &world::Entity| {
            let (x, y, z) = e.pos;
            (x - eye.x).powi(2) + (y - eye.y).powi(2) + (z - eye.z).powi(2)
        };
        let Some((_, e)) = self
            .entities
            .iter()
            .filter(|(_, e)| world::is_player(e.entity_type))
            .min_by(|(_, a), (_, b)| abstand(a).total_cmp(&abstand(b)))
        else {
            return vec!["Entity-Latenz: kein anderer Spieler in Reichweite".to_string()];
        };

        let (rx, ry, rz) = e.render_pos(self.tick_progress());
        let (hx, hy, hz) = e.hit_pos();
        let (sx, sy, sz) = e.target_pos();
        let strecke = |a: (f64, f64, f64), b: (f64, f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt();

        let takt = match e.update_interval_ms() {
            Some((last, avg, max)) => format!("letzter {last:.0}ms  Mittel {avg:.0}ms  max {max:.0}ms"),
            None => "noch kein Abstand gemessen (Entity steht still)".to_string(),
        };

        vec![
            format!("-- Entity-Latenz (nächster Spieler, {:.1} Blöcke entfernt) --", abstand(e).sqrt()),
            format!("Paket-Takt: {takt}"),
            format!("Alter der Serverposition: {:.0}ms", e.position_age_ms(now)),
            format!("Bild↔Trefferbox: {:.3} Blöcke", strecke((rx, ry, rz), (hx, hy, hz))),
            format!("Bild↔Serverpos:  {:.3} Blöcke", strecke((rx, ry, rz), (sx, sy, sz))),
        ]
    }

    // Die Spalte im Blick: der Strahl laeuft bis 128 Bloecke; die erste Spalte, der etwas fehlt
    // (Daten, Mesh, vom Hoehlen-Culling ganz ausgeblendet), sonst die mit dem ersten Block.
    pub(crate) fn look_chunk_debug_line(&self, gpu: &GpuState) -> String {
        let start = self.camera.position;
        let dir = self.camera.forward().as_dvec3();
        let br = &gpu.block_renderer;
        let mut ziel = None;
        let mut letzte = None;
        for i in 0..512 {
            let p = start + dir * (i as f64 * 0.25);
            let key = ((p.x.floor() as i32).div_euclid(16), (p.z.floor() as i32).div_euclid(16));
            if letzte != Some(key) {
                letzte = Some(key);
                let (_, sichtbar) = br.column_debug(key);
                if self.world.column(key.0, key.1).is_none() || !br.has_column_mesh(key) || sichtbar == 0 {
                    ziel = Some(key);
                    break;
                }
            }
            if self.world.block_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) != 0 {
                ziel = Some(key);
                break;
            }
        }
        let Some(key) = ziel.or(letzte) else { return String::new() };
        let ja = |b: bool| if b { "ja" } else { "NEIN" };
        let (sections, sichtbar) = br.column_debug(key);
        let vor = |t: Option<Instant>| t.map_or("nie".to_string(), |t| format!("vor {:.1} s", t.elapsed().as_secs_f32()));
        let (empfangen, vergessen, kaputt) = self.column_history.get(&key).copied().unwrap_or_default();
        format!(
            "Blick-Chunk {} {}: Daten {} | Mesh {} ({} Sections, {} sichtbar) | dirty {} | empfangen {} | vergessen {} | unlesbar {} | Welt-Reset {}",
            key.0,
            key.1,
            ja(self.world.column(key.0, key.1).is_some()),
            ja(br.has_column_mesh(key)),
            sections.count_ones(),
            (sections & sichtbar).count_ones(),
            ja(self.world.is_dirty(key.0, key.1)),
            vor(empfangen),
            vor(vergessen),
            vor(kaputt),
            self.world_reset_debug.map_or("nie".to_string(), |(t, grund)| format!("{} ({grund})", vor(Some(t)))),
        )
    }

    pub(crate) fn build_debug_overlay(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, hud_lines: &mut Vec<UiLine>, texts: &mut Vec<UiText>) {
        let pos = self.physics.pos;
        let block_x = pos.x.floor() as i64;
        let block_y = pos.y.floor() as i64;
        let block_z = pos.z.floor() as i64;
        let section_size = world::SECTION_SIZE as i64;
        let chunk_x = block_x.div_euclid(section_size);
        let chunk_z = block_z.div_euclid(section_size);
        let local_x = block_x.rem_euclid(section_size);
        let local_z = block_z.rem_euclid(section_size);

        let yaw_deg = self.camera.yaw.rem_euclid(360.0);
        let pitch_deg = self.camera.pitch;
        // Vanilla-Formel fuer die Blickrichtung aus dem Yaw (MathHelper.floor(yaw*4/360+0.5)
        // & 3), s. AbstractClientPlayerEntity/HungerManager-Debug-HUD-Vorbild: yaw 0=Sued,
        // 90=West, 180=Nord, 270=Ost, da unsere Kamera (s. camera.rs::forward) exakt dieser
        // Konvention folgt.
        let facing = match ((yaw_deg * 4.0 / 360.0 + 0.5).floor() as i64).rem_euclid(4) {
            0 => "Süd (+Z)",
            1 => "West (-X)",
            2 => "Nord (-Z)",
            _ => "Ost (+X)",
        };

        let state = if self.physics.swimming {
            "Schwimmen"
        } else if self.physics.crouching {
            "Schleichen"
        } else if self.physics.sprinting {
            "Sprinten"
        } else {
            "Gehen"
        };

        let mut lines = vec![
            format!("Coral-Client-Debug (F3)  |  {} FPS", self.frame_stats.fps()),
            format!("XYZ: {:.3} / {:.3} / {:.3}", pos.x, pos.y, pos.z),
            format!("Block: {block_x} {block_y} {block_z}"),
            format!("Chunk: {chunk_x} {chunk_z}  (rel {local_x} {local_z} im Chunk)"),
            format!("Blick: {facing}  Yaw={yaw_deg:.1}  Pitch={pitch_deg:.1}"),
            format!("Perspektive (F5): {}  |  FOV {:.0}", self.perspective.hud_label(), self.settings.fov_of(self.perspective)),
            format!("Zustand: {state}  |  am Boden: {}", self.physics.on_ground),
            format!(
                "Welt: {} Spalten geladen, {} im Mesh-Pool, {} gemesht",
                self.world.column_count(),
                self.mesh_jobs_pending.len(),
                self.render_limit_meshed.len(),
            ),
            format!(
                "Chunks: {} empfangen, {} gemesht  |  Mesh-VRAM: {:.1} MB",
                self.chunks_received,
                gpu.block_renderer.mesh_count(),
                gpu.block_renderer.vertex_bytes() as f32 / (1024.0 * 1024.0),
            ),
            match self.gpu.as_ref().map(|g| g.block_renderer.culling_stats) {
                Some((gesamt, gezeichnet)) if self.settings.cave_culling => {
                    format!("Gelaende: {gezeichnet} von {gesamt} Abschnitten gezeichnet (Sichtkegel + Hoehlen-Culling)")
                }
                Some((gesamt, gezeichnet)) => format!("Gelaende: {gezeichnet} von {gesamt} Abschnitten gezeichnet (Hoehlen-Culling AUS)"),
                None => String::new(),
            },
            self.lod_debug_line(),
            self.look_chunk_debug_line(gpu),
            self.transactions.debug_line(Instant::now()),
            // Welche Partikel gerade leben -- so laesst sich sagen, woher ein unerwarteter
            // Partikel stammt.
            {
                let arten = self.particles.type_counts();
                let gesamt: usize = arten.iter().map(|(_, n)| n).sum();
                let liste: Vec<String> = arten.iter().take(6).map(|(name, n)| format!("{name} {n}")).collect();
                format!("Partikel: {gesamt}  ({})", liste.join(", "))
            },
            format!(
                "Entities: {}  |  Positions-Updates: {}/s  |  Server: {}",
                self.entities.len(),
                self.entity_update_stats.per_second(),
                self.server_address
            ),
            // Weltton: die Aufnahmen kommen erst beim ersten Vorkommen vom Datentraeger
            // (s. vanilla_sounds), die Zahl waechst also im Spiel. Bleibt sie bei 0, obwohl
            // etwas passiert, hakt es zwischen Paket und Datei -- und genau das sieht man
            // sonst nirgends, weil ein fehlender Klang keine Fehlermeldung ist.
            if self.vanilla_sounds.is_ready() {
                format!("Ton: {} Aufnahmen geladen  |  Hauptlautstärke {}", self.vanilla_sounds.cached(), volume_text(self.settings.master_volume))
            } else {
                "Ton: sounds.json nicht gelesen -- Weltgeräusche stumm".to_string()
            },
            // Die eigenen Statuseffekte im Klartext. Sie sind sonst NIRGENDS sichtbar: der
            // Client zeichnet keine Effektsymbole, und ein falsch geglaubter Effekt aeussert
            // sich nur darin, dass man sich anders bewegt als man sollte -- also genau als
            // das Gefuehl "irgendwas stimmt nicht", ohne jeden Anhaltspunkt.
            self.effect_debug_line(),
            // Was der Server als Trank-Partikel des eigenen Spielers meldet (Farben, sonst
            // "keine") und wie viele andere Entities gerade welche streuen -- die Wirbel kommen
            // allein aus dieser Liste, nicht aus den Effekten oben.
            {
                let eigene: Vec<String> = self.own_potion_swirls.iter().map(|argb| format!("#{:06X}/{}", argb & 0xFF_FFFF, argb >> 24)).collect();
                // Je streuendes Entity: Typ, Abstand, ob unsichtbar, und die Farben, wie der Server
                // sie meldet -- die naechsten drei.
                let ich = self.physics.pos;
                let mut fremde: Vec<(f64, String)> = self
                    .entities
                    .iter()
                    .filter(|(_, e)| !e.potion_swirls.is_empty())
                    .map(|(_, e)| {
                        let (x, y, z) = e.pos;
                        let d = DVec3::new(x, y, z).distance(ich);
                        let farben: Vec<String> = e.potion_swirls.iter().map(|argb| format!("#{:06X}/{}", argb & 0xFF_FFFF, argb >> 24)).collect();
                        let typ = world::entity_type_name(e.entity_type).unwrap_or("?");
                        (d, format!("{typ} {d:.1}m{} {}", if e.invisible { " unsichtbar" } else { "" }, farben.join(" ")))
                    })
                    .collect();
                fremde.sort_by(|a, b| a.0.total_cmp(&b.0));
                let liste: Vec<String> = fremde.iter().take(3).map(|(_, t)| t.clone()).collect();
                format!(
                    "Trank-Partikel: eigene {}{}  |  andere ({}): {}",
                    if eigene.is_empty() { "keine".to_string() } else { eigene.join(" ") },
                    if self.own_potion_swirls_ambient { " (Leuchtfeuer)" } else { "" },
                    fremde.len(),
                    if liste.is_empty() { "keine".to_string() } else { liste.join("; ") }
                )
            },
        ];
        lines.extend(self.sun_ray_debug_lines());
        lines.extend(self.entity_latency_lines());

        let line_height = gpu.text_height(UI_SCALE_SMALL) + 2.0;
        for (row, line) in lines.iter().enumerate() {
            texts.push(UiText { text: line.clone(), x: 6.0, y: 6.0 + row as f32 * line_height, color: COLOR_WHITE, scale: UI_SCALE_SMALL });
        }

        // Graphen unten rechts, von oben: GPU, RAM (mit Swap), CPU. Gruen = Rechner gesamt,
        // pink = Coral (s. sysstats). Hier nur die Kurven; Flaechen und Ueberschriften liegen in
        // der Overlay-Ebene (s. debug_graph_overlay), damit alles ueber dem Scoreboard steht.
        let _ = rects;
        let stats = self.sys_stats.snapshot();
        for (area, serie) in debug_graph_areas(gpu).into_iter().zip([&stats.cpu, &stats.ram, &stats.gpu]) {
            push_stats_graph(hud_lines, area, serie);
        }
    }

    // Flaechen und Ueberschriften der F3-Graphen als unterste Overlay-Gruppe: ueber dem
    // normalen HUD-Text (Scoreboard), unter Benachrichtigungen.
    pub(crate) fn debug_graph_overlay(&self, overlay: &mut UiOverlay) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        if !(self.debug_overlay && self.mode == AppMode::World && !self.hud_hidden) || self.connecting_view() {
            return;
        }
        let stats = self.sys_stats.snapshot();
        let (mut rects, mut texts) = (Vec::new(), Vec::new());
        for (area, (titel, serie)) in debug_graph_areas(gpu).into_iter().zip([("CPU", &stats.cpu), ("RAM", &stats.ram), ("GPU", &stats.gpu)]) {
            push_stats_graph_frame(gpu, &mut rects, &mut texts, area, titel, serie);
        }
        overlay.groups.push(UiOverlayGroup { rects, texts, sprites: Vec::new(), pivot: (0.0, 0.0), angle: 0.0 });
    }

    // Verbindungs-Overlay (World-Modus, bevor der erste Chunk da ist): Titel + Status +
    // die letzten Log-Zeilen, mittig, als Phasen-Anzeige. Verschwindet mit dem ersten Chunk.
    pub(crate) fn build_connecting_overlay(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        let (sw, sh) = gpu.size();
        // Abgedunkelt wie die Welt hinter dem Pausemenue.
        let dunkel = self.settings.menu_backdrop_opacity.min(100) as f32 / 100.0;
        rects.push(UiRect { rect: Rect { x: 0.0, y: 0.0, w: sw, h: sh }, color: [0.0, 0.0, 0.0, dunkel], ..UiRect::FLAT });
        let top = (sh * 0.32).round();
        // Ueberschrift wie bei LiquidBounce: wohin die Verbindung wirklich geht (SRV-Ziel, IP,
        // Port). Bis die TCP-Verbindung steht, die eingegebene Adresse.
        let titel = match &self.conn_route {
            Some(ziel) => ziel.clone(),
            None => format!("Verbinde mit {} ...", self.server_address),
        };
        texts.push(gpu.centered_text(Rect { x: 0.0, y: top, w: sw, h: 44.0 }, titel, theme::text(), UI_SCALE_NORMAL));
        texts.push(gpu.centered_text(
            Rect { x: 0.0, y: top + 48.0, w: sw, h: 28.0 },
            format!("Status: {}", self.conn_status.label()),
            self.conn_status.color(),
            UI_SCALE_SMALL,
        ));
        // Letzte Log-Zeilen als Connect-Phasen-Anzeige (Handshake, Login, Konfiguration, ...).
        let start = self.log_lines.len().saturating_sub(OVERLAY_LOG_LINES);
        let base_y = top + 88.0;
        for (row, line) in self.log_lines[start..].iter().enumerate() {
            texts.push(gpu.centered_text(
                Rect { x: 0.0, y: base_y + row as f32 * LOG_LINE_HEIGHT, w: sw, h: LOG_LINE_HEIGHT },
                line.clone(),
                theme::text(),
                UI_SCALE_SMALL,
            ));
        }
        let r = connect_cancel_rect();
        let b = Button {
            slider: None,
            action: ButtonAction::Nothing,
            rect: r,
            label: String::new(),
            enabled: true,
            color: theme::get(theme::Slot::Surface2),
            scale: UI_SCALE_SMALL,
        };
        let (flaeche, farbe) = styled_button(&b, self.hover_level(r, r.contains(self.cursor_pos.0, self.cursor_pos.1)));
        rects.push(flaeche);
        texts.push(gpu.centered_text(r, "Abbrechen".to_string(), farbe, UI_SCALE_SMALL));
    }

    // Abbrechen-Knopf im Verbindungs-Overlay: trennt immer -- auch wenn der Verbindungs-Task
    // schon weg ist und nur die Ansicht noch auf den ersten Chunk wartet.
    pub(crate) fn cancel_connect(&mut self) {
        self.play_ui_sound(ui_sound::UiSound::Close);
        if self.conn_task.is_some() {
            self.disconnect();
            return;
        }
        self.keep_last_view();
        self.conn_status = ConnStatus::Disconnected;
        self.reset_world();
        self.mode = AppMode::Menu;
        self.screen = MenuScreen::Connect;
        self.push_log("== getrennt (vom Nutzer) ==");
    }

    // Tearing-Test-Balken (s. Settings::show_tearing_test), falls eingeschaltet. Im Menue UND im
    // Spiel: Tearing faellt gerade bei schnellen Kameradrehungen auf, und genau dort muss der
    // Test mitlaufen, um TearFree und die Riss-Steuerung unter echter Last zu pruefen.
    // Riss-Steuerung (Settings::tear_position): nur, wenn wir selbst takten (bei Fifo reisst
    // nichts) und VBlank-Zeitstempel da sind.
    pub(crate) fn tear_control_active(&self) -> bool {
        self.settings.tear_position != 0
            && self.vblank.as_ref().is_some_and(|v| v.latest().is_some())
            && !self.gpu.as_ref().is_some_and(|gpu| gpu.present_mode_self_paces())
    }

    // Wo im Bildaufbau (ns seit Beginn des Auslesens von Zeile 0) der Bildwechsel liegen soll.
    // Die Austastluecke ist geschaetzt: die Riss-Steuerung kennt nur die sichtbare Hoehe, und die
    // uebliche 1080p-Timing (CEA/CVT) hat 45 Zeilen Luecke auf 1080 sichtbare -- bei anderen
    // Timings liegt "Unten"/"Austastluecke" um ein paar Zeilen daneben.
    pub(crate) fn tear_target_phase(&self, period_ns: f64) -> Option<f64> {
        const BLANK_LINES: f64 = 45.0;
        // Abstand zum Rand: die Streuung des Bildwechsels liegt bei ~0,1 ms. Ein Fehler zur
        // Luecke hin ist unsichtbar, einer ins Bild hinein laege nur wenige Zeilen tief.
        const MARGIN_NS: f64 = 150_000.0;
        // Mit den echten Zeilen des Bildmodus, sonst geschaetzt.
        let visible = match self.vblank.as_ref().and_then(|v| v.lines) {
            Some((sichtbar, gesamt)) => period_ns * sichtbar as f64 / gesamt as f64,
            None => {
                let height = self.gpu.as_ref()?.size().1 as f64;
                period_ns * height / (height + BLANK_LINES)
            }
        };
        let sichtbar_ziel = match self.settings.tear_position {
            1 => visible - MARGIN_NS,
            _ => return None,
        };
        Some((sichtbar_ziel + self.settings.tear_timing_us as f64 * 1000.0).rem_euclid(period_ns))
    }

    // Wie viele Monitorbilder ein Frame mit Riss-Steuerung dauert: aus dem FPS-Limit gerundet
    // (120 bei 120 Hz = 1, 60 = 2), mindestens 1.
    pub(crate) fn tear_step_periods(&self, period_ns: f64) -> f64 {
        let base = self.effective_frame_interval();
        if base.is_zero() { 1.0 } else { (base.as_secs_f64() * 1e9 / period_ns).round().max(1.0) }
    }

    // Takt mit Riss-Steuerung (statt des gewoehnlichen Limiters in about_to_wait): jeder Frame
    // hat einen festen Praesentations-Termin -- letzter VBlank + Zielphase + ganze Perioden --,
    // und wird um tear_lead_ns frueher angefangen. submit_and_present wartet dann auf die GPU
    // und bis genau auf den Termin.
    //
    // Frueher regelte hier ein Phasenregler den Frame-ANFANG nach; der Bildwechsel folgte dann
    // aber weiter der schwankenden Frame-Arbeit (gemessen ±0,16 ms) -- an der nur 0,33 ms
    // breiten Austastluecke genug, dass der Riss zwischen oberem und unterem Rand sprang.
    pub(crate) fn schedule_tear_frame(&mut self, event_loop: &ActiveEventLoop) {
        let Some((last, period)) = self.vblank.as_ref().and_then(|v| v.latest()) else { return };
        let Some(target) = self.tear_target_phase(period) else { return };
        let base = last.ts_ns as f64 + target;
        let slot_after = |t: i64| -> i64 { (base + ((t as f64 - base) / period).ceil() * period) as i64 };
        let now_ns = vblank::monotonic_ns();
        let lead = self.tear_lead_ns as i64;
        let mut slot = self.tear_slot_ns.unwrap_or_else(|| slot_after(now_ns + lead));
        // Termin nicht mehr erreichbar (Ruckler, Fokuswechsel): den naechsten erreichbaren nehmen.
        if now_ns > slot - lead / 2 {
            slot = slot_after(now_ns + lead);
        }
        if now_ns >= slot - lead {
            self.tear_present_at = Some(slot);
            if let Some(gpu) = &self.gpu {
                gpu.window.request_redraw();
            }
            let step = self.tear_step_periods(period) * period;
            // Neu aus dem letzten VBlank gerechnet statt aufaddiert -- so laeuft nichts weg.
            slot = slot_after(slot + (step - period / 2.0) as i64);
        }
        self.tear_slot_ns = Some(slot);
        let wait = (slot - lead - now_ns).max(0) as u64;
        self.next_frame_time = Instant::now() + Duration::from_nanos(wait);
        event_loop.set_control_flow(ControlFlow::WaitUntil(next_wake(self.next_frame_time, self.tick_deadline)));
    }

    // Vorlauf der Riss-Steuerung nachfuehren: der Frame soll mit der eingestellten Reserve
    // (Settings::tear_reserve_us) vor seinem Termin fertig sein. Knapp daneben (unter einem
    // Drittel der Reserve oder zu spaet) sofort deutlich mehr, sonst langsam zurueck -- jede
    // Millisekunde Vorlauf ist eine Millisekunde Latenz.
    pub(crate) fn adapt_tear_lead(&mut self, ready_ns: i64, present_at_ns: i64) {
        let want = self.settings.tear_reserve_us as f64 * 1000.0;
        let slack = (present_at_ns - ready_ns) as f64;
        if slack < want / 3.0 {
            self.tear_lead_ns += want - slack;
        } else {
            self.tear_lead_ns -= (slack - want) * 0.02;
        }
        // Untergrenze klein genug, dass auch eine kleine Reserve wirkt (ein Frame braucht bei
        // uns ~1,5 ms von Beginn bis GPU fertig -- das regelt der Vorlauf von selbst ein).
        const MIN_LEAD_NS: f64 = 500_000.0;
        let period = self.vblank.as_ref().and_then(|v| v.latest()).map_or(8_333_333.0, |(_, p)| p);
        let max = self.tear_step_periods(period) * period - 500_000.0;
        self.tear_lead_ns = self.tear_lead_ns.clamp(MIN_LEAD_NS, max.max(MIN_LEAD_NS));
    }

    // Messungen nur, solange sie jemand braucht: die VBlanks fuer Scanout-Timing, Profiler und
    // die Render-Seite (Zeile Compositing), die Beobachtung des Fensters nur fuer die
    // Render-Seite. Danach enden ihre Threads (sie laufen nur, solange jemand den Stand haelt).
    pub(crate) fn update_system_watch(&mut self) {
        let render_seite = self.mode == AppMode::Menu && self.screen == MenuScreen::SettingsGeneral;
        if self.settings.tear_position != 0 || self.profiler.active() || render_seite {
            self.ensure_vblank();
        } else if self.vblank.is_some() {
            self.vblank = None;
            self.vblank_tried = false;
            self.profiler.set_vblank(None);
        }
        if !render_seite {
            self.window_path = None;
            self.window_path_tried = false;
        } else if !self.window_path_tried {
            self.window_path_tried = true;
            self.window_path = self.gpu.as_ref().and_then(|g| x11_window_id(&g.window)).and_then(system_info::watch_window);
        }
    }

    // VBlank-Messung einmal starten (Profiler oder Riss-Steuerung brauchen sie).
    pub(crate) fn ensure_vblank(&mut self) -> Option<std::sync::Arc<vblank::VblankMonitor>> {
        if !self.vblank_tried {
            self.vblank_tried = true;
            // Der Bildschirm unter dem Fenster -- die Messung soll GENAU dessen Takt folgen.
            let ziel = self.gpu.as_ref().and_then(|g| g.window.current_monitor()).map(|m| vblank::Target {
                x: m.position().x,
                y: m.position().y,
                w: m.size().width,
                h: m.size().height,
                refresh_mhz: m.refresh_rate_millihertz(),
            });
            self.vblank = vblank::start(ziel);
            match &self.vblank {
                Some(v) => self.push_log(format!("VBlank-Messung über {}", v.source)),
                None => self.push_log("Keine VBlank-Zeitstempel (kein Zugriff auf /dev/dri/card*?)"),
            }
        }
        self.vblank.clone()
    }

    pub(crate) fn toggle_profiler(&mut self) {
        let monitor = self.ensure_vblank();
        self.profiler.set_vblank(monitor);
        let path = shared_config_dir().map(|dir| dir.join("frame_profile.csv"));
        let csv = self.profiler.toggle(path);
        if self.profiler.active() {
            let wohin = csv.map(|p| p.display().to_string()).unwrap_or_else(|| "keine CSV-Datei".into());
            self.push_log(format!("Frame-Profiler an, schreibt {wohin}"));
            self.notify(notifications::Notification::success("Frame-Profiler an".to_string(), wohin));
        } else {
            self.push_log("Frame-Profiler aus");
        }
    }

    // Das Frame-Intervall, das wir selbst anpeilen -- None, wenn der Present-Modus taktet
    // (Fifo) oder kein Deckel gesetzt ist. Nur fuer die Ausreisser-Erkennung des Profilers.
    pub(crate) fn profiler_target_interval(&self) -> Option<Duration> {
        if self.gpu.as_ref().is_some_and(|gpu| gpu.present_mode_self_paces()) {
            return None;
        }
        Some(self.effective_frame_interval()).filter(|d| !d.is_zero())
    }

    // Die Profiler-Tabelle oben rechts, auf dunklem Grund.
    pub(crate) fn push_profiler_overlay(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if !self.profiler.active() {
            return;
        }
        let rows = self.profiler.overlay_rows(self.profiler_target_interval());
        let scale = UI_SCALE_SMALL;
        let line_h = gpu.text_height(scale) + 4.0;
        let (col_avg, col_max) = (300.0, 380.0);
        let width = rows
            .iter()
            .filter(|r| r.len() == 1)
            .map(|r| gpu.measure_text(&r[0], scale))
            .fold(col_max, f32::max)
            + 16.0;
        let (screen_w, _) = gpu.size();
        let x = screen_w - width - 8.0;
        let y0 = 8.0;
        rects.push(UiRect { rect: Rect { x, y: y0, w: width, h: rows.len() as f32 * line_h + 12.0 }, color: [0.0, 0.0, 0.0, 0.75], ..UiRect::FLAT });
        for (i, row) in rows.iter().enumerate() {
            let y = y0 + 6.0 + i as f32 * line_h;
            let color = if i == 0 { [1.0, 0.85, 0.3, 1.0] } else { COLOR_HUD };
            texts.push(UiText { text: row[0].clone(), x: x + 8.0, y, color, scale });
            for (cell, right) in row.iter().skip(1).zip([col_avg, col_max]) {
                let w = gpu.measure_text(cell, scale);
                texts.push(UiText { text: cell.clone(), x: (x + 8.0 + right - w).round(), y, color, scale });
            }
        }
    }

    pub(crate) fn push_tearing_bars(&self, rects: &mut Vec<UiRect>, screen_w: f32, screen_h: f32) {
        if !self.settings.show_tearing_test {
            return;
        }
        let elapsed = self.start_time.elapsed().as_secs_f32();
        let travel = screen_w + TEARING_BAR_WIDTH_PX;
        for (i, color) in TEARING_BAR_COLORS.iter().enumerate() {
            let phase = i as f32 / TEARING_BAR_COLORS.len() as f32;
            let progress = (elapsed / TEARING_BAR_SECONDS_PER_SWEEP + phase).fract().max(0.0);
            let x = progress * travel - TEARING_BAR_WIDTH_PX;
            rects.push(UiRect { rect: Rect { x, y: 0.0, w: TEARING_BAR_WIDTH_PX, h: screen_h }, color: *color, ..UiRect::FLAT });
        }
    }
}
