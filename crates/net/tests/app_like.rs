// Stellt nach, wie die App das Netzwerk anbindet -- im Gegensatz zum loopback-Test, der
// eine current_thread-Runtime (#[tokio::test]) und ein blockierendes recv() nutzt:
//   - eigene Multi-Thread-Runtime (tokio::runtime::Runtime::new)
//   - join() als spawn()-Hintergrundtask
//   - Events per try_recv aus einer Frame-Schleife abgeholt, nicht blockierend
//
// Hintergrund: der Client empfaengt genau EINEN Chunk, der loopback-Test dagegen ~473.
// Dieser Test grenzt ein, ob das an dieser Anbindung liegt oder an allem darueber (GPU).
//
// Manuell ausfuehren: cargo test -p net --test app_like -- --ignored --nocapture

use std::time::{Duration, Instant};

#[test]
#[ignore]
fn app_like_polling_receives_chunks() {
    let rt = tokio::runtime::Runtime::new().expect("Runtime");
    let (tx, rx) = std::sync::mpsc::channel();

    // Bei jedem Lauf ein frischer Name: ein Spieler, der im Test gestorben ist, haengt bei
    // jedem weiteren Join im Todesbildschirm -- und ein toter Spieler bekommt keine Chunks.
    // Das hat schon einmal wie ein Fehler im Client ausgesehen und war keiner.
    let name = std::env::var("MC_TEST_NAME")
        .unwrap_or_else(|_| format!("T{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 100000));
    let (_outgoing_tx, outgoing_rx) = tokio::sync::mpsc::unbounded_channel();
    let task = rt.spawn(async move {
        let rotation = std::sync::Arc::new(net::SharedLook::default());
        let _ = net::join("127.0.0.1:25565", &name, None, net::Events::new(tx), outgoing_rx, rotation).await;
    });

    // Frame-Schleife wie App::about_to_wait: alle ~16ms nicht blockierend leerlaufen.
    let start = Instant::now();
    let mut chunks = 0usize;
    let (mut mit_licht, mut licht_ebenen) = (0usize, 0usize);
    let mut positions = 0usize;
    while start.elapsed() < Duration::from_secs(15) {
        while let Ok(event) = rx.try_recv() {
            match event {
                net::ClientEvent::Chunk(c) => {
                    chunks += 1;
                    // Himmelslicht (Shader-Pfad): mit dem Chunk mitgelesen?
                    if let Some(l) = &c.light {
                        mit_licht += 1;
                        licht_ebenen += l.sky.len();
                    }
                }
                net::ClientEvent::ResourcePackPush(_) | net::ClientEvent::ResourcePackPop(_) | net::ClientEvent::LightUpdate { .. } => {}
                net::ClientEvent::PlayerPosition(..) => positions += 1,
                // Zaehlt wie eine Positionsmeldung: fuer den Durchsatz-Test ist nur wichtig,
                // dass der Server uns verortet hat.
                net::ClientEvent::ServerRotation { .. } => positions += 1,
                // Ton zaehlt fuer den Durchsatz-Test nicht mit, soll aber sichtbar sein:
                // ein Server, der ueberhaupt keine Geraeusche schickt, waere ein Hinweis.
                net::ClientEvent::Sound(sound) => println!("[log] SOUND {}", sound.name),
                net::ClientEvent::StopSound { category, name } => println!("[log] STOP SOUND {category:?} {name:?}"),
                net::ClientEvent::DamageTypes(effects) => println!("[log] {} Schadensarten", effects.len()),
                net::ClientEvent::Enchantments(entries) => println!("[log] {} Verzauberungen", entries.len()),
                net::ClientEvent::NamedRegistry { registry, entries } => println!("[log] {registry}: {} Eintraege", entries.len()),
                net::ClientEvent::SignText { pos, .. } => println!("[log] Schild bei {pos:?}"),
                net::ClientEvent::OpenSignEditor { pos, front } => println!("[log] Schild-Editor {pos:?} vorn={front}"),
                net::ClientEvent::OpenBook { hand } => println!("[log] Buch oeffnen, Hand {hand}"),
                net::ClientEvent::MapData(d) => println!("Karte {}", d.id),
                net::ClientEvent::RainGradient(g) => println!("[log] Regenstaerke {g}"),
                net::ClientEvent::ThunderGradient(g) => println!("[log] Gewitterstaerke {g}"),
                net::ClientEvent::InitialChunksComing => println!("[log] Chunks kommen"),
                net::ClientEvent::ProjectileHitPlayer => println!("[log] Pfeiltreffer"),
                net::ClientEvent::DimensionRegistry(dims) => println!("[log] {} Dimensionsarten", dims.len()),
                net::ClientEvent::DimensionType(i) => println!("[log] Dimensionsart {i}"),
                net::ClientEvent::Log(msg) => println!("[log] {msg}"),
                net::ClientEvent::Connected => println!("[log] CONNECTED"),
                net::ClientEvent::Disconnected(r) => println!("[log] DISCONNECTED: {r}"),
                net::ClientEvent::Reconfiguring => println!("[log] RECONFIGURING"),
                net::ClientEvent::Respawned { world_change } => println!("[log] RESPAWNED (Weltwechsel: {world_change})"),
                net::ClientEvent::WorldIdentity { dimension, seed } => println!("[log] WELT {dimension} {seed:x}"),
                // Chunk-Entladen und Entity-Verkehr sind fuer diesen Durchsatz-Test egal.
                net::ClientEvent::ForgetChunk(..)
                | net::ClientEvent::EntitySpawn(_)
                | net::ClientEvent::EntityMoveRel { .. }
                | net::ClientEvent::EntityRot { .. }
                | net::ClientEvent::EntityPos { .. }
                | net::ClientEvent::VehicleMove { .. }
                | net::ClientEvent::BossEvent { .. }
                | net::ClientEvent::EntitiesRemove(_)
                | net::ClientEvent::EntityEquipment { .. }
                | net::ClientEvent::Health(..)
                | net::ClientEvent::EntityVelocity(..)
                | net::ClientEvent::ExplosionKnockback(_)
                | net::ClientEvent::ExplosionEffects(_)
                | net::ClientEvent::Particles(_)
                | net::ClientEvent::LevelEvent(_)
                | net::ClientEvent::MobEffect { .. }
                | net::ClientEvent::BiomeRegistry(_)
                | net::ClientEvent::BlockUpdate(_)
                | net::ClientEvent::BlockChangedAck(_)
                | net::ClientEvent::OwnEntityId(_)
                | net::ClientEvent::Chat(..)
                | net::ClientEvent::ActionBar(_)
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
                // Neuere Ereignisse sind fuer diesen Test belanglos.
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }

    task.abort();
    println!("[applike] nach 15s: {chunks} Chunks, {positions} Positions-Events");
    println!("[app_like] {chunks} Chunks, {mit_licht} mit Himmelslicht, {licht_ebenen} Lichtebenen mit Daten");
    assert_eq!(mit_licht, chunks, "jeder Chunk muss lesbares Himmelslicht mitbringen");
    assert!(chunks > 50, "App-artige Anbindung bekommt nur {chunks} Chunks -- hier liegt der Fehler");
}
