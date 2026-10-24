// Abbauen, Benutzen und Platzieren von Bloecken und Items, Vorhersage auf dem Client.

use crate::*;

impl App {
    // Eine Eingabe-Aktion fuer den naechsten Tick vormerken. Der Deckel ist reine Vorsicht:
    // solange die eigene Spalte nicht geladen ist, laeuft kein Tick (s. update_movement) --
    // ohne Grenze wuechse die Liste dann unbegrenzt weiter.
    pub(crate) fn queue_input_action(&mut self, action: PendingInputAction) {
        // Solange die Welt noch laedt, wird gar nichts gemerkt. Vanilla ruft handleKeybinds()
        // nur bei `screen == null && overlay == null` auf, und MouseHandler.onPress legt einen
        // Klick ueberhaupt erst dann in die Keybind-Warteschlange -- Klicks im
        // Verbinden-/Terrain-Bildschirm gehen dort also ersatzlos verloren.
        //
        // Ohne diese Sperre sammelten sich alle Klicks aus dem Ladebildschirm in
        // pending_input_actions und gingen im ERSTEN Welt-Tick auf einen Schlag raus. Im
        // Proxy-Mitschnitt von anticheat-test.com waren das vier Swing-Pakete in einem
        // einzigen Tick -- etwas, das ein Vanilla-Client nie sendet (ein Tick sind 50 ms).
        if self.player_loaded_pending {
            return;
        }
        if self.slot_queue.add_action(action) {
            return;
        }
        if self.pending_input_actions.len() < 64 {
            self.pending_input_actions.push(action);
        }
    }

    // ClientPlayerInteractionManager.syncSelectedSlot(): nur bei Abweichung ein Paket, und
    // immer mit dem AKTUELLEN Slot -- Zwischenschritte eines schnellen Scrolls fallen weg.
    pub(crate) fn sync_selected_slot(&mut self) {
        if self.last_synced_slot == self.inventory.held_slot {
            return;
        }
        self.last_synced_slot = self.inventory.held_slot;
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::set_carried_item_payload(self.inventory.held_slot as u16));
        }
    }

    // Alles, was Vanilla im Tick VOR dem Bewegungspaket verschickt: erst der Hotbar-Abgleich
    // (MinecraftClient.tick -> interactionManager.tick -> syncSelectedSlot), dann die
    // Eingabe-Aktionen (handleInputEvents) -- und ERST DANACH tickt der Spieler und sendet
    // seine Bewegung.
    //
    // ECHTER BUG gewesen (Grim "Post: held item change" mit x20/x40/x60-Zaehler
    // beim Hotbar-Scrollen): diese Pakete gingen im Moment des Tastendrucks raus, also an
    // einer beliebigen Stelle im Frame. Grims Post-Check (checks/impl/misc/Post, aus dem
    // Server-Jar nachgesehen) merkt sich nach jedem Bewegungspaket alle eingehenden Pakete
    // einer festen Liste -- HELD_ITEM_CHANGE, ATTACK/INTERACT_ENTITY, PLAYER_DIGGING,
    // PLAYER_ABILITIES, USE_ITEM, BLOCK_PLACEMENT -- und flaggt, sobald die Transaktion
    // (Ping-Antwort) eintrifft und die Liste nicht leer ist. Vanilla kann diese Reihenfolge
    // gar nicht erzeugen, weil es Eingaben nicht sofort verschickt, sondern gepuffert im
    // Tick abarbeitet. Genau das macht diese Funktion nach.
    pub(crate) fn flush_input_actions(&mut self) {
        // Naechste wartende Slottaste -- erst, wenn der aktuelle Slot seinen Tick hatte (an den
        // Server gesendet) und seine Klicks abgearbeitet sind.
        let isst = self.server_still_eating();
        let frei = self.inventory.held_slot == self.last_synced_slot && self.use_queue == 0 && !isst;
        if let Some(naechster) = self.slot_queue.next_for_tick(frei) {
            self.select_hotbar_slot(naechster.slot);
            self.use_queue = naechster.uses;
            self.pending_input_actions.extend(naechster.actions);
        }
        self.sync_selected_slot();
        // Reihenfolge wie Vanillas handleKeybinds: Hotbar, ..., keySwapOffhand, keyDrop, und
        // ERST DANACH der Block mit Angriff und
        // Benutzen. Tausch und Wegwerfen gehen also vor; die Angriffe bleiben liegen und
        // werden in tick_item_use an genau der Stelle abgearbeitet, an der Vanilla sie hat.
        //
        // Isst der Server noch (s. server_still_eating), warten Tausch und Wegwerfen: beides
        // braeche das Essen dort ab.
        let mut angriffe = Vec::new();
        for action in std::mem::take(&mut self.pending_input_actions) {
            match action {
                PendingInputAction::Attack => angriffe.push(action),
                _ if isst => angriffe.push(action),
                PendingInputAction::DropItem(ganz) => self.drop_selected_item(ganz),
                PendingInputAction::SwapOffhand => self.send_player_action(protocol::packets::PLAYER_ACTION_SWAP_OFFHAND),
            }
        }
        self.pending_input_actions = angriffe;
        self.attack_broke_block = false;
        // Vanillas hitResult: EINMAL pro Tick bestimmt (gameRenderer.pick in tick(), vor
        // handleKeybinds), und continueAttack am Ende arbeitet mit genau diesem Ziel -- auch
        // wenn ein Rechtsklick dazwischen einen Block davorgesetzt hat. Hier zielte der Abbau
        // mit einem frischen Strahl und sprang im SELBEN Tick auf den gerade gesetzten Block:
        // ABORT+START auf eine Stelle, die Grim erst mit dem naechsten Bewegungspaket als
        // belegt kennt -- AirLiquidBreak ("block=air, type=START_DIGGING"), gemessen gegen
        // Fabric mit Crystal Fix: dort folgt der Wechsel einen Tick spaeter.
        let tick_ziel = self.crosshair_target();
        self.tick_item_use();
        // Hier liegt nur noch etwas, wenn tick_item_use vor dem Benutzen-Block ausgestiegen ist
        // (Bildschirm/Chat offen) -- dort wie bisher. Zurueckgestellter Tausch und Wegwerfen
        // bleiben fuer den naechsten Tick liegen.
        for action in std::mem::take(&mut self.pending_input_actions) {
            match action {
                PendingInputAction::Attack => self.send_attack(),
                _ => self.pending_input_actions.push(action),
            }
        }
        // Letzte Zeile von handleInputEvents:
        //   handleBlockBreaking(currentScreen == null && !bl3 && attackKey.isPressed() && mouse.isCursorLocked())
        let welt = self.mode == AppMode::World && !self.screen_overlay_open() && self.chat_input.is_none();
        let gehalten = self.keybind_physical_down[KeyBindAction::Attack.index()];
        self.handle_block_breaking(welt && !self.attack_broke_block && gehalten, tick_ziel);
    }

    // attack_range des aktiven Items -- des gerade benutzten, sonst der Haupthand (Vanillas
    // LivingEntity.getActiveItem). Zuschauer haben keins.
    pub(crate) fn active_attack_range(&self) -> Option<protocol::item::AttackRange> {
        if self.game_mode == GAME_MODE_SPECTATOR {
            return None;
        }
        let hand = if self.using_item && self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
        world::item_components::Components::of(self.own_hand_stack(hand)?).attack_range().copied()
    }

    // Ziel mit der Reichweite des Items (LocalPlayer.raycastHitResult, Zweig mit attack_range):
    // Entities nur zwischen Mindest- und Hoechstreichweite des Items, die Hoechstreichweite
    // waechst um das, was man sich gerade in Blickrichtung bewegt. Ein Block VOR der
    // Mindestreichweite faengt ab, einer dahinter kuerzt den Strahl. Ohne Entity bleibt ein
    // Block in Blockreichweite; sonst None, dann gilt die gewoehnliche Wahl. Bei einer Entity
    // dazu der Abstand des Trefferpunkts vom Auge.
    pub(crate) fn attack_range_target(&self, eye: DVec3, dir: DVec3) -> Option<(CrosshairTarget, f64)> {
        let r = self.active_attack_range()?;
        let kreativ = self.game_mode == GAME_MODE_CREATIVE;
        let (min, max) = if kreativ { (r.min_creative_reach, r.max_creative_reach) } else { (r.min_reach, r.max_reach) };
        let (min, max) = (min as f64, max as f64 + self.physics.vel.dot(dir).max(0.0));
        let block = physics::raycast_block(eye, dir, max, &self.world, &self.block_shapes);
        let block_dist = block.map(|hit| (DVec3::new(hit.block.0 as f64, hit.block.1 as f64, hit.block.2 as f64) + DVec3::new(hit.cursor.0 as f64, hit.cursor.1 as f64, hit.cursor.2 as f64)).distance(eye));
        let als_block = || match (block, block_dist) {
            (Some(hit), Some(d)) if d <= self.attr_block_range => Some((CrosshairTarget::Block(hit.block, hit.face), d)),
            _ => None,
        };
        if block_dist.is_some_and(|d| d < min) {
            return als_block();
        }
        let ende = block_dist.unwrap_or(max);
        // Vom Beruehrpunkt einer knapp verfehlten Box zu ihrer Mitte: wie weit ist frei?
        let frei = |von: (f64, f64, f64), nach: (f64, f64, f64)| -> f64 {
            let (a, b) = (DVec3::new(von.0, von.1, von.2), DVec3::new(nach.0, nach.1, nach.2));
            let laenge = a.distance(b);
            if laenge < 1e-9 {
                return 1.0;
            }
            let richtung = (b - a) / laenge;
            match physics::raycast_block(a, richtung, laenge, &self.world, &self.block_shapes) {
                Some(hit) => (DVec3::new(hit.block.0 as f64, hit.block.1 as f64, hit.block.2 as f64) + DVec3::new(hit.cursor.0 as f64, hit.cursor.1 as f64, hit.cursor.2 as f64)).distance(a) / laenge,
                None => 1.0,
            }
        };
        let margin = r.hitbox_margin as f64;
        if let Some((id, d)) = self.entities.pick_segment((eye.x, eye.y, eye.z), (dir.x, dir.y, dir.z), min, ende, margin, &frei) {
            // Minecraft.startAttack prueft den Trefferpunkt noch einmal gegen die Reichweite.
            if d >= min - margin && d <= max + margin {
                return Some((CrosshairTarget::Entity(id), d));
            }
        }
        als_block()
    }

    // Was unter dem Fadenkreuz liegt -- GameRenderer.findCrosshairTarget: erst der Block-
    // strahl (4.5), dann Entities, aber nur bis zur Entity-Reichweite (3) UND nicht hinter
    // dem getroffenen Block. Ein Item mit eigener Reichweite (attack_range) waehlt vorher
    // selbst (s. attack_range_target).
    pub(crate) fn crosshair_target(&self) -> CrosshairTarget {
        let attack_reach = self.attr_entity_range;
        let eye = self.physics.eye_pos();
        let dir = self.camera.forward();
        let dir = DVec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
        if let Some((ziel, _)) = self.attack_range_target(eye, dir) {
            return ziel;
        }
        let block = physics::raycast_block(eye, dir, self.attr_block_range, &self.world, &self.block_shapes);
        let block_dist = block.map(|hit| {
            let punkt = DVec3::new(
                hit.block.0 as f64 + hit.cursor.0 as f64,
                hit.block.1 as f64 + hit.cursor.1 as f64,
                hit.block.2 as f64 + hit.cursor.2 as f64,
            );
            punkt.distance(eye)
        });
        let entity_reach = block_dist.map_or(attack_reach, |d| d.min(attack_reach));
        if let Some(id) = self.entities.pick((eye.x, eye.y, eye.z), (dir.x, dir.y, dir.z), entity_reach) {
            return CrosshairTarget::Entity(id);
        }
        match block {
            Some(hit) => CrosshairTarget::Block(hit.block, hit.face),
            None => CrosshairTarget::Miss,
        }
    }

    // MinecraftClient.handleBlockBreaking.
    pub(crate) fn handle_block_breaking(&mut self, breaking: bool, ziel: CrosshairTarget) {
        if self.using_item || self.outgoing_tx.is_none() || self.conn_status != ConnStatus::Connected {
            return;
        }
        // Zuschauer bauen nichts ab, ihr Linksklick teleportiert (s. send_attack).
        if self.game_mode == GAME_MODE_SPECTATOR {
            self.cancel_block_breaking();
            return;
        }
        // Mit einer Stichwaffe wird gar nicht abgebaut (und auch nichts abgebrochen).
        if self.main_hand_piercing() {
            return;
        }
        if breaking {
            if let CrosshairTarget::Block(pos, face) = ziel {
                // No Tool Break: wie losgelassen -- ein laufender Abbau wird abgebrochen.
                if self.block_mining_guarded(pos) {
                    self.cancel_block_breaking();
                    return;
                }
                if self.world.block_at(pos.0, pos.1, pos.2) != world::STATE_AIR
                    && self.update_block_breaking_progress(pos, face)
                {
                    // spawnBlockBreakingParticle + swingHand -- der Schwung geht jeden Tick raus.
                    self.with_particles(|p, ctx, _| p.add_block_hit(pos, face as u8, ctx));
                    self.swing_arm();
                }
                return;
            }
        }
        self.cancel_block_breaking();
    }

    // No Tool Break: wuerde das Abbauen dieses Blocks ein geschuetztes Werkzeug gefaehrden?
    // Dann true und (hoechstens alle paar Sekunden) eine Benachrichtigung.
    pub(crate) fn block_mining_guarded(&mut self, pos: (i32, i32, i32)) -> bool {
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        let Some((name, rest)) = self.tool_break_guard(state) else { return false };
        let jetzt = Instant::now();
        if self.tool_break_notified_at.is_none_or(|t| jetzt.duration_since(t) >= NO_TOOL_BREAK_NOTIFY_EVERY) {
            self.tool_break_notified_at = Some(jetzt);
            self.notify(notifications::Notification::warning(
                "Abbauen gesperrt",
                format!("{name} hat nur noch {rest} Haltbarkeit -- erst mit Erfahrung reparieren."),
            ));
        }
        true
    }

    // Name und Resthaltbarkeit des Werkzeugs in der Haupthand, wenn No Tool Break das
    // Abbauen von `state` sperren muss:
    //   - Modul an, nicht im Kreativmodus (dort nutzt nichts ab),
    //   - Diamant- oder Netherit-Werkzeug (Spitzhacke, Axt, Schaufel, Hacke, Schwert),
    //   - mit Reparatur -- nur solche Werkzeuge lohnt es zu retten, sie lassen sich per
    //     Erfahrung wieder auffuellen,
    //   - Resthaltbarkeit bei oder unter der Reserve,
    //   - und der Block kostet ueberhaupt Haltbarkeit: Bloecke mit Haerte 0 (Gras, Blumen,
    //     Fackeln) nutzen kein Werkzeug ab (MiningToolComponent.postMine), die bleiben frei.
    pub(crate) fn tool_break_guard(&self, state: u32) -> Option<(String, i32)> {
        if !self.settings.module_no_tool_break || self.game_mode == GAME_MODE_CREATIVE {
            return None;
        }
        if state == world::STATE_AIR || world::mining::hardness(state) == 0.0 {
            return None;
        }
        let stack = self.own_hand_stack(protocol::packets::HAND_MAIN)?;
        let name = world::item_registry::item_name(stack.item_id)?;
        let standard = protected_tool_durability(name)?;
        if !self.enchantments.has(stack, "minecraft:mending") {
            return None;
        }
        let rest = max_damage_of(stack, name).unwrap_or(standard) - stack.damage.unwrap_or(0);
        (rest <= self.settings.no_tool_break_reserve as i32).then(|| (tool_display_name(stack, name), rest))
    }

    // ClientPlayerInteractionManager.attackBlock -- der erste Linksklick auf einen Block.
    pub(crate) fn attack_block(&mut self, pos: (i32, i32, i32), face: i32) -> bool {
        if self.block_breaking_restricted(pos) {
            return false;
        }
        if self.game_mode == GAME_MODE_CREATIVE {
            let sequence = self.next_block_sequence();
            self.send_destroy_action(protocol::packets::PLAYER_ACTION_START_DESTROY_BLOCK, pos, face, sequence);
            self.break_block_client(pos, sequence);
            self.block_breaking_cooldown = 5;
        } else if !self.breaking_block || !self.is_currently_breaking(pos) {
            if self.breaking_block {
                if let (Some(alt), Some(tx)) = (self.breaking_pos, &self.outgoing_tx) {
                    self.dig_packet_this_tick = true;
                    let _ = tx.send(protocol::packets::player_action_block_payload(
                        protocol::packets::PLAYER_ACTION_ABORT_DESTROY_BLOCK,
                        alt.0,
                        alt.1,
                        alt.2,
                        face,
                        0,
                    ));
                }
            }
            let state = self.world.block_at(pos.0, pos.1, pos.2);
            let sequence = self.next_block_sequence();
            self.send_destroy_action(protocol::packets::PLAYER_ACTION_START_DESTROY_BLOCK, pos, face, sequence);
            if state != world::STATE_AIR && self.breaking_delta(state) >= 1.0 {
                self.break_block_client(pos, sequence);
            } else {
                self.breaking_block = true;
                self.breaking_pos = Some(pos);
                self.breaking_selected = self.own_hand_stack(protocol::packets::HAND_MAIN).cloned();
                self.breaking_progress = 0.0;
                self.block_breaking_sound_cooldown = 0;
            }
        }
        true
    }

    // ClientPlayerInteractionManager.updateBlockBreakingProgress -- jeder Tick mit gehaltener
    // Taste. true heisst: der Arm schwingt.
    pub(crate) fn update_block_breaking_progress(&mut self, pos: (i32, i32, i32), face: i32) -> bool {
        if self.block_breaking_cooldown > 0 {
            self.block_breaking_cooldown -= 1;
            return true;
        }
        if self.game_mode == GAME_MODE_CREATIVE {
            self.block_breaking_cooldown = 5;
            let sequence = self.next_block_sequence();
            self.send_destroy_action(protocol::packets::PLAYER_ACTION_START_DESTROY_BLOCK, pos, face, sequence);
            self.break_block_client(pos, sequence);
            return true;
        }
        if !self.is_currently_breaking(pos) {
            return self.attack_block(pos, face);
        }
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        if state == world::STATE_AIR {
            self.breaking_block = false;
            return false;
        }
        self.breaking_progress += self.breaking_delta(state);
        // Der Schlagklang alle vier Ticks spielt der eigene Client (PositionedSoundInstance,
        // kein Paket): Lautstaerke (v+1)/8, Tonhoehe p*0.5.
        if self.block_breaking_sound_cooldown.is_multiple_of(4) {
            let gruppe = world::block_sounds::sound_group(world::block_name(state).unwrap_or(""));
            self.play_block_sound(gruppe.hit, pos, (gruppe.volume + 1.0) / 8.0, gruppe.pitch * 0.5);
        }
        self.block_breaking_sound_cooldown += 1;
        if self.breaking_progress >= 1.0 {
            self.breaking_block = false;
            let sequence = self.next_block_sequence();
            self.send_destroy_action(protocol::packets::PLAYER_ACTION_STOP_DESTROY_BLOCK, pos, face, sequence);
            self.break_block_client(pos, sequence);
            self.breaking_progress = 0.0;
            self.block_breaking_cooldown = 5;
        }
        true
    }

    // ClientPlayerInteractionManager.cancelBlockBreaking -- Taste los oder Ziel verloren.
    pub(crate) fn cancel_block_breaking(&mut self) {
        if !self.breaking_block {
            return;
        }
        if let (Some(pos), Some(tx)) = (self.breaking_pos, &self.outgoing_tx) {
            self.dig_packet_this_tick = true;
            // Direction.DOWN und ohne Sequenz, genau wie Vanilla.
            let _ = tx.send(protocol::packets::player_action_block_payload(
                protocol::packets::PLAYER_ACTION_ABORT_DESTROY_BLOCK,
                pos.0,
                pos.1,
                pos.2,
                0,
                0,
            ));
        }
        self.breaking_block = false;
        self.breaking_progress = 0.0;
        // player.resetTicksSince()
        self.ticks_since_last_attack = 0;
    }

    // Vanillas isCurrentlyBreaking: gleiche Stelle UND dasselbe Item in der Hand
    // (ItemStack.areItemsAndComponentsEqual -- die Anzahl zaehlt nicht).
    pub(crate) fn is_currently_breaking(&self, pos: (i32, i32, i32)) -> bool {
        if self.breaking_pos != Some(pos) {
            return false;
        }
        let jetzt = self.own_hand_stack(protocol::packets::HAND_MAIN);
        match (jetzt, self.breaking_selected.as_ref()) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.item_id == b.item_id
                    && a.damage == b.damage
                    && a.max_damage == b.max_damage
                    && a.custom_name == b.custom_name
                    && a.enchantments == b.enchantments
                    && a.stored_enchantments == b.stored_enchantments
                    && a.glint_override == b.glint_override
                    && a.removed_components == b.removed_components
                    && a.use_cooldown == b.use_cooldown
                    && a.tool == b.tool
                    && a.can_place_on == b.can_place_on
                    && a.can_break == b.can_break
            }
            _ => false,
        }
    }

    // Player.blockActionRestricted: Zuschauer nie, Abenteuer nur mit einem Haupthand-Item,
    // dessen can_break den Block nennt.
    pub(crate) fn block_breaking_restricted(&self, pos: (i32, i32, i32)) -> bool {
        match self.game_mode {
            GAME_MODE_SPECTATOR => true,
            GAME_MODE_ADVENTURE => {
                let state = self.world.block_at(pos.0, pos.1, pos.2);
                let preds = self.own_hand_stack(protocol::packets::HAND_MAIN).and_then(|s| s.can_break.as_deref());
                !world::item_behavior::adventure_allows(preds, state)
            }
            _ => false,
        }
    }

    pub(crate) fn next_block_sequence(&mut self) -> i32 {
        self.block_sequence = self.block_sequence.wrapping_add(1);
        self.block_sequence
    }

    pub(crate) fn send_destroy_action(&mut self, action: i32, pos: (i32, i32, i32), face: i32, sequence: i32) {
        interaktions_log(match action {
            protocol::packets::PLAYER_ACTION_START_DESTROY_BLOCK => "start_destroy_block",
            protocol::packets::PLAYER_ACTION_STOP_DESTROY_BLOCK => "stop_destroy_block",
            _ => "destroy_block",
        });
        if let Some(tx) = &self.outgoing_tx {
            self.dig_packet_this_tick = true;
            let _ = tx.send(protocol::packets::player_action_block_payload(action, pos.0, pos.1, pos.2, face, sequence));
        }
    }

    // Das Werkzeug der Haupthand: gesetztes tool-Component, entferntes, oder der Item-Default.
    pub(crate) fn main_hand_tool(&self) -> Option<world::mining::Tool> {
        let stack = self.own_hand_stack(protocol::packets::HAND_MAIN)?;
        if let Some(t) = &stack.tool {
            return Some(world::mining::Tool {
                rules: t
                    .rules
                    .iter()
                    .map(|r| world::mining::ToolRule {
                        blocks: match &r.tag {
                            Some(tag) => world::mining::BlockSet::Tag(tag.clone()),
                            None => world::mining::BlockSet::Ids(r.ids.iter().map(|&i| i as u32).collect()),
                        },
                        speed: r.speed,
                        correct_for_drops: r.correct_for_drops,
                    })
                    .collect(),
                default_speed: t.default_speed,
                can_destroy_blocks_in_creative: t.can_destroy_blocks_in_creative,
            });
        }
        if stack.removed_components.contains(&protocol::item::COMPONENT_TOOL) {
            return None;
        }
        world::mining::default_tool(world::item_registry::item_name(stack.item_id)?).cloned()
    }

    // BlockState.calcBlockBreakingDelta fuer den eigenen Spieler.
    pub(crate) fn breaking_delta(&self, state: u32) -> f32 {
        let effekt = |id: i32| self.own_effects.get(&id).copied();
        // StatusEffectUtil.getHasteAmplifier: die hoehere Stufe aus Eile und Conduit-Kraft.
        let haste = match (effekt(EFFECT_HASTE), effekt(EFFECT_CONDUIT_POWER)) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        // Entity.updateSubmergedInWaterState: Augenhoehe minus 1/9 liegt unter der
        // Wasseroberflaeche der Zelle.
        let auge = self.physics.eye_pos().y - 0.11111111;
        let (ex, ez) = (self.physics.eye_pos().x.floor() as i32, self.physics.eye_pos().z.floor() as i32);
        let ey = auge.floor() as i32;
        let unter_wasser = world::fluid_kind(self.world.block_at(ex, ey, ez)) == Some(world::FluidKind::Water)
            && world::fluid_height_at(&self.world, ex, ey, ez).is_some_and(|h| auge < ey as f64 + h);
        let input = world::mining::BreakSpeedInput {
            mining_efficiency: self.attr_mining_efficiency,
            block_break_speed: self.attr_block_break_speed,
            submerged_mining_speed: self.attr_submerged_mining_speed,
            haste,
            mining_fatigue: effekt(EFFECT_MINING_FATIGUE),
            submerged_in_water: unter_wasser,
            on_ground: self.physics.on_ground,
        };
        world::mining::breaking_delta(state, self.main_hand_tool().as_ref(), &input)
    }

    // ClientPlayerInteractionManager.breakBlock: lokal abbauen, zurueckgenommen ueber die
    // Sequenz (derselbe Weg wie eine Platzierung, s. push_pending_block). Uebrig bleibt die
    // Fluessigkeit des Blocks -- ein wassergefuellter Block wird zu Wasser, alles andere zu Luft.
    pub(crate) fn break_block_client(&mut self, pos: (i32, i32, i32), sequence: i32) {
        if self.block_breaking_restricted(pos) {
            return;
        }
        let state = self.world.block_at(pos.0, pos.1, pos.2);
        if state == world::STATE_AIR {
            return;
        }
        // ItemStack.canMine: im Kreativmodus bauen Werkzeuge ohne can_destroy_blocks_in_creative
        // (Schwerter, Dreizack, Streitkolben) nichts ab.
        if self.game_mode == GAME_MODE_CREATIVE && self.main_hand_tool().is_some_and(|t| !t.can_destroy_blocks_in_creative) {
            return;
        }
        // OperatorBlock: nur fuer Operatoren -- das Recht kennen wir nicht, der Server entscheidet.
        let name = world::block_name(state).unwrap_or("");
        if matches!(
            name,
            "command_block" | "chain_command_block" | "repeating_command_block" | "structure_block" | "jigsaw" | "test_block" | "test_instance_block"
        ) {
            return;
        }
        // Block.onBreak -> spawnBreakParticles: der Server schickt das Level Event 2001 an alle
        // AUSSER den Abbauenden, der eigene Client zeichnet den Staub selbst -- und zwar noch
        // aus dem alten Blockzustand, also vor dem Ueberschreiben.
        self.with_particles(|p, ctx, _| p.add_block_break(pos, state, ctx));
        // Dasselbe Level Event spielt auch den Abbauklang -- fuer den Abbauenden also ebenfalls
        // nur aus dem eigenen Client.
        self.play_block_break_sound(pos, state);
        // Vanillas MultiPlayerGameMode.destroyBlock:
        //     FluidState fluidState = level.getFluidState(blockPos);
        //     level.setBlock(blockPos, fluidState.createLegacyBlock(), 11);
        // Uebrig bleibt also die FLUESSIGKEIT des Blocks, nicht "waterlogged ja/nein".
        //
        // ECHTER BUG gewesen (Grim "Simulation" beim Schwimmen und Abbauen von Seegras):
        // hier zaehlte nur die Property waterlogged=true. Seegras, hohes Seegras und Seetang
        // haben diese Property GAR NICHT -- es gibt von ihnen keine trockene Variante, ihr
        // FluidState ist immer eine volle Wasserquelle (s. world::fluid::always_wet). Wir
        // machten aus abgebautem Seegras also LUFT: eine Luftblase mitten im Wasser, die es
        // auf dem Server nicht gibt. Ab da liefen Stroemungsschub, Auftrieb und Widerstand
        // gegen eine andere Welt als Grims Vorhersage.
        //
        // fluid_kind ist dieselbe Quelle, mit der auch die Schwimmphysik fragt -- Abbau und
        // Physik koennen sich dadurch nicht mehr uneinig sein, was nass ist.
        let rest = match world::fluid_kind(state) {
            Some(world::FluidKind::Water) => block_place::fluid_source_state("water").unwrap_or(world::STATE_AIR),
            Some(world::FluidKind::Lava) => block_place::fluid_source_state("lava").unwrap_or(world::STATE_AIR),
            None => world::STATE_AIR,
        };
        // In die ECHTEN Chunkdaten, wie beim Umklappen einer Falltuer (s.
        // predict_openable_toggle): nur so verschwindet der Block sofort auch aus dem Mesh. Die
        // Vorhersage-Schicht ist nur fuer Geisterbloecke da, die GEZEICHNET werden -- ein
        // abgebauter Block ist Luft und haette dort nichts zu zeichnen.
        self.world.set_block(pos.0, pos.1, pos.2, rest);
        self.push_pending_block(pos, sequence, state);
        if let Some(p) = self.pending_blocks.iter_mut().find(|p| p.pos == pos) {
            p.entfernt.get_or_insert(state);
        }
    }

    // Ein Rechtsklick vormerken. Der Deckel haelt die Schlange kurz: sie soll ein paar sehr
    // schnelle Klicks auffangen, nicht einen Autoclicker puffern -- laege sie hoeher, wuerde
    // der Client nach einer Klickorgie noch sekundenlang von selbst weiterplatzieren.
    pub(crate) const USE_QUEUE_MAX: u32 = 4;
    // Hoechstens so lange warten Klicks auf das Verschwinden eines gezuendeten Ankers (s.
    // tick_item_use) -- falls der Server ihn doch stehen laesst (geschuetzte Region, eigene
    // Dimension mit Anker-Spawn), gehen sie danach ganz normal raus.
    pub(crate) const ANCHOR_WAIT_MAX_TICKS: u32 = 20;
    // Ebenso fuer das Warten auf die Bestaetigung eines ausgeleerten Eimers (s. bucket_ack_wait).
    pub(crate) const BUCKET_ACK_WAIT_MAX_TICKS: u32 = 20;

    pub(crate) fn queue_use_click(&mut self) {
        if self.mode != AppMode::World || self.screen_overlay_open() || self.chat_input.is_some() {
            return;
        }
        if self.slot_queue.add_use(Self::USE_QUEUE_MAX) {
            return;
        }
        self.use_queue = (self.use_queue + 1).min(Self::USE_QUEUE_MAX);
    }

    // Einmal pro Tick, VOR dem Bewegungspaket -- dieselbe Stelle, an der Vanilla seine
    // Eingaben abarbeitet (s. flush_input_actions). Genau ein Benutzen pro Tick:
    //
    //   1. Steht etwas in der Warteschlange, gilt das -- unabhaengig vom Cooldown. Auch
    //      Vanilla bremst einzelne Klicks nicht (doItemUse prueft itemUseCooldown nicht).
    //   2. Sonst feuert eine GEHALTENE Taste, sobald der Cooldown abgelaufen ist. Das ist
    //      Vanillas zweiter Zweig (useKey.isPressed() && itemUseCooldown == 0).
    pub(crate) fn tick_item_use(&mut self) {
        // Vanillas handleInputEvents ist an dieser Stelle ein ENTWEDER-ODER, kein Nacheinander:
        // benutzt der Spieler gerade ein Item, wird beim Loslassen der Benutzen-Taste die
        // Benutzung beendet, und alle gepufferten Angriffs-, Benutzen- und Pick-Klicks werden
        // verworfen. Sonst werden erst alle Angriffe, dann alle Benutzungen abgearbeitet.
        //
        // Waehrend einer laufenden Benutzung werden gepufferte Klicks also VERWORFEN, nicht
        // aufgehoben. Genau daran haengt GrimACs PacketOrderI (grim.packetorder.input_tick_order):
        // sein PacketOrderProcessor merkt sich pro Tick, was reinkam, und flaggt ein USE_ITEM/
        // BLOCK_PLACEMENT, das im selben Tick NACH einem RELEASE_USE_ITEM ankommt
        // ("type=place/use, releasing=true"). Vanilla kann das nicht erzeugen -- wir schon,
        // solange erst losgelassen und dann noch ein Klick aus der Schlange gefeuert wurde.
        //
        // Das RELEASE haengt an `using_item`, also an Vanillas isUsingItem() -- und zwar genau
        // daran, nicht an "irgendwann mal geklickt".
        //
        // Hier stand vorher `use_started` mit der Begruendung, `using_item` sei eine Teilmenge
        // dessen, wofuer Grim `slowedByUsingItem` setzt -- "lieber ein RELEASE zu viel". Diese
        // Annahme ist FALSCH, nachgelesen in GrimACs eigenem Jar: PacketPlayerDigging.
        // handleUseItem setzt das Flag nur, wenn ItemBehaviour.canUse(...) true meldet, und das
        // ist es ausschliesslich fuer Items mit CONSUMABLE-Komponente (Essen/Trank, consume
        // seconds > 0) oder BLOCKS_ATTACKS (Schild) -- also genau fuer das, was auch unser
        // is_used_continuously abdeckt. Fuer eine Enderperle setzt Grim das Flag gar nicht erst.
        //
        // Der "Sicherheitsabstand" kostete also nichts, brachte aber ein Paket ein, das ein
        // Vanilla-Client NIE schickt: nach jedem Perlenwurf, jedem Blocksetzen, jedem
        // Kristall ging ein RELEASE_USE_ITEM raus. Unnoetige Pakete sind genau das, was
        // Grims PacketOrder-Pruefungen auswerten.
        let was_using = self.using_item;
        // Vanilla gibt die Benutzung frei, wenn isUsingItem() gilt und die Taste oben ist --
        // ausgeloest von isUsingItem(), NICHT davon, ob wir selbst die Benutzung begonnen haben.
        // Seit die Metadaten eine Benutzung auch STARTEN koennen (s. EntityMeta), kann using_item
        // gelten, ohne dass use_started gesetzt ist; genau dann muss beim Loslassen trotzdem das
        // RELEASE raus, sonst bleibt der Schild serverseitig oben.
        if !self.use_held {
            self.use_started = false;
        }
        // Consumable-Optimizer "Durchdruecken": knapp vor dem Ende losgelassen -- weiter
        // benutzen, als waere die Taste noch unten. Fuer den Server ist das nur ein etwas
        // laengerer Tastendruck.
        let durchhalten = !self.use_held && self.using_item && self.eat_hold_through();
        if durchhalten {
            self.eat_held_through = true;
        }
        if !self.use_held && self.using_item && !durchhalten {
            // Fuer den Dreizack unten: welcher Stack gerade benutzt wurde, und wie lange.
            let benutzte_hand = if self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
            let benutzt = self.own_hand_stack(benutzte_hand).cloned();
            let benutzt_ticks = self.own_use_ticks;
            self.using_item = false;
            // Neue bzw. beendete Benutzung: der Zaehler der Ess-/Spannhaltung faengt neu an. Sonst
            // erbte ein Bissen, der im selben Tick auf den vorigen folgt, dessen Stand (der
            // Apfel sprang fuer ein Bild zurueck und stand danach sofort am Mund).
            self.own_use_ticks = 0;
            if let Some(tx) = &self.outgoing_tx {
                // ECHTER BUG gewesen, und zwar von mir mit dem Rechtsklick eingeschleppt:
                // Vanilla beendet die Benutzung beim Loslassen der Taste
                // (interactionManager.stopUsingItem -> PlayerAction RELEASE_USE_ITEM). Wir
                // schickten das nie. GrimACs PacketPlayerDigging setzt `slowedByUsingItem`
                // beim USE_ITEM und raeumt es NUR bei RELEASE_USE_ITEM (oder Slotwechsel)
                // wieder weg -- fuer Grim benutzten wir also dauerhaft ein Item und
                // bewegten uns mit dem Item-Faktor 0.2, waehrend wir selbst mit dem
                // Sneak-Faktor 0.3 rechneten (0.03823018 gegen 0.02549019 = 1.4998).
                //
                // Ohne Sequenznummer und ohne den Zaehler weiterzudrehen -- Vanillas
                // stopUsingItem schickt das Paket mit Sequenz 0 (s. player_action_payload).
                interaktions_log("release_use_item");
                let _ = tx.send(protocol::packets::player_action_payload(protocol::packets::PLAYER_ACTION_RELEASE_USE_ITEM));
            }
            // Danach player.stopUsingItem() -> TridentItem.onStoppedUsing, im Client-Zweig: ab
            // 10 Ticks Benutzung und mit Sog (im Wasser oder Regen) schiesst man sich selbst
            // los. Der Server bewegt uns dabei NICHT -- ohne das hier passierte beim Loslassen
            // gar nichts, und GrimAC saehe eine Bewegung, die ausbleibt.
            if let Some(st) = benutzt {
                let staerke = self.trident_spin_strength(&st);
                if staerke > 0.0 && benutzt_ticks >= TRIDENT_MIN_USE_TICKS && !will_break_next_use(&st) && self.touching_water_or_rain() {
                    self.physics.start_riptide(self.camera.yaw, self.look_pitch_deg(), staerke, &self.world, &self.block_shapes);
                }
            }
        }
        let welt = self.mode == AppMode::World && !self.screen_overlay_open() && self.chat_input.is_none();
        if !welt {
            // Bildschirm offen: Schlange leeren, aber nichts senden -- ein Klick im Inventar
            // darf nicht spaeter in der Welt nachfeuern.
            self.use_queue = 0;
            self.use_cooldown = self.use_cooldown.saturating_sub(1);
            return;
        }
        // Vanillas `if (player.isUsingItem())`-Zweig: waehrend einer laufenden DAUER-Benutzung
        // (Essen, Bogen, Schild) werden gepufferte Klicks verworfen statt abgearbeitet.
        //
        // ECHTER BUG GEWESEN: hier stand `if released || was_using`, das Verwerfen galt also
        // auch fuer den blossen Tick des LOSLASSENS. Vanilla kennt das nicht -- sein
        // Verwerfen haengt einzig an isUsingItem(). Ein Doppelklick, dessen zweiter Klick noch
        // in der Schlange stand, wenn die Taste hochging, wurde bei uns weggeworfen.
        if was_using {
            self.use_queue = 0;
            self.pending_input_actions.retain(|a| !matches!(a, PendingInputAction::Attack));
            self.use_cooldown = self.use_cooldown.saturating_sub(1);
            return;
        }

        // EIN Strahl pro Tick, den sich ALLE Klicks dieses Ticks teilen -- Vanillas
        // gameRenderer.pick laeuft einmal in tick(), und handleKeybinds arbeitet
        // danach saemtliche gepufferten Klicks gegen dieses EINE hitResult ab.
        //
        // Das ist der Grund, warum der Anchor-Airplace in Vanilla funktioniert: zwei Klicks im
        // selben Tick tragen dieselbe Zielposition. Der erste zuendet den geladenen Anchor
        // (der Server sprengt den Block weg), der zweite trifft beim Server dieselbe, jetzt
        // LEERE Stelle -- Luft ist ersetzbar, der Anker wird genau dort gesetzt. Wer den
        // Strahl fuer den zweiten Klick neu schiesst, zielt ins Leere dahinter.
        let treffer = self.pick_block();
        // Liegt ein Entity VOR dem Block (Vanillas crosshairTarget, s. crosshair_target), geht
        // der Rechtsklick an das Entity, nicht an den Block dahinter. Sonst liessen sich
        // durch End-Kristalle hindurch Bloecke setzen.
        let entity_ziel = self.pick_use_entity(treffer);
        let treffer = if entity_ziel.is_some() { None } else { treffer };
        if self.use_queue > 0 || self.use_held {
            match treffer {
                Some(hit) => interaktions_log_pos("pick", hit.block),
                None => interaktions_log("pick(-)"),
            }
        }

        // Vanillas else-Zweig: ERST alle Angriffsklicks, DANN die Benutzungen --
        //     while (keyAttack.consumeClick()) bl3 |= startAttack();
        //     while (keyUse.consumeClick())    startUseItem();
        //
        // ECHTER BUG gewesen (Grim "MultiActionsE" beim schnellen Schild-Spammen
        // mit Schlaegen in die Luft): hier liefen die Benutzungen VOR den Angriffen. Grims
        // PacketPlayerDigging.handleUseItem setzt slowedByUsingItem beim Eintreffen des
        // USE_ITEM SOFORT, und MultiActionsE flaggt jedes ANIMATION-Paket (Armschwung), das
        // danach kommt, solange das gilt. Unser Schwung landete dadurch im selben Tick HINTER
        // dem Schild-USE_ITEM -- fuer Grim ein Schlag mit erhobenem Schild, den Vanilla nicht
        // erzeugen kann. Bei Vanilla steht der Schwung vorn, noch ohne Schild.
        //
        // Der Strahl oben steht bewusst VOR den Angriffen: Vanillas hitResult wird einmal pro
        // Tick berechnet (gameRenderer.pick) und von Angriff und Benutzen gemeinsam genutzt. Ein
        // Angriff, der einen Block sofort abbaut, darf den Strahl der Benutzung nicht umlenken.
        // Zurueckgestellter Tausch/Wegwerfen (s. flush_input_actions) bleibt liegen.
        for action in std::mem::take(&mut self.pending_input_actions) {
            match action {
                PendingInputAction::Attack => self.send_attack(),
                _ => self.pending_input_actions.push(action),
            }
        }

        // Vanillas `while (useKey.wasPressed()) doItemUse();` -- ALLE gemerkten Klicks in
        // DIESEM Tick, nicht einer pro Tick. Auch das braucht der Airplace.
        // Klicks nach einem gezuendeten Anker warten, bis der Anker lokal verschwunden ist
        // (s. unten beim Zuenden). Angriffe oben laufen normal weiter.
        if let Some((pos, getickt)) = self.anchor_wait {
            let steht = {
                let state = self.world.block_at(pos.0, pos.1, pos.2);
                world::block_name(state) == Some("respawn_anchor")
                    && world::state_props::property(state, "charges").is_some_and(|c| c != "0")
            };
            if steht && getickt < Self::ANCHOR_WAIT_MAX_TICKS {
                self.anchor_wait = Some((pos, getickt + 1));
                self.use_cooldown = self.use_cooldown.saturating_sub(1);
                return;
            }
            self.anchor_wait = None;
        }
        // No Extinguish DC Ghost: brennt man beim Ausleeren eines Wasser- oder Pulverschnee-Eimers,
        // warten die Klicks auf demselben Slot, bis der Server das Ausleeren bestaetigt hat.
        //
        // Warum: der Server setzt das Wasser schon beim USE_ITEM, geloescht wird das Feuer aber
        // erst im Spieler-Tick (applyEffectsFromBlocks -> WaterFluid.entityInside). Kommen
        // Ausleeren und Aufnehmen im selben Server-Tick an -- auch auf zwei Client-Ticks
        // verteilt, wenn das Netz sie buendelt --, ist das Wasser vor diesem Tick schon wieder
        // weg. Was erst nach der Bestaetigung (BlockChangedAck) rausgeht, landet sicher in einem
        // spaeteren Server-Tick als das Ausleeren.
        if let Some((seq, slot, getickt)) = self.bucket_ack_wait {
            if slot == self.inventory.held_slot && getickt < Self::BUCKET_ACK_WAIT_MAX_TICKS {
                self.bucket_ack_wait = Some((seq, slot, getickt + 1));
                self.use_cooldown = self.use_cooldown.saturating_sub(1);
                return;
            }
            self.bucket_ack_wait = None;
        }
        // Consumable Optimizer: lokal fertig gegessen, der Server aber noch nicht. Ein neues
        // USE_ITEM, das vor seinem letzten Ess-Tick ankommt, setzt dort das Essen NEU an -- der
        // Bissen ist verloren (gegen Paper gemessen, s. net/tests/eat_real.rs: zweites USE 31
        // Ticks nach dem ersten = nichts gegessen, nach 32 = gegessen). Lokal sind wir nach 31
        // Ticks fertig; ein schneller neuer Rechtsklick oder die Wiederholung bei gehaltener
        // Taste traf genau diese Luecke: sichtbar fertig gegessen,
        // serverseitig aber nicht.
        //
        // Auf die Bestaetigung des Servers zu warten, kostete eine Rundreise. Noetig ist sie
        // nicht: beide USE_ITEM laufen ueber dieselbe Verbindung, ihr ABSTAND kommt beim Server
        // also unabhaengig vom Ping an. Es reicht, die Klicks im Tick direkt nach dem lokalen
        // Ende zurueckzuhalten: der naechste Bissen geht dann 33 Ticks nach dem vorigen raus,
        // einen mehr als noetig -- Puffer gegen Schwankungen im Netz. Nur solange noch etwas
        // Essbares in der Hand ist.
        // Meldet der Server vorher "fertig", faellt die Sperre sofort weg (s. EntityMeta).
        if self.eat_next_use_delay > 0 {
            self.eat_next_use_delay -= 1;
            if self.consumable_in_hand() {
                self.use_cooldown = self.use_cooldown.saturating_sub(1);
                return;
            }
        }
        // Mit "Benutzen beim Abbauen" doch noch ein PacketOrderI: Grims `digging` setzt JEDES
        // Start-, Ende- und Abbruch-Paket, geraeumt wird es erst vom naechsten Tick-Paket. Ein
        // Rechtsklick nach einem Linksklick im selben Tick (oder ein neuer Abbau-Start, weil
        // der gerade gesetzte Block das Ziel wechselt) flaggt also. Dann warten die Klicks wie
        // oben auf den naechsten Tick -- verworfen wird keiner.
        if self.dig_packet_this_tick {
            self.use_cooldown = self.use_cooldown.saturating_sub(1);
            return;
        }
        let modus = self.settings.hold_use_repeat;
        // Ob die Benutzungen dieses Ticks gemerkte Klicks sind oder die Wiederholung des
        // Haltens, entscheidet allein, ob die Schlange leer war (s. item_uses_this_tick).
        let wiederholung = self.use_queue == 0;
        // Mit einer Rakete in einer Hand entscheidet der Raketen-Schalter statt der Stufe.
        let rakete = [protocol::packets::HAND_MAIN, protocol::packets::HAND_OFF]
            .iter()
            .any(|&h| self.own_hand_stack(h).and_then(|s| world::item_registry::item_name(s.item_id)) == Some("firework_rocket"));
        let wiederholen = if rakete { self.settings.hold_use_repeat_rockets } else { modus != HoldUseRepeat::Never };
        let (anzahl, cooldown) = item_uses_this_tick(self.use_queue, self.use_cooldown, self.use_held, wiederholen);
        self.use_queue = 0;
        self.use_cooldown = cooldown;
        for i in 0..anzahl {
            self.use_started = true;
            self.use_placed_block = false;
            self.use_detonated_anchor = None;
            self.use_emptied_bucket = None;
            self.do_item_use(wiederholung && modus == HoldUseRepeat::ExceptBlocks, treffer, entity_ziel);
            // ABWEICHUNG VON VANILLA, bewusst: hat dieser Klick einen Block gesetzt, warten die
            // uebrigen Klicks des Ticks auf den naechsten. Vanilla arbeitet sie alle gegen DEN
            // EINEN Strahl dieses Ticks ab -- der zweite zielt dann auf die Stelle, an der jetzt
            // schon der neue Block steht, und schlaegt fehl. Frueher setzte ein Doppelklick
            // oft nur EINEN Block, naemlich immer dann, wenn beide Klicks in dieselben 50 ms
            // fielen (im Interaktions-Log: zweimal use_item_on auf dieselbe Position).
            //
            // Den Strahl je Klick neu zu schiessen waere die falsche Loesung: zwei verschiedene
            // Ziele im selben Tick kann Vanilla nicht erzeugen, das faellt einem Anticheat auf.
            // Ein Klick im naechsten Tick ist dagegen genau das, was ein Spieler erzeugt, dessen
            // Klicks zufaellig auf zwei Ticks fielen. Nur nach einem GESETZTEN Block -- der
            // Anchor-Airplace braucht zwei Klicks im selben Tick (s. oben), und dort setzt der
            // erste Klick keinen Block, er zuendet den Anker.
            let rest = anzahl - i - 1;
            if self.use_placed_block && rest > 0 && !wiederholung {
                self.use_queue = rest.min(Self::USE_QUEUE_MAX);
                break;
            }
            // Zweite Erweiterung der Schlange: hat dieser Klick einen geladenen Anker
            // gezuendet, und haelt man KEINEN Block (Perle, Kristall, Schwert...), warten alle
            // weiteren Klicks, bis der Anker lokal weg ist. Sonst sprengt ein Doppelklick mit der
            // Enderperle auf einen geladenen Anker ihn, wirft aber nie die Perle.
            //
            // Warum: der Client weiss nicht selbst, dass der Anker explodiert (das entscheidet
            // der Server), fuer ihn steht der Anker bis zum Block-Update weiter da. Jeder
            // weitere Klick in dieser Zeit -- im selben Tick wie in den naechsten, bis eine
            // Rundreise vorbei ist -- trifft also wieder den Anker, wird als Anker-Benutzung
            // angenommen und wirft nichts. Erst wenn der Anker weg ist, geht der Strahl ins
            // Leere und der Klick wird zum USE_ITEM, also zum Wurf.
            //
            // Mit einem Block in der Hand gilt das NICHT: der Anchor-Airplace braucht gerade
            // den zweiten Klick im selben Tick auf der Anker-Stelle (s. oben).
            // Ebenso nach einem ausgeleerten Wasser-/Pulverschnee-Eimer: die uebrigen Klicks warten
            // auf die Bestaetigung (s. oben bei bucket_ack_wait).
            if let Some(seq) = self.use_emptied_bucket.take() {
                if self.settings.no_extinguish_dc_ghost && self.own_on_fire {
                    self.bucket_ack_wait = Some((seq, self.inventory.held_slot, 0));
                    if !wiederholung {
                        self.use_queue = rest.min(Self::USE_QUEUE_MAX);
                    }
                    break;
                }
            }
            if let Some(pos) = self.use_detonated_anchor.take() {
                let haelt_block = self.main_hand_item_name().is_some_and(world::block_registry::is_block);
                if !wiederholung && !haelt_block {
                    self.anchor_wait = Some((pos, 0));
                    self.use_queue = rest.min(Self::USE_QUEUE_MAX);
                    break;
                }
            }
        }
    }

    // Vanillas gameRenderer.pick, auf den Blockstrahl reduziert. Eigene Funktion, damit
    // pro Tick genau EIN Strahl entsteht, den sich alle Klicks teilen (s. tick_item_use).
    pub(crate) fn pick_block(&self) -> Option<physics::BlockRayHit> {
        // Ursprung ist die HARTE Augenhoehe der Pose, nicht die geglaettete Kamerahoehe --
        // aus demselben Grund wie beim Angriff (s. send_attack).
        let eye = self.physics.eye_pos();
        let dir = self.camera.forward();
        let dir = DVec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
        physics::raycast_block(eye, dir, self.attr_block_range, &self.world, &self.block_shapes)
    }

    // Das Entity, das der Rechtsklick trifft, falls es naeher liegt als der Blocktreffer
    // `block` -- dieselbe Regel wie crosshair_target (Reichweite 3, ein Block davor faengt
    // ab). Mit der Trefferstelle relativ zur Entity-Position, wie INTERACT_AT sie braucht
    // (Vanillas `hitResult.getPos().subtract(entity.getPos())`).
    pub(crate) fn pick_use_entity(&self, block: Option<physics::BlockRayHit>) -> Option<(i32, (f32, f32, f32))> {
        let entity_reach = self.attr_entity_range;
        let eye = self.physics.eye_pos();
        let dir = self.camera.forward();
        let dir = DVec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
        // Mit attack_range teilen sich Angriff und Rechtsklick dasselbe Ziel (Vanilla hat nur
        // ein hitResult). Die Trefferstelle liegt dann auf dem Blickstrahl im Trefferabstand.
        match self.attack_range_target(eye, dir) {
            Some((CrosshairTarget::Entity(id), d)) => {
                let e = self.entities.get(id)?;
                let punkt = eye + dir * d;
                let (x, y, z) = e.hit_pos();
                return Some((id, ((punkt.x - x) as f32, (punkt.y - y) as f32, (punkt.z - z) as f32)));
            }
            Some(_) => return None,
            None => {}
        }
        let block_dist = block.map(|hit| {
            DVec3::new(
                hit.block.0 as f64 + hit.cursor.0 as f64,
                hit.block.1 as f64 + hit.cursor.1 as f64,
                hit.block.2 as f64 + hit.cursor.2 as f64,
            )
            .distance(eye)
        });
        let reach = block_dist.map_or(entity_reach, |d| d.min(entity_reach));
        let (id, t) = self.entities.pick_hit((eye.x, eye.y, eye.z), (dir.x, dir.y, dir.z), reach)?;
        let e = self.entities.get(id)?;
        let punkt = eye + dir * t;
        let (x, y, z) = e.hit_pos();
        Some((id, ((punkt.x - x) as f32, (punkt.y - y) as f32, (punkt.z - z) as f32)))
    }

    // Vanillas MinecraftClient.doItemUse(), auf das reduziert, was ueber die Leitung geht:
    // Strahl gegen die Bloecke, dann entweder USE_ITEM_ON (Treffer) oder USE_ITEM (ins Leere).
    //
    // BEWUSST NUR DIE HAUPTHAND: Vanilla probiert beide Haende der Reihe nach und nimmt die
    // zweite erst, wenn die erste PASS meldet -- was der Client aber gar nicht selbst
    // entscheidet, sondern vom Server erfaehrt. Ohne diese Antwort waere jede Zweithand-
    // Heuristik geraten; die Zweithand kommt, wenn der Block-Changed-Ack ausgewertet wird.
    // Ein frisch gespawntes Geschoss optisch an die Hand seines Schuetzen ruecken.
    //
    // Das Data-Feld des Spawn-Pakets traegt bei Projektilen die Entity-ID des Schuetzen
    // (Projectile.getAddEntityPacket). Damit findet man seine Blickrichtung und seine
    // Ausruestung -- und daraus, aus welcher Hand und mit welcher Waffe geschossen wurde.
    // Der Versatz selbst ist reine Darstellung, s. world::entity::launch_render_offset.
    pub(crate) fn set_projectile_launch_offset(&mut self, spawn: &protocol::packets::AddEntity) {
        use world::entity::{launch_hand_offset, shooting_hand};
        // Windkugeln nicht: WindChargeItem erzeugt sie in Augenhoehe mitten im Spieler, nicht an
        // der Hand -- und die der Breeze haben ohnehin keine.
        if !matches!(world::entity::projectile_kind(spawn.entity_type), Some(k) if k != world::entity::ProjectileKind::WindCharge) || spawn.data == 0 {
            return;
        }
        // Der eigene Schuss: wir stehen nicht in der Entity-Tabelle, unsere Blickrichtung
        // und unsere Haende kommen aus der Physik bzw. dem Inventar.
        let (yaw, pitch, hand) = if Some(spawn.data) == self.own_entity_id {
            let haupt = self.hand_stack(inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8));
            let zweit = self.hand_stack(inventory::PLAYER_OFFHAND);
            (self.camera.yaw, self.look_pitch_deg(), shooting_hand(haupt.as_ref(), zweit.as_ref()))
        } else {
            let Some(schuetze) = self.entities.get(spawn.data) else { return };
            // Der Versatz meint die SEITE: bei Linkshaendern ist die Haupthand links.
            let (nebenhand, waffe) = shooting_hand(schuetze.main_hand.as_ref(), schuetze.off_hand.as_ref());
            (schuetze.yaw_deg, schuetze.pitch_deg, (nebenhand != schuetze.left_handed, waffe))
        };
        let (offhand, waffe) = hand;
        self.entities.set_launch_offset(spawn.entity_id, launch_hand_offset(yaw, pitch, offhand, waffe));
    }

    // Was in einem eigenen Hand-Slot liegt, in der Form, die world::entity erwartet.
    pub(crate) fn hand_stack(&self, slot: usize) -> Option<world::entity::HeldItem> {
        let stack = self.inventory.player.get(slot)?.as_ref()?;
        Some(world::entity::HeldItem::from_stack(stack))
    }

    // Boxen aller anderen Entities fuer physics::tick_riptide (Vanillas getOtherEntities): wir
    // selbst stehen nicht in der Tabelle, muessen also nicht herausgefiltert werden.
    pub(crate) fn riptide_entity_boxes(&self) -> Vec<(DVec3, DVec3, bool)> {
        self.entities
            .iter()
            .map(|(_, e)| {
                let (width, height) = e.hit_size();
                let (x, y, z) = e.hit_pos();
                let half = width / 2.0;
                (DVec3::new(x - half, y, z - half), DVec3::new(x + half, y + height, z + half), world::is_living(e.entity_type))
            })
            .collect()
    }

    // Entity.isTouchingWaterOrRain: Wasserkontakt ODER Regen am Block der Fuesse bzw. auf Hoehe
    // der Box-Oberkante (isBeingRainedOn prueft beide, s. world::weather::has_rain).
    pub(crate) fn touching_water_or_rain(&self) -> bool {
        if self.physics.touching_water {
            return true;
        }
        let p = self.physics.pos;
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        let oben = (p.y + self.physics.current_height()).floor() as i32;
        let klima = |biom: u32| self.biome_climate.get(biom as usize).copied();
        [p.y.floor() as i32, oben]
            .into_iter()
            .any(|y| world::weather::has_rain(&self.world, &self.block_shapes, self.rain_gradient, klima, x, y, z))
    }

    // Die Sog-Staerke eines Stacks, falls er ein Dreizack ist (s.
    // world::enchantment::Enchantments::trident_spin_strength) -- 0 = kein Sog.
    pub(crate) fn trident_spin_strength(&self, stack: &protocol::item::ItemStack) -> f32 {
        if world::item_registry::item_name(stack.item_id) != Some("trident") {
            return 0.0;
        }
        self.enchantments.trident_spin_strength(stack)
    }

    // Name des Items in der Haupthand -- fuer die Frage, ob eine Benutzung durchlaeuft.
    // Sticht die Haupthand statt zu schlagen (piercing_weapon)?
    pub(crate) fn main_hand_piercing(&self) -> bool {
        self.own_hand_stack(protocol::packets::HAND_MAIN).is_some_and(world::item_behavior::piercing)
    }

    pub(crate) fn main_hand_item_name(&self) -> Option<&'static str> {
        let slot = inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8);
        self.inventory.player.get(slot).and_then(|s| s.as_ref()).and_then(|stack| world::item_registry::item_name(stack.item_id))
    }

    // `ohne_bloecke`: die Wiederholung unter "Ausser Bloecke" (s. HoldUseRepeat) -- eine
    // Nebenhand mit einem Block faellt dann durch wie ein PASS, OHNE Paket (so wie no-hold-use
    // es in interactBlock/interactItem macht). Hat die HAUPThand den Block, entfaellt die
    // ganze Wiederholung (s. PacketOrderD in der Schleife).
    pub(crate) fn do_item_use(&mut self, ohne_bloecke: bool, treffer: Option<physics::BlockRayHit>, entity_ziel: Option<(i32, (f32, f32, f32))>) {
        if self.outgoing_tx.is_none() || self.conn_status != ConnStatus::Connected {
            return;
        }
        // MinecraftClient.startUseItem: `if (!player.isHandsBusy())`. Hat ein frueherer Klick
        // DIESES Ticks eine Dauerbenutzung begonnen, laeuft der naechste ins Leere -- er setzt
        // in Vanilla nur noch den Cooldown (den setzt unsere Schleife ohnehin).
        if self.using_item {
            return;
        }
        // MinecraftClient.doItemUse beginnt mit `if (!interactionManager.isBreakingBlock())` --
        // waehrend des Abbaus verschluckt Vanilla den Rechtsklick KOMPLETT, es geht kein Paket
        // raus und nicht einmal rightClickDelay wird gesetzt. Im Crystal-PvP ist genau das
        // teuer: ein verrutschter Linksklick sperrt das Setzen, bis man ihn wieder loslaesst.
        //
        // ECHTER FLAG GEWESEN (GrimAC PacketOrderI, "type=place/use, releasing=false,
        // digging=true", gemessen auf anticheat-test.com) -- aber NICHT wegen des
        // Nicht-Verschluckens. Hier stand ein `cancel_block_breaking()`: der Abbau wurde
        // ABGEBROCHEN und der Rechtsklick danach ausgefuehrt. Damit stehen ein
        // PlayerDigging(ABORT) und ein USE_ITEM(_ON) im SELBEN Tick im Strom, und genau das
        // kennt Grims PacketOrderProcessor als unmoeglich (sein `digging`-Flag wird erst vom
        // naechsten Tick-Paket geraeumt).
        //
        // Eine Fabric-Mod, die bloss die `isBreakingBlock()`-Sperre entfernt, bricht den Abbau
        // NICHT ab -- sie laesst ihn weiterlaufen und benutzt zusaetzlich. Deshalb flaggt sie
        // nicht, und deshalb tun wir es jetzt genauso: kein Abbruch, kein Zusatzpaket. Der
        // Abbau laeuft im selben Tick unten in handle_block_breaking() einfach weiter.
        //
        // Geht die Abbau-Taste in diesem Tick hoch, schickt handle_block_breaking() sein ABORT
        // NACH unserem Benutzen-Paket -- auch das ist harmlos: PacketOrderI prueft nur ein
        // Benutzen NACH einem Digging-Paket, nicht umgekehrt.
        if self.breaking_block && !self.settings.use_while_mining {
            return;
        }
        if self.game_mode == GAME_MODE_SPECTATOR {
            self.spectator_use(treffer, entity_ziel);
            return;
        }
        // BEIDE HAENDE, der Reihe nach -- Vanillas `for (Hand hand : Hand.values())`. Die
        // Schleife bricht ab, sobald eine Hand die Benutzung ANNIMMT; nur wenn sie durchfaellt,
        // kommt die naechste dran.
        //
        // GENAU DAS FEHLTE (das Schild liess sich nicht halten): frueher lief hier
        // nur die Haupthand, mit dem Vermerk, die Zweithand sei nicht entscheidbar. Das stimmt
        // so nicht -- Vanilla entscheidet es sehr wohl selbst, ueber das ActionResult, und
        // genau daran haengt jedes Schild: es liegt in der NEBENhand, und ohne diesen
        // Durchgang hat der Server nie ein USE_ITEM fuer sie gesehen. Man konnte also nicht
        // blocken.
        let (haupt, zweit) = self.own_hand_items();
        let haende = [
            (protocol::packets::HAND_MAIN, haupt.and_then(|i| world::item_registry::item_name(i.item_id))),
            (protocol::packets::HAND_OFF, zweit.and_then(|i| world::item_registry::item_name(i.item_id))),
        ];

        for (hand, item) in haende {
            if ohne_bloecke && item.is_some_and(world::block_registry::is_block) {
                // Die HAUPThand darf nicht still ausfallen, waehrend die Nebenhand weiter
                // Pakete schickt: Vanilla schickt fuer die Haupthand immer zuerst etwas.
                // ECHTER FLAG GEWESEN (Grim PacketOrderD "Skipped Mainhand", 4x je Sekunde =
                // jede Wiederholung): Block in der Hand, Rechtsklick auf ein Entity gehalten --
                // die Nebenhand schickte ihre Interact-Pakete allein. Faellt die Haupthand
                // durch, entfaellt deshalb die ganze Wiederholung.
                if hand == protocol::packets::HAND_MAIN {
                    return;
                }
                continue;
            }
            // Abklingzeit dieser Hand (s. item_cooldowns). Sie sperrt useOnBlock UND use, nicht
            // aber die Pakete: beide gehen trotzdem raus.
            let stack = self.own_hand_stack(hand).cloned();
            let abkuehlend = stack.as_ref().is_some_and(|st| self.item_cooldowns.is_cooling_down(st));
            // SCHRITT 1a -- case ENTITY: interactEntityAtLocation, dann interactEntity. Beide
            // Pakete gehen raus; ob das Entity annimmt, entscheidet der Server. Fuer End-
            // Kristalle (der Fall, um den es geht) ist beides PASS, also weiter zu Schritt 2
            // -- einen Block dahinter setzt Vanilla nie, der Strahl endet am Entity.
            if let Some((id, at)) = entity_ziel {
                interaktions_log("interact_entity");
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::interact_at_entity_payload(id, at, hand, self.sneak_held));
                    let _ = tx.send(protocol::packets::interact_entity_payload(id, hand, self.sneak_held));
                }
                // Einsteigen nimmt der Client selbst schon an (AbstractBoatEntity.interact,
                // MinecartEntity.interact geben clientseitig SUCCESS): Arm schwingen, fertig --
                // sonst ginge noch ein USE_ITEM fuer das Item in der Hand hinterher, und ein
                // Eimer schoepfte neben dem Boot Wasser.
                if self.click_boards_vehicle(id) {
                    self.swing_arm();
                    return;
                }
            }
            // SCHRITT 1 -- interactBlock. Nur bei einem Blocktreffer, und das Paket geht IMMER
            // raus (Vanilla verschickt es in sendSequencedPacket, bevor ueberhaupt feststeht,
            // was dabei herauskommt).
            if let Some(hit) = treffer {
                // ERST hochzaehlen, DANN lesen -- genau wie Vanillas sendSequencedPacket
                // (PendingUpdateManager.incrementSequence, dann getSequence). Die erste Nummer einer Welt ist
                // damit 1, nicht 0. GrimACs BadPacketsH erwartet lastSequence+1 und faengt bei
                // lastSequence=0 an -- eine 0 als erste Sequenz flaggte einmal pro Join
                // ("expected=1, id=0").
                self.block_sequence = self.block_sequence.wrapping_add(1);
                let sequence = self.block_sequence;
                interaktions_log_pos("use_item_on", hit.block);
                // Session-Info: Anker und Betten explodieren ausserhalb ihrer Dimension -- wer
                // dabei stirbt, zaehlt als eigener Kill (s. send_attack).
                let (bx, by, bz) = hit.block;
                if world::block_name(self.world.block_at(bx, by, bz)).is_some_and(|n| n == "respawn_anchor" || n.ends_with("_bed")) {
                    self.session_engage_near((bx as f64 + 0.5, by as f64 + 0.5, bz as f64 + 0.5), session_info::BLOCK_EXPLOSION_RADIUS);
                }
                if let Some(tx) = &self.outgoing_tx {
                    let _ = tx.send(protocol::packets::use_item_on_payload(
                        hand,
                        hit.block.0,
                        hit.block.1,
                        hit.block.2,
                        hit.face,
                        hit.cursor.0,
                        hit.cursor.1,
                        hit.cursor.2,
                        hit.inside,
                        sequence,
                    ));
                }
                match self.resolve_block_use(hit.block, hit.face, hit.cursor.1, sequence, item, abkuehlend, hand) {
                    BlockUse::Angenommen { schwingt } => {
                        if schwingt {
                            self.swing_arm();
                        }
                        return;
                    }
                    BlockUse::Fehlgeschlagen => return,
                    // PASS -- weiter zu Schritt 2, genau wie Vanillas durchfallender
                    // `case BLOCK:`.
                    BlockUse::Durchgereicht => {}
                }
            }

            // SCHRITT 2 -- interactItem. In Vanilla steht das UNTER der switch-Anweisung, nicht
            // darin: der `case BLOCK:`-Zweig kehrt nur bei Success oder Fail zurueck, sonst
            // faellt er hierher durch: ist der Stapel nicht leer, folgt interactItem.
            //
            // Das FEHLTE ebenfalls (Wassereimer liessen sich nicht platzieren): wir
            // haben entweder USE_ITEM_ON oder USE_ITEM geschickt, nie beides. BucketItem hat
            // aber gar kein useOnBlock -- die ganze Eimerlogik haengt an `use`, also am
            // zweiten Paket.
            //
            // LEERE HAND: hier schickt Vanilla GAR NICHTS und geht
            // gleich zur naechsten Hand. Der Ausstieg muss VOR der Sequenznummer stehen: sie
            // wird erst in sendSequencedPacket hochgezaehlt, also nur fuer ein Paket, das auch
            // rausgeht. Eine hier verbrannte Nummer risse eine Luecke in die Folge, auf die
            // GrimACs BadPacketsH schaut.
            let Some(item) = item else { continue };
            self.block_sequence = self.block_sequence.wrapping_add(1);
            let sequence = self.block_sequence;
            interaktions_log("use_item");
            if let Some(tx) = &self.outgoing_tx {
                let _ = tx.send(protocol::packets::use_item_payload(hand, sequence, self.camera.yaw, self.look_pitch_deg()));
            }
            // interactItem: `if (isCoolingDown(itemStack)) return PASS` -- NACH dem Paket,
            // weiter mit der naechsten Hand. Das ist der Schild-Fall: waehrend der fuenf
            // Sekunden nach einem Axthieb geht das Schild nicht hoch.
            if abkuehlend {
                continue;
            }
            // Nimmt `Item.use` an? Dann endet die Schleife hier -- die andere Hand kommt gar
            // nicht mehr zum Zug (s. item_registry::item_use_accepted).
            // FireworkRocketItem.use: gleitend SUCCESS (Schwung vom Client), sonst PASS.
            let rakete_gleitend = item == "firework_rocket" && self.physics.gliding;
            let angenommen = match stack.as_ref() {
                Some(st) => world::item_behavior::item_use_accepted(st, treffer.is_some()),
                None => world::item_registry::item_use_accepted(item, treffer.is_some()),
            };
            if !angenommen && !rakete_gleitend {
                continue;
            }
            // WritableBookItem.use oeffnet den Editor schon im Client (LocalPlayer.openItemGui),
            // sofern das Buch seine Seiten-Component hat.
            if item == "writable_book" {
                if let Some(st) = stack.as_ref() {
                    if st.writable_pages.is_some() || !st.removed_components.contains(&world::item_components::COMPONENT_WRITABLE_BOOK_CONTENT) {
                        self.open_book_editor(hand);
                    }
                }
            }
            // TridentItem.use: FAIL (weiter zur naechsten Hand, keine Benutzung), wenn der
            // Dreizack beim naechsten Einsatz zerbraeche -- oder wenn er Sog traegt und man weder
            // im Wasser noch im Regen steht. Das USE_ITEM ist da schon raus, genau wie in Vanilla.
            if item == "trident" {
                if let Some(st) = stack.as_ref() {
                    if will_break_next_use(st) || (self.trident_spin_strength(st) > 0.0 && !self.touching_water_or_rain()) {
                        continue;
                    }
                }
            }
            // BowItem.use / CrossbowItem.use: ohne Munition (und nicht im Kreativmodus) FAIL --
            // keine Benutzung, also auch keine Spannanimation. Sonst liesse sich der Bogen
            // ohne Pfeile spannen. Eine GELADENE Armbrust schiesst, da zaehlt nichts.
            let spannt = item == "bow" || (item == "crossbow" && !stack.as_ref().is_some_and(|st| st.is_charged()));
            if spannt && self.game_mode != GAME_MODE_CREATIVE && self.projectile_for(item).is_none() {
                self.arrow_empty_at = Some(Instant::now());
                continue;
            }
            // Consumable.canConsume: Essen mit FoodComponent nur, wenn der Hungerbalken nicht
            // voll ist (PlayerEntity.canConsume -> canAlwaysEat || abilities.invulnerable ||
            // hungerManager.isNotFull). Sonst FAIL -- keine Essanimation, keine Verlangsamung,
            // und wie bei jedem FAIL darf die andere Hand ran. Sonst koennte man satt immer
            // weiteressen. Eine vom Server GESETZTE food-Component (Golden Heads) gilt vor dem
            // Item-Standard, eine entfernte heisst: keine Hunger-Pruefung.
            let braucht_hunger = stack.as_ref().map_or_else(|| world::item_registry::needs_hunger(item), world::item_behavior::needs_hunger);
            let unverwundbar = self.game_mode == GAME_MODE_CREATIVE || self.game_mode == GAME_MODE_SPECTATOR;
            if braucht_hunger && self.food_level >= 20 && !unverwundbar {
                continue;
            }
            // Ein Item mit Benutzungsanimation laeuft ab jetzt DURCH -- bis die Taste
            // losgelassen wird. Vanilla schickt in der Zeit kein weiteres USE_ITEM und daempft
            // die Bewegung. Das gehoert hierher und nicht an den Anfang: Vanilla setzt die
            // laufende Hand erst in `ItemStack.use`, also INNERHALB von interactItem und nur
            // bei ANGENOMMENER Benutzung. Wer mit dem Bogen in der Hand eine Kiste oeffnet,
            // spannt ihn nicht.
            // Wasser-/Lavaeimer: Vanillas BucketItem.placeFluid setzt die Fluessigkeit schon
            // im Client (s. block_place::bucket_place).
            if let (Some(hit), Some(fluid)) = (treffer, bucket_fluid(item)) {
                let welt = &self.world;
                if let Some((pos, neu)) = block_place::bucket_place(hit.block, hit.face, fluid, self.sneak_held, |x, y, z| welt.block_at(x, y, z)) {
                    let vorher = self.world.block_at(pos.0, pos.1, pos.2);
                    self.world.set_pending_block(pos.0, pos.1, pos.2, neu);
                    self.push_pending_block(pos, sequence, vorher);
                    if item == "water_bucket" {
                        self.use_emptied_bucket = Some(sequence);
                    }
                    // BucketItem.playEmptyingSound: world.playSound(user, pos, ...) -- der
                    // Server schickt den Klang allen AUSSER dem Spieler, der ihn selbst spielt.
                    // Ohne das war Ausleeren stumm.
                    let mitte = DVec3::new(pos.0 as f64 + 0.5, pos.1 as f64 + 0.5, pos.2 as f64 + 0.5);
                    self.play_event_at(bucket_empty_sound(item), mitte, protocol::packets::SoundCategory::Blocks);
                }
            }
            // Eine GELADENE Armbrust schiesst nur (CrossbowItem.use: shootAll + CONSUME, OHNE
            // setCurrentHand) -- keine Dauerbenutzung. Sonst laedt die Armbrust beim
            // Durchdruecken nicht nach: mit using_item=true blieb die Wiederholung bis zum
            // Loslassen gesperrt; Vanilla schickt 4 Ticks spaeter das naechste USE_ITEM, und
            // das beginnt das Nachladen.
            let schiesst_nur = item == "crossbow" && stack.as_ref().is_some_and(|st| st.is_charged());
            let dauerhaft = stack.as_ref().map_or_else(|| world::item_registry::is_used_continuously(item), world::item_behavior::used_continuously);
            if dauerhaft && !schiesst_nur {
                self.using_item = true;
                self.eat_held_through = false;
                // Neue bzw. beendete Benutzung: der Zaehler der Ess-/Spannhaltung faengt neu an. Sonst
                // erbte ein Bissen, der im selben Tick auf den vorigen folgt, dessen Stand (der
                // Apfel sprang fuer ein Bild zurueck und stand danach sofort am Mund).
                self.own_use_ticks = 0;
                self.using_offhand = hand == protocol::packets::HAND_OFF;
                self.using_item_id = stack.as_ref().map(|st| st.item_id);
                // KineticWeaponComponent.playSound in Item.use -- auch im Client.
                if stack.as_ref().is_some_and(|st| world::item_behavior::kinetic(st).is_some()) {
                    self.play_own_event(if item == "wooden_spear" { "item.spear_wood.use" } else { "item.spear.use" });
                }
            } else if let Some(ticks) = stack.as_ref().and_then(item_cooldowns::use_cooldown_ticks) {
                // ItemStack.use -> applyRemainderAndCooldown: bei SOFORT wirkenden Items
                // (Enderperle, Windkugel) setzt schon der Client die Abklingzeit. Bei
                // Dauerbenutzung (Chorusfrucht) erst beim Aufessen -- die meldet der Server.
                if let Some(group) = stack.as_ref().and_then(item_cooldowns::group_of) {
                    self.item_cooldowns.set(&group, ticks);
                }
            }
            // Der Schwung haengt daran, ob `Item.use` SUCCESS meldet -- CONSUME (Essen, Bogen,
            // Schild) nimmt zwar an, schwingt aber nicht. Mit einem Block im Fadenkreuz
            // schwingen zusaetzlich die gefuellten Eimer mit, ihr eigener Strahl trifft
            // denselben Block wie unserer (s. block_hit_use_swings).
            let schwingt = rakete_gleitend
                || match (stack.as_ref(), treffer.is_some()) {
                    (Some(st), true) => world::item_behavior::block_hit_use_swings(st),
                    (Some(st), false) => world::item_behavior::air_use_swings(st),
                    (None, true) => world::item_registry::block_hit_use_swings(item),
                    (None, false) => world::item_registry::air_use_swings(item),
                };
            if schwingt {
                self.swing_arm();
            }
            // Wurf-Items nimmt schon der Client aus der Hand (ItemStack.consume steht in ihrem
            // `use` ausserhalb des Server-Zweigs; Raketen dagegen nur beim Server). Ohne das
            // stand ein Wurftrank bis zur Serverantwort noch in der Haupthand, und der zweite
            // Klick eines Doppelklicks warf ihn dort ein zweites Mal statt den aus der
            // Zweithand -- beim Server ging er ins Leere.
            if THROWN_ITEMS.contains(&item) {
                self.consume_hand_item(hand);
            }
            // Anziehen per Rechtsklick ebenso schon im Client (s. predict_equip_swap). Sonst
            // galt eine gerade angezogene Elytra bis zur Serverantwort als nicht getragen, und
            // ein Sprung in dieser Zeit startete keinen Gleitflug.
            if stack.as_ref().is_some_and(world::item_behavior::equips_on_use) {
                self.predict_equip_swap(hand);
            }
            return;
        }
    }


    // Die Blockbenutzung ausfuehren (soweit vorhersagbar) und beantworten, was Vanillas
    // interactBlockInternal daraus gemacht haette. Das Ergebnis entscheidet ZWEI Dinge: ob der
    // Arm schwingt, und ob danach noch ein USE_ITEM hinterhergeht (s. do_item_use).
    //
    // Die Reihenfolge ist Vanillas interactBlockInternal:
    //   1. Wer SCHLEICHT und etwas in der Hand hat, ueberspringt die Blockbenutzung ganz
    //      (`shouldCancelInteraction() && irgendeine Hand voll`) -- so setzt man einen Block
    //      auf eine Falltuer, statt sie aufzuklappen.
    //   2. Sonst die Benutzung des Blocks selbst (onUseWithItem/onUse).
    //   3. Erst wenn die nicht greift, `itemStack.useOnBlock` -- das Platzieren aus der Hand
    //      und die Items, die auf einem Block wirken.
    //
    // Schritt 2 stand hier frueher erst NACH dem Platzieren, und das war ein echter Fehler:
    // wer mit einem Block in der Hand auf eine Kiste klickt, oeffnet in Vanilla die Kiste
    // (onUse liefert SUCCESS und kehrt zurueck) und platziert NICHTS. Wir haben stattdessen
    // zuerst platziert -- der Block flackerte neben der Kiste auf, bis der Ack ihn zuruecknahm.
    pub(crate) fn resolve_block_use(
        &mut self,
        block: (i32, i32, i32),
        face: i32,
        cursor_y: f32,
        sequence: i32,
        item: Option<&'static str>,
        abkuehlend: bool,
        hand: i32,
    ) -> BlockUse {
        let (haupt, zweit) = self.own_hand_items();
        let ueberspringen = self.sneak_held && (haupt.is_some() || zweit.is_some());
        let haupthand = hand == protocol::packets::HAND_MAIN;

        // --- Schritt 2: der Block selbst -------------------------------------------------
        // Vanillas interactBlockInternal: erst onUseWithItem mit DIESER Hand; nur wenn das
        // PASS_TO_DEFAULT_BLOCK_ACTION liefert UND es die Haupthand ist, kommt onUse. Die
        // Nebenhand erreicht onUse also nie.
        if !ueberspringen {
            match self.resolve_anchor_use(block, sequence, item, hand) {
                AnkerBenutzung::KeinAnker | AnkerBenutzung::Standard => {}
                AnkerBenutzung::Ergebnis(ergebnis) => return ergebnis,
                // PASS: onUse entfaellt, weiter mit dem Item dieser Hand.
                AnkerBenutzung::NurItem => return self.use_item_on_block(block, face, cursor_y, sequence, item, abkuehlend, hand),
            }
            // Kupfergolem-Statue: onUseWithItem mit dieser Hand, also auch die Nebenhand.
            if let Some(neu) = block_place::next_statue_pose_state(self.world.block_at(block.0, block.1, block.2), item) {
                self.predict_block_state(block, neu, sequence);
                return BlockUse::Angenommen { schwingt: true };
            }
            if haupthand {
                if self.predict_openable_toggle(block, sequence) {
                    return BlockUse::Angenommen { schwingt: true };
                }
                // Die uebrigen benutzbaren Bloecke: Kiste, Ofen, Hebel, Bett... Vorhersagen
                // laesst sich davon nichts (der Server oeffnet den Bildschirm), aber der Arm
                // schwingt.
                let name = world::block_name(self.world.block_at(block.0, block.1, block.2));
                if name.is_some_and(block_place::is_hand_interactive) {
                    return BlockUse::Angenommen { schwingt: true };
                }
            }
        }
        self.use_item_on_block(block, face, cursor_y, sequence, item, abkuehlend, hand)
    }

    // Schritt 3 von interactBlockInternal: itemStack.useOnBlock.
    pub(crate) fn use_item_on_block(
        &mut self,
        block: (i32, i32, i32),
        face: i32,
        cursor_y: f32,
        sequence: i32,
        item: Option<&'static str>,
        abkuehlend: bool,
        hand: i32,
    ) -> BlockUse {

        // --- Schritt 3: itemStack.useOnBlock ---------------------------------------------
        // Leere Hand oder Abklingzeit:
        // `if (!itemStack.isEmpty() && !isCoolingDown(itemStack)) ... else return PASS`.
        let Some(item) = item.filter(|_| !abkuehlend) else { return BlockUse::Durchgereicht };
        // ItemStack.useOn: ohne Baurecht (Abenteuer) PASS, solange can_place_on den
        // ANGEKLICKTEN Block nicht nennt -- dann geht wie in Vanilla das USE_ITEM hinterher.
        // Vorher sagten wir dort Bloecke vorher, die der Server sofort wieder wegnahm.
        if self.game_mode == GAME_MODE_ADVENTURE {
            let state = self.world.block_at(block.0, block.1, block.2);
            let preds = self.own_hand_stack(hand).and_then(|s| s.can_place_on.as_deref());
            if !world::item_behavior::adventure_allows(preds, state) {
                return BlockUse::Durchgereicht;
            }
        }
        // BlockItem.useOnBlock kennt kein PASS: entweder der Block steht (SUCCESS) oder die
        // Platzierung war unmoeglich (FAIL). Beides beendet die Benutzung -- ein USE_ITEM
        // hinterher waere ein Paket, das Vanilla nie schickt.
        if world::block_registry::is_block(item) {
            return if self.predict_block_place(block, face, cursor_y, sequence, item) {
                // BlockItem.place: `itemStack.decrementUnlessCreative(1, player)` -- auch im
                // Client (s. consume_hand_item).
                self.consume_hand_item(hand);
                self.use_placed_block = true;
                BlockUse::Angenommen { schwingt: true }
            } else {
                BlockUse::Fehlgeschlagen
            };
        }
        // Items, die auf einem Block wirken, ohne einen zu setzen: Feuerzeug, Hacke, Schaufel,
        // Spawn-Ei. NICHT die Fluessigkeitseimer -- BucketItem hat gar kein useOnBlock, seine
        // ganze Wirkung haengt an `use`, also am durchgereichten zweiten Paket.
        // FireworkRocketItem.useOnBlock: gleitend PASS -- dann geht das USE_ITEM hinterher, und
        // DAS zuendet die Rakete am Spieler statt einer am Boden.
        if item == "firework_rocket" && self.physics.gliding {
            return BlockUse::Durchgereicht;
        }
        if world::item_registry::acts_on_block(item) {
            // Pulverschnee loescht wie Wasser (PowderSnowBlock.entityInside: EXTINGUISH).
            if item == "powder_snow_bucket" {
                self.use_emptied_bucket = Some(sequence);
            }
            return BlockUse::Angenommen { schwingt: true };
        }
        BlockUse::Durchgereicht
    }

    // Den Arm schwingen -- lokal UND fuer die anderen Spieler. Beides gehoert zusammen:
    // wer nur das Paket schickt, sieht seinen eigenen Arm nicht bewegen, und wer nur den
    // Zaehler setzt, schwingt fuer alle anderen unsichtbar (der Server schickt den eigenen
    // Schwung nicht an einen selbst zurueck, s. ClientEvent::EntityAnimation).
    pub(crate) fn swing_arm(&mut self) {
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::swing_payload());
        }
        self.own_swing_ticks = restart_swing(self.own_swing_ticks, self.own_swing_duration());
    }

    // Vanillas PendingUpdateManager.addPendingUpdate: pro POSITION genau ein Eintrag. Sagt man
    // dieselbe Stelle ein zweites Mal vorher (Falltuer schnell auf und wieder zu), wird NUR die
    // Sequenznummer hochgezogen -- der gemerkte Zustand bleibt der von VOR der ersten
    // Vorhersage (`compute(... pendingUpdate.withSequence(sequence) ...)`). Zwei Eintraege auf
    // derselben Stelle wuerden beim Ack nacheinander angewendet und koennten die Reihenfolge
    // verdrehen.
    pub(crate) fn push_pending_block(&mut self, pos: (i32, i32, i32), sequence: i32, aufloesung: u32) {
        if let Some(vorhanden) = self.pending_blocks.iter_mut().find(|p| p.pos == pos) {
            vorhanden.sequence = sequence;
            return;
        }
        self.pending_blocks.push(PendingBlock { pos, sequence, aufloesung, entfernt: None, placed_at: std::time::Instant::now() });
    }

    // Tuer/Falltuer/Zauntor SOFORT lokal umklappen, statt auf das Block-Update des Servers zu
    // warten -- s. block_place::is_hand_openable fuer das Warum (Vanilla und GrimAC machen
    // beide genau das, unser Warten kostete eine volle Rundreise mit falscher Blockform).
    // Rueckgabe: true, wenn die Benutzung als Blockbenutzung durchging (dann wird nichts mehr
    // platziert).
    //
    // Anders als beim Platzieren landet der neue Zustand in den ECHTEN Chunk-Daten, nicht in
    // der Vorhersage-Schicht: Vanilla setzt ihn auch reguler (world.setBlockState in
    // TrapdoorBlock.flip), die Falltuer soll sofort sichtbar klappen und nicht eine Rundreise
    // lang als halbdurchsichtiger Geisterblock haengen. Zurueckgenommen wird er trotzdem --
    // ueber denselben PendingBlock-Eintrag wie eine Platzierung (s. resolve_pending_blocks).
    pub(crate) fn predict_openable_toggle(&mut self, hit_block: (i32, i32, i32), sequence: i32) -> bool {
        let (x, y, z) = hit_block;
        let state = self.world.block_at(x, y, z);
        let Some(name) = world::block_name(state) else { return false };
        if !block_place::is_hand_openable(name) {
            return false;
        }
        // Vanilla: `bl2 = shouldCancelInteraction() && (Haupthand oder Nebenhand voll)` --
        // schleichend MIT etwas in der Hand wird der Block uebersprungen und stattdessen
        // platziert. Mit leeren Haenden oeffnet auch ein schleichender Klick.
        let haende_voll =
            self.main_hand_item_name().is_some() || self.inventory.player.get(inventory::PLAYER_OFFHAND).is_some_and(|s| s.is_some());
        if self.sneak_held && haende_voll {
            return false;
        }
        let Some(neu) = block_place::toggled_open_state(state) else { return false };
        self.predict_block_state((x, y, z), neu, sequence);
        // Tuer: beide Haelften, s. block_place::door_other_half.
        if let Some((ox, oy, oz)) = block_place::door_other_half(name, hit_block, state) {
            let andere = self.world.block_at(ox, oy, oz);
            if world::block_name(andere) == Some(name) {
                if let Some(neu_andere) = block_place::toggled_open_state(andere) {
                    self.predict_block_state((ox, oy, oz), neu_andere, sequence);
                }
            }
        }
        true
    }

    // Seelenanker: die vollstaendige Fallunterscheidung aus RespawnAnchorBlock, weil er der
    // eine Block ist, dessen Annahme wirklich vom ZUSTAND abhaengt (s. den Kommentar bei
    // block_place::is_hand_interactive, wo er deshalb fehlt). None = der Anker nimmt die
    // Benutzung nicht an, der Aufrufer macht mit itemStack.useOnBlock weiter.
    //
    //   onUseWithItem: Glowstone + charges < 4  -> charge(), SUCCESS  (Schwung vom Client)
    //                  sonst                    -> PASS_TO_DEFAULT_BLOCK_ACTION
    //   onUse:         charges == 0             -> PASS               (also platzieren!)
    //                  charges > 0              -> auf dem CLIENT immer CONSUME
    //
    // Das CONSUME im letzten Fall ist ein `Success` mit SwingSource.NONE -- angenommen, aber
    // OHNE eigenen Schwung. Der Server entscheidet dort unabhaengig (Spawn setzen bzw. in der
    // Oberwelt explodieren) und schickt bei SUCCESS_SERVER seinerseits das Animate-Paket;
    // wer hier zusaetzlich selbst schwaenge, schwaenge doppelt.
    //
    // Frueherer Fehler: Anker liessen sich nicht laden, es landete nur Glowstone daneben.
    // Der Anker hatte gar keine eigene Behandlung -- mit Glowstone in der Hand lief
    // er in den Platzierungs-Zweig, und das Glowstone landete neben dem Anker.
    //
    // Die Hand zaehlt (onUseWithItem bekommt den Stapel DIESER Hand): Glowstone in der
    // Nebenhand laedt ebenfalls -- im Haupthand-Durchgang kommt dann PASS (onUse entfaellt,
    // das Haupthand-Item darf trotzdem useOnBlock), im Nebenhand-Durchgang wird geladen.
    //
    // Und ein gerade erst VORHERGESAGTER Anker laesst sich sofort laden, wie in Vanilla, wo er
    // schon in der Welt steht (s. predict_block_state). Sonst landet
    // die Ladung unter dem noch unbestaetigten Anker und bleibt bis
    // zur Server-Antwort unsichtbar.
    pub(crate) fn resolve_anchor_use(&mut self, hit_block: (i32, i32, i32), sequence: i32, item: Option<&str>, hand: i32) -> AnkerBenutzung {
        let (x, y, z) = hit_block;
        let state = self.world.block_at(x, y, z);
        if world::block_name(state) != Some("respawn_anchor") {
            return AnkerBenutzung::KeinAnker;
        }
        let Some(charges) = world::state_props::property(state, "charges").and_then(|c| c.parse::<u8>().ok()) else {
            return AnkerBenutzung::KeinAnker;
        };
        let ladbar = charges < block_place::ANCHOR_MAX_CHARGES;
        // onUseWithItem: Glowstone dieser Hand + ladbar -> charge() + SUCCESS.
        if item == Some("glowstone") && ladbar {
            if let Some(neu) = block_place::charged_anchor_state(state) {
                self.predict_block_state(hit_block, neu, sequence);
                self.consume_hand_item(hand);
                return AnkerBenutzung::Ergebnis(BlockUse::Angenommen { schwingt: true });
            }
        }
        if hand != protocol::packets::HAND_MAIN {
            // Nebenhand: onUse gibt es fuer sie nicht.
            return AnkerBenutzung::Standard;
        }
        let nebenhand_glowstone = self.own_hand_stack(protocol::packets::HAND_OFF).and_then(|s| world::item_registry::item_name(s.item_id)) == Some("glowstone");
        if nebenhand_glowstone && ladbar {
            return AnkerBenutzung::NurItem;
        }
        // onUse: ohne Ladung PASS (weiter zum Item, man kann also dagegen bauen), sonst im
        // Client CONSUME -- angenommen, ohne eigenen Schwung.
        if charges == 0 {
            return AnkerBenutzung::Standard;
        }
        // Ausserhalb des Nethers sprengt der Server den Anker weg (im Nether setzt er nur den
        // Spawnpunkt, der Anker bleibt stehen) -- s. tick_item_use, anchor_wait.
        if self.dimension_type_name != "minecraft:the_nether" {
            self.use_detonated_anchor = Some(hit_block);
            // Fuer den Ghostblock-Indikator: der Anker wandert aus den Chunkdaten in die
            // Vorhersage-Schicht. Strahl und Kollision sehen ihn dort unveraendert (der
            // Airplace und anchor_wait haengen daran), gezeichnet wird er aber als Geist --
            // deckend bis zur Toleranz, danach halbdurchsichtig, bis das Block-Update des
            // Servers ihn wegnimmt. Explodiert er doch nicht, stellt der Ack ihn zurueck
            // (Aufloesung = der Anker selbst).
            if self.world.pending_block_at(x, y, z).is_none() {
                self.world.set_block(x, y, z, world::STATE_AIR);
                self.world.set_pending_block(x, y, z, state);
            }
            self.push_pending_block(hit_block, sequence, state);
        }
        AnkerBenutzung::Ergebnis(BlockUse::Angenommen { schwingt: false })
    }

    // Einen Blockzustand vorhersagen (Umklappen, Anker laden) -- in DERSELBEN Schicht, in der
    // die Stelle gerade steht: ist sie selbst noch eine unbestaetigte Platzierung, wird die
    // Vorhersage weitergeschrieben, sonst die Chunkdaten. Vorher ging es immer in die
    // Chunkdaten, und die Vorhersage-Schicht darueber (sie hat Vorrang) verdeckte die
    // Aenderung bis zur Server-Antwort.
    pub(crate) fn predict_block_state(&mut self, pos: (i32, i32, i32), neu: u32, sequence: i32) {
        let (x, y, z) = pos;
        let vorher = self.world.block_at(x, y, z);
        if self.world.pending_block_at(x, y, z).is_some() {
            self.world.set_pending_block(x, y, z, neu);
        } else {
            self.world.set_block(x, y, z, neu);
        }
        // Existiert schon ein Eintrag, zieht das nur die Sequenz hoch (s. push_pending_block).
        self.push_pending_block(pos, sequence, vorher);
    }

    // Den Stapel dieser Hand um eins verringern, wie Vanillas decrementUnlessCreative -- das
    // laeuft im Client mit (BlockItem.place, RespawnAnchorBlock.onUseWithItem). Ohne das
    // sagte der Client mit dem LETZTEN Block/Glowstone weitere Klicks voraus, die es nie gab;
    // die Zahl korrigierte erst das Slot-Update des Servers.
    // Equippable.swapWithEquipmentSlot: Vanilla tauscht schon im Client, nur die Statistik zaehlt
    // der Server. Gegen Fluch der Bindung (ausser kreativ) und gegen das gleiche Stueck tut sich
    // nichts. Ein einzelnes Stueck wandert in den Ruestungsplatz und das alte in die Hand; im
    // Kreativmodus bleibt die Hand voll, solange vorher nichts angezogen war. Gestapelte
    // Ruestung (nur per Server-Component) bleibt dem Server ueberlassen -- dort muesste das alte
    // Stueck erst einen freien Platz finden.
    pub(crate) fn predict_equip_swap(&mut self, hand: i32) {
        let hand_slot = if hand == protocol::packets::HAND_OFF {
            inventory::PLAYER_OFFHAND
        } else {
            inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8)
        };
        let Some(stack) = self.inventory.player.get(hand_slot).cloned().flatten() else { return };
        let Some(ziel) = world::item_behavior::equip_slot_on_use(&stack).filter(|_| stack.count == 1) else { return };
        let ruestung = inventory::PLAYER_ARMOR_START + (5 - ziel) as usize;
        let alt = self.inventory.player.get(ruestung).cloned().flatten();
        let kreativ = self.game_mode == GAME_MODE_CREATIVE;
        if let Some(a) = &alt {
            if (self.enchantments.prevents_armor_change(a) && !kreativ) || a.stackable_with(&stack) {
                return;
            }
        }
        let hand_danach = match (&alt, kreativ) {
            (None, true) => Some(stack.clone()),
            _ => alt,
        };
        if let Some(platz) = self.inventory.player.get_mut(ruestung) {
            *platz = Some(stack);
        }
        if let Some(platz) = self.inventory.player.get_mut(hand_slot) {
            *platz = hand_danach;
        }
    }

    pub(crate) fn consume_hand_item(&mut self, hand: i32) {
        if self.game_mode == GAME_MODE_CREATIVE {
            return;
        }
        let slot = if hand == protocol::packets::HAND_OFF {
            inventory::PLAYER_OFFHAND
        } else {
            inventory::PLAYER_HOTBAR_START + self.inventory.held_slot.min(8)
        };
        if let Some(eintrag) = self.inventory.player.get_mut(slot) {
            if let Some(stack) = eintrag.as_mut() {
                stack.count -= 1;
                if stack.count <= 0 {
                    *eintrag = None;
                }
            }
        }
    }

    // "Bett verlassen": ClientCommandC2SPacket mit STOP_SLEEPING. Vanilla haengt das an den
    // Knopf im Schlaf-Bildschirm; wir an die Schleichtaste. Der Zustand selbst wird NICHT
    // lokal zurueckgesetzt -- er gilt erst als beendet, wenn der Server die geleerte
    // SLEEPING_POSITION zurueckschickt. Sonst liefen Client und Server auseinander, falls der
    // Server das Aufstehen ablehnt (z.B. Monster in der Naehe).
    pub(crate) fn leave_bed(&mut self) {
        let (Some(tx), Some(id)) = (&self.outgoing_tx, self.own_entity_id) else { return };
        let _ = tx.send(protocol::packets::player_command_stop_sleeping_payload(id));
    }

    // Den gerade abgeschickten Platzierungsversuch LOKAL vorwegnehmen -- Vanillas
    // ClientPlayerInteractionManager.interactBlock, das die Benutzung im Client ausfuehrt und
    // im PendingUpdateManager den vorherigen Zustand samt Sequenznummer hinterlegt.
    //
    // Der Block landet dabei NICHT in den Chunk-Daten, sondern in der Vorhersage-Schicht der
    // Welt (world::World::set_pending_block): Kollision und Strahl sehen ihn sofort, der
    // Mesher nicht -- gezeichnet wird er halbdurchsichtig, bis der Server ihn bestaetigt
    // (s. ghost_vertices).
    // Rueckgabe: waere hier ein Block gesetzt worden? Das ist NICHT dasselbe wie "wurde etwas
    // vorhergesagt" -- vorhergesagt wird nur bei eindeutiger Ausrichtung, gesetzt wird auch
    // eine Treppe. Fuer den Armschwung zaehlt die Platzierung, nicht die Vorhersage.
    pub(crate) fn predict_block_place(&mut self, hit_block: (i32, i32, i32), face: i32, cursor_y: f32, sequence: i32, item: &'static str) -> bool {
        if !world::block_registry::is_block(item) {
            return false;
        }
        let getroffen = world::block_name(self.world.block_at(hit_block.0, hit_block.1, hit_block.2));
        let ziel = block_place::target_pos(hit_block, face, getroffen);
        // Nur auf einen ersetzbaren Platz -- steht dort schon etwas Festes, hat der Server
        // ganz andere Regeln (Kiste oeffnen, Knopf druecken), und wir raten nicht.
        let vorher = self.world.block_at(ziel.0, ziel.1, ziel.2);
        if !world::block_name(vorher).is_some_and(block_place::is_replaceable) {
            return false;
        }
        // Ab hier steht fest, dass Vanilla platziert. Ob wir es auch VORHERSAGEN, haengt
        // daran, ob die Ausrichtung eindeutig ist -- mehrstatige Bloecke (Treppen, Zaeune)
        // setzt der Server allein, der Arm schwingt trotzdem.
        let truhe_daneben = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|(dx, dz)| block_place::is_single_chest(self.world.block_at(ziel.0 + dx, ziel.1, ziel.2 + dz)));
        let state = block_place::torch_state(item, face)
            .or_else(|| block_place::facing_state(item, self.camera.yaw))
            .or_else(|| block_place::facing_waterlogged_state(item, self.camera.yaw, block_place::is_water_source(vorher), truhe_daneben))
            .unwrap_or_else(|| block_place::axis_state(item, face).or_else(|| block_place::default_state_for_item(item)));
        // Ein Block IM Spieler wird in Vanilla gar nicht erst gesetzt -- wuerden wir ihn
        // vorhersagen, schoebe unsere eigene Kollision uns weg und die Abweichung zum Server
        // waere groesser statt kleiner.
        let fuss = self.physics.pos;
        let min = [fuss.x - self.physics.width() / 2.0, fuss.y, fuss.z - self.physics.width() / 2.0];
        let max = [
            fuss.x + self.physics.width() / 2.0,
            fuss.y + self.physics.current_height(),
            fuss.z + self.physics.width() / 2.0,
        ];
        let Some(state) = state else {
            // Ausrichtung unbekannt: steckte der Block in jeder Form im Spieler, setzt Vanilla
            // sicher nichts.
            // Trefferhoehe ueber der Zielzelle wie clickLocation.y - pos.y.
            let ort = block_place::PlaceSpot { face, rel_y: (hit_block.1 - ziel.1) as f32 + cursor_y, yaw_deg: self.camera.yaw, vorher };
            if block_place::every_state_collides(item, ziel, &ort, &self.block_shapes, min, max) {
                return false;
            }
            self.play_place_sound(ziel, item);
            return true;
        };
        if block_place::collides_with_player(ziel, state, &self.block_shapes, min, max) {
            return false;
        }
        self.world.set_pending_block(ziel.0, ziel.1, ziel.2, state);
        self.play_place_sound(ziel, item);
        // Was beim Ack gilt, wenn bis dahin NICHTS vom Server kam: der alte Zustand. Genau
        // Vanillas addPendingUpdate(pos, alterState) -- eine abgelehnte Platzierung nimmt sich
        // damit von selbst zurueck, ohne dass der Server sie widerrufen muesste.
        self.push_pending_block(ziel, sequence, vorher);
        true
    }

    // Server-Blockupdate auf einer Stelle, fuer die noch eine Vorhersage offen ist: Vanillas
    // PendingUpdateManager.hasPendingUpdate merkt sich den Serverzustand und wendet ihn NICHT
    // sofort an -- sonst flackerte der Block zwischen Vorhersage und Serverstand hin und her,
    // solange noch Ack und Update unterwegs sind. Gibt true zurueck, wenn das Update
    // aufgehoben wurde.
    pub(crate) fn defer_block_update(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
        let Some(pending) = self.pending_blocks.iter_mut().find(|p| p.pos == (x, y, z)) else {
            return false;
        };
        pending.aufloesung = state;
        true
    }

    // Block Changed Ack: alles bis EINSCHLIESSLICH dieser Sequenznummer ist beim Server durch
    // (Vanillas PendingUpdateManager.processPendingUpdates). Die aufgehobene Aufloesung wird
    // jetzt in die echten Chunk-Daten geschrieben und die Vorhersage faellt weg -- ab da
    // zeichnet der normale Mesher den Block wieder undurchsichtig.
    // Die bestaetigten Geister einer gerade fertig gemeshten Spalte fallenlassen -- ab jetzt
    // zeichnet der gewoehnliche Chunk-Pass den Block undurchsichtig.
    pub(crate) fn drop_ghosts_of_column(&mut self, column: (i32, i32)) {
        // Nur, wenn fuer diese Spalte auch wirklich nichts mehr aussteht. Sonst koennte ein
        // ALTES Ergebnis (Job war schon unterwegs, als der Ack kam) den Geist wegnehmen,
        // obwohl der gerade hochgeladene Mesh den Block noch gar nicht enthaelt -- genau das
        // Loch, das hier vermieden werden soll.
        if self.world.is_dirty(column.0, column.1) || self.mesh_jobs_pending.contains(&column) {
            return;
        }
        let welt = &mut self.world;
        let quellen = &mut self.ao_fade_sources;
        self.ghosts_awaiting_mesh.retain(|g| {
            if g.column != column {
                return true;
            }
            welt.clear_pending_block(g.pos.0, g.pos.1, g.pos.2);
            // Der Schatten dieses Blocks ist jetzt eingeblendet -- spaetere Meshes (z.B. ein
            // gleich aufgeladener Anker) sollen ihn nicht noch einmal einblenden.
            quellen.retain(|(p, _, _)| *p != g.pos);
            false
        });
    }

    // Notbremse: Geister, deren Mesh nie kam (Spalte entladen, Welt zurueckgesetzt), nach
    // GHOST_MESH_GRACE trotzdem wegnehmen.
    pub(crate) fn expire_stale_ghosts(&mut self) {
        if self.ghosts_awaiting_mesh.is_empty() {
            return;
        }
        let jetzt = std::time::Instant::now();
        let welt = &mut self.world;
        self.ghosts_awaiting_mesh.retain(|g| {
            if g.deadline > jetzt {
                return true;
            }
            welt.clear_pending_block(g.pos.0, g.pos.1, g.pos.2);
            false
        });
    }

    // Das Einblenden der Eckabdunklung einen Schritt weiter: je Quad linear von "vorher" nach
    // "nachher" (s. AoFade), am Ende genau der neue Mesh. Abgelaufene Quellen fallen weg.
    pub(crate) fn step_ao_fades(&mut self) {
        let jetzt = Instant::now();
        self.ao_fade_sources.retain(|(_, _, t)| jetzt.duration_since(*t) < AO_FADE_SOURCE_TTL);
        if self.ao_fades.is_empty() {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let mut puffer = Vec::new();
        self.ao_fades.retain(|f| {
            let a = (jetzt.duration_since(f.start).as_secs_f32() / AO_FADE.as_secs_f32()).min(1.0);
            for q in &f.quads {
                puffer.clear();
                puffer.extend(q.before.iter().zip(&q.after).map(|(v, n)| world::TerrainVertex {
                    color: std::array::from_fn(|i| (v.color[i] as f32 + (n.color[i] as f32 - v.color[i] as f32) * a).round() as u8),
                    ..*n
                }));
                gpu.block_renderer.write_column_vertices(&gpu.queue, f.column, q.start, &puffer);
            }
            a < 1.0
        });
    }

    pub(crate) fn resolve_pending_blocks(&mut self, sequence: i32) {
        if self.bucket_ack_wait.is_some_and(|(seq, _, _)| sequence.wrapping_sub(seq) >= 0) {
            self.bucket_ack_wait = None;
        }
        let mut faellig: Vec<PendingBlock> = Vec::new();
        self.pending_blocks.retain(|p| {
            // wrapping_sub: die Sequenz laeuft in i32 um; ein Vergleich auf die DIFFERENZ
            // haelt das aus, ein direkter `<=` nicht.
            if sequence.wrapping_sub(p.sequence) >= 0 {
                faellig.push(p.clone());
                false
            } else {
                true
            }
        });
        if faellig.is_empty() {
            return;
        }
        for p in faellig {
            let (x, y, z) = p.pos;
            let vorhergesagt = self.world.pending_block_at(x, y, z);
            // Der Zustand VOR der Platzierung -- fuers Einblenden des Schattens (s. AoFade).
            let vorher = self.world.chunk_block_at(x, y, z);
            self.world.set_block(x, y, z, p.aufloesung);
            // Bestaetigt der Server GENAU das, was wir vorhergesagt haben, bleibt der Geist
            // stehen, bis der neue Mesh da ist -- sonst klafft an der Stelle ein paar Frames
            // lang ein Loch (s. GhostAwaitingMesh). Weicht die Aufloesung ab (Platzierung
            // abgelehnt, Server setzt etwas anderes), muss der Geist SOFORT weg: ihn laenger
            // stehen zu lassen hiesse, einen Block zu zeigen, den es gar nicht gibt.
            if vorhergesagt == Some(p.aufloesung) {
                if self.settings.smooth_lighting_percent > 0 && vorher != p.aufloesung {
                    self.ao_fade_sources.push((p.pos, vorher, Instant::now()));
                }
                self.ghosts_awaiting_mesh.push(GhostAwaitingMesh {
                    pos: p.pos,
                    column: (x.div_euclid(16), z.div_euclid(16)),
                    deadline: std::time::Instant::now() + GHOST_MESH_GRACE,
                });
            } else {
                self.world.clear_pending_block(x, y, z);
            }
        }
    }

    // Der Sprint-Reset beim Zuschlagen ("W-Tap"). Vanillas PlayerEntity.attack ->
    // knockbackTarget(target, staerke, ...): bei Staerke ueber 0 bekommt das Ziel Rueckstoss,
    // die eigene Geschwindigkeit wird waagerecht mit 0.6 multipliziert und der Sprint endet.
    //
    // `strength` ist die Rueckstoss-Staerke: Grundwert 0 plus 0.5, WENN der Schlag ein
    // Sprint-Rueckstoss war -- also nur bei `isSprinting() && getAttackCooldownProgress(0.5)
    // > 0.9`. Ohne aufgeladenen Angriff (Klickspam) passiert nichts, und genau deshalb bleibt
    // beim schnellen Kristall-Klicken der Sprint stehen.
    //
    // Fuer WELCHE Ziele, entscheidet GrimAC (PacketPlayerAttack, im Bytecode des 2.3.74-Jars
    // nachgelesen): Spieler und Nicht-Lebewesen (Kristalle, Boote, Rahmen) zaehlen, jedes
    // andere Lebewesen (Dorfbewohner, Mobs, Ruestungsstaender) NICHT -- dort rechnet Grim gar
    // keine Daempfung ein. Frueher daempften wir bei jedem Ziel: ein Sprint-Schlag auf einen
    // Dorfbewohner war dann 40 % langsamer, als Grim erwartet (harte Simulation-Flags
    // beim Schlagen von Dorfbewohnern im Sprint).
    pub(crate) fn apply_attack_slowdown(&mut self, entity_id: i32) {
        if !self.physics.sprinting {
            return;
        }
        let zaehlt = self.entities.get(entity_id).is_none_or(|e| world::is_player(e.entity_type) || !world::is_living(e.entity_type));
        if !zaehlt {
            return;
        }
        let tempo = self.attack_speed_attribute();
        if !angriff_ist_aufgeladen(self.ticks_since_last_attack, tempo) {
            return;
        }
        self.physics.vel.x *= 0.6;
        self.physics.vel.z *= 0.6;
        self.physics.sprinting = false;
    }

    // Geometrie aller unbestaetigten Platzierungen -- gezeichnet mit halber Deckkraft
    // (render::BlockRenderer::ghost_pipeline), damit sichtbar ist, was noch Vorhersage und was
    // schon bestaetigt ist. Sobald der Ack da ist, wandert der Block in die Chunk-Daten und
    // der normale Mesher zeichnet ihn undurchsichtig.
    // AUGENRELATIV: die Vertices gehen mit Null-Versatz in den Zeichenaufruf (s.
    // render::BlockRenderer::render), muessen die Kameraposition also selbst herausrechnen --
    // und zwar in f64, sonst waere die Blockposition bei grossen Weltkoordinaten schon in
    // f32 zerfallen, bevor sie im Puffer landet (derselbe Grund wie bei ColumnOrigin).
    // Rueckgabe: Vertices, davon deckend gezeichnet die ersten `.1`, danach `.2` durchscheinende
    // (Wasser, Glas -- s. render::BlockRenderer::ghost_translucent_count), der Rest halbdurchsichtig.
    pub(crate) fn ghost_vertices(&self) -> (Vec<world::mesher::BlockVertex>, usize, usize) {
        let mut deckend = Vec::new();
        let mut durchscheinend = Vec::new();
        let mut durchsichtig = Vec::new();
        let eye = self.camera.position;
        let jetzt = std::time::Instant::now();
        let toleranz = Duration::from_millis(self.settings.ghost_tolerance_ms as u64);
        for (pos, state) in self.world.pending_blocks() {
            let base = (DVec3::new(pos.0 as f64, pos.1 as f64, pos.2 as f64) - eye).as_vec3();
            // Wasser und Glas brauchen ihre echte Transparenz -- in der deckenden Pipeline war
            // frisch platziertes Wasser undurchsichtig, bis der Chunk-Mesh kam.
            let name = world::block_name(state).unwrap_or("");
            let durchscheint = world::fluid_kind(state) == Some(world::FluidKind::Water) || world::mesher::is_glass(name);
            let ziel = if !self.ghost_is_solid(pos, jetzt, toleranz) {
                &mut durchsichtig
            } else if durchscheint {
                &mut durchscheinend
            } else {
                &mut deckend
            };
            // Biomfarbe fuers Wasser aus der Spalte des Blocks (wie im Chunk-Mesh).
            let biom = self
                .world
                .column(pos.0.div_euclid(16), pos.2.div_euclid(16))
                .map(|c| c.biome_at(pos.0.rem_euclid(16), pos.1, pos.2.rem_euclid(16)))
                .unwrap_or(0);
            let wasser = self.biome_palette.colors_of(biom).water;
            let block_at = |x: i32, y: i32, z: i32| self.world.block_at(x, y, z);
            ziel.extend(world::mesher::block_ghost_vertices_at(
                base.into(),
                pos,
                state,
                &self.block_uvs,
                &self.block_shapes,
                &self.cross_blocks,
                wasser,
                &block_at,
            ));
        }
        // Abgebaute, noch unbestaetigte Bloecke: nach der Toleranz halbdurchsichtig an ihrer
        // alten Stelle, solange dort nichts anderes steht (s. PendingBlock::entfernt).
        if self.settings.module_ghost_tolerance {
            for p in &self.pending_blocks {
                let Some(state) = p.entfernt else { continue };
                let (x, y, z) = p.pos;
                if jetzt.duration_since(p.placed_at) < toleranz
                    || self.world.pending_block_at(x, y, z).is_some()
                    || !(self.world.block_at(x, y, z) == world::STATE_AIR || world::fluid_kind(self.world.block_at(x, y, z)).is_some())
                {
                    continue;
                }
                let base = (DVec3::new(x as f64, y as f64, z as f64) - eye).as_vec3();
                let block_at = |x: i32, y: i32, z: i32| self.world.block_at(x, y, z);
                durchsichtig.extend(world::mesher::block_ghost_vertices_at(
                    base.into(),
                    p.pos,
                    state,
                    &self.block_uvs,
                    &self.block_shapes,
                    &self.cross_blocks,
                    [1.0, 1.0, 1.0],
                    &block_at,
                ));
            }
        }
        self.display_block_vertices(&mut deckend);
        let solid_count = deckend.len();
        let translucent_count = durchscheinend.len();
        deckend.append(&mut durchscheinend);
        deckend.append(&mut durchsichtig);
        (deckend, solid_count, translucent_count)
    }

    // Wird dieser Geisterblock wie ein ECHTER Block gezeichnet?
    //
    // Ja, solange die Vorhersage noch jung genug ist -- und immer, wenn der Server sie bereits
    // bestaetigt hat und nur noch der Chunk-Mesh aussteht (s. GhostAwaitingMesh): da ist
    // nichts mehr fraglich, halbdurchsichtig waere schlicht falsch.
    //
    // Ist der Ghostblock-Indikator aus, ist jede Vorhersage deckend wie in Vanilla -- nichts
    // wird je halbdurchsichtig.
    pub(crate) fn ghost_is_solid(&self, pos: (i32, i32, i32), jetzt: std::time::Instant, toleranz: Duration) -> bool {
        if self.ghosts_awaiting_mesh.iter().any(|g| g.pos == pos) {
            return true;
        }
        if !self.settings.module_ghost_tolerance {
            return true;
        }
        self.pending_blocks
            .iter()
            .find(|p| p.pos == pos)
            .is_some_and(|p| jetzt.duration_since(p.placed_at) < toleranz)
    }

    pub(crate) fn send_player_action(&mut self, action: i32) {
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::player_action_payload(action));
        }
    }

    // --- Reiten (s. vehicle.rs, world::riding) -------------------------------------------
}
