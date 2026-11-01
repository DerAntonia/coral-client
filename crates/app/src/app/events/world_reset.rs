// Welt zuruecksetzen und Player Loaded.

use super::*;

impl App {
    // Alles verwerfen, was zur alten Verbindung gehoert: die Welt eines Servers hat auf
    // dem naechsten nichts zu suchen.
    pub(crate) fn reset_world(&mut self) {
        self.reset_dialogs();
        self.server_links.clear();
        // Verbindungs-Warnungen gehoeren zur Verbindung.
        if let Some(id) = self.packet_gap_note.take() {
            self.notifications.dismiss(id, Instant::now());
        }
        self.lagback_note = None;
        self.rubberband_note = None;
        self.transactions = transactions::Transactions::default();
        // Den Standort fuers naechste Verbinden merken (s. lod::App::lod_preview_saved).
        self.lod_remember_pose();
        self.reset_world_state();
        // Der Sprachchat gehoert zur Verbindung.
        self.voice.reset();
        // Pop-Zaehler gehoeren zur Verbindung, nicht zum Spieler (wie in der Mod).
        self.pop_counts.clear();
        // Server-HUD (Scoreboard/Titel/Actionbar) gehoert zur alten Verbindung -- nicht auf
        // den naechsten Server mitschleppen. Bewusst NICHT in reset_world_state(): beim
        // blossen Dimensionswechsel bleibt das HUD stehen (Vanilla raeumt in
        // MinecraftClient.setWorld() nur Renderer/Partikel/Sounds, nicht die InGameHud).
        self.action_bar = None;
        self.title_text = None;
        self.subtitle_text = None;
        self.sb_objective_titles.clear();
        self.sb_sidebar_objective = None;
        self.sb_scores.clear();
        self.sb_teams.clear();
        self.sb_entry_team.clear();
        self.tab_entries.clear();
        self.tab_header.clear();
        self.tab_footer.clear();
        self.sb_list_objective = None;
        self.sb_objective_hearts.clear();
        self.sb_objective_formats.clear();
        self.sb_score_formats.clear();
        // Spielernamen und Statuswerte gehoeren ebenfalls zur alten Verbindung. Die
        // Statuswerte zurueck auf Vanillas Startwerte, damit beim naechsten Server nicht
        // kurz die Leisten des vorigen dastehen, bis dessen erstes Set Health kommt.
        self.player_names.clear();
        self.health = 20.0;
        self.max_health = 20.0;
        self.armor = 0;
        self.xp_level = 0;
        self.xp_progress = 0.0;
        self.air_ticks = MAX_AIR_TICKS;
    }

    // Der Teil davon, der zur WELT gehoert (nicht zur Verbindung): genau das, was Vanilla in
    // ClientPlayNetworkHandler.onPlayerRespawn beim Bau einer neuen ClientWorld hinter sich
    // laesst. Deshalb auch beim Dimensionswechsel aufgerufen, s. ClientEvent::Respawned.
    pub(crate) fn reset_world_state(&mut self) {
        // Die Sequenznummer gehoert zur Welt: Vanilla haelt sie im PendingUpdateManager der
        // ClientWorld, eine neue Welt faengt also wieder bei 0 an (erste gesendete Nummer 1).
        // GrimACs BadPacketsH setzt seinen Erwartungswert an derselben Stelle zurueck.
        self.block_sequence = 0;
        // Offene Vorhersagen ebenso -- ihr Ack kommt in der neuen Welt nie mehr (world.clear()
        // raeumt die Vorhersage-Schicht selbst).
        self.pending_blocks.clear();
        self.ghosts_awaiting_mesh.clear();
        self.lod_save_dirty();
        self.world.clear();
        self.pistons.clear(&mut self.world);
        self.entities.clear();
        // Neue Welt, neue Runde: die Teams koennen andere sein.
        self.team_color_memory.clear();
        self.own_ride = None;
        self.ride_yaw_spin = (0.0, 0.0);
        // BossBarHud.clear beim Weltwechsel.
        self.boss_bars.clear();
        // ParticleManager.setWorld -> clearParticles.
        self.particles.clear();
        self.chunks_received = 0;
        self.camera_placed = false;
        // Beim Trennen/Reconnect ist der Tod-Zustand der alten Verbindung hinfaellig.
        self.is_dead = false;
        self.sleeping_at = None;
        self.physics.sleeping = false;
        self.render_limit_meshed.clear();
        self.lod.mask_changed = true;
        self.column_meshed_at.clear();
        // Noch laufende Mesh-Jobs der alten Verbindung senden ihr Ergebnis spaeter trotzdem
        // noch ueber den Kanal (der Pool kennt reset_world() nicht) -- die werden beim
        // Eintreffen stillschweigend verworfen (kein passender Nachbar/Weltzustand mehr,
        // s. mesh_pending_columns), sollen den Zaehler hier aber nicht dauerhaft verfaelschen.
        self.mesh_jobs_pending.clear();
        self.mesh_jobs_stale_on_completion.clear();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.block_renderer.clear_meshes();
        }
    }

    // ClientPlayNetworkHandler.tick -> setPlayerLoaded: einmal, sobald der Chunk unter der
    // (vom Server geschickten) Position geladen ist.
    pub(crate) fn tick_player_loaded(&mut self) {
        if !self.player_loaded_pending || !self.player_loaded_have_pos {
            return;
        }
        // Botcheck-Pass: Vanillas Ladebildschirm genau nachgehen (s. vanilla_level_ready).
        if self.settings.module_botcheck_pass {
            if !self.vanilla_level_ready() {
                return;
            }
            self.send_player_loaded();
            return;
        }
        let (cx, cz) = ((self.physics.pos.x / 16.0).floor() as i32, (self.physics.pos.z / 16.0).floor() as i32);
        if self.world.column(cx, cz).is_none() {
            return;
        }
        // Vanilla meldet sich nicht im selben Tick fertig, in dem die Startposition ankommt:
        // notifyPlayerLoaded() haengt an levelLoadTracker.isLevelReady(), und der Tracker
        // bekommt seinen tickClientLoad() erst im Client-Tick. Dadurch liegt zwischen dem
        // Teleport und dem PlayerLoaded genau ein Tick, in dem LocalPlayer.tick() noch gar
        // nicht laeuft -- im Mitschnitt von anticheat-test.com geht dort NUR ein
        // ClientTickEnd raus, kein Positionspaket.
        //
        // Ohne diesen Vorlauf landeten bei uns das Teleport-PosRot und der erste
        // Positionsbericht im SELBEN Tick. Sonar zaehlt aber Positionen gegen Ticks
        // (GravityHandler.checkClientTick: "expected client tick end but got position") und
        // sieht dann zwei Positionen ohne ClientTickEnd dazwischen.
        if self.load_tracker_ticks == 0 {
            self.load_tracker_ticks = 1;
            return;
        }
        self.send_player_loaded();
    }

    pub(crate) fn send_player_loaded(&mut self) {
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::player_loaded_payload());
            self.player_loaded_pending = false;
            self.client_loaded = true;
        }
    }

    // Vanillas Ladebildschirm (ClientChunkLoadProgress), einmal pro Client-Tick gefragt.
    // Solange er laeuft, tickt der Spieler nicht -- es geht nur ClientTickEnd raus, keine
    // Bewegung. Das ist die Zeitachse eines echten Clients: erst die Welt zeigen, dann
    // loslaufen. Ein Client, der sofort nach der Startposition losfunkt, sieht aus wie ein Bot.
    //
    //   1. Start: warten auf das Game Event INITIAL_CHUNKS_COMING.
    //   2. LoadChunks: warten, bis der Chunk-Abschnitt unter dem Spieler GEBAUT ist und zu
    //      30 % eingeblendet (WorldRenderer.isRenderingReady). Mit Vanillas Vorgabe fuer das
    //      Einblenden (0.75 s) sind das 225 ms nach dem ersten Upload. Ein leerer Abschnitt hat
    //      nichts einzublenden und gilt sofort, sobald er gebaut ist.
    //      Sofort fertig ist man ausserhalb der Bauhoehe, tot oder als Zuschauer.
    //   3. Nach 30 s ab Beginn laesst Vanilla einen trotzdem hinein.
    //
    // Abweichung: bleibt INITIAL_CHUNKS_COMING ganz aus, haengt Vanilla fuer immer im
    // Ladebildschirm. Wir lassen nach denselben 30 s trotzdem los, statt nie zu spielen.
    pub(crate) fn vanilla_level_ready(&self) -> bool {
        if self.world_loading_since.elapsed() >= VANILLA_LOAD_TIMEOUT {
            return true;
        }
        if !self.initial_chunks_coming {
            return false;
        }
        let p = self.physics.pos;
        let (bx, by, bz) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let min_y = self.world.min_y();
        if by < min_y || by >= min_y + self.world_height || self.is_dead || self.game_mode == GAME_MODE_SPECTATOR {
            return true;
        }
        let spalte = (bx.div_euclid(16), bz.div_euclid(16));
        let Some(gebaut) = self.column_meshed_at.get(&spalte) else { return false };
        let sy = by.div_euclid(16) * 16;
        let (sx, sz) = (spalte.0 * 16, spalte.1 * 16);
        let leer = (sy..sy + 16).all(|y| {
            (sx..sx + 16).all(|x| (sz..sz + 16).all(|z| world::block_name(self.world.block_at(x, y, z)).is_none_or(|n| n.ends_with("air"))))
        });
        leer || gebaut.elapsed() >= VANILLA_CHUNK_FADE_READY
    }
}
