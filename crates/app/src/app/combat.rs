// Angreifen.

use crate::*;

impl App {
    // Move Player Pos+Rot -- einmal pro Physik-Tick, wie ein echter Vanilla-Client. Nur
    // waehrend einer bestehenden Verbindung sinnvoll; ohne outgoing_tx (nicht verbunden)
    // oder ausserhalb des Play-States laeuft die Physik trotzdem (freies Herumlaufen im
    // leeren/Test-Zustand), nur eben ohne dass der Server etwas davon erfaehrt.
    // Linksklick im World-Modus: Vanilla-Angriff. Raycast aus der Augenposition entlang
    // der Blickrichtung gegen die Entity-Hitboxen (exakt die gezeichneten Boxen, 3 Bloecke
    // Survival-Reichweite). Treffer -> Interact(ATTACK) + Swing, Luftschlag -> nur Swing
    // (Vanilla schwingt den Arm bei jedem Klick, sichtbar fuer andere Spieler).
    pub(crate) fn attack_click(&mut self) {
        self.queue_input_action(PendingInputAction::Attack);
    }

    // Der eigentliche Angriff, ausgefuehrt beim Leeren der Warteschlange im Tick (s.
    // flush_input_actions) -- der Raycast passiert also mit der Kamera zum Tickzeitpunkt,
    // wie in Vanilla, wo handleInputEvents() ebenfalls im Tick laeuft.
    pub(crate) fn send_attack(&mut self) {
        if self.outgoing_tx.is_none() {
            return;
        }
        if self.conn_status != ConnStatus::Connected {
            return;
        }
        // Zu wenig aufgeladen fuer das Item (minimum_attack_charge): GAR NICHTS, nicht einmal
        // ein Schwung (Player.cannotAttackWithItem in startAttack).
        if self.attack_refused(0) {
            return;
        }
        // Zuschauer: Linksklick auf einen Block teleportiert dorthin (s. spectator.rs). Ueber
        // einem Entity bleibt es Vanilla -- der Server setzt die Kamera in das Entity.
        if self.game_mode == GAME_MODE_SPECTATOR && self.spectator_teleport_click() {
            return;
        }
        // Speer in der Haupthand: Stich statt Schlag (MinecraftClient.doAttack, noch VOR dem
        // Blick auf das Ziel).
        if let Some(speer) = self.main_hand_item_name().filter(|_| self.main_hand_piercing()) {
            self.spear_stab(speer);
            return;
        }
        // Ursprung ist die HARTE Augenhoehe der Pose, NICHT die geglaettete Kamerahoehe.
        // Vanilla trennt das genauso: Entity.raycast() nimmt getCameraPosVec(), und das rechnet
        // mit getStandingEyeHeight() -- waehrend die Ansicht ueber Camera.cameraY laeuft (s.
        // App::camera_eye_height). Das ist kein Zufall: waehrend des Abtauchens/Aufstehens
        // klaeffen beide ein paar Ticks lang um bis zu 0.17 Bloecke auseinander, und getroffen
        // wird nach dem echten Kopf, nicht nach dem noch nachlaufenden Bild.
        // Ziel wie Vanillas crosshairTarget (s. crosshair_target): ein Block VOR dem Gegner
        // faengt den Schlag ab.
        let ziel = self.crosshair_target();
        if let CrosshairTarget::Block(pos, face) = ziel {
            // No Tool Break: der Klick verpufft ganz, ohne Schwung.
            if self.block_mining_guarded(pos) {
                return;
            }
            // doAttack, case BLOCK: attackBlock, und bl3 = "danach ist dort Luft".
            if self.world.block_at(pos.0, pos.1, pos.2) != world::STATE_AIR {
                self.attack_block(pos, face);
                if self.world.block_at(pos.0, pos.1, pos.2) == world::STATE_AIR {
                    self.attack_broke_block = true;
                }
            }
            self.swing_arm();
            return;
        }
        let Some(tx) = &self.outgoing_tx else { return };
        let target = match ziel {
            CrosshairTarget::Entity(id) => Some(id),
            _ => None,
        };
        if let Some(entity_id) = target {
            let _ = tx.send(protocol::packets::attack_entity_payload(entity_id, self.sneak_held));
        }
        let _ = tx.send(protocol::packets::swing_payload());
        // Session-Info: wer jetzt stirbt, zaehlt als eigener Kill -- der Getroffene selbst bzw.
        // beim Kristall jeder in Reichweite der Explosion.
        if let Some((id, e)) = target.and_then(|id| Some((id, self.entities.get(id)?))) {
            let pos = e.target_pos();
            match world::entity_type_name(e.entity_type) {
                Some("player") => self.session.engage(id, Instant::now()),
                Some("end_crystal") => self.session_engage_near(pos, session_info::CRYSTAL_RADIUS),
                _ => {}
            }
        }
        // Crystal-Optimizer (s. crystal_optimizer.rs): ein getroffener End-Kristall verschwindet
        // sofort, statt eine Rundreise lang auf das Entfernen-Paket zu warten -- aber nur, wenn
        // der Schlag ihn auch zerstoeren kann (Schwaeche).
        if let Some(entity_id) = target {
            let ist_kristall = self.entities.get(entity_id).is_some_and(|e| world::entity_type_name(e.entity_type) == Some("end_crystal"));
            if ist_kristall && self.settings.module_crystal_optimizer && !self.crystal_opted_out {
                let effekt = |id: i32| self.own_effects.get(&id).copied();
                let waffe = self.main_hand_item_name();
                if crystal_optimizer::can_destroy_crystal(waffe, effekt(crystal_optimizer::MOB_EFFECT_WEAKNESS), effekt(crystal_optimizer::MOB_EFFECT_STRENGTH)) {
                    self.entities.remove(&[entity_id]);
                }
            }
        }
        if let Some(entity_id) = target {
            self.apply_attack_slowdown(entity_id);
        }
        // resetTicksSince(): Vanillas ClientPlayerInteractionManager.attackEntity ruft es nach
        // JEDEM Angriff, auch wenn nichts getroffen wurde -- die Aufladung faengt also selbst
        // bei einem Schlag ins Leere von vorn an. (Beim Block-Schlag NICHT, s. oben.)
        self.ticks_since_last_attack = 0;
    }

    // Vanillas Attribut attack_speed: es folgt dem Item in der Hand erst im Spieler-Tick
    // (LivingEntity.detectEquipmentUpdates), also NACH den Eingaben. Im Tick eines Wechsels
    // rechnen Aufladung und Mindestladung deshalb noch mit dem Tempo des alten Items -- ein
    // Schwert (12,5 Ticks) laesst so sofort mit dem Speer stechen, der fuer sich bis zu 23
    // Ticks braucht (Lunge-Swap). last_attack_hand_item ist genau dieser Stand (s. Tickschleife).
    //
    // Grundwert und fremde Modifikatoren (Eile, Plugins) kommen vom Server: auf Servern mit
    // 1.8-Kampf ist der Grundwert hoch (anticheat-test: 20), jeder Schlag also sofort geladen.
    // Mit Vanillas fester 4.0 hielten wir Spam-Hits fuer ungeladen und liessen den Sprint-
    // Slowdown weg, den Grim erwartet (Simulation 0.06 je Treffer).
    pub(crate) fn attack_speed_attribute(&self) -> f32 {
        let item = self.last_attack_hand_item.map_or(4.0, world::item_registry::attack_speed) - 4.0;
        match self.attr_attack_speed {
            Some((a, b, c)) => ((a + item as f64) * (1.0 + b) * c) as f32,
            None => 4.0 + item,
        }
    }

    // Player.cannotAttackWithItem fuer die Haupthand: verlangt das Item eine Mindestladung
    // (minimum_attack_charge, Speere 1.0) und ist die Aufladung `voraus` Ticks spaeter noch
    // darunter? Ohne Ladungs-Bonus (+0.5) -- strenger als die Anzeige.
    pub(crate) fn attack_refused(&self, voraus: i32) -> bool {
        let Some(stack) = self.own_hand_stack(protocol::packets::HAND_MAIN) else { return false };
        let mindestens = world::item_behavior::minimum_attack_charge(stack);
        if mindestens <= 0.0 {
            return false;
        }
        let tempo = self.attack_speed_attribute();
        ((self.ticks_since_last_attack + voraus) as f32 / (20.0 / tempo)) < mindestens
    }

    // Speer-Stich -- MinecraftClient.doAttack mit PiercingWeaponComponent (die Mindestladung
    // prueft send_attack davor, s. attack_refused):
    //  - attackWithPiercingWeapon: PlayerAction STAB, beforePlayerAttack (Aufladung auf 0) und
    //    der Stich-Klang, den der Server uns als Ausloeser nicht schickt.
    //  - swingHand.
    // Kein Block-Abbau und kein Ziel: welche Entities in 2..4,5 Bloecken getroffen werden,
    // entscheidet der Server.
    pub(crate) fn spear_stab(&mut self, speer: &str) {
        if let Some(tx) = &self.outgoing_tx {
            let _ = tx.send(protocol::packets::player_action_payload(protocol::packets::PLAYER_ACTION_STAB));
        }
        self.ticks_since_last_attack = 0;
        self.play_own_event(if speer == "wooden_spear" { "item.spear_wood.attack" } else { "item.spear.attack" });
        self.swing_arm();
    }

    // Laeuft gerade der Ansturm mit einem Speer? Der bremst nicht und laesst sprinten
    // (UseEffectsComponent des Speers), fuer die Bewegung ist er also keine Benutzung.
    // LivingEntity.canGlide: irgendein getragenes Stueck mit glider in seinem Slot, das beim
    // naechsten Gebrauch nicht zerbricht (Ruestungsplaetze: Kopf, Brust, Beine, Fuesse).
    pub(crate) fn own_glider_equipped(&self) -> bool {
        (0..4).any(|i| {
            let slot_ordinal = 5 - i as u8;
            self.inventory.player.get(inventory::PLAYER_ARMOR_START + i).and_then(|s| s.as_ref()).is_some_and(|s| world::item_behavior::can_glide_with(s, slot_ordinal))
        })
    }

    // use_effects des gerade benutzten Items (LocalPlayer: Tempo-Faktor und Sprint beim
    // Benutzen) -- Speere erlauben beim Ansturm volles Tempo und Sprinten.
    pub(crate) fn own_use_effects(&self) -> protocol::item::UseEffects {
        let hand = if self.using_offhand { protocol::packets::HAND_OFF } else { protocol::packets::HAND_MAIN };
        match self.own_hand_stack(hand).filter(|_| self.using_item) {
            Some(stack) => world::item_behavior::use_effects(stack),
            None => protocol::item::UseEffects { can_sprint: false, interact_vibrations: true, speed_multiplier: world::item_behavior::DEFAULT_USE_SPEED },
        }
    }

    // --- Modul Eigenname (s. ego_name) ------------------------------------------------------
}
