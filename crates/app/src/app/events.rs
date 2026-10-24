// Server-Ereignisse (drain_events), Log, Welt zuruecksetzen, Lagback und Benachrichtigungen.

use crate::*;

impl App {
    // Der Deckel: entweder die Refresh-Rate des Monitors oder ein fester Wert aus den
    // Einstellungen.
    pub(crate) fn effective_frame_interval(&self) -> Duration {
        match self.settings.fps_limit {
            FPS_LIMIT_MONITOR => self.monitor_frame_interval,
            // Kein Deckel: nie schlafen. next_frame_time bleibt dadurch immer schon
            // erreicht, die Schleife rendert so schnell sie kann (s. about_to_wait).
            FPS_LIMIT_UNLIMITED => Duration::ZERO,
            fps => Duration::from_secs_f64(1.0 / fps as f64),
        }
    }

    pub(crate) fn push_log(&mut self, line: impl Into<String>) {
        // Das Log steht im Launcher (s. launcher log_section): der leitet stdout/stderr des
        // Clients in client.log um und liest von dort mit. Hier bleiben nur die letzten
        // Zeilen fuer die Phasen-Anzeige beim Verbinden (s. build_connecting_overlay).
        let line = line.into();
        eprintln!("{line}");
        self.log_lines.push(line);
        if self.log_lines.len() > LOG_MAX_LINES {
            let weg = self.log_lines.len() - LOG_MAX_LINES;
            self.log_lines.drain(0..weg);
        }
    }

    // Alles, was zu EINER Verbindung gehoert, vergessen: Welt, Inventar, Spielerliste, Skins,
    // Schilder, Karten, Chat-Signatur. Beim Trennen und beim Transfer zu einem anderen Host.
    pub(crate) fn forget_connection_state(&mut self) {
        self.consumable.reset();
        self.reset_world();
        self.inventory.reset();
        self.recipe_book.forget_server();
        // Der Slot-Abgleich gehoert zur Verbindung (Vanilla baut den
        // InteractionManager pro Join neu), sonst bliebe der alte Stand stehen
        // und der erste Wechsel nach dem Neuverbinden ginge verloren.
        self.last_synced_slot = 0;
        self.pending_input_actions.clear();
        // Spielerliste und Skins gehoeren zur alten Verbindung: die UUIDs des
        // naechsten Servers sind andere (im Offline-Mode sogar fuer denselben
        // Namen). Alle drei zusammen, sonst zeigt skin_slot_order auf Eintraege,
        // die es in `skins` nicht mehr gibt, und die Platzvergabe greift daneben.
        self.own_uuid = None;
        self.own_entity_id_published.store(-1, std::sync::atomic::Ordering::Relaxed);
        self.skins.clear();
        self.skin_slot_order.clear();
        self.cape_slot_order.clear();
        self.tab_heads.clear();
        self.tab_head_order.clear();
        self.signs.clear();
        self.maps.clear();
        self.placed_heads.clear();
        self.sign_edit = None;
        self.book_view = None;
        self.book_edit = None;
        self.head_textures.clear();
        self.head_lookups.clear();
        self.requested_skins.clear();
        self.skin_waiters.clear();
        self.skin_sources.clear();
        self.skin_retries.clear();
        // Items in Haenden ebenso: die Item-Plaetze im Atlas werden beim naechsten
        // Server neu vergeben, und ein Server mit anderen Custom-Items koennte
        // dieselbe Item-Id anders aufloesen.
        self.held_items.clear();
        self.map_slots.clear();
        // Signaturkette gehoert zur Verbindung -- beim naechsten Join eine neue.
        self.chat_signer = None;
    }

    // Verbindungs-Events aus dem Netzwerk-Task einsammeln (nicht blockierend).
    pub(crate) fn drain_events(&mut self) {
        self.publish_ego_look();
        loop {
            // Was ein Tab hinten gesammelt hat, kommt zuerst (s. app/tabs.rs). Ein Versatz oder
            // eine Stockung darin ist zu der Zeit passiert, nicht jetzt: keine Meldung dafuer.
            let (event, nachgeholt) = match self.event_backlog.pop_front() {
                Some(event) => (event, true),
                None => match self.events_rx.try_recv() {
                    Ok(event) => (event, false),
                    Err(_) => break,
                },
            };
            self.replaying_backlog = nachgeholt;
            // Log-Zeilen schreibt der Netz-Task selbst, alles andere kam vom Server.
            if !matches!(event, net::ClientEvent::Log(_) | net::ClientEvent::PacketStall { .. }) {
                self.server_event_seen();
            }
            match event {
                net::ClientEvent::Log(msg) => self.push_log(msg),
                net::ClientEvent::PacketStall { outgoing, millis } => self.note_rubberband(outgoing, millis),
                net::ClientEvent::RawPacket(data) => {
                    let id = protocol::packets::peek_packet_id(&data).unwrap_or(-1);
                    self.addon_hook(coral_addon::Event::PacketIn { id, data });
                }
                net::ClientEvent::TransactionPing(id) => {
                    if let Some(zeile) = self.transactions.push(id, Instant::now()) {
                        self.push_log(zeile);
                    }
                }
                net::ClientEvent::Connected => {
                    let server = self.server_address.clone();
                    self.addon_hook(coral_addon::Event::Join { server });
                    self.conn_status = ConnStatus::Connected;
                    // Mit dem Spiel beginnt ein neuer Bildschirm: Fenster aus der
                    // Konfiguration sind vorbei.
                    self.reset_dialogs();
                    self.reset_own_entity_flags();
                    chat_log::session_start(&self.server_address);
                    self.player_loaded_pending = true;
                    self.player_loaded_have_pos = false;
                    self.client_loaded = false;
                    self.load_tracker_ticks = 0;
                    // ClientPlayNetworkHandler.startWorldLoading: neuer Ladebildschirm.
                    self.initial_chunks_coming = false;
                    self.world_loading_since = Instant::now();
                    // Vanilla baut bei jedem Weltwechsel einen NEUEN LocalPlayer
                    // (ClientPacketListener.handleLogin/handleRespawn -> createPlayer). Dessen
                    // xLast/yLast/zLast, yRotLast/xRotLast und positionReminder starten alle
                    // bei 0 -- der "zuletzt gesendet"-Stand der alten Welt ueberlebt also
                    // nicht. Beim TELEPORT wird dagegen nichts davon angefasst (s. dort).
                    //
                    // Sichtbar im Mitschnitt: nach dem Teleport auf yaw=0/pitch=0 sieht Vanilla
                    // keine Rotationsaenderung gegenueber der 0 des frischen Spielers und
                    // schickt ein MovePlayerPos (0x1D). Wir schleppten die alte Kameradrehung
                    // mit, sahen eine Aenderung und schickten ein MovePlayerPosRot (0x1E) --
                    // ein anderer Pakettyp, den LimboAPI auch ueber einen anderen Zweig
                    // verarbeitet (MovePacket statt MovePositionOnlyPacket).
                    self.last_sent_pos = DVec3::ZERO;
                    self.last_sent_yaw_deg = 0.0;
                    self.last_sent_pitch_deg = 0.0;
                    self.ticks_since_position_sent = 0;
                    // Vanilla loest beim Oeffnen eines Bildschirms alle Tasten (KeyMapping.
                    // releaseAll()) -- was vorher gedrueckt wurde, ueberlebt den Uebergang
                    // nicht. Noch wartende Klicks gehoeren zur alten Welt und duerfen nicht
                    // in den ersten Tick der neuen laufen.
                    self.pending_input_actions.clear();
                    self.slot_queue = SlotQueue::default();
                    self.push_log("== verbunden ==");
                    // Crystal-Optimizer: ein Server, der in dieser Sitzung schon abgeschaltet
                    // hat, bleibt abgeschaltet (ConnectEventListener der Mod).
                    self.crystal_opted_out = self.crystal_opt_out_servers.contains(&self.server_address);
                    self.voice.reset();
                    if self.settings.module_voice_chat {
                        self.announce_voice_chat();
                    }
                    if self.settings.module_crystal_optimizer {
                        self.announce_crystal_optimizer();
                    }
                    self.consumable.reset();
                    if self.settings.module_consumable_optimizer {
                        self.announce_consumable_optimizer();
                    }
                    // Signierter Chat braucht den Profilschluessel des Kontos. Erst jetzt
                    // anfordern (nicht beim Login): der Schluessel gilt nur ein paar Tage
                    // und die Sitzung gehoert zu GENAU dieser Verbindung.
                    self.request_profile_key();
                    // Automatisch in die Welt, sobald die Verbindung steht -- aber nur, wenn
                    // der Nutzer noch auf dem Startbildschirm wartet (nicht, wenn er
                    // inzwischen in den Einstellungen o.ae. blaettert; das waere ein
                    // ueberraschender Sprung).
                    if self.mode == AppMode::Menu && self.screen == MenuScreen::Main {
                        self.mode = AppMode::World;
                        self.restore_held_controls();
                    }
                }
                net::ClientEvent::Disconnected(reason) => {
                    self.addon_hook(coral_addon::Event::Leave { reason: reason.clone() });
                    self.keep_last_view();
                    self.conn_status = ConnStatus::Disconnected;
                    // Der Task hat sich selbst beendet; der Handle ist nur noch Altlast.
                    self.conn_task = None;
                    // Server-Resourcepacks gehoeren zur Verbindung (s. server_packs).
                    self.server_packs.clear();
                    self.forget_connection_state();
                    // Dialoge und Links gehoeren ebenso zur Verbindung.
                    self.reset_dialogs();
                    self.server_links.clear();
                    // Aus World/Death/Pause zurueck auf die Server-Liste.
                    self.mode = AppMode::Menu;
                    self.screen = MenuScreen::Connect;
                    self.disconnect_popup = Some(disconnect_popup_text(&reason).to_string());
                    self.push_log(format!("getrennt: {reason}"));
                }
                net::ClientEvent::ResourcePackPush(push) => self.server_pack_offered(push),
                net::ClientEvent::LightUpdate { x, z, light } => {
                    // Nur der Shader-Pfad zeichnet Licht -- nur dann lohnt das Neu-Meshen.
                    if self.world.apply_sky_light(x, z, &light) && self.settings.render_path == 1 {
                        for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                            if self.world.column(x + dx, z + dz).is_some() {
                                self.world.mark_dirty(x + dx, z + dz);
                            }
                        }
                    }
                }
                net::ClientEvent::ResourcePackPop(id) => self.server_packs.remove(id),
                net::ClientEvent::Reconfiguring => {
                    // Der Server holt uns zurueck in die Konfiguration (Serverwechsel hinter
                    // einem Proxy, "Relog"). Danach kommt eine neue Welt -- die alte muss weg,
                    // sonst bleiben ihre Chunks und Entities stehen. Die Verbindung selbst
                    // bleibt bestehen, deshalb NICHT zurueck ins Menue: der Client zeigt
                    // wieder das Verbindungs-Overlay (haengt an chunks_received == 0) und
                    // laeuft normal weiter, sobald die neuen Chunks da sind.
                    self.keep_last_view();
                    self.reset_world();
                    self.world_reset_debug = Some((Instant::now(), "Rekonfiguration"));
                    self.inventory.reset();
                }
                net::ClientEvent::Route(ziel) => self.conn_route = Some(ziel),
                net::ClientEvent::Transferring { host, port } => {
                    self.conn_route = None;
                    // Der Server schickt uns weiter (Transfer), der Netz-Task verbindet schon
                    // neu. Alles von der alten Verbindung muss weg wie beim Trennen -- nur
                    // NICHT zurueck ins Menue (Verbindungs-Overlay wie bei der
                    // Rekonfiguration) und die Server-Resourcepacks bleiben (s. ClientEvent::
                    // Transferring).
                    self.keep_last_view();
                    self.conn_status = ConnStatus::Connecting;
                    self.forget_connection_state();
                    self.push_log(format!("Transfer nach {host}:{port}"));
                }
                // Ton. Hier im EVENT-Pfad, nicht in der Tick-Schleife: drain_events laeuft pro
                // Frame, der Tick nur alle 50 ms -- ein tickgekoppelter Ton waere als
                // Reaktionssignal um bis zu eine Zwanzigstelsekunde zu spaet.
                net::ClientEvent::Sound(sound) => self.play_server_sound(sound),
                net::ClientEvent::Chunk(chunk) => {
                    self.chunks_received += 1;
                    let (cx, cz) = (chunk.x, chunk.z);
                    self.column_history.entry((cx, cz)).or_default().0 = Some(Instant::now());
                    self.signs.retain(|p, _| (p.0.div_euclid(16), p.2.div_euclid(16)) != (cx, cz));
                    self.signs.extend(chunk.signs);
                    self.placed_heads.retain(|p, _| (p.0.div_euclid(16), p.2.div_euclid(16)) != (cx, cz));
                    for (pos, profil) in &chunk.heads {
                        self.note_placed_head(*pos, profil);
                    }
                    // Nur so viele Sections wie die Dimension hoch ist, wie Vanilla; dahinter
                    // haengen manche Server Muell an (s. protocol::chunk, tail_unreadable).
                    // Ohne bekannte Dimension nur bei Muell kuerzen, dann auf die Overworld.
                    let hoehe = self
                        .dimension_types
                        .iter()
                        .find(|d| d.name == self.dimension_type_name)
                        .map(|d| (d.height.max(0) / 16) as usize)
                        .or(chunk.tail_unreadable.then_some(384 / 16));
                    let sections = chunk
                        .sections
                        .into_iter()
                        .take(hoehe.unwrap_or(usize::MAX))
                        .map(|s| world::ChunkSection::from_states_and_biomes(s.block_states, s.biomes))
                        .collect();
                    // Markiert die Spalte (und ihre Nachbarn) als zu meshen; gebaut wird
                    // sie budgetiert im Frame, s. mesh_pending_columns.
                    self.world.insert_column(chunk.x, chunk.z, sections);
                    if let Some(light) = &chunk.light {
                        self.world.apply_sky_light(chunk.x, chunk.z, light);
                    }
                    self.lod_ingest_column(chunk.x, chunk.z);
                }
                net::ClientEvent::PlayerPosition(sync) => {
                    // Der Server setzt uns: ein laufender Klick-Teleport ist damit vorbei.
                    self.spectator_flight = None;
                    let (x, y, z) = sync.pos;
                    let (yaw_deg, pitch_deg, flags) = (sync.yaw, sync.pitch, sync.flags);
                    // Die erste Position nach dem Betreten ist kein Lag-back (s. note_lagback).
                    let hatte_position = self.player_loaded_have_pos;
                    self.player_loaded_have_pos = true;
                    // Sitzend laesst Vanilla Position und Drehung liegen
                    // (ClientPacketListener.handleMovePlayer, nur ohne isPassenger): die Stelle
                    // gibt der Sitz vor, die Drehung bleibt die eigene. Bestaetigt ist der
                    // Teleport schon (s. connection.rs). Beispiel UHC-Drachenflug: das Plugin
                    // teleportiert den Sitz jeden Tick, Vanilla teleportiert den Passagier mit,
                    // und dessen Drehung ist der Stand, den der Server zuletzt von uns hatte --
                    // die Kamera sprang dauernd dorthin zurueck.
                    if self.own_ride.is_some() {
                        eprintln!("[teleport] sitzend, uebergangen: pos=({x:.2},{y:.2},{z:.2}) flags={flags:#x}");
                        continue;
                    }
                    // JEDE Synchronize Player Position hart uebernehmen, nicht nur die
                    // erste: das Packet kommt genau dann, wenn der Server die Position
                    // des Clients ueberstimmt (/tp, "moved too quickly"-Korrektur,
                    // Respawn). Vanilla snappt hier auch -- wer es ignoriert, laeuft
                    // permanent desynchron weiter (gesehen: endloser Void-Fall, waehrend
                    // der Server vergeblich zurueckteleportiert). X/Y/Z sind die Fuesse;
                    // PlayerPhysics::new nullt auch die Fallgeschwindigkeit, wie Vanilla
                    // beim Teleport.
                    //
                    // ECHTER BUG gewesen (Grim BadPacketsN, "ignored or failed to accept a
                    // required server teleport"): X/Y/Z koennen laut Flags
                    // RELATIV zur aktuellen Position gemeint sein (s. packets::
                    // POSITION_FLAG_REL_*), nicht immer absolut -- wurde bisher ignoriert,
                    // also musste vor dem Ueberschreiben von physics.pos aufgeloest werden.
                    let old_pos = self.physics.pos;
                    let resolved_x = if flags & protocol::packets::POSITION_FLAG_REL_X != 0 { old_pos.x + x } else { x };
                    let resolved_y = if flags & protocol::packets::POSITION_FLAG_REL_Y != 0 { old_pos.y + y } else { y };
                    let resolved_z = if flags & protocol::packets::POSITION_FLAG_REL_Z != 0 { old_pos.z + z } else { z };
                    // Perle und Chorusfrucht teleportieren mit allen Delta-Bits (der Schwung
                    // bleibt, s. teleport_velocity) -- das ist gewollt, kein Zuruecksetzen.
                    const DELTA_ALLE: i32 = protocol::packets::POSITION_FLAG_REL_DELTA_X
                        | protocol::packets::POSITION_FLAG_REL_DELTA_Y
                        | protocol::packets::POSITION_FLAG_REL_DELTA_Z;
                    const POS_REL: i32 = protocol::packets::POSITION_FLAG_REL_X
                        | protocol::packets::POSITION_FLAG_REL_Y
                        | protocol::packets::POSITION_FLAG_REL_Z;
                    // Perle/Chorus: Schwung bleibt, Ziel absolut. Alles relativ (applepvp,
                    // flags=0x1ff) ist dagegen ein Versatz, kein Perlenwurf.
                    let perle = flags & DELTA_ALLE == DELTA_ALLE && flags & POS_REL == 0;
                    let strecke = old_pos.distance(DVec3::new(resolved_x, resolved_y, resolved_z));
                    // Immer ins Client-Log (selten, nur bei Server-Teleports): woran eine
                    // fehlende oder falsche Lag-back-Karte lag, steht dann schon da.
                    let urteil = if !hatte_position {
                        "erste Position"
                    } else if strecke < LAGBACK_MIN_BLOCKS {
                        "an Ort und Stelle"
                    } else if perle {
                        "Perle/Chorus"
                    } else if strecke > LAGBACK_MAX_BLOCKS {
                        "zu weit (echter Teleport)"
                    } else {
                        "Lag-back"
                    };
                    eprintln!(
                        "[teleport] {strecke:.3} Bloecke, flags={flags:#x}, vel=({:.3},{:.3},{:.3}), von ({:.2},{:.2},{:.2}) nach ({resolved_x:.2},{resolved_y:.2},{resolved_z:.2}) -> {urteil}",
                        sync.vel.0, sync.vel.1, sync.vel.2, old_pos.x, old_pos.y, old_pos.z
                    );
                    if hatte_position && !perle {
                        self.note_lagback(strecke);
                    }
                    // ZWEITER ECHTER BUG gewesen (wer Maus und Tastatur gleichzeitig bewegte,
                    // wurde sofort an den Spawnpunkt zurueckteleportiert): Yaw/Pitch aus diesem Paket wurden bisher
                    // komplett verworfen, die Kamera behielt einfach ihren alten Wert. Grims
                    // Teleport-Warteschlange verlangt aber, dass das naechste Rotationspaket
                    // zur vom Server erwarteten Blickrichtung passt -- ohne Maus-Input senden
                    // wir nie eins (faellt also nicht auf), sobald die Maus dazukommt, schon.
                    // Konvention wie beim Senden (s. send_position_update): yaw direkt in
                    // Grad (Kamera speichert selbst in Grad, s. camera.rs), pitch mit
                    // gedrehtem Vorzeichen.
                    let old_yaw_deg = self.camera.yaw;
                    let old_pitch_deg = self.look_pitch_deg();
                    let resolved_yaw_deg =
                        if flags & protocol::packets::POSITION_FLAG_REL_YAW != 0 { old_yaw_deg + yaw_deg } else { yaw_deg };
                    let resolved_pitch_deg =
                        if flags & protocol::packets::POSITION_FLAG_REL_PITCH != 0 { old_pitch_deg + pitch_deg } else { pitch_deg };
                    if std::env::var_os("CORAL_SWIM_DEBUG").is_some() {
                        eprintln!(
                            "[possync] server korrigiert Position -> ({resolved_x}, {resolved_y}, {resolved_z}) yaw={resolved_yaw_deg} pitch={resolved_pitch_deg} flags={flags:#x}, vorher war physics.pos={old_pos:?}"
                        );
                    }
                    self.camera_placed = true;
                    // Der Tastenzustand gehoert NICHT zur Position: ein Server-Teleport
                    // verschiebt uns, er laesst aber niemanden die Sneak-Taste loslassen.
                    // PlayerPhysics::new() setzt prev_sneak auf false -- damit sah der
                    // naechste Tick nach JEDEM Setback wie ein frischer Tastendruck aus und
                    // liess die Schleich-Verlangsamung einen Tick lang weg. Im
                    // CORAL_SNEAK_DEBUG-Mitschnitt beginnt jeder Fehler-Schub genau
                    // damit ("prev=false ... faktor=0.9800"), und zwar jedes Mal exakt auf
                    // der Setback-Position -- daraus wurde aus einem einmaligen Flag eine
                    // Endlosschleife. Vanilla kennt so einen Reset nicht.
                    // Genauso die Koerperhaltung: Grim behaelt seine Pose ueber den Teleport
                    // hinweg (handleTeleport baut nur die Box neu auf), ein Reset auf Stehen
                    // waere also sofort wieder ein Hitbox-Versatz -- s. carry_over_state.
                    // Die GESCHWINDIGKEIT aus dem Teleport (Vanillas PositionMoveRotation::
                    // calculateAbsolute, angewandt in setValuesFromPositionPacket). Sie MUSS
                    // vor dem Neuanlegen der Physik berechnet werden -- PlayerPhysics::new
                    // nullt sie, und genau dieses Nullen war der Fehler: bei der Enderperle
                    // sind alle DELTA-Bits gesetzt und der Paketwert ist 0, der eigene Schwung
                    // bleibt also erhalten. Wir standen nach jeder Perle schlagartig still.
                    let neue_vel = teleport_velocity(
                        self.physics.vel,
                        DVec3::new(sync.vel.0, sync.vel.1, sync.vel.2),
                        flags,
                        (old_yaw_deg, old_pitch_deg),
                        (resolved_yaw_deg, resolved_pitch_deg.clamp(-90.0, 90.0)),
                    );
                    let uebernommen = self.physics.carry_over_state();
                    self.physics = physics::PlayerPhysics::new(DVec3::new(resolved_x, resolved_y, resolved_z));
                    self.physics.body = self.attr_body;
                    self.physics.restore_carry_over_state(uebernommen);
                    self.physics.vel = neue_vel;
                    // KEIN refresh_on_ground: Vanillas setValuesFromPositionPacket laesst
                    // onGround und den Standblock in Ruhe, beide kommen ueber
                    // carry_over_state herueber (s. dortigen Kommentar -- die Neumessung war
                    // die Ursache eines Grim-Simulation-Flags nach jeder Enderperle).
                    // Die gezeichnete Zwischenposition wie Vanillas setOldPosAndRot: dieselbe
                    // Aenderung auch auf die vorige Tick-Position, relative Achsen also nur
                    // verschoben, absolute gesetzt. Den Tick-Takt (physics_accum) fasst Vanilla
                    // gar nicht an.
                    //
                    // ECHTER BUG gewesen (applepvp): der Server schickt "Teleports" mit allen
                    // Relativ-Bits und Versatz 0 -- man bleibt, wo man ist. Wir setzten
                    // trotzdem die Zwischenposition auf den neuesten Tick (Sprung nach vorn um
                    // bis zu einen Tick Bewegung) und den Takt auf 0 (bis zu 50 ms Stillstand):
                    // sah jedes Mal aus wie ein Teleport.
                    let prev = self.physics_render_prev;
                    let achse = |bit: i32, alt: f64, rel: f64, abs: f64| if flags & bit != 0 { alt + rel } else { abs };
                    self.physics_render_prev = DVec3::new(
                        achse(protocol::packets::POSITION_FLAG_REL_X, prev.x, x, resolved_x),
                        achse(protocol::packets::POSITION_FLAG_REL_Y, prev.y, y, resolved_y),
                        achse(protocol::packets::POSITION_FLAG_REL_Z, prev.z, z, resolved_z),
                    );
                    self.apply_server_rotation(resolved_yaw_deg, resolved_pitch_deg);
                    // Netz-Task sofort mit der neuen Blickrichtung versorgen, damit ein direkt
                    // folgender Teleport (Setback-Kette) die richtige Rotation zurueckmeldet.
                    self.shared_rotation.set(resolved_yaw_deg, resolved_pitch_deg);
                    self.camera.position = self.physics.eye_pos();
                    // Der "zuletzt gesendet"-Stand wird hier BEWUSST NICHT zurueckgesetzt.
                    // ClientPacketListener.setValuesFromPositionPacket fasst in Vanilla nur
                    // Position, Geschwindigkeit, Rotation und oldPosAndRot an -- xLast/yLast/
                    // zLast, yRotLast/xRotLast, positionReminder, lastOnGround und
                    // lastHorizontalCollision bleiben stehen. Der naechste Spieler-Tick
                    // vergleicht deshalb gegen die Position VOR dem Teleport, sieht damit
                    // praktisch immer eine grosse Differenz und schickt ein volles
                    // Positionspaket.
                    //
                    // Sichtbar im Mitschnitt von anticheat-test.com: Vanilla sendet erst das
                    // Bestaetigungs-PosRot (aus handleMovePlayer) und im naechsten Tick NOCH
                    // EINMAL dieselbe Position als MovePlayerPos. Mit dem frueheren Reset fiel
                    // dieses zweite Paket bei uns aus -- ein Bewegungspaket weniger als Vanilla
                    // ueber den gesamten Botcheck.
                    // Die Teleport-Bestaetigungs-Bewegung (das Paket, das Grim exakt an der
                    // Zielposition sehen MUSS) wird bewusst NICHT hier im App-Thread gesendet,
                    // sondern direkt im Netz-Thread (s. connection.rs, handle_play_packet):
                    // nur dort behaelt es seine Reihenfolge relativ zu den Transaktions-Pongs.
                    // Ein hier gesendetes Paket kaeme einen Frame spaeter -- nach bereits
                    // rausgegangenen Pongs -- und liefe damit an Grims Transaktionsfenster vorbei.
                }
                net::ClientEvent::ServerRotation { yaw_deg, pitch_deg } => {
                    // Der Server hat unsere Blickrichtung gedreht (Player Rotation 0x47).
                    // Legitim: Zwischensequenzen, "schau hierhin"-Effekte, Minispiel-Kameras.
                    // Vanilla folgt dem kommentarlos, wir auch -- die Bestaetigung ist im
                    // Netz-Task schon raus (s. connection.rs), hier bleibt nur die Kamera.
                    //
                    // Vorzeichen wie ueberall sonst: das Protokoll zaehlt Pitch nach unten
                    // positiv, unsere Kamera nach oben (s. look_pitch_deg / die Teleport-
                    // Behandlung weiter oben) -- deshalb das Minus.
                    self.apply_server_rotation(yaw_deg, pitch_deg);
                    // Kein Ruecksetzen von ticks_since_position_sent und kein Reset der
                    // "zuletzt gesendet"-Position: dieses Paket bewegt uns NICHT, es dreht
                    // uns nur. Die Positionsbuchhaltung bleibt damit gueltig.
                }
                net::ClientEvent::ChunkUndecodable(x, z) => {
                    self.column_history.entry((x, z)).or_default().2 = Some(Instant::now());
                    self.push_log(format!("Chunk {x} {z} nicht dekodierbar"));
                }
                net::ClientEvent::ForgetChunk(x, z) => {
                    self.column_history.entry((x, z)).or_default().1 = Some(Instant::now());
                    self.column_light.remove(&(x, z));
                    self.signs.retain(|p, _| (p.0.div_euclid(16), p.2.div_euclid(16)) != (x, z));
                    self.placed_heads.retain(|p, _| (p.0.div_euclid(16), p.2.div_euclid(16)) != (x, z));
                    // Erst aus dem Weltzustand, dann das zugehoerige Mesh aus dem VRAM --
                    // ohne das Entladen wuechsen beide beim Umherlaufen unbegrenzt (der
                    // eigentliche Grund, warum wir ueberhaupt hoehere Sichtweite fahren
                    // koennen, s. protocol::REQUESTED_VIEW_DISTANCE).
                    self.lod_before_forget(x, z);
                    if self.world.remove_column(x, z) {
                        self.render_limit_meshed.remove(&(x, z));
                        self.lod.mask_changed = true;
                        self.column_meshed_at.remove(&(x, z));
                        if let Some(gpu) = self.gpu.as_mut() {
                            gpu.block_renderer.remove_column_mesh((x, z));
                        }
                    }
                }
                net::ClientEvent::BlockUpdate(blocks) => {
                    // s. world::World::set_block: markiert die betroffene(n) Spalte(n) dirty,
                    // mesh_pending_columns() (laeuft jeden Frame) baut sie dann neu.
                    for (x, y, z, state) in blocks {
                        // Steht auf dieser Stelle noch eine eigene Vorhersage aus, wird das
                        // Update AUFGEHOBEN statt angewendet (s. defer_block_update).
                        if self.defer_block_update(x, y, z, state) {
                            continue;
                        }
                        self.world.set_block(x, y, z, state);
                        self.pistons.block_changed((x, y, z), state);
                        if self.settings.module_chunk_saver {
                            self.lod.dirty_columns.insert((x.div_euclid(16), z.div_euclid(16)));
                        }
                    }
                }
                net::ClientEvent::BlockChangedAck(sequence) => self.resolve_pending_blocks(sequence),
                // Kolbenhub: der Client rechnet ihn nach wie Vanilla (s. pistons.rs). Typ 1 ist
                // auch das Oeffnen einer Truhe -- deshalb zuerst nachsehen, ob dort ein Kolben steht.
                net::ClientEvent::BlockEvent(pos, typ, daten) if self.pistons.block_event(&mut self.world, &self.block_shapes, pos, typ, daten) => {}
                // Shulker-/ChestBlockEntity.onSyncedBlockEvent: Typ 1 = Zahl der Betrachter;
                // > 0 oeffnet, 0 schliesst.
                net::ClientEvent::BlockEvent(pos, 1, betrachter) => {
                    if world::block_entity::has_lid(self.world.block_at(pos.0, pos.1, pos.2)) {
                        self.block_lids.entry(pos).or_default().opening = betrachter > 0;
                    }
                }
                net::ClientEvent::BlockEvent(..) => {}
                // Rueckstoss. Set Entity Motion SETZT die Geschwindigkeit (Vanillas
                // Entity.setVelocityClient), die Explosion ADDIERT sie
                // (addVelocityInternal) -- der Unterschied ist nicht kosmetisch: wer beim
                // Kristall-Rueckstoss setzt statt addiert, verliert die eigene Bewegung.
                //
                // Nur die EIGENE Entity: fremde Spieler werden ohnehin aus ihren
                // Positionspaketen interpoliert, eine zusaetzliche Geschwindigkeit wuerde
                // dort nur doppelt zaehlen.
                net::ClientEvent::EntityVelocity(entity_id, (vx, vy, vz)) => {
                    if Some(entity_id) == self.own_entity_id {
                        self.physics.vel = DVec3::new(vx, vy, vz);
                    } else {
                        // Fuer Projektile ist das die Korrektur ihrer Flugbahn -- ohne sie
                        // fliegen sie mit der Anfangsgeschwindigkeit weiter, auch wenn der
                        // Server sie laengst umgelenkt hat (s. world::entity::tick_projectile).
                        self.entities.set_velocity(entity_id, (vx, vy, vz));
                    }
                }
                net::ClientEvent::ExplosionKnockback((vx, vy, vz)) => {
                    self.physics.vel += DVec3::new(vx, vy, vz);
                }
                net::ClientEvent::MobEffect { entity_id, effect, amplifier, duration, flags } => {
                    if Some(entity_id) == self.own_entity_id {
                        // Ein zweites Paket zum selben Effekt ERSETZT den bisherigen Stand --
                        // so verlaengert ein Trank die Restdauer und ein staerkerer ueberdeckt
                        // den schwaecheren. Was gilt, entscheidet der Server; der Client legt
                        // nur ab, was ihm gesagt wird.
                        // Die Dauer regelt allein der Server (s. own_effects); gebraucht wird sie
                        // nur fuer die Anzeige.
                        self.own_effects.insert(effect, amplifier);
                        self.shown_effects.insert(effect, ShownEffect { amplifier, duration, flags, since: Instant::now() });
                    }
                }
                net::ClientEvent::MobEffectRemoved { entity_id, effect } => {
                    if Some(entity_id) == self.own_entity_id {
                        self.own_effects.remove(&effect);
                        self.shown_effects.remove(&effect);
                    }
                }
                net::ClientEvent::OwnEntityId(id) => {
                    // Login baut in Vanilla eine neue Welt, und mit ihr eine Grenze ohne
                    // Einschraenkung; die des Servers folgt mit Initialize Border.
                    self.world.border = Default::default();
                    // Das Login-Paket heisst: FRISCHER Spieler. Das passiert nicht nur beim
                    // ersten Beitritt, sondern auch bei jedem Serverwechsel hinter einem Proxy
                    // (Velocity/Bungee schickt dem Client dabei ein neues Login, KEIN Respawn).
                    //
                    // ECHTER BUG gewesen (Speed blieb nach dem Serverwechsel aktiv): die Effekte wurden nur beim Respawn geleert. Wer in einem
                    // Spielmodus Schnelligkeit bekam und dann in die Lobby wechselte, behielt
                    // sie fuer den Rest der Sitzung -- der neue Server hatte nie einen Grund,
                    // ein Remove-Paket zu schicken, denn aus SEINER Sicht hatte man den Effekt
                    // nie. Vanilla trifft das nicht: dort entsteht beim Login ein neuer
                    // ClientPlayerEntity, und mit ihm eine leere Effektliste.
                    self.own_effects.clear();
                    self.shown_effects.clear();
                    self.absorption = 0.0;
                    // Dasselbe fuer die Abklingzeiten: sie leben im ItemCooldownManager des
                    // Spielerobjekts und fangen mit ihm leer an.
                    self.item_cooldowns = item_cooldowns::ItemCooldowns::default();
                    self.breaking_block = false;
                    self.breaking_pos = None;
                    self.breaking_progress = 0.0;
                    self.block_breaking_cooldown = 0;
                    self.attr_block_break_speed = 1.0;
                    self.attr_mining_efficiency = 0.0;
                    self.attr_submerged_mining_speed = 0.2;
                    // Genauso die Bewegungs-Attribute: sie haengen an den getragenen Teilen
                    // und am Server, ein neuer Server faengt mit den Vanilla-Grundwerten an.
                    self.attr_movement_speed = physics::BASE_SPEED;
                    self.attr_sneaking_speed = physics::DEFAULT_SNEAKING_SPEED;
                    self.attr_water_movement_efficiency = 0.0;
                    self.attr_body = physics::BodyAttributes::default();
                    self.physics.body = self.attr_body;
                    self.attr_attack_speed = None;
                    self.attr_block_range = BLOCK_INTERACTION_REACH;
                    self.attr_entity_range = ENTITY_INTERACTION_REACH;
                    // Neuer Spieler, neue Faehigkeiten; GameMode und Abilities des Servers folgen.
                    self.physics.abilities = physics::Abilities::default();
                    self.camera_entity = None;
                    self.op_level = 0;
                    self.own_entity_id = Some(id);
                    // Auch fuer den Sofort-Ton-Haken sichtbar machen -- der prueft damit, ob
                    // ein Totem-Pop uns selbst betrifft, ohne den Hauptthread zu fragen.
                    self.own_entity_id_published.store(id, std::sync::atomic::Ordering::Relaxed);
                    // Login setzt den Spieler genauso neu auf wie ein Respawn -- auch beim
                    // Serverwechsel hinter einem Proxy, wo kein neues Connected kommt.
                    self.forget_sent_input_state();
                    self.reset_own_entity_flags();
                    // ... auch beim "geladen"-Stand: der neue Server ignoriert Bewegung und
                    // Klicks (ServerPlayNetworkHandler.canInteractWithGame), bis PlayerLoaded
                    // kommt oder 60 Ticks um sind. Gesetzt wurde das nur beim ersten
                    // Verbinden und beim Respawn -- nach einem Serverwechsel hinter dem Proxy
                    // kam es nie, und die ersten drei Sekunden liefen ins Leere (der
                    // Serverwechsel verschluckte W).
                    self.player_loaded_pending = true;
                    self.player_loaded_have_pos = false;
                    self.client_loaded = false;
                    self.load_tracker_ticks = 0;
                    // ClientPlayNetworkHandler.startWorldLoading: neuer Ladebildschirm.
                    self.initial_chunks_coming = false;
                    self.world_loading_since = Instant::now();
                    // Vanilla baut bei jedem Weltwechsel einen NEUEN LocalPlayer
                    // (ClientPacketListener.handleLogin/handleRespawn -> createPlayer). Dessen
                    // xLast/yLast/zLast, yRotLast/xRotLast und positionReminder starten alle
                    // bei 0 -- der "zuletzt gesendet"-Stand der alten Welt ueberlebt also
                    // nicht. Beim TELEPORT wird dagegen nichts davon angefasst (s. dort).
                    //
                    // Sichtbar im Mitschnitt: nach dem Teleport auf yaw=0/pitch=0 sieht Vanilla
                    // keine Rotationsaenderung gegenueber der 0 des frischen Spielers und
                    // schickt ein MovePlayerPos (0x1D). Wir schleppten die alte Kameradrehung
                    // mit, sahen eine Aenderung und schickten ein MovePlayerPosRot (0x1E) --
                    // ein anderer Pakettyp, den LimboAPI auch ueber einen anderen Zweig
                    // verarbeitet (MovePacket statt MovePositionOnlyPacket).
                    self.last_sent_pos = DVec3::ZERO;
                    self.last_sent_yaw_deg = 0.0;
                    self.last_sent_pitch_deg = 0.0;
                    self.ticks_since_position_sent = 0;
                    // Vanilla loest beim Oeffnen eines Bildschirms alle Tasten (KeyMapping.
                    // releaseAll()) -- was vorher gedrueckt wurde, ueberlebt den Uebergang
                    // nicht. Noch wartende Klicks gehoeren zur alten Welt und duerfen nicht
                    // in den ersten Tick der neuen laufen.
                    self.pending_input_actions.clear();
                    self.slot_queue = SlotQueue::default();
                }
                net::ClientEvent::WorldIdentity { dimension, seed } => self.lod_world_identity(dimension, seed),
                net::ClientEvent::Respawned { world_change } => {
                    // Ein neuer Spieler sitzt nirgends -- und sieht durch die eigenen Augen.
                    self.dismount_own();
                    self.camera_entity = None;
                    self.forget_sent_input_state();
                    self.reset_own_entity_flags();
                    // Vanilla setzt bei JEDEM Respawn setLoaded(false) und meldet sich neu.
                    self.player_loaded_pending = true;
                    self.player_loaded_have_pos = false;
                    self.client_loaded = false;
                    self.load_tracker_ticks = 0;
                    // ClientPlayNetworkHandler.startWorldLoading: neuer Ladebildschirm.
                    self.initial_chunks_coming = false;
                    self.world_loading_since = Instant::now();
                    // Vanilla baut bei jedem Weltwechsel einen NEUEN LocalPlayer
                    // (ClientPacketListener.handleLogin/handleRespawn -> createPlayer). Dessen
                    // xLast/yLast/zLast, yRotLast/xRotLast und positionReminder starten alle
                    // bei 0 -- der "zuletzt gesendet"-Stand der alten Welt ueberlebt also
                    // nicht. Beim TELEPORT wird dagegen nichts davon angefasst (s. dort).
                    //
                    // Sichtbar im Mitschnitt: nach dem Teleport auf yaw=0/pitch=0 sieht Vanilla
                    // keine Rotationsaenderung gegenueber der 0 des frischen Spielers und
                    // schickt ein MovePlayerPos (0x1D). Wir schleppten die alte Kameradrehung
                    // mit, sahen eine Aenderung und schickten ein MovePlayerPosRot (0x1E) --
                    // ein anderer Pakettyp, den LimboAPI auch ueber einen anderen Zweig
                    // verarbeitet (MovePacket statt MovePositionOnlyPacket).
                    self.last_sent_pos = DVec3::ZERO;
                    self.last_sent_yaw_deg = 0.0;
                    self.last_sent_pitch_deg = 0.0;
                    self.ticks_since_position_sent = 0;
                    // Vanilla loest beim Oeffnen eines Bildschirms alle Tasten (KeyMapping.
                    // releaseAll()) -- was vorher gedrueckt wurde, ueberlebt den Uebergang
                    // nicht. Noch wartende Klicks gehoeren zur alten Welt und duerfen nicht
                    // in den ersten Tick der neuen laufen.
                    self.pending_input_actions.clear();
                    self.slot_queue = SlotQueue::default();
                    // Vanilla baut den Spieler beim Respawn neu auf, die Effekte sind damit
                    // weg -- der Server schickt danach, was weiterlaeuft. Wer sie behielte,
                    // liefe nach dem Tod mit dem Tempo seines letzten Lebens herum.
                    self.own_effects.clear();
                    self.shown_effects.clear();
                    self.absorption = 0.0;
                    // Neuer Spieler, neuer ItemCooldownManager.
                    self.item_cooldowns = item_cooldowns::ItemCooldowns::default();
                    if world_change {
                        // Portal/Weltwechsel: Vanilla baut eine neue ClientWorld -- Chunks,
                        // Entities und vor allem die Block-Sequenznummer fangen von vorn an.
                        // Ohne den Reset zaehlt der Client nach dem Portal weiter, waehrend
                        // GrimACs BadPacketsH bei 0 neu anfaengt: garantierter Flag beim
                        // ersten Blockklick drueben.
                        self.reset_world_state();
                        self.world_reset_debug = Some((Instant::now(), "Weltwechsel-Respawn"));
                    }
                }
                net::ClientEvent::OwnUuid(uuid) => {
                    self.own_uuid = Some(uuid);
                }
                net::ClientEvent::EntityHeadYaw { id, head_yaw_deg } => {
                    if let Some(entity) = self.entities.get_mut(id) {
                        entity.set_head_yaw(head_yaw_deg);
                    }
                }
                net::ClientEvent::EntitySleeping { id, pos } => {
                    // Fremde Spieler liegen ueber ihre Pose schon richtig im Bett (s.
                    // EntityPose) -- hier zaehlt nur der eigene Spieler.
                    if Some(id) == self.own_entity_id {
                        let vorher = self.sleeping_at;
                        self.sleeping_at = pos;
                        self.physics.sleeping = pos.is_some();
                        if let Some((bx, by, bz)) = pos {
                            // LivingEntity.setPositionInBed(), aufgerufen genau dann, wenn sich
                            // SLEEPING_POSITION aendert (LivingEntity.onTrackedDataSet, Zweig
                            // world.isClient): Mitte der Zelle, Y um 0.6875 versetzt.
                            self.physics.pos = DVec3::new(bx as f64 + 0.5, by as f64 + 0.6875, bz as f64 + 0.5);
                            self.physics.vel = DVec3::ZERO;
                            self.physics.refresh_on_ground(&self.world, &self.block_shapes);
                            if vorher.is_none() {
                                self.push_log("== schlafen (Schleichen zum Aufstehen) ==");
                            }
                        } else if vorher.is_some() {
                            self.push_log("== aufgewacht ==");
                        }
                    }
                }
                net::ClientEvent::EntityPose { id, pose } => {
                    self.entities.set_pose(id, pose);
                }
                net::ClientEvent::Passengers { vehicle, passengers } => {
                    self.sync_own_body();
                    self.entities.set_passengers(vehicle, &passengers);
                    self.update_own_ride(vehicle, &passengers);
                }
                net::ClientEvent::BossEvent { uuid, event } => self.on_boss_event(uuid, event),
                net::ClientEvent::VehicleMove { pos, yaw_deg, pitch_deg } => {
                    self.on_vehicle_move(pos, yaw_deg, pitch_deg);
                }
                net::ClientEvent::EntityMeta(update) => {
                    // LivingEntity.onTrackedDataSet(LIVING_FLAGS) auf dem Client: meldet der
                    // Server fuer UNS "benutzt nichts", endet die laufende Benutzung. Genau so
                    // nimmt er einem das Schild nach einem Axthieb aus der Hand
                    // (applyShieldCooldown -> clearActiveItem). Fehlte das, blieb das Schild
                    // bei uns oben, und nach der Abklingzeit ging kein neues USE_ITEM raus --
                    // man blockte serverseitig nicht mehr, obwohl die Taste gehalten war.
                    //
                    // BEIDE Richtungen, wie Vanillas LocalPlayer.onSyncedDataUpdated:
                    //     if (bl && !startedUsingItem)      startUsingItem(hand);
                    //     else if (!bl && startedUsingItem) stopUsingItem();
                    //
                    // ECHTER BUG gewesen ("Schild rastet beim Spammen ein", Grim NoSlow +
                    // MultiActionsE): hier stand nur die "stop"-Richtung, mit der Begruendung,
                    // Vanilla loese den umgekehrten Fall anders. Tut es nicht. Beim Spammen gibt
                    // es ein Rennen: wir schicken USE_ITEM, dann kommt noch ein VERALTETES "benutzt
                    // nichts" vom Server (aus der Zeit vor unserem USE_ITEM) und setzt uns
                    // zurueck. Kurz darauf meldet der Server "benutzt" -- Vanilla hebt den Schild
                    // daraufhin wieder an und schickt beim Loslassen sein RELEASE_USE_ITEM. Wir
                    // blieben unten, schickten beim Loslassen NICHTS, und der Schild blieb
                    // serverseitig oben: eingerastet. Grim rechnete ab da mit Item-Verlangsamung,
                    // wir nicht.
                    if Some(update.entity_id) == self.own_entity_id {
                        if let Some(a) = update.absorption {
                            self.absorption = a.max(0.0);
                        }
                        if let Some(t) = update.frozen_ticks {
                            self.own_frozen_ticks = t;
                        }
                        // Elytra-Flug: beendet (gelandet, im Wasser, Start abgelehnt) wird er
                        // allein vom Server -- sein Flag gilt, wie in Vanillas DataTracker.
                        if let Some(flags) = update.flags {
                            self.physics.gliding = flags & protocol::packets::ENTITY_FLAG_GLIDING != 0;
                            self.own_on_fire = flags & protocol::packets::ENTITY_FLAG_ON_FIRE != 0;
                            self.own_invisible = flags & protocol::packets::ENTITY_FLAG_INVISIBLE != 0;
                            self.own_glowing = flags & protocol::packets::ENTITY_FLAG_GLOWING != 0;
                        }
                        if let Some(farben) = &update.potion_swirls {
                            self.own_potion_swirls = farben.clone();
                        }
                        if let Some(v) = update.potion_swirls_ambient {
                            self.own_potion_swirls_ambient = v;
                        }
                        if let Some(flags) = update.living_flags {
                            // Sog-Pose: das Flag kommt allein vom Server (s. physics::using_riptide).
                            self.physics.using_riptide = flags & protocol::packets::LIVING_FLAG_USING_RIPTIDE != 0;
                            let benutzt = flags & protocol::packets::LIVING_FLAG_USING_ITEM != 0;
                            let nebenhand = flags & protocol::packets::LIVING_FLAG_OFFHAND_ACTIVE != 0;
                            // Consumable Optimizer: der Server meldet "fertig" -- nicht mehr warten.
                            // Waehrend wir selbst essen oder auf ihn warten, gilt sein Flag nicht
                            // (es ist dann veraltet, s. consumable_optimizer.rs). Mit Aggressive
                            // Prediction gilt es fuer die eigene Benutzung nie, bei keinem Item: die
                            // Verlangsamung beginnt und endet allein im Client.
                            if !benutzt {
                                self.consumable_server_done("Flag");
                            }
                            let eigenes_essen = self.consumable_optimizer_active()
                                && (self.settings.eat_aggressive_prediction
                                    || self.consumable.waiting(Instant::now())
                                    || self.own_used_item().is_some_and(|(name, st)| consumable_optimizer::consume_ticks(name, st.as_ref()).is_some()));
                            if eigenes_essen {
                            } else if benutzt && !self.using_item {
                                // LocalPlayer.startUsingItem: nur mit einem Item in dieser Hand.
                                let slot = if nebenhand {
                                    inventory::PLAYER_OFFHAND
                                } else {
                                    inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8)
                                };
                                if self.hand_stack(slot).is_some() {
                                    self.using_item = true;
                                    self.eat_held_through = false;
                                    // Neue bzw. beendete Benutzung: der Zaehler der Ess-/Spannhaltung faengt neu an. Sonst
                                    // erbte ein Bissen, der im selben Tick auf den vorigen folgt, dessen Stand (der
                                    // Apfel sprang fuer ein Bild zurueck und stand danach sofort am Mund).
                                    self.own_use_ticks = 0;
                                    self.using_offhand = nebenhand;
                                    self.using_item_id = self.hand_stack(slot).map(|st| st.item_id);
                                }
                            } else if !benutzt && self.using_item {
                                self.using_item = false;
                                // Neue bzw. beendete Benutzung: der Zaehler der Ess-/Spannhaltung faengt neu an. Sonst
                                // erbte ein Bissen, der im selben Tick auf den vorigen folgt, dessen Stand (der
                                // Apfel sprang fuer ein Bild zurueck und stand danach sofort am Mund).
                                self.own_use_ticks = 0;
                            }
                        }
                    }
                    // Kopf in einem item_display (etwa der Ball auf mcpvp.club): Profil merken wie
                    // bei der Ausruestung, geladen wird ueber request_head_skins.
                    if let Some(profil) = update.display_item.as_ref().and_then(|s| s.as_ref()).and_then(|s| s.profile.as_ref()) {
                        self.note_head_profile(profil);
                    }
                    self.entities.apply_metadata(update.entity_id, &update);
                }
                net::ClientEvent::Hardcore(h) => self.hardcore = h,
                net::ClientEvent::GameMode(mode) => {
                    if mode != self.game_mode {
                        self.previous_game_mode = Some(self.game_mode);
                    }
                    // Session-Info: Minispiele machen Tote zu Zuschauern.
                    if mode == GAME_MODE_SPECTATOR && matches!(self.game_mode, GAME_MODE_SURVIVAL | GAME_MODE_ADVENTURE) {
                        self.session.became_spectator(Instant::now());
                    }
                    self.game_mode = mode;
                    self.spectator_flight = None;
                    // GameType.updatePlayerAbilities: der Client stellt die Faehigkeiten des neuen
                    // Modus sofort selbst ein, das Abilities-Paket des Servers folgt.
                    let a = &mut self.physics.abilities;
                    a.may_fly = matches!(mode, GAME_MODE_CREATIVE | GAME_MODE_SPECTATOR);
                    a.instabuild = mode == GAME_MODE_CREATIVE;
                    a.invulnerable = a.may_fly;
                    a.flying = mode == GAME_MODE_SPECTATOR || (mode == GAME_MODE_CREATIVE && a.flying);
                }
                net::ClientEvent::SetCamera(id) => {
                    self.camera_entity = (Some(id) != self.own_entity_id).then_some(id);
                }
                net::ClientEvent::Border(update) => self.world.border.apply(update),
                net::ClientEvent::Abilities(p) => {
                    let a = &mut self.physics.abilities;
                    a.invulnerable = p.flags & protocol::packets::ABILITY_INVULNERABLE != 0;
                    a.flying = p.flags & protocol::packets::ABILITY_FLYING != 0;
                    a.may_fly = p.flags & protocol::packets::ABILITY_MAY_FLY != 0;
                    a.instabuild = p.flags & protocol::packets::ABILITY_INSTABUILD != 0;
                    a.fly_speed = p.fly_speed;
                    a.walk_speed = p.walk_speed;
                }
                net::ClientEvent::MovementAttributes { id, movement_speed, sneaking_speed, water_movement_efficiency } => {
                    if Some(id) == self.own_entity_id {
                        if let Some(v) = movement_speed {
                            self.attr_movement_speed = v;
                        }
                        if let Some(v) = sneaking_speed {
                            self.attr_sneaking_speed = v;
                        }
                        if let Some(v) = water_movement_efficiency {
                            self.attr_water_movement_efficiency = v;
                        }
                    }
                }
                net::ClientEvent::BodyAttributes { id, gravity, jump_strength, movement_efficiency, scale, step_height } => {
                    if Some(id) == self.own_entity_id {
                        let b = &mut self.attr_body;
                        if let Some(v) = gravity {
                            b.gravity = v;
                        }
                        if let Some(v) = jump_strength {
                            b.jump_strength = v;
                        }
                        if let Some(v) = movement_efficiency {
                            b.movement_efficiency = v;
                        }
                        if let Some(v) = scale {
                            b.scale = v as f32;
                        }
                        if let Some(v) = step_height {
                            b.set_step_height(v);
                        }
                        self.physics.body = self.attr_body;
                    } else if let (Some(v), Some(e)) = (scale, self.entities.get_mut(id)) {
                        e.scale = v as f32;
                    }
                }
                net::ClientEvent::AttackSpeed { id, parts } => {
                    if Some(id) == self.own_entity_id {
                        self.attr_attack_speed = Some(parts);
                    }
                }
                net::ClientEvent::RangeAttributes { id, block, entity } => {
                    if Some(id) == self.own_entity_id {
                        if let Some(v) = block {
                            self.attr_block_range = v;
                        }
                        if let Some(v) = entity {
                            self.attr_entity_range = v;
                        }
                    }
                }
                net::ClientEvent::MiningAttributes { id, block_break_speed, mining_efficiency, submerged_mining_speed } => {
                    if Some(id) == self.own_entity_id {
                        if let Some(v) = block_break_speed {
                            self.attr_block_break_speed = v as f32;
                        }
                        if let Some(v) = mining_efficiency {
                            self.attr_mining_efficiency = v as f32;
                        }
                        if let Some(v) = submerged_mining_speed {
                            self.attr_submerged_mining_speed = v as f32;
                        }
                    }
                }
                net::ClientEvent::ItemCooldown { group, ticks } => {
                    // ClientPlayNetworkHandler.onCooldownUpdate.
                    if ticks == 0 {
                        self.item_cooldowns.remove(&group);
                    } else {
                        self.item_cooldowns.set(&group, ticks);
                    }
                }
                net::ClientEvent::CustomPayload { channel, data } => {
                    if channel.starts_with("voicechat:") {
                        self.handle_voice_payload(&channel, &data);
                    } else if channel == crystal_optimizer::CHANNEL_OPT_OUT {
                        self.handle_crystal_opt_out();
                    } else if channel == consumable_optimizer::CHANNEL_DISABLE && !self.consumable.disabled_by_server {
                        self.consumable.disabled_by_server = true;
                        self.consumable.stop_waiting();
                        self.push_log("Consumable-Optimizer: vom Server abgeschaltet");
                        self.notify(notifications::Notification::warning("Consumable-Optimizer", "Dieser Server hat den Consumable-Optimizer abgeschaltet."));
                    }
                }
                net::ClientEvent::CommandSuggestions(suggestions) => {
                    self.handle_command_suggestions(suggestions);
                }
                net::ClientEvent::EntityEvent { entity_id, status } => {
                    // Operator-Stufe des eigenen Spielers (EntityEvent.PERMISSION_LEVEL_ALL ..
                    // _OWNERS = 24..28 -> Stufe 0..4).
                    if (24..=28).contains(&status) && self.own_entity_id == Some(entity_id) {
                        self.op_level = (status - 24) as u8;
                    }
                    if status == ENTITY_STATUS_KINETIC_HIT {
                        self.kinetic_hit_sound(entity_id);
                    }
                    if status == ENTITY_STATUS_DIED {
                        self.session_death(entity_id);
                        self.entity_died(entity_id);
                    }
                    if matches!(status, 47..=52 | 65) {
                        self.item_broken(entity_id, status);
                    }
                    if status == ENTITY_STATUS_THROWN_HIT {
                        self.thrown_item_hit(entity_id);
                    }
                    // Der Server hat das eigene Essen beendet (LivingEntity.completeUsingItem).
                    // Hat der Consumable-Optimizer schon selbst beendet, isst man nicht mehr --
                    // dann gab es die Kruemel schon.
                    if status == ENTITY_STATUS_USE_ITEM_COMPLETE && self.own_entity_id == Some(entity_id) && self.using_item {
                        self.own_eating_particles(true);
                    }
                    // Auch das ist das "fertig" des Servers, auf das der Consumable Optimizer
                    // wartet. pvphq meldet gemessen NUR so, das Flag "benutzt Item aus" kommt dort nie.
                    if status == ENTITY_STATUS_USE_ITEM_COMPLETE && self.own_entity_id == Some(entity_id) {
                        // Hat der Optimizer schon lokal beendet, gehoert das Event zu DIESEM Essen --
                        // eine inzwischen neu begonnene Benutzung darf es nicht abbrechen.
                        let lokal_beendet = self.consumable.waiting(Instant::now());
                        self.consumable_server_done("Event 9");
                        // Wie Vanilla (Player.handleEntityEvent -> completeUsingItem): das Essen
                        // endet hier, auch vor dem lokalen Ende. Gemessen auf pvphq: der Gap ohne
                        // Components heisst im Client 32 Ticks, der Server meldet nach ~29, und das
                        // Flag "benutzt Item aus" schickt er gar nicht -- ohne das ass man bis zum
                        // lokalen Ende weiter und wartete danach noch auf ihn.
                        if self.using_item && !lokal_beendet {
                            if self.consumable_optimizer_active() {
                                eprintln!("[essen] Server fertig (Event 9) vor dem lokalen Ende, bei Tick {}", self.own_use_ticks);
                            }
                            self.using_item = false;
                            self.own_use_ticks = 0;
                        }
                    }
                    // Totem: 30 Ticks Glitzer (ClientPlayNetworkHandler.onEntityStatus, Fall 35).
                    // Tod: 20 Rauchwoelkchen (LivingEntity.handleStatus, Fall 60). Beides fuer
                    // JEDES Entity, auch fuer einen selbst.
                    if status == protocol::packets::ENTITY_STATUS_USE_TOTEM {
                        let eigener = self.own_entity_id == Some(entity_id);
                        // Den EIGENEN Pop-Klang hat der Sofort-Haken im Netzwerk-Task schon
                        // gespielt (s. start_connect) -- hier nur fremde Pops.
                        if !eigener {
                            self.play_other_pop_sound(entity_id);
                        }
                        self.count_pop(entity_id);
                        // Pop-Optik (s. toteming.rs): Partikelwolke je eigener/fremder Pop
                        // abschaltbar.
                        let t = &self.settings.toteming;
                        let ohne_partikel = t.visuals && if eigener { t.hide_own_particles } else { t.hide_other_particles };
                        if !ohne_partikel {
                            self.with_particles(|p, ctx, body| p.add_emitter(entity_id, protocol::particle::PARTICLE_TOTEM_OF_UNDYING, 30, body, ctx));
                        }
                    } else if status == ENTITY_STATUS_DEATH_PARTICLES {
                        self.with_particles(|p, ctx, body| {
                            if let Some(b) = body(entity_id) {
                                p.add_death_poof(b, ctx);
                            }
                        });
                    }
                    self.handle_entity_event(entity_id, status);
                }
                net::ClientEvent::Particles(packet) => {
                    self.with_particles(|p, ctx, _| p.on_packet(&packet, ctx));
                }
                net::ClientEvent::ExplosionEffects(explosion) => {
                    // Der Explosionston steckt IM Paket -- einen eigenen Ton-Paket schickt der
                    // Server dafuer nicht (ClientPlayNetworkHandler.onExplosion spielt ihn mit
                    // Lautstaerke 4 und Tonhoehe (1 + (r - r) * 0.2) * 0.7).
                    if let Some(name) = explosion.sound.clone() {
                        let rng = self.particles.rng();
                        let pitch = (1.0 + (rng.next_float() - rng.next_float()) * 0.2) * 0.7;
                        let seed = rng.next_int(i32::MAX) as i64;
                        self.play_sound_effect(protocol::packets::SoundEffect {
                            name,
                            category: protocol::packets::SoundCategory::Blocks,
                            position: Some(explosion.center),
                            entity_id: None,
                            volume: 4.0,
                            pitch,
                            seed,
                        }, "explosion");
                    }
                    self.with_particles(|p, ctx, _| p.on_explosion(&explosion, ctx));
                }
                net::ClientEvent::LevelEvent(event) => {
                    // 2001: ein ANDERER hat einen Block abgebaut (der eigene Abbau kommt nicht
                    // zurueck, s. finish_block_break). Datenwert = Block-State.
                    if event.event == protocol::particle::LEVEL_EVENT_BLOCK_BROKEN {
                        self.with_particles(|p, ctx, _| p.add_block_break(event.pos, event.data as u32, ctx));
                        self.play_block_break_sound(event.pos, event.data as u32);
                    }
                    // 1010/1011: Jukebox startet ein Lied (Datenwert = jukebox_song-Index) bzw.
                    // verstummt (Vanilla LevelEventHandler).
                    // 2002/2007: Wurf- bzw. Verweiltrank zerplatzt. Den Klang schickt der Server
                    // nicht als Paket -- Vanillas LevelEventHandler spielt ihn selbst
                    // (SPLASH_POTION_BREAK, NEUTRAL, Lautstaerke 1, Tonhoehe 0.9..1.0).
                    if event.event == 2002 || event.event == 2007 {
                        let instant = event.event == 2007;
                        self.with_particles(|p, ctx, _| p.add_potion_splash(event.pos, event.data as u32, instant, ctx));
                        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0);
                        let mut rng = JavaRandom::new(seed);
                        let (x, y, z) = event.pos;
                        self.play_sound_effect(protocol::packets::SoundEffect {
                            name: "minecraft:entity.splash_potion.break".to_string(),
                            category: protocol::packets::SoundCategory::Neutral,
                            position: Some((x as f64 + 0.5, y as f64, z as f64 + 0.5)),
                            entity_id: None,
                            volume: 1.0,
                            pitch: rng.next_float() * 0.1 + 0.9,
                            seed,
                        }, "levelevent");
                    }
                    if event.event == 1010 {
                        self.start_jukebox(event.pos, event.data);
                    } else if event.event == 1011 {
                        self.stop_jukebox(event.pos);
                    }
                }
                net::ClientEvent::EntityDamaged { entity_id, damage_type } => {
                    self.entities.damaged(entity_id);
                    // Der eigene Schadenston kommt NICHT als Paket -- der Client spielt ihn
                    // selbst (s. play_own_hurt_sound).
                    if Some(entity_id) == self.own_entity_id {
                        self.play_own_hurt_sound(damage_type);
                    }
                }
                net::ClientEvent::EntityAnimation { entity_id, animation } => {
                    // Krit und verzauberter Treffer: 3 Ticks lang Funken um den Getroffenen
                    // (ClientPlayNetworkHandler.onEntityAnimation -> addEmitter). Auch fuer
                    // Treffer AUF einen selbst -- der Server schickt das Paket an alle.
                    let hit_particle = match animation {
                        protocol::packets::ANIMATION_CRIT => Some(protocol::particle::PARTICLE_CRIT),
                        protocol::packets::ANIMATION_ENCHANTED_HIT => Some(protocol::particle::PARTICLE_ENCHANTED_HIT),
                        _ => None,
                    };
                    if let Some(t) = hit_particle {
                        self.with_particles(|p, ctx, body| p.add_emitter(entity_id, t, 3, body, ctx));
                    }
                    // Nur die beiden Schwuenge interessieren hier. Aufwachen (2) betrifft das
                    // Schlafen, die beiden Treffer-Arten (4/5) sind Partikel -- beides kennt
                    // dieser Client nicht, und ein stiller Zweig ist besser als ein falscher
                    // Armschwung bei jedem Kritischen.
                    // Der kritische Treffer ist in Vanilla nur ein Partikeleffekt -- hier
                    // faerbt er wahlweise das Aufblitzen um (s. CRIT_COLOR_ROW). Gemerkt
                    // wird er IMMER, auch bei abgeschaltetem Modul: ein Zustand, der vom
                    // Einstellungsstand abhaengt, waere beim Einschalten mitten im Kampf
                    // lueckenhaft.
                    if animation == protocol::packets::ANIMATION_CRIT {
                        if Some(entity_id) == self.own_entity_id {
                            // Der eigene Spieler steht nicht in self.entities -- dieselbe
                            // Regel, eigene Felder (s. own_hurt_by_crit).
                            if self.own_hurt_ticks > 0 {
                                self.own_hurt_by_crit = true;
                            }
                            self.own_crit_grace = CRIT_GRACE_TICKS;
                        } else {
                            self.entities.crit(entity_id);
                        }
                        continue;
                    }
                    let off_hand = match animation {
                        protocol::packets::ANIMATION_SWING_MAIN_HAND => false,
                        protocol::packets::ANIMATION_SWING_OFF_HAND => true,
                        _ => continue,
                    };
                    // Ein Schwung fuer UNS SELBST. Vanilla behandelt ihn wie jeden anderen
                    // (ClientPlayNetworkHandler.onEntityAnimation -> swingHand), und beim eigenen
                    // Spieler ist swingHand ueberschrieben: ClientPlayerEntity.swingHand schwingt
                    // UND schickt ein HandSwing-Paket mit derselben Hand zurueck. Der Client
                    // "echot" den Schwung also.
                    //
                    // Genau darauf wartet Sonars Protokoll-Stufe (ProtocolHandler.sendArmAnimation):
                    // sie schickt einen Schwung fuer die Spieler-Entity und laesst erst weiter,
                    // wenn das HandSwing zurueckkommt. Ohne Echo blieb die Pruefung dort stehen.
                    // Auch im normalen Spiel kommt das vor (Server-seitige Schwuenge, s.
                    // resolve_anchor_use) -- und auch dort echot Vanilla.
                    // Immer, nicht nur mit Botcheck-Pass: das Echo kostet nichts und ist schlicht
                    // Vanillas Verhalten.
                    if Some(entity_id) == self.own_entity_id {
                        if let Some(tx) = &self.outgoing_tx {
                            let _ = tx.send(protocol::packets::swing_hand_payload(off_hand));
                        }
                        if !off_hand {
                            self.own_swing_ticks = restart_swing(self.own_swing_ticks, self.own_swing_duration());
                        }
                        continue;
                    }
                    self.entities.swing_hand(entity_id, off_hand);
                }
                net::ClientEvent::WorldTime { time_of_day, ticking } => {
                    self.world_time_of_day = time_of_day;
                    self.world_time_ticking = ticking;
                    self.sky_time_pending = false;
                }
                net::ClientEvent::DamageTypes(effects) => self.damage_type_effects = effects,
                net::ClientEvent::Enchantments(entries) => {
                    self.creative_registries.enchantment = entries.iter().map(|(n, _)| n.clone()).collect();
                    self.enchantments = world::enchantment::Enchantments::from_registry(entries);
                }
                net::ClientEvent::NamedRegistry { registry, entries } if registry == "minecraft:dialog" => {
                    self.dialog_registry = entries;
                }
                net::ClientEvent::ServerLinks(links) => self.server_links = links,
                net::ClientEvent::ShowDialog(d) => self.show_dialog_ref(d),
                net::ClientEvent::ClearDialog => self.clear_dialog(),
                net::ClientEvent::CodeOfConduct(text) => self.code_of_conduct_received(text),
                // Sonstiges > No chat clear: der Chat bleibt.
                net::ClientEvent::ResetChat if self.settings.no_chat_clear => {}
                net::ClientEvent::ResetChat => {
                    self.chat_messages.clear();
                    self.chat_times.clear();
                }
                net::ClientEvent::NamedRegistry { registry, entries } if registry == "minecraft:painting_variant" => {
                    self.creative_registries.painting_variant = entries.into_iter().map(|(n, _)| n).collect();
                }
                net::ClientEvent::NamedRegistry { registry, entries } if registry == "minecraft:jukebox_song" => {
                    // Der Klang steht im Feld sound_event -- als Name oder als eingebettete
                    // Beschreibung mit sound_id. Fehlt er (bekanntes Datenpaket ohne Daten),
                    // folgt Vanilla dem festen Schema music_disc.<name>.
                    self.jukebox_songs = entries
                        .iter()
                        .map(|(name, daten)| {
                            let klang = daten.as_ref().and_then(|d| d.get("sound_event"));
                            klang
                                .and_then(|k| k.as_str().or_else(|| k.get("sound_id").and_then(|v| v.as_str())))
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("minecraft:music_disc.{}", name.rsplit(':').next().unwrap_or(name)))
                        })
                        .collect();
                }
                net::ClientEvent::NamedRegistry { registry, entries } if registry == "minecraft:cat_variant" => {
                    // Die Textur steht in asset_id ("minecraft:entity/cat/tabby"); fehlen die
                    // Daten (bekanntes Paket), ist sie wie in Vanilla nach dem Namen benannt.
                    self.cat_variants = entries
                        .iter()
                        .map(|(name, daten)| {
                            let asset = daten.as_ref().and_then(|d| d.get("asset_id")).and_then(|v| v.as_str());
                            match asset {
                                Some(a) => {
                                    let pfad = a.strip_prefix("minecraft:").unwrap_or(a);
                                    world::entity_model::CAT_TEXTURES.iter().copied().find(|t| *t == pfad)
                                }
                                None => world::entity_model::cat_texture_for(name),
                            }
                        })
                        .collect();
                }
                net::ClientEvent::NamedRegistry { registry, entries } => {
                    if registry == "minecraft:banner_pattern" {
                        self.creative_registries.banner_pattern = entries.iter().map(|(n, _)| n.clone()).collect();
                    }
                    self.trim_registries.apply(&registry, entries);
                }
                net::ClientEvent::StopSound { category, name } => self.stop_server_sounds(category, name.as_deref()),
                net::ClientEvent::SignText { pos, text } => {
                    self.signs.insert(pos, text);
                }
                net::ClientEvent::HeadAt { pos, profile } => self.note_placed_head(pos, &profile),
                net::ClientEvent::OpenSignEditor { pos, front } => self.open_sign_editor(pos, front),
                net::ClientEvent::OpenBook { hand } => self.open_book(hand),
                net::ClientEvent::MapData(daten) => self.maps.apply(daten),
                net::ClientEvent::RainGradient(g) => self.rain_gradient = g,
                net::ClientEvent::ThunderGradient(g) => self.thunder_gradient = g,
                net::ClientEvent::InitialChunksComing => self.initial_chunks_coming = true,
                // Vanilla spielt den Treffer-Klang an den eigenen Augen: leise und tief
                // (Lautstaerke 0.18, Tonhoehe 0.45), Kategorie Spieler.
                net::ClientEvent::ProjectileHitPlayer => {
                    let auge = self.physics.eye_pos();
                    self.play_sound_effect(protocol::packets::SoundEffect {
                        name: "minecraft:entity.arrow.hit_player".to_string(),
                        category: protocol::packets::SoundCategory::Players,
                        position: Some((auge.x, auge.y, auge.z)),
                        entity_id: None,
                        volume: 0.18,
                        pitch: 0.45,
                        seed: self.start_time.elapsed().as_nanos() as i64,
                    }, "gameevent");
                }
                net::ClientEvent::ContainerContent(content) => {
                    // Der Stapel an der Maus steht MIT im Paket und muss mit uebernommen
                    // werden.
                    //
                    // ECHTER BUG gewesen (nach dem Ziehen hing das Item noch
                    // an der Maus, als haette man es nur genommen): read_container_content
                    // liest den Cursor zwar, hier wurde er aber weggeworfen. Ein voller
                    // Container-Abgleich (den der Server bei abweichender Revision schickt,
                    // und genau die entsteht mitten in einer Verteil-Kette) hat damit die
                    // Slots aktualisiert und den Cursor auf dem alten Stand gelassen -- die
                    // Items lagen also verteilt in den Slots UND zusaetzlich noch am Zeiger.
                    //
                    // Nach einem Lesefehler in der Slot-Liste steht die Leseposition falsch,
                    // dann ist der Cursor kein echter Wert (s. ContainerContent) -- den reicht
                    // der Server ohnehin per Set Cursor Item nach.
                    let cursor_gueltig = content.truncated_at.is_none();
                    let cursor = content.cursor.clone();
                    self.with_drag_suspended(|app| {
                        app.inventory.set_content(content.sync_id, content.revision, content.slots);
                        if cursor_gueltig {
                            app.inventory.cursor = cursor;
                        }
                    });
                    self.hold_quick_craft_preview();
                }
                net::ClientEvent::ContainerSlot(update) => {
                    // Wie Vanilla (handleContainerSetSlot): nur das Spielerinventar selbst, und
                    // nur wenn ein Hotbar-Platz ein Item bekommt oder mehr davon.
                    let platz = update.slot as usize;
                    if update.sync_id == 0 && update.slot >= 0 && (inventory::PLAYER_HOTBAR_START..inventory::PLAYER_HOTBAR_START + 9).contains(&platz) {
                        let alt = self.inventory.player.get(platz).cloned().flatten().filter(|s| s.count > 0).map_or(0, |s| s.count);
                        let neu = update.stack.as_ref().filter(|s| s.count > 0).map_or(0, |s| s.count);
                        if neu > 0 && alt < neu {
                            self.hotbar_pop[platz - inventory::PLAYER_HOTBAR_START] = Some(Instant::now());
                        }
                    }
                    self.with_drag_suspended(|app| app.inventory.set_slot(update.sync_id, update.revision, update.slot, update.stack));
                }
                net::ClientEvent::CursorItem(stack) => {
                    self.with_drag_suspended(|app| app.inventory.cursor = stack);
                }
                net::ClientEvent::PlayerInventorySlot(slot, stack) => {
                    self.with_drag_suspended(|app| app.inventory.set_player_slot(slot, stack));
                }
                net::ClientEvent::PlayerInfo(entries) => self.handle_player_info(entries),
                net::ClientEvent::PlayerInfoRemove(uuids) => {
                    for uuid in uuids {
                        self.tab_entries.remove(&uuid);
                    }
                }
                // Vanilla (onPlayerListHeader): ein leerer Text heisst "keine Kopf-/Fusszeile".
                net::ClientEvent::TabList { header, footer } => {
                    let (header, footer) = (self.ego_incoming(header, false), self.ego_incoming(footer, false));
                    let leer = |spans: &[protocol::nbt::TextSpan]| spans.iter().all(|s| s.text.is_empty());
                    self.tab_header = if leer(&header) { Vec::new() } else { header };
                    self.tab_footer = if leer(&footer) { Vec::new() } else { footer };
                }
                // Sonstiges > No slot set: der eigene Slot bleibt. Bei Reset glaubt
                // sync_selected_slot, der Server habe seinen Slot, und schickt im naechsten
                // Tick den eigenen -- an der Stelle, an der Vanilla Slotwechsel schickt.
                net::ClientEvent::HeldSlot(slot) if self.settings.no_slot_set != SERVER_SET_FOLLOW && (0..9).contains(&slot) => {
                    if self.settings.no_slot_set == SERVER_SET_RESET {
                        self.last_synced_slot = slot as usize;
                    }
                }
                net::ClientEvent::HeldSlot(slot) => {
                    // Der Server darf den aktiven Slot setzen (Respawn, /clear, Plugins).
                    if (0..9).contains(&slot) {
                        self.inventory.held_slot = slot as usize;
                        // Was der Spieler noch vorhatte, galt dem alten Stand.
                        self.slot_queue.clear();
                    }
                }
                net::ClientEvent::OpenScreen(screen) => {
                    self.quick_craft_pending = None;
                    // Vom Server geoeffnet (Truhe, Plugin-Menue): Zeiger wie in Vanilla. Dort
                    // zentriert erst das Entsperren der Maus (Mouse.unlockCursor) -- und das
                    // passiert nur, wenn vorher KEIN Bildschirm offen war. Schickt der Server
                    // einen neuen Bildschirm, waehrend schon einer offen ist (Seite eines
                    // Plugin-Menues umblaettern, Wechsel aus dem Inventar), bleibt der Zeiger
                    // stehen. Die Slot-Mitte (s. toggle_inventory) gilt nur fuers selbst
                    // geoeffnete Inventar.
                    let war_offen = self.inventory.screen_open;
                    self.inventory.open_screen(screen.sync_id, screen.screen_type, screen.title);
                    // Nicht, waehrend man im Pausemenue/in den Einstellungen ist: dort gehoert
                    // der Zeiger dem Menue, und ein Server, der nach einem Teleport ein Fenster
                    // oeffnet (Kit-Auswahl o.ae.), riss ihn sonst in die Mitte.
                    if !war_offen && self.mode == AppMode::World {
                        if let Some(size) = self.gpu.as_ref().map(|g| g.size()) {
                            self.place_cursor((size.0 / 2.0, size.1 / 2.0));
                        }
                    }
                    // ECHTER BUG gewesen (nach dem Rechtsklick auf ein Custom-Item blieb der
                    // Rechtsklick gedrueckt, wenn sich der Container wieder schloss): bei OFFENEM Inventarbildschirm bedienen die Maustasten die
                    // Slots, und der Ereignisstrom bricht dort ab (s. `continue` im Rohpfad).
                    // Das LOSLASSEN der Taste, mit der man den Container geoeffnet hat, kommt
                    // also nie bei apply_bind an -- use_held blieb true und die Benutzung lief
                    // nach dem Schliessen endlos weiter.
                    //
                    // Vanilla hat denselben Bruch und loest ihn genauso: MinecraftClient.
                    // setScreen ruft beim Oeffnen KeyBinding.unpressAll(). clear_controls() ist
                    // unsere Entsprechung -- und behandelt Umschalt-Bindungen richtig, genau
                    // wie Vanillas restoreToggleStates beim Schliessen.
                    self.clear_controls();
                }
                net::ClientEvent::ContainerData(sync_id, index, value) => self.inventory.set_data(sync_id, index, value),
                net::ClientEvent::MerchantOffers(offers) => self.inventory.set_merchant_offers(offers),
                net::ClientEvent::RecipeBookAdd(add) => self.recipe_book.apply_add(add),
                net::ClientEvent::RecipeBookRemove(ids) => self.recipe_book.apply_remove(&ids),
                net::ClientEvent::ItemTags(tags) => self.recipe_book.tags = tags,
                net::ClientEvent::BlockTags(tags) => world::mining::set_server_block_tags(tags),
                // Sonstiges > No UI close: der Server schliesst nichts (oder nur das eigene
                // Inventar nicht -- offen ohne Container und ohne Schild-Editor).
                net::ClientEvent::CloseScreen(_)
                    if self.settings.no_ui_close == NO_UI_CLOSE_ALWAYS
                        || (self.settings.no_ui_close == NO_UI_CLOSE_INVENTORY
                            && self.inventory.screen_open
                            && self.inventory.open.is_none()
                            && self.sign_edit.is_none()) => {}
                net::ClientEvent::CloseScreen(_sync_id) => {
                    // Vanilla schliesst bei Container Close jeden offenen Screen, ohne auf die
                    // ID zu schauen -- auch den Schild-Editor, der dabei seinen Text als Sign
                    // Update schickt. Server pruefen so den Client (Editor auf, sofort zu,
                    // Antwort mit dem vorbefuellten Text erwartet).
                    if self.sign_edit.is_some() {
                        self.finish_sign_edit();
                    }
                    self.quick_craft_pending = None;
                    self.inventory.close();
                    self.restore_held_controls();
                }
                net::ClientEvent::Chat(spans, overlay) => {
                    let mut spans = spans;
                    if !overlay {
                        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                        self.session_text(&text, false);
                        // Addons duerfen die Zeile verwerfen oder ersetzen (dann ohne Farben).
                        match self.addon_hook(coral_addon::Event::ChatIn { text }) {
                            coral_addon::Flow::Cancel => continue,
                            coral_addon::Flow::ReplaceText(t) => spans = vec![protocol::nbt::TextSpan { text: t, ..Default::default() }],
                            _ => {}
                        }
                    }
                    let spans = self.ego_incoming(spans, !overlay);
                    // overlay=true ist Actionbar (ueber der Hotbar), nicht die Chat-Liste.
                    if overlay {
                        // Die Actionbar zeichnet bisher einfarbig -- dort wird der Text
                        // zusammengezogen, statt die Abschnitte wegzuwerfen, bevor sie
                        // ueberhaupt jemand sehen koennte.
                        self.action_bar = Some((spans, Instant::now()));
                    } else {
                        self.push_chat_message(spans);
                    }
                }
                net::ClientEvent::DeathMessage { victim, killer } => {
                    let name = self.own_name().map(str::to_string);
                    self.session.death_message(&victim, killer.as_deref(), name.as_deref(), Instant::now());
                }
                net::ClientEvent::ActionBar(spans) => {
                    let spans = self.ego_incoming(spans, false);
                    self.action_bar = Some((spans, Instant::now()));
                }
                net::ClientEvent::Title(text) => {
                    self.session_text(&text.iter().map(|s| s.text.as_str()).collect::<String>(), true);
                    let text = self.ego_incoming(text, false);
                    self.title_debug("Titel", &text);
                    self.title_text = if text.iter().all(|s| s.text.is_empty()) { None } else { Some(text) };
                    self.title_set_at = Instant::now();
                }
                net::ClientEvent::Subtitle(text) => {
                    self.session_text(&text.iter().map(|s| s.text.as_str()).collect::<String>(), true);
                    let text = self.ego_incoming(text, false);
                    self.title_debug("Untertitel", &text);
                    self.subtitle_text = if text.iter().all(|s| s.text.is_empty()) { None } else { Some(text) };
                }
                net::ClientEvent::TitleFade { fade_in, stay, fade_out } => {
                    self.title_fade_in = fade_in;
                    self.title_stay = stay;
                    self.title_fade_out = fade_out;
                    self.title_set_at = Instant::now();
                }
                net::ClientEvent::ClearTitles => {
                    self.title_text = None;
                    self.subtitle_text = None;
                }
                net::ClientEvent::ScoreboardDisplay { slot, objective } => {
                    // Slot 1 = Seitenleiste (auch die teamgefaerbten 3..18 behandeln wir wie die
                    // normale Seitenleiste). Leerer Objective-Name = Leiste ausblenden.
                    if slot == 1 || (3..=18).contains(&slot) {
                        self.sb_sidebar_objective = (!objective.is_empty()).then_some(objective);
                    } else if slot == 0 {
                        // Slot 0 = Spielerliste: die Scores stehen rechts neben den Namen.
                        self.sb_list_objective = (!objective.is_empty()).then_some(objective);
                    }
                }
                net::ClientEvent::ScoreboardObjective { name, mode, display, hearts, number_format } => {
                    match mode {
                        // anlegen(0)/aktualisieren(2): Titel und Darstellung merken.
                        0 | 2 => {
                            self.sb_objective_titles.insert(name.clone(), display.unwrap_or_else(|| name.clone()));
                            if hearts {
                                self.sb_objective_hearts.insert(name.clone());
                            } else {
                                self.sb_objective_hearts.remove(&name);
                            }
                            match number_format {
                                Some(format) => self.sb_objective_formats.insert(name.clone(), format),
                                None => self.sb_objective_formats.remove(&name),
                            };
                        }
                        // entfernen(1): alles zu diesem Objective wegwerfen.
                        _ => {
                            self.sb_objective_titles.remove(&name);
                            self.sb_scores.remove(&name);
                            self.sb_objective_hearts.remove(&name);
                            self.sb_objective_formats.remove(&name);
                            self.sb_score_formats.retain(|(o, _), _| o != &name);
                            if self.sb_sidebar_objective.as_deref() == Some(name.as_str()) {
                                self.sb_sidebar_objective = None;
                            }
                            if self.sb_list_objective.as_deref() == Some(name.as_str()) {
                                self.sb_list_objective = None;
                            }
                        }
                    }
                }
                net::ClientEvent::Team { name, mode, prefix, suffix, name_tag_visibility, color, collision_rule, entries, .. } => {
                    match mode {
                        // 0 = anlegen (Info + Mitglieder), 2 = nur Info aktualisieren.
                        0 | 2 => {
                            self.sb_teams.insert(
                                name.clone(),
                                TeamInfo {
                                    prefix: prefix.unwrap_or_default(),
                                    suffix: suffix.unwrap_or_default(),
                                    name_tag_visibility: name_tag_visibility.unwrap_or(TEAM_VISIBILITY_ALWAYS),
                                    color: color.unwrap_or(-1),
                                    collision_rule: collision_rule.unwrap_or(0),
                                },
                            );
                        }
                        // 1 = Team entfernen: auch alle Mitgliedschaften darauf loesen.
                        1 => {
                            self.sb_teams.remove(&name);
                            self.sb_entry_team.retain(|_, t| t != &name);
                        }
                        _ => {}
                    }
                    match mode {
                        0 | 3 => {
                            for e in entries {
                                self.sb_entry_team.insert(e, name.clone());
                            }
                        }
                        4 => {
                            for e in entries {
                                // Nur loesen, wenn der Eintrag wirklich in DIESEM Team ist.
                                if self.sb_entry_team.get(&e).map(|t| t == &name).unwrap_or(false) {
                                    self.sb_entry_team.remove(&e);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                net::ClientEvent::ScoreboardScore { holder, objective, score, display, number_format } => {
                    let schluessel = (objective.clone(), holder.clone());
                    match number_format {
                        Some(format) => self.sb_score_formats.insert(schluessel, format),
                        None => self.sb_score_formats.remove(&schluessel),
                    };
                    self.sb_scores.entry(objective).or_default().insert(holder, (score, display));
                }
                net::ClientEvent::ScoreboardResetScore { holder, objective } => {
                    match objective {
                        Some(obj) => {
                            if let Some(m) = self.sb_scores.get_mut(&obj) {
                                m.remove(&holder);
                            }
                            self.sb_score_formats.remove(&(obj, holder));
                        }
                        // ohne Objective: Holder aus allen entfernen.
                        None => {
                            for m in self.sb_scores.values_mut() {
                                m.remove(&holder);
                            }
                            self.sb_score_formats.retain(|(_, h), _| h != &holder);
                        }
                    }
                }
                net::ClientEvent::EntitySpawn(spawn) => {
                    self.entities.spawn(
                        spawn.entity_id,
                        spawn.uuid,
                        spawn.entity_type,
                        (spawn.x, spawn.y, spawn.z),
                        spawn.yaw_deg,
                        spawn.pitch_deg,
                        spawn.vel,
                    );
                    // Ein fallender Block traegt seinen Blockzustand im Data-Feld.
                    if world::entity_type_name(spawn.entity_type) == Some("falling_block") {
                        self.entities.set_falling_block(spawn.entity_id, spawn.data as u32);
                    }
                    // Ein Rahmen traegt seine Richtung im Data-Feld.
                    if world::entity::is_item_frame(spawn.entity_type) {
                        self.entities.set_frame_facing(spawn.entity_id, spawn.data);
                    }
                    self.set_projectile_launch_offset(&spawn);
                }
                net::ClientEvent::EntityMoveRel { id, dx, dy, dz, rot } => {
                    self.entities.move_relative(id, dx, dy, dz);
                    if let Some((yaw, pitch)) = rot {
                        self.entities.set_rotation(id, yaw, pitch);
                    }
                    self.entity_update_stats.record(Instant::now());
                }
                net::ClientEvent::EntityRot { id, yaw_deg, pitch_deg } => {
                    self.entities.set_rotation(id, yaw_deg, pitch_deg);
                }
                net::ClientEvent::EntityPos { id, x, y, z, yaw_deg, pitch_deg, teleport } => {
                    if teleport {
                        self.entities.teleport(id, x, y, z, yaw_deg, pitch_deg);
                        // ClientPlayNetworkHandler.onEntityPosition: wird das selbst gelenkte
                        // Fahrzeug versetzt, meldet der Client gleich den neuen Stand zurueck.
                        if let Some(ride) = self.own_ride.as_mut().filter(|r| r.controlling && r.vehicle == id) {
                            ride.sim_pos = DVec3::new(x, y, z);
                            ride.sim_yaw = yaw_deg;
                            ride.sim_pitch = pitch_deg;
                            self.send_vehicle_move();
                        }
                    } else {
                        self.entities.set_position(id, x, y, z);
                        self.entities.set_rotation(id, yaw_deg, pitch_deg);
                    }
                    self.entity_update_stats.record(Instant::now());
                }
                net::ClientEvent::EntitiesRemove(ids) => {
                    // Verschwindet das eigene Fahrzeug, sitzt man nicht mehr.
                    if self.own_ride.as_ref().is_some_and(|r| ids.contains(&r.vehicle)) {
                        self.dismount_own();
                    }
                    self.entities.remove(&ids);
                }
                net::ClientEvent::EntityEquipment { id, entries } => {
                    for entry in entries {
                        // Item 0 ist "air" -- der Server schickt einen leeren Slot zwar als
                        // count<=0 (also None), aber ein ausdrueckliches air soll genauso
                        // eine leere Hand ergeben und nicht als unbekanntes Modell landen.
                        let item = entry.item.filter(|stack| stack.item_id != 0);
                        // Die Textur-Property eines Kopfes merken: HeldItem traegt nur den
                        // Schluessel, geladen wird ueber request_head_skins.
                        if let Some(profil) = item.as_ref().and_then(|stack| stack.profile.as_ref()) {
                            self.note_head_profile(profil);
                        }
                        let held = item.map(|stack| world::HeldItem::from_stack(&stack));
                        self.entities.set_equipment(id, entry.slot, held);
                    }
                }
                net::ClientEvent::Experience { progress, level, total: _ } => {
                    self.xp_progress = progress.clamp(0.0, 1.0);
                    self.xp_level = level.max(0);
                }
                net::ClientEvent::Attributes { id, armor, max_health } => {
                    // Attribute kommen fuer JEDES getrackte Entity -- uns interessiert nur
                    // das eigene.
                    if Some(id) == self.own_entity_id {
                        if let Some(armor) = armor {
                            self.armor = armor.clamp(0.0, 20.0) as i32;
                        }
                        if let Some(max_health) = max_health {
                            self.max_health = max_health.max(1.0) as f32;
                        }
                    }
                }
                net::ClientEvent::EntityAir { id, air } => {
                    if Some(id) == self.own_entity_id {
                        self.air_ticks = air.clamp(0, MAX_AIR_TICKS);
                    }
                }
                net::ClientEvent::Health(health, food, saturation) => {
                    // Vanillas HungerManager.canSprint(): foodLevel > 6. Darunter laesst sich
                    // kein Sprint mehr STARTEN und ein laufender endet (s. physics::Input).
                    self.food_level = food;
                    self.food_saturation = saturation;
                    // Gesunken heisst getroffen (s. own_hurt_ticks). Erst ab dem ZWEITEN
                    // Paket, sonst zaehlt der Anfangswert als Treffer.
                    if self.own_health_initialized && health < self.health {
                        self.own_hurt_ticks = world::MAX_HURT_TIME;
                        self.hurt_cam_side = -self.hurt_cam_side;
                        self.own_ticks_since_hurt = 0;
                        self.session.hurt(Instant::now());
                        self.own_hurt_by_crit = self.own_crit_grace > 0;
                    }
                    self.own_health_initialized = true;
                    self.health = health.max(0.0);
                    let dead = health <= 0.0;
                    if dead && !self.is_dead {
                        // Gerade gestorben: Gestorben-Screen erzwingen (egal, wo man war).
                        self.is_dead = true;
                        self.session.own_died(Instant::now());
                        self.mode = AppMode::Menu;
                        self.screen = MenuScreen::Death;
                        self.clear_controls();
                        self.push_log("== gestorben ==");
                    } else if !dead && self.is_dead {
                        // Leben wieder da (Respawn hat durchgegriffen) -- Screen freigeben.
                        self.is_dead = false;
                        if self.screen == MenuScreen::Death {
                            self.mode = AppMode::World;
                            self.screen = MenuScreen::Main;
                        }
                    }
                }
                net::ClientEvent::DimensionRegistry(dims) => {
                    // Kommt im Configuration-State, also vor dem Login-Paket und damit vor
                    // dem DimensionType-Index, der hier hineinzeigt.
                    self.dimension_types = dims;
                }
                net::ClientEvent::DimensionType(index) => {
                    // Die Untergrenze der Dimension. Ohne sie laegen alle Bloecke in der
                    // Hoehe der Overworld -- auf einer Welt mit min_y = 0 also 64 Bloecke zu
                    // tief (s. world::chunk::DEFAULT_MIN_Y).
                    match self.dimension_types.get(index.max(0) as usize).cloned() {
                        Some(dim) => {
                            if self.dimension_type_name != dim.name {
                                self.sky_world_pending = Some(Instant::now());
                            }
                            self.dimension_type_name = dim.name.clone();
                            if self.world.min_y() != dim.min_y {
                                self.push_log(format!("Welt-Untergrenze: {} (Dimension {})", dim.min_y, dim.name));
                            }
                            self.world.set_min_y(dim.min_y);
                            self.world_height = dim.height;
                        }
                        // Kein Eintrag: bei der Vorgabe bleiben statt zu raten.
                        None => self.push_log(format!("Unbekannte Dimensionsart {index} -- Welthöhe bleibt auf der Vorgabe")),
                    }
                }
                net::ClientEvent::BiomeRegistry(biomes) => self.apply_biome_registry(&biomes),
            }
        }
        self.replaying_backlog = false;
    }

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
                    self.grass_colormap.as_ref().map(|c| c.sample(b.temperature, b.downfall)).unwrap_or([1.0, 1.0, 1.0]);
                let foliage =
                    self.foliage_colormap.as_ref().map(|c| c.sample(b.temperature, b.downfall)).unwrap_or([1.0, 1.0, 1.0]);
                let dry_foliage =
                    self.dry_foliage_colormap.as_ref().map(|c| c.sample(b.temperature, b.downfall)).unwrap_or([1.0, 1.0, 1.0]);
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
        let player_cx = (self.physics.pos.x / world::SECTION_SIZE as f64).floor() as i64;
        let player_cz = (self.physics.pos.z / world::SECTION_SIZE as f64).floor() as i64;
        let max_distance_sq = (self.render_distance() as i64).pow(2);
        if let Some(gpu) = self.gpu.as_mut() {
            // Auf Settings::mesh_columns_per_frame gedeckelt (s. dortigen Feldkommentar) --
            // die eigentlich teure Stelle ist der GPU-Upload hier, nicht das Einreihen der
            // Jobs oben. Ohne Deckel wuerde ein Schwall gleichzeitig fertiger Ergebnisse
            // (z.B. direkt nach dem Verbinden) hunderte Buffer-Uploads in einem einzigen
            // Frame verursachen -- verbleibende Ergebnisse bleiben einfach im Kanal und
            // kommen naechsten Frame dran, nichts geht dabei verloren.
            for _ in 0..self.settings.mesh_columns_per_frame {
                let Ok(MeshResult { key, opaque, groups, runs, visibility, min_y, translucent: translucent_vertices, fade_quads, block_light }) =
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
                let (dx, dz) = (key.0 as i64 - player_cx, key.1 as i64 - player_cz);
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
            let ao_strength = if self.settings.render_path == 1 { 0.0 } else { self.settings.smooth_lighting_percent as f32 / 100.0 };
            let show_markers = self.settings.module_show_markers;
            // Licht je Vertex nur fuer den Shader-Pfad (s. world::light) -- der PvP-Pfad meshet
            // wie bisher, ohne einen einzigen Lichtwert zu berechnen.
            let mit_licht = self.settings.render_path == 1;
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
                let block_light = feld.as_ref().and_then(|f| f.column_block_light());
                let _ = tx.send(MeshResult { key, opaque, groups, runs, visibility, min_y, translucent: translucent_vertices, fade_quads, block_light });
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
        self.world.light_remesh = self.settings.render_path == 1;
        world::mesher::set_flat_face_shade(self.settings.render_path == 1);
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
        gpu.use_shader_path = self.settings.render_path == 1;

        let player_cx = (self.physics.pos.x / world::SECTION_SIZE as f64).floor() as i32;
        let player_cz = (self.physics.pos.z / world::SECTION_SIZE as f64).floor() as i32;

        // Quadrierter euklidischer Abstand statt Chebyshev (max(|dx|,|dz|)) -- Letzteres
        // ergibt ein QUADRAT (jede Spalte in der (2R+1)x(2R+1)-Box zaehlt gleich weit, auch
        // die Ecken bei vollem Diagonalabstand), sichtbar als eckige statt runde Sichtweiten-
        // Grenze. Quadriert statt Wurzel gezogen: spart sqrt() pro Spalte
        // und bleibt trotzdem exakt (a <= b^2 ist aequivalent zu sqrt(a) <= b fuer a,b >= 0),
        // Sortierschluessel bleibt eine reine Ganzzahl.
        let max_distance = self.hero_render_distance.unwrap_or(self.settings.max_render_distance) as i64;
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
                let dx = cx as i64 - player_cx as i64;
                let dz = cz as i64 - player_cz as i64;
                (dx * dx + dz * dz, cx, cz)
            }));
            self.render_limit_order.sort_unstable();
        }
        // Herausnehmen und danach zurueckgeben: die Schleife leiht sich `self` mutable
        // (world/render_limit_meshed/gpu), waehrend sie ueber die Liste laeuft. mem::take
        // loest das ohne jede Allokation -- der Puffer selbst bleibt ueber Frames erhalten.
        let columns = std::mem::take(&mut self.render_limit_order);

        // clear_dirty() laeuft JEDEN Frame fuer jede Spalte ausserhalb des Limits, nicht nur
        // fuer schon fertig gemeshte -- sonst koennte eine gerade erst dirty markierte
        // Spalte (Job eingereiht, aber noch nicht fertig) aus dem Limit fallen und ihr Job
        // liefe trotzdem noch durch (render_limit_meshed weiss ja erst NACH dem Job davon).
        // mark_dirty() dagegen NUR fuer noch nicht gemeshte Spalten -- sonst wuerde eine
        // bereits fertige, im Limit liegende Spalte JEDEN Frame erneut als dirty markiert und
        // liefe in einer Endlosschleife immer wieder durch den Mesh-Pool.
        for &(d, cx, cz) in &columns {
            let should_render = d <= max_distance_sq;
            if should_render {
                if !self.render_limit_meshed.contains(&(cx, cz)) {
                    self.world.mark_dirty(cx, cz);
                }
            } else {
                self.world.clear_dirty(cx, cz);
                if self.render_limit_meshed.remove(&(cx, cz)) {
                    self.lod.mask_changed = true;
                    gpu.block_renderer.remove_column_mesh((cx, cz));
                }
                self.column_meshed_at.remove(&(cx, cz));
            }
        }
        self.render_limit_order = columns;
    }

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

    pub(crate) fn handle_entity_event(&mut self, entity_id: i32, status: i8) {
        if status != protocol::packets::ENTITY_STATUS_USE_TOTEM || self.own_entity_id != Some(entity_id) {
            return;
        }
        // Ohne Totem-Modul: Vanillas eigene Pop-Animation, das grosse Totem vor dem Bildschirm
        // (ClientPlayNetworkHandler.onEntityStatus -> showFloatingItem). Fehlte frueher
        // ganz. Mit Modul ersetzt dessen Anzeige sie (sonst
        // liefen zwei Animationen uebereinander).
        if !self.settings.module_totem_indicator {
            // Pop-Optik: das Flug-Totem abschaltbar (showFloatingItem in der Mod).
            if self.settings.toteming.visuals && self.settings.toteming.hide_own_floating {
                return;
            }
            let zufall = |salz: u64| {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
                let h = (t ^ salz).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                (h >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
            };
            self.floating_totem = Some((modules::TOTEM_ANIMATION_TICKS, zufall(0x51), zufall(0xA7)));
            return;
        }
        // DEN TON HAT DER NETZWERK-TASK SCHON GESPIELT (s. start_connect, Sofort-Haken) --
        // hier laeuft nur noch, was ohnehin auf ein Frame warten muss: Anzeige und Log.
        // Der Ton darf NICHT auch hier stehen, sonst klingt er zweimal.
        //
        // Und die Hand wird hier nicht neu entschieden, sondern aus derselben
        // veroeffentlichten Quelle gelesen wie der Ton. Zweimal rechnen hiesse: Bild und Ton
        // koennten verschiedene Haende meinen (Gold sehen, Zweithand-Ton hoeren).
        let hand = modules::TotemHand::from_published(self.totem_hand_published.load(std::sync::atomic::Ordering::Relaxed));
        self.totem_animation = Some(modules::TotemPopAnimation::start(hand));
        self.push_log(format!("Totem gepoppt: {}", hand.label()));
    }

    // Blickneigung in VANILLA-Konvention (positiv = nach unten). Unsere Kamera zaehlt
    // umgekehrt (positiv = nach oben, s. camera.rs), deshalb das gedrehte Vorzeichen.
    //
    // Eine Quelle fuer alles, was diesen Wert braucht: das Bewegungspaket UND das eigene
    // Spielermodell. ECHTER BUG gewesen (oben und unten waren fuer die eigene Figur
    // vertauscht): das Paket drehte das Vorzeichen, das Modell nicht -- in der
    // 3.-Person-Ansicht senkte die Figur also den Kopf, wenn man nach oben schaute.
    pub(crate) fn look_pitch_deg(&self) -> f32 {
        -self.camera.pitch
    }

    // Vom Server gesetzte Blickrichtung (Protokoll-Konvention: Pitch nach unten positiv).
    // Sonstiges > No rotate set: bei Ignore/Reset bleibt die Kamera. Die Bestaetigung an den
    // Server traegt seine Richtung trotzdem (s. connection.rs) -- bei Reset tut der naechste
    // Tick so, als sei zuletzt diese gesendet, und schickt damit die eigene hinterher.
    pub(crate) fn apply_server_rotation(&mut self, yaw_deg: f32, pitch_deg: f32) {
        match self.settings.no_rotate_set {
            SERVER_SET_FOLLOW => {
                self.camera.yaw = yaw_deg;
                self.camera.pitch = (-pitch_deg).clamp(-MAX_PITCH, MAX_PITCH);
            }
            SERVER_SET_RESET => {
                self.last_sent_yaw_deg = yaw_deg;
                self.last_sent_pitch_deg = pitch_deg;
            }
            _ => {}
        }
    }

    // Ein Ereignis vom Server ist da: eine stehende "Keine Pakete"-Karte weicht der Meldung,
    // wie lange es still war.
    pub(crate) fn server_event_seen(&mut self) {
        let jetzt = Instant::now();
        let stille = jetzt.duration_since(self.last_server_event);
        self.last_server_event = jetzt;
        if let Some(id) = self.packet_gap_note.take() {
            self.notifications.dismiss(id, jetzt);
            self.notify(notifications::Notification::info("Server wieder da", format!("{:.1} s lang kamen keine Pakete.", stille.as_secs_f32())));
        }
    }

    // Seit PACKET_GAP_WARN nichts vom Server: einmal warnen, bis wieder etwas kommt. Nur im
    // Spiel -- beim Verbinden ist Warten normal.
    pub(crate) fn check_packet_gap(&mut self) {
        if self.conn_status != ConnStatus::Connected || self.chunks_received == 0 || self.packet_gap_note.is_some() {
            return;
        }
        if self.last_server_event.elapsed() >= PACKET_GAP_WARN {
            let karte = notifications::Notification::warning("Keine Pakete vom Server", format!("Seit {} s kam nichts mehr an.", PACKET_GAP_WARN.as_secs()))
                .with_lifetime(PACKET_GAP_NOTE_LIFETIME);
            self.packet_gap_note = Some(self.notify(karte));
        }
    }

    // Der Server hat uns versetzt, `strecke` Bloecke weit. Nah dran heisst Lag-back (Flag,
    // Korrektur) -- weiter weg ist ein echter Teleport (/tp, Warp) und keine Meldung wert.
    // Folgen weitere kurz danach, zaehlt die stehende Karte mit, statt zu stapeln.
    pub(crate) fn note_lagback(&mut self, strecke: f64) {
        if !self.settings.module_connection_debug || self.replaying_backlog || !(LAGBACK_MIN_BLOCKS..=LAGBACK_MAX_BLOCKS).contains(&strecke) {
            return;
        }
        let jetzt = Instant::now();
        if let Some((id, anzahl, zuletzt)) = self.lagback_note {
            let anzahl = anzahl + 1;
            if jetzt.duration_since(zuletzt) < LAGBACK_GROUP
                && self.notifications.renew(id, format!("Lag-back ×{anzahl}"), format!("Zuletzt {strecke:.1} Blöcke zurückgesetzt."))
            {
                self.lagback_note = Some((id, anzahl, jetzt));
                return;
            }
        }
        let id = self.notify(notifications::Notification::warning("Lag-back", format!("{strecke:.1} Blöcke zurückgesetzt.")));
        self.lagback_note = Some((id, 1, jetzt));
    }

    // Stockung im Paketfluss (s. net::ClientEvent::PacketStall). Solange die Karte steht,
    // zaehlt sie weitere mit.
    pub(crate) fn note_rubberband(&mut self, ausgehend: bool, millis: u32) {
        if !self.settings.module_connection_debug || self.replaying_backlog {
            return;
        }
        let text = if ausgehend {
            format!("O: {millis} ms lang ging nichts an den Server.")
        } else {
            format!("I: {millis} ms lang kam nichts, dann alles auf einmal.")
        };
        if let Some((id, anzahl)) = self.rubberband_note {
            let anzahl = anzahl + 1;
            if self.notifications.renew(id, format!("Starkes Rubberbanding ×{anzahl}"), text.clone()) {
                self.rubberband_note = Some((id, anzahl));
                return;
            }
        }
        let id = self.notify(notifications::Notification::warning("Starkes Rubberbanding", text));
        self.rubberband_note = Some((id, 1));
    }

    pub(crate) fn notify(&mut self, note: notifications::Notification) -> notifications::NotificationId {
        if self.notification_history.len() >= NOTIFICATION_HISTORY {
            self.notification_history.pop_front();
            self.notifications_seen = self.notifications_seen.saturating_sub(1);
        }
        self.notification_history.push_back((note.title.clone(), note.body.clone(), note.kind, Instant::now()));
        self.notifications.push(note, Instant::now())
    }

    // Wo der Zeiger fuer die Benachrichtigungen steht -- None, solange die Maus die Kamera
    // dreht: dann gibt es keinen Zeiger, und cursor_pos ist ein alter Stand.
    pub(crate) fn notification_pointer(&self) -> Option<(f32, f32)> {
        let frei = match self.mode {
            AppMode::Menu => true,
            AppMode::World => self.screen_overlay_open() || self.chat_input.is_some(),
        };
        (frei && self.focused).then_some(self.cursor_pos)
    }

    // Einmal je Bild, vor dem Zeichnen: Stapel, Animationen und Ziehen weiterschalten.
    pub(crate) fn tick_notifications(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let pointer = self.notification_pointer();
        let inset = if self.chrome_visible() { TOPBAR_H } else { 0.0 };
        self.notifications.set_top_inset(inset);
        self.notifications.update(Instant::now(), gpu.size(), pointer, |n| notification_layout(gpu, n).height);
    }

    // Die Karten als oberste UI-Ebene (s. UiOverlay).
    pub(crate) fn build_notifications(&self) -> UiOverlay {
        let mut overlay = UiOverlay::default();
        let Some(gpu) = self.gpu.as_ref() else { return overlay };
        // Zuerst, also unter den Karten.
        self.debug_graph_overlay(&mut overlay);
        let now = Instant::now();
        // Fallende Karten zuletzt, also obenauf: sie stuerzen an den stehenden vorbei.
        let mut karten: Vec<_> = self.notifications.cards(now).collect();
        karten.sort_by_key(|k| k.falling);
        for card in karten {
            let a = card.alpha;
            if a <= 0.0 {
                continue;
            }
            let layout = notification_layout(gpu, card.note);
            let mit_alpha = |c: [f32; 4], f: f32| [c[0], c[1], c[2], c[3] * a * f];
            let akzent = notification_accent(card.note.kind);
            let grund = theme::get(if card.hovered { theme::Slot::Surface2 } else { theme::Slot::Surface1 });
            // Gerade stehend auf ganze Pixel: der Bitmap-Font soll scharf bleiben, auch
            // waehrend die Karte gleitet. Gekippt ist das egal -- dann liegt ohnehin kein
            // Texel mehr auf einem Pixel, und Runden liesse sie nur ruckeln.
            let gerade = card.angle == 0.0;
            let px = |v: f32| if gerade { v.round() } else { v };
            let (x, y, w, h) = (px(card.x), px(card.y), card.w, card.h);
            let mut g = UiOverlayGroup { rects: Vec::new(), texts: Vec::new(), sprites: Vec::new(), pivot: card.pivot, angle: card.angle };
            // Wie die Karten im Benachrichtigungs-Panel: Flaeche mit Rahmen, Streifen innen.
            let karte = Rect { x, y, w, h };
            g.rects.push(UiRect::shadow(karte, 6.0, 4.0, 16.0, 0.45 * a));
            g.rects.push(UiRect::framed(karte, mit_alpha(grund, 1.0), 6.0, 1.0, mit_alpha(theme::get(theme::Slot::Border1), 1.0)));
            g.rects.push(UiRect::rounded(Rect { x: x + 6.0, y: y + 8.0, w: 3.0, h: h - 16.0 }, mit_alpha(akzent, 1.0), 1.5));
            // Restliche Standzeit als duenne Leiste am unteren Rand.
            let innen = w - 12.0;
            g.rects.push(UiRect::rounded(
                Rect { x: x + 6.0, y: y + h - 4.0 - NOTIFICATION_TIMER_H, w: innen * card.remaining, h: NOTIFICATION_TIMER_H },
                mit_alpha(akzent, 0.5),
                1.0,
            ));
            let tx = x + NOTIFICATION_ACCENT_W + NOTIFICATION_PADDING;
            let mut ty = y + NOTIFICATION_PADDING;
            for line in layout.title {
                g.texts.push(UiText { text: line, x: tx, y: ty, color: mit_alpha(theme::text(), 1.0), scale: UI_SCALE_SMALL });
                ty += notification_line_h(gpu);
            }
            ty += NOTIFICATION_BODY_GAP - 4.0;
            for line in layout.body {
                g.texts.push(UiText { text: line, x: tx, y: ty, color: mit_alpha(theme::text_2(), 1.0), scale: UI_SCALE_SMALL });
                ty += notification_line_h(gpu);
            }
            overlay.groups.push(g);
        }
        self.build_volume_display(&mut overlay);
        overlay
    }
}
