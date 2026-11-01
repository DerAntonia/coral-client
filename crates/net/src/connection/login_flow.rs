// Transfer, Login und Konfiguration.

use super::*;

// Ziel eines Transfer-Pakets: Host + Port, so wie der Server sie schickt.
pub(super) type TransferTarget = (String, u16);

// Configuration-State: Client Information senden, Known Packs beantworten, Keep Alive/Ping
// bedienen, bis Finish Configuration kommt -> in Play-State wechseln (None). Some = der Server
// hat uns stattdessen weitergeschickt (Transfer).
pub(super) async fn do_configuration(conn: &mut Connection, events: &Events, outgoing: &mut Outgoing) -> Result<Option<TransferTarget>, NetError> {
    conn.send(&packets::client_information_payload()).await?;
    // Direkt danach der Brand -- genau die Reihenfolge, die Vanilla verwendet.
    conn.send(&packets::brand_payload(packets::CLIENT_BRAND)).await?;

    // Jede unbehandelte Config-Packet-ID EINMAL protokollieren -- dieselbe Technik wie im
    // Play-Loop. Ohne das ist ein Disconnect im Configuration-State eine Blackbox: man sieht
    // nur, dass es vorbei ist, nicht welche Anforderung des Servers wir uebergangen haben
    // (Resourcepack, Cookie, Transfer, ...).
    let mut seen_unhandled: Vec<i32> = Vec::new();
    let mut outgoing_closed = false;

    loop {
        // Auch hier den Sendekanal bedienen: Server, die ihr Resourcepack in der Konfiguration
        // schicken, warten oft auf "geladen", bevor sie die Konfiguration beenden. Die App
        // schreibt Status-Antworten mit der Play-ID; alles andere waere hier ein falsches
        // Paket und faellt weg (die App sendet ausserhalb des Spiels sonst nichts).
        let payload = tokio::select! {
            biased;
            msg = outgoing.recv(), if !outgoing_closed => {
                match msg {
                    Some(p) => {
                        if let Some(config) = packets::config_outgoing_payload(&p) {
                            conn.send(&config).await?;
                        }
                    }
                    None => outgoing_closed = true,
                }
                continue;
            }
            result = conn.recv() => result?,
        };
        let id = packets::peek_packet_id(&payload).map_err(|_| NetError::Protocol("leeres Config-Packet".into()))?;
        match id {
            packets::CB_CONFIG_COOKIE_REQUEST => {
                answer_cookie_request(conn, &payload, packets::SB_CONFIG_COOKIE_RESPONSE).await?;
            }
            packets::CB_CONFIG_STORE_COOKIE => store_cookie(conn, &payload),
            packets::CB_CONFIG_RESOURCE_PACK_PUSH => match packets::read_resource_pack_push(&payload) {
                Ok(push) => {
                    let _ = events.send(ClientEvent::ResourcePackPush(push));
                }
                Err(_) => log(events, "Resourcepack-Anfrage nicht lesbar -- ignoriert"),
            },
            packets::CB_CONFIG_RESOURCE_PACK_POP => {
                if let Ok(id) = packets::read_resource_pack_pop(&payload) {
                    let _ = events.send(ClientEvent::ResourcePackPop(id));
                }
            }
            packets::CB_CONFIG_KNOWN_PACKS => {
                conn.send(&packets::serverbound_known_packs_empty_payload()).await?;
            }
            packets::CB_CONFIG_PLUGIN_MESSAGE => {
                if let Ok((channel, data)) = packets::read_custom_payload(&payload) {
                    let _ = events.send(ClientEvent::CustomPayload { channel, data });
                }
            }
            packets::CB_CONFIG_KEEP_ALIVE => {
                let ka = packets::read_keep_alive_id(&payload).map_err(|_| NetError::Protocol("KeepAlive".into()))?;
                conn.send(&packets::keep_alive_config_payload(ka)).await?;
            }
            packets::CB_CONFIG_PING => {
                let ping = packets::read_ping_id(&payload).map_err(|_| NetError::Protocol("Ping".into()))?;
                conn.send(&packets::pong_config_payload(ping)).await?;
            }
            packets::CB_CONFIG_DISCONNECT => {
                return Err(NetError::Protocol(disconnect_message("Configuration", &payload)));
            }
            packets::CB_CONFIG_FINISH => {
                conn.send(&packets::acknowledge_finish_configuration_payload()).await?;
                return Ok(None); // -> Play-State
            }
            packets::CB_CONFIG_TRANSFER => {
                let ziel = packets::read_transfer(&payload).map_err(|_| NetError::Protocol("Transfer-Paket nicht lesbar".into()))?;
                return Ok(Some(ziel));
            }
            protocol::recipe::CB_CONFIG_UPDATE_TAGS => {
                if let Ok(tags) = protocol::recipe::read_item_tags(&payload) {
                    let _ = events.send(ClientEvent::ItemTags(tags));
                }
                if let Ok(tags) = protocol::recipe::read_block_tags(&payload) {
                    let _ = events.send(ClientEvent::BlockTags(tags));
                }
            }
            packets::CB_CONFIG_RESET_CHAT => {
                let _ = events.send(ClientEvent::ResetChat);
            }
            // Schaltet in Vanilla experimentelle Inhalte frei; dieser Client kennt ohnehin
            // nur die festen Item- und Blocklisten, braucht die Liste also nicht.
            packets::CB_CONFIG_UPDATE_ENABLED_FEATURES => {
                if let Ok(features) = packets::read_enabled_features(&payload) {
                    if features.iter().any(|f| f != "minecraft:vanilla") {
                        log(events, format!("Server schaltet Features frei: {}", features.join(", ")));
                    }
                }
            }
            // Zusaetze fuer Absturzberichte -- dieser Client schreibt keine.
            packets::CB_CONFIG_CUSTOM_REPORT_DETAILS => {}
            packets::CB_CONFIG_SERVER_LINKS => match packets::read_server_links(&payload) {
                Ok(links) => {
                    let _ = events.send(ClientEvent::ServerLinks(links));
                }
                Err(_) => log(events, "Server-Links nicht lesbar -- ignoriert"),
            },
            packets::CB_CONFIG_SHOW_DIALOG => match packets::read_show_dialog_config(&payload) {
                Ok(dialog) => {
                    let _ = events.send(ClientEvent::ShowDialog(dialog));
                }
                Err(_) => log(events, "Dialog nicht lesbar -- ignoriert"),
            },
            packets::CB_CONFIG_CLEAR_DIALOG => {
                let _ = events.send(ClientEvent::ClearDialog);
            }
            packets::CB_CONFIG_CODE_OF_CONDUCT => {
                let text = packets::read_code_of_conduct(&payload).map_err(|_| NetError::Protocol("Verhaltenskodex nicht lesbar".into()))?;
                let _ = events.send(ClientEvent::CodeOfConduct(text));
            }
            packets::CB_CONFIG_REGISTRY_DATA => {
                // Nur die gebrauchten Registries werden gelesen, der Rest (Painting, ...) wird
                // ignoriert -- jeder Leser liefert None, wenn dieses Paket eine andere
                // Registry trug.
                //
                // Biome: Gras-/Laub-/Wasser-Faerbung.
                match packets::read_registry_data(&payload) {
                    Ok(Some(biomes)) => {
                        let _ = events.send(ClientEvent::BiomeRegistry(biomes));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Biome) konnte nicht gelesen werden -- ignoriert"),
                }
                // Dimensionsarten: min_y und Hoehe. Ohne sie steht die ganze Welt in der
                // falschen Hoehe, sobald ein Server nicht die Overworld-Masse benutzt.
                match packets::read_registry_dimension_types(&payload) {
                    Ok(Some(dims)) => {
                        let _ = events.send(ClientEvent::DimensionRegistry(dims));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Dimensionsarten) konnte nicht gelesen werden -- ignoriert"),
                }
                // Schadensarten: welcher Klang beim eigenen Treffer gehoert wird. Der Index im
                // Schadenspaket zeigt in genau diese Liste.
                match packets::read_damage_types(&payload) {
                    Ok(Some(effects)) => {
                        let _ = events.send(ClientEvent::DamageTypes(effects));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Schadensarten) konnte nicht gelesen werden -- ignoriert"),
                }
                // Verzauberungen: Namen, Tooltips, und die Effekte, die der Client selbst
                // ausfuehrt (Schnellladen, Sog, Fluch der Bindung -- s. world::enchantment).
                match packets::read_enchantments(&payload) {
                    Ok(Some(entries)) => {
                        let _ = events.send(ClientEvent::Enchantments(entries));
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Verzauberungen) konnte nicht gelesen werden -- ignoriert"),
                }
                match packets::read_named_registry(&payload) {
                    Ok(Some((registry, entries))) => {
                        let _ = events.send(ClientEvent::NamedRegistry { registry, entries });
                    }
                    Ok(None) => {}
                    Err(_) => log(events, "Registry Data (Trims/Banner) konnte nicht gelesen werden -- ignoriert"),
                }
            }
            // Plugin Message, Update Tags etc. -- fuer den Join ignorierbar, aber einmal
            // sichtbar machen (s. seen_unhandled).
            _ => {
                if !seen_unhandled.contains(&id) {
                    seen_unhandled.push(id);
                    log(events, format!("Config-Paket 0x{id:02X} nicht behandelt ({} Bytes)", payload.len()));
                }
            }
        }
    }
}

// Play-State: haelt die Verbindung offen und bedient Keep Alive + Teleport-Bestaetigung,
// damit der Spieler verbunden bleibt und wirklich spawnt. Laeuft bis Disconnect/Fehler oder
// bis der Server uns weiterschickt (Ok mit dem Transfer-Ziel).
// Unbekannte Packet-IDs werden geloggt (einmalig pro ID), um die versionsabhaengigen
// Play-IDs empirisch gegen den echten Server verifizieren zu koennen.
// `snapshot`: Wiedergabe einer Aufnahme ab der Mitte, deren Anfangszustand vor dem ersten Paket.
pub(super) async fn run_play_loop(
    conn: &mut Connection,
    events: &Events,
    outgoing: &mut Outgoing,
    rotation: &SharedRotation,
    snapshot: Vec<Vec<u8>>,
) -> Result<TransferTarget, NetError> {
    log(events, "Play-State erreicht -- im Spiel");
    conn.log.enter_play();
    conn.set_record_play(true);
    let _ = events.send(ClientEvent::Connected);

    let mut stats = PlayStats::default();
    let mut unlesbar = 0;
    for b in &snapshot {
        match crate::snapshot_decode(b) {
            Some(e) => {
                // Ausgangspunkt fuer den Dimensionsvergleich beim Respawn, wie beim Login.
                if let ClientEvent::WorldIdentity { dimension, .. } = &e {
                    stats.dimension = Some(dimension.clone());
                }
                let _ = events.send(e);
            }
            None => unlesbar += 1,
        }
    }
    if !snapshot.is_empty() {
        log(events, format!("Wiedergabe: Anfangszustand mit {} Eintraegen geladen{}", snapshot.len(), if unlesbar > 0 { format!(", {unlesbar} unlesbar") } else { String::new() }));
    }
    drop(snapshot);
    // Chunk-Stapel-Bestaetigung: OHNE die schickt der Server nach dem ersten Stapel keinen
    // einzigen Chunk mehr (s. packets::CB_PLAY_CHUNK_BATCH_START).
    let mut chunk_batch = crate::chunk_batch::ChunkBatchSizeCalculator::new();
    // Wird true, sobald die App ihren Sender fallen laesst (z.B. beim Reconnect/Trennen).
    // Ohne diese Wache wuerde outgoing.recv() danach bei jedem select! sofort wieder mit
    // None zurueckkommen -- eine Busy-Loop, die einen Kern voll auslastet.
    let mut outgoing_closed = false;

    // Chunk-Wachhund, s. CHUNK_WATCHDOG_INTERVAL. Nach einer Rekonfiguration faengt die
    // Wartezeit von vorn an -- der Server schickt danach eine komplett neue Welt.
    let mut play_start = std::time::Instant::now();
    let mut watchdog = tokio::time::interval(CHUNK_WATCHDOG_INTERVAL);
    watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut stockung = crate::stall::StallWatch::default();
    let tick_end = packets::client_tick_end_payload();
    // In der Wiedergabe sind Pausen gewollt, keine Stockung.
    let wiedergabe = conn.replay.is_some();
    let melde = |outgoing: bool, d: Duration| {
        if !wiedergabe {
            let _ = events.send(ClientEvent::PacketStall { outgoing, millis: d.as_millis().min(u128::from(u32::MAX)) as u32 });
        }
    };

    let mut war_hinten = false;
    loop {
        // Im Hintergrund-Tab schickt die App keine Ticks: die Luecke bis zum ersten Tick danach
        // ist keine Stockung. Mitmessen erst wieder ab dem Moment, in dem der Tab vorn ist.
        let hinten = events.in_background();
        if hinten || war_hinten {
            stockung = crate::stall::StallWatch::default();
        }
        war_hinten = hinten;
        tokio::select! {
            // biased + Sendezweig ZUERST: die Reihenfolge auf der Leitung darf nicht vom
            // Zufall abhaengen.
            //
            // ECHTER BUG gewesen, sichtbar als GrimAC "Post (use item)" und
            // "PacketOrderO (type=PONG)": ohne `biased` waehlt tokio::select! zwischen zwei
            // gleichzeitig bereiten Zweigen ZUFAELLIG. Traf ein Ping des Servers ein, waehrend
            // die Pakete eines Ticks noch in der Warteschlange lagen, konnte die Pong-Antwort
            // MITTEN in diesen Stapel geschrieben werden. Grims Post-Check merkt sich alle
            // Pakete seiner Liste nach dem Bewegungspaket und flaggt, sobald die Transaktion
            // (Pong) eintrifft und die Liste nicht leer ist -- ein Benutzen-Paket, das
            // zwischen Bewegungspaket und Pong rutscht, ist genau das.
            //
            // Vanilla kann das nicht: dort gehen die Pakete eines Ticks und die Ping-Antwort
            // durch DIESELBE Netty-Event-Loop-Warteschlange, die Reihenfolge ist also die der
            // Einreihung -- ein waehrend des Tick-Stapels eintreffender Ping wird erst nach
            // dem Stapel beantwortet. Genau das stellt `biased` mit dem Sendezweig vorn her.
            //
            // Aushungern kann der Empfangszweig dadurch nicht: die App erzeugt eine Handvoll
            // Pakete je 50-ms-Tick, die Warteschlange ist praktisch immer sofort leer.
            biased;
            msg = outgoing.recv(), if !outgoing_closed => {
                match msg {
                    Some(payload) => {
                        conn.flush_held().await?;
                        let start = std::time::Instant::now();
                        conn.send(&payload).await?;
                        let jetzt = std::time::Instant::now();
                        let haengt = jetzt.duration_since(start);
                        if haengt >= crate::stall::OUT_WRITE_STALL {
                            melde(true, haengt);
                        } else if payload == tick_end {
                            if let Some(luecke) = stockung.tick_end(jetzt) {
                                melde(true, luecke);
                            }
                        }
                    }
                    None => outgoing_closed = true,
                }
            }
            result = conn.recv() => {
                let payload = result?;
                if let Some(pause) = stockung.incoming(std::time::Instant::now()) {
                    melde(false, pause);
                }
                let flow = handle_play_packet(conn, events, &payload, &mut stats, &mut chunk_batch, rotation).await?;
                if !events.in_background() {
                    conn.flush_held().await?;
                }
                match flow {
                    PlayFlow::Continue => {}
                    PlayFlow::Transfer(ziel) => return Ok(ziel),
                    PlayFlow::Reconfigure => {
                        // Zurueck in den Configuration-State und danach wieder hierher --
                        // s. packets::CB_PLAY_START_CONFIGURATION.
                        log(events, "Server schickt uns zurueck in die Konfiguration");
                        conn.send(&packets::configuration_acknowledged_payload()).await?;
                        conn.set_record_play(false);
                        let _ = events.send(ClientEvent::Reconfiguring);
                        if let Some(ziel) = do_configuration(conn, events, outgoing).await? {
                            return Ok(ziel);
                        }
                        conn.set_record_play(true);
                        log(events, "Konfiguration erneut abgeschlossen -- zurueck im Spiel");
                        // Alles, was zur alten Welt gehoert, faengt von vorn an: Zaehler,
                        // Chunk-Tempo und die Frist des Wachhunds.
                        stats = PlayStats::default();
                        chunk_batch = crate::chunk_batch::ChunkBatchSizeCalculator::new();
                        play_start = std::time::Instant::now();
                        // Die Konfiguration selbst ist keine Stockung.
                        stockung = crate::stall::StallWatch::default();
                    }
                }
            }
            _ = watchdog.tick() => {
                let wartet = play_start.elapsed();
                if stats.chunks_ok == 0 && wartet >= CHUNK_WATCHDOG_GRACE && !events.in_background() && !wiedergabe {
                    log(events, stats.chunk_stall_report(wartet));
                    // Erneut bestaetigen: bleibt beim Server auch nur EINE Bestaetigung aus,
                    // steht die Chunk-Lieferung fuer immer (ChunkDataSender.sendChunkBatches
                    // sendet nur solange unacknowledgedBatches < maxUnacknowledgedBatches, und
                    // maxUnacknowledgedBatches ist vor der ersten Bestaetigung 1). Eine
                    // ueberzaehlige Bestaetigung ist dagegen harmlos: onAcknowledgeChunks
                    // zieht nur den Zaehler herunter (ohne untere Schranke) und setzt das
                    // Tempo neu -- sie kann die Lieferung also nur anschieben, nie bremsen.
                    stats.acks_sent += 1;
                    conn.send(&packets::chunk_batch_received_payload(chunk_batch.desired_chunks_per_tick())).await?;
                }
            }
        }
    }
}

// Wie oft der Chunk-Wachhund nachsieht und wie lange er dem normalen Ablauf Zeit laesst,
// bevor er sich meldet. Zweck: der Client blieb gelegentlich beim Verbinden haengen -- Chat,
// Inventar und Entities kamen an, aber kein einziger Chunk, und das Verbindungs-Overlay
// (build_connecting_overlay, sichtbar solange chunks_received == 0) stand fuer immer. Ohne
// Diagnose ist von aussen nicht unterscheidbar, ob der Server nichts mehr schickt, leere
// Stapel schickt oder wir die Chunk-Pakete nicht erkennen -- genau das steht jetzt im Log.
pub(super) const CHUNK_WATCHDOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
pub(super) const CHUNK_WATCHDOG_GRACE: std::time::Duration = std::time::Duration::from_secs(5);
