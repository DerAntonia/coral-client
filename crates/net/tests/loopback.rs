// Integrationstest gegen einen echten, lokal laufenden Offline-Mode-Server auf
// 127.0.0.1:25565 (siehe Briefing "Testumgebung"). Braucht einen laufenden Server,
// daher #[ignore] -- manuell ausfuehren mit: cargo test -p net -- --ignored --nocapture

use std::time::{Duration, Instant};

#[tokio::test]
#[ignore]
async fn ping_local_server() {
    let json = net::ping("127.0.0.1:25565").await.expect("Ping fehlgeschlagen");
    println!("Status-JSON: {json}");
    assert!(json.contains("version"));
}

// join() haelt die Verbindung offen und kehrt nur bei Disconnect/Fehler zurueck. Der
// Test laeuft es 6s lang: ein Timeout bedeutet "erfolgreich verbunden geblieben"
// (Play-State erreicht, Keep-Alive bedient); ein frueher Fehler = Join fehlgeschlagen.
#[tokio::test]
#[ignore]
async fn join_local_server() {
    let (tx, rx) = std::sync::mpsc::channel();
    // Empfangene Events nebenbei ausgeben, um den Ablauf zu sehen.
    let collector = std::thread::spawn(move || {
        let mut chunks = 0usize;
        let mut total_solid = 0usize;
        let mut world_state = world::World::new();
        let mut entities = world::Entities::new();
        let mut entity_spawns = 0usize;
        let mut entity_moves = 0usize;
        while let Ok(ev) = rx.recv() {
            match ev {
                net::ClientEvent::Sound(sound) => println!("[event] SOUND {} @ {:?}", sound.name, sound.position),
                net::ClientEvent::StopSound { category, name } => println!("[event] STOP SOUND {category:?} {name:?}"),
                net::ClientEvent::DamageTypes(effects) => println!("[event] {} Schadensarten", effects.len()),
                net::ClientEvent::Enchantments(entries) => println!("[event] {} Verzauberungen", entries.len()),
                net::ClientEvent::NamedRegistry { registry, entries } => println!("[event] {registry}: {} Eintraege", entries.len()),
                net::ClientEvent::SignText { pos, .. } => println!("[event] Schild bei {pos:?}"),
                net::ClientEvent::OpenSignEditor { pos, front } => println!("[event] Schild-Editor {pos:?} vorn={front}"),
                net::ClientEvent::OpenBook { hand } => println!("[event] Buch oeffnen, Hand {hand}"),
                net::ClientEvent::MapData(d) => println!("Karte {}", d.id),
                net::ClientEvent::RainGradient(g) => println!("[event] Regenstaerke {g}"),
                net::ClientEvent::ThunderGradient(g) => println!("[event] Gewitterstaerke {g}"),
                net::ClientEvent::InitialChunksComing => println!("[event] Chunks kommen"),
                net::ClientEvent::ProjectileHitPlayer => println!("[event] Pfeiltreffer"),
                net::ClientEvent::DimensionRegistry(dims) => println!("[event] {} Dimensionsarten", dims.len()),
                net::ClientEvent::DimensionType(i) => println!("[event] Dimensionsart {i}"),
                net::ClientEvent::Log(msg) => println!("[event] {msg}"),
                net::ClientEvent::Connected => println!("[event] CONNECTED"),
                net::ClientEvent::Disconnected(reason) => println!("[event] DISCONNECTED: {reason}"),
                net::ClientEvent::Reconfiguring => println!("[event] RECONFIGURING"),
                net::ClientEvent::Respawned { world_change } => println!("[event] RESPAWNED (Weltwechsel: {world_change})"),
                net::ClientEvent::WorldIdentity { dimension, seed } => println!("[event] WELT {dimension} {seed:x}"),
                net::ClientEvent::Chunk(chunk) => {
                    chunks += 1;
                    let solid: usize = chunk
                        .sections
                        .iter()
                        .map(|s| s.block_states.iter().filter(|&&id| id != protocol::chunk::STATE_ID_AIR).count())
                        .sum();
                    total_solid += solid;
                    if chunks <= 3 {
                        // block_count aus dem Packet vs. selbst gezaehlte Nicht-Luft-Bloecke:
                        // Stimmen die ueberein, ist das Bit-Unpacking korrekt.
                        let claimed: i32 = chunk.sections.iter().map(|s| s.block_count as i32).sum();
                        println!(
                            "[chunk] ({}, {}) Sections={} | block_count laut Server={} | selbst gezaehlt={}  {}",
                            chunk.x,
                            chunk.z,
                            chunk.sections.len(),
                            claimed,
                            solid,
                            if claimed as usize == solid { "OK" } else { "<-- ABWEICHUNG!" }
                        );
                    }
                    if chunks == 30 {
                        println!("[chunk] 30 Chunks dekodiert, {total_solid} feste Bloecke gesamt");
                    }

                    let sections = chunk.sections.into_iter().map(|s| world::ChunkSection::from_states(s.block_states)).collect();
                    world_state.insert_column(chunk.x, chunk.z, sections);
                }
                net::ClientEvent::ResourcePackPush(_) | net::ClientEvent::ResourcePackPop(_) | net::ClientEvent::LightUpdate { .. } => {}
                net::ClientEvent::PlayerPosition(sync) => {
                    let (x, y, z) = sync.pos;
                    println!("[event] Spielerposition: {x:.1} / {y:.1} / {z:.1}");
                }
                net::ClientEvent::ServerRotation { yaw_deg, pitch_deg } => {
                    println!("[event] Server dreht die Kamera: yaw={yaw_deg:.1} pitch={pitch_deg:.1}");
                }
                net::ClientEvent::ForgetChunk(x, z) => {
                    // Wie der Client: entladene Spalten wieder aus dem Weltzustand werfen,
                    // damit die Vertex-Bilanz am Ende die tatsaechlich geladene Welt misst.
                    world_state.remove_column(x, z);
                }
                // Entity-Events wie im Client auf den Entity-Zustand anwenden -- die
                // Zusammenfassung am Ende zeigt, ob Spawn/Move/Remove real dekodiert werden.
                net::ClientEvent::EntitySpawn(spawn) => {
                    entity_spawns += 1;
                    entities.spawn(
                        spawn.entity_id,
                        spawn.uuid,
                        spawn.entity_type,
                        (spawn.x, spawn.y, spawn.z),
                        spawn.yaw_deg,
                        spawn.pitch_deg,
                        spawn.vel,
                    );
                }
                net::ClientEvent::EntityMoveRel { id, dx, dy, dz, rot } => {
                    entity_moves += 1;
                    entities.move_relative(id, dx, dy, dz);
                    if let Some((yaw, pitch)) = rot {
                        entities.set_rotation(id, yaw, pitch);
                    }
                }
                net::ClientEvent::EntityRot { id, yaw_deg, pitch_deg } => {
                    entities.set_rotation(id, yaw_deg, pitch_deg);
                }
                net::ClientEvent::EntityPos { id, x, y, z, yaw_deg, pitch_deg, .. } => {
                    entity_moves += 1;
                    entities.set_position(id, x, y, z);
                    entities.set_rotation(id, yaw_deg, pitch_deg);
                }
                net::ClientEvent::EntitiesRemove(ids) => {
                    entities.remove(&ids);
                }
                net::ClientEvent::EntityEquipment { id, entries } => {
                    for entry in entries {
                        let held = entry
                            .item
                            .filter(|stack| stack.item_id != 0)
                            .map(|stack| world::HeldItem::from_stack(&stack));
                        entities.set_equipment(id, entry.slot, held);
                    }
                }
                net::ClientEvent::Health(h, food) => {
                    println!("[event] Leben: {h}, Nahrung: {food}");
                }
                net::ClientEvent::EntityVelocity(id, vel) => {
                    println!("[event] Geschwindigkeit fuer {id}: {vel:?}");
                }
                net::ClientEvent::ExplosionKnockback(vel) => {
                    println!("[event] Explosions-Rueckstoss: {vel:?}");
                }
                net::ClientEvent::ExplosionEffects(e) => {
                    println!("[event] Explosion bei {:?}, {} Blockpartikel-Arten", e.center, e.block_particles.len());
                }
                net::ClientEvent::Particles(p) => {
                    println!("[event] Partikel {:?} x{}", protocol::particle::particle_name(p.particle.type_id), p.count);
                }
                net::ClientEvent::LevelEvent(e) => {
                    println!("[event] Level-Event {} bei {:?}", e.event, e.pos);
                }
                net::ClientEvent::MobEffect { entity_id, effect, amplifier, duration, .. } => {
                    println!("[event] Effekt {effect} (Staerke {amplifier}, {duration} Ticks) fuer {entity_id}");
                }
                net::ClientEvent::BiomeRegistry(biomes) => {
                    println!("[event] Biom-Registry: {} Eintraege", biomes.len());
                }
                net::ClientEvent::BlockUpdate(blocks) => {
                    println!("[event] Block-Update: {} Bloecke", blocks.len());
                }
                net::ClientEvent::BlockChangedAck(sequence) => {
                    println!("[event] Block Changed Ack: Sequenz {sequence}");
                }
                net::ClientEvent::OwnEntityId(id) => {
                    println!("[event] eigene EntityId: {id}");
                }
                net::ClientEvent::Chat(spans, _overlay) => {
                    // Die Abschnitte tragen jetzt ihre Farben (s. ClientEvent::Chat) -- fuer
                    // die Ausgabe hier reicht der zusammengezogene Text.
                    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                    println!("[event] Chat: {text}");
                }
                // HUD-Overlay-Events (Actionbar/Titel/Scoreboard) sind fuer diesen
                // Netzwerk-Loopback-Test ohne Belang.
                net::ClientEvent::ActionBar(_)
                | net::ClientEvent::Title(_)
                | net::ClientEvent::Subtitle(_)
                | net::ClientEvent::TitleFade { .. }
                | net::ClientEvent::ClearTitles
                | net::ClientEvent::ScoreboardDisplay { .. }
                | net::ClientEvent::ScoreboardObjective { .. }
                | net::ClientEvent::ScoreboardScore { .. }
                | net::ClientEvent::ScoreboardResetScore { .. }
                | net::ClientEvent::Team { .. }
                | net::ClientEvent::Passengers { .. }
                | net::ClientEvent::VehicleMove { .. }
                | net::ClientEvent::BossEvent { .. }
                | net::ClientEvent::ItemCooldown { .. }
                | net::ClientEvent::CustomPayload { .. }
                | net::ClientEvent::MiningAttributes { .. }
                | net::ClientEvent::MovementAttributes { .. }
                | net::ClientEvent::RangeAttributes { .. }
                | net::ClientEvent::GameMode(_)
                | net::ClientEvent::Abilities(_)
                | net::ClientEvent::SetCamera(_)
                | net::ClientEvent::Experience { .. }
                | net::ClientEvent::Attributes { .. }
                | net::ClientEvent::EntityAir { .. }
                | net::ClientEvent::ContainerContent(_)
                | net::ClientEvent::ContainerSlot(_)
                | net::ClientEvent::CursorItem(_)
                | net::ClientEvent::PlayerInventorySlot(..)
                | net::ClientEvent::HeldSlot(_)
                | net::ClientEvent::OpenScreen(_)
                | net::ClientEvent::CloseScreen(_)
                | net::ClientEvent::ContainerData(..)
                | net::ClientEvent::MerchantOffers(..)
                | net::ClientEvent::PlayerInfo(_)
                | net::ClientEvent::PlayerInfoRemove(_)
                | net::ClientEvent::TabList { .. }
                | net::ClientEvent::OwnUuid(_)
                | net::ClientEvent::EntityEvent { .. }
                | net::ClientEvent::CommandSuggestions(_)
                | net::ClientEvent::EntityHeadYaw { .. }
                | net::ClientEvent::EntityPose { .. }
                | net::ClientEvent::EntityMeta(_)
                | net::ClientEvent::EntityDamaged { .. }
                | net::ClientEvent::EntityAnimation { .. }
                | net::ClientEvent::MobEffectRemoved { .. }
                | net::ClientEvent::BlockEvent(..)
                | net::ClientEvent::HeadAt { .. }
                | net::ClientEvent::WorldTime { .. }
                | net::ClientEvent::EntitySleeping { .. } => {}
            }
        }

        println!("[entity] {} Spawns, {} Positions-Updates, {} aktuell getrackt", entity_spawns, entity_moves, entities.len());

        // Kanal zu (join() ist durch) -- jetzt messen, was die empfangene Welt an
        // Geometrie ergibt. Bewusst hier auf der CPU: der Face-Culling-Mesher ist nicht
        // sparsam, und das ist die Zahl, die im Client als VRAM auflaufen wuerde.
        let started = Instant::now();
        let mut vertices = 0usize;
        for (x, z) in world_state.take_dirty(usize::MAX) {
            let (opaque, translucent) = world::mesh_column(
                &world_state,
                x,
                z,
                &world::BlockUvs::single_tile(),
                &world::BlockShapes::all_full_cubes(),
                &world::BiomePalette::fallback(),
                &world::CrossBlocks::none(),
                &world::BlockShapes::all_full_cubes(),
            );
            vertices += opaque.len() + translucent.len();
        }
        let bytes = vertices * std::mem::size_of::<world::BlockVertex>();
        println!(
            "[mesh] {} Spalten -> {} Vertices ({:.1} MB Vertex-Daten) in {:.1}s ({:.0} ms/Spalte)",
            world_state.column_count(),
            vertices,
            bytes as f64 / (1024.0 * 1024.0),
            started.elapsed().as_secs_f64(),
            started.elapsed().as_secs_f64() * 1000.0 / world_state.column_count().max(1) as f64,
        );
    });

    let (_outgoing_tx, outgoing_rx) = tokio::sync::mpsc::unbounded_channel();
    let rotation = std::sync::Arc::new(net::SharedLook::default());
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        net::join("127.0.0.1:25565", "IntegrationTest", None, net::Events::new(tx), outgoing_rx, rotation),
    )
    .await;

    match result {
        Err(_timeout) => println!("Verbindung 20s gehalten -- Join erfolgreich"),
        Ok(Ok(())) => println!("Verbindung sauber beendet"),
        Ok(Err(e)) => panic!("Join fehlgeschlagen: {e}"),
    }

    // Mit dem Ende von join() (auch per Timeout) faellt der Sender weg, der Collector-
    // Thread laeuft aus und gibt seine Mesh-Statistik aus. Ohne dieses Warten waere der
    // Test vorher zu Ende und wir saehen die Zahlen nie.
    let _ = collector.join();
}
