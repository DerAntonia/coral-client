// Server-Ereignisse (drain_events), Log, Welt zuruecksetzen, Lagback und Benachrichtigungen.

use crate::{App, Duration, FPS_LIMIT_MONITOR, FPS_LIMIT_UNLIMITED, LOG_MAX_LINES, Instant, ConnStatus, chat_log, DVec3, SlotQueue, AppMode, MenuScreen, disconnect_popup_text, LAGBACK_MIN_BLOCKS, LAGBACK_MAX_BLOCKS, teleport_velocity, physics, ShownEffect, item_cooldowns, BLOCK_INTERACTION_REACH, ENTITY_INTERACTION_REACH, consumable_optimizer, inventory, GAME_MODE_SPECTATOR, GAME_MODE_SURVIVAL, GAME_MODE_ADVENTURE, GAME_MODE_CREATIVE, crystal_optimizer, notifications, ENTITY_STATUS_KINETIC_HIT, ENTITY_STATUS_DIED, ENTITY_STATUS_THROWN_HIT, ENTITY_STATUS_USE_ITEM_COMPLETE, ENTITY_STATUS_DEATH_PARTICLES, JavaRandom, CRIT_GRACE_TICKS, restart_swing, SERVER_SET_FOLLOW, SERVER_SET_RESET, NO_UI_CLOSE_ALWAYS, NO_UI_CLOSE_INVENTORY, TeamInfo, TEAM_VISIBILITY_ALWAYS, MAX_AIR_TICKS, saturate, MeshResult, AoFade, transactions, VANILLA_LOAD_TIMEOUT, VANILLA_CHUNK_FADE_READY, modules, MAX_PITCH, PACKET_GAP_WARN, PACKET_GAP_NOTE_LIFETIME, LAGBACK_GROUP, NOTIFICATION_HISTORY, TOPBAR_H, notification_layout, UiOverlay, notification_accent, theme, UiOverlayGroup, Rect, UiRect, NOTIFICATION_TIMER_H, NOTIFICATION_ACCENT_W, NOTIFICATION_PADDING, UiText, UI_SCALE_SMALL, notification_line_h, NOTIFICATION_BODY_GAP};

mod world_mesh;
mod world_reset;
mod server_events;
mod notice;

impl App {
    // Der Deckel: entweder die Refresh-Rate des Monitors oder ein fester Wert aus den
    // Einstellungen.
    pub(crate) fn effective_frame_interval(&self) -> Duration {
        match self.settings.fps_limit {
            FPS_LIMIT_MONITOR => self.monitor_frame_interval,
            // Kein Deckel: nie schlafen. next_frame_time bleibt dadurch immer schon
            // erreicht, die Schleife rendert so schnell sie kann (s. about_to_wait).
            FPS_LIMIT_UNLIMITED => Duration::ZERO,
            fps => Duration::from_secs_f64(1.0 / f64::from(fps)),
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
        self.snapshot_config.clear();
        self.textures_props.clear();
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
            self.snapshot_note_config(&event);
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
                net::ClientEvent::Connected => self.on_connected(),
                net::ClientEvent::Disconnected(reason) => self.on_disconnected(reason),
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
                    // Die Registries kommen neu, die Server-Packs bleiben.
                    self.snapshot_config.retain(|(pack, _)| *pack);
                }
                net::ClientEvent::Route(ziel) => self.conn_route = Some(ziel),
                net::ClientEvent::RecordingStarted(session) => self.write_snapshot(session),
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
                net::ClientEvent::Chunk(chunk) => self.on_chunk(chunk),
                net::ClientEvent::PlayerPosition(sync) => self.on_player_position(sync),
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
                net::ClientEvent::ForgetChunk(x, z) => self.on_forget_chunk(x, z),
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
                net::ClientEvent::OwnEntityId(id) => self.on_own_entity_id(id),
                net::ClientEvent::WorldIdentity { dimension, seed } => self.lod_world_identity(dimension, seed),
                net::ClientEvent::Respawned { world_change } => self.on_respawned(world_change),
                net::ClientEvent::OwnUuid(uuid) => {
                    self.own_uuid = Some(uuid);
                }
                net::ClientEvent::EntityHeadYaw { id, head_yaw_deg } => {
                    if let Some(entity) = self.entities.get_mut(id) {
                        entity.set_head_yaw(head_yaw_deg);
                    }
                }
                net::ClientEvent::EntitySleeping { id, pos } => self.on_entity_sleeping(id, pos),
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
                net::ClientEvent::EntityMeta(update) => self.on_entity_meta(update),
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
                net::ClientEvent::BodyAttributes { id, gravity, jump_strength, movement_efficiency, scale, step_height } => self.on_body_attributes(id, gravity, jump_strength, movement_efficiency, scale, step_height),
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
                net::ClientEvent::ItemCooldown { group, ticks } => self.on_item_cooldown(group, ticks),
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
                net::ClientEvent::EntityEvent { entity_id, status } => self.on_entity_event(entity_id, status),
                net::ClientEvent::Particles(packet) => {
                    self.with_particles(|p, ctx, _| p.on_packet(&packet, ctx));
                }
                net::ClientEvent::ExplosionEffects(explosion) => self.on_explosion_effects(explosion),
                net::ClientEvent::LevelEvent(event) => self.on_level_event(event),
                net::ClientEvent::EntityDamaged { entity_id, damage_type } => {
                    self.entities.damaged(entity_id);
                    // Der eigene Schadenston kommt NICHT als Paket -- der Client spielt ihn
                    // selbst (s. play_own_hurt_sound).
                    if Some(entity_id) == self.own_entity_id {
                        self.play_own_hurt_sound(damage_type);
                    }
                }
                net::ClientEvent::EntityAnimation { entity_id, animation } => self.on_entity_animation(entity_id, animation),
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
                                .and_then(|k| k.as_str().or_else(|| k.get("sound_id").and_then(|v| v.as_str()))).map_or_else(|| format!("minecraft:music_disc.{}", name.rsplit(':').next().unwrap_or(name)), str::to_string)
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
                net::ClientEvent::ContainerContent(content) => self.on_container_content(content),
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
                net::ClientEvent::OpenScreen(screen) => self.on_open_screen(screen),
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
                net::ClientEvent::Chat(spans, overlay) => self.on_chat(spans, overlay),
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
                net::ClientEvent::ScoreboardObjective { name, mode, display, hearts, number_format } => self.on_scoreboard_objective(name, mode, display, hearts, number_format),
                net::ClientEvent::Team { name, mode, prefix, suffix, name_tag_visibility, color, collision_rule, entries, .. } => self.on_team(name, mode, prefix, suffix, name_tag_visibility, color, collision_rule, entries),
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
                net::ClientEvent::EntitySpawn(spawn) => self.on_entity_spawn(spawn),
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
                net::ClientEvent::Health(health, food, saturation) => self.on_health(health, food, saturation),
                net::ClientEvent::DimensionRegistry(dims) => {
                    // Kommt im Configuration-State, also vor dem Login-Paket und damit vor
                    // dem DimensionType-Index, der hier hineinzeigt.
                    self.dimension_types = dims;
                }
                net::ClientEvent::DimensionType(index) => self.on_dimension_type(index),
                net::ClientEvent::BiomeRegistry(biomes) => self.apply_biome_registry(&biomes),
            }
        }
        self.replaying_backlog = false;
    }

    fn on_forget_chunk(&mut self, x: i32, z: i32) {
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

    fn on_item_cooldown(&mut self, group: String, ticks: i32) {
        // ClientPlayNetworkHandler.onCooldownUpdate.
        if ticks == 0 {
            self.item_cooldowns.remove(&group);
        } else {
            self.item_cooldowns.set(&group, ticks);
            // Schild deaktiviert (BlocksAttacks.disable): der Server setzt den Cooldown
            // und beendet das Blocken ueber das Benutzt-Flag -- das bei Aggressive
            // Prediction nicht mehr ankommt. Der Cooldown fuers gerade blockende Item
            // beendet es darum auch hier. Nur bei Blockern: das Ziegenhorn bekommt
            // seinen Cooldown schon zu Beginn der Benutzung.
            let hand = if self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
            let blockt = self.own_hand_stack(hand).filter(|st| world::item_behavior::blocks_attacks(st).is_some());
            if self.using_item && blockt.is_some_and(|st| item_cooldowns::group_of(st).as_deref() == Some(group.as_str())) {
                self.using_item = false;
                self.own_use_ticks = 0;
            }
        }
    }

    fn on_container_content(&mut self, content: protocol::packets::ContainerContent) {
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

    fn on_open_screen(&mut self, screen: protocol::packets::OpenScreen) {
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
            if let Some(size) = self.gpu.as_ref().map(super::super::GpuState::size) {
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

    fn on_chat(&mut self, spans: Vec<protocol::nbt::TextSpan>, overlay: bool) {
        let mut spans = spans;
        if !overlay {
            let text: String = spans.iter().map(|s| s.text.as_str()).collect();
            self.session_text(&text, false);
            // Addons duerfen die Zeile verwerfen oder ersetzen (dann ohne Farben).
            match self.addon_hook(coral_addon::Event::ChatIn { text }) {
                coral_addon::Flow::Cancel => return,
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

    fn on_scoreboard_objective(&mut self, name: String, mode: u8, display: Option<String>, hearts: bool, number_format: Option<protocol::packets::NumberFormat>) {
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

    fn on_team(&mut self, name: String, mode: u8, prefix: Option<String>, suffix: Option<String>, name_tag_visibility: Option<u8>, color: Option<i32>, collision_rule: Option<u8>, entries: Vec<String>) {
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
                    if self.sb_entry_team.get(&e).is_some_and(|t| t == &name) {
                        self.sb_entry_team.remove(&e);
                    }
                }
            }
            _ => {}
        }
    }

    fn on_entity_spawn(&mut self, spawn: protocol::packets::AddEntity) {
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

    fn on_health(&mut self, health: f32, food: i32, saturation: f32) {
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

    fn on_dimension_type(&mut self, index: i32) {
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

    fn on_connected(&mut self) {
        let server = self.server_address.clone();
        self.addon_hook(coral_addon::Event::Join { server });
        self.conn_status = ConnStatus::Connected;
        // Mit dem Spiel beginnt ein neuer Bildschirm: Fenster aus der
        // Konfiguration sind vorbei.
        self.reset_dialogs();
        self.reset_own_entity_flags();
        if !self.replay_active() {
            chat_log::session_start(&self.server_address);
        }
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
        if !self.replay_active() {
            self.request_profile_key();
        }
        // Automatisch in die Welt, sobald die Verbindung steht -- aber nur, wenn
        // der Nutzer noch auf dem Startbildschirm wartet (nicht, wenn er
        // inzwischen in den Einstellungen o.ae. blaettert; das waere ein
        // ueberraschender Sprung).
        if self.mode == AppMode::Menu && self.screen == MenuScreen::Main {
            self.mode = AppMode::World;
            self.restore_held_controls();
        }
    }

    fn on_disconnected(&mut self, reason: String) {
        self.addon_hook(coral_addon::Event::Leave { reason: reason.clone() });
        // Eine Wiedergabe ist kein Blick auf den gewaehlten Server.
        if self.replay.take().is_none() {
            self.keep_last_view();
        }
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

    fn on_chunk(&mut self, chunk: protocol::chunk::ChunkData) {
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

    fn on_player_position(&mut self, sync: protocol::packets::PlayerPositionSync) {
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
            return;
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

    fn on_own_entity_id(&mut self, id: i32) {
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

    fn on_respawned(&mut self, world_change: bool) {
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

    fn on_entity_sleeping(&mut self, id: i32, pos: Option<(i32, i32, i32)>) {
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
                self.physics.pos = DVec3::new(f64::from(bx) + 0.5, f64::from(by) + 0.6875, f64::from(bz) + 0.5);
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

    fn on_entity_meta(&mut self, update: Box<protocol::packets::EntityDataUpdate>) {
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

    fn on_body_attributes(&mut self, id: i32, gravity: Option<f64>, jump_strength: Option<f64>, movement_efficiency: Option<f64>, scale: Option<f64>, step_height: Option<f64>) {
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

    fn on_entity_event(&mut self, entity_id: i32, status: i8) {
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

    fn on_explosion_effects(&mut self, explosion: protocol::particle::Explosion) {
        // Der Explosionston steckt IM Paket -- einen eigenen Ton-Paket schickt der
        // Server dafuer nicht (ClientPlayNetworkHandler.onExplosion spielt ihn mit
        // Lautstaerke 4 und Tonhoehe (1 + (r - r) * 0.2) * 0.7).
        if let Some(name) = explosion.sound.clone() {
            let rng = self.particles.rng();
            let pitch = (1.0 + (rng.next_float() - rng.next_float()) * 0.2) * 0.7;
            let seed = i64::from(rng.next_int(i32::MAX));
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

    fn on_level_event(&mut self, event: protocol::particle::LevelEvent) {
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
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
            let mut rng = JavaRandom::new(seed);
            let (x, y, z) = event.pos;
            self.play_sound_effect(protocol::packets::SoundEffect {
                name: "minecraft:entity.splash_potion.break".to_string(),
                category: protocol::packets::SoundCategory::Neutral,
                position: Some((f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5)),
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

    fn on_entity_animation(&mut self, entity_id: i32, animation: u8) {
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
            return;
        }
        let off_hand = match animation {
            protocol::packets::ANIMATION_SWING_MAIN_HAND => false,
            protocol::packets::ANIMATION_SWING_OFF_HAND => true,
            _ => return,
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
            return;
        }
        self.entities.swing_hand(entity_id, off_hand);
    }
}
