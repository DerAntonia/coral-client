// coral-record: der Snapshot einer Aufnahme, die mitten im Spiel beginnt (s. record::Tag::Z,
// net::snapshot). Aus dem Zustand der App entstehen dieselben dekodierten Ereignisse, die beim
// Join kaemen, in derselben Reihenfolge: Registries, Login, Spieler, Spielerliste, Scoreboard,
// Welt, Entities. Die Wiedergabe spielt sie vor dem ersten Paket durch den normalen Weg.
//
// Hier im Hauptthread wird nur eingesammelt (die Chunk-Spalten als Arc, also ohne Kopie);
// Spalten umwandeln und kodieren macht ein eigener Thread.
//
// Noch nicht drin: Koepfe mit Skin in der Welt, Karten, offene Container, Titel.

use crate::{App, Instant};
use std::sync::Arc;

// Eine Spalte samt ihrer Schilder.
struct ColumnJob {
    x: i32,
    z: i32,
    column: Arc<world::chunk::ChunkColumn>,
    signs: Vec<((i32, i32, i32), protocol::sign::SignText)>,
}

impl App {
    // Merkt die Registries und Packs der Verbindung in kodierter Form (s. drain_events).
    pub(crate) fn snapshot_note_config(&mut self, event: &net::ClientEvent) {
        use net::ClientEvent as E;
        let pack = matches!(event, E::ResourcePackPush(_));
        let registry = matches!(
            event,
            E::BiomeRegistry(_) | E::DimensionRegistry(_) | E::DamageTypes(_) | E::Enchantments(_) | E::NamedRegistry { .. } | E::ItemTags(_) | E::BlockTags(_)
        );
        if pack || registry {
            if let Some(b) = net::snapshot_encode(event) {
                self.snapshot_config.push((pack, b));
            }
        }
    }

    // Die Aufnahme `session` hat gerade mitten im Spiel begonnen: alles davor ist verarbeitet.
    pub(crate) fn write_snapshot(&mut self, session: u64) {
        let start = Instant::now();
        let config: Vec<Vec<u8>> = self.snapshot_config.iter().map(|(_, b)| b.clone()).collect();
        let vor = self.snapshot_before_world();
        let mut signs: std::collections::HashMap<(i32, i32), Vec<_>> = std::collections::HashMap::new();
        for (pos, text) in &self.signs {
            signs.entry((pos.0.div_euclid(16), pos.2.div_euclid(16))).or_default().push((*pos, text.clone()));
        }
        let columns: Vec<ColumnJob> = self
            .world
            .column_keys()
            .filter_map(|(x, z)| Some(ColumnJob { x, z, column: self.world.column_arc(x, z)?, signs: signs.remove(&(x, z)).unwrap_or_default() }))
            .collect();
        let nach = self.snapshot_after_world();
        let gesammelt = start.elapsed();
        let (n_spalten, n_ereignisse) = (columns.len(), config.len() + vor.len() + nach.len());
        self.push_log(format!("coral-record: Snapshot mit {n_spalten} Spalten und {n_ereignisse} Ereignissen ({:.1} ms eingesammelt)", gesammelt.as_secs_f64() * 1000.0));
        let gestartet = std::thread::Builder::new().name("coral-snapshot".into()).spawn(move || {
            let mut p = record::Producer::blocking();
            let mut unkodierbar = 0;
            let mut bytes = 0;
            let mut push = |p: &mut record::Producer, e: &net::ClientEvent| match net::snapshot_encode(e) {
                Some(b) => {
                    bytes += b.len();
                    p.push(session, record::Tag::Z, Instant::now(), &b);
                }
                None => unkodierbar += 1,
            };
            for b in &config {
                p.push(session, record::Tag::Z, Instant::now(), b);
            }
            for e in &vor {
                push(&mut p, e);
            }
            for job in columns {
                push(&mut p, &net::ClientEvent::Chunk(column_chunk(job)));
            }
            for e in &nach {
                push(&mut p, e);
            }
            // Leer = abgeschlossen (s. record::Tag::Z).
            p.push(session, record::Tag::Z, Instant::now(), &[]);
            p.flush();
            eprintln!(
                "coral-record: Snapshot geschrieben, {:.1} MB in {:.0} ms{}",
                bytes as f64 / 1e6,
                start.elapsed().as_secs_f64() * 1000.0,
                if unkodierbar > 0 { format!(", {unkodierbar} nicht kodierbar") } else { String::new() }
            );
        });
        if let Err(e) = gestartet {
            self.push_log(format!("coral-record: Snapshot-Thread nicht startbar ({e})"));
        }
    }

    // Was vor den Chunks kommt: Login, eigener Spieler, Spielerliste, Scoreboard, Wetter.
    fn snapshot_before_world(&self) -> Vec<net::ClientEvent> {
        use net::ClientEvent as E;
        use protocol::packets as p;
        let mut ev = Vec::new();
        if let Some(id) = self.own_entity_id {
            ev.push(E::OwnEntityId(id));
        }
        if let Some((dimension, seed)) = self.lod.identity.clone() {
            ev.push(E::WorldIdentity { dimension, seed });
        }
        if let Some(i) = self.dimension_types.iter().position(|d| d.name == self.dimension_type_name) {
            ev.push(E::DimensionType(i as i32));
        }
        ev.push(E::Hardcore(self.hardcore));
        ev.push(E::GameMode(self.game_mode));
        let a = &self.physics.abilities;
        let mut flags = 0;
        for (an, bit) in [(a.invulnerable, p::ABILITY_INVULNERABLE), (a.flying, p::ABILITY_FLYING), (a.may_fly, p::ABILITY_MAY_FLY), (a.instabuild, p::ABILITY_INSTABUILD)] {
            if an {
                flags |= bit;
            }
        }
        ev.push(E::Abilities(p::PlayerAbilities { flags, fly_speed: a.fly_speed, walk_speed: a.walk_speed }));
        ev.push(E::Health(self.health, self.food_level, self.food_saturation));
        ev.push(E::Experience { progress: self.xp_progress, level: self.xp_level, total: 0 });
        if let Some(id) = self.own_entity_id {
            ev.push(E::Attributes { id, armor: Some(f64::from(self.armor)), max_health: Some(f64::from(self.max_health)) });
            for (&effect, s) in &self.shown_effects {
                let vergangen = (s.since.elapsed().as_secs_f64() * 20.0) as i32;
                let duration = if s.duration == p::MOB_EFFECT_INFINITE { s.duration } else { (s.duration - vergangen).max(0) };
                ev.push(E::MobEffect { entity_id: id, effect, amplifier: s.amplifier, duration, flags: s.flags });
            }
        }
        ev.push(E::HeldSlot(self.inventory.held_slot as i32));
        ev.push(E::ContainerContent(p::ContainerContent {
            sync_id: 0,
            revision: self.inventory.player_revision,
            slots: self.inventory.player.clone(),
            cursor: self.inventory.cursor.clone(),
            truncated_at: None,
            truncated_detail: None,
        }));

        // Spielerliste. Wer nicht (mehr) in der Liste steht, aber einen Namen hat (etwa NPCs, die
        // nach dem Spawn aus der Liste fliegen), kommt hinein und gleich wieder heraus.
        let mut eintraege: Vec<p::PlayerInfoEntry> = self
            .tab_entries
            .iter()
            .map(|(uuid, t)| p::PlayerInfoEntry {
                uuid: *uuid,
                name: Some(t.name.clone()),
                textures_property: self.textures_props.get(uuid).cloned(),
                game_mode: Some(t.game_mode),
                listed: Some(t.listed),
                latency: Some(t.latency),
                display_name: Some(t.display_name.clone()),
                list_order: Some(t.list_order),
                show_hat: Some(t.show_hat),
            })
            .collect();
        let nur_namen: Vec<[u8; 16]> = self.player_names.keys().filter(|u| !self.tab_entries.contains_key(*u)).copied().collect();
        for uuid in &nur_namen {
            eintraege.push(p::PlayerInfoEntry {
                uuid: *uuid,
                name: self.player_names.get(uuid).cloned(),
                textures_property: self.textures_props.get(uuid).cloned(),
                game_mode: None,
                listed: None,
                latency: None,
                display_name: None,
                list_order: None,
                show_hat: None,
            });
        }
        ev.push(E::PlayerInfo(eintraege));
        if !nur_namen.is_empty() {
            ev.push(E::PlayerInfoRemove(nur_namen));
        }
        ev.push(E::TabList { header: self.tab_header.clone(), footer: self.tab_footer.clone() });

        // Scoreboard: Teams mit Mitgliedern, Objectives, Anzeige, Scores.
        for (name, t) in &self.sb_teams {
            let entries = self.sb_entry_team.iter().filter(|(_, team)| *team == name).map(|(e, _)| e.clone()).collect();
            ev.push(E::Team {
                name: name.clone(),
                mode: 0,
                prefix: Some(t.prefix.clone()),
                suffix: Some(t.suffix.clone()),
                name_tag_visibility: Some(t.name_tag_visibility),
                color: Some(t.color),
                collision_rule: Some(t.collision_rule),
                entries,
                display_name: None,
            });
        }
        for (name, titel) in &self.sb_objective_titles {
            ev.push(E::ScoreboardObjective {
                name: name.clone(),
                mode: 0,
                display: Some(titel.clone()),
                hearts: self.sb_objective_hearts.contains(name),
                number_format: self.sb_objective_formats.get(name).cloned(),
            });
        }
        if let Some(o) = &self.sb_sidebar_objective {
            ev.push(E::ScoreboardDisplay { slot: 1, objective: o.clone() });
        }
        if let Some(o) = &self.sb_list_objective {
            ev.push(E::ScoreboardDisplay { slot: 0, objective: o.clone() });
        }
        for (objective, scores) in &self.sb_scores {
            for (holder, (score, display)) in scores {
                ev.push(E::ScoreboardScore {
                    holder: holder.clone(),
                    objective: objective.clone(),
                    score: *score,
                    display: display.clone(),
                    number_format: self.sb_score_formats.get(&(objective.clone(), holder.clone())).cloned(),
                });
            }
        }
        for b in &self.boss_bars {
            ev.push(E::BossEvent { uuid: b.uuid, event: p::BossEvent::Add { name: b.name.clone(), progress: b.target, color: b.color, style: b.style, flags: 0 } });
        }
        ev.push(E::Border(self.world.border.snapshot()));
        ev.push(E::WorldTime { time_of_day: self.world_time_of_day, ticking: self.world_time_ticking });
        ev.push(E::RainGradient(self.rain_gradient));
        ev.push(E::ThunderGradient(self.thunder_gradient));
        ev
    }

    // Was nach den Chunks kommt: die eigene Position und die Entities.
    fn snapshot_after_world(&self) -> Vec<net::ClientEvent> {
        use net::ClientEvent as E;
        let mut ev = Vec::new();
        let pos = self.physics.pos;
        ev.push(E::PlayerPosition(protocol::packets::PlayerPositionSync {
            pos: (pos.x, pos.y, pos.z),
            vel: (0.0, 0.0, 0.0),
            yaw: self.camera.yaw,
            pitch: self.look_pitch_deg(),
            flags: 0,
        }));
        let (entities, passengers) = self.entities.snapshot();
        for e in entities {
            let id = e.spawn.entity_id;
            let kopf = e.spawn.head_yaw_deg;
            ev.push(E::EntitySpawn(e.spawn));
            ev.push(E::EntityMeta(Box::new(e.meta)));
            ev.push(E::EntityPose { id, pose: e.pose });
            ev.push(E::EntityHeadYaw { id, head_yaw_deg: kopf });
            if !e.equipment.is_empty() {
                ev.push(E::EntityEquipment { id, entries: e.equipment });
            }
        }
        for (vehicle, passengers) in passengers {
            ev.push(E::Passengers { vehicle, passengers });
        }
        if let Some(id) = self.camera_entity {
            ev.push(E::SetCamera(id));
        }
        ev
    }
}

// Eine Spalte, wie sie als Chunk-Paket ankaeme: Sections, Himmelslicht, Schilder.
fn column_chunk(job: ColumnJob) -> protocol::chunk::ChunkData {
    let sections = job
        .column
        .sections
        .iter()
        .map(|s| protocol::chunk::ChunkSectionData {
            block_count: s.iter_states().filter(|&v| v != world::STATE_AIR).count() as i16,
            block_states: s.iter_states().collect(),
            biomes: s.biomes().to_vec(),
        })
        .collect();
    let mut light = protocol::chunk::LightData::default();
    for (i, layer) in job.column.sky.iter().enumerate() {
        match layer {
            world::light::SkyLayer::Uniform(0) => light.sky_empty.push(i),
            world::light::SkyLayer::Uniform(v) => light.sky.push((i, vec![v | v << 4; 2048])),
            world::light::SkyLayer::Data(d) => light.sky.push((i, d.to_vec())),
        }
    }
    protocol::chunk::ChunkData { x: job.x, z: job.z, sections, signs: job.signs, heads: Vec::new(), light: Some(light), tail_unreadable: false }
}
