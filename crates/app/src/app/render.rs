// Welt zeichnen: Entities, Ruestung, gehaltene Items, Hitboxen, Himmel, Wetter, Partikel, Schilder.

use crate::*;

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
        let fire_fade = (self.fire_alpha * block_anim::FADE_FULL as f32).round() as u8;
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
                modelle.extend(def.leaves.iter().filter(|l| l.special.is_none()).map(|l| l.model.clone()));
            } else {
                modelle.extend(self.hand_model_of(hand, false, 0.0));
            }
        }
        // Und was die Hand-Items der Entities (und die eigenen in der 3. Person) gerade zeigen.
        let (own_main, own_off) = self.own_hand_items();
        let haende = self.entities.iter().flat_map(|(_, e)| [(e.main_hand, true), (e.off_hand, false)]).chain([(own_main, true), (own_off, false)]);
        let zusatz: Vec<String> = haende.filter_map(|(item, rechts)| self.held_model_override(&item?, rechts)).collect();
        modelle.extend(zusatz);
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
        for (_, e) in self.entities.iter() {
            if world::entity::is_item_frame(e.entity_type) {
                if let Some(id) = e.item.and_then(|h| h.map_id) {
                    gebraucht.push((id, true));
                }
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
            let platz = match self.map_slots.iter().position(|p| p.is_some_and(|p| p.id == id && p.im_rahmen == im_rahmen)) {
                Some(i) => i,
                None => {
                    // Frei, sonst der aelteste -- aber keiner, der in diesem Bild schon gebraucht wird.
                    let Some(i) = self.map_slots.iter().enumerate().filter(|(_, p)| p.is_none_or(|p| p.zuletzt < jetzt)).min_by_key(|(_, p)| p.map_or(0, |p| p.zuletzt + 1)).map(|(i, _)| i) else { continue };
                    self.map_slots[i] = None;
                    i
                }
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
            self.held_items.emit_model_raw(&modell, basis.then(Affine::translation([-0.5, -0.5, -0.5])), ENTITY_BRIGHTNESS, out);
        }
        let Some(item) = e.item else { return };
        let innen = basis.then(Affine::translation([0.0, 0.0, if e.invisible { 0.5 } else { 0.4375 }]));
        match karte {
            Some(id) => {
                let drehung = (e.frame_rotation.rem_euclid(4) * 2) as f32 * 45.0;
                let m = innen
                    .then(rotation_z_deg(drehung))
                    .then(rotation_z_deg(180.0))
                    .then(Affine::scale([1.0 / 128.0; 3]))
                    .then(Affine::translation([-64.0, -64.0, -1.0]));
                if let Some(rect) = self.map_rect(id, true) {
                    push_map_quad(m, rect, ENTITY_BRIGHTNESS, out);
                }
            }
            None => {
                let drehung = e.frame_rotation.rem_euclid(8) as f32 * 45.0;
                let m = innen.then(rotation_z_deg(drehung)).then(Affine::scale([0.5; 3]));
                self.held_items.set_skull(self.held_skull_texture(&item));
                self.held_items.emit_fixed(item.model_id, m, ENTITY_BRIGHTNESS, out);
            }
        }
    }

    // Die Blockarme eines FREMDEN Entities -- SOFORT, sobald er das Schild benutzt, wie in
    // Vanilla. Es gab einmal ein Modul, das das Schild erst nach der 5-Tick-Blockfrist
    // zeigte; entfernt (2026-09-19): man muss sofort sehen, dass der Gegner hochzieht, um
    // reagieren zu koennen.
    pub(crate) fn entity_blocking_arms(&self, e: &world::Entity) -> (bool, bool) {
        blocking_arms(e.using_item_right_arm(), e.arm_items())
    }

    // Dasselbe wie Entity::using_item_right_arm, nur fuer uns selbst: der eigene
    // Benutzungszustand kommt nicht vom Server zurueck, sondern aus der eigenen Buchfuehrung
    // (s. using_item / using_offhand).
    pub(crate) fn own_using_item_right_arm(&self) -> Option<bool> {
        self.using_item.then_some(!self.using_offhand)
    }

    // Was der EIGENE Spieler in den Haenden haelt -- aus dem Inventar statt aus dem
    // Entity-Zustand: fuer sich selbst schickt der Server kein Set Equipment, die Haupthand
    // ist der aktive Hotbar-Slot (dieselbe Zuordnung wie in totem_in_haenden).
    pub(crate) fn own_hand_stack(&self, hand: i32) -> Option<&protocol::item::ItemStack> {
        let slot = if hand == protocol::packets::HAND_OFF {
            inventory::PLAYER_OFFHAND
        } else {
            inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8)
        };
        self.inventory.player.get(slot).and_then(|s| s.as_ref()).filter(|st| st.item_id != 0)
    }

    pub(crate) fn own_hand_items(&self) -> (Option<world::HeldItem>, Option<world::HeldItem>) {
        let von_slot = |slot: usize| -> Option<world::HeldItem> {
            let stack = self.inventory.player.get(slot).and_then(|s| s.as_ref())?;
            (stack.item_id != 0).then(|| world::HeldItem::from_stack(stack))
        };
        (von_slot(inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8)), von_slot(inventory::PLAYER_OFFHAND))
    }

    // Was die Figur im Inventar in den Haenden haelt: wie own_hand_items, nur haengt gerade ein
    // Stapel an der Maus, haelt sie DEN in der Haupthand (ersetzt, was dort liegt).
    pub(crate) fn doll_hand_items(&self) -> (Option<world::HeldItem>, Option<world::HeldItem>) {
        let (main, off) = self.own_hand_items();
        let an_der_maus = self.inventory.cursor.as_ref().filter(|s| s.item_id != 0).map(world::HeldItem::from_stack);
        (an_der_maus.or(main), off)
    }

    // Die eigene Ruestung, in derselben Anordnung wie Entity::armor (nach
    // ArmorSlot::index): Fuesse, Beine, Brust, Kopf.
    //
    // Quelle ist das EIGENE Inventar, nicht das Set-Equipment-Paket -- das schickt der Server
    // an alle ANDEREN, nicht an einen selbst (derselbe Grund wie beim Armschwung, s.
    // ClientEvent::EntityAnimation). Die vier Ruestungsslots des Spielerinventars liegen ab
    // PLAYER_ARMOR_START und sind von oben nach unten sortiert (Helm zuerst), Entity::armor
    // dagegen von unten nach oben -- deshalb der umgekehrte Zugriff.
    pub(crate) fn own_armor(&self) -> [Option<world::HeldItem>; 4] {
        let von_slot = |slot: usize| -> Option<world::HeldItem> {
            let stack = self.inventory.player.get(slot).and_then(|s| s.as_ref())?;
            (stack.item_id != 0).then(|| world::HeldItem::from_stack(stack))
        };
        std::array::from_fn(|i| von_slot(inventory::PLAYER_ARMOR_START + (3 - i)))
    }

    // Getragene Ruestung ueber das Grundmodell legen. `pose` und `base` sind DIESELBEN wie
    // beim Koerper -- nur so folgt der Helm dem Kopf und die Beinschienen den Beinen.
    //
    // Vanillas Reihenfolge (ArmorFeatureRenderer.render) ist Brust, Hose, Schuhe, Kopf. Sie
    // ist hier nicht bloss Geschmack: Brustpanzer und Hose ueberlappen sich am Rumpf, und
    // wer die Hose zuletzt zeichnete, saehe ihren Guertel ueber dem Panzer liegen.
    pub(crate) fn emit_armor(
        &self,
        armor: &[Option<world::HeldItem>; 4],
        models: &[world::Model; 4],
        pose: &world::Pose,
        base: world::entity_model::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) {
        for slot in world::armor::ArmorSlot::ALL {
            let Some(item) = armor[slot.index()] else { continue };
            let Some(name) = world::item_registry::item_name(item.item_id) else { continue };
            // Ein Kopf auf dem Kopf (HeadFeatureRenderer): am Kopfteil, scale(1.1875, -1.1875,
            // -1.1875), translate(-0.5, 0, -0.5), dann das Kopfmodell wie in der Hand.
            if slot == world::armor::ArmorSlot::Head {
                if let Some((tex, hut)) = self.held_skull_texture(&item) {
                    let kopf = world::entity_model::part_transform(&models[slot.index()], pose, base, "head");
                    if let Some(kopf) = kopf {
                        let b = kopf
                            .then(world::entity_model::Affine::scale([1.1875, -1.1875, -1.1875]))
                            .then(world::entity_model::Affine::translation([-0.5, 0.0, -0.5]));
                        held_item::emit_skull_model(tex, hut, b, ENTITY_BRIGHTNESS, out);
                    }
                    continue;
                }
            }
            // Welches Equipment-Asset das Item an DIESEM Slot zeigt -- aus der equippable-
            // Component, sonst Vanillas Standard. Ein Helm im Stiefel-Slot wird nicht als
            // Stiefel gezeichnet (Vanilla prueft dasselbe: `component.slot() == slot`).
            let Some(asset) = world::armor::worn_asset(name, item.equip, slot) else { continue };
            let Some(equipment) = self.equipment_models.get(&asset).and_then(Option::as_ref) else { continue };
            let model = &models[slot.index()];
            // Die Ebenen in Dateireihenfolge (EquipmentLayerRenderer.renderLayers). Eine
            // faerbbare Ebene nimmt die Farbe des Items, sonst ihre Ersatzfarbe; ist beides 0,
            // faellt sie weg -- Vanillas getColorForLayer.
            for ebene in equipment.layers(slot.inner()) {
                let tint = match ebene.dyeable {
                    None => [1.0, 1.0, 1.0],
                    Some(ersatz) => {
                        let c = item.dyed.filter(|&c| c != 0).unwrap_or(ersatz);
                        if c == 0 {
                            continue;
                        }
                        [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0]
                    }
                };
                let Some(tex) = self.equipment_texture(&ebene.texture) else { continue };
                let start = out.len();
                world::entity_model::emit_tinted(model, pose, base, tex, tint, ENTITY_BRIGHTNESS, out);
                self.push_armor_glint(item, model, pose, base, out, start);
            }
            let asset_pfad = asset.split_once(':').map_or(asset.as_str(), |(_, p)| p);
            // Ruestungsbesatz darueber (EquipmentRenderer: die Trim-Ebene nach den Grundebenen,
            // mit demselben Modell).
            if let Some(tex) = self
                .trim_key(&item, slot, asset_pfad)
                .and_then(|k| self.trim_textures.get(&k).copied().flatten())
                .and_then(|platz| self.entity_atlas.trim_slot_rect(platz))
            {
                let start = out.len();
                world::entity_model::emit(model, pose, base, tex, ENTITY_BRIGHTNESS, out);
                self.push_armor_glint(item, model, pose, base, out, start);
            }
        }
    }

    // Schluessel der Trim-Textur eines Ruestungsteils: Ebene (humanoid oder
    // humanoid_leggings), Muster und Palette -- die Palette des Materials, ausser das
    // Material nennt fuer dieses Equipment-Asset (Pfad ohne Namespace, "iron") eine Ausnahme
    // (Eisen-Trim auf Eisenruestung -> iron_darker, sonst verschwaende er darin).
    pub(crate) fn trim_key(&self, item: &world::HeldItem, slot: world::armor::ArmorSlot, asset: &str) -> Option<String> {
        if !self.settings.armor_trims {
            return None;
        }
        let (material, muster) = item.trim?;
        let (palette, ausnahmen) = self.trim_registries.materials.get(usize::try_from(material).ok()?)?;
        let muster = self.trim_registries.patterns.get(usize::try_from(muster).ok()?)?;
        let palette = ausnahmen.get(asset).unwrap_or(palette);
        let ebene = if slot.inner() { "humanoid_leggings" } else { "humanoid" };
        Some(format!("{ebene}/{muster}_{palette}"))
    }

    // Die Trim-Texturen aller sichtbaren Ruestungen erzeugen und hochladen, soweit noch nicht
    // geschehen. Sind alle Plaetze belegt, wird der aelteste verdraengt.
    // Die Windkugel-Textur laeuft in Vanilla durch: RenderTypes.breezeWind verschiebt sie um
    // ageInTicks * 0.03 in u (WindChargeRenderer.xOffset), mit wiederholter Textur. Im Atlas
    // gibt es kein Wiederholen -- deshalb wird die Textur an ihrem Platz selbst verschoben,
    // um ganze Texel wie bei Vanillas Nearest-Abtastung. Ein gemeinsamer Takt fuer alle
    // Windkugeln statt ihres eigenen Alters; nebeneinander faellt das nicht auf.
    pub(crate) fn scroll_wind_charge_texture(&mut self) {
        let fliegt = self.entities.iter().any(|(_, e)| matches!(world::entity_type_name(e.entity_type), Some("wind_charge" | "breeze_wind_charge")));
        if !fliegt {
            return;
        }
        let Some(rect) = self.entity_atlas.rect(world::entity_model::WIND_CHARGE_TEXTURE) else { return };
        let groesse = self.entity_atlas.size as f32;
        let (x0, y0) = ((rect[0] * groesse).round() as u32, (rect[1] * groesse).round() as u32);
        let (w, h) = (((rect[2] - rect[0]) * groesse).round() as u32, ((rect[3] - rect[1]) * groesse).round() as u32);
        if w == 0 || h == 0 {
            return;
        }
        let alter = self.own_age_ticks + self.tick_progress();
        let versatz = (((alter * 0.03) % 1.0) * w as f32) as u32 % w;
        if self.wind_charge_shift == Some(versatz) {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let stride = self.entity_atlas.size as usize * 4;
        let mut pixel = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                // uv' = uv + versatz: an Stelle x steht, was vorher bei x + versatz stand.
                let quelle = (y0 as usize + y) * stride + (x0 as usize + (x + versatz as usize) % w as usize) * 4;
                let ziel = (y * w as usize + x) * 4;
                pixel[ziel..ziel + 4].copy_from_slice(&self.entity_atlas.rgba[quelle..quelle + 4]);
            }
        }
        gpu.block_renderer.upload_item(&gpu.queue, (x0, y0), w, h, &pixel);
        self.wind_charge_shift = Some(versatz);
    }

    // Textur einer Equipment-Ebene ("ns:entity/equipment/<ebene>/<name>"): aus dem Atlas, wenn
    // sie dort steht (Vanilla-Ruestung, auch vom Pack ersetzt), sonst aus den nachgeladenen
    // Plaetzen (s. ensure_trim_textures).
    pub(crate) fn equipment_texture(&self, texture: &str) -> Option<[f32; 4]> {
        if let Some(rect) = texture.strip_prefix("minecraft:").and_then(|p| self.entity_atlas.rect(p)) {
            return Some(rect);
        }
        let platz = self.trim_textures.get(&format!("eq:{texture}")).copied().flatten()?;
        self.entity_atlas.trim_slot_rect(platz)
    }

    // Vor dem Bauen der Vertices: Equipment-Modelle der getragenen Ruestung laden und
    // Texturen nachholen, die nicht im Atlas stehen -- Trims (zusammengesetzt aus Muster und
    // Palette) und Ebenen eigener Equipment-Assets. Beide teilen sich die Trim-Plaetze; ist
    // alles belegt, weicht der am laengsten belegte.
    pub(crate) fn ensure_trim_textures(&mut self) {
        let mut schluessel: Vec<String> = Vec::new();
        let ruestungen = self.entities.iter().map(|(_, e)| e.armor).chain(std::iter::once(self.own_armor())).collect::<Vec<_>>();
        for armor in &ruestungen {
            for slot in world::armor::ArmorSlot::ALL {
                let Some(item) = armor[slot.index()] else { continue };
                let Some(name) = world::item_registry::item_name(item.item_id) else { continue };
                let Some(asset) = world::armor::worn_asset(name, item.equip, slot) else { continue };
                if !self.equipment_models.contains_key(&asset) {
                    let modell = self.installation.load_equipment_model(&asset);
                    self.equipment_models.insert(asset.clone(), modell);
                }
                let Some(Some(equipment)) = self.equipment_models.get(&asset) else { continue };
                for ebene in equipment.layers(slot.inner()) {
                    let im_atlas = ebene.texture.strip_prefix("minecraft:").is_some_and(|p| self.entity_atlas.rect(p).is_some());
                    let k = format!("eq:{}", ebene.texture);
                    if !im_atlas && !self.trim_textures.contains_key(&k) && !schluessel.contains(&k) {
                        schluessel.push(k);
                    }
                }
                let asset_pfad = asset.split_once(':').map_or(asset.as_str(), |(_, p)| p);
                if let Some(k) = self.trim_key(&item, slot, asset_pfad) {
                    if !self.trim_textures.contains_key(&k) && !schluessel.contains(&k) {
                        schluessel.push(k);
                    }
                }
            }
            // Die Fluegel des Brust-Items (s. emit_elytra): eigenes Asset, Ebene "wings".
            let Some(item) = armor[world::armor::ArmorSlot::Chest.index()] else { continue };
            let Some(asset) = world::item_registry::item_name(item.item_id).and_then(|n| world::armor::wings_asset(n, item.equip)) else { continue };
            if !self.equipment_models.contains_key(&asset) {
                let modell = self.installation.load_equipment_model(&asset);
                self.equipment_models.insert(asset.clone(), modell);
            }
            let Some(Some(equipment)) = self.equipment_models.get(&asset) else { continue };
            for ebene in &equipment.wings {
                let im_atlas = ebene.texture.strip_prefix("minecraft:").is_some_and(|p| self.entity_atlas.rect(p).is_some());
                let k = format!("eq:{}", ebene.texture);
                if !im_atlas && !self.trim_textures.contains_key(&k) && !schluessel.contains(&k) {
                    schluessel.push(k);
                }
            }
        }
        for k in schluessel {
            let pixel = if let Some(textur) = k.strip_prefix("eq:") {
                // Equipment-Texturen haben dasselbe 64x32-Raster wie die Trim-Plaetze; ein
                // HD-Pack mit groesseren Bildern bleibt hier aussen vor.
                let (ns, pfad) = textur.split_once(':').unwrap_or(("minecraft", textur));
                self.installation
                    .load_texture(&format!("{ns}/textures/{pfad}.png"))
                    .ok()
                    .filter(|t| t.width == assets::TRIM_WIDTH && t.height == assets::TRIM_HEIGHT)
                    .map(|t| t.rgba)
            } else {
                k.split_once('/')
                    .and_then(|(ebene, rest)| rest.split_once('_').map(|(muster, palette)| (ebene, muster, palette)))
                    .and_then(|(ebene, muster, palette)| assets::build_trim_texture(&mut self.installation, ebene, muster, palette))
            };
            let Some(pixel) = pixel else {
                self.trim_textures.insert(k, None);
                continue;
            };
            let kapazitaet = self.entity_atlas.trim_slot_count();
            if kapazitaet == 0 {
                return;
            }
            let platz = if self.trim_slot_order.len() < kapazitaet {
                self.trim_slot_order.len()
            } else {
                let Some(alt) = self.trim_slot_order.pop_front() else { return };
                self.trim_textures.remove(&alt).flatten().unwrap_or(0)
            };
            self.trim_slot_order.push_back(k.clone());
            if let (Some(gpu), Some(origin)) = (self.gpu.as_ref(), self.entity_atlas.trim_slot_origin(platz)) {
                gpu.block_renderer.upload_cape(&gpu.queue, origin, assets::TRIM_WIDTH, assets::TRIM_HEIGHT, &pixel);
            }
            self.trim_textures.insert(k, Some(platz));
        }
    }

    // Die Fluegel auf dem Ruecken (WingsLayer): fuer jedes Brust-Item, dessen Equipment-Asset
    // eine Ebene "wings" hat -- von Haus aus die Elytra, Server-Packs geben auch anderen Items
    // eigene Fluegel. Vanilla setzt das Modell um 0.125 nach hinten an die Wurzel des Spielermodells -- nicht an
    // den Rumpf, deshalb macht es das Kippen beim Schleichen nicht mit, sondern hat dafuer eigene
    // Winkel (s. world::elytra_pose). Wer einen Umhang hat, traegt dessen Bild auch auf den
    // Fluegeln (SkinTextures.elytraTexture faellt auf den Umhang zurueck; beide Bilder haben
    // dasselbe 64x32-Raster), sofern die Ebene use_player_texture hat. Liefert, ob das Asset
    // Fluegel hat -- dann entfaellt der Umhang (CapeLayer).
    pub(crate) fn emit_elytra(
        &self,
        armor: &[Option<world::HeldItem>; 4],
        cape_tex: Option<[f32; 4]>,
        glide: Option<&world::Glide>,
        sneaking: bool,
        base: world::entity_model::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) -> bool {
        let Some(chest) = armor[world::armor::ArmorSlot::Chest.index()] else { return false };
        let Some(asset) = world::item_registry::item_name(chest.item_id).and_then(|n| world::armor::wings_asset(n, chest.equip)) else {
            return false;
        };
        let Some(Some(equipment)) = self.equipment_models.get(&asset) else { return false };
        if equipment.wings.is_empty() {
            return false;
        }
        let base = base.then(world::entity_model::Affine::translation([0.0, 0.0, 0.125]));
        let pose = world::elytra_pose(glide, sneaking);
        let anfang = out.len();
        for ebene in &equipment.wings {
            let tex = match cape_tex.filter(|_| ebene.use_player_texture) {
                Some(t) => Some(t),
                None => self.equipment_texture(&ebene.texture),
            };
            if let Some(tex) = tex {
                world::emit_entity_model(&world::elytra_model(), &pose, base, tex, ENTITY_BRIGHTNESS, out);
            }
        }
        // Beidseitig wie der End-Kristall: Vanilla zeichnet Equipment mit armorCutoutNoCull.
        // Viele Umhang-Bilder fuellen nur die Rueckseite der Fluegel, die Vorderseite ist
        // durchsichtig -- von vorn sieht man dann die Innenseite der Rueckseite.
        let rueckseiten: Vec<world::BlockVertex> = out[anfang..].chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
        out.extend(rueckseiten);
        true
    }

    // Glint auf einer Ruestungsebene (ARMOR_ENTITY_GLINT). Vanilla legt ihn auf JEDE Ebene
    // eines glitzernden Teils (EquipmentRenderer: ein Glint-Verbraucher je Ebene).
    pub(crate) fn push_armor_glint(
        &self,
        item: world::HeldItem,
        model: &world::entity_model::Model,
        pose: &world::Pose,
        base: world::entity_model::Affine,
        out: &[world::BlockVertex],
        start: usize,
    ) {
        if item.enchanted {
            self.push_glint(out, start, render::glint::GLINT_SCALE_ARMOR, render::glint::GLINT_TEXTURE_ARMOR, |l| {
                world::entity_model::emit(model, pose, base, [0.0, 0.0, 1.0, 1.0], 1.0, l)
            });
        }
    }

    // Item in der Hand eines Entities, an den jeweiligen Arm gehaengt. Vanilla zeichnet beide
    // Haende in HeldItemFeatureRenderer.render -- rechte zuerst, dann die linke.
    //
    // Die zugehoerige ARMHALTUNG setzt apply_item_arm_poses, und zwar vor diesem Aufruf: das
    // Item haengt an der Armtransformation, also muss der Arm schon angehoben sein.
    pub(crate) fn emit_held_items(
        &self,
        // (rechter Arm, linker Arm) -- bei Linkshaendern ist die Haupthand links.
        hands: (Option<world::HeldItem>, Option<world::HeldItem>),
        blocking: (bool, bool),
        // Schwungfortschritt eines Speerstichs der Haupthand (sonst 0) und ob sie der rechte
        // Arm ist (s. world::entity_model::third_person_stab_item_offset).
        stich: (f32, bool),
        model: &world::entity_model::Model,
        pose: &world::Pose,
        base: world::entity_model::Affine,
        out: &mut Vec<world::BlockVertex>,
    ) {
        for (item, right, blockt) in [(hands.0, true, blocking.0), (hands.1, false, blocking.1)] {
            let Some(item) = item else { continue };
            let arm_part = if right { "right_arm" } else { "left_arm" };
            let Some(arm) = world::entity_model::part_transform(model, pose, base, arm_part) else { continue };
            // Das Schild geht einen eigenen Weg -- sein Item-Modell hat gar keine Geometrie
            // (s. world::entity_model::shield_model).
            if world::item_registry::item_name(item.item_id) == Some("shield") {
                self.emit_shield(held_item::hand_pre_transform(arm, right), right, false, blockt, item.enchanted, item.banner, out);
                continue;
            }
            let start = out.len();
            let versatz = if right == stich.1 && stich.0 > 0.0 { world::entity_model::third_person_stab_item_offset(stich.0) } else { world::Affine::IDENTITY };
            let pre = held_item::hand_pre_transform(arm, right).then(versatz);
            // Pack-Modelle je Stapel (Verzauberung, custom_model_data ...), sobald geladen.
            if let Some(modell) = self.held_model_override(&item, right).filter(|m| self.held_items.has_model(m)) {
                self.held_items.emit_model_pre(&modell, pre, right, ENTITY_BRIGHTNESS, false, out);
                if item.enchanted {
                    self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                        self.held_items.emit_model_pre(&modell, pre, right, 1.0, true, l)
                    });
                }
                continue;
            }
            self.held_items.set_skull(self.held_skull_texture(&item));
            self.held_items.emit_pre(item.model_id, item.potion_color, pre, right, ENTITY_BRIGHTNESS, false, out);
            if item.enchanted {
                self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                    self.held_items.emit_pre(item.model_id, item.potion_color, pre, right, 1.0, true, l)
                });
            }
        }
    }

    // Das Schild als Entity-Modell. `pre` ist dieselbe Vorkette wie bei jedem anderen
    // Handgegenstand (held_item::hand_pre_transform bzw. Vanillas Ego-Kette), danach folgt
    // Transformation.apply und zuletzt ShieldModelRenderer.render:
    //
    //     1. verschieben um translation (x links negiert)
    //     2. drehen um rotation
    //     3. skalieren um scale
    //     4. verschieben um (-0.5, -0.5, -0.5)   <- gehoert HIERHIN
    //     5. dann der SpecialModelRenderer: spiegeln (1, -1, -1)
    //
    // ECHTER FEHLER GEWESEN (das Schild schwebte in der Luft): ich hatte den
    // -0.5-Versatz weggelassen, weil ich ihn im Quad-Zweig (submitItem) vermutet hatte -- dort
    // zentriert er ja das 0..1-Wuerfelmodell eines gewoehnlichen Items. Er steht aber in
    // Transformation.apply SELBST, also VOR der Verzweigung in Quads und Spezialmodell, und
    // gilt damit fuer das Schild genauso. Ohne ihn sass es einen halben Block in jeder Achse
    // neben der Hand -- gut einen Meter daneben.
    //
    // Die Spiegelung -1 auf Y und Z hat uebrigens die Determinante +1: die Umlaufrichtung
    // der Dreiecke bleibt erhalten, das Backface-Culling der Pipeline greift also weiterhin
    // richtig.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_shield(
        &self,
        pre: world::entity_model::Affine,
        right_hand: bool,
        first_person: bool,
        blocking: bool,
        glint: bool,
        banner: Option<world::entity::ShieldBanner>,
        out: &mut Vec<world::BlockVertex>,
    ) {
        use world::entity_model::Affine;
        let Some(tex) = self.entity_atlas.rect(self.shield.model.texture) else { return };
        let platte_tex = match banner {
            Some(_) => self.entity_atlas.rect(SHIELD_BANNER_BASE).unwrap_or(tex),
            None => tex,
        };
        let tabelle = match (first_person, blocking) {
            (true, false) => &self.shield.first_person,
            (true, true) => &self.shield.first_person_blocking,
            (false, false) => &self.shield.third_person,
            (false, true) => &self.shield.third_person_blocking,
        };
        // Vanilla nimmt den Eintrag des jeweiligen ItemDisplayContext UND spiegelt danach die
        // x-Verschiebung (Transformation.apply mit leftHanded) -- beides, nicht eines davon.
        let display = if right_hand { tabelle[0] } else { tabelle[1].mirrored() };
        let base = pre
            .then(Affine::translation(display.translation))
            .then(Affine::rotation_xyz_deg(display.rotation[0], display.rotation[1], display.rotation[2]))
            .then(Affine::scale(display.scale))
            .then(Affine::translation([-0.5, -0.5, -0.5]))
            .then(Affine::scale([1.0, -1.0, -1.0]));
        let start = out.len();
        world::emit_entity_model(&self.shield.plate, &world::Pose::new(), base, platte_tex, ENTITY_BRIGHTNESS, out);
        // Die Bannerlagen (BannerBlockEntityRenderer.renderCanvas): erst die Grundfarbe als
        // Lage "base", dann jedes Muster in seiner Farbe -- dieselbe Platte, gleiche Tiefe,
        // die spaetere Lage gewinnt (Tiefentest LessEqual).
        if let Some(banner) = banner {
            let lagen = std::iter::once(("base", banner.base)).chain(
                banner.layers().iter().filter_map(|(id, farbe)| Some((self.trim_registries.banners.get(*id as usize)?.as_str(), *farbe))),
            );
            for (muster, farbe) in lagen {
                if let Some(t) = self.entity_atlas.rect(&format!("entity/shield/{muster}")) {
                    world::entity_model::emit_tinted(&self.shield.plate, &world::Pose::new(), base, t, dye_rgb(farbe), ENTITY_BRIGHTNESS, out);
                }
            }
        }
        // Glint nur auf der Platte, mit der ITEM-Glinttextur (RenderLayers.entityGlint).
        if glint {
            self.push_glint(out, start, render::glint::GLINT_SCALE_SHIELD, render::glint::GLINT_TEXTURE_ITEM, |l| {
                world::emit_entity_model(&self.shield.plate, &world::Pose::new(), base, [0.0, 0.0, 1.0, 1.0], 1.0, l)
            });
        }
        world::emit_entity_model(&self.shield.handle, &world::Pose::new(), base, tex, ENTITY_BRIGHTNESS, out);
    }

    // Schild mit Banner im Inventar: wie set_shield_icon, nur die Platte mit shield_base und
    // den eingefaerbten Bannerlagen darueber (s. emit_shield).
    pub(crate) fn push_shield_banner_icon(&self, sprites: &mut Vec<UiSprite>, banner: world::entity::ShieldBanner, x: f32, y: f32, size: f32) {
        use world::entity_model::Affine;
        let base = Affine::translation([-0.5, -0.5, -0.5]).then(Affine::scale([1.0, -1.0, -1.0]));
        let boxen = |part: &world::Model, tex: [f32; 4]| {
            let mut v = Vec::new();
            world::emit_entity_model(part, &world::Pose::new(), base, tex, 1.0, &mut v);
            v
        };
        let (Some(holz), Some(grund)) = (self.entity_atlas.rect(self.shield.model.texture), self.entity_atlas.rect(SHIELD_BANNER_BASE)) else { return };
        let mut lagen = vec![(vec![boxen(&self.shield.plate, grund), boxen(&self.shield.handle, holz)], [1.0; 3])];
        let muster = std::iter::once(("base", banner.base))
            .chain(banner.layers().iter().filter_map(|(id, farbe)| Some((self.trim_registries.banners.get(*id as usize)?.as_str(), *farbe))));
        for (m, farbe) in muster {
            if let Some(t) = self.entity_atlas.rect(&format!("entity/shield/{m}")) {
                lagen.push((vec![boxen(&self.shield.plate, t)], dye_rgb(farbe)));
            }
        }
        self.item_icons.push_layers(sprites, "shield", &lagen, x, y, size, 1.0);
    }

    // Das Feuer um ein brennendes Entity (Vanillas FireCommandRenderer): uebereinander
    // gestaffelte, zur Kamera gedrehte Flaechen im Wechsel aus fire_0 und fire_1, jede 0.45
    // hoeher, etwas schmaler und etwas weiter hinten als die vorige, bis die Hoehe des Entities
    // erreicht ist. Voll hell (Licht 240) und beidseitig, weil der Entity-Durchgang Rueckseiten
    // verwirft.
    pub(crate) fn emit_entity_fire(&self, feet: [f32; 3], width: f32, height: f32, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::Affine;
        let ticks = self.own_age_ticks.max(0.0) as u64;
        let (Some(r0), Some(r1)) = (
            self.entity_atlas.animated_rect(ENTITY_FIRE_TEXTURES[0], ticks),
            self.entity_atlas.animated_rect(ENTITY_FIRE_TEXTURES[1], ticks),
        ) else {
            return;
        };
        let f = width * 1.4;
        if f <= 0.0 {
            return;
        }
        let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
        // Nur um die Hochachse zur Kamera (MathHelper.rotateAround(Y_AXIS, Kamera)).
        let drehung = affine_from_mat4(display_billboard_matrix(1, 0.0, 0.0, cam_yaw, cam_pitch));
        let mut rest = height / f;
        let basis = Affine::translation(feet)
            .then(Affine::scale([f; 3]))
            .then(drehung)
            .then(Affine::translation([0.0, 0.0, 0.3 - rest.trunc() * 0.02]));
        let (mut g, mut j, mut k) = (0.5f32, 0.0f32, 0.0f32);
        let mut l = 0u32;
        while rest > 0.0 {
            let [mut u0, v0, mut u1, v1] = if l.is_multiple_of(2) { r0 } else { r1 };
            if (l / 2).is_multiple_of(2) {
                std::mem::swap(&mut u0, &mut u1);
            }
            // Vanillas Ecken: (-g,-j) (g,-j) (g,1.4-j) (-g,1.4-j) mit den UVs (u1,v1) (u0,v1)
            // (u0,v0) (u1,v0) -- u0/u1 hier schon nach dem Tausch benannt.
            let ecke = |x: f32, y: f32, u: f32, v: f32| world::BlockVertex { position: basis.apply([x, y, k]), color: [1.0; 3], uv: [u, v] };
            let a = ecke(-g, -j, u1, v1);
            let b = ecke(g, -j, u0, v1);
            let c = ecke(g, 1.4 - j, u0, v0);
            let d = ecke(-g, 1.4 - j, u1, v0);
            out.extend_from_slice(&[a, b, c, a, c, d, a, c, b, a, d, c]);
            rest -= 0.45;
            j -= 0.45;
            g *= 0.9;
            k -= 0.03;
            l += 1;
        }
    }

    // Liegende Items und Wurfgeschosse: das Item-Modell mit seiner GROUND-Haltung.
    //   Dropped (ItemEntityRenderer): Hoehe aus der Modellbox (Unterkante + 1/16 ueber dem
    //     Boden), Wippen sin(Alter/10 + Versatz)*0.1 + 0.1, Drehung Alter/20 + Versatz um Y,
    //     und je nach Stapel 1 bis 5 Kopien -- duenne (flache) Modelle hintereinander, dicke
    //     zufaellig verstreut.
    //   Flying (FlyingItemEntityRenderer): skaliert und zur Kamera gedreht.
    // Der Versatz (uniqueOffset) ist in Vanilla zufaellig je Entity; hier aus der Entity-Id
    // abgeleitet, damit er ueber die Frames gleich bleibt.
    pub(crate) fn emit_item_entity(
        &self,
        id: i32,
        e: &world::Entity,
        art: world::ItemRenderKind,
        feet: [f32; 3],
        progress: f32,
        out: &mut Vec<world::BlockVertex>,
    ) {
        use world::entity_model::Affine;
        let item = match (e.item, art) {
            (Some(item), _) => item,
            (None, world::ItemRenderKind::Flying { default_item, .. }) => {
                let Some(item_id) = world::item_registry::item_id_by_name(default_item) else { return };
                world::HeldItem::plain(item_id)
            }
            // Ein liegendes Item ohne empfangenen Stapel zeigt Vanilla gar nicht (leerer Stack).
            (None, world::ItemRenderKind::Dropped) => return,
        };
        let mut lokal = Vec::new();
        self.held_items.set_skull(self.held_skull_texture(&item));
        // Laeuft das Item ueber den Core-Shader eines Packs, braucht es die Geometrie hier nur
        // fuer seine Masse (Hoehe ueber dem Boden, Abstand der Kopien) -- gezeichnet wird es
        // unten je Platzierung (s. HeldItems::emit_vanilla).
        let umgeleitet = self.held_items.routes_to_vanilla(item.model_id);
        self.held_items.without_vanilla(|| self.held_items.emit_ground(item.model_id, item.potion_color, Affine::IDENTITY, ENTITY_BRIGHTNESS, false, &mut lokal));
        if lokal.is_empty() {
            return;
        }
        let alter = e.age_ticks(progress);
        let platzierungen: Vec<Affine> = match art {
            world::ItemRenderKind::Flying { scale, .. } => {
                // Das eigene, eben geworfene Geschoss saesse sonst die ersten Ticks mitten im
                // Bild (aeltere Vanilla-Fassungen blenden es genauso aus).
                let [x, y, z] = feet;
                if alter < 2.0 && x * x + y * y + z * z < 12.25 {
                    return;
                }
                let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
                let drehung = affine_from_mat4(display_billboard_matrix(3, 0.0, 0.0, cam_yaw, cam_pitch));
                vec![Affine::translation(feet).then(Affine::scale([scale; 3])).then(drehung)]
            }
            world::ItemRenderKind::Dropped => {
                let (mut min_y, mut min_z, mut max_z) = (f32::MAX, f32::MAX, f32::MIN);
                for v in &lokal {
                    min_y = min_y.min(v.position[1]);
                    min_z = min_z.min(v.position[2]);
                    max_z = max_z.max(v.position[2]);
                }
                let versatz = item_unique_offset(id);
                let hoehe = -min_y + 0.0625 + (alter / 10.0 + versatz).sin() * 0.1 + 0.1;
                let basis = Affine::translation(feet)
                    .then(Affine::translation([0.0, hoehe, 0.0]))
                    .then(Affine::rotation_y_deg((alter / 20.0 + versatz).to_degrees()));
                dropped_item_copies(e.item_count, e.item_seed, max_z - min_z).into_iter().map(|p| basis.then(p)).collect()
            }
        };
        if umgeleitet {
            for p in &platzierungen {
                self.held_items.emit_ground(item.model_id, item.potion_color, *p, ENTITY_BRIGHTNESS, false, &mut Vec::new());
            }
            return;
        }
        let start = out.len();
        for p in &platzierungen {
            out.extend(lokal.iter().map(|v| world::BlockVertex { position: p.apply(v.position), ..*v }));
        }
        if item.enchanted {
            self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                let mut uv = Vec::new();
                self.held_items.emit_ground(item.model_id, item.potion_color, Affine::IDENTITY, 1.0, true, &mut uv);
                for p in &platzierungen {
                    l.extend(uv.iter().map(|v| world::BlockVertex { position: p.apply(v.position), ..*v }));
                }
            });
        }
    }

    // Glint-Twins fuer das, was seit `start` in `out` gelandet ist (s. render::glint).
    // `lokal` gibt DIESELBE Geometrie noch einmal mit lokalen UVs aus. Stimmt die Anzahl nicht
    // (ein Modell fehlte beim zweiten Mal), lieber kein Glint als ein verschobener.
    pub(crate) fn push_glint(
        &self,
        out: &[world::BlockVertex],
        start: usize,
        scale: f32,
        texture: f32,
        lokal: impl FnOnce(&mut Vec<world::BlockVertex>),
    ) {
        let mut l = Vec::new();
        lokal(&mut l);
        if l.len() != out.len() - start {
            return;
        }
        render::glint::glint_vertices(&out[start..], &l, scale, texture, &mut self.glint_out.borrow_mut());
    }

    // Ein item_display oder block_display als freistehende Weltgeometrie. `feet` ist die
    // augenrelative Entity-Position; darauf dieselbe Kette wie beim Text-Display
    // (DisplayEntityRenderer.render): Billboard-Drehung, dann die AffineTransformation.
    // Ein item_display dreht danach noch um 180 Grad um Y (ItemDisplayEntityRenderer).
    //
    // text_display kommt hier NICHT vor, s. build_display_text.
    pub(crate) fn emit_display_entity(&self, display: &world::DisplayInfo, age: f32, feet: [f32; 3], yaw: f32, pitch: f32, out: &mut Vec<world::BlockVertex>) {
        // block_display laeuft ueber den Block-Atlas, s. display_block_vertices.
        if matches!(display.content, world::DisplayContent::Block(_)) {
            return;
        }
        let Some((item, _)) = Self::display_model(display) else { return };
        let m = self.display_matrix(display, age, feet, yaw, pitch) * glam::Mat4::from_rotation_y(std::f32::consts::PI);
        self.held_items.set_skull(self.held_skull_texture(&item));
        self.held_items.emit_display(item.model_id, item.potion_color, affine_from_mat4(m), display.item_mode, ENTITY_BRIGHTNESS, out);
    }

    // DisplayEntityRenderer.render: Entity-Position, Billboard-Drehung, AffineTransformation
    // (zum Entity-Alter `age` interpoliert, s. world::DisplayInfo::transform_at).
    pub(crate) fn display_matrix(&self, display: &world::DisplayInfo, age: f32, feet: [f32; 3], yaw: f32, pitch: f32) -> glam::Mat4 {
        let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
        glam::Mat4::from_translation(glam::Vec3::from(feet))
            * display_billboard_matrix(display.billboard, yaw, pitch, cam_yaw, cam_pitch)
            * display_affine_matrix(&display.transform_at(age))
    }

    // Alle block_displays als echte Blockgeometrie im Block-Atlas (augenrelativ, s.
    // world::mesher::block_display_vertices). Sie reisen im Puffer der Geisterbloecke mit, in
    // dessen deckendem Teil -- derselbe Atlas, dieselbe Pipeline, und der wird ohnehin jedes
    // Bild neu gebaut.
    pub(crate) fn display_block_vertices(&self, out: &mut Vec<world::mesher::BlockVertex>) {
        let eye = self.camera.position;
        let progress = self.tick_progress();
        for (_, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            let Some(display) = &e.display else { continue };
            let world::DisplayContent::Block(state) = display.content else { continue };
            if state == world::STATE_AIR {
                continue;
            }
            let (x, y, z) = e.render_pos(progress);
            let v = (DVec3::new(x, y, z) - eye).as_vec3();
            let m = self.display_matrix(display, e.age_ticks(progress), [v.x, v.y, v.z], e.render_yaw(progress), e.render_pitch(progress));
            let start = out.len();
            out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
            for vert in &mut out[start..] {
                vert.position = m.transform_point3(glam::Vec3::from(vert.position)).into();
            }
        }
        // Fallende Bloecke (FallingBlockRenderer): das Blockmodell um einen halben Block nach
        // -x/-z versetzt, denn das Entity steht in der Mitte der Zelle. Steht an seiner Stelle
        // schon derselbe Block (gerade gelandet, das Entfernen ist noch unterwegs), wird er
        // nicht doppelt gezeichnet -- so macht es auch Vanilla.
        for (_, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            let Some(state) = e.falling_block else { continue };
            if state == world::STATE_AIR {
                continue;
            }
            let (x, y, z) = e.render_pos(progress);
            if self.world.block_at(x.floor() as i32, y.floor() as i32, z.floor() as i32) == state {
                continue;
            }
            let v = (DVec3::new(x - 0.5, y, z - 0.5) - eye).as_vec3();
            let start = out.len();
            out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
            for vert in &mut out[start..] {
                vert.position = [vert.position[0] + v.x, vert.position[1] + v.y, vert.position[2] + v.z];
            }
        }
        // Gezuendetes TNT (TntRenderer): der Block um seine Mitte, eine halbe Hoehe ueber den
        // Fuessen. In den letzten zehn Ticks blaeht er sich bis auf 130 % auf, und alle fuenf
        // Ticks blitzt er weiss (TntMinecartRenderer.renderWhiteSolidBlock).
        let tnt_vorgabe = block_place::default_state_for_item("tnt");
        for (_, e) in self.entities.iter() {
            if world::entity_type_name(e.entity_type) != Some("tnt") || !self.entity_in_render_distance(e) {
                continue;
            }
            let Some(state) = e.tnt_block.or(tnt_vorgabe).filter(|s| *s != world::STATE_AIR) else { continue };
            let rest = e.tnt_fuse as f32 - progress + 1.0;
            let groesse = if rest < 10.0 {
                let f = (1.0 - rest / 10.0).clamp(0.0, 1.0);
                1.0 + f * f * f * f * 0.3
            } else {
                1.0
            };
            let blitz = (e.tnt_fuse / 5) % 2 == 0;
            let (x, y, z) = e.render_pos(progress);
            let mitte = (DVec3::new(x, y + 0.5, z) - eye).as_vec3();
            let start = out.len();
            out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
            for vert in &mut out[start..] {
                let p = (glam::Vec3::from(vert.position) - glam::Vec3::splat(0.5)) * groesse + mitte;
                vert.position = p.into();
                if blitz {
                    *vert = vert.with_white_flash();
                }
            }
        }
        // Vom Kolben bewegte Bloecke (PistonHeadRenderer): ihre Zelle zeigt moving_piston, also
        // nichts; der Block selbst faehrt hier zwischen Start- und Zielzelle.
        for m in &self.pistons.moving {
            let versatz = m.offset(progress);
            for (state, faehrt) in m.render_states(progress) {
                let zelle = DVec3::new(m.pos.0 as f64, m.pos.1 as f64, m.pos.2 as f64);
                let v = (zelle + if faehrt { versatz } else { DVec3::ZERO } - eye).as_vec3();
                let start = out.len();
                out.extend(world::mesher::block_display_vertices(state, &self.block_uvs, &self.block_shapes, &self.cross_blocks));
                for vert in &mut out[start..] {
                    vert.position = [vert.position[0] + v.x, vert.position[1] + v.y, vert.position[2] + v.z];
                }
            }
        }
    }

    // (Item-Id, mittig zentriert?) fuer den Inhalt eines Display-Entities.
    //
    // Ein block_display traegt einen BLOCK-State, die Texturen dieses Clients haengen aber am
    // ITEM (s. held_item::HeldItems) -- deshalb der Umweg ueber den Blocknamen. Das ist eine
    // bewusste Naeherung mit bekannter Grenze: gezeichnet wird der Wuerfel des zugehoerigen
    // Item-Modells, nicht die echte Blockform. Ein block_display mit einer Treppe erscheint
    // damit als voller Wuerfel. Der Alternativweg (echte Blockgeometrie aus dem Mesher) liegt
    // im BLOCK-Atlas und waere ein zusaetzlicher Zeichenaufruf pro Display mitten in der
    // Entity-Schleife.
    pub(crate) fn display_model(display: &world::DisplayInfo) -> Option<(world::HeldItem, bool)> {
        match &display.content {
            world::DisplayContent::Item(item) => Some(((*item)?, true)),
            world::DisplayContent::Block(state) => {
                let name = world::block_registry::block_name(*state)?;
                // Luft ist der Vorgabewert eines frisch gespawnten block_display -- und
                // zeichnet nichts.
                if name == "air" {
                    return None;
                }
                Some((world::HeldItem::plain(world::item_registry::item_id_by_name(name)?), false))
            }
            world::DisplayContent::Text(_) => None,
        }
    }

    // Das Schubsen durch andere Lebewesen (Dorfbewohner, Mobs, Spieler), wie es der Vanilla-
    // Client selbst rechnet: jedes Lebewesen fragt in seinem Tick (LivingEntity.tickCramming),
    // ob seine Hitbox die des lokalen Spielers schneidet (ClientWorld.getCrammedEntities
    // liefert dort nur ihn), und stoesst ihn dann weg (Entity.pushAwayFrom): waagerecht vom
    // Schieber weg, je Achse hoechstens 0.05, schwaecher nur auf sehr kurze Entfernung nicht.
    // Vanilla tickt den Spieler vor den Mobs; ihr Stoss landet also in der Geschwindigkeit des
    // NAECHSTEN Spieler-Ticks -- deshalb wird er hier direkt davor addiert.
    //
    // Fehlte ganz: zwischen Dorfbewohnern lief die Grim-Simulation laufend auseinander (Fehler
    // ~0.04, der mit der Bodenreibung 0.546 je Tick auslief -- genau ein fehlender Stoss).
    pub(crate) fn entity_push(&self) -> DVec3 {
        let p = self.physics.pos;
        // LivingEntity.isPushable fuer den Spieler: lebendig, kein Zuschauer, klettert nicht.
        if self.is_dead || self.game_mode == GAME_MODE_SPECTATOR || self.physics.is_climbing(&self.world) {
            return DVec3::ZERO;
        }
        let halb = self.physics.width() / 2.0;
        let (p_min, p_max) = (DVec3::new(p.x - halb, p.y, p.z - halb), DVec3::new(p.x + halb, p.y + self.physics.current_height(), p.z + halb));
        let eigenes_team = self.own_uuid.and_then(|u| self.player_names.get(&u)).and_then(|n| self.sb_entry_team.get(n)).map(String::as_str);
        let regel = |team: Option<&str>| team.and_then(|t| self.sb_teams.get(t)).map_or(0, |t| t.collision_rule);
        let mut schub = DVec3::ZERO;
        for (_, e) in self.entities.iter() {
            if !world::is_living(e.entity_type) || world::entity_type_name(e.entity_type) == Some("armor_stand") {
                continue;
            }
            let (x, y, z) = e.pos;
            let (w, h) = e.box_size();
            let schneidet = x - w / 2.0 < p_max.x && x + w / 2.0 > p_min.x && y < p_max.y && y + h > p_min.y && z - w / 2.0 < p_max.z && z + w / 2.0 > p_min.z;
            if !schneidet {
                continue;
            }
            let eintrag = if world::is_player(e.entity_type) { self.player_names.get(&e.uuid).cloned() } else { Some(uuid_hyphenated(&e.uuid)) };
            let team = eintrag.as_ref().and_then(|n| self.sb_entry_team.get(n)).map(String::as_str);
            if !team_allows_push(team, regel(team), eigenes_team, regel(eigenes_team)) {
                continue;
            }
            let (mut dx, mut dz) = (x - p.x, z - p.z);
            let mut f = dx.abs().max(dz.abs());
            if f < 0.01f32 as f64 {
                continue;
            }
            f = f.sqrt();
            dx /= f;
            dz /= f;
            let g = (1.0 / f).min(1.0);
            // Dieselbe Reihenfolge wie Vanilla: erst g, dann der Faktor (Rundung).
            dx *= g;
            dz *= g;
            dx *= 0.05f32 as f64;
            dz *= 0.05f32 as f64;
            schub.x -= dx;
            schub.z -= dz;
        }
        schub
    }

    // Liegt das Entity innerhalb der Entity-Renderdistanz (Settings::entity_render_distance)?
    // Gemessen von der Kamera aus; gilt fuer alles Gezeichnete, nicht fuers Anvisieren.
    pub(crate) fn entity_in_render_distance(&self, e: &world::Entity) -> bool {
        // Durch wessen Augen man sieht, das zeichnet Vanilla in der Ich-Sicht nicht.
        if self.perspective.is_first_person() && self.spectated_entity().is_some_and(|c| std::ptr::eq(c, e)) {
            return false;
        }
        let (x, y, z) = e.pos;
        // Wie Vanilla nur in gezeichneten Chunks (LevelExtractor.isEntityVisible ->
        // LevelRenderer.isSectionCompiledAndVisible; ausserhalb der Bauhoehe immer). Beim
        // Entladen eines Chunks wirft Vanilla dessen Entities nicht weg, es tickt und zeichnet
        // sie nur nicht mehr. Hier wurden sie weiter gezeichnet, eingefroren an ihrer letzten
        // Stelle, wie Geister-Entities.
        let by = y.floor() as i32;
        let min_y = self.world.min_y();
        if by >= min_y && by < min_y + self.world_height {
            let spalte = ((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16));
            if !self.render_limit_meshed.contains(&spalte) {
                return false;
            }
        }
        let d = self.settings.entity_render_distance;
        if d >= ENTITY_DISTANCE_MAX {
            return true;
        }
        DVec3::new(x, y, z).distance_squared(self.camera.position) <= (d as f64) * (d as f64)
    }

    // Shader-Pfad: Licht an einer Blockposition -- Himmelslicht vom Server, Blocklicht aus dem
    // letzten Mesh der Spalte (s. column_light). (Blocklicht je Kanal, Himmelslicht.)
    pub(crate) fn raw_light_at(&self, x: i32, y: i32, z: i32) -> ([u8; 3], u8) {
        let key = (x.div_euclid(16), z.div_euclid(16));
        let (lx, lz) = (x.rem_euclid(16), z.rem_euclid(16));
        let himmel = self.world.column(key.0, key.1).map_or(15, |c| c.sky_at(lx, y, lz));
        let block = self.column_light.get(&key).map_or([0; 3], |l| l.at(lx, y, lz));
        (block, himmel)
    }

    // Licht an einem Punkt, gepackt wie world::light::pack_vertex_light: je Kanal das hellste
    // der (bis zu acht) Felder, die der Punkt beruehrt, undurchsichtige ausgenommen. So
    // bekommt eine Ecke auf der Blockgrenze (Bett, Truhe, Auge an der Decke) das Licht der
    // freien Seite statt der Dunkelheit im Nachbarblock.
    pub(crate) fn light_near(&self, p: DVec3) -> u32 {
        let mut best: Option<([u8; 3], u8)> = None;
        for i in 0..8 {
            let o = |b: u32| if i >> b & 1 == 1 { 0.02 } else { -0.02 };
            let (x, y, z) = ((p.x + o(0)).floor() as i32, (p.y + o(1)).floor() as i32, (p.z + o(2)).floor() as i32);
            if world::light::opacity(self.world.block_at(x, y, z)) >= 15 {
                continue;
            }
            let (b, h) = self.raw_light_at(x, y, z);
            best = Some(match best {
                None => (b, h),
                Some((bb, hh)) => ([bb[0].max(b[0]), bb[1].max(b[1]), bb[2].max(b[2])], hh.max(h)),
            });
        }
        let (b, h) = best.unwrap_or_else(|| self.raw_light_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32));
        world::light::pack_vertex_light(b.map(|v| v as f32), h as f32)
    }

    // Licht fuer die Vanilla-Vierecke (s. HeldItems::emit_vanilla): je Viereck an seiner Mitte,
    // als UV2 wie Vanillas LightmapTextureManager.pack (Blocklicht * 16, Himmelslicht * 16).
    pub(crate) fn vanilla_item_lights(&self, mut verts: Vec<render::vanilla_draw::VanillaVertex>) -> Vec<render::vanilla_draw::VanillaVertex> {
        let eye = self.camera.position;
        for quad in verts.chunks_mut(4) {
            let mitte = quad.iter().fold(glam::Vec3::ZERO, |a, v| a + glam::Vec3::from(v.position)) / quad.len() as f32;
            let p = eye + mitte.as_dvec3();
            let (block, himmel) = self.raw_light_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let b = block.iter().copied().max().unwrap_or(0).min(15) as i32;
            for v in quad {
                if v.uv2 == [-1, -1] {
                    v.uv2 = [b * 16, himmel.min(15) as i32 * 16];
                }
            }
        }
        verts
    }

    // Core-Shader, die ein Resourcepack ersetzt, uebersetzen und einsetzen (s. render::
    // vanilla_shader). Ohne Ersatz bleibt alles beim eigenen Zeichenweg; scheitert die
    // Uebersetzung, steht der Grund im Log und es bleibt ebenso.
    pub(crate) fn apply_pack_shaders(&mut self) {
        const ITEM: &str = "minecraft/shaders/core/rendertype_item_entity_translucent_cull";
        let ersetzt = self.installation.pack_provides(&format!("{ITEM}.vsh")) || self.installation.pack_provides(&format!("{ITEM}.fsh"));
        let mut meldung = None;
        let program = if ersetzt {
            let installation = &mut self.installation;
            let mut load = |pfad: &str| installation.load_bytes(pfad).ok().and_then(|b| String::from_utf8(b).ok());
            match render::vanilla_shader::compile(ITEM, &mut load) {
                Ok(p) => Some(p),
                Err(e) => {
                    meldung = Some(format!("Pack-Shader {ITEM} nicht nutzbar: {e}"));
                    None
                }
            }
        } else {
            None
        };
        if let Some(gpu) = self.gpu.as_mut() {
            let aktiv = program.is_some();
            match gpu.block_renderer.vanilla.set_item_program(&gpu.device, program) {
                Err(e) => meldung = Some(format!("Pack-Shader {ITEM} nicht nutzbar: {e}")),
                Ok(()) if aktiv => meldung = Some(format!("Pack-Shader aktiv: {ITEM}")),
                Ok(()) => {}
            }
        }
        if let Some(m) = meldung {
            eprintln!("[shader] {m}");
            self.push_log(m);
        }
        self.apply_border_shader();
    }

    // Die Weltgrenze laeuft immer ueber Vanillas Programm rendertype_world_border: aus dem
    // Pack, wenn eins es ersetzt, sonst Vanillas eigenes aus dem Jar -- so sehen Grenzen aus
    // Packs genauso aus wie in Vanilla. Dazu die Kraftfeld-Textur, ebenfalls aus dem Pack.
    pub(crate) fn apply_border_shader(&mut self) {
        const BORDER: &str = "minecraft/shaders/core/rendertype_world_border";
        let installation = &mut self.installation;
        let mut load = |pfad: &str| installation.load_bytes(pfad).ok().and_then(|b| String::from_utf8(b).ok());
        let program = render::vanilla_shader::compile(BORDER, &mut load);
        let textur = self.installation.load_texture("minecraft/textures/misc/forcefield.png");
        let ersetzt = self.installation.pack_provides(&format!("{BORDER}.vsh")) || self.installation.pack_provides(&format!("{BORDER}.fsh"));
        let Some(gpu) = self.gpu.as_mut() else { return };
        let ergebnis = match (program, textur) {
            (Ok(p), Ok(tex)) => gpu.block_renderer.vanilla.set_border_program(&gpu.device, &gpu.queue, Some(p), (tex.width, tex.height, &tex.rgba)),
            (Err(e), _) => Err(e),
            (_, Err(e)) => Err(format!("forcefield.png: {e}")),
        };
        let meldung = match ergebnis {
            Err(e) => Some(format!("Weltgrenze ohne Shader {BORDER}: {e}")),
            Ok(()) if ersetzt => Some(format!("Pack-Shader aktiv: {BORDER}")),
            Ok(()) => None,
        };
        if let Some(m) = meldung {
            eprintln!("[shader] {m}");
            self.push_log(m);
        }
    }

    // Alle Entity-Vertices AUGENRELATIV: sie gehen mit Null-Versatz in den Zeichenaufruf
    // (s. render::BlockRenderer::render), rechnen die Kameraposition also selbst heraus.
    // Die Subtraktion passiert in f64, VOR der f32-Wandlung -- bei absoluten Weltkoordinaten
    // waere die Entity-Position auf einem Server wie DonutSMP schon auf 0.015625 gerastert,
    // bevor sie ueberhaupt im Puffer landet (derselbe Grund wie bei ColumnOrigin).
    // Rueckgabe ist ein PAAR gleich langer Listen: die Vertices und je Vertex die
    // Schadens-Faerbung (rgb + Anteil, s. block.wgsl::vs_entity). Getrennt statt als
    // erweiterter Vertex-Typ, damit die Chunk-Vertices (Millionen davon) nicht 16 Byte
    // breiter werden -- der Tint betrifft nur Entities.
    //
    // Die Faerbung wird nicht je Vertex berechnet, sondern je Entity EINMAL und danach mit
    // `resize` auf alle seither entstandenen Vertices ausgedehnt. Das haelt die Zuordnung
    // auch dann richtig, wenn ein Entity aus mehreren Zeichenschritten besteht (Modell +
    // Handgegenstaende + Umhang).
    // Der dritte Wert: wie viele Vertices am ENDE durchscheinend gezeichnet werden (s.
    // BlockRenderer::set_entity_vertices) -- die Windkugeln.
    // Der zweite Wert daneben: im Shader-Pfad je Vertex das Licht (s. light_near), sonst leer.
    // Die Farbe des Leucht-Umrisses (Entity.getTeamColor): die Farbe des Teams, in dem der
    // Eintrag steht (Spieler mit ihrem Namen, alle anderen mit ihrer UUID), sonst weiss.
    pub(crate) fn glow_color(&self, eintrag: Option<&String>) -> [f32; 4] {
        let farbe = eintrag
            .and_then(|n| self.sb_entry_team.get(n))
            .and_then(|t| self.sb_teams.get(t))
            .and_then(|t| protocol::nbt::formatting_color(t.color))
            .unwrap_or(0xFF_FF_FF);
        let kanal = |s: u32| ((farbe >> s) & 0xFF) as f32 / 255.0;
        [kanal(16), kanal(8), kanal(0), 1.0]
    }

    pub(crate) fn entity_glow_color(&self, e: &world::Entity) -> [f32; 4] {
        let eintrag = if world::is_player(e.entity_type) { self.player_names.get(&e.uuid).cloned() } else { Some(uuid_hyphenated(&e.uuid)) };
        self.glow_color(eintrag.as_ref())
    }

    // Rueckgabe zusaetzlich zu Vertices, Faerbung, Licht und Zahl der durchscheinenden: die
    // Silhouetten leuchtender Entities fuer den Umriss (Vertices und je Vertex die Farbe, s.
    // render::entity_outline). Unsichtbare, die leuchten, stehen NUR dort -- in Vanilla sieht
    // man von ihnen den Umriss, aber keinen Koerper.
    #[allow(clippy::type_complexity)]
    pub(crate) fn build_entity_vertices(&self) -> (Vec<world::BlockVertex>, Vec<[f32; 4]>, Vec<u32>, usize, (Vec<world::BlockVertex>, Vec<[f32; 4]>)) {
        // Anteil 0 = Textur unveraendert (s. fs_entity).
        const KEINE_FAERBUNG: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
        // Shader-Pfad: Licht je Vertex, parallel zu `tints` gefuellt. Ein Wert je Entity (an
        // seinen Augen, wie Vanilla), Blockentities je Vertex, Feuer und Spuren unbeleuchtet.
        const UNBELEUCHTET: u32 = 1 << 31;
        let lit = self.settings.render_path == 1;
        let mut lights: Vec<u32> = Vec::new();
        let tint = self.settings.damage_tint_rgba();
        // Die zweite Faerbung fuer kritische Treffer. None = Modul aus, dann bleibt es bei der
        // einen Farbe fuer alles (s. CRIT_COLOR_ROW).
        let crit_tint = self.settings.module_crit_color_enabled.then(|| self.settings.crit_color_rgba());
        let blitz_ticks = self.settings.damage_tint_ticks;
        // Vertex-Bereiche der Koerpermodelle (Skin). Vanilla faerbt nur sie: Ruestung, Items,
        // Schild und Umhang zeichnet es ohne Schadens-Overlay. Mit "Nur Skin" wird am Ende
        // alles ausserhalb dieser Bereiche wieder entfaerbt.
        let mut haut: Vec<std::ops::Range<usize>> = Vec::new();
        // Brennende Entities: (Fuesse, Breite, Hoehe). Das Feuer kommt erst nach der Schleife
        // dran -- Vanilla zeichnet es ohne Schadens-Overlay, es darf also nie mitblitzen.
        let mut feuer: Vec<([f32; 3], f32, f32)> = Vec::new();
        let eye = self.camera.position;
        // Weltposition -> augenrelative f32-Position fuers Modell.
        let rel = |p: DVec3| -> [f32; 3] {
            let v = (p - eye).as_vec3();
            [v.x, v.y, v.z]
        };
        // So gross wie im letzten Frame: ein Spielermodell allein hat Hunderte Vertices, die Liste
        // wuchs sonst jeden Frame Stueck fuer Stueck (Profiler: realloc).
        let mut out = Vec::with_capacity(((self.entities.len() + 1) * 36).max(self.entity_vertex_hint));
        let mut tints: Vec<[f32; 4]> = Vec::with_capacity(out.capacity());
        // Windkugeln: Vanilla zeichnet sie mit entityTranslucent, ihre Textur hat durchweg
        // Alpha 127. Im normalen Entity-Pass (Schnitt bei 0.5) verschwand sie damit ganz.
        // Sie kommen ans Ende und laufen durchscheinend.
        let mut durchscheinend: Vec<world::BlockVertex> = Vec::new();
        // Leuchtende: Bereiche in `out` mit ihrer Umrissfarbe, und was nur als Umriss erscheint.
        let mut umriss: Vec<(std::ops::Range<usize>, [f32; 4])> = Vec::new();
        let mut nur_umriss: Vec<world::BlockVertex> = Vec::new();
        let mut nur_umriss_farbe: Vec<[f32; 4]> = Vec::new();

        // Eigener Spieler -- NUR in der 3. Person. Im Ego-Modus saesse die Kamera mitten in
        // dieser Box und man saehe sie von innen. Ohne sie waere die 3.-Person-Ansicht
        // sinnlos: man schaut auf die Stelle, an der man steht, und dort ist nichts.
        // Ein Kasten wie bei allen anderen Entities auch (dieser Client zeichnet keine
        // Spielermodelle), nur in Blau, damit man sich von Fremden unterscheidet.
        if !self.perspective.is_first_person() {
            let feet = self.physics_render_prev.lerp(self.physics.pos, (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0) as f64);
            // Der eigene Spieler mit echtem Modell -- Gierung ist die Kamera-Gierung, die
            // Schrittamplitude kommt aus der eigenen Horizontalgeschwindigkeit (die kennen
            // wir hier exakt, anders als bei fremden Entities).
            let pitch = self.look_pitch_deg();
            // Bruchteil des laufenden Ticks -- dieselbe Groesse, mit der auch die Position
            // interpoliert wird, damit Schritt und Bewegung zusammenpassen.
            let tick_progress = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0);
            let (swing, amount) = (self.own_limbs.animation_progress(tick_progress), self.own_limbs.amplitude(tick_progress));
            // Der Kopf bekommt die Drehung RELATIV zum Koerper (Vanilla:
            // LivingEntityRenderer.updateRenderState). Vorher stand hier 0.0 -- der Kopf
            // schaute damit immer starr in Koerperrichtung, und weil der Koerper direkt an der
            // Maus hing, gab es die Vanilla-Traegheit gar nicht.
            let (own_main, own_off) = self.own_hand_items();
            let (block_right, block_left) = blocking_arms(self.own_using_item_right_arm(), (own_main, own_off));
            let glide = self.physics.gliding.then_some(world::Glide {
                ticks: self.own_gliding_ticks as f32 + tick_progress,
                pitch_deg: pitch,
                yaw_deg: self.camera.yaw,
                velocity: [self.physics.vel.x as f32, self.physics.vel.y as f32, self.physics.vel.z as f32],
            });
            // LivingEntityRenderer.updateRenderState: wer sitzt, schwingt keine Beine.
            let reitet = self.own_ride.is_some();
            let (swing, amount) = if reitet { (0.0, 0.0) } else { (swing, amount) };
            let pose = world::entity_model::biped_pose_full(
                self.own_body_yaw.relative_head_yaw(tick_progress),
                pitch,
                swing,
                amount,
                world::entity_model::BipedExtras {
                    riding: reitet,
                    sneaking: self.physics.crouching,
                    lean: self.own_leaning_pitch,
                    age: self.own_age_ticks + tick_progress,
                    // Der eigene Schwung kommt nicht vom Server zurueck (der schickt ihn an
                    // alle ausser uns), sondern aus dem eigenen Zaehler -- derselbe, der auch
                    // den Arm in der Ego-Ansicht bewegt.
                    hand_swing: self.own_hand_swing(tick_progress),
                    swing_right_arm: true,
                    item_right: own_main.is_some(),
                    item_left: own_off.is_some(),
                    block_right,
                    block_left,
                    spear_right: is_spear_item(own_main),
                    spear_left: is_spear_item(own_off),
                    stab: is_spear_item(own_main),
                    glide,
                },
            );
            // Schwimm-/Kriech-Haltung: der ganze Koerper kippt in die Waagerechte (s.
            // world::Lean). Ohne das stand die Figur beim Schwimmen aufrecht im Wasser.
            let lean = world::Lean {
                amount: self.own_leaning_pitch,
                pitch_deg: pitch,
                in_water: self.physics.touching_water,
                // AvatarRenderer: isVisuallySwimming, also die POSE -- auch beim Kriechen an
                // Land. Mit isSwimming fehlte dort der Versatz, die Figur lag zu tief.
                swimming: self.physics.swimming,
            };
            // Der eigene Skin kommt aus derselben Quelle wie der von Fremden: dem Eintrag
            // zur EIGENEN UUID in der Spielerliste. Genau so macht es Vanilla
            // (AbstractClientPlayerEntity.getSkin() -> getPlayerListEntry(getUuid())); ein
            // Sonderweg ueber das Microsoft-Konto waere falsch, weil im Offline-Mode die
            // Konto-UUID nicht der vom Server zugewiesenen entspricht.
            let (model, tex) = self.own_player_model_and_texture();
            // Gedreht wird das MODELL mit der Koerperdrehung, nicht mit der Blickrichtung --
            // sonst dreht sich die ganze Figur samt Beinen sofort mit der Maus mit.
            let body_yaw = self.own_body_yaw.body_yaw(tick_progress);
            let base = match &glide {
                Some(glide) => world::entity_transform_gliding(rel(feet), body_yaw, self.physics.body.scale, world::entity_model::PLAYER_MODEL_SCALE, glide),
                None => world::entity_transform_leaning(rel(feet), body_yaw, self.physics.body.scale, world::entity_model::PLAYER_MODEL_SCALE, &lean),
            };
            let eigener_start = out.len();
            let eigene_umrissfarbe = self.own_glowing.then(|| self.glow_color(self.own_uuid.and_then(|u| self.player_names.get(&u))));
            // Unsichtbar: wie bei Fremden nur Koerper und Umhang weg (s. dort).
            if !self.own_invisible {
                let start = out.len();
                world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
                haut.push(start..out.len());
            } else if let Some(farbe) = eigene_umrissfarbe {
                let vorher = nur_umriss.len();
                world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut nur_umriss);
                nur_umriss_farbe.resize(nur_umriss_farbe.len() + nur_umriss.len() - vorher, farbe);
            }
            if self.own_on_fire && self.game_mode != GAME_MODE_SPECTATOR {
                feuer.push((rel(feet), self.physics.width() as f32, self.physics.current_height() as f32));
            }
            let own_armor = self.own_armor();
            self.emit_armor(&own_armor, &self.armor_models, &pose, base, &mut out);
            let stich = if is_spear_item(own_main) { self.own_hand_swing(tick_progress) } else { 0.0 };
            self.emit_held_items(self.own_hand_items(), (block_right, block_left), (stich, true), model, &pose, base, &mut out);
            let own_cape = self.own_uuid.and_then(|uuid| self.cape_texture(&uuid));
            let elytra = self.emit_elytra(&own_armor, own_cape, glide.as_ref(), self.physics.crouching, base, &mut out);
            // Der Umhang: eigener Zeichenaufruf, weil er eine andere Textur benutzt als der
            // Skin. Dieselbe Einbettung und dieselbe Pose -- er haengt am Rumpf und macht
            // dessen Haltung mit (s. world::cape_model). Mit Elytra nicht: dann haengt er als
            // Bild auf den Fluegeln (s. emit_elytra).
            if let Some(cape_tex) = own_cape.filter(|_| !elytra && !self.own_invisible) {
                let (vertical, swing, side) = world::cape_angles(
                    &self.own_cape,
                    // f64 und absolut -- cape_angles braucht nur die DIFFERENZ der beiden
                    // Positionen, und die ist in f64 ohnehin exakt.
                    [self.physics.pos.x, self.physics.pos.y, self.physics.pos.z],
                    [self.physics_render_prev.x, self.physics_render_prev.y, self.physics_render_prev.z],
                    // KOERPERdrehung, nicht Blickrichtung: Vanillas updateCape rechnet mit
                    // lerpAngleDegrees(..., lastBodyYaw, bodyYaw). Der Umhang haengt am Rumpf,
                    // also muss auch seine Seitwaerts-/Wehrichtung am Rumpf haengen -- mit der
                    // Blickrichtung wuerde er beim Umschauen im Stand zur Seite schlagen,
                    // obwohl sich der Koerper gar nicht bewegt.
                    self.own_body_yaw.body_yaw(tick_progress),
                    tick_progress,
                );
                let cape_pose = world::cape_pose(&pose, vertical, swing, side);
                world::emit_entity_model(&self.cape_model, &cape_pose, base, cape_tex, ENTITY_BRIGHTNESS, &mut out);
            }
            // Der eigene Spieler blitzt genauso auf -- sichtbar nur in der 3. Person, aber
            // dort waere sein Fehlen sofort auffaellig. Die Quelle ist eine andere als bei
            // Fremden: fuer den eigenen Spieler schickt der Server KEIN Damage-Paket
            // (ServerWorld sendet es nur an die anderen Beobachter), Vanilla leitet den
            // Treffer stattdessen aus dem Sinken der eigenen Gesundheit ab -- s.
            // ClientPlayerEntity.updateHealth und App::own_hurt_ticks.
            let eigene_faerbung = match (self.own_ticks_since_hurt < blitz_ticks, crit_tint) {
                (false, _) => KEINE_FAERBUNG,
                (true, Some(crit)) if self.own_hurt_by_crit => crit,
                (true, _) => tint,
            };
            tints.resize(out.len(), eigene_faerbung);
            if lit {
                lights.resize(out.len(), self.light_near(feet + DVec3::new(0.0, self.physics.current_height() * 0.9, 0.0)));
            }
            if let Some(farbe) = eigene_umrissfarbe {
                umriss.push((eigener_start..out.len(), farbe));
            }
        }

        // Frueher stand hier ein `if self.entities.is_empty() { return out; }`. Das war ein
        // BUG: der Ausstieg uebersprang auch emit_beds() weiter unten -- ohne ein einziges
        // Entity in Reichweite verschwanden alle Betten. Gespart hat er nichts, die Schleife
        // ueber eine leere Tabelle tut ohnehin nichts.
        // Spieler mit gehobenem Schild (Schild-Modul): ihr ganzer Vertex-Bereich bekommt den
        // Farbfilter, soweit dort nicht schon ein Schadensblitz liegt (s. unten).
        let mut schild_bereiche: Vec<(std::ops::Range<usize>, [f32; 4])> = Vec::new();
        // Die Glint-Zwillinge dieser Spieler (s. glint_out): bei voller Deckkraft fallen sie
        // weg -- der Glanz wird ADDITIV ueber das fertige Modell gelegt und liess den Spieler
        // sonst trotz 100 % nicht ganz in der Filterfarbe erscheinen.
        let mut schild_glint: Vec<std::ops::Range<usize>> = Vec::new();
        for (_id, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            let entity_start = out.len();
            let glint_start = self.glint_out.borrow().len();
            // Ein einziger Bruchteil fuer ALLES an diesem Entity: Position, Rotation, Schritt,
            // Alter und Umhang muessen mit demselben Wert interpolieren, sonst laufen sie
            // gegeneinander. Seit die Entity-Glaettung dem Vanilla-Modell folgt, ist das der
            // ganz normale Tick-Fortschritt des Clients (s. world::Entity::render_pos).
            let progress = self.tick_progress();
            let (x, y, z) = e.render_pos(progress);
            let feet = rel(DVec3::new(x, y, z));
            let body_yaw = e.render_yaw(progress);
            let head_rel = e.relative_head_yaw(progress);
            let pitch = e.render_pitch(progress);
            let (swing, amount) = e.limb_swing(progress);
            // Display-Entities zuerst: sie haben kein Modell und keine Trefferbox, sondern
            // zeigen einen Gegenstand, einen Blockzustand oder Text. Text wird NICHT hier
            // gezeichnet (er ist Bildschirm-Geometrie, s. build_world_labels), die beiden
            // anderen schon.
            //
            // Ohne diesen Zweig fielen sie in den "kein Modell"-Fall ganz unten und stuenden
            // als orangefarbene, spielergrosse Kaesten in der Welt.
            if let Some(display) = &e.display {
                self.emit_display_entity(display, e.age_ticks(progress), feet, body_yaw, pitch, &mut out);
            }
            // Ein unsichtbares Entity (Entity.FLAGS Bit 5) bekommt kein Modell. Das ist der
            // Normalfall bei Hologrammen: ein unsichtbarer Armorstand traegt den Namen. Sein
            // NAMENSSCHILD zeichnet build_world_labels trotzdem -- genau wie Vanilla, das
            // renderLabelIfPresent unabhaengig von der Sichtbarkeit aufruft.
            //
            // Spieler zuerst: eigener Skin-Platz und eigenes Modell (wide/slim).
            //
            // Kein `continue` mehr in diesem Zweig, sondern if/else: am Schleifenende muss
            // die Faerbung dieses Entities auf ALLE seine Vertices ausgedehnt werden, und
            // ein Ausstieg mittendrin haette genau die Spieler ausgelassen.
            //
            // Unsichtbare SPIELER laufen trotzdem in ihren Zweig: Vanilla blendet nur den
            // Koerper (und den Umhang) aus, Ruestung, Elytra und gehaltene Items bleiben
            // sichtbar (die Feature-Renderer pruefen die Unsichtbarkeit nicht).
            else if world::entity_type_name(e.entity_type) == Some("armor_stand") {
                self.emit_armor_stand(e, feet, body_yaw, &mut out, &mut haut);
            } else if e.invisible && !is_player_like(e.entity_type) && !world::entity::is_item_frame(e.entity_type) {
                // nichts zeichnen (ein unsichtbarer Rahmen zeigt seinen Inhalt weiter)
            } else if matches!(world::entity_type_name(e.entity_type), Some("interaction" | "marker" | "area_effect_cloud")) {
                // Unsichtbar auch in Vanilla (kein Modell, nur Klickflaeche bzw. Datentraeger).
                // Sonst stehen sie (etwa auf mcpvp.club) als orange Kaesten neben den NPCs.
            } else if world::entity_type_name(e.entity_type) == Some("experience_orb") {
                // Als Bild bei den Partikeln, s. experience_orb_vertices.
            } else if e.falling_block.is_some() || world::entity_type_name(e.entity_type) == Some("tnt") {
                // Als echter Block im Block-Atlas, s. display_block_vertices.
            } else if is_player_like(e.entity_type) && self.dying.contains_key(_id) && matches!(self.settings.death_animation, DEATH_ANIM_OFF | DEATH_ANIM_EXPLOSION) {
                // Todesanimation Aus oder Explosion: die Leiche wird nicht gezeichnet (die
                // Teile der Explosion zeichnet die Schleife unten).
            } else if is_player_like(e.entity_type) {
                // ECHTER BUG gewesen: hier stand `false` fest -- ein fremder Spieler blieb
                // beim Schleichen kerzengerade stehen, obwohl die Pose vom Server ankam und
                // biped_pose sie auch auswerten kann. Sichtbar wurde es erst im Duell.
                // Die eigene Figur hatte den Zustand die ganze Zeit (s. physics.crouching
                // weiter oben) -- die beiden Aufrufstellen waren auseinandergelaufen.
                // ECHTER BUG gewesen (fremde Spieler schwangen sichtbar nicht): hier gab es den Handschlag gar nicht. Er folgt aus keiner
                // Groesse, die man mitbekommt -- er kommt als eigenes Paket (s.
                // packets::CB_PLAY_ANIMATE) und muss von dort bis hierher durchgereicht
                // werden.
                let (hand_swing, swing_right_arm) = e.hand_swing(progress);
                // Blockt der Gegner gerade? Das steht in seinem LIVING_FLAGS-Byte (s.
                // Entity::using_item_right_arm) -- die wichtigste Auskunft im Nahkampf.
                let (block_right, block_left) = self.entity_blocking_arms(e);
                // Was welcher Arm haelt: bei Linkshaendern die Haupthand links (s.
                // Entity::left_handed).
                let (arm_rechts, arm_links) = e.arm_items();
                let (speer_rechts, speer_links) = e.arm_spears();
                // Elytra-Gleitflug: ECHTER BUG gewesen -- fliegende Spieler sahen falsch aus.
                // Der Flug-Zustand kam
                // zwar vom Server an (Entity.FLAGS Bit 7), die Figur stand aber aufrecht in der
                // Luft und hatte keine Fluegel.
                let glide = e.glide(progress);
                // Schwimm-/Kriechhaltung: beim Sprint-Schwimmen standen fremde Spieler standen dabei aufrecht,
                // weil nur die eigene Figur die Neigung bekam.
                let lean = e.lean(progress);
                let reitet = self.entities.is_riding(*_id);
                let (swing, amount) = if reitet { (0.0, 0.0) } else { (swing, amount) };
                let pose = world::entity_model::biped_pose_full(
                    head_rel,
                    pitch,
                    swing,
                    amount,
                    world::entity_model::BipedExtras {
                        riding: reitet,
                        sneaking: e.sneaking(),
                        lean: lean.amount,
                        age: e.age_ticks(progress),
                        hand_swing,
                        swing_right_arm,
                        item_right: arm_rechts.is_some(),
                        item_left: arm_links.is_some(),
                        block_right,
                        block_left,
                        spear_right: speer_rechts,
                        spear_left: speer_links,
                        // Die Schlagart haengt am Haupthand-Item, auch fuer die Nebenhand.
                        stab: e.main_hand_spear(),
                        glide,
                    },
                );
                let (model, tex) = self.player_model_and_texture(&e.uuid);
                let base = match &glide {
                    // Sterbend: umkippen statt Flug oder Neigung (setupRotations prueft den Tod
                    // zuerst).
                    _ if self.death_time(*_id).is_some() => world::entity_model::entity_transform_dying(
                        feet,
                        body_yaw,
                        e.scale,
                        world::entity_model::PLAYER_MODEL_SCALE,
                        self.death_time(*_id).unwrap_or_default(),
                    ),
                    // LivingEntityRenderer: das ganze Modell mal SCALE (s. Entity::scale).
                    Some(glide) => world::entity_transform_gliding(feet, body_yaw, e.scale, world::entity_model::PLAYER_MODEL_SCALE, glide),
                    None => world::entity_transform_leaning(feet, body_yaw, e.scale, world::entity_model::PLAYER_MODEL_SCALE, &lean),
                };
                if !e.invisible {
                    let start = out.len();
                    world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
                    haut.push(start..out.len());
                } else if e.glowing {
                    let vorher = nur_umriss.len();
                    world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut nur_umriss);
                    nur_umriss_farbe.resize(nur_umriss_farbe.len() + nur_umriss.len() - vorher, self.entity_glow_color(e));
                }
                self.emit_armor(&e.armor, &self.armor_models, &pose, base, &mut out);
                // Gestochen wird mit der Haupthand -- an ihrem Arm.
                let haupthand_schwingt = swing_right_arm != e.left_handed;
                let stich = if e.main_hand_spear() && haupthand_schwingt { hand_swing } else { 0.0 };
                self.emit_held_items((arm_rechts, arm_links), (block_right, block_left), (stich, swing_right_arm), model, &pose, base, &mut out);
                let cape = self.cape_texture(&e.uuid);
                let elytra = self.emit_elytra(&e.armor, cape, glide.as_ref(), e.sneaking(), base, &mut out);
                if let Some(cape_tex) = cape.filter(|_| !elytra && !e.invisible) {
                    let (vertical, swing, side) = e.cape_angles(progress);
                    let cape_pose = world::cape_pose(&pose, vertical, swing, side);
                    world::emit_entity_model(&self.cape_model, &cape_pose, base, cape_tex, ENTITY_BRIGHTNESS, &mut out);
                }
            } else if world::entity_type_name(e.entity_type) == Some("end_crystal") {
                self.emit_end_crystal(*_id, e, feet, progress, &mut out);
            } else if matches!(world::entity_type_name(e.entity_type), Some("wind_charge" | "breeze_wind_charge")) {
                if self.projectile_in_render_range(e, feet, 4.0) {
                    self.emit_wind_charge(e, feet, progress, &mut durchscheinend);
                }
            } else if let Some(pfeil_tex) = arrow_texture(e.entity_type) {
                if self.projectile_in_render_range(e, feet, 1.0) {
                    self.emit_arrow(e, feet, progress, pfeil_tex, &mut out);
                }
            } else if let Some(art) = world::item_render_kind(e.entity_type) {
                self.emit_item_entity(*_id, e, art, feet, progress, &mut out);
            } else if world::entity::is_item_frame(e.entity_type) {
                self.emit_item_frame(e, feet, &mut out);
            } else {
                match self.model_for_type(e.entity_type) {
                    Some((model, kind, tex)) => {
                        use world::entity_model::ModelKind;
                        let pose = match kind {
                            ModelKind::Biped | ModelKind::Enderman => {
                                // Mobs schlagen genauso -- ein Zombie holt sichtbar aus,
                                // bevor er trifft, und das ist im Nahkampf die einzige
                                // Vorwarnung, die man bekommt.
                                let (hand_swing, swing_right_arm) = e.hand_swing(progress);
                                let reitet = self.entities.is_riding(*_id);
                                let (swing, amount) = if reitet { (0.0, 0.0) } else { (swing, amount) };
                                world::entity_model::biped_pose_full(
                                    head_rel,
                                    pitch,
                                    swing,
                                    amount,
                                    world::entity_model::BipedExtras {
                                        riding: reitet,
                                        age: e.age_ticks(progress),
                                        hand_swing,
                                        swing_right_arm,
                                        item_right: e.arm_items().0.is_some(),
                                        item_left: e.arm_items().1.is_some(),
                                        block_right: self.entity_blocking_arms(e).0,
                                        block_left: self.entity_blocking_arms(e).1,
                                        ..Default::default()
                                    },
                                )
                            }
                            ModelKind::Quadruped => world::entity_model::quadruped_pose(head_rel, pitch, swing, amount),
                            ModelKind::Creeper => world::entity_model::creeper_pose(head_rel, pitch, swing, amount),
                            ModelKind::Spider => world::entity_model::spider_pose(head_rel, pitch, swing, amount),
                            // Der Schleim wird nicht animiert (Vanilla laesst ihn beim Huepfen
                            // wabbern -- dafuer brauchte es den Sprungzustand vom Server).
                            ModelKind::Slime => world::Pose::new(),
                            ModelKind::Feline => world::entity_model::feline_pose(head_rel, pitch, swing, amount, e.sitting(), e.sneaking()),
                            ModelKind::Turtle => world::entity_model::turtle_pose(head_rel, pitch, swing, amount, !e.is_in_water()),
                        };
                        // Katzen: die Farbe aus der Variante, sonst die Standard-Katze.
                        let tex = match e.cat_variant.and_then(|v| usize::try_from(v).ok()).and_then(|i| self.cat_variants.get(i).copied().flatten()) {
                            Some(pfad) if kind == ModelKind::Feline => self.entity_atlas.rect(pfad).unwrap_or(tex),
                            _ => tex,
                        };
                        let model_scale = world::entity_type_name(e.entity_type).map_or(1.0, world::entity_model::model_scale);
                        // Mobs bekommen 1.0: die 0.9375 sind eine Eigenheit von
                        // PlayerEntityRenderer.scale(), die uebrigen LivingEntityRenderer erben
                        // die leere Standardfassung (kein scale-Aufruf).
                        let mut pose = pose;
                        // Nur noch fuer die Modellarten OHNE eigenen Handschlag-Zweig
                        // (Vierbeiner, Creeper, Spinne): beim Biped steckt die Armhaltung
                        // schon in biped_pose_full, an der richtigen Stelle vor dem Schlag.
                        if !matches!(kind, ModelKind::Biped | ModelKind::Enderman) {
                            apply_item_arm_poses(&mut pose, e.arm_items().0.is_some(), e.arm_items().1.is_some());
                        }
                        let base = match self.death_time(*_id) {
                            Some(t) => world::entity_model::entity_transform_dying(feet, body_yaw, e.scale, model_scale, t),
                            None => world::entity_transform(feet, body_yaw, e.scale, model_scale),
                        };
                        let start = out.len();
                        world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
                        haut.push(start..out.len());
                        // Ruestung nur am Biped: die anderen Modelle haben die Teile gar
                        // nicht, die armor_model erwartet -- ein Ruestungshelm ueber einem
                        // Spinnenkopf waere falsch, nicht bloss unschoen.
                        if matches!(kind, ModelKind::Biped) {
                            self.emit_armor(&e.armor, &self.armor_models, &pose, base, &mut out);
                        }
                        // Auch Mobs tragen etwas (Zombie mit Schaufel, Skelett mit Bogen). Die
                        // Modelle ohne Arme (Spinne, Schleim, ...) haben schlicht kein
                        // "right_arm"-Teil -- part_transform liefert dann None und es passiert
                        // nichts, ganz ohne Sonderfall pro ModelKind.
                        self.emit_held_items(
                            e.arm_items(),
                            self.entity_blocking_arms(e),
                            (0.0, true),
                            model,
                            &pose,
                            base,
                            &mut out,
                        );
                    }
                    // Modell aus der Datentabelle (Dorfbewohner, Tiere, Boote, Loren, ...).
                    None if world::entity_type_name(e.entity_type).and_then(world::mob_catalog::mob_look).is_some() => {
                        let start = out.len();
                        self.emit_catalog_entity(e, feet, body_yaw, head_rel, pitch, (swing, amount), &mut out);
                        haut.push(start..out.len());
                    }
                    // Kein Modell fuer diesen Typ (Items, Pfeile, ...) -- Box wie bisher.
                    // Kleinkram bleibt klein und grau, damit eine spielergrosse Box um ein
                    // Item nicht so tut, als stuende da ein Gegner.
                    None => {
                        let (half_w, height, color) = if !world::is_attackable(e.entity_type) {
                            (0.125f32, 0.25f32, [0.6, 0.6, 0.6])
                        } else {
                            (physics::PLAYER_WIDTH / 2.0, physics::PLAYER_HEIGHT, [1.0, 0.65, 0.15])
                        };
                        let (x, y, z) = (feet[0], feet[1], feet[2]);
                        out.extend(world::box_vertices(
                            [x - half_w, y, z - half_w],
                            [x + half_w, y + height, z + half_w],
                            color,
                            // Die weisse Kachel des ENTITY-Atlas, nicht die des Block-Atlas:
                            // dieser Durchgang bindet den Entity-Atlas (s.
                            // EntityAtlas::white_rect).
                            self.entity_atlas.white_rect(),
                        ));
                    }
                }
            }
            if e.on_fire {
                let (breite, hoehe) = e.hit_size();
                feuer.push((feet, breite as f32, hoehe as f32));
            }
            // Alles, was dieses Entity gerade beigesteuert hat, bekommt seine Faerbung.
            // is_hurt() ist bei Nicht-Lebewesen nie wahr (s. world::Entity::on_damaged), ein
            // brennendes Boot blitzt also nicht -- genau wie in Vanilla.
            //
            // War der Treffer ein KRITISCHER und ist das Modul an, gilt dessen eigene Farbe
            // (s. world::Entity::is_hurt_by_crit). Die Abfrage sitzt bewusst hier und nicht in
            // der Entity: welche Farbe gilt, ist eine Frage der Einstellungen, nicht des
            // Weltzustands.
            // Pfeile-Modul "Weisser Pfeil": die Faerbung mischt nach Textur und Licht voll auf
            // Weiss (s. block.wgsl::fs_entity) -- der Pfeil ist dann eine weisse Silhouette.
            let weisser_pfeil = self.settings.module_arrows && self.settings.arrow_white && arrow_texture(e.entity_type).is_some();
            // Sterbend: rot bis zum Ende (LivingEntityRenderer: hasRedOverlay mit deathTime > 0).
            let faerbung = match (e.damage_flash(blitz_ticks) || self.death_time(*_id).is_some(), crit_tint) {
                _ if weisser_pfeil => [1.0, 1.0, 1.0, 1.0],
                (false, _) => KEINE_FAERBUNG,
                (true, Some(crit)) if e.hurt_by_crit() => crit,
                (true, _) => tint,
            };
            tints.resize(out.len(), faerbung);
            if e.glowing && out.len() > entity_start {
                umriss.push((entity_start..out.len(), self.entity_glow_color(e)));
            }
            if lit {
                let (ex, ey, ez) = e.render_pos(progress);
                let hoehe = e.hit_size().1;
                lights.resize(out.len(), self.light_near(DVec3::new(ex, ey + hoehe * 0.85, ez)));
            }
            // "Nur Skin" loescht spaeter alle Faerbung ausserhalb von `haut` -- der weisse
            // Pfeil zaehlt dafuer als Haut, sonst verschwaende er mit der Einstellung.
            if weisser_pfeil {
                haut.push(entity_start..out.len());
            }
            // Schild-Modul, Anzeige "Modell": Cooldown oder gehobenes Schild faerbt den Spieler.
            if !e.invisible {
                if let Some(filter) = self.shield_mark_color(*_id, e, ShieldMark::Model) {
                    schild_bereiche.push((entity_start..out.len(), filter));
                    // Bei voller Deckkraft faellt der Glanz weg (s. unten).
                    if filter[3] >= 1.0 {
                        schild_glint.push(glint_start..self.glint_out.borrow().len());
                    }
                }
            }
        }
        for (fuesse, breite, hoehe) in feuer {
            self.emit_entity_fire(fuesse, breite, hoehe, &mut out);
        }
        self.emit_trails(eye, self.tick_progress(), &mut out);
        tints.resize(out.len(), KEINE_FAERBUNG);
        if lit {
            lights.resize(out.len(), UNBELEUCHTET);
        }
        for b in &self.death_bursts {
            let start = out.len();
            let (model, tex) = self.player_model_and_texture(&b.uuid);
            let pose = b.pose(self.tick_progress());
            let feet = DVec3::new(b.feet.0, b.feet.1, b.feet.2);
            let base = world::entity_model::entity_transform(rel(feet), b.yaw, b.scale, world::entity_model::PLAYER_MODEL_SCALE);
            world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
            self.emit_armor(&b.armor, &self.armor_models, &pose, base, &mut out);
            haut.push(start..out.len());
            tints.resize(out.len(), KEINE_FAERBUNG);
            if lit {
                lights.resize(out.len(), self.light_near(feet + DVec3::new(0.0, 0.5, 0.0)));
            }
        }
        if lit {
            lights.resize(out.len(), UNBELEUCHTET);
        }
        // Betten sind Blockgeometrie im Entity-Puffer, kein Lebewesen -- nie gefaerbt.
        self.emit_beds(eye, &mut out);
        self.emit_shulker_boxes(eye, &mut out);
        self.emit_chests(eye, &mut out);
        self.emit_skulls(eye, &mut out);
        self.emit_statues(eye, &mut out);
        self.emit_signs(eye, &mut out);
        tints.resize(out.len(), KEINE_FAERBUNG);
        if lit {
            for v in &out[lights.len()..] {
                lights.push(self.light_near(eye + glam::Vec3::from(v.position).as_dvec3()));
            }
        }
        if self.settings.damage_tint_skin_only {
            let mut nur_haut = vec![KEINE_FAERBUNG; tints.len()];
            for bereich in haut {
                nur_haut[bereich.clone()].copy_from_slice(&tints[bereich]);
            }
            tints = nur_haut;
        }
        // Erst nach "Nur Skin": der Filter gilt ausdruecklich fuer den GANZEN Spieler samt
        // Umhang, Ruestung und Items. Wo gerade ein Schadensblitz liegt, bleibt der sichtbar.
        {
            let mut glint = self.glint_out.borrow_mut();
            for bereich in schild_glint.into_iter().rev() {
                glint.drain(bereich);
            }
        }
        for (bereich, filter) in schild_bereiche {
            for t in &mut tints[bereich] {
                if *t == KEINE_FAERBUNG {
                    *t = filter;
                }
            }
        }
        let anzahl = durchscheinend.len();
        out.extend(durchscheinend);
        tints.resize(out.len(), KEINE_FAERBUNG);
        if lit {
            for v in &out[lights.len()..] {
                lights.push(self.light_near(eye + glam::Vec3::from(v.position).as_dvec3()));
            }
        }
        for (bereich, farbe) in umriss {
            nur_umriss_farbe.resize(nur_umriss_farbe.len() + bereich.len(), farbe);
            nur_umriss.extend_from_slice(&out[bereich]);
        }
        (out, tints, lights, anzahl, (nur_umriss, nur_umriss_farbe))
    }

    // Hitbox-Anzeige (Modul, s. Settings::module_hitbox_*): Drahtgitter-Box um jedes
    // angreifbare Fremd-Entity, wahlweise an seiner GEGLAETTETEN Bildposition (render_pos,
    // dieselbe, die build_entity_vertices auch fuers Modell benutzt) und/oder an seiner
    // ROHEN, zuletzt vom Server geschickten Position (target_pos) -- Letztere zeigt, wie weit
    // die Bild-Glaettung dem Server-Stand gerade hinterherhaengt, Erstere ist die Position,
    // die man tatsaechlich vor sich sieht.
    //
    // Welche Entities eine Box bekommen und in welcher Farbe, regeln die eigenen Arten (s.
    // hitbox_kinds); was keine eigene Art hat, nur wenn es angreifbar ist (Mobs, s.
    // world::is_attackable). Dazu Spinnennetze und Wasserquellen als Bloecke.
    //
    // Hier entstehen nur die KANTEN als Endpunkt-Paare; ihre Breite bekommt die Anzeige erst
    // im Vertex-Shader nach der Projektion (s. push_hitbox_edge und block.wgsl::vs_hitbox).
    // Die eingestellte Pixelbreite und die Deckkraft reisen deshalb nur als Werte je Vertex
    // mit (s. render::HitboxVertex) -- der Blockumriss im selben Puffer hat eigene.
    //
    // Die gestrichelte Darstellung teilt die Kanten dagegen SCHON HIER auf: dafuer braucht es
    // eine Weltlaenge, und die haengt (anders als die Breite) sinnvollerweise an der
    // Entfernung des jeweiligen Segments -- ein Strichmuster, das im Bildschirmraum konstant
    // waere, saehe an einer schraeg wegfuehrenden Kante falsch aus.
    // Schild-Modul: welche Markierung (Cooldown vor Hochhalten -- wer im Cooldown steckt, kann
    // ohnehin nicht blocken) dieses Entity in der Darstellung `art` bekommt, mit ihrer Farbe.
    pub(crate) fn shield_mark_color(&self, id: i32, e: &world::Entity, art: ShieldMark) -> Option<[f32; 4]> {
        let set = &self.settings;
        if !set.module_shields || !world::is_player(e.entity_type) {
            return None;
        }
        if set.shield_cd_players && set.shield_mark(ShieldColor::Cooldown) == art && self.shield_cooldowns.contains_key(&id) {
            return Some(set.shield_color_rgba(ShieldColor::Cooldown));
        }
        if set.shield_hold_players && set.shield_mark(ShieldColor::Hold) == art && !e.invisible {
            let (rechts, links) = self.entity_blocking_arms(e);
            if rechts || links {
                return Some(set.shield_color_rgba(ShieldColor::Hold));
            }
        }
        None
    }

    // Traegt gerade irgendein Spieler eine Hitbox-Markierung des Schild-Moduls? Dann wird die
    // Hitbox-Anzeige auch ohne ihr eigenes Modul gezeichnet (s. build_hitbox_vertices).
    pub(crate) fn any_shield_hitbox_mark(&self) -> bool {
        self.entities.iter().any(|(id, e)| self.shield_mark_color(*id, e, ShieldMark::Hitbox).is_some())
    }

    // Farbe des Namensschilds eines Spielers (0xRRGGBB) und woher sie stammt. Der Reihe nach:
    //   1. die Farbe seines Scoreboard-Teams (so faerbt Vanilla den Namen),
    //   2. der Name in der Tab-Liste (Anzeigename aus dem Spielerlisten-Paket): die Farbe
    //      des Abschnitts, der den Spielernamen enthaelt,
    //   3. die letzte Farbe im Team-Prefix (Server, die den Namen nur ueber den Prefix
    //      faerben und die Teamfarbe leer lassen),
    //   4. ein eigenes Namensschild des Servers: ein Text-Display als Passagier, dort der
    //      Abschnitt mit dem Namen, sonst der erste farbige.
    // None = nichts davon.
    pub(crate) fn name_tag_color(&self, uuid: &[u8; 16], entity_id: Option<i32>) -> (Option<u32>, &'static str) {
        let name = self.player_names.get(uuid);
        let team = name.and_then(|n| self.sb_entry_team.get(n)).and_then(|t| self.sb_teams.get(t));
        if let Some(c) = team.and_then(|t| protocol::nbt::formatting_color(t.color)) {
            return (Some(c), "Teamfarbe");
        }
        let tab = self.tab_entries.get(uuid).and_then(|t| t.display_name.as_ref());
        if let Some(c) = tab.zip(name).and_then(|(spans, n)| spans.iter().find(|s| s.text.contains(n.as_str())).and_then(|s| s.color)) {
            return (Some(c), "Tab-Liste");
        }
        if let Some(c) = team.and_then(|t| protocol::nbt::legacy_to_spans(&t.prefix, None).iter().rev().find_map(|s| s.color)) {
            return (Some(c), "Prefix");
        }
        // Was tatsaechlich ueber dem Kopf steht (s. head_labels): der Abschnitt mit dem Namen
        // zaehlt, sonst der erste farbige.
        let Some(name) = name else { return (None, "keine") };
        let mut ersatz = None;
        for spans in self.head_labels(uuid, entity_id) {
            if let Some(c) = spans.iter().find(|s| s.text.contains(name.as_str())).and_then(|s| s.color) {
                return (Some(c), "Beschriftung");
            }
            ersatz = ersatz.or_else(|| spans.iter().find_map(|s| s.color.filter(|c| *c != 0xFFFFFF)));
        }
        if let Some(c) = ersatz {
            return (Some(c), "Beschriftung (Rest)");
        }
        // Die Seitenleiste: Team-Modi listen dort oft die Spieler in ihrer Teamfarbe -- fuer den
        // EIGENEN Namen meist die einzige Quelle, denn ueber dem eigenen Kopf zeigt der Server
        // kein Namensschild.
        if let Some((_, zeilen)) = self.scoreboard_lines() {
            for zeile in zeilen {
                let text: String = zeile.iter().map(|s| s.text.as_str()).collect();
                if !text.contains(name.as_str()) {
                    continue;
                }
                let farbe = zeile
                    .iter()
                    .find(|s| s.text.contains(name.as_str()))
                    .and_then(|s| s.color)
                    .or_else(|| zeile.iter().find_map(|s| s.color.filter(|c| *c != 0xFFFFFF)));
                if let Some(c) = farbe {
                    return (Some(c), "Seitenleiste");
                }
            }
        }
        (None, "keine")
    }

    // Jede Beschriftung ueber dem Kopf eines Spielers, die seinen Namen enthaelt: Spielername,
    // Text-Display, Custom Name eines Ruestungsstaenders ... -- egal woher, was zaehlt, ist was
    // dort steht. Health-Anzeigen ("19 ❤") enthalten den Namen nicht und fallen heraus.
    pub(crate) fn head_labels(&self, uuid: &[u8; 16], entity_id: Option<i32>) -> Vec<Vec<protocol::nbt::TextSpan>> {
        let Some(name) = self.player_names.get(uuid) else { return Vec::new() };
        // Die eigene Figur steht nicht in der Entity-Liste: ihr Fusspunkt kommt aus der Physik.
        let fuss = if Some(*uuid) == self.own_uuid {
            Some((self.physics.pos.x, self.physics.pos.y, self.physics.pos.z))
        } else {
            entity_id.and_then(|id| self.entities.get(id)).map(|e| e.pos)
        };
        let Some(fuss) = fuss else { return Vec::new() };
        let mut out = Vec::new();
        for (lid, le) in self.entities.iter() {
            let Some(label) = self.world_label(*lid, le, 0.0) else { continue };
            let a = label.anchor;
            if (a.x - fuss.0).abs() > 0.8 || (a.z - fuss.2).abs() > 0.8 || a.y < fuss.1 || a.y > fuss.1 + 4.5 {
                continue;
            }
            if label.spans.iter().map(|s| s.text.as_str()).collect::<String>().contains(name.as_str()) {
                out.push(label.spans.into_owned());
            }
        }
        out
    }

    // Alles, woran das Teams-Modul das Team eines Spielers erkennt (s. teams::side_of).
    pub(crate) fn team_facts(&self, uuid: &[u8; 16], entity_id: Option<i32>) -> (teams::Facts, &'static str) {
        let name = self.player_names.get(uuid);
        let team = name.and_then(|n| self.sb_entry_team.get(n)).cloned();
        let (color, quelle) = self.name_tag_color(uuid, entity_id);
        // Der angezeigte Name als Klartext: Team-Prefix + Name, sonst die Tab-Liste, sonst
        // die Beschriftung ueber dem Kopf.
        let klartext = |spans: &[protocol::nbt::TextSpan]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
        let text = team
            .as_ref()
            .and_then(|t| self.sb_teams.get(t))
            .map(|t| klartext(&protocol::nbt::legacy_to_spans(&format!("{}{}", t.prefix, name.map_or("", |n| n.as_str())), None)))
            .or_else(|| self.tab_entries.get(uuid).and_then(|t| t.display_name.as_deref()).map(klartext))
            .or_else(|| self.head_labels(uuid, entity_id).first().map(|l| klartext(l)));
        let prefix = text.zip(name).and_then(|(t, n)| teams::prefix_before(&t, n));
        let ruestung = if Some(*uuid) == self.own_uuid {
            self.own_armor()
        } else {
            entity_id.and_then(|id| self.entities.get(id)).map_or([None; 4], |e| e.armor)
        };
        let armor = ruestung.map(|r| r.and_then(|r| r.dyed));
        (teams::Facts { team, color, prefix, armor }, quelle)
    }

    // Die eigene Figur als moegliches Fahrzeug an die Entity-Liste melden (s.
    // Entities::set_own_body): Namensschild-Displays auf dem eigenen Kopf laufen sonst nicht mit.
    pub(crate) fn sync_own_body(&mut self) {
        let Some(id) = self.own_entity_id else { return };
        let p = self.physics.pos;
        self.entities.set_own_body(id, (p.x, p.y, p.z), self.camera.yaw, self.physics.current_height());
    }

    // Vorschlaege fuer die eigene Teamfarbe: die Namensfarben der anderen Spieler, haeufigste
    // zuerst, ohne Weiss. Eine schon gewaehlte bleibt dabei, auch wenn sie gerade niemand traegt.
    pub(crate) fn team_color_suggestions(&self) -> Vec<u32> {
        let mut zaehler: Vec<(u32, usize)> = Vec::new();
        for (uuid, f) in &self.team_facts {
            let Some(c) = f.color.filter(|c| *c != 0xFFFFFF) else { continue };
            if Some(*uuid) == self.own_uuid {
                continue;
            }
            match zaehler.iter_mut().find(|(z, _)| *z == c) {
                Some(z) => z.1 += 1,
                None => zaehler.push((c, 1)),
            }
        }
        zaehler.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut out: Vec<u32> = zaehler.into_iter().map(|(c, _)| c).collect();
        if let Some(c) = self.settings.team_own_color.filter(|c| !out.contains(c)) {
            out.insert(0, c);
        }
        out.truncate(COLOR_PICK_MAX);
        out
    }

    // Teams-Modul: die Merkmale aller Spieler einmal pro Tick (das Zeichnen liest nur
    // team_facts), und jede Aenderung ins Log -- woran die Erkennung haengt, laesst sich nur
    // am echten Server pruefen.
    pub(crate) fn update_team_facts(&mut self) {
        self.team_facts.clear();
        if !self.settings.module_teams {
            return;
        }
        let eigene = match self.own_uuid {
            Some(u) => vec![(u, self.own_entity_id)],
            None => Vec::new(),
        };
        let spieler: Vec<([u8; 16], Option<i32>)> = eigene
            .into_iter()
            .chain(self.entities.iter().filter(|(_, e)| world::is_player(e.entity_type)).map(|(id, e)| (e.uuid, Some(*id))))
            .collect();
        for (uuid, id) in spieler {
            let (mut fakten, mut quelle) = self.team_facts(&uuid, id);
            if let (true, Some(c)) = (Some(uuid) == self.own_uuid, self.settings.team_own_color) {
                fakten.color = Some(c);
                quelle = "eingestellt";
            }
            match fakten.color {
                Some(c) => {
                    self.team_color_memory.insert(uuid, c);
                }
                None => {
                    if let Some(&c) = self.team_color_memory.get(&uuid) {
                        fakten.color = Some(c);
                        quelle = "gemerkt";
                    }
                }
            }
            let name = self.player_names.get(&uuid).cloned().unwrap_or_else(|| "?".to_string());
            let eigen = if Some(uuid) == self.own_uuid { " (du)" } else { "" };
            let hex = |c: Option<u32>| c.map_or("-".to_string(), |c| format!("#{c:06X}"));
            let zeile = format!(
                "[teams] {name}{eigen}: Name {} ({quelle}) | Team {} | Prefix {} | Ruestung {}",
                hex(fakten.color),
                fakten.team.as_deref().unwrap_or("-"),
                fakten.prefix.as_deref().unwrap_or("-"),
                fakten.armor.map(hex).join(" ")
            );
            if self.team_color_log.get(&uuid) != Some(&zeile) {
                eprintln!("{zeile}");
                self.team_color_log.insert(uuid, zeile);
            }
            self.team_facts.insert(uuid, fakten);
        }
    }

    // Wie das Teams-Modul die Box dieses Spielers zeichnet: Farbe, Deckkraft (0..1), Dichte
    // in Pixeln und gepunktet -- oder None, wenn seine Seite nicht ueberschrieben wird.
    pub(crate) fn team_hitbox_style(&self, e: &world::Entity) -> Option<([f32; 3], f32, u32, bool)> {
        if !world::is_player(e.entity_type) || Some(e.uuid) == self.own_uuid {
            return None;
        }
        let leer = teams::Facts::default();
        let eigene = self.own_uuid.and_then(|u| self.team_facts.get(&u)).unwrap_or(&leer);
        let fremde = self.team_facts.get(&e.uuid).unwrap_or(&leer);
        self.team_style_of(teams::side_of(&self.settings.team_match, eigene, fremde), fremde.display_color())
    }

    // Stil einer Seite fuer einen Spieler mit der Namensfarbe `name_color` (ohne Farbe weiss,
    // wie ein ungefaerbter Name). None = Modul aus oder die Seite wird nicht ueberschrieben.
    pub(crate) fn team_style_of(&self, seite: teams::Side, name_color: Option<u32>) -> Option<([f32; 3], f32, u32, bool)> {
        let set = &self.settings;
        let st = set.team_hitboxes[seite.index()];
        if !set.module_teams || !st.on {
            return None;
        }
        let farbe = if st.nametag {
            let c = name_color.unwrap_or(0xFFFFFF);
            [((c >> 16) & 0xFF) as f32 / 255.0, ((c >> 8) & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0]
        } else {
            hsv_to_rgb(st.hue as f32, st.saturation as f32 / 100.0, st.value as f32 / 100.0)
        };
        Some((farbe, st.opacity as f32 / 100.0, st.density_px, st.dotted))
    }

    pub(crate) fn build_hitbox_vertices(&self, screen_height: f32) -> Vec<render::HitboxVertex> {
        use hitbox_kinds::HitboxKind;
        let set = &self.settings;
        let stil = |art: HitboxKind| set.module_hitbox_styles[art.index()];
        let farbe = |st: hitbox_kinds::HitboxStyle| hsv_to_rgb(st.hue as f32, st.saturation as f32 / 100.0, st.value as f32 / 100.0);
        let modul_an = set.module_hitbox_enabled && (set.module_hitbox_show_smoothed || set.module_hitbox_show_raw);
        // Die Bloecke haben keine geglaettete oder rohe Lage, sie haengen nur am Modul.
        let netz = set.module_hitbox_enabled && stil(HitboxKind::Cobweb).on;
        let wasser = set.module_hitbox_enabled && stil(HitboxKind::Water).on;
        // Ohne Hitbox-Modul nur die markierten Spieler, und nur ihre geglaettete Box.
        let (show_smoothed, show_raw) = if modul_an { (set.module_hitbox_show_smoothed, set.module_hitbox_show_raw) } else { (true, false) };
        let schild = !modul_an && !self.entities.is_empty() && self.any_shield_hitbox_mark();
        let team = set.module_teams && set.team_hitboxes.iter().any(|t| t.on);
        if !(modul_an || schild || netz || wasser || team) {
            return Vec::new();
        }
        let eye = self.camera.position;
        let rel = |p: DVec3| -> Vec3 { (p - eye).as_vec3() };
        let smoothed_color =
            hsv_to_rgb(set.module_hitbox_smoothed_hue as f32, set.module_hitbox_smoothed_saturation as f32 / 100.0, set.module_hitbox_smoothed_value as f32 / 100.0);
        let raw_color = hsv_to_rgb(set.module_hitbox_raw_hue as f32, set.module_hitbox_raw_saturation as f32 / 100.0, set.module_hitbox_raw_value as f32 / 100.0);
        // Halbe Bildhoehe in Sichtraum-Einheiten bei d=1 -- dieselbe Groesse, die die
        // Projektion selbst benutzt (s. block_renderer::CameraUniform.params.y).
        let half_fov_tan = (self.camera.fov_degrees.to_radians() * 0.5).tan();
        let progress = self.tick_progress();
        // Nur Schild-Markierungen: voll deckend, sie sollen auffallen.
        let alpha = if set.module_hitbox_enabled { set.module_hitbox_opacity as f32 / 100.0 } else { 1.0 };
        let mut out = Vec::new();
        // Eine Box aus ihren Ecken (augenrelativ) als zwoelf Kanten, mit Dichte in Pixeln und
        // ob gepunktet (das Teams-Modul hat dafuer eigene Werte).
        let mut push_box = |min: Vec3, max: Vec3, color: [f32; 4], density_px: u32, dotted: bool| {
            let half_width = density_px as f32 / 2.0;
            let corner = |sel: [bool; 3]| Vec3::new(if sel[0] { max.x } else { min.x }, if sel[1] { max.y } else { min.y }, if sel[2] { max.z } else { min.z });
            const EDGES: [([bool; 3], [bool; 3]); 12] = [
                ([false, false, false], [true, false, false]),
                ([false, true, false], [true, true, false]),
                ([false, false, true], [true, false, true]),
                ([false, true, true], [true, true, true]),
                ([false, false, false], [false, true, false]),
                ([true, false, false], [true, true, false]),
                ([false, false, true], [false, true, true]),
                ([true, false, true], [true, true, true]),
                ([false, false, false], [false, false, true]),
                ([true, false, false], [true, false, true]),
                ([false, true, false], [false, true, true]),
                ([true, true, false], [true, true, true]),
            ];
            for (a_sel, b_sel) in EDGES {
                let a = corner(a_sel);
                let b = corner(b_sel);
                // Strich+Luecke-Laenge dieser KANTE, aus ihrer EIGENEN Entfernung (nicht
                // der der ganzen Box): eine nahe Kante braucht mehr Weltlaenge fuer
                // dasselbe Bildschirmmuster als eine ferne.
                let dash_len = dotted.then(|| {
                    let dist = ((a + b) * 0.5).length().max(0.05);
                    let px_per_world = ((screen_height * 0.5) / (dist * half_fov_tan)).max(1e-6);
                    // Das Strichmuster ist bewusst laenger als die Linie breit ist:
                    // Striche in Breite der Linie waeren Punkte, keine Striche.
                    density_px as f32 * HITBOX_DASH_PER_DENSITY / px_per_world
                });
                match dash_len {
                    Some(len) if len > 0.001 => {
                        let total = (b - a).length();
                        let dir = (b - a) / total.max(1e-6);
                        let mut s = 0.0;
                        let mut on = true;
                        while s < total {
                            let end = (s + len).min(total);
                            if on {
                                push_hitbox_edge(&mut out, a + dir * s, a + dir * end, color, half_width);
                            }
                            s = end;
                            on = !on;
                        }
                    }
                    _ => push_hitbox_edge(&mut out, a, b, color, half_width),
                }
            }
        };
        let (dichte, gepunktet) = (set.module_hitbox_density_px, set.module_hitbox_dotted);
        for (id, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            // Display-Entities sind nicht anvisierbar (s. Entities::pick) und haetten hier
            // eine Box, auf die man gar nicht zielen kann.
            if e.display.is_some() {
                continue;
            }
            // Schild-Modul: Cooldown bzw. gehobenes Schild faerbt die Box (Anzeige "Hitbox").
            // Sonst die eigene Art mit ihrer Farbe und Deckkraft, und alles andere
            // Angreifbare (Mobs) in der Farbe von "Geglättet".
            let art = world::entity_type_name(e.entity_type).and_then(HitboxKind::of_entity);
            // Teams-Modul: eigene Farbe, Deckkraft, Dichte und Strichelung fuer Verbuendete
            // bzw. Gegner. Es zeichnet auch ohne Hitbox-Modul (dann nur die geglaettete Box).
            let team_stil = self.team_hitbox_style(e);
            let (dichte, gepunktet) = team_stil.map_or((dichte, gepunktet), |(_, _, d, p)| (d, p));
            let (smoothed_color, raw_color, a) = match (self.shield_mark_color(*id, e, ShieldMark::Hitbox), art) {
                (Some([r, g, b, _]), _) => ([r, g, b], [r, g, b], alpha),
                (None, _) if team_stil.is_some() => {
                    let (farbe, deckkraft, _, _) = team_stil.expect("eben geprueft");
                    (farbe, farbe, deckkraft)
                }
                (None, _) if !modul_an => continue,
                (None, Some(art)) if stil(art).custom => {
                    if !stil(art).on {
                        continue;
                    }
                    (farbe(stil(art)), raw_color, alpha * stil(art).alpha as f32 / 100.0)
                }
                (None, Some(art)) if art.shown_by_default(set.module_hitbox_items) => (smoothed_color, raw_color, alpha),
                (None, Some(_)) => continue,
                (None, None) if world::is_attackable(e.entity_type) => (smoothed_color, raw_color, alpha),
                (None, None) => continue,
            };
            let (width, height) = e.hit_size();
            let half_w = width as f32 / 2.0;
            let height = height as f32;
            // Unterkante der Box, wie hit_pos (Windkugeln haengen tiefer).
            let unten = e.box_y_offset();
            let mut fuesse = |pos: DVec3, [r, g, b]: [f32; 3]| {
                let feet = rel(pos);
                push_box(feet + Vec3::new(-half_w, 0.0, -half_w), feet + Vec3::new(half_w, height, half_w), [r, g, b, a], dichte, gepunktet);
            };
            if show_smoothed {
                let (x, y, z) = e.render_pos(progress);
                fuesse(DVec3::new(x, y + unten, z), smoothed_color);
            }
            if show_raw {
                let (x, y, z) = e.target_pos();
                fuesse(DVec3::new(x, y + unten, z), raw_color);
            }
        }
        // Der eigene Spieler, nur in der 3. Person (wie Vanillas F3+B): in der Ich-Sicht sitzt
        // die Kamera in der Box. Er steht nicht in self.entities, Farbe wie die Art Spieler.
        // Mit dem Teams-Modul gehoert sie zu den Verbuendeten -- man ist im eigenen Team.
        let team_eigen = self.team_style_of(teams::Side::Friendly, self.own_uuid.and_then(|u| self.team_facts.get(&u)).and_then(|f| f.display_color()));
        let eigene = match (team_eigen, stil(HitboxKind::Player)) {
            (Some((f, a, _, _)), _) => Some((f, a)),
            (None, st) if st.custom => st.on.then(|| (farbe(st), alpha * st.alpha as f32 / 100.0)),
            _ => HitboxKind::Player.shown_by_default(set.module_hitbox_items).then_some((smoothed_color, alpha)),
        };
        let (eigen_dichte, eigen_gepunktet) = team_eigen.map_or((dichte, gepunktet), |(_, _, d, p)| (d, p));
        let eigen_roh = team_eigen.map_or(raw_color, |(f, ..)| f);
        // Abschaltbar im Modul ("Eigene Hitbox").
        if let (true, true, false, Some((farbe_geglaettet, a))) = (modul_an, set.module_hitbox_self, self.perspective.is_first_person(), eigene) {
            let half_w = self.physics.width() as f32 / 2.0;
            let height = self.physics.current_height() as f32;
            let mut fuesse = |pos: DVec3, [r, g, b]: [f32; 3]| {
                let feet = rel(pos);
                push_box(feet + Vec3::new(-half_w, 0.0, -half_w), feet + Vec3::new(half_w, height, half_w), [r, g, b, a], eigen_dichte, eigen_gepunktet);
            };
            if show_smoothed {
                fuesse(self.physics_render_prev.lerp(self.physics.pos, progress as f64), farbe_geglaettet);
            }
            if show_raw {
                fuesse(self.physics.pos, eigen_roh);
            }
        }
        // Spinnennetze im Umkreis als ganzer Block (so gross ist ihre Trefferflaeche, auch
        // wenn das Modell nur ein Kreuz ist), Wasserquellen nur in Reichweite des Eimers.
        // Ihre Kanten liegen genau in Flaechen, die schon gezeichnet sind: der Wasseroberflaeche
        // und dem Rahmen, den PvP-Packs ins Netz-Modell malen. Wie der Blockumriss ein kleines
        // Stueck zum Auge gezogen (s. OUTLINE_PULL), sonst gewinnt dort die Flaeche den
        // Tiefentest.
        let nah = |p: DVec3| -> Vec3 { ((p - eye) * (1.0 - OUTLINE_PULL)).as_vec3() };
        if netz || wasser {
            let auge = self.physics.eye_pos();
            let reichweite = self.attr_block_range;
            let r = if netz { HITBOX_COBWEB_RADIUS } else { reichweite.ceil() as i32 + 1 };
            let (cx, cy, cz) = (auge.x.floor() as i32, auge.y.floor() as i32, auge.z.floor() as i32);
            let (st_netz, st_wasser) = (stil(HitboxKind::Cobweb), stil(HitboxKind::Water));
            let mit_alpha = |st: hitbox_kinds::HitboxStyle| {
                let [r, g, b] = farbe(st);
                [r, g, b, alpha * st.alpha as f32 / 100.0]
            };
            for x in cx - r..=cx + r {
                for y in cy - r..=cy + r {
                    for z in cz - r..=cz + r {
                        let state = self.world.block_at(x, y, z);
                        if state == world::STATE_AIR {
                            continue;
                        }
                        let ecke = DVec3::new(x as f64, y as f64, z as f64);
                        if netz && world::block_name(state) == Some("cobweb") {
                            push_box(nah(ecke), nah(ecke + DVec3::ONE), mit_alpha(st_netz), dichte, gepunktet);
                        } else if wasser && hitbox_kinds::bucket_source(state) {
                            let Some(h) = world::fluid_height_at(&self.world, x, y, z) else { continue };
                            let max = ecke + DVec3::new(1.0, h, 1.0);
                            // Naechster Punkt der Box zum Auge: liegt er ausserhalb der
                            // Reichweite, trifft der Eimer diese Quelle nicht.
                            if auge.clamp(ecke, max).distance(auge) > reichweite {
                                continue;
                            }
                            push_box(nah(ecke), nah(max), mit_alpha(st_wasser), dichte, gepunktet);
                        }
                    }
                }
            }
        }
        out
    }

    // Betten. Sie sind BLOECKE, werden hier aber wie Entities gezeichnet -- ihr Blockmodell ist
    // leer (models/block/bed.json hat nur ein "particle"-Feld), Vanilla zeichnet sie mit einem
    // eigenen BlockEntity-Renderer und einer Entity-Textur. Ohne das bleibt ein Bett komplett
    // unsichtbar.
    //
    // Gezeichnet wird nur, was in der Naehe liegt (s. block_entity_shown) -- auch, damit eine
    // weit geladene Welt nicht tausende Betten pro Frame anfasst.
    // Die Positionen kommen aus World::beds, dem Index, der beim Laden/Aendern gepflegt wird.
    // `eye`: der Bezugspunkt des Entity-Puffers -- alles darin ist augenrelativ (s.
    // build_entity_vertices), die Blockposition wird also davon abgezogen.
    pub(crate) fn emit_beds(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        for (pos, state) in self.world.beds() {
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some(bett) = world::bed_instance(pos, state) else { continue };
            let Some(tex) = self.entity_atlas.rect(bett.texture) else { continue };
            let model = if bett.head { &self.bed_head_model } else { &self.bed_foot_model };
            // isFoot ist im WELT-Renderer immer false: der -1-Versatz gehoert allein zum
            // Item-Modell (renderAsItem zeichnet beide Haelften aus EINEM Ursprung). In der
            // Welt hat jede Haelfte ihr eigenes BlockEntity und damit ihren eigenen Ursprung.
            let base = world::entity_model::bed_transform(block_rel(pos, eye), bett.facing_deg, false);
            world::emit_entity_model(model, &world::Pose::new(), base, tex, ENTITY_BRIGHTNESS, out);
        }
    }

    // Shulkerkisten (ShulkerBoxBlockEntityRenderer) -- derselbe Weg wie die Betten, aus
    // demselben Index (World::beds fuehrt beide). Der Deckel folgt block_lids.
    pub(crate) fn emit_shulker_boxes(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        let t = self.tick_progress();
        let model = world::entity_model::shulker_box_model();
        for (pos, state) in self.world.beds() {
            let Some(kiste) = world::shulker_instance(pos, state) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some(tex) = self.entity_atlas.rect(kiste.texture) else { continue };
            let offen = self.block_lids.get(&pos).map_or(0.0, |d| d.last + (d.progress - d.last) * t);
            let base = world::entity_model::shulker_box_transform(block_rel(pos, eye), kiste.facing);
            world::emit_entity_model(model, &world::entity_model::shulker_box_pose(offen), base, tex, ENTITY_BRIGHTNESS, out);
        }
    }

    // CORAL_TITLE_DEBUG=1: jeden Titel/Untertitel Abschnitt fuer Abschnitt mit Zeichencodes
    // ausgeben, und welche Zeichen die aktuelle Schrift gar nicht kennt (die fallen beim
    // Zeichnen still weg). Gebaut fuer "beim Titel fehlt oft das letzte Zeichen".
    pub(crate) fn title_debug(&self, art: &str, spans: &[protocol::nbt::TextSpan]) {
        if std::env::var_os("CORAL_TITLE_DEBUG").is_none() {
            return;
        }
        eprintln!("[title] {art}: {} Abschnitt(e)", spans.len());
        for (i, span) in spans.iter().enumerate() {
            let codes: Vec<String> = span
                .text
                .chars()
                .map(|c| {
                    let fehlt = self.gpu.as_ref().is_some_and(|g| g.font_atlas.glyph(c).is_none());
                    format!("U+{:04X}{}", c as u32, if fehlt { "(fehlt)" } else { "" })
                })
                .collect();
            eprintln!("[title]   #{i} farbe={:?} {:?} [{}]", span.color, span.text, codes.join(" "));
        }
    }

    // Profil eines platzierten Kopfes merken: die Textur-Property wie bei Kopf-Items (s.
    // note_head_profile), geladen wird ueber request_head_skins.
    // Block-Entities (Truhen, Betten, Schilder, ...) wie Vanilla nur in gezeichneten Chunks
    // (BlockEntityRenderDispatcher laeuft ueber die kompilierten Sections). Sonst blieben sie
    // nach dem Verkleinern der Sichtweite einzeln in der Luft stehen.
    pub(crate) fn block_column_drawn(&self, pos: (i32, i32, i32)) -> bool {
        self.render_limit_meshed.contains(&(pos.0.div_euclid(16), pos.2.div_euclid(16)))
    }

    // Gezeichneter Chunk und innerhalb der Block-Entity-Renderdistanz (vom Auge aus).
    pub(crate) fn block_entity_shown(&self, pos: (i32, i32, i32)) -> bool {
        let d = self.settings.block_entity_render_distance;
        self.block_column_drawn(pos) && (d >= BLOCK_ENTITY_DISTANCE_MAX || self.near_eye(pos, d as f64))
    }

    pub(crate) fn near_eye(&self, pos: (i32, i32, i32), abstand: f64) -> bool {
        let mitte = DVec3::new(pos.0 as f64 + 0.5, pos.1 as f64 + 0.5, pos.2 as f64 + 0.5);
        (mitte - self.physics.eye_pos()).length_squared() <= abstand * abstand
    }

    pub(crate) fn note_placed_head(&mut self, pos: (i32, i32, i32), profil: &protocol::item::HeadProfile) {
        self.note_head_profile(profil);
        match head_skin_key(profil) {
            Some(key) => {
                self.placed_heads.insert(pos, key);
            }
            None => {
                self.placed_heads.remove(&pos);
            }
        }
    }

    // Platzierte Koepfe (SkullBlockEntityRenderer.render): am Boden auf der Blockmitte, an der
    // Wand ein Viertel Block von der Wand weg und 4 Pixel hoch, gedreht in 22.5-Grad-Stufen.
    pub(crate) fn emit_skulls(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        for (pos, state) in self.world.beds() {
            let Some(kopf) = world::skull_instance(pos, state) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some((tex, hut)) = self.skull_texture_for(kopf.item, self.placed_heads.get(&pos).copied()) else { continue };
            let b = block_rel(pos, eye);
            let versatz = match kopf.wall {
                None => [0.5, 0.0, 0.5],
                Some((dx, dz)) => [0.5 - dx as f32 * 0.25, 0.25, 0.5 - dz as f32 * 0.25],
            };
            let base = world::Affine::translation([b[0] + versatz[0], b[1] + versatz[1], b[2] + versatz[2]]);
            held_item::emit_skull_at(tex, hut, base, kopf.yaw_deg, ENTITY_BRIGHTNESS, out);
        }
    }

    // Kupfergolem-Statuen (CopperGolemStatueBlockRenderer): das Golem-Modell der Haltung auf
    // der Blockmitte, in die FACING-Richtung gedreht, mit Vanillas Statuen-Haltung (Wurzel auf
    // y = 0, um Z halb gedreht). Frueher als Lebewesen eingebettet -- das stimmte nur fuer
    // den Stehend-Layer, die anderen Haltungen schwebten 1,5 Bloecke hoch.
    pub(crate) fn emit_statues(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        for (pos, state) in self.world.beds() {
            let Some(statue) = world::statue_instance(pos, state) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let (Some(model), Some(tex)) = (world::model_data::layer_model(statue.layer), self.entity_atlas.rect(statue.texture)) else { continue };
            let b = block_rel(pos, eye);
            let base = world::entity_model::copper_golem_statue_transform(b, statue.yaw_deg);
            world::emit_entity_model(model, &world::entity_model::copper_golem_statue_pose(model), base, tex, ENTITY_BRIGHTNESS, out);
        }
    }

    // Truhen (ChestBlockEntityRenderer) aus demselben Index wie Betten und Shulker. Eine
    // Doppeltruhe geht als Ganzes auf: Vanilla nimmt den groesseren Deckelstand beider Haelften
    // (ChestBlock.getAnimationProgressRetriever).
    pub(crate) fn emit_chests(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        let t = self.tick_progress();
        let weihnachten = is_christmas();
        let deckel = |pos: &(i32, i32, i32)| self.block_lids.get(pos).map_or(0.0, |d| d.last + (d.progress - d.last) * t);
        for (pos, state) in self.world.beds() {
            let Some(truhe) = world::chest_instance(pos, state, weihnachten) else { continue };
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some(tex) = self.entity_atlas.rect(truhe.texture) else { continue };
            let offen = truhe.partner.map_or(0.0, |p| deckel(&p)).max(deckel(&pos));
            let base = world::entity_model::chest_transform(block_rel(pos, eye), truhe.yaw_deg);
            world::emit_entity_model(world::entity_model::chest_model(truhe.half), &world::entity_model::chest_pose(offen), base, tex, ENTITY_BRIGHTNESS, out);
        }
    }

    // ShulkerBoxBlockEntity.updateAnimation / ChestLidAnimator.step: 0.1 pro Tick auf bzw. zu.
    // Fertig geschlossene und abgebaute fliegen raus.
    pub(crate) fn tick_block_lids(&mut self) {
        let world = &self.world;
        self.block_lids.retain(|pos, d| {
            d.last = d.progress;
            d.progress = if d.opening { (d.progress + 0.1).min(1.0) } else { (d.progress - 0.1).max(0.0) };
            (d.opening || d.last > 0.0) && world::block_entity::has_lid(world.block_at(pos.0, pos.1, pos.2))
        });
    }

    // Schilder (SignBlockEntityRenderer): wie die Betten BLOCK-Entities mit leerem
    // Blockmodell, gezeichnet mit Entity-Modell und -Textur. Ohne das fehlen sie
    // ganz. Der Text kommt im UI-Pass (s. build_sign_texts).
    // Augenrelativ wie emit_beds. Frueher standen die Schilder irgendwo im Himmel:
    // hier stand die absolute Blockposition in einem augenrelativen Puffer.
    pub(crate) fn emit_signs(&self, eye: DVec3, out: &mut Vec<world::BlockVertex>) {
        for &pos in self.signs.keys() {
            if !self.block_entity_shown(pos) {
                continue;
            }
            let Some((holz, art, grad)) = self.sign_at(pos) else { continue };
            let (textur, modell) = match art {
                world::entity_model::SignKind::Hanging(a) => {
                    (world::entity_model::HANGING_SIGN_TEXTURES[holz], world::entity_model::hanging_sign_model(a))
                }
                _ => (world::entity_model::SIGN_TEXTURES[holz], world::entity_model::sign_model(art == world::entity_model::SignKind::Standing)),
            };
            let Some(tex) = self.entity_atlas.rect(textur) else { continue };
            let base = world::entity_model::sign_base(block_rel(pos, eye), art, grad);
            let base = world::entity_model::sign_model_transform(base, art);
            world::emit_entity_model(&modell, &world::Pose::new(), base, tex, ENTITY_BRIGHTNESS, out);
        }
    }

    // Holzart, Art und Drehung (Grad, Vanillas getRotationDegrees) des Schildes an `pos`, oder
    // None, wenn dort (inzwischen) keins steht. Stehend und an der Decke haengend: rotation *
    // 22.5; an der Wand: die Blickrichtung (Sued 0, West 90, Nord 180, Ost 270).
    pub(crate) fn sign_at(&self, pos: (i32, i32, i32)) -> Option<(usize, world::entity_model::SignKind, f32)> {
        use world::entity_model::{HangingAttachment, SignKind};
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        let (holz, mut art) = world::entity_model::sign_kind(world::block_name(state)?)?;
        if art == SignKind::Hanging(HangingAttachment::Ceiling) && world::state_props::property(state, "attached") == Some("true") {
            art = SignKind::Hanging(HangingAttachment::CeilingMiddle);
        }
        let grad = match art {
            SignKind::Standing | SignKind::Hanging(HangingAttachment::Ceiling) | SignKind::Hanging(HangingAttachment::CeilingMiddle) => {
                world::state_props::property(state, "rotation").and_then(|r| r.parse::<f32>().ok()).unwrap_or(0.0) * 22.5
            }
            _ => match world::state_props::property(state, "facing") {
                Some("west") => 90.0,
                Some("north") => 180.0,
                Some("east") => 270.0,
                _ => 0.0,
            },
        };
        Some((holz, art, grad))
    }

    // Modell + Grundform + Atlas-Rechteck fuer einen Entity-Typ, oder None wenn es hier
    // (noch) kein Modell dafuer gibt.
    // Ein End-Kristall wie EndCrystalEntityRenderer: das Modell (s. world::end_crystal_model)
    // mit Textur aus dem Resourcepack, BEIDSEITIG -- Vanilla zeichnet ihn mit
    // entityCutoutNoCull, man sieht durch die durchsichtigen Stellen des Glases also auch
    // dessen Rueckseiten. Die Entity-Pipeline hier verwirft Rueckseiten, deshalb wird jedes
    // Dreieck ein zweites Mal andersherum ausgegeben.
    //
    // Das Alter beginnt in Vanilla bei einem Zufallswert (endCrystalAge = nextInt(100000)),
    // damit nicht alle Kristalle im Gleichtakt schweben; hier fest aus der Entity-ID.
    // Ein Pfeil wie ArrowRenderer: das Modell entlang der Flugrichtung gedreht. Die Rotation
    // kommt aus der Zwischenbild-Interpolation, nicht aus dem Tickwert -- ein Pfeil dreht
    // sich waehrend des Fluges staendig (s. world::entity::tick_projectile), und die
    // Tickstufen waeren bei seiner Geschwindigkeit deutlich sichtbar.
    pub(crate) fn emit_arrow(&self, e: &world::Entity, feet: [f32; 3], progress: f32, texture: &'static str, out: &mut Vec<world::BlockVertex>) {
        let Some(tex) = self.entity_atlas.rect(texture) else { return };
        let (yaw, pitch) = (e.render_yaw(progress), e.render_pitch(progress));
        let model = world::entity_model::arrow_model(texture);
        let pose = world::entity_model::Pose::new();
        // Der Abschuss-Versatz geht NUR hier drauf, nicht in feet selbst: die Trefferbox und
        // alles andere, was an der Position haengt, bleibt an der echten Stelle.
        let versatz = e.launch_render_offset(progress);
        let sichtbar = [feet[0] + versatz[0], feet[1] + versatz[1], feet[2] + versatz[2]];
        let base = world::entity_model::arrow_transform(sichtbar, yaw, pitch);
        world::emit_entity_model(&model, &pose, base, tex, ENTITY_BRIGHTNESS, out);
    }

    pub(crate) fn emit_end_crystal(&self, id: i32, e: &world::Entity, feet: [f32; 3], progress: f32, out: &mut Vec<world::BlockVertex>) {
        let Some(tex) = self.entity_atlas.rect(world::entity_model::END_CRYSTAL_TEXTURE) else { return };
        // Klein genug, um es je Bild neu zu bauen (vier Teile).
        let model = world::entity_model::end_crystal_model(e.end_crystal_show_bottom);
        let start = (id as u32).wrapping_mul(2_654_435_761) % 100_000;
        let pose = world::entity_model::end_crystal_pose(start as f32 + e.age_ticks(progress));
        let anfang = out.len();
        world::emit_entity_model(&model, &pose, world::entity_model::end_crystal_transform(feet), tex, ENTITY_BRIGHTNESS, out);
        let rueckseiten: Vec<world::BlockVertex> = out[anfang..].chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
        out.extend(rueckseiten);
    }

    // Entity.shouldRender(distance): mittlere Kantenlaenge der Box mal 64 mal
    // renderDistanceMultiplier (GameOptions: Sichtweite / 8, auf 1..2,5 begrenzt). Windkugeln
    // (ExplosiveProjectileEntity) rechnen die Kante noch mal 4. `feet` ist schon
    // kamerarelativ.
    pub(crate) fn projectile_in_render_range(&self, e: &world::Entity, feet: [f32; 3], faktor: f64) -> bool {
        let (breite, hoehe) = e.hit_size();
        let kante = (breite * 2.0 + hoehe) / 3.0;
        let multiplikator = (self.settings.max_render_distance as f64 / 8.0).clamp(1.0, 2.5);
        let grenze = kante * faktor * 64.0 * multiplikator;
        let d2 = feet.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>();
        d2 < grenze * grenze
    }

    // Eine Windkugel wie WindChargeEntityRenderer (s. world::entity_model::wind_charge_model).
    // Ohne Modell stand dort der
    // Platzhalter-Kasten. Beidseitig wie der Kristall: die durchsichtigen Stellen des
    // Wirbels zeigen in Vanilla die Innenseiten.
    pub(crate) fn emit_wind_charge(&self, e: &world::Entity, feet: [f32; 3], progress: f32, out: &mut Vec<world::BlockVertex>) {
        let Some(tex) = self.entity_atlas.rect(world::entity_model::WIND_CHARGE_TEXTURE) else { return };
        let model = world::entity_model::wind_charge_model();
        let pose = world::entity_model::wind_charge_pose(e.age_ticks(progress));
        let anfang = out.len();
        world::emit_entity_model(&model, &pose, world::entity_model::Affine::translation(feet), tex, ENTITY_BRIGHTNESS, out);
        let rueckseiten: Vec<world::BlockVertex> = out[anfang..].chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
        out.extend(rueckseiten);
    }

    pub(crate) fn model_for_type(&self, entity_type: i32) -> Option<(&world::Model, world::entity_model::ModelKind, [f32; 4])> {
        let name = world::entity_type_name(entity_type)?;
        let (_, model, kind) = self.entity_models.iter().find(|(n, _, _)| *n == name)?;
        let tex = self.entity_atlas.rect(model.texture)?;
        Some((model, *kind, tex))
    }

    // Die Ego-Hand: der eigene rechte Arm im Sichtraum, mit dem eigenen Skin.
    //
    // Vanillas Kette (HeldItemRenderer.renderItem -> renderArmHoldingItem), Schritt fuer
    // Schritt:
    //   1. Nachschwingen: die Hand dreht sich um ein Zehntel der Differenz zwischen echter und
    //      geglaetteter Blickrichtung (s. own_render_yaw) -- beim schnellen Umsehen bleibt sie
    //      kurz zurueck.
    //   2. Platzierung samt Schlag- und Wechsel-Animation (s. world::first_person_arm_transform).
    //   3. Der Arm selbst in Grundhaltung mit leichtem Rollen nach aussen.
    // Eine Zeile fuers F3-Overlay: welche Effekte der Client dem eigenen Spieler zuschreibt und
    // was daraus fuers Tempo folgt.
    pub(crate) fn effect_debug_line(&self) -> String {
        if self.own_effects.is_empty() {
            return "Effekte: keine  |  Tempo x1.00".to_string();
        }
        let mut teile: Vec<String> = self
            .own_effects
            .iter()
            .map(|(effekt, staerke)| format!("{} {}", protocol::packets::mob_effect_name(*effekt), staerke + 1))
            .collect();
        teile.sort();
        format!("Effekte: {}  |  Tempo x{:.2}", teile.join(", "), self.effect_speed_factor())
    }

    // Der Tempofaktor aus Schnelligkeit und Langsamkeit (s. physics::effect_speed_factor).
    pub(crate) fn effect_speed_factor(&self) -> f64 {
        let staerke = |effekt: i32| self.own_effects.get(&effekt).copied();
        physics::effect_speed_factor(staerke(protocol::packets::MOB_EFFECT_SPEED), staerke(protocol::packets::MOB_EFFECT_SLOWNESS))
    }

    // Die Himmelsfarbe dieses Frames, fertig fuer wgpu (also LINEAR).
    //
    // Zwei Quellen: entweder die feste Farbe des Moduls (s. ModuleId::SkyColor) oder der
    // Tag/Nacht-Verlauf aus der Weltzeit. Die Umrechnung nach linear passiert bewusst ganz am
    // Ende und an EINER Stelle -- beide Wege liefern Anzeige-Farben.
    // Modul Himmelsfarbe: gilt nur im PvP-Pfad. Der Shader-Pfad hat eigenen Himmel, Sonne und
    // Tageslicht -- eine feste Farbe dort passte weder zum Verlauf noch zu den Schatten.
    pub(crate) fn sky_color_module_active(&self) -> bool {
        self.sky_module_mode() == Some(SkyMode::Solid)
    }

    // Die Art des Moduls Himmel, solange es wirkt (an, und nur im Leistungs-Pfad). Nicht beim
    // Launcher-Bild: das zeigt die Szene bei ihrer eigenen Uhrzeit (Tag), nicht den Himmel der Config.
    pub(crate) fn sky_module_mode(&self) -> Option<SkyMode> {
        (self.settings.module_sky_color_enabled && self.settings.render_path != 1 && self.hero_fov.is_none()).then_some(self.settings.sky_mode)
    }

    // Die Weltzeit, nach der der Himmel aussieht: die echte, oder mit dem Modul (Feste
    // Uhrzeit) die gewaehlte Uhrzeit am selben Tag -- der Tag bleibt, damit Packs mit
    // Tageszyklus (days/daysLoop) ihren Himmel des Tages zeigen.
    pub(crate) fn sky_day_time(&self) -> i64 {
        // Einfarbig kennt keine Uhrzeit (die Zeile ist dort ausgegraut).
        if self.settings.module_sky_color_enabled && self.settings.module_sky_fixed_time && self.settings.sky_mode != SkyMode::Solid && self.hero_fov.is_none() {
            self.world_time_of_day.div_euclid(SKY_TIME_DAY as i64) * SKY_TIME_DAY as i64 + self.settings.module_sky_time as i64
        } else {
            self.world_time_of_day
        }
    }

    // Pack-Himmel, einmal pro Spieltick: Hoehe und Biom der Kamera (s. custom_sky::tick).
    pub(crate) fn tick_custom_sky(&mut self) {
        let Some(welt) = custom_sky::world_index(&self.dimension_type_name) else { return };
        if self.custom_sky.is_empty() {
            return;
        }
        let p = self.physics.pos;
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let biom = self.biome_names.get(self.world.biome_at(x, y, z) as usize).map_or("", String::as_str);
        self.custom_sky.tick(welt, y, biom);
    }

    // Das Wetter fuers Bild: Regenstaerke, Gewitterstaerke (wie Vanillas getThunderLevel schon
    // mit der Regenstaerke multipliziert) und die vom Modul fest vorgegebene Art.
    pub(crate) fn visual_weather(&self) -> (f32, f32, Option<precipitation::Kind>) {
        let server = (self.rain_gradient, self.thunder_gradient * self.rain_gradient, None);
        if !self.settings.module_weather {
            return server;
        }
        match self.settings.weather_mode {
            WeatherMode::Server => server,
            WeatherMode::Clear => (0.0, 0.0, None),
            WeatherMode::Rain => (1.0, 0.0, Some(precipitation::Kind::Rain)),
            WeatherMode::Snow => (1.0, 0.0, Some(precipitation::Kind::Snow)),
        }
    }

    // Das Wetter fuer den Himmel (Farbe, Pack-Himmel). Das Modul Wetter legt nur fest, was
    // faellt: fester Regen oder Schnee soll den Himmel nicht grau machen und die Ebenen des
    // Pack-Himmels nicht ausblenden. Nur "Klar" haelt auch den Himmel klar.
    pub(crate) fn sky_weather(&self) -> (f32, f32) {
        if self.settings.module_weather && self.settings.weather_mode == WeatherMode::Clear {
            return (0.0, 0.0);
        }
        (self.rain_gradient, self.thunder_gradient * self.rain_gradient)
    }

    // Wo Niederschlag faellt. Ohne Modul wie Vanilla (Quader, 10 Bloecke); mit Modul zwischen
    // zwei Kugeln um die Kamera (Von/Bis).
    pub(crate) fn weather_bounds(&self) -> precipitation::Bounds {
        let set = &self.settings;
        if set.module_weather {
            precipitation::Bounds {
                inner: set.weather_radius_inner.min(set.weather_radius.saturating_sub(1)),
                outer: set.weather_radius,
                sphere: true,
            }
        } else {
            precipitation::Bounds { inner: 0, outer: precipitation::RADIUS_DEFAULT, sphere: false }
        }
    }

    // Niederschlag, einmal pro Tick: die Daecher um die Kamera vermessen (nur solange etwas
    // faellt). Bewusst anders als Vanilla: Barrieren halten ihn nicht auf -- Server ziehen
    // damit unsichtbare Decken und Waende ein, und Schnee, der in der Luft an einer
    // unsichtbaren Flaeche endet, sieht kaputt aus (Nutzerwunsch). Nur fuers Bild; der
    // Dreizack fragt weiter Vanillas Heightmap (s. touching_water_or_rain).
    pub(crate) fn tick_weather(&mut self) {
        let (regen, _, _) = self.visual_weather();
        let radius = self.weather_bounds().outer;
        let p = self.camera.position;
        let mitte = (p.x.floor() as i32, p.z.floor() as i32);
        let (welt, formen) = (&self.world, &self.block_shapes);
        self.precipitation.tick(mitte, radius, regen > 0.0, |x, z| {
            welt.motion_blocking_top_except(x, z, formen, |state| world::block_registry::block_name(state) == Some("barrier"))
        });
    }

    // Niederschlag, einmal pro Frame: die Quads um die aktuelle Kamera.
    pub(crate) fn build_weather_vertices(&mut self) -> [u32; 2] {
        let (regen, _, fest) = self.visual_weather();
        let grenzen = self.weather_bounds();
        let (welt, klima) = (&self.world, &self.biome_climate);
        let art = |x: i32, y: i32, z: i32| {
            fest.or_else(|| match world::weather::precipitation(*klima.get(welt.biome_at(x, y, z) as usize)?, y) {
                world::weather::Precipitation::None => None,
                world::weather::Precipitation::Rain => Some(precipitation::Kind::Rain),
                world::weather::Precipitation::Snow => Some(precipitation::Kind::Snow),
            })
        };
        let mut out = std::mem::take(&mut self.weather_vertices);
        let anzahl = self.precipitation.build(self.camera.position, self.tick_progress(), grenzen, regen, art, &mut out);
        self.weather_vertices = out;
        anzahl
    }

    // Pack-Himmel, einmal pro Frame: fertige Bilder an die GPU, lange nicht gebrauchte weg,
    // die Ebenen dieses Frames bestimmen und fehlende Bilder anfordern.
    pub(crate) fn update_custom_sky(&mut self) {
        let day_time = self.sky_day_time();
        let (rain, thunder) = self.sky_weather();
        // Modul Himmel: Einfarbig und Verlauf ersetzen den Pack-Himmel.
        let modus = self.sky_module_mode();
        let pack_himmel = !matches!(modus, Some(SkyMode::Solid | SkyMode::Gradient));
        let verlauf = (modus == Some(SkyMode::Gradient)).then(|| {
            let c = self.sky_clear_color();
            render::gradient_sky::GradientFrame {
                sky: [c.r as f32, c.g as f32, c.b as f32],
                clear: 1.0 - rain.clamp(0.0, 1.0),
                sky_angle: world::sky::sky_angle(day_time),
                sun: self.settings.sky_sun,
                moon: self.settings.sky_moon,
                stars: self.settings.sky_stars,
            }
        });
        let Some(gpu) = self.gpu.as_mut() else { return };
        gpu.sky_gradient = verlauf;
        let frei = self.custom_sky.poll(&mut |bild| gpu.custom_sky.add_texture(&gpu.device, bild.width, bild.height, bild.rgba));
        for id in frei {
            gpu.custom_sky.remove_texture(id);
        }
        gpu.custom_sky.upload_step(&gpu.queue);
        let max_dim = gpu.device.limits().max_texture_dimension_2d;
        // Beim Verbinden, bis die Uhrzeit da ist, und nach einem Dimensionswechsel: die Welt
        // vorladen, in der man ist (vor dem Login die zuletzt bekannte, meist die Oberwelt),
        // noch nichts zeigen. Der Login kommt vor der Uhrzeit -- bis dahin sind die Bilder
        // meist schon fertig, und der Himmel steht beim Betreten gleich richtig da. Nach einem
        // Wechsel wartet er hoechstens SKY_WORLD_WAIT, damit ein riesiges Pack nicht ewig
        // ohne Himmel bleibt.
        let verbunden = self.conn_status != ConnStatus::Disconnected;
        if !verbunden || !pack_himmel {
            self.sky_world_pending = None;
        }
        let mut wartet = self.sky_time_pending && verbunden;
        if (wartet || self.sky_world_pending.is_some()) && pack_himmel {
            let welt = custom_sky::world_index(&self.dimension_type_name).unwrap_or(0);
            let installation = &mut self.installation;
            let fertig = self.custom_sky.preload(welt, max_dim, &mut |pfad| installation.load_bytes(pfad).ok());
            if fertig || self.sky_world_pending.is_some_and(|t| t.elapsed() > SKY_WORLD_WAIT) {
                self.sky_world_pending = None;
            }
        }
        wartet |= self.sky_world_pending.is_some();
        gpu.sky_layers = match custom_sky::world_index(&self.dimension_type_name) {
            Some(welt) if pack_himmel && !wartet && !self.custom_sky.is_empty() => {
                let frame = custom_sky::SkyFrame {
                    world: welt,
                    day_time,
                    celestial: world::sky::sky_angle(day_time),
                    rain,
                    thunder,
                };
                let installation = &mut self.installation;
                self.custom_sky.frame(&frame, max_dim, &mut |pfad| installation.load_bytes(pfad).ok())
            }
            _ => Vec::new(),
        };
    }

    pub(crate) fn sky_clear_color(&self) -> wgpu::Color {
        let srgb = if self.sky_color_module_active() {
            self.settings.sky_color_rgb()
        } else {
            let (regen, gewitter) = self.sky_weather();
            world::sky::weather_sky(world::sky::time_of_day_sky(SKY_DAY_SRGB, self.sky_day_time()), regen, gewitter)
        };
        wgpu::Color {
            r: world::sky::srgb_to_linear(srgb[0]) as f64,
            g: world::sky::srgb_to_linear(srgb[1]) as f64,
            b: world::sky::srgb_to_linear(srgb[2]) as f64,
            a: 1.0,
        }
    }

    // Vanillas getHandSwingProgress fuer den EIGENEN Spieler: 0, solange nicht geschlagen
    // wird, sonst 0..1 ueber die Schwungdauer. Unser Zaehler laeuft rueckwaerts (s.
    // own_swing_ticks).
    //
    // Denselben Wert brauchen ZWEI Ansichten: der Arm in der Ego-Perspektive und das eigene
    // Modell in der 3. Person. Frueher rechnete ihn nur die erste aus -- in der 3. Person
    // stand die eigene Figur beim Schlagen still.
    pub(crate) fn own_hand_swing(&self, tick_progress: f32) -> f32 {
        if self.own_swing_ticks == 0 {
            return 0.0;
        }
        swing_progress_from_ticks(self.own_swing_ticks, tick_progress, self.own_swing_duration())
    }

    // LivingEntity.getHandSwingDuration fuer uns selbst: am Haupthand-Item (Speere 13..23
    // Ticks, sonst 6). Wechselt man mitten im Schwung das Item, laeuft der Zaehler einfach mit
    // der neuen Dauer weiter -- swing_progress_from_ticks deckelt, es springt also nichts.
    pub(crate) fn own_swing_duration(&self) -> u32 {
        self.own_hand_stack(protocol::packets::HAND_MAIN).map_or(PLAYER_SWING_TICKS, |s| world::item_behavior::swing(s).1)
    }

    pub(crate) fn build_block_selection_vertices(&self) -> Vec<render::HitboxVertex> {
        // Wie Vanilla: mit verstecktem HUD (F1) auch kein Blockrahmen. Als Zuschauer keiner --
        // dort baut und oeffnet man nichts, der Rahmen stuende nur im Bild. Beim Schreiben aufs
        // Schild auch nicht -- der Rahmen laege genau um den Text.
        if self.hud_hidden || self.game_mode == GAME_MODE_SPECTATOR || self.sign_edit.is_some() {
            return Vec::new();
        }
        let CrosshairTarget::Block(pos, _) = self.crosshair_target() else { return Vec::new() };
        // Blockumriss-Modul: beim Abbauen dieses Blocks keinen Rahmen (s.
        // Settings::block_outline_hide_mining).
        if self.settings.module_block_outline && self.settings.block_outline_hide_mining && self.breaking_block && self.breaking_pos == Some(pos) {
            return Vec::new();
        }
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        let mut boxes = world::outline_boxes(state, &self.block_shapes);
        if boxes.is_empty() {
            return Vec::new();
        }
        // Zufallsversatz des Umrisses (Blumen, Bambus, Tropfstein) wie beim Zielen (s. physics).
        let (odx, odz) = world::block_name(state).map(|n| world::outline_xz_offset(n, pos.0, pos.2)).unwrap_or((0.0, 0.0));
        for b in &mut boxes {
            b.min[0] += odx;
            b.max[0] += odx;
            b.min[2] += odz;
            b.max[2] += odz;
        }
        let (color, half_width) = if self.settings.module_block_outline {
            (self.settings.block_outline_rgba(), self.settings.block_outline_width_px as f32 / 2.0)
        } else {
            vanilla_block_outline(self.gpu.as_ref().map_or(1920.0, |g| g.size().0))
        };
        let eye = self.camera.position;
        let mut out = Vec::new();
        for shape in boxes {
            // Augenrelativ in f64 gerechnet und ERST danach auf f32 verkuerzt -- bei
            // sechsstelligen Weltkoordinaten waere die Summe in f32 laengst gerastert und
            // der Umriss wuerde sichtbar zittern (derselbe Grund wie bei ColumnOrigin).
            //
            // Dazu ein kleines Stueck zum Auge gezogen (s. OUTLINE_PULL): auf dem Bildschirm
            // an derselben Stelle, in der Tiefe aber VOR den Flaechen, auf denen die Kante
            // liegt. Vorher lag das Band genau in deren Ebene und rang mit ihnen um den
            // Tiefentest -- vorn kam nur die Haelfte ueber der Luft durch, hinten (wo beide
            // Seiten Flaechen sind) oft gar nichts. Echte Verdecker davor verdecken weiterhin.
            let ecke = |v: [f32; 3]| {
                ((DVec3::new(pos.0 as f64 + v[0] as f64, pos.1 as f64 + v[1] as f64, pos.2 as f64 + v[2] as f64) - eye) * (1.0 - OUTLINE_PULL))
                    .as_vec3()
            };
            let (min, max) = (ecke(shape.min), ecke(shape.max));
            let corner = |mask: [bool; 3]| {
                Vec3::new(if mask[0] { max.x } else { min.x }, if mask[1] { max.y } else { min.y }, if mask[2] { max.z } else { min.z })
            };
            const EDGES: [([bool; 3], [bool; 3]); 12] = [
                ([false, false, false], [true, false, false]), ([false, true, false], [true, true, false]),
                ([false, false, true], [true, false, true]), ([false, true, true], [true, true, true]),
                ([false, false, false], [false, true, false]), ([true, false, false], [true, true, false]),
                ([false, false, true], [false, true, true]), ([true, false, true], [true, true, true]),
                ([false, false, false], [false, false, true]), ([true, false, false], [true, false, true]),
                ([false, true, false], [false, true, true]), ([true, true, false], [true, true, true]),
            ];
            for (a, b) in EDGES {
                push_hitbox_edge(&mut out, corner(a), corner(b), color, half_width);
            }
        }
        out
    }

    // Vanillas Abbau-Animation: die Risstextur (destroy_stage_0..9) ueber der echten Form
    // des Blocks, den wir gerade abbauen -- s. world::mesher::block_breaking_vertices.
    //
    // Die Stufe ist Vanillas MultiPlayerGameMode.getDestroyStage(): `(int)(Fortschritt * 10)`,
    // und BEI FORTSCHRITT 0 noch gar nichts (dort liefert Vanilla -1). Mehr als Stufe 9 gibt
    // es nicht -- bei 1.0 ist der Block ohnehin weg.
    //
    // AUGENRELATIV wie die Geisterbloecke (s. ghost_vertices): Blockposition minus Auge in
    // f64, erst das Ergebnis nach f32.
    pub(crate) fn build_crumbling_vertices(&self) -> Vec<world::mesher::BlockVertex> {
        if !self.breaking_block || self.breaking_progress <= 0.0 {
            return Vec::new();
        }
        let Some(pos) = self.breaking_pos else { return Vec::new() };
        let stage = ((self.breaking_progress * 10.0) as usize).min(assets::DESTROY_STAGE_COUNT - 1);
        let Some(stage_uv) = self.block_uvs.destroy_stage_rect(stage) else { return Vec::new() };
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        if state == world::chunk::STATE_AIR {
            return Vec::new();
        }
        let base = (DVec3::new(pos.0 as f64, pos.1 as f64, pos.2 as f64) - self.camera.position).as_vec3();
        let block_at = |x: i32, y: i32, z: i32| self.world.block_at(x, y, z);
        world::mesher::block_breaking_vertices(base.into(), pos, state, stage_uv, &self.block_shapes, &block_at)
    }

    // Die Spielerfigur im Inventar -- Vanillas "Puppe" (InventoryScreen.drawEntity).
    //
    // Rueckgabe: (Vertices, Sichtmatrix, Projektionsmatrix) oder None, wenn gerade kein
    // Bildschirm mit Figur offen ist. Die Geometrie ist dieselbe wie beim eigenen Modell in
    // der 3. Person -- Skin, Ruestung, beide Haende -- nur mit einer eigenen Pose und einer
    // eigenen, ORTHOGRAFISCHEN Kamera.
    pub(crate) fn build_doll(&self, screen: (f32, f32)) -> Option<(Vec<world::BlockVertex>, glam::Mat4, glam::Mat4)> {
        let layout = if self.creative_shown() {
            self.creative_layout(screen)?
        } else {
            inventory::screen_layout(&self.inventory, screen, self.inventory_scale(screen))
        };
        let rect = layout.doll?;
        if self.open_menu_type() == Some(inventory::MENU_ENCHANTMENT) {
            return self.build_enchant_book(rect, screen);
        }
        let (mx, my) = self.cursor_pos;
        let (cx, cy) = rect.center();

        // Vanillas Blickfolge aus InventoryScreen.drawEntity: i = atan(dx / 40), j = atan(dy / 40)
        // mit dx/dy = Figurmitte minus Zeiger. Koerper 180 + 20i, Blick (und Kopf) 180 + 40i,
        // Neigung -20j.
        //
        // Die 40 sind PIXEL, die atan-Werte sind RADIANT und werden danach mit 20 bzw. 40 als
        // GRAD weiterverrechnet -- das ist keine Schlamperei von mir, sondern steht so im
        // Jar. Ein "sauberes" to_degrees() dazwischen liesse die Figur der Maus rund
        // 57-mal zu weit folgen.
        // Die 40 sind GUI-Pixel; `cx`/`mx` stehen in Bildschirmpixeln, also mit der
        // GUI-Skalierung umgerechnet (pixels_per_meter ist DOLL_SIZE mal genau dieser Skala).
        let gui_scale = rect.pixels_per_meter / inventory::DOLL_SIZE;
        let i = ((cx - mx) / (40.0 * gui_scale)).atan();
        let j = ((cy - my) / (40.0 * gui_scale)).atan();
        // Vanilla setzt bodyYaw = 180 + i*20 und schaut die Figur durch einen gespiegelten
        // Matrix-Stack an (scale(size, size, -size) zusammen mit rotateZ(PI)). Netto ist das
        // dasselbe wie eine zusaetzliche halbe Drehung um die Y-Achse -- diese Kamera spiegelt
        // NICHT (s. doll_camera), also steckt die halbe Drehung hier im Winkel:
        // 180 + i*20 + 180 = i*20.
        let body_yaw = i * 20.0;
        // headYaw - bodyYaw: der Kopf dreht sich doppelt so weit wie der Rumpf, die Pose
        // braucht nur die DIFFERENZ (s. biped_pose_full). Ein RELATIVER Winkel, den die halbe
        // Drehung oben nicht beruehrt.
        let head_rel = i * 20.0;
        let pitch = -j * 20.0;

        let (model, tex) = self.own_player_model_and_texture();
        let (main, off) = self.doll_hand_items();
        let pose = world::entity_model::biped_pose_full(
            head_rel,
            pitch,
            // Die Figur im Inventar steht still: kein Laufschwingen, kein Schlag. Vanilla
            // zeichnet zwar das echte Spieler-Entity, aber mit angehaltenen Animationen --
            // `limbDistance`/`limbAngle` bleiben auf ihrem Stand, und in Ruhe sind sie null.
            0.0,
            0.0,
            world::entity_model::BipedExtras {
                // Die Figur im Inventar sitzt mit, wenn man reitet (Vanilla zeichnet dort das
                // echte Spieler-Entity).
                riding: self.own_ride.is_some(),
                age: self.own_age_ticks,
                item_right: main.is_some(),
                item_left: off.is_some(),
                block_right: blocking_arms(self.own_using_item_right_arm(), (main, off)).0,
                block_left: blocking_arms(self.own_using_item_right_arm(), (main, off)).1,
                spear_right: is_spear_item(main),
                spear_left: is_spear_item(off),
                ..Default::default()
            },
        );
        let base = world::entity_transform([0.0, 0.0, 0.0], body_yaw, 1.0, world::entity_model::PLAYER_MODEL_SCALE);

        let mut out = Vec::with_capacity(512);
        world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
        self.emit_armor(&self.own_armor(), &self.armor_models, &pose, base, &mut out);
        self.emit_held_items((main, off), blocking_arms(self.own_using_item_right_arm(), (main, off)), (0.0, true), model, &pose, base, &mut out);
        if let Some(cape_tex) = self.own_uuid.and_then(|uuid| self.cape_texture(&uuid)) {
            // Der Umhang haengt in Ruhe gerade nach unten -- alle drei Winkel null.
            let cape_pose = world::cape_pose(&pose, 0.0, 0.0, 0.0);
            world::emit_entity_model(&self.cape_model, &cape_pose, base, cape_tex, ENTITY_BRIGHTNESS, &mut out);
        }

        let (view, proj) = doll_camera(rect, screen);
        Some((out, view, proj))
    }

    // Vanillas schwebendes Pop-Totem (InGameOverlayRenderer.renderFloatingItem), 1:1: im
    // Sichtraum mit dem Hand-Sichtfeld von 70 Grad, deshalb im selben Durchgang wie die Hand
    // -- aber in JEDER Perspektive, wie in Vanilla.
    pub(crate) fn emit_floating_totem(&self, out: &mut Vec<world::BlockVertex>) {
        let Some((ticks, off_x, off_y)) = self.floating_totem else { return };
        let Some(totem) = world::item_registry::item_id_by_name("totem_of_undying") else { return };
        let tick_progress = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0);
        let i = (modules::TOTEM_ANIMATION_TICKS - ticks) as f32;
        let f = (i + tick_progress) / modules::TOTEM_ANIMATION_TICKS as f32;
        let k = modules::animation_phase(f);
        let aspect = self.gpu.as_ref().map(|g| g.size().0 / g.size().1.max(1.0)).unwrap_or(16.0 / 9.0);
        let m = off_x * 0.3 * aspect;
        let n = off_y * 0.3;
        let schwung = (k * 2.0).sin().abs();
        use world::entity_model::Affine;
        let pre = Affine::translation([m * schwung, n * schwung, -10.0 + 9.0 * k.sin()])
            .then(Affine::scale([0.8; 3]))
            .then(Affine::rotation_y_deg(900.0 * k.sin().abs()))
            .then(world::rotation_x_deg(6.0 * (f * 8.0).cos()))
            .then(Affine::rotation_axis_angle((6.0 * (f * 8.0).cos()).to_radians(), [0.0, 0.0, 1.0]));
        // Volle Helligkeit wie Vanilla (Licht 15728880).
        self.held_items.emit_fixed(totem, pre, 1.0, out);
    }

    pub(crate) fn build_hand_vertices(&self) -> Vec<world::BlockVertex> {
        // GameRenderer.renderItemInHand: nicht als Zuschauer.
        if self.mode != AppMode::World || self.is_dead || self.game_mode == GAME_MODE_SPECTATOR {
            return Vec::new();
        }
        // Versteckter HUD (F1) nimmt wie in Vanilla auch die Hand weg -- das Totem bleibt, es
        // ist eine Meldung, kein HUD. Beim Verbinden ebenso keine Hand.
        if !self.perspective.is_first_person() || self.hud_hidden || self.connecting_view() {
            let mut out = Vec::new();
            self.emit_floating_totem(&mut out);
            return out;
        }
        let tick_progress = (self.physics_accum / physics::TICK_DT).clamp(0.0, 1.0);
        let swing_progress = self.own_hand_swing(tick_progress);
        let equip_progress = self.own_last_equip_progress + (self.own_equip_progress - self.own_last_equip_progress) * tick_progress;

        // 1. Nachschwingen.
        let render_yaw = world::lerp_angle_degrees(tick_progress, self.own_last_render_yaw, self.own_render_yaw);
        let render_pitch = self.own_last_render_pitch + (self.own_render_pitch - self.own_last_render_pitch) * tick_progress;
        let sway = world::rotation_x_deg((self.look_pitch_deg() - render_pitch) * 0.1)
            .then(world::Affine::rotation_y_deg(world::wrap_degrees(self.camera.yaw - render_yaw) * 0.1));

        let mut out = Vec::with_capacity(128);

        // ENTWEDER Arm ODER Item -- nicht beides. Vanilla zeichnet den nackten Arm nur bei
        // LEERER Hand (HeldItemRenderer.renderFirstPersonItem ruft dann renderArmHoldingItem,
        // sonst geht es in den Item-Zweig). Wer ein Schwert
        // haelt, sieht in Vanilla das Schwert und keinen Arm daneben.
        //
        // Nur die Haupthand: fuer die Nebenhand zeichnet Vanilla einen zweiten, gespiegelten
        // Arm -- den gibt es hier noch nicht (es wird genau EIN Arm gebaut).
        // Karten in der Hand (ItemInHandRenderer): mit leerer Nebenhand haelt die Haupthand sie mit
        // beiden Armen vor sich, sonst haengt sie einhaendig neben dem Arm.
        let karte = |hand| self.own_hand_stack(hand).and_then(|s| s.map_id);
        let neben_karte = karte(protocol::packets::HAND_OFF);
        let neben_leer = self.own_hand_items().1.is_none();
        match (self.own_hand_items().0, karte(protocol::packets::HAND_MAIN)) {
            (Some(_), Some(id)) if neben_leer => self.emit_two_handed_map(id, sway, equip_progress, swing_progress, &mut out),
            (Some(_), Some(id)) => self.emit_one_handed_map(id, sway, true, equip_progress, swing_progress, &mut out),
            (None, _) => {
                // 2. + 3.
                let base = sway.then(world::first_person_arm_transform(true, equip_progress, swing_progress));
                let (model, tex) = self.own_hand_model_and_texture();
                let pose = world::first_person_arm_pose(true);
                world::emit_entity_model(model, &pose, base, tex, ENTITY_BRIGHTNESS, &mut out);
            }
            (Some(main), None) => {
                // Das Item haengt NICHT am Armmodell: Vanilla setzt es mit einer eigenen
                // Kette in denselben Sichtraum (s. world::first_person_item_transform).
                let benutzt = self.own_using_item_right_arm() == Some(true);
                let f = self.own_use_ticks as f32 + tick_progress;
                let item_base = sway.then(self.first_person_item_pose(protocol::packets::HAND_MAIN, true, benutzt, f, equip_progress, swing_progress));
                // Blockt die HAUPThand gerade? (Rechtshaendig gedacht wie ueberall hier.)
                let (block_main, _) = blocking_arms(self.own_using_item_right_arm(), self.own_hand_items());
                self.emit_first_person_item(main, item_base, true, block_main, protocol::packets::HAND_MAIN, benutzt, f, &mut out);
            }
        }

        // DIE NEBENHAND. Vanilla zeichnet sie als eigenen, gespiegelten Durchgang
        // (HeldItemRenderer.renderFirstPersonItem mit Arm.LEFT) -- bisher fehlte sie hier ganz,
        // und damit auch jedes Schild in der Ego-Perspektive. Schilde liegen praktisch immer in der Nebenhand.
        //
        // BEWUSSTE VEREINFACHUNG: Anlege- und Schlagfortschritt stehen auf null. Beide werden
        // in diesem Client nur fuer die HAUPThand gefuehrt (ein Zaehler, s. own_equip_progress
        // / own_hand_swing); den der Haupthand einfach mitzubenutzen waere schlechter als
        // keiner -- das Schild wippte dann bei jedem Hotbar-Wechsel und bei jedem Schlag mit,
        // obwohl sich in der Nebenhand gar nichts geaendert hat.
        if let Some(id) = neben_karte {
            self.emit_one_handed_map(id, sway, false, 0.0, 0.0, &mut out);
        } else if let Some(off) = self.own_hand_items().1 {
            let benutzt = self.own_using_item_right_arm() == Some(false);
            let f = self.own_use_ticks as f32 + tick_progress;
            let item_base = sway.then(self.first_person_item_pose(protocol::packets::HAND_OFF, false, benutzt, f, 0.0, 0.0));
            let (_, block_off) = blocking_arms(self.own_using_item_right_arm(), self.own_hand_items());
            self.emit_first_person_item(off, item_base, false, block_off, protocol::packets::HAND_OFF, benutzt, f, &mut out);
        }
        self.emit_floating_totem(&mut out);
        out
    }

    // ItemInHandRenderer.renderTwoHandedMap: die Karte mittig vor sich, nach dem Blick nach unten
    // gekippt (calculateMapTilt), beide Arme darunter.
    pub(crate) fn emit_two_handed_map(&self, id: i32, sway: world::Affine, equip: f32, swing: f32, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_x_deg, Affine};
        let pi = std::f32::consts::PI;
        let wurzel = swing.max(0.0).sqrt();
        let (a, b) = (-0.2 * (swing * pi).sin(), -0.4 * (wurzel * pi).sin());
        let neigung = map_tilt(self.look_pitch_deg());
        let basis = sway
            .then(Affine::translation([0.0, -a / 2.0, b]))
            .then(Affine::translation([0.0, 0.04 + equip * -1.2 + neigung * -0.5, -0.72]))
            .then(rotation_x_deg(neigung * -85.0));
        if !self.own_invisible {
            let arme = basis.then(Affine::rotation_y_deg(90.0));
            self.emit_map_hand(arme, true, out);
            self.emit_map_hand(arme, false, out);
        }
        let karte = basis.then(rotation_x_deg((wurzel * pi).sin() * 20.0)).then(Affine::scale([2.0; 3]));
        self.emit_map_in_hand(id, karte, out);
    }

    // ItemInHandRenderer.renderOneHandedMap: der Arm leicht nach aussen gerollt, die Karte daneben.
    pub(crate) fn emit_one_handed_map(&self, id: i32, sway: world::Affine, rechts: bool, equip: f32, swing: f32, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_x_deg, rotation_z_deg, Affine};
        let pi = std::f32::consts::PI;
        let s = if rechts { 1.0 } else { -1.0 };
        let basis = sway.then(Affine::translation([s * 0.125, -0.125, 0.0]));
        if !self.own_invisible {
            let arm = basis.then(rotation_z_deg(s * 10.0)).then(world::first_person_arm_transform(rechts, equip, swing));
            let (_, tex) = self.own_hand_model_and_texture();
            let modell = world::player_arm_model(self.own_slim(), rechts);
            world::emit_entity_model(&modell, &world::first_person_arm_pose(rechts), arm, tex, ENTITY_BRIGHTNESS, out);
        }
        let wurzel = swing.max(0.0).sqrt();
        let f10 = (wurzel * pi).sin();
        let karte = basis
            .then(Affine::translation([s * 0.51, -0.08 + equip * -1.2, -0.75]))
            .then(Affine::translation([s * -0.5 * f10, 0.4 * (wurzel * 2.0 * pi).sin() - 0.3 * f10, -0.3 * (swing * pi).sin()]))
            .then(rotation_x_deg(f10 * -45.0))
            .then(Affine::rotation_y_deg(s * f10 * -30.0));
        self.emit_map_in_hand(id, karte, out);
    }

    // ItemInHandRenderer.renderMapHand: ein Arm unter der beidhaendig gehaltenen Karte.
    pub(crate) fn emit_map_hand(&self, basis: world::Affine, rechts: bool, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_x_deg, rotation_z_deg, Affine};
        let s = if rechts { 1.0 } else { -1.0 };
        let m = basis
            .then(Affine::rotation_y_deg(92.0))
            .then(rotation_x_deg(45.0))
            .then(rotation_z_deg(s * -41.0))
            .then(Affine::translation([s * 0.3, -1.1, 0.45]));
        let (_, tex) = self.own_hand_model_and_texture();
        let modell = world::player_arm_model(self.own_slim(), rechts);
        world::emit_entity_model(&modell, &world::first_person_arm_pose(rechts), m, tex, ENTITY_BRIGHTNESS, out);
    }

    // ItemInHandRenderer.renderMap: Hintergrund (mit Daten das Schachbrett) und das Kartenbild
    // samt Markierungen, 0.38 gross und um die Mitte gelegt.
    pub(crate) fn emit_map_in_hand(&self, id: i32, basis: world::Affine, out: &mut Vec<world::BlockVertex>) {
        use world::entity_model::{rotation_z_deg, Affine};
        let m = basis
            .then(Affine::rotation_y_deg(180.0))
            .then(rotation_z_deg(180.0))
            .then(Affine::scale([0.38; 3]))
            .then(Affine::translation([-0.5, -0.5, 0.0]))
            .then(Affine::scale([1.0 / 128.0; 3]));
        let bild = self.map_rect(id, false);
        let grund = if self.maps.get(id).is_some() { "map/map_background_checkerboard" } else { "map/map_background" };
        if let Some([u0, v0, u1, v1]) = self.entity_atlas.rect(grund) {
            let ecke = |x: f32, y: f32, u: f32, v: f32| world::BlockVertex { position: m.apply([x, y, 0.0]), color: [ENTITY_BRIGHTNESS; 3], uv: [u, v] };
            let (a, b, c, d) = (ecke(-7.0, 135.0, u0, v1), ecke(135.0, 135.0, u1, v1), ecke(135.0, -7.0, u1, v0), ecke(-7.0, -7.0, u0, v0));
            out.extend([a, b, c, a, c, d]);
        }
        if let Some(rect) = bild {
            push_map_quad(m, rect, ENTITY_BRIGHTNESS, out);
        }
    }

    // Ein Item in der eigenen Hand (Ego-Ansicht). Trennt nur den Sonderfall Schild ab, das
    // ueber ein Entity-Modell statt sein Item-Modell gezeichnet wird (s. emit_shield).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_first_person_item(
        &self,
        item: world::HeldItem,
        base: world::entity_model::Affine,
        right_hand: bool,
        blocking: bool,
        hand: i32,
        benutzt: bool,
        use_ticks: f32,
        out: &mut Vec<world::BlockVertex>,
    ) {
        if world::item_registry::item_name(item.item_id) == Some("shield") {
            self.emit_shield(base, right_hand, true, blocking, item.enchanted, item.banner, out);
            return;
        }
        // Gespannter Bogen / geladene Armbrust: ein ANDERES Modell, kein anderer Winkel
        // (s. hand_item_model). Solange es noch nicht geladen ist, bleibt es beim Grundmodell
        // -- besser ein Bild zu spaet als ein Frame lang eine leere Hand.
        if let Some(modell) = self.hand_model_of(hand, benutzt, use_ticks) {
            let modell = modell.as_str();
            if self.held_items.has_model(modell) {
                let start = out.len();
                self.held_items.emit_first_person_model(modell, base, right_hand, ENTITY_BRIGHTNESS, out);
                if item.enchanted {
                    self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                        self.held_items.emit_first_person_model_local(modell, base, right_hand, l)
                    });
                }
                return;
            }
        }
        let start = out.len();
        self.held_items.set_skull(self.held_skull_texture(&item));
        self.held_items.emit_first_person(item.model_id, item.potion_color, base, right_hand, ENTITY_BRIGHTNESS, out);
        if item.enchanted {
            self.push_glint(out, start, render::glint::GLINT_SCALE_ITEM, render::glint::GLINT_TEXTURE_ITEM, |l| {
                self.held_items.emit_first_person_local(item.model_id, item.potion_color, base, right_hand, l)
            });
        }
    }

    // Das Zustandsmodell des Items in dieser Hand, falls es eines gibt (s. hand_item_model).
    // Welches Modell die eigene Hand gerade zeigt, wenn es nicht das gewoehnliche des Items ist:
    // die Item-Definition mit Benutzung (Spannstufen), Ladung und Components ausgewertet und mit
    // dem Ergebnis eines schlichten Stapels desselben Items verglichen. None = das gewoehnliche
    // (dafuer hat HeldItems den vollstaendigen Weg mit Traenken, Koepfen und Sondermodellen).
    pub(crate) fn hand_model_of(&self, hand: i32, benutzt: bool, use_ticks: f32) -> Option<String> {
        let stack = self.own_hand_stack(hand)?;
        let _name = world::item_registry::item_name(stack.item_id)?;
        let kontext = if hand == protocol::packets::HAND_MAIN { "firstperson_righthand" } else { "firstperson_lefthand" };
        let benutzung = benutzt.then(|| item_model_input::UseState {
            ticks: use_ticks,
            duration: world::item_behavior::consume_ticks(stack).unwrap_or(72_000) as f32,
            pull_ticks: self.enchantments.crossbow_pull_ticks(stack) as f32,
        });
        self.model_override(stack, kontext, benutzung)
    }

    // Das Modell, das die Item-Definition fuer diesen Stapel an `kontext` waehlt -- None, wenn
    // es dasselbe ist wie bei einem schlichten Stapel desselben Items (dann bleibt es beim
    // vollstaendigen Weg in HeldItems mit Traenken, Koepfen und Sondermodellen).
    pub(crate) fn model_override(&self, stack: &protocol::item::ItemStack, kontext: &str, benutzung: Option<item_model_input::UseState>) -> Option<String> {
        let name = world::item_registry::item_name(stack.item_id)?;
        let def = self.item_icons.definition(&model_definition_id(stack, name))?;
        let mut input = self.item_model_input(stack, kontext);
        input.using = benutzung;
        let zweige = def.evaluate(&input);
        let mut schlicht = protocol::item::ItemStack::new(stack.item_id, 1);
        schlicht.item_model = stack.item_model.clone();
        if zweige == def.evaluate(&self.item_model_input(&schlicht, kontext)) {
            return None;
        }
        let blatt = def.leaves.get(*zweige.first()? as usize)?;
        blatt.special.is_none().then(|| blatt.model.clone())
    }

    // Dasselbe fuer ein Item in der Hand eines Entities (3. Person).
    pub(crate) fn held_model_override(&self, item: &world::HeldItem, right: bool) -> Option<String> {
        let stack = world::item_components::resolve(item.stack)?;
        self.model_override(&stack, if right { "thirdperson_righthand" } else { "thirdperson_lefthand" }, None)
    }

    // Wie das Item dieser Hand in der Ego-Ansicht liegt. Ohne laufende Benutzung ist das
    // Vanillas gewoehnliche Kette (Anlegen + Schwung); Bogen und Armbrust haben eigene
    // Haltungen (HeldItemRenderer.renderFirstPersonItem).
    //
    // Vanilla behandelt die
    // Armbrust in einem EIGENEN Zweig -- beim Laden liegt sie quer vor der Kamera, geladen
    // haelt die Haupthand sie ein Stueck nach aussen gedreht, damit der Pfeil sichtbar ist.
    pub(crate) fn first_person_item_pose(
        &self,
        hand: i32,
        right_hand: bool,
        benutzt: bool,
        use_ticks: f32,
        equip_progress: f32,
        swing_progress: f32,
    ) -> world::entity_model::Affine {
        let normal = || world::entity_model::first_person_item_transform(right_hand, equip_progress, swing_progress);
        let Some(stack) = self.own_hand_stack(hand) else { return normal() };
        let Some(name) = world::item_registry::item_name(stack.item_id) else { return normal() };
        if benutzt {
            if let Some(dauer) = world::item_behavior::consume_ticks(stack) {
                // Vanilla rechnet mit der VERBLEIBENDEN Zeit (getItemUseTimeLeft - Bild + 1),
                // hier zaehlt use_ticks aufwaerts.
                let dauer = dauer as f32;
                let rest = dauer - use_ticks + 1.0;
                return world::entity_model::first_person_eat_pose(right_hand, equip_progress, rest, dauer);
            }
        }
        // Speer: Rechtsklick halten = Ansturm (UseAction.SPEAR), sonst sticht er statt zu
        // schwingen (SwingAnimationType.STAB), s. world::first_person_stab_transform.
        if let Some(kinetik) = world::item_behavior::kinetic(stack) {
            if benutzt {
                return world::entity_model::first_person_spear_charge(right_hand, use_ticks, kinetik);
            }
            return world::entity_model::first_person_stab_transform(right_hand, equip_progress, swing_progress);
        }
        match name {
            "crossbow" => {
                let geladen = stack.is_charged();
                if benutzt && !geladen {
                    let pull = crossbow_pull_progress(use_ticks, self.enchantments.crossbow_pull_ticks(stack) as f32).min(1.0);
                    return world::entity_model::first_person_crossbow_pull_pose(right_hand, equip_progress, use_ticks, pull);
                }
                // Geladen und nicht am Schlagen: Vanilla nur fuer die HAUPThand.
                let basis = normal();
                if geladen && swing_progress < 0.001 && hand == protocol::packets::HAND_MAIN {
                    basis.then(world::entity_model::first_person_crossbow_charged_offset(right_hand))
                } else {
                    basis
                }
            }
            "bow" if benutzt => {
                // Vanillas Spannkurve: g = (g*g + 2g) / 3 auf f/20.
                let t = use_ticks / 20.0;
                let pull = ((t * t + t * 2.0) / 3.0).min(1.0);
                world::entity_model::first_person_bow_pose(right_hand, equip_progress, use_ticks, pull)
            }
            _ => normal(),
        }
    }

    // Arm-Modell + Skin-Rechteck, nach derselben Regel wie das ganze Spielermodell.
    // Hat der eigene Skin schmale Arme?
    pub(crate) fn own_slim(&self) -> bool {
        let uuid = self.own_uuid.unwrap_or([0; 16]);
        self.skins.get(&uuid).is_some_and(|s| s.slim)
    }

    pub(crate) fn own_hand_model_and_texture(&self) -> (&world::Model, [f32; 4]) {
        const KEIN_SPIELER: [u8; 16] = [0; 16];
        let uuid = self.own_uuid.unwrap_or(KEIN_SPIELER);
        let skin = self.skins.get(&uuid).copied();
        let slim = skin.is_some_and(|s| s.slim);
        let model = if slim { &self.hand_model_slim } else { &self.hand_model_wide };
        let tex = skin
            .and_then(|s| s.slot)
            .and_then(|slot| self.entity_atlas.skin_slot_rect(slot))
            .or_else(|| self.entity_atlas.rect(assets::MISSING_SKIN))
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        (model, tex)
    }

    // Modell + Textur fuer den eigenen Spieler (3.-Person-Ansicht). Dieselbe Suche wie bei
    // Fremden, nur mit der eigenen UUID. Solange die noch nicht da ist (Login-Success kommt
    // vor dem Play-State) oder der Server keine Textur-Property schickt (Offline-Mode),
    // steht schwarz-lila da (s. player_model_and_texture).
    pub(crate) fn own_player_model_and_texture(&self) -> (&world::Model, [f32; 4]) {
        // Die Null-UUID kann kein Spieler sein, ist also garantiert ein Fehlschlag in
        // `skins` -- damit ist "UUID noch unbekannt" derselbe Pfad wie "kein Skin geladen".
        const KEIN_SPIELER: [u8; 16] = [0; 16];
        self.player_model_and_texture(self.own_uuid.as_ref().unwrap_or(&KEIN_SPIELER))
    }

    // Atlas-Rechteck des Umhangs eines Spielers, oder None wenn er keinen hat (oder er noch
    // nicht geladen ist). Kein Standard-Umhang -- ohne Cape-Property gibt es keinen, und
    // Vanilla zeichnet dann auch keinen.
    pub(crate) fn cape_texture(&self, uuid: &[u8; 16]) -> Option<[f32; 4]> {
        let slot = self.skins.get(uuid)?.cape_slot?;
        self.entity_atlas.cape_slot_rect(slot)
    }

    // Modell + Atlas-Rechteck fuer einen fremden Spieler: sein eigener Skin-Platz, wenn
    // geladen, sonst schwarz-lila -- bewusst kein Standard-Skin, der saehe aus wie ein echter.
    // Das MODELL (slim/classic) haengt an der Textur-Property und gilt auch ohne geladenes
    // Bild; ohne Property classic.
    pub(crate) fn player_model_and_texture(&self, uuid: &[u8; 16]) -> (&world::Model, [f32; 4]) {
        let skin = self.skins.get(uuid).copied();
        let slim = skin.is_some_and(|s| s.slim);
        let model = if slim { &self.player_slim } else { &self.player_wide };
        let tex = skin
            .and_then(|s| s.slot)
            .and_then(|slot| self.entity_atlas.skin_slot_rect(slot))
            .or_else(|| self.entity_atlas.rect(assets::MISSING_SKIN))
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        (model, tex)
    }

    // Einmal pro Tick: jede Flugbahn um die aktuelle Position ihres Pfeils verlaengern und
    // abgelaufene Punkte abschneiden. Jeder Punkt altert fuer sich (Tick seiner Aufnahme) --
    // die Spur wird also vom alten Ende her aufgeraeumt, als liefe ein zweiter Pfeil hinterher.
    // Ist der Pfeil einmal gelandet, kommt nie wieder etwas dazu, auch wenn er danach noch
    // einmal losfliegt (z. B. weil der Block unter ihm abgebaut wird).
    pub(crate) fn tick_trails(&mut self) {
        self.trail_tick += 1;
        if !(self.settings.module_arrows && self.settings.arrow_trail) {
            self.trails.clear();
            return;
        }
        let jetzt = self.trail_tick;
        let grenze = self.trail_lifetime_ticks();
        for t in self.trails.values_mut() {
            t.lebt = false;
        }
        for (id, e) in self.entities.iter() {
            if arrow_texture(e.entity_type).is_none() {
                continue;
            }
            let t = self.trails.entry(*id).or_default();
            t.lebt = true;
            if t.gelandet {
                continue;
            }
            // Mit dem Abschuss-Versatz: die Spur beginnt dort, wo man den Pfeil SIEHT
            // (s. world::Entity::launch_render_offset), nicht an seiner Rechenposition.
            let (x, y, z) = e.render_pos(1.0);
            let v = e.launch_render_offset(1.0);
            let pos = DVec3::new(x + v[0] as f64, y + v[1] as f64, z + v[2] as f64);
            if t.punkte.back().is_none_or(|(p, _)| p.distance_squared(pos) > 1e-6) {
                t.punkte.push_back((pos, jetzt));
            }
            // Ein Pfeil, der schon steckt, als man ihn zum ersten Mal sieht, bekommt so gar
            // keine Spur (ein einzelner Punkt zeichnet nichts).
            if e.in_ground() {
                t.gelandet = true;
            }
        }
        let alter = |tick: u64| jetzt.saturating_sub(tick) as f32;
        self.trails.retain(|_, t| {
            // Der vorderste Punkt bleibt, solange das Stueck zum naechsten noch sichtbar ist --
            // es wird beim Zeichnen genau an der Ablaufgrenze abgeschnitten (s. emit_trails).
            while t.punkte.len() >= 2 && alter(t.punkte[1].1) >= grenze {
                t.punkte.pop_front();
            }
            if t.punkte.len() == 1 && alter(t.punkte[0].1) >= grenze && (t.gelandet || !t.lebt) {
                t.punkte.pop_front();
            }
            t.lebt || !t.punkte.is_empty()
        });
    }

    // Wie lange ein Spurpunkt insgesamt lebt: Standzeit + Ausblenden, in Ticks.
    pub(crate) fn trail_lifetime_ticks(&self) -> f32 {
        (self.settings.trail_hold_tenths + self.settings.trail_fade_tenths) as f32 * 2.0
    }

    // Die Flugbahnen als Baender, die sich zur Kamera drehen. Ausgeblendet wird ueber die
    // Breite (die Entity-Pipeline mischt nicht): jeder Punkt steht erst die eingestellte Zeit
    // in voller Breite und laeuft dann ueber die Fade-Dauer auf null. Die weisse Atlas-Kachel
    // mit voller Vertexfarbe ergibt reines Weiss.
    pub(crate) fn emit_trails(&self, eye: DVec3, progress: f32, out: &mut Vec<world::BlockVertex>) {
        if self.trails.is_empty() {
            return;
        }
        let [u0, v0, u1, v1] = self.entity_atlas.white_rect();
        let uv = [(u0 + u1) / 2.0, (v0 + v1) / 2.0];
        let halten = self.settings.trail_hold_tenths as f32 * 2.0;
        let fade = self.settings.trail_fade_tenths as f32 * 2.0;
        let grenze = halten + fade;
        let breite = |alter: f32| {
            let f = if alter <= halten {
                1.0
            } else if fade > 0.0 {
                1.0 - (alter - halten) / fade
            } else {
                0.0
            };
            TRAIL_WIDTH * 0.5 * f.clamp(0.0, 1.0)
        };
        let jetzt = self.trail_tick as f32 + progress;
        for (id, t) in &self.trails {
            let mut punkte: Vec<(Vec3, f32)> = t.punkte.iter().map(|(p, tick)| ((*p - eye).as_vec3(), jetzt - *tick as f32)).collect();
            if t.lebt && !t.gelandet {
                if let Some(e) = self.entities.get(*id) {
                    let (x, y, z) = e.render_pos(progress);
                    let v = e.launch_render_offset(progress);
                    punkte.push(((DVec3::new(x, y, z) - eye).as_vec3() + Vec3::from(v), 0.0));
                }
            }
            for paar in punkte.windows(2) {
                let ((mut a, mut alter_a), (b, alter_b)) = (paar[0], paar[1]);
                if alter_b >= grenze {
                    continue;
                }
                // Das aelteste Stueck genau an der Ablaufgrenze abschneiden: so wandert das
                // Ende gleichmaessig, statt Stueck fuer Stueck wegzuspringen.
                if alter_a > grenze {
                    let t = (alter_a - grenze) / (alter_a - alter_b);
                    a += (b - a) * t;
                    alter_a = grenze;
                }
                // Quer zur Flugrichtung UND zur Blickrichtung -- so zeigt das Band immer seine
                // Breitseite zur Kamera.
                let quer = (b - a).cross((a + b) * 0.5);
                if quer.length_squared() < 1e-12 {
                    continue;
                }
                let quer = quer.normalize();
                let (qa, qb) = (quer * breite(alter_a), quer * breite(alter_b));
                let ecke = |p: Vec3| world::BlockVertex { position: [p.x, p.y, p.z], color: [1.0; 3], uv };
                let (p1, p2, p3, p4) = (ecke(a - qa), ecke(a + qa), ecke(b + qb), ecke(b - qb));
                // Beide Wicklungen: die Pipeline verwirft Rueckseiten.
                out.extend_from_slice(&[p1, p2, p3, p1, p3, p4, p1, p3, p2, p1, p4, p3]);
            }
        }
    }

    // Der eigene Schild-Cooldown (Schild-Modul): Rest in Ticks und verbleibender Anteil 0..1,
    // oder None ohne Cooldown bzw. wenn die Anzeige gerade nicht passt.
    // Modul "Timer": Anteil 0..1 des Kreises. Beim Essen fuellt er sich bis "fertig". Bogen und
    // Armbrust laufen andersherum: der Kreis LEERT sich und ist weg, sobald der Bogen ganz
    // gespannt (kritischer Pfeil) bzw. die Armbrust geladen ist -- beim Zielen stoert dann
    // nichts mehr am Fadenkreuz (gewuenscht). None = kein Kreis.
    pub(crate) fn use_timer_ring(&self) -> Option<f32> {
        if !self.settings.module_use_timer || self.is_dead || self.inventory.screen_open || self.hud_hidden {
            return None;
        }
        let (name, stack) = self.own_used_item()?;
        let pull = stack.as_ref().map_or(25, |st| self.enchantments.crossbow_pull_ticks(st));
        let voll = consumable_optimizer::timer_ticks(name, stack.as_ref(), pull)?;
        let fortschritt = if voll == 0 { 1.0 } else { ((self.own_use_ticks as f32 + self.tick_progress()) / voll as f32).min(1.0) };
        if matches!(name, "bow" | "crossbow") {
            // 0 = gespannt: kein Kreis, aber auch kein Schild-Cooldown bis zum Loslassen.
            return Some(1.0 - fortschritt);
        }
        Some(fortschritt)
    }

    pub(crate) fn own_shield_cooldown(&self) -> Option<(f32, f32)> {
        if !(self.settings.module_shields && self.settings.shield_cd_self) || self.is_dead || self.inventory.screen_open || self.hud_hidden {
            return None;
        }
        let shield = protocol::item::ItemStack::new(world::item_registry::item_id_by_name("shield")?, 1);
        let rest = self.item_cooldowns.remaining_ticks(&shield, self.tick_progress());
        (rest > 0.0).then(|| (rest, self.item_cooldowns.progress(&shield, self.tick_progress())))
    }

    // Entity-Ereignis vom Server. Bisher interessiert nur eines davon: das gepoppte Totem am
    // EIGENEN Spieler. Fremde Pops (Status 35 an einer anderen EntityId) werden verworfen --
    // die Hand eines anderen Spielers koennte man ohnehin nicht bestimmen, dessen Haende
    // kennen wir nicht.
    // Die Partikel-Quads dieses Bildes in self.particle_vertices. Sie liegen in der Bildebene
    // der Kamera (Vanillas Rotator.ALL_AXIS: die Kameradrehung selbst).
    pub(crate) fn build_particle_vertices(&mut self) -> render::ParticleDraws {
        let forward = self.camera.view_forward();
        // Die Rechte aus dem Gierwinkel statt aus forward x Y: bei senkrechtem Blick waere das
        // Kreuzprodukt null. Die Frontansicht blickt um 180 Grad gedreht (s. view_forward).
        let yaw = (self.camera.yaw + if self.camera.view_inverted { 180.0 } else { 0.0 }).to_radians();
        let right = Vec3::new(-yaw.cos(), 0.0, -yaw.sin());
        let up = right.cross(forward).normalize_or_zero();
        let tick_progress = self.tick_progress();
        let mut out = std::mem::take(&mut self.particle_vertices);
        self.particles.set_tuning(self.settings.particle_tuning);
        let mut draws = self.particles.build_vertices(self.camera.position, right, up, tick_progress, &mut out);
        // XP-Kugeln ans Ende der durchscheinenden Partikel (gleicher Atlas, gleiche Mischung).
        let kugeln = self.experience_orb_vertices(right, up, tick_progress);
        let an = (draws.opaque + draws.translucent) as usize;
        draws.translucent += kugeln.len() as u32;
        out.splice(an..an, kugeln);
        self.particle_vertices = out;
        draws
    }

    // ExperienceOrbRenderer: ein zur Kamera gedrehtes Viereck, 0,3 Bloecke breit, 0,1 ueber
    // der Kugelposition und nach oben versetzt (das Bild reicht von -0,25 bis 0,75). Welches
    // der 16 Bilder, sagt der Wert der Kugel (ExperienceOrb.getIcon); die Farbe pulsiert mit
    // dem Alter zwischen Gruen und Gelb, halb durchsichtig.
    pub(crate) fn experience_orb_vertices(&self, right: Vec3, up: Vec3, tick_progress: f32) -> Vec<render::ParticleVertex> {
        let Some(&[u0, v0, u1, v1]) = self.particle_atlas.sprites("experience_orb").first() else { return Vec::new() };
        let mut out = Vec::new();
        for (_, e) in self.entities.iter() {
            if world::entity_type_name(e.entity_type) != Some("experience_orb") || e.invisible {
                continue;
            }
            let bild = experience_orb_icon(e.orb_value);
            let (du, dv) = ((u1 - u0) / 4.0, (v1 - v0) / 4.0);
            let (a, b) = (u0 + du * (bild % 4) as f32, v0 + dv * (bild / 4) as f32);
            let (x, y, z) = e.render_pos(tick_progress);
            let mitte = (DVec3::new(x, y + 0.1, z) - self.camera.position).as_vec3() + up * 0.075;
            let f = e.age_ticks(tick_progress) / 2.0;
            let farbe = [(f.sin() + 1.0) * 0.5, 1.0, ((f + 4.188_790_3).sin() + 1.0) * 0.1, 128.0 / 255.0];
            let (r, u) = (right * 0.15, up * 0.15);
            let ecken = [(mitte + r - u, [a + du, b + dv]), (mitte + r + u, [a + du, b]), (mitte - r + u, [a, b]), (mitte - r - u, [a, b + dv])];
            for i in [0, 1, 2, 0, 2, 3] {
                let (pos, uv) = ecken[i];
                out.push(render::ParticleVertex { position: pos.to_array(), color: farbe, uv });
            }
        }
        out
    }

    // Die Partikel zusammen mit allem, was sie aus der Welt lesen: Kamera, Stufe, Bloecke und
    // wo ein Entity steht (fuer die Emitter). Als Closure statt als Rueckgabewert, weil die
    // Welt nur geliehen werden kann, solange `self.particles` getrennt davon veraenderbar
    // bleibt.
    // LivingEntity.tickStatusEffects, Client-Zweig: jedes Lebewesen mit Trankeffekten streut
    // ab und zu einen Wirbel in einer seiner Effektfarben: mit Chance 1 / (i * j) je Tick,
    // i = 15 unsichtbar bzw. 4 sonst, j = 5 fuer Ambient-Effekte bzw. 1 sonst, an einer
    // zufaelligen Stelle des Koerpers.
    //
    // Fehlte frueher ganz.
    // AbstractArrow.tick im Client: ein fliegender Krit-Pfeil streut je Tick vier Krit-
    // Partikel entlang seiner Geschwindigkeit, jeder mit der Gegengeschwindigkeit und 0.2 nach
    // oben -- die Spur, an der man einen voll gespannten Schuss erkennt.
    pub(crate) fn tick_crit_trails(&mut self) {
        let spuren: Vec<_> = self.entities.iter().filter_map(|(_, e)| e.crit_trail()).collect();
        if spuren.is_empty() {
            return;
        }
        let krit = protocol::particle::ParticleEffect { type_id: protocol::particle::PARTICLE_CRIT, options: protocol::particle::ParticleOptions::None };
        self.with_particles(|p, ctx, _| {
            for ((x, y, z), (vx, vy, vz)) in spuren {
                for i in 0..4 {
                    let t = i as f64 / 4.0;
                    p.add_world_particle(&krit, DVec3::new(x + vx * t, y + vy * t, z + vz * t), DVec3::new(-vx, -vy + 0.2, -vz), ctx);
                }
            }
        });
    }

    pub(crate) fn tick_potion_swirls(&mut self) {
        let mut quellen: Vec<(Vec<u32>, bool, bool, DVec3, f64, f64)> = Vec::new();
        for (_, e) in self.entities.iter() {
            if e.potion_swirls.is_empty() {
                continue;
            }
            let (w, h) = e.hit_size();
            let (x, y, z) = e.hit_pos();
            quellen.push((e.potion_swirls.clone(), e.potion_swirls_ambient, e.invisible, DVec3::new(x, y, z), w, h));
        }
        if !self.own_potion_swirls.is_empty() {
            quellen.push((
                self.own_potion_swirls.clone(),
                self.own_potion_swirls_ambient,
                self.own_invisible,
                self.physics.pos,
                0.6,
                self.physics.current_height(),
            ));
        }
        if quellen.is_empty() {
            return;
        }
        self.with_particles(|p, ctx, _| {
            for (farben, ambient, unsichtbar, pos, w, h) in quellen {
                let i = if unsichtbar { 15 } else { 4 };
                let j = if ambient { 5 } else { 1 };
                if p.rng().next_int(i * j) != 0 {
                    continue;
                }
                let farbe = farben[p.rng().next_int(farben.len() as i32) as usize];
                let r = p.rng();
                let x = pos.x + w * (2.0 * r.next_double() - 1.0) * 0.5;
                let y = pos.y + h * r.next_double();
                let z = pos.z + w * (2.0 * r.next_double() - 1.0) * 0.5;
                p.add_potion_swirl(farbe, DVec3::new(x, y, z), ctx);
            }
        });
    }

    // AreaEffectCloud.clientTick fuer jede Trankwolke in der Welt (Verweiltrank, Drachenatem).
    pub(crate) fn tick_area_clouds(&mut self) {
        let wolken: Vec<(DVec3, f32, bool, protocol::particle::ParticleEffect)> = self
            .entities
            .iter()
            .filter(|(_, e)| world::entity_type_name(e.entity_type) == Some("area_effect_cloud"))
            .map(|(_, e)| {
                let (x, y, z) = e.pos;
                (DVec3::new(x, y, z), e.cloud_radius, e.cloud_waiting, e.cloud_particle.clone())
            })
            .collect();
        if wolken.is_empty() {
            return;
        }
        self.with_particles(|p, ctx, _| {
            for (pos, radius, wartet, partikel) in &wolken {
                p.tick_area_cloud(*pos, *radius, *wartet, partikel, ctx);
            }
        });
    }

    // Kruemel, waehrend andere essen (ItemStack.onUseTick -> Consumable): ab gut einem Fuenftel
    // der Essdauer alle vier Ticks fuenf Stueck vor dem Gesicht. Der Client rechnet das fuer
    // jeden Spieler selbst, der Server schickt dafuer nichts.
    pub(crate) fn tick_eating_particles(&mut self) {
        let mut quellen: Vec<(&'static str, DVec3, f32, f32)> = Vec::new();
        for (_, e) in self.entities.iter() {
            let Some((item, ticks)) = e.used_item() else { continue };
            let Some(stack) = world::item_components::resolve(item.stack) else { continue };
            let Some(dauer) = world::item_behavior::consume_ticks(&stack) else { continue };
            if !eating_crumbs_due(dauer, ticks) || !world::item_behavior::consume_particles(&stack) {
                continue;
            }
            let Some(name) = particle_item_name(&item) else { continue };
            let (x, y, z) = e.pos;
            quellen.push((name, DVec3::new(x, y + e.eye_height(), z), e.yaw_deg, e.pitch_deg));
        }
        if quellen.is_empty() {
            return;
        }
        self.with_particles(|p, ctx, _| {
            for (name, auge, yaw, pitch) in quellen {
                p.add_item_use_particles(name, 5, auge, yaw, pitch, ctx);
            }
        });
    }

    // Dasselbe fuer die eigene Figur; `fertig` = das Essen ist gerade beendet, dann sechzehn
    // Kruemel auf einmal (Consumable.onConsume).
    pub(crate) fn own_eating_particles(&mut self, fertig: bool) {
        let Some((_, Some(stack))) = self.own_used_item() else { return };
        let Some(dauer) = world::item_behavior::consume_ticks(&stack) else { return };
        if !world::item_behavior::consume_particles(&stack) || (!fertig && !eating_crumbs_due(dauer, self.own_use_ticks)) {
            return;
        }
        let Some(name) = particle_item_name(&world::HeldItem::from_stack(&stack)) else { return };
        let (auge, yaw, pitch) = (self.physics.eye_pos(), self.camera.yaw, self.look_pitch_deg());
        self.with_particles(|p, ctx, _| p.add_item_use_particles(name, if fertig { 16 } else { 5 }, auge, yaw, pitch, ctx));
    }

    // LivingEntity.breakItem (Entity-Ereignisse 47..52, 65, 68): ein Item ist zerbrochen. Der
    // Klang kommt nicht vom Server, jeder Client spielt ihn selbst -- dazu fuenf Splitter.
    pub(crate) fn item_broken(&mut self, entity_id: i32, status: i8) {
        let eigener = self.own_entity_id == Some(entity_id);
        let item = if eigener {
            let (haupt, zweit) = self.own_hand_items();
            let ruestung = self.own_armor();
            match status {
                47 => haupt,
                48 => zweit,
                49..=52 => ruestung[(52 - status) as usize],
                _ => None,
            }
        } else {
            let Some(e) = self.entities.get(entity_id) else { return };
            match status {
                47 => e.main_hand,
                48 => e.off_hand,
                49..=52 => e.armor[(52 - status) as usize],
                65 => e.body_item,
                _ => None,
            }
        };
        let Some(item) = item else { return };
        let (pos, auge, yaw, pitch) = if eigener {
            (self.physics.pos, self.physics.eye_pos(), self.camera.yaw, self.look_pitch_deg())
        } else {
            let Some(e) = self.entities.get(entity_id) else { return };
            let (x, y, z) = e.pos;
            (DVec3::new(x, y, z), DVec3::new(x, y + e.eye_height(), z), e.yaw_deg, e.pitch_deg)
        };
        self.type_pitch_seed = self.type_pitch_seed.wrapping_add(1);
        let tonhoehe = 0.8 + (zufallswert(self.type_pitch_seed) + 1.0) * 0.5 * 0.4;
        self.play_event_at_scaled("entity.item.break", pos, 0.8, tonhoehe, protocol::packets::SoundCategory::Players);
        if let Some(name) = particle_item_name(&item) {
            self.with_particles(|p, ctx, _| p.add_item_use_particles(name, 5, auge, yaw, pitch, ctx));
        }
    }

    // Snowball/ThrownEgg.handleEntityEvent(3): das Wurfgeschoss ist aufgeschlagen.
    pub(crate) fn thrown_item_hit(&mut self, entity_id: i32) {
        let Some(e) = self.entities.get(entity_id) else { return };
        let typ = world::entity_type_name(e.entity_type);
        let streuen = match typ {
            Some("snowball") => false,
            Some("egg") => true,
            _ => return,
        };
        let name = e.item.as_ref().and_then(particle_item_name).or(typ).unwrap_or("snowball");
        let (x, y, z) = e.pos;
        let pos = DVec3::new(x, y, z);
        self.with_particles(|p, ctx, _| p.add_thrown_item_hit(name, pos, streuen, ctx));
    }

    pub(crate) fn with_particles(
        &mut self,
        f: impl FnOnce(&mut particles::Particles, &particles::SpawnContext, &dyn Fn(i32) -> Option<particles::EmitterBody>),
    ) {
        self.particles.set_tuning(self.settings.particle_tuning);
        let ctx = particles::SpawnContext {
            camera: self.camera.view_position(),
            mode: particles::ParticleMode::from_index(self.settings.particles_mode),
            world: &self.world,
            shapes: &self.block_shapes,
            biomes: &self.biome_palette,
            player: Some(particles::NearPlayer { pos: self.physics.pos, vel_y: self.physics.vel.y }),
        };
        let entities = &self.entities;
        let own_id = self.own_entity_id;
        // Die eigene Figur steht nicht in `entities` (dort nur die anderen).
        let own = particles::EmitterBody { pos: self.physics.pos, width: self.physics.width(), height: self.physics.current_height() };
        let body = move |id: i32| -> Option<particles::EmitterBody> {
            if Some(id) == own_id {
                return Some(own);
            }
            let e = entities.get(id)?;
            let (width, height) = e.hit_size();
            Some(particles::EmitterBody { pos: DVec3::new(e.pos.0, e.pos.1, e.pos.2), width, height })
        };
        f(&mut self.particles, &ctx, &body);
        for t in self.particles.take_new_unsupported() {
            self.push_log(format!("Partikel noch nicht umgesetzt: {}", protocol::particle::particle_name(t).unwrap_or("?")));
        }
    }

    // Beschriftungen, die IN der Welt haengen: text_display-Entities und die Namensschilder
    // von Entities mit eigenem Namen (Entity.CUSTOM_NAME + NAME_VISIBLE).
    //
    // Beides zusammen ist das, was Server als "Hologramm" ausliefern: entweder ein
    // text_display oder -- klassisch und immer noch verbreitet -- ein unsichtbarer
    // Armorstand mit dauerhaft eingeblendetem Namen. Vorher zeigte dieser Client davon
    // nichts: der Text fehlte ganz, und der Traeger stand als orangefarbener Kasten da.
    //
    // SPIELERNAMEN laufen seit 2026-09-15 durch DENSELBEN Weg (s. world_label). Vorher gab es
    // dafuer einen zweiten Zeichenpfad, der die Schilder in fester Groesse und immer mit
    // Sichtlinien-Pruefung zeichnete -- beides wich von Vanilla ab, und der doppelte Pfad war
    // der Grund, warum die Regeln dort (unsichtbar, schleichend, Sichtweite) fehlten.
    //
    // Was daran NICHT Vanilla ist und bewusst so bleibt:
    //   - Die Schrift steht im Bildschirmraum, nicht als Flaeche in der Welt. Verdeckung
    //     muss deshalb wie bei den Namensschildern von Hand geprueft werden (s. sicht_frei),
    //     und die Warp-Projektionen erreichen sie nicht (derselbe Vorbehalt wie dort).
    //   - Der ganzzahlige Faktor springt in Stufen, statt stufenlos zu schrumpfen. Eine
    //     Bruchskalierung zerlegt einen Bitmap-Font sichtbar; die Stufe ist das kleinere
    //     Uebel.
    //
    // TEXT-DISPLAYS gehen seit 2026-09-19 einen eigenen Weg (s. build_display_text): Vanilla
    // legt sie mit Billboard-Modus und Drehungen als echte Flaeche in die Welt -- ein FIXED-
    // Display (der Vorgabewert!) steht wie ein Schild im Raum, statt sich zur Kamera zu
    // drehen. Nur Namensschilder stehen in Vanilla immer parallel zum Bildschirm
    // (LabelCommandRenderer: `multiply(cameraState.orientation)`), und genau das ist dieser
    // Bildschirmraum-Weg.
    //
    // Schrift und Rand aus dem Bereich Nametags -- fuer die Namensschilder (UiText) wie fuer die
    // projizierten Text-Displays (world_label_glyphs, gezeichnet in gpu.label_font).
    pub(crate) fn build_world_labels(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        gpu.world_label_glyphs.borrow_mut().clear();
        gpu.world_label_quads.borrow_mut().clear();
        gpu.label_font.set(self.settings.font_of(ShadowArea::Nametags));
        self.begin_text_area(gpu, texts, ShadowArea::Nametags);
        self.build_world_labels_in(gpu, rects, texts);
        Self::end_text_area(gpu, texts);
    }

    pub(crate) fn build_world_labels_in(&self, gpu: &GpuState, rects: &mut Vec<UiRect>, texts: &mut Vec<UiText>) {
        if self.entities.is_empty() {
            return;
        }
        let (sw, sh) = gpu.size();
        let aspect = sw / sh.max(1.0);
        let view_proj = self.camera.view_proj_matrix(aspect);
        let eye = self.camera.view_position();
        let progress = self.tick_progress();
        // "Oben" aus Sicht der Kamera, in Weltkoordinaten: die Y-Achse, um den Blickanteil
        // bereinigt. Damit wird gleich gemessen, wie viele Bildschirmpixel ein Weltblock an
        // dieser Stelle hoch ist -- daraus ergibt sich der Schriftfaktor.
        let forward = self.camera.view_forward();
        let up = {
            let v = glam::Vec3::Y - forward * forward.dot(glam::Vec3::Y);
            if v.length_squared() > 1e-6 {
                v.normalize()
            } else {
                // Senkrecht nach oben/unten geschaut: dann taugt Y nicht mehr als Bezug.
                self.camera.right().cross(forward).normalize()
            }
        };
        // Schrifthoehe und Zeilenabstand in SCHRIFTPIXELN (also beim Faktor 1).
        let font_line = gpu.text_height(1.0);
        // s. label_debug_log -- nur gesammelt, wenn die Umgebungsvariable gesetzt ist.
        let mut debug_zeilen: Vec<String> = Vec::new();
        let debug = label_debug_an();
        let line_pitch = font_line + WORLD_LABEL_LINE_GAP;

        for (&_id, e) in self.entities.iter() {
            if !self.entity_in_render_distance(e) {
                continue;
            }
            // Der eigene Koerper traegt in Vanilla nie eine Beschriftung.
            if Some(e.uuid) == self.own_uuid {
                continue;
            }
            let Some(label) = self.world_label(_id, e, progress) else { continue };
            // Skalierung 0 (oder negativ) heisst: der Server hat die Beschriftung bewusst
            // unsichtbar gemacht.
            if label.world_scale <= 0.0 {
                continue;
            }
            if eye.distance_squared(label.anchor) > label.max_distance * label.max_distance {
                continue;
            }
            if let Some(basis) = label.display_matrix {
                self.build_display_text(gpu, &label, basis, view_proj, (sw, sh));
                continue;
            }
            // Augenrelativ, genau wie die Weltgeometrie (s. build_entity_vertices).
            let rel = (label.anchor - self.camera.position).as_vec3();
            let clip = view_proj * glam::Vec4::new(rel.x, rel.y, rel.z, 1.0);
            if clip.w <= NAMETAG_MIN_CLIP_W {
                continue;
            }
            let ndc = (clip.x / clip.w, clip.y / clip.w);
            if !(-1.0..=1.0).contains(&ndc.0) || !(-1.0..=1.0).contains(&ndc.1) {
                continue;
            }
            // Die Sichtlinien-Pruefung ERST hier, nach der Projektion: sie tastet die
            // Strecke bis zum Anker in 0.25-Bloecke-Schritten ab und ist damit das mit
            // Abstand Teuerste an dieser Schleife. Auf einem Server mit vielen Hologrammen
            // ist der grosse Teil davon ohnehin ausserhalb des Bildes -- die billigen
            // Abweisungen gehoeren davor.
            //
            // Ein Text mit "see through"-Flag darf durch Waende gelesen werden -- Vanilla
            // zeichnet ihn dann ohne Tiefentest.
            if !label.see_through && !self.sicht_frei(eye, label.anchor) {
                continue;
            }
            // Wie viele Bildschirmpixel ist EIN Weltblock an dieser Tiefe hoch? Gemessen
            // statt aus der Projektionsmatrix zurueckgerechnet: der Aufbau der Matrix (FOV,
            // Seitenverhaeltnis, Vorzeichen der Y-Achse) muss hier dann nirgends doppelt
            // stehen. `up` steht senkrecht auf der Blickrichtung, der Tiefenwert w bleibt
            // dadurch derselbe -- die Messung ist exakt, nicht genaehert.
            let clip_up = view_proj * glam::Vec4::new(rel.x + up.x, rel.y + up.y, rel.z + up.z, 1.0);
            if clip_up.w <= NAMETAG_MIN_CLIP_W {
                continue;
            }
            let px_pro_block = ((clip_up.y / clip_up.w - ndc.1) * 0.5 * sh).abs();
            let faktor = WORLD_LABEL_WORLD_SCALE * label.world_scale * px_pro_block;
            if !faktor.is_finite() {
                continue;
            }
            // STUFENLOS, nicht auf ganze Stufen gerundet: gerundet sprang die Schrift beim
            // Gehen sichtbar in Groessen.
            // Der Preis ist eine leicht unregelmaessige Pixelgroesse in der Ferne; das ist
            // erheblich weniger stoerend als das Springen, und in Vanilla schrumpft die
            // Schrift ebenfalls stufenlos.
            //
            // GEDECKELT, nicht weggeworfen: eine Beschriftung, die in der Ferne einfach
            // verschwindet, sieht wie ein Fehler aus. Vanilla zeichnet sie weiter, nur immer
            // kleiner. Wo Schluss ist, entscheidet allein die Sichtweite
            // (s. label.max_distance).
            let scale = faktor.clamp(WORLD_LABEL_MIN_SCALE, WORLD_LABEL_MAX_SCALE);

            // Zeilen trennen, umbrechen und vermessen -- alles in Schriftpixeln, erst danach
            // mal `scale` in Bildschirmpixel.
            let zeilen = wrap_label_lines(&split_label_lines(&self.ego_spans(&label.spans)), label.line_width, |t| gpu.measure_text(t, 1.0));
            if zeilen.is_empty() {
                continue;
            }
            let breiten: Vec<f32> = zeilen.iter().map(|z| z.iter().map(|s| gpu.measure_text(&s.text, 1.0)).sum()).collect();
            let block_breite = breiten.iter().copied().fold(0.0f32, f32::max);
            // Vanillas `n`: die letzte Zeile bekommt den Zeilenabstand nicht mehr dazu.
            let block_hoehe = zeilen.len() as f32 * line_pitch - WORLD_LABEL_LINE_GAP;

            if debug {
                let text: String = zeilen.iter().flat_map(|z| z.iter().map(|s| s.text.clone())).collect();
                let typ = world::entity_type_name(e.entity_type).unwrap_or("?");
                let quelle = if e.display.is_some() {
                    "Display"
                } else if world::is_player(e.entity_type) {
                    "Spieler"
                } else {
                    "Custom-Name"
                };
                let team = self
                    .player_names
                    .get(&e.uuid)
                    .and_then(|n| self.sb_entry_team.get(n))
                    .map(|t| {
                        let sicht = self.sb_teams.get(t).map(|i| i.name_tag_visibility).unwrap_or(TEAM_VISIBILITY_ALWAYS);
                        format!("{t}(sicht={sicht})")
                    })
                    .unwrap_or_else(|| "-".to_string());
                debug_zeilen.push(format!(
                    "{quelle} id={_id} typ={typ} y={:.2} dist={:.1} durchwand={} team={team} text={text:?}",
                    label.anchor.y,
                    eye.distance(label.anchor),
                    label.see_through,
                ));
            }
            let (sx, sy) = ((ndc.0 * 0.5 + 0.5) * sw, (0.5 - ndc.1 * 0.5) * sh);
            let links = (sx - block_breite * scale / 2.0).round();
            // Ein Text-Display waechst NACH OBEN vom Ankerpunkt weg (Vanillas
            // `translate(1 - m/2, -n, 0)` setzt die Unterkante auf den Ursprung); ein
            // Namensschild sitzt dagegen mittig auf seinem Ankerpunkt.
            let block_px = block_hoehe * scale;
            let oben = if label.above_anchor { (sy - block_px).round() } else { (sy - block_px / 2.0).round() };

            // Hintergrundflaeche, ein Schriftpixel Luft ringsum (Vanilla: -1 bis m bzw. n).
            let bg = argb_to_rgba(label.background);
            if bg[3] > 0.0 {
                rects.push(UiRect {
                    rect: Rect {
                        x: links - scale,
                        y: oben - scale,
                        w: (block_breite + 1.0) * scale,
                        h: (block_hoehe + 1.0) * scale,
                    },
                    color: bg,
                    ..UiRect::FLAT
                });
            }

            let deckkraft = label.opacity as f32 / 255.0;
            for (i, zeile) in zeilen.iter().enumerate() {
                // Ausrichtung innerhalb des Blocks (TextDisplayEntity.TextAlignment).
                let versatz = match label.align {
                    LabelAlign::Left => 0.0,
                    LabelAlign::Right => block_breite - breiten[i],
                    LabelAlign::Center => (block_breite - breiten[i]) / 2.0,
                };
                let mut x = links + versatz * scale;
                let y = oben + i as f32 * line_pitch * scale;
                for span in zeile {
                    let mut color = span.color.map_or(COLOR_WHITE, rgb_to_rgba);
                    color[3] *= deckkraft;
                    let breite = gpu.measure_text(&span.text, 1.0) * scale;
                    push_span_shadow(texts, span.shadow);
                    texts.push(UiText { text: span.text.clone(), x: x.round(), y, color, scale });
                    x += breite;
                }
            }
        }
        if debug {
            label_debug_log(&debug_zeilen);
        }
    }

    // Der Text aller nahen Schilder als Flaeche in der Welt -- derselbe Weg wie beim
    // Text-Display (s. build_display_text): Vanillas Matrixkette bis in die Textebene, jede
    // Glyphe auf den Bildschirm projiziert. Nach AbstractSignBlockEntityRenderer.renderText:
    //   [Rueckseite: rotY(180)], translate(Textversatz), scale(f, -f, f),
    //   f = 1/64 * Textgroesse (2/3 stehend, 1 haengend), Zeile i bei y = i * h - 2 * h.
    //
    // Gezeichnet wird im Welt-Durchgang mit Tiefentest (s. GpuState::world_text_pipeline):
    // Entities und Bloecke davor verdecken den Text, und wie in Vanilla bleibt er bei
    // verstecktem HUD (F1) und unter offenen Fenstern stehen. Schrift und Rand aus dem Bereich
    // Schilder.
    pub(crate) fn build_sign_texts(&self, gpu: &GpuState) {
        const SIGN_TEXT_DISTANCE: f64 = 32.0;
        gpu.sign_glyphs.borrow_mut().clear();
        let _schrift = self.area_font_scope(gpu, ShadowArea::Signs);
        gpu.sign_font.set(self.settings.font_of(ShadowArea::Signs));
        let (sw, sh) = gpu.size();
        let view_proj = self.camera.view_proj_matrix(sw / sh.max(1.0));
        let auge = self.camera.view_position();
        // Die Seite im Editor zeichnet unten der Editor selbst, mit dem Text, wie er gerade
        // getippt ist.
        let editiert = self.sign_edit_in_world(gpu).and(self.sign_edit.as_ref());
        for (&pos, text) in &self.signs {
            let mitte = DVec3::new(pos.0 as f64 + 0.5, pos.1 as f64 + 0.5, pos.2 as f64 + 0.5);
            if !self.block_column_drawn(pos) || (mitte - auge).length_squared() > SIGN_TEXT_DISTANCE * SIGN_TEXT_DISTANCE {
                continue;
            }
            for (seite, vorn) in [(&text.front, true), (&text.back, false)] {
                if seite.lines.iter().all(|z| z.iter().all(|s| s.text.is_empty())) || editiert.is_some_and(|e| e.pos == pos && e.front == vorn) {
                    continue;
                }
                let Some((frame, zeile_h)) = self.sign_side_frame(pos, vorn) else { continue };
                // Verdeckung macht der Tiefentest -- nur ohne ihn (Shader-Pfad) der Strahl.
                if self.sign_side_visible(frame, view_proj, zeile_h, (sw, sh), gpu.use_shader_path) {
                    self.push_sign_side(gpu, seite, frame, view_proj, zeile_h, (sw, sh));
                }
            }
        }
        if let Some(edit) = editiert {
            self.push_sign_edit_in_world(gpu, edit, view_proj, (sw, sh));
        }
    }

    // Vanillas Matrixkette bis in die Textebene einer Schildseite (s. build_sign_texts) und
    // die Zeilenhoehe dort; None, wenn an `pos` kein Schild steht.
    pub(crate) fn sign_side_frame(&self, pos: (i32, i32, i32), vorn: bool) -> Option<(glam::Mat4, f32)> {
        use world::entity_model::{Affine, SignKind};
        let (_, art, grad) = self.sign_at(pos)?;
        let haengend = matches!(art, SignKind::Hanging(_));
        let rel = (DVec3::new(pos.0 as f64, pos.1 as f64, pos.2 as f64) - self.camera.position).as_vec3();
        let mut b = world::entity_model::sign_base([rel.x, rel.y, rel.z], art, grad);
        let (versatz, f, zeile_h) = if haengend { ([0.0, -0.32, 0.073], 1.0 / 64.0, 9.0) } else { ([0.0, 1.0 / 3.0, 0.046_666_667], (1.0 / 64.0) * (2.0 / 3.0), 10.0) };
        if !vorn {
            b = b.then(Affine::rotation_y_deg(180.0));
        }
        Some((mat4_from_affine(&b.then(Affine::translation(versatz)).then(Affine::scale([f, -f, f]))), zeile_h))
    }

    // Sieht man diese Schildseite gerade: von vorn, im Bild und (mit `verdeckung`) nicht hinter
    // Bloecken?
    pub(crate) fn sign_side_visible(&self, frame: glam::Mat4, view_proj: glam::Mat4, zeile_h: f32, screen: (f32, f32), verdeckung: bool) -> bool {
        let (sw, sh) = screen;
        let project = sign_projector(view_proj * frame, screen);
        let oben = -2.0 * zeile_h;
        // Von hinten unsichtbar (dieselbe Pruefung wie beim Text-Display).
        let [Some(a), Some(b), Some(c)] = [project(-45.0, oben), project(45.0, oben), project(45.0, -oben)] else { return false };
        if (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0) <= 0.0 {
            return false;
        }
        if [a, b, c].iter().all(|p| p.0 < 0.0) || [a, b, c].iter().all(|p| p.0 > sw) || [a, b, c].iter().all(|p| p.1 < 0.0) || [a, b, c].iter().all(|p| p.1 > sh) {
            return false;
        }
        if !verdeckung {
            return true;
        }
        // Verdeckt? Gemessen an einem Punkt knapp vor der Textmitte -- die Mitte selbst liegt
        // im Umriss des eigenen Schildes.
        let m = frame * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);
        let welt = self.camera.position + DVec3::new(m.x as f64, m.y as f64, m.z as f64);
        let auge = self.camera.view_position();
        let vor = welt + (auge - welt).normalize_or_zero() * 0.3;
        self.sicht_frei(auge, vor)
    }

    // Schreibt man direkt auf dem Schild in der Welt? Ja, solange man die bearbeitete Seite
    // sieht; sonst (Schild im Ruecken, verdeckt, HUD versteckt) das Editor-Fenster wie Vanilla.
    // Liefert Matrix und Zeilenhoehe der Seite.
    pub(crate) fn sign_edit_in_world(&self, gpu: &GpuState) -> Option<(glam::Mat4, f32)> {
        let edit = self.sign_edit.as_ref()?;
        if self.mode != AppMode::World || self.hud_hidden || !self.settings.sign_edit_in_world {
            return None;
        }
        let (sw, sh) = gpu.size();
        let view_proj = self.camera.view_proj_matrix(sw / sh.max(1.0));
        let (frame, zeile_h) = self.sign_side_frame(edit.pos, edit.front)?;
        self.sign_side_visible(frame, view_proj, zeile_h, (sw, sh), true).then_some((frame, zeile_h))
    }

    // Die Seite im Editor auf dem Schild selbst: der getippte Text und der Schreibcursor am
    // Ende der aktuellen Zeile. Der Cursor gleitet wie in den Eingabefeldern (s. push_caret),
    // hier auch beim Zeilenwechsel, und blinkt nach jeder Bewegung von vorn.
    pub(crate) fn push_sign_edit_in_world(&self, gpu: &GpuState, edit: &SignEdit, view_proj: glam::Mat4, screen: (f32, f32)) {
        let Some((frame, zeile_h)) = self.sign_side_frame(edit.pos, edit.front) else { return };
        let glowing = self.signs.get(&edit.pos).is_some_and(|t| if edit.front { t.front.glowing } else { t.back.glowing });
        let seite = protocol::sign::SignSide {
            lines: std::array::from_fn(|i| {
                let t = &edit.lines[i];
                if t.is_empty() { Vec::new() } else { vec![protocol::nbt::TextSpan { text: t.clone(), ..Default::default() }] }
            }),
            color: edit.color,
            glowing,
        };
        self.push_sign_side(gpu, &seite, frame, view_proj, zeile_h, screen);

        let jetzt = Instant::now();
        let ziel = (gpu.measure_text(&edit.lines[edit.row], 1.0) / 2.0, edit.row as f32 * zeile_h - 2.0 * zeile_h);
        let c = match edit.caret.get() {
            Some(c) if c.to == ziel => c,
            Some(c) => SignCaret { from: c.at(jetzt), to: ziel, start: jetzt },
            None => SignCaret { from: ziel, to: ziel, start: jetzt },
        };
        edit.caret.set(Some(c));
        let (x, top) = c.at(jetzt);
        let h = gpu.text_height(1.0);
        let project = sign_projector(view_proj * frame, screen);
        let [Some(a), Some(b), Some(cc), Some(d)] = [project(x + 0.5, top - 1.0), project(x + 1.5, top - 1.0), project(x + 1.5, top + h), project(x + 0.5, top + h)]
        else {
            return;
        };
        let mut farbe = rgb_to_rgba(sign_text_color(edit.color, glowing));
        farbe[3] *= caret_alpha(jetzt.duration_since(c.start));
        let v = |p: (f32, f32)| Vertex { position: pixel_to_ndc(p.0, p.1, screen), color: farbe, ..Vertex::PLAIN };
        gpu.world_label_quads.borrow_mut().extend_from_slice(&[v(a), v(b), v(cc), v(a), v(cc), v(d)]);
    }

    // Eine Schildseite in gpu.sign_glyphs, mit der Tiefe jedes Punktes fuer den Welt-Durchgang.
    // Rand: wie bei der Weltschrift, ausser der Bereich Schilder hat ihn ausgeschaltet.
    pub(crate) fn push_sign_side(&self, gpu: &GpuState, seite: &protocol::sign::SignSide, frame: glam::Mat4, view_proj: glam::Mat4, zeile_h: f32, screen: (f32, f32)) {
        let clip = view_proj * frame;
        let project = sign_projector(clip, screen);
        let tiefe = |x: f32, y: f32| {
            let c = clip * glam::Vec4::new(x, y, 0.0, 1.0);
            c.z / c.w
        };
        let oben = -2.0 * zeile_h;
        let raster = match (project(0.0, 0.0), project(1.0, 0.0)) {
            (Some(p), Some(q)) => ((q.0 - p.0).hypot(q.1 - p.1)).clamp(1.0, WORLD_LABEL_MAX_SCALE),
            _ => 1.0,
        };
        let vorgabe = sign_text_color(seite.color, seite.glowing);
        let rand_an = self.settings.shadow_of(ShadowArea::Signs) != Some(TextShadow::Off);
        let mut glyphen = gpu.sign_glyphs.borrow_mut();
        for (i, zeile) in seite.lines.iter().enumerate() {
            let breite: f32 = zeile.iter().map(|s| gpu.measure_text(&s.text, 1.0)).sum();
            let mut x = -breite / 2.0;
            let top = i as f32 * zeile_h + oben;
            for span in zeile {
                let color = rgb_to_rgba(span.color.unwrap_or(vorgabe));
                let rand = gpu.world_text_border(color, span.shadow, rand_an);
                x += gpu.shape_projected_text(&span.text, x, top, color, rand, raster, &project, Some(&tiefe), screen, &mut glyphen);
            }
        }
        let ganz = glyphen.len().min(MAX_SIGN_TEXT_VERTICES) / VERTS_PER_QUAD * VERTS_PER_QUAD;
        glyphen.truncate(ganz);
    }

    // Kamerawinkel wie Vanillas Camera.getYaw/-Pitch: in der Frontansicht umgedreht (s.
    // Camera::view_forward). camera.pitch zaehlt nach OBEN positiv, Minecraft nach unten --
    // daher das Vorzeichen.
    pub(crate) fn vanilla_camera_angles(&self) -> (f32, f32) {
        if self.camera.view_inverted {
            (self.camera.yaw + 180.0, self.camera.pitch)
        } else {
            (self.camera.yaw, -self.camera.pitch)
        }
    }

    // Ein Text-Display als Flaeche in der Welt: Vanillas Matrixkette (Entity-Position,
    // Billboard, AffineTransformation, Text-Skalierung -- s. WorldLabel::display_matrix und
    // display_text_frame), jede Glyphe und der Hintergrund einzeln auf den Bildschirm
    // projiziert. Wie Vanilla nur von VORN sichtbar: von hinten zeigt ein Display nichts.
    pub(crate) fn build_display_text(&self, gpu: &GpuState, label: &WorldLabel, basis: glam::Mat4, view_proj: glam::Mat4, screen: (f32, f32)) {
        let line_pitch = gpu.text_height(1.0) + WORLD_LABEL_LINE_GAP;
        let zeilen = wrap_label_lines(&split_label_lines(&self.ego_spans(&label.spans)), label.line_width, |t| gpu.measure_text(t, 1.0));
        if zeilen.is_empty() {
            return;
        }
        let breiten: Vec<f32> = zeilen.iter().map(|z| z.iter().map(|s| gpu.measure_text(&s.text, 1.0)).sum()).collect();
        let m = breiten.iter().copied().fold(0.0f32, f32::max);
        let n = zeilen.len() as f32 * line_pitch - WORLD_LABEL_LINE_GAP;
        let frame = basis * display_text_frame(m, n);
        let clip = view_proj * frame;
        let (sw, sh) = screen;
        let project = |x: f32, y: f32| -> Option<(f32, f32)> {
            let c = clip * glam::Vec4::new(x, y, 0.0, 1.0);
            (c.w > NAMETAG_MIN_CLIP_W).then(|| ((c.x / c.w * 0.5 + 0.5) * sw, (0.5 - c.y / c.w * 0.5) * sh))
        };

        // Die Hintergrundflaeche (Vanilla: -1..m, -1..n) entscheidet ueber Sichtbarkeit.
        let ecken = [project(-1.0, -1.0), project(m, -1.0), project(m, n), project(-1.0, n)];
        let [Some(a), Some(b), Some(c), Some(d)] = ecken else { return };
        // Rueckseite: in der Textebene laufen die Ecken im Uhrzeigersinn (y nach unten);
        // auf dem Bildschirm gespiegelt heisst, man schaut von hinten.
        let kreuz = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if kreuz <= 0.0 {
            return;
        }
        let (xs, ys) = ([a.0, b.0, c.0, d.0], [a.1, b.1, c.1, d.1]);
        if xs.iter().all(|&x| x < 0.0) || xs.iter().all(|&x| x > sw) || ys.iter().all(|&y| y < 0.0) || ys.iter().all(|&y| y > sh) {
            return;
        }
        // Verdeckung wie bei den Namensschildern, gemessen an der Mitte der Flaeche.
        if !label.see_through {
            let mitte = frame * glam::Vec4::new((m - 1.0) / 2.0, (n - 1.0) / 2.0, 0.0, 1.0);
            let welt = self.camera.position + glam::DVec3::new(mitte.x as f64, mitte.y as f64, mitte.z as f64);
            if !self.sicht_frei(self.camera.view_position(), welt) {
                return;
            }
        }

        let bg = argb_to_rgba(label.background);
        if bg[3] > 0.0 {
            let v = |p: (f32, f32)| Vertex { position: pixel_to_ndc(p.0, p.1, screen), color: bg, ..Vertex::PLAIN };
            gpu.world_label_quads.borrow_mut().extend_from_slice(&[v(a), v(b), v(c), v(a), v(c), v(d)]);
        }

        // Rasterstufe der Glyphen: wie viele Bildschirmpixel ein Schriftpixel etwa breit ist.
        let raster = match (project(m / 2.0, n / 2.0), project(m / 2.0 + 1.0, n / 2.0)) {
            (Some(p), Some(q)) => ((q.0 - p.0).hypot(q.1 - p.1)).clamp(1.0, WORLD_LABEL_MAX_SCALE),
            _ => 1.0,
        };
        let deckkraft = label.opacity as f32 / 255.0;
        let mut glyphen = gpu.world_label_glyphs.borrow_mut();
        for (i, zeile) in zeilen.iter().enumerate() {
            let mut x = match label.align {
                LabelAlign::Left => 0.0,
                LabelAlign::Right => m - breiten[i],
                LabelAlign::Center => (m - breiten[i]) / 2.0,
            };
            let top = i as f32 * line_pitch;
            for span in zeile {
                let mut color = span.color.map_or(COLOR_WHITE, rgb_to_rgba);
                color[3] *= deckkraft;
                let rand = gpu.world_text_border(color, span.shadow, label.shadow);
                x += gpu.shape_projected_text(&span.text, x, top, color, rand, raster, &project, None, screen, &mut glyphen);
            }
        }
    }

    // Was an diesem Entity als Weltbeschriftung haengt -- oder None, wenn nichts.
    pub(crate) fn world_label<'a>(&self, id: i32, e: &'a world::Entity, progress: f32) -> Option<WorldLabel<'a>> {
        let (x, y, z) = e.render_pos(progress);
        // Ein SPIELER traegt den Namen aus der Tabellenliste, nicht aus seinen Metadaten.
        // Alles andere daran ist dasselbe wie bei jeder anderen Beschriftung -- und genau
        // deshalb steht es hier und nicht mehr in einem eigenen Zeichenpfad.
        if world::is_player(e.entity_type) {
            let anker = DVec3::new(x, y + self.name_label_height(e), z);
            // Vanillas vier Regeln auf einmal (s. player_label_visible). Die Entfernung misst
            // Vanilla zum ENTITY, nicht zum Ankerpunkt -- daher hier die Fussposition.
            let dist_sq = self.camera.view_position().distance_squared(DVec3::new(x, y, z));
            if !player_label_visible(Some(e.uuid) == self.own_uuid, e.invisible, e.sneaking(), dist_sq) {
                return None;
            }
            let name = self.player_names.get(&e.uuid)?;
            // Vanillas Team-Regel (s. team_allows_name). Der eigene Teamname kommt ueber den
            // eigenen Spielernamen -- Teams fuehren Spieler namentlich, nicht per UUID.
            let team_name = self.sb_entry_team.get(name).map(String::as_str);
            let own_team = self
                .own_uuid
                .and_then(|u| self.player_names.get(&u))
                .and_then(|eigener| self.sb_entry_team.get(eigener))
                .map(String::as_str);
            let team = team_name.and_then(|t| self.sb_teams.get(t));
            if !team_allows_name(team, own_team, team_name) {
                return None;
            }
            // Letzte Zeile von LivingEntityRenderer.hasLabel: `&& !livingEntity.hasPassengers()`.
            // Sonst stehen Namen doppelt da: Server setzen ihr eigenes Namensschild als
            // Text-Display-PASSAGIER auf den Spieler -- Vanilla zeigt dann den Spielernamen
            // nicht. Die Regel steht in Vanilla HINTER dem Team-Switch, und der kehrt fuer jede
            // Sichtbarkeitsregel selbst zurueck; sie greift also nur bei Spielern ohne Team.
            if team.is_none() && self.entities.has_passengers(id) {
                return None;
            }
            // Vanillas Team.decorateName: Prefix + Name + Suffix, alles in der Teamfarbe
            // (PlayerEntity.getDisplayName -> Team.decorateName). Genau daran erkennt man auf
            // den meisten Servern Rang und Fraktion.
            let farbe = team.and_then(|t| protocol::nbt::formatting_color(t.color));
            let beschriftet = match team {
                Some(t) => format!("{}\u{a7}r{name}{}", t.prefix, t.suffix),
                None => name.clone(),
            };
            // Sprachchat: ein Notenzeichen vor dem Namen -- gruen beim Sprechen, tuerkis beim
            // Fluestern, rot fuer taub geschaltete Spieler.
            let beschriftet = match self.voice_nametag_icon(&e.uuid) {
                Some(code) => format!("\u{a7}{code}\u{266a} \u{a7}r{beschriftet}"),
                None => beschriftet,
            };
            return Some(WorldLabel {
                anchor: anker,
                // Prefix/Suffix koennen eigene Paragraphen-Codes tragen -- deshalb durch
                // denselben Auswerter wie jeder andere Text, statt sie roh anzuzeigen.
                spans: std::borrow::Cow::Owned(protocol::nbt::legacy_to_spans(&beschriftet, farbe)),
                world_scale: 1.0,
                background: protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND,
                opacity: 0xFF,
                // Wie Vanilla: ein stehender oder laufender Spieler zeigt seinen Namen auch
                // durch Waende (die SEETHROUGH-Pipeline testet keine Tiefe), ein
                // schleichender nur in Sichtlinie.
                see_through: !e.sneaking(),
                shadow: true,
                align: LabelAlign::Center,
                above_anchor: false,
                // Ein Namensschild bricht in Vanilla nicht um.
                line_width: f32::INFINITY,
                // Schleichend halbiert Vanilla die Sichtweite auf 32 Bloecke
                // (LivingEntityRenderer.hasLabel: `if (d >= 1024.0) return false`).
                max_distance: if e.sneaking() { NAMETAG_SNEAK_DISTANCE } else { NAMETAG_MAX_DISTANCE },
                display_matrix: None,
            });
        }
        if let Some(display) = &e.display {
            let world::DisplayContent::Text(text) = &display.content else { return None };
            if text.spans.is_empty() {
                return None;
            }
            return Some(WorldLabel {
                // Die Verschiebung der AffineTransformation gehoert zum Anker; Drehung und
                // Skalierung wirken danach (s. Kommentar an build_world_labels).
                anchor: DVec3::new(x + display.translation[0] as f64, y + display.translation[1] as f64, z + display.translation[2] as f64),
                spans: std::borrow::Cow::Borrowed(&text.spans),
                // Die Y-Skalierung bestimmt die Schrifthoehe; X/Z getrennt zu behandeln
                // hiesse, die Schrift zu verzerren, was der Bitmap-Font nicht kann.
                world_scale: display.scale[1],
                background: if text.flags & protocol::packets::TEXT_DISPLAY_FLAG_DEFAULT_BACKGROUND != 0 {
                    protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND
                } else {
                    text.background
                },
                opacity: text.opacity,
                see_through: text.flags & protocol::packets::TEXT_DISPLAY_FLAG_SEE_THROUGH != 0,
                shadow: text.flags & protocol::packets::TEXT_DISPLAY_FLAG_SHADOW != 0,
                align: LabelAlign::from_flags(text.flags),
                above_anchor: true,
                line_width: WORLD_LABEL_DEFAULT_LINE_WIDTH,
                max_distance: (display.view_range as f64).max(0.0) * DISPLAY_VIEW_RANGE_BLOCKS,
                display_matrix: Some({
                    let (cam_yaw, cam_pitch) = self.vanilla_camera_angles();
                    let rel = (DVec3::new(x, y, z) - self.camera.position).as_vec3();
                    glam::Mat4::from_translation(rel)
                        * display_billboard_matrix(display.billboard, e.render_yaw(progress), e.render_pitch(progress), cam_yaw, cam_pitch)
                        * display_affine_matrix(&display.transform_at(e.age_ticks(progress)))
                }),
            });
        }
        // Namensschild eines Entities mit eigenem Namen. Vanillas Entity.shouldRenderName()
        // ist genau `isCustomNameVisible()` -- der Name allein genuegt nicht, er muss auch
        // eingeblendet sein.
        if !e.name_visible {
            return None;
        }
        let spans = e.custom_name.as_ref()?;
        Some(WorldLabel {
            anchor: DVec3::new(x, y + self.name_label_height(e), z),
            spans: std::borrow::Cow::Borrowed(spans),
            world_scale: 1.0,
            background: protocol::packets::TEXT_DISPLAY_INITIAL_BACKGROUND,
            opacity: 0xFF,
            see_through: false,
            shadow: true,
            align: LabelAlign::Center,
            above_anchor: false,
            // Ein Namensschild bricht in Vanilla nicht um.
            line_width: f32::INFINITY,
            max_distance: NAMETAG_MAX_DISTANCE,
            display_matrix: None,
        })
    }

    // Vanillas Entity.getNameLabelHeight() = getHeight() + 0.5.
    //
    // Der Armorstand ist hier der wichtige Sonderfall, nicht eine Feinheit: er ist der
    // Traeger fast jedes klassischen Hologramms, und seine Hoehe haengt an zwei Flags. Mit
    // der pauschalen Spielerhoehe von hit_box_size schwebte ein Marker-Hologramm 1.8 Bloecke
    // zu hoch -- also sichtbar an der falschen Stelle.
    pub(crate) fn name_label_height(&self, e: &world::Entity) -> f64 {
        let hoehe = match world::entity_type_name(e.entity_type) {
            Some("armor_stand") => e.armor_stand_height(),
            // Fuer alles andere die Trefferbox samt Jungtier und SCALE.
            _ => e.hit_size().1,
        };
        hoehe + NAMETAG_HEIGHT_ABOVE_HEAD
    }

    // Ist die Sichtlinie zwischen zwei Weltpunkten frei von Bloecken? Ersatz fuer den
    // Tiefentest, den es fuer die im Bildschirmraum gezeichneten Namensschilder nicht gibt.
    //
    // Abgetastet statt exakt durchgerastert: physics::raycast_block laeuft ueber ALLE Zellen
    // der umschliessenden Box, was auf 64 Bloecken Entfernung sechsstellig viele Zellen waeren.
    // Hier wird stattdessen in festen Schritten entlang der Strecke geprueft, ob der Punkt in
    // einer Umriss-Box des dortigen Blocks liegt -- dieselbe Formtabelle, gegen die auch
    // gezielt wird (world::outline_boxes), also z.B. kein Fehlalarm durch hohes Gras.
    //
    // Der Preis der Abtastung: eine hauchduenne Wand genau zwischen zwei Abtastpunkten wird
    // uebersehen. Bei SIGHT_SAMPLE_STEP von 0.25 Bloecken betrifft das nur Formen, die duenner
    // als eine Glasscheibe sind -- und ein kurz zu frueh sichtbares Schild ist der harmlosere
    // Fehler gegenueber einem, das nie erscheint.
    pub(crate) fn sicht_frei(&self, von: DVec3, bis: DVec3) -> bool {
        let strecke = bis - von;
        let laenge = strecke.length();
        if laenge <= f64::EPSILON {
            return true;
        }
        let richtung = strecke / laenge;
        let schritte = (laenge / SIGHT_SAMPLE_STEP).ceil() as i32;
        for i in 1..schritte {
            let p = von + richtung * (i as f64 * SIGHT_SAMPLE_STEP);
            let (bx, by, bz) = (p.x.floor(), p.y.floor(), p.z.floor());
            let state = self.world.block_at(bx as i32, by as i32, bz as i32);
            if state == world::STATE_AIR {
                continue;
            }
            let lokal = [(p.x - bx) as f32, (p.y - by) as f32, (p.z - bz) as f32];
            for shape in world::outline_boxes(state, &self.block_shapes) {
                let drin = (0..3).all(|a| lokal[a] >= shape.min[a] && lokal[a] <= shape.max[a]);
                if drin {
                    return false;
                }
            }
        }
        true
    }

    // Ein Entity mit Modell aus world::mob_catalog: Geometrie aus den Modelldaten, Grundtextur
    // plus Schichten (Dorfbewohner), gemeinsame Laufanimation. Boote und Loren stehen nicht
    // wie Lebewesen in der Welt, sondern um ihre Mitte gedreht und 0.375 hoeher
    // (AbstractBoatEntityRenderer / AbstractMinecartEntityRenderer).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_catalog_entity(
        &self,
        e: &world::Entity,
        feet: [f32; 3],
        body_yaw: f32,
        head_rel: f32,
        pitch: f32,
        (swing, amount): (f32, f32),
        out: &mut Vec<world::BlockVertex>,
    ) {
        use world::entity_model::Affine;
        use world::mob_catalog::{self as katalog, Anim};
        let Some(name) = world::entity_type_name(e.entity_type) else { return };
        let Some(look) = katalog::mob_look(name) else { return };
        let layer = if e.baby { look.baby_layer.unwrap_or(look.layer) } else { look.layer };
        let Some(model) = world::model_data::layer_model(layer) else { return };
        let grund = if e.baby { look.baby_texture.unwrap_or(look.texture) } else { look.texture };
        let mut texturen = vec![grund.to_string()];
        if matches!(name, "villager" | "zombie_villager") {
            texturen.extend(katalog::villager_layers(name == "zombie_villager", e.villager_data, e.baby));
        }
        let base = if katalog::is_boat(name) {
            Affine::translation(feet)
                .then(Affine::rotation_y_deg(180.0 - body_yaw))
                .then(Affine::translation([0.0, 0.375, 0.0]))
                .then(Affine::scale([-1.0, -1.0, 1.0]))
                .then(Affine::rotation_y_deg(90.0))
        } else if katalog::is_minecart(name) {
            Affine::translation(feet)
                .then(Affine::translation([0.0, 0.375, 0.0]))
                .then(Affine::rotation_y_deg(180.0 - body_yaw))
                .then(Affine::scale([-1.0, -1.0, 1.0]))
        } else {
            world::entity_transform(feet, body_yaw, e.scale, 1.0)
        };
        let haelt = e.main_hand.is_some() || e.off_hand.is_some();
        let mut pose = katalog::walk_pose(model, look.anim, head_rel, pitch, swing, amount, haelt);
        if look.anim == Anim::Walker {
            apply_item_arm_poses(&mut pose, e.arm_items().0.is_some(), e.arm_items().1.is_some());
        }
        for tex in &texturen {
            if let Some(rect) = self.entity_atlas.rect(tex) {
                world::emit_entity_model(model, &pose, base, rect, ENTITY_BRIGHTNESS, out);
            }
        }
        self.emit_held_items(e.arm_items(), self.entity_blocking_arms(e), (0.0, true), model, &pose, base, out);
    }

    // Ein Armorstand (ArmorStandRenderer): der Koerper in seinen sechs Gelenkwinkeln, Ruestung
    // und Gehaltenes in denselben Winkeln. Unsichtbar faellt nur der Koerper weg -- Koepfe und
    // Ruestung auf unsichtbaren Staendern bleiben stehen, wie in Vanilla.
    pub(crate) fn emit_armor_stand(
        &self,
        e: &world::Entity,
        feet: [f32; 3],
        body_yaw: f32,
        out: &mut Vec<world::BlockVertex>,
        haut: &mut Vec<std::ops::Range<usize>>,
    ) {
        use protocol::packets::{ARMOR_STAND_FLAG_NO_BASEPLATE, ARMOR_STAND_FLAG_SHOW_ARMS, ARMOR_STAND_FLAG_SMALL};
        let flags = e.armor_stand_flags();
        let small = flags & ARMOR_STAND_FLAG_SMALL != 0;
        let [kopf, rumpf, linker_arm, rechter_arm, linkes_bein, rechtes_bein] = e.armor_stand_poses.map(|r| r.map(f32::to_radians));
        let mut pose = world::Pose::new();
        pose.rotate("head", kopf);
        // Die Staebe am Rumpf folgen dem Rumpf (ArmorStandModel.setupAnim).
        for teil in ["body", "right_body_stick", "left_body_stick", "shoulder_stick"] {
            pose.rotate(teil, rumpf);
        }
        pose.rotate("left_arm", linker_arm);
        pose.rotate("right_arm", rechter_arm);
        pose.rotate("left_leg", linkes_bein);
        pose.rotate("right_leg", rechtes_bein);
        // Die Bodenplatte dreht gegen den Stand und bleibt so am Blockraster ausgerichtet.
        pose.rotate("base_plate", [0.0, -body_yaw.to_radians(), 0.0]);
        let base = world::entity_transform(feet, body_yaw, e.scale, 1.0);
        let layer = if small { "armor_stand_small#main" } else { "armor_stand#main" };
        let textur = world::mob_catalog::mob_look("armor_stand").and_then(|look| self.entity_atlas.rect(look.texture));
        if let (false, Some(model), Some(tex)) = (e.invisible, world::model_data::layer_model(layer), textur) {
            let mut koerper = pose.clone();
            let mut verstecken = |teil: &'static str| {
                let mut p = koerper.get(teil);
                p.hidden = true;
                koerper.set(teil, p);
            };
            if flags & ARMOR_STAND_FLAG_SHOW_ARMS == 0 {
                verstecken("left_arm");
                verstecken("right_arm");
            }
            if flags & ARMOR_STAND_FLAG_NO_BASEPLATE != 0 {
                verstecken("base_plate");
            }
            let start = out.len();
            world::emit_entity_model(model, &koerper, base, tex, ENTITY_BRIGHTNESS, out);
            haut.push(start..out.len());
        }
        let models = world::entity_model::armor_stand_armor_models(small);
        self.emit_armor(&e.armor, models, &pose, base, out);
        // Items sitzen auch ohne sichtbare Arme in der Hand (ArmorStandModel.translateToHand);
        // die Brust-Ebene hat beide Arme in den Winkeln des Standes.
        let arme = &models[world::armor::ArmorSlot::Chest.index()];
        self.emit_held_items(e.arm_items(), (false, false), (0.0, true), arme, &pose, base, out);
    }

    // Hinter allen Menues ohne Verbindung, nicht nur auf dem alten Startbildschirm: der Client
    // startet in der Serverliste, dort war das Bild sonst nie zu sehen. Der HUD-Editor hat
    // seinen eigenen Hintergrund (letzter Weltblick), der Todesbildschirm die Welt.
    pub(crate) fn wallpaper_visible(&self) -> bool {
        self.mode == AppMode::Menu
            && self.conn_status == ConnStatus::Disconnected
            && !matches!(self.screen, MenuScreen::HudEditor | MenuScreen::Death)
    }

    // HUD-Editor ohne Welt dahinter: der letzte Weltblick (s. last_view.rs) statt Schwarz.
    pub(crate) fn last_view_visible(&self) -> bool {
        self.mode == AppMode::Menu && self.screen == MenuScreen::HudEditor && self.conn_status != ConnStatus::Connected
    }

    // Beim Verlassen einer Welt das letzte Weltbild als letzten Weltblick behalten und speichern.
    // VOR reset_world, solange die Welt noch da ist.
    //
    // Dafuer wird die Welt eigens noch einmal gezeichnet, mit allem ausgeblendet wie bei F1 (kein
    // HUD, keine Hand, kein Blockrahmen). Vorher wurde nur gelesen, was gerade in der
    // Offscreen-Textur stand: beim Trennen aus dem Spiel heraus war das ein Bild MIT HUD und
    // wurde verworfen -- der Blick blieb dann der einer frueheren Sitzung.
    pub(crate) fn keep_last_view(&mut self) {
        let Some(bild) = self.capture_clean_view() else { return };
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_last_view(&bild);
        }
        // Zugleich der Blick dieses Servereintrags fuers naechste Verbinden -- wenn er auf
        // "Letzter Moment" steht (s. ServerEntry::view_last).
        if self.connect_view_last() {
            self.store_connect_view(&bild);
        }
        if let Some(dir) = shared_config_dir() {
            last_view::save(last_view::path(&dir), bild);
        }
    }

    // Steht der Servereintrag, mit dem gerade verbunden ist, auf "Letzter Moment"? Ohne Eintrag
    // (Quick Connect) nach der Adresse.
    pub(crate) fn connect_view_last(&self) -> bool {
        let adresse = self.server_address.trim();
        match self.servers.iter().find(|e| e.address.trim().eq_ignore_ascii_case(adresse)) {
            Some(e) => e.view_last.unwrap_or_else(|| server_view_last_default(&e.name, &e.address)),
            None => server_view_last_default("", adresse),
        }
    }

    // Ein Bild als Verbindungsbild dieses Servereintrags hochladen und speichern.
    pub(crate) fn store_connect_view(&mut self, bild: &wallpaper::Decoded) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        gpu.set_connect_view(bild);
        self.connect_view_for = Some(self.server_address.clone());
        self.connect_view_rx = None;
        if let Some(dir) = shared_config_dir() {
            last_view::save(last_view::server_path(&dir, &self.server_address), bild.clone());
        }
    }

    // "Erster Moment": eine Sekunde nach dem ersten Chunk (dann steht das Gelaende um einen
    // herum) einmal je Verbindung aufnehmen. Transfers zaehlen zur selben Verbindung.
    pub(crate) fn tick_connect_view_capture(&mut self) {
        if self.connect_view_taken || self.mode != AppMode::World || self.chunks_received == 0 || !self.camera_placed {
            return;
        }
        if self.connect_view_last() {
            return;
        }
        let seit = *self.connect_view_first_at.get_or_insert_with(Instant::now);
        if seit.elapsed() < CONNECT_VIEW_FIRST_DELAY {
            return;
        }
        self.connect_view_taken = true;
        if let Some(bild) = self.capture_clean_view() {
            self.store_connect_view(&bild);
        }
    }

    // Launcher-Hintergrundbild (s. hero_scene.rs): fehlt die Datei, die Szene ohne Verbindung in
    // die leere Welt laden, warten, bis alles gemesht ist, einmal ohne HUD zeichnen und
    // speichern. Verbindet man dazwischen, wird abgebrochen und beim naechsten Start neu versucht.
    pub(crate) fn tick_hero_scene(&mut self) {
        if self.hero_done {
            return;
        }
        let Some(seit) = self.hero_building else {
            // Erst wenn der Start durch ist: Packs geladen, keine Verbindung.
            if self.gpu.is_none()
                || self.conn_status != ConnStatus::Disconnected
                || self.pack_load_job.is_some()
                || self.app_started.elapsed() < HERO_START_DELAY
            {
                return;
            }
            if hero_scene::path().is_none() || hero_scene::is_current() {
                self.hero_done = true;
                return;
            }
            self.world.set_min_y(HERO_MIN_Y);
            let Some(szene) = hero_scene::load(HERO_MIN_Y, HERO_SECTIONS) else {
                self.hero_done = true;
                return;
            };
            self.apply_biome_registry(&szene.biomes);
            for (cx, cz, sections, licht) in szene.columns {
                self.world.insert_column(cx, cz, sections);
                self.world.apply_sky_light(cx, cz, &licht);
            }
            self.physics.pos = szene.pos;
            self.physics_render_prev = szene.pos;
            self.camera.yaw = szene.yaw;
            self.camera.pitch = szene.pitch;
            self.hero_fov = Some(szene.fov);
            self.hero_render_distance = Some(szene.render_distance);
            // Wie nach der ersten Position vom Server: erst dann waehlt enforce_render_limits die
            // Spalten aus, die gezeichnet werden -- ohne das blieb das Bild leer.
            self.camera_placed = true;
            self.world_time_of_day = szene.time_of_day;
            self.world_time_ticking = false;
            self.hero_building = Some(Instant::now());
            return;
        };
        if self.conn_status != ConnStatus::Disconnected {
            self.hero_building = None;
            self.hero_done = true;
            self.hero_fov = None;
            self.hero_render_distance = None;
            return;
        }
        let fertig = self.world.dirty_count() == 0 && self.mesh_jobs_pending.is_empty() && self.pack_load_job.is_none();
        if !fertig || seit.elapsed() < HERO_SETTLE {
            if seit.elapsed() > HERO_GIVE_UP {
                self.hero_building = None;
                self.hero_done = true;
                self.hero_fov = None;
                self.hero_render_distance = None;
                self.reset_world();
            }
            return;
        }
        self.hero_building = None;
        self.hero_done = true;
        // Ohne Verbindung fuehrt update_movement die Kamera nicht nach -- sie stuende sonst noch
        // dort, wo sie vor der Szene war, und saehe nur Himmel.
        self.camera.position = self.physics.pos + DVec3::new(0.0, self.physics.eye_height() as f64, 0.0);
        self.push_log(format!(
            "Launcher-Bild: {} Spalten gemesht, Kamera {:.1} {:.1} {:.1}",
            self.render_limit_meshed.len(),
            self.camera.position.x,
            self.camera.position.y,
            self.camera.position.z
        ));
        if let (Some(bild), Some(pfad)) = (self.capture_clean_view(), hero_scene::path()) {
            // Steht das Hintergrundbild auf "Coral", gleich zeigen -- die Datei schreibt ein
            // Thread erst noch.
            if wallpaper::WallpaperMode::from_index(self.settings.wallpaper) == wallpaper::WallpaperMode::Coral {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.set_wallpaper(Some(&bild));
                }
            }
            last_view::save(pfad, bild);
            if let Some(v) = hero_scene::version_path() {
                let _ = std::fs::write(v, hero_scene::VERSION);
            }
        }
        self.hero_fov = None;
        self.hero_render_distance = None;
        self.reset_world();
        self.world_time_ticking = true;
    }

    // Die Welt einmal ohne HUD und Hand zeichnen und auslesen (s. keep_last_view).
    pub(crate) fn capture_clean_view(&mut self) -> Option<wallpaper::Decoded> {
        // Ohne geladene Welt (Abbruch beim Verbinden) gibt es nichts Neues zu zeigen.
        if self.gpu.is_none() || self.world.column_keys().next().is_none() {
            return None;
        }
        let versteckt = self.hud_hidden;
        let modus = self.mode;
        self.hud_hidden = true;
        // Im Menue-Modus zeichnet build_hand_vertices die Hand nie (auch kein Totem).
        self.mode = AppMode::Menu;
        let _ = self.draw_world_frame(None, false);
        self.hud_hidden = versteckt;
        self.mode = modus;
        self.gpu.as_ref()?.read_world_view()
    }

    // Verbindungsbildschirm mit dem letzten Blick dieses Servereintrags statt der leeren Welt.
    pub(crate) fn connect_view_shown(&self) -> bool {
        self.connecting_view()
            && self.connect_view_for.as_deref() == Some(self.server_address.as_str())
            && self.gpu.as_ref().is_some_and(|g| g.connect_view.is_some())
    }

    // Beim Verbinden: den gespeicherten Blick dieses Servereintrags laden, falls nicht schon da.
    pub(crate) fn request_connect_view(&mut self) {
        if self.connect_view_for.as_deref() == Some(self.server_address.as_str()) {
            return;
        }
        self.connect_view_for = None;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.connect_view = None;
        }
        self.connect_view_rx =
            shared_config_dir().map(|dir| (self.server_address.clone(), last_view::load(last_view::server_path(&dir, &self.server_address))));
    }

    // Den gespeicherten letzten Weltblick einmal nach dem Start laden und hochladen.
    pub(crate) fn poll_last_view(&mut self) {
        if let Some((fuer, rx)) = &self.connect_view_rx {
            match rx.try_recv() {
                Ok(bild) => {
                    let fuer = fuer.clone();
                    self.connect_view_rx = None;
                    if let (Some(bild), Some(gpu)) = (bild, self.gpu.as_mut()) {
                        gpu.set_connect_view(&bild);
                        self.connect_view_for = Some(fuer);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.connect_view_rx = None,
            }
        }
        if !self.last_view_requested && self.gpu.is_some() {
            self.last_view_requested = true;
            self.last_view_rx = shared_config_dir().map(|dir| last_view::load(last_view::path(&dir)));
        }
        let Some(rx) = &self.last_view_rx else { return };
        match rx.try_recv() {
            Ok(bild) => {
                self.last_view_rx = None;
                // Schon ein frischerer da (Welt verlassen, waehrend noch geladen wurde)? Dann
                // bleibt der.
                if let (Some(bild), Some(gpu)) = (bild, self.gpu.as_mut()) {
                    if gpu.last_view.is_none() {
                        gpu.set_last_view(&bild);
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.last_view_rx = None,
        }
    }

    // Ein Weltbild zeichnen: Vertices bauen, hochladen, GpuState::render_world. Mit
    // `acquired` = None nur ins offscreen_view, ohne HUD -- das laufende Bild hinter dem
    // Pausemenue (s. world_backdrop_visible), statt eines eingefrorenen.
    // Shader-Pfad: Voxel-Abbild um die Kamera nachfuehren (s. render::voxel_volume). Legt den
    // Shader-Pfad beim ersten Mal an -- die Voxel-Klassen brauchen die Blockformen der App.
    // Weltaenderungen werden auch ohne Shader-Pfad abgeholt, sonst sammeln sie sich an.
    pub(crate) fn update_voxel_volume(&mut self) {
        let (cleared, columns) = self.world.take_voxel_changes();
        let Some(gpu) = self.gpu.as_mut() else { return };
        if !gpu.use_shader_path {
            if let Some(sp) = gpu.shader_path.as_mut() {
                sp.voxels_mut().note_changes(cleared, &columns);
            }
            return;
        }
        let format = gpu.config.format;
        let (shapes, cross, uvs) = (&self.block_shapes, &self.cross_blocks, &self.block_uvs);
        let sp = gpu.shader_path.get_or_insert_with(|| {
            let atlas = gpu.block_renderer.atlas_texture().create_view(&wgpu::TextureViewDescriptor::default());
            let mut sp = render::shader_path::ShaderPath::new(&gpu.device, format, world::voxel::VoxelClasses::build(shapes, cross, uvs), atlas);
            // Das Licht-Raster im Hintergrund bauen: sonst ruckelt jeder Schritt ueber eine
            // Zellgrenze (s. VoxelVolume::set_async_lights).
            sp.voxels_mut().set_async_lights(true);
            sp
        });
        let voxels = sp.voxels_mut();
        voxels.note_changes(cleared, &columns);
        voxels.set_world_bottom(self.world.min_y());
        let eye = self.camera.position;
        let s = world::SECTION_SIZE as f64;
        let center = [(eye.x / s).floor() as i32, (eye.y / s).floor() as i32, (eye.z / s).floor() as i32];
        let welt = &self.world;
        voxels.update(
            &gpu.queue,
            center,
            |sx, sy, sz| {
                let column = welt.column(sx, sz)?;
                let i = sy - column.min_y.div_euclid(world::SECTION_SIZE);
                usize::try_from(i).ok().and_then(|i| column.sections.get(i))
            },
            VOXEL_SECTIONS_PER_FRAME,
        );
        voxels.update_lights(&gpu.queue, [eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32]);
    }

    pub(crate) fn draw_world_frame(&mut self, acquired: Option<(wgpu::SurfaceTexture, f32)>, with_hud: bool) -> Option<FrameTiming> {
        use profiler::Phase;
        let t = Instant::now();
        if with_hud {
            self.chat_selection_autoscroll();
        }
        let upload_ms = self.advance_atlas_animations();
        self.profiler.add(Phase::Animations, t);
        self.profiler.add_ms(Phase::Animations, -upload_ms);
        self.profiler.add_ms(Phase::AnimUpload, upload_ms);
        self.update_zoom();
        self.camera.fov_degrees = match self.hero_fov {
            Some(fov) => fov,
            None => zoomed_fov(self.settings.fov_of(self.perspective), self.settings.zoom_factor_tenths, self.zoom_amount),
        };
        self.camera.projection = self.settings.projection_mode;
        let ui_started = Instant::now();
        // Ohne HUD (hinter einem Menue) zeichnet render_menu Benachrichtigungen und Zeiger.
        let (mut overlay, (hud_rects, hud_lines, hud_texts, hud_sprites, doll_split)) = if with_hud {
            self.tick_notifications();
            self.poll_resource_pack_job();
            let mut overlay = self.build_notifications();
            // Im Modul-Menue im Spiel gibt es ebenfalls Farbfelder und das Namensfenster.
            self.build_color_popup(&mut overlay);
            self.build_dropdown(&mut overlay);
            self.build_theme_name_dialog(&mut overlay);
            (overlay, self.build_hud())
        } else {
            (UiOverlay::default(), Default::default())
        };
        if with_hud {
            self.update_falling_glyphs(&hud_texts, &mut overlay);
        }
        self.profiler.add(Phase::Hud, ui_started);
        let t = Instant::now();
        // VOR dem Bauen der Vertices: neu aufgetauchte Items in den Atlas
        // holen (braucht &mut self und die GPU-Warteschlange, das Bauen
        // selbst nicht).
        self.ensure_held_item_textures();
        self.ensure_map_textures();
        self.ensure_trim_textures();
        self.scroll_wind_charge_texture();
        // Dasselbe fuer Skins, die aus dem Atlas verdraengt wurden, deren
        // Spieler aber wieder da ist. Kostet fast nichts, solange nichts
        // fehlt -- die Anfrage-Sperre laesst pro Verdraengung genau einen
        // Nachladeversuch durch (s. refresh_missing_skins).
        self.refresh_missing_skins();
        self.request_head_skins();
        self.profiler.add(Phase::Textures, t);
        let t = Instant::now();
        // Glint-Twins (s. glint_out): je Bau leeren und gleich danach abholen.
        self.glint_out.borrow_mut().clear();
        // Items in der Welt ueber den Core-Shader eines Resourcepacks, falls einer ihn ersetzt
        // (s. held_item::HeldItems::set_vanilla_route).
        let pack_items = self.gpu.as_ref().is_some_and(|g| g.block_renderer.vanilla.has_item_program());
        self.held_items.set_vanilla_route(pack_items);
        let (entity_verts, entity_tints, entity_lights, entity_translucent, (outline_verts, outline_colors)) = self.build_entity_vertices();
        self.entity_vertex_hint = entity_verts.len();
        self.held_items.set_vanilla_route(false);
        let vanilla_items = self.vanilla_item_lights(self.held_items.take_vanilla_vertices());
        let entity_glint = self.glint_out.take();
        self.profiler.add(Phase::Entities, t);
        let t = Instant::now();
        let mut hitbox_verts = self.build_hitbox_vertices(self.gpu.as_ref().map(|g| g.size().1).unwrap_or(1080.0));
        let selection_verts = self.build_block_selection_vertices();
        // Komplementaer: eigener Durchgang nach der Welt (s. render::OutlineComplement). Nur im
        // PvP-Pfad -- der Shader-Pfad zeichnet den Umriss wie bisher in seiner Farbe.
        let komplementaer = self.settings.module_block_outline
            && self.settings.block_outline_complementary
            && self.gpu.as_ref().is_some_and(|g| !g.use_shader_path);
        let outline_complement = if komplementaer { selection_verts } else { hitbox_verts.extend(selection_verts); Vec::new() };
        self.profiler.add(Phase::Hitboxes, t);
        let t = Instant::now();
        let hand_verts = self.build_hand_vertices();
        // Shader-Pfad: die Hand im Licht an den Augen (s. render::BlockRenderer::hand_light).
        let hand_light = (self.settings.render_path == 1).then(|| self.light_near(self.camera.position));
        let hand_glint = self.glint_out.take();
        self.profiler.add(Phase::Hand, t);
        // Fadenkreuz nur in der Ego-Ansicht -- in der 3. Person saesse es
        // in der Bildmitte, waehrend der Blickstrahl vom Spieler ausgeht.
        // Bei offenem Inventar auch kein Fadenkreuz -- man zielt gerade
        // nicht in die Welt, sondern sortiert Items.
        let wackeln = self.arrow_empty_shake_px() / self.gpu.as_ref().map_or(1920.0, |g| g.size().0);
        let crosshair_center = (with_hud && self.crosshair_visible())
            .then_some((0.5 + wackeln, 0.5));
        // Feuer-Modus "Fadenkreuz": beim Brennen stehen Kreuz und Ring in der eigenen Farbe --
        // auch anstelle der Komplementaerfarbe. Die Zahlen daneben (Pfeile, Countdown) bleiben
        // in der normalen Fadenkreuz-Farbe (s. CrosshairExtras::fire_rgb). Nur wenn das Feuer
        // auch Schaden macht: nicht im Kreativ- oder Zuschauermodus, nicht mit Feuerresistenz.
        let brennt = self.settings.fire_mode == FIRE_MODE_CROSSHAIR
            && self.own_on_fire
            && !matches!(self.game_mode, GAME_MODE_CREATIVE | GAME_MODE_SPECTATOR)
            && !self.own_effects.contains_key(&protocol::packets::MOB_EFFECT_FIRE_RESISTANCE);
        let crosshair_mode = self.settings.crosshair_color_mode as u32;
        let crosshair_rgb = self.settings.crosshair_rgb();
        let crosshair_outline = self.settings.crosshair_outline;
        let mut crosshair_extras = if with_hud { self.crosshair_extras() } else { CrosshairExtras::default() };
        crosshair_extras.fire_rgb = brennt.then(|| self.settings.fire_crosshair_rgb());
        // Nur der Komplementaer-Modus kann aufgehellt werden -- eine fest gewaehlte Farbe
        // ist ja bereits genau die gewaehlte Farbe.
        let crosshair_boost =
            if crosshair_mode == 0 { self.settings.crosshair_brightness_boost as f32 / 100.0 } else { 0.0 };
        // Bei offenem Inventar zeichnen wir den Zeiger selbst (die Maus ist
        // im Vollbild gefangen, es gibt keinen System-Cursor). Haengt gerade
        // ein Stapel an der Maus, ersetzt DER den Zeiger -- sonst laege die
        // Pfeilspitze mitten im Item.
        let inventory_cursor = (with_hud
            && (self.module_menu_open
                || self.chat_input.is_some()
                || self.connecting_view()
                || self.dialog_shown()
                || self.code_of_conduct.is_some()
                || (self.inventory.screen_open && self.inventory.cursor.is_none())))
            .then_some(self.cursor_pos)
            // Spielmodus-Rad: derselbe Zeiger, an der Stelle, die das Rad fuehrt.
            .or_else(|| self.gamemode_wheel_cursor().filter(|_| with_hud));
        // Geisterbloecke JEDEN Frame neu bauen. Ihre Vertices sind
        // augenrelativ (s. ghost_vertices) -- ein zwischengespeicherter Satz
        // behaelt den Versatz zur ALTEN Kameraposition und wandert damit beim
        // Gehen sichtbar mit. Es sind hoechstens ein paar Dutzend
        // unbestaetigte Platzierungen (s. GHOST_VERTEX_CAPACITY), der Neubau
        // kostet also nichts, was ein Zwischenspeicher wert waere.
        let t = Instant::now();
        self.step_ao_fades();
        let (ghost_verts, ghost_solid, ghost_translucent) = self.ghost_vertices();
        // Shader-Pfad: Licht je Vertex fuer den deckenden Teil (s. BlockRenderer::set_ghost_vertices).
        let ghost_lights: Vec<u32> = if self.settings.render_path == 1 {
            let eye = self.camera.position;
            ghost_verts[..ghost_solid].iter().map(|v| self.light_near(eye + glam::Vec3::from(v.position).as_dvec3())).collect()
        } else {
            Vec::new()
        };
        let crumbling_verts = self.build_crumbling_vertices();
        let particle_draws = self.build_particle_vertices();
        let [regen, schnee] = self.build_weather_vertices();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.block_renderer.set_particle_vertices(&gpu.queue, &self.particle_vertices, particle_draws);
            // Texturen neu hochladen, wenn das Modul umschaltet (s. WeatherTexture::chosen).
            let modul = self.settings.module_weather;
            if self.weather_textures_uploaded != Some(modul) {
                let [r, s] = &self.weather_textures;
                gpu.block_renderer.set_weather_textures(
                    &gpu.device,
                    &gpu.queue,
                    weather_texture_part(r.chosen(modul)),
                    weather_texture_part(s.chosen(modul)),
                );
                self.weather_textures_uploaded = Some(modul);
            }
            gpu.block_renderer.set_weather_vertices(&gpu.queue, &self.weather_vertices, regen, schnee);
        }
        self.profiler.add(Phase::WorldExtras, t);
        self.update_voxel_volume();
        self.update_chunk_saver();
        self.update_custom_sky();
        // Vor dem &mut-Ausleihen der GPU berechnen -- danach ist `self` nicht
        // mehr lesbar.
        let sky = self.sky_clear_color();
        let sky_module = self.sky_color_module_active();
        let scattering = self.settings.shader_scattering_percent as f32 / 100.0;
        let sun_size_deg = self.settings.shader_sun_size_tenths as f32 / 10.0;
        let block_shadows = self.settings.shader_block_shadows;
        let block_softness_deg = self.settings.shader_block_softness_deg as f32;
        let lightmap_debug = self.settings.shader_lightmap_debug;
        let atmosphere = !matches!(self.dimension_type_name.as_str(), "minecraft:the_nether" | "minecraft:the_end");
        let cursor_fx = self.cursor_fx_frame();
        // Ebenfalls vor dem &mut-Ausleihen der GPU: build_doll liest das
        // Inventar, den Skin und die Zeigerposition.
        let t = Instant::now();
        let doll = self.gpu.as_ref().filter(|_| with_hud).and_then(|gpu| self.build_doll(gpu.size()));
        let doll_glint = self.glint_out.take();
        self.profiler.add(Phase::Doll, t);
        let t_upload = Instant::now();
        // Die wandernde Glint-Matrix haengt an der Uhr, nicht am Tick.
        let glint_ms = self.start_time.elapsed().as_millis() as u64;
        let glint_speed = self.settings.glint_speed as f32 / 100.0;
        let glint_strength = self.settings.glint_strength as f32 / 100.0;
        // Die Weltgrenze in Sichtweite (Vanilla: die eingestellte Sichtweite in Bloecken).
        let border_draw = render::vanilla_draw::world_border_draw(&self.world.border, self.tick_progress(), self.camera.position, self.settings.max_render_distance as f64 * 16.0);
        let upload_ms = &mut 0.0f32;
        let timing = self.gpu
            .as_mut()
            .and_then(|gpu| {
                gpu.block_renderer.set_ghost_vertices(&gpu.queue, &ghost_verts, ghost_solid, ghost_translucent, &ghost_lights);
                gpu.block_renderer.set_crumbling_vertices(&gpu.queue, &crumbling_verts);
                gpu.block_renderer.set_entity_vertices(&gpu.queue, &entity_verts, &entity_tints, &entity_lights, entity_translucent);
                gpu.block_renderer.set_outline_vertices(&gpu.queue, &outline_verts, &outline_colors);
                gpu.block_renderer.vanilla.set_item_vertices(&gpu.device, &gpu.queue, &vanilla_items);
                gpu.block_renderer.vanilla.set_border(&gpu.queue, self.camera.view_matrix(), glint_ms, border_draw.as_ref());
                gpu.block_renderer.vanilla_game_time = self.world_time_of_day.rem_euclid(24000) as f32 / 24000.0;
                gpu.block_renderer.set_entity_glint_vertices(&gpu.queue, &entity_glint);
                gpu.block_renderer.update_glint(&gpu.queue, glint_ms, glint_speed, glint_strength);
                gpu.update_gui_glint(glint_ms, glint_speed, glint_strength);
                // Linienbreite + Bildschirmgroesse: die Verbreiterung passiert
                // im Vertex-Shader, nicht in den Vertices (s. vs_hitbox).
                gpu.block_renderer.set_hitbox_params(gpu.size());
                gpu.block_renderer.set_hitbox_vertices(&gpu.queue, &hitbox_verts);
                gpu.block_renderer.set_outline_complement_vertices(&gpu.queue, &outline_complement);
                gpu.block_renderer.set_hand_vertices(&gpu.queue, &hand_verts);
                gpu.block_renderer.hand_light = hand_light;
                gpu.block_renderer.set_hand_glint_vertices(&gpu.queue, &hand_glint);
                // Spielerfigur im Inventar: Geometrie UND Kamera stehen erst
                // hier fest (beides haengt an der Fenstergroesse), und die
                // leere Liste raeumt sie wieder weg, sobald das Inventar zu
                // ist -- sonst haenge sie im naechsten Frame noch im Puffer.
                match &doll {
                    Some((verts, view, proj)) => {
                        gpu.block_renderer.update_doll_camera(&gpu.queue, *view, *proj);
                        gpu.block_renderer.set_doll_vertices(&gpu.queue, verts);
                        gpu.block_renderer.set_doll_glint_vertices(&gpu.queue, &doll_glint);
                    }
                    None => {
                        gpu.block_renderer.set_doll_vertices(&gpu.queue, &[]);
                        gpu.block_renderer.set_doll_glint_vertices(&gpu.queue, &[]);
                    }
                }
                *upload_ms = t_upload.elapsed().as_secs_f32() * 1000.0;
                // Tageslicht fuer den Shader-Pfad (Himmelslicht nachts gedimmt, s. block.wgsl).
                // Mit festem Himmel (Modul Himmelsfarbe) gilt auch das Licht fest: die
                // Helligkeit der gewaehlten Farbe -- sonst stuende nachts ein heller Himmel
                // ueber einer dunklen Welt.
                gpu.block_renderer.daylight = if sky_module {
                    self.settings.module_sky_color_value as f32 / 100.0
                } else {
                    world::sky::daylight_factor(self.world_time_of_day)
                };
                gpu.sky_angle = world::sky::sky_angle(self.world_time_of_day);
                gpu.atmosphere = atmosphere;
                gpu.scattering = scattering;
                gpu.sun_size_deg = sun_size_deg;
                gpu.block_shadows = block_shadows;
                gpu.block_softness_deg = block_softness_deg;
                gpu.lightmap_debug = lightmap_debug;
                gpu.render_world(
                    &self.camera,
                    sky,
                    &hud_rects,
                    &hud_lines,
                    &hud_texts,
                    &hud_sprites,
                    &overlay,
                    inventory_cursor,
                    cursor_fx,
                    crosshair_center,
                    crosshair_mode,
                    crosshair_rgb,
                    crosshair_boost,
                    crosshair_outline,
                    crosshair_extras,
                    // CMAA2 deaktiviert (s. CMAA2_ROW-Kommentar), unabhaengig
                    // vom gespeicherten Settings-Wert -- die Umschalt-UI ist
                    // ausgehaengt, ein alter true-Wert aus settings.conf soll
                    // trotzdem nicht mehr greifen.
                    false,
                    self.settings.color_saturation(),
                    doll_split,
                    acquired,
                )
            });
        self.profiler.add_ms(Phase::Upload, *upload_ms);
        timing
    }

    // Menues ueber der Welt (Pause, Einstellungen, HUD-Editor, ...) zeigen das letzte
    // Weltbild statt Schwarz, solange man verbunden ist -- wie in Vanilla.
    pub(crate) fn world_backdrop_visible(&self) -> bool {
        self.mode == AppMode::Menu && self.conn_status == ConnStatus::Connected
    }

    // Die eingestellte Quelle (neu) laden, s. wallpaper::Loader::request. Verkleinert wird
    // auf den Monitor, nicht aufs Fenster: sonst waere das Bild nach dem Maximieren unscharf.
    pub(crate) fn request_wallpaper(&mut self, force: bool) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let ziel = gpu.window.current_monitor().map(|m| (m.size().width, m.size().height)).unwrap_or((gpu.config.width, gpu.config.height));
        let modus = wallpaper::WallpaperMode::from_index(self.settings.wallpaper);
        let gespeichert = shared_config_dir().map(|d| wallpaper::stored_path(&d));
        self.wallpaper_checked_at = Instant::now();
        self.wallpaper_loader.request(modus, gespeichert, hero_scene::path(), ziel, force);
    }

    // Einmal pro Durchlauf: fertige Bilder hochladen, Dialog-Ergebnisse umsetzen, und beim
    // Desktop-Hintergrund ab und zu nachsehen, ob er gewechselt wurde.
    pub(crate) fn poll_wallpaper(&mut self) {
        for event in self.wallpaper_loader.poll() {
            match event {
                wallpaper::Event::Loaded(bild) => {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.set_wallpaper(Some(&bild));
                    }
                }
                wallpaper::Event::Cleared => {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.set_wallpaper(None);
                    }
                }
                wallpaper::Event::Failed(e) => {
                    self.push_log(format!("Hintergrundbild: {e}"));
                    self.notify(notifications::Notification::warning("Hintergrundbild nicht geladen", e));
                }
                wallpaper::Event::Imported => {
                    self.settings.wallpaper = wallpaper::WallpaperMode::File.index();
                    self.persist_settings();
                    self.request_wallpaper(true);
                }
                wallpaper::Event::ImportFailed(e) => {
                    self.push_log(format!("Hintergrundbild: {e}"));
                    self.notify(notifications::Notification::error("Bild nicht übernommen", e));
                }
                wallpaper::Event::PickCancelled => {}
            }
        }
        if wallpaper::WallpaperMode::from_index(self.settings.wallpaper) == wallpaper::WallpaperMode::Desktop
            && self.wallpaper_visible()
            && self.wallpaper_checked_at.elapsed() >= WALLPAPER_DESKTOP_RECHECK
        {
            self.request_wallpaper(false);
        }
    }

    // Feuer vor den Augen, Modus Vanilla (ScreenEffectRenderer.submitFire): zwei Bilder der
    // Feuertextur in den unteren Ecken, je eine Einheit gross, eine halbe Einheit vor dem Auge,
    // 0,24 zur Seite, 0,3 nach unten und um 10 Grad zur Mitte gedreht -- gesehen mit 70 Grad
    // Sichtfeld wie die Hand. "Hoehe" schiebt beide nach unten, bei 0 % ganz aus dem Bild.
    // In Streifen zerlegt, weil die Sprites ihre UVs linear ueber zwei Dreiecke legen und das
    // schraeg stehende Bild sonst einen Knick in der Diagonale bekaeme.
    pub(crate) fn build_fire_overlay(&self, gpu: &GpuState, sprites: &mut Vec<UiSprite>) {
        if self.mode != AppMode::World
            || !self.own_on_fire
            || self.game_mode == GAME_MODE_SPECTATOR
            || !self.perspective.is_first_person()
            || self.settings.fire_mode != FIRE_MODE_VANILLA
            || self.settings.fire_height == 0
        {
            return;
        }
        let Some(feuer) = self.ui_atlas.rect(&format!("tex/{}", assets::FIRE_OVERLAY)) else { return };
        let (sw, sh) = gpu.size();
        let tan = 35f32.to_radians().tan();
        let aspect = sw / sh.max(1.0);
        let senken = (1.0 - self.settings.fire_height as f32 / 100.0) * 0.55;
        let [u0, v0, u1, v1] = feuer.uv;
        const STREIFEN: usize = 8;
        for (seite, winkel) in [(0.24f32, -10f32), (-0.24, 10.0)] {
            let (sin, cos) = winkel.to_radians().sin_cos();
            let bild = |lx: f32, ly: f32| {
                let (x, z) = (lx * cos - 0.5 * sin + seite, -lx * sin - 0.5 * cos);
                let y = ly - 0.3 - senken;
                let (nx, ny) = (x / (-z * tan * aspect), y / (-z * tan));
                [(nx + 1.0) / 2.0 * sw, (1.0 - ny) / 2.0 * sh]
            };
            for i in 0..STREIFEN {
                let (a, b) = (i as f32 / STREIFEN as f32, (i + 1) as f32 / STREIFEN as f32);
                let (xa, xb) = (a - 0.5, b - 0.5);
                // Links im Bild liegt das rechte Ende der Textur (gespiegelt wie in Vanilla).
                let (ua, ub) = (u1 + (u0 - u1) * a, u1 + (u0 - u1) * b);
                sprites.push(UiSprite {
                    corners: [bild(xa, 0.5), bild(xb, 0.5), bild(xb, -0.5), bild(xa, -0.5)],
                    uv: [ua, v0, ub, v1],
                    color: [1.0, 1.0, 1.0, 0.9],
                    atlas: SpriteAtlas::Ui,
                });
            }
        }
    }

    // Hurt-Cam: Vanillas Verlauf (GameRenderer.bobHurt) -- schnell hin und weich zurueck ueber
    // die zehn Ticks nach dem Treffer --, aber als reines Rollen um die Blickachse und mit
    // jedem Treffer zur anderen Seite.
    pub(crate) fn hurt_cam_roll(&self) -> f32 {
        if !self.settings.hurt_cam || self.own_hurt_ticks == 0 || self.game_mode == GAME_MODE_SPECTATOR {
            return 0.0;
        }
        let t = ((self.own_hurt_ticks as f32 - self.tick_progress()) / world::MAX_HURT_TIME as f32).clamp(0.0, 1.0);
        let verlauf = (t.powi(4) * std::f32::consts::PI).sin();
        self.hurt_cam_side * verlauf * HURT_CAM_MAX_DEGREES * self.settings.hurt_cam_strength as f32 / 100.0
    }

    // Kameraversatz der aktuellen Perspektive (Vanillas Camera.update()-Zweig fuer
    // thirdPerson): in der Frontansicht erst die Blickrichtung umdrehen, dann in BEIDEN
    // 3.-Person-Ansichten um THIRD_PERSON_DISTANCE nach hinten weg -- weshalb die Kamera
    // vorn vor dem Spieler landet und ihn anschaut. Die Distanz wird an Waenden verkuerzt
    // (s. physics::clip_camera_distance), sonst steckte man in der Wand und saehe ins Leere.
    pub(crate) fn update_perspective_offset(&mut self) {
        self.camera.roll_degrees = self.hurt_cam_roll();
        self.camera.view_inverted = self.perspective.is_front_view();
        if self.perspective.is_first_person() {
            self.camera.view_offset = glam::Vec3::ZERO;
            return;
        }
        // Hoehenversatz ZUERST auf das Auge legen: das Zurueckweichen (und damit auch die
        // Wandpruefung) soll von der tatsaechlichen Kameraposition ausgehen, nicht von der
        // Augenhoehe -- sonst rutschte eine angehobene Kamera hinterher doch noch in die
        // Decke, weil der Strahl eine Etage tiefer geprueft hat.
        let y_offset = self.settings.camera_y_offset_of(self.perspective) as f64;
        let eye = self.camera.position + DVec3::new(0.0, y_offset, 0.0);
        let back = -self.camera.view_forward().as_dvec3();
        let wanted = self.settings.camera_distance_of(self.perspective) as f64;
        let distance = physics::clip_camera_distance(eye, back, wanted, &self.world, &self.block_shapes);
        self.camera.view_offset = (back * distance + DVec3::new(0.0, y_offset, 0.0)).as_vec3();
    }
}
