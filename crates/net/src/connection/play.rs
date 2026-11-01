// Spielphase: Statistik, Ablauf und die Behandlung der Play-Pakete.

use super::*;

// Zaehlwerk des Play-Loops. Frueher lose &mut-Parameter von handle_play_packet -- als Struct
// zusammengefasst, weil der Wachhund oben denselben Stand braucht.
#[derive(Default)]
pub(super) struct PlayStats {
    pub(super) chunks_ok: usize,
    pub(super) chunks_failed: usize,
    // Einmal gemeldet: ein Chunk mit unlesbarem Rest hinter den Sections (s. protocol::chunk).
    pub(super) chunk_tail_logged: bool,
    pub(super) entity_errors: usize,
    // Alle Play-Pakete seit dem Betreten des Play-States -- die Zahl beantwortet die erste
    // Frage bei einem Haenger: kommt ueberhaupt noch etwas an?
    pub(super) packets: usize,
    pub(super) batches: usize,
    pub(super) last_batch_size: i32,
    pub(super) acks_sent: usize,
    // Jede nicht behandelte Packet-ID einmal (gedeckelt). Kommen Chunks unter einer ID an,
    // die wir nicht kennen, steht sie hier -- sonst waere dieser Fall unsichtbar.
    pub(super) unknown_ids: Vec<i32>,
    // Nur unter CORAL_SOUND_DEBUG gefuellt: wie oft jede Paket-ID kam, und unter welchen
    // IDs ein Paket sich als Ton-Paket lesen liess (s. packets::looks_like_sound). Damit ist
    // "es kommt kein Ton an" beantwortbar, ohne IDs zu raten -- die Antwort steht dann
    // entweder in einer leeren Liste (der Server schickt wirklich nichts) oder in einer ID,
    // die wir nicht als Ton behandeln.
    pub(super) id_counts: std::collections::BTreeMap<i32, u32>,
    pub(super) sound_hits: std::collections::BTreeMap<i32, (u32, String)>,
    pub(super) last_dump: Option<std::time::Instant>,
    // Markierung fuer die einmalige CORAL_PACKET_SCAN-Ausgabe des ersten Play-Pakets.
    pub(super) first_packet_scanned: bool,
    // Welt-RegistryKey der Dimension, in der wir gerade stecken (aus Login bzw. Respawn).
    // Nur dafuer da, beim naechsten Respawn Tod (gleiche Dimension) von Portal (andere
    // Dimension) zu unterscheiden -- s. packets::CB_PLAY_RESPAWN.
    pub(super) dimension: Option<String>,
}

impl PlayStats {
    pub(super) const MAX_UNKNOWN_IDS: usize = 16;

    // Alle 10 Sekunden ein Lagebild. Bewusst nicht je Paket: in einer belebten Szene waeren
    // das hunderte Zeilen je Sekunde, und die eine Zeile, auf die es ankommt, ginge unter.
    pub(super) fn maybe_dump_sound_report(&mut self) {
        const INTERVALL: std::time::Duration = std::time::Duration::from_secs(10);
        let jetzt = std::time::Instant::now();
        match self.last_dump {
            // Noch keine volle Runde vorbei.
            Some(t) if jetzt.duration_since(t) < INTERVALL => return,
            // Erstes Paket: die Uhr stellen, aber noch nichts berichten -- ein Lagebild ueber
            // ein einziges Paket sagt nichts.
            None => {
                self.last_dump = Some(jetzt);
                return;
            }
            _ => self.last_dump = Some(jetzt),
        }

        eprintln!("[ton] --- Lagebild: {} Play-Pakete, {} verschiedene IDs ---", self.packets, self.id_counts.len());
        if self.sound_hits.is_empty() {
            eprintln!("[ton] KEIN einziges Paket liess sich als Ton-Paket lesen.");
            // Die haeufigsten IDs mitgeben: wenn der Server ueberhaupt nichts schickt, sieht
            // man das hier sofort an einer fast leeren Liste.
            let mut top: Vec<_> = self.id_counts.iter().map(|(id, n)| (*n, *id)).collect();
            top.sort_unstable_by(|a, b| b.cmp(a));
            let liste = top.iter().take(12).map(|(n, id)| format!("0x{id:02x}x{n}")).collect::<Vec<_>>().join(" ");
            eprintln!("[ton] haeufigste IDs: {liste}");
        } else {
            for (id, (n, name)) in &self.sound_hits {
                let behandelt = *id == packets::CB_PLAY_SOUND || *id == packets::CB_PLAY_SOUND_ENTITY;
                eprintln!(
                    "[ton] ID 0x{id:02x}: {n}x als Ton lesbar, zuletzt {name}{}",
                    if behandelt { "" } else { "   <-- DIESE ID BEHANDELN WIR NICHT" }
                );
            }
        }
    }

    pub(super) fn note_unknown_id(&mut self, id: i32) -> bool {
        if self.unknown_ids.contains(&id) {
            return false;
        }
        if self.unknown_ids.len() < Self::MAX_UNKNOWN_IDS {
            self.unknown_ids.push(id);
        }
        true
    }

    pub(super) fn chunk_stall_report(&self, wartet: std::time::Duration) -> String {
        let ids = if self.unknown_ids.is_empty() {
            String::from("keine")
        } else {
            self.unknown_ids.iter().map(|id| format!("0x{id:02X}")).collect::<Vec<_>>().join(" ")
        };
        format!(
            "keine Chunk-Daten seit {:.0}s: {} Pakete, {} Stapel (zuletzt {} Chunks), {} Fehl-Chunks, {} Bestaetigungen, unbekannte IDs: {ids}",
            wartet.as_secs_f32(),
            self.packets,
            self.batches,
            self.last_batch_size,
            self.chunks_failed,
            self.acks_sent,
        )
    }
}

// Ausgelagert aus run_play_loop, damit die select!-Schleife dort uebersichtlich bleibt --
// diese Funktion entscheidet nur noch, was mit EINEM eingehenden Packet zu tun ist.
// Entity-Parse-Fehler: die ersten paar melden, danach schweigen (nicht das Log fluten --
// bei einem Formatfehler kommen die Pakete im 20-Hz-Takt).
// CORAL_SOUND_DEBUG=1: schreibt jedes eingehende Ton-Paket mit -- Name, Kategorie, Ort
// und ob es ueberhaupt lesbar war. Gebaut, weil "ich hoere nichts" von aussen nicht
// unterscheidbar ist zwischen: der Server schickt gar nichts, das Paket ist unlesbar, der
// Klang ist unbekannt, die Quelle ist nicht auffindbar oder die Datei fehlt. Die Antwort
// steht in genau einer dieser Zeilen (Gegenstueck in app::play_server_sound).
//
// Einmal abgefragt und gemerkt: das laeuft im Paketpfad, und dort will man keine
// Umgebungsvariable je Geraeusch nachschlagen.
pub fn sound_debug() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("CORAL_SOUND_DEBUG").is_some())
}

pub(super) fn log_entity_parse_error(events: &Events, entity_errors: &mut usize, kind: &str) {
    *entity_errors += 1;
    if *entity_errors <= 3 {
        log(events, format!("FEHLER: Entity-Packet nicht dekodierbar ({kind}, Fall {entity_errors})"));
    }
}

// Was der Play-Loop nach diesem Paket tun soll. Nur der Zustandswechsel muss nach oben
// gegeben werden -- alles andere erledigt handle_play_packet selbst.
pub(super) enum PlayFlow {
    Continue,
    Reconfigure,
    Transfer(TransferTarget),
}

pub(super) async fn handle_play_packet(
    conn: &mut Connection,
    events: &Events,
    payload: &[u8],
    stats: &mut PlayStats,
    chunk_batch: &mut crate::chunk_batch::ChunkBatchSizeCalculator,
    rotation: &SharedRotation,
) -> Result<PlayFlow, NetError> {
    let id = packets::peek_packet_id(payload).map_err(|_| NetError::Protocol("leeres Play-Packet".into()))?;
    stats.packets += 1;
    if events.wants_raw() {
        let _ = events.send(ClientEvent::RawPacket(payload.to_vec()));
    }

    if sound_debug() {
        *stats.id_counts.entry(id).or_default() += 1;
        // Jedes Paket versuchsweise als Ton lesen -- unabhaengig davon, unter welcher ID es
        // kam. Kommen Toene unter einer anderen ID an, steht es damit im Bericht, statt dass
        // erneut Paket-IDs abgezaehlt werden muessten.
        if let Some((entity_gebunden, name)) = packets::looks_like_sound(payload) {
            let eintrag = stats.sound_hits.entry(id).or_insert_with(|| (0, String::new()));
            eintrag.0 += 1;
            eintrag.1 = format!("{name}{}", if entity_gebunden { " (an Entity)" } else { "" });
        }
        stats.maybe_dump_sound_report();
    }

    // TEMPORAERE Diagnose (CORAL_PACKET_SCAN=1): Login (play) ist garantiert das ALLERERSTE
    // Play-Packet -- loggt dessen ID + erste Bytes, um playerId (erstes Feld, 4-Byte-Int laut
    // ClientboundLoginPacket) ohne Raten zu lokalisieren.
    if std::env::var_os("CORAL_PACKET_SCAN").is_some() && !stats.first_packet_scanned {
        eprintln!("[scan] ERSTES Play-Packet: id=0x{id:02x} bytes={:02x?}", &payload[..payload.len().min(24)]);
        stats.first_packet_scanned = true;
    }

    if id == packets::CB_PLAY_START_CONFIGURATION {
        return Ok(PlayFlow::Reconfigure);
    }
    if id == packets::CB_PLAY_TRANSFER {
        let ziel = packets::read_transfer(payload).map_err(|_| NetError::Protocol("Transfer-Paket nicht lesbar".into()))?;
        return Ok(PlayFlow::Transfer(ziel));
    }

    if id == packets::CB_PLAY_CHUNK_DATA {
        return on_chunk_data(events, payload, stats);
    } else if id == packets::CB_PLAY_CHUNK_BATCH_START {
        chunk_batch.on_start_chunk_send();
    } else if id == packets::CB_PLAY_CHUNK_BATCH_FINISHED {
        // PFLICHT-ANTWORT, nicht Kosmetik: der Server sendet erst weiter, wenn dieser
        // Stapel bestaetigt ist (ChunkDataSender.maxUnacknowledgedBatches startet bei 1).
        // Deshalb auch bei unlesbarer Groesse antworten -- ohne Bestaetigung stuende die
        // Chunk-Lieferung fuer immer, mit einer ungenauen Tempo-Angabe nur ein Stapel.
        let batch_size = packets::read_chunk_batch_size(payload).unwrap_or(0);
        chunk_batch.on_chunk_sent(batch_size);
        let desired = chunk_batch.desired_chunks_per_tick();
        stats.batches += 1;
        stats.last_batch_size = batch_size;
        // Solange noch kein Chunk angekommen ist, JEDEN Stapel melden (nicht nur den ersten):
        // beim Haenger ist gerade die Frage entscheidend, ob der Server weiter Stapel schickt
        // -- und wenn ja, ob leere. Nach dem 3. Stapel uebernimmt der Wachhund das Zaehlen,
        // sonst waeren die 6 Zeilen des Verbindungs-Overlays sofort vollgeschrieben.
        if stats.chunks_ok == 0 && stats.batches <= 3 {
            log(events, format!("Chunk-Stapel {} bestaetigt ({batch_size} Chunks, Wunsch {desired:.1}/Tick)", stats.batches));
        }
        stats.acks_sent += 1;
        conn.reply(events, &packets::chunk_batch_received_payload(desired)).await?;
    } else if id == packets::CB_PLAY_LOGIN {
        if let Ok(entity_id) = packets::read_login_player_id(payload) {
            let _ = events.send(ClientEvent::OwnEntityId(entity_id));
        }
        // Ausgangspunkt fuer den Dimensionsvergleich beim ersten Respawn.
        stats.dimension = packets::read_login_dimension(payload).ok();
        if let Ok((dimension, seed)) = packets::read_login_world(payload) {
            let _ = events.send(ClientEvent::WorldIdentity { dimension, seed });
        }
        if let Ok(index) = packets::read_login_dimension_type(payload) {
            let _ = events.send(ClientEvent::DimensionType(index));
        }
        if let Ok(mode) = packets::read_login_game_mode(payload) {
            let _ = events.send(ClientEvent::GameMode(mode));
        }
        if let Ok(hardcore) = packets::read_login_hardcore(payload) {
            let _ = events.send(ClientEvent::Hardcore(hardcore));
        }
    } else if id == packets::CB_PLAY_RESPAWN {
        // Die Dimension entscheidet, ob es ein Weltwechsel war (s. packets::CB_PLAY_RESPAWN).
        // Laesst sie sich nicht lesen oder kennen wir die alte nicht, gilt das NICHT als
        // Wechsel: ein faelschlich zurueckgesetzter Zaehler flaggt garantiert, ein nicht
        // zurueckgesetzter nur, wenn wirklich die Dimension gewechselt hat.
        let neue_dimension = packets::read_respawn_dimension(payload).ok();
        let world_change = match (&stats.dimension, &neue_dimension) {
            (Some(alt), Some(neu)) => alt != neu,
            _ => false,
        };
        if neue_dimension.is_some() {
            stats.dimension = neue_dimension;
        }
        if let Ok(index) = packets::read_respawn_dimension_type(payload) {
            let _ = events.send(ClientEvent::DimensionType(index));
        }
        if let Ok((dimension, seed)) = packets::read_respawn_world(payload) {
            let _ = events.send(ClientEvent::WorldIdentity { dimension, seed });
        }
        let _ = events.send(ClientEvent::Respawned { world_change });
        // NACH Respawned: die App setzt dort den Spieler zurueck.
        if let Ok(mode) = packets::read_respawn_game_mode(payload) {
            let _ = events.send(ClientEvent::GameMode(mode));
        }
    } else if id == packets::CB_PLAY_GAME_EVENT {
        return on_game_event(events, payload);
    } else if id == packets::CB_PLAY_KEEP_ALIVE {
        let ka = packets::read_keep_alive_id(payload).map_err(|_| NetError::Protocol("KeepAlive".into()))?;
        conn.send(&packets::keep_alive_play_payload(ka)).await?;
    } else if id == packets::CB_PLAY_COOKIE_REQUEST {
        answer_cookie_request(conn, payload, packets::SB_PLAY_COOKIE_RESPONSE).await?;
    } else if id == packets::CB_PLAY_STORE_COOKIE {
        store_cookie(conn, payload);
    } else if id == packets::CB_PLAY_LIGHT_UPDATE {
        if let Ok((x, z, light)) = protocol::chunk::parse_light_update(payload) {
            let _ = events.send(ClientEvent::LightUpdate { x, z, light });
        }
    } else if id == packets::CB_PLAY_RESOURCE_PACK_PUSH {
        match packets::read_resource_pack_push(payload) {
            Ok(push) => {
                let _ = events.send(ClientEvent::ResourcePackPush(push));
            }
            Err(_) => log(events, "Resourcepack-Anfrage nicht lesbar -- ignoriert"),
        }
    } else if id == packets::CB_PLAY_RESOURCE_PACK_POP {
        if let Ok(id) = packets::read_resource_pack_pop(payload) {
            let _ = events.send(ClientEvent::ResourcePackPop(id));
        }
    } else if id == packets::CB_PLAY_SERVER_LINKS {
        match packets::read_server_links(payload) {
            Ok(links) => {
                let _ = events.send(ClientEvent::ServerLinks(links));
            }
            Err(_) => log(events, "Server-Links nicht lesbar -- ignoriert"),
        }
    } else if id == packets::CB_PLAY_SHOW_DIALOG {
        match packets::read_show_dialog_play(payload) {
            Ok(dialog) => {
                let _ = events.send(ClientEvent::ShowDialog(dialog));
            }
            Err(_) => log(events, "Dialog nicht lesbar -- ignoriert"),
        }
    } else if id == packets::CB_PLAY_CLEAR_DIALOG {
        let _ = events.send(ClientEvent::ClearDialog);
    } else if id == packets::CB_PLAY_CUSTOM_REPORT_DETAILS {
        // Zusaetze fuer Absturzberichte -- dieser Client schreibt keine.
    } else if id == packets::CB_PLAY_PING {
        let ping = packets::read_ping_id(payload).map_err(|_| NetError::Protocol("Ping".into()))?;
        interaktions_log("pong");
        // Auch im Hintergrund-Tab sofort: Anticheats (TotemGuard, Grim) kicken mit "Timed
        // out", wenn ihre Pings zu lange unbeantwortet bleiben -- KeepAlive allein reicht
        // ihnen nicht. Vorn geht Zurueckgehaltenes trotzdem zuerst raus.
        if !events.in_background() {
            conn.flush_held().await?;
        }
        conn.send(&packets::pong_play_payload(ping)).await?;
        let _ = events.send(ClientEvent::TransactionPing(ping));
    } else if id == packets::CB_PLAY_PLAYER_POSITION {
        return on_player_position(conn, events, payload, rotation).await;
    } else if id == packets::CB_PLAY_PLAYER_ROTATION {
        return on_player_rotation(conn, events, payload, rotation).await;
    } else if id == packets::CB_PLAY_FORGET_LEVEL_CHUNK {
        if let Ok((x, z)) = packets::read_forget_level_chunk(payload) {
            let _ = events.send(ClientEvent::ForgetChunk(x, z));
        }
    } else if id == packets::CB_PLAY_ADD_ENTITY {
        // Parse-Fehler bei Entity-Paketen: Format falsch verstanden. Wie bei Chunks nicht
        // die Verbindung abbrechen, aber die ersten Faelle melden, damit es auffaellt.
        match packets::read_add_entity(payload) {
            Ok(spawn) => {
                let _ = events.send(ClientEvent::EntitySpawn(spawn));
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "AddEntity"),
        }
    } else if id == packets::CB_PLAY_MOVE_ENTITY_POS {
        match packets::read_move_entity_pos(payload) {
            Ok((eid, dx, dy, dz)) => {
                let _ = events.send(ClientEvent::EntityMoveRel { id: eid, dx, dy, dz, rot: None });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "MoveEntityPos"),
        }
    } else if id == packets::CB_PLAY_MOVE_ENTITY_POS_ROT {
        match packets::read_move_entity_pos_rot(payload) {
            Ok((eid, dx, dy, dz, yaw, pitch)) => {
                let _ = events.send(ClientEvent::EntityMoveRel { id: eid, dx, dy, dz, rot: Some((yaw, pitch)) });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "MoveEntityPosRot"),
        }
    } else if id == packets::CB_PLAY_MOVE_ENTITY_ROT {
        match packets::read_move_entity_rot(payload) {
            Ok((eid, yaw, pitch)) => {
                let _ = events.send(ClientEvent::EntityRot { id: eid, yaw_deg: yaw, pitch_deg: pitch });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "MoveEntityRot"),
        }
    } else if id == packets::CB_PLAY_ENTITY_POSITION_SYNC || id == packets::CB_PLAY_TELEPORT_ENTITY {
        // Teleport Entity teilt das Layout bis einschliesslich Pitch mit Position Sync
        // (s. packets.rs); die Relativ-Flags am Ende werden vorerst nicht ausgewertet.
        match packets::read_entity_position_sync(payload) {
            Ok((eid, x, y, z, yaw, pitch)) => {
                let teleport = id == packets::CB_PLAY_TELEPORT_ENTITY;
                let _ = events.send(ClientEvent::EntityPos { id: eid, x, y, z, yaw_deg: yaw, pitch_deg: pitch, teleport });
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "EntityPositionSync"),
        }
    } else if id == packets::CB_PLAY_CUSTOM_PAYLOAD {
        if let Ok((channel, data)) = packets::read_custom_payload(payload) {
            let _ = events.send(ClientEvent::CustomPayload { channel, data });
        }
    } else if let Some(update) = packets::read_border_update(payload) {
        if let Ok(update) = update {
            let _ = events.send(ClientEvent::Border(update));
        }
    } else if id == packets::CB_PLAY_SET_CAMERA {
        if let Ok(entity) = packets::read_set_camera(payload) {
            let _ = events.send(ClientEvent::SetCamera(entity));
        }
    } else if id == packets::CB_PLAY_PLAYER_ABILITIES {
        if let Ok(abilities) = packets::read_player_abilities(payload) {
            let _ = events.send(ClientEvent::Abilities(abilities));
        }
    } else if id == packets::CB_PLAY_COOLDOWN {
        if let Ok((group, ticks)) = packets::read_cooldown(payload) {
            let _ = events.send(ClientEvent::ItemCooldown { group, ticks });
        }
    } else if id == packets::CB_PLAY_BOSS_EVENT {
        if let Ok((uuid, event)) = packets::read_boss_event(payload) {
            let _ = events.send(ClientEvent::BossEvent { uuid, event });
        }
    } else if id == packets::CB_PLAY_MOVE_VEHICLE {
        if let Ok((pos, yaw_deg, pitch_deg)) = packets::read_move_vehicle(payload) {
            let _ = events.send(ClientEvent::VehicleMove { pos, yaw_deg, pitch_deg });
        }
    } else if id == packets::CB_PLAY_SET_PASSENGERS {
        if let Ok((vehicle, passengers)) = packets::read_set_passengers(payload) {
            let _ = events.send(ClientEvent::Passengers { vehicle, passengers });
        }
    } else if id == packets::CB_PLAY_REMOVE_ENTITIES {
        match packets::read_remove_entities(payload) {
            Ok(ids) => {
                let _ = events.send(ClientEvent::EntitiesRemove(ids));
            }
            Err(_) => log_entity_parse_error(events, &mut stats.entity_errors, "RemoveEntities"),
        }
    } else if id == packets::CB_PLAY_BLOCK_UPDATE {
        if let Ok((x, y, z, state)) = packets::read_block_update(payload) {
            let _ = events.send(ClientEvent::BlockUpdate(vec![(x, y, z, state)]));
        }
    } else if id == packets::CB_PLAY_SECTION_BLOCKS_UPDATE {
        if let Ok(blocks) = packets::read_section_blocks_update(payload) {
            let _ = events.send(ClientEvent::BlockUpdate(blocks));
        }
    } else if id == packets::CB_PLAY_SET_ENTITY_MOTION {
        if let Ok((entity_id, vel)) = packets::read_entity_velocity(payload) {
            let _ = events.send(ClientEvent::EntityVelocity(entity_id, vel));
        }
    } else if id == packets::CB_PLAY_EXPLODE {
        // Ein Lesefehler darf hier NICHT die Verbindung abbrechen -- und der Rueckstoss wird
        // getrennt gelesen, damit ein unlesbarer Partikel dahinter ihn nicht mitreisst.
        if let Ok(Some(kb)) = packets::read_explosion_knockback(payload) {
            let _ = events.send(ClientEvent::ExplosionKnockback(kb));
        }
        if let Ok(explosion) = protocol::particle::read_explosion(payload) {
            let _ = events.send(ClientEvent::ExplosionEffects(explosion));
        }
    } else if id == protocol::particle::CB_PLAY_LEVEL_PARTICLES {
        if let Ok(p) = protocol::particle::read_level_particles(payload) {
            let _ = events.send(ClientEvent::Particles(p));
        }
    } else if id == protocol::particle::CB_PLAY_LEVEL_EVENT {
        if let Ok(e) = protocol::particle::read_level_event(payload) {
            let _ = events.send(ClientEvent::LevelEvent(e));
        }
    } else if id == packets::CB_PLAY_SET_TIME {
        if let Ok((_world_age, time_of_day, ticking)) = packets::read_world_time(payload) {
            let _ = events.send(ClientEvent::WorldTime { time_of_day, ticking });
        }
    } else if id == packets::CB_PLAY_UPDATE_MOB_EFFECT {
        if let Ok((entity_id, effect, amplifier, duration, flags)) = packets::read_mob_effect(payload) {
            let _ = events.send(ClientEvent::MobEffect { entity_id, effect, amplifier, duration, flags });
        }
    } else if id == packets::CB_PLAY_REMOVE_MOB_EFFECT {
        if let Ok((entity_id, effect)) = packets::read_remove_mob_effect(payload) {
            let _ = events.send(ClientEvent::MobEffectRemoved { entity_id, effect });
        }
    } else if id == packets::CB_PLAY_BLOCK_EVENT {
        if let Ok((pos, typ, daten)) = packets::read_block_event(payload) {
            let _ = events.send(ClientEvent::BlockEvent(pos, typ, daten));
        }
    } else if id == packets::CB_PLAY_BLOCK_CHANGED_ACK {
        if let Ok(sequence) = packets::read_block_changed_ack(payload) {
            let _ = events.send(ClientEvent::BlockChangedAck(sequence));
        }
    } else if id == packets::CB_PLAY_SET_HEALTH {
        if let Ok(health) = packets::read_set_health(payload) {
            let _ = events.send(ClientEvent::Health(health.0, health.1, health.2));
        }
    } else if id == packets::CB_PLAY_DISCONNECT {
        return Err(NetError::Protocol(disconnect_message("Play", payload)));
    } else if id == packets::CB_PLAY_SYSTEM_CHAT {
        if let Ok((spans, overlay)) = packets::read_system_chat_spans(payload) {
            let _ = events.send(ClientEvent::Chat(spans, overlay));
        }
        if let Ok(Some((victim, killer))) = packets::read_system_chat_death(payload) {
            let _ = events.send(ClientEvent::DeathMessage { victim, killer });
        }
    } else if id == packets::CB_PLAY_SET_ACTION_BAR_TEXT {
        if let Ok(spans) = packets::read_action_bar_spans(payload) {
            let _ = events.send(ClientEvent::ActionBar(spans));
        }
    } else if id == packets::CB_PLAY_SET_TITLE_TEXT {
        if let Ok(text) = packets::read_title_spans(payload) {
            let _ = events.send(ClientEvent::Title(text));
        }
    } else if id == packets::CB_PLAY_SET_SUBTITLE_TEXT {
        if let Ok(text) = packets::read_subtitle_spans(payload) {
            let _ = events.send(ClientEvent::Subtitle(text));
        }
    } else if id == packets::CB_PLAY_SET_TITLES_ANIMATION {
        if let Ok((fade_in, stay, fade_out)) = packets::read_title_fade(payload) {
            let _ = events.send(ClientEvent::TitleFade { fade_in, stay, fade_out });
        }
    } else if id == packets::CB_PLAY_CLEAR_TITLES {
        // bool reset ignorieren -- wir blenden ohnehin aus.
        let _ = events.send(ClientEvent::ClearTitles);
    } else if id == packets::CB_PLAY_SET_DISPLAY_OBJECTIVE {
        if let Ok((slot, objective)) = packets::read_display_objective(payload) {
            let _ = events.send(ClientEvent::ScoreboardDisplay { slot, objective });
        }
    } else if id == packets::CB_PLAY_SET_OBJECTIVE {
        if let Ok(o) = packets::read_objective(payload) {
            let _ = events.send(ClientEvent::ScoreboardObjective {
                name: o.name,
                mode: o.mode,
                display: o.display,
                hearts: o.hearts,
                number_format: o.number_format,
            });
        }
    } else if id == packets::CB_PLAY_SET_SCORE {
        if let Ok(s) = packets::read_score(payload) {
            let _ = events.send(ClientEvent::ScoreboardScore {
                holder: s.holder,
                objective: s.objective,
                score: s.score,
                display: s.display,
                number_format: s.number_format,
            });
        }
    } else if id == packets::CB_PLAY_SET_PLAYER_TEAM {
        if let Ok(t) = packets::read_team(payload) {
            let _ = events.send(ClientEvent::Team {
                name: t.name,
                mode: t.mode,
                prefix: t.prefix,
                suffix: t.suffix,
                name_tag_visibility: t.name_tag_visibility,
                color: t.color,
                collision_rule: t.collision_rule,
                entries: t.entries,
                display_name: t.display_name,
            });
        }
    } else if id == packets::CB_PLAY_RESET_SCORE {
        if let Ok((holder, objective)) = packets::read_reset_score(payload) {
            let _ = events.send(ClientEvent::ScoreboardResetScore { holder, objective });
        }
    } else if id == packets::CB_PLAY_CONTAINER_SET_CONTENT {
        if let Ok(content) = packets::read_container_content(payload) {
            if let Some((index, err)) = &content.truncated_at {
                // Ein Component-Typ, den item.rs noch nicht byteweise kennt: ab hier ist die
                // Slot-Liste unlesbar. Einmal ins Log, damit genau dieser Typ nachgeruestet
                // werden kann statt still einen halbleeren Container zu zeigen.
                log(events, format!("Inventar: Slot {index} unlesbar ({err:?}) -- Rest der Liste verworfen"));
                if let Some((wo, bytes)) = &content.truncated_detail {
                    log(events, format!("Inventar: {wo}"));
                    dump_unreadable_slot(content.sync_id, *index, wo, bytes);
                }
            }
            let _ = events.send(ClientEvent::ContainerContent(content));
        }
    } else if id == packets::CB_PLAY_CONTAINER_SET_SLOT {
        match packets::read_container_set_slot(payload) {
            Ok(update) => {
                let _ = events.send(ClientEvent::ContainerSlot(update));
            }
            Err(_) => report_unreadable_single_slot(events, "Container-Slot", payload),
        }
    } else if id == packets::CB_PLAY_ROTATE_HEAD {
        if let Ok((eid, head_yaw_deg)) = packets::read_rotate_head(payload) {
            let _ = events.send(ClientEvent::EntityHeadYaw { id: eid, head_yaw_deg });
        }
    } else if id == packets::CB_PLAY_COMMAND_SUGGESTIONS {
        if let Some(suggestions) = packets::read_command_suggestions(payload) {
            let _ = events.send(ClientEvent::CommandSuggestions(suggestions));
        }
    } else if id == packets::CB_PLAY_ENTITY_EVENT {
        if let Ok((entity_id, status)) = packets::read_entity_event(payload) {
            let _ = events.send(ClientEvent::EntityEvent { entity_id, status });
        }
    } else if id == packets::CB_PLAY_SOUND || id == packets::CB_PLAY_SOUND_ENTITY {
        // Beide Ton-Pakete an EINER Stelle: sie unterscheiden sich nur darin, ob der Ort als
        // Koordinate oder als Entity-ID kommt (s. packets::SoundEffect). Wo die Entity steht,
        // weiss der Netz-Task nicht -- das loest die App auf, die den Weltzustand haelt.
        //
        // Ok(None) heisst "gelesen, aber unbekannter Klang" (z.B. neuere Serverversion) und
        // ist bewusst still: das Paket war vollstaendig, es gibt nur nichts abzuspielen.
        let gelesen = if id == packets::CB_PLAY_SOUND { packets::read_sound(payload) } else { packets::read_sound_entity(payload) };
        if sound_debug() {
            match &gelesen {
                Ok(Some(s)) => eprintln!(
                    "[ton] 0x{id:02x} {} kat={:?} pos={:?} ent={:?} vol={} pitch={}",
                    s.name, s.category, s.position, s.entity_id, s.volume, s.pitch
                ),
                Ok(None) => eprintln!("[ton] 0x{id:02x} gelesen, aber Klang oder Kategorie unbekannt"),
                Err(e) => eprintln!("[ton] 0x{id:02x} NICHT LESBAR: {e:?} ({} Byte)", payload.len()),
            }
        }
        if let Ok(Some(sound)) = gelesen {
            let _ = events.send(ClientEvent::Sound(sound));
        }
    } else if id == packets::CB_PLAY_STOP_SOUND {
        if let Ok((category, name)) = packets::read_stop_sound(payload) {
            let _ = events.send(ClientEvent::StopSound { category, name });
        }
    } else if id == packets::CB_PLAY_ANIMATE {
        if let Ok((entity_id, animation)) = packets::read_entity_animation(payload) {
            let _ = events.send(ClientEvent::EntityAnimation { entity_id, animation });
        }
    } else if id == packets::CB_PLAY_DAMAGE_EVENT {
        if let Ok((entity_id, damage_type)) = packets::read_damage_event(payload) {
            let _ = events.send(ClientEvent::EntityDamaged { entity_id, damage_type });
        }
    } else if id == packets::CB_PLAY_SET_EQUIPMENT {
        // Ein unbekannter Component-Typ im Stapel macht den REST des Pakets unlesbar (die
        // Slot-Kette laeuft byteweise weiter) -- dann bleibt die Hand eben leer, statt die
        // Verbindung abzubrechen. Genau wie beim Inventar (s. CB_PLAY_CONTAINER_SET_CONTENT).
        if let Ok((entity_id, entries)) = packets::read_set_equipment(payload) {
            let _ = events.send(ClientEvent::EntityEquipment { id: entity_id, entries });
        }
    } else if id == packets::CB_PLAY_SET_EXPERIENCE {
        if let Ok((progress, level, total)) = packets::read_experience(payload) {
            let _ = events.send(ClientEvent::Experience { progress, level, total });
        }
    } else if id == packets::CB_PLAY_UPDATE_ATTRIBUTES {
        return on_update_attributes(events, payload);
    } else if id == packets::CB_PLAY_SET_ENTITY_DATA {
        // Ausgewertet werden Pose (Hoehe der Trefferbox), Schlafposition, Restluft und --
        // gebuendelt als EntityMeta -- Sichtbarkeit, Namensschild und Display-Inhalt. Was
        // nicht im Paket steht, hat sich nicht geaendert: der Server schickt nur geaenderte
        // Felder.
        if let Ok(update) = packets::read_set_entity_data(payload) {
            if let Some(pose) = update.pose {
                let _ = events.send(ClientEvent::EntityPose { id: update.entity_id, pose });
            }
            if let Some(pos) = update.sleeping_pos {
                let _ = events.send(ClientEvent::EntitySleeping { id: update.entity_id, pos });
            }
            if let Some(air) = update.air {
                let _ = events.send(ClientEvent::EntityAir { id: update.entity_id, air });
            }
            // Nur senden, wenn ueberhaupt etwas Einschlaegiges drinstand -- Positions-/
            // Gesundheits-Updates schicken ihren Metadaten-Strom sonst pro Tick durch die
            // Warteschlange, ohne dass sich daran je etwas aendert.
            let betrifft_anzeige = meta_betrifft_anzeige(&update);
            if betrifft_anzeige {
                let _ = events.send(ClientEvent::EntityMeta(Box::new(update)));
            }
        }
    } else if id == packets::CB_PLAY_PLAYER_INFO_UPDATE {
        // Traegt (bei ADD_PLAYER) Name und Textur-Property jedes Spielers -- die Grundlage
        // fuer echte Skins. Nicht lesbar heisst hier: still verwerfen, das Rendern faellt
        // dann auf den Standard-Skin zurueck.
        if let Some(entries) = packets::parse_player_info_update(payload) {
            let _ = events.send(ClientEvent::PlayerInfo(entries));
        }
    } else if id == packets::CB_PLAY_PLAYER_INFO_REMOVE {
        if let Some(uuids) = packets::parse_player_info_remove(payload) {
            let _ = events.send(ClientEvent::PlayerInfoRemove(uuids));
        }
    } else if id == packets::CB_PLAY_TAB_LIST {
        if let Ok((header, footer)) = packets::read_tab_list(payload) {
            let _ = events.send(ClientEvent::TabList { header, footer });
        }
    } else if id == packets::CB_PLAY_SET_CURSOR_ITEM {
        if let Ok(stack) = packets::read_cursor_item(payload) {
            let _ = events.send(ClientEvent::CursorItem(stack));
        }
    } else if id == packets::CB_PLAY_SET_PLAYER_INVENTORY {
        match packets::read_player_inventory_slot(payload) {
            Ok((slot, stack)) => {
                let _ = events.send(ClientEvent::PlayerInventorySlot(slot, stack));
            }
            Err(_) => report_unreadable_single_slot(events, "Spieler-Slot", payload),
        }
    } else if id == packets::CB_PLAY_SET_CARRIED_ITEM {
        if let Ok(slot) = packets::read_set_carried_item(payload) {
            let _ = events.send(ClientEvent::HeldSlot(slot));
        }
    } else if id == protocol::sign::CB_PLAY_BLOCK_ENTITY_DATA {
        if let Ok(Some((pos, text))) = protocol::sign::read_block_entity_data(payload) {
            let _ = events.send(ClientEvent::SignText { pos, text });
        }
        if let Ok(Some((pos, profile))) = protocol::sign::read_block_entity_head(payload) {
            let _ = events.send(ClientEvent::HeadAt { pos, profile });
        }
    } else if id == protocol::sign::CB_PLAY_OPEN_SIGN_EDITOR {
        if let Ok((pos, front)) = protocol::sign::read_open_sign_editor(payload) {
            let _ = events.send(ClientEvent::OpenSignEditor { pos, front });
        }
    } else if id == packets::CB_PLAY_MAP_ITEM_DATA {
        if let Ok(daten) = packets::read_map_item_data(payload) {
            let _ = events.send(ClientEvent::MapData(daten));
        }
    } else if id == packets::CB_PLAY_OPEN_BOOK {
        if let Ok(hand) = packets::read_open_book(payload) {
            let _ = events.send(ClientEvent::OpenBook { hand });
        }
    } else if id == protocol::recipe::CB_PLAY_RECIPE_BOOK_ADD {
        match protocol::recipe::read_recipe_book_add(payload) {
            Ok(add) => {
                let _ = events.send(ClientEvent::RecipeBookAdd(add));
            }
            Err(_) => log(events, "Rezeptbuch-Paket nicht lesbar -- ignoriert"),
        }
    } else if id == protocol::recipe::CB_PLAY_RECIPE_BOOK_REMOVE {
        if let Ok(ids) = protocol::recipe::read_recipe_book_remove(payload) {
            let _ = events.send(ClientEvent::RecipeBookRemove(ids));
        }
    } else if id == protocol::recipe::CB_PLAY_UPDATE_TAGS {
        if let Ok(tags) = protocol::recipe::read_item_tags(payload) {
            let _ = events.send(ClientEvent::ItemTags(tags));
        }
        if let Ok(tags) = protocol::recipe::read_block_tags(payload) {
            let _ = events.send(ClientEvent::BlockTags(tags));
        }
    } else if id == protocol::recipe::CB_PLAY_RECIPE_BOOK_SETTINGS || id == protocol::recipe::CB_PLAY_PLACE_GHOST_RECIPE {
        // Buch-Einstellungen des Servers und Geister-Rezepte: das Buch hier hat eigene.
    } else if id == packets::CB_PLAY_OPEN_SCREEN {
        if let Ok(screen) = packets::read_open_screen(payload) {
            let _ = events.send(ClientEvent::OpenScreen(screen));
        }
    } else if id == packets::CB_PLAY_CONTAINER_SET_DATA {
        if let Ok((sync_id, index, value)) = packets::read_container_set_data(payload) {
            let _ = events.send(ClientEvent::ContainerData(sync_id, index, value));
        }
    } else if id == packets::CB_PLAY_MERCHANT_OFFERS {
        if let Ok(offers) = packets::read_merchant_offers(payload) {
            let _ = events.send(ClientEvent::MerchantOffers(offers));
        }
    } else if id == packets::CB_PLAY_CONTAINER_CLOSE {
        if let Ok(sync_id) = packets::read_container_close(payload) {
            let _ = events.send(ClientEvent::CloseScreen(sync_id));
        }
    } else if stats.note_unknown_id(id) && std::env::var_os("CORAL_PACKET_SCAN").is_some() {
        // Diagnose: Play-Packet-IDs sind versionsabhaengig und die Wiki dokumentiert
        // 776, nicht 774. Statt zu raten, jede unbekannte ID einmal daraufhin testen,
        // ob sich dahinter eine plausible Spielerposition verbirgt (VarInt + 3 Doubles).
        if let Ok(packets::PlayerPositionSync { pos: (x, y, z), .. }) = packets::read_player_position(payload) {
            let plausible =
                (-30_000_000.0..30_000_000.0).contains(&x) && (-1000.0..1000.0).contains(&y) && (-30_000_000.0..30_000_000.0).contains(&z);
            if plausible {
                log(events, format!("[scan] id=0x{id:02x} ({} Byte) -> {x:.1} / {y:.1} / {z:.1}", payload.len()));
            }
        }
    }
    Ok(PlayFlow::Continue)
}

async fn on_player_position(conn: &mut Connection, events: &Events, payload: &[u8], rotation: &SharedRotation) -> Result<PlayFlow, NetError> {
    let tp = packets::read_teleport_id(payload).map_err(|_| NetError::Protocol("TeleportId".into()))?;
    // Ein Teleport ist fuer GrimAC eine Sonder-Tick-Grenze: das Bewegungspaket, das ihn
    // annimmt, setzt seine Per-Tick-Merker NICHT zurueck (Check.isTickPacket ->
    // !lastPacketWasTeleport). Zwei unserer Ticks fallen dann fuer ihn in einen zusammen.
    interaktions_log("teleport(server)");
    conn.reply(events, &packets::confirm_teleport_payload(tp)).await?;

    // Position erst nach dem Bestaetigen melden: die Teleport-Bestaetigung ist
    // Pflicht, ein Parse-Fehler in der Position darf sie nicht verhindern.
    if let Ok(sync) = packets::read_player_position(payload) {
        let (x, y, z) = sync.pos;
        let (yaw, pitch, flags) = (sync.yaw, sync.pitch, sync.flags);
        log(events, format!("Spawn-Position: {x:.1} / {y:.1} / {z:.1}"));

        // ECHTER BUG gewesen (BadPacketsN-Endlosschleife -> Dauer-Setback
        // zum Spawn, spaeter zusaetzlich Timer/duplicate-look). Grims Teleport-
        // Warteschlange (SetbackTeleportUtil.checkTeleportQueue) akzeptiert einen Server-
        // Teleport NUR, wenn das naechste Bewegungspaket die Zielposition bei absoluten
        // Teleports EXAKT meldet (Schwelle 0.0) UND es GENAU im Transaktionsfenster
        // ankommt: lastTransactionReceived == teleport.transaction. Grim sendet vor UND
        // nach dem Teleport je eine Transaktion (clientbound Ping 0x3B); ist unser
        // Pong fuer die NACH-Transaktion schon raus, bevor unser Bewegungspaket ankommt,
        // gilt lastTransactionReceived > teleport.transaction -> BadPacketsN + neuer
        // Setback (im Proxy-Log als Endlosschleife sichtbar).
        //
        // Genau DAS passierte, weil die Pongs (0x2C) hier synchron im Netz-Thread
        // rausgehen, das Bewegungspaket aber erst einen Frame spaeter im App-Thread --
        // also immer NACH den Pongs. Vanilla (ClientPlayNetworkHandler.onPlayerPosition-
        // Look) sendet die Bestaetigungs-Bewegung dagegen in-order direkt nach dem
        // TeleportConfirm, vor dem naechsten Pong. Hier genau nachgebaut: sofort, im selben
        // Netz-Thread.
        //
        // ZWINGEND ein Full-Paket (Position UND Rotation), KEIN reines Positions-Paket:
        // Grim ruft die Teleport-Warteschlangen-Pruefung (SetbackTeleportUtil.
        // checkTeleportQueue, die den Teleport ueberhaupt erst als akzeptiert markiert) NUR
        // fuer Pakete auf, die BEIDES aendern -- CheckManagerListener:
        //   flying.hasPositionChanged() && flying.hasRotationChanged() ? checkTeleportQueue(..) : ..
        // Ein reines Positions-Paket wird dort gar nicht geprueft, der Teleport bliebe ewig
        // unbestaetigt -> BadPacketsN-Endlosschleife (genau der zuvor beobachtete Bug, als
        // hier faelschlich move_player_pos_payload stand). Vanilla sendet an dieser Stelle
        // ebenfalls ein Full (onGround=false). Rotation kommt aus dem von der App laufend
        // gepflegten SharedRotation-Wert; bei relativer Rotation (Setbacks, Flag gesetzt)
        // ist das exakt die beizubehaltende aktuelle Blickrichtung, bei absoluter (Spawn)
        // nehmen wir die Rotation aus dem Teleport selbst. Nur fuer ABSOLUTE Position (kein
        // REL_X/Y/Z): bei relativer Position fehlt uns hier die alte -- selten und nicht die
        // Setback-/Spawn-Faelle, die die Schleife ausloesen.
        let pos_absolute = flags & (packets::POSITION_FLAG_REL_X | packets::POSITION_FLAG_REL_Y | packets::POSITION_FLAG_REL_Z) == 0;
        if pos_absolute {
            let (cur_yaw, cur_pitch) = rotation.get();
            // Sitzend nimmt Vanilla die Drehung des Pakets nicht an und meldet die eigene
            // (s. SharedLook) -- sonst stellte der Server unseren Kopf auf seinen alten
            // Stand zurueck, bei jedem Teleport des Fahrzeugs.
            let sitzt = rotation.riding();
            let send_yaw = if sitzt { cur_yaw } else if flags & packets::POSITION_FLAG_REL_YAW != 0 { cur_yaw + yaw } else { yaw };
            let send_pitch = if sitzt { cur_pitch } else if flags & packets::POSITION_FLAG_REL_PITCH != 0 { cur_pitch + pitch } else { pitch };
            conn.reply(events, &packets::move_player_pos_rot_payload(x, y, z, send_yaw, send_pitch, false, false)).await?;
        }

        let _ = events.send(ClientEvent::PlayerPosition(sync));
    }
    Ok(PlayFlow::Continue)
}

fn on_update_attributes(events: &Events, payload: &[u8]) -> Result<PlayFlow, NetError> {
    if let Ok(update) = packets::read_attributes(payload) {
        if update.armor.is_some() || update.max_health.is_some() {
            let _ = events.send(ClientEvent::Attributes { id: update.entity_id, armor: update.armor, max_health: update.max_health });
        }
        if update.movement_speed.is_some() || update.sneaking_speed.is_some() || update.water_movement_efficiency.is_some() {
            let _ = events.send(ClientEvent::MovementAttributes {
                id: update.entity_id,
                movement_speed: update.movement_speed,
                sneaking_speed: update.sneaking_speed,
                water_movement_efficiency: update.water_movement_efficiency,
            });
        }
        if let Some(parts) = update.attack_speed {
            let _ = events.send(ClientEvent::AttackSpeed { id: update.entity_id, parts });
        }
        if update.gravity.is_some()
            || update.jump_strength.is_some()
            || update.movement_efficiency.is_some()
            || update.scale.is_some()
            || update.step_height.is_some()
        {
            let _ = events.send(ClientEvent::BodyAttributes {
                id: update.entity_id,
                gravity: update.gravity,
                jump_strength: update.jump_strength,
                movement_efficiency: update.movement_efficiency,
                scale: update.scale,
                step_height: update.step_height,
            });
        }
        if update.block_interaction_range.is_some() || update.entity_interaction_range.is_some() {
            let _ = events.send(ClientEvent::RangeAttributes {
                id: update.entity_id,
                block: update.block_interaction_range,
                entity: update.entity_interaction_range,
            });
        }
        if update.block_break_speed.is_some() || update.mining_efficiency.is_some() || update.submerged_mining_speed.is_some() {
            let _ = events.send(ClientEvent::MiningAttributes {
                id: update.entity_id,
                block_break_speed: update.block_break_speed,
                mining_efficiency: update.mining_efficiency,
                submerged_mining_speed: update.submerged_mining_speed,
            });
        }
    }
    Ok(PlayFlow::Continue)
}

fn on_chunk_data(events: &Events, payload: &[u8], stats: &mut PlayStats) -> Result<PlayFlow, NetError> {
    // Ein Parse-Fehler heisst hier: Format falsch verstanden (die Sections gingen
    // nicht exakt im Data-Feld auf). Nicht die Verbindung abbrechen, aber zaehlen
    // und melden -- so sehen wir sofort, ob die Dekodierung wirklich stimmt.
    if let Ok(chunk) = protocol::chunk::parse_chunk_data(payload) {
        if stats.chunks_ok == 0 {
            let non_empty = chunk.sections.iter().filter(|s| !s.is_empty()).count();
            log(
                events,
                format!(
                    "erster Chunk dekodiert: ({}, {}), {} Sections, {} nicht leer",
                    chunk.x,
                    chunk.z,
                    chunk.sections.len(),
                    non_empty
                ),
            );
        }
        stats.chunks_ok += 1;
        if chunk.tail_unreadable && !stats.chunk_tail_logged {
            stats.chunk_tail_logged = true;
            log(events, format!("Chunk {} {}: Muell hinter den Sections, uebergangen (wie Vanilla)", chunk.x, chunk.z));
        }
        if stats.chunks_ok.is_multiple_of(50) {
            log(events, format!("Chunks dekodiert: {} ok, {} Fehler", stats.chunks_ok, stats.chunks_failed));
        }
        let _ = events.send(ClientEvent::Chunk(chunk));
    } else {
        stats.chunks_failed += 1;
        // Die Koordinaten stehen hinter der Paket-ID (ein Byte, 0x2C).
        if let (Some(x), Some(z)) = (payload.get(1..5), payload.get(5..9)) {
            let (x, z) = (i32::from_be_bytes(x.try_into().unwrap()), i32::from_be_bytes(z.try_into().unwrap()));
            let _ = events.send(ClientEvent::ChunkUndecodable(x, z));
        }
        if stats.chunks_failed == 1 {
            log(
                events,
                format!("FEHLER: Chunk nicht dekodierbar. Layout-Analyse:\n{}", protocol::chunk::debug_chunk_layout(payload)),
            );
        }
    }
    Ok(PlayFlow::Continue)
}

async fn on_player_rotation(conn: &mut Connection, events: &Events, payload: &[u8], rotation: &SharedRotation) -> Result<PlayFlow, NetError> {
    // Der Server dreht unsere Kamera, ohne uns zu bewegen -- eigenes Paket seit 1.21.2.
    // Legitim benutzt von Plugins fuer Zwischensequenzen, "schau hierhin"-Effekte,
    // Minispiel-Kameras und Aufzuege/Fahrstuehle. Vanilla folgt dem kommentarlos.
    //
    // nach ClientPlayNetworkHandler.onPlayerRotation (1.21.11, nachgesehen):
    //   1. Relativ-Flags je Achse EINZELN aufloesen (PositionFlag.ofRot),
    //   2. Yaw/Pitch setzen, updateLastAngles() -- also ohne Nachziehen/Interpolation,
    //   3. SOFORT ein LookAndOnGround(yaw, pitch, false, false) zuruecksenden.
    //
    // Schritt 3 ist keine Kosmetik, sondern die Bestaetigung: der Server erfaehrt nur so,
    // dass wir die Drehung uebernommen haben. Genau wie bei der Teleport-Bestaetigung
    // muss sie IM NETZ-TASK rausgehen und nicht erst einen Frame spaeter aus dem
    // App-Thread -- sonst liegt sie hinter dem naechsten Pong, und fuer eine
    // transaktionsbasierte Pruefung ist die Antwort damit im falschen Fenster (derselbe
    // Fehler, der beim Teleport die BadPacketsN-Endlosschleife ausgeloest hat).
    //
    // Vanilla schickt hier fest onGround=false und horizontalCollision=false -- nicht
    // unseren echten Zustand. Bewusst uebernommen: eine Abweichung davon waere genau die
    // Art Detail, an der eine serverseitige Pruefung uns von einem echten Client
    // unterscheidet.
    if let Ok((yaw, relative_yaw, pitch, relative_pitch)) = packets::read_player_rotation(payload) {
        let (cur_yaw, cur_pitch) = rotation.get();
        let neu_yaw = if relative_yaw { cur_yaw + yaw } else { yaw };
        let neu_pitch = if relative_pitch { cur_pitch + pitch } else { pitch };
        interaktions_log("server_rotation");
        conn.reply(events, &packets::move_player_rot_payload(neu_yaw, neu_pitch, false, false)).await?;
        // Erst NACH dem Senden in den geteilten Wert schreiben: kaeme die App vorher dazu,
        // ein eigenes Bewegungspaket zu schicken, stuende dort schon die neue Rotation,
        // bevor der Server sie bestaetigt bekommen hat.
        rotation.set(neu_yaw, neu_pitch);
        let _ = events.send(ClientEvent::ServerRotation { yaw_deg: neu_yaw, pitch_deg: neu_pitch });
    }
    Ok(PlayFlow::Continue)
}

fn on_game_event(events: &Events, payload: &[u8]) -> Result<PlayFlow, NetError> {
    if let Ok((reason, value)) = packets::read_game_event(payload) {
        match reason {
            packets::GAME_EVENT_GAME_MODE_CHANGED => {
                let _ = events.send(ClientEvent::GameMode(value as u8));
            }
            packets::GAME_EVENT_RAIN_STARTED => {
                let _ = events.send(ClientEvent::RainGradient(0.0));
            }
            packets::GAME_EVENT_RAIN_STOPPED => {
                let _ = events.send(ClientEvent::RainGradient(1.0));
            }
            packets::GAME_EVENT_RAIN_GRADIENT => {
                let _ = events.send(ClientEvent::RainGradient(value.clamp(0.0, 1.0)));
            }
            packets::GAME_EVENT_THUNDER_GRADIENT => {
                let _ = events.send(ClientEvent::ThunderGradient(value.clamp(0.0, 1.0)));
            }
            packets::GAME_EVENT_INITIAL_CHUNKS_COMING => {
                let _ = events.send(ClientEvent::InitialChunksComing);
            }
            packets::GAME_EVENT_PROJECTILE_HIT_PLAYER => {
                let _ = events.send(ClientEvent::ProjectileHitPlayer);
            }
            _ => {}
        }
    }
    Ok(PlayFlow::Continue)
}
