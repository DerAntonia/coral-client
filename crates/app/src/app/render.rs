// Welt zeichnen: Entities, Ruestung, gehaltene Items, Hitboxen, Himmel, Wetter, Partikel, Schilder.

use crate::{NEAR_SINGLE_SIDED_MARGIN, HITBOX_LINES_OUTLINE, hitbox_kinds, profiler, App, block_anim, Instant, model_definition_id, HAND_MODEL_PRELOAD_MAX, MapSlot, ENTITY_BRIGHTNESS, push_map_quad, blocking_arms, inventory, held_item, SHIELD_BANNER_BASE, dye_rgb, UiSprite, ENTITY_FIRE_TEXTURES, affine_from_mat4, display_billboard_matrix, item_unique_offset, dropped_item_copies, display_affine_matrix, DVec3, GAME_MODE_SPECTATOR, uuid_hyphenated, team_allows_push, ENTITY_DISTANCE_MAX, physics, is_spear_item, arms_taken, is_player_like, DEATH_ANIM_OFF, DEATH_ANIM_EXPLOSION, DEATH_ANIM_FREEZE, DEATH_FREEZE_TICKS, arrow_texture, apply_item_arm_poses, ShieldMark, ShieldColor, teams, COLOR_PICK_MAX, hsv_to_rgb, Vec3, HITBOX_DASH_PER_DENSITY, push_hitbox_edge, OUTLINE_PULL, HITBOX_COBWEB_RADIUS, block_rel, BLOCK_ENTITY_DISTANCE_MAX, head_skin_key, is_christmas, SkyMode, SKY_TIME_DAY, custom_sky, precipitation, WeatherMode, ConnStatus, SKY_WORLD_WAIT, SKY_DAY_SRGB, swing_progress_from_ticks, PLAYER_SWING_TICKS, CrosshairTarget, doll_camera, modules, AppMode, map_tilt, item_model_input, crossbow_pull_progress, TRAIL_WIDTH, consumable_optimizer, experience_orb_icon, eating_crumbs_due, particle_item_name, zufallswert, particles, GpuState, UiRect, UiText, ShadowArea, label_debug_an, WORLD_LABEL_LINE_GAP, NAMETAG_MIN_CLIP_W, WORLD_LABEL_WORLD_SCALE, WORLD_LABEL_MIN_SCALE, WORLD_LABEL_MAX_SCALE, wrap_label_lines, split_label_lines, TEAM_VISIBILITY_ALWAYS, argb_to_rgba, Rect, LabelAlign, COLOR_WHITE, rgb_to_rgba, push_span_shadow, label_debug_log, mat4_from_affine, sign_projector, SignEdit, SignCaret, sign_text_color, caret_alpha, Vertex, pixel_to_ndc, WorldLabel, display_text_frame, player_label_visible, team_allows_name, NAMETAG_SNEAK_DISTANCE, NAMETAG_MAX_DISTANCE, WORLD_LABEL_DEFAULT_LINE_WIDTH, DISPLAY_VIEW_RANGE_BLOCKS, NAMETAG_HEIGHT_ABOVE_HEAD, SIGHT_SAMPLE_STEP, MenuScreen, shared_config_dir, last_view, server_view_last_default, wallpaper, CONNECT_VIEW_FIRST_DELAY, HERO_START_DELAY, hero_scene, HERO_MIN_Y, HERO_SECTIONS, HERO_SETTLE, HERO_GIVE_UP, FrameTiming, zoomed_fov, UiOverlay, FIRE_MODE_CROSSHAIR, GAME_MODE_CREATIVE, CrosshairExtras, weather_texture_part, notifications, WALLPAPER_DESKTOP_RECHECK, FIRE_MODE_VANILLA, SpriteAtlas, HURT_CAM_MAX_DEGREES, SHOULDER_FADE_NEAR, SHOULDER_FADE_FAR, Perspective, KeyBindAction, FREELOOK_VIEW_FIRST, FREELOOK_VIEW_BACK, FREELOOK_VIEW_FRONT, MAX_PITCH};

mod equipment;
mod display_entities;
mod anim_debug;
mod entity_vertices;
mod culling;
pub(crate) mod skin_frame;
mod teams_hitboxes;
mod block_entities;
mod sky_weather;
mod first_person;
mod trails_particles;
mod world_labels;
mod views;
mod frame;
mod camera;

// Ein Entity-Modell, dessen Vertices erst nach der Entity-Schleife entstehen, verteilt auf
// mehrere Kerne (s. App::run_model_jobs). Der Platz in `out` ist schon reserviert -- alles,
// was sich Bereiche merkt (Faerbung, Umriss, Motion Blur), sieht dieselben Zahlen wie beim
// sofortigen Zeichnen. Nur fuer Bereiche, die vor run_model_jobs niemand LIEST (Glint und
// Elytra lesen ihre eigenen Vertices und bleiben deshalb sofort).
pub(crate) struct ModelJob<'m> {
    start: usize,
    len: usize,
    model: &'m world::Model,
    pose: world::Pose,
    base: world::entity_model::Affine,
    tex: [f32; 4],
    // Schon mit der Helligkeit verrechnet, s. world::entity_model::emit_into.
    color: [f32; 3],
    two_sided: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn defer_model<'m>(
    jobs: &mut Vec<ModelJob<'m>>,
    out: &mut Vec<world::BlockVertex>,
    model: &'m world::Model,
    pose: &world::Pose,
    base: world::entity_model::Affine,
    tex: [f32; 4],
    color: [f32; 3],
    two_sided: bool,
) {
    let len = world::entity_model::vertex_count(model, pose, two_sided);
    if len == 0 {
        return;
    }
    let start = out.len();
    out.resize(start + len, bytemuck::Zeroable::zeroed());
    jobs.push(ModelJob { start, len, model, pose: pose.clone(), base, tex, color, two_sided });
}

impl App {
    // Wasser und alle anderen animierten Blocktexturen weiterschalten und die geaenderten
    // Kacheln in EINEM Upload in den Atlas schreiben (s. BlockRenderer::upload_atlas_tiles --
    // einzeln hochgeladen kostete das bei gleichzeitigem Framewechsel mehrere Millisekunden).
    // Ohne GPU nichts tun: der Stand der Blockanimationen bleibt dann None und wird beim
    // ersten Frame mit GPU hochgeladen.
    // Rueckgabe: Zeit fuer den Upload in ms (fuer den Frame-Profiler).
    // Modul "Dynamisches Feuer" (Nachbau der dynamicfire-Mod, FireFadeState.tick): mit
    // Feuerresistenz blenden Feuerbloecke je Tick um 0,05 aus -- nach 20 Ticks unsichtbar --,
    // ohne sie wieder ein. Die Mod verlangt dafuer nur den Effekt, egal wie stark.
    pub(crate) fn tick_fire_alpha(&mut self) {
        let resistent = self.settings.module_dynamic_fire && self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_FIRE_RESISTANCE);
        let ziel = if resistent { 0.0 } else { 1.0 };
        const STEP: f32 = 1.0 / block_anim::FADE_FULL as f32;
        self.fire_alpha = if self.fire_alpha < ziel { (self.fire_alpha + STEP).min(ziel) } else { (self.fire_alpha - STEP).max(ziel) };
    }

    // Der Animations-Thread laedt ab jetzt selbst in den Atlas hoch (s. block_anim::UploadTarget).
    pub(crate) fn connect_block_animations(&mut self) {
        if let Some(gpu) = self.gpu.as_ref() {
            let ziel = block_anim::UploadTarget {
                device: gpu.device.clone(),
                queue: gpu.queue.clone(),
                texture: gpu.block_renderer.atlas_texture_shared(),
            };
            self.block_animations.set_target(ziel);
        }
    }

    pub(crate) fn advance_atlas_animations(&mut self) -> f32 {
        if self.gpu.is_none() {
            return 0.0;
        }
        let tick = (self.block_anim_start.elapsed().as_millis() / 50) as u64;
        let fire_fade = (self.fire_alpha * f32::from(block_anim::FADE_FULL)).round() as u8;
        self.block_animations.request(tick, fire_fade);
        // Animierte Item-Texturen (liegende Items, Items in Haenden) im selben Takt.
        if let Some(gpu) = self.gpu.as_ref() {
            let (queue, renderer) = (&gpu.queue, &gpu.block_renderer);
            self.held_items.tick_animations(tick, &mut |origin, w, h, rgba| renderer.upload_item(queue, origin, w, h, rgba));
            // Und dieselben Bilder im Inventar und in der Hotbar (UI-Atlas).
            self.ui_anim_last.resize(self.ui_atlas.animations.len(), None);
            for (anim, last) in self.ui_atlas.animations.iter().zip(self.ui_anim_last.iter_mut()) {
                let state = anim.state_at(tick);
                if *last == Some(state) {
                    continue;
                }
                *last = Some(state);
                if let Some(pixels) = anim.render(state).first() {
                    let n = anim.tile_size;
                    gpu.queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &gpu.ui_atlas_texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: anim.origin.0, y: anim.origin.1, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        pixels,
                        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(n * 4), rows_per_image: Some(n) },
                        wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
                    );
                }
            }
        }
        let rendered = self.block_animations.drain();
        let mut tiles: Vec<(&[Vec<u8>], (u32, u32), u32)> = rendered.iter().map(|(mips, o, t)| (mips.as_slice(), *o, *t)).collect();

        // Wasser: fertige Frames aus dem Atlas-Bau, nur Mip-Stufe 0 (aus der Ferne faellt der
        // stehende erste Frame nicht auf, s. BlockAtlas::water_animation_frames).
        const WATER_FRAME_MS: f32 = 100.0; // water_still.png.mcmeta "frametime":2 bei 20 TPS
        if let Some(origin) = self.water_tile_origin.filter(|_| self.water_animation_frames.len() > 1) {
            if self.water_anim_last_switch.elapsed().as_secs_f32() * 1000.0 >= WATER_FRAME_MS {
                self.water_anim_last_switch = Instant::now();
                self.water_anim_frame = (self.water_anim_frame + 1) % self.water_animation_frames.len();
                tiles.push((std::slice::from_ref(&self.water_animation_frames[self.water_anim_frame]), origin, self.water_tile_size));
                // Fliesskachel im GLEICHEN Takt. Eigener Modulo, weil water_flow.png nicht
                // zwingend gleich viele Frames hat wie water_still.png.
                if let (Some(flow_origin), false) = (self.water_flow_tile_origin, self.water_flow_animation_frames.is_empty()) {
                    let idx = self.water_anim_frame % self.water_flow_animation_frames.len();
                    tiles.push((std::slice::from_ref(&self.water_flow_animation_frames[idx]), flow_origin, self.water_tile_size));
                }
            }
        }
        let t = Instant::now();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.block_renderer.upload_atlas_tiles(&gpu.device, &gpu.queue, &tiles);
        }
        t.elapsed().as_secs_f32() * 1000.0
    }

    // Frame-Metrik oben rechts. Rechtsbuendig -- die Zahlen aendern sich staendig, ein
    // fester linker Rand wuerde die Anzeige zappeln lassen.
    // Entity-Boxen fuer diesen Frame: jede getrackte Entity als spielergrosse Box
    // (0.6 x 1.8, wie die eigene Hitbox in physics.rs) an ihrer interpolierten Position
    // (s. world::entity-Modulkommentar zur adaptiven Lerp-Dauer). MVP bewusst ohne Typ-
    // Unterscheidung/Modelle -- sichtbar und richtig positioniert schlaegt huebsch (s.
    // Briefing: "funktioniert, auch wenn haesslich" ist akzeptabel).
    // Texturen fuer alles, was gerade in irgendeiner Hand liegt, in den Entity-Atlas holen.
    // Laeuft pro Frame, macht aber nur beim ERSTEN Auftauchen eines Items Arbeit -- danach
    // ist es in held_item::HeldItems vermerkt (auch der Fehlschlag).
    pub(crate) fn ensure_held_item_textures(&mut self) {
        // (Item, Trankfarbe): ein Trank braucht je Farbe eine eigene Textur (s.
        // HeldItems::load_composite).
        let mut gebraucht: Vec<(u32, Option<u32>)> = Vec::new();
        for (_id, e) in self.entities.iter() {
            gebraucht.extend(e.main_hand.iter().chain(e.off_hand.iter()).map(|item| (item.model_id, item.potion_color)));
            // item_display/block_display zeichnen ebenfalls ein Item-Modell und brauchen
            // deshalb denselben Atlas-Platz (s. emit_display_entity).
            if let Some((item, _)) = e.display.as_ref().and_then(Self::display_model) {
                gebraucht.push((item.model_id, item.potion_color));
            }
            // Liegende Items und Wurfgeschosse (s. emit_item_entity) -- ohne empfangenen Stapel
            // die Vorgabe des Typs.
            match (e.item, world::item_render_kind(e.entity_type)) {
                (Some(item), Some(_)) => gebraucht.push((item.model_id, item.potion_color)),
                (None, Some(world::ItemRenderKind::Flying { default_item, .. })) => {
                    gebraucht.extend(world::item_registry::item_id_by_name(default_item).map(|id| (id, None)));
                }
                _ => {}
            }
        }
        // Gegenstandsrahmen: ihr Inhalt (ausser Karten, die eigene Plaetze haben).
        let mut rahmen_modelle: Vec<String> = Vec::new();
        for (_id, e) in self.entities.iter() {
            if !world::entity::is_item_frame(e.entity_type) {
                continue;
            }
            let karte = e.item.is_some_and(|h| h.map_id.is_some());
            if let Some(item) = e.item.filter(|_| !karte) {
                gebraucht.push((item.model_id, item.potion_color));
            }
            let leuchtend = world::entity_type_name(e.entity_type) == Some("glow_item_frame");
            rahmen_modelle.push(format!("block/{}{}", if leuchtend { "glow_item_frame" } else { "item_frame" }, if karte { "_map" } else { "" }));
        }
        // Mehr Animationen: Laternen als Blockmodell in der Hand, Schwerter auf dem Ruecken.
        let anim = self.anim_features();
        for (_, e) in self.entities.iter().filter(|(_, e)| world::is_player(e.entity_type)) {
            if anim & world::more_anims::LANTERN != 0 {
                rahmen_modelle.extend(super::anims::lantern_models([e.main_hand, e.off_hand]));
            }
            if anim & world::more_anims::SWORD != 0 {
                gebraucht.extend(e.last_sword.map(|h| (h.model_id, h.potion_color)));
            }
        }
        if anim & world::more_anims::LANTERN != 0 {
            let (haupt, neben) = self.own_hand_items();
            rahmen_modelle.extend(super::anims::lantern_models([haupt, neben]));
        }
        if anim & world::more_anims::SWORD != 0 {
            gebraucht.extend(self.own_last_sword.map(|h| (h.model_id, h.potion_color)));
        }
        // Der eigene Spieler steht nicht in der Entity-Tabelle (s. own_hand_items), sein
        // Item braucht in der 3. Person aber genauso eine Textur.
        let (own_main, own_off) = self.own_hand_items();
        gebraucht.extend(own_main.iter().chain(own_off.iter()).map(|item| (item.model_id, item.potion_color)));
        // Und der Stapel an der Maus, den die Figur im Inventar haelt (s. doll_hand_items).
        let (doll_main, _) = self.doll_hand_items();
        gebraucht.extend(doll_main.iter().map(|item| (item.model_id, item.potion_color)));
        // Das schwebende Pop-Totem (s. emit_floating_totem) braucht sein Modell auch dann,
        // wenn gerade keines mehr in der Hand liegt -- es wurde ja eben verbraucht.
        if self.floating_totem.is_some() {
            gebraucht.extend(world::item_registry::item_id_by_name("totem_of_undying").map(|id| (id, None)));
        }
        if gebraucht.is_empty() && rahmen_modelle.is_empty() {
            return;
        }
        gebraucht.sort_unstable();
        gebraucht.dedup();
        // Das Schild braucht KEINEN Nachladeplatz: es wird als Entity-Modell mit einer fest
        // im Atlas liegenden Textur gezeichnet (s. emit_shield). Ueber den normalen Weg
        // belegte es einen der 48 Item-Plaetze mit "block/dark_oak_planks" -- der
        // Partikeltextur aus shield.json, die nie jemand zu sehen bekommt.
        gebraucht.retain(|&(id, _)| world::item_registry::item_name(id) != Some("shield"));
        // Zustandsmodelle der eigenen Haende (gespannter Bogen, ladende/geladene Armbrust,
        // Pack-Modelle je Verzauberung): bei kleinen Definitionen ALLE Zweige auf einmal
        // anfordern, nicht nur den gerade gebrauchten -- beim Spannen wechselt er mehrmals je
        // Sekunde, und ein Modell, das erst beim Wechsel geladen wird, fehlt genau in dem Bild,
        // in dem es gebraucht wird. Grosse (Kompass, Uhr) belegten dagegen alle Nachladeplaetze.
        let mut modelle: Vec<String> = Vec::new();
        for hand in [protocol::packets::HAND_MAIN, protocol::packets::HAND_OFF] {
            let Some(stack) = self.own_hand_stack(hand) else { continue };
            let Some(name) = world::item_registry::item_name(stack.item_id) else { continue };
            let Some(def) = self.item_icons.definition(&model_definition_id(stack, name)) else { continue };
            if def.leaves.len() <= HAND_MODEL_PRELOAD_MAX {
                modelle.extend(def.leaves.iter().filter(|l| l.special.is_none() || crate::held_item::special_model(&l.model).is_some()).map(|l| l.model.clone()));
            } else {
                modelle.extend(self.hand_model_of(hand, false, 0.0));
            }
        }
        // Und was die Hand-Items der Entities (und die eigenen in der 3. Person) gerade zeigen.
        let (own_main, own_off) = self.own_hand_items();
        let haende = self.entities.iter().flat_map(|(_, e)| [(e.main_hand, true), (e.off_hand, false)]).chain([(own_main, true), (own_off, false)]);
        let haende: Vec<(Option<world::HeldItem>, bool)> = haende.collect();
        let zusatz: Vec<String> = haende.iter().filter_map(|&(item, rechts)| self.held_model_override(&item?, rechts, false)).collect();
        modelle.extend(zusatz);
        // Der Dreizack wechselt beim Ausholen in die Wurfhaltung (s. held_model_override): beide
        // Haltungen bereit halten, sobald ihn jemand in der Hand hat.
        if haende.iter().any(|(item, _)| item.is_some_and(|h| world::item_registry::item_name(h.item_id) == Some("trident"))) {
            modelle.extend(["item/trident_in_hand", "item/trident_throwing"].map(String::from));
        }
        modelle.extend(rahmen_modelle);
        modelle.sort_unstable();
        modelle.dedup();
        let Some(gpu) = self.gpu.as_ref() else { return };
        for (item_id, trank) in gebraucht {
            let queue = &gpu.queue;
            let renderer = &gpu.block_renderer;
            self.held_items.ensure(item_id, trank, &mut self.installation, &self.entity_atlas, &mut |_slot, origin, w, h, rgba| {
                renderer.upload_item(queue, origin, w, h, rgba);
            });
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        for modell in modelle {
            let queue = &gpu.queue;
            let renderer = &gpu.block_renderer;
            self.held_items.ensure_model(&modell, &mut self.installation, &self.entity_atlas, &mut |_slot, origin, w, h, rgba| {
                renderer.upload_item(queue, origin, w, h, rgba);
            });
        }
    }

    // Die Karten, die dieses Bild zeigt (in Rahmen ohne Spielermarken, in der Hand mit), in die
    // Kartenplaetze bringen: neu gezeichnet, wenn sich ihr Stand geaendert hat; ist kein Platz
    // frei, weicht der am laengsten nicht gebrauchte.
    pub(crate) fn ensure_map_textures(&mut self) {
        self.map_frame_counter += 1;
        let mut gebraucht: Vec<(i32, bool)> = Vec::new();
        let karten_in_haenden = self.anim_features() & world::more_anims::MAP != 0;
        for (_, e) in self.entities.iter() {
            if world::entity::is_item_frame(e.entity_type) {
                if let Some(id) = e.item.and_then(|h| h.map_id) {
                    gebraucht.push((id, true));
                }
            }
            // Mehr Animationen: Karten in fremden Haenden zeigen ihren Inhalt.
            if karten_in_haenden && world::is_player(e.entity_type) {
                gebraucht.extend([e.main_hand, e.off_hand].into_iter().flatten().filter_map(|h| h.map_id).map(|id| (id, false)));
            }
        }
        for hand in [protocol::packets::HAND_MAIN, protocol::packets::HAND_OFF] {
            if let Some(id) = self.own_hand_stack(hand).and_then(|s| s.map_id) {
                gebraucht.push((id, false));
            }
        }
        gebraucht.sort_unstable();
        gebraucht.dedup();
        let anzahl = self.entity_atlas.map_slot_count();
        if self.map_slots.len() != anzahl {
            self.map_slots = vec![None; anzahl];
        }
        for (id, im_rahmen) in gebraucht {
            let Some(stand) = self.maps.get(id) else { continue };
            let revision = stand.revision;
            let jetzt = self.map_frame_counter;
            let platz = if let Some(i) = self.map_slots.iter().position(|p| p.is_some_and(|p| p.id == id && p.im_rahmen == im_rahmen)) { i } else {
                // Frei, sonst der aelteste -- aber keiner, der in diesem Bild schon gebraucht wird.
                let Some(i) = self.map_slots.iter().enumerate().filter(|(_, p)| p.is_none_or(|p| p.zuletzt < jetzt)).min_by_key(|(_, p)| p.map_or(0, |p| p.zuletzt + 1)).map(|(i, _)| i) else { continue };
                self.map_slots[i] = None;
                i
            };
            let aktuell = self.map_slots[platz].is_some_and(|p| p.revision == revision);
            self.map_slots[platz] = Some(MapSlot { id, im_rahmen, revision, zuletzt: jetzt });
            if aktuell {
                continue;
            }
            let installation = &mut self.installation;
            let sprites = &mut self.map_sprites;
            let mut sprite = |name: &str| {
                sprites
                    .entry(name.to_string())
                    .or_insert_with(|| {
                        let t = installation.load_texture(&format!("minecraft/textures/map/decorations/{name}.png")).ok()?;
                        (t.width == t.height && t.width > 0).then_some((t.width, t.rgba))
                    })
                    .clone()
            };
            let Some(stand) = self.maps.get(id) else { continue };
            let bild = world::map::render(stand, im_rahmen, &mut sprite);
            let (Some(origin), Some(gpu)) = (self.entity_atlas.map_slot_origin(platz), self.gpu.as_ref()) else { continue };
            let n = world::map::MAP_SIZE as u32;
            gpu.block_renderer.upload_item(&gpu.queue, origin, n, n, &bild);
        }
    }

    // Das Atlas-Rechteck des Bildes dieser Karte, sobald es geladen ist.
    pub(crate) fn map_rect(&self, id: i32, im_rahmen: bool) -> Option<[f32; 4]> {
        let i = self.map_slots.iter().position(|p| p.is_some_and(|p| p.id == id && p.im_rahmen == im_rahmen))?;
        self.entity_atlas.map_slot_rect(i)
    }

    // Ein Gegenstandsrahmen wie ItemFrameRenderer: an der Wand ausgerichtet, das Rahmenmodell
    // (mit Karte das flache), darin das Item mit seiner FIXED-Haltung auf halber Groesse und um
    // Achtel gedreht -- oder die Karte, die den ganzen Rahmen fuellt (Viertel-Drehungen).
    pub(crate) fn emit_item_frame(&self, e: &world::Entity, feet: [f32; 3], out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_x_deg, rotation_z_deg, Affine};
        let leuchtend = world::entity_type_name(e.entity_type) == Some("glow_item_frame");
        let schritt: [f32; 3] = match e.frame_facing {
            0 => [0.0, -1.0, 0.0],
            1 => [0.0, 1.0, 0.0],
            2 => [0.0, 0.0, -1.0],
            3 => [0.0, 0.0, 1.0],
            4 => [-1.0, 0.0, 0.0],
            _ => [1.0, 0.0, 0.0],
        };
        // Direction.toYRot: Sueden 0, Westen 90, Norden 180, Osten 270.
        // Waagerecht: yRot = 180 - toYRot (Sueden 0, Westen 90, Norden 180, Osten 270); senkrecht
        // um X gekippt.
        let (x_rot, y_rot) = match e.frame_facing {
            0 => (90.0, 180.0),
            1 => (-90.0, 180.0),
            2 => (0.0, 0.0),
            3 => (0.0, 180.0),
            4 => (0.0, 90.0),
            _ => (0.0, -90.0),
        };
        let basis = Affine::translation(feet)
            .then(Affine::translation(schritt.map(|s| s * 0.46875)))
            .then(rotation_x_deg(x_rot))
            .then(Affine::rotation_y_deg(y_rot));
        let karte = e.item.and_then(|h| h.map_id);
        if !e.invisible {
            let modell = format!("block/{}{}", if leuchtend { "glow_item_frame" } else { "item_frame" }, if karte.is_some() { "_map" } else { "" });
            let start = out.len();
            self.held_items.emit_model_raw(&modell, basis.then(Affine::translation([-0.5, -0.5, -0.5])), ENTITY_BRIGHTNESS, out);
            // Eine Karte ohne durchsichtige Pixel deckt die Vorderseite des Bretts ganz ab.
            if karte.and_then(|id| self.maps.get(id)).is_some_and(|m| m.opaque) {
                keep_frame_triangles(out, start, schritt, |vorn| !vorn);
            }
        }
        let Some(item) = e.item else { return };
        let innen = basis.then(Affine::translation([0.0, 0.0, if e.invisible { 0.5 } else { 0.4375 }]));
        if let Some(id) = karte {
            let drehung = (e.frame_rotation.rem_euclid(4) * 2) as f32 * 45.0;
            let m = innen
                .then(rotation_z_deg(drehung))
                .then(rotation_z_deg(180.0))
                .then(Affine::scale([1.0 / 128.0; 3]))
                .then(Affine::translation([-64.0, -64.0, -1.0]));
            if let Some(rect) = self.map_rect(id, true) {
                push_map_quad(m, rect, ENTITY_BRIGHTNESS, out);
            }
        } else {
            let drehung = e.frame_rotation.rem_euclid(8) as f32 * 45.0;
            let m = innen.then(rotation_z_deg(drehung)).then(Affine::scale([0.5; 3]));
            self.held_items.set_skull(self.held_skull_texture(&item));
            let start = out.len();
            self.held_items.emit_fixed(item.model_id, m, ENTITY_BRIGHTNESS, out);
            // Weit weg nur noch die Vorderseite des Items, ohne Kanten und Seitenflaechen.
            let abstand_sq = feet.iter().map(|v| v * v).sum::<f32>();
            if abstand_sq > ITEM_FRAME_LOD_DISTANCE * ITEM_FRAME_LOD_DISTANCE {
                keep_frame_triangles(out, start, schritt, |vorn| vorn);
            }
        }
    }
}

// Ab hier zeichnet ein Rahmen vom Item nur die Vorderseite (in Bloecken).
const ITEM_FRAME_LOD_DISTANCE: f32 = 128.0;

// Behaelt von den Dreiecken ab `start` die, fuer die `behalten` ja sagt. "Vorn" heisst: parallel
// zur Wand und in der vordersten solchen Ebene, gemessen entlang `richtung` (aus der Wand heraus).
// Dreiecke quer zur Wand zaehlen nie als vorn.
fn keep_frame_triangles(out: &mut Vec<world::BlockVertex>, start: usize, richtung: [f32; 3], behalten: impl Fn(bool) -> bool) {
    let r = Vec3::from(richtung);
    let rest: Vec<world::BlockVertex> = out.drain(start..).collect();
    let ebene: Vec<Option<f32>> = rest
        .chunks_exact(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(t[i].position));
            let n = (b - a).cross(c - a);
            let laenge = n.length();
            (laenge > 1e-12 && (n / laenge).dot(r).abs() > 0.99).then(|| ((a + b + c) / 3.0).dot(r))
        })
        .collect();
    let vorderste = ebene.iter().flatten().fold(f32::MIN, |m, &d| m.max(d));
    for (t, d) in rest.chunks_exact(3).zip(&ebene) {
        if behalten(d.is_some_and(|d| d > vorderste - 1e-3)) {
            out.extend_from_slice(t);
        }
    }
    out.extend_from_slice(rest.chunks_exact(3).remainder());
}

#[cfg(test)]
mod frame_cull_tests {
    use super::keep_frame_triangles;

    fn v(p: [f32; 3]) -> world::BlockVertex {
        world::BlockVertex { position: p, color: [1.0; 3], uv: [0.0; 2] }
    }

    // Rahmen schaut nach Sueden (+z): Brett hinten (z 0), Vorderseite (z 0.1), eine Kante quer.
    fn dreiecke() -> Vec<world::BlockVertex> {
        vec![
            v([0.0, 0.0, 0.0]), v([1.0, 0.0, 0.0]), v([0.0, 1.0, 0.0]),
            v([0.0, 0.0, 0.1]), v([0.0, 1.0, 0.1]), v([1.0, 0.0, 0.1]),
            v([0.0, 0.0, 0.0]), v([0.0, 0.0, 0.1]), v([0.0, 1.0, 0.0]),
        ]
    }

    #[test]
    fn nur_vorderseite_bleibt() {
        let mut out = vec![v([9.0; 3])];
        out.extend(dreiecke());
        keep_frame_triangles(&mut out, 1, [0.0, 0.0, 1.0], |vorn| vorn);
        assert_eq!(out.len(), 4);
        assert!(out[1..].iter().all(|p| (p.position[2] - 0.1).abs() < 1e-6));
    }

    #[test]
    fn vorderseite_faellt_weg() {
        let mut out = dreiecke();
        keep_frame_triangles(&mut out, 0, [0.0, 0.0, 1.0], |vorn| !vorn);
        assert_eq!(out.len(), 6);
        assert!(out.iter().all(|p| p.position[2] < 0.1 || p.position[0] == 0.0));
    }
}
